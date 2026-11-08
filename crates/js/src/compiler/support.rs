//! The constructs that the parser accepts but that the compiler refuses
//! in a runtime without module hooks: a linear scan over the node tables
//! of a script before any function compiles. The script fails with a
//! `SyntaxError` "not supported yet (construct)" at the first such
//! construct in the source. These are `import()` and `import.meta` (ADR
//! 0034 sub-feature 3a-3): they need the host's loader and meta hook
//! (`Runtime::set_module_hooks`), and the web layer installs none before
//! sub-feature 3b-1, so its scripts keep the refusal.

use swb_js_syntax::{ExprKind, Script};

/// The construct of an expression that needs module hooks.
fn unsupported_expr(kind: ExprKind) -> Option<&'static str> {
    Some(match kind {
        ExprKind::ImportCall { .. } => "import()",
        ExprKind::ImportMeta => "import.meta",
        _ => return None,
    })
}

/// The first construct of the script (by source offset) that needs module
/// hooks, with its offset.
pub(super) fn first_unsupported(script: &Script) -> Option<(u32, &'static str)> {
    let ast = &script.ast;
    let mut first: Option<(u32, &'static str)> = None;
    let mut note = |offset: u32, construct: &'static str| {
        if first.is_none_or(|(at, _)| offset < at) {
            first = Some((offset, construct));
        }
    };
    for id in ast.expr_ids() {
        let expr = ast.expr(id);
        if let Some(construct) = unsupported_expr(expr.kind) {
            note(expr.span.start, construct);
        }
    }
    first
}
