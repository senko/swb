//! `String.prototype[@@iterator]` and `%StringIteratorPrototype%`
//! (ECMA-262 §22.1.3.36, §22.1.5): iteration by code points, so
//! `for`-`of`, spread and destructuring of a string visit a surrogate pair
//! as one element and a lone surrogate as one element.
//!
//! The iterator holds the string, flattened when the iterator is made,
//! and the index of the next code unit. Each step allocates its element
//! (a string of one or two code units; the strings of one unit below 256
//! are shared) and the result object.

use swb_js_text::{String16, is_lead_surrogate, is_trail_surrogate};

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, StringIterator};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// Installs `next` and `@@toStringTag` of `%StringIteratorPrototype%`.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::StringIteratorPrototype)?;
    let next = super::method(rt, realm, proto, "next", 0, next)?;
    rt.set_intrinsic(realm, Intrinsic::StringIteratorNext, next);
    super::to_string_tag(rt, proto, "String Iterator")
}

/// `String.prototype[@@iterator]()` (§22.1.3.36): `CreateStringIterator`.
pub(super) fn string_iterator(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype[Symbol.iterator]")?;
    rt.flatten(string)?;
    let proto = rt.intrinsic(call.realm(), Intrinsic::StringIteratorPrototype)?;
    let state = StringIterator {
        string: Some(string),
        position: 0,
    };
    let iterator = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::StringIterator(Box::new(state)))?;
    Ok(NativeReturn::Value(iterator.into()))
}

/// `%StringIteratorPrototype%.next()` (§22.1.5.1): the next code point as
/// a string of one or two code units.
fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(iterator) = call.this() else {
        return Err(incompatible(rt, call.this()));
    };
    if !matches!(
        rt.heap.object(iterator)?.kind,
        ObjectKind::StringIterator(_)
    ) {
        return Err(incompatible(rt, call.this()));
    }
    let result = match next_value(rt, iterator)? {
        Some(element) => rt.iter_result(element, false)?,
        None => rt.iter_result(Value::Undefined, true)?,
    };
    Ok(NativeReturn::Value(result.into()))
}

/// The next element of the String Iterator `iterator`, or `None` when it
/// is done: `next` without its result object (the compiled iteration
/// steps call it for the built-in `next`, `vm/iter.rs`).
pub(super) fn next_value(rt: &mut Runtime, iterator: Gc<Object>) -> VmResult<Option<Value>> {
    let ObjectKind::StringIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(VmError::invariant("not a String Iterator"));
    };
    let (Some(string), position) = (state.string, state.position) else {
        return Ok(None);
    };
    let text = rt.heap.text(string)?;
    let Some(first) = text.get(position) else {
        if let ObjectKind::StringIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
            state.string = None;
        }
        return Ok(None);
    };
    let mut width = 1;
    let mut pair = None;
    if is_lead_surrogate(first)
        && let Some(second) = text.get(position + 1)
        && is_trail_surrogate(second)
    {
        pair = Some(second);
        width = 2;
    }
    if let ObjectKind::StringIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.position = position + width;
    }
    let element = match pair {
        // One unit: the shared string of the heap, for a unit below 256.
        None => rt.unit_string(first)?,
        Some(second) => rt.new_string(String16::from_units(&[first, second]))?,
    };
    Ok(Some(element.into()))
}

/// V8's `TypeError` for `next` on another receiver.
fn incompatible(rt: &Runtime, receiver: Value) -> VmError {
    VmError::type_error(format!(
        "Method String Iterator.prototype.next called on incompatible receiver {}",
        match receiver {
            Value::Object(object) => rt.object_text(object),
            _ => rt.primitive_text(receiver),
        }
    ))
}
