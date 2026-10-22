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

use swb_js_syntax::messages::{STACK_OVERFLOW, TOO_MANY_ARGUMENTS, TOO_MANY_VARIABLES};
use swb_js_syntax::{
    Ast, BindingId, BindingKind, ExprId, Function, FunctionId, FunctionKind, FunctionScope, NameId,
    Reference, ScopeId, ScopeKind, ScopeTree, Script, StmtId, Storage,
};
use swb_js_text::{RecursionBudget, Str16, String16};

use super::{Builder, CResult, CompileError, ConstSpec, Session};
use crate::bytecode::{
    CodeKind, GlobalDecl, GlobalKind, Handler, Insn, MAX_REGISTERS, Reg, ThisMode,
};

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
    pub(super) labels: Vec<NameId>,
    pub(super) kind: BreakableKind,
    /// The number of enclosing `finally` blocks when the statement
    /// started: a jump to it crosses the blocks above this depth.
    pub(super) finally_depth: usize,
    pub(super) breaks: Vec<usize>,
    pub(super) continues: Vec<usize>,
}

/// A `finally` block while its `try` and `catch` parts compile, or the
/// implicit `finally` block that closes an iterator (the body of a
/// `for`-`of` loop, an array pattern).
pub(super) struct Finally {
    /// The register of the completion kind.
    pub(super) kind: Reg,
    /// The register of the completion value.
    pub(super) value: Reg,
    /// The jumps to the block (patched to its first instruction).
    pub(super) entries: Vec<usize>,
    /// The jump targets that pass through the block: (breakable, whether
    /// it is a `continue`). Target `k` has the kind `COMPLETION_JUMP + k`.
    pub(super) targets: Vec<(usize, bool)>,
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
    /// `AsyncIteratorClose` of the iterator record at the register (`for
    /// await`): the result of `return` is awaited.
    AsyncClose(Reg),
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
    /// The key texts of the sites, each once, and their indices.
    site_keys: Vec<String16>,
    site_key_map: HashMap<String16, u32>,
    /// Per site, the index of its key.
    sites: Vec<u32>,
    /// The first register above the declared bindings.
    first_temp: u32,
    next_reg: u32,
    max_reg: u32,
    pub(super) breakables: Vec<Breakable>,
    pub(super) pending_labels: Vec<NameId>,
    /// The `finally` blocks around the code being compiled, innermost
    /// last.
    pub(super) finallys: Vec<Finally>,
    /// The exception handlers, in the order of their creation.
    pub(super) handlers: Vec<Handler>,
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
    pub(super) derived_result: Option<(Reg, Reg)>,
    /// The jumps of a derived constructor's returns to its epilogue.
    pub(super) derived_exits: Vec<usize>,
    /// The direct `eval` calls: (instruction, scope of the call).
    pub(super) eval_sites: Vec<(u32, ScopeId)>,
    /// The kind of the code (set when `compile` starts).
    pub(super) kind: CodeKind,
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
            site_keys: Vec::new(),
            site_key_map: HashMap::new(),
            sites: Vec::new(),
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
            kind: CodeKind::Normal,
        }
    }

    /// Compiles the function.
    pub(super) fn compile(mut self) -> CResult<Builder> {
        if self.fscope.register_count > MAX_REGISTERS {
            return Err(self.too_many_variables());
        }
        let kind = self.code_kind()?;
        self.kind = kind;
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
        let mut body_start = self.here();
        self.prologue(kind)?;
        // An async generator's parameters throw to the caller (§15.6.2);
        // its body after `InitialYield` rejects requests.
        if kind == CodeKind::AsyncGenerator {
            body_start = self.here();
        }
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
        if kind.is_async() {
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
            site_keys: self.site_keys,
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
        Ok(match function.kind {
            // Indirect eval code is global code with a top scope of its own.
            FunctionKind::Script if self.scopes.scope(function.scope).kind == ScopeKind::Eval => {
                CodeKind::Eval
            }
            FunctionKind::Script => CodeKind::Script,
            FunctionKind::Eval => CodeKind::Eval,
            _ if function.is_generator && function.is_async => CodeKind::AsyncGenerator,
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
        CompileError::unsupported(self.position, construct)
    }

    /// More variables than registers (V8 accepts more; the register
    /// operand has 16 bits).
    fn too_many_variables(&self) -> CompileError {
        CompileError::syntax(self.position, TOO_MANY_VARIABLES)
    }

    /// The temporaries of an expression do not fit the registers left.
    fn too_many_registers(&self) -> CompileError {
        CompileError::syntax(
            self.position,
            format!(
                "Expression needs too many registers (only {MAX_REGISTERS} allowed in a function)"
            ),
        )
    }

    /// The count of call arguments as the 16-bit operand of the call
    /// instructions.
    pub(super) fn argument_count(&self, count: usize) -> CResult<u16> {
        u16::try_from(count).map_err(|_| CompileError::syntax(self.position, TOO_MANY_ARGUMENTS))
    }

    pub(super) fn internal(message: &'static str) -> CompileError {
        CompileError::Heap(crate::error::Error::invariant(message))
    }

    // --- Recursion budget ---

    pub(super) fn enter(&mut self, weight: u32) -> CResult<()> {
        self.budget
            .enter(weight)
            .map_err(|_| CompileError::range(self.position, STACK_OVERFLOW))
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

    /// A new property site with a key text. Every access gets its own
    /// site, because the site holds the access's inline cache.
    pub(super) fn site(&mut self, text: String16) -> u32 {
        let key = if let Some(&key) = self.site_key_map.get(&text) {
            key
        } else {
            let key = self.site_keys.len() as u32;
            self.site_keys.push(text.clone());
            self.site_key_map.insert(text, key);
            key
        };
        let index = self.sites.len() as u32;
        self.sites.push(key);
        index
    }

    /// A new site with the key of `site` (the write of a compound
    /// assignment, whose read uses `site`).
    pub(super) fn site_like(&mut self, site: u32) -> u32 {
        let Some(&key) = self.sites.get(site as usize) else {
            return site;
        };
        let index = self.sites.len() as u32;
        self.sites.push(key);
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
                    _ => {
                        let site = self.site(self.name_text(b.name));
                        Insn::SetGlobal { src, site }
                    }
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
    pub(super) fn block_function_var(&mut self, function: FunctionId) -> CResult<()> {
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
                let site = self.site(self.name_text(self.scopes.binding(b).name));
                self.emit(Insn::SetGlobal { src: value, site });
                self.patch_here(skip);
            }
            _ => self.store_ref(target, value)?,
        }
        self.release(mark);
        Ok(())
    }

    /// The per-iteration copies of a `for` head's `let` cells (§14.7.4.4
    /// `CreatePerIterationEnvironment`).
    pub(super) fn copy_cells(&mut self, scope: ScopeId) {
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
        if matches!(kind, CodeKind::Generator | CodeKind::AsyncGenerator) {
            self.emit(Insn::InitialYield);
        }
        Ok(())
    }

    /// The handler of an async function's whole code after `AsyncStart`
    /// (the parameters and the body, §15.8.4 step 3 and §27.7.5.2 step
    /// 3.h): a throw that nothing in it catches rejects the result
    /// promise. `start` is the first instruction after `AsyncStart`. In
    /// an async generator the handler covers the body after `InitialYield`
    /// (§27.6.3.2): the throw rejects the request at the head of the
    /// queue.
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
}
