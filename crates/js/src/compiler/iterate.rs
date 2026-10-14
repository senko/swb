//! Iteration in compiled code: `for`-`of` (§14.7.5) and spread in array
//! literals and argument lists (§13.2.4.1, §13.3.8.1).
//!
//! An iterator record lives in three registers (iterator, `next`, state;
//! see [`Insn::GetIterator`]). A step is `IteratorCall` and
//! `IteratorValue`: the call of `next` is an ordinary frame for script
//! code (so iterating a generator does not recurse in Rust), and the
//! built-in Array Iterator steps without a result object.
//!
//! `for`-`of` (ADR 0026 section 5): the binding and the body run in an
//! implicit `finally` block that closes the iterator on every abrupt
//! completion (`break` out of the loop, `continue` or `break` to an outer
//! label, `return`, a throw, and the return of a generator suspended in
//! the body, which takes the compiled return path). The step itself is
//! outside the block: a failure of the iterator does not close it. The
//! layout:
//!
//! ```text
//!       GetIterator rec <- right
//!   T:  IteratorCall v, rec
//!       IteratorValue v, rec -> X     (done: to X)
//!   S:  binding of v, body            <- the close's handler range
//!       Jump T                         (continue: to T)
//!   B:  Int kind = normal              (break: to B)
//!       IteratorClose rec, kind
//!       EndFinally kind value -> X     (routes of return and outer jumps)
//!       ...                            (the handler stub)
//!   X:
//! ```
//!
//! Spread does not close its iterator (§13.2.4.1 has no close): the loop
//! appends each value to the array of the literal, or of the argument
//! list of a call, which the call copies onto the value stack.

use swb_js_syntax::{AssignTarget, ExprId, ExprKind, ForHead, ScopeId, StmtId, StmtKind};
use swb_js_text::String16;

use super::CResult;
use super::function::{BreakableKind, FinallyBody, FunctionCompiler};
use super::pattern::{BindMode, PatternSource};
use crate::bytecode::{
    HINT_NO_TEXT, HINT_SHIFT, ITER_CALL, ITER_GENERIC, ITER_RENDERED, ITER_SPREAD, Insn, Reg,
};

/// The most links of a member chain that a "not iterable" message
/// renders.
const RENDER_LINKS: usize = 8;

/// The `hint` of `GetIterator` with the generic message.
pub(super) const GENERIC_HINT: u32 = (ITER_GENERIC << HINT_SHIFT) | HINT_NO_TEXT;

impl FunctionCompiler<'_> {
    /// `for (left of right) body` (§14.7.5.6 `ForIn/OfHeadEvaluation`,
    /// §14.7.5.7 `ForIn/OfBodyEvaluation`); see the module documentation.
    pub(super) fn for_of(
        &mut self,
        left: ForHead,
        right: ExprId,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let declaration = match left {
            ForHead::Declaration(stmt) => {
                let StmtKind::Variables { kind, declarators } = self.ast.stmt(stmt).kind else {
                    return Err(Self::internal("a for-of declaration without variables"));
                };
                let [declarator] = self.ast.declarators(declarators) else {
                    return Err(Self::internal("a for-of declaration with several bindings"));
                };
                Some((kind, declarator.target))
            }
            ForHead::Target(_) => None,
        };
        // A loop that does not run its body has the value `undefined`.
        if let Some(completion) = self.completion {
            self.emit(Insn::Undefined { dst: completion });
        }
        // The right side sees the head's bindings uninitialized.
        self.enter_scope(scope)?;
        let rec = self.temps(3)?;
        self.expr(right, Some(rec))?;
        self.position = self.ast.expr(right).span.start;
        let hint = self.iteration_hint(right, ITER_RENDERED, true);
        self.emit(Insn::GetIterator {
            dst: rec,
            src: rec,
            hint,
        });
        let value = self.temp()?;
        // Chromium reports the steps at the loop's target.
        self.position = match left {
            ForHead::Declaration(_) => declaration.map_or(self.position, |(_, target)| {
                self.ast.pattern(target).span.start
            }),
            ForHead::Target(AssignTarget::Simple(target)) => self.target_position(target),
            ForHead::Target(AssignTarget::Pattern(pattern)) => self.ast.pattern(pattern).span.start,
        };
        let top = self.here();
        self.emit(Insn::IteratorCall { dst: value, rec });
        let exit = self.emit(Insn::IteratorValue {
            dst: value,
            rec,
            offset: 0,
        });
        let start = self.push_close()?;
        self.push_breakable(labels, BreakableKind::Loop);
        match (declaration, left) {
            (Some((kind, target)), _) => self.bind_head_pattern(kind, target, value, scope)?,
            (None, ForHead::Target(AssignTarget::Simple(target))) => {
                self.assign_to_target(target, value)?;
            }
            (None, ForHead::Target(AssignTarget::Pattern(pattern))) => {
                self.bind_pattern(pattern, value, BindMode::Assign, PatternSource::Nested)?;
            }
            (None, ForHead::Declaration(_)) => {
                return Err(Self::internal("a for-of declaration without a binding"));
            }
        }
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("loop stack"))?;
        for at in breakable.continues {
            self.patch(at, top);
        }
        self.jump_back(top);
        // `finally_block` starts with the normal completion, which is
        // where `break` goes: it closes the iterator.
        let close = self.here();
        self.finally_block(start, FinallyBody::Close(rec))?;
        for at in breakable.breaks {
            self.patch(at, close);
        }
        self.patch_here(exit);
        // Release the iterator now, not when the register is reused.
        self.emit(Insn::Undefined { dst: rec });
        self.emit(Insn::Undefined { dst: rec + 1 });
        Ok(())
    }

    /// One element of an array literal or argument list into `array`: a
    /// hole, the values of a spread's iterator (`form` selects the
    /// message for a value that is not iterable, reported at `position`
    /// or else at the operand), or a value.
    pub(super) fn array_element(
        &mut self,
        array: Reg,
        item: ExprId,
        form: u32,
        position: Option<u32>,
    ) -> CResult<()> {
        let mark = self.mark();
        match self.ast.expr(item).kind {
            ExprKind::Hole => {
                self.emit(Insn::Hole { array });
            }
            ExprKind::Spread(operand) => {
                let rec = self.temps(3)?;
                self.expr(operand, Some(rec))?;
                self.position = position.unwrap_or(self.ast.expr(operand).span.start);
                let hint = self.iteration_hint(operand, form, true);
                self.emit(Insn::GetIterator {
                    dst: rec,
                    src: rec,
                    hint,
                });
                self.emit(Insn::SpreadArray { array, rec });
                self.position = self.ast.expr(operand).span.start;
                let v = self.temp()?;
                let top = self.here();
                self.emit(Insn::IteratorCall { dst: v, rec });
                let exit = self.emit(Insn::IteratorValue {
                    dst: v,
                    rec,
                    offset: 0,
                });
                self.emit(Insn::Push { array, src: v });
                self.jump_back(top);
                self.patch_here(exit);
                // Release the iterator now, not when the register is reused.
                self.emit(Insn::Undefined { dst: rec });
                self.emit(Insn::Undefined { dst: rec + 1 });
            }
            _ => {
                let v = self.expr(item, None)?;
                self.emit(Insn::Push { array, src: v });
            }
        }
        self.release(mark);
        Ok(())
    }

    /// The `hint` of `GetIterator` for the value of `e` in the message
    /// form `form`: V8 names a simple expression (an identifier, `this`,
    /// a literal; a member chain if `members`; a call by its callee),
    /// else gives the generic message.
    pub(super) fn iteration_hint(&mut self, e: ExprId, form: u32, members: bool) -> u32 {
        let e = self.unparen(e);
        let (form, text) = match self.ast.expr(e).kind {
            ExprKind::Call {
                callee,
                optional: false,
                ..
            } if form != ITER_SPREAD => (ITER_CALL, self.chain_text(callee, members)),
            _ => (form, self.chain_text(e, members)),
        };
        match text {
            Some(text) => self.text_hint(form, &text),
            None if form == ITER_SPREAD => (ITER_SPREAD << HINT_SHIFT) | HINT_NO_TEXT,
            None => GENERIC_HINT,
        }
    }

    /// A `hint` with a text.
    pub(super) fn text_hint(&mut self, form: u32, text: &str) -> u32 {
        let index = self.string_const(String16::from(text));
        if index >= HINT_NO_TEXT {
            return GENERIC_HINT;
        }
        (form << HINT_SHIFT) | index
    }

    /// The text of an identifier, `this`, a literal or (with `members`) a
    /// member chain of at most [`RENDER_LINKS`] links on one of them.
    fn chain_text(&self, e: ExprId, members: bool) -> Option<String> {
        let mut names = Vec::new();
        let mut current = self.unparen(e);
        while let ExprKind::Member {
            object,
            property,
            optional: false,
        } = self.ast.expr(current).kind
        {
            if !members || names.len() == RENDER_LINKS {
                return None;
            }
            names.push(self.name_text(property).to_string_lossy());
            current = self.unparen(object);
        }
        let base = match self.ast.expr(current).kind {
            ExprKind::Identifier(_)
            | ExprKind::This(_)
            | ExprKind::Null
            | ExprKind::Boolean(_)
            | ExprKind::Number(_)
            | ExprKind::String(_) => self.render(current, 1)?,
            _ => return None,
        };
        let mut text = base;
        for name in names.iter().rev() {
            text.push('.');
            text.push_str(name);
        }
        Some(text)
    }
}
