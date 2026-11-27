//! The state that a pipeline's owner and its two threads share: commands
//! (the run, seeks), the video packet queue, the decoded frame queue, what
//! the sample ring holds, what each thread waits for, the events of the
//! threads, and the counters.
//!
//! One mutex guards it; the demux thread waits on `demux_cv` and the video
//! thread on `video_cv`, and whoever changes what a thread waits for
//! notifies it. Nothing polls: a thread that has nothing to do blocks until
//! it is notified (ADR 0047 section 6, "Hidden elements": paused pipelines
//! use no CPU).
//!
//! **Notifies.** The two queues notify by themselves: each change of the
//! video packet queue ([`PacketQueue`]) and each removal from the frame
//! queue ([`FrameQueue`]) notifies the thread at the other end, under the
//! lock, so no code path can take a frame or a packet and forget the
//! notify. The other changes that a thread waits for notify explicitly: a
//! new run (`run`), a seek (`seek`, `epoch`), and room in the sample ring
//! (the ring is outside the lock: the silent output's `update`, and for an
//! external consumer `update` and `has_pending_work`). The watchdog wakes
//! threads that wait for something that no longer holds (a lost wake-up,
//! [`Shared::check_progress`]).
//!
//! **Runs.** Each start of the threads (the open, a reopen after a suspend)
//! is a run with a new number; a thread whose run is no longer
//! `State::run` stops at its next check. Closing, suspending and a fatal
//! error end the run this way.
//!
//! **Epochs.** Each seek starts an epoch. Frames carry the epoch they were
//! decoded in, and the owner uses only data of the current epoch, so data
//! of a seek that was replaced never shows.
//!
//! **Waits.** Before a thread waits, it records what it waits for
//! (`demux_wait`, `video_wait`). When both threads wait for something that
//! only the owner can give (room in a queue that the owner empties), the
//! pipeline is stalled on its owner ([`State::stalled_on_owner`]): it has no
//! pending work, and the owner moves on without the data it lacks (a file
//! whose tracks lie far apart in the file, ADR 0047 section 7).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;

use super::opener::SourceOpener;
use super::pool::FramePool;
use super::slots::{PipelineSlots, RunPlace};
use crate::decode::Packet;
use crate::origin::Timeline;
use crate::ring::SampleRing;
use crate::{MediaError, MediaInfo, OUTPUT_RATE};

/// The owner's wake-up: called from the pipeline's threads when a report
/// is ready or data that the owner waits for was decoded (like the
/// network's wake-up of the event loop).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// The samples of the ring: 1 s at 48 kHz (ADR 0047 section 6).
pub(crate) const RING_FRAMES: usize = OUTPUT_RATE as usize;
/// The most bytes of video packets in the queue (ADR 0047 section 6; the
/// default of `PipelineOptions::packet_bytes`).
pub(crate) const MAX_PACKET_BYTES: usize = 16 << 20;
/// The most packets in the video packet queue (an `End` or `Flush` always
/// goes in), and in the queue of audio packets that wait for room in the
/// ring: tiny packets cannot grow a queue without bound.
pub(crate) const MAX_PACKETS: usize = 4096;
/// The most media time of video packets in the queue.
pub(crate) const MAX_PACKET_SPAN_US: i64 = 2_000_000;
/// The most decoded frames in the queue, and the fewest.
pub(crate) const MAX_FRAMES: usize = 6;
pub(crate) const MIN_FRAMES: usize = 2;
/// The most bytes of decoded frames in the queue.
pub(crate) const MAX_FRAME_BYTES: u64 = 96 << 20;
/// The pixmaps of a run besides its frame queue (M14 feature 10a): the
/// current frame, the frame being converted, and the one that paint holds
/// until it takes the next.
pub(crate) const EXTRA_PIXMAPS: usize = 3;
/// The share of the frame budget of a video of one frame size (M14 feature
/// 10a): the queue that its bounds want (2 to 6 frames within 96 MiB) and
/// [`EXTRA_PIXMAPS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameShare {
    /// The bytes of one RGBA frame.
    pub(crate) frame: u64,
    /// The frames of the queue that the bounds want.
    pub(crate) queue: usize,
}

impl FrameShare {
    /// The share of frames of `width` x `height` pixels.
    pub(crate) fn of(width: u32, height: u32) -> FrameShare {
        let frame = u64::from(width.max(1)) * u64::from(height.max(1)) * 4;
        let queue = usize::try_from(MAX_FRAME_BYTES / frame)
            .unwrap_or(MAX_FRAMES)
            .clamp(MIN_FRAMES, MAX_FRAMES);
        FrameShare { frame, queue }
    }

    /// The bytes of a queue of `frames` and the extra pixmaps.
    fn bytes(self, frames: usize) -> u64 {
        self.frame
            .saturating_mul(u64::try_from(frames + EXTRA_PIXMAPS).unwrap_or(u64::MAX))
    }

    /// The fewest bytes that playback needs.
    pub(crate) fn min(self) -> u64 {
        self.bytes(MIN_FRAMES)
    }

    /// The bytes of the whole queue.
    pub(crate) fn want(self) -> u64 {
        self.bytes(self.queue)
    }

    /// The frame cap that `granted` bytes allow (at least [`MIN_FRAMES`]: a
    /// grant is never below [`FrameShare::min`]).
    pub(crate) fn cap(self, granted: u64) -> usize {
        usize::try_from(granted / self.frame)
            .unwrap_or(usize::MAX)
            .saturating_sub(EXTRA_PIXMAPS)
            .clamp(MIN_FRAMES, self.queue)
    }
}

/// Audio that the ring holds before the time bound of the video packet
/// queue holds: below it, the video packets may span more than 2 s (up to
/// their byte bound), so a file whose video runs ahead of its audio does
/// not block the audio.
pub(crate) const AUDIO_LOW_FRAMES: u64 = OUTPUT_RATE as u64 / 4;
/// The no-progress limit (ADR 0047 section 11; the default of
/// `PipelineOptions::stall_limit`).
pub(crate) const STALL_LIMIT: Duration = Duration::from_secs(10);

/// An item of the video packet queue.
pub(crate) enum VideoItem {
    /// A packet of the video track.
    Packet(Packet),
    /// A seek: flush the decoder; the frames from here on belong to
    /// `epoch`; the frame at `target_us` (stream time) is the seek's.
    Flush { epoch: u64, target_us: i64 },
    /// The end of the stream: drain the decoder.
    End,
}

/// The video packet queue and what its bounds need: the bytes and the
/// count of its packets, and the times of its packets in queue order (so
/// the span needs no scan). It notifies the thread at the other end of each
/// change: a push the video thread (an item), a removal the demux thread
/// (room). The fields are private, so every change goes through these
/// methods.
pub(crate) struct PacketQueue {
    items: VecDeque<VideoItem>,
    bytes: usize,
    packets: usize,
    /// The times of the queued packets that have one, in queue order.
    times: VecDeque<i64>,
    /// The demux thread's condition variable (it waits for room).
    room: Arc<Condvar>,
    /// The video thread's condition variable (it waits for an item).
    filled: Arc<Condvar>,
}

impl PacketQueue {
    pub(crate) fn new(room: Arc<Condvar>, filled: Arc<Condvar>) -> PacketQueue {
        PacketQueue {
            items: VecDeque::new(),
            bytes: 0,
            packets: 0,
            times: VecDeque::new(),
            room,
            filled,
        }
    }

    /// Queues `item` and notifies the video thread.
    pub(crate) fn push(&mut self, item: VideoItem) {
        if let VideoItem::Packet(p) = &item {
            self.bytes = self.bytes.saturating_add(p.size());
            self.packets += 1;
            if let Some(t) = p.pts_us() {
                self.times.push_back(t);
            }
        }
        self.items.push_back(item);
        // All waiters: a thread of an older run that has not seen the new
        // run yet must not take the only notify.
        self.filled.notify_all();
    }

    /// Takes the first item and notifies the demux thread (room).
    pub(crate) fn pop(&mut self) -> Option<VideoItem> {
        let item = self.items.pop_front()?;
        if let VideoItem::Packet(p) = &item {
            self.bytes = self.bytes.saturating_sub(p.size());
            self.packets = self.packets.saturating_sub(1);
            if p.pts_us().is_some() {
                self.times.pop_front();
            }
        }
        self.room.notify_all();
        Some(item)
    }

    /// Empties the queue and notifies the demux thread (room).
    pub(crate) fn clear(&mut self) {
        let had_items = !self.items.is_empty();
        self.items.clear();
        self.times.clear();
        self.bytes = 0;
        self.packets = 0;
        if had_items {
            self.room.notify_all();
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether the queue is at a bound: `max_bytes`, [`MAX_PACKETS`], or
    /// (with `timed`) [`MAX_PACKET_SPAN_US`] from its first packet to its
    /// last (decode order, so B-frames make the span a frame or two
    /// inexact).
    fn full(&self, max_bytes: usize, timed: bool) -> bool {
        if self.bytes >= max_bytes || self.packets >= MAX_PACKETS {
            return true;
        }
        timed
            && matches!(
                (self.times.front(), self.times.back()),
                (Some(a), Some(b)) if b.saturating_sub(*a) >= MAX_PACKET_SPAN_US
            )
    }
}

/// A decoded frame in the queue.
pub(crate) struct QueuedFrame {
    pub(crate) epoch: u64,
    /// Media time.
    pub(crate) pts_us: i64,
    pub(crate) duration_us: i64,
    /// `None` when the video is not painted (no conversion).
    pub(crate) image: Option<Arc<Pixmap>>,
    /// The frame of a seek (shown when the seek ends).
    pub(crate) at_target: bool,
}

/// The queue of decoded frames. Each removal notifies the video thread,
/// which may wait for room (`VideoWait::Frames`); the frames are private,
/// so no code path can take one without the notify (a lost wake-up of the
/// video thread hung virtual time: review of feature 5).
pub(crate) struct FrameQueue {
    items: VecDeque<QueuedFrame>,
    /// The video thread's condition variable.
    room: Arc<Condvar>,
}

impl FrameQueue {
    pub(crate) fn new(room: Arc<Condvar>) -> FrameQueue {
        FrameQueue {
            items: VecDeque::new(),
            room,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn front(&self) -> Option<&QueuedFrame> {
        self.items.front()
    }

    pub(crate) fn iter(&self) -> std::collections::vec_deque::Iter<'_, QueuedFrame> {
        self.items.iter()
    }

    pub(crate) fn push_back(&mut self, frame: QueuedFrame) {
        self.items.push_back(frame);
    }

    /// Takes the first frame and notifies the video thread.
    pub(crate) fn pop_front(&mut self) -> Option<QueuedFrame> {
        let frame = self.items.pop_front()?;
        self.notify();
        Some(frame)
    }

    /// Takes the first `n` frames (at most all) and notifies the video
    /// thread.
    pub(crate) fn take_front(&mut self, n: usize) -> Vec<QueuedFrame> {
        let n = n.min(self.items.len());
        let taken: Vec<_> = self.items.drain(..n).collect();
        if !taken.is_empty() {
            self.notify();
        }
        taken
    }

    /// Takes every frame and notifies the video thread.
    pub(crate) fn take_all(&mut self) -> Vec<QueuedFrame> {
        self.take_front(self.items.len())
    }

    fn notify(&self) {
        // Under the pipeline's lock; all waiters, as for the packet queue.
        self.room.notify_all();
    }
}

/// A seek for the demux thread.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeekCmd {
    pub(crate) epoch: u64,
    /// Media time.
    pub(crate) target_us: i64,
}

/// What a thread tells the owner.
pub(crate) enum ThreadEvent {
    /// The demuxer opened (the first open of the pipeline).
    Opened(Box<(MediaInfo, Timeline)>),
    /// The pipeline failed; its run ended.
    Failed(MediaError),
}

/// What the demux thread waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DemuxWait {
    /// Room in the video packet queue; `audio_pending`: audio also waits
    /// for room in the ring.
    Packets { audio_pending: bool },
    /// Room in the ring.
    Ring,
    /// Nothing: everything is read (a seek or the end of the run wakes
    /// it).
    Idle,
}

/// What the video thread waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoWait {
    /// An item in the packet queue.
    Packets,
    /// Room in the frame queue for a frame of `epoch`.
    Frames { epoch: u64 },
}

/// The owner's need of data, for the no-progress limit (ADR 0047 section
/// 11).
pub(crate) struct Watch {
    /// The owner waits for data now (pending work, a seek, an underrun).
    pub(crate) needs_data: bool,
    /// When `needs_data` became true.
    pub(crate) since: Instant,
    /// The watchdog has an entry for the pipeline.
    pub(crate) armed: bool,
    /// When the watchdog found every thread waiting for something that no
    /// longer holds (a lost wake-up), with no progress since.
    pub(crate) lost: Option<Instant>,
}

/// The state under the mutex.
pub(crate) struct State {
    /// The current run.
    pub(crate) run: u64,
    /// The run whose demux thread started (it got a place).
    pub(crate) active_run: u64,
    /// The place of `active_run` and its threads (taken to detach them).
    pub(crate) place: Option<(u64, Arc<RunPlace>)>,
    /// The current epoch (the owner's).
    pub(crate) epoch: u64,
    /// A seek that the demux thread has not taken yet.
    pub(crate) seek: Option<SeekCmd>,
    /// The epoch of the data that the demux thread produces now.
    pub(crate) demux_epoch: u64,
    /// The metadata, from the first open.
    pub(crate) info: Option<(MediaInfo, Timeline)>,
    /// The tracks that play (known after the open).
    pub(crate) video_on: bool,
    pub(crate) audio_on: bool,
    pub(crate) packets: PacketQueue,
    /// The byte bound of the video packet queue.
    pub(crate) max_packet_bytes: usize,
    pub(crate) frames: FrameQueue,
    pub(crate) frame_cap: usize,
    /// The bytes of one RGBA frame that the run's reservation of frame
    /// bytes is for (0 before it reserved): a larger decoded frame grows the
    /// reservation (`video.rs`).
    pub(crate) frame_size: u64,
    /// The end (media time) of the last queued frame of the current epoch.
    pub(crate) video_end_us: Option<i64>,
    /// The epoch whose frames are all decoded (the decoder drained).
    pub(crate) video_drained: Option<u64>,
    /// The epoch whose samples are all in the ring.
    pub(crate) audio_drained: Option<u64>,
    /// The sample ring of the current run (with an audio track; made at
    /// the open, dropped when the run ends, so a suspended pipeline holds
    /// none).
    pub(crate) ring: Option<Arc<SampleRing>>,
    /// The gain of the rings.
    pub(crate) gain: f32,
    /// Another consumer (the audio mixer, on its real-time thread) reads the
    /// ring and notifies nobody: the demux thread, which waits for room in
    /// the ring, wakes by itself (a timed wait,
    /// [`State::external_wait`]), so a page thread that is busy does not
    /// starve the audio (M14 feature 8).
    pub(crate) external: bool,
    /// The ring position and media time of the first sample of
    /// `demux_epoch` (`None` before the first sample).
    pub(crate) ring_base: Option<(u64, i64)>,
    /// The media time after the last sample of `demux_epoch` in the ring.
    pub(crate) audio_end_us: Option<i64>,
    /// What the threads wait for, with their run.
    pub(crate) demux_wait: Option<(u64, DemuxWait)>,
    pub(crate) video_wait: Option<(u64, VideoWait)>,
    /// The media time ranges that the demuxer read.
    pub(crate) buffered: Vec<(i64, i64)>,
    pub(crate) buffered_dirty: bool,
    pub(crate) events: VecDeque<ThreadEvent>,
    /// The owner waits for data: the next thread that produces some calls
    /// the owner's notify.
    pub(crate) wants_notify: bool,
    /// The video is painted (frames are converted).
    pub(crate) painted: bool,
    /// The owner plays.
    pub(crate) playing: bool,
    /// The media time of the owner's last update.
    pub(crate) media_now: i64,
    /// Where the failure of the run lies in media time (the end of what the
    /// failing thread had decoded), when the thread knows; set with the
    /// `Failed` event.
    pub(crate) failed_at: Option<i64>,
    pub(crate) watch: Watch,
    /// The demux thread of the current run waits for bytes of the frame
    /// budget (M14 feature 10a): the pipeline waits as for a place, without
    /// pending work.
    pub(crate) frame_wait: bool,
}

/// Counters for tests and measurements.
#[derive(Default)]
pub(crate) struct Counters {
    pub(crate) demux_wakeups: AtomicU64,
    pub(crate) video_wakeups: AtomicU64,
    pub(crate) frames_decoded: AtomicU64,
    pub(crate) frames_converted: AtomicU64,
    pub(crate) late_frames: AtomicU64,
    /// Frames that were not converted because the run held its share of
    /// the frame budget.
    pub(crate) frames_over_budget: AtomicU64,
}

impl Counters {
    pub(crate) fn add(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// When the pipeline's threads last made progress (read bytes or a
/// packet, decoded a frame or samples); shared with the byte source.
pub(crate) struct Progress {
    born: Instant,
    /// Nanoseconds from `born` to the last progress.
    last_ns: AtomicU64,
}

impl Progress {
    fn new() -> Progress {
        Progress {
            born: Instant::now(),
            last_ns: AtomicU64::new(0),
        }
    }

    /// Records progress now.
    pub(crate) fn mark(&self) {
        let ns = u64::try_from(self.born.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.last_ns.fetch_max(ns, Ordering::Relaxed);
    }

    /// The time of the last progress.
    pub(crate) fn last(&self) -> Instant {
        self.born + Duration::from_nanos(self.last_ns.load(Ordering::Relaxed))
    }
}

/// What the owner and the threads of one pipeline share.
pub(crate) struct Shared {
    /// The pipeline's id among the waiters for a place.
    pub(crate) id: u64,
    pub(crate) state: Mutex<State>,
    /// The demux thread waits on it (the packet queue holds it too).
    pub(crate) demux_cv: Arc<Condvar>,
    /// The video thread waits on it (both queues hold it too).
    pub(crate) video_cv: Arc<Condvar>,
    pub(crate) pool: FramePool,
    pub(crate) notify: Notify,
    pub(crate) opener: Mutex<SourceOpener>,
    pub(crate) cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    pub(crate) slots: PipelineSlots,
    /// The tracks that the owner wants (video, audio).
    pub(crate) tracks: (bool, bool),
    pub(crate) counters: Counters,
    pub(crate) progress: Arc<Progress>,
    /// The no-progress limit.
    pub(crate) stall_limit: Duration,
}

/// The settings of a pipeline that its threads need.
pub(crate) struct Settings {
    pub(crate) tracks: (bool, bool),
    pub(crate) max_packet_bytes: usize,
    pub(crate) stall_limit: Duration,
    /// The video is painted at the open.
    pub(crate) painted: bool,
}

impl Shared {
    pub(crate) fn new(
        opener: SourceOpener,
        slots: PipelineSlots,
        notify: Notify,
        settings: &Settings,
    ) -> Shared {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let demux_cv = Arc::new(Condvar::new());
        let video_cv = Arc::new(Condvar::new());
        Shared {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            state: Mutex::new(State {
                run: 0,
                active_run: 0,
                place: None,
                epoch: 0,
                seek: None,
                demux_epoch: 0,
                info: None,
                video_on: false,
                audio_on: false,
                packets: PacketQueue::new(Arc::clone(&demux_cv), Arc::clone(&video_cv)),
                max_packet_bytes: settings.max_packet_bytes.max(1),
                frames: FrameQueue::new(Arc::clone(&video_cv)),
                frame_cap: MIN_FRAMES,
                frame_size: 0,
                video_end_us: None,
                video_drained: None,
                audio_drained: None,
                ring: None,
                gain: 1.0,
                external: false,
                ring_base: None,
                audio_end_us: None,
                demux_wait: None,
                video_wait: None,
                buffered: Vec::new(),
                buffered_dirty: false,
                events: VecDeque::new(),
                wants_notify: false,
                painted: settings.painted,
                playing: false,
                media_now: 0,
                failed_at: None,
                watch: Watch {
                    needs_data: false,
                    since: Instant::now(),
                    armed: false,
                    lost: None,
                },
                frame_wait: false,
            }),
            demux_cv,
            video_cv,
            pool: FramePool::default(),
            cancel: opener.canceller(),
            opener: Mutex::new(opener),
            notify,
            slots,
            tracks: settings.tracks,
            counters: Counters::default(),
            progress: Arc::new(Progress::new()),
            stall_limit: settings.stall_limit,
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records progress of a thread.
    pub(crate) fn progressed(&self) {
        self.progress.mark();
    }

    /// The demux thread of `run` waits for `wait` until it is notified.
    pub(crate) fn wait_demux<'a>(
        &'a self,
        mut guard: MutexGuard<'a, State>,
        run: u64,
        wait: DemuxWait,
    ) -> MutexGuard<'a, State> {
        guard.demux_wait = Some((run, wait));
        let timeout = guard.external_wait(wait);
        let mut guard = self.park(guard, &self.demux_cv, &self.counters.demux_wakeups, timeout);
        if guard.demux_wait.is_some_and(|(r, _)| r == run) {
            guard.demux_wait = None;
        }
        guard
    }

    /// The video thread of `run` waits for `wait` until it is notified.
    pub(crate) fn wait_video<'a>(
        &'a self,
        mut guard: MutexGuard<'a, State>,
        run: u64,
        wait: VideoWait,
    ) -> MutexGuard<'a, State> {
        guard.video_wait = Some((run, wait));
        let mut guard = self.park(guard, &self.video_cv, &self.counters.video_wakeups, None);
        if guard.video_wait.is_some_and(|(r, _)| r == run) {
            guard.video_wait = None;
        }
        guard
    }

    /// Waits on `cv`; but when the owner waits for data and this wait
    /// stalls the pipeline on its owner, wakes the owner instead and
    /// returns (the caller checks its condition again and waits then). With
    /// a `timeout` (an external consumer of the ring), the wait ends by
    /// itself after it.
    fn park<'a>(
        &'a self,
        mut guard: MutexGuard<'a, State>,
        cv: &Condvar,
        wakeups: &AtomicU64,
        timeout: Option<Duration>,
    ) -> MutexGuard<'a, State> {
        if guard.wants_notify && guard.stalled_on_owner() {
            guard.wants_notify = false;
            drop(guard);
            (self.notify)();
            return self.lock();
        }
        let guard = match timeout {
            Some(timeout) => {
                cv.wait_timeout(guard, timeout)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => cv.wait(guard).unwrap_or_else(PoisonError::into_inner),
        };
        Counters::add(wakeups);
        guard
    }

    /// Reports a failure of `run` (if it is still the current run) and ends
    /// the run.
    pub(crate) fn fail(&self, run: u64, error: MediaError) {
        self.fail_at(run, error, None);
    }

    /// [`Shared::fail`] with the media time `at_us` where the failing thread
    /// stopped (the end of its last good frame or samples). The time depends
    /// on the data alone, not on the speed of the threads, so the owner can
    /// let the playback go on to it.
    pub(crate) fn fail_at(&self, run: u64, error: MediaError, at_us: Option<i64>) {
        let mut st = self.lock();
        if st.run != run {
            return;
        }
        log::debug!("media pipeline: {error} (at {at_us:?})");
        st.failed_at = at_us;
        st.events.push_back(ThreadEvent::Failed(error));
        st.run += 1;
        st.frame_wait = false;
        if let Some((r, place)) = &st.place
            && *r == run
        {
            place.end(self.stall_limit);
        }
        drop(st);
        self.demux_cv.notify_all();
        self.video_cv.notify_all();
        (self.notify)();
    }

    /// The watchdog's check at `now` (ADR 0047 section 11): `None` when the
    /// owner no longer needs data, or when the pipeline made no progress
    /// for the limit while it did (then the run fails with a decode error
    /// and its threads are detached); else the time of the next check.
    ///
    /// Threads that wait for room that only the owner frees (stalled on
    /// the owner) are not a stall: the owner's flag may be older than its
    /// last look. Threads that all wait, but for something that no longer
    /// holds, missed a wake-up: the check wakes them, and fails the run
    /// when the same holds a limit later with no progress meanwhile.
    pub(crate) fn check_progress(&self, now: Instant) -> Option<Instant> {
        let mut st = self.lock();
        // A run that waits for frame bytes waits as for a place: no limit
        // (M14 feature 10a). The limit counts again from the owner's next
        // need after the wait.
        if !st.watch.needs_data || st.frame_wait {
            // The disarmed watchdog is safe with a pending entry because
            // `note_needs` arms it again at the owner's next need.
            st.watch.armed = false;
            st.watch.needs_data = false;
            st.watch.lost = None;
            return None;
        }
        let run = st.run;
        let parked = st.demux_wait.is_some_and(|(r, _)| r == run)
            && (!st.video_on || st.video_wait.is_some_and(|(r, _)| r == run));
        if parked && st.stalled_on_owner() {
            st.watch.lost = None;
            self.progress.mark();
            return Some(now + self.stall_limit);
        }
        let last = if parked {
            // A lost wake-up: the threads check their conditions again.
            self.demux_cv.notify_all();
            self.video_cv.notify_all();
            let since = st
                .watch
                .lost
                .filter(|t| self.progress.last() <= *t)
                .unwrap_or(now);
            st.watch.lost = Some(since);
            since
        } else {
            st.watch.lost = None;
            self.progress.last().max(st.watch.since)
        };
        let due = last + self.stall_limit;
        if now < due {
            return Some(due);
        }
        st.watch.armed = false;
        st.watch.needs_data = false;
        st.watch.lost = None;
        let place = match st.place.take() {
            Some((r, place)) if r == run => {
                // In transit until the detach below hands it on.
                place.end_before_detach();
                Some(place)
            }
            other => {
                st.place = other;
                None
            }
        };
        let error = MediaError::Decode(format!(
            "no progress for {} ms",
            self.stall_limit.as_millis()
        ));
        log::warn!("media pipeline: {error}");
        st.events.push_back(ThreadEvent::Failed(error));
        st.run += 1;
        drop(st);
        // The threads stop at their next check; a read that blocks in the
        // source returns at the cancel; a thread that stays blocked holds
        // no place.
        if let Some(place) = place {
            place.detach();
        }
        if let Some(cancel) = &self.cancel {
            cancel();
        }
        self.demux_cv.notify_all();
        self.video_cv.notify_all();
        (self.notify)();
        None
    }

    /// Calls the owner's notify if it waits for data (after a thread
    /// produced some; `wants` is the taken flag).
    pub(crate) fn wake_owner(&self, wants: bool) {
        if wants {
            (self.notify)();
        }
    }

    /// Gives a frame's image back to the pool.
    pub(crate) fn recycle(&self, frame: QueuedFrame) {
        if let Some(image) = frame.image {
            self.pool.give(image);
        }
    }
}

impl State {
    /// Whether the ring is full (the demux thread cannot write more until
    /// the output consumes).
    pub(crate) fn ring_full(&self) -> bool {
        self.ring.as_ref().is_some_and(|r| r.free() == 0)
    }

    /// How long the demux thread waits for `wait` when an external consumer
    /// reads the ring and notifies nobody; `None` otherwise (the owner's
    /// notifies are enough). The consumer takes 48,000 frames a second, so
    /// the wait ends when the ring has fallen to about twice
    /// [`AUDIO_LOW_FRAMES`] (the video packet queue's 2 s bound lifts below
    /// [`AUDIO_LOW_FRAMES`]), between 20 and 500 ms: a refill of up to half
    /// a ring about twice a second while the ring is full, and a look every
    /// 20 ms once it is low.
    pub(crate) fn external_wait(&self, wait: DemuxWait) -> Option<Duration> {
        if !self.external
            || !matches!(
                wait,
                DemuxWait::Ring
                    | DemuxWait::Packets {
                        audio_pending: true
                    }
            )
        {
            return None;
        }
        let spare = self
            .ring
            .as_ref()
            .map_or(0, |r| r.available())
            .saturating_sub(AUDIO_LOW_FRAMES);
        let ms = (spare * 1000 / u64::from(OUTPUT_RATE)).clamp(20, 500);
        Some(Duration::from_millis(ms))
    }

    /// Whether the ring has less than [`AUDIO_LOW_FRAMES`] of an audio track
    /// that has not ended.
    fn audio_low(&self) -> bool {
        self.audio_on
            && self.audio_drained != Some(self.demux_epoch)
            && self
                .ring
                .as_ref()
                .is_some_and(|r| r.available() < AUDIO_LOW_FRAMES)
    }

    /// Whether the video packet queue is full (ADR 0047 section 6: its byte
    /// bound, its count, and 2 s while the audio is not low).
    pub(crate) fn packets_full(&self) -> bool {
        self.packets.full(self.max_packet_bytes, !self.audio_low())
    }

    /// Whether the demux thread of the current run waits, and what it waits
    /// for still holds (so only the owner can let it go on).
    pub(crate) fn demux_blocked(&self) -> bool {
        let Some((run, wait)) = self.demux_wait else {
            return false;
        };
        if run != self.run || self.seek.is_some() {
            return false;
        }
        match wait {
            DemuxWait::Idle => true,
            DemuxWait::Ring => self.ring_full(),
            DemuxWait::Packets { audio_pending } => {
                self.packets_full() && (!audio_pending || self.ring_full())
            }
        }
    }

    /// Whether the video thread of the current run waits, and what it waits
    /// for still holds (no video thread counts as waiting).
    fn video_blocked(&self) -> bool {
        if !self.video_on {
            return true;
        }
        let Some((run, wait)) = self.video_wait else {
            return false;
        };
        if run != self.run {
            return false;
        }
        match wait {
            VideoWait::Packets => self.packets.is_empty(),
            VideoWait::Frames { epoch } => {
                epoch == self.epoch && self.frames.len() >= self.frame_cap
            }
        }
    }

    /// Whether no thread can go on before the owner moves: the demux thread
    /// waits for room in the packet queue or in the ring (or has read
    /// everything), and the video thread waits for room in the frame queue
    /// or for packets. The owner then moves on with the data that is there
    /// (a track whose data lies far later in the file has a gap until it
    /// comes).
    pub(crate) fn stalled_on_owner(&self) -> bool {
        self.demux_blocked() && self.video_blocked()
    }

    /// Takes the owner's wish to be notified.
    pub(crate) fn take_wants_notify(&mut self) -> bool {
        std::mem::take(&mut self.wants_notify)
    }

    /// Adds the media time range `[start, end]` to the buffered ranges,
    /// merging ranges that touch; marks the ranges as changed when a new
    /// range starts or one grows by at least 1 s.
    pub(crate) fn add_buffered(&mut self, start: i64, end: i64) {
        let end = end.max(start);
        // The ranges are few and sorted.
        let i = self.buffered.partition_point(|r| r.1 < start);
        if let Some(r) = self.buffered.get_mut(i)
            && r.0 <= end
        {
            let old = *r;
            r.0 = r.0.min(start);
            r.1 = r.1.max(end);
            let grown = (r.1 - old.1) + (old.0 - r.0);
            // Merge with the following ranges that it reaches now.
            let reach = r.1;
            let mut j = i + 1;
            while self.buffered.get(j).is_some_and(|n| n.0 <= reach) {
                let n = self.buffered.remove(j);
                if let Some(r) = self.buffered.get_mut(i) {
                    r.1 = r.1.max(n.1);
                }
                self.buffered_dirty = true;
                j = i + 1;
            }
            if grown >= 1_000_000 {
                self.buffered_dirty = true;
            }
            return;
        }
        self.buffered.insert(i, (start, end));
        self.buffered_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(stall_limit: Duration) -> Shared {
        Shared::new(
            SourceOpener::memory(Vec::new(), true),
            PipelineSlots::new(1),
            Arc::new(|| {}),
            &Settings {
                tracks: (true, true),
                max_packet_bytes: MAX_PACKET_BYTES,
                stall_limit,
                painted: true,
            },
        )
    }

    fn state() -> State {
        shared(STALL_LIMIT)
            .state
            .into_inner()
            .expect("not poisoned")
    }

    fn packet_queue() -> PacketQueue {
        PacketQueue::new(Arc::new(Condvar::new()), Arc::new(Condvar::new()))
    }

    /// Waits until the demux thread of run 1 waits, `waits` times so far.
    fn wait_parked(shared: &Shared, waits: u64) {
        let started = Instant::now();
        loop {
            let parked = shared.lock().demux_wait.is_some();
            let woken = shared.counters.demux_wakeups.load(Ordering::Relaxed);
            if parked && woken + 1 >= waits {
                return;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "not parked");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn the_watchdog_wakes_threads_that_missed_a_wake_up() {
        let limit = Duration::from_millis(200);
        let shared = Arc::new(shared(limit));
        let ring = Arc::new(SampleRing::new(4));
        ring.push_silence(4);
        {
            let mut st = shared.lock();
            st.run = 1;
            st.audio_on = true;
            st.ring = Some(Arc::clone(&ring));
            st.watch.needs_data = true;
        }
        // A demux thread (audio alone) that waits for room in the ring.
        let (tx, rx) = std::sync::mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let mut st = thread_shared.lock();
            while st.ring_full() {
                st = thread_shared.wait_demux(st, 1, DemuxWait::Ring);
            }
            drop(st);
            tx.send(()).expect("the test waits");
        });
        wait_parked(&shared, 1);
        // Stalled on the owner: no failure, however long the owner takes.
        let later = Instant::now() + 5 * limit;
        assert_eq!(shared.check_progress(later), Some(later + limit));
        assert_eq!(
            shared.check_progress(later + limit),
            Some(later + 2 * limit)
        );
        assert!(rx.try_recv().is_err());
        // Room without a notify: a lost wake-up. The check wakes the
        // thread, which sees the room and goes on.
        ring.skip(2);
        assert_eq!(shared.check_progress(later), Some(later + limit));
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the thread wakes");
        thread.join().expect("the thread ends");
        assert!(shared.lock().events.is_empty());
    }

    #[test]
    fn a_lost_wake_up_that_stays_fails_at_the_limit() {
        let limit = Duration::from_millis(200);
        let shared = Arc::new(shared(limit));
        {
            let mut st = shared.lock();
            st.run = 1;
            st.watch.needs_data = true;
        }
        // A thread whose wait disagrees with the stall rule: it waits for
        // room in a ring that has room (there is none: never full).
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let mut st = thread_shared.lock();
            while st.run == 1 {
                st = thread_shared.wait_demux(st, 1, DemuxWait::Ring);
            }
        });
        wait_parked(&shared, 1);
        let now = Instant::now();
        assert_eq!(shared.check_progress(now), Some(now + limit));
        // Woken, it waits again without progress.
        wait_parked(&shared, 2);
        assert_eq!(shared.check_progress(now + limit / 2), Some(now + limit));
        assert_eq!(shared.check_progress(now + limit), None);
        let st = shared.lock();
        assert!(matches!(st.events.front(), Some(ThreadEvent::Failed(_))));
        assert_eq!(st.run, 2);
        drop(st);
        thread.join().expect("the run ended: the thread ends");
    }

    #[test]
    fn a_seek_ends_the_stall_on_the_owner() {
        let mut st = state();
        st.run = 1;
        st.video_on = false;
        st.demux_wait = Some((1, DemuxWait::Idle));
        assert!(st.stalled_on_owner());
        st.seek = Some(SeekCmd {
            epoch: 1,
            target_us: 0,
        });
        assert!(!st.demux_blocked());
        assert!(!st.stalled_on_owner());
    }

    #[test]
    fn every_removal_from_the_queues_notifies_the_waiting_thread() {
        let shared = Arc::new(shared(STALL_LIMIT));
        {
            let mut st = shared.lock();
            st.run = 1;
            st.video_on = true;
            st.frame_cap = 2;
            for pts_us in [0, 40_000] {
                st.frames.push_back(QueuedFrame {
                    epoch: 0,
                    pts_us,
                    duration_us: 40_000,
                    image: None,
                    at_target: false,
                });
            }
        }
        // A video thread that waits for room in the frame queue, as often
        // as the queue is full.
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let mut st = thread_shared.lock();
            let mut rounds = 0;
            while st.run == 1 {
                if st.frames.len() < st.frame_cap {
                    rounds += 1;
                    st.frames.push_back(QueuedFrame {
                        epoch: 0,
                        pts_us: 0,
                        duration_us: 40_000,
                        image: None,
                        at_target: false,
                    });
                    continue;
                }
                st = thread_shared.wait_video(st, 1, VideoWait::Frames { epoch: 0 });
            }
            rounds
        });
        let refilled = |n: usize| {
            let started = Instant::now();
            loop {
                let st = shared.lock();
                if st.frames.len() == 2 && st.video_wait.is_some() {
                    return;
                }
                drop(st);
                assert!(started.elapsed() < Duration::from_secs(10), "round {n}");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        refilled(0);
        // Each way of taking frames wakes the thread, which refills.
        let _ = shared.lock().frames.pop_front();
        refilled(1);
        let _ = shared.lock().frames.take_front(1);
        refilled(2);
        let _ = shared.lock().frames.take_all();
        refilled(3);
        shared.lock().run = 2;
        shared.video_cv.notify_all();
        assert_eq!(thread.join().expect("the thread ends"), 4);
    }

    #[test]
    fn buffered_ranges_merge() {
        let mut st = state();
        st.add_buffered(0, 40);
        st.add_buffered(40, 80);
        assert_eq!(st.buffered, [(0, 80)]);
        st.add_buffered(1_000, 1_040);
        assert_eq!(st.buffered, [(0, 80), (1_000, 1_040)]);
        st.buffered_dirty = false;
        st.add_buffered(60, 1_020);
        assert_eq!(st.buffered, [(0, 1_040)]);
        assert!(st.buffered_dirty);
        st.buffered_dirty = false;
        st.add_buffered(500, 600);
        assert!(!st.buffered_dirty);
        st.add_buffered(1_040, 2_100_000);
        assert!(st.buffered_dirty);
        assert_eq!(st.buffered, [(0, 2_100_000)]);
    }

    /// The packets of a test file of `fixtures/media/`.
    fn packets(name: &str) -> Vec<Packet> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/media")
            .join(name);
        let source = crate::FileSource::open(path).expect("the test file");
        let mut demux = crate::Demux::open(Box::new(source)).expect("opens");
        let mut out = Vec::new();
        while let Some(p) = demux.read().expect("reads") {
            out.push(p);
        }
        out
    }

    #[test]
    fn the_packet_queue_keeps_its_bounds_without_a_scan() {
        let all = packets("mp4-h264-video-only.mp4");
        assert_eq!(all.len(), 50);
        let total: usize = all.iter().map(Packet::size).sum();
        let mut q = packet_queue();
        // `End` and `Flush` do not count.
        q.push(VideoItem::Flush {
            epoch: 1,
            target_us: 0,
        });
        for p in all {
            q.push(VideoItem::Packet(p));
        }
        q.push(VideoItem::End);
        assert_eq!((q.packets, q.bytes, q.times.len()), (50, total, 50));
        // 25 fps: the first and the last packet are 1.96 s apart (B-frames
        // move the last one by up to two frames).
        assert!(!q.full(usize::MAX, true));
        assert!(q.full(total, false));
        assert!(!q.full(total + 1, false));
        // Take ten packets: the counts and the span follow.
        assert!(matches!(q.pop(), Some(VideoItem::Flush { .. })));
        for _ in 0..10 {
            assert!(matches!(q.pop(), Some(VideoItem::Packet(_))));
        }
        assert_eq!((q.packets, q.times.len()), (40, 40));
        assert!(q.bytes < total);
        q.clear();
        assert!(q.is_empty());
        assert_eq!((q.packets, q.bytes), (0, 0));
        // The count bound: 4,096 packets, whatever their bytes and times.
        while q.packets < MAX_PACKETS {
            for p in packets("mp4-h264-video-only.mp4") {
                q.push(VideoItem::Packet(p));
            }
        }
        assert!(q.full(usize::MAX, false));
        while q.packets >= MAX_PACKETS {
            q.pop();
        }
        assert!(!q.full(usize::MAX, false));
    }
}
