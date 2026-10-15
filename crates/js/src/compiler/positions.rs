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
//! - Destructuring: a value that an object pattern cannot destructure is
//!   reported at the target of the first property (at the pattern if the
//!   first key is computed or there is none); a value without an
//!   iterator for an array pattern at the initializer, or at the `=` of
//!   an assignment. The steps of `for`-`of` are at the loop's target; a
//!   spread argument without an iterator is at the call.
//! - Inside an optional chain, a property read `a?.b.c` is at the `.`
//!   before the name; a tagged template call is at its `` ` ``.
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
            ExprKind::Member { property, .. }
            | ExprKind::SuperMember { property, .. }
            | ExprKind::PrivateMember {
                name: swb_js_syntax::Ident { name: property, .. },
                ..
            } => self
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

    /// The position of the `.` of `a.b` (the first `.` after the object).
    pub(super) fn dot_position(&self, member: ExprId) -> u32 {
        let node = self.ast.expr(member);
        let (ExprKind::Member { object, .. } | ExprKind::PrivateMember { object, .. }) = node.kind
        else {
            return node.span.start;
        };
        self.find_token(self.ast.expr(object).span.end, b'.')
            .unwrap_or(node.span.start)
    }

    /// The position of an access to a private member `a.#x` other than a
    /// plain read: Chromium reports a field at the `.` (a compound
    /// assignment, an update, a destructuring or `for`-`of` target) and a
    /// method or accessor at the name.
    pub(super) fn private_position(&self, member: ExprId) -> u32 {
        let ExprKind::PrivateMember { name, .. } = self.ast.expr(member).kind else {
            return self.ast.expr(member).span.start;
        };
        let field = self
            .scopes
            .private_name(name.reference)
            .and_then(|binding| self.session.private_kinds.get(&binding))
            .is_none_or(|&kind| kind == crate::bytecode::PrivateKind::Field);
        if field {
            self.dot_position(member)
        } else {
            self.name_position(member)
        }
    }

    /// The position of a plain read of a private member: the name, or the
    /// `?.` of an optional link (Chromium's).
    pub(super) fn private_read_position(&self, member: ExprId) -> u32 {
        match self.ast.expr(member).kind {
            ExprKind::PrivateMember {
                object,
                optional: true,
                ..
            } => self
                .find_token(self.ast.expr(object).span.end, b'?')
                .unwrap_or(self.ast.expr(member).span.start),
            _ => self.read_position(member),
        }
    }

    /// The position of the `` ` `` that starts the template of a tagged
    /// template (Chromium's position of the call).
    pub(super) fn template_position(&self, tag: ExprId, fallback: u32) -> u32 {
        self.find_token(self.ast.expr(tag).span.end, b'`')
            .unwrap_or(fallback)
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

    /// The position of the first token at or after `offset`.
    pub(super) fn token_position(&self, offset: u32) -> u32 {
        self.skip_trivia(offset).unwrap_or(offset)
    }

    /// The position of a reference for the steps of a `for`-`of` loop
    /// (Chromium reports them at the loop's target): the name of `a.b`,
    /// else the start.
    pub(super) fn target_position(&self, target: ExprId) -> u32 {
        let target = self.unparen(target);
        match self.ast.expr(target).kind {
            ExprKind::Member { .. } => self.name_position(target),
            ExprKind::PrivateMember { .. } => self.private_position(target),
            _ => self.ast.expr(target).span.start,
        }
    }

    /// The `(` of a class constructor's parameter list, where V8 places
    /// the code that its function runs before the body (the instance
    /// fields): the first `(` after the start of the key `constructor`.
    pub(super) fn parameters_position(&self, start: u32) -> u32 {
        (start..start.saturating_add(64))
            .find(|&at| self.text.get(at as usize) == Some(u16::from(b'(')))
            .unwrap_or(start)
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
