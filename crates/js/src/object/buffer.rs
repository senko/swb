//! The data of `ArrayBuffer` and `DataView` objects (ECMA-262 §25.1 and
//! §25.3, <https://tc39.es/ecma262/#sec-arraybuffer-objects>), the views on
//! a buffer, and the element types of their reads and writes (Table 71,
//! `RawBytesToNumeric` §25.1.3.14, `NumericToRawBytes` §25.1.3.17). The
//! built-in functions are in `builtins/array_buffer.rs` and
//! `builtins/data_view.rs`.
//!
//! Memory (ADR 0026 section 15, M7 11a): the bytes of a buffer are a Rust
//! vector that the buffer object owns, outside the arenas; the collector
//! drops it with the object. The heap counts the capacity of the vector
//! like any other buffer of a live thing, so it counts in the heap limit
//! and in the collection threshold. The vector is allocated fallibly
//! (`try_reserve_exact`), after a reservation ([`Heap::reserve_buffer`]),
//! and zero-filled. Detaching drops the vector (or hands it to the new
//! buffer of `transfer`) and takes its bytes out of the estimate at once.
//!
//! A view (a `DataView` now; the typed arrays of feature 11b) is a
//! [`BufferView`]: the buffer, the byte offset and a fixed byte length or
//! none for a view that tracks the length of a resizable buffer.
//! [`Heap::view_range`] gives the bytes that a view covers, or `None` when
//! the buffer is detached or the view is out of bounds.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::{Object, ObjectKind};

/// The longest byte length of a buffer (V8's limit, measured in Chromium
/// 148: `new ArrayBuffer(2 ** 35 - 1)` fails to allocate, `2 ** 35` is an
/// invalid length).
pub(crate) const MAX_BYTE_LENGTH: u64 = (1 << 35) - 1;

/// The largest `maxByteLength` of a resizable buffer (V8's limit,
/// measured: 2^35 − 4096). swb does not reserve it: a resizable buffer
/// allocates only its current length.
pub(crate) const MAX_RESIZABLE_BYTE_LENGTH: u64 = (1 << 35) - 4096;

/// The internal slots of an `ArrayBuffer`: `[[ArrayBufferData]]` with its
/// `[[ArrayBufferByteLength]]` (the length of the vector; `None` when the
/// buffer is detached) and `[[ArrayBufferMaxByteLength]]` of a resizable
/// buffer. `[[ArrayBufferDetachKey]]` is always undefined (no host sets
/// one).
#[derive(Debug)]
pub struct ArrayBufferData {
    bytes: Option<Vec<u8>>,
    max_byte_length: Option<usize>,
}

impl ArrayBufferData {
    /// The byte length; 0 for a detached buffer.
    pub(crate) fn byte_length(&self) -> usize {
        self.bytes.as_ref().map_or(0, Vec::len)
    }

    /// `IsDetachedBuffer` (§25.1.3.4).
    pub(crate) fn is_detached(&self) -> bool {
        self.bytes.is_none()
    }

    /// `[[ArrayBufferMaxByteLength]]`; `None` for a fixed-length buffer
    /// (`IsFixedLengthArrayBuffer`, §25.1.3.9). A detached resizable buffer
    /// keeps it.
    pub(crate) fn max_byte_length(&self) -> Option<usize> {
        self.max_byte_length
    }

    /// The bytes; `None` when the buffer is detached.
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    /// The bytes that the data owns (the capacity of the vector).
    pub(crate) fn heap_size(&self) -> usize {
        size_of::<ArrayBufferData>() + self.bytes.as_ref().map_or(0, Vec::capacity)
    }
}

/// `[[ViewedArrayBuffer]]`, `[[ByteOffset]]` and `[[ByteLength]]` of a view
/// (§25.3, and the typed arrays of §23.2). `byte_length` is `None` for
/// `auto`: the view tracks the length of a resizable buffer.
#[derive(Clone, Copy, Debug)]
pub struct BufferView {
    /// The `ArrayBuffer`.
    pub(crate) buffer: Gc<Object>,
    /// The offset of the first byte in the buffer.
    pub(crate) byte_offset: usize,
    /// The fixed length, or `None` (auto).
    pub(crate) byte_length: Option<usize>,
}

/// A zero-filled vector of `len` bytes, or `None` when the allocator
/// cannot give the memory (never an abort).
pub(crate) fn zeroed_bytes(len: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    Some(bytes)
}

/// Changes the length of `bytes` to `len`: zero-fills new bytes (fallible)
/// and gives back the capacity of a vector that shrinks to less than half
/// of it. Returns `false`, with `bytes` unchanged, when the memory is not
/// available.
pub(crate) fn set_byte_length(bytes: &mut Vec<u8>, len: usize) -> bool {
    if len > bytes.len() {
        if bytes.try_reserve_exact(len - bytes.len()).is_err() {
            return false;
        }
        bytes.resize(len, 0);
    } else {
        bytes.truncate(len);
        if len < bytes.capacity() / 2 {
            bytes.shrink_to_fit();
        }
    }
    true
}

/// Why a view access failed (§25.3.1.5 `GetViewValue`, §25.3.1.6
/// `SetViewValue`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewError {
    /// The buffer is detached or the view is out of bounds (a
    /// `TypeError`).
    OutOfBounds,
    /// The element does not fit in the view (a `RangeError`).
    Index,
}

impl Heap {
    /// Makes room for `bytes` bytes of a buffer, as [`Heap::reserve_live`]
    /// does (it can collect), but answers `false` instead of ending the
    /// script when they do not fit under 90 % of the heap limit:
    /// `CreateByteDataBlock` throws a `RangeError` (§6.2.9.1), as V8 does
    /// ("Array buffer allocation failed"). With the full limit, a script
    /// that keeps buffers between 90 % and 100 % of it would end at the
    /// next collection instead.
    pub(crate) fn reserve_buffer(&mut self, bytes: usize, roots: &dyn RootSource) -> bool {
        self.reserve_live(bytes, roots).is_ok()
    }

    /// A new `ArrayBuffer` object with these bytes (charged with the
    /// object) and, for a resizable buffer, its maximum length.
    pub(crate) fn new_array_buffer(
        &mut self,
        proto: Option<Gc<Object>>,
        bytes: Vec<u8>,
        max_byte_length: Option<usize>,
    ) -> Result<Gc<Object>> {
        let data = Box::new(ArrayBufferData {
            bytes: Some(bytes),
            max_byte_length,
        });
        self.new_object_with_kind(proto, ObjectKind::ArrayBuffer(data))
    }

    /// The data of an `ArrayBuffer`, or `None` for another object.
    pub(crate) fn array_buffer(&self, object: Gc<Object>) -> Result<Option<&ArrayBufferData>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::ArrayBuffer(data) => Some(data),
            _ => None,
        })
    }

    /// The data of an `ArrayBuffer` for writing; an error for another
    /// object.
    fn array_buffer_mut(&mut self, object: Gc<Object>) -> Result<&mut ArrayBufferData> {
        match &mut self.object_mut(object)?.kind {
            ObjectKind::ArrayBuffer(data) => Ok(data),
            _ => Err(Error::invariant("a buffer operation on another object")),
        }
    }

    /// `DetachArrayBuffer` (§25.1.3.5): the buffer loses its bytes, which
    /// are returned (`None` if it was detached already) and taken out of
    /// the heap estimate. An error for another object.
    pub(crate) fn detach_array_buffer(&mut self, buffer: Gc<Object>) -> Result<Option<Vec<u8>>> {
        let bytes = self.array_buffer_mut(buffer)?.bytes.take();
        if let Some(bytes) = &bytes {
            self.release_bytes(bytes.capacity());
        }
        Ok(bytes)
    }

    /// Sets the byte length of a resizable buffer that is not detached
    /// (the steps of `ArrayBuffer.prototype.resize` after its checks). The
    /// caller reserved the growth. Returns `false`, with the buffer
    /// unchanged, when the memory is not available.
    pub(crate) fn resize_array_buffer(&mut self, buffer: Gc<Object>, len: usize) -> Result<bool> {
        let data = self.array_buffer_mut(buffer)?;
        let Some(bytes) = data.bytes.as_mut() else {
            return Err(Error::invariant("a resize of a detached buffer"));
        };
        let before = bytes.capacity();
        if !set_byte_length(bytes, len) {
            return Ok(false);
        }
        let after = bytes.capacity();
        if after >= before {
            self.charge(after - before);
        } else {
            self.release_bytes(before - after);
        }
        Ok(true)
    }

    /// The bytes of a buffer that is not detached, set to `len` bytes, for
    /// the new buffer of `transfer` (§25.1.3.3): the buffer is detached.
    /// `None`, with the buffer unchanged, when the memory is not available
    /// (the caller reserved the growth).
    pub(crate) fn take_buffer_bytes(
        &mut self,
        buffer: Gc<Object>,
        len: usize,
    ) -> Result<Option<Vec<u8>>> {
        let data = self.array_buffer_mut(buffer)?;
        let mut bytes = data
            .bytes
            .take()
            .ok_or(Error::invariant("a transfer of a detached buffer"))?;
        let before = bytes.capacity();
        if !set_byte_length(&mut bytes, len) {
            data.bytes = Some(bytes);
            return Ok(None);
        }
        self.release_bytes(before);
        Ok(Some(bytes))
    }

    /// A new vector of `len` bytes: those of `source` from `from` as far as
    /// it has them, then zeros (the default case of `slice`, after the
    /// caller's reservation). `None` when the memory is not available.
    pub(crate) fn copy_of_buffer_bytes(
        &self,
        source: Gc<Object>,
        from: usize,
        len: usize,
    ) -> Result<Option<Vec<u8>>> {
        let bytes = self
            .array_buffer(source)?
            .and_then(ArrayBufferData::bytes)
            .ok_or(Error::invariant("a copy of a detached buffer"))?;
        let part = bytes.get(from..).unwrap_or_default();
        let part = part.get(..len).unwrap_or(part);
        let mut copy = Vec::new();
        if copy.try_reserve_exact(len).is_err() {
            return Ok(None);
        }
        copy.extend_from_slice(part);
        copy.resize(len, 0);
        Ok(Some(copy))
    }

    /// Copies `count` bytes from `source` at `from` to the start of
    /// `target` (both buffers live and not detached, the ranges inside
    /// them; the steps of `slice`). The target's vector is moved out
    /// while it is written, so nothing is allocated.
    pub(crate) fn copy_buffer_bytes(
        &mut self,
        target: Gc<Object>,
        source: Gc<Object>,
        from: usize,
        count: usize,
    ) -> Result<()> {
        let error = Error::invariant("a buffer copy outside the buffers");
        let mut to = self.array_buffer_mut(target)?.bytes.take().ok_or(error)?;
        let copied = (|| {
            let bytes = self.array_buffer(source)?.and_then(ArrayBufferData::bytes);
            let part = bytes
                .and_then(|bytes| bytes.get(from..from.checked_add(count)?))
                .ok_or(error)?;
            to.get_mut(..count).ok_or(error)?.copy_from_slice(part);
            Ok(())
        })();
        self.array_buffer_mut(target)?.bytes = Some(to);
        copied
    }

    // --- Views ---

    /// A new `DataView` object.
    pub(crate) fn new_data_view(
        &mut self,
        proto: Option<Gc<Object>>,
        view: BufferView,
    ) -> Result<Gc<Object>> {
        self.object(view.buffer)?;
        self.new_object_with_kind(proto, ObjectKind::DataView(Box::new(view)))
    }

    /// The view record of a `DataView`, or `None` for another object.
    pub(crate) fn data_view(&self, object: Gc<Object>) -> Result<Option<BufferView>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::DataView(view) => Some(**view),
            _ => None,
        })
    }

    /// The bytes of the buffer that a view covers, as (start, length):
    /// `IsViewOutOfBounds` (§25.3.1.4) and `GetViewByteLength` (§25.3.1.3)
    /// with a witness record of the current length. `None` when the buffer
    /// is detached or the view is out of bounds.
    pub(crate) fn view_range(&self, view: &BufferView) -> Result<Option<(usize, usize)>> {
        let data = self
            .array_buffer(view.buffer)?
            .ok_or(Error::invariant("a view of another object"))?;
        if data.is_detached() {
            return Ok(None);
        }
        let buffer_length = data.byte_length();
        let start = view.byte_offset;
        if start > buffer_length {
            return Ok(None);
        }
        let length = match view.byte_length {
            None => buffer_length - start,
            Some(length) => {
                if length > buffer_length - start {
                    return Ok(None);
                }
                length
            }
        };
        Ok(Some((start, length)))
    }

    /// The bytes of an element of `size` bytes at `index` of a view (the
    /// checks of `GetViewValue` after the conversions).
    fn view_element(
        &self,
        view: &BufferView,
        index: u64,
        size: usize,
    ) -> Result<std::result::Result<std::ops::Range<usize>, ViewError>> {
        let Some((start, length)) = self.view_range(view)? else {
            return Ok(Err(ViewError::OutOfBounds));
        };
        match usize::try_from(index) {
            Ok(index) if index <= length && size <= length - index => {
                Ok(Ok(start + index..start + index + size))
            }
            _ => Ok(Err(ViewError::Index)),
        }
    }

    /// `GetViewValue` after the conversions (§25.3.1.5 steps 5 to 11):
    /// the element of type `ty` at `index` of the view.
    pub(crate) fn view_get(
        &self,
        view: &BufferView,
        index: u64,
        ty: ElementType,
        little_endian: bool,
    ) -> Result<std::result::Result<RawValue, ViewError>> {
        let range = match self.view_element(view, index, ty.size())? {
            Ok(range) => range,
            Err(error) => return Ok(Err(error)),
        };
        let bytes = self
            .array_buffer(view.buffer)?
            .and_then(ArrayBufferData::bytes)
            .and_then(|bytes| bytes.get(range))
            .ok_or(Error::invariant("a view element outside its buffer"))?;
        Ok(Ok(decode(ty, bytes, little_endian)))
    }

    /// `SetViewValue` after the conversions (§25.3.1.6 steps 6 to 12): writes
    /// the element of type `ty` at `index` of the view.
    pub(crate) fn view_set(
        &mut self,
        view: &BufferView,
        index: u64,
        ty: ElementType,
        value: Element,
        little_endian: bool,
    ) -> Result<std::result::Result<(), ViewError>> {
        let range = match self.view_element(view, index, ty.size())? {
            Ok(range) => range,
            Err(error) => return Ok(Err(error)),
        };
        let raw = encode(ty, value, little_endian);
        let target = self
            .array_buffer_mut(view.buffer)?
            .bytes
            .as_mut()
            .and_then(|bytes| bytes.get_mut(range))
            .ok_or(Error::invariant("a view element outside its buffer"))?;
        let source = raw
            .get(..ty.size())
            .ok_or(Error::invariant("an element longer than 8 bytes"))?;
        target.copy_from_slice(source);
        Ok(Ok(()))
    }
}

// --- Element types ---

/// The element types of Table 71 (§23.2): the types of `DataView`'s
/// methods and of the typed arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementType {
    Int8,
    Uint8,
    /// Used by `Uint8ClampedArray` (feature 11b); `DataView` has no such
    /// method.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the Uint8ClampedArray of feature 11b uses it")
    )]
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}

impl ElementType {
    /// The Element Size of Table 71, in bytes.
    pub(crate) const fn size(self) -> usize {
        match self {
            ElementType::Int8 | ElementType::Uint8 | ElementType::Uint8Clamped => 1,
            ElementType::Int16 | ElementType::Uint16 | ElementType::Float16 => 2,
            ElementType::Int32 | ElementType::Uint32 | ElementType::Float32 => 4,
            ElementType::BigInt64 | ElementType::BigUint64 | ElementType::Float64 => 8,
        }
    }

    /// `IsBigIntElementType` (§25.1.3.12).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the typed arrays of feature 11b use it")
    )]
    pub(crate) const fn is_bigint(self) -> bool {
        matches!(self, ElementType::BigInt64 | ElementType::BigUint64)
    }

    /// The name in the method names of `DataView` (`getInt8`) and in the
    /// typed array names (`Int8Array`).
    pub(crate) const fn name(self) -> &'static str {
        match self {
            ElementType::Int8 => "Int8",
            ElementType::Uint8 => "Uint8",
            ElementType::Uint8Clamped => "Uint8Clamped",
            ElementType::Int16 => "Int16",
            ElementType::Uint16 => "Uint16",
            ElementType::Int32 => "Int32",
            ElementType::Uint32 => "Uint32",
            ElementType::BigInt64 => "BigInt64",
            ElementType::BigUint64 => "BigUint64",
            ElementType::Float16 => "Float16",
            ElementType::Float32 => "Float32",
            ElementType::Float64 => "Float64",
        }
    }
}

/// A value to store: a Number (after `ToNumber`) for the Number types, the
/// 64 bits of a `BigInt` (after `ToBigInt64` or `ToBigUint64`, which give
/// the same bits) for the `BigInt` types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Element {
    Number(f64),
    Bits(u64),
}

/// A value read from the bytes: an integer that fits in 32 bits (the
/// small integer types, without an allocation), another Number, or a
/// 64-bit `BigInt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RawValue {
    Int(i32),
    Number(f64),
    BigInt64(i64),
    BigUint64(u64),
}

/// `ToUint8Clamp` (§7.1.12): round to nearest, ties to even, in 0 to 255.
fn to_uint8_clamp(n: f64) -> u8 {
    if n.is_nan() {
        return 0;
    }
    n.clamp(0.0, 255.0).round_ties_even() as u8
}

/// `NumericToRawBytes` (§25.1.3.17): the bytes of `value` as type `ty`,
/// in the first `ty.size()` entries. The integer conversions
/// (`ToInt8` … `ToUint32`, §7.1.6 to §7.1.11) reduce modulo 2^32 and then
/// keep the low bits. NaN: see `js-text/src/number/binary16.rs` for
/// binary16; binary32 and binary64 keep the bits of the double (the
/// conversion of the hardware), as Chromium 148 does (measured).
pub(crate) fn encode(ty: ElementType, value: Element, little_endian: bool) -> [u8; 8] {
    let n = match value {
        Element::Number(n) => n,
        Element::Bits(bits) => return order(bits.to_le_bytes(), 8, little_endian),
    };
    let int = || crate::vm::convert::to_uint32(n);
    let (raw, size): (u64, usize) = match ty {
        ElementType::Int8 | ElementType::Uint8 => (u64::from(int() as u8), 1),
        ElementType::Uint8Clamped => (u64::from(to_uint8_clamp(n)), 1),
        ElementType::Int16 | ElementType::Uint16 => (u64::from(int() as u16), 2),
        ElementType::Int32 | ElementType::Uint32 => (u64::from(int()), 4),
        ElementType::Float16 => (u64::from(swb_js_text::number::f16_to_bits(n)), 2),
        ElementType::Float32 => (u64::from((n as f32).to_bits()), 4),
        // A Number for a BigInt type does not happen (the caller converts
        // with ToBigInt); the bits of the double keep it defined.
        ElementType::Float64 | ElementType::BigInt64 | ElementType::BigUint64 => (n.to_bits(), 8),
    };
    order(raw.to_le_bytes(), size, little_endian)
}

/// The first `size` bytes of `bytes` (little endian), reversed for big
/// endian.
fn order(mut bytes: [u8; 8], size: usize, little_endian: bool) -> [u8; 8] {
    if !little_endian && let Some(part) = bytes.get_mut(..size) {
        part.reverse();
    }
    bytes
}

/// `RawBytesToNumeric` (§25.1.3.14): the value of the element bytes
/// (`bytes.len()` is the element size).
pub(crate) fn decode(ty: ElementType, bytes: &[u8], little_endian: bool) -> RawValue {
    let mut raw = [0u8; 8];
    for (i, &byte) in bytes.iter().take(8).enumerate() {
        let at = if little_endian {
            i
        } else {
            bytes.len() - 1 - i
        };
        if let Some(slot) = raw.get_mut(at) {
            *slot = byte;
        }
    }
    let bits = u64::from_le_bytes(raw);
    match ty {
        ElementType::Int8 => RawValue::Int(i32::from(bits as u8 as i8)),
        ElementType::Uint8 | ElementType::Uint8Clamped => RawValue::Int(i32::from(bits as u8)),
        ElementType::Int16 => RawValue::Int(i32::from(bits as u16 as i16)),
        ElementType::Uint16 => RawValue::Int(i32::from(bits as u16)),
        ElementType::Int32 => RawValue::Int(bits as u32 as i32),
        ElementType::Uint32 => RawValue::Number(f64::from(bits as u32)),
        ElementType::Float16 => RawValue::Number(swb_js_text::number::f16_from_bits(bits as u16)),
        ElementType::Float32 => RawValue::Number(f64::from(f32::from_bits(bits as u32))),
        ElementType::Float64 => RawValue::Number(f64::from_bits(bits)),
        ElementType::BigInt64 => RawValue::BigInt64(bits as i64),
        ElementType::BigUint64 => RawValue::BigUint64(bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_wrap_and_keep_their_byte_order() {
        let bytes = encode(ElementType::Int16, Element::Number(-2.0), false);
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        let bytes = encode(ElementType::Uint32, Element::Number(4_294_967_297.0), true);
        assert_eq!(&bytes[..4], &[1, 0, 0, 0]);
        let bytes = encode(ElementType::Int8, Element::Number(-129.0), true);
        assert_eq!(bytes[0], 0x7F);
        let bytes = encode(ElementType::Uint8, Element::Number(f64::INFINITY), true);
        assert_eq!(bytes[0], 0);
        assert_eq!(
            decode(ElementType::Int32, &[0x80, 0, 0, 0], false),
            RawValue::Int(i32::MIN)
        );
        assert_eq!(
            decode(ElementType::Uint32, &[0xFF; 4], true),
            RawValue::Number(4_294_967_295.0)
        );
        assert_eq!(
            decode(ElementType::Int16, &[0x80, 0x01], true),
            RawValue::Int(384)
        );
    }

    #[test]
    fn clamping_rounds_ties_to_even() {
        let clamp = |n| encode(ElementType::Uint8Clamped, Element::Number(n), true)[0];
        assert_eq!(clamp(0.5), 0);
        assert_eq!(clamp(1.5), 2);
        assert_eq!(clamp(2.5), 2);
        assert_eq!(clamp(254.5), 254);
        assert_eq!(clamp(300.0), 255);
        assert_eq!(clamp(-3.0), 0);
        assert_eq!(clamp(f64::NAN), 0);
        assert_eq!(clamp(f64::INFINITY), 255);
    }

    #[test]
    fn floats_and_bigints() {
        let bytes = encode(ElementType::Float32, Element::Number(1.5), false);
        assert_eq!(&bytes[..4], &[0x3F, 0xC0, 0, 0]);
        assert_eq!(
            decode(ElementType::Float64, &0.1f64.to_be_bytes(), false),
            RawValue::Number(0.1)
        );
        let bytes = encode(ElementType::BigInt64, Element::Bits(u64::MAX), true);
        assert_eq!(bytes, [0xFF; 8]);
        assert_eq!(
            decode(ElementType::BigInt64, &[0xFF; 8], true),
            RawValue::BigInt64(-1)
        );
        assert_eq!(
            decode(ElementType::Float16, &[0x3C, 0x00], false),
            RawValue::Number(1.0)
        );
        assert!(ElementType::BigUint64.is_bigint() && !ElementType::Float64.is_bigint());
    }

    #[test]
    fn length_changes_keep_the_bytes() {
        let mut bytes = zeroed_bytes(4).expect("4 bytes");
        bytes[0] = 7;
        assert!(set_byte_length(&mut bytes, 100));
        assert_eq!((bytes.len(), bytes[0], bytes[99]), (100, 7, 0));
        assert!(set_byte_length(&mut bytes, 1));
        assert_eq!(bytes, [7]);
        assert!(bytes.capacity() < 100);
        assert!(zeroed_bytes(usize::MAX).is_none());
    }
}
