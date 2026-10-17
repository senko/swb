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
//! stops there (`InitialYield`). An async function starts with
//! `AsyncStart`, and one handler covers all of its code after it: a throw
//! that nothing catches rejects the function's promise (`AsyncReject` at
//! the end of the code). The script's prologue declares its global
//! bindings (§16.1.7).
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
    AssignTarget, Ast, BindingId, BindingKind, ExprId, ForHead, Function, FunctionId, FunctionKind,
    FunctionScope, NameId, Reference, Resolution, ScopeId, ScopeKind, ScopeTree, Script, StmtId,
    StmtKind, Storage, VariableKind,
};
use swb_js_text::{RecursionBudget, Str16, String16};

use super::pattern::{BindMode, PatternSource};
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
pub(super) enum BreakableKind {
    /// A loop: `break` and `continue`, with or without a label.
    Loop,
    /// A `switch`: `break` with or without a label.
    Switch,
    /// Another labelled statement: `break label` only.
    Label,
}

/// A statement that `break` (and for loops `continue`) can target.
pub(super) struct Breakable {
    labels: Vec<NameId>,
    kind: BreakableKind,
    /// The number of enclosing `finally` blocks when the statement
    /// started: a jump to it crosses the blocks above this depth.
    finally_depth: usize,
    pub(super) breaks: Vec<usize>,
    pub(super) continues: Vec<usize>,
}

/// A `finally` block while its `try` and `catch` parts compile, or the
/// implicit `finally` block that closes an iterator (the body of a
/// `for`-`of` loop, an array pattern).
pub(super) struct Finally {
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

/// The code of a `finally` block.
#[derive(Clone, Copy)]
pub(super) enum FinallyBody {
    /// The block of a `try` statement.
    Block(StmtId),
    /// `IteratorClose` of the iterator record at the register (ADR 0026
    /// section 5: `for`-`of` and array patterns close their iterator on
    /// every abrupt completion).
    Close(Reg),
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
    /// The text of the script (for the positions of tokens).
    pub(super) text: Str16<'a>,
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
    pub(super) breakables: Vec<Breakable>,
    pub(super) pending_labels: Vec<NameId>,
    /// The `finally` blocks around the code being compiled, innermost
    /// last.
    finallys: Vec<Finally>,
    /// The exception handlers, in the order of their creation.
    handlers: Vec<Handler>,
    /// The short-circuit jumps of the optional chains being compiled,
    /// innermost last.
    pub(super) chain_exits: Vec<Vec<usize>>,
    /// The links of the optional chains being compiled (the member
    /// accesses and calls along each chain, for source positions).
    pub(super) chain_links: Vec<ExprId>,
    /// The source offset of the construct being compiled.
    pub(super) position: u32,
    lines: Vec<(u32, u32)>,
    pub(super) call_names: Vec<(u32, u32)>,
    /// The strict ranges of sloppy code (class definitions).
    pub(super) strict_ranges: Vec<(u32, u32)>,
    /// The register of the script's completion value.
    pub(super) completion: Option<Reg>,
    /// The global declarations of a script.
    global_decls: Option<Vec<GlobalDecl>>,
    pub(super) strict: bool,
    /// A derived constructor: two registers for the check of its result
    /// at each return.
    derived_result: Option<(Reg, Reg)>,
    /// The jumps of a derived constructor's returns to its epilogue.
    pub(super) derived_exits: Vec<usize>,
    /// The direct `eval` calls: (instruction, scope of the call).
    pub(super) eval_sites: Vec<(u32, ScopeId)>,
}

impl<'a> FunctionCompiler<'a> {
    pub(super) fn new(
        script: &'a Script,
        id: FunctionId,
        session: &'a mut Session,
        budget: &'a mut RecursionBudget,
        text: Str16<'a>,
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
            text,
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
            chain_exits: Vec::new(),
            chain_links: Vec::new(),
            position: function.span.start,
            lines: Vec::new(),
            call_names: Vec::new(),
            strict_ranges: Vec::new(),
            completion: None,
            global_decls: None,
            strict: function.strict,
            derived_result: None,
            derived_exits: Vec::new(),
            eval_sites: Vec::new(),
        }
    }

    /// Compiles the function.
    pub(super) fn compile(mut self) -> CResult<Builder> {
        if self.fscope.register_count > MAX_REGISTERS {
            return Err(self.too_many_variables());
        }
        let kind = self.code_kind()?;
        // Script and eval code have a completion value (§16.1.6, §19.2.1.1).
        let is_script = matches!(kind, CodeKind::Script | CodeKind::Eval);
        if is_script {
            let completion = self.temp()?;
            self.emit(Insn::Undefined { dst: completion });
            self.completion = Some(completion);
        }
        if kind == CodeKind::DerivedConstructor {
            let result = self.temps(2)?;
            self.derived_result = Some((result, result + 1));
        }
        if kind == CodeKind::Async {
            self.emit(Insn::AsyncStart);
        }
        let body_start = self.here();
        self.prologue(kind)?;
        for &statement in self.ast.stmts(self.function.body) {
            self.stmt(statement)?;
        }
        if kind == CodeKind::Initializer {
            self.initializer_body()?;
        }
        self.position = self.function.span.end;
        let result = if let Some(completion) = self.completion {
            completion
        } else {
            self.undefined_temp()?
        };
        self.emit_return(result);
        if let Some(registers) = self.derived_result {
            self.derived_epilogue(registers);
        }
        if kind == CodeKind::Async {
            self.async_epilogue(body_start)?;
        }
        let this_mode = if matches!(self.function.kind, FunctionKind::Arrow | FunctionKind::Eval) {
            ThisMode::Lexical
        } else if self.strict {
            ThisMode::Strict
        } else {
            ThisMode::Global
        };
        // CreateDynamicFunction step 33: `SetFunctionName(F, "anonymous")`.
        let dynamic = self.script.dynamic_function == Some(self.id);
        // Stack traces name the frames of eval code `eval` (V8).
        let name = self
            .function
            .name
            .map(|ident| self.name_text(ident.name))
            .or_else(|| super::class::initializer_name(self.function.kind))
            .or_else(|| (kind == CodeKind::Eval).then(|| String16::from("eval")))
            .or_else(|| dynamic.then(|| String16::from("anonymous")));
        // A function with a direct `eval` has an `arguments` binding when
        // §10.2.11 needs the object (the scope analysis adds it). A rest
        // parameter needs the extra arguments too.
        let keeps_extra_args =
            !is_script && (self.fscope.arguments_binding.is_some() || self.has_rest_parameter());
        let globals = self.globals_of_script(is_script);
        // Sorted by start, the outer of two ranges with the same start
        // first (the order that the verifier and the lookup expect).
        let length = self.expected_argument_count();
        let span = self.source_span();
        let eval = self.eval_description()?;
        let mut captures: Vec<_> = self.fscope.captures.iter().map(|c| c.source).collect();
        if let Some(plan) = &self.session.caller {
            captures.extend(plan.sources(self.script, self.id));
        }
        let mut handlers = self.handlers;
        handlers.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        Ok(Builder {
            insns: self.insns,
            constants: self.constants,
            sites: self.sites,
            captures,
            globals,
            register_count: self.max_reg,
            param_count: self.ast.patterns(self.function.params).len() as u32,
            length,
            kind,
            this_mode,
            strict: self.strict,
            strict_ranges: self.strict_ranges,
            keeps_extra_args,
            name,
            lines: self.lines,
            call_names: self.call_names,
            handlers,
            span,
            eval,
        })
    }

    fn code_kind(&self) -> CResult<CodeKind> {
        let function = self.function;
        if function.is_async && function.is_generator {
            return Err(self.unsupported("async generator"));
        }
        Ok(match function.kind {
            // Indirect eval code is global code with a top scope of its own.
            FunctionKind::Script if self.scopes.scope(function.scope).kind == ScopeKind::Eval => {
                CodeKind::Eval
            }
            FunctionKind::Script => CodeKind::Script,
            FunctionKind::Eval => CodeKind::Eval,
            _ if function.is_generator => CodeKind::Generator,
            _ if function.is_async => CodeKind::Async,
            FunctionKind::Normal => CodeKind::Normal,
            FunctionKind::Arrow => CodeKind::Arrow,
            // Accessors of object literals are methods (§15.4.5): not
            // constructors, without `prototype`.
            FunctionKind::Method | FunctionKind::Getter | FunctionKind::Setter => CodeKind::Method,
            FunctionKind::ClassConstructor => CodeKind::ClassConstructor,
            FunctionKind::DerivedConstructor => CodeKind::DerivedConstructor,
            FunctionKind::InstanceInitializer | FunctionKind::StaticInitializer => {
                CodeKind::Initializer
            }
            FunctionKind::Module => return Err(self.unsupported("module")),
        })
    }

    /// The source range of the function for `Function.prototype.toString`:
    /// a class constructor has the text of its class (§15.7.14 step 18 of
    /// the 2025 text, `[[SourceText]]`).
    fn source_span(&self) -> (u32, u32) {
        let span = match self.session.function_classes.get(&self.id) {
            Some(&class)
                if self.function.kind != FunctionKind::InstanceInitializer
                    && self.function.kind != FunctionKind::StaticInitializer =>
            {
                self.ast.class(class).span
            }
            _ => self.function.span,
        };
        (span.start, span.end)
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
    pub(super) fn undefined_temp(&mut self) -> CResult<Reg> {
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

    pub(super) fn patch(&mut self, at: usize, target: usize) {
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

    /// A new constant (not shared with equal ones).
    pub(super) fn push_constant(&mut self, constant: ConstSpec) -> u32 {
        let index = self.constants.len() as u32;
        self.constants.push(constant);
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
            .map(Str16::to_string16)
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
                    BindingKind::Let | BindingKind::Const | BindingKind::Class => {
                        Insn::InitGlobalLexical { src, name }
                    }
                    _ => Insn::SetGlobal { src, name },
                };
                self.emit(insn);
            }
            Storage::Unassigned => return Err(Self::internal("a binding without storage")),
            Storage::Caller => {
                return self.init_var_env_binding(binding, src, b.kind == BindingKind::Function);
            }
            Storage::Import => return Err(self.unsupported("module")),
            Storage::Private(_) => return Err(Self::internal("a store to a private name")),
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
        self.enter_scope_bindings(scope)?;
        self.declare_functions(scope)
    }

    /// The cells and TDZ markers of a scope's bindings (scope entry
    /// without the function declarations).
    pub(super) fn enter_scope_bindings(&mut self, scope: ScopeId) -> CResult<()> {
        for &binding in self.scopes.bindings_of(scope) {
            let b = *self.scopes.binding(binding);
            // The record of a dynamic environment (made by the `with`
            // statement or below) and the private environment of a class
            // body (made by the class definition right after the scope
            // entry) go into new cells.
            if let (BindingKind::DynamicEnv | BindingKind::PrivateEnvironment, Storage::Cell(r)) =
                (b.kind, b.storage)
            {
                self.emit(Insn::NewCell {
                    dst: r,
                    empty: false,
                });
                continue;
            }
            match b.kind {
                // Set up by the prologue (the home object and the active
                // function too), or below (the record of eval vars).
                BindingKind::Parameter
                | BindingKind::This
                | BindingKind::NewTarget
                | BindingKind::Arguments
                | BindingKind::FunctionName
                | BindingKind::DynamicEnv
                | BindingKind::HomeObject
                | BindingKind::ActiveFunction
                | BindingKind::PrivateName
                | BindingKind::PrivateEnvironment => continue,
                BindingKind::Import => return Err(self.unsupported("module")),
                BindingKind::Var
                | BindingKind::Function
                | BindingKind::BlockFunctionVar
                | BindingKind::Let
                | BindingKind::Const
                | BindingKind::CatchParameter
                | BindingKind::Class
                | BindingKind::ClassName => {}
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
        self.enter_var_record(scope)
    }

    /// Creates the function declarations of a scope.
    pub(super) fn declare_functions(&mut self, scope: ScopeId) -> CResult<()> {
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

    /// The evaluation of a function declaration that Annex B.3.2 hoists:
    /// the value of its block binding goes into its var binding
    /// (B.3.2.1, `F.[[VarEnv]].SetMutableBinding`).
    fn block_function_var(&mut self, function: FunctionId) -> CResult<()> {
        let Some(target) = self.scopes.function(function).block_function_var else {
            return Ok(());
        };
        let Some(ident) = self.ast.function(function).name else {
            return Err(Self::internal("a function declaration without a name"));
        };
        let var = self.reference(target).binding;
        // Annex B.3.2.3 (and B.3.2.2 in indirect eval code): the var
        // binding of eval code exists only if `EvalDeclarationInstantiation`
        // created it.
        if var.is_some_and(|b| self.skips_block_function(b)) {
            return Ok(());
        }
        let mark = self.mark();
        let value = self.load_ref(ident.reference, None)?;
        match var {
            // `genv.SetMutableBinding(F, fobj, false)`: the variable
            // environment itself, not through the scopes between.
            Some(b) if self.scopes.binding(b).storage == Storage::Caller => {
                self.init_var_env_binding(b, value, false)?;
            }
            // Annex B.3.2.2: in a script (which has global declarations),
            // only if `DeclareGlobals` hoisted the function, a decision at
            // run time.
            Some(b)
                if self.scopes.binding(b).kind == BindingKind::BlockFunctionVar
                    && self.scopes.binding(b).storage == Storage::Global
                    && self.global_decls.is_some() =>
            {
                let name = self.name_const(self.scopes.binding(b).name);
                let hoisted = self.temp()?;
                self.emit(Insn::BlockFunctionHoisted { dst: hoisted, name });
                let skip = self.emit(Insn::JumpIfFalse {
                    cond: hoisted,
                    offset: 0,
                });
                self.emit(Insn::SetGlobal { src: value, name });
                self.patch_here(skip);
            }
            _ => self.store_ref(target, value)?,
        }
        self.release(mark);
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
        if kind == CodeKind::ClassConstructor {
            self.base_constructor_fields()?;
        }
        if kind == CodeKind::Script {
            self.collect_globals(scope);
            if self.global_decls.as_ref().is_some_and(|g| !g.is_empty()) {
                self.emit(Insn::DeclareGlobals);
            }
        }
        // The arguments object: an unmapped one from the parameters before
        // they become cells, a mapped one from their cells.
        let arguments_binding = self
            .fscope
            .arguments_binding
            .filter(|&b| self.scopes.binding(b).kind == BindingKind::Arguments);
        let mapped = self.fscope.mapped_arguments;
        let mut arguments = None;
        if let Some(binding) = arguments_binding
            && !mapped
        {
            let t = self.temp()?;
            self.emit(Insn::Arguments { dst: t, mapped });
            arguments = Some((binding, t));
        }
        // Captured parameters (and all parameters of a function with a
        // mapped arguments object) become cells. A repeated name has one
        // binding in the register of its last position (the position that
        // a mapped object maps), so a cell is made at that position only.
        for (position, &binding) in self.fscope.params.iter().enumerate() {
            if let Storage::Cell(r) = self.scopes.binding(binding).storage
                && usize::from(r) == position
            {
                self.emit(Insn::MakeCell { reg: r });
            }
        }
        if let Some(binding) = arguments_binding
            && mapped
        {
            let t = self.temp()?;
            self.emit(Insn::Arguments { dst: t, mapped });
            arguments = Some((binding, t));
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
                BindingKind::NewTarget => {
                    self.init_special(binding, Insn::NewTarget { dst: 0 })?;
                }
                BindingKind::HomeObject => {
                    self.init_special(binding, Insn::HomeObject { dst: 0 })?;
                }
                BindingKind::ActiveFunction => {
                    self.init_special(binding, Insn::Callee { dst: 0 })?;
                }
                BindingKind::Arguments => {
                    if let Some((_, t)) = arguments {
                        self.init_value(binding, t)?;
                    }
                }
                _ => {}
            }
        }
        if self.has_simple_parameters() {
            self.enter_scope(scope)?;
        } else {
            self.parameter_list(scope)?;
        }
        if kind == CodeKind::Generator {
            self.emit(Insn::InitialYield);
        }
        Ok(())
    }

    /// The handler of an async function's whole code after `AsyncStart`
    /// (the parameters and the body, §15.8.4 step 3 and §27.7.5.2 step
    /// 3.h): a throw that nothing in it catches rejects the result
    /// promise. `start` is the first instruction after `AsyncStart`.
    fn async_epilogue(&mut self, start: usize) -> CResult<()> {
        let reason = self.temp()?;
        let target = self.emit(Insn::AsyncReject { src: reason });
        self.handlers.push(Handler {
            start: start as u32,
            end: target as u32,
            target: target as u32,
            register: reason,
        });
        Ok(())
    }

    /// Initializes `this`, `new.target` or the callee binding with the
    /// value that `load` (with any `dst`) produces.
    fn init_special(&mut self, binding: BindingId, load: Insn) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        let (Storage::Register(r) | Storage::Cell(r)) = b.storage else {
            return Err(Self::internal("a function binding without a register"));
        };
        let load = match load {
            Insn::Callee { .. } => Insn::Callee { dst: r },
            Insn::NewTarget { .. } => Insn::NewTarget { dst: r },
            Insn::HomeObject { .. } => Insn::HomeObject { dst: r },
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
                BindingKind::BlockFunctionVar => GlobalKind::BlockFunction,
                BindingKind::Function => GlobalKind::Function,
                BindingKind::Let | BindingKind::Class => GlobalKind::Let,
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
        if resets_completion(statement.kind) {
            self.reset_completion();
        }
        self.stmt_kind(statement.kind)?;
        self.release(mark);
        self.position = outer;
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// The code of one statement.
    fn stmt_kind(&mut self, kind: StmtKind) -> CResult<()> {
        match kind {
            StmtKind::Empty => {}
            StmtKind::Function(function) => self.block_function_var(function)?,
            StmtKind::ForIn {
                left,
                right,
                body,
                scope,
            } => self.for_in(left, right, body, scope)?,
            StmtKind::ForOf {
                left,
                right,
                body,
                scope,
                is_await: false,
            } => self.for_of(left, right, body, scope)?,
            StmtKind::Class(class) => self.class_declaration(class)?,
            StmtKind::With {
                object,
                body,
                scope,
            } => self.with_statement(object, body, scope)?,
            kind @ (StmtKind::ForOf { is_await: true, .. }
            | StmtKind::Import
            | StmtKind::ExportDeclaration(_)
            | StmtKind::ExportDefault { .. }
            | StmtKind::ExportList) => {
                let construct = super::support::unsupported_stmt(kind).unwrap_or("statement");
                return Err(self.unsupported(construct));
            }
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
                    // The temporaries of one declarator are free after it.
                    let mark = self.mark();
                    self.declarator(kind, declarator.target, declarator.init)?;
                    self.release(mark);
                }
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => self.if_statement(test, consequent, alternate)?,
            StmtKind::While { test, body } => self.while_statement(test, body)?,
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
        Ok(())
    }

    /// Sets the completion value of script and eval code to `undefined`
    /// (§8.4 `UpdateEmpty`).
    pub(super) fn reset_completion(&mut self) {
        if let Some(completion) = self.completion {
            self.emit(Insn::Undefined { dst: completion });
        }
    }

    /// A class declaration (§15.7.15 `BindingClassDeclarationEvaluation`):
    /// the class, then its binding in the enclosing scope.
    fn class_declaration(&mut self, id: swb_js_syntax::ClassId) -> CResult<()> {
        let declaration = self
            .ast
            .class(id)
            .declaration
            .ok_or(Self::internal("a class declaration without a binding"))?;
        let binding = self
            .reference(declaration)
            .binding
            .ok_or(Self::internal("a class declaration without a binding"))?;
        let t = self.class_definition(id, None, None)?;
        self.init_binding(binding, t)
    }

    /// `if` (§14.6).
    fn if_statement(
        &mut self,
        test: ExprId,
        consequent: StmtId,
        alternate: Option<StmtId>,
    ) -> CResult<()> {
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
        Ok(())
    }

    /// `while` (§14.7.3).
    fn while_statement(&mut self, test: ExprId, body: StmtId) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let top = self.here();
        let exits = self.branch_false(test)?;
        self.loop_body(labels, body, Some(top), exits, None)
    }

    /// `do ... while` (§14.7.2).
    fn do_while(&mut self, body: StmtId, test: ExprId) -> CResult<()> {
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
                | StmtKind::ForIn { .. }
                | StmtKind::ForOf { .. }
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
        init: Option<ExprId>,
    ) -> CResult<()> {
        let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(target).kind else {
            let init = init.ok_or(Self::internal("a pattern declaration without a value"))?;
            let mark = self.mark();
            let value = self.temp()?;
            self.expr(init, Some(value))?;
            let mode = if kind == VariableKind::Var {
                BindMode::Var
            } else {
                BindMode::Init
            };
            self.bind_pattern(target, value, mode, PatternSource::Expr(init))?;
            self.release(mark);
            return Ok(());
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
        test: Option<ExprId>,
        update: Option<ExprId>,
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

    /// `for (left in right) body` (§14.7.5.6 `ForIn/OfHeadEvaluation`,
    /// §14.7.5.7 `ForIn/OfBodyEvaluation`, Annex B.3.5 for the initializer
    /// of `var`). The keys come from an iterator that the VM makes
    /// (`ForInStart`); `ForInNext` puts each key into a register or ends
    /// the loop. A `let` or `const` binding that is a cell gets a new cell
    /// in each iteration.
    fn for_in(
        &mut self,
        left: ForHead,
        right: ExprId,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let declaration = match left {
            ForHead::Declaration(stmt) => {
                let StmtKind::Variables { kind, declarators } = self.ast.stmt(stmt).kind else {
                    return Err(Self::internal("a for-in declaration without variables"));
                };
                let [declarator] = self.ast.declarators(declarators) else {
                    return Err(Self::internal("a for-in declaration with several bindings"));
                };
                let declarator = *declarator;
                if kind == VariableKind::Var && declarator.init.is_some() {
                    self.declarator(kind, declarator.target, declarator.init)?;
                }
                Some((kind, declarator.target))
            }
            ForHead::Target(_) => None,
        };
        // The value of a loop that does not run its body is `undefined`
        // (§14.7.5.7 step 2; a nullish object breaks with an empty value,
        // which §14.7.1.1 turns into `undefined`).
        if let Some(completion) = self.completion {
            self.emit(Insn::Undefined { dst: completion });
        }
        self.enter_scope(scope)?;
        let iterator = self.temp()?;
        let key = self.temp()?;
        self.expr(right, Some(iterator))?;
        self.position = self.ast.expr(right).span.start;
        self.emit(Insn::ForInStart {
            dst: iterator,
            src: iterator,
        });
        let top = self.here();
        let exit = self.emit(Insn::ForInNext {
            dst: key,
            iter: iterator,
            offset: 0,
        });
        match (declaration, left) {
            (Some((kind, target)), _) => self.for_in_binding(kind, target, key, scope)?,
            (None, ForHead::Target(AssignTarget::Simple(target))) => {
                // Chromium reports a write to a private member at its name.
                let inner = self.unparen(target);
                if let swb_js_syntax::ExprKind::PrivateMember { .. } = self.ast.expr(inner).kind {
                    self.position = self.name_position(inner);
                }
                self.assign_to_target(target, key)?;
            }
            (None, ForHead::Target(AssignTarget::Pattern(pattern))) => {
                self.bind_pattern(pattern, key, BindMode::Assign, PatternSource::Nested)?;
            }
            (None, ForHead::Declaration(_)) => {
                return Err(Self::internal("a for-in declaration without a binding"));
            }
        }
        self.loop_body(labels, body, Some(top), vec![exit], None)?;
        // After a `break`, the register still holds the iterator and its
        // key list; release them now, not when the register is reused.
        self.emit(Insn::Undefined { dst: iterator });
        Ok(())
    }

    /// The binding of a `for`-`in` declaration gets the key in `key`.
    fn for_in_binding(
        &mut self,
        kind: VariableKind,
        target: swb_js_syntax::PatternId,
        key: Reg,
        scope: ScopeId,
    ) -> CResult<()> {
        let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(target).kind else {
            return self.bind_head_pattern(kind, target, key, scope);
        };
        if kind == VariableKind::Var {
            return self.store_ref(ident.reference, key);
        }
        let binding = self
            .reference(ident.reference)
            .binding
            .ok_or(Self::internal("a declaration without a binding"))?;
        match self.scopes.binding(binding).storage {
            // A new binding per iteration (§14.7.5.7 step 6.g): closures
            // of earlier iterations keep their cells.
            Storage::Cell(r) => {
                self.emit(Insn::Move { dst: r, src: key });
                self.emit(Insn::MakeCell { reg: r });
                Ok(())
            }
            _ => self.init_binding(binding, key),
        }
    }

    /// The body of a loop, its continue target and its exit. For `while`,
    /// `continue_to` is the test; for `for`, `next` holds the update.
    fn loop_body(
        &mut self,
        labels: Vec<NameId>,
        body: StmtId,
        continue_to: Option<usize>,
        exits: Vec<usize>,
        next: Option<(ScopeId, bool, Option<ExprId>, usize)>,
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
    pub(super) fn push_breakable(&mut self, labels: Vec<NameId>, kind: BreakableKind) {
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
            match self.derived_result {
                Some(registers) => self.derived_return(value, registers),
                None => {
                    self.emit(Insn::Return { src: value });
                }
            }
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
        discriminant: ExprId,
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
            self.finally_block(start, FinallyBody::Block(finalizer))?;
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
        // The value of the statement is the catch block's (§14.15.3).
        self.reset_completion();
        self.enter_scope(scope)?;
        if let Some(param) = clause.param {
            self.bind_pattern(param, exception, BindMode::Init, PatternSource::Catch)?;
        }
        for &statement in self.ast.stmts(body) {
            self.stmt(statement)?;
        }
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// Starts the implicit `finally` block that closes the iterator record
    /// at `rec` (see [`FunctionCompiler::finally_block`]); the protected
    /// code starts at the next instruction.
    pub(super) fn push_close(&mut self) -> CResult<usize> {
        let kind = self.temps(2)?;
        self.finallys.push(Finally {
            kind,
            value: kind + 1,
            entries: Vec::new(),
            targets: Vec::new(),
        });
        Ok(self.here())
    }

    /// The `finally` block of the innermost entry of `finallys`, which
    /// protects the code from `start`: the normal completion, the block,
    /// its routes and its handler stub. The block starts at the position
    /// of the next instruction (which sets the normal completion).
    pub(super) fn finally_block(&mut self, start: usize, body: FinallyBody) -> CResult<()> {
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
        match body {
            // A `finally` block that completes normally keeps the value of
            // the `try` statement (§14.15.3): the completion value of
            // script and eval code is saved around it.
            FinallyBody::Block(finalizer) => match self.completion {
                Some(completion) => {
                    let saved = self.temp()?;
                    self.emit(Insn::Move {
                        dst: saved,
                        src: completion,
                    });
                    self.stmt(finalizer)?;
                    self.emit(Insn::Move {
                        dst: completion,
                        src: saved,
                    });
                }
                None => self.stmt(finalizer)?,
            },
            FinallyBody::Close(rec) => {
                self.emit(Insn::IteratorClose { rec, kind });
            }
        }
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

/// Whether a statement's value is `UpdateEmpty(C, undefined)` (§14.6 to
/// §14.15): `undefined` unless a statement inside it produces a value.
/// (`for`-`in` and `for`-`of` set it themselves.)
fn resets_completion(kind: StmtKind) -> bool {
    matches!(
        kind,
        StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. }
            | StmtKind::With { .. }
    )
}
