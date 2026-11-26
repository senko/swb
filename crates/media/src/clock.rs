//! The media clock (ADR 0047 section 7): media time = anchor media time +
//! (clock time − anchor clock time) while it runs, at rate 1.
//!
//! Times are microseconds. The clock does not read a clock itself: the
//! owner passes the clock time of its source to every call
//! ([`ClockSource`]), so the same code runs on page time (headless, tests,
//! virtual time) and on the audio clock.

/// Where the clock time that the owner passes comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockSource {
    /// The page clock (`PageClock`, ADR 0028 section 7) in real or virtual
    /// time, in microseconds: headless mode, tests, the GUI for an element
    /// without audio or while the audio output is off or failed (there it
    /// is the monotonic clock). The silent output consumes the samples at
    /// this clock's rate.
    #[default]
    Page,
    /// The audio clock of the element's source in the mixer
    /// ([`crate::output::Source::clock_us`], ADR 0047 section 7): the
    /// frames that the mixer took for the element, over 48,000, minus the
    /// output delay, with an interpolation between callbacks. Its origin is
    /// the moment the source joined the mixer.
    Audio,
}

/// A media clock with an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaClock {
    source: ClockSource,
    anchor_media: i64,
    anchor_clock: i64,
    running: bool,
}

impl MediaClock {
    /// A stopped clock at media time `media` on `source`.
    pub fn new(source: ClockSource, media: i64) -> MediaClock {
        MediaClock {
            source,
            anchor_media: media,
            anchor_clock: 0,
            running: false,
        }
    }

    /// The clock source.
    pub fn source(&self) -> ClockSource {
        self.source
    }

    /// Whether the clock runs.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The media time at clock time `now`.
    pub fn media_time(&self, now: i64) -> i64 {
        if self.running {
            self.anchor_media
                .saturating_add(now.saturating_sub(self.anchor_clock))
        } else {
            self.anchor_media
        }
    }

    /// The clock time at which the media time is `media`, while the clock
    /// runs; `None` when it is stopped.
    pub fn clock_time(&self, media: i64) -> Option<i64> {
        self.running.then(|| {
            self.anchor_clock
                .saturating_add(media.saturating_sub(self.anchor_media))
        })
    }

    /// Starts the clock at clock time `now` from its media time (no jump).
    pub fn start(&mut self, now: i64) {
        if !self.running {
            self.anchor_clock = now;
            self.running = true;
        }
    }

    /// Stops the clock at clock time `now`; the media time stays.
    pub fn stop(&mut self, now: i64) {
        self.anchor_media = self.media_time(now);
        self.running = false;
    }

    /// Sets the media time (a seek, the end, an underrun): the clock is
    /// stopped there.
    pub fn set(&mut self, media: i64) {
        self.anchor_media = media;
        self.running = false;
    }

    /// Moves the clock to another source without a jump: the media time at
    /// the old source's clock time `old_now` stays, and the new source's
    /// clock time `new_now` is the same moment. A stopped clock only changes
    /// its source.
    pub fn rebase(&mut self, source: ClockSource, old_now: i64, new_now: i64) {
        if self.running {
            self.anchor_media = self.media_time(old_now);
            self.anchor_clock = new_now;
        }
        self.source = source;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_runs_from_its_anchor() {
        let mut c = MediaClock::new(ClockSource::Page, 1_000);
        assert_eq!(c.media_time(50_000), 1_000);
        assert_eq!(c.clock_time(2_000), None);
        c.start(10_000);
        assert_eq!(c.media_time(10_000), 1_000);
        assert_eq!(c.media_time(260_000), 251_000);
        assert_eq!(c.clock_time(2_000), Some(11_000));
        // A second start does not move the anchor.
        c.start(20_000);
        assert_eq!(c.media_time(260_000), 251_000);
        c.stop(260_000);
        assert!(!c.is_running());
        assert_eq!(c.media_time(900_000), 251_000);
        c.start(1_000_000);
        assert_eq!(c.media_time(1_100_000), 351_000);
        c.set(5);
        assert_eq!(c.media_time(2_000_000), 5);
    }

    #[test]
    fn a_rebase_keeps_the_media_time() {
        let mut c = MediaClock::new(ClockSource::Page, 1_000);
        c.start(10_000);
        // At page time 60,000 the media time is 51,000; the audio clock
        // reads 7,000 then.
        c.rebase(ClockSource::Audio, 60_000, 7_000);
        assert_eq!(c.source(), ClockSource::Audio);
        assert_eq!(c.media_time(7_000), 51_000);
        assert_eq!(c.media_time(57_000), 101_000);
        // And back: no jump either.
        c.rebase(ClockSource::Page, 57_000, 500_000);
        assert_eq!(c.media_time(500_000), 101_000);
        // A stopped clock only changes its source.
        c.stop(510_000);
        c.rebase(ClockSource::Audio, 0, 9_999_999);
        assert_eq!(c.media_time(1), 111_000);
        c.start(3);
        assert_eq!(c.media_time(13), 111_010);
    }
}
