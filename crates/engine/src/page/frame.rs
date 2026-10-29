//! The rendering update of a document with scripts (HTML "update the
//! rendering",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>;
//! ADR 0028 section 7; M8 features 7b1 and 7b2): the frame runs, in this
//! order, the resize steps (`resize` at the window), the scroll steps
//! (`scroll` events, CSSOM View "run the scroll steps"), the media query
//! changes (`change` at `MediaQueryList`s) and the animation frame
//! callbacks.
//!
//! A frame is due when something needs it: a scroll or a resize, a change
//! of the media environment (`frame_due`, one frame length after it), or
//! an animation frame callback (the next frame on a grid of 60 frames per
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

/// The length of a frame in milliseconds of page time (60 Hz).
const FRAME_MS: f64 = 1000.0 / 60.0;

/// The scroll offsets that `scroll` events were fired for, to find what
/// scrolled since the last frame.
#[derive(Clone, Debug, Default)]
pub(super) struct ScrollSeen {
    document: Point,
    elements: HashMap<NodeId, Point>,
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
        let callbacks = self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::has_animation_frame_callbacks)
            .then(|| {
                // The grid keeps the time stable between calls; after a
                // pause the frame is due at once (one frame, not one per
                // grid time missed).
                let latest_passed = (self.clock.now() / FRAME_MS).floor() * FRAME_MS;
                match self.input.last_frame {
                    Some(last) => grid_at_or_after(last + FRAME_MS).max(latest_passed),
                    None => latest_passed,
                }
            });
        match (self.input.frame_due, callbacks) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// True if a frame is due for something other than animation frame
    /// callbacks (a page with a `requestAnimationFrame` loop can still be
    /// idle).
    pub(super) fn frame_work_due(&self) -> Option<f64> {
        self.input.frame_due.filter(|_| self.frames_run())
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
        let mut ran = false;
        if std::mem::take(&mut self.input.resized) {
            self.dispatch_event(DispatchTarget::Window, &TrustedEvent::new("resize"));
            ran = true;
        }
        ran |= self.fire_scroll_events();
        ran |= self
            .run_task(ScriptRuntime::evaluate_media_queries)
            .unwrap_or(false);
        if self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::has_animation_frame_callbacks)
        {
            self.run_task(|s| s.run_animation_frame_callbacks(due));
            ran = true;
        }
        ran
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
