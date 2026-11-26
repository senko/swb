//! The playback API of the media element (M14 feature 6b, ADR 0047
//! section 10; HTML "media elements",
//! <https://html.spec.whatwg.org/multipage/media.html#playing-the-media-resource>):
//! `play()` and `pause()`, seeking, the end and `loop`, `played`, `volume`,
//! `muted`, autoplay and the policy that guards it, and the failure that the
//! pipeline met after the metadata.
//!
//! **The clock.** The pipeline (`swb_media::pipeline`) counts the media time:
//! the element tells it to play at the moment of HTML's "notify about
//! playing" (`playing`), to pause at the internal pause steps, and to seek;
//! it hands back `timeupdate` points, the end and the end of a seek as notes
//! (`step.rs` handles them one at a time). The element's `paused`, `seeking`
//! and `ended` follow the HTML algorithms; the events follow Chromium's order
//! (probes `media-playback.json`, `media-autoplay.json`).
//!
//! **The promises** of `play()` have numbers that grow for each element; the
//! element keeps the highest one that waits for `playing`
//! ([`Element::pending_upto`]) and settles all up to it with one task: HTML
//! settles all pending promises together, so a range of numbers says which.
//!
//! **A late failure.** A failure that the pipeline meets after the metadata
//! (the damage lies later in the file) ends its run: the pipeline cannot play
//! any more. Chromium loads such a file and fails when the playback reaches
//! the damaged packet (`error` with code 3, `timeupdate`, `pause`; ADR 0047
//! "The late failure" has the measured rows). The element keeps the failure
//! ([`Element::damage`]) and plays without the pipeline: it counts the media
//! time on the page clock ([`Emulation`]), fires the `timeupdate` points on
//! the pipeline's grid, and fails where the pipeline says
//! (`Failure::at_us`, `Pipeline::failed_at`: shortly before where the failing
//! thread stopped; it depends on the data, not on the speed of the threads,
//! and in virtual time the playback has not passed it when the failure
//! arrives). The time at which the failure arrives depends on the threads;
//! the playback from there to the damage does not. A failure without a place
//! (the source, a file without audio whose demuxer failed) comes out at the
//! position of the playback, as does one that arrives after the playback
//! passed its place (real time). A seek of such an element ends at once.

use swb_dom::NodeId;
use swb_web::{MediaTask, PlayRejection, PlayStart};

use super::backend::{Fetch, Note};
use super::select::Cx;
use super::{
    Element, Emulation, HAVE_CURRENT_DATA, HAVE_ENOUGH_DATA, HAVE_METADATA, MAX_PENDING_PLAYS,
    Media, NETWORK_EMPTY, NETWORK_NO_SOURCE, Phase,
};

/// `readyState` `HAVE_FUTURE_DATA`: the element can play on (swb shows it
/// only as part of `HAVE_ENOUGH_DATA`).
const HAVE_FUTURE_DATA: u16 = 3;

/// The distance of the `timeupdate` points of a playback that the element
/// counts itself (as the pipeline's, every 250 ms of media time).
const TICK_US: i64 = 250_000;

/// Seconds as whole microseconds (saturating; NaN is 0).
fn to_us(seconds: f64) -> i64 {
    (seconds * 1e6).round() as i64
}

/// Adds the range `start` to `end` to `ranges` (sorted by start): ranges that
/// overlap or touch it merge with it (`TimeRanges`).
pub(super) fn add_range(ranges: &mut Vec<(f64, f64)>, start: f64, end: f64) {
    let (mut start, mut end) = (start, end);
    ranges.retain(|&(s, e)| {
        if s <= end && e >= start {
            start = start.min(s);
            end = end.max(e);
            false
        } else {
            true
        }
    });
    let at = ranges.partition_point(|&(s, _)| s < start);
    ranges.insert(at, (start, end));
}

/// Whether the `loop` attribute is set on `node`.
fn loops(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.dom
        .element(node)
        .is_some_and(|e| e.attr("loop").is_some())
}

/// Whether the `autoplay` attribute is set on `node`.
pub(super) fn autoplay_attribute(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.dom
        .element(node)
        .is_some_and(|e| e.attr("autoplay").is_some())
}

/// Whether the policy lets the element play: the document has had a user
/// activation, or the element is a `video` that is muted (or has the
/// volume 0). Chromium's default policy ("document user activation
/// required"), measured in `media-autoplay.json`.
pub(super) fn allowed_to_play(cx: &Cx<'_>, element: &Element) -> bool {
    cx.activated || (element.video && element.silent())
}

impl Media {
    // ----- play() and pause() -----

    /// `play()`: the `play()` steps of HTML. Says whether the call made a
    /// promise.
    /// <https://html.spec.whatwg.org/multipage/media.html#dom-media-play>
    pub(crate) fn play(&mut self, cx: &Cx<'_>, node: NodeId) -> PlayStart {
        let Some(element) = self.element_mut(cx, node) else {
            return PlayStart::NotSupported;
        };
        if !allowed_to_play(cx, element) {
            return PlayStart::NotAllowed;
        }
        if element.error.as_ref().is_some_and(|e| e.code == 4) {
            return PlayStart::NotSupported;
        }
        if element.play_last - element.play_delivered >= MAX_PENDING_PLAYS {
            return PlayStart::TooMany;
        }
        element.play_last += 1;
        let id = element.play_last;
        element.pending_upto = Some(id);
        self.internal_play(cx, node, !cx.activated);
        PlayStart::Pending(id)
    }

    /// HTML "internal play steps".
    /// <https://html.spec.whatwg.org/multipage/media.html#internal-play-steps>
    /// `exempt`: the policy lets the element play only because it is a muted
    /// `video`.
    fn internal_play(&mut self, cx: &Cx<'_>, node: NodeId, exempt: bool) {
        self.start_selection_if_empty(node);
        self.begin_deferred_fetch(cx, node);
        let restart = self
            .elements
            .get(&node)
            .is_some_and(|e| e.ended && e.meta.is_some());
        if restart {
            // Playback has ended: it starts again at the beginning.
            self.do_seek(node, 0.0);
        }
        let now_ms = self.now_ms();
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        element.can_autoplay = false;
        // HTML internal play steps: the poster goes.
        element.show_poster = false;
        if element.paused {
            element.paused = false;
            element.muted_exempt = exempt;
            // The run of playback that `played` records starts here, also
            // before the metadata (Chromium: a media with a start time of
            // 1.5 s has `played` 0-3.5 when `play()` came first).
            let position = element.position(now_ms);
            element.run_start.get_or_insert(position);
            let ready = element.ready_state >= HAVE_FUTURE_DATA;
            self.queue(node, "play");
            if ready {
                self.notify_playing(node);
            } else {
                self.queue(node, "waiting");
            }
        } else if element.ready_state >= HAVE_FUTURE_DATA {
            self.resolve_pending(node);
        }
    }

    /// An element that no resource was chosen for runs the resource
    /// selection (HTML "internal play steps", step 1).
    fn start_selection_if_empty(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.network_state == NETWORK_EMPTY && element.phase == Phase::Empty {
            element.network_state = NETWORK_NO_SOURCE;
            element.phase = Phase::Selecting;
            self.queue_select(node);
        }
    }

    /// A fetch that `preload=none` held back starts: `play()` asks for the
    /// resource.
    fn begin_deferred_fetch(&mut self, cx: &Cx<'_>, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.phase == Phase::Deferred
            && let Some(url) = element.deferred.take()
        {
            self.begin_fetch(cx, node, &url);
        }
    }

    /// `pause()`: the `pause()` steps of HTML and the "internal pause steps".
    /// <https://html.spec.whatwg.org/multipage/media.html#dom-media-pause>
    pub(crate) fn pause(&mut self, cx: &Cx<'_>, node: NodeId) {
        if self.element_mut(cx, node).is_none() {
            return;
        }
        self.start_selection_if_empty(node);
        self.pause_steps(node, PlayRejection::Pause);
    }

    /// HTML "internal pause steps": the element is paused, its events are
    /// queued and the pending promises are rejected with `why`.
    /// <https://html.spec.whatwg.org/multipage/media.html#internal-pause-steps>
    pub(super) fn pause_steps(&mut self, node: NodeId, why: PlayRejection) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        element.can_autoplay = false;
        if element.paused {
            return;
        }
        element.paused = true;
        element.muted_exempt = false;
        self.stop_clock(node);
        self.queue(node, "timeupdate");
        self.queue(node, "pause");
        self.reject_pending(node, why);
    }

    /// HTML "notify about playing": `playing`, and the pending promises
    /// resolve; the media clock runs.
    pub(super) fn notify_playing(&mut self, node: NodeId) {
        self.queue(node, "playing");
        self.resolve_pending(node);
        self.start_clock(node);
    }

    /// The pending `play()` promises resolve (one task for all of them).
    fn resolve_pending(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if let Some(upto) = element.pending_upto.take() {
            self.push_task(node, node, MediaTask::ResolvePlay(upto));
        }
    }

    /// The pending `play()` promises reject with `why`.
    pub(super) fn reject_pending(&mut self, node: NodeId, why: PlayRejection) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if let Some(upto) = element.pending_upto.take() {
            self.push_task(node, node, MediaTask::RejectPlay(upto, why));
        }
    }

    // ----- The clock -----

    /// The media clock runs from the current position.
    fn start_clock(&mut self, node: NodeId) {
        let (now_us, now_ms) = (self.now_us, self.now_ms());
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let position = element.position(now_ms);
        element.run_start.get_or_insert(position);
        element.frozen = None;
        let failed = element.damage.is_some();
        if failed {
            let start = to_us(position);
            element.start_emulation(start, start.saturating_add(TICK_US), now_us);
        } else if let Some(fetch) = &mut element.fetch {
            fetch.play(now_us);
        }
    }

    /// The media clock stops; the run of playback goes into `played`.
    pub(super) fn stop_clock(&mut self, node: NodeId) {
        let (now_us, now_ms) = (self.now_us, self.now_ms());
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let position = element.position(now_ms);
        if let Some(emulation) = element.emulation.take() {
            element.frozen = Some(emulation.position(now_us) as f64 / 1e6);
        }
        element.commit_played(position);
        if let Some(fetch) = &mut element.fetch {
            fetch.pause(now_us);
        }
    }

    // ----- Seeking -----

    /// `currentTime = seconds`: HTML "seek" (before the metadata, the
    /// default playback start position).
    /// <https://html.spec.whatwg.org/multipage/media.html#dom-media-currenttime>
    pub(crate) fn seek(&mut self, cx: &Cx<'_>, node: NodeId, seconds: f64) {
        let Some(element) = self.element_mut(cx, node) else {
            return;
        };
        if element.meta.is_none() {
            // HTML: the default playback start position holds the value as
            // set (Chromium: -5 reads -5); the seek after the metadata
            // clamps it, and a position that is not above 0 starts at 0.
            element.default_start = Some(if seconds.is_nan() { 0.0 } else { seconds });
            return;
        }
        self.do_seek(node, seconds);
    }

    /// HTML "seek" with the time on the timeline (the metadata is known): the
    /// position is the target (clamped) at once, `seeking` is true and the
    /// event `seeking` is queued; the pipeline seeks, and its end is a note.
    /// <https://html.spec.whatwg.org/multipage/media.html#seeking>
    pub(super) fn do_seek(&mut self, node: NodeId, seconds: f64) {
        let now_ms = self.now_ms();
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let Some(meta) = &element.meta else {
            return;
        };
        // (A file can have a duration that is not positive; `clamp` panics
        // for a maximum below the minimum.)
        let target = if seconds.is_nan() {
            0.0
        } else {
            seconds.max(0.0).min(meta.duration.max(0.0))
        };
        let position = element.position(now_ms);
        element.commit_played(position);
        element.emulation = None;
        element.ended = false;
        element.seeking = true;
        // A seek with data (also to 0) takes the poster away: the metadata
        // is there. One before it only sets the start position (`seek`).
        element.show_poster = false;
        element.frozen = None;
        let was = element.ready_state;
        element.ready_state = element.ready_state.min(HAVE_METADATA);
        let paused = element.paused;
        let accepted = element
            .fetch
            .as_mut()
            .is_some_and(|fetch| fetch.seek((target * 1e6).round() as i64));
        if !accepted {
            // The pipeline failed: nothing can seek. The seek ends at once.
            element.frozen = Some(target);
            if let Some(fetch) = &mut element.fetch {
                fetch.push_note(Note::SeekDone(target));
            }
        }
        self.queue(node, "seeking");
        if was >= HAVE_FUTURE_DATA && !paused {
            self.queue(node, "waiting");
        }
    }

    /// The seek ended at `seconds`: `timeupdate`, `seeked`, and the data is
    /// there again (`canplay`, `playing` if the element plays,
    /// `canplaythrough`). A seek that the load made after the metadata (a
    /// `#t=` fragment, a position that a script set before it) ends before
    /// the first data: the note of the first data follows and is handled in
    /// the same unit (Chromium: `seeked` before `loadeddata`).
    pub(super) fn note_seek_done(&mut self, cx: &Cx<'_>, node: NodeId, position: f64) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if !element.seeking {
            return;
        }
        element.seeking = false;
        element.frozen = element.frozen.map(|_| position);
        let first = !element.first_data;
        let playing = !element.paused;
        if playing {
            element.run_start = Some(position);
        }
        self.queue(node, "timeupdate");
        self.queue(node, "seeked");
        if first {
            if let Some(Note::FirstData) = self
                .elements
                .get_mut(&node)
                .and_then(|e| e.fetch.as_mut())
                .and_then(Fetch::next_note)
            {
                self.first_data(cx, node);
            }
            return;
        }
        self.data_again(node);
    }

    /// The data is there again after a seek: `canplay`, `playing`,
    /// `canplaythrough`.
    fn data_again(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        element.ready_state = HAVE_ENOUGH_DATA;
        let playing = !element.paused;
        self.queue(node, "canplay");
        if playing {
            self.notify_playing(node);
        }
        self.queue(node, "canplaythrough");
    }

    // ----- Notes of the pipeline while it plays -----

    /// A `timeupdate` point of the playback.
    pub(super) fn note_time_update(&mut self, node: NodeId) {
        if self
            .elements
            .get(&node)
            .is_some_and(|e| !e.paused && !e.seeking)
        {
            self.queue(node, "timeupdate");
        }
    }

    /// Playback ran out of decoded data (real time): `waiting`.
    pub(super) fn note_underrun(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.paused || element.seeking || element.ready_state < HAVE_ENOUGH_DATA {
            return;
        }
        element.ready_state = HAVE_CURRENT_DATA;
        self.queue(node, "waiting");
    }

    /// Playback goes on after an underrun: `canplay`, `playing`.
    pub(super) fn note_resumed(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.ready_state != HAVE_CURRENT_DATA {
            return;
        }
        element.ready_state = HAVE_ENOUGH_DATA;
        let playing = !element.paused;
        self.queue(node, "canplay");
        if playing {
            self.notify_playing(node);
        }
    }

    /// Playback reached the end of the media (HTML "when the current
    /// playback position reaches the end of the media resource"):
    /// `timeupdate`; with `loop` the playback seeks to the start and goes
    /// on; else the element pauses (`pause`) and `ended` follows.
    pub(super) fn note_ended(&mut self, cx: &Cx<'_>, node: NodeId) {
        let now_ms = self.now_ms();
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.paused || element.seeking {
            return;
        }
        let position = element.position(now_ms);
        // A run that ends where it began leaves no range (Chromium: `play()`
        // at the end after a seek to the end has `played` empty; a pause
        // without time does leave one).
        if element.run_start.is_some_and(|start| start < position) {
            element.commit_played(position);
        }
        element.run_start = None;
        self.queue(node, "timeupdate");
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if loops(cx, node) {
            // The seek to the start is the next unit: the `timeupdate`
            // is dispatched at the end of the media (`ct` 2), as Chromium's.
            element.loop_pending = true;
            return;
        }
        element.paused = true;
        element.ended = true;
        element.muted_exempt = false;
        self.queue(node, "pause");
        self.reject_pending(node, PlayRejection::Pause);
        self.queue(node, "ended");
    }

    /// The unit after the end of a looping media: the playback seeks to the
    /// start and goes on, unless a listener paused or seeked meanwhile.
    pub(super) fn step_loop(&mut self, node: NodeId) -> bool {
        let now_us = self.now_us;
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        if !std::mem::take(&mut element.loop_pending) {
            return false;
        }
        if element.paused || element.seeking {
            return true;
        }
        self.do_seek(node, 0.0);
        // The pipeline stopped at the end: it plays again once the seek is
        // done.
        if let Some(fetch) = self.elements.get_mut(&node).and_then(|e| e.fetch.as_mut()) {
            fetch.play(now_us);
        }
        true
    }

    // ----- Autoplay -----

    /// The first data came (`loadeddata`, `canplay`, `canplaythrough`) and
    /// the element may start by itself (HTML "ready state" changes): an
    /// element that `play()` started goes on (`playing` between `canplay`
    /// and `canplaythrough`); an `autoplay` element that the policy allows
    /// starts (`play`, `playing`). Chromium's order differs: with a user
    /// activation the start comes with `canplay`, without one (a muted
    /// `video`) after `canplaythrough` (probe `media-autoplay.json`).
    pub(super) fn first_data(&mut self, cx: &Cx<'_>, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        element.first_data = true;
        element.ready_state = HAVE_ENOUGH_DATA;
        element.idle_since = None;
        let autoplay = element.paused
            && element.can_autoplay
            && autoplay_attribute(cx, node)
            && allowed_to_play(cx, element);
        self.queue(node, "loadeddata");
        if autoplay && cx.activated {
            self.queue(node, "canplay");
            self.autoplay_start(node, false);
            self.notify_playing(node);
            self.queue(node, "canplaythrough");
            return;
        }
        let playing = !self.elements.get(&node).is_none_or(|e| e.paused);
        self.queue(node, "canplay");
        if playing {
            self.notify_playing(node);
        }
        self.queue(node, "canplaythrough");
        if autoplay && let Some(element) = self.elements.get_mut(&node) {
            // The start is the next unit: the three events above are
            // dispatched while the element is paused (Chromium).
            element.autoplay_pending = true;
        }
    }

    /// The unit after the first data of an `autoplay` element that the
    /// policy allows as a muted `video`: it starts (`play`, `playing`),
    /// unless a listener played, paused or changed the volume meanwhile.
    pub(super) fn step_autoplay(&mut self, cx: &Cx<'_>, node: NodeId) -> bool {
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        if !std::mem::take(&mut element.autoplay_pending) {
            return false;
        }
        if element.paused && element.can_autoplay && allowed_to_play(cx, element) {
            self.autoplay_start(node, true);
            self.notify_playing(node);
        }
        true
    }

    /// The `autoplay` attribute starts the element: it is not paused, `play`
    /// is queued. `exempt`: the policy allowed it as a muted `video`.
    fn autoplay_start(&mut self, node: NodeId, exempt: bool) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        element.paused = false;
        element.can_autoplay = false;
        element.show_poster = false;
        element.muted_exempt = exempt;
        self.queue(node, "play");
    }

    // ----- volume and muted -----

    /// `volume = value` (in 0 to 1).
    pub(crate) fn set_volume(&mut self, cx: &Cx<'_>, node: NodeId, volume: f64) {
        let Some(element) = self.element_mut(cx, node) else {
            return;
        };
        if element.volume == volume {
            return;
        }
        let was_silent = element.silent();
        element.volume = volume;
        self.volume_changed(cx, node, was_silent);
    }

    /// `muted = value`.
    pub(crate) fn set_muted(&mut self, cx: &Cx<'_>, node: NodeId, muted: bool) {
        let Some(element) = self.element_mut(cx, node) else {
            return;
        };
        if element.muted == muted {
            return;
        }
        let was_silent = element.silent();
        element.muted = muted;
        self.volume_changed(cx, node, was_silent);
    }

    /// The volume or the muted state changed: `volumechange`, the gain of
    /// the pipeline; an element that plays only because it was silent pauses
    /// when it is not any more and the user has done nothing (Chromium).
    fn volume_changed(&mut self, cx: &Cx<'_>, node: NodeId, was_silent: bool) {
        self.queue(node, "volumechange");
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let gain = if element.muted {
            0.0
        } else {
            element.volume as f32
        };
        if let Some(fetch) = &mut element.fetch {
            fetch.set_gain(gain);
        }
        let loud = !element.silent();
        if was_silent && loud && element.muted_exempt && !element.paused && !cx.activated {
            self.pause_steps(node, PlayRejection::Pause);
        }
    }

    /// `playbackRate = value`: the value is kept and `ratechange` fires; the
    /// playback runs at 1 (backlog).
    pub(crate) fn set_rate(&mut self, cx: &Cx<'_>, node: NodeId, rate: f64, default: bool) {
        let Some(element) = self.element_mut(cx, node) else {
            return;
        };
        let slot = if default {
            &mut element.default_rate
        } else {
            &mut element.rate
        };
        if *slot == rate {
            return;
        }
        *slot = rate;
        self.queue(node, "ratechange");
    }

    // ----- A failure after the metadata -----

    /// One unit of the playback that counts its own time: the next
    /// `timeupdate`, or the failure at the damage. True if it did one.
    pub(super) fn step_emulation(&mut self, node: NodeId) -> bool {
        let now_us = self.now_us;
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        let Some(emulation) = element.emulation.as_mut() else {
            return false;
        };
        if now_us < emulation.next_due() {
            return false;
        }
        if emulation.position(now_us) >= emulation.deadline {
            self.damage_error(node);
        } else {
            emulation.next_tick = emulation.next_tick.saturating_add(TICK_US);
            self.queue(node, "timeupdate");
        }
        true
    }

    /// The playback reached the damage: `error` with the failure's code, then
    /// the element pauses (`timeupdate`, `pause`), as Chromium does.
    fn damage_error(&mut self, node: NodeId) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let Some(failure) = element.damage.clone() else {
            return;
        };
        let at = element
            .emulation
            .take()
            .map_or(0.0, |e| e.deadline as f64 / 1e6);
        element.frozen = Some(at);
        element.commit_played(at);
        element.paused = true;
        element.muted_exempt = false;
        let error = self.new_error(failure.code, &failure.message);
        if let Some(element) = self.elements.get_mut(&node) {
            element.error = Some(error);
        }
        self.queue(node, "error");
        self.queue(node, "timeupdate");
        self.queue(node, "pause");
        self.reject_pending(node, PlayRejection::Pause);
    }

    /// The pipeline of `node` failed after the metadata (its `Fetch::late`):
    /// the element keeps the failure, and a playback that goes on counts its
    /// own time from here.
    pub(super) fn adopt_damage(&mut self, node: NodeId) {
        let now_us = self.now_us;
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if element.damage.is_some() {
            return;
        }
        let Some(failure) = element.fetch.as_ref().and_then(|f| f.late()).cloned() else {
            return;
        };
        element.damage = Some(failure);
        let playing = !element.paused && !element.seeking && element.run_start.is_some();
        if playing {
            // The playback goes on from here to the damage, on the points
            // of the pipeline's grid (the failure arrives when the threads
            // meet the damage, so the position here depends on their speed;
            // what follows does not).
            let Some(fetch) = element.fetch.as_ref() else {
                return;
            };
            let position = to_us(fetch.position_at(now_us));
            let next_tick = fetch
                .next_timeupdate_us()
                .unwrap_or(position.saturating_add(TICK_US));
            element.start_emulation(position, next_tick, now_us);
        }
    }
}

impl Element {
    /// The run of playback that began at `run_start` ends at `position`: it
    /// joins `played`. An element without metadata has no media to have
    /// played: a `play()` and `pause()` before it leave `played` empty
    /// (Chromium).
    pub(super) fn commit_played(&mut self, position: f64) {
        if let Some(start) = self.run_start.take()
            && self.meta.is_some()
        {
            add_range(&mut self.played, start, position.max(start));
        }
    }

    /// Playback that counts its own time starts at the media time `position`
    /// at page time `now` (microseconds); its next `timeupdate` is at
    /// `next_tick`. It fails at the damage (see the module comment, "A late
    /// failure"): never before `position`, never after the duration.
    pub(super) fn start_emulation(&mut self, position: i64, next_tick: i64, now: i64) {
        let duration = self.meta.as_ref().map_or(i64::MAX, |m| to_us(m.duration));
        let at = self
            .damage
            .as_ref()
            .and_then(|failure| failure.at_us)
            .unwrap_or(position);
        self.frozen = None;
        self.emulation = Some(Emulation {
            anchor_media: position,
            anchor: now,
            next_tick,
            deadline: at.min(duration).max(position),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::add_range;

    #[test]
    fn ranges_merge_when_they_touch_or_overlap() {
        let mut r = Vec::new();
        add_range(&mut r, 0.0, 0.5);
        add_range(&mut r, 6.0, 6.5);
        assert_eq!(r, vec![(0.0, 0.5), (6.0, 6.5)]);
        add_range(&mut r, 0.5, 1.0);
        assert_eq!(r, vec![(0.0, 1.0), (6.0, 6.5)]);
        add_range(&mut r, 0.8, 6.2);
        assert_eq!(r, vec![(0.0, 6.5)]);
        // A range without length stays.
        add_range(&mut r, 9.0, 9.0);
        assert_eq!(r, vec![(0.0, 6.5), (9.0, 9.0)]);
    }
}
