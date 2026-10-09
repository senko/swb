//! The parser: recursive descent for statements, precedence climbing for
//! binary operators (ADR 0026 section 3). It drives the [`Lexer`] one
//! token at a time, builds the [`Ast`] and records declarations and
//! identifier occurrences in the [`ScopeTree`]. All early errors of the
//! supported subset are reported here, at the first error in the source
//! (§16.1.5).
//!
//! Goal symbols: the parser asks the lexer for tokens with the `Div` goal
//! and re-scans a token where the syntactic context needs another goal:
//! `/` and `/=` where a primary expression starts become a regular
//! expression literal, and the `}` that ends a template substitution
//! continues the template (§12, memo 2.2).
//!
//! Recursion: every recursive level charges the shared recursion budget
//! (ADR 0026 section 9), and each link of a left-associative chain
//! (`a + b + c`, `a.b.c`, `f()()`) charges it too, because it makes the
//! tree one level deeper. Exceeding the budget is a `RangeError`, as in
//! Chromium.
//!
//! Constructs outside the subset of the spike give a `SyntaxError` with
//! the message "not supported yet" ([`ParseError::unsupported`]).

mod expressions;
mod functions;
mod statements;
#[cfg(test)]
mod tests;

use swb_js_text::{CodeUnit, RecursionBudget, Str16};

use crate::ast::{
    Ast, Declarator, ExprId, FunctionId, FunctionKind, Ident, List, PatternId, Property, ScopeId,
    StmtId, TemplateElement,
};
use crate::error::ParseError;
use crate::interner::{Interner, NameId, names};
use crate::lexer::{Lexer, LexerOptions, MAX_SOURCE_LEN};
use crate::messages;
use crate::scope::{Redeclared, ScopeKind, ScopeTree, analysis};
use crate::token::{Goal, Legacy, Token, TokenKind, TokenValue};

/// The budget weight of one recursive parser level (a statement or an
/// assignment expression). Measured stack use per nesting level, in a
/// debug build (`opt-level = 1`) and a release build: up to 2,080 bytes
/// (nested template substitutions), 1,856 (parentheses, arrays, objects),
/// 1,792 (call arguments); statements 470 to 1,150. The weight leaves a
/// margin of about 1.5.
pub(crate) const LEVEL_WEIGHT: u32 = 3072;

/// The budget weight of a unary operator, `new` or the right operand of
/// `**` (measured: up to 480 bytes per level).
pub(crate) const SMALL_WEIGHT: u32 = 768;

/// The extra budget weight of a function: a nested function expression
/// costs up to 5,330 bytes per level, of which two levels are charged
/// with [`LEVEL_WEIGHT`].
pub(crate) const FUNCTION_WEIGHT: u32 = 2048;

/// The budget weight of one link of a left-associative chain: one more
/// tree level, which later passes that recurse over the tree pay for.
/// A pass that recurses over the tree must use at most this much stack
/// per level (in a debug build), or charge the budget itself.
pub(crate) const CHAIN_WEIGHT: u32 = 256;

/// The result type of the parser.
pub(crate) type PResult<T> = Result<T, ParseError>;

/// A parsed and analysed script.
#[derive(Debug)]
pub struct Script {
    /// The nodes.
    pub ast: Ast,
    /// The scopes, bindings and identifier occurrences, with the results
    /// of the scope analysis.
    pub scopes: ScopeTree,
    /// The names of the script.
    pub names: Interner,
    /// The top-level code: a [`FunctionKind::Script`] function.
    pub top: FunctionId,
}

impl Script {
    /// The text of a name (lossy for lone surrogates).
    pub fn name_text(&self, name: NameId) -> String {
        self.names
            .get(name)
            .map(Str16::to_string_lossy)
            .unwrap_or_default()
    }
}

/// Checks the length of a source: offsets are `u32`, so a source has
/// fewer than 2^31 code units (ADR 0026 section 3).
pub fn check_source_len(len: usize) -> Result<(), ParseError> {
    if len > MAX_SOURCE_LEN {
        return Err(ParseError::syntax(0, messages::SOURCE_TOO_LONG));
    }
    Ok(())
}

/// Parses a classic script (ECMA-262 §16.1.5 `ParseScript`) and runs the
/// scope analysis. `budget` is the shared recursion budget: the parse
/// charges it while it nests and gives everything back when it returns.
pub fn parse_script(source: Str16<'_>, budget: &mut RecursionBudget) -> Result<Script, ParseError> {
    check_source_len(source.len())?;
    let initial = *budget;
    let result = match source {
        Str16::Latin1(units) => Parser::new(units, budget).parse_script(),
        Str16::Wide(units) => Parser::new(units, budget).parse_script(),
    };
    *budget = initial;
    result
}

/// [`parse_script`] with explicit analysis limits, for tests.
#[cfg(test)]
pub(crate) fn parse_script_with_limits(
    source: &[u16],
    limits: analysis::Limits,
) -> Result<Script, ParseError> {
    let mut budget = RecursionBudget::default();
    let mut parser = Parser::new(source, &mut budget);
    parser.limits = limits;
    parser.parse_script()
}

/// What kind of statement position a statement is in (for function and
/// lexical declarations, which only a statement list allows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StmtContext {
    /// An element of a statement list (block, function body, script).
    ListItem,
    /// The body of an `if` or `else`: a sloppy-mode function declaration
    /// is allowed (Annex B.3.3).
    If,
    /// The body of a labelled statement in a statement list: a sloppy-mode
    /// function declaration is allowed (Annex B.3.1).
    LabelInList,
    /// Any other single statement (loop bodies, labels there).
    Other,
}

/// A label in scope.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Label {
    name: NameId,
    /// Whether it labels an iteration statement (`continue` may name it).
    is_loop: bool,
    /// Whether it labels a block.
    is_block: bool,
}

/// The state of the function being parsed.
#[derive(Clone, Debug)]
pub(crate) struct Context {
    function: FunctionId,
    kind: FunctionKind,
    strict: bool,
    generator: bool,
    /// Labels at indices below this belong to enclosing functions.
    label_base: usize,
    /// The number of enclosing iteration statements in this function.
    iterations: u32,
    /// The name of the function, for the retroactive strict mode check.
    name: Option<(NameId, u32)>,
    /// The parameters, for the retroactive strict mode check.
    params: List<PatternId>,
    /// The offset of the first repeated parameter name.
    duplicate_param: Option<u32>,
}

impl Context {
    fn new(
        function: FunctionId,
        kind: FunctionKind,
        strict: bool,
        generator: bool,
        label_base: usize,
    ) -> Self {
        Context {
            function,
            kind,
            strict,
            generator,
            label_base,
            iterations: 0,
            name: None,
            params: List::EMPTY,
            duplicate_param: None,
        }
    }
}

/// How an identifier is used (for the early errors of §13.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdentUse {
    /// An `IdentifierReference` or a label.
    Reference,
    /// A `BindingIdentifier` of a `var`, a parameter, a function name or
    /// a catch parameter.
    Binding,
    /// A `BindingIdentifier` of `let` or `const`.
    Lexical,
}

/// The parser state.
pub(crate) struct Parser<'a, 'b, U: CodeUnit> {
    lexer: Lexer<'a, U>,
    /// The current token.
    token: Token,
    /// The token after the current one, if the parser looked ahead. The
    /// lexer is positioned after it.
    peeked: Option<Token>,
    /// The end of the previous token.
    prev_end: u32,
    /// Whether the previous token was `await` (for the error message when
    /// a statement does not end after it, as in `x = await 1`).
    prev_await: bool,
    /// The limits of the scope analysis (smaller ones in tests).
    limits: analysis::Limits,
    ast: Ast,
    scopes: ScopeTree,
    budget: &'b mut RecursionBudget,
    ctx: Context,
    /// The current scope.
    scope: ScopeId,
    labels: Vec<Label>,
    /// The first label of the chain of labels directly before the
    /// statement being parsed.
    label_chain: Option<usize>,
    // Scratch stacks for building lists (each nested list restores its
    // mark before it returns).
    scratch_exprs: Vec<ExprId>,
    scratch_stmts: Vec<StmtId>,
    scratch_patterns: Vec<PatternId>,
    scratch_properties: Vec<Property>,
    scratch_declarators: Vec<Declarator>,
    scratch_quasis: Vec<TemplateElement>,
}

impl<'a, 'b, U: CodeUnit> Parser<'a, 'b, U> {
    fn new(source: &'a [U], budget: &'b mut RecursionBudget) -> Self {
        let lexer = Lexer::new(source, LexerOptions::script(), Interner::new());
        let mut ast = Ast::default();
        let mut scopes = ScopeTree::default();
        let top = ast.push_function(crate::ast::Function {
            kind: FunctionKind::Script,
            is_generator: false,
            is_async: false,
            strict: false,
            is_declaration: false,
            expression_body: false,
            name: None,
            params: List::EMPTY,
            body: List::EMPTY,
            span: crate::ast::Span::new(0, source.len() as u32),
            scope: ScopeId::from_index(0),
            parent: None,
        });
        scopes.add_function();
        let scope = scopes.push_scope(ScopeKind::Script, None, top, 0);
        Parser {
            lexer,
            token: Token {
                kind: TokenKind::Eof,
                start: 0,
                end: 0,
                newline_before: false,
                escaped: false,
                legacy: Legacy::None,
                value: TokenValue::None,
            },
            peeked: None,
            prev_end: 0,
            prev_await: false,
            limits: analysis::Limits::DEFAULT,
            ast,
            scopes,
            budget,
            ctx: Context::new(top, FunctionKind::Script, false, false, 0),
            scope,
            labels: Vec::new(),
            label_chain: None,
            scratch_exprs: Vec::new(),
            scratch_stmts: Vec::new(),
            scratch_patterns: Vec::new(),
            scratch_properties: Vec::new(),
            scratch_declarators: Vec::new(),
            scratch_quasis: Vec::new(),
        }
    }

    /// `ScriptBody`: the statements up to the end, with the directive
    /// prologue; then the scope analysis.
    fn parse_script(mut self) -> PResult<Script> {
        self.token = self.lexer.next_token(Goal::HashbangOrRegExp)?;
        let body = self.parse_statements(TokenKind::Eof, true)?;
        let top = self.ctx.function;
        let strict = self.ctx.strict;
        if let Some(function) = self.ast.function_mut(top) {
            function.body = body;
            function.strict = strict;
        }
        analysis::analyze(&self.ast, &mut self.scopes, &self.limits)?;
        Ok(Script {
            ast: self.ast,
            scopes: self.scopes,
            names: self.lexer.into_interner(),
            top,
        })
    }

    // --- Tokens ---

    /// Moves to the next token.
    fn advance(&mut self) -> PResult<()> {
        self.prev_end = self.token.end;
        self.prev_await = self.token.kind == TokenKind::Await;
        self.token = match self.peeked.take() {
            Some(token) => token,
            None => self.lexer.next_token(Goal::Div)?,
        };
        Ok(())
    }

    /// The token after the current one.
    fn peek(&mut self) -> PResult<&Token> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lexer.next_token(Goal::Div)?);
        }
        Ok(self
            .peeked
            .as_ref()
            .expect("the peeked token was just stored"))
    }

    /// Scans the current token again with another goal symbol.
    fn rescan(&mut self, goal: Goal) -> PResult<()> {
        self.peeked = None;
        self.token = self.lexer.rescan(&self.token, goal)?;
        Ok(())
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.token.kind == kind
    }

    /// Consumes the current token if it is `kind`.
    fn eat(&mut self, kind: TokenKind) -> PResult<bool> {
        if self.at(kind) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Consumes a token of `kind`, or fails with "Unexpected token".
    fn expect(&mut self, kind: TokenKind) -> PResult<()> {
        if self.at(kind) {
            self.advance()
        } else {
            Err(self.unexpected())
        }
    }

    /// Whether the current token is the identifier `name` without escapes.
    fn at_contextual(&self, name: NameId) -> bool {
        self.token.kind == TokenKind::Identifier
            && !self.token.escaped
            && self.token.name() == Some(name)
    }

    /// The end of a statement: `;`, or an inserted semicolon (§12.10)
    /// before `}`, at the end, or after a line terminator.
    fn consume_semicolon(&mut self) -> PResult<()> {
        match self.token.kind {
            TokenKind::Semicolon => self.advance(),
            TokenKind::RBrace | TokenKind::Eof => Ok(()),
            _ if self.token.newline_before => Ok(()),
            _ if self.prev_await => Err(ParseError::syntax(
                self.token.start,
                messages::AWAIT_OUTSIDE_ASYNC,
            )),
            _ => Err(self.unexpected()),
        }
    }

    // --- Errors ---

    /// The error for an unexpected current token, worded like V8's.
    fn unexpected(&self) -> ParseError {
        self.unexpected_token(&self.token)
    }

    fn unexpected_token(&self, token: &Token) -> ParseError {
        let message: std::borrow::Cow<'static, str> = match token.kind {
            TokenKind::Eof => messages::UNEXPECTED_END.into(),
            TokenKind::Number | TokenKind::BigInt => messages::UNEXPECTED_NUMBER.into(),
            TokenKind::String => messages::UNEXPECTED_STRING.into(),
            TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateHead
            | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail => messages::UNEXPECTED_TEMPLATE.into(),
            TokenKind::Identifier => {
                let name = token.name().unwrap_or(names::AWAIT);
                if self.ctx.strict && is_strict_reserved(name) {
                    messages::UNEXPECTED_STRICT_RESERVED.into()
                } else {
                    messages::unexpected_identifier(&self.name_text(name)).into()
                }
            }
            TokenKind::Yield if self.ctx.strict => messages::UNEXPECTED_STRICT_RESERVED.into(),
            TokenKind::Yield => messages::unexpected_identifier("yield").into(),
            TokenKind::Enum => messages::UNEXPECTED_RESERVED.into(),
            TokenKind::RegExp => "Unexpected regular expression".into(),
            kind => messages::unexpected_token(kind.text()).into(),
        };
        ParseError::syntax(token.start, message)
    }

    fn name_text(&self, name: NameId) -> String {
        self.lexer
            .interner()
            .get(name)
            .map(Str16::to_string_lossy)
            .unwrap_or_default()
    }

    fn redeclared(&self, name: NameId, offset: u32) -> ParseError {
        ParseError::syntax(offset, messages::already_declared(&self.name_text(name)))
    }

    /// Converts a redeclaration result into the early error.
    fn declared<T>(&self, result: Result<T, Redeclared>, name: NameId, offset: u32) -> PResult<T> {
        result.map_err(|Redeclared| self.redeclared(name, offset))
    }

    // --- Recursion budget ---

    /// Charges one recursive level.
    fn enter(&mut self) -> PResult<()> {
        self.charge(LEVEL_WEIGHT)
    }

    fn leave(&mut self) {
        self.budget.leave(LEVEL_WEIGHT);
    }

    /// Charges a small recursive level (unary operators, `new`, `**`).
    fn enter_small(&mut self) -> PResult<()> {
        self.charge(SMALL_WEIGHT)
    }

    fn leave_small(&mut self) {
        self.budget.leave(SMALL_WEIGHT);
    }

    fn charge(&mut self, weight: u32) -> PResult<()> {
        self.budget
            .enter(weight)
            .map_err(|_| ParseError::too_deep(self.token.start))
    }

    // --- Identifiers (§13.1) ---

    /// Checks the current token as an identifier used as `usage` and
    /// returns its name. Does not advance.
    fn check_identifier(&self, usage: IdentUse) -> PResult<NameId> {
        self.check_identifier_token(&self.token, usage)
    }

    /// Checks `token` as an identifier used as `usage`.
    fn check_identifier_token(&self, token: &Token, usage: IdentUse) -> PResult<NameId> {
        let (name, escaped) = match token.kind {
            TokenKind::Identifier => (token.name().unwrap_or(names::AWAIT), token.escaped),
            TokenKind::Yield => (names::YIELD, false),
            TokenKind::Await => (names::AWAIT, false),
            _ => return Err(self.unexpected_token(token)),
        };
        let error = |message: &'static str| Err(ParseError::syntax(token.start, message));
        if escaped
            && let Some(keyword) = TokenKind::keyword_from_name(name)
            && !matches!(keyword, TokenKind::Yield | TokenKind::Await)
        {
            return error(messages::ESCAPED_KEYWORD);
        }
        if name == names::YIELD && self.ctx.generator {
            return Err(ParseError::syntax(
                token.start,
                messages::unexpected_identifier("yield"),
            ));
        }
        if self.ctx.strict && is_strict_reserved(name) {
            return error(messages::UNEXPECTED_STRICT_RESERVED);
        }
        if usage != IdentUse::Reference && self.ctx.strict && is_eval_or_arguments(name) {
            return error(messages::UNEXPECTED_EVAL_OR_ARGUMENTS);
        }
        if usage == IdentUse::Lexical && name == names::LET {
            return error(messages::LET_IN_LEXICAL);
        }
        Ok(name)
    }

    /// Whether the current token can be an identifier (before the checks
    /// of [`Parser::check_identifier`]).
    fn at_identifier(&self) -> bool {
        matches!(
            self.token.kind,
            TokenKind::Identifier | TokenKind::Yield | TokenKind::Await
        )
    }

    /// Records an identifier occurrence in the current scope.
    fn reference(&mut self, name: NameId, offset: u32, declaration: bool) -> Ident {
        let reference = self
            .scopes
            .push_reference(name, self.scope, offset, declaration);
        Ident { name, reference }
    }

    /// Opens a scope inside the current one and makes it current. Returns
    /// the previous scope, to restore with [`Parser::close_scope`].
    fn open_scope(&mut self, kind: ScopeKind, start: u32) -> ScopeId {
        let outer = self.scope;
        self.scope = self
            .scopes
            .push_scope(kind, Some(outer), self.ctx.function, start);
        outer
    }

    fn close_scope(&mut self, outer: ScopeId) {
        self.scopes.close_scope(self.scope);
        self.scope = outer;
    }

    /// Checks a literal token against the strict mode rules for Annex B
    /// numeric and string forms.
    fn check_legacy(&self, token: &Token) -> PResult<()> {
        if !self.ctx.strict {
            return Ok(());
        }
        legacy_error(token.legacy).map_or(Ok(()), |message| {
            Err(ParseError::syntax(token.start, message))
        })
    }
}

/// The strict mode error of an Annex B literal form.
fn legacy_error(legacy: Legacy) -> Option<&'static str> {
    match legacy {
        Legacy::None => None,
        Legacy::OctalInteger => Some(messages::STRICT_OCTAL_LITERAL),
        Legacy::LeadingZeroDecimal => Some(messages::STRICT_LEADING_ZERO),
        Legacy::OctalEscape => Some(messages::STRICT_OCTAL_ESCAPE),
        Legacy::EightOrNine => Some(messages::STRICT_EIGHT_OR_NINE),
    }
}

/// The identifiers that strict mode code reserves (§13.1.1, §12.7.2).
fn is_strict_reserved(name: NameId) -> bool {
    matches!(
        name,
        names::IMPLEMENTS
            | names::INTERFACE
            | names::LET
            | names::PACKAGE
            | names::PRIVATE
            | names::PROTECTED
            | names::PUBLIC
            | names::STATIC
            | names::YIELD
    )
}

/// `eval` and `arguments`, which strict mode code cannot bind or assign.
fn is_eval_or_arguments(name: NameId) -> bool {
    name == names::EVAL || name == names::ARGUMENTS
}
