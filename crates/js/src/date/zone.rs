//! The local time zone (ECMA-262 §21.4.1.25 `LocalTime` and §21.4.1.26
//! `UTC(t)`, <https://tc39.es/ecma262/#sec-localtime>) over the `jiff`
//! library (ADR 0027).
//!
//! A zone answers two questions in integral milliseconds: the offset of a
//! UTC time, and the UTC time of a local time. For a local time that
//! happens twice (the clock goes back) or never (the clock jumps forward),
//! the ES2025 rule is the offset *before* the transition: the first of two
//! instants, and for a skipped time the instant that the offset before the
//! transition gives (§21.4.1.26 with `GetNamedTimeZoneEpochNanoseconds`).
//!
//! `jiff` handles the years -9999 to 9999. A time outside gets the answer
//! of the same calendar position in an earlier or later 400-year era
//! (146,097 days, a whole number of weeks): the rules of a zone depend only
//! on the month, the weekday and the leap year, and before its first
//! transition a zone has a fixed offset (its local mean time), so the
//! answer equals what a rule-based database such as ICU gives for
//! the years beyond 9999 (measured in Chromium 148 for ±275,760 years).

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{AmbiguousOffset, TimeZone as JiffZone};

use super::time_value::{DAYS_PER_ERA, fields};

/// Why a zone specification does not give a zone.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimeZoneError {
    /// The name is not in the time zone database of the system and not a
    /// POSIX `TZ` rule.
    #[error("unknown time zone \"{0}\"")]
    Unknown(String),
}

/// A time zone: the source of the local time of `Date`.
#[derive(Clone, Debug)]
pub struct TimeZone {
    name: String,
    repr: Repr,
}

#[derive(Clone, Debug)]
enum Repr {
    Utc,
    Rules(JiffZone),
}

/// The span of UTC times (in milliseconds) that is safe for `jiff`:
/// -7,600 to 9,890. A time outside moves by whole eras into it.
const SAFE_LOW: i64 = -300_000_000_000_000;
const SAFE_HIGH: i64 = 250_000_000_000_000;
const ERA_MS: i64 = DAYS_PER_ERA * 86_400_000;

/// Moves `t` by whole 400-year eras into `SAFE_LOW..=SAFE_HIGH`.
fn into_safe_span(t: i64) -> i64 {
    if t > SAFE_HIGH {
        t - (t - SAFE_HIGH + ERA_MS - 1) / ERA_MS * ERA_MS
    } else if t < SAFE_LOW {
        t + (SAFE_LOW - t + ERA_MS - 1) / ERA_MS * ERA_MS
    } else {
        t
    }
}

impl TimeZone {
    /// UTC.
    pub fn utc() -> TimeZone {
        TimeZone {
            name: "UTC".to_owned(),
            repr: Repr::Utc,
        }
    }

    /// The zone of the system: the `TZ` variable, else `/etc/localtime`.
    /// When neither gives a zone (no time zone database), the zone is UTC
    /// and a warning says so.
    pub fn system() -> TimeZone {
        match JiffZone::try_system() {
            Ok(zone) => {
                let name = zone
                    .iana_name()
                    .map_or_else(|| "system".to_owned(), str::to_owned);
                log::debug!("time zone of the system: {name}");
                TimeZone {
                    name,
                    repr: Repr::Rules(zone),
                }
            }
            Err(error) => {
                log::warn!("no time zone of the system ({error}); using UTC");
                TimeZone::utc()
            }
        }
    }

    /// The zone of a specification: `UTC`, an IANA name of the database of
    /// the system (`Europe/Zagreb`), or a POSIX `TZ` rule
    /// (`EST5EDT,M3.2.0,M11.1.0`), which needs no database.
    pub fn from_spec(spec: &str) -> Result<TimeZone, TimeZoneError> {
        let spec = spec.trim();
        if spec.eq_ignore_ascii_case("utc") {
            return Ok(TimeZone::utc());
        }
        let zone = JiffZone::get(spec)
            .or_else(|_| JiffZone::posix(spec))
            .map_err(|_| TimeZoneError::Unknown(spec.to_owned()))?;
        Ok(TimeZone {
            name: spec.to_owned(),
            repr: Repr::Rules(zone),
        })
    }

    /// The name or rule that the zone was made from.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The offset of local time from UTC at the UTC time `t`, in
    /// milliseconds (`LocalTZA(t, true)`, §21.4.1.25). `t` is integral.
    pub(crate) fn offset_at_utc(&self, t: i64) -> i64 {
        match &self.repr {
            Repr::Utc => 0,
            Repr::Rules(zone) => {
                let moved = into_safe_span(t);
                match Timestamp::from_millisecond(moved) {
                    Ok(ts) => i64::from(zone.to_offset(ts).seconds()) * 1000,
                    Err(_) => 0,
                }
            }
        }
    }

    /// The name of the zone at the UTC time `t` that `toString` and
    /// `toTimeString` print in parentheses. Chromium prints the long
    /// English name (`Central European Summer Time`); until feature 12b
    /// (ADR 0027) this is the abbreviation of the database (`CEST`,
    /// `EDT`; `+09` for a zone without letters). This is the one place to
    /// replace.
    pub(crate) fn display_name_at_utc(&self, t: i64) -> String {
        match &self.repr {
            Repr::Utc => "UTC".to_owned(),
            Repr::Rules(zone) => match Timestamp::from_millisecond(into_safe_span(t)) {
                Ok(ts) => zone.to_offset_info(ts).abbreviation().to_owned(),
                Err(_) => "UTC".to_owned(),
            },
        }
    }

    /// `UTC(t)` (§21.4.1.26): the UTC time of the local time `local` in
    /// milliseconds. A local time that is repeated gives its first
    /// instant, a skipped one the instant that the offset before the
    /// transition gives.
    pub(crate) fn utc_from_local(&self, local: i64) -> i64 {
        match &self.repr {
            Repr::Utc => local,
            Repr::Rules(zone) => local - offset_of_local(zone, local),
        }
    }
}

/// The offset (in ms) that `UTC(t)` subtracts from the local time.
fn offset_of_local(zone: &JiffZone, local: i64) -> i64 {
    let moved = fields(into_safe_span(local));
    // The fields are in range by construction; a failure means a bug in
    // the era shift and gives the offset 0.
    let Ok(datetime) = DateTime::new(
        moved.year as i16,
        moved.month as i8 + 1,
        moved.day as i8,
        moved.hour as i8,
        moved.minute as i8,
        moved.second as i8,
        i32::from(moved.ms) * 1_000_000,
    ) else {
        return 0;
    };
    let offset = match zone.to_ambiguous_timestamp(datetime).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        AmbiguousOffset::Gap { before, .. } | AmbiguousOffset::Fold { before, .. } => before,
    };
    i64::from(offset.seconds()) * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(spec: &str) -> TimeZone {
        TimeZone::from_spec(spec).expect("a POSIX rule is a zone")
    }

    const NEW_YORK: &str = "EST5EDT,M3.2.0,M11.1.0";
    const HOUR: i64 = 3_600_000;

    #[test]
    fn utc_has_no_offset() {
        let utc = TimeZone::utc();
        assert_eq!(utc.offset_at_utc(123), 0);
        assert_eq!(utc.utc_from_local(123), 123);
        assert_eq!(utc.name(), "UTC");
        assert_eq!(TimeZone::from_spec(" utc ").unwrap().name(), "UTC");
    }

    #[test]
    fn unknown_names_fail() {
        let error = TimeZone::from_spec("No/Such_Zone").unwrap_err();
        assert_eq!(error.to_string(), "unknown time zone \"No/Such_Zone\"");
    }

    #[test]
    fn rule_offsets() {
        let ny = rule(NEW_YORK);
        // 2024-07-01T12:00Z is summer time, 2024-01-01T12:00Z is not.
        assert_eq!(ny.offset_at_utc(1_719_835_200_000), -4 * HOUR);
        assert_eq!(ny.offset_at_utc(1_704_110_400_000), -5 * HOUR);
        assert_eq!(ny.display_name_at_utc(1_719_835_200_000), "EDT");
    }

    #[test]
    fn skipped_and_repeated_local_times() {
        let ny = rule(NEW_YORK);
        // 2024-03-10: 02:00 to 03:00 local does not exist. 02:30 local
        // takes the offset before (-5): 07:30Z, which is 03:30 EDT.
        let gap = 1_710_028_800_000 + 2 * HOUR + HOUR / 2; // 02:30 as local ms
        assert_eq!(ny.utc_from_local(gap), gap + 5 * HOUR);
        // 2024-11-03: 01:00 to 02:00 local happens twice; the first is EDT.
        let fold = 1_730_592_000_000 + HOUR + HOUR / 2; // 01:30 as local ms
        assert_eq!(ny.utc_from_local(fold), fold + 4 * HOUR);
        // Outside the transitions the offset is exact.
        let plain = 1_719_835_200_000;
        assert_eq!(ny.utc_from_local(plain), plain + 4 * HOUR);
    }

    #[test]
    fn eras_beyond_the_database() {
        let ny = rule(NEW_YORK);
        // Year 275760: July is summer time, January is not.
        let july = 8_639_999_999_999_000 - 60 * 86_400_000;
        assert_eq!(ny.offset_at_utc(july).abs(), 4 * HOUR);
        let january = -8_639_999_999_999_000 + 250 * 86_400_000;
        assert_eq!(ny.offset_at_utc(january).abs(), 5 * HOUR);
        // The ends of the time range and a local time beyond them.
        let _ = ny.offset_at_utc(8_640_000_000_000_000);
        let _ = ny.offset_at_utc(-8_640_000_000_000_000);
        let _ = ny.utc_from_local(8_640_000_000_000_000 + 86_400_000);
        let _ = ny.utc_from_local(-8_640_000_000_000_000 - 86_400_000);
    }

    #[test]
    fn era_shift_keeps_the_calendar() {
        assert_eq!(into_safe_span(0), 0);
        assert_eq!(into_safe_span(SAFE_HIGH), SAFE_HIGH);
        let moved = into_safe_span(8_640_000_000_000_000);
        assert!((SAFE_HIGH - ERA_MS..=SAFE_HIGH).contains(&moved));
        assert_eq!((8_640_000_000_000_000 - moved) % ERA_MS, 0);
        let moved = into_safe_span(-8_640_000_000_000_000);
        assert!((SAFE_LOW..SAFE_LOW + ERA_MS).contains(&moved));
        assert_eq!((moved + 8_640_000_000_000_000) % ERA_MS, 0);
    }
}
