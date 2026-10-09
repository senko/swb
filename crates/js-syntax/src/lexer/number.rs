//! Numeric literals (§12.9.3, Annex B.1.1 for the legacy forms).
//!
//! Decimal literals: the digits without separators go to Rust's `f64`
//! parser, which rounds correctly (ADR 0026 section 7). Hexadecimal, octal
//! and binary literals: an exact accumulator that rounds to nearest, ties
//! to even, like the MV-to-Number conversion of §12.9.3.3.

use swb_js_text::{CodeUnit, unicode};

use super::{INVALID_TOKEN, Lexeme, Lexer, ascii, error_at, is_ascii_id_start, is_decimal_digit};
use crate::SyntaxError;
use crate::token::{Legacy, TokenKind, TokenValue};

const SEPARATOR_AFTER_ZERO: &str = "Numeric separator can not be used after leading 0.";
const SEPARATOR_TWICE: &str = "Only one underscore is allowed as numeric separator";
const SEPARATOR_AT_END: &str = "Numeric separators are not allowed at the end of numeric literals";

/// The value of a digit in `radix` (2, 8, 10 or 16), or `None`.
fn digit_value(unit: u16, radix: u32) -> Option<u32> {
    let digit = char::from_u32(u32::from(unit))?.to_digit(16)?;
    (digit < radix).then_some(digit)
}

/// The exact value of a hexadecimal, octal or binary integer, rounded to a
/// double at the end. It keeps the first 64 significant bits, the number
/// of bits after them, and whether any of those bits is set.
struct RadixValue {
    bits_per_digit: u32,
    high: u64,
    dropped_bits: u64,
    sticky: bool,
}

impl RadixValue {
    fn new(radix: u32) -> Self {
        RadixValue {
            bits_per_digit: radix.trailing_zeros(),
            high: 0,
            dropped_bits: 0,
            sticky: false,
        }
    }

    fn push(&mut self, digit: u32) {
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
    fn finish(&self) -> f64 {
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

impl<U: CodeUnit> Lexer<'_, U> {
    /// Scans a `NumericLiteral` that starts with a digit or with `.` and a
    /// digit.
    pub(super) fn scan_number(&mut self, start: usize) -> Result<Lexeme, SyntaxError> {
        self.pos = start;
        if self.is_at(0, b'0') {
            match self.ahead(1).map(|u| u | 0x20) {
                Some(0x78) => return self.scan_radix_literal(start, 16),
                Some(0x6F) => return self.scan_radix_literal(start, 8),
                Some(0x62) => return self.scan_radix_literal(start, 2),
                _ => {}
            }
            if self.is_at(1, b'_') {
                return Err(error_at(start + 1, SEPARATOR_AFTER_ZERO));
            }
            if self.ahead(1).is_some_and(is_decimal_digit) {
                return self.scan_legacy_literal();
            }
        }
        self.digits.clear();
        if !self.is_at(0, b'.') {
            self.scan_decimal_digits()?;
        }
        self.scan_decimal_rest(false)
    }

    /// Scans decimal digits with separators into `self.digits`; returns
    /// their count.
    fn scan_decimal_digits(&mut self) -> Result<usize, SyntaxError> {
        let mut digits = std::mem::take(&mut self.digits);
        let result = self.scan_digits(10, true, |d| {
            digits.push(char::from(b'0' + d as u8));
        });
        self.digits = digits;
        result
    }

    /// Scans the fraction, exponent and `BigInt` suffix of a decimal
    /// literal whose integer part is in `self.digits`. `legacy` is set for
    /// a `NonOctalDecimalIntegerLiteral` (`08`), which cannot be a `BigInt`.
    fn scan_decimal_rest(&mut self, legacy: bool) -> Result<Lexeme, SyntaxError> {
        let mut integer = true;
        if self.is_at(0, b'.') {
            self.pos += 1;
            self.digits.push('.');
            self.scan_decimal_digits()?;
            integer = false;
        }
        if self.peek().is_some_and(|u| u | 0x20 == ascii(b'e')) {
            self.pos += 1;
            self.digits.push('e');
            if let Some(sign @ (0x2B | 0x2D)) = self.peek() {
                self.digits.push(char::from(sign as u8));
                self.pos += 1;
            }
            if self.scan_decimal_digits()? == 0 {
                return Err(error_at(self.pos, INVALID_TOKEN));
            }
            integer = false;
        }
        if self.is_at(0, b'n') {
            if !integer || legacy {
                return Err(error_at(self.pos, INVALID_TOKEN));
            }
            self.pos += 1;
            self.check_after_numeric()?;
            let text = self.digits.as_str().into();
            return Ok(Lexeme::with_value(
                TokenKind::BigInt,
                TokenValue::BigInt(text),
            ));
        }
        self.check_after_numeric()?;
        let value: f64 = self
            .digits
            .parse()
            .map_err(|_| error_at(self.pos, INVALID_TOKEN))?;
        Ok(Lexeme::with_value(
            TokenKind::Number,
            TokenValue::Number(value),
        ))
    }

    /// Scans `0` followed by a decimal digit: a `LegacyOctalIntegerLiteral`
    /// (`017`) or a `NonOctalDecimalIntegerLiteral` (`08`, `019.5`). Both
    /// are errors in strict mode code; neither allows separators.
    fn scan_legacy_literal(&mut self) -> Result<Lexeme, SyntaxError> {
        self.digits.clear();
        let mut octal = true;
        while let Some(unit) = self.peek().filter(|&u| is_decimal_digit(u)) {
            octal &= unit < ascii(b'8');
            self.digits.push(char::from(unit as u8));
            self.pos += 1;
        }
        if self.is_at(0, b'_') {
            return Err(error_at(self.pos, INVALID_TOKEN));
        }
        let mut lexeme = if octal {
            if self.is_at(0, b'n') {
                return Err(error_at(self.pos, INVALID_TOKEN));
            }
            self.check_after_numeric()?;
            let mut value = RadixValue::new(8);
            for digit in self.digits.bytes() {
                value.push(u32::from(digit - b'0'));
            }
            Lexeme::with_value(TokenKind::Number, TokenValue::Number(value.finish()))
        } else {
            self.scan_decimal_rest(true)?
        };
        lexeme.legacy = if octal {
            Legacy::OctalInteger
        } else {
            Legacy::LeadingZeroDecimal
        };
        Ok(lexeme)
    }

    /// Scans a hexadecimal, octal or binary literal (`0x`, `0o`, `0b`).
    fn scan_radix_literal(&mut self, start: usize, radix: u32) -> Result<Lexeme, SyntaxError> {
        self.pos = start + 2;
        let digits_start = self.pos;
        let mut value = RadixValue::new(radix);
        if self.scan_digits(radix, true, |d| value.push(d))? == 0 {
            return Err(error_at(self.pos, INVALID_TOKEN));
        }
        if self.is_at(0, b'n') {
            let prefix = match radix {
                16 => "0x",
                8 => "0o",
                _ => "0b",
            };
            let mut text = String::from(prefix);
            for unit in self.slice(digits_start, self.pos) {
                let unit = unit.value();
                if unit != ascii(b'_') {
                    text.push(char::from(unit as u8));
                }
            }
            self.pos += 1;
            self.check_after_numeric()?;
            return Ok(Lexeme::with_value(
                TokenKind::BigInt,
                TokenValue::BigInt(text.into()),
            ));
        }
        self.check_after_numeric()?;
        Ok(Lexeme::with_value(
            TokenKind::Number,
            TokenValue::Number(value.finish()),
        ))
    }

    /// Scans digits of `radix`, with `NumericLiteralSeparators` between
    /// digits if `separators` is set, and passes each digit value to
    /// `push`. Returns the number of digits. A separator before the first
    /// digit is not consumed.
    fn scan_digits(
        &mut self,
        radix: u32,
        separators: bool,
        mut push: impl FnMut(u32),
    ) -> Result<usize, SyntaxError> {
        let mut count = 0;
        while let Some(unit) = self.peek() {
            if let Some(digit) = digit_value(unit, radix) {
                push(digit);
                count += 1;
                self.pos += 1;
            } else if unit == ascii(b'_') && separators && count > 0 {
                match self.ahead(1) {
                    Some(0x5F) => return Err(error_at(self.pos + 1, SEPARATOR_TWICE)),
                    Some(next) if digit_value(next, radix).is_some() => self.pos += 1,
                    _ => return Err(error_at(self.pos, SEPARATOR_AT_END)),
                }
            } else {
                break;
            }
        }
        Ok(count)
    }

    /// The source character after a `NumericLiteral` must not be an
    /// `IdentifierStart` or a `DecimalDigit` (§12.9.3).
    fn check_after_numeric(&self) -> Result<(), SyntaxError> {
        let Some(unit) = self.peek() else {
            return Ok(());
        };
        let bad = if unit < 0x80 {
            is_decimal_digit(unit) || is_ascii_id_start(unit) || unit == ascii(b'\\')
        } else {
            self.code_point_at(self.pos)
                .is_some_and(|(cp, _)| unicode::is_identifier_start(cp))
        };
        if bad {
            return Err(error_at(self.pos, INVALID_TOKEN));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radix_value(radix: u32, digits: &str) -> f64 {
        let mut value = RadixValue::new(radix);
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
