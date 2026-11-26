//! The demux thread of a pipeline (ADR 0047 section 6): opens the byte
//! source and the demuxer, reads packets, decodes and resamples the audio
//! into the sample ring, sends the video packets to the video thread, and
//! runs the seeks. It starts the video thread after the open and joins it
//! before it ends; its place among the open pipelines is given back when
//! both have ended.
//!
//! The demuxer gives the packets in file order. While the ring is full, the
//! audio packets wait in a bounded backlog and the thread reads on, so that
//! the video packets behind them reach the video thread (a file whose audio
//! lies far ahead of its video in the file); the backlog is decoded as the
//! output frees room in the ring.

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use super::shared::{
    DemuxWait, MAX_FRAME_BYTES, MAX_FRAMES, MAX_PACKETS, MIN_FRAMES, Progress, RING_FRAMES,
    SeekCmd, Shared, State, ThreadEvent, VideoItem,
};
use super::slots::{RunPlace, Slot};
use super::video;
use crate::decode::{AudioChunk, AudioDecoder, Demux, Output, Packet, VideoDecoder};
use crate::file::seek_demux;
use crate::origin::{Timeline, timeline};
use crate::ring::{RingBase, SampleRing};
use crate::{ByteSource, MediaError, OUTPUT_CHANNELS, OUTPUT_RATE};

/// A gap or overlap of the audio timestamps up to this size is filled with
/// silence or cut; a larger one starts a new timeline base (a broken file).
const AUDIO_MAX_GAP_US: i64 = 10_000_000;
/// Timestamp jitter that is not a gap (WebM times blocks in milliseconds).
const AUDIO_JITTER_US: i64 = 1_000;

/// How a step of the thread ended.
enum Flow {
    /// Go on.
    Continue,
    /// A command came (a seek) or the run ended: go back to the main loop.
    Interrupted,
}

/// How far [`Worker::pump_audio`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pump {
    /// Writes what fits in the ring now.
    Free,
    /// Also waits for room in the ring while the backlog is at its bound.
    Bounded,
    /// Waits for room until the decoder has drained (the end of the data).
    All,
}

/// The result of one write into the ring.
enum Write {
    /// Samples were written (or nothing was left).
    Progress,
    /// The ring is full.
    Full,
    Interrupted,
}

/// Samples of a decoded chunk that wait for room in the ring.
struct Rest {
    /// Silence before the samples (a gap in the times).
    silence: u64,
    samples: Vec<f32>,
    /// The next sample value to write.
    at: usize,
}

/// Spawns the demux thread of `run`, which holds `slot` (with its video
/// thread) until it ends. On failure the slot is given back and the run
/// fails.
pub(crate) fn spawn(shared: &Arc<Shared>, run: u64, slot: Slot) {
    let place = RunPlace::new(slot);
    {
        let mut st = shared.lock();
        if st.run == run {
            st.place = Some((run, Arc::clone(&place)));
        } else {
            // The run ended before its thread started.
            place.end(shared.stall_limit);
        }
    }
    let thread_shared = Arc::clone(shared);
    let guard = place.enter();
    let started = thread::Builder::new()
        .name("swb-media-demux".to_owned())
        .spawn(move || {
            let panic = PanicGuard {
                shared: Arc::clone(&thread_shared),
                run,
            };
            main(&thread_shared, run, &place);
            drop(panic);
            // The thread ends before its place goes to the next pipeline.
            drop(guard);
        });
    if let Err(e) = started {
        shared.fail(
            run,
            MediaError::Decode(format!("cannot start the demux thread: {e}")),
        );
    }
}

/// Reports a panic of a pipeline thread as an error of its run.
pub(crate) struct PanicGuard {
    pub(crate) shared: Arc<Shared>,
    pub(crate) run: u64,
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        if thread::panicking() {
            self.shared.fail(
                self.run,
                MediaError::Decode("internal error in the media pipeline".to_owned()),
            );
        }
    }
}

fn main(shared: &Arc<Shared>, run: u64, place: &Arc<RunPlace>) {
    if shared.lock().run != run {
        return;
    }
    let mut worker = match Worker::open(shared, run, place) {
        Ok(w) => w,
        Err(e) => {
            shared.fail(run, e);
            return;
        }
    };
    if let Err(e) = worker.run() {
        let at = worker.failure_position();
        shared.fail_at(run, e, at);
    }
    // Dropping the worker ends and joins the video thread.
}

/// The demux thread's state.
struct Worker {
    shared: Arc<Shared>,
    run: u64,
    demux: Demux,
    audio: Option<AudioDecoder>,
    video_stream: Option<usize>,
    audio_stream: Option<usize>,
    video_thread: Option<JoinHandle<()>>,
    /// The sample ring of this run (with an audio track).
    ring: Option<Arc<SampleRing>>,
    timeline: Timeline,
    seekable: bool,
    /// Packets were read since the demuxer opened (a stream that cannot
    /// seek then reopens for a seek).
    read_any: bool,
    /// The end of the container was read.
    eof: bool,
    /// The current buffered range: its start and end (media time).
    range: Option<(i64, i64)>,
    /// Audio packets read while the ring was full, and their bytes.
    backlog: VecDeque<Packet>,
    backlog_bytes: usize,
    /// The bound of `backlog_bytes`.
    max_backlog_bytes: usize,
    /// Decoded samples that wait for room in the ring.
    rest: Option<Rest>,
    /// The audio decoder got the end of the stream.
    audio_finishing: bool,
    /// The audio decoder drained (until the next seek).
    audio_done: bool,
}

/// Is `run` still the current run (and no seek waiting)?
fn interrupted(st: &State, run: u64) -> bool {
    st.run != run || st.seek.is_some()
}

impl Worker {
    /// Opens the source, the demuxer and the decoders; reports the
    /// metadata at the first open; starts the video thread.
    fn open(shared: &Arc<Shared>, run: u64, place: &Arc<RunPlace>) -> Result<Worker, MediaError> {
        let (demux, video_on, audio_on) = open_demux(shared, run)?;
        let info = demux.info().clone();
        let tl = timeline(&info);
        let mut demux = demux;
        let video_decoder = if video_on {
            demux.video_decoder()?
        } else {
            None
        };
        let audio = if audio_on {
            demux.audio_decoder()?
        } else {
            None
        };
        let video_stream = video_decoder.as_ref().map(VideoDecoder::stream);
        let audio_stream = audio.as_ref().map(AudioDecoder::stream);
        let ring = audio_stream.map(|_| Arc::new(SampleRing::new(RING_FRAMES)));
        let (epoch, max_packet_bytes) = {
            let mut st = shared.lock();
            if st.run != run {
                return Err(MediaError::Source("the pipeline closed".to_owned()));
            }
            st.video_on = video_stream.is_some();
            st.audio_on = audio_stream.is_some();
            if let Some(r) = &ring {
                r.set_gain(st.gain);
            }
            st.ring.clone_from(&ring);
            st.frame_cap = info.video.as_ref().map_or(MIN_FRAMES, |v| {
                let (w, h) = v.coded_size;
                let bytes = u64::from(w.max(1)) * u64::from(h.max(1)) * 4;
                usize::try_from(MAX_FRAME_BYTES / bytes)
                    .unwrap_or(MAX_FRAMES)
                    .clamp(MIN_FRAMES, MAX_FRAMES)
            });
            // The data from here on belongs to the current epoch; a seek
            // that is waiting starts the next one.
            let epoch = st.epoch;
            st.demux_epoch = epoch;
            st.ring_base = None;
            st.audio_end_us = None;
            st.packets.clear();
            let max_packet_bytes = st.max_packet_bytes;
            if st.info.is_none() {
                st.info = Some((info.clone(), tl));
                st.events
                    .push_back(ThreadEvent::Opened(Box::new((info.clone(), tl))));
                drop(st);
                (shared.notify)();
            }
            (epoch, max_packet_bytes)
        };
        shared.progressed();
        let video_thread = match video_decoder {
            Some(decoder) => Some(video::spawn(
                shared,
                run,
                place.enter(),
                decoder,
                epoch,
                tl.offset_us,
                info.video.as_ref().and_then(|v| v.frame_rate),
            )?),
            None => None,
        };
        Ok(Worker {
            shared: Arc::clone(shared),
            run,
            demux,
            audio,
            video_stream,
            audio_stream,
            video_thread,
            ring,
            timeline: tl,
            seekable: info.seekable,
            read_any: false,
            eof: false,
            range: None,
            backlog: VecDeque::new(),
            backlog_bytes: 0,
            max_backlog_bytes: (max_packet_bytes / 4).max(1),
            rest: None,
            audio_finishing: false,
            audio_done: false,
        })
    }

    /// Where a failure of this thread lies in media time: the end of the
    /// audio that it decoded (the demux thread decodes the audio in order,
    /// so the time depends on the data, not on the speed of the threads);
    /// `None` without audio.
    fn failure_position(&self) -> Option<i64> {
        let st = self.shared.lock();
        self.audio_stream?;
        if st.demux_epoch == st.epoch {
            st.audio_end_us
        } else {
            None
        }
    }

    /// The main loop: commands, then packets.
    fn run(&mut self) -> Result<(), MediaError> {
        loop {
            let cmd = {
                let mut st = self.shared.lock();
                if st.run != self.run {
                    return Ok(());
                }
                if st.seek.is_none() && self.eof {
                    // Everything is read: wait for a seek or the end of the
                    // run.
                    while st.run == self.run && st.seek.is_none() {
                        st = self.shared.wait_demux(st, self.run, DemuxWait::Idle);
                    }
                    continue;
                }
                st.seek.take()
            };
            if let Some(cmd) = cmd {
                self.seek(cmd)?;
                continue;
            }
            self.step()?;
        }
    }

    /// Writes the audio that fits in the ring, then reads and handles one
    /// packet.
    fn step(&mut self) -> Result<(), MediaError> {
        if let Flow::Interrupted = self.pump_audio(Pump::Free)? {
            return Ok(());
        }
        let packet = match self.demux.read() {
            Ok(p) => p,
            Err(e) => {
                if self.shared.lock().run != self.run {
                    // Cancelled: the run ended.
                    return Ok(());
                }
                return Err(e);
            }
        };
        let Some(packet) = packet else {
            return self.end_of_stream();
        };
        self.shared.progressed();
        self.read_any = true;
        let stream = packet.stream();
        if Some(stream) == self.video_stream {
            self.add_buffered(&packet);
            self.push_video(VideoItem::Packet(packet))?;
        } else if Some(stream) == self.audio_stream {
            self.add_buffered(&packet);
            self.backlog_bytes = self.backlog_bytes.saturating_add(packet.size());
            self.backlog.push_back(packet);
            self.pump_audio(Pump::Bounded)?;
        }
        Ok(())
    }

    fn add_buffered(&mut self, packet: &Packet) {
        let Some(pts) = packet.pts_us() else {
            return;
        };
        // Media times start at 0 (an AAC priming packet has a negative
        // time).
        let start = pts.saturating_sub(self.timeline.offset_us).max(0);
        let end = start.saturating_add(packet.duration_us().unwrap_or(0).max(0));
        let (s, e) = match self.range {
            Some((s, e)) => (s.min(start), e.max(end)),
            None => (start, end),
        };
        self.range = Some((s, e));
        let mut st = self.shared.lock();
        if st.run == self.run {
            st.add_buffered(s, e);
        }
    }

    /// Whether audio waits for room in the ring (decoded or compressed).
    fn audio_pending(&self) -> bool {
        self.rest.is_some() || !self.backlog.is_empty()
    }

    /// Whether the backlog of audio packets is at its bound.
    fn backlog_full(&self) -> bool {
        self.backlog.len() >= MAX_PACKETS || self.backlog_bytes >= self.max_backlog_bytes
    }

    /// Waits until the video packet queue has room, then queues `item`
    /// (an `End` or `Flush` always goes in). While it waits, audio that
    /// waits for room in the ring is written when the ring has room.
    fn push_video(&mut self, item: VideoItem) -> Result<Flow, MediaError> {
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        if let VideoItem::Packet(_) = &item {
            loop {
                if interrupted(&st, self.run) {
                    return Ok(Flow::Interrupted);
                }
                if !st.packets_full() {
                    break;
                }
                let audio_pending = self.audio_pending();
                if audio_pending && !st.ring_full() {
                    drop(st);
                    if let Flow::Interrupted = self.pump_audio(Pump::Free)? {
                        return Ok(Flow::Interrupted);
                    }
                    st = shared.lock();
                    continue;
                }
                st = shared.wait_demux(st, self.run, DemuxWait::Packets { audio_pending });
            }
        } else if st.run != self.run {
            return Ok(Flow::Interrupted);
        }
        // Notifies the video thread (`PacketQueue`).
        st.packets.push(item);
        Ok(Flow::Continue)
    }

    /// Decodes the backlog and writes the samples into the ring, as far as
    /// `mode` says.
    fn pump_audio(&mut self, mode: Pump) -> Result<Flow, MediaError> {
        loop {
            if self.rest.is_some() {
                let block = match mode {
                    Pump::Free => false,
                    Pump::Bounded => self.backlog_full(),
                    Pump::All => true,
                };
                match self.write_rest(block) {
                    Write::Progress => continue,
                    Write::Full => return Ok(Flow::Continue),
                    Write::Interrupted => return Ok(Flow::Interrupted),
                }
            }
            if self.audio_done {
                return Ok(Flow::Continue);
            }
            let Some(audio) = &mut self.audio else {
                return Ok(Flow::Continue);
            };
            match audio.receive()? {
                Output::Ready(chunk) => {
                    self.shared.progressed();
                    if let Flow::Interrupted = self.place_chunk(chunk) {
                        return Ok(Flow::Interrupted);
                    }
                }
                Output::NeedsInput => {
                    if let Some(packet) = self.backlog.pop_front() {
                        self.backlog_bytes = self.backlog_bytes.saturating_sub(packet.size());
                        if !audio.send(&packet)? {
                            return Err(MediaError::Decode(
                                "the audio decoder neither takes packets nor gives samples"
                                    .to_owned(),
                            ));
                        }
                    } else if mode == Pump::All && self.eof && !self.audio_finishing {
                        audio.finish()?;
                        self.audio_finishing = true;
                    } else {
                        return Ok(Flow::Continue);
                    }
                }
                Output::Ended => {
                    self.audio_done = true;
                    let mut st = self.shared.lock();
                    if st.run == self.run {
                        st.audio_drained = Some(st.demux_epoch);
                    }
                    let wants = st.take_wants_notify();
                    drop(st);
                    self.shared.wake_owner(wants);
                    return Ok(Flow::Continue);
                }
            }
        }
    }

    /// Places a decoded chunk at its time (a small gap becomes silence, an
    /// overlap is cut, a large jump starts a new base) as the samples that
    /// wait for the ring.
    fn place_chunk(&mut self, chunk: AudioChunk) -> Flow {
        let Some(ring) = self.ring.clone() else {
            return Flow::Continue;
        };
        let channels = OUTPUT_CHANNELS as usize;
        let pts = chunk.pts_us.saturating_sub(self.timeline.offset_us);
        let mut at = 0;
        let mut silence = 0u64;
        let mut st = self.shared.lock();
        if interrupted(&st, self.run) {
            return Flow::Interrupted;
        }
        let new_base = match st.ring_base {
            None => true,
            Some((pos, base)) => {
                let written = ring.write_position().saturating_sub(pos);
                let expected = base.saturating_add(frames_to_us(written));
                let diff = pts.saturating_sub(expected);
                if diff.abs() > AUDIO_MAX_GAP_US {
                    log::debug!("media pipeline: the audio times jump by {diff} us");
                    true
                } else {
                    if diff > AUDIO_JITTER_US {
                        silence = us_to_frames(diff);
                    } else if diff < -AUDIO_JITTER_US {
                        at = usize::try_from(us_to_frames(-diff))
                            .unwrap_or(usize::MAX)
                            .saturating_mul(channels)
                            .min(chunk.samples.len());
                    }
                    false
                }
            }
        };
        if new_base {
            let position = ring.write_position();
            st.ring_base = Some((position, pts));
            // Published before the first frame of the base is written.
            ring.set_base(RingBase {
                position,
                media_us: pts,
                epoch: st.demux_epoch,
            });
        }
        drop(st);
        self.rest = Some(Rest {
            silence,
            samples: chunk.samples,
            at,
        });
        Flow::Continue
    }

    /// Writes the waiting samples into the ring as far as they fit; with
    /// `block`, waits for room first.
    fn write_rest(&mut self, block: bool) -> Write {
        let Some(ring) = self.ring.clone() else {
            self.rest = None;
            return Write::Progress;
        };
        let channels = OUTPUT_CHANNELS as usize;
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        loop {
            if interrupted(&st, self.run) {
                return Write::Interrupted;
            }
            if ring.free() > 0 {
                break;
            }
            if !block {
                return Write::Full;
            }
            st = shared.wait_demux(st, self.run, DemuxWait::Ring);
        }
        let Some(rest) = &mut self.rest else {
            return Write::Progress;
        };
        if rest.silence > 0 {
            rest.silence -= ring.push_silence(rest.silence);
        } else {
            let n = ring.push(rest.samples.get(rest.at..).unwrap_or_default());
            rest.at = rest.at.saturating_add(n * channels);
        }
        if rest.silence == 0 && rest.at >= rest.samples.len() {
            self.rest = None;
        }
        if let Some((pos, base)) = st.ring_base {
            let written = ring.write_position().saturating_sub(pos);
            st.audio_end_us = Some(base.saturating_add(frames_to_us(written)));
        }
        let wants = st.take_wants_notify();
        drop(st);
        shared.progressed();
        shared.wake_owner(wants);
        Write::Progress
    }

    /// The end of the container: the video decoder drains; the audio waits
    /// for room until its decoder has drained.
    fn end_of_stream(&mut self) -> Result<(), MediaError> {
        self.eof = true;
        {
            let mut st = self.shared.lock();
            if st.run == self.run {
                st.buffered_dirty = true;
            }
        }
        if self.video_stream.is_some()
            && let Flow::Interrupted = self.push_video(VideoItem::End)?
        {
            return Ok(());
        }
        self.pump_audio(Pump::All)?;
        Ok(())
    }

    /// Runs a seek: the demuxer goes to the key frame at or before the
    /// target, the queues and the ring are flushed, the decoders flush and
    /// drop what comes before the target. A source that cannot seek opens
    /// again and plays from the start.
    fn seek(&mut self, cmd: SeekCmd) -> Result<(), MediaError> {
        let mut target = cmd.target_us;
        if self.seekable {
            let stream_target = target.saturating_add(self.timeline.offset_us);
            match seek_demux(
                &mut self.demux,
                self.video_stream,
                self.audio_stream,
                stream_target,
            ) {
                Ok(()) => {}
                Err(e @ MediaError::Source(_)) => {
                    if self.shared.lock().run != self.run {
                        return Ok(());
                    }
                    return Err(e);
                }
                // A demuxer that cannot seek decodes on from where it is.
                Err(e) => log::debug!("media pipeline: {e}"),
            }
        } else {
            // A stream: from the start (ADR 0047 section 7; Chromium).
            target = self.timeline.start_us;
            if self.read_any {
                self.reopen()?;
            }
        }
        let stream_target = target.saturating_add(self.timeline.offset_us);
        {
            let mut st = self.shared.lock();
            if st.run != self.run {
                return Ok(());
            }
            st.demux_epoch = cmd.epoch;
            st.ring_base = None;
            st.audio_end_us = None;
            if let Some(ring) = &self.ring {
                ring.discard_all();
            }
            st.packets.clear();
            if self.video_stream.is_some() {
                // Notifies the video thread.
                st.packets.push(VideoItem::Flush {
                    epoch: cmd.epoch,
                    target_us: stream_target,
                });
            }
        }
        self.backlog.clear();
        self.backlog_bytes = 0;
        self.rest = None;
        self.audio_finishing = false;
        self.audio_done = false;
        if let Some(audio) = &mut self.audio {
            audio.flush();
            audio.start_at(stream_target);
        }
        self.eof = false;
        self.range = None;
        Ok(())
    }

    /// Opens the source and the demuxer again (a stream that cannot seek).
    /// The decoders stay: the streams are the same.
    fn reopen(&mut self) -> Result<(), MediaError> {
        let (demux, _, _) = open_demux(&self.shared, self.run)?;
        self.demux = demux;
        let video = self.video_stream.is_some();
        let audio = self.audio_stream.is_some();
        self.demux.select(video, audio)?;
        self.read_any = false;
        Ok(())
    }

    /// Ends the video thread and waits for it.
    fn stop_video(&mut self) {
        {
            let mut st = self.shared.lock();
            if st.run == self.run {
                st.run += 1;
            }
        }
        self.shared.video_cv.notify_all();
        if let Some(t) = self.video_thread.take()
            && t.join().is_err()
        {
            log::debug!("media pipeline: the video thread panicked");
        }
    }
}

impl Drop for Worker {
    /// The video thread ends with the demux thread, also when the demux
    /// thread unwinds from a panic.
    fn drop(&mut self) {
        self.stop_video();
    }
}

/// A byte source that records each read as progress of the pipeline (a
/// slow source that delivers is not a stalled one).
struct Counted {
    inner: Box<dyn ByteSource>,
    progress: Arc<Progress>,
}

impl ByteSource for Counted {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.progress.mark();
        }
        Ok(n)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.inner.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.inner.size()
    }
}

/// Opens the source and the demuxer of `run`, and selects the tracks that
/// play.
fn open_demux(shared: &Shared, run: u64) -> Result<(Demux, bool, bool), MediaError> {
    let source = shared
        .opener
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .open()
        .map_err(|e| MediaError::Source(format!("cannot open the source: {e}")))?;
    if shared.lock().run != run {
        // The run ended while the source opened: its cancel may have come
        // before the open, which undoes a cancel (a net source resumes).
        // Cancel again, so that nothing reads for a run that ended.
        if let Some(cancel) = &shared.cancel {
            cancel();
        }
        return Err(MediaError::Source("the pipeline closed".to_owned()));
    }
    let source = Box::new(Counted {
        inner: source,
        progress: Arc::clone(&shared.progress),
    });
    let mut demux = Demux::open(source)?;
    let (want_video, want_audio) = shared.tracks;
    let has_video = demux.info().video.is_some();
    let has_audio = demux.info().audio.is_some();
    // A track that is not wanted plays anyway when it is the only one (an
    // `<audio>` element with a video file still has a timeline).
    let audio_on = has_audio && (want_audio || !has_video);
    let video_on = has_video && (want_video || !audio_on);
    demux.select(video_on, audio_on)?;
    Ok((demux, video_on, audio_on))
}

/// Microseconds of `frames` at the output rate.
pub(crate) fn frames_to_us(frames: u64) -> i64 {
    i64::try_from(u128::from(frames) * 1_000_000 / u128::from(OUTPUT_RATE)).unwrap_or(i64::MAX)
}

/// Output frames in `us` microseconds, rounded (0 for a negative time).
pub(crate) fn us_to_frames(us: i64) -> u64 {
    let us = u128::try_from(us).unwrap_or(0);
    u64::try_from((us * u128::from(OUTPUT_RATE) + 500_000) / 1_000_000).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::super::opener::SourceOpener;
    use super::super::shared::{MAX_PACKET_BYTES, STALL_LIMIT, Settings};
    use super::super::slots::PipelineSlots;
    use super::*;

    /// A run that ended before its thread started (the owner suspended
    /// between the place and the spawn) puts its place in transit, so that
    /// the next waiter's owner counts the handover as pending work.
    #[test]
    fn a_run_that_ended_before_its_thread_started_is_in_transit() {
        let slots = PipelineSlots::new(1);
        slots.set_handover_delay(Duration::from_millis(300));
        let shared = Arc::new(Shared::new(
            SourceOpener::memory(Vec::new(), true),
            slots.clone(),
            Arc::new(|| {}),
            &Settings {
                tracks: (true, true),
                max_packet_bytes: MAX_PACKET_BYTES,
                stall_limit: STALL_LIMIT,
                painted: true,
            },
        ));
        let held: Arc<Mutex<Option<Slot>>> = Arc::default();
        let h = Arc::clone(&held);
        slots.acquire(
            1,
            Arc::new(|| {}),
            Box::new(move |s| {
                *h.lock().expect("lock") = Some(s);
                None
            }),
        );
        let slot = held.lock().expect("lock").take().expect("a place");
        // Run 1 is over: the current run is 2.
        shared.lock().run = 2;
        spawn(&shared, 1, slot);
        // The thread ends at once and waits before it hands the place on.
        assert_eq!(slots.in_transit(), 1);
        let started = Instant::now();
        while slots.in_transit() > 0 {
            assert!(started.elapsed() < Duration::from_secs(10), "stuck");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(slots.open_count(), 0);
    }
}
