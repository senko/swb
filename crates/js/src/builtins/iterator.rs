//! Iteration objects (ECMA-262 §27.1.2, §23.1.5): `%IteratorPrototype%`
//! with `@@iterator` (the `Iterator` constructor and the helper methods
//! are feature 6), `%ArrayIteratorPrototype%` with `next` and
//! `@@toStringTag`, and `Array.prototype.keys`, `values`, `entries` and
//! `@@iterator`.
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
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

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
    super::method(rt, realm, array_iterator_proto, "next", 0, next)?;
    super::to_string_tag(rt, array_iterator_proto, "Array Iterator")?;
    install_array_methods(rt, realm)
}

/// `Array.prototype.keys`, `values`, `entries` and `@@iterator` (the same
/// function object as `values`, §23.1.3.40).
fn install_array_methods(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ArrayPrototype)?;
    let methods: [(&str, NativeFn); 3] = [("entries", entries), ("keys", keys), ("values", values)];
    let mut values_function = None;
    for (name, func) in methods {
        let function = super::method(rt, realm, proto, name, 0, func)?;
        if name == "values" {
            values_function = Some(function);
        }
    }
    if let Some(function) = values_function {
        let key = rt.symbol_key(WellKnown::Iterator);
        rt.define(proto, key, function.into(), true, false, true)?;
        rt.set_intrinsic(realm, Intrinsic::ArrayPrototypeValues, function);
    }
    Ok(())
}

/// `%IteratorPrototype%[@@iterator]()` (§27.1.2.1): returns `this`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn iterator_iterator(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Entries)
}

fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Keys)
}

fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
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
    let ObjectKind::ArrayIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(incompatible(rt, call.this()));
    };
    let (Some(target), index, kind) = (state.target, state.next_index, state.kind) else {
        return finished(rt);
    };
    let step = array_iterator_step(rt, iterator, target, index, kind);
    if !matches!(step, Ok(Some(_))) {
        // Done, or `length` or an element threw: the iterator is finished.
        set_target(rt, iterator, None)?;
    }
    match step? {
        Some(result) => Ok(NativeReturn::Value(result.into())),
        None => finished(rt),
    }
}

/// One step of the closure of `CreateArrayIterator`: the result object,
/// or `None` at the end of the array-like.
fn array_iterator_step(
    rt: &mut Runtime,
    iterator: Gc<Object>,
    target: Gc<Object>,
    index: f64,
    kind: IterationKind,
) -> VmResult<Option<Gc<Object>>> {
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
    Ok(Some(rt.iter_result(result, false)?))
}

fn set_target(rt: &mut Runtime, iterator: Gc<Object>, target: Option<Gc<Object>>) -> VmResult<()> {
    if let ObjectKind::ArrayIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.target = target;
    }
    Ok(())
}

/// `{ value: undefined, done: true }`.
fn finished(rt: &mut Runtime) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(
        rt.iter_result(Value::Undefined, true)?.into(),
    ))
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
