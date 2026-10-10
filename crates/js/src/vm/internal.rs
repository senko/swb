//! The internal methods of objects for Rust code (ECMA-262 §10.1, §10.4):
//! the layer between the natives and the heap's object model.
//!
//! The heap implements the methods that need no allocation and no script
//! code. This layer adds what does: the character properties of String
//! objects (each read allocates a string), the conversion of the value of
//! `length` for arrays (`ToNumber` can run script code), the immutable
//! prototype of `%Object.prototype%` (§10.4.7) and the error messages
//! (V8's wording) of the failing `DefinePropertyOrThrow` and
//! `[[SetPrototypeOf]]`. The result of a read is recorded in the current
//! handle scope.

use crate::heap::Gc;
use crate::object::{INVALID_ARRAY_LENGTH, Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult};

/// The outcome of `[[SetPrototypeOf]]` for the caller that must tell the
/// reasons apart (the messages of `TypeError`s differ).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProtoSet {
    /// The prototype is set (or was the same).
    Done,
    /// The object is not extensible.
    NotExtensible,
    /// The new prototype chain would contain the object.
    Cycle,
    /// The object is an immutable prototype object (§10.4.7).
    Immutable,
}

impl Runtime {
    /// `[[GetOwnProperty]]` (§10.1.5, with §10.4.3.5 for String objects).
    /// The character of a String object is a new string, recorded in the
    /// current handle scope.
    pub(crate) fn own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        if let Some(unit) = self.heap.string_object_unit(object, key)? {
            let text = swb_js_text::String16::from_units(&[unit]);
            let character = self.heap.alloc_string(text)?;
            self.heap.record(character);
            return Ok(Some(Property::Data {
                value: Value::String(character),
                writable: false,
                enumerable: true,
                configurable: false,
            }));
        }
        Ok(self.heap.get_own_property(object, key)?)
    }

    /// `[[DefineOwnProperty]]` (§10.1.6, §10.4.2.1, §10.4.3.2). For the
    /// `length` of an array, a string or object in `[[Value]]` is
    /// converted first (`ArraySetLength` steps 3 to 5, which can run
    /// script code).
    pub(crate) fn define_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        mut desc: PropertyDescriptor,
    ) -> VmResult<bool> {
        if let Some(value @ (Value::String(_) | Value::Object(_))) = desc.value
            && key == PropertyKey::String(self.heap.length_atom())
            && matches!(self.heap.object(object)?.kind, ObjectKind::Array { .. })
        {
            desc.value = Some(self.convert_array_length(value)?);
        }
        Ok(self.heap.define_own_property(object, key, desc, &self.vm)?)
    }

    /// `ArraySetLength` steps 3 to 5 (§10.4.2.4): `ToUint32(V)` and
    /// `ToNumber(V)` (two conversions, as the specification says), a
    /// `RangeError` if they differ. The heap's define takes only
    /// primitives.
    pub(crate) fn convert_array_length(&mut self, value: Value) -> VmResult<Value> {
        self.with_scope(|rt| {
            let integer = rt.to_number(value)?;
            let number = rt.to_number(value)?;
            if f64::from(super::convert::to_uint32(integer)) != number {
                return Err(VmError::range_error(INVALID_ARRAY_LENGTH));
            }
            Ok(Value::number(number))
        })
    }

    /// `DefinePropertyOrThrow` (§7.3.8): a refusal is a `TypeError` with
    /// V8's wording.
    pub(crate) fn define_or_throw(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
    ) -> VmResult<()> {
        let before = self.heap.array_length(object).ok();
        if self.define_own_property(object, key, desc)? {
            return Ok(());
        }
        // ArraySetLength stops at an element that cannot be deleted
        // (§10.4.2.4 step 17); V8 names that element.
        if let (Some((old, true)), Some(new)) = (before, desc.value.and_then(Value::as_number))
            && key == PropertyKey::String(self.heap.length_atom())
            && new < f64::from(old)
            && let Ok((length, _)) = self.heap.array_length(object)
        {
            return Err(VmError::type_error(format!(
                "Cannot delete property '{}' of [object Array]",
                length.saturating_sub(1)
            )));
        }
        Err(self.define_failure(object, key))
    }

    /// `CreateDataPropertyOrThrow(object, key, value)` (§7.3.7).
    pub(crate) fn create_data_property_or_throw(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
    ) -> VmResult<()> {
        if self
            .heap
            .create_data_property(object, key, value, &self.vm)?
        {
            Ok(())
        } else {
            Err(self.define_failure(object, key))
        }
    }

    /// The `TypeError` for a refused definition of `key`.
    pub(crate) fn define_failure(&mut self, object: Gc<Object>, key: PropertyKey) -> VmError {
        let text = self.key_text(key);
        let exists = matches!(self.own_property(object, key), Ok(Some(_)));
        if exists {
            VmError::type_error(format!("Cannot redefine property: {text}"))
        } else {
            VmError::type_error(format!(
                "Cannot define property {text}, object is not extensible"
            ))
        }
    }

    /// Whether `object` is the `prototype` of Number, Boolean, String or
    /// Symbol in some realm.
    pub(crate) fn is_wrapper_prototype(&self, object: Gc<Object>) -> bool {
        const WRAPPERS: [Intrinsic; 4] = [
            Intrinsic::NumberPrototype,
            Intrinsic::BooleanPrototype,
            Intrinsic::StringPrototype,
            Intrinsic::SymbolPrototype,
        ];
        self.vm.realms.iter().any(|realm| {
            WRAPPERS
                .iter()
                .any(|which| realm.intrinsic(*which) == Some(object))
        })
    }

    /// Whether `object` is the `%Object.prototype%` of a realm, an
    /// immutable prototype object (§10.4.7).
    fn is_immutable_prototype(&self, object: Gc<Object>) -> bool {
        self.vm
            .realms
            .iter()
            .any(|realm| realm.intrinsic(Intrinsic::ObjectPrototype) == Some(object))
    }

    /// `[[SetPrototypeOf]]` (§10.1.2, §10.4.7.1). The cycle check charges
    /// the time countdown for the prototypes it visits.
    pub(crate) fn set_prototype_of(
        &mut self,
        object: Gc<Object>,
        proto: Option<Gc<Object>>,
    ) -> VmResult<ProtoSet> {
        if self.is_immutable_prototype(object) {
            // SetImmutablePrototype (§10.4.7.2).
            let same = self.heap.get_prototype_of(object)? == proto;
            return Ok(if same {
                ProtoSet::Done
            } else {
                ProtoSet::Immutable
            });
        }
        let extensible = self.heap.is_extensible(object)?;
        let (done, steps) = self.heap.set_prototype_of_counted(object, proto)?;
        self.charge(steps)?;
        Ok(match (done, extensible) {
            (true, _) => ProtoSet::Done,
            (false, false) => ProtoSet::NotExtensible,
            (false, true) => ProtoSet::Cycle,
        })
    }

    /// The `TypeError` of a refused `[[SetPrototypeOf]]` (V8's wording).
    pub(crate) fn set_prototype_failure(&self, object: Gc<Object>, why: ProtoSet) -> VmError {
        match why {
            ProtoSet::NotExtensible => {
                VmError::type_error(format!("{} is not extensible", self.object_text(object)))
            }
            ProtoSet::Immutable => VmError::type_error(
                "Immutable prototype object 'Object.prototype' cannot have their prototype set",
            ),
            ProtoSet::Cycle | ProtoSet::Done => VmError::type_error("Cyclic __proto__ value"),
        }
    }
}
