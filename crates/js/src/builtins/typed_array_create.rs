//! The creation of typed arrays (ECMA-262 §23.2.5,
//! <https://tc39.es/ecma262/#sec-typedarray-constructors>, and §23.2.4):
//! `AllocateTypedArray` with `AllocateTypedArrayBuffer`, the four
//! `InitializeTypedArrayFrom…` operations of the constructors,
//! `TypedArraySpeciesCreate`, `TypedArrayCreateFromConstructor` and
//! `ValidateTypedArray`, with V8's messages (measured in Chromium 148,
//! `tools/probes/js-typed.json`).
//!
//! The steps run in the specification's order, so the prototype getter of
//! `new.target` runs before the source is read. A new array gets its buffer
//! when it is made (the slots of `typed_array.rs` never change), and its
//! elements are written into that buffer, which no script code can reach
//! before the constructor returns; the conversions of the elements still
//! run one by one, in order. Byte lengths are checked against V8's limit
//! (2^35 − 1, "Invalid typed array length: N") before the allocation,
//! which is a `RangeError` at the heap limit ("Array buffer allocation
//! failed"). Loops over script-controlled sources take one step of the time
//! countdown per element; bulk copies charge one step per 128 bytes.
//!
//! `TypedArraySpeciesCreate` with the default constructor (a typed array
//! whose `constructor` and `@@species` are the built-in ones) makes the
//! array without the call of the constructor, which is not observable.

use crate::heap::Gc;
use crate::object::{
    BufferView, Element, ElementType, MAX_BYTE_LENGTH, Object, RawValue, TypedArrayData,
};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::number::number_to_string;
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

use super::array_buffer::{ALLOCATION_FAILED, index_value};

/// The elements that a conversion loop between typed arrays converts
/// between two charges of the time countdown.
pub(super) const CHARGE_CHUNK: usize = 1 << 14;

/// V8's message for a length that `ToIndex` rejects or that does not fit.
pub(super) fn invalid_length(length: f64) -> VmError {
    VmError::range_error(format!(
        "Invalid typed array length: {}",
        number_to_string(length)
    ))
}

/// V8's message for a byte offset that `ToIndex` rejects or that is past
/// the end of the buffer.
fn bad_offset(offset: f64) -> VmError {
    VmError::range_error(format!(
        "Start offset {} is outside the bounds of the buffer",
        number_to_string(offset)
    ))
}

/// The `TypeError` of a source that is detached or out of bounds, as V8
/// words it for `name` (`Construct`, `%TypedArray%.prototype.set`, …).
pub(super) fn out_of_bounds(name: &str) -> VmError {
    VmError::type_error(format!(
        "Cannot perform {name} on a detached or out-of-bounds ArrayBuffer"
    ))
}

/// V8's `TypeError` for typed arrays of different content types.
pub(super) fn content_mismatch() -> VmError {
    VmError::type_error("Cannot mix BigInt and other types, use explicit conversions")
}

/// The constructor name of an element type (`Int8Array`).
pub(super) fn type_name(ty: ElementType) -> String {
    format!("{}Array", ty.name())
}

/// The value read from an element as the value to store in another
/// element of the same content type (no conversion runs script code).
pub(super) fn element_of_raw(raw: RawValue) -> Element {
    match raw {
        RawValue::Int(int) => Element::Int(int),
        RawValue::Number(number) => Element::Number(number),
        RawValue::BigInt64(int) => Element::Bits(int as u64),
        RawValue::BigUint64(int) => Element::Bits(int),
    }
}

/// The arguments of `TypedArrayCreateFromConstructor` and
/// `TypedArraySpeciesCreate`: one length, or a buffer with a byte offset
/// and a length (`None` for the two-argument form of `subarray`).
#[derive(Clone, Copy, Debug)]
pub(super) enum CreateArgs {
    Length(usize),
    Buffer(Gc<Object>, usize, Option<usize>),
}

impl CreateArgs {
    fn values(self) -> Vec<Value> {
        match self {
            CreateArgs::Length(length) => vec![Value::number(length as f64)],
            CreateArgs::Buffer(buffer, offset, None) => {
                vec![buffer.into(), Value::number(offset as f64)]
            }
            CreateArgs::Buffer(buffer, offset, Some(length)) => vec![
                buffer.into(),
                Value::number(offset as f64),
                Value::number(length as f64),
            ],
        }
    }
}

impl Runtime {
    /// `ToIndex` (§7.1.22) with the Number for V8's messages: the index, or
    /// the Number (after `ToNumber`) that it rejects.
    pub(super) fn index_or_number(&mut self, value: Value) -> VmResult<Result<u64, f64>> {
        let number = match value {
            Value::Int(int) => f64::from(int),
            _ => self.to_number(value)?,
        };
        Ok(index_value(to_integer_or_infinity(number)).ok_or(number))
    }

    /// `AllocateTypedArrayBuffer` (§23.2.5.1.6) without the object: a new
    /// `%ArrayBuffer%` of `realm` for `length` elements of `ty`. A byte
    /// length above V8's limit is "Invalid typed array length", one that
    /// the heap limit does not allow "Array buffer allocation failed".
    pub(super) fn typed_buffer(
        &mut self,
        realm: u32,
        ty: ElementType,
        length: u64,
    ) -> VmResult<Gc<Object>> {
        let bytes = length
            .checked_mul(ty.size() as u64)
            .filter(|&bytes| bytes <= MAX_BYTE_LENGTH)
            .ok_or_else(|| invalid_length(length as f64))?;
        let proto = self.intrinsic(realm, Intrinsic::ArrayBufferPrototype)?;
        self.allocate_array_buffer(proto, bytes, None, ALLOCATION_FAILED)
    }

    /// A new typed array object with these slots, recorded in the current
    /// handle scope.
    pub(super) fn new_typed(
        &mut self,
        proto: Gc<Object>,
        data: TypedArrayData,
    ) -> VmResult<Gc<Object>> {
        let array = self.heap.new_typed_array(Some(proto), data)?;
        Ok(self.heap.record(array))
    }

    /// `AllocateTypedArray` with a length (§23.2.5.1.1 step 6): a new
    /// array of `length` zero elements over a new buffer.
    pub(super) fn allocate_typed_array(
        &mut self,
        realm: u32,
        proto: Gc<Object>,
        ty: ElementType,
        length: u64,
    ) -> VmResult<(Gc<Object>, TypedArrayData)> {
        let buffer = self.typed_buffer(realm, ty, length)?;
        let byte_length = self
            .heap
            .array_buffer(buffer)?
            .map(crate::object::ArrayBufferData::byte_length)
            .ok_or(VmError::invariant("a new buffer that is not a buffer"))?;
        let data = TypedArrayData {
            view: BufferView {
                buffer,
                byte_offset: 0,
                byte_length: Some(byte_length),
            },
            ty,
        };
        Ok((self.new_typed(proto, data)?, data))
    }

    // --- The forms of the constructor (§23.2.5.1) ---

    /// `InitializeTypedArrayFromTypedArray` (§23.2.5.1.2): a copy of the
    /// elements of `source`, converted when the types differ.
    pub(super) fn typed_from_typed(
        &mut self,
        realm: u32,
        proto: Gc<Object>,
        ty: ElementType,
        source: TypedArrayData,
    ) -> VmResult<Gc<Object>> {
        let Some(length) = self.heap.typed_length(&source)? else {
            return Err(out_of_bounds("Construct"));
        };
        let (array, data) = self.allocate_typed_array(realm, proto, ty, length as u64)?;
        if source.ty.is_bigint() != ty.is_bigint() {
            return Err(content_mismatch());
        }
        self.copy_elements(&data, 0, &source, 0, length)?;
        Ok(array)
    }

    /// Copies `count` elements of `source` from `from` to `target` at `to`
    /// (both in bounds, the same content type): the bytes for the same
    /// element type, otherwise element by element with the conversion of
    /// `SetValueInBuffer` (no script code). Overlapping ranges of one
    /// buffer are read from a copy of the source bytes (`CloneArrayBuffer`
    /// of `SetTypedArrayFromTypedArray`). Charges the time countdown.
    pub(super) fn copy_elements(
        &mut self,
        target: &TypedArrayData,
        to: usize,
        source: &TypedArrayData,
        from: usize,
        count: usize,
    ) -> VmResult<()> {
        let target_start = target.view.byte_offset + to * target.ty.size();
        let source_start = source.view.byte_offset + from * source.ty.size();
        let source_bytes = count * source.ty.size();
        if target.ty == source.ty {
            self.heap.copy_between_buffers(
                (target.view.buffer, target_start),
                (source.view.buffer, source_start),
                source_bytes,
            )?;
            return self.charge_bytes(source_bytes);
        }
        let clone = if target.view.buffer == source.view.buffer {
            if !self.heap.reserve_buffer(source_bytes, &self.vm) {
                return Err(VmError::range_error(ALLOCATION_FAILED));
            }
            let bytes = self
                .heap
                .clone_buffer_bytes(source.view.buffer, source_start, source_bytes)?
                .ok_or_else(|| VmError::range_error(ALLOCATION_FAILED))?;
            Some(bytes)
        } else {
            None
        };
        let size = source.ty.size();
        for k in 0..count {
            // The conversions report their work as they go, so that a
            // conversion of a large array reaches the time check.
            if k % CHARGE_CHUNK == CHARGE_CHUNK - 1 {
                self.charge_bytes(CHARGE_CHUNK * (size + target.ty.size()))?;
            }
            let raw = match &clone {
                Some(bytes) => {
                    let part = bytes
                        .get(k * size..(k + 1) * size)
                        .ok_or(VmError::invariant("an element outside its copy"))?;
                    crate::object::decode_element(source.ty, part)
                }
                None => self
                    .heap
                    .typed_get(source, from + k)?
                    .ok_or(VmError::invariant("a copy outside the source"))?,
            };
            self.heap.typed_set(target, to + k, element_of_raw(raw))?;
        }
        self.charge_bytes((count % CHARGE_CHUNK) * (size + target.ty.size()))
    }

    /// `InitializeTypedArrayFromArrayBuffer` (§23.2.5.1.3): a view of
    /// `buffer` from `offset` with `length` elements (both converted with
    /// `ToIndex`), or one that tracks the length of a resizable buffer.
    pub(super) fn typed_from_buffer(
        &mut self,
        proto: Gc<Object>,
        ty: ElementType,
        buffer: Gc<Object>,
        offset: Value,
        length: Value,
    ) -> VmResult<Gc<Object>> {
        let size = ty.size() as u64;
        let offset = self.index_or_number(offset)?.map_err(bad_offset)?;
        if !offset.is_multiple_of(size) {
            return Err(VmError::range_error(format!(
                "start offset of {} should be a multiple of {size}",
                type_name(ty)
            )));
        }
        let new_length = if length.is_undefined() {
            None
        } else {
            Some(self.index_or_number(length)?.map_err(invalid_length)?)
        };
        self.view_of_buffer(proto, ty, buffer, offset, new_length)
    }

    /// The steps of `InitializeTypedArrayFromArrayBuffer` after the
    /// conversions (from step 6), also for the default case of
    /// `TypedArraySpeciesCreate`.
    fn view_of_buffer(
        &mut self,
        proto: Gc<Object>,
        ty: ElementType,
        buffer: Gc<Object>,
        offset: u64,
        new_length: Option<u64>,
    ) -> VmResult<Gc<Object>> {
        let size = ty.size() as u64;
        let data = self
            .heap
            .array_buffer(buffer)?
            .ok_or(VmError::invariant("a view of another object"))?;
        if data.is_detached() {
            return Err(VmError::type_error(
                "Cannot perform Construct on a detached ArrayBuffer",
            ));
        }
        let buffer_length = data.byte_length() as u64;
        let fixed = data.max_byte_length().is_none();
        let byte_length = match new_length {
            None if !fixed => {
                if offset > buffer_length {
                    return Err(bad_offset(offset as f64));
                }
                None
            }
            None => {
                if !buffer_length.is_multiple_of(size) {
                    return Err(VmError::range_error(format!(
                        "byte length of {} should be a multiple of {size}",
                        type_name(ty)
                    )));
                }
                let bytes = buffer_length
                    .checked_sub(offset)
                    .ok_or_else(|| bad_offset(offset as f64))?;
                Some(bytes)
            }
            Some(length) => {
                let bytes = length
                    .checked_mul(size)
                    .filter(|&bytes| {
                        offset
                            .checked_add(bytes)
                            .is_some_and(|end| end <= buffer_length)
                    })
                    .ok_or_else(|| invalid_length(length as f64))?;
                Some(bytes)
            }
        };
        let to_usize =
            |n: u64| usize::try_from(n).map_err(|_| VmError::invariant("a view beyond memory"));
        let data = TypedArrayData {
            view: BufferView {
                buffer,
                byte_offset: to_usize(offset)?,
                byte_length: byte_length.map(to_usize).transpose()?,
            },
            ty,
        };
        self.new_typed(proto, data)
    }

    /// `InitializeTypedArrayFromList` (§23.2.5.1.4) with the values of the
    /// internal array `list` (from `IteratorToList`).
    pub(super) fn typed_from_list(
        &mut self,
        realm: u32,
        proto: Gc<Object>,
        ty: ElementType,
        list: Gc<Object>,
    ) -> VmResult<Gc<Object>> {
        let (length, _) = self.heap.array_length(list)?;
        let (array, data) = self.allocate_typed_array(realm, proto, ty, u64::from(length))?;
        for k in 0..length {
            self.tick()?;
            let value = self
                .heap
                .dense_elements(list)?
                .and_then(|values| values.get(k as usize).copied())
                .ok_or(VmError::invariant("a list with a hole"))?;
            self.with_scope(|rt| rt.typed_set_element(&data, f64::from(k), value))?;
        }
        Ok(array)
    }

    /// `InitializeTypedArrayFromArrayLike` (§23.2.5.1.5).
    pub(super) fn typed_from_array_like(
        &mut self,
        realm: u32,
        proto: Gc<Object>,
        ty: ElementType,
        source: Gc<Object>,
    ) -> VmResult<Gc<Object>> {
        let length = self.length_of_array_like(source)?;
        let (array, data) = self.allocate_typed_array(realm, proto, ty, length as u64)?;
        for k in 0..length as u64 {
            self.tick()?;
            self.with_scope(|rt| {
                let key = rt.index_key(k as f64)?;
                let value = rt.get_value(source.into(), key)?;
                rt.typed_set_element(&data, k as f64, value)
            })?;
        }
        Ok(array)
    }

    /// `IteratorToList(GetIteratorFromMethod(items, method))` (§7.4.13)
    /// into a new internal array (no prototype, never reachable from
    /// script code), recorded in the current handle scope. An Array
    /// Iterator with the built-in `next` over an array without holes is
    /// copied at once (nothing can observe the steps).
    pub(super) fn iterable_to_list(&mut self, items: Value, method: Value) -> VmResult<Gc<Object>> {
        let mut record = self.get_iterator_from_method(items, method)?;
        let list = self.heap.new_array(None, 0)?;
        self.heap.record(list);
        if let Some(iterator) = self.builtin_array_step(record.iterator, record.next_method)
            && self.spread_array_iterator(list, iterator)?
        {
            return Ok(list);
        }
        loop {
            let done = self.with_scope(|rt| {
                let Some(value) = rt.iterator_step_value(&mut record)? else {
                    return Ok(true);
                };
                if !rt.heap.push_element(list, value)? {
                    return Err(VmError::invariant("an internal list that cannot grow"));
                }
                Ok(false)
            })?;
            if done {
                return Ok(list);
            }
        }
    }

    // --- Species and validation (§23.2.4) ---

    /// `ValidateTypedArray(value)` (§23.2.4.4) with V8's messages for the
    /// built-in function `name`: the slots, or a `TypeError` for another
    /// value or an array that is out of bounds.
    pub(super) fn validate_typed_array(
        &self,
        value: Value,
        name: &str,
    ) -> VmResult<(Gc<Object>, TypedArrayData, usize)> {
        let Some((object, data)) = self.typed_slots(value)? else {
            return Err(self.incompatible_receiver(name, value));
        };
        match self.heap.typed_length(&data)? {
            Some(length) => Ok((object, data, length)),
            None => Err(out_of_bounds(name)),
        }
    }

    /// The object and the slots of `value` if it is a typed array.
    pub(super) fn typed_slots(
        &self,
        value: Value,
    ) -> VmResult<Option<(Gc<Object>, TypedArrayData)>> {
        let Value::Object(object) = value else {
            return Ok(None);
        };
        Ok(self.heap.typed_array(object)?.map(|data| (object, data)))
    }

    /// `TypedArrayCreateFromConstructor(constructor, args)` (§23.2.4.2) with
    /// the messages of the built-in function `name`.
    pub(super) fn typed_create_from_constructor(
        &mut self,
        constructor: Value,
        args: CreateArgs,
        name: &str,
    ) -> VmResult<(Gc<Object>, TypedArrayData)> {
        let result = self.construct(constructor, &args.values())?;
        self.heap.record(result);
        let (object, data, length) = self.validate_typed_array(result, name)?;
        if let CreateArgs::Length(wanted) = args
            && length < wanted
        {
            return Err(VmError::type_error(
                "Derived TypedArray constructor created an array which was too small",
            ));
        }
        Ok((object, data))
    }

    /// `TypedArraySpeciesCreate(exemplar, args)` (§23.2.4.1) with the
    /// messages of the built-in function `name`. With the default
    /// constructor the array is made without the call.
    pub(super) fn typed_species_create(
        &mut self,
        exemplar: Gc<Object>,
        ty: ElementType,
        args: CreateArgs,
        name: &str,
    ) -> VmResult<(Gc<Object>, TypedArrayData)> {
        let realm = self.current_realm();
        let (default, proto) = self.typed_array_intrinsics(realm, ty)?;
        let getter = self.intrinsic(realm, Intrinsic::TypedArraySpeciesGetter)?;
        let constructor = self.species_constructor(exemplar, default, getter)?;
        if constructor == Value::Object(default) {
            let array = match args {
                CreateArgs::Length(length) => {
                    return self.allocate_typed_array(realm, proto, ty, length as u64);
                }
                CreateArgs::Buffer(buffer, offset, length) => self.view_of_buffer(
                    proto,
                    ty,
                    buffer,
                    offset as u64,
                    length.map(|length| length as u64),
                )?,
            };
            let data = self
                .heap
                .typed_array(array)?
                .ok_or(VmError::invariant("a new typed array without its slots"))?;
            return Ok((array, data));
        }
        let (array, data) = self.typed_create_from_constructor(constructor, args, name)?;
        if data.ty.is_bigint() != ty.is_bigint() {
            return Err(content_mismatch());
        }
        Ok((array, data))
    }

    /// `GetMethod(items, @@iterator)` of the constructor and `from` with
    /// V8's message for a value that is not callable (`what` names the
    /// function in it).
    pub(super) fn typed_iterator_method(
        &mut self,
        items: Value,
        what: &str,
    ) -> VmResult<Option<Value>> {
        let key: PropertyKey = self.symbol_key(WellKnown::Iterator);
        let method = self.get_value(items, key)?;
        if method.is_nullish() {
            return Ok(None);
        }
        if !self.is_callable(method)? {
            return Err(VmError::type_error(format!(
                "{what} requires that the property of the first argument, items[Symbol.iterator], when exists, be a function"
            )));
        }
        Ok(Some(method))
    }
}
