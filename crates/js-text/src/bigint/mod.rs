//! The integers of `BigInt` (ECMA-262 §6.1.6.2,
//! <https://tc39.es/ecma262/#sec-ecmascript-language-types-bigint-type>;
//! ADR 0026 section 7): sign and magnitude, the magnitude in 64-bit limbs,
//! least significant first, without zero limbs at the top; zero is never
//! negative. So two equal values have equal representations, and the
//! derived equality is the mathematical one.
//!
//! The operations are pure functions without a heap. Each returns a new
//! value or a [`BigIntError`]: a result above [`MAX_BITS`] bits is an
//! error before any work in proportion to its size, and every allocation
//! is fallible. The time of an operation grows with the sizes of its
//! operands; [`cost`] gives a bound in work units that the engine
//! charges to its time countdown (ADR 0026 section 9).
//!
//! - `limbs`: the magnitude arithmetic on limb slices.
//! - `arith`: [`add`], [`sub`], [`mul`], [`pow`], [`neg`], [`not`].
//! - `div`: [`div`], [`rem`] (truncating; Knuth's Algorithm D).
//! - `bits`: the shifts, the bitwise operators on the infinite two's
//!   complement form, [`as_int_n`], [`as_uint_n`].
//! - `float`: the conversions to and from doubles and the exact
//!   comparison with a double.
//! - `parse`: `StringToBigInt` and the literals.
//! - `format`: `toString` in the radixes 2 to 36.

mod arith;
mod bits;
pub mod cost;
mod div;
mod float;
mod format;
mod limbs;
mod parse;

use std::cmp::Ordering;

pub use arith::{add, mul, neg, not, pow, sub};
pub use bits::{as_int_n, as_uint_n, bit_and, bit_or, bit_xor, shift_left, shift_right};
pub use div::{div, div_rem, rem};
pub use float::{compare_f64, from_f64, to_f64};
pub use format::to_string_radix;
pub use parse::{compare_text, parse_literal, string_to_bigint};

/// The largest size of a value in bits; a larger result is
/// [`BigIntError::TooLarge`]. Chromium 148 allows 2^30 − 64 bits
/// (1,073,741,760). swb's limit is lower, so that the slowest single
/// operation on values of this size (`toString` in a radix that is not a
/// power of two, with the quadratic algorithms of this module) takes at
/// most 100 ms in a release build: 32 ms measured, and 126 ms with 2^19
/// bits (ADR 0026 sections 7 and 15, docs/performance.md).
pub const MAX_BITS: u64 = 1 << 18;

/// Why an operation has no result. The messages are V8's where V8 has
/// one; the engine adds the details of [`BigIntError::NotIntegral`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigIntError {
    /// The result would have more than [`MAX_BITS`] bits (`RangeError`).
    TooLarge,
    /// A division or remainder by zero (`RangeError`).
    DivisionByZero,
    /// An exponentiation with a negative exponent (`RangeError`).
    NegativeExponent,
    /// `NumberToBigInt` of a value that is not an integral Number
    /// (`RangeError`: "The number X cannot be converted to a `BigInt` because
    /// it is not an integer").
    NotIntegral,
    /// An allocation failed.
    OutOfMemory,
    /// The text is not a `StringIntegerLiteral`: `StringToBigInt` returns
    /// undefined (§7.1.14), which `BigInt(string)` turns into a
    /// `SyntaxError` ("Cannot convert X to a `BigInt`").
    InvalidSyntax,
}

impl std::fmt::Display for BigIntError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BigIntError::TooLarge => "Maximum BigInt size exceeded",
            BigIntError::DivisionByZero => "Division by zero",
            BigIntError::NegativeExponent => "Exponent must be positive",
            BigIntError::NotIntegral => "The number cannot be converted to a BigInt",
            BigIntError::OutOfMemory => "Out of memory",
            BigIntError::InvalidSyntax => "Cannot convert the text to a BigInt",
        })
    }
}

impl std::error::Error for BigIntError {}

/// The result of a `BigInt` operation.
pub type Result<T> = std::result::Result<T, BigIntError>;

/// An integer of any size up to [`MAX_BITS`] bits.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BigInt {
    /// Whether the value is negative (never for zero).
    negative: bool,
    /// The magnitude, least significant limb first, without zero limbs at
    /// the top (empty for zero).
    limbs: Vec<u64>,
}

impl BigInt {
    /// Zero.
    pub const ZERO: BigInt = BigInt {
        negative: false,
        limbs: Vec::new(),
    };

    /// The value with sign `negative` and magnitude `limbs` (least
    /// significant first), normalized; an error for a magnitude above the
    /// limit.
    pub fn from_limbs(negative: bool, mut limbs: Vec<u64>) -> Result<Self> {
        limbs::trim(&mut limbs);
        if limbs::bit_len(&limbs) > MAX_BITS {
            return Err(BigIntError::TooLarge);
        }
        Ok(Self::from_normalized(negative, limbs))
    }

    /// The value with a magnitude that has no zero limbs at the top and is
    /// within the limit.
    fn from_normalized(negative: bool, limbs: Vec<u64>) -> Self {
        debug_assert!(limbs.last() != Some(&0), "a normalized magnitude");
        BigInt {
            negative: negative && !limbs.is_empty(),
            limbs,
        }
    }

    /// The value `value`.
    pub fn from_i64(value: i64) -> Result<Self> {
        Self::from_u128(value < 0, u128::from(value.unsigned_abs()))
    }

    /// The value `value`.
    pub fn from_u64(value: u64) -> Result<Self> {
        Self::from_u128(false, u128::from(value))
    }

    /// The value with sign `negative` and magnitude `magnitude`, in a
    /// vector of exactly the limbs it needs.
    pub(crate) fn from_u128(negative: bool, magnitude: u128) -> Result<Self> {
        let low = magnitude as u64;
        let high = (magnitude >> 64) as u64;
        let len = if high != 0 { 2 } else { usize::from(low != 0) };
        let mut limbs = limbs::try_vec(len)?;
        limbs.extend([low, high].into_iter().take(len));
        Ok(Self::from_normalized(negative, limbs))
    }

    /// A copy (fallible, unlike `Clone`).
    pub(crate) fn try_clone(&self) -> Result<Self> {
        let mut limbs = limbs::try_vec(self.limbs.len())?;
        limbs.extend_from_slice(&self.limbs);
        Ok(BigInt {
            negative: self.negative,
            limbs,
        })
    }

    /// Whether the value is 0.
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Whether the value is below 0.
    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// The magnitude, least significant limb first, without zero limbs at
    /// the top.
    pub fn limbs(&self) -> &[u64] {
        &self.limbs
    }

    /// The number of bits of the magnitude (0 for zero).
    pub fn bit_len(&self) -> u64 {
        limbs::bit_len(&self.limbs)
    }

    /// The bytes of the allocation of the magnitude, for the heap's
    /// accounting.
    pub fn heap_bytes(&self) -> usize {
        self.limbs.capacity() * 8
    }

    /// The value as an `i64`, if it fits.
    pub fn to_i64(&self) -> Option<i64> {
        let magnitude = self.to_u64_magnitude()?;
        if self.negative {
            0i64.checked_sub_unsigned(magnitude)
        } else {
            i64::try_from(magnitude).ok()
        }
    }

    /// The magnitude as a `u64`, if it fits.
    fn to_u64_magnitude(&self) -> Option<u64> {
        match self.limbs[..] {
            [] => Some(0),
            [limb] => Some(limb),
            _ => None,
        }
    }

    /// The value modulo 2^64 (`ToBigUint64`, §7.1.16, after the
    /// conversion to a `BigInt`).
    pub fn to_u64_wrapping(&self) -> u64 {
        let low = self.limbs.first().copied().unwrap_or(0);
        if self.negative {
            low.wrapping_neg()
        } else {
            low
        }
    }

    /// The value modulo 2^64 as a signed integer (`ToBigInt64`, §7.1.15).
    pub fn to_i64_wrapping(&self) -> i64 {
        self.to_u64_wrapping() as i64
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    /// `BigInt::lessThan` (§6.1.6.2.12) and equality.
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => limbs::cmp(&self.limbs, &other.limbs),
            (true, true) => limbs::cmp(&other.limbs, &self.limbs),
        }
    }
}

/// An error unless a result of `bits` bits is within the limit.
fn check_bits(bits: u64) -> Result<()> {
    if bits > MAX_BITS {
        Err(BigIntError::TooLarge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
