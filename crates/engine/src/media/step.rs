//! What the page's pump does with the media elements: one *unit* of work
//! for each element per step (the stable-state part of a selection, or one
//! note of a fetch, or a step of the playback that counts its own time, or
//! the suspend of an idle pipeline). The page delivers the events that a
//! unit queued before it asks for the next one, so the state that listeners
//! read is the same on every run (see the module comment of `mod.rs`,
//! "Lockstep").

use swb_dom::NodeId;

use super::backend::{Fetch, Meta, Note};
use super::select::Cx;
use super::{
    Element, Entry, HAVE_ENOUGH_DATA, HAVE_METADATA, Media, NETWORK_IDLE, NETWORK_LOADING, Phase,
    SUSPEND_AFTER_US,
};

/// The start time of a media fragment (`#t=10`, `#t=npt:1:30.5,2:00`; Media
/// Fragments URI 1.0, "temporal dimension", normal play time): seconds.
/// <https://www.w3.org/TR/media-frags/#naming-time>
pub(super) fn fragment_start(url: &str) -> Option<f64> {
    let (_, fragment) = url.split_once('#')?;
    let value = fragment
        .split('&')
        .find_map(|pair| pair.strip_prefix("t="))?;
    let value = value.strip_prefix("npt:").unwrap_or(value);
    let start = value.split(',').next()?;
    if start.is_empty() {
        return None;
    }
    let mut seconds = 0.0;
    for part in start.split(':') {
        let number: f64 = part
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
        seconds = seconds * 60.0 + number;
    }
    Some(seconds)
}

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
        self.now_us = super::ms_to_us(cx.now_ms);
        // The stable-state steps of the selections whose marker is at the
        // front of the queue: the tasks before it were delivered. A marker
        // of an older epoch (a later load replaced it) is dropped.
        let mut progressed = false;
        while let Some(&Entry::Select { node, epoch }) = self.events.front() {
            self.events.pop_front();
            // A stale marker counts as progress too: the tasks behind it
            // wait for the next round of the page.
            progressed = true;
            if self
                .elements
                .get(&node)
                .is_some_and(|e| e.epoch == epoch && e.phase == Phase::Selecting)
            {
                self.selection_step(cx, node);
                if strict {
                    return true;
                }
            }
        }
        // The unit after a failed source: its events were delivered.
        while let Some(node) = self.failed.pop_first() {
            if self.step_failed(node) {
                progressed = true;
                if strict {
                    return true;
                }
            }
        }
        let nodes: Vec<NodeId> = self
            .elements
            .iter()
            .filter(|(_, e)| e.phase == Phase::Fetching)
            .map(|(&node, _)| node)
            .collect();
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
        if self.step_loop(node) || self.step_autoplay(cx, node) || self.step_emulation(node) {
            return true;
        }
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        let Some(fetch) = element.fetch.as_mut() else {
            return false;
        };
        if fetch.is_suspended() {
            return false;
        }
        fetch.poll(now_us);
        let has_notes = fetch.has_notes();
        self.adopt_damage(node);
        if has_notes {
            return self.step_note(cx, node, strict);
        }
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        let Some(fetch) = element.fetch.as_mut() else {
            return false;
        };
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
        // An idle pipeline with all its data suspends after 15 s (not while
        // the element plays, seeks or counts its own time).
        let idle = element.ready_state == HAVE_ENOUGH_DATA
            && element.paused
            && !element.seeking
            && element.emulation.is_none()
            && fetch.can_suspend();
        match (idle, element.idle_since) {
            (true, None) => {
                element.idle_since = Some(now_us);
                false
            }
            // Whole microseconds on both sides: the due time (`since` + 15 s)
            // goes through the page clock in milliseconds and comes back as
            // the same microsecond (`ms_to_us` rounds), so it needs no margin.
            (true, Some(since)) if now_us.saturating_sub(since) >= SUSPEND_AFTER_US => {
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
    /// source to settle, and the first data and the end of a seek for enough
    /// data, so that the state that the events show does not depend on the
    /// speed of the threads and the network. In real time each note is
    /// handled when it comes, as Chromium fires `loadedmetadata` at the parse
    /// and `loadeddata` at the first frame, also on a slow link.
    fn step_note(&mut self, cx: &Cx<'_>, node: NodeId, strict: bool) -> bool {
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        let first_data = element.first_data;
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
        // The end of a seek waits for enough data too; a seek that ends
        // before the first data brings the note of the first data with it.
        if strict
            && fetch.front_is_seek_done()
            && (!fetch.enough() || (!first_data && !fetch.second_is_first_data()))
        {
            return false;
        }
        let Some(note) = fetch.next_note() else {
            return false;
        };
        match note {
            Note::Metadata(meta) => self.metadata(node, meta),
            Note::FirstData => self.first_data(cx, node),
            Note::Failed(failure) => self.load_failed(cx, node, &failure),
            Note::SeekDone(position) => self.note_seek_done(cx, node, position),
            Note::TimeUpdate => self.note_time_update(node),
            Note::Underrun => self.note_underrun(node),
            Note::Resumed => self.note_resumed(node),
            Note::Ended => self.note_ended(cx, node),
        }
        true
    }

    /// The metadata arrived: `progress` and `suspend` (when the network is
    /// idle) come first, as the bytes arrived before the demuxer opened
    /// them; then `durationchange`, `resize` (a video with a size) and
    /// `loadedmetadata`. Then the playback goes to its start position, if it
    /// has one (HTML "media element load algorithm", "once the metadata has
    /// been loaded"): the position that a script set before, or the start of
    /// a media fragment.
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
        let start = element
            .default_start
            .take()
            .filter(|&t| t > 0.0)
            .or_else(|| fragment_start(&element.current_src).filter(|&t| t > 0.0));
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
        if let Some(start) = start {
            self.do_seek(node, start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fragment_start;

    #[test]
    fn a_media_fragment_gives_its_start_time() {
        assert_eq!(fragment_start("a.mp4#t=1.0"), Some(1.0));
        assert_eq!(fragment_start("a.mp4#t=10"), Some(10.0));
        assert_eq!(fragment_start("a.mp4#t=npt:1:30.5,2:00"), Some(90.5));
        assert_eq!(fragment_start("a.mp4#t=0:01:00"), Some(60.0));
        assert_eq!(fragment_start("a.mp4#x=1&t=3"), Some(3.0));
        assert_eq!(fragment_start("a.mp4#t=,20"), None);
        assert_eq!(fragment_start("a.mp4#t=x"), None);
        assert_eq!(fragment_start("a.mp4#t=-1"), None);
        assert_eq!(fragment_start("a.mp4"), None);
    }
}
