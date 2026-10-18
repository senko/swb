//! The methods of `%TypedArray%.prototype` that write the elements or
//! make an array of the same type without a callback (ECMA-262 §23.2.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-%typedarrayprototype%-object>):
//! `fill`, `copyWithin`, `reverse`, `toReversed` and `with`.
//!
//! The conversions of the arguments run script code, which can detach,
//! shrink or grow the buffer; `fill` and `copyWithin` read the length again
//! afterwards (and throw when the array is out of bounds) and `with`
//! reads the elements that are gone as `undefined`. The loops work on the
//! bytes of the buffer, a chunk of [`CHARGE_CHUNK`] elements at a time with
//! a charge of the time countdown after each.

use crate::heap::Gc;
use crate::object::{Object, TypedArrayData, element_bits};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::typed_array::INVALID_INDEX;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::typed_array_create::CHARGE_CHUNK;
use super::typed_array_method::{unavailable_now, validate_this};
use super::typed_array_proto::relative_index;

/// `%TypedArray%.prototype.fill(value [, start [, end]])` (§23.2.3.8).
pub(super) fn fill(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (array, data, length) = validate_this(rt, call, "fill")?;
    let element = rt.element_for(data.ty, rt.arg(call, 0))?;
    let start = relative_index(rt, rt.arg(call, 1), length, None)?;
    let end = relative_index(rt, rt.arg(call, 2), length, Some(length))?;
    // The conversions can have changed the buffer.
    let current = rt
        .heap
        .typed_length(&data)?
        .ok_or_else(|| unavailable_now("fill"))?;
    let end = end.min(current);
    let bits = element_bits(data.ty, element);
    let mut from = start;
    while from < end {
        let to = (from + CHARGE_CHUNK).min(end);
        rt.heap.typed_fill(&data, (from, to), bits)?;
        rt.charge_bytes((to - from) * data.ty.size())?;
        from = to;
    }
    Ok(NativeReturn::Value(array.into()))
}

/// `%TypedArray%.prototype.copyWithin(target, start [, end])`
/// (§23.2.3.6): a copy of the bytes within the buffer, as `memmove`.
pub(super) fn copy_within(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (array, data, length) = validate_this(rt, call, "copyWithin")?;
    let to = relative_index(rt, rt.arg(call, 0), length, None)?;
    let from = relative_index(rt, rt.arg(call, 1), length, None)?;
    let end = relative_index(rt, rt.arg(call, 2), length, Some(length))?;
    let count = end.saturating_sub(from).min(length - to);
    if count > 0 {
        let current = rt
            .heap
            .typed_length(&data)?
            .ok_or_else(|| unavailable_now("copyWithin"))?;
        copy_bytes(rt, &data, (to, from, count), current)?;
    }
    Ok(NativeReturn::Value(array.into()))
}

/// Steps 17.f to n of `copyWithin`: `count` elements from element `from`
/// to element `to`, for a buffer that now holds `current` elements. The
/// count is cut to what is left of the source and of the target (the
/// result that V8 and test262 expect for a buffer that a conversion
/// shrank, `coerced-target-start-end-shrink.js`; the literal byte loop of
/// the specification text copies nothing when the overlapping copy has to
/// start past the limit), then the bytes move as `memmove` does.
fn copy_bytes(
    rt: &mut Runtime,
    data: &TypedArrayData,
    (to, from, count): (usize, usize, usize),
    current: usize,
) -> VmResult<()> {
    let count = count
        .min(current.saturating_sub(from))
        .min(current.saturating_sub(to));
    let size = data.ty.size();
    let offset = data.view.byte_offset;
    let bytes = count * size;
    rt.charge_bytes(bytes)?;
    let buffer = data.view.buffer;
    rt.heap.copy_between_buffers(
        (buffer, offset + to * size),
        (buffer, offset + from * size),
        bytes,
    )?;
    Ok(())
}

/// Reverses the first `length` elements of the array in place.
fn reverse_in_place(rt: &mut Runtime, data: &TypedArrayData, length: usize) -> VmResult<()> {
    let pairs = length / 2;
    let mut first = 0;
    while first < pairs {
        let count = CHARGE_CHUNK.min(pairs - first);
        rt.heap.typed_reverse_pairs(data, length, (first, count))?;
        rt.charge_bytes(count * 2 * data.ty.size())?;
        first += count;
    }
    Ok(())
}

/// `%TypedArray%.prototype.reverse()` (§23.2.3.24).
pub(super) fn reverse(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (array, data, length) = validate_this(rt, call, "reverse")?;
    reverse_in_place(rt, &data, length)?;
    Ok(NativeReturn::Value(array.into()))
}

impl Runtime {
    /// `TypedArrayCreateSameType(exemplar, « length »)` (§23.2.4.3): a new
    /// array of the type of `data` in the current realm, with the
    /// intrinsic prototype and no species lookup.
    pub(super) fn typed_create_same_type(
        &mut self,
        data: &TypedArrayData,
        length: usize,
    ) -> VmResult<(Gc<Object>, TypedArrayData)> {
        let realm = self.current_realm();
        let (_, proto) = self.typed_array_intrinsics(realm, data.ty)?;
        self.allocate_typed_array(realm, proto, data.ty, length as u64)
    }
}

/// Copies the first `count` elements of `source` to the start of `target`
/// (the same type; `count` within both).
pub(super) fn copy_prefix(
    rt: &mut Runtime,
    target: &TypedArrayData,
    source: &TypedArrayData,
    count: usize,
) -> VmResult<()> {
    let bytes = count * source.ty.size();
    rt.charge_bytes(bytes)?;
    rt.heap.copy_between_buffers(
        (target.view.buffer, target.view.byte_offset),
        (source.view.buffer, source.view.byte_offset),
        bytes,
    )?;
    Ok(())
}

/// `%TypedArray%.prototype.toReversed()` (§23.2.3.33).
pub(super) fn to_reversed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "toReversed")?;
    let (result, target) = rt.typed_create_same_type(&data, length)?;
    copy_prefix(rt, &target, &data, length)?;
    reverse_in_place(rt, &target, length)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `%TypedArray%.prototype.with(index, value)` (§23.2.3.36).
pub(super) fn with(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "with")?;
    let relative = match rt.arg(call, 0) {
        Value::Int(int) => f64::from(int),
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    let actual = if relative >= 0.0 {
        relative
    } else {
        length as f64 + relative
    };
    let element = rt.element_for(data.ty, rt.arg(call, 1))?;
    if rt.heap.valid_index(&data, actual)?.is_none() {
        return Err(VmError::range_error(INVALID_INDEX));
    }
    let (result, target) = rt.typed_create_same_type(&data, length)?;
    // The conversions can have shrunk the source: the elements that are
    // gone are `undefined`, which `Set` turns into NaN or 0 (and into a
    // `TypeError` for a `BigInt` array).
    let current = rt.heap.typed_length(&data)?.unwrap_or(0).min(length);
    copy_prefix(rt, &target, &data, current)?;
    if current < length {
        fill_undefined(rt, &target, current, length)?;
    }
    if actual < length as f64 {
        rt.heap.typed_set(&target, actual as usize, element)?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Set(target, k, undefined)` for the elements `from..to` of a new array.
fn fill_undefined(
    rt: &mut Runtime,
    target: &TypedArrayData,
    from: usize,
    to: usize,
) -> VmResult<()> {
    if target.ty.is_bigint() {
        return Err(VmError::type_error("Cannot convert undefined to a BigInt"));
    }
    let bits = element_bits(target.ty, crate::object::Element::Number(f64::NAN));
    let mut at = from;
    while at < to {
        let end = (at + CHARGE_CHUNK).min(to);
        rt.heap.typed_fill(target, (at, end), bits)?;
        rt.charge_bytes((end - at) * target.ty.size())?;
        at = end;
    }
    Ok(())
}
