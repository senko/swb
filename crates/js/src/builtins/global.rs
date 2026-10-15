//! The global functions `eval`, `isNaN`, `isFinite`, `parseFloat` and
//! `parseInt` (ECMA-262 §19.2.1 to §19.2.5). The value properties
//! (`NaN`, `Infinity`, `undefined`, `globalThis`) are defined with the
//! global object (`vm/realm.rs`). The parsers are in `js-text`
//! ([`swb_js_text::number`]).

use swb_js_text::number;

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_int32;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmResult};

/// Installs the global functions; returns `parseInt` and `parseFloat`,
/// which `Number` shares.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<(Gc<Object>, Gc<Object>)> {
    let eval = rt.new_builtin(realm, "eval", 1, crate::vm::eval_function, false)?;
    super::global(rt, realm, "eval", eval.into())?;
    if let Some(record) = rt.vm.realms.get_mut(realm as usize) {
        record.set_intrinsic(Intrinsic::Eval, eval);
    }
    let is_nan = rt.new_builtin(realm, "isNaN", 1, is_nan, false)?;
    super::global(rt, realm, "isNaN", is_nan.into())?;
    let is_finite = rt.new_builtin(realm, "isFinite", 1, is_finite, false)?;
    super::global(rt, realm, "isFinite", is_finite.into())?;
    let parse_float = rt.new_builtin(realm, "parseFloat", 1, parse_float, false)?;
    super::global(rt, realm, "parseFloat", parse_float.into())?;
    let parse_int = rt.new_builtin(realm, "parseInt", 2, parse_int, false)?;
    super::global(rt, realm, "parseInt", parse_int.into())?;
    Ok((parse_int, parse_float))
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

/// `parseFloat(string)` (§19.2.4), charged for the scan of the text and
/// the rounding of its decimal digits.
fn parse_float(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.to_string(rt.arg(call, 0))?;
    let text = rt.heap.string(string)?;
    let units = text.len();
    let value = number::parse_float(text.as_str16());
    rt.charge_number_scan(units)?;
    rt.charge_decimal_digits(units)?;
    Ok(NativeReturn::Value(Value::number(value)))
}

/// `parseInt(string, radix)` (§19.2.5): `ToString` before `ToInt32`;
/// charged for the scan of the text, and for the exact value: the
/// rounding of the decimal digits in radix 10 (or 0), the multi-precision
/// integer in the radixes that are not powers of two.
fn parse_int(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.to_string(rt.arg(call, 0))?;
    let radix = match rt.arg(call, 1) {
        Value::Int(i) => i,
        Value::Undefined => 0,
        value => to_int32(rt.to_number(value)?),
    };
    let text = rt.heap.string(string)?;
    let units = text.len();
    let value = number::parse_int(text.as_str16(), radix);
    rt.charge_number_scan(units)?;
    if radix == 0 || radix == 10 {
        rt.charge_decimal_digits(units)?;
    } else if (3..=36).contains(&radix) && !radix.unsigned_abs().is_power_of_two() {
        rt.charge(units.min(EXACT_RADIX_UNITS) / EXACT_RADIX_UNITS_PER_STEP)?;
    }
    Ok(NativeReturn::Value(Value::number(value)))
}

/// The most digits that `parseInt` accumulates exactly in a radix that is
/// not a power of two (647 in radix 3; more digits are infinite).
const EXACT_RADIX_UNITS: usize = 700;

/// The digits of the exact accumulation per time step (a step is about
/// 12 ns): 646 digits in radix 3 take about 0.9 µs, 198 in radix 36
/// about 0.5 µs, scan included.
const EXACT_RADIX_UNITS_PER_STEP: usize = 4;
