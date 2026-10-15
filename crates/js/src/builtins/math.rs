//! `Math` (ECMA-262 §21.3, with `sumPrecise` of ECMA-262 2026, which
//! Chromium 148 has): the value properties and all functions.
//!
//! The transcendental functions are Rust's `f64` methods (the platform's
//! libm); the specification lets them be approximations, and they can
//! differ from V8's in the last bit. `hypot` scales by the largest
//! magnitude and adds the squares with Kahan's compensated summation,
//! which gives V8's results (20,000 random argument lists measured in
//! Node.js 22, all equal). `max`, `min` and `hypot` convert all arguments
//! before they look at any value. `random` is xorshift128+
//! (`vm/random.rs`). `sumPrecise` and `f16round` are exact
//! (`js-text`).

use swb_js_text::number::{ExactSum, f16_round};

use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::{exponentiate, to_int32, to_uint32};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let math = rt.heap.new_object(Some(proto))?;
    rt.heap.record(math);
    super::to_string_tag(rt, math, "Math")?;
    // Chromium's order: the functions of ECMA-262 2024, the value
    // properties, then the newer functions.
    let functions: [(&str, u32, NativeFn); 35] = [
        ("abs", 1, abs),
        ("acos", 1, acos),
        ("acosh", 1, acosh),
        ("asin", 1, asin),
        ("asinh", 1, asinh),
        ("atan", 1, atan),
        ("atanh", 1, atanh),
        ("atan2", 2, atan2),
        ("ceil", 1, ceil),
        ("cbrt", 1, cbrt),
        ("expm1", 1, expm1),
        ("clz32", 1, clz32),
        ("cos", 1, cos),
        ("cosh", 1, cosh),
        ("exp", 1, exp),
        ("floor", 1, floor),
        ("fround", 1, fround),
        ("hypot", 2, hypot),
        ("imul", 2, imul),
        ("log", 1, log),
        ("log1p", 1, log1p),
        ("log2", 1, log2),
        ("log10", 1, log10),
        ("max", 2, max),
        ("min", 2, min),
        ("pow", 2, pow),
        ("random", 0, random),
        ("round", 1, round),
        ("sign", 1, sign),
        ("sin", 1, sin),
        ("sinh", 1, sinh),
        ("sqrt", 1, sqrt),
        ("tan", 1, tan),
        ("tanh", 1, tanh),
        ("trunc", 1, trunc),
    ];
    for (name, length, function) in functions {
        super::method(rt, realm, math, name, length, function)?;
    }
    for (name, value) in [
        ("E", std::f64::consts::E),
        ("LN10", std::f64::consts::LN_10),
        ("LN2", std::f64::consts::LN_2),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("PI", std::f64::consts::PI),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ] {
        let key = rt.heap.key_from_str(name)?;
        rt.define(math, key, Value::number(value), false, false, false)?;
    }
    super::method(rt, realm, math, "f16round", 1, f16round)?;
    super::method(rt, realm, math, "sumPrecise", 1, sum_precise)?;
    super::global(rt, realm, "Math", math.into())
}

/// `ToNumber` of argument `index`, without a conversion for a Number.
#[inline]
fn number_arg(rt: &mut Runtime, call: &NativeCall, index: usize) -> VmResult<f64> {
    match rt.arg(call, index) {
        Value::Int(i) => Ok(f64::from(i)),
        Value::Double(d) => Ok(d),
        value => rt.with_scope(|rt| rt.to_number(value)),
    }
}

/// A Number result of a native.
#[inline]
#[allow(clippy::unnecessary_wraps, reason = "the result type of the natives")]
fn result(value: f64) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::number(value)))
}

/// The functions of one argument that are a method of `f64` (or a small
/// function): `ToNumber`, then the operation.
macro_rules! unary {
    ($($(#[$doc:meta])* $name:ident => $op:expr;)*) => {
        $(
            $(#[$doc])*
            fn $name(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
                let op: fn(f64) -> f64 = $op;
                result(op(number_arg(rt, call, 0)?))
            }
        )*
    };
}

unary! {
    /// `Math.acos(x)` (§21.3.2.2).
    acos => f64::acos;
    /// `Math.acosh(x)` (§21.3.2.3).
    acosh => f64::acosh;
    /// `Math.asin(x)` (§21.3.2.4).
    asin => f64::asin;
    /// `Math.asinh(x)` (§21.3.2.5).
    asinh => f64::asinh;
    /// `Math.atan(x)` (§21.3.2.6).
    atan => f64::atan;
    /// `Math.atanh(x)` (§21.3.2.7).
    atanh => f64::atanh;
    /// `Math.cbrt(x)` (§21.3.2.9).
    cbrt => f64::cbrt;
    /// `Math.cos(x)` (§21.3.2.12).
    cos => f64::cos;
    /// `Math.cosh(x)` (§21.3.2.13).
    cosh => f64::cosh;
    /// `Math.exp(x)` (§21.3.2.14).
    exp => f64::exp;
    /// `Math.expm1(x)` (§21.3.2.15).
    expm1 => f64::exp_m1;
    /// `Math.fround(x)` (§21.3.2.17): the nearest binary32 value.
    fround => |x| f64::from(x as f32);
    /// `Math.f16round(x)` (ECMA-262 2025 §21.3.2.18).
    f16round => f16_round;
    /// `Math.log(x)` (§21.3.2.20).
    log => f64::ln;
    /// `Math.log1p(x)` (§21.3.2.21).
    log1p => f64::ln_1p;
    /// `Math.log10(x)` (§21.3.2.22).
    log10 => f64::log10;
    /// `Math.log2(x)` (§21.3.2.23).
    log2 => f64::log2;
    /// `Math.sin(x)` (§21.3.2.30).
    sin => f64::sin;
    /// `Math.sinh(x)` (§21.3.2.31).
    sinh => f64::sinh;
    /// `Math.sqrt(x)` (§21.3.2.32).
    sqrt => f64::sqrt;
    /// `Math.tan(x)` (§21.3.2.33).
    tan => f64::tan;
    /// `Math.tanh(x)` (§21.3.2.34).
    tanh => f64::tanh;
    /// `Math.sign(x)` (§21.3.2.29): NaN and ±0 stay, else ±1.
    sign => |x| if x.is_nan() || x == 0.0 { x } else { x.signum() };
}

/// The rounding functions keep an integer argument as it is.
macro_rules! rounding {
    ($($(#[$doc:meta])* $name:ident => $op:expr;)*) => {
        $(
            $(#[$doc])*
            fn $name(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
                if let Value::Int(i) = rt.arg(call, 0) {
                    return Ok(NativeReturn::Value(Value::Int(i)));
                }
                let op: fn(f64) -> f64 = $op;
                result(op(number_arg(rt, call, 0)?))
            }
        )*
    };
}

rounding! {
    /// `Math.ceil(x)` (§21.3.2.10).
    ceil => f64::ceil;
    /// `Math.floor(x)` (§21.3.2.16).
    floor => f64::floor;
    /// `Math.round(x)` (§21.3.2.28).
    round => round_half_up;
    /// `Math.trunc(x)` (§21.3.2.37).
    trunc => f64::trunc;
}

/// `Math.abs(x)` (§21.3.2.1).
fn abs(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if let Value::Int(i) = rt.arg(call, 0)
        && i != i32::MIN
    {
        return Ok(NativeReturn::Value(Value::Int(i.abs())));
    }
    result(number_arg(rt, call, 0)?.abs())
}

/// The rounding of `Math.round` (§21.3.2.28): the closest integer, the
/// one closer to +∞ on a tie; -0 for -0.5 ≤ x < 0, +0 for 0 < x < 0.5.
fn round_half_up(x: f64) -> f64 {
    if !x.is_finite() || x.trunc() == x {
        return x;
    }
    if x > 0.0 && x < 0.5 {
        return 0.0;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    // Both are exact for a non-integral double (|x| < 2^52).
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.atan2(y, x)` (§21.3.2.8).
fn atan2(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let y = number_arg(rt, call, 0)?;
    let x = number_arg(rt, call, 1)?;
    result(y.atan2(x))
}

/// `Math.pow(base, exponent)` (§21.3.2.26): `Number::exponentiate`, as
/// the `**` operator computes it. Both arguments are converted first.
fn pow(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let base = number_arg(rt, call, 0)?;
    let exponent = number_arg(rt, call, 1)?;
    result(exponentiate(base, exponent))
}

/// `Math.clz32(x)` (§21.3.2.11).
fn clz32(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let n = to_uint32(number_arg(rt, call, 0)?);
    Ok(NativeReturn::Value(Value::Int(n.leading_zeros() as i32)))
}

/// `Math.imul(x, y)` (§21.3.2.19).
fn imul(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let a = to_int32(number_arg(rt, call, 0)?);
    let b = to_int32(number_arg(rt, call, 1)?);
    Ok(NativeReturn::Value(Value::Int(a.wrapping_mul(b))))
}

/// `Math.max(...args)` (§21.3.2.24) and `Math.min` (§21.3.2.25): every
/// argument is converted first, in order; NaN wins, and +0 is larger
/// than -0. Charged one time step per argument.
fn extreme(rt: &mut Runtime, call: &NativeCall, larger: bool) -> VmResult<NativeReturn> {
    rt.charge(call.argc())?;
    let mut best = if larger {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    };
    let mut nan = false;
    for index in 0..call.argc() {
        let n = number_arg(rt, call, index)?;
        if n.is_nan() {
            nan = true;
        } else if (larger && (n > best || (n == 0.0 && best == 0.0 && best.is_sign_negative())))
            || (!larger && (n < best || (n == 0.0 && best == 0.0 && n.is_sign_negative())))
        {
            best = n;
        }
    }
    result(if nan { f64::NAN } else { best })
}

/// `Math.max(...args)` (§21.3.2.24).
fn max(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    extreme(rt, call, true)
}

/// `Math.min(...args)` (§21.3.2.25).
fn min(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    extreme(rt, call, false)
}

/// `Math.hypot(...args)` (§21.3.2.18): the arguments are converted in
/// place (the value stack keeps them), then an infinity gives +∞, NaN
/// gives NaN, and otherwise the square root of the sum of squares, scaled
/// by the largest magnitude and summed with Kahan's compensation (V8's
/// results). Charged two time steps per argument.
fn hypot(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    rt.charge(call.argc() * 2)?;
    let mut largest = 0.0f64;
    let mut infinite = false;
    let mut nan = false;
    for index in 0..call.argc() {
        let n = number_arg(rt, call, index)?;
        rt.set_arg(call, index, Value::Double(n))?;
        infinite |= n.is_infinite();
        nan |= n.is_nan();
        largest = largest.max(n.abs());
    }
    if infinite {
        return result(f64::INFINITY);
    }
    if nan {
        return result(f64::NAN);
    }
    if largest == 0.0 {
        return result(0.0);
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for index in 0..call.argc() {
        let n = rt.arg(call, index).as_number().unwrap_or(0.0).abs() / largest;
        let term = n * n - compensation;
        let total = sum + term;
        compensation = (total - sum) - term;
        sum = total;
    }
    result(sum.sqrt() * largest)
}

/// `Math.random()` (§21.3.2.27).
fn random(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    result(rt.vm.random.next_double())
}

/// `Math.sumPrecise(items)` (ECMA-262 2026 §21.3.2.34): the exact sum of
/// the Numbers of an iterable, rounded once; V8's messages.
fn sum_precise(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let items = rt.arg(call, 0);
    if items.is_nullish() {
        return Err(VmError::type_error(
            "Math.sumPrecise called on null or undefined",
        ));
    }
    // GetIterator (§7.4.3), with V8's message for this function.
    let key = rt.symbol_key(WellKnown::Iterator);
    let method = rt.get_value(items, key)?;
    if !rt.is_callable(method)? {
        let text = match items {
            Value::Object(object) => rt.object_text(object),
            Value::Bool(_) => "[object Boolean]".to_owned(),
            Value::String(_) => "[object String]".to_owned(),
            Value::Symbol(_) => "[object Symbol]".to_owned(),
            Value::BigInt(_) => "[object BigInt]".to_owned(),
            _ => "[object Number]".to_owned(),
        };
        return Err(VmError::type_error(format!("{text} is not iterable")));
    }
    let mut record = rt.get_iterator_from_method(items, method)?;
    let mut sum = ExactSum::new();
    // NaN, +∞, -∞, -0 (nothing but -0 so far) or a finite sum.
    let mut state = State::MinusZero;
    loop {
        let value = rt.with_scope(|rt| rt.iterator_step_value(&mut record))?;
        let Some(value) = value else {
            break;
        };
        let Some(n) = value.as_number() else {
            let error = VmError::type_error(format!(
                "Type of 'Iterator value' must be 'number', found '{}'",
                type_name(value)
            ));
            return Err(rt.iterator_close_after_error(&record, error));
        };
        state = match state {
            State::NaN => State::NaN,
            _ if n.is_nan() => State::NaN,
            State::PlusInfinity if n == f64::NEG_INFINITY => State::NaN,
            State::MinusInfinity if n == f64::INFINITY => State::NaN,
            State::PlusInfinity | State::MinusInfinity => state,
            _ if n == f64::INFINITY => State::PlusInfinity,
            _ if n == f64::NEG_INFINITY => State::MinusInfinity,
            _ if n == 0.0 && n.is_sign_negative() => state,
            _ => {
                sum.add(n);
                State::Finite
            }
        };
    }
    result(match state {
        State::NaN => f64::NAN,
        State::PlusInfinity => f64::INFINITY,
        State::MinusInfinity => f64::NEG_INFINITY,
        State::MinusZero => -0.0,
        State::Finite => sum.value(),
    })
}

/// The state of `Math.sumPrecise`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    NaN,
    PlusInfinity,
    MinusInfinity,
    MinusZero,
    Finite,
}

/// `typeof` of a primitive for V8's message.
fn type_name(value: Value) -> &'static str {
    match value {
        Value::Undefined | Value::Empty | Value::Cell(_) => "undefined",
        Value::Null | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Int(_) | Value::Double(_) => "number",
        Value::String(_) => "string",
        Value::Symbol(_) => "symbol",
        Value::BigInt(_) => "bigint",
    }
}
