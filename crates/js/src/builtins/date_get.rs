//! The getters of `Date.prototype` (ECMA-262 §21.4.4.2 to §21.4.4.40,
//! <https://tc39.es/ecma262/#sec-properties-of-the-date-prototype-object>):
//! the fields of the local time and of the UTC time, `getTime`, `valueOf`,
//! `getTimezoneOffset` and Annex B `getYear` (§B.2.3.1). A Date object
//! with the time value NaN gives NaN.

use crate::date::time_value::{Fields, fields};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmResult};

/// The field of the local (`utc` false) or UTC time of `this`.
fn get_field(
    rt: &mut Runtime,
    call: &NativeCall,
    utc: bool,
    field: fn(&Fields) -> f64,
) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date_getter(call)?;
    if t.is_nan() {
        return Ok(NativeReturn::Value(Value::Double(f64::NAN)));
    }
    let time = if utc { t } else { rt.local_time(t) };
    Ok(NativeReturn::Value(Value::number(field(&fields(
        time as i64,
    )))))
}

macro_rules! getters {
    ($($name:ident, $utc:expr, |$f:ident| $expr:expr;)*) => {
        $(
            pub(super) fn $name(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
                get_field(rt, call, $utc, |$f| $expr)
            }
        )*
    };
}

getters! {
    get_date, false, |f| f64::from(f.day);
    get_day, false, |f| f64::from(f.weekday);
    get_full_year, false, |f| f.year as f64;
    get_hours, false, |f| f64::from(f.hour);
    get_milliseconds, false, |f| f64::from(f.ms);
    get_minutes, false, |f| f64::from(f.minute);
    get_month, false, |f| f64::from(f.month);
    get_seconds, false, |f| f64::from(f.second);
    get_utc_date, true, |f| f64::from(f.day);
    get_utc_day, true, |f| f64::from(f.weekday);
    get_utc_full_year, true, |f| f.year as f64;
    get_utc_hours, true, |f| f64::from(f.hour);
    get_utc_milliseconds, true, |f| f64::from(f.ms);
    get_utc_minutes, true, |f| f64::from(f.minute);
    get_utc_month, true, |f| f64::from(f.month);
    get_utc_seconds, true, |f| f64::from(f.second);
}

/// `Date.prototype.getYear()` (§B.2.3.1): the local year minus 1900. V8
/// words its receiver error as for a method that is not a getter.
pub(super) fn get_year(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date(call, "getYear")?;
    if t.is_nan() {
        return Ok(NativeReturn::Value(Value::Double(f64::NAN)));
    }
    let local = rt.local_time(t);
    Ok(NativeReturn::Value(Value::number(
        fields(local as i64).year as f64 - 1900.0,
    )))
}

/// `Date.prototype.getTime()` (§21.4.4.10).
pub(super) fn get_time(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date_getter(call)?;
    Ok(NativeReturn::Value(Value::number(t)))
}

/// `Date.prototype.valueOf()` (§21.4.4.44).
pub(super) fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    get_time(rt, call)
}

/// `Date.prototype.getTimezoneOffset()` (§21.4.4.11): `(t - LocalTime(t))
/// / msPerMinute`, in minutes (fractional for a zone offset with seconds,
/// as the specification says).
pub(super) fn get_timezone_offset(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, t) = rt.this_date_getter(call)?;
    if t.is_nan() {
        return Ok(NativeReturn::Value(Value::Double(f64::NAN)));
    }
    let offset = rt.local_offset(t);
    // `0 - offset`, so that a zero offset gives +0.
    let minutes = (0.0 - offset as f64) / 60_000.0;
    Ok(NativeReturn::Value(Value::number(minutes)))
}
