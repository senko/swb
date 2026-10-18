//! The data of typed arrays (ECMA-262 §23.2) and the parts of the
//! `TypedArray` exotic object (§10.4.5,
//! <https://tc39.es/ecma262/#sec-typedarray-exotic-objects>) that run no
//! script code: `CanonicalNumericIndexString`, `IsValidIntegerIndex`,
//! `TypedArrayLength` with `IsTypedArrayOutOfBounds`, and the element reads
//! and writes after the conversions. `ops.rs` dispatches the internal
//! methods here; the runtime adds the conversions that can run script
//! code (`vm/typed_array.rs`), the built-in functions are in
//! `builtins/typed_array*.rs`.
//!
//! A typed array is a view (`buffer.rs`): the buffer, the byte offset, and
//! a fixed byte length or none for an array that tracks the length of a
//! resizable buffer (`[[ByteLength]]` and `[[ArrayLength]]` are both
//! `auto` then), plus its element type. Its elements are not stored in the
//! object: every canonical numeric string key (an array index, but also
//! "-0", "1.5", "4294967295" or "NaN") is an element or no property, never
//! an ordinary property, so the object's own element storage stays empty.
//! The elements are little endian (the agent's `[[LittleEndian]]`; V8 on
//! x86 and ARM).

use swb_js_text::Str16;

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap};
use crate::object::buffer::{ArrayBufferData, BufferView, Element, ElementType, RawValue};
use crate::object::buffer::{decode, decode_bits, element_bits};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::string::PropertyKey;
use crate::value::Value;

/// The byte order of the elements.
const LITTLE_ENDIAN: bool = true;

/// The internal slots of a typed array (§23.2.7):
/// `[[ViewedArrayBuffer]]`, `[[ByteOffset]]`, `[[ByteLength]]` (with
/// `[[ArrayLength]]`, the byte length divided by the element size; both
/// `auto` when `view.byte_length` is `None`) and the element type, which
/// gives `[[TypedArrayName]]` and `[[ContentType]]`. The fields never
/// change after the array is made.
#[derive(Clone, Copy, Debug)]
pub struct TypedArrayData {
    /// The buffer and the range.
    pub(crate) view: BufferView,
    /// The element type.
    pub(crate) ty: ElementType,
}

impl TypedArrayData {
    /// The length for a buffer of `buffer_len` bytes, or `None` when the
    /// array is out of bounds (`IsTypedArrayOutOfBounds` and `TypedArrayLength`,
    /// §10.4.5).
    pub(super) fn length_in(&self, buffer_len: usize) -> Option<usize> {
        let offset = self.view.byte_offset;
        let room = buffer_len.checked_sub(offset)?;
        match self.view.byte_length {
            None => Some(room / self.ty.size()),
            Some(bytes) => (bytes <= room).then(|| bytes / self.ty.size()),
        }
    }
}

/// `CanonicalNumericIndexString` (§7.1.21): the Number that `text` is the
/// canonical text of (`ToString(ToNumber(text))` is `text`), or −0 for
/// "-0"; `None` for other strings. Only strings that can start a Number's
/// text are parsed, and none longer than the longest such text.
pub(crate) fn canonical_numeric_index(text: Str16<'_>) -> Option<f64> {
    const LONGEST: usize = 32;
    let first = text.units().next()?;
    let starts_number = (u16::from(b'0')..=u16::from(b'9')).contains(&first)
        || first == u16::from(b'-')
        || first == u16::from(b'I')
        || first == u16::from(b'N');
    if !starts_number || text.len() > LONGEST {
        return None;
    }
    if text.eq_str("-0") {
        return Some(-0.0);
    }
    let number = swb_js_text::number::string_to_number(text);
    text.eq_str(&swb_js_text::number::number_to_string(number))
        .then_some(number)
}

/// The element index of a numeric index for a typed array of `length`
/// elements (`IsValidIntegerIndex` after the detach and bounds checks,
/// <https://tc39.es/ecma262/#sec-isvalidintegerindex>): an integer from
/// +0 to `length` − 1 (−0 is not one).
fn element_index(index: f64, length: usize) -> Option<usize> {
    if index.fract() != 0.0 || (index == 0.0 && index.is_sign_negative()) || index < 0.0 {
        return None;
    }
    // Below 2^53 the conversion is exact; larger values are past any
    // length.
    let index = index as u64;
    usize::try_from(index).ok().filter(|&index| index < length)
}

/// The bytes of element `index` of a typed array within a buffer's bytes,
/// or `None` when the array is out of bounds or the index is not below its
/// length (`TypedArrayLength` without the division: the element must end
/// within the array's bytes).
fn element_range(
    data: &TypedArrayData,
    buffer_len: usize,
    index: usize,
) -> Option<std::ops::Range<usize>> {
    let size = data.ty.size();
    let start = index.checked_mul(size)?;
    let end = start.checked_add(size)?;
    let room = buffer_len.checked_sub(data.view.byte_offset)?;
    let fits = match data.view.byte_length {
        Some(bytes) => bytes <= room && end <= bytes,
        None => end <= room,
    };
    let offset = data.view.byte_offset;
    fits.then(|| offset + start..offset + end)
}

impl Heap {
    /// A new typed array object. The buffer must be live.
    pub(crate) fn new_typed_array(
        &mut self,
        proto: Option<Gc<Object>>,
        data: TypedArrayData,
    ) -> Result<Gc<Object>> {
        self.object(data.view.buffer)?;
        self.new_object_with_kind(proto, ObjectKind::TypedArray(Box::new(data)))
    }

    /// The slots of a typed array, or `None` for another object.
    pub(crate) fn typed_array(&self, object: Gc<Object>) -> Result<Option<TypedArrayData>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::TypedArray(data) => Some(**data),
            _ => None,
        })
    }

    /// The bytes of the buffer of a typed array; `None` when detached.
    pub(super) fn typed_bytes(&self, data: &TypedArrayData) -> Result<Option<&[u8]>> {
        Ok(self
            .array_buffer(data.view.buffer)?
            .ok_or(Error::invariant("a typed array of another object"))?
            .bytes())
    }

    /// `TypedArrayLength` of a fresh witness record, or `None` when the array
    /// is out of bounds or its buffer detached (`IsTypedArrayOutOfBounds`,
    /// <https://tc39.es/ecma262/#sec-istypedarrayoutofbounds>).
    pub(crate) fn typed_length(&self, data: &TypedArrayData) -> Result<Option<usize>> {
        Ok(self
            .typed_bytes(data)?
            .and_then(|bytes| data.length_in(bytes.len())))
    }

    /// `IsTypedArrayFixedLength`
    /// (<https://tc39.es/ecma262/#sec-istypedarrayfixedlength>, used by
    /// `[[PreventExtensions]]`): a fixed `[[ArrayLength]]` on a buffer that
    /// cannot be resized.
    pub(crate) fn typed_fixed_length(&self, data: &TypedArrayData) -> Result<bool> {
        let buffer = self
            .array_buffer(data.view.buffer)?
            .ok_or(Error::invariant("a typed array of another object"))?;
        Ok(data.view.byte_length.is_some() && buffer.max_byte_length().is_none())
    }

    /// `IsValidIntegerIndex(O, index)`: the element index, or `None`.
    pub(crate) fn valid_index(&self, data: &TypedArrayData, index: f64) -> Result<Option<usize>> {
        Ok(self
            .typed_length(data)?
            .and_then(|length| element_index(index, length)))
    }

    /// `TypedArrayGetElement`
    /// (<https://tc39.es/ecma262/#sec-typedarraygetelement>) for an element index: the raw
    /// value, or `None` when the index is not valid.
    pub(crate) fn typed_get(
        &self,
        data: &TypedArrayData,
        index: usize,
    ) -> Result<Option<RawValue>> {
        let Some(bytes) = self.typed_bytes(data)? else {
            return Ok(None);
        };
        let Some(range) = element_range(data, bytes.len(), index) else {
            return Ok(None);
        };
        let raw = bytes
            .get(range)
            .ok_or(Error::invariant("a typed array element outside its buffer"))?;
        let bits = load(raw).ok_or(Error::invariant("an element of another size"))?;
        Ok(Some(decode_bits(data.ty, bits)))
    }

    /// The end of `TypedArraySetElement`
    /// (<https://tc39.es/ecma262/#sec-typedarraysetelement>) after the
    /// conversion: writes `value` to element `index` if it is valid.
    /// Returns whether it wrote.
    pub(crate) fn typed_set(
        &mut self,
        data: &TypedArrayData,
        index: usize,
        value: Element,
    ) -> Result<bool> {
        let bits = element_bits(data.ty, value);
        let buffer = self.array_buffer_bytes_mut(data.view.buffer)?;
        let Some(bytes) = buffer else {
            return Ok(false);
        };
        let Some(range) = element_range(data, bytes.len(), index) else {
            return Ok(false);
        };
        let target = bytes
            .get_mut(range)
            .ok_or(Error::invariant("a typed array element outside its buffer"))?;
        if !store(target, bits) {
            return Err(Error::invariant("an element of another size"));
        }
        Ok(true)
    }

    /// The numeric index of a property key for the internal methods of a
    /// typed array: an array index, or the Number of a canonical numeric
    /// string; `None` for other keys (ordinary properties).
    pub(crate) fn numeric_index(&self, key: PropertyKey) -> Result<Option<f64>> {
        match key {
            PropertyKey::Index(index) => Ok(Some(f64::from(index))),
            PropertyKey::String(name) => Ok(canonical_numeric_index(self.text(name)?)),
            PropertyKey::Symbol(_) => Ok(None),
        }
    }

    /// The conversion of `value` to an element of type `ty` when it runs no
    /// script code and cannot fail (a Number for a Number type, a `BigInt`
    /// for a `BigInt` type); `None` when the runtime must convert it.
    pub(crate) fn plain_element(&self, ty: ElementType, value: Value) -> Result<Option<Element>> {
        Ok(match (ty.is_bigint(), value) {
            (false, Value::Int(int)) => Some(Element::Int(int)),
            (false, Value::Double(number)) => Some(Element::Number(number)),
            (true, Value::BigInt(bigint)) => {
                Some(Element::Bits(self.bigint(bigint)?.to_u64_wrapping()))
            }
            _ => None,
        })
    }

    /// The own property of a typed array for a numeric index (`[[GetOwnProperty]]`,
    /// §10.4.5): a writable, enumerable, configurable data
    /// property for a valid index, else none. The element of a `BigInt`
    /// array is a new `BigInt`, which the heap cannot allocate here: the
    /// runtime reads those (`Runtime::own_property`), so this is an
    /// internal error.
    pub(crate) fn typed_own_property(
        &self,
        data: &TypedArrayData,
        index: f64,
    ) -> Result<Option<Property>> {
        let Some(index) = self.valid_index(data, index)? else {
            return Ok(None);
        };
        let value = match self.typed_get(data, index)? {
            Some(RawValue::Int(int)) => Value::Int(int),
            Some(RawValue::Number(number)) => Value::number(number),
            Some(RawValue::BigInt64(_) | RawValue::BigUint64(_)) => {
                return Err(Error::invariant("a heap read of a BigInt element"));
            }
            None => return Ok(None),
        };
        Ok(Some(Property::Data {
            value,
            writable: true,
            enumerable: true,
            configurable: true,
        }))
    }

    /// Steps 1.b.i to v of `[[DefineOwnProperty]]` (§10.4.5) for a
    /// numeric index: the element index if the definition can apply (a
    /// valid index and a descriptor that keeps the element a writable,
    /// enumerable, configurable data property), else `None` (the result is
    /// false).
    pub(crate) fn typed_define_target(
        &self,
        data: &TypedArrayData,
        index: f64,
        desc: &PropertyDescriptor,
    ) -> Result<Option<usize>> {
        let Some(index) = self.valid_index(data, index)? else {
            return Ok(None);
        };
        let refused = desc.configurable == Some(false)
            || desc.enumerable == Some(false)
            || desc.is_accessor()
            || desc.writable == Some(false);
        Ok((!refused).then_some(index))
    }

    /// `[[DefineOwnProperty]]` of a typed array for a numeric index with a
    /// value that needs no conversion by the runtime (see
    /// [`Heap::plain_element`]); other values are an internal error (the
    /// runtime converts them first, `Runtime::define_own_property`).
    pub(crate) fn typed_define_own_property(
        &mut self,
        data: &TypedArrayData,
        index: f64,
        desc: &PropertyDescriptor,
    ) -> Result<bool> {
        let Some(element_index) = self.typed_define_target(data, index, desc)? else {
            return Ok(false);
        };
        if let Some(value) = desc.value {
            let element = self.plain_element(data.ty, value)?.ok_or(Error::invariant(
                "a typed array element that needs a conversion",
            ))?;
            self.typed_set(data, element_index, element)?;
        }
        Ok(true)
    }

    /// The bytes of a buffer for writing; `None` when it is detached.
    pub(super) fn array_buffer_bytes_mut(
        &mut self,
        buffer: Gc<Object>,
    ) -> Result<Option<&mut [u8]>> {
        match &mut self.object_mut(buffer)?.kind {
            ObjectKind::ArrayBuffer(data) => Ok(data.bytes_mut()),
            _ => Err(Error::invariant("a typed array of another object")),
        }
    }

    /// Copies `count` bytes inside or between buffers (`set` and `slice` of
    /// typed arrays of one element type; `copy_within` like `memmove`
    /// when both are the same buffer). The ranges must be inside the
    /// buffers, unless `count` is 0 (a callback can have shrunk a buffer
    /// to nothing); nothing is allocated.
    pub(crate) fn copy_between_buffers(
        &mut self,
        target: (Gc<Object>, usize),
        source: (Gc<Object>, usize),
        count: usize,
    ) -> Result<()> {
        if count == 0 {
            return Ok(());
        }
        let error = Error::invariant("a typed array copy outside the buffers");
        let (to, from) = (target.1, source.1);
        let to_end = to.checked_add(count).ok_or(error)?;
        let from_end = from.checked_add(count).ok_or(error)?;
        if target.0 == source.0 {
            let bytes = self.array_buffer_bytes_mut(target.0)?.ok_or(error)?;
            if to_end > bytes.len() || from_end > bytes.len() {
                return Err(error);
            }
            bytes.copy_within(from..from_end, to);
            return Ok(());
        }
        let mut moved = match &mut self.object_mut(target.0)?.kind {
            ObjectKind::ArrayBuffer(data) => data.take_bytes().ok_or(error)?,
            _ => return Err(error),
        };
        let copied = (|| {
            let bytes = self
                .array_buffer(source.0)?
                .and_then(ArrayBufferData::bytes)
                .ok_or(error)?;
            let part = bytes.get(from..from_end).ok_or(error)?;
            moved
                .get_mut(to..to_end)
                .ok_or(error)?
                .copy_from_slice(part);
            Ok(())
        })();
        if let ObjectKind::ArrayBuffer(data) = &mut self.object_mut(target.0)?.kind {
            data.put_bytes(moved);
        }
        copied
    }

    /// Copies `count` bytes from `source` to `target` (buffer, byte index)
    /// one byte at a time from the first (`slice` of a typed array, whose
    /// specification copies forward: within one buffer, a later byte can
    /// read what an earlier step wrote). The ranges must be inside the
    /// buffers.
    pub(crate) fn copy_bytes_forward(
        &mut self,
        target: (Gc<Object>, usize),
        source: (Gc<Object>, usize),
        count: usize,
    ) -> Result<()> {
        if target.0 != source.0 || target.1 <= source.1 || target.1 >= source.1 + count {
            // No byte is read after it was written: a block copy is the same.
            return self.copy_between_buffers(target, source, count);
        }
        let error = Error::invariant("a typed array copy outside the buffers");
        let bytes = self.array_buffer_bytes_mut(target.0)?.ok_or(error)?;
        let (to, from) = (target.1, source.1);
        if to.checked_add(count).is_none_or(|end| end > bytes.len()) {
            return Err(error);
        }
        for i in 0..count {
            let byte = *bytes.get(from + i).ok_or(error)?;
            *bytes.get_mut(to + i).ok_or(error)? = byte;
        }
        Ok(())
    }

    /// A copy of `count` bytes of a buffer from `from` (for `set` from an
    /// overlapping typed array of another element type: the
    /// `CloneArrayBuffer` of `SetTypedArrayFromTypedArray`). The caller
    /// reserved the bytes. `None` when the allocator cannot give them.
    pub(crate) fn clone_buffer_bytes(
        &self,
        buffer: Gc<Object>,
        from: usize,
        count: usize,
    ) -> Result<Option<Vec<u8>>> {
        let bytes = self
            .array_buffer(buffer)?
            .and_then(ArrayBufferData::bytes)
            .ok_or(Error::invariant("a clone of a detached buffer"))?;
        let part = from
            .checked_add(count)
            .and_then(|end| bytes.get(from..end))
            .ok_or(Error::invariant("a clone outside the buffer"))?;
        let mut copy = Vec::new();
        if copy.try_reserve_exact(count).is_err() {
            return Ok(None);
        }
        copy.extend_from_slice(part);
        Ok(Some(copy))
    }
}

/// The value of an element read from raw bytes (`bytes` holds an element of
/// type `ty`, little endian), for loops over copies of a buffer.
pub(crate) fn decode_element(ty: ElementType, bytes: &[u8]) -> RawValue {
    decode(ty, bytes, LITTLE_ENDIAN)
}

/// The little-endian integer of the bytes of an element (1, 2, 4 or 8
/// bytes), read without a call of `memcpy`; `None` for another length.
pub(crate) fn load(bytes: &[u8]) -> Option<u64> {
    Some(match bytes.len() {
        1 => u64::from(*bytes.first()?),
        2 => u64::from(u16::from_le_bytes(bytes.try_into().ok()?)),
        4 => u64::from(u32::from_le_bytes(bytes.try_into().ok()?)),
        8 => u64::from_le_bytes(bytes.try_into().ok()?),
        _ => return None,
    })
}

/// Writes the low bytes of `bits` into the bytes of an element (1, 2, 4
/// or 8 bytes, little endian) without a call of `memcpy`; false for
/// another length.
pub(super) fn store(target: &mut [u8], bits: u64) -> bool {
    let bytes = bits.to_le_bytes();
    match target.len() {
        1 => match target.first_mut() {
            Some(byte) => *byte = bytes[0],
            None => return false,
        },
        2 => match <&mut [u8; 2]>::try_from(target) {
            Ok(part) => *part = [bytes[0], bytes[1]],
            Err(_) => return false,
        },
        4 => match <&mut [u8; 4]>::try_from(target) {
            Ok(part) => *part = [bytes[0], bytes[1], bytes[2], bytes[3]],
            Err(_) => return false,
        },
        8 => match <&mut [u8; 8]>::try_from(target) {
            Ok(part) => *part = bytes,
            Err(_) => return false,
        },
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_js_text::String16;

    fn numeric(text: &str) -> Option<f64> {
        canonical_numeric_index(String16::from(text).as_str16())
    }

    #[test]
    fn canonical_numeric_strings() {
        assert_eq!(numeric("0"), Some(0.0));
        assert_eq!(numeric("-0").map(f64::is_sign_negative), Some(true));
        assert_eq!(numeric("1.5"), Some(1.5));
        assert_eq!(numeric("1e+21"), Some(1e21));
        assert_eq!(numeric("-1"), Some(-1.0));
        assert_eq!(numeric("4294967295"), Some(4_294_967_295.0));
        assert_eq!(numeric("Infinity"), Some(f64::INFINITY));
        assert_eq!(numeric("-Infinity"), Some(f64::NEG_INFINITY));
        assert!(numeric("NaN").is_some_and(f64::is_nan));
        for text in [
            "01",
            "1.0",
            " 1",
            "0x1",
            "+1",
            "1e21",
            "+Infinity",
            "-NaN",
            "",
            "a",
            "0.0000001",
            "-",
            "1.",
        ] {
            assert_eq!(numeric(text), None, "{text}");
        }
        assert_eq!(numeric("1e-7"), Some(1e-7));
    }

    #[test]
    fn element_indices() {
        assert_eq!(element_index(0.0, 2), Some(0));
        assert_eq!(element_index(1.0, 2), Some(1));
        assert_eq!(element_index(2.0, 2), None);
        assert_eq!(element_index(-0.0, 2), None);
        assert_eq!(element_index(-1.0, 2), None);
        assert_eq!(element_index(0.5, 2), None);
        assert_eq!(element_index(f64::NAN, 2), None);
        assert_eq!(element_index(f64::INFINITY, 2), None);
        assert_eq!(element_index(9e15, usize::MAX), Some(9_000_000_000_000_000));
    }
}
