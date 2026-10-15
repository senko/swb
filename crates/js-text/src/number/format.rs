//! Number to text (ADR 0026 section 7): `Number::toString` in radix 10
//! and in the other radixes, `toFixed`, `toExponential`, `toPrecision`
//! and the `toLocaleString` of the en-US locale.
//!
//! - Radix 10, shortest digits (§6.1.6.1.20): Rust's `{:e}` formatting
//!   gives the shortest digits that round-trip and the exponent.
//! - `toFixed`, `toExponential` and `toPrecision` with a digit count
//!   (§21.1.3.2, §21.1.3.3, §21.1.3.5): exact digits of the double, from
//!   multi-precision arithmetic ([`super::natural`]) in the manner of the
//!   fixed-format digit generation of Steele and White (1990), with a tie
//!   rounded to the larger value as the specification requires.
//! - Other radixes (§6.1.6.1.20 leaves the digits to the implementation):
//!   the rule of radix 10 with the radix in place of 10, the shortest
//!   digits that read back as the double, the closest of them, on a tie
//!   the one whose digits form an even integer, from the free-format
//!   algorithm of Burger and Dybvig (1996) ([`super::shortest`]); written
//!   out positionally, without an exponent.
//! - `toLocaleString` without ECMA-402: Chromium's output for the en-US
//!   locale without options (measured, ADR 0026 section 15).

use super::natural::Natural;
use super::shortest::shortest_radix_digits;

/// The digits of the radixes 2 to 36.
const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `Number::toString(x)` in radix 10 (§6.1.6.1.20).
pub fn number_to_string(x: f64) -> String {
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
    let (digits, n) = shortest_digits(x);
    let k = digits.len() as i32;
    let mut out = String::with_capacity(digits.len() + 8);
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
        push_exponent(&mut out, n - 1);
    }
    out
}

/// The shortest decimal digits of a positive finite `x` that round-trip
/// and the position `n` of the decimal point (x ≈ 0.ddd × 10^n).
/// Among the shortest digit strings it takes the one closest to `x`, the
/// even one on a tie (the recommendation of §6.1.6.1.20, note 2).
pub fn shortest_digits(x: f64) -> (String, i32) {
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
    (digits, exponent + 1)
}

/// The number of significant digits in a `{:e}` string.
fn digit_count(formatted: &str) -> Option<usize> {
    let (mantissa, _) = formatted.split_once('e')?;
    Some(mantissa.chars().filter(char::is_ascii_digit).count())
}

/// Appends "e+n" or "e-n".
fn push_exponent(out: &mut String, exponent: i32) {
    out.push('e');
    out.push(if exponent >= 0 { '+' } else { '-' });
    out.push_str(&exponent.unsigned_abs().to_string());
}

/// The mantissa and exponent of a positive finite double: x = m × 2^e.
pub(crate) fn decompose(x: f64) -> (u64, i32) {
    let bits = x.to_bits();
    let fraction = bits & ((1 << 52) - 1);
    let biased = ((bits >> 52) & 0x7FF) as i32;
    if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    }
}

/// Where [`round_decimal`] rounds.
#[derive(Clone, Copy, Debug)]
enum Rounding {
    /// To this many significant digits.
    Significant(u32),
    /// To this many digits after the decimal point.
    Fraction(u32),
}

/// The exact decimal digits of a positive finite `x`, rounded as
/// `rounding` says, half away from zero (a tie gives the larger value).
/// Returns the digit values and the power of ten of the first digit;
/// for `Fraction`, no digits mean that the value rounds to 0.
fn round_decimal(x: f64, rounding: Rounding) -> (Vec<u8>, i32) {
    let (mantissa, exponent) = decompose(x);
    // x = num / den exactly.
    let mut num = Natural::from_u64(mantissa);
    let mut den = Natural::from_u64(1);
    if exponent >= 0 {
        num.shl(exponent.unsigned_abs());
    } else {
        den.shl(exponent.unsigned_abs());
    }
    // Scale to 1 ≤ num / den < 10 with x = num / den × 10^k; the estimate of k is
    // at most one off.
    let mut k = x.log10().floor() as i32;
    if k >= 0 {
        den.mul_pow10(k.unsigned_abs());
    } else {
        num.mul_pow10(k.unsigned_abs());
    }
    loop {
        let mut ten_den = den.clone();
        ten_den.mul_small(10);
        if num < ten_den {
            break;
        }
        den = ten_den;
        k += 1;
    }
    while num < den {
        num.mul_small(10);
        k -= 1;
    }
    let count = match rounding {
        Rounding::Significant(p) => i64::from(p),
        Rounding::Fraction(f) => i64::from(k) + 1 + i64::from(f),
    };
    let last = match rounding {
        Rounding::Fraction(f) => -(f as i32),
        Rounding::Significant(_) => 0,
    };
    if count < 0 {
        return (Vec::new(), last);
    }
    if count == 0 {
        // x is below one unit of the last place: 1 if it is at least half
        // of it (x / 10^(k+1) ≥ 0.5), else 0.
        let mut five_den = den.clone();
        five_den.mul_small(5);
        return if num >= five_den {
            (vec![1], k + 1)
        } else {
            (Vec::new(), last)
        };
    }
    let mut digits = Vec::with_capacity(count as usize + 1);
    for i in 0..count {
        if i > 0 {
            num.mul_small(10);
        }
        digits.push(num.quotient_digit(&den) as u8);
    }
    // The rest num / den is below 1: round up if it is at least one half.
    num.shl(1);
    if num >= den && increment(&mut digits) {
        // All nines: 10…0, one place higher.
        digits.insert(0, 1);
        if matches!(rounding, Rounding::Significant(_)) {
            digits.pop();
        }
        k += 1;
    }
    (digits, k)
}

/// Adds one to the last digit with carries; true if the carry leaves the
/// first digit (all digits were 9 and are 0 now).
fn increment(digits: &mut [u8]) -> bool {
    for digit in digits.iter_mut().rev() {
        if *digit == 9 {
            *digit = 0;
        } else {
            *digit += 1;
            return false;
        }
    }
    true
}

/// Decimal digit values as text.
fn decimal_text(digits: &[u8]) -> String {
    digits.iter().map(|&d| char::from(b'0' + d)).collect()
}

/// `Number.prototype.toFixed` (§21.1.3.3) for a finite `x` and
/// `fraction_digits` ≤ 100.
pub fn to_fixed(x: f64, fraction_digits: u32) -> String {
    let mut out = String::new();
    let mut x = x;
    if x < 0.0 {
        out.push('-');
        x = -x;
    }
    if x >= 1e21 {
        out.push_str(&number_to_string(x));
        return out;
    }
    let f = fraction_digits as usize;
    let mut m = if x == 0.0 {
        String::new()
    } else {
        decimal_text(&round_decimal(x, Rounding::Fraction(fraction_digits)).0)
    };
    if m.is_empty() {
        m.push('0');
    }
    if f != 0 {
        if m.len() <= f {
            let zeros = "0".repeat(f + 1 - m.len());
            m.insert_str(0, &zeros);
        }
        m.insert(m.len() - f, '.');
    }
    out.push_str(&m);
    out
}

/// `Number.prototype.toExponential` (§21.1.3.2) for a finite `x`; with
/// `None` for an undefined argument (as many digits as needed), else
/// `fraction_digits` ≤ 100.
pub fn to_exponential(x: f64, fraction_digits: Option<u32>) -> String {
    let mut out = String::new();
    let mut x = x;
    if x < 0.0 {
        out.push('-');
        x = -x;
    }
    let (digits, e) = if x == 0.0 {
        ("0".repeat(fraction_digits.unwrap_or(0) as usize + 1), 0)
    } else if let Some(f) = fraction_digits {
        let (digits, k) = round_decimal(x, Rounding::Significant(f + 1));
        (decimal_text(&digits), k)
    } else {
        let (digits, n) = shortest_digits(x);
        (digits, n - 1)
    };
    let (first, rest) = digits.split_at(1);
    out.push_str(first);
    if !rest.is_empty() {
        out.push('.');
        out.push_str(rest);
    }
    push_exponent(&mut out, e);
    out
}

/// `Number.prototype.toPrecision` (§21.1.3.5) for a finite `x` and
/// 1 ≤ `precision` ≤ 100.
pub fn to_precision(x: f64, precision: u32) -> String {
    let mut out = String::new();
    let mut x = x;
    if x < 0.0 {
        out.push('-');
        x = -x;
    }
    let p = precision as i32;
    let (m, e) = if x == 0.0 {
        ("0".repeat(precision as usize), 0)
    } else {
        let (digits, k) = round_decimal(x, Rounding::Significant(precision));
        (decimal_text(&digits), k)
    };
    if e < -6 || e >= p {
        let (first, rest) = m.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        push_exponent(&mut out, e);
        return out;
    }
    if e == p - 1 {
        out.push_str(&m);
    } else if e >= 0 {
        let (int, frac) = m.split_at(e as usize + 1);
        out.push_str(int);
        out.push('.');
        out.push_str(frac);
    } else {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-(e + 1)) as usize));
        out.push_str(&m);
    }
    out
}

/// `Number::toString(x, radix)` (§6.1.6.1.20) for a radix from 2 to 36
/// other than 10: the shortest digits that read back as `x`, the closest
/// of them, on a tie the even integer ([`shortest_radix_digits`]),
/// written out without an exponent: "-" for a negative value, the
/// digits and zeros up to the point, or "0." and zeros before them.
/// Another radix gives the radix 10 form.
pub fn radix_string(x: f64, radix: u32) -> String {
    if !(2..=36).contains(&radix) || radix == 10 || !x.is_finite() || x == 0.0 {
        return number_to_string(x);
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let (digits, n) = shortest_radix_digits(x.abs(), radix);
    let text = |digits: &[u8]| -> String {
        digits
            .iter()
            .map(|&d| char::from(DIGITS[usize::from(d) % 36]))
            .collect()
    };
    let k = digits.len() as i32;
    if n >= k {
        out.push_str(&text(&digits));
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if n > 0 {
        let (int, frac) = digits.split_at(n as usize);
        out.push_str(&text(int));
        out.push('.');
        out.push_str(&text(frac));
    } else {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&text(&digits));
    }
    out
}

/// `Number.prototype.toLocaleString()` as Chromium prints it for the
/// en-US locale without options (measured in Chromium 148): the shortest
/// round-trip digits rounded half away from zero to at most three
/// fraction digits, trailing zeros removed, the integer part in groups of
/// three separated by ","; "NaN", "∞", and a "-" for every negative value,
/// also -0 and values that round to 0.
pub fn locale_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    let mut out = String::new();
    if x.is_sign_negative() {
        out.push('-');
    }
    let x = x.abs();
    if x.is_infinite() {
        out.push('∞');
        return out;
    }
    if x == 0.0 {
        out.push('0');
        return out;
    }
    let (digits, n) = shortest_digits(x);
    // The digit values from the first integer place to the third fraction
    // place. Place p has the power 10^(int_len - 1 - p); digit i of the
    // shortest digits has 10^(n - 1 - i).
    let mut places: Vec<u8> = Vec::new();
    let int_len = n.max(1) as usize;
    let shift = int_len as i64 - i64::from(n);
    let at = |place: usize| -> u8 {
        let index = place as i64 - shift;
        if index < 0 {
            0
        } else {
            digits
                .as_bytes()
                .get(index as usize)
                .map_or(0, |&b| b - b'0')
        }
    };
    for place in 0..int_len + 3 {
        places.push(at(place));
    }
    let round_up = at(int_len + 3) >= 5;
    let mut int_len = int_len;
    if round_up && increment(&mut places) {
        places.insert(0, 1);
        int_len += 1;
    }
    let (int, frac) = places.split_at(int_len);
    let first = int.iter().position(|&d| d != 0).unwrap_or(int.len() - 1);
    let int = &int[first..];
    for (i, &d) in int.iter().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(char::from(b'0' + d));
    }
    let frac_len = frac.iter().rposition(|&d| d != 0).map_or(0, |p| p + 1);
    if frac_len > 0 {
        out.push('.');
        out.push_str(&decimal_text(&frac[..frac_len]));
    }
    out
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
}
