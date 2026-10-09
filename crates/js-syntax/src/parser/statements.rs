//! Statements and declarations (ECMA-262 clause 14), the directive
//! prologue (§11.2.1) and automatic semicolon insertion (§12.10).

use swb_js_text::CodeUnit;

use super::expressions::is_property_target;
use super::patterns::{DeclKind, Leaf};
use super::{AwaitMode, IdentUse, Label, PResult, Parser, StmtContext, legacy_error};
use crate::ast::{
    AssignTarget, CatchClause, Declarator, ExprId, ExprKind, ForHead, List, PatternKind, Span,
    StmtId, StmtKind, SwitchCase, VariableKind,
};
use crate::error::ParseError;
use crate::interner::names;
use crate::messages;
use crate::scope::ScopeKind;
use crate::token::{Legacy, TokenKind};

impl<U: CodeUnit> Parser<'_, '_, U> {
    /// A statement list up to `end` (not consumed). With `directives`, the
    /// list starts with a directive prologue (§11.2.1).
    pub(super) fn parse_statements(
        &mut self,
        end: TokenKind,
        directives: bool,
    ) -> PResult<List<StmtId>> {
        let mark = self.scratch_stmts.len();
        let mut prologue = directives;
        // The first legacy octal string of the prologue: an error if a
        // later directive makes the code strict.
        let mut prologue_legacy: Option<(u32, Legacy)> = None;
        while !self.at(end) {
            if self.at(TokenKind::Eof) {
                return Err(self.unexpected());
            }
            self.recorded.truncate(self.scope_base);
            if prologue && !self.at(TokenKind::String) {
                prologue = false;
            }
            if !prologue {
                let stmt = self.parse_statement(StmtContext::ListItem)?;
                self.scratch_stmts.push(stmt);
                continue;
            }
            let token = Span::new(self.token.start, self.token.end);
            let legacy = self.token.legacy;
            let stmt = self.parse_statement(StmtContext::ListItem)?;
            self.scratch_stmts.push(stmt);
            if !self.is_directive(stmt, token) {
                prologue = false;
                continue;
            }
            if legacy != Legacy::None && prologue_legacy.is_none() {
                let offset = self.legacy_offset(token.start, token.end, legacy);
                prologue_legacy = Some((offset, legacy));
            }
            if self.is_use_strict(token) {
                self.apply_use_strict(prologue_legacy, token.start)?;
            }
        }
        let list = self.ast.push_stmts(&self.scratch_stmts[mark..]);
        self.scratch_stmts.truncate(mark);
        Ok(list)
    }

    /// Whether a statement is a directive: an expression statement that
    /// consists of exactly the string literal token `token`.
    fn is_directive(&self, stmt: StmtId, token: Span) -> bool {
        let StmtKind::Expr(expr) = self.ast.stmt(stmt).kind else {
            return false;
        };
        let expr = self.ast.expr(expr);
        matches!(expr.kind, ExprKind::String(_)) && expr.span == token
    }

    /// Whether the string token is exactly `"use strict"` or
    /// `'use strict'` (no escapes, §11.2.1).
    fn is_use_strict(&self, token: Span) -> bool {
        token.end - token.start == 12
            && self
                .lexer
                .text(token.start + 1, token.end - 1)
                .is_some_and(|text| text.eq_str("use strict"))
    }

    /// A Use Strict Directive: the rest of the function (or script) is
    /// strict mode code, and so are its name and parameters (§11.2.2,
    /// §15.2.1), which the parser has already read.
    fn apply_use_strict(
        &mut self,
        prologue_legacy: Option<(u32, Legacy)>,
        directive: u32,
    ) -> PResult<()> {
        if !self.ctx.simple_params {
            // §15.2.1: also in code that is strict already.
            return Err(ParseError::syntax(
                directive,
                messages::USE_STRICT_NON_SIMPLE,
            ));
        }
        if self.ctx.strict {
            return Ok(());
        }
        self.ctx.strict = true;
        if let Some((offset, legacy)) = prologue_legacy
            && let Some(message) = legacy_error(legacy)
        {
            return Err(ParseError::syntax(offset, message));
        }
        if let Some((name, offset)) = self.ctx.name {
            check_strict_binding_name(name, offset)?;
        }
        let params = self.ctx.params;
        // A simple parameter list has identifiers only.
        for &param in self.ast.patterns(params) {
            let pattern = self.ast.pattern(param);
            if let PatternKind::Identifier(ident) = pattern.kind {
                check_strict_binding_name(ident.name, pattern.span.start)?;
            }
        }
        if let Some(offset) = self.ctx.duplicate_param {
            return Err(ParseError::syntax(offset, messages::DUPLICATE_PARAMETER));
        }
        Ok(())
    }

    /// A statement or declaration in `context`.
    pub(super) fn parse_statement(&mut self, context: StmtContext) -> PResult<StmtId> {
        self.enter()?;
        let stmt = self.parse_statement_inner(context);
        self.leave();
        stmt
    }

    fn parse_statement_inner(&mut self, context: StmtContext) -> PResult<StmtId> {
        let start = self.token.start;
        // In async code `await:` is an await expression with a missing
        // operand (V8 reports the `:`).
        let is_label = self.at_identifier()
            && !(self.at(TokenKind::Yield) && self.ctx.generator)
            && !(self.at(TokenKind::Await) && self.ctx.await_mode == AwaitMode::Expression)
            && self.peek_kind()? == TokenKind::Colon;
        let is_let = self.at_let_declaration()?;
        if !is_label {
            self.label_chain = None;
        }
        if self.ctx.generator
            && self.at(TokenKind::Identifier)
            && self.token.escaped
            && self.token.name() == Some(names::YIELD)
        {
            // V8 reports an escaped `yield` at the start of a statement in
            // a generator as an escaped keyword (elsewhere as `yield`).
            return Err(ParseError::syntax(start, messages::ESCAPED_KEYWORD));
        }
        let kind = self.token.kind;
        match kind {
            _ if is_label => self.parse_labeled(context),
            TokenKind::LBrace => self.parse_block_statement(),
            TokenKind::Semicolon => {
                self.advance()?;
                Ok(self
                    .ast
                    .push_stmt(StmtKind::Empty, Span::new(start, self.prev_end)))
            }
            TokenKind::Var => {
                let stmt = self.parse_variables(VariableKind::Var, false)?;
                self.consume_semicolon()?;
                Ok(self.finish_stmt(stmt, start))
            }
            TokenKind::Const => {
                if context != StmtContext::ListItem {
                    // V8 (Node.js 22): "Unexpected token 'const'", unlike
                    // `let` below.
                    return Err(self.unexpected());
                }
                let stmt = self.parse_variables(VariableKind::Const, false)?;
                self.consume_semicolon()?;
                Ok(self.finish_stmt(stmt, start))
            }
            TokenKind::Identifier if is_let => {
                if context != StmtContext::ListItem {
                    // `let` on its own line is an identifier there (with
                    // automatic semicolon insertion); `let [` never is.
                    let next = self.peek()?;
                    if next.newline_before && next.kind != TokenKind::LBracket {
                        return self.parse_expression_statement();
                    }
                    return Err(ParseError::syntax(start, messages::LEXICAL_IN_STATEMENT));
                }
                let stmt = self.parse_variables(VariableKind::Let, false)?;
                self.consume_semicolon()?;
                Ok(self.finish_stmt(stmt, start))
            }
            TokenKind::Function => self.parse_function_statement(context, start, false),
            TokenKind::Identifier if self.at_async_function()? => {
                if self.token.escaped {
                    return Err(ParseError::syntax(start, messages::ESCAPED_KEYWORD));
                }
                self.advance()?;
                self.parse_function_statement(context, start, true)
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Break | TokenKind::Continue => self.parse_jump(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Throw => self.parse_throw(),
            TokenKind::Try => self.parse_try(),
            TokenKind::Debugger => {
                self.advance()?;
                self.consume_semicolon()?;
                Ok(self
                    .ast
                    .push_stmt(StmtKind::Debugger, Span::new(start, self.prev_end)))
            }
            TokenKind::Class if context != StmtContext::ListItem => Err(self.unexpected()),
            TokenKind::Class => {
                let class = self.parse_class(true)?;
                Ok(self.finish_stmt(StmtKind::Class(class), start))
            }
            TokenKind::Switch => self.parse_switch(),
            TokenKind::With if self.ctx.strict => {
                Err(ParseError::syntax(start, messages::STRICT_WITH))
            }
            TokenKind::With => self.parse_with(),
            TokenKind::Import
                if matches!(self.peek_kind()?, TokenKind::LParen | TokenKind::Dot) =>
            {
                self.parse_expression_statement()
            }
            TokenKind::Import | TokenKind::Export => {
                Err(ParseError::unsupported(start, "module syntax"))
            }
            _ => self.parse_expression_statement(),
        }
    }

    /// The kind of the token after the current one.
    pub(super) fn peek_kind(&mut self) -> PResult<TokenKind> {
        Ok(self.peek()?.kind)
    }

    /// Whether the token after the current one is the identifier `name`
    /// without escapes.
    fn peek_contextual(&mut self, name: crate::NameId) -> PResult<bool> {
        let next = self.peek()?;
        Ok(next.kind == TokenKind::Identifier && !next.escaped && next.name() == Some(name))
    }

    /// Whether the current token is `async` (possibly with escapes)
    /// followed by `function` on the same line: an async function
    /// declaration (§15.8).
    fn at_async_function(&mut self) -> PResult<bool> {
        if !(self.at(TokenKind::Identifier) && self.token.name() == Some(names::ASYNC)) {
            return Ok(false);
        }
        let next = self.peek()?;
        Ok(next.kind == TokenKind::Function && !next.newline_before)
    }

    /// Whether the current `let` starts a lexical declaration: `let`
    /// followed by an identifier, `[` or `{` (§14.3.1; an expression
    /// statement cannot start with `let [`, §14.5).
    fn at_let_declaration(&mut self) -> PResult<bool> {
        if !self.at_contextual(names::LET) {
            return Ok(false);
        }
        Ok(matches!(
            self.peek_kind()?,
            TokenKind::Identifier
                | TokenKind::Yield
                | TokenKind::Await
                | TokenKind::LBracket
                | TokenKind::LBrace
        ))
    }

    /// Sets the span of a statement to `start..` the end of its last token.
    fn finish_stmt(&mut self, kind: StmtKind, start: u32) -> StmtId {
        self.ast.push_stmt(kind, Span::new(start, self.prev_end))
    }

    fn parse_expression_statement(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        let expr = self.parse_expression(false)?;
        self.consume_semicolon()?;
        Ok(self.finish_stmt(StmtKind::Expr(expr), start))
    }

    /// A block statement with its own scope.
    fn parse_block_statement(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        let entered = self.open_scope(ScopeKind::Block, start);
        let stmt = self.parse_block_in_scope(start);
        self.leave_scope(entered);
        stmt
    }

    /// `{ StatementList }` in the current scope.
    fn parse_block_in_scope(&mut self, start: u32) -> PResult<StmtId> {
        self.expect(TokenKind::LBrace)?;
        let body = self.parse_statements(TokenKind::RBrace, false)?;
        self.expect(TokenKind::RBrace)?;
        let scope = self.scope;
        Ok(self.finish_stmt(StmtKind::Block { body, scope }, start))
    }

    /// `var`, `let` or `const` declarations, without the terminator.
    /// `no_in` is set in a `for` head, where a declarator without an
    /// initializer can be the binding of `for`-`in` or `for`-`of`.
    pub(super) fn parse_variables(&mut self, kind: VariableKind, no_in: bool) -> PResult<StmtKind> {
        self.advance()?;
        let mark = self.scratch_declarators.len();
        let result = self.parse_declarators(kind, no_in);
        let declarators = self.ast.push_declarators(&self.scratch_declarators[mark..]);
        self.scratch_declarators.truncate(mark);
        result?;
        Ok(StmtKind::Variables { kind, declarators })
    }

    fn parse_declarators(&mut self, kind: VariableKind, no_in: bool) -> PResult<()> {
        let decl = match kind {
            VariableKind::Var => DeclKind::Var,
            VariableKind::Let => DeclKind::Let,
            VariableKind::Const => DeclKind::Const,
        };
        loop {
            let start = self.token.start;
            let binding_mark = self.scopes.binding_mark();
            let target = self.parse_binding_target(decl, Leaf::Top)?;
            let init = if self.eat(TokenKind::Eq)? {
                Some(self.parse_assignment(no_in)?)
            } else {
                None
            };
            if init.is_none() {
                let for_in_of = no_in && (self.at(TokenKind::In) || self.at_contextual(names::OF));
                let pattern = !matches!(self.ast.pattern(target).kind, PatternKind::Identifier(_));
                if !for_in_of && pattern {
                    return Err(ParseError::syntax(
                        start,
                        messages::MISSING_DESTRUCTURING_INITIALIZER,
                    ));
                }
                if !for_in_of && kind == VariableKind::Const {
                    return Err(ParseError::syntax(
                        start,
                        messages::MISSING_CONST_INITIALIZER,
                    ));
                }
            }
            if kind != VariableKind::Var {
                let range = binding_mark..self.scopes.binding_mark();
                self.scopes
                    .set_init_end_range(range, self.scope, false, self.prev_end);
            }
            self.scratch_declarators.push(Declarator {
                target,
                init,
                span: Span::new(start, self.prev_end),
            });
            if !self.eat(TokenKind::Comma)? {
                return Ok(());
            }
        }
    }

    /// A function declaration in statement position (§14.1, Annex B.3.1,
    /// B.3.3); the current token is `function`, `start` is where the
    /// declaration starts (`async` or `function`).
    fn parse_function_statement(
        &mut self,
        context: StmtContext,
        start: u32,
        is_async: bool,
    ) -> PResult<StmtId> {
        let generator = self.peek_kind()? == TokenKind::Star;
        match context {
            StmtContext::ListItem => {}
            _ if is_async => {
                return Err(ParseError::syntax(
                    start,
                    messages::ASYNC_FUNCTION_IN_STATEMENT,
                ));
            }
            _ if generator && context != StmtContext::Other => {
                // V8 marks the `*`.
                let star = self.peek()?.start;
                return Err(ParseError::syntax(star, messages::GENERATOR_IN_STATEMENT));
            }
            _ if self.ctx.strict => {
                return Err(ParseError::syntax(start, messages::STRICT_FUNCTION));
            }
            StmtContext::Other => {
                return Err(ParseError::syntax(start, messages::SLOPPY_FUNCTION));
            }
            StmtContext::LabelInList => {}
            StmtContext::If => {
                // Annex B.3.3: as if the declaration were in a block.
                let entered = self.open_scope(ScopeKind::Block, start);
                let function = self.parse_function_declaration(start, false);
                let scope = self.scope;
                self.leave_scope(entered);
                let function = self.ast.push_stmt(
                    StmtKind::Function(function?),
                    Span::new(start, self.prev_end),
                );
                let body = self.ast.push_stmts(&[function]);
                return Ok(self.finish_stmt(StmtKind::Block { body, scope }, start));
            }
        }
        let function = self.parse_function_declaration(start, is_async)?;
        Ok(self.finish_stmt(StmtKind::Function(function), start))
    }

    /// A labelled statement (§14.13).
    fn parse_labeled(&mut self, context: StmtContext) -> PResult<StmtId> {
        let start = self.token.start;
        let name = self.check_identifier(IdentUse::Reference)?;
        let base = self.ctx.label_base;
        if self
            .labels
            .get(base..)
            .is_some_and(|labels| labels.iter().any(|label| label.name == name))
        {
            return Err(ParseError::syntax(
                start,
                messages::label_redeclared(&self.name_text(name)),
            ));
        }
        self.advance()?;
        self.expect(TokenKind::Colon)?;
        let chain = self.label_chain.unwrap_or(self.labels.len());
        let is_loop = matches!(
            self.token.kind,
            TokenKind::While | TokenKind::Do | TokenKind::For
        );
        let is_block = self.at(TokenKind::LBrace);
        self.labels.push(Label {
            name,
            is_loop,
            is_block,
        });
        if is_loop {
            for label in self.labels.get_mut(chain..).unwrap_or(&mut []) {
                label.is_loop = true;
            }
        }
        self.label_chain = Some(chain);
        let body_context = match context {
            StmtContext::ListItem | StmtContext::LabelInList => StmtContext::LabelInList,
            _ => StmtContext::Other,
        };
        let body = self.parse_statement(body_context);
        self.labels.pop();
        let body = body?;
        Ok(self.finish_stmt(StmtKind::Labeled { label: name, body }, start))
    }

    fn parse_if(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        self.expect(TokenKind::LParen)?;
        let test = self.parse_expression(false)?;
        self.expect(TokenKind::RParen)?;
        let consequent = self.parse_statement(StmtContext::If)?;
        let alternate = if self.eat(TokenKind::Else)? {
            Some(self.parse_statement(StmtContext::If)?)
        } else {
            None
        };
        Ok(self.finish_stmt(
            StmtKind::If {
                test,
                consequent,
                alternate,
            },
            start,
        ))
    }

    /// The body of an iteration statement.
    fn parse_loop_body(&mut self) -> PResult<StmtId> {
        self.ctx.iterations += 1;
        let body = self.parse_statement(StmtContext::Other);
        self.ctx.iterations -= 1;
        body
    }

    fn parse_while(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        self.expect(TokenKind::LParen)?;
        let test = self.parse_expression(false)?;
        self.expect(TokenKind::RParen)?;
        let body = self.parse_loop_body()?;
        Ok(self.finish_stmt(StmtKind::While { test, body }, start))
    }

    fn parse_do_while(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        let body = self.parse_loop_body()?;
        self.expect(TokenKind::While)?;
        self.expect(TokenKind::LParen)?;
        let test = self.parse_expression(false)?;
        self.expect(TokenKind::RParen)?;
        // §12.10.1: a semicolon is inserted after `do ... while (...)`
        // even without a line terminator.
        self.eat(TokenKind::Semicolon)?;
        Ok(self.finish_stmt(StmtKind::DoWhile { body, test }, start))
    }

    /// A `for` statement: the three-part form (§14.7.4), `for`-`in`,
    /// `for`-`of` and `for await`-`of` (§14.7.5). The head has its own
    /// scope.
    fn parse_for(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        let is_await = self.at_await_keyword();
        if is_await {
            if self.ctx.await_mode != AwaitMode::Expression {
                return Err(self.unexpected());
            }
            if self.token.escaped {
                return Err(ParseError::syntax(
                    self.token.start,
                    messages::ESCAPED_KEYWORD,
                ));
            }
            self.advance()?;
        }
        self.expect(TokenKind::LParen)?;
        let entered = self.open_scope(ScopeKind::For, start);
        let stmt = if is_await {
            self.parse_for_await_rest(start)
        } else {
            self.parse_for_rest(start)
        };
        self.leave_scope(entered);
        stmt
    }

    /// Whether the current token is `await` (also escaped, which is an
    /// error after `for`).
    fn at_await_keyword(&self) -> bool {
        self.at(TokenKind::Await)
            || (self.at(TokenKind::Identifier) && self.token.name() == Some(names::AWAIT))
    }

    /// The head and body of `for await (... of ...)` after the `(`
    /// (§14.7.5): a declaration or a target, then `of`.
    fn parse_for_await_rest(&mut self, start: u32) -> PResult<StmtId> {
        let init_start = self.token.start;
        let binding_mark = self.scopes.binding_mark();
        let is_let = self.at_let_declaration()?;
        let declaration = match self.token.kind {
            TokenKind::Var => Some(VariableKind::Var),
            TokenKind::Const => Some(VariableKind::Const),
            TokenKind::Identifier if is_let => Some(VariableKind::Let),
            _ => None,
        };
        if let Some(kind) = declaration {
            let decl = self.parse_variables(kind, true)?;
            if !self.at_contextual(names::OF) {
                self.check_escaped_of()?;
                return Err(self.for_await_head_error(decl));
            }
            let head = ForDeclaration {
                start,
                init_start,
                binding_mark,
                kind,
                is_await: true,
            };
            return self.parse_for_in_of_declaration(head, decl);
        }
        let head_start = self.token.start;
        let starts_with_let = self.at_contextual(names::LET);
        // `for await (async of x)` is valid (§14.7.5 has no `async of`
        // lookahead rule for `for await`): `async` is the target here,
        // not the start of an async arrow function.
        let outer = self.cover_error.take();
        let first = if self.at_contextual(names::ASYNC) && self.peek_contextual(names::OF)? {
            self.parse_identifier_reference()
        } else {
            // `for await ( LeftHandSideExpression of ...`.
            self.parse_left_hand_side()
        };
        let first = first.inspect_err(|_| self.cover_error = outer)?;
        if !self.at_contextual(names::OF) {
            self.check_cover_error()?;
            self.check_escaped_of()?;
            return Err(self.unexpected());
        }
        if starts_with_let {
            return Err(ParseError::syntax(head_start, messages::FOR_OF_LET));
        }
        let target = self.for_target(first);
        self.cover_error = outer;
        let target = target?;
        let right = self.parse_for_in_of_right(true)?;
        self.finish_for_in_of(start, ForHead::Target(target), right, true, true)
    }

    /// Where V8 marks an initializer of a for-in/of binding: the
    /// identifier, or the last token of a pattern.
    fn binding_error_offset(&self, target: crate::PatternId) -> u32 {
        let target = self.ast.pattern(target);
        match target.kind {
            PatternKind::Identifier(_) => target.span.start,
            _ => target.span.end.saturating_sub(1),
        }
    }

    /// `of` written with escapes after a `for await` head (§14.7.5: a
    /// keyword must not contain escapes).
    fn check_escaped_of(&self) -> PResult<()> {
        if self.at(TokenKind::Identifier)
            && self.token.escaped
            && self.token.name() == Some(names::OF)
        {
            return Err(ParseError::syntax(
                self.token.start,
                messages::ESCAPED_KEYWORD,
            ));
        }
        Ok(())
    }

    /// The error for a declaration in a `for await` head that `of` does not
    /// follow: V8 reports an initializer at the binding.
    fn for_await_head_error(&self, decl: StmtKind) -> ParseError {
        if let StmtKind::Variables { declarators, .. } = decl
            && let Some(declarator) = self
                .ast
                .declarators(declarators)
                .iter()
                .find(|d| d.init.is_some())
        {
            let target = self.binding_error_offset(declarator.target);
            return ParseError::syntax(target, messages::FOR_AWAIT_INITIALIZER);
        }
        self.unexpected()
    }

    fn parse_for_rest(&mut self, start: u32) -> PResult<StmtId> {
        let init_start = self.token.start;
        let binding_mark = self.scopes.binding_mark();
        let is_let = self.at_let_declaration()?;
        let declaration = match self.token.kind {
            TokenKind::Var => Some(VariableKind::Var),
            TokenKind::Const => Some(VariableKind::Const),
            TokenKind::Identifier if is_let => Some(VariableKind::Let),
            _ => None,
        };
        if let Some(kind) = declaration {
            let decl = self.parse_variables(kind, true)?;
            if self.at(TokenKind::In) || self.at_contextual(names::OF) {
                let head = ForDeclaration {
                    start,
                    init_start,
                    binding_mark,
                    kind,
                    is_await: false,
                };
                return self.parse_for_in_of_declaration(head, decl);
            }
            let init = self.finish_stmt(decl, init_start);
            return self.parse_for_three(start, Some(init));
        }
        if self.at(TokenKind::Semicolon) {
            return self.parse_for_three(start, None);
        }
        self.parse_for_expression_head(start)
    }

    /// The rest of a `for` statement whose head starts with an
    /// expression.
    fn parse_for_expression_head(&mut self, start: u32) -> PResult<StmtId> {
        let head_start = self.token.start;
        let starts_with_let = self.at_contextual(names::LET);
        let first = if self.at_contextual(names::ASYNC) && self.peek_contextual(names::OF)? {
            // `for (async of` cannot start a for-of head (§14.7.5); only
            // `for (async of => {}; ;)` is valid (an async arrow).
            self.advance()?;
            if self.peek_kind()? != TokenKind::Arrow {
                return Err(ParseError::syntax(head_start, messages::FOR_OF_ASYNC));
            }
            self.parse_async_arrow_identifier(head_start, true)?
        } else {
            self.parse_assignment_cover(true)?
        };
        let is_in = self.at(TokenKind::In);
        let is_of = self.at_contextual(names::OF);
        if is_in || is_of {
            if is_of && starts_with_let {
                return Err(ParseError::syntax(head_start, messages::FOR_OF_LET));
            }
            let target = self.for_target(first)?;
            return self.parse_for_in_of_rest(start, ForHead::Target(target), is_of);
        }
        self.check_cover_error()?;
        let init = self.parse_expression_rest(first, head_start, true)?;
        if self.at(TokenKind::In) || self.at_contextual(names::OF) {
            return Err(ParseError::syntax(head_start, messages::INVALID_FOR_TARGET));
        }
        let init = self.finish_stmt(StmtKind::Expr(init), head_start);
        self.parse_for_three(start, Some(init))
    }

    /// The target of `for`-`in` or `for`-`of` without a declaration
    /// (§14.7.5.1): an assignment pattern or a simple target.
    fn for_target(&mut self, first: ExprId) -> PResult<AssignTarget> {
        let node = *self.ast.expr(first);
        if matches!(node.kind, ExprKind::Array(_) | ExprKind::Object(_)) {
            let pattern = self.assignment_pattern(first)?;
            self.cover_error = None;
            return Ok(AssignTarget::Pattern(pattern));
        }
        self.check_cover_error()?;
        let target = self.unparenthesized(first);
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => {
                self.check_strict_target(ident.name, node.span.start)?;
                Ok(AssignTarget::Simple(target))
            }
            kind if is_property_target(kind) => Ok(AssignTarget::Simple(target)),
            // Runtime errors for call targets (Annex B), in sloppy mode.
            ExprKind::Call { .. } if !self.ctx.strict => Ok(AssignTarget::Simple(target)),
            ExprKind::Array(_) | ExprKind::Object(_) => Err(ParseError::syntax(
                node.span.start,
                messages::INVALID_DESTRUCTURING_TARGET,
            )),
            _ => Err(ParseError::syntax(
                node.span.start,
                messages::INVALID_FOR_TARGET,
            )),
        }
    }

    /// `for (var/let/const binding in/of ...)`: one declarator, no
    /// initializer (except Annex B.3.5 `for (var x = 1 in o)` in sloppy
    /// mode code).
    fn parse_for_in_of_declaration(
        &mut self,
        head: ForDeclaration,
        decl: StmtKind,
    ) -> PResult<StmtId> {
        let is_of = self.at_contextual(names::OF);
        let StmtKind::Variables { declarators, .. } = decl else {
            return Err(self.unexpected());
        };
        let list = self.ast.declarators(declarators);
        let [declarator] = list else {
            let message = if head.is_await {
                messages::FOR_AWAIT_SINGLE_BINDING
            } else if is_of {
                messages::FOR_OF_SINGLE_BINDING
            } else {
                messages::FOR_IN_SINGLE_BINDING
            };
            // V8 marks the first binding.
            let first = list.first().map_or(head.init_start, |d| d.span.start);
            return Err(ParseError::syntax(first, message));
        };
        if declarator.init.is_some() && head.is_await {
            let target = self.binding_error_offset(declarator.target);
            return Err(ParseError::syntax(target, messages::FOR_AWAIT_INITIALIZER));
        }
        if declarator.init.is_some() {
            let annex_b = !is_of
                && head.kind == VariableKind::Var
                && !self.ctx.strict
                && matches!(
                    self.ast.pattern(declarator.target).kind,
                    PatternKind::Identifier(_)
                );
            if !annex_b {
                let message = if is_of {
                    messages::FOR_OF_INITIALIZER
                } else {
                    messages::FOR_IN_INITIALIZER
                };
                let at = self.binding_error_offset(declarator.target);
                return Err(ParseError::syntax(at, message));
            }
        }
        let stmt = self.finish_stmt(decl, head.init_start);
        let right = self.parse_for_in_of_right(is_of)?;
        if head.kind != VariableKind::Var {
            // The bindings are uninitialized while the right side runs
            // (§14.7.5.6 step 2).
            let range = head.binding_mark..self.scopes.binding_mark();
            self.scopes
                .set_init_end_range(range, self.scope, false, self.prev_end);
        }
        self.finish_for_in_of(
            head.start,
            ForHead::Declaration(stmt),
            right,
            is_of,
            head.is_await,
        )
    }

    /// The rest of `for (target in/of right) body`.
    fn parse_for_in_of_rest(&mut self, start: u32, left: ForHead, is_of: bool) -> PResult<StmtId> {
        let right = self.parse_for_in_of_right(is_of)?;
        self.finish_for_in_of(start, left, right, is_of, false)
    }

    /// `in Expression` or `of AssignmentExpression`.
    fn parse_for_in_of_right(&mut self, is_of: bool) -> PResult<ExprId> {
        self.advance()?;
        if is_of {
            self.parse_assignment(false)
        } else {
            self.parse_expression(false)
        }
    }

    fn finish_for_in_of(
        &mut self,
        start: u32,
        left: ForHead,
        right: ExprId,
        is_of: bool,
        is_await: bool,
    ) -> PResult<StmtId> {
        self.expect(TokenKind::RParen)?;
        let body = self.parse_loop_body()?;
        let scope = self.scope;
        if let Some(entry) = self.scopes.scope_mut(scope) {
            entry.kind = ScopeKind::ForInOf;
        }
        let kind = if is_of {
            StmtKind::ForOf {
                left,
                right,
                body,
                scope,
                is_await,
            }
        } else {
            StmtKind::ForIn {
                left,
                right,
                body,
                scope,
            }
        };
        Ok(self.finish_stmt(kind, start))
    }

    /// The rest of a three-part `for` after its initialization.
    fn parse_for_three(&mut self, start: u32, init: Option<StmtId>) -> PResult<StmtId> {
        self.expect(TokenKind::Semicolon)?;
        let test = if self.at(TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression(false)?)
        };
        self.expect(TokenKind::Semicolon)?;
        let update = if self.at(TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expression(false)?)
        };
        self.expect(TokenKind::RParen)?;
        let body = self.parse_loop_body()?;
        let scope = self.scope;
        Ok(self.finish_stmt(
            StmtKind::For {
                init,
                test,
                update,
                body,
                scope,
            },
            start,
        ))
    }

    /// `with (object) body` (§14.11): sloppy mode code only.
    fn parse_with(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        self.expect(TokenKind::LParen)?;
        let object = self.parse_expression(false)?;
        self.expect(TokenKind::RParen)?;
        let entered = self.open_scope(ScopeKind::With, self.token.start);
        let scope = self.scope;
        let body = self.parse_statement(StmtContext::Other);
        self.leave_scope(entered);
        let body = body?;
        Ok(self.finish_stmt(
            StmtKind::With {
                object,
                body,
                scope,
            },
            start,
        ))
    }

    /// `break` and `continue` (§14.8, §14.9, §14.9.1, §14.8.1).
    fn parse_jump(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        let is_break = self.at(TokenKind::Break);
        self.advance()?;
        let label = if self.at_identifier() && !self.token.newline_before {
            let offset = self.token.start;
            let name = self.check_identifier(IdentUse::Reference)?;
            let found = self
                .labels
                .get(self.ctx.label_base..)
                .and_then(|labels| labels.iter().rev().find(|label| label.name == name))
                .copied();
            match found {
                None => {
                    return Err(ParseError::syntax(
                        offset,
                        messages::undefined_label(&self.name_text(name)),
                    ));
                }
                Some(label) if !is_break && !label.is_loop => {
                    let text = self.name_text(name);
                    let message = if label.is_block {
                        messages::continue_not_loop(&text)
                    } else {
                        messages::undefined_label(&text)
                    };
                    return Err(ParseError::syntax(offset, message));
                }
                Some(_) => {}
            }
            self.advance()?;
            Some(name)
        } else {
            let allowed = if is_break {
                self.ctx.iterations + self.ctx.switches > 0
            } else {
                self.ctx.iterations > 0
            };
            if !allowed {
                let message = if is_break {
                    messages::ILLEGAL_BREAK
                } else {
                    messages::ILLEGAL_CONTINUE
                };
                return Err(ParseError::syntax(start, message));
            }
            None
        };
        self.consume_semicolon()?;
        let kind = if is_break {
            StmtKind::Break { label }
        } else {
            StmtKind::Continue { label }
        };
        Ok(self.finish_stmt(kind, start))
    }

    /// `return` (§14.10), only in function bodies (not in class static
    /// blocks, §15.7.1).
    fn parse_return(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        if matches!(
            self.ctx.kind,
            crate::FunctionKind::Script | crate::FunctionKind::StaticInitializer
        ) {
            return Err(ParseError::syntax(start, messages::ILLEGAL_RETURN));
        }
        self.advance()?;
        let argument = if self.at(TokenKind::Semicolon)
            || self.at(TokenKind::RBrace)
            || self.at(TokenKind::Eof)
            || self.token.newline_before
        {
            None
        } else {
            Some(self.parse_expression(false)?)
        };
        self.consume_semicolon()?;
        Ok(self.finish_stmt(StmtKind::Return(argument), start))
    }

    /// `throw` (§14.14): no line terminator before the operand.
    fn parse_throw(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        if self.token.newline_before {
            return Err(ParseError::syntax(start, messages::NEWLINE_AFTER_THROW));
        }
        let argument = self.parse_expression(false)?;
        self.consume_semicolon()?;
        Ok(self.finish_stmt(StmtKind::Throw(argument), start))
    }

    /// `switch` (§14.12): the clauses share the scope of the case block.
    fn parse_switch(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        self.expect(TokenKind::LParen)?;
        let discriminant = self.parse_expression(false)?;
        self.expect(TokenKind::RParen)?;
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let entered = self.open_scope(ScopeKind::Switch, self.token.start);
        self.ctx.switches += 1;
        let cases = self.parse_case_block();
        self.ctx.switches -= 1;
        let scope = self.scope;
        self.leave_scope(entered);
        let cases = cases?;
        Ok(self.finish_stmt(
            StmtKind::Switch {
                discriminant,
                cases,
                scope,
            },
            start,
        ))
    }

    /// `{ CaseClauses DefaultClause CaseClauses }` (§14.12).
    fn parse_case_block(&mut self) -> PResult<List<SwitchCase>> {
        self.expect(TokenKind::LBrace)?;
        let mark = self.scratch_cases.len();
        let result = self.parse_case_clauses();
        let cases = self.ast.push_cases(&self.scratch_cases[mark..]);
        self.scratch_cases.truncate(mark);
        result?;
        self.expect(TokenKind::RBrace)?;
        Ok(cases)
    }

    fn parse_case_clauses(&mut self) -> PResult<()> {
        let mut has_default = false;
        while !self.at(TokenKind::RBrace) {
            let start = self.token.start;
            let test = match self.token.kind {
                TokenKind::Case => {
                    self.advance()?;
                    Some(self.parse_expression(false)?)
                }
                TokenKind::Default => {
                    if has_default {
                        return Err(ParseError::syntax(start, messages::MULTIPLE_DEFAULTS));
                    }
                    has_default = true;
                    self.advance()?;
                    None
                }
                _ => return Err(self.unexpected()),
            };
            self.expect(TokenKind::Colon)?;
            let body = self.parse_clause_statements()?;
            self.scratch_cases.push(SwitchCase {
                test,
                body,
                span: Span::new(start, self.prev_end),
            });
        }
        Ok(())
    }

    /// The statements of one clause, up to the next clause or the end of
    /// the case block.
    fn parse_clause_statements(&mut self) -> PResult<List<StmtId>> {
        let mark = self.scratch_stmts.len();
        while !matches!(
            self.token.kind,
            TokenKind::Case | TokenKind::Default | TokenKind::RBrace
        ) {
            if self.at(TokenKind::Eof) {
                self.scratch_stmts.truncate(mark);
                return Err(self.unexpected());
            }
            self.recorded.truncate(self.scope_base);
            match self.parse_statement(StmtContext::ListItem) {
                Ok(stmt) => self.scratch_stmts.push(stmt),
                Err(error) => {
                    self.scratch_stmts.truncate(mark);
                    return Err(error);
                }
            }
        }
        let list = self.ast.push_stmts(&self.scratch_stmts[mark..]);
        self.scratch_stmts.truncate(mark);
        Ok(list)
    }

    /// `try` (§14.15).
    fn parse_try(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let block = self.parse_block_statement()?;
        let handler = if self.at(TokenKind::Catch) {
            Some(self.parse_catch()?)
        } else {
            None
        };
        let finalizer = if self.eat(TokenKind::Finally)? {
            if !self.at(TokenKind::LBrace) {
                return Err(self.unexpected());
            }
            Some(self.parse_block_statement()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(ParseError::syntax(
                self.prev_start,
                messages::MISSING_CATCH_OR_FINALLY,
            ));
        }
        Ok(self.finish_stmt(
            StmtKind::Try {
                block,
                handler,
                finalizer,
            },
            start,
        ))
    }

    /// A catch clause: the parameter and the block share one scope.
    fn parse_catch(&mut self) -> PResult<CatchClause> {
        let start = self.token.start;
        self.advance()?;
        let entered = self.open_scope(ScopeKind::Catch, start);
        let clause = self.parse_catch_in_scope();
        self.leave_scope(entered);
        clause
    }

    fn parse_catch_in_scope(&mut self) -> PResult<CatchClause> {
        let param = if self.eat(TokenKind::LParen)? {
            let start = self.token.start;
            // `catch ([let])` is accepted in sloppy mode code, V8 rejects it
            // ("let is disallowed as a lexically bound name"). ECMA-262
            // 2025 has no such rule for a catch parameter: 14.15.1 (early
            // errors of Catch) lists duplicate BoundNames and the clashes
            // with the LexicallyDeclaredNames and (B.3.4) VarDeclaredNames
            // of the Block; 14.3.1.1 (`let` in BoundNames) covers `let` and
            // `const` declarations only; 13.1.1 allows the identifier
            // `let` in sloppy mode code. test262 has no test for it. (I
            // could not fetch the specification text; this is from my
            // reading of those clauses.)
            let pattern = if matches!(self.token.kind, TokenKind::LBracket | TokenKind::LBrace) {
                self.parse_binding_target(DeclKind::Catch, Leaf::Top)?
            } else {
                let name = self.check_identifier(IdentUse::Name)?;
                let declared = self
                    .scopes
                    .declare_catch_param(self.scope, name, start, false);
                self.declared(declared, name, start)?;
                let ident = self.reference(name, start, true);
                self.advance()?;
                self.ast.push_pattern(
                    PatternKind::Identifier(ident),
                    Span::new(start, self.prev_end),
                )
            };
            self.expect(TokenKind::RParen)?;
            Some(pattern)
        } else {
            None
        };
        if !self.at(TokenKind::LBrace) {
            return Err(self.unexpected());
        }
        let body = self.parse_block_in_scope(self.token.start)?;
        Ok(CatchClause { param, body })
    }
}

/// The parts of a `for` head with a declaration.
#[derive(Clone, Copy)]
struct ForDeclaration {
    /// The start of the statement.
    start: u32,
    /// The start of the declaration.
    init_start: u32,
    /// The binding mark before the declaration.
    binding_mark: usize,
    kind: VariableKind,
    /// `for await`.
    is_await: bool,
}

/// The strict mode rules for a binding name read in sloppy mode.
fn check_strict_binding_name(name: crate::NameId, offset: u32) -> PResult<()> {
    if super::is_strict_reserved(name) {
        return Err(ParseError::syntax(
            offset,
            messages::UNEXPECTED_STRICT_RESERVED,
        ));
    }
    if super::is_eval_or_arguments(name) {
        return Err(ParseError::syntax(
            offset,
            messages::UNEXPECTED_EVAL_OR_ARGUMENTS,
        ));
    }
    Ok(())
}
