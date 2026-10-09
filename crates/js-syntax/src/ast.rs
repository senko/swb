//! The abstract syntax tree (ADR 0026 section 3).
//!
//! The nodes of one script live in an arena ([`Ast`]): vectors of nodes,
//! addressed by `u32` ids, freed as a whole after the compile. Lists of
//! children ([`List`]) are ranges in shared pools. Positions are
//! code-unit offsets ([`Span`]). Names are [`NameId`]s of the script's
//! [`crate::Interner`]; string values are [`StringId`]s.
//!
//! The node types are laid out for the whole ES2025 grammar. The parser
//! produces a subset until M7; the forms that M7 adds get new enum
//! variants without a change of the existing ones:
//!
//! - [`PatternKind`]: array and object patterns, defaults and rest
//!   elements next to the identifier form;
//! - [`AssignTarget`]: a pattern for destructuring assignment;
//! - [`ExprKind`]: spread, classes, tagged templates, optional chains,
//!   `super`, `new.target`, `await`, `yield*` (a flag of `Yield`);
//! - [`StmtKind`]: `for`-`in`, `for`-`of`, `with`, classes;
//! - [`FunctionKind`] and [`Function::is_async`] already cover all
//!   function forms.

use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use swb_js_regexp::Flags;
use swb_js_text::String16;

use crate::interner::NameId;

/// A source range in code units: `start..end`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// The offset of the first code unit.
    pub start: u32,
    /// The offset after the last code unit.
    pub end: u32,
}

impl Span {
    /// The range `start..end`.
    pub const fn new(start: u32, end: u32) -> Self {
        Span { start, end }
    }
}

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The id of the element at `index` of its table.
            pub(crate) fn from_index(index: usize) -> Self {
                let value = u32::try_from(index)
                    .ok()
                    .and_then(|i| i.checked_add(1))
                    .and_then(NonZeroU32::new)
                    .expect("tables have fewer than 2^32 - 1 elements");
                $name(value)
            }

            /// The index of the element in its table.
            pub fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.index())
            }
        }
    )*};
}

ids! {
    /// An expression in [`Ast::expr`].
    ExprId;
    /// A statement in [`Ast::stmt`].
    StmtId;
    /// A binding pattern in [`Ast::pattern`].
    PatternId;
    /// A function (or the top-level code of the script) in
    /// [`Ast::function`].
    FunctionId;
    /// A string value in [`Ast::string`].
    StringId;
    /// A template literal in [`Ast::template`].
    TemplateId;
    /// A scope in [`crate::ScopeTree::scope`].
    ScopeId;
    /// A binding in [`crate::ScopeTree::binding`].
    BindingId;
    /// An identifier occurrence (a reference or a binding identifier) in
    /// [`crate::ScopeTree::reference`].
    RefId;
}

/// A list of children: a range in one of the pools of the [`Ast`].
pub struct List<T> {
    start: u32,
    len: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> List<T> {
    /// The empty list.
    pub const EMPTY: List<T> = List {
        start: 0,
        len: 0,
        marker: PhantomData,
    };

    pub(crate) fn new(start: usize, len: usize) -> Self {
        List {
            start: u32::try_from(start).expect("pools have fewer than 2^32 elements"),
            len: u32::try_from(len).expect("pools have fewer than 2^32 elements"),
            marker: PhantomData,
        }
    }

    /// The number of elements.
    pub fn len(self) -> usize {
        self.len as usize
    }

    /// Whether the list is empty.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.start as usize + self.len as usize
    }
}

impl<T> Clone for List<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for List<T> {}

impl<T> PartialEq for List<T> {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.len == other.len
    }
}

impl<T> Eq for List<T> {}

impl<T> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "List({}..+{})", self.start, self.len)
    }
}

/// An identifier with its occurrence id. Every identifier of the source
/// that names a binding (references and binding identifiers) has its own
/// [`RefId`]; the scope analysis says where it resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident {
    /// The name (escapes decoded).
    pub name: NameId,
    /// The occurrence.
    pub reference: RefId,
}

// --- Expressions ---

/// An expression node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Expr {
    /// The kind and the children.
    pub kind: ExprKind,
    /// The source range.
    pub span: Span,
}

/// The kinds of expressions (ECMA-262 clause 13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExprKind {
    /// An `IdentifierReference`.
    Identifier(Ident),
    /// `this`. The reference resolves to the `this` binding of the nearest
    /// non-arrow function (or of the script).
    This(RefId),
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A numeric literal.
    Number(f64),
    /// A string literal.
    String(StringId),
    /// A template literal without a tag.
    Template(TemplateId),
    /// A regular expression literal: the pattern text and the checked
    /// flags.
    RegExp {
        /// The source text between the slashes.
        pattern: StringId,
        /// The flags.
        flags: Flags,
    },
    /// An array literal. An elision is a [`ExprKind::Hole`] element.
    Array(List<ExprId>),
    /// An elision in an array literal (`[a, , b]`).
    Hole,
    /// An object literal.
    Object(List<Property>),
    /// A function expression or an arrow function.
    Function(FunctionId),
    /// A parenthesized expression. Kept for the early errors that depend
    /// on parentheses; the compiler treats it as its content.
    Paren(ExprId),
    /// A unary operator (`delete`, `void`, `typeof`, `+`, `-`, `~`, `!`).
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        argument: ExprId,
    },
    /// `++` or `--`, prefix or postfix. The target is an
    /// [`ExprKind::Identifier`], [`ExprKind::Member`], [`ExprKind::Index`]
    /// or (in sloppy mode code, Annex B web compatibility) an
    /// [`ExprKind::Call`], which throws a `ReferenceError` at run time.
    /// Parentheses around the target are removed.
    Update {
        /// `++` or `--`.
        op: UpdateOp,
        /// Whether the operator comes first.
        prefix: bool,
        /// The target.
        target: ExprId,
    },
    /// A binary operator other than `&&`, `||` and `??`.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        left: ExprId,
        /// The right operand.
        right: ExprId,
    },
    /// `&&`, `||` or `??`.
    Logical {
        /// The operator.
        op: LogicalOp,
        /// The left operand.
        left: ExprId,
        /// The right operand.
        right: ExprId,
    },
    /// `test ? consequent : alternate`.
    Conditional {
        /// The condition.
        test: ExprId,
        /// The value if the condition is truthy.
        consequent: ExprId,
        /// The value otherwise.
        alternate: ExprId,
    },
    /// An assignment, plain or compound.
    Assign {
        /// The operator.
        op: AssignOp,
        /// The target.
        target: AssignTarget,
        /// The value.
        value: ExprId,
    },
    /// The comma operator: two or more expressions.
    Sequence(List<ExprId>),
    /// `object.property`.
    Member {
        /// The object.
        object: ExprId,
        /// The property name.
        property: NameId,
    },
    /// `object[index]`.
    Index {
        /// The object.
        object: ExprId,
        /// The key expression.
        index: ExprId,
    },
    /// A call. A direct `eval` is a call whose callee is the identifier
    /// `eval`, possibly in parentheses.
    Call {
        /// The function.
        callee: ExprId,
        /// The arguments.
        arguments: List<ExprId>,
    },
    /// `new callee(arguments)`; `new callee` has no arguments.
    New {
        /// The constructor.
        callee: ExprId,
        /// The arguments.
        arguments: List<ExprId>,
    },
    /// `yield` with an optional operand (in generator functions).
    Yield {
        /// The operand.
        argument: Option<ExprId>,
    },
}

/// The target of an assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignTarget {
    /// A simple target: an [`ExprKind::Identifier`], [`ExprKind::Member`]
    /// or [`ExprKind::Index`], or (in sloppy mode code, `=` and the
    /// arithmetic compound operators only) an [`ExprKind::Call`], which
    /// throws a `ReferenceError` at run time (Chromium's web
    /// compatibility behaviour). Parentheses around the target are
    /// removed.
    Simple(ExprId),
}

/// Unary operators (§13.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `delete`
    Delete,
    /// `void`
    Void,
    /// `typeof`
    Typeof,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `~`
    BitNot,
    /// `!`
    Not,
}

/// Update operators (§13.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOp {
    /// `++`
    Increment,
    /// `--`
    Decrement,
}

/// Binary operators (§13.6 to §13.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    /// `**`
    Exponent,
    /// `*`
    Multiply,
    /// `/`
    Divide,
    /// `%`
    Remainder,
    /// `+`
    Add,
    /// `-`
    Subtract,
    /// `<<`
    ShiftLeft,
    /// `>>`
    ShiftRight,
    /// `>>>`
    ShiftRightUnsigned,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessEqual,
    /// `>=`
    GreaterEqual,
    /// `instanceof`
    Instanceof,
    /// `in`
    In,
    /// `==`
    Equal,
    /// `!=`
    NotEqual,
    /// `===`
    StrictEqual,
    /// `!==`
    StrictNotEqual,
    /// `&`
    BitAnd,
    /// `^`
    BitXor,
    /// `|`
    BitOr,
}

/// Short-circuit operators (§13.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOp {
    /// `&&`
    And,
    /// `||`
    Or,
    /// `??`
    Coalesce,
}

/// Assignment operators (§13.15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// A compound operator: `+=` and the others.
    Compound(BinaryOp),
    /// `&&=`, `||=`, `??=`: assign only if the short-circuit operator
    /// evaluates its right operand.
    Logical(LogicalOp),
}

/// A property definition of an object literal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Property {
    /// The kind.
    pub kind: PropertyKind,
    /// The key.
    pub key: PropertyKey,
    /// The value: an expression, or for methods an
    /// [`ExprKind::Function`] with [`FunctionKind::Method`] (getters and
    /// setters with their kinds).
    pub value: ExprId,
    /// The source range.
    pub span: Span,
}

/// The kinds of property definitions (§13.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    /// `key: value`.
    Init,
    /// `__proto__: value` (not computed, not shorthand): sets the
    /// prototype (§13.2.5.5, Annex B.3.1).
    Proto,
    /// `name` (shorthand): the value is an [`ExprKind::Identifier`].
    Shorthand,
    /// `key() {}`, `*key() {}`.
    Method,
    /// `get key() {}` (M7).
    Getter,
    /// `set key(v) {}` (M7).
    Setter,
}

/// The key of a property definition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropertyKey {
    /// An identifier name or a string literal.
    Name(NameId),
    /// A numeric literal; the key is its string form.
    Number(f64),
    /// `[expression]`.
    Computed(ExprId),
}

/// A template literal: `quasis.len() == expressions.len() + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    /// The string parts.
    pub quasis: List<TemplateElement>,
    /// The substitutions.
    pub expressions: List<ExprId>,
}

/// A string part of a template literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateElement {
    /// The template value (TV); `None` if the part has an invalid escape
    /// (only possible in tagged templates).
    pub cooked: Option<StringId>,
    /// The template raw value (TRV).
    pub raw: StringId,
}

// --- Patterns ---

/// A binding pattern: the target of a declaration, a parameter or a
/// catch parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// The kind.
    pub kind: PatternKind,
    /// The source range.
    pub span: Span,
}

/// The kinds of binding patterns (§14.3.3). M7 adds array and object
/// patterns, defaults and rest elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternKind {
    /// A `BindingIdentifier`.
    Identifier(Ident),
}

// --- Statements ---

/// A statement node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stmt {
    /// The kind and the children.
    pub kind: StmtKind,
    /// The source range.
    pub span: Span,
}

/// The kinds of statements and declarations (ECMA-262 clause 14).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StmtKind {
    /// `;`
    Empty,
    /// `debugger;`
    Debugger,
    /// An expression statement.
    Expr(ExprId),
    /// A block with its scope. The scope also holds the declarations of
    /// a sloppy-mode `if (x) function f() {}`, which the parser wraps in
    /// a block (Annex B.3.3).
    Block {
        /// The statements.
        body: List<StmtId>,
        /// The block scope.
        scope: ScopeId,
    },
    /// `var`, `let` or `const` declarations.
    Variables {
        /// The declaration kind.
        kind: VariableKind,
        /// The declarators.
        declarators: List<Declarator>,
    },
    /// A function declaration. Its binding is initialized when its scope
    /// is entered ([`crate::ScopeTree::function_declarations`]).
    Function(FunctionId),
    /// `if`.
    If {
        /// The condition.
        test: ExprId,
        /// The statement if the condition is truthy.
        consequent: StmtId,
        /// The `else` statement.
        alternate: Option<StmtId>,
    },
    /// `while`.
    While {
        /// The condition.
        test: ExprId,
        /// The body.
        body: StmtId,
    },
    /// `do ... while`.
    DoWhile {
        /// The body.
        body: StmtId,
        /// The condition.
        test: ExprId,
    },
    /// A three-part `for` loop.
    For {
        /// The initialization: a [`StmtKind::Variables`] or a
        /// [`StmtKind::Expr`].
        init: Option<StmtId>,
        /// The condition.
        test: Option<ExprId>,
        /// The update expression.
        update: Option<ExprId>,
        /// The body.
        body: StmtId,
        /// The scope of `let` and `const` declarations in the head
        /// ([`crate::Scope::per_iteration`] says whether each iteration
        /// copies its cells, §14.7.4.4).
        scope: ScopeId,
    },
    /// A labelled statement.
    Labeled {
        /// The label.
        label: NameId,
        /// The statement.
        body: StmtId,
    },
    /// `break` with an optional label.
    Break {
        /// The label.
        label: Option<NameId>,
    },
    /// `continue` with an optional label.
    Continue {
        /// The label.
        label: Option<NameId>,
    },
    /// `return` (in functions only).
    Return(Option<ExprId>),
    /// `throw`.
    Throw(ExprId),
    /// `switch` (§14.12).
    Switch {
        /// The value that the clauses compare with.
        discriminant: ExprId,
        /// The `case` and `default` clauses in source order.
        cases: List<SwitchCase>,
        /// The scope of the case block (one scope for all clauses).
        scope: ScopeId,
    },
    /// `try` with `catch`, `finally` or both.
    Try {
        /// The `try` block (a [`StmtKind::Block`]).
        block: StmtId,
        /// The `catch` clause.
        handler: Option<CatchClause>,
        /// The `finally` block (a [`StmtKind::Block`]).
        finalizer: Option<StmtId>,
    },
}

/// A clause of a `switch` statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchCase {
    /// The expression of `case`; `None` for `default`.
    pub test: Option<ExprId>,
    /// The statements of the clause.
    pub body: List<StmtId>,
    /// The source range of the clause.
    pub span: Span,
}

/// The `catch` clause of a `try` statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatchClause {
    /// The catch parameter (absent in `catch {}`).
    pub param: Option<PatternId>,
    /// The body, a [`StmtKind::Block`]. Its scope holds the parameter and
    /// the declarations of the block (one scope: §14.15.1 forbids lexical
    /// declarations with the parameter's name).
    pub body: StmtId,
}

/// The kinds of variable declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    /// `var`
    Var,
    /// `let`
    Let,
    /// `const`
    Const,
}

/// One binding of a variable declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declarator {
    /// The binding target.
    pub target: PatternId,
    /// The initializer.
    pub init: Option<ExprId>,
    /// The source range.
    pub span: Span,
}

// --- Functions ---

/// A function, an arrow function, a method, or the top-level code of the
/// script ([`FunctionKind::Script`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// The kind.
    pub kind: FunctionKind,
    /// Whether it is a generator (`function*`).
    pub is_generator: bool,
    /// Whether it is `async` (M7; always false now).
    pub is_async: bool,
    /// Whether its code is strict mode code.
    pub strict: bool,
    /// Whether it is a declaration (its name is bound in the enclosing
    /// scope) rather than an expression (its name is bound in its own
    /// [`crate::ScopeKind::FunctionName`] scope).
    pub is_declaration: bool,
    /// Whether an arrow function has an expression body; the parser
    /// stores it as one `return` statement.
    pub expression_body: bool,
    /// The binding identifier of a declaration or a named expression.
    pub name: Option<Ident>,
    /// The parameters.
    pub params: List<PatternId>,
    /// The body. For the script, the top-level statements.
    pub body: List<StmtId>,
    /// The source text of the function (for `Function.prototype.toString`,
    /// §20.2.3.5): from `function` (or the first parameter token of an
    /// arrow, or the key of a method) to the closing `}`.
    pub span: Span,
    /// The scope of the parameters and the top-level declarations of the
    /// body.
    pub scope: ScopeId,
    /// The function in which this one is defined; `None` for the script.
    pub parent: Option<FunctionId>,
}

/// The kinds of functions (§10.2, §15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    /// The top-level code of a script.
    Script,
    /// A function declaration or expression.
    Normal,
    /// An arrow function: lexical `this` and `arguments`.
    Arrow,
    /// A method of an object literal.
    Method,
    /// A getter (M7).
    Getter,
    /// A setter (M7).
    Setter,
    /// The constructor of a base class (M7).
    ClassConstructor,
    /// The constructor of a derived class (M7).
    DerivedConstructor,
}

impl FunctionKind {
    /// Whether the function has its own `this` and `arguments`.
    pub fn has_own_this(self) -> bool {
        self != FunctionKind::Arrow
    }
}

// --- The arena ---

/// The nodes of one script.
#[derive(Debug, Default)]
pub struct Ast {
    exprs: Vec<Expr>,
    stmts: Vec<Stmt>,
    patterns: Vec<Pattern>,
    functions: Vec<Function>,
    strings: Vec<String16>,
    templates: Vec<Template>,
    expr_lists: Vec<ExprId>,
    stmt_lists: Vec<StmtId>,
    pattern_lists: Vec<PatternId>,
    properties: Vec<Property>,
    declarators: Vec<Declarator>,
    template_elements: Vec<TemplateElement>,
    cases: Vec<SwitchCase>,
}

/// Looks up `id` in `table`. Ids are only created by the table's arena, so
/// a miss means an id of another script: an internal error.
fn lookup<T>(table: &[T], index: usize) -> &T {
    table
        .get(index)
        .expect("an id is used only with the Ast that created it")
}

impl Ast {
    /// The expression `id`.
    pub fn expr(&self, id: ExprId) -> &Expr {
        lookup(&self.exprs, id.index())
    }

    /// The statement `id`.
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        lookup(&self.stmts, id.index())
    }

    /// The pattern `id`.
    pub fn pattern(&self, id: PatternId) -> &Pattern {
        lookup(&self.patterns, id.index())
    }

    /// The function `id`.
    pub fn function(&self, id: FunctionId) -> &Function {
        lookup(&self.functions, id.index())
    }

    /// The string value `id`.
    pub fn string(&self, id: StringId) -> &String16 {
        lookup(&self.strings, id.index())
    }

    /// The template literal `id`.
    pub fn template(&self, id: TemplateId) -> &Template {
        lookup(&self.templates, id.index())
    }

    /// The number of functions (including the script).
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// All function ids, in source order (the script first).
    pub fn function_ids(&self) -> impl Iterator<Item = FunctionId> + use<> {
        (0..self.functions.len()).map(FunctionId::from_index)
    }

    /// The number of expression nodes.
    pub fn expr_count(&self) -> usize {
        self.exprs.len()
    }

    /// The elements of an expression list.
    pub fn exprs(&self, list: List<ExprId>) -> &[ExprId] {
        self.expr_lists.get(list.range()).unwrap_or(&[])
    }

    /// The elements of a statement list.
    pub fn stmts(&self, list: List<StmtId>) -> &[StmtId] {
        self.stmt_lists.get(list.range()).unwrap_or(&[])
    }

    /// The elements of a pattern list.
    pub fn patterns(&self, list: List<PatternId>) -> &[PatternId] {
        self.pattern_lists.get(list.range()).unwrap_or(&[])
    }

    /// The elements of a property list.
    pub fn properties(&self, list: List<Property>) -> &[Property] {
        self.properties.get(list.range()).unwrap_or(&[])
    }

    /// The elements of a declarator list.
    pub fn declarators(&self, list: List<Declarator>) -> &[Declarator] {
        self.declarators.get(list.range()).unwrap_or(&[])
    }

    /// The clauses of a `switch` statement.
    pub fn cases(&self, list: List<SwitchCase>) -> &[SwitchCase] {
        self.cases.get(list.range()).unwrap_or(&[])
    }

    /// The elements of a template element list.
    pub fn template_elements(&self, list: List<TemplateElement>) -> &[TemplateElement] {
        self.template_elements.get(list.range()).unwrap_or(&[])
    }

    /// The approximate heap size of the arena in bytes.
    pub fn heap_size(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.exprs)
            + bytes(&self.stmts)
            + bytes(&self.patterns)
            + bytes(&self.functions)
            + bytes(&self.strings)
            + self.strings.iter().map(|s| s.len() * 2).sum::<usize>()
            + bytes(&self.templates)
            + bytes(&self.expr_lists)
            + bytes(&self.stmt_lists)
            + bytes(&self.pattern_lists)
            + bytes(&self.properties)
            + bytes(&self.declarators)
            + bytes(&self.template_elements)
            + bytes(&self.cases)
    }

    // --- Building (parser only) ---

    pub(crate) fn push_expr(&mut self, kind: ExprKind, span: Span) -> ExprId {
        self.exprs.push(Expr { kind, span });
        ExprId::from_index(self.exprs.len() - 1)
    }

    pub(crate) fn push_stmt(&mut self, kind: StmtKind, span: Span) -> StmtId {
        self.stmts.push(Stmt { kind, span });
        StmtId::from_index(self.stmts.len() - 1)
    }

    pub(crate) fn push_pattern(&mut self, kind: PatternKind, span: Span) -> PatternId {
        self.patterns.push(Pattern { kind, span });
        PatternId::from_index(self.patterns.len() - 1)
    }

    pub(crate) fn push_function(&mut self, function: Function) -> FunctionId {
        self.functions.push(function);
        FunctionId::from_index(self.functions.len() - 1)
    }

    pub(crate) fn function_mut(&mut self, id: FunctionId) -> Option<&mut Function> {
        self.functions.get_mut(id.index())
    }

    pub(crate) fn functions_from(&mut self, start: usize) -> &mut [Function] {
        self.functions.get_mut(start..).unwrap_or(&mut [])
    }

    pub(crate) fn push_string(&mut self, value: String16) -> StringId {
        self.strings.push(value);
        StringId::from_index(self.strings.len() - 1)
    }

    pub(crate) fn push_template(&mut self, template: Template) -> TemplateId {
        self.templates.push(template);
        TemplateId::from_index(self.templates.len() - 1)
    }

    pub(crate) fn push_exprs(&mut self, items: &[ExprId]) -> List<ExprId> {
        push_list(&mut self.expr_lists, items)
    }

    pub(crate) fn push_stmts(&mut self, items: &[StmtId]) -> List<StmtId> {
        push_list(&mut self.stmt_lists, items)
    }

    pub(crate) fn push_patterns(&mut self, items: &[PatternId]) -> List<PatternId> {
        push_list(&mut self.pattern_lists, items)
    }

    pub(crate) fn push_properties(&mut self, items: &[Property]) -> List<Property> {
        push_list(&mut self.properties, items)
    }

    pub(crate) fn push_declarators(&mut self, items: &[Declarator]) -> List<Declarator> {
        push_list(&mut self.declarators, items)
    }

    pub(crate) fn push_cases(&mut self, items: &[SwitchCase]) -> List<SwitchCase> {
        push_list(&mut self.cases, items)
    }

    pub(crate) fn push_template_elements(
        &mut self,
        items: &[TemplateElement],
    ) -> List<TemplateElement> {
        push_list(&mut self.template_elements, items)
    }
}

fn push_list<T: Copy>(pool: &mut Vec<T>, items: &[T]) -> List<T> {
    if items.is_empty() {
        return List::EMPTY;
    }
    let start = pool.len();
    pool.extend_from_slice(items);
    List::new(start, items.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_sizes_stay_small() {
        // The AST of a large bundle has about one expression node per
        // token; keep them compact (memo 2.1).
        assert!(size_of::<ExprKind>() <= 16, "{}", size_of::<ExprKind>());
        assert!(size_of::<Expr>() <= 24, "{}", size_of::<Expr>());
        assert!(size_of::<Stmt>() <= 32, "{}", size_of::<Stmt>());
        assert_eq!(size_of::<Option<ExprId>>(), 4);
    }

    #[test]
    fn lists_are_ranges_in_pools() {
        let mut ast = Ast::default();
        let a = ast.push_expr(ExprKind::Null, Span::new(0, 4));
        let b = ast.push_expr(ExprKind::Boolean(true), Span::new(5, 9));
        let list = ast.push_exprs(&[a, b]);
        assert_eq!(ast.exprs(list), &[a, b]);
        assert_eq!(list.len(), 2);
        assert_eq!(ast.exprs(List::EMPTY), &[] as &[ExprId]);
        assert_eq!(ast.expr(b).span, Span::new(5, 9));
        assert_eq!(format!("{a:?}"), "ExprId(0)");
    }
}
