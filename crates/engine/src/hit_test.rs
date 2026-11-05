//! Hit testing: which node is at a point, and which link contains it.
//!
//! The display list has hit regions in paint order (see
//! `swb_paint::DisplayList::hit_test`), so the node found is the one that
//! is painted on top, with positioned boxes, z-index, overflow clips,
//! transforms, and fixed and sticky boxes at the current scroll offset
//! taken into account. A point in the content of a frame goes on into the
//! frame's document (ADR 0031 sections 4 and 8): the test of one document
//! ([`hit_test_in`]) reports the point in the frame, the page follows it.

use swb_dom::{Document, NodeId, local_name, ns};
use swb_layout::Point;
use swb_net::Url;
use swb_paint::{DisplayList, Hit, HitWork};

use crate::frames::FrameId;

/// The result of a hit test.
#[derive(Clone, Debug, PartialEq)]
pub struct HitResult {
    /// The frame whose document has the node ([`FrameId::TOP`] for the
    /// page's own document). A point in the content of an `iframe` hits a
    /// node of the frame's document; a point on its border hits the
    /// `iframe` element, in the parent's document.
    pub frame: FrameId,
    /// The innermost node at the point (a text node or an element).
    pub node: NodeId,
    /// The innermost link element (`a` or `area` with `href`) that contains
    /// the node.
    pub link_element: Option<NodeId>,
    /// The target of that link.
    pub link: Option<Url>,
}

/// The result of a hit test in one document.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DocumentHit {
    /// What the document has at the point. For a point in the content of
    /// an `iframe`, the `iframe` element.
    pub(crate) result: HitResult,
    /// For a point in the content of an `iframe`: the point in the frame's
    /// viewport (CSS px).
    pub(crate) into_frame: Option<Point>,
}

/// Finds the topmost node at `point` (document coordinates, CSS px) when
/// the viewport is scrolled to `scroll`, with `scale` device pixels per CSS
/// px (a frame's viewport starts on a device pixel). `work` is the budget
/// of the whole test, which a test through several frames shares.
pub(crate) fn hit_test_in(
    doc: &Document,
    list: &DisplayList,
    point: Point,
    scroll: Point,
    scale: f32,
    base_url: Option<&Url>,
    work: &HitWork,
) -> Option<DocumentHit> {
    let (node, into_frame) = match list.hit_test_with(point, scroll, work)? {
        Hit::Node(node) => (node, None),
        Hit::Frame { node, point, rect } => (
            node,
            Some(point_in_frame(point, rect.origin(), scroll, scale)),
        ),
    };
    let link = base_url.and_then(|base| link_around(doc, node, base));
    Some(DocumentHit {
        result: HitResult {
            frame: FrameId::TOP,
            node,
            link_element: link.as_ref().map(|(n, _)| *n),
            link: link.map(|(_, url)| url),
        },
        into_frame,
    })
}

/// The point in the viewport of a frame whose content box starts at
/// `corner` (document coordinates): the rasterizer draws the frame from
/// the corner snapped to a device pixel of the viewport (`paint/raster/
/// frame.rs`), and the point follows it.
fn point_in_frame(point: Point, corner: Point, scroll: Point, scale: f32) -> Point {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let snap = |corner: f32, scroll: f32| ((corner - scroll) * scale).round() / scale;
    Point::new(
        point.x - scroll.x - snap(corner.x, scroll.x),
        point.y - scroll.y - snap(corner.y, scroll.y),
    )
}

/// The innermost link element that contains `node` (or is `node`), and its
/// target resolved against `base`.
pub(crate) fn link_around(doc: &Document, node: NodeId, base: &Url) -> Option<(NodeId, Url)> {
    std::iter::once(node)
        .chain(doc.ancestors(node))
        .find_map(|n| Some((n, link_target(doc, n, base)?)))
}

/// The target of `node` if it is a link: an `a` or `area` element with an
/// `href` that resolves against `base`.
pub(crate) fn link_target(doc: &Document, node: NodeId, base: &Url) -> Option<Url> {
    let e = doc.element(node)?;
    if e.name.ns == ns!(svg) && *e.local_name() == local_name!("a") {
        // An SVG `a`: `href`, or `xlink:href` (SVG 2 §6.1).
        return base.join(e.svg_href()?.trim()).ok();
    }
    if !(e.is_html_named(&local_name!("a")) || e.is_html_named(&local_name!("area"))) {
        return None;
    }
    base.join(e.attr("href")?.trim()).ok()
}

/// The browsing context name that a link or form asks for (HTML "get an
/// element's target",
/// <https://html.spec.whatwg.org/multipage/document-sequences.html#get-an-element's-target>):
/// the `target` attribute of the element, else the `target` of the first
/// `base` element that has one; `None` if there is none or it is empty.
pub(crate) fn navigation_target(doc: &Document, node: NodeId) -> Option<String> {
    let own = doc
        .element(node)
        .and_then(|e| e.attr("target"))
        .map(str::to_owned);
    own.or_else(|| base_target(doc)).filter(|t| !t.is_empty())
}

/// The `target` attribute of the first `base` element that has one.
pub(crate) fn base_target(doc: &Document) -> Option<String> {
    doc.descendants(NodeId::DOCUMENT)
        .filter_map(|n| {
            doc.element(n)
                .filter(|e| e.is_html_named(&local_name!("base")))
        })
        .find_map(|e| e.attr("target").map(str::to_owned))
}
