//! The audio clock of one source in the mixer (ADR 0047 section 7).
//!
//! **What the clock is.** The frames that the mixer took for the element
//! (`taken`: frames of its ring, and the frames of silence that align it with
//! the element's clock; the frames of an empty ring are not counted), minus
//! the device's delay, over 48,000. Its origin is the moment the source
//! joined: the owner anchors the element's `MediaClock` on it
//! ([`crate::MediaClock::rebase`]), so the media time is the media time of
//! the sound that comes out of the device now.
//!
//! **Interpolation.** The mixer's callback comes every 512 frames (10.7 ms),
//! and the snapshot of `taken` and the delay is from the last callback. The
//! clock adds the time since that callback, read from the output's
//! [`super::MonoClock`], up to the larger of the delay and the cycle (what
//! the device can have played of the frames it got); and never more than
//! `taken`. While the source is starved (its ring was empty in the last
//! cycle) the clock does not add anything: it stops with the frames.
//!
//! **Never back.** The value that [`Source::clock_us`] returns is the
//! largest so far: a delay that grows, or a snapshot that has fewer frames
//! than the interpolation had assumed, makes the clock stand still, never
//! go back.
//!
//! **No data yet.** From the join to the first callback, and while the
//! snapshot cannot be read, the clock stands at its last value (0 at the
//! join).

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use super::control::Output;
use super::mix::Reading;
use crate::OUTPUT_RATE;

/// The audio clock of a reading at the clock time `now_ns`, in microseconds
/// (the frames taken, less the delay, with the interpolation: see the module
/// comment).
fn audible_us(reading: &Reading, now_ns: u64) -> i64 {
    let taken = i64::try_from(reading.taken).unwrap_or(i64::MAX);
    // The frames of a cycle are handed over at its callback and play during
    // the cycle after it at the earliest: whatever delay the device reports,
    // at least a cycle of frames is not audible yet (a null sink reports 0).
    let delay = reading.delay_frames.max(i64::from(reading.cycle_frames));
    let mut ahead = 0;
    if !reading.starved && reading.cycles > 0 {
        let behind_ns = now_ns.saturating_sub(reading.snap_ns);
        let frames = u128::from(behind_ns) * u128::from(OUTPUT_RATE) / 1_000_000_000;
        ahead = i64::try_from(frames).unwrap_or(i64::MAX).min(delay);
    }
    // Never more than the frames that the mixer took.
    let audible = taken.saturating_sub(delay).saturating_add(ahead).min(taken);
    // Truncating, signed (not `ring::frames_to_us`: that one is unsigned).
    audible.saturating_mul(1_000_000) / i64::from(OUTPUT_RATE)
}

/// One element's place in the mixer. Dropping it fades the source out and
/// frees its slot (a later tick of the output frees it on a thread of the
/// owner's side, never on the real-time thread).
pub struct Source {
    output: Arc<Output>,
    index: usize,
    epoch: u64,
    session: u64,
    /// The largest clock value returned so far (microseconds).
    last_us: AtomicI64,
}

impl Source {
    pub(super) fn new(output: Arc<Output>, index: usize, epoch: u64, session: u64) -> Source {
        Source {
            output,
            index,
            epoch,
            session,
            last_us: AtomicI64::new(0),
        }
    }

    /// The audio clock in microseconds (see the module comment). After the
    /// device failed it goes on in real time from where it was at the
    /// failure (the output kept the last reading), so that the element that
    /// continues on the page clock at its next step has lost no time.
    pub fn clock_us(&self) -> i64 {
        let inner = self.output.inner();
        let now_ns = inner.clock.now_ns();
        let us = match inner.mix.reading(self.index, self.epoch) {
            Some(reading) => audible_us(&reading, now_ns),
            None => match self.us_after_failure(now_ns) {
                Some(us) => us,
                None => return self.last_us.load(Ordering::Relaxed),
            },
        };
        self.last_us.fetch_max(us, Ordering::Relaxed).max(us)
    }

    /// The clock of a source whose device failed: its last reading
    /// interpolated to the failure, then the time since.
    fn us_after_failure(&self, now_ns: u64) -> Option<i64> {
        let inner = self.output.inner();
        let finals = inner
            .finals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_, _, reading) = finals
            .readings
            .iter()
            .find(|(index, epoch, _)| *index == self.index && *epoch == self.epoch)?;
        let at_failure = audible_us(reading, finals.at_ns);
        let since_us = now_ns.saturating_sub(finals.at_ns) / 1000;
        Some(at_failure.saturating_add(i64::try_from(since_us).unwrap_or(i64::MAX)))
    }

    /// Whether the source is still in the mixer: false after the device
    /// failed (its slots are gone), when the element continues on another
    /// clock.
    pub fn is_live(&self) -> bool {
        self.output.inner().session.load(Ordering::Acquire) == self.session
    }

    /// Whether the last cycle found the source's ring empty (its clock
    /// stands still).
    pub fn is_starved(&self) -> bool {
        self.output
            .inner()
            .mix
            .reading(self.index, self.epoch)
            .is_some_and(|r| r.starved)
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.output.retire(self.index, self.epoch, self.session);
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeDevice;
    use super::super::{DeviceState, ManualClock, MonoClock};
    use super::*;
    use crate::ring::SampleRing;

    struct Rig {
        output: Arc<Output>,
        device: FakeDevice,
        clock: Arc<ManualClock>,
    }

    fn rig() -> Rig {
        let device = FakeDevice::new();
        let clock = Arc::new(ManualClock::default());
        clock.advance_ms(1);
        let output = Output::manual(
            Box::new(device.clone()),
            Arc::clone(&clock) as Arc<dyn MonoClock>,
        );
        output.request(None, true);
        Rig {
            output,
            device,
            clock,
        }
    }

    fn ring(frames: usize) -> Arc<SampleRing> {
        let ring = Arc::new(SampleRing::new(frames));
        ring.push(&vec![0.1; frames * 2]);
        ring
    }

    /// One callback of 512 frames, 10.666 ms after the last one.
    fn cycle(rig: &Rig, delay: i64) {
        rig.clock.advance_ns(10_666_667);
        rig.device.pull(512, delay).expect("the device runs");
    }

    /// A callback at the current time of the manual clock.
    fn cycle_now(rig: &Rig, delay: i64) {
        rig.device.pull(512, delay).expect("the device runs");
    }

    /// The microseconds of `frames` frames, as the clock computes them.
    fn us(frames: i64) -> i64 {
        frames * 1_000_000 / 48_000
    }

    #[test]
    fn the_clock_is_the_frames_taken_minus_the_delay() {
        let r = rig();
        let source = r.output.attach(ring(48_000), 0).expect("joins");
        // Before the first callback: 0.
        assert_eq!(source.clock_us(), 0);
        cycle(&r, 512);
        // 512 frames taken, 512 in the device: nothing is audible yet.
        assert_eq!(source.clock_us(), 0);
        cycle(&r, 512);
        assert_eq!(source.clock_us(), us(512));
        cycle(&r, 512);
        assert_eq!(source.clock_us(), us(1024));
        // A delay that grows by a cycle: the clock stands still.
        cycle(&r, 1024);
        assert_eq!(source.clock_us(), us(1024));
    }

    #[test]
    fn the_clock_interpolates_between_callbacks_and_stops_at_the_frames_taken() {
        let r = rig();
        let source = r.output.attach(ring(48_000), 0).expect("joins");
        cycle(&r, 512);
        cycle(&r, 512);
        let base = source.clock_us();
        // 5 ms after the callback: 240 frames more.
        r.clock.advance_ms(5);
        let mid = source.clock_us();
        assert_eq!(mid - base, us(240));
        // The next callback comes 5.67 ms later: the snapshot is where the
        // interpolation was or later (the clock never went back).
        r.clock.advance_ns(5_666_667);
        cycle_now(&r, 512);
        assert!(source.clock_us() >= mid);
        // A callback that never comes: the clock stops at the frames that
        // the device got (what was taken), not beyond.
        r.clock.advance_ms(500);
        assert_eq!(source.clock_us(), us(1536));
    }

    #[test]
    fn the_clock_never_goes_back() {
        let r = rig();
        let source = r.output.attach(ring(48_000), 0).expect("joins");
        let mut last = 0;
        for delay in [512, 512, 1024, 2048, 512, 0, 512, 4096, 512] {
            cycle(&r, delay);
            for _ in 0..4 {
                r.clock.advance_ms(2);
                let now = source.clock_us();
                assert!(now >= last, "{now} < {last} at delay {delay}");
                last = now;
            }
        }
        assert!(last > 0);
    }

    #[test]
    fn an_empty_ring_stops_the_clock_and_new_samples_start_it() {
        let r = rig();
        // 1,000 frames: the second cycle finds the ring empty.
        let ring = ring(1000);
        let source = r.output.attach(Arc::clone(&ring), 0).expect("joins");
        cycle(&r, 0);
        assert!(!source.is_starved());
        cycle(&r, 0);
        assert!(source.is_starved());
        let at_end = source.clock_us();
        // The 1,000 frames that were taken, less the cycle that is not
        // audible yet (the device reported 0).
        assert_eq!(at_end, us(1000 - 512));
        // The mixer plays silence for it, and its clock stands still.
        r.clock.advance_ms(30);
        cycle(&r, 0);
        r.clock.advance_ms(30);
        assert_eq!(source.clock_us(), at_end);
        // New samples: the clock goes on from where it stopped.
        ring.push(&vec![0.1; 512 * 2]);
        cycle(&r, 0);
        assert!(!source.is_starved());
        // (The frames count, not the microseconds, adds up: 1 us of rounding.)
        assert!((source.clock_us() - (at_end + us(512))).abs() <= 1);
    }

    #[test]
    fn a_seek_discards_the_ring_and_the_clock_waits_for_the_new_frames() {
        let r = rig();
        let ring = ring(4096);
        let source = r.output.attach(Arc::clone(&ring), 0).expect("joins");
        cycle(&r, 0);
        cycle(&r, 0);
        let before = source.clock_us();
        assert_eq!(before, us(512));
        // The seek: the ring is discarded, nothing comes for a while.
        ring.discard_all();
        cycle(&r, 0);
        cycle(&r, 0);
        assert!(source.is_starved());
        assert_eq!(source.clock_us(), before);
        // The first frames of the new position.
        ring.push(&vec![0.2; 256 * 2]);
        cycle(&r, 0);
        assert!((source.clock_us() - (before + us(256))).abs() <= 1);
    }

    #[test]
    fn silence_that_aligns_the_source_counts_as_played_time() {
        let r = rig();
        // The ring is 480 frames (10 ms) ahead of the media clock: the
        // mixer plays 480 frames of silence first, and the clock moves
        // through them.
        let ring = ring(48_000);
        let source = r.output.attach(Arc::clone(&ring), 480).expect("joins");
        cycle(&r, 0);
        cycle(&r, 0);
        assert_eq!(source.clock_us(), us(512));
        assert_eq!(
            ring.read_position(),
            544,
            "1,024 frames, 480 of them silence"
        );
    }

    #[test]
    fn a_dead_source_goes_on_in_real_time_from_the_failure() {
        let r = rig();
        let source = r.output.attach(ring(48_000), 0).expect("joins");
        cycle(&r, 512);
        cycle(&r, 512);
        let before = source.clock_us();
        // 4 ms after the last callback the device fails; the owner looks at
        // the clock 100 ms later. The element's time did not stand still:
        // it has the 4 ms that the clock had interpolated at the failure,
        // and the 100 ms since.
        r.clock.advance_ms(4);
        r.device.report(DeviceState::Error);
        r.output.tick();
        assert!(!source.is_live());
        r.clock.advance_ms(100);
        let after = source.clock_us();
        assert!(
            (after - before - 104_000).abs() <= 1_000,
            "{before} -> {after}"
        );
        // Never back, and still moving.
        r.clock.advance_ms(50);
        assert!(source.clock_us() >= after + 49_000);
    }
}
