//! The exact sum of doubles for `Math.sumPrecise` (ECMA-262 2026
//! §21.3.2.34) and the binary16 rounding of `Math.f16round` (§21.3.2.18).
//!
//! The sum is a fixed-point accumulator: every finite double is an integer
//! multiple of 2^-1074 below 2^1024, and fewer than 2^53 of them sum to
//! less than 2^2151, so an integer of 2,176 bits in two's complement holds
//! every partial sum exactly. An addition touches two limbs and carries at
//! most through all 34; the result is rounded once, to nearest, ties to
//! even (an exact sum of 2^1024 − 2^970 or more becomes infinity, as the
//! Number value of the specification does).

use super::format::decompose;

/// The limbs of the accumulator (34 × 64 = 2,176 bits).
const LIMBS: usize = 34;

/// An exact sum of finite doubles.
#[derive(Clone, Debug)]
pub struct ExactSum {
    /// The sum × 2^1074, two's complement, least significant limb first.
    limbs: [u64; LIMBS],
}

impl Default for ExactSum {
    fn default() -> Self {
        ExactSum { limbs: [0; LIMBS] }
    }
}

impl ExactSum {
    /// An empty sum (0).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a finite double (NaN and infinities are the caller's cases).
    pub fn add(&mut self, x: f64) {
        if x == 0.0 || !x.is_finite() {
            return;
        }
        let (mantissa, exponent) = decompose(x.abs());
        let position = (exponent + 1074) as u32;
        let index = (position / 64) as usize;
        let wide = u128::from(mantissa) << (position % 64);
        let parts = [wide as u64, (wide >> 64) as u64];
        if x > 0.0 {
            let mut carry = 0u64;
            for (i, limb) in self.limbs.iter_mut().enumerate().skip(index) {
                let add = parts.get(i - index).copied().unwrap_or(0);
                let (sum, c1) = limb.overflowing_add(add);
                let (sum, c2) = sum.overflowing_add(carry);
                *limb = sum;
                carry = u64::from(c1 || c2);
                if carry == 0 && i > index {
                    break;
                }
            }
        } else {
            let mut borrow = 0u64;
            for (i, limb) in self.limbs.iter_mut().enumerate().skip(index) {
                let sub = parts.get(i - index).copied().unwrap_or(0);
                let (diff, b1) = limb.overflowing_sub(sub);
                let (diff, b2) = diff.overflowing_sub(borrow);
                *limb = diff;
                borrow = u64::from(b1 || b2);
                if borrow == 0 && i > index {
                    break;
                }
            }
        }
    }

    /// The sum rounded to the nearest double, ties to even; +0 for an
    /// exact zero.
    pub fn value(&self) -> f64 {
        let mut magnitude = self.limbs;
        let negative = magnitude[LIMBS - 1] >> 63 == 1;
        if negative {
            // Two's complement negation.
            let mut carry = 1u64;
            for limb in &mut magnitude {
                let (sum, c) = (!*limb).overflowing_add(carry);
                *limb = sum;
                carry = u64::from(c);
            }
        }
        let Some(top_limb) = magnitude.iter().rposition(|&l| l != 0) else {
            return 0.0;
        };
        let top = top_limb as u32 * 64 + magnitude[top_limb].ilog2();
        let bit = |i: u32| (magnitude[(i / 64) as usize] >> (i % 64)) & 1;
        let result = if top < 53 {
            // Below 2^53 × 2^-1074: exact.
            magnitude[0] as f64 * f64::from_bits(1)
        } else {
            let low = top - 52;
            let mut mantissa = 0u64;
            for i in (low..=top).rev() {
                mantissa = (mantissa << 1) | bit(i);
            }
            let half = bit(low - 1) == 1;
            let sticky = (0..low - 1).any(|i| bit(i) == 1);
            let mut top = top;
            if half && (sticky || mantissa & 1 == 1) {
                mantissa += 1;
                if mantissa == 1 << 53 {
                    mantissa >>= 1;
                    top += 1;
                }
            }
            // mantissa × 2^(top - 52 - 1074); the biased exponent of the
            // double is top - 52 - 1074 + 52 + 1023.
            let biased = u64::from(top) - 51;
            if biased >= 0x7FF {
                f64::INFINITY
            } else {
                f64::from_bits((biased << 52) | (mantissa & ((1 << 52) - 1)))
            }
        };
        if negative { -result } else { result }
    }
}

/// `Math.f16round(x)` (§21.3.2.18): `x` rounded to the nearest IEEE 754
/// binary16 value, ties to even, as a double (directly, not through
/// binary32, which could round twice).
pub fn f16_round(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 || x.is_infinite() {
        return x;
    }
    let magnitude = x.abs();
    // The spacing of binary16 values at this magnitude: 2^-24 below the
    // normal range (2^-14), else 2^(e - 10).
    let (_, exponent) = decompose(magnitude);
    let floor_log2 = if magnitude < f64::MIN_POSITIVE {
        -1075
    } else {
        exponent + 52
    };
    let quantum_exponent = (floor_log2 - 10).max(-24);
    let quantum = 2f64.powi(quantum_exponent);
    // Exact: scaling by a power of two that stays in range.
    let rounded = (magnitude / quantum).round_ties_even() * quantum;
    let rounded = if rounded > 65504.0 {
        f64::INFINITY
    } else {
        rounded
    };
    rounded.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(values: &[f64]) -> f64 {
        let mut s = ExactSum::new();
        for &v in values {
            s.add(v);
        }
        s.value()
    }

    #[test]
    fn sums_are_exact() {
        // Expected values from Chromium 148 (`Math.sumPrecise`).
        assert_eq!(sum(&[1e20, 0.1, -1e20]), 0.1);
        assert_eq!(sum(&[0.1, 0.2]), 0.300_000_000_000_000_04);
        assert_eq!(sum(&[1e308, 1e308, -1e308]), 1e308);
        assert_eq!(sum(&[1e308, 1e308]), f64::INFINITY);
        assert_eq!(sum(&[-1e308, -1e308]), f64::NEG_INFINITY);
        assert_eq!(sum(&[1.0, 1e100, 1.0, -1e100]), 2.0);
        assert_eq!(sum(&[2f64.powi(53), 1.0, 1.0]), 9_007_199_254_740_994.0);
        assert_eq!(sum(&[5e-324, 5e-324]), 1e-323);
        assert_eq!(sum(&[f64::MAX, 9.979_201_547_673_598e291]), f64::MAX);
        // A tie at the top: MAX + half an ulp is 2^1024, infinity.
        assert_eq!(sum(&[f64::MAX, 2f64.powi(970)]), f64::INFINITY);
        assert_eq!(sum(&[f64::MAX, 2f64.powi(970), -f64::MAX]), 2f64.powi(970));
        assert_eq!(sum(&[3.0, -3.0]), 0.0);
        assert!(sum(&[3.0, -3.0]).is_sign_positive());
        assert_eq!(sum(&[-0.5, -2f64.powi(-60)]), -0.5);
        assert_eq!(sum(&[1.0, 2f64.powi(-53)]), 1.0);
        assert_eq!(
            sum(&[1.0, 2f64.powi(-53), f64::from_bits(1)]),
            1.000_000_000_000_000_2
        );
        assert_eq!(
            sum(&[-f64::MIN_POSITIVE, 5e-324]),
            -2.225_073_858_507_201e-308
        );
        // Many terms that cancel.
        let mut s = ExactSum::new();
        for i in 0..10_000 {
            s.add(if i % 2 == 0 { f64::MAX } else { -f64::MAX });
            s.add(1e-300);
        }
        assert_eq!(s.value(), 1e-296);
    }

    #[test]
    fn f16_rounding() {
        // Expected values from Chromium 148 (`Math.f16round`).
        let cases = [
            (1.337, 1.336_914_062_5),
            (65504.0, 65504.0),
            (65519.99999, 65504.0),
            (65520.0, f64::INFINITY),
            (6.103_515_625e-5, 6.103_515_625e-5),
            (5.960_464_477_539_063e-8, 5.960_464_477_539_063e-8),
            (2.980_232_238_769_531_2e-8, 0.0),
            (2.980_232_238_769_532e-8, 5.960_464_477_539_063e-8),
            (1e-8, 0.0),
            (1.000_976_562_5, 1.000_976_562_5),
            (1.000_488_281_25, 1.0),
            (1.000_488_281_250_001, 1.000_976_562_5),
            (1.001_464_843_75, 1.001_953_125),
            (0.1, 0.099_975_585_937_5),
            (5e-324, 0.0),
            (f64::MAX, f64::INFINITY),
        ];
        for (x, expected) in cases {
            assert_eq!(f16_round(x), expected, "{x}");
            assert_eq!(f16_round(-x), -expected, "{x}");
        }
        assert!(f16_round(-1e-8).is_sign_negative());
        assert!(f16_round(f64::NAN).is_nan());
    }
}
