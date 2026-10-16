//! The Unicode methods of `String.prototype`: `toLowerCase`,
//! `toUpperCase`, `toLocaleLowerCase`, `toLocaleUpperCase` (ECMA-262
//! §22.1.3.28 to §22.1.3.31), `normalize` (§22.1.3.15) and `localeCompare`
//! (§22.1.3.12). The algorithms and tables are in `swb_js_text::unicode`
//! (ADR 0026 section 15); this module converts the arguments, works in
//! slices and does the bookkeeping of a call from a script.
//!
//! - There is no `Intl`: the `toLocale` forms ignore their arguments and
//!   use the root locale, and `localeCompare` ignores `locales` and
//!   `options`.
//! - A conversion that changes nothing returns the receiver itself (no
//!   copy). A short Latin-1 or ASCII text needs no table.
//! - A long text is converted in slices of [`SLICE`] code units. Between
//!   two slices the code checks V8's maximum length (a `RangeError`
//!   "Invalid string length" before the memory is allocated), reserves the
//!   memory in the heap's accounting (doubling, like [`Builder`]) and
//!   charges the time. The receiver is flattened first (a safepoint:
//!   `this` and the arguments are on the value stack, so they are rooted).
//! - `localeCompare` normalizes a text to NFD only if it is not NFD
//!   already, and compares in a pass that reports its work to the time
//!   check.

use std::cmp::Ordering;

use swb_js_text::unicode::case::{self, CaseOp, Latin1Case};
use swb_js_text::unicode::collate;
use swb_js_text::unicode::normalize::{self, Form, MAX_DECOMPOSITION, Normalizer};
use swb_js_text::{Str16, String16};

use super::string::{Builder, INVALID_STRING_LENGTH, string_result};
use crate::heap::Gc;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::convert::MAX_STRING_LENGTH;
use crate::vm::time::UNITS_PER_STEP;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

/// The number of code units that a conversion handles between two
/// bookkeeping steps.
const SLICE: usize = 1 << 14;

/// The message of V8's `RangeError` for a bad normalization form.
const BAD_FORM: &str = "The normalization form should be one of NFC, NFD, NFKC, NFKD.";

// --- Case mapping ---

/// `String.prototype.toLowerCase()` (§22.1.3.28).
pub(super) fn to_lower_case(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    convert_case(rt, call, "String.prototype.toLowerCase", CaseOp::Lower)
}

/// `String.prototype.toUpperCase()` (§22.1.3.30).
pub(super) fn to_upper_case(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    convert_case(rt, call, "String.prototype.toUpperCase", CaseOp::Upper)
}

/// `String.prototype.toLocaleLowerCase([locales])` (§22.1.3.27): the
/// root locale, whatever the argument.
pub(super) fn to_locale_lower_case(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    convert_case(
        rt,
        call,
        "String.prototype.toLocaleLowerCase",
        CaseOp::Lower,
    )
}

/// `String.prototype.toLocaleUpperCase([locales])` (§22.1.3.29).
pub(super) fn to_locale_upper_case(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    convert_case(
        rt,
        call,
        "String.prototype.toLocaleUpperCase",
        CaseOp::Upper,
    )
}

/// The case conversion of the receiver: the Unicode Default Case
/// Conversion with the full mappings.
fn convert_case(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &str,
    op: CaseOp,
) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, method)?;
    let len = rt.string_len(string)?;
    if len == 0 {
        return string_result(string);
    }
    let text = rt.flat_text(string)?;
    if let Str16::Latin1(bytes) = text
        && len <= SLICE
    {
        // The common case: no table, one pass, and no copy if nothing
        // changes. A longer text takes the sliced path below, which checks
        // the length and heap limits and charges time for each slice.
        let converted = case::convert_latin1(bytes, op);
        rt.charge_units(len)?;
        match converted {
            Latin1Case::Unchanged => return string_result(string),
            Latin1Case::Changed(out) => {
                let result = rt.new_string(out)?;
                return string_result(result);
            }
            Latin1Case::Wide => {}
        }
    }
    let text = rt.heap.text(string)?;
    let first = case::first_change(text, op);
    rt.charge_units(len)?;
    let Some(first) = first else {
        return string_result(string);
    };
    if len <= SLICE {
        let text = rt.heap.text(string)?;
        let mut out = String16::try_with_capacity(len, false).unwrap_or_default();
        if let Some(prefix) = text.slice(0, first) {
            out.push_str16(prefix);
        }
        case::map_chunk(text, first, len, op, &mut out);
        rt.charge_units(len - first)?;
        let result = rt.new_string(out)?;
        return string_result(result);
    }
    let mut builder = Builder::new();
    builder.push_range(rt, string, 0, first)?;
    let mut at = first;
    while at < len {
        let limit = (at + SLICE).min(len);
        // At most three units for each unit.
        builder.check(rt, 3 * (limit - at))?;
        let text = rt.heap.text(string)?;
        let stop = case::map_chunk(text, at, limit, op, builder.text_mut());
        rt.charge_units(stop - at)?;
        at = stop;
    }
    let result = builder.finish(rt)?;
    string_result(result)
}

// --- normalize ---

/// `String.prototype.normalize([form])` (§22.1.3.15).
pub(super) fn normalize(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.normalize")?;
    let form = match rt.arg(call, 0) {
        Value::Undefined => Form::Nfc,
        value => {
            let name = rt.to_string(value)?;
            Form::from_name(rt.flat_text(name)?).ok_or(VmError::range_error(BAD_FORM))?
        }
    };
    match normalized_text(rt, string, form)? {
        None => string_result(string),
        Some(text) => {
            let result = rt.new_string(text)?;
            string_result(result)
        }
    }
}

/// The text of `string` in `form`, or `None` if the string is in the form
/// already. The string is rooted by the caller.
fn normalized_text(
    rt: &mut Runtime,
    string: Gc<JsString>,
    form: Form,
) -> VmResult<Option<String16>> {
    let len = rt.string_len(string)?;
    let text = rt.flat_text(string)?;
    let done = normalize::is_normalized(text, form);
    rt.charge_units(len)?;
    if done {
        return Ok(None);
    }
    let mut normalizer = Normalizer::new(form);
    let mut builder = Builder::new();
    let mut reserved = 0usize;
    let mut at = 0;
    while at < len {
        let limit = (at + SLICE).min(len);
        // The output so far, the run being sorted and the next slice.
        let bytes = builder
            .len()
            .saturating_add(MAX_DECOMPOSITION * (limit - at))
            .saturating_mul(4)
            .saturating_add(normalizer.in_flight().saturating_mul(32));
        if bytes > reserved {
            reserved = bytes.saturating_mul(2).max(1 << 17);
            rt.heap.reserve(reserved, &rt.vm)?;
        }
        let text = rt.heap.text(string)?;
        let stop = normalizer.feed(text, at, limit, builder.text_mut());
        rt.charge_units(stop - at)?;
        if builder.len().saturating_add(normalizer.in_flight()) > MAX_STRING_LENGTH {
            return Err(VmError::range_error(INVALID_STRING_LENGTH));
        }
        at = stop;
    }
    normalizer.finish(builder.text_mut());
    if builder.len() > MAX_STRING_LENGTH {
        return Err(VmError::range_error(INVALID_STRING_LENGTH));
    }
    builder.into_text(rt).map(Some)
}

// --- localeCompare ---

/// `String.prototype.localeCompare(that)` (§22.1.3.12): -1, 0 or 1 by
/// the order of `swb_js_text::unicode::collate`.
pub(super) fn locale_compare(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = rt.this_string(call, "String.prototype.localeCompare")?;
    let that = rt.string_arg(call, 0)?;
    rt.flatten(this)?;
    rt.flatten(that)?;
    if this == that {
        return Ok(NativeReturn::Value(Value::Int(0)));
    }
    let (len_this, len_that) = (rt.string_len(this)?, rt.string_len(that)?);
    if rt.heap.text(this)? == rt.heap.text(that)? {
        rt.charge_units(len_this)?;
        return Ok(NativeReturn::Value(Value::Int(0)));
    }
    let owned_this = normalized_text(rt, this, Form::Nfd)?;
    let owned_that = normalized_text(rt, that, Form::Nfd)?;
    let text_this = match &owned_this {
        Some(text) => text.as_str16(),
        None => rt.heap.text(this)?,
    };
    let text_that = match &owned_that {
        Some(text) => text.as_str16(),
        None => rt.heap.text(that)?,
    };
    let mut termination = None;
    let vm = &mut rt.vm;
    let order = collate::compare_decomposed(text_this, text_that, &mut |count| match vm
        .charge(count / UNITS_PER_STEP)
    {
        Ok(()) => false,
        Err(error) => {
            termination = Some(error);
            true
        }
    });
    if let Some(error) = termination {
        return Err(error.into());
    }
    rt.charge_units(len_this + len_that)?;
    let result = match order {
        Some(Ordering::Less) => -1,
        Some(Ordering::Greater) => 1,
        _ => 0,
    };
    Ok(NativeReturn::Value(Value::Int(result)))
}
