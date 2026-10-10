//! `Reflect` (ECMA-262 §28.1): the 13 functions that expose the internal
//! methods of objects.
//!
//! `Reflect.apply` and `Reflect.construct` end in a deferred call, without
//! Rust recursion. The list of arguments is limited by the value stack.

use crate::heap::Gc;
use crate::object::{Object, SetResult};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::Lookup;
use crate::vm::internal::ProtoSet;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let reflect = rt.heap.new_object(Some(proto))?;
    rt.heap.record(reflect);
    let functions: [(&str, u32, NativeFn); 13] = [
        ("apply", 3, apply),
        ("construct", 2, construct),
        ("defineProperty", 3, define_property),
        ("deleteProperty", 2, delete_property),
        ("get", 2, get),
        ("getOwnPropertyDescriptor", 2, get_own_property_descriptor),
        ("getPrototypeOf", 1, get_prototype_of),
        ("has", 2, has),
        ("isExtensible", 1, is_extensible),
        ("ownKeys", 1, own_keys),
        ("preventExtensions", 1, prevent_extensions),
        ("set", 3, set),
        ("setPrototypeOf", 2, set_prototype_of),
    ];
    for (name, length, func) in functions {
        super::method(rt, realm, reflect, name, length, func)?;
    }
    super::global(rt, realm, "Reflect", reflect.into())
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the result type of native functions"
)]
fn ok(value: bool) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::Bool(value)))
}

/// The first argument, which must be an object (V8: "Reflect.get called
/// on non-object").
fn target(rt: &Runtime, call: &NativeCall, name: &str) -> VmResult<Gc<Object>> {
    match rt.arg(call, 0) {
        Value::Object(object) => Ok(object),
        _ => Err(VmError::type_error(format!(
            "Reflect.{name} called on non-object"
        ))),
    }
}

/// The text "X, which is a Y" of V8's message for `Reflect.apply`.
fn not_a_function(rt: &Runtime, value: Value) -> VmError {
    let kind = match value {
        Value::Null => None,
        Value::Undefined => Some("undefined"),
        Value::Bool(_) => Some("boolean"),
        Value::Int(_) | Value::Double(_) => Some("number"),
        Value::String(_) => Some("string"),
        Value::Symbol(_) => Some("symbol"),
        Value::BigInt(_) => Some("bigint"),
        Value::Object(_) | Value::Empty | Value::Cell(_) => Some("object"),
    };
    let which = match kind {
        None => "null".to_owned(),
        Some("object") => "an object".to_owned(),
        Some(kind) => format!("a {kind}"),
    };
    VmError::type_error(format!(
        "Function.prototype.apply was called on {}, which is {which} and not a function",
        rt.primitive_text(value)
    ))
}

/// `Reflect.apply(target, thisArgument, argumentsList)` (§28.1.1).
fn apply(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let callee = rt.arg(call, 0);
    if !rt.is_callable(callee)? {
        return Err(not_a_function(rt, callee));
    }
    let args = rt.list_from_array_like(rt.arg(call, 2))?;
    Ok(NativeReturn::Call {
        callee,
        this: rt.arg(call, 1),
        args,
    })
}

/// `Reflect.construct(target, argumentsList [, newTarget])` (§28.1.2).
fn construct(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let callee = rt.arg(call, 0);
    if !rt.is_constructor(callee)? {
        return Err(VmError::type_error(format!(
            "{} is not a constructor",
            rt.primitive_text(callee)
        )));
    }
    let new_target = if call.argc() > 2 {
        rt.arg(call, 2)
    } else {
        callee
    };
    if !rt.is_constructor(new_target)? {
        return Err(VmError::type_error(format!(
            "{} is not a constructor",
            rt.primitive_text(new_target)
        )));
    }
    let args = rt.list_from_array_like(rt.arg(call, 1))?;
    Ok(NativeReturn::Construct {
        callee,
        new_target,
        args,
    })
}

/// `Reflect.defineProperty(target, propertyKey, attributes)` (§28.1.3).
fn define_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "defineProperty")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    let desc = rt.parse_property_descriptor(rt.arg(call, 2))?;
    ok(rt.define_own_property(object, key, desc)?)
}

/// `Reflect.deleteProperty(target, propertyKey)` (§28.1.4).
fn delete_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "deleteProperty")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    ok(rt.heap.delete(object, key)?)
}

/// `Reflect.get(target, propertyKey [, receiver])` (§28.1.5).
fn get(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "get")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    let receiver = if call.argc() > 2 {
        rt.arg(call, 2)
    } else {
        object.into()
    };
    match rt.get_with_receiver(object, key, receiver)? {
        Lookup::Value(value) => Ok(NativeReturn::Value(value)),
        Lookup::Getter { getter, receiver } => Ok(NativeReturn::Call {
            callee: getter,
            this: receiver,
            args: Vec::new(),
        }),
    }
}

/// `Reflect.getOwnPropertyDescriptor(target, propertyKey)` (§28.1.7).
fn get_own_property_descriptor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "getOwnPropertyDescriptor")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    let property = rt.own_property(object, key)?;
    Ok(NativeReturn::Value(rt.descriptor_object(property)?))
}

/// `Reflect.getPrototypeOf(target)` (§28.1.8).
fn get_prototype_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "getPrototypeOf")?;
    let proto = rt.heap.get_prototype_of(object)?;
    Ok(NativeReturn::Value(proto.map_or(Value::Null, Value::from)))
}

/// `Reflect.has(target, propertyKey)` (§28.1.9).
fn has(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "has")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    ok(rt.has_property(object, key)?)
}

/// `Reflect.isExtensible(target)` (§28.1.10).
fn is_extensible(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "isExtensible")?;
    ok(rt.heap.is_extensible(object)?)
}

/// `Reflect.ownKeys(target)` (§28.1.11).
fn own_keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "ownKeys")?;
    let keys = rt.own_keys(object)?;
    rt.heap.record_keys(&keys);
    let result = rt.new_array()?;
    for (index, key) in keys.into_iter().enumerate() {
        rt.tick()?;
        let item = rt.heap.key_to_value(key)?;
        let index = PropertyKey::Index(index as u32);
        rt.heap.create_data_property(result, index, item, &rt.vm)?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Reflect.preventExtensions(target)` (§28.1.12).
fn prevent_extensions(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "preventExtensions")?;
    ok(rt.heap.prevent_extensions(object)?)
}

/// `Reflect.set(target, propertyKey, V [, receiver])` (§28.1.13).
fn set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "set")?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    let receiver = if call.argc() > 3 {
        rt.arg(call, 3)
    } else {
        object.into()
    };
    let value = rt.array_length_operand(receiver, key, rt.arg(call, 2))?;
    match rt.heap.set(object, key, value, receiver, &rt.vm)? {
        SetResult::Done(done) => ok(done),
        SetResult::CallSetter { setter, receiver } => {
            rt.call(setter, receiver, &[value])?;
            ok(true)
        }
    }
}

/// `Reflect.setPrototypeOf(target, proto)` (§28.1.14).
fn set_prototype_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = target(rt, call, "setPrototypeOf")?;
    let proto = match rt.arg(call, 1) {
        Value::Object(proto) => Some(proto),
        Value::Null => None,
        other => {
            return Err(VmError::type_error(format!(
                "Object prototype may only be an Object or null: {}",
                rt.primitive_text(other)
            )));
        }
    };
    ok(rt.set_prototype_of(object, proto)? == ProtoSet::Done)
}
