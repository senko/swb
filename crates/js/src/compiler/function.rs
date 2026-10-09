//! The compiler of one function: registers, constants, sites, jumps,
//! scope entry, the function prologue and the statements.
//!
//! Scope entry (the contract of the scope analysis, `js-syntax`
//! `scope/mod.rs`): a scope's cells are created when it is entered, and
//! every binding with `needs_tdz` gets the `Empty` marker on every entry
//! (registers are reused between sibling blocks). The function
//! declarations of the scope are created after the cells, in source order.
//!
//! The prologue (`FunctionDeclarationInstantiation`, §10.2.11): the
//! arguments object (before the parameters become cells), the cells of
//! captured parameters, the bindings of the function's own name, `this`
//! and the declarations, then the function declarations. A generator
//! stops there (`InitialYield`). The script's prologue declares its
//! global bindings (§16.1.7).
//!
//! `try` (§14.15, ADR 0026 section 5, memo 1.4): the handler table maps
//! the `try` block to the `catch` code, and the `try` and `catch` code to
//! a stub that enters the `finally` block with a throw completion. A
//! `finally` block exists once; it has two registers for the pending
//! completion (kind and value). `break`, `continue` and `return` that
//! leave it set the completion and jump to the block; the block ends with
//! `EndFinally` and a table of routes, one per kind of completion that
//! can reach it, each continuing through the next outer `finally` block.
//! The layout:
//!
//! ```text
//!       try block                     <- catch range, finally range
//!       Jump L                         (with a catch clause)
//!   C:  catch block                   <- finally range
//!   L:  Int kind = normal
//!   F:  finally block
//!       EndFinally kind value -> X    (normal: to X; throw: rethrow)
//!       Jump R0, Jump R1, ...         (return, jump target 0, ...)
//!   R0: route of the return
//!   R1: route of jump target 0 ...
//!   H:  Int kind = throw; Jump F      (the handler of the finally range)
//!   X:
//! ```

use std::collections::HashMap;

use swb_js_syntax::messages::{NOT_SUPPORTED, STACK_OVERFLOW, TOO_MANY_VARIABLES};
use swb_js_syntax::{
    Ast, BindingId, BindingKind, Function, FunctionId, FunctionKind, FunctionScope, NameId,
    Reference, Resolution, ScopeId, ScopeKind, ScopeTree, Script, StmtId, StmtKind, Storage,
    VariableKind,
};
use swb_js_text::{RecursionBudget, String16};

use super::{Builder, CResult, CompileError, ConstSpec, Session};
use crate::bytecode::{
    COMPLETION_JUMP, COMPLETION_NORMAL, COMPLETION_RETURN, COMPLETION_THROW, CodeKind, GlobalDecl,
    GlobalKind, Handler, Insn, MAX_REGISTERS, Reg, ThisMode,
};
use crate::error::ThrowKind;

/// The budget weight of one statement level of the compiler's recursion:
/// the measured stack use in a debug build (`opt-level = 1`) is up to
/// 1,120 bytes per level (nested `while`; blocks and `if` 752), release
/// 704; the weight leaves a margin of 1.6.
pub(super) const STMT_WEIGHT: u32 = 1792;

/// The budget weight of one expression level: measured up to 992 bytes
/// in a debug build (nested assignments; calls 656, objects 896, unary
/// operators 544, parentheses 368), release 880; a margin of 1.5.
pub(super) const EXPR_WEIGHT: u32 = 1536;

/// The kinds of statements that `break` or `continue` can target.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakableKind {
    /// A loop: `break` and `continue`, with or without a label.
    Loop,
    /// A `switch`: `break` with or without a label.
    Switch,
    /// Another labelled statement: `break label` only.
    Label,
}

/// A statement that `break` (and for loops `continue`) can target.
struct Breakable {
    labels: Vec<NameId>,
    kind: BreakableKind,
    /// The number of enclosing `finally` blocks when the statement
    /// started: a jump to it crosses the blocks above this depth.
    finally_depth: usize,
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

/// A `finally` block while its `try` and `catch` parts compile.
struct Finally {
    /// The register of the completion kind.
    kind: Reg,
    /// The register of the completion value.
    value: Reg,
    /// The jumps to the block (patched to its first instruction).
    entries: Vec<usize>,
    /// The jump targets that pass through the block: (breakable, whether
    /// it is a `continue`). Target `k` has the kind `COMPLETION_JUMP + k`.
    targets: Vec<(usize, bool)>,
}

/// The compiler of one function.
pub(super) struct FunctionCompiler<'a> {
    pub(super) script: &'a Script,
    pub(super) ast: &'a Ast,
    pub(super) scopes: &'a ScopeTree,
    pub(super) id: FunctionId,
    pub(super) function: &'a Function,
    pub(super) fscope: &'a FunctionScope,
    pub(super) session: &'a mut Session,
    budget: &'a mut RecursionBudget,
    pub(super) insns: Vec<Insn>,
    constants: Vec<ConstSpec>,
    string_constants: HashMap<String16, u32>,
    number_constants: HashMap<u64, u32>,
    function_constants: HashMap<FunctionId, u32>,
    sites: Vec<String16>,
    site_map: HashMap<String16, u32>,
    /// The first register above the declared bindings.
    first_temp: u32,
    next_reg: u32,
    max_reg: u32,
    breakables: Vec<Breakable>,
    pending_labels: Vec<NameId>,
    /// The `finally` blocks around the code being compiled, innermost
    /// last.
    finallys: Vec<Finally>,
    /// The exception handlers, in the order of their creation.
    handlers: Vec<Handler>,
    /// The source offset of the construct being compiled.
    pub(super) position: u32,
    lines: Vec<(u32, u32)>,
    pub(super) call_names: Vec<(u32, u32)>,
    /// The register of the script's completion value.
    completion: Option<Reg>,
    /// The global declarations of a script.
    global_decls: Option<Vec<GlobalDecl>>,
    pub(super) strict: bool,
}

impl<'a> FunctionCompiler<'a> {
    pub(super) fn new(
        script: &'a Script,
        id: FunctionId,
        session: &'a mut Session,
        budget: &'a mut RecursionBudget,
    ) -> Self {
        let function = script.ast.function(id);
        let fscope = script.scopes.function(id);
        FunctionCompiler {
            script,
            ast: &script.ast,
            scopes: &script.scopes,
            id,
            function,
            fscope,
            session,
            budget,
            insns: Vec::new(),
            constants: Vec::new(),
            string_constants: HashMap::new(),
            number_constants: HashMap::new(),
            function_constants: HashMap::new(),
            sites: Vec::new(),
            site_map: HashMap::new(),
            first_temp: fscope.register_count,
            next_reg: fscope.register_count,
            max_reg: fscope.register_count,
            breakables: Vec::new(),
            pending_labels: Vec::new(),
            finallys: Vec::new(),
            handlers: Vec::new(),
            position: function.span.start,
            lines: Vec::new(),
            call_names: Vec::new(),
            completion: None,
            global_decls: None,
            strict: function.strict,
        }
    }

    /// Compiles the function.
    pub(super) fn compile(mut self) -> CResult<Builder> {
        if self.fscope.register_count > MAX_REGISTERS {
            return Err(self.too_many_variables());
        }
        let kind = self.code_kind()?;
        let is_script = kind == CodeKind::Script;
        if is_script {
            let completion = self.temp()?;
            self.emit(Insn::Undefined { dst: completion });
            self.completion = Some(completion);
        }
        self.prologue(kind)?;
        for &statement in self.ast.stmts(self.function.body) {
            self.stmt(statement)?;
        }
        self.position = self.function.span.end;
        let result = if let Some(completion) = self.completion {
            completion
        } else {
            self.undefined_temp()?
        };
        self.emit(Insn::Return { src: result });
        let this_mode = if self.function.kind == FunctionKind::Arrow {
            ThisMode::Lexical
        } else if self.strict {
            ThisMode::Strict
        } else {
            ThisMode::Global
        };
        let name = self.function.name.map(|ident| self.name_text(ident.name));
        // TODO(M7, direct eval): a sloppy function with a direct `eval`
        // and no `arguments` reference needs the arguments object too,
        // because the eval code can read it.
        let uses_arguments = !is_script && self.fscope.arguments_binding.is_some();
        let globals = self.globals_of_script(is_script);
        // Sorted by start, the outer of two ranges with the same start
        // first (the order that the verifier and the lookup expect).
        let mut handlers = self.handlers;
        handlers.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        Ok(Builder {
            insns: self.insns,
            constants: self.constants,
            sites: self.sites,
            captures: self.fscope.captures.iter().map(|c| c.source).collect(),
            globals,
            register_count: self.max_reg,
            param_count: self.ast.patterns(self.function.params).len() as u32,
            kind,
            this_mode,
            strict: self.strict,
            uses_arguments,
            name,
            lines: self.lines,
            call_names: self.call_names,
            handlers,
            span: (self.function.span.start, self.function.span.end),
        })
    }

    fn code_kind(&self) -> CResult<CodeKind> {
        let function = self.function;
        if function.is_async {
            return Err(self.unsupported("async function"));
        }
        Ok(match function.kind {
            FunctionKind::Script => CodeKind::Script,
            _ if function.is_generator => CodeKind::Generator,
            FunctionKind::Normal => CodeKind::Normal,
            FunctionKind::Arrow => CodeKind::Arrow,
            FunctionKind::Method => CodeKind::Method,
            FunctionKind::Getter | FunctionKind::Setter => {
                return Err(self.unsupported("getter or setter"));
            }
            FunctionKind::ClassConstructor | FunctionKind::DerivedConstructor => {
                return Err(self.unsupported("class"));
            }
        })
    }

    /// The global declarations of the script (§16.1.7).
    fn globals_of_script(&mut self, is_script: bool) -> Vec<GlobalDecl> {
        if !is_script {
            return Vec::new();
        }
        self.global_decls.take().unwrap_or_default()
    }

    // --- Errors ---

    /// The error for a construct that the compiler does not support yet.
    pub(super) fn unsupported(&self, construct: &'static str) -> CompileError {
        CompileError::Script {
            kind: ThrowKind::SyntaxError,
            message: format!("{NOT_SUPPORTED} ({construct})").into(),
            offset: self.position,
        }
    }

    /// More variables than registers (V8 accepts more; the register
    /// operand has 16 bits).
    fn too_many_variables(&self) -> CompileError {
        CompileError::Script {
            kind: ThrowKind::SyntaxError,
            message: TOO_MANY_VARIABLES.into(),
            offset: self.position,
        }
    }

    /// The temporaries of an expression do not fit the registers left.
    fn too_many_registers(&self) -> CompileError {
        CompileError::Script {
            kind: ThrowKind::SyntaxError,
            message: format!(
                "Expression needs too many registers (only {MAX_REGISTERS} allowed in a function)"
            )
            .into(),
            offset: self.position,
        }
    }

    pub(super) fn internal(message: &'static str) -> CompileError {
        CompileError::Heap(crate::error::Error::invariant(message))
    }

    // --- Recursion budget ---

    pub(super) fn enter(&mut self, weight: u32) -> CResult<()> {
        self.budget.enter(weight).map_err(|_| CompileError::Script {
            kind: ThrowKind::RangeError,
            message: STACK_OVERFLOW.into(),
            offset: self.position,
        })
    }

    pub(super) fn leave(&mut self, weight: u32) {
        self.budget.leave(weight);
    }

    // --- Registers ---

    /// A new temporary register.
    pub(super) fn temp(&mut self) -> CResult<Reg> {
        self.temps(1)
    }

    /// `count` consecutive temporary registers; returns the first.
    pub(super) fn temps(&mut self, count: usize) -> CResult<Reg> {
        let first = self.next_reg;
        let end = u64::from(first) + count as u64;
        if end > u64::from(MAX_REGISTERS) {
            return Err(self.too_many_registers());
        }
        self.next_reg = end as u32;
        self.max_reg = self.max_reg.max(self.next_reg);
        Reg::try_from(first).map_err(|_| self.too_many_registers())
    }

    /// The current top of the temporaries.
    pub(super) fn mark(&self) -> u32 {
        self.next_reg
    }

    /// Frees the temporaries from `mark` on.
    pub(super) fn release(&mut self, mark: u32) {
        self.next_reg = mark.max(self.first_temp);
    }

    /// A new temporary that holds `undefined`.
    fn undefined_temp(&mut self) -> CResult<Reg> {
        let t = self.temp()?;
        self.emit(Insn::Undefined { dst: t });
        Ok(t)
    }

    /// The register for a result: `dst` or a new temporary.
    pub(super) fn target(&mut self, dst: Option<Reg>) -> CResult<Reg> {
        match dst {
            Some(dst) => Ok(dst),
            None => self.temp(),
        }
    }

    // --- Emission ---

    /// Appends an instruction; returns its position.
    pub(super) fn emit(&mut self, insn: Insn) -> usize {
        let pc = self.insns.len();
        if self
            .lines
            .last()
            .is_none_or(|&(_, offset)| offset != self.position)
        {
            self.lines.push((pc as u32, self.position));
        }
        self.insns.push(insn);
        pc
    }

    /// The position of the next instruction.
    pub(super) fn here(&self) -> usize {
        self.insns.len()
    }

    /// Sets the jump at `at` to go to the next instruction.
    pub(super) fn patch_here(&mut self, at: usize) {
        let target = self.here();
        self.patch(at, target);
    }

    fn patch(&mut self, at: usize, target: usize) {
        let offset = target as i64 - at as i64;
        if let Some(insn) = self.insns.get_mut(at) {
            insn.set_jump_offset(offset as i32);
        }
    }

    /// Emits a jump back to `target`.
    pub(super) fn jump_back(&mut self, target: usize) {
        let at = self.here();
        let offset = target as i64 - at as i64;
        self.emit(Insn::Jump {
            offset: offset as i32,
        });
    }

    // --- Constants and sites ---

    /// The constant index of a text.
    pub(super) fn string_const(&mut self, text: String16) -> u32 {
        if let Some(&index) = self.string_constants.get(&text) {
            return index;
        }
        let index = self.constants.len() as u32;
        self.constants.push(ConstSpec::String(text.clone()));
        self.string_constants.insert(text, index);
        index
    }

    /// The constant index of a name.
    pub(super) fn name_const(&mut self, name: NameId) -> u32 {
        let text = self.name_text(name);
        self.string_const(text)
    }

    /// The constant index of a number.
    pub(super) fn number_const(&mut self, value: f64) -> u32 {
        let bits = value.to_bits();
        if let Some(&index) = self.number_constants.get(&bits) {
            return index;
        }
        let index = self.constants.len() as u32;
        self.constants.push(ConstSpec::Number(value));
        self.number_constants.insert(bits, index);
        index
    }

    /// The constant index of a nested function's code.
    pub(super) fn function_const(&mut self, function: FunctionId) -> u32 {
        if let Some(&index) = self.function_constants.get(&function) {
            return index;
        }
        let index = self.constants.len() as u32;
        self.constants.push(ConstSpec::Function(function));
        self.function_constants.insert(function, index);
        index
    }

    /// The property site of a key text.
    pub(super) fn site(&mut self, text: String16) -> u32 {
        if let Some(&index) = self.site_map.get(&text) {
            return index;
        }
        let index = self.sites.len() as u32;
        self.sites.push(text.clone());
        self.site_map.insert(text, index);
        index
    }

    /// The text of a name.
    pub(super) fn name_text(&self, name: NameId) -> String16 {
        self.script
            .names
            .get(name)
            .map(swb_js_text::Str16::to_string16)
            .unwrap_or_default()
    }

    // --- Bindings ---

    /// Initializes a binding at its declaration or at scope entry: a
    /// register, a cell, or a global by name.
    pub(super) fn init_binding(&mut self, binding: BindingId, src: Reg) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        match b.storage {
            Storage::Register(r) => {
                if r != src {
                    self.emit(Insn::Move { dst: r, src });
                }
            }
            Storage::Cell(r) => {
                self.emit(Insn::StoreCell { cell: r, src });
            }
            Storage::Global => {
                let name = self.name_const(b.name);
                let insn = match b.kind {
                    BindingKind::Function => Insn::InitGlobalFunction { src, name },
                    BindingKind::Let | BindingKind::Const => Insn::InitGlobalLexical { src, name },
                    _ => Insn::SetGlobal { src, name },
                };
                self.emit(insn);
            }
            Storage::Unassigned => return Err(Self::internal("a binding without storage")),
        }
        Ok(())
    }

    /// The reference record of an identifier occurrence.
    pub(super) fn reference(&self, id: swb_js_syntax::RefId) -> Reference {
        *self.scopes.reference(id)
    }

    /// The kind of the binding that a reference resolves to.
    pub(super) fn binding_kind(&self, reference: &Reference) -> Option<BindingKind> {
        reference.binding.map(|b| self.scopes.binding(b).kind)
    }

    // --- Scopes ---

    /// Scope entry: cells, TDZ markers, function declarations.
    pub(super) fn enter_scope(&mut self, scope: ScopeId) -> CResult<()> {
        for &binding in self.scopes.bindings_of(scope) {
            let b = *self.scopes.binding(binding);
            match b.kind {
                // Set up by the prologue.
                BindingKind::Parameter
                | BindingKind::This
                | BindingKind::NewTarget
                | BindingKind::Arguments
                | BindingKind::FunctionName => continue,
                BindingKind::Var
                | BindingKind::Function
                | BindingKind::Let
                | BindingKind::Const
                | BindingKind::CatchParameter => {}
            }
            let lexical = b.kind.is_lexical();
            match b.storage {
                Storage::Register(r) if lexical && b.needs_tdz => {
                    self.emit(Insn::Empty { dst: r });
                }
                Storage::Cell(r) => {
                    self.emit(Insn::NewCell {
                        dst: r,
                        empty: lexical && b.needs_tdz,
                    });
                }
                _ => {}
            }
        }
        self.declare_functions(scope)
    }

    /// Creates the function declarations of a scope.
    fn declare_functions(&mut self, scope: ScopeId) -> CResult<()> {
        for &function in self.scopes.function_declarations(scope) {
            let Some(ident) = self.ast.function(function).name else {
                return Err(Self::internal("a function declaration without a name"));
            };
            let Some(binding) = self.scopes.reference(ident.reference).binding else {
                return Err(Self::internal("a function declaration without a binding"));
            };
            let mark = self.mark();
            let t = self.temp()?;
            let index = self.function_const(function);
            self.emit(Insn::Closure { dst: t, index });
            self.init_binding(binding, t)?;
            self.release(mark);
        }
        Ok(())
    }

    /// The per-iteration copies of a `for` head's `let` cells (§14.7.4.4
    /// `CreatePerIterationEnvironment`).
    fn copy_cells(&mut self, scope: ScopeId) {
        for &binding in self.scopes.bindings_of(scope) {
            let b = self.scopes.binding(binding);
            if b.kind == BindingKind::Let
                && let Storage::Cell(r) = b.storage
            {
                self.emit(Insn::CopyCell { reg: r });
            }
        }
    }

    /// `FunctionDeclarationInstantiation` (§10.2.11) or the script's
    /// `GlobalDeclarationInstantiation` (§16.1.7).
    fn prologue(&mut self, kind: CodeKind) -> CResult<()> {
        let scope = self.function.scope;
        if kind == CodeKind::Script {
            self.collect_globals(scope);
            if self.global_decls.as_ref().is_some_and(|g| !g.is_empty()) {
                self.emit(Insn::DeclareGlobals);
            }
        }
        // The arguments object, from the parameters before they become
        // cells.
        let arguments = match self.fscope.arguments_binding {
            Some(binding) if self.scopes.binding(binding).kind == BindingKind::Arguments => {
                let t = self.temp()?;
                self.emit(Insn::Arguments { dst: t });
                Some((binding, t))
            }
            _ => None,
        };
        // Captured parameters (and all parameters of a function with a
        // mapped arguments object) become cells. A repeated name has one
        // binding in the register of its last position.
        let mut seen = Vec::new();
        for &binding in &self.fscope.params {
            if seen.contains(&binding) {
                continue;
            }
            seen.push(binding);
            if let Storage::Cell(r) = self.scopes.binding(binding).storage {
                self.emit(Insn::MakeCell { reg: r });
            }
        }
        // The function's own name (named function expressions).
        if let Some(parent) = self.scopes.scope(scope).parent
            && self.scopes.scope(parent).kind == ScopeKind::FunctionName
            && self.scopes.scope(parent).function == self.id
        {
            for &binding in self.scopes.bindings_of(parent) {
                self.init_special(binding, Insn::Callee { dst: 0 })?;
            }
        }
        for &binding in self.scopes.bindings_of(scope) {
            match self.scopes.binding(binding).kind {
                BindingKind::This => self.init_special(binding, Insn::This { dst: 0 })?,
                BindingKind::Arguments => {
                    if let Some((_, t)) = arguments {
                        self.init_value(binding, t)?;
                    }
                }
                _ => {}
            }
        }
        self.enter_scope(scope)?;
        if kind == CodeKind::Generator {
            self.emit(Insn::InitialYield);
        }
        Ok(())
    }

    /// Initializes `this` or the callee binding with the value that `load`
    /// (with any `dst`) produces.
    fn init_special(&mut self, binding: BindingId, load: Insn) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        let (Storage::Register(r) | Storage::Cell(r)) = b.storage else {
            return Err(Self::internal("a function binding without a register"));
        };
        let load = match load {
            Insn::Callee { .. } => Insn::Callee { dst: r },
            _ => Insn::This { dst: r },
        };
        self.emit(load);
        if matches!(b.storage, Storage::Cell(_)) {
            self.emit(Insn::MakeCell { reg: r });
        }
        Ok(())
    }

    /// Puts a value computed at function entry into a binding's register,
    /// as a new cell if the binding is captured.
    fn init_value(&mut self, binding: BindingId, src: Reg) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        let (Storage::Register(r) | Storage::Cell(r)) = b.storage else {
            return Err(Self::internal("a function binding without a register"));
        };
        if r != src {
            self.emit(Insn::Move { dst: r, src });
        }
        if matches!(b.storage, Storage::Cell(_)) {
            self.emit(Insn::MakeCell { reg: r });
        }
        Ok(())
    }

    /// The global declarations of the script scope.
    fn collect_globals(&mut self, scope: ScopeId) {
        let mut decls = Vec::new();
        for &binding in self.scopes.bindings_of(scope) {
            let b = *self.scopes.binding(binding);
            if b.storage != Storage::Global {
                continue;
            }
            let kind = match b.kind {
                BindingKind::Var => GlobalKind::Var,
                BindingKind::Function => GlobalKind::Function,
                BindingKind::Let => GlobalKind::Let,
                BindingKind::Const => GlobalKind::Const,
                _ => continue,
            };
            let name = self.name_const(b.name);
            decls.push(GlobalDecl { name, kind });
        }
        self.global_decls = Some(decls);
    }

    // --- Statements ---

    pub(super) fn stmt(&mut self, id: StmtId) -> CResult<()> {
        self.enter(STMT_WEIGHT)?;
        let statement = *self.ast.stmt(id);
        let outer = std::mem::replace(&mut self.position, statement.span.start);
        let mark = self.mark();
        match statement.kind {
            StmtKind::Empty | StmtKind::Function(_) => {}
            StmtKind::ForIn { .. } => return Err(self.unsupported("for-in")),
            StmtKind::ForOf { .. } => return Err(self.unsupported("for-of")),
            StmtKind::With { .. } => return Err(self.unsupported("with")),
            StmtKind::Debugger => {
                self.emit(Insn::Nop);
            }
            StmtKind::Expr(e) => match self.completion {
                Some(completion) => {
                    self.expr(e, Some(completion))?;
                }
                None => self.effect(e)?,
            },
            StmtKind::Block { body, scope } => {
                self.enter_scope(scope)?;
                for &s in self.ast.stmts(body) {
                    self.stmt(s)?;
                }
            }
            StmtKind::Variables { kind, declarators } => {
                for declarator in self.ast.declarators(declarators) {
                    self.declarator(kind, declarator.target, declarator.init)?;
                }
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let exits = self.branch_false(test)?;
                self.stmt(consequent)?;
                if let Some(alternate) = alternate {
                    let skip = self.emit(Insn::Jump { offset: 0 });
                    for exit in exits {
                        self.patch_here(exit);
                    }
                    self.stmt(alternate)?;
                    self.patch_here(skip);
                } else {
                    for exit in exits {
                        self.patch_here(exit);
                    }
                }
            }
            StmtKind::While { test, body } => {
                let labels = std::mem::take(&mut self.pending_labels);
                let top = self.here();
                let exits = self.branch_false(test)?;
                self.loop_body(labels, body, Some(top), exits, None)?;
            }
            StmtKind::DoWhile { body, test } => self.do_while(body, test)?,
            StmtKind::For {
                init,
                test,
                update,
                body,
                scope,
            } => self.for_statement(init, test, update, body, scope)?,
            StmtKind::Labeled { label, body } => self.labeled(label, body)?,
            StmtKind::Break { label } => {
                let target = self.find_breakable(label, false)?;
                self.jump_to(target, false);
            }
            StmtKind::Continue { label } => {
                let target = self.find_breakable(label, true)?;
                self.jump_to(target, true);
            }
            StmtKind::Return(argument) => {
                let value = match argument {
                    Some(e) => self.expr(e, None)?,
                    None => self.undefined_temp()?,
                };
                self.emit_return(value);
            }
            StmtKind::Throw(e) => {
                let value = self.expr(e, None)?;
                self.emit(Insn::Throw { src: value });
            }
            StmtKind::Switch {
                discriminant,
                cases,
                scope,
            } => self.switch_statement(discriminant, cases, scope)?,
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler, finalizer)?,
        }
        self.release(mark);
        self.position = outer;
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// `do ... while` (§14.7.2).
    fn do_while(&mut self, body: StmtId, test: swb_js_syntax::ExprId) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let top = self.here();
        self.push_breakable(labels, BreakableKind::Loop);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("loop stack"))?;
        for at in breakable.continues {
            self.patch_here(at);
        }
        let mark = self.mark();
        let v = self.expr(test, None)?;
        let at = self.here();
        self.emit(Insn::JumpIfTrue {
            cond: v,
            offset: (top as i64 - at as i64) as i32,
        });
        self.release(mark);
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// A labelled statement (§14.13): a loop takes the label for
    /// `continue`; any other statement is a target of `break` only.
    fn labeled(&mut self, label: NameId, body: StmtId) -> CResult<()> {
        self.pending_labels.push(label);
        let body_kind = self.ast.stmt(body).kind;
        if matches!(
            body_kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::Labeled { .. }
        ) {
            return self.stmt(body);
        }
        let labels = std::mem::take(&mut self.pending_labels);
        self.push_breakable(labels, BreakableKind::Label);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("label stack"))?;
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// One declarator of `var`, `let` or `const`.
    fn declarator(
        &mut self,
        kind: VariableKind,
        target: swb_js_syntax::PatternId,
        init: Option<swb_js_syntax::ExprId>,
    ) -> CResult<()> {
        let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(target).kind else {
            return Err(self.unsupported("destructuring"));
        };
        let reference = self.reference(ident.reference);
        let name = self.name_text(ident.name);
        match (kind, init) {
            (VariableKind::Var, None) => Ok(()),
            (VariableKind::Var, Some(value)) => {
                self.assign_identifier_value(ident.reference, value, None, name)?;
                Ok(())
            }
            (VariableKind::Let | VariableKind::Const, init) => {
                // A register that is safe to fill directly.
                if let Resolution::Register(r) = reference.resolution
                    && init.is_none_or(|value| self.writes_target_last(value))
                {
                    match init {
                        Some(value) => {
                            self.expr_named(value, Some(r), name)?;
                        }
                        None => {
                            self.emit(Insn::Undefined { dst: r });
                        }
                    }
                    return Ok(());
                }
                let mark = self.mark();
                let t = self.temp()?;
                match init {
                    Some(value) => {
                        self.expr_named(value, Some(t), name)?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: t });
                    }
                }
                let binding = reference
                    .binding
                    .ok_or(Self::internal("a declaration without a binding"))?;
                self.init_binding(binding, t)?;
                self.release(mark);
                Ok(())
            }
        }
    }

    /// A `for` statement (§14.7.4).
    fn for_statement(
        &mut self,
        init: Option<StmtId>,
        test: Option<swb_js_syntax::ExprId>,
        update: Option<swb_js_syntax::ExprId>,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        self.enter_scope(scope)?;
        if let Some(init) = init {
            match self.ast.stmt(init).kind {
                // The value of an initializer expression is not the
                // statement's completion value.
                StmtKind::Expr(e) => {
                    self.position = self.ast.stmt(init).span.start;
                    self.effect(e)?;
                }
                _ => self.stmt(init)?,
            }
        }
        let per_iteration = self.scopes.scope(scope).per_iteration;
        if per_iteration {
            self.copy_cells(scope);
        }
        let top = self.here();
        let exits = match test {
            Some(test) => self.branch_false(test)?,
            None => Vec::new(),
        };
        self.loop_body(
            labels,
            body,
            None,
            exits,
            Some((scope, per_iteration, update, top)),
        )
    }

    /// The body of a loop, its continue target and its exit. For `while`,
    /// `continue_to` is the test; for `for`, `next` holds the update.
    fn loop_body(
        &mut self,
        labels: Vec<NameId>,
        body: StmtId,
        continue_to: Option<usize>,
        exits: Vec<usize>,
        next: Option<(ScopeId, bool, Option<swb_js_syntax::ExprId>, usize)>,
    ) -> CResult<()> {
        self.push_breakable(labels, BreakableKind::Loop);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("loop stack"))?;
        match (continue_to, next) {
            (Some(top), _) => {
                for at in breakable.continues {
                    self.patch(at, top);
                }
                self.jump_back(top);
            }
            (None, Some((scope, per_iteration, update, top))) => {
                for at in breakable.continues {
                    self.patch_here(at);
                }
                if per_iteration {
                    self.copy_cells(scope);
                }
                if let Some(update) = update {
                    self.effect(update)?;
                }
                self.jump_back(top);
            }
            (None, None) => return Err(Self::internal("a loop without a continue target")),
        }
        for at in exits.into_iter().chain(breakable.breaks) {
            self.patch_here(at);
        }
        Ok(())
    }

    /// Starts a statement that `break` or `continue` can target.
    fn push_breakable(&mut self, labels: Vec<NameId>, kind: BreakableKind) {
        self.breakables.push(Breakable {
            labels,
            kind,
            finally_depth: self.finallys.len(),
            breaks: Vec::new(),
            continues: Vec::new(),
        });
    }

    /// The breakable statement that `break label` or `continue label`
    /// targets.
    fn find_breakable(&self, label: Option<NameId>, is_continue: bool) -> CResult<usize> {
        self.breakables
            .iter()
            .rposition(|b| {
                let is_loop = b.kind == BreakableKind::Loop;
                match label {
                    Some(label) => b.labels.contains(&label) && (is_loop || !is_continue),
                    None if is_continue => is_loop,
                    None => b.kind != BreakableKind::Label,
                }
            })
            .ok_or(Self::internal("a break or continue without a target"))
    }

    /// A `break` (or `continue`) to the breakable statement `target`: a
    /// jump, or through the innermost `finally` block that it leaves.
    fn jump_to(&mut self, target: usize, is_continue: bool) {
        let depth = self.breakables.get(target).map_or(0, |b| b.finally_depth);
        if self.finallys.len() > depth
            && let Some(finally) = self.finallys.last_mut()
        {
            let known = finally
                .targets
                .iter()
                .position(|&t| t == (target, is_continue));
            let k = known.unwrap_or_else(|| {
                finally.targets.push((target, is_continue));
                finally.targets.len() - 1
            });
            let kind = finally.kind;
            self.emit(Insn::Int {
                dst: kind,
                value: COMPLETION_JUMP + k as i32,
            });
            let at = self.emit(Insn::Jump { offset: 0 });
            if let Some(finally) = self.finallys.last_mut() {
                finally.entries.push(at);
            }
            return;
        }
        let at = self.emit(Insn::Jump { offset: 0 });
        if let Some(b) = self.breakables.get_mut(target) {
            if is_continue {
                b.continues.push(at);
            } else {
                b.breaks.push(at);
            }
        }
    }

    /// Returns the value of `value`: a `Return`, or through the innermost
    /// `finally` block.
    pub(super) fn emit_return(&mut self, value: Reg) {
        let Some(finally) = self.finallys.last() else {
            self.emit(Insn::Return { src: value });
            return;
        };
        let (kind, register) = (finally.kind, finally.value);
        if value != register {
            self.emit(Insn::Move {
                dst: register,
                src: value,
            });
        }
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_RETURN,
        });
        let at = self.emit(Insn::Jump { offset: 0 });
        if let Some(finally) = self.finallys.last_mut() {
            finally.entries.push(at);
        }
    }

    /// `switch` (§14.12.4 `CaseBlockEvaluation`): the tests in source
    /// order, then the jump to `default` (the order of the specification:
    /// the clauses before `default`, then the ones after it, then
    /// `default`), then the bodies, which fall through.
    fn switch_statement(
        &mut self,
        discriminant: swb_js_syntax::ExprId,
        cases: swb_js_syntax::List<swb_js_syntax::SwitchCase>,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let value = self.temp()?;
        self.expr(discriminant, Some(value))?;
        self.enter_scope(scope)?;
        let cases = self.ast.cases(cases).to_vec();
        let mut tests = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = case.test {
                self.position = case.span.start;
                let mark = self.mark();
                let v = self.expr(test, None)?;
                let t = self.temp()?;
                self.emit(Insn::StrictEq {
                    dst: t,
                    a: value,
                    b: v,
                });
                tests.push((i, self.emit(Insn::JumpIfTrue { cond: t, offset: 0 })));
                self.release(mark);
            }
        }
        let otherwise = self.emit(Insn::Jump { offset: 0 });
        self.push_breakable(labels, BreakableKind::Switch);
        let mut starts = Vec::with_capacity(cases.len());
        for case in &cases {
            starts.push(self.here());
            for &statement in self.ast.stmts(case.body) {
                self.stmt(statement)?;
            }
        }
        let breakable = self
            .breakables
            .pop()
            .ok_or(Self::internal("switch stack"))?;
        for (i, at) in tests {
            let start = starts.get(i).copied().unwrap_or_else(|| self.here());
            self.patch(at, start);
        }
        let default = cases
            .iter()
            .position(|c| c.test.is_none())
            .and_then(|i| starts.get(i).copied())
            .unwrap_or_else(|| self.here());
        self.patch(otherwise, default);
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// `try` (§14.15.3); see the module documentation for the layout.
    fn try_statement(
        &mut self,
        block: StmtId,
        handler: Option<swb_js_syntax::CatchClause>,
        finalizer: Option<StmtId>,
    ) -> CResult<()> {
        if finalizer.is_some() {
            let kind = self.temps(2)?;
            self.finallys.push(Finally {
                kind,
                value: kind + 1,
                entries: Vec::new(),
                targets: Vec::new(),
            });
        }
        let start = self.here();
        self.stmt(block)?;
        if let Some(clause) = handler {
            let end = self.here();
            let skip = self.emit(Insn::Jump { offset: 0 });
            let mark = self.mark();
            let exception = self.temp()?;
            let target = self.here();
            if end > start {
                self.handlers.push(Handler {
                    start: start as u32,
                    end: end as u32,
                    target: target as u32,
                    register: exception,
                });
            }
            self.catch_clause(clause, exception)?;
            self.release(mark);
            self.patch_here(skip);
        }
        if let Some(finalizer) = finalizer {
            self.finally_block(start, finalizer)?;
        }
        Ok(())
    }

    /// The `catch` clause: the scope of the parameter and the block, the
    /// parameter's binding, the statements.
    fn catch_clause(&mut self, clause: swb_js_syntax::CatchClause, exception: Reg) -> CResult<()> {
        let StmtKind::Block { body, scope } = self.ast.stmt(clause.body).kind else {
            return Err(Self::internal("a catch clause without a block"));
        };
        self.enter(STMT_WEIGHT)?;
        self.position = self.ast.stmt(clause.body).span.start;
        self.enter_scope(scope)?;
        if let Some(param) = clause.param {
            let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(param).kind else {
                return Err(self.unsupported("destructuring"));
            };
            let binding = self
                .reference(ident.reference)
                .binding
                .ok_or(Self::internal("a catch parameter without a binding"))?;
            self.init_binding(binding, exception)?;
        }
        for &statement in self.ast.stmts(body) {
            self.stmt(statement)?;
        }
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// The `finally` block of a `try` statement whose code starts at
    /// `start`, with its routes and its handler stub.
    fn finally_block(&mut self, start: usize, finalizer: StmtId) -> CResult<()> {
        let finally = self.finallys.pop().ok_or(Self::internal("finally stack"))?;
        let (kind, value) = (finally.kind, finally.value);
        let end = self.here();
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_NORMAL,
        });
        let entry = self.here();
        for at in finally.entries {
            self.patch(at, entry);
        }
        self.stmt(finalizer)?;
        let end_finally = self.emit(Insn::EndFinally {
            kind,
            value,
            offset: 0,
        });
        // The table: the return, then each jump target.
        let table: Vec<usize> = (0..=finally.targets.len())
            .map(|_| self.emit(Insn::Jump { offset: 0 }))
            .collect();
        let mut routes = table.into_iter();
        if let Some(at) = routes.next() {
            self.patch_here(at);
            self.emit_return(value);
        }
        for (&(target, is_continue), at) in finally.targets.iter().zip(routes) {
            self.patch_here(at);
            self.jump_to(target, is_continue);
        }
        // The handler of the `try` and `catch` code.
        let stub = self.here();
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_THROW,
        });
        self.jump_back(entry);
        if end > start {
            self.handlers.push(Handler {
                start: start as u32,
                end: end as u32,
                target: stub as u32,
                register: value,
            });
        }
        self.patch_here(end_finally);
        Ok(())
    }
}
