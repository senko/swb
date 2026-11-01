//! Lazy loading of images (M9 feature 4, ADR 0032; HTML "lazy loading
//! attributes",
//! <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#lazy-loading-attributes>,
//! and the lazy load steps of "update the image data",
//! <https://html.spec.whatwg.org/multipage/images.html#update-the-image-data>).
//!
//! An `img` element with `loading=lazy` in a document with scripting does
//! not fetch its image when its source is selected. The selection is kept
//! (`LazyImages::deferred`, element to URL), and the load starts when the
//! frame of the page finds the element's box near the viewport. The
//! distances are Chromium 148's (measured, `tools/probes/web-lazy-loading.json`):
//! [`VERTICAL_DISTANCE`] above and below, [`HORIZONTAL_DISTANCE`] left and
//! right, counted from the viewport edge to the nearest edge of the border
//! box, whatever the viewport size. The box is the painted box (scroll
//! offsets of scroll containers, transforms, fixed and sticky positioning);
//! clipping by `overflow` does not hide it, and an element without a box
//! (`display: none`, the content of a closed `details` or of
//! `content-visibility: hidden`) never starts.
//!
//! Once the load of an element started this way, or `loading` changed so
//! that the deferred load started, the element loads at once from then on
//! ([`LazyImages::released`]; measured: `lazy-flag` cases). An element
//! that was never deferred (eager before) defers its next load if `loading`
//! is `lazy` then.
//!
//! Without scripting the attribute has no effect (the specification's
//! anti-tracking rule; Chromium 148 loads all images of such a document at
//! once, also `display: none` ones).
//!
//! The check ([`DocumentState::release_lazy_images`]) runs in the frame of
//! the page, only while images are deferred. It lays out only for a
//! reason that can bring a box near the viewport and that no other layout
//! covers: a newly deferred element, a scroll, a change of the viewport or
//! of the style sheets. Otherwise it waits for a layout that happens for
//! another reason (paint, a query of a script, an observer), so that a page
//! that changes the DOM in every frame does not pay a layout and a walk
//! of all fragments for an image that never comes near. It never forces a
//! layout in a script task. See ADR 0032.

use std::collections::{HashMap, HashSet};

use swb_dom::NodeId;
use swb_layout::{FragmentRef, Point, Rect, Size};
use swb_net::Url;
use swb_text::FontContext;

use super::DocumentState;
use super::geometry::map_rect;

/// How far above and below the viewport a lazy image starts to load, in
/// CSS px (Chromium 148 without network information).
pub(crate) const VERTICAL_DISTANCE: f32 = 3000.0;

/// How far left and right of the viewport, in CSS px.
pub(crate) const HORIZONTAL_DISTANCE: f32 = 1500.0;

/// What a check saw, to find out whether another one can give a different
/// answer. The DOM generation is not part of it: a DOM change moves a
/// box only through a layout, which changes `layout`.
#[derive(Clone, Debug)]
struct Seen {
    layout: u64,
    sheets_settled: bool,
    scroll: Point,
    offsets: HashMap<NodeId, Point>,
    viewport: Size,
}

/// The deferred lazy images of a document.
#[derive(Debug, Default)]
pub(crate) struct LazyImages {
    /// The elements whose load waits for the viewport, with the URL of
    /// their last selection.
    deferred: HashMap<NodeId, Url>,
    /// The elements whose deferred load started: they never defer again.
    released: HashSet<NodeId>,
    /// True if an element was deferred since the last check: its box is
    /// not known, so the next check lays out.
    fresh: bool,
    /// What the last check saw.
    checked: Option<Seen>,
}

impl LazyImages {
    /// True if loads wait for the viewport.
    pub(crate) fn any_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }
}

impl DocumentState {
    /// True if the next load of the image of `node` waits for the viewport:
    /// the document has scripting, `loading` is `lazy` (ASCII case
    /// insensitive), and the element has not been released.
    fn will_lazy_load(&self, node: NodeId) -> bool {
        self.dom.scripting()
            && !self.lazy.released.contains(&node)
            && self
                .dom
                .element(node)
                .and_then(|e| e.attr("loading"))
                .is_some_and(|v| v.eq_ignore_ascii_case("lazy"))
    }

    /// Starts the load of `url` for the element `node`, or defers it.
    pub(super) fn start_node_image(&mut self, node: NodeId, url: Url) {
        if self.will_lazy_load(node) {
            self.lazy_defer(node, url);
        } else {
            // A deferred load of an element that is not lazy any more
            // starts: the element is released (HTML "lazy load resumption
            // steps").
            if self.lazy.deferred.remove(&node).is_some() {
                self.lazy.released.insert(node);
            }
            self.start_image(url);
        }
    }

    /// True if the load of `node` waits for the viewport.
    pub(super) fn lazy_is_deferred(&self, node: NodeId) -> bool {
        self.lazy.deferred.contains_key(&node)
    }

    /// Defers the load of `url` for `node` until its box is near the
    /// viewport. The next check lays out to find the box.
    fn lazy_defer(&mut self, node: NodeId, url: Url) {
        self.lazy.deferred.insert(node, url);
        self.lazy.fresh = true;
    }

    /// Starts the deferred load of `url` for `node`, and releases the
    /// element. If the image is known already (no request starts), the
    /// element's `load` event comes now.
    fn lazy_start(&mut self, node: NodeId, url: Url) {
        self.lazy.released.insert(node);
        self.start_image(url);
        self.finish_known_watch(node);
    }

    /// The element `node` needs no load now (no source): it waits for
    /// nothing.
    pub(super) fn lazy_cancel(&mut self, node: NodeId) {
        self.lazy.deferred.remove(&node);
    }

    /// The selection of `node` gave `url`, which is known already. A
    /// deferred load stays deferred, now for this URL (Chromium keeps such
    /// an image unstarted until its box is near the viewport; measured,
    /// case `same-url`).
    pub(super) fn lazy_known(&mut self, node: NodeId, url: &Url) {
        if let Some(deferred) = self.lazy.deferred.get_mut(&node) {
            deferred.clone_from(url);
        }
    }

    /// True if the element `node` takes the image at the known `url` only
    /// when it comes near the viewport: it defers, and `watch_image` waits
    /// for the release.
    pub(super) fn lazy_defers_known(&mut self, node: NodeId, url: &Url) -> bool {
        if self.will_lazy_load(node) {
            self.lazy_defer(node, url.clone());
            true
        } else {
            self.lazy.deferred.remove(&node);
            false
        }
    }

    /// The element left the document: it forgets its state.
    pub(super) fn lazy_forget(&mut self, node: NodeId) {
        self.lazy.deferred.remove(&node);
        self.lazy.released.remove(&node);
    }

    /// The `loading` attribute of `node` changed: a deferred load of an
    /// element that is not lazy any more starts (HTML "lazy load
    /// resumption steps").
    pub(super) fn lazy_loading_changed(&mut self, node: NodeId) {
        if !self.lazy.deferred.contains_key(&node) || self.will_lazy_load(node) {
            return;
        }
        if let Some(url) = self.lazy.deferred.remove(&node) {
            self.lazy_start(node, url);
        }
    }

    /// Starts every deferred load (the page cannot run frames).
    pub(crate) fn release_all_lazy_images(&mut self) {
        let mut all: Vec<(NodeId, Url)> = self.lazy.deferred.drain().collect();
        all.sort_by_key(|(node, _)| *node);
        for (node, url) in all {
            self.lazy_start(node, url);
        }
    }

    fn lazy_seen(&self) -> Seen {
        Seen {
            layout: self.layout_serial,
            sheets_settled: self.stylesheets_settled(),
            scroll: self.scroll,
            offsets: self.scrollers.offsets().clone(),
            viewport: self.view.viewport,
        }
    }

    /// True if the check must lay out: an element was deferred since the
    /// last check, or the scroll offsets, the viewport or the state of the
    /// style sheets changed. Compares in place.
    fn lazy_forced(&self) -> bool {
        let Some(seen) = &self.lazy.checked else {
            return true;
        };
        self.lazy.fresh
            || seen.scroll != self.scroll
            || seen.viewport != self.view.viewport
            || seen.sheets_settled != self.stylesheets_settled()
            || &seen.offsets != self.scrollers.offsets()
    }

    /// True if the check has work: images are deferred, and either the
    /// check must lay out ([`DocumentState::lazy_forced`]) or a layout
    /// happened since the last check for another reason (paint, a query of a
    /// script, an observer). A DOM change alone does not make it due: a
    /// page that changes the DOM in every frame does not pay a layout and a
    /// walk for deferred images (ADR 0032).
    pub(crate) fn lazy_due(&self) -> bool {
        self.lazy.any_deferred()
            && (self.lazy_forced()
                || (self.fragments.is_some()
                    && self
                        .lazy
                        .checked
                        .as_ref()
                        .is_some_and(|seen| seen.layout != self.layout_serial)))
    }

    /// True if images are deferred.
    pub(crate) fn has_deferred_images(&self) -> bool {
        self.lazy.any_deferred()
    }

    /// The check of the lazy load observer: lays out if it must
    /// ([`DocumentState::lazy_forced`]), then starts the load of each
    /// deferred image whose box is within the distances of the viewport.
    /// Returns true if a load started.
    pub(crate) fn release_lazy_images(&mut self, fonts: &mut FontContext) -> bool {
        if !self.lazy_due() {
            return false;
        }
        if self.lazy_forced() {
            self.update_layout(fonts);
        }
        self.lazy.fresh = false;
        self.lazy.checked = Some(self.lazy_seen());
        let mut hits = self.lazy_in_range();
        hits.sort_unstable();
        hits.dedup();
        let started = !hits.is_empty();
        for node in hits {
            if let Some(url) = self.lazy.deferred.remove(&node) {
                self.lazy_start(node, url);
            }
        }
        started
    }

    /// The deferred elements with a painted box near the viewport.
    fn lazy_in_range(&self) -> Vec<NodeId> {
        let Some(tree) = &self.fragments else {
            return Vec::new();
        };
        let view = self.view.viewport;
        let scroll = self.scroll;
        let port = Rect::new(
            scroll.x - HORIZONTAL_DISTANCE,
            scroll.y - VERTICAL_DISTANCE,
            view.width + 2.0 * HORIZONTAL_DISTANCE,
            view.height + 2.0 * VERTICAL_DISTANCE,
        );
        let deferred = &self.lazy.deferred;
        let mut hits = Vec::new();
        tree.walk_painted(
            self.scrollers.offsets(),
            scroll,
            |fragment, rect, matrix| {
                if let FragmentRef::Box(b) = fragment
                    && let (Some(node), None) = (b.node, b.pseudo)
                    && deferred.contains_key(&node)
                    && within(&port, &map_rect(matrix, rect))
                {
                    hits.push(node);
                }
            },
        );
        hits
    }
}

/// True if `rect` is inside `port` or overlaps it. A rectangle that only
/// touches the edge of `port` is outside (measured: an image whose top is
/// exactly 3000 px below the viewport does not load).
fn within(port: &Rect, rect: &Rect) -> bool {
    rect.x < port.right()
        && rect.right() > port.x
        && rect.y < port.bottom()
        && rect.bottom() > port.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_that_touches_the_edge_is_outside() {
        let port = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(within(&port, &Rect::new(10.0, 99.0, 5.0, 5.0)));
        assert!(!within(&port, &Rect::new(10.0, 100.0, 5.0, 5.0)));
        assert!(!within(&port, &Rect::new(10.0, -5.0, 5.0, 5.0)));
        assert!(within(&port, &Rect::new(10.0, -4.0, 5.0, 5.0)));
        // A box without size inside the port counts.
        assert!(within(&port, &Rect::new(50.0, 50.0, 0.0, 0.0)));
    }
}
