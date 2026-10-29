//! The page clock (ADR 0028 section 7, "Clocks") and the time of a
//! document (High Resolution Time, <https://w3c.github.io/hr-time/>).
//!
//! A [`PageClock`] belongs to a page and outlives its documents. It counts
//! page time in milliseconds from its creation, in one of two modes:
//!
//! - real time: page time is the time since the clock was created;
//! - virtual time: page time stands still until the event loop moves it
//!   to the next task ([`PageClock::advance_to`]), when no task is due and
//!   no request is pending. Tests and comparisons then do not wait for
//!   timers, and their results do not depend on the speed of the machine.
//!
//! The clock maps page time to wall time from a start time: the system
//! time at creation, or a fixed time (`swb --now`). `Date.now()` is the
//! wall time of the current page time, except in real time from the
//! system time: there it is the system time, which also counts the time
//! that the machine was suspended (page time does not).
//!
//! A document's time origin is the page time when its runtime was created
//! ([`DocumentClock`]); `performance.now()`, `performance.timeOrigin` and
//! `Event.timeStamp` count from it, coarsened to 0.1 ms as Chromium does for
//! documents that are not cross-origin isolated (measured: the smallest
//! step of `performance.now()` is 0.1 ms).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How a page clock advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    /// Page time is real time.
    Real,
    /// Page time moves only when the event loop moves it.
    Virtual,
}

/// The clock of a page (shared by its documents; cloning shares it).
#[derive(Clone, Debug)]
pub struct PageClock(Rc<ClockState>);

#[derive(Debug)]
struct ClockState {
    mode: ClockMode,
    /// The wall time of page time 0, in milliseconds since the epoch.
    start_wall: f64,
    /// True if `start_wall` is a fixed time, not the system time.
    fixed_start: bool,
    /// The real time of page time 0.
    start: Instant,
    /// The page time in virtual mode.
    virtual_now: Cell<f64>,
}

/// The largest time value of ECMAScript (§21.4.1.22, `TimeClip`), in
/// milliseconds since the epoch.
const MAX_TIME_MS: f64 = 8.64e15;

impl PageClock {
    /// A clock in `mode` whose page time 0 is `start` (milliseconds since
    /// the epoch), or the system time now.
    pub fn new(mode: ClockMode, start: Option<f64>) -> PageClock {
        let start = start.filter(|t| t.is_finite() && t.abs() <= MAX_TIME_MS);
        PageClock(Rc::new(ClockState {
            mode,
            start_wall: start.unwrap_or_else(system_time_ms),
            fixed_start: start.is_some(),
            start: Instant::now(),
            virtual_now: Cell::new(0.0),
        }))
    }

    /// A real-time clock that starts at the system time.
    pub fn real() -> PageClock {
        PageClock::new(ClockMode::Real, None)
    }

    /// The mode.
    pub fn mode(&self) -> ClockMode {
        self.0.mode
    }

    /// True in virtual time.
    pub fn is_virtual(&self) -> bool {
        self.0.mode == ClockMode::Virtual
    }

    /// The page time in milliseconds.
    pub fn now(&self) -> f64 {
        match self.0.mode {
            ClockMode::Real => self.0.start.elapsed().as_secs_f64() * 1000.0,
            ClockMode::Virtual => self.0.virtual_now.get(),
        }
    }

    /// The wall time now, in milliseconds since the epoch (`Date.now()`).
    pub fn wall_now(&self) -> f64 {
        if self.0.mode == ClockMode::Real && !self.0.fixed_start {
            return system_time_ms();
        }
        self.0.start_wall + self.now()
    }

    /// The wall time of page time 0, in milliseconds since the epoch.
    pub fn start_wall(&self) -> f64 {
        self.0.start_wall
    }

    /// In virtual time, moves the page time forward to `time` (never
    /// back); in real time, does nothing.
    pub fn advance_to(&self, time: f64) {
        if self.0.mode == ClockMode::Virtual && time > self.0.virtual_now.get() {
            self.0.virtual_now.set(time);
        }
    }

    /// The real time at which page time reaches `time`: in real time the
    /// instant, in virtual time now (the event loop moves the clock
    /// itself).
    pub fn instant_of(&self, time: f64) -> Instant {
        match self.0.mode {
            ClockMode::Real => {
                let ms = (time - self.now()).max(0.0);
                let wait = Duration::try_from_secs_f64(ms / 1000.0).unwrap_or(Duration::MAX);
                Instant::now().checked_add(wait).unwrap_or_else(far_future)
            }
            ClockMode::Virtual => Instant::now(),
        }
    }
}

impl Default for PageClock {
    fn default() -> PageClock {
        PageClock::real()
    }
}

/// An instant far enough away to stand for "never".
fn far_future() -> Instant {
    let now = Instant::now();
    now.checked_add(Duration::from_hours(24 * 365))
        .unwrap_or(now)
}

/// The system time in milliseconds since the epoch (0 before it).
fn system_time_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Coarsens a time for scripts (0.1 ms steps, rounded down).
fn coarsen(ms: f64) -> f64 {
    (ms * 10.0).floor() / 10.0
}

/// The time of a document: the page clock and the document's time origin.
#[derive(Clone, Debug, Default)]
pub(crate) struct DocumentClock {
    pub(crate) page: PageClock,
    /// The page time of the time origin.
    origin: f64,
}

impl DocumentClock {
    /// The clock of a document whose time origin is now.
    pub(crate) fn new(page: PageClock) -> DocumentClock {
        let origin = page.now();
        DocumentClock { page, origin }
    }

    /// `performance.now()`: the coarsened time since the time origin (HR
    /// Time "current high resolution time").
    pub(crate) fn now(&self) -> f64 {
        self.at(self.page.now())
    }

    /// The `performance.now()` value of the page time `time`.
    pub(crate) fn at(&self, time: f64) -> f64 {
        coarsen((time - self.origin).max(0.0))
    }

    /// `performance.timeOrigin`: the wall time of the time origin.
    pub(crate) fn time_origin(&self) -> f64 {
        coarsen(self.page.start_wall() + self.origin)
    }

    /// The page time now, unrounded.
    pub(crate) fn page_now(&self) -> f64 {
        self.page.now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_time_moves_only_forward_and_only_when_told() {
        let clock = PageClock::new(ClockMode::Virtual, Some(1_000_000.0));
        assert_eq!(clock.now(), 0.0);
        assert_eq!(clock.wall_now(), 1_000_000.0);
        clock.advance_to(25.5);
        clock.advance_to(10.0);
        assert_eq!(clock.now(), 25.5);
        let document = DocumentClock::new(clock.clone());
        clock.advance_to(30.04);
        assert_eq!(document.now(), 4.5);
        assert_eq!(document.time_origin(), 1_000_025.5);
    }

    #[test]
    fn real_time_ignores_advances_and_bad_start_times() {
        let clock = PageClock::new(ClockMode::Real, Some(f64::NAN));
        clock.advance_to(1e9);
        assert!(clock.now() < 1e6);
        assert!(clock.start_wall() > 1.5e12);
        assert!(clock.instant_of(clock.now() + 50.0) > Instant::now());
        assert_eq!(coarsen(12.3456), 12.3);
    }
}
