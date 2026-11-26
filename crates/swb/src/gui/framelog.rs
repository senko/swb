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
//! - `frame at=T render=D compose=D present=D full=F rects=N pixels=N`:
//!   one per presented frame: the time after `present`, and the durations
//!   of the page render, of the composition (toolbar, copy into the window
//!   buffer) and of `present`; then the damage of the page raster (ADR 0044
//!   section 18): `full` 1 for a full raster, the number of rectangles and
//!   the device pixels rasterized (all 0 when the page was not drawn).
//!
//! - `media at=T wakeups=N shown=N dropped=N decoded=N converted=N`
//!   (M14 feature 7b): at most twice per second, when something changed:
//!   the wake-ups of the event loop since the start (each `new_events`),
//!   and the counters of the video pipelines of the page
//!   (`Page::media_counters`). For the measurements of video playback; the
//!   reader of the other lines ignores it.
//!
//! - `sync at=T video=S media=S audio=A` (M14 feature 8): one per playing,
//!   painted video at each presented frame (after its `frame` line): `video`
//!   is the presentation time of the frame that was drawn and `media` the
//!   element's media time at the present, both in seconds of media time (six
//!   decimals); `audio` is 1 while the element's clock is the audio clock.
//!   The sync trace of ADR 0047 section 7 (works item 4): `media - video`
//!   is at most a frame period plus the time that the page took to draw.
//!
//! The cost without the debug level is a few counters per event and three
//! `Instant` reads per frame (the start, the end of the page render and the
//! end of the composition). The log lines and the clock reads of batches
//! exist only at the debug level.

use std::time::{Duration, Instant};

use swb_engine::{FrameDamage, MediaCounters, MediaSync};

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
    /// The batches so far: the wake-ups of the event loop.
    wakeups: u64,
    /// What the last `media` line said: its time and the numbers.
    media_logged: (f64, u64, MediaCounters),
}

/// The least time between two `media` lines, in seconds.
const MEDIA_LINE_EVERY: f64 = 0.5;

impl FrameLog {
    /// A frame log that prints if the debug level of `swb::gui` is on now.
    pub(crate) fn new() -> Self {
        FrameLog {
            enabled: log::log_enabled!(target: TARGET, log::Level::Debug),
            counts: InputCounts::default(),
            batch_start: 0.0,
            wakeups: 0,
            media_logged: (0.0, 0, MediaCounters::default()),
        }
    }

    /// A batch of window events starts (`new_events`).
    pub(crate) fn begin_batch(&mut self) {
        self.counts = InputCounts::default();
        self.wakeups += 1;
        if self.enabled {
            self.batch_start = monotonic_seconds();
        }
    }

    /// Counts input events of the batch.
    pub(crate) fn counts_mut(&mut self) -> &mut InputCounts {
        &mut self.counts
    }

    /// The batch ends (the end of `about_to_wait`): prints its line if it
    /// had input. `frame` is true if the batch requested a redraw. `media`
    /// reads the counters of the page's videos; it runs only when a
    /// `media` line may be due.
    pub(crate) fn end_batch(&mut self, frame: bool, media: impl FnOnce() -> MediaCounters) {
        if !self.enabled {
            return;
        }
        let now = monotonic_seconds();
        if now - self.media_logged.0 >= MEDIA_LINE_EVERY {
            let counters = media();
            if (self.wakeups, counters) != (self.media_logged.1, self.media_logged.2) {
                let line = media_line(now, self.wakeups, &counters);
                log::debug!(target: TARGET, "{line}");
                self.media_logged = (now, self.wakeups, counters);
            }
        }
        if self.counts.is_empty() {
            return;
        }
        let line = batch_line(self.batch_start, now, &self.counts, frame);
        log::debug!(target: TARGET, "{line}");
        self.counts = InputCounts::default();
    }

    /// The durations of the parts of one redraw, since `started` (the
    /// start of the redraw): the page render ended at `rendered`, the
    /// composition at `composed`, and `present` ended now. `damage` is what
    /// the page raster drew (`None`: the page was not drawn).
    pub(crate) fn frame(
        &self,
        started: Instant,
        rendered: Instant,
        composed: Instant,
        damage: Option<&FrameDamage>,
    ) {
        if !self.enabled {
            return;
        }
        let presented = Instant::now();
        let line = frame_line(
            monotonic_seconds(),
            rendered.duration_since(started),
            composed.duration_since(rendered),
            presented.duration_since(composed),
            damage,
        );
        log::debug!(target: TARGET, "{line}");
    }
}

impl FrameLog {
    /// The sync trace (M14 feature 8): after a present, one `sync` line for
    /// each playing, painted video. `samples` reads them; it runs only at
    /// the debug level.
    pub(crate) fn sync(&self, samples: impl FnOnce() -> Vec<MediaSync>) {
        if !self.enabled {
            return;
        }
        let at = monotonic_seconds();
        for sample in samples() {
            log::debug!(target: TARGET, "{}", sync_line(at, &sample));
        }
    }
}

/// The text of a sync line: the time of the present, the presentation time
/// of the frame that was drawn and the media time of the element, in
/// seconds, and whether the element's clock is the audio clock.
pub(crate) fn sync_line(at: f64, sample: &MediaSync) -> String {
    format!(
        "sync at={at:.6} video={:.6} media={:.6} audio={}",
        sample.pts_us as f64 / 1e6,
        sample.media_us as f64 / 1e6,
        u8::from(sample.audio_clock)
    )
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

/// The text of a frame line: the times, then the damage of the page raster
/// (`full` 1 for a full raster, the number of rectangles and the device
/// pixels rasterized; all 0 when the page was not drawn).
pub(crate) fn frame_line(
    at: f64,
    render: Duration,
    compose: Duration,
    present: Duration,
    damage: Option<&FrameDamage>,
) -> String {
    let (full, rects, pixels) =
        damage.map_or((false, 0, 0), |d| (d.full, d.rects.len(), d.pixels()));
    format!(
        "frame at={at:.6} render={:.6} compose={:.6} present={:.6} full={} rects={rects} \
         pixels={pixels}",
        render.as_secs_f64(),
        compose.as_secs_f64(),
        present.as_secs_f64(),
        u8::from(full)
    )
}

/// The text of a media line: the wake-ups of the event loop so far and the
/// counters of the video pipelines.
pub(crate) fn media_line(at: f64, wakeups: u64, counters: &MediaCounters) -> String {
    format!(
        "media at={at:.6} wakeups={wakeups} shown={} dropped={} decoded={} converted={}",
        counters.shown, counters.dropped, counters.decoded, counters.converted
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
            frame_line(100.25, ms(10), ms(3), Duration::from_micros(1500), None),
            "frame at=100.250000 render=0.010000 compose=0.003000 present=0.001500 full=0 \
             rects=0 pixels=0"
        );
        let damage = FrameDamage {
            full: false,
            rects: vec![swb_paint::PixelRect::from_edges(0, 0, 10, 20)],
            viewport_pixels: 1000,
        };
        assert!(
            frame_line(1.0, ms(1), ms(1), ms(1), Some(&damage))
                .ends_with(" full=0 rects=1 pixels=200")
        );
    }

    #[test]
    fn sync_line_format() {
        let sample = MediaSync {
            pts_us: 1_040_000,
            media_us: 1_052_345,
            audio_clock: true,
        };
        assert_eq!(
            sync_line(7.5, &sample),
            "sync at=7.500000 video=1.040000 media=1.052345 audio=1"
        );
        let page_clock = MediaSync {
            audio_clock: false,
            ..sample
        };
        assert!(sync_line(7.5, &page_clock).ends_with(" audio=0"));
    }

    #[test]
    fn the_clock_runs_forward() {
        let a = monotonic_seconds();
        let b = monotonic_seconds();
        assert!(a > 0.0 && b >= a);
    }

    #[test]
    fn media_line_format() {
        let counters = MediaCounters {
            shown: 250,
            dropped: 2,
            decoded: 260,
            converted: 255,
        };
        assert_eq!(
            media_line(7.5, 301, &counters),
            "media at=7.500000 wakeups=301 shown=250 dropped=2 decoded=260 converted=255"
        );
    }

    #[test]
    fn a_batch_without_input_prints_nothing() {
        let mut log = FrameLog::new();
        log.enabled = true;
        log.begin_batch();
        assert!(log.counts.is_empty());
        log.counts_mut().moves += 1;
        assert!(!log.counts.is_empty());
        log.end_batch(false, MediaCounters::default);
        assert!(log.counts.is_empty());
    }

    #[test]
    fn the_media_counters_are_read_at_most_twice_per_second() {
        let mut log = FrameLog::new();
        log.enabled = true;
        let mut reads = 0;
        for _ in 0..3 {
            log.begin_batch();
            log.end_batch(false, || {
                reads += 1;
                MediaCounters::default()
            });
        }
        assert_eq!(reads, 1, "the second and the third batch come at once");
    }
}
