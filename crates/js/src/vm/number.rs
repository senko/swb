//! Number to string in radix 10 (§6.1.6.1.20 `Number::toString`) and
//! string to number (§7.1.4.1.1 `StringToNumber`), ADR 0026 section 7.
//!
//! `Number::toString` takes the shortest digits that round-trip from Rust's
//! `{:e}` formatting (it prints the shortest representation and the
//! exponent) and lays them out as the specification requires.
//!
//! `StringToNumber`: own code checks the `StringNumericLiteral` syntax; valid
//! decimal text goes to Rust's float parser, which rounds correctly. The
//! exact version with all its edge cases is M7 feature 4.

use swb_js_text::{RadixAccumulator, Str16, unicode};

/// `Number::toString(x)` in radix 10 (§6.1.6.1.20).
pub(crate) fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    // `{:e}` gives "d.ddde±x" with the shortest round-trip digits.
    let shortest = format!("{x:e}");
    // The shortest form breaks exact ties upward; the specification wants
    // the closest digit string with the same count, the even one on a tie.
    // Exact rounding at that digit count rounds half to even.
    let formatted = match digit_count(&shortest) {
        Some(n) => {
            let exact = format!("{:.*e}", n.saturating_sub(1), x);
            if exact.parse::<f64>().is_ok_and(|v| v == x) {
                exact
            } else {
                shortest
            }
        }
        None => shortest,
    };
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let k = digits.len() as i32;
    // x = 0.digits × 10^n.
    let n = exponent + 1;
    let mut out = String::new();
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        out.push_str(int);
        out.push('.');
        out.push_str(frac);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// The number of significant digits in a `{:e}` string.
fn digit_count(formatted: &str) -> Option<usize> {
    let (mantissa, _) = formatted.split_once('e')?;
    Some(mantissa.chars().filter(char::is_ascii_digit).count())
}

/// Whether a code unit is `StrWhiteSpaceChar` (§7.1.4.1: `WhiteSpace` and
/// `LineTerminator`).
fn is_str_whitespace(unit: u16) -> bool {
    let unit = u32::from(unit);
    unicode::is_whitespace(unit) || unicode::is_line_terminator(unit)
}

/// `StringToNumber` (§7.1.4.1.1): NaN for text that is not a
/// `StringNumericLiteral`.
pub(crate) fn string_to_number(text: Str16<'_>) -> f64 {
    let units: Vec<u16> = text.units().collect();
    let start = units
        .iter()
        .position(|&u| !is_str_whitespace(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_str_whitespace(u))
        .map_or(start, |p| p + 1);
    let Some(trimmed) = units.get(start..end) else {
        return 0.0;
    };
    if trimmed.is_empty() {
        return 0.0;
    }
    // Only ASCII can be part of a numeric literal.
    let Some(ascii) = trimmed
        .iter()
        .map(|&u| u8::try_from(u).ok().filter(u8::is_ascii))
        .collect::<Option<Vec<u8>>>()
    else {
        return f64::NAN;
    };
    let Ok(text) = std::str::from_utf8(&ascii) else {
        return f64::NAN;
    };
    // NonDecimalIntegerLiteral: no sign allowed.
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return parse_radix(digits, radix);
        }
    }
    let (sign, unsigned) = match text.as_bytes().first() {
        Some(b'+') => (1.0, &text[1..]),
        Some(b'-') => (-1.0, &text[1..]),
        _ => (1.0, text),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal_literal(unsigned) {
        return f64::NAN;
    }
    unsigned.parse::<f64>().map_or(f64::NAN, |v| sign * v)
}

/// `StrUnsignedDecimalLiteral` without `Infinity`: digits with an optional
/// fraction (at least one digit in all) and an optional exponent.
fn is_decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let int_digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = bytes
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_digits = bytes
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == bytes.len()
}

/// The value of digits in a radix (at least one digit, no separators),
/// exact and rounded to nearest, ties to even, like a numeric literal.
fn parse_radix(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut value = RadixAccumulator::new(radix);
    for c in digits.chars() {
        let Some(digit) = c.to_digit(radix) else {
            return f64::NAN;
        };
        value.push(digit);
    }
    value.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_layouts() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (123.456, "123.456"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (0.000_001, "0.000001"),
            (1.23e-18, "1.23e-18"),
            (5e-324, "5e-324"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0 / 3.0, "33.333333333333336"),
            (1e100, "1e+100"),
            (f64::MAX, "1.7976931348623157e+308"),
            (2f64.powi(53), "9007199254740992"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (x, expected) in cases {
            assert_eq!(number_to_string(x), expected, "{x:e}");
        }
    }

    #[test]
    fn string_to_number_syntax() {
        let n = |s: &str| string_to_number(swb_js_text::String16::from(s).as_str16());
        assert_eq!(n(""), 0.0);
        assert_eq!(n("  12  "), 12.0);
        assert_eq!(n("1e3"), 1000.0);
        assert_eq!(n(".5"), 0.5);
        assert_eq!(n("5."), 5.0);
        assert_eq!(n("+5"), 5.0);
        assert!(n("-0x10").is_nan());
        assert_eq!(n("0x10"), 16.0);
        assert_eq!(n("0b11"), 3.0);
        assert_eq!(n("0o17"), 15.0);
        assert_eq!(n("-Infinity"), f64::NEG_INFINITY);
        assert!(n("infinity").is_nan());
        assert!(n("1_0").is_nan());
        assert!(n("1e").is_nan());
        assert!(n(".").is_nan());
        assert!(n("inf").is_nan());
        assert!(n("NaN").is_nan());
        assert_eq!(n("\u{3000}7\u{FEFF}"), 7.0);
        assert!(n("12px").is_nan());
        assert!(n("-0").is_sign_negative());
    }

    #[test]
    fn string_to_number_radix_is_exact() {
        // Expected values from Node.js 22 (`Number(text)`).
        let n = |s: &str| string_to_number(swb_js_text::String16::from(s).as_str16());
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
