//! `Iterator.from` and `Iterator.concat` (ECMA-262 §27.1.3.2,
//! <https://tc39.es/ecma262/#sec-iterator.from>; the iterator sequencing
//! proposal, <https://tc39.es/proposal-iterator-sequencing/>),
//! `%WrapForValidIteratorPrototype%` (§27.1.3.2.1.1) and
//! `GetIteratorFlattenable` (§7.4.5, also used by `flatMap`).
//!
//! `Iterator.concat` returns a helper object (`iterator_helper.rs`): the
//! open methods and iterables of its arguments are read when it is called
//! (a non-object argument or an argument without `@@iterator` is a
//! `TypeError` then); each iterable is opened when the previous one is
//! done, and the iterator of the current one is closed by `return`.

use crate::heap::Gc;
use crate::object::{HelperOp, IteratorHelper, ObjectKind, WrappedIterator};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::iter::IteratorRecord;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

use super::iterator_helper::{
    PassResult, get_iterator_direct, iterator_key, new_helper_object, scoped_pass, snapshot, update,
};

/// `GetMethod(value, key)` (§7.3.11) with V8's message for a property that
/// is neither nullish nor callable ("'5' returned for property 'return' of
/// object '#<Object>' is not a function").
pub(super) fn get_method_v8(
    rt: &mut Runtime,
    value: Value,
    key: PropertyKey,
) -> VmResult<Option<Value>> {
    let method = rt.get_value(value, key)?;
    if method.is_nullish() {
        return Ok(None);
    }
    if !rt.is_callable(method)? {
        return Err(rt.not_function_property(method, &rt.key_text(key), value));
    }
    Ok(Some(method))
}

/// `GetIteratorFlattenable(obj, primitiveHandling)` (§7.4.5). `name` is
/// the method for V8's "called on non-object"; `allow_strings` selects
/// `iterate-string-primitives` (`Iterator.from`) over `reject-primitives`
/// (`flatMap`).
pub(super) fn get_iterator_flattenable(
    rt: &mut Runtime,
    value: Value,
    name: &str,
    allow_strings: bool,
) -> VmResult<IteratorRecord> {
    let acceptable =
        matches!(value, Value::Object(_)) || (allow_strings && matches!(value, Value::String(_)));
    if !acceptable {
        return Err(VmError::type_error(format!("{name} called on non-object")));
    }
    let method = get_method_v8(rt, value, iterator_key(rt))?;
    let iterator = match method {
        None => value,
        Some(method) => rt.call(method, value, &[])?,
    };
    if !matches!(iterator, Value::Object(_)) {
        return Err(VmError::type_error(format!(
            "{} is not iterable",
            rt.value_text(value)
        )));
    }
    get_iterator_direct(rt, iterator)
}

/// `Iterator.from(O)` (§27.1.3.2.1).
pub(super) fn from(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    let record = get_iterator_flattenable(rt, value, "Iterator.from", true)?;
    let constructor = rt.intrinsic(call.realm(), Intrinsic::IteratorConstructor)?;
    if rt.ordinary_has_instance(constructor.into(), record.iterator)? {
        return Ok(NativeReturn::Value(record.iterator));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::WrapForValidIteratorPrototype)?;
    let wrapper = rt.heap.new_object_with_kind(
        Some(proto),
        ObjectKind::WrapForValidIterator(Box::new(WrappedIterator { record })),
    )?;
    Ok(NativeReturn::Value(wrapper.into()))
}

/// The record of a receiver made by `Iterator.from`.
fn this_wrapped(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<IteratorRecord> {
    if let Value::Object(object) = call.this()
        && let ObjectKind::WrapForValidIterator(state) = &rt.heap.object(object)?.kind
    {
        return Ok(state.record);
    }
    Err(rt.incompatible_receiver(
        &format!("%WrapForValidIteratorPrototype%.{method}"),
        call.this(),
    ))
}

/// `%WrapForValidIteratorPrototype%.next()` (§27.1.3.2.1.1.1): calls the
/// `next` method of the wrapped iterator (a deferred call).
pub(super) fn wrap_next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let record = this_wrapped(rt, call, "next")?;
    if !rt.is_callable(record.next_method)? {
        return Err(rt.not_callable(record.next_method));
    }
    Ok(NativeReturn::Call {
        callee: record.next_method,
        this: record.iterator,
        args: Vec::new(),
    })
}

/// `%WrapForValidIteratorPrototype%.return()` (§27.1.3.2.1.1.2).
pub(super) fn wrap_return(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let record = this_wrapped(rt, call, "return")?;
    let key = PropertyKey::String(rt.vm.atoms.return_);
    match get_method_v8(rt, record.iterator, key)? {
        None => {
            let result = rt.iter_result(Value::Undefined, true)?;
            Ok(NativeReturn::Value(result.into()))
        }
        Some(method) => Ok(NativeReturn::Call {
            callee: method,
            this: record.iterator,
            args: Vec::new(),
        }),
    }
}

/// `Iterator.concat(...items)` (iterator sequencing proposal).
pub(super) fn concat(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let count = call.argc();
    // The pairs are stored in the helper while they are read, so that
    // they stay alive without a handle scope that grows with the count.
    let bytes = count.saturating_mul(2 * size_of::<Value>());
    rt.heap.check_room(bytes)?;
    let unopened = IteratorRecord {
        iterator: Value::Undefined,
        next_method: Value::Undefined,
        done: false,
    };
    let mut helper = IteratorHelper::new(HelperOp::Concat, unopened, Value::Undefined, 0.0);
    helper.sources.reserve_exact(count * 2);
    let NativeReturn::Value(Value::Object(helper)) = new_helper_object(rt, call.realm(), helper)?
    else {
        return Err(VmError::invariant("a helper is not an object"));
    };
    rt.heap.record(helper);
    for index in 0..count {
        rt.tick()?;
        let item = rt.arg(call, index);
        if !matches!(item, Value::Object(_)) {
            return Err(VmError::type_error("Iterator.concat called on non-object"));
        }
        let Some(method) = get_method_v8(rt, item, iterator_key(rt))? else {
            return Err(VmError::type_error(format!(
                "{} is not iterable",
                rt.value_text(item)
            )));
        };
        update(rt, helper, |state| state.sources.extend([method, item]))?;
    }
    Ok(NativeReturn::Value(helper.into()))
}

/// The closure of `Iterator.concat` from the top of its outer loop to the
/// next `Yield` or to its end: reads the current iterator, and when that
/// is done opens the next iterable. A failure of `next` or of an open
/// method closes nothing.
pub(super) fn step_concat(
    rt: &mut Runtime,
    helper: Gc<crate::object::Object>,
) -> VmResult<Option<Value>> {
    loop {
        let pass = scoped_pass(rt, |rt| {
            let state = snapshot(rt, helper)?;
            if let Some(mut inner) = state.inner {
                if let Some(value) = rt.iterator_step_value(&mut inner)? {
                    return Ok(PassResult::Yield(value));
                }
                update(rt, helper, |state| state.inner = None)?;
                return Ok(PassResult::Again);
            }
            let Some((method, iterable)) = next_source(rt, helper)? else {
                return Ok(PassResult::Done);
            };
            let iterator = rt.call(method, iterable, &[])?;
            if !matches!(iterator, Value::Object(_)) {
                return Err(VmError::type_error(format!(
                    "{} is not iterable",
                    rt.value_text(iterator)
                )));
            }
            let record = get_iterator_direct(rt, iterator)?;
            update(rt, helper, |state| state.inner = Some(record))?;
            Ok(PassResult::Again)
        })?;
        match pass {
            PassResult::Again => {}
            PassResult::Done => return Ok(None),
            PassResult::Yield(value) => return Ok(Some(value)),
        }
    }
}

/// The open method and the iterable of the next argument of `concat`.
fn next_source(
    rt: &mut Runtime,
    helper: Gc<crate::object::Object>,
) -> VmResult<Option<(Value, Value)>> {
    let mut pair = None;
    update(rt, helper, |state| {
        let at = state.next_source * 2;
        if let (Some(&method), Some(&iterable)) = (state.sources.get(at), state.sources.get(at + 1))
        {
            state.next_source += 1;
            pair = Some((method, iterable));
        }
    })?;
    if let Some((method, iterable)) = pair {
        // Both stay alive in the helper; the scope keeps them while the
        // open method runs.
        rt.heap.record(method);
        rt.heap.record(iterable);
    }
    Ok(pair)
}
