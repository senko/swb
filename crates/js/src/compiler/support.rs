//! The constructs that the parser accepts but the compiler does not
//! compile yet (M7 feature 3): a linear scan over the node tables of a
//! script before any function compiles. The script fails with a
//! `SyntaxError` "not supported yet (construct)" at the first such
//! construct in the source; the compile functions never see them.

use swb_js_syntax::{
    AssignTarget, ExprKind, PatternKind, PropertyKey, PropertyKind, Script, StmtKind,
};

/// The construct of an expression that the compiler does not support.
pub(super) fn unsupported_expr(kind: ExprKind) -> Option<&'static str> {
    Some(match kind {
        ExprKind::Spread(_) => "spread",
        ExprKind::OptionalChain(_) => "optional chaining",
        ExprKind::TaggedTemplate { .. } => "tagged template",
        ExprKind::NewTarget(_) => "new.target",
        ExprKind::BigInt(_) => "BigInt literal",
        ExprKind::Yield { delegate: true, .. } => "yield*",
        ExprKind::Assign {
            target: AssignTarget::Pattern(_),
            ..
        } => "destructuring assignment",
        _ => return None,
    })
}

/// The construct of a statement that the compiler does not support.
fn unsupported_stmt(kind: StmtKind) -> Option<&'static str> {
    Some(match kind {
        StmtKind::ForIn { .. } => "for-in",
        StmtKind::ForOf { .. } => "for-of",
        StmtKind::With { .. } => "with",
        _ => return None,
    })
}

/// The construct of a pattern other than a binding identifier.
fn unsupported_pattern(kind: PatternKind) -> Option<&'static str> {
    Some(match kind {
        PatternKind::Identifier(_) => return None,
        PatternKind::Default { .. } => "default value",
        PatternKind::Rest(_) => "rest element",
        PatternKind::Expr(_)
        | PatternKind::Array(_)
        | PatternKind::Object { .. }
        | PatternKind::Hole => "destructuring",
    })
}

/// The first construct of the script (by source offset) that the
/// compiler does not support, with its offset.
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
        if let ExprKind::Object(properties) = expr.kind {
            for property in ast.properties(properties) {
                if property.kind == PropertyKind::Spread {
                    note(property.span.start, "object spread");
                }
                if let PropertyKey::BigInt(_) = property.key {
                    note(property.span.start, "BigInt literal");
                }
            }
        }
    }
    for id in ast.stmt_ids() {
        let stmt = ast.stmt(id);
        if let Some(construct) = unsupported_stmt(stmt.kind) {
            note(stmt.span.start, construct);
        }
    }
    for id in ast.pattern_ids() {
        let pattern = ast.pattern(id);
        if let Some(construct) = unsupported_pattern(pattern.kind) {
            note(pattern.span.start, construct);
        }
    }
    first
}
