//! `Object.fromEntries` (ECMA-262 §20.1.2.7) and `Object.groupBy`
//! (§20.1.2.12 with `GroupBy`, §7.3.35, for property keys): the two
//! functions of `Object` that consume an iterable.
//!
//! Both read the iterable with the iterator operations of `vm/iter.rs`.
//! A step runs in its own handle scope, so the loop does not grow its
//! scope, and ticks the time countdown, so an iterator that never ends
//! stops at the time limit. The result is built while the loop runs
//! (nothing observes it before the function returns): `groupBy` keeps
//! each group as an array in the result object, so the groups are
//! reachable for the collector and need no Rust-side vectors whose size a
//! script controls. An error after a value was taken closes the iterator
//! (§7.4.11).

use crate::heap::Gc;
use crate::object::{Object, Property};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::MAX_SAFE_INTEGER;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// `Object.fromEntries(iterable)` (§20.1.2.7).
pub(super) fn from_entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let iterable = rt.arg(call, 0);
    if iterable.is_nullish() {
        return Err(VmError::type_error("undefined is not iterable"));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
    let object = rt.heap.new_object(Some(proto))?;
    rt.heap.record(object);
    let mut record = rt.get_iterator(iterable)?;
    loop {
        let more = rt.with_scope(|rt| {
            let Some(item) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            match add_entry(rt, object, item) {
                Ok(()) => Ok(true),
                Err(error) => Err(rt.iterator_close_after_error(&record, error)),
            }
        })?;
        if !more {
            return Ok(NativeReturn::Value(object.into()));
        }
    }
}

/// One step of `AddEntriesFromIterable` (§24.1.1.2) with the adder of
/// `fromEntries`: `CreateDataPropertyOrThrow(object, key, value)`.
fn add_entry(rt: &mut Runtime, object: Gc<Object>, item: Value) -> VmResult<()> {
    if !matches!(item, Value::Object(_)) {
        return Err(VmError::type_error(format!(
            "Iterator value {} is not an entry object",
            rt.describe_value(item)
        )));
    }
    let key = rt.get_value(item, PropertyKey::Index(0))?;
    let value = rt.get_value(item, PropertyKey::Index(1))?;
    let key = rt.key_arg(key)?;
    if !rt.heap.create_data_property(object, key, value, &rt.vm)? {
        return Err(rt.define_failure(object, key));
    }
    Ok(())
}

/// `Object.groupBy(items, callback)` (§20.1.2.12).
pub(super) fn group_by(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let items = rt.arg(call, 0);
    if items.is_nullish() {
        return Err(VmError::type_error(
            "Object.groupBy called on null or undefined",
        ));
    }
    let callback = rt.arg(call, 1);
    if !rt.is_callable(callback)? {
        return Err(VmError::type_error(format!(
            "{} is not a function",
            rt.describe_value(callback)
        )));
    }
    let result = rt.heap.new_object(None)?;
    rt.heap.record(result);
    let mut record = rt.get_iterator(items)?;
    let mut k = 0.0;
    loop {
        let more = rt.with_scope(|rt| {
            if k >= MAX_SAFE_INTEGER {
                let error = VmError::type_error("Invalid array length");
                return Err(rt.iterator_close_after_error(&record, error));
            }
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            match add_to_group(rt, result, callback, value, k) {
                Ok(()) => Ok(true),
                Err(error) => Err(rt.iterator_close_after_error(&record, error)),
            }
        })?;
        if !more {
            return Ok(NativeReturn::Value(result.into()));
        }
        k += 1.0;
    }
}

/// One step of `GroupBy`: the key of `value`, then `AddValueToKeyedGroup`.
fn add_to_group(
    rt: &mut Runtime,
    result: Gc<Object>,
    callback: Value,
    value: Value,
    k: f64,
) -> VmResult<()> {
    let key = rt.call(callback, Value::Undefined, &[value, Value::number(k)])?;
    let key = rt.key_arg(key)?;
    let existing = rt.heap.get_own_property(result, key)?;
    let group = if let Some(Property::Data {
        value: Value::Object(group),
        ..
    }) = existing
    {
        group
    } else {
        let group = rt.new_array()?;
        rt.heap
            .create_data_property(result, key, group.into(), &rt.vm)?;
        group
    };
    let (length, _) = rt.heap.array_length(group)?;
    if length == u32::MAX {
        return Err(VmError::range_error(crate::object::INVALID_ARRAY_LENGTH));
    }
    rt.heap
        .create_data_property(group, PropertyKey::Index(length), value, &rt.vm)?;
    Ok(())
}
