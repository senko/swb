//! `Array` (ECMA-262 §23.1): the constructor, `Array.isArray`, and the
//! prototype methods `push`, `join`, `forEach`, `map` and `toString`.
//!
//! The methods are generic: they work on any object with a `length`
//! (`LengthOfArrayLike`), test holes with `[[HasProperty]]` and read with
//! `[[Get]]`, which can run getters (re-entry). Each step of their loops
//! is a step of the time countdown, so a loop over 2^32 − 1 holes ends at
//! the deadline.

use swb_js_text::String16;

use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::{MAX_SAFE_INTEGER, MAX_STRING_LENGTH, checked_array_length};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

/// A joined text above this many code units reserves its memory in the
/// heap's accounting at each doubling (the buffer is Rust memory until
/// the string exists).
const RESERVE_FROM: usize = 1 << 16;

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ArrayPrototype)?;
    let array = super::constructor(rt, realm, "Array", 1, array_constructor, proto)?;
    super::method(rt, realm, array, "isArray", 1, is_array)?;
    let methods: [(&str, u32, NativeFn); 5] = [
        ("push", 1, push),
        ("join", 1, join),
        ("forEach", 1, for_each),
        ("map", 1, map),
        ("toString", 0, to_string),
    ];
    for (name, length, func) in methods {
        super::method(rt, realm, proto, name, length, func)?;
    }
    Ok(())
}

/// `Array(...values)` (§23.1.1.1), with and without `new`.
fn array_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let proto = rt.prototype_from(call, Intrinsic::ArrayPrototype)?;
    let argc = call.argc();
    if argc == 1 {
        let length = rt.arg(call, 0);
        if let Some(number) = length.as_number() {
            let array = rt
                .heap
                .new_array(Some(proto), checked_array_length(number)?)?;
            return Ok(NativeReturn::Value(array.into()));
        }
    }
    let array = rt.heap.new_array(Some(proto), 0)?;
    rt.heap.record(array);
    for i in 0..argc {
        rt.tick()?;
        let value = rt.arg(call, i);
        rt.heap
            .create_data_property(array, PropertyKey::Index(i as u32), value, &rt.vm)?;
    }
    Ok(NativeReturn::Value(array.into()))
}

/// `Array.isArray(arg)` (§23.1.2.2; no proxies yet).
fn is_array(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let result = match rt.arg(call, 0) {
        Value::Object(object) => matches!(rt.heap.object(object)?.kind, ObjectKind::Array { .. }),
        _ => false,
    };
    Ok(NativeReturn::Value(Value::Bool(result)))
}

/// `Array.prototype.push(...items)` (§23.1.3.23).
fn push(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let length = rt.length_of_array_like(object)?;
    let count = call.argc();
    if length + count as f64 > MAX_SAFE_INTEGER {
        return Err(VmError::type_error(format!(
            "Pushing {count} elements on an array-like of length {} is disallowed, as the total surpasses 2**53-1",
            crate::vm::number::number_to_string(length)
        )));
    }
    for i in 0..count {
        rt.tick()?;
        let key = rt.index_key(length + i as f64)?;
        let value = rt.arg(call, i);
        rt.set_or_throw(object, key, value)?;
    }
    let new_length = Value::number(length + count as f64);
    let key = PropertyKey::String(rt.heap.length_atom());
    rt.set_or_throw(object, key, new_length)?;
    Ok(NativeReturn::Value(new_length))
}

/// `Array.prototype.join(separator)` (§23.1.3.18). A receiver that is
/// already being joined gives the empty string (V8's cycle detection; the
/// specification would recurse without end).
fn join(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    if rt.vm.join_stack.contains(&object) {
        return Ok(NativeReturn::Value(rt.vm.atoms.empty.into()));
    }
    let length = rt.length_of_array_like(object)?;
    let separator = match rt.arg(call, 0) {
        Value::Undefined => String16::from(","),
        value => {
            let string = rt.to_string(value)?;
            let units = rt.heap.string(string)?.as_str16().to_string16();
            rt.charge_units(units.len())?;
            units
        }
    };
    rt.vm.join_stack.push(object);
    let result = join_elements(rt, object, length, &separator);
    rt.vm.join_stack.pop();
    let text = result?;
    if text.len() >= RESERVE_FROM {
        rt.heap.reserve(text.len() * 2, &rt.vm)?;
    }
    let string = rt.heap.alloc_string(text)?;
    Ok(NativeReturn::Value(string.into()))
}

/// The loop of `join`.
fn join_elements(
    rt: &mut Runtime,
    object: Gc<Object>,
    length: f64,
    separator: &String16,
) -> VmResult<String16> {
    let mut text = String16::new();
    let mut reserved = RESERVE_FROM;
    let mut k = 0.0;
    while k < length {
        rt.tick()?;
        if k > 0.0 {
            text.push_str16(separator.as_str16());
            rt.charge_units(separator.len())?;
        }
        rt.with_scope(|rt| {
            let key = rt.index_key(k)?;
            let element = rt.get_value(object.into(), key)?;
            if !element.is_nullish() {
                let string = rt.to_string(element)?;
                let piece = rt.heap.string(string)?.as_str16();
                let units = piece.len();
                text.push_str16(piece);
                // The copy costs steps in proportion to its size.
                rt.charge_units(units)?;
            }
            Ok(())
        })?;
        if text.len() > MAX_STRING_LENGTH {
            return Err(VmError::range_error("Invalid string length"));
        }
        if text.len() > reserved {
            // The buffer counts against the heap limit before it is a
            // string (checked at each doubling).
            reserved = text.len() * 2;
            rt.heap.reserve(reserved * 2, &rt.vm)?;
        }
        k += 1.0;
    }
    Ok(text)
}

/// `Array.prototype.forEach(callback, thisArg)` (§23.1.3.15).
fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_of(call, "Array.prototype.forEach")?;
    let length = rt.length_of_array_like(object)?;
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(rt.not_callable(callback));
    }
    let this_arg = rt.arg(call, 1);
    let mut k = 0.0;
    while k < length {
        rt.tick()?;
        rt.with_scope(|rt| {
            let key = rt.index_key(k)?;
            if rt.has_property(object, key)? {
                let value = rt.get_value(object.into(), key)?;
                rt.call(
                    callback,
                    this_arg,
                    &[value, Value::number(k), object.into()],
                )?;
            }
            Ok(())
        })?;
        k += 1.0;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `Array.prototype.map(callback, thisArg)` (§23.1.3.21). Deviation: the
/// result is always a plain array of the current realm; `ArraySpeciesCreate`
/// (`constructor` and `Symbol.species`) comes in M7.
fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_of(call, "Array.prototype.map")?;
    let length = rt.length_of_array_like(object)?;
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(rt.not_callable(callback));
    }
    let length_u32 = checked_array_length(length)?;
    let proto = rt.intrinsic(rt.current_realm(), Intrinsic::ArrayPrototype)?;
    let result = rt.heap.new_array(Some(proto), length_u32)?;
    rt.heap.record(result);
    let this_arg = rt.arg(call, 1);
    let mut k = 0.0;
    while k < length {
        rt.tick()?;
        rt.with_scope(|rt| {
            let key = rt.index_key(k)?;
            if rt.has_property(object, key)? {
                let value = rt.get_value(object.into(), key)?;
                let mapped = rt.call(
                    callback,
                    this_arg,
                    &[value, Value::number(k), object.into()],
                )?;
                if !rt.heap.create_data_property(result, key, mapped, &rt.vm)? {
                    return Err(VmError::type_error(format!(
                        "Cannot add property {}, object is not extensible",
                        rt.key_text(key)
                    )));
                }
            }
            Ok(())
        })?;
        k += 1.0;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.toString()` (§23.1.3.36): calls `join` as a deferred
/// call, or `Object.prototype.toString` if `join` is not callable.
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let key = rt.heap.key_from_str("join")?;
    let join = rt.get_value(object.into(), key)?;
    if rt.is_callable(join)? {
        return Ok(NativeReturn::Call {
            callee: join,
            this: object.into(),
            args: Vec::new(),
        });
    }
    let text = super::object::builtin_tag_text(rt, object.into())?;
    Ok(NativeReturn::Value(text))
}
