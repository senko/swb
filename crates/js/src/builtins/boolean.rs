//! `Boolean` (ECMA-262 §20.3): the constructor (a Boolean object with
//! `new`) and the prototype methods `toString` and `valueOf`.

use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::BooleanPrototype)?;
    super::constructor(rt, realm, "Boolean", 1, boolean, proto)?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    Ok(())
}

/// `Boolean(value)` (§20.3.1.1): `ToBoolean`; with `new`, a Boolean
/// object whose prototype comes from `new.target`.
fn boolean(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.to_boolean(rt.arg(call, 0))?;
    if call.new_target().is_undefined() {
        return Ok(NativeReturn::Value(Value::Bool(value)));
    }
    let proto = rt.prototype_from(call, Intrinsic::BooleanPrototype)?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::BooleanWrapper(value))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `thisBooleanValue` (§20.3.3.3.1).
fn this_boolean(rt: &Runtime, call: &NativeCall) -> VmResult<bool> {
    match call.this() {
        Value::Bool(b) => Ok(b),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::BooleanWrapper(b) => Ok(b),
            _ => Err(super::primitive::requires(rt, call, "Boolean")),
        },
        _ => Err(super::primitive::requires(rt, call, "Boolean")),
    }
}

/// `Boolean.prototype.toString()` (§20.3.3.2).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let atoms = &rt.vm.atoms;
    let text = if this_boolean(rt, call)? {
        atoms.true_
    } else {
        atoms.false_
    };
    Ok(NativeReturn::Value(text.into()))
}

/// `Boolean.prototype.valueOf()` (§20.3.3.3).
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::Bool(this_boolean(rt, call)?)))
}
