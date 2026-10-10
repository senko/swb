//! The abstract syntax tree (ADR 0026 section 3).
//!
//! The nodes of one script live in an arena ([`Ast`]): vectors of nodes,
//! addressed by `u32` ids, freed as a whole after the compile. Lists of
//! children ([`List`]) are ranges in shared pools. Positions are
//! code-unit offsets ([`Span`]). Names are [`NameId`]s of the script's
//! [`crate::Interner`]; string values are [`StringId`]s.
//!
//! The node types cover the whole ES2025 grammar. Module code adds the
//! import and export statements; the module records of a module are in
//! [`crate::ModuleRecord`].
//!
//! Classes ([`Class`]) live in a table of their own, with their elements
//! ([`ClassElement`]). The field initializers of a class belong to
//! synthetic functions ([`FunctionKind::InstanceInitializer`],
//! [`FunctionKind::StaticInitializer`]), so that `this`, `super` and the
//! captures inside them follow the rules of functions.
//!
//! Patterns ([`Pattern`]) serve both binding patterns (declarations,
//! parameters, `catch`) and assignment patterns (destructuring
//! assignment): the leaves are [`PatternKind::Identifier`] (a binding) or
//! [`PatternKind::Expr`] (an assignment target).

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
    /// A class in [`Ast::class`].
    ClassId;
    /// A `super(...)` call in [`Ast::super_call`].
    SuperCallId;
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
    /// `object.property`, or `object?.property` in an optional chain.
    Member {
        /// The object.
        object: ExprId,
        /// The property name.
        property: NameId,
        /// `?.`: if the object is `null` or `undefined`, the whole
        /// [`ExprKind::OptionalChain`] around it is `undefined`.
        optional: bool,
    },
    /// `object[index]`, or `object?.[index]` in an optional chain.
    Index {
        /// The object.
        object: ExprId,
        /// The key expression.
        index: ExprId,
        /// `?.[`: see [`ExprKind::Member`].
        optional: bool,
    },
    /// A call. A direct `eval` is a call whose callee is the identifier
    /// `eval`, possibly in parentheses. Arguments can be
    /// [`ExprKind::Spread`].
    Call {
        /// The function.
        callee: ExprId,
        /// The arguments.
        arguments: List<ExprId>,
        /// `callee?.(arguments)`: see [`ExprKind::Member`].
        optional: bool,
    },
    /// `new callee(arguments)`; `new callee` has no arguments. Arguments
    /// can be [`ExprKind::Spread`].
    New {
        /// The constructor.
        callee: ExprId,
        /// The arguments.
        arguments: List<ExprId>,
    },
    /// `yield` with an optional operand (in generator functions), or
    /// `yield* argument`.
    Yield {
        /// The operand.
        argument: Option<ExprId>,
        /// `yield*`: delegate to the iterator of the operand (§15.5.5).
        delegate: bool,
    },
    /// `...argument`: an element of an array literal or an argument of a
    /// call or `new`.
    Spread(ExprId),
    /// An optional chain (§13.3.9): the member accesses and calls of
    /// `a?.b.c()` up to the end of the chain. The links with `optional`
    /// set test their object (or callee) for `null` and `undefined`; if it
    /// is, the value of the whole chain is `undefined` and the rest of the
    /// chain is not evaluated. Parentheses end a chain: `(a?.b).c`.
    OptionalChain(ExprId),
    /// A tagged template: `tag` is called with the template object and
    /// the substitutions (§13.3.11). Its elements can have no cooked
    /// value (an invalid escape).
    TaggedTemplate {
        /// The function.
        tag: ExprId,
        /// The template.
        template: TemplateId,
    },
    /// `new.target` (§13.3.12): the reference resolves to the
    /// `new.target` binding of the nearest non-arrow function, like
    /// [`ExprKind::This`].
    NewTarget(RefId),
    /// A `BigInt` literal: its digits with the radix prefix (`0x`, `0o`,
    /// `0b`), without separators and without the `n`. The value comes
    /// with M7 feature 10.
    BigInt(StringId),
    /// A class expression (§15.7).
    Class(ClassId),
    /// `await argument` (§15.8): in the body of an async function.
    Await(ExprId),
    /// `super.property` (§13.3.7): the property of the prototype of the
    /// home object, with `this` as the receiver. `this` resolves to the
    /// `this` binding and `home` to the home object binding
    /// ([`crate::BindingKind::HomeObject`]) of the nearest non-arrow
    /// function (a method, accessor, constructor or class initializer).
    SuperMember {
        /// The property name.
        property: NameId,
        /// The `this` occurrence.
        this: RefId,
        /// The home object occurrence.
        home: RefId,
    },
    /// `super[index]` (§13.3.7), as [`ExprKind::SuperMember`].
    SuperIndex {
        /// The key expression.
        index: ExprId,
        /// The `this` occurrence.
        this: RefId,
        /// The home object occurrence.
        home: RefId,
    },
    /// `super(arguments)` (§13.3.7.1): only in the constructor of a
    /// derived class and in arrow functions inside it.
    SuperCall(SuperCallId),
    /// `object.#name`, or `object?.#name` in an optional chain
    /// (§13.3.2). The identifier is a private name (its text starts with
    /// `#`); it resolves to the private name binding
    /// ([`crate::BindingKind::PrivateName`]) of a class body scope.
    PrivateMember {
        /// The object.
        object: ExprId,
        /// The private name.
        name: Ident,
        /// `?.#name`: see [`ExprKind::Member`].
        optional: bool,
    },
    /// `#name in object` (§13.10.1): whether the object has the private
    /// name.
    PrivateIn {
        /// The private name.
        name: Ident,
        /// The object.
        object: ExprId,
    },
    /// `import(specifier)` or `import(specifier, options)` (§13.3.10):
    /// loads a module and returns a promise of its namespace object. Also
    /// in scripts.
    ImportCall {
        /// The module specifier.
        specifier: ExprId,
        /// The options object (import attributes, ES2025).
        options: Option<ExprId>,
    },
    /// `import.meta` (§13.3.12): the module's meta object (module code
    /// only).
    ImportMeta,
}

/// A `super(arguments)` call (§13.3.7.1). The occurrences resolve to the
/// implicit bindings of the nearest non-arrow function (a derived
/// constructor): the call constructs the parent class with the function's
/// `new.target`, initializes the `this` binding with the result and runs
/// the field initializers of the active function's class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuperCall {
    /// The arguments; they can be [`ExprKind::Spread`].
    pub arguments: List<ExprId>,
    /// The `this` occurrence (the call initializes the binding).
    pub this: RefId,
    /// The `new.target` occurrence.
    pub new_target: RefId,
    /// The occurrence of the active function binding
    /// ([`crate::BindingKind::ActiveFunction`]): the parent constructor is
    /// its prototype at the time of the call (§13.3.7.2).
    pub function: RefId,
}

/// The target of an assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignTarget {
    /// A simple target: an [`ExprKind::Identifier`], [`ExprKind::Member`],
    /// [`ExprKind::Index`], [`ExprKind::PrivateMember`],
    /// [`ExprKind::SuperMember`] or [`ExprKind::SuperIndex`] (not in an
    /// optional chain), or (in sloppy mode code, `=` and the
    /// arithmetic compound operators only) an [`ExprKind::Call`], which
    /// throws a `ReferenceError` at run time (Chromium's web
    /// compatibility behaviour). Parentheses around the target are
    /// removed.
    Simple(ExprId),
    /// An assignment pattern (destructuring assignment, `=` only;
    /// §13.15.5): an [`PatternKind::Array`] or [`PatternKind::Object`]
    /// whose leaves are [`PatternKind::Expr`].
    Pattern(PatternId),
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
    /// `get key() {}`.
    Getter,
    /// `set key(v) {}`.
    Setter,
    /// `...value` (object spread, §13.2.5.4). The key has no meaning.
    Spread,
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
    /// A `BigInt` literal (the digits as in [`ExprKind::BigInt`]); the
    /// key is the decimal string of its value (M7 feature 10).
    BigInt(StringId),
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

/// A pattern: the target of a declaration, a parameter or a catch
/// parameter (a binding pattern, §14.3.3), or of a destructuring
/// assignment (an assignment pattern, §13.15.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// The kind.
    pub kind: PatternKind,
    /// The source range.
    pub span: Span,
}

/// The kinds of patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternKind {
    /// A `BindingIdentifier`: a binding of a binding pattern.
    Identifier(Ident),
    /// A target of an assignment pattern: an [`ExprKind::Identifier`] or
    /// a property access ([`ExprKind::Member`], [`ExprKind::Index`],
    /// [`ExprKind::PrivateMember`], [`ExprKind::SuperMember`],
    /// [`ExprKind::SuperIndex`]); parentheses removed.
    Expr(ExprId),
    /// `[a, , b = 1, ...c]`: the elements in order; an elision is a
    /// [`PatternKind::Hole`], a rest element a [`PatternKind::Rest`] (only
    /// the last element).
    Array(List<PatternId>),
    /// `{ a, b: c, [d]: e = 1, ...f }`.
    Object {
        /// The properties in order.
        properties: List<PatternProperty>,
        /// The rest element's target: a [`PatternKind::Identifier`] or a
        /// [`PatternKind::Expr`].
        rest: Option<PatternId>,
    },
    /// An element with an initializer: `target = value`; the value is
    /// used when the element is `undefined`.
    Default {
        /// The target.
        target: PatternId,
        /// The initializer.
        value: ExprId,
    },
    /// `...target`: the last element of an array pattern or of a
    /// parameter list.
    Rest(PatternId),
    /// An elision in an array pattern.
    Hole,
}

/// A property of an object pattern. A shorthand (`{ a }`, `{ a = 1 }`)
/// has the key `a` and the value `a` (with its default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternProperty {
    /// The key.
    pub key: PropertyKey,
    /// The target, possibly with a [`PatternKind::Default`].
    pub value: PatternId,
    /// The source range.
    pub span: Span,
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
    /// `for (left in right) body` (§14.7.5).
    ForIn {
        /// The declaration or the assignment target.
        left: ForHead,
        /// The object whose keys the loop visits.
        right: ExprId,
        /// The body.
        body: StmtId,
        /// The scope of a `let` or `const` declaration in the head. The
        /// right side is evaluated in it, with the bindings uninitialized
        /// (`for (let x in x)` throws, §14.7.5.6); each iteration creates
        /// new cells for the bindings (§14.7.5.7).
        scope: ScopeId,
    },
    /// `for (left of right) body` (§14.7.5); the scope as in
    /// [`StmtKind::ForIn`].
    ForOf {
        /// The declaration or the assignment target.
        left: ForHead,
        /// The iterable.
        right: ExprId,
        /// The body.
        body: StmtId,
        /// The scope of a `let` or `const` declaration in the head.
        scope: ScopeId,
        /// `for await (left of right)`: iterates an async iterator (in
        /// async functions only).
        is_await: bool,
    },
    /// A class declaration (§15.7). Its binding is a lexical binding of
    /// the enclosing scope ([`crate::BindingKind::Class`]), initialized
    /// when the declaration runs.
    Class(ClassId),
    /// `with (object) body` (sloppy mode code only, §14.11).
    With {
        /// The object of the object environment.
        object: ExprId,
        /// The body.
        body: StmtId,
        /// The scope of the body ([`crate::ScopeKind::With`]): names
        /// inside it resolve through the object first
        /// ([`crate::DynamicLookup`]).
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
    /// An import declaration (§16.2.2): no code. Its bindings are
    /// created when the module is linked ([`crate::ModuleRecord`]).
    Import,
    /// `export` with a declaration (§16.2.3): the declaration (a
    /// [`StmtKind::Variables`], [`StmtKind::Function`] or
    /// [`StmtKind::Class`]), which runs as usual. `export default
    /// function` and `export default class` are declarations too; without
    /// a name, the function's name is `*default*` (its `name` property is
    /// "default"), and the class has no name and binds `*default*`
    /// ([`Class::declaration`]).
    ExportDeclaration(StmtId),
    /// `export default AssignmentExpression;`: evaluates the value (an
    /// anonymous function or class gets the name "default") and
    /// initializes the module's binding `*default*` (the occurrence
    /// `binding`).
    ExportDefault {
        /// The value.
        value: ExprId,
        /// The declaration occurrence of `*default*`.
        binding: RefId,
    },
    /// The other export forms (`export { a as b }`, `export * from "m"`,
    /// `export { a } from "m"`): no code.
    ExportList,
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

/// The left side of a `for`-`in` or `for`-`of` statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForHead {
    /// `var`, `let` or `const` with one declarator: a
    /// [`StmtKind::Variables`]. Only `for (var x = init in o)` in sloppy
    /// mode code has an initializer (Annex B.3.5).
    Declaration(StmtId),
    /// An assignment target or pattern; in sloppy mode code also a call
    /// (a `ReferenceError` at run time, as for `f() = 1`).
    Target(AssignTarget),
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
    /// Whether it is a generator (`function*`, `*m() {}`).
    pub is_generator: bool,
    /// Whether it is `async` (an async function, arrow function or
    /// method; with `is_generator` an async generator).
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
    /// The parameters: one pattern per element of the parameter list; a
    /// rest parameter is a [`PatternKind::Rest`] (the last).
    pub params: List<PatternId>,
    /// The body. For the script, the top-level statements.
    pub body: List<StmtId>,
    /// The source text of the function (for `Function.prototype.toString`,
    /// §20.2.3.5): from `function` (or the first parameter token of an
    /// arrow, or the key of a method) to the closing `}`.
    pub span: Span,
    /// The scope of the parameters and (see `body_scope`) the top-level
    /// declarations of the body.
    pub scope: ScopeId,
    /// The scope of the top-level declarations of the body: the same as
    /// `scope`, except for a function with parameter expressions
    /// (defaults or computed keys in the parameters), whose body has a
    /// [`crate::ScopeKind::FunctionBody`] scope of its own (§10.2.11
    /// step 28).
    pub body_scope: ScopeId,
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
    /// A method of an object literal or a class (it has a home object
    /// for `super` properties).
    Method,
    /// A getter of an object literal or a class.
    Getter,
    /// A setter of an object literal or a class.
    Setter,
    /// The constructor of a base class.
    ClassConstructor,
    /// The constructor of a derived class (`class C extends B`): `this`
    /// is uninitialized until `super(...)` returns.
    DerivedConstructor,
    /// The synthetic function of a class that runs the initializers of
    /// its instance fields ([`Class::instance_init`]): `this` is the new
    /// object, the home object is the class prototype, `new.target` is
    /// `undefined`. It has no parameters and no statements; the
    /// initializers are the values of the field elements.
    InstanceInitializer,
    /// The synthetic function of a class that runs its static field
    /// initializers and static blocks in source order
    /// ([`Class::static_init`]): `this` and the home object are the
    /// class constructor. Each static block has a var scope of its own
    /// ([`crate::ScopeKind::StaticBlock`]).
    StaticInitializer,
    /// The top-level code of direct eval code ([`crate::parse_eval`]):
    /// `this`, `new.target`, `arguments` and `super` are the caller's
    /// ([`crate::Resolution::Caller`]). Indirect eval code is a
    /// [`FunctionKind::Script`] whose top scope is a
    /// [`crate::ScopeKind::Eval`].
    Eval,
    /// The top-level code of a module ([`crate::parse_module`]): strict,
    /// `this` is `undefined`, `await` is allowed.
    Module,
}

impl FunctionKind {
    /// Whether the function has its own `this` and `arguments`.
    pub fn has_own_this(self) -> bool {
        !matches!(self, FunctionKind::Arrow | FunctionKind::Eval)
    }

    /// Whether the function is a method-like function with a home object
    /// (`super` properties are allowed, §15.4).
    pub fn has_home_object(self) -> bool {
        matches!(
            self,
            FunctionKind::Method
                | FunctionKind::Getter
                | FunctionKind::Setter
                | FunctionKind::ClassConstructor
                | FunctionKind::DerivedConstructor
                | FunctionKind::InstanceInitializer
                | FunctionKind::StaticInitializer
        )
    }
}

// --- Classes ---

/// A class declaration or expression (§15.7). Class code is strict mode
/// code.
///
/// Scopes: `scope` ([`crate::ScopeKind::Class`]) holds the inner binding
/// of the name, which the heritage, the computed keys and the methods
/// see (immutable, uninitialized until the class is defined). Its child
/// `body_scope` ([`crate::ScopeKind::ClassBody`]) holds the private names
/// of the class ([`crate::BindingKind::PrivateName`]), created when the
/// class definition starts. The heritage is evaluated in `scope` (it
/// cannot see the private names), the computed keys in `body_scope`; the
/// scopes of the methods and of the initializer functions are children of
/// `body_scope`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Class {
    /// The binding identifier, if any.
    pub name: Option<NameId>,
    /// For a declaration: the occurrence of the name that initializes the
    /// binding in the enclosing scope.
    pub declaration: Option<RefId>,
    /// For a named class: the occurrence of the name that initializes the
    /// inner binding in `scope`.
    pub inner: Option<RefId>,
    /// The `extends` expression (a `LeftHandSideExpression`).
    pub heritage: Option<ExprId>,
    /// The `constructor` method ([`FunctionKind::ClassConstructor`] or
    /// [`FunctionKind::DerivedConstructor`]); `None` for the default
    /// constructor (§15.7.14 step 14).
    pub constructor: Option<FunctionId>,
    /// The other elements, in source order.
    pub elements: List<ClassElement>,
    /// The scope of the inner name binding.
    pub scope: ScopeId,
    /// The scope of the private names.
    pub body_scope: ScopeId,
    /// The function that initializes the instance fields: present if the
    /// class has an instance field.
    pub instance_init: Option<FunctionId>,
    /// The function that runs the static fields and static blocks:
    /// present if the class has one.
    pub static_init: Option<FunctionId>,
    /// The source text of the class (for `Function.prototype.toString` of
    /// the constructor): from `class` to the closing `}`.
    pub span: Span,
}

/// An element of a class body other than the constructor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassElement {
    /// The kind.
    pub kind: ClassElementKind,
    /// Whether the element is `static`.
    pub is_static: bool,
    /// The name; [`ClassKey::StaticBlock`] for a static block.
    pub key: ClassKey,
    /// For a method or an accessor: an [`ExprKind::Function`]. For a field:
    /// the initializer, an expression of the class's initializer function
    /// (`instance_init` or `static_init`). `None` for a field without an
    /// initializer and for a static block.
    pub value: Option<ExprId>,
    /// The source range.
    pub span: Span,
}

/// The kinds of class elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassElementKind {
    /// A method (also a generator, async or async generator method).
    Method,
    /// `get name() {}`.
    Getter,
    /// `set name(v) {}`.
    Setter,
    /// A field: `name` or `name = value`.
    Field,
    /// `static { ... }`: statements of the class's `static_init` function,
    /// in a [`crate::ScopeKind::StaticBlock`] scope.
    StaticBlock {
        /// The statements.
        body: List<StmtId>,
        /// The var scope of the block.
        scope: ScopeId,
    },
}

/// The name of a class element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClassKey {
    /// A property name (as in object literals).
    Property(PropertyKey),
    /// A private name (`#name`; the text includes the `#`). The
    /// occurrence resolves to the private name binding of the class body
    /// scope.
    Private(Ident),
    /// No name: a static block.
    StaticBlock,
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
    pattern_properties: Vec<PatternProperty>,
    properties: Vec<Property>,
    declarators: Vec<Declarator>,
    template_elements: Vec<TemplateElement>,
    cases: Vec<SwitchCase>,
    classes: Vec<Class>,
    class_elements: Vec<ClassElement>,
    super_calls: Vec<SuperCall>,
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

    /// The class `id`.
    pub fn class(&self, id: ClassId) -> &Class {
        lookup(&self.classes, id.index())
    }

    /// The `super(...)` call `id`.
    pub fn super_call(&self, id: SuperCallId) -> &SuperCall {
        lookup(&self.super_calls, id.index())
    }

    /// The elements of a class.
    pub fn class_elements(&self, list: List<ClassElement>) -> &[ClassElement] {
        self.class_elements.get(list.range()).unwrap_or(&[])
    }

    /// All class ids, in the order in which their parse ended (inner
    /// classes before outer ones).
    pub fn class_ids(&self) -> impl Iterator<Item = ClassId> + use<> {
        (0..self.classes.len()).map(ClassId::from_index)
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

    /// All expression ids, in creation order.
    pub fn expr_ids(&self) -> impl Iterator<Item = ExprId> + use<> {
        (0..self.exprs.len()).map(ExprId::from_index)
    }

    /// All statement ids, in creation order.
    pub fn stmt_ids(&self) -> impl Iterator<Item = StmtId> + use<> {
        (0..self.stmts.len()).map(StmtId::from_index)
    }

    /// All pattern ids, in creation order.
    pub fn pattern_ids(&self) -> impl Iterator<Item = PatternId> + use<> {
        (0..self.patterns.len()).map(PatternId::from_index)
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

    /// The properties of an object pattern.
    pub fn pattern_properties(&self, list: List<PatternProperty>) -> &[PatternProperty] {
        self.pattern_properties.get(list.range()).unwrap_or(&[])
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
            + bytes(&self.pattern_properties)
            + bytes(&self.properties)
            + bytes(&self.declarators)
            + bytes(&self.template_elements)
            + bytes(&self.cases)
            + bytes(&self.classes)
            + bytes(&self.class_elements)
            + bytes(&self.super_calls)
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

    pub(crate) fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.get_mut(id.index())
    }

    pub(crate) fn push_function(&mut self, function: Function) -> FunctionId {
        self.functions.push(function);
        FunctionId::from_index(self.functions.len() - 1)
    }

    pub(crate) fn function_mut(&mut self, id: FunctionId) -> Option<&mut Function> {
        self.functions.get_mut(id.index())
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

    pub(crate) fn push_pattern_properties(
        &mut self,
        items: &[PatternProperty],
    ) -> List<PatternProperty> {
        push_list(&mut self.pattern_properties, items)
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

    pub(crate) fn push_class(&mut self, class: Class) -> ClassId {
        self.classes.push(class);
        ClassId::from_index(self.classes.len() - 1)
    }

    pub(crate) fn push_class_elements(&mut self, items: &[ClassElement]) -> List<ClassElement> {
        push_list(&mut self.class_elements, items)
    }

    pub(crate) fn push_super_call(&mut self, call: SuperCall) -> SuperCallId {
        self.super_calls.push(call);
        SuperCallId::from_index(self.super_calls.len() - 1)
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
