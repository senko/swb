//! `DataView` (ECMA-262 §25.3,
//! <https://tc39.es/ecma262/#sec-dataview-objects>): the constructor, the
//! accessors `buffer`, `byteLength` and `byteOffset`, and the `get` and
//! `set` methods of the eleven element types (`GetViewValue`, §25.3.1.5,
//! and `SetViewValue`, §25.3.1.6). A view without a length on a resizable
//! buffer tracks its length; the bounds are checked after the conversions,
//! so a `valueOf` that detaches or shrinks the buffer is seen. The bytes
//! and the element conversions are in `object/buffer.rs`.
//!
//! The keys are in Chromium 148's order and the messages are V8's
//! (measured with `tools/probes/js-binary.json`), also the odd one of the
//! length check after `OrdinaryCreateFromConstructor`
//! (`Invalid DataView length undefined`). A read of a small integer type gives a Number
//! without an allocation.

use crate::heap::Gc;
use crate::object::{BufferView, ElementType, Object, ViewError};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::number::number_to_string;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

use super::array_buffer::index_value;

/// V8's message for an element outside the view (also for an index that
/// `ToIndex` rejects).
const OUTSIDE: &str = "Offset is outside the bounds of the DataView";

/// The methods in Chromium 148's order: the type and its `get` and `set`
/// functions.
const METHODS: [(ElementType, NativeFn, NativeFn); 11] = [
    (
        ElementType::Int8,
        |rt, call| get(rt, call, ElementType::Int8),
        |rt, call| set(rt, call, ElementType::Int8),
    ),
    (
        ElementType::Uint8,
        |rt, call| get(rt, call, ElementType::Uint8),
        |rt, call| set(rt, call, ElementType::Uint8),
    ),
    (
        ElementType::Int16,
        |rt, call| get(rt, call, ElementType::Int16),
        |rt, call| set(rt, call, ElementType::Int16),
    ),
    (
        ElementType::Uint16,
        |rt, call| get(rt, call, ElementType::Uint16),
        |rt, call| set(rt, call, ElementType::Uint16),
    ),
    (
        ElementType::Int32,
        |rt, call| get(rt, call, ElementType::Int32),
        |rt, call| set(rt, call, ElementType::Int32),
    ),
    (
        ElementType::Uint32,
        |rt, call| get(rt, call, ElementType::Uint32),
        |rt, call| set(rt, call, ElementType::Uint32),
    ),
    (
        ElementType::Float32,
        |rt, call| get(rt, call, ElementType::Float32),
        |rt, call| set(rt, call, ElementType::Float32),
    ),
    (
        ElementType::Float64,
        |rt, call| get(rt, call, ElementType::Float64),
        |rt, call| set(rt, call, ElementType::Float64),
    ),
    (
        ElementType::BigInt64,
        |rt, call| get(rt, call, ElementType::BigInt64),
        |rt, call| set(rt, call, ElementType::BigInt64),
    ),
    (
        ElementType::BigUint64,
        |rt, call| get(rt, call, ElementType::BigUint64),
        |rt, call| set(rt, call, ElementType::BigUint64),
    ),
    (
        ElementType::Float16,
        |rt, call| get(rt, call, ElementType::Float16),
        |rt, call| set(rt, call, ElementType::Float16),
    ),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::DataViewPrototype)?;
    super::constructor(rt, realm, "DataView", 1, data_view_constructor, proto)?;
    super::getter(rt, realm, proto, "buffer", buffer)?;
    super::getter(rt, realm, proto, "byteLength", byte_length)?;
    super::getter(rt, realm, proto, "byteOffset", byte_offset)?;
    for (ty, getter, setter) in METHODS {
        let name = ty.name();
        super::method(rt, realm, proto, &format!("get{name}"), 1, getter)?;
        super::method(rt, realm, proto, &format!("set{name}"), 2, setter)?;
    }
    super::to_string_tag(rt, proto, "DataView")
}

/// `RequireInternalSlot(this, [[DataView]])` with V8's message; `method`
/// gives the text between "Method " and " called" (built only for the
/// error).
fn this_view(
    rt: &Runtime,
    call: &NativeCall,
    method: impl FnOnce() -> String,
) -> VmResult<BufferView> {
    if let Value::Object(object) = call.this()
        && let Some(view) = rt.heap.data_view(object)?
    {
        return Ok(view);
    }
    Err(rt.incompatible_receiver(&method(), call.this()))
}

/// V8's `TypeError` for a view whose buffer is detached or that is out of
/// bounds.
fn out_of_bounds(method: &str) -> VmError {
    VmError::type_error(format!(
        "Cannot perform {method} on a detached or out-of-bounds ArrayBuffer"
    ))
}

// --- The constructor ---

/// V8's message for an offset that `ToIndex` rejects or that is past the
/// end of the buffer.
fn bad_offset(offset: f64) -> VmError {
    VmError::range_error(format!(
        "Start offset {} is outside the bounds of the buffer",
        number_to_string(offset)
    ))
}

/// V8's message for a length that `ToIndex` rejects or that does not fit.
fn bad_length(length: f64) -> VmError {
    VmError::range_error(format!(
        "Invalid DataView length {}",
        number_to_string(length)
    ))
}

/// V8's `TypeError` for a detached buffer in the constructor.
fn detached_buffer() -> VmError {
    VmError::type_error("Cannot perform DataView constructor on a detached ArrayBuffer")
}

/// `DataView ( buffer [ , byteOffset [ , byteLength ] ] )` (§25.3.2.1).
fn data_view_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error("Constructor DataView requires 'new'"));
    }
    let buffer = match rt.arg(call, 0) {
        Value::Object(object) if rt.heap.array_buffer(object)?.is_some() => object,
        _ => {
            return Err(VmError::type_error(
                "First argument to DataView constructor must be an ArrayBuffer",
            ));
        }
    };
    let offset = rt.index_integer(rt.arg(call, 1))?;
    let offset = index_value(offset).ok_or_else(|| bad_offset(offset))?;
    let (detached, buffer_length, fixed) = buffer_state(rt, buffer)?;
    if detached {
        return Err(detached_buffer());
    }
    if offset > buffer_length {
        return Err(bad_offset(offset as f64));
    }
    let length_arg = rt.arg(call, 2);
    let view_length = if length_arg.is_undefined() {
        fixed.then_some(buffer_length - offset)
    } else {
        let length = rt.index_integer(length_arg)?;
        let length = index_value(length).ok_or_else(|| bad_length(length))?;
        if offset + length > buffer_length {
            return Err(bad_length(length as f64));
        }
        Some(length)
    };
    let proto = rt.prototype_from(call, Intrinsic::DataViewPrototype)?;
    // The prototype's getter can detach or shrink the buffer.
    let (detached, buffer_length, _) = buffer_state(rt, buffer)?;
    if detached {
        return Err(detached_buffer());
    }
    if offset > buffer_length {
        return Err(bad_offset(offset as f64));
    }
    if !length_arg.is_undefined() && view_length.is_some_and(|len| offset + len > buffer_length) {
        return Err(VmError::range_error("Invalid DataView length undefined"));
    }
    let view = BufferView {
        buffer,
        byte_offset: to_usize(offset)?,
        byte_length: view_length.map(to_usize).transpose()?,
    };
    let object = rt.heap.new_data_view(Some(proto), view)?;
    Ok(NativeReturn::Value(object.into()))
}

/// Whether a buffer is detached, its byte length and whether it has a
/// fixed length.
fn buffer_state(rt: &Runtime, buffer: Gc<Object>) -> VmResult<(bool, u64, bool)> {
    let data = rt
        .heap
        .array_buffer(buffer)?
        .ok_or(VmError::invariant("a DataView of another object"))?;
    Ok((
        data.is_detached(),
        data.byte_length() as u64,
        data.max_byte_length().is_none(),
    ))
}

/// An offset or length within a buffer as `usize` (it is at most the
/// buffer's length).
fn to_usize(value: u64) -> VmResult<usize> {
    usize::try_from(value).map_err(|_| VmError::invariant("a view range above usize"))
}

// --- The accessors ---

/// `get DataView.prototype.buffer` (§25.3.4.1).
fn buffer(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let view = this_view(rt, call, || "get DataView.prototype.buffer".to_owned())?;
    Ok(NativeReturn::Value(view.buffer.into()))
}

/// `get DataView.prototype.byteLength` (§25.3.4.2).
fn byte_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "get DataView.prototype.byteLength";
    let view = this_view(rt, call, || NAME.to_owned())?;
    let (_, length) = rt
        .heap
        .view_range(&view)?
        .ok_or_else(|| out_of_bounds(NAME))?;
    Ok(NativeReturn::Value(Value::number(length as f64)))
}

/// `get DataView.prototype.byteOffset` (§25.3.4.3).
fn byte_offset(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "get DataView.prototype.byteOffset";
    let view = this_view(rt, call, || NAME.to_owned())?;
    rt.heap
        .view_range(&view)?
        .ok_or_else(|| out_of_bounds(NAME))?;
    Ok(NativeReturn::Value(Value::number(view.byte_offset as f64)))
}

// --- get and set ---

/// `GetViewValue ( view, requestIndex, isLittleEndian, type )`
/// (§25.3.1.5), for `DataView.prototype.getInt8` and the others. The 8-bit
/// types ignore the second argument.
fn get(rt: &mut Runtime, call: &NativeCall, ty: ElementType) -> VmResult<NativeReturn> {
    let method = || format!("DataView.prototype.get{}", ty.name());
    let view = this_view(rt, call, method)?;
    let index = rt.index_integer(rt.arg(call, 0))?;
    let little_endian = ty.size() > 1 && rt.to_boolean(rt.arg(call, 1))?;
    let index = index_value(index).ok_or_else(|| VmError::range_error(OUTSIDE))?;
    match rt.heap.view_get(&view, index, ty, little_endian)? {
        Ok(raw) => Ok(NativeReturn::Value(rt.element_value(raw)?)),
        Err(ViewError::OutOfBounds) => Err(out_of_bounds(&method())),
        Err(ViewError::Index) => Err(VmError::range_error(OUTSIDE)),
    }
}

/// `SetViewValue ( view, requestIndex, isLittleEndian, type, value )`
/// (§25.3.1.6): `ToIndex`, then `ToBigInt` or `ToNumber` of the value, then
/// `ToBoolean`, then the checks.
fn set(rt: &mut Runtime, call: &NativeCall, ty: ElementType) -> VmResult<NativeReturn> {
    let method = || format!("DataView.prototype.set{}", ty.name());
    let view = this_view(rt, call, method)?;
    let index = rt.index_integer(rt.arg(call, 0))?;
    let index = index_value(index).ok_or_else(|| VmError::range_error(OUTSIDE))?;
    let value = rt.element_of(ty, rt.arg(call, 1))?;
    let little_endian = ty.size() > 1 && rt.to_boolean(rt.arg(call, 2))?;
    match rt.heap.view_set(&view, index, ty, value, little_endian)? {
        Ok(()) => Ok(NativeReturn::Value(Value::Undefined)),
        Err(ViewError::OutOfBounds) => Err(out_of_bounds(&method())),
        Err(ViewError::Index) => Err(VmError::range_error(OUTSIDE)),
    }
}
