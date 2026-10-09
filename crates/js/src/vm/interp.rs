//! The interpreter loop (ADR 0026 sections 4 and 5, memo 1.1).
//!
//! A loop around a `match` on the instruction. The program counter, the
//! base of the register window and the code are locals of the loop; they
//! are written back to the frame record before an operation that can
//! call out (calls, accessors, conversions that run script code) or
//! throw, and loaded again after a frame change. Register access is
//! bounds-checked; a failed check is an internal error, never a panic.
//!
//! Invariants between two instructions: the value stack ends at the top
//! of the top frame's window, and the handle scopes are those of the
//! loop's entry. Safepoints run at calls and backward jumps.
//!
//! Errors: an instruction that fails returns the error from the loop; the
//! frame record holds the pc after the failing instruction.
//! [`Runtime::run_frames`] then searches the handler tables: for each
//! frame from the top, it looks up `pc - 1` in the code's table (the
//! innermost range). On a hit it pops the frames above, closes the handle
//! scopes and the no-GC regions down to the loop's entry, cuts the value
//! stack to the top of the frame's window, stores the thrown value in the
//! handler's register (a `Raise` becomes an error object only here) and
//! runs the loop again at the handler. Terminations and internal errors
//! skip the search.

use std::rc::Rc;

use crate::bytecode::{
    COMPLETION_NORMAL, COMPLETION_RETURN, COMPLETION_THROW, Constant, Handler, Insn, RESUME_NEXT,
    RESUME_RETURN, RESUME_THROW,
};
use crate::object::{GetResult, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::call::{CallSite, Step};
use crate::vm::property::{Lookup, SetOutcome};
use crate::vm::{Intrinsic, ReturnTo, VmError, VmResult};

/// What `target[key]` does with the key (the error for an `undefined` or
/// `null` base differs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Delete,
}

impl Runtime {
    /// Runs the frames from the top frame until that frame (whose result
    /// goes to the host) returns; returns its result. An exception that a
    /// handler of these frames catches continues there; on an uncaught
    /// error the frames down to and including the entry frame are removed.
    pub(crate) fn run_frames(&mut self) -> VmResult<Value> {
        let entry = self.vm.frames.len();
        if entry == 0 {
            return Err(VmError::invariant("run without a frame"));
        }
        let scope_depth = self.heap.scope_depth();
        let no_gc = self.heap.no_gc_depth();
        loop {
            let mut error = match self.execute() {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            if matches!(error, VmError::Throw(_) | VmError::Raise { .. })
                && let Some((index, handler)) = self.find_handler(entry - 1)
            {
                match self.enter_handler(index, handler, error, scope_depth, no_gc) {
                    Ok(()) => continue,
                    // Creating the error object failed (the heap limit).
                    Err(failure) => error = failure,
                }
            }
            if self.vm.error_offset.is_none()
                && let Some(frame) = self.vm.frames.last()
            {
                let pc = (frame.pc as usize).saturating_sub(1);
                self.vm.error_offset = frame.compiled.source_offset(pc);
            }
            self.unwind(entry - 1);
            self.heap.close_scopes_to(scope_depth);
            self.heap.restore_no_gc_depth(no_gc);
            return Err(error);
        }
    }

    /// The innermost handler for the instruction that failed in the
    /// frames from the top down to `lowest`: the frame's index and the
    /// handler.
    fn find_handler(&self, lowest: usize) -> Option<(usize, Handler)> {
        let frames = self.vm.frames.get(lowest..)?;
        frames.iter().enumerate().rev().find_map(|(i, frame)| {
            // A frame whose first instruction has not run has no failing
            // instruction.
            let pc = (frame.pc as usize).checked_sub(1)?;
            let handler = frame.compiled.handler_at(pc)?;
            Some((lowest + i, handler))
        })
    }

    /// Continues at a handler of frame `index`: pops the frames above it
    /// (their generators complete), restores the stacks and the regions of
    /// the loop's entry, stores the thrown value.
    fn enter_handler(
        &mut self,
        index: usize,
        handler: Handler,
        error: VmError,
        scope_depth: usize,
        no_gc: u32,
    ) -> VmResult<()> {
        self.unwind(index + 1);
        let frame = self
            .vm
            .frames
            .get(index)
            .ok_or(VmError::invariant("a handler frame without a record"))?;
        let base = frame.base as usize;
        let top = base + frame.compiled.register_count as usize;
        // A deferred call or a native can have left a window above the
        // top frame.
        self.vm.stack.resize(top, Value::Undefined);
        self.heap.close_scopes_to(scope_depth);
        self.heap.restore_no_gc_depth(no_gc);
        // The value is not rooted until it is in the register; only the
        // creation of an error object allocates, and it has no other
        // unrooted value.
        let value = match error {
            VmError::Throw(value) => value,
            VmError::Raise { kind, message } => self.error_object(kind, &message)?.into(),
            other => return Err(other),
        };
        let slot = self
            .vm
            .stack
            .get_mut(base + usize::from(handler.register))
            .ok_or(VmError::invariant("a handler register outside the frame"))?;
        *slot = value;
        if let Some(frame) = self.vm.frames.get_mut(index) {
            frame.pc = handler.target;
        }
        self.vm.error_offset = None;
        Ok(())
    }

    /// The loop.
    #[allow(clippy::too_many_lines)]
    fn execute(&mut self) -> VmResult<Value> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        let mut code = Rc::clone(&frame.compiled);
        let mut pc = frame.pc as usize;
        let mut base = frame.base as usize;

        // Register read and write.
        macro_rules! get {
            ($r:expr) => {
                match self.vm.stack.get(base + usize::from($r)) {
                    Some(&value) => value,
                    None => return Err(VmError::invariant("a register outside the stack")),
                }
            };
        }
        macro_rules! put {
            ($r:expr, $v:expr) => {{
                let value = $v;
                match self.vm.stack.get_mut(base + usize::from($r)) {
                    Some(slot) => *slot = value,
                    None => return Err(VmError::invariant("a register outside the stack")),
                }
            }};
        }
        // Writes the pc back to the frame record.
        macro_rules! save {
            () => {
                if let Some(frame) = self.vm.frames.last_mut() {
                    frame.pc = pc as u32;
                }
            };
        }
        // Unwraps a result; on an error, saves the pc and leaves the loop.
        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(value) => value,
                    Err(error) => {
                        save!();
                        return Err(VmError::from(error));
                    }
                }
            };
        }
        // Loads the locals from the (new) top frame.
        macro_rules! reload {
            () => {{
                let Some(frame) = self.vm.frames.last() else {
                    return Err(VmError::invariant("no frame"));
                };
                code = Rc::clone(&frame.compiled);
                pc = frame.pc as usize;
                base = frame.base as usize;
            }};
        }
        macro_rules! jump {
            ($offset:expr) => {{
                let offset: i32 = $offset;
                let target = (pc as i64 - 1) + i64::from(offset);
                if offset <= 0 {
                    // A backward jump is a safepoint and a step of the
                    // time countdown.
                    if self.heap.gc_due() {
                        tri!(self.heap.safepoint(&self.vm));
                    }
                    self.vm.countdown = self.vm.countdown.wrapping_sub(1);
                    if self.vm.countdown == 0 {
                        tri!(self.check_time());
                    }
                }
                pc = match usize::try_from(target) {
                    Ok(target) => target,
                    Err(_) => return Err(VmError::invariant("a jump before the code")),
                };
            }};
        }
        // Calls an accessor found by a property instruction as a
        // script-to-script call (a deferred call, memo 1.2).
        macro_rules! call_accessor {
            ($callee:expr, $this:expr, $args:expr, $ret:expr) => {{
                save!();
                let args: &[Value] = $args;
                let ret: ReturnTo = $ret;
                let restore_top = base + code.register_count as usize;
                let window = tri!(self.push_window($callee, $this, args));
                let site = CallSite {
                    window,
                    argc: args.len(),
                    ret,
                    restore_top,
                    construct: false,
                };
                match tri!(self.invoke(site)) {
                    Step::Value(value) => {
                        self.vm.stack.truncate(restore_top);
                        if let ReturnTo::Register(register) = ret {
                            put!(register, value);
                        }
                    }
                    Step::Entered => reload!(),
                    Step::NotCallable => {
                        tri!(Err(VmError::invariant("an accessor that is not callable")))
                    }
                }
            }};
        }
        // A property read result: a value or a getter to call.
        macro_rules! lookup {
            ($dst:expr, $lookup:expr) => {{
                match $lookup {
                    Lookup::Value(value) => put!($dst, value),
                    Lookup::Getter { getter, receiver } => {
                        call_accessor!(getter, receiver, &[], ReturnTo::Register($dst))
                    }
                }
            }};
        }
        macro_rules! store {
            ($outcome:expr, $value:expr) => {{
                match $outcome {
                    SetOutcome::Done => {}
                    SetOutcome::Setter { setter, receiver } => {
                        call_accessor!(setter, receiver, &[$value], ReturnTo::Discard)
                    }
                }
            }};
        }
        macro_rules! string_const {
            ($index:expr) => {
                match code.string($index) {
                    Some(name) => name,
                    None => return Err(VmError::invariant("a name constant that is not a string")),
                }
            };
        }
        macro_rules! site {
            ($index:expr) => {
                match code.sites.get($index as usize) {
                    Some(&key) => key,
                    None => return Err(VmError::invariant("a site outside the table")),
                }
            };
        }
        // Integer fast path of an arithmetic operator, else doubles, else
        // the general path.
        macro_rules! arith {
            ($dst:expr, $a:expr, $b:expr, $int:expr, $double:expr, $op:expr) => {{
                let a = get!($a);
                let b = get!($b);
                let result = match (a, b) {
                    (Value::Int(x), Value::Int(y)) => $int(x, y),
                    _ => match (a.as_number(), b.as_number()) {
                        (Some(x), Some(y)) => Value::Double($double(x, y)),
                        _ => {
                            save!();
                            tri!(self.with_scope(|rt| rt.arith_slow($op, a, b)))
                        }
                    },
                };
                put!($dst, result);
            }};
        }
        macro_rules! compare {
            ($a:expr, $b:expr, $int:expr, $double:expr, $slow:expr) => {{
                let a = get!($a);
                let b = get!($b);
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => $int(x, y),
                    _ => match (a.as_number(), b.as_number()) {
                        (Some(x), Some(y)) => $double(x, y),
                        _ => {
                            save!();
                            tri!(self.with_scope(|rt| $slow(rt, a, b)))
                        }
                    },
                }
            }};
        }

        loop {
            let Some(&insn) = code.insns.get(pc) else {
                return Err(VmError::invariant("the pc is outside the code"));
            };
            pc += 1;
            match insn {
                Insn::Undefined { dst } => put!(dst, Value::Undefined),
                Insn::Null { dst } => put!(dst, Value::Null),
                Insn::Bool { dst, value } => put!(dst, Value::Bool(value)),
                Insn::Int { dst, value } => put!(dst, Value::Int(value)),
                Insn::Const { dst, index } => match code.constants.get(index as usize) {
                    Some(Constant::Value(value)) => put!(dst, *value),
                    _ => return Err(VmError::invariant("a constant that is not a value")),
                },
                Insn::Empty { dst } => put!(dst, Value::Empty),
                Insn::Move { dst, src } => put!(dst, get!(src)),

                // --- Bindings ---
                Insn::CheckTdz { src, name } => {
                    if get!(src).is_empty() {
                        let name = string_const!(name);
                        let text = self.name_text(name);
                        tri!(Err(VmError::reference_error(format!(
                            "Cannot access '{text}' before initialization"
                        ))));
                    }
                }
                Insn::NewCell { dst, empty } => {
                    let initial = if empty {
                        Value::Empty
                    } else {
                        Value::Undefined
                    };
                    let cell = tri!(self.heap.alloc_cell(initial));
                    put!(dst, Value::Cell(cell));
                }
                Insn::MakeCell { reg } => {
                    let value = get!(reg);
                    let cell = tri!(self.heap.alloc_cell(value));
                    put!(reg, Value::Cell(cell));
                }
                Insn::CopyCell { reg } => {
                    let Value::Cell(old) = get!(reg) else {
                        return Err(VmError::invariant("CopyCell on a register without a cell"));
                    };
                    let value = tri!(self.heap.cell(old)).value;
                    let cell = tri!(self.heap.alloc_cell(value));
                    put!(reg, Value::Cell(cell));
                }
                Insn::LoadCell { dst, cell } => {
                    let Value::Cell(cell) = get!(cell) else {
                        return Err(VmError::invariant("LoadCell on a register without a cell"));
                    };
                    let value = tri!(self.heap.cell(cell)).value;
                    put!(dst, value);
                }
                Insn::StoreCell { cell, src } => {
                    let Value::Cell(cell) = get!(cell) else {
                        return Err(VmError::invariant("StoreCell on a register without a cell"));
                    };
                    let value = get!(src);
                    tri!(self.heap.cell_mut(cell)).value = value;
                }
                Insn::LoadCapture { dst, index } => {
                    let cell = tri!(self.capture(index));
                    let value = tri!(self.heap.cell(cell)).value;
                    put!(dst, value);
                }
                Insn::StoreCapture { src, index } => {
                    let cell = tri!(self.capture(index));
                    let value = get!(src);
                    tri!(self.heap.cell_mut(cell)).value = value;
                }
                Insn::This { dst } => {
                    let this = self.vm.frames.last().map_or(Value::Undefined, |f| f.this);
                    put!(dst, this);
                }
                Insn::Callee { dst } => {
                    let callee = self
                        .vm
                        .frames
                        .last()
                        .and_then(|f| f.function)
                        .map_or(Value::Undefined, Value::Object);
                    put!(dst, callee);
                }
                Insn::Arguments { dst } => {
                    save!();
                    tri!(self.create_arguments(base + usize::from(dst)));
                }

                // --- Globals ---
                Insn::DeclareGlobals => {
                    save!();
                    tri!(self.declare_globals(&code));
                }
                Insn::GetGlobal { dst, name } => {
                    let name = string_const!(name);
                    if let Some(found) = tri!(self.get_global(name)) {
                        lookup!(dst, found);
                    } else {
                        let text = self.name_text(name);
                        tri!(Err(VmError::reference_error(format!(
                            "{text} is not defined"
                        ))));
                    }
                }
                Insn::TypeofGlobal { dst, name } => {
                    let name = string_const!(name);
                    save!();
                    let result = tri!(self.with_scope(|rt| rt.typeof_global(name)));
                    put!(dst, result);
                }
                Insn::SetGlobal { src, name } => {
                    let name = string_const!(name);
                    let value = get!(src);
                    save!();
                    let outcome = tri!(self.set_global(name, value, code.strict));
                    store!(outcome, value);
                }
                Insn::InitGlobalLexical { src, name } => {
                    let name = string_const!(name);
                    let value = get!(src);
                    tri!(self.init_global_lexical(name, value));
                }
                Insn::InitGlobalFunction { src, name } => {
                    let name = string_const!(name);
                    let value = get!(src);
                    save!();
                    tri!(self.init_global_function(name, value));
                }
                Insn::DeleteGlobal { dst, name } => {
                    let name = string_const!(name);
                    let result = tri!(self.delete_global(name));
                    put!(dst, Value::Bool(result));
                }

                // --- Properties ---
                Insn::GetNamed { dst, obj, site } => {
                    let key = site!(site);
                    let target = get!(obj);
                    if let Value::Object(object) = target
                        && !matches!(key, PropertyKey::Index(_))
                    {
                        match tri!(self.heap.get(object, key, target)) {
                            GetResult::Value(value) => put!(dst, value),
                            GetResult::CallGetter { getter, receiver } => {
                                call_accessor!(getter, receiver, &[], ReturnTo::Register(dst));
                            }
                        }
                    } else if let Value::Object(_) = target {
                        // An index: String objects have their own.
                        let found = tri!(self.get_property(target, key));
                        lookup!(dst, found);
                    } else {
                        save!();
                        let found = tri!(self.get_primitive_property(target, key));
                        lookup!(dst, found);
                    }
                }
                Insn::SetNamed { obj, src, site } => {
                    let key = site!(site);
                    let target = get!(obj);
                    let value = get!(src);
                    save!();
                    let outcome = tri!(self.put_value(target, key, value, code.strict));
                    store!(outcome, value);
                }
                Insn::GetIndex { dst, obj, key } => {
                    let target = get!(obj);
                    let key_value = get!(key);
                    save!();
                    let key = match key_value {
                        Value::Int(index) if index >= 0 => PropertyKey::Index(index as u32),
                        // Strings and symbols convert without script code.
                        Value::String(string) if !target.is_nullish() => {
                            tri!(self.heap.key_from_string(string))
                        }
                        Value::Symbol(symbol) => PropertyKey::Symbol(symbol),
                        _ => tri!(self.with_scope(|rt| rt.key_for_access(
                            target,
                            key_value,
                            Access::Read
                        ))),
                    };
                    let found = tri!(self.get_property(target, key));
                    lookup!(dst, found);
                }
                Insn::SetIndex { obj, key, src } => {
                    let target = get!(obj);
                    let key_value = get!(key);
                    let value = get!(src);
                    save!();
                    let key = match key_value {
                        Value::Int(index) if index >= 0 => PropertyKey::Index(index as u32),
                        // Strings and symbols convert without script code.
                        Value::String(string) if !target.is_nullish() => {
                            tri!(self.heap.key_from_string(string))
                        }
                        Value::Symbol(symbol) => PropertyKey::Symbol(symbol),
                        _ => tri!(self.with_scope(|rt| rt.key_for_access(
                            target,
                            key_value,
                            Access::Write
                        ))),
                    };
                    let outcome = tri!(self.put_value(target, key, value, code.strict));
                    store!(outcome, value);
                }
                Insn::DeleteNamed { dst, obj, site } => {
                    let key = site!(site);
                    let target = get!(obj);
                    save!();
                    let result = tri!(self.delete_property(target, key, code.strict));
                    put!(dst, Value::Bool(result));
                }
                Insn::DeleteIndex { dst, obj, key } => {
                    let target = get!(obj);
                    let key_value = get!(key);
                    save!();
                    let result = tri!(self.with_scope(|rt| {
                        let key = rt.key_for_access(target, key_value, Access::Delete)?;
                        rt.delete_property(target, key, code.strict)
                    }));
                    put!(dst, Value::Bool(result));
                }
                Insn::In { dst, key, obj } => {
                    let key_value = get!(key);
                    let target = get!(obj);
                    save!();
                    let result = tri!(self.with_scope(|rt| rt.has_property_op(key_value, target)));
                    put!(dst, Value::Bool(result));
                }
                Insn::ToKey { dst, src } => {
                    let value = get!(src);
                    save!();
                    let key = tri!(self.with_scope(|rt| {
                        let key = rt.to_property_key(value)?;
                        Ok(super::convert::key_as_value(key))
                    }));
                    put!(dst, key);
                }

                // --- Literals ---
                Insn::NewObject { dst } => {
                    let realm = self.current_realm();
                    let proto = tri!(self.intrinsic(realm, Intrinsic::ObjectPrototype));
                    let object = tri!(self.heap.new_object(Some(proto)));
                    put!(dst, Value::Object(object));
                }
                Insn::NewArray { dst } => {
                    let realm = self.current_realm();
                    let proto = tri!(self.intrinsic(realm, Intrinsic::ArrayPrototype));
                    let array = tri!(self.heap.new_array(Some(proto), 0));
                    put!(dst, Value::Object(array));
                }
                Insn::DefineNamed { obj, src, site } => {
                    let key = site!(site);
                    let Value::Object(object) = get!(obj) else {
                        return Err(VmError::invariant("a literal that is not an object"));
                    };
                    let value = get!(src);
                    tri!(self.heap.create_data_property(object, key, value, &self.vm));
                }
                Insn::DefineIndex { obj, key, src } => {
                    let Value::Object(object) = get!(obj) else {
                        return Err(VmError::invariant("a literal that is not an object"));
                    };
                    let key = tri!(self.key_from_converted(get!(key)));
                    let value = get!(src);
                    tri!(self.heap.create_data_property(object, key, value, &self.vm));
                }
                Insn::Push { array, src } => {
                    let Value::Object(object) = get!(array) else {
                        return Err(VmError::invariant("an array literal that is not an object"));
                    };
                    let value = get!(src);
                    let (length, _) = tri!(self.heap.array_length(object));
                    tri!(self.heap.create_data_property(
                        object,
                        PropertyKey::Index(length),
                        value,
                        &self.vm
                    ));
                }
                Insn::Hole { array } => {
                    let Value::Object(object) = get!(array) else {
                        return Err(VmError::invariant("an array literal that is not an object"));
                    };
                    tri!(self.append_hole(object));
                }
                Insn::SetProto { obj, src } => {
                    let Value::Object(object) = get!(obj) else {
                        return Err(VmError::invariant("a literal that is not an object"));
                    };
                    let proto = match get!(src) {
                        Value::Object(proto) => Some(Some(proto)),
                        Value::Null => Some(None),
                        _ => None,
                    };
                    if let Some(proto) = proto {
                        tri!(self.heap.set_prototype_of(object, proto));
                    }
                }

                // --- Arithmetic ---
                Insn::Add { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    let result = match (x, y) {
                        (Value::Int(p), Value::Int(q)) => p
                            .checked_add(q)
                            .map_or(Value::Double(f64::from(p) + f64::from(q)), Value::Int),
                        // Both operands are in registers, so they stay
                        // rooted if the concatenation collects.
                        (Value::String(p), Value::String(q)) => {
                            save!();
                            Value::String(tri!(self.concat(p, q)))
                        }
                        _ => {
                            if let (Some(p), Some(q)) = (x.as_number(), y.as_number()) {
                                Value::Double(p + q)
                            } else {
                                save!();
                                tri!(self.with_scope(|rt| rt.add_slow(x, y)))
                            }
                        }
                    };
                    put!(dst, result);
                }
                Insn::Sub { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| x
                        .checked_sub(y)
                        .map_or(Value::Double(f64::from(x) - f64::from(y)), Value::Int),
                    |x: f64, y: f64| x - y,
                    super::convert::NumberOp::Sub
                ),
                Insn::Mul { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    super::convert::int_mul,
                    |x: f64, y: f64| x * y,
                    super::convert::NumberOp::Mul
                ),
                Insn::Div { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    super::convert::int_div,
                    |x: f64, y: f64| x / y,
                    super::convert::NumberOp::Div
                ),
                Insn::Rem { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    super::convert::int_rem,
                    |x: f64, y: f64| x % y,
                    super::convert::NumberOp::Rem
                ),
                Insn::Exp { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Double(super::convert::exponentiate(
                        f64::from(x),
                        f64::from(y)
                    )),
                    super::convert::exponentiate,
                    super::convert::NumberOp::Exp
                ),
                Insn::BitAnd { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Int(x & y),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_int32(x) & super::convert::to_int32(y)
                    ),
                    super::convert::NumberOp::BitAnd
                ),
                Insn::BitOr { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Int(x | y),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_int32(x) | super::convert::to_int32(y)
                    ),
                    super::convert::NumberOp::BitOr
                ),
                Insn::BitXor { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Int(x ^ y),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_int32(x) ^ super::convert::to_int32(y)
                    ),
                    super::convert::NumberOp::BitXor
                ),
                Insn::Shl { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Int(x.wrapping_shl(y as u32 & 31)),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_int32(x).wrapping_shl(super::convert::to_uint32(y) & 31)
                    ),
                    super::convert::NumberOp::Shl
                ),
                Insn::Shr { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::Int(x >> (y as u32 & 31)),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_int32(x) >> (super::convert::to_uint32(y) & 31)
                    ),
                    super::convert::NumberOp::Shr
                ),
                Insn::UShr { dst, a, b } => arith!(
                    dst,
                    a,
                    b,
                    |x: i32, y: i32| Value::number(f64::from((x as u32) >> (y as u32 & 31))),
                    |x: f64, y: f64| f64::from(
                        super::convert::to_uint32(x) >> (super::convert::to_uint32(y) & 31)
                    ),
                    super::convert::NumberOp::UShr
                ),

                // --- Comparison ---
                Insn::StrictEq { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    let equal = match (x, y) {
                        (Value::Int(p), Value::Int(q)) => p == q,
                        _ => tri!(self.strictly_equal(x, y)),
                    };
                    put!(dst, Value::Bool(equal));
                }
                Insn::StrictNe { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    let equal = match (x, y) {
                        (Value::Int(p), Value::Int(q)) => p == q,
                        _ => tri!(self.strictly_equal(x, y)),
                    };
                    put!(dst, Value::Bool(!equal));
                }
                Insn::Eq { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    // Without an object operand no script code runs.
                    let equal = if matches!(x, Value::Object(_)) || matches!(y, Value::Object(_)) {
                        save!();
                        tri!(self.with_scope(|rt| rt.loosely_equal(x, y)))
                    } else {
                        tri!(self.loosely_equal(x, y))
                    };
                    put!(dst, Value::Bool(equal));
                }
                Insn::Ne { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    // Without an object operand no script code runs.
                    let equal = if matches!(x, Value::Object(_)) || matches!(y, Value::Object(_)) {
                        save!();
                        tri!(self.with_scope(|rt| rt.loosely_equal(x, y)))
                    } else {
                        tri!(self.loosely_equal(x, y))
                    };
                    put!(dst, Value::Bool(!equal));
                }
                Insn::Lt { dst, a, b } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x < y,
                        |x: f64, y: f64| x < y,
                        |rt: &mut Runtime, x, y| rt.compare_values(x, y, false, false)
                    );
                    put!(dst, Value::Bool(r));
                }
                Insn::Le { dst, a, b } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x <= y,
                        |x: f64, y: f64| x <= y,
                        |rt: &mut Runtime, x, y| rt.compare_values(y, x, true, true)
                    );
                    put!(dst, Value::Bool(r));
                }
                Insn::Gt { dst, a, b } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x > y,
                        |x: f64, y: f64| x > y,
                        |rt: &mut Runtime, x, y| rt.compare_values(y, x, true, false)
                    );
                    put!(dst, Value::Bool(r));
                }
                Insn::Ge { dst, a, b } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x >= y,
                        |x: f64, y: f64| x >= y,
                        |rt: &mut Runtime, x, y| rt.compare_values(x, y, false, true)
                    );
                    put!(dst, Value::Bool(r));
                }
                Insn::InstanceOf { dst, a, b } => {
                    let x = get!(a);
                    let y = get!(b);
                    save!();
                    let result = tri!(self.with_scope(|rt| rt.instance_of(x, y)));
                    put!(dst, Value::Bool(result));
                }

                // --- Unary ---
                Insn::Neg { dst, src } => {
                    let value = get!(src);
                    let result = match value {
                        Value::Int(0) => Value::Double(-0.0),
                        Value::Int(x) if x != i32::MIN => Value::Int(-x),
                        _ => {
                            save!();
                            let x = tri!(self.with_scope(|rt| rt.to_number(value)));
                            Value::Double(-x)
                        }
                    };
                    put!(dst, result);
                }
                Insn::ToNumber { dst, src } | Insn::ToNumeric { dst, src } => {
                    let value = get!(src);
                    let result = match value {
                        Value::Int(_) | Value::Double(_) => value,
                        _ => {
                            save!();
                            Value::number(tri!(self.with_scope(|rt| rt.to_number(value))))
                        }
                    };
                    put!(dst, result);
                }
                Insn::BitNot { dst, src } => {
                    let value = get!(src);
                    let result = if let Value::Int(x) = value {
                        !x
                    } else {
                        save!();
                        let x = tri!(self.with_scope(|rt| rt.to_number(value)));
                        !super::convert::to_int32(x)
                    };
                    put!(dst, Value::Int(result));
                }
                Insn::Not { dst, src } => {
                    let value = get!(src);
                    let truthy = tri!(self.to_boolean(value));
                    put!(dst, Value::Bool(!truthy));
                }
                Insn::Typeof { dst, src } => {
                    let value = get!(src);
                    let name = tri!(self.typeof_value(value));
                    put!(dst, Value::String(name));
                }
                Insn::Inc { dst, src } => {
                    let value = get!(src);
                    let result = match value {
                        Value::Int(x) if x < i32::MAX => Value::Int(x + 1),
                        _ => {
                            save!();
                            let x = tri!(self.with_scope(|rt| rt.to_number(value)));
                            Value::Double(x + 1.0)
                        }
                    };
                    put!(dst, result);
                }
                Insn::Dec { dst, src } => {
                    let value = get!(src);
                    let result = match value {
                        Value::Int(x) if x > i32::MIN => Value::Int(x - 1),
                        _ => {
                            save!();
                            let x = tri!(self.with_scope(|rt| rt.to_number(value)));
                            Value::Double(x - 1.0)
                        }
                    };
                    put!(dst, result);
                }
                Insn::ToString { dst, src } => {
                    let value = get!(src);
                    let result = if let Value::String(_) = value {
                        value
                    } else {
                        save!();
                        Value::String(tri!(self.with_scope(|rt| rt.to_string(value))))
                    };
                    put!(dst, result);
                }

                // --- Control flow ---
                Insn::Jump { offset } => jump!(offset),
                Insn::JumpIfTrue { cond, offset } => {
                    if tri!(self.to_boolean(get!(cond))) {
                        jump!(offset);
                    }
                }
                Insn::JumpIfFalse { cond, offset } => {
                    if !tri!(self.to_boolean(get!(cond))) {
                        jump!(offset);
                    }
                }
                Insn::JumpIfNotNullish { src, offset } => {
                    if !matches!(get!(src), Value::Undefined | Value::Null) {
                        jump!(offset);
                    }
                }
                Insn::JumpIfNotLt { a, b, offset } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x < y,
                        |x: f64, y: f64| x < y,
                        |rt: &mut Runtime, x, y| rt.compare_values(x, y, false, false)
                    );
                    if !r {
                        jump!(offset);
                    }
                }
                Insn::JumpIfNotLe { a, b, offset } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x <= y,
                        |x: f64, y: f64| x <= y,
                        |rt: &mut Runtime, x, y| rt.compare_values(y, x, true, true)
                    );
                    if !r {
                        jump!(offset);
                    }
                }
                Insn::JumpIfNotGt { a, b, offset } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x > y,
                        |x: f64, y: f64| x > y,
                        |rt: &mut Runtime, x, y| rt.compare_values(y, x, true, false)
                    );
                    if !r {
                        jump!(offset);
                    }
                }
                Insn::JumpIfNotGe { a, b, offset } => {
                    let r = compare!(
                        a,
                        b,
                        |x: i32, y: i32| x >= y,
                        |x: f64, y: f64| x >= y,
                        |rt: &mut Runtime, x, y| rt.compare_values(x, y, false, true)
                    );
                    if !r {
                        jump!(offset);
                    }
                }

                // --- Calls ---
                Insn::Call { callee, argc } | Insn::New { callee, argc } => {
                    let construct = matches!(insn, Insn::New { .. });
                    save!();
                    if self.heap.gc_due() {
                        tri!(self.heap.safepoint(&self.vm));
                    }
                    let site = CallSite {
                        window: base + usize::from(callee) + 2,
                        argc: usize::from(argc),
                        ret: ReturnTo::Register(callee),
                        restore_top: base + code.register_count as usize,
                        construct,
                    };
                    match tri!(self.invoke(site)) {
                        Step::Value(value) => put!(callee, value),
                        Step::Entered => reload!(),
                        Step::NotCallable => {
                            let what = if construct {
                                "is not a constructor"
                            } else {
                                "is not a function"
                            };
                            let text = self.callee_text(&code, pc - 1);
                            tri!(Err(VmError::type_error(format!("{text} {what}"))));
                        }
                    }
                }
                Insn::Return { src } => {
                    let value = get!(src);
                    save!();
                    if let Some(value) = tri!(self.return_from_frame(value)) {
                        return Ok(value);
                    }
                    reload!();
                }
                Insn::Closure { dst, index } => {
                    let Some(Constant::Function(handle, compiled)) =
                        code.constants.get(index as usize)
                    else {
                        return Err(VmError::invariant(
                            "a closure of a constant that is not code",
                        ));
                    };
                    let (handle, compiled) = (*handle, Rc::clone(compiled));
                    let captures = tri!(self.collect_captures(base, &compiled));
                    let realm = self.current_realm();
                    save!();
                    let scope = self.heap.open_scope();
                    let function = tri!(self.new_closure(handle, &compiled, captures, realm));
                    put!(dst, Value::Object(function));
                    tri!(self.heap.close_scope(scope));
                }

                // --- Errors ---
                Insn::Throw { src } => {
                    let value = get!(src);
                    tri!(Err(VmError::Throw(value)));
                }
                Insn::ThrowConstAssign { name: _ } => {
                    tri!(Err(VmError::type_error("Assignment to constant variable.")));
                }
                Insn::ThrowInvalidAssign => {
                    tri!(Err(VmError::reference_error(
                        "Invalid left-hand side in assignment"
                    )));
                }
                Insn::EndFinally {
                    kind,
                    value,
                    offset,
                } => match get!(kind) {
                    Value::Int(COMPLETION_NORMAL) => jump!(offset),
                    Value::Int(COMPLETION_THROW) => {
                        let exception = get!(value);
                        tri!(Err(VmError::Throw(exception)));
                    }
                    // The table of routes follows the instruction and ends
                    // before its target.
                    Value::Int(k)
                        if k >= COMPLETION_RETURN
                            && k - COMPLETION_RETURN < offset.saturating_sub(1) =>
                    {
                        pc += (k - COMPLETION_RETURN) as usize;
                    }
                    _ => return Err(VmError::invariant("a finally block without a completion")),
                },

                // --- Generators ---
                Insn::InitialYield => {
                    save!();
                    if let Some(value) = tri!(self.initial_yield()) {
                        return Ok(value);
                    }
                    reload!();
                }
                Insn::Yield { reg } => {
                    save!();
                    if let Some(value) = tri!(self.yield_value(reg)) {
                        return Ok(value);
                    }
                    reload!();
                }
                Insn::Resume { reg, offset } => match get!(reg.saturating_add(1)) {
                    Value::Int(RESUME_NEXT) => jump!(offset),
                    Value::Int(RESUME_THROW) => {
                        let exception = get!(reg);
                        tri!(Err(VmError::Throw(exception)));
                    }
                    // The compiled return path follows.
                    Value::Int(RESUME_RETURN) => {}
                    _ => return Err(VmError::invariant("a resumption without a mode")),
                },
                Insn::Nop => {}
            }
        }
    }

    /// The cell of capture `index` of the running function.
    fn capture(&self, index: u32) -> VmResult<crate::heap::Gc<crate::value::ValueCell>> {
        let function = self
            .vm
            .frames
            .last()
            .and_then(|f| f.function)
            .ok_or(VmError::invariant("a capture outside a function"))?;
        match &self.heap.object(function)?.kind {
            ObjectKind::Function(closure) => closure
                .captures
                .get(index as usize)
                .copied()
                .ok_or(VmError::invariant("a capture index outside the captures")),
            _ => Err(VmError::invariant(
                "a capture of a function without captures",
            )),
        }
    }

    /// The cells of a new closure, from the registers and captures of the
    /// running frame (flat closures, ADR 0026 section 3).
    fn collect_captures(
        &self,
        base: usize,
        code: &crate::bytecode::FunctionCode,
    ) -> VmResult<Box<[crate::heap::Gc<crate::value::ValueCell>]>> {
        code.captures
            .iter()
            .map(|source| match *source {
                swb_js_syntax::CaptureSource::ParentRegister(register) => {
                    match self.vm.stack.get(base + usize::from(register)) {
                        Some(Value::Cell(cell)) => Ok(*cell),
                        _ => Err(VmError::invariant("a captured register without a cell")),
                    }
                }
                swb_js_syntax::CaptureSource::ParentCapture(index) => self.capture(index),
            })
            .collect()
    }

    /// The text of the callee of the call at `pc` for an error message.
    fn callee_text(&self, code: &crate::bytecode::FunctionCode, pc: usize) -> String {
        match code.call_name(pc) {
            Some(Value::String(text)) => self.name_text(text),
            _ => "(intermediate value)".to_owned(),
        }
    }

    /// `typeof name` for a global name: `"undefined"` if it is not
    /// declared, otherwise the type of its value (a getter is called
    /// through re-entry).
    fn typeof_global(&mut self, name: crate::heap::Gc<crate::string::JsString>) -> VmResult<Value> {
        let value = match self.get_global(name)? {
            None => Value::Undefined,
            Some(Lookup::Value(value)) => value,
            Some(Lookup::Getter { getter, receiver }) => self.call(getter, receiver, &[])?,
        };
        Ok(Value::String(self.typeof_value(value)?))
    }

    /// The property key for `target[key]`: `ToPropertyKey` after the check
    /// that the base is not `undefined` or `null` (whose error names the
    /// key).
    fn key_for_access(
        &mut self,
        target: Value,
        key: Value,
        access: Access,
    ) -> VmResult<PropertyKey> {
        if access != Access::Delete && matches!(target, Value::Undefined | Value::Null) {
            // The error names the key; a key that needs script code to
            // convert is shown by its type.
            let key = match key {
                Value::Object(_) => PropertyKey::String(self.vm.atoms.object),
                _ => self.to_property_key(key)?,
            };
            return Err(if access == Access::Write {
                self.cannot_set(target, key)
            } else {
                self.cannot_read(target, key)
            });
        }
        self.to_property_key(key)
    }
}
