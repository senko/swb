//! `%TypedArray.prototype%` (ECMA-262 §23.2.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-%typedarrayprototype%-object>):
//! the accessors `buffer`, `byteLength`, `byteOffset`, `length` and
//! `@@toStringTag`, and `set`, `subarray`, `slice`, `entries`, `keys`,
//! `values` and `@@iterator` (the same function object as `values`).
//!
//! The other methods are in `typed_array_callback.rs` (callbacks),
//! `typed_array_search.rs`, `typed_array_change.rs` and
//! `typed_array_sort.rs`; [`METHODS`] has them all in Chromium 148's key
//! order. `toString` is the function object of `Array.prototype`.
//!
//! The messages are V8's (measured with `tools/probes/js-typed.json`),
//! which differ between the methods for the same check: `set` and the
//! iterator methods say "this is not a typed array." for another receiver,
//! `subarray` and `slice` "Method … called on incompatible receiver";
//! `keys` says "Cannot perform … on a detached `ArrayBuffer`" where
//! `values` and `entries` say "on a detached or out-of-bounds `ArrayBuffer`".

use crate::heap::Gc;
use crate::object::{IterationKind, Object, PropertyDescriptor, TypedArrayData};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::{clamp_from_end, to_integer_or_infinity};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

use super::typed_array_callback::{
    every, filter, find, find_index, find_last, find_last_index, for_each, map, reduce,
    reduce_right, some,
};
use super::typed_array_change::{copy_within, fill, reverse, to_reversed, with};
use super::typed_array_create::{
    CHARGE_CHUNK, CreateArgs, content_mismatch, element_of_raw, out_of_bounds, type_name,
};
use super::typed_array_search::{at, includes, index_of, join, last_index_of, to_locale_string};
use super::typed_array_sort::{sort, to_sorted};

/// The methods of `%TypedArray.prototype%` in Chromium 148's key order
/// (after `constructor` and the accessors), with their `length`. `None`
/// marks `toString`, which [`install`] takes from `Array.prototype`.
const METHODS: [(&str, u32, Option<NativeFn>); 31] = [
    ("entries", 0, Some(entries)),
    ("keys", 0, Some(keys)),
    ("values", 0, Some(values)),
    ("at", 1, Some(at)),
    ("copyWithin", 2, Some(copy_within)),
    ("every", 1, Some(every)),
    ("fill", 1, Some(fill)),
    ("filter", 1, Some(filter)),
    ("find", 1, Some(find)),
    ("findIndex", 1, Some(find_index)),
    ("findLast", 1, Some(find_last)),
    ("findLastIndex", 1, Some(find_last_index)),
    ("forEach", 1, Some(for_each)),
    ("includes", 1, Some(includes)),
    ("indexOf", 1, Some(index_of)),
    ("join", 1, Some(join)),
    ("lastIndexOf", 1, Some(last_index_of)),
    ("map", 1, Some(map)),
    ("reverse", 0, Some(reverse)),
    ("reduce", 1, Some(reduce)),
    ("reduceRight", 1, Some(reduce_right)),
    ("set", 1, Some(set)),
    ("slice", 2, Some(slice)),
    ("some", 1, Some(some)),
    ("sort", 1, Some(sort)),
    ("subarray", 2, Some(subarray)),
    ("toReversed", 0, Some(to_reversed)),
    ("toSorted", 1, Some(to_sorted)),
    ("with", 2, Some(with)),
    ("toLocaleString", 0, Some(to_locale_string)),
    // §23.2.3.32: the same function object as `Array.prototype.toString`.
    ("toString", 0, None),
];

/// Installs the properties of `%TypedArray.prototype%` (`constructor` is
/// `%TypedArray%`).
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    constructor: Gc<Object>,
    proto: Gc<Object>,
) -> VmResult<()> {
    let key = crate::string::PropertyKey::String(rt.vm.atoms.constructor);
    rt.define(proto, key, constructor.into(), true, false, true)?;
    super::getter(rt, realm, proto, "buffer", buffer)?;
    super::getter(rt, realm, proto, "byteLength", byte_length)?;
    super::getter(rt, realm, proto, "byteOffset", byte_offset)?;
    let length_getter = super::getter(rt, realm, proto, "length", length)?;
    rt.set_intrinsic(realm, Intrinsic::TypedArrayLengthGetter, length_getter);
    let mut values_function = None;
    for (name, length, func) in METHODS {
        let Some(func) = func else { continue };
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "values" {
            values_function = Some(function);
        }
    }
    // §23.2.3.32: `toString` is the function object of `Array.prototype`.
    let array_proto = rt.intrinsic(realm, Intrinsic::ArrayPrototype)?;
    let key = rt.heap.key_from_str("toString")?;
    let to_string = rt.get_value(array_proto.into(), key)?;
    rt.define(proto, key, to_string, true, false, true)?;
    let tag = rt.new_builtin(realm, "get [Symbol.toStringTag]", 0, to_string_tag, false)?;
    let key = rt.symbol_key(WellKnown::ToStringTag);
    let desc = PropertyDescriptor::accessor(tag.into(), Value::Undefined, false, true);
    rt.define_own_property(proto, key, desc)?;
    // §23.2.3.40: `@@iterator` is the same function object as `values`.
    let values_function = values_function.ok_or(VmError::invariant(
        "%TypedArray%.prototype.values is missing",
    ))?;
    let key = rt.symbol_key(WellKnown::Iterator);
    rt.define(proto, key, values_function.into(), true, false, true)
}

// --- Receivers ---

/// `RequireInternalSlot(this, [[TypedArrayName]])` with V8's message
/// "Method NAME called on incompatible receiver …".
fn this_typed(
    rt: &Runtime,
    call: &NativeCall,
    name: &str,
) -> VmResult<(Gc<Object>, TypedArrayData)> {
    match rt.typed_slots(call.this())? {
        Some(slots) => Ok(slots),
        None => Err(rt.incompatible_receiver(name, call.this())),
    }
}

/// `RequireInternalSlot(this, [[TypedArrayName]])` with V8's message of
/// `set` and the iterator methods.
fn this_typed_short(rt: &Runtime, call: &NativeCall) -> VmResult<(Gc<Object>, TypedArrayData)> {
    rt.typed_slots(call.this())?
        .ok_or_else(|| VmError::type_error("this is not a typed array."))
}

// --- The accessors (§23.2.3.2, §23.2.3.3, §23.2.3.4, §23.2.3.22, §23.2.3.41) ---

/// `get %TypedArray%.prototype.buffer`: also for a detached buffer.
fn buffer(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data) = this_typed(rt, call, "get %TypedArray%.prototype.buffer")?;
    Ok(NativeReturn::Value(data.view.buffer.into()))
}

/// `get %TypedArray%.prototype.byteLength`: 0 when out of bounds.
fn byte_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data) = this_typed(rt, call, "get TypedArray.prototype.byteLength")?;
    let length = rt.heap.typed_length(&data)?.unwrap_or(0);
    Ok(NativeReturn::Value(Value::number(
        (length * data.ty.size()) as f64,
    )))
}

/// `get %TypedArray%.prototype.byteOffset`: 0 when out of bounds.
fn byte_offset(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data) = this_typed(rt, call, "get TypedArray.prototype.byteOffset")?;
    let offset = match rt.heap.typed_length(&data)? {
        Some(_) => data.view.byte_offset,
        None => 0,
    };
    Ok(NativeReturn::Value(Value::number(offset as f64)))
}

/// `get %TypedArray%.prototype.length`: 0 when out of bounds.
fn length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data) = this_typed(rt, call, "get TypedArray.prototype.length")?;
    let length = rt.heap.typed_length(&data)?.unwrap_or(0);
    Ok(NativeReturn::Value(Value::number(length as f64)))
}

/// `get %TypedArray%.prototype[@@toStringTag]`: the name of the
/// constructor, or `undefined` for a receiver that is not a typed array.
fn to_string_tag(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Some((_, data)) = rt.typed_slots(call.this())? else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    let name = rt.heap.intern_str(&type_name(data.ty))?;
    Ok(NativeReturn::Value(name.into()))
}

// --- The iterators (§23.2.3.7, §23.2.3.19, §23.2.3.37) ---

/// `%TypedArray%.prototype.entries()`.
fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(rt, call, IterationKind::Entries, "entries")
}

/// `%TypedArray%.prototype.keys()`.
fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(rt, call, IterationKind::Keys, "keys")
}

/// `%TypedArray%.prototype.values()`; also `@@iterator`.
fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(rt, call, IterationKind::Values, "values")
}

/// `ValidateTypedArray(this)`, then `CreateArrayIterator(this, kind)`.
fn create_iterator(
    rt: &mut Runtime,
    call: &NativeCall,
    kind: IterationKind,
    method: &str,
) -> VmResult<NativeReturn> {
    let (array, data) = this_typed_short(rt, call)?;
    if rt.heap.typed_length(&data)?.is_none() {
        let name = format!("%TypedArray%.prototype.{method}");
        // V8 words `keys` without "or out-of-bounds" (measured).
        return Err(if kind == IterationKind::Keys {
            VmError::type_error(format!("Cannot perform {name} on a detached ArrayBuffer"))
        } else {
            out_of_bounds(&name)
        });
    }
    let iterator = super::iterator::new_array_iterator(rt, call.realm(), array, kind)?;
    Ok(NativeReturn::Value(iterator.into()))
}

// --- set (§23.2.3.26) ---

/// `%TypedArray%.prototype.set(source [, offset])`.
fn set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.prototype.set";
    let (_, target) = this_typed_short(rt, call)?;
    let offset = match rt.arg(call, 1) {
        Value::Int(int) => f64::from(int),
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    if offset < 0.0 {
        return Err(VmError::range_error("offset is out of bounds"));
    }
    let source = rt.arg(call, 0);
    match rt.typed_slots(source)? {
        Some((_, source)) => set_from_typed_array(rt, &target, offset, &source, NAME)?,
        None => set_from_array_like(rt, &target, offset, source, NAME)?,
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The target offset as an index when `count` elements fit at it into
/// `length` elements, else V8's `RangeError`.
fn fitting_offset(offset: f64, count: f64, length: usize) -> VmResult<usize> {
    if offset == f64::INFINITY || count + offset > length as f64 {
        return Err(VmError::range_error("offset is out of bounds"));
    }
    Ok(offset as usize)
}

/// `SetTypedArrayFromTypedArray` (§23.2.3.26.1): a copy of the bytes for
/// the same element type (also within one buffer), else of the converted
/// elements (from a copy of the source bytes when both share a buffer).
fn set_from_typed_array(
    rt: &mut Runtime,
    target: &TypedArrayData,
    offset: f64,
    source: &TypedArrayData,
    name: &str,
) -> VmResult<()> {
    let target_length = rt
        .heap
        .typed_length(target)?
        .ok_or_else(|| out_of_bounds(name))?;
    let source_length = rt
        .heap
        .typed_length(source)?
        .ok_or_else(|| out_of_bounds(name))?;
    let to = fitting_offset(offset, source_length as f64, target_length)?;
    if target.ty.is_bigint() != source.ty.is_bigint() {
        return Err(content_mismatch());
    }
    rt.copy_elements(target, to, source, 0, source_length)
}

/// `SetTypedArrayFromArrayLike` (§23.2.3.26.2): each element of the
/// source with `Get`, converted with `TypedArraySetElement` (which skips
/// an index that a conversion made invalid).
fn set_from_array_like(
    rt: &mut Runtime,
    target: &TypedArrayData,
    offset: f64,
    source: Value,
    name: &str,
) -> VmResult<()> {
    let target_length = rt
        .heap
        .typed_length(target)?
        .ok_or_else(|| out_of_bounds(name))?;
    let source = rt.to_object(source)?;
    rt.heap.record(source);
    let source_length = rt.length_of_array_like(source)?;
    let to = fitting_offset(offset, source_length, target_length)?;
    for k in 0..source_length as u64 {
        rt.tick()?;
        rt.with_scope(|rt| {
            let key = rt.index_key(k as f64)?;
            let value = rt.get_value(source.into(), key)?;
            rt.typed_set_element(target, (to as u64 + k) as f64, value)
        })?;
    }
    Ok(())
}

// --- subarray (§23.2.3.30) and slice (§23.2.3.27) ---

/// A relative index of `subarray` and `slice` (`ToIntegerOrInfinity`,
/// from the end when negative, clamped to `0..=length`); `default` for
/// `undefined` where the step says so.
pub(super) fn relative_index(
    rt: &mut Runtime,
    value: Value,
    length: usize,
    default: Option<usize>,
) -> VmResult<usize> {
    if let (Value::Undefined, Some(default)) = (value, default) {
        return Ok(default);
    }
    let relative = match value {
        Value::Int(int) => f64::from(int),
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    Ok(clamp_from_end(relative, length as f64) as usize)
}

/// `%TypedArray%.prototype.subarray(start, end)`: a view of the same
/// buffer through `TypedArraySpeciesCreate`, which tracks the length of
/// the buffer when the receiver does and `end` is `undefined`.
fn subarray(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.prototype.subarray";
    let (array, data) = this_typed(rt, call, NAME)?;
    let source_length = rt.heap.typed_length(&data)?.unwrap_or(0);
    let start = relative_index(rt, rt.arg(call, 0), source_length, None)?;
    let size = data.ty.size();
    let begin = data.view.byte_offset + start * size;
    let end_arg = rt.arg(call, 1);
    let args = if data.view.byte_length.is_none() && end_arg.is_undefined() {
        CreateArgs::Buffer(data.view.buffer, begin, None)
    } else {
        let end = relative_index(rt, end_arg, source_length, Some(source_length))?;
        CreateArgs::Buffer(data.view.buffer, begin, Some(end.saturating_sub(start)))
    };
    let (result, _) = rt.typed_species_create(array, data.ty, args, NAME)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `%TypedArray%.prototype.slice(start, end)`: a new array through
/// `TypedArraySpeciesCreate`; the bytes are copied for the same element
/// type (forward, byte by byte, as the specification says: a target view
/// of the same buffer sees the bytes it wrote), else the elements are
/// converted.
fn slice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.prototype.slice";
    let (array, data, length) = rt.validate_typed_array(call.this(), NAME)?;
    let start = relative_index(rt, rt.arg(call, 0), length, None)?;
    let end = relative_index(rt, rt.arg(call, 1), length, Some(length))?;
    let count = end.saturating_sub(start);
    let (result, target) =
        rt.typed_species_create(array, data.ty, CreateArgs::Length(count), NAME)?;
    if count > 0 {
        // A conversion or the species constructor can have detached or
        // shrunk the buffer.
        let length = rt
            .heap
            .typed_length(&data)?
            .ok_or_else(|| out_of_bounds(NAME))?;
        let count = end.min(length).saturating_sub(start);
        copy_slice(rt, &target, &data, start, count)?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// Step 10 of `slice`: `count` elements of `source` from `start` to the
/// start of `target`.
fn copy_slice(
    rt: &mut Runtime,
    target: &TypedArrayData,
    source: &TypedArrayData,
    start: usize,
    count: usize,
) -> VmResult<()> {
    if target.ty == source.ty {
        let size = source.ty.size();
        let from = source.view.byte_offset + start * size;
        let bytes = count * size;
        // Charged before the copy: within one buffer it goes byte by byte.
        rt.charge_bytes(bytes)?;
        rt.heap.copy_bytes_forward(
            (target.view.buffer, target.view.byte_offset),
            (source.view.buffer, from),
            bytes,
        )?;
        return Ok(());
    }
    // `Get` and `Set` of each element: the types differ, the content types
    // are the same (no conversion runs script code). An index that is not
    // valid in the target is skipped, as `Set` does.
    let element_bytes = source.ty.size() + target.ty.size();
    for n in 0..count {
        if n % CHARGE_CHUNK == CHARGE_CHUNK - 1 {
            rt.charge_bytes(CHARGE_CHUNK * element_bytes)?;
        }
        let Some(raw) = rt.heap.typed_get(source, start + n)? else {
            continue;
        };
        rt.heap.typed_set(target, n, element_of_raw(raw))?;
    }
    rt.charge_bytes((count % CHARGE_CHUNK) * element_bytes)
}
