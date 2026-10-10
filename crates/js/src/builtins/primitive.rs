//! `String` and `Number` (ECMA-262 §22.1, §21.1): the functions (type
//! conversions), the constructors (wrapper objects) and the prototype
//! methods `toString` and `valueOf`. `Boolean` is in [`super::boolean`].

use swb_js_text::String16;

use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::number::number_to_string;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::StringPrototype)?;
    super::constructor(rt, realm, "String", 1, string, proto)?;
    super::method(rt, realm, proto, "toString", 0, string_value_of)?;
    super::method(rt, realm, proto, "valueOf", 0, string_value_of)?;
    let proto = rt.intrinsic(realm, Intrinsic::NumberPrototype)?;
    super::constructor(rt, realm, "Number", 1, number, proto)?;
    super::method(rt, realm, proto, "toString", 1, number_to_string_method)?;
    super::method(rt, realm, proto, "valueOf", 0, number_value_of)?;
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

/// `Number(value)` (§21.1.1.1): `ToNumber` (`BigInt` is M7); with `new`,
/// a Number object.
fn number(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let n = if call.argc() == 0 {
        0.0
    } else {
        rt.to_number(rt.arg(call, 0))?
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

/// `thisNumberValue` (§21.1.3.7.1).
fn this_number(rt: &Runtime, call: &NativeCall) -> VmResult<f64> {
    match call.this() {
        Value::Int(i) => Ok(f64::from(i)),
        Value::Double(d) => Ok(d),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::NumberWrapper(n) => Ok(n),
            _ => Err(requires(rt, call, "Number")),
        },
        _ => Err(requires(rt, call, "Number")),
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

/// `Number.prototype.valueOf()` (§21.1.3.7).
fn number_value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::number(this_number(rt, call)?)))
}

/// `Number.prototype.toString(radix)` (§21.1.3.6). Radix 10 is
/// `Number::toString`; other radices give the exact digits of integers
/// below 2^53 and of fractions that end within 52 digits (binary
/// fractions in power-of-two radices). Deviation: other fractions get up
/// to 52 digits, not the shortest digits that V8 prints (M7).
fn number_to_string_method(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let x = this_number(rt, call)?;
    let radix = match rt.arg(call, 0) {
        Value::Undefined => 10.0,
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    if !(2.0..=36.0).contains(&radix) {
        return Err(VmError::range_error(
            "toString() radix argument must be between 2 and 36",
        ));
    }
    let text = if radix == 10.0 || !x.is_finite() {
        String16::from(number_to_string(x).as_str())
    } else {
        String16::from(to_radix(x, radix as u32).as_str())
    };
    Ok(NativeReturn::Value(rt.heap.alloc_string(text)?.into()))
}

/// The digits of a finite number in a radix other than 10.
fn to_radix(x: f64, radix: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let digit = |d: f64| char::from(DIGITS.get(d as usize).copied().unwrap_or(b'0'));
    let negative = x < 0.0;
    let x = x.abs();
    let base = f64::from(radix);
    let mut integer = x.trunc();
    let mut fraction = x - integer;
    let mut int_digits = Vec::new();
    loop {
        let d = integer % base;
        int_digits.push(digit(d));
        integer = ((integer - d) / base).trunc();
        if integer < 1.0 {
            break;
        }
    }
    let mut text = String::new();
    if negative {
        text.push('-');
    }
    text.extend(int_digits.iter().rev());
    if fraction > 0.0 {
        text.push('.');
        for _ in 0..52 {
            fraction *= base;
            let d = fraction.trunc();
            text.push(digit(d));
            fraction -= d;
            if fraction <= 0.0 {
                break;
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::to_radix;

    #[test]
    fn radix_digits() {
        assert_eq!(to_radix(255.0, 16), "ff");
        assert_eq!(to_radix(-255.0, 36), "-73");
        assert_eq!(to_radix(0.5, 2), "0.1");
        assert_eq!(to_radix(3.75, 2), "11.11");
        assert_eq!(to_radix(1e21, 16), "3635c9adc5dea00000");
        assert_eq!(to_radix(0.0, 7), "0");
    }
}
