//! Reading the characters of a pattern: code units without `u` and `v`,
//! code points (surrogate pairs decoded) with them (§22.2.3.3,
//! `RegExpInitialize` step 10).

use swb_js_text::Str16;

/// A position in the pattern text.
pub(super) struct Cursor<'a> {
    text: Str16<'a>,
    pos: usize,
    unicode: bool,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(text: Str16<'a>, unicode: bool) -> Self {
        Cursor {
            text,
            pos: 0,
            unicode,
        }
    }

    /// The position in code units.
    pub(super) fn pos(&self) -> usize {
        self.pos
    }

    /// The character at the position.
    pub(super) fn peek(&self) -> Option<u32> {
        if self.unicode {
            self.text.code_point_at(self.pos).map(|(cp, _)| cp)
        } else {
            self.text.get(self.pos).map(u32::from)
        }
    }

    /// The code unit `offset` units after the position. Use it only to look
    /// for ASCII characters.
    pub(super) fn unit(&self, offset: usize) -> Option<u32> {
        self.text.get(self.pos + offset).map(u32::from)
    }

    /// Whether the code unit `offset` units ahead is the ASCII byte `b`.
    pub(super) fn is(&self, offset: usize, b: u8) -> bool {
        self.unit(offset) == Some(u32::from(b))
    }

    /// Whether the text at the position starts with these ASCII bytes.
    pub(super) fn starts_with(&self, bytes: &[u8]) -> bool {
        bytes.iter().enumerate().all(|(i, &b)| self.is(i, b))
    }

    /// Reads the character at the position.
    pub(super) fn next(&mut self) -> Option<u32> {
        if self.unicode {
            let (cp, next) = self.text.code_point_at(self.pos)?;
            self.pos = next;
            Some(cp)
        } else {
            let unit = self.text.get(self.pos)?;
            self.pos += 1;
            Some(u32::from(unit))
        }
    }

    /// Reads the ASCII byte `b` if it is next.
    pub(super) fn eat(&mut self, b: u8) -> bool {
        if self.is(0, b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Skips `n` code units (ASCII characters that the caller has seen).
    pub(super) fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    /// Reads one hexadecimal digit.
    pub(super) fn hex_digit(&mut self) -> Option<u32> {
        let value = char::from_u32(self.unit(0)?)?.to_digit(16)?;
        self.pos += 1;
        Some(value)
    }

    /// Reads exactly `n` hexadecimal digits, or nothing.
    pub(super) fn hex_digits(&mut self, n: usize) -> Option<u32> {
        let start = self.pos;
        let mut value = 0;
        for _ in 0..n {
            let Some(d) = self.hex_digit() else {
                self.pos = start;
                return None;
            };
            value = value * 16 + d;
        }
        Some(value)
    }

    /// Reads a run of decimal digits; returns its start and end in code
    /// units (empty if no digit follows).
    pub(super) fn digits(&mut self) -> (usize, usize) {
        let start = self.pos;
        while self.unit(0).is_some_and(|u| (0x30..=0x39).contains(&u)) {
            self.pos += 1;
        }
        (start, self.pos)
    }

    /// The decimal digits `start..end` as a number, saturated at
    /// `u64::MAX`.
    pub(super) fn number(&self, start: usize, end: usize) -> u64 {
        (start..end).fold(0u64, |acc, i| {
            let digit = self
                .text
                .get(i)
                .map_or(0, |u| u64::from(u.saturating_sub(0x30)));
            acc.saturating_mul(10).saturating_add(digit)
        })
    }

    /// Compares two runs of decimal digits by value.
    pub(super) fn compare_numbers(
        &self,
        a: (usize, usize),
        b: (usize, usize),
    ) -> std::cmp::Ordering {
        let trim = |(start, end): (usize, usize)| {
            let mut s = start;
            while s + 1 < end && self.text.get(s) == Some(0x30) {
                s += 1;
            }
            (s, end)
        };
        let (a, b) = (trim(a), trim(b));
        (a.1 - a.0).cmp(&(b.1 - b.0)).then_with(|| {
            let units = |(s, e): (usize, usize)| (s..e).map(|i| self.text.get(i));
            units(a).cmp(units(b))
        })
    }

    /// Moves back to an earlier position.
    pub(super) fn reset(&mut self, pos: usize) {
        self.pos = pos;
    }
}
