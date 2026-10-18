//! The setters of `Date.prototype` (ECMA-262 §21.4.4.20 to §21.4.4.29 and
//! the UTC forms, <https://tc39.es/ecma262/#sec-date.prototype.sethours>),
//! `setTime` and Annex B `setYear` (§B.2.3.2), in the order of the
//! conversions of ES2025: the time value of the Date object is read first,
//! then every argument is converted with `ToNumber` in order (a `valueOf`
//! that changes the Date object has no effect on the result), and only
//! then does a NaN time value end the call (except `setFullYear` and
//! `setYear`, which start from +0).

use crate::date::time_value::{
    day_of, fields, make_date, make_day, make_full_year, make_time, time_clip, time_within_day,
};
use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmResult};

use super::date::optional_number;

/// The component that a setter starts with.
#[derive(Clone, Copy)]
enum First {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Date,
    Month,
    FullYear,
}

/// The setter `first` with its optional arguments, on the local (`utc`
/// false) or UTC time of `this` (§21.4.4.20 and the others). `name` is the
/// method name for the message about the receiver.
fn set_components(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    first: First,
    utc: bool,
) -> VmResult<NativeReturn> {
    let (object, time) = rt.this_date(call, name)?;
    // The first argument always converts (an absent one is NaN); the
    // optional ones convert when present, in order.
    let mut args = [rt.to_number(rt.arg(call, 0))?, 0.0, 0.0, 0.0];
    let mut present = [true, false, false, false];
    let max = match first {
        First::Milliseconds | First::Date => 1,
        First::Seconds | First::Month => 2,
        First::Minutes | First::FullYear => 3,
        First::Hours => 4,
    };
    for i in 1..max {
        if call.argc() > i {
            args[i] = optional_number(rt, call, i, 0.0)?;
            present[i] = true;
        }
    }
    let base = if time.is_nan() {
        if !matches!(first, First::FullYear) {
            return Ok(NativeReturn::Value(Value::Double(f64::NAN)));
        }
        0.0
    } else if utc {
        time
    } else {
        rt.local_time(time)
    };
    let f = fields(base as i64);
    // Argument `i` if it is present, else the current field.
    let pick = |i: usize, current: f64| if present[i] { args[i] } else { current };
    let (year, month, day) = (f.year as f64, f64::from(f.month), f64::from(f.day));
    let (hour, minute, second, ms) = (
        f64::from(f.hour),
        f64::from(f.minute),
        f64::from(f.second),
        f64::from(f.ms),
    );
    let whole_day = day_of(base as i64) as f64;
    let within = time_within_day(base as i64) as f64;
    let date = match first {
        First::Milliseconds => make_date(whole_day, make_time(hour, minute, second, args[0])),
        First::Seconds => make_date(whole_day, make_time(hour, minute, args[0], pick(1, ms))),
        First::Minutes => make_date(
            whole_day,
            make_time(hour, args[0], pick(1, second), pick(2, ms)),
        ),
        First::Hours => make_date(
            whole_day,
            make_time(args[0], pick(1, minute), pick(2, second), pick(3, ms)),
        ),
        First::Date => make_date(make_day(year, month, args[0]), within),
        First::Month => make_date(make_day(year, args[0], pick(1, day)), within),
        First::FullYear => make_date(make_day(args[0], pick(1, month), pick(2, day)), within),
    };
    store(rt, object, date, utc)
}

/// `TimeClip` of the date (as `UTC(date)` unless it is UTC already), stored
/// in the Date object and returned.
fn store(rt: &mut Runtime, object: Gc<Object>, date: f64, utc: bool) -> VmResult<NativeReturn> {
    let utc_date = if utc { date } else { rt.utc_of_local(date) };
    let value = time_clip(utc_date);
    rt.set_date_value(object, value)?;
    Ok(NativeReturn::Value(Value::number(value)))
}

macro_rules! setters {
    ($($fn_name:ident, $js_name:expr, $first:expr, $utc:expr;)*) => {
        $(
            pub(super) fn $fn_name(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
                set_components(rt, call, $js_name, $first, $utc)
            }
        )*
    };
}

setters! {
    set_milliseconds, "setMilliseconds", First::Milliseconds, false;
    set_seconds, "setSeconds", First::Seconds, false;
    set_minutes, "setMinutes", First::Minutes, false;
    set_hours, "setHours", First::Hours, false;
    set_date, "setDate", First::Date, false;
    set_month, "setMonth", First::Month, false;
    set_full_year, "setFullYear", First::FullYear, false;
    set_utc_milliseconds, "setUTCMilliseconds", First::Milliseconds, true;
    set_utc_seconds, "setUTCSeconds", First::Seconds, true;
    set_utc_minutes, "setUTCMinutes", First::Minutes, true;
    set_utc_hours, "setUTCHours", First::Hours, true;
    set_utc_date, "setUTCDate", First::Date, true;
    set_utc_month, "setUTCMonth", First::Month, true;
    set_utc_full_year, "setUTCFullYear", First::FullYear, true;
}

/// `Date.prototype.setTime(time)` (§21.4.4.27).
pub(super) fn set_time(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, _) = rt.this_date(call, "setTime")?;
    let time = rt.to_number(rt.arg(call, 0))?;
    let value = time_clip(time);
    rt.set_date_value(object, value)?;
    Ok(NativeReturn::Value(Value::number(value)))
}

/// `Date.prototype.setYear(year)` (§B.2.3.2): like `setFullYear` with
/// `MakeFullYear`, which reads the years 0 to 99 as 1900 to 1999.
pub(super) fn set_year(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, time) = rt.this_date(call, "setYear")?;
    let year = rt.to_number(rt.arg(call, 0))?;
    let base = if time.is_nan() {
        0.0
    } else {
        rt.local_time(time)
    };
    let f = fields(base as i64);
    let day = make_day(make_full_year(year), f64::from(f.month), f64::from(f.day));
    let date = make_date(day, time_within_day(base as i64) as f64);
    store(rt, object, date, false)
}
