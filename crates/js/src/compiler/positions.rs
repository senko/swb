//! The source positions of instructions for stack traces: the positions
//! that Chromium reports (measured in Node.js 22), not the start of
//! each expression.
//!
//! - A property read `a.b` is at the start of the name `b`; `a[k]` is at
//!   the `[`.
//! - A call is at the last token of the callee if that is an identifier
//!   (`f()`, `a.b()`: the name), else at the `(` (`a[k]()`, `(f)()`,
//!   `f()()`). A `new` expression is at the `new`.
//! - A write to a property (`a.b = v`, `a.b += v`) is at the operator.
//!
//! The AST has the span of each expression but not of the tokens inside
//! it, so the `[`, the `(` and the operator are found by a scan over the
//! source text that skips white space, comments, `)` and `?.`.

use swb_js_syntax::{ExprId, ExprKind};

use super::function::FunctionCompiler;

impl FunctionCompiler<'_> {
    /// The position of the name of `a.b` (the end of the expression minus
    /// the length of the name; an escape in the name makes it a few units
    /// off).
    pub(super) fn name_position(&self, member: ExprId) -> u32 {
        let node = self.ast.expr(member);
        let length = match node.kind {
            ExprKind::Member { property, .. } | ExprKind::SuperMember { property, .. } => self
                .script
                .names
                .get(property)
                .map_or(0, swb_js_text::Str16::len),
            _ => 0,
        };
        node.span
            .end
            .saturating_sub(length as u32)
            .max(node.span.start)
    }

    /// The position of the `[` of `a[k]`: the first `[` after the object.
    pub(super) fn bracket_position(&self, index_expr: ExprId) -> u32 {
        let node = self.ast.expr(index_expr);
        let ExprKind::Index { object, .. } = node.kind else {
            return node.span.start;
        };
        self.find_token(self.ast.expr(object).span.end, b'[')
            .unwrap_or(node.span.start)
    }

    /// The position that Chromium reports for a call whose callee is
    /// `callee`.
    pub(super) fn callee_position(&self, callee: ExprId, fallback: u32) -> u32 {
        let node = self.ast.expr(callee);
        match node.kind {
            ExprKind::Identifier(_) => node.span.start,
            ExprKind::Member { .. } | ExprKind::SuperMember { .. } => self.name_position(callee),
            _ => self.find_token(node.span.end, b'(').unwrap_or(fallback),
        }
    }

    /// The position of the operator of an assignment: the first token
    /// after the target.
    pub(super) fn operator_position(&self, target: ExprId, fallback: u32) -> u32 {
        let end = self.ast.expr(target).span.end;
        self.skip_trivia(end).unwrap_or(fallback)
    }

    /// The offset of the first `wanted` after `from`, skipping white space,
    /// comments, `)` and `?.`; `None` if something else comes first.
    fn find_token(&self, from: u32, wanted: u8) -> Option<u32> {
        let mut at = from;
        loop {
            at = self.skip_trivia(at)?;
            let unit = self.text.get(at as usize)?;
            if unit == u16::from(wanted) {
                return Some(at);
            }
            match u8::try_from(unit).ok()? {
                b')' | b'?' | b'.' => at += 1,
                _ => return None,
            }
        }
    }

    /// The offset of the first unit after `from` that is not white space
    /// or part of a comment (`None` at the end of the text).
    fn skip_trivia(&self, from: u32) -> Option<u32> {
        let mut at = from as usize;
        loop {
            let unit = self.text.get(at)?;
            let next = self.text.get(at + 1);
            if unit == u16::from(b'/') && next == Some(u16::from(b'*')) {
                at += 2;
                while !(self.text.get(at)? == u16::from(b'*')
                    && self.text.get(at + 1) == Some(u16::from(b'/')))
                {
                    at += 1;
                }
                at += 2;
            } else if unit == u16::from(b'/') && next == Some(u16::from(b'/')) {
                while !matches!(self.text.get(at)?, 0x0A | 0x0D | 0x2028 | 0x2029) {
                    at += 1;
                }
            } else if matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x2028 | 0x2029 | 0xFEFF) {
                at += 1;
            } else {
                return u32::try_from(at).ok();
            }
        }
    }
}
