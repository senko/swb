//! Iteration objects (ECMA-262 §27.1.2, §23.1.5): `%IteratorPrototype%`
//! with `@@iterator` (the `Iterator` constructor and the helper methods
//! are feature 6), `%ArrayIteratorPrototype%` with `next` and
//! `@@toStringTag`, and `Array.prototype.keys`, `values` and `entries`
//! (`array.rs` installs them, with `@@iterator`).
//!
//! An Array Iterator works on any object with a `length` (arrays and
//! array-likes). It reads `length` again at each step, so growth and
//! shrinkage show; it never allocates in proportion to the length. Once
//! it has finished, or when reading `length` or an element threw, it stays
//! finished (the specification's generator closure has completed).

use crate::heap::Gc;
use crate::object::{ArrayIterator, IterationKind, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let iterator_proto = rt.intrinsic(realm, Intrinsic::IteratorPrototype)?;
    super::symbol_method(
        rt,
        realm,
        iterator_proto,
        WellKnown::Iterator,
        0,
        iterator_iterator,
        (true, true),
    )?;
    let array_iterator_proto = rt.intrinsic(realm, Intrinsic::ArrayIteratorPrototype)?;
    let next = super::method(rt, realm, array_iterator_proto, "next", 0, next)?;
    rt.set_intrinsic(realm, Intrinsic::ArrayIteratorNext, next);
    super::to_string_tag(rt, array_iterator_proto, "Array Iterator")
}

/// `%IteratorPrototype%[@@iterator]()` (§27.1.2.1): returns `this`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn iterator_iterator(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// `Array.prototype.entries()` (§23.1.3.5).
pub(super) fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Entries)
}

/// `Array.prototype.keys()` (§23.1.3.19).
pub(super) fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Keys)
}

/// `Array.prototype.values()` (§23.1.3.40); also `@@iterator`.
pub(super) fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Values)
}

/// `CreateArrayIterator` (§23.1.5.1) for `ToObject(this)`.
fn create_array_iterator(
    rt: &mut Runtime,
    call: &NativeCall,
    kind: IterationKind,
) -> VmResult<NativeReturn> {
    let target = rt.this_object(call)?;
    let proto = rt.intrinsic(call.realm(), Intrinsic::ArrayIteratorPrototype)?;
    let state = ArrayIterator {
        target: Some(target),
        next_index: 0.0,
        kind,
    };
    let iterator = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::ArrayIterator(Box::new(state)))?;
    Ok(NativeReturn::Value(iterator.into()))
}

/// `%ArrayIteratorPrototype%.next()` (§23.1.5.2.1).
fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(iterator) = call.this() else {
        return Err(incompatible(rt, call.this()));
    };
    if !matches!(rt.heap.object(iterator)?.kind, ObjectKind::ArrayIterator(_)) {
        return Err(incompatible(rt, call.this()));
    }
    let value = array_iterator_next_value(rt, iterator)?;
    let result = rt.iter_result(value.unwrap_or(Value::Undefined), value.is_none())?;
    Ok(NativeReturn::Value(result.into()))
}

/// The value of the next step of the Array Iterator `iterator`, or `None`
/// when it is finished: `next` without its result object. The compiled
/// iteration steps call it for an Array Iterator whose `next` is the
/// built-in one, which is not observable. The value is recorded in the
/// current handle scope.
pub(crate) fn array_iterator_next_value(
    rt: &mut Runtime,
    iterator: Gc<Object>,
) -> VmResult<Option<Value>> {
    let ObjectKind::ArrayIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(VmError::invariant(
            "a step of an Array Iterator on another object",
        ));
    };
    let (Some(target), index, kind) = (state.target, state.next_index, state.kind) else {
        return Ok(None);
    };
    let step = array_iterator_step(rt, iterator, target, index, kind);
    if !matches!(step, Ok(Some(_))) {
        // Done, or `length` or an element threw: the iterator is finished.
        set_target(rt, iterator, None)?;
    }
    step
}

/// One step of the closure of `CreateArrayIterator`: the value, or `None`
/// at the end of the array-like.
fn array_iterator_step(
    rt: &mut Runtime,
    iterator: Gc<Object>,
    target: Gc<Object>,
    index: f64,
    kind: IterationKind,
) -> VmResult<Option<Value>> {
    let length = rt.length_of_array_like(target)?;
    if index >= length {
        return Ok(None);
    }
    if let ObjectKind::ArrayIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.next_index = index + 1.0;
    }
    let number = Value::number(index);
    let result = match kind {
        IterationKind::Keys => number,
        IterationKind::Values | IterationKind::Entries => {
            let key = rt.index_key(index)?;
            let element = rt.get_value(target.into(), key)?;
            if kind == IterationKind::Values {
                element
            } else {
                rt.pair_array(number, element)?.into()
            }
        }
    };
    Ok(Some(result))
}

fn set_target(rt: &mut Runtime, iterator: Gc<Object>, target: Option<Gc<Object>>) -> VmResult<()> {
    if let ObjectKind::ArrayIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.target = target;
    }
    Ok(())
}

/// V8's `TypeError` for `next` on another receiver.
fn incompatible(rt: &Runtime, receiver: Value) -> VmError {
    VmError::type_error(format!(
        "Method Array Iterator.prototype.next called on incompatible receiver {}",
        match receiver {
            Value::Object(object) => rt.object_text(object),
            _ => rt.primitive_text(receiver),
        }
    ))
}
