//! The promise combinators (ECMA-262 §27.2.4.1 to §27.2.4.5):
//! `Promise.all`, `allSettled`, `any` and `race`, with their element
//! functions.
//!
//! The specification's records that the element functions share (the list
//! of values or errors, the capability, `remainingElementsCount`) are one
//! internal array per call ([`State`]), so an element function has two
//! slots (the state and its index) and the loop holds no Rust vector that
//! a script sizes. An element function that has run sets its index slot
//! to `undefined` (`[[AlreadyCalled]]`); the pair of `allSettled` shares
//! a cell instead. Each step of the loop runs in its own handle scope.
//!
//! The capability always has real resolving functions: `all`, `any` and
//! `race` pass them to `then`.

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::iter::IteratorRecord;
use crate::vm::promise::{Capability, CapabilityRecord, cell_of};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

use super::promise::{invoke_then, this_constructor};

/// The combinator of a call.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    All,
    AllSettled,
    Any,
    Race,
}

impl Which {
    fn name(self) -> &'static str {
        match self {
            Which::All => "Promise.all",
            Which::AllSettled => "Promise.allSettled",
            Which::Any => "Promise.any",
            Which::Race => "Promise.race",
        }
    }
}

/// The positions in the state array.
const LIST: u32 = 0;
const PROMISE: u32 = 1;
const RESOLVE: u32 = 2;
const REJECT: u32 = 3;
const REMAINING: u32 = 4;

/// `Promise.all(iterable)` (§27.2.4.1).
pub(super) fn all(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    combinator(rt, call, Which::All)
}

/// `Promise.allSettled(iterable)` (§27.2.4.2).
pub(super) fn all_settled(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    combinator(rt, call, Which::AllSettled)
}

/// `Promise.any(iterable)` (§27.2.4.3).
pub(super) fn any(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    combinator(rt, call, Which::Any)
}

/// `Promise.race(iterable)` (§27.2.4.5).
pub(super) fn race(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    combinator(rt, call, Which::Race)
}

/// The common steps: the capability, `GetPromiseResolve`, `GetIterator`,
/// the loop, and the close and rejection after an error.
fn combinator(rt: &mut Runtime, call: &NativeCall, which: Which) -> VmResult<NativeReturn> {
    let constructor = this_constructor(call, which.name())?;
    let realm = call.realm();
    let capability = rt.new_promise_capability(constructor.into(), realm, true)?;
    // GetPromiseResolve (§27.2.4.1.1).
    let key = rt.heap.key_from_str("resolve")?;
    let promise_resolve = rt.get_value(constructor.into(), key).and_then(|resolve| {
        if rt.is_callable(resolve)? {
            Ok(resolve)
        } else {
            Err(VmError::type_error("resolve is not a function"))
        }
    });
    let promise_resolve = match rt.reject_if_abrupt(promise_resolve, &capability)? {
        Ok(resolve) => resolve,
        Err(promise) => return Ok(NativeReturn::Value(promise)),
    };
    let iterable = rt.arg(call, 0);
    let mut record = match rt.get_iterator(iterable) {
        Ok(record) => record,
        Err(error) => {
            let promise = rt.reject_if_abrupt(Err(error), &capability)?;
            return Ok(NativeReturn::Value(promise.unwrap_or_else(|p| p)));
        }
    };
    let state = State::new(rt, &capability, which)?;
    let target = Target {
        constructor,
        promise_resolve,
        realm,
    };
    let result = perform(rt, which, &mut record, target, state);
    let result = match result {
        Err(error) if !record.done => Err(rt.iterator_close_after_error(&record, error)),
        other => other,
    };
    let promise = rt.reject_if_abrupt(result, &capability)?;
    Ok(NativeReturn::Value(promise.unwrap_or_else(|p| p)))
}

/// The shared records of one call: an internal array (no prototype, never
/// reachable from script code) with the list of values or errors (another
/// internal array), the capability's three values and
/// `remainingElementsCount`.
#[derive(Clone, Copy)]
struct State(Gc<Object>);

impl State {
    /// The state of a call, recorded in the current handle scope.
    fn new(rt: &mut Runtime, capability: &Capability, which: Which) -> VmResult<State> {
        let list = if which == Which::Race {
            Value::Undefined
        } else {
            let list = rt.heap.new_array(None, 0)?;
            rt.heap.record(list).into()
        };
        let [promise, resolve, reject] = capability.values();
        let values = [list, promise, resolve, reject, Value::Int(1)];
        let state = rt.heap.new_array_from(None, &values, &rt.vm)?;
        Ok(State(rt.heap.record(state)))
    }

    fn get(self, rt: &Runtime, index: u32) -> VmResult<Value> {
        rt.heap
            .dense_elements(self.0)?
            .and_then(|values| values.get(index as usize).copied())
            .ok_or(VmError::invariant("a combinator state without its records"))
    }

    fn set(self, rt: &mut Runtime, index: u32, value: Value) -> VmResult<()> {
        if rt.heap.store_dense(self.0, index, value)? {
            Ok(())
        } else {
            Err(VmError::invariant("a combinator state that is not dense"))
        }
    }

    /// The list of values or errors.
    fn list(self, rt: &Runtime) -> VmResult<Gc<Object>> {
        match self.get(rt, LIST)? {
            Value::Object(list) => Ok(list),
            _ => Err(VmError::invariant("a combinator state without its list")),
        }
    }

    fn capability(self, rt: &Runtime) -> VmResult<Capability> {
        Ok(Capability::Record(Box::new(CapabilityRecord {
            promise: self.get(rt, PROMISE)?,
            resolve: self.get(rt, RESOLVE)?,
            reject: self.get(rt, REJECT)?,
        })))
    }

    /// `remainingElementsCount.[[Value]] += delta`; the new value.
    fn add_remaining(self, rt: &mut Runtime, delta: i32) -> VmResult<f64> {
        let count = self.get(rt, REMAINING)?.as_number().unwrap_or(0.0) + f64::from(delta);
        self.set(rt, REMAINING, Value::number(count))?;
        Ok(count)
    }

    /// Appends `undefined` to the list (step 4.c of `PerformPromiseAll`).
    fn append(self, rt: &mut Runtime) -> VmResult<()> {
        let list = self.list(rt)?;
        if rt.heap.push_element(list, Value::Undefined)? {
            Ok(())
        } else {
            Err(VmError::range_error(crate::object::INVALID_ARRAY_LENGTH))
        }
    }

    /// `CreateArrayFromList(list)` in the current realm.
    fn result_array(self, rt: &mut Runtime) -> VmResult<Gc<Object>> {
        let list = self.list(rt)?;
        let values = rt.heap.dense_elements(list)?.map(<[Value]>::to_vec);
        let values = values.ok_or(VmError::invariant("a combinator list that is not dense"))?;
        rt.charge(values.len())?;
        rt.array_from_values(&values)
    }
}

/// The constructor of a call, its `resolve` and the combinator's realm.
#[derive(Clone, Copy)]
struct Target {
    constructor: Gc<Object>,
    promise_resolve: Value,
    realm: u32,
}

/// `PerformPromiseAll`, `PerformPromiseAllSettled`, `PerformPromiseAny`
/// and `PerformPromiseRace` (§27.2.4.1.2, §27.2.4.2.1, §27.2.4.3.1,
/// §27.2.4.5.1): the capability's promise, or the error that rejects it.
fn perform(
    rt: &mut Runtime,
    which: Which,
    record: &mut IteratorRecord,
    target: Target,
    state: State,
) -> VmResult<Value> {
    let Target {
        constructor,
        promise_resolve,
        realm,
    } = target;
    let mut index: u32 = 0;
    loop {
        // A scope that also closes after an error: the caller goes on to
        // close the iterator and reject the capability.
        let more = rt.scoped(|rt| {
            let Some(next) = rt.iterator_step_value(record)? else {
                return Ok(false);
            };
            if which != Which::Race {
                state.append(rt)?;
            }
            let next_promise = rt.call(promise_resolve, constructor.into(), &[next])?;
            let args = element_handlers(rt, which, state, index, realm)?;
            if which != Which::Race {
                state.add_remaining(rt, 1)?;
            }
            invoke(rt, next_promise, args)?;
            index += 1;
            Ok(true)
        })?;
        if !more {
            break;
        }
    }
    // The iterator is done.
    let capability = state.capability(rt)?;
    if which == Which::Race {
        return Ok(capability.promise());
    }
    if state.add_remaining(rt, -1)? == 0.0 {
        if which == Which::Any {
            let error = aggregate_error(rt, state)?;
            return Err(VmError::Throw(error.into()));
        }
        let array = state.result_array(rt)?;
        rt.capability_resolve(&capability, array.into())?;
    }
    Ok(capability.promise())
}

/// The two arguments of `then` for element `index`.
fn element_handlers(
    rt: &mut Runtime,
    which: Which,
    state: State,
    index: u32,
    realm: u32,
) -> VmResult<[Value; 2]> {
    let slots = |extra: Option<Value>| {
        let mut slots = vec![Value::Object(state.0), Value::number(f64::from(index))];
        slots.extend(extra);
        slots.into_boxed_slice()
    };
    let element = |rt: &mut Runtime, func: NativeFn, extra| -> VmResult<Value> {
        Ok(rt
            .new_builtin_with_slots(realm, 1, func, slots(extra))?
            .into())
    };
    Ok(match which {
        Which::All => [
            element(rt, all_resolve_element, None)?,
            state.get(rt, REJECT)?,
        ],
        Which::AllSettled => {
            let cell = rt.heap.alloc_cell(Value::Bool(false))?;
            rt.heap.record(cell);
            [
                element(rt, settled_resolve_element, Some(Value::Cell(cell)))?,
                element(rt, settled_reject_element, Some(Value::Cell(cell)))?,
            ]
        }
        Which::Any => [
            state.get(rt, RESOLVE)?,
            element(rt, any_reject_element, None)?,
        ],
        Which::Race => [state.get(rt, RESOLVE)?, state.get(rt, REJECT)?],
    })
}

/// `Invoke(nextPromise, "then", args)` through re-entry.
fn invoke(rt: &mut Runtime, target: Value, args: [Value; 2]) -> VmResult<()> {
    match invoke_then(rt, target, args.to_vec())? {
        NativeReturn::Call { callee, this, args } => {
            rt.call(callee, this, &args)?;
            Ok(())
        }
        _ => Err(VmError::invariant("Invoke without a call")),
    }
}

/// The state and the index of an element function, after the check and
/// update of `[[AlreadyCalled]]`; `None` when it was called already. With
/// a cell (`allSettled`), the cell is the shared record.
fn claim_element(rt: &mut Runtime, function: Gc<Object>) -> VmResult<Option<(State, u32)>> {
    let Value::Object(state) = rt.native_slot(function, 0)? else {
        return Err(VmError::invariant("an element function without its state"));
    };
    let index = match &rt.heap.object(function)?.kind {
        ObjectKind::Native(native) if native.slots.len() == 3 => {
            let cell = cell_of(rt.native_slot(function, 2)?)?;
            let record = rt.heap.cell_mut(cell)?;
            if record.value == Value::Bool(true) {
                return Ok(None);
            }
            record.value = Value::Bool(true);
            rt.native_slot(function, 1)?
        }
        _ => {
            let index = rt.native_slot(function, 1)?;
            if index.is_undefined() {
                return Ok(None);
            }
            rt.set_native_slot(function, 1, Value::Undefined)?;
            index
        }
    };
    let Some(index) = index.as_number() else {
        return Err(VmError::invariant("an element function without its index"));
    };
    Ok(Some((State(state), index as u32)))
}

/// Stores `value` at `index` of the list and decrements the count; true
/// when it reached zero.
fn store_element(rt: &mut Runtime, state: State, index: u32, value: Value) -> VmResult<bool> {
    let list = state.list(rt)?;
    if !rt.heap.store_dense(list, index, value)? {
        return Err(VmError::invariant("a combinator list that is not dense"));
    }
    Ok(state.add_remaining(rt, -1)? == 0.0)
}

/// `Promise.all` Resolve Element Functions (§27.2.4.1.3).
fn all_resolve_element(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Some((state, index)) = claim_element(rt, call.callee())? else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    if store_element(rt, state, index, rt.arg(call, 0))? {
        let array = state.result_array(rt)?;
        let capability = state.capability(rt)?;
        rt.capability_resolve(&capability, array.into())?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `Promise.allSettled` Resolve and Reject Element Functions
/// (§27.2.4.2.2, §27.2.4.2.3).
fn settled_element(rt: &mut Runtime, call: &NativeCall, fulfilled: bool) -> VmResult<NativeReturn> {
    let Some((state, index)) = claim_element(rt, call.callee())? else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    let proto = rt.intrinsic(rt.current_realm(), Intrinsic::ObjectPrototype)?;
    let object = rt.heap.new_object(Some(proto))?;
    rt.heap.record(object);
    let (status, name) = if fulfilled {
        ("fulfilled", "value")
    } else {
        ("rejected", "reason")
    };
    let status = rt.heap.intern_str(status)?;
    rt.heap.record(status);
    let key = rt.heap.key_from_str("status")?;
    rt.create_data_property_or_throw(object, key, status.into())?;
    let key = rt.heap.key_from_str(name)?;
    rt.create_data_property_or_throw(object, key, rt.arg(call, 0))?;
    if store_element(rt, state, index, object.into())? {
        let array = state.result_array(rt)?;
        let capability = state.capability(rt)?;
        rt.capability_resolve(&capability, array.into())?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

fn settled_resolve_element(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    settled_element(rt, call, true)
}

fn settled_reject_element(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    settled_element(rt, call, false)
}

/// `Promise.any` Reject Element Functions (§27.2.4.3.2).
fn any_reject_element(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Some((state, index)) = claim_element(rt, call.callee())? else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    if store_element(rt, state, index, rt.arg(call, 0))? {
        let error = aggregate_error(rt, state)?;
        let capability = state.capability(rt)?;
        rt.capability_reject(&capability, error.into())?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The `AggregateError` of `Promise.any` with the errors of the list: V8's
/// message "All promises were rejected" and its `stack`, then `errors`
/// (writable, not enumerable, configurable). Recorded in the current
/// handle scope.
fn aggregate_error(rt: &mut Runtime, state: State) -> VmResult<Gc<Object>> {
    let errors = state.result_array(rt)?;
    let proto = rt.intrinsic(rt.current_realm(), Intrinsic::AggregateErrorPrototype)?;
    let error = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::Error(None))?;
    rt.heap.record(error);
    rt.install_stack(error, None)?;
    let message = rt.heap.alloc_str("All promises were rejected")?;
    rt.heap.record(message);
    let key = PropertyKey::String(rt.vm.atoms.message);
    rt.define(error, key, message.into(), true, false, true)?;
    let key = rt.heap.key_from_str("errors")?;
    let desc = PropertyDescriptor::data(errors.into(), true, false, true);
    rt.define_own_property(error, key, desc)?;
    Ok(error)
}
