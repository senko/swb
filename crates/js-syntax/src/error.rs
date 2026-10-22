//! The errors of the front end: a JavaScript `SyntaxError` with its
//! position, and the compile error that the parser returns.

use std::borrow::Cow;
use std::fmt;

/// A syntax error (ECMA-262 §16.1.5: `ParseScript` returns a list of them;
/// the front end stops at the first). The engine turns it into a
/// `SyntaxError` object with `message` as its message.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("SyntaxError: {message} (at offset {offset})")]
pub struct SyntaxError {
    /// The code-unit offset in the source where the error was found.
    pub offset: u32,
    /// The message, worded like Chromium's where the lexer knows it.
    pub message: Cow<'static, str>,
}

impl SyntaxError {
    /// An error at `offset`.
    pub fn new(offset: u32, message: impl Into<Cow<'static, str>>) -> Self {
        SyntaxError {
            offset,
            message: message.into(),
        }
    }
}

/// The JavaScript error type of a [`ParseError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// A `SyntaxError`: the source is not a valid script.
    Syntax,
    /// A `RangeError`: the source nests deeper than the recursion budget
    /// allows (Chromium reports "Maximum call stack size exceeded" for the
    /// same inputs), or the scope analysis hit a resource limit.
    Range,
}

impl ErrorKind {
    /// The name of the error class that the script sees.
    pub fn class_name(self) -> &'static str {
        match self {
            ErrorKind::Syntax => "SyntaxError",
            ErrorKind::Range => "RangeError",
        }
    }
}

/// The error of [`crate::parse_script`]: the first error in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The error type that the script sees.
    pub kind: ErrorKind,
    /// The code-unit offset where the error was found.
    pub offset: u32,
    /// The message, worded like Chromium's where it was measured.
    pub message: Cow<'static, str>,
}

impl ParseError {
    /// A `SyntaxError` at `offset`.
    pub fn syntax(offset: u32, message: impl Into<Cow<'static, str>>) -> Self {
        ParseError {
            kind: ErrorKind::Syntax,
            offset,
            message: message.into(),
        }
    }

    /// A `RangeError` for a resource limit of the analysis.
    pub fn range(offset: u32, message: impl Into<Cow<'static, str>>) -> Self {
        ParseError {
            kind: ErrorKind::Range,
            ..ParseError::syntax(offset, message)
        }
    }

    /// The error for nesting beyond the recursion budget.
    pub fn too_deep(offset: u32) -> Self {
        ParseError {
            kind: ErrorKind::Range,
            ..ParseError::syntax(offset, crate::messages::STACK_OVERFLOW)
        }
    }
}

impl From<SyntaxError> for ParseError {
    fn from(error: SyntaxError) -> Self {
        ParseError::syntax(error.offset, error.message)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.class_name(), self.message)?;
        write!(f, " (at offset {})", self.offset)
    }
}

impl std::error::Error for ParseError {}
