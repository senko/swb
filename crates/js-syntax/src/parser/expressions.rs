//! Expressions (ECMA-262 clause 13): precedence climbing for the binary
//! operators, recursive descent for the rest.

use swb_js_text::CodeUnit;

use super::{CHAIN_WEIGHT, IdentUse, PResult, Parser};
use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, ExprId, ExprKind, List, LogicalOp, Property, PropertyKey,
    PropertyKind, Span, Template, TemplateElement, UnaryOp, UpdateOp,
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
fn starts_expression(kind: TokenKind) -> bool {
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

impl<U: CodeUnit> Parser<'_, '_, U> {
    fn push_expr(&mut self, kind: ExprKind, start: u32) -> ExprId {
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
        if !self.at(TokenKind::Comma) {
            return Ok(first);
        }
        let mark = self.scratch_exprs.len();
        self.scratch_exprs.push(first);
        while self.eat(TokenKind::Comma)? {
            let next = self.parse_assignment(no_in)?;
            self.scratch_exprs.push(next);
        }
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        Ok(self.push_expr(ExprKind::Sequence(list), start))
    }

    /// `AssignmentExpression` (§13.15), including arrow functions and
    /// `yield`.
    pub(super) fn parse_assignment(&mut self, no_in: bool) -> PResult<ExprId> {
        self.enter()?;
        let expr = self.parse_assignment_inner(no_in);
        self.leave();
        expr
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
            if arrow.kind != TokenKind::Arrow || arrow.newline_before {
                return Err(self.unexpected());
            }
            self.advance()?;
            return self.parse_arrow_function(start, &[], None, no_in);
        }
        let marks = self.scopes.marks();
        let functions = self.ast.function_count();
        let left = self.parse_conditional(no_in)?;
        if self.at(TokenKind::Arrow) {
            if self.token.newline_before {
                return Err(self.unexpected());
            }
            return self.parse_arrow_from_cover(left, start, marks, functions, no_in);
        }
        let Some(op) = assignment_operator(self.token.kind) else {
            return Ok(left);
        };
        let target = self.assignment_target(left, op)?;
        self.advance()?;
        let value = self.parse_assignment(no_in)?;
        Ok(self.push_expr(ExprKind::Assign { op, target, value }, start))
    }

    /// The target of an assignment with `op` (§13.15.1).
    fn assignment_target(&mut self, left: ExprId, op: AssignOp) -> PResult<AssignTarget> {
        let start = self.ast.expr(left).span.start;
        let target = self.unparenthesized(left);
        match self.kind_of(target) {
            ExprKind::Identifier(ident) => {
                self.check_strict_target(ident.name, start)?;
                Ok(AssignTarget::Simple(target))
            }
            ExprKind::Member { .. } | ExprKind::Index { .. } => Ok(AssignTarget::Simple(target)),
            // Chromium accepts `f() = x` and throws a ReferenceError at run
            // time (web compatibility); later editions of ECMA-262 allow
            // it in sloppy mode code, except for the logical operators.
            ExprKind::Call { .. } if !self.ctx.strict && !matches!(op, AssignOp::Logical(_)) => {
                Ok(AssignTarget::Simple(target))
            }
            ExprKind::Object(_) | ExprKind::Array(_)
                if target == left && op == AssignOp::Assign =>
            {
                Err(ParseError::unsupported(start, "destructuring assignment"))
            }
            _ => Err(ParseError::syntax(
                start,
                messages::INVALID_ASSIGNMENT_TARGET,
            )),
        }
    }

    /// Strict mode code cannot assign to `eval` or `arguments` (§13.1.1).
    fn check_strict_target(&self, name: crate::NameId, offset: u32) -> PResult<()> {
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
            ExprKind::Member { .. } | ExprKind::Index { .. } => Ok(target),
            ExprKind::Call { .. } if !self.ctx.strict => Ok(target),
            _ => Err(ParseError::syntax(start, message)),
        }
    }

    /// `YieldExpression` (§15.5): `yield` with an optional operand on the
    /// same line.
    fn parse_yield(&mut self, no_in: bool) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        if self.token.newline_before {
            return Ok(self.push_expr(ExprKind::Yield { argument: None }, start));
        }
        if self.at(TokenKind::Star) {
            return Err(ParseError::unsupported(start, "yield*"));
        }
        let argument = if starts_expression(self.token.kind) {
            Some(self.parse_assignment(no_in)?)
        } else {
            None
        };
        Ok(self.push_expr(ExprKind::Yield { argument }, start))
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
        let mut left = self.parse_unary()?;
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
            (Operator::Binary(BinaryOp::Exponent), ExprKind::Unary { .. }) => ParseError::syntax(
                self.ast.expr(left).span.start,
                messages::UNARY_BEFORE_EXPONENT,
            ),
            _ => return Ok(()),
        };
        Err(error)
    }

    /// `UnaryExpression` and prefix `UpdateExpression` (§13.4, §13.5).
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
            _ => return self.parse_postfix(),
        };
        self.advance()?;
        self.enter_small()?;
        let argument = self.parse_unary();
        self.leave_small();
        let argument = argument?;
        if op == UnaryOp::Delete && self.ctx.strict {
            let operand = self.unparenthesized(argument);
            if matches!(self.kind_of(operand), ExprKind::Identifier(_)) {
                return Err(ParseError::syntax(
                    self.ast.expr(operand).span.start,
                    messages::DELETE_IDENTIFIER,
                ));
            }
        }
        Ok(self.push_expr(ExprKind::Unary { op, argument }, start))
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
    fn parse_left_hand_side(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let object = if self.at(TokenKind::New) {
            self.parse_new()?
        } else {
            self.parse_primary()?
        };
        self.parse_chain(object, start, true)
    }

    /// The member accesses (and with `calls`, the calls) after `object`.
    /// Each link charges the budget: the chain makes the tree deeper.
    fn parse_chain(&mut self, mut object: ExprId, start: u32, calls: bool) -> PResult<ExprId> {
        let mut links = 0u32;
        let result = loop {
            let link = match self.token.kind {
                TokenKind::Dot | TokenKind::LBracket => self.parse_member(object, start),
                TokenKind::LParen if calls => self.parse_call(object, start),
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                    Err(ParseError::unsupported(self.token.start, "tagged template"))
                }
                TokenKind::QuestionDot => Err(ParseError::unsupported(
                    self.token.start,
                    "optional chaining",
                )),
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
        result
    }

    /// `.name` or `[expression]` after `object`.
    fn parse_member(&mut self, object: ExprId, start: u32) -> PResult<ExprId> {
        if self.eat(TokenKind::Dot)? {
            let property = match self.token.kind {
                TokenKind::Identifier => self.token.name().unwrap_or(names::AWAIT),
                TokenKind::PrivateName => {
                    return Err(ParseError::unsupported(self.token.start, "private name"));
                }
                kind => match kind.keyword_name() {
                    Some(name) => name,
                    None => return Err(self.unexpected()),
                },
            };
            self.advance()?;
            return Ok(self.push_expr(ExprKind::Member { object, property }, start));
        }
        self.expect(TokenKind::LBracket)?;
        let index = self.parse_expression(false)?;
        self.expect(TokenKind::RBracket)?;
        Ok(self.push_expr(ExprKind::Index { object, index }, start))
    }

    /// A call with its arguments. A call of the plain name `eval` (also
    /// in parentheses) is a direct `eval` (§13.3.6.1).
    fn parse_call(&mut self, callee: ExprId, start: u32) -> PResult<ExprId> {
        let arguments = self.parse_arguments()?;
        let target = self.unparenthesized(callee);
        if let ExprKind::Identifier(ident) = self.kind_of(target)
            && ident.name == names::EVAL
            && let Some(function) = self.scopes.function_mut(self.ctx.function)
        {
            function.has_direct_eval = true;
        }
        Ok(self.push_expr(ExprKind::Call { callee, arguments }, start))
    }

    /// Arguments (§13.3.8): `(a, b,)`.
    fn parse_arguments(&mut self) -> PResult<List<ExprId>> {
        let open = self.token.start;
        self.expect(TokenKind::LParen)?;
        let mark = self.scratch_exprs.len();
        while !self.at(TokenKind::RParen) {
            if self.at(TokenKind::Ellipsis) {
                return Err(ParseError::unsupported(self.token.start, "spread"));
            }
            let argument = self.parse_assignment(false)?;
            self.scratch_exprs.push(argument);
            if self.at(TokenKind::RParen) {
                break;
            }
            if !self.eat(TokenKind::Comma)? {
                return Err(ParseError::syntax(
                    open,
                    messages::MISSING_PAREN_AFTER_ARGUMENTS,
                ));
            }
        }
        self.advance()?;
        if self.scratch_exprs.len() - mark > usize::from(u16::MAX) {
            return Err(ParseError::syntax(open, messages::TOO_MANY_ARGUMENTS));
        }
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        Ok(list)
    }

    /// `new` `MemberExpression` Arguments, or `new` `NewExpression` (§13.3.5).
    fn parse_new(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        if self.at(TokenKind::Dot) {
            return Err(ParseError::unsupported(start, "new.target"));
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

    /// The `MemberExpression` after `new`: no calls.
    fn parse_new_callee(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let callee = if self.at(TokenKind::New) {
            self.parse_new()?
        } else {
            self.parse_primary()?
        };
        self.parse_chain(callee, start, false)
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
                return self.parse_template();
            }
            TokenKind::Slash | TokenKind::SlashEq => {
                self.rescan(Goal::RegExp)?;
                return self.parse_regexp();
            }
            TokenKind::RegExp => return self.parse_regexp(),
            TokenKind::LParen => return self.parse_parenthesized(),
            TokenKind::LBracket => return self.parse_array(),
            TokenKind::LBrace => return self.parse_object(),
            TokenKind::Function => return self.parse_function_expression(),
            TokenKind::Class => return Err(ParseError::unsupported(start, "class")),
            TokenKind::Super => return Err(ParseError::unsupported(start, "super")),
            TokenKind::Import => return Err(ParseError::unsupported(start, "import")),
            TokenKind::BigInt => return Err(ParseError::unsupported(start, "BigInt literal")),
            TokenKind::PrivateName => return Err(ParseError::unsupported(start, "private name")),
            _ => return Err(self.unexpected()),
        };
        self.advance()?;
        Ok(self.push_expr(kind, start))
    }

    /// `IdentifierReference` (§13.1).
    fn parse_identifier_reference(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        if self.at_contextual(names::ASYNC) && !self.peek()?.newline_before {
            match self.peek_kind()? {
                TokenKind::Function => {
                    return Err(ParseError::unsupported(start, "async function"));
                }
                TokenKind::Identifier => {
                    return Err(ParseError::unsupported(start, "async arrow function"));
                }
                _ => {}
            }
        }
        let name = self.check_identifier(IdentUse::Reference)?;
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

    /// A template literal without a tag (§13.2.8).
    fn parse_template(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        let quasi_mark = self.scratch_quasis.len();
        let expr_mark = self.scratch_exprs.len();
        loop {
            let element = self.template_element()?;
            self.scratch_quasis.push(element);
            let tail = matches!(
                self.token.kind,
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateTail
            );
            self.advance()?;
            if tail {
                break;
            }
            let expression = self.parse_expression(false)?;
            self.scratch_exprs.push(expression);
            if !self.at(TokenKind::RBrace) {
                return Err(ParseError::syntax(start, messages::MISSING_TEMPLATE_BRACE));
            }
            self.rescan(Goal::TemplateTail)?;
        }
        let quasis = self
            .ast
            .push_template_elements(&self.scratch_quasis[quasi_mark..]);
        let expressions = self.ast.push_exprs(&self.scratch_exprs[expr_mark..]);
        self.scratch_quasis.truncate(quasi_mark);
        self.scratch_exprs.truncate(expr_mark);
        let template = self.ast.push_template(Template {
            quasis,
            expressions,
        });
        Ok(self.push_expr(ExprKind::Template(template), start))
    }

    /// The string part of the current template token. Without a tag, an
    /// invalid escape is a syntax error (§13.2.8.1).
    fn template_element(&mut self) -> PResult<TemplateElement> {
        let TokenValue::Template(template) = &self.token.value else {
            return Err(self.unexpected());
        };
        let cooked = template.cooked.clone()?;
        let raw = template.raw.clone();
        Ok(TemplateElement {
            cooked: Some(self.ast.push_string(cooked)),
            raw: self.ast.push_string(raw),
        })
    }

    /// `CoverParenthesizedExpressionAndArrowParameterList` (§13.2): a
    /// parenthesized expression, or the parameters of an arrow function,
    /// which [`Parser::parse_arrow_from_cover`] converts at `=>`.
    fn parse_parenthesized(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let inner_start = self.token.start;
        if self.at(TokenKind::Ellipsis) {
            return Err(ParseError::unsupported(inner_start, "rest parameter"));
        }
        let first = self.parse_assignment(false)?;
        let inner = if self.at(TokenKind::Comma) {
            let mark = self.scratch_exprs.len();
            self.scratch_exprs.push(first);
            while self.eat(TokenKind::Comma)? {
                if self.at(TokenKind::RParen) {
                    // A trailing comma: only arrow parameters allow it.
                    if self.peek_kind()? != TokenKind::Arrow {
                        return Err(self.unexpected());
                    }
                    break;
                }
                if self.at(TokenKind::Ellipsis) {
                    return Err(ParseError::unsupported(self.token.start, "rest parameter"));
                }
                let next = self.parse_assignment(false)?;
                self.scratch_exprs.push(next);
            }
            let items = &self.scratch_exprs[mark..];
            let inner = if let [only] = items {
                *only
            } else {
                let list = self.ast.push_exprs(items);
                self.push_expr(ExprKind::Sequence(list), inner_start)
            };
            self.scratch_exprs.truncate(mark);
            inner
        } else {
            first
        };
        self.expect(TokenKind::RParen)?;
        Ok(self.push_expr(ExprKind::Paren(inner), start))
    }

    /// An array literal (§13.2.4) with holes.
    fn parse_array(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_exprs.len();
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
            if self.at(TokenKind::Ellipsis) {
                return Err(ParseError::unsupported(self.token.start, "spread"));
            }
            let element = self.parse_assignment(false)?;
            self.scratch_exprs.push(element);
            if !self.at(TokenKind::RBracket) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        let list = self.ast.push_exprs(&self.scratch_exprs[mark..]);
        self.scratch_exprs.truncate(mark);
        Ok(self.push_expr(ExprKind::Array(list), start))
    }

    /// An object literal (§13.2.5).
    fn parse_object(&mut self) -> PResult<ExprId> {
        let start = self.token.start;
        self.advance()?;
        let mark = self.scratch_properties.len();
        let mut has_proto = false;
        while !self.at(TokenKind::RBrace) {
            let property = self.parse_property()?;
            if property.kind == PropertyKind::Proto {
                if has_proto {
                    return Err(ParseError::syntax(
                        property.span.start,
                        messages::DUPLICATE_PROTO,
                    ));
                }
                has_proto = true;
            }
            self.scratch_properties.push(property);
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        let list = self.ast.push_properties(&self.scratch_properties[mark..]);
        self.scratch_properties.truncate(mark);
        Ok(self.push_expr(ExprKind::Object(list), start))
    }

    /// A `PropertyDefinition` (§13.2.5).
    fn parse_property(&mut self) -> PResult<Property> {
        let start = self.token.start;
        if self.at(TokenKind::Star) {
            self.advance()?;
            let key = self.parse_property_key()?;
            let value = self.parse_method(start, true)?;
            return Ok(self.property(PropertyKind::Method, key, value, start));
        }
        if self.at(TokenKind::Ellipsis) {
            return Err(ParseError::unsupported(start, "spread property"));
        }
        let key_token = self.token.clone();
        if self.token.kind == TokenKind::Identifier && !self.token.escaped {
            let name = self.token.name();
            let accessor = name == Some(names::GET) || name == Some(names::SET);
            if (accessor || name == Some(names::ASYNC))
                && !matches!(
                    self.peek_kind()?,
                    TokenKind::Comma
                        | TokenKind::Colon
                        | TokenKind::LParen
                        | TokenKind::RBrace
                        | TokenKind::Eq
                )
            {
                let construct = if accessor {
                    "getter or setter"
                } else {
                    "async method"
                };
                return Err(ParseError::unsupported(start, construct));
            }
        }
        let key = self.parse_property_key()?;
        match self.token.kind {
            TokenKind::Colon => {
                self.advance()?;
                let value = self.parse_assignment(false)?;
                let kind = if key == PropertyKey::Name(names::PROTO) {
                    PropertyKind::Proto
                } else {
                    PropertyKind::Init
                };
                Ok(self.property(kind, key, value, start))
            }
            TokenKind::LParen => {
                let value = self.parse_method(start, false)?;
                Ok(self.property(PropertyKind::Method, key, value, start))
            }
            TokenKind::Eq => Err(ParseError::unsupported(start, "destructuring assignment")),
            TokenKind::Comma | TokenKind::RBrace => {
                // Shorthand: the key must be an IdentifierReference.
                if !matches!(
                    key_token.kind,
                    TokenKind::Identifier | TokenKind::Yield | TokenKind::Await
                ) {
                    return Err(self.unexpected_token(&key_token));
                }
                let name = self.check_identifier_token(&key_token, IdentUse::Reference)?;
                let ident = self.reference(name, start, false);
                let value = self
                    .ast
                    .push_expr(ExprKind::Identifier(ident), Span::new(start, self.prev_end));
                Ok(self.property(PropertyKind::Shorthand, key, value, start))
            }
            _ => Err(self.unexpected()),
        }
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

    /// `PropertyName` (§13.2.5): an identifier name, a string, a number or
    /// a computed key.
    fn parse_property_key(&mut self) -> PResult<PropertyKey> {
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
                return Err(ParseError::unsupported(self.token.start, "BigInt literal"));
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
