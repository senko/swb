//! The order of `String.prototype.localeCompare` (ECMA-262 §22.1.3.12)
//! without `Intl` (ADR 0026 section 15).
//!
//! The specification requires a consistent total order in which
//! canonically equivalent strings compare as equal. swb compares the two
//! strings in their canonical decomposition (NFD, from [`super::normalize`])
//! in four levels; a level decides only if all earlier ones are equal:
//!
//! 1. The base characters: combining marks (`Canonical_Combining_Class`
//!    not 0) are left out, letters are compared by their simple case
//!    folding, and the classes come in the order of the Unicode Collation
//!    Algorithm's default table for the common cases (UTS #10: white
//!    space, punctuation and symbols, digits, letters; the ASCII
//!    punctuation in the order that Chromium shows). A few letters that
//!    are not a base letter and a mark in Unicode (`æ`, `œ`, `ß`, `ø`, `đ`,
//!    `ł`, `ħ`, `ı`) stand for their base letters. Control characters
//!    are ignored.
//! 2. The marks, each at its position among the base characters. The
//!    marks U+0300 to U+036F are in the order that Chromium shows.
//! 3. The case of each base character: lower case before upper case.
//! 4. The code points of the decomposed strings.
//!
//! This is not the Unicode Collation Algorithm: there are no contractions,
//! no tailoring and no weights for scripts other than the rule "by code
//! point after folding". The measured agreement with Chromium is in the
//! ADR.

use std::cell::RefCell;
use std::cmp::Ordering;

use super::case::simple_fold;
use super::normalize::combining_class;
use crate::Str16;

/// The ASCII characters that are not letters or digits, in the order of
/// the default collation: white space first, then punctuation and symbols.
const ASCII_PUNCTUATION: &[u8] = b"\t\n\x0B\x0C\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// The marks U+0300 to U+036F in the order of the default collation.
const MARK_ORDER: [u16; 112] = [
    0x34F, 0x332, 0x313, 0x343, 0x314, 0x301, 0x341, 0x300, 0x340, 0x306, 0x302, 0x30C, 0x30A,
    0x342, 0x308, 0x344, 0x30B, 0x303, 0x307, 0x338, 0x327, 0x328, 0x304, 0x30D, 0x30E, 0x312,
    0x315, 0x31A, 0x33D, 0x33E, 0x33F, 0x346, 0x34A, 0x34B, 0x34C, 0x350, 0x351, 0x352, 0x357,
    0x35B, 0x35D, 0x35E, 0x316, 0x317, 0x318, 0x319, 0x31C, 0x31D, 0x31E, 0x31F, 0x320, 0x329,
    0x32A, 0x32B, 0x32C, 0x32F, 0x333, 0x33A, 0x33B, 0x33C, 0x347, 0x348, 0x349, 0x34D, 0x34E,
    0x353, 0x354, 0x355, 0x356, 0x359, 0x35A, 0x35C, 0x35F, 0x362, 0x336, 0x337, 0x335, 0x305,
    0x309, 0x30F, 0x310, 0x311, 0x31B, 0x321, 0x322, 0x323, 0x324, 0x325, 0x326, 0x32D, 0x32E,
    0x330, 0x331, 0x334, 0x339, 0x345, 0x358, 0x360, 0x361, 0x363, 0x368, 0x369, 0x364, 0x36A,
    0x365, 0x36B, 0x366, 0x36C, 0x36D, 0x367, 0x36E, 0x36F,
];

/// Letters that count as a base letter with a mark: `(code point, base
/// letter, mark weight)`.
const MARKED_LETTERS: [(u32, u8, u32); 8] = [
    (0xF8, b'o', 1),  // ø
    (0x111, b'd', 2), // đ
    (0x127, b'h', 3), // ħ
    (0x131, b'i', 4), // ı
    (0x142, b'l', 5), // ł
    (0xD8, b'o', 1),  // Ø
    (0x110, b'd', 2), // Đ
    (0x141, b'l', 5), // Ł
];

/// Letters that count as two letters: `(code point, letters)`.
const EXPANSIONS: [(u32, &[u8; 2]); 6] = [
    (0xE6, b"ae"),
    (0xC6, b"ae"),
    (0x153, b"oe"),
    (0x152, b"oe"),
    (0xDF, b"ss"),
    (0x1E9E, b"ss"),
];

/// One element of a string's collation key.
#[derive(Clone, Copy)]
struct Weight {
    /// The primary weight; 0 for a mark.
    primary: u32,
    /// The weight of the mark; 0 for a base character without one.
    secondary: u32,
    /// 1 for an upper case letter.
    tertiary: u8,
}

const SYMBOLS: u32 = 1 << 24;
const DIGITS: u32 = 2 << 24;
const LETTERS: u32 = 3 << 24;

/// The primary weight of a base character (folded), or `None` if it is ignored.
fn primary_weight(cp: u32) -> Option<u32> {
    if cp < 0x80 {
        let byte = cp as u8;
        if let Some(index) = ASCII_PUNCTUATION.iter().position(|&c| c == byte) {
            return Some(SYMBOLS + index as u32);
        }
        return match byte {
            b'0'..=b'9' => Some(DIGITS + u32::from(byte - b'0')),
            b'a'..=b'z' => Some(LETTERS + u32::from(byte - b'a')),
            _ => None, // controls
        };
    }
    Some(match cp {
        0x80..=0x9F => return None,
        0xA0 => SYMBOLS + 5,
        0xA1..=0xA9 | 0xAB..=0xB4 | 0xB6..=0xB9 | 0xBB..=0xBF | 0xD7 | 0xF7 => SYMBOLS + 0x100 + cp,
        0x2000..=0x206F | 0x20A0..=0x20CF | 0x2190..=0x2BFF | 0x3000..=0x303F => {
            SYMBOLS + 0x100 + cp
        }
        _ => LETTERS + 0x100 + cp,
    })
}

fn mark_weight(cp: u32) -> u32 {
    MARK_ORDER
        .iter()
        .position(|&mark| u32::from(mark) == cp)
        .map_or(1000 + cp, |index| 10 + index as u32)
}

/// Counts the work of a comparison and asks the caller, every
/// [`TICK`] code points, whether to go on.
struct Ticker<'t> {
    count: usize,
    hook: &'t mut dyn FnMut(usize) -> bool,
    stopped: bool,
}

/// The number of code points between two calls of the hook.
const TICK: usize = 1 << 15;

impl Ticker<'_> {
    /// Counts one code point. Returns `true` when the comparison must stop.
    fn step(&mut self) -> bool {
        self.count += 1;
        if self.count >= TICK {
            self.stopped |= (self.hook)(self.count);
            self.count = 0;
        }
        self.stopped
    }
}

/// The weights of the code points of a decomposed text.
struct Weights<'a, 't> {
    text: Str16<'a>,
    at: usize,
    /// The second letter of an expansion, to come next.
    pending: Option<Weight>,
    ticker: &'a RefCell<Ticker<'t>>,
}

impl<'a, 't> Weights<'a, 't> {
    fn new(text: Str16<'a>, ticker: &'a RefCell<Ticker<'t>>) -> Self {
        Weights {
            text,
            at: 0,
            pending: None,
            ticker,
        }
    }

    fn next_code_point(&mut self) -> Option<u32> {
        let (cp, next) = self.text.code_point_at(self.at)?;
        self.at = next;
        Some(cp)
    }
}

impl Iterator for Weights<'_, '_> {
    type Item = Weight;

    fn next(&mut self) -> Option<Weight> {
        if let Some(weight) = self.pending.take() {
            return Some(weight);
        }
        loop {
            if self.ticker.borrow_mut().step() {
                return None;
            }
            let cp = self.next_code_point()?;
            if cp >= 0x300 && combining_class(cp) != 0 {
                return Some(Weight {
                    primary: 0,
                    secondary: mark_weight(cp),
                    tertiary: 0,
                });
            }
            let folded = simple_fold(cp);
            let tertiary = u8::from(folded != cp);
            if let Some(&(_, letters)) = EXPANSIONS.iter().find(|entry| entry.0 == cp) {
                let make = |letter: u8| Weight {
                    primary: LETTERS + u32::from(letter - b'a'),
                    secondary: 0,
                    tertiary: tertiary + 2, // after the letters themselves
                };
                self.pending = Some(make(letters[1]));
                return Some(make(letters[0]));
            }
            if let Some(&(_, base, mark)) = MARKED_LETTERS.iter().find(|entry| entry.0 == cp) {
                self.pending = Some(Weight {
                    primary: 0,
                    secondary: mark,
                    tertiary: 0,
                });
                return Some(Weight {
                    primary: LETTERS + u32::from(base - b'a'),
                    secondary: 0,
                    tertiary,
                });
            }
            if let Some(primary) = primary_weight(folded) {
                return Some(Weight {
                    primary,
                    secondary: 0,
                    tertiary,
                });
            }
        }
    }
}

/// Compares two texts that are in NFD (the caller normalizes them).
/// `hook` is called about every [`TICK`] code points with their number;
/// if it returns `true` the comparison stops and the result is `None`
/// (the caller charges time there).
pub fn compare_decomposed(
    a: Str16<'_>,
    b: Str16<'_>,
    hook: &mut dyn FnMut(usize) -> bool,
) -> Option<Ordering> {
    let ticker = RefCell::new(Ticker {
        count: 0,
        hook,
        stopped: false,
    });
    let bases = |text| Weights::new(text, &ticker).filter(|w| w.primary != 0);
    let order = bases(a)
        .map(|w| w.primary)
        .cmp(bases(b).map(|w| w.primary))
        .then_with(|| {
            Weights::new(a, &ticker)
                .map(|w| w.secondary)
                .cmp(Weights::new(b, &ticker).map(|w| w.secondary))
        })
        .then_with(|| {
            bases(a)
                .map(|w| w.tertiary)
                .cmp(bases(b).map(|w| w.tertiary))
        })
        .then_with(|| {
            let tick = |_: &u32| {
                ticker.borrow_mut().step();
            };
            a.code_points()
                .inspect(tick)
                .cmp(b.code_points().inspect(tick))
        });
    let stopped = ticker.borrow().stopped;
    (!stopped).then_some(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::String16;
    use crate::unicode::normalize::{Form, normalize};

    fn cmp(a: &str, b: &str) -> Ordering {
        let a = normalize(String16::from(a).as_str16(), Form::Nfd);
        let b = normalize(String16::from(b).as_str16(), Form::Nfd);
        compare_decomposed(a.as_str16(), b.as_str16(), &mut |_| false).expect("never stops")
    }

    #[test]
    fn letters_ignore_case_first() {
        assert_eq!(cmp("a", "B"), Ordering::Less);
        assert_eq!(cmp("a", "A"), Ordering::Less);
        assert_eq!(cmp("A", "a"), Ordering::Greater);
        assert_eq!(cmp("ab", "AB"), Ordering::Less);
        assert_eq!(cmp("Ab", "aB"), Ordering::Greater);
        assert_eq!(cmp("a", "a"), Ordering::Equal);
        assert_eq!(cmp("abc", "ab"), Ordering::Greater);
    }

    #[test]
    fn accents_after_the_base_letters() {
        assert_eq!(cmp("e", "\u{E9}"), Ordering::Less);
        assert_eq!(cmp("\u{E9}", "f"), Ordering::Less);
        assert_eq!(cmp("resume", "r\u{E9}sum\u{E9}"), Ordering::Less);
        assert_eq!(cmp("\u{E9}", "\u{E8}"), Ordering::Less); // acute before grave
        assert_eq!(cmp("e\u{301}", "\u{E9}"), Ordering::Equal);
        assert_eq!(cmp("\u{1E69}", "s\u{323}\u{307}"), Ordering::Equal);
    }

    #[test]
    fn classes_and_ignored_controls() {
        assert_eq!(cmp("a b", "ab"), Ordering::Less);
        assert_eq!(cmp("_", "-"), Ordering::Less);
        assert_eq!(cmp("$", "0"), Ordering::Less);
        assert_eq!(cmp("9", "a"), Ordering::Less);
        assert_eq!(cmp("a\u{1}b", "ab"), Ordering::Less); // only the last level
        assert_eq!(cmp("\u{20AC}", "1"), Ordering::Less);
        assert_eq!(cmp("z", "\u{3B1}"), Ordering::Less);
        assert_eq!(cmp("\u{3B1}", "\u{430}"), Ordering::Less);
        assert_eq!(cmp("ae", "\u{E6}"), Ordering::Less);
        assert_eq!(cmp("\u{E6}", "af"), Ordering::Less);
        assert_eq!(cmp("ss", "\u{DF}"), Ordering::Less);
    }

    #[test]
    fn is_a_total_order_on_a_sample() {
        let words = [
            "",
            "a",
            "A",
            "b",
            "\u{E9}",
            "e\u{301}",
            "E",
            "\u{3A3}",
            "\u{3C3}",
            "\u{3C2}",
            "z",
            "a b",
            "ab",
            "a\u{1}b",
            "\u{1E69}",
            "\u{AC00}",
            "\u{1100}\u{1161}",
            "\u{10400}",
            "\u{FFFD}",
            "x\u{323}\u{307}",
            "x\u{307}\u{323}",
            "\u{E6}",
            "ae",
        ];
        for a in words {
            assert_eq!(cmp(a, a), Ordering::Equal);
            for b in words {
                assert_eq!(cmp(a, b), cmp(b, a).reverse(), "{a:?} {b:?}");
                for c in words {
                    if cmp(a, b) != Ordering::Greater && cmp(b, c) != Ordering::Greater {
                        assert_ne!(cmp(a, c), Ordering::Greater, "{a:?} {b:?} {c:?}");
                    }
                }
            }
        }
    }
}
