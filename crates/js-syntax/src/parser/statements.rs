//! Statements and declarations (ECMA-262 clause 14), the directive
//! prologue (§11.2.1) and automatic semicolon insertion (§12.10).

use swb_js_text::CodeUnit;

use super::{IdentUse, Label, PResult, Parser, StmtContext, legacy_error};
use crate::ast::{
    CatchClause, Declarator, ExprKind, List, PatternKind, Span, StmtId, StmtKind, VariableKind,
};
use crate::error::ParseError;
use crate::interner::names;
use crate::messages;
use crate::scope::{BindingKind, ScopeKind};
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
                prologue_legacy = Some((token.start, legacy));
            }
            if self.is_use_strict(token) {
                self.apply_use_strict(prologue_legacy)?;
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
    fn apply_use_strict(&mut self, prologue_legacy: Option<(u32, Legacy)>) -> PResult<()> {
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
        for &param in self.ast.patterns(params) {
            let pattern = self.ast.pattern(param);
            let PatternKind::Identifier(ident) = pattern.kind;
            check_strict_binding_name(ident.name, pattern.span.start)?;
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
        let is_label = self.at_identifier()
            && !(self.at(TokenKind::Yield) && self.ctx.generator)
            && self.peek_kind()? == TokenKind::Colon;
        let is_let = self.at_let_declaration()?;
        if !is_label {
            self.label_chain = None;
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
            TokenKind::Function => self.parse_function_statement(context),
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
            TokenKind::Class => Err(ParseError::unsupported(start, "class")),
            TokenKind::Switch => Err(ParseError::unsupported(start, "switch")),
            TokenKind::With if self.ctx.strict => {
                Err(ParseError::syntax(start, messages::STRICT_WITH))
            }
            TokenKind::With => Err(ParseError::unsupported(start, "with")),
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
        if self.at_contextual(names::ASYNC)
            && self.peek_kind()? == TokenKind::Function
            && !self.peek()?.newline_before
        {
            return Err(ParseError::unsupported(start, "async function"));
        }
        let expr = self.parse_expression(false)?;
        self.consume_semicolon()?;
        Ok(self.finish_stmt(StmtKind::Expr(expr), start))
    }

    /// A block statement with its own scope.
    fn parse_block_statement(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        let outer = self.open_scope(ScopeKind::Block, start);
        let stmt = self.parse_block_in_scope(start);
        self.close_scope(outer);
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
    /// `no_in` is set in a `for` head.
    pub(super) fn parse_variables(&mut self, kind: VariableKind, no_in: bool) -> PResult<StmtKind> {
        self.advance()?;
        let mark = self.scratch_declarators.len();
        loop {
            let start = self.token.start;
            if matches!(self.token.kind, TokenKind::LBracket | TokenKind::LBrace) {
                return Err(ParseError::unsupported(start, "destructuring"));
            }
            let usage = if kind == VariableKind::Var {
                IdentUse::Binding
            } else {
                IdentUse::Lexical
            };
            let name = self.check_identifier(usage)?;
            let binding = match kind {
                VariableKind::Var => self.scopes.declare_var(self.scope, name, start),
                VariableKind::Let => {
                    self.scopes
                        .declare_lexical(self.scope, name, BindingKind::Let, start)
                }
                VariableKind::Const => {
                    self.scopes
                        .declare_lexical(self.scope, name, BindingKind::Const, start)
                }
            };
            let binding = self.declared(binding, name, start)?;
            let ident = self.reference(name, start, true);
            self.advance()?;
            let target = self.ast.push_pattern(
                PatternKind::Identifier(ident),
                Span::new(start, self.prev_end),
            );
            let init = if self.eat(TokenKind::Eq)? {
                Some(self.parse_assignment(no_in)?)
            } else {
                None
            };
            if kind == VariableKind::Const && init.is_none() {
                let for_in_of = no_in && (self.at(TokenKind::In) || self.at_contextual(names::OF));
                if !for_in_of {
                    return Err(ParseError::syntax(
                        self.token.start,
                        messages::MISSING_CONST_INITIALIZER,
                    ));
                }
            }
            if kind != VariableKind::Var {
                self.scopes.set_init_end(binding, self.prev_end);
            }
            self.scratch_declarators.push(Declarator {
                target,
                init,
                span: Span::new(start, self.prev_end),
            });
            if !self.eat(TokenKind::Comma)? {
                break;
            }
        }
        let declarators = self.ast.push_declarators(&self.scratch_declarators[mark..]);
        self.scratch_declarators.truncate(mark);
        Ok(StmtKind::Variables { kind, declarators })
    }

    /// A function declaration in statement position (§14.1, Annex B.3.1,
    /// B.3.3).
    fn parse_function_statement(&mut self, context: StmtContext) -> PResult<StmtId> {
        let start = self.token.start;
        let generator = self.peek_kind()? == TokenKind::Star;
        match context {
            StmtContext::ListItem => {}
            _ if generator && context != StmtContext::Other => {
                return Err(ParseError::syntax(start, messages::GENERATOR_IN_STATEMENT));
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
                let outer = self.open_scope(ScopeKind::Block, start);
                let function = self.parse_function_declaration();
                let scope = self.scope;
                self.close_scope(outer);
                let function = self.ast.push_stmt(
                    StmtKind::Function(function?),
                    Span::new(start, self.prev_end),
                );
                let body = self.ast.push_stmts(&[function]);
                return Ok(self.finish_stmt(StmtKind::Block { body, scope }, start));
            }
        }
        let function = self.parse_function_declaration()?;
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

    /// A `for` statement (§14.7.4). Only the three-part form is in the
    /// subset; `for`-`in` and `for`-`of` are reported as not supported.
    fn parse_for(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        if self.at(TokenKind::Await) {
            return Err(ParseError::unsupported(self.token.start, "for await"));
        }
        self.expect(TokenKind::LParen)?;
        let outer = self.open_scope(ScopeKind::For, start);
        let stmt = self.parse_for_rest(start);
        self.close_scope(outer);
        stmt
    }

    fn parse_for_rest(&mut self, start: u32) -> PResult<StmtId> {
        let init_start = self.token.start;
        let is_let = self.at_let_declaration()?;
        let init = match self.token.kind {
            TokenKind::Semicolon => None,
            TokenKind::Var => Some(self.parse_variables(VariableKind::Var, true)?),
            TokenKind::Const => Some(self.parse_variables(VariableKind::Const, true)?),
            TokenKind::Identifier if is_let => Some(self.parse_variables(VariableKind::Let, true)?),
            _ => Some(StmtKind::Expr(self.parse_expression(true)?)),
        };
        if self.at(TokenKind::In) {
            return Err(ParseError::unsupported(self.token.start, "for-in"));
        }
        if self.at_contextual(names::OF) {
            return Err(ParseError::unsupported(self.token.start, "for-of"));
        }
        let init = init.map(|kind| self.finish_stmt(kind, init_start));
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
            if self.ctx.iterations == 0 {
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

    /// `return` (§14.10), only in function bodies.
    fn parse_return(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        if self.ctx.kind == crate::FunctionKind::Script {
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
                self.token.start,
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
        let outer = self.open_scope(ScopeKind::Catch, start);
        let clause = self.parse_catch_in_scope();
        self.close_scope(outer);
        clause
    }

    fn parse_catch_in_scope(&mut self) -> PResult<CatchClause> {
        let param = if self.eat(TokenKind::LParen)? {
            let start = self.token.start;
            if matches!(self.token.kind, TokenKind::LBracket | TokenKind::LBrace) {
                return Err(ParseError::unsupported(start, "destructuring"));
            }
            let name = self.check_identifier(IdentUse::Binding)?;
            self.scopes.declare_catch_param(self.scope, name, start);
            let ident = self.reference(name, start, true);
            self.advance()?;
            self.expect(TokenKind::RParen)?;
            Some(self.ast.push_pattern(
                PatternKind::Identifier(ident),
                Span::new(start, self.prev_end),
            ))
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
