//! Array exotic objects (ECMA-262 §10.4.2): `[[DefineOwnProperty]]` and
//! `ArraySetLength`.
//!
//! `length` is a field of the object kind with a writable flag, not a
//! stored property. Shrinking `length` costs at most the number of stored
//! elements (memo 3.2).
//!
//! The bulk copy for spread ([`Heap::append_array_elements`]) appends the
//! elements of an array without holes to another array, in place of the
//! steps of an Array Iterator that run no script code.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::ops::length_value;
use crate::object::property::{Apply, validate_and_apply};
use crate::object::{Elements, Object, ObjectKind, Property, PropertyDescriptor};
use crate::string::PropertyKey;
use crate::value::Value;

/// The message of the `RangeError` for an invalid array length.
pub(crate) const INVALID_ARRAY_LENGTH: &str = "Invalid array length";

/// The message of the same `RangeError` when `[[Set]]` writes `length`
/// (Chromium 148; `Object.defineProperty` keeps the short one).
pub(crate) const INVALID_LENGTH_WRITE: &str =
    "Failed to set the 'length' property on 'Array': Invalid array length";

/// The `TypeError`s of `ToNumber` of a symbol and a `BigInt` length.
pub(crate) const SYMBOL_LENGTH: &str = "Cannot convert a Symbol value to a number";
pub(crate) const BIGINT_LENGTH: &str = "Cannot convert a BigInt value to a number";

/// The same `TypeError`s when `[[Set]]` writes `length` (Chromium 148).
const SYMBOL_LENGTH_WRITE: &str =
    "Failed to set the 'length' property on 'Array': Cannot convert a Symbol value to a number";
const BIGINT_LENGTH_WRITE: &str =
    "Failed to set the 'length' property on 'Array': Cannot convert a BigInt value to a number";

/// The error of a write of `length` with `[[Set]]`: an invalid length
/// and a symbol or `BigInt` value get Chromium's longer messages.
pub(crate) fn length_write_error(error: Error) -> Error {
    match error {
        Error::Throw { message, .. } if message == INVALID_ARRAY_LENGTH => {
            Error::range_error(INVALID_LENGTH_WRITE)
        }
        Error::Throw { message, .. } if message == SYMBOL_LENGTH => {
            Error::type_error(SYMBOL_LENGTH_WRITE)
        }
        Error::Throw { message, .. } if message == BIGINT_LENGTH => {
            Error::type_error(BIGINT_LENGTH_WRITE)
        }
        error => error,
    }
}

/// [`length_write_error`] for the message of an error of the VM.
pub(crate) fn length_write_message(message: &str) -> Option<&'static str> {
    match message {
        INVALID_ARRAY_LENGTH => Some(INVALID_LENGTH_WRITE),
        SYMBOL_LENGTH => Some(SYMBOL_LENGTH_WRITE),
        BIGINT_LENGTH => Some(BIGINT_LENGTH_WRITE),
        _ => None,
    }
}

/// The elements that [`Heap::append_array_elements`] copies per round
/// (a fixed buffer, not one that the script sizes).
const APPEND_CHUNK: usize = 256;

/// `ToUint32` of the value of a `length` descriptor, with the check of
/// `ArraySetLength` steps 3 to 5: the value must be an integer from 0 to
/// 2^32 − 1.
///
/// Strings and objects need `ToNumber`, which can run script code or needs
/// `StringToNumber`; the caller converts them before (an internal error
/// otherwise).
fn to_array_length(value: Value) -> Result<u32> {
    let number = match value {
        Value::Int(int) => f64::from(int),
        Value::Double(double) => double,
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(b)),
        Value::Symbol(_) => {
            return Err(Error::type_error(SYMBOL_LENGTH));
        }
        Value::BigInt(_) => {
            return Err(Error::type_error(BIGINT_LENGTH));
        }
        Value::String(_) | Value::Object(_) | Value::Empty | Value::Cell(_) => {
            return Err(Error::invariant(
                "ArraySetLength needs a primitive; the caller applies ToNumber to strings and objects",
            ));
        }
    };
    if number >= 0.0 && number <= f64::from(u32::MAX) && number.fract() == 0.0 {
        Ok(number as u32)
    } else {
        Err(Error::range_error(INVALID_ARRAY_LENGTH))
    }
}

impl Heap {
    /// The length of an array whose elements are dense without holes
    /// (every index below `length` holds a data value), with the
    /// writable flag; `None` for another object or form.
    fn hole_free_length(&self, array: Gc<Object>) -> Result<Option<(u32, bool)>> {
        let record = self.object(array)?;
        let ObjectKind::Array {
            length,
            length_writable,
        } = record.kind
        else {
            return Ok(None);
        };
        Ok(match &record.elements {
            Elements::Dense { values, count }
                if values.len() == length as usize && *count == length =>
            {
                Some((length, length_writable))
            }
            _ => None,
        })
    }

    /// Appends the elements of the array `source` from index `start` on
    /// to the end of the array `target`, as `CreateDataProperty` at
    /// `target.length`, `target.length + 1`, ... would. Only for the form
    /// where reading `source` runs no script code and leaves no gap:
    /// both arrays dense without holes, and `target.length` writable.
    /// Returns the number of elements appended, or `None` (and changes
    /// nothing) for any other form. Can collect.
    pub(crate) fn append_array_elements(
        &mut self,
        target: Gc<Object>,
        source: Gc<Object>,
        start: u32,
        roots: &dyn RootSource,
    ) -> Result<Option<u32>> {
        let Some((source_length, _)) = self.hole_free_length(source)? else {
            return Ok(None);
        };
        let Some((target_length, true)) = self.hole_free_length(target)? else {
            return Ok(None);
        };
        let count = source_length.saturating_sub(start);
        let Some(new_length) = target_length
            .checked_add(count)
            .filter(|&length| length < u32::MAX)
        else {
            return Ok(None);
        };
        // The vector grows once: to the exact size for a large copy (a
        // doubling would count up to twice the elements against the heap
        // limit), else by doubling, so that many small copies stay linear.
        let capacity = match &self.object(target)?.elements {
            Elements::Dense { values, .. } => values.capacity(),
            Elements::Sparse(_) => return Ok(None),
        };
        let needed = new_length as usize;
        let grown = if needed <= capacity {
            capacity
        } else if count as usize >= capacity {
            needed
        } else {
            needed.max(capacity * 2)
        };
        let bytes = (grown - capacity) * size_of::<Value>();
        self.reserve_with(bytes, roots, &[target.into(), source.into()])?;
        let before = self.object_bytes(target)?;
        if let Elements::Dense { values, .. } = &mut self.object_mut(target)?.elements {
            values.try_reserve_exact(grown - values.len())?;
        }
        let mut buffer = [Value::Undefined; APPEND_CHUNK];
        let mut next = start as usize;
        let end = source_length as usize;
        while next < end {
            let chunk = (end - next).min(APPEND_CHUNK);
            let Elements::Dense { values, .. } = &self.object(source)?.elements else {
                return Err(Error::invariant("the source of a bulk copy became sparse"));
            };
            let part = values
                .get(next..next + chunk)
                .ok_or(Error::invariant("a bulk copy outside the elements"))?;
            let slots = buffer
                .get_mut(..chunk)
                .ok_or(Error::invariant("a bulk copy chunk outside the buffer"))?;
            slots.copy_from_slice(part);
            let Elements::Dense { values, count } = &mut self.object_mut(target)?.elements else {
                return Err(Error::invariant("the target of a bulk copy became sparse"));
            };
            values.extend_from_slice(slots);
            *count += chunk as u32;
            next += chunk;
        }
        self.set_array_length_field(target, new_length, true)?;
        self.charge_growth(target, before)?;
        Ok(Some(count))
    }

    /// `length` and its writable flag of an array.
    pub fn array_length(&self, array: Gc<Object>) -> Result<(u32, bool)> {
        match self.object(array)?.kind {
            ObjectKind::Array {
                length,
                length_writable,
            } => Ok((length, length_writable)),
            _ => Err(Error::invariant("not an array")),
        }
    }

    pub(crate) fn set_array_length_field(
        &mut self,
        array: Gc<Object>,
        new_length: u32,
        writable: bool,
    ) -> Result<()> {
        match &mut self.object_mut(array)?.kind {
            ObjectKind::Array {
                length,
                length_writable,
            } => {
                *length = new_length;
                *length_writable = writable;
                Ok(())
            }
            _ => Err(Error::invariant("not an array")),
        }
    }

    /// `[[DefineOwnProperty]]` of arrays (§10.4.2.1). Can collect.
    pub(crate) fn array_define_own_property(
        &mut self,
        array: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        if key == PropertyKey::String(self.names.length) {
            return self.array_set_length(array, desc);
        }
        let PropertyKey::Index(index) = key else {
            return self.ordinary_define_own_property(array, key, desc, roots);
        };
        let (length, writable) = self.array_length(array)?;
        if index >= length && !writable {
            return Ok(false);
        }
        if !self.ordinary_define_own_property(array, key, desc, roots)? {
            return Ok(false);
        }
        if index >= length {
            // `index` is below 2^32 − 1, so the new length fits.
            self.set_array_length_field(array, index + 1, writable)?;
        }
        Ok(true)
    }

    /// `ArraySetLength` (§10.4.2.4).
    fn array_set_length(&mut self, array: Gc<Object>, desc: PropertyDescriptor) -> Result<bool> {
        let Some(value) = desc.value else {
            return self.define_length(array, &desc);
        };
        let new_length = to_array_length(value)?;
        let mut new_desc = desc;
        new_desc.value = Some(length_value(new_length));
        let (old_length, old_writable) = self.array_length(array)?;
        if new_length >= old_length {
            return self.define_length(array, &new_desc);
        }
        if !old_writable {
            return Ok(false);
        }
        let new_writable = new_desc.writable != Some(false);
        if !new_writable {
            // Step 13: keep `length` writable until the elements are gone.
            new_desc.writable = Some(true);
        }
        if !self.define_length(array, &new_desc)? {
            return Ok(false);
        }
        let blocked = self.object_mut(array)?.elements.remove_from(new_length);
        if let Some(index) = blocked {
            new_desc.value = Some(length_value(index + 1));
            if !new_writable {
                new_desc.writable = Some(false);
            }
            self.define_length(array, &new_desc)?;
            return Ok(false);
        }
        if !new_writable {
            let read_only = PropertyDescriptor {
                writable: Some(false),
                ..PropertyDescriptor::default()
            };
            self.define_length(array, &read_only)?;
        }
        Ok(true)
    }

    /// OrdinaryDefineOwnProperty(A, "length", Desc): `ValidateAndApply`
    /// against the `length` field. The caller has checked that a
    /// `[[Value]]` in `desc` is a valid length.
    fn define_length(&mut self, array: Gc<Object>, desc: &PropertyDescriptor) -> Result<bool> {
        let (length, writable) = self.array_length(array)?;
        let current = Property::Data {
            value: length_value(length),
            writable,
            enumerable: false,
            configurable: false,
        };
        let extensible = self.object(array)?.extensible;
        match validate_and_apply(self, extensible, desc, Some(current))? {
            Apply::Reject => Ok(false),
            Apply::Unchanged => Ok(true),
            Apply::Update(Property::Data {
                value, writable, ..
            }) => {
                let new_length = to_array_length(value)?;
                self.set_array_length_field(array, new_length, writable)?;
                Ok(true)
            }
            Apply::Create(_) | Apply::Update(Property::Accessor { .. }) => Err(Error::invariant(
                "the length of an array became an accessor",
            )),
        }
    }
}
