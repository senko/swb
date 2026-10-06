//! Scrolling: the viewport and the scroll containers (programmatic
//! scrolling, the mouse wheel, scroll into view), scrolling to a fragment,
//! and the element that a URL fragment indicates (the scroll target of a
//! navigation and the `:target` element). Keyboard scrolling is in
//! `input.rs`. Design: ADR 0019.

use std::collections::HashMap;

use swb_dom::{Document, NodeId, QuirksMode, local_name};
use swb_layout::{FragmentRef, FragmentTree, Point, Rect, ScrollOffsets, Size, scroll_range};
use swb_style::Overflow;

use super::{Page, ScrollTarget};
use crate::scrollers::{Align, snap_scroll_offset};

/// The scroll state of an element, as the DOM's `scrollLeft`,
/// `scrollTop`, `scrollWidth`, `scrollHeight`, `clientWidth` and
/// `clientHeight` describe it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementScroll {
    /// The scroll offset.
    pub offset: Point,
    /// The size of the scrollable overflow rectangle (at least the size of
    /// the scrollport).
    pub scroll_size: Size,
    /// The size of the scrollport: the padding box (scrollbars take no
    /// space); the viewport for the element that scrolls the page and, as
    /// CSSOM View says, for the body in quirks mode.
    pub client_size: Size,
    /// True for a scroll container, and for the element that scrolls the
    /// page (the root element; in quirks mode the body if it is not a
    /// scroll container). False for other elements: their offset is zero
    /// and both sizes are the size of their padding box.
    pub scrollable: bool,
}

impl Page {
    /// The size of the scrollable content in CSS px.
    pub fn content_size(&mut self) -> Size {
        self.update_layout();
        self.fragments
            .as_ref()
            .map_or(self.viewport, |f| f.scroll_size)
    }

    /// Scrolls by a delta in CSS px.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.scroll_to(Point::new(self.scroll.x + dx, self.scroll.y + dy));
    }

    /// Scrolls to a position in CSS px, clamped to the content. Replaces a
    /// scroll to the fragment that waits for the page to load.
    pub fn scroll_to(&mut self, p: Point) {
        self.update_layout();
        self.pending_scroll = None;
        let before = self.scroll;
        self.scroll = p;
        self.clamp_scroll();
        if self.scroll != before {
            // Other content is under the pointer now.
            self.update_hover(false);
        }
    }

    /// Clamps the scroll position to the content. Without a layout the
    /// content size is not known; the position is then clamped after the
    /// next layout.
    pub(super) fn clamp_scroll(&mut self) {
        let max = match &self.fragments {
            Some(fragments) => scroll_range(fragments.scroll_size, self.viewport),
            None => Point::new(swb_style::Length::MAX_PX, swb_style::Length::MAX_PX),
        };
        self.scroll = snap_scroll_offset(self.scroll, max);
    }

    /// The axes on which the user can scroll the viewport (horizontal,
    /// vertical): not those where the viewport's overflow is `hidden` or
    /// `clip`. Scripts and scroll into view can scroll those too.
    pub(super) fn viewport_user_axes(&self) -> (bool, bool) {
        let user = |o: Overflow| !matches!(o, Overflow::Hidden | Overflow::Clip);
        self.fragments.as_ref().map_or((true, true), |f| {
            let (x, y) = f.viewport_overflow;
            (user(x), user(y))
        })
    }

    // ----- Scroll containers -----

    /// The scroll offsets of the scroll containers.
    pub(crate) fn scroll_offsets(&self) -> &dyn ScrollOffsets {
        self.scrollers.offsets()
    }

    /// Shows or hides the overlay scroll indicators of the scroll
    /// containers and the viewport (the GUI shows them; screenshots for
    /// comparisons with Chromium do not).
    pub fn set_scroll_indicators(&mut self, show: bool) {
        if self.scroll_indicators != show {
            self.scroll_indicators = show;
            self.display_list = None;
        }
    }

    /// True if `node` is the element whose scroll position is the
    /// viewport's (`document.scrollingElement`,
    /// <https://drafts.csswg.org/cssom-view/#dom-document-scrollingelement>):
    /// the root element, or in quirks mode the body if it is not a scroll
    /// container. (In quirks mode, the root element then is an element that
    /// does not scroll.)
    fn is_scrolling_element(&self, node: NodeId) -> bool {
        let Some(doc) = &self.document else {
            return false;
        };
        if doc.quirks_mode == QuirksMode::Quirks {
            doc.body() == Some(node) && self.scrollers.get(node).is_none()
        } else {
            doc.document_element() == Some(node)
        }
    }

    /// The scroll state of element `node`. `None` if it has no box.
    pub fn element_scroll(&mut self, node: NodeId) -> Option<ElementScroll> {
        self.update_layout();
        if self.is_scrolling_element(node) {
            let content = self.content_size();
            return Some(ElementScroll {
                offset: self.scroll,
                scroll_size: content,
                client_size: self.viewport,
                scrollable: true,
            });
        }
        if let Some(b) = self.scrollers.get(node) {
            // CSSOM View: `clientWidth` and `clientHeight` of the body in
            // quirks mode are the viewport's.
            let quirks_body = self
                .document
                .as_ref()
                .is_some_and(|d| d.quirks_mode == QuirksMode::Quirks && d.body() == Some(node));
            return Some(ElementScroll {
                offset: self.scrollers.offset(node),
                scroll_size: b.scroll_size,
                client_size: if quirks_body { self.viewport } else { b.port },
                scrollable: true,
            });
        }
        let tree = self.fragments.as_ref()?;
        let mut padding = None;
        tree.walk(|fragment, _| {
            if padding.is_none()
                && let FragmentRef::Box(b) = fragment
                && b.node == Some(node)
                && b.pseudo.is_none()
            {
                padding = Some(b.padding_rect());
            }
        });
        let size = padding.map(|r| Size::new(r.width, r.height))?;
        Some(ElementScroll {
            offset: Point::default(),
            scroll_size: size,
            client_size: size,
            scrollable: false,
        })
    }

    /// Scrolls element `node` to `offset` (clamped to its scroll range),
    /// as the DOM's `scrollTo` does: also with `overflow: hidden`. The
    /// element that scrolls the page (see `is_scrolling_element`) scrolls
    /// the viewport; other elements that are not scroll containers do not
    /// scroll. Returns the new offset, or `None` if the element has no box.
    pub fn scroll_element_to(&mut self, node: NodeId, offset: Point) -> Option<Point> {
        self.update_layout();
        if self.is_scrolling_element(node) {
            self.scroll_to(offset);
            return Some(self.scroll);
        }
        if self.scrollers.get(node).is_none() {
            return self.element_scroll(node).map(|s| s.offset);
        }
        self.set_element_scroll(node, offset);
        Some(self.scrollers.offset(node))
    }

    /// Sets the scroll offset of a scroll container (by the user or a
    /// script; it replaces a scroll to the fragment that waits for the
    /// page to load, as `scroll_to` does). Returns true if it changed; then
    /// the display list is built again (the layout stays) and the hovered
    /// link follows the content.
    pub(super) fn set_element_scroll(&mut self, node: NodeId, offset: Point) -> bool {
        self.pending_scroll = None;
        if !self.scrollers.set(node, offset) {
            return false;
        }
        self.display_list = None;
        self.update_hover(false);
        true
    }

    /// Scrolls by the user (wheel or keys): the first scroll container in
    /// the scroll chain of `start` (itself included) that the user can
    /// scroll in the direction of the delta, else the viewport. `delta`
    /// gives the delta for a scroll container from the size of its
    /// scrollport; `viewport` is the delta for the viewport. Returns true
    /// if something scrolled.
    pub(super) fn user_scroll(
        &mut self,
        start: Option<NodeId>,
        delta: impl Fn(Size) -> (f32, f32),
        viewport: (f32, f32),
    ) -> bool {
        self.update_layout();
        let chain = match (start, &self.document, &self.styles) {
            (Some(start), Some(doc), Some(styles)) => {
                self.scrollers.chain(doc, styles, start, true)
            }
            _ => Vec::new(),
        };
        for node in chain {
            let Some(port) = self.scrollers.get(node).map(|b| b.port) else {
                continue;
            };
            let (dx, dy) = delta(port);
            if let Some(to) = self.scrollers.user_target(node, dx, dy) {
                return self.set_element_scroll(node, to);
            }
        }
        // The viewport, on the axes that the user can scroll.
        let (user_x, user_y) = self.viewport_user_axes();
        let dx = if user_x { viewport.0 } else { 0.0 };
        let dy = if user_y { viewport.1 } else { 0.0 };
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let before = self.scroll;
        self.scroll_by(dx, dy);
        self.scroll != before
    }

    /// Handles the mouse wheel outside the page area (over the browser's
    /// toolbar): scrolls the viewport by (`dx`, `dy`) CSS px on the axes
    /// that the user can scroll. Returns true if it scrolled.
    pub fn wheel_page(&mut self, dx: f32, dy: f32) -> bool {
        let delta = (finite_or_zero(dx), finite_or_zero(dy));
        self.user_scroll(None, |_| (0.0, 0.0), delta)
    }

    /// Handles the mouse wheel at a point in viewport coordinates: scrolls
    /// by (`dx`, `dy`) CSS px the innermost scroll container under the
    /// pointer that can scroll in that direction, else the next one in its
    /// scroll chain, else the viewport (as Chromium chains scrolling;
    /// deviation: each event picks the scroll container again, Chromium
    /// keeps it for a whole wheel gesture). Returns true if something
    /// scrolled.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let (dx, dy) = (finite_or_zero(dx), finite_or_zero(dy));
        // The pointer is where the wheel turns (the hovered link and the
        // cursor follow it after the scroll).
        self.input.pointer = Some(Point::new(x, y));
        let start = self.hit_test(x, y).map(|h| h.node);
        self.user_scroll(start, |_| (dx, dy), (dx, dy))
    }

    /// Scrolls so that the element is visible, as Chromium does for focus
    /// navigation: every scroll container that contains it, then the
    /// viewport, with "center if needed" on each axis (no scroll if it is
    /// visible, the nearest edge if it is partly visible, centered if it
    /// is not visible). Replaces a scroll to the fragment that waits for
    /// the page to load.
    pub fn scroll_into_view(&mut self, node: NodeId) {
        self.update_layout();
        let Some(rect) = self.element_box(node) else {
            return;
        };
        self.pending_scroll = None;
        // Blink does not scroll horizontally if 32 px of the box are visible.
        let changed = self.reveal(
            node,
            rect,
            Align::CenterIfNeeded(Some(32.0)),
            Align::CenterIfNeeded(None),
        );
        if changed {
            self.update_hover(false);
        }
    }

    /// Scrolls `rect` (the box of `node` in document coordinates, with the
    /// scroll offsets applied) into view: in each scroll container of the
    /// node's scroll chain from the innermost, then in the viewport. After
    /// each scroll container, the part of the box inside its scrollport is
    /// revealed in the next one (as in Blink). Scrolls `overflow: hidden`
    /// containers too. Does not cancel a pending scroll to the fragment and
    /// does not update the hover state. Returns true if something
    /// scrolled.
    fn reveal(&mut self, node: NodeId, rect: Rect, x: Align, y: Align) -> bool {
        let (Some(doc), Some(styles), Some(tree)) = (&self.document, &self.styles, &self.fragments)
        else {
            return false;
        };
        let chain = self.scrollers.chain(doc, styles, node, false);
        let ports = self.scrollers.ports(tree, &chain);
        let mut rect = rect;
        let mut changed = false;
        for scroller in chain {
            let Some(&port) = ports.get(&scroller) else {
                continue;
            };
            let offset = self.scrollers.offset(scroller);
            // In the coordinates of the scrolled content.
            let start_x = rect.x - port.x + offset.x;
            let start_y = rect.y - port.y + offset.y;
            let to = Point::new(
                x.position(start_x, rect.width, offset.x, port.width),
                y.position(start_y, rect.height, offset.y, port.height),
            );
            if self.scrollers.set(scroller, to) {
                changed = true;
                self.display_list = None;
                let new = self.scrollers.offset(scroller);
                rect = rect.translate(Point::new(offset.x - new.x, offset.y - new.y));
            }
            rect = rect.intersection(&port).unwrap_or(rect);
        }
        let before = self.scroll;
        let (scroll, viewport) = (self.scroll, self.viewport);
        self.scroll = Point::new(
            x.position(rect.x, rect.width, scroll.x, viewport.width),
            y.position(rect.y, rect.height, scroll.y, viewport.height),
        );
        self.clamp_scroll();
        changed || self.scroll != before
    }

    /// The area that the scroll containers of `node` leave visible: the
    /// intersection of their scrollports, in document coordinates with the
    /// scroll offsets applied. `Some(None)` if they leave nothing visible;
    /// `None` if no scroll container contains the node (or there is no
    /// layout). The viewport is not part of it.
    pub fn scroll_clip(&mut self, node: NodeId) -> Option<Option<Rect>> {
        self.update_layout();
        let (doc, styles, tree) = (
            self.document.as_ref()?,
            self.styles.as_ref()?,
            self.fragments.as_ref()?,
        );
        let chain = self.scrollers.chain(doc, styles, node, false);
        let ports = self.scrollers.ports(tree, &chain);
        let mut clips = chain.iter().filter_map(|scroller| ports.get(scroller));
        let first = *clips.next()?;
        Some(clips.try_fold(first, |clip, port| clip.intersection(port)))
    }

    // ----- Fragments -----

    /// Scrolls to the element that the fragment indicates (HTML "scroll to
    /// the fragment"): the element with that ID or an `<a>` with that name,
    /// or the top of the document for an empty fragment or `top`. Without a
    /// layout yet, scrolls after the first layout.
    pub(super) fn scroll_to_fragment(&mut self, fragment: &str) {
        self.update_layout();
        if self.fragments.is_none() {
            self.pending_scroll = Some(ScrollTarget::Fragment(fragment.to_owned()));
            return;
        }
        self.pending_scroll = None;
        if self.reveal_fragment(fragment) {
            self.update_hover(false);
        }
    }

    /// Makes the element that `fragment` indicates the target (`:target`)
    /// and returns it.
    pub(super) fn set_target(&mut self, fragment: Option<&str>) -> Option<NodeId> {
        let target = self
            .document
            .as_ref()
            .and_then(|doc| indicated(doc, fragment?));
        self.update_states(|s| s.target = target);
        target
    }

    /// Scrolls to what a fragment indicates, if anything
    /// (<https://html.spec.whatwg.org/multipage/browsing-the-web.html#the-indicated-part-of-the-document>):
    /// the target element at the top of each scroll container and of the
    /// viewport (CSSOM View `scrollIntoView` with `block: "start"` and
    /// `inline: "nearest"`), or the top of the document. Returns true if
    /// something scrolled.
    pub(super) fn reveal_fragment(&mut self, fragment: &str) -> bool {
        let (Some(doc), Some(tree)) = (&self.document, &self.fragments) else {
            return false;
        };
        let decoded = percent_decode(fragment);
        match indicated(doc, fragment) {
            Some(target) => {
                let Some((node, rect)) = first_rendered(doc, tree, self.scroll_offsets(), target)
                else {
                    return false;
                };
                self.reveal(node, rect, Align::Nearest, Align::Start)
            }
            None if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") => {
                let before = self.scroll;
                self.scroll = Point::default();
                self.scroll != before
            }
            None => false,
        }
    }
}

/// Replaces values that are not finite by 0.
fn finite_or_zero(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// The element that a fragment indicates: by its text, then percent-decoded.
pub(super) fn indicated(doc: &Document, fragment: &str) -> Option<NodeId> {
    indicated_element(doc, fragment).or_else(|| indicated_element(doc, &percent_decode(fragment)))
}

/// The element with the ID `name`, or the first `<a>` with that name.
fn indicated_element(doc: &Document, name: &str) -> Option<NodeId> {
    if name.is_empty() {
        return None;
    }
    doc.element_by_id(name).or_else(|| {
        doc.find_element(NodeId::DOCUMENT, |e| {
            e.is_html_named(&local_name!("a")) && e.attr("name") == Some(name)
        })
    })
}

/// The first fragment of `node` (document coordinates, scroll offsets
/// applied) and the node it belongs to. A node without fragments (for
/// example an empty `<a name>`) uses the first fragment that follows it in
/// tree order.
fn first_rendered(
    doc: &Document,
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    node: NodeId,
) -> Option<(NodeId, Rect)> {
    let mut first: HashMap<NodeId, Rect> = HashMap::new();
    tree.walk_scrolled(offsets, |fragment, origin| {
        let (owner, rect) = match fragment {
            FragmentRef::Box(b) if b.pseudo.is_none() => match b.node {
                Some(owner) => (owner, b.border_rect.translate(origin)),
                None => return,
            },
            FragmentRef::Text(t) => (t.node, t.rect.translate(origin)),
            FragmentRef::Box(_) => return,
        };
        first.entry(owner).or_insert(rect);
    });
    doc.descendants(NodeId::DOCUMENT)
        .skip_while(|&n| n != node)
        .find_map(|n| first.get(&n).map(|r| (n, *r)))
}

/// Percent-decodes `s` (<https://url.spec.whatwg.org/#percent-decode>) and
/// decodes the result as UTF-8, with replacement characters for invalid
/// sequences.
fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&swb_net::percent_decode(s.as_bytes())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%C4%8D"), "č");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
