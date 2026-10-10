//! Property descriptors as objects (ECMA-262 §6.2.6.4 and §6.2.6.5) and
//! the functions of `Object` that define and read properties:
//! `defineProperty`, `defineProperties`, `create`,
//! `getOwnPropertyDescriptor` and `getOwnPropertyDescriptors`.

use crate::heap::Gc;
use crate::object::{Object, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

impl Runtime {
    /// Reads the field `name` of a descriptor object: `None` if the object
    /// does not have it (`HasProperty`, then `Get`). The value is recorded
    /// in the current handle scope.
    fn descriptor_field(&mut self, object: Gc<Object>, name: &str) -> VmResult<Option<Value>> {
        let key = self.heap.key_from_str(name)?;
        self.heap.record(key);
        if !self.has_property(object, key)? {
            return Ok(None);
        }
        Ok(Some(self.get_value(object.into(), key)?))
    }

    /// `ToPropertyDescriptor` (§6.2.6.5). Reads the fields in the order of
    /// the specification; getters of the descriptor object run through
    /// re-entry. Every value is recorded in the current handle scope.
    pub(super) fn parse_property_descriptor(
        &mut self,
        value: Value,
    ) -> VmResult<PropertyDescriptor> {
        let Value::Object(object) = value else {
            return Err(VmError::type_error(format!(
                "Property description must be an object: {}",
                self.primitive_text(value)
            )));
        };
        let mut desc = PropertyDescriptor::default();
        for name in ["enumerable", "configurable"] {
            let flag = match self.descriptor_field(object, name)? {
                Some(v) => Some(self.to_boolean(v)?),
                None => None,
            };
            if name == "enumerable" {
                desc.enumerable = flag;
            } else {
                desc.configurable = flag;
            }
        }
        desc.value = self.descriptor_field(object, "value")?;
        desc.writable = match self.descriptor_field(object, "writable")? {
            Some(v) => Some(self.to_boolean(v)?),
            None => None,
        };
        for (name, what) in [("get", "Getter"), ("set", "Setter")] {
            let Some(function) = self.descriptor_field(object, name)? else {
                continue;
            };
            if !function.is_undefined() && !self.is_callable(function)? {
                return Err(VmError::type_error(format!(
                    "{what} must be a function: {}",
                    self.primitive_text(function)
                )));
            }
            if name == "get" {
                desc.get = Some(function);
            } else {
                desc.set = Some(function);
            }
        }
        if desc.is_accessor() && desc.is_data() {
            return Err(VmError::type_error(format!(
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute, {}",
                self.object_text(object)
            )));
        }
        Ok(desc)
    }

    /// `FromPropertyDescriptor` (§6.2.6.4): `undefined` for no property,
    /// otherwise a new object with the fields in the order of the
    /// specification. The result is recorded in the current handle scope.
    pub(super) fn descriptor_object(&mut self, property: Option<Property>) -> VmResult<Value> {
        let Some(property) = property else {
            return Ok(Value::Undefined);
        };
        let proto = self.intrinsic(self.current_realm(), Intrinsic::ObjectPrototype)?;
        let object = self.heap.new_object(Some(proto))?;
        self.heap.record(object);
        let mut fields: Vec<(&str, Value)> = Vec::with_capacity(6);
        match property {
            Property::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => fields.extend([
                ("value", value),
                ("writable", Value::Bool(writable)),
                ("enumerable", Value::Bool(enumerable)),
                ("configurable", Value::Bool(configurable)),
            ]),
            Property::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => fields.extend([
                ("get", get),
                ("set", set),
                ("enumerable", Value::Bool(enumerable)),
                ("configurable", Value::Bool(configurable)),
            ]),
        }
        for (name, value) in fields {
            let key = self.heap.key_from_str(name)?;
            self.heap.record(key);
            self.heap
                .create_data_property(object, key, value, &self.vm)?;
        }
        Ok(object.into())
    }

    /// `ObjectDefineProperties` (§20.1.2.3.1): reads all descriptors first,
    /// then defines them.
    pub(super) fn object_define_properties(
        &mut self,
        object: Gc<Object>,
        properties: Value,
    ) -> VmResult<()> {
        let props = self.to_object(properties)?;
        self.heap.record(props);
        let keys = self.own_keys(props)?;
        self.heap.record_keys(&keys);
        let mut descriptors = Vec::new();
        for key in keys {
            self.tick()?;
            let Some(own) = self.own_property(props, key)? else {
                continue;
            };
            if !own.enumerable() {
                continue;
            }
            let desc_object = self.get_value(props.into(), key)?;
            descriptors.push((key, self.parse_property_descriptor(desc_object)?));
        }
        for (key, desc) in descriptors {
            self.tick()?;
            self.define_or_throw(object, key, desc)?;
        }
        Ok(())
    }
}

/// `Object.defineProperty(O, P, Attributes)` (§20.1.2.4).
pub(super) fn define_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = rt.arg(call, 0) else {
        return Err(VmError::type_error(
            "Object.defineProperty called on non-object",
        ));
    };
    let key = rt.to_property_key(rt.arg(call, 1))?;
    rt.heap.record(key);
    let desc = rt.parse_property_descriptor(rt.arg(call, 2))?;
    rt.define_or_throw(object, key, desc)?;
    Ok(NativeReturn::Value(object.into()))
}

/// `Object.defineProperties(O, Properties)` (§20.1.2.3).
pub(super) fn define_properties(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = rt.arg(call, 0) else {
        return Err(VmError::type_error(
            "Object.defineProperties called on non-object",
        ));
    };
    rt.object_define_properties(object, rt.arg(call, 1))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `Object.create(O, Properties)` (§20.1.2.2).
pub(super) fn create(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let proto = match rt.arg(call, 0) {
        Value::Object(proto) => Some(proto),
        Value::Null => None,
        other => {
            return Err(VmError::type_error(format!(
                "Object prototype may only be an Object or null: {}",
                rt.primitive_text(other)
            )));
        }
    };
    let object = rt.heap.new_object(proto)?;
    rt.heap.record(object);
    let properties = rt.arg(call, 1);
    if !properties.is_undefined() {
        rt.object_define_properties(object, properties)?;
    }
    Ok(NativeReturn::Value(object.into()))
}

/// `Object.getOwnPropertyDescriptor(O, P)` (§20.1.2.8).
pub(super) fn get_own_property_descriptor(
    rt: &mut Runtime,
    call: &NativeCall,
) -> VmResult<NativeReturn> {
    let object = rt.to_object(rt.arg(call, 0))?;
    rt.heap.record(object);
    let key = rt.to_property_key(rt.arg(call, 1))?;
    rt.heap.record(key);
    let property = rt.own_property(object, key)?;
    Ok(NativeReturn::Value(rt.descriptor_object(property)?))
}

/// `Object.getOwnPropertyDescriptors(O)` (§20.1.2.9).
pub(super) fn get_own_property_descriptors(
    rt: &mut Runtime,
    call: &NativeCall,
) -> VmResult<NativeReturn> {
    let object = rt.to_object(rt.arg(call, 0))?;
    rt.heap.record(object);
    let keys = rt.own_keys(object)?;
    rt.heap.record_keys(&keys);
    let proto = rt.intrinsic(rt.current_realm(), Intrinsic::ObjectPrototype)?;
    let result = rt.heap.new_object(Some(proto))?;
    rt.heap.record(result);
    for key in keys {
        rt.tick()?;
        rt.with_scope(|rt| {
            let property = rt.own_property(object, key)?;
            let desc = rt.descriptor_object(property)?;
            if !desc.is_undefined() {
                rt.heap.create_data_property(result, key, desc, &rt.vm)?;
            }
            Ok(())
        })?;
    }
    Ok(NativeReturn::Value(result.into()))
}
