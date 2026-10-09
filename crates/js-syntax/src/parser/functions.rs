//! Functions (ECMA-262 clause 15): declarations, expressions, methods and
//! arrow functions, their parameters and bodies.

use swb_js_text::CodeUnit;

use super::{Context, IdentUse, PResult, Parser};
use crate::ast::{
    AssignOp, AssignTarget, ExprId, ExprKind, Function, FunctionId, FunctionKind, Ident, List,
    PatternId, PatternKind, ScopeId, Span, StmtId, StmtKind,
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
struct FunctionHead {
    kind: FunctionKind,
    generator: bool,
    is_declaration: bool,
    /// The binding identifier and its offset.
    name: Option<(Ident, u32)>,
    /// The source start of the function.
    start: u32,
}

/// The parts of a parsed function.
#[derive(Clone, Copy)]
struct FunctionParts {
    params: List<PatternId>,
    body: List<StmtId>,
    expression_body: bool,
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    /// Adds a function record whose content is filled in when the parse
    /// of the function ends.
    fn begin_function(&mut self, start: u32) -> FunctionId {
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
            parent: Some(self.ctx.function),
        });
        self.scopes.add_function();
        id
    }

    /// `FunctionDeclaration` and `GeneratorDeclaration` (§15.2, §15.5). The
    /// current token is `function`.
    pub(super) fn parse_function_declaration(&mut self) -> PResult<FunctionId> {
        let start = self.token.start;
        self.advance()?;
        let generator = self.eat(TokenKind::Star)?;
        if !self.at_identifier() {
            if self.at(TokenKind::LParen) {
                return Err(ParseError::syntax(start, messages::FUNCTION_NAME_REQUIRED));
            }
            return Err(self.unexpected());
        }
        // The name is bound in the enclosing scope, with its rules.
        let name_offset = self.token.start;
        let name = self.check_identifier(IdentUse::Binding)?;
        let function = self.begin_function(start);
        let declared =
            self.scopes
                .declare_function(self.scope, name, name_offset, function, !self.ctx.strict);
        self.declared(declared, name, start)?;
        let ident = self.reference(name, name_offset, true);
        self.advance()?;
        let head = FunctionHead {
            kind: FunctionKind::Normal,
            generator,
            is_declaration: true,
            name: Some((ident, name_offset)),
            start,
        };
        self.parse_function_rest(function, head, self.scope)?;
        Ok(function)
    }

    /// `FunctionExpression` and `GeneratorExpression` (§15.2, §15.5). A name
    /// is bound in its own scope, with the rules of the function itself.
    pub(super) fn parse_function_expression(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let generator = self.eat(TokenKind::Star)?;
        let function = self.begin_function(start);
        let mut parent_scope = self.scope;
        let mut name = None;
        if self.at_identifier() {
            let offset = self.token.start;
            let outer_generator = std::mem::replace(&mut self.ctx.generator, generator);
            let checked = self.check_identifier(IdentUse::Binding);
            self.ctx.generator = outer_generator;
            let checked = checked?;
            parent_scope =
                self.scopes
                    .push_scope(ScopeKind::FunctionName, Some(self.scope), function, offset);
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

    /// A method of an object literal (§15.4); the current token is `(`.
    pub(super) fn parse_method(&mut self, start: u32, generator: bool) -> PResult<ExprId> {
        let function = self.begin_function(start);
        let head = FunctionHead {
            kind: FunctionKind::Method,
            generator,
            is_declaration: false,
            name: None,
            start,
        };
        self.parse_function_rest(function, head, self.scope)?;
        Ok(self.ast.push_expr(
            ExprKind::Function(function),
            Span::new(start, self.prev_end),
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
        let scope = self.scopes.push_scope(
            ScopeKind::Function,
            Some(parent_scope),
            function,
            self.token.start,
        );
        let outer_scope = std::mem::replace(&mut self.scope, scope);
        let mut context = Context::new(
            function,
            head.kind,
            self.ctx.strict,
            head.generator,
            self.labels.len(),
        );
        context.name = head.name.map(|(ident, offset)| (ident.name, offset));
        let outer = std::mem::replace(&mut self.ctx, context);
        let parts = match self.charge(super::FUNCTION_WEIGHT) {
            Ok(()) => {
                let parts = self.parse_params_and_body(function, head.kind);
                self.budget.leave(super::FUNCTION_WEIGHT);
                parts
            }
            Err(error) => Err(error),
        };
        let strict = self.ctx.strict;
        self.ctx = outer;
        self.scope = outer_scope;
        let parts = parts?;
        self.finish_function(function, head, parts, strict, scope);
        Ok(())
    }

    fn finish_function(
        &mut self,
        function: FunctionId,
        head: FunctionHead,
        parts: FunctionParts,
        strict: bool,
        scope: ScopeId,
    ) {
        let end = self.prev_end;
        if let Some(record) = self.ast.function_mut(function) {
            record.kind = head.kind;
            record.is_generator = head.generator;
            record.strict = strict;
            record.is_declaration = head.is_declaration;
            record.expression_body = parts.expression_body;
            record.name = head.name.map(|(ident, _)| ident);
            record.params = parts.params;
            record.body = parts.body;
            record.span = Span::new(head.start, end);
            record.scope = scope;
        }
    }

    fn parse_params_and_body(
        &mut self,
        function: FunctionId,
        kind: FunctionKind,
    ) -> PResult<FunctionParts> {
        let params = self.parse_params(function, kind)?;
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let body = self.parse_function_body()?;
        Ok(FunctionParts {
            params,
            body,
            expression_body: false,
        })
    }

    /// `{ FunctionBody }` with its directive prologue.
    fn parse_function_body(&mut self) -> PResult<List<StmtId>> {
        self.expect(TokenKind::LBrace)?;
        let body = self.parse_statements(TokenKind::RBrace, true)?;
        self.expect(TokenKind::RBrace)?;
        Ok(body)
    }

    /// `FormalParameters` (§15.1): identifiers only in the subset.
    fn parse_params(
        &mut self,
        function: FunctionId,
        kind: FunctionKind,
    ) -> PResult<List<PatternId>> {
        self.expect(TokenKind::LParen)?;
        let mark = self.scratch_patterns.len();
        while !self.at(TokenKind::RParen) {
            let start = self.token.start;
            match self.token.kind {
                TokenKind::Ellipsis => {
                    return Err(ParseError::unsupported(start, "rest parameter"));
                }
                TokenKind::LBracket | TokenKind::LBrace => {
                    return Err(ParseError::unsupported(start, "destructuring"));
                }
                _ => {}
            }
            let name = self.check_identifier(IdentUse::Binding)?;
            let ident = self.reference(name, start, true);
            self.advance()?;
            if self.at(TokenKind::Eq) {
                return Err(ParseError::unsupported(start, "default parameter"));
            }
            let span = Span::new(start, self.prev_end);
            let pattern = self.declare_param(function, ident, span);
            self.scratch_patterns.push(pattern);
            if !self.at(TokenKind::RParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        if self.scratch_patterns.len() - mark > MAX_PARAMS {
            return Err(ParseError::syntax(
                self.prev_end,
                messages::TOO_MANY_PARAMETERS,
            ));
        }
        let params = self.ast.push_patterns(&self.scratch_patterns[mark..]);
        self.scratch_patterns.truncate(mark);
        self.ctx.params = params;
        // Methods and arrow functions have UniqueFormalParameters
        // (§15.4.1, §15.3.1); strict functions too (§15.2.1).
        let unique = self.ctx.strict || matches!(kind, FunctionKind::Method | FunctionKind::Arrow);
        if unique && let Some(offset) = self.ctx.duplicate_param {
            return Err(ParseError::syntax(offset, messages::DUPLICATE_PARAMETER));
        }
        Ok(params)
    }

    /// Declares a parameter of the current function and returns its
    /// pattern.
    fn declare_param(&mut self, function: FunctionId, ident: Ident, span: Span) -> PatternId {
        let (binding, duplicate) = self
            .scopes
            .declare_param(self.scope, ident.name, span.start);
        if duplicate && self.ctx.duplicate_param.is_none() {
            self.ctx.duplicate_param = Some(span.start);
        }
        if let Some(record) = self.scopes.function_mut(function) {
            record.params.push(binding);
        }
        self.ast.push_pattern(PatternKind::Identifier(ident), span)
    }

    /// Converts the expression before `=>` into arrow parameters (the
    /// cover grammar, §15.3.1): an identifier, or a parenthesized list of
    /// identifiers. `start` is where the expression starts; `marks` and
    /// `functions` are the table sizes before it.
    pub(super) fn parse_arrow_from_cover(
        &mut self,
        left: ExprId,
        start: u32,
        marks: (usize, usize),
        functions: usize,
        no_in: bool,
    ) -> PResult<ExprId> {
        let expr = *self.ast.expr(left);
        let mut params: Vec<(Ident, Span)> = Vec::new();
        match expr.kind {
            ExprKind::Identifier(ident) if expr.span.start == start => {
                params.push((ident, expr.span));
            }
            ExprKind::Paren(inner) if expr.span.start == start => {
                let items: Vec<ExprId> = match self.ast.expr(inner).kind {
                    ExprKind::Sequence(list) => self.ast.exprs(list).to_vec(),
                    _ => vec![inner],
                };
                for item in items {
                    params.push(self.arrow_param(item)?);
                }
            }
            ExprKind::Call { callee, .. }
                if matches!(self.ast.expr(callee).kind,
                    ExprKind::Identifier(ident) if ident.name == names::ASYNC) =>
            {
                return Err(ParseError::unsupported(start, "async arrow function"));
            }
            _ => {
                return Err(ParseError::syntax(
                    start,
                    messages::MALFORMED_ARROW_PARAMETERS,
                ));
            }
        }
        self.parse_arrow_function(start, &params, Some((marks, functions)), no_in)
    }

    /// One parameter of an arrow function, from its cover expression.
    fn arrow_param(&self, item: ExprId) -> PResult<(Ident, Span)> {
        let expr = self.ast.expr(item);
        match expr.kind {
            ExprKind::Identifier(ident) => Ok((ident, expr.span)),
            ExprKind::Assign {
                op: AssignOp::Assign,
                target: AssignTarget::Simple(target),
                ..
            } if matches!(self.ast.expr(target).kind, ExprKind::Identifier(_)) => Err(
                ParseError::unsupported(expr.span.start, "default parameter"),
            ),
            ExprKind::Array(_) | ExprKind::Object(_) => {
                Err(ParseError::unsupported(expr.span.start, "destructuring"))
            }
            _ => Err(ParseError::syntax(
                expr.span.start,
                messages::INVALID_DESTRUCTURING_TARGET,
            )),
        }
    }

    /// An arrow function (§15.3) from its parameters; the current token is
    /// `=>`. `cover` holds the table sizes before the parameters, whose
    /// identifier occurrences and nested scopes move into the arrow
    /// function's scope.
    pub(super) fn parse_arrow_function(
        &mut self,
        start: u32,
        params: &[(Ident, Span)],
        cover: Option<((usize, usize), usize)>,
        no_in: bool,
    ) -> PResult<ExprId> {
        let function = self.begin_function(start);
        let scope = self
            .scopes
            .push_scope(ScopeKind::Function, Some(self.scope), function, start);
        if let Some((marks, functions)) = cover {
            self.scopes.reparent_since(marks, self.scope, scope);
            let outer = self.ctx.function;
            for record in self.ast.functions_from(functions) {
                if record.parent == Some(outer) && record.kind != FunctionKind::Script {
                    record.parent = Some(function);
                }
            }
            // The arrow function itself is in that range too.
            if let Some(record) = self.ast.function_mut(function) {
                record.parent = Some(outer);
            }
        }
        let outer_scope = std::mem::replace(&mut self.scope, scope);
        let context = Context::new(
            function,
            FunctionKind::Arrow,
            self.ctx.strict,
            false,
            self.labels.len(),
        );
        let outer = std::mem::replace(&mut self.ctx, context);
        let parts = self.parse_arrow_rest(function, params, no_in);
        let strict = self.ctx.strict;
        self.ctx = outer;
        self.scope = outer_scope;
        let parts = parts?;
        let head = FunctionHead {
            kind: FunctionKind::Arrow,
            generator: false,
            is_declaration: false,
            name: None,
            start,
        };
        self.finish_function(function, head, parts, strict, scope);
        Ok(self.ast.push_expr(
            ExprKind::Function(function),
            Span::new(start, self.prev_end),
        ))
    }

    fn parse_arrow_rest(
        &mut self,
        function: FunctionId,
        params: &[(Ident, Span)],
        no_in: bool,
    ) -> PResult<FunctionParts> {
        if params.len() > MAX_PARAMS {
            return Err(ParseError::syntax(
                self.token.start,
                messages::TOO_MANY_PARAMETERS,
            ));
        }
        let mark = self.scratch_patterns.len();
        for &(ident, span) in params {
            if self.ctx.strict && super::is_eval_or_arguments(ident.name) {
                return Err(ParseError::syntax(
                    span.start,
                    messages::UNEXPECTED_EVAL_OR_ARGUMENTS,
                ));
            }
            let pattern = self.declare_param(function, ident, span);
            if self.ctx.duplicate_param.is_some() {
                return Err(ParseError::syntax(
                    span.start,
                    messages::DUPLICATE_PARAMETER,
                ));
            }
            if let Some(reference) = self.scopes.reference_mut(ident.reference) {
                reference.declaration = true;
            }
            self.scratch_patterns.push(pattern);
        }
        let list = self.ast.push_patterns(&self.scratch_patterns[mark..]);
        self.scratch_patterns.truncate(mark);
        self.ctx.params = list;
        self.expect(TokenKind::Arrow)?;
        if self.at(TokenKind::LBrace) {
            let body = self.parse_function_body()?;
            return Ok(FunctionParts {
                params: list,
                body,
                expression_body: false,
            });
        }
        let body_start = self.token.start;
        let value = self.parse_assignment(no_in)?;
        let stmt = self.ast.push_stmt(
            StmtKind::Return(Some(value)),
            Span::new(body_start, self.prev_end),
        );
        Ok(FunctionParts {
            params: list,
            body: self.ast.push_stmts(&[stmt]),
            expression_body: true,
        })
    }
}
