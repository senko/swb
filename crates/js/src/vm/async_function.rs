//! Async functions (ECMA-262 §15.8, §27.7,
//! <https://tc39.es/ecma262/#sec-async-function-objects>, ADR 0026
//! section 5).
//!
//! An async function runs on the generator machinery (`generator.rs`).
//! Its first instruction ([`crate::bytecode::Insn::AsyncStart`], §15.8.4
//! `EvaluateAsyncFunctionBody` step 1) creates the result promise and an
//! internal generator object without a prototype that holds the promise;
//! no script ever sees that object. The body then runs at once, also the
//! parameters (`FunctionDeclarationInstantiation`), until the first
//! `await`.
//!
//! `await v` (§27.7.5.3 Await) is `PromiseResolve(%Promise%, v)` and an
//! internal reaction ([`crate::vm::Continuation`]) on that promise whose
//! data is the generator object; then the frame moves into the object, as
//! at a `yield`, and the caller receives the result promise. A native
//! promise with the default `constructor` is awaited as it is, so the
//! `await` costs one job. The reaction's job resumes the frame in the
//! interpreter loop at the host level ([`async_resume`]): the value of the
//! fulfilled promise becomes the value of the `await`, a rejection throws
//! at the `await` (the dispatch after it is the `Resume` of `yield`). A
//! chain of async functions that await each other therefore costs no Rust
//! stack.
//!
//! The end of the body (§27.7.5.2 `AsyncBlockStart`): a return resolves
//! the result promise (in [`Runtime::return_from_frame`]); a throw that
//! nothing in the body catches reaches the handler that covers the whole
//! code after `AsyncStart` ([`crate::bytecode::Insn::AsyncReject`]), which
//! rejects it. So does a throw in the parameters. A termination skips the
//! handler: the promise stays pending and never becomes a rejection; the
//! frame of a running async function is removed and its object completes
//! (section 5, "Termination").
//!
//! A suspended async function is reachable only through the reaction on
//! the promise that it awaits: when no one can settle that promise, the
//! frame is garbage.

use crate::bytecode::{Insn, Reg};
use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::call::Step;
use crate::vm::generator::{GeneratorState, GeneratorStatus};
use crate::vm::{Intrinsic, ReactionKind, Resume, ResumeMode, ReturnTo, VmError, VmResult};

impl Runtime {
    /// Runs `AsyncStart`, `Await` or `AsyncReject` for the interpreter
    /// loop (the pc is saved). Returns a value for the host when the top
    /// frame's result goes there.
    #[inline(never)]
    pub(crate) fn async_instruction(&mut self, insn: Insn) -> VmResult<Option<Value>> {
        match insn {
            Insn::AsyncStart => {
                self.async_start()?;
                Ok(None)
            }
            Insn::Await { reg } => self.await_value(reg),
            Insn::AsyncReject { src } => {
                let frame = self
                    .vm
                    .frames
                    .last()
                    .ok_or(VmError::invariant("no frame"))?;
                let value = self
                    .vm
                    .stack
                    .get(frame.base as usize + usize::from(src))
                    .copied()
                    .ok_or(VmError::invariant("a register outside the stack"))?;
                self.async_reject(value)
            }
            _ => Err(VmError::invariant("not an async instruction")),
        }
    }

    /// `AsyncStart`: the result promise and the generator object of the
    /// running async function's frame (§15.8.4 step 1,
    /// `NewPromiseCapability(%Promise%)` without functions, which no script
    /// can reach).
    fn async_start(&mut self) -> VmResult<()> {
        let realm = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?
            .realm;
        self.with_scope(|rt| {
            let promise = rt.new_promise(realm)?;
            let state = GeneratorState {
                status: GeneratorStatus::Executing,
                frame: None,
                registers: Vec::new(),
                resume_register: None,
                promise: Some(promise),
            };
            let object = rt
                .heap
                .new_object_with_kind(None, ObjectKind::Generator(Box::new(state)))?;
            // The frame roots the object (and so the promise) from here on.
            match rt.vm.frames.last_mut() {
                Some(frame) => frame.generator = Some(object),
                None => return Err(VmError::invariant("no frame")),
            }
            Ok(())
        })
    }

    /// The generator object and result promise of the running async
    /// function's frame.
    fn running_async(&self) -> VmResult<(Gc<Object>, Gc<Object>)> {
        let generator = self
            .vm
            .frames
            .last()
            .and_then(|frame| frame.generator)
            .ok_or(VmError::invariant("await outside an async function frame"))?;
        let promise = self.async_promise(generator)?;
        Ok((generator, promise))
    }

    /// The result promise of an async function's generator object; `None`
    /// for the object of a generator.
    pub(crate) fn async_promise_of(&self, generator: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        match &self.heap.object(generator)?.kind {
            ObjectKind::Generator(state) => Ok(state.promise),
            _ => Err(VmError::invariant("a generator frame without a generator")),
        }
    }

    fn async_promise(&self, generator: Gc<Object>) -> VmResult<Gc<Object>> {
        self.async_promise_of(generator)?
            .ok_or(VmError::invariant("an async frame without its promise"))
    }

    /// `Await(value)` (§27.7.5.3) of the value in `register`: suspends the
    /// running async function and delivers its result promise to the
    /// caller. Returns the promise if the caller is the host. An abrupt
    /// `PromiseResolve` (a `constructor` getter that throws) throws at the
    /// `await`, before the suspension.
    fn await_value(&mut self, register: Reg) -> VmResult<Option<Value>> {
        let (generator, result) = self.running_async()?;
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("no frame"))?;
        let realm = frame.realm;
        let value = self
            .vm
            .stack
            .get(frame.base as usize + usize::from(register))
            .copied()
            .ok_or(VmError::invariant("a register outside the stack"))?;
        self.with_scope(|rt| {
            let constructor = rt.intrinsic(realm, Intrinsic::PromiseConstructor)?;
            let Value::Object(promise) = rt.promise_resolve(constructor, value, realm)? else {
                return Err(VmError::invariant("PromiseResolve without a promise"));
            };
            rt.then_internal(promise, async_resume, generator.into())
        })?;
        // The reaction roots the generator object; the object roots the
        // result promise.
        let ret = self.suspend_top(generator, Some(register), GeneratorStatus::SuspendedYield)?;
        Ok(self.deliver(ret, result.into()))
    }

    /// The end of a running async function's body (§27.7.5.2 steps 3.e
    /// to 3.h): the result promise is resolved with `value` (a return) or
    /// rejected (`rejected`, a throw), then the frame is removed. Returns
    /// the promise if the caller is the host. The resolution can run a
    /// getter of `then` (through re-entry) and enqueue a job. The
    /// specification removes the frame first; Chromium 148 shows the
    /// function in the stack of an error that such a getter makes
    /// (measured), so the frame stays until the promise is resolved.
    #[inline(never)]
    pub(crate) fn async_return(
        &mut self,
        generator: Gc<Object>,
        value: Value,
        rejected: bool,
    ) -> VmResult<Option<Value>> {
        let promise = self.async_promise(generator)?;
        // The frame's registers root `value`, and the frame the promise.
        if rejected {
            self.reject_promise(promise, value)?;
        } else {
            self.with_scope(|rt| rt.resolve_promise(promise, value))?;
        }
        // Nothing allocates from here until the promise is delivered.
        self.complete_generator(generator)?;
        let frame = self
            .vm
            .frames
            .pop()
            .ok_or(VmError::invariant("return without a frame"))?;
        self.vm
            .stack
            .resize(frame.restore_top as usize, Value::Undefined);
        Ok(self.deliver(frame.ret, promise.into()))
    }

    /// `AsyncReject`: the body threw `value` and nothing in it caught it.
    fn async_reject(&mut self, value: Value) -> VmResult<Option<Value>> {
        let (generator, _) = self.running_async()?;
        self.async_return(generator, value, true)
    }
}

/// The internal reaction of an `await` (the closures of §27.7.5.3 steps
/// 3 to 6): resumes the suspended frame in `data` with the value of the
/// fulfilled promise, or throws the reason at the `await`. It runs from a
/// job at the host level, so the frame runs in a fresh interpreter loop
/// with no frame below it. The reaction has no capability; the result is
/// ignored. A termination or an engine error passes through.
fn async_resume(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Value::Object(generator) = data else {
        return Err(VmError::invariant("an await reaction without its function"));
    };
    let mode = match kind {
        ReactionKind::Fulfill => ResumeMode::Next,
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
        _ => Err(VmError::invariant(
            "an awaiting function that did not resume",
        )),
    }
}
