//! Bytecode (ADR 0026 section 4): the instruction set of the register
//! machine, the code objects and the verifier.
//!
//! A function's frame is a window of registers in the value stack: the
//! parameters (registers `0..param_count`), the local bindings that the
//! scope analysis assigned, then the temporaries of the compiler. An
//! instruction is a value of [`Insn`] with typed operands: registers are
//! `u16`, constant, site, capture and name indices `u32`, jump offsets
//! `i32` (relative to the jump instruction). A test keeps an instruction
//! at 12 bytes or less.
//!
//! Instruction families:
//!
//! - loads and moves (`Undefined` to `Move`);
//! - bindings: TDZ checks, cells, captures, `this`, the callee and
//!   `arguments`;
//! - globals, accessed by name (§9.1.1.4);
//! - properties: named access through a property site, keyed access,
//!   `delete`, `in`;
//! - literals: objects and arrays;
//! - operators (three registers or two);
//! - control flow: jumps (a backward jump is a safepoint);
//! - calls: `Call` and `New` take a window of consecutive registers
//!   `callee, this, arg0, ...`; the result goes into the callee register;
//! - errors, the end of a `finally` block, and the generator suspension
//!   and resumption points.
//!
//! Exceptions (ADR 0026 section 5): the handler table maps ranges of
//! instructions to a handler and the register that receives the caught
//! value. A `finally` block keeps the pending completion in two
//! registers, its kind ([`COMPLETION_NORMAL`] and the others) and its
//! value; [`Insn::EndFinally`] continues it.
//!
//! The code object ([`FunctionCode`], held by [`CodeObject`] in the heap)
//! holds the instructions, the constant pool (strings, numbers, nested
//! function code), the property sites, the capture sources of closures,
//! the line table, the handler table and the
//! global declarations of a script. The collector traces its constants
//! through [`GenericData`].

use std::rc::Rc;

use swb_js_syntax::CaptureSource;
use swb_js_text::String16;

use crate::heap::{Gc, Generic, GenericData, Tracer};
use crate::string::{JsString, PropertyKey};
use crate::value::Value;

/// A register operand: an index into the frame's register window.
pub(crate) type Reg = u16;

/// The most registers of one frame (the operand width).
pub(crate) const MAX_REGISTERS: u32 = 65_535;

/// The completion kinds of a `finally` block (the kind register): the
/// block was entered normally, by a throw (the value register holds the
/// exception), by a `return` (the value register holds the result), or by
/// a `break` or `continue` to the jump target `kind - COMPLETION_JUMP` of
/// the block's table.
pub(crate) const COMPLETION_NORMAL: i32 = 0;
pub(crate) const COMPLETION_THROW: i32 = 1;
pub(crate) const COMPLETION_RETURN: i32 = 2;
pub(crate) const COMPLETION_JUMP: i32 = 3;

/// The resume modes of a generator (the register after a `yield`'s value
/// register): `next(v)`, `throw(e)`, `return(v)` (§27.5.3.3, §27.5.3.4).
pub(crate) const RESUME_NEXT: i32 = 0;
pub(crate) const RESUME_THROW: i32 = 1;
pub(crate) const RESUME_RETURN: i32 = 2;

/// One instruction. `dst` is the register that receives the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Insn {
    // --- Loads and moves ---
    /// `dst = undefined`.
    Undefined {
        dst: Reg,
    },
    /// `dst = null`.
    Null {
        dst: Reg,
    },
    /// `dst = value`.
    Bool {
        dst: Reg,
        value: bool,
    },
    /// `dst = value` (an integer Number).
    Int {
        dst: Reg,
        value: i32,
    },
    /// `dst = constants[index]`.
    Const {
        dst: Reg,
        index: u32,
    },
    /// `dst = Empty` (the uninitialized marker of the temporal dead zone).
    Empty {
        dst: Reg,
    },
    /// `dst = src`.
    Move {
        dst: Reg,
        src: Reg,
    },

    // --- Bindings ---
    /// Throws a `ReferenceError` if `src` holds `Empty`; `name` is the
    /// constant with the binding's name.
    CheckTdz {
        src: Reg,
        name: u32,
    },
    /// `dst` = a new cell holding `Empty` (if `empty`) or `undefined`.
    NewCell {
        dst: Reg,
        empty: bool,
    },
    /// `reg` = a new cell holding the value of `reg` (a parameter or a
    /// value computed at function entry).
    MakeCell {
        reg: Reg,
    },
    /// `reg` = a new cell with the value of the cell in `reg` (the
    /// per-iteration copy of a `for (let ...)` binding, §14.7.4.4).
    CopyCell {
        reg: Reg,
    },
    /// `dst` = the value of the cell in `cell`.
    LoadCell {
        dst: Reg,
        cell: Reg,
    },
    /// The cell in `cell` gets the value of `src`.
    StoreCell {
        cell: Reg,
        src: Reg,
    },
    /// `dst` = the value of the function's captured cell `index`.
    LoadCapture {
        dst: Reg,
        index: u32,
    },
    /// The function's captured cell `index` gets the value of `src`.
    StoreCapture {
        src: Reg,
        index: u32,
    },
    /// `dst` = the `this` value of the frame.
    This {
        dst: Reg,
    },
    /// `dst` = the running function (the binding of a named function
    /// expression).
    Callee {
        dst: Reg,
    },
    /// `dst` = a new (unmapped) arguments object.
    Arguments {
        dst: Reg,
    },

    // --- Globals (by name; `name` is a string constant) ---
    /// `GlobalDeclarationInstantiation` (§16.1.7) for the script's
    /// [`FunctionCode::globals`], except the function values.
    DeclareGlobals,
    /// `dst` = the global binding `name`; `ReferenceError` if missing.
    GetGlobal {
        dst: Reg,
        name: u32,
    },
    /// `dst` = `typeof name` for a global name (no error if missing).
    TypeofGlobal {
        dst: Reg,
        name: u32,
    },
    /// `PutValue` on the global binding `name`.
    SetGlobal {
        src: Reg,
        name: u32,
    },
    /// Initializes the global lexical binding `name`.
    InitGlobalLexical {
        src: Reg,
        name: u32,
    },
    /// `CreateGlobalFunctionBinding` (§9.1.1.4.18) with the value of `src`.
    InitGlobalFunction {
        src: Reg,
        name: u32,
    },
    /// `dst` = `delete name` for a global name.
    DeleteGlobal {
        dst: Reg,
        name: u32,
    },

    // --- Properties ---
    /// `dst = obj[sites[site]]`.
    GetNamed {
        dst: Reg,
        obj: Reg,
        site: u32,
    },
    /// `obj[sites[site]] = src`.
    SetNamed {
        obj: Reg,
        src: Reg,
        site: u32,
    },
    /// `dst = obj[key]`.
    GetIndex {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    /// `obj[key] = src`.
    SetIndex {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// `dst = delete obj[sites[site]]`.
    DeleteNamed {
        dst: Reg,
        obj: Reg,
        site: u32,
    },
    /// `dst = delete obj[key]`.
    DeleteIndex {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    /// `dst = key in obj`.
    In {
        dst: Reg,
        key: Reg,
        obj: Reg,
    },
    /// `dst = ToPropertyKey(src)`, as a string or symbol value (or an
    /// integer for an array index).
    ToKey {
        dst: Reg,
        src: Reg,
    },

    // --- Literals ---
    /// `dst` = a new ordinary object.
    NewObject {
        dst: Reg,
    },
    /// `dst` = a new array with room for `capacity` elements.
    NewArray {
        dst: Reg,
        capacity: u32,
    },
    /// CreateDataPropertyOrThrow(obj, sites[site], src).
    DefineNamed {
        obj: Reg,
        src: Reg,
        site: u32,
    },
    /// CreateDataPropertyOrThrow(obj, ToPropertyKey(key), src).
    DefineIndex {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// Appends `src` to the array literal `array`.
    Push {
        array: Reg,
        src: Reg,
    },
    /// Appends a hole to the array literal `array`.
    Hole {
        array: Reg,
    },
    /// Sets the prototype of the object literal `obj` (`__proto__: src`).
    SetProto {
        obj: Reg,
        src: Reg,
    },

    // --- Binary operators: dst = a op b ---
    Add {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Sub {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Mul {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Div {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Rem {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Exp {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitAnd {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitOr {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitXor {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Shl {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Shr {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UShr {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Eq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Ne {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    StrictEq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    StrictNe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Lt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Le {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Gt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Ge {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    InstanceOf {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Unary operators: dst = op src ---
    Neg {
        dst: Reg,
        src: Reg,
    },
    ToNumber {
        dst: Reg,
        src: Reg,
    },
    ToNumeric {
        dst: Reg,
        src: Reg,
    },
    BitNot {
        dst: Reg,
        src: Reg,
    },
    Not {
        dst: Reg,
        src: Reg,
    },
    Typeof {
        dst: Reg,
        src: Reg,
    },
    /// `dst = ToNumeric(src) + 1`.
    Inc {
        dst: Reg,
        src: Reg,
    },
    /// `dst = ToNumeric(src) - 1`.
    Dec {
        dst: Reg,
        src: Reg,
    },
    /// `dst = ToString(src)` (template literals).
    ToString {
        dst: Reg,
        src: Reg,
    },

    // --- Control flow (targets are `pc of the jump + offset`) ---
    Jump {
        offset: i32,
    },
    JumpIfTrue {
        cond: Reg,
        offset: i32,
    },
    JumpIfFalse {
        cond: Reg,
        offset: i32,
    },
    /// Jumps if `src` is neither `undefined` nor `null`.
    JumpIfNotNullish {
        src: Reg,
        offset: i32,
    },
    /// Jumps unless `a < b` (a fused compare and branch for loops).
    JumpIfNotLt {
        a: Reg,
        b: Reg,
        offset: i32,
    },
    /// Jumps unless `a <= b`.
    JumpIfNotLe {
        a: Reg,
        b: Reg,
        offset: i32,
    },
    /// Jumps unless `a > b`.
    JumpIfNotGt {
        a: Reg,
        b: Reg,
        offset: i32,
    },
    /// Jumps unless `a >= b`.
    JumpIfNotGe {
        a: Reg,
        b: Reg,
        offset: i32,
    },

    // --- Calls ---
    /// Calls `callee` with `this` in `callee + 1` and `argc` arguments
    /// from `callee + 2`; the result goes into `callee`.
    Call {
        callee: Reg,
        argc: u16,
    },
    /// `new callee(...)` with the arguments from `callee + 2`; the result
    /// goes into `callee`.
    New {
        callee: Reg,
        argc: u16,
    },
    /// Returns the value of `src`.
    Return {
        src: Reg,
    },
    /// `dst` = a new closure of the function code `constants[index]`.
    Closure {
        dst: Reg,
        index: u32,
    },

    // --- Errors ---
    /// Throws the value of `src`.
    Throw {
        src: Reg,
    },
    /// Throws the `TypeError` of an assignment to a constant binding.
    ThrowConstAssign {
        name: u32,
    },
    /// Throws the `ReferenceError` of an assignment to a call expression
    /// (Annex B web compatibility; Chromium throws at run time).
    ThrowInvalidAssign,

    /// The end of a `finally` block: continues the completion in `kind`
    /// and `value`. Normal: jumps by `offset` (past the block's table).
    /// Throw: throws `value`. Return and jumps: continues at the entry
    /// `kind - COMPLETION_RETURN` of the table of jumps that follows the
    /// instruction (the table ends before `offset`).
    EndFinally {
        kind: Reg,
        value: Reg,
        offset: i32,
    },

    // --- Generators ---
    /// The end of a generator's prologue: creates the generator object,
    /// suspends the frame and returns the object to the caller.
    InitialYield,
    /// Yields the value of `reg` (as `{ value, done: false }`); on resume,
    /// `reg` receives the value that was sent and `reg + 1` the resume
    /// mode ([`RESUME_NEXT`] and the others).
    Yield {
        reg: Reg,
    },
    /// The dispatch after a `yield` on the resume mode in `reg + 1`: next
    /// jumps by `offset`, throw throws the value in `reg`, return
    /// continues with the next instruction (the compiled return path,
    /// which runs the `finally` blocks).
    Resume {
        reg: Reg,
        offset: i32,
    },

    /// Does nothing (`debugger`).
    Nop,
}

impl Insn {
    /// The jump offset of a jump instruction.
    pub(crate) fn jump_offset(&self) -> Option<i32> {
        match *self {
            Insn::Jump { offset }
            | Insn::JumpIfTrue { offset, .. }
            | Insn::JumpIfFalse { offset, .. }
            | Insn::JumpIfNotNullish { offset, .. }
            | Insn::JumpIfNotLt { offset, .. }
            | Insn::JumpIfNotLe { offset, .. }
            | Insn::JumpIfNotGt { offset, .. }
            | Insn::JumpIfNotGe { offset, .. }
            | Insn::EndFinally { offset, .. }
            | Insn::Resume { offset, .. } => Some(offset),
            _ => None,
        }
    }

    /// Sets the jump offset of a jump instruction (the compiler patches
    /// forward jumps).
    pub(crate) fn set_jump_offset(&mut self, new: i32) {
        match self {
            Insn::Jump { offset }
            | Insn::JumpIfTrue { offset, .. }
            | Insn::JumpIfFalse { offset, .. }
            | Insn::JumpIfNotNullish { offset, .. }
            | Insn::JumpIfNotLt { offset, .. }
            | Insn::JumpIfNotLe { offset, .. }
            | Insn::JumpIfNotGt { offset, .. }
            | Insn::JumpIfNotGe { offset, .. }
            | Insn::EndFinally { offset, .. }
            | Insn::Resume { offset, .. } => *offset = new,
            _ => {}
        }
    }

    /// Whether control never continues with the next instruction.
    fn is_terminator(&self) -> bool {
        matches!(
            self,
            Insn::Jump { .. }
                | Insn::Return { .. }
                | Insn::Throw { .. }
                | Insn::ThrowConstAssign { .. }
                | Insn::ThrowInvalidAssign
        )
    }
}

/// An entry of the constant pool.
#[derive(Clone, Debug)]
pub(crate) enum Constant {
    /// A string (an atom) or a number.
    Value(Value),
    /// The code of a nested function: its code object and the shared code.
    Function(Gc<Generic>, Rc<FunctionCode>),
}

/// The kind of a function's code: how it is called and what its function
/// objects have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeKind {
    /// The top-level code of a script.
    Script,
    /// A function declaration or expression: a constructor with a
    /// `prototype` object.
    Normal,
    /// An arrow function: lexical `this`, not a constructor.
    Arrow,
    /// A method of an object literal: not a constructor.
    Method,
    /// A generator function: not a constructor; its `prototype` object
    /// inherits from the generator prototype.
    Generator,
}

/// How a function binds `this` (§10.2.1.2 `OrdinaryCallBindThis`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThisMode {
    /// Arrow functions: `this` comes from the enclosing function.
    Lexical,
    /// Strict code: `this` as passed.
    Strict,
    /// Sloppy code: `undefined` and `null` become the global object,
    /// primitives become wrapper objects.
    Global,
}

/// A range of instructions with a handler. The table of a code object is
/// sorted by `start`, and for equal starts the outer range comes first;
/// two ranges are disjoint or nested, so the last range that contains a
/// position is the innermost one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Handler {
    /// The first instruction of the range.
    pub(crate) start: u32,
    /// The instruction after the range.
    pub(crate) end: u32,
    /// The first instruction of the handler.
    pub(crate) target: u32,
    /// The register that receives the caught value.
    pub(crate) register: Reg,
}

/// The kinds of global declarations of a script (§16.1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalKind {
    /// `var`: a property of the global object.
    Var,
    /// A function declaration: a property of the global object, set by
    /// [`Insn::InitGlobalFunction`].
    Function,
    /// `let`: a mutable binding of the global lexical record.
    Let,
    /// `const`: an immutable binding of the global lexical record.
    Const,
}

/// A global declaration of a script: the name (a string constant index)
/// and its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalDecl {
    pub(crate) name: u32,
    pub(crate) kind: GlobalKind,
}

/// The compiled code of one function (or of a script), shared by all its
/// closures and by all realms of a runtime. Immutable after the compile.
#[derive(Debug)]
pub(crate) struct FunctionCode {
    pub(crate) insns: Box<[Insn]>,
    pub(crate) constants: Box<[Constant]>,
    /// The keys of the property sites (later with inline caches).
    pub(crate) sites: Box<[PropertyKey]>,
    /// Where a closure of this code gets each captured cell.
    pub(crate) captures: Box<[CaptureSource]>,
    /// The global declarations (scripts only).
    pub(crate) globals: Box<[GlobalDecl]>,
    /// The size of the register window.
    pub(crate) register_count: u32,
    /// The number of declared parameters (the `length` of the function).
    pub(crate) param_count: u32,
    pub(crate) kind: CodeKind,
    pub(crate) this_mode: ThisMode,
    pub(crate) strict: bool,
    /// Whether the code creates an `arguments` object (the frame keeps
    /// the arguments beyond the declared parameters).
    pub(crate) uses_arguments: bool,
    /// The name of the function (the empty string for anonymous ones).
    pub(crate) name: Gc<JsString>,
    /// (first instruction, source offset), sorted by instruction.
    pub(crate) lines: Box<[(u32, u32)]>,
    /// The exception handlers.
    pub(crate) handlers: Box<[Handler]>,
    /// (instruction, constant index of the callee text) for the messages
    /// of `Call` and `New` ("f is not a function"), sorted.
    pub(crate) call_names: Box<[(u32, u32)]>,
    /// The source text of the script (shared by all its functions) and the
    /// range of this function in it (code units; for messages and
    /// `Function.prototype.toString`).
    pub(crate) source: Rc<String16>,
    pub(crate) span: (u32, u32),
}

impl FunctionCode {
    /// The source text of the function (the part of the script that
    /// `span` names), lossy for lone surrogates.
    pub(crate) fn source_text(&self) -> Option<String16> {
        let (start, end) = self.span;
        self.source
            .as_str16()
            .slice(start as usize, end as usize)
            .map(swb_js_text::Str16::to_string16)
    }

    /// The bytes that the code owns outside its own record.
    pub(crate) fn heap_size(&self) -> usize {
        self.insns.len() * size_of::<Insn>()
            + self.constants.len() * size_of::<Constant>()
            + self.sites.len() * size_of::<PropertyKey>()
            + self.captures.len() * size_of::<CaptureSource>()
            + self.globals.len() * size_of::<GlobalDecl>()
            + self.lines.len() * 8
            + self.handlers.len() * size_of::<Handler>()
            + self.call_names.len() * 8
            // Only the script's code owns the source text for the heap
            // limit; the functions share it.
            + if self.kind == CodeKind::Script {
                self.source.len() * 2
            } else {
                0
            }
    }

    /// The source offset of the instruction at `pc`.
    pub(crate) fn source_offset(&self, pc: usize) -> Option<u32> {
        let pc = pc as u32;
        let index = self.lines.partition_point(|&(start, _)| start <= pc);
        index
            .checked_sub(1)
            .and_then(|i| self.lines.get(i))
            .map(|&(_, offset)| offset)
    }

    /// The innermost handler whose range contains `pc`.
    pub(crate) fn handler_at(&self, pc: usize) -> Option<Handler> {
        let pc = u32::try_from(pc).ok()?;
        // The ranges that start at or before `pc`; the last one that
        // still contains it is the innermost.
        let candidates = self.handlers.partition_point(|h| h.start <= pc);
        self.handlers
            .get(..candidates)?
            .iter()
            .rev()
            .find(|h| pc < h.end)
            .copied()
    }

    /// The constant with the callee text of the call at `pc`.
    pub(crate) fn call_name(&self, pc: usize) -> Option<Value> {
        let pc = pc as u32;
        let index = self
            .call_names
            .binary_search_by_key(&pc, |&(at, _)| at)
            .ok()?;
        let (_, constant) = *self.call_names.get(index)?;
        match self.constants.get(constant as usize)? {
            Constant::Value(value) => Some(*value),
            Constant::Function(..) => None,
        }
    }

    /// The string constant at `index`.
    pub(crate) fn string(&self, index: u32) -> Option<Gc<JsString>> {
        match self.constants.get(index as usize)? {
            Constant::Value(Value::String(string)) => Some(*string),
            _ => None,
        }
    }
}

/// The heap form of a code object: a generic heap thing that traces the
/// constants of its code.
pub(crate) struct CodeObject(pub(crate) Rc<FunctionCode>);

impl GenericData for CodeObject {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        let code = &self.0;
        for constant in &code.constants {
            match constant {
                Constant::Value(value) => tracer.value(*value),
                Constant::Function(child, _) => tracer.generic(*child),
            }
        }
        for key in &code.sites {
            tracer.key(*key);
        }
        tracer.string(code.name);
    }

    fn heap_size(&self) -> usize {
        size_of::<FunctionCode>() + self.0.heap_size()
    }
}

/// A listing of a function's code and of its nested functions, in the
/// order of their constants: one instruction per line with its position.
pub(crate) fn disassemble(code: &Rc<FunctionCode>, heap: &crate::heap::Heap) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let mut work = vec![Rc::clone(code)];
    while let Some(code) = work.pop() {
        let name = heap
            .string(code.name)
            .map(|s| s.as_str16().to_string_lossy())
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "function {name:?}: {:?}, {} registers, {} parameters",
            code.kind, code.register_count, code.param_count
        );
        for (pc, insn) in code.insns.iter().enumerate() {
            let _ = writeln!(out, "  {pc:4} {insn:?}");
        }
        let mut children: Vec<Rc<FunctionCode>> = code
            .constants
            .iter()
            .filter_map(|c| match c {
                Constant::Function(_, child) => Some(Rc::clone(child)),
                Constant::Value(_) => None,
            })
            .collect();
        children.reverse();
        work.extend(children);
    }
    out
}

/// A failed check of the verifier: an engine bug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifyError {
    /// The instruction.
    pub(crate) pc: usize,
    /// What is wrong.
    pub(crate) message: &'static str,
}

/// Checks a function's code after code generation (ADR 0026 section 4):
/// register operands below the register count, call windows inside the
/// frame, jump targets inside the code, constant, site and capture
/// indices inside their tables with the right kinds, a last instruction
/// that does not fall off the end, and a consistent handler table.
pub(crate) fn verify(code: &FunctionCode) -> Result<(), VerifyError> {
    let len = code.insns.len();
    let registers = code.register_count;
    if registers > MAX_REGISTERS || code.param_count > registers {
        return Err(VerifyError {
            pc: 0,
            message: "register count",
        });
    }
    if !code.insns.last().is_some_and(Insn::is_terminator) {
        return Err(VerifyError {
            pc: len,
            message: "the code can run past its end",
        });
    }
    for (pc, insn) in code.insns.iter().enumerate() {
        let fail = |message| Err(VerifyError { pc, message });
        let mut ok = true;
        for_each_register(insn, |r| ok &= u32::from(r) < registers);
        if !ok {
            return fail("register operand out of range");
        }
        match *insn {
            Insn::Call { callee, argc } | Insn::New { callee, argc } => {
                if u32::from(callee) + 2 + u32::from(argc) > registers {
                    return fail("call window outside the frame");
                }
            }
            Insn::Const { index, .. } => {
                if !matches!(code.constants.get(index as usize), Some(Constant::Value(_))) {
                    return fail("constant index");
                }
            }
            Insn::Closure { index, .. } => {
                if !matches!(
                    code.constants.get(index as usize),
                    Some(Constant::Function(..))
                ) {
                    return fail("function constant index");
                }
            }
            Insn::CheckTdz { name, .. }
            | Insn::GetGlobal { name, .. }
            | Insn::TypeofGlobal { name, .. }
            | Insn::SetGlobal { name, .. }
            | Insn::InitGlobalLexical { name, .. }
            | Insn::InitGlobalFunction { name, .. }
            | Insn::DeleteGlobal { name, .. }
            | Insn::ThrowConstAssign { name } => {
                if code.string(name).is_none() {
                    return fail("name constant index");
                }
            }
            Insn::GetNamed { site, .. }
            | Insn::SetNamed { site, .. }
            | Insn::DeleteNamed { site, .. }
            | Insn::DefineNamed { site, .. } => {
                if site as usize >= code.sites.len() {
                    return fail("site index");
                }
            }
            Insn::LoadCapture { index, .. } | Insn::StoreCapture { index, .. } => {
                if index as usize >= code.captures.len() {
                    return fail("capture index");
                }
            }
            Insn::InitialYield | Insn::Yield { .. } | Insn::Resume { .. }
                if code.kind != CodeKind::Generator =>
            {
                return fail("yield outside a generator");
            }
            // The resume mode goes into the register after `reg`.
            Insn::Yield { reg } | Insn::Resume { reg, .. } if u32::from(reg) + 1 >= registers => {
                return fail("resume mode register out of range");
            }
            _ => {}
        }
        if let Some(offset) = insn.jump_offset() {
            let target = pc as i64 + i64::from(offset);
            if target < 0 || target >= len as i64 {
                return fail("jump target outside the code");
            }
        }
    }
    for global in &code.globals {
        if code.string(global.name).is_none() {
            return Err(VerifyError {
                pc: 0,
                message: "global declaration name",
            });
        }
    }
    verify_handlers(code)
}

/// The handler table: ranges inside the code, targets inside the code,
/// registers inside the frame, sorted by start, and two ranges either
/// disjoint or nested.
fn verify_handlers(code: &FunctionCode) -> Result<(), VerifyError> {
    let len = code.insns.len() as u32;
    let mut open: Vec<&Handler> = Vec::new();
    let mut previous_start = 0;
    for handler in &code.handlers {
        let fail = |message| {
            Err(VerifyError {
                pc: handler.start as usize,
                message,
            })
        };
        if handler.start >= handler.end || handler.end > len || handler.target >= len {
            return fail("handler range or target outside the code");
        }
        if u32::from(handler.register) >= code.register_count {
            return fail("handler register out of range");
        }
        if handler.start < previous_start {
            return fail("handler table not sorted");
        }
        previous_start = handler.start;
        while open.last().is_some_and(|outer| outer.end <= handler.start) {
            open.pop();
        }
        if open.last().is_some_and(|outer| handler.end > outer.end) {
            return fail("handler ranges overlap without nesting");
        }
        open.push(handler);
    }
    Ok(())
}

/// Calls `f` with every register operand of an instruction.
#[allow(clippy::too_many_lines, reason = "one arm per operand layout")]
fn for_each_register(insn: &Insn, mut f: impl FnMut(Reg)) {
    use Insn as I;
    match *insn {
        I::Undefined { dst }
        | I::Null { dst }
        | I::Bool { dst, .. }
        | I::Int { dst, .. }
        | I::Const { dst, .. }
        | I::Empty { dst }
        | I::NewCell { dst, .. }
        | I::LoadCapture { dst, .. }
        | I::This { dst }
        | I::Callee { dst }
        | I::Arguments { dst }
        | I::GetGlobal { dst, .. }
        | I::TypeofGlobal { dst, .. }
        | I::DeleteGlobal { dst, .. }
        | I::NewObject { dst }
        | I::NewArray { dst, .. }
        | I::Closure { dst, .. } => f(dst),
        I::MakeCell { reg } | I::CopyCell { reg } | I::Yield { reg } | I::Resume { reg, .. } => {
            f(reg);
        }
        I::CheckTdz { src, .. }
        | I::StoreCapture { src, .. }
        | I::SetGlobal { src, .. }
        | I::InitGlobalLexical { src, .. }
        | I::InitGlobalFunction { src, .. }
        | I::Return { src }
        | I::Throw { src }
        | I::JumpIfNotNullish { src, .. } => f(src),
        I::JumpIfTrue { cond, .. } | I::JumpIfFalse { cond, .. } => f(cond),
        I::Hole { array } => f(array),
        I::Call { callee, .. } | I::New { callee, .. } => f(callee),
        I::Move { dst, src }
        | I::LoadCell { dst, cell: src }
        | I::ToKey { dst, src }
        | I::Neg { dst, src }
        | I::ToNumber { dst, src }
        | I::ToNumeric { dst, src }
        | I::BitNot { dst, src }
        | I::Not { dst, src }
        | I::Typeof { dst, src }
        | I::Inc { dst, src }
        | I::Dec { dst, src }
        | I::ToString { dst, src }
        | I::StoreCell { cell: dst, src }
        | I::EndFinally {
            kind: dst,
            value: src,
            ..
        }
        | I::Push { array: dst, src }
        | I::SetProto { obj: dst, src }
        | I::GetNamed { dst, obj: src, .. }
        | I::SetNamed { obj: dst, src, .. }
        | I::DeleteNamed { dst, obj: src, .. }
        | I::DefineNamed { obj: dst, src, .. }
        | I::JumpIfNotLt { a: dst, b: src, .. }
        | I::JumpIfNotLe { a: dst, b: src, .. }
        | I::JumpIfNotGt { a: dst, b: src, .. }
        | I::JumpIfNotGe { a: dst, b: src, .. } => {
            f(dst);
            f(src);
        }
        I::GetIndex { dst, obj, key }
        | I::DeleteIndex { dst, obj, key }
        | I::In { dst, key, obj } => {
            f(dst);
            f(obj);
            f(key);
        }
        I::SetIndex { obj, key, src } | I::DefineIndex { obj, key, src } => {
            f(obj);
            f(key);
            f(src);
        }
        I::Add { dst, a, b }
        | I::Sub { dst, a, b }
        | I::Mul { dst, a, b }
        | I::Div { dst, a, b }
        | I::Rem { dst, a, b }
        | I::Exp { dst, a, b }
        | I::BitAnd { dst, a, b }
        | I::BitOr { dst, a, b }
        | I::BitXor { dst, a, b }
        | I::Shl { dst, a, b }
        | I::Shr { dst, a, b }
        | I::UShr { dst, a, b }
        | I::Eq { dst, a, b }
        | I::Ne { dst, a, b }
        | I::StrictEq { dst, a, b }
        | I::StrictNe { dst, a, b }
        | I::Lt { dst, a, b }
        | I::Le { dst, a, b }
        | I::Gt { dst, a, b }
        | I::Ge { dst, a, b }
        | I::InstanceOf { dst, a, b } => {
            f(dst);
            f(a);
            f(b);
        }
        I::DeclareGlobals
        | I::Jump { .. }
        | I::ThrowConstAssign { .. }
        | I::ThrowInvalidAssign
        | I::InitialYield
        | I::Nop => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_are_at_most_12_bytes() {
        // ADR 0026 section 4: two registers and one 32-bit operand, or
        // three registers.
        assert!(size_of::<Insn>() <= 12, "{}", size_of::<Insn>());
    }

    fn code(insns: Vec<Insn>, registers: u32) -> FunctionCode {
        FunctionCode {
            insns: insns.into(),
            constants: Box::new([]),
            sites: Box::new([]),
            captures: Box::new([]),
            globals: Box::new([]),
            register_count: registers,
            param_count: 0,
            kind: CodeKind::Normal,
            this_mode: ThisMode::Strict,
            strict: true,
            uses_arguments: false,
            name: Gc::new(0, 0),
            lines: Box::new([]),
            handlers: Box::new([]),
            call_names: Box::new([]),
            source: Rc::new(String16::default()),
            span: (0, 0),
        }
    }

    #[test]
    fn verifier_checks_registers_jumps_and_the_end() {
        let ok = code(
            vec![
                Insn::Int { dst: 0, value: 1 },
                Insn::JumpIfFalse { cond: 0, offset: 2 },
                Insn::Jump { offset: -2 },
                Insn::Return { src: 0 },
            ],
            1,
        );
        assert_eq!(verify(&ok), Ok(()));
        let bad_register = code(vec![Insn::Return { src: 1 }], 1);
        assert_eq!(
            verify(&bad_register).map_err(|e| e.message),
            Err("register operand out of range")
        );
        let bad_jump = code(vec![Insn::Jump { offset: 5 }], 1);
        assert_eq!(
            verify(&bad_jump).map_err(|e| e.message),
            Err("jump target outside the code")
        );
        let falls_off = code(vec![Insn::Undefined { dst: 0 }], 1);
        assert!(verify(&falls_off).is_err());
        let bad_call = code(
            vec![Insn::Call { callee: 0, argc: 1 }, Insn::Return { src: 0 }],
            2,
        );
        assert!(verify(&bad_call).is_err());
        let bad_constant = code(
            vec![Insn::Const { dst: 0, index: 0 }, Insn::Return { src: 0 }],
            1,
        );
        assert!(verify(&bad_constant).is_err());
    }

    #[test]
    fn verifier_checks_the_handler_table() {
        let mut nested = code(vec![Insn::Nop; 6], 2);
        nested.insns = vec![
            Insn::Nop,
            Insn::Nop,
            Insn::Nop,
            Insn::Nop,
            Insn::Nop,
            Insn::Return { src: 0 },
        ]
        .into();
        nested.handlers = vec![
            Handler {
                start: 0,
                end: 4,
                target: 4,
                register: 1,
            },
            Handler {
                start: 1,
                end: 2,
                target: 3,
                register: 1,
            },
        ]
        .into();
        assert_eq!(verify(&nested), Ok(()));
        nested.handlers = vec![
            Handler {
                start: 0,
                end: 3,
                target: 4,
                register: 1,
            },
            Handler {
                start: 2,
                end: 4,
                target: 4,
                register: 1,
            },
        ]
        .into();
        assert!(verify(&nested).is_err());
        nested.handlers = vec![Handler {
            start: 0,
            end: 3,
            target: 9,
            register: 1,
        }]
        .into();
        assert!(verify(&nested).is_err());
    }

    #[test]
    fn the_innermost_handler_wins() {
        let mut table = code(vec![Insn::Nop; 8], 2);
        let handler = |start, end, target| Handler {
            start,
            end,
            target,
            register: 1,
        };
        // Sorted by start, the outer range first for equal starts.
        table.handlers = vec![
            handler(0, 6, 7),
            handler(0, 3, 6),
            handler(1, 2, 5),
            handler(4, 5, 5),
        ]
        .into();
        let target = |pc| table.handler_at(pc).map(|h| h.target);
        assert_eq!(target(0), Some(6));
        assert_eq!(target(1), Some(5));
        assert_eq!(target(2), Some(6));
        assert_eq!(target(3), Some(7));
        assert_eq!(target(4), Some(5));
        assert_eq!(target(5), Some(7));
        assert_eq!(target(6), None);
    }

    #[test]
    fn the_verifier_checks_the_resume_registers() {
        let mut generator = code(
            vec![
                Insn::Yield { reg: 0 },
                Insn::Resume { reg: 0, offset: 1 },
                Insn::Return { src: 0 },
            ],
            2,
        );
        generator.kind = CodeKind::Generator;
        assert_eq!(verify(&generator), Ok(()));
        // The mode register `reg + 1` is outside the frame.
        generator.register_count = 1;
        assert!(verify(&generator).is_err());
        // Not in a generator.
        generator.register_count = 2;
        generator.kind = CodeKind::Normal;
        assert!(verify(&generator).is_err());
        // The target of `EndFinally` must be inside the code.
        let finally = code(
            vec![
                Insn::EndFinally {
                    kind: 0,
                    value: 1,
                    offset: 5,
                },
                Insn::Return { src: 0 },
            ],
            2,
        );
        assert!(verify(&finally).is_err());
    }
}
