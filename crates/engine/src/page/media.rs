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
use crate::document::{DocumentState, MediaGeometry};
use crate::frames::FrameId;

/// What one frame adds to [`FramesShownKey`].
#[derive(Clone, Debug, PartialEq)]
struct FrameKey {
    id: FrameId,
    parent: Option<FrameId>,
    container: Option<NodeId>,
    has_box: bool,
    has_media: bool,
    /// What the frame's document was told last.
    shown: bool,
    parent_shown: bool,
    /// The inputs of the window test of the container in the parent.
    parent_geometry: Option<MediaGeometry>,
}

/// What [`Page::update_frames_shown`] depends on.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FramesShownKey {
    hidden: bool,
    frames: Vec<FrameKey>,
}

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

    /// The page time of the next media due time (the frames of the painted
    /// videos, the `timeupdate` points and ends, the suspend of an idle
    /// pipeline), if any. In real time a video that is not painted has no
    /// frame times: nobody sees them (`Pipeline::next_wake`); virtual time
    /// keeps them, so its jumps do not depend on the paint.
    pub(super) fn media_next_due(&self) -> Option<f64> {
        let coarse = !self.clock.is_virtual();
        let now = crate::media::ms_to_us(self.clock.now());
        self.media_frames()
            .into_iter()
            .filter_map(|frame| {
                let doc = self.doc_of(frame)?;
                // The media's own due times, and the changes of the default
                // controls that time brings (the auto-hide, the fade).
                [
                    doc.media_next_due(coarse, now),
                    doc.media_controls_next_due(now),
                ]
                .into_iter()
                .flatten()
                .reduce(f64::min)
            })
            .reduce(f64::min)
    }

    /// True if a video that is painted has a converted frame due at the page
    /// time now that it does not show yet (M14 feature 7b): the frame is
    /// outdated, and the next render draws it ([`Page::frame_outdated`]). It
    /// polls and converts nothing; for each video it reads the frame queue
    /// of a running pipeline under a short lock. The video thread converts
    /// the frames ahead of time, so no frame waits for a conversion here.
    pub(super) fn media_frame_due(&self) -> bool {
        if self.lent {
            return false;
        }
        let now = crate::media::ms_to_us(self.clock.now());
        self.media_frames().into_iter().any(|frame| {
            self.doc_of(frame)
                .is_some_and(|d| d.media_frame_due(now) || d.media_controls_stale(now))
        })
    }

    /// The page time (ms) of the next change of the default controls'
    /// picture that time brings (the auto-hide, a fade, the loading arc):
    /// `None` when nothing changes without input (tests: a paused video
    /// with a still pointer, and a playing one with hidden controls, cause
    /// no wake-up).
    #[doc(hidden)]
    pub fn media_controls_due(&self) -> Option<f64> {
        let now = self.controls_now();
        self.media_frames()
            .into_iter()
            .filter_map(|frame| self.doc_of(frame)?.media_controls_next_due(now))
            .reduce(f64::min)
    }

    /// The page time in microseconds, for the default controls.
    fn controls_now(&self) -> i64 {
        crate::media::ms_to_us(self.clock.now())
    }

    /// The pointer is at `point` (the active document's viewport) over
    /// `node`: the hover state of the default controls (M14 feature 9).
    /// True if a state changed.
    pub(super) fn controls_pointer(
        &mut self,
        node: Option<NodeId>,
        point: swb_layout::Point,
    ) -> bool {
        let now = self.controls_now();
        self.parts()
            .is_some_and(|(doc, _)| doc.controls_pointer(node, point, now))
    }

    /// The pointer left the active document.
    pub(super) fn controls_leave(&mut self) -> bool {
        let now = self.controls_now();
        self.parts().is_some_and(|(doc, _)| doc.controls_leave(now))
    }

    /// A press of the primary button at `point` of the active document: true
    /// if it is on a default control (the page's scripts get no event).
    pub(super) fn controls_down(&mut self, point: swb_layout::Point) -> bool {
        let now = self.controls_now();
        let node = self.hit_doc(point.x, point.y).map(|h| h.node);
        self.parts()
            .is_some_and(|(doc, _)| doc.controls_down(node, point, now))
    }

    /// A press on a default control moves the focus as a press on the
    /// element's picture does (measured in Chromium 148: `activeElement` is
    /// the media element after a press on any control, with `blur` and
    /// `focusout` on the old element, then `focus` and `focusin` on the media
    /// element, before the action; probes `in-focus-*` of
    /// `tools/probes/media-controls-input.json`), without events to the page
    /// for the press itself and without a selection change. `scripted`: the
    /// document's scripts see the focus events.
    pub(super) fn controls_press_focus(&mut self, point: swb_layout::Point, scripted: bool) {
        let focus = self
            .hit_doc(point.x, point.y)
            .and_then(|h| self.doc.as_ref()?.click_focus_target(h.node));
        if focus.is_some() && focus != self.focused_element() {
            self.set_keyboard_modality(false);
        }
        if scripted {
            if focus.is_none() {
                self.regain_window_focus();
            }
            self.scripted_focus(focus, false);
        } else {
            self.update_states(|s| {
                s.focus = focus;
                s.focus_visible = false;
            });
        }
    }

    /// True if a press on a default control is in progress.
    pub(super) fn controls_pressed(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.controls_pressed())
    }

    /// The pointer moved to `point` with a press in progress: a drag of the
    /// timeline or the volume slider follows. True if the page changed.
    pub(super) fn controls_drag(&mut self, point: swb_layout::Point) -> bool {
        let now = self.controls_now();
        self.parts()
            .is_some_and(|(doc, _)| doc.controls_drag(point, now))
    }

    /// The release of the primary button at `point`: true if the press was
    /// on a default control (the page's scripts get no event for the
    /// release).
    pub(super) fn controls_up(&mut self, point: swb_layout::Point) -> bool {
        let now = self.controls_now();
        self.parts()
            .is_some_and(|(doc, _)| doc.controls_up(point, now))
    }

    /// The `click` on `target` was not canceled: a press on the picture of a
    /// media element plays or pauses it.
    pub(super) fn controls_picture_click(&mut self, target: NodeId) -> bool {
        let now = self.controls_now();
        self.parts()
            .is_some_and(|(doc, _)| doc.controls_picture_click(target, now))
    }

    /// Tells the documents the page time of the next picture of their
    /// default controls.
    fn set_controls_time(&mut self) {
        let now = self.controls_now();
        for frame in self.media_frames() {
            if let Some(doc) = self.doc_of_mut(frame) {
                doc.set_controls_time(now);
            }
        }
    }

    /// How many walks of a fragment tree the window test of the videos ran,
    /// summed over the documents (tests: a render without a change runs
    /// none).
    #[doc(hidden)]
    pub fn media_window_walks(&self) -> u64 {
        self.media_frames()
            .into_iter()
            .filter_map(|frame| self.doc_of(frame))
            .map(DocumentState::media_window_walks)
            .sum()
    }

    /// What the video pipelines of the page counted so far (summed over the
    /// documents): for the GUI's frame log and the measurements.
    pub fn media_counters(&self) -> crate::media::MediaCounters {
        let mut sum = crate::media::MediaCounters::default();
        for frame in self.media_frames() {
            if let Some(doc) = self.doc_of(frame) {
                sum += doc.media_counters();
            }
        }
        sum
    }

    /// The synchronization of each video that plays and is painted now (the
    /// frame that is current and the media time on the element's clock), for
    /// the GUI's frame log: the sync trace of M14 feature 8 (ADR 0047
    /// section 7). Reads only; empty without a playing video.
    pub fn media_sync_samples(&self) -> Vec<crate::media::MediaSync> {
        if self.lent {
            return Vec::new();
        }
        let now = crate::media::ms_to_us(self.clock.now());
        let mut samples = Vec::new();
        for frame in self.media_frames() {
            if let Some(doc) = self.doc_of(frame) {
                samples.extend(doc.media_sync_samples(now));
            }
        }
        samples
    }

    /// Tells the frame documents that have videos, and the frames above
    /// them, whether their parent shows them (ADR 0047 notes of feature 7b):
    /// the frame has a box that is painted and, with the viewport rule, lies
    /// within the margin of the parent's viewport, and the parent is shown
    /// itself. The videos of a frame that is not shown convert no frames. It
    /// runs at each render of a page with frames, but looks only when its
    /// key changed (the frames, and for each the layout, display list,
    /// scroll and viewport of the parent, and the window state); a render
    /// without a change walks nothing. A look walks a parent's boxes only
    /// for the frames that lead to a video.
    pub(super) fn update_frames_shown(&mut self) {
        if self.frames.count() == 0 {
            return;
        }
        let order = self.frames.tree_order();
        if self.frames_shown.as_ref() == Some(&self.frames_shown_key(&order)) {
            return;
        }
        self.look_at_frames_shown(&order);
        // The key after the look: the look changed what the frames show.
        self.frames_shown = Some(self.frames_shown_key(&order));
    }

    /// What [`Page::update_frames_shown`] depends on, for the frames in
    /// `order`.
    fn frames_shown_key(&self, order: &[FrameId]) -> FramesShownKey {
        let frames = order
            .iter()
            .skip(1)
            .filter_map(|&id| {
                let frame = self.frames.get(id)?;
                let doc = frame.doc.as_deref();
                let parent_doc = frame.parent.and_then(|p| self.doc_of(p));
                Some(FrameKey {
                    id,
                    parent: frame.parent,
                    container: frame.container,
                    has_box: frame.has_box,
                    has_media: doc.is_some_and(DocumentState::has_media),
                    shown: doc.is_some_and(DocumentState::media_is_shown),
                    parent_shown: parent_doc.is_some_and(DocumentState::media_is_shown),
                    parent_geometry: parent_doc.map(DocumentState::media_geometry),
                })
            })
            .collect();
        FramesShownKey {
            hidden: self.maker.media.window_hidden(),
            frames,
        }
    }

    /// The look of [`Page::update_frames_shown`].
    fn look_at_frames_shown(&mut self, order: &[FrameId]) {
        let mut needed: std::collections::HashSet<FrameId> = std::collections::HashSet::new();
        for &id in order.iter().skip(1) {
            let has_media = self
                .frames
                .get(id)
                .and_then(|f| f.doc.as_deref())
                .is_some_and(DocumentState::has_media);
            let mut current = has_media.then_some(id);
            while let Some(frame) = current {
                if !needed.insert(frame) {
                    break;
                }
                current = self.frames.get(frame).and_then(|f| f.parent);
            }
        }
        // Parents come before their children.
        for &id in order.iter().skip(1).filter(|id| needed.contains(id)) {
            let Some(frame) = self.frames.get(id) else {
                continue;
            };
            let (parent, container, has_box) = (frame.parent, frame.container, frame.has_box);
            let shown = has_box
                && parent.zip(container).is_some_and(|(parent, container)| {
                    let parent_doc = self.doc_of(parent);
                    parent_doc.is_some_and(|d| {
                        (parent == FrameId::TOP || d.media_is_shown())
                            && d.element_in_media_window(container)
                    })
                });
            if let Some(doc) = self.frames.get_mut(id).and_then(|f| f.doc.as_deref_mut()) {
                doc.set_media_shown(shown);
            }
        }
    }

    /// Tells the page that the window is hidden (minimized, or fully
    /// covered where the platform says so) or shown again (ADR 0047 notes of
    /// feature 7b): while it is hidden no video converts frames. Decoding,
    /// the clocks and the events go on. A video that is shown again shows
    /// its frame (a paused one through the quiet seek of
    /// `Pipeline::set_painted`). Does nothing if the state is the same.
    pub fn set_window_hidden(&mut self, hidden: bool) {
        if self.maker.media.window_hidden() == hidden {
            return;
        }
        self.maker.media.set_window_hidden(hidden);
        // The layout that exists now is enough: the flags follow the next
        // layout and paint as they do for a scroll.
        self.update_frames_shown();
        for frame in self.media_frames() {
            if let Some(doc) = self.doc_of_mut(frame) {
                doc.update_media_visibility();
            }
        }
    }

    /// Brings the images of the videos to the media time of the page clock
    /// before a paint (ADR 0047 section 9): each pipeline takes the frame
    /// that is due, and a new frame bumps the generation of its image. In
    /// virtual time it does nothing while a media still has pending work:
    /// the pipeline's data for the page time is not all decoded, and a
    /// poll then would report an underrun that virtual time never has
    /// (`Page::screenshot` settles first).
    pub(super) fn sync_media_frames(&mut self) {
        if !self.lent {
            self.set_controls_time();
        }
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

    /// Gives the page's media elements an audio output (M14 feature 8, ADR
    /// 0047 section 8): the elements that play with audio join its mixer, and
    /// take their media time from it (the audio clock). The GUI calls it at
    /// the start; headless mode, the automation server and the tests never
    /// do, so their elements use the silent output. `None` takes the output
    /// away: elements that play leave the mixer at their next step and
    /// continue on the page clock from their current media time.
    #[cfg(feature = "media")]
    pub fn set_audio_output(&mut self, output: Option<std::sync::Arc<swb_media::output::Output>>) {
        self.maker.media.set_output(output);
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
