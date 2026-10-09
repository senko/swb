//! Numeric literals (§12.9.3, Annex B.1.1 for the legacy forms).
//!
//! Decimal literals: the digits without separators go to Rust's `f64`
//! parser, which rounds correctly (ADR 0026 section 7). Hexadecimal, octal
//! and binary literals: an exact accumulator that rounds to nearest, ties
//! to even, like the MV-to-Number conversion of §12.9.3.3.

use swb_js_text::{CodeUnit, RadixAccumulator, unicode};

use super::{Lexeme, Lexer, ascii, error_at, is_ascii_id_start, is_decimal_digit};
use crate::SyntaxError;
use crate::messages::{INVALID_TOKEN, SEPARATOR_AFTER_ZERO, SEPARATOR_AT_END, SEPARATOR_TWICE};
use crate::token::{Legacy, TokenKind, TokenValue};

/// The value of a digit in `radix` (2, 8, 10 or 16), or `None`.
pub(super) fn digit_value(unit: u16, radix: u32) -> Option<u32> {
    let digit = char::from_u32(u32::from(unit))?.to_digit(16)?;
    (digit < radix).then_some(digit)
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
                return self.scan_legacy_literal(start);
            }
        }
        self.digits.clear();
        if !self.is_at(0, b'.') {
            self.scan_decimal_digits()?;
        }
        self.scan_decimal_rest(false, start)
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
    fn scan_decimal_rest(&mut self, legacy: bool, start: usize) -> Result<Lexeme, SyntaxError> {
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
                return Err(error_at(start, INVALID_TOKEN));
            }
            integer = false;
        }
        if self.is_at(0, b'n') {
            if !integer || legacy {
                return Err(error_at(start, INVALID_TOKEN));
            }
            self.pos += 1;
            self.check_after_numeric(start)?;
            let text = self.digits.as_str().into();
            return Ok(Lexeme::with_value(
                TokenKind::BigInt,
                TokenValue::BigInt(text),
            ));
        }
        self.check_after_numeric(start)?;
        let value: f64 = self
            .digits
            .parse()
            .map_err(|_| error_at(start, INVALID_TOKEN))?;
        Ok(Lexeme::with_value(
            TokenKind::Number,
            TokenValue::Number(value),
        ))
    }

    /// Scans `0` followed by a decimal digit: a `LegacyOctalIntegerLiteral`
    /// (`017`) or a `NonOctalDecimalIntegerLiteral` (`08`, `019.5`). Both
    /// are errors in strict mode code; neither allows separators.
    fn scan_legacy_literal(&mut self, start: usize) -> Result<Lexeme, SyntaxError> {
        self.digits.clear();
        let mut octal = true;
        while let Some(unit) = self.peek().filter(|&u| is_decimal_digit(u)) {
            octal &= unit < ascii(b'8');
            self.digits.push(char::from(unit as u8));
            self.pos += 1;
        }
        if self.is_at(0, b'_') {
            // V8 marks the start of an octal literal, the separator in a
            // decimal one.
            return Err(error_at(
                if octal { start } else { self.pos },
                INVALID_TOKEN,
            ));
        }
        let mut lexeme = if octal {
            if self.is_at(0, b'n') {
                return Err(error_at(start, INVALID_TOKEN));
            }
            self.check_after_numeric(start)?;
            let mut value = RadixAccumulator::new(8);
            for digit in self.digits.bytes() {
                value.push(u32::from(digit - b'0'));
            }
            Lexeme::with_value(TokenKind::Number, TokenValue::Number(value.finish()))
        } else {
            self.scan_decimal_rest(true, start)?
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
        let mut value = RadixAccumulator::new(radix);
        if self.scan_digits(radix, true, |d| value.push(d))? == 0 {
            return Err(error_at(start, INVALID_TOKEN));
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
            self.check_after_numeric(start)?;
            return Ok(Lexeme::with_value(
                TokenKind::BigInt,
                TokenValue::BigInt(text.into()),
            ));
        }
        self.check_after_numeric(start)?;
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
                    _ => return Err(error_at(self.pos + 1, SEPARATOR_AT_END)),
                }
            } else {
                break;
            }
        }
        Ok(count)
    }

    /// The source character after a `NumericLiteral` must not be an
    /// `IdentifierStart` or a `DecimalDigit` (§12.9.3).
    fn check_after_numeric(&self, start: usize) -> Result<(), SyntaxError> {
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
            return Err(error_at(start, INVALID_TOKEN));
        }
        Ok(())
    }
}
