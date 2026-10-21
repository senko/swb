//! `Date` (ECMA-262 §21.4, <https://tc39.es/ecma262/#sec-date-objects>):
//! the constructor with its argument forms, `Date.now`, `Date.parse`,
//! `Date.UTC`, the receiver checks and the local time conversions that the
//! prototype methods share. The getters are in `date_get.rs`, the setters
//! in `date_set.rs`, the text forms, `toJSON` and `@@toPrimitive` in
//! `date_string.rs`. The time arithmetic, the zone, the formats and the
//! parser are in `crate::date`.
//!
//! Where V8 words a `TypeError` for a wrong receiver, the messages follow
//! it: "this is not a Date object." for the getters, and "Method
//! Date.prototype.NAME called on incompatible receiver X" for the other
//! methods.

use crate::date::parse;
use crate::date::time_value::{
    MAX_TIME_VALUE, MS_PER_DAY, make_date, make_day, make_full_year, make_time, time_clip,
};
use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::convert::Hint;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

/// The prototype methods in the order of Chromium 148's keys, with their
/// `length`. `toGMTString` follows `toUTCString` (the same function
/// object, §B.2.3.3).
fn prototype_methods() -> Vec<(&'static str, u32, NativeFn)> {
    use super::date_get as g;
    use super::date_set as s;
    use super::date_string as t;
    vec![
        ("toString", 0, t::to_string),
        ("toDateString", 0, t::to_date_string),
        ("toTimeString", 0, t::to_time_string),
        ("toISOString", 0, t::to_iso_string),
        ("toUTCString", 0, t::to_utc_string),
        ("getDate", 0, g::get_date),
        ("setDate", 1, s::set_date),
        ("getDay", 0, g::get_day),
        ("getFullYear", 0, g::get_full_year),
        ("setFullYear", 3, s::set_full_year),
        ("getHours", 0, g::get_hours),
        ("setHours", 4, s::set_hours),
        ("getMilliseconds", 0, g::get_milliseconds),
        ("setMilliseconds", 1, s::set_milliseconds),
        ("getMinutes", 0, g::get_minutes),
        ("setMinutes", 3, s::set_minutes),
        ("getMonth", 0, g::get_month),
        ("setMonth", 2, s::set_month),
        ("getSeconds", 0, g::get_seconds),
        ("setSeconds", 2, s::set_seconds),
        ("getTime", 0, g::get_time),
        ("setTime", 1, s::set_time),
        ("getTimezoneOffset", 0, g::get_timezone_offset),
        ("getUTCDate", 0, g::get_utc_date),
        ("setUTCDate", 1, s::set_utc_date),
        ("getUTCDay", 0, g::get_utc_day),
        ("getUTCFullYear", 0, g::get_utc_full_year),
        ("setUTCFullYear", 3, s::set_utc_full_year),
        ("getUTCHours", 0, g::get_utc_hours),
        ("setUTCHours", 4, s::set_utc_hours),
        ("getUTCMilliseconds", 0, g::get_utc_milliseconds),
        ("setUTCMilliseconds", 1, s::set_utc_milliseconds),
        ("getUTCMinutes", 0, g::get_utc_minutes),
        ("setUTCMinutes", 3, s::set_utc_minutes),
        ("getUTCMonth", 0, g::get_utc_month),
        ("setUTCMonth", 2, s::set_utc_month),
        ("getUTCSeconds", 0, g::get_utc_seconds),
        ("setUTCSeconds", 2, s::set_utc_seconds),
        ("valueOf", 0, g::value_of),
        ("getYear", 0, g::get_year),
        ("setYear", 1, s::set_year),
        ("toJSON", 1, t::to_json),
        ("toLocaleString", 0, t::to_locale_string),
        ("toLocaleDateString", 0, t::to_locale_date_string),
        ("toLocaleTimeString", 0, t::to_locale_time_string),
    ]
}

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::DatePrototype)?;
    let date = super::constructor(rt, realm, "Date", 7, date_constructor, proto)?;
    super::method(rt, realm, date, "now", 0, now)?;
    super::method(rt, realm, date, "parse", 1, parse_function)?;
    super::method(rt, realm, date, "UTC", 7, utc)?;
    for (name, length, func) in prototype_methods() {
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "toUTCString" {
            let key = rt.heap.key_from_str("toGMTString")?;
            rt.define(proto, key, function.into(), true, false, true)?;
        }
    }
    super::symbol_method(
        rt,
        realm,
        proto,
        WellKnown::ToPrimitive,
        1,
        super::date_string::to_primitive,
        (false, true),
    )?;
    Ok(())
}

// --- Receivers and local time ---

impl Runtime {
    /// The Date object that is `this` and its time value, with V8's
    /// "this is not a Date object." for another receiver (the getters).
    pub(super) fn this_date_getter(&self, call: &NativeCall) -> VmResult<(Gc<Object>, f64)> {
        self.date_value(call.this())
            .ok_or_else(|| VmError::type_error("this is not a Date object."))
    }

    /// The Date object that is `this` and its time value (`thisTimeValue`,
    /// §21.4.4.41.4); another receiver gives V8's "Method
    /// Date.prototype.NAME called on incompatible receiver X".
    pub(super) fn this_date(&self, call: &NativeCall, method: &str) -> VmResult<(Gc<Object>, f64)> {
        self.date_value(call.this()).ok_or_else(|| {
            self.incompatible_receiver(&format!("Date.prototype.{method}"), call.this())
        })
    }

    /// The Date object and time value of `value`, if it is a Date object.
    fn date_value(&self, value: Value) -> Option<(Gc<Object>, f64)> {
        let Value::Object(object) = value else {
            return None;
        };
        match self.heap.object(object).ok()?.kind {
            ObjectKind::Date(t) => Some((object, t)),
            _ => None,
        }
    }

    /// Stores the time value `t` in a Date object.
    pub(super) fn set_date_value(&mut self, object: Gc<Object>, t: f64) -> VmResult<()> {
        self.heap.object_mut(object)?.kind = ObjectKind::Date(t);
        Ok(())
    }

    /// `LocalTime(t)` (§21.4.1.25) for a time value; NaN stays NaN.
    pub(super) fn local_time(&mut self, t: f64) -> f64 {
        if t.is_nan() {
            return f64::NAN;
        }
        t + self.local_offset(t) as f64
    }

    /// `LocalTZA(t, true)` (§21.4.1.25): the offset of the local zone at
    /// the UTC time `t` (a finite time value), in milliseconds.
    pub(super) fn local_offset(&mut self, t: f64) -> i64 {
        self.vm.time.zone().offset_at_utc(t as i64)
    }

    /// `UTC(t)` (§21.4.1.26): the UTC time of a local time; NaN if `t` is
    /// not finite or so far outside the range that no zone brings it back
    /// (the caller clips the result).
    pub(super) fn utc_of_local(&mut self, local: f64) -> f64 {
        if !local.is_finite() || local.abs() > MAX_TIME_VALUE + 2.0 * MS_PER_DAY {
            return f64::NAN;
        }
        self.vm.time.zone().utc_from_local(local as i64) as f64
    }

    /// A new string value, recorded in the native's handle scope.
    pub(super) fn text_result(&mut self, text: &str) -> VmResult<NativeReturn> {
        let string = self.heap.alloc_str(text)?;
        self.heap.record(string);
        Ok(NativeReturn::Value(string.into()))
    }
}

// --- The constructor and the static functions ---

/// `Date(...values)` (§21.4.2.1): called as a function, the string of the
/// current time; with `new`, a Date object from the current time, a time
/// value, a string, another Date, or the components.
fn date_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        let now = rt.vm.time.now();
        let text = super::date_string::date_to_string(rt, now);
        return rt.text_result(&text);
    }
    let time = match call.argc() {
        0 => rt.vm.time.now(),
        1 => time_from_one_argument(rt, rt.arg(call, 0))?,
        _ => time_from_components(rt, call)?,
    };
    let proto = rt.prototype_from(call, Intrinsic::DatePrototype)?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::Date(time))?;
    Ok(NativeReturn::Value(object.into()))
}

/// The time value of `new Date(value)` (§21.4.2.1 step 4): the value of a
/// Date object, the parse of a string, else `TimeClip(ToNumber(value))`.
fn time_from_one_argument(rt: &mut Runtime, value: Value) -> VmResult<f64> {
    if let Some((_, t)) = rt.date_value(value) {
        return Ok(t);
    }
    let primitive = rt.to_primitive(value, Hint::Default)?;
    if let Value::String(string) = primitive {
        return parse_text(rt, string);
    }
    Ok(time_clip(rt.to_number(primitive)?))
}

/// The conversions of the 2 to 7 components of `new Date(y, m, ...)` and
/// `Date.UTC`: all arguments in order, then `MakeFullYear`, `MakeDay`,
/// `MakeTime` and `MakeDate`. Returns the date as a number (a local time
/// for the constructor; the caller applies `UTC(t)` and `TimeClip`).
fn components(rt: &mut Runtime, call: &NativeCall) -> VmResult<f64> {
    let year = rt.to_number(rt.arg(call, 0))?;
    let month = optional_number(rt, call, 1, 0.0)?;
    let date = optional_number(rt, call, 2, 1.0)?;
    let hours = optional_number(rt, call, 3, 0.0)?;
    let minutes = optional_number(rt, call, 4, 0.0)?;
    let seconds = optional_number(rt, call, 5, 0.0)?;
    let ms = optional_number(rt, call, 6, 0.0)?;
    Ok(make_date(
        make_day(make_full_year(year), month, date),
        make_time(hours, minutes, seconds, ms),
    ))
}

/// `ToNumber` of argument `index` if it is present, else `default`.
pub(super) fn optional_number(
    rt: &mut Runtime,
    call: &NativeCall,
    index: usize,
    default: f64,
) -> VmResult<f64> {
    if call.argc() > index {
        rt.to_number(rt.arg(call, index))
    } else {
        Ok(default)
    }
}

fn time_from_components(rt: &mut Runtime, call: &NativeCall) -> VmResult<f64> {
    let local = components(rt, call)?;
    let utc = rt.utc_of_local(local);
    Ok(time_clip(utc))
}

/// `Date.now()` (§21.4.3.1).
#[allow(clippy::unnecessary_wraps, reason = "the result type of the natives")]
fn now(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::number(rt.vm.time.now())))
}

/// `Date.parse(string)` (§21.4.3.2).
fn parse_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.to_string(rt.arg(call, 0))?;
    let time = parse_text(rt, string)?;
    Ok(NativeReturn::Value(Value::number(time)))
}

/// The time value of a date string (NaN if it has none of the forms of
/// `crate::date::parse`), with `UTC(t)` for a local time.
fn parse_text(rt: &mut Runtime, string: Gc<JsString>) -> VmResult<f64> {
    let text = rt.flat_text(string)?;
    // The exact parsers stop at the first unit that does not fit; the
    // reader of the other forms scans the text once, and the scan is
    // charged. A rope flattens (and is charged) first.
    let (parsed, scanned) = parse::parse_with_cost(text);
    rt.charge_units(scanned)?;
    let Some(parsed) = parsed else {
        return Ok(f64::NAN);
    };
    let time = if parsed.local {
        rt.utc_of_local(parsed.time)
    } else {
        parsed.time
    };
    Ok(time_clip(time))
}

/// `Date.UTC(year [, month [, date [, hours [, minutes [, seconds [,
/// ms]]]]]])` (§21.4.3.4).
fn utc(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let time = components(rt, call)?;
    Ok(NativeReturn::Value(Value::number(time_clip(time))))
}
