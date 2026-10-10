//! The global functions `isNaN` and `isFinite` (ECMA-262 §19.2.2,
//! §19.2.3) and a `Math` object with `pow` only (§21.3.2.26;
//! `propertyHelper.js` of test262 needs it; the rest of `Math` is M7
//! feature 4).

use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::exponentiate;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let is_nan = rt.new_builtin(realm, "isNaN", 1, is_nan, false)?;
    super::global(rt, realm, "isNaN", is_nan.into())?;
    let is_finite = rt.new_builtin(realm, "isFinite", 1, is_finite, false)?;
    super::global(rt, realm, "isFinite", is_finite.into())?;
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let math = rt.heap.new_object(Some(proto))?;
    rt.heap.record(math);
    super::method(rt, realm, math, "pow", 2, pow)?;
    super::to_string_tag(rt, math, "Math")?;
    super::global(rt, realm, "Math", math.into())
}

/// `isNaN(number)` (§19.2.3).
fn is_nan(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let number = rt.to_number(rt.arg(call, 0))?;
    Ok(NativeReturn::Value(Value::Bool(number.is_nan())))
}

/// `isFinite(number)` (§19.2.2).
fn is_finite(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let number = rt.to_number(rt.arg(call, 0))?;
    Ok(NativeReturn::Value(Value::Bool(number.is_finite())))
}

/// `Math.pow(base, exponent)` (§21.3.2.26): `Number::exponentiate`, as
/// the `**` operator computes it. Both arguments are converted first.
fn pow(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let base = rt.to_number(rt.arg(call, 0))?;
    let exponent = rt.to_number(rt.arg(call, 1))?;
    Ok(NativeReturn::Value(Value::number(exponentiate(
        base, exponent,
    ))))
}
