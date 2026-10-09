//! The lexer: ECMA-262 2025 clause 12 with Annex B.1
//! (<https://tc39.es/ecma262/2025/#sec-ecmascript-language-lexical-grammar>).
//!
//! The parser asks for one token at a time and passes the goal symbol of
//! its context ([`Goal`]); there is no token array (ADR 0026 section 3).
//! The lexer can restart at any offset ([`Lexer::restore`],
//! [`Lexer::rescan`]), so the parser can re-scan a token with another goal.
//!
//! The lexer is generic over the code-unit width of the source
//! ([`CodeUnit`]). Positions are `u32` code-unit offsets. It never
//! recurses, and every loop advances through the source, so the time is
//! linear in the source length.

mod number;
mod regexp;
mod string;
#[cfg(test)]
mod tests;

use swb_js_text::{
    CodeUnit, Str16, combine_surrogates, is_lead_surrogate, is_trail_surrogate, unicode,
};

use crate::SyntaxError;
use crate::interner::Interner;
use crate::token::{Goal, Legacy, Token, TokenKind, TokenValue, keyword_kind};

/// The largest source length in code units: offsets are `u32`, and a
/// source has fewer than 2^31 code units (ADR 0026 section 3).
pub const MAX_SOURCE_LEN: usize = (1 << 31) - 1;

const LF: u16 = 0x0A;
const CR: u16 = 0x0D;
const LS: u16 = 0x2028;
const PS: u16 = 0x2029;

/// Chromium's message for most lexical errors.
pub(crate) const INVALID_TOKEN: &str = "Invalid or unexpected token";

/// The code unit of an ASCII character.
const fn ascii(c: u8) -> u16 {
    c as u16
}

/// Whether an ASCII code unit is `IdentifierStartChar`.
fn is_ascii_id_start(unit: u16) -> bool {
    matches!(unit, 0x61..=0x7A | 0x41..=0x5A | 0x24 | 0x5F)
}

/// Whether an ASCII code unit is `IdentifierPartChar`.
fn is_ascii_id_part(unit: u16) -> bool {
    matches!(unit, 0x61..=0x7A | 0x41..=0x5A | 0x30..=0x39 | 0x24 | 0x5F)
}

/// Whether a code unit is a decimal digit.
fn is_decimal_digit(unit: u16) -> bool {
    (0x30..=0x39).contains(&unit)
}

/// Options of the lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexerOptions {
    /// Whether `<!--` and `-->` start comments (Annex B.1.1). True for
    /// scripts, false for modules.
    pub html_comments: bool,
}

impl LexerOptions {
    /// The options for a script.
    pub const fn script() -> Self {
        LexerOptions {
            html_comments: true,
        }
    }

    /// The options for a module.
    pub const fn module() -> Self {
        LexerOptions {
            html_comments: false,
        }
    }
}

impl Default for LexerOptions {
    fn default() -> Self {
        Self::script()
    }
}

/// A position of the lexer between two tokens, to restart from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// The code-unit offset.
    pub offset: u32,
    /// Whether a line terminator was seen since the last token.
    pub newline_before: bool,
}

/// What a scan function returns: the token without its position.
struct Lexeme {
    kind: TokenKind,
    value: TokenValue,
    escaped: bool,
    legacy: Legacy,
}

impl Lexeme {
    fn new(kind: TokenKind) -> Self {
        Lexeme {
            kind,
            value: TokenValue::None,
            escaped: false,
            legacy: Legacy::None,
        }
    }

    fn with_value(kind: TokenKind, value: TokenValue) -> Self {
        Lexeme {
            value,
            ..Lexeme::new(kind)
        }
    }
}

/// The lexer over a source of code units of width `U`.
pub struct Lexer<'a, U: CodeUnit> {
    source: &'a [U],
    pos: usize,
    /// Whether a line terminator was seen since the last token.
    newline_before: bool,
    options: LexerOptions,
    interner: Interner,
    /// The code units of an identifier that has escapes or non-ASCII
    /// characters.
    name_buf: Vec<u16>,
    /// The digits of a decimal literal, without separators.
    digits: String,
}

impl<'a, U: CodeUnit> Lexer<'a, U> {
    /// A lexer at the start of `source`. Fails if the source is longer
    /// than [`MAX_SOURCE_LEN`].
    pub fn new(
        source: &'a [U],
        options: LexerOptions,
        interner: Interner,
    ) -> Result<Self, SyntaxError> {
        if source.len() > MAX_SOURCE_LEN {
            return Err(SyntaxError::new(0, "Script is too large"));
        }
        Ok(Lexer {
            source,
            pos: 0,
            newline_before: false,
            options,
            interner,
            name_buf: Vec::new(),
            digits: String::new(),
        })
    }

    /// The source.
    pub fn source(&self) -> &'a [U] {
        self.source
    }

    /// The source text `start..end`, or `None` if the range is not inside
    /// the source.
    pub fn text(&self, start: u32, end: u32) -> Option<Str16<'a>> {
        U::view(self.source).slice(start as usize, end as usize)
    }

    /// The name table.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// The name table, for the parser to add names.
    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    /// Ends the lexer and returns its name table.
    pub fn into_interner(self) -> Interner {
        self.interner
    }

    /// The current position: after the last token, before the white space
    /// and comments that follow it.
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            offset: self.pos as u32,
            newline_before: self.newline_before,
        }
    }

    /// Continues from a position. An offset past the end means the end.
    pub fn restore(&mut self, checkpoint: Checkpoint) {
        self.pos = (checkpoint.offset as usize).min(self.source.len());
        self.newline_before = checkpoint.newline_before;
    }

    /// Scans `token` again with another goal symbol (for example `/` as a
    /// regular expression literal instead of a division), and continues
    /// after the new token.
    pub fn rescan(&mut self, token: &Token, goal: Goal) -> Result<Token, SyntaxError> {
        self.restore(Checkpoint {
            offset: token.start,
            newline_before: token.newline_before,
        });
        self.next_token(goal)
    }

    /// The next token for the goal symbol `goal`. At the end of the source
    /// it returns [`TokenKind::Eof`] (again on each call).
    pub fn next_token(&mut self, goal: Goal) -> Result<Token, SyntaxError> {
        self.skip_trivia(goal)?;
        let start = self.pos;
        let newline_before = std::mem::take(&mut self.newline_before);
        let lexeme = match self.peek() {
            None => Lexeme::new(TokenKind::Eof),
            Some(unit) => self.scan_token(start, unit, goal)?,
        };
        Ok(Token {
            kind: lexeme.kind,
            start: start as u32,
            end: self.pos as u32,
            newline_before,
            escaped: lexeme.escaped,
            legacy: lexeme.legacy,
            value: lexeme.value,
        })
    }

    fn scan_token(&mut self, start: usize, unit: u16, goal: Goal) -> Result<Lexeme, SyntaxError> {
        if unit >= 0x80 {
            return match self.code_point_at(start) {
                Some((cp, _)) if unicode::is_identifier_start(cp) => {
                    self.scan_identifier_slow(start, true)
                }
                _ => Err(error_at(start, INVALID_TOKEN)),
            };
        }
        match unit as u8 {
            b'a'..=b'z' | b'A'..=b'Z' | b'$' | b'_' => self.scan_identifier(start, true),
            b'0'..=b'9' => self.scan_number(start),
            b'.' if self.ahead(1).is_some_and(is_decimal_digit) => self.scan_number(start),
            b'"' | b'\'' => self.scan_string(start),
            b'`' => self.scan_template(start, true),
            b'}' if goal.allows_template_tail() => self.scan_template(start, false),
            b'/' if goal.allows_regexp() => self.scan_regexp(start),
            b'#' => self.scan_private_name(start),
            b'\\' => self.scan_identifier_slow(start, true),
            _ => self.scan_punctuator(start, unit),
        }
    }

    // --- Reading the source ---

    /// The code unit at the current position.
    #[inline]
    fn peek(&self) -> Option<u16> {
        self.source.get(self.pos).map(|u| u.value())
    }

    /// The code unit `n` units after the current position.
    #[inline]
    fn ahead(&self, n: usize) -> Option<u16> {
        self.source.get(self.pos + n).map(|u| u.value())
    }

    /// Whether the unit `n` after the current position is the ASCII `c`.
    #[inline]
    fn is_at(&self, n: usize, c: u8) -> bool {
        self.ahead(n) == Some(ascii(c))
    }

    /// The code point at `pos` (a surrogate pair is decoded, an unpaired
    /// surrogate is returned as it is) and its length in code units.
    fn code_point_at(&self, pos: usize) -> Option<(u32, usize)> {
        let unit = self.source.get(pos)?.value();
        if is_lead_surrogate(unit)
            && let Some(trail) = self.source.get(pos + 1).map(|u| u.value())
            && is_trail_surrogate(trail)
        {
            return Some((combine_surrogates(unit, trail), 2));
        }
        Some((u32::from(unit), 1))
    }

    /// The units `start..end` (empty if the range is not inside the
    /// source).
    fn slice(&self, start: usize, end: usize) -> &'a [U] {
        self.source.get(start..end).unwrap_or(&[])
    }

    // --- White space and comments ---

    /// Skips white space, line terminators and comments (§12.2 to §12.4,
    /// Annex B.1.1), and records line terminators.
    fn skip_trivia(&mut self, goal: Goal) -> Result<(), SyntaxError> {
        // Only trivia comes before this token: it is the first one.
        let input_start = self.pos == 0;
        if input_start
            && goal == Goal::HashbangOrRegExp
            && self.is_at(0, b'#')
            && self.is_at(1, b'!')
        {
            // HashbangComment (§12.5).
            self.skip_line_comment();
        }
        let html = self.options.html_comments;
        while let Some(unit) = self.peek() {
            match unit {
                0x09 | 0x0B | 0x0C | 0x20 => self.pos += 1,
                LF | CR => {
                    self.pos += 1;
                    self.newline_before = true;
                }
                0x2F if self.is_at(1, b'/') => self.skip_line_comment(),
                0x2F if self.is_at(1, b'*') => self.skip_block_comment()?,
                // SingleLineHTMLOpenComment: `<!--`.
                0x3C if html
                    && self.is_at(1, b'!')
                    && self.is_at(2, b'-')
                    && self.is_at(3, b'-') =>
                {
                    self.skip_line_comment();
                }
                // HTMLCloseComment: `-->` at the start of a line or of the
                // input, after white space and comments only.
                0x2D if html
                    && (self.newline_before || input_start)
                    && self.is_at(1, b'-')
                    && self.is_at(2, b'>') =>
                {
                    self.skip_line_comment();
                }
                LS | PS => {
                    self.pos += 1;
                    self.newline_before = true;
                }
                0x80.. if unicode::is_whitespace(u32::from(unit)) => self.pos += 1,
                _ => break,
            }
        }
        Ok(())
    }

    /// Skips to the next line terminator (not including it).
    fn skip_line_comment(&mut self) {
        while let Some(unit) = self.peek() {
            if matches!(unit, LF | CR | LS | PS) {
                break;
            }
            self.pos += 1;
        }
    }

    /// Skips a `/* */` comment; one that contains a line terminator counts
    /// as a line terminator (§12.4).
    fn skip_block_comment(&mut self) -> Result<(), SyntaxError> {
        let start = self.pos;
        self.pos += 2;
        while let Some(unit) = self.peek() {
            self.pos += 1;
            match unit {
                0x2A if self.peek() == Some(ascii(b'/')) => {
                    self.pos += 1;
                    return Ok(());
                }
                LF | CR | LS | PS => self.newline_before = true,
                _ => {}
            }
        }
        Err(error_at(start, INVALID_TOKEN))
    }

    // --- Identifiers (§12.7) ---

    /// Scans an `IdentifierName` that starts with an ASCII character. A
    /// reserved word becomes its keyword kind if `keywords` is set.
    fn scan_identifier(&mut self, start: usize, keywords: bool) -> Result<Lexeme, SyntaxError> {
        let mut end = start;
        while let Some(unit) = self.source.get(end).map(|u| u.value()) {
            if unit < 0x80 && is_ascii_id_part(unit) {
                end += 1;
            } else {
                if unit >= 0x80 || unit == ascii(b'\\') {
                    return self.scan_identifier_slow(start, keywords);
                }
                break;
            }
        }
        self.pos = end;
        let text = self.slice(start, end);
        if keywords && let Some(kind) = keyword_of(text) {
            return Ok(Lexeme::new(kind));
        }
        let name = self.interner.intern(U::view(text));
        Ok(Lexeme::with_value(
            TokenKind::Identifier,
            TokenValue::Name(name),
        ))
    }

    /// Scans an `IdentifierName` that can contain escapes and non-ASCII
    /// characters. Reserved words are recognized only without escapes
    /// (§12.7.2); an escaped one is an identifier with `escaped` set, which
    /// the parser rejects where a reserved word is not allowed.
    fn scan_identifier_slow(
        &mut self,
        start: usize,
        keywords: bool,
    ) -> Result<Lexeme, SyntaxError> {
        self.pos = start;
        self.name_buf.clear();
        let mut escaped = false;
        loop {
            let here = self.pos;
            let first = here == start;
            let cp = if self.peek() == Some(ascii(b'\\')) {
                if !self.is_at(1, b'u') {
                    return Err(error_at(here, INVALID_TOKEN));
                }
                self.pos += 2;
                let cp = self.scan_unicode_escape(here)?;
                let valid = if first {
                    unicode::is_identifier_start(cp)
                } else {
                    unicode::is_identifier_part(cp)
                };
                if !valid {
                    return Err(error_at(here, INVALID_TOKEN));
                }
                escaped = true;
                cp
            } else {
                let Some((cp, len)) = self.code_point_at(here) else {
                    break;
                };
                let valid = if first {
                    unicode::is_identifier_start(cp)
                } else {
                    unicode::is_identifier_part(cp)
                };
                if !valid {
                    break;
                }
                self.pos += len;
                cp
            };
            push_code_point(&mut self.name_buf, cp);
        }
        if self.pos == start {
            return Err(error_at(start, INVALID_TOKEN));
        }
        if keywords
            && !escaped
            && let Some(kind) = keyword_of(self.slice(start, self.pos))
        {
            return Ok(Lexeme::new(kind));
        }
        let name = self.interner.intern(Str16::Wide(&self.name_buf));
        Ok(Lexeme {
            escaped,
            ..Lexeme::with_value(TokenKind::Identifier, TokenValue::Name(name))
        })
    }

    /// Scans a `PrivateIdentifier`: `#` `IdentifierName` (§12.7).
    fn scan_private_name(&mut self, start: usize) -> Result<Lexeme, SyntaxError> {
        let name_start = start + 1;
        let lexeme = match self.source.get(name_start).map(|u| u.value()) {
            Some(unit) if unit < 0x80 && is_ascii_id_start(unit) => {
                self.scan_identifier(name_start, false)?
            }
            Some(unit) if unit >= 0x80 || unit == ascii(b'\\') => {
                self.scan_identifier_slow(name_start, false)?
            }
            _ => return Err(error_at(start, INVALID_TOKEN)),
        };
        Ok(Lexeme {
            kind: TokenKind::PrivateName,
            ..lexeme
        })
    }

    // --- Punctuators (§12.8) ---

    /// Scans a punctuator that starts with the ASCII unit `unit`.
    fn scan_punctuator(&mut self, start: usize, unit: u16) -> Result<Lexeme, SyntaxError> {
        use TokenKind as K;
        let at = |n: usize, c: u8| self.is_at(n, c);
        let (kind, len) = match unit as u8 {
            b'{' => (K::LBrace, 1),
            b'}' => (K::RBrace, 1),
            b'(' => (K::LParen, 1),
            b')' => (K::RParen, 1),
            b'[' => (K::LBracket, 1),
            b']' => (K::RBracket, 1),
            b';' => (K::Semicolon, 1),
            b',' => (K::Comma, 1),
            b':' => (K::Colon, 1),
            b'~' => (K::Tilde, 1),
            b'.' if at(1, b'.') && at(2, b'.') => (K::Ellipsis, 3),
            b'.' => (K::Dot, 1),
            b'<' if at(1, b'<') && at(2, b'=') => (K::ShlEq, 3),
            b'<' if at(1, b'<') => (K::Shl, 2),
            b'<' if at(1, b'=') => (K::LtEq, 2),
            b'<' => (K::Lt, 1),
            b'>' if at(1, b'>') && at(2, b'>') && at(3, b'=') => (K::UShrEq, 4),
            b'>' if at(1, b'>') && at(2, b'>') => (K::UShr, 3),
            b'>' if at(1, b'>') && at(2, b'=') => (K::ShrEq, 3),
            b'>' if at(1, b'>') => (K::Shr, 2),
            b'>' if at(1, b'=') => (K::GtEq, 2),
            b'>' => (K::Gt, 1),
            b'=' if at(1, b'=') && at(2, b'=') => (K::EqEqEq, 3),
            b'=' if at(1, b'=') => (K::EqEq, 2),
            b'=' if at(1, b'>') => (K::Arrow, 2),
            b'=' => (K::Eq, 1),
            b'!' if at(1, b'=') && at(2, b'=') => (K::NotEqEq, 3),
            b'!' if at(1, b'=') => (K::NotEq, 2),
            b'!' => (K::Bang, 1),
            b'+' if at(1, b'+') => (K::PlusPlus, 2),
            b'+' if at(1, b'=') => (K::PlusEq, 2),
            b'+' => (K::Plus, 1),
            b'-' if at(1, b'-') => (K::MinusMinus, 2),
            b'-' if at(1, b'=') => (K::MinusEq, 2),
            b'-' => (K::Minus, 1),
            b'*' if at(1, b'*') && at(2, b'=') => (K::StarStarEq, 3),
            b'*' if at(1, b'*') => (K::StarStar, 2),
            b'*' if at(1, b'=') => (K::StarEq, 2),
            b'*' => (K::Star, 1),
            b'/' if at(1, b'=') => (K::SlashEq, 2),
            b'/' => (K::Slash, 1),
            b'%' if at(1, b'=') => (K::PercentEq, 2),
            b'%' => (K::Percent, 1),
            b'&' if at(1, b'&') && at(2, b'=') => (K::AmpAmpEq, 3),
            b'&' if at(1, b'&') => (K::AmpAmp, 2),
            b'&' if at(1, b'=') => (K::AmpEq, 2),
            b'&' => (K::Amp, 1),
            b'|' if at(1, b'|') && at(2, b'=') => (K::PipePipeEq, 3),
            b'|' if at(1, b'|') => (K::PipePipe, 2),
            b'|' if at(1, b'=') => (K::PipeEq, 2),
            b'|' => (K::Pipe, 1),
            b'^' if at(1, b'=') => (K::CaretEq, 2),
            b'^' => (K::Caret, 1),
            b'?' if at(1, b'?') && at(2, b'=') => (K::QuestionQuestionEq, 3),
            b'?' if at(1, b'?') => (K::QuestionQuestion, 2),
            // OptionalChainingPunctuator: `?.` [lookahead ∉ DecimalDigit].
            b'?' if at(1, b'.') && !self.ahead(2).is_some_and(is_decimal_digit) => {
                (K::QuestionDot, 2)
            }
            b'?' => (K::Question, 1),
            _ => return Err(error_at(start, INVALID_TOKEN)),
        };
        self.pos = start + len;
        Ok(Lexeme::new(kind))
    }
}

/// Appends a code point to UTF-16 units.
fn push_code_point(units: &mut Vec<u16>, cp: u32) {
    if let Ok(unit) = u16::try_from(cp) {
        units.push(unit);
    } else {
        let offset = cp.saturating_sub(0x10000);
        units.push(0xD800 + ((offset >> 10) & 0x3FF) as u16);
        units.push(0xDC00 + (offset & 0x3FF) as u16);
    }
}

/// A syntax error at a code-unit offset.
fn error_at(offset: usize, message: &'static str) -> SyntaxError {
    SyntaxError::new(offset as u32, message)
}

/// The reserved word spelled by `text`, or `None`.
fn keyword_of<U: CodeUnit>(text: &[U]) -> Option<TokenKind> {
    let mut bytes = [0u8; 10];
    if text.len() > bytes.len() {
        return None;
    }
    for (byte, unit) in bytes.iter_mut().zip(text) {
        *byte = u8::try_from(unit.value()).ok()?;
    }
    keyword_kind(bytes.get(..text.len())?)
}
