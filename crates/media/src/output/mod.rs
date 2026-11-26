//! The audio output (ADR 0047 section 8; M14 feature 8): one process-wide
//! [`Output`] that the GUI turns on, a [`Mix`] that sums the sample rings of
//! the playing elements on the device's real-time thread, and the audio
//! clock of each element ([`Source::clock_us`]).
//!
//! The device sits behind the [`Backend`] trait: the PipeWire stream
//! (feature `pipewire`, [`Output::pipewire`]) in the GUI, and the
//! [`fake`] device in the tests, which a test drives by hand (it pulls N
//! frames and reports a delay) so that the mixer, the states, the fallback,
//! the timers (with an injected [`MonoClock`]) and the engine's integration
//! are deterministic. Headless mode, the automation server and the tests
//! never connect to PipeWire: they never turn the output on, and every
//! pipeline uses its silent output.
//!
//! Parts: `mix.rs` (the real-time callback and its slot protocol),
//! `control.rs` (the states, the timers and the fallback), `source.rs` (the
//! audio clock), `fake.rs` and `pw.rs` (the devices).

mod control;
pub mod fake;
mod mix;
#[cfg(feature = "pipewire")]
mod pw;
mod source;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub use control::{Output, OutputStats, Phase};
pub use mix::{MAX_SOURCES, Mix, RAMP_FRAMES, Reading};
pub use source::Source;

use crate::OUTPUT_RATE;
use crate::ring::SampleRing;

/// A monotonic clock in nanoseconds. The output reads it for the time of
/// each callback (the audio clock's interpolation) and for its timers; a
/// test injects [`ManualClock`].
pub trait MonoClock: Send + Sync {
    /// Nanoseconds since some fixed point; never goes back.
    fn now_ns(&self) -> u64;
}

/// The real clock: `Instant`, from the first use.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl MonoClock for SystemClock {
    fn now_ns(&self) -> u64 {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let epoch = EPOCH.get_or_init(Instant::now);
        u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// A clock that a test sets.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// Moves the clock on by `ms` milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.0.fetch_add(ms * 1_000_000, Ordering::Relaxed);
    }

    /// Moves the clock on by `ns` nanoseconds.
    pub fn advance_ns(&self, ns: u64) {
        self.0.fetch_add(ns, Ordering::Relaxed);
    }
}

impl MonoClock for ManualClock {
    fn now_ns(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// What the owner wakes when the output changes state (a device that
/// streams now, or failed): the page's wake-up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The state of a device stream, as the backend reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Not connected (also after the server went away).
    Unconnected,
    /// Connecting.
    Connecting,
    /// Connected, not running.
    Paused,
    /// Cycles run.
    Streaming,
    /// Failed.
    Error,
}

/// What the device calls: [`Sink::fill`] from its real-time thread once per
/// cycle, [`Sink::state`] from its loop thread at each change of state.
pub trait Sink: Send + Sync {
    /// Fills `out` (interleaved stereo `f32`) for one cycle; `delay_frames`
    /// is the delay between its frames and the device.
    fn fill(&self, out: &mut [f32], delay_frames: i64);
    /// The device's stream changed state.
    fn state(&self, state: DeviceState);
}

/// A device that the output opened: it can be stopped and started; dropping
/// it closes it (no callback runs afterwards).
pub trait Device: Send {
    /// Starts or stops the cycles.
    fn set_active(&mut self, active: bool);
}

/// Opens devices.
pub trait Backend: Send + Sync {
    /// Opens a device that calls `sink`, without waiting for it to stream
    /// (it reports [`DeviceState::Streaming`] when it does). An error means
    /// that there is no server or no stream.
    fn connect(&self, sink: Arc<dyn Sink>) -> Result<Box<dyn Device>, String>;
}

/// The frames that a source's ring is ahead of the media clock: the media
/// time of the ring's next frame ([`SampleRing::base`] maps its position)
/// minus `media_us`, in frames. A positive number is silence to play before
/// the first frame ([`Mix::attach`]), a negative number is frames to skip,
/// so that the sound is at the clock's time. 0 without a base (no sample
/// yet). At most 2 s either way.
pub fn lead_frames(ring: &SampleRing, media_us: i64) -> i64 {
    let Some(base) = ring.base() else {
        return 0;
    };
    let content = base.media_time(ring.read_position());
    let us = content.saturating_sub(media_us);
    let frames = us.saturating_mul(i64::from(OUTPUT_RATE)) / 1_000_000;
    frames.clamp(-2 * i64::from(OUTPUT_RATE), 2 * i64::from(OUTPUT_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::RingBase;

    #[test]
    fn the_lead_is_the_distance_of_the_ring_from_the_clock() {
        let ring = SampleRing::new(1000);
        assert_eq!(lead_frames(&ring, 5_000), 0);
        ring.set_base(RingBase {
            position: 0,
            media_us: 1_000_000,
            epoch: 0,
        });
        ring.push(&[0.0; 2000]);
        // The next frame is at 1 s; the clock is at 0.99 s: 480 frames of
        // silence first.
        assert_eq!(lead_frames(&ring, 990_000), 480);
        // The clock is ahead: skip.
        assert_eq!(lead_frames(&ring, 1_010_000), -480);
        ring.skip(480);
        assert_eq!(lead_frames(&ring, 1_010_000), 0);
        // A jump of minutes is bounded.
        assert_eq!(lead_frames(&ring, -600_000_000), 96_000);
    }

    #[test]
    fn the_manual_clock_moves_when_told() {
        let c = ManualClock::default();
        assert_eq!(c.now_ns(), 0);
        c.advance_ms(3);
        c.advance_ns(5);
        assert_eq!(c.now_ns(), 3_000_005);
        assert!(SystemClock.now_ns() <= SystemClock.now_ns());
    }
}
