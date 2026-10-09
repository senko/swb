//! Generators (ECMA-262 §27.5, ADR 0026 section 5, memo 1.3).
//!
//! Calling a generator function runs its prologue (the parameters and
//! the declarations, §15.5.2 `EvaluateGeneratorBody`) and stops at
//! [`crate::bytecode::Insn::InitialYield`]: the frame record and its
//! register window move into a new generator object, which the call
//! returns. `next(v)` (a native method on the generator prototype) asks
//! the interpreter to move them back on top of the stacks and to continue;
//! `v` becomes the value of the `yield` expression. `yield` moves the
//! frame out again and returns `{ value, done: false }` to the caller of
//! `next`; a return completes the generator with `{ value, done: true }`.
//!
//! Captured bindings are heap cells, so nothing refers to a position in
//! the moved window. `return()` and `throw()` and the resume modes come
//! with M7 feature 7.

use crate::bytecode::Reg;
use crate::heap::{Gc, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::call::Step;
use crate::vm::{
    Frame, Intrinsic, NativeCall, NativeReturn, Resume, ReturnTo, STACK_OVERFLOW, VmError, VmResult,
};

/// The state of a generator (§27.5.2 `[[GeneratorState]]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorStatus {
    SuspendedStart,
    SuspendedYield,
    Executing,
    Completed,
}

/// The payload of a generator object.
pub struct GeneratorState {
    pub(crate) status: GeneratorStatus,
    /// The suspended frame record.
    pub(crate) frame: Option<Frame>,
    /// The register window of the suspended frame. While the generator
    /// runs, the vector is empty and keeps its capacity for the next
    /// suspension.
    pub(crate) registers: Vec<Value>,
    /// The register that receives the value of `next(v)`.
    pub(crate) resume_register: Option<Reg>,
}

impl GeneratorState {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(frame) = &self.frame {
            frame.trace(tracer);
        }
        for value in &self.registers {
            tracer.value(*value);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<GeneratorState>()
            + self.registers.capacity() * size_of::<Value>()
            + self
                .frame
                .as_ref()
                .map_or(0, |f| f.extra_args.capacity() * size_of::<Value>())
    }
}

/// `%GeneratorPrototype%.next(value)` (§27.5.1.2): asks the interpreter
/// to resume the generator.
pub(crate) fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let generator = match call.this() {
        Value::Object(object)
            if matches!(rt.heap.object(object)?.kind, ObjectKind::Generator(_)) =>
        {
            object
        }
        _ => {
            return Err(VmError::type_error(
                "next method called on incompatible receiver",
            ));
        }
    };
    let value = rt.arg(call, 0);
    Ok(NativeReturn::Resume(Resume { generator, value }))
}

impl Runtime {
    fn generator_state(&mut self, generator: Gc<Object>) -> VmResult<&mut GeneratorState> {
        match &mut self.heap.object_mut(generator)?.kind {
            ObjectKind::Generator(state) => Ok(state),
            _ => Err(VmError::invariant("a generator frame without a generator")),
        }
    }

    /// `[[InitialYield]]`: creates the generator object of the running
    /// generator function (with the function's `prototype` as its
    /// prototype, §10.1.13 `OrdinaryCreateFromConstructor`), suspends the
    /// frame in it and delivers it to the caller. Returns the object if the
    /// caller is the host.
    pub(crate) fn initial_yield(&mut self) -> VmResult<Option<Value>> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        let realm = frame.realm;
        let function = frame
            .function
            .ok_or(VmError::invariant("a generator frame without a function"))?;
        let key = PropertyKey::String(self.vm.atoms.prototype);
        let proto = match self.heap.get(function, key, function.into())? {
            crate::object::GetResult::Value(Value::Object(proto)) => proto,
            _ => self.intrinsic(realm, Intrinsic::GeneratorPrototype)?,
        };
        let state = GeneratorState {
            status: GeneratorStatus::SuspendedStart,
            frame: None,
            registers: Vec::new(),
            resume_register: None,
        };
        let generator = self
            .heap
            .new_object_with_kind(Some(proto), ObjectKind::Generator(Box::new(state)))?;
        // The frame roots the generator until it is suspended in it.
        if let Some(frame) = self.vm.frames.last_mut() {
            frame.generator = Some(generator);
        }
        let ret = self.suspend_top(generator, None, GeneratorStatus::SuspendedStart)?;
        Ok(self.deliver(ret, generator.into()))
    }

    /// `yield` (§27.5.3.7 Yield): suspends the running generator frame and
    /// delivers `{ value, done: false }` to the caller of `next`. Returns
    /// the result if the caller is the host.
    pub(crate) fn yield_value(&mut self, register: Reg) -> VmResult<Option<Value>> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        let generator = frame
            .generator
            .ok_or(VmError::invariant("yield outside a generator frame"))?;
        let value = self
            .vm
            .stack
            .get(frame.base as usize + usize::from(register))
            .copied()
            .ok_or(VmError::invariant("a register outside the stack"))?;
        let scope = self.heap.open_scope();
        let result = self.iter_result(value, false)?;
        self.heap.record(result);
        let ret = self.suspend_top(generator, Some(register), GeneratorStatus::SuspendedYield)?;
        self.heap.close_scope(scope)?;
        Ok(self.deliver(ret, result.into()))
    }

    /// Moves the top frame and its register window into the generator.
    /// Returns where the frame's result was to go.
    fn suspend_top(
        &mut self,
        generator: Gc<Object>,
        resume_register: Option<Reg>,
        status: GeneratorStatus,
    ) -> VmResult<ReturnTo> {
        let frame = self.vm.frames.pop().ok_or(VmError::invariant("no frame"))?;
        let start = frame.base as usize;
        let end = start + frame.compiled.register_count as usize;
        let ret = frame.ret;
        let restore_top = frame.restore_top as usize;
        let Self { heap, vm, .. } = self;
        let window = vm
            .stack
            .get(start..end)
            .ok_or(VmError::invariant("a frame window outside the stack"))?;
        let ObjectKind::Generator(state) = &mut heap.object_mut(generator)?.kind else {
            return Err(VmError::invariant("a generator frame without a generator"));
        };
        state.registers.clear();
        state.registers.extend_from_slice(window);
        state.frame = Some(frame);
        state.resume_register = resume_register;
        state.status = status;
        vm.stack.resize(restore_top, Value::Undefined);
        Ok(ret)
    }

    /// Resumes a suspended generator with `value` (§27.5.3.3
    /// GeneratorResume): moves its frame back on top of the stacks. The
    /// frame's result goes to `ret`; the stack returns to `restore_top`.
    pub(crate) fn resume_generator(
        &mut self,
        generator: Gc<Object>,
        value: Value,
        ret: ReturnTo,
        restore_top: usize,
    ) -> VmResult<Step> {
        let status = self.generator_state(generator)?.status;
        match status {
            GeneratorStatus::Executing => {
                return Err(VmError::type_error("Generator is already running"));
            }
            GeneratorStatus::Completed => {
                let result = self.iter_result(Value::Undefined, true)?;
                return Ok(Step::Value(result.into()));
            }
            GeneratorStatus::SuspendedStart | GeneratorStatus::SuspendedYield => {}
        }
        if self.vm.frames.len() >= self.vm.frame_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let window = self.vm.stack.len();
        let Self { heap, vm, .. } = self;
        let ObjectKind::Generator(state) = &mut heap.object_mut(generator)?.kind else {
            return Err(VmError::invariant("not a generator"));
        };
        if window + state.registers.len() > vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let mut frame = state
            .frame
            .take()
            .ok_or(VmError::invariant("a suspended generator without a frame"))?;
        vm.stack.extend_from_slice(&state.registers);
        state.registers.clear();
        state.status = GeneratorStatus::Executing;
        if status == GeneratorStatus::SuspendedYield
            && let Some(register) = state.resume_register
            && let Some(slot) = vm.stack.get_mut(window + usize::from(register))
        {
            *slot = value;
        }
        frame.base = window as u32;
        frame.ret = ret;
        frame.restore_top = restore_top as u32;
        frame.generator = Some(generator);
        vm.frames.push(frame);
        Ok(Step::Entered)
    }

    /// Marks the generator of a frame that returned or threw as completed.
    pub(crate) fn complete_generator(&mut self, generator: Gc<Object>) -> VmResult<()> {
        let state = self.generator_state(generator)?;
        state.status = GeneratorStatus::Completed;
        state.frame = None;
        state.registers = Vec::new();
        Ok(())
    }

    /// `CreateIterResultObject` (§7.4.14). The arguments of the internal
    /// methods are rooted during their reservations, so `value` must only
    /// be rooted by the caller until this returns.
    pub(crate) fn iter_result(&mut self, value: Value, done: bool) -> VmResult<Gc<Object>> {
        let realm = self.vm.frames.last().map_or(0, |f| f.realm);
        let proto = self.intrinsic(realm, Intrinsic::ObjectPrototype)?;
        let object = self.heap.new_object(Some(proto))?;
        let value_key = PropertyKey::String(self.vm.atoms.value);
        self.heap
            .create_data_property(object, value_key, value, &self.vm)?;
        let done_key = PropertyKey::String(self.vm.atoms.done);
        self.heap
            .create_data_property(object, done_key, Value::Bool(done), &self.vm)?;
        Ok(object)
    }
}
