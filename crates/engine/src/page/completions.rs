//! Network completions of requests that input handlers start, in virtual
//! time (ADR 0028 section 7, "Clocks"; M12 feature 3c).
//!
//! In real time a response comes some milliseconds after its request, so
//! the rendering update
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>)
//! can run between them. In virtual time the loader's thread answers at
//! once, and the completion is handled in the same virtual millisecond. A
//! page that reads the layout or the hover state between the two (a
//! sidebar that changes in steps after a click) then sees a different
//! sequence.
//!
//! Measured in Chromium 148 (probe `tools/probes/network-frames.json`,
//! three runs; `fetch()`, `XMLHttpRequest`, an inserted image and an
//! inserted script alike): a request that a click handler starts completes
//! after 5.6 to 10.4 ms, and exactly one animation frame (rendering update)
//! runs between the request and the completion, in every run (12 of 12). A
//! request that a timer, the parser or the `load` event starts completes
//! after 1.2 to 5.9 ms, and a frame falls between in 7 of 24 single
//! requests and in 11 of 96 requests of chains (as often as a 60 Hz frame
//! falls into a 2 to 5 ms window; the count differs between runs).
//!
//! So in virtual time the completion of a request that was started while
//! an input event was dispatched (a pointer, mouse or key event, with the
//! microtasks of its listeners) waits for the next rendering update after
//! the request. The requests are recognized by their ids: ids grow with
//! each request, so the requests of a dispatch are a range of ids. The
//! rendering update releases the held completions after its steps, in the
//! order of their requests (not of the threads that answered them, so a
//! run does not depend on the threads); a completion that arrives later for a
//! request of an earlier dispatch is handled at once (an update has run
//! since the request).
//!
//! Only after the `load` event (requests that delay it are not held, so the
//! load waits never need a frame) and only when the page runs rendering
//! updates. A held completion counts as pending work: the loops of the
//! event loop do not call the page idle. Virtual time waits until every
//! request of the dispatches has completed (the frame then finds all
//! completions, whatever the order in which the threads answered), and may
//! then jump to the next frame even if other requests are in flight.

use std::collections::VecDeque;

use swb_net::{Completion, RequestId};

use super::Page;

/// Completions that wait for a rendering update, and the requests whose
/// completions will.
#[derive(Debug, Default)]
pub(super) struct HeldCompletions {
    /// The ids `first..end` of the requests that input dispatches started
    /// since the last rendering update, in order and without overlap.
    ranges: Vec<(RequestId, RequestId)>,
    /// The completions that arrived for those requests.
    waiting: VecDeque<Completion>,
}

impl HeldCompletions {
    /// Adds the requests `first..end` (merged with the last range if it
    /// ends at `first`).
    fn add(&mut self, first: RequestId, end: RequestId) {
        match self.ranges.last_mut() {
            Some(last) if last.1 == first => last.1 = end,
            _ => self.ranges.push((first, end)),
        }
    }

    /// The number of requests in the ranges.
    fn total(&self) -> u64 {
        self.ranges.iter().map(|&(first, end)| end - first).sum()
    }

    /// True if the request `id` started in an input dispatch since the last
    /// rendering update.
    fn contains(&self, id: RequestId) -> bool {
        // The ranges are sorted: ids only grow.
        let at = self.ranges.partition_point(|&(_, end)| end <= id);
        self.ranges.get(at).is_some_and(|&(first, _)| first <= id)
    }
}

impl Page {
    /// The id that the next request gets, to bracket an input dispatch for
    /// [`Page::note_input_requests`].
    pub(super) fn next_request_id(&self) -> RequestId {
        self.loader.next_request_id()
    }

    /// True if requests that start now wait for a rendering update: virtual
    /// time, the `load` event has fired, and the page runs rendering
    /// updates.
    fn holds_input_requests(&self) -> bool {
        self.clock.is_virtual()
            && self.runs_rendering_updates()
            && self
                .script
                .as_ref()
                .is_some_and(swb_web::ScriptRuntime::document_loaded)
    }

    /// Notes that an input dispatch started the requests from `first` on
    /// (`first` is [`Page::next_request_id`] from before it): their
    /// completions wait for a rendering update. Asks for one.
    pub(super) fn note_input_requests(&mut self, first: RequestId) {
        let end = self.next_request_id();
        if end > first && self.holds_input_requests() {
            self.held.add(first, end);
            self.request_frame();
        }
    }

    /// Handles a completion that the loader delivered, or holds it for the
    /// next rendering update. Returns true if it was handled and changed
    /// something.
    pub(super) fn accept_completion(&mut self, completion: Completion) -> bool {
        if self.held.contains(completion.id) {
            self.held.waiting.push_back(completion);
            return false;
        }
        self.handle_completion(completion)
    }

    /// True if requests of input dispatches wait for a rendering update:
    /// their completions have not all arrived, or are held.
    pub(super) fn has_held_requests(&self) -> bool {
        !self.held.ranges.is_empty() || !self.held.waiting.is_empty()
    }

    /// True if held completions wait for a rendering update and every
    /// request of the input dispatches has completed, so that the frame
    /// finds all of them (the order of a run does not depend on the
    /// threads). Only then may virtual time jump to the frame while other
    /// work is pending.
    pub(super) fn held_completions_ready(&self) -> bool {
        !self.held.waiting.is_empty() && self.held.waiting.len() as u64 >= self.held.total()
    }

    /// The rendering update ran its steps (or cannot run any more): the
    /// requests of the earlier dispatches are no longer held, and the held
    /// completions are handled, in order. Returns true if one changed
    /// something.
    pub(super) fn release_held_completions(&mut self) -> bool {
        self.held.ranges.clear();
        // The order of the requests, not of the threads that answered them.
        self.held
            .waiting
            .make_contiguous()
            .sort_by_key(|completion| completion.id);
        let mut changed = false;
        while !self.lent
            && let Some(completion) = self.held.waiting.pop_front()
        {
            changed |= self.handle_completion(completion);
        }
        if changed {
            self.note_hover_stale();
        }
        changed
    }

    /// Ends the hold when no rendering update can come (the scripts were
    /// stopped).
    pub(super) fn release_held_if_no_updates(&mut self) -> bool {
        if self.has_held_requests() && !self.runs_rendering_updates() {
            return self.release_held_completions();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::HeldCompletions;

    #[test]
    fn ranges_merge_and_answer_membership() {
        let mut held = HeldCompletions::default();
        held.add(5, 8);
        held.add(8, 9);
        held.add(20, 22);
        assert_eq!(held.ranges, vec![(5, 9), (20, 22)]);
        let ids: Vec<u64> = (0..25).filter(|&id| held.contains(id)).collect();
        assert_eq!(ids, vec![5, 6, 7, 8, 20, 21]);
        assert_eq!(held.total(), 6);
    }
}
