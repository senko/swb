//! The JavaScript front end of swb (ADR 0026 section 3): the lexer, the
//! parser with the early errors, the abstract syntax tree and the scope
//! analysis.
//!
//! [`parse_script`] parses a classic script into a [`Script`]: the tree
//! ([`Ast`]), the scopes with the results of the scope analysis
//! ([`ScopeTree`]) and the names ([`Interner`]). The parser drives the
//! [`Lexer`]: it asks for one [`Token`] at a time with the [`Goal`]
//! symbol of its context.
//!
//! The "memo" references in the sources are the study memos in
//! `docs/js-study/` (memo 2: `02-front-end.md`).

mod ast;
pub mod dump;
mod error;
mod interner;
mod lexer;
mod lines;
pub mod messages;
mod parser;
mod scope;
mod token;

pub use ast::{
    AssignOp, AssignTarget, Ast, BinaryOp, BindingId, CatchClause, Declarator, Expr, ExprId,
    ExprKind, Function, FunctionId, FunctionKind, Ident, List, LogicalOp, Pattern, PatternId,
    PatternKind, Property, PropertyKey, PropertyKind, RefId, ScopeId, Span, Stmt, StmtId, StmtKind,
    StringId, SwitchCase, Template, TemplateElement, TemplateId, UnaryOp, UpdateOp, VariableKind,
};
pub use error::{ErrorKind, ParseError, SyntaxError};
pub use interner::{Interner, NameId, names};
pub use lexer::{Checkpoint, Lexer, LexerOptions, MAX_SOURCE_LEN};
pub use lines::{LineIndex, Location};
pub use parser::{Script, check_source_len, parse_script};
pub use scope::{
    Binding, BindingKind, Capture, CaptureSource, FunctionScope, Reference, Resolution, Scope,
    ScopeKind, ScopeTree, Storage,
};
pub use token::{Goal, Legacy, Template as TemplateToken, Token, TokenKind, TokenValue};
