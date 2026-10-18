//! Division and remainder (`BigInt::divide` and `BigInt::remainder`,
//! §6.1.6.2.5 and §6.1.6.2.6,
//! <https://tc39.es/ecma262/#sec-numeric-types-bigint-divide>): the
//! quotient is truncated towards zero and the remainder has the sign of
//! the dividend.
//!
//! A divisor of one limb divides limb by limb with a precomputed
//! reciprocal (Möller and Granlund, "Improved division by invariant
//! integers", IEEE Transactions on Computers 60(2), 2011, Algorithm 4),
//! which replaces the slow 128-by-64-bit hardware division with two
//! multiplications. Longer divisors use Knuth's Algorithm D (TAOCP vol. 2,
//! §4.3.1), whose quotient estimate uses the same reciprocal.

use std::cmp::Ordering;

use super::{BigInt, BigIntError, Result, limbs};

/// `BigInt::divide` (§6.1.6.2.5): `x / y`, truncated towards zero.
pub fn div(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    Ok(div_rem(x, y)?.0)
}

/// `BigInt::remainder` (§6.1.6.2.6): `x % y`, with the sign of `x`.
pub fn rem(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    Ok(div_rem(x, y)?.1)
}

/// The truncated quotient and the remainder of `x / y`.
pub fn div_rem(x: &BigInt, y: &BigInt) -> Result<(BigInt, BigInt)> {
    if y.is_zero() {
        return Err(BigIntError::DivisionByZero);
    }
    let negative = x.negative != y.negative;
    if let ([a], [b]) = (&x.limbs[..], &y.limbs[..]) {
        return Ok((
            BigInt::from_u128(negative, u128::from(a / b))?,
            BigInt::from_u128(x.negative, u128::from(a % b))?,
        ));
    }
    if limbs::cmp(&x.limbs, &y.limbs) == Ordering::Less {
        return Ok((BigInt::ZERO, x.try_clone()?));
    }
    let (quotient, remainder) = match y.limbs[..] {
        [divisor] => {
            let mut quotient = limbs::try_vec(x.limbs.len())?;
            quotient.extend_from_slice(&x.limbs);
            let remainder = div_rem_limb(&mut quotient, &Reciprocal::new(divisor));
            limbs::trim(&mut quotient);
            let mut rest = limbs::try_vec(usize::from(remainder != 0))?;
            rest.extend(Some(remainder).filter(|&r| r != 0));
            (quotient, rest)
        }
        _ => long_division(&x.limbs, &y.limbs)?,
    };
    Ok((
        BigInt::from_normalized(negative, quotient),
        BigInt::from_normalized(x.negative, remainder),
    ))
}

/// A divisor of one limb with the data for fast division by it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Reciprocal {
    /// The divisor shifted left until its top bit is set.
    normalized: u64,
    /// The shift.
    shift: u32,
    /// floor((2^128 − 1) / normalized) − 2^64.
    inverse: u64,
}

impl Reciprocal {
    /// The data for `divisor`, which is not 0.
    pub(super) const fn new(divisor: u64) -> Self {
        debug_assert!(divisor != 0, "a reciprocal of zero");
        let shift = divisor.leading_zeros();
        let normalized = divisor << shift;
        // normalized ≥ 2^63, so the quotient is below 2^65 and at least
        // 2^64.
        let divisor = if normalized == 0 { 1 << 63 } else { normalized };
        let inverse = (u128::MAX / divisor as u128) as u64;
        Reciprocal {
            normalized,
            shift,
            inverse,
        }
    }

    /// The quotient and remainder of (`high` × 2^64 + `low`) / the
    /// normalized divisor, for `high` below it (Möller and Granlund,
    /// Algorithm 4).
    #[inline]
    fn div_2by1(&self, high: u64, low: u64) -> (u64, u64) {
        let d = self.normalized;
        let product = u128::from(self.inverse) * u128::from(high);
        let estimate = product.wrapping_add((u128::from(high) << 64) | u128::from(low));
        let mut q = ((estimate >> 64) as u64).wrapping_add(1);
        let mut r = low.wrapping_sub(q.wrapping_mul(d));
        if r > estimate as u64 {
            q = q.wrapping_sub(1);
            r = r.wrapping_add(d);
        }
        if r >= d {
            q = q.wrapping_add(1);
            r -= d;
        }
        (q, r)
    }
}

/// Divides `limbs` in place by the divisor of `reciprocal` and returns
/// the remainder. The top limbs of the quotient may be zero.
pub(super) fn div_rem_limb(limbs: &mut [u64], reciprocal: &Reciprocal) -> u64 {
    let shift = reciprocal.shift;
    if shift == 0 {
        let mut r = 0;
        for limb in limbs.iter_mut().rev() {
            let (q, rest) = reciprocal.div_2by1(r, *limb);
            *limb = q;
            r = rest;
        }
        return r;
    }
    // Divide (limbs × 2^shift) by (divisor × 2^shift), shifting each limb
    // as it is read; the quotient is the same and the remainder is
    // shifted.
    let mut r = limbs.last().map_or(0, |&top| top >> (64 - shift));
    let mut iter = limbs.iter_mut().rev().peekable();
    while let Some(limb) = iter.next() {
        let below = iter.peek().map_or(0, |next| **next >> (64 - shift));
        let (q, rest) = reciprocal.div_2by1(r, (*limb << shift) | below);
        *limb = q;
        r = rest;
    }
    r >> shift
}

/// `numerator / divisor` by Knuth's Algorithm D, for a divisor of at least
/// two limbs that is not larger than the numerator: the quotient and the
/// remainder, both normalized.
#[expect(clippy::many_single_char_names, reason = "the names of Algorithm D")]
fn long_division(numerator: &[u64], divisor: &[u64]) -> Result<(Vec<u64>, Vec<u64>)> {
    let n = divisor.len();
    let m = numerator.len() - n;
    // D1: normalize so that the divisor's top bit is set.
    let shift = divisor.last().map_or(0, |top| top.leading_zeros());
    let v = shifted(divisor, shift, n)?;
    let mut u = shifted(numerator, shift, numerator.len() + 1)?;
    let (Some(&v1), Some(&v2)) = (v.get(n - 1), v.get(n - 2)) else {
        return Err(BigIntError::OutOfMemory);
    };
    let reciprocal = Reciprocal::new(v1);
    let mut quotient = limbs::try_zeroed(m + 1)?;
    // D2 to D7: one quotient limb per step, from the top.
    for j in (0..=m).rev() {
        let Some(window) = u.get_mut(j..=j + n) else {
            break;
        };
        let q = quotient_limb(window, &v, v1, v2, &reciprocal);
        if let Some(slot) = quotient.get_mut(j) {
            *slot = q;
        }
    }
    limbs::trim(&mut quotient);
    // D8: the remainder is the low n limbs, shifted back.
    u.truncate(n);
    if shift > 0 {
        let mut carry = 0;
        for limb in u.iter_mut().rev() {
            let next = *limb << (64 - shift);
            *limb = (*limb >> shift) | carry;
            carry = next;
        }
    }
    limbs::trim(&mut u);
    Ok((quotient, u))
}

/// The limbs of `value` × 2^`shift` (`shift` < 64) in a vector of `len`
/// limbs (at least the length of `value`; one more holds the bits shifted
/// out of the top).
fn shifted(value: &[u64], shift: u32, len: usize) -> Result<Vec<u64>> {
    let mut out = limbs::try_vec(len)?;
    out.extend_from_slice(value);
    out.resize(len, 0);
    if shift > 0 {
        let mut carry = 0;
        for limb in &mut out {
            let next = *limb >> (64 - shift);
            *limb = (*limb << shift) | carry;
            carry = next;
        }
    }
    Ok(out)
}

/// Steps D3 to D6 of Algorithm D for the window u[j..=j+n] of the
/// numerator: estimates the quotient limb from the top two limbs, corrects
/// it with the third, subtracts its multiple of `v` from the window and
/// adds `v` back if the estimate was one too large. `v1` and `v2` are the
/// top two limbs of `v`.
fn quotient_limb(window: &mut [u64], v: &[u64], v1: u64, v2: u64, reciprocal: &Reciprocal) -> u64 {
    let n = v.len();
    let (u0, u1, u2) = (window[n], window[n - 1], window[n - 2]);
    // D3: q̂ = floor((u0 × b + u1) / v1), at most b − 1; r̂ the rest.
    let (mut q, mut r, mut r_fits) = if u0 >= v1 {
        // u0 = v1 (the window's top n limbs are below v): q̂ = b − 1 and
        // r̂ = u0 × b + u1 − (b − 1) × v1 = u1 + v1.
        let (r, overflow) = u1.overflowing_add(v1);
        (u64::MAX, r, !overflow)
    } else {
        let (q, r) = reciprocal.div_2by1(u0, u1);
        (q, r, true)
    };
    // While q̂ × v2 > r̂ × b + u2: q̂ is too large (at most twice).
    while r_fits && u128::from(q) * u128::from(v2) > ((u128::from(r) << 64) | u128::from(u2)) {
        q -= 1;
        let (next, overflow) = r.overflowing_add(v1);
        r = next;
        r_fits = !overflow;
    }
    // D4: subtract q̂ × v.
    let mut carry = 0u64;
    let mut borrow = false;
    for (x, &y) in window.iter_mut().zip(v) {
        let product = u128::from(q) * u128::from(y) + u128::from(carry);
        carry = (product >> 64) as u64;
        let (diff, b1) = x.overflowing_sub(product as u64);
        let (diff, b2) = diff.overflowing_sub(u64::from(borrow));
        *x = diff;
        borrow = b1 || b2;
    }
    let (top, b1) = window[n].overflowing_sub(carry);
    let (top, b2) = top.overflowing_sub(u64::from(borrow));
    window[n] = top;
    if b1 || b2 {
        // D6: the estimate was one too large; add v back.
        #[cfg(test)]
        tests::count_add_back();
        q -= 1;
        let carry = limbs::add_assign(&mut window[..n], v);
        window[n] = window[n].wrapping_add(u64::from(carry));
    }
    q
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static ADD_BACKS: Cell<usize> = const { Cell::new(0) };
    }

    /// Counts a step D6 (test builds only).
    pub(super) fn count_add_back() {
        ADD_BACKS.with(|count| count.set(count.get() + 1));
    }

    /// The steps D6 since the last call.
    pub(in crate::bigint) fn take_add_backs() -> usize {
        ADD_BACKS.with(|count| count.replace(0))
    }

    #[test]
    fn reciprocal_division_matches_hardware() {
        for divisor in [
            1u64,
            3,
            10,
            1 << 63,
            (1 << 63) + 1,
            u64::MAX,
            0x8AC7_2304_89E8_0000,
        ] {
            let reciprocal = Reciprocal::new(divisor);
            for value in [[0u64, 0], [u64::MAX, 0], [12_345, 7], [u64::MAX, u64::MAX]] {
                let mut limbs = value;
                let r = div_rem_limb(&mut limbs, &reciprocal);
                let whole = (u128::from(value[1]) << 64) | u128::from(value[0]);
                let q = whole / u128::from(divisor);
                assert_eq!(
                    (u128::from(limbs[1]) << 64) | u128::from(limbs[0]),
                    q,
                    "{whole:#x} / {divisor:#x}"
                );
                assert_eq!(u128::from(r), whole % u128::from(divisor));
            }
        }
    }

    /// The magnitude of a hexadecimal field of the reference table.
    fn hex(field: &str) -> Vec<u64> {
        let digits = field.trim_start_matches('-').as_bytes();
        digits
            .rchunks(16)
            .map(|chunk| u64::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn reference_cases_need_the_add_back_step() {
        let table = include_str!("../../tests/data/bigint_reference.tsv");
        let mut count = 0;
        for line in table.lines().filter(|line| line.starts_with("divab\t")) {
            let fields: Vec<&str> = line.split('\t').collect();
            take_add_backs();
            let (q, r) = long_division(&hex(fields[1]), &hex(fields[2])).unwrap();
            assert!(take_add_backs() > 0, "{line}");
            assert_eq!((q, r), (hex(fields[3]), hex(fields[4])), "{line}");
            count += 1;
        }
        assert!(count >= 40, "{count} cases");
    }

    #[test]
    fn add_back_step_runs() {
        // Hacker's Delight's case for 16-bit digits, with 64-bit limbs:
        // (b^3 (b/2 − 1) + b^2 b/2) / (b^2 b/2 + 1), b = 2^64.
        take_add_backs();
        let half = 1u64 << 63;
        let (q, r) = long_division(&[0, 0, half, half - 1], &[1, 0, half]).unwrap();
        assert_eq!(take_add_backs(), 1);
        // q = b − 2 and r = numerator − q × divisor (Python's int).
        assert_eq!(q, [u64::MAX - 1]);
        assert_eq!(r, [2, u64::MAX, half - 1]);
    }
}
