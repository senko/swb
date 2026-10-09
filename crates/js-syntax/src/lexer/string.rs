//! String literals (§12.9.4), template parts (§12.9.6) and their escape
//! sequences, with the legacy octal escapes of Annex B.1.2.

use swb_js_text::{CodeUnit, String16};

use super::number::digit_value;
use super::{CR, LF, LS, Lexeme, Lexer, PS, ascii, error_at, is_decimal_digit};
use crate::SyntaxError;
use crate::messages::{
    EIGHT_NINE_IN_TEMPLATE, INVALID_HEX_ESCAPE, INVALID_TOKEN, INVALID_UNICODE_ESCAPE,
    OCTAL_IN_TEMPLATE, UNDEFINED_CODE_POINT, UNEXPECTED_END,
};
use crate::token::{Legacy, Template, TokenKind, TokenValue};

const BACKSLASH: u16 = ascii(b'\\');
const BACKTICK: u16 = ascii(b'`');
const DOLLAR: u16 = ascii(b'$');

impl<U: CodeUnit> Lexer<'_, U> {
    /// Scans a `StringLiteral`.
    pub(super) fn scan_string(&mut self, start: usize) -> Result<Lexeme, SyntaxError> {
        let quote = self.peek().unwrap_or(0);
        self.pos = start + 1;
        let mut value = String16::new();
        let mut legacy = Legacy::None;
        loop {
            let run = self.pos;
            while let Some(unit) = self.peek() {
                if unit == quote || unit == BACKSLASH || unit == LF || unit == CR {
                    break;
                }
                self.pos += 1;
            }
            value.push_units(self.slice(run, self.pos));
            match self.peek() {
                Some(unit) if unit == quote => {
                    self.pos += 1;
                    break;
                }
                Some(BACKSLASH) => {
                    let kind = self.scan_escape(&mut value, false)?;
                    if legacy == Legacy::None {
                        legacy = kind;
                    }
                }
                // The end of the source or a line terminator (U+2028 and
                // U+2029 are allowed in strings).
                _ => return Err(error_at(start, INVALID_TOKEN)),
            }
        }
        Ok(Lexeme {
            legacy,
            ..Lexeme::with_value(TokenKind::String, TokenValue::String(value))
        })
    }

    /// Scans a template part that starts with `` ` `` (`head`) or `}`: a
    /// `NoSubstitutionTemplate`, `TemplateHead`, `TemplateMiddle` or
    /// `TemplateTail`. An invalid escape sequence does not end the scan; it
    /// makes the cooked value absent (§12.9.6, `NotEscapeSequence`).
    pub(super) fn scan_template(
        &mut self,
        start: usize,
        head: bool,
    ) -> Result<Lexeme, SyntaxError> {
        self.pos = start + 1;
        let mut cooked = String16::new();
        let mut cooked_error = None;
        let mut raw = String16::new();
        let kind = loop {
            let run = self.pos;
            while let Some(unit) = self.peek() {
                if matches!(unit, BACKTICK | BACKSLASH | DOLLAR | CR) {
                    break;
                }
                self.pos += 1;
            }
            let text = self.slice(run, self.pos);
            cooked.push_units(text);
            raw.push_units(text);
            match self.peek() {
                None => return Err(error_at(start, UNEXPECTED_END)),
                Some(BACKTICK) => {
                    self.pos += 1;
                    break if head {
                        TokenKind::NoSubstitutionTemplate
                    } else {
                        TokenKind::TemplateTail
                    };
                }
                Some(DOLLAR) if self.is_at(1, b'{') => {
                    self.pos += 2;
                    break if head {
                        TokenKind::TemplateHead
                    } else {
                        TokenKind::TemplateMiddle
                    };
                }
                Some(DOLLAR) => {
                    self.pos += 1;
                    cooked.push(DOLLAR);
                    raw.push(DOLLAR);
                }
                // CR LF and CR become LF in both values (§12.9.6.1).
                Some(CR) => {
                    self.pos += 1;
                    if self.peek() == Some(LF) {
                        self.pos += 1;
                    }
                    cooked.push(LF);
                    raw.push(LF);
                }
                Some(_) => {
                    self.scan_template_escape(start, &mut cooked, &mut raw, &mut cooked_error)?;
                }
            }
        };
        let template = Template {
            cooked: match cooked_error {
                Some(error) => Err(error),
                None => Ok(cooked),
            },
            raw,
        };
        Ok(Lexeme::with_value(
            kind,
            TokenValue::Template(Box::new(template)),
        ))
    }

    /// Scans the escape sequence or line continuation at `\` in a template
    /// and appends it to the cooked and raw values. An invalid escape
    /// sequence stores its error in `cooked_error` and the scan goes on.
    fn scan_template_escape(
        &mut self,
        start: usize,
        cooked: &mut String16,
        raw: &mut String16,
        cooked_error: &mut Option<SyntaxError>,
    ) -> Result<(), SyntaxError> {
        let escape = self.pos;
        if self.ahead(1) == Some(CR) {
            // A LineContinuation: nothing in the cooked value, `\` LF in
            // the raw value.
            self.pos += 2;
            if self.peek() == Some(LF) {
                self.pos += 1;
            }
            raw.push(BACKSLASH);
            raw.push(LF);
            return Ok(());
        }
        if let Err(error) = self.scan_escape(cooked, true) {
            if self.pos >= self.source.len() {
                // An escape that the source ends in reports its own
                // error, except a lone backslash.
                if escape + 1 >= self.source.len() {
                    return Err(error_at(start, UNEXPECTED_END));
                }
                return Err(error);
            }
            // A NotEscapeSequence: the scan continues after the escape
            // character; the following units are ordinary template
            // characters.
            cooked_error.get_or_insert(error);
            self.pos = escape + 2;
        }
        raw.push_units(self.slice(escape, self.pos));
        Ok(())
    }

    /// Scans an escape sequence at `\` and appends its value to `out`
    /// (`EscapeSequence` and `LineContinuation` in strings, §12.9.4;
    /// `TemplateEscapeSequence` in templates, §12.9.6). Returns whether it
    /// was a legacy octal escape or `\8` or `\9` (strings only; in
    /// templates they are errors).
    fn scan_escape(&mut self, out: &mut String16, template: bool) -> Result<Legacy, SyntaxError> {
        let escape = self.pos;
        self.pos += 1;
        let Some(unit) = self.peek() else {
            return Err(error_at(escape, INVALID_TOKEN));
        };
        self.pos += 1;
        let value = match unit {
            // LineContinuation.
            LF | LS | PS => return Ok(Legacy::None),
            CR => {
                if self.peek() == Some(LF) {
                    self.pos += 1;
                }
                return Ok(Legacy::None);
            }
            0x62 => 0x08, // \b
            0x66 => 0x0C, // \f
            0x6E => LF,   // \n
            0x72 => CR,   // \r
            0x74 => 0x09, // \t
            0x76 => 0x0B, // \v
            0x78 => {
                // \x HexDigit HexDigit
                let value = self
                    .scan_hex_digits(2)
                    .ok_or_else(|| error_at(escape, INVALID_HEX_ESCAPE))?;
                out.push(value as u16);
                return Ok(Legacy::None);
            }
            0x75 => {
                // \u
                let cp = self.scan_unicode_escape(escape)?;
                out.push_code_point(cp);
                return Ok(Legacy::None);
            }
            0x30 if !self.peek().is_some_and(is_decimal_digit) => 0, // \0
            0x30..=0x37 => {
                if template {
                    return Err(error_at(escape, OCTAL_IN_TEMPLATE));
                }
                out.push(self.scan_legacy_octal_escape(unit));
                return Ok(Legacy::OctalEscape);
            }
            0x38 | 0x39 => {
                // NonOctalDecimalEscapeSequence (Annex B.1.2).
                if template {
                    return Err(error_at(escape, EIGHT_NINE_IN_TEMPLATE));
                }
                out.push(unit);
                return Ok(Legacy::EightOrNine);
            }
            // NonEscapeCharacter, including the quotes and `\`. A lead
            // surrogate is pushed alone; its trail follows as an ordinary
            // character.
            _ => unit,
        };
        out.push(value);
        Ok(Legacy::None)
    }

    /// The value of a `LegacyOctalEscapeSequence` whose first digit `first`
    /// is consumed (Annex B.1.2): up to three digits if the first is 0 to
    /// 3, up to two otherwise.
    fn scan_legacy_octal_escape(&mut self, first: u16) -> u16 {
        let mut value = first - ascii(b'0');
        let max_digits = if first <= ascii(b'3') { 3 } else { 2 };
        for _ in 1..max_digits {
            match self.peek() {
                Some(unit @ 0x30..=0x37) => {
                    value = value * 8 + (unit - ascii(b'0'));
                    self.pos += 1;
                }
                _ => break,
            }
        }
        value
    }

    /// Scans exactly `count` hexadecimal digits; `None` (and the position
    /// unchanged) if they are not there.
    fn scan_hex_digits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for n in 0..count {
            value = value * 16 + digit_value(self.ahead(n)?, 16)?;
        }
        self.pos += count;
        Some(value)
    }

    /// Scans the rest of a `UnicodeEscapeSequence` after `\u` (§12.9.4):
    /// four hexadecimal digits or `{CodePoint}`. `escape` is the offset of
    /// the `\`, for errors.
    pub(super) fn scan_unicode_escape(&mut self, escape: usize) -> Result<u32, SyntaxError> {
        if !self.is_at(0, b'{') {
            return self
                .scan_hex_digits(4)
                .ok_or_else(|| error_at(escape, INVALID_UNICODE_ESCAPE));
        }
        self.pos += 1;
        let mut value: u32 = 0;
        let mut count = 0;
        while let Some(digit) = self.peek().and_then(|unit| digit_value(unit, 16)) {
            // Saturates above the largest code point; leading zeros are
            // allowed in any number.
            value = ((value << 4) | digit).min(0x11_0000);
            count += 1;
            self.pos += 1;
        }
        if value > 0x10_FFFF {
            return Err(error_at(escape, UNDEFINED_CODE_POINT));
        }
        if count == 0 || !self.is_at(0, b'}') {
            return Err(error_at(escape, INVALID_UNICODE_ESCAPE));
        }
        self.pos += 1;
        Ok(value)
    }
}
