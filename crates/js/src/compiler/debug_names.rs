//! The names of anonymous functions in stack traces. Chromium infers a
//! name for a function expression that is assigned to a member
//! (`a.b.c = function () {}` is `a.b.c`, `A.prototype.m = ...` is `A.m`,
//! `this.x = ...` is `x`) or that is an element of an array literal that
//! initializes a variable (`var f = [function () {}]` is `f`). These
//! names are for stack traces only: the `name` property of the function
//! stays empty (§8.4.5 `NamedEvaluation` does not name them).

use swb_js_syntax::{ExprId, ExprKind};
use swb_js_text::String16;

use super::function::FunctionCompiler;

/// The most links of a member path that the inference follows.
const PATH_LIMIT: usize = 32;

impl FunctionCompiler<'_> {
    /// Records the stack trace name of `value` if it is an anonymous
    /// function expression assigned to `target`.
    pub(super) fn note_member_function(&mut self, target: ExprId, value: ExprId) {
        let inner = self.unparen(value);
        let ExprKind::Function(function) = self.ast.expr(inner).kind else {
            return;
        };
        if self.ast.function(function).name.is_some() {
            return;
        }
        if let Some(path) = self.member_path(target) {
            self.session.debug_names.insert(function, path);
        }
    }

    /// Records `name` for the anonymous function expressions that are
    /// elements of the array literal `array`.
    pub(super) fn note_array_functions(&mut self, array: ExprId, name: &String16) {
        let ExprKind::Array(list) = self.ast.expr(array).kind else {
            return;
        };
        for &element in self.ast.exprs(list) {
            let inner = self.unparen(element);
            if let ExprKind::Function(function) = self.ast.expr(inner).kind
                && self.ast.function(function).name.is_none()
            {
                self.session.debug_names.insert(function, name.clone());
            }
        }
    }

    /// The dotted path of a member expression whose base is an
    /// identifier or `this`; `None` for other bases.
    fn member_path(&self, target: ExprId) -> Option<String16> {
        let mut parts: Vec<String16> = Vec::new();
        let mut current = target;
        for _ in 0..PATH_LIMIT {
            match self.ast.expr(current).kind {
                ExprKind::Member {
                    object, property, ..
                } => {
                    parts.push(self.name_text(property));
                    current = object;
                }
                ExprKind::Index { object, index, .. } => {
                    parts.push(
                        self.constant_key(index)
                            .unwrap_or_else(|| String16::from("<computed>")),
                    );
                    current = object;
                }
                ExprKind::Identifier(ident) => {
                    parts.push(self.name_text(ident.name));
                    break;
                }
                ExprKind::This(_) => break,
                _ => return None,
            }
        }
        parts.reverse();
        let mut path = String16::default();
        let last = parts.len().saturating_sub(1);
        for (i, part) in parts.iter().enumerate() {
            if i != last && part.as_str16().eq_str("prototype") {
                continue;
            }
            if !path.is_empty() {
                path.push(u16::from(b'.'));
            }
            path.push_str16(part.as_str16());
        }
        Some(path)
    }
}
