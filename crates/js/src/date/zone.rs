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

use std::borrow::Cow;

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{AmbiguousOffset, TimeZone as JiffZone};

use super::time_value::{DAYS_PER_ERA, days_from_civil, fields, is_leap_year, time_within_day};
use super::zone_names::{self, ZoneNames};

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
    /// The names of CLDR for `name`; `None` for a name that CLDR does not
    /// have (a POSIX rule, a file without a name).
    names: Option<ZoneNames>,
    /// The standard offset and the saving of the present rules, in
    /// seconds, for the GMT format of a name that CLDR does not have.
    present: (i32, i32),
    /// For a zone with a negative saving in the database, how ICU sees it.
    rearguard: Option<&'static Rearguard>,
    /// The name is in the time zone database (not a POSIX rule or a file
    /// without a name): a zone that CLDR does not have gets the GMT
    /// format; the others show the abbreviation of their rule.
    iana: bool,
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

/// Two UTC times in 2025, in the middle of January and of July: the
/// present rules of a zone are read from its offsets then.
const PRESENT_SAMPLES: [i64; 2] = [1_736_942_400, 1_752_580_800];

/// A zone that the database lists with a negative saving in some years
/// (the winter time is the saving time: `GMT` in Dublin, `+00` in
/// Morocco, `WAT` in Namibia). ICU has the "rearguard" form: the summer
/// time is the saving time. Between `from` and `until` (UTC milliseconds)
/// the zone is in daylight saving time when its offset is above
/// `threshold` seconds.
#[derive(Debug)]
struct Rearguard {
    zone: &'static str,
    from: i64,
    until: i64,
    threshold: i32,
}

/// The zones with a negative saving. Dublin from 1971-10-31T02:00Z;
/// Morocco always (the database lists a positive saving before 2018,
/// which gives the same); Namibia from 1994-03-20T22:00Z to
/// 2017-10-23T22:00Z (the standard time is +01, the summer time +02).
static REARGUARD_ZONES: [Rearguard; 5] = [
    Rearguard {
        zone: "Europe/Dublin",
        from: 57_722_400_000,
        until: i64::MAX,
        threshold: 0,
    },
    Rearguard {
        zone: "Eire",
        from: 57_722_400_000,
        until: i64::MAX,
        threshold: 0,
    },
    Rearguard {
        zone: "Africa/Casablanca",
        from: i64::MIN,
        until: i64::MAX,
        threshold: 0,
    },
    Rearguard {
        zone: "Africa/El_Aaiun",
        from: i64::MIN,
        until: i64::MAX,
        threshold: 0,
    },
    Rearguard {
        zone: "Africa/Windhoek",
        from: 764_200_800_000,
        until: 1_508_796_000_000,
        threshold: 3600,
    },
];

impl TimeZone {
    /// A zone with its CLDR names and present rules looked up.
    fn new(name: String, repr: Repr, iana: bool) -> TimeZone {
        let lookup = match repr {
            Repr::Utc => "Etc/UTC",
            Repr::Rules(_) => &name,
        };
        let rearguard = REARGUARD_ZONES.iter().find(|entry| entry.zone == name);
        let present = match rearguard {
            // The present rules of Morocco and Ireland, in ICU's form.
            Some(entry) if entry.threshold == 0 => (0, 3600),
            _ => present_rules(&repr),
        };
        TimeZone {
            names: zone_names::find(lookup),
            present,
            rearguard,
            iana,
            name,
            repr,
        }
    }

    /// UTC.
    pub fn utc() -> TimeZone {
        TimeZone::new("UTC".to_owned(), Repr::Utc, true)
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
                let iana = zone.iana_name().is_some();
                TimeZone::new(name, Repr::Rules(zone), iana)
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
        let (zone, iana) = match JiffZone::get(spec) {
            Ok(zone) => (zone, true),
            Err(_) => (
                JiffZone::posix(spec).map_err(|_| TimeZoneError::Unknown(spec.to_owned()))?,
                false,
            ),
        };
        Ok(TimeZone::new(spec.to_owned(), Repr::Rules(zone), iana))
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
    /// `toTimeString` print in parentheses: the long English name of CLDR
    /// (`Central European Summer Time`), or the localized GMT format
    /// (`GMT+01:00`) for a zone and a time that have no such name. A zone
    /// that CLDR does not know (a POSIX rule) shows the abbreviation of
    /// the rule (`EDT`). ADR 0027 section 6.
    pub(crate) fn display_name_at_utc(&self, t: i64) -> Cow<'static, str> {
        let Repr::Rules(zone) = &self.repr else {
            return Cow::Borrowed(
                self.names
                    .and_then(|names| names.long_name(false))
                    .unwrap_or("Coordinated Universal Time"),
            );
        };
        let Ok(ts) = Timestamp::from_millisecond(into_safe_span(t)) else {
            return Cow::Borrowed("Coordinated Universal Time");
        };
        let info = zone.to_offset_info(ts);
        if !self.iana {
            return Cow::Owned(info.abbreviation().to_owned());
        }
        // The saving time is decided at the equivalent time.
        let same = equivalent_time(t);
        let saving = match Timestamp::from_millisecond(same) {
            Ok(ts) if same != t => zone.to_offset_info(ts),
            _ => info,
        };
        let daylight = match self.rearguard {
            Some(entry) if (entry.from..entry.until).contains(&same) => {
                saving.offset().seconds() > entry.threshold
            }
            _ => saving.dst().is_dst(),
        };
        if let Some(name) = self.names.and_then(|names| names.long_name(daylight)) {
            return Cow::Borrowed(name);
        }
        // The GMT format of the standard offset of the present rules plus
        // their saving, as ICU does for a zone that has no name of this
        // kind.
        let (standard, saving) = self.present;
        Cow::Owned(zone_names::gmt_format(
            standard + if daylight { saving } else { 0 },
        ))
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

/// The standard offset and the saving of the present rules of a zone, in
/// seconds: the lower of the two offsets of `PRESENT_SAMPLES` and the
/// difference (0 for a zone without saving time).
fn present_rules(repr: &Repr) -> (i32, i32) {
    let Repr::Rules(zone) = repr else {
        return (0, 0);
    };
    let offset =
        |seconds: i64| Timestamp::from_second(seconds).map_or(0, |ts| zone.to_offset(ts).seconds());
    let [a, b] = PRESENT_SAMPLES.map(offset);
    (a.min(b), a.abs_diff(b).try_into().unwrap_or(0))
}

/// The first time (ms) after the range in which Chromium decides the
/// saving time of a name by the time itself: 2038-01-19T03:14:08Z, 2^31
/// seconds; the range starts at the epoch. Measured: Australia/Adelaide
/// (no summer time in 1969) is in summer time at -1 ms and in standard
/// time at 0 ms. The end is bracketed by Africa/Casablanca, whose
/// Ramadan of 2037 (from October) is real and whose name in February 2038
/// has the Ramadan of 2027; no zone with rules that differ from year to
/// year has a change near 2038-01-19 to show the exact instant.
const EQUIVALENT_FROM: i64 = 2_147_483_648_000;

/// The year that Chromium 148 uses in place of a year outside the range,
/// for each kind of year: the row is `[common, leap]`, the column the
/// weekday of January 1 (0 is Sunday). Measured: the names of seven zones
/// whose rules differ from year to year (`Africa/Cairo`,
/// `America/Sao_Paulo`, `Asia/Tehran`, `Asia/Amman`, `America/Santiago`,
/// `Pacific/Fiji`, `Africa/Casablanca`) on 122 days of every year from
/// 1900 to 2100. For each of the 133 years outside 1970 to 2037, exactly
/// one year inside, of the same kind, had the same names on all days in
/// all zones (the same year for the years before 1970 and after 2037);
/// ES5.1 §15.9.1.8 allows such a mapping. The table is the result, not a
/// formula.
const EQUIVALENT_YEARS: [[i64; 7]; 2] = [
    [2023, 2035, 2019, 2031, 2015, 2027, 2011],
    [2012, 2024, 2008, 2020, 2032, 2016, 2028],
];

/// The time that Chromium uses to decide whether a zone is in daylight
/// saving time for the name: the time itself from 1970-01-01 to
/// 2038-01-19T03:14:08Z (exclusive); outside, the same month, day and time
/// of day (UTC) in the year of [`EQUIVALENT_YEARS`] that has the same
/// weekday of January 1 and the same leap year.
fn equivalent_time(t: i64) -> i64 {
    if (0..EQUIVALENT_FROM).contains(&t) {
        return t;
    }
    let f = fields(t);
    let first_weekday = (days_from_civil(f.year, 1, 1) + 4).rem_euclid(7);
    let row = usize::from(is_leap_year(f.year));
    let Some(year) = EQUIVALENT_YEARS
        .get(row)
        .and_then(|years| years.get(first_weekday as usize))
    else {
        return t;
    };
    let day = days_from_civil(*year, i64::from(f.month) + 1, i64::from(f.day));
    day * 86_400_000 + time_within_day(t)
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
    fn every_equivalent_year_has_its_kind() {
        for (row, years) in EQUIVALENT_YEARS.iter().enumerate() {
            for (weekday, year) in years.iter().enumerate() {
                assert_eq!(is_leap_year(*year), row == 1, "{year}");
                assert_eq!(
                    (days_from_civil(*year, 1, 1) + 4).rem_euclid(7),
                    weekday as i64
                );
                assert!((2008..=2035).contains(year));
            }
        }
    }

    #[test]
    fn the_equivalent_time_keeps_the_calendar_position() {
        // Inside the range the time itself; the range is 1970-01-01 to
        // 2038-01-19T03:14:08Z (exclusive).
        for t in [0, 1, 1_700_000_000_000, EQUIVALENT_FROM - 1] {
            assert_eq!(equivalent_time(t), t);
        }
        // 2038-01-19T03:14:08Z: January 1, 2038 is a Friday of a common
        // year, so the year is 2027.
        let t = EQUIVALENT_FROM;
        let same = equivalent_time(t);
        assert_eq!(fields(same).year, 2027);
        assert_eq!((fields(same).month, fields(same).day), (0, 19));
        assert_eq!(time_within_day(same), time_within_day(t));
        // -1 ms: 1969-12-31T23:59:59.999Z, a Wednesday start of a common
        // year, so 2031; February 29 of a leap year stays February 29.
        assert_eq!(fields(equivalent_time(-1)).year, 2031);
        let leap_day = days_from_civil(1928, 2, 29) * 86_400_000;
        let moved = fields(equivalent_time(leap_day));
        assert_eq!((moved.year, moved.month, moved.day), (2012, 1, 29));
    }

    #[test]
    fn names_before_1970_use_the_equivalent_year() {
        // Measured in Chromium 148 (the group `names-years`).
        let name = |zone: &str, t: i64| {
            let zone = TimeZone::from_spec(zone).expect("the zone is in the database");
            zone.display_name_at_utc(t).into_owned()
        };
        let july = |year: i64| days_from_civil(year, 7, 15) * 86_400_000;
        assert_eq!(
            name("Africa/Cairo", july(1000)),
            "Eastern European Summer Time"
        );
        assert_eq!(
            name("Africa/Algiers", july(1945)),
            "Central European Standard Time"
        );
    }

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
        assert_eq!(
            TimeZone::utc().display_name_at_utc(0),
            "Coordinated Universal Time"
        );
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
