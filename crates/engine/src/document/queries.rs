//! Queries of a document that need its layout (a layout flush first):
//! element geometry, the scroll state of the viewport and of scroll
//! containers and the scrolls that scripts can ask for (scroll to a
//! position, scroll into view, scroll to a fragment), hit testing,
//! rendered text and the selection, and selector queries (no flush).
//! Design of scrolling: ADR 0019. The scrolls of the user (wheel, keys)
//! are in `crate::page`.

use std::collections::HashMap;

use swb_dom::{Document, NodeId, QuirksMode, local_name};
use swb_layout::{FragmentRef, FragmentTree, Point, Rect, ScrollOffsets, Size, scroll_range};
use swb_style::{Overflow, StyleMap, Visibility};
use swb_text::FontContext;

use super::DocumentState;
use crate::focus;
use crate::forms::{ControlType, InputType};
use crate::hit_test::{self, HitResult};
use crate::scrollers::{Align, snap_scroll_offset};
use crate::selection::{self, Selection, Snap, TextPosition};

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

/// True if the element has a box, is visible and is not in the content of
/// a closed `details`.
pub(crate) fn is_rendered_in(
    doc: &Document,
    tree: &FragmentTree,
    styles: &StyleMap,
    node: NodeId,
) -> bool {
    tree.element_boxes().contains_key(&node)
        && !doc.in_closed_details_content(node)
        && styles
            .get(node)
            .is_some_and(|s| s.visibility == Visibility::Visible)
}

impl DocumentState {
    // ----- Geometry -----

    /// The union of the border boxes of an element as painted, in document
    /// coordinates (CSS px), if it has a box: with the scroll offsets of
    /// scroll containers, and with transforms and fixed and sticky
    /// positioning at the current scroll position (as
    /// `getBoundingClientRect()` plus the scroll offset).
    pub fn element_box(&mut self, node: NodeId, fonts: &mut FontContext) -> Option<Rect> {
        self.update_layout(fonts);
        self.fragments
            .as_ref()?
            .element_boxes_scrolled(self.scrollers.offsets(), self.scroll)
            .get(&node)
            .copied()
    }

    /// The size of the scrollable content in CSS px.
    pub fn content_size(&mut self, fonts: &mut FontContext) -> Size {
        self.update_layout(fonts);
        self.fragments
            .as_ref()
            .map_or(self.view.viewport, |f| f.scroll_size)
    }

    /// True if the element has a box and is visible.
    pub(crate) fn is_rendered(&mut self, node: NodeId, fonts: &mut FontContext) -> bool {
        self.update_layout(fonts);
        let (Some(tree), Some(styles)) = (&self.fragments, &self.styles) else {
            return false;
        };
        is_rendered_in(&self.dom, tree, styles, node)
    }

    /// The elements that match a selector list, in tree order (as
    /// `querySelectorAll`). `None` if the selector list is invalid.
    pub fn query_selector_all(&self, selectors: &str) -> Option<Vec<NodeId>> {
        swb_style::query_selector_all(&self.dom, selectors, &self.states)
    }

    /// Hit-tests a point in viewport coordinates (CSS px).
    pub fn hit_test(&mut self, x: f32, y: f32, fonts: &mut FontContext) -> Option<HitResult> {
        self.update_display_list(fonts);
        let list = self.display_list.as_ref()?;
        let point = Point::new(x + self.scroll.x, y + self.scroll.y);
        hit_test::hit_test(
            &self.dom,
            list,
            point,
            self.scroll,
            Some(self.base_url.value()),
        )
    }

    // ----- Scrolling -----

    /// Scrolls the viewport to a position in CSS px, clamped to the
    /// content. Replaces a scroll to the fragment that waits for the
    /// document to load. Returns true if the position changed.
    pub fn scroll_to(&mut self, p: Point, fonts: &mut FontContext) -> bool {
        self.update_layout(fonts);
        self.pending_scroll = None;
        let before = self.scroll;
        self.scroll = p;
        self.clamp_scroll();
        self.scroll != before
    }

    /// Clamps the scroll position to the content. Without a layout the
    /// content size is not known; the position is then clamped after the
    /// next layout.
    pub(crate) fn clamp_scroll(&mut self) {
        let max = match &self.fragments {
            Some(fragments) => scroll_range(fragments.scroll_size, self.view.viewport),
            None => Point::new(swb_style::Length::MAX_PX, swb_style::Length::MAX_PX),
        };
        self.scroll = snap_scroll_offset(self.scroll, max);
    }

    /// The axes on which the user can scroll the viewport (horizontal,
    /// vertical): not those where the viewport's overflow is `hidden` or
    /// `clip`. Scripts and scroll into view can scroll those too.
    pub(crate) fn viewport_user_axes(&self) -> (bool, bool) {
        let user = |o: Overflow| !matches!(o, Overflow::Hidden | Overflow::Clip);
        self.fragments.as_ref().map_or((true, true), |f| {
            let (x, y) = f.viewport_overflow;
            (user(x), user(y))
        })
    }

    /// The scroll offsets of the scroll containers.
    pub(crate) fn scroll_offsets(&self) -> &dyn ScrollOffsets {
        self.scrollers.offsets()
    }

    /// True if `node` is the element whose scroll position is the
    /// viewport's (`document.scrollingElement`,
    /// <https://drafts.csswg.org/cssom-view/#dom-document-scrollingelement>):
    /// the root element, or in quirks mode the body if it is not a scroll
    /// container. (In quirks mode, the root element then is an element that
    /// does not scroll.)
    fn is_scrolling_element(&self, node: NodeId) -> bool {
        let doc = &self.dom;
        if doc.quirks_mode == QuirksMode::Quirks {
            doc.body() == Some(node) && self.scrollers.get(node).is_none()
        } else {
            doc.document_element() == Some(node)
        }
    }

    /// The scroll state of element `node`. `None` if it has no box.
    pub fn element_scroll(
        &mut self,
        node: NodeId,
        fonts: &mut FontContext,
    ) -> Option<ElementScroll> {
        self.update_layout(fonts);
        if self.is_scrolling_element(node) {
            let content = self.content_size(fonts);
            return Some(ElementScroll {
                offset: self.scroll,
                scroll_size: content,
                client_size: self.view.viewport,
                scrollable: true,
            });
        }
        if let Some(b) = self.scrollers.get(node) {
            // CSSOM View: `clientWidth` and `clientHeight` of the body in
            // quirks mode are the viewport's.
            let doc = &self.dom;
            let quirks_body = doc.quirks_mode == QuirksMode::Quirks && doc.body() == Some(node);
            return Some(ElementScroll {
                offset: self.scrollers.offset(node),
                scroll_size: b.scroll_size,
                client_size: if quirks_body {
                    self.view.viewport
                } else {
                    b.port
                },
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
    /// scroll. Returns the new offset (`None` if the element has no box)
    /// and whether something scrolled.
    pub fn scroll_element_to(
        &mut self,
        node: NodeId,
        offset: Point,
        fonts: &mut FontContext,
    ) -> (Option<Point>, bool) {
        self.update_layout(fonts);
        if self.is_scrolling_element(node) {
            let scrolled = self.scroll_to(offset, fonts);
            return (Some(self.scroll), scrolled);
        }
        if self.scrollers.get(node).is_none() {
            return (self.element_scroll(node, fonts).map(|s| s.offset), false);
        }
        let scrolled = self.set_element_scroll(node, offset);
        (Some(self.scrollers.offset(node)), scrolled)
    }

    /// Sets the scroll offset of a scroll container (by the user or a
    /// script; it replaces a scroll to the fragment that waits for the
    /// document to load, as `scroll_to` does). Returns true if it changed;
    /// then the display list is built again (the layout stays).
    pub(crate) fn set_element_scroll(&mut self, node: NodeId, offset: Point) -> bool {
        self.pending_scroll = None;
        if !self.scrollers.set(node, offset) {
            return false;
        }
        self.display_list = None;
        true
    }

    /// Scrolls so that the element is visible, as Chromium does for focus
    /// navigation: every scroll container that contains it, then the
    /// viewport, with "center if needed" on each axis (no scroll if it is
    /// visible, the nearest edge if it is partly visible, centered if it
    /// is not visible). Replaces a scroll to the fragment that waits for
    /// the document to load. Returns true if something scrolled.
    pub fn scroll_into_view(&mut self, node: NodeId, fonts: &mut FontContext) -> bool {
        let Some(rect) = self.element_box(node, fonts) else {
            return false;
        };
        self.pending_scroll = None;
        // Blink does not scroll horizontally if 32 px of the box are visible.
        self.reveal(
            node,
            rect,
            Align::CenterIfNeeded(Some(32.0)),
            Align::CenterIfNeeded(None),
        )
    }

    /// Scrolls `rect` (the box of `node` in document coordinates, with the
    /// scroll offsets applied) into view: in each scroll container of the
    /// node's scroll chain from the innermost, then in the viewport. After
    /// each scroll container, the part of the box inside its scrollport is
    /// revealed in the next one (as in Blink). Scrolls `overflow: hidden`
    /// containers too. Does not cancel a pending scroll to the fragment.
    /// Returns true if something scrolled.
    fn reveal(&mut self, node: NodeId, rect: Rect, x: Align, y: Align) -> bool {
        let (Some(styles), Some(tree)) = (&self.styles, &self.fragments) else {
            return false;
        };
        let (chain, fixed) = self
            .scrollers
            .chain_and_fixed(&self.dom, styles, node, false);
        let ports = self.scrollers.ports(tree, &chain, self.scroll);
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
        // A scroll of the viewport does not move a box fixed to it (as in
        // Chromium, also when it is partly outside the viewport).
        if fixed {
            return changed;
        }
        let before = self.scroll;
        let (scroll, viewport) = (self.scroll, self.view.viewport);
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
    pub fn scroll_clip(&mut self, node: NodeId, fonts: &mut FontContext) -> Option<Option<Rect>> {
        self.update_layout(fonts);
        let (styles, tree) = (self.styles.as_ref()?, self.fragments.as_ref()?);
        let chain = self.scrollers.chain(&self.dom, styles, node, false);
        let ports = self.scrollers.ports(tree, &chain, self.scroll);
        let mut clips = chain.iter().filter_map(|scroller| ports.get(scroller));
        let first = *clips.next()?;
        Some(clips.try_fold(first, |clip, port| clip.intersection(port)))
    }

    // ----- Fragments -----

    /// Scrolls to the element that the fragment indicates (HTML "scroll to
    /// the fragment"): the element with that ID or an `<a>` with that name,
    /// or the top of the document for an empty fragment or `top`. Without a
    /// layout yet, scrolls after the first layout. Returns true if
    /// something scrolled.
    pub(crate) fn scroll_to_fragment(&mut self, fragment: &str, fonts: &mut FontContext) -> bool {
        self.update_layout(fonts);
        if self.fragments.is_none() {
            self.pending_scroll = Some(super::ScrollTarget::Fragment(fragment.to_owned()));
            return false;
        }
        self.pending_scroll = None;
        self.reveal_fragment(fragment)
    }

    /// Scrolls to what a fragment indicates, if anything
    /// (<https://html.spec.whatwg.org/multipage/browsing-the-web.html#the-indicated-part-of-the-document>):
    /// the target element at the top of each scroll container and of the
    /// viewport (CSSOM View `scrollIntoView` with `block: "start"` and
    /// `inline: "nearest"`), or the top of the document. Returns true if
    /// something scrolled.
    pub(crate) fn reveal_fragment(&mut self, fragment: &str) -> bool {
        let Some(tree) = &self.fragments else {
            return false;
        };
        let decoded = percent_decode(fragment);
        match indicated(&self.dom, fragment) {
            Some(target) => {
                let Some((node, rect)) =
                    first_rendered(&self.dom, tree, self.scroll_offsets(), self.scroll, target)
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

    // ----- Text and the selection -----

    /// The rendered text of an element (or text node), as `innerText`:
    /// collapsed white space, line breaks between blocks. Empty if the
    /// node has no rendered text.
    pub fn element_text(&mut self, node: NodeId, fonts: &mut FontContext) -> String {
        self.update_layout(fonts);
        if self.dom.get(node).is_none() || self.fragments.is_none() || self.styles.is_none() {
            return String::new();
        }
        self.tree_order();
        let (Some(tree), Some(styles)) = (&self.fragments, &self.styles) else {
            return String::new();
        };
        let extents = selection::text_extents(tree);
        let Some(range) = selection::subtree_range(&self.dom, node, &extents) else {
            return String::new();
        };
        let order = self.tree_order.value();
        let (start, end) = range.ordered(order);
        selection::selected_text(&self.dom, styles, start, end, order)
    }

    /// The selected text, with white space collapsed and line breaks
    /// between blocks (as `innerText`). Empty without a selection. While a
    /// text control has the focus, the text selected in it (nothing for a
    /// password field).
    pub(crate) fn selected_text(&mut self, fonts: &mut FontContext) -> String {
        if let Some(text) = self.control_selected_text() {
            return text;
        }
        self.update_layout(fonts);
        let Some(selection) = self.visible_selection() else {
            return String::new();
        };
        self.tree_order();
        let Some(styles) = &self.styles else {
            return String::new();
        };
        let order = self.tree_order.value();
        let (start, end) = selection.ordered(order);
        selection::selected_text(&self.dom, styles, start, end, order)
    }

    /// The selected text of the focused text control: `Some("")` if a
    /// text control has the focus but nothing is selected, or if it is a
    /// password field. `None` without a focused text control.
    pub(crate) fn control_selected_text(&self) -> Option<String> {
        let state = self.forms.value().get(self.states.focus?)?;
        if !state.ty.is_text() {
            return None;
        }
        if state.ty == ControlType::Input(InputType::Password) {
            return Some(String::new());
        }
        Some(state.edit.selected_text().to_owned())
    }

    /// Selects all text of the document. Returns true if the selection
    /// changed.
    pub(crate) fn select_all(&mut self, fonts: &mut FontContext) -> bool {
        self.update_layout(fonts);
        let Some(tree) = &self.fragments else {
            return false;
        };
        let range =
            selection::subtree_range(&self.dom, NodeId::DOCUMENT, &selection::text_extents(tree));
        self.set_selection(range)
    }

    /// The text position at a point in viewport coordinates.
    pub(crate) fn position_at(
        &mut self,
        point: Point,
        snap: Snap,
        fonts: &mut FontContext,
    ) -> Option<TextPosition> {
        self.update_layout(fonts);
        let tree = self.fragments.as_ref()?;
        let document_point = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        selection::position_at(tree, self.scroll_offsets(), document_point, snap)
    }

    /// The selection of a triple click: the paragraph at a point.
    pub(crate) fn paragraph_selection(
        &mut self,
        point: Point,
        fonts: &mut FontContext,
    ) -> Option<Selection> {
        let position = self.position_at(point, Snap::Character, fonts)?;
        let (styles, tree) = (self.styles.as_ref()?, self.fragments.as_ref()?);
        selection::paragraph_at(&self.dom, styles, &selection::text_extents(tree), position)
    }

    // ----- Elements under the pointer -----

    /// The element that a click on `node` focuses.
    pub(crate) fn click_focus_target(&self, node: NodeId) -> Option<NodeId> {
        let tree = self.fragments.as_ref()?;
        let doc = &self.dom;
        let boxes = tree.element_boxes();
        focus::click_target(doc, node, |n| {
            boxes.contains_key(&n) && !doc.in_closed_details_content(n)
        })
    }

    /// The element of a hit node: the node itself, or the parent of a text
    /// node.
    pub(crate) fn element_of(&self, node: NodeId) -> Option<NodeId> {
        if self.dom.get(node)?.is_element() {
            Some(node)
        } else {
            self.dom.parent_element(node)
        }
    }
}

/// The element that a fragment indicates: by its text, then percent-decoded.
pub(crate) fn indicated(doc: &Document, fragment: &str) -> Option<NodeId> {
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

/// The first fragment of `node` as painted at the viewport scroll offset
/// `scroll` (document coordinates, with the scroll offsets of scroll
/// containers, transforms, and fixed and sticky positioning) and the node
/// it belongs to. A node without fragments (for
/// example an empty `<a name>`) uses the first fragment that follows it in
/// tree order.
fn first_rendered(
    doc: &Document,
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    scroll: Point,
    node: NodeId,
) -> Option<(NodeId, Rect)> {
    let mut first: HashMap<NodeId, Rect> = HashMap::new();
    tree.walk_painted(offsets, scroll, |fragment, rect, matrix| {
        let owner = match fragment {
            FragmentRef::Box(b) if b.pseudo.is_none() => match b.node {
                Some(owner) => owner,
                None => return,
            },
            FragmentRef::Text(t) => t.node,
            FragmentRef::Box(_) => return,
        };
        first.entry(owner).or_insert_with(|| matrix.map_rect(&rect));
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
