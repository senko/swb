//! Values of `input` elements: value sanitization and the HTML
//! microsyntaxes of numbers, dates and times.

use std::fmt::Write;

use swb_dom::{ElementData, is_html_whitespace, is_valid_float};

use super::{DateType, InputType};

/// The value sanitization algorithm of input type `ty` of element `e`.
/// <https://html.spec.whatwg.org/multipage/input.html#value-sanitization-algorithm>
pub(super) fn sanitize(e: &ElementData, ty: InputType, value: &str) -> String {
    match ty {
        InputType::Text | InputType::Search | InputType::Tel | InputType::Password => {
            value.replace(['\n', '\r'], "")
        }
        InputType::Url | InputType::Email => value
            .replace(['\n', '\r'], "")
            .trim_matches(is_html_whitespace)
            .to_owned(),
        InputType::Number if !is_valid_float(value) => String::new(),
        InputType::Range => sanitize_range(e, value),
        InputType::Date(kind) => {
            let value = value.replace(['\n', '\r'], "");
            normalized_date(kind, &value).unwrap_or_default()
        }
        InputType::Color => {
            let valid = value.len() == 7
                && value.starts_with('#')
                && value.bytes().skip(1).all(|b| b.is_ascii_hexdigit());
            if valid {
                value.to_ascii_lowercase()
            } else {
                "#000000".to_owned()
            }
        }
        _ => value.to_owned(),
    }
}

/// The value sanitization of `type=range`: an invalid value becomes the
/// default (the middle of the range); the value is clamped to `min` and
/// `max`, rounded to the nearest step (`step` default 1, from `min`) and
/// written as a number (`10.0` gives `10`; measured in Chromium).
/// <https://html.spec.whatwg.org/multipage/input.html#range-state-(type=range)>
fn sanitize_range(e: &ElementData, value: &str) -> String {
    let bound = |name: &str, default: f64| {
        e.attr(name)
            .filter(|v| is_valid_float(v))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(default)
    };
    let min = bound("min", 0.0);
    let max = bound("max", 100.0).max(min);
    let parsed = is_valid_float(value)
        .then(|| value.parse::<f64>().ok())
        .flatten();
    let number = parsed.unwrap_or(min + (max - min) / 2.0);
    let step = match e.attr("step") {
        Some(s) if s.eq_ignore_ascii_case("any") => None,
        Some(s) if is_valid_float(s) => s.parse::<f64>().ok().filter(|&v| v > 0.0).or(Some(1.0)),
        _ => Some(1.0),
    };
    let mut result = number.clamp(min, max);
    if let Some(step) = step {
        let steps = (result - min) / step;
        if (steps - steps.round()).abs() > 1e-9 {
            // The nearest step; the higher one if two are equally near.
            let mut n = (steps + 0.5).floor();
            while min + n * step > max && n > 0.0 {
                n -= 1.0;
            }
            result = min + n * step;
        }
    }
    format_range(result)
}

/// A range value as a number string (no `-0`).
fn format_range(number: f64) -> String {
    format!("{}", if number == 0.0 { 0.0 } else { number })
}

/// The latest date of date and time inputs: the end of the ECMAScript time
/// range (275760-09-13T00:00Z). Deviation: the specification has no
/// maximum; Chromium (measured) treats later values as invalid, and so
/// does swb. This also keeps the date arithmetic small.
const MAX_DATE: (u32, u32, u32) = (275_760, 9, 13);

/// The last week that starts before [`MAX_DATE`] (measured in Chromium).
const MAX_WEEK: (u32, u32) = (275_760, 37);

/// The value of a date or time input, if it is valid for its type (with
/// a `datetime-local` value normalized: `T` between date and time, the
/// shortest time).
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#dates-and-times>
fn normalized_date(kind: DateType, value: &str) -> Option<String> {
    let valid = match kind {
        DateType::Date => parse_date(value).is_some(),
        DateType::Month => parse_month(value).is_some(),
        DateType::Week => is_valid_week(value),
        DateType::Time => is_valid_time(value),
        DateType::DateTimeLocal => {
            let (date, time) = value.split_once(['T', ' '])?;
            let day = parse_date(date)?;
            if !is_valid_time(time) {
                return None;
            }
            let time = shortest_time(time);
            // The range ends at midnight of the last day.
            if day == MAX_DATE && time != "00:00" {
                return None;
            }
            return Some(format!("{date}T{time}"));
        }
    };
    valid.then(|| value.to_owned())
}

/// Parses `n` or more ASCII digits (exactly `n` if `exact`).
fn number(s: &str, n: usize, exact: bool) -> Option<u32> {
    let ok = s.len() >= n && (!exact || s.len() == n) && s.bytes().all(|b| b.is_ascii_digit());
    if !ok {
        return None;
    }
    s.parse().ok()
}

/// A year of a date or time value: 4 or more digits, from 1 to the year
/// of [`MAX_DATE`].
fn year(s: &str) -> Option<u32> {
    number(s, 4, false).filter(|y| (1..=MAX_DATE.0).contains(y))
}

/// A valid month string `YYYY-MM`: the year and the month.
fn parse_month(s: &str) -> Option<(u32, u32)> {
    let (year, month) = s.rsplit_once('-')?;
    let year = self::year(year)?;
    let month = number(month, 2, true).filter(|m| (1..=12).contains(m))?;
    ((year, month) <= (MAX_DATE.0, MAX_DATE.1)).then_some((year, month))
}

/// A valid date string `YYYY-MM-DD`.
fn parse_date(s: &str) -> Option<(u32, u32, u32)> {
    let (month, day) = s.rsplit_once('-')?;
    let (year, month) = parse_month(month)?;
    let day = number(day, 2, true).filter(|&d| d >= 1 && d <= days_in_month(year, month))?;
    ((year, month, day) <= MAX_DATE).then_some((year, month, day))
}

fn is_leap(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// A valid week string `YYYY-Www`; week 53 only in years that have it.
fn is_valid_week(s: &str) -> bool {
    let Some((year, week)) = s.split_once("-W") else {
        return false;
    };
    let (Some(year), Some(week)) = (self::year(year), number(week, 2, true)) else {
        return false;
    };
    // A year has 53 weeks if it starts on a Thursday, or on a Wednesday in
    // a leap year.
    let jan1 = weekday(year, 1, 1);
    let weeks = if jan1 == 4 || (jan1 == 3 && is_leap(year)) {
        53
    } else {
        52
    };
    (1..=weeks).contains(&week) && (year, week) <= MAX_WEEK
}

/// The day of the week (0 is Sunday) of a date in the proleptic Gregorian
/// calendar (Sakamoto's method). The arithmetic is in `u64` and saturates,
/// so that no year can overflow it.
fn weekday(year: u32, month: u32, day: u32) -> u64 {
    const T: [u64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let year = u64::from(year);
    let y = if month < 3 {
        year.saturating_sub(1)
    } else {
        year
    };
    let t = T
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(0);
    (y + y / 4 - y / 100 + y / 400 + t + u64::from(day)) % 7
}

/// A valid time string `HH:MM`, `HH:MM:SS` or `HH:MM:SS.s` (1 to 3
/// fraction digits).
fn is_valid_time(s: &str) -> bool {
    let mut parts = s.splitn(3, ':');
    let hour = parts.next().and_then(|h| number(h, 2, true));
    let minute = parts.next().and_then(|m| number(m, 2, true));
    let valid_hm = hour.is_some_and(|h| h < 24) && minute.is_some_and(|m| m < 60);
    match parts.next() {
        None => valid_hm,
        Some(rest) => {
            let (seconds, fraction) = match rest.split_once('.') {
                Some((s, f)) => (s, Some(f)),
                None => (rest, None),
            };
            valid_hm
                && number(seconds, 2, true).is_some_and(|s| s < 60)
                && fraction
                    .is_none_or(|f| (1..=3).contains(&f.len()) && number(f, 1, false).is_some())
        }
    }
}

/// A valid time in its shortest form: without zero seconds and without
/// trailing zeros of the fraction.
fn shortest_time(time: &str) -> String {
    let (hm, rest) = time.split_at(time.len().min(5));
    let seconds = rest.strip_prefix(':').unwrap_or("");
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let fraction = fraction.trim_end_matches('0');
    match (whole, fraction) {
        ("" | "00", "") => hm.to_owned(),
        (whole, "") => format!("{hm}:{whole}"),
        (whole, fraction) => format!("{hm}:{whole}.{fraction}"),
    }
}

// ----- Numbers of dates and times (constraint validation) -----

/// Days since 1970-01-01 of a date of the proleptic Gregorian calendar.
/// This and [`civil_from_days`] are Howard Hinnant's algorithms
/// (<https://howardhinnant.github.io/date_algorithms.html>, docs/credits.md).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The Monday of week 1 of a week-year (the week with January 4th), in
/// days since 1970-01-01.
fn first_monday(year: u32) -> i64 {
    let jan4 = days_from_civil(i64::from(year), 1, 4);
    // `weekday` counts from Sunday: Monday is 1.
    jan4 - ((weekday(year, 1, 4) as i64) + 6) % 7
}

/// The seconds of a valid time string (`HH:MM`, `HH:MM:SS`, `HH:MM:SS.s`).
fn time_seconds(s: &str) -> Option<f64> {
    if !is_valid_time(s) {
        return None;
    }
    let mut parts = s.splitn(3, ':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = match parts.next() {
        Some(rest) => rest.parse().ok()?,
        None => 0.0,
    };
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// The value of a date or time string of type `kind` as a number in the
/// unit that the `step` attribute counts: days (`date`), months since
/// 1970-01 (`month`), weeks since the week of 1970-01-01 (`week`), seconds
/// (`time`: since midnight; `datetime-local`: since 1970-01-01T00:00).
/// `None` if the string is not valid for the type.
/// <https://html.spec.whatwg.org/multipage/input.html#concept-input-value-string-number>
pub(super) fn native_value(kind: DateType, s: &str) -> Option<f64> {
    match kind {
        DateType::Date => {
            let (y, m, d) = parse_date(s)?;
            Some(days_from_civil(i64::from(y), m, d) as f64)
        }
        DateType::Month => {
            let (y, m) = parse_month(s)?;
            Some((i64::from(y) - 1970) as f64 * 12.0 + f64::from(m) - 1.0)
        }
        DateType::Week => {
            if !is_valid_week(s) {
                return None;
            }
            let (y, w) = s.split_once("-W")?;
            let (y, w): (u32, i64) = (y.parse().ok()?, w.parse().ok()?);
            let monday = first_monday(y) + (w - 1) * 7;
            Some(((monday + 3) / 7) as f64)
        }
        DateType::Time => time_seconds(s),
        DateType::DateTimeLocal => {
            let (date, time) = s.split_once(['T', ' '])?;
            let (y, m, d) = parse_date(date)?;
            let seconds = time_seconds(time)?;
            Some(days_from_civil(i64::from(y), m, d) as f64 * 86_400.0 + seconds)
        }
    }
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A time of day as Chromium shows it in English (`9:00 AM`, `9:00:30 AM`,
/// `9:00:30.500 AM`, with U+202F before the marker). Seconds show when
/// they are not zero or `seconds` is true; milliseconds show when they are
/// not zero.
fn format_time(seconds_of_day: f64, seconds: bool) -> String {
    let total_ms = (seconds_of_day * 1000.0).round() as i64;
    let total_ms = total_ms.rem_euclid(86_400_000);
    let (hours, minutes) = (total_ms / 3_600_000, total_ms / 60_000 % 60);
    let (secs, ms) = (total_ms / 1000 % 60, total_ms % 1000);
    let hour12 = match hours % 12 {
        0 => 12,
        h => h,
    };
    let mut out = format!("{hour12}:{minutes:02}");
    if seconds || secs != 0 || ms != 0 {
        let _ = write!(out, ":{secs:02}");
    }
    if ms != 0 {
        let _ = write!(out, ".{ms:03}");
    }
    // ICU's English pattern has a narrow no-break space before the marker.
    out.push_str(if hours < 12 {
        "\u{202f}AM"
    } else {
        "\u{202f}PM"
    });
    out
}

/// A date as Chromium shows it in English (`01/31/2021`).
fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{m:02}/{d:02}/{y:04}")
}

/// The value `x` (as [`native_value`] gives it) of a date or time type as
/// Chromium shows it in messages, in English: `01/31/2021`, `January 2021`,
/// `Week 05, 2021`, `9:00 AM`, `01/31/2021, 9:00 AM`. `seconds` forces the
/// seconds of times.
pub(super) fn format_native(kind: DateType, x: f64, seconds: bool) -> String {
    match kind {
        DateType::Date => format_date(x.round() as i64),
        DateType::Month => {
            let months = x.round() as i64;
            let (year, month) = (1970 + months.div_euclid(12), months.rem_euclid(12));
            let name = MONTH_NAMES.get(month as usize).copied().unwrap_or("");
            format!("{name} {year}")
        }
        DateType::Week => {
            // The Thursday of the week decides its week-year.
            let thursday = x.round() as i64 * 7;
            let (year, _, _) = civil_from_days(thursday);
            let week = (thursday - days_from_civil(year, 1, 1)) / 7 + 1;
            format!("Week {week:02}, {year}")
        }
        DateType::Time => format_time(x, seconds),
        DateType::DateTimeLocal => {
            let days = (x / 86_400.0).floor();
            let rest = x - days * 86_400.0;
            format!(
                "{}, {}",
                format_date(days as i64),
                format_time(rest, seconds)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_values_and_names() {
        let n = |kind, s| native_value(kind, s);
        assert_eq!(n(DateType::Date, "1970-01-02"), Some(1.0));
        assert_eq!(n(DateType::Date, "2020-01-02"), Some(18_263.0));
        assert_eq!(n(DateType::Month, "1970-03"), Some(2.0));
        assert_eq!(n(DateType::Week, "1970-W01"), Some(0.0));
        assert_eq!(n(DateType::Week, "1970-W03"), Some(2.0));
        assert_eq!(n(DateType::Time, "10:30:15.5"), Some(37_815.5));
        assert_eq!(
            n(DateType::DateTimeLocal, "1970-01-02T00:01"),
            Some(86_460.0)
        );
        assert_eq!(n(DateType::Date, "2023-02-29"), None);
        assert_eq!(format_native(DateType::Date, 18_263.0, false), "01/02/2020");
        assert_eq!(format_native(DateType::Month, 603.0, false), "April 2020");
        assert_eq!(
            format_native(
                DateType::Week,
                n(DateType::Week, "2021-W01").unwrap(),
                false
            ),
            "Week 01, 2021"
        );
        assert_eq!(
            format_native(
                DateType::Week,
                n(DateType::Week, "2020-W53").unwrap(),
                false
            ),
            "Week 53, 2020"
        );
        assert_eq!(format_native(DateType::Time, 0.0, false), "12:00\u{202f}AM");
        assert_eq!(
            format_native(DateType::Time, 46_800.0, false),
            "1:00\u{202f}PM"
        );
        assert_eq!(
            format_native(DateType::Time, 32_430.5, false),
            "9:00:30.500\u{202f}AM"
        );
        assert_eq!(
            format_native(DateType::Time, 32_400.0, true),
            "9:00:00\u{202f}AM"
        );
        assert_eq!(
            format_native(DateType::DateTimeLocal, 86_460.0, false),
            "01/02/1970, 12:01\u{202f}AM"
        );
    }

    #[test]
    fn dates_and_times() {
        let date = |kind, v: &str| normalized_date(kind, v);
        assert_eq!(
            date(DateType::Date, "2024-02-29").as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(date(DateType::Date, "2023-02-29"), None);
        assert_eq!(date(DateType::Date, "24-01-01"), None);
        assert_eq!(date(DateType::Month, "2024-13"), None);
        assert_eq!(
            date(DateType::Month, "12024-01").as_deref(),
            Some("12024-01")
        );
        assert_eq!(
            date(DateType::Week, "2020-W53").as_deref(),
            Some("2020-W53")
        );
        assert_eq!(date(DateType::Week, "2021-W53"), None);
        assert_eq!(
            date(DateType::Time, "23:59:59.999").as_deref(),
            Some("23:59:59.999")
        );
        assert_eq!(date(DateType::Time, "24:00"), None);
        assert_eq!(date(DateType::Time, "12:00:00.1234"), None);
        assert_eq!(
            date(DateType::DateTimeLocal, "2024-01-02 03:04:00.500").as_deref(),
            Some("2024-01-02T03:04:00.5")
        );
        assert_eq!(
            date(DateType::DateTimeLocal, "2024-01-02T03:04:00").as_deref(),
            Some("2024-01-02T03:04")
        );
        assert_eq!(date(DateType::DateTimeLocal, "2024-01-02"), None);
    }

    /// Years 0 and above the maximum are invalid, and huge numbers do not
    /// overflow (the limits are Chromium's, measured).
    #[test]
    fn date_and_time_limits() {
        let valid = [
            (DateType::Week, "0001-W01"),
            (DateType::Week, "275760-W37"),
            (DateType::Date, "0001-01-01"),
            (DateType::Date, "275760-09-13"),
            (DateType::Month, "275760-09"),
            (DateType::DateTimeLocal, "275760-09-12T23:59"),
            (DateType::DateTimeLocal, "275760-09-13T00:00"),
        ];
        for (kind, value) in valid {
            assert_eq!(normalized_date(kind, value).as_deref(), Some(value));
        }
        let invalid = [
            (DateType::Week, "0000-W01"),
            (DateType::Week, "4000000000-W01"),
            (DateType::Week, "99999999999-W01"),
            (DateType::Week, "275760-W38"),
            (DateType::Date, "0000-01-01"),
            (DateType::Date, "275760-09-14"),
            (DateType::Date, "4294967295-12-31"),
            (DateType::Month, "275760-10"),
            (DateType::Month, "99999999999-01"),
            (DateType::DateTimeLocal, "275760-09-13T00:01"),
            (DateType::DateTimeLocal, "275760-09-13T00:00:00.001"),
            (DateType::DateTimeLocal, "4000000000-01-01T00:00"),
            (DateType::Time, "99:99"),
            (DateType::Time, "99999999999:00"),
        ];
        for (kind, value) in invalid {
            assert_eq!(normalized_date(kind, value), None, "{value}");
        }
        // 2024-01-01 is a Monday, 2000-01-01 a Saturday; extreme years do
        // not overflow.
        assert_eq!(weekday(2024, 1, 1), 1);
        assert_eq!(weekday(2000, 1, 1), 6);
        assert!(weekday(u32::MAX, 12, 31) < 7 && weekday(0, 1, 1) < 7);
    }
}
