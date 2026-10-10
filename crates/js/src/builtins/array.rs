//! `Array` (ECMA-262 §23.1): the constructor, `Array.isArray`, and the
//! prototype methods `push`, `join`, `indexOf`, `slice`, `forEach`, `map`
//! and `toString`. The other methods are M7 feature 6.
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
use crate::vm::convert::{
    MAX_SAFE_INTEGER, MAX_STRING_LENGTH, checked_array_length, to_integer_or_infinity,
};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

/// A joined text above this many code units reserves its memory in the
/// heap's accounting at each doubling (the buffer is Rust memory until
/// the string exists).
const RESERVE_FROM: usize = 1 << 16;

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ArrayPrototype)?;
    let array = super::constructor(rt, realm, "Array", 1, array_constructor, proto)?;
    rt.set_intrinsic(realm, Intrinsic::ArrayConstructor, array);
    super::method(rt, realm, array, "isArray", 1, is_array)?;
    super::array_species::install(rt, realm, array, proto)?;
    let methods: [(&str, u32, NativeFn); 7] = [
        ("push", 1, push),
        ("join", 1, join),
        ("indexOf", 1, index_of),
        ("slice", 2, slice),
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

/// `Array.prototype.map(callback, thisArg)` (§23.1.3.21).
fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_of(call, "Array.prototype.map")?;
    let length = rt.length_of_array_like(object)?;
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(rt.not_callable(callback));
    }
    let result = rt.array_species_create(object, length)?;
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
                rt.create_data_property_or_throw(result, key, mapped)?;
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

/// The start index of a range argument of `slice` and the like
/// (§23.1.3.28 steps 4 to 7): a relative index clamped to `0..=length`.
fn relative_index(rt: &mut Runtime, value: Value, length: f64, default: f64) -> VmResult<f64> {
    if value.is_undefined() {
        return Ok(default);
    }
    let relative = to_integer_or_infinity(rt.to_number(value)?);
    Ok(if relative < 0.0 {
        (length + relative).max(0.0)
    } else {
        relative.min(length)
    })
}

/// `Array.prototype.slice(start, end)` (§23.1.3.28).
fn slice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let length = rt.length_of_array_like(object)?;
    let start = relative_index(rt, rt.arg(call, 0), length, 0.0)?;
    let end = relative_index(rt, rt.arg(call, 1), length, length)?;
    let count = (end - start).max(0.0);
    let result = rt.array_species_create(object, count)?;
    let mut k = start;
    let mut n = 0.0;
    while k < end {
        rt.tick()?;
        rt.with_scope(|rt| {
            let from = rt.index_key(k)?;
            if rt.has_property(object, from)? {
                let value = rt.get_value(object.into(), from)?;
                let to = rt.index_key(n)?;
                rt.create_data_property_or_throw(result, to, value)?;
            }
            Ok(())
        })?;
        k += 1.0;
        n += 1.0;
    }
    let key = PropertyKey::String(rt.heap.length_atom());
    rt.set_or_throw(result, key, Value::number(n))?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.indexOf(searchElement, fromIndex)` (§23.1.3.17).
fn index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_of(call, "Array.prototype.indexOf")?;
    let length = rt.length_of_array_like(object)?;
    let not_found = NativeReturn::Value(Value::Int(-1));
    if length == 0.0 {
        return Ok(not_found);
    }
    let n = to_integer_or_infinity(rt.to_number(rt.arg(call, 1))?);
    if n == f64::INFINITY {
        return Ok(not_found);
    }
    let mut k = if n >= 0.0 { n } else { (length + n).max(0.0) };
    let search = rt.arg(call, 0);
    while k < length {
        rt.tick()?;
        let found = rt.with_scope(|rt| {
            let key = rt.index_key(k)?;
            if rt.has_property(object, key)? {
                let element = rt.get_value(object.into(), key)?;
                return rt.strictly_equal(search, element);
            }
            Ok(false)
        })?;
        if found {
            return Ok(NativeReturn::Value(Value::number(k)));
        }
        k += 1.0;
    }
    Ok(not_found)
}
