//! Exact conversion of hexadecimal, octal and binary digit strings to
//! numbers, for numeric literals (§12.9.3.3) and `StringToNumber`
//! (§7.1.4.1.1).

/// The exact value of a hexadecimal, octal or binary integer, rounded to a
/// double at the end. It keeps the first 64 significant bits, the number
/// of bits after them, and whether any of those bits is set.
pub struct RadixAccumulator {
    bits_per_digit: u32,
    high: u64,
    dropped_bits: u64,
    sticky: bool,
}

impl RadixAccumulator {
    /// An accumulator for digits of `radix` (2, 8 or 16).
    pub fn new(radix: u32) -> Self {
        RadixAccumulator {
            bits_per_digit: radix.trailing_zeros(),
            high: 0,
            dropped_bits: 0,
            sticky: false,
        }
    }

    /// Appends one digit (its value, below the radix).
    pub fn push(&mut self, digit: u32) {
        let bits = self.bits_per_digit;
        let digit = u64::from(digit);
        let free = self.high.leading_zeros();
        if free >= bits {
            self.high = (self.high << bits) | digit;
        } else {
            // The digit's top `free` bits fit; the rest are dropped.
            let rest = bits - free;
            self.high = (self.high << free) | (digit >> rest);
            self.sticky |= digit & ((1 << rest) - 1) != 0;
            self.dropped_bits += u64::from(rest);
        }
    }

    /// The value rounded to the nearest double, ties to even.
    pub fn finish(&self) -> f64 {
        if self.high == 0 {
            return 0.0;
        }
        let significant = 64 - self.high.leading_zeros();
        let (mantissa, shift) = if significant <= 53 {
            (self.high, 0)
        } else {
            let shift = significant - 53;
            let mut mantissa = self.high >> shift;
            let rest = self.high & ((1 << shift) - 1);
            let half = 1 << (shift - 1);
            if rest > half || (rest == half && (self.sticky || mantissa & 1 == 1)) {
                mantissa += 1;
            }
            (mantissa, shift)
        };
        // The mantissa has at most 54 bits (2^53 after a carry), so the
        // conversion is exact; the scaling by a power of two is exact or
        // overflows to infinity.
        scale(mantissa as f64, u64::from(shift) + self.dropped_bits)
    }
}

/// `value` times 2^`exponent`.
fn scale(value: f64, exponent: u64) -> f64 {
    if exponent > 2100 {
        return f64::INFINITY;
    }
    let mut value = value;
    let mut rest = exponent as u32;
    while rest > 0 {
        let step = rest.min(1000);
        value *= f64::from_bits(u64::from(step + 1023) << 52);
        rest -= step;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radix_value(radix: u32, digits: &str) -> f64 {
        let mut value = RadixAccumulator::new(radix);
        for c in digits.chars() {
            value.push(c.to_digit(radix).expect("test digits are valid"));
        }
        value.finish()
    }

    #[test]
    fn radix_values_round_to_nearest_even() {
        assert_eq!(radix_value(16, "0"), 0.0);
        assert_eq!(radix_value(16, "ff"), 255.0);
        assert_eq!(radix_value(16, "1fffffffffffff"), 9_007_199_254_740_991.0);
        // 2^53 + 1: a tie, rounds down to the even 2^53.
        assert_eq!(radix_value(16, "20000000000001"), 9_007_199_254_740_992.0);
        // 2^53 + 3: a tie, rounds up to the even 2^53 + 4.
        assert_eq!(radix_value(16, "20000000000003"), 9_007_199_254_740_996.0);
        // 2^53 + 1 followed by a set bit far below: above the tie.
        assert_eq!(
            radix_value(2, &format!("1{}1{}1", "0".repeat(52), "0".repeat(80))),
            (9_007_199_254_740_994.0f64) * 2f64.powi(81)
        );
        assert_eq!(
            radix_value(16, "1fffffffffffff1"),
            144_115_188_075_855_860.0
        );
        assert_eq!(
            radix_value(8, "7777777777777777777777"),
            73_786_976_294_838_210_000.0
        );
        assert_eq!(radix_value(16, &"f".repeat(256)), f64::INFINITY);
        assert_eq!(
            radix_value(16, &format!("1{}", "0".repeat(256))),
            f64::INFINITY
        );
        assert_eq!(
            radix_value(16, &format!("1{}", "0".repeat(255))),
            2f64.powi(1020)
        );
        assert_eq!(radix_value(2, &"1".repeat(1024)), f64::INFINITY);
        // 2^1023 - 1 rounds up to 2^1023.
        assert_eq!(radix_value(2, &"1".repeat(1023)), 2f64.powi(1023));
    }
}
