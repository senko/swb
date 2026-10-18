//! Async generators (ECMA-262 §27.6,
//! <https://tc39.es/ecma262/#sec-asyncgenerator-objects>, ADR 0026
//! section 5).
//!
//! An async generator is a generator object (`generator.rs`) whose state
//! has the `[[AsyncGeneratorState]]` and the `[[AsyncGeneratorQueue]]` of
//! §27.6.2 ([`AsyncGenerator`]). Calling an async generator function runs
//! the prologue and stops at `InitialYield`, as for a generator.
//!
//! `next`, `return` and `throw` (§27.6.1) make a promise (the intrinsic
//! capability of `NewPromiseCapability(%Promise%)`), append a request to
//! the queue and, when the generator is suspended, resume it
//! (`AsyncGeneratorResume`): the native returns a resume request, so the
//! frame runs in the caller's interpreter loop, and the frame delivers the
//! request's promise to the caller when it next suspends or completes
//! ([`AsyncGenerator::caller_promise`]). A frame that a job resumes (after
//! an `await`) delivers nothing.
//!
//! `yield v` is `Await(v)` and [`crate::bytecode::Insn::AsyncYield`]
//! (`AsyncGeneratorYield`, §27.6.3.8): the request at the head of the
//! queue is resolved with `{ value, done: false }`; if more requests wait,
//! the frame continues with the next one at once, else it suspends. The
//! value of a return request is awaited before the frame resumes
//! (`AsyncGeneratorUnwrapYieldResumption`, §27.6.3.7), here, by a
//! continuation that resumes the frame in return mode (or in throw mode
//! when the promise rejects); the frame then takes the compiled return
//! path as after a generator's `yield`.
//!
//! The end of the body (§27.6.3.2 `AsyncGeneratorStart`): a return or a
//! throw that nothing catches (the handler `AsyncReject` covers all of the
//! body) completes the head request (`AsyncGeneratorCompleteStep`) and
//! drains the queue (`AsyncGeneratorDrainQueue`); a return request in the
//! queue awaits its value first (`AsyncGeneratorAwaitReturn`), through a
//! continuation that goes on with the drain.
//!
//! The queue lives in the generator object: it is traced with it, and its
//! bytes count as the object's heap size, so a queue that grows without
//! end ends at the heap limit. A suspended async generator that no one can
//! reach is garbage; one that waits at an `await` is reachable only from
//! the promise's reaction.

use std::collections::VecDeque;

use crate::bytecode::{RESUME_NEXT, RESUME_THROW, Reg};
use crate::heap::{Gc, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::call::Step;
use crate::vm::generator::{GeneratorKind, GeneratorStatus, incompatible_receiver};
use crate::vm::{
    Intrinsic, NativeCall, NativeReturn, ReactionKind, Resume, ResumeMode, ReturnTo, VmError,
    VmResult,
};

/// `[[AsyncGeneratorState]]` (§27.6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorState {
    #[default]
    SuspendedStart,
    SuspendedYield,
    Executing,
    DrainingQueue,
    Completed,
}

/// An `AsyncGeneratorRequest` Record (§27.6.3.1): the completion and the
/// promise of its capability (always `%Promise%`, whose functions no
/// script sees).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Request {
    pub(crate) mode: ResumeMode,
    pub(crate) value: Value,
    pub(crate) promise: Gc<Object>,
}

/// The async part of an async generator's state.
#[derive(Default)]
pub(crate) struct AsyncGenerator {
    pub(crate) state: AsyncGeneratorState,
    /// `[[AsyncGeneratorQueue]]`.
    pub(crate) queue: VecDeque<Request>,
    /// The promise that the frame delivers to the caller of the `next`,
    /// `return` or `throw` call that resumed it, at its next suspension or
    /// at its end.
    pub(crate) caller_promise: Option<Gc<Object>>,
}

impl AsyncGenerator {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for request in &self.queue {
            tracer.value(request.value);
            tracer.object(request.promise);
        }
        if let Some(promise) = self.caller_promise {
            tracer.object(promise);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<AsyncGenerator>() + self.queue.capacity() * size_of::<Request>()
    }
}

/// `%AsyncGeneratorPrototype%.next(value)` (§27.6.1.2).
pub(crate) fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    request(rt, call, ResumeMode::Next, "next")
}

/// `%AsyncGeneratorPrototype%.return(value)` (§27.6.1.3).
pub(crate) fn return_(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    request(rt, call, ResumeMode::Return, "return")
}

/// `%AsyncGeneratorPrototype%.throw(exception)` (§27.6.1.4).
pub(crate) fn throw(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    request(rt, call, ResumeMode::Throw, "throw")
}

/// The three methods: `AsyncGeneratorValidate` (a failure rejects the
/// promise), then the steps of the mode.
fn request(
    rt: &mut Runtime,
    call: &NativeCall,
    mode: ResumeMode,
    method: &str,
) -> VmResult<NativeReturn> {
    let realm = call.realm();
    let promise = rt.new_promise(realm)?;
    let this = call.this();
    let Some(generator) = rt.as_async_generator(this)? else {
        let reason = rt.thrown_value(incompatible_receiver(rt, "AsyncGenerator", method, this))?;
        rt.heap.record(reason);
        rt.reject_promise(promise, reason)?;
        return Ok(NativeReturn::Value(promise.into()));
    };
    let value = rt.arg(call, 0);
    let mut state = rt.async_generator(generator)?.state;
    match mode {
        ResumeMode::Next => {
            if state == AsyncGeneratorState::Completed {
                let result = rt.iter_result(Value::Undefined, true)?;
                rt.heap.record(result);
                rt.resolve_promise(promise, result.into())?;
                return Ok(NativeReturn::Value(promise.into()));
            }
            rt.enqueue_request(generator, mode, value, promise)?;
        }
        ResumeMode::Return => {
            rt.enqueue_request(generator, mode, value, promise)?;
            if matches!(
                state,
                AsyncGeneratorState::SuspendedStart | AsyncGeneratorState::Completed
            ) {
                // The body never runs: the frame is garbage.
                rt.complete_generator(generator)?;
                rt.async_generator(generator)?.state = AsyncGeneratorState::DrainingQueue;
                if !rt.await_return(generator, realm)? {
                    rt.drain_queue(generator)?;
                }
                return Ok(NativeReturn::Value(promise.into()));
            }
        }
        ResumeMode::Throw => {
            if state == AsyncGeneratorState::SuspendedStart {
                rt.complete_generator(generator)?;
                rt.async_generator(generator)?.state = AsyncGeneratorState::Completed;
                state = AsyncGeneratorState::Completed;
            }
            if state == AsyncGeneratorState::Completed {
                rt.reject_promise(promise, value)?;
                return Ok(NativeReturn::Value(promise.into()));
            }
            rt.enqueue_request(generator, mode, value, promise)?;
        }
    }
    match (state, mode) {
        (AsyncGeneratorState::SuspendedStart, ResumeMode::Next)
        | (AsyncGeneratorState::SuspendedYield, _) => {
            rt.resume_async_generator(generator, mode, value, promise, realm)
        }
        // Executing or draining the queue: the request waits.
        _ => Ok(NativeReturn::Value(promise.into())),
    }
}

impl Runtime {
    /// The object of `value` if it is an async generator
    /// (`AsyncGeneratorValidate`, §27.6.3.3).
    fn as_async_generator(&self, value: Value) -> VmResult<Option<Gc<Object>>> {
        let Value::Object(object) = value else {
            return Ok(None);
        };
        Ok(match &self.heap.object(object)?.kind {
            ObjectKind::Generator(state)
                if matches!(state.kind, GeneratorKind::AsyncGenerator(_)) =>
            {
                Some(object)
            }
            _ => None,
        })
    }

    /// The promise that the code waiting for `generator` awaits (for
    /// async stack traces): an async function's result promise, or the
    /// promise of the request at the head of an async generator's queue.
    pub(crate) fn awaited_promise_of(&self, generator: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        match &self.heap.object(generator)?.kind {
            ObjectKind::Generator(state) => Ok(match &state.kind {
                GeneratorKind::AsyncFunction(promise) => Some(*promise),
                GeneratorKind::AsyncGenerator(data) => data.queue.front().map(|r| r.promise),
                GeneratorKind::Sync => None,
            }),
            _ => Err(VmError::invariant("a generator frame without a generator")),
        }
    }

    /// The async part of the state of an async generator object.
    pub(crate) fn async_generator(
        &mut self,
        generator: Gc<Object>,
    ) -> VmResult<&mut AsyncGenerator> {
        match &mut self.generator_state(generator)?.kind {
            GeneratorKind::AsyncGenerator(data) => Ok(data),
            _ => Err(VmError::invariant("an async generator without its queue")),
        }
    }

    /// `AsyncGeneratorEnqueue` (§27.6.3.4); the growth of the queue is
    /// charged to the heap.
    fn enqueue_request(
        &mut self,
        generator: Gc<Object>,
        mode: ResumeMode,
        value: Value,
        promise: Gc<Object>,
    ) -> VmResult<()> {
        let queue = &mut self.async_generator(generator)?.queue;
        let before = queue.capacity();
        queue
            .try_reserve(1)
            .map_err(|_| VmError::from(crate::error::Termination::HeapLimit))?;
        queue.push_back(Request {
            mode,
            value,
            promise,
        });
        let grown = queue.capacity() - before;
        self.heap.charge(grown * size_of::<Request>());
        Ok(())
    }

    /// `AsyncGeneratorResume` (§27.6.3.6) of a suspended generator with
    /// the request that was just enqueued. Next and throw resume the frame
    /// in the caller's loop; it delivers `promise` when it suspends. A
    /// return at a `yield` first awaits its value
    /// (`AsyncGeneratorUnwrapYieldResumption`), and the caller gets the
    /// promise at once.
    fn resume_async_generator(
        &mut self,
        generator: Gc<Object>,
        mode: ResumeMode,
        value: Value,
        promise: Gc<Object>,
        realm: u32,
    ) -> VmResult<NativeReturn> {
        let (mode, value) = match mode {
            ResumeMode::Return => {
                self.async_generator(generator)?.state = AsyncGeneratorState::Executing;
                match self.await_resumption(generator, value, realm)? {
                    None => return Ok(NativeReturn::Value(promise.into())),
                    // `PromiseResolve` threw: the `yield` throws it.
                    Some(error) => (ResumeMode::Throw, error),
                }
            }
            ResumeMode::Next | ResumeMode::Throw => (mode, value),
        };
        self.async_generator(generator)?.caller_promise = Some(promise);
        // The value is an argument of the call, or the error is in a
        // Rust local until the resumption writes it into a register;
        // nothing collects in between.
        Ok(NativeReturn::Resume(Resume {
            generator,
            value,
            mode,
        }))
    }

    /// `Await(value)` of a return request at a `yield`
    /// (§27.6.3.7): `PromiseResolve` and a continuation that resumes the
    /// frame in return mode with the value, or in throw mode with the
    /// reason. `Some(error)` if `PromiseResolve` threw.
    fn await_resumption(
        &mut self,
        generator: Gc<Object>,
        value: Value,
        realm: u32,
    ) -> VmResult<Option<Value>> {
        self.with_scope(|rt| {
            let constructor = rt.intrinsic(realm, Intrinsic::PromiseConstructor)?;
            let resolved = rt.promise_resolve(constructor, value, realm);
            match rt.completion(resolved)? {
                Ok(Value::Object(promise)) => {
                    rt.then_internal(promise, return_resumption, generator.into())?;
                    Ok(None)
                }
                Ok(_) => Err(VmError::invariant("PromiseResolve without a promise")),
                Err(error) => Ok(Some(error)),
            }
        })
    }

    /// Runs [`crate::bytecode::Insn::AsyncYield`] (`delegate` false) or
    /// [`crate::bytecode::Insn::AsyncYieldDelegate`] for the interpreter
    /// loop. Returns a value for the host when the frame suspends and its
    /// result goes there.
    pub(crate) fn async_yield(&mut self, register: Reg, delegate: bool) -> VmResult<Option<Value>> {
        let generator = self.running_generator()?;
        if delegate {
            // `IteratorValue(innerResult)`; the register keeps it rooted.
            let result = self.top_register(register)?;
            let key = PropertyKey::String(self.vm.atoms.value);
            let value = self.with_scope(|rt| rt.get_value(result, key))?;
            self.set_top_register(register, value)?;
        }
        let value = self.top_register(register)?;
        // AsyncGeneratorCompleteStep(generator, value, false).
        self.with_scope(|rt| rt.complete_step(generator, Ok(value), false))?;
        let head = self.async_generator(generator)?.queue.front().copied();
        match head {
            // The frame continues with the next request (step 11).
            Some(Request {
                mode: ResumeMode::Next,
                value,
                ..
            }) => self.write_resumption(register, value, RESUME_NEXT)?,
            Some(Request {
                mode: ResumeMode::Throw,
                value,
                ..
            }) => self.write_resumption(register, value, RESUME_THROW)?,
            Some(Request {
                mode: ResumeMode::Return,
                value,
                ..
            }) => {
                let realm = self.current_realm();
                match self.await_resumption(generator, value, realm)? {
                    None => return self.suspend_async_generator(generator, register),
                    Some(error) => self.write_resumption(register, error, RESUME_THROW)?,
                }
            }
            None => {
                self.async_generator(generator)?.state = AsyncGeneratorState::SuspendedYield;
                return self.suspend_async_generator(generator, register);
            }
        }
        Ok(None)
    }

    /// Suspends the running async generator's frame at the `yield` whose
    /// register is `register` and delivers the caller's promise.
    fn suspend_async_generator(
        &mut self,
        generator: Gc<Object>,
        register: Reg,
    ) -> VmResult<Option<Value>> {
        let result = self
            .async_generator(generator)?
            .caller_promise
            .take()
            .map_or(Value::Undefined, Value::Object);
        let ret = self.suspend_top(generator, Some(register), GeneratorStatus::SuspendedYield)?;
        Ok(self.deliver(ret, result))
    }

    /// Writes a resumption into the registers of a `yield` in the top
    /// frame: the value and the mode.
    fn write_resumption(&mut self, register: Reg, value: Value, mode: i32) -> VmResult<()> {
        self.set_top_register(register, value)?;
        self.set_top_register(register.saturating_add(1), Value::Int(mode))
    }

    /// The end of a running async generator's body (§27.6.3.2 steps 4.f
    /// to 4.k): `Ok(value)` for a return, `Err(reason)` for a throw. The
    /// head request is completed and the queue drained while the frame is
    /// still on the stack (it roots the value), then the frame is removed.
    /// Returns the caller's promise if the caller is the host.
    pub(crate) fn async_generator_complete(
        &mut self,
        generator: Gc<Object>,
        completion: Result<Value, Value>,
    ) -> VmResult<Option<Value>> {
        self.complete_generator(generator)?;
        self.async_generator(generator)?.state = AsyncGeneratorState::DrainingQueue;
        self.with_scope(|rt| {
            rt.complete_step(generator, completion, true)?;
            rt.drain_queue(generator)
        })?;
        let result = self
            .async_generator(generator)?
            .caller_promise
            .take()
            .map_or(Value::Undefined, Value::Object);
        let frame = self
            .vm
            .frames
            .pop()
            .ok_or(VmError::invariant("return without a frame"))?;
        self.vm
            .stack
            .resize(frame.restore_top as usize, Value::Undefined);
        Ok(self.deliver(frame.ret, result))
    }

    /// `AsyncGeneratorCompleteStep` (§27.6.3.5): removes the head request
    /// and resolves its promise with `{ value, done }` or rejects it. The
    /// caller roots the value and has a handle scope open.
    fn complete_step(
        &mut self,
        generator: Gc<Object>,
        completion: Result<Value, Value>,
        done: bool,
    ) -> VmResult<()> {
        let request =
            self.async_generator(generator)?
                .queue
                .pop_front()
                .ok_or(VmError::invariant(
                    "an async generator step without a request",
                ))?;
        let promise = self.heap.record(request.promise);
        match completion {
            Err(reason) => self.reject_promise(promise, reason),
            Ok(value) => {
                let result = self.iter_result(value, done)?;
                self.heap.record(result);
                self.resolve_promise(promise, result.into())
            }
        }
    }

    /// `AsyncGeneratorDrainQueue` (§27.6.3.10): completes the waiting
    /// requests (`next` with `{ undefined, done: true }`, `throw` with its
    /// exception) until a return request, which awaits its value; the
    /// generator is completed when the queue is empty.
    fn drain_queue(&mut self, generator: Gc<Object>) -> VmResult<()> {
        loop {
            // A queue of a million requests drains in one call.
            self.tick()?;
            let head = self.async_generator(generator)?.queue.front().copied();
            match head {
                None => {
                    self.async_generator(generator)?.state = AsyncGeneratorState::Completed;
                    return Ok(());
                }
                Some(Request {
                    mode: ResumeMode::Return,
                    ..
                }) => {
                    let realm = self.current_realm();
                    if self.await_return(generator, realm)? {
                        return Ok(());
                    }
                }
                Some(Request {
                    mode: ResumeMode::Next,
                    ..
                }) => self.complete_step(generator, Ok(Value::Undefined), true)?,
                Some(Request {
                    mode: ResumeMode::Throw,
                    value,
                    ..
                }) => {
                    // The queue no longer roots the value once the request
                    // is removed.
                    self.heap.record(value);
                    self.complete_step(generator, Err(value), true)?;
                }
            }
        }
    }

    /// `AsyncGeneratorAwaitReturn` (§27.6.3.9) of the return request at
    /// the head of the queue: `PromiseResolve` of its value and a
    /// continuation that completes the request and goes on with the drain.
    /// Returns whether it awaits; if `PromiseResolve` threw, the request is
    /// rejected and the caller goes on with the drain.
    fn await_return(&mut self, generator: Gc<Object>, realm: u32) -> VmResult<bool> {
        let value = match self.async_generator(generator)?.queue.front() {
            Some(request) => request.value,
            None => return Err(VmError::invariant("an await of a return without a request")),
        };
        self.with_scope(|rt| {
            let constructor = rt.intrinsic(realm, Intrinsic::PromiseConstructor)?;
            let resolved = rt.promise_resolve(constructor, value, realm);
            match rt.completion(resolved)? {
                Ok(Value::Object(promise)) => {
                    rt.then_internal(promise, return_settled, generator.into())?;
                    Ok(true)
                }
                Ok(_) => Err(VmError::invariant("PromiseResolve without a promise")),
                Err(error) => {
                    rt.complete_step(generator, Err(error), true)?;
                    Ok(false)
                }
            }
        })
    }

    /// The frame of an async generator that a termination removed: the
    /// generator is completed, its waiting requests are dropped (their
    /// promises stay pending; ADR 0026 section 5).
    pub(crate) fn abandon_async_generator(&mut self, generator: Gc<Object>) -> VmResult<()> {
        if let GeneratorKind::AsyncGenerator(data) = &mut self.generator_state(generator)?.kind {
            data.state = AsyncGeneratorState::Completed;
            data.queue = VecDeque::new();
            data.caller_promise = None;
        }
        Ok(())
    }

    /// A register of the top frame.
    pub(crate) fn top_register(&self, register: Reg) -> VmResult<Value> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        self.vm
            .stack
            .get(frame.base as usize + usize::from(register))
            .copied()
            .ok_or(VmError::invariant("a register outside the stack"))
    }

    /// Writes a register of the top frame.
    pub(crate) fn set_top_register(&mut self, register: Reg, value: Value) -> VmResult<()> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        let at = frame.base as usize + usize::from(register);
        match self.vm.stack.get_mut(at) {
            Some(slot) => {
                *slot = value;
                Ok(())
            }
            None => Err(VmError::invariant("a register outside the stack")),
        }
    }
}

/// The continuation of the await of a return request's value at a
/// `yield` (`AsyncGeneratorUnwrapYieldResumption`): resumes the frame in
/// return mode with the value, or in throw mode with the reason, from the
/// job at the host level.
fn return_resumption(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Value::Object(generator) = data else {
        return Err(VmError::invariant("a return await without its generator"));
    };
    let mode = match kind {
        ReactionKind::Fulfill => ResumeMode::Return,
        ReactionKind::Reject => ResumeMode::Throw,
    };
    let restore_top = rt.vm.stack.len();
    let resume = Resume {
        generator,
        value,
        mode,
    };
    match rt.resume_generator(resume, ReturnTo::Host, restore_top)? {
        Step::Entered => {
            rt.run_frames()?;
            Ok(Value::Undefined)
        }
        _ => Err(VmError::invariant("an async generator that did not resume")),
    }
}

/// The continuation of `AsyncGeneratorAwaitReturn` (the closures of
/// §27.6.3.9 steps 11 and 13): completes the head request with the value
/// or the reason, then drains the queue. A termination during the drain
/// completes the generator, as when it removes a frame, so that a later
/// request does not wait behind the dropped ones.
fn return_settled(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Value::Object(generator) = data else {
        return Err(VmError::invariant("a return await without its generator"));
    };
    let completion = match kind {
        ReactionKind::Fulfill => Ok(value),
        ReactionKind::Reject => Err(value),
    };
    let drained = rt.with_scope(|rt| {
        rt.complete_step(generator, completion, true)?;
        rt.drain_queue(generator)
    });
    if let Err(VmError::Terminated(_)) = drained {
        rt.abandon_async_generator(generator)?;
    }
    drained?;
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::error::Termination;
    use crate::runtime::{Runtime, RuntimeConfig, ScriptError};
    use crate::value::Value;
    use crate::vm::time::TIME_CHECK_INTERVAL;

    /// A termination inside the drain of the queue from a job completes
    /// the generator: the dropped requests stay pending, a later `next`
    /// gives `{ done: true }`.
    #[test]
    fn a_termination_in_a_drain_completes_the_generator() {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("runtime");
        rt.eval(
            "var g = (async function* () {})(); var settle; \
             g.return(new Promise(r => { settle = r; })); \
             for (var i = 0; i < 100000; i++) g.next(); settle();",
        )
        .expect("script");
        // The next time check comes inside the drain of 100,000 requests.
        rt.vm.countdown.set(TIME_CHECK_INTERVAL);
        rt.set_deadline(Some(Instant::now()));
        assert_eq!(
            rt.run_jobs(),
            Err(ScriptError::Terminated(Termination::TimeLimit))
        );
        rt.set_deadline(None);
        rt.clear_jobs();
        rt.eval("var done; g.next().then(r => { done = r.done; });")
            .expect("script");
        rt.run_jobs().expect("jobs");
        assert_eq!(rt.eval("done").expect("script"), Value::Bool(true));
    }
}
