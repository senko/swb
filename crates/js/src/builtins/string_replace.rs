//! `String.prototype.replace`, `replaceAll` and `split` (ECMA-262
//! §22.1.3.19, §22.1.3.20, §22.1.3.23) and `GetSubstitution`
//! (§22.1.3.19.1).
//!
//! The three methods dispatch on an object argument first: `GetMethod`
//! of `@@replace` or `@@split`, called as a deferred call (no Rust
//! recursion) with the receiver and the other argument (`RegExp` objects
//! have them, `regexp_symbol.rs`). Without such a method,
//! the argument is converted to a string and the search is the plain
//! `StringIndexOf` of [`super::string_search`].
//!
//! [`get_substitution`] takes the captures and the named-capture object as
//! parameters, so that `RegExp.prototype[@@replace]` calls it with its
//! match; `replace` and `replaceAll` call it with none.
//!
//! `replaceAll` and `split` find their matches one at a time: the
//! positions are not collected (the string cannot change meanwhile, so
//! this is not observable). With a replacer function, `replace` and
//! `replaceAll` are native loops (`vm/native_loop.rs`) whose calls run as
//! frames of the interpreter loop. Each match charges the time of its search,
//! its copy and a step of the countdown. The result of `replaceAll` is
//! built in a [`Builder`], which stops at V8's maximum length; `split`
//! checks the heap limit every [`SAFEPOINT_PIECES`] pieces.

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::to_uint32;
use crate::vm::native_loop::{LoopStep, NativeLoop};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

use super::string::{Builder, string_result};

/// The pieces of `split` between two checks of the heap limit.
const SAFEPOINT_PIECES: usize = 256;

/// The pieces that `split` makes room for before it has found them (the
/// vector doubles after that).
const SPLIT_CAPACITY: usize = 16;

/// The most pieces that the split cache keeps.
const SPLIT_CACHE_PIECES: usize = 64;

/// The `TypeError` of a receiver that is `undefined` or `null`, in V8's
/// words.
fn require_coercible(call: &NativeCall, method: &str) -> VmResult<Value> {
    let this = call.this();
    if this.is_nullish() {
        return Err(VmError::type_error(format!(
            "String.prototype.{method} called on null or undefined"
        )));
    }
    Ok(this)
}

/// The deferred call of a `@@replace` or `@@split` method.
fn dispatch(method: Value, target: Value, this: Value, argument: Value) -> NativeReturn {
    NativeReturn::Call {
        callee: method,
        this: target,
        args: vec![this, argument],
    }
}

// --- replace ---

/// `String.prototype.replace(searchValue, replaceValue)` (§22.1.3.19).
pub(super) fn replace(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = require_coercible(call, "replace")?;
    let search_value = rt.arg(call, 0);
    let replace_value = rt.arg(call, 1);
    if matches!(search_value, Value::Object(_)) {
        let key = rt.symbol_key(WellKnown::Replace);
        if let Some(replacer) = rt.get_method(search_value, key)? {
            return Ok(dispatch(replacer, search_value, this, replace_value));
        }
    }
    let string = rt.to_string(this)?;
    let search = rt.to_string(search_value)?;
    let replacement = Replacement::new(rt, replace_value)?;
    let search_length = rt.string_len(search)?;
    let Some(position) = rt.string_index_of(string, search, 0)? else {
        return string_result(string);
    };
    if let Some(function) = replacement.function {
        return FunctionReplace::start(rt, (string, search, function), position, false);
    }
    let length = rt.string_len(string)?;
    let template_length = match replacement.template {
        Some(template) => rt.string_len(template)?,
        None => 0,
    };
    let mut builder = Builder::with_capacity(length - search_length + template_length);
    builder.push_range(rt, string, 0, position)?;
    replacement.append(rt, &mut builder, search, string, position)?;
    builder.push_range(rt, string, position + search_length, length)?;
    string_result(builder.finish(rt)?)
}

/// `replaceValue` of `replace` and `replaceAll`: a function (called for
/// each match) or a template string.
struct Replacement {
    function: Option<Value>,
    template: Option<Gc<JsString>>,
    /// Whether the template has a `$`; a template without one is the
    /// replacement itself.
    has_dollar: bool,
}

impl Replacement {
    /// `IsCallable(replaceValue)`, else `ToString(replaceValue)` (rooted).
    fn new(rt: &mut Runtime, replace_value: Value) -> VmResult<Replacement> {
        if rt.is_callable(replace_value)? {
            return Ok(Replacement {
                function: Some(replace_value),
                template: None,
                has_dollar: false,
            });
        }
        let template = rt.to_string(replace_value)?;
        rt.flatten(template)?;
        let has_dollar = rt
            .heap
            .text(template)?
            .units()
            .any(|unit| unit == u16::from(b'$'));
        let length = rt.string_len(template)?;
        rt.charge_units(length)?;
        Ok(Replacement {
            function: None,
            template: Some(template),
            has_dollar,
        })
    }

    /// Appends the replacement for the match of `matched` at `position`
    /// in `string` (flat). A function is called with `undefined` as
    /// `this` and its result converted; a template goes through
    /// `GetSubstitution` with no captures.
    fn append(
        &self,
        rt: &mut Runtime,
        out: &mut Builder,
        matched: Gc<JsString>,
        string: Gc<JsString>,
        position: usize,
    ) -> VmResult<()> {
        if let Some(function) = self.function {
            return rt.with_scope(|rt| {
                let result = rt.call(
                    function,
                    Value::Undefined,
                    &[
                        matched.into(),
                        Value::number(position as f64),
                        string.into(),
                    ],
                )?;
                let text = rt.to_string(result)?;
                out.push_string(rt, text)
            });
        }
        let Some(template) = self.template else {
            return Ok(());
        };
        if !self.has_dollar {
            return out.push_string(rt, template);
        }
        get_substitution(
            rt,
            out,
            matched,
            string,
            position,
            &[],
            Value::Undefined,
            template,
        )
    }
}

/// `replace` and `replaceAll` with a string to search and a replacer
/// function, as a native loop (`vm/native_loop.rs`): a step appends the
/// text before the next match and calls the function (the next match is
/// searched first; nothing can observe the order); the next step appends
/// the string of its result. The registers hold the string, the searched
/// string and the function.
struct FunctionReplace {
    string: Gc<JsString>,
    search: Gc<JsString>,
    function: Value,
    out: Builder,
    search_length: usize,
    /// The end of the last match.
    end_of_last: usize,
    /// The next match.
    next: Option<usize>,
    /// `replaceAll`: every match.
    all: bool,
    /// Whether the call of a replacement runs.
    waiting: bool,
}

impl FunctionReplace {
    fn start(
        rt: &mut Runtime,
        (string, search, function): (Gc<JsString>, Gc<JsString>, Value),
        first: usize,
        all: bool,
    ) -> VmResult<NativeReturn> {
        let search_length = rt.string_len(search)?;
        let state = FunctionReplace {
            string,
            search,
            function,
            out: Builder::new(),
            search_length,
            end_of_last: 0,
            next: Some(first),
            all,
            waiting: false,
        };
        rt.native_loop(state, &[string.into(), search.into(), function])
    }
}

impl NativeLoop for FunctionReplace {
    fn step(&mut self, rt: &mut Runtime, _regs: usize, input: Value) -> VmResult<LoopStep> {
        let string = self.string;
        if self.waiting {
            self.waiting = false;
            let text = rt.to_string(input)?;
            rt.heap.record(text);
            self.out.push_string(rt, text)?;
        }
        if let Some(at) = self.next {
            self.out.push_range(rt, string, self.end_of_last, at)?;
            self.end_of_last = at + self.search_length;
            self.next = if self.all {
                let advance = self.search_length.max(1);
                rt.string_index_of(string, self.search, at + advance)?
            } else {
                None
            };
            let args = [self.search.into(), Value::number(at as f64), string.into()];
            rt.push_call(self.function, Value::Undefined, &args)?;
            self.waiting = true;
            return Ok(LoopStep::Call);
        }
        let length = rt.string_len(string)?;
        self.out.push_range(rt, string, self.end_of_last, length)?;
        let out = std::mem::replace(&mut self.out, Builder::new());
        Ok(LoopStep::Done(out.finish(rt)?.into()))
    }
}

// --- replaceAll ---

/// `String.prototype.replaceAll(searchValue, replaceValue)` (§22.1.3.20).
pub(super) fn replace_all(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = require_coercible(call, "replaceAll")?;
    let search_value = rt.arg(call, 0);
    let replace_value = rt.arg(call, 1);
    if matches!(search_value, Value::Object(_)) {
        if rt.is_regexp(search_value)? {
            check_global_flag(rt, search_value)?;
        }
        let key = rt.symbol_key(WellKnown::Replace);
        let replacer = rt.get_value(search_value, key)?;
        if !replacer.is_nullish() {
            if !rt.is_callable(replacer)? {
                return Err(VmError::type_error(format!(
                    "'{}' returned for property 'Symbol(Symbol.replace)' of object '{}' is not a function",
                    rt.primitive_text(replacer),
                    rt.primitive_text(search_value)
                )));
            }
            return Ok(dispatch(replacer, search_value, this, replace_value));
        }
    }
    let string = rt.to_string(this)?;
    let search = rt.to_string(search_value)?;
    let replacement = Replacement::new(rt, replace_value)?;
    let search_length = rt.string_len(search)?;
    let advance = search_length.max(1);
    let length = rt.string_len(string)?;
    let Some(first) = rt.string_index_of(string, search, 0)? else {
        return string_result(string);
    };
    if let Some(function) = replacement.function {
        return FunctionReplace::start(rt, (string, search, function), first, true);
    }
    let mut builder = Builder::new();
    let mut end_of_last = 0;
    let mut position = Some(first);
    while let Some(at) = position {
        rt.tick()?;
        builder.push_range(rt, string, end_of_last, at)?;
        replacement.append(rt, &mut builder, search, string, at)?;
        end_of_last = at + search_length;
        position = rt.string_index_of(string, search, at + advance)?;
    }
    builder.push_range(rt, string, end_of_last, length)?;
    string_result(builder.finish(rt)?)
}

/// Step 2.b of `replaceAll`: the flags of a regular expression must
/// contain `g`.
fn check_global_flag(rt: &mut Runtime, search_value: Value) -> VmResult<()> {
    let key = rt.heap.key_from_str("flags")?;
    rt.heap.record(key);
    let flags = rt.get_value(search_value, key)?;
    if flags.is_nullish() {
        return Err(VmError::type_error(
            "String.prototype.replaceAll called on null or undefined",
        ));
    }
    let flags = rt.to_string(flags)?;
    let has_g = rt
        .flat_text(flags)?
        .units()
        .any(|unit| unit == u16::from(b'g'));
    if has_g {
        Ok(())
    } else {
        Err(VmError::type_error(
            "String.prototype.replaceAll called with a non-global RegExp argument",
        ))
    }
}

// --- GetSubstitution ---

/// The unit `index` of a flat string.
fn unit_of(rt: &Runtime, string: Gc<JsString>, index: usize) -> VmResult<Option<u16>> {
    Ok(rt.heap.text(string)?.get(index))
}

/// The value of a decimal digit unit.
fn digit(unit: Option<u16>) -> Option<usize> {
    unit.filter(|u| (u16::from(b'0')..=u16::from(b'9')).contains(u))
        .map(|u| usize::from(u - u16::from(b'0')))
}

/// `GetSubstitution(matched, str, position, captures, namedCaptures,
/// replacementTemplate)` (§22.1.3.19.1), appended to `out`. `captures`
/// holds strings and `undefined`; `named_captures` is `undefined` or an
/// object. The caller roots every handle; `string` and `template` are
/// flattened here.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameters of the specification's abstract operation"
)]
pub(super) fn get_substitution(
    rt: &mut Runtime,
    out: &mut Builder,
    matched: Gc<JsString>,
    string: Gc<JsString>,
    position: usize,
    captures: &[Value],
    named_captures: Value,
    template: Gc<JsString>,
) -> VmResult<()> {
    rt.flatten(string)?;
    rt.flatten(template)?;
    let template_length = rt.string_len(template)?;
    let string_length = rt.string_len(string)?;
    let mut close = CloseScan::default();
    let mut index = 0;
    while index < template_length {
        // The literal run up to the next `$`.
        let mut dollar = index;
        while unit_of(rt, template, dollar)?.is_some_and(|u| u != u16::from(b'$')) {
            dollar += 1;
        }
        out.push_range(rt, template, index, dollar)?;
        if dollar >= template_length {
            break;
        }
        rt.tick()?;
        index = substitute_reference(
            rt,
            out,
            (matched, string, position, string_length),
            (captures, named_captures),
            (template, dollar),
            &mut close,
        )?;
    }
    Ok(())
}

/// The first `>` of the template at or after `from` (`None`: there is
/// none). The references of one template ask from increasing indices, so
/// the template is scanned at most once for all its `$<`.
#[derive(Default)]
struct CloseScan {
    from: usize,
    found: Option<usize>,
    scanned: bool,
}

impl CloseScan {
    /// The index of the first `>` at or after `start`.
    fn find(
        &mut self,
        rt: &mut Runtime,
        template: Gc<JsString>,
        start: usize,
    ) -> VmResult<Option<usize>> {
        let known = self.scanned && start >= self.from && self.found.is_none_or(|at| at >= start);
        if !known {
            let text = rt.heap.text(template)?;
            let found = (start..text.len()).find(|&i| text.get(i) == Some(u16::from(b'>')));
            let end = found.unwrap_or(text.len());
            rt.charge_units(end.saturating_sub(start))?;
            *self = Self {
                from: start,
                found,
                scanned: true,
            };
        }
        Ok(self.found)
    }
}

/// One `$` reference of the template at `dollar`; returns the index after
/// it.
fn substitute_reference(
    rt: &mut Runtime,
    out: &mut Builder,
    (matched, string, position, string_length): (Gc<JsString>, Gc<JsString>, usize, usize),
    (captures, named_captures): (&[Value], Value),
    (template, dollar): (Gc<JsString>, usize),
    close: &mut CloseScan,
) -> VmResult<usize> {
    let next = unit_of(rt, template, dollar + 1)?;
    match next.and_then(|u| u8::try_from(u).ok()) {
        Some(b'$') => {
            out.check(rt, 1)?;
            out.push_unit(u16::from(b'$'));
            Ok(dollar + 2)
        }
        Some(b'`') => {
            out.push_range(rt, string, 0, position)?;
            Ok(dollar + 2)
        }
        Some(b'&') => {
            out.push_string(rt, matched)?;
            Ok(dollar + 2)
        }
        Some(b'\'') => {
            let tail = position + rt.string_len(matched)?;
            out.push_range(rt, string, tail.min(string_length), string_length)?;
            Ok(dollar + 2)
        }
        Some(b'0'..=b'9') => {
            let first = digit(next).unwrap_or(0);
            let second = digit(unit_of(rt, template, dollar + 2)?);
            let (mut count, mut number) = match second {
                Some(second) => (2, first * 10 + second),
                None => (1, first),
            };
            if number > captures.len() && count == 2 {
                count = 1;
                number = first;
            }
            if (1..=captures.len()).contains(&number) {
                if let Value::String(capture) = captures[number - 1] {
                    out.push_string(rt, capture)?;
                }
            } else {
                out.push_range(rt, template, dollar, dollar + 1 + count)?;
            }
            Ok(dollar + 1 + count)
        }
        Some(b'<') => substitute_named(rt, out, named_captures, template, dollar, close),
        _ => {
            out.check(rt, 1)?;
            out.push_unit(u16::from(b'$'));
            Ok(dollar + 1)
        }
    }
}

/// The reference `$<name>`; without named captures or without a `>` it
/// stays as text. Returns the index after the reference.
fn substitute_named(
    rt: &mut Runtime,
    out: &mut Builder,
    named_captures: Value,
    template: Gc<JsString>,
    dollar: usize,
    close: &mut CloseScan,
) -> VmResult<usize> {
    let close = match named_captures {
        Value::Undefined => None,
        _ => close.find(rt, template, dollar + 2)?,
    };
    let Some(close) = close else {
        out.push_range(rt, template, dollar, dollar + 2)?;
        return Ok(dollar + 2);
    };
    rt.with_scope(|rt| {
        let name = rt.substring(template, dollar + 2, close)?;
        rt.heap.record(name);
        let key = rt.string_key(name)?;
        let capture = rt.get_value(named_captures, key)?;
        if !capture.is_undefined() {
            let text = rt.to_string(capture)?;
            out.push_string(rt, text)?;
        }
        Ok(())
    })?;
    Ok(close + 1)
}

// --- split ---

/// `String.prototype.split(separator, limit)` (§22.1.3.23).
pub(super) fn split(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = require_coercible(call, "split")?;
    let separator = rt.arg(call, 0);
    let limit = rt.arg(call, 1);
    if matches!(separator, Value::Object(_)) {
        let key = rt.symbol_key(WellKnown::Split);
        if let Some(splitter) = rt.get_method(separator, key)? {
            return Ok(dispatch(splitter, separator, this, limit));
        }
    }
    let string = rt.to_string(this)?;
    let limit = match limit {
        Value::Undefined => u32::MAX,
        value => to_uint32(rt.to_number(value)?),
    } as usize;
    let separator_string = rt.to_string(separator)?;
    if let Some(cache) = &rt.vm.split_cache
        && cache.subject == string
        && cache.separator == separator_string
        && cache.limit == limit
        && !separator.is_undefined()
    {
        // The same strings give the same pieces: copy them into a new
        // array in one step.
        let pieces = cache.pieces;
        let proto = rt.intrinsic(rt.current_realm(), Intrinsic::ArrayPrototype)?;
        let length = rt.heap.dense_elements(pieces)?.map_or(0, <[Value]>::len);
        rt.charge(length)?;
        let array = rt
            .heap
            .new_array_copy(Some(proto), pieces, &rt.vm)?
            .ok_or(VmError::invariant("the split cache is not dense"))?;
        rt.heap.record(array);
        return Ok(NativeReturn::Value(array.into()));
    }
    // The dense builder (`object/element_access.rs`): the pieces are
    // appended to the vector without the general property definition.
    let array = rt.new_array_with_capacity(limit.min(SPLIT_CAPACITY))?;
    if limit == 0 {
        return Ok(NativeReturn::Value(array.into()));
    }
    if separator.is_undefined() {
        push_piece(rt, array, 0, string.into())?;
        return Ok(NativeReturn::Value(array.into()));
    }
    let separator_length = rt.string_len(separator_string)?;
    let length = rt.string_len(string)?;
    if separator_length == 0 {
        split_units(rt, array, string, length.min(limit))?;
    } else if length == 0 {
        push_piece(rt, array, 0, string.into())?;
    } else {
        split_at_separator(rt, array, string, separator_string, limit)?;
    }
    remember_split(rt, string, separator_string, limit, array)?;
    Ok(NativeReturn::Value(array.into()))
}

/// Keeps the pieces of a `split` with few pieces for the next `split` of
/// the same strings (a loop that splits a constant string makes the same
/// pieces each time). The cache holds a private copy, so changes of the
/// returned array do not reach it.
fn remember_split(
    rt: &mut Runtime,
    subject: Gc<JsString>,
    separator: Gc<JsString>,
    limit: usize,
    array: Gc<Object>,
) -> VmResult<()> {
    let Some(values) = rt.heap.dense_elements(array)? else {
        return Ok(());
    };
    if values.len() > SPLIT_CACHE_PIECES {
        return Ok(());
    }
    let values = values.to_vec();
    let pieces = rt.heap.new_array_from(None, &values, &rt.vm)?;
    rt.vm.split_cache = Some(crate::vm::SplitCache {
        subject,
        separator,
        limit,
        pieces,
    });
    Ok(())
}

/// Appends `piece` as element `index` (the array has no holes).
fn push_piece(rt: &mut Runtime, array: Gc<Object>, index: usize, piece: Value) -> VmResult<()> {
    if rt.heap.push_element(array, piece)? {
        return Ok(());
    }
    rt.heap
        .create_data_property(array, PropertyKey::Index(index as u32), piece, &rt.vm)?;
    Ok(())
}

/// Checks the heap limit (and collects if due) after a number of pieces;
/// the array and the strings are rooted.
fn piece_safepoint(rt: &mut Runtime, count: usize) -> VmResult<()> {
    rt.tick()?;
    if count.is_multiple_of(SAFEPOINT_PIECES) {
        rt.heap.reserve(SAFEPOINT_PIECES * 64, &rt.vm)?;
    }
    Ok(())
}

/// Step 9 of `split`: an array of the first `count` code units.
fn split_units(
    rt: &mut Runtime,
    array: Gc<Object>,
    string: Gc<JsString>,
    count: usize,
) -> VmResult<()> {
    rt.flatten(string)?;
    for index in 0..count {
        piece_safepoint(rt, index + 1)?;
        let unit = rt
            .heap
            .text(string)?
            .get(index)
            .ok_or(VmError::invariant("a split position outside the string"))?;
        // No handle record: the array holds the piece before the next
        // safepoint.
        let piece = rt.heap.unit_string(unit)?;
        push_piece(rt, array, index, piece.into())?;
    }
    Ok(())
}

/// Steps 11 and 12 of `split`: the pieces between the separators.
fn split_at_separator(
    rt: &mut Runtime,
    array: Gc<Object>,
    string: Gc<JsString>,
    separator: Gc<JsString>,
    limit: usize,
) -> VmResult<()> {
    let separator_length = rt.string_len(separator)?;
    let length = rt.string_len(string)?;
    let mut count = 0;
    let mut start = 0;
    let mut found = rt.string_index_of(string, separator, 0)?;
    while let Some(at) = found {
        piece_safepoint(rt, count + 1)?;
        let piece = rt.substring(string, start, at)?;
        push_piece(rt, array, count, piece.into())?;
        count += 1;
        if count == limit {
            return Ok(());
        }
        start = at + separator_length;
        found = rt.string_index_of(string, separator, start)?;
    }
    let piece = rt.substring(string, start, length)?;
    push_piece(rt, array, count, piece.into())
}

#[cfg(test)]
mod tests {
    //! `GetSubstitution` with captures and named captures, as the
    //! `RegExp` methods call it; the expected texts are
    //! those of `'1abc2'.replace(/(b)(z)?/, template)` and the same with
    //! `(?<n>b)(?<m>z)?` in Node.js 22.

    use super::*;
    use crate::runtime::{Runtime, RuntimeConfig};

    /// `GetSubstitution("b", "1abc2", 2, captures, named, template)`;
    /// `named` is the source of the object of named captures.
    fn substitute(template: &str, captures: &[Option<&str>], named: Option<&str>) -> String {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        let named = named.map_or(Value::Undefined, |source| {
            rt.eval(source).expect("an object of named captures")
        });
        rt.scoped(|rt| {
            let matched = rt.heap.alloc_str("b")?;
            rt.heap.record(matched);
            let string = rt.heap.alloc_str("1abc2")?;
            rt.heap.record(string);
            let template = rt.heap.alloc_str(template)?;
            rt.heap.record(template);
            let mut values = Vec::new();
            for capture in captures {
                values.push(match capture {
                    Some(text) => {
                        let text = rt.heap.alloc_str(text)?;
                        rt.heap.record(text);
                        Value::String(text)
                    }
                    None => Value::Undefined,
                });
            }
            let mut out = Builder::new();
            get_substitution(rt, &mut out, matched, string, 2, &values, named, template)?;
            let result = out.finish(rt)?;
            Ok(rt.heap.string_lossy(result, 1000))
        })
        .expect("a substitution")
    }

    #[test]
    fn numbered_captures() {
        let captures = [Some("b"), None];
        assert_eq!(
            substitute("$1|$2|$3|$01|$10|$11|$001|$00|$0", &captures, None),
            "b||$3|b|b0|b1|$001|$00|$0"
        );
        // Two digits when the capture exists.
        let many: Vec<Option<&str>> = (0..12).map(|_| Some("c")).collect();
        assert_eq!(substitute("$11|$12|$13", &many, None), "c|c|c3");
    }

    #[test]
    fn specials() {
        assert_eq!(substitute("$$ $& $` $' $x $", &[], None), "$ b 1a c2 $x $");
    }

    #[test]
    fn named_captures() {
        let named = "({ n: 'b', m: undefined })";
        assert_eq!(
            substitute("$<n>|$<m>|$<z>|$<n|$<>", &[], Some(named)),
            "b|||"
        );
        // After the last `>`, each `$<` stays as text.
        assert_eq!(substitute("$<n>$<m$<n$<", &[], Some(named)), "b$<m$<n$<");
        // Without named captures the reference stays.
        assert_eq!(substitute("$<n>|$<n", &[], None), "$<n>|$<n");
        // The value converts with `ToString`.
        assert_eq!(substitute("$<n>", &[], Some("({ n: 42 })")), "42");
    }
}
