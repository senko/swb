//! The frame log of the GUI: what `RUST_LOG=swb::gui=debug` prints about
//! input and frames, for `swbtools gui-latency` (docs/testing.md).
//!
//! Two kinds of lines, with fixed formats (times in seconds of
//! `CLOCK_MONOTONIC`, six decimals; the reader is `tools/swbtools/
//! frame_log.py`):
//!
//! - `batch start=T end=T moves=N wheels=N presses=N releases=N keys=N
//!   frame=F`: one per batch of window events that had input, from
//!   `new_events` to the end of `about_to_wait`. The counts are the input
//!   events that the window delivered in the batch. `frame` is 1 if the
//!   batch requested a redraw (the frame decision at the end of
//!   `about_to_wait`, ADR 0040), else 0: a batch with `frame=0` changed
//!   nothing that a frame shows, so no frame follows it.
//! - `frame at=T render=D compose=D present=D`: one per presented frame:
//!   the time after `present`, and the durations of the page render, of
//!   the composition (toolbar, copy into the window buffer) and of
//!   `present`.
//!
//! The cost without the debug level is a few counters per event and one
//! `Instant` read per frame. The log lines and the clock reads of batches
//! exist only at the debug level.

use std::time::{Duration, Instant};

/// The target of the log lines.
const TARGET: &str = "swb::gui";

/// Input events by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputCounts {
    /// Pointer moves.
    pub(crate) moves: u32,
    /// Wheel events.
    pub(crate) wheels: u32,
    /// Button presses.
    pub(crate) presses: u32,
    /// Button releases.
    pub(crate) releases: u32,
    /// Key events (presses and releases).
    pub(crate) keys: u32,
}

impl InputCounts {
    /// True if no input event was counted.
    fn is_empty(&self) -> bool {
        *self == InputCounts::default()
    }
}

/// The state of the frame log: the input of the current batch.
#[derive(Debug)]
pub(crate) struct FrameLog {
    enabled: bool,
    counts: InputCounts,
    batch_start: f64,
}

impl FrameLog {
    /// A frame log that prints if the debug level of `swb::gui` is on now.
    pub(crate) fn new() -> Self {
        FrameLog {
            enabled: log::log_enabled!(target: TARGET, log::Level::Debug),
            counts: InputCounts::default(),
            batch_start: 0.0,
        }
    }

    /// A batch of window events starts (`new_events`).
    pub(crate) fn begin_batch(&mut self) {
        self.counts = InputCounts::default();
        if self.enabled {
            self.batch_start = monotonic_seconds();
        }
    }

    /// Counts input events of the batch.
    pub(crate) fn counts_mut(&mut self) -> &mut InputCounts {
        &mut self.counts
    }

    /// The batch ends (the end of `about_to_wait`): prints its line if it
    /// had input. `frame` is true if the batch requested a redraw.
    pub(crate) fn end_batch(&mut self, frame: bool) {
        if !self.enabled || self.counts.is_empty() {
            return;
        }
        let line = batch_line(self.batch_start, monotonic_seconds(), &self.counts, frame);
        log::debug!(target: TARGET, "{line}");
        self.counts = InputCounts::default();
    }

    /// The durations of the parts of one redraw, since `started` (the
    /// start of the redraw): the page render ended at `rendered`, the
    /// composition at `composed`, and `present` ended now.
    pub(crate) fn frame(&self, started: Instant, rendered: Instant, composed: Instant) {
        if !self.enabled {
            return;
        }
        let presented = Instant::now();
        let line = frame_line(
            monotonic_seconds(),
            rendered.duration_since(started),
            composed.duration_since(rendered),
            presented.duration_since(composed),
        );
        log::debug!(target: TARGET, "{line}");
    }
}

/// The text of a batch line.
pub(crate) fn batch_line(start: f64, end: f64, counts: &InputCounts, frame: bool) -> String {
    format!(
        "batch start={start:.6} end={end:.6} moves={} wheels={} presses={} releases={} keys={} \
         frame={}",
        counts.moves,
        counts.wheels,
        counts.presses,
        counts.releases,
        counts.keys,
        u8::from(frame)
    )
}

/// The text of a frame line.
pub(crate) fn frame_line(
    at: f64,
    render: Duration,
    compose: Duration,
    present: Duration,
) -> String {
    format!(
        "frame at={at:.6} render={:.6} compose={:.6} present={:.6}",
        render.as_secs_f64(),
        compose.as_secs_f64(),
        present.as_secs_f64()
    )
}

/// The time of `CLOCK_MONOTONIC` in seconds (the clock that tools read
/// with `time.clock_gettime(time.CLOCK_MONOTONIC)`).
#[cfg(unix)]
pub(crate) fn monotonic_seconds() -> f64 {
    use rustix::time::{ClockId, clock_gettime};
    let t = clock_gettime(ClockId::Monotonic);
    t.tv_sec as f64 + t.tv_nsec as f64 / 1e9
}

/// The time since the first call, in seconds (no `CLOCK_MONOTONIC` here).
#[cfg(not(unix))]
pub(crate) fn monotonic_seconds() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_line_format() {
        let counts = InputCounts {
            moves: 3,
            wheels: 1,
            presses: 0,
            releases: 2,
            keys: 4,
        };
        assert_eq!(
            batch_line(12.5, 12.5625, &counts, true),
            "batch start=12.500000 end=12.562500 moves=3 wheels=1 presses=0 releases=2 keys=4 \
             frame=1"
        );
        assert!(batch_line(1.0, 2.0, &counts, false).ends_with(" frame=0"));
    }

    #[test]
    fn frame_line_format() {
        let ms = Duration::from_millis;
        assert_eq!(
            frame_line(100.25, ms(10), ms(3), Duration::from_micros(1500)),
            "frame at=100.250000 render=0.010000 compose=0.003000 present=0.001500"
        );
    }

    #[test]
    fn the_clock_runs_forward() {
        let a = monotonic_seconds();
        let b = monotonic_seconds();
        assert!(a > 0.0 && b >= a);
    }

    #[test]
    fn a_batch_without_input_prints_nothing() {
        let mut log = FrameLog {
            enabled: true,
            counts: InputCounts::default(),
            batch_start: 0.0,
        };
        log.begin_batch();
        assert!(log.counts.is_empty());
        log.counts_mut().moves += 1;
        assert!(!log.counts.is_empty());
        log.end_batch(false);
        assert!(log.counts.is_empty());
    }
}
