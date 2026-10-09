//! Classes (ECMA-262 §15.7): declarations and expressions, the heritage,
//! the elements of the body (methods, accessors, fields, static blocks),
//! private names and the early errors of class bodies.
//!
//! Class code is strict mode code. The heritage and the computed keys
//! belong to the enclosing function (its `this`, `yield`, `await`); the
//! methods are functions; the field initializers belong to the synthetic
//! initializer functions of the class ([`crate::Class::instance_init`],
//! [`crate::Class::static_init`]), each parsed with a fresh context of
//! that function. Static blocks are var scopes in the static initializer.
//!
//! Private names: a use (`this.#x`, `#x in o`) resolves to the innermost
//! enclosing class that declares the name, also later in its body. The
//! parser keeps the declared names of each open class and the first use
//! of each name that is not resolved yet; when a class ends, its
//! unresolved uses move to the enclosing class (the smaller map into the
//! larger one, so the total cost is linear in the number of uses, up to a
//! logarithmic factor), and at the outermost class the first one in
//! source order is the early error (`AllPrivateIdentifiersValid`,
//! §15.7.7).

use std::collections::HashMap;

use swb_js_text::{CodeUnit, String16};

use super::functions::FunctionHead;
use super::{AwaitMode, Context, Entered, IdentUse, PResult, Parser};
use crate::ast::{
    Class, ClassElement, ClassElementKind, ClassId, ClassKey, ExprId, FunctionId, FunctionKind,
    List, PropertyKey, ScopeId, Span,
};
use crate::error::ParseError;
use crate::interner::{NameId, names};
use crate::messages;
use crate::scope::{BindingKind, ScopeKind};
use crate::token::TokenKind;

/// How a private name was declared, for the duplicate check (§15.7.1:
/// only a getter and a setter of the same placement may share a name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateDecl {
    /// A field or a method.
    Other,
    /// A getter (`true`: static).
    Getter(bool),
    /// A setter.
    Setter(bool),
    /// A getter and a setter.
    Pair,
}

/// The private names of one class being parsed.
#[derive(Debug, Default)]
pub(crate) struct PrivateNames {
    declared: HashMap<NameId, PrivateDecl>,
    /// The offset of the first use of each name that this class has not
    /// declared (yet).
    unresolved: HashMap<NameId, u32>,
}

impl PrivateNames {
    fn note_use(&mut self, name: NameId, offset: u32) {
        if !self.declared.contains_key(&name) {
            let first = self.unresolved.entry(name).or_insert(offset);
            *first = (*first).min(offset);
        }
    }

    /// Takes over the uses of an ended inner class, which did not declare
    /// them, keeping the earliest offset of each name.
    fn absorb(&mut self, mut inner: HashMap<NameId, u32>) {
        if inner.len() > self.unresolved.len() {
            std::mem::swap(&mut inner, &mut self.unresolved);
        }
        for (name, offset) in inner {
            self.note_use(name, offset);
        }
    }
}

/// A synthetic initializer function of a class being parsed.
#[derive(Clone, Copy, Debug)]
struct Initializer {
    function: FunctionId,
    scope: ScopeId,
}

/// The state of the class body being parsed.
struct ClassBody {
    body_scope: ScopeId,
    derived: bool,
    constructor: Option<FunctionId>,
    instance_init: Option<Initializer>,
    static_init: Option<Initializer>,
    elements: Vec<ClassElement>,
    start: u32,
}

/// The modifiers before a class element's name.
#[derive(Clone, Copy, Debug, Default)]
struct Modifiers {
    is_static: bool,
    is_async: bool,
    generator: bool,
    /// `get` or `set`.
    accessor: Option<FunctionKind>,
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    /// `ClassDeclaration` or `ClassExpression` (§15.7); the current token
    /// is `class`. A declaration binds its name in the current scope.
    pub(super) fn parse_class(&mut self, is_declaration: bool) -> PResult<ClassId> {
        self.enter()?;
        let outer_strict = std::mem::replace(&mut self.ctx.strict, true);
        let class = self.parse_class_inner(is_declaration);
        self.ctx.strict = outer_strict;
        self.leave();
        class
    }

    fn parse_class_inner(&mut self, is_declaration: bool) -> PResult<ClassId> {
        let start = self.token.start;
        self.advance()?;
        let mut name = None;
        if self.at_identifier() {
            let offset = self.token.start;
            if self.token.escaped && self.token.name().is_some_and(super::is_strict_reserved) {
                // V8 reports `class st\u0061tic {}` like the plain word.
                return Err(ParseError::syntax(
                    offset,
                    messages::UNEXPECTED_STRICT_RESERVED,
                ));
            }
            let usage = if is_declaration {
                IdentUse::Name
            } else {
                IdentUse::Binding
            };
            let checked = self.check_identifier(usage)?;
            if checked == names::AWAIT {
                // An async arrow function's head is reparsed with
                // [+Await] (§15.9), where this name is an error.
                self.ctx.last_await_name = Some(offset);
            }
            name = Some((checked, offset));
            self.advance()?;
        } else if is_declaration {
            return Err(self.unexpected());
        }
        let mut declaration = None;
        if let Some((name, offset)) = name
            && is_declaration
        {
            let declared =
                self.scopes
                    .declare_lexical(self.scope, name, BindingKind::Class, offset);
            let binding = self.declared(declared, name, start)?;
            declaration = Some((binding, self.reference(name, offset, true).reference));
        }
        let class_scope = self.new_scope(ScopeKind::Class, self.scope, start);
        let entered = self.enter_scope(class_scope);
        let result = self.parse_class_tail(start, class_scope, name);
        self.leave_class_scope(entered);
        let (mut class, inner_binding) = result?;
        let end = self.prev_end;
        if let Some(binding) = inner_binding {
            self.scopes.set_init_end(binding, end);
        }
        if let Some((binding, reference)) = declaration {
            self.scopes.set_init_end(binding, end);
            class.declaration = Some(reference);
        }
        Ok(self.ast.push_class(class))
    }

    /// The inner name binding, the heritage and the body; the current
    /// scope is the class scope. Returns the class and the inner binding.
    fn parse_class_tail(
        &mut self,
        start: u32,
        class_scope: ScopeId,
        name: Option<(NameId, u32)>,
    ) -> PResult<(Class, Option<crate::BindingId>)> {
        let mut inner = None;
        let mut inner_binding = None;
        if let Some((name, offset)) = name {
            inner_binding = Some(self.scopes.declare_class_name(class_scope, name, offset));
            inner = Some(self.reference(name, offset, true).reference);
        }
        let heritage = if self.eat(TokenKind::Extends)? {
            Some(self.parse_heritage()?)
        } else {
            None
        };
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let body_start = self.token.start;
        let body_scope = self.new_scope(ScopeKind::ClassBody, class_scope, body_start);
        let entered = self.enter_scope(body_scope);
        self.classes.push(PrivateNames::default());
        let mut body = ClassBody {
            body_scope,
            derived: heritage.is_some(),
            constructor: None,
            instance_init: None,
            static_init: None,
            elements: Vec::new(),
            start: body_start,
        };
        let result = self.parse_class_body(&mut body);
        let private = self.classes.pop().unwrap_or_default();
        self.leave_class_scope(entered);
        result?;
        self.resolve_private_names(private)?;
        let end = self.prev_end;
        let instance_init = self.finish_initializer(body.instance_init, end);
        let static_init = self.finish_initializer(body.static_init, end);
        let class = Class {
            name: name.map(|(name, _)| name),
            declaration: None,
            inner,
            heritage,
            constructor: body.constructor,
            elements: self.ast.push_class_elements(&body.elements),
            scope: class_scope,
            body_scope,
            instance_init,
            static_init,
            span: Span::new(start, end),
        };
        Ok((class, inner_binding))
    }

    /// `ClassHeritage`: `extends LeftHandSideExpression`. Its early errors
    /// of the cover grammar are its own.
    fn parse_heritage(&mut self) -> PResult<ExprId> {
        let outer = self.cover_error.take();
        let heritage = self.parse_left_hand_side()?;
        self.check_cover_error()?;
        self.cover_error = outer;
        Ok(heritage)
    }

    /// Closes a class scope. The items recorded in it stay in the log of
    /// the enclosing scope: a class is part of an expression, so an arrow
    /// function whose parameters contain it takes over its scopes, its
    /// methods and the direct `eval` calls of its computed keys (see
    /// [`crate::scope::ScopeTree::move_scope`]).
    fn leave_class_scope(&mut self, entered: Entered) {
        self.scopes.close_scope(self.scope);
        self.scope = entered.outer;
        self.scope_base = entered.outer_base;
    }

    /// `{ ClassBody }`.
    fn parse_class_body(&mut self, body: &mut ClassBody) -> PResult<()> {
        self.expect(TokenKind::LBrace)?;
        while !self.at(TokenKind::RBrace) {
            if self.eat(TokenKind::Semicolon)? {
                continue;
            }
            if self.at(TokenKind::Eof) {
                return Err(self.unexpected());
            }
            self.parse_class_element(body)?;
        }
        self.advance()
    }

    /// One `ClassElement` other than `;`.
    fn parse_class_element(&mut self, body: &mut ClassBody) -> PResult<()> {
        let start = self.token.start;
        let mut modifiers = Modifiers {
            is_static: self.at_modifier(names::STATIC, true)?,
            ..Modifiers::default()
        };
        if modifiers.is_static {
            self.advance()?;
            if self.at(TokenKind::LBrace) {
                let element = self.parse_static_block(body, start)?;
                body.elements.push(element);
                return Ok(());
            }
        }
        // The source text of a method starts after `static` (§15.4, for
        // `Function.prototype.toString`).
        let method_start = self.token.start;
        if self.at_modifier(names::ASYNC, false)? {
            self.check_modifier_escape()?;
            modifiers.is_async = true;
            self.advance()?;
        }
        if self.eat(TokenKind::Star)? {
            modifiers.generator = true;
        } else if !modifiers.is_async {
            for (name, kind) in [
                (names::GET, FunctionKind::Getter),
                (names::SET, FunctionKind::Setter),
            ] {
                if self.at_modifier(name, true)? {
                    self.check_modifier_escape()?;
                    modifiers.accessor = Some(kind);
                    self.advance()?;
                    break;
                }
            }
        }
        let key_start = self.token.start;
        let key = self.parse_class_element_name()?;
        let element = if self.at(TokenKind::LParen) {
            let starts = (start, method_start, key_start);
            self.parse_class_method(body, starts, key, modifiers)?
        } else if modifiers.is_async || modifiers.generator || modifiers.accessor.is_some() {
            return Err(self.unexpected());
        } else {
            self.parse_field(body, start, key_start, key, modifiers.is_static)?
        };
        if let Some(element) = element {
            body.elements.push(element);
        }
        Ok(())
    }

    /// Whether the current token is the contextual word `name` used as a
    /// modifier: followed by the start of an element name (`*` too), on
    /// the same line unless `newline` allows a line break (`async` does
    /// not, §15.8). Otherwise the word is itself the element's name.
    fn at_modifier(&mut self, name: NameId, newline: bool) -> PResult<bool> {
        if !(self.at(TokenKind::Identifier) && self.token.name() == Some(name)) {
            return Ok(false);
        }
        if self.token.escaped && name == names::STATIC {
            // An escaped `static` before a name: V8 reads a field named
            // `static` (and then fails at the name).
            return Ok(false);
        }
        let next = self.peek()?;
        if next.newline_before && !newline {
            return Ok(false);
        }
        let starts_name = next.kind.is_keyword()
            || matches!(
                next.kind,
                TokenKind::Identifier
                    | TokenKind::PrivateName
                    | TokenKind::String
                    | TokenKind::Number
                    | TokenKind::BigInt
                    | TokenKind::LBracket
            )
            || (next.kind == TokenKind::Star && name != names::GET && name != names::SET)
            || (name == names::STATIC && next.kind == TokenKind::LBrace);
        Ok(starts_name)
    }

    /// `async`, `get` and `set` written with escapes are not modifiers.
    fn check_modifier_escape(&self) -> PResult<()> {
        if self.token.escaped {
            return Err(ParseError::syntax(
                self.token.start,
                messages::ESCAPED_KEYWORD,
            ));
        }
        Ok(())
    }

    /// `ClassElementName`: a property name or a private name; advances.
    fn parse_class_element_name(&mut self) -> PResult<ClassKey> {
        if !self.at(TokenKind::PrivateName) {
            return Ok(ClassKey::Property(self.parse_property_key()?));
        }
        let offset = self.token.start;
        let name = self.current_private_id()?;
        let binding_scope = self.scope;
        self.scopes
            .declare_private_name(binding_scope, name, offset);
        let ident = self.reference(name, offset, true);
        self.advance()?;
        Ok(ClassKey::Private(ident))
    }

    /// A method, accessor or the constructor; the current token is `(`.
    /// `starts`: where the element, the method's source text and the key
    /// start.
    fn parse_class_method(
        &mut self,
        body: &mut ClassBody,
        starts: (u32, u32, u32),
        key: ClassKey,
        modifiers: Modifiers,
    ) -> PResult<Option<ClassElement>> {
        let (start, method_start, key_start) = starts;
        let property_name = property_name(key);
        if !modifiers.is_static && property_name == Some(names::CONSTRUCTOR) {
            let message = if modifiers.accessor.is_some() {
                Some(messages::CONSTRUCTOR_ACCESSOR)
            } else if modifiers.generator {
                Some(messages::CONSTRUCTOR_GENERATOR)
            } else if modifiers.is_async {
                Some(messages::CONSTRUCTOR_ASYNC)
            } else if body.constructor.is_some() {
                Some(messages::DUPLICATE_CONSTRUCTOR)
            } else {
                None
            };
            if let Some(message) = message {
                return Err(ParseError::syntax(key_start, message));
            }
            let kind = if body.derived {
                FunctionKind::DerivedConstructor
            } else {
                FunctionKind::ClassConstructor
            };
            let value =
                self.parse_method(FunctionHead::method(kind, false, false, method_start))?;
            if let crate::ExprKind::Function(function) = self.ast.expr(value).kind {
                body.constructor = Some(function);
            }
            return Ok(None);
        }
        if let ClassKey::Private(ident) = key
            && self.is_private_constructor(ident.name)
        {
            return Err(ParseError::syntax(key_start, messages::CONSTRUCTOR_PRIVATE));
        }
        if modifiers.is_static && property_name == Some(names::PROTOTYPE) {
            return Err(ParseError::syntax(key_start, messages::STATIC_PROTOTYPE));
        }
        let (kind, function_kind) = match modifiers.accessor {
            Some(FunctionKind::Getter) => (ClassElementKind::Getter, FunctionKind::Getter),
            Some(_) => (ClassElementKind::Setter, FunctionKind::Setter),
            None => (ClassElementKind::Method, FunctionKind::Method),
        };
        let head = FunctionHead::method(
            function_kind,
            modifiers.generator,
            modifiers.is_async,
            method_start,
        );
        let value = self.parse_method(head)?;
        if let ClassKey::Private(ident) = key {
            // V8 reports a duplicate private method at its end.
            let decl = match kind {
                ClassElementKind::Getter => PrivateDecl::Getter(modifiers.is_static),
                ClassElementKind::Setter => PrivateDecl::Setter(modifiers.is_static),
                _ => PrivateDecl::Other,
            };
            self.declare_private(ident.name, decl, self.prev_end.saturating_sub(1))?;
        }
        Ok(Some(ClassElement {
            kind,
            is_static: modifiers.is_static,
            key,
            value: Some(value),
            span: Span::new(start, self.prev_end),
        }))
    }

    /// A `FieldDefinition` and its `;` (or an inserted one).
    fn parse_field(
        &mut self,
        body: &mut ClassBody,
        start: u32,
        key_start: u32,
        key: ClassKey,
        is_static: bool,
    ) -> PResult<Option<ClassElement>> {
        match key {
            ClassKey::Property(PropertyKey::Name(name)) => {
                if name == names::CONSTRUCTOR {
                    return Err(ParseError::syntax(key_start, messages::CONSTRUCTOR_FIELD));
                }
                if is_static && name == names::PROTOTYPE {
                    return Err(ParseError::syntax(key_start, messages::STATIC_PROTOTYPE));
                }
            }
            ClassKey::Private(ident) => {
                if self.is_private_constructor(ident.name) {
                    return Err(ParseError::syntax(key_start, messages::CONSTRUCTOR_FIELD));
                }
            }
            ClassKey::Property(_) | ClassKey::StaticBlock => {}
        }
        let initializer = self.initializer(body, is_static, start);
        let value = if self.eat(TokenKind::Eq)? {
            Some(self.parse_field_initializer(initializer, is_static)?)
        } else {
            None
        };
        match self.token.kind {
            TokenKind::Semicolon => self.advance()?,
            TokenKind::RBrace => {}
            _ if self.token.newline_before => {}
            // `await 1` where `await` is an identifier. In async code V8
            // words it like any unexpected token.
            _ if self.prev_await.is_some() && self.ctx.await_mode == AwaitMode::Identifier => {
                let at = self.prev_await.unwrap_or(self.token.start);
                return Err(ParseError::syntax(at, messages::AWAIT_OUTSIDE_ASYNC));
            }
            _ => return Err(self.unexpected()),
        }
        if let ClassKey::Private(ident) = key {
            // V8 reports a duplicate at the last token of the field.
            self.declare_private(ident.name, PrivateDecl::Other, self.prev_start)?;
        }
        Ok(Some(ClassElement {
            kind: ClassElementKind::Field,
            is_static,
            key,
            value,
            span: Span::new(start, self.prev_end),
        }))
    }

    /// The instance or static initializer function of the class, created
    /// for its first element.
    fn initializer(&mut self, body: &mut ClassBody, is_static: bool, start: u32) -> Initializer {
        let slot = if is_static {
            &mut body.static_init
        } else {
            &mut body.instance_init
        };
        if let Some(init) = *slot {
            return init;
        }
        let function = self.begin_function(start);
        let kind = if is_static {
            FunctionKind::StaticInitializer
        } else {
            FunctionKind::InstanceInitializer
        };
        if let Some(record) = self.ast.function_mut(function) {
            record.kind = kind;
            record.strict = true;
        }
        let scope = self.new_scope_of(ScopeKind::Function, body.body_scope, function, body.start);
        if let Some(record) = self.ast.function_mut(function) {
            record.scope = scope;
            record.body_scope = scope;
        }
        let init = Initializer { function, scope };
        let slot = if is_static {
            &mut body.static_init
        } else {
            &mut body.instance_init
        };
        *slot = Some(init);
        init
    }

    /// The context of code in an initializer function: no `yield`.
    /// `await` is reserved in static blocks (§15.7.1). In field
    /// initializers ECMA-262 2025 keeps the `[Await]` parameter of the
    /// class (`Initializer[+In, ?Yield, ?Await]`); swb follows V8, where
    /// `await` is an identifier in instance field initializers and
    /// reserved in static ones, also outside async functions.
    fn initializer_context(&self, init: Initializer, kind: FunctionKind) -> Context {
        let mut context = Context::new(init.function, kind, true, false, self.labels.len());
        context.await_mode = if kind == FunctionKind::StaticInitializer {
            AwaitMode::Reserved
        } else {
            AwaitMode::Identifier
        };
        context
    }

    /// The `Initializer` of a field, in the initializer function.
    fn parse_field_initializer(&mut self, init: Initializer, is_static: bool) -> PResult<ExprId> {
        let kind = if is_static {
            FunctionKind::StaticInitializer
        } else {
            FunctionKind::InstanceInitializer
        };
        let context = self.initializer_context(init, kind);
        let outer = std::mem::replace(&mut self.ctx, context);
        let entered = self.enter_scope(init.scope);
        let value = self.parse_assignment(false);
        self.leave_scope(entered);
        let inner = std::mem::replace(&mut self.ctx, outer);
        self.add_initializer_evals(init.function, inner.direct_evals);
        value
    }

    /// `static { ... }` (§15.7, `ClassStaticBlock`); the current token is
    /// `{`.
    fn parse_static_block(&mut self, body: &mut ClassBody, start: u32) -> PResult<ClassElement> {
        let init = self.initializer(body, true, start);
        let context = self.initializer_context(init, FunctionKind::StaticInitializer);
        let outer = std::mem::replace(&mut self.ctx, context);
        let function_scope = self.enter_scope(init.scope);
        let block_scope = self.new_scope(ScopeKind::StaticBlock, init.scope, self.token.start);
        let block = self.enter_scope(block_scope);
        let statements = self.parse_static_block_body();
        self.leave_scope(block);
        self.leave_scope(function_scope);
        let inner = std::mem::replace(&mut self.ctx, outer);
        self.add_initializer_evals(init.function, inner.direct_evals);
        Ok(ClassElement {
            kind: ClassElementKind::StaticBlock {
                body: statements?,
                scope: block_scope,
            },
            is_static: true,
            key: ClassKey::StaticBlock,
            value: None,
            span: Span::new(start, self.prev_end),
        })
    }

    fn parse_static_block_body(&mut self) -> PResult<List<crate::StmtId>> {
        self.expect(TokenKind::LBrace)?;
        let statements = self.parse_statements(TokenKind::RBrace, false)?;
        self.expect(TokenKind::RBrace)?;
        Ok(statements)
    }

    fn add_initializer_evals(&mut self, function: FunctionId, evals: u32) {
        if evals > 0
            && let Some(record) = self.scopes.function_mut(function)
        {
            record.has_direct_eval = true;
        }
    }

    /// Fills in the span of an initializer function at the end of its
    /// class.
    fn finish_initializer(&mut self, init: Option<Initializer>, end: u32) -> Option<FunctionId> {
        let init = init?;
        if let Some(record) = self.ast.function_mut(init.function) {
            let start = record.span.start;
            record.span = Span::new(start, end);
        }
        Some(init.function)
    }

    // --- Private names ---

    /// The id (with `#`) of the current private name token. The lexer
    /// gives every private name token its text; a token without one is
    /// reported as unexpected.
    pub(super) fn current_private_id(&mut self) -> PResult<NameId> {
        match self.token.name() {
            Some(name) => Ok(self.private_id(name)),
            None => Err(self.unexpected()),
        }
    }

    /// The id of `#name` for the private name `name` (without `#`).
    pub(super) fn private_id(&mut self, name: NameId) -> NameId {
        if let Some(&id) = self.private_ids.get(&name) {
            return id;
        }
        let mut text = String16::from("#");
        if let Some(units) = self.lexer.interner().get(name) {
            text.push_str16(units);
        }
        let id = self.lexer.interner_mut().intern(text.as_str16());
        self.private_ids.insert(name, id);
        id
    }

    /// Whether a private name (with `#`) is `#constructor` (§15.7.1).
    fn is_private_constructor(&self, name: NameId) -> bool {
        self.lexer
            .interner()
            .get(name)
            .is_some_and(|text| text.eq_str("#constructor"))
    }

    /// Notes a use of a private name (with `#`): an early error outside
    /// classes, otherwise checked when the classes end.
    pub(super) fn note_private_use(&mut self, name: NameId, offset: u32) -> PResult<()> {
        match self.classes.last_mut() {
            Some(class) => {
                class.note_use(name, offset);
                Ok(())
            }
            None => Err(self.undeclared_private(name, offset)),
        }
    }

    pub(super) fn undeclared_private(&self, name: NameId, offset: u32) -> ParseError {
        ParseError::syntax(offset, messages::undeclared_private(&self.name_text(name)))
    }

    /// Declares a private name of the current class; a repeated name is
    /// an early error at `offset` (§15.7.1).
    fn declare_private(&mut self, name: NameId, decl: PrivateDecl, offset: u32) -> PResult<()> {
        let Some(class) = self.classes.last_mut() else {
            return Ok(());
        };
        let merged = match (class.declared.get(&name).copied(), decl) {
            (None, decl) => Some(decl),
            (Some(PrivateDecl::Getter(a)), PrivateDecl::Setter(b))
            | (Some(PrivateDecl::Setter(a)), PrivateDecl::Getter(b))
                if a == b =>
            {
                Some(PrivateDecl::Pair)
            }
            _ => None,
        };
        match merged {
            Some(decl) => {
                class.declared.insert(name, decl);
                Ok(())
            }
            None => Err(self.redeclared(name, offset)),
        }
    }

    /// At the end of a class: its uses of names it did not declare move to
    /// the enclosing class; outside all classes they are early errors.
    fn resolve_private_names(&mut self, class: PrivateNames) -> PResult<()> {
        let PrivateNames {
            declared,
            mut unresolved,
        } = class;
        for name in declared.keys() {
            unresolved.remove(name);
        }
        if let Some(outer) = self.classes.last_mut() {
            outer.absorb(unresolved);
            return Ok(());
        }
        // The first use in source order.
        match unresolved.into_iter().min_by_key(|&(_, offset)| offset) {
            Some((name, offset)) => Err(self.undeclared_private(name, offset)),
            None => Ok(()),
        }
    }
}

/// The name of a non-computed property key (identifier or string), for the
/// `constructor` and `prototype` rules (`PropName`, §15.7.1).
fn property_name(key: ClassKey) -> Option<NameId> {
    match key {
        ClassKey::Property(PropertyKey::Name(name)) => Some(name),
        _ => None,
    }
}
