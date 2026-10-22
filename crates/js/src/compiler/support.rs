//! The constructs that the parser accepts but the compiler does not
//! compile: a linear scan over the node tables of a script before any
//! function compiles. The script fails with a `SyntaxError` "not
//! supported yet (construct)" at the first such construct in the source;
//! the compile functions never see them. These are module code,
//! `import()` and `import.meta`.

use swb_js_syntax::{ExprKind, Script, StmtKind};

/// The construct of an expression that the compiler does not support.
pub(super) fn unsupported_expr(kind: ExprKind) -> Option<&'static str> {
    Some(match kind {
        ExprKind::ImportCall { .. } => "import()",
        ExprKind::ImportMeta => "import.meta",
        _ => return None,
    })
}

/// The construct of a statement that the compiler does not support.
pub(super) fn unsupported_stmt(kind: StmtKind) -> Option<&'static str> {
    Some(match kind {
        StmtKind::Import
        | StmtKind::ExportDeclaration(_)
        | StmtKind::ExportDefault { .. }
        | StmtKind::ExportList => "module syntax",
        _ => return None,
    })
}

/// The first construct of the script (by source offset) that the
/// compiler does not support, with its offset.
pub(super) fn first_unsupported(script: &Script) -> Option<(u32, &'static str)> {
    if script.module.is_some() {
        return Some((0, "module"));
    }
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
    for id in ast.stmt_ids() {
        let stmt = ast.stmt(id);
        if let Some(construct) = unsupported_stmt(stmt.kind) {
            note(stmt.span.start, construct);
        }
    }
    first
}
