//! `String.prototype[@@iterator]` and `%StringIteratorPrototype%`
//! (ECMA-262 §22.1.3.36, §22.1.5): iteration by code points, so
//! `for`-`of`, spread and destructuring of a string visit a surrogate pair
//! as one element and a lone surrogate as one element.
//!
//! The iterator holds the string, flattened when the iterator is made,
//! and the index of the next code unit. Each step allocates its element
//! (one string of one or two code units) and the result object.

use swb_js_text::{String16, is_lead_surrogate, is_trail_surrogate};

use crate::object::{ObjectKind, StringIterator};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// Installs `next` and `@@toStringTag` of `%StringIteratorPrototype%`.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::StringIteratorPrototype)?;
    super::method(rt, realm, proto, "next", 0, next)?;
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
    let ObjectKind::StringIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(incompatible(rt, call.this()));
    };
    let (Some(string), position) = (state.string, state.position) else {
        let result = rt.iter_result(Value::Undefined, true)?;
        return Ok(NativeReturn::Value(result.into()));
    };
    let text = rt.heap.text(string)?;
    let Some(first) = text.get(position) else {
        if let ObjectKind::StringIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
            state.string = None;
        }
        let result = rt.iter_result(Value::Undefined, true)?;
        return Ok(NativeReturn::Value(result.into()));
    };
    let mut element = String16::new();
    element.push(first);
    let mut width = 1;
    if is_lead_surrogate(first)
        && let Some(second) = text.get(position + 1)
        && is_trail_surrogate(second)
    {
        element.push(second);
        width = 2;
    }
    if let ObjectKind::StringIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.position = position + width;
    }
    let element = rt.new_string(element)?;
    let result = rt.iter_result(element.into(), false)?;
    Ok(NativeReturn::Value(result.into()))
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
