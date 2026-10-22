//! `RegExp.prototype[@@replace]` (ECMA-262 §22.2.6.11), split from
//! `regexp_symbol.rs`.
//!
//! `@@replace` collects the results before it builds the string, as the
//! specification does, when a replacer function or a script's `exec` can
//! observe the order; with a template and the built-in `exec` it appends
//! each replacement as the match is found (the same result: the reads of
//! the results and `GetSubstitution` run no script code). Collected
//! matches are spans in a vector charged to the heap; results of a
//! script's `exec` are kept in an internal array.
//!
use std::rc::Rc;

use swb_js_regexp::{Captures, Flags, Regex};

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::regexp_exec::{Exec, advance_string_index};
use super::regexp_symbol::has_letter;
use super::string::Builder;
use super::string_replace::get_substitution;

/// The captures that a template can name (`$1` to `$99`).
const TEMPLATE_CAPTURES: usize = 99;

/// The replacement of `@@replace`: a function or a template (rooted).
#[derive(Clone, Copy)]
enum Replacer {
    Function(Value),
    Template { text: Gc<JsString>, dollar: bool },
}

/// A result that `@@replace` processes after the loop.
#[derive(Clone, Copy)]
enum Pending {
    /// A built-in match: its spans start at this index of `spans`.
    Match { at: usize, regex: usize },
    /// The result of a script's `exec`: its index in the internal array.
    Object(u32),
}

/// The results that `@@replace` collected (step 12) when it cannot
/// append them at once.
struct Collected {
    pending: Vec<Pending>,
    /// For each built-in match: the number of groups plus one, then a
    /// start and an end per span (`u32::MAX` for a group that did not
    /// take part).
    spans: Vec<u32>,
    /// The programs of the matches (one entry per change of program).
    regexes: Vec<Rc<Regex>>,
    /// The results of a script's `exec` (an internal array).
    objects: Option<Gc<Object>>,
    /// The bytes charged for the vectors.
    charged: usize,
}

impl Collected {
    fn new() -> Self {
        Collected {
            pending: Vec::new(),
            spans: Vec::new(),
            regexes: Vec::new(),
            objects: None,
            charged: 0,
        }
    }

    /// The bytes of the vectors.
    fn bytes(&self) -> usize {
        self.pending.capacity() * size_of::<Pending>() + self.spans.capacity() * 4
    }

    /// Checks the heap limit for the growth of the vectors and charges it.
    fn account(&mut self, rt: &mut Runtime, extra: usize) -> VmResult<()> {
        let needed = self.bytes() + extra * 2;
        if needed > self.charged {
            let growth = needed - self.charged;
            rt.heap.reserve(growth, &rt.vm)?;
            rt.heap.charge(growth);
            self.charged = needed;
        }
        Ok(())
    }

    fn push_match(
        &mut self,
        rt: &mut Runtime,
        captures: &Captures,
        regex: Rc<Regex>,
    ) -> VmResult<()> {
        self.account(rt, (captures.len() * 2 + 1) * 4 + size_of::<Pending>())?;
        if !self
            .regexes
            .last()
            .is_some_and(|last| Rc::ptr_eq(last, &regex))
        {
            self.regexes.push(regex);
        }
        let at = self.spans.len();
        self.spans.push(captures.len() as u32);
        for span in captures.spans() {
            let (start, end) = span.map_or((u32::MAX, u32::MAX), |(s, e)| (s as u32, e as u32));
            self.spans.push(start);
            self.spans.push(end);
        }
        self.pending.push(Pending::Match {
            at,
            regex: self.regexes.len() - 1,
        });
        Ok(())
    }

    fn push_object(&mut self, rt: &mut Runtime, object: Gc<Object>) -> VmResult<()> {
        self.account(rt, size_of::<Pending>())?;
        let objects = if let Some(objects) = self.objects {
            objects
        } else {
            let objects = rt.heap.new_array(None, 0)?;
            self.objects = Some(rt.heap.record(objects));
            objects
        };
        let (length, _) = rt.heap.array_length(objects)?;
        rt.push_value(objects, object.into())?;
        self.pending.push(Pending::Object(length));
        Ok(())
    }

    /// The captures of the built-in match at `at`.
    fn captures(&self, at: usize) -> Vec<Option<(usize, usize)>> {
        let mut out = Vec::new();
        self.captures_into(at, &mut out);
        out
    }

    /// [`Collected::captures`] into a vector that the caller reuses.
    fn captures_into(&self, at: usize, out: &mut Vec<Option<(usize, usize)>>) {
        let count = self.spans.get(at).copied().unwrap_or(0) as usize;
        out.clear();
        out.extend((0..count).map(|i| {
            let start = self.spans.get(at + 1 + 2 * i).copied().unwrap_or(u32::MAX);
            let end = self.spans.get(at + 2 + 2 * i).copied().unwrap_or(u32::MAX);
            (start != u32::MAX).then_some((start as usize, end as usize))
        }));
    }
}

/// The output of `@@replace` (steps 13 to 17).
struct Accumulator {
    out: Builder,
    next_source: usize,
}

/// `RegExp.prototype[@@replace](string, replaceValue)` (§22.2.6.11).
pub(super) fn symbol_replace(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.@@replace")?;
    let string = rt.flat_string_arg(call)?;
    let length = rt.string_len(string)?;
    let replace_value = rt.arg(call, 1);
    let replacer = if rt.is_callable(replace_value)? {
        Replacer::Function(replace_value)
    } else {
        let text = rt.to_string(replace_value)?;
        rt.heap.record(text);
        rt.flatten(text)?;
        let dollar = has_letter(rt, text, b'$')?;
        Replacer::Template { text, dollar }
    };
    let (global, unicode) = rt.flag_info(regexp)?;
    let mut acc = Accumulator {
        out: Builder::new(),
        next_source: 0,
    };
    // A replacer function sees the results after the loop; a template is
    // applied at once while every result is a built-in match.
    let mut collected = matches!(replacer, Replacer::Function(_)).then(Collected::new);
    if global && rt.regexp_is_pristine(regexp, false)? {
        let regex = rt.regexp_program(regexp)?;
        pristine_global_loop(rt, regexp, string, &regex, |rt, captures| {
            rt.with_scope(|rt| match collected.as_mut() {
                Some(collected) => collected.push_match(rt, captures, Rc::clone(&regex)),
                None => apply_match(rt, &mut acc, string, captures.spans(), &regex, replacer),
            })
        })?;
    } else {
        replace_loop(
            rt,
            regexp,
            string,
            (global, unicode),
            &mut acc,
            &mut collected,
            replacer,
        )?;
    }
    if let Replacer::Function(function) = replacer
        && let Some(collected) = collected.take_if(|collected| !collected.pending.is_empty())
    {
        // The replacer runs for the collected results in a native loop.
        let objects = collected.objects.map_or(Value::Undefined, Value::Object);
        let registers = [string.into(), function, objects];
        let state = ReplaceLoop {
            string,
            function,
            collected,
            acc,
            length,
            next: 0,
            waiting: None,
            spans: Vec::new(),
            args: Vec::new(),
        };
        return rt.native_loop(state, &registers);
    }
    if let Some(collected) = &collected {
        apply_collected(rt, &mut acc, string, collected, replacer)?;
    }
    if acc.next_source < length {
        acc.out.push_range(rt, string, acc.next_source, length)?;
    }
    let result = acc.out.finish(rt)?;
    Ok(NativeReturn::Value(result.into()))
}

/// The global loop of `RegExpBuiltinExec` on a pristine `RegExp` object
/// (`regexp_fast.rs`): `each` gets the matches in order, from `lastIndex`
/// 0, each next search starting at the end of the last match (one
/// character further after an empty match), until a search fails or
/// starts beyond the end; then `lastIndex` is 0 again and the legacy
/// static properties have the last match. These are the steps of the
/// loops of `@@match` and `@@replace`, without the reads and writes of
/// `lastIndex`, which no script code can observe in between.
pub(super) fn pristine_global_loop(
    rt: &mut Runtime,
    regexp: Gc<Object>,
    string: Gc<JsString>,
    regex: &Rc<Regex>,
    mut each: impl FnMut(&mut Runtime, &Captures) -> VmResult<()>,
) -> VmResult<()> {
    let length = rt.string_len(string)?;
    let unicode =
        regex.flags().contains(Flags::UNICODE) || regex.flags().contains(Flags::UNICODE_SETS);
    let mut position = 0usize;
    let mut last = None;
    loop {
        rt.tick()?;
        let Some(captures) = rt.regexp_run(regex, string, position)? else {
            break;
        };
        let (start, end) = captures.get(0).unwrap_or((0, 0));
        each(rt, &captures)?;
        position = if start == end {
            advance_string_index(rt.heap.text(string)?, end as f64, unicode) as usize
        } else {
            end
        };
        last = Some(captures);
        if position > length {
            break;
        }
    }
    rt.set_last_index(regexp, 0.0)?;
    if let Some(last) = &last {
        rt.note_last_match(string, last);
    }
    Ok(())
}

/// Step 12 of `@@replace` through `RegExpExec`.
fn replace_loop(
    rt: &mut Runtime,
    regexp: Gc<Object>,
    string: Gc<JsString>,
    (global, unicode): (bool, bool),
    acc: &mut Accumulator,
    collected: &mut Option<Collected>,
    replacer: Replacer,
) -> VmResult<()> {
    if global {
        rt.set_last_index(regexp, 0.0)?;
    }
    let mut objects_rooted = false;
    loop {
        rt.tick()?;
        let more = rt.with_scope(|rt| {
            let exec = rt.regexp_exec(regexp, string)?;
            let empty = match &exec {
                Exec::Null => return Ok(false),
                Exec::Match(captures, regex) => {
                    let (start, end) = captures.get(0).unwrap_or((0, 0));
                    if let Some(collected) = collected.as_mut() {
                        collected.push_match(rt, captures, Rc::clone(regex))?;
                    } else {
                        apply_match(rt, acc, string, captures.spans(), regex, replacer)?;
                    }
                    start == end
                }
                Exec::Object(object) => {
                    collected
                        .get_or_insert_with(Collected::new)
                        .push_object(rt, *object)?;
                    if global {
                        let matched = rt.result_match_string(string, &exec)?;
                        rt.string_len(matched)? == 0
                    } else {
                        false
                    }
                }
            };
            if !global {
                return Ok(false);
            }
            if empty {
                rt.advance_last_index(regexp, string, unicode)?;
            }
            Ok(true)
        })?;
        // The internal array of results is made in the scope of an
        // iteration; the outer scope keeps it from now on.
        if !objects_rooted && let Some(objects) = collected.as_ref().and_then(|c| c.objects) {
            rt.heap.record(objects);
            objects_rooted = true;
        }
        if !more {
            return Ok(());
        }
    }
}

/// Steps 14 and 15 of `@@replace` for the collected results, in order.
fn apply_collected(
    rt: &mut Runtime,
    acc: &mut Accumulator,
    string: Gc<JsString>,
    collected: &Collected,
    replacer: Replacer,
) -> VmResult<()> {
    for pending in &collected.pending {
        rt.tick()?;
        rt.with_scope(|rt| match *pending {
            Pending::Match { at, regex } => {
                let regex = collected
                    .regexes
                    .get(regex)
                    .ok_or(VmError::invariant("a collected match without its program"))?;
                let spans = collected.captures(at);
                apply_match(rt, acc, string, &spans, regex, replacer)
            }
            Pending::Object(index) => {
                let objects = collected
                    .objects
                    .ok_or(VmError::invariant("a collected result without its array"))?;
                let object = rt.get_value(objects.into(), PropertyKey::Index(index))?;
                apply_object(rt, acc, string, object, replacer)
            }
        })?;
    }
    Ok(())
}

/// What step 15 of `@@replace` reads from a result: the matched string,
/// its position, the captures (`values` from `first` on) and the groups
/// (rooted).
struct Parts {
    matched: Gc<JsString>,
    position: usize,
    values: Vec<Value>,
    first: usize,
    named: Value,
}

impl Parts {
    fn captures(&self) -> &[Value] {
        self.values.get(self.first..).unwrap_or(&[])
    }
}

/// Step 15 for a built-in match: the matched string, the captures and
/// the groups come from the spans.
fn apply_match(
    rt: &mut Runtime,
    acc: &mut Accumulator,
    string: Gc<JsString>,
    spans: &[Option<(usize, usize)>],
    regex: &Regex,
    replacer: Replacer,
) -> VmResult<()> {
    let (position, end) = spans.first().copied().flatten().unwrap_or((0, 0));
    let template_only = matches!(replacer, Replacer::Template { dollar: false, .. });
    if template_only {
        // No `$`: the replacement is the template itself.
        if position >= acc.next_source {
            acc.out.push_range(rt, string, acc.next_source, position)?;
            if let Replacer::Template { text, .. } = replacer {
                acc.out.push_string(rt, text)?;
            }
            acc.next_source = end;
        }
        return Ok(());
    }
    let mut values = Vec::with_capacity(spans.len());
    for span in spans {
        values.push(match *span {
            Some((start, end)) => {
                let piece = rt.substring(string, start, end)?;
                Value::String(rt.heap.record(piece))
            }
            None => Value::Undefined,
        });
    }
    let named = rt.groups_object(regex, &values)?;
    let Value::String(matched) = values.first().copied().unwrap_or_default() else {
        return Err(VmError::invariant("a match without its string"));
    };
    let captures = values.get(1..).unwrap_or(&[]);
    replace_one(
        rt,
        acc,
        string,
        (matched, position),
        captures,
        named,
        replacer,
    )
}

/// The [`Parts`] of a built-in match (the steps of [`apply_match`] for
/// the replacer loop).
fn match_parts(
    rt: &mut Runtime,
    string: Gc<JsString>,
    spans: &[Option<(usize, usize)>],
    regex: &Regex,
) -> VmResult<Parts> {
    let (position, _) = spans.first().copied().flatten().unwrap_or((0, 0));
    let mut values = Vec::with_capacity(spans.len());
    for span in spans {
        values.push(match *span {
            Some((start, end)) => {
                let piece = rt.substring(string, start, end)?;
                Value::String(rt.heap.record(piece))
            }
            None => Value::Undefined,
        });
    }
    let named = rt.groups_object(regex, &values)?;
    let Some(&Value::String(matched)) = values.first() else {
        return Err(VmError::invariant("a match without its string"));
    };
    Ok(Parts {
        matched,
        position,
        values,
        first: 1,
        named,
    })
}

/// Step 15 for the result of a script's `exec`: the reads of the
/// specification, in its order.
fn apply_object(
    rt: &mut Runtime,
    acc: &mut Accumulator,
    string: Gc<JsString>,
    result: Value,
    replacer: Replacer,
) -> VmResult<()> {
    let template = matches!(replacer, Replacer::Template { .. });
    let parts = object_parts(rt, string, result, template)?;
    replace_one(
        rt,
        acc,
        string,
        (parts.matched, parts.position),
        parts.captures(),
        parts.named,
        replacer,
    )
}

/// The [`Parts`] of the result of a script's `exec`, read in the order of
/// the specification (a template uses at most [`TEMPLATE_CAPTURES`]
/// captures).
fn object_parts(
    rt: &mut Runtime,
    string: Gc<JsString>,
    result: Value,
    template: bool,
) -> VmResult<Parts> {
    let Value::Object(object) = result else {
        return Err(VmError::invariant(
            "a collected result that is not an object",
        ));
    };
    let result_length = rt.length_of_array_like(object)?;
    let count = (result_length - 1.0).max(0.0);
    let matched = rt.get_value(result, PropertyKey::Index(0))?;
    let matched = rt.to_string(matched)?;
    rt.heap.record(matched);
    let key = PropertyKey::String(rt.vm.atoms.index);
    let position = rt.get_value(result, key)?;
    let position = to_integer_or_infinity(rt.to_number(position)?);
    let length = rt.string_len(string)?;
    let position = position.clamp(0.0, length as f64) as usize;
    // The captures go into an internal array: their number is the
    // script's `length`.
    let list = rt.heap.new_array(None, 0)?;
    rt.heap.record(list);
    let mut n = 1.0;
    while n <= count {
        rt.tick()?;
        let key = rt.index_key(n)?;
        let capture = rt.get_value(result, key)?;
        let capture = if capture.is_undefined() {
            capture
        } else {
            Value::String(rt.to_string(capture)?)
        };
        rt.push_value(list, capture)?;
        n += 1.0;
    }
    let key = PropertyKey::String(rt.vm.atoms.groups);
    let named = rt.get_value(result, key)?;
    rt.heap.record(named);
    let mut captures = rt
        .heap
        .dense_elements(list)?
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    if template {
        captures.truncate(TEMPLATE_CAPTURES);
    }
    Ok(Parts {
        matched,
        position,
        values: captures,
        first: 0,
        named,
    })
}

/// Steps 14 and 15 of `@@replace` with a replacer function, as a native
/// loop (`vm/native_loop.rs`): a step reads the next collected result and
/// calls the function; the next one appends the string of its result.
/// The registers hold the string, the function and the internal array
/// of the results of a script's `exec`.
struct ReplaceLoop {
    string: Gc<JsString>,
    function: Value,
    collected: Collected,
    acc: Accumulator,
    /// The length of the string.
    length: usize,
    /// The next result.
    next: usize,
    /// The position and the length of the match whose call runs, and
    /// whether it is in order (`position >= nextSourcePosition`).
    waiting: Option<(usize, usize, bool)>,
    /// Vectors that the steps reuse: the spans of a match and the
    /// arguments of a call.
    spans: Vec<Option<(usize, usize)>>,
    args: Vec<Value>,
}

impl ReplaceLoop {
    /// The [`Parts`] of the collected result `pending`.
    fn parts(&mut self, rt: &mut Runtime, pending: Pending) -> VmResult<Parts> {
        match pending {
            Pending::Match { at, regex } => {
                let regex = self
                    .collected
                    .regexes
                    .get(regex)
                    .ok_or(VmError::invariant("a collected match without its program"))?;
                self.collected.captures_into(at, &mut self.spans);
                match_parts(rt, self.string, &self.spans, regex)
            }
            Pending::Object(index) => {
                let objects = self
                    .collected
                    .objects
                    .ok_or(VmError::invariant("a collected result without its array"))?;
                let object = rt.get_value(objects.into(), PropertyKey::Index(index))?;
                object_parts(rt, self.string, object, false)
            }
        }
    }
}

impl crate::vm::native_loop::NativeLoop for ReplaceLoop {
    fn step(
        &mut self,
        rt: &mut Runtime,
        _regs: usize,
        input: Value,
    ) -> VmResult<crate::vm::native_loop::LoopStep> {
        let string = self.string;
        if let Some((position, match_length, in_order)) = self.waiting.take() {
            let replacement = rt.to_string(input)?;
            rt.heap.record(replacement);
            if in_order {
                self.acc
                    .out
                    .push_range(rt, string, self.acc.next_source, position)?;
                self.acc.out.push_string(rt, replacement)?;
                self.acc.next_source = position + match_length;
            }
        }
        if let Some(&pending) = self.collected.pending.get(self.next) {
            self.next += 1;
            let parts = self.parts(rt, pending)?;
            let match_length = rt.string_len(parts.matched)?;
            let in_order = parts.position >= self.acc.next_source;
            if parts.captures().len() + 4 > rt.vm.stack_limit {
                return Err(VmError::range_error(crate::vm::STACK_OVERFLOW));
            }
            let args = &mut self.args;
            args.clear();
            args.push(parts.matched.into());
            args.extend_from_slice(parts.captures());
            args.push(Value::number(parts.position as f64));
            args.push(string.into());
            if !parts.named.is_undefined() {
                args.push(parts.named);
            }
            rt.push_call(self.function, Value::Undefined, args)?;
            self.waiting = Some((parts.position, match_length, in_order));
            return Ok(crate::vm::native_loop::LoopStep::Call);
        }
        if self.acc.next_source < self.length {
            self.acc
                .out
                .push_range(rt, string, self.acc.next_source, self.length)?;
        }
        let out = std::mem::replace(&mut self.acc.out, Builder::new());
        let result = out.finish(rt)?;
        Ok(crate::vm::native_loop::LoopStep::Done(result.into()))
    }
}

/// The replacement of one result and its place in the output (steps
/// 15.k to 15.p). The values are rooted.
fn replace_one(
    rt: &mut Runtime,
    acc: &mut Accumulator,
    string: Gc<JsString>,
    (matched, position): (Gc<JsString>, usize),
    captures: &[Value],
    named: Value,
    replacer: Replacer,
) -> VmResult<()> {
    let match_length = rt.string_len(matched)?;
    let in_order = position >= acc.next_source;
    match replacer {
        Replacer::Function(function) => {
            if captures.len() + 4 > rt.vm.stack_limit {
                return Err(VmError::range_error(crate::vm::STACK_OVERFLOW));
            }
            let mut args = Vec::with_capacity(captures.len() + 4);
            args.push(matched.into());
            args.extend_from_slice(captures);
            args.push(Value::number(position as f64));
            args.push(string.into());
            if !named.is_undefined() {
                args.push(named);
            }
            let value = rt.call(function, Value::Undefined, &args)?;
            let replacement = rt.to_string(value)?;
            rt.heap.record(replacement);
            if in_order {
                acc.out.push_range(rt, string, acc.next_source, position)?;
                acc.out.push_string(rt, replacement)?;
            }
        }
        Replacer::Template { text, .. } => {
            let named = if named.is_undefined() {
                named
            } else {
                Value::Object(rt.this_object_value(named)?)
            };
            if in_order {
                acc.out.push_range(rt, string, acc.next_source, position)?;
                get_substitution(
                    rt,
                    &mut acc.out,
                    matched,
                    string,
                    position,
                    captures,
                    named,
                    text,
                )?;
            } else {
                let mut scratch = Builder::new();
                get_substitution(
                    rt,
                    &mut scratch,
                    matched,
                    string,
                    position,
                    captures,
                    named,
                    text,
                )?;
            }
        }
    }
    if in_order {
        acc.next_source = position + match_length;
    }
    Ok(())
}

impl Runtime {
    /// `ToObject(value)`, rooted.
    fn this_object_value(&mut self, value: Value) -> VmResult<Gc<Object>> {
        let object = self.to_object(value)?;
        Ok(self.heap.record(object))
    }
}
