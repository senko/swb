//! Component values: the output of the CSS parser below the rule level.
//!
//! A component value is a preserved token, a function or a simple block.
//! See <https://www.w3.org/TR/css-syntax-3/#component-value>.
//!
//! [`ComponentValue`] flattens the preserved tokens into its own variants, so
//! that property parsers can match `ComponentValue::Ident(..)` directly.

use crate::tokenizer::{Number, Token};

/// A component value.
///
/// <https://www.w3.org/TR/css-syntax-3/#component-value>
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    /// An identifier, as written (match keywords ASCII case-insensitively).
    Ident(Box<str>),
    /// An at-keyword (only in unusual places, for example a value).
    AtKeyword(Box<str>),
    /// A hash: `#value`. `is_id` is true if `value` is a valid identifier.
    Hash {
        /// The value without the `#`.
        value: Box<str>,
        /// The "id" type flag.
        is_id: bool,
    },
    /// A quoted string, escapes resolved.
    String(Box<str>),
    /// A string that contains an unescaped newline.
    BadString,
    /// An unquoted `url(...)`. The quoted form `url("...")` is a
    /// [`ComponentValue::Function`] named `url` with a string argument; use
    /// [`crate::Parser::expect_url`] to accept both.
    Url(Box<str>),
    /// A malformed unquoted URL.
    BadUrl,
    /// A single code point that is not part of another token.
    Delim(char),
    /// A number.
    Number(Number),
    /// A percentage. The value is the number before `%` (50 for `50%`).
    Percentage(Number),
    /// A number with a unit.
    Dimension {
        /// The number.
        number: Number,
        /// The unit, as written (match it ASCII case-insensitively).
        unit: Box<str>,
    },
    /// One range of a `unicode-range` descriptor, for example `U+0-7F`.
    /// The parser produces it only for declarations named `unicode-range`.
    /// <https://drafts.csswg.org/css-syntax/#urange>
    UnicodeRange {
        /// The first code point.
        start: u32,
        /// The last code point (inclusive).
        end: u32,
    },
    /// Whitespace.
    Whitespace,
    /// `<!--`.
    Cdo,
    /// `-->`.
    Cdc,
    /// `:`.
    Colon,
    /// `;`.
    Semicolon,
    /// `,`.
    Comma,
    /// An unmatched `)`.
    CloseParen,
    /// An unmatched `]`.
    CloseSquare,
    /// An unmatched `}`.
    CloseCurly,
    /// A function: a name and the values between the parentheses.
    Function(Function),
    /// A simple block: `(...)`, `[...]` or `{...}`.
    Block(SimpleBlock),
}

/// A function component value, for example `rgb(1 2 3)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// The name, as written (match it ASCII case-insensitively).
    pub name: Box<str>,
    /// The values between the parentheses.
    pub arguments: Vec<ComponentValue>,
}

/// The bracket type of a simple block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// `(...)`.
    Paren,
    /// `[...]`.
    Square,
    /// `{...}`.
    Curly,
}

impl BlockKind {
    /// The opening bracket.
    pub fn open(self) -> char {
        match self {
            BlockKind::Paren => '(',
            BlockKind::Square => '[',
            BlockKind::Curly => '{',
        }
    }

    /// The closing bracket.
    pub fn close(self) -> char {
        match self {
            BlockKind::Paren => ')',
            BlockKind::Square => ']',
            BlockKind::Curly => '}',
        }
    }
}

/// A simple block component value.
#[derive(Clone, Debug, PartialEq)]
pub struct SimpleBlock {
    /// The bracket type.
    pub kind: BlockKind,
    /// The values between the brackets.
    pub contents: Vec<ComponentValue>,
}

impl ComponentValue {
    /// Converts a preserved token. Returns `None` for tokens that start a
    /// function or a block; the parser handles those.
    pub(crate) fn from_preserved_token(token: Token) -> Option<Self> {
        Some(match token {
            Token::Ident(s) => ComponentValue::Ident(s),
            Token::AtKeyword(s) => ComponentValue::AtKeyword(s),
            Token::Hash { value, is_id } => ComponentValue::Hash { value, is_id },
            Token::String(s) => ComponentValue::String(s),
            Token::BadString => ComponentValue::BadString,
            Token::Url(s) => ComponentValue::Url(s),
            Token::BadUrl => ComponentValue::BadUrl,
            Token::Delim(c) => ComponentValue::Delim(c),
            Token::Number(n) => ComponentValue::Number(n),
            Token::Percentage(n) => ComponentValue::Percentage(n),
            Token::Dimension { number, unit } => ComponentValue::Dimension { number, unit },
            Token::Whitespace => ComponentValue::Whitespace,
            Token::Cdo => ComponentValue::Cdo,
            Token::Cdc => ComponentValue::Cdc,
            Token::Colon => ComponentValue::Colon,
            Token::Semicolon => ComponentValue::Semicolon,
            Token::Comma => ComponentValue::Comma,
            Token::CloseParen => ComponentValue::CloseParen,
            Token::CloseSquare => ComponentValue::CloseSquare,
            Token::CloseCurly => ComponentValue::CloseCurly,
            Token::Function(_) | Token::OpenParen | Token::OpenSquare | Token::OpenCurly => {
                return None;
            }
        })
    }

    /// True for whitespace.
    pub fn is_whitespace(&self) -> bool {
        matches!(self, ComponentValue::Whitespace)
    }

    /// True for a comma.
    pub fn is_comma(&self) -> bool {
        matches!(self, ComponentValue::Comma)
    }

    /// True for the delim `c`.
    pub fn is_delim(&self, c: char) -> bool {
        matches!(self, ComponentValue::Delim(d) if *d == c)
    }

    /// The identifier, if this is one.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            ComponentValue::Ident(s) => Some(s),
            _ => None,
        }
    }

    /// True if this is an identifier that matches `name` ASCII
    /// case-insensitively.
    pub fn is_ident(&self, name: &str) -> bool {
        self.as_ident()
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
    }

    /// The function, if this is one.
    pub fn as_function(&self) -> Option<&Function> {
        match self {
            ComponentValue::Function(f) => Some(f),
            _ => None,
        }
    }

    /// True if this is a function whose name matches `name` ASCII
    /// case-insensitively.
    pub fn is_function(&self, name: &str) -> bool {
        self.as_function()
            .is_some_and(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// True if this value is, or contains at any depth, a function whose
    /// name matches `name` ASCII case-insensitively. Use it to find `var()`
    /// before parsing a property value.
    pub fn contains_function(&self, name: &str) -> bool {
        match self {
            ComponentValue::Function(f) => {
                f.name.eq_ignore_ascii_case(name) || contains_function(&f.arguments, name)
            }
            ComponentValue::Block(b) => contains_function(&b.contents, name),
            _ => false,
        }
    }

    /// The contents of a simple block of the given kind, if this is one.
    pub fn as_block(&self, kind: BlockKind) -> Option<&[ComponentValue]> {
        match self {
            ComponentValue::Block(b) if b.kind == kind => Some(&b.contents),
            _ => None,
        }
    }
}

/// True if any of `values` is, or contains at any depth, a function whose
/// name matches `name` ASCII case-insensitively (for example `var`).
pub fn contains_function(values: &[ComponentValue], name: &str) -> bool {
    values.iter().any(|v| v.contains_function(name))
}

/// Returns `values` without leading and trailing whitespace.
pub fn trim_whitespace(values: &[ComponentValue]) -> &[ComponentValue] {
    let start = values
        .iter()
        .position(|v| !v.is_whitespace())
        .unwrap_or(values.len());
    let end = values
        .iter()
        .rposition(|v| !v.is_whitespace())
        .map_or(start, |i| i + 1);
    &values[start..end]
}

/// Splits `values` at top-level commas. Commas inside functions and blocks
/// do not split. The parts are not trimmed.
pub(crate) fn split_on_commas(
    values: &[ComponentValue],
) -> impl Iterator<Item = &[ComponentValue]> {
    values.split(ComponentValue::is_comma)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_component_values;

    #[test]
    fn helpers() {
        let values = parse_component_values("  a , rgb(1,2) [x] ");
        let trimmed = trim_whitespace(&values);
        assert!(trimmed[0].is_ident("A"));
        assert!(matches!(trimmed.last(), Some(ComponentValue::Block(_))));
        let parts: Vec<_> = split_on_commas(trimmed).collect();
        assert_eq!(parts.len(), 2);
        assert!(trim_whitespace(parts[1])[0].is_function("RGB"));
        assert_eq!(
            trimmed.last().and_then(|v| v.as_block(BlockKind::Square)),
            Some(&[ComponentValue::Ident("x".into())][..])
        );
        assert_eq!(trim_whitespace(&[ComponentValue::Whitespace]).len(), 0);
        let values = parse_component_values("1px calc(2px + max(VAR(--x), 3px)) [env(y)]");
        assert!(contains_function(&values, "var"));
        assert!(contains_function(&values, "env"));
        assert!(contains_function(&values, "calc"));
        assert!(!contains_function(&values, "attr"));
    }
}
