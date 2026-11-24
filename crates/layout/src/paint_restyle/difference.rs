//! The check-mode compare of two fragment trees
//! ([`FragmentTree::first_difference`]): the first difference of the
//! refreshed tree and a full layout, as a path of child indices and what
//! differs (ADR 0044 section 9).

use std::sync::Arc;

use swb_style::ComputedStyle;

use crate::fragment::{BoxFragment, Fragment, FragmentTree, TextFragment};
use crate::geom::Rect;

impl FragmentTree {
    /// The first difference between this tree and `other`, as a path of
    /// child indices and what differs; `None` if they are equal. The
    /// check mode of the engine compares the refreshed tree with a full
    /// layout. Scrollable overflow compares within 0.01 px: the refresh
    /// adds up the extents again.
    pub fn first_difference(&self, other: &FragmentTree) -> Option<String> {
        if self.scroll_size != other.scroll_size {
            return Some(format!(
                "scroll size {:?} against {:?}",
                self.scroll_size, other.scroll_size
            ));
        }
        if self.viewport_overflow != other.viewport_overflow
            || self.viewport_scrollbar_hidden != other.viewport_scrollbar_hidden
            || self.viewport != other.viewport
        {
            return Some("the viewport state".to_owned());
        }
        match (&self.canvas_background, &other.canvas_background) {
            (None, None) => {}
            (Some(a), Some(b)) if a.source == b.source => {
                if let Some(d) = style_difference(&a.style, &b.style) {
                    return Some(format!("canvas background style: {d}"));
                }
            }
            _ => return Some("the canvas background source".to_owned()),
        }
        let (a, b): (Vec<_>, Vec<_>) = (self.roots().collect(), other.roots().collect());
        if a.len() != b.len() {
            return Some(format!("{} roots against {}", a.len(), b.len()));
        }
        for (i, (x, y)) in a.into_iter().zip(b).enumerate() {
            if let Some(d) = box_difference(x, y, &mut vec![i]) {
                return Some(d);
            }
        }
        None
    }
}

/// The first line that differs between the pretty debug output of two
/// values, with the lines before it as context.
fn debug_difference(a: &impl std::fmt::Debug, b: &impl std::fmt::Debug) -> String {
    let (a, b) = (format!("{a:#?}"), format!("{b:#?}"));
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let Some(at) = a.iter().zip(&b).position(|(x, y)| x != y) else {
        return format!("{} lines against {}", a.len(), b.len());
    };
    let context = a[at.saturating_sub(6)..at]
        .iter()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" / ");
    format!("{context} / {} -> {}", a[at].trim(), b[at].trim())
}

/// The first line that differs between the debug output of two styles,
/// or `None` if the styles are equal.
fn style_difference(a: &Arc<ComputedStyle>, b: &Arc<ComputedStyle>) -> Option<String> {
    if Arc::ptr_eq(a, b) || **a == **b {
        return None;
    }
    Some(debug_difference(&**a, &**b))
}

fn rect_near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 0.01
        && (a.y - b.y).abs() < 0.01
        && (a.width - b.width).abs() < 0.01
        && (a.height - b.height).abs() < 0.01
}

fn overflow_near(a: Option<Rect>, b: Option<Rect>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => rect_near(a, b),
        _ => false,
    }
}

fn children_difference(
    a: &[Fragment],
    b: &[Fragment],
    what: &str,
    path: &mut Vec<usize>,
) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("{path:?}: {} {what} against {}", a.len(), b.len()));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        path.push(i);
        let found = match (x, y) {
            (Fragment::Box(x), Fragment::Box(y)) => box_difference(x, y, path),
            (Fragment::Text(x), Fragment::Text(y)) => text_difference(x, y, path),
            _ => Some(format!("{path:?}: a box against a text")),
        };
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

fn box_difference(a: &BoxFragment, b: &BoxFragment, path: &mut Vec<usize>) -> Option<String> {
    let label = format!("{path:?} (node {:?})", a.node);
    let at = |what: &str| Some(format!("{label}: {what}"));
    if a.node != b.node || a.pseudo != b.pseudo {
        return at("the element");
    }
    if let Some(d) = style_difference(&a.style, &b.style) {
        return at(&format!("style: {d}"));
    }
    if a.border_rect != b.border_rect {
        return at(&format!(
            "border box {:?} against {:?}",
            a.border_rect, b.border_rect
        ));
    }
    if a.border != b.border || a.padding != b.padding {
        return at("border or padding");
    }
    if a.content != b.content {
        return at(&format!(
            "content: {}",
            debug_difference(&a.content, &b.content)
        ));
    }
    if a.first_baseline != b.first_baseline
        || a.last_baseline != b.last_baseline
        || a.is_inline != b.is_inline
        || a.in_positioned_inline != b.in_positioned_inline
        || a.hanging_from != b.hanging_from
        || a.is_root != b.is_root
    {
        return at("baselines or flags");
    }
    if !overflow_near(a.scrollable_overflow, b.scrollable_overflow) {
        return at(&format!(
            "scrollable overflow {:?} against {:?}",
            a.scrollable_overflow, b.scrollable_overflow
        ));
    }
    if let Some(d) = children_difference(&a.children, &b.children, "children", path) {
        return Some(d);
    }
    match (&a.hidden, &b.hidden) {
        (None, None) => None,
        (Some(x), Some(y)) => children_difference(x, y, "hidden children", path),
        _ => at("hidden contents"),
    }
}

fn text_difference(a: &TextFragment, b: &TextFragment, path: &mut Vec<usize>) -> Option<String> {
    let label = format!("{path:?} (text of node {:?})", a.node);
    let at = |what: &str| Some(format!("{label}: {what}"));
    if a.node != b.node || a.text != b.text {
        return at("the text");
    }
    if let Some(d) = style_difference(&a.style, &b.style) {
        return at(&format!("style: {d}"));
    }
    if a.rect != b.rect
        || a.baseline != b.baseline
        || a.font != b.font
        || a.font_size != b.font_size
        || a.glyphs != b.glyphs
        || a.carets != b.carets
        || a.line_top != b.line_top
        || a.line_height != b.line_height
    {
        return at("geometry or glyphs");
    }
    None
}
