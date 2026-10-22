//! Case mapping for `String.prototype.toLowerCase` and `toUpperCase`
//! (ECMA-262 §22.1.3.28 and §22.1.3.30) and case folding for
//! `localeCompare`.
//!
//! ECMA-262 says to convert with the Unicode Default Case Conversion
//! algorithm (Unicode Standard 17.0 §3.13, "Default Case Conversion",
//! definitions D134 and following; <https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-3/#G33992>):
//! the full mappings, that is the simple mappings of `UnicodeData.txt`
//! and the unconditional mappings of `SpecialCasing.txt`, which may
//! give up to three code points, and the context `Final_Sigma` (Table
//! 3-17) for U+03A3 in lower case. The language-sensitive mappings
//! (Lithuanian, Turkish, Azeri) are not done: the `toLocale` forms use the
//! root locale, as without ECMA-402. Lone surrogates and code points
//! without a mapping map to themselves.
//!
//! The functions work on slices of a text so that the caller can charge
//! time and check limits between them ([`map_chunk`]). A Latin-1 text
//! needs no table at all.

use super::case_tables as t;
use super::in_ranges;
use crate::Str16;

/// The direction of a case conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseOp {
    /// `toLowerCase`.
    Lower,
    /// `toUpperCase`.
    Upper,
}

/// The delta of a run table for `cp`, if a run maps it.
fn run_delta(runs: &[(u32, u32, u8, i32)], cp: u32) -> Option<i32> {
    let index = runs.partition_point(|run| run.0 <= cp).checked_sub(1)?;
    let &(first, last, step, delta) = runs.get(index)?;
    (cp <= last && (cp - first).is_multiple_of(u32::from(step))).then_some(delta)
}

fn apply(cp: u32, delta: i32) -> u32 {
    cp.wrapping_add_signed(delta)
}

/// The full mapping of a code point: up to three code points and their
/// number.
type Mapped = ([u32; 3], usize);

fn special(table: &[(u32, [u32; 3])], cp: u32) -> Option<Mapped> {
    let index = table.binary_search_by_key(&cp, |entry| entry.0).ok()?;
    let mapped = table.get(index)?.1;
    let len = mapped.iter().take_while(|&&c| c != 0).count();
    Some((mapped, len))
}

/// The full lowercase mapping of `cp` without the context `Final_Sigma`
/// (U+03A3 maps to U+03C3).
fn lower_full(cp: u32) -> Mapped {
    if cp < 0x80 {
        let lower = if (0x41..=0x5A).contains(&cp) {
            cp + 32
        } else {
            cp
        };
        return ([lower, 0, 0], 1);
    }
    if let Some(mapped) = special(&t::LOWER_SPECIAL, cp) {
        return mapped;
    }
    (
        [
            run_delta(&t::LOWER_RUNS, cp).map_or(cp, |d| apply(cp, d)),
            0,
            0,
        ],
        1,
    )
}

/// The full uppercase mapping of `cp`.
fn upper_full(cp: u32) -> Mapped {
    if cp < 0x80 {
        let upper = if (0x61..=0x7A).contains(&cp) {
            cp - 32
        } else {
            cp
        };
        return ([upper, 0, 0], 1);
    }
    if let Some(mapped) = special(&t::UPPER_SPECIAL, cp) {
        return mapped;
    }
    (
        [
            run_delta(&t::UPPER_RUNS, cp).map_or(cp, |d| apply(cp, d)),
            0,
            0,
        ],
        1,
    )
}

/// The simple case folding of `cp` (`CaseFolding.txt`, status C and S).
pub fn simple_fold(cp: u32) -> u32 {
    if cp < 0x80 {
        return if (0x41..=0x5A).contains(&cp) {
            cp + 32
        } else {
            cp
        };
    }
    run_delta(&t::FOLD_RUNS, cp).map_or(cp, |d| apply(cp, d))
}

/// The code points of a run table that the table maps, with their
/// mappings.
fn run_changes(runs: &'static [(u32, u32, u8, i32)]) -> impl Iterator<Item = (u32, u32)> {
    runs.iter().flat_map(|&(first, last, step, delta)| {
        (first..=last)
            .step_by(usize::from(step))
            .map(move |cp| (cp, apply(cp, delta)))
    })
}

/// Every code point whose simple case folding (`CaseFolding.txt`, status
/// C and S) is another code point, with that folding, in code point
/// order. Regular expressions with `u` or `v` and `i` use it to build the
/// sets of characters that match a class (§22.2.2.7.3, Canonicalize).
pub fn simple_fold_changes() -> impl Iterator<Item = (u32, u32)> {
    let ascii = (0x41..=0x5A).map(|cp| (cp, cp + 32));
    ascii.chain(run_changes(&t::FOLD_RUNS).filter(|&(cp, _)| cp >= 0x80))
}

/// `Canonicalize` of a regular expression without `u` and `v` but with
/// `i` (ECMA-262 §22.2.2.7.3,
/// <https://tc39.es/ecma262/#sec-runtime-semantics-canonicalize-ch>): the
/// full uppercase mapping of the code unit, unless that is not exactly one
/// code unit or maps a unit at or above 128 to one below 128.
pub fn regexp_canonicalize(unit: u16) -> u16 {
    let cp = u32::from(unit);
    if cp < 0x80 {
        return if (0x61..=0x7A).contains(&cp) {
            unit - 32
        } else {
            unit
        };
    }
    let (mapped, len) = upper_full(cp);
    if len != 1 || mapped[0] < 0x80 {
        return unit;
    }
    u16::try_from(mapped[0]).unwrap_or(unit)
}

/// Every code unit that [`regexp_canonicalize`] changes, with its
/// canonical unit, in code unit order.
pub fn regexp_canonicalize_changes() -> impl Iterator<Item = (u16, u16)> {
    let ascii = (0x61..=0x7A).map(|u: u16| (u, u - 32));
    let others = run_changes(&t::UPPER_RUNS).filter_map(|(cp, _)| {
        let unit = u16::try_from(cp).ok().filter(|&u| u >= 0x80)?;
        let canonical = regexp_canonicalize(unit);
        (canonical != unit).then_some((unit, canonical))
    });
    ascii.chain(others)
}

/// Whether `cp` has the property `Cased`.
fn is_cased(cp: u32) -> bool {
    if cp < 0x80 {
        return (cp as u8).is_ascii_alphabetic();
    }
    in_ranges(&t::CASED, cp)
}

/// Whether `cp` has the property `Case_Ignorable`.
fn is_case_ignorable(cp: u32) -> bool {
    if cp < 0x80 {
        return matches!(cp, 0x27 | 0x2E | 0x3A | 0x5E | 0x60);
    }
    in_ranges(&t::CASE_IGNORABLE, cp)
}

/// The condition `Final_Sigma` (Unicode Standard Table 3-17) for the
/// U+03A3 at units `start..end`: a cased letter, then zero or more
/// case-ignorable characters before it, and no case-ignorable
/// characters and then a cased letter after it. A run of case-ignorable
/// characters ends at the next cased letter, so the scans of all sigmas
/// of a text take linear time.
fn is_final_sigma(text: Str16<'_>, start: usize, end: usize) -> bool {
    let mut at = start;
    let mut after_cased = false;
    while let Some((cp, previous)) = text.code_point_before(at) {
        at = previous;
        if is_case_ignorable(cp) {
            continue;
        }
        after_cased = is_cased(cp);
        break;
    }
    if !after_cased {
        return false;
    }
    let mut at = end;
    while let Some((cp, next)) = text.code_point_at(at) {
        at = next;
        if is_case_ignorable(cp) {
            continue;
        }
        return !is_cased(cp);
    }
    true
}

/// The Latin-1 byte `b` in lower case (a byte: the result is Latin-1).
fn latin1_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() || ((0xC0..=0xDE).contains(&b) && b != 0xD7) {
        b + 32
    } else {
        b
    }
}

/// Whether the Latin-1 byte `b` is different in upper case.
fn latin1_upper_changes(b: u8) -> bool {
    b.is_ascii_lowercase()
        || b == 0xB5
        || b == 0xDF
        || b == 0xFF
        || ((0xE0..=0xFE).contains(&b) && b != 0xF7)
}

/// The index of the first code unit whose code point changes in the
/// conversion, or `None` if the whole text stays as it is (the caller
/// returns the same string without a copy). Surrogate pairs are decoded.
pub fn first_change(text: Str16<'_>, op: CaseOp) -> Option<usize> {
    match (text, op) {
        (Str16::Latin1(bytes), CaseOp::Lower) => bytes.iter().position(|&b| latin1_lower(b) != b),
        (Str16::Latin1(bytes), CaseOp::Upper) => {
            bytes.iter().position(|&b| latin1_upper_changes(b))
        }
        (Str16::Wide(_), _) => {
            let mut at = 0;
            while let Some((cp, next)) = text.code_point_at(at) {
                let changes = match op {
                    CaseOp::Lower => {
                        let (mapped, len) = lower_full(cp);
                        len != 1 || mapped[0] != cp
                    }
                    CaseOp::Upper => {
                        let (mapped, len) = upper_full(cp);
                        len != 1 || mapped[0] != cp
                    }
                };
                if changes {
                    return Some(at);
                }
                at = next;
            }
            None
        }
    }
}

/// The result of [`convert_latin1`].
pub enum Latin1Case {
    /// The conversion changes nothing.
    Unchanged,
    /// The converted text.
    Changed(crate::String16),
    /// The result has code units above U+00FF (`µ` and `ÿ` in upper
    /// case): use [`map_chunk`].
    Wide,
}

/// The conversion of a Latin-1 text in one pass, without the tables. The
/// result is Latin-1 as well, except for the two characters named at
/// [`Latin1Case::Wide`].
pub fn convert_latin1(bytes: &[u8], op: CaseOp) -> Latin1Case {
    let first = match op {
        CaseOp::Lower => bytes.iter().position(|&b| latin1_lower(b) != b),
        CaseOp::Upper => bytes.iter().position(|&b| latin1_upper_changes(b)),
    };
    let Some(first) = first else {
        return Latin1Case::Unchanged;
    };
    let mut out = Vec::new();
    if out.try_reserve_exact(bytes.len()).is_err() {
        return Latin1Case::Wide;
    }
    out.extend_from_slice(&bytes[..first]);
    let rest = &bytes[first..];
    match op {
        CaseOp::Lower => out.extend(rest.iter().map(|&b| latin1_lower(b))),
        // A text without the three special characters maps unit by unit.
        CaseOp::Upper if !rest.iter().any(|&b| matches!(b, 0xB5 | 0xDF | 0xFF)) => {
            out.extend(
                rest.iter()
                    .map(|&b| if latin1_upper_changes(b) { b - 32 } else { b }),
            );
        }
        CaseOp::Upper => {
            for &b in rest {
                match b {
                    0xB5 | 0xFF => return Latin1Case::Wide,
                    0xDF => out.extend_from_slice(b"SS"),
                    _ if latin1_upper_changes(b) => out.push(b - 32),
                    _ => out.push(b),
                }
            }
        }
    }
    Latin1Case::Changed(crate::String16::from_latin1(out))
}

/// Converts the code points of `text` that start at unit `from` (a
/// code point boundary) until the unit `limit` is reached or passed, and
/// appends the result to `out`. Returns the index where it stopped (a
/// code point boundary, at least `limit` unless the text ended). The
/// output has at most three units for each unit of input.
pub fn map_chunk(
    text: Str16<'_>,
    from: usize,
    limit: usize,
    op: CaseOp,
    out: &mut crate::String16,
) -> usize {
    let limit = limit.min(text.len());
    if let Str16::Latin1(bytes) = text {
        for &b in bytes.get(from..limit).unwrap_or(&[]) {
            match op {
                CaseOp::Lower => out.push(u16::from(latin1_lower(b))),
                CaseOp::Upper => match b {
                    0xDF => {
                        out.push(u16::from(b'S'));
                        out.push(u16::from(b'S'));
                    }
                    0xB5 => out.push(0x39C),
                    0xFF => out.push(0x178),
                    _ if latin1_upper_changes(b) => out.push(u16::from(b - 32)),
                    _ => out.push(u16::from(b)),
                },
            }
        }
        return limit.max(from);
    }
    let mut at = from;
    while at < limit {
        let Some((cp, next)) = text.code_point_at(at) else {
            break;
        };
        let (mapped, len) = match op {
            CaseOp::Lower if cp == 0x3A3 => {
                if is_final_sigma(text, at, next) {
                    ([0x3C2, 0, 0], 1)
                } else {
                    ([0x3C3, 0, 0], 1)
                }
            }
            CaseOp::Lower => lower_full(cp),
            CaseOp::Upper => upper_full(cp),
        };
        if len == 1 && mapped[0] == cp {
            // Keeps a lone surrogate as it is.
            for unit in at..next {
                out.push(text.get(unit).unwrap_or(0xFFFD));
            }
        } else {
            for &mapped_cp in mapped.iter().take(len) {
                out.push_code_point(mapped_cp);
            }
        }
        at = next;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::String16;

    fn convert(s: &[u16], op: CaseOp) -> Vec<u16> {
        let text = Str16::Wide(s);
        let mut out = String16::new();
        let end = map_chunk(text, 0, usize::MAX, op, &mut out);
        assert_eq!(end, s.len());
        out.as_str16().units().collect()
    }

    fn lower(s: &str) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16_lossy(&convert(&units, CaseOp::Lower))
    }

    fn upper(s: &str) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16_lossy(&convert(&units, CaseOp::Upper))
    }

    #[test]
    fn simple_mappings() {
        assert_eq!(lower("ABC\u{C0}\u{130}"), "abc\u{E0}i\u{307}");
        assert_eq!(upper("abc\u{E0}\u{DF}"), "ABC\u{C0}SS");
        assert_eq!(upper("\u{1C5}\u{1F88}"), "\u{1C4}\u{1F08}\u{399}");
        assert_eq!(upper("\u{390}"), "\u{399}\u{308}\u{301}");
        assert_eq!(upper("\u{FB03}"), "FFI");
        // Supplementary: Deseret.
        assert_eq!(lower("\u{10400}"), "\u{10428}");
        assert_eq!(upper("\u{10428}"), "\u{10400}");
        // The Kelvin sign is lower case k.
        assert_eq!(lower("\u{212A}"), "k");
    }

    #[test]
    fn latin1_special_cases() {
        let mut out = String16::new();
        map_chunk(
            Str16::Latin1(&[0xB5, 0xFF, 0xDF, 0xE9]),
            0,
            4,
            CaseOp::Upper,
            &mut out,
        );
        let units: Vec<u16> = out.as_str16().units().collect();
        assert_eq!(units, [0x39C, 0x178, 0x53, 0x53, 0xC9]);
        let mut out = String16::new();
        map_chunk(
            Str16::Latin1(&[0xC9, 0xD7, 0x41]),
            0,
            3,
            CaseOp::Lower,
            &mut out,
        );
        let units: Vec<u16> = out.as_str16().units().collect();
        assert_eq!(units, [0xE9, 0xD7, 0x61]);
        assert!(out.is_latin1());
    }

    #[test]
    fn latin1_in_one_pass() {
        assert!(matches!(
            convert_latin1(b"abc 1", CaseOp::Lower),
            Latin1Case::Unchanged
        ));
        assert!(matches!(
            convert_latin1(b"ABC", CaseOp::Upper),
            Latin1Case::Unchanged
        ));
        let Latin1Case::Changed(text) = convert_latin1(b"Ab\xDF\xE9", CaseOp::Upper) else {
            panic!("changed");
        };
        assert_eq!(
            text.as_str16().units().collect::<Vec<_>>(),
            [0x41, 0x42, 0x53, 0x53, 0xC9]
        );
        assert!(matches!(
            convert_latin1(b"a\xB5", CaseOp::Upper),
            Latin1Case::Wide
        ));
        assert!(matches!(
            convert_latin1(b"a\xFF", CaseOp::Upper),
            Latin1Case::Wide
        ));
        let Latin1Case::Changed(text) = convert_latin1(b"A\xC0\xFF", CaseOp::Lower) else {
            panic!("changed");
        };
        assert_eq!(
            text.as_str16().units().collect::<Vec<_>>(),
            [0x61, 0xE0, 0xFF]
        );
    }

    #[test]
    fn final_sigma() {
        assert_eq!(lower("\u{391}\u{3A3}"), "\u{3B1}\u{3C2}");
        assert_eq!(lower("\u{3A3}"), "\u{3C3}");
        assert_eq!(lower("\u{391}\u{3A3}\u{392}"), "\u{3B1}\u{3C3}\u{3B2}");
        // Case-ignorable characters between: the apostrophe, a combining mark.
        assert_eq!(lower("A'\u{3A3}"), "a'\u{3C2}");
        assert_eq!(lower("A\u{3A3}'B"), "a\u{3C3}'b");
        assert_eq!(lower("A\u{3A3}'."), "a\u{3C2}'.");
        assert_eq!(lower("A.\u{3A3}\u{301}"), "a.\u{3C2}\u{301}");
        // Not preceded by a cased letter.
        assert_eq!(lower("1\u{3A3}"), "1\u{3C3}");
        assert_eq!(lower(" \u{3A3}"), " \u{3C3}");
        // A lone surrogate is neither cased nor case-ignorable.
        assert_eq!(
            convert(&[0x41, 0xD800, 0x3A3], CaseOp::Lower),
            [0x61, 0xD800, 0x3C3]
        );
        // Supplementary cased letter before.
        assert_eq!(lower("\u{10400}\u{3A3}"), "\u{10428}\u{3C2}");
    }

    #[test]
    fn lone_surrogates_and_unassigned_stay() {
        let units = [0xD800, 0x41, 0xDC00, 0xDBFF, 0xDFFF];
        assert_eq!(
            convert(&units, CaseOp::Lower),
            [0xD800, 0x61, 0xDC00, 0xDBFF, 0xDFFF]
        );
        assert_eq!(convert(&units, CaseOp::Upper), units);
        assert_eq!(upper("\u{378}\u{10FFFF}"), "\u{378}\u{10FFFF}");
    }

    #[test]
    fn first_change_finds_the_start() {
        assert_eq!(first_change(Str16::Latin1(b"abc"), CaseOp::Lower), None);
        assert_eq!(first_change(Str16::Latin1(b"abC"), CaseOp::Lower), Some(2));
        assert_eq!(first_change(Str16::Latin1(b"ABC"), CaseOp::Upper), None);
        assert_eq!(
            first_change(Str16::Latin1(&[0x41, 0xDF]), CaseOp::Upper),
            Some(1)
        );
        assert_eq!(
            first_change(Str16::Wide(&[0x3B1, 0x3B2]), CaseOp::Lower),
            None
        );
        assert_eq!(
            first_change(Str16::Wide(&[0x3B1, 0x3A3]), CaseOp::Lower),
            Some(1)
        );
    }

    #[test]
    fn chunks_continue_where_they_stopped() {
        let units: Vec<u16> = "A\u{10400}\u{10400}b".encode_utf16().collect();
        let text = Str16::Wide(&units);
        let mut out = String16::new();
        let mut at = 0;
        while at < units.len() {
            at = map_chunk(text, at, at + 1, CaseOp::Lower, &mut out);
        }
        assert_eq!(out.to_string_lossy(), "a\u{10428}\u{10428}b");
    }

    #[test]
    fn case_folding() {
        assert_eq!(simple_fold('A' as u32), 'a' as u32);
        assert_eq!(simple_fold(0x3C2), 0x3C3);
        assert_eq!(simple_fold(0x17F), 's' as u32);
        assert_eq!(simple_fold(0xDF), 0xDF);
        assert_eq!(simple_fold(0x1E9E), 0xDF);
    }

    #[test]
    fn ascii_fast_paths_agree_with_the_tables() {
        for cp in 0..0x80u32 {
            assert_eq!(is_cased(cp), in_ranges(&t::CASED, cp), "{cp:#x}");
            assert_eq!(
                is_case_ignorable(cp),
                in_ranges(&t::CASE_IGNORABLE, cp),
                "{cp:#x}"
            );
            let lower = run_delta(&t::LOWER_RUNS, cp).map_or(cp, |d| apply(cp, d));
            let upper = run_delta(&t::UPPER_RUNS, cp).map_or(cp, |d| apply(cp, d));
            assert_eq!(lower_full(cp).0[0], lower, "{cp:#x}");
            assert_eq!(upper_full(cp).0[0], upper, "{cp:#x}");
            assert_eq!(
                simple_fold(cp),
                run_delta(&t::FOLD_RUNS, cp).map_or(cp, |d| apply(cp, d))
            );
        }
    }

    #[test]
    fn latin1_functions_agree_with_the_tables() {
        for b in 0..=255u8 {
            let cp = u32::from(b);
            assert_eq!(u32::from(latin1_lower(b)), lower_full(cp).0[0], "{cp:#x}");
            let (upper, len) = upper_full(cp);
            assert_eq!(
                latin1_upper_changes(b),
                len != 1 || upper[0] != cp,
                "{cp:#x}"
            );
        }
    }

    #[test]
    fn regexp_canonicalize_follows_the_rules() {
        let c = |u: u32| u32::from(regexp_canonicalize(u as u16));
        assert_eq!(c(0x61), 0x41);
        assert_eq!(c(0x41), 0x41);
        assert_eq!(c(0xE5), 0xC5);
        assert_eq!(c(0x17F), 0x17F, "ſ maps to S: non-ASCII to ASCII");
        assert_eq!(c(0x131), 0x131, "ı maps to I");
        assert_eq!(c(0xDF), 0xDF, "ß maps to SS");
        assert_eq!(c(0x1F80), 0x1F80, "the full mapping has two code points");
        assert_eq!(c(0x3C2), 0x3A3);
        assert_eq!(c(0x212A), 0x212A, "already upper case");
        assert_eq!(c(0xFF), 0x178);
        assert_eq!(c(0xD800), 0xD800);
    }

    #[test]
    fn change_lists_match_the_functions() {
        let changes: Vec<(u16, u16)> = regexp_canonicalize_changes().collect();
        assert!(changes.windows(2).all(|w| w[0].0 < w[1].0));
        let mut listed = changes.iter().peekable();
        for unit in 0..=u16::MAX {
            let canonical = regexp_canonicalize(unit);
            if canonical == unit {
                assert!(listed.peek().is_none_or(|&&(u, _)| u != unit));
            } else {
                assert_eq!(listed.next(), Some(&(unit, canonical)));
            }
        }
        assert!(listed.next().is_none());
        let folds: Vec<(u32, u32)> = simple_fold_changes().collect();
        assert!(folds.windows(2).all(|w| w[0].0 < w[1].0));
        let mut listed = folds.iter().peekable();
        for cp in 0..=0x0010_FFFF {
            let folded = simple_fold(cp);
            if folded != cp {
                assert_eq!(listed.next(), Some(&(cp, folded)), "{cp:#x}");
            }
        }
        assert!(listed.next().is_none());
    }

    #[test]
    fn tables_are_sorted() {
        for runs in [&t::LOWER_RUNS[..], &t::UPPER_RUNS, &t::FOLD_RUNS] {
            for pair in runs.windows(2) {
                assert!(pair[0].1 < pair[1].0, "{pair:?}");
            }
        }
        for table in [&t::LOWER_SPECIAL[..], &t::UPPER_SPECIAL] {
            for pair in table.windows(2) {
                assert!(pair[0].0 < pair[1].0);
            }
        }
    }
}
