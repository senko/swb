//! Time for the `Date` built-in (ECMA-262 §21.4.1,
//! <https://tc39.es/ecma262/#sec-time-values-and-time-range>) and the host
//! hook for the clock and the time zone (ADR 0027).
//!
//! `time_value` has the calendar and the `Make*` operations, `zone` the
//! local time zone (over `jiff`), `zone_names` and the generated
//! `zone_names_data` the long zone names, `format` the text forms, and
//! `parse` the reader of `Date.parse` (`iso_variants` and `written` read
//! the forms beyond the date time string format). The built-in objects
//! are in `builtins/date*.rs`.
//!
//! The host sets the clock ([`crate::Runtime::set_clock`]) and the zone
//! ([`crate::Runtime::set_time_zone`]). Without them, `Date.now()` reads
//! the system clock and the local zone is the system zone, found at the
//! first use (so a runtime that never touches a `Date` reads no files).

pub(crate) mod format;
mod iso_variants;
pub(crate) mod parse;
pub(crate) mod time_value;
mod written;
mod zone;
mod zone_names;
mod zone_names_data;

pub use zone::{TimeZone, TimeZoneError};

/// A source of the current time: milliseconds since the epoch (integral).
pub type Clock = Box<dyn Fn() -> f64>;

/// The clock and the time zone of a runtime.
#[derive(Default)]
pub(crate) struct TimeHost {
    zone: Option<TimeZone>,
    clock: Option<Clock>,
}

impl TimeHost {
    pub(crate) fn set_zone(&mut self, zone: TimeZone) {
        self.zone = Some(zone);
    }

    pub(crate) fn set_clock(&mut self, clock: Clock) {
        self.clock = Some(clock);
    }

    /// The local time zone: the one the host set, else the zone of the
    /// system (looked up once).
    pub(crate) fn zone(&mut self) -> &TimeZone {
        self.zone.get_or_insert_with(TimeZone::system)
    }

    /// `now` for `Date.now()` and `new Date()`: milliseconds since the
    /// epoch, integral and within the time range (NaN for a host clock
    /// outside it).
    pub(crate) fn now(&self) -> f64 {
        let now = match &self.clock {
            Some(clock) => clock().floor(),
            None => system_now(),
        };
        time_value::time_clip(now)
    }
}

/// The system clock in milliseconds since the epoch (0 before it).
fn system_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => millis(elapsed),
        Err(before) => -millis(before.duration()),
    }
}

/// The whole milliseconds of a duration, without the 128-bit integer of
/// `Duration::as_millis` (its conversion to `f64` is a library call). Exact
/// below 2^53 / 1000 seconds; the clock is far below.
fn millis(duration: std::time::Duration) -> f64 {
    duration.as_secs() as f64 * 1000.0 + f64::from(duration.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_clock_is_floored_and_clipped() {
        let mut host = TimeHost::default();
        host.set_clock(Box::new(|| 1_700_000_000_123.9));
        assert_eq!(host.now(), 1_700_000_000_123.0);
        host.set_clock(Box::new(|| 9e15));
        assert!(host.now().is_nan());
        host.set_clock(Box::new(|| -1.5));
        assert_eq!(host.now(), -2.0);
    }

    #[test]
    fn the_system_clock_is_after_2023() {
        assert!(TimeHost::default().now() > 1_700_000_000_000.0);
    }

    #[test]
    fn durations_give_whole_milliseconds() {
        use std::time::Duration;
        let cases = [
            Duration::new(1_700_000_000, 123_999_999),
            Duration::new(0, 999_999),
            Duration::new(253_402_300_799, 999_000_000),
        ];
        for duration in cases {
            assert_eq!(millis(duration), duration.as_millis() as f64);
        }
    }

    #[test]
    fn the_set_zone_wins_over_the_system_zone() {
        let mut host = TimeHost::default();
        host.set_zone(TimeZone::from_spec("EST5EDT,M3.2.0,M11.1.0").unwrap());
        assert_eq!(host.zone().name(), "EST5EDT,M3.2.0,M11.1.0");
    }
}
