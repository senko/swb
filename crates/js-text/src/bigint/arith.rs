//! Addition, subtraction, multiplication, exponentiation, negation and
//! bitwise NOT (§6.1.6.2.1 to §6.1.6.2.4, §6.1.6.2.7 and §6.1.6.2.8,
//! <https://tc39.es/ecma262/#sec-numeric-types-bigint-add>).
//!
//! Operands of one limb compute in 128-bit arithmetic, so the result has
//! a vector of exactly the limbs that it needs. Multiplication is the
//! schoolbook method: at the size limit, a product takes about as long as
//! the slowest other operation (`toString`), so Karatsuba's method would
//! not raise the limit by much (docs/performance.md).

use std::cmp::Ordering;

use super::{BigInt, BigIntError, MAX_BITS, Result, check_bits, limbs};

/// `BigInt::add` (§6.1.6.2.7): `x + y`.
pub fn add(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    signed_add(x, y.negative, &y.limbs)
}

/// `BigInt::subtract` (§6.1.6.2.8): `x − y`.
pub fn sub(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    signed_add(x, !y.negative && !y.is_zero(), &y.limbs)
}

/// `x + (−1)^negative × magnitude`.
fn signed_add(x: &BigInt, negative: bool, magnitude: &[u64]) -> Result<BigInt> {
    if let ([a], [b]) = (&x.limbs[..], magnitude) {
        let (a, b) = (i128::from(*a), i128::from(*b));
        let a = if x.negative { -a } else { a };
        let b = if negative { -b } else { b };
        let sum = a + b;
        return BigInt::from_u128(sum < 0, sum.unsigned_abs());
    }
    if x.negative == negative {
        let sum = limbs::add(&x.limbs, magnitude)?;
        check_bits(limbs::bit_len(&sum))?;
        return Ok(BigInt::from_normalized(negative, sum));
    }
    match limbs::cmp(&x.limbs, magnitude) {
        Ordering::Equal => Ok(BigInt::ZERO),
        Ordering::Greater => Ok(BigInt::from_normalized(
            x.negative,
            limbs::sub(&x.limbs, magnitude)?,
        )),
        Ordering::Less => Ok(BigInt::from_normalized(
            negative,
            limbs::sub(magnitude, &x.limbs)?,
        )),
    }
}

/// `BigInt::multiply` (§6.1.6.2.4): `x × y`.
pub fn mul(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    let negative = x.negative != y.negative;
    match (&x.limbs[..], &y.limbs[..]) {
        ([], _) | (_, []) => Ok(BigInt::ZERO),
        ([a], [b]) => BigInt::from_u128(negative, u128::from(*a) * u128::from(*b)),
        (a, b) => {
            // The product has bit_len(a) + bit_len(b) bits or one less.
            let bits = limbs::bit_len(a) + limbs::bit_len(b);
            check_bits(bits - 1)?;
            let product = limbs::mul(a, b)?;
            check_bits(limbs::bit_len(&product))?;
            Ok(BigInt::from_normalized(negative, product))
        }
    }
}

/// `BigInt::unaryMinus` (§6.1.6.2.1): `−x`.
pub fn neg(x: &BigInt) -> Result<BigInt> {
    let mut out = x.try_clone()?;
    out.negative = !x.negative && !x.is_zero();
    Ok(out)
}

/// `BigInt::bitwiseNOT` (§6.1.6.2.2): `−x − 1`.
pub fn not(x: &BigInt) -> Result<BigInt> {
    sub(&neg(x)?, &one()?)
}

/// The value 1.
fn one() -> Result<BigInt> {
    BigInt::from_u64(1)
}

/// `BigInt::exponentiate` (§6.1.6.2.3): `base ** exponent`; a
/// `RangeError` for a negative exponent. The size of the result is
/// estimated before any work: a result that is certainly above the limit
/// is an error at once.
pub fn pow(base: &BigInt, exponent: &BigInt) -> Result<BigInt> {
    if exponent.negative {
        return Err(BigIntError::NegativeExponent);
    }
    if exponent.is_zero() {
        return one();
    }
    // Bases 0, 1 and −1 give 0, 1 and ±1 for any exponent.
    let odd = exponent.limbs.first().is_some_and(|&limb| limb & 1 == 1);
    let negative = base.negative && odd;
    match base.limbs[..] {
        [] => return Ok(BigInt::ZERO),
        [1] => return BigInt::from_u128(negative, 1),
        _ => {}
    }
    // |base| ≥ 2, so the result has more than `exponent` bits.
    let Some(exponent) = exponent.to_u64_magnitude().filter(|&e| e <= MAX_BITS) else {
        return Err(BigIntError::TooLarge);
    };
    let base_bits = limbs::bit_len(&base.limbs);
    if base.limbs.iter().rev().skip(1).all(|&limb| limb == 0)
        && base.limbs.last().is_some_and(|top| top.is_power_of_two())
    {
        // A power of two: a shift.
        let shift = (base_bits - 1) * exponent;
        check_bits(shift + 1)?;
        return power_of_two(negative, shift);
    }
    // The result has floor(exponent × log2|base|) + 1 bits; the estimate
    // is accurate to far better than one bit at these sizes.
    if exponent as f64 * log2(&base.limbs) >= MAX_BITS as f64 + 1.0 {
        return Err(BigIntError::TooLarge);
    }
    let magnitude = pow_magnitude(&base.limbs, exponent)?;
    check_bits(limbs::bit_len(&magnitude))?;
    Ok(BigInt::from_normalized(negative, magnitude))
}

/// 2^`shift` with a sign; `shift` is within the limit.
pub(super) fn power_of_two(negative: bool, shift: u64) -> Result<BigInt> {
    let words = (shift / 64) as usize;
    let mut out = limbs::try_zeroed(words + 1)?;
    if let Some(top) = out.last_mut() {
        *top = 1 << (shift % 64);
    }
    Ok(BigInt::from_normalized(negative, out))
}

/// An estimate of log2 of a magnitude of at least one limb, from its top
/// 64 bits.
pub(super) fn log2(magnitude: &[u64]) -> f64 {
    let bits = limbs::bit_len(magnitude);
    let shift = bits.saturating_sub(64);
    let top = limbs::bits_at(magnitude, shift);
    shift as f64 + (top as f64).log2()
}

/// `base^exponent` by left-to-right binary exponentiation (Knuth, TAOCP
/// vol. 2, §4.6.3); `exponent` ≥ 1 and the result is known to be within
/// a few bits of the limit.
fn pow_magnitude(base: &[u64], exponent: u64) -> Result<Vec<u64>> {
    let mut result = limbs::try_vec(base.len())?;
    result.extend_from_slice(base);
    let top = exponent.ilog2();
    for bit in (0..top).rev() {
        result = limbs::mul(&result, &result)?;
        if exponent >> bit & 1 == 1 {
            result = limbs::mul(&result, base)?;
        }
    }
    Ok(result)
}
