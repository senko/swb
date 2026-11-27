//! The media pipeline (ADR 0047 sections 6 and 7): per open media resource
//! a demux thread and a video thread, bounded queues, the media clock,
//! audio and video synchronization, seeking, the silent output, virtual
//! time, the bound on open pipelines, the budget of decoded frames
//! (`slots.rs`) and the pool of pixmaps (`pool.rs`).
//!
//! `due.rs` (due times and pending work) and `update.rs` (the update step)
//! hold parts of `impl Pipeline`.
//!
//! The element (in the engine) owns a [`Pipeline`] and drives it from the
//! page thread: it passes the time of its clock to [`Pipeline::update`],
//! which advances the media clock, makes the frame at the media time
//! current, lets the silent output consume the samples up to it, and
//! returns the [`Report`]s (metadata, the first frame, buffered ranges,
//! underrun and resume, seek done, `timeupdate` points, the end, errors).
//! The pipeline calls the owner's notify (from its threads) when a report
//! is ready or data that the owner waits for was decoded.
//!
//! **Virtual time** (ADR 0028 section 7): [`Pipeline::next_due`] is the
//! clock time of the next frame, the next `timeupdate` point (every 250 ms
//! of media time) or the end; [`Pipeline::has_pending_work`] is true while
//! the pipeline has not decoded what that time needs (the frame and the
//! samples up to it, or the end), while it opens or seeks. An owner that
//! moves virtual time only when no work is pending gets the same frames and
//! reports on every run, whatever the speed of the machine.
//!
//! **Tracks far apart in the file.** When both threads wait for room that
//! only the owner frees (the demuxer reached its queue bounds and the data
//! that the owner needs is further on in the file), the pipeline is stalled
//! on its owner: no work is pending, and the clock runs on with what is
//! there: silence where the ring has no samples for the media time (an
//! audio gap), the last frame where no frame is decoded (late frames are
//! dropped when they come).
//!
//! **No progress** (ADR 0047 section 11): a pipeline that needs data and
//! made no progress for 10 s fails with a decode error; its threads are
//! detached (`watchdog.rs`).
//!
//! All times are microseconds: media times on the element's timeline
//! ([`Timeline`]), clock times on the owner's clock.

mod demux;
mod due;
mod opener;
mod pool;
mod shared;
mod slots;
mod update;
mod video;
mod watchdog;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tiny_skia::Pixmap;

pub use crate::Notify;
pub use opener::SourceOpener;
pub use slots::{FRAME_BUDGET_BYTES, MAX_DETACHED_THREADS, MAX_OPEN_PIPELINES, PipelineSlots};

use self::shared::{MAX_PACKET_BYTES, STALL_LIMIT, SeekCmd, Settings, Shared, State, ThreadEvent};
use crate::clock::{ClockSource, MediaClock};
use crate::origin::Timeline;
use crate::ring::SampleRing;
use crate::{MediaError, MediaInfo};

/// `timeupdate` comes every 250 ms of media time while playing (Chromium).
pub const TIMEUPDATE_INTERVAL_US: i64 = 250_000;
/// After an underrun, playback resumes when this much audio is decoded
/// past the media time (or the audio ended, or the ring is full).
const RESUME_AHEAD_US: i64 = 250_000;

/// Who consumes the samples of the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioOutput {
    /// The pipeline's silent output consumes them in [`Pipeline::update`],
    /// up to the media time (headless mode, tests, no audio device).
    #[default]
    Silent,
    /// Another consumer reads the ring ([`Pipeline::samples`]: the audio
    /// mixer, `crate::output`). The ring has one consumer only, so the
    /// pipeline consumes nothing. The consumer does not wake the demux
    /// thread (it must not take the pipeline's lock, and it runs on a
    /// real-time thread): the demux thread that waits for room in the ring
    /// wakes by itself, on a timer ([`State::external_wait`]: between 20 and
    /// 500 ms), so the audio does not depend on the owner's thread. The owner
    /// switches with [`Pipeline::set_output`].
    External,
}

/// The options of a pipeline.
#[derive(Clone)]
pub struct PipelineOptions {
    /// Decode the video track (a `<video>` element).
    pub video: bool,
    /// Decode the audio track. A track that is not wanted is decoded
    /// anyway when it is the only one.
    pub audio: bool,
    /// The bound on open pipelines (default: the process's).
    pub slots: PipelineSlots,
    /// The owner's wake-up.
    pub notify: Notify,
    /// The silent output keeps the samples that it consumes
    /// ([`Pipeline::take_captured_audio`]); for tests and measurements.
    pub capture_audio: bool,
    /// The source of the clock time that the owner passes.
    pub clock: ClockSource,
    /// Who consumes the samples (default: the silent output).
    pub output: AudioOutput,
    /// The most bytes of compressed video packets in the queue (ADR 0047
    /// section 6: 16 MiB); the audio packets that the demuxer reads while
    /// the ring is full get a quarter of it. Tests set a small bound so that
    /// a small file reaches it.
    pub packet_bytes: usize,
    /// The no-progress limit (ADR 0047 section 11: 10 s): a pipeline that
    /// needs data and has made no progress for this long fails with a
    /// decode error, and its threads are detached. Tests set a short one.
    pub stall_limit: Duration,
    /// Whether the video is painted from the open (default: true); see
    /// [`Pipeline::set_painted`]. An element that is hidden at the open
    /// sets false, so that no frame is converted before its first
    /// `set_painted`.
    pub painted: bool,
}

impl PipelineOptions {
    /// Both tracks, the process's bound, `notify` as the wake-up, the
    /// silent output, the bounds of ADR 0047.
    pub fn new(notify: Notify) -> PipelineOptions {
        PipelineOptions {
            video: true,
            audio: true,
            slots: PipelineSlots::global(),
            notify,
            capture_audio: false,
            clock: ClockSource::Page,
            output: AudioOutput::Silent,
            packet_bytes: MAX_PACKET_BYTES,
            stall_limit: STALL_LIMIT,
            painted: true,
        }
    }
}

/// The metadata of an open pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// What the demuxer found.
    pub info: MediaInfo,
    /// The element's timeline (start, duration).
    pub timeline: Timeline,
}

impl Metadata {
    /// The natural size of the video (`videoWidth`, `videoHeight`).
    pub fn natural_size(&self) -> Option<(u32, u32)> {
        self.info.video.as_ref().map(|v| v.natural_size)
    }
}

/// What the pipeline tells its owner ([`Pipeline::update`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// The metadata is known (`loadedmetadata`).
    Metadata(Box<Metadata>),
    /// The data at the current position is decoded for the first time:
    /// the first frame (video), or the first samples (audio alone)
    /// (`loadeddata`).
    FirstFrame,
    /// The media time ranges that the demuxer read changed.
    Buffered(Vec<(i64, i64)>),
    /// Playback ran out of decoded data: the clock stopped at `media_us`
    /// (`waiting`).
    Underrun {
        /// The media time where the clock stopped.
        media_us: i64,
    },
    /// Enough is decoded again: the clock runs.
    Resumed {
        /// The media time where the clock runs on.
        media_us: i64,
    },
    /// A seek ended: the frame at the position is current (`seeked`).
    SeekDone {
        /// The position: the target, or the start for a source that
        /// cannot seek.
        media_us: i64,
    },
    /// A `timeupdate` point while playing.
    TimeUpdate {
        /// The media time.
        media_us: i64,
    },
    /// Every track is decoded to the end (the duration, or the end of the
    /// data when the file has no duration) and the media time reached it: the
    /// clock stopped there and the pipeline no longer plays.
    Ended {
        /// The media time at the end.
        media_us: i64,
    },
    /// The pipeline failed and stopped; [`MediaError::element_code`] gives
    /// the code of `MediaError`.
    Error(MediaError),
}

/// The frame that is shown now.
#[derive(Debug, Clone)]
pub struct CurrentFrame {
    /// Its presentation time (media time).
    pub pts_us: i64,
    /// Its duration.
    pub duration_us: i64,
    /// The pixels; `None` for a frame decoded while the video was not
    /// painted (then [`Pipeline::current_image`] has the last one).
    pub image: Option<Arc<Pixmap>>,
    /// Changes with each new current frame.
    pub generation: u64,
}

/// The counters of the shown and the dropped frames (the numbers that
/// `getVideoPlaybackQuality()` would give; no IDL member reads them yet).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlaybackQuality {
    /// Frames that were shown or dropped.
    pub total_frames: u64,
    /// Frames that were dropped (their successor was due already).
    pub dropped_frames: u64,
}

/// Counters of the pipeline's threads, for tests and measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Wake-ups of the demux thread from a wait.
    pub demux_wakeups: u64,
    /// Wake-ups of the video thread from a wait.
    pub video_wakeups: u64,
    /// Video frames decoded.
    pub frames_decoded: u64,
    /// Video frames converted to RGBA.
    pub frames_converted: u64,
    /// Video frames that the video thread dropped because they were late.
    pub late_frames: u64,
    /// Pixmaps allocated for frames.
    pub pixmaps_allocated: u64,
    /// Pixmaps of the current run that are alive now (in the queue,
    /// current, held by paint, free in the pool; M14 feature 10a).
    pub pixmaps_live: usize,
    /// The bytes of those pixmaps (review of 10a).
    pub pixmap_bytes: u64,
    /// Frames that were not converted because the run held its share of
    /// the frame budget (M14 feature 10a).
    pub frames_over_budget: u64,
}

/// Where the pipeline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// A run started or waits for a place.
    Running,
    /// Closed to free its place; opens again when it needs data.
    Suspended,
    /// Failed; does nothing more.
    Failed,
    /// Closed for good.
    Closed,
}

#[derive(Debug, Clone, Copy)]
struct Seeking {
    epoch: u64,
    target_us: i64,
    /// Report `SeekDone` (not for the seek of a reopen).
    report: bool,
    /// `ended` after the seek (a seek that only brings back the pixels of
    /// the frame at the position, [`Pipeline::set_painted`], keeps it).
    ended: bool,
}

/// A media pipeline. Dropping it closes it.
pub struct Pipeline {
    shared: Arc<Shared>,
    phase: Phase,
    clock: MediaClock,
    metadata: Option<Metadata>,
    playing: bool,
    seeking: Option<Seeking>,
    underrun: bool,
    /// The clock ran past the audio while the pipeline was stalled on its
    /// owner: until the audio covers the media time again, only the video
    /// can stop the clock.
    audio_gap: bool,
    ended: bool,
    output: AudioOutput,
    current: Option<CurrentFrame>,
    last_image: Option<Arc<Pixmap>>,
    /// The size of the full image while the image is a reduced copy
    /// ([`Pipeline::reduce_retained_image`]).
    reduced_from: Option<(u32, u32)>,
    generation: u64,
    first_frame_reported: bool,
    next_timeupdate: Option<i64>,
    /// The media time of the last update (or the seek target).
    position: i64,
    quality: PlaybackQuality,
    capture: Option<Vec<f32>>,
    /// Reports made outside `update` (by `pause`, `suspend`).
    reports: Vec<Report>,
}

impl Pipeline {
    /// Starts a pipeline over the bytes that `opener` gives. Its threads
    /// start when it gets a place among the open pipelines.
    pub fn open(opener: SourceOpener, options: PipelineOptions) -> Pipeline {
        let PipelineOptions {
            video,
            audio,
            slots,
            notify,
            capture_audio,
            clock,
            output,
            packet_bytes,
            stall_limit,
            painted,
        } = options;
        let settings = Settings {
            tracks: (video, audio),
            max_packet_bytes: packet_bytes,
            stall_limit,
            painted,
        };
        let shared = Arc::new(Shared::new(opener, slots, notify, &settings));
        let mut p = Pipeline {
            shared,
            phase: Phase::Running,
            clock: MediaClock::new(clock, 0),
            metadata: None,
            playing: false,
            seeking: None,
            underrun: false,
            audio_gap: false,
            ended: false,
            output,
            current: None,
            last_image: None,
            reduced_from: None,
            generation: 0,
            first_frame_reported: false,
            next_timeupdate: None,
            position: 0,
            quality: PlaybackQuality::default(),
            capture: capture_audio.then(Vec::new),
            reports: Vec::new(),
        };
        {
            let mut st = p.shared.lock();
            st.wants_notify = true;
            st.external = output == AudioOutput::External;
        }
        p.start_run();
        p
    }

    /// Starts a new run of the threads (when a place is free). While too
    /// many detached threads run (ADR 0047 section 11), the pipeline fails
    /// instead.
    fn start_run(&mut self) {
        self.phase = Phase::Running;
        self.audio_gap = false;
        let run = {
            let mut st = self.shared.lock();
            st.run += 1;
            if self.shared.slots.detached_threads() >= MAX_DETACHED_THREADS {
                st.events
                    .push_back(ThreadEvent::Failed(MediaError::Decode(format!(
                        "{MAX_DETACHED_THREADS} media threads are stuck"
                    ))));
                return;
            }
            st.run
        };
        let weak = Arc::downgrade(&self.shared);
        self.shared.slots.acquire(
            self.shared.id,
            Arc::clone(&self.shared.notify),
            Box::new(move |slot| {
                let Some(shared) = weak.upgrade() else {
                    return Some(slot);
                };
                {
                    let mut st = shared.lock();
                    if st.run != run {
                        return Some(slot);
                    }
                    st.active_run = run;
                }
                demux::spawn(&shared, run, slot);
                None
            }),
        );
    }

    /// Ends the current run: its threads stop (a blocked read of the source
    /// returns at once) and give their place back.
    fn end_run(&mut self) {
        let (frames, place): (Vec<_>, _) = {
            let mut st = self.shared.lock();
            st.run += 1;
            st.packets.clear();
            // The ring goes with the run; the next run makes its own.
            st.ring = None;
            st.ring_base = None;
            st.audio_end_us = None;
            st.watch.needs_data = false;
            st.frame_wait = false;
            let place = st.place.take();
            // In the same critical section as the end of the run: no
            // waiter's owner sees the run ended and the place not yet in
            // transit.
            if let Some((_, place)) = &place {
                place.end(self.shared.stall_limit);
            }
            (st.frames.take_all(), place)
        };
        drop(frames);
        // The threads hold the place until they end.
        drop(place);
        // A run that still waits for a place or for frame bytes leaves the
        // queue.
        self.shared.slots.forget(self.shared.id);
        self.shared.slots.forget_frames(self.shared.id);
        // A suspended pipeline keeps only its current frame.
        self.shared.pool.clear();
        if let Some(cancel) = &self.shared.cancel {
            cancel();
        }
        self.shared.demux_cv.notify_all();
        self.shared.video_cv.notify_all();
    }

    /// The metadata, once known.
    pub fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    /// Whether the pipeline plays (`play` and not paused, ended or failed).
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether the last play reached the end.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Whether the pipeline ran out of data while playing.
    pub fn is_underrun(&self) -> bool {
        self.underrun
    }

    /// Whether the pipeline is suspended.
    pub fn is_suspended(&self) -> bool {
        self.phase == Phase::Suspended
    }

    /// Where the playback fails, in media time (microseconds), once the
    /// pipeline failed: the end of the frames or samples that the failing
    /// thread had decoded, less [`READY_AHEAD_US`] (Chromium's `error` comes
    /// about that much before the damage). It depends on the data alone, so an
    /// owner that plays on to it gets the same events whatever the speed of
    /// the threads, and an owner in virtual time has not played past it when
    /// the failure arrives (an owner in real time may have: the clock runs by
    /// itself). `None` for a failure without a place in the media (the
    /// source, the demuxer of a file without audio, the no-progress limit).
    pub fn failed_at(&self) -> Option<i64> {
        if self.phase != Phase::Failed {
            return None;
        }
        let at = self.shared.lock().failed_at?;
        Some(at.saturating_sub(READY_AHEAD_US).max(0))
    }

    /// The media time of the next `timeupdate` point of a clock that runs
    /// (or ran when the pipeline failed): an owner that plays on after a
    /// failure continues the same points.
    pub fn next_timeupdate(&self) -> Option<i64> {
        self.next_timeupdate
    }

    /// Whether the pipeline failed.
    pub fn is_failed(&self) -> bool {
        self.phase == Phase::Failed
    }

    /// Whether the pipeline waits for a place among the open pipelines, or
    /// for its share of the frame budget (M14 feature 10a: its run has a
    /// place and the metadata, and waits until other runs give frame bytes
    /// back; see [`PipelineSlots`]).
    pub fn is_waiting(&self) -> bool {
        let st = self.shared.lock();
        self.phase == Phase::Running && waits(&st)
    }

    /// Whether the pipeline waits for frame bytes (M14 feature 10a).
    pub fn waits_for_frame_budget(&self) -> bool {
        let st = self.shared.lock();
        self.phase == Phase::Running && st.frame_wait
    }

    /// Whether the pipeline waits for a place while a place is in transit
    /// (the run of another pipeline ended and its threads are ending; see
    /// [`PipelineSlots::in_transit`]): the place may come to this pipeline
    /// at any moment. An owner in virtual time counts it as pending work,
    /// so that the time at which the place arrives does not depend on how
    /// fast the ending threads are (M14 feature 6a). When the transit ends,
    /// the pipeline calls the owner's notify.
    pub fn awaits_handover(&self) -> bool {
        self.is_waiting() && self.shared.slots.in_transit() > 0
    }

    /// The media time of the last update, or the target of a seek: the
    /// official playback position.
    pub fn position(&self) -> i64 {
        self.position
    }

    /// The media time at clock time `now`, for a read of `currentTime`
    /// between two updates (M14 feature 6b): while the clock runs, the clock
    /// time moves it on from the last update (up to the end); else the last
    /// position. It changes nothing: the reports and the frames wait for
    /// [`Pipeline::update`].
    pub fn time_at(&self, now: i64) -> i64 {
        if !self.clock.is_running() {
            return self.position;
        }
        let end = {
            let st = self.shared.lock();
            self.end_time(&st)
        };
        self.clock
            .media_time(now)
            .clamp(self.position, end.max(self.position))
    }

    /// The frame shown now.
    pub fn current_frame(&self) -> Option<&CurrentFrame> {
        self.current.as_ref()
    }

    /// The image of the frame shown now, or of the last frame that was
    /// converted (while the video is not painted, frames are not
    /// converted).
    pub fn current_image(&self) -> Option<&Arc<Pixmap>> {
        self.last_image.as_ref()
    }

    /// The bytes of the frame image that the pipeline keeps while it holds
    /// no place (suspended, waiting, failed or closed: the frame budget of
    /// the slots no longer counts it; M14 feature 10a): `None` while a run
    /// holds a place, or without an image.
    pub fn retained_image_bytes(&self) -> Option<u64> {
        if self.holds_place() {
            return None;
        }
        let image = self
            .current
            .as_ref()
            .and_then(|c| c.image.as_ref())
            .or(self.last_image.as_ref())?;
        Some(u64::from(image.width()) * u64::from(image.height()) * 4)
    }

    /// Drops the frame image that the pipeline keeps while it holds no
    /// place (the owner's budget of retained frames, M14 feature 10a); the
    /// frame keeps its time, so the image comes back through the quiet seek
    /// of [`Pipeline::set_painted`] when the video is painted again. False
    /// when a run holds a place (its frames are in the slots' budget) or
    /// there is no image.
    pub fn drop_retained_image(&mut self) -> bool {
        if self.holds_place() {
            return false;
        }
        self.reduced_from = None;
        let had = self.last_image.take().is_some();
        let current = self.current.as_mut().and_then(|c| c.image.take()).is_some();
        had || current
    }

    /// Replaces the frame image that the pipeline keeps while it holds no
    /// place by `image`, a reduced copy of it (the owner's budget of
    /// retained frames keeps a copy at the size of the element's box; M14
    /// feature 10a, review fixes). The frame keeps its time;
    /// [`Pipeline::reduced_from`] gives the size of the full image until a
    /// new frame comes. False when a run holds a place (its frames are in
    /// the slots' budget) or there is no image.
    pub fn reduce_retained_image(&mut self, image: Arc<Pixmap>) -> bool {
        if self.holds_place() {
            return false;
        }
        let current = self.current.as_ref().and_then(|c| c.image.as_ref());
        let Some(full) = current.or(self.last_image.as_ref()) else {
            return false;
        };
        let full = (full.width(), full.height());
        if let Some(c) = self.current.as_mut()
            && c.image.is_some()
        {
            c.image = Some(Arc::clone(&image));
        }
        if self.last_image.is_some() {
            self.last_image = Some(image);
        }
        self.reduced_from = Some(self.reduced_from.unwrap_or(full));
        true
    }

    /// The size of the full frame image while the image is a reduced copy
    /// ([`Pipeline::reduce_retained_image`]).
    pub fn reduced_from(&self) -> Option<(u32, u32)> {
        self.reduced_from
    }

    /// The image is a reduced copy that is too small now (the element's box
    /// grew): the frame at the position is decoded again at full size by
    /// the quiet seek of [`Pipeline::set_painted`] (a suspended pipeline
    /// opens again); the copy shows until the frame comes. A playing video
    /// shows its next frame anyway. Does nothing for a full image or a video
    /// that is not painted.
    pub fn restore_full_image(&mut self) {
        if self.reduced_from.is_some() && self.shared.lock().painted {
            self.quiet_seek();
        }
    }

    /// Decodes the frame at the position again without a report, when the
    /// pipeline is paused, not seeking, and has its metadata.
    fn quiet_seek(&mut self) {
        if !self.playing
            && self.seeking.is_none()
            && matches!(self.phase, Phase::Running | Phase::Suspended)
            && self.metadata.is_some()
        {
            self.start_seek(self.position, false, self.ended);
        }
    }

    /// Whether a run of the pipeline holds a place (its threads may run).
    fn holds_place(&self) -> bool {
        self.phase == Phase::Running && !waits(&self.shared.lock())
    }

    /// The media time ranges that the demuxer read.
    pub fn buffered(&self) -> Vec<(i64, i64)> {
        self.shared.lock().buffered.clone()
    }

    /// The counters of the frames shown and dropped, also those that the
    /// video thread dropped because they were late.
    pub fn quality(&self) -> PlaybackQuality {
        let late = self.shared.counters.late_frames.load(Ordering::Relaxed);
        PlaybackQuality {
            total_frames: self.quality.total_frames + late,
            dropped_frames: self.quality.dropped_frames + late,
        }
    }

    /// Counters of the threads.
    pub fn stats(&self) -> PipelineStats {
        let c = &self.shared.counters;
        PipelineStats {
            demux_wakeups: c.demux_wakeups.load(Ordering::Relaxed),
            video_wakeups: c.video_wakeups.load(Ordering::Relaxed),
            frames_decoded: c.frames_decoded.load(Ordering::Relaxed),
            frames_converted: c.frames_converted.load(Ordering::Relaxed),
            late_frames: c.late_frames.load(Ordering::Relaxed),
            pixmaps_allocated: self.shared.pool.allocated(),
            pixmaps_live: self.shared.pool.live(),
            pixmap_bytes: self.shared.pool.live_bytes(),
            frames_over_budget: c.frames_over_budget.load(Ordering::Relaxed),
        }
    }

    /// The frame cap of the current run (the most frames in its queue; 2 to
    /// 6, fewer when the frame budget is short).
    pub fn frame_cap(&self) -> usize {
        self.shared.lock().frame_cap
    }

    /// The sample ring of the audio track while a run is open (the audio
    /// output reads it; feature 8's mixer, with [`AudioOutput::External`]).
    /// Each run (the open, a reopen after a suspend) has its own ring. Its
    /// gain is the element's volume. [`SampleRing::base`] maps its positions
    /// to media times without the pipeline's lock.
    pub fn samples(&self) -> Option<Arc<SampleRing>> {
        self.shared.lock().ring.clone()
    }

    /// Who consumes the samples now.
    pub fn output(&self) -> AudioOutput {
        self.output
    }

    /// Changes the consumer of the ring (M14 feature 8): `External` while the
    /// element is in the audio mixer, `Silent` otherwise. The owner switches
    /// to `External` before it adds the ring to the mixer and to `Silent`
    /// when it takes it out (the mixer may read a few more frames while its
    /// fade-out ends: both consumers only read, so frames are skipped at
    /// worst). With `External` the demux thread refills the ring by itself
    /// (a timed wait), as the mixer notifies nobody.
    pub fn set_output(&mut self, output: AudioOutput) {
        if self.output == output {
            return;
        }
        self.output = output;
        self.shared.lock().external = output == AudioOutput::External;
        // A demux thread that waits for room without a timeout looks again.
        self.shared.demux_cv.notify_all();
    }

    /// Whether the media clock runs (playing, with its data, no seek, no
    /// underrun, not at the end).
    pub fn is_clock_running(&self) -> bool {
        self.clock.is_running()
    }

    /// Moves the clock to another source without a jump: the media time
    /// that the old source's clock time `old_now` gives stays, and the new
    /// source's `new_now` is the same moment ([`MediaClock::rebase`]). From
    /// then on the owner passes the clock time of the new source. With
    /// [`ClockSource::Audio`] an empty ring is an underrun (the mixer's clock
    /// stands still), and the end of the audio track ends the audio clock
    /// ([`Pipeline::audio_finished`]).
    pub fn rebase_clock(&mut self, source: ClockSource, old_now: i64, new_now: i64) {
        self.clock.rebase(source, old_now, new_now);
    }

    /// Whether the audio track is decoded to its end and the ring is empty:
    /// nothing is left for the audio clock to count (the owner then takes the
    /// clock back to the page clock: the video, or the end of the media,
    /// goes on). False without an audio track.
    pub fn audio_finished(&self) -> bool {
        let st = self.shared.lock();
        st.audio_on
            && st.audio_drained == Some(st.epoch)
            && st.ring.as_ref().is_none_or(|r| r.available() == 0)
    }

    /// Sets the gain of the samples (the element's `volume`, 0 when muted),
    /// also for the rings of later runs.
    pub fn set_gain(&mut self, gain: f32) {
        let mut st = self.shared.lock();
        st.gain = gain;
        if let Some(ring) = &st.ring {
            ring.set_gain(gain);
        }
    }

    /// The samples that the silent output consumed since the last call
    /// (interleaved stereo at 48 kHz), with `capture_audio`.
    pub fn take_captured_audio(&mut self) -> Vec<f32> {
        self.capture
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Whether the video is painted: frames that are not painted are
    /// decoded (the decoder and the clock need them) but not converted
    /// (ADR 0047 section 6, "Hidden elements").
    ///
    /// A video that becomes painted while the current frame has no pixels
    /// (it was decoded while the video was not painted) and that is not
    /// playing seeks to its position and shows the frame there, without a
    /// `SeekDone` report and without a change of the position or of `ended`
    /// (the seek that the element does not see). A suspended pipeline opens
    /// again for it (M14 feature 10a: the owner may have dropped its image,
    /// [`Pipeline::drop_retained_image`]; [`Pipeline::is_suspended`] then
    /// turns false). A playing video shows its next frame.
    pub fn set_painted(&mut self, painted: bool) {
        let was = std::mem::replace(&mut self.shared.lock().painted, painted);
        let stale = self.current.as_ref().is_some_and(|c| c.image.is_none());
        if painted && !was && stale {
            self.quiet_seek();
        }
    }

    /// Starts playing at clock time `now` (a suspended pipeline opens
    /// again). The clock runs once the metadata is known, no seek runs and
    /// the data is there. A pipeline that ended stays at its end: the owner
    /// seeks first (HTML's `play()` at the end seeks to the start).
    pub fn play(&mut self, now: i64) {
        match self.phase {
            Phase::Failed | Phase::Closed => return,
            Phase::Suspended => self.resume(),
            Phase::Running => {}
        }
        if self.playing {
            return;
        }
        self.playing = true;
        self.shared.lock().playing = true;
        self.sync_clock(now);
    }

    /// Pauses at clock time `now`.
    pub fn pause(&mut self, now: i64) {
        if !self.playing {
            return;
        }
        let reports = self.update(now);
        self.reports.extend(reports);
        self.playing = false;
        self.clock.stop(now);
        self.next_timeupdate = None;
        self.shared.lock().playing = false;
    }

    /// Seeks to the media time `target_us`: the position is the target at
    /// once; `SeekDone` comes when the frame and the samples at the target
    /// are decoded. A seek replaces a seek that has not ended (only the last
    /// one runs). Before the metadata it waits for it; a suspended pipeline
    /// opens again; a source that cannot seek plays again from the start.
    pub fn seek(&mut self, target_us: i64) {
        self.start_seek(target_us, true, false);
    }

    /// The seek of [`Pipeline::seek`]; `report` says whether `SeekDone`
    /// comes, `ended` whether the pipeline is at its end after the seek.
    fn start_seek(&mut self, target_us: i64, report: bool, ended: bool) {
        match self.phase {
            Phase::Failed | Phase::Closed => return,
            Phase::Suspended => {
                self.seeking = Some(Seeking {
                    epoch: 0,
                    target_us,
                    report,
                    ended,
                });
                self.position = target_us;
                self.resume();
                return;
            }
            Phase::Running => {}
        }
        let epoch = self.request_seek(target_us);
        self.seeking = Some(Seeking {
            epoch,
            target_us,
            report,
            ended,
        });
        self.position = target_us;
        self.clock.set(target_us);
        self.underrun = false;
        self.audio_gap = false;
        self.ended = false;
        self.next_timeupdate = None;
    }

    /// Tells the demux thread to seek; returns the new epoch.
    fn request_seek(&mut self, target_us: i64) -> u64 {
        let (epoch, frames): (u64, Vec<_>) = {
            let mut st = self.shared.lock();
            st.epoch += 1;
            let epoch = st.epoch;
            st.seek = Some(SeekCmd { epoch, target_us });
            // The video thread drops a frame as late when its successor is
            // due at `media_now` (M14 feature 7b): until the owner's next
            // update (which waits for the seek), the position is the target.
            // With the old position (the end of a loop) the frames that
            // follow the target frame were all late, and the picture froze
            // for the rest of the file's length.
            st.media_now = target_us;
            st.video_drained = None;
            st.audio_drained = None;
            st.video_end_us = None;
            st.wants_notify = true;
            (epoch, st.frames.take_all())
        };
        for f in frames {
            self.shared.recycle(f);
        }
        self.shared.demux_cv.notify_all();
        self.shared.video_cv.notify_all();
        epoch
    }

    /// Suspends at clock time `now` (ADR 0047 section 6): pauses, closes the
    /// demuxer, the decoders and the threads, and gives the place to the
    /// next pipeline; keeps the metadata, the current frame and the
    /// position. `play`, `seek` and `resume` open it again.
    pub fn suspend(&mut self, now: i64) {
        if self.phase != Phase::Running {
            return;
        }
        self.pause(now);
        let reports = self.update(now);
        self.reports.extend(reports);
        self.end_run();
        self.phase = Phase::Suspended;
    }

    /// Opens a suspended pipeline again and seeks to its position.
    pub fn resume(&mut self) {
        if self.phase != Phase::Suspended {
            return;
        }
        let target = self.seeking.map_or(self.position, |s| s.target_us);
        let report = self.seeking.is_some_and(|s| s.report);
        let ended = self.seeking.is_some_and(|s| s.ended);
        if self.metadata.is_some() || self.seeking.is_some() {
            let epoch = self.request_seek(target);
            self.seeking = Some(Seeking {
                epoch,
                target_us: target,
                report,
                ended,
            });
        }
        self.start_run();
    }

    /// Closes the pipeline: its threads end and give their place back.
    pub fn close(&mut self) {
        if self.phase == Phase::Closed {
            return;
        }
        self.playing = false;
        self.end_run();
        self.phase = Phase::Closed;
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.close();
    }
}

/// How far the decoding is ahead of the media time that the owner waits for
/// (M14 feature 6b, review fix): the owner in virtual time goes on to the
/// media time `t` only when the data up to `t` + this is decoded, or the
/// decoders are done or stopped. So when a decoder fails at the place `F`
/// (the end of its last good frame or samples), the owner has not played
/// past `F` minus this: the playback can go on from where it is to a place
/// that depends on the data alone ([`Pipeline::failed_at`]). Chromium also
/// decodes about 0.2 s ahead of its playback (its `error` comes 0.15 to
/// 0.25 s before the place of swb's decoder in the probe rows
/// `late-failure-damage-*`).
const READY_AHEAD_US: i64 = 200_000;

/// Whether the current run waits for a place, or for its frame bytes.
fn waits(st: &State) -> bool {
    st.active_run != st.run || st.frame_wait
}

/// Whether the data for the media time `t` is decoded, and a little more
/// ([`READY_AHEAD_US`]): the frame that is current at `t` and the samples up
/// to `t` (or the queue or the ring is full, so nothing more can be decoded
/// before the owner moves on).
fn ready_for(st: &State, t: i64) -> bool {
    let ahead = t.saturating_add(READY_AHEAD_US);
    let video = !st.video_on
        || st.video_drained == Some(st.epoch)
        || st.frames.len() >= st.frame_cap
        || st
            .frames
            .iter()
            .any(|f| f.epoch == st.epoch && f.pts_us >= ahead);
    let audio = !st.audio_on
        || st.audio_drained == Some(st.epoch)
        || (st.demux_epoch == st.epoch
            && (st.ring_full() || st.audio_end_us.is_some_and(|e| e >= ahead)));
    video && audio
}

/// Whether the audio's decoded data reaches the media time `m` (or the
/// audio ended).
fn audio_covers(st: &State, m: i64) -> bool {
    st.audio_drained == Some(st.epoch)
        || (st.demux_epoch == st.epoch && st.audio_end_us.is_some_and(|e| e >= m))
}

/// Whether every track that plays is decoded to the end `end`: it ended,
/// or its decoded data reaches past `end` (what comes later does not
/// play). Both only grow with decoding, so the end is the same on every
/// run.
fn end_reached(st: &State, end: i64) -> bool {
    let video = !st.video_on
        || st.video_drained == Some(st.epoch)
        || st.video_end_us.is_some_and(|e| e >= end);
    let audio = !st.audio_on
        || st.audio_drained == Some(st.epoch)
        || (st.demux_epoch == st.epoch && st.audio_end_us.is_some_and(|e| e >= end));
    video && audio
}

/// Whether every track that plays has all its data decoded.
fn drained(st: &State) -> bool {
    (!st.video_on || st.video_drained == Some(st.epoch))
        && (!st.audio_on || st.audio_drained == Some(st.epoch))
}

/// Whether the seek `s` has its frame and samples (the audio decoder drops
/// the samples before the target, so any sample of the epoch is at it).
fn seek_ready(st: &State, s: Seeking) -> bool {
    if st.epoch != s.epoch || st.seek.is_some() {
        return false;
    }
    let video = !st.video_on
        || st.video_drained == Some(s.epoch)
        || st.frames.iter().any(|f| f.epoch == s.epoch && f.at_target);
    let audio = !st.audio_on
        || st.audio_drained == Some(s.epoch)
        || (st.demux_epoch == s.epoch && st.audio_end_us.is_some());
    video && audio
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::opener::SourceOpener;
    use crate::pipeline::slots::PipelineSlots;

    /// A pipeline whose threads are gone (they fail to open the empty
    /// source) and whose state the test sets: the video plays, its decoder
    /// has not drained, a frame lasts past the end.
    fn playing_near_the_end(duration_us: i64) -> Pipeline {
        let mut options = PipelineOptions::new(Arc::new(|| {}));
        options.slots = PipelineSlots::new(1);
        let mut p = Pipeline::open(SourceOpener::memory(Vec::new(), true), options);
        p.end_run();
        p.metadata = Some(Metadata {
            info: MediaInfo {
                container: String::new(),
                duration_us: Some(duration_us),
                start_us: Some(0),
                seekable: true,
                stream_count: 1,
                video: None,
                audio: None,
            },
            timeline: Timeline {
                offset_us: 0,
                start_us: 0,
                duration_us: Some(duration_us),
            },
        });
        p.current = Some(CurrentFrame {
            pts_us: 0,
            duration_us: i64::MAX / 2,
            image: None,
            generation: 0,
        });
        p.playing = true;
        p.position = duration_us - 100_000;
        p.clock.set(p.position);
        p.clock.start(0);
        p.next_timeupdate = Some(p.position);
        p.shared.lock().video_on = true;
        p
    }

    /// The clock reaches the end before the decoder drained: no
    /// `TimeUpdate` there (the end reports the last one), then `Ended`
    /// when the decoder drains, whatever the threads' speed.
    #[test]
    fn the_end_owns_the_last_timeupdate() {
        let end = 1_000_000;
        let mut p = playing_near_the_end(end);
        let shared = Arc::clone(&p.shared);
        let mut st = shared.lock();
        let mut out = Vec::new();
        p.advance(200_000, &mut st, &mut out);
        assert!(
            out.iter()
                .all(|r| !matches!(r, Report::TimeUpdate { .. } | Report::Ended { .. })),
            "{out:?}"
        );
        assert_eq!(p.position, end);
        // The clock is at the end and the decoder is not done.
        p.advance(300_000, &mut st, &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert!(p.playing && !p.ended);
        // The decoder drains: the end.
        st.video_drained = Some(st.epoch);
        p.advance(400_000, &mut st, &mut out);
        assert!(matches!(out.as_slice(), [Report::Ended { media_us }] if *media_us == end));
    }
}
