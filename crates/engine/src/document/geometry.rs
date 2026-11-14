//! Element geometry and scrolling for scripts (CSSOM View,
//! <https://drafts.csswg.org/cssom-view/>; M8 feature 7b, ADR 0028 section
//! 9): `getClientRects`, `offset*`, `client*`, `scroll*`, the scrolls of
//! scripts, `scrollIntoView`, `elementsFromPoint` and the origins of the
//! `offsetX` and `layerX` of dispatched mouse events.
//!
//! Each query flushes the layout first ([`DocumentState::script_layout_flush`];
//! a run in a script task counts as forced). The values come from two
//! indexes, each built by one walk of the fragment tree, so that reading
//! the geometry of every element costs one walk, not one per element:
//!
//! - the boxes of the layout ([`BoxGeometry`], keyed by the layout serial):
//!   the border boxes as layout placed them, without transforms and scroll
//!   offsets, for `offset*`, `client*` and `scroll*`;
//! - the client rectangles at a scroll state ([`RectIndex`], keyed by the
//!   layout serial and the scroll offsets): the border box of each box
//!   fragment as painted, with transforms, sticky and fixed positioning
//!   and the scroll offsets of scroll containers, in document coordinates.
//!
//! Units, as Chromium 148 gives them (measured with
//! `tools/probes/web-geometry.json`): rectangles are in layout units
//! (multiples of 1/64 px) where no transform applies, and the result of
//! `f32` arithmetic on those where one does; `offset*` and `client*` are
//! the values in layout units rounded to whole px (halves up);
//! `scrollWidth` and `scrollHeight` too; scroll offsets are whole px.

use std::collections::HashMap;
use std::sync::Arc;

use swb_dom::{NodeId, QuirksMode, local_name, ns};
use swb_layout::{
    BoxContent, BoxFragment, Caret, Fragment, FragmentRef, Matrix, Point, Rect, Size,
    scrolling_area,
};
use swb_style::cssom::resolved::layout_unit;
use swb_style::{Display, Position};
use swb_web::{
    DomRect, EventOrigins, ObservedBox, ObservedPosition, OffsetMetrics, ScrollAlign,
    ScrollMetrics, ScrollRequest,
};

use super::DocumentState;
use crate::scrollers::Align;

/// The first box fragment of an element in the fragment tree: the children
/// list that holds it and its index (`None` for the root fragment).
type FragmentAt = Option<(Arc<Vec<Fragment>>, usize)>;

/// What `offset*`, `client*` and `scroll*` need of the boxes of an element
/// in one layout. Positions are in document coordinates as layout placed
/// the boxes (no transforms, no scroll offsets).
#[derive(Clone, Debug)]
pub(crate) struct BoxGeometry {
    /// The border box of the first box fragment.
    first: Rect,
    /// The union of the border boxes of all its box fragments.
    union: Rect,
    /// The borders of the first fragment (top, right, bottom, left).
    border: [f32; 4],
    /// The paddings of the first fragment.
    padding: [f32; 4],
    /// True if the first fragment is a part of an inline box (not an
    /// atomic inline).
    inline: bool,
    /// Where the first fragment is, for its scrolling area.
    at: FragmentAt,
}

/// The boxes of the elements of a layout.
pub(crate) type GeometryIndex = HashMap<NodeId, BoxGeometry>;

/// `getBBox` and `getTotalLength` of the elements of one outer `<svg>`
/// (`None`: the `<svg>` itself), from `SvgContent::script_geometry`.
pub(crate) type SvgGeometry = HashMap<Option<NodeId>, (Rect, f32)>;

/// The client rectangles of the elements at one scroll state, in document
/// coordinates (CSS px).
#[derive(Debug, Default)]
pub(crate) struct RectIndex {
    serial: u64,
    scroll: Point,
    offsets: HashMap<NodeId, Point>,
    rects: HashMap<NodeId, Vec<Rect>>,
    /// The text fragments of each text node at the same state, built at the
    /// first `Range.getClientRects` ([`DocumentState::script_range_rects`]).
    texts: Option<HashMap<NodeId, Vec<TextPiece>>>,
}

/// A text fragment as painted, for the client rectangles of ranges.
#[derive(Debug)]
struct TextPiece {
    /// The rectangle of the fragment (its content area) in document
    /// coordinates as layout placed it, with the scroll offsets of scroll
    /// containers.
    rect: Rect,
    /// The transform to painted document coordinates.
    matrix: Matrix,
    /// The caret stops of the fragment.
    carets: Arc<[Caret]>,
    /// True if its `white-space` preserves line breaks.
    breaks: bool,
}

/// `value` in Chromium's layout units (a multiple of 1/64 px).
fn unit(value: f32) -> f32 {
    layout_unit(f64::from(value)) as f32
}

/// `value` rounded to a whole px as Chromium rounds layout units (halves
/// up), saturated to `i32`.
fn whole(value: f32) -> i32 {
    // `as` saturates; NaN becomes 0.
    (f64::from(unit(value)) + 0.5).floor() as i32
}

/// The border box of `b` at `rect` in layout units: Chromium keeps the
/// content size, the paddings and the borders in layout units each, so a
/// box of 100.4 px with paddings of 0.3 px is 100.984375 px wide
/// (100.390625 + 2 × 0.296875), not 101.
fn snap_box(b: &BoxFragment, rect: Rect) -> Rect {
    let (p, e) = (&b.padding, &b.border);
    let content_width = rect.width - p.left - p.right - e.left - e.right;
    let content_height = rect.height - p.top - p.bottom - e.top - e.bottom;
    let width = [content_width, p.left, p.right, e.left, e.right]
        .into_iter()
        .map(unit)
        .sum::<f32>();
    let height = [content_height, p.top, p.bottom, e.top, e.bottom]
        .into_iter()
        .map(unit)
        .sum::<f32>();
    Rect::new(unit(rect.x), unit(rect.y), width, height)
}

/// The bounding box of `rect` mapped by `matrix`, from its four corners in
/// `f32`, as Chromium 148 maps a box through a transform: its size is the
/// difference of the edges (a box translated by 3.3 px is 7.999999 px
/// wide; a rotation by 45 degrees gives the same `f32` values as
/// Chromium's).
pub(super) fn map_rect(matrix: &Matrix, rect: Rect) -> Rect {
    if *matrix == Matrix::IDENTITY {
        return rect;
    }
    let corners = [
        matrix.apply(Point::new(rect.x, rect.y)),
        matrix.apply(Point::new(rect.right(), rect.y)),
        matrix.apply(Point::new(rect.x, rect.bottom())),
        matrix.apply(Point::new(rect.right(), rect.bottom())),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in corners {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Normalizes a coordinate that a script gave (CSSOM View "normalize
/// non-finite values"): NaN and the infinities are 0, and so are values
/// beyond the `f32` range (Chromium: `scrollTo(0, 1e300)` leaves the
/// position at 0, `3.4e38` scrolls to the end).
fn finite(value: f64) -> f32 {
    let value = value as f32;
    if value.is_finite() { value } else { 0.0 }
}

impl DocumentState {
    /// The layout flush of a geometry query; false if there is no layout
    /// or `node` is not in the document.
    fn geometry_flush(&mut self, node: NodeId) -> bool {
        self.geometry_layout() && self.dom.is_connected(node)
    }

    /// The layout flush of a geometry query, then the scroll into view that
    /// a `focus()` of the task asked for (see
    /// [`DocumentState::script_request_scroll`]). False if there is no
    /// layout.
    fn geometry_layout(&mut self) -> bool {
        if !self.script_layout_flush() {
            return false;
        }
        if let Some(node) = self.script_requests.scroll_into_view.take()
            && self.dom.is_connected(node)
        {
            self.with_lent_fonts(|doc, fonts| doc.scroll_into_view(node, fonts));
        }
        true
    }

    /// The boxes of `node` in the current layout (after a flush).
    fn box_geometry(&mut self, node: NodeId) -> Option<&BoxGeometry> {
        let serial = self.layout_serial;
        if self.geometry.as_ref().is_none_or(|(s, _)| *s != serial) {
            let tree = self.fragments.as_ref()?;
            self.geometry = Some((serial, index_geometry(tree)));
        }
        self.geometry.as_ref()?.1.get(&node)
    }

    /// The client rectangles of `node` in document coordinates (after a
    /// flush).
    fn painted_rects(&mut self, node: NodeId) -> &[Rect] {
        self.update_rect_index();
        self.client_rects
            .as_ref()
            .and_then(|i| i.rects.get(&node))
            .map_or(&[], Vec::as_slice)
    }

    /// Builds the index of client rectangles again if the layout or a
    /// scroll offset changed since it was built (after a flush).
    fn update_rect_index(&mut self) {
        let serial = self.layout_serial;
        let current = self.client_rects.as_ref().is_some_and(|i| {
            i.serial == serial && i.scroll == self.scroll && &i.offsets == self.scrollers.offsets()
        });
        if !current && let Some(tree) = &self.fragments {
            self.client_rects = Some(RectIndex {
                serial,
                scroll: self.scroll,
                offsets: self.scrollers.offsets().clone(),
                rects: index_rects(tree, self.scrollers.offsets(), self.scroll),
                texts: None,
            });
        }
    }

    /// `Range.getClientRects` (CSSOM View §6.2) of `range`, in viewport
    /// coordinates: for each piece of [`swb_dom::Document::range_pieces`] in
    /// tree order, the client rectangles of an element, or the rectangles of
    /// the selected part of a text node on each of its fragments (width 0 for
    /// an empty part: a collapsed range, or a boundary point at the end of a
    /// text node). Empty for a range outside the document.
    pub(crate) fn script_range_rects(&mut self, range: swb_dom::LiveRange) -> Vec<DomRect> {
        if !self.geometry_flush(range.start.node) {
            return Vec::new();
        }
        let pieces = self.dom.range_pieces(range);
        self.update_rect_index();
        let (Some(index), Some(tree)) = (self.client_rects.as_mut(), self.fragments.as_ref())
        else {
            return Vec::new();
        };
        let texts = index
            .texts
            .get_or_insert_with(|| index_texts(tree, &index.offsets, index.scroll));
        let mut out = Vec::new();
        for piece in pieces {
            match piece {
                swb_dom::RangePiece::Element(node) => {
                    out.extend(index.rects.get(&node).into_iter().flatten().copied());
                }
                swb_dom::RangePiece::Text { node, start, end } => {
                    if let Some(list) = texts.get(&node) {
                        let data = self.dom.node(node).as_text().unwrap_or_default();
                        text_rects(list, data, start, end, &mut out);
                    }
                }
            }
        }
        let scroll = self.scroll;
        out.iter()
            .map(|r| DomRect {
                x: f64::from(r.x) - f64::from(scroll.x),
                y: f64::from(r.y) - f64::from(scroll.y),
                width: f64::from(r.width),
                height: f64::from(r.height),
            })
            .collect()
    }

    /// `getClientRects()` of `node`, in viewport coordinates.
    pub(crate) fn script_client_rects(&mut self, node: NodeId) -> Vec<DomRect> {
        if !self.geometry_flush(node) {
            return Vec::new();
        }
        let scroll = self.scroll;
        self.painted_rects(node)
            .iter()
            .map(|r| DomRect {
                x: f64::from(r.x) - f64::from(scroll.x),
                y: f64::from(r.y) - f64::from(scroll.y),
                width: f64::from(r.width),
                height: f64::from(r.height),
            })
            .collect()
    }

    /// True if the document is in quirks mode.
    fn quirks(&self) -> bool {
        self.dom.quirks_mode == QuirksMode::Quirks
    }

    /// `offsetParent` and the `offset*` values of `node` (CSSOM View §7).
    pub(crate) fn script_offset_metrics(&mut self, node: NodeId) -> OffsetMetrics {
        if !self.geometry_flush(node) {
            return OffsetMetrics::default();
        }
        let Some(g) = self.box_geometry(node).cloned() else {
            return OffsetMetrics::default();
        };
        let (width, height) = (whole(g.union.width), whole(g.union.height));
        let body = self.dom.body();
        // Chromium lays out a `br` as text: it has an offset parent but no
        // offset box (all four values 0).
        if self
            .dom
            .element(node)
            .is_some_and(|e| e.is_html_named(&local_name!("br")))
        {
            return OffsetMetrics {
                parent: self.offset_parent(node),
                ..OffsetMetrics::default()
            };
        }
        if body == Some(node) {
            return OffsetMetrics {
                width,
                height,
                ..OffsetMetrics::default()
            };
        }
        let fixed = self
            .styles
            .as_ref()
            .and_then(|s| s.get(node))
            .is_some_and(|s| s.position == Position::Fixed);
        let parent = if self.dom.document_element() == Some(node) {
            None
        } else if fixed {
            // Chromium 148: the containing block of a fixed box that is a
            // transformed or filtered ancestor; none for the viewport.
            self.fixed_containing_block(node)
        } else {
            self.offset_parent(node)
        };
        // Relative to the padding edge of the offset parent; for the body,
        // to the border box of the root element (Chromium 148: with `html
        // { margin: 5px }` a box at x 23 has `offsetLeft` 18); for none, to
        // the initial containing block.
        // A fixed box in an inline box with a transform or a filter has it
        // as its offset parent, but the viewport as its containing block.
        let basis = parent.filter(|&p| {
            !fixed
                || self
                    .styles
                    .as_ref()
                    .and_then(|s| s.get(p))
                    .is_some_and(|s| s.display != Display::Inline)
        });
        let origin = match basis {
            Some(p) if Some(p) == body => self
                .dom
                .document_element()
                .and_then(|root| self.box_geometry(root))
                .map(|r| r.first.origin()),
            Some(p) => self
                .box_geometry(p)
                .map(|p| Point::new(p.first.x + p.border[3], p.first.y + p.border[0])),
            None => None,
        }
        .unwrap_or_default();
        let first = g.first.origin() + self.sticky_shift(node, g.first);
        OffsetMetrics {
            parent,
            left: whole_difference(first.x, origin.x),
            top: whole_difference(first.y, origin.y),
            width,
            height,
        }
    }

    /// How far sticky positioning moves the box of `node` (at `layout` in
    /// the layout) at the current scroll offsets: Chromium 148's `offsetTop`
    /// of a stuck box includes it (a box with `top: 10px` at `scrollY` 200
    /// has `offsetTop` 210). Zero for other boxes.
    fn sticky_shift(&mut self, node: NodeId, layout: Rect) -> Point {
        let sticky = self
            .styles
            .as_ref()
            .and_then(|s| s.get(node))
            .is_some_and(|s| s.position == Position::Sticky);
        let Some(painted) = sticky
            .then(|| self.painted_rects(node).first().copied())
            .flatten()
        else {
            return Point::default();
        };
        // The painted box moved by minus the offsets of its scroll
        // containers; those do not count.
        let scrolled = self.styles.as_ref().map_or(Point::default(), |styles| {
            self.scrollers
                .chain(&self.dom, styles, node, false)
                .into_iter()
                .fold(Point::default(), |sum, s| sum + self.scrollers.offset(s))
        });
        Point::new(
            painted.x - layout.x + scrolled.x,
            painted.y - layout.y + scrolled.y,
        )
    }

    /// The nearest ancestor of `node` that is transformed or has a
    /// `filter` other than `none`, also an inline box (Chromium 148 takes
    /// it as the offset parent of a fixed box although layout does not
    /// make it the containing block).
    fn fixed_containing_block(&self, node: NodeId) -> Option<NodeId> {
        let styles = self.styles.as_ref()?;
        let root = self.dom.document_element();
        self.dom.flat_ancestors(node).find(|&ancestor| {
            // The `filter` of the root element is not one (Chromium 148).
            styles.get(ancestor).is_some_and(|s| {
                s.has_transform() || (s.is_filter_containing_block() && Some(ancestor) != root)
            })
        })
    }

    /// The offset parent of `node` (CSSOM View §7 "offsetParent"): the
    /// nearest ancestor that is the containing block of absolutely
    /// positioned boxes (positioned or transformed; Chromium 148 also
    /// takes a transformed ancestor, as the specification now says), the
    /// body, or, for an element that is not positioned, a `td`, `th` or
    /// `table`. The caller handles the root, the body and fixed boxes. The
    /// ancestors are those of the flat tree, and the answer is retargeted
    /// against `node` (CSSOM View; measured in Chromium 148, case
    /// `render-reads`: the offset parent of a shadow element is the light
    /// `body`).
    fn offset_parent(&self, node: NodeId) -> Option<NodeId> {
        self.offset_parent_in_flat_tree(node)
            .map(|parent| self.dom.retarget(parent, node))
    }

    /// [`Self::offset_parent`] before the retargeting.
    fn offset_parent_in_flat_tree(&self, node: NodeId) -> Option<NodeId> {
        let styles = self.styles.as_ref()?;
        let body = self.dom.body();
        let own_static = styles
            .get(node)
            .is_none_or(|s| s.position == Position::Static);
        let root = self.dom.document_element();
        for ancestor in self.dom.flat_ancestors(node) {
            let element = self.dom.element(ancestor)?;
            if body == Some(ancestor) {
                return Some(ancestor);
            }
            // The `filter` of the root element is not a containing block.
            if styles.get(ancestor).is_some_and(|s| {
                s.position != Position::Static
                    || s.has_transform()
                    || (s.is_filter_containing_block() && Some(ancestor) != root)
                    // Layout and paint containment make a containing block
                    // (measured in Chromium 148, `cv-hidden-boxes`).
                    || ((s.has_layout_containment() || s.has_paint_containment())
                        && Some(ancestor) != root)
            }) {
                return Some(ancestor);
            }
            let name = element.local_name();
            let html = element.name.ns == ns!(html);
            if own_static
                && html
                && (*name == local_name!("td")
                    || *name == local_name!("th")
                    || *name == local_name!("table"))
            {
                return Some(ancestor);
            }
            // An SVG `foreignObject` holds its HTML content as a containing
            // block does (Chromium: the offset parent of a `div` in it).
            if element.name.ns == ns!(svg) && *name == local_name!("foreignObject") {
                return Some(ancestor);
            }
        }
        None
    }

    /// True if `node` reports the viewport as its client area: the root
    /// element in standards mode, the body in quirks mode (CSSOM View
    /// `clientWidth`).
    fn has_viewport_client_area(&self, node: NodeId) -> bool {
        if self.quirks() {
            self.dom.body() == Some(node)
        } else {
            self.dom.document_element() == Some(node)
        }
    }

    /// `clientLeft`, `clientTop`, `clientWidth` and `clientHeight` of
    /// `node` (CSSOM View §6): the borders and the padding box (scrollbars
    /// take no space), zero for an inline box or without a box.
    pub(crate) fn script_client_metrics(&mut self, node: NodeId) -> [i32; 4] {
        if !self.geometry_flush(node) {
            return [0; 4];
        }
        let viewport = self
            .has_viewport_client_area(node)
            .then_some(self.view.viewport);
        let Some(g) = self.box_geometry(node) else {
            return match viewport {
                Some(v) => [0, 0, whole(v.width), whole(v.height)],
                None => [0; 4],
            };
        };
        if g.inline {
            return [0; 4];
        }
        let [top, right, bottom, left] = g.border.map(unit);
        let size = viewport.unwrap_or_else(|| {
            Size::new(
                unit(g.first.width) - left - right,
                unit(g.first.height) - top - bottom,
            )
        });
        [
            whole(left),
            whole(top),
            whole(size.width),
            whole(size.height),
        ]
    }

    /// The `scroll*` values of `node` (CSSOM View §6).
    pub(crate) fn script_scroll_metrics(&mut self, node: NodeId) -> ScrollMetrics {
        if !self.geometry_flush(node) {
            return ScrollMetrics::default();
        }
        let offset = if self.is_scrolling_element(node) {
            self.scroll
        } else {
            self.scrollers.offset(node)
        };
        let root = self.dom.document_element() == Some(node);
        let quirks_body =
            self.quirks() && self.dom.body() == Some(node) && self.scrollers.get(node).is_none();
        let size = if root || quirks_body {
            // The viewport's scrolling area.
            self.fragments
                .as_ref()
                .map_or(self.view.viewport, |f| f.scroll_size)
        } else if let Some(b) = self.scrollers.get(node) {
            b.scroll_size
        } else {
            match self.box_geometry(node) {
                Some(g) if !g.inline => match &g.at {
                    Some((children, index)) => match children.get(*index) {
                        Some(Fragment::Box(b)) => scrolling_area(b),
                        _ => Size::default(),
                    },
                    None => Size::default(),
                },
                _ => Size::default(),
            }
        };
        ScrollMetrics {
            left: f64::from(offset.x),
            top: f64::from(offset.y),
            width: whole(size.width),
            height: whole(size.height),
        }
    }

    /// Scrolls `node` as `scrollTop = ...`, `element.scrollTo` and
    /// `element.scrollBy` do (CSSOM View §6): the element that scrolls the
    /// page scrolls the viewport, a scroll container its content (also with
    /// `overflow: hidden`); other elements do not scroll.
    pub(crate) fn script_scroll_element(&mut self, node: NodeId, request: ScrollRequest) {
        if !self.geometry_flush(node) {
            return;
        }
        if self.is_scrolling_element(node) {
            self.script_scroll_viewport(request);
            return;
        }
        if self.scrollers.get(node).is_none() {
            return;
        }
        let current = self.scrollers.offset(node);
        let target = scroll_target(current, request);
        self.set_element_scroll(node, target);
    }

    /// Scrolls the viewport as `window.scrollTo` and `scrollBy` do.
    pub(crate) fn script_scroll_viewport(&mut self, request: ScrollRequest) {
        if !self.geometry_layout() {
            return;
        }
        let target = scroll_target(self.scroll, request);
        self.with_lent_fonts(|doc, fonts| doc.scroll_to(target, fonts));
    }

    /// `scrollIntoView` (CSSOM View "scroll an element into view"): every
    /// scroll container of `node`, then the viewport, with `block` on the
    /// vertical and `inline` on the horizontal axis.
    pub(crate) fn script_scroll_into_view(
        &mut self,
        node: NodeId,
        block: ScrollAlign,
        inline: ScrollAlign,
    ) {
        if !self.geometry_flush(node) {
            return;
        }
        let Some(rect) = self
            .with_lent_fonts(|doc, fonts| doc.element_box(node, fonts))
            .flatten()
        else {
            return;
        };
        self.pending_scroll = None;
        self.reveal(node, rect, align(inline), align(block));
    }

    /// The scroll into view of `focus()` without `preventScroll`. Chromium
    /// scrolls at once (`scrollY` right after `focus()` shows it); swb
    /// scrolls at the next geometry query of the task or when the task ends
    /// (the page), so that a loop that changes the DOM and calls `focus()`
    /// does not lay out at each call (the focus change itself restyles, so
    /// the layout is never current here). A later `focus()` replaces the
    /// request. Departure: if the element is removed or hidden before that
    /// point, the scroll is lost (Chromium has made it: `focus(); remove();
    /// scrollY` gives the scrolled position there, 0 in swb).
    pub(crate) fn script_request_scroll(&mut self, node: NodeId) {
        self.script_requests.scroll_into_view = Some(node);
    }

    /// `scrollX` and `scrollY` with the scroll of a `focus()` of the task
    /// applied (a layout flush only if one is waiting).
    pub(crate) fn script_scroll_position(&mut self) -> (f64, f64) {
        if self.script_requests.scroll_into_view.is_some() {
            self.geometry_layout();
        }
        self.script_scroll_offset()
    }

    /// `document.scrollingElement`: the root element, or in quirks mode the
    /// body if it is not a scroll container.
    pub(crate) fn script_scrolling_element(&mut self) -> Option<NodeId> {
        if !self.quirks() {
            return self.dom.document_element();
        }
        let body = self.dom.body()?;
        self.geometry_layout();
        self.is_scrolling_element(body).then_some(body)
    }

    /// The elements at the point (`x`, `y`) of the viewport, topmost first
    /// (`elementsFromPoint`; at most one without `all`). The point is
    /// rounded to whole px (Chromium 148: `(799.9, 5)` in a viewport 800 px
    /// wide is outside). A text hit gives its element; a point where no box
    /// is gives the root element, which is also the last of all.
    pub(crate) fn script_elements_from_point(&mut self, x: f64, y: f64, all: bool) -> Vec<NodeId> {
        let (x, y) = ((x + 0.5).floor(), (y + 0.5).floor());
        let view = self.view.viewport;
        if x < 0.0 || y < 0.0 || x >= f64::from(view.width) || y >= f64::from(view.height) {
            return Vec::new();
        }
        if !self.geometry_layout() {
            return Vec::new();
        }
        self.with_lent_fonts(DocumentState::update_display_list);
        let Some(list) = &self.display_list else {
            return Vec::new();
        };
        let point = Point::new(x as f32 + self.scroll.x, y as f32 + self.scroll.y);
        let hits = list.hit_test_all(point, self.scroll);
        let mut found: Vec<NodeId> = Vec::new();
        for hit in hits {
            // The bindings retarget the elements against the document or
            // the shadow root that asks.
            let element = self.element_of(hit);
            if let Some(element) = element
                && !found.contains(&element)
            {
                found.push(element);
                if !all {
                    return found;
                }
            }
        }
        if let Some(root) = self.dom.document_element()
            && !found.contains(&root)
        {
            found.push(root);
        }
        found
    }

    /// The layout box of `node` for the intersection and resize observers
    /// (feature 9a); `None` if it has no box.
    pub(crate) fn script_observed_box(&mut self, node: NodeId) -> Option<ObservedBox> {
        if !self.geometry_flush(node) {
            return None;
        }
        let g = self.box_geometry(node)?.clone();
        let style = self.query_computed_style(node, None)?;
        // As `snap_box`: the content size and each padding and border are
        // layout units; the border box is their sum (measured: a width of
        // 100.4 px gives a content box of 100.390625 px).
        let [bt, br, bb, bl] = g.border.map(|v| f64::from(unit(v)));
        let [pt, pr, pb, pl] = g.padding.map(|v| f64::from(unit(v)));
        let [rbt, rbr, rbb, rbl] = g.border;
        let [rpt, rpr, rpb, rpl] = g.padding;
        let content_width = f64::from(unit((g.first.width - rbl - rbr - rpl - rpr).max(0.0)));
        let content_height = f64::from(unit((g.first.height - rbt - rbb - rpt - rpb).max(0.0)));
        Some(ObservedBox {
            border_size: (
                content_width + bl + br + pl + pr,
                content_height + bt + bb + pt + pb,
            ),
            content_size: (content_width, content_height),
            padding_origin: (pl, pt),
            border_widths: [bt, br, bb, bl],
            inline: g.inline,
            position: match style.position {
                Position::Absolute => ObservedPosition::Absolute,
                Position::Fixed => ObservedPosition::Fixed,
                Position::Static => ObservedPosition::Static,
                _ => ObservedPosition::Relative,
            },
            transformed: style.has_transform() || style.is_filter_containing_block(),
            clips: style.clips_x() || style.clips_y(),
        })
    }

    /// The size of the content box of the first box of `node` in layout
    /// units (`img.width` and `img.height` of a rendered image); `None` if
    /// it has no box.
    pub(crate) fn script_content_size(&mut self, node: NodeId) -> Option<(f64, f64)> {
        if !self.geometry_flush(node) {
            return None;
        }
        let g = self.box_geometry(node)?;
        let [top, right, bottom, left] = g.border;
        let [pt, pr, pb, pl] = g.padding;
        let width = unit(g.first.width) - unit(left) - unit(right) - unit(pl) - unit(pr);
        let height = unit(g.first.height) - unit(top) - unit(bottom) - unit(pt) - unit(pb);
        Some((f64::from(width.max(0.0)), f64::from(height.max(0.0))))
    }

    /// `getBBox` and `getTotalLength` of an SVG element (feature 7b2): its
    /// fill bounding box in its user space and the length of its path,
    /// from the content of its outer `<svg>` as layout drew it; `None` if
    /// it is not drawn (not in the document, `display: none`, an element
    /// that swb draws nothing for).
    pub(crate) fn script_svg_geometry(&mut self, node: NodeId) -> Option<(DomRect, f64)> {
        if !self.geometry_flush(node) {
            return None;
        }
        let is_svg = |n: NodeId| {
            self.dom
                .element(n)
                .is_some_and(|e| e.name.ns == ns!(svg) && e.name.local == local_name!("svg"))
        };
        let in_svg = |n: NodeId| self.dom.element(n).is_some_and(|e| e.name.ns == ns!(svg));
        // The outer `<svg>`: the last `svg` element of the chain of SVG
        // elements from `node` up.
        let root = std::iter::once(node)
            .chain(self.dom.ancestors(node))
            .take_while(|&n| in_svg(n))
            .filter(|&n| is_svg(n))
            .last()?;
        // The geometry of all the elements of the `<svg>`, once per layout:
        // a loop over its elements walks the fragments once.
        let serial = self.layout_serial;
        if self
            .svg_geometry
            .as_ref()
            .is_none_or(|(s, r, _)| *s != serial || *r != root)
        {
            let tree = self.fragments.as_ref()?;
            let mut found = None;
            tree.walk(|f, _| {
                if found.is_none()
                    && let FragmentRef::Box(b) = f
                    && b.node == Some(root)
                    && let BoxContent::Svg(svg) = &b.content
                {
                    let content = b.content_rect();
                    found = Some(svg.script_geometry(Size::new(content.width, content.height)));
                }
            });
            self.svg_geometry = Some((serial, root, found.unwrap_or_default()));
        }
        let all = &self.svg_geometry.as_ref()?.2;
        let &(rect, length) = all.get(&(node != root).then_some(node))?;
        let rect = DomRect {
            x: f64::from(rect.x),
            y: f64::from(rect.y),
            width: f64::from(rect.width),
            height: f64::from(rect.height),
        };
        Some((rect, f64::from(length)))
    }

    /// The origins of `offsetX` and `layerX` of a mouse event dispatched at
    /// `target`: the padding edge of its first box as painted (with the
    /// scroll offsets of its scroll containers) and the border edge of the
    /// nearest positioned inclusive ancestor (Chromium 148: the layer
    /// position; the origin without one), in document coordinates.
    pub(crate) fn script_event_origins(&mut self, target: NodeId) -> Option<EventOrigins> {
        if !self.geometry_flush(target) {
            return None;
        }
        let first = *self.painted_rects(target).first()?;
        let border = self.box_geometry(target).map_or([0.0; 4], |g| g.border);
        let padding = (
            f64::from(first.x + unit(border[3])),
            f64::from(first.y + unit(border[0])),
        );
        let positioned = std::iter::once(target)
            .chain(self.dom.flat_ancestors(target))
            .take_while(|&n| self.dom.element(n).is_some())
            .find(|&n| {
                self.styles
                    .as_ref()
                    .and_then(|s| s.get(n))
                    .is_some_and(|s| s.position != Position::Static)
            });
        let layer = positioned
            .and_then(|n| self.painted_rects(n).first().copied())
            .map_or((0.0, 0.0), |r| (f64::from(r.x), f64::from(r.y)));
        Some(EventOrigins { padding, layer })
    }

    /// Notes that `node` (the document for the viewport) scrolled, for its
    /// `scroll` event in the next frame (CSSOM View "pending scroll event
    /// targets": each target once, in the order they scrolled).
    pub(crate) fn note_scrolled(&mut self, node: NodeId) {
        if !self.scrolled.contains(&node) {
            self.scrolled.push(node);
        }
    }
}

/// `a - b` in layout units, rounded to whole px.
fn whole_difference(a: f32, b: f32) -> i32 {
    let difference = f64::from(unit(a)) - f64::from(unit(b));
    (difference + 0.5).floor() as i32
}

/// Where a script scroll goes from `current`.
fn scroll_target(current: Point, request: ScrollRequest) -> Point {
    let axis = |value: Option<f64>, current: f32| match value {
        Some(v) if request.by => current + finite(v),
        Some(v) => finite(v),
        None => current,
    };
    Point::new(axis(request.x, current.x), axis(request.y, current.y))
}

/// The alignment of a `ScrollLogicalPosition`.
fn align(position: ScrollAlign) -> Align {
    match position {
        ScrollAlign::Start => Align::Start,
        ScrollAlign::Center => Align::Center,
        ScrollAlign::End => Align::End,
        ScrollAlign::Nearest => Align::Nearest,
    }
}

/// The boxes of the elements of a layout: those of the root fragment and
/// of the top layer.
fn index_geometry(tree: &swb_layout::FragmentTree) -> GeometryIndex {
    let mut index = GeometryIndex::new();
    for root in tree.roots() {
        add_box(root, Point::default(), None, &mut index);
    }
    index
}

/// Adds box `b` (whose parent's border box is at `origin`) and its
/// descendants to `index`. The depth of the fragment tree is bounded by
/// layout's box depth limit.
fn add_box(b: &BoxFragment, origin: Point, at: FragmentAt, index: &mut GeometryIndex) {
    let rect = b.border_rect.translate(origin);
    if let (Some(node), None) = (b.node, b.pseudo) {
        index
            .entry(node)
            .and_modify(|g| g.union = g.union.union(&rect))
            .or_insert_with(|| BoxGeometry {
                first: rect,
                union: rect,
                border: [b.border.top, b.border.right, b.border.bottom, b.border.left],
                padding: [
                    b.padding.top,
                    b.padding.right,
                    b.padding.bottom,
                    b.padding.left,
                ],
                inline: b.is_inline,
                at,
            });
    }
    // The content of a closed `details` counts (see `index_rects`).
    for list in std::iter::once(&b.children).chain(b.hidden.iter()) {
        for (i, child) in list.iter().enumerate() {
            if let Fragment::Box(child) = child {
                add_box(child, rect.origin(), Some((Arc::clone(list), i)), index);
            }
        }
    }
}

/// The client rectangles of the elements (CSSOM View `getClientRects`):
/// the border box of each box fragment as painted, in document
/// coordinates, in layout units mapped through the transforms; the boxes
/// of the elements inside an `<svg>` too.
fn index_rects(
    tree: &swb_layout::FragmentTree,
    offsets: &dyn swb_layout::ScrollOffsets,
    scroll: Point,
) -> HashMap<NodeId, Vec<Rect>> {
    let mut rects: HashMap<NodeId, Vec<Rect>> = HashMap::new();
    tree.walk_painted_with_hidden(offsets, scroll, |fragment, layout_rect, matrix| {
        let FragmentRef::Box(b) = fragment else {
            return;
        };
        let (Some(node), None) = (b.node, b.pseudo) else {
            return;
        };
        let rect = map_rect(matrix, snap_box(b, layout_rect));
        rects.entry(node).or_default().push(clamp(rect));
        if let BoxContent::Svg(svg) = &b.content {
            let content = b
                .content_rect()
                .translate(layout_rect.origin() - b.border_rect.origin());
            let size = Size::new(content.width, content.height);
            for (inner, local) in svg.element_boxes(size) {
                let rect = map_rect(matrix, local.translate(content.origin()));
                rects.insert(inner, vec![clamp(rect)]);
            }
        }
    });
    rects
}

/// The fragments of each text node as painted (see [`index_rects`]); the
/// text of pseudo-elements and list markers has no caret stops and no node.
fn index_texts(
    tree: &swb_layout::FragmentTree,
    offsets: &dyn swb_layout::ScrollOffsets,
    scroll: Point,
) -> HashMap<NodeId, Vec<TextPiece>> {
    let mut texts: HashMap<NodeId, Vec<TextPiece>> = HashMap::new();
    tree.walk_painted_with_hidden(offsets, scroll, |fragment, rect, matrix| {
        let FragmentRef::Text(t) = fragment else {
            return;
        };
        if t.carets.is_empty() {
            return;
        }
        texts.entry(t.node).or_default().push(TextPiece {
            rect,
            matrix: *matrix,
            carets: Arc::clone(&t.carets),
            breaks: !matches!(
                t.style.white_space,
                swb_style::WhiteSpace::Normal | swb_style::WhiteSpace::Nowrap
            ),
        });
    });
    texts
}

/// A line's selected part before its fragment's transform: the rectangle in
/// layout units and the index of the fragment whose transform maps it.
type Line = (Rect, usize);

/// The rectangles of the bytes `start..end` of a text node with the text
/// `data` on its fragments `pieces`, one per line as Chromium gives them (the
/// fragments of a line, which swb splits at words, are joined before the
/// transform maps them): the selected part of each line; a rectangle of width 0
/// at the end of a line whose preserved line break is selected, and one on each
/// empty line between preserved line breaks; for a collapsed part (or one that
/// no fragment shows), a rectangle of width 0 at the first caret stop at its
/// start. A text node of collapsed white space shows nothing and has no
/// rectangle (measured, cases `geometry`, `geometry-more` and
/// `geometry-review`).
fn text_rects(pieces: &[TextPiece], data: &str, start: u32, end: u32, out: &mut Vec<Rect>) {
    if shows_nothing(pieces, data) {
        return;
    }
    let mut lines: Vec<Line> = Vec::new();
    if start < end {
        selected_lines(pieces, data, (start, end), &mut lines);
    }
    if lines.is_empty() {
        for (i, piece) in pieces.iter().enumerate() {
            if let Some(caret) = piece.carets.iter().find(|c| c.offset == start) {
                out.push(mapped(pieces, (piece_box(piece, (caret.x, caret.x)), i)));
                return;
            }
        }
        return;
    }
    out.extend(lines.iter().map(|&line| mapped(pieces, line)));
}

/// True if a text node shows nothing: white space that collapsed, with no
/// width on any fragment.
fn shows_nothing(pieces: &[TextPiece], data: &str) -> bool {
    data.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c))
        && pieces.iter().all(|p| {
            !p.breaks
                && p.carets
                    .first()
                    .zip(p.carets.last())
                    .is_none_or(|(a, b)| a.x == b.x)
        })
}

/// The lines of the selected bytes `start..end`: the part of each fragment,
/// joined per line, and the preserved line breaks between fragments.
fn selected_lines(
    pieces: &[TextPiece],
    data: &str,
    (start, end): (u32, u32),
    lines: &mut Vec<Line>,
) {
    for (i, piece) in pieces.iter().enumerate() {
        let (Some(first), Some(last)) = (piece.carets.first(), piece.carets.last()) else {
            continue;
        };
        let (low, high) = (first.offset.min(last.offset), first.offset.max(last.offset));
        if low < end
            && high > start
            && let Some(span) = caret_span(&piece.carets, start.max(low), end.min(high))
        {
            join_or_push(lines, (piece_box(piece, span), i));
        }
        if piece.breaks {
            line_breaks(pieces, data, i, (start, end), lines);
        }
    }
}

/// The selected preserved line breaks between the fragment `i` and the next
/// one: the first ends the line of fragment `i` (width 0 at its end); each
/// further one is an empty line, a width 0 rectangle at the start of the next
/// fragment's line, on the lines evenly spaced between the two fragments.
fn line_breaks(
    pieces: &[TextPiece],
    data: &str,
    i: usize,
    (start, end): (u32, u32),
    lines: &mut Vec<Line>,
) {
    let piece = &pieces[i];
    let Some(next) = pieces.get(i + 1) else {
        return;
    };
    let high = piece.carets.iter().map(|c| c.offset).max().unwrap_or(0);
    let next_low = next.carets.iter().map(|c| c.offset).min().unwrap_or(0);
    if next_low <= high {
        return;
    }
    let gap = data
        .as_bytes()
        .get(high as usize..next_low as usize)
        .unwrap_or_default();
    let mut breaks: Vec<u32> = gap
        .iter()
        .zip(high..)
        .filter(|&(&b, _)| b == b'\n')
        .map(|(_, at)| at)
        .collect();
    if breaks.is_empty() {
        breaks.push(high);
    }
    let count = breaks.len();
    for (k, &at) in breaks.iter().enumerate() {
        if at < start || at >= end {
            continue;
        }
        if k == 0 {
            if let Some(caret) = piece.carets.iter().find(|c| c.offset == high) {
                lines.push((piece_box(piece, (caret.x, caret.x)), i));
            }
            continue;
        }
        let Some(left) = next.carets.first() else {
            continue;
        };
        let mut rect = piece_box(next, (left.x, left.x));
        let share = k as f32 / count as f32;
        rect.y = unit(piece.rect.y + (next.rect.y - piece.rect.y) * share);
        lines.push((rect, i + 1));
    }
}

/// Adds `line` to `lines`, joined with the last one if it continues it on the
/// same line (neither is empty).
fn join_or_push(lines: &mut Vec<Line>, line: Line) {
    let rect = line.0;
    if let Some((last, _)) = lines.last_mut()
        && last.width > 0.0
        && rect.width > 0.0
        && (last.y - rect.y).abs() < 0.01
        && (last.height - rect.height).abs() < 0.01
        && rect.x >= last.x
        && rect.x - last.right() < 0.5
    {
        let right = last.right().max(rect.right());
        last.width = right - last.x;
        return;
    }
    lines.push(line);
}

/// The horizontal extent (relative to the fragment's left edge) of the caret
/// stops with offsets in `from..=to`.
fn caret_span(carets: &[Caret], from: u32, to: u32) -> Option<(f32, f32)> {
    let mut span: Option<(f32, f32)> = None;
    for c in carets.iter().filter(|c| (from..=to).contains(&c.offset)) {
        span = Some(span.map_or((c.x, c.x), |(a, b)| (a.min(c.x), b.max(c.x))));
    }
    span
}

/// The rectangle of the horizontal extent `span` of a text fragment, before
/// its transform, in layout units.
fn piece_box(piece: &TextPiece, (left, right): (f32, f32)) -> Rect {
    let x0 = unit(piece.rect.x + left);
    let x1 = unit(piece.rect.x + right);
    Rect::new(x0, unit(piece.rect.y), x1 - x0, unit(piece.rect.height))
}

/// `line` as painted: mapped through the transform of its fragment.
fn mapped(pieces: &[TextPiece], (rect, piece): Line) -> Rect {
    pieces
        .get(piece)
        .map_or(rect, |p| clamp(map_rect(&p.matrix, rect)))
}

/// `rect` within the range of layout lengths.
fn clamp(rect: Rect) -> Rect {
    let c = swb_style::Length::clamp_px;
    Rect::new(c(rect.x), c(rect.y), c(rect.width), c(rect.height))
}
