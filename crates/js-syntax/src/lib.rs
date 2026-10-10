//! The JavaScript front end of swb (ADR 0026 section 3): the lexer, the
//! parser with the early errors, the abstract syntax tree and the scope
//! analysis.
//!
//! [`parse_script`] parses a classic script into a [`Script`]: the tree
//! ([`Ast`]), the scopes with the results of the scope analysis
//! ([`ScopeTree`]) and the names ([`Interner`]). [`parse_module`] parses
//! a module (with its [`ModuleRecord`]) and [`parse_eval`] eval code in
//! the context of its call ([`EvalContext`]). The parser drives the
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
mod module;
mod parser;
mod scope;
mod token;

pub use ast::{
    AssignOp, AssignTarget, Ast, BinaryOp, BindingId, CatchClause, Class, ClassElement,
    ClassElementKind, ClassId, ClassKey, Declarator, Expr, ExprId, ExprKind, ForHead, Function,
    FunctionId, FunctionKind, Ident, List, LogicalOp, Pattern, PatternId, PatternKind,
    PatternProperty, Property, PropertyKey, PropertyKind, RefId, ScopeId, Span, Stmt, StmtId,
    StmtKind, StringId, SuperCall, SuperCallId, SwitchCase, Template, TemplateElement, TemplateId,
    UnaryOp, UpdateOp, VariableKind,
};
pub use error::{ErrorKind, ParseError, SyntaxError};
pub use interner::{Interner, NameId, names};
pub use lexer::{Checkpoint, Lexer, LexerOptions, MAX_SOURCE_LEN};
pub use lines::{LineIndex, Location};
pub use module::{
    ExportEntry, ExportImportName, ImportAttribute, ImportEntry, ImportName, ModuleRecord,
    ModuleRequest,
};
pub use parser::{EvalContext, Script, check_source_len, parse_eval, parse_module, parse_script};
pub use scope::{
    Binding, BindingKind, Capture, CaptureSource, DynamicEnv, DynamicKind, DynamicLookup,
    FunctionScope, Reference, Resolution, Scope, ScopeKind, ScopeTree, Storage,
};
pub use token::{Goal, Legacy, Template as TemplateToken, Token, TokenKind, TokenValue};
