//! The page's side of the media elements (M14 feature 6a, ADR 0047 sections
//! 6, 7 and 10): `Page::pump` steps the media of each document and delivers
//! the events that the steps queued; virtual time waits for the media that
//! have pending work and jumps to the next suspend time.
//!
//! A step changes the state of each element by one unit and queues its
//! events (`crate::media`); the events are delivered (each a task) before the
//! next step, so what a listener reads is the same on every run. The
//! pipelines call the loader's waker from their threads (a completion that no
//! request owns), so the loops that wait for completions wake up for them.

use swb_dom::NodeId;
use swb_web::MediaInfo;

use super::Page;
use crate::document::DocumentState;
use crate::frames::FrameId;

/// The most rounds of a pump: a step and the delivery of its events. A
/// page whose listeners start loads again and again does not hold the pump.
const MAX_ROUNDS: usize = 64;

/// How long (real time) a screenshot waits for the media to settle; then it
/// draws what is there.
const SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// The most steps of a settle (each one a [`Page::pump_media`]). A listener
/// that starts a load from its own event again and again does not hold the
/// screenshot.
const SETTLE_ROUNDS: usize = 256;

impl Page {
    /// The documents of the page, the top-level one first.
    fn media_frames(&self) -> Vec<FrameId> {
        let mut ids = vec![FrameId::TOP];
        if self.frames.count() > 0 {
            ids.extend(self.frames.tree_order().into_iter().skip(1));
        }
        ids
    }

    /// Delivers the events that the documents queued, steps the media
    /// elements, and does it again until nothing changes (at most
    /// [`MAX_ROUNDS`] times). The events come first: those that a script's
    /// `src` or `load()` queued (`abort`, `emptied`) are delivered in the
    /// state that the synchronous part of the load algorithm left
    /// (`NETWORK_NO_SOURCE`), before the stable-state part of the resource
    /// selection runs, as in Chromium. Returns true if anything changed.
    pub(super) fn pump_media(&mut self) -> bool {
        self.pump_media_until(None)
    }

    /// [`Self::pump_media`] that stops between two rounds once `deadline`
    /// has passed (a round runs the listeners of its events, which can take
    /// long; the deadline does not interrupt a round).
    fn pump_media_until(&mut self, deadline: Option<std::time::Instant>) -> bool {
        if self.lent {
            return false;
        }
        let mut changed = false;
        for _ in 0..MAX_ROUNDS {
            if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
                break;
            }
            let delivered = self.deliver_media_events();
            let now = self.clock.now();
            let strict = self.clock.is_virtual();
            let mut progressed = false;
            for frame in self.media_frames() {
                if let Some(doc) = self.doc_of_mut(frame) {
                    progressed |= doc.media_step(now, strict);
                }
            }
            if !progressed && !delivered {
                break;
            }
            changed = true;
        }
        changed
    }

    /// Hands the media events that the documents queued to their runtimes
    /// (one slice each). Returns true if any events were taken.
    fn deliver_media_events(&mut self) -> bool {
        let mut delivered = false;
        for frame in self.media_frames() {
            let events = match self.doc_of_mut(frame) {
                Some(doc) if doc.has_media_events() => doc.take_media_events(),
                _ => continue,
            };
            delivered = true;
            // Without a realm nothing listens; the events go.
            if self.frame_runtime(frame).is_none() || events.is_empty() {
                continue;
            }
            self.run_task_in(frame, |runtime| runtime.media_events(events));
            self.check_load_event();
        }
        delivered
    }

    /// True if virtual time must wait for a media element: it opens a
    /// resource, or has data to settle (the loops that wait for the page's
    /// activity wait for it, as for a request in flight).
    pub(super) fn media_pending(&self) -> bool {
        self.media_frames()
            .into_iter()
            .any(|frame| self.doc_of(frame).is_some_and(DocumentState::media_pending))
    }

    /// The page time of the next media due time (the suspend of an idle
    /// pipeline), if any.
    pub(super) fn media_next_due(&self) -> Option<f64> {
        self.media_frames()
            .into_iter()
            .filter_map(|frame| self.doc_of(frame)?.media_next_due())
            .reduce(f64::min)
    }

    /// Brings the images of the videos to the media time of the page clock
    /// before a paint (ADR 0047 section 9): each pipeline takes the frame
    /// that is due, and a new frame bumps the generation of its image. In
    /// virtual time it does nothing while a media still has pending work:
    /// the pipeline's data for the page time is not all decoded, and a
    /// poll then would report an underrun that virtual time never has
    /// (`Page::screenshot` settles first).
    pub(super) fn sync_media_frames(&mut self) {
        if self.lent || (self.clock.is_virtual() && self.media_pending()) {
            return;
        }
        let now = crate::media::ms_to_us(self.clock.now());
        for frame in self.media_frames() {
            if let Some(doc) = self.doc_of_mut(frame) {
                doc.sync_media_images(Some(now));
            }
        }
    }

    /// In virtual time, runs the pipelines of the page's videos to the
    /// state that a paint at the page time shows (a hidden video that got a
    /// box has a frame to decode), so that a screenshot is the same on every
    /// run, also under load (ADR 0047 section 9). The layout comes first:
    /// it tells which videos have a box. Does nothing without media
    /// elements, in real time, and in a task.
    pub(super) fn settle_media_for_screenshot(&mut self) {
        if self.lent
            || !self.clock.is_virtual()
            || !self
                .media_frames()
                .into_iter()
                .any(|frame| self.doc_of(frame).is_some_and(DocumentState::has_media))
        {
            return;
        }
        self.update_layout();
        if self.media_pending() && !self.settle_media() {
            log::warn!("a media element did not settle in time; the screenshot may miss a frame");
        }
    }

    /// Steps the media of the page until none has pending work (its
    /// pipeline decodes what the page time needs), at most [`SETTLE_ROUNDS`]
    /// steps and [`SETTLE_TIMEOUT`] of real time; false if it gave up. It
    /// runs no page task of its own (no timer, no animation frame, no
    /// script): only the media steps and the delivery of the events that they
    /// queue, which the lockstep needs (`crate::media`: a step waits for the
    /// events of the one before). While it waits for the pipelines it accepts
    /// the loader completions that arrive (a response of the page's fetches
    /// is handled as in the page's own loop). A listener runs for each such
    /// event, so a listener that starts a load from its event is bounded by
    /// the rounds and the deadline; one that starts it from a timer does not
    /// run at all. The deadline is checked between the rounds of the media
    /// steps, not inside a listener: a slow listener delays the return by its
    /// own run time. The page time does not move.
    fn settle_media(&mut self) -> bool {
        let deadline = std::time::Instant::now() + SETTLE_TIMEOUT;
        for _ in 0..SETTLE_ROUNDS {
            if !self.media_pending() {
                return true;
            }
            if self.lent || std::time::Instant::now() >= deadline {
                return false;
            }
            if self.pump_media_until(Some(deadline)) {
                continue;
            }
            // The pipelines wake the loader when they have something.
            let wait = deadline.saturating_duration_since(std::time::Instant::now());
            if wait.is_zero() {
                return false;
            }
            if let Some(completion) = self.loader.recv_timeout(wait) {
                self.accept_completion(completion);
            }
        }
        !self.media_pending()
    }

    /// Turns the loads of the media elements of the page's documents on (the
    /// default) or off; the setting applies to the loads that start after
    /// it. Off, a media element behaves as before M14: it loads nothing and
    /// has no events. For tests whose Chromium recordings play media that
    /// swb cannot (Media Source Extensions come in M15): the M13 exit test
    /// of videojs.org, where the player's HLS source would fail and Video.js
    /// would show an error dialog that the recording does not have.
    pub fn set_media(&mut self, enabled: bool) {
        self.maker.media.set_enabled(enabled);
    }

    /// Exists for tests only (the product never calls it). Gives the page
    /// 16 places for open pipelines of its own, instead of
    /// the process's 16 (ADR 0047 section 6), for the pipelines that start
    /// from now on. For tests that run pages in parallel in one process, so
    /// that they do not wait for each other's places. `handover_delay`
    /// (zero but in the tests of the handover) makes the last thread of a
    /// run that ended wait that long before it hands its place on, as a
    /// slow thread would. Without the feature `media` it does nothing.
    pub fn isolate_media(&mut self, handover_delay: std::time::Duration) {
        self.maker.media.isolate(handover_delay);
    }

    /// The number of media elements of the page whose pipeline runs (not
    /// suspended, failed or waiting for a place): at most 16 run at a time
    /// (ADR 0047 section 6). For the tests and the automation.
    pub fn media_pipelines(&self) -> usize {
        self.media_frames()
            .into_iter()
            .filter_map(|frame| self.doc_of(frame))
            .map(DocumentState::media_open_fetches)
            .sum()
    }

    /// The video frames that the pipeline of the media element `node` of the
    /// top-level document converted to pixels (a hidden video converts
    /// none, ADR 0047 section 6). For tests; `None` for an element without
    /// a pipeline, and without the feature `media`.
    pub fn media_frames_converted(&self, node: NodeId) -> Option<u64> {
        self.doc.as_deref()?.media_frames_converted(node)
    }

    /// What the media element `node` of the top-level document reports (the
    /// state that scripts read), for the tests and the automation of the
    /// media element. `None` without a document.
    pub fn media_info(&mut self, node: NodeId) -> Option<MediaInfo> {
        self.doc.as_deref_mut().map(|doc| doc.media_info(node))
    }
}
