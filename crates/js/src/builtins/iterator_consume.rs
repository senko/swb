//! The `Iterator.prototype` methods that consume an iterator (ECMA-262
//! §27.1.4, <https://tc39.es/ecma262/#sec-iteratorprototype-object>):
//! `reduce`, `toArray`, `forEach`, `some`, `every` and `find`.
//!
//! Each method checks that its receiver is an object, closes the iterator
//! with the `TypeError` when its callback is not callable (the
//! specification's `IfAbruptCloseIterator` on the argument check), reads
//! `next` once (`GetIteratorDirect`) and loops with a handle scope per
//! step. A callback that throws closes the iterator with that error;
//! `some`, `every` and `find` close it with a normal completion when they
//! stop. `toArray` builds the array with the dense builder, and copies the
//! rest of a built-in Array Iterator in one step.

use crate::object::INVALID_ARRAY_LENGTH;
use crate::runtime::Runtime;
use crate::value::{Value, ValueCell};
use crate::vm::iter::IteratorRecord;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::iterator_helper::{close, close_after_error, get_iterator_direct, receiver};

/// The receiver, a callable first argument and the iterator record.
fn start(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
) -> VmResult<(Value, Value, IteratorRecord)> {
    let this = receiver(call, name)?;
    let function = rt.arg(call, 0);
    if !rt.is_callable(function)? {
        let error = rt.not_callable(function);
        return Err(close_after_error(rt, this, error));
    }
    let record = get_iterator_direct(rt, this)?;
    Ok((this, function, record))
}

/// `Iterator.prototype.reduce(reducer [, initialValue])` (<https://tc39.es/ecma262/#sec-iterator.prototype.reduce>).
pub(super) fn reduce(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, reducer, mut record) = start(rt, call, "reduce")?;
    // The accumulator lives in a cell, since each step has a handle scope
    // of its own.
    let accumulator = rt.heap.alloc_cell(rt.arg(call, 1))?;
    rt.heap.record(accumulator);
    let mut counter = 0.0;
    if call.argc() < 2 {
        let first = rt.with_scope(|rt| {
            let step = rt.iterator_step_value(&mut record)?;
            if let Some(value) = step {
                set_cell(rt, accumulator, value)?;
            }
            Ok(step.is_some())
        })?;
        if !first {
            return Err(VmError::type_error(
                "Reduce of a done iterator with no initial value",
            ));
        }
        counter = 1.0;
    }
    loop {
        let more = rt.with_scope(|rt| {
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            let previous = rt.heap.cell(accumulator)?.value;
            let args = [previous, value, Value::number(counter)];
            match rt.call(reducer, Value::Undefined, &args) {
                Ok(result) => set_cell(rt, accumulator, result)?,
                Err(error) => return Err(rt.iterator_close_after_error(&record, error)),
            }
            Ok(true)
        })?;
        if !more {
            return Ok(NativeReturn::Value(rt.heap.cell(accumulator)?.value));
        }
        counter += 1.0;
    }
}

fn set_cell(rt: &mut Runtime, cell: crate::heap::Gc<ValueCell>, value: Value) -> VmResult<()> {
    rt.heap.cell_mut(cell)?.value = value;
    Ok(())
}

/// `Iterator.prototype.toArray()` (<https://tc39.es/ecma262/#sec-iterator.prototype.toArray>).
pub(super) fn to_array(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = receiver(call, "toArray")?;
    let mut record = get_iterator_direct(rt, this)?;
    let array = rt.new_array_with_capacity(0)?;
    rt.heap.record(array);
    // An Array Iterator over an array without holes: one copy.
    if let Some(iterator) = rt.builtin_array_step(record.iterator, record.next_method)
        && rt.spread_array_iterator(array, iterator)?
    {
        let (length, _) = rt.heap.array_length(array)?;
        rt.set_length(array, f64::from(length))?;
        return Ok(NativeReturn::Value(array.into()));
    }
    loop {
        let done = rt.with_scope(|rt| {
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(true);
            };
            if !rt.heap.push_element(array, value)? {
                return Err(VmError::range_error(INVALID_ARRAY_LENGTH));
            }
            Ok(false)
        })?;
        if done {
            return Ok(NativeReturn::Value(array.into()));
        }
    }
}

/// `Iterator.prototype.forEach(procedure)` (<https://tc39.es/ecma262/#sec-iterator.prototype.forEach>).
pub(super) fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, procedure, mut record) = start(rt, call, "forEach")?;
    let mut counter = 0.0;
    loop {
        let more = rt.with_scope(|rt| {
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            let args = [value, Value::number(counter)];
            match rt.call(procedure, Value::Undefined, &args) {
                Ok(_) => Ok(true),
                Err(error) => Err(rt.iterator_close_after_error(&record, error)),
            }
        })?;
        if !more {
            return Ok(NativeReturn::Value(Value::Undefined));
        }
        counter += 1.0;
    }
}

/// Which method `search` runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Search {
    /// `some`: stops at the first truthy result, returns true.
    Some,
    /// `every`: stops at the first falsy result, returns false.
    Every,
    /// `find`: stops at the first truthy result, returns the value.
    Find,
}

/// What a pass of `search` found.
enum Found {
    Exhausted,
    Continue,
    Stopped(Value),
}

/// `some`, `every` and `find` (see the three methods below): the
/// loop is the same; the first result that decides closes the iterator
/// with a normal completion.
fn search(rt: &mut Runtime, call: &NativeCall, name: &str, kind: Search) -> VmResult<NativeReturn> {
    let (_, predicate, mut record) = start(rt, call, name)?;
    let mut counter = 0.0;
    loop {
        let found = rt.with_scope(|rt| {
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(Found::Exhausted);
            };
            let args = [value, Value::number(counter)];
            let result = match rt.call(predicate, Value::Undefined, &args) {
                Ok(result) => result,
                Err(error) => return Err(rt.iterator_close_after_error(&record, error)),
            };
            let truthy = rt.to_boolean(result)?;
            if truthy == (kind != Search::Every) {
                close(rt, &record)?;
                return Ok(Found::Stopped(value));
            }
            Ok(Found::Continue)
        })?;
        match found {
            Found::Continue => counter += 1.0,
            Found::Exhausted => {
                return Ok(NativeReturn::Value(match kind {
                    Search::Some => Value::Bool(false),
                    Search::Every => Value::Bool(true),
                    Search::Find => Value::Undefined,
                }));
            }
            Found::Stopped(value) => {
                return Ok(NativeReturn::Value(match kind {
                    Search::Some => Value::Bool(true),
                    Search::Every => Value::Bool(false),
                    Search::Find => rt.heap.record(value),
                }));
            }
        }
    }
}

/// `Iterator.prototype.some(predicate)` (<https://tc39.es/ecma262/#sec-iterator.prototype.some>).
pub(super) fn some(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    search(rt, call, "some", Search::Some)
}

/// `Iterator.prototype.every(predicate)` (<https://tc39.es/ecma262/#sec-iterator.prototype.every>).
pub(super) fn every(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    search(rt, call, "every", Search::Every)
}

/// `Iterator.prototype.find(predicate)` (<https://tc39.es/ecma262/#sec-iterator.prototype.find>).
pub(super) fn find(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    search(rt, call, "find", Search::Find)
}
