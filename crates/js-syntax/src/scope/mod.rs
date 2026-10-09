//! Scopes, bindings and identifier occurrences (ADR 0026 section 3).
//!
//! The parser records the declarations of each scope as it parses, and
//! checks the redeclaration early errors (§14.2.1, §14.3.1.1, §15.2.1,
//! §16.1.1 and Annex B.3.2.4, B.3.4) at the declaration. It records
//! every identifier occurrence with its innermost scope. After the parse,
//! the scope analysis ([`analysis`]) resolves each occurrence, finds the
//! captured bindings and assigns storage: registers, cells or global
//! access by name.
//!
//! Scope model:
//!
//! - [`ScopeKind::Script`]: top-level declarations. `var` and function
//!   declarations are global object properties, `let` and `const` are
//!   global lexical bindings; both are accessed by name (§9.1.1.4).
//! - [`ScopeKind::Function`]: the parameters, `var` and function
//!   declarations and the top-level lexical declarations of a function
//!   body. (Parameter expressions, M7, need a separate scope for the
//!   parameters, §10.2.11 step 28.)
//! - [`ScopeKind::FunctionName`]: the name of a named function
//!   expression, between the enclosing scope and the function's scope.
//! - [`ScopeKind::Block`], [`ScopeKind::Catch`] (the catch parameter and
//!   the declarations of the catch block), [`ScopeKind::For`] (the
//!   lexical declarations of a `for` head).
//!
//! Contract for the compiler:
//!
//! - Scope entry initializes the scope's storage: it creates the cells
//!   ([`Scope::has_cells`]), initializes the function declarations
//!   ([`ScopeTree::function_declarations`]) and writes the uninitialized
//!   marker (temporal dead zone) into the register or the new cell of
//!   each binding with [`Binding::needs_tdz`]. Registers are reused
//!   between sibling blocks, so the marker must be written on every
//!   entry, also when the register is reused. Bindings without the flag
//!   need no marker; a `let x;` without initializer stores `undefined`
//!   at the declaration.
//! - [`FunctionScope::mapped_arguments`]: the parameters are cells; the
//!   arguments object holds them.
//! - The analysis keeps [`analysis::TEMPORARIES_RESERVE`] registers free
//!   of declared bindings, but the compiler must check the total register
//!   count of a function (declared bindings and temporaries) against the
//!   limit of 65,535 again and report the same error.
//! - Resource limits (`RangeError` from the parse): the total number of
//!   capture entries of a script and the captures of one function, see
//!   [`analysis::Limits`].
//!
//! Block-level function declarations follow the strict-mode semantics in
//! sloppy mode too (a lexical binding of the block); the Annex B.3.2
//! semantics come in M7. Duplicate block-level function declarations in
//! sloppy mode are allowed (B.3.2.4).

pub(crate) mod analysis;

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use crate::ast::{BindingId, FunctionId, List, RefId, ScopeId};
use crate::interner::NameId;

/// The kinds of scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// The top level of a script.
    Script,
    /// The parameters and the body of a function.
    Function,
    /// The binding of a named function expression's own name.
    FunctionName,
    /// A block statement.
    Block,
    /// A `catch` clause: the parameter and the block.
    Catch,
    /// The head of a `for` statement with `let` or `const`.
    For,
}

/// A scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The kind.
    pub kind: ScopeKind,
    /// The enclosing scope; `None` for the script scope.
    pub parent: Option<ScopeId>,
    /// The function whose frame holds the bindings of this scope.
    pub function: FunctionId,
    /// The offset where the scope starts.
    pub start: u32,
    /// The bindings of the scope, in declaration order (after the
    /// analysis).
    pub bindings: List<BindingId>,
    /// The function declarations to initialize when the scope is entered,
    /// in source order (the last one of a name wins).
    pub functions: List<FunctionId>,
    /// Whether some binding of the scope is a cell (cells are created
    /// when the scope is entered).
    pub has_cells: bool,
    /// A `for` scope whose `let` bindings are cells: the loop copies them
    /// into new cells (`CreatePerIterationEnvironment`, §14.7.4.4) before
    /// the first test and before every update. A closure created in the
    /// head keeps the cells of the initialization (`for (let i = 0, g =
    /// () => i; i < 1; i++) { i = 5 }`: `g()` returns 0). The cells of
    /// the scope are created at scope entry as usual; the copies are the
    /// compiler's job, with the registers that [`Binding::storage`] names.
    pub per_iteration: bool,
    /// The number of bindings declared so far (for a fast skip during
    /// resolution).
    binding_count: u32,
    /// The nearest enclosing function or script scope (or the scope
    /// itself): where `var` declarations of this scope hoist to.
    hoist_to: ScopeId,
}

/// The kinds of bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// `var`.
    Var,
    /// `let`: temporal dead zone until its declaration runs.
    Let,
    /// `const`: temporal dead zone, immutable.
    Const,
    /// A function declaration: initialized when its scope is entered.
    Function,
    /// A parameter.
    Parameter,
    /// A catch parameter.
    CatchParameter,
    /// The name of a named function expression: the function itself,
    /// immutable (assignment is ignored in sloppy mode and a `TypeError`
    /// in strict mode).
    FunctionName,
    /// The `arguments` object of a non-arrow function (implicit, or a
    /// `var arguments`), created at function entry.
    Arguments,
    /// The `this` value of a non-arrow function or of the script
    /// (implicit; its name is `this`), stored at function entry.
    This,
}

impl BindingKind {
    /// Whether the binding has a temporal dead zone.
    pub fn is_lexical(self) -> bool {
        matches!(self, BindingKind::Let | BindingKind::Const)
    }
}

/// Where a binding lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Not assigned yet (before the analysis).
    Unassigned,
    /// A register of the function's frame.
    Register(u16),
    /// A cell whose handle is in a register of the function's frame. The
    /// cell is created when the scope is entered (for parameters: at
    /// function entry, with the argument value).
    Cell(u16),
    /// A global binding, accessed by name: the declarative part of the
    /// global environment, then the global object (§9.1.1.4).
    Global,
}

/// A binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The name (`this` for the implicit `this` binding).
    pub name: NameId,
    /// The kind.
    pub kind: BindingKind,
    /// The scope.
    pub scope: ScopeId,
    /// The offset of the binding identifier (of the first declaration),
    /// or of the scope for implicit bindings.
    pub offset: u32,
    /// For `let` and `const`: the offset after the declarator; a load in
    /// the same function after it needs no TDZ check.
    pub init_end: u32,
    /// Whether an inner function refers to it (or a direct `eval` can).
    pub captured: bool,
    /// For `let`, `const` (and `class`, M7): whether the binding can be
    /// read before its initialization, so that the compiler must write
    /// the uninitialized marker at scope entry. True when any reference
    /// has `tdz_check`, or a closure or a direct `eval` can read it.
    /// Registers are reused between sibling blocks, so a register that
    /// holds a stale value from an earlier block must be reset at scope
    /// entry, and a cell must be created in the uninitialized state. A
    /// binding without the flag needs no marker: every read follows its
    /// declarator in the same function.
    pub needs_tdz: bool,
    /// The storage.
    pub storage: Storage,
}

/// How an identifier occurrence resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Not resolved yet (before the analysis).
    Unresolved,
    /// A register of the current function.
    Register(u16),
    /// A cell of the current function; the register holds its handle.
    Cell(u16),
    /// A cell of an enclosing function: the index in the current
    /// function's captures ([`FunctionScope::captures`]).
    Capture(u32),
    /// A global binding, accessed by name.
    Global,
}

/// An identifier occurrence: a reference, or the binding identifier of a
/// declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reference {
    /// The name.
    pub name: NameId,
    /// The innermost scope at the occurrence.
    pub scope: ScopeId,
    /// The offset.
    pub offset: u32,
    /// Whether it is the binding identifier of a declaration (the
    /// compiler initializes the binding there).
    pub declaration: bool,
    /// The binding, or `None` for a global binding or an unresolvable
    /// name.
    pub binding: Option<BindingId>,
    /// How it resolves.
    pub resolution: Resolution,
    /// Whether a load must check the temporal dead zone.
    pub tdz_check: bool,
}

/// Where a function gets one of its captured cells when its closure is
/// created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureSource {
    /// A cell of the enclosing function, whose handle is in this register.
    ParentRegister(u16),
    /// A capture of the enclosing function, by index.
    ParentCapture(u32),
}

/// A captured cell of a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capture {
    /// The binding.
    pub binding: BindingId,
    /// Where the closure gets it.
    pub source: CaptureSource,
}

/// The results of the scope analysis for one function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionScope {
    /// The binding of each parameter position (duplicate names in sloppy
    /// mode share a binding, whose register is the last position).
    pub params: Vec<BindingId>,
    /// The registers of the declared bindings: the parameters first
    /// (registers `0..params.len()`), then the other bindings. The
    /// compiler allocates temporaries after these.
    pub register_count: u32,
    /// The cells of enclosing functions that the function uses (flat
    /// closures: a cell several levels up is captured by every function
    /// in between).
    pub captures: Vec<Capture>,
    /// The `this` binding, if the function or an arrow function inside it
    /// uses `this`.
    pub this_binding: Option<BindingId>,
    /// The `arguments` binding, if the function or an arrow function
    /// inside it uses `arguments`: the function creates the object at
    /// entry.
    pub arguments_binding: Option<BindingId>,
    /// Whether the function has a mapped `arguments` object (sloppy mode,
    /// simple parameters, an `arguments` binding): all its parameters are
    /// cells, which the object holds.
    ///
    /// Two cases for the compiler: with duplicate parameter names
    /// (`function f(a, a)`), only the last parameter of a name is mapped
    /// (§10.4.4.7), so earlier indices hold plain values. A sloppy function
    /// with a direct `eval` but no `arguments` reference has this flag
    /// false, yet `eval` code can read `arguments`: its parameters are
    /// cells (scope description), and the compiler must create the mapped
    /// object for it too.
    pub mapped_arguments: bool,
    /// Whether the function contains a direct call of `eval` (not in a
    /// nested function).
    pub has_direct_eval: bool,
    /// Whether the function or a nested one has a direct `eval`: the
    /// function keeps a scope description for the eval code, and all its
    /// bindings are cells (ADR 0026 section 3, "Dynamic scope"). The
    /// eval semantics come in M7.
    pub needs_scope_description: bool,
}

/// A fast hasher for the `u64` keys of the scope maps (scope id and name
/// id). The keys are dense ids that a script cannot choose freely, so a
/// multiplicative hash is enough.
#[derive(Default)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(byte)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (value ^ (value >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub(crate) type IdMap<V> = HashMap<u64, V, BuildHasherDefault<IdHasher>>;

/// The map key of a name in a scope.
fn key(scope: ScopeId, name: NameId) -> u64 {
    ((scope.index() as u64) << 32) | u64::from(name.index())
}

/// A redeclaration early error: the name is already declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Redeclared;

/// The scopes, bindings and identifier occurrences of a script.
#[derive(Debug, Default)]
pub struct ScopeTree {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    references: Vec<Reference>,
    functions: Vec<FunctionScope>,
    /// The declared names of each scope.
    declared: IdMap<BindingId>,
    /// Per (function, name): how many open block scopes declare the name
    /// other than as a catch parameter. A `var` of the name inside them
    /// is an early error (§14.2.1, Annex B.3.4).
    open_lexical: IdMap<u32>,
    /// The entries of `open_lexical` to undo when a scope closes, in the
    /// order of declaration: (scope, key).
    open_lexical_log: Vec<(ScopeId, u64)>,
    /// Per (function, name): the highest scope id in which a `var` of the
    /// name was declared. A scope that is still open contains all scopes
    /// created after it, so a lexical declaration of the name in it
    /// conflicts exactly when this id is not lower than its own.
    last_var_scope: IdMap<ScopeId>,
    /// Function declarations: (scope, function), in source order.
    function_decls: Vec<(ScopeId, FunctionId)>,
    /// The pools of [`Scope::bindings`] and [`Scope::functions`].
    scope_bindings: Vec<BindingId>,
    scope_functions: Vec<FunctionId>,
}

fn lookup<T>(table: &[T], index: usize) -> &T {
    table
        .get(index)
        .expect("an id is used only with the ScopeTree that created it")
}

impl ScopeTree {
    /// The scope `id`.
    pub fn scope(&self, id: ScopeId) -> &Scope {
        lookup(&self.scopes, id.index())
    }

    /// The binding `id`.
    pub fn binding(&self, id: BindingId) -> &Binding {
        lookup(&self.bindings, id.index())
    }

    /// The identifier occurrence `id`.
    pub fn reference(&self, id: RefId) -> &Reference {
        lookup(&self.references, id.index())
    }

    /// The analysis results of function `id`.
    pub fn function(&self, id: FunctionId) -> &FunctionScope {
        lookup(&self.functions, id.index())
    }

    /// The bindings of a scope.
    pub fn bindings_of(&self, scope: ScopeId) -> &[BindingId] {
        let list = self.scope(scope).bindings;
        self.scope_bindings.get(list.range()).unwrap_or(&[])
    }

    /// The function declarations that a scope initializes on entry.
    pub fn function_declarations(&self, scope: ScopeId) -> &[FunctionId] {
        let list = self.scope(scope).functions;
        self.scope_functions.get(list.range()).unwrap_or(&[])
    }

    /// The number of scopes.
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// The number of bindings.
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// The number of identifier occurrences.
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    /// The approximate heap size in bytes.
    pub fn heap_size(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.scopes)
            + bytes(&self.bindings)
            + bytes(&self.references)
            + bytes(&self.functions)
            + self
                .functions
                .iter()
                .map(|f| f.params.capacity() * 4 + f.captures.capacity() * size_of::<Capture>())
                .sum::<usize>()
            + self.declared.capacity() * 16
            + self.open_lexical.capacity() * 16
            + self.last_var_scope.capacity() * 16
            + bytes(&self.open_lexical_log)
            + bytes(&self.function_decls)
            + bytes(&self.scope_bindings)
            + bytes(&self.scope_functions)
    }

    // --- Building (parser) ---

    /// Adds the analysis record of a new function.
    pub(crate) fn add_function(&mut self) {
        self.functions.push(FunctionScope::default());
    }

    pub(crate) fn function_mut(&mut self, id: FunctionId) -> Option<&mut FunctionScope> {
        self.functions.get_mut(id.index())
    }

    /// Opens a scope.
    pub(crate) fn push_scope(
        &mut self,
        kind: ScopeKind,
        parent: Option<ScopeId>,
        function: FunctionId,
        start: u32,
    ) -> ScopeId {
        let id = ScopeId::from_index(self.scopes.len());
        let hoist_to = match (kind, parent) {
            (ScopeKind::Function | ScopeKind::Script, _) | (_, None) => id,
            (_, Some(parent)) => self.scope(parent).hoist_to,
        };
        self.scopes.push(Scope {
            kind,
            parent,
            function,
            start,
            bindings: List::EMPTY,
            functions: List::EMPTY,
            has_cells: false,
            per_iteration: false,
            binding_count: 0,
            hoist_to,
        });
        id
    }

    /// Closes the innermost scope `scope`: its lexical names no longer
    /// conflict with a `var` of the enclosing code.
    pub(crate) fn close_scope(&mut self, scope: ScopeId) {
        while let Some(&(owner, key)) = self.open_lexical_log.last() {
            if owner != scope {
                break;
            }
            self.open_lexical_log.pop();
            if let Some(count) = self.open_lexical.get_mut(&key) {
                *count = count.saturating_sub(1);
            }
        }
    }

    pub(crate) fn scope_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
        self.scopes.get_mut(id.index())
    }

    pub(crate) fn binding_mut(&mut self, id: BindingId) -> Option<&mut Binding> {
        self.bindings.get_mut(id.index())
    }

    /// Records an identifier occurrence.
    pub(crate) fn push_reference(
        &mut self,
        name: NameId,
        scope: ScopeId,
        offset: u32,
        declaration: bool,
    ) -> RefId {
        self.references.push(Reference {
            name,
            scope,
            offset,
            declaration,
            binding: None,
            resolution: Resolution::Unresolved,
            tdz_check: false,
        });
        RefId::from_index(self.references.len() - 1)
    }

    pub(crate) fn reference_mut(&mut self, id: RefId) -> Option<&mut Reference> {
        self.references.get_mut(id.index())
    }

    /// The number of references, scopes and functions so far (marks for
    /// the arrow function cover grammar).
    pub(crate) fn marks(&self) -> (usize, usize) {
        (self.references.len(), self.scopes.len())
    }

    /// Moves what the parser recorded since `marks` in scope `from` into
    /// scope `to`: the occurrences and the child scopes of an arrow
    /// function's parameters, which were parsed as an expression before
    /// the `=>` (the cover grammar, §15.3).
    pub(crate) fn reparent_since(&mut self, marks: (usize, usize), from: ScopeId, to: ScopeId) {
        let (references, scopes) = marks;
        for reference in self.references.get_mut(references..).unwrap_or(&mut []) {
            if reference.scope == from {
                reference.scope = to;
            }
        }
        let var_scope = self.scope(to).hoist_to;
        for (index, scope) in self.scopes.iter_mut().enumerate().skip(scopes) {
            if scope.parent == Some(from) && index != to.index() {
                scope.parent = Some(to);
                if !matches!(scope.kind, ScopeKind::Function | ScopeKind::Script) {
                    scope.hoist_to = var_scope;
                }
            }
        }
    }

    fn new_binding(
        &mut self,
        scope: ScopeId,
        name: NameId,
        kind: BindingKind,
        offset: u32,
    ) -> BindingId {
        self.bindings.push(Binding {
            name,
            kind,
            scope,
            offset,
            init_end: offset,
            captured: false,
            needs_tdz: false,
            storage: Storage::Unassigned,
        });
        let id = BindingId::from_index(self.bindings.len() - 1);
        self.declared.insert(key(scope, name), id);
        if let Some(scope) = self.scopes.get_mut(scope.index()) {
            scope.binding_count += 1;
        }
        id
    }

    fn declared_in(&self, scope: ScopeId, name: NameId) -> Option<BindingId> {
        self.declared.get(&key(scope, name)).copied()
    }

    fn is_var_scope(&self, scope: ScopeId) -> bool {
        matches!(
            self.scope(scope).kind,
            ScopeKind::Function | ScopeKind::Script
        )
    }

    /// The key of a name in a function, for the early error tables.
    fn function_key(&self, scope: ScopeId, name: NameId) -> u64 {
        ((self.scope(scope).function.index() as u64) << 32) | u64::from(name.index())
    }

    /// Whether a `var` of `name` was declared inside `scope`, which is
    /// still open (so everything created since it opened is inside it).
    fn var_declared_inside(&self, scope: ScopeId, name: NameId) -> bool {
        self.last_var_scope
            .get(&self.function_key(scope, name))
            .is_some_and(|last| last.index() >= scope.index())
    }

    /// Notes that block `scope` declares `name` lexically.
    fn note_lexical(&mut self, scope: ScopeId, name: NameId) {
        if self.is_var_scope(scope) {
            return;
        }
        let key = self.function_key(scope, name);
        *self.open_lexical.entry(key).or_insert(0) += 1;
        self.open_lexical_log.push((scope, key));
    }

    /// Declares a `let` or `const` binding in `scope`.
    pub(crate) fn declare_lexical(
        &mut self,
        scope: ScopeId,
        name: NameId,
        kind: BindingKind,
        offset: u32,
    ) -> Result<BindingId, Redeclared> {
        if self.declared_in(scope, name).is_some() || self.var_declared_inside(scope, name) {
            return Err(Redeclared);
        }
        self.note_lexical(scope, name);
        Ok(self.new_binding(scope, name, kind, offset))
    }

    /// Declares a `var` binding from `scope`: it hoists to the nearest
    /// function or script scope, through blocks that must not declare the
    /// name lexically (a catch parameter may have it, Annex B.3.4).
    /// Returns the binding (an existing one if the function already has
    /// the name).
    ///
    /// The work is constant per call: the blocks in between are checked
    /// through counters (`open_lexical`), not by walking them.
    pub(crate) fn declare_var(
        &mut self,
        scope: ScopeId,
        name: NameId,
        offset: u32,
    ) -> Result<BindingId, Redeclared> {
        let key = self.function_key(scope, name);
        if self.open_lexical.get(&key).is_some_and(|&count| count > 0) {
            return Err(Redeclared);
        }
        let var_scope = self.scope(scope).hoist_to;
        let binding = match self.declared_in(var_scope, name) {
            Some(existing) if self.binding(existing).kind.is_lexical() => {
                return Err(Redeclared);
            }
            Some(existing) => existing,
            None => self.new_binding(var_scope, name, BindingKind::Var, offset),
        };
        let last = self.last_var_scope.entry(key).or_insert(scope);
        if last.index() < scope.index() {
            *last = scope;
        }
        Ok(binding)
    }

    /// Declares a parameter in a function scope. Returns the binding and
    /// whether the name repeats an earlier parameter.
    pub(crate) fn declare_param(
        &mut self,
        scope: ScopeId,
        name: NameId,
        offset: u32,
    ) -> (BindingId, bool) {
        match self.declared_in(scope, name) {
            Some(existing) => (existing, true),
            None => (
                self.new_binding(scope, name, BindingKind::Parameter, offset),
                false,
            ),
        }
    }

    /// Declares a catch parameter in a fresh catch scope.
    pub(crate) fn declare_catch_param(
        &mut self,
        scope: ScopeId,
        name: NameId,
        offset: u32,
    ) -> BindingId {
        self.new_binding(scope, name, BindingKind::CatchParameter, offset)
    }

    /// Declares the name of a named function expression in its
    /// [`ScopeKind::FunctionName`] scope.
    pub(crate) fn declare_function_name(
        &mut self,
        scope: ScopeId,
        name: NameId,
        offset: u32,
    ) -> BindingId {
        self.new_binding(scope, name, BindingKind::FunctionName, offset)
    }

    /// Declares a function declaration of `function` in `scope`: var-like
    /// at the top level of a function or script, lexical in a block.
    pub(crate) fn declare_function(
        &mut self,
        scope: ScopeId,
        name: NameId,
        offset: u32,
        function: FunctionId,
        sloppy: bool,
    ) -> Result<BindingId, Redeclared> {
        let existing = self.declared_in(scope, name);
        let binding = if self.is_var_scope(scope) {
            match existing {
                Some(id) => {
                    let kind = self.binding(id).kind;
                    if kind.is_lexical() {
                        return Err(Redeclared);
                    }
                    if kind == BindingKind::Var
                        && let Some(binding) = self.binding_mut(id)
                    {
                        binding.kind = BindingKind::Function;
                    }
                    id
                }
                None => self.new_binding(scope, name, BindingKind::Function, offset),
            }
        } else {
            match existing {
                // Annex B.3.2.4: duplicate function declarations in a
                // block of sloppy mode code.
                Some(id) if sloppy && self.binding(id).kind == BindingKind::Function => id,
                Some(_) => return Err(Redeclared),
                None if self.var_declared_inside(scope, name) => return Err(Redeclared),
                None => {
                    self.note_lexical(scope, name);
                    self.new_binding(scope, name, BindingKind::Function, offset)
                }
            }
        };
        self.function_decls.push((scope, function));
        Ok(binding)
    }

    /// Sets the end of a lexical declarator (for the TDZ check elision).
    pub(crate) fn set_init_end(&mut self, binding: BindingId, end: u32) {
        if let Some(binding) = self.binding_mut(binding) {
            binding.init_end = end;
        }
    }
}
#[cfg(test)]
mod tests;
