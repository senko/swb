//! The ES2025 set methods (ECMA-262 §24.2.4,
//! <https://tc39.es/ecma262/#sec-properties-of-the-set-prototype-object>):
//! `union`, `intersection`, `difference`, `symmetricDifference`,
//! `isSubsetOf`, `isSupersetOf` and `isDisjointFrom`, with `GetSetRecord`
//! (§24.2.1.2).
//!
//! The argument is any object with `size`, `has` and `keys`, read in this
//! order. The methods follow the specification's branches: where the
//! receiver has at most `size` elements they call `has` of the argument
//! for each element of the receiver (`intersection` iterates the smaller
//! one), otherwise they iterate `keys()` of the argument. The loop over
//! the receiver is a native loop of the table: it sees the elements that a
//! `has` call adds and skips those it deletes, as the specification's
//! `thisSize` loops do, also across a compaction. `difference` and
//! `symmetricDifference` change a copy (the receiver's data at the time of
//! the copy). Each element or key that a loop looks at is a step of the
//! time countdown; the copy charges its length. The result is a plain
//! `Set` with `%Set.prototype%` (no species).
//!
//! The messages are V8's (measured in Chromium 148,
//! `tools/probes/js-collections.json`). Unlike the specification,
//! `GetIteratorFromMethod` of `keys` checks that `next` is callable at once
//! (V8 does; the error is the same `TypeError` the first step would
//! throw).

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::iter::IteratorRecord;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

use super::collection::Which;

/// A Set Record (§24.2.1.1).
struct SetRecord {
    object: Value,
    size: f64,
    has: Value,
    keys: Value,
}

/// `GetSetRecord ( obj )` (§24.2.1.2); `method` is the text of the
/// method for the message of an argument that is not an object.
fn get_set_record(rt: &mut Runtime, other: Value, method: &str) -> VmResult<SetRecord> {
    if !matches!(other, Value::Object(_)) {
        return Err(VmError::type_error(format!(
            "{method} argument must be an object"
        )));
    }
    let key = rt.heap.key_from_str("size")?;
    let raw = rt.get_value(other, key)?;
    let number = rt.to_number(raw)?;
    if number.is_nan() {
        return Err(VmError::type_error("The .size property is NaN"));
    }
    let size = to_integer_or_infinity(number);
    if size < 0.0 {
        return Err(VmError::range_error(format!(
            "'{}' is an invalid size",
            crate::vm::number::number_to_string(number)
        )));
    }
    let has = callable_property(rt, other, "has")?;
    let keys = callable_property(rt, other, "keys")?;
    Ok(SetRecord {
        object: other,
        size,
        has,
        keys,
    })
}

/// `Get(object, name)`, which must be callable (V8's message names the
/// property as a string value).
fn callable_property(rt: &mut Runtime, object: Value, name: &str) -> VmResult<Value> {
    let key = rt.heap.key_from_str(name)?;
    let value = rt.get_value(object, key)?;
    if !rt.is_callable(value)? {
        return Err(VmError::type_error(format!(
            "string \"{name}\" is not a function"
        )));
    }
    Ok(value)
}

/// `GetIteratorFromMethod(otherRecord.[[SetObject]],
/// otherRecord.[[Keys]])` (§7.4.2).
fn open_keys(rt: &mut Runtime, record: &SetRecord) -> VmResult<IteratorRecord> {
    let iterator = rt.call(record.keys, record.object, &[])?;
    rt.heap.record(iterator);
    if !matches!(iterator, Value::Object(_)) {
        return Err(VmError::type_error(
            "Result of the keys method is not an object",
        ));
    }
    let next_key = crate::string::PropertyKey::String(rt.vm.atoms.next);
    let next_method = rt.get_value(iterator, next_key)?;
    if !rt.is_callable(next_method)? {
        return Err(VmError::type_error("string \"next\" is not a function"));
    }
    Ok(IteratorRecord {
        iterator,
        next_method,
        done: false,
    })
}

/// What a step of a loop decides.
enum Flow {
    /// Go on with the next element.
    Continue,
    /// End the loop and close the keys iterator (the answer is known).
    Stop,
}

/// Steps the keys iterator until it is done or `f` stops. `f` gets each
/// key canonicalized. Returns whether the iterator ran to its end; a
/// `Stop` closes the iterator (`IteratorClose` with a normal completion).
fn each_key(
    rt: &mut Runtime,
    record: &mut IteratorRecord,
    mut f: impl FnMut(&mut Runtime, Value) -> VmResult<Flow>,
) -> VmResult<bool> {
    loop {
        let flow = rt.with_scope(|rt| {
            let Some(next) = rt.iterator_step_value(record)? else {
                return Ok(None);
            };
            let next = rt.collection_key(next)?;
            f(rt, next).map(Some)
        })?;
        match flow {
            None => return Ok(true),
            Some(Flow::Continue) => {}
            Some(Flow::Stop) => {
                close_keys(rt, record)?;
                return Ok(false);
            }
        }
    }
}

/// `IteratorClose(keysIterator, NormalCompletion(unused))` (§7.4.11) with
/// V8's message for a `return` that gives a non-object.
fn close_keys(rt: &mut Runtime, record: &IteratorRecord) -> VmResult<()> {
    let key = crate::string::PropertyKey::String(rt.vm.atoms.return_);
    let Some(method) = rt.get_method(record.iterator, key)? else {
        return Ok(());
    };
    let result = rt.call(method, record.iterator, &[])?;
    if !matches!(result, Value::Object(_)) {
        return Err(VmError::type_error("return called on non-object"));
    }
    Ok(())
}

/// Visits the live entries of `source`'s table in order, including those
/// added during the loop, until `f` returns false. Returns whether the
/// loop ran to the end.
fn each_element(
    rt: &mut Runtime,
    source: Gc<Object>,
    mut f: impl FnMut(&mut Runtime, Value) -> VmResult<bool>,
) -> VmResult<bool> {
    let id = rt.heap.collection_loop_begin(source)?;
    let result = (|| loop {
        let step = rt.with_scope(|rt| {
            let (entry, work) = rt.heap.collection_loop_next(source, id)?;
            rt.charge(work)?;
            match entry {
                None => Ok(None),
                Some(entry) => f(rt, entry.key).map(Some),
            }
        })?;
        match step {
            None => return Ok(true),
            Some(true) => {}
            Some(false) => return Ok(false),
        }
    })();
    rt.heap.collection_loop_end(source, id);
    result
}

/// `ToBoolean(? Call(otherRecord.[[Has]], otherRecord.[[SetObject]], «
/// value »))`.
fn other_has(rt: &mut Runtime, record: &SetRecord, value: Value) -> VmResult<bool> {
    let result = rt.call(record.has, record.object, &[value])?;
    rt.to_boolean(result)
}

/// The receiver, the Set Record of the argument and the number of
/// elements of the receiver.
fn start(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
) -> VmResult<(Gc<Object>, SetRecord, bool)> {
    let method = format!("Set.prototype.{name}");
    let this = rt.this_collection(call, Which::Set, &method)?;
    let other = rt.arg(call, 0);
    let record = get_set_record(rt, other, &method)?;
    let small = (rt.collection_size(this)? as f64) <= record.size;
    Ok((this, record, small))
}

/// A new empty `Set` with `%Set.prototype%`, recorded in the current
/// handle scope.
fn new_set(rt: &mut Runtime, call: &NativeCall) -> VmResult<Gc<Object>> {
    let proto = rt.intrinsic(call.realm(), Intrinsic::SetPrototype)?;
    let set = rt.heap.new_collection(Some(proto), false)?;
    Ok(rt.heap.record(set))
}

/// `a copy of set.[[SetData]]` into the new set `result`.
fn copy_into(rt: &mut Runtime, this: Gc<Object>, result: Gc<Object>) -> VmResult<()> {
    rt.charge(rt.collection_size(this)?)?;
    rt.heap.collection_copy(this, result)?;
    Ok(())
}

fn has_key(rt: &Runtime, set: Gc<Object>, key: Value) -> VmResult<bool> {
    Ok(rt.heap.collection_find(set, key)?.is_some())
}

/// `Set.prototype.union(other)` (§24.2.4.16).
pub(super) fn union(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, _) = start(rt, call, "union")?;
    let mut keys = open_keys(rt, &record)?;
    let result = new_set(rt, call)?;
    copy_into(rt, this, result)?;
    each_key(rt, &mut keys, |rt, next| {
        if !has_key(rt, result, next)? {
            rt.heap.collection_append(result, next, Value::Undefined)?;
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Set.prototype.intersection(other)` (§24.2.4.9).
pub(super) fn intersection(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, small) = start(rt, call, "intersection")?;
    let result = new_set(rt, call)?;
    if small {
        each_element(rt, this, |rt, entry| {
            if other_has(rt, &record, entry)? && !has_key(rt, result, entry)? {
                rt.heap.collection_append(result, entry, Value::Undefined)?;
            }
            Ok(true)
        })?;
    } else {
        let mut keys = open_keys(rt, &record)?;
        each_key(rt, &mut keys, |rt, next| {
            if has_key(rt, this, next)? && !has_key(rt, result, next)? {
                rt.heap.collection_append(result, next, Value::Undefined)?;
            }
            Ok(Flow::Continue)
        })?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Set.prototype.difference(other)` (§24.2.4.5).
pub(super) fn difference(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, small) = start(rt, call, "difference")?;
    let result = new_set(rt, call)?;
    copy_into(rt, this, result)?;
    if small {
        // The loop runs over the copy; `has` can change the receiver, not
        // the copy.
        each_element(rt, result, |rt, entry| {
            if other_has(rt, &record, entry)? {
                rt.heap.collection_remove(result, entry)?;
            }
            Ok(true)
        })?;
    } else {
        let mut keys = open_keys(rt, &record)?;
        each_key(rt, &mut keys, |rt, next| {
            rt.heap.collection_remove(result, next)?;
            Ok(Flow::Continue)
        })?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Set.prototype.symmetricDifference(other)` (§24.2.4.15).
pub(super) fn symmetric_difference(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, _) = start(rt, call, "symmetricDifference")?;
    let mut keys = open_keys(rt, &record)?;
    let result = new_set(rt, call)?;
    copy_into(rt, this, result)?;
    each_key(rt, &mut keys, |rt, next| {
        let in_result = has_key(rt, result, next)?;
        if has_key(rt, this, next)? {
            if in_result {
                rt.heap.collection_remove(result, next)?;
            }
        } else if !in_result {
            rt.heap.collection_append(result, next, Value::Undefined)?;
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Set.prototype.isSubsetOf(other)` (§24.2.4.11).
pub(super) fn is_subset_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, small) = start(rt, call, "isSubsetOf")?;
    if !small {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    }
    let all = each_element(rt, this, |rt, entry| other_has(rt, &record, entry))?;
    Ok(NativeReturn::Value(Value::Bool(all)))
}

/// `Set.prototype.isSupersetOf(other)` (§24.2.4.12).
pub(super) fn is_superset_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, _) = start(rt, call, "isSupersetOf")?;
    if (rt.collection_size(this)? as f64) < record.size {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    }
    let mut keys = open_keys(rt, &record)?;
    let all = each_key(rt, &mut keys, |rt, next| {
        Ok(if has_key(rt, this, next)? {
            Flow::Continue
        } else {
            Flow::Stop
        })
    })?;
    Ok(NativeReturn::Value(Value::Bool(all)))
}

/// `Set.prototype.isDisjointFrom(other)` (§24.2.4.10).
pub(super) fn is_disjoint_from(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (this, record, small) = start(rt, call, "isDisjointFrom")?;
    let disjoint = if small {
        each_element(rt, this, |rt, entry| Ok(!other_has(rt, &record, entry)?))?
    } else {
        let mut keys = open_keys(rt, &record)?;
        each_key(rt, &mut keys, |rt, next| {
            Ok(if has_key(rt, this, next)? {
                Flow::Stop
            } else {
                Flow::Continue
            })
        })?
    };
    Ok(NativeReturn::Value(Value::Bool(disjoint)))
}
