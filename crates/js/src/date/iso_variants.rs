//! The variants of ISO 8601 that the date time string format of ECMA-262
//! (§21.4.1.32) does not take, as Chromium 148 reads them (measured; ADR
//! 0027 section 5a). The shape is that of the specification, with these
//! changes:
//!
//! - `t` and `z` in lower case (RFC 3339 section 5.6, NOTE: "the T and Z
//!   characters in this syntax may alternatively be lower case");
//! - a fraction of one digit or more (ISO 8601-1 allows a decimal
//!   fraction of any length; the value is cut to milliseconds, not
//!   rounded);
//! - an offset `+hhmm` or `+hh:mm`;
//! - a day from 1 to 31 in every month (the date rolls over into the next
//!   month);
//!
//! A date alone is UTC, a date with a time but no offset is local time.
//! A date with `Z` and no time (`2000-01-01Z`) is not in this shape: the
//! written forms read it, with their rule for two-digit years.
//! The reader stops at the first unit that does not fit.

use swb_js_text::Str16;

use super::parse::{Cursor, Parsed, time_of};

/// The meaning of `text` in this shape, if it has it.
pub(super) fn parse(text: Str16<'_>) -> (Option<Parsed>, usize) {
    let mut c = Cursor::new(text);
    let parsed = read(&mut c);
    (parsed, c.at + 1)
}

fn read(c: &mut Cursor<'_>) -> Option<Parsed> {
    let year = if c.eat(b'+') {
        c.numeral(6)?
    } else if c.eat(b'-') {
        // -000000 is not a year (§21.4.1.32 "Expanded years").
        let year = c.numeral(6)?;
        if year == 0 {
            return None;
        }
        -year
    } else {
        c.numeral(4)?
    };
    let (mut month, mut day) = (1, 1);
    if c.eat(b'-') {
        month = c.numeral(2)?;
        if c.eat(b'-') {
            day = c.numeral(2)?;
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if c.at_end() {
        // A date alone is UTC; the year is the number as written.
        return Some(time_of((year, month, day), (0, 0, 0, 0), None, false));
    }
    if !(c.eat(b'T') || c.eat(b't')) {
        return None;
    }
    let (clock, offset) = clock_and_offset(c)?;
    Some(time_of((year, month, day), clock, offset, true))
}

/// Takes `Z` or `z`.
fn zulu(c: &mut Cursor<'_>) -> bool {
    c.eat(b'Z') || c.eat(b'z')
}

/// Hour, minute, second and millisecond.
type Clock = (i64, i64, i64, i64);

/// `HH:mm`, `HH:mm:ss` or `HH:mm:ss.f` and the offset, to the end.
fn clock_and_offset(c: &mut Cursor<'_>) -> Option<(Clock, Option<i64>)> {
    let hour = c.numeral(2)?;
    if !c.eat(b':') {
        return None;
    }
    let minute = c.numeral(2)?;
    let (mut second, mut ms) = (0, 0);
    if c.eat(b':') {
        second = c.numeral(2)?;
        if c.eat(b'.') {
            ms = fraction(c)?;
        }
    }
    if hour > 24 || minute > 59 || second > 59 || (hour == 24 && (minute, second, ms) != (0, 0, 0))
    {
        return None;
    }
    let offset = if zulu(c) {
        Some(0)
    } else if let Some(sign) = c.sign() {
        let hours = c.numeral(2)?;
        c.eat(b':');
        let minutes = c.numeral(2)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        Some(sign * (hours * 60 + minutes))
    } else {
        None
    };
    c.at_end().then_some(((hour, minute, second, ms), offset))
}

/// One or more fraction digits as milliseconds: the first three digits,
/// the rest cut off.
fn fraction(c: &mut Cursor<'_>) -> Option<i64> {
    let mut digits = 0;
    let mut ms = 0;
    while let Some(unit) = c.text.get(c.at) {
        let Some(figure) = unit.checked_sub(u16::from(b'0')).filter(|d| *d < 10) else {
            break;
        };
        if digits < 3 {
            ms = ms * 10 + i64::from(figure);
        }
        digits += 1;
        c.at += 1;
    }
    match digits {
        0 => None,
        1 => Some(ms * 100),
        2 => Some(ms * 10),
        _ => Some(ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(text: &str) -> Option<(f64, bool)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        parse(Str16::Wide(&units)).0.map(|p| (p.time, p.local))
    }

    #[test]
    fn lower_case_and_long_fractions() {
        assert_eq!(
            time("2000-01-01t00:00:00.5z"),
            Some((946_684_800_500.0, false))
        );
        assert_eq!(
            time("2000-01-01T00:00:00.123456Z"),
            Some((946_684_800_123.0, false))
        );
        assert_eq!(
            time("2000-01-01T00:00:00.9999999"),
            Some((946_684_800_999.0, true))
        );
        assert_eq!(time("2000-01-01T00:00:00."), None);
    }

    #[test]
    fn offsets_and_days_past_the_month_end() {
        assert_eq!(
            time("2000-01-01T00:00:00+0100"),
            Some((946_681_200_000.0, false))
        );
        assert_eq!(time("2000-01-01T00:00+01"), None);
        assert_eq!(
            time("2000-02-31T12:00:00Z"),
            Some((951_998_400_000.0, false))
        );
        assert_eq!(time("2000-02-32T12:00:00Z"), None);
        // A date alone is UTC, with a day that rolls over.
        assert_eq!(time("2000-02-30"), Some((951_868_800_000.0, false)));
        assert_eq!(time("2000-01-01Z"), None);
        assert_eq!(time("2000-01-01+01:00"), None);
    }

    #[test]
    fn midnight_at_the_end_of_the_day_takes_no_remainder() {
        assert!(time("2000-01-01T24:00:00.000").is_some());
        assert!(time("2000-01-01T24:00:00.001").is_none());
        assert!(time("2000-01-01T24:01").is_none());
    }
}
