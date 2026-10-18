//! `Number` (ECMA-262 §21.1): the constructor, the value properties, the
//! static functions (`Number.parseFloat` and `Number.parseInt` are the
//! global functions) and the prototype methods. The digits come from
//! `js-text` ([`swb_js_text::number`]): exact for `toFixed`,
//! `toExponential` and `toPrecision`, the shortest digits that read back
//! (the closest of them) for `toString` in radixes other than 10,
//! Chromium's en-US output for `toLocaleString` (no ECMA-402; the
//! arguments are ignored, ADR 0026 section 15).

use swb_js_text::number;

use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::bigint::Numeric;
use crate::vm::convert::{MAX_SAFE_INTEGER, to_integer_or_infinity};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// Installs `Number`; `parse_int` and `parse_float` are the global
/// functions, which `Number` shares.
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    parse_int: Gc<Object>,
    parse_float: Gc<Object>,
) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::NumberPrototype)?;
    let ctor = super::constructor(rt, realm, "Number", 1, number, proto)?;
    rt.heap.record(ctor);
    super::method(rt, realm, ctor, "isFinite", 1, is_finite)?;
    super::method(rt, realm, ctor, "isInteger", 1, is_integer)?;
    super::method(rt, realm, ctor, "isNaN", 1, is_nan)?;
    super::method(rt, realm, ctor, "isSafeInteger", 1, is_safe_integer)?;
    for (name, function) in [("parseFloat", parse_float), ("parseInt", parse_int)] {
        let key = rt.heap.key_from_str(name)?;
        rt.define(ctor, key, function.into(), true, false, true)?;
    }
    // V8's order: the functions, then the value properties.
    for (name, value) in [
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", f64::from_bits(1)),
        ("NaN", f64::NAN),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("MAX_SAFE_INTEGER", MAX_SAFE_INTEGER),
        ("MIN_SAFE_INTEGER", -MAX_SAFE_INTEGER),
        ("EPSILON", f64::EPSILON),
    ] {
        let key = rt.heap.key_from_str(name)?;
        rt.define(ctor, key, Value::number(value), false, false, false)?;
    }
    super::method(rt, realm, proto, "toExponential", 1, to_exponential)?;
    super::method(rt, realm, proto, "toFixed", 1, to_fixed)?;
    super::method(rt, realm, proto, "toPrecision", 1, to_precision)?;
    super::method(rt, realm, proto, "toString", 1, to_string)?;
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    super::method(rt, realm, proto, "toLocaleString", 0, to_locale_string)?;
    Ok(())
}

/// `Number(value)` (§21.1.1.1): `ToNumeric`, a `BigInt` converted to the
/// nearest Number; with `new`, a Number object.
fn number(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let n = if call.argc() == 0 {
        0.0
    } else {
        match rt.to_numeric(rt.arg(call, 0))? {
            Numeric::Number(n) => n,
            Numeric::BigInt(b) => rt.bigint_to_number(b)?,
        }
    };
    if call.new_target().is_undefined() {
        return Ok(NativeReturn::Value(Value::number(n)));
    }
    let proto = rt.prototype_from(call, Intrinsic::NumberPrototype)?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::NumberWrapper(n))?;
    Ok(NativeReturn::Value(object.into()))
}

/// Argument `index` if it is a Number (the static predicates do not
/// convert).
fn number_value(rt: &Runtime, call: &NativeCall, index: usize) -> Option<f64> {
    rt.arg(call, index).as_number()
}

/// A Boolean result of a native.
#[allow(clippy::unnecessary_wraps, reason = "the result type of the natives")]
fn boolean(value: bool) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::Bool(value)))
}

/// `Number.isFinite(number)` (§21.1.2.2).
fn is_finite(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    boolean(number_value(rt, call, 0).is_some_and(f64::is_finite))
}

/// `Number.isInteger(number)` (§21.1.2.3): `IsIntegralNumber`.
fn is_integer(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    boolean(number_value(rt, call, 0).is_some_and(is_integral))
}

/// `IsIntegralNumber` (§7.2.6) of a Number.
fn is_integral(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

/// `Number.isNaN(number)` (§21.1.2.4).
fn is_nan(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    boolean(number_value(rt, call, 0).is_some_and(f64::is_nan))
}

/// `Number.isSafeInteger(number)` (§21.1.2.5).
fn is_safe_integer(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    boolean(
        number_value(rt, call, 0).is_some_and(|x| is_integral(x) && x.abs() <= MAX_SAFE_INTEGER),
    )
}

/// `ThisNumberValue` (§21.1.3.7.1).
fn this_number(rt: &Runtime, call: &NativeCall) -> VmResult<f64> {
    match call.this() {
        Value::Int(i) => Ok(f64::from(i)),
        Value::Double(d) => Ok(d),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::NumberWrapper(n) => Ok(n),
            _ => Err(super::primitive::requires(rt, call, "Number")),
        },
        _ => Err(super::primitive::requires(rt, call, "Number")),
    }
}

/// A string result, charged `steps` time steps per eight characters of
/// the text, in proportion to the work that made it.
fn text_result(rt: &mut Runtime, text: &str, steps: usize) -> VmResult<NativeReturn> {
    rt.charge(text.len() * steps / 8)?;
    Ok(NativeReturn::Value(rt.heap.alloc_str(text)?.into()))
}

/// The time steps per eight characters (a step is about 12 ns) of the
/// exact formats: a digit costs a multi-precision step of up to 35 limbs,
/// about 60 ns (`(5e-324).toPrecision(100)` takes 6.5 µs).
const EXACT_STEPS: usize = 40;

/// The steps per eight characters of `toString` in radixes other than 10,
/// from the cost per character measured in release builds (a step is
/// about 12 ns): `Math.random().toString(36)` takes about 240 ns for 13
/// characters (12 steps per eight), `(1e-300).toString(36)` 2.7 µs for
/// 204 (9), `Number.MAX_VALUE.toString(3)` 4.4 µs for 647 (4.5),
/// `Number.MIN_VALUE.toString(2)` 2.3 µs for 1,076 (1.4). The highest
/// rate, that of the short strings, covers nearly all of them (the
/// review measured up to 1.13 times the charge for values near 1e30 in
/// radix 13).
const RADIX_STEPS: usize = 12;

/// The steps per eight characters of `toLocaleString`, about 5 ns per
/// character.
const LOCALE_STEPS: usize = 4;

/// `ToIntegerOrInfinity` of argument `index`.
fn integer_arg(rt: &mut Runtime, call: &NativeCall, index: usize) -> VmResult<f64> {
    let value = rt.arg(call, index);
    Ok(to_integer_or_infinity(rt.to_number(value)?))
}

/// `Number.prototype.toExponential(fractionDigits)` (§21.1.3.2).
fn to_exponential(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    let undefined = rt.arg(call, 0).is_undefined();
    let f = integer_arg(rt, call, 0)?;
    if !x.is_finite() {
        return text_result(rt, &number::number_to_string(x), 0);
    }
    if !(0.0..=100.0).contains(&f) {
        return Err(VmError::range_error(
            "toExponential() argument must be between 0 and 100",
        ));
    }
    let digits = (!undefined).then_some(f as u32);
    text_result(rt, &number::to_exponential(x, digits), EXACT_STEPS)
}

/// `Number.prototype.toFixed(fractionDigits)` (§21.1.3.3).
fn to_fixed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    let f = integer_arg(rt, call, 0)?;
    if !(0.0..=100.0).contains(&f) {
        return Err(VmError::range_error(
            "toFixed() digits argument must be between 0 and 100",
        ));
    }
    if !x.is_finite() {
        return text_result(rt, &number::number_to_string(x), 0);
    }
    text_result(rt, &number::to_fixed(x, f as u32), EXACT_STEPS)
}

/// `Number.prototype.toLocaleString()` (§21.1.3.4) without ECMA-402:
/// Chromium's en-US output; the arguments are ignored.
fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    text_result(rt, &number::locale_string(x), LOCALE_STEPS)
}

/// `Number.prototype.toPrecision(precision)` (§21.1.3.5).
fn to_precision(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    if rt.arg(call, 0).is_undefined() {
        return text_result(rt, &number::number_to_string(x), 0);
    }
    let p = integer_arg(rt, call, 0)?;
    if !x.is_finite() {
        return text_result(rt, &number::number_to_string(x), 0);
    }
    if !(1.0..=100.0).contains(&p) {
        return Err(VmError::range_error(
            "toPrecision() argument must be between 1 and 100",
        ));
    }
    text_result(rt, &number::to_precision(x, p as u32), EXACT_STEPS)
}

/// `Number.prototype.toString(radix)` (§21.1.3.6).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    let radix = match rt.arg(call, 0) {
        Value::Undefined => 10.0,
        _ => integer_arg(rt, call, 0)?,
    };
    if !(2.0..=36.0).contains(&radix) {
        return Err(VmError::range_error(
            "toString() radix argument must be between 2 and 36",
        ));
    }
    if radix == 10.0 {
        return text_result(rt, &number::number_to_string(x), 0);
    }
    text_result(rt, &number::radix_string(x, radix as u32), RADIX_STEPS)
}

/// `Number.prototype.valueOf()` (§21.1.3.7).
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::number(this_number(rt, call)?)))
}
