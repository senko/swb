//! The IEEE 754 binary16 format of `DataView.prototype.getFloat16` and
//! `setFloat16` (ECMA-262 §25.1.3.14 `RawBytesToNumeric` and §25.1.3.17
//! `NumericToRawBytes`, <https://tc39.es/ecma262/#sec-numerictorawbytes>)
//! and of `Float16Array`: the bits of a double rounded to binary16 (round
//! to nearest, ties to even, with [`f16_round`]), and the double of
//! binary16 bits.
//!
//! NaN: the specification lets the implementation choose the bits. These
//! are the bits that Chromium 148 writes and reads (measured with
//! `tools/probes/js-binary.json`): a NaN becomes the quiet NaN `0x7E00`
//! with the sign of the double, and a binary16 NaN becomes the quiet
//! double NaN with the same sign and the payload in the top bits of the
//! fraction, as the hardware conversion between binary32 and binary64 does.

use super::sum::f16_round;

/// The sign bit of a binary16 value.
const SIGN: u16 = 0x8000;
/// The exponent bits of infinity and NaN.
const EXPONENT: u16 = 0x7C00;
/// The quiet bit of a NaN.
const QUIET: u16 = 0x0200;
/// The fraction bits.
const FRACTION: u16 = 0x03FF;

/// The binary16 bits of `x`, rounded to nearest, ties to even.
pub fn f16_to_bits(x: f64) -> u16 {
    let sign = if x.is_sign_negative() { SIGN } else { 0 };
    if x.is_nan() {
        return sign | EXPONENT | QUIET;
    }
    let rounded = f16_round(x).abs();
    if rounded.is_infinite() {
        return sign | EXPONENT;
    }
    if rounded == 0.0 {
        return sign;
    }
    let bits = rounded.to_bits();
    // The rounded value is exact in binary16, so these shifts drop only
    // zero bits.
    let exponent = ((bits >> 52) & 0x7FF) as i32 - 1023;
    if exponent < -14 {
        // Subnormal: a multiple of 2^-24 below 2^-14.
        let fraction = (rounded * 16_777_216.0) as u16;
        return sign | fraction;
    }
    let biased = (exponent + 15) as u16;
    let fraction = ((bits >> 42) & u64::from(FRACTION)) as u16;
    sign | (biased << 10) | fraction
}

/// The double of binary16 bits.
pub fn f16_from_bits(bits: u16) -> f64 {
    let negative = bits & SIGN != 0;
    let exponent = (bits & EXPONENT) >> 10;
    let fraction = u64::from(bits & FRACTION);
    let sign = if negative { 1u64 << 63 } else { 0 };
    let magnitude = match exponent {
        0 => fraction as f64 / 16_777_216.0,
        0x1F if fraction == 0 => f64::INFINITY,
        0x1F => return f64::from_bits(sign | 0x7FF8_0000_0000_0000 | (fraction << 42)),
        _ => f64::from_bits(((u64::from(exponent) + 1023 - 15) << 52) | (fraction << 42)),
    };
    if negative { -magnitude } else { magnitude }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_round_trip() {
        // Every binary16 value that is not a NaN gives its bits back.
        for bits in 0..=u16::MAX {
            let value = f16_from_bits(bits);
            if value.is_nan() {
                continue;
            }
            assert_eq!(f16_to_bits(value), bits, "{bits:#06x}");
        }
    }

    #[test]
    fn values_of_chromium() {
        // Expected bits from Chromium 148 (`setFloat16`).
        let cases = [
            (1.0, 0x3C00),
            (65504.0, 0x7BFF),
            (65519.99, 0x7BFF),
            (65520.0, 0x7C00),
            (-65520.0, 0xFC00),
            (2049.0, 0x6800),
            (2051.0, 0x6802),
            (5.960_464_477_539_063e-8, 0x0001),
            (2.980_232_238_769_531_2e-8, 0x0000),
            (2.980_232_238_769_532e-8, 0x0001),
            (6.103_515_625e-5, 0x0400),
            (6.097_555_160_522_461e-5, 0x03FF),
            (-0.0, 0x8000),
            (0.1, 0x2E66),
            (f64::INFINITY, 0x7C00),
            (f64::NEG_INFINITY, 0xFC00),
            (f64::NAN, 0x7E00),
            (-f64::NAN, 0xFE00),
        ];
        for (x, bits) in cases {
            assert_eq!(f16_to_bits(x), bits, "{x}");
        }
    }

    #[test]
    fn nans_keep_sign_and_payload() {
        assert_eq!(f16_from_bits(0x7C01).to_bits(), 0x7FF8_0400_0000_0000);
        assert_eq!(f16_from_bits(0x7E00).to_bits(), 0x7FF8_0000_0000_0000);
        assert_eq!(f16_from_bits(0xFE00).to_bits(), 0xFFF8_0000_0000_0000);
        assert_eq!(f16_from_bits(0x3555), 0.333_251_953_125);
        assert_eq!(f16_from_bits(0x0001), 5.960_464_477_539_063e-8);
        assert!(f16_from_bits(0x8000).is_sign_negative());
    }
}
