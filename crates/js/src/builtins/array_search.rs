//! The `Array.prototype` methods that search or read the elements without
//! a callback (ECMA-262 §23.1.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! `at`, `indexOf`, `lastIndexOf`, `includes`, `join`, `toString` and
//! `toLocaleString`.
//!
//! On an array with dense elements the searches compare in the element
//! vector without a handle scope per element (the comparisons run no
//! script code); on sparse elements they visit only the stored indices.
//! `includes` reads holes as `undefined` (it uses `Get`, not
//! `HasProperty`), so a run of holes matches only a search for
//! `undefined`. `join` adds the separators of a run of holes at once.

use swb_js_text::String16;

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::{Equality, Value};
use crate::vm::convert::{MAX_STRING_LENGTH, from_index_start, to_integer_or_infinity};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array::receiver;
use super::array_access::{Flow, Item};

/// The elements that a dense search compares per time step.
const COMPARES_PER_STEP: usize = 4;

/// The elements that a dense search compares between two charges.
const SEARCH_CHUNK: usize = 4096;

/// A joined text above this many code units reserves its memory in the
/// heap's accounting at each doubling (the buffer is Rust memory until
/// the string exists).
const RESERVE_FROM: usize = 1 << 16;

/// `Array.prototype.at(index)` (§23.1.3.1).
pub(super) fn at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let relative = to_integer_or_infinity(rt.to_number(rt.arg(call, 0))?);
    let k = if relative >= 0.0 {
        relative
    } else {
        length + relative
    };
    if k < 0.0 || k >= length {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    Ok(NativeReturn::Value(rt.get_index(object, k)?))
}

/// The outcome of a search in a dense vector.
enum Dense {
    /// The receiver is not a dense array; take the general steps.
    NotDense,
    /// The index of the match, or `None`.
    Done(Option<f64>),
}

/// Searches the dense vector of `object` for `search` from `from` up to
/// `end` (or down to `end` with `descending`, `from` included). Holes
/// match only with `holes` (a search for `undefined` by `includes`).
fn dense_search(
    rt: &mut Runtime,
    object: Gc<Object>,
    (from, end): (f64, f64),
    descending: bool,
    (search, mode, holes): (Value, Equality, bool),
) -> VmResult<Dense> {
    let Some(state) = rt.heap.dense_array(object)? else {
        return Ok(Dense::NotDense);
    };
    let stored = state.stored_len as f64;
    let key = (search, mode, holes);
    if descending {
        if holes && from >= stored {
            return Ok(Dense::Done(Some(from)));
        }
        let top = from.min(stored - 1.0);
        return Ok(Dense::Done(dense_scan(rt, object, (top, end), true, key)?));
    }
    if let Some(k) = dense_scan(rt, object, (from, end.min(stored)), false, key)? {
        return Ok(Dense::Done(Some(k)));
    }
    // An index at or above the end of the vector is a hole.
    let first_hole = from.max(stored);
    Ok(Dense::Done(
        (holes && first_hole < end).then_some(first_hole),
    ))
}

/// The scan of [`dense_search`] inside the vector, in chunks with a time
/// charge after each.
fn dense_scan(
    rt: &mut Runtime,
    object: Gc<Object>,
    (from, end): (f64, f64),
    descending: bool,
    (search, mode, holes): (Value, Equality, bool),
) -> VmResult<Option<f64>> {
    if (!descending && from >= end) || (descending && (from < end || from < 0.0)) {
        return Ok(None);
    }
    let (mut low, mut high) = if descending {
        (end as usize, from as usize + 1)
    } else {
        (from as usize, end as usize)
    };
    while low < high {
        let chunk = (high - low).min(SEARCH_CHUNK);
        let range = if descending {
            high - chunk..high
        } else {
            low..low + chunk
        };
        let found = {
            let heap = &rt.heap;
            let values = heap
                .object(object)?
                .elements
                .dense()
                .ok_or(VmError::invariant("a dense search on sparse elements"))?;
            let part = values
                .get(range.clone())
                .ok_or(VmError::invariant("a dense search outside the vector"))?;
            // A Number compares without the general equality: both forms
            // of a Number are the same value; NaN matches only itself in
            // `SameValueZero`.
            let number = search.as_number();
            let nan_matches = mode == Equality::SameValueZero;
            let matches = |value: &Value| -> crate::error::Result<bool> {
                match (number, value) {
                    (_, Value::Empty) => Ok(holes),
                    (Some(x), Value::Int(int)) => Ok(f64::from(*int) == x),
                    (Some(x), Value::Double(double)) => {
                        Ok(*double == x || (nan_matches && x.is_nan() && double.is_nan()))
                    }
                    (Some(_), _) => Ok(false),
                    (None, _) => heap.equal(mode, search, *value),
                }
            };
            let mut found = None;
            if descending {
                for (offset, value) in part.iter().enumerate().rev() {
                    if matches(value)? {
                        found = Some(offset);
                        break;
                    }
                }
            } else {
                for (offset, value) in part.iter().enumerate() {
                    if matches(value)? {
                        found = Some(offset);
                        break;
                    }
                }
            }
            found
        };
        rt.charge(chunk / COMPARES_PER_STEP)?;
        if let Some(offset) = found {
            return Ok(Some((range.start + offset) as f64));
        }
        if descending {
            high -= chunk;
        } else {
            low += chunk;
        }
    }
    Ok(None)
}

/// `Array.prototype.indexOf(searchElement, fromIndex)` (§23.1.3.17).
pub(super) fn index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some("indexOf"))?;
    let not_found = NativeReturn::Value(Value::Int(-1));
    if length == 0.0 {
        return Ok(not_found);
    }
    let n = to_integer_or_infinity(rt.to_number(rt.arg(call, 1))?);
    if n == f64::INFINITY {
        return Ok(not_found);
    }
    let start = from_index_start(n, length);
    let search = rt.arg(call, 0);
    if let Dense::Done(found) = dense_search(
        rt,
        object,
        (start, length),
        false,
        (search, Equality::Strict, false),
    )? {
        return Ok(NativeReturn::Value(
            found.map_or(Value::Int(-1), Value::number),
        ));
    }
    let mut result = None;
    rt.for_present(object, start, length, |rt, k, element| {
        if rt.strictly_equal(search, element)? {
            result = Some(k);
            return Ok(Flow::Break);
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(
        result.map_or(Value::Int(-1), Value::number),
    ))
}

/// `Array.prototype.lastIndexOf(searchElement, fromIndex)` (§23.1.3.20).
pub(super) fn last_index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let not_found = NativeReturn::Value(Value::Int(-1));
    if length == 0.0 {
        return Ok(not_found);
    }
    let n = if call.argc() > 1 {
        to_integer_or_infinity(rt.to_number(rt.arg(call, 1))?)
    } else {
        length - 1.0
    };
    if n == f64::NEG_INFINITY {
        return Ok(not_found);
    }
    let start = if n >= 0.0 {
        n.min(length - 1.0)
    } else {
        length + n
    };
    if start < 0.0 {
        return Ok(not_found);
    }
    let search = rt.arg(call, 0);
    if let Dense::Done(found) = dense_search(
        rt,
        object,
        (start, 0.0),
        true,
        (search, Equality::Strict, false),
    )? {
        return Ok(NativeReturn::Value(
            found.map_or(Value::Int(-1), Value::number),
        ));
    }
    let mut result = None;
    rt.for_present_rev(object, start, 0.0, |rt, k, element| {
        if rt.strictly_equal(search, element)? {
            result = Some(k);
            return Ok(Flow::Break);
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(
        result.map_or(Value::Int(-1), Value::number),
    ))
}

/// `Array.prototype.includes(searchElement, fromIndex)` (§23.1.3.16).
pub(super) fn includes(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if length == 0.0 {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    }
    let n = to_integer_or_infinity(rt.to_number(rt.arg(call, 1))?);
    if n == f64::INFINITY {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    }
    let start = from_index_start(n, length);
    let search = rt.arg(call, 0);
    let holes = search.is_undefined();
    if let Dense::Done(found) = dense_search(
        rt,
        object,
        (start, length),
        false,
        (search, Equality::SameValueZero, holes),
    )? {
        return Ok(NativeReturn::Value(Value::Bool(found.is_some())));
    }
    let mut found = false;
    rt.for_index(object, start, length, false, |rt, item| {
        let matched = match item {
            Item::Value(_, element) => rt.heap.equal(Equality::SameValueZero, search, element)?,
            Item::Gap(..) => holes,
        };
        if matched {
            found = true;
            return Ok(Flow::Break);
        }
        Ok(Flow::Continue)
    })?;
    Ok(NativeReturn::Value(Value::Bool(found)))
}

// --- join, toString, toLocaleString ---

/// `Array.prototype.join(separator)` (§23.1.3.18). A receiver that is
/// already being joined gives the empty string (V8's cycle detection; the
/// specification would recurse without end).
pub(super) fn join(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if rt.vm.join_stack.contains(&object) {
        return Ok(NativeReturn::Value(rt.vm.atoms.empty.into()));
    }
    // At most one element and a separator that converts without effects:
    // the result is the element itself when it is a string, or empty.
    if length <= 1.0 && matches!(rt.arg(call, 0), Value::Undefined | Value::String(_)) {
        let element = if length == 0.0 {
            Some(Value::Undefined)
        } else {
            rt.heap.index_get_fast(object, 0)
        };
        match element {
            Some(Value::String(string)) => return Ok(NativeReturn::Value(string.into())),
            Some(Value::Undefined | Value::Null) => {
                return Ok(NativeReturn::Value(rt.vm.atoms.empty.into()));
            }
            _ => {}
        }
    }
    let separator = match rt.arg(call, 0) {
        Value::Undefined => String16::from(","),
        value => {
            let string = rt.to_string(value)?;
            let units = rt.heap.copy_string(string)?;
            rt.charge_units(units.len())?;
            units
        }
    };
    joined(rt, object, length, &separator, false)
}

/// `Array.prototype.toLocaleString()` (§23.1.3.32), with "," as the
/// separator, as in V8. Without ECMA-402 the elements' `toLocaleString`
/// gets no arguments (§23.1.3.32 step 6.c.i; test262); Chromium passes
/// `locales` and `options` on (ECMA-402 §19.5.1).
pub(super) fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if rt.vm.join_stack.contains(&object) {
        return Ok(NativeReturn::Value(rt.vm.atoms.empty.into()));
    }
    joined(rt, object, length, &String16::from(","), true)
}

/// The common part of `join` and `toLocaleString`: the receiver is on the
/// join stack while its elements are converted.
fn joined(
    rt: &mut Runtime,
    object: Gc<Object>,
    length: f64,
    separator: &String16,
    locale: bool,
) -> VmResult<NativeReturn> {
    rt.vm.join_stack.push(object);
    let result = join_elements(rt, object, length, separator, locale);
    rt.vm.join_stack.pop();
    let text = result?;
    if text.len() >= RESERVE_FROM {
        rt.heap.reserve(text.len() * 2, &rt.vm)?;
    }
    let string = rt.heap.alloc_string(text)?;
    Ok(NativeReturn::Value(string.into()))
}

/// The text that `join` builds, with the checks of its length and of the
/// heap limit.
struct JoinText<'a> {
    text: String16,
    separator: &'a String16,
    reserved: usize,
}

impl JoinText<'_> {
    /// Adds `count` separators.
    fn separators(&mut self, rt: &mut Runtime, count: f64) -> VmResult<()> {
        if count <= 0.0 || self.separator.is_empty() {
            return Ok(());
        }
        let total = count * self.separator.len() as f64 + self.text.len() as f64;
        if total > MAX_STRING_LENGTH as f64 {
            return Err(VmError::range_error("Invalid string length"));
        }
        self.room(rt, total as usize)?;
        for _ in 0..count as usize {
            self.text.push_str16(self.separator.as_str16());
        }
        rt.charge_units(count as usize * self.separator.len())
    }

    /// Checks the length limit and reserves heap memory for `total` code
    /// units (at each doubling).
    fn room(&mut self, rt: &mut Runtime, total: usize) -> VmResult<()> {
        if total > MAX_STRING_LENGTH {
            return Err(VmError::range_error("Invalid string length"));
        }
        if total > self.reserved {
            // The buffer counts against the heap limit before it is a
            // string (checked at each doubling).
            self.reserved = total.max(self.reserved * 2);
            rt.heap.reserve(self.reserved * 2, &rt.vm)?;
        }
        Ok(())
    }
}

/// The loop of `join` (with `locale`, of `toLocaleString`).
fn join_elements(
    rt: &mut Runtime,
    object: Gc<Object>,
    length: f64,
    separator: &String16,
    locale: bool,
) -> VmResult<String16> {
    let mut out = JoinText {
        text: String16::new(),
        separator,
        reserved: RESERVE_FROM,
    };
    rt.for_index(object, 0.0, length, false, |rt, item| {
        match item {
            Item::Gap(from, to) => {
                let count = if from == 0.0 {
                    to - from - 1.0
                } else {
                    to - from
                };
                out.separators(rt, count)?;
            }
            Item::Value(k, element) => {
                if k > 0.0 {
                    out.separators(rt, 1.0)?;
                }
                if !element.is_nullish() {
                    let string = if locale {
                        let key = rt.heap.key_from_str("toLocaleString")?;
                        let method = rt.get_value(element, key)?;
                        if !rt.is_callable(method)? {
                            return Err(rt.not_callable(method));
                        }
                        let result = rt.call(method, element, &[])?;
                        rt.to_string(result)?
                    } else {
                        rt.to_string(element)?
                    };
                    let units = rt.heap.string(string)?.len();
                    out.room(rt, out.text.len() + units)?;
                    rt.heap.push_string(&mut out.text, string)?;
                    // The copy costs steps in proportion to its size.
                    rt.charge_units(units)?;
                }
            }
        }
        Ok(Flow::Continue)
    })?;
    Ok(out.text)
}

/// `Array.prototype.toString()` (§23.1.3.36): calls `join` as a deferred
/// call, or `Object.prototype.toString` if `join` is not callable.
pub(super) fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object(call)?;
    let key = rt.heap.key_from_str("join")?;
    let join = rt.get_value(object.into(), key)?;
    if rt.is_callable(join)? {
        return Ok(NativeReturn::Call {
            callee: join,
            this: object.into(),
            args: Vec::new(),
        });
    }
    let text = super::object::builtin_tag_text(rt, object.into())?;
    Ok(NativeReturn::Value(text))
}
