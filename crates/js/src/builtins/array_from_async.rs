//! `Array.fromAsync(items, mapper, thisArg)` (ECMA-262 §23.1.2.2 of the
//! 2026 draft, <https://tc39.es/ecma262/#sec-array.fromasync>).
//!
//! The specification writes it as an async function. Here it is a state
//! machine in an internal object ([`FromAsync`]): each `Await` is
//! `PromiseResolve(%Promise%, value)` and an internal reaction whose data
//! is the state object, and the reaction's job runs the next step. So a
//! million elements cost a million jobs and no Rust stack, a termination
//! in a step leaves the result promise pending, and a state that waits for
//! a promise that no one can settle is garbage. A throw in a step rejects
//! the result promise; after a throw from the mapper or from the
//! definition of an element, the iterator is closed first
//! (`IfAbruptCloseAsyncIterator`, which awaits the result of `return`).
//!
//! V8's messages (measured in Chromium 148): a mapper that is not callable
//! `5 is not a function`; a method of `items` that is not callable
//! `Array.fromAsync requires that the property of the first argument,
//! items[Symbol.asyncIterator], when exists, be a function` (also for
//! `Symbol.iterator`); `undefined` and `null` fail at the read of
//! `Symbol.asyncIterator`; an iterator result that is not an object
//! `Iterator result Array.fromAsync is not an object`; an async iterator
//! method whose result is not an object `Result of the Symbol.iterator
//! method is not an object`.
//!
//! Deliberate differences from Chromium 148 (which follows an older text
//! of the proposal): V8 also closes the iterator when `next` throws, when
//! its promise rejects and when the `value` getter throws, calls `return`
//! of an Async-from-Sync Iterator a second time after a value rejected,
//! and calls `return` again in every job (never settling the promise) when
//! the close itself fails; swb follows the specification.

use crate::heap::{Gc, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::checked_array_length;
use crate::vm::iter::IteratorRecord;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, ReactionKind, VmError, VmResult, WellKnown};

/// The step that an `Await` of the state waits for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// The result of the iterator's `next` (step 3.j.ii.5).
    Next,
    /// The mapped value of an iterator's value (step 3.j.ii.11.b).
    IteratorMapped,
    /// An element of the array-like (step 4.f.iii).
    Element,
    /// The mapped value of an element (step 4.f.iv.2).
    ElementMapped,
    /// The result of `return` after an error; the result promise is
    /// rejected with the error afterwards.
    Closing(Value),
}

/// The variables of a running `Array.fromAsync`.
pub struct FromAsync {
    /// The result promise.
    promise: Gc<Object>,
    /// The array being filled.
    array: Gc<Object>,
    /// The mapper; `undefined` when not mapping.
    mapper: Value,
    this_arg: Value,
    /// The iterator (the iterable form), or `None` (the array-like form).
    record: Option<IteratorRecord>,
    /// The array-like object and its length (the array-like form).
    array_like: Value,
    length: f64,
    /// The next index.
    k: f64,
    phase: Phase,
}

impl FromAsync {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.promise);
        tracer.object(self.array);
        tracer.value(self.mapper);
        tracer.value(self.this_arg);
        if let Some(record) = &self.record {
            tracer.value(record.iterator);
            tracer.value(record.next_method);
        }
        tracer.value(self.array_like);
        if let Phase::Closing(error) = self.phase {
            tracer.value(error);
        }
    }
}

/// V8's message for a result of `next` that is not an object.
const NOT_AN_OBJECT: &str = "Iterator result Array.fromAsync is not an object";

/// The message of a method of `items` that is not callable.
fn method_error(name: &str) -> VmError {
    VmError::type_error(format!(
        "Array.fromAsync requires that the property of the first argument, items[Symbol.{name}], when exists, be a function"
    ))
}

/// `Array.fromAsync(items, mapper, thisArg)`: the steps before the first
/// `Await` run here; a throw rejects the result promise.
pub(super) fn from_async(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let promise = rt.new_promise(call.realm())?;
    let started = start(rt, call, promise).map(|()| Value::Undefined);
    if let Err(reason) = rt.completion(started)? {
        rt.reject_promise(promise, reason)?;
    }
    Ok(NativeReturn::Value(promise.into()))
}

/// Steps 1 to 4 up to the first `Await`.
fn start(rt: &mut Runtime, call: &NativeCall, promise: Gc<Object>) -> VmResult<()> {
    let constructor = call.this();
    let items = rt.arg(call, 0);
    let mapper = rt.arg(call, 1);
    let this_arg = rt.arg(call, 2);
    if !mapper.is_undefined() && !rt.is_callable(mapper)? {
        return Err(VmError::type_error(format!(
            "{} is not a function",
            rt.value_text(mapper)
        )));
    }
    let key = rt.symbol_key(WellKnown::AsyncIterator);
    let async_method = rt.get_value(items, key)?;
    let record = if async_method.is_nullish() {
        let key = rt.symbol_key(WellKnown::Iterator);
        let sync_method = rt.get_value(items, key)?;
        if sync_method.is_nullish() {
            None
        } else if !rt.is_callable(sync_method)? {
            return Err(method_error("iterator"));
        } else {
            let record = rt.get_iterator_from_method(items, sync_method)?;
            Some(rt.create_async_from_sync_iterator(record)?)
        }
    } else if !rt.is_callable(async_method)? {
        return Err(method_error("asyncIterator"));
    } else {
        let iterator = rt.call(async_method, items, &[])?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(VmError::type_error(
                "Result of the Symbol.iterator method is not an object",
            ));
        }
        let key = PropertyKey::String(rt.vm.atoms.next);
        let next_method = rt.get_value(iterator, key)?;
        rt.heap.record(next_method);
        Some(IteratorRecord {
            iterator,
            next_method,
            done: false,
        })
    };
    let is_constructor = rt.is_constructor(constructor)?;
    let (array, array_like, length) = if record.is_some() {
        let array = if is_constructor {
            construct_object(rt, constructor, &[])?
        } else {
            rt.new_array_with_capacity(0)?
        };
        (array, Value::Undefined, 0.0)
    } else {
        let array_like = rt.to_object(items)?;
        rt.heap.record(array_like);
        let length = rt.length_of_array_like(array_like)?;
        let array = if is_constructor {
            construct_object(rt, constructor, &[Value::number(length)])?
        } else {
            checked_array_length(length)?;
            rt.new_array_with_capacity(0)?
        };
        (array, Value::Object(array_like), length)
    };
    let state = FromAsync {
        promise,
        array,
        mapper,
        this_arg,
        record,
        array_like,
        length,
        k: 0.0,
        phase: Phase::Next,
    };
    let object = rt
        .heap
        .new_object_with_kind(None, ObjectKind::ArrayFromAsync(Box::new(state)))?;
    rt.heap.record(object);
    if record.is_some() {
        next_step(rt, object)
    } else {
        element_step(rt, object)
    }
}

/// `Construct(constructor, args)`; the result is an object.
fn construct_object(rt: &mut Runtime, constructor: Value, args: &[Value]) -> VmResult<Gc<Object>> {
    match rt.construct(constructor, args)? {
        Value::Object(object) => Ok(object),
        _ => Err(VmError::invariant("a constructor returned a non-object")),
    }
}

impl Runtime {
    fn array_from_async_state(&mut self, object: Gc<Object>) -> VmResult<&mut FromAsync> {
        match &mut self.heap.object_mut(object)?.kind {
            ObjectKind::ArrayFromAsync(state) => Ok(state),
            _ => Err(VmError::invariant(
                "an Array.fromAsync step without its state",
            )),
        }
    }

    /// `Await(value)` of a step: the reaction resumes [`resume`] in
    /// `phase`. A throw of `PromiseResolve` is the step's error.
    fn state_await(&mut self, object: Gc<Object>, value: Value, phase: Phase) -> VmResult<()> {
        self.array_from_async_state(object)?.phase = phase;
        let realm = self.current_realm();
        let constructor = self.intrinsic(realm, Intrinsic::PromiseConstructor)?;
        let Value::Object(promise) = self.promise_resolve(constructor, value, realm)? else {
            return Err(VmError::invariant("PromiseResolve without a promise"));
        };
        self.then_internal(promise, resume, object.into())
    }
}

/// The steps of the iterable form from the call of `next` (step 3.j.ii.1
/// to 3.j.ii.5).
fn next_step(rt: &mut Runtime, object: Gc<Object>) -> VmResult<()> {
    rt.tick()?;
    let state = rt.array_from_async_state(object)?;
    let Some(record) = state.record else {
        return Err(VmError::invariant("an iterator step without an iterator"));
    };
    let result = rt.call(record.next_method, record.iterator, &[])?;
    rt.state_await(object, result, Phase::Next)
}

/// The steps of the array-like form from the check of `k` (step 4.f).
fn element_step(rt: &mut Runtime, object: Gc<Object>) -> VmResult<()> {
    rt.tick()?;
    let state = rt.array_from_async_state(object)?;
    let (k, length, array, array_like) = (state.k, state.length, state.array, state.array_like);
    if k >= length {
        rt.set_length(array, length)?;
        return finish(rt, object, Ok(array.into()));
    }
    let Value::Object(array_like) = array_like else {
        return Err(VmError::invariant("an element step without an array-like"));
    };
    let value = rt.get_index(array_like, k)?;
    rt.heap.record(value);
    rt.state_await(object, value, Phase::Element)
}

/// Settles the result promise: fulfilled with the array, or rejected.
fn finish(rt: &mut Runtime, object: Gc<Object>, result: Result<Value, Value>) -> VmResult<()> {
    let promise = rt.array_from_async_state(object)?.promise;
    match result {
        Ok(value) => rt.resolve_promise(promise, value),
        Err(reason) => rt.reject_promise(promise, reason),
    }
}

/// The continuation of the `Await` of a step (`data` is the state). A
/// throw that the step does not handle rejects the result promise; a
/// termination passes through.
fn resume(rt: &mut Runtime, data: Value, kind: ReactionKind, value: Value) -> VmResult<Value> {
    let Value::Object(object) = data else {
        return Err(VmError::invariant(
            "an Array.fromAsync step without its state",
        ));
    };
    let settled = match kind {
        ReactionKind::Fulfill => Ok(value),
        ReactionKind::Reject => Err(value),
    };
    let phase = rt.array_from_async_state(object)?.phase;
    // The scope closes after an error too: the error is handled here.
    let step = rt.scoped(|rt| run_phase(rt, object, phase, settled));
    if let Err(reason) = rt.completion(step.map(|()| Value::Undefined))? {
        finish(rt, object, Err(reason))?;
    }
    Ok(Value::Undefined)
}

/// One step after an `Await` of `phase` settled. The errors after the
/// value of an iterator close it (`IfAbruptCloseAsyncIterator`); an `Err`
/// that the step returns rejects the result promise (in [`resume`]).
fn run_phase(
    rt: &mut Runtime,
    object: Gc<Object>,
    phase: Phase,
    settled: Result<Value, Value>,
) -> VmResult<()> {
    match phase {
        Phase::Next => {
            let result = settled.map_err(VmError::Throw)?;
            if !matches!(result, Value::Object(_)) {
                return Err(VmError::type_error(NOT_AN_OBJECT));
            }
            let done = rt.get_value(result, PropertyKey::String(rt.vm.atoms.done))?;
            if rt.to_boolean(done)? {
                let state = rt.array_from_async_state(object)?;
                let (array, k) = (state.array, state.k);
                rt.set_length(array, k)?;
                return finish(rt, object, Ok(array.into()));
            }
            let value = rt.get_value(result, PropertyKey::String(rt.vm.atoms.value))?;
            rt.heap.record(value);
            let mapped = map_value(rt, object, value, Phase::IteratorMapped);
            match rt.completion(mapped)? {
                // The mapped value is awaited.
                Ok(Value::Empty) => Ok(()),
                Ok(value) => store_and_continue(rt, object, value),
                Err(reason) => close(rt, object, reason),
            }
        }
        Phase::IteratorMapped => match settled {
            Ok(value) => store_and_continue(rt, object, value),
            Err(reason) => close(rt, object, reason),
        },
        Phase::Element => {
            let value = settled.map_err(VmError::Throw)?;
            match map_value(rt, object, value, Phase::ElementMapped)? {
                Value::Empty => Ok(()),
                value => store_and_continue(rt, object, value),
            }
        }
        Phase::ElementMapped => {
            let value = settled.map_err(VmError::Throw)?;
            store_and_continue(rt, object, value)
        }
        Phase::Closing(error) => finish(rt, object, Err(error)),
    }
}

/// The mapping of a value: without a mapper the value itself; else the
/// call of the mapper and the `Await` of its result (`Empty`: awaiting).
fn map_value(rt: &mut Runtime, object: Gc<Object>, value: Value, phase: Phase) -> VmResult<Value> {
    let state = rt.array_from_async_state(object)?;
    let (mapper, this_arg, k) = (state.mapper, state.this_arg, state.k);
    if mapper.is_undefined() {
        return Ok(value);
    }
    let mapped = rt.call(mapper, this_arg, &[value, Value::number(k)])?;
    rt.state_await(object, mapped, phase)?;
    Ok(Value::Empty)
}

/// `CreateDataPropertyOrThrow(array, k, value)` (a failure closes the
/// iterator), then the next step.
fn store_and_continue(rt: &mut Runtime, object: Gc<Object>, value: Value) -> VmResult<()> {
    let state = rt.array_from_async_state(object)?;
    let (array, k, iterable) = (state.array, state.k, state.record.is_some());
    let defined = rt.create_index(array, k, value).map(|()| Value::Undefined);
    if let Err(reason) = rt.completion(defined)? {
        if iterable {
            return close(rt, object, reason);
        }
        return Err(VmError::Throw(reason));
    }
    rt.array_from_async_state(object)?.k = k + 1.0;
    if iterable {
        next_step(rt, object)
    } else {
        element_step(rt, object)
    }
}

/// `AsyncIteratorClose(iteratorRecord, ThrowCompletion(error))`
/// (§7.4.13): calls `return` and awaits its result; its errors are
/// dropped. The result promise is rejected with `error` at the end.
fn close(rt: &mut Runtime, object: Gc<Object>, error: Value) -> VmResult<()> {
    let Some(record) = rt.array_from_async_state(object)?.record else {
        return finish(rt, object, Err(error));
    };
    let closed = rt.scoped(|rt| {
        let key = PropertyKey::String(rt.vm.atoms.return_);
        let Some(method) = rt.get_method(record.iterator, key)? else {
            return Ok(false);
        };
        let result = rt.call(method, record.iterator, &[])?;
        rt.state_await(object, result, Phase::Closing(error))?;
        Ok(true)
    });
    match closed {
        Ok(true) => Ok(()),
        Ok(false) | Err(VmError::Throw(_) | VmError::Raise { .. }) => {
            finish(rt, object, Err(error))
        }
        Err(other) => Err(other),
    }
}
