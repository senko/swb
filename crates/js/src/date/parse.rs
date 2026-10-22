//! `Date.parse` (ECMA-262 §21.4.3.2) and the string form of the `Date`
//! constructor: the date time string format (§21.4.1.32,
//! <https://tc39.es/ecma262/#sec-date-time-string-format>) and the two
//! forms that `toString` and `toUTCString` print (§21.4.4.41, §21.4.4.43),
//! which the specification requires `Date.parse` to read back.
//!
//! These forms are exact: a string that is not in one of them gives
//! `None`, which is NaN. The forms that Chromium accepts beyond them
//! (month and weekday names in other orders, slashes, two-digit years,
//! zone abbreviations, lower case letters, more than three fraction
//! digits) are read by [`parse_fallback`], which [`parse`] tries last. The parsers read the text in place, from
//! the start; each stops at the first unit that does not fit, and none
//! reads more than a few dozen units except the name in parentheses at the
//! end of the `toString` form, of which only the first and last units
//! count.

use swb_js_text::Str16;

// The weekday name of the printed forms is read and ignored: the day of
// the week follows from the date.
use super::format::{MONTHS, WEEKDAYS};
use super::time_value::{days_in_month, make_date, make_day, make_time};
use super::{iso_variants, written};

/// What a string means: the time value of the fields (UTC, with an offset
/// in the text already applied), and whether the fields are in local time,
/// so that the caller still applies `UTC(t)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Parsed {
    /// Milliseconds, not clipped; NaN if the numbers overflow.
    pub(crate) time: f64,
    /// The text has no offset and a time: local time (§21.4.1.32 "date-time
    /// forms are local time"). Date-only forms are UTC.
    pub(crate) local: bool,
}

/// [`parse`], and the number of units that the reader of Chromium's other
/// forms scanned (0 when one of the exact forms fits or ends at once): the
/// caller charges them against the time limit.
pub(crate) fn parse_with_cost(text: Str16<'_>) -> (Option<Parsed>, usize) {
    if let Some(parsed) = parse_iso(text)
        .or_else(|| parse_to_string(text))
        .or_else(|| parse_to_utc_string(text))
    {
        return (Some(parsed), 0);
    }
    parse_fallback(text)
}

/// Chromium's other forms (ADR 0027 section 5a): the variants of ISO 8601
/// ([`super::iso_variants`]), then the written forms
/// ([`super::written`]). The second number is the units looked at.
fn parse_fallback(text: Str16<'_>) -> (Option<Parsed>, usize) {
    let (parsed, scanned) = iso_variants::parse(text);
    if parsed.is_some() {
        return (parsed, scanned);
    }
    let (parsed, more) = written::parse(text);
    (parsed, scanned + more)
}

/// A position in the text with the readers of fixed-width pieces.
pub(super) struct Cursor<'a> {
    pub(super) text: Str16<'a>,
    pub(super) at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(text: Str16<'a>) -> Self {
        Cursor { text, at: 0 }
    }

    /// The ASCII character at the position, if it is one.
    fn peek(&self) -> Option<u8> {
        self.text
            .get(self.at)
            .and_then(|unit| u8::try_from(unit).ok())
    }

    pub(super) fn at_end(&self) -> bool {
        self.at >= self.text.len()
    }

    /// Takes the ASCII character `ch` if it is next.
    pub(super) fn eat(&mut self, ch: u8) -> bool {
        if self.peek() == Some(ch) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Whether the ASCII text `word` is at the position `at`.
    fn has_at(&self, at: usize, word: &str) -> bool {
        word.bytes()
            .enumerate()
            .all(|(i, byte)| self.text.get(at + i) == Some(u16::from(byte)))
    }

    /// Takes the ASCII text `word` if it is next.
    fn eat_str(&mut self, word: &str) -> bool {
        let found = self.has_at(self.at, word);
        if found {
            self.at += word.len();
        }
        found
    }

    /// Takes exactly `count` ASCII decimal numerals.
    pub(super) fn numeral(&mut self, count: usize) -> Option<i64> {
        let mut value = 0_i64;
        for i in 0..count {
            let unit = self.text.get(self.at + i)?;
            let figure = unit.checked_sub(u16::from(b'0')).filter(|d| *d < 10)?;
            value = value * 10 + i64::from(figure);
        }
        self.at += count;
        Some(value)
    }

    /// Takes a run of `min` to `max` ASCII decimal numerals (as many as
    /// there are).
    fn numeral_run(&mut self, min: usize, max: usize) -> Option<i64> {
        let length = (0..max)
            .take_while(|i| {
                self.text
                    .get(self.at + i)
                    .is_some_and(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit))
            })
            .count();
        if length < min {
            return None;
        }
        self.numeral(length)
    }

    /// Takes a three-letter name of `names` (its index).
    fn name(&mut self, names: &[&str]) -> Option<usize> {
        let index = names.iter().position(|name| self.has_at(self.at, name))?;
        self.at += 3;
        Some(index)
    }

    /// Takes `+` (1) or `-` (-1).
    pub(super) fn sign(&mut self) -> Option<i64> {
        if self.eat(b'+') {
            Some(1)
        } else if self.eat(b'-') {
            Some(-1)
        } else {
            None
        }
    }
}

/// The time value of the fields and whether they are local.
pub(super) fn time_of(
    (year, month, day): (i64, i64, i64),
    (hour, minute, second, ms): (i64, i64, i64, i64),
    offset_minutes: Option<i64>,
    has_time: bool,
) -> Parsed {
    let days = make_day(year as f64, (month - 1) as f64, day as f64);
    let time = make_time(hour as f64, minute as f64, second as f64, ms as f64);
    let mut value = make_date(days, time);
    if let Some(minutes) = offset_minutes {
        value -= minutes as f64 * 60_000.0;
    }
    Parsed {
        time: value,
        local: has_time && offset_minutes.is_none(),
    }
}

/// The date time string format (§21.4.1.32): `YYYY`, `YYYY-MM` or
/// `YYYY-MM-DD` (the year `YYYY` or `±YYYYYY`), optionally followed by
/// `THH:mm`, `THH:mm:ss` or `THH:mm:ss.sss` and `Z` or `±HH:mm`.
fn parse_iso(text: Str16<'_>) -> Option<Parsed> {
    let mut c = Cursor::new(text);
    let year = if c.eat(b'+') {
        c.numeral(6)?
    } else if c.eat(b'-') {
        // The year -000000 is not valid (§21.4.1.32 "Expanded years").
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
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if c.at_end() {
        return Some(time_of((year, month, day), (0, 0, 0, 0), None, false));
    }
    if !c.eat(b'T') {
        return None;
    }
    let hour = c.numeral(2)?;
    if !c.eat(b':') {
        return None;
    }
    let minute = c.numeral(2)?;
    let (mut second, mut ms) = (0, 0);
    if c.eat(b':') {
        second = c.numeral(2)?;
        if c.eat(b'.') {
            ms = c.numeral(3)?;
        }
    }
    if hour > 24 || minute > 59 || second > 59 || (hour == 24 && (minute, second, ms) != (0, 0, 0))
    {
        return None;
    }
    let offset = if c.eat(b'Z') {
        Some(0)
    } else if let Some(sign) = c.sign() {
        let hours = c.numeral(2)?;
        if !c.eat(b':') {
            return None;
        }
        let minutes = c.numeral(2)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        Some(sign * (hours * 60 + minutes))
    } else {
        None
    };
    if !c.at_end() {
        return None;
    }
    Some(time_of(
        (year, month, day),
        (hour, minute, second, ms),
        offset,
        true,
    ))
}

/// The year of `toString` and `toUTCString` (§21.4.4.41.2): four or more
/// numerals, with a minus sign for a negative year.
fn printed_year(c: &mut Cursor<'_>) -> Option<i64> {
    let negative = c.eat(b'-');
    let year = c.numeral_run(4, 7)?;
    Some(if negative { -year } else { year })
}

/// The clock `HH:mm:ss` of the printed forms.
fn printed_clock(c: &mut Cursor<'_>) -> Option<(i64, i64, i64)> {
    let hour = c.numeral(2)?;
    if !c.eat(b':') {
        return None;
    }
    let minute = c.numeral(2)?;
    if !c.eat(b':') {
        return None;
    }
    let second = c.numeral(2)?;
    (hour < 24 && minute < 60 && second < 60).then_some((hour, minute, second))
}

/// The form of `toString` (§21.4.4.41): `Mon Jul 01 2024 13:05:09
/// GMT+0200 (name)`; the name in parentheses (anything up to the closing
/// parenthesis at the end) is optional.
fn parse_to_string(text: Str16<'_>) -> Option<Parsed> {
    let mut c = Cursor::new(text);
    if c.name(&WEEKDAYS).is_some() && !c.eat(b' ') {
        return None;
    }
    let month = c.name(&MONTHS)? as i64 + 1;
    if !c.eat(b' ') {
        return None;
    }
    let day = c.numeral(2)?;
    if !c.eat(b' ') {
        return None;
    }
    let year = printed_year(&mut c)?;
    if !c.eat(b' ') {
        return None;
    }
    let (hour, minute, second) = printed_clock(&mut c)?;
    if !c.eat_str(" GMT") {
        return None;
    }
    let sign = c.sign()?;
    let hours = c.numeral(2)?;
    let minutes = c.numeral(2)?;
    if minutes > 59 || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if !c.at_end() {
        // " (name)": text up to a closing parenthesis at the very end.
        let closed = c.has_at(c.at, " (")
            && text.len() > c.at + 2
            && text.get(text.len() - 1) == Some(u16::from(b')'));
        if !closed {
            return None;
        }
    }
    Some(time_of(
        (year, month, day),
        (hour, minute, second, 0),
        Some(sign * (hours * 60 + minutes)),
        true,
    ))
}

/// The form of `toUTCString` (§21.4.4.43): `Mon, 01 Jul 2024 11:05:09
/// GMT`.
fn parse_to_utc_string(text: Str16<'_>) -> Option<Parsed> {
    let mut c = Cursor::new(text);
    if c.name(&WEEKDAYS).is_some() && !c.eat_str(", ") {
        return None;
    }
    let day = c.numeral(2)?;
    if !c.eat(b' ') {
        return None;
    }
    let month = c.name(&MONTHS)? as i64 + 1;
    if !c.eat(b' ') {
        return None;
    }
    let year = printed_year(&mut c)?;
    if !c.eat(b' ') {
        return None;
    }
    let (hour, minute, second) = printed_clock(&mut c)?;
    if !c.eat_str(" GMT") || !c.at_end() || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(time_of(
        (year, month, day),
        (hour, minute, second, 0),
        Some(0),
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The meaning in the exact forms.
    fn time(text: &str) -> Option<(f64, bool)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let text = Str16::Wide(&units);
        parse_iso(text)
            .or_else(|| parse_to_string(text))
            .or_else(|| parse_to_utc_string(text))
            .map(|p| (p.time, p.local))
    }

    #[test]
    fn date_only_is_utc() {
        assert_eq!(time("2000"), Some((946_684_800_000.0, false)));
        assert_eq!(time("2000-01"), Some((946_684_800_000.0, false)));
        assert_eq!(time("2000-01-01"), Some((946_684_800_000.0, false)));
        assert_eq!(time("2000-12-31"), Some((978_220_800_000.0, false)));
    }

    #[test]
    fn date_time_without_offset_is_local() {
        assert_eq!(time("2000-01-01T00:00"), Some((946_684_800_000.0, true)));
        assert_eq!(time("2000-01-01T00:00:00.5"), None);
        assert_eq!(
            time("2000-01-01T00:00:00.500"),
            Some((946_684_800_500.0, true))
        );
        assert_eq!(time("2000T00:00"), Some((946_684_800_000.0, true)));
    }

    #[test]
    fn offsets() {
        assert_eq!(time("2000-01-01T00:00Z"), Some((946_684_800_000.0, false)));
        assert_eq!(
            time("2000-01-01T00:00:00+01:00"),
            Some((946_681_200_000.0, false))
        );
        assert_eq!(
            time("2000-01-01T00:00:00-23:59"),
            Some((946_684_800_000.0 + 86_340_000.0, false))
        );
        assert_eq!(time("2000-01-01T00:00:00+24:00"), None);
        assert_eq!(time("2000-01-01T00:00:00+0100"), None);
        assert_eq!(time("2000-01-01T00:00:00+01"), None);
    }

    #[test]
    fn rejected_forms() {
        for text in [
            "",
            "2000-1-1",
            "2000-13-01",
            "2000-00-01",
            "2000-01-00",
            "2000-01-32",
            "2000-02-30",
            "2001-02-29",
            "-000000-01-01",
            "+002000-01-01T24:00:01Z",
            "2000-01-01T24:00:00.001Z",
            "2000-01-01T00:60Z",
            "2000-01-01T00:00:60Z",
            "2000-01-01t00:00Z",
            "2000-01-01 00:00",
            "2000-01-01T00",
            "2000-01-01T00:00Zx",
            "20000101",
            " 2000",
            "+2000-01-01",
            "\u{0662}\u{0660}\u{0660}\u{0660}",
        ] {
            assert_eq!(time(text), None, "{text:?}");
        }
        assert_eq!(time("2000-02-29"), Some((951_782_400_000.0, false)));
    }

    #[test]
    fn expanded_years_and_24_hours() {
        assert_eq!(
            time("+275760-09-13T00:00:00.000Z"),
            Some((8_640_000_000_000_000.0, false))
        );
        assert_eq!(
            time("-271821-04-20T00:00:00.000Z"),
            Some((-8_640_000_000_000_000.0, false))
        );
        assert_eq!(
            time("-000001-01-01T00:00:00Z"),
            Some((-62_198_755_200_000.0, false))
        );
        assert_eq!(
            time("2000-01-01T24:00:00Z"),
            Some((946_771_200_000.0, false))
        );
    }

    #[test]
    fn to_string_form() {
        let expected = Some((1_719_831_909_000.0, false));
        assert_eq!(time("Mon Jul 01 2024 13:05:09 GMT+0200 (CEST)"), expected);
        assert_eq!(
            time("Mon Jul 01 2024 13:05:09 GMT+0200 (Central European Summer Time)"),
            expected
        );
        assert_eq!(time("Mon Jul 01 2024 13:05:09 GMT+0200"), expected);
        assert_eq!(time("Jul 01 2024 13:05:09 GMT+0200"), expected);
        assert_eq!(
            time("Fri Jan 01 -0001 00:00:00 GMT+0000 (UTC)"),
            Some((-62_198_755_200_000.0, false))
        );
        assert_eq!(
            time("Sat Sep 13 275760 00:00:00 GMT+0000"),
            Some((8_640_000_000_000_000.0, false))
        );
        assert_eq!(time("Mon Jul 01 2024 13:05:09 GMT+0200 CEST"), None);
        assert_eq!(time("Mon Jul 01 2024 13:05:09 GMT+0200 (CEST) x"), None);
        assert_eq!(time("Mon Jul 32 2024 13:05:09 GMT+0200"), None);
    }

    #[test]
    fn to_utc_string_form() {
        assert_eq!(
            time("Mon, 01 Jul 2024 11:05:09 GMT"),
            Some((1_719_831_909_000.0, false))
        );
        assert_eq!(
            time("Fri, 31 Dec -0001 23:59:59 GMT"),
            Some((-62_167_219_201_000.0, false))
        );
        assert_eq!(time("Mon, 01 Jul 2024 11:05:09 UTC"), None);
    }

    #[test]
    fn a_long_text_ends_at_the_first_wrong_unit() {
        let long = "x".repeat(10_000_000);
        assert_eq!(time(&long), None);
        let long = format!(
            "Mon Jul 01 2024 13:05:09 GMT+0200 ({})",
            "x".repeat(1_000_000)
        );
        assert_eq!(time(&long), Some((1_719_831_909_000.0, false)));
    }
}
