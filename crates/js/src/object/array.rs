//! Array exotic objects (ECMA-262 §10.4.2): `[[DefineOwnProperty]]` and
//! `ArraySetLength`.
//!
//! `length` is a field of the object kind with a writable flag, not a
//! stored property. Shrinking `length` costs at most the number of stored
//! elements (memo 3.2).

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::ops::length_value;
use crate::object::property::{Apply, validate_and_apply};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::string::PropertyKey;
use crate::value::Value;

/// `ToUint32` of the value of a `length` descriptor, with the check of
/// `ArraySetLength` steps 3 to 5: the value must be an integer from 0 to
/// 2^32 − 1.
///
/// Strings and objects need `ToNumber`, which can run script code or needs
/// `StringToNumber`; the caller converts them before (an internal error
/// otherwise).
fn array_length(value: Value) -> Result<u32> {
    let number = match value {
        Value::Int(int) => f64::from(int),
        Value::Double(double) => double,
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(b)),
        Value::Symbol(_) => {
            return Err(Error::type_error(
                "Cannot convert a Symbol value to a number",
            ));
        }
        Value::BigInt(_) => {
            return Err(Error::type_error(
                "Cannot convert a BigInt value to a number",
            ));
        }
        Value::String(_) | Value::Object(_) | Value::Empty => {
            return Err(Error::invariant(
                "ArraySetLength needs a primitive; the caller applies ToNumber to strings and objects",
            ));
        }
    };
    if number >= 0.0 && number <= f64::from(u32::MAX) && number.fract() == 0.0 {
        Ok(number as u32)
    } else {
        Err(Error::range_error("Invalid array length"))
    }
}

impl Heap {
    /// `length` and its writable flag of an array.
    pub fn array_length(&self, array: Gc<Object>) -> Result<(u32, bool)> {
        match self.object(array)?.kind {
            ObjectKind::Array {
                length,
                length_writable,
            } => Ok((length, length_writable)),
            ObjectKind::Ordinary | ObjectKind::Host { .. } => Err(Error::invariant("not an array")),
        }
    }

    fn set_array_length_field(
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
            ObjectKind::Ordinary | ObjectKind::Host { .. } => Err(Error::invariant("not an array")),
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
        let new_length = array_length(value)?;
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
                let new_length = array_length(value)?;
                self.set_array_length_field(array, new_length, writable)?;
                Ok(true)
            }
            Apply::Create(_) | Apply::Update(Property::Accessor { .. }) => Err(Error::invariant(
                "the length of an array became an accessor",
            )),
        }
    }
}
