//! Patterns: binding patterns of declarations, parameters and `catch`
//! (§14.3.3), and the cover grammar conversions (§5.1.4): an array or
//! object literal before `=` into an assignment pattern (§13.15.5), and
//! the expression before `=>` into the parameters of an arrow function
//! (§15.3.1).
//!
//! The conversions recurse over the literal; each level charges the
//! recursion budget. A converted node is never converted again, except
//! once from an assignment pattern into a binding pattern (an arrow
//! parameter such as `([a] = b) => 0`), which changes the nodes in place.
//! So the conversions are linear in the size of the cover expression.

use swb_js_text::CodeUnit;

use super::expressions::{is_property_target, starts_expression};
use super::{AwaitMode, IdentUse, LEVEL_WEIGHT, PResult, Parser};
use crate::ast::{
    AssignOp, AssignTarget, BindingId, ExprId, ExprKind, Ident, List, PatternId, PatternKind,
    PatternProperty, PropertyKind, Span,
};
use crate::error::ParseError;
use crate::interner::NameId;
use crate::messages;
use crate::scope::BindingKind;
use crate::token::TokenKind;

/// What a binding pattern declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeclKind {
    Var,
    Let,
    Const,
    /// A parameter of the current function.
    Param,
    /// A name in a `catch` parameter pattern (`catch ([e])`).
    Catch,
}

impl DeclKind {
    fn usage(self) -> IdentUse {
        match self {
            DeclKind::Let | DeclKind::Const => IdentUse::Lexical,
            _ => IdentUse::Binding,
        }
    }
}

/// Where a target of a pattern is, for V8's choice of message: a whole
/// declaration or parameter (`var eval`), an element inside a pattern
/// (`[eval]`, `[a.b]`), or a shorthand or an element with an initializer
/// inside a pattern (`{eval}`, `[eval = 1]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    Top,
    Plain,
    Named,
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    fn push_pattern(&mut self, kind: PatternKind, start: u32) -> PatternId {
        self.ast.push_pattern(kind, Span::new(start, self.prev_end))
    }

    // --- Binding patterns, parsed directly ---

    /// `BindingIdentifier` or `BindingPattern` (§14.3.3).
    pub(super) fn parse_binding_target(
        &mut self,
        kind: DeclKind,
        leaf: Leaf,
    ) -> PResult<PatternId> {
        match self.token.kind {
            TokenKind::LBracket => self.parse_array_binding(kind),
            TokenKind::LBrace => self.parse_object_binding(kind),
            TokenKind::Await
                if leaf != Leaf::Top && self.ctx.await_mode == AwaitMode::Expression =>
            {
                // V8 reads an element of a pattern in async code as an
                // expression: `await` starts an await expression.
                let start = self.token.start;
                if self.ctx.in_params {
                    return Err(ParseError::syntax(start, messages::AWAIT_IN_PARAMETER));
                }
                self.advance()?;
                if starts_expression(self.token.kind) {
                    return Err(ParseError::syntax(
                        start,
                        messages::INVALID_DESTRUCTURING_TARGET,
                    ));
                }
                Err(self.unexpected())
            }
            _ if self.at_identifier() => {
                let start = self.token.start;
                let ident = self.parse_binding_identifier(kind, leaf)?;
                if leaf != Leaf::Top
                    && matches!(
                        self.token.kind,
                        TokenKind::Dot | TokenKind::LBracket | TokenKind::QuestionDot
                    )
                {
                    return Err(ParseError::syntax(start, messages::PROPERTY_IN_DECLARATION));
                }
                Ok(self.push_pattern(PatternKind::Identifier(ident), start))
            }
            TokenKind::Number
            | TokenKind::BigInt
            | TokenKind::String
            | TokenKind::LParen
            | TokenKind::This
            | TokenKind::Null
            | TokenKind::True
            | TokenKind::False
                if leaf != Leaf::Top =>
            {
                Err(ParseError::syntax(
                    self.token.start,
                    messages::INVALID_DESTRUCTURING_TARGET,
                ))
            }
            _ => Err(self.unexpected()),
        }
    }

    /// `BindingElement`: a target with an optional initializer.
    pub(super) fn parse_binding_element(
        &mut self,
        kind: DeclKind,
        leaf: Leaf,
    ) -> PResult<PatternId> {
        let start = self.token.start;
        let target = self.parse_binding_target(kind, leaf)?;
        if !self.eat(TokenKind::Eq)? {
            return Ok(target);
        }
        let value = self.parse_assignment(false)?;
        Ok(self.push_pattern(PatternKind::Default { target, value }, start))
    }

    /// The current identifier as a binding of `kind`: checks, declares
    /// and records it, and advances.
    fn parse_binding_identifier(&mut self, kind: DeclKind, leaf: Leaf) -> PResult<Ident> {
        let start = self.token.start;
        let name = match self.check_identifier(kind.usage()) {
            Err(error)
                if leaf == Leaf::Plain
                    && (error.message == messages::UNEXPECTED_EVAL_OR_ARGUMENTS
                        || (self.at(TokenKind::Yield) && self.ctx.generator))
                    && self.peek_kind()? != TokenKind::Eq =>
            {
                return Err(ParseError::syntax(
                    start,
                    messages::INVALID_DESTRUCTURING_TARGET,
                ));
            }
            other => other?,
        };
        self.declare_name(kind, name, start)?;
        let ident = self.reference(name, start, true);
        self.advance()?;
        Ok(ident)
    }

    /// Declares a name of a binding pattern in the current scope.
    pub(super) fn declare_name(
        &mut self,
        kind: DeclKind,
        name: NameId,
        offset: u32,
    ) -> PResult<BindingId> {
        let scope = self.scope;
        let result = match kind {
            DeclKind::Var => self.scopes.declare_var(scope, name, offset),
            DeclKind::Let => self
                .scopes
                .declare_lexical(scope, name, BindingKind::Let, offset),
            DeclKind::Const => self
                .scopes
                .declare_lexical(scope, name, BindingKind::Const, offset),
            DeclKind::Catch => self.scopes.declare_catch_param(scope, name, offset, true),
            DeclKind::Param => return Ok(self.declare_param_name(name, offset)),
        };
        self.declared(result, name, offset)
    }

    /// Declares a parameter of the current function; notes the first
    /// repeated name.
    pub(super) fn declare_param_name(&mut self, name: NameId, offset: u32) -> BindingId {
        let (binding, duplicate) = self.scopes.declare_param(self.scope, name, offset);
        if duplicate && self.ctx.duplicate_param.is_none() {
            self.ctx.duplicate_param = Some(offset);
        }
        binding
    }

    /// `ArrayBindingPattern`.
    fn parse_array_binding(&mut self, kind: DeclKind) -> PResult<PatternId> {
        self.enter()?;
        let result = self.parse_array_binding_inner(kind);
        self.leave();
        result
    }

    fn parse_array_binding_inner(&mut self, kind: DeclKind) -> PResult<PatternId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_patterns.len();
        let result = self.parse_array_binding_elements(kind);
        let list = self.ast.push_patterns(&self.scratch_patterns[mark..]);
        self.scratch_patterns.truncate(mark);
        result?;
        Ok(self.push_pattern(PatternKind::Array(list), start))
    }

    fn parse_array_binding_elements(&mut self, kind: DeclKind) -> PResult<()> {
        while !self.at(TokenKind::RBracket) {
            let start = self.token.start;
            if self.eat(TokenKind::Comma)? {
                let hole = self
                    .ast
                    .push_pattern(PatternKind::Hole, Span::new(start, start));
                self.scratch_patterns.push(hole);
                continue;
            }
            if self.eat(TokenKind::Ellipsis)? {
                let target = self.parse_binding_target(kind, Leaf::Plain)?;
                if self.at(TokenKind::Eq) {
                    return Err(ParseError::syntax(
                        start,
                        messages::INVALID_DESTRUCTURING_TARGET,
                    ));
                }
                let rest = self.push_pattern(PatternKind::Rest(target), start);
                self.scratch_patterns.push(rest);
                return match self.token.kind {
                    TokenKind::RBracket => self.advance(),
                    TokenKind::Comma => Err(ParseError::syntax(start, messages::REST_NOT_LAST)),
                    _ => Err(self.unexpected()),
                };
            }
            let element = self.parse_binding_element(kind, Leaf::Plain)?;
            self.scratch_patterns.push(element);
            if !self.at(TokenKind::RBracket) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()
    }

    /// `ObjectBindingPattern`.
    fn parse_object_binding(&mut self, kind: DeclKind) -> PResult<PatternId> {
        self.enter()?;
        let result = self.parse_object_binding_inner(kind);
        self.leave();
        result
    }

    fn parse_object_binding_inner(&mut self, kind: DeclKind) -> PResult<PatternId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_pattern_properties.len();
        let rest = self.parse_object_binding_properties(kind);
        let properties = self
            .ast
            .push_pattern_properties(&self.scratch_pattern_properties[mark..]);
        self.scratch_pattern_properties.truncate(mark);
        let rest = rest?;
        Ok(self.push_pattern(PatternKind::Object { properties, rest }, start))
    }

    /// The properties up to and including `}`; returns the rest element.
    fn parse_object_binding_properties(&mut self, kind: DeclKind) -> PResult<Option<PatternId>> {
        while !self.at(TokenKind::RBrace) {
            if self.eat(TokenKind::Ellipsis)? {
                if !self.at_identifier() {
                    return Err(ParseError::syntax(
                        self.token.start,
                        messages::REST_NOT_IDENTIFIER,
                    ));
                }
                let target_start = self.token.start;
                let ident = self.parse_binding_identifier(kind, Leaf::Plain)?;
                let target = self.push_pattern(PatternKind::Identifier(ident), target_start);
                return match self.token.kind {
                    TokenKind::RBrace => {
                        self.advance()?;
                        Ok(Some(target))
                    }
                    TokenKind::Comma => {
                        Err(ParseError::syntax(target_start, messages::REST_NOT_LAST))
                    }
                    TokenKind::Dot | TokenKind::LBracket => Err(ParseError::syntax(
                        target_start,
                        messages::PROPERTY_IN_DECLARATION,
                    )),
                    _ => Err(self.unexpected()),
                };
            }
            let property = self.parse_binding_property(kind)?;
            self.scratch_pattern_properties.push(property);
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        Ok(None)
    }

    /// `BindingProperty`: `key: element`, or a shorthand with an optional
    /// initializer.
    fn parse_binding_property(&mut self, kind: DeclKind) -> PResult<PatternProperty> {
        let start = self.token.start;
        let key_token = self.token.clone();
        let key = self.parse_property_key()?;
        if self.eat(TokenKind::Colon)? {
            let value = self.parse_binding_element(kind, Leaf::Plain)?;
            return Ok(PatternProperty {
                key,
                value,
                span: Span::new(start, self.prev_end),
            });
        }
        // A shorthand: the key must be a BindingIdentifier.
        if !matches!(
            key_token.kind,
            TokenKind::Identifier | TokenKind::Yield | TokenKind::Await
        ) {
            return Err(self.unexpected_key(&key_token));
        }
        let name = self.check_identifier_token(&key_token, kind.usage())?;
        self.declare_name(kind, name, start)?;
        let ident = self.reference(name, start, true);
        let mut value = self.push_pattern(PatternKind::Identifier(ident), start);
        if self.eat(TokenKind::Eq)? {
            let init = self.parse_assignment(false)?;
            value = self.push_pattern(
                PatternKind::Default {
                    target: value,
                    value: init,
                },
                start,
            );
        }
        Ok(PatternProperty {
            key,
            value,
            span: Span::new(start, self.prev_end),
        })
    }

    // --- Assignment patterns, from an array or object literal ---

    /// Converts an array or object literal (not parenthesized) into an
    /// assignment pattern (§13.15.5.1), with its early errors.
    pub(super) fn assignment_pattern(&mut self, expr: ExprId) -> PResult<PatternId> {
        self.charge(LEVEL_WEIGHT)?;
        let result = self.assignment_pattern_inner(expr);
        self.budget.leave(LEVEL_WEIGHT);
        result
    }

    fn assignment_pattern_inner(&mut self, expr: ExprId) -> PResult<PatternId> {
        let node = *self.ast.expr(expr);
        let trailing = self.trailing_comma_after_spread.contains(&expr);
        let kind = match node.kind {
            ExprKind::Array(list) => {
                let items = self.ast.exprs(list).to_vec();
                let mut elements = Vec::with_capacity(items.len());
                for (index, &item) in items.iter().enumerate() {
                    let item_node = *self.ast.expr(item);
                    let element = match item_node.kind {
                        ExprKind::Hole => self.ast.push_pattern(PatternKind::Hole, item_node.span),
                        ExprKind::Spread(argument) => {
                            if index + 1 != items.len() || trailing {
                                return Err(ParseError::syntax(
                                    item_node.span.start,
                                    messages::REST_NOT_LAST,
                                ));
                            }
                            if matches!(self.ast.expr(argument).kind, ExprKind::Assign { .. }) {
                                // V8 marks the `...`.
                                return Err(ParseError::syntax(
                                    item_node.span.start,
                                    messages::INVALID_DESTRUCTURING_TARGET,
                                ));
                            }
                            let target = self.assignment_target_pattern(argument)?;
                            self.ast
                                .push_pattern(PatternKind::Rest(target), item_node.span)
                        }
                        _ => self.assignment_element(item)?,
                    };
                    elements.push(element);
                }
                PatternKind::Array(self.ast.push_patterns(&elements))
            }
            ExprKind::Object(list) => {
                let properties = self.ast.properties(list).to_vec();
                let mut converted = Vec::with_capacity(properties.len());
                let mut rest = None;
                for (index, property) in properties.iter().enumerate() {
                    let value = match property.kind {
                        PropertyKind::Init | PropertyKind::Proto => {
                            self.assignment_element(property.value)?
                        }
                        PropertyKind::Shorthand => self.shorthand_target(property.value)?,
                        PropertyKind::Spread => {
                            // V8 checks the target first and marks the
                            // last token of the rest element.
                            let target = self.object_rest_target(property.value)?;
                            if index + 1 != properties.len() || trailing {
                                return Err(ParseError::syntax(
                                    self.last_token_start(property.value),
                                    messages::REST_NOT_LAST,
                                ));
                            }
                            rest = Some(target);
                            break;
                        }
                        PropertyKind::Method | PropertyKind::Getter | PropertyKind::Setter => {
                            return Err(ParseError::syntax(
                                property.span.start,
                                messages::INVALID_DESTRUCTURING_TARGET,
                            ));
                        }
                    };
                    converted.push(PatternProperty {
                        key: property.key,
                        value,
                        span: property.span,
                    });
                }
                let properties = self.ast.push_pattern_properties(&converted);
                PatternKind::Object { properties, rest }
            }
            _ => {
                return Err(ParseError::syntax(
                    node.span.start,
                    messages::INVALID_DESTRUCTURING_TARGET,
                ));
            }
        };
        Ok(self.ast.push_pattern(kind, node.span))
    }

    /// An element of an assignment pattern: `target` or `target = value`.
    fn assignment_element(&mut self, item: ExprId) -> PResult<PatternId> {
        let node = *self.ast.expr(item);
        let ExprKind::Assign {
            op: AssignOp::Assign,
            target,
            value,
        } = node.kind
        else {
            return self.assignment_target_pattern(item);
        };
        let target = match target {
            AssignTarget::Pattern(pattern) => pattern,
            AssignTarget::Simple(expr) => {
                let leaf = self.assignment_leaf(expr, Leaf::Named)?;
                // `[(a) = 1]`: the assignment removed the parentheses; the
                // leaf's span keeps them visible for an arrow parameter
                // (see `binding_from_pattern`).
                if let Some(pattern) = self.ast.pattern_mut(leaf) {
                    pattern.span.start = node.span.start;
                }
                leaf
            }
        };
        Ok(self
            .ast
            .push_pattern(PatternKind::Default { target, value }, node.span))
    }

    /// `DestructuringAssignmentTarget`: a nested literal (not
    /// parenthesized) or a simple target.
    fn assignment_target_pattern(&mut self, item: ExprId) -> PResult<PatternId> {
        if matches!(
            self.ast.expr(item).kind,
            ExprKind::Array(_) | ExprKind::Object(_)
        ) {
            return self.assignment_pattern(item);
        }
        self.assignment_leaf(item, Leaf::Plain)
    }

    /// A simple assignment target in a pattern: an identifier (not `eval`
    /// or `arguments` in strict mode code) or a property access;
    /// parentheses are allowed (`[(a)] = 1`).
    fn assignment_leaf(&mut self, item: ExprId, leaf: Leaf) -> PResult<PatternId> {
        let span = self.ast.expr(item).span;
        let target = self.unparenthesized(item);
        let invalid = || {
            Err(ParseError::syntax(
                span.start,
                messages::INVALID_DESTRUCTURING_TARGET,
            ))
        };
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => {
                if self.ctx.strict && super::is_eval_or_arguments(ident.name) {
                    let message = match leaf {
                        Leaf::Plain => messages::INVALID_DESTRUCTURING_TARGET,
                        Leaf::Top | Leaf::Named => messages::UNEXPECTED_EVAL_OR_ARGUMENTS,
                    };
                    return Err(ParseError::syntax(span.start, message));
                }
            }
            kind if is_property_target(kind) => {}
            _ => return invalid(),
        }
        Ok(self.ast.push_pattern(PatternKind::Expr(target), span))
    }

    /// The value of a shorthand property in an assignment pattern: `a`
    /// or `a = 1` (a `CoverInitializedName`, stored as an assignment).
    fn shorthand_target(&mut self, value: ExprId) -> PResult<PatternId> {
        let node = *self.ast.expr(value);
        match node.kind {
            ExprKind::Assign {
                target: AssignTarget::Simple(target),
                value: init,
                ..
            } => {
                let target = self.assignment_leaf(target, Leaf::Named)?;
                Ok(self.ast.push_pattern(
                    PatternKind::Default {
                        target,
                        value: init,
                    },
                    node.span,
                ))
            }
            _ => self.assignment_leaf(value, Leaf::Named),
        }
    }

    /// The start of the last token of `expr`, where V8 marks an error
    /// about a whole rest element (`...a.b` marks `b`, `...a[0]` marks
    /// `]`). Approximate for a member name with escapes.
    pub(super) fn last_token_start(&self, expr: ExprId) -> u32 {
        let node = self.ast.expr(expr);
        match node.kind {
            ExprKind::Identifier(_) => node.span.start,
            ExprKind::Member { property, .. } => {
                let length = self
                    .lexer
                    .interner()
                    .get(property)
                    .map_or(1, swb_js_text::Str16::len);
                node.span.end.saturating_sub(length as u32)
            }
            ExprKind::Assign { target, .. } => match target {
                AssignTarget::Simple(target) => self.last_token_start(target),
                AssignTarget::Pattern(pattern) => {
                    self.ast.pattern(pattern).span.end.saturating_sub(1)
                }
            },
            _ => node.span.end.saturating_sub(1),
        }
    }

    /// The error for a rest element of a parenthesized list that is not
    /// last or has a default (`(...a, b)`, `(...a = 1)`).
    pub(super) fn rest_parameter_error(&self, argument: ExprId) -> ParseError {
        let message = if matches!(self.ast.expr(argument).kind, ExprKind::Assign { .. }) {
            messages::REST_PARAMETER_DEFAULT
        } else {
            messages::REST_PARAMETER_NOT_LAST
        };
        ParseError::syntax(self.last_token_start(argument), message)
    }

    /// The target of `...` in an object assignment pattern: a simple
    /// target only. V8 words a nested pattern or a default (`...a = 1`,
    /// not parenthesized) and another expression differently.
    fn object_rest_target(&mut self, argument: ExprId) -> PResult<PatternId> {
        let target = self.unparenthesized(argument);
        let message = match self.ast.expr(target).kind {
            ExprKind::Identifier(_) => return self.assignment_leaf(argument, Leaf::Plain),
            kind if is_property_target(kind) => {
                return self.assignment_leaf(argument, Leaf::Plain);
            }
            ExprKind::Array(_) | ExprKind::Object(_) => messages::REST_NOT_ASSIGNABLE,
            ExprKind::Assign {
                op: AssignOp::Assign,
                ..
            } if target == argument => messages::REST_NOT_ASSIGNABLE,
            _ => messages::INVALID_DESTRUCTURING_TARGET,
        };
        Err(ParseError::syntax(
            self.ast.expr(argument).span.start,
            message,
        ))
    }

    // --- Arrow parameters, from the cover expression ---

    /// One element of an arrow function's parameters (not a rest
    /// element), from its cover expression; declares its names in the
    /// current scope (the arrow's).
    pub(super) fn arrow_param(&mut self, item: ExprId, leaf: Leaf) -> PResult<PatternId> {
        let node = *self.ast.expr(item);
        let ExprKind::Assign {
            op: AssignOp::Assign,
            target,
            value,
        } = node.kind
        else {
            return self.binding_from_expr(item, leaf);
        };
        let target = match target {
            AssignTarget::Pattern(pattern) => self.binding_from_pattern(pattern)?,
            // `((a) = 1) => 0`: the assignment removed the parentheses.
            AssignTarget::Simple(expr) if self.ast.expr(expr).span.start != node.span.start => {
                return Err(ParseError::syntax(
                    node.span.start,
                    messages::INVALID_DESTRUCTURING_TARGET,
                ));
            }
            AssignTarget::Simple(expr) if is_property_target(self.ast.expr(expr).kind) => {
                Err(ParseError::syntax(
                    self.ast.expr(expr).span.start,
                    messages::PROPERTY_IN_DECLARATION,
                ))?
            }
            AssignTarget::Simple(expr) => {
                let leaf = if leaf == Leaf::Top { leaf } else { Leaf::Named };
                self.binding_from_expr(expr, leaf)?
            }
        };
        Ok(self
            .ast
            .push_pattern(PatternKind::Default { target, value }, node.span))
    }

    /// A binding target from an expression: an identifier, or an array
    /// or object literal (not parenthesized).
    pub(super) fn binding_from_expr(&mut self, item: ExprId, leaf: Leaf) -> PResult<PatternId> {
        let node = *self.ast.expr(item);
        match node.kind {
            ExprKind::Identifier(ident) => {
                let ident = self.declare_cover_name(ident, node.span.start, leaf)?;
                Ok(self
                    .ast
                    .push_pattern(PatternKind::Identifier(ident), node.span))
            }
            ExprKind::Array(_) | ExprKind::Object(_) => {
                self.charge(LEVEL_WEIGHT)?;
                let result = self.binding_from_literal(item);
                self.budget.leave(LEVEL_WEIGHT);
                result
            }
            kind if leaf != Leaf::Top && is_property_target(kind) => Err(ParseError::syntax(
                node.span.start,
                messages::PROPERTY_IN_DECLARATION,
            )),
            ExprKind::Yield { .. } => {
                let message = if leaf == Leaf::Top {
                    messages::INVALID_DESTRUCTURING_TARGET
                } else {
                    messages::YIELD_IN_PARAMETER
                };
                Err(ParseError::syntax(node.span.start, message))
            }
            _ => Err(ParseError::syntax(
                node.span.start,
                messages::INVALID_DESTRUCTURING_TARGET,
            )),
        }
    }

    fn binding_from_literal(&mut self, expr: ExprId) -> PResult<PatternId> {
        let node = *self.ast.expr(expr);
        let trailing = self.trailing_comma_after_spread.contains(&expr);
        let kind = match node.kind {
            ExprKind::Array(list) => {
                let items = self.ast.exprs(list).to_vec();
                let mut elements = Vec::with_capacity(items.len());
                for (index, &item) in items.iter().enumerate() {
                    let item_node = *self.ast.expr(item);
                    let element = match item_node.kind {
                        ExprKind::Hole => self.ast.push_pattern(PatternKind::Hole, item_node.span),
                        ExprKind::Spread(argument) => {
                            if index + 1 != items.len() || trailing {
                                return Err(ParseError::syntax(
                                    item_node.span.start,
                                    messages::REST_NOT_LAST,
                                ));
                            }
                            let target = self.binding_from_expr(argument, Leaf::Plain)?;
                            self.ast
                                .push_pattern(PatternKind::Rest(target), item_node.span)
                        }
                        _ => self.arrow_param(item, Leaf::Plain)?,
                    };
                    elements.push(element);
                }
                PatternKind::Array(self.ast.push_patterns(&elements))
            }
            ExprKind::Object(list) => {
                let properties = self.ast.properties(list).to_vec();
                let mut converted = Vec::with_capacity(properties.len());
                let mut rest = None;
                for (index, property) in properties.iter().enumerate() {
                    let value = match property.kind {
                        PropertyKind::Init | PropertyKind::Proto => {
                            self.arrow_param(property.value, Leaf::Plain)?
                        }
                        PropertyKind::Shorthand => self.arrow_param(property.value, Leaf::Named)?,
                        PropertyKind::Spread => {
                            if !matches!(
                                self.ast.expr(property.value).kind,
                                ExprKind::Identifier(_)
                            ) {
                                let value = self.ast.expr(property.value);
                                let message = if is_property_target(value.kind) {
                                    messages::PROPERTY_IN_DECLARATION
                                } else {
                                    messages::REST_NOT_IDENTIFIER
                                };
                                return Err(ParseError::syntax(value.span.start, message));
                            }
                            if index + 1 != properties.len() || trailing {
                                return Err(ParseError::syntax(
                                    self.last_token_start(property.value),
                                    messages::REST_NOT_LAST,
                                ));
                            }
                            rest = Some(self.binding_from_expr(property.value, Leaf::Plain)?);
                            break;
                        }
                        PropertyKind::Method | PropertyKind::Getter | PropertyKind::Setter => {
                            return Err(ParseError::syntax(
                                property.span.start,
                                messages::INVALID_DESTRUCTURING_TARGET,
                            ));
                        }
                    };
                    converted.push(PatternProperty {
                        key: property.key,
                        value,
                        span: property.span,
                    });
                }
                let properties = self.ast.push_pattern_properties(&converted);
                PatternKind::Object { properties, rest }
            }
            _ => {
                return Err(ParseError::syntax(
                    node.span.start,
                    messages::INVALID_DESTRUCTURING_TARGET,
                ));
            }
        };
        Ok(self.ast.push_pattern(kind, node.span))
    }

    /// Changes an assignment pattern (`[a]` of `([a] = b) => 0`) into a
    /// binding pattern in place: its identifier targets become bindings.
    /// Uses an explicit stack (the pattern's depth was charged when it
    /// was converted).
    fn binding_from_pattern(&mut self, root: PatternId) -> PResult<PatternId> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let pattern = *self.ast.pattern(id);
            match pattern.kind {
                PatternKind::Expr(expr) => {
                    let node = *self.ast.expr(expr);
                    let ExprKind::Identifier(ident) = node.kind else {
                        return Err(ParseError::syntax(
                            pattern.span.start,
                            messages::PROPERTY_IN_DECLARATION,
                        ));
                    };
                    if node.span != pattern.span {
                        // Parenthesized: `([(a)] = b) => 0`.
                        return Err(ParseError::syntax(
                            pattern.span.start,
                            messages::INVALID_DESTRUCTURING_TARGET,
                        ));
                    }
                    let ident = self.declare_cover_name(ident, pattern.span.start, Leaf::Plain)?;
                    if let Some(node) = self.ast.pattern_mut(id) {
                        node.kind = PatternKind::Identifier(ident);
                    }
                }
                PatternKind::Array(list) => {
                    stack.extend(self.ast.patterns(list).iter().rev());
                }
                PatternKind::Object { properties, rest } => {
                    stack.extend(rest);
                    stack.extend(
                        self.ast
                            .pattern_properties(properties)
                            .iter()
                            .rev()
                            .map(|p| p.value),
                    );
                }
                PatternKind::Default { target, .. } | PatternKind::Rest(target) => {
                    stack.push(target);
                }
                PatternKind::Identifier(_) | PatternKind::Hole => {}
            }
        }
        Ok(root)
    }

    /// Declares an identifier of an arrow function's parameters, which
    /// was recorded as a reference.
    fn declare_cover_name(&mut self, ident: Ident, offset: u32, leaf: Leaf) -> PResult<Ident> {
        if self.ctx.strict && super::is_eval_or_arguments(ident.name) {
            let message = if leaf == Leaf::Plain {
                messages::INVALID_DESTRUCTURING_TARGET
            } else {
                messages::UNEXPECTED_EVAL_OR_ARGUMENTS
            };
            return Err(ParseError::syntax(offset, message));
        }
        self.declare_param_name(ident.name, offset);
        if let Some(reference) = self.scopes.reference_mut(ident.reference) {
            reference.declaration = true;
        }
        Ok(ident)
    }

    // --- Properties of parameter lists ---

    /// Whether a parameter list has expressions: an initializer or a
    /// computed key anywhere (`ContainsExpression`, §8.4.2).
    pub(super) fn has_parameter_expressions(&self, params: List<PatternId>) -> bool {
        let mut stack: Vec<PatternId> = self.ast.patterns(params).to_vec();
        while let Some(id) = stack.pop() {
            match self.ast.pattern(id).kind {
                PatternKind::Default { .. } => return true,
                PatternKind::Array(list) => stack.extend(self.ast.patterns(list)),
                PatternKind::Object { properties, rest } => {
                    for property in self.ast.pattern_properties(properties) {
                        if matches!(property.key, crate::ast::PropertyKey::Computed(_)) {
                            return true;
                        }
                        stack.push(property.value);
                    }
                    stack.extend(rest);
                }
                PatternKind::Rest(target) => stack.push(target),
                PatternKind::Identifier(_) | PatternKind::Expr(_) | PatternKind::Hole => {}
            }
        }
        false
    }

    /// Whether a parameter list is simple: identifiers only (§15.1.3).
    pub(super) fn is_simple_parameter_list(&self, params: List<PatternId>) -> bool {
        self.ast
            .patterns(params)
            .iter()
            .all(|&p| matches!(self.ast.pattern(p).kind, PatternKind::Identifier(_)))
    }
}
