//! Multi-precision natural numbers for the exact number conversions (ADR
//! 0026 section 7): a vector of 32-bit limbs, least significant first,
//! without zero limbs at the top. Only the operations that the digit
//! generation of `toFixed`, `toExponential`, `toPrecision` and
//! `toString(radix)` and the exact `parseInt` need (rounding to the
//! nearest double: [`Natural::to_f64`]); `BigInt` (M7 feature 10) can
//! grow this.
//!
//! The digit step divides two naturals whose quotient is a single digit:
//! an estimate from the leading 64 bits of the divisor, which is never too
//! large, and at most a few corrections (the idea of the quotient estimate
//! of Knuth, TAOCP vol. 2, §4.3.1, Algorithm D, for a one-digit quotient).

use std::cmp::Ordering;

/// A natural number (an integer ≥ 0) of any size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Natural {
    limbs: Vec<u32>,
}

impl Natural {
    /// The natural number `value`.
    pub(crate) fn from_u64(value: u64) -> Self {
        let mut n = Natural {
            limbs: vec![value as u32, (value >> 32) as u32],
        };
        n.trim();
        n
    }

    /// Whether the value is 0.
    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Whether the value is odd.
    #[cfg(test)]
    pub(crate) fn is_odd(&self) -> bool {
        self.limbs.first().is_some_and(|&limb| limb & 1 == 1)
    }

    /// The number of significant bits (0 for zero).
    pub(crate) fn bit_len(&self) -> u64 {
        match self.limbs.last() {
            None => 0,
            Some(&top) => (self.limbs.len() as u64 - 1) * 32 + u64::from(32 - top.leading_zeros()),
        }
    }

    /// Removes zero limbs at the top.
    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    /// Multiplies by 2^`bits`.
    pub(crate) fn shl(&mut self, bits: u32) {
        if self.is_zero() {
            return;
        }
        let words = (bits / 32) as usize;
        let bits = bits % 32;
        if bits > 0 {
            let mut carry = 0u32;
            for limb in &mut self.limbs {
                let next = *limb >> (32 - bits);
                *limb = (*limb << bits) | carry;
                carry = next;
            }
            if carry != 0 {
                self.limbs.push(carry);
            }
        }
        if words > 0 {
            self.limbs.splice(0..0, std::iter::repeat_n(0, words));
        }
    }

    /// Multiplies by `factor`.
    pub(crate) fn mul_small(&mut self, factor: u32) {
        if factor == 0 {
            self.limbs.clear();
            return;
        }
        let mut carry = 0u64;
        for limb in &mut self.limbs {
            let product = u64::from(*limb) * u64::from(factor) + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }

    /// Multiplies by 10^`exponent`.
    pub(crate) fn mul_pow10(&mut self, exponent: u32) {
        self.mul_pow(10, exponent);
    }

    /// Multiplies by `base`^`exponent` for a `base` from 2 to 36, with as
    /// many factors of `base` per limb step as fit in 32 bits.
    pub(crate) fn mul_pow(&mut self, base: u32, exponent: u32) {
        let (chunk, per_chunk) = largest_power(base);
        let mut rest = exponent;
        while rest >= per_chunk {
            self.mul_small(chunk);
            rest -= per_chunk;
        }
        if rest > 0 {
            self.mul_small(base.pow(rest));
        }
    }

    /// Adds `value`.
    pub(crate) fn add_small(&mut self, value: u32) {
        let mut carry = value;
        for limb in &mut self.limbs {
            if carry == 0 {
                return;
            }
            let (sum, overflow) = limb.overflowing_add(carry);
            *limb = sum;
            carry = u32::from(overflow);
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
    }

    /// Adds `other`.
    pub(crate) fn add_assign(&mut self, other: &Natural) {
        if self.limbs.len() < other.limbs.len() {
            self.limbs.resize(other.limbs.len(), 0);
        }
        let mut carry = 0u64;
        for (i, limb) in self.limbs.iter_mut().enumerate() {
            let add = u64::from(other.limbs.get(i).copied().unwrap_or(0));
            if add == 0 && carry == 0 && i >= other.limbs.len() {
                break;
            }
            let sum = u64::from(*limb) + add + carry;
            *limb = sum as u32;
            carry = sum >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }

    /// Whether any of the bits below bit `shift` is set.
    fn any_bit_below(&self, shift: u64) -> bool {
        let word = (shift / 32) as usize;
        let bits = (shift % 32) as u32;
        let whole = self.limbs.iter().take(word).any(|&limb| limb != 0);
        let part = bits > 0
            && self
                .limbs
                .get(word)
                .is_some_and(|&limb| limb & ((1 << bits) - 1) != 0);
        whole || part
    }

    /// The value rounded to the nearest double, ties to even; infinity
    /// from 2^1024 − 2^970 upward (the tie above `f64::MAX` goes to the
    /// even 2^1024, which overflows).
    pub(crate) fn to_f64(&self) -> f64 {
        let len = self.bit_len();
        if len <= 53 {
            // Exact.
            return self.bits_from(0) as f64;
        }
        if len > 1024 {
            return f64::INFINITY;
        }
        // The top 53 bits, the next bit, and whether a lower bit is set.
        let shift = len - 54;
        let top = self.bits_from(shift) as u64;
        let mut mantissa = top >> 1;
        let half = top & 1 == 1;
        if half && (mantissa & 1 == 1 || self.any_bit_below(shift)) {
            mantissa += 1;
        }
        // mantissa ≤ 2^53 converts exactly; the scale 2^(shift + 1) is at
        // most 2^971, and the product overflows to infinity only for
        // 2^53 × 2^971.
        let scale = f64::from_bits((shift + 1 + 1023) << 52);
        mantissa as f64 * scale
    }

    /// Subtracts `other`, which must not be larger.
    pub(crate) fn sub_assign(&mut self, other: &Natural) {
        debug_assert!(*self >= *other, "a natural minus a larger one");
        let mut borrow = 0u64;
        for (i, limb) in self.limbs.iter_mut().enumerate() {
            let sub = u64::from(other.limbs.get(i).copied().unwrap_or(0)) + borrow;
            let current = u64::from(*limb);
            if current >= sub {
                *limb = (current - sub) as u32;
                borrow = 0;
            } else {
                *limb = (current + (1 << 32) - sub) as u32;
                borrow = 1;
            }
            if borrow == 0 && i + 1 >= other.limbs.len() {
                break;
            }
        }
        self.trim();
    }

    /// Subtracts `other` × `factor`, which must not be larger.
    fn sub_mul_small(&mut self, other: &Natural, factor: u32) {
        let mut carry = 0u64;
        let mut borrow = 0u64;
        for (i, limb) in self.limbs.iter_mut().enumerate() {
            let product =
                u64::from(other.limbs.get(i).copied().unwrap_or(0)) * u64::from(factor) + carry;
            carry = product >> 32;
            let sub = (product & 0xFFFF_FFFF) + borrow;
            let current = u64::from(*limb);
            if current >= sub {
                *limb = (current - sub) as u32;
                borrow = 0;
            } else {
                *limb = (current + (1 << 32) - sub) as u32;
                borrow = 1;
            }
        }
        debug_assert!(carry == 0 && borrow == 0, "a natural minus a larger one");
        self.trim();
    }

    /// The bits from `shift` upward, as a `u128`; the caller knows that
    /// they fit.
    fn bits_from(&self, shift: u64) -> u128 {
        let word = (shift / 32) as usize;
        let bits = (shift % 32) as u32;
        let mut value = 0u128;
        for (i, &limb) in self.limbs.iter().enumerate().skip(word).take(5) {
            let position = (i - word) as u32 * 32;
            value |= u128::from(limb) << position;
        }
        value >> bits
    }

    /// Replaces `self` with `self mod divisor` and returns
    /// `floor(self / divisor)`, which must be below 2^32 and in practice is
    /// below 36 (`self` < 36 × `divisor`). `divisor` is not 0.
    pub(crate) fn quotient_digit(&mut self, divisor: &Natural) -> u32 {
        if *self < *divisor {
            return 0;
        }
        let shift = divisor.bit_len().saturating_sub(64);
        let numerator = self.bits_from(shift);
        let denominator = divisor.bits_from(shift);
        // With a shift, the divisor's bits are rounded down; one more
        // makes the estimate never too large (and at most two too small).
        let estimate = if shift == 0 {
            numerator / denominator
        } else {
            numerator / (denominator + 1)
        };
        let mut quotient = u32::try_from(estimate).unwrap_or(u32::MAX);
        if quotient > 0 {
            self.sub_mul_small(divisor, quotient);
        }
        while *self >= *divisor {
            self.sub_assign(divisor);
            quotient += 1;
        }
        quotient
    }
}

/// The largest power of `base` (2 to 36) that fits in 32 bits, and its
/// exponent.
pub(crate) fn largest_power(base: u32) -> (u32, u32) {
    let mut power = base;
    let mut exponent = 1;
    while let Some(next) = power.checked_mul(base) {
        power = next;
        exponent += 1;
    }
    (power, exponent)
}

impl PartialOrd for Natural {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Natural {
    fn cmp(&self, other: &Self) -> Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: u64) -> Natural {
        Natural::from_u64(value)
    }

    #[test]
    fn arithmetic_matches_u128() {
        let mut a = n(0xFFFF_FFFF_FFFF);
        a.mul_small(1_000_000_007);
        assert_eq!(a.bits_from(0), 0xFFFF_FFFF_FFFFu128 * 1_000_000_007);
        a.shl(37);
        assert_eq!(a.bits_from(0), (0xFFFF_FFFF_FFFFu128 * 1_000_000_007) << 37);
        a.sub_assign(&n(1));
        assert_eq!(
            a.bits_from(0),
            ((0xFFFF_FFFF_FFFFu128 * 1_000_000_007) << 37) - 1
        );
        let mut b = n(3);
        b.mul_pow10(20);
        assert_eq!(b.bits_from(0), 300_000_000_000_000_000_000);
        assert_eq!(n(0).bit_len(), 0);
        assert_eq!(n(1).bit_len(), 1);
        assert_eq!(n(u64::MAX).bit_len(), 64);
        assert!(n(5) < n(6));
        assert!(b > a.clone() || b <= a);
    }

    #[test]
    fn quotient_digits_of_large_numbers() {
        // divisor = 3^200 (about 317 bits); dividend = divisor × q + r.
        let mut divisor = n(1);
        for _ in 0..200 {
            divisor.mul_small(3);
        }
        let below = {
            let mut d = divisor.clone();
            d.sub_assign(&n(1));
            d
        };
        for q in [0u32, 1, 2, 9, 17, 35] {
            for rest in [n(0), n(1), n(12_345), below.clone()] {
                let mut product = divisor.clone();
                product.mul_small(q);
                let mut work = product.clone();
                for (i, &limb) in rest.limbs.iter().enumerate() {
                    let mut part = n(u64::from(limb));
                    part.shl(32 * i as u32);
                    work = add(&work, &part);
                }
                assert_eq!(work.quotient_digit(&divisor), q, "q={q}");
                assert_eq!(work, rest, "q={q}");
            }
        }
        // A one-limb divisor: the exact path.
        let mut work = n(99);
        assert_eq!(work.quotient_digit(&n(10)), 9);
        assert_eq!(work, n(9));
    }

    /// `a + b`.
    fn add(a: &Natural, b: &Natural) -> Natural {
        let mut out = Natural::default();
        let mut carry = 0u64;
        for i in 0..a.limbs.len().max(b.limbs.len()) {
            let sum = u64::from(a.limbs.get(i).copied().unwrap_or(0))
                + u64::from(b.limbs.get(i).copied().unwrap_or(0))
                + carry;
            out.limbs.push(sum as u32);
            carry = sum >> 32;
        }
        if carry > 0 {
            out.limbs.push(carry as u32);
        }
        out.trim();
        out
    }
}
