//! Doubles and `BigInt`s: the conversion to a double (`Number(bigint)`,
//! §21.1.1.1 step 2.a, "𝔽(ℝ(prim))": rounded to nearest, ties to even;
//! <https://tc39.es/ecma262/#sec-number-constructor-number-value>), the
//! conversion from an integral double (`NumberToBigInt`, §21.2.1.1.1,
//! <https://tc39.es/ecma262/#sec-numbertobigint>) and the exact
//! comparison with a double, for `IsLessThan` and `IsLooselyEqual` (§7.2.13
//! and §7.2.14; <https://tc39.es/ecma262/#sec-islessthan>), without an
//! allocation.

use std::cmp::Ordering;

use super::{BigInt, BigIntError, Result, limbs};

/// The bits of a double's significand without the implicit bit.
const FRACTION_BITS: u32 = 52;

/// The value rounded to the nearest double, ties to even; ±infinity from
/// 2^1024 − 2^970 upward (that tie rounds to the even 2^1024).
pub fn to_f64(x: &BigInt) -> f64 {
    let magnitude = magnitude_to_f64(&x.limbs);
    if x.negative { -magnitude } else { magnitude }
}

fn magnitude_to_f64(magnitude: &[u64]) -> f64 {
    let len = limbs::bit_len(magnitude);
    if len <= 64 {
        // One rounding of a u64 to a double: Rust rounds to nearest, ties
        // to even.
        return limbs::bits_at(magnitude, 0) as f64;
    }
    if len > 1024 {
        return f64::INFINITY;
    }
    // The top 54 bits: 53 for the significand and the rounding bit; and
    // whether a bit below them is set.
    let shift = len - 54;
    let top = limbs::bits_at(magnitude, shift) & ((1 << 54) - 1);
    let mut significand = top >> 1;
    if top & 1 == 1 && (significand & 1 == 1 || limbs::any_bit_below(magnitude, shift)) {
        significand += 1;
    }
    // significand ≤ 2^53 converts exactly; the scale 2^(shift + 1) is at
    // most 2^971, and the product overflows to infinity only for
    // 2^53 × 2^971.
    let scale = f64::from_bits((shift + 1 + 1023) << FRACTION_BITS);
    significand as f64 * scale
}

/// `NumberToBigInt` (§21.2.1.1.1): the value of an integral double; an
/// error for NaN, the infinities and fractions.
pub fn from_f64(value: f64) -> Result<BigInt> {
    if !value.is_finite() || value.trunc() != value {
        return Err(BigIntError::NotIntegral);
    }
    let (significand, exponent) = decompose(value.abs());
    let negative = value < 0.0;
    if exponent <= 0 {
        // The value is integral, so the low −exponent bits are zero.
        let shift = exponent.unsigned_abs().min(63);
        return BigInt::from_u128(negative, u128::from(significand >> shift));
    }
    let words = (exponent / 64) as usize;
    let bits = (exponent % 64) as u32;
    let wide = u128::from(significand) << bits;
    let mut out = limbs::try_zeroed(words + 2)?;
    if let Some(slot) = out.get_mut(words..words + 2) {
        slot.copy_from_slice(&[wide as u64, (wide >> 64) as u64]);
    }
    limbs::trim(&mut out);
    Ok(BigInt::from_normalized(negative, out))
}

/// A finite non-negative double as `significand × 2^exponent`.
fn decompose(value: f64) -> (u64, i32) {
    let bits = value.to_bits();
    let biased = ((bits >> FRACTION_BITS) & 0x7FF) as i32;
    let fraction = bits & ((1 << FRACTION_BITS) - 1);
    if biased == 0 {
        // Zero or subnormal.
        (fraction, -1074)
    } else {
        (fraction | (1 << FRACTION_BITS), biased - 1075)
    }
}

/// The order of `x` and the double `y`, exactly; `None` if `y` is NaN.
/// ±0 are both 0.
pub fn compare_f64(x: &BigInt, y: f64) -> Option<Ordering> {
    if y.is_nan() {
        return None;
    }
    if y.is_infinite() {
        return Some(if y > 0.0 {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let y_negative = y < 0.0;
    let y_zero = y == 0.0;
    let order = match (x.negative, y_negative) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) if y_zero => {
            if x.is_zero() {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        }
        (negative, _) => {
            let order = compare_magnitude(&x.limbs, y.abs());
            if negative { order.reverse() } else { order }
        }
    };
    Some(order)
}

/// The order of a magnitude and a finite positive double.
fn compare_magnitude(magnitude: &[u64], y: f64) -> Ordering {
    let (significand, exponent) = decompose(y);
    if exponent < 0 {
        // y = integer part + fraction; the integer part fits in 53 bits.
        let shift = exponent.unsigned_abs();
        let (integer, fraction) = if shift >= 64 {
            (0, significand != 0)
        } else {
            (
                significand >> shift,
                significand & ((1u64 << shift) - 1) != 0,
            )
        };
        let x = match magnitude {
            [] => 0,
            [limb] => *limb,
            _ => return Ordering::Greater,
        };
        return x.cmp(&integer).then(if fraction {
            Ordering::Less
        } else {
            Ordering::Equal
        });
    }
    // y = significand × 2^exponent is an integer of
    // bit_len(significand) + exponent bits.
    let y_bits = u64::from(64 - significand.leading_zeros()) + u64::from(exponent.unsigned_abs());
    let x_bits = limbs::bit_len(magnitude);
    if x_bits != y_bits {
        return x_bits.cmp(&y_bits);
    }
    // Same length: compare the top 64 bits of x with y's significand
    // aligned to them, then the rest of x with y's zeros.
    let shift = x_bits.saturating_sub(64);
    let x_top = limbs::bits_at(magnitude, shift);
    let y_top = if x_bits >= 64 {
        significand << (64 - u64::from(64 - significand.leading_zeros()))
    } else {
        significand << exponent
    };
    x_top.cmp(&y_top).then_with(|| {
        if limbs::any_bit_below(magnitude, shift) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    })
}
