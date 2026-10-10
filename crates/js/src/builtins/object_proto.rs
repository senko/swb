//! `Object.prototype` (ECMA-262 §20.1.3) and its Annex B additions
//! (§B.2.2): `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`,
//! `toLocaleString`, `toString`, `valueOf`, the `__proto__` accessor and
//! `__defineGetter__`, `__defineSetter__`, `__lookupGetter__`,
//! `__lookupSetter__`.

use crate::object::{Object, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::internal::ProtoSet;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmError, VmResult};

pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    proto: crate::heap::Gc<Object>,
) -> VmResult<()> {
    let methods: [(&str, u32, NativeFn); 10] = [
        ("hasOwnProperty", 1, has_own_property),
        ("isPrototypeOf", 1, is_prototype_of),
        ("propertyIsEnumerable", 1, property_is_enumerable),
        ("toLocaleString", 0, to_locale_string),
        ("toString", 0, to_string),
        ("valueOf", 0, value_of),
        ("__defineGetter__", 2, define_getter),
        ("__defineSetter__", 2, define_setter),
        ("__lookupGetter__", 1, lookup_getter),
        ("__lookupSetter__", 1, lookup_setter),
    ];
    for (name, length, func) in methods {
        super::method(rt, realm, proto, name, length, func)?;
    }
    let get = rt.new_builtin(realm, "get __proto__", 0, proto_getter, false)?;
    let set = rt.new_builtin(realm, "set __proto__", 1, proto_setter, false)?;
    let key = rt.heap.key_from_str("__proto__")?;
    let desc = PropertyDescriptor::accessor(get.into(), set.into(), false, true);
    rt.define_own_property(proto, key, desc)?;
    Ok(())
}

/// `Object.prototype.hasOwnProperty(V)` (§20.1.3.2).
fn has_own_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = rt.key_arg(rt.arg(call, 0))?;
    let object = rt.this_object(call)?;
    let own = rt.heap.string_object_unit(object, key)?.is_some()
        || rt.heap.get_own_property(object, key)?.is_some();
    Ok(NativeReturn::Value(Value::Bool(own)))
}

/// `Object.prototype.isPrototypeOf(V)` (§20.1.3.3). A step of the
/// countdown per prototype visited.
fn is_prototype_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(value) = rt.arg(call, 0) else {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    };
    let this = rt.this_object(call)?;
    let mut current = rt.heap.get_prototype_of(value)?;
    while let Some(proto) = current {
        rt.tick()?;
        if proto == this {
            return Ok(NativeReturn::Value(Value::Bool(true)));
        }
        current = rt.heap.get_prototype_of(proto)?;
    }
    Ok(NativeReturn::Value(Value::Bool(false)))
}

/// `Object.prototype.propertyIsEnumerable(V)` (§20.1.3.4).
fn property_is_enumerable(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = rt.key_arg(rt.arg(call, 0))?;
    let object = rt.this_object(call)?;
    let enumerable = rt.heap.string_object_unit(object, key)?.is_some()
        || rt
            .heap
            .get_own_property(object, key)?
            .is_some_and(|property| property.enumerable());
    Ok(NativeReturn::Value(Value::Bool(enumerable)))
}

/// `Object.prototype.toLocaleString()` (§20.1.3.5): `Invoke(this,
/// "toString")` as a deferred call.
fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = call.this();
    if this.is_nullish() {
        return Err(VmError::type_error(
            "Object.prototype.toLocaleString called on null or undefined",
        ));
    }
    let key = crate::string::PropertyKey::String(rt.vm.atoms.to_string);
    let method = rt.get_value(this, key)?;
    if !rt.is_callable(method)? {
        return Err(rt.not_callable(method));
    }
    Ok(NativeReturn::Call {
        callee: method,
        this,
        args: Vec::new(),
    })
}

/// `Object.prototype.toString()` (§20.1.3.6).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(super::object::builtin_tag_text(
        rt,
        call.this(),
    )?))
}

/// `Object.prototype.valueOf()` (§20.1.3.7): `ToObject(this)`.
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(rt.to_object(call.this())?.into()))
}

/// `get Object.prototype.__proto__` (§B.2.2.1.1).
fn proto_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let proto = rt.heap.get_prototype_of(object)?;
    Ok(NativeReturn::Value(proto.map_or(Value::Null, Value::from)))
}

/// `set Object.prototype.__proto__` (§B.2.2.1.2).
fn proto_setter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.this().is_nullish() {
        return Err(VmError::type_error(
            "set Object.prototype.__proto__ called on null or undefined",
        ));
    }
    let proto = match rt.arg(call, 0) {
        Value::Object(proto) => Some(proto),
        Value::Null => None,
        _ => return Ok(NativeReturn::Value(Value::Undefined)),
    };
    let Value::Object(object) = call.this() else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    match rt.set_prototype_of(object, proto)? {
        ProtoSet::Done => Ok(NativeReturn::Value(Value::Undefined)),
        why => Err(rt.set_prototype_failure(object, why)),
    }
}

/// `Object.prototype.__defineGetter__(P, getter)` (§B.2.2.2).
fn define_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    define_accessor_half(rt, call, true)
}

/// `Object.prototype.__defineSetter__(P, setter)` (§B.2.2.3).
fn define_setter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    define_accessor_half(rt, call, false)
}

fn define_accessor_half(
    rt: &mut Runtime,
    call: &NativeCall,
    getter: bool,
) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let function = rt.arg(call, 1);
    if !rt.is_callable(function)? {
        let name = if getter {
            "__defineGetter__"
        } else {
            "__defineSetter__"
        };
        return Err(VmError::type_error(format!(
            "Object.prototype.{name}: Expecting function"
        )));
    }
    let key = rt.key_arg(rt.arg(call, 0))?;
    let desc = PropertyDescriptor {
        get: getter.then_some(function),
        set: (!getter).then_some(function),
        enumerable: Some(true),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    rt.define_or_throw(object, key, desc)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `Object.prototype.__lookupGetter__(P)` (§B.2.2.4).
fn lookup_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    lookup_accessor_half(rt, call, true)
}

/// `Object.prototype.__lookupSetter__(P)` (§B.2.2.5).
fn lookup_setter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    lookup_accessor_half(rt, call, false)
}

fn lookup_accessor_half(
    rt: &mut Runtime,
    call: &NativeCall,
    getter: bool,
) -> VmResult<NativeReturn> {
    let mut object = rt.this_object(call)?;
    let key = rt.key_arg(rt.arg(call, 0))?;
    loop {
        rt.tick()?;
        if let Some(property) = rt.own_property(object, key)? {
            let result = match property {
                crate::object::Property::Accessor { get, set, .. } => {
                    if getter {
                        get
                    } else {
                        set
                    }
                }
                crate::object::Property::Data { .. } => Value::Undefined,
            };
            return Ok(NativeReturn::Value(result));
        }
        match rt.heap.get_prototype_of(object)? {
            Some(proto) => object = proto,
            None => return Ok(NativeReturn::Value(Value::Undefined)),
        }
    }
}
