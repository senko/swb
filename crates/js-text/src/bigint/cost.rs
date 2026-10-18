//! The time of the operations, as a number of work units for the engine's
//! time countdown (ADR 0026 section 9). One unit is about 0.7 ns in a
//! release build: one step of the schoolbook multiplication's inner loop
//! (a 64 × 64-bit multiply-add). Each function bounds the work of the
//! operation of the same name for the given operands, also when the
//! operation fails at once (then the cost is [`BASE`]); the engine can
//! charge it before or after the call. The weights are measured with the
//! `bigint_bench` example (docs/performance.md, "`BigInt`"); measured
//! times are between 0.3 and 1.3 ns per unit.

use super::{BigInt, MAX_BITS, arith};

/// Units per limb of a linear pass that adds, copies or combines limbs.
const LINEAR_PER_LIMB: u64 = 1;

/// Units per limb of a division by one limb (the reciprocal's dependent
/// chain of two multiplications, about 3.7 ns).
const DIVISION_PER_LIMB: u64 = 5;

/// Units per limb of a shift that does not grow the value: each limb of
/// the result combines two limbs of the operand (measured 1.7 ns).
const SHRINK_PER_LIMB: u64 = 3;

/// Units per digit of a `toString` in a radix that is a power of two.
const POWER_OF_TWO_DIGIT: u64 = 2;

/// Units per digit of the last step of `toString` in other radixes (the
/// digits of each limb-sized chunk).
const CHUNK_DIGIT: u64 = 2;

/// The fixed part of each operation: the call, the allocation and the
/// checks. Also the whole cost of an operation that fails at once.
pub const BASE: u64 = 20;

fn len(x: &BigInt) -> u64 {
    x.limbs.len() as u64
}

/// [`super::add`] and [`super::sub`].
pub fn add(x: &BigInt, y: &BigInt) -> u64 {
    BASE + LINEAR_PER_LIMB * len(x).max(len(y))
}

/// [`super::mul`]: the product of the lengths. A product that is
/// certainly above the limit fails at once.
pub fn mul(x: &BigInt, y: &BigInt) -> u64 {
    if (x.bit_len() + y.bit_len()).saturating_sub(1) > MAX_BITS {
        return BASE;
    }
    BASE + len(x) * len(y) + LINEAR_PER_LIMB * (len(x) + len(y))
}

/// [`super::div`], [`super::rem`] and [`super::div_rem`]: the quotient's
/// length times the divisor's.
pub fn div(x: &BigInt, y: &BigInt) -> u64 {
    let (lx, ly) = (len(x), len(y));
    if ly == 0 || lx < ly {
        return BASE + LINEAR_PER_LIMB * lx;
    }
    if ly == 1 {
        return BASE + DIVISION_PER_LIMB * lx;
    }
    BASE + (lx - ly + 1) * (ly + DIVISION_PER_LIMB) + 3 * LINEAR_PER_LIMB * lx
}

/// [`super::pow`]: from the estimated length L of the result, about
/// L²/3 for the squarings (the last one multiplies two values of L/2
/// limbs) and the multiplications by the base.
pub fn pow(base: &BigInt, exponent: &BigInt) -> u64 {
    let e = match exponent.limbs[..] {
        [e] if !exponent.negative => e,
        _ => return BASE,
    };
    if base.limbs.is_empty() || base.limbs == [1] {
        return BASE;
    }
    let bits = e as f64 * arith::log2(&base.limbs);
    if bits >= MAX_BITS as f64 + 1.0 {
        return BASE;
    }
    let result = (bits / 64.0) as u64 + 2;
    BASE + result * result / 3 + result * (len(base) + 64)
}

/// [`super::shift_left`] (`right` false) and [`super::shift_right`]
/// (`right` true): the length of the result (at most the length of `x`
/// plus the shift in limbs); a shift that does not grow the value costs
/// more per limb.
pub fn shift(x: &BigInt, y: &BigInt, right: bool) -> u64 {
    let grows = y.negative == right;
    let amount = match y.limbs[..] {
        [amount] if grows => amount,
        [] => 0,
        _ if grows => u64::MAX,
        _ => 0,
    };
    if amount.saturating_add(x.bit_len()) > MAX_BITS {
        return BASE;
    }
    if !grows {
        return BASE + SHRINK_PER_LIMB * len(x);
    }
    BASE + LINEAR_PER_LIMB * (len(x) + amount.div_ceil(64))
}

/// [`super::bit_and`], [`super::bit_or`], [`super::bit_xor`] and
/// [`super::not`]: one pass over both operands.
pub fn bitwise(x: &BigInt, y: &BigInt) -> u64 {
    BASE + 2 * LINEAR_PER_LIMB * len(x).max(len(y))
}

/// [`super::as_int_n`] and [`super::as_uint_n`]: the limbs of the result
/// (for a negative `x`, `bits` within the limit).
pub fn as_n(bits: u64, x: &BigInt) -> u64 {
    let limbs = if x.negative {
        bits.min(MAX_BITS).div_ceil(64).max(len(x))
    } else {
        len(x)
    };
    BASE + 2 * LINEAR_PER_LIMB * limbs
}

/// [`super::to_string_radix`]: quadratic in the length for a radix that
/// is not a power of two (L²/2 limb divisions), linear otherwise.
pub fn to_string(x: &BigInt, radix: u32) -> u64 {
    let lx = len(x);
    if radix.is_power_of_two() {
        let digits = x
            .bit_len()
            .div_ceil(u64::from(radix.trailing_zeros().max(1)));
        return BASE + POWER_OF_TWO_DIGIT * digits;
    }
    BASE + DIVISION_PER_LIMB * (lx * (lx + 1) / 2) + CHUNK_DIGIT * 64 * lx
}

/// [`super::string_to_bigint`] and [`super::parse_literal`] of a text of
/// `units` code units: the scan, and for a radix that is not a power of
/// two (all decimal text) about L²/4 for a result of L limbs (L²/2 steps
/// of a single-limb multiply-add, which takes about half a unit).
pub fn parse(units: usize) -> u64 {
    let units = units as u64;
    // A decimal digit is log2(10) < 3.33 bits; at most the limit.
    let result = (units.saturating_mul(10) / 3)
        .min(MAX_BITS + 64)
        .div_ceil(64);
    (BASE + result * result / 4).saturating_add(units)
}

/// [`super::to_f64`], [`super::from_f64`], [`super::compare_f64`], the
/// comparison of two values, [`super::neg`] and `try_clone`: at most one
/// pass over the operand.
pub fn linear(x: &BigInt) -> u64 {
    BASE + LINEAR_PER_LIMB * len(x)
}
