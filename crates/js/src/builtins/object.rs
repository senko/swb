//! `Object` (ECMA-262 §20.1): the constructor, `Object.keys`, and
//! `Object.prototype.toString` and `valueOf`.

use crate::object::{ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let object = super::constructor(rt, realm, "Object", 1, object_constructor, proto)?;
    super::method(rt, realm, object, "keys", 1, keys)?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    super::method(rt, realm, proto, "hasOwnProperty", 1, has_own_property)?;
    Ok(())
}

/// `Object(value)` (§20.1.1.1): a new object for `undefined` and `null`,
/// otherwise `ToObject(value)`. (A `new.target` other than `Object`
/// needs subclassing, M7.)
fn object_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    if value.is_nullish() {
        let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
        let object = rt.heap.new_object(Some(proto))?;
        return Ok(NativeReturn::Value(object.into()));
    }
    Ok(NativeReturn::Value(rt.to_object(value)?.into()))
}

/// `Object.keys(O)` (§20.1.2.18): the own enumerable string keys in the
/// order of `[[OwnPropertyKeys]]` (indices ascending, then strings in
/// creation order).
fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.to_object(rt.arg(call, 0))?;
    rt.heap.record(object);
    let proto = rt.intrinsic(rt.current_realm(), Intrinsic::ArrayPrototype)?;
    let result = rt.heap.new_array(Some(proto), 0)?;
    rt.heap.record(result);
    let mut count = 0;
    // A String object's own index properties come from its string
    // (§10.4.3.3); the String object does not store them.
    if let ObjectKind::StringWrapper(string) = rt.heap.object(object)?.kind {
        let length = rt.heap.string(string)?.len() as u32;
        for index in 0..length {
            rt.tick()?;
            let text = rt.heap.key_to_value(PropertyKey::Index(index))?;
            rt.heap
                .create_data_property(result, PropertyKey::Index(count), text, &rt.vm)?;
            count += 1;
        }
    }
    let own = rt.heap.own_property_keys(object)?;
    rt.heap.record_keys(&own);
    // Listing the keys cost one step per key; the loop costs another.
    rt.charge(own.len())?;
    for key in own {
        rt.tick()?;
        if key.is_symbol() {
            continue;
        }
        let enumerable = match rt.heap.get_own_property(object, key)? {
            Some(Property::Data { enumerable, .. } | Property::Accessor { enumerable, .. }) => {
                enumerable
            }
            None => false,
        };
        if enumerable {
            let text = rt.heap.key_to_value(key)?;
            rt.heap
                .create_data_property(result, PropertyKey::Index(count), text, &rt.vm)?;
            count += 1;
        }
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Object.prototype.toString()` (§20.1.3.6).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(builtin_tag_text(rt, call.this())?))
}

/// The text `[object Tag]` of `Object.prototype.toString`. Without
/// `Symbol.toStringTag` (M7), generator objects get the tag that
/// `%GeneratorPrototype%[@@toStringTag]` would give.
pub(super) fn builtin_tag_text(rt: &mut Runtime, value: Value) -> VmResult<Value> {
    let tag = match value {
        Value::Undefined => "Undefined",
        Value::Null => "Null",
        _ => {
            let object = rt.to_object(value)?;
            match &rt.heap.object(object)?.kind {
                ObjectKind::Array { .. } => "Array",
                ObjectKind::Arguments => "Arguments",
                ObjectKind::Function(_) | ObjectKind::Native(_) => "Function",
                ObjectKind::Error => "Error",
                ObjectKind::BooleanWrapper(_) => "Boolean",
                ObjectKind::NumberWrapper(_) => "Number",
                ObjectKind::StringWrapper(_) => "String",
                ObjectKind::Generator(_) => "Generator",
                ObjectKind::Ordinary | ObjectKind::Host { .. } | ObjectKind::SymbolWrapper(_) => {
                    "Object"
                }
            }
        }
    };
    let text = rt.heap.alloc_str(&format!("[object {tag}]"))?;
    Ok(text.into())
}

/// `Object.prototype.hasOwnProperty(V)` (§20.1.3.2).
fn has_own_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = rt.to_property_key(rt.arg(call, 0))?;
    rt.heap.record(key);
    let object = rt.this_object(call)?;
    let own = rt.string_object_index(object, key)?.is_some()
        || rt.heap.get_own_property(object, key)?.is_some();
    Ok(NativeReturn::Value(Value::Bool(own)))
}

/// `Object.prototype.valueOf()` (§20.1.3.7): `ToObject(this)`.
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(rt.to_object(call.this())?.into()))
}
