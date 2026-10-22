//! Text to number (ADR 0026 section 7): `StringToNumber` (§7.1.4.1.1),
//! the prefix of `parseFloat` (§19.2.4) and `parseInt` (§19.2.5).
//!
//! Own code checks the syntax. Decimal digits go to Rust's float parser,
//! which rounds correctly, in a compact form: at most [`MAX_DIGITS`]
//! significant digits and a "1" after them if any later digit is not 0
//! (the rest cannot change the rounding: a midpoint between two doubles
//! has at most 767 significant digits), and an exponent clamped to a
//! range beyond the doubles. So the work is one pass over the text and
//! the memory is bounded, also for 16 million digits.
//!
//! Hexadecimal, octal and binary digits, and `parseInt` in the radixes 2,
//! 4, 8, 16 and 32, round exactly ([`RadixAccumulator`]).
//!
//! `parseInt` in the other radixes except 10 gives the exact value of the
//! digit prefix rounded to the nearest double, ties to even, like the
//! radixes 10 and 2^k. (§19.2.5 allows an implementation-approximated
//! integer in these radixes; swb takes the exact one, so that every
//! radix follows the same rule.) Leading zeros are skipped; more
//! significant digits than [`infinite_digits`] mean a value of at least
//! 2^1024, which rounds to infinity without arithmetic; the others (at
//! most about 1,030 bits) accumulate exactly in a [`Natural`], several
//! digits per limb step, and round once. So the work is linear in the
//! text and the memory bounded.

use super::natural::{Natural, largest_power};
use super::radix::RadixAccumulator;
use crate::string16::{CodeUnit, Str16};
use crate::unicode;

/// The significant decimal digits that are kept exactly.
const MAX_DIGITS: usize = 800;

/// 2^53: integers below it are exact doubles.
const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_992;

/// Whether a code unit is `StrWhiteSpaceChar` (§7.1.4.1: `WhiteSpace` and
/// `LineTerminator`), the set of `TrimString`.
pub(crate) fn is_str_whitespace(unit: u16) -> bool {
    let unit = u32::from(unit);
    unicode::is_whitespace(unit) || unicode::is_line_terminator(unit)
}

/// `StringToNumber` (§7.1.4.1.1): NaN for text that is not a
/// `StringNumericLiteral`.
pub fn string_to_number(text: Str16<'_>) -> f64 {
    match text {
        Str16::Latin1(units) => string_to_number_units(units),
        Str16::Wide(units) => string_to_number_units(units),
    }
}

/// `parseFloat(string)` (§19.2.4) of the text after `ToString`.
pub fn parse_float(text: Str16<'_>) -> f64 {
    match text {
        Str16::Latin1(units) => parse_float_units(units),
        Str16::Wide(units) => parse_float_units(units),
    }
}

/// `parseInt(string, radix)` (§19.2.5) of the text after `ToString`;
/// `radix` is `ToInt32(radix)`.
pub fn parse_int(text: Str16<'_>, radix: i32) -> f64 {
    match text {
        Str16::Latin1(units) => parse_int_units(units, radix),
        Str16::Wide(units) => parse_int_units(units, radix),
    }
}

/// The unit at `index` as an ASCII byte, or 0 (also for non-ASCII units).
fn byte_at<U: CodeUnit>(units: &[U], index: usize) -> u8 {
    units
        .get(index)
        .and_then(|u| u8::try_from(u.value()).ok())
        .filter(u8::is_ascii)
        .unwrap_or(0)
}

/// The index of the first unit that is not white space.
fn trim_start<U: CodeUnit>(units: &[U]) -> usize {
    units
        .iter()
        .position(|u| !is_str_whitespace(u.value()))
        .unwrap_or(units.len())
}

/// Whether `units[at..]` starts with the ASCII text `word`.
fn starts_with<U: CodeUnit>(units: &[U], at: usize, word: &[u8]) -> bool {
    word.iter()
        .enumerate()
        .all(|(i, &b)| byte_at(units, at + i) == b)
}

/// The sign at `at` (`+` or `-`): the factor and the index after it.
fn sign_at<U: CodeUnit>(units: &[U], at: usize) -> (f64, usize) {
    match byte_at(units, at) {
        b'-' => (-1.0, at + 1),
        b'+' => (1.0, at + 1),
        _ => (1.0, at),
    }
}

fn string_to_number_units<U: CodeUnit>(units: &[U]) -> f64 {
    let start = trim_start(units);
    let end = units
        .iter()
        .rposition(|u| !is_str_whitespace(u.value()))
        .map_or(start, |p| p + 1);
    let Some(text) = units.get(start..end) else {
        return 0.0;
    };
    if text.is_empty() {
        return 0.0;
    }
    // NonDecimalIntegerLiteral: no sign, at least one digit.
    if byte_at(text, 0) == b'0' {
        let radix = match byte_at(text, 1) | 0x20 {
            b'x' => 16,
            b'o' => 8,
            b'b' => 2,
            _ => 0,
        };
        if radix != 0 {
            let digits = text.get(2..).unwrap_or_default();
            let (count, value) = radix_digits(digits, radix);
            return if count == digits.len() && count > 0 {
                value
            } else {
                f64::NAN
            };
        }
    }
    let (sign, at) = sign_at(text, 0);
    if starts_with(text, at, b"Infinity") && at + 8 == text.len() {
        return sign * f64::INFINITY;
    }
    match scan_unsigned_decimal(text, at) {
        Some((end, value)) if end == text.len() => sign * value,
        _ => f64::NAN,
    }
}

fn parse_float_units<U: CodeUnit>(units: &[U]) -> f64 {
    let (sign, at) = sign_at(units, trim_start(units));
    if starts_with(units, at, b"Infinity") {
        return sign * f64::INFINITY;
    }
    scan_unsigned_decimal(units, at).map_or(f64::NAN, |(_, value)| sign * value)
}

fn parse_int_units<U: CodeUnit>(units: &[U], radix: i32) -> f64 {
    let (sign, mut at) = sign_at(units, trim_start(units));
    let mut radix = radix;
    let mut strip_prefix = true;
    if radix != 0 {
        if !(2..=36).contains(&radix) {
            return f64::NAN;
        }
        strip_prefix = radix == 16;
    } else {
        radix = 10;
    }
    if strip_prefix && byte_at(units, at) == b'0' && byte_at(units, at + 1) | 0x20 == b'x' {
        at += 2;
        radix = 16;
    }
    let digits = units.get(at..).unwrap_or_default();
    let radix = radix.unsigned_abs();
    let value = if radix == 10 {
        let mut decimal = DecimalDigits::default();
        let count = digits
            .iter()
            .take_while(|u| match digit_value(u.value(), 10) {
                Some(d) => {
                    decimal.push(d as u8, false);
                    true
                }
                None => false,
            })
            .count();
        (count > 0).then(|| decimal.finish(0))
    } else if radix.is_power_of_two() {
        let (count, value) = radix_digits(digits, radix);
        (count > 0).then_some(value)
    } else {
        other_radix_digits(digits, radix)
    };
    match value {
        None => f64::NAN,
        Some(value) => sign * value,
    }
}

/// The value of a digit in `radix` (2 to 36, letters of both cases).
fn digit_value(unit: u16, radix: u32) -> Option<u32> {
    char::from_u32(u32::from(unit))?.to_digit(radix)
}

/// The leading digits of `units` in a power-of-two radix: their count and
/// their value, rounded exactly.
fn radix_digits<U: CodeUnit>(units: &[U], radix: u32) -> (usize, f64) {
    let mut value = RadixAccumulator::new(radix);
    let mut count = 0;
    for unit in units {
        let Some(digit) = digit_value(unit.value(), radix) else {
            break;
        };
        value.push(digit);
        count += 1;
    }
    (count, value.finish())
}

/// The value of the leading digits of `units` in a radix that is not a
/// power of two and not 10, rounded to the nearest double (ties to
/// even); `None` without digits. See the module comment.
fn other_radix_digits<U: CodeUnit>(units: &[U], radix: u32) -> Option<f64> {
    let count = units
        .iter()
        .take_while(|u| digit_value(u.value(), radix).is_some())
        .count();
    if count == 0 {
        return None;
    }
    let digits = units.get(..count).unwrap_or_default();
    let zeros = digits
        .iter()
        .take_while(|u| digit_value(u.value(), radix) == Some(0))
        .count();
    let significant = digits.get(zeros..).unwrap_or_default();
    if significant.is_empty() {
        return Some(0.0);
    }
    // n significant digits are at least radix^(n − 1).
    if significant.len() > infinite_digits(radix) {
        return Some(f64::INFINITY);
    }
    // At most `infinite_digits(radix)` = n digits: below radix^n, which is
    // below 2^1024 × radix ≤ 2^1030.
    // Groups of digits whose value fits in a limb.
    let (chunk, per_chunk) = largest_power(radix);
    let mut value = Natural::default();
    for group in significant.chunks(per_chunk as usize) {
        let mut part = 0u32;
        for unit in group {
            part = part * radix + digit_value(unit.value(), radix).unwrap_or(0);
        }
        if group.len() == per_chunk as usize {
            value.mul_small(chunk);
        } else {
            value.mul_pow(radix, group.len() as u32);
        }
        value.add_small(part);
    }
    Some(value.to_f64())
}

/// The number of significant digits in `radix` above which an integer is
/// certainly infinite as a double: the smallest n with
/// radix^n ≥ 2^1024, so that n + 1 digits are at least 2^1024. The
/// floating-point quotient is far from an integer for all radixes from 2
/// to 36 that are not powers of two (the closest are 1024 / log2(33) ≈
/// 202.9976 and 1024 / log2(11) ≈ 296.0024), so its ceiling is exact.
fn infinite_digits(radix: u32) -> usize {
    (1024.0 / f64::from(radix).log2()).ceil() as usize
}

/// `StrUnsignedDecimalLiteral` without `Infinity` (§7.1.4.1) at `start`:
/// the longest prefix and its value, or `None` if there are no digits.
fn scan_unsigned_decimal<U: CodeUnit>(units: &[U], start: usize) -> Option<(usize, f64)> {
    let mut decimal = DecimalDigits::default();
    let mut at = start;
    while let Some(d) = ascii_digit(units, at) {
        decimal.push(d, false);
        at += 1;
    }
    let int_digits = at - start;
    let mut frac_digits = 0;
    if byte_at(units, at) == b'.' && (int_digits > 0 || ascii_digit(units, at + 1).is_some()) {
        at += 1;
        while let Some(d) = ascii_digit(units, at) {
            decimal.push(d, true);
            at += 1;
            frac_digits += 1;
        }
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    let mut exponent = 0i64;
    if byte_at(units, at) | 0x20 == b'e' {
        let (sign, mut digits_at) = match byte_at(units, at + 1) {
            b'-' => (-1, at + 2),
            b'+' => (1, at + 2),
            _ => (1, at + 1),
        };
        if ascii_digit(units, digits_at).is_some() {
            // Saturates far beyond the range of doubles.
            while let Some(d) = ascii_digit(units, digits_at) {
                exponent = (exponent * 10 + i64::from(d)).min(1_000_000_000_000);
                digits_at += 1;
            }
            exponent *= sign;
            at = digits_at;
        }
    }
    Some((at, decimal.finish(exponent)))
}

/// The ASCII digit at `at`, as a value.
fn ascii_digit<U: CodeUnit>(units: &[U], at: usize) -> Option<u8> {
    let b = byte_at(units, at);
    b.is_ascii_digit().then(|| b - b'0')
}

/// The significant digits of a decimal number in a bounded buffer (see
/// the module comment). The first 19 digits accumulate in an integer; only
/// a longer number keeps its digits as text.
#[derive(Default)]
struct DecimalDigits {
    /// The value of the kept digits while there are at most 19.
    small: u64,
    /// The number of kept significant digits (at most [`MAX_DIGITS`]).
    count: usize,
    /// All kept digits as ASCII, once there are more than 19.
    long: Vec<u8>,
    /// The value is the kept digits (an integer) × 10^`scale`.
    scale: i64,
    /// A digit after the kept ones is not 0.
    sticky: bool,
}

impl DecimalDigits {
    /// Appends a digit of the integer part or of the fraction.
    fn push(&mut self, digit: u8, fraction: bool) {
        if self.count == 0 && digit == 0 {
            // A leading zero.
            if fraction {
                self.scale -= 1;
            }
        } else if self.count < MAX_DIGITS {
            if self.count < 19 {
                self.small = self.small * 10 + u64::from(digit);
            } else {
                if self.long.is_empty() {
                    self.long.extend(self.small.to_string().bytes());
                }
                self.long.push(b'0' + digit);
            }
            self.count += 1;
            if fraction {
                self.scale -= 1;
            }
        } else {
            if !fraction {
                self.scale += 1;
            }
            self.sticky |= digit != 0;
        }
    }

    /// The value times 10^`exponent`, rounded to nearest, ties to even.
    fn finish(self, exponent: i64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let mut power = self.scale.saturating_add(exponent);
        // Clinger's fast path: an exact integer times an exact power of
        // ten rounds once.
        if self.count <= 19 && self.small < MAX_EXACT_INTEGER && (-22..=22).contains(&power) {
            let value = self.small as f64;
            let scale = 10f64.powi(power.unsigned_abs() as i32);
            return if power >= 0 {
                value * scale
            } else {
                value / scale
            };
        }
        let mut text = if self.count <= 19 {
            self.small.to_string()
        } else {
            self.long.iter().map(|&b| char::from(b)).collect()
        };
        if self.sticky {
            text.push('1');
            power -= 1;
        }
        let top = power.saturating_add(text.len() as i64);
        // The value is below 10^top and at least 10^(top - 1).
        if top > 400 {
            return f64::INFINITY;
        }
        if top < -400 {
            return 0.0;
        }
        text.push('e');
        text.push_str(&power.to_string());
        text.parse().unwrap_or(f64::NAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::String16;

    fn n(s: &str) -> f64 {
        string_to_number(String16::from(s).as_str16())
    }

    fn pf(s: &str) -> f64 {
        parse_float(String16::from(s).as_str16())
    }

    fn pi(s: &str, radix: i32) -> f64 {
        parse_int(String16::from(s).as_str16(), radix)
    }

    fn same(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    #[test]
    fn string_to_number_syntax() {
        // Expected values from Node.js 22 (`Number(text)`).
        let cases: &[(&str, f64)] = &[
            ("", 0.0),
            ("  12  ", 12.0),
            ("1e3", 1000.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("+5", 5.0),
            ("-0x10", f64::NAN),
            ("+0x10", f64::NAN),
            ("0x10", 16.0),
            ("0X1f", 31.0),
            ("0b11", 3.0),
            ("0o17", 15.0),
            ("0x", f64::NAN),
            ("0b2", f64::NAN),
            ("-Infinity", f64::NEG_INFINITY),
            ("+Infinity", f64::INFINITY),
            ("Infinityx", f64::NAN),
            ("infinity", f64::NAN),
            ("1_0", f64::NAN),
            ("1e", f64::NAN),
            ("1e+", f64::NAN),
            (".", f64::NAN),
            ("e5", f64::NAN),
            (".e5", f64::NAN),
            ("inf", f64::NAN),
            ("NaN", f64::NAN),
            ("\u{3000}7\u{FEFF}", 7.0),
            ("\u{2028}\u{a0}8\n\t", 8.0),
            ("\u{180e}1", f64::NAN),
            ("12px", f64::NAN),
            ("-0", -0.0),
            ("-.0e5", -0.0),
            ("00012", 12.0),
            ("1.2.3", f64::NAN),
            ("1e99999999999", f64::INFINITY),
            ("1e-99999999999", 0.0),
            ("-1e400", f64::NEG_INFINITY),
            ("2.4703282292062328e-324", 5e-324),
            ("2.4703282292062327e-324", 0.0),
            ("1.7976931348623158e308", f64::MAX),
            ("1.7976931348623159e308", f64::INFINITY),
            ("9007199254740993", 9_007_199_254_740_992.0),
            (
                "9007199254740993.0000000000000000000000001",
                9_007_199_254_740_994.0,
            ),
            ("0.1", 0.1),
            ("123456789012345678901234567890", 1.234_567_890_123_456_8e29),
        ];
        for &(text, expected) in cases {
            assert!(same(n(text), expected), "{text:?}: {}", n(text));
        }
        // Many zeros before and after the digits, and long exponents.
        let zeros = "0".repeat(5000);
        assert!(same(n(&format!("{zeros}1{zeros}e-5000")), 1.0));
        assert!(same(n(&format!("0.{zeros}1e5001")), 1.0));
        assert!(same(n(&format!("0.{zeros}1e400")), 0.0));
        assert!(same(n(&format!("1{zeros}")), f64::INFINITY));
        assert!(same(n(&format!("1.{zeros}")), 1.0));
        // A tie broken by a digit far behind the kept digits.
        let tie = format!("9007199254740993.{}1", "0".repeat(2000));
        assert!(same(n(&tie), 9_007_199_254_740_994.0));
        let tie = format!("9007199254740993.{}", "0".repeat(2000));
        assert!(same(n(&tie), 9_007_199_254_740_992.0));
    }

    #[test]
    fn parse_float_prefixes() {
        let cases: &[(&str, f64)] = &[
            ("  2.5abc", 2.5),
            ("-.5", -0.5),
            ("-0", -0.0),
            ("1e", 1.0),
            ("1e+", 1.0),
            ("1e+x", 1.0),
            ("1.e3", 1000.0),
            (".e1", f64::NAN),
            ("+-1", f64::NAN),
            ("Infinityx", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("infinity", f64::NAN),
            ("0x10", 0.0),
            ("1_000", 1.0),
            ("", f64::NAN),
            ("\u{feff}\u{2029}42", 42.0),
            ("5..", 5.0),
        ];
        for &(text, expected) in cases {
            assert!(same(pf(text), expected), "{text:?}: {}", pf(text));
        }
    }

    #[test]
    fn parse_int_rules() {
        // Expected values from Node.js 22.
        let cases: &[(&str, i32, f64)] = &[
            ("  0x1f", 16, 31.0),
            ("0x1f", 0, 31.0),
            ("0x1f", 15, 0.0),
            ("-0x1f", 0, -31.0),
            ("0x", 0, f64::NAN),
            ("-0", 0, -0.0),
            ("1e3", 0, 1.0),
            ("z", 36, 35.0),
            ("Z", 36, 35.0),
            ("12", 1, f64::NAN),
            ("12", 37, f64::NAN),
            ("12", -1, f64::NAN),
            ("\u{180e}1", 0, f64::NAN),
            ("12345678901234567890123", 10, 1.234_567_890_123_456_8e22),
            (
                "1111111111111111111111111111111111111111",
                3,
                6_078_832_729_528_464_000.0,
            ),
            ("zzzzzzzzzzzzzzzzzzzz", 36, 1.336_749_453_884_373_4e31),
            ("20000000000001", 16, 9_007_199_254_740_992.0),
            ("20000000000003", 16, 9_007_199_254_740_996.0),
            ("3333333333333333333333333333", 4, 72_057_594_037_927_936.0),
            // Exact values where Node.js 22 differs (radix-reference.js):
            // Node.js gives 39177157694427710 and 5.6358546387436854e48.
            ("312040012022424233141333", 5, 39_177_157_694_427_720.0),
            (
                "e37c72fe62g43c2ak9ji3ii0baki56i6aj8d0",
                21,
                5.635_854_638_743_686e48,
            ),
            // 2^53 + 1 and 2^53 + 3 in radix 3: ties to even.
            (
                "1121202011211211122211100012101120",
                3,
                9_007_199_254_740_992.0,
            ),
            (
                "1121202011211211122211100012101122",
                3,
                9_007_199_254_740_996.0,
            ),
            ("00000000000000000000000000000000000000000012", 7, 9.0),
        ];
        for &(text, radix, expected) in cases {
            assert!(
                same(pi(text, radix), expected),
                "{text:?} {radix}: {}",
                pi(text, radix)
            );
        }
        // 2^1024 − 2^970 rounds to infinity (the tie goes to the even
        // 2^1024); one less rounds to MAX_VALUE (Node.js 22: Infinity).
        let limit = "42326224003605451313404606424603102545256425310410560632111456064261463155124363530353223121450601021315163560425426035266656456521322425256322252312051120324133001605113564212256352016400046306146640232033351220111665223061450050236615355603242164633651232540032342460310154006024460325161415423324626023355412513241145410440216352546633552655124640225260521663410";
        assert!(same(pi(limit, 7), f64::INFINITY));
        let below = format!("{}06", &limit[..limit.len() - 2]);
        assert!(same(pi(&below, 7), f64::MAX));
        // More significant digits than the cut-off: infinity without
        // arithmetic, also after many leading zeros.
        let many = format!("{}1{}", "0".repeat(5000), "0".repeat(infinite_digits(7)));
        assert!(same(pi(&many, 7), f64::INFINITY));
        let fewer = format!(
            "{}1{}",
            "0".repeat(5000),
            "0".repeat(infinite_digits(7) - 2)
        );
        assert!(same(
            pi(&fewer, 7),
            7f64.powi(infinite_digits(7) as i32 - 2)
        ));
    }

    #[test]
    fn string_to_number_radix_is_exact() {
        // Expected values from Node.js 22 (`Number(text)`).
        let n = |s: &str| string_to_number(String16::from(s).as_str16());
        // 2^53 + 1 is a tie and rounds down to the even 2^53.
        assert_eq!(n("0x20000000000001"), 9_007_199_254_740_992.0);
        // 2^53 + 3 is a tie and rounds up to the even 2^53 + 4.
        assert_eq!(n("0x20000000000003"), 9_007_199_254_740_996.0);
        assert_eq!(n("0x1fffffffffffff1"), 144_115_188_075_855_860.0);
        assert_eq!(n("0o7777777777777777777777"), 73_786_976_294_838_210_000.0);
        assert_eq!(
            n("0B111111111111111111111111111111111111111111111111111111"),
            18_014_398_509_481_984.0
        );
        assert_eq!(n(&format!("0x{}", "f".repeat(256))), f64::INFINITY);
        assert_eq!(n(&format!("0x1{}", "0".repeat(255))), 2f64.powi(1020));
    }
}
