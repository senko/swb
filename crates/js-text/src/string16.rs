//! Borrowed and owned text of both code-unit widths (ADR 0026 section 7).
//!
//! Text is a sequence of 16-bit code units with UTF-16 semantics; it can
//! contain unpaired surrogates. The narrow width stores one byte per unit
//! (Latin-1, all units below 256), the wide width two bytes.

use std::fmt;
use std::hash::Hash;
use std::slice;

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
}

/// A code unit of one of the two widths: `u8` for Latin-1 text and `u16`
/// for other text. Code that reads text of both widths (the lexer, the
/// regular expression matcher) is generic over this trait.
pub trait CodeUnit:
    sealed::Sealed + Copy + Eq + Ord + Hash + fmt::Debug + Default + 'static
{
    /// Whether this is the two-byte width.
    const WIDE: bool;

    /// The value of the unit.
    fn value(self) -> u16;

    /// The borrowed view of a slice of units.
    fn view(units: &[Self]) -> Str16<'_>;
}

impl CodeUnit for u8 {
    const WIDE: bool = false;

    #[inline]
    fn value(self) -> u16 {
        u16::from(self)
    }

    fn view(units: &[u8]) -> Str16<'_> {
        Str16::Latin1(units)
    }
}

impl CodeUnit for u16 {
    const WIDE: bool = true;

    #[inline]
    fn value(self) -> u16 {
        self
    }

    fn view(units: &[u16]) -> Str16<'_> {
        Str16::Wide(units)
    }
}

/// Whether a code unit is a leading (high) surrogate, U+D800..U+DBFF.
#[inline]
pub fn is_lead_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

/// Whether a code unit is a trailing (low) surrogate, U+DC00..U+DFFF.
#[inline]
pub fn is_trail_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

/// The code point of a surrogate pair (`UTF16SurrogatePairToCodePoint`,
/// ECMA-262 §11.1.3).
#[inline]
pub fn combine_surrogates(lead: u16, trail: u16) -> u32 {
    0x10000 + ((u32::from(lead) - 0xD800) << 10) + (u32::from(trail) - 0xDC00)
}

/// A borrowed view of text in one of the two widths. The wide variant may
/// hold text whose units are all below 256; comparisons look at the units,
/// not at the width.
#[derive(Clone, Copy)]
pub enum Str16<'a> {
    /// One byte per code unit.
    Latin1(&'a [u8]),
    /// Two bytes per code unit.
    Wide(&'a [u16]),
}

impl<'a> Str16<'a> {
    /// The number of code units.
    pub fn len(self) -> usize {
        match self {
            Str16::Latin1(units) => units.len(),
            Str16::Wide(units) => units.len(),
        }
    }

    /// Whether the text has no code units.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// The code unit at `index`, or `None` past the end.
    #[inline]
    pub fn get(self, index: usize) -> Option<u16> {
        match self {
            Str16::Latin1(units) => units.get(index).map(|&u| u16::from(u)),
            Str16::Wide(units) => units.get(index).copied(),
        }
    }

    /// The units `start..end`, or `None` if the range is not inside the text.
    pub fn slice(self, start: usize, end: usize) -> Option<Str16<'a>> {
        match self {
            Str16::Latin1(units) => units.get(start..end).map(Str16::Latin1),
            Str16::Wide(units) => units.get(start..end).map(Str16::Wide),
        }
    }

    /// An iterator over the code units.
    pub fn units(self) -> Units<'a> {
        Units(match self {
            Str16::Latin1(units) => UnitsRepr::Latin1(units.iter()),
            Str16::Wide(units) => UnitsRepr::Wide(units.iter()),
        })
    }

    /// An iterator over the code points: surrogate pairs are decoded,
    /// unpaired surrogates are returned as they are (`CodePointAt`, §11.1.4).
    pub fn code_points(self) -> CodePoints<'a> {
        CodePoints { text: self, pos: 0 }
    }

    /// The text as UTF-8; unpaired surrogates become U+FFFD.
    pub fn to_string_lossy(self) -> String {
        match self {
            Str16::Latin1(units) => units.iter().map(|&u| char::from(u)).collect(),
            Str16::Wide(units) => char::decode_utf16(units.iter().copied())
                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect(),
        }
    }

    /// The owned copy, in the narrow width if all units are below 256.
    pub fn to_string16(self) -> String16 {
        let mut out = String16::new();
        out.push_str16(self);
        out
    }

    /// Whether the text has the same code units as the UTF-16 form of `s`.
    pub fn eq_str(self, s: &str) -> bool {
        self.units().eq(s.encode_utf16())
    }
}

impl PartialEq for Str16<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Str16::Latin1(a), Str16::Latin1(b)) => a == b,
            (Str16::Wide(a), Str16::Wide(b)) => a == b,
            _ => self.len() == other.len() && self.units().eq(other.units()),
        }
    }
}

impl Eq for Str16<'_> {}

impl fmt::Debug for Str16<'_> {
    /// A quoted string; unpaired surrogates appear as `\u{d800}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for cp in self.code_points() {
            match char::from_u32(cp) {
                Some(c) => write!(f, "{}", c.escape_debug())?,
                None => write!(f, "\\u{{{cp:x}}}")?,
            }
        }
        f.write_str("\"")
    }
}

impl fmt::Display for Str16<'_> {
    /// The text with unpaired surrogates replaced by U+FFFD.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

/// An iterator over the code units of a [`Str16`].
#[derive(Clone, Debug)]
pub struct Units<'a>(UnitsRepr<'a>);

#[derive(Clone, Debug)]
enum UnitsRepr<'a> {
    Latin1(slice::Iter<'a, u8>),
    Wide(slice::Iter<'a, u16>),
}

impl Iterator for Units<'_> {
    type Item = u16;

    #[inline]
    fn next(&mut self) -> Option<u16> {
        match &mut self.0 {
            UnitsRepr::Latin1(iter) => iter.next().map(|&u| u16::from(u)),
            UnitsRepr::Wide(iter) => iter.next().copied(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            UnitsRepr::Latin1(iter) => iter.size_hint(),
            UnitsRepr::Wide(iter) => iter.size_hint(),
        }
    }
}

impl ExactSizeIterator for Units<'_> {}

/// An iterator over the code points of a [`Str16`]: surrogate pairs are
/// decoded, unpaired surrogates are returned as they are.
#[derive(Clone, Debug)]
pub struct CodePoints<'a> {
    text: Str16<'a>,
    pos: usize,
}

impl Iterator for CodePoints<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let unit = self.text.get(self.pos)?;
        self.pos += 1;
        if is_lead_surrogate(unit)
            && let Some(trail) = self.text.get(self.pos)
            && is_trail_surrogate(trail)
        {
            self.pos += 1;
            return Some(combine_surrogates(unit, trail));
        }
        Some(u32::from(unit))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = self.text.len().saturating_sub(self.pos);
        (rest.div_ceil(2), Some(rest))
    }
}

/// Owned text. It uses the narrow width whenever all units are below 256,
/// so two equal texts always have the same width, and the derived
/// equality and hash compare the units.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct String16(Repr);

#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    /// All units are below 256.
    Latin1(Vec<u8>),
    /// At least one unit is 256 or above.
    Wide(Vec<u16>),
}

impl Default for String16 {
    fn default() -> Self {
        Self::new()
    }
}

impl String16 {
    /// The empty text.
    pub const fn new() -> Self {
        String16(Repr::Latin1(Vec::new()))
    }

    /// A copy of the code units, in the narrow width if possible.
    pub fn from_units(units: &[u16]) -> Self {
        Str16::Wide(units).to_string16()
    }

    /// The borrowed view.
    pub fn as_str16(&self) -> Str16<'_> {
        match &self.0 {
            Repr::Latin1(units) => Str16::Latin1(units),
            Repr::Wide(units) => Str16::Wide(units),
        }
    }

    /// The number of code units.
    pub fn len(&self) -> usize {
        self.as_str16().len()
    }

    /// Whether the text has no code units.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the text uses the narrow width (all units below 256).
    pub fn is_latin1(&self) -> bool {
        matches!(self.0, Repr::Latin1(_))
    }

    /// Appends one code unit.
    pub fn push(&mut self, unit: u16) {
        match &mut self.0 {
            Repr::Latin1(units) => match u8::try_from(unit) {
                Ok(byte) => units.push(byte),
                Err(_) => {
                    self.widen().push(unit);
                }
            },
            Repr::Wide(units) => units.push(unit),
        }
    }

    /// Appends a code point: one unit, or a surrogate pair above U+FFFF.
    /// A value above U+10FFFF appends U+FFFD.
    pub fn push_code_point(&mut self, cp: u32) {
        match u16::try_from(cp) {
            Ok(unit) => self.push(unit),
            Err(_) if cp <= 0x10_FFFF => {
                let offset = cp - 0x10000;
                self.push(0xD800 + (offset >> 10) as u16);
                self.push(0xDC00 + (offset & 0x3FF) as u16);
            }
            Err(_) => self.push(0xFFFD),
        }
    }

    /// Appends text of either width.
    pub fn push_str16(&mut self, text: Str16<'_>) {
        match (&mut self.0, text) {
            (Repr::Latin1(units), Str16::Latin1(more)) => units.extend_from_slice(more),
            (Repr::Wide(units), Str16::Latin1(more)) => {
                units.extend(more.iter().map(|&u| u16::from(u)));
            }
            (Repr::Wide(units), Str16::Wide(more)) => units.extend_from_slice(more),
            (Repr::Latin1(units), Str16::Wide(more)) => {
                if more.iter().all(|&u| u < 256) {
                    units.extend(more.iter().map(|&u| u as u8));
                } else {
                    self.widen().extend_from_slice(more);
                }
            }
        }
    }

    /// Appends code units of either width.
    pub fn push_units<U: CodeUnit>(&mut self, units: &[U]) {
        self.push_str16(U::view(units));
    }

    /// The text as UTF-8; unpaired surrogates become U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        self.as_str16().to_string_lossy()
    }

    /// Converts the text to the wide width and returns its units.
    fn widen(&mut self) -> &mut Vec<u16> {
        if let Repr::Latin1(units) = &self.0 {
            let wide = units.iter().map(|&u| u16::from(u)).collect();
            self.0 = Repr::Wide(wide);
        }
        match &mut self.0 {
            Repr::Wide(units) => units,
            Repr::Latin1(_) => unreachable!("the text was widened above"),
        }
    }
}

impl From<&str> for String16 {
    /// The UTF-16 form of `s`, in the narrow width if all its characters
    /// are below U+0100.
    fn from(s: &str) -> Self {
        if s.is_ascii() {
            return String16(Repr::Latin1(s.as_bytes().to_vec()));
        }
        if s.chars().all(|c| u32::from(c) < 256) {
            return String16(Repr::Latin1(
                s.chars().map(|c| u32::from(c) as u8).collect(),
            ));
        }
        String16(Repr::Wide(s.encode_utf16().collect()))
    }
}

impl fmt::Debug for String16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_str16(), f)
    }
}

impl fmt::Display for String16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.as_str16(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_picks_the_width() {
        assert!(String16::from("abc").is_latin1());
        assert!(String16::from("caf\u{e9}\u{ff}").is_latin1());
        assert_eq!(String16::from("caf\u{e9}").len(), 4);
        let wide = String16::from("\u{100}\u{1F600}");
        assert!(!wide.is_latin1());
        assert_eq!(wide.len(), 3);
        assert_eq!(wide.to_string_lossy(), "\u{100}\u{1F600}");
    }

    #[test]
    fn equal_texts_of_both_widths() {
        assert_eq!(
            String16::from_units(&[0x61, 0xFF]),
            String16::from("a\u{ff}")
        );
        assert!(String16::from_units(&[0x61, 0xFF]).is_latin1());
        assert_eq!(Str16::Wide(&[0x61, 0x62]), Str16::Latin1(b"ab"));
        assert_ne!(Str16::Wide(&[0x61]), Str16::Latin1(b"ab"));
        assert!(Str16::Latin1(b"ab").eq_str("ab"));
    }

    #[test]
    fn push_widens_on_the_first_wide_unit() {
        let mut s = String16::new();
        s.push(0x41);
        s.push_units(&[0x42u8, 0xE9]);
        assert!(s.is_latin1());
        s.push_units(&[0x43u16]);
        assert!(s.is_latin1());
        s.push(0x3B1);
        assert!(!s.is_latin1());
        s.push_units(&[0x44u8]);
        assert_eq!(s.to_string_lossy(), "AB\u{e9}C\u{3b1}D");
    }

    #[test]
    fn push_code_point() {
        let mut s = String16::new();
        s.push_code_point(0x1F600);
        s.push_code_point(0x41);
        s.push_code_point(0x11_0000);
        assert_eq!(
            s.as_str16().units().collect::<Vec<_>>(),
            [0xD83D, 0xDE00, 0x41, 0xFFFD]
        );
    }

    #[test]
    fn code_points_decode_pairs_and_keep_unpaired_surrogates() {
        let units = [0x61, 0xD83D, 0xDE00, 0xD800, 0x62, 0xDC00, 0xDBFF];
        let text = Str16::Wide(&units);
        assert_eq!(
            text.code_points().collect::<Vec<_>>(),
            [0x61, 0x1F600, 0xD800, 0x62, 0xDC00, 0xDBFF]
        );
        assert_eq!(
            text.to_string_lossy(),
            "a\u{1F600}\u{FFFD}b\u{FFFD}\u{FFFD}"
        );
        assert_eq!(
            format!("{text:?}"),
            "\"a\u{1F600}\\u{d800}b\\u{dc00}\\u{dbff}\""
        );
    }

    #[test]
    fn slice_and_get() {
        let text = Str16::Latin1(b"hello");
        assert_eq!(text.get(1), Some(u16::from(b'e')));
        assert_eq!(text.get(5), None);
        assert!(text.slice(1, 3).is_some_and(|s| s.eq_str("el")));
        assert!(text.slice(4, 6).is_none());
        assert!(text.slice(3, 2).is_none());
    }

    #[test]
    fn latin1_to_string() {
        assert_eq!(
            Str16::Latin1(&[0x63, 0xE9, 0xFF]).to_string_lossy(),
            "c\u{e9}\u{ff}"
        );
    }
}
