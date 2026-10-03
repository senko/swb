//! Values of `input` elements: value sanitization and the HTML
//! microsyntaxes of numbers, dates and times.

use swb_dom::{ElementData, is_html_whitespace};

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
        InputType::Range if !is_valid_float(value) => {
            // The default value: the middle of `min` (0) and `max` (100).
            let bound = |name: &str, default: f64| {
                e.attr(name)
                    .filter(|v| is_valid_float(v))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(default)
            };
            let (min, max) = (bound("min", 0.0), bound("max", 100.0));
            let middle = if max < min {
                min
            } else {
                min + (max - min) / 2.0
            };
            format!("{middle}")
        }
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

/// True for a valid floating-point number
/// (<https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#valid-floating-point-number>).
pub(crate) fn is_valid_float(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (mantissa, exponent) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (mantissa, None),
    };
    let digits = |d: &str| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match frac {
        Some(frac) => (int.is_empty() || digits(int)) && digits(frac),
        None => digits(int),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['+', '-']).unwrap_or(e)));
    mantissa_ok && exponent_ok
}

/// Parses an attribute with the rules for parsing non-negative integers;
/// values above `u32::MAX` saturate.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-negative-integers>
pub(crate) fn parse_non_negative(value: &str) -> Option<u32> {
    let value = value.trim_start_matches(is_html_whitespace);
    let value = value.strip_prefix('+').unwrap_or(value);
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let digits = value.get(..end).filter(|d| !d.is_empty())?;
    Some(digits.parse::<u32>().unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_syntax() {
        for valid in ["0", "-1", "1.5", ".5", "1e5", "1E-5", "-0.5e+2"] {
            assert!(is_valid_float(valid), "{valid}");
        }
        for invalid in ["", "-", "1.", "+1", "1e", "e5", "1.2.3", " 1", "0x1"] {
            assert!(!is_valid_float(invalid), "{invalid}");
        }
        assert_eq!(parse_non_negative(" 17px"), Some(17));
        assert_eq!(parse_non_negative("99999999999"), Some(u32::MAX));
        assert_eq!(parse_non_negative("-1"), None);
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
