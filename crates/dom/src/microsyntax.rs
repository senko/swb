//! Parsers for the HTML microsyntaxes of attribute values: integers,
//! non-negative integers and floating-point numbers.
//!
//! <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#numbers>

use crate::is_html_whitespace;

/// The rules for parsing integers (leading whitespace, optional sign,
/// digits; trailing text is ignored). Values with more than 18 digits
/// (after leading zeros) saturate to `±i64::MAX`. `-0` is 0.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-integers>
pub fn parse_integer(input: &str) -> Option<i64> {
    let input = input.trim_start_matches(is_html_whitespace);
    let (negative, rest) = match input.as_bytes().first() {
        Some(b'-') => (true, &input[1..]),
        Some(b'+') => (false, &input[1..]),
        _ => (false, input),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let significant = rest[..digits].trim_start_matches('0');
    let value: i64 = if significant.is_empty() {
        0
    } else if significant.len() > 18 {
        i64::MAX
    } else {
        significant.parse().ok()?
    };
    Some(if negative { -value } else { value })
}

/// The rules for parsing non-negative integers: like [`parse_integer`],
/// but a negative result (other than `-0`) is an error.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-negative-integers>
pub fn parse_non_negative_integer(input: &str) -> Option<i64> {
    parse_integer(input).filter(|v| *v >= 0)
}

/// [`parse_non_negative_integer`] for 32-bit attributes: values above
/// `u32::MAX` saturate.
pub fn parse_non_negative_u32(input: &str) -> Option<u32> {
    parse_non_negative_integer(input).map(|v| u32::try_from(v).unwrap_or(u32::MAX))
}

/// True for a valid non-negative integer: one or more ASCII digits.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#valid-non-negative-integer>
pub fn is_valid_non_negative_integer(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// True for a valid floating-point number: an optional `-`, digits with an
/// optional fraction or only a fraction, and an optional exponent.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#valid-floating-point-number>
pub fn is_valid_float(s: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers() {
        assert_eq!(parse_integer(" 42abc"), Some(42));
        assert_eq!(parse_integer("-3"), Some(-3));
        assert_eq!(parse_integer("+3"), Some(3));
        assert_eq!(parse_integer("x"), None);
        assert_eq!(parse_integer(""), None);
        assert_eq!(parse_integer("-"), None);
        assert_eq!(parse_integer("0000000000000000000005"), Some(5));
        assert_eq!(parse_integer("-000"), Some(0));
        assert_eq!(parse_integer("1234567890123456789012"), Some(i64::MAX));
        assert_eq!(parse_integer("-1234567890123456789012"), Some(-i64::MAX));
    }

    #[test]
    fn non_negative_integers() {
        assert_eq!(parse_non_negative_integer(" 12abc"), Some(12));
        assert_eq!(parse_non_negative_integer("+3"), Some(3));
        assert_eq!(parse_non_negative_integer("-0"), Some(0));
        assert_eq!(parse_non_negative_integer("-1"), None);
        assert_eq!(parse_non_negative_integer(""), None);
        assert_eq!(parse_non_negative_u32(" 17px"), Some(17));
        assert_eq!(parse_non_negative_u32("-0"), Some(0));
        assert_eq!(parse_non_negative_u32("99999999999"), Some(u32::MAX));
        assert_eq!(parse_non_negative_u32("-1"), None);
        assert!(is_valid_non_negative_integer("007"));
        assert!(!is_valid_non_negative_integer(""));
        assert!(!is_valid_non_negative_integer("+1"));
        assert!(!is_valid_non_negative_integer("1x"));
    }

    #[test]
    fn floating_point_numbers() {
        for valid in [
            "0", "1", "-1", "1.5", ".5", "-.5", "1e5", "1E+5", "1.5e-3", "1E-5", "-0.5e+2",
        ] {
            assert!(is_valid_float(valid), "{valid}");
        }
        for invalid in [
            "", "-", ".", "1.", "+1", "1e", "1e+", "e5", "1.5.5", "1.2.3", "1x", " 1", "inf",
            "NaN", "0x1",
        ] {
            assert!(!is_valid_float(invalid), "{invalid}");
        }
    }
}
