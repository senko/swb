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
    BoxContent, BoxFragment, Fragment, FragmentRef, Matrix, Point, Rect, Size, scrolling_area,
};
use swb_style::Position;
use swb_style::cssom::resolved::layout_unit;
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
fn map_rect(matrix: &Matrix, rect: Rect) -> Rect {
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
            let root = self.fragments.as_ref()?.root.as_ref()?;
            self.geometry = Some((serial, index_geometry(root)));
        }
        self.geometry.as_ref()?.1.get(&node)
    }

    /// The client rectangles of `node` in document coordinates (after a
    /// flush).
    fn painted_rects(&mut self, node: NodeId) -> &[Rect] {
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
            });
        }
        self.client_rects
            .as_ref()
            .and_then(|i| i.rects.get(&node))
            .map_or(&[], Vec::as_slice)
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
        let parent = if fixed || self.dom.document_element() == Some(node) {
            None
        } else {
            self.offset_parent(node)
        };
        // Relative to the padding edge of the offset parent; for the body,
        // to the border box of the root element (Chromium 148: with `html
        // { margin: 5px }` a box at x 23 has `offsetLeft` 18); for none, to
        // the initial containing block.
        let origin = match parent {
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

    /// The offset parent of `node` (CSSOM View §7 "offsetParent"): the
    /// nearest ancestor that is the containing block of absolutely
    /// positioned boxes (positioned or transformed; Chromium 148 also
    /// takes a transformed ancestor, as the specification now says), the
    /// body, or, for an element that is not positioned, a `td`, `th` or
    /// `table`. The caller handles the root, the body and fixed boxes.
    fn offset_parent(&self, node: NodeId) -> Option<NodeId> {
        let styles = self.styles.as_ref()?;
        let body = self.dom.body();
        let own_static = styles
            .get(node)
            .is_none_or(|s| s.position == Position::Static);
        for ancestor in self.dom.ancestors(node) {
            let element = self.dom.element(ancestor)?;
            if body == Some(ancestor) {
                return Some(ancestor);
            }
            if styles
                .get(ancestor)
                .is_some_and(|s| s.position != Position::Static || s.has_transform())
            {
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
            let element = if self.dom.element(hit).is_some() {
                Some(hit)
            } else {
                self.dom
                    .parent(hit)
                    .filter(|&p| self.dom.element(p).is_some())
            };
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
            transformed: style.has_transform(),
            clips: style.overflow_x.clips() || style.overflow_y.clips(),
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
            .chain(self.dom.ancestors(target))
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

/// The boxes of the elements of the layout whose root fragment is `root`.
fn index_geometry(root: &BoxFragment) -> GeometryIndex {
    let mut index = GeometryIndex::new();
    add_box(root, Point::default(), None, &mut index);
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

/// `rect` within the range of layout lengths.
fn clamp(rect: Rect) -> Rect {
    let c = swb_style::Length::clamp_px;
    Rect::new(c(rect.x), c(rect.y), c(rect.width), c(rect.height))
}
