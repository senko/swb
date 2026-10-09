//! The error of the front end: a JavaScript `SyntaxError` with its
//! position.

use std::borrow::Cow;

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
