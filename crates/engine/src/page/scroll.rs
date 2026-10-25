//! Scrolling of the page: the scroll queries and scrolls of the document
//! (`crate::document`), with the hover state that follows the content; the
//! scrolling by the user (the mouse wheel; the keys are in `input.rs`) and
//! scrolling to a fragment. Design: ADR 0019.

use swb_dom::NodeId;
use swb_layout::{Point, Rect, Size};

use super::Page;
use crate::document::ElementScroll;

impl Page {
    /// The size of the scrollable content in CSS px.
    pub fn content_size(&mut self) -> Size {
        let viewport = self.view.viewport;
        self.parts()
            .map_or(viewport, |(doc, fonts)| doc.content_size(fonts))
    }

    /// Scrolls by a delta in CSS px.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        let scroll = self.scroll_position();
        self.scroll_to(Point::new(scroll.x + dx, scroll.y + dy));
    }

    /// Scrolls to a position in CSS px, clamped to the content. Replaces a
    /// scroll to the fragment that waits for the page to load.
    pub fn scroll_to(&mut self, p: Point) {
        if let Some((doc, fonts)) = self.parts()
            && doc.scroll_to(p, fonts)
        {
            // Other content is under the pointer now.
            self.update_hover(false);
            self.note_scroll(None);
        }
    }

    /// The scroll state of element `node`. `None` if it has no box.
    pub fn element_scroll(&mut self, node: NodeId) -> Option<ElementScroll> {
        let (doc, fonts) = self.parts()?;
        doc.element_scroll(node, fonts)
    }

    /// Scrolls element `node` to `offset` (clamped to its scroll range),
    /// as the DOM's `scrollTo` does: also with `overflow: hidden`. The
    /// element that scrolls the page scrolls the viewport; other elements
    /// that are not scroll containers do not scroll. Returns the new
    /// offset, or `None` if the element has no box.
    pub fn scroll_element_to(&mut self, node: NodeId, offset: Point) -> Option<Point> {
        let (doc, fonts) = self.parts()?;
        let (offset, scrolled) = doc.scroll_element_to(node, offset, fonts);
        if scrolled {
            self.update_hover(false);
            self.note_scroll(Some(node));
        }
        offset
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
        let Some((doc, fonts)) = self.parts() else {
            return false;
        };
        doc.update_layout(fonts);
        let chain = match (start, &doc.styles) {
            (Some(start), Some(styles)) => doc.scrollers.chain(&doc.dom, styles, start, true),
            _ => Vec::new(),
        };
        for node in chain {
            let Some(port) = doc.scrollers.get(node).map(|b| b.port) else {
                continue;
            };
            let (dx, dy) = delta(port);
            if let Some(to) = doc.scrollers.user_target(node, dx, dy) {
                let scrolled = doc.set_element_scroll(node, to);
                if scrolled {
                    self.update_hover(false);
                    self.note_scroll(Some(node));
                }
                return scrolled;
            }
        }
        // The viewport, on the axes that the user can scroll.
        let (user_x, user_y) = doc.viewport_user_axes();
        let dx = if user_x { viewport.0 } else { 0.0 };
        let dy = if user_y { viewport.1 } else { 0.0 };
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let before = self.scroll_position();
        self.scroll_by(dx, dy);
        self.scroll_position() != before
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
    /// scrolled or a listener changed the page.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let (dx, dy) = (finite_or_zero(dx), finite_or_zero(dy));
        // The pointer is where the wheel turns (the hovered link and the
        // cursor follow it after the scroll).
        self.input.pointer = Some(Point::new(x, y));
        // With scripts, `wheel` goes first; a listener that cancels it
        // keeps the page from scrolling.
        let generation = self.dom_generation();
        if self.script_input() && !self.scripted_wheel(Point::new(x, y), dx, dy) {
            return generation != self.dom_generation();
        }
        let start = self.hit_test(x, y).map(|h| h.node);
        let scrolled = self.user_scroll(start, |_| (dx, dy), (dx, dy));
        // Listeners may have changed the page as well.
        scrolled || generation != self.dom_generation()
    }

    /// Scrolls so that the element is visible, as Chromium does for focus
    /// navigation: every scroll container that contains it, then the
    /// viewport, with "center if needed" on each axis (no scroll if it is
    /// visible, the nearest edge if it is partly visible, centered if it
    /// is not visible). Replaces a scroll to the fragment that waits for
    /// the page to load.
    pub fn scroll_into_view(&mut self, node: NodeId) {
        if let Some((doc, fonts)) = self.parts()
            && doc.scroll_into_view(node, fonts)
        {
            self.update_hover(false);
            self.note_scroll(None);
        }
    }

    /// The area that the scroll containers of `node` leave visible: the
    /// intersection of their scrollports, in document coordinates with the
    /// scroll offsets applied. `Some(None)` if they leave nothing visible;
    /// `None` if no scroll container contains the node (or there is no
    /// layout). The viewport is not part of it.
    pub fn scroll_clip(&mut self, node: NodeId) -> Option<Option<Rect>> {
        let (doc, fonts) = self.parts()?;
        doc.scroll_clip(node, fonts)
    }

    // ----- Fragments -----

    /// Scrolls to the element that the fragment indicates (HTML "scroll to
    /// the fragment"). Without a layout yet, scrolls after the first
    /// layout.
    pub(super) fn scroll_to_fragment(&mut self, fragment: &str) {
        if let Some((doc, fonts)) = self.parts()
            && doc.scroll_to_fragment(fragment, fonts)
        {
            self.update_hover(false);
            self.note_scroll(None);
        }
    }

    /// Makes the element that `fragment` indicates the target (`:target`)
    /// and returns it.
    pub(super) fn set_target(&mut self, fragment: Option<&str>) -> Option<NodeId> {
        let target = self
            .doc
            .as_ref()
            .and_then(|doc| crate::document::indicated(doc.dom(), fragment?));
        self.update_states(|s| s.target = target);
        target
    }
}

/// Replaces values that are not finite by 0.
fn finite_or_zero(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}
