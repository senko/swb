//! `BigInt` (ECMA-262 §21.2, <https://tc39.es/ecma262/#sec-bigint-objects>):
//! the function (a conversion; `new BigInt` throws), `BigInt.asIntN`,
//! `BigInt.asUintN` and the prototype methods `toLocaleString`,
//! `toString`, `valueOf` and `@@toStringTag`. The operators and the
//! conversions are in `vm/bigint.rs` and `vm/convert.rs`; the arithmetic
//! in `js-text`.
//!
//! `toLocaleString` has no ECMA-402: it ignores its arguments and groups
//! the digits as Chromium does with its default locale (en-US), like
//! `Number.prototype.toLocaleString` (ADR 0026 section 15).

use swb_js_text::bigint::{self, cost};

use crate::heap::Gc;
use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::value::{BigInt, Value};
use crate::vm::bigint::bigint_error;
use crate::vm::convert::{Hint, MAX_SAFE_INTEGER, to_integer_or_infinity};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// V8's message for a bit count that `ToIndex` rejects.
const BAD_INDEX: &str = "Invalid value: not (convertible to) a safe integer";

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::BigIntPrototype)?;
    let ctor = super::constructor(rt, realm, "BigInt", 1, bigint_function, proto)?;
    rt.heap.record(ctor);
    // V8's order of the static functions.
    super::method(rt, realm, ctor, "asUintN", 2, as_uint_n)?;
    super::method(rt, realm, ctor, "asIntN", 2, as_int_n)?;
    super::method(rt, realm, proto, "toLocaleString", 0, to_locale_string)?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    super::to_string_tag(rt, proto, "BigInt")
}

/// `BigInt(value)` (§21.2.1.1): `NumberToBigInt` of a Number, else
/// `ToBigInt` of the primitive; `new BigInt` is a `TypeError`.
fn bigint_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if !call.new_target().is_undefined() {
        return Err(VmError::type_error("BigInt is not a constructor"));
    }
    let primitive = rt.to_primitive(rt.arg(call, 0), Hint::Number)?;
    rt.heap.record(primitive);
    let result = match primitive.as_number() {
        Some(number) => rt.number_to_bigint(number)?,
        None => rt.primitive_to_bigint(primitive)?,
    };
    Ok(NativeReturn::Value(result.into()))
}

/// `ToIndex` (§7.1.22) of the bit count of `asIntN` and `asUintN`.
fn bit_count(rt: &mut Runtime, value: Value) -> VmResult<u64> {
    let integer = to_integer_or_infinity(rt.to_number(value)?);
    if !(0.0..=MAX_SAFE_INTEGER).contains(&integer) {
        return Err(VmError::range_error(BAD_INDEX));
    }
    Ok(integer as u64)
}

/// The common part of `asIntN` and `asUintN`: `ToIndex(bits)`, then
/// `ToBigInt(bigint)`, then the operation, charged.
fn as_n(
    rt: &mut Runtime,
    call: &NativeCall,
    op: fn(u64, &BigInt) -> bigint::Result<BigInt>,
) -> VmResult<NativeReturn> {
    let bits = bit_count(rt, rt.arg(call, 0))?;
    let x = rt.to_bigint(rt.arg(call, 1))?;
    let units = cost::as_n(bits, rt.heap.bigint(x)?);
    rt.charge_bigint(units)?;
    let result = op(bits, rt.heap.bigint(x)?).map_err(bigint_error)?;
    Ok(NativeReturn::Value(rt.new_bigint(result)?.into()))
}

/// `BigInt.asIntN(bits, bigint)` (§21.2.2.1).
fn as_int_n(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    as_n(rt, call, bigint::as_int_n)
}

/// `BigInt.asUintN(bits, bigint)` (§21.2.2.2).
fn as_uint_n(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    as_n(rt, call, bigint::as_uint_n)
}

/// `ThisBigIntValue` (§21.2.3.4.1), with V8's message.
fn this_bigint(rt: &Runtime, call: &NativeCall) -> VmResult<Gc<BigInt>> {
    match call.this() {
        Value::BigInt(b) => Ok(b),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::BigIntWrapper(b) => Ok(b),
            _ => Err(super::primitive::requires(rt, call, "BigInt")),
        },
        _ => Err(super::primitive::requires(rt, call, "BigInt")),
    }
}

/// `BigInt.prototype.toLocaleString()` (§21.2.3.2) without ECMA-402.
fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_bigint(rt, call)?;
    Ok(NativeReturn::Value(rt.bigint_locale_string(x)?.into()))
}

/// `BigInt.prototype.toString(radix)` (§21.2.3.3): the radix is checked
/// before the conversion.
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_bigint(rt, call)?;
    let radix = match rt.arg(call, 0) {
        Value::Undefined => 10.0,
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    if !(2.0..=36.0).contains(&radix) {
        return Err(VmError::range_error(
            "toString() radix argument must be between 2 and 36",
        ));
    }
    Ok(NativeReturn::Value(
        rt.bigint_to_string(x, radix as u32)?.into(),
    ))
}

/// `BigInt.prototype.valueOf()` (§21.2.3.4).
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(this_bigint(rt, call)?.into()))
}
