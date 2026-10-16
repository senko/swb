//! The `Array.prototype` methods that call a function for the elements
//! (ECMA-262 §23.1.3, <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! `forEach`, `map`, `filter`, `every`, `some`, `reduce`, `reduceRight`
//! and the `find` family.
//!
//! All but the `find` family skip the indices that `HasProperty` does not
//! find, so on sparse elements they visit only the stored indices
//! (`array_access.rs`). The `find` family calls its predicate for every
//! index, holes included (§23.1.3.12.1 `FindViaPredicate`), so its cost
//! is the length. A value that a loop keeps from one step to the next
//! (the accumulator of `reduce`) lives in a cell recorded in the native's
//! handle scope, since each step has a scope of its own.

use crate::heap::Gc;
use crate::runtime::Runtime;
use crate::value::{Value, ValueCell};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array::{callback_arg, receiver};
use super::array_access::{Flow, Item};

/// `Array.prototype.forEach(callback, thisArg)` (§23.1.3.15).
pub(super) fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some("forEach"))?;
    let callback = callback_arg(rt, call, 0)?;
    let this_arg = rt.arg(call, 1);
    rt.for_present(object, 0.0, length, |rt, k, value| {
        rt.call(
            callback,
            this_arg,
            &[value, Value::number(k), object.into()],
        )?;
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `Array.prototype.map(callback, thisArg)` (§23.1.3.21).
pub(super) fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some("map"))?;
    let callback = callback_arg(rt, call, 0)?;
    let result = rt.array_species_create(object, length)?;
    let this_arg = rt.arg(call, 1);
    rt.for_present(object, 0.0, length, |rt, k, value| {
        let mapped = rt.call(
            callback,
            this_arg,
            &[value, Value::number(k), object.into()],
        )?;
        rt.create_index(result, k, mapped)?;
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.filter(callback, thisArg)` (§23.1.3.8).
pub(super) fn filter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some("filter"))?;
    let callback = callback_arg(rt, call, 0)?;
    let result = rt.array_species_create(object, 0.0)?;
    let this_arg = rt.arg(call, 1);
    let mut to = 0.0;
    rt.for_present(object, 0.0, length, |rt, k, value| {
        let selected = rt.call(
            callback,
            this_arg,
            &[value, Value::number(k), object.into()],
        )?;
        if rt.to_boolean(selected)? {
            if !rt.heap.push_element(result, value)? {
                rt.create_index(result, to, value)?;
            }
            to += 1.0;
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(result.into()))
}

/// `every` (§23.1.3.6) and `some` (§23.1.3.29): the first callback result
/// whose `ToBoolean` is `stop` ends the loop.
fn every_or_some(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    stop: bool,
) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some(name))?;
    let callback = callback_arg(rt, call, 0)?;
    let this_arg = rt.arg(call, 1);
    let mut stopped = false;
    rt.for_present(object, 0.0, length, |rt, k, value| {
        let result = rt.call(
            callback,
            this_arg,
            &[value, Value::number(k), object.into()],
        )?;
        if rt.to_boolean(result)? == stop {
            stopped = true;
            return Ok(Flow::Break);
        }
        Ok(Flow::Continue)
    })?;
    // `every` is true unless it stopped, `some` true only if it stopped.
    Ok(NativeReturn::Value(Value::Bool(stopped == stop)))
}

/// `Array.prototype.every(callback, thisArg)` (§23.1.3.6).
pub(super) fn every(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "every", false)
}

/// `Array.prototype.some(callback, thisArg)` (§23.1.3.29).
pub(super) fn some(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "some", true)
}

/// The `TypeError` of `reduce` without an initial value on no elements.
fn empty_reduce() -> VmError {
    VmError::type_error("Reduce of empty array with no initial value")
}

/// `reduce` (§23.1.3.24) and `reduceRight` (§23.1.3.25).
fn reduce_either(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    right: bool,
) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some(name))?;
    let callback = callback_arg(rt, call, 0)?;
    if length == 0.0 && call.argc() < 2 {
        return Err(empty_reduce());
    }
    let accumulator = rt.heap.alloc_cell(rt.arg(call, 1))?;
    rt.heap.record(accumulator);
    let mut started = call.argc() >= 2;
    let step = |rt: &mut Runtime, k: f64, value: Value| -> VmResult<Flow> {
        if !started {
            set_cell(rt, accumulator, value)?;
            started = true;
            return Ok(Flow::Continue);
        }
        let previous = rt.heap.cell(accumulator)?.value;
        let next = rt.call(
            callback,
            Value::Undefined,
            &[previous, value, Value::number(k), object.into()],
        )?;
        set_cell(rt, accumulator, next)?;
        Ok(Flow::Continue)
    };
    if right {
        rt.for_present_rev(object, length - 1.0, 0.0, step)?;
    } else {
        rt.for_present(object, 0.0, length, step)?;
    }
    if !started {
        return Err(empty_reduce());
    }
    Ok(NativeReturn::Value(rt.heap.cell(accumulator)?.value))
}

fn set_cell(rt: &mut Runtime, cell: Gc<ValueCell>, value: Value) -> VmResult<()> {
    rt.heap.cell_mut(cell)?.value = value;
    Ok(())
}

/// `Array.prototype.reduce(callback, initialValue)` (§23.1.3.24).
pub(super) fn reduce(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduce", false)
}

/// `Array.prototype.reduceRight(callback, initialValue)` (§23.1.3.25).
pub(super) fn reduce_right(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduceRight", true)
}

/// What the `find` family returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    Value,
    Index,
}

/// `FindViaPredicate` (§23.1.3.12.1) for `find`, `findIndex`, `findLast`
/// and `findLastIndex`.
fn find_via_predicate(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    descending: bool,
    found: Found,
) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some(name))?;
    let predicate = callback_arg(rt, call, 0)?;
    let this_arg = rt.arg(call, 1);
    let mut result = None;
    rt.for_index(object, 0.0, length, descending, |rt, item| {
        let mut test = |rt: &mut Runtime, k: f64, value: Value| -> VmResult<bool> {
            let outcome = rt.call(
                predicate,
                this_arg,
                &[value, Value::number(k), object.into()],
            )?;
            if rt.to_boolean(outcome)? {
                result = Some((k, value));
                return Ok(true);
            }
            Ok(false)
        };
        match item {
            Item::Value(k, value) => {
                if test(rt, k, value)? {
                    return Ok(Flow::Break);
                }
            }
            Item::Gap(from, to) => {
                let count = (to - from) as u64;
                for i in 0..count {
                    if i > 0 {
                        rt.tick()?;
                    }
                    let k = if descending {
                        to - 1.0 - i as f64
                    } else {
                        from + i as f64
                    };
                    if rt.with_scope(|rt| test(rt, k, Value::Undefined))? {
                        return Ok(Flow::Break);
                    }
                }
            }
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(match (result, found) {
        (Some((_, value)), Found::Value) => value,
        (Some((k, _)), Found::Index) => Value::number(k),
        (None, Found::Value) => Value::Undefined,
        (None, Found::Index) => Value::Int(-1),
    }))
}

/// `Array.prototype.find(predicate, thisArg)` (§23.1.3.9).
pub(super) fn find(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "find", false, Found::Value)
}

/// `Array.prototype.findIndex(predicate, thisArg)` (§23.1.3.10).
pub(super) fn find_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findIndex", false, Found::Index)
}

/// `Array.prototype.findLast(predicate, thisArg)` (§23.1.3.11).
pub(super) fn find_last(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLast", true, Found::Value)
}

/// `Array.prototype.findLastIndex(predicate, thisArg)` (§23.1.3.12).
pub(super) fn find_last_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLastIndex", true, Found::Index)
}
