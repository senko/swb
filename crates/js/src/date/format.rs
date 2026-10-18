//! The text forms of a Date (ECMA-262 §21.4.4.41 `Date.prototype.toString`
//! and its helpers `DateString`, `TimeString` and `TimeZoneString`,
//! §21.4.4.43 `toUTCString`, §21.4.4.36 `toISOString`,
//! <https://tc39.es/ecma262/#sec-todatestring>) and Chromium 148's en-US
//! output of the `toLocale*String` methods (ECMA-402 is outside M7).

use std::fmt::Write as _;

use super::time_value::Fields;

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The names of the weekdays and months in the forms of `toString` and
/// `toUTCString`; the parser (`parse.rs`) reads the same words.
pub(crate) fn day_name(weekday: u8) -> &'static str {
    DAY_NAMES.get(usize::from(weekday)).copied().unwrap_or("")
}

/// See [`day_name`].
pub(crate) fn month_name(month: u8) -> &'static str {
    MONTH_NAMES.get(usize::from(month)).copied().unwrap_or("")
}

/// The month names for the parser, in order.
pub(crate) const MONTHS: [&str; 12] = MONTH_NAMES;

/// The weekday names for the parser, in order.
pub(crate) const WEEKDAYS: [&str; 7] = DAY_NAMES;

/// The year of `DateString` (§21.4.4.41.2): at least four digits, with a
/// minus sign for a negative year.
fn push_year(out: &mut String, year: i64) {
    if year < 0 {
        out.push('-');
    }
    let _ = write!(out, "{:04}", year.unsigned_abs());
}

/// `DateString(tv)` (§21.4.4.41.2): `Mon Jul 01 2024`.
pub(crate) fn date_string(f: &Fields) -> String {
    let mut out = String::with_capacity(16);
    let _ = write!(
        out,
        "{} {} {:02} ",
        day_name(f.weekday),
        month_name(f.month),
        f.day
    );
    push_year(&mut out, f.year);
    out
}

/// `TimeString(tv)` (§21.4.4.41.1): `13:05:09 GMT`.
pub(crate) fn time_string(f: &Fields) -> String {
    format!("{:02}:{:02}:{:02} GMT", f.hour, f.minute, f.second)
}

/// `TimeZoneString(tv)` (§21.4.4.41.3): the offset `+0200` (minutes
/// truncated, as `GMT+0122` for a mean-time offset of 1:22:00) and the
/// name of the zone in parentheses.
pub(crate) fn time_zone_string(offset_ms: i64, name: &str) -> String {
    let sign = if offset_ms >= 0 { '+' } else { '-' };
    let minutes = offset_ms.unsigned_abs() / 60_000;
    format!("{sign}{:02}{:02} ({name})", minutes / 60, minutes % 60)
}

/// `toString` (§21.4.4.41): `Mon Jul 01 2024 13:05:09 GMT+0200 (CEST)`.
/// `local` are the fields of the local time.
pub(crate) fn to_string(local: &Fields, offset_ms: i64, zone_name: &str) -> String {
    format!(
        "{} {}{}",
        date_string(local),
        time_string(local),
        time_zone_string(offset_ms, zone_name)
    )
}

/// `toTimeString` (§21.4.4.42): `13:05:09 GMT+0200 (CEST)`.
pub(crate) fn to_time_string(local: &Fields, offset_ms: i64, zone_name: &str) -> String {
    format!(
        "{}{}",
        time_string(local),
        time_zone_string(offset_ms, zone_name)
    )
}

/// `toUTCString` (§21.4.4.43): `Mon, 01 Jul 2024 11:05:09 GMT`.
pub(crate) fn to_utc_string(f: &Fields) -> String {
    let mut out = String::with_capacity(32);
    let _ = write!(
        out,
        "{}, {:02} {} ",
        day_name(f.weekday),
        f.day,
        month_name(f.month)
    );
    push_year(&mut out, f.year);
    let _ = write!(out, " {:02}:{:02}:{:02} GMT", f.hour, f.minute, f.second);
    out
}

/// `toISOString` (§21.4.4.36): `2024-07-01T11:05:09.007Z`; a year outside
/// 0 to 9999 has a sign and six digits.
pub(crate) fn to_iso_string(f: &Fields) -> String {
    let mut out = String::with_capacity(27);
    if (0..=9999).contains(&f.year) {
        let _ = write!(out, "{:04}", f.year);
    } else {
        let sign = if f.year < 0 { '-' } else { '+' };
        let _ = write!(out, "{sign}{:06}", f.year.unsigned_abs());
    }
    let _ = write!(
        out,
        "-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        f.month + 1,
        f.day,
        f.hour,
        f.minute,
        f.second,
        f.ms
    );
    out
}

/// The year of the en-US calendar: the years before 1 count down from 1
/// (1 BC is the year 0, so the year -1 prints as 2).
fn locale_year(year: i64) -> i64 {
    if year > 0 { year } else { 1 - year }
}

/// `toLocaleDateString()` of Chromium 148 in en-US: `7/1/2024`.
pub(crate) fn to_locale_date_string(f: &Fields) -> String {
    format!("{}/{}/{}", f.month + 1, f.day, locale_year(f.year))
}

/// `toLocaleTimeString()` of Chromium 148 in en-US: `1:05:09 PM` (a plain
/// space before the day period).
pub(crate) fn to_locale_time_string(f: &Fields) -> String {
    let hour = match f.hour % 12 {
        0 => 12,
        h => h,
    };
    let period = if f.hour < 12 { "AM" } else { "PM" };
    format!("{hour}:{:02}:{:02} {period}", f.minute, f.second)
}

/// `toLocaleString()` of Chromium 148 in en-US: `7/1/2024, 1:05:09 PM`.
pub(crate) fn to_locale_string(f: &Fields) -> String {
    format!("{}, {}", to_locale_date_string(f), to_locale_time_string(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::time_value::fields;

    // 2024-07-01T11:05:09.007Z
    const T: i64 = 1_719_831_909_007;

    #[test]
    fn utc_forms() {
        let f = fields(T);
        assert_eq!(to_utc_string(&f), "Mon, 01 Jul 2024 11:05:09 GMT");
        assert_eq!(to_iso_string(&f), "2024-07-01T11:05:09.007Z");
        assert_eq!(date_string(&f), "Mon Jul 01 2024");
    }

    #[test]
    fn local_forms() {
        let local = fields(T + 7_200_000);
        assert_eq!(
            to_string(&local, 7_200_000, "CEST"),
            "Mon Jul 01 2024 13:05:09 GMT+0200 (CEST)"
        );
        assert_eq!(
            to_time_string(&local, -17_762_000, "EST"),
            "13:05:09 GMT-0456 (EST)"
        );
    }

    #[test]
    fn years_outside_four_digits() {
        let f = fields(-62_198_755_200_000);
        assert_eq!(to_utc_string(&f), "Fri, 01 Jan -0001 00:00:00 GMT");
        assert_eq!(to_iso_string(&f), "-000001-01-01T00:00:00.000Z");
        assert_eq!(to_locale_string(&f), "1/1/2, 12:00:00 AM");
        let f = fields(8_640_000_000_000_000);
        assert_eq!(to_utc_string(&f), "Sat, 13 Sep 275760 00:00:00 GMT");
        assert_eq!(to_iso_string(&f), "+275760-09-13T00:00:00.000Z");
        let f = fields(-8_640_000_000_000_000);
        assert_eq!(date_string(&f), "Tue Apr 20 -271821");
        assert_eq!(to_locale_date_string(&f), "4/20/271822");
    }

    #[test]
    fn locale_forms() {
        let f = fields(T + 7_200_000);
        assert_eq!(to_locale_string(&f), "7/1/2024, 1:05:09 PM");
        assert_eq!(to_locale_time_string(&fields(0)), "12:00:00 AM");
        assert_eq!(
            to_locale_time_string(&fields(12 * 3_600_000)),
            "12:00:00 PM"
        );
    }
}
