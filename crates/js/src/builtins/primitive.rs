//! `String` (ECMA-262 §22.1): the function (a type conversion), the
//! constructor (wrapper objects) and the prototype methods `toString` and
//! `valueOf`; and V8's message for a receiver of the wrong type, which the
//! other wrappers share. `Number` is in [`super::number`], `Boolean` in
//! [`super::boolean`].

use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::StringPrototype)?;
    super::constructor(rt, realm, "String", 1, string, proto)?;
    super::method(rt, realm, proto, "toString", 0, string_value_of)?;
    super::method(rt, realm, proto, "valueOf", 0, string_value_of)?;
    Ok(())
}

/// `String(value)` (§22.1.1.1): `ToString`, except that a symbol becomes
/// its descriptive string when called without `new`; with `new`, a String
/// object.
fn string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let construct = !call.new_target().is_undefined();
    let text = match rt.arg(call, 0) {
        _ if call.argc() == 0 => rt.vm.atoms.empty,
        Value::Symbol(symbol) if !construct => super::symbol::descriptive_string(rt, symbol)?,
        value => rt.to_string(value)?,
    };
    if !construct {
        return Ok(NativeReturn::Value(text.into()));
    }
    rt.heap.record(text);
    let proto = rt.prototype_from(call, Intrinsic::StringPrototype)?;
    let length = rt.heap.string(text)?.len();
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::StringWrapper(text))?;
    rt.heap.record(object);
    let key = PropertyKey::String(rt.heap.length_atom());
    rt.define(
        object,
        key,
        Value::number(length as f64),
        false,
        false,
        false,
    )?;
    Ok(NativeReturn::Value(object.into()))
}

/// `String.prototype.toString()` and `valueOf()` (§22.1.3.32, §22.1.3.35):
/// `thisStringValue`.
fn string_value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    match call.this() {
        value @ Value::String(_) => Ok(NativeReturn::Value(value)),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::StringWrapper(string) => Ok(NativeReturn::Value(string.into())),
            _ => Err(requires(rt, call, "String")),
        },
        _ => Err(requires(rt, call, "String")),
    }
}

/// V8's `TypeError` for a receiver of the wrong type
/// ("Number.prototype.valueOf requires that 'this' be a Number").
pub(super) fn requires(rt: &Runtime, call: &NativeCall, type_name: &str) -> VmError {
    let key = PropertyKey::String(rt.vm.atoms.name);
    let method = match rt.heap.get_own_property(call.callee(), key) {
        Ok(Some(crate::object::Property::Data {
            value: Value::String(name),
            ..
        })) => rt.name_text(name),
        _ => String::new(),
    };
    VmError::type_error(format!(
        "{type_name}.prototype.{method} requires that 'this' be a {type_name}"
    ))
}
