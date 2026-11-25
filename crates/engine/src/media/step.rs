//! What the page's pump does with the media elements: one *unit* of work
//! for each element per step (the stable-state part of a selection, or one
//! note of a fetch, or the suspend of an idle pipeline). The page delivers
//! the events that a unit queued before it asks for the next one, so the
//! state that listeners read is the same on every run (see the module
//! comment of `mod.rs`, "Lockstep").

use swb_dom::NodeId;

use super::backend::{Fetch, Meta, Note};
use super::select::Cx;
use super::{
    Element, HAVE_ENOUGH_DATA, HAVE_METADATA, Media, NETWORK_IDLE, NETWORK_LOADING, Phase,
    SUSPEND_AFTER_MS,
};

impl Media {
    /// One unit of work for each element, at page time `cx.now_ms`.
    /// Returns true if any element did something (the page then delivers
    /// the events and steps again).
    ///
    /// With `strict` (virtual time) only one unit of work is done at a time,
    /// by the first element (in the order of the nodes) that can do one; an
    /// element that waits for its threads holds the later ones back. So the
    /// events of different elements come in the same order on every run,
    /// whichever thread was faster. In real time every element goes on as
    /// soon as it can.
    pub(crate) fn step(&mut self, cx: &Cx<'_>, strict: bool) -> bool {
        self.now_us = (cx.now_ms * 1000.0) as i64;
        let nodes: Vec<NodeId> = self
            .elements
            .iter()
            .filter(|(_, e)| matches!(e.phase, Phase::Selecting | Phase::Fetching))
            .map(|(&node, _)| node)
            .collect();
        let mut progressed = false;
        for node in nodes {
            if self.step_element(cx, node, strict) {
                progressed = true;
                if strict {
                    break;
                }
            } else if strict && self.elements.get(&node).is_some_and(Element::pending) {
                break;
            }
        }
        progressed
    }

    fn step_element(&mut self, cx: &Cx<'_>, node: NodeId, strict: bool) -> bool {
        let now_us = self.now_us;
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        if element.phase == Phase::Selecting {
            self.selection_step(cx, node);
            return true;
        }
        let Some(fetch) = element.fetch.as_mut() else {
            return false;
        };
        if fetch.is_suspended() {
            return false;
        }
        fetch.poll(now_us);
        if fetch.has_notes() {
            return self.step_note(cx, node, strict);
        }
        // The network went idle (the cache holds what the read-ahead asks
        // for): `suspend`.
        if element.meta.is_some()
            && !element.suspend_sent
            && element.network_state == NETWORK_LOADING
            && fetch.net_idle()
        {
            element.suspend_sent = true;
            element.network_state = NETWORK_IDLE;
            self.queue(node, "suspend");
            return true;
        }
        // An idle pipeline with all its data suspends after 15 s.
        let idle = element.ready_state == HAVE_ENOUGH_DATA && fetch.can_suspend();
        match (idle, element.idle_since) {
            (true, None) => {
                element.idle_since = Some(cx.now_ms);
                false
            }
            (true, Some(since)) if cx.now_ms - since >= SUSPEND_AFTER_MS => {
                fetch.suspend(now_us);
                element.idle_since = None;
                true
            }
            (false, Some(_)) => {
                element.idle_since = None;
                false
            }
            _ => false,
        }
    }

    /// Handles the first note of the fetch of `node`, if it can be handled
    /// now. With `strict` (virtual time) the metadata waits for the byte
    /// source to settle and the first data for enough data, so that the
    /// state that the events show does not depend on the speed of the
    /// threads and the network. In real time each note is handled when it
    /// comes, as Chromium fires `loadedmetadata` at the parse and
    /// `loadeddata` at the first frame, also on a slow link.
    fn step_note(&mut self, cx: &Cx<'_>, node: NodeId, strict: bool) -> bool {
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        let Some(fetch) = element.fetch.as_mut() else {
            return false;
        };
        if strict && fetch.net_unsettled() {
            return false;
        }
        // Chromium goes from `HAVE_METADATA` to `HAVE_ENOUGH_DATA` in one
        // step: `loadeddata` comes with `canplay` and `canplaythrough`.
        if strict && fetch.front_is_first_data() && !fetch.enough() {
            return false;
        }
        let Some(note) = fetch.next_note() else {
            return false;
        };
        match note {
            Note::Metadata(meta) => self.metadata(node, meta),
            Note::FirstData => {
                if let Some(element) = self.elements.get_mut(&node) {
                    element.ready_state = HAVE_ENOUGH_DATA;
                    element.idle_since = None;
                }
                self.queue(node, "loadeddata");
                self.queue(node, "canplay");
                self.queue(node, "canplaythrough");
            }
            Note::Failed(failure) => self.load_failed(cx, node, &failure),
        }
        true
    }

    /// The metadata arrived: `progress` and `suspend` (when the network is
    /// idle) come first, as the bytes arrived before the demuxer opened
    /// them; then `durationchange`, `resize` (a video with a size) and
    /// `loadedmetadata`.
    fn metadata(&mut self, node: NodeId, meta: Meta) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let size = meta.size.filter(|_| element.video);
        let progress = !std::mem::replace(&mut element.progress_sent, true);
        let net_idle = element.fetch.as_ref().is_some_and(Fetch::net_idle);
        let suspend = net_idle && !std::mem::replace(&mut element.suspend_sent, true);
        if suspend {
            element.network_state = NETWORK_IDLE;
        }
        element.meta = Some(meta);
        element.ready_state = HAVE_METADATA;
        if progress {
            self.queue(node, "progress");
        }
        if suspend {
            self.queue(node, "suspend");
        }
        self.queue(node, "durationchange");
        if size.is_some() {
            self.queue(node, "resize");
        }
        self.queue(node, "loadedmetadata");
    }
}
