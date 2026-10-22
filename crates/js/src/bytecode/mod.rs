//! Bytecode (ADR 0026 section 4): the instruction set of the register
//! machine and the code objects. The verifier and the listing are in
//! `verify.rs`, the scope descriptions of functions with a direct `eval`
//! in `eval_info.rs`.
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
//! - literals: objects (data properties, accessors, spread, the names of
//!   functions with computed keys) and arrays, the template objects of
//!   tagged templates;
//! - operators (three registers or two);
//! - control flow: jumps (a backward jump is a safepoint), the jump of an
//!   optional chain, the key iteration of `for`-`in`;
//! - calls: `Call` and `New` take a window of consecutive registers
//!   `callee, this, arg0, ...`; the result goes into the callee register;
//! - errors, the end of a `finally` block, and the generator suspension
//!   and resumption points;
//! - classes and `super`: the class definition (heritage, constructor and
//!   prototype, methods, fields), `super(...)` with `new.target` in the
//!   slot of `this`, the checks of a derived constructor's `this`, and
//!   `super` properties on a pair of registers (the super base, `this`).
//!
//! Exceptions (ADR 0026 section 5): the handler table maps ranges of
//! instructions to a handler and the register that receives the caught
//! value. A `finally` block keeps the pending completion in two
//! registers, its kind ([`COMPLETION_NORMAL`] and the others) and its
//! value; [`Insn::EndFinally`] continues it.
//!
//! The code object ([`FunctionCode`], held by [`CodeObject`] in the heap)
//! holds the instructions, the constant pool (strings, numbers, nested
//! function code, the strings of tagged templates), the property sites,
//! the capture sources of closures, the line table, the handler table and
//! the global declarations of a script. The collector traces its
//! constants through [`GenericData`]; the heap form also keeps the
//! template objects that the code created.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use swb_js_syntax::CaptureSource;
use swb_js_text::String16;

mod eval_info;
mod verify;

pub(crate) use crate::source_text::Source;
pub(crate) use eval_info::{
    EvalFlags, EvalInfo, EvalName, EvalScope, EvalSite, EvalSlot, EvalVarEnv, EvalWalk,
    find_eval_name,
};
pub(crate) use verify::{disassemble, verify};

use crate::heap::{Gc, Generic, GenericData, HandleMap, Tracer};
use crate::object::{Object, SiteCache};
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

/// The prefixes of [`Insn::SetFunctionName`]: none, `get `, `set `.
pub(crate) const NAME_PREFIX_NONE: u32 = 0;
pub(crate) const NAME_PREFIX_GET: u32 = 1;
pub(crate) const NAME_PREFIX_SET: u32 = 2;

/// The contexts of [`Insn::ThrowInvalidAssign`], which select V8's
/// message: an assignment, a prefix or postfix update, the target of a
/// `for`-`in` or `for`-`of` loop.
pub(crate) const INVALID_TARGET_ASSIGN: u32 = 0;
pub(crate) const INVALID_TARGET_PREFIX: u32 = 1;
pub(crate) const INVALID_TARGET_POSTFIX: u32 = 2;
pub(crate) const INVALID_TARGET_FOR: u32 = 3;

/// The forms of V8's message when [`Insn::GetIterator`] finds no
/// iterator (the top four bits of its `hint`; the other bits are the
/// constant index of a rendered source text, or [`HINT_NO_TEXT`]):
/// `number 5 is not iterable (cannot read property
/// Symbol(Symbol.iterator))`, `x is not iterable`, `f is not a function
/// or its return value is not iterable`, for spread arguments `Spread
/// syntax requires ...iterable[Symbol.iterator] to be a function` (`x is
/// not iterable (cannot read property null)` for `undefined` and `null`),
/// and for spread in an array literal `x is not iterable`, also when the
/// iterator's `next` is not callable (V8 checks it there); for the
/// operand of `yield*` that is not a simple expression `yield*
/// (intermediate value) is not iterable`.
pub(crate) const ITER_GENERIC: u32 = 0;
pub(crate) const ITER_RENDERED: u32 = 1;
pub(crate) const ITER_CALL: u32 = 2;
pub(crate) const ITER_SPREAD: u32 = 3;
pub(crate) const ITER_ARRAY_SPREAD: u32 = 4;
pub(crate) const ITER_YIELD_STAR: u32 = 5;
/// The shift of the form in a `hint`.
pub(crate) const HINT_SHIFT: u32 = 28;
/// The text index of a `hint` without a rendered text.
pub(crate) const HINT_NO_TEXT: u32 = (1 << HINT_SHIFT) - 1;
/// The top bit of a `hint`: the record is for an array pattern, which
/// may step a plain array without an Array Iterator
/// (`vm/array_pattern.rs`).
pub(crate) const HINT_PATTERN: u32 = 1 << 31;
/// The state of an iterator record in registers (`rec + 2`) that steps a
/// plain array directly: `ARRAY_PATTERN + i` for the next index `i`; the
/// record's iterator register holds the array.
pub(crate) const ARRAY_PATTERN: i32 = 2;

/// The states of an async `yield*` step in its mode register (`rec + 3`)
/// besides the resume modes ([`Insn::AsyncDelegateCall`]): the close of a
/// delegate without `throw`, and a return mode without a `return` method.
pub(crate) const DELEGATE_CLOSE: i32 = 3;
pub(crate) const DELEGATE_RETURN_MISSING: i32 = 4;

/// The forms of V8's message for a `next`, `throw` or `return` method of
/// a `yield*` delegate that is not callable ([`Insn::DelegateCall`]):
/// `number 5 is not a function` when the operand of `yield*` is a simple
/// expression (an identifier, `this`, a literal, a conditional or logical
/// expression, an optional chain), else `yield* (intermediate value) is
/// not iterable` (measured in Chromium 148).
pub(crate) const DELEGATE_TYPED: u16 = 0;
pub(crate) const DELEGATE_INTERMEDIATE: u16 = 1;

/// The forms of V8's message for [`Insn::RequireObjectCoercible`]
/// (`V` is `undefined` or `null`, `C` the constant text):
/// `Cannot destructure C as it is V.`; `Cannot destructure C'V' as it
/// is V.`; the same with `object null` for `null` (a parameter);
/// `Cannot read properties of VC` (a nested pattern).
pub(crate) const COERCIBLE_RENDERED: u16 = 0;
pub(crate) const COERCIBLE_VALUE: u16 = 1;
pub(crate) const COERCIBLE_PARAMETER: u16 = 2;
pub(crate) const COERCIBLE_READ: u16 = 3;

/// The state register of an iterator record in registers (`rec + 2`, see
/// [`Insn::GetIterator`]) while `next` runs: the record counts as done for
/// `IteratorClose` (a failure of the step does not close the iterator),
/// and [`Insn::IteratorValue`] takes the result object from its `dst`.
pub(crate) const ITERATOR_PENDING: i32 = 1;

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
    /// `dst` = a new arguments object: unmapped (§10.4.4.6), or mapped
    /// (§10.4.4.7) to the parameters whose registers hold cells (the
    /// parameter registers are cells already; with a repeated name only
    /// the last position is).
    Arguments {
        dst: Reg,
        mapped: bool,
    },
    /// `dst` = `new.target` of the frame (`undefined` in a call).
    NewTarget {
        dst: Reg,
    },
    /// `dst` = a new array of the arguments from position `start` on (a
    /// rest parameter, §10.2.11 step 25 with `CreateArrayFromList`).
    RestArguments {
        dst: Reg,
        start: u32,
    },

    // --- Globals (by name; `name` is a string constant, `site` a property
    // site with the name as its key and the cache of the access) ---
    /// `GlobalDeclarationInstantiation` (§16.1.7) for the script's
    /// [`FunctionCode::globals`], except the function values.
    DeclareGlobals,
    /// `dst` = the global binding `sites[site]`; `ReferenceError` if
    /// missing.
    GetGlobal {
        dst: Reg,
        site: u32,
    },
    /// `dst` = `typeof name` for a global name (no error if missing).
    TypeofGlobal {
        dst: Reg,
        name: u32,
    },
    /// `PutValue` on the global binding `sites[site]`.
    SetGlobal {
        src: Reg,
        site: u32,
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
    /// `dst` = whether Annex B.3.2.2 hoists the function declarations in
    /// blocks of a sloppy script into the global var `name` (see
    /// `Runtime::block_function_hoisted`). The evaluation of such a
    /// declaration is `SetGlobal` (`genv.SetMutableBinding(name, F,
    /// false)`) behind a jump on it.
    BlockFunctionHoisted {
        dst: Reg,
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
    /// `dst` = a new ordinary object for an object literal whose
    /// properties are all data properties with constant keys that are not
    /// array indices: it has the final shape of the literal, with the
    /// `count` keys `sites[site..site + count]` in order (each once), and
    /// `undefined` values that [`Insn::InitSlot`] then sets. The shape is
    /// cached in the entry of `sites[site]` (a literal without keys has one
    /// site, with the empty string, for the entry).
    NewLiteral {
        dst: Reg,
        count: u16,
        site: u32,
    },
    /// Sets the named property in `slot` of the object of
    /// [`Insn::NewLiteral`] in `obj` to the value of `src`.
    InitSlot {
        obj: Reg,
        src: Reg,
        slot: u32,
    },
    /// `dst` = a new empty array.
    NewArray {
        dst: Reg,
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
    /// Defines the getter `src` of the key in `key` (a value of `ToKey` or
    /// a string constant) on the object literal `obj`: enumerable and
    /// configurable, merged with an existing setter (§13.2.5.5,
    /// §15.4.5 `MethodDefinitionEvaluation`).
    DefineGetter {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// Defines the setter `src`, like [`Insn::DefineGetter`].
    DefineSetter {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// `SetFunctionName` (§10.2.9) of the closure in `func` with the key
    /// in `key` at run time (computed keys); `prefix` is
    /// [`NAME_PREFIX_NONE`], [`NAME_PREFIX_GET`] or [`NAME_PREFIX_SET`].
    SetFunctionName {
        func: Reg,
        key: Reg,
        prefix: u32,
    },
    /// `CopyDataProperties(obj, src, « »)` (§7.3.25) for object spread:
    /// the own enumerable string and symbol keys of `ToObject(src)` in
    /// order, read with `[[Get]]`; nothing for `undefined` and `null`.
    CopyDataProperties {
        obj: Reg,
        src: Reg,
    },
    /// The object of an object rest element (§8.6.2
    /// `RestBindingInitialization`, §13.15.5.4): `dst` = a new ordinary
    /// object with `CopyDataProperties(dst, src, excluded)`, where the
    /// array in `excluded` holds the keys (values of `ToKey`) of the
    /// pattern's other properties.
    CopyRest {
        dst: Reg,
        src: Reg,
        excluded: Reg,
    },
    /// `dst` = the template object of the template literal
    /// `constants[index]` (§13.2.8.4 `GetTemplateObject`): one frozen
    /// array per site and realm, cached in the code object.
    GetTemplate {
        dst: Reg,
        index: u32,
    },
    /// `dst` = a new `RegExp` object for the regular expression literal
    /// `constants[index]` (§13.2.7.3: each evaluation makes a new object;
    /// the compiled pattern is cached in the constant).
    NewRegExp {
        dst: Reg,
        index: u32,
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
    /// Jumps if `src` is `undefined` or `null` (the links of an optional
    /// chain).
    JumpIfNullish {
        src: Reg,
        offset: i32,
    },
    /// `for`-`in` (§14.7.5.6): `dst` = the key iterator of
    /// `ToObject(src)` (an internal object), or `undefined` if `src` is
    /// `undefined` or `null` (no iterations).
    ForInStart {
        dst: Reg,
        src: Reg,
    },
    /// The next key of the `for`-`in` iterator in `iter` into `dst` (a
    /// string); jumps by `offset` when there is none.
    ForInNext {
        dst: Reg,
        iter: Reg,
        offset: i32,
    },
    /// Jumps if `src` is not `undefined` (the default values of patterns
    /// and parameters).
    JumpIfNotUndefined {
        src: Reg,
        offset: i32,
    },

    // --- Iteration (§7.4; ADR 0026 section 5) ---
    /// `GetIterator(src, sync)` (§7.4.3): an iterator record in three
    /// registers: `dst` = the iterator, `dst + 1` = its `next` method,
    /// `dst + 2` = the state, `false` (not done). The state is `true`
    /// when the record is done, [`ITERATOR_PENDING`] while a step runs.
    /// `hint` selects the message for a value that is not iterable
    /// ([`ITER_GENERIC`] and the others). With [`HINT_PATTERN`], a plain
    /// array whose iteration is the built-in one makes a record in the
    /// state [`ARRAY_PATTERN`] + 0 that holds the array instead of an
    /// iterator; `IteratorCall`, `IteratorValue`, `SpreadArray` and
    /// `IteratorClose` step it directly.
    GetIterator {
        dst: Reg,
        src: Reg,
        hint: u32,
    },
    /// The call of `next` of `IteratorStepValue` (§7.4.10) on the record
    /// at `rec`; nothing if the record is done. For an Array Iterator
    /// whose `next` is the built-in one, the step runs here without a
    /// result object: `dst` = the value, or the record is done. Otherwise
    /// the state becomes [`ITERATOR_PENDING`] and `next` is called (as a
    /// frame if it is script code); its result goes into `dst`.
    IteratorCall {
        dst: Reg,
        rec: Reg,
    },
    /// The rest of `IteratorStepValue`: if the record is done, `dst` =
    /// `undefined` and jumps by `offset`; after [`Insn::IteratorCall`] of
    /// script code, checks the result object in `dst` and reads `done`
    /// (done: the record is done, jumps) and `value` into `dst`. A
    /// failure leaves the state at [`ITERATOR_PENDING`], which
    /// `IteratorClose` treats as done.
    IteratorValue {
        dst: Reg,
        rec: Reg,
        offset: i32,
    },
    /// The bulk copy of a spread or an array rest: if the record at `rec`
    /// is not done and steps a built-in Array Iterator (of values) over an
    /// array without holes, appends the rest of that array to the array
    /// `array` and makes the record and the iterator done, as the steps
    /// would; otherwise nothing (the step loop that follows does the
    /// work). The steps run no script code, so the copy is not observable.
    SpreadArray {
        array: Reg,
        rec: Reg,
    },
    /// `IteratorClose` (§7.4.11) of the record at `rec` unless it is done
    /// (it is done afterwards). `kind` holds a completion kind: after
    /// [`COMPLETION_THROW`] the errors of `return` are dropped (the
    /// thrown value stays in a register of the `finally` block); after
    /// any other kind, they and a result that is not an object throw.
    IteratorClose {
        rec: Reg,
        kind: Reg,
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
    /// A call with spread arguments (§13.3.8.1 `ArgumentListEvaluation`):
    /// `this` in `callee + 1`, an array of the arguments in `callee + 2`
    /// (made by the compiled code, so reading it is not observable); the
    /// result goes into `callee`.
    CallSpread {
        callee: Reg,
    },
    /// `new callee(...)` with spread arguments, like
    /// [`Insn::CallSpread`].
    NewSpread {
        callee: Reg,
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
    /// (Annex B web compatibility; Chromium throws at run time). `context`
    /// is one of the `INVALID_TARGET_*` constants.
    ThrowInvalidAssign {
        context: u32,
    },
    /// `RequireObjectCoercible(src)` (§7.2.1) of an object pattern: a
    /// `TypeError` if `src` is `undefined` or `null`, with V8's message
    /// of `form` ([`COERCIBLE_RENDERED`] and the others) and the text in
    /// the string constant `message`.
    RequireObjectCoercible {
        src: Reg,
        form: u16,
        message: u32,
    },

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
    /// The call of one step of `yield*` (§15.5.5 step 8): the delegate's
    /// iterator is in `rec`, its `next` in `rec + 1`, the received value
    /// in `rec + 2` and the resume mode in `rec + 3`. Calls `next(v)`, or
    /// the iterator's `throw(v)` or `return(v)` (read with `GetMethod`);
    /// the result goes into `rec + 2` (a script frame delivers it there).
    /// In throw mode without a `throw` method, closes the iterator and
    /// throws the protocol `TypeError`; in return mode without a `return`
    /// method, jumps by `offset` with the value in `rec + 2`. `form`
    /// selects V8's message for a method that is not callable
    /// ([`DELEGATE_TYPED`], [`DELEGATE_INTERMEDIATE`]).
    DelegateCall {
        rec: Reg,
        form: u16,
        offset: i32,
    },
    /// The check of a `yield*` step's result in `rec + 2`: it must be an
    /// object; if its `done` is true, `rec + 2` receives its `value`, a
    /// throw mode in `rec + 3` becomes next mode (the delegate handled
    /// the throw), and the instruction jumps by `offset`.
    DelegateResult {
        rec: Reg,
        offset: i32,
    },
    /// `GeneratorYield` of the iterator result in `reg`, which the caller
    /// of `next` receives as it is (§15.5.5 step 8.a.vii); on resume, as
    /// [`Insn::Yield`].
    YieldDelegate {
        reg: Reg,
    },

    // --- Async functions ---
    /// The first instruction of an async function (§15.8.4
    /// `EvaluateAsyncFunctionBody` step 1): creates the result promise and
    /// the object that keeps the frame while it waits at an `await`.
    AsyncStart,
    /// `await` of the value in `reg` (§27.7.5.3 Await): suspends the frame
    /// until the promise of the value settles, and gives the result
    /// promise to the caller. On resume, `reg` receives the value or the
    /// reason and `reg + 1` the mode ([`RESUME_NEXT`] or [`RESUME_THROW`]);
    /// a [`Insn::Resume`] follows.
    Await {
        reg: Reg,
    },
    /// The handler of the whole async function (§27.7.5.2
    /// `AsyncBlockStart` step 3.h): rejects the result promise with the
    /// value in `src` and returns the promise. In an async generator
    /// (§27.6.3.2 `AsyncGeneratorStart`): completes the generator with the
    /// throw completion, rejects the request at the head of its queue and
    /// drains the queue.
    AsyncReject {
        src: Reg,
    },

    // --- Classes (§15.7) and `super` (§13.3.7) ---
    /// `dst` = the home object of the running function (`[[HomeObject]]`,
    /// §10.2.5; `undefined` if it has none).
    HomeObject {
        dst: Reg,
    },
    /// `dst` = a new closure of the method code `constants[index]` whose
    /// home object is the object in `home` (§15.4.4 `DefineMethod`).
    MethodClosure {
        dst: Reg,
        home: Reg,
        index: u32,
    },
    /// The heritage of a class (§15.7.14 step 8): from the value in `src`,
    /// `dst` = the prototype's parent (an object or `null`) and `dst + 1`
    /// = the constructor's parent; the `TypeError`s of a value that is not
    /// a constructor or `null` and of a `prototype` that is not an object
    /// or `null`.
    ClassHeritage {
        dst: Reg,
        src: Reg,
    },
    /// Creates a class (§15.7.14 steps 9 to 17) with the constructor code
    /// `constants[index]`: reads the parents from `dst` and `dst + 1`
    /// (`Empty` in `dst`: a class without heritage), writes the
    /// constructor into `dst` and its `prototype` object into `dst + 1`.
    NewClass {
        dst: Reg,
        index: u32,
    },
    /// Defines the method `src` under the key in `key` (a value of
    /// `ToKey` or a string constant) on `obj`: writable, not enumerable,
    /// configurable (§10.2.8 `DefineMethodProperty`).
    DefineMethod {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// Defines the getter `src` of a class: not enumerable, configurable.
    DefineClassGetter {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// Defines the setter `src` of a class, like
    /// [`Insn::DefineClassGetter`].
    DefineClassSetter {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// `DefineField` (§7.3.33) of a field with the key in `key` (a value
    /// of `ToKey`): `CreateDataPropertyOrThrow(obj, key, src)`.
    DefineField {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    /// `DefineField` of a field with the key of `sites[site]`.
    DefineFieldNamed {
        obj: Reg,
        src: Reg,
        site: u32,
    },
    /// `dst` = element `index` of the computed field keys of the running
    /// initializer function (evaluated when its class was defined).
    FieldKey {
        dst: Reg,
        index: u32,
    },
    /// The initializer function `src` gets the array of its class's
    /// computed field keys in `keys`.
    SetFieldKeys {
        func: Reg,
        keys: Reg,
    },
    /// The class constructor `func` gets its instance field initializer
    /// function `src` (`[[Fields]]`).
    SetClassFields {
        func: Reg,
        src: Reg,
    },
    /// `InitializeInstanceElements(this, func)` (§7.3.34): calls the
    /// field initializer function of the class constructor `func` with the
    /// value of `this` as the receiver; nothing if it has none.
    InitFields {
        this: Reg,
        func: Reg,
    },
    /// `dst` = `func.[[GetPrototypeOf]]()` (§13.3.7.2
    /// `GetSuperConstructor`).
    SuperConstructor {
        dst: Reg,
        func: Reg,
    },
    /// `super(...)` (§13.3.7.1): `Construct(callee, args, newTarget)` with
    /// `newTarget` in `callee + 1` and `argc` arguments from `callee + 2`;
    /// the active function is in `callee - 1` (for the message when the
    /// callee is not a constructor). The result goes into `callee`.
    SuperCall {
        callee: Reg,
        argc: u16,
    },
    /// `super(...)` with spread arguments: an array of the arguments in
    /// `callee + 2`, like [`Insn::CallSpread`].
    SuperCallSpread {
        callee: Reg,
    },
    /// Throws the `ReferenceError` of a derived constructor's `this` before
    /// `super(...)` if `src` holds `Empty`.
    CheckThis {
        src: Reg,
    },
    /// Throws the `ReferenceError` of a second `super(...)` call if `src`
    /// (the `this` binding) does not hold `Empty`.
    CheckSuperOnce {
        src: Reg,
    },
    /// The result of a derived constructor (§10.2.2 steps 10 to 12): an
    /// object in `dst` stays; `undefined` becomes the value of `this`
    /// (which must be initialized); anything else is a `TypeError`.
    DerivedReturn {
        dst: Reg,
        this: Reg,
    },
    /// `dst` = `home.[[GetPrototypeOf]]()` (§9.1.1.3.5 `GetSuperBase`):
    /// an object or `null`.
    SuperBase {
        dst: Reg,
        home: Reg,
    },
    /// `dst` = `base.[[Get]](ToPropertyKey(key), this)` with `this` in
    /// `base + 1` (a `super` property, §13.3.7.3).
    GetSuper {
        dst: Reg,
        base: Reg,
        key: Reg,
    },
    /// [`Insn::GetSuper`] with the key of `sites[site]`.
    GetSuperNamed {
        dst: Reg,
        base: Reg,
        site: u32,
    },
    /// `base.[[Set]](ToPropertyKey(key), src, this)` with `this` in
    /// `base + 1`; a failure throws in strict mode code.
    SetSuper {
        base: Reg,
        key: Reg,
        src: Reg,
    },
    /// [`Insn::SetSuper`] with the key of `sites[site]`.
    SetSuperNamed {
        base: Reg,
        src: Reg,
        site: u32,
    },
    /// Throws the `ReferenceError` of `delete super.x` (§13.5.1.2).
    ThrowSuperDelete,

    // --- Private names (§6.2.12, §7.3.26 to §7.3.32) ---
    /// `dst` = a new private environment with a new Private Name for each
    /// name of the constant `constants[index]` (§15.7.14 steps 6 and 7).
    NewPrivateEnv {
        dst: Reg,
        index: u32,
    },
    /// The private method `src` becomes the value of name `index` of the
    /// private environment `env`.
    PrivateMethod {
        env: Reg,
        src: Reg,
        index: u32,
    },
    /// The getter `src` of the private accessor `index` of `env`.
    PrivateGetter {
        env: Reg,
        src: Reg,
        index: u32,
    },
    /// The setter `src` of the private accessor `index` of `env`.
    PrivateSetter {
        env: Reg,
        src: Reg,
        index: u32,
    },
    /// The class constructor `func` owns the private environment `env`:
    /// static private methods check their receiver against it, and its
    /// instances get the brand of the instance private methods.
    PrivateClass {
        env: Reg,
        func: Reg,
    },
    /// The field initializer function `func` gets the private environment
    /// `env` of its class (for [`Insn::LoadPrivateEnv`]).
    SetPrivateEnv {
        func: Reg,
        env: Reg,
    },
    /// `dst` = the private environment of the running field initializer
    /// function.
    LoadPrivateEnv {
        dst: Reg,
    },
    /// `dst` = `PrivateGet(obj, name)` (§7.3.31) with name `index` of the
    /// private environment `env`; a getter runs as a call.
    GetPrivate {
        dst: Reg,
        obj: Reg,
        env: Reg,
        index: u32,
    },
    /// `PrivateSet(obj, name, src)` (§7.3.32); a setter runs as a call.
    SetPrivate {
        obj: Reg,
        src: Reg,
        env: Reg,
        index: u32,
    },
    /// `dst` = `#name in obj` (§13.10.1): a `TypeError` if `obj` is not an
    /// object.
    HasPrivate {
        dst: Reg,
        obj: Reg,
        env: Reg,
        index: u32,
    },
    /// `PrivateFieldAdd(obj, name, src)` (§7.3.28) of a field definition.
    DefinePrivateField {
        obj: Reg,
        src: Reg,
        env: Reg,
        index: u32,
    },

    // --- Dynamic environments (`vm/env.rs`, ADR 0026 "Dynamic scope") ---
    /// `dst` = the record of a `with` statement for the value of `obj`
    /// (`ToObject`, §14.11.2), linked to the record (or `undefined`) in
    /// `outer`.
    NewWithEnv {
        dst: Reg,
        obj: Reg,
        outer: Reg,
    },
    /// `dst` = a new record for the vars of sloppy direct eval code, linked
    /// to the record (or `undefined`) in `outer`.
    NewVarEnv {
        dst: Reg,
        outer: Reg,
    },
    /// `dst` = the first of `count` records, from the one in `env` on, that
    /// has a binding of the name constant `name` (`HasBinding`), or
    /// `undefined`; [`crate::vm::env::ALL_RECORDS`] walks all records.
    FindBinding {
        dst: Reg,
        env: Reg,
        count: u16,
        name: u32,
    },
    /// `dst` = `GetBindingValue` of the name `name` in the record in
    /// `record`.
    GetBinding {
        dst: Reg,
        record: Reg,
        name: u32,
    },
    /// `SetMutableBinding` of the name `name` in the record in `record` to
    /// the value of `src`.
    SetBinding {
        record: Reg,
        src: Reg,
        name: u32,
    },
    /// `dst` = `DeleteBinding` of the name `name` in the record in
    /// `record`.
    DeleteBinding {
        dst: Reg,
        record: Reg,
        name: u32,
    },
    /// `dst` = `WithBaseObject` of the record in `record` (the `this` of a
    /// call of a name that a `with` object has).
    WithBase {
        dst: Reg,
        record: Reg,
    },
    /// The record of eval vars in `record` gets the binding `name` with
    /// the value of `src` (a function declaration of eval code,
    /// §19.2.1.3 step 17).
    SetVarBinding {
        record: Reg,
        src: Reg,
        name: u32,
    },
    /// A call `eval(...)` (§13.3.6.1): a direct eval if the callee is the
    /// realm's `%eval%`, else an ordinary call; the window as in
    /// [`Insn::Call`].
    CallEval {
        callee: Reg,
        argc: u16,
    },
    /// [`Insn::CallEval`] with spread arguments (the window of
    /// [`Insn::CallSpread`]).
    CallEvalSpread {
        callee: Reg,
    },

    /// Does nothing (`debugger`).
    Nop,

    // --- Async iteration ---
    /// `GetIterator(src, async)` (§7.4.3): an iterator record in three
    /// registers, as [`Insn::GetIterator`]; a sync iterator is wrapped in
    /// an Async-from-Sync Iterator (§27.1.6.1).
    GetAsyncIterator {
        dst: Reg,
        src: Reg,
        hint: u32,
    },
    /// The call of `next` of a `for await` step (§14.7.5.7 step 6.a) on
    /// the record at `rec`: the state becomes [`ITERATOR_PENDING`] and
    /// `next` is called (as a frame if it is script code); its result goes
    /// into `dst`. An [`Insn::Await`] of `dst` and an
    /// [`Insn::IteratorValue`] follow.
    AsyncIteratorCall {
        dst: Reg,
        rec: Reg,
    },
    /// `AsyncGeneratorYield` (§27.6.3.8) of the value in `reg`, which an
    /// `Await` before it has awaited: resolves the request at the head of
    /// the queue with `{ value, done: false }`, then continues with the
    /// next request, or suspends the generator. On resume, `reg` receives
    /// the value and `reg + 1` the mode, as after [`Insn::Yield`]; the
    /// value of a return request is awaited first (§27.6.3.7), and a
    /// rejection resumes in throw mode.
    AsyncYield {
        reg: Reg,
    },
    /// `AsyncGeneratorYield(? IteratorValue(innerResult))` of `yield*` in
    /// an async generator (§15.5.5 step 8.a.vi): reads the `value` of the
    /// iterator result in `reg`, then works as [`Insn::AsyncYield`].
    AsyncYieldDelegate {
        reg: Reg,
    },
    /// The start of `AsyncIteratorClose` (§7.4.13) of the record at `rec`
    /// with the completion kind in `kind`: if the record is not done, it
    /// becomes done, and `return` is read and called (through re-entry);
    /// its result goes into `dst` for the [`Insn::Await`] that follows.
    /// Jumps by `offset` (past the await) when the record is done, when the
    /// iterator has no `return` method, or after a throw completion when
    /// the read or the call fails (the error is dropped).
    AsyncClose {
        rec: Reg,
        kind: Reg,
        dst: Reg,
        offset: i32,
    },
    /// The end of `AsyncIteratorClose` after the await of the result of
    /// `return` in `reg` (the mode in `reg + 1`): after a throw completion
    /// in `kind` nothing; else a rejection throws, and a result that is
    /// not an object is a `TypeError`.
    AsyncCloseEnd {
        reg: Reg,
        kind: Reg,
    },
    /// One step of `yield*` in an async generator (§15.5.5 step 8 with
    /// genKind async) on a record of six registers: the iterator, its
    /// `next`, the received value, the resume mode, and the pair of an
    /// [`Insn::Await`] (value and mode). Calls `next(v)`, `throw(v)` or
    /// `return(v)` by the mode (as a frame if it is script code); the
    /// result goes into `rec + 4`. In throw mode without a `throw` method,
    /// calls `return()` instead, for the close before the protocol error
    /// ([`DELEGATE_CLOSE`] in `rec + 3`; without `return`, throws the
    /// error); in return mode without a `return` method, the received
    /// value goes into `rec + 4` ([`DELEGATE_RETURN_MISSING`]). `form`
    /// selects V8's message for a `return` that is not callable, as for
    /// [`Insn::DelegateCall`].
    AsyncDelegateCall {
        rec: Reg,
        form: u16,
    },
    /// The rest of an async `yield*` step after the await of `rec + 4`: a
    /// rejection throws; the result must be an object; if it is done (or
    /// the delegate has no `return` method), `rec + 2` receives the value
    /// and the instruction jumps by `offset` (to the [`Insn::Resume`] at
    /// the loop's end); else `rec + 2` receives the result object. After
    /// the close of [`DELEGATE_CLOSE`], throws V8's protocol `TypeError`.
    AsyncDelegateResult {
        rec: Reg,
        offset: i32,
    },
}

impl Insn {
    /// The jump offset of a jump instruction.
    pub(crate) fn jump_offset(&self) -> Option<i32> {
        match *self {
            Insn::Jump { offset }
            | Insn::JumpIfTrue { offset, .. }
            | Insn::JumpIfFalse { offset, .. }
            | Insn::JumpIfNotNullish { offset, .. }
            | Insn::JumpIfNullish { offset, .. }
            | Insn::ForInNext { offset, .. }
            | Insn::JumpIfNotUndefined { offset, .. }
            | Insn::IteratorValue { offset, .. }
            | Insn::JumpIfNotLt { offset, .. }
            | Insn::JumpIfNotLe { offset, .. }
            | Insn::JumpIfNotGt { offset, .. }
            | Insn::JumpIfNotGe { offset, .. }
            | Insn::EndFinally { offset, .. }
            | Insn::Resume { offset, .. }
            | Insn::DelegateCall { offset, .. }
            | Insn::AsyncClose { offset, .. }
            | Insn::AsyncDelegateResult { offset, .. }
            | Insn::DelegateResult { offset, .. } => Some(offset),
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
            | Insn::JumpIfNullish { offset, .. }
            | Insn::ForInNext { offset, .. }
            | Insn::JumpIfNotUndefined { offset, .. }
            | Insn::IteratorValue { offset, .. }
            | Insn::JumpIfNotLt { offset, .. }
            | Insn::JumpIfNotLe { offset, .. }
            | Insn::JumpIfNotGt { offset, .. }
            | Insn::JumpIfNotGe { offset, .. }
            | Insn::EndFinally { offset, .. }
            | Insn::Resume { offset, .. }
            | Insn::DelegateCall { offset, .. }
            | Insn::AsyncClose { offset, .. }
            | Insn::AsyncDelegateResult { offset, .. }
            | Insn::DelegateResult { offset, .. } => *offset = new,
            _ => {}
        }
    }

    /// Whether control never continues with the next instruction.
    fn is_terminator(&self) -> bool {
        matches!(
            self,
            Insn::Jump { .. }
                | Insn::Return { .. }
                | Insn::AsyncReject { .. }
                | Insn::Throw { .. }
                | Insn::ThrowConstAssign { .. }
                | Insn::ThrowInvalidAssign { .. }
                | Insn::ThrowSuperDelete
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
    /// The strings of a tagged template (atoms): the cooked string of each
    /// part (`None` for an invalid escape) and its raw string.
    Template(Box<[TemplatePart]>),
    /// The private names of a class body ([`Insn::NewPrivateEnv`]).
    PrivateNames(Box<PrivateNames>),
    /// A regular expression literal ([`Insn::NewRegExp`]).
    RegExp(Box<RegExpLiteral>),
}

/// A regular expression literal: the pattern text (an atom), the flags,
/// and the program, compiled at the first evaluation and shared by the
/// objects that the literal makes.
#[derive(Clone, Debug)]
pub(crate) struct RegExpLiteral {
    /// The source text between the slashes (`[[OriginalSource]]`).
    pub(crate) source: Gc<JsString>,
    /// The flags (checked by the front end).
    pub(crate) flags: swb_js_regexp::Flags,
    /// The compiled pattern, once the literal has been evaluated.
    pub(crate) program: RefCell<Option<Rc<swb_js_regexp::Regex>>>,
}

/// The private names of a class body, in the order of the indices of the
/// private name bindings.
#[derive(Clone, Debug)]
pub(crate) struct PrivateNames {
    /// The name of the class in V8's messages about its private methods:
    /// its binding identifier, else `.` if an expression uses one of its
    /// static private methods or accessors, else `anonymous` (measured in
    /// Node.js 22; the name that `NamedEvaluation` gives does not count).
    pub(crate) class_name: Gc<JsString>,
    /// The names.
    pub(crate) names: Box<[PrivateNameSpec]>,
}

/// One private name of a class body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrivateNameSpec {
    /// The name with its `#` (an atom): the `[[Description]]`.
    pub(crate) description: Gc<JsString>,
    /// What the class declares under the name.
    pub(crate) kind: PrivateKind,
    /// Whether the element is static.
    pub(crate) is_static: bool,
}

/// The kinds of private elements (§6.2.10 `PrivateElement` `[[Kind]]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateKind {
    /// A field: a value on each object that has it.
    Field,
    /// A method: one function for all objects of the class.
    Method,
    /// A getter, a setter, or both.
    Accessor,
}

/// One string part of a tagged template's constant.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TemplatePart {
    /// The template value; `None` if the part has an invalid escape.
    pub(crate) cooked: Option<Gc<JsString>>,
    /// The raw value.
    pub(crate) raw: Gc<JsString>,
}

/// The kind of a function's code: how it is called and what its function
/// objects have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeKind {
    /// The top-level code of a script.
    Script,
    /// The top-level code of eval code (direct or indirect, §19.2.1): its
    /// declarations were instantiated before it runs
    /// (`EvalDeclarationInstantiation`, `vm/eval.rs`); its closure is
    /// internal.
    Eval,
    /// A function declaration or expression: a constructor with a
    /// `prototype` object.
    Normal,
    /// An arrow function: lexical `this`, not a constructor.
    Arrow,
    /// A method of an object literal or a class: not a constructor.
    Method,
    /// A generator function: not a constructor; its `prototype` object
    /// inherits from the generator prototype.
    Generator,
    /// An async function, arrow function or method (§15.8): not a
    /// constructor, no `prototype`; the function object inherits from
    /// `%AsyncFunction.prototype%`.
    Async,
    /// An async generator function or method (§15.6): not a constructor;
    /// its `prototype` object inherits from `%AsyncGeneratorPrototype%`.
    AsyncGenerator,
    /// The constructor of a class without heritage (§15.7.14): `new` only;
    /// the class definition creates its `prototype`.
    ClassConstructor,
    /// The constructor of a derived class: `new` only; `this` is
    /// uninitialized until `super(...)` returns.
    DerivedConstructor,
    /// The function that initializes the fields of a class (instance or
    /// static, with the static blocks): called by the engine only.
    Initializer,
}

impl CodeKind {
    /// Whether the code is an async function or an async generator (the
    /// code that can `await`).
    pub(crate) fn is_async(self) -> bool {
        matches!(self, CodeKind::Async | CodeKind::AsyncGenerator)
    }

    /// Whether the code is a class constructor (`[[IsClassConstructor]]`).
    pub(crate) fn is_class_constructor(self) -> bool {
        matches!(
            self,
            CodeKind::ClassConstructor | CodeKind::DerivedConstructor
        )
    }
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
    /// The var binding of a function declaration in a block that Annex
    /// B.3.2.2 hoists: created only if no lexical declaration has the name
    /// and `CanDeclareGlobalVar` allows it; never an error.
    BlockFunction,
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
    /// The keys of the property sites. Each named access, global access
    /// and object literal has its own site.
    pub(crate) sites: Box<[PropertyKey]>,
    /// The inline cache entries of the sites, one per site; allocated at
    /// the first fill (most functions of a page never run).
    pub(crate) caches: OnceCell<Box<[Cell<SiteCache>]>>,
    /// Where a closure of this code gets each captured cell.
    pub(crate) captures: Box<[CaptureSource]>,
    /// The global declarations (scripts only).
    pub(crate) globals: Box<[GlobalDecl]>,
    /// The size of the register window.
    pub(crate) register_count: u32,
    /// The number of elements of the parameter list: the registers that
    /// receive the arguments.
    pub(crate) param_count: u32,
    /// The `length` of the function: the elements before the first one
    /// with a default value or the rest parameter (§15.1.5
    /// `ExpectedArgumentCount`).
    pub(crate) length: u32,
    pub(crate) kind: CodeKind,
    pub(crate) this_mode: ThisMode,
    pub(crate) strict: bool,
    /// The instruction ranges `[start, end)` of sloppy code that are
    /// strict mode code: the class definitions in it (§11.2.2: all parts
    /// of a class are strict, also the heritage and computed keys that
    /// the enclosing code evaluates). Sorted and disjoint.
    pub(crate) strict_ranges: Box<[(u32, u32)]>,
    /// Whether the frame keeps the arguments beyond the declared
    /// parameters: the code creates an `arguments` object or has a rest
    /// parameter.
    pub(crate) keeps_extra_args: bool,
    /// The name of the function (the empty string for anonymous ones).
    pub(crate) name: Gc<JsString>,
    /// The name that stack traces show for a function without a name:
    /// the dotted path of the member that it is assigned to (`a.b.c = function () {}`),
    /// as Chromium infers it; the empty string if there is none.
    pub(crate) debug_name: Gc<JsString>,
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
    pub(crate) source: Rc<Source>,
    pub(crate) span: (u32, u32),
    /// The scope description of a function with a direct `eval`.
    pub(crate) eval: Option<Box<EvalInfo>>,
}

impl FunctionCode {
    /// Whether the instruction at `pc` is strict mode code.
    pub(crate) fn strict_at(&self, pc: usize) -> bool {
        if self.strict || self.strict_ranges.is_empty() {
            return self.strict;
        }
        let after = self
            .strict_ranges
            .partition_point(|&(start, _)| start as usize <= pc);
        after
            .checked_sub(1)
            .and_then(|i| self.strict_ranges.get(i))
            .is_some_and(|&(_, end)| pc < end as usize)
    }

    /// The source text of the function (the part of the script that
    /// `span` names), lossy for lone surrogates.
    pub(crate) fn source_text(&self) -> Option<String16> {
        let (start, end) = self.span;
        self.source.text.slice(start as usize, end as usize)
    }

    /// The bytes that the code owns outside its own record.
    pub(crate) fn heap_size(&self) -> usize {
        let template_parts: usize = self
            .constants
            .iter()
            .map(|constant| match constant {
                Constant::Template(parts) => parts.len() * size_of::<TemplatePart>(),
                Constant::PrivateNames(names) => {
                    size_of::<PrivateNames>() + names.names.len() * size_of::<PrivateNameSpec>()
                }
                Constant::RegExp(literal) => {
                    size_of::<RegExpLiteral>()
                        + literal
                            .program
                            .borrow()
                            .as_ref()
                            .map_or(0, crate::object::program_share)
                }
                _ => 0,
            })
            .sum();
        self.insns.len() * size_of::<Insn>()
            + self.constants.len() * size_of::<Constant>()
            + template_parts
            + self.sites.len() * size_of::<PropertyKey>()
            + self
                .caches
                .get()
                .map_or(0, |caches| caches.len() * size_of::<Cell<SiteCache>>())
            + self.captures.len() * size_of::<CaptureSource>()
            + self.globals.len() * size_of::<GlobalDecl>()
            + self.lines.len() * 8
            + self.handlers.len() * size_of::<Handler>()
            + self.call_names.len() * 8
            // Only the script's code (or the function of a `Function`
            // call) owns the source text for the heap limit; the
            // functions share it.
            + if self.is_dynamic() || matches!(self.kind, CodeKind::Script | CodeKind::Eval) {
                self.source.text.len() * 2
            } else {
                0
            }
            + self.eval.as_ref().map_or(0, |info| info.heap_size())
    }

    /// Whether this is the function that a `Function` constructor call
    /// made (`CreateDynamicFunction`, §20.2.1.1.1; its source is
    /// `(function anonymous(...`, so it starts at offset 1). It owns its
    /// source for the heap limit (the script around it is garbage once the
    /// function exists), and stack traces do not show its name `anonymous`
    /// (V8).
    pub(crate) fn is_dynamic(&self) -> bool {
        self.source.dynamic_function && self.span.0 == 1
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
            Constant::Function(..)
            | Constant::Template(_)
            | Constant::PrivateNames(_)
            | Constant::RegExp(_) => None,
        }
    }

    /// The cache entry of a site (`Empty` before the first fill).
    #[inline]
    pub(crate) fn cache(&self, site: u32) -> SiteCache {
        self.caches
            .get()
            .and_then(|caches| caches.get(site as usize))
            .map_or(SiteCache::Empty, Cell::get)
    }

    /// Stores the cache entry of a site; allocates the table at the first
    /// fill and returns its bytes then (the caller charges them), else 0.
    /// Without memory for the table nothing is stored.
    pub(crate) fn fill_cache(&self, site: u32, entry: SiteCache) -> usize {
        let mut allocated = 0;
        let caches = if let Some(caches) = self.caches.get() {
            caches
        } else {
            let mut table = Vec::new();
            if table.try_reserve_exact(self.sites.len()).is_err() {
                return 0;
            }
            table.resize(self.sites.len(), Cell::new(SiteCache::Empty));
            allocated = table.len() * size_of::<Cell<SiteCache>>();
            self.caches.get_or_init(|| table.into_boxed_slice())
        };
        if let Some(cell) = caches.get(site as usize) {
            cell.set(entry);
        }
        allocated
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
/// constants of its code. It also holds the template objects of the
/// code's tagged templates (§13.2.8.4: one per site and realm), so they
/// live as long as the code.
pub(crate) struct CodeObject {
    pub(crate) code: Rc<FunctionCode>,
    /// The template objects by (constant index, realm).
    pub(crate) templates: RefCell<HandleMap<(u32, u32), Gc<Object>>>,
}

impl CodeObject {
    /// The code object of compiled code, without template objects.
    pub(crate) fn new(code: Rc<FunctionCode>) -> Self {
        CodeObject {
            code,
            templates: RefCell::new(HandleMap::default()),
        }
    }
}

impl GenericData for CodeObject {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        let code = &self.code;
        for constant in &code.constants {
            match constant {
                Constant::Value(value) => tracer.value(*value),
                Constant::Function(child, _) => tracer.generic(*child),
                Constant::Template(parts) => {
                    for part in parts {
                        if let Some(cooked) = part.cooked {
                            tracer.string(cooked);
                        }
                        tracer.string(part.raw);
                    }
                }
                Constant::PrivateNames(names) => {
                    tracer.string(names.class_name);
                    for name in &names.names {
                        tracer.string(name.description);
                    }
                }
                Constant::RegExp(literal) => tracer.string(literal.source),
            }
        }
        for key in &code.sites {
            tracer.key(*key);
        }
        tracer.string(code.name);
        tracer.string(code.debug_name);
        if let Some(info) = &code.eval {
            info.trace(tracer);
        }
        for &object in self.templates.borrow().values() {
            tracer.object(object);
        }
    }

    fn heap_size(&self) -> usize {
        size_of::<FunctionCode>()
            + self.code.heap_size()
            + self.templates.borrow().capacity() * TEMPLATE_ENTRY_BYTES
    }
}

/// The bytes of one entry of [`CodeObject::templates`] (key, value and
/// the hash table's control byte, rounded up).
pub(crate) const TEMPLATE_ENTRY_BYTES: usize = 24;

#[cfg(test)]
mod tests {
    use super::*;
    use swb_js_text::Str16;

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
            caches: OnceCell::new(),
            captures: Box::new([]),
            globals: Box::new([]),
            register_count: registers,
            param_count: 0,
            length: 0,
            kind: CodeKind::Normal,
            this_mode: ThisMode::Strict,
            strict: true,
            strict_ranges: Box::new([]),
            keeps_extra_args: false,
            name: Gc::new(0, 0),
            debug_name: Gc::new(0, 0),
            lines: Box::new([]),
            handlers: Box::new([]),
            call_names: Box::new([]),
            source: Rc::new(Source::new(Str16::Latin1(&[]), Rc::from(""))),
            span: (0, 0),
            eval: None,
        }
    }

    #[test]
    fn strict_ranges_mark_strict_code_in_sloppy_code() {
        let mut sloppy = code(vec![Insn::Nop; 10], 1);
        sloppy.strict = false;
        sloppy.strict_ranges = Box::new([(2, 4), (6, 7)]);
        let strict: Vec<usize> = (0..10).filter(|&pc| sloppy.strict_at(pc)).collect();
        assert_eq!(strict, [2, 3, 6]);
        sloppy.strict_ranges = Box::new([]);
        assert!(!(0..10).any(|pc| sloppy.strict_at(pc)));
        assert!(code(vec![Insn::Nop], 1).strict_at(0));
    }

    #[test]
    fn verifier_checks_strict_ranges() {
        let insns = vec![Insn::Nop, Insn::Nop, Insn::Return { src: 0 }];
        for (ranges, ok) in [
            (vec![(0, 1), (1, 3)], true),
            (vec![(1, 2), (0, 1)], false),
            (vec![(2, 1)], false),
            (vec![(0, 4)], false),
        ] {
            let mut c = code(insns.clone(), 1);
            c.strict_ranges = ranges.into();
            assert_eq!(verify(&c).is_ok(), ok);
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
    fn the_verifier_checks_the_operands_of_feature_3a() {
        // `ForInNext` jumps; its target must be inside the code.
        let for_in = code(
            vec![
                Insn::ForInStart { dst: 0, src: 0 },
                Insn::ForInNext {
                    dst: 1,
                    iter: 0,
                    offset: 2,
                },
                Insn::Jump { offset: -1 },
                Insn::Return { src: 0 },
            ],
            2,
        );
        assert_eq!(verify(&for_in), Ok(()));
        let mut bad = code(for_in.insns.to_vec(), 2);
        bad.insns = vec![
            Insn::ForInNext {
                dst: 1,
                iter: 0,
                offset: 9,
            },
            Insn::Return { src: 0 },
        ]
        .into();
        assert!(verify(&bad).is_err());
        // A template constant must be a template; a name prefix must be
        // one of the three.
        let template = code(
            vec![
                Insn::GetTemplate { dst: 0, index: 0 },
                Insn::Return { src: 0 },
            ],
            1,
        );
        assert_eq!(
            verify(&template).map_err(|e| e.message),
            Err("template constant index")
        );
        let prefix = code(
            vec![
                Insn::SetFunctionName {
                    func: 0,
                    key: 0,
                    prefix: 3,
                },
                Insn::Return { src: 0 },
            ],
            1,
        );
        assert_eq!(
            verify(&prefix).map_err(|e| e.message),
            Err("function name prefix")
        );
        let registers = code(
            vec![
                Insn::DefineGetter {
                    obj: 0,
                    key: 1,
                    src: 2,
                },
                Insn::Return { src: 0 },
            ],
            2,
        );
        assert!(verify(&registers).is_err());
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
