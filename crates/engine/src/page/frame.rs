//! The rendering update of a document with scripts (HTML "update the
//! rendering",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>;
//! ADR 0028 section 7; M8 features 7b1 and 7b2): the frame runs, in this
//! order, the resize steps (`resize` at the window), the scroll steps
//! (`scroll` events, CSSOM View "run the scroll steps"), the media query
//! changes (`change` at `MediaQueryList`s), the animation frame callbacks,
//! the check of the lazy images (M9 feature 4, ADR 0032) and the observers.
//!
//! A frame is due when something needs it: a scroll or a resize, a change
//! of the media environment (`frame_due`, one frame length after it), an
//! animation frame callback, or a deferred lazy image whose surroundings
//! changed (the next frame on a grid of 60 frames per
//! second of page time, at least one frame length after the last frame).
//! In virtual time the clock jumps to the frame like to a task, so a
//! `requestAnimationFrame` loop sees 16.67 ms between frames, as in
//! Chromium's headless shell (measured: 16.7 ms on average over 20
//! frames). Style and layout are not part of the frame here: they run when
//! something needs them (painting in the GUI, a query of a script, a
//! screenshot), which no script can tell apart (ADR 0028 section 9).

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_layout::Point;
use swb_web::{DispatchTarget, ScriptRuntime, TrustedEvent};

use super::Page;
use crate::document::DocumentState;
use crate::frames::FrameId;

/// The length of a frame in milliseconds of page time (60 Hz).
const FRAME_MS: f64 = 1000.0 / 60.0;

/// The scroll offsets that `scroll` events were fired for, to find what
/// scrolled since the last frame.
#[derive(Clone, Debug, Default)]
pub(super) struct ScrollSeen {
    document: Point,
    elements: HashMap<NodeId, Point>,
}

/// What can change an observation: the DOM, the layouts that ran, the scroll
/// offsets and the viewport. The observers run in a frame when this differs
/// from what they saw at the last one (or when an observation was added).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ObservedState {
    generation: u64,
    layouts: u32,
    document_scroll: Point,
    scrollers: HashMap<NodeId, Point>,
    viewport: swb_layout::Size,
    frames: u64,
}

/// The first time of the frame grid at or after `time` (the grid starts at
/// page time 0; a small tolerance keeps a time on the grid there).
fn grid_at_or_after(time: f64) -> f64 {
    (time / FRAME_MS - 1e-6).ceil() * FRAME_MS
}

impl Page {
    /// Asks for a frame one frame length from now (if none is due
    /// earlier), for documents with scripts.
    pub(super) fn request_frame(&mut self) {
        if self.script.is_some() && self.input.frame_due.is_none() {
            self.input.frame_due = Some(self.clock.now() + FRAME_MS);
        }
    }

    /// Notes that something may have scrolled (`element`: the scroll
    /// container that the caller scrolled, if it is one): the frame fires
    /// `scroll` events for what did, in the order it scrolled (CSSOM View
    /// "pending scroll event targets"; the document recorded the scrolls of
    /// the viewport and of its scroll containers, also those of a script
    /// task, in `DocumentState::scrolled`).
    pub(super) fn note_scroll(&mut self, element: Option<NodeId>) {
        let scrolled = self
            .doc
            .as_deref_mut()
            .map(|d| std::mem::take(&mut d.scrolled))
            .unwrap_or_default();
        if self.script.is_none() {
            return;
        }
        for node in scrolled.into_iter().chain(element) {
            if !self.input.scroll_order.contains(&node) {
                self.input.scroll_order.push(node);
            }
        }
        self.request_frame();
    }

    /// What the observers would see now; `None` while the document state is
    /// lent to a task.
    fn observed_state(&self) -> Option<ObservedState> {
        let doc = self.doc.as_deref()?;
        Some(ObservedState {
            generation: doc.dom.generation(),
            layouts: doc
                .flushes
                .layout_runs
                .saturating_add(doc.flushes.style_runs),
            document_scroll: doc.scroll,
            scrollers: doc.scrollers.offsets().clone(),
            viewport: self.view.viewport,
            frames: self.frames_observed_state(),
        })
    }

    /// What can change an observation in the frame documents with a realm: a
    /// sum, which is enough to see that something changed.
    fn frames_observed_state(&self) -> u64 {
        if self.frames.count() == 0 {
            return 0;
        }
        let mut sum = 0u64;
        for id in self.frames_with_realms() {
            if let Some(doc) = self.doc_of(id) {
                sum = sum
                    .wrapping_add(doc.dom.generation())
                    .wrapping_add(u64::from(doc.flushes.layout_runs))
                    .wrapping_add(u64::from(doc.flushes.style_runs))
                    .wrapping_add(f64::from(doc.scroll.x + doc.scroll.y).to_bits());
            }
        }
        sum
    }

    /// True if the observers need a frame: an observation was added, or
    /// something that changes an observation changed since the last frame.
    fn observers_due(&self) -> bool {
        let Some(script) = self.script.as_ref() else {
            return false;
        };
        script.has_observations()
            && (script.observations_requested()
                || self
                    .observed_state()
                    .is_some_and(|now| self.input.observed.as_ref() != Some(&now)))
    }

    /// The time of the next frame on the grid, at least one frame length
    /// after the last.
    fn grid_frame_time(&self) -> f64 {
        // The grid keeps the time stable between calls; after a pause the
        // frame is due at once (one frame, not one per grid time missed).
        let latest_passed = (self.clock.now() / FRAME_MS).floor() * FRAME_MS;
        match self.input.last_frame {
            Some(last) => grid_at_or_after(last + FRAME_MS).max(latest_passed),
            None => latest_passed,
        }
    }

    /// True if the document has a runtime whose scripts can run (not
    /// stopped by the time limits): only then are frames due.
    pub(super) fn frames_run(&self) -> bool {
        self.script.as_ref().is_some_and(|s| !s.stopped())
    }

    /// The page time of the next frame, if one is needed. None after the
    /// scripts were stopped: no frame runs then, and a due time in the
    /// past would keep the event loop from advancing or being idle.
    pub(super) fn frame_time(&self) -> Option<f64> {
        if !self.frames_run() {
            return None;
        }
        let callbacks = (self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::has_animation_frame_callbacks)
            || self.observers_due()
            || self.lazy_due())
        .then(|| self.grid_frame_time());
        match (self.input.frame_due, callbacks) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// True if a frame is due for something other than animation frame
    /// callbacks (a page with a `requestAnimationFrame` loop can still be
    /// idle).
    pub(super) fn frame_work_due(&self) -> Option<f64> {
        let observers = (self.frames_run() && (self.observers_due() || self.lazy_due()))
            .then(|| self.grid_frame_time());
        self.input
            .frame_due
            .filter(|_| self.frames_run())
            .into_iter()
            .chain(observers)
            .reduce(f64::min)
    }

    /// The rendering update of a frame that is due: `resize`, `scroll`,
    /// media query changes and animation frame callbacks, each step as one
    /// or more tasks. Returns true if a step ran something.
    pub(super) fn run_frame(&mut self) -> bool {
        let Some(due) = self.frame_time() else {
            return false;
        };
        if due > self.clock.now() || !self.script_input() {
            return false;
        }
        self.input.frame_due = None;
        // The time the frame was due, so that a late pump keeps the grid.
        self.input.last_frame = Some(due);
        let documents = self.frames_with_realms();
        // A frame's viewport comes from its parent's layout: a size that a
        // change in the parent gave it shows in this update, if the frame's
        // window reacts to it.
        if self.frames_watch_viewports(&documents) {
            self.update_layout();
        }
        let mut ran = false;
        if std::mem::take(&mut self.input.resized) {
            self.dispatch_event(DispatchTarget::Window, &TrustedEvent::new("resize"));
            ran = true;
        }
        ran |= self.fire_scroll_events();
        ran |= self
            .run_task(ScriptRuntime::evaluate_media_queries)
            .unwrap_or(false);
        if self.has_animation_frames_in(FrameId::TOP) {
            self.run_task(|s| s.run_animation_frame_callbacks(due));
            ran = true;
        }
        // The documents in tree order, top first (measured order of the
        // animation frame callbacks: parent, parent, child).
        for &id in &documents {
            ran |= self.frame_rendering_steps(id, due);
        }
        ran |= self.run_lazy_check();
        // Style and layout, then the observers (HTML "update the
        // rendering"): resize observers, then intersection observers.
        if self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::has_observations)
        {
            if !documents.is_empty() {
                self.update_layout();
            }
            ran |= self.run_observers_in(FrameId::TOP, due);
            for &id in &documents {
                ran |= self.run_observers_in(id, due);
            }
            self.input.observed = self.observed_state();
        }
        ran
    }

    /// The frames whose documents have a realm, in tree order, without the
    /// top level.
    fn frames_with_realms(&self) -> Vec<FrameId> {
        if !self.frames_may_have_realms() {
            return Vec::new();
        }
        self.frames
            .tree_order()
            .into_iter()
            .skip(1)
            .filter(|&id| self.frames.get(id).is_some_and(|f| f.script.is_some()))
            .collect()
    }

    /// True if a frame document among `documents` reacts to a new size of its
    /// viewport.
    fn frames_watch_viewports(&self, documents: &[FrameId]) -> bool {
        let Some(script) = self.script.as_ref() else {
            return false;
        };
        documents.iter().any(|&id| {
            self.frames
                .get(id)
                .and_then(|f| f.script)
                .is_some_and(|key| script.watches_viewport_in(key))
        })
    }

    /// True if the document of `frame` has animation frame callbacks.
    fn has_animation_frames_in(&self, frame: FrameId) -> bool {
        let Some(script) = self.script.as_ref() else {
            return false;
        };
        self.frames
            .get(frame)
            .and_then(|f| f.script)
            .is_some_and(|key| script.has_animation_frames_in(key))
    }

    /// The steps of the rendering update for a frame document before layout:
    /// `resize`, `scroll`, media queries, animation frame callbacks. True if
    /// a step ran something.
    fn frame_rendering_steps(&mut self, id: FrameId, due: f64) -> bool {
        let mut ran = false;
        let resized = self
            .frames
            .get_mut(id)
            .is_some_and(|f| std::mem::take(&mut f.resized));
        if resized {
            let event = TrustedEvent::new("resize");
            self.run_task_in(id, |s| s.dispatch_event(DispatchTarget::Window, &event));
            ran = true;
        }
        let scrolled = self
            .doc_of_mut(id)
            .map(|d| std::mem::take(&mut d.scrolled))
            .unwrap_or_default();
        for node in scrolled {
            let mut event = TrustedEvent::new("scroll");
            event.bubbles = node == NodeId::DOCUMENT;
            self.run_task_in(id, |s| s.dispatch_event(DispatchTarget::Node(node), &event));
            ran = true;
        }
        ran |= self
            .run_task_in(id, ScriptRuntime::evaluate_media_queries)
            .unwrap_or(false);
        if self.has_animation_frames_in(id) {
            self.run_task_in(id, |s| s.run_animation_frame_callbacks(due));
            ran = true;
        }
        ran
    }

    /// The observers of the document of `frame`, after layout. True if one
    /// ran.
    fn run_observers_in(&mut self, frame: FrameId, due: f64) -> bool {
        let observing = self.script.as_ref().is_some_and(|script| {
            self.frames
                .get(frame)
                .and_then(|f| f.script)
                .is_some_and(|key| script.has_observations_in(key))
        });
        if !observing {
            return false;
        }
        let resize = self
            .run_task_in(frame, ScriptRuntime::run_resize_observers)
            .unwrap_or(false);
        let intersection = self
            .run_task_in(frame, |s| s.update_intersection_observers(due))
            .unwrap_or(false);
        resize || intersection
    }

    /// A task changed the DOM of the top-level document, or of a frame
    /// document: a frame's viewport may have a new size, which the next
    /// rendering update finds if a frame's window reacts to it.
    pub(super) fn note_frame_viewports_may_change(&mut self) {
        if self.frames.count() > 0 {
            let documents = self.frames_with_realms();
            if self.frames_watch_viewports(&documents) {
                self.request_frame();
            }
        }
    }

    /// True if a deferred lazy image can be in range now: some are
    /// deferred and a new one, a scroll offset, the viewport, the style
    /// sheets or a layout that happened for another reason changed since
    /// the last check (a DOM change alone does not count; ADR 0032).
    fn lazy_due(&self) -> bool {
        self.frames_run()
            && (self.doc.as_deref().is_some_and(DocumentState::lazy_due) || self.frame_lazy_due())
    }

    /// True if a frame document has a lazy image that can be in range now
    /// (a scroll of the frame's viewport, a new frame size, ...). The
    /// documents of frames have the scripting flag of the page, so their
    /// lazy images are deferred; the frame loop of the page checks them
    /// (ADR 0032, ADR 0031 section 8).
    fn frame_lazy_due(&self) -> bool {
        self.frames.count() > 0
            && self
                .frames
                .tree_order()
                .into_iter()
                .skip(1)
                .filter_map(|id| self.frames.get(id)?.doc.as_deref())
                .any(DocumentState::lazy_due)
    }

    /// The check of the lazy images, if it is due (the loads of the images
    /// near the viewport start), in the page's document and in the frame
    /// documents. True if a load started.
    fn run_lazy_check(&mut self) -> bool {
        if self.lent {
            return false;
        }
        let started = match self.parts() {
            Some((doc, fonts)) => doc.release_lazy_images(fonts),
            None => false,
        };
        started | self.run_frame_lazy_checks()
    }

    /// The check of the lazy images of the frame documents whose check is
    /// due, each in the viewport of its frame.
    fn run_frame_lazy_checks(&mut self) -> bool {
        if !self.frame_lazy_due() {
            return false;
        }
        self.layout_frames();
        let mut started = false;
        for id in self.frames.tree_order().into_iter().skip(1) {
            let Some(doc) = self.frames.get_mut(id).and_then(|f| f.doc.as_deref_mut()) else {
                continue;
            };
            if doc.lazy_due() {
                started |= doc.with_own_fonts(&mut self.fonts, DocumentState::release_lazy_images);
            }
        }
        started
    }

    /// A document whose scripts cannot run frames (no runtime, or stopped
    /// by the time limits) starts all its deferred lazy images.
    pub(super) fn release_lazy_without_frames(&mut self) -> bool {
        if self.lent || self.frames_run() {
            return false;
        }
        let mut released = match self.doc.as_deref_mut() {
            Some(doc) if doc.has_deferred_images() => {
                doc.release_all_lazy_images();
                true
            }
            _ => false,
        };
        for id in self.frames.tree_order().into_iter().skip(1) {
            if let Some(doc) = self.frames.get_mut(id).and_then(|f| f.doc.as_deref_mut())
                && doc.has_deferred_images()
            {
                doc.release_all_lazy_images();
                released = true;
            }
        }
        released
    }

    /// The scroll steps: fires `scroll` once at each target that scrolled
    /// since the last frame (the document for the viewport), in the order
    /// they scrolled (measured in Chromium 148: a container that a script
    /// scrolled before the viewport gets its event first; a scroll back to
    /// the old position still fires). Targets whose offsets changed without
    /// a note (a layout clamped them) follow, the document first, then in
    /// tree order. Returns true if an event was fired.
    fn fire_scroll_events(&mut self) -> bool {
        let Some(doc) = self.doc.as_deref() else {
            return false;
        };
        let seen = std::mem::take(&mut self.input.scroll_seen);
        let current = ScrollSeen {
            document: doc.scroll,
            elements: doc.scrollers.offsets().clone(),
        };
        let mut targets = std::mem::take(&mut self.input.scroll_order);
        if current.document != seen.document && !targets.contains(&NodeId::DOCUMENT) {
            targets.push(NodeId::DOCUMENT);
        }
        let unnoted: std::collections::HashSet<NodeId> = current
            .elements
            .iter()
            .filter(|(node, offset)| seen.elements.get(node) != Some(offset))
            .map(|(node, _)| *node)
            .chain(
                seen.elements
                    .keys()
                    .filter(|node| !current.elements.contains_key(node))
                    .copied(),
            )
            .filter(|node| !targets.contains(node))
            .collect();
        if !unnoted.is_empty() {
            let doc = self.document();
            targets.extend(doc.into_iter().flat_map(|d| {
                d.descendants(NodeId::DOCUMENT)
                    .filter(|n| unnoted.contains(n))
                    .collect::<Vec<_>>()
            }));
        }
        self.input.scroll_seen = current;
        let fired = !targets.is_empty();
        for node in targets {
            self.fire_scroll(node, node == NodeId::DOCUMENT);
        }
        fired
    }

    /// Fires `scroll` at `node` (it bubbles for the document only).
    fn fire_scroll(&mut self, node: NodeId, bubbles: bool) {
        if !self.is_connected(node) {
            return;
        }
        let mut event = TrustedEvent::new("scroll");
        event.bubbles = bubbles;
        self.dispatch_event(DispatchTarget::Node(node), &event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_grid_keeps_times_on_it() {
        assert_eq!(grid_at_or_after(0.0), 0.0);
        let one = grid_at_or_after(1.0);
        assert!((one - FRAME_MS).abs() < 1e-9);
        assert_eq!(grid_at_or_after(one), one);
        let two = grid_at_or_after(one + FRAME_MS);
        assert!((two - 2.0 * FRAME_MS).abs() < 1e-9);
        assert_eq!(grid_at_or_after(two), two);
    }
}
