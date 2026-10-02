//! CSS tokenizer.
//!
//! Implements the tokenization algorithm of CSS Syntax Level 3:
//! <https://www.w3.org/TR/css-syntax-3/#tokenization>.
//!
//! The tokenizer works on bytes and slices the input where it can, so that a
//! token without escapes costs at most one allocation. The CSS tokenizer has
//! no state between tokens, so the parser can restart it at any token
//! boundary (see [`Tokenizer::set_position`]).
//!
//! Deviation: every code point at or above U+0080 is an ident code point, as
//! in the 2021 Candidate Recommendation and in browsers. The current Editor's
//! Draft uses a smaller set.

use std::borrow::Cow;

/// The numeric part of a number, percentage or dimension token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number {
    /// The numeric value. Values outside the `f32` range are clamped to the
    /// largest finite value.
    pub value: f32,
    /// The value as an integer when the token has the "integer" type flag
    /// (the source has no fraction and no exponent). `None` for the "number"
    /// type flag. Values outside the `i32` range are clamped.
    pub int_value: Option<i32>,
    /// True if the source text starts with `+` or `-`.
    pub has_sign: bool,
}

impl Number {
    /// True if the token has the "integer" type flag.
    pub fn is_integer(&self) -> bool {
        self.int_value.is_some()
    }
}

/// A CSS token, as produced by the tokenizer.
///
/// See <https://www.w3.org/TR/css-syntax-3/#tokenization>. Comments are not
/// tokens; the tokenizer drops them.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// `<ident-token>`.
    Ident(Box<str>),
    /// `<function-token>`: the name, without the `(`.
    Function(Box<str>),
    /// `<at-keyword-token>`: the name, without the `@`.
    AtKeyword(Box<str>),
    /// `<hash-token>`: the value without the `#`. `is_id` is the "id" type
    /// flag: the value is a valid identifier.
    Hash {
        /// The value without the `#`.
        value: Box<str>,
        /// The "id" type flag.
        is_id: bool,
    },
    /// `<string-token>`: the value without quotes, escapes resolved.
    String(Box<str>),
    /// `<bad-string-token>`: a string with an unescaped newline.
    BadString,
    /// `<url-token>`: the URL of an unquoted `url(...)`.
    Url(Box<str>),
    /// `<bad-url-token>`.
    BadUrl,
    /// `<delim-token>`.
    Delim(char),
    /// `<number-token>`.
    Number(Number),
    /// `<percentage-token>`. The value is the number before `%` (50 for
    /// `50%`).
    Percentage(Number),
    /// `<dimension-token>`.
    Dimension {
        /// The number before the unit.
        number: Number,
        /// The unit, as written (match it ASCII case-insensitively).
        unit: Box<str>,
    },
    /// `<whitespace-token>` (one token for a run of whitespace).
    Whitespace,
    /// `<CDO-token>`: `<!--`.
    Cdo,
    /// `<CDC-token>`: `-->`.
    Cdc,
    /// `<colon-token>`.
    Colon,
    /// `<semicolon-token>`.
    Semicolon,
    /// `<comma-token>`.
    Comma,
    /// `<[-token>`.
    OpenSquare,
    /// `<]-token>`.
    CloseSquare,
    /// `<(-token>`.
    OpenParen,
    /// `<)-token>`.
    CloseParen,
    /// `<{-token>`.
    OpenCurly,
    /// `<}-token>`.
    CloseCurly,
}

/// Preprocesses CSS source text.
///
/// Replaces CR LF, CR and FF with LF, and NUL with U+FFFD. Returns the input
/// unchanged (without a copy) when it contains none of these.
///
/// <https://www.w3.org/TR/css-syntax-3/#input-preprocessing>
pub fn preprocess(input: &str) -> Cow<'_, str> {
    if !input.bytes().any(|b| matches!(b, b'\r' | b'\x0c' | b'\0')) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\x0c' => out.push('\n'),
            '\0' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Splits CSS source text into tokens.
///
/// This is a convenience function for tests and debugging. The parser
/// tokenizes on demand and does not build this list.
pub fn tokenize(css: &str) -> Vec<Token> {
    let input = preprocess(css);
    let mut tokenizer = Tokenizer::new(&input);
    let mut tokens = Vec::new();
    while let Some(token) = tokenizer.next_token() {
        tokens.push(token);
    }
    tokens
}

/// Produces tokens from preprocessed CSS text.
pub(crate) struct Tokenizer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

const REPLACEMENT: char = '\u{FFFD}';

fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n')
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_ident_byte(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit() || b == b'-'
}

/// <https://www.w3.org/TR/css-syntax-3/#non-printable-code-point>
fn is_non_printable(b: u8) -> bool {
    matches!(b, 0x00..=0x08 | 0x0b | 0x0e..=0x1f | 0x7f)
}

impl<'a> Tokenizer<'a> {
    /// Creates a tokenizer. `input` must already be preprocessed (see
    /// [`preprocess`]).
    pub(crate) fn new(input: &'a str) -> Self {
        Tokenizer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    /// The preprocessed input text.
    pub(crate) fn input(&self) -> &'a str {
        self.input
    }

    /// The byte offset of the next token (or of a comment before it).
    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    /// Restarts tokenization at a byte offset that an earlier call to
    /// [`Tokenizer::position`] returned.
    pub(crate) fn set_position(&mut self, pos: usize) {
        self.pos = pos.min(self.bytes.len());
    }

    fn byte(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn is_digit_at(&self, offset: usize) -> bool {
        self.byte(offset).is_some_and(|b| b.is_ascii_digit())
    }

    /// <https://www.w3.org/TR/css-syntax-3/#starts-with-a-valid-escape>
    fn is_valid_escape_at(&self, offset: usize) -> bool {
        self.byte(offset) == Some(b'\\') && self.byte(offset + 1) != Some(b'\n')
    }

    /// <https://www.w3.org/TR/css-syntax-3/#would-start-an-identifier>
    fn starts_ident_sequence_at(&self, offset: usize) -> bool {
        match self.byte(offset) {
            Some(b'-') => match self.byte(offset + 1) {
                Some(b) if is_ident_start(b) || b == b'-' => true,
                _ => self.is_valid_escape_at(offset + 1),
            },
            Some(b'\\') => self.is_valid_escape_at(offset),
            Some(b) => is_ident_start(b),
            None => false,
        }
    }

    /// <https://www.w3.org/TR/css-syntax-3/#starts-with-a-number>
    fn starts_number_at(&self, offset: usize) -> bool {
        match self.byte(offset) {
            Some(b'+' | b'-') => {
                self.is_digit_at(offset + 1)
                    || (self.byte(offset + 1) == Some(b'.') && self.is_digit_at(offset + 2))
            }
            Some(b'.') => self.is_digit_at(offset + 1),
            Some(b) => b.is_ascii_digit(),
            None => false,
        }
    }

    /// Returns the next token, or `None` at the end of the input.
    ///
    /// <https://www.w3.org/TR/css-syntax-3/#consume-token>
    pub(crate) fn next_token(&mut self) -> Option<Token> {
        self.consume_comments();
        let b = self.byte(0)?;
        let token = match b {
            b' ' | b'\t' | b'\n' => {
                self.consume_whitespace();
                Token::Whitespace
            }
            b'"' | b'\'' => {
                self.pos += 1;
                self.consume_string(b)
            }
            b'#' => {
                if self.byte(1).is_some_and(is_ident_byte) || self.is_valid_escape_at(1) {
                    self.pos += 1;
                    let is_id = self.starts_ident_sequence_at(0);
                    let value = self.consume_ident_sequence();
                    Token::Hash { value, is_id }
                } else {
                    self.delim()
                }
            }
            b'(' => self.single(Token::OpenParen),
            b')' => self.single(Token::CloseParen),
            b'[' => self.single(Token::OpenSquare),
            b']' => self.single(Token::CloseSquare),
            b'{' => self.single(Token::OpenCurly),
            b'}' => self.single(Token::CloseCurly),
            b',' => self.single(Token::Comma),
            b':' => self.single(Token::Colon),
            b';' => self.single(Token::Semicolon),
            b'+' | b'.' => {
                if self.starts_number_at(0) {
                    self.consume_numeric()
                } else {
                    self.delim()
                }
            }
            b'-' => {
                if self.starts_number_at(0) {
                    self.consume_numeric()
                } else if self.byte(1) == Some(b'-') && self.byte(2) == Some(b'>') {
                    self.pos += 3;
                    Token::Cdc
                } else if self.starts_ident_sequence_at(0) {
                    self.consume_ident_like()
                } else {
                    self.delim()
                }
            }
            b'<' => {
                if self.bytes.get(self.pos + 1..self.pos + 4) == Some(b"!--") {
                    self.pos += 4;
                    Token::Cdo
                } else {
                    self.delim()
                }
            }
            b'@' => {
                if self.starts_ident_sequence_at(1) {
                    self.pos += 1;
                    Token::AtKeyword(self.consume_ident_sequence())
                } else {
                    self.delim()
                }
            }
            b'\\' => {
                if self.is_valid_escape_at(0) {
                    self.consume_ident_like()
                } else {
                    // Parse error: a backslash before a newline.
                    self.delim()
                }
            }
            b'0'..=b'9' => self.consume_numeric(),
            b if is_ident_start(b) => self.consume_ident_like(),
            _ => self.delim(),
        };
        Some(token)
    }

    fn single(&mut self, token: Token) -> Token {
        self.pos += 1;
        token
    }

    /// Consumes one code point and returns it as a delim token.
    fn delim(&mut self) -> Token {
        let c = self.current_char();
        self.pos += c.len_utf8();
        Token::Delim(c)
    }

    /// The code point at the current position, or U+FFFD if the position is
    /// not at a code point boundary or at the end.
    fn current_char(&self) -> char {
        self.input
            .get(self.pos..)
            .and_then(|s| s.chars().next())
            .unwrap_or(REPLACEMENT)
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-comment>
    fn consume_comments(&mut self) {
        while self.byte(0) == Some(b'/') && self.byte(1) == Some(b'*') {
            let body_start = self.pos + 2;
            match self.input.get(body_start..).and_then(|s| s.find("*/")) {
                Some(end) => self.pos = body_start + end + 2,
                None => self.pos = self.bytes.len(),
            }
        }
    }

    fn consume_whitespace(&mut self) {
        while self.byte(0).is_some_and(is_whitespace) {
            self.pos += 1;
        }
    }

    /// Consumes an escaped code point. The backslash is already consumed.
    ///
    /// <https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point>
    fn consume_escaped_code_point(&mut self) -> char {
        let Some(b) = self.byte(0) else {
            return REPLACEMENT;
        };
        if !b.is_ascii_hexdigit() {
            let c = self.current_char();
            self.pos += c.len_utf8();
            return c;
        }
        let mut value: u32 = 0;
        let mut digits = 0;
        while digits < 6 {
            match self.byte(0).and_then(|b| (b as char).to_digit(16)) {
                Some(d) => {
                    value = value * 16 + d;
                    self.pos += 1;
                    digits += 1;
                }
                None => break,
            }
        }
        if self.byte(0).is_some_and(is_whitespace) {
            self.pos += 1;
        }
        if value == 0 {
            return REPLACEMENT;
        }
        // `from_u32` rejects surrogates and values above U+10FFFF.
        char::from_u32(value).unwrap_or(REPLACEMENT)
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-name>
    fn consume_ident_sequence(&mut self) -> Box<str> {
        let mut escaped: Option<String> = None;
        let mut run_start = self.pos;
        loop {
            match self.byte(0) {
                Some(b) if is_ident_byte(b) => self.pos += 1,
                Some(b'\\') if self.is_valid_escape_at(0) => {
                    let out = escaped.get_or_insert_with(String::new);
                    out.push_str(&self.input[run_start..self.pos]);
                    self.pos += 1;
                    out.push(self.consume_escaped_code_point());
                    run_start = self.pos;
                }
                _ => break,
            }
        }
        finish_run(escaped, &self.input[run_start..self.pos])
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-ident-like-token>
    fn consume_ident_like(&mut self) -> Token {
        let name = self.consume_ident_sequence();
        if self.byte(0) != Some(b'(') {
            return Token::Ident(name);
        }
        self.pos += 1;
        if !name.eq_ignore_ascii_case("url") {
            return Token::Function(name);
        }
        let ws_start = self.pos;
        self.consume_whitespace();
        if matches!(self.byte(0), Some(b'"' | b'\'')) {
            // Keep one whitespace code point; it becomes a whitespace token
            // inside the function.
            if self.pos > ws_start {
                self.pos -= 1;
            }
            return Token::Function(name);
        }
        self.consume_url()
    }

    /// Consumes an unquoted URL. `url(` and leading whitespace are already
    /// consumed.
    ///
    /// <https://www.w3.org/TR/css-syntax-3/#consume-url-token>
    fn consume_url(&mut self) -> Token {
        let mut value: Option<String> = None;
        let mut run_start = self.pos;
        loop {
            let Some(b) = self.byte(0) else {
                // Parse error: EOF in URL.
                return Token::Url(finish_run(value, &self.input[run_start..self.pos]));
            };
            match b {
                b')' => {
                    let url = finish_run(value, &self.input[run_start..self.pos]);
                    self.pos += 1;
                    return Token::Url(url);
                }
                b if is_whitespace(b) => {
                    let url = finish_run(value, &self.input[run_start..self.pos]);
                    self.consume_whitespace();
                    match self.byte(0) {
                        None => return Token::Url(url),
                        Some(b')') => {
                            self.pos += 1;
                            return Token::Url(url);
                        }
                        Some(_) => {
                            self.consume_bad_url_remnants();
                            return Token::BadUrl;
                        }
                    }
                }
                b'"' | b'\'' | b'(' => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                b if is_non_printable(b) => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                b'\\' => {
                    if !self.is_valid_escape_at(0) {
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                    let out = value.get_or_insert_with(String::new);
                    out.push_str(&self.input[run_start..self.pos]);
                    self.pos += 1;
                    out.push(self.consume_escaped_code_point());
                    run_start = self.pos;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-remnants-of-bad-url>
    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.byte(0) {
                None => return,
                Some(b')') => {
                    self.pos += 1;
                    return;
                }
                Some(b'\\') if self.is_valid_escape_at(0) => {
                    self.pos += 1;
                    self.consume_escaped_code_point();
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Consumes a string. The opening quote is already consumed.
    ///
    /// <https://www.w3.org/TR/css-syntax-3/#consume-string-token>
    fn consume_string(&mut self, quote: u8) -> Token {
        let mut value: Option<String> = None;
        let mut run_start = self.pos;
        loop {
            match self.byte(0) {
                None => {
                    // Parse error: EOF in string.
                    return Token::String(finish_run(value, &self.input[run_start..self.pos]));
                }
                Some(b) if b == quote => {
                    let s = finish_run(value, &self.input[run_start..self.pos]);
                    self.pos += 1;
                    return Token::String(s);
                }
                Some(b'\n') => {
                    // Parse error: the newline is not consumed.
                    return Token::BadString;
                }
                Some(b'\\') => {
                    let out = value.get_or_insert_with(String::new);
                    out.push_str(&self.input[run_start..self.pos]);
                    self.pos += 1;
                    match self.byte(0) {
                        None => {}
                        Some(b'\n') => self.pos += 1,
                        Some(_) => out.push(self.consume_escaped_code_point()),
                    }
                    run_start = self.pos;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-numeric-token>
    fn consume_numeric(&mut self) -> Token {
        let number = self.consume_number();
        if self.starts_ident_sequence_at(0) {
            let unit = self.consume_ident_sequence();
            Token::Dimension { number, unit }
        } else if self.byte(0) == Some(b'%') {
            self.pos += 1;
            Token::Percentage(number)
        } else {
            Token::Number(number)
        }
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-number>
    fn consume_number(&mut self) -> Number {
        let start = self.pos;
        let has_sign = matches!(self.byte(0), Some(b'+' | b'-'));
        if has_sign {
            self.pos += 1;
        }
        let int_start = self.pos;
        self.consume_digits();
        let int_end = self.pos;
        let mut is_integer = true;
        if self.byte(0) == Some(b'.') && self.is_digit_at(1) {
            self.pos += 1;
            self.consume_digits();
            is_integer = false;
        }
        if matches!(self.byte(0), Some(b'e' | b'E')) {
            let exponent_digits = if self.is_digit_at(1) {
                Some(1)
            } else if matches!(self.byte(1), Some(b'+' | b'-')) && self.is_digit_at(2) {
                Some(2)
            } else {
                None
            };
            if let Some(skip) = exponent_digits {
                self.pos += skip;
                self.consume_digits();
                is_integer = false;
            }
        }
        let repr = &self.input[start..self.pos];
        let value = repr.parse::<f32>().unwrap_or(0.0);
        let value = if value.is_finite() {
            value
        } else if value > 0.0 {
            f32::MAX
        } else {
            f32::MIN
        };
        let int_value = is_integer.then(|| {
            let negative = self.byte_at(start) == Some(b'-');
            parse_clamped_int(&self.bytes[int_start..int_end], negative)
        });
        Number {
            value,
            int_value,
            has_sign,
        }
    }

    fn byte_at(&self, pos: usize) -> Option<u8> {
        self.bytes.get(pos).copied()
    }

    fn consume_digits(&mut self) {
        while self.is_digit_at(0) {
            self.pos += 1;
        }
    }
}

/// Builds the final value of a string-like token from the escaped prefix (if
/// any) and the last unescaped run.
fn finish_run(escaped: Option<String>, run: &str) -> Box<str> {
    match escaped {
        None => Box::from(run),
        Some(mut out) => {
            out.push_str(run);
            out.into_boxed_str()
        }
    }
}

/// Parses ASCII digits into an `i32`, clamping on overflow.
fn parse_clamped_int(digits: &[u8], negative: bool) -> i32 {
    let mut value: i64 = 0;
    for &d in digits {
        value = value * 10 + i64::from(d - b'0');
        if value > i64::from(i32::MAX) + 1 {
            break;
        }
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(s: &str) -> Token {
        Token::Ident(s.into())
    }

    fn num(value: f32, int_value: Option<i32>, has_sign: bool) -> Number {
        Number {
            value,
            int_value,
            has_sign,
        }
    }

    #[test]
    fn simple_rule() {
        assert_eq!(
            tokenize("a{color:red}"),
            vec![
                ident("a"),
                Token::OpenCurly,
                ident("color"),
                Token::Colon,
                ident("red"),
                Token::CloseCurly,
            ]
        );
    }

    #[test]
    fn whitespace_and_comments() {
        assert_eq!(
            tokenize("a /* c */  b/**/c"),
            vec![
                ident("a"),
                Token::Whitespace,
                Token::Whitespace,
                ident("b"),
                ident("c"),
            ]
        );
        // An unterminated comment runs to the end.
        assert_eq!(
            tokenize("a /* never closed"),
            vec![ident("a"), Token::Whitespace]
        );
        assert_eq!(tokenize("/* only */"), vec![]);
    }

    #[test]
    fn numbers() {
        let cases: &[(&str, Number)] = &[
            ("1", num(1.0, Some(1), false)),
            ("1e3", num(1000.0, None, false)),
            ("1E3", num(1000.0, None, false)),
            ("+.5", num(0.5, None, true)),
            ("-0", num(-0.0, Some(0), true)),
            (".5e-2", num(0.005, None, false)),
            ("-12.5", num(-12.5, None, true)),
            ("+7", num(7.0, Some(7), true)),
            ("1e+2", num(100.0, None, false)),
            ("99999999999", num(99_999_999_999.0, Some(i32::MAX), false)),
            ("-99999999999", num(-99_999_999_999.0, Some(i32::MIN), true)),
            ("1e999", num(f32::MAX, None, false)),
        ];
        for (src, expected) in cases {
            assert_eq!(tokenize(src), vec![Token::Number(*expected)], "{src}");
        }
        // `-0` keeps its sign bit.
        let [Token::Number(n)] = tokenize("-0")[..] else {
            panic!("expected a number");
        };
        assert!(n.value.is_sign_negative());
    }

    #[test]
    fn number_edge_cases() {
        // "1." is a number followed by a delim.
        assert_eq!(
            tokenize("1."),
            vec![Token::Number(num(1.0, Some(1), false)), Token::Delim('.')]
        );
        // "1e" is a dimension with unit "e"; "1e-" too ("e-" is an ident).
        assert_eq!(
            tokenize("1e"),
            vec![Token::Dimension {
                number: num(1.0, Some(1), false),
                unit: "e".into()
            }]
        );
        assert_eq!(
            tokenize("1e-x"),
            vec![Token::Dimension {
                number: num(1.0, Some(1), false),
                unit: "e-x".into()
            }]
        );
        // "+" and "-" alone are delims.
        assert_eq!(
            tokenize("+ -"),
            vec![Token::Delim('+'), Token::Whitespace, Token::Delim('-')]
        );
        assert_eq!(
            tokenize("50%"),
            vec![Token::Percentage(num(50.0, Some(50), false))]
        );
        assert_eq!(
            tokenize("1.5px"),
            vec![Token::Dimension {
                number: num(1.5, None, false),
                unit: "px".into()
            }]
        );
        assert_eq!(
            tokenize("2n+1"),
            vec![
                Token::Dimension {
                    number: num(2.0, Some(2), false),
                    unit: "n".into()
                },
                Token::Number(num(1.0, Some(1), true)),
            ]
        );
        assert_eq!(
            tokenize("2n-1"),
            vec![Token::Dimension {
                number: num(2.0, Some(2), false),
                unit: "n-1".into()
            }]
        );
    }

    #[test]
    fn idents() {
        assert_eq!(tokenize("--foo"), vec![ident("--foo")]);
        assert_eq!(tokenize("--"), vec![ident("--")]);
        assert_eq!(tokenize("-foo"), vec![ident("-foo")]);
        assert_eq!(tokenize("-\\31"), vec![ident("-1")]);
        assert_eq!(tokenize("_x9"), vec![ident("_x9")]);
        assert_eq!(tokenize("héllo"), vec![ident("héllo")]);
        assert_eq!(tokenize("-"), vec![Token::Delim('-')]);
        assert_eq!(
            tokenize("-1a"),
            vec![Token::Dimension {
                number: num(-1.0, Some(-1), true),
                unit: "a".into()
            }]
        );
    }

    #[test]
    fn escapes() {
        assert_eq!(tokenize("\\41 b"), vec![ident("Ab")]);
        assert_eq!(tokenize("\\000041"), vec![ident("A")]);
        assert_eq!(tokenize("a\\.b"), vec![ident("a.b")]);
        assert_eq!(tokenize("\\0"), vec![ident("\u{FFFD}")]);
        assert_eq!(tokenize("\\D800"), vec![ident("\u{FFFD}")]);
        assert_eq!(tokenize("\\110000"), vec![ident("\u{FFFD}")]);
        assert_eq!(tokenize("\\1F600"), vec![ident("😀")]);
        // A backslash at EOF is an escape of EOF.
        assert_eq!(tokenize("\\"), vec![ident("\u{FFFD}")]);
        // A backslash before a newline is a delim.
        assert_eq!(
            tokenize("\\\n"),
            vec![Token::Delim('\\'), Token::Whitespace]
        );
        assert_eq!(
            tokenize("\"a\\\"b\\\nc\\"),
            vec![Token::String("a\"bc".into())]
        );
        assert_eq!(tokenize("'\\e9'"), vec![Token::String("é".into())]);
    }

    #[test]
    fn hashes() {
        assert_eq!(
            tokenize("#foo"),
            vec![Token::Hash {
                value: "foo".into(),
                is_id: true
            }]
        );
        assert_eq!(
            tokenize("#123"),
            vec![Token::Hash {
                value: "123".into(),
                is_id: false
            }]
        );
        assert_eq!(
            tokenize("#-1"),
            vec![Token::Hash {
                value: "-1".into(),
                is_id: false
            }]
        );
        assert_eq!(tokenize("# "), vec![Token::Delim('#'), Token::Whitespace]);
    }

    #[test]
    fn strings() {
        assert_eq!(tokenize("\"abc\""), vec![Token::String("abc".into())]);
        assert_eq!(tokenize("'a\"b'"), vec![Token::String("a\"b".into())]);
        // Unterminated at EOF: a normal string.
        assert_eq!(tokenize("\"abc"), vec![Token::String("abc".into())]);
        // A newline makes a bad string; the newline stays.
        assert_eq!(
            tokenize("\"abc\nd"),
            vec![Token::BadString, Token::Whitespace, ident("d")]
        );
    }

    #[test]
    fn urls() {
        assert_eq!(tokenize("url(foo.png)"), vec![Token::Url("foo.png".into())]);
        assert_eq!(
            tokenize("URL(  foo.png  )"),
            vec![Token::Url("foo.png".into())]
        );
        assert_eq!(tokenize("url(a\\)b)"), vec![Token::Url("a)b".into())]);
        assert_eq!(tokenize("url(foo"), vec![Token::Url("foo".into())]);
        assert_eq!(tokenize("url()"), vec![Token::Url("".into())]);
        assert_eq!(
            tokenize("url( \"foo\" )"),
            vec![
                Token::Function("url".into()),
                Token::Whitespace,
                Token::String("foo".into()),
                Token::Whitespace,
                Token::CloseParen,
            ]
        );
        assert_eq!(
            tokenize("url('x')"),
            vec![
                Token::Function("url".into()),
                Token::String("x".into()),
                Token::CloseParen,
            ]
        );
        // Bad URLs consume up to the closing parenthesis.
        assert_eq!(
            tokenize("url(a b) c"),
            vec![Token::BadUrl, Token::Whitespace, ident("c")]
        );
        assert_eq!(tokenize("url(a\"b)"), vec![Token::BadUrl]);
        assert_eq!(tokenize("url(a(b)"), vec![Token::BadUrl]);
        assert_eq!(tokenize("url(a\u{1}b)"), vec![Token::BadUrl]);
        assert_eq!(
            tokenize("url(a\\\nb) x"),
            vec![Token::BadUrl, Token::Whitespace, ident("x")]
        );
        assert_eq!(tokenize("url(a\\)b c\\) d)"), vec![Token::BadUrl]);
    }

    #[test]
    fn cdo_cdc_and_delims() {
        assert_eq!(
            tokenize("<!-- a -->"),
            vec![
                Token::Cdo,
                Token::Whitespace,
                ident("a"),
                Token::Whitespace,
                Token::Cdc,
            ]
        );
        assert_eq!(tokenize("<"), vec![Token::Delim('<')]);
        assert_eq!(
            tokenize("<!-"),
            vec![Token::Delim('<'), Token::Delim('!'), Token::Delim('-')]
        );
        assert_eq!(tokenize("@"), vec![Token::Delim('@')]);
        assert_eq!(tokenize("@media"), vec![Token::AtKeyword("media".into())]);
        assert_eq!(tokenize("@-x"), vec![Token::AtKeyword("-x".into())]);
        assert_eq!(tokenize("^"), vec![Token::Delim('^')]);
        // Non-ASCII code points are ident code points.
        assert_eq!(tokenize("§"), vec![ident("§")]);
    }

    #[test]
    fn functions_and_brackets() {
        assert_eq!(
            tokenize("rgb(1,2)[x]"),
            vec![
                Token::Function("rgb".into()),
                Token::Number(num(1.0, Some(1), false)),
                Token::Comma,
                Token::Number(num(2.0, Some(2), false)),
                Token::CloseParen,
                Token::OpenSquare,
                ident("x"),
                Token::CloseSquare,
            ]
        );
    }

    #[test]
    fn preprocessing() {
        assert_eq!(preprocess("a\r\nb\rc\x0cd\0"), "a\nb\nc\nd\u{FFFD}");
        assert!(matches!(preprocess("plain"), Cow::Borrowed(_)));
        assert_eq!(
            tokenize("\"a\r\nb\""),
            vec![
                Token::BadString,
                Token::Whitespace,
                ident("b"),
                Token::String("".into())
            ]
        );
        assert_eq!(
            tokenize("\\\r\n"),
            vec![Token::Delim('\\'), Token::Whitespace]
        );
    }

    #[test]
    fn restart_at_position() {
        let mut t = Tokenizer::new("a b");
        let _ = t.next_token();
        let pos = t.position();
        assert_eq!(t.next_token(), Some(Token::Whitespace));
        t.set_position(pos);
        assert_eq!(t.next_token(), Some(Token::Whitespace));
        assert_eq!(t.next_token(), Some(ident("b")));
        assert_eq!(t.next_token(), None);
    }
}
