//! What the media element needs from the media crates, behind one interface
//! that has the same shape with and without the Cargo feature `media`
//! (ADR 0047 section 3): a [`Fetch`] starts a load of one resource and
//! tells what happens to it as [`Note`]s, one at a time.
//!
//! With the feature, a [`Fetch`] is a `swb_media::pipeline::Pipeline` over
//! the loader's media byte source (`Loader::open_media`, `SourceOpener::net`).
//! Without it, nothing is fetched: the load fails with code 4 as a failed
//! request does (`MEDIA_ELEMENT_ERROR: Format error`), one step after it
//! started, so that `loadstart` comes first.

use swb_net::{Destination, Loader, Request};

/// What the media of one page share: the places of open pipelines (the
/// process's 16, ADR 0047 section 6, unless the page has its own) and
/// whether media elements load. One per [`crate::Page`], handed to each of
/// its documents.
#[derive(Clone)]
pub(crate) struct MediaContext {
    /// The places of open pipelines: [`swb_media::pipeline::PipelineSlots::global`]
    /// unless [`MediaContext::isolate`] gave the page its own.
    #[cfg(feature = "media")]
    slots: std::sync::Arc<std::sync::Mutex<swb_media::pipeline::PipelineSlots>>,
    /// The audio output that the elements that play with audio join, if the
    /// page has one ([`crate::Page::set_audio_output`]; the GUI sets it,
    /// headless mode, the automation server and the tests never do: their
    /// pipelines use the silent output).
    #[cfg(feature = "media")]
    output: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<swb_media::output::Output>>>>,
    /// False: media elements do not start loads ([`crate::Page::set_media`]).
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// True: only a video near the viewport converts frames (the page paints
    /// frames, the GUI; [`crate::Page::set_paints_frames`]).
    viewport_rule: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// True: the window is hidden (minimized or occluded) and no video
    /// converts frames ([`crate::Page::set_window_hidden`]).
    window_hidden: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl MediaContext {
    /// A new context: the process's places of open pipelines; media
    /// elements load.
    pub(crate) fn new() -> MediaContext {
        MediaContext {
            #[cfg(feature = "media")]
            slots: std::sync::Arc::new(std::sync::Mutex::new(
                swb_media::pipeline::PipelineSlots::global(),
            )),
            #[cfg(feature = "media")]
            output: std::sync::Arc::new(std::sync::Mutex::new(None)),
            enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            viewport_rule: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            window_hidden: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Whether only a video near the viewport converts frames.
    pub(crate) fn viewport_rule(&self) -> bool {
        self.viewport_rule
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Turns the viewport rule on or off.
    pub(crate) fn set_viewport_rule(&self, on: bool) {
        self.viewport_rule
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the window is hidden.
    pub(crate) fn window_hidden(&self) -> bool {
        self.window_hidden
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Tells that the window is hidden or shown.
    pub(crate) fn set_window_hidden(&self, hidden: bool) {
        self.window_hidden
            .store(hidden, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether media elements start loads.
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Turns the loads of media elements on or off.
    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Gives the page places of its own (16), for the pipelines that start
    /// from now on ([`crate::Page::isolate_media`]); the last thread of a
    /// run that ended waits `handover_delay` before it hands its place on
    /// (a test hook).
    #[cfg_attr(not(feature = "media"), allow(clippy::unused_self))]
    pub(crate) fn isolate(&self, handover_delay: std::time::Duration) {
        #[cfg(feature = "media")]
        {
            let slots =
                swb_media::pipeline::PipelineSlots::new(swb_media::pipeline::MAX_OPEN_PIPELINES);
            slots.set_handover_delay(handover_delay);
            *self
                .slots
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = slots;
        }
        #[cfg(not(feature = "media"))]
        let _ = handover_delay;
    }

    /// Sets (or removes) the audio output of the page's media elements.
    #[cfg(feature = "media")]
    pub(crate) fn set_output(&self, output: Option<std::sync::Arc<swb_media::output::Output>>) {
        *self
            .output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = output;
    }

    /// The audio output, if the page has one.
    #[cfg(feature = "media")]
    pub(crate) fn output(&self) -> Option<std::sync::Arc<swb_media::output::Output>> {
        self.output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The places that a new pipeline uses.
    #[cfg(feature = "media")]
    fn slots(&self) -> swb_media::pipeline::PipelineSlots {
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Default for MediaContext {
    fn default() -> MediaContext {
        MediaContext::new()
    }
}

/// How much to read ahead of the playback position (`preload`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preload {
    /// `none` (after a load that was asked for) and `metadata`: 3 s.
    Metadata,
    /// `auto`: 15 s.
    Auto,
}

/// What a fetch tells the element, one note at a time. Without the feature
/// `media` only a failure comes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(feature = "media"), allow(dead_code))]
pub(crate) enum Note {
    /// The container opened: `loadedmetadata`.
    Metadata(Meta),
    /// The first data at the position is decoded: `loadeddata`.
    FirstData,
    /// The load failed before the metadata, or the pipeline failed later
    /// (the element decides which it is).
    Failed(Failure),
    /// A seek ended at this media time (seconds).
    SeekDone(f64),
    /// A `timeupdate` point of the playback.
    TimeUpdate,
    /// Playback ran out of decoded data.
    Underrun,
    /// Playback went on after an underrun.
    Resumed,
    /// Playback reached the end of the media.
    Ended,
}

/// What the demuxer found, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Meta {
    /// `duration`: infinity when the file does not tell.
    pub(crate) duration: f64,
    /// The media time where playback starts (`currentTime` after the load).
    pub(crate) start: f64,
    /// `videoWidth` and `videoHeight`, when the file has a video track.
    pub(crate) size: Option<(u32, u32)>,
    /// Whether `seekable` is the whole timeline.
    pub(crate) seekable: bool,
}

/// A failed load or a failed pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    /// `MediaError.code`.
    pub(crate) code: u16,
    /// `MediaError.message`.
    pub(crate) message: String,
    /// True when bytes arrived (the request itself succeeded): `progress`
    /// and `suspend` come before the `error`.
    pub(crate) bytes_arrived: bool,
    /// For a failure of the pipeline after the metadata: the media time
    /// (microseconds) where the playback fails (`Pipeline::failed_at`), when
    /// the failing thread knows: the playback goes on to it, then fails.
    pub(crate) at_us: Option<i64>,
}

/// What the video pipelines of a page counted (M14 feature 7b; the GUI's
/// frame log and the measurements). All zero without the feature `media`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaCounters {
    /// Frames that were shown (made current).
    pub shown: u64,
    /// Frames that were dropped, because their successor was due as well
    /// or because they were late when decoded
    /// (`getVideoPlaybackQuality().droppedVideoFrames`).
    pub dropped: u64,
    /// Frames decoded.
    pub decoded: u64,
    /// Frames converted to pixels.
    pub converted: u64,
}

/// One look at the synchronization of a playing, painted video (M14 feature
/// 8, ADR 0047 section 7): the frame that is current and the element's media
/// time, as the GUI's frame log records them at a present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaSync {
    /// The presentation time of the frame that is current (microseconds).
    pub pts_us: i64,
    /// The media time of the element at the look, on its clock: the audio
    /// clock while the element is in the audio mixer (microseconds).
    pub media_us: i64,
    /// Whether the element's clock is the audio clock.
    pub audio_clock: bool,
}

impl std::ops::AddAssign for MediaCounters {
    fn add_assign(&mut self, other: MediaCounters) {
        self.shown += other.shown;
        self.dropped += other.dropped;
        self.decoded += other.decoded;
        self.converted += other.converted;
    }
}

/// The message of a failed request or a bad URL (Chromium's).
pub(crate) const FORMAT_ERROR: &str = "MEDIA_ELEMENT_ERROR: Format error";

#[cfg(feature = "media")]
pub(crate) use real::Fetch;
#[cfg(not(feature = "media"))]
pub(crate) use stub::Fetch;

/// The request for the media resource at `url`: the destination, the
/// credentials and the CORS mode of the element's `crossorigin` attribute
/// (HTML "potentially CORS-enabled fetch": without the attribute the
/// request is `no-cors` with credentials `include`; `anonymous` is `cors`
/// with `same-origin` credentials; `use-credentials` `cors` with `include`).
pub(crate) fn request_for(
    url: swb_net::Url,
    video: bool,
    crossorigin: Option<&str>,
    initiator: Option<swb_net::Origin>,
    site_for_cookies: Option<swb_net::Origin>,
) -> Request {
    use swb_net::{CredentialsMode, RequestMode};
    let destination = if video {
        Destination::Video
    } else {
        Destination::Audio
    };
    let mut request = Request::get(url, destination)
        .with_initiator(initiator)
        .with_site_for_cookies(site_for_cookies);
    if let Some(value) = crossorigin {
        request.mode = RequestMode::Cors;
        request.credentials = if value.eq_ignore_ascii_case("use-credentials") {
            CredentialsMode::Include
        } else {
            CredentialsMode::SameOrigin
        };
    }
    request
}

#[cfg(not(feature = "media"))]
#[allow(clippy::unused_self)] // The same interface as the real fetch.
mod stub {
    //! Without the feature: nothing is fetched.

    use super::{
        FORMAT_ERROR, Failure, Loader, MediaContext, MediaCounters, Note, Preload, Request,
    };

    /// A load that fails at once.
    pub(crate) struct Fetch {
        failed: Option<Failure>,
    }

    impl Fetch {
        /// Starts the load of `request`: it fails with code 4, without a
        /// request.
        pub(crate) fn start(
            _ctx: &MediaContext,
            _loader: &Loader,
            _request: Request,
            _video: bool,
            _preload: Preload,
        ) -> Fetch {
            Fetch {
                failed: Some(Failure {
                    code: 4,
                    message: FORMAT_ERROR.to_owned(),
                    bytes_arrived: false,
                    at_us: None,
                }),
            }
        }

        /// Lets the fetch look at its pipeline (`now_us`, the page clock).
        pub(crate) fn poll(&mut self, _now_us: i64) {}

        /// The next note.
        pub(crate) fn next_note(&mut self) -> Option<Note> {
            self.failed.take().map(Note::Failed)
        }

        /// Whether a note waits.
        pub(crate) fn has_notes(&self) -> bool {
            self.failed.is_some()
        }

        /// Whether the next note is the first data.
        pub(crate) fn front_is_first_data(&self) -> bool {
            false
        }

        /// Whether the next note is the metadata and the byte source has
        /// not read enough for it yet.
        pub(crate) fn net_unsettled(&self) -> bool {
            false
        }

        /// Whether the byte source stopped (the cache holds what the
        /// read-ahead asks for, or all of the resource).
        pub(crate) fn net_idle(&self) -> bool {
            true
        }

        /// Whether the pipeline still opens (it has pending work).
        pub(crate) fn pending(&self) -> bool {
            false
        }

        /// Whether the data is enough to play through (it needs data
        /// otherwise).
        pub(crate) fn enough(&self) -> bool {
            true
        }

        /// `buffered` for a duration of `duration`, in seconds.
        pub(crate) fn buffered(&self, _duration: f64) -> Vec<(f64, f64)> {
            Vec::new()
        }

        /// Whether the pipeline runs and can suspend.
        pub(crate) fn can_suspend(&self) -> bool {
            false
        }

        /// Closes the pipeline's threads and keeps its state.
        pub(crate) fn suspend(&mut self, _now_us: i64) {}

        /// The pipeline is suspended.
        pub(crate) fn is_suspended(&self) -> bool {
            false
        }

        /// Tells the byte source what to read ahead.
        pub(crate) fn set_preload(&mut self, _preload: Preload) {}

        /// Starts the media clock (no pipeline, no clock).
        pub(crate) fn play(&mut self, _now_us: i64) {}

        /// Stops the media clock.
        pub(crate) fn pause(&mut self, _now_us: i64) {}

        /// Seeks; false when there is no pipeline to do it.
        pub(crate) fn seek(&mut self, _target_us: i64) -> bool {
            false
        }

        /// The playback position at clock time `now_us`, in seconds.
        pub(crate) fn position_at(&self, _now_us: i64) -> f64 {
            0.0
        }

        /// The gain of the audio (`volume`, 0 when muted).
        pub(crate) fn set_gain(&mut self, _gain: f32) {}

        /// The page time (ms) of the next frame, `timeupdate` point or end.
        pub(crate) fn next_due(&self, _coarse: bool, _now_us: i64) -> Option<f64> {
            None
        }

        /// The failure that came after the metadata.
        pub(crate) fn late(&self) -> Option<&Failure> {
            None
        }

        /// The media time (microseconds) of the next `timeupdate` point.
        pub(crate) fn next_timeupdate_us(&self) -> Option<i64> {
            None
        }

        /// Whether the note after the next one is the first data.
        pub(crate) fn second_is_first_data(&self) -> bool {
            false
        }

        /// Whether the next note is the end of a seek.
        pub(crate) fn front_is_seek_done(&self) -> bool {
            false
        }

        /// Adds a note (a seek of a pipeline that cannot seek ends at once).
        pub(crate) fn push_note(&mut self, _note: Note) {}

        /// The pixels of the frame shown now.
        pub(crate) fn frame_image(&self) -> Option<std::sync::Arc<swb_paint::Pixmap>> {
            None
        }

        /// Whether the video is painted (frames are converted).
        pub(crate) fn set_painted(&mut self, _painted: bool) {}

        /// The frames converted so far (tests).
        pub(crate) fn frames_converted(&self) -> u64 {
            0
        }

        /// No pipeline, no frame.
        pub(crate) fn frame_due(&self, _now_us: i64) -> bool {
            false
        }

        /// No pipeline, no counters.
        pub(crate) fn counters(&self) -> MediaCounters {
            MediaCounters::default()
        }

        /// No pipeline, nothing to look at.
        pub(crate) fn sync_sample(&self, _now_us: i64) -> Option<super::MediaSync> {
            None
        }
    }
}

#[cfg(feature = "media")]
mod real {
    //! With the feature: a pipeline over the loader's media byte source.

    use std::collections::VecDeque;

    use swb_media::output::{Phase, Source, lead_frames};
    use swb_media::pipeline::{AudioOutput, Pipeline, PipelineOptions, Report, SourceOpener};
    use swb_media::{ClockSource, MediaError};
    use swb_net::{MediaHandle, MediaOptions, ReadAhead};

    use super::{
        FORMAT_ERROR, Failure, Loader, MediaContext, MediaCounters, Meta, Note, Preload, Request,
    };

    /// The metadata waits until the byte source has read this much (or all
    /// of a smaller resource): a small file is read whole before the first
    /// event, so the events do not depend on how fast the threads are.
    const SETTLE_BYTES: usize = 1 << 20;

    /// A pipeline and the byte source under it.
    pub(crate) struct Fetch {
        pipeline: Pipeline,
        handle: MediaHandle,
        video: bool,
        /// Reports that `poll` took and the element has not read.
        notes: VecDeque<Note>,
        /// The metadata was reported.
        metadata_seen: bool,
        /// The first data was reported (or made up after a late failure).
        first_data: bool,
        /// A seek came while the note of the first data waited: the note
        /// follows the end of the seek (see [`Fetch::seek`]).
        first_data_held: bool,
        /// A failure that came after the metadata: the element keeps it for
        /// the playback (feature 6b) and does not report it at load, as
        /// Chromium loads a file whose damage lies later and fails when it
        /// plays there. The load goes on as if the first data had come, so
        /// that the events do not depend on when the threads met the damage.
        late: Option<Failure>,
        suspended: bool,
        /// The page's media context: its audio output, looked up when the
        /// element plays.
        ctx: MediaContext,
        /// The page's wake-up (the audio output calls it when the device
        /// streams or fails).
        waker: std::sync::Arc<dyn Fn() + Send + Sync>,
        /// The element's place in the audio mixer, while it plays with
        /// audio and the output streams. Then the clock time that the
        /// pipeline gets is the source's audio clock; else it is the page
        /// time (ADR 0047 section 7, "Implementation notes (M14 feature
        /// 8)").
        audio: Option<Source>,
        /// `play()` came and the output was not asked since: the next
        /// request may try a failed output again.
        retry_output: bool,
    }

    fn read_ahead(preload: Preload) -> ReadAhead {
        match preload {
            Preload::Metadata => ReadAhead::METADATA,
            Preload::Auto => ReadAhead::AUTO,
        }
    }

    impl Fetch {
        /// Opens the pipeline over the resource of `request`.
        pub(crate) fn start(
            ctx: &MediaContext,
            loader: &Loader,
            request: Request,
            video: bool,
            preload: Preload,
        ) -> Fetch {
            let options = MediaOptions {
                read_ahead: read_ahead(preload),
                ..MediaOptions::default()
            };
            let resource = loader.open_media_with(request, options);
            let handle = resource.handle();
            // The element and its page wake up when the byte source makes
            // progress and when the pipeline has something to report.
            handle.set_notify(loader.waker());
            drop(resource);
            let mut options = PipelineOptions::new(loader.waker());
            options.video = video;
            options.slots = ctx.slots();
            // Painted from the open; the first layout without a box for the
            // element says otherwise (`DocumentState::update_media_visibility`).
            options.painted = true;
            let pipeline = Pipeline::open(SourceOpener::net(handle.clone()), options);
            Fetch {
                pipeline,
                handle,
                video,
                notes: VecDeque::new(),
                metadata_seen: false,
                first_data: false,
                first_data_held: false,
                late: None,
                suspended: false,
                ctx: ctx.clone(),
                waker: loader.waker(),
                audio: None,
                retry_output: false,
            }
        }

        // ----- The audio clock (ADR 0047 section 7) -----

        /// The clock time that the pipeline gets at page time `now_us`: the
        /// audio clock of the element's source in the mixer, else the page
        /// time.
        fn clock_now(&self, now_us: i64) -> i64 {
            self.audio.as_ref().map_or(now_us, Source::clock_us)
        }

        /// Whether the element should be in the mixer: it plays, its clock
        /// runs, its audio has samples to count.
        fn wants_mixing(&self) -> bool {
            !self.suspended
                && self.pipeline.is_playing()
                && self.pipeline.is_clock_running()
                && self.pipeline.samples().is_some()
                && !self.pipeline.audio_finished()
        }

        /// Moves the element into or out of the audio mixer, as the state
        /// of its playback and of the output says (at each poll, `play` and
        /// `pause`). It joins when its clock runs and the device streams;
        /// it leaves when its clock stops (a pause, a seek, an underrun, the
        /// end), when its audio ends before the video, when the device
        /// failed (its source is gone), and when the page has no output any
        /// more. Both moves keep the media time (no jump): the clock is
        /// rebased from one source to the other at the same moment.
        fn sync_audio(&mut self, now_us: i64) {
            let Some(output) = self.ctx.output() else {
                self.detach_audio(now_us);
                return;
            };
            match &self.audio {
                Some(source) => {
                    if !source.is_live() || !self.wants_mixing() {
                        self.detach_audio(now_us);
                    }
                }
                None if self.wants_mixing() => {
                    // Nothing can change while the output has failed (until
                    // a `play()` may try it again) or the table is full (the
                    // 33rd element): do not call `request` (a lock, a
                    // tick, a wake-up of the timer thread) and do not move
                    // the pipeline between the outputs at every poll.
                    match output.phase() {
                        Phase::Failed if !self.retry_output => return,
                        Phase::Running if output.is_full() => return,
                        _ => {}
                    }
                    // A failed output is tried again by a `play()`, not by
                    // the steps of an element that already plays.
                    let retry = std::mem::take(&mut self.retry_output);
                    output.request(Some(&self.waker), retry);
                    if !output.is_ready() {
                        return;
                    }
                    let Some(ring) = self.pipeline.samples() else {
                        return;
                    };
                    // The ring is where the media clock is, or ahead of it
                    // (silence first) or behind it (frames skipped).
                    let lead = lead_frames(&ring, self.pipeline.time_at(now_us));
                    self.pipeline.set_output(AudioOutput::External);
                    match output.attach(ring, lead) {
                        Some(source) => {
                            let audio_now = source.clock_us();
                            self.pipeline
                                .rebase_clock(ClockSource::Audio, now_us, audio_now);
                            self.audio = Some(source);
                        }
                        // The 33rd element: it stays on the silent output.
                        None => self.pipeline.set_output(AudioOutput::Silent),
                    }
                }
                None => {}
            }
        }

        /// Takes the element out of the mixer (its source fades out) and
        /// goes on at page time `now_us` from the media time that the audio
        /// clock had.
        fn detach_audio(&mut self, now_us: i64) {
            let Some(source) = self.audio.take() else {
                return;
            };
            let audio_now = source.clock_us();
            drop(source);
            self.pipeline.set_output(AudioOutput::Silent);
            self.pipeline
                .rebase_clock(ClockSource::Page, audio_now, now_us);
        }

        /// Takes the reports of the pipeline at page time `now_us`.
        pub(crate) fn poll(&mut self, now_us: i64) {
            if self.suspended {
                return;
            }
            self.poll_pipeline(now_us);
            self.sync_audio(now_us);
        }

        fn poll_pipeline(&mut self, now_us: i64) {
            let clock_now = self.clock_now(now_us);
            for report in self.pipeline.update(clock_now) {
                match report {
                    Report::Metadata(m) => {
                        self.metadata_seen = true;
                        let timeline = m.timeline;
                        self.handle
                            .set_duration(timeline.duration_us.map(|d| d as f64 / 1e6));
                        self.notes.push_back(Note::Metadata(Meta {
                            duration: timeline
                                .duration_us
                                .map_or(f64::INFINITY, |d| d as f64 / 1e6),
                            start: timeline.start_us as f64 / 1e6,
                            size: m.natural_size().filter(|&(w, h)| w > 0 && h > 0),
                            seekable: m.info.seekable,
                        }));
                    }
                    Report::FirstFrame => {
                        self.first_data = true;
                        self.notes.push_back(Note::FirstData);
                    }
                    Report::Error(error) => {
                        let mut failure = self.failure(&error);
                        if self.metadata_seen {
                            failure.at_us = self.pipeline.failed_at();
                            self.late = Some(failure);
                            // A note of the first data that a seek holds
                            // (`Fetch::seek`) comes now: the failed
                            // pipeline never reports the end of that seek.
                            if !self.first_data || std::mem::take(&mut self.first_data_held) {
                                self.first_data = true;
                                self.notes.push_back(Note::FirstData);
                            }
                        } else {
                            self.notes.push_back(Note::Failed(failure));
                        }
                    }
                    Report::SeekDone { media_us } => {
                        self.notes.push_back(Note::SeekDone(media_us as f64 / 1e6));
                        if std::mem::take(&mut self.first_data_held) {
                            self.notes.push_back(Note::FirstData);
                        }
                    }
                    Report::TimeUpdate { .. } => self.notes.push_back(Note::TimeUpdate),
                    Report::Underrun { .. } => self.notes.push_back(Note::Underrun),
                    Report::Resumed { .. } => self.notes.push_back(Note::Resumed),
                    Report::Ended { .. } => self.notes.push_back(Note::Ended),
                    // Buffered ranges are read live.
                    Report::Buffered(_) => {}
                }
            }
        }

        /// The failure that `error` is, in the element's terms (ADR 0047
        /// section 10: Chromium's messages).
        fn failure(&self, error: &MediaError) -> Failure {
            let bytes_arrived = self.handle.error().is_none();
            let (code, message) = if bytes_arrived {
                match error {
                    MediaError::NotMedia(_) => (
                        4,
                        "PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open context failed"
                            .to_owned(),
                    ),
                    MediaError::Unsupported(_) | MediaError::NotBuilt => (
                        4,
                        "PipelineStatus::DEMUXER_ERROR_NO_SUPPORTED_STREAMS: FFmpegDemuxer: no supported streams"
                            .to_owned(),
                    ),
                    MediaError::Decode(_) | MediaError::Seek(_) => (
                        3,
                        format!(
                            "PipelineStatus::PIPELINE_ERROR_DECODE: {} decode error!",
                            if self.video { "video" } else { "audio" }
                        ),
                    ),
                    MediaError::Source(_) => (
                        2,
                        "PipelineStatus::PIPELINE_ERROR_READ: FFmpegDemuxer: data source error"
                            .to_owned(),
                    ),
                }
            } else {
                // The request failed (a status, CORS, a refused
                // connection): Chromium's message for every such load.
                (4, FORMAT_ERROR.to_owned())
            };
            Failure {
                code,
                message,
                bytes_arrived,
                at_us: None,
            }
        }

        /// The next note, in the order of the reports.
        pub(crate) fn next_note(&mut self) -> Option<Note> {
            self.notes.pop_front()
        }

        /// Whether a note waits.
        pub(crate) fn has_notes(&self) -> bool {
            !self.notes.is_empty()
        }

        /// Whether the next note is the first data.
        pub(crate) fn front_is_first_data(&self) -> bool {
            matches!(self.notes.front(), Some(Note::FirstData))
        }

        /// Whether the next note is the metadata and the byte source has not
        /// read enough for it ([`SETTLE_BYTES`], or all of the resource).
        pub(crate) fn net_unsettled(&self) -> bool {
            if !matches!(self.notes.front(), Some(Note::Metadata(_))) {
                return false;
            }
            let stats = self.handle.stats();
            !(stats.idle || stats.cached_bytes >= SETTLE_BYTES)
        }

        /// Whether the byte source stopped fetching.
        pub(crate) fn net_idle(&self) -> bool {
            self.handle.stats().idle
        }

        /// Whether the pipeline has not decoded what the owner needs next
        /// (it opens), or waits for a place that is being handed on (the
        /// threads of a run that ended are ending): virtual time waits.
        pub(crate) fn pending(&self) -> bool {
            // `awaits_handover` is read first. While the pipeline waits for
            // a place, `has_pending_work` is false. If the handover ended
            // between the two reads (`has_pending_work` first), both would
            // be false although the pipeline now has the place and work to
            // do. Read in this order, a handover that ends after the first
            // read is seen by the second.
            !self.suspended
                && (self.pipeline.awaits_handover()
                    || self.pipeline.has_pending_work()
                    || self.pipeline.has_unread_reports())
        }

        /// Whether the decoded data is enough to play through; a failure
        /// after the first data counts as enough (the error comes with the
        /// playback).
        pub(crate) fn enough(&self) -> bool {
            self.suspended || self.late.is_some() || self.pipeline.has_enough_data()
        }

        /// `buffered`, in seconds, for a duration of `duration`: the whole
        /// timeline once the byte source holds all of the resource (Chromium,
        /// measured: a truncated or damaged file that arrived whole is
        /// buffered to its duration), else the ranges that the demuxer read,
        /// clamped to the duration.
        pub(crate) fn buffered(&self, duration: f64) -> Vec<(f64, f64)> {
            let stats = self.handle.stats();
            if stats.idle
                && stats
                    .size
                    .is_some_and(|size| stats.cached_bytes as u64 >= size)
                && duration.is_finite()
                && duration > 0.0
            {
                return vec![(0.0, duration)];
            }
            self.pipeline
                .buffered()
                .into_iter()
                .map(|(a, b)| (a as f64 / 1e6, (b as f64 / 1e6).min(duration)))
                .filter(|(a, b)| a < b)
                .collect()
        }

        /// Whether the pipeline runs: it can suspend.
        pub(crate) fn can_suspend(&self) -> bool {
            !self.suspended && !self.pipeline.is_failed() && !self.pipeline.is_waiting()
        }

        /// Closes the threads of the pipeline and gives its place to the
        /// next one; the metadata, the position and the byte cache stay.
        pub(crate) fn suspend(&mut self, now_us: i64) {
            self.pipeline.suspend(self.clock_now(now_us));
            self.detach_audio(now_us);
            self.suspended = true;
        }

        /// The pipeline is suspended.
        pub(crate) fn is_suspended(&self) -> bool {
            self.suspended
        }

        /// Tells the byte source what to read ahead.
        pub(crate) fn set_preload(&mut self, preload: Preload) {
            self.handle.set_read_ahead(read_ahead(preload));
        }

        /// Starts the media clock at clock time `now_us`; a suspended
        /// pipeline opens again (ADR 0047 section 6).
        pub(crate) fn play(&mut self, now_us: i64) {
            self.suspended = false;
            self.retry_output = true;
            self.pipeline.play(self.clock_now(now_us));
            self.sync_audio(now_us);
        }

        /// Stops the media clock at page time `now_us`; the element leaves
        /// the audio mixer.
        pub(crate) fn pause(&mut self, now_us: i64) {
            if !self.suspended {
                self.pipeline.pause(self.clock_now(now_us));
                self.detach_audio(now_us);
            }
        }

        /// Seeks to `target_us` (a suspended pipeline opens again). False
        /// when the pipeline failed: it cannot seek. A seek that ended and
        /// that the element has not read is dropped: the new one replaces it.
        /// A note of the first data that the element has not read yet (the
        /// first frame was decoded before the seek that the metadata brings:
        /// a position that a script set, a `#t=` fragment) moves behind the
        /// end of the seek, where it is when the first frame comes after the
        /// seek; the order of the notes does not depend on which thread was
        /// faster (Chromium: `seeked` before `loadeddata`).
        pub(crate) fn seek(&mut self, target_us: i64) -> bool {
            if self.pipeline.is_failed() {
                return false;
            }
            self.suspended = false;
            self.notes.retain(|n| !matches!(n, Note::SeekDone(_)));
            if let Some(at) = self.notes.iter().position(|n| matches!(n, Note::FirstData)) {
                self.notes.remove(at);
                self.first_data_held = true;
            }
            self.pipeline.seek(target_us);
            // The clock stops at the seek, and the ring is discarded: the
            // element leaves the mixer and joins again when the clock runs.
            // (A clock that is stopped has no time to rebase.)
            self.detach_audio(0);
            true
        }

        /// The playback position at page time `now_us`, in seconds.
        pub(crate) fn position_at(&self, now_us: i64) -> f64 {
            self.pipeline.time_at(self.clock_now(now_us)) as f64 / 1e6
        }

        /// The gain of the audio (`volume`, 0 when muted).
        pub(crate) fn set_gain(&mut self, gain: f32) {
            self.pipeline.set_gain(gain);
        }

        /// The page time (ms) of the next frame, `timeupdate` point or end
        /// while the clock runs. With `coarse` (a real-time page), a video
        /// that is not painted has no frame times
        /// ([`swb_media::pipeline::Pipeline::next_wake`]).
        pub(crate) fn next_due(&self, coarse: bool, now_us: i64) -> Option<f64> {
            if self.suspended {
                return None;
            }
            let due = if coarse {
                self.pipeline.next_wake()
            } else {
                self.pipeline.next_due()
            }?;
            // The pipeline's due time is on its clock; the page waits on
            // its own: the audio clock runs at the rate of real time, so the
            // time that is left is the same (ADR 0047 notes of feature 8).
            let due = match &self.audio {
                Some(source) => now_us.saturating_add(due.saturating_sub(source.clock_us())),
                None => due,
            };
            Some(due as f64 / 1000.0)
        }

        /// The failure that came after the metadata.
        pub(crate) fn late(&self) -> Option<&Failure> {
            self.late.as_ref()
        }

        /// The media time (microseconds) of the next `timeupdate` point.
        pub(crate) fn next_timeupdate_us(&self) -> Option<i64> {
            self.pipeline.next_timeupdate()
        }

        /// Whether the note after the next one is the first data.
        pub(crate) fn second_is_first_data(&self) -> bool {
            matches!(self.notes.get(1), Some(Note::FirstData))
        }

        /// Whether the next note is the end of a seek.
        pub(crate) fn front_is_seek_done(&self) -> bool {
            matches!(self.notes.front(), Some(Note::SeekDone(_)))
        }

        /// Adds a note (a seek of a pipeline that cannot seek ends at once).
        pub(crate) fn push_note(&mut self, note: Note) {
            self.notes.push_back(note);
        }

        /// The pixels of the frame shown now, as the last `poll` left it
        /// (`None` before the first frame). A frame that was decoded while
        /// the video was not painted has no pixels: the last frame that was
        /// converted shows until a converted one comes (a video that scrolls
        /// back into view shows its old frame for a moment, not a hole; ADR
        /// 0047 notes of feature 7b).
        pub(crate) fn frame_image(&self) -> Option<std::sync::Arc<swb_paint::Pixmap>> {
            let frame = self.pipeline.current_frame()?;
            frame
                .image
                .clone()
                .or_else(|| self.pipeline.current_image().cloned())
        }

        /// Whether the video is painted: frames that are not are decoded
        /// but not converted (ADR 0047 section 6, "Hidden elements").
        pub(crate) fn set_painted(&mut self, painted: bool) {
            self.pipeline.set_painted(painted);
        }

        /// The frames converted so far (tests).
        pub(crate) fn frames_converted(&self) -> u64 {
            self.pipeline.stats().frames_converted
        }

        /// Whether a poll at clock time `now_us` would show a frame with
        /// pixels that is not the one shown (the frame decision of the GUI;
        /// see [`Pipeline::frame_due`]).
        pub(crate) fn frame_due(&self, now_us: i64) -> bool {
            !self.suspended && self.pipeline.frame_due(self.clock_now(now_us))
        }

        /// The frame that is current and the media time at page time
        /// `now_us`, while the video plays (the frame log's sync trace).
        pub(crate) fn sync_sample(&self, now_us: i64) -> Option<super::MediaSync> {
            if self.suspended || !self.pipeline.is_playing() || !self.pipeline.is_clock_running() {
                return None;
            }
            let frame = self.pipeline.current_frame()?;
            Some(super::MediaSync {
                pts_us: frame.pts_us,
                media_us: self.pipeline.time_at(self.clock_now(now_us)),
                audio_clock: self.audio.is_some(),
            })
        }

        /// The counters of the pipeline.
        pub(crate) fn counters(&self) -> MediaCounters {
            let quality = self.pipeline.quality();
            let stats = self.pipeline.stats();
            MediaCounters {
                shown: quality.total_frames.saturating_sub(quality.dropped_frames),
                dropped: quality.dropped_frames,
                decoded: stats.frames_decoded,
                converted: stats.frames_converted,
            }
        }
    }
}

#[cfg(all(test, feature = "media"))]
mod tests {
    use super::MediaContext;
    use swb_media::pipeline::PipelineSlots;

    /// ADR 0047 section 6: the pipelines of all pages share the process's
    /// 16 places; a page that is isolated (tests) has its own.
    #[test]
    fn pages_share_the_process_places_unless_isolated() {
        let a = MediaContext::new();
        let b = MediaContext::new();
        assert!(a.slots() == PipelineSlots::global());
        assert!(b.slots() == a.slots());
        let shared = a.clone();
        a.isolate(std::time::Duration::ZERO);
        assert!(a.slots() != PipelineSlots::global());
        // The documents of the page (clones of its context) follow.
        assert!(shared.slots() == a.slots());
        assert!(b.slots() == PipelineSlots::global());
    }
}
