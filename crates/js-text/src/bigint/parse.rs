//! Text to `BigInt`: `StringToBigInt` (§7.1.14,
//! <https://tc39.es/ecma262/#sec-stringtobigint>) with the
//! `StringIntegerLiteral` grammar, and the value of a `BigInt` literal
//! (§12.9.3, <https://tc39.es/ecma262/#sec-literals-numeric-literals>) as
//! the lexer keeps it: the digits with the radix prefix (`0x`, `0o`,
//! `0b`), without separators and without the `n`.
//!
//! The digits are checked in one pass, then accumulated: in a radix that
//! is a power of two by bit groups; in the others as many digits per limb
//! step as fit in 64 bits (`acc × radix^k + chunk`), quadratic in the
//! number of digits. A digit count whose value is certainly above
//! [`MAX_BITS`] is an error before any arithmetic.

use std::cmp::Ordering;

use super::{BigInt, BigIntError, MAX_BITS, Result, check_bits, limbs};
use crate::number::is_str_whitespace;
use crate::string16::{CodeUnit, Str16};

/// `StringToBigInt` (§7.1.14): [`BigIntError::InvalidSyntax`] for text that
/// is not a `StringIntegerLiteral` (the specification's undefined).
/// White space around the integer is allowed; the empty text and white
/// space alone are 0; a sign is allowed only before decimal digits;
/// numeric separators are not allowed.
pub fn string_to_bigint(text: Str16<'_>) -> Result<BigInt> {
    match text {
        Str16::Latin1(units) => parse_units(units, true),
        Str16::Wide(units) => parse_units(units, true),
    }
}

/// The value of a `BigInt` literal's text: decimal digits, or `0x`, `0o`
/// or `0b` and digits (also with an upper-case prefix letter); no sign,
/// no white space, no separators.
pub fn parse_literal(text: Str16<'_>) -> Result<BigInt> {
    match text {
        Str16::Latin1(units) => parse_units(units, false),
        Str16::Wide(units) => parse_units(units, false),
    }
}

/// The order of `x` and the value of `text` by `StringToBigInt`, for
/// `IsLessThan` and `IsLooselyEqual` with a `BigInt` and a String
/// (§7.2.13 step 3, §7.2.14 steps 6 and 7): `None` if the text is not a
/// `StringIntegerLiteral` (undefined). A valid text whose value is above
/// the size limit is larger in magnitude than every `BigInt`, so it
/// compares by its sign without an error.
pub fn compare_text(x: &BigInt, text: Str16<'_>) -> Result<Option<Ordering>> {
    match string_to_bigint(text) {
        Ok(y) => Ok(Some(x.cmp(&y))),
        Err(BigIntError::InvalidSyntax) => Ok(None),
        Err(BigIntError::TooLarge) => {
            let negative = match text {
                Str16::Latin1(units) => trim(units).first().is_some_and(|&u| byte(u) == b'-'),
                Str16::Wide(units) => trim(units).first().is_some_and(|&u| byte(u) == b'-'),
            };
            Ok(Some(if negative {
                Ordering::Greater
            } else {
                Ordering::Less
            }))
        }
        Err(error) => Err(error),
    }
}

/// The ASCII byte of a unit, or 0.
fn byte<U: CodeUnit>(unit: U) -> u8 {
    u8::try_from(unit.value())
        .ok()
        .filter(u8::is_ascii)
        .unwrap_or(0)
}

/// The value of a digit in radix 36, or 36 for a unit that is no digit.
fn digit_value<U: CodeUnit>(unit: U) -> u32 {
    match byte(unit) {
        b @ b'0'..=b'9' => u32::from(b - b'0'),
        b @ b'a'..=b'z' => u32::from(b - b'a') + 10,
        b @ b'A'..=b'Z' => u32::from(b - b'A') + 10,
        _ => 36,
    }
}

/// The text without white space at both ends.
fn trim<U: CodeUnit>(units: &[U]) -> &[U] {
    let start = units
        .iter()
        .position(|u| !is_str_whitespace(u.value()))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|u| !is_str_whitespace(u.value()))
        .map_or(start, |p| p + 1);
    units.get(start..end).unwrap_or_default()
}

fn parse_units<U: CodeUnit>(units: &[U], string: bool) -> Result<BigInt> {
    let text = if string { trim(units) } else { units };
    if text.is_empty() {
        return if string {
            Ok(BigInt::ZERO)
        } else {
            Err(BigIntError::InvalidSyntax)
        };
    }
    let (negative, radix, digits) = split_literal(text, string);
    if digits.is_empty() || digits.iter().any(|&u| digit_value(u) >= radix) {
        return Err(BigIntError::InvalidSyntax);
    }
    let magnitude = magnitude_of_digits(digits, radix)?;
    Ok(BigInt::from_normalized(negative, magnitude))
}

/// The sign, the radix and the digits of a trimmed literal; a sign only
/// in a string and only before decimal digits.
fn split_literal<U: CodeUnit>(text: &[U], string: bool) -> (bool, u32, &[U]) {
    let first = text.first().map_or(0, |&u| byte(u));
    let second = text.get(1).map_or(0, |&u| byte(u));
    let radix = match (first, second | 0x20) {
        (b'0', b'x') => 16,
        (b'0', b'o') => 8,
        (b'0', b'b') => 2,
        _ => 10,
    };
    if radix != 10 {
        return (false, radix, text.get(2..).unwrap_or_default());
    }
    match first {
        b'+' | b'-' if string => (first == b'-', 10, text.get(1..).unwrap_or_default()),
        _ => (false, 10, text),
    }
}

/// The magnitude of valid digits in `radix`, normalized.
fn magnitude_of_digits<U: CodeUnit>(digits: &[U], radix: u32) -> Result<Vec<u64>> {
    let start = digits
        .iter()
        .position(|&u| digit_value(u) != 0)
        .unwrap_or(digits.len());
    let digits = digits.get(start..).unwrap_or_default();
    let Some(&first) = digits.first() else {
        return Ok(Vec::new());
    };
    let count = digits.len() as u64;
    if radix.is_power_of_two() {
        let width = radix.trailing_zeros();
        let bits =
            (count - 1) * u64::from(width) + u64::from(32 - digit_value(first).leading_zeros());
        check_bits(bits)?;
        return pack_bits(digits, width);
    }
    // The value has floor(log2(first digit × radix^(count − 1))) + 1 bits,
    // at least (count − 1) × log2(radix) + 1 and below
    // count × log2(radix) + 1.
    let log2 = f64::from(radix).log2();
    if (count - 1) as f64 * log2 >= MAX_BITS as f64 + 1.0 {
        return Err(BigIntError::TooLarge);
    }
    let capacity = (count as f64 * log2 / 64.0) as usize + 2;
    let mut acc = limbs::try_vec(capacity)?;
    let (chunk_value, per_chunk) = largest_power(radix);
    let head = match digits.len() % per_chunk {
        0 => per_chunk,
        rest => rest,
    };
    let (head, tail) = digits.split_at(head.min(digits.len()));
    acc.push(chunk_of(head, radix));
    for chunk in tail.chunks(per_chunk) {
        let carry = limbs::mul_small_add(&mut acc, chunk_value, chunk_of(chunk, radix));
        if carry != 0 {
            if acc.len() == acc.capacity() {
                return Err(BigIntError::OutOfMemory);
            }
            acc.push(carry);
        }
    }
    limbs::trim(&mut acc);
    check_bits(limbs::bit_len(&acc))?;
    Ok(acc)
}

/// The value of at most `largest_power(radix).1` digits.
fn chunk_of<U: CodeUnit>(digits: &[U], radix: u32) -> u64 {
    digits.iter().fold(0u64, |acc, &u| {
        acc.wrapping_mul(u64::from(radix))
            .wrapping_add(u64::from(digit_value(u)))
    })
}

/// The magnitude of digits in a radix 2^`width` (within the limit), from
/// the lowest digit up.
fn pack_bits<U: CodeUnit>(digits: &[U], width: u32) -> Result<Vec<u64>> {
    let len = (digits.len() as u64 * u64::from(width)).div_ceil(64) as usize;
    let mut out = limbs::try_vec(len)?;
    let mut acc = 0u128;
    let mut filled = 0;
    for &unit in digits.iter().rev() {
        acc |= u128::from(digit_value(unit)) << filled;
        filled += width;
        if filled >= 64 {
            out.push(acc as u64);
            acc >>= 64;
            filled -= 64;
        }
    }
    if filled > 0 {
        out.push(acc as u64);
    }
    limbs::trim(&mut out);
    Ok(out)
}

/// The largest power of `radix` (2 to 36) that fits in a limb, and its
/// exponent.
pub(super) const fn largest_power(radix: u32) -> (u64, usize) {
    let radix = radix as u64;
    let mut power = radix;
    let mut exponent = 1;
    while let Some(next) = power.checked_mul(radix) {
        power = next;
        exponent += 1;
    }
    (power, exponent)
}
