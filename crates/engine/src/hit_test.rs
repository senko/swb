//! Hit testing: which node is at a point, and which link contains it.
//!
//! The display list has hit regions in paint order (see
//! `swb_paint::DisplayList::hit_test`), so the node found is the one that
//! is painted on top, with positioned boxes, z-index and overflow clips
//! taken into account.

use swb_dom::{Document, NodeId, local_name};
use swb_layout::Point;
use swb_net::Url;
use swb_paint::DisplayList;

/// The result of a hit test.
#[derive(Clone, Debug, PartialEq)]
pub struct HitResult {
    /// The innermost node at the point (a text node or an element).
    pub node: NodeId,
    /// The target of the innermost link that contains the node.
    pub link: Option<Url>,
}

/// Finds the topmost node at `point` (document coordinates, CSS px).
pub(crate) fn hit_test(
    doc: &Document,
    list: &DisplayList,
    point: Point,
    base_url: Option<&Url>,
) -> Option<HitResult> {
    let node = list.hit_test(point)?;
    let link = base_url.and_then(|base| enclosing_link(doc, node, base));
    Some(HitResult { node, link })
}

fn enclosing_link(doc: &Document, node: NodeId, base: &Url) -> Option<Url> {
    std::iter::once(node)
        .chain(doc.ancestors(node))
        .find_map(|n| {
            let e = doc.element(n)?;
            let is_link =
                e.is_html_named(&local_name!("a")) || e.is_html_named(&local_name!("area"));
            if !is_link {
                return None;
            }
            let href = e.attr("href")?.trim();
            base.join(href).ok()
        })
}
