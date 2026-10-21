//! The written forms of dates that `Date.parse` reads beyond the date time
//! string format: the date of RFC 5322 and its obsolete syntax, RFC 850,
//! C `asctime`, the English and the numeric forms of everyday text, and
//! the output of `toLocale*String` in en-US. ADR 0027 section 5a lists the
//! forms with their sources; every rule below was measured in Chromium 148
//! (the probe snippets are the groups `parse-fallback*` of
//! `crates/js/tests/data/date-snippets.txt`).
//!
//! The text is cut into tokens in one pass (numbers, words, single
//! punctuation characters; white space and comments in parentheses only
//! separate), and the first 32 tokens after some free text are matched
//! against the forms:
//!
//! ```text
//! text   = [free text] date [clock] [zone] [year] [zone]
//!        | clock [zone] date [zone]
//! date   = month day [year]  |  day month [year]  |  year month [day]
//!        | m "/" d [ "/" y ]  |  y "/" m [ "/" d ]
//! clock  = [","] h ":" mm [ ":" ss [ "." fraction ] ] [ "AM" | "PM" ]
//! zone   = "Z" | "UT" | "UTC" | "GMT" [ offset ] | "EST" ... "PDT" | offset
//! ```
//!
//! A month is a word whose first three letters name it; the separators
//! of a date are white space, `,`, `-`, `/` or `.`; the free text is words
//! other than month names (weekday names, "Date:", also zone names and
//! `AM`) and `,`, `.`, `:`, `;`, `)`; a word is a run of ASCII letters and
//! non-ASCII units that are not white space; a zone offset alone needs
//! a clock before it; the year after the clock is for `asctime`. A text
//! without a zone is local time. No token buffer or state grows with the
//! text: after the first 32 tokens the text is rejected.

use swb_js_text::Str16;

use super::parse::Parsed;
use super::time_value::{make_date, make_day, make_time};

/// Tokens kept after the free text. A full date with clock, fraction,
/// AM/PM and an offset with a colon has 21.
const MAX_TOKENS: usize = 32;

/// The year of a date without one (`Jan 1`), as measured.
const DEFAULT_YEAR: i64 = 2001;

/// Numbers stop growing at the largest `u32`, so that a long run of digits
/// cannot overflow. Every field that a form accepts is far below it (the
/// largest year has six digits).
const NUMBER_CAP: u64 = 0xFFFF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A run of ASCII digits: its value (capped), its length, and the
    /// milliseconds that it means as a fraction (first three digits).
    Num {
        value: u32,
        digits: u32,
        ms: u16,
    },
    /// A run of ASCII letters and of non-ASCII units that are not white
    /// space (measured: `ĀJul` is one word, not a month, and `Ā1` is a
    /// word that touches a number): the first three units (ASCII letters
    /// in lower case, other units as `?`) and the length.
    Word {
        head: [u8; 3],
        len: usize,
    },
    /// One ASCII character that is not a digit, a letter or white space.
    Punct(u8),
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tok {
    kind: Kind,
    /// White space or a comment came before.
    gap: bool,
    /// The first thing before was a comment, not white space (`5:06(x)`).
    glued: bool,
}

const END: Tok = Tok {
    kind: Kind::End,
    gap: false,
    glued: false,
};

/// White space between tokens: the white space and line terminators of
/// ECMAScript except U+2028 and U+2029 (measured).
fn is_space(unit: u16) -> bool {
    matches!(
        unit,
        0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000
            ..=0x200A | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

struct Lexer<'a> {
    text: Str16<'a>,
    at: usize,
    /// Units looked at, for the cost.
    work: usize,
}

impl Lexer<'_> {
    /// The next token. NUL ends the text (measured).
    fn next(&mut self) -> Tok {
        let mut gap = false;
        let mut glued = false;
        let unit = loop {
            let Some(unit) = self.text.get(self.at) else {
                return Tok { gap, glued, ..END };
            };
            self.work += 1;
            if unit == 0 {
                self.at = self.text.len();
                return Tok { gap, glued, ..END };
            }
            if is_space(unit) {
                self.at += 1;
                gap = true;
            } else if unit == u16::from(b'(') {
                glued |= !gap;
                self.skip_comment();
                gap = true;
            } else {
                break unit;
            }
        };
        let kind = match u8::try_from(unit) {
            Ok(b'0'..=b'9') => self.number(),
            Ok(byte) if byte.is_ascii_alphabetic() => self.word(),
            Ok(byte) if byte.is_ascii() => {
                self.at += 1;
                Kind::Punct(byte)
            }
            _ => self.word(),
        };
        Tok { kind, gap, glued }
    }

    /// A comment (RFC 5322 section 3.2.2): nested parentheses; one that is
    /// not closed runs to the end.
    fn skip_comment(&mut self) {
        let mut depth = 0_usize;
        while let Some(unit) = self.text.get(self.at) {
            self.work += 1;
            if unit == 0 {
                self.at = self.text.len();
                return;
            }
            self.at += 1;
            if unit == u16::from(b'(') {
                depth += 1;
            } else if unit == u16::from(b')') {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return;
                }
            }
        }
    }

    fn number(&mut self) -> Kind {
        let (mut value, mut digits, mut lead, mut lead_digits) = (0_u64, 0_u32, 0_u16, 0_u32);
        while let Some(unit) = self.text.get(self.at) {
            let Some(figure) = unit.checked_sub(u16::from(b'0')).filter(|d| *d < 10) else {
                break;
            };
            self.work += 1;
            self.at += 1;
            value = (value * 10 + u64::from(figure)).min(NUMBER_CAP);
            digits = digits.saturating_add(1);
            if lead_digits < 3 {
                lead = lead * 10 + figure;
                lead_digits += 1;
            }
        }
        let ms = match lead_digits {
            1 => lead * 100,
            2 => lead * 10,
            _ => lead,
        };
        Kind::Num {
            value: value as u32,
            digits,
            ms,
        }
    }

    fn word(&mut self) -> Kind {
        let mut head = [0_u8; 3];
        let mut len = 0_usize;
        while let Some(unit) = self.text.get(self.at) {
            let letter = match u8::try_from(unit) {
                Ok(byte) if byte.is_ascii_alphabetic() => byte.to_ascii_lowercase(),
                Ok(byte) if byte.is_ascii() => break,
                _ if is_space(unit) => break,
                _ => b'?',
            };
            self.work += 1;
            self.at += 1;
            if let Some(slot) = head.get_mut(len) {
                *slot = letter;
            }
            len += 1;
        }
        Kind::Word { head, len }
    }
}

const MONTHS: [&[u8; 3]; 12] = [
    b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov", b"dec",
];

/// What a word means.
#[derive(Clone, Copy, PartialEq)]
enum Word {
    /// A month (1 to 12): any word that starts with its three letters.
    Month(i64),
    /// `Z`, `UT`, `UTC`, `GMT`: an offset of 0 that an offset may follow.
    Universal,
    /// The abbreviation of a North American zone (RFC 5322 section 4.3):
    /// minutes east of UTC.
    Zone(i64),
    Am,
    Pm,
    /// A word that no form uses (a weekday name, `on`): free text
    /// before the date, an error after it.
    Free,
}

fn classify(head: [u8; 3], len: usize) -> Word {
    if len >= 3
        && let Some(index) = MONTHS.iter().position(|month| **month == head)
    {
        return Word::Month(index as i64 + 1);
    }
    let Some(word) = head.get(..len) else {
        return Word::Free;
    };
    match word {
        b"z" | b"ut" | b"utc" | b"gmt" => Word::Universal,
        b"est" | b"cdt" => Word::Zone(-300),
        b"edt" => Word::Zone(-240),
        b"cst" | b"mdt" => Word::Zone(-360),
        b"mst" | b"pdt" => Word::Zone(-420),
        b"pst" => Word::Zone(-480),
        b"am" => Word::Am,
        b"pm" => Word::Pm,
        _ => Word::Free,
    }
}

/// The tokens after the free text and whether there was free text, or
/// `None` if the text is too long or the free text is not free. Also the
/// units looked at.
#[allow(clippy::type_complexity)]
fn tokens(text: Str16<'_>) -> (Option<([Tok; MAX_TOKENS], usize, Tok, bool)>, usize) {
    let mut lexer = Lexer {
        text,
        at: 0,
        work: 0,
    };
    let mut buffer = [END; MAX_TOKENS];
    let mut count = 0;
    let mut free = true;
    let mut skipped = false;
    let mut after_word = false;
    let end = loop {
        let tok = lexer.next();
        if tok.kind == Kind::End {
            break tok;
        }
        if free {
            match tok.kind {
                // Every word but a month is free text here, also a zone
                // name, `AM`, `PM` and `T` (measured: `GMT Jul 1 2024` and
                // `PM 5:06 Jul 1 2024` are local time, 5:06 in the morning).
                Kind::Word { head, len } if !matches!(classify(head, len), Word::Month(_)) => {
                    after_word = true;
                    skipped = true;
                    continue;
                }
                Kind::Punct(b'.' | b',' | b':' | b';' | b')') => {
                    after_word = false;
                    skipped = true;
                    continue;
                }
                // A word that touches a number is not free text: `x1`.
                Kind::Num { .. } if after_word && !tok.gap => return (None, lexer.work),
                _ => free = false,
            }
        }
        let Some(slot) = buffer.get_mut(count) else {
            return (None, lexer.work);
        };
        *slot = tok;
        count += 1;
    };
    (Some((buffer, count, end, skipped)), lexer.work)
}

/// The fields of a date; the year is absent in `Jan 1`.
struct Date {
    /// The full year, if the text has one.
    year: Option<i64>,
    month: i64,
    day: i64,
}

struct Clock {
    hour: i64,
    minute: i64,
    second: i64,
    ms: i64,
}

struct Reader {
    toks: [Tok; MAX_TOKENS],
    count: usize,
    /// The token after the last: `End`, with a comment before it or not.
    end: Tok,
    /// Free text came before the tokens.
    skipped: bool,
    at: usize,
}

impl Reader {
    fn tok(&self, at: usize) -> Tok {
        if at < self.count {
            self.toks.get(at).copied().unwrap_or(END)
        } else {
            self.end
        }
    }

    /// The token at the position.
    fn peek(&self) -> Tok {
        self.tok(self.at)
    }

    /// Whether the token at `at` is a number that starts a clock (`5:`).
    fn clock_start(&self, at: usize) -> bool {
        matches!(self.tok(at).kind, Kind::Num { .. })
            && self.tok(at + 1).kind == Kind::Punct(b':')
            && !self.tok(at + 1).gap
    }

    /// A number of the date: not the hour of a clock.
    fn date_number(&mut self) -> Option<(u32, u32)> {
        if self.clock_start(self.at) {
            return None;
        }
        let Kind::Num { value, digits, .. } = self.peek().kind else {
            return None;
        };
        self.at += 1;
        Some((value, digits))
    }

    /// Takes the separator of date fields: one of `,` `-` `/` `.`. Returns
    /// whether the next token is separated from the last (by it or by
    /// white space).
    fn separator(&mut self) -> bool {
        let tok = self.peek();
        // A minus sign after white space is a sign, not a separator
        // (`Jan 1 -5`), except after a month name that starts the date
        // (`Jan -1 2000`).
        let sign = tok.kind == Kind::Punct(b'-') && tok.gap && !self.after_month();
        let before_field = match self.tok(self.at + 1).kind {
            Kind::Num { .. } => true,
            Kind::Word { head, len } => matches!(classify(head, len), Word::Month(_)),
            _ => false,
        };
        if matches!(tok.kind, Kind::Punct(b',' | b'-' | b'/' | b'.' | b';'))
            && !sign
            && before_field
        {
            self.at += 1;
            return true;
        }
        tok.gap
    }

    fn after_month(&self) -> bool {
        self.at == 1
            && matches!(self.tok(0).kind, Kind::Word { head, len }
                if matches!(classify(head, len), Word::Month(_)))
    }

    fn month_word(&self) -> Option<i64> {
        match self.peek().kind {
            Kind::Word { head, len } => match classify(head, len) {
                Word::Month(month) => Some(month),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether the token at the position is a zone name (`Z`, `GMT`,
    /// `EST`, ...).
    fn zone_word(&self) -> bool {
        matches!(self.peek().kind, Kind::Word { head, len }
            if matches!(classify(head, len), Word::Universal | Word::Zone(_)))
    }

    /// The date: a month name first, a number first (then a month name or
    /// numbers), or the expanded year of ISO 8601 (`+002024-07-01`).
    fn date(&mut self) -> Option<Date> {
        if let Some(month) = self.month_word() {
            self.at += 1;
            return self.after_month_first(month);
        }
        if let Some(year) = self.expanded_year() {
            let (month, _) = self.date_number()?;
            self.separator();
            let day = self.date_number().map_or(1, |(value, _)| value);
            return Some(Date {
                year: Some(year),
                month: i64::from(month),
                day: i64::from(day),
            });
        }
        let (first, _) = self.date_number()?;
        if first > 31 && (self.zone_word() || self.clock_start(self.at)) {
            // `2024 17:06`, `2024Z`: January 1.
            return Some(Date {
                year: Some(full_year(first)),
                month: 1,
                day: 1,
            });
        }
        if self.month_word().is_none() && !self.separator() {
            return None;
        }
        if let Some(month) = self.month_word() {
            self.at += 1;
            self.separator();
            let second = self.date_number().map(|(value, _)| value);
            // `1 Jul 2024`, `2024 Jul 1`
            return if (1..=31).contains(&first) {
                Some(named(month, i64::from(first), second))
            } else {
                Some(named(month, i64::from(second.unwrap_or(1)), Some(first)))
            };
        }
        let (second, _) = self.date_number()?;
        self.separator();
        let third = self.date_number().map(|(value, _)| value);
        if first > 31 {
            // `2024/07/01`
            return Some(Date {
                year: Some(full_year(first)),
                month: i64::from(second),
                day: i64::from(third.unwrap_or(1)),
            });
        }
        // `7/1/2024`
        Some(Date {
            year: third.map(full_year),
            month: i64::from(first),
            day: i64::from(second),
        })
    }

    /// `+yyyyyy` or `-yyyyyy` at the start of the text and the separator
    /// after it (`-000000` is not a year). A negative year is as written;
    /// a positive one takes the rule for two-digit years (measured:
    /// `+000050 7 1` is 1950, `-000050 7 1` the year -50). After free text,
    /// white space or a comment, Chromium ignores the sign (` -000001 01`
    /// is January 1, 2001): not one of these forms.
    fn expanded_year(&mut self) -> Option<i64> {
        if self.skipped || self.at != 0 || self.peek().gap {
            return None;
        }
        let sign = match self.peek().kind {
            Kind::Punct(b'+') => 1,
            Kind::Punct(b'-') => -1,
            _ => return None,
        };
        let Kind::Num {
            value, digits: 6, ..
        } = self.tok(self.at + 1).kind
        else {
            return None;
        };
        if self.tok(self.at + 1).gap || (sign < 0 && value == 0) || self.clock_start(self.at + 1) {
            return None;
        }
        self.at += 2;
        self.separator();
        Some(if sign > 0 {
            full_year(value)
        } else {
            -i64::from(value)
        })
    }

    /// `Jul 1, 2024`, `Jul 1`, `Jul 2024`.
    fn after_month_first(&mut self, month: i64) -> Option<Date> {
        self.separator();
        let (first, _) = self.date_number()?;
        self.separator();
        let second = self.date_number().map(|(value, _)| value);
        match second {
            Some(year) if (1..=31).contains(&first) => {
                Some(named(month, i64::from(first), Some(year)))
            }
            Some(_) => None,
            None if (1..=31).contains(&first) => Some(named(month, i64::from(first), None)),
            None => Some(named(month, 1, Some(first))),
        }
    }

    /// `[,] h:mm[:ss[.f]] [AM|PM]`
    #[allow(clippy::option_option)] // `None`: a bad clock; `Some(None)`: no clock
    fn clock(&mut self, loose: bool) -> Option<Option<Clock>> {
        let comma = matches!(self.peek().kind, Kind::Punct(b',' | b';'));
        let start = self.at + usize::from(comma);
        if !self.clock_start(start) {
            return Some(None);
        }
        self.at = start;
        let hour = self.clock_number()?;
        self.at += 1; // the colon
        // White space after the colon (`12: 00`) is read only after a date.
        if !loose && self.peek().gap {
            return None;
        }
        let minute = self.clock_number()?;
        let (mut second, mut ms) = (0, 0);
        if self.peek().kind == Kind::Punct(b':') && !self.peek().gap {
            self.at += 1;
            if !loose && self.peek().gap {
                return None;
            }
            second = self.clock_number()?;
            if self.peek().kind == Kind::Punct(b'.') && !self.peek().gap {
                self.at += 1;
                let Kind::Num { ms: fraction, .. } = self.peek().kind else {
                    return None;
                };
                if self.peek().gap {
                    return None;
                }
                self.at += 1;
                ms = i64::from(fraction);
            }
        }
        if minute > 59
            || second > 59
            || hour > 24
            || (hour == 24 && (minute, second, ms) != (0, 0, 0))
        {
            return None;
        }
        let mut hour = hour;
        let mut meridiem_taken = false;
        if let Kind::Word { head, len } = self.peek().kind
            && self.peek().gap
            && let meridiem @ (Word::Am | Word::Pm) = classify(head, len)
        {
            if hour > 12 {
                return None;
            }
            hour = hour % 12 + if meridiem == Word::Pm { 12 } else { 0 };
            self.at += 1;
            meridiem_taken = true;
        }
        // A comment that touches the last number of the clock is not
        // allowed (`5:06(x)`), one after AM or PM is.
        if !meridiem_taken && self.peek().glued {
            return None;
        }
        Some(Some(Clock {
            hour,
            minute,
            second,
            ms,
        }))
    }

    fn clock_number(&mut self) -> Option<i64> {
        let Kind::Num { value, .. } = self.peek().kind else {
            return None;
        };
        self.at += 1;
        Some(i64::from(value))
    }

    /// A zone, if one is next: `Some(Some(minutes east of UTC))`; `None`
    /// is an error (a sign without a valid offset).
    #[allow(clippy::option_option)] // `None`: a bad zone; `Some(None)`: no zone
    fn zone(&mut self, clock: bool) -> Option<Option<i64>> {
        let tok = self.peek();
        match tok.kind {
            // After a clock only `Z` may touch it (`5:06Z`, not `5:06EST`).
            Kind::Word { len, .. } if clock && !tok.gap && len > 1 => Some(None),
            Kind::Word { head, len } => match classify(head, len) {
                Word::Universal => {
                    self.at += 1;
                    if matches!(self.peek().kind, Kind::Punct(b'+' | b'-')) {
                        return self.offset().map(Some);
                    }
                    Some(Some(0))
                }
                Word::Zone(minutes) => {
                    self.at += 1;
                    Some(Some(minutes))
                }
                _ => Some(None),
            },
            Kind::Punct(b'+' | b'-') if clock => self.offset().map(Some),
            _ => Some(None),
        }
    }

    /// `+h`, `+hh`, `+hmm`, `+hhmm`, `+h:mm`, `+hh:mm`.
    fn offset(&mut self) -> Option<i64> {
        let sign = match self.peek().kind {
            Kind::Punct(b'+') => 1,
            Kind::Punct(b'-') => -1,
            _ => return None,
        };
        self.at += 1;
        let Kind::Num { value, digits, .. } = self.peek().kind else {
            return None;
        };
        if self.peek().gap {
            return None;
        }
        self.at += 1;
        let (hours, minutes) = match digits {
            1 | 2 => {
                let colon = self.peek();
                if colon.kind == Kind::Punct(b':') && !colon.gap {
                    self.at += 1;
                    let Kind::Num {
                        value: minutes,
                        digits: 1 | 2,
                        ..
                    } = self.peek().kind
                    else {
                        return None;
                    };
                    self.at += 1;
                    (i64::from(value), i64::from(minutes))
                } else {
                    (i64::from(value), 0)
                }
            }
            3 | 4 => (i64::from(value / 100), i64::from(value % 100)),
            _ => return None,
        };
        (hours <= 23 && minutes <= 59).then_some(sign * (hours * 60 + minutes))
    }
}

fn named(month: i64, day: i64, year: Option<u32>) -> Date {
    Date {
        year: year.map(full_year),
        month,
        day,
    }
}

/// The year of the text: 0 to 49 mean 2000 to 2049, 50 to 99 mean 1950
/// to 1999 (RFC 5322 section 4.3 for two digits; any other number is the
/// year itself, also with three digits, which the RFC maps to 19xx).
fn full_year(year: u32) -> i64 {
    match year {
        0..=49 => i64::from(year) + 2000,
        50..=99 => i64::from(year) + 1900,
        _ => i64::from(year),
    }
}

/// The meaning of `text` in the written forms, and the units looked at.
pub(super) fn parse(text: Str16<'_>) -> (Option<Parsed>, usize) {
    let (found, work) = tokens(text);
    let Some((toks, count, end, skipped)) = found else {
        return (None, work);
    };
    let mut reader = Reader {
        toks,
        count,
        end,
        skipped,
        at: 0,
    };
    (read(&mut reader), work)
}

fn read(reader: &mut Reader) -> Option<Parsed> {
    let (mut date, clock, mut offset);
    if reader.clock_start(0) {
        // `5:06 PM Jan 1 2003`: the clock, a zone, then the date.
        clock = reader.clock(false)?;
        offset = reader.zone(true)?;
        // White space separates the date from the clock or the zone.
        if !reader.peek().gap {
            return None;
        }
        date = reader.date()?;
        if offset.is_none() {
            offset = reader.zone(false)?;
        }
    } else {
        date = reader.date()?;
        clock = reader.clock(true)?;
        offset = reader.zone(clock.is_some())?;
        if date.year.is_none()
            && clock.is_some()
            && !reader.clock_start(reader.at)
            && !reader.peek().glued
            && let Kind::Num { value, .. } = reader.peek().kind
        {
            // `Mon Jul 1 11:05:09 2024`: the year of `asctime` follows the
            // clock.
            reader.at += 1;
            date.year = Some(full_year(value));
            if offset.is_none() {
                offset = reader.zone(true)?;
            }
        }
    }
    if reader.peek().kind != Kind::End {
        return None;
    }
    let year = date.year.unwrap_or(DEFAULT_YEAR);
    if !(1..=12).contains(&date.month) || !(1..=31).contains(&date.day) {
        return None;
    }
    let clock = clock.unwrap_or(Clock {
        hour: 0,
        minute: 0,
        second: 0,
        ms: 0,
    });
    let days = make_day(year as f64, (date.month - 1) as f64, date.day as f64);
    let time = make_time(
        clock.hour as f64,
        clock.minute as f64,
        clock.second as f64,
        clock.ms as f64,
    );
    let mut value = make_date(days, time);
    if let Some(minutes) = offset {
        value -= minutes as f64 * 60_000.0;
    }
    Some(Parsed {
        time: value,
        local: offset.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::time_value::fields;

    fn time(text: &str) -> Option<(f64, bool)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        parse(Str16::Wide(&units)).0.map(|p| (p.time, p.local))
    }

    /// 2024-07-01T11:05:09Z
    const T: f64 = 1_719_831_909_000.0;

    #[test]
    fn rfc_5322_dates() {
        assert_eq!(time("Mon, 01 Jul 2024 11:05:09 GMT"), Some((T, false)));
        assert_eq!(time("Mon, 1 Jul 2024 11:05:09 +0000"), Some((T, false)));
        assert_eq!(
            time("Mon, 01 Jul 2024 11:05:09 +0100"),
            Some((T - 3_600_000.0, false))
        );
        // Obsolete syntax: no weekday comma, two-digit year, zone names,
        // a comment, no seconds.
        assert_eq!(time("Mon 01 Jul 24 11:05:09 UT"), Some((T, false)));
        assert_eq!(time("01 Jul 2024 06:05:09 EST"), Some((T, false)));
        assert_eq!(time("01 Jul 2024 04:05:09 PDT (Pacific)"), Some((T, false)));
        assert_eq!(time("01 Jul 2024 11:05 GMT"), Some((T - 9_000.0, false)));
        assert_eq!(time("Mon, 01 Jul 2024 11:05:09 A"), None);
    }

    #[test]
    fn rfc_850_and_asctime() {
        assert_eq!(time("Monday, 01-Jul-24 11:05:09 GMT"), Some((T, false)));
        assert_eq!(time("Monday, 01-Jul-2024 11:05:09 GMT"), Some((T, false)));
        assert_eq!(time("Mon Jul  1 11:05:09 2024"), Some((T, true)));
        assert_eq!(time("Mon Jul 1 11:05:09 GMT 2024"), Some((T, false)));
        assert_eq!(time("Mon Jul 1 11:05:09 2024 GMT"), Some((T, false)));
    }

    #[test]
    fn written_english_and_numeric_dates() {
        let local = |text| time(text).map(|(t, _)| t);
        let midnight = 1_719_792_000_000.0;
        assert_eq!(time("Jul 1, 2024"), Some((midnight, true)));
        assert_eq!(local("July 1 2024"), Some(midnight));
        assert_eq!(local("1 Jul 2024"), Some(midnight));
        assert_eq!(local("Thursday, July 1, 2024"), Some(midnight));
        assert_eq!(local("7/1/2024"), Some(midnight));
        assert_eq!(local("2024/07/01"), Some(midnight));
        assert_eq!(local("2024-7-1"), Some(midnight));
        assert_eq!(local("7/1/24"), Some(midnight));
        assert_eq!(local("13/1/2024"), None);
        assert_eq!(local("Feb 30 2024"), local("Mar 1 2024"));
        assert_eq!(local("Dec 32 2024"), None);
        // The year of a date without one is 2001.
        assert_eq!(local("Jul 1"), local("Jul 1 2001"));
    }

    #[test]
    fn clocks_and_zones() {
        let utc = |text| time(text).map(|(t, local)| (t - T, local));
        assert_eq!(utc("Jul 1 2024 11:05:09 UTC"), Some((0.0, false)));
        assert_eq!(utc("Jul 1 2024 12:05:09 GMT+0100"), Some((0.0, false)));
        assert_eq!(utc("Jul 1 2024 12:05:09 GMT +01:00"), Some((0.0, false)));
        assert_eq!(utc("Jul 1 2024 12:05:09 +1"), Some((0.0, false)));
        assert_eq!(utc("Jul 1 2024 11:05:09.123456 z"), Some((123.0, false)));
        assert_eq!(utc("7/1/2024, 11:05:09 AM Z"), Some((0.0, false)));
        assert_eq!(utc("7/1/2024, 11:05:09 PM Z"), Some((43_200_000.0, false)));
        assert_eq!(utc("7/1/2024 12:05 AM Z"), Some((-39_609_000.0, false)));
        assert_eq!(utc("Jul 1 2024 13:05:09 PM"), None);
        // An offset without a clock is not a zone.
        assert_eq!(time("Jul 1 2024 +0100"), None);
        assert_eq!(time("Jul 1 2024 11:05:09 GMT+2400"), None);
        assert_eq!(time("Jul 1 2024 24:00"), time("Jul 2 2024 0:00"));
        assert_eq!(time("Jul 1 2024 24:01"), None);
    }

    #[test]
    fn the_clock_may_come_first() {
        let utc = |text| time(text).map(|(t, local)| (t - T, local));
        assert_eq!(utc("11:05:09 AM Z Jul 1 2024"), Some((0.0, false)));
        assert_eq!(utc("11:05:09 GMT Jul 1 2024"), Some((0.0, false)));
        assert_eq!(utc("12:05:09 +0100 7/1/2024"), Some((0.0, false)));
        assert_eq!(time("11:05 PM 7/1/2024"), time("Jul 1 2024 23:05"));
        assert_eq!(time("11:05"), None);
        assert_eq!(time("11:05 Z"), None);
        assert_eq!(time("11:05, 7/1/2024"), None);
        assert_eq!(time("11:05(c) 7/1/2024"), None);
    }

    #[test]
    fn expanded_years_and_years_alone() {
        let utc = |text| time(text).map(|(t, local)| (t - T, local));
        assert_eq!(utc("+002024-07-01 11:05:09 Z"), Some((0.0, false)));
        assert_eq!(
            time("-000001-01-01 00:00 Z"),
            Some((-62_198_755_200_000.0, false))
        );
        assert_eq!(time("-000000-01-01 00:00 Z"), None);
        assert_eq!(time("2024 EST"), Some((1_704_085_200_000.0, false)));
        assert_eq!(time("2024 17:06"), Some((1_704_128_760_000.0, true)));
        // Not a year alone.
        assert_eq!(time("2024"), None);
        assert_eq!(time("12 EST"), None);
        // A positive year takes the rule for two digits; a negative one
        // does not.
        let year_of = |text| time(text).map(|(t, _)| fields(t as i64).year);
        assert_eq!(year_of("+000050 7 1"), Some(1950));
        assert_eq!(year_of("+000049 7 1"), Some(2049));
        assert_eq!(year_of("-000050 7 1"), Some(-50));
        // Only at the very start of the text.
        assert_eq!(time(" -000001 01"), None);
        assert_eq!(time("(x)-000001 01"), None);
        assert_eq!(time("Mon -000001 01"), None);
    }

    #[test]
    fn free_text_and_comments_and_nul() {
        assert!(time("on Jul 1 2024").is_some());
        assert!(time("Date: Jul 1 2024").is_some());
        assert!(time("Jul(x) 1 (a(b)) 2024 (end").is_some());
        assert!(time("Jul 1 2024\0 junk").is_some());
        assert!(time("Jul 1 2024 junk").is_none());
        assert!(time("x1 Jul 2024").is_none());
        assert!(time("Jul 1 2024 )").is_none());
        assert!(time("\0Jul 1 2024").is_none());
        assert!(time("Jul").is_none());
        assert!(time("").is_none());
        // Every word before the date but a month is free text, also a zone
        // name: the date stays local.
        for text in [
            "T Jul 1 2024",
            "GMT Jul 1 2024",
            "PM Jul 1 2024",
            "Z 2024-07-01",
        ] {
            assert_eq!(time(text).map(|(_, local)| local), Some(true), "{text}");
        }
        // A non-ASCII unit that is not white space belongs to a word.
        assert!(time("\u{100} Jul 1 2024").is_some());
        assert!(time("\u{e9}, Jul 1 2024").is_some());
        assert!(time("\u{100}x Jul 1 2024").is_some());
        assert!(time("\u{100}Jul 1 2024").is_none());
        assert!(time("\u{100}1 Jul 2024").is_none());
        assert!(time("Jan\u{2028}1 2000").is_some());
    }

    /// No panic on any sequence of units: a deterministic mix of the units
    /// that the readers look at, lone surrogates and NUL.
    #[test]
    fn random_units_do_not_panic() {
        let alphabet: Vec<u16> = " \t\u{a0}\u{2028}()+-:./,;0123456789ZzTtAaPpMmGgUuCcEeSsDdJjNnOoVvBbRrYyLlIiHh\u{e9}\0"
            .encode_utf16()
            .chain([0xd800, 0xdc00, 0xffff])
            .collect();
        let mut seed = 0x2545_f491_u32;
        for _ in 0..20_000 {
            let mut units = Vec::new();
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            for _ in 0..(seed >> 24) % 48 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                units.push(alphabet[(seed >> 16) as usize % alphabet.len()]);
            }
            let _ = parse(Str16::Wide(&units));
            let _ = super::super::iso_variants::parse(Str16::Wide(&units));
        }
    }

    #[test]
    fn too_many_tokens_and_long_runs_are_rejected_in_one_pass() {
        let long = "1 ".repeat(100_000);
        let units: Vec<u16> = long.encode_utf16().collect();
        let (parsed, work) = parse(Str16::Wide(&units));
        assert!(parsed.is_none());
        assert!(work < 100);
        let words = format!("{}Jul 1 2024", "x ".repeat(100_000));
        let units: Vec<u16> = words.encode_utf16().collect();
        let (parsed, work) = parse(Str16::Wide(&units));
        assert!(parsed.is_some());
        assert!(work <= 2 * units.len());
    }
}
