//! Expressions (ECMA-262 clause 13): precedence climbing for the binary
//! operators, recursive descent for the rest.

use swb_js_text::CodeUnit;

use super::functions::{ArrowHead, ArrowStart, FunctionHead};
use super::{AwaitMode, CHAIN_WEIGHT, IdentUse, PResult, Parser, Recorded};
use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, ExprId, ExprKind, FunctionKind, List, LogicalOp, Property,
    PropertyKey, PropertyKind, Span, SuperCall, Template, TemplateElement, TemplateId, UnaryOp,
    UpdateOp,
};
use crate::error::ParseError;
use crate::interner::names;
use crate::messages;
use crate::token::{Goal, TokenKind, TokenValue};

/// The binary operators, their precedence (higher binds tighter) and
/// whether they short-circuit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Binary(BinaryOp),
    Logical(LogicalOp),
}

/// The operator of a token and its precedence. `in` is not an operator in
/// the head of a `for` statement (`[~In]`).
fn binary_operator(kind: TokenKind, no_in: bool) -> Option<(Operator, u8)> {
    use BinaryOp as B;
    use Operator::{Binary, Logical};
    let entry = match kind {
        TokenKind::QuestionQuestion => (Logical(LogicalOp::Coalesce), 1),
        TokenKind::PipePipe => (Logical(LogicalOp::Or), 1),
        TokenKind::AmpAmp => (Logical(LogicalOp::And), 2),
        TokenKind::Pipe => (Binary(B::BitOr), 3),
        TokenKind::Caret => (Binary(B::BitXor), 4),
        TokenKind::Amp => (Binary(B::BitAnd), 5),
        TokenKind::EqEq => (Binary(B::Equal), 6),
        TokenKind::NotEq => (Binary(B::NotEqual), 6),
        TokenKind::EqEqEq => (Binary(B::StrictEqual), 6),
        TokenKind::NotEqEq => (Binary(B::StrictNotEqual), 6),
        TokenKind::Lt => (Binary(B::Less), 7),
        TokenKind::Gt => (Binary(B::Greater), 7),
        TokenKind::LtEq => (Binary(B::LessEqual), 7),
        TokenKind::GtEq => (Binary(B::GreaterEqual), 7),
        TokenKind::Instanceof => (Binary(B::Instanceof), 7),
        TokenKind::In if !no_in => (Binary(B::In), 7),
        TokenKind::Shl => (Binary(B::ShiftLeft), 8),
        TokenKind::Shr => (Binary(B::ShiftRight), 8),
        TokenKind::UShr => (Binary(B::ShiftRightUnsigned), 8),
        TokenKind::Plus => (Binary(B::Add), 9),
        TokenKind::Minus => (Binary(B::Subtract), 9),
        TokenKind::Star => (Binary(B::Multiply), 10),
        TokenKind::Slash => (Binary(B::Divide), 10),
        TokenKind::Percent => (Binary(B::Remainder), 10),
        TokenKind::StarStar => (Binary(B::Exponent), 11),
        _ => return None,
    };
    Some(entry)
}

/// The precedence of `|`: the operands of `??` (§13.13).
const BIT_OR_PRECEDENCE: u8 = 3;

/// The precedence of the relational operators (`#x in o`, §13.10).
const RELATIONAL_PRECEDENCE: u8 = 7;

/// The precedence of the shift operators: the right operand of
/// `#x in` is a `ShiftExpression`.
const SHIFT_PRECEDENCE: u8 = 8;

/// The assignment operator of a token.
fn assignment_operator(kind: TokenKind) -> Option<AssignOp> {
    use BinaryOp as B;
    let op = match kind {
        TokenKind::Eq => AssignOp::Assign,
        TokenKind::PlusEq => AssignOp::Compound(B::Add),
        TokenKind::MinusEq => AssignOp::Compound(B::Subtract),
        TokenKind::StarEq => AssignOp::Compound(B::Multiply),
        TokenKind::SlashEq => AssignOp::Compound(B::Divide),
        TokenKind::PercentEq => AssignOp::Compound(B::Remainder),
        TokenKind::StarStarEq => AssignOp::Compound(B::Exponent),
        TokenKind::ShlEq => AssignOp::Compound(B::ShiftLeft),
        TokenKind::ShrEq => AssignOp::Compound(B::ShiftRight),
        TokenKind::UShrEq => AssignOp::Compound(B::ShiftRightUnsigned),
        TokenKind::AmpEq => AssignOp::Compound(B::BitAnd),
        TokenKind::PipeEq => AssignOp::Compound(B::BitOr),
        TokenKind::CaretEq => AssignOp::Compound(B::BitXor),
        TokenKind::AmpAmpEq => AssignOp::Logical(LogicalOp::And),
        TokenKind::PipePipeEq => AssignOp::Logical(LogicalOp::Or),
        TokenKind::QuestionQuestionEq => AssignOp::Logical(LogicalOp::Coalesce),
        _ => return None,
    };
    Some(op)
}

/// Whether a token can start an expression (for the optional operand of
/// `yield`).
pub(super) fn starts_expression(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::PrivateName
            | TokenKind::Number
            | TokenKind::BigInt
            | TokenKind::String
            | TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateHead
            | TokenKind::RegExp
            | TokenKind::Slash
            | TokenKind::SlashEq
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Bang
            | TokenKind::Tilde
            | TokenKind::PlusPlus
            | TokenKind::MinusMinus
            | TokenKind::Lt
            | TokenKind::This
            | TokenKind::Null
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Function
            | TokenKind::Class
            | TokenKind::New
            | TokenKind::Typeof
            | TokenKind::Void
            | TokenKind::Delete
            | TokenKind::Yield
            | TokenKind::Await
            | TokenKind::Super
            | TokenKind::Import
    )
}

/// Whether an expression of this kind is a property access that can be
/// assigned to (§13.15.1: not in an optional chain).
pub(super) fn is_property_target(kind: ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Member { .. }
            | ExprKind::Index { .. }
            | ExprKind::PrivateMember { .. }
            | ExprKind::SuperMember { .. }
            | ExprKind::SuperIndex { .. }
    )
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    pub(super) fn push_expr(&mut self, kind: ExprKind, start: u32) -> ExprId {
        self.ast.push_expr(kind, Span::new(start, self.prev_end))
    }

    fn kind_of(&self, expr: ExprId) -> ExprKind {
        self.ast.expr(expr).kind
    }

    /// The expression inside any parentheses.
    pub(super) fn unparenthesized(&self, mut expr: ExprId) -> ExprId {
        while let ExprKind::Paren(inner) = self.kind_of(expr) {
            expr = inner;
        }
        expr
    }

    /// Expression: assignment expressions separated by commas.
    pub(super) fn parse_expression(&mut self, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        let first = self.parse_assignment(no_in)?;
        self.parse_expression_rest(first, start, no_in)
    }

    /// The rest of an expression after its first assignment expression
    /// `first`, which starts at `start`.
    pub(super) fn parse_expression_rest(
        &mut self,
        first: ExprId,
        start: u32,
        no_in: bool,
    ) -> PResult<ExprId> {
        if !self.at(TokenKind::Comma) {
            return Ok(first);
        }
        let mark = self.scratch_exprs.len();
        self.scratch_exprs.push(first);
        let result = self.parse_sequence_items(no_in);
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        result?;
        Ok(self.push_expr(ExprKind::Sequence(list), start))
    }

    fn parse_sequence_items(&mut self, no_in: bool) -> PResult<()> {
        while self.eat(TokenKind::Comma)? {
            let next = self.parse_assignment(no_in)?;
            self.scratch_exprs.push(next);
        }
        Ok(())
    }

    /// `AssignmentExpression` (§13.15), including arrow functions and
    /// `yield`. An early error that waits for the cover grammar
    /// ([`Parser::cover_error`]) is reported here: the expression is
    /// complete and was not converted into a pattern.
    pub(super) fn parse_assignment(&mut self, no_in: bool) -> PResult<ExprId> {
        let outer = self.cover_error.take();
        self.enter()?;
        let expr = self.parse_assignment_inner(no_in);
        self.leave();
        let expr = expr?;
        self.check_cover_error()?;
        self.cover_error = outer;
        Ok(expr)
    }

    /// An `AssignmentExpression` that may still become a pattern: an
    /// element of an array literal, a property value of an object
    /// literal, an element in parentheses (arrow parameters) or the start
    /// of a `for` head. If it is an array or object literal, an early
    /// error that waits for the cover grammar stays pending for the
    /// caller.
    pub(super) fn parse_assignment_cover(&mut self, no_in: bool) -> PResult<ExprId> {
        let outer = self.cover_error.take();
        self.enter()?;
        let expr = self.parse_assignment_inner(no_in);
        self.leave();
        let expr = expr?;
        if self.cover_error.is_some()
            && !matches!(self.kind_of(expr), ExprKind::Array(_) | ExprKind::Object(_))
        {
            self.check_cover_error()?;
        }
        self.cover_error = match (outer, self.cover_error) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (a, b) => a.or(b),
        };
        Ok(expr)
    }

    /// Reports the pending early error of the cover grammar, if any.
    pub(super) fn check_cover_error(&mut self) -> PResult<()> {
        match self.cover_error.take() {
            Some((offset, message)) => Err(ParseError::syntax(offset, message)),
            None => Ok(()),
        }
    }

    /// Notes an early error that applies unless the expression becomes a
    /// pattern; the first one in the source counts.
    fn note_cover_error(&mut self, offset: u32, message: &'static str) {
        if self.cover_error.is_none_or(|(first, _)| offset < first) {
            self.cover_error = Some((offset, message));
        }
    }

    fn parse_assignment_inner(&mut self, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        if self.at(TokenKind::Yield) && self.ctx.generator {
            return self.parse_yield(no_in);
        }
        if self.at(TokenKind::LParen) && self.peek_kind()? == TokenKind::RParen {
            // `() => ...`: the only place where `()` is valid.
            self.advance()?;
            let arrow = self.peek()?;
            if arrow.kind == TokenKind::Arrow && arrow.newline_before {
                // V8 reports the `=>` after a line break.
                let arrow = arrow.clone();
                return Err(self.unexpected_token(&arrow));
            }
            if arrow.kind != TokenKind::Arrow {
                return Err(self.unexpected());
            }
            self.advance()?;
            let arrow = ArrowStart {
                start,
                mark: None,
                is_async: false,
            };
            return self.parse_arrow_function(arrow, ArrowHead::Empty, no_in);
        }
        if self.at_contextual(names::ASYNC) {
            let next = self.peek()?;
            if !next.newline_before
                && matches!(
                    next.kind,
                    TokenKind::Identifier | TokenKind::Yield | TokenKind::Await
                )
            {
                // `async x => ...` (§15.9).
                self.advance()?;
                return self.parse_async_arrow_identifier(start, no_in);
            }
        }
        let mark = self.recorded.len();
        let left = self.parse_conditional(no_in)?;
        if self.at(TokenKind::Arrow) {
            if self.token.newline_before {
                return Err(self.unexpected());
            }
            self.cover_error = None;
            return self.parse_arrow_from_cover(left, start, mark, no_in);
        }
        let Some(op) = assignment_operator(self.token.kind) else {
            return Ok(left);
        };
        let target = if op == AssignOp::Assign
            && matches!(self.kind_of(left), ExprKind::Array(_) | ExprKind::Object(_))
        {
            let pattern = self.assignment_pattern(left)?;
            self.cover_error = None;
            AssignTarget::Pattern(pattern)
        } else {
            // V8 reports the target of `{a = 1} += 1` before the
            // `CoverInitializedName`.
            let literal = matches!(self.kind_of(left), ExprKind::Array(_) | ExprKind::Object(_));
            if !literal {
                self.check_cover_error()?;
            }
            self.assignment_target(left, op)?
        };
        self.advance()?;
        let value = self.parse_assignment(no_in)?;
        Ok(self.push_expr(ExprKind::Assign { op, target, value }, start))
    }

    /// The target of an assignment with `op` (§13.15.1) other than a
    /// destructuring pattern.
    fn assignment_target(&mut self, left: ExprId, op: AssignOp) -> PResult<AssignTarget> {
        let start = self.ast.expr(left).span.start;
        let target = self.unparenthesized(left);
        match self.kind_of(target) {
            ExprKind::Identifier(ident) => {
                self.check_strict_target(ident.name, start)?;
                Ok(AssignTarget::Simple(target))
            }
            kind if is_property_target(kind) => Ok(AssignTarget::Simple(target)),
            // Chromium accepts `f() = x` and throws a ReferenceError at run
            // time (web compatibility); later editions of ECMA-262 allow
            // it in sloppy mode code, except for the logical operators.
            ExprKind::Call { .. } if !self.ctx.strict && !matches!(op, AssignOp::Logical(_)) => {
                Ok(AssignTarget::Simple(target))
            }
            _ => Err(ParseError::syntax(
                start,
                messages::INVALID_ASSIGNMENT_TARGET,
            )),
        }
    }

    /// Strict mode code cannot assign to `eval` or `arguments` (§13.1.1).
    pub(super) fn check_strict_target(&self, name: crate::NameId, offset: u32) -> PResult<()> {
        if self.ctx.strict && super::is_eval_or_arguments(name) {
            return Err(ParseError::syntax(
                offset,
                messages::UNEXPECTED_EVAL_OR_ARGUMENTS,
            ));
        }
        Ok(())
    }

    /// The target of `++` or `--` (§13.4.1).
    fn update_target(&mut self, operand: ExprId, message: &'static str) -> PResult<ExprId> {
        let start = self.ast.expr(operand).span.start;
        let target = self.unparenthesized(operand);
        match self.kind_of(target) {
            ExprKind::Identifier(ident) => {
                self.check_strict_target(ident.name, start)?;
                Ok(target)
            }
            kind if is_property_target(kind) => Ok(target),
            ExprKind::Call { .. } if !self.ctx.strict => Ok(target),
            _ => {
                // V8 reports a pending `CoverInitializedName` first.
                self.check_cover_error()?;
                Err(ParseError::syntax(start, message))
            }
        }
    }

    /// `YieldExpression` (§15.5): `yield` with an optional operand on the
    /// same line.
    fn parse_yield(&mut self, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        if self.ctx.in_params {
            return Err(ParseError::syntax(start, messages::YIELD_IN_PARAMETER));
        }
        self.ctx.last_yield = Some(start);
        self.advance()?;
        if self.token.newline_before {
            return Ok(self.push_expr(
                ExprKind::Yield {
                    argument: None,
                    delegate: false,
                },
                start,
            ));
        }
        if self.eat(TokenKind::Star)? {
            let argument = self.parse_assignment(no_in)?;
            return Ok(self.push_expr(
                ExprKind::Yield {
                    argument: Some(argument),
                    delegate: true,
                },
                start,
            ));
        }
        let argument = if starts_expression(self.token.kind) {
            Some(self.parse_assignment(no_in)?)
        } else {
            None
        };
        Ok(self.push_expr(
            ExprKind::Yield {
                argument,
                delegate: false,
            },
            start,
        ))
    }

    /// `ConditionalExpression` (§13.14).
    fn parse_conditional(&mut self, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        let test = self.parse_binary(0, no_in)?;
        if !self.eat(TokenKind::Question)? {
            return Ok(test);
        }
        let consequent = self.parse_assignment(false)?;
        self.expect(TokenKind::Colon)?;
        let alternate = self.parse_assignment(no_in)?;
        Ok(self.push_expr(
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            },
            start,
        ))
    }

    /// The binary operators of precedence `min` and higher, by precedence
    /// climbing (§13.6 to §13.13).
    fn parse_binary(&mut self, min: u8, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        let mut left = if self.at(TokenKind::PrivateName) {
            self.parse_private_in(min, no_in)?
        } else {
            self.parse_unary()?
        };
        let mut links = 0u32;
        let result = loop {
            let Some((op, precedence)) = binary_operator(self.token.kind, no_in) else {
                break Ok(left);
            };
            if precedence < min {
                break Ok(left);
            }
            if let Err(error) = self.check_operand_mix(left, op) {
                break Err(error);
            }
            if let Err(error) = self.charge(CHAIN_WEIGHT) {
                break Err(error);
            }
            links += 1;
            match self.parse_binary_link(left, op, precedence, no_in, start) {
                Ok(expr) => left = expr,
                Err(error) => break Err(error),
            }
        };
        self.budget.leave(links * CHAIN_WEIGHT);
        result
    }

    /// Reads the operator and the right operand of one binary link.
    fn parse_binary_link(
        &mut self,
        left: ExprId,
        op: Operator,
        precedence: u8,
        no_in: bool,
        start: u32,
    ) -> PResult<ExprId> {
        self.advance()?;
        let kind = match op {
            Operator::Binary(BinaryOp::Exponent) => {
                // Right-associative: the right operand is again an
                // ExponentiationExpression.
                self.enter_small()?;
                let right = self.parse_binary(precedence, no_in);
                self.leave_small();
                ExprKind::Binary {
                    op: BinaryOp::Exponent,
                    left,
                    right: right?,
                }
            }
            Operator::Binary(op) => {
                let right = self.parse_binary(precedence + 1, no_in)?;
                ExprKind::Binary { op, left, right }
            }
            Operator::Logical(LogicalOp::Coalesce) => {
                let right = self.parse_binary(BIT_OR_PRECEDENCE, no_in)?;
                ExprKind::Logical {
                    op: LogicalOp::Coalesce,
                    left,
                    right,
                }
            }
            Operator::Logical(op) => {
                let right = self.parse_binary(precedence + 1, no_in)?;
                ExprKind::Logical { op, left, right }
            }
        };
        Ok(self.push_expr(kind, start))
    }

    /// The early errors that depend on the left operand of an operator:
    /// `??` does not mix with `&&` and `||` without parentheses (§13.13),
    /// and the left operand of `**` is not a unary expression (§13.6).
    fn check_operand_mix(&self, left: ExprId, op: Operator) -> PResult<()> {
        let left_kind = self.kind_of(left);
        let error = match (op, left_kind) {
            (
                Operator::Logical(LogicalOp::Coalesce),
                ExprKind::Logical {
                    op: LogicalOp::And | LogicalOp::Or,
                    ..
                },
            )
            | (
                Operator::Logical(LogicalOp::And | LogicalOp::Or),
                ExprKind::Logical {
                    op: LogicalOp::Coalesce,
                    ..
                },
            ) => self.unexpected(),
            (Operator::Binary(BinaryOp::Exponent), ExprKind::Unary { .. } | ExprKind::Await(_)) => {
                ParseError::syntax(
                    self.ast.expr(self.innermost_unary(left)).span.start,
                    messages::UNARY_BEFORE_EXPONENT,
                )
            }
            _ => return Ok(()),
        };
        Err(error)
    }

    /// The innermost unary or `await` expression in the chain of operands
    /// that starts at `expr`. V8 reports `**` after a unary expression
    /// while it parses the innermost one (`-await 1 ** 2` marks `await`).
    fn innermost_unary(&self, expr: ExprId) -> ExprId {
        let mut current = expr;
        while let ExprKind::Unary { argument, .. } | ExprKind::Await(argument) =
            self.kind_of(current)
        {
            if !matches!(
                self.kind_of(argument),
                ExprKind::Unary { .. } | ExprKind::Await(_)
            ) {
                break;
            }
            current = argument;
        }
        current
    }

    /// `#x in object` (§13.10): a private name is valid only as the left
    /// operand of `in` at the start of a relational expression.
    fn parse_private_in(&mut self, min: u8, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        if self.classes.is_empty() {
            // V8 reports this before the form of the expression.
            let name = self.current_private_id()?;
            return Err(self.undeclared_private(name, self.prev_start));
        }
        if min > RELATIONAL_PRECEDENCE || no_in || self.peek_kind()? != TokenKind::In {
            return Err(self.unexpected());
        }
        let name = self.private_name_use()?;
        self.advance()?;
        self.advance()?;
        let object = self.parse_binary(SHIFT_PRECEDENCE, no_in)?;
        Ok(self.push_expr(ExprKind::PrivateIn { name, object }, start))
    }

    /// The current private name token as a use (a reference that must
    /// resolve to a private name of an enclosing class); does not advance.
    /// V8 reports an undeclared name at the token before it (the `.` of
    /// `this.#x`).
    pub(super) fn private_name_use(&mut self) -> PResult<crate::Ident> {
        let start = self.token.start;
        let name = self.current_private_id()?;
        self.note_private_use(name, self.prev_start)?;
        Ok(self.reference(name, start, false))
    }

    /// `UnaryExpression` and prefix `UpdateExpression` (§13.4, §13.5), and
    /// `AwaitExpression` (§15.8).
    fn parse_unary(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let op = match self.token.kind {
            TokenKind::Delete => UnaryOp::Delete,
            TokenKind::Void => UnaryOp::Void,
            TokenKind::Typeof => UnaryOp::Typeof,
            TokenKind::Plus => UnaryOp::Plus,
            TokenKind::Minus => UnaryOp::Minus,
            TokenKind::Tilde => UnaryOp::BitNot,
            TokenKind::Bang => UnaryOp::Not,
            TokenKind::PlusPlus | TokenKind::MinusMinus => return self.parse_prefix_update(),
            TokenKind::Await if self.ctx.await_mode == AwaitMode::Expression => {
                return self.parse_await();
            }
            _ => return self.parse_postfix(),
        };
        self.advance()?;
        self.enter_small()?;
        let argument = self.parse_unary();
        self.leave_small();
        let argument = argument?;
        if op == UnaryOp::Delete {
            self.check_delete(argument)?;
        }
        Ok(self.push_expr(ExprKind::Unary { op, argument }, start))
    }

    /// The early errors of `delete` (§13.5.1.1): an identifier in strict
    /// mode code, a private member (V8 marks the private name, or the
    /// closing parenthesis around it).
    fn check_delete(&self, argument: ExprId) -> PResult<()> {
        let operand = self.unparenthesized(argument);
        let operand = match self.kind_of(operand) {
            ExprKind::OptionalChain(chain) => chain,
            _ => operand,
        };
        match self.kind_of(operand) {
            ExprKind::Identifier(_) if self.ctx.strict => Err(ParseError::syntax(
                self.last_token_start(argument),
                messages::DELETE_IDENTIFIER,
            )),
            ExprKind::PrivateMember { name, .. } => {
                let offset = if operand == argument
                    || matches!(self.kind_of(argument), ExprKind::OptionalChain(_))
                {
                    self.scopes.reference(name.reference).offset
                } else {
                    self.ast.expr(argument).span.end.saturating_sub(1)
                };
                Err(ParseError::syntax(offset, messages::DELETE_PRIVATE))
            }
            _ => Ok(()),
        }
    }

    /// `await UnaryExpression` (§15.8) in an async function.
    fn parse_await(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        if self.ctx.in_params {
            return Err(ParseError::syntax(start, messages::AWAIT_IN_PARAMETER));
        }
        self.ctx.last_await = Some(start);
        self.advance()?;
        self.enter_small()?;
        let argument = self.parse_unary();
        self.leave_small();
        let argument = argument?;
        Ok(self.push_expr(ExprKind::Await(argument), start))
    }

    fn parse_prefix_update(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let op = if self.at(TokenKind::PlusPlus) {
            UpdateOp::Increment
        } else {
            UpdateOp::Decrement
        };
        self.advance()?;
        self.enter_small()?;
        let operand = self.parse_unary();
        self.leave_small();
        let target = self.update_target(operand?, messages::INVALID_PREFIX_TARGET)?;
        Ok(self.push_expr(
            ExprKind::Update {
                op,
                prefix: true,
                target,
            },
            start,
        ))
    }

    /// Postfix `UpdateExpression`: no line terminator before `++` or `--`.
    fn parse_postfix(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let operand = self.parse_left_hand_side()?;
        let op = match self.token.kind {
            TokenKind::PlusPlus if !self.token.newline_before => UpdateOp::Increment,
            TokenKind::MinusMinus if !self.token.newline_before => UpdateOp::Decrement,
            _ => return Ok(operand),
        };
        let target = self.update_target(operand, messages::INVALID_POSTFIX_TARGET)?;
        self.advance()?;
        Ok(self.push_expr(
            ExprKind::Update {
                op,
                prefix: false,
                target,
            },
            start,
        ))
    }

    /// `LeftHandSideExpression`: `new`, member access and calls (§13.3).
    pub(super) fn parse_left_hand_side(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let object = if self.at(TokenKind::New) {
            self.parse_new()?
        } else {
            self.parse_primary()?
        };
        self.parse_chain(object, start, true)
    }

    /// The member accesses, tagged templates and (with `calls`) the calls
    /// after `object`. Each link charges the budget: the chain makes the
    /// tree deeper. A chain with `?.` is wrapped in an
    /// [`ExprKind::OptionalChain`] (§13.3.9).
    fn parse_chain(&mut self, mut object: ExprId, start: u32, calls: bool) -> PResult<ExprId> {
        let mut links = 0u32;
        let mut optional = false;
        let result = loop {
            let link = match self.token.kind {
                TokenKind::Dot | TokenKind::LBracket => self.parse_member(object, start),
                TokenKind::LParen if calls => {
                    let async_head = links == 0 && self.is_async_callee(object);
                    self.parse_call(object, start, false, async_head)
                }
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead if optional => Err(
                    ParseError::syntax(self.token.start, messages::OPTIONAL_CHAIN_TEMPLATE),
                ),
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                    self.parse_tagged_template(object, start)
                }
                TokenKind::QuestionDot if !calls => Err(ParseError::syntax(
                    self.token.start,
                    messages::OPTIONAL_CHAIN_NEW,
                )),
                TokenKind::QuestionDot => {
                    optional = true;
                    self.parse_optional_link(object, start)
                }
                _ => break Ok(object),
            };
            match link.and_then(|expr| self.charge(CHAIN_WEIGHT).map(|()| expr)) {
                Ok(expr) => {
                    links += 1;
                    object = expr;
                }
                Err(error) => break Err(error),
            }
        };
        self.budget.leave(links * CHAIN_WEIGHT);
        let object = result?;
        if optional {
            return Ok(self.push_expr(ExprKind::OptionalChain(object), start));
        }
        Ok(object)
    }

    /// Whether `callee` followed by the current `(` can be the head of an
    /// async arrow function: the identifier `async` without escapes and
    /// parentheses, with no line terminator before `(` (§15.9).
    fn is_async_callee(&self, callee: ExprId) -> bool {
        let node = self.ast.expr(callee);
        matches!(node.kind, ExprKind::Identifier(ident) if ident.name == names::ASYNC)
            && node.span.end - node.span.start == 5
            && !self.token.newline_before
    }

    /// `.name`, `.#name` or `[expression]` after `object`.
    fn parse_member(&mut self, object: ExprId, start: u32) -> PResult<ExprId> {
        if self.eat(TokenKind::Dot)? {
            if self.at(TokenKind::PrivateName) {
                return self.parse_private_member(object, start, false);
            }
            let property = self.member_name()?;
            return Ok(self.push_expr(
                ExprKind::Member {
                    object,
                    property,
                    optional: false,
                },
                start,
            ));
        }
        self.parse_index(object, start, false)
    }

    /// `.#name` (or `?.#name`) after `object`; the current token is the
    /// private name.
    fn parse_private_member(
        &mut self,
        object: ExprId,
        start: u32,
        optional: bool,
    ) -> PResult<ExprId> {
        let name = self.private_name_use()?;
        self.advance()?;
        Ok(self.push_expr(
            ExprKind::PrivateMember {
                object,
                name,
                optional,
            },
            start,
        ))
    }

    /// The `IdentifierName` after `.` or `?.`; advances.
    fn member_name(&mut self) -> PResult<crate::NameId> {
        let property = match self.token.kind {
            TokenKind::Identifier => self.token.name().unwrap_or(names::AWAIT),
            kind => match kind.keyword_name() {
                Some(name) => name,
                None => return Err(self.unexpected()),
            },
        };
        self.advance()?;
        Ok(property)
    }

    /// `[expression]` after `object`; the current token is `[`.
    fn parse_index(&mut self, object: ExprId, start: u32, optional: bool) -> PResult<ExprId> {
        self.expect(TokenKind::LBracket)?;
        let index = self.parse_expression(false)?;
        self.expect(TokenKind::RBracket)?;
        Ok(self.push_expr(
            ExprKind::Index {
                object,
                index,
                optional,
            },
            start,
        ))
    }

    /// The link after `?.`: `.name`, `[expression]` or a call (§13.3.9).
    fn parse_optional_link(&mut self, object: ExprId, start: u32) -> PResult<ExprId> {
        self.advance()?;
        match self.token.kind {
            TokenKind::LBracket => self.parse_index(object, start, true),
            TokenKind::LParen => self.parse_call(object, start, true, false),
            TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => Err(ParseError::syntax(
                self.token.start,
                messages::OPTIONAL_CHAIN_TEMPLATE,
            )),
            TokenKind::PrivateName => self.parse_private_member(object, start, true),
            _ => {
                let property = self.member_name()?;
                Ok(self.push_expr(
                    ExprKind::Member {
                        object,
                        property,
                        optional: true,
                    },
                    start,
                ))
            }
        }
    }

    /// A call with its arguments. A call of the plain name `eval` (also
    /// in parentheses) is a direct `eval` (§13.3.6.1). With `async_head`
    /// (`async(...)`), the arguments may still become the parameters of
    /// an async arrow function: early errors of the cover grammar stay
    /// pending ([`Parser::cover_error`]).
    fn parse_call(
        &mut self,
        callee: ExprId,
        start: u32,
        optional: bool,
        async_head: bool,
    ) -> PResult<ExprId> {
        let (arguments, spread_comma) = self.parse_arguments_cover(async_head)?;
        let target = self.unparenthesized(callee);
        if !optional
            && let ExprKind::Identifier(ident) = self.kind_of(target)
            && ident.name == names::EVAL
        {
            self.ctx.direct_evals += 1;
            self.recorded.push(Recorded::Eval);
        }
        let call = self.push_expr(
            ExprKind::Call {
                callee,
                arguments,
                optional,
            },
            start,
        );
        if async_head {
            self.async_heads.insert(call, spread_comma);
        }
        Ok(call)
    }

    /// A tagged template (§13.3.11): `tag` is called with the template.
    fn parse_tagged_template(&mut self, tag: ExprId, start: u32) -> PResult<ExprId> {
        let template = self.parse_template_literal(true)?;
        Ok(self.push_expr(ExprKind::TaggedTemplate { tag, template }, start))
    }

    /// Arguments (§13.3.8): `(a, ...b,)`.
    pub(super) fn parse_arguments(&mut self) -> PResult<List<ExprId>> {
        Ok(self.parse_arguments_cover(false)?.0)
    }

    /// Arguments; with `cover`, the arguments of `async(...)`, which can
    /// become arrow parameters. Returns the list and the offset of the
    /// comma after the first spread argument, if any.
    fn parse_arguments_cover(&mut self, cover: bool) -> PResult<(List<ExprId>, Option<u32>)> {
        let open = self.token.start;
        self.expect(TokenKind::LParen)?;
        let mark = self.scratch_exprs.len();
        let result = self.parse_argument_items(cover);
        let count = self.scratch_exprs.len() - mark;
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        let spread_comma = result?;
        if count > usize::from(u16::MAX) {
            return Err(ParseError::syntax(open, messages::TOO_MANY_ARGUMENTS));
        }
        Ok((list, spread_comma))
    }

    /// The arguments up to and including `)`.
    fn parse_argument_items(&mut self, cover: bool) -> PResult<Option<u32>> {
        let mut spread_comma = None;
        while !self.at(TokenKind::RParen) {
            let start = self.token.start;
            let spread = self.eat(TokenKind::Ellipsis)?;
            let mut argument = if cover {
                self.parse_assignment_cover(false)?
            } else {
                self.parse_assignment(false)?
            };
            if spread {
                argument = self.push_expr(ExprKind::Spread(argument), start);
            }
            self.scratch_exprs.push(argument);
            if self.at(TokenKind::RParen) {
                break;
            }
            let comma = self.token.start;
            if !self.eat(TokenKind::Comma)? {
                return Err(ParseError::syntax(
                    self.prev_start,
                    messages::MISSING_PAREN_AFTER_ARGUMENTS,
                ));
            }
            if spread && spread_comma.is_none() {
                spread_comma = Some(comma);
            }
        }
        self.advance()?;
        Ok(spread_comma)
    }

    /// `new` `MemberExpression` Arguments, or `new` `NewExpression`
    /// (§13.3.5), or `new.target` (§13.3.12).
    fn parse_new(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        if self.eat(TokenKind::Dot)? {
            return self.parse_new_target(start);
        }
        self.enter_small()?;
        let callee = self.parse_new_callee();
        self.leave_small();
        let callee = callee?;
        let arguments = if self.at(TokenKind::LParen) {
            self.parse_arguments()?
        } else {
            List::EMPTY
        };
        Ok(self.push_expr(ExprKind::New { callee, arguments }, start))
    }

    /// `new.target` after `new.`: only in functions (and arrow functions
    /// inside them).
    fn parse_new_target(&mut self, start: u32) -> PResult<ExprId> {
        if !(self.token.kind == TokenKind::Identifier && self.token.name() == Some(names::TARGET)) {
            return Err(self.unexpected());
        }
        if self.token.escaped {
            return Err(ParseError::syntax(start, messages::ESCAPED_NEW_TARGET));
        }
        if !self.ctx.new_target {
            return Err(ParseError::syntax(
                self.token.start,
                messages::NEW_TARGET_OUTSIDE_FUNCTION,
            ));
        }
        let reference = self.reference(names::NEW_TARGET, start, false).reference;
        self.advance()?;
        Ok(self.push_expr(ExprKind::NewTarget(reference), start))
    }

    /// The `MemberExpression` after `new`: no calls.
    fn parse_new_callee(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let callee = match self.token.kind {
            TokenKind::New => self.parse_new()?,
            TokenKind::Super => self.parse_super(false)?,
            _ => self.parse_primary()?,
        };
        self.parse_chain(callee, start, false)
    }

    /// `super.name`, `super[expression]` (in methods) or `super(...)` (in
    /// derived constructors; not after `new`), §13.3.7. V8 marks `super`
    /// for every misplaced form.
    fn parse_super(&mut self, calls: bool) -> PResult<ExprId> {
        let start = self.token.start;
        let misplaced = || Err(ParseError::syntax(start, messages::UNEXPECTED_SUPER));
        self.advance()?;
        match self.token.kind {
            TokenKind::Dot | TokenKind::LBracket if !self.ctx.super_property => misplaced(),
            TokenKind::Dot => {
                self.advance()?;
                if self.at(TokenKind::PrivateName) {
                    return Err(ParseError::syntax(
                        self.token.start,
                        messages::UNEXPECTED_PRIVATE_FIELD,
                    ));
                }
                let this = self.reference(names::THIS, start, false).reference;
                let home = self.reference(names::SUPER, start, false).reference;
                let property = self.member_name()?;
                Ok(self.push_expr(
                    ExprKind::SuperMember {
                        property,
                        this,
                        home,
                    },
                    start,
                ))
            }
            TokenKind::LBracket => {
                let this = self.reference(names::THIS, start, false).reference;
                let home = self.reference(names::SUPER, start, false).reference;
                self.advance()?;
                let index = self.parse_expression(false)?;
                self.expect(TokenKind::RBracket)?;
                Ok(self.push_expr(ExprKind::SuperIndex { index, this, home }, start))
            }
            TokenKind::LParen if calls && self.ctx.super_call => {
                let this = self.reference(names::THIS, start, false).reference;
                let new_target = self.reference(names::NEW_TARGET, start, false).reference;
                let function = self
                    .reference(names::ACTIVE_FUNCTION, start, false)
                    .reference;
                let arguments = self.parse_arguments()?;
                let call = self.ast.push_super_call(SuperCall {
                    arguments,
                    this,
                    new_target,
                    function,
                });
                Ok(self.push_expr(ExprKind::SuperCall(call), start))
            }
            _ => misplaced(),
        }
    }

    /// `PrimaryExpression` (§13.2).
    fn parse_primary(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let kind = match self.token.kind {
            TokenKind::This => {
                let reference = self.reference(names::THIS, start, false).reference;
                ExprKind::This(reference)
            }
            TokenKind::Identifier | TokenKind::Yield | TokenKind::Await => {
                return self.parse_identifier_reference();
            }
            TokenKind::Null => ExprKind::Null,
            TokenKind::True => ExprKind::Boolean(true),
            TokenKind::False => ExprKind::Boolean(false),
            TokenKind::Number => {
                self.check_legacy(&self.token)?;
                match self.token.value {
                    TokenValue::Number(value) => ExprKind::Number(value),
                    _ => ExprKind::Number(f64::NAN),
                }
            }
            TokenKind::String => {
                self.check_legacy(&self.token)?;
                let value = match &self.token.value {
                    TokenValue::String(value) => value.clone(),
                    _ => swb_js_text::String16::new(),
                };
                ExprKind::String(self.ast.push_string(value))
            }
            TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                let template = self.parse_template_literal(false)?;
                return Ok(self.push_expr(ExprKind::Template(template), start));
            }
            TokenKind::Slash | TokenKind::SlashEq => {
                self.rescan(Goal::RegExp)?;
                return self.parse_regexp();
            }
            TokenKind::RegExp => return self.parse_regexp(),
            TokenKind::LParen => return self.parse_parenthesized(),
            TokenKind::LBracket => return self.parse_array(),
            TokenKind::LBrace => return self.parse_object(),
            TokenKind::Function => return self.parse_function_expression(start, false),
            TokenKind::Class => {
                let class = self.parse_class(false)?;
                return Ok(self.push_expr(ExprKind::Class(class), start));
            }
            TokenKind::Super => return self.parse_super(true),
            TokenKind::Import => return Err(ParseError::unsupported(start, "import")),
            TokenKind::BigInt => {
                let digits = match &self.token.value {
                    TokenValue::BigInt(text) => swb_js_text::String16::from(&**text),
                    _ => swb_js_text::String16::new(),
                };
                ExprKind::BigInt(self.ast.push_string(digits))
            }
            _ => return Err(self.unexpected()),
        };
        self.advance()?;
        Ok(self.push_expr(kind, start))
    }

    /// `IdentifierReference` (§13.1), or an async function expression
    /// (`async function`, §15.8).
    pub(super) fn parse_identifier_reference(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        if self.at_contextual(names::ASYNC) {
            let next = self.peek()?;
            if next.kind == TokenKind::Function && !next.newline_before {
                self.advance()?;
                return self.parse_function_expression(start, true);
            }
        }
        let name = self.check_identifier(IdentUse::Reference)?;
        if name == names::AWAIT {
            self.ctx.last_await_name = Some(start);
        }
        let ident = self.reference(name, start, false);
        self.advance()?;
        Ok(self.push_expr(ExprKind::Identifier(ident), start))
    }

    /// A regular expression literal; the lexer has checked the flags.
    fn parse_regexp(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let TokenValue::RegExp { body_end, flags } = self.token.value else {
            return Err(self.unexpected());
        };
        let pattern = self
            .lexer
            .text(start + 1, body_end)
            .map(swb_js_text::Str16::to_string16)
            .unwrap_or_default();
        let pattern = self.ast.push_string(pattern);
        self.advance()?;
        Ok(self.push_expr(ExprKind::RegExp { pattern, flags }, start))
    }

    /// A template literal (§13.2.8); `tagged` for the template of a tagged
    /// template, whose parts may have invalid escapes.
    fn parse_template_literal(&mut self, tagged: bool) -> PResult<TemplateId> {
        let quasi_mark = self.scratch_quasis.len();
        let expr_mark = self.scratch_exprs.len();
        let result = self.parse_template_parts(tagged);
        let quasis = self
            .ast
            .push_template_elements(&self.scratch_quasis[quasi_mark..]);
        let expressions = self.ast.push_exprs(&self.scratch_exprs[expr_mark..]);
        self.scratch_quasis.truncate(quasi_mark);
        self.scratch_exprs.truncate(expr_mark);
        result?;
        Ok(self.ast.push_template(Template {
            quasis,
            expressions,
        }))
    }

    fn parse_template_parts(&mut self, tagged: bool) -> PResult<()> {
        loop {
            let element = self.template_element(tagged)?;
            self.scratch_quasis.push(element);
            let tail = matches!(
                self.token.kind,
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateTail
            );
            self.advance()?;
            if tail {
                return Ok(());
            }
            let expression = self.parse_expression(false)?;
            self.scratch_exprs.push(expression);
            if !self.at(TokenKind::RBrace) {
                return Err(ParseError::syntax(
                    self.prev_start,
                    messages::MISSING_TEMPLATE_BRACE,
                ));
            }
            self.rescan(Goal::TemplateTail)?;
        }
    }

    /// The string part of the current template token. Without a tag, an
    /// invalid escape is a syntax error (§13.2.8.1); with a tag, the part
    /// has no cooked value.
    fn template_element(&mut self, tagged: bool) -> PResult<TemplateElement> {
        let TokenValue::Template(template) = &self.token.value else {
            return Err(self.unexpected());
        };
        let cooked = match &template.cooked {
            Ok(cooked) => Some(cooked.clone()),
            Err(_) if tagged => None,
            Err(error) => return Err(error.clone().into()),
        };
        let raw = template.raw.clone();
        Ok(TemplateElement {
            cooked: cooked.map(|cooked| self.ast.push_string(cooked)),
            raw: self.ast.push_string(raw),
        })
    }

    /// `CoverParenthesizedExpressionAndArrowParameterList` (§13.2): a
    /// parenthesized expression, or the parameters of an arrow function,
    /// which [`Parser::parse_arrow_from_cover`] converts at `=>`. A rest
    /// element or a trailing comma requires the `=>`.
    fn parse_parenthesized(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        if self.at(TokenKind::RParen) {
            // `()` before `=>` where no arrow function can start
            // (`a + () => 1`): an empty group, which the `=>` then rejects
            // as V8 does ("Malformed arrow function parameter list", or
            // "Unexpected token '=>'" after a heritage).
            if self.peek_kind()? != TokenKind::Arrow {
                return Err(self.unexpected());
            }
            let empty = self.ast.push_exprs(&[]);
            let inner = self.push_expr(ExprKind::Sequence(empty), self.token.start);
            self.advance()?;
            return Ok(self.push_expr(ExprKind::Paren(inner), start));
        }
        let inner_start = self.token.start;
        let mark = self.scratch_exprs.len();
        let result = self.parse_parenthesized_items();
        let inner = match &self.scratch_exprs[mark..] {
            [only] => *only,
            items => {
                let list = self.ast.push_exprs(items);
                self.push_expr(ExprKind::Sequence(list), inner_start)
            }
        };
        self.scratch_exprs.truncate(mark);
        result?;
        Ok(self.push_expr(ExprKind::Paren(inner), start))
    }

    /// The elements in parentheses, up to and including `)`.
    fn parse_parenthesized_items(&mut self) -> PResult<()> {
        loop {
            if self.at(TokenKind::Ellipsis) {
                let rest = self.token.clone();
                self.advance()?;
                let argument = self.parse_assignment_cover(false)?;
                let spread = self.push_expr(ExprKind::Spread(argument), rest.start);
                self.scratch_exprs.push(spread);
                if let ExprKind::Yield { .. } = self.kind_of(argument) {
                    return Err(ParseError::syntax(
                        self.ast.expr(argument).span.start,
                        messages::unexpected_identifier("yield"),
                    ));
                }
                if !matches!(
                    self.kind_of(argument),
                    ExprKind::Identifier(_)
                        | ExprKind::Array(_)
                        | ExprKind::Object(_)
                        | ExprKind::Assign { .. }
                ) {
                    return Err(self.unexpected_token(&rest));
                }
                if self.at(TokenKind::Comma) {
                    return Err(self.rest_parameter_error(argument));
                }
                self.expect(TokenKind::RParen)?;
                if self.token.kind != TokenKind::Arrow {
                    return Err(self.unexpected_token(&rest));
                }
                return Ok(());
            }
            let item = self.parse_assignment_cover(false)?;
            self.scratch_exprs.push(item);
            if !self.eat(TokenKind::Comma)? {
                return self.expect(TokenKind::RParen);
            }
            if self.at(TokenKind::RParen) {
                // A trailing comma: only arrow parameters allow it.
                if self.peek_kind()? != TokenKind::Arrow {
                    return Err(self.unexpected());
                }
                return self.advance();
            }
        }
    }

    /// An array literal (§13.2.4) with holes and spread elements.
    fn parse_array(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_exprs.len();
        let result = self.parse_array_elements();
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        let trailing = result?;
        let array = self.push_expr(ExprKind::Array(list), start);
        if trailing {
            self.trailing_comma_after_spread.insert(array);
        }
        Ok(array)
    }

    /// The elements up to and including `]`; returns whether a comma
    /// follows a spread element at the end (`[...a,]`).
    fn parse_array_elements(&mut self) -> PResult<bool> {
        let mut trailing = false;
        while !self.at(TokenKind::RBracket) {
            if self.at(TokenKind::Comma) {
                let hole_start = self.token.start;
                self.advance()?;
                let hole = self
                    .ast
                    .push_expr(ExprKind::Hole, Span::new(hole_start, hole_start));
                self.scratch_exprs.push(hole);
                continue;
            }
            let start = self.token.start;
            let spread = self.eat(TokenKind::Ellipsis)?;
            let mut element = self.parse_assignment_cover(false)?;
            if spread {
                element = self.push_expr(ExprKind::Spread(element), start);
            }
            self.scratch_exprs.push(element);
            if !self.at(TokenKind::RBracket) {
                self.expect(TokenKind::Comma)?;
                trailing = spread && self.at(TokenKind::RBracket);
            }
        }
        self.advance()?;
        Ok(trailing)
    }

    /// An object literal (§13.2.5).
    fn parse_object(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_properties.len();
        let result = self.parse_object_properties();
        let list = self.ast.push_properties(&self.scratch_properties[mark..]);
        self.scratch_properties.truncate(mark);
        let trailing = result?;
        let object = self.push_expr(ExprKind::Object(list), start);
        if trailing {
            self.trailing_comma_after_spread.insert(object);
        }
        Ok(object)
    }

    /// The properties up to and including `}`; returns whether a comma
    /// follows a spread property at the end (`{...a,}`).
    fn parse_object_properties(&mut self) -> PResult<bool> {
        let mut has_proto = false;
        let mut trailing = false;
        while !self.at(TokenKind::RBrace) {
            let property = self.parse_property()?;
            if property.kind == PropertyKind::Proto {
                if has_proto {
                    // Not an error in a pattern (§13.2.5.1, Annex B.3.1).
                    self.note_cover_error(property.span.start, messages::DUPLICATE_PROTO);
                }
                has_proto = true;
            }
            self.scratch_properties.push(property);
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
                trailing = property.kind == PropertyKind::Spread && self.at(TokenKind::RBrace);
            }
        }
        self.advance()?;
        Ok(trailing)
    }

    /// A `PropertyDefinition` (§13.2.5).
    fn parse_property(&mut self) -> PResult<Property> {
        let start = self.token.start;
        if self.eat(TokenKind::Star)? {
            let key = self.parse_property_key()?;
            let head = FunctionHead::method(FunctionKind::Method, true, false, start);
            let value = self.parse_method(head)?;
            return Ok(self.property(PropertyKind::Method, key, value, start));
        }
        if self.eat(TokenKind::Ellipsis)? {
            let value = self.parse_assignment_cover(false)?;
            return Ok(self.property(PropertyKind::Spread, PropertyKey::Number(0.0), value, start));
        }
        let key_token = self.token.clone();
        if self.token.kind == TokenKind::Identifier && !self.token.escaped {
            let name = self.token.name();
            let accessor = name == Some(names::GET) || name == Some(names::SET);
            let next = self.peek()?;
            let is_async = name == Some(names::ASYNC) && !next.newline_before;
            if (accessor || is_async)
                && !matches!(
                    next.kind,
                    TokenKind::Comma
                        | TokenKind::Colon
                        | TokenKind::LParen
                        | TokenKind::RBrace
                        | TokenKind::Eq
                )
            {
                self.advance()?;
                if is_async {
                    // `async m() {}`, `async *m() {}` (§15.8, §15.6).
                    let generator = self.eat(TokenKind::Star)?;
                    let key = self.parse_property_key()?;
                    let head = FunctionHead::method(FunctionKind::Method, generator, true, start);
                    let value = self.parse_method(head)?;
                    return Ok(self.property(PropertyKind::Method, key, value, start));
                }
                let (kind, function) = if name == Some(names::GET) {
                    (PropertyKind::Getter, FunctionKind::Getter)
                } else {
                    (PropertyKind::Setter, FunctionKind::Setter)
                };
                let key = self.parse_property_key()?;
                let value =
                    self.parse_method(FunctionHead::method(function, false, false, start))?;
                return Ok(self.property(kind, key, value, start));
            }
        }
        let key = self.parse_property_key()?;
        match self.token.kind {
            TokenKind::Colon => {
                self.advance()?;
                let value = self.parse_assignment_cover(false)?;
                let kind = if key == PropertyKey::Name(names::PROTO) {
                    PropertyKind::Proto
                } else {
                    PropertyKind::Init
                };
                Ok(self.property(kind, key, value, start))
            }
            TokenKind::LParen => {
                let head = FunctionHead::method(FunctionKind::Method, false, false, start);
                let value = self.parse_method(head)?;
                Ok(self.property(PropertyKind::Method, key, value, start))
            }
            TokenKind::Comma | TokenKind::RBrace | TokenKind::Eq => {
                self.parse_shorthand(&key_token, key, start)
            }
            kind => {
                let starts_key = kind.is_keyword()
                    || matches!(
                        kind,
                        TokenKind::Identifier
                            | TokenKind::String
                            | TokenKind::Number
                            | TokenKind::BigInt
                            | TokenKind::LBracket
                    )
                    || (kind == TokenKind::Star && key_token.name() == Some(names::ASYNC));
                let contextual = matches!(
                    key_token.name(),
                    Some(names::GET | names::SET | names::ASYNC)
                );
                if starts_key && key_token.escaped && contextual {
                    // `g\u0065t a() {}`: V8 reports the escaped keyword.
                    return Err(ParseError::syntax(start, messages::ESCAPED_KEYWORD));
                }
                Err(self.unexpected())
            }
        }
    }

    /// A shorthand property (`a`), or a `CoverInitializedName` (`a = 1`),
    /// which only a pattern allows (§13.2.5.1); the key token was `key`.
    fn parse_shorthand(
        &mut self,
        key_token: &crate::token::Token,
        key: PropertyKey,
        start: u32,
    ) -> PResult<Property> {
        // The key must be an IdentifierReference.
        if !matches!(
            key_token.kind,
            TokenKind::Identifier | TokenKind::Yield | TokenKind::Await
        ) {
            return Err(self.unexpected_key(key_token));
        }
        if key_token.escaped
            && key_token.name() == Some(names::AWAIT)
            && self.ctx.await_mode != AwaitMode::Identifier
        {
            // V8 words an escaped shorthand `await` like a plain one.
            return Err(ParseError::syntax(start, messages::UNEXPECTED_RESERVED));
        }
        let name = self.check_identifier_token(key_token, IdentUse::Reference)?;
        if name == names::AWAIT {
            self.ctx.last_await_name = Some(start);
        }
        let ident = self.reference(name, start, false);
        let mut value = self
            .ast
            .push_expr(ExprKind::Identifier(ident), Span::new(start, self.prev_end));
        if self.at(TokenKind::Eq) {
            self.note_cover_error(start, messages::INVALID_SHORTHAND_INITIALIZER);
            self.advance()?;
            let init = self.parse_assignment(false)?;
            value = self.push_expr(
                ExprKind::Assign {
                    op: AssignOp::Assign,
                    target: AssignTarget::Simple(value),
                    value: init,
                },
                start,
            );
        }
        Ok(self.property(PropertyKind::Shorthand, key, value, start))
    }

    fn property(
        &self,
        kind: PropertyKind,
        key: PropertyKey,
        value: ExprId,
        start: u32,
    ) -> Property {
        Property {
            kind,
            key,
            value,
            span: Span::new(start, self.prev_end),
        }
    }

    /// `PropertyName` (§13.2.5): an identifier name, a string, a number, a
    /// `BigInt` or a computed key.
    pub(super) fn parse_property_key(&mut self) -> PResult<PropertyKey> {
        let key = match self.token.kind {
            TokenKind::Identifier => PropertyKey::Name(self.token.name().unwrap_or(names::AWAIT)),
            TokenKind::String => {
                self.check_legacy(&self.token)?;
                let TokenValue::String(value) = &self.token.value else {
                    return Err(self.unexpected());
                };
                let value = value.clone();
                PropertyKey::Name(self.lexer.interner_mut().intern(value.as_str16()))
            }
            TokenKind::Number => {
                self.check_legacy(&self.token)?;
                match self.token.value {
                    TokenValue::Number(value) => PropertyKey::Number(value),
                    _ => return Err(self.unexpected()),
                }
            }
            TokenKind::BigInt => {
                let digits = match &self.token.value {
                    TokenValue::BigInt(text) => swb_js_text::String16::from(&**text),
                    _ => swb_js_text::String16::new(),
                };
                PropertyKey::BigInt(self.ast.push_string(digits))
            }
            TokenKind::LBracket => {
                self.advance()?;
                let expr = self.parse_assignment(false)?;
                if !self.at(TokenKind::RBracket) {
                    return Err(self.unexpected());
                }
                PropertyKey::Computed(expr)
            }
            kind => match kind.keyword_name() {
                Some(name) => PropertyKey::Name(name),
                None => return Err(self.unexpected()),
            },
        };
        self.advance()?;
        Ok(key)
    }
}
