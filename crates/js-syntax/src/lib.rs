//! The JavaScript front end of swb (ADR 0026 section 3): the lexer, and
//! later the parser, the abstract syntax tree, the early errors and the
//! scope analysis.
//!
//! The parser drives the [`Lexer`]: it asks for one [`Token`] at a time
//! with the [`Goal`] symbol of its context. Identifier names go into an
//! [`Interner`] that the lexer and the parser share.

mod error;
mod interner;
mod lexer;
mod token;

pub use error::SyntaxError;
pub use interner::{Interner, NameId, names};
pub use lexer::{Checkpoint, Lexer, LexerOptions, MAX_SOURCE_LEN};
pub use token::{Goal, Legacy, Template, Token, TokenKind, TokenValue};
