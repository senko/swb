//! The video thread of a pipeline (ADR 0047 sections 6 and 9): decodes the
//! video packets that the demux thread queues, converts the frames to RGBA
//! into pooled pixmaps (not when the video is not painted), and queues them
//! for the owner. After a seek it keeps only the frame at the target and
//! the frames after it. A frame that is already late when it is decoded
//! (its successor is due) is dropped without its conversion.

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use super::demux::PanicGuard;
use super::shared::{Counters, QueuedFrame, Shared, VideoItem, VideoWait};
use super::slots::PlaceGuard;
use crate::MediaError;
use crate::decode::{Output, Packet, RawVideoFrame, VideoDecoder};
use crate::file::SeekFilter;

/// The duration of a frame without one and without a frame rate.
const DEFAULT_FRAME_US: i64 = 40_000;

/// Spawns the video thread of `run`, which holds `place` (a thread of the
/// run's place) until it ends.
pub(crate) fn spawn(
    shared: &Arc<Shared>,
    run: u64,
    place: PlaceGuard,
    decoder: VideoDecoder,
    epoch: u64,
    offset_us: i64,
    frame_rate: Option<(u32, u32)>,
) -> Result<JoinHandle<()>, MediaError> {
    let frame_us = frame_rate
        .filter(|(n, d)| *n > 0 && *d > 0)
        .map_or(DEFAULT_FRAME_US, |(n, d)| {
            (i64::from(d) * 1_000_000 / i64::from(n)).max(1)
        });
    let mut worker = Worker {
        shared: Arc::clone(shared),
        run,
        decoder,
        epoch,
        offset_us,
        frame_us,
        filter: SeekFilter::default(),
        awaiting_target: false,
        last_end: None,
        target_us: 0,
        pending: None,
        drained: false,
    };
    thread::Builder::new()
        .name("swb-media-video".to_owned())
        .spawn(move || {
            let panic = PanicGuard {
                shared: Arc::clone(&worker.shared),
                run,
            };
            if let Err(e) = worker.run() {
                let at = worker.failure_position();
                worker.shared.fail_at(run, e, Some(at));
            }
            drop(panic);
            drop(place);
        })
        .map_err(|e| MediaError::Decode(format!("cannot start the video thread: {e}")))
}

/// What the thread does next.
enum Next {
    Item(VideoItem),
    Stop,
}

struct Worker {
    shared: Arc<Shared>,
    run: u64,
    decoder: VideoDecoder,
    /// The epoch of the packets that the decoder gets now.
    epoch: u64,
    offset_us: i64,
    /// The duration of a frame without one.
    frame_us: i64,
    filter: SeekFilter,
    /// The next frame that comes out is the frame of a seek.
    awaiting_target: bool,
    /// The end of the last frame (media time), for frames without a time.
    last_end: Option<i64>,
    /// The target of the last seek (0 before one), where a failure lies
    /// when no frame came after it.
    target_us: i64,
    /// A packet that the decoder refused because it was full.
    pending: Option<Packet>,
    /// The decoder drained (until the next flush).
    drained: bool,
}

impl Worker {
    fn run(&mut self) -> Result<(), MediaError> {
        loop {
            if self.drained {
                match self.next_item() {
                    Next::Stop => return Ok(()),
                    Next::Item(VideoItem::Flush { epoch, target_us }) => {
                        self.flush(epoch, target_us);
                    }
                    // Nothing comes after the end but a flush.
                    Next::Item(_) => {}
                }
                continue;
            }
            match self.decoder.receive()? {
                Output::Ready(raw) => {
                    let (held, frame) = self.filter.offer(raw);
                    for f in [held, frame].into_iter().flatten() {
                        if !self.push(&f)? {
                            return Ok(());
                        }
                    }
                }
                Output::Ended => {
                    if let Some(held) = self.filter.finish()
                        && !self.push(&held)?
                    {
                        return Ok(());
                    }
                    self.drained = true;
                    let mut st = self.shared.lock();
                    if st.run == self.run && st.epoch == self.epoch {
                        st.video_drained = Some(self.epoch);
                    }
                    let wants = st.take_wants_notify();
                    drop(st);
                    self.shared.wake_owner(wants);
                }
                Output::NeedsInput => {
                    if let Some(p) = self.pending.take() {
                        self.send(p)?;
                        continue;
                    }
                    match self.next_item() {
                        Next::Stop => return Ok(()),
                        Next::Item(VideoItem::Packet(p)) => self.send(p)?,
                        Next::Item(VideoItem::End) => self.decoder.finish()?,
                        Next::Item(VideoItem::Flush { epoch, target_us }) => {
                            self.flush(epoch, target_us);
                        }
                    }
                }
            }
        }
    }

    fn send(&mut self, packet: Packet) -> Result<(), MediaError> {
        if !self.decoder.send(&packet)? {
            // Full: take frames first, then send it again.
            self.pending = Some(packet);
        }
        Ok(())
    }

    fn flush(&mut self, epoch: u64, target_us: i64) {
        self.decoder.flush();
        self.epoch = epoch;
        self.filter.start(target_us);
        self.awaiting_target = true;
        self.pending = None;
        self.drained = false;
        self.last_end = None;
        self.target_us = target_us;
    }

    /// Where a failure of the decoder lies in media time: the end of the
    /// last frame that came out (the decoder gives frames in presentation
    /// order), or the target of the last seek. It depends on the data, not
    /// on the speed of the threads.
    fn failure_position(&self) -> i64 {
        self.last_end.unwrap_or(self.target_us)
    }

    /// The next item of the packet queue; blocks until there is one.
    fn next_item(&mut self) -> Next {
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        loop {
            if st.run != self.run {
                return Next::Stop;
            }
            // Taking an item notifies the demux thread (`PacketQueue`).
            if let Some(item) = st.packets.pop() {
                return Next::Item(item);
            }
            st = shared.wait_video(st, self.run, VideoWait::Packets);
        }
    }

    /// Converts and queues a decoded frame; `false` when the run ended.
    fn push(&mut self, raw: &RawVideoFrame) -> Result<bool, MediaError> {
        let shared = Arc::clone(&self.shared);
        Counters::add(&shared.counters.frames_decoded);
        shared.progressed();
        let pts = raw
            .pts_us()
            .map(|p| p.saturating_sub(self.offset_us))
            .or(self.last_end)
            .unwrap_or(0);
        let duration = raw
            .duration_us()
            .filter(|d| *d > 0)
            .unwrap_or(self.frame_us);
        self.last_end = Some(pts.saturating_add(duration));
        let at_target = std::mem::take(&mut self.awaiting_target);
        let painted = {
            let mut st = shared.lock();
            loop {
                if st.run != self.run {
                    return Ok(false);
                }
                if st.epoch != self.epoch {
                    // A newer seek: this frame is of no use.
                    return Ok(true);
                }
                if st.frames.len() < st.frame_cap {
                    break;
                }
                st = shared.wait_video(st, self.run, VideoWait::Frames { epoch: self.epoch });
            }
            if !at_target && st.playing && pts.saturating_add(duration) <= st.media_now {
                // Late: its successor is due already.
                Counters::add(&shared.counters.late_frames);
                return Ok(true);
            }
            st.painted
        };
        let image = if painted {
            let (w, h) = raw.size();
            let mut image = shared
                .pool
                .take(w, h)
                .ok_or_else(|| MediaError::Decode(format!("no image of {w} x {h} pixels")))?;
            match Arc::get_mut(&mut image) {
                Some(pixmap) => self.decoder.convert(raw, pixmap)?,
                None => {
                    return Err(MediaError::Decode(
                        "a pooled image is still in use".to_owned(),
                    ));
                }
            }
            Counters::add(&shared.counters.frames_converted);
            Some(image)
        } else {
            None
        };
        let frame = QueuedFrame {
            epoch: self.epoch,
            pts_us: pts,
            duration_us: duration,
            image,
            at_target,
        };
        let mut st = shared.lock();
        if st.run != self.run {
            drop(st);
            shared.recycle(frame);
            return Ok(false);
        }
        if st.epoch != self.epoch {
            drop(st);
            shared.recycle(frame);
            return Ok(true);
        }
        st.video_end_us = Some(pts.saturating_add(duration));
        st.frames.push_back(frame);
        let wants = st.take_wants_notify();
        drop(st);
        shared.wake_owner(wants);
        Ok(true)
    }
}
