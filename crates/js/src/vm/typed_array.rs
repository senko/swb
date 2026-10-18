//! The internal methods of typed arrays (ECMA-262 §10.4.5,
//! <https://tc39.es/ecma262/#sec-typedarray-exotic-objects>) for the
//! parts that the heap cannot do (`object/typed_array.rs` has the rest):
//! the conversions of `TypedArraySetElement`, which can run script code
//! (`ToNumber`, `ToBigInt`), and the elements of the `BigInt` arrays,
//! which are new `BigInt`s. The generic internal methods (`proxy.rs`,
//! `internal.rs`, `property.rs`) dispatch here for a canonical numeric key
//! of a typed array.
//!
//! A conversion can detach or shrink the buffer: the element is written
//! only if its index is still valid after it (`TypedArraySetElement` step
//! 3), and the data of the array (the buffer handle, offset and length)
//! never changes, so a copy of it stays right while script code runs. The
//! typed array itself keeps the buffer alive; the caller holds the array.

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor, TypedArrayData};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmResult};

/// V8's message for a definition at a numeric key that is not a valid
/// index of the typed array (measured in Chromium 148).
pub(crate) const INVALID_INDEX: &str = "Invalid typed array index";

impl Runtime {
    /// The slots of `object` and the numeric index of `key` if `object` is
    /// a typed array and `key` a canonical numeric key (§10.4.5: the cases
    /// of the internal methods that differ from the ordinary ones).
    pub(crate) fn typed_numeric(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<(TypedArrayData, f64)>> {
        let Some(data) = self.heap.typed_array(object)? else {
            return Ok(None);
        };
        Ok(self.heap.numeric_index(key)?.map(|index| (data, index)))
    }

    /// The value of a call of `getter` with `receiver` when `getter` is
    /// the built-in getter of `%TypedArray%.prototype.length` (of any realm)
    /// and `receiver` a typed array: the length (0 when out of bounds),
    /// which the call would return without running script code. `None`
    /// for any other getter or receiver (the call runs, and throws for a
    /// receiver that is not a typed array).
    pub(crate) fn typed_length_getter_value(
        &self,
        getter: Value,
        receiver: Value,
    ) -> VmResult<Option<Value>> {
        let (Value::Object(getter), Value::Object(receiver)) = (getter, receiver) else {
            return Ok(None);
        };
        let ObjectKind::Native(native) = &self.heap.object(getter)?.kind else {
            return Ok(None);
        };
        let builtin = self
            .vm
            .realms
            .get(native.realm as usize)
            .and_then(|realm| realm.intrinsic(Intrinsic::TypedArrayLengthGetter));
        if builtin != Some(getter) {
            return Ok(None);
        }
        let Some(data) = self.heap.typed_array(receiver)? else {
            return Ok(None);
        };
        let length = self.heap.typed_length(&data)?.unwrap_or(0);
        Ok(Some(Value::number(length as f64)))
    }

    /// `TypedArrayGetElement` for an element index: the value, or
    /// `undefined` for an index that is not valid. A `BigInt` element is
    /// a new `BigInt` (not recorded).
    pub(crate) fn typed_element(&mut self, data: &TypedArrayData, index: usize) -> VmResult<Value> {
        match self.heap.typed_get(data, index)? {
            Some(raw) => self.element_value(raw),
            None => Ok(Value::Undefined),
        }
    }

    /// `TypedArrayGetElement` for a numeric index (any Number).
    pub(crate) fn typed_index_value(
        &mut self,
        data: &TypedArrayData,
        index: f64,
    ) -> VmResult<Value> {
        match self.heap.valid_index(data, index)? {
            Some(index) => self.typed_element(data, index),
            None => Ok(Value::Undefined),
        }
    }

    /// `TypedArraySetElement(O, index, value)`
    /// (<https://tc39.es/ecma262/#sec-typedarraysetelement>): converts
    /// `value` to the content type, then writes it if `index` is (still) a
    /// valid index.
    pub(crate) fn typed_set_element(
        &mut self,
        data: &TypedArrayData,
        index: f64,
        value: Value,
    ) -> VmResult<()> {
        let element = match self.heap.plain_element(data.ty, value)? {
            Some(element) => element,
            None => self.element_of(data.ty, value)?,
        };
        if let Some(index) = self.heap.valid_index(data, index)? {
            self.heap.typed_set(data, index, element)?;
        }
        Ok(())
    }

    /// `[[GetOwnProperty]]` of a typed array for a numeric index: the data
    /// property of a valid index (its value recorded in the current handle
    /// scope), else none.
    pub(crate) fn typed_own_property(
        &mut self,
        data: &TypedArrayData,
        index: f64,
    ) -> VmResult<Option<Property>> {
        let Some(index) = self.heap.valid_index(data, index)? else {
            return Ok(None);
        };
        let value = self.typed_element(data, index)?;
        self.heap.record(value);
        Ok(Some(Property::Data {
            value,
            writable: true,
            enumerable: true,
            configurable: true,
        }))
    }

    /// `[[DefineOwnProperty]]` of a typed array for a numeric index
    /// (§10.4.5): the checks of the index and the descriptor, then
    /// `TypedArraySetElement` with the value, which can run script code.
    pub(crate) fn typed_define_own_property(
        &mut self,
        data: &TypedArrayData,
        index: f64,
        desc: &PropertyDescriptor,
    ) -> VmResult<bool> {
        if self.heap.typed_define_target(data, index, desc)?.is_none() {
            return Ok(false);
        }
        if let Some(value) = desc.value {
            self.typed_set_element(data, index, value)?;
        }
        Ok(true)
    }
}
