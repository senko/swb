//! Scroll containers of a page (CSS Overflow 3 §3,
//! <https://www.w3.org/TR/css-overflow-3/#scroll-container>): their scroll
//! offsets, kept across layouts and keyed by the element; their scroll
//! ranges from the last layout; the scroll chain of a node; and the
//! alignment of a box in a scrollport for "scroll into view". The viewport
//! is scrolled by `Page` (`page/scroll.rs`). Design: ADR 0019.

use std::collections::{HashMap, HashSet};

use swb_dom::{Document, NodeId};
use swb_layout::{FragmentRef, FragmentTree, Point, Rect, Size, clamp_scroll_offset};
use swb_style::{Display, Overflow, Position, StyleMap};

/// One scroll container, from the last layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollBox {
    /// The largest scroll offset on each axis.
    pub(crate) max: Point,
    /// The size of the scrollport (the padding box).
    pub(crate) port: Size,
    /// The size of the scrollable overflow rectangle (at least the
    /// scrollport).
    pub(crate) scroll_size: Size,
    /// True if the user can scroll horizontally (`overflow-x` is not
    /// `hidden`; `hidden` scrolls only programmatically).
    pub(crate) user_x: bool,
    /// True if the user can scroll vertically.
    pub(crate) user_y: bool,
}

/// The scroll containers of a document and their scroll offsets.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scrollers {
    /// Scroll offsets that are not zero, clamped to the ranges of the last
    /// layout.
    offsets: HashMap<NodeId, Point>,
    /// The scroll containers of the last layout.
    boxes: HashMap<NodeId, ScrollBox>,
}

impl Scrollers {
    /// The scroll offsets, for layout walks and paint.
    pub(crate) fn offsets(&self) -> &HashMap<NodeId, Point> {
        &self.offsets
    }

    /// The scroll container of element `node`, if it is one.
    pub(crate) fn get(&self, node: NodeId) -> Option<&ScrollBox> {
        self.boxes.get(&node)
    }

    /// The scroll offset of `node` (zero if it is not scrolled).
    pub(crate) fn offset(&self, node: NodeId) -> Point {
        self.offsets.get(&node).copied().unwrap_or_default()
    }

    /// Takes the scroll containers of a new layout. The offsets of elements
    /// that are no longer scroll containers are dropped; the others are
    /// clamped to the new ranges.
    pub(crate) fn update(&mut self, tree: &FragmentTree) {
        self.boxes.clear();
        tree.walk(|fragment, _| {
            if let FragmentRef::Box(b) = fragment
                && let (Some(node), Some(overflow)) = (b.scroll_node(), b.scrollable_overflow)
            {
                let port = b.scrollport();
                self.boxes.insert(
                    node,
                    ScrollBox {
                        max: b.max_scroll_offset(),
                        port: Size::new(port.width, port.height),
                        scroll_size: Size::new(overflow.width, overflow.height),
                        user_x: b.style.overflow_x != Overflow::Hidden,
                        user_y: b.style.overflow_y != Overflow::Hidden,
                    },
                );
            }
        });
        let boxes = &self.boxes;
        self.offsets.retain(|node, offset| match boxes.get(node) {
            Some(b) => {
                *offset = snap_scroll_offset(*offset, b.max);
                *offset != Point::default()
            }
            None => false,
        });
    }

    /// Sets the scroll offset of scroll container `node`, clamped to its
    /// range. Returns true if it changed. Does nothing for other nodes.
    pub(crate) fn set(&mut self, node: NodeId, offset: Point) -> bool {
        let Some(b) = self.boxes.get(&node) else {
            return false;
        };
        let new = snap_scroll_offset(offset, b.max);
        let old = self.offset(node);
        if new == old {
            return false;
        }
        if new == Point::default() {
            self.offsets.remove(&node);
        } else {
            self.offsets.insert(node, new);
        }
        true
    }

    /// The offset that the user scrolls `node` to with a delta of (`dx`,
    /// `dy`): the axes with `overflow: hidden` do not move. `None` if no
    /// axis that the user can scroll can move in the direction of the
    /// delta (the offset is at the end of the range), or if `node` is not
    /// a scroll container.
    pub(crate) fn user_target(&self, node: NodeId, dx: f32, dy: f32) -> Option<Point> {
        let b = self.boxes.get(&node)?;
        let offset = self.offset(node);
        let can = |delta: f32, offset: f32, max: f32| {
            (delta > 0.0 && offset < max) || (delta < 0.0 && offset > 0.0)
        };
        let x = b.user_x && can(dx, offset.x, b.max.x);
        let y = b.user_y && can(dy, offset.y, b.max.y);
        (x || y).then(|| {
            let dx = if b.user_x { dx } else { 0.0 };
            let dy = if b.user_y { dy } else { 0.0 };
            Point::new(offset.x + dx, offset.y + dy)
        })
    }

    /// The scroll containers whose offsets move `node` (an element or a
    /// text node), innermost first: its ancestors in the chain of
    /// containing blocks (an absolutely positioned element skips the
    /// ancestors below its nearest positioned ancestor; nothing moves a
    /// fixed element), and with `include_self` the element `node` itself.
    /// For a text node, the chain starts at its parent element. This is the
    /// scroll chain of wheel and keyboard scrolling and the list of boxes
    /// that "scroll into view" scrolls. The same rule as
    /// `swb_layout::ScrollState`. At most the DOM depth long.
    pub(crate) fn chain(
        &self,
        doc: &Document,
        styles: &StyleMap,
        node: NodeId,
        include_self: bool,
    ) -> Vec<NodeId> {
        let start = if doc.get(node).is_some_and(swb_dom::Node::is_element) {
            Some(node)
        } else {
            doc.parent_element(node)
        };
        let mut chain = Vec::new();
        let mut skip_to_positioned = false;
        // True while `element` is `node` itself.
        let mut is_self = start == Some(node);
        let mut current = start;
        while let Some(element) = current {
            let position = styles
                .get(element)
                .filter(|s| s.display != Display::Contents && s.display != Display::None)
                .map_or(Position::Static, |s| s.position);
            if !is_self && position != Position::Static {
                skip_to_positioned = false;
            }
            if (include_self || !is_self)
                && !skip_to_positioned
                && self.boxes.contains_key(&element)
            {
                chain.push(element);
            }
            match position {
                Position::Fixed => break,
                Position::Absolute => skip_to_positioned = true,
                _ => {}
            }
            is_self = false;
            current = doc.parent_element(element);
        }
        chain
    }

    /// The scrollports (padding boxes) of the scroll containers in
    /// `nodes`, in document coordinates with the scroll offsets applied.
    pub(crate) fn ports(&self, tree: &FragmentTree, nodes: &[NodeId]) -> HashMap<NodeId, Rect> {
        let mut ports = HashMap::new();
        if nodes.is_empty() {
            return ports;
        }
        let wanted: HashSet<NodeId> = nodes.iter().copied().collect();
        tree.walk_scrolled(&self.offsets, |fragment, origin| {
            if let FragmentRef::Box(b) = fragment
                && let Some(node) = b.scroll_node()
                && wanted.contains(&node)
            {
                ports.insert(node, b.padding_rect().translate(origin));
            }
        });
        ports
    }
}

/// A scroll offset in whole CSS px (halves round up, as in Chromium 148,
/// which keeps integer scroll offsets also at higher device pixel ratios),
/// clamped to `0..=max` (NaN becomes 0). `max` is a whole number
/// (`swb_layout::scroll_range`), so the result is too.
pub(crate) fn snap_scroll_offset(offset: Point, max: Point) -> Point {
    clamp_scroll_offset(Point::new(offset.x.round(), offset.y.round()), max)
}

/// The page step of Page Up, Page Down and Space for a scrollport of
/// `length` px, as Chromium computes it (`ScrollableArea::PageStep`):
/// 87.5 % of the length in whole pixels, truncated, at least 1.
pub(crate) fn page_step(length: f32) -> f32 {
    (length.round() * 0.875).trunc().max(1.0)
}

/// How "scroll into view" aligns a box in a scrollport on one axis
/// (CSSOM View, <https://drafts.csswg.org/cssom-view/#scroll-a-target-into-view>).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Align {
    /// The start edges together (`"start"`; the block axis of fragment
    /// navigation).
    Start,
    /// No scroll if the box is visible, else the nearest edges together
    /// (`"nearest"`).
    Nearest,
    /// Chromium's "center if needed" for focus navigation: no scroll if
    /// the box is visible (or, with a value, if that many px of it are
    /// visible) or covers the view; the nearest edge if it is partly
    /// visible; centered if it is not visible.
    CenterIfNeeded(Option<f32>),
}

impl Align {
    /// The scroll position on one axis that shows `start..start + size` in
    /// a view of `view` px scrolled to `scroll` (all in the coordinates of
    /// the scrolled content). Not clamped to the scroll range.
    pub(crate) fn position(self, start: f32, size: f32, scroll: f32, view: f32) -> f32 {
        match self {
            Align::Start => start,
            Align::Nearest => nearest(start, size, scroll, view),
            Align::CenterIfNeeded(min_reveal) => {
                center_if_needed(start, size, scroll, view, min_reveal)
            }
        }
    }
}

/// The `"nearest"` alignment of CSSOM View: no scroll if the box is inside
/// the view or covers it; otherwise its start edge at the view's start if
/// it is before the view and smaller than it (or after the view and
/// larger), else its end edge at the view's end.
fn nearest(start: f32, size: f32, scroll: f32, view: f32) -> f32 {
    let (end, view_end) = (start + size, scroll + view);
    if (start >= scroll && end <= view_end) || (start <= scroll && end >= view_end) {
        scroll
    } else if (start < scroll && size < view) || (end > view_end && size > view) {
        start
    } else {
        end - view
    }
}

/// Chromium's "center if needed" alignment (`ScrollAlignment` in Blink):
/// no scroll if the box is visible (or, with `min_reveal`, if that much of
/// it is visible) or covers the view; the nearest edge if it is partly
/// visible; centered if it is not visible.
fn center_if_needed(start: f32, size: f32, scroll: f32, view: f32, min_reveal: Option<f32>) -> f32 {
    let (end, view_end) = (start + size, scroll + view);
    let visible = (end.min(view_end) - start.max(scroll)).max(0.0);
    if visible >= size || min_reveal.is_some_and(|m| visible >= m) || visible >= view {
        scroll
    } else if visible > 0.0 {
        // The nearest edge is the end if the box is after the view and
        // smaller than it, or before the view and larger than it.
        let align_end = (end > view_end && size < view) || (end < view_end && size > view);
        if align_end { end - view } else { start }
    } else {
        start + size / 2.0 - view / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_if_needed_alignment() {
        let y = |start, size, scroll| center_if_needed(start, size, scroll, 600.0, None);
        // Visible: no scroll.
        assert_eq!(y(100.0, 50.0, 0.0), 0.0);
        // Below the view: centered.
        assert_eq!(y(1000.0, 50.0, 0.0), 725.0);
        // Partly below: the bottom edge at the bottom of the view.
        assert_eq!(y(580.0, 50.0, 0.0), 30.0);
        // Partly above: the top edge at the top of the view.
        assert_eq!(y(80.0, 50.0, 100.0), 80.0);
        // Larger than the view and covering it: no scroll.
        assert_eq!(y(500.0, 2000.0, 1000.0), 1000.0);
        // Larger than the view, partly below: the top edge at the top.
        assert_eq!(y(300.0, 2000.0, 0.0), 300.0);
        // Horizontally, 32 visible px are enough.
        assert_eq!(center_if_needed(760.0, 100.0, 0.0, 800.0, Some(32.0)), 0.0);
        assert_eq!(center_if_needed(780.0, 100.0, 0.0, 800.0, Some(32.0)), 80.0);
    }

    #[test]
    fn nearest_alignment() {
        let x = |start, size, scroll| nearest(start, size, scroll, 100.0);
        assert_eq!(x(20.0, 50.0, 0.0), 0.0);
        // After the view: the end edges together.
        assert_eq!(x(150.0, 50.0, 0.0), 100.0);
        // Before the view: the start edges together.
        assert_eq!(x(20.0, 50.0, 60.0), 20.0);
        // Larger than the view and after it: the start edges together.
        assert_eq!(x(50.0, 300.0, 0.0), 50.0);
        // Covering the view: no scroll.
        assert_eq!(x(0.0, 300.0, 50.0), 50.0);
        assert_eq!(Align::Start.position(70.0, 10.0, 0.0, 100.0), 70.0);
    }

    #[test]
    fn offsets_are_clamped_and_zero_offsets_are_not_stored() {
        let node = NodeId::from_index(5).expect("a node ID");
        let mut s = Scrollers::default();
        s.boxes.insert(
            node,
            ScrollBox {
                max: Point::new(0.0, 100.0),
                port: Size::new(100.0, 100.0),
                scroll_size: Size::new(100.0, 200.0),
                user_x: true,
                user_y: true,
            },
        );
        assert!(s.set(node, Point::new(50.0, 500.0)));
        assert_eq!(s.offset(node), Point::new(0.0, 100.0));
        assert!(!s.set(node, Point::new(0.0, 100.0)));
        assert!(s.set(node, Point::new(f32::NAN, -5.0)));
        assert!(s.offsets.is_empty());
        assert_eq!(s.user_target(node, 0.0, 10.0), Some(Point::new(0.0, 10.0)));
        assert_eq!(s.user_target(node, 0.0, -10.0), None);
        assert_eq!(s.user_target(node, 10.0, 0.0), None);
        let other = NodeId::from_index(6).expect("a node ID");
        assert!(!s.set(other, Point::new(1.0, 1.0)));
        assert_eq!(s.user_target(other, 0.0, 10.0), None);
    }

    #[test]
    fn hidden_axes_do_not_move_with_the_user() {
        let node = NodeId::from_index(5).expect("a node ID");
        let mut s = Scrollers::default();
        s.boxes.insert(
            node,
            ScrollBox {
                max: Point::new(100.0, 100.0),
                port: Size::new(100.0, 100.0),
                scroll_size: Size::new(200.0, 200.0),
                user_x: false,
                user_y: true,
            },
        );
        assert_eq!(s.user_target(node, 30.0, 30.0), Some(Point::new(0.0, 30.0)));
        assert_eq!(s.user_target(node, 30.0, 0.0), None);
    }

    /// A document laid out with the test fonts (inline styles only), and
    /// its scroll containers.
    fn laid_out(html: &str) -> (Document, StyleMap, FragmentTree, Scrollers) {
        let doc = swb_dom::parse_html(html);
        let stylist = swb_style::Stylist::new(doc.quirks_mode);
        let base = swb_net::Url::parse("about:blank").expect("a valid URL");
        let styles = swb_style::compute_styles(
            &doc,
            &stylist,
            &swb_css::MediaEnvironment::default(),
            &swb_style::ElementStates::default(),
            &base,
        );
        let input = swb_layout::LayoutInput {
            document: &doc,
            styles: &styles,
            viewport: Size::new(800.0, 600.0),
            replaced: &swb_layout::NoReplacedSizes,
            controls: &swb_layout::NoFormControls,
        };
        let tree = swb_layout::layout(&input, &mut swb_text::FontContext::for_tests());
        let mut scrollers = Scrollers::default();
        scrollers.update(&tree);
        (doc, styles, tree, scrollers)
    }

    /// Scroll containers in scroll containers, with absolutely positioned
    /// and fixed boxes whose containing blocks are inside and outside
    /// them. Every scroll container has a range.
    const NESTED: &str = "<!DOCTYPE html><body style='margin:0'>\
        <div id=s1 style='overflow:auto;height:50px'><div id=a style='height:100px'>\
        <div id=abs style='position:absolute;width:5px;height:5px'></div>\
        <div id=rel style='position:relative;height:30px'>\
        <div id=s2 style='overflow:auto;height:20px'><div id=tall style='height:100px'></div>\
        <div id=abs2 style='position:absolute;width:5px;height:5px'></div>text</div></div>\
        <div id=fix style='position:fixed;width:5px;height:5px'>\
        <div id=infix style='height:5px'></div></div>\
        <div id=s3 style='position:relative;overflow:auto;height:20px'>\
        <div style='height:100px'></div>\
        <div id=abs3 style='position:absolute;width:5px;height:5px'></div></div>\
        <span id=pi style='position:relative'><div id=card style='height:10px'></div>\
        <div id=badge style='position:absolute;width:5px;height:5px'></div>\
        <span style='display:block'><div id=badge2 style='position:absolute;width:5px;\
        height:5px'></div></span>\
        <div style='float:left;width:5px;height:5px'><div id=badge3 style='position:absolute;\
        width:5px;height:5px'></div></div></span></div></div>";

    #[test]
    fn the_scroll_chain_follows_the_containing_blocks() {
        let (doc, styles, _, scrollers) = laid_out(NESTED);
        let id = |id: &str| doc.element_by_id(id).expect("an element");
        let chain =
            |node: NodeId, include_self: bool| scrollers.chain(&doc, &styles, node, include_self);
        let (s1, s2) = (id("s1"), id("s2"));
        assert_eq!(chain(id("tall"), false), vec![s2, s1]);
        assert_eq!(chain(id("abs"), false), Vec::<NodeId>::new());
        assert_eq!(chain(id("abs2"), false), vec![s1]);
        assert_eq!(chain(id("fix"), false), Vec::<NodeId>::new());
        assert_eq!(chain(id("infix"), false), Vec::<NodeId>::new());
        assert_eq!(chain(s2, false), vec![s1]);
        assert_eq!(chain(s2, true), vec![s2, s1]);
        // A positioned scroll container contains its absolutely positioned
        // children.
        assert_eq!(chain(id("abs3"), false), vec![id("s3"), s1]);
        // A positioned inline box around blocks contains the absolutely
        // positioned boxes in it, also in a nested block.
        assert_eq!(chain(id("badge"), false), vec![s1]);
        assert_eq!(chain(id("badge2"), false), vec![s1]);
        assert_eq!(chain(id("badge3"), false), vec![s1]);
        // A text node starts at its parent element, which moves it.
        let text = doc
            .children(s2)
            .find(|&n| doc.get(n).is_some_and(|n| n.as_text().is_some()))
            .expect("a text node");
        assert_eq!(chain(text, false), vec![s2, s1]);
    }

    #[test]
    fn the_scroll_chain_agrees_with_the_scroll_state() {
        let (doc, styles, tree, scrollers) = laid_out(NESTED);
        let plain = tree.element_boxes();
        for &scroller in scrollers.boxes.keys() {
            let offsets = HashMap::from([(scroller, Point::new(0.0, 1.0))]);
            let scrolled = tree.element_boxes_scrolled(&offsets);
            for (node, rect) in &plain {
                let moved = scrolled[node].y != rect.y;
                let in_chain = scrollers
                    .chain(&doc, &styles, *node, false)
                    .contains(&scroller);
                let name = |n: NodeId| doc.element(n).and_then(|e| e.attr("id")).map(str::to_owned);
                assert_eq!(
                    moved,
                    in_chain,
                    "{node:?} ({:?}) and the scroller {:?}",
                    name(*node),
                    name(scroller)
                );
            }
        }
    }
}
