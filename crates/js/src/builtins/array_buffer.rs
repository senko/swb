//! `ArrayBuffer` (ECMA-262 §25.1,
//! <https://tc39.es/ecma262/#sec-arraybuffer-objects>): the constructor
//! with `maxByteLength` (`AllocateArrayBuffer`, §25.1.3.1), `isView`,
//! `@@species`, and `byteLength`, `maxByteLength`, `resizable`,
//! `detached`, `resize`, `slice`, `transfer` and `transferToFixedLength`
//! (`ArrayBufferCopyAndDetach`, §25.1.3.3) of the prototype; the engine
//! operations that the views share (`ToIndex`, the element conversions);
//! and `DetachArrayBuffer` for the host ([`Runtime::detach_array_buffer`]).
//! The bytes and their accounting are in `object/buffer.rs`.
//!
//! The keys are in Chromium 148's order and the messages are V8's
//! (measured with `tools/probes/js-binary.json`). Deliberate differences
//! from Chromium 148 (ADR 0026 section 15): `ToIndex` of the length
//! rejects 2^53 and more at once, before `maxByteLength` is read and
//! before the prototype (V8 reads both first and rejects the length later);
//! `resize` converts the new length before it checks for a detached buffer,
//! as §25.1.6.6 says (V8 checks the buffer first). A buffer above the heap
//! limit is a `RangeError` ("Array buffer allocation failed"), not the end
//! of the script. There is no `SharedArrayBuffer` (Chromium 148 defines it
//! only in cross-origin isolated pages) and no immutable buffer
//! (`transferToImmutable`; Chromium 148 does not have it).

use swb_js_text::bigint::BigInt;

use crate::heap::Gc;
use crate::object::{
    Element, ElementType, MAX_BYTE_LENGTH, MAX_RESIZABLE_BYTE_LENGTH, Object, ObjectKind,
    PropertyDescriptor, RawValue, zeroed_bytes,
};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::bigint::bigint_error;
use crate::vm::convert::{MAX_SAFE_INTEGER, to_integer_or_infinity};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

/// V8's message for a byte length that `ToIndex` or the allocation
/// rejects.
pub(super) const INVALID_LENGTH: &str = "Invalid array buffer length";

/// V8's message for a `maxByteLength` that `ToIndex` rejects, that is below
/// the length, or that is above V8's limit.
const INVALID_MAX_LENGTH: &str = "Invalid array buffer max length";

/// V8's message for a buffer that the memory (here: the heap limit) does
/// not allow.
pub(super) const ALLOCATION_FAILED: &str = "Array buffer allocation failed";

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ArrayBufferPrototype)?;
    let ctor = super::constructor(rt, realm, "ArrayBuffer", 1, array_buffer_constructor, proto)?;
    rt.heap.record(ctor);
    rt.set_intrinsic(realm, Intrinsic::ArrayBufferConstructor, ctor);
    super::method(rt, realm, ctor, "isView", 1, is_view)?;
    let species = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    rt.set_intrinsic(realm, Intrinsic::ArrayBufferSpeciesGetter, species);
    let key = rt.symbol_key(WellKnown::Species);
    let desc = PropertyDescriptor::accessor(species.into(), Value::Undefined, false, true);
    rt.define_own_property(ctor, key, desc)?;
    // Chromium 148's order of the keys.
    super::getter(rt, realm, proto, "byteLength", byte_length)?;
    super::method(rt, realm, proto, "slice", 2, slice)?;
    super::getter(rt, realm, proto, "maxByteLength", max_byte_length)?;
    super::getter(rt, realm, proto, "resizable", resizable)?;
    super::method(rt, realm, proto, "resize", 1, resize)?;
    super::method(rt, realm, proto, "transfer", 0, transfer)?;
    super::method(
        rt,
        realm,
        proto,
        "transferToFixedLength",
        0,
        transfer_to_fixed_length,
    )?;
    super::getter(rt, realm, proto, "detached", detached)?;
    super::to_string_tag(rt, proto, "ArrayBuffer")
}

/// The integer of `ToIndex` (§7.1.22) in the range 0 to 2^53 − 1, or
/// `None`.
pub(super) fn index_value(integer: f64) -> Option<u64> {
    (0.0..=MAX_SAFE_INTEGER)
        .contains(&integer)
        .then_some(integer as u64)
}

/// V8's `TypeError` for an operation on a detached buffer.
fn detached_error(method: &str) -> VmError {
    VmError::type_error(format!("Cannot perform {method} on a detached ArrayBuffer"))
}

impl Runtime {
    /// The first step of `ToIndex` (§7.1.22): `ToIntegerOrInfinity`; the
    /// caller checks the range ([`index_value`]) and words the error.
    pub(super) fn index_integer(&mut self, value: Value) -> VmResult<f64> {
        if let Value::Int(int) = value {
            return Ok(f64::from(int));
        }
        Ok(to_integer_or_infinity(self.to_number(value)?))
    }

    /// The value to store for an element of type `ty`: `ToBigInt64` or
    /// `ToBigUint64` (§7.1.15, §7.1.16) for the `BigInt` types, `ToNumber`
    /// for the others (the conversions of `SetViewValue`, §25.3.1.6, and
    /// of the typed arrays). Can run script code.
    pub(crate) fn element_of(&mut self, ty: ElementType, value: Value) -> VmResult<Element> {
        Ok(match ty {
            ElementType::BigInt64 => Element::Bits(self.to_big_int64(value)? as u64),
            ElementType::BigUint64 => Element::Bits(self.to_big_uint64(value)?),
            _ => Element::Number(self.to_number(value)?),
        })
    }

    /// The JavaScript value of an element that was read: a Number, or a
    /// new `BigInt` (not recorded; the caller returns it or records it).
    pub(crate) fn element_value(&mut self, raw: RawValue) -> VmResult<Value> {
        Ok(match raw {
            RawValue::Int(int) => Value::Int(int),
            RawValue::Number(number) => Value::number(number),
            RawValue::BigInt64(int) => Value::BigInt(self.bigint_from_i64(int)?),
            RawValue::BigUint64(int) => {
                let value = BigInt::from_u64(int).map_err(bigint_error)?;
                Value::BigInt(self.new_bigint(value)?)
            }
        })
    }

    /// `CreateByteDataBlock` (§6.2.9.1): `len` zero bytes after a
    /// reservation (a safepoint: the caller's handles must be rooted),
    /// with the zero-fill charged to the time countdown. Bytes that the
    /// heap limit does not allow, or that the allocator cannot give, are a
    /// `RangeError` with V8's message.
    pub(super) fn new_byte_block(&mut self, len: usize) -> VmResult<Vec<u8>> {
        if !self.heap.reserve_buffer(len, &self.vm) {
            return Err(VmError::range_error(ALLOCATION_FAILED));
        }
        let bytes = zeroed_bytes(len).ok_or_else(|| VmError::range_error(ALLOCATION_FAILED))?;
        self.charge_bytes(len)?;
        Ok(bytes)
    }

    /// The steps of `AllocateArrayBuffer` (§25.1.3.1) after
    /// `OrdinaryCreateFromConstructor` (the caller resolved the prototype
    /// and compared the length with the maximum): a length above V8's
    /// limit is a `RangeError` with `too_long`, a maximum above V8's limit
    /// one with V8's message. The buffer is recorded.
    pub(super) fn allocate_array_buffer(
        &mut self,
        proto: Gc<Object>,
        byte_length: u64,
        max_byte_length: Option<u64>,
        too_long: &'static str,
    ) -> VmResult<Gc<Object>> {
        if byte_length > MAX_BYTE_LENGTH {
            return Err(VmError::range_error(too_long));
        }
        if max_byte_length.is_some_and(|max| max > MAX_RESIZABLE_BYTE_LENGTH) {
            return Err(VmError::range_error(INVALID_MAX_LENGTH));
        }
        let len =
            usize::try_from(byte_length).map_err(|_| VmError::range_error(ALLOCATION_FAILED))?;
        let max = max_byte_length
            .map(usize::try_from)
            .transpose()
            .map_err(|_| VmError::range_error(INVALID_MAX_LENGTH))?;
        let bytes = self.new_byte_block(len)?;
        let buffer = self.heap.new_array_buffer(Some(proto), bytes, max)?;
        Ok(self.heap.record(buffer))
    }

    /// `DetachArrayBuffer` (§25.1.3.5) for the host: `$262.detachArrayBuffer`
    /// of test262 (later the transfer lists of `postMessage`). A detached
    /// buffer stays detached; a value that is not an `ArrayBuffer` is a
    /// `TypeError`. The bytes are freed at once.
    pub fn detach_array_buffer(&mut self, value: Value) -> VmResult<()> {
        let Value::Object(buffer) = value else {
            return Err(VmError::type_error("Argument is not an ArrayBuffer"));
        };
        if self.heap.array_buffer(buffer)?.is_none() {
            return Err(VmError::type_error("Argument is not an ArrayBuffer"));
        }
        self.heap.detach_array_buffer(buffer)?;
        Ok(())
    }
}

/// `RequireInternalSlot(this, [[ArrayBufferData]])` with V8's message
/// (`method` is the text between "Method " and " called").
fn this_buffer(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<Gc<Object>> {
    if let Value::Object(object) = call.this()
        && rt.heap.array_buffer(object)?.is_some()
    {
        return Ok(object);
    }
    Err(rt.incompatible_receiver(method, call.this()))
}

/// The data of a buffer that [`this_buffer`] checked.
fn buffer_data(rt: &Runtime, buffer: Gc<Object>) -> VmResult<&crate::object::ArrayBufferData> {
    rt.heap
        .array_buffer(buffer)?
        .ok_or(VmError::invariant("an ArrayBuffer without its data"))
}

// --- The constructor ---

/// `ArrayBuffer ( length [ , options ] )` (§25.1.4.1).
fn array_buffer_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(
            "Constructor ArrayBuffer requires 'new'",
        ));
    }
    let length = rt.index_integer(rt.arg(call, 0))?;
    let byte_length = index_value(length).ok_or_else(|| VmError::range_error(INVALID_LENGTH))?;
    let max = max_byte_length_option(rt, rt.arg(call, 1))?;
    // AllocateArrayBuffer step 3: before the prototype.
    if max.is_some_and(|max| byte_length > max) {
        return Err(VmError::range_error(INVALID_MAX_LENGTH));
    }
    let proto = rt.prototype_from(call, Intrinsic::ArrayBufferPrototype)?;
    let buffer = rt.allocate_array_buffer(proto, byte_length, max, INVALID_LENGTH)?;
    Ok(NativeReturn::Value(buffer.into()))
}

/// `GetArrayBufferMaxByteLengthOption` (§25.1.3.7).
fn max_byte_length_option(rt: &mut Runtime, options: Value) -> VmResult<Option<u64>> {
    let Value::Object(options) = options else {
        return Ok(None);
    };
    let key = rt.heap.key_from_str("maxByteLength")?;
    rt.heap.record(key);
    let value = rt.get_value(options.into(), key)?;
    if value.is_undefined() {
        return Ok(None);
    }
    let max = rt.index_integer(value)?;
    index_value(max)
        .map(Some)
        .ok_or_else(|| VmError::range_error(INVALID_MAX_LENGTH))
}

/// `ArrayBuffer.isView ( arg )` (§25.1.5.1): whether `arg` has a
/// `[[ViewedArrayBuffer]]` slot (a `DataView` or a typed array).
fn is_view(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let view = match rt.arg(call, 0) {
        Value::Object(object) => matches!(
            rt.heap.object(object)?.kind,
            ObjectKind::DataView(_) | ObjectKind::TypedArray(_)
        ),
        _ => false,
    };
    Ok(NativeReturn::Value(Value::Bool(view)))
}

/// `get ArrayBuffer[@@species]` (§25.1.5.3): the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

// --- The accessors ---

/// `get ArrayBuffer.prototype.byteLength` (§25.1.6.1): 0 when detached.
fn byte_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let buffer = this_buffer(rt, call, "get ArrayBuffer.prototype.byteLength")?;
    let length = buffer_data(rt, buffer)?.byte_length();
    Ok(NativeReturn::Value(Value::number(length as f64)))
}

/// `get ArrayBuffer.prototype.maxByteLength` (§25.1.6.4): 0 when detached,
/// the length for a fixed-length buffer.
fn max_byte_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let buffer = this_buffer(rt, call, "get ArrayBuffer.prototype.maxByteLength")?;
    let data = buffer_data(rt, buffer)?;
    let length = if data.is_detached() {
        0
    } else {
        data.max_byte_length().unwrap_or(data.byte_length())
    };
    Ok(NativeReturn::Value(Value::number(length as f64)))
}

/// `get ArrayBuffer.prototype.resizable` (§25.1.6.5).
fn resizable(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let buffer = this_buffer(rt, call, "get ArrayBuffer.prototype.resizable")?;
    let resizable = buffer_data(rt, buffer)?.max_byte_length().is_some();
    Ok(NativeReturn::Value(Value::Bool(resizable)))
}

/// `get ArrayBuffer.prototype.detached` (§25.1.6.3).
fn detached(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let buffer = this_buffer(rt, call, "get ArrayBuffer.prototype.detached")?;
    let detached = buffer_data(rt, buffer)?.is_detached();
    Ok(NativeReturn::Value(Value::Bool(detached)))
}

// --- resize ---

/// `ArrayBuffer.prototype.resize ( newLength )` (§25.1.6.6): in place; the
/// vector keeps its capacity unless it shrinks below half of it.
fn resize(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "ArrayBuffer.prototype.resize";
    let invalid = || VmError::range_error(format!("{NAME}: Invalid length parameter"));
    let buffer = this_buffer(rt, call, NAME)?;
    let Some(max) = buffer_data(rt, buffer)?.max_byte_length() else {
        return Err(rt.incompatible_receiver(NAME, call.this()));
    };
    let length = rt.index_integer(rt.arg(call, 0))?;
    let length = index_value(length).ok_or_else(invalid)?;
    let data = buffer_data(rt, buffer)?;
    if data.is_detached() {
        return Err(detached_error(NAME));
    }
    let old = data.byte_length();
    let len = usize::try_from(length)
        .ok()
        .filter(|&len| len <= max)
        .ok_or_else(invalid)?;
    if len > old && !rt.heap.reserve_buffer(len - old, &rt.vm) {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    if !rt.heap.resize_array_buffer(buffer, len)? {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    // An upper bound of the work: a reallocation copies up to `len` bytes,
    // and a growth zero-fills the new ones.
    rt.charge_bytes(len)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

// --- slice ---

/// The relative index of `slice` (`ToClampedIndex`, §7.1.26): from the end
/// for a negative value, clamped to 0 and `length`.
fn clamped_index(rt: &mut Runtime, value: Value, length: f64) -> VmResult<f64> {
    let relative = rt.index_integer(value)?;
    Ok(if relative < 0.0 {
        (length + relative).max(0.0)
    } else {
        relative.min(length)
    })
}

/// `ArrayBuffer.prototype.slice ( start, end )` (§25.1.6.7).
fn slice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "ArrayBuffer.prototype.slice";
    let buffer = this_buffer(rt, call, NAME)?;
    if buffer_data(rt, buffer)?.is_detached() {
        return Err(detached_error(NAME));
    }
    let length = buffer_data(rt, buffer)?.byte_length() as f64;
    let first = clamped_index(rt, rt.arg(call, 0), length)?;
    let last = match rt.arg(call, 1) {
        Value::Undefined => length,
        end => clamped_index(rt, end, length)?,
    };
    let new_length = (last - first).max(0.0);
    let realm = call.realm();
    let default = rt.intrinsic(realm, Intrinsic::ArrayBufferConstructor)?;
    let getter = rt.intrinsic(realm, Intrinsic::ArrayBufferSpeciesGetter)?;
    let constructor = rt.species_constructor(buffer, default, getter)?;
    if constructor == Value::Object(default) {
        let target = slice_copy(rt, buffer, first as usize, new_length as usize, realm)?;
        return Ok(NativeReturn::Value(target.into()));
    }
    let target = match rt.construct(constructor, &[Value::number(new_length)])? {
        Value::Object(object) => rt.heap.record(object),
        _ => return Err(VmError::invariant("a constructor returned a primitive")),
    };
    check_slice_target(rt, buffer, target, new_length)?;
    let current = buffer_data(rt, buffer)?.byte_length();
    let first = first as usize;
    if current > first {
        let count = (new_length as usize).min(current - first);
        rt.heap.copy_buffer_bytes(target, buffer, first, count)?;
        rt.charge_bytes(count)?;
    }
    Ok(NativeReturn::Value(target.into()))
}

/// The end of `slice` for the default constructor: `Construct(%ArrayBuffer%,
/// « newLength »)` without the call (its prototype is the fixed
/// `%ArrayBuffer%.prototype`, and no script code runs from here on, so the
/// checks of the new buffer cannot fail), with the new bytes copied from
/// the buffer instead of zero-filled and then overwritten.
fn slice_copy(
    rt: &mut Runtime,
    buffer: Gc<Object>,
    first: usize,
    len: usize,
    realm: u32,
) -> VmResult<Gc<Object>> {
    if !rt.heap.reserve_buffer(len, &rt.vm) {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    // The getter of `constructor` can have detached the buffer (step 22).
    if buffer_data(rt, buffer)?.is_detached() {
        return Err(detached_error("ArrayBuffer.prototype.slice"));
    }
    let bytes = rt
        .heap
        .copy_of_buffer_bytes(buffer, first, len)?
        .ok_or_else(|| VmError::range_error(ALLOCATION_FAILED))?;
    let proto = rt.intrinsic(realm, Intrinsic::ArrayBufferPrototype)?;
    let target = rt.heap.new_array_buffer(Some(proto), bytes, None)?;
    rt.heap.record(target);
    rt.charge_bytes(len)?;
    Ok(target)
}

/// The checks of `slice` on the new buffer (steps 15 to 21), with V8's
/// messages.
fn check_slice_target(
    rt: &Runtime,
    buffer: Gc<Object>,
    target: Gc<Object>,
    new_length: f64,
) -> VmResult<()> {
    const NAME: &str = "ArrayBuffer.prototype.slice";
    let Some(data) = rt.heap.array_buffer(target)? else {
        return Err(rt.incompatible_receiver(NAME, target.into()));
    };
    if data.is_detached() {
        return Err(detached_error(NAME));
    }
    if target == buffer {
        return Err(VmError::type_error(
            "ArrayBuffer subclass returned this from species constructor",
        ));
    }
    if (data.byte_length() as f64) < new_length {
        return Err(VmError::type_error(
            "Derived ArrayBuffer constructor created a buffer which was too small",
        ));
    }
    if buffer_data(rt, buffer)?.is_detached() {
        return Err(detached_error(NAME));
    }
    Ok(())
}

// --- transfer ---

/// `ArrayBuffer.prototype.transfer ( [ newLength ] )` (§25.1.6.8).
fn transfer(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    copy_and_detach(rt, call, true, "ArrayBuffer.prototype.transfer")
}

/// `ArrayBuffer.prototype.transferToFixedLength ( [ newLength ] )`
/// (§25.1.6.9).
fn transfer_to_fixed_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    copy_and_detach(
        rt,
        call,
        false,
        "ArrayBuffer.prototype.transferToFixedLength",
    )
}

/// `ArrayBufferCopyAndDetach` (§25.1.3.3) with V8's messages: a negative
/// length is an "Invalid array buffer length", one of 2^53 and more, or
/// above V8's limit, an "Invalid length parameter" of the method. The new
/// buffer takes the vector of the old one (the note of step 15 allows the
/// move), so a transfer to the same or a smaller length copies nothing.
fn copy_and_detach(
    rt: &mut Runtime,
    call: &NativeCall,
    preserve_resizability: bool,
    name: &'static str,
) -> VmResult<NativeReturn> {
    let invalid = || VmError::range_error(format!("{name}: Invalid length parameter"));
    let buffer = this_buffer(rt, call, name)?;
    let new_length = match rt.arg(call, 0) {
        Value::Undefined => buffer_data(rt, buffer)?.byte_length() as u64,
        value => {
            let length = rt.index_integer(value)?;
            if length < 0.0 {
                return Err(VmError::range_error(INVALID_LENGTH));
            }
            index_value(length).ok_or_else(invalid)?
        }
    };
    let data = buffer_data(rt, buffer)?;
    if data.is_detached() {
        return Err(detached_error(name));
    }
    let max = if preserve_resizability {
        data.max_byte_length()
    } else {
        None
    };
    if new_length > MAX_BYTE_LENGTH {
        return Err(invalid());
    }
    if max.is_some_and(|max| new_length > max as u64) {
        return Err(VmError::range_error(INVALID_LENGTH));
    }
    let len = usize::try_from(new_length).map_err(|_| VmError::range_error(ALLOCATION_FAILED))?;
    let old = data.byte_length();
    if len > old && !rt.heap.reserve_buffer(len - old, &rt.vm) {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::ArrayBufferPrototype)?;
    let Some(bytes) = rt.heap.take_buffer_bytes(buffer, len)? else {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    };
    let target = rt.heap.new_array_buffer(Some(proto), bytes, max)?;
    if len != old {
        rt.charge_bytes(len)?;
    }
    Ok(NativeReturn::Value(target.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeConfig;

    #[test]
    fn index_values() {
        assert_eq!(index_value(0.0), Some(0));
        assert_eq!(index_value(MAX_SAFE_INTEGER), Some((1 << 53) - 1));
        assert_eq!(index_value(MAX_SAFE_INTEGER + 1.0), None);
        assert_eq!(index_value(-1.0), None);
        assert_eq!(index_value(f64::INFINITY), None);
    }

    /// The host operation detaches once and rejects other values.
    #[test]
    fn the_host_detaches_buffers() {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        let buffer = rt
            .eval("globalThis.b = new ArrayBuffer(8); b")
            .expect("runs");
        rt.scoped(|rt| rt.detach_array_buffer(buffer))
            .expect("detaches");
        rt.scoped(|rt| rt.detach_array_buffer(buffer))
            .expect("detaches again");
        let value = rt.eval("b.detached + ':' + b.byteLength").expect("runs");
        assert_eq!(rt.display(value), "true:0");
        assert!(
            rt.scoped(|rt| rt.detach_array_buffer(Value::Int(1)))
                .is_err()
        );
        let other = rt.eval("({})").expect("runs");
        assert!(rt.scoped(|rt| rt.detach_array_buffer(other)).is_err());
    }
}
