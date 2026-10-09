//! Regular expression literals (§12.9.5). The lexer finds the end of the
//! body and the flags; `swb-js-regexp` checks them (§13.2.7.2,
//! `IsValidRegularExpressionLiteral`).

use swb_js_text::{CodeUnit, unicode};

use super::{CR, LF, LS, Lexeme, Lexer, PS, error_at};
use crate::SyntaxError;
use crate::token::{TokenKind, TokenValue};

const UNTERMINATED: &str = "Invalid regular expression: missing /";

impl<U: CodeUnit> Lexer<'_, U> {
    /// Scans a `RegularExpressionLiteral` that starts with `/` at `start`.
    /// White space and comments are skipped before, so the first body
    /// character is not `*` or `/`.
    pub(super) fn scan_regexp(&mut self, start: usize) -> Result<Lexeme, SyntaxError> {
        self.pos = start + 1;
        let mut in_class = false;
        let body_end = loop {
            let Some(unit) = self.peek() else {
                return Err(error_at(start, UNTERMINATED));
            };
            match unit {
                LF | CR | LS | PS => return Err(error_at(start, UNTERMINATED)),
                // RegularExpressionBackslashSequence: `\` and a character
                // that is not a line terminator.
                0x5C => match self.ahead(1) {
                    None | Some(LF | CR | LS | PS) => {
                        return Err(error_at(start, UNTERMINATED));
                    }
                    Some(_) => self.pos += 1,
                },
                0x5B => in_class = true,
                0x5D => in_class = false,
                0x2F if !in_class => break self.pos,
                _ => {}
            }
            self.pos += 1;
        };
        self.pos += 1;
        let flags_start = self.pos;
        // RegularExpressionFlags: IdentifierPartChar, without escapes.
        while let Some((cp, len)) = self.code_point_at(self.pos) {
            if !unicode::is_identifier_part(cp) {
                break;
            }
            self.pos += len;
        }
        let pattern = U::view(self.slice(start + 1, body_end));
        let flags_text = U::view(self.slice(flags_start, self.pos));
        let flags = swb_js_regexp::check_literal(pattern, flags_text)
            .map_err(|error| SyntaxError::new(start as u32, error.to_string()))?;
        if self.peek() == Some(0x5C) {
            // An escape after the flags would start an identifier, which
            // can never follow a regular expression literal directly.
            // Chromium reports it as a flag error.
            return Err(SyntaxError::new(
                start as u32,
                swb_js_regexp::Error::InvalidFlags.to_string(),
            ));
        }
        Ok(Lexeme::with_value(
            TokenKind::RegExp,
            TokenValue::RegExp {
                body_end: body_end as u32,
                flags,
            },
        ))
    }
}
