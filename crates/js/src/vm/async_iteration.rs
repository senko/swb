//! Async iteration in compiled code (ECMA-262 §7.4.3 `GetIterator` with
//! kind async, §14.7.5 `for await`, §15.5.5 `yield*` in async generators,
//! §7.4.13 `AsyncIteratorClose`) and the Async-from-Sync Iterator objects
//! (§27.1.6, <https://tc39.es/ecma262/#sec-async-from-sync-iterator-objects>).
//!
//! The compiled loops keep their records in registers, as `for`-`of` and
//! `yield*` do (`compiler/iterate.rs`), and await with the `Await`
//! instruction of async functions, so a step suspends the frame and costs
//! no Rust stack. The instructions here run out of line (one arm of the
//! interpreter loop): the call of `next` (a frame of the same loop for
//! script code), the close of a `for await` loop around its await, and the
//! two halves of an async `yield*` step around its await.
//!
//! An Async-from-Sync Iterator wraps the record of a sync iterator. Its
//! `next`, `return` and `throw` call the sync method, then resolve their
//! promise with the result's `value` awaited: `PromiseResolve` of the
//! value and a reaction with internal continuations that build the
//! iterator result (the `unwrap` closure) and, since the 2023 change of
//! §27.1.6.4, close the sync iterator when the value rejects.

use std::rc::Rc;

use crate::bytecode::{
    COMPLETION_THROW, DELEGATE_CLOSE, DELEGATE_RETURN_MISSING, DELEGATE_TYPED, FunctionCode,
    HINT_NO_TEXT, HINT_SHIFT, ITER_CALL, ITER_RENDERED, ITER_YIELD_STAR, ITERATOR_PENDING,
    RESUME_NEXT, RESUME_RETURN, RESUME_THROW, Reg,
};
use crate::heap::Gc;
use crate::object::{Object, ObjectKind, WrappedIterator};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::call::{CallSite, Step};
use crate::vm::iter::IteratorRecord;
use crate::vm::promise::{Capability, Continuation, Handler};
use crate::vm::{
    Intrinsic, NativeCall, NativeReturn, ReactionKind, ReturnTo, VmError, VmResult, WellKnown,
};

/// V8's message of a `yield*` delegate without a `throw` method.
pub(crate) const NO_THROW_METHOD: &str = "The iterator does not provide a 'throw' method.";

/// V8's message for the operand of `yield*` in an async generator that
/// is not async iterable, when it is not a simple expression.
const YIELD_STAR_NOT_ASYNC_ITERABLE: &str = "yield* (intermediate value) is not async iterable";

impl Runtime {
    /// `GetIterator(value, async)` (§7.4.3) with V8's messages in the form
    /// that `hint` selects (measured in Chromium 148): for a rendered
    /// operand `x is not async iterable` (`f(...) is not a function or its
    /// return value is not async iterable` for a call), for the operand of
    /// `yield*` that is not a simple expression `yield* (intermediate
    /// value) is not async iterable`, else, and when `@@iterator` is
    /// `null`, the error of calling the sync method (`undefined is not a
    /// function`). An `undefined` or `null` value fails at the read of
    /// `@@asyncIterator`. The record's values are recorded in the current
    /// handle scope.
    pub(crate) fn get_async_iterator(
        &mut self,
        value: Value,
        hint: u32,
        code: &FunctionCode,
    ) -> VmResult<IteratorRecord> {
        let key = self.symbol_key(WellKnown::AsyncIterator);
        let method = self.get_value(value, key)?;
        if method.is_nullish() {
            let key = self.symbol_key(WellKnown::Iterator);
            let sync = self.get_value(value, key)?;
            if !self.is_callable(sync)? {
                return Err(self.async_iteration_error(sync, hint, code));
            }
            let record = self.get_iterator_from_method(value, sync)?;
            return self.create_async_from_sync_iterator(record);
        }
        if !self.is_callable(method)? {
            return Err(self.not_callable(method));
        }
        let iterator = self.call(method, value, &[])?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(VmError::type_error(
                "Result of the Symbol.asyncIterator method is not an object",
            ));
        }
        let key = PropertyKey::String(self.vm.atoms.next);
        let next_method = self.get_value(iterator, key)?;
        Ok(IteratorRecord {
            iterator,
            next_method,
            done: false,
        })
    }

    /// The `TypeError` of a value without a callable `@@asyncIterator` or
    /// `@@iterator` (`sync` is the value of `@@iterator`).
    fn async_iteration_error(&self, sync: Value, hint: u32, code: &FunctionCode) -> VmError {
        let index = hint & HINT_NO_TEXT;
        let text = (index != HINT_NO_TEXT)
            .then(|| code.string(index))
            .flatten()
            .map(|text| self.name_text(text));
        match (hint >> HINT_SHIFT, text) {
            (ITER_RENDERED, Some(text)) if sync != Value::Null => {
                VmError::type_error(format!("{text} is not async iterable"))
            }
            (ITER_CALL, Some(text)) if sync != Value::Null => VmError::type_error(format!(
                "{text}(...) is not a function or its return value is not async iterable"
            )),
            (ITER_YIELD_STAR, _) => VmError::type_error(YIELD_STAR_NOT_ASYNC_ITERABLE),
            _ => self.not_callable(sync),
        }
    }

    /// `CreateAsyncFromSyncIterator` (§27.1.6.1). The record's values are
    /// recorded in the current handle scope.
    pub(crate) fn create_async_from_sync_iterator(
        &mut self,
        record: IteratorRecord,
    ) -> VmResult<IteratorRecord> {
        let realm = self.current_realm();
        let proto = self.intrinsic(realm, Intrinsic::AsyncFromSyncIteratorPrototype)?;
        let kind = ObjectKind::AsyncFromSyncIterator(Box::new(WrappedIterator { record }));
        let iterator = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(iterator);
        // `! Get(asyncIterator, "next")`: the prototype's method (no script
        // can reach the object or the prototype).
        let key = PropertyKey::String(self.vm.atoms.next);
        let next_method = self.get_value(iterator.into(), key)?;
        Ok(IteratorRecord {
            iterator: iterator.into(),
            next_method,
            done: false,
        })
    }

    /// Runs one of the async iteration instructions for the interpreter
    /// loop (the pc is saved): a call pushes a frame, a jump moves the
    /// saved pc.
    pub(crate) fn async_iteration_instruction(
        &mut self,
        insn: crate::bytecode::Insn,
    ) -> VmResult<()> {
        use crate::bytecode::Insn;
        match insn {
            Insn::GetAsyncIterator { dst, src, hint } => {
                let value = self.top_register(src)?;
                let code = Rc::clone(&self.top_frame()?.compiled);
                let record = self.with_scope(|rt| rt.get_async_iterator(value, hint, &code))?;
                self.set_top_register(dst, record.iterator)?;
                self.set_top_register(dst.saturating_add(1), record.next_method)?;
                self.set_top_register(dst.saturating_add(2), Value::Bool(false))
            }
            Insn::AsyncIteratorCall { dst, rec } => {
                let iterator = self.top_register(rec)?;
                let next = self.top_register(rec.saturating_add(1))?;
                self.set_top_register(rec.saturating_add(2), Value::Int(ITERATOR_PENDING))?;
                self.call_into_register(next, iterator, &[], dst)
            }
            Insn::AsyncClose {
                rec,
                kind,
                dst,
                offset,
            } => {
                if !self.async_close_start(rec, kind, dst)? {
                    self.jump_top(offset)?;
                }
                Ok(())
            }
            Insn::AsyncCloseEnd { reg, kind } => {
                if matches!(self.top_register(kind)?, Value::Int(COMPLETION_THROW)) {
                    return Ok(());
                }
                let result = self.top_register(reg)?;
                if matches!(
                    self.top_register(reg.saturating_add(1))?,
                    Value::Int(RESUME_THROW)
                ) {
                    return Err(VmError::Throw(result));
                }
                if !matches!(result, Value::Object(_)) {
                    return Err(self.iterator_result_error(result));
                }
                Ok(())
            }
            Insn::AsyncDelegateCall { rec, form } => self.async_delegate_call(rec, form),
            Insn::AsyncDelegateResult { rec, offset } => {
                if self.async_delegate_result(rec)? {
                    self.jump_top(offset)?;
                }
                Ok(())
            }
            _ => Err(VmError::invariant("not an async iteration instruction")),
        }
    }

    fn top_frame(&self) -> VmResult<&crate::vm::Frame> {
        self.vm.frames.last().ok_or(VmError::invariant("no frame"))
    }

    /// Moves the saved pc of the top frame by a forward jump `offset`
    /// (relative to the instruction that ran).
    fn jump_top(&mut self, offset: i32) -> VmResult<()> {
        let frame = self
            .vm
            .frames
            .last_mut()
            .ok_or(VmError::invariant("no frame"))?;
        let target = i64::from(frame.pc) - 1 + i64::from(offset);
        frame.pc =
            u32::try_from(target).map_err(|_| VmError::invariant("a jump before the code"))?;
        Ok(())
    }

    /// Calls `callee` with `this` and `args` for the top frame, the result
    /// going into its register `dst`: script code runs as a frame of the
    /// interpreter loop (pushed here; the loop continues in it), a native
    /// function's result is written at once.
    fn call_into_register(
        &mut self,
        callee: Value,
        this: Value,
        args: &[Value],
        dst: Reg,
    ) -> VmResult<()> {
        let frame = self.top_frame()?;
        let restore_top = frame.base as usize + frame.compiled.register_count as usize;
        let window = self.push_window(callee, this, args)?;
        let site = CallSite {
            window,
            argc: args.len(),
            ret: ReturnTo::Register(dst),
            restore_top,
            construct: false,
            new_target: None,
        };
        match self.invoke(site)? {
            Step::Value(value) => {
                self.vm.stack.truncate(restore_top);
                self.set_top_register(dst, value)
            }
            Step::Entered => Ok(()),
            Step::NotCallable => {
                self.vm.stack.truncate(restore_top);
                Err(self.not_callable(callee))
            }
        }
    }

    /// The first half of `AsyncIteratorClose` (§7.4.13 steps 3 and 4.a
    /// to 4.c): returns whether `return` was called (its result is in
    /// `dst` for the await). After a throw completion in `kind`, the
    /// errors of the read and the call are dropped (a termination or an
    /// engine error is not).
    fn async_close_start(&mut self, rec: Reg, kind: Reg, dst: Reg) -> VmResult<bool> {
        let state = rec.saturating_add(2);
        if !matches!(self.top_register(state)?, Value::Bool(false)) {
            return Ok(false);
        }
        self.set_top_register(state, Value::Bool(true))?;
        let iterator = self.top_register(rec)?;
        let throw = matches!(self.top_register(kind)?, Value::Int(COMPLETION_THROW));
        let key = PropertyKey::String(self.vm.atoms.return_);
        // The scope closes after an error too: a dropped error does not
        // reach the unwinder.
        let called = self.scoped(|rt| match rt.get_method(iterator, key)? {
            None => Ok(None),
            Some(method) => rt.call(method, iterator, &[]).map(Some),
        });
        match called {
            Ok(None) => Ok(false),
            Ok(Some(result)) => {
                self.set_top_register(dst, result)?;
                Ok(true)
            }
            Err(VmError::Throw(_) | VmError::Raise { .. }) if throw => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The call of one async `yield*` step (§15.5.5 step 8 with genKind
    /// async); see [`crate::bytecode::Insn::AsyncDelegateCall`]. A `next`
    /// or `throw` that is not callable is V8's `TypeError` with the value's
    /// type for every form of the operand; a `return` gets the message of
    /// `form` (measured in Chromium 148).
    fn async_delegate_call(&mut self, rec: Reg, form: u16) -> VmResult<()> {
        let iterator = self.top_register(rec)?;
        let next = self.top_register(rec.saturating_add(1))?;
        let received = self.top_register(rec.saturating_add(2))?;
        let mode = self.top_register(rec.saturating_add(3))?;
        let result = rec.saturating_add(4);
        let (method, args): (Value, &[Value]) = match mode {
            Value::Int(RESUME_NEXT) => (next, &[received]),
            Value::Int(RESUME_THROW) => {
                let key = PropertyKey::String(self.vm.atoms.throw_);
                match self.with_scope(|rt| rt.get_value(iterator, key))? {
                    method if !method.is_nullish() => (method, &[received]),
                    _ => {
                        // AsyncIteratorClose with a normal completion, then
                        // the protocol error.
                        let key = PropertyKey::String(self.vm.atoms.return_);
                        let close = self.with_scope(|rt| rt.get_method(iterator, key))?;
                        let Some(close) = close else {
                            return Err(VmError::type_error(NO_THROW_METHOD));
                        };
                        self.set_top_register(rec.saturating_add(3), Value::Int(DELEGATE_CLOSE))?;
                        self.set_top_register(result, close)?;
                        return self.call_into_register(close, iterator, &[], result);
                    }
                }
            }
            Value::Int(RESUME_RETURN) => {
                let key = PropertyKey::String(self.vm.atoms.return_);
                match self.with_scope(|rt| rt.get_value(iterator, key))? {
                    method if !method.is_nullish() => (method, &[received]),
                    _ => {
                        self.set_top_register(
                            rec.saturating_add(3),
                            Value::Int(DELEGATE_RETURN_MISSING),
                        )?;
                        return self.set_top_register(result, received);
                    }
                }
            }
            _ => return Err(VmError::invariant("a yield* step without a mode")),
        };
        // The register keeps the method alive until it is in the window.
        self.set_top_register(result, method)?;
        if !self.is_callable(method)? {
            if mode == Value::Int(RESUME_RETURN) && form != DELEGATE_TYPED {
                return Err(VmError::type_error(YIELD_STAR_NOT_ASYNC_ITERABLE));
            }
            return Err(self.not_callable(method));
        }
        self.call_into_register(method, iterator, args, result)
    }

    /// The rest of an async `yield*` step after its await; returns
    /// whether the loop ends (the value is in `rec + 2`).
    fn async_delegate_result(&mut self, rec: Reg) -> VmResult<bool> {
        let awaited = self.top_register(rec.saturating_add(4))?;
        if matches!(
            self.top_register(rec.saturating_add(5))?,
            Value::Int(RESUME_THROW)
        ) {
            return Err(VmError::Throw(awaited));
        }
        let mode_register = rec.saturating_add(3);
        let value_register = rec.saturating_add(2);
        match self.top_register(mode_register)? {
            Value::Int(DELEGATE_CLOSE) => {
                if !matches!(awaited, Value::Object(_)) {
                    return Err(self.iterator_result_error(awaited));
                }
                Err(VmError::type_error(NO_THROW_METHOD))
            }
            Value::Int(DELEGATE_RETURN_MISSING) => {
                self.set_top_register(value_register, awaited)?;
                self.set_top_register(mode_register, Value::Int(RESUME_RETURN))?;
                Ok(true)
            }
            mode => {
                // The register roots the result while its getters run.
                let Some(value) = self.with_scope(|rt| rt.delegate_result(awaited))? else {
                    self.set_top_register(value_register, awaited)?;
                    return Ok(false);
                };
                self.set_top_register(value_register, value)?;
                // A throw that the delegate handled continues normally.
                if mode == Value::Int(RESUME_THROW) {
                    self.set_top_register(mode_register, Value::Int(RESUME_NEXT))?;
                }
                Ok(true)
            }
        }
    }

    /// The record of an Async-from-Sync Iterator (the `this` of its
    /// methods, which no script can reach).
    fn sync_record_of(&self, value: Value) -> VmResult<IteratorRecord> {
        if let Value::Object(object) = value
            && let ObjectKind::AsyncFromSyncIterator(state) = &self.heap.object(object)?.kind
        {
            return Ok(state.record);
        }
        Err(VmError::invariant(
            "an Async-from-Sync Iterator method on another object",
        ))
    }

    /// `AsyncFromSyncIteratorContinuation(result, promiseCapability,
    /// syncIteratorRecord, closeOnRejection)` (§27.1.6.4).
    fn async_from_sync_continuation(
        &mut self,
        result: Value,
        promise: Gc<Object>,
        record: &IteratorRecord,
        close_on_rejection: bool,
        realm: u32,
    ) -> VmResult<Value> {
        let capability = Capability::Intrinsic(promise);
        let done = self.get_value(result, PropertyKey::String(self.vm.atoms.done));
        let done = match self.reject_if_abrupt(done, &capability)? {
            Ok(done) => self.to_boolean(done)?,
            Err(promise) => return Ok(promise),
        };
        let value = self.get_value(result, PropertyKey::String(self.vm.atoms.value));
        let value = match self.reject_if_abrupt(value, &capability)? {
            Ok(value) => value,
            Err(promise) => return Ok(promise),
        };
        self.async_from_sync_wrap(value, done, promise, record, close_on_rejection, realm)
    }

    /// The rest of `AsyncFromSyncIteratorContinuation` (§27.1.6.4 steps 5
    /// to 13) with the value and `done` of the sync result.
    fn async_from_sync_wrap(
        &mut self,
        value: Value,
        done: bool,
        promise: Gc<Object>,
        record: &IteratorRecord,
        close_on_rejection: bool,
        realm: u32,
    ) -> VmResult<Value> {
        let capability = Capability::Intrinsic(promise);
        let constructor = self.intrinsic(realm, Intrinsic::PromiseConstructor)?;
        let wrapper = self.promise_resolve(constructor, value, realm);
        let wrapper = match self.completion(wrapper)? {
            Ok(wrapper) => wrapper,
            Err(error) => {
                if !done && close_on_rejection {
                    // IteratorClose with the throw completion: the errors
                    // of `return` are dropped.
                    let close = self.iterator_close_after_error(record, VmError::Throw(error));
                    if let error @ (VmError::Terminated(_) | VmError::Internal(_)) = close {
                        return Err(error);
                    }
                }
                self.reject_promise(promise, error)?;
                return Ok(promise.into());
            }
        };
        let Value::Object(wrapper) = wrapper else {
            return Err(VmError::invariant("PromiseResolve without a promise"));
        };
        let on_fulfilled = Handler::Internal(Continuation {
            func: unwrap,
            data: Value::Bool(done),
        });
        let on_rejected = if done || !close_on_rejection {
            Handler::Empty
        } else {
            Handler::Internal(Continuation {
                func: close_sync_iterator,
                data: record.iterator,
            })
        };
        self.perform_promise_then(wrapper, on_fulfilled, on_rejected, Some(capability))?;
        Ok(promise.into())
    }
}

/// `%AsyncFromSyncIteratorPrototype%.next(value)` (§27.1.6.2.1).
pub(crate) fn from_sync_next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let realm = call.realm();
    let promise = rt.new_promise(realm)?;
    let record = rt.sync_record_of(call.this())?;
    rt.tick()?;
    if let Some(builtin) = rt.builtin_step(record.iterator, record.next_method) {
        // The built-in `next` of an Array, Map or Set Iterator: the step
        // without its result object, which nothing can observe.
        // `Empty` (never a value of the step) stands for done.
        let step = crate::builtins::builtin_iterator_next_value(rt, builtin)
            .map(|step| step.unwrap_or(Value::Empty));
        let (value, done) = match rt.completion(step)? {
            Ok(Value::Empty) => (Value::Undefined, true),
            Ok(value) => (value, false),
            Err(reason) => {
                rt.reject_promise(promise, reason)?;
                return Ok(NativeReturn::Value(promise.into()));
            }
        };
        let value = rt.async_from_sync_wrap(value, done, promise, &record, true, realm)?;
        return Ok(NativeReturn::Value(value));
    }
    let args: &[Value] = if call.argc() > 0 {
        &[rt.arg(call, 0)]
    } else {
        &[]
    };
    let result = rt.call(record.next_method, record.iterator, args);
    let result = match result {
        Ok(result) if !matches!(result, Value::Object(_)) => Err(rt.iterator_result_error(result)),
        other => other,
    };
    let capability = Capability::Intrinsic(promise);
    let result = match rt.reject_if_abrupt(result, &capability)? {
        Ok(result) => result,
        Err(promise) => return Ok(NativeReturn::Value(promise)),
    };
    let value = rt.async_from_sync_continuation(result, promise, &record, true, realm)?;
    Ok(NativeReturn::Value(value))
}

/// `%AsyncFromSyncIteratorPrototype%.return(value)` (§27.1.6.2.2).
pub(crate) fn from_sync_return(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    from_sync_abrupt(rt, call, false)
}

/// `%AsyncFromSyncIteratorPrototype%.throw(value)` (§27.1.6.2.3).
pub(crate) fn from_sync_throw(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    from_sync_abrupt(rt, call, true)
}

/// `return` and `throw` of an Async-from-Sync Iterator.
fn from_sync_abrupt(rt: &mut Runtime, call: &NativeCall, throw: bool) -> VmResult<NativeReturn> {
    let realm = call.realm();
    let promise = rt.new_promise(realm)?;
    let capability = Capability::Intrinsic(promise);
    let record = rt.sync_record_of(call.this())?;
    let key = PropertyKey::String(if throw {
        rt.vm.atoms.throw_
    } else {
        rt.vm.atoms.return_
    });
    let method = rt.get_method(record.iterator, key);
    let method = match method {
        Ok(method) => Ok(method.unwrap_or(Value::Undefined)),
        Err(error) => Err(error),
    };
    let method = match rt.reject_if_abrupt(method, &capability)? {
        Ok(method) => method,
        Err(promise) => return Ok(NativeReturn::Value(promise)),
    };
    let value = rt.arg(call, 0);
    if method.is_undefined() {
        if throw {
            // Close the sync iterator, then the protocol error.
            let closed = rt.iterator_close(&record).map(|()| Value::Undefined);
            if let Err(promise) = rt.reject_if_abrupt(closed, &capability)? {
                return Ok(NativeReturn::Value(promise));
            }
            let error = rt.thrown_value(VmError::type_error(NO_THROW_METHOD))?;
            rt.heap.record(error);
            rt.reject_promise(promise, error)?;
        } else {
            let result = rt.iter_result(value, true)?;
            rt.heap.record(result);
            rt.resolve_promise(promise, result.into())?;
        }
        return Ok(NativeReturn::Value(promise.into()));
    }
    let args: &[Value] = if call.argc() > 0 { &[value] } else { &[] };
    let result = rt.call(method, record.iterator, args);
    let result = match result {
        Ok(result) if !matches!(result, Value::Object(_)) => Err(rt.iterator_result_error(result)),
        other => other,
    };
    let result = match rt.reject_if_abrupt(result, &capability)? {
        Ok(result) => result,
        Err(promise) => return Ok(NativeReturn::Value(promise)),
    };
    let value = rt.async_from_sync_continuation(result, promise, &record, throw, realm)?;
    Ok(NativeReturn::Value(value))
}

/// The `unwrap` closure of §27.1.6.4 step 6: `{ value, done }` with the
/// `done` of the sync result (the data).
fn unwrap(rt: &mut Runtime, data: Value, _kind: ReactionKind, value: Value) -> VmResult<Value> {
    let done = matches!(data, Value::Bool(true));
    let result = rt.iter_result(value, done)?;
    Ok(rt.heap.record(result).into())
}

/// The `closeIterator` closure of §27.1.6.4 step 10 (the 2023 change):
/// closes the sync iterator (the data) after its value rejected, and
/// rejects with the reason; the errors of `return` are dropped.
fn close_sync_iterator(
    rt: &mut Runtime,
    data: Value,
    _kind: ReactionKind,
    reason: Value,
) -> VmResult<Value> {
    let record = IteratorRecord {
        iterator: data,
        next_method: Value::Undefined,
        done: false,
    };
    Err(rt.iterator_close_after_error(&record, VmError::Throw(reason)))
}
