//! `BigInt` to text: `BigInt::toString` (§6.1.6.2.23,
//! <https://tc39.es/ecma262/#sec-numeric-types-bigint-tostring>) in the
//! radixes 2 to 36, with lower-case letters and a `-` for negative values.
//!
//! A radix that is a power of two reads the digits as bit groups. The
//! others divide the magnitude repeatedly by the largest power of the
//! radix that fits in a limb (one pass over the remaining limbs per
//! chunk of digits, with the reciprocal of `div`), which is quadratic in
//! the length.

use super::div::{Reciprocal, div_rem_limb};
use super::parse::largest_power;
use super::{BigInt, BigIntError, Result, limbs};

/// The digit characters.
const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// For each radix (index 2 to 36): the digit count of the largest power
/// of it that fits in a limb, and the reciprocal of that power.
const CHUNKS: [(usize, Reciprocal); 37] = {
    let mut table = [(1, Reciprocal::new(1)); 37];
    let mut radix = 2;
    while radix <= 36 {
        let (power, digits) = largest_power(radix as u32);
        table[radix] = (digits, Reciprocal::new(power));
        radix += 1;
    }
    table
};

/// The text of `x` in `radix` (2 to 36; another radix counts as 10), as
/// ASCII bytes.
pub fn to_string_radix(x: &BigInt, radix: u32) -> Result<Vec<u8>> {
    let radix = if (2..=36).contains(&radix) { radix } else { 10 };
    if x.is_zero() {
        return try_bytes(b"0");
    }
    if radix.is_power_of_two() {
        power_of_two_digits(x, radix.trailing_zeros())
    } else {
        chunked_digits(x, radix)
    }
}

/// A byte vector with the content `text`.
fn try_bytes(text: &[u8]) -> Result<Vec<u8>> {
    let mut out = try_text(text.len())?;
    out.extend_from_slice(text);
    Ok(out)
}

/// An empty byte vector with room for `len` bytes.
fn try_text(len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| BigIntError::OutOfMemory)?;
    Ok(out)
}

/// The digits in radix 2^`width`, from the top bit group.
fn power_of_two_digits(x: &BigInt, width: u32) -> Result<Vec<u8>> {
    let count = x.bit_len().div_ceil(u64::from(width));
    let mut out = try_text(count as usize + usize::from(x.negative))?;
    if x.negative {
        out.push(b'-');
    }
    let mask = (1u64 << width) - 1;
    for i in (0..count).rev() {
        let digit = limbs::bits_at(&x.limbs, i * u64::from(width)) & mask;
        out.push(DIGITS[digit as usize]);
    }
    Ok(out)
}

/// The digits in a radix that is not a power of two, by repeated division
/// by radix^k.
fn chunked_digits(x: &BigInt, radix: u32) -> Result<Vec<u8>> {
    let Some(&(per_chunk, reciprocal)) = CHUNKS.get(radix as usize) else {
        return Err(BigIntError::InvalidSyntax);
    };
    if let [limb] = x.limbs[..] {
        // One limb: the digits directly.
        let mut buffer = [0u8; 64];
        let digits = chunk_digits(limb, radix, &mut buffer, 0);
        let mut out = try_text(digits.len() + usize::from(x.negative))?;
        if x.negative {
            out.push(b'-');
        }
        out.extend_from_slice(digits);
        return Ok(out);
    }
    // Every chunk takes more than 58 bits (divisor > 2^64 / 36).
    let chunk_count = (x.bit_len() / 58) as usize + 1;
    let mut chunks = limbs::try_vec(chunk_count)?;
    let mut work = limbs::try_vec(x.limbs.len())?;
    work.extend_from_slice(&x.limbs);
    while !work.is_empty() {
        if chunks.len() == chunks.capacity() {
            return Err(BigIntError::OutOfMemory);
        }
        chunks.push(div_rem_limb(&mut work, &reciprocal));
        limbs::trim(&mut work);
    }
    let Some((&top, rest)) = chunks.split_last() else {
        return try_bytes(b"0");
    };
    let mut buffer = [0u8; 64];
    let top_digits = chunk_digits(top, radix, &mut buffer, 0);
    let len = usize::from(x.negative) + top_digits.len() + rest.len() * per_chunk;
    let mut out = try_text(len)?;
    if x.negative {
        out.push(b'-');
    }
    out.extend_from_slice(top_digits);
    for &chunk in rest.iter().rev() {
        out.extend_from_slice(chunk_digits(chunk, radix, &mut buffer, per_chunk));
    }
    Ok(out)
}

/// The digits of `value` in `radix`, at least `width` of them (with
/// leading zeros), written into the end of `buffer`.
fn chunk_digits(mut value: u64, radix: u32, buffer: &mut [u8; 64], width: usize) -> &[u8] {
    let radix = u64::from(radix);
    let mut start = buffer.len();
    while (value > 0 || buffer.len() - start < width) && start > 0 {
        start -= 1;
        buffer[start] = DIGITS[(value % radix) as usize];
        value /= radix;
    }
    &buffer[start..]
}
