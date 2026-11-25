//! The sample ring of a pipeline (ADR 0047 sections 6 and 8): 48 kHz
//! stereo `f32` samples from the demux thread (the one producer) to the
//! audio output (the one consumer: the silent output of the pipeline's
//! owner now, the PipeWire mixer on its real-time thread with feature 8).
//!
//! The consumer side takes no lock and allocates nothing: the samples are
//! `AtomicU32` cells that hold the bits of an `f32`, and the positions are
//! atomic counters of frames (one frame is a left and a right sample). The
//! gain is atomic too. No `unsafe` code.
//!
//! Positions count frames since the ring was made and never wrap
//! (`u64`). The producer may discard everything that it wrote (a seek):
//! the consumer then skips to the discard position. Because the producer
//! may reuse the discarded cells before the consumer has moved past them,
//! the consumer checks the discard position again after it copied, and
//! replaces the frames that were discarded meanwhile with silence.
//!
//! **The media time of a position** ([`SampleRing::base`]): the producer
//! publishes where the frames are on the media timeline (a position, its
//! media time and the seek epoch) before it writes the first frame of that
//! base, through a sequence counter, so that the consumer reads the three
//! values consistently without a lock.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering, fence};

use crate::OUTPUT_RATE;

/// Channels per frame (interleaved left and right).
const CHANNELS: usize = 2;

/// Where the frames of a ring are on the media timeline: the frame at
/// `position` is at `media_us`, and the frames after it follow at 48 kHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingBase {
    /// A ring position (frames since the ring was made).
    pub position: u64,
    /// The media time of the frame at `position` (microseconds).
    pub media_us: i64,
    /// The seek epoch of the frames from `position` on (each seek of the
    /// pipeline starts a new one).
    pub epoch: u64,
}

impl RingBase {
    /// The media time of the frame at `position` (at or after the base's
    /// position; an earlier one gives the base's time).
    pub fn media_time(&self, position: u64) -> i64 {
        let frames = position.saturating_sub(self.position);
        let us = u128::from(frames) * 1_000_000 / u128::from(OUTPUT_RATE);
        self.media_us
            .saturating_add(i64::try_from(us).unwrap_or(i64::MAX))
    }
}

/// A single-producer single-consumer ring of stereo `f32` frames.
pub struct SampleRing {
    cells: Box<[AtomicU32]>,
    capacity: u64,
    /// Frames written since the start.
    write: AtomicU64,
    /// Frames read (or skipped) since the start.
    read: AtomicU64,
    /// Frames before this position are discarded.
    discard: AtomicU64,
    /// The gain as `f32` bits.
    gain: AtomicU32,
    /// The sequence counter of the base: odd while the producer writes it,
    /// 0 before the first base.
    base_seq: AtomicU64,
    base_position: AtomicU64,
    base_media_us: AtomicI64,
    base_epoch: AtomicU64,
}

impl SampleRing {
    /// A ring of `capacity` frames (at least 1), with the gain 1.
    pub fn new(capacity: usize) -> SampleRing {
        let capacity = capacity.max(1);
        let cells = (0..capacity * CHANNELS)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        SampleRing {
            cells,
            capacity: capacity as u64,
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            discard: AtomicU64::new(0),
            gain: AtomicU32::new(1.0f32.to_bits()),
            base_seq: AtomicU64::new(0),
            base_position: AtomicU64::new(0),
            base_media_us: AtomicI64::new(0),
            base_epoch: AtomicU64::new(0),
        }
    }

    /// Where the frames are on the media timeline (any thread; no lock):
    /// the base that the producer published last. `None` before the first
    /// one, and also when 64 reads in a row found the producer writing a
    /// base (it was preempted between its two stores); the consumer then
    /// keeps the last base it got and asks again later. A consumer that
    /// reads it after [`SampleRing::pop`] gets the base of the frames it
    /// just read or a newer one (a seek or a jump of the times publishes a
    /// new base before its first frame); frames before a base's position
    /// belong to an older base.
    pub fn base(&self) -> Option<RingBase> {
        // A sequence lock: the producer writes a base in a few
        // instructions, so a retry is rare; give up after a few.
        for _ in 0..64 {
            let seq = self.base_seq.load(Ordering::Acquire);
            if seq == 0 {
                return None;
            }
            if seq % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let base = RingBase {
                position: self.base_position.load(Ordering::Relaxed),
                media_us: self.base_media_us.load(Ordering::Relaxed),
                epoch: self.base_epoch.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.base_seq.load(Ordering::Relaxed) == seq {
                return Some(base);
            }
        }
        None
    }

    /// The capacity in frames.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// The position after the last written frame.
    pub fn write_position(&self) -> u64 {
        self.write.load(Ordering::Acquire)
    }

    /// The position of the next frame that the consumer reads (discarded
    /// frames are skipped).
    pub fn read_position(&self) -> u64 {
        self.read
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::SeqCst))
    }

    /// Frames that the consumer can read now.
    pub fn available(&self) -> u64 {
        self.write_position().saturating_sub(self.read_position())
    }

    /// The gain that the consumer applies (the element's volume; 0 when
    /// muted).
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Sets the gain (any thread).
    pub fn set_gain(&self, gain: f32) {
        let gain = if gain.is_finite() { gain } else { 0.0 };
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    fn cell(&self, frame: u64, channel: usize) -> Option<&AtomicU32> {
        let slot = usize::try_from(frame % self.capacity).ok()?;
        self.cells.get(slot * CHANNELS + channel)
    }
}

/// The producer (the pipeline's demux thread, which serializes its writes).
#[cfg_attr(
    not(feature = "ffmpeg"),
    allow(dead_code, reason = "the producer is the pipeline, which needs FFmpeg")
)]
impl SampleRing {
    /// Frames that the producer can write now.
    pub(crate) fn free(&self) -> u64 {
        let used = self
            .write
            .load(Ordering::Relaxed)
            .saturating_sub(self.read_position());
        self.capacity.saturating_sub(used)
    }

    /// Writes as many frames of `samples` (interleaved stereo) as fit;
    /// returns the frames written.
    pub(crate) fn push(&self, samples: &[f32]) -> usize {
        let frames = (samples.len() / CHANNELS).min(usize::try_from(self.free()).unwrap_or(0));
        let start = self.write.load(Ordering::Relaxed);
        for (i, pair) in samples
            .as_chunks::<CHANNELS>()
            .0
            .iter()
            .take(frames)
            .enumerate()
        {
            let pos = start + i as u64;
            for (ch, s) in pair.iter().enumerate() {
                if let Some(c) = self.cell(pos, ch) {
                    c.store(s.to_bits(), Ordering::Relaxed);
                }
            }
        }
        self.write.store(start + frames as u64, Ordering::Release);
        frames
    }

    /// Writes up to `frames` frames of silence; returns the frames written.
    pub(crate) fn push_silence(&self, frames: u64) -> u64 {
        let n = frames.min(self.free());
        let start = self.write.load(Ordering::Relaxed);
        for pos in start..start + n {
            for ch in 0..CHANNELS {
                if let Some(c) = self.cell(pos, ch) {
                    c.store(0, Ordering::Relaxed);
                }
            }
        }
        self.write.store(start + n, Ordering::Release);
        n
    }

    /// Discards every frame written so far (a seek or a flush); returns the
    /// write position, where the next frames start.
    pub(crate) fn discard_all(&self) -> u64 {
        let w = self.write.load(Ordering::Relaxed);
        self.discard.fetch_max(w, Ordering::SeqCst);
        // The cell stores of the frames after the discard come after this
        // fence: a consumer that reads one of them (and then has its acquire
        // fence, `pop`) sees the discard. x86 orders stores anyway, so the
        // tests on the development machine cannot show a missing fence; ARM
        // can reorder them.
        fence(Ordering::Release);
        w
    }

    /// Publishes the base of the frames from `base.position` on (before the
    /// first of them is written).
    pub(crate) fn set_base(&self, base: RingBase) {
        // Only the producer writes the counter, so it is even here.
        let seq = self.base_seq.load(Ordering::Relaxed);
        self.base_seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.base_position.store(base.position, Ordering::Relaxed);
        self.base_media_us.store(base.media_us, Ordering::Relaxed);
        self.base_epoch.store(base.epoch, Ordering::Relaxed);
        self.base_seq.store(seq + 2, Ordering::Release);
    }
}

/// The consumer (the audio output): no lock, no allocation.
impl SampleRing {
    /// Reads up to `out.len() / 2` frames into `out` (interleaved stereo),
    /// with the gain applied; returns the frames read. Frames that the
    /// producer discarded while they were copied come out as silence.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let start = self.read_position();
        let w = self.write.load(Ordering::Acquire);
        let n = (out.len() / CHANNELS).min(usize::try_from(w.saturating_sub(start)).unwrap_or(0));
        let gain = self.gain();
        for (i, pair) in out
            .as_chunks_mut::<CHANNELS>()
            .0
            .iter_mut()
            .take(n)
            .enumerate()
        {
            let pos = start + i as u64;
            for (ch, s) in pair.iter_mut().enumerate() {
                *s = self
                    .cell(pos, ch)
                    .map_or(0.0, |c| f32::from_bits(c.load(Ordering::Relaxed)) * gain);
            }
        }
        // Pairs with the release fence of `discard_all`: if a cell above
        // held a frame written after a discard, the load below sees that
        // discard (no test on x86 can show a missing fence).
        fence(Ordering::Acquire);
        let discarded = self.discard.load(Ordering::SeqCst);
        if discarded > start {
            let stale = usize::try_from(discarded - start)
                .unwrap_or(usize::MAX)
                .min(n);
            for s in out.iter_mut().take(stale * CHANNELS) {
                *s = 0.0;
            }
        }
        self.read.fetch_max(start + n as u64, Ordering::AcqRel);
        n
    }

    /// Skips up to `frames` frames; returns the frames skipped.
    pub fn skip(&self, frames: u64) -> u64 {
        let start = self.read_position();
        let n = frames.min(self.write_position().saturating_sub(start));
        self.read.fetch_max(start + n, Ordering::AcqRel);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn frames_come_out_in_order_with_the_gain() {
        let ring = SampleRing::new(4);
        assert_eq!(ring.push(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0]), 3);
        assert_eq!(ring.available(), 3);
        // Only one frame of room is left.
        assert_eq!(ring.push(&[4.0, -4.0, 5.0, -5.0]), 1);
        ring.set_gain(0.5);
        let mut out = [0.0; 6];
        assert_eq!(ring.pop(&mut out), 3);
        assert_eq!(out, [0.5, -0.5, 1.0, -1.0, 1.5, -1.5]);
        assert_eq!(ring.push(&[6.0, -6.0, 7.0, -7.0, 8.0, -8.0]), 3);
        let mut out = [0.0; 8];
        assert_eq!(ring.pop(&mut out), 4);
        assert_eq!(out, [2.0, -2.0, 3.0, -3.0, 3.5, -3.5, 4.0, -4.0]);
        assert_eq!(ring.available(), 0);
        ring.set_gain(f32::NAN);
        assert!(ring.gain().abs() < f32::EPSILON);
    }

    #[test]
    fn a_discard_drops_the_written_frames() {
        let ring = SampleRing::new(4);
        ring.push(&[1.0; 8]);
        assert_eq!(ring.free(), 0);
        let at = ring.discard_all();
        assert_eq!(at, 4);
        assert_eq!(ring.available(), 0);
        assert_eq!(ring.free(), 4);
        ring.push(&[2.0, 2.0]);
        assert_eq!(ring.push_silence(10), 3);
        let mut out = [9.0; 8];
        assert_eq!(ring.pop(&mut out), 4);
        assert_eq!(out, [2.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(ring.skip(5), 0);
    }

    #[test]
    fn the_base_maps_positions_to_media_times() {
        let ring = SampleRing::new(4);
        assert_eq!(ring.base(), None);
        ring.set_base(RingBase {
            position: 0,
            media_us: 1_000_000,
            epoch: 0,
        });
        ring.push(&[0.0; 8]);
        let base = ring.base().expect("a base");
        assert_eq!(base.media_time(0), 1_000_000);
        assert_eq!(base.media_time(48_000), 2_000_000);
        // A seek: a new base for the frames after the discard.
        let at = ring.discard_all();
        ring.set_base(RingBase {
            position: at,
            media_us: 500_000,
            epoch: 1,
        });
        let base = ring.base().expect("a base");
        assert_eq!((base.position, base.epoch), (4, 1));
        assert_eq!(base.media_time(4 + 480), 510_000);
    }

    #[test]
    fn one_producer_and_one_consumer_on_two_threads() {
        // The producer writes a counting sequence; the consumer checks that
        // it reads it in order, without loss.
        let ring = Arc::new(SampleRing::new(64));
        let total = 100_000u32;
        let producer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut next = 0u32;
                while next < total {
                    let v = next as f32;
                    if ring.push(&[v, -v]) == 1 {
                        next += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            })
        };
        let mut expected = 0u32;
        let mut out = [0.0f32; 32];
        while expected < total {
            let n = ring.pop(&mut out);
            for pair in out.as_chunks::<2>().0.iter().take(n) {
                assert_eq!(pair[0], expected as f32);
                assert_eq!(pair[1], -(expected as f32));
                expected += 1;
            }
            if n == 0 {
                std::thread::yield_now();
            }
        }
        producer.join().expect("the producer ends");
    }
}
