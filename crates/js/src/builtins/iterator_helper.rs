//! Iterator helpers (ECMA-262 §27.1.4, <https://tc39.es/ecma262/#sec-iteratorprototype-object>
//! and §27.1.2.1 `%IteratorHelperPrototype%`): `Iterator.prototype.map`,
//! `filter`, `take`, `drop` and `flatMap`, the helper objects they return
//! and the iterator sequencing helper of `Iterator.concat` (which
//! `iterator_from.rs` creates).
//!
//! The specification writes each helper as an abstract closure that
//! yields (`CreateIteratorFromClosure`, §27.1.2.1 and §27.5.3.1). swb keeps
//! the variables of the closure in the object (`object/iterator_helper.rs`,
//! traced by the collector) and runs one step of the closure per `next`:
//! a native state machine with the states of a generator. A step starts
//! at the top of the closure's loop, because every `Yield` of the
//! specification is at the end of an iteration.
//!
//! * `next` (`GeneratorResume`, §27.5.3.3): a helper that is executing
//!   throws a `TypeError` (V8: "Generator is already running"); a completed
//!   one returns `{ value: undefined, done: true }`; otherwise the step
//!   runs. A step that returns without a value, or throws, completes the
//!   helper.
//! * `return` (§27.1.2.1.2, `GeneratorResumeAbrupt`): before the first
//!   `next` it completes the helper and closes the underlying iterator;
//!   while suspended at a yield it runs the `IfAbruptCloseIterator` that
//!   follows the `Yield` (for `flatMap` and `concat` the inner iterator
//!   first); a completed helper does nothing.
//! * A callback that throws closes the underlying iterator with that
//!   error (`IfAbruptCloseIterator`); a failure of the underlying `next`
//!   does not close it.
//!
//! Stepping a helper whose underlying iterator is a built-in iterator or
//! another helper with the built-in `next` takes no result object
//! (`Runtime::builtin_step`); the nesting charges the recursion budget, so
//! a chain of helpers that is too deep ends with a `RangeError`, as in V8.

use crate::heap::Gc;
use crate::object::{HelperOp, HelperStatus, IteratorHelper, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::iter::IteratorRecord;
use crate::vm::{
    Intrinsic, NativeCall, NativeReturn, STACK_OVERFLOW, VmError, VmResult, WellKnown,
};

/// The recursion budget that a helper charges when it runs a step that
/// another step called directly (no call of `next` through the
/// interpreter). Measured in `tests/language_hostile.rs`; the Rust frames
/// of a step are far smaller than those of a call through the
/// interpreter (`REENTRY_WEIGHT`).
const STEP_WEIGHT: u32 = 2 * 1024;

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The V8 text of the `TypeError` for a helper that is running.
const ALREADY_RUNNING: &str = "Generator is already running";

// --- The methods of Iterator.prototype that make helpers ---

/// `Iterator.prototype.map(mapper)` (<https://tc39.es/ecma262/#sec-iterator.prototype.map>).
pub(super) fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    with_function(rt, call, "map", HelperOp::Map)
}

/// `Iterator.prototype.filter(predicate)` (<https://tc39.es/ecma262/#sec-iterator.prototype.filter>).
pub(super) fn filter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    with_function(rt, call, "filter", HelperOp::Filter)
}

/// `Iterator.prototype.flatMap(mapper)` (<https://tc39.es/ecma262/#sec-iterator.prototype.flatMap>).
pub(super) fn flat_map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    with_function(rt, call, "flatMap", HelperOp::FlatMap)
}

/// The common start of `map`, `filter` and `flatMap`: the receiver is an
/// object, the function is callable (else the iterator is closed), then
/// `GetIteratorDirect`.
fn with_function(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    op: HelperOp,
) -> VmResult<NativeReturn> {
    let this = receiver(call, name)?;
    let function = rt.arg(call, 0);
    if !rt.is_callable(function)? {
        let error = if op == HelperOp::FlatMap {
            // V8 words this one with the method's name.
            VmError::type_error("Iterator.prototype.flatMap is not a function")
        } else {
            rt.not_callable(function)
        };
        return Err(close_after_error(rt, this, error));
    }
    let underlying = get_iterator_direct(rt, this)?;
    create_helper(rt, call.realm(), op, underlying, function, 0.0)
}

/// `Iterator.prototype.take(limit)` (<https://tc39.es/ecma262/#sec-iterator.prototype.take>).
pub(super) fn take(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    with_limit(rt, call, "take", HelperOp::Take)
}

/// `Iterator.prototype.drop(limit)` (<https://tc39.es/ecma262/#sec-iterator.prototype.drop>).
pub(super) fn drop(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    with_limit(rt, call, "drop", HelperOp::Drop)
}

/// The common start of `take` and `drop`: `ToNumber(limit)`, a `RangeError`
/// for NaN and negative integers (both close the iterator), then
/// `GetIteratorDirect`.
fn with_limit(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    op: HelperOp,
) -> VmResult<NativeReturn> {
    let this = receiver(call, name)?;
    let limit = rt.arg(call, 0);
    let number = match rt.to_number(limit) {
        Ok(number) => number,
        Err(error) => return Err(close_after_error(rt, this, error)),
    };
    let integer = to_integer_or_infinity(number);
    // Finite limits above 2^53 - 1 are a `RangeError` too (test262; V8
    // in Chromium 148 accepts them).
    let too_large = number.is_finite() && number > MAX_SAFE_INTEGER;
    if number.is_nan() || too_large || integer < 0.0 {
        let error = VmError::range_error(format!("{} must be positive", rt.primitive_text(limit)));
        return Err(close_after_error(rt, this, error));
    }
    let underlying = get_iterator_direct(rt, this)?;
    create_helper(rt, call.realm(), op, underlying, Value::Undefined, integer)
}

/// The receiver of an `Iterator.prototype` method, which must be an
/// object (V8: "Iterator.prototype.map called on non-object").
pub(super) fn receiver(call: &NativeCall, name: &str) -> VmResult<Value> {
    match call.this() {
        this @ Value::Object(_) => Ok(this),
        _ => Err(VmError::type_error(format!(
            "Iterator.prototype.{name} called on non-object"
        ))),
    }
}

/// `IteratorClose(record, error)` (§7.4.11) for an iterator whose `next`
/// method has not been read yet: runs `return` and keeps `error`.
pub(super) fn close_after_error(rt: &mut Runtime, iterator: Value, error: VmError) -> VmError {
    let record = IteratorRecord {
        iterator,
        next_method: Value::Undefined,
        done: false,
    };
    rt.iterator_close_after_error(&record, error)
}

/// `GetIteratorDirect(obj)` (§7.4.4): the record of `obj` and its `next`
/// method, which is not checked for being callable.
pub(super) fn get_iterator_direct(rt: &mut Runtime, iterator: Value) -> VmResult<IteratorRecord> {
    let key = PropertyKey::String(rt.vm.atoms.next);
    let next_method = rt.get_value(iterator, key)?;
    Ok(IteratorRecord {
        iterator,
        next_method,
        done: false,
    })
}

/// Creates the helper object (`CreateIteratorFromClosure`, §27.1.2.1) in
/// the suspended-start state. The handles of the record and the function
/// are rooted by the caller.
pub(super) fn create_helper(
    rt: &mut Runtime,
    realm: u32,
    op: HelperOp,
    underlying: IteratorRecord,
    function: Value,
    count: f64,
) -> VmResult<NativeReturn> {
    let helper = IteratorHelper::new(op, underlying, function, count);
    new_helper_object(rt, realm, helper)
}

/// The object for a helper payload.
pub(super) fn new_helper_object(
    rt: &mut Runtime,
    realm: u32,
    helper: IteratorHelper,
) -> VmResult<NativeReturn> {
    let proto = rt.intrinsic(realm, Intrinsic::IteratorHelperPrototype)?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::IteratorHelper(Box::new(helper)))?;
    Ok(NativeReturn::Value(object.into()))
}

// --- %IteratorHelperPrototype% ---

/// `%IteratorHelperPrototype%.next()` (§27.1.2.1.1).
pub(super) fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let helper = this_helper(rt, call, "next")?;
    let value = next_value(rt, helper)?;
    let result = rt.iter_result(value.unwrap_or(Value::Undefined), value.is_none())?;
    Ok(NativeReturn::Value(result.into()))
}

/// `%IteratorHelperPrototype%.return()` (§27.1.2.1.2).
pub(super) fn return_(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let helper = this_helper(rt, call, "return")?;
    let snapshot = snapshot(rt, helper)?;
    match snapshot.status {
        HelperStatus::Executing => return Err(VmError::type_error(ALREADY_RUNNING)),
        HelperStatus::Completed => {}
        HelperStatus::SuspendedStart => {
            set_status(rt, helper, HelperStatus::Completed)?;
            // `concat` has no iterator open before its first step.
            if snapshot.op != HelperOp::Concat {
                close(rt, &snapshot.underlying)?;
            }
        }
        HelperStatus::SuspendedYield => {
            // The `Yield` completes abruptly with a return completion; the
            // closure runs the `IteratorClose` calls that follow it.
            set_status(rt, helper, HelperStatus::Executing)?;
            let closed = close_at_yield(rt, &snapshot);
            set_status(rt, helper, HelperStatus::Completed)?;
            closed?;
        }
    }
    let result = rt.iter_result(Value::Undefined, true)?;
    Ok(NativeReturn::Value(result.into()))
}

/// The `TypeError` for a receiver that is not a helper (V8's words).
fn incompatible(rt: &Runtime, receiver: Value, method: &str) -> VmError {
    VmError::type_error(format!(
        "Method Iterator Helper.prototype.{method} called on incompatible receiver {}",
        rt.primitive_text(receiver)
    ))
}

/// The receiver of `next` or `return` (`RequireInternalSlot`).
fn this_helper(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<Gc<Object>> {
    if let Value::Object(object) = call.this()
        && matches!(rt.heap.object(object)?.kind, ObjectKind::IteratorHelper(_))
    {
        return Ok(object);
    }
    Err(incompatible(rt, call.this(), method))
}

/// The state of a helper that a step needs (all `Copy`).
#[derive(Clone, Copy)]
pub(super) struct Snapshot {
    pub(super) status: HelperStatus,
    pub(super) op: HelperOp,
    pub(super) underlying: IteratorRecord,
    pub(super) function: Value,
    pub(super) count: f64,
    pub(super) inner: Option<IteratorRecord>,
}

pub(super) fn snapshot(rt: &Runtime, helper: Gc<Object>) -> VmResult<Snapshot> {
    match &rt.heap.object(helper)?.kind {
        ObjectKind::IteratorHelper(state) => Ok(Snapshot {
            status: state.status,
            op: state.op,
            underlying: state.underlying,
            function: state.function,
            count: state.count,
            inner: state.inner,
        }),
        _ => Err(VmError::invariant(
            "an iterator helper step on another object",
        )),
    }
}

/// Changes a field of the payload of a helper.
pub(super) fn update(
    rt: &mut Runtime,
    helper: Gc<Object>,
    change: impl FnOnce(&mut IteratorHelper),
) -> VmResult<()> {
    match &mut rt.heap.object_mut(helper)?.kind {
        ObjectKind::IteratorHelper(state) => {
            change(state);
            Ok(())
        }
        _ => Err(VmError::invariant(
            "an iterator helper step on another object",
        )),
    }
}

fn set_status(rt: &mut Runtime, helper: Gc<Object>, status: HelperStatus) -> VmResult<()> {
    update(rt, helper, |state| state.status = status)
}

/// `GeneratorResume` (§27.5.3.3) of a helper without the result object:
/// the next value, or `None` when the helper is done (the closure
/// returned, or it was completed before). The value is recorded in the
/// current handle scope. The compiled iteration steps and the natives
/// that consume an iterator call this for a helper whose `next` is the
/// built-in one (`Runtime::builtin_step`).
pub(crate) fn next_value(rt: &mut Runtime, helper: Gc<Object>) -> VmResult<Option<Value>> {
    match snapshot(rt, helper)?.status {
        HelperStatus::Executing => return Err(VmError::type_error(ALREADY_RUNNING)),
        HelperStatus::Completed => return Ok(None),
        HelperStatus::SuspendedStart | HelperStatus::SuspendedYield => {}
    }
    if rt.budget.enter(STEP_WEIGHT).is_err() {
        return Err(VmError::range_error(STACK_OVERFLOW));
    }
    set_status(rt, helper, HelperStatus::Executing)?;
    let result = run(rt, helper);
    rt.budget.leave(STEP_WEIGHT);
    let status = match result {
        Ok(Some(_)) => HelperStatus::SuspendedYield,
        _ => HelperStatus::Completed,
    };
    set_status(rt, helper, status)?;
    result
}

/// Runs the closure of a helper from the top of its loop to the next
/// `Yield` (`Some`) or to its end (`None`).
fn run(rt: &mut Runtime, helper: Gc<Object>) -> VmResult<Option<Value>> {
    let snapshot = snapshot(rt, helper)?;
    match snapshot.op {
        HelperOp::Map => step_map(rt, helper, snapshot),
        HelperOp::Filter => step_filter(rt, helper, snapshot),
        HelperOp::Take => step_take(rt, helper, snapshot),
        HelperOp::Drop => step_drop(rt, helper, snapshot),
        HelperOp::FlatMap => step_flat_map(rt, helper, snapshot),
        HelperOp::Concat => super::iterator_from::step_concat(rt, helper),
    }
}

/// Runs `body` in a handle scope of its own, so that a loop that skips
/// values does not grow its caller's scope; a yielded value passes to the
/// enclosing scope. After an error the scope stays open for the unwinder,
/// as with `Runtime::with_scope`.
pub(super) fn scoped_pass(
    rt: &mut Runtime,
    body: impl FnOnce(&mut Runtime) -> VmResult<PassResult>,
) -> VmResult<PassResult> {
    let scope = rt.heap.open_scope();
    match body(rt)? {
        PassResult::Yield(value) => Ok(PassResult::Yield(rt.heap.close_scope_with(scope, value)?)),
        other => {
            rt.heap.close_scope(scope)?;
            Ok(other)
        }
    }
}

/// The outcome of one pass over a loop (see [`scoped_pass`]).
pub(super) enum PassResult {
    /// Run another pass.
    Again,
    /// The closure returns.
    Done,
    /// The closure yields the value.
    Yield(Value),
}

/// `map` (<https://tc39.es/ecma262/#sec-iterator.prototype.map>).
fn step_map(rt: &mut Runtime, helper: Gc<Object>, snapshot: Snapshot) -> VmResult<Option<Value>> {
    let mut underlying = snapshot.underlying;
    let Some(value) = rt.iterator_step_value(&mut underlying)? else {
        return Ok(None);
    };
    let args = [value, Value::number(snapshot.count)];
    let mapped = match rt.call(snapshot.function, Value::Undefined, &args) {
        Ok(mapped) => mapped,
        Err(error) => return Err(rt.iterator_close_after_error(&underlying, error)),
    };
    update(rt, helper, |state| state.count += 1.0)?;
    Ok(Some(mapped))
}

/// `filter` (<https://tc39.es/ecma262/#sec-iterator.prototype.filter>).
fn step_filter(
    rt: &mut Runtime,
    helper: Gc<Object>,
    snapshot: Snapshot,
) -> VmResult<Option<Value>> {
    let mut underlying = snapshot.underlying;
    let mut counter = snapshot.count;
    loop {
        let pass = scoped_pass(rt, |rt| {
            let Some(value) = rt.iterator_step_value(&mut underlying)? else {
                return Ok(PassResult::Done);
            };
            let args = [value, Value::number(counter)];
            let selected = match rt.call(snapshot.function, Value::Undefined, &args) {
                Ok(selected) => selected,
                Err(error) => return Err(rt.iterator_close_after_error(&underlying, error)),
            };
            counter += 1.0;
            Ok(if rt.to_boolean(selected)? {
                PassResult::Yield(value)
            } else {
                PassResult::Again
            })
        })?;
        match pass {
            PassResult::Again => {}
            PassResult::Done => return Ok(None),
            PassResult::Yield(value) => {
                update(rt, helper, |state| state.count = counter)?;
                return Ok(Some(value));
            }
        }
    }
}

/// `take` (<https://tc39.es/ecma262/#sec-iterator.prototype.take>): `count` is `remaining`.
fn step_take(rt: &mut Runtime, helper: Gc<Object>, snapshot: Snapshot) -> VmResult<Option<Value>> {
    let mut underlying = snapshot.underlying;
    if snapshot.count == 0.0 {
        // `Return ? IteratorClose(iterated, ReturnCompletion(undefined))`.
        close(rt, &underlying)?;
        return Ok(None);
    }
    if snapshot.count != f64::INFINITY {
        update(rt, helper, |state| state.count = snapshot.count - 1.0)?;
    }
    rt.iterator_step_value(&mut underlying)
}

/// `drop` (<https://tc39.es/ecma262/#sec-iterator.prototype.drop>): `count` is `remaining`; the skipped steps use
/// `IteratorStep`, so they do not read `value`.
fn step_drop(rt: &mut Runtime, helper: Gc<Object>, snapshot: Snapshot) -> VmResult<Option<Value>> {
    let mut underlying = snapshot.underlying;
    let mut remaining = snapshot.count;
    while remaining > 0.0 {
        if remaining != f64::INFINITY {
            remaining -= 1.0;
        }
        let more = rt.with_scope(|rt| step_only(rt, &mut underlying))?;
        if !more {
            return Ok(None);
        }
    }
    update(rt, helper, |state| state.count = 0.0)?;
    rt.iterator_step_value(&mut underlying)
}

/// `IteratorStep` (§7.4.9) as a flag: whether the iterator produced
/// another result.
fn step_only(rt: &mut Runtime, record: &mut IteratorRecord) -> VmResult<bool> {
    if rt
        .builtin_step(record.iterator, record.next_method)
        .is_some()
    {
        // The built-in step reads the value as part of `next`.
        return Ok(rt.iterator_step_value(record)?.is_some());
    }
    Ok(rt.iterator_step(record)?.is_some())
}

/// `flatMap` (<https://tc39.es/ecma262/#sec-iterator.prototype.flatMap>). `inner` is the iterator being flattened; it is
/// stored in the object whenever it changes, since a pass ends its scope.
fn step_flat_map(
    rt: &mut Runtime,
    helper: Gc<Object>,
    snapshot: Snapshot,
) -> VmResult<Option<Value>> {
    let mut outer = snapshot.underlying;
    loop {
        let pass = scoped_pass(rt, |rt| {
            let state = self::snapshot(rt, helper)?;
            if let Some(mut inner) = state.inner {
                return match rt.iterator_step_value(&mut inner) {
                    Err(error) => Err(rt.iterator_close_after_error(&outer, error)),
                    Ok(Some(value)) => Ok(PassResult::Yield(value)),
                    Ok(None) => {
                        update(rt, helper, |state| {
                            state.inner = None;
                            state.count += 1.0;
                        })?;
                        Ok(PassResult::Again)
                    }
                };
            }
            let Some(value) = rt.iterator_step_value(&mut outer)? else {
                return Ok(PassResult::Done);
            };
            let args = [value, Value::number(state.count)];
            let mapped = match rt.call(snapshot.function, Value::Undefined, &args) {
                Ok(mapped) => mapped,
                Err(error) => return Err(rt.iterator_close_after_error(&outer, error)),
            };
            let inner = match super::iterator_from::get_iterator_flattenable(
                rt,
                mapped,
                "Iterator.prototype.flatMap",
                false,
            ) {
                Ok(inner) => inner,
                Err(error) => return Err(rt.iterator_close_after_error(&outer, error)),
            };
            update(rt, helper, |state| state.inner = Some(inner))?;
            Ok(PassResult::Again)
        })?;
        match pass {
            PassResult::Again => {}
            PassResult::Done => return Ok(None),
            PassResult::Yield(value) => return Ok(Some(value)),
        }
    }
}

// --- Closing ---

/// `IteratorClose(record, NormalCompletion(unused))` (§7.4.11), with V8's
/// message for a `return` that gives a non-object. A return completion
/// (`return()` of a helper, `take` at its limit) closes the same way.
pub(super) fn close(rt: &mut Runtime, record: &IteratorRecord) -> VmResult<()> {
    let key = PropertyKey::String(rt.vm.atoms.return_);
    let Some(method) = rt.get_method(record.iterator, key)? else {
        return Ok(());
    };
    let result = rt.call(method, record.iterator, &[])?;
    if !matches!(result, Value::Object(_)) {
        return Err(VmError::type_error("return called on non-object"));
    }
    Ok(())
}

/// The `IfAbruptCloseIterator` calls after the `Yield` of the closure,
/// run by `return()`: the inner iterator of `flatMap` is closed first,
/// and when that fails the outer iterator is closed with the failure.
fn close_at_yield(rt: &mut Runtime, snapshot: &Snapshot) -> VmResult<()> {
    match (snapshot.op, snapshot.inner) {
        (HelperOp::Concat, Some(inner)) => close(rt, &inner),
        (HelperOp::Concat, None) => Ok(()),
        (HelperOp::FlatMap, Some(inner)) => match close(rt, &inner) {
            Ok(()) => close(rt, &snapshot.underlying),
            Err(error) => Err(rt.iterator_close_after_error(&snapshot.underlying, error)),
        },
        _ => close(rt, &snapshot.underlying),
    }
}

/// The well-known symbol key of `@@iterator`.
pub(super) fn iterator_key(rt: &Runtime) -> PropertyKey {
    rt.symbol_key(WellKnown::Iterator)
}
