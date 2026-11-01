//! The text forms of `Date.prototype` (ECMA-262 §21.4.4.35 to §21.4.4.45),
//! `toJSON` (§21.4.4.37) and `@@toPrimitive` (§21.4.4.45), and the
//! `toLocale*String` methods: Chromium 148's en-US output without
//! arguments, `Intl.DateTimeFormat` with a locale or options (ADR 0027
//! sections 7 and 10).
//! The formats themselves are in `crate::date::format`.

use std::borrow::Cow;

use super::intl_datetime::{self, Defaults, Required};
use crate::date::format;
use crate::date::time_value::{Fields, fields};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::Hint;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

/// `ToDateString(tv)` (§21.4.4.41.4): `Invalid Date` for NaN, else the
/// local date, time and zone.
pub(super) fn date_to_string(rt: &mut Runtime, t: f64) -> String {
    if t.is_nan() {
        return "Invalid Date".to_owned();
    }
    let time = t as i64;
    let zone = rt.vm.time.zone();
    let offset = zone.offset_at_utc(time);
    let name = zone.display_name_at_utc(time);
    format::to_string(&fields(time + offset), offset, &name)
}

/// The text of `this` Date object by `render`, or `Invalid Date`.
fn text_of(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &str,
    render: fn(&mut Runtime, i64) -> String,
) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date(call, method)?;
    if t.is_nan() {
        return rt.text_result("Invalid Date");
    }
    let text = render(rt, t as i64);
    rt.text_result(&text)
}

/// The local fields of the UTC time `t` with the offset and the zone name.
fn local(rt: &mut Runtime, t: i64) -> (Fields, i64, Cow<'static, str>) {
    let zone = rt.vm.time.zone();
    let offset = zone.offset_at_utc(t);
    (fields(t + offset), offset, zone.display_name_at_utc(t))
}

/// `Date.prototype.toString()` (§21.4.4.41).
pub(super) fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    text_of(rt, call, "toString", |rt, t| {
        let (f, offset, name) = local(rt, t);
        format::to_string(&f, offset, &name)
    })
}

/// `Date.prototype.toDateString()` (§21.4.4.35).
pub(super) fn to_date_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    text_of(rt, call, "toDateString", |rt, t| {
        let zone = rt.vm.time.zone();
        format::date_string(&fields(t + zone.offset_at_utc(t)))
    })
}

/// `Date.prototype.toTimeString()` (§21.4.4.42).
pub(super) fn to_time_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    text_of(rt, call, "toTimeString", |rt, t| {
        let (f, offset, name) = local(rt, t);
        format::to_time_string(&f, offset, &name)
    })
}

/// `Date.prototype.toUTCString()` (§21.4.4.43); `toGMTString` is the same
/// function object (§B.2.3.3).
pub(super) fn to_utc_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    text_of(rt, call, "toUTCString", |_, t| {
        format::to_utc_string(&fields(t))
    })
}

/// `Date.prototype.toISOString()` (§21.4.4.36): a `RangeError` for an
/// invalid date.
pub(super) fn to_iso_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date(call, "toISOString")?;
    if t.is_nan() {
        return Err(VmError::range_error("Invalid time value"));
    }
    rt.text_result(&format::to_iso_string(&fields(t as i64)))
}

/// The text of `toLocaleString` and its two relatives (ECMA-402 §20.4):
/// `plain` is Chromium 148's en-US output, used without arguments; with a
/// locale or options, `Intl.DateTimeFormat` makes the text.
fn locale_text(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &'static str,
    plain: fn(&Fields) -> String,
    required: Required,
    defaults: Defaults,
) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date(call, method)?;
    if t.is_nan() {
        return rt.text_result("Invalid Date");
    }
    let text = if rt.arg(call, 0).is_undefined() && rt.arg(call, 1).is_undefined() {
        plain(&local(rt, t as i64).0)
    } else {
        let site = match method {
            "toLocaleString" => "Date.prototype.toLocaleString",
            "toLocaleDateString" => "Date.prototype.toLocaleDateString",
            _ => "Date.prototype.toLocaleTimeString",
        };
        intl_datetime::to_locale_text(rt, call, t as i64, required, defaults, site)?
    };
    rt.text_result(&text)
}

/// `Date.prototype.toLocaleString([locales [, options]])`.
pub(super) fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    locale_text(
        rt,
        call,
        "toLocaleString",
        format::to_locale_string,
        Required::Any,
        Defaults::All,
    )
}

/// `Date.prototype.toLocaleDateString([locales [, options]])`.
pub(super) fn to_locale_date_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    locale_text(
        rt,
        call,
        "toLocaleDateString",
        format::to_locale_date_string,
        Required::Date,
        Defaults::Date,
    )
}

/// `Date.prototype.toLocaleTimeString([locales [, options]])`.
pub(super) fn to_locale_time_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    locale_text(
        rt,
        call,
        "toLocaleTimeString",
        format::to_locale_time_string,
        Required::Time,
        Defaults::Time,
    )
}

/// `Date.prototype.toJSON(key)` (§21.4.4.37): `null` for a date whose
/// number is not finite, else the result of `toISOString` (a deferred
/// call).
pub(super) fn to_json(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let primitive = rt.to_primitive(object.into(), Hint::Number)?;
    if primitive.as_number().is_some_and(|n| !n.is_finite()) {
        return Ok(NativeReturn::Value(Value::Null));
    }
    let key = rt.heap.key_from_str("toISOString")?;
    let method = rt.get_value(object.into(), key)?;
    if !rt.is_callable(method)? {
        return Err(VmError::type_error("toISOString is not a function"));
    }
    Ok(NativeReturn::Call {
        callee: method,
        this: object.into(),
        args: Vec::new(),
    })
}

/// `Date.prototype[@@toPrimitive](hint)` (§21.4.4.45): `OrdinaryToPrimitive`
/// with `"string"` first for the hints `"string"` and `"default"`, and
/// `"number"` first for `"number"`; any other hint is a `TypeError`.
pub(super) fn to_primitive(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = call.this() else {
        return Err(rt.incompatible_receiver("Date.prototype [ @@toPrimitive ]", call.this()));
    };
    let hint = match rt.arg(call, 0) {
        Value::String(text) => rt.heap.text(text)?.to_string_lossy(),
        _ => String::new(),
    };
    let order = match hint.as_str() {
        "string" | "default" => Hint::String,
        "number" => Hint::Number,
        _ => {
            let shown = rt.value_text(rt.arg(call, 0));
            return Err(VmError::type_error(format!("Invalid hint: {shown}")));
        }
    };
    Ok(NativeReturn::Value(
        rt.ordinary_to_primitive(object, order)?,
    ))
}
