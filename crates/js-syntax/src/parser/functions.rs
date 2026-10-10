//! Functions (ECMA-262 clause 15): declarations, expressions, methods,
//! accessors and arrow functions (also `async` ones and async
//! generators, §15.6, §15.8, §15.9), their parameters and bodies.

use swb_js_text::CodeUnit;

use super::patterns::{DeclKind, Leaf};
use super::{AwaitMode, Context, IdentUse, PResult, Parser, Recorded};
use crate::ast::{
    ExprId, ExprKind, Function, FunctionId, FunctionKind, Ident, List, PatternId, PatternKind,
    RefId, ScopeId, Span, StmtId, StmtKind,
};
use crate::error::ParseError;
use crate::interner::names;
use crate::messages;
use crate::scope::ScopeKind;
use crate::token::TokenKind;

/// The largest number of parameters (Chromium's limit).
const MAX_PARAMS: usize = 65534;

/// What the parser knows about a function before its body.
#[derive(Clone, Copy)]
pub(super) struct FunctionHead {
    pub(super) kind: FunctionKind,
    pub(super) generator: bool,
    pub(super) is_async: bool,
    pub(super) is_declaration: bool,
    /// The binding identifier and its offset.
    pub(super) name: Option<(Ident, u32)>,
    /// The source start of the function.
    pub(super) start: u32,
}

impl FunctionHead {
    /// The head of a method-like function without a name.
    pub(super) fn method(kind: FunctionKind, generator: bool, is_async: bool, start: u32) -> Self {
        FunctionHead {
            kind,
            generator,
            is_async,
            is_declaration: false,
            name: None,
            start,
        }
    }
}

/// The parts of a parsed function.
#[derive(Clone, Copy)]
struct FunctionParts {
    params: List<PatternId>,
    body: List<StmtId>,
    expression_body: bool,
    body_scope: ScopeId,
}

/// The parameters of an arrow function, as parsed before the `=>`.
#[derive(Clone, Copy, Debug)]
pub(super) enum ArrowHead {
    /// `() =>`.
    Empty,
    /// `a =>`: the identifier expression.
    Identifier(ExprId),
    /// `(a, b = 1, ...c) =>`: the expression inside the parentheses (a
    /// [`ExprKind::Sequence`] for more than one element).
    Parenthesized(ExprId),
    /// `async (a, b) =>`: the arguments of the call `async(a, b)`
    /// (§15.9, `CoverCallExpressionAndAsyncArrowHead`).
    AsyncCall {
        /// The arguments.
        arguments: List<ExprId>,
        /// The occurrence of the callee `async`, which is not a reference.
        callee: RefId,
        /// The offset of the comma after the first spread argument.
        spread_comma: Option<u32>,
    },
}

/// How an arrow function was written.
#[derive(Clone, Copy, Debug)]
pub(super) struct ArrowStart {
    /// Where the arrow function starts (`async` or the parameters).
    pub(super) start: u32,
    /// The length of [`Parser::recorded`] before the parameters: what was
    /// recorded since then moves into the arrow function's scope.
    pub(super) mark: Option<usize>,
    /// Whether it is an async arrow function.
    pub(super) is_async: bool,
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    /// Adds a function record whose content is filled in when the parse
    /// of the function ends.
    pub(super) fn begin_function(&mut self, start: u32) -> FunctionId {
        let id = self.ast.push_function(Function {
            kind: FunctionKind::Normal,
            is_generator: false,
            is_async: false,
            strict: false,
            is_declaration: false,
            expression_body: false,
            name: None,
            params: List::EMPTY,
            body: List::EMPTY,
            span: Span::new(start, start),
            scope: ScopeId::from_index(0),
            body_scope: ScopeId::from_index(0),
            parent: Some(self.ctx.function),
        });
        self.scopes.add_function();
        self.recorded.push(Recorded::Function(id));
        id
    }

    /// `FunctionDeclaration`, `GeneratorDeclaration` and their async forms
    /// (§15.2, §15.5, §15.6, §15.8). The current token is `function`;
    /// `start` is where the declaration starts (`async` or `function`).
    pub(super) fn parse_function_declaration(
        &mut self,
        start: u32,
        is_async: bool,
    ) -> PResult<FunctionId> {
        self.parse_function_declaration_in(start, is_async, false)
    }

    /// The function declaration of `export default` (§16.2.3,
    /// `HoistableDeclaration[+Default]`): without a name it binds
    /// `*default*`.
    pub(super) fn parse_function_declaration_default(
        &mut self,
        start: u32,
        is_async: bool,
    ) -> PResult<FunctionId> {
        self.parse_function_declaration_in(start, is_async, true)
    }

    fn parse_function_declaration_in(
        &mut self,
        start: u32,
        is_async: bool,
        default_export: bool,
    ) -> PResult<FunctionId> {
        let function_token = self.token.start;
        self.advance()?;
        let generator = self.eat(TokenKind::Star)?;
        let anonymous = default_export && self.at(TokenKind::LParen);
        if !self.at_identifier() && !anonymous {
            if self.at(TokenKind::LParen) {
                return Err(ParseError::syntax(
                    function_token,
                    messages::FUNCTION_NAME_REQUIRED,
                ));
            }
            return Err(self.unexpected());
        }
        // The name is bound in the enclosing scope, with its rules.
        let name_offset = self.token.start;
        let name = if anonymous {
            names::DEFAULT_EXPORT
        } else {
            self.check_identifier(IdentUse::Name)?
        };
        let function = self.begin_function(start);
        let plain_sloppy = !self.ctx.strict && !generator && !is_async;
        let declared =
            self.scopes
                .declare_function(self.scope, name, name_offset, function, plain_sloppy);
        self.declared(declared, name, start)?;
        let ident = self.reference(name, name_offset, true);
        if !anonymous {
            self.advance()?;
        }
        let head = FunctionHead {
            kind: FunctionKind::Normal,
            generator,
            is_async,
            is_declaration: true,
            name: Some((ident, name_offset)),
            start,
        };
        self.parse_function_rest(function, head, self.scope)?;
        Ok(function)
    }

    /// The `await` rules of a function's own code: an `AwaitExpression` in
    /// async functions; otherwise an identifier, but a reserved word in
    /// module code (§12.7.2).
    pub(super) fn function_await_mode(&self, is_async: bool) -> AwaitMode {
        if is_async {
            AwaitMode::Expression
        } else if self.module_code {
            AwaitMode::Reserved
        } else {
            AwaitMode::Identifier
        }
    }

    /// `FunctionExpression`, `GeneratorExpression` and their async forms
    /// (§15.2, §15.5, §15.6, §15.8). A name is bound in its own scope,
    /// with the `yield` and `await` rules of the function itself. The
    /// current token is `function`; `start` is where the expression starts.
    pub(super) fn parse_function_expression(
        &mut self,
        start: u32,
        is_async: bool,
    ) -> PResult<ExprId> {
        self.advance()?;
        let generator = self.eat(TokenKind::Star)?;
        let function = self.begin_function(start);
        let mut parent_scope = self.scope;
        let mut name = None;
        if self.at_identifier() {
            let offset = self.token.start;
            let outer_generator = std::mem::replace(&mut self.ctx.generator, generator);
            let own_await = self.function_await_mode(is_async);
            let outer_await = std::mem::replace(&mut self.ctx.await_mode, own_await);
            let checked = self.check_identifier(IdentUse::Name);
            self.ctx.generator = outer_generator;
            self.ctx.await_mode = outer_await;
            let checked = checked?;
            parent_scope = self.new_scope_of(ScopeKind::FunctionName, self.scope, function, offset);
            self.scopes
                .declare_function_name(parent_scope, checked, offset);
            let reference = self
                .scopes
                .push_reference(checked, parent_scope, offset, true);
            name = Some((
                Ident {
                    name: checked,
                    reference,
                },
                offset,
            ));
            self.advance()?;
        }
        let head = FunctionHead {
            kind: FunctionKind::Normal,
            generator,
            is_async,
            is_declaration: false,
            name,
            start,
        };
        self.parse_function_rest(function, head, parent_scope)?;
        Ok(self.ast.push_expr(
            ExprKind::Function(function),
            Span::new(start, self.prev_end),
        ))
    }

    /// A method, getter, setter or class constructor (§15.4, §15.7); the
    /// current token is `(`.
    pub(super) fn parse_method(&mut self, head: FunctionHead) -> PResult<ExprId> {
        let function = self.begin_function(head.start);
        self.parse_function_rest(function, head, self.scope)?;
        Ok(self.ast.push_expr(
            ExprKind::Function(function),
            Span::new(head.start, self.prev_end),
        ))
    }

    /// The parameters and the body of a function whose scope is a child
    /// of `parent_scope`; fills in the function record.
    fn parse_function_rest(
        &mut self,
        function: FunctionId,
        head: FunctionHead,
        parent_scope: ScopeId,
    ) -> PResult<()> {
        let scope = self.new_scope_of(
            ScopeKind::Function,
            parent_scope,
            function,
            self.token.start,
        );
        let entered = self.enter_scope(scope);
        let mut context = Context::new(
            function,
            head.kind,
            self.ctx.strict,
            head.generator,
            self.labels.len(),
        );
        context.name = head.name.map(|(ident, offset)| (ident.name, offset));
        context.await_mode = self.function_await_mode(head.is_async);
        let outer = std::mem::replace(&mut self.ctx, context);
        let parts = match self.charge(super::FUNCTION_WEIGHT) {
            Ok(()) => {
                let parts = self.parse_params_and_body(head.kind);
                self.budget.leave(super::FUNCTION_WEIGHT);
                parts
            }
            Err(error) => Err(error),
        };
        let inner = std::mem::replace(&mut self.ctx, outer);
        self.leave_scope(entered);
        let parts = parts?;
        self.finish_function(function, head, parts, &inner, scope);
        Ok(())
    }

    fn finish_function(
        &mut self,
        function: FunctionId,
        head: FunctionHead,
        parts: FunctionParts,
        inner: &Context,
        scope: ScopeId,
    ) {
        let end = self.prev_end;
        if let Some(record) = self.ast.function_mut(function) {
            record.kind = head.kind;
            record.is_generator = head.generator;
            record.is_async = head.is_async;
            record.strict = inner.strict;
            record.is_declaration = head.is_declaration;
            record.expression_body = parts.expression_body;
            record.name = head.name.map(|(ident, _)| ident);
            record.params = parts.params;
            record.body = parts.body;
            record.span = Span::new(head.start, end);
            record.scope = scope;
            record.body_scope = parts.body_scope;
        }
        if let Some(record) = self.scopes.function_mut(function) {
            record.has_direct_eval = inner.direct_evals > 0;
            record.sloppy_body_eval = inner.sloppy_evals > inner.sloppy_param_evals;
            record.sloppy_param_eval = inner.sloppy_param_evals > 0;
        }
    }

    fn parse_params_and_body(&mut self, kind: FunctionKind) -> PResult<FunctionParts> {
        let params = self.parse_params(kind)?;
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let (body, body_scope) = self.parse_body_in_scope(params, Self::parse_function_body)?;
        Ok(FunctionParts {
            params,
            body,
            expression_body: false,
            body_scope,
        })
    }

    /// Parses the body with `parse`, in a scope of its own if the
    /// parameters have expressions (§10.2.11 step 28). Returns the body
    /// and its scope.
    fn parse_body_in_scope(
        &mut self,
        params: List<PatternId>,
        parse: impl FnOnce(&mut Self) -> PResult<List<StmtId>>,
    ) -> PResult<(List<StmtId>, ScopeId)> {
        if !self.has_parameter_expressions(params) {
            return Ok((parse(self)?, self.scope));
        }
        let entered = self.open_scope(ScopeKind::FunctionBody, self.token.start);
        let body_scope = self.scope;
        let body = parse(self);
        self.leave_scope(entered);
        Ok((body?, body_scope))
    }

    /// `{ FunctionBody }` with its directive prologue.
    fn parse_function_body(&mut self) -> PResult<List<StmtId>> {
        self.expect(TokenKind::LBrace)?;
        let body = self.parse_statements(TokenKind::RBrace, true)?;
        self.expect(TokenKind::RBrace)?;
        Ok(body)
    }

    /// `FormalParameters` (§15.1): binding elements and a rest parameter.
    fn parse_params(&mut self, kind: FunctionKind) -> PResult<List<PatternId>> {
        let open = self.token.start;
        self.expect(TokenKind::LParen)?;
        self.ctx.in_params = true;
        let mark = self.scratch_patterns.len();
        let marks = self.scratch_param_marks.len();
        let result = self.parse_param_elements();
        self.ctx.in_params = false;
        let params = self.ast.push_patterns(&self.scratch_patterns[mark..]);
        self.scratch_patterns.truncate(mark);
        let element_marks: Vec<(usize, u32)> = self.scratch_param_marks.drain(marks..).collect();
        result?;
        self.finish_params(params, kind, &element_marks, open)?;
        Ok(params)
    }

    /// The elements of a parameter list up to and including `)`.
    fn parse_param_elements(&mut self) -> PResult<()> {
        while !self.at(TokenKind::RParen) {
            let start = self.token.start;
            let binding_mark = self.scopes.binding_mark();
            if self.eat(TokenKind::Ellipsis)? {
                let target = self.parse_binding_target(DeclKind::Param, Leaf::Top)?;
                if self.at(TokenKind::Eq) {
                    return Err(ParseError::syntax(
                        self.token.start,
                        messages::REST_PARAMETER_DEFAULT,
                    ));
                }
                let rest = self
                    .ast
                    .push_pattern(PatternKind::Rest(target), Span::new(start, self.prev_end));
                self.scratch_patterns.push(rest);
                self.scratch_param_marks.push((binding_mark, self.prev_end));
                if !self.at(TokenKind::RParen) {
                    if self.at(TokenKind::Comma) {
                        return Err(ParseError::syntax(
                            self.token.start,
                            messages::REST_PARAMETER_NOT_LAST,
                        ));
                    }
                    return Err(self.unexpected());
                }
                break;
            }
            let element = self.parse_binding_element(DeclKind::Param, Leaf::Top)?;
            self.scratch_patterns.push(element);
            self.scratch_param_marks.push((binding_mark, self.prev_end));
            if !self.at(TokenKind::RParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()
    }

    /// The checks and records of a complete parameter list of the current
    /// function: the count, simple or not, duplicates (§15.1.1, §15.2.1),
    /// the arity of accessors (§15.4.1), the registers of the arguments,
    /// and the temporal dead zone of parameters when the list has
    /// expressions. `element_marks` has, per element, the binding mark
    /// before it and its end.
    fn finish_params(
        &mut self,
        params: List<PatternId>,
        kind: FunctionKind,
        element_marks: &[(usize, u32)],
        open: u32,
    ) -> PResult<()> {
        let patterns = self.ast.patterns(params);
        if patterns.len() > MAX_PARAMS {
            return Err(ParseError::syntax(
                self.prev_end,
                messages::TOO_MANY_PARAMETERS,
            ));
        }
        let has_rest = patterns
            .last()
            .is_some_and(|&p| matches!(self.ast.pattern(p).kind, PatternKind::Rest(_)));
        match kind {
            FunctionKind::Getter if !patterns.is_empty() => {
                return Err(ParseError::syntax(open, messages::GETTER_PARAMETERS));
            }
            FunctionKind::Setter if has_rest => {
                return Err(ParseError::syntax(open, messages::SETTER_REST));
            }
            FunctionKind::Setter if patterns.len() != 1 => {
                return Err(ParseError::syntax(open, messages::SETTER_PARAMETERS));
            }
            _ => {}
        }
        let simple = self.is_simple_parameter_list(params);
        let positional: Vec<_> = if simple {
            patterns
                .iter()
                .filter_map(|&p| match self.ast.pattern(p).kind {
                    PatternKind::Identifier(ident) => {
                        self.scopes.declared_in(self.scope, ident.name)
                    }
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        let count = patterns.len() as u32;
        if let Some(record) = self.scopes.function_mut(self.ctx.function) {
            record.params = positional;
            record.argument_registers = count;
        }
        self.ctx.params = params;
        self.ctx.simple_params = simple;
        // Methods, accessors and arrow functions have
        // UniqueFormalParameters (§15.4.1, §15.3.1); strict functions and
        // non-simple lists too (§15.2.1).
        let unique =
            self.ctx.strict || !simple || kind.has_home_object() || kind == FunctionKind::Arrow;
        if unique && let Some(offset) = self.ctx.duplicate_param {
            return Err(ParseError::syntax(offset, messages::DUPLICATE_PARAMETER));
        }
        if self.has_parameter_expressions(params) {
            let all = self.scopes.binding_mark();
            for (index, &(mark, end)) in element_marks.iter().enumerate() {
                let next = element_marks.get(index + 1).map_or(all, |&(m, _)| m);
                self.scopes
                    .set_init_end_range(mark..next, self.scope, true, end);
            }
        }
        Ok(())
    }

    /// Converts the expression before `=>` into arrow parameters (the
    /// cover grammar, §15.3.1, §15.9.1): an identifier, a parenthesized
    /// list, or the arguments of a call `async(...)`. `start` is where the
    /// expression starts; `mark` is the length of [`Parser::recorded`]
    /// before it.
    pub(super) fn parse_arrow_from_cover(
        &mut self,
        left: ExprId,
        start: u32,
        mark: usize,
        no_in: bool,
    ) -> PResult<ExprId> {
        let expr = *self.ast.expr(left);
        let mut is_async = false;
        let head = match expr.kind {
            ExprKind::Identifier(_) if expr.span.start == start => ArrowHead::Identifier(left),
            ExprKind::Paren(inner) if expr.span.start == start => ArrowHead::Parenthesized(inner),
            ExprKind::Call {
                callee, arguments, ..
            } if expr.span.start == start
                && let Some(&spread_comma) = self.async_heads.get(&left)
                && let ExprKind::Identifier(callee) = self.ast.expr(callee).kind =>
            {
                is_async = true;
                ArrowHead::AsyncCall {
                    arguments,
                    callee: callee.reference,
                    spread_comma,
                }
            }
            _ => {
                return Err(ParseError::syntax(
                    start,
                    messages::MALFORMED_ARROW_PARAMETERS,
                ));
            }
        };
        let arrow = ArrowStart {
            start,
            mark: Some(mark),
            is_async,
        };
        self.parse_arrow_function(arrow, head, no_in)
    }

    /// `async x => ...` (§15.9); the current token is the parameter (after
    /// `async`, without a line terminator in between).
    pub(super) fn parse_async_arrow_identifier(
        &mut self,
        start: u32,
        no_in: bool,
    ) -> PResult<ExprId> {
        let mark = self.recorded.len();
        let param_token = self.token.clone();
        let param = self.parse_identifier_reference()?;
        if !self.at(TokenKind::Arrow) {
            return Err(self.unexpected_token(&param_token));
        }
        if self.token.newline_before {
            return Err(self.unexpected());
        }
        let arrow = ArrowStart {
            start,
            mark: Some(mark),
            is_async: true,
        };
        self.parse_arrow_function(arrow, ArrowHead::Identifier(param), no_in)
    }

    /// An arrow function (§15.3, §15.9) from its parameters; the current
    /// token is `=>`. What was recorded since `arrow.mark` moves into the
    /// arrow function's scope.
    pub(super) fn parse_arrow_function(
        &mut self,
        arrow: ArrowStart,
        head: ArrowHead,
        no_in: bool,
    ) -> PResult<ExprId> {
        let start = arrow.start;
        // `(a = yield) => 0` in a generator, `(a = await 1) => 0` in an
        // async function (§15.3.1, §15.9.1).
        let head_yield = self.ctx.last_yield.filter(|&offset| offset >= start);
        let head_await = self.ctx.last_await.filter(|&offset| offset >= start);
        let moved = arrow.mark.map_or_else(Vec::new, |mark| {
            self.recorded.split_off(mark.min(self.recorded.len()))
        });
        let callee = match head {
            ArrowHead::AsyncCall { callee, .. } => Some(callee),
            _ => None,
        };
        let outer_function = self.ctx.function;
        let outer_scope = self.scope;
        let function = self.begin_function(start);
        let scope = self.new_scope_of(ScopeKind::Function, outer_scope, function, start);
        let mut evals = MovedEvals::default();
        // An async arrow function's parameters cannot use `await` as an
        // identifier (§15.9: `[+Await]`).
        let mut await_name = None;
        let nested_await_name = self.ctx.last_await_name.filter(|&offset| offset >= start);
        for item in moved {
            match item {
                Recorded::Reference(id) if Some(id) == callee => self.scopes.mark_dead(id),
                Recorded::Reference(id) => {
                    if arrow.is_async && await_name.is_none() {
                        let reference = self.scopes.reference(id);
                        if reference.name == names::AWAIT && reference.scope == outer_scope {
                            await_name = Some(reference.offset);
                        }
                    }
                    self.scopes.move_reference(id, outer_scope, scope);
                }
                Recorded::Scope(id) => self.scopes.move_scope(id, outer_scope, scope),
                Recorded::Function(id) => {
                    if let Some(record) = self.ast.function_mut(id)
                        && record.parent == Some(outer_function)
                    {
                        record.parent = Some(function);
                    }
                }
                Recorded::Eval { sloppy, in_params } => evals.add(sloppy, in_params),
            }
        }
        evals.take_from(&mut self.ctx);
        let entered = self.enter_scope(scope);
        let mut context = self.ctx.arrow(function, self.labels.len(), arrow.is_async);
        context.await_mode = self.function_await_mode(arrow.is_async);
        // The moved calls were in the arrow function's parameters.
        context.direct_evals = evals.all;
        context.sloppy_evals = evals.sloppy;
        context.sloppy_param_evals = evals.sloppy;
        let outer = std::mem::replace(&mut self.ctx, context);
        let checks = HeadChecks {
            yield_at: head_yield,
            await_at: head_await,
            await_name: await_name.or(nested_await_name.filter(|_| arrow.is_async)),
        };
        let parts = self.parse_arrow_rest(head, checks, no_in);
        let inner = std::mem::replace(&mut self.ctx, outer);
        self.leave_scope(entered);
        let parts = parts?;
        let head = FunctionHead {
            kind: FunctionKind::Arrow,
            generator: false,
            is_async: arrow.is_async,
            is_declaration: false,
            name: None,
            start,
        };
        self.finish_function(function, head, parts, &inner, scope);
        Ok(self.ast.push_expr(
            ExprKind::Function(function),
            Span::new(start, self.prev_end),
        ))
    }

    fn parse_arrow_rest(
        &mut self,
        head: ArrowHead,
        checks: HeadChecks,
        no_in: bool,
    ) -> PResult<FunctionParts> {
        let params = self.arrow_params(head)?;
        if let Some(offset) = checks.await_name {
            return Err(ParseError::syntax(offset, messages::AWAIT_BINDING_IN_ASYNC));
        }
        if let Some(offset) = checks.yield_at {
            return Err(ParseError::syntax(offset, messages::YIELD_IN_PARAMETER));
        }
        if let Some(offset) = checks.await_at {
            return Err(ParseError::syntax(offset, messages::AWAIT_IN_PARAMETER));
        }
        self.expect(TokenKind::Arrow)?;
        if self.at(TokenKind::LBrace) {
            let (body, body_scope) = self.parse_body_in_scope(params, Self::parse_function_body)?;
            return Ok(FunctionParts {
                params,
                body,
                expression_body: false,
                body_scope,
            });
        }
        let (body, body_scope) = self.parse_body_in_scope(params, |parser| {
            let body_start = parser.token.start;
            let value = parser.parse_assignment(no_in)?;
            let stmt = parser.ast.push_stmt(
                StmtKind::Return(Some(value)),
                Span::new(body_start, parser.prev_end),
            );
            Ok(parser.ast.push_stmts(&[stmt]))
        })?;
        Ok(FunctionParts {
            params,
            body,
            expression_body: true,
            body_scope,
        })
    }

    /// Converts and declares the parameters of an arrow function; the
    /// current scope is the arrow's.
    fn arrow_params(&mut self, head: ArrowHead) -> PResult<List<PatternId>> {
        let is_async = matches!(head, ArrowHead::AsyncCall { .. });
        let items: Vec<ExprId> = match head {
            ArrowHead::Empty => Vec::new(),
            ArrowHead::Identifier(item) => vec![item],
            ArrowHead::Parenthesized(inner) => match self.ast.expr(inner).kind {
                ExprKind::Sequence(list) => self.ast.exprs(list).to_vec(),
                _ => vec![inner],
            },
            ArrowHead::AsyncCall {
                arguments,
                spread_comma,
                ..
            } => {
                // A spread argument of `async(...)` is a rest parameter:
                // only the last, without a trailing comma. V8 marks the
                // comma after it.
                if let Some(comma) = spread_comma {
                    return Err(ParseError::syntax(comma, messages::REST_PARAMETER_NOT_LAST));
                }
                self.ast.exprs(arguments).to_vec()
            }
        };
        if items.len() > MAX_PARAMS {
            return Err(ParseError::syntax(
                self.token.start,
                messages::TOO_MANY_PARAMETERS,
            ));
        }
        let mut patterns = Vec::with_capacity(items.len());
        let mut element_marks = Vec::with_capacity(items.len());
        for item in items {
            let node = *self.ast.expr(item);
            let binding_mark = self.scopes.binding_mark();
            let pattern = match node.kind {
                ExprKind::Spread(argument) => {
                    if let ExprKind::Assign { value, .. } = self.ast.expr(argument).kind {
                        if is_async {
                            // V8 marks the end of the initializer here.
                            return Err(ParseError::syntax(
                                self.last_token_start(value),
                                messages::REST_PARAMETER_DEFAULT,
                            ));
                        }
                        return Err(self.rest_parameter_error(argument));
                    }
                    let target = self.binding_from_expr(argument, Leaf::Top)?;
                    self.ast.push_pattern(PatternKind::Rest(target), node.span)
                }
                _ => self.arrow_param(item, Leaf::Top)?,
            };
            patterns.push(pattern);
            element_marks.push((binding_mark, node.span.end));
        }
        let params = self.ast.push_patterns(&patterns);
        self.finish_params(
            params,
            FunctionKind::Arrow,
            &element_marks,
            self.token.start,
        )?;
        Ok(params)
    }
}

/// The direct `eval` calls that move from the enclosing function into an
/// arrow function's parameters.
#[derive(Clone, Copy, Debug, Default)]
struct MovedEvals {
    all: u32,
    sloppy: u32,
    /// Those that were in the enclosing function's own parameter list.
    sloppy_in_params: u32,
}

impl MovedEvals {
    fn add(&mut self, sloppy: bool, in_params: bool) {
        self.all += 1;
        if sloppy {
            self.sloppy += 1;
            if in_params {
                self.sloppy_in_params += 1;
            }
        }
    }

    /// Takes the calls out of the enclosing function's counts.
    fn take_from(self, outer: &mut Context) {
        outer.direct_evals = outer.direct_evals.saturating_sub(self.all);
        outer.sloppy_evals = outer.sloppy_evals.saturating_sub(self.sloppy);
        outer.sloppy_param_evals = outer
            .sloppy_param_evals
            .saturating_sub(self.sloppy_in_params);
    }
}

/// The early errors of an arrow function's head that the parameter
/// conversion does not see: the offsets of a `yield` expression, an
/// `await` expression, and (async arrow functions) an `await` identifier.
#[derive(Clone, Copy, Debug)]
struct HeadChecks {
    yield_at: Option<u32>,
    await_at: Option<u32>,
    await_name: Option<u32>,
}
