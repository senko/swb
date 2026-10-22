//! The symbol methods of `RegExp.prototype` (ECMA-262 §22.2.6.8 to
//! §22.2.6.14): `@@match`, `@@matchAll` with `%RegExpStringIteratorPrototype%`
//! (§22.2.9), `@@replace`, `@@search` and `@@split`.
//!
//! Each follows the specification step by step through `RegExpExec`. When
//! `exec` is the built-in one, a result is the captures of the match
//! ([`Exec::Match`]): the reads of a result array that no script can see
//! (`0`, `length`, `index`, the captures, `groups`) come from them.
//!
//! `@@replace` collects the results before it builds the string, as the
//! specification does, when a replacer function or a script's `exec` can
//! observe the order; with a template and the built-in `exec` it appends
//! each replacement as the match is found (the same result: the reads of
//! the results and `GetSubstitution` run no script code). Collected
//! matches are spans in a vector charged to the heap; results of a
//! script's `exec` are kept in an internal array.
//!
//! `@@split` of a pristine `RegExp` (`regexp_fast.rs`) without the `y`
//! flag searches each next match from the current position with the
//! object's own program, which gives the matches of the sticky splitter
//! at each position without making the splitter. All other cases make
//! the splitter with the species constructor.

use std::rc::Rc;

use swb_js_regexp::{Captures, Flags, Regex};

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, RegExpStringIterator};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::{to_integer_or_infinity, to_uint32};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

use super::regexp_exec::{Exec, advance_string_index};
use super::string::Builder;
use super::string_replace::get_substitution;

/// The captures that a template can name (`$1` to `$99`).
const TEMPLATE_CAPTURES: usize = 99;

pub(super) fn install(rt: &mut Runtime, realm: u32, proto: Gc<Object>) -> VmResult<()> {
    let methods: [(WellKnown, u32, crate::vm::NativeFn); 5] = [
        (WellKnown::Match, 1, symbol_match),
        (WellKnown::MatchAll, 1, symbol_match_all),
        (WellKnown::Replace, 2, symbol_replace),
        (WellKnown::Search, 1, symbol_search),
        (WellKnown::Split, 2, symbol_split),
    ];
    for (which, length, func) in methods {
        super::symbol_method(rt, realm, proto, which, length, func, (true, true))?;
    }
    let iterator_proto = rt.intrinsic(realm, Intrinsic::RegExpStringIteratorPrototype)?;
    let next = super::method(rt, realm, iterator_proto, "next", 0, iterator_next)?;
    rt.set_intrinsic(realm, Intrinsic::RegExpStringIteratorNext, next);
    super::to_string_tag(rt, iterator_proto, "RegExp String Iterator")
}

/// Whether the text of a flags string contains a letter.
fn has_letter(rt: &Runtime, flags: Gc<JsString>, letter: u8) -> VmResult<bool> {
    Ok(rt
        .heap
        .text(flags)?
        .units()
        .any(|unit| unit == u16::from(letter)))
}

impl Runtime {
    /// `ToString(argument 0)`, rooted and flat.
    fn flat_string_arg(&mut self, call: &NativeCall) -> VmResult<Gc<JsString>> {
        let string = self.string_arg(call, 0)?;
        self.heap.record(string);
        self.flatten(string)?;
        Ok(string)
    }

    /// Whether `ToString(Get(R, "flags"))` contains `g`, and `u` or `v`.
    fn flag_info(&mut self, regexp: Gc<Object>) -> VmResult<(bool, bool)> {
        if let Some(flags) = self.pristine_flags(regexp)? {
            let unicode = flags.contains(Flags::UNICODE) || flags.contains(Flags::UNICODE_SETS);
            return Ok((flags.contains(Flags::GLOBAL), unicode));
        }
        let (_, global, unicode) = self.flags_of(regexp)?;
        Ok((global, unicode))
    }

    /// The flags string of `R` (rooted, flat) and whether it has `g` and
    /// `u` or `v`.
    fn flags_of(&mut self, regexp: Gc<Object>) -> VmResult<(Gc<JsString>, bool, bool)> {
        let flags = self.get_flags_string(regexp)?;
        self.flatten(flags)?;
        let global = has_letter(self, flags, b'g')?;
        let unicode = has_letter(self, flags, b'u')? || has_letter(self, flags, b'v')?;
        Ok((flags, global, unicode))
    }

    /// The steps after an empty match of the global loops: `lastIndex` =
    /// `AdvanceStringIndex(S, ToLength(Get(R, "lastIndex")), unicode)`.
    fn advance_last_index(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
        unicode: bool,
    ) -> VmResult<()> {
        let this_index = self.last_index(regexp)?;
        let next = advance_string_index(self.heap.text(string)?, this_index, unicode);
        self.set_last_index(regexp, next)
    }

    /// `ToString(Get(result, "0"))` of a result, rooted.
    fn result_match_string(&mut self, string: Gc<JsString>, exec: &Exec) -> VmResult<Gc<JsString>> {
        match exec {
            Exec::Match(captures, _) => {
                let (start, end) = captures.get(0).unwrap_or((0, 0));
                let piece = self.substring(string, start, end)?;
                Ok(self.heap.record(piece))
            }
            Exec::Object(object) => {
                let value = self.get_value((*object).into(), PropertyKey::Index(0))?;
                let text = self.to_string(value)?;
                Ok(self.heap.record(text))
            }
            Exec::Null => Ok(self.vm.atoms.empty),
        }
    }

    /// `SpeciesConstructor(R, %RegExp%)` (§7.3.22) with V8's messages.
    fn regexp_species_constructor(&mut self, regexp: Gc<Object>) -> VmResult<Value> {
        let default =
            Value::Object(self.intrinsic(self.current_realm(), Intrinsic::RegExpConstructor)?);
        let key = PropertyKey::String(self.vm.atoms.constructor);
        let constructor = self.get_value(regexp.into(), key)?;
        self.heap.record(constructor);
        let Value::Object(_) = constructor else {
            if constructor.is_undefined() {
                return Ok(default);
            }
            return Err(VmError::type_error(
                "The .constructor property is not an object",
            ));
        };
        let key = self.symbol_key(WellKnown::Species);
        let species = self.get_value(constructor, key)?;
        if species.is_nullish() {
            return Ok(default);
        }
        if self.is_constructor(species)? {
            return Ok(self.heap.record(species));
        }
        Err(VmError::type_error(
            "object.constructor[Symbol.species] is not a constructor",
        ))
    }

    /// `Construct(C, « R, flags »)` of `@@split` and `@@matchAll`: an object
    /// (rooted).
    fn construct_regexp(
        &mut self,
        constructor: Value,
        regexp: Gc<Object>,
        flags: Gc<JsString>,
    ) -> VmResult<Gc<Object>> {
        match self.construct(constructor, &[regexp.into(), flags.into()])? {
            Value::Object(object) => Ok(self.heap.record(object)),
            _ => Err(VmError::invariant("a constructor returned a primitive")),
        }
    }
}

// --- @@match ---

/// `RegExp.prototype[@@match](string)` (§22.2.6.8).
fn symbol_match(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.@@match")?;
    let string = rt.flat_string_arg(call)?;
    let (global, unicode) = rt.flag_info(regexp)?;
    if !global {
        return Ok(NativeReturn::Value(rt.regexp_exec_value(regexp, string)?));
    }
    if rt.regexp_is_pristine(regexp, false)? {
        let regex = rt.regexp_program(regexp)?;
        let array = rt.new_array()?;
        pristine_global_loop(rt, regexp, string, &regex, |rt, captures| {
            let (start, end) = captures.get(0).unwrap_or((0, 0));
            let piece = rt.substring(string, start, end)?;
            rt.push_value(array, piece.into())
        })?;
        let (count, _) = rt.heap.array_length(array)?;
        return Ok(NativeReturn::Value(if count == 0 {
            Value::Null
        } else {
            array.into()
        }));
    }
    rt.set_last_index(regexp, 0.0)?;
    let array = rt.new_array()?;
    let mut count = 0u32;
    loop {
        rt.tick()?;
        let more = rt.with_scope(|rt| {
            let exec = rt.regexp_exec(regexp, string)?;
            if matches!(exec, Exec::Null) {
                return Ok(false);
            }
            let matched = rt.result_match_string(string, &exec)?;
            rt.push_value(array, matched.into())?;
            if rt.string_len(matched)? == 0 {
                rt.advance_last_index(regexp, string, unicode)?;
            }
            Ok(true)
        })?;
        if !more {
            break;
        }
        count += 1;
    }
    Ok(NativeReturn::Value(if count == 0 {
        Value::Null
    } else {
        array.into()
    }))
}

// --- @@search ---

/// `RegExp.prototype[@@search](string)` (§22.2.6.12).
fn symbol_search(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.@@search")?;
    let string = rt.flat_string_arg(call)?;
    let key = PropertyKey::String(rt.vm.atoms.last_index);
    let previous = rt.get_value(regexp.into(), key)?;
    rt.heap.record(previous);
    if !rt.heap.same_value(previous, Value::Int(0))? {
        rt.set_or_throw(regexp, key, Value::Int(0))?;
    }
    let exec = rt.regexp_exec(regexp, string)?;
    let current = rt.get_value(regexp.into(), key)?;
    if !rt.heap.same_value(current, previous)? {
        rt.set_or_throw(regexp, key, previous)?;
    }
    let index = match exec {
        Exec::Null => Value::Int(-1),
        Exec::Match(captures, _) => Value::number(captures.get(0).map_or(0, |(s, _)| s) as f64),
        Exec::Object(object) => {
            let key = PropertyKey::String(rt.vm.atoms.index);
            rt.get_value(object.into(), key)?
        }
    };
    Ok(NativeReturn::Value(index))
}

// --- @@matchAll ---

/// `RegExp.prototype[@@matchAll](string)` (§22.2.6.9).
fn symbol_match_all(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.@@matchAll")?;
    let string = rt.flat_string_arg(call)?;
    let constructor = rt.regexp_species_constructor(regexp)?;
    let (flags, global, unicode) = rt.flags_of(regexp)?;
    let matcher = rt.construct_regexp(constructor, regexp, flags)?;
    let last_index = rt.last_index(regexp)?;
    rt.set_last_index(matcher, last_index)?;
    let proto = rt.intrinsic(call.realm(), Intrinsic::RegExpStringIteratorPrototype)?;
    let state = RegExpStringIterator {
        matcher: Some(matcher),
        string,
        global,
        full_unicode: unicode,
    };
    let iterator = rt.heap.new_object_with_kind(
        Some(proto),
        ObjectKind::RegExpStringIterator(Box::new(state)),
    )?;
    Ok(NativeReturn::Value(iterator.into()))
}

/// `%RegExpStringIteratorPrototype%.next()` (§22.2.9.2.1): the steps of
/// the closure of `CreateRegExpStringIterator` (§22.2.9.1) for one
/// match.
fn iterator_next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let iterator = match call.this() {
        Value::Object(object)
            if matches!(
                rt.heap.object(object)?.kind,
                ObjectKind::RegExpStringIterator(_)
            ) =>
        {
            object
        }
        _ => {
            let name = "%RegExpStringIterator%.prototype.next";
            return Err(rt.incompatible_receiver(name, call.this()));
        }
    };
    let result = match iterator_next_value(rt, iterator)? {
        Some(value) => rt.iter_result(value, false)?,
        None => rt.iter_result(Value::Undefined, true)?,
    };
    Ok(NativeReturn::Value(result.into()))
}

/// The next match of the `RegExp` String Iterator `iterator`, or `None`
/// when it is done: `next` without its result object (the compiled
/// iteration steps call it for the built-in `next`, `vm/iter.rs`). The
/// value is recorded in the current handle scope.
pub(super) fn iterator_next_value(
    rt: &mut Runtime,
    iterator: Gc<Object>,
) -> VmResult<Option<Value>> {
    let ObjectKind::RegExpStringIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(VmError::invariant("not a RegExp String Iterator"));
    };
    let (matcher, string, global, unicode) = (
        state.matcher,
        state.string,
        state.global,
        state.full_unicode,
    );
    let Some(regexp) = matcher else {
        return Ok(None);
    };
    let finish = |rt: &mut Runtime| -> VmResult<()> {
        if let ObjectKind::RegExpStringIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
            state.matcher = None;
        }
        Ok(())
    };
    // The closure runs in a generator: a throw completes it, so later
    // calls return `done`.
    let step = iterator_step(rt, regexp, string, global, unicode);
    if !matches!(step, Ok(Some(_))) {
        finish(rt)?;
    }
    Ok(match step? {
        Some((value, last)) => {
            if last {
                finish(rt)?;
            }
            Some(rt.heap.record(value))
        }
        None => None,
    })
}

/// One step of the closure of `CreateRegExpStringIterator`: the match
/// and whether it is the last (without `g`), or `None` at the end.
fn iterator_step(
    rt: &mut Runtime,
    regexp: Gc<Object>,
    string: Gc<JsString>,
    global: bool,
    unicode: bool,
) -> VmResult<Option<(Value, bool)>> {
    let exec = rt.regexp_exec(regexp, string)?;
    let value = match &exec {
        Exec::Null => return Ok(None),
        Exec::Match(captures, regex) => rt.match_array(string, captures, regex)?.into(),
        Exec::Object(object) => (*object).into(),
    };
    if !global {
        return Ok(Some((value, true)));
    }
    let matched = rt.result_match_string(string, &exec)?;
    if rt.string_len(matched)? == 0 {
        rt.advance_last_index(regexp, string, unicode)?;
    }
    Ok(Some((value, false)))
}

// --- @@replace ---

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
fn symbol_replace(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
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
fn pristine_global_loop(
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

// --- @@split ---

/// `RegExp.prototype[@@split](string, limit)` (§22.2.6.14).
fn symbol_split(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.@@split")?;
    let string = rt.flat_string_arg(call)?;
    let limit_value = rt.arg(call, 1);
    // The spec converts `limit` (step 13) after it reads `flags` and
    // makes the splitter, and calls the splitter's `exec` after it. An
    // object limit can run a `valueOf` that changes `exec`, so it takes
    // the spec path; a primitive limit runs no script code.
    if !matches!(limit_value, Value::Object(_)) && rt.regexp_is_pristine(regexp, true)? {
        let regex = rt.regexp_program(regexp)?;
        if !regex.flags().contains(Flags::STICKY) {
            let limit = split_limit(rt, limit_value)?;
            let array = rt.new_array()?;
            if limit > 0 {
                split_search(rt, array, string, &regex, limit)?;
            }
            return Ok(NativeReturn::Value(array.into()));
        }
    }
    let constructor = rt.regexp_species_constructor(regexp)?;
    let (flags, _, unicode) = rt.flags_of(regexp)?;
    let new_flags = if has_letter(rt, flags, b'y')? {
        flags
    } else {
        let y = rt.heap.intern_str("y")?;
        let joined = rt.concat(flags, y)?;
        rt.heap.record(joined)
    };
    let splitter = rt.construct_regexp(constructor, regexp, new_flags)?;
    let array = rt.new_array()?;
    let limit = split_limit(rt, limit_value)?;
    if limit == 0 {
        return Ok(NativeReturn::Value(array.into()));
    }
    let size = rt.string_len(string)?;
    if size == 0 {
        if matches!(rt.regexp_exec(splitter, string)?, Exec::Null) {
            rt.push_value(array, string.into())?;
        }
        return Ok(NativeReturn::Value(array.into()));
    }
    split_sticky(rt, array, string, splitter, (limit, unicode))?;
    Ok(NativeReturn::Value(array.into()))
}

/// Step 11: `lim` (`2^32 - 1` for `undefined`).
fn split_limit(rt: &mut Runtime, limit: Value) -> VmResult<u32> {
    Ok(match limit {
        Value::Undefined => u32::MAX,
        value => to_uint32(rt.to_number(value)?),
    })
}

/// Steps 15 to 21 with the splitter: a sticky `RegExpExec` at each
/// position.
fn split_sticky(
    rt: &mut Runtime,
    array: Gc<Object>,
    string: Gc<JsString>,
    splitter: Gc<Object>,
    (limit, unicode): (u32, bool),
) -> VmResult<()> {
    let size = rt.string_len(string)?;
    let mut count = 0u32;
    let mut piece_start = 0usize;
    let mut position = 0usize;
    while position < size {
        rt.tick()?;
        let step = rt.with_scope(|rt| {
            rt.set_last_index(splitter, position as f64)?;
            let exec = rt.regexp_exec(splitter, string)?;
            if matches!(exec, Exec::Null) {
                return Ok(None);
            }
            let end_index = (rt.last_index(splitter)? as usize).min(size);
            if end_index == piece_start {
                return Ok(None);
            }
            let piece = rt.substring(string, piece_start, position)?;
            rt.push_value(array, piece.into())?;
            count += 1;
            if count == limit {
                return Ok(Some(true));
            }
            piece_start = end_index;
            let captures = match &exec {
                Exec::Match(captures, _) => captures.len().saturating_sub(1) as f64,
                Exec::Object(object) => (rt.length_of_array_like(*object)? - 1.0).max(0.0),
                Exec::Null => 0.0,
            };
            let mut i = 1.0;
            while i <= captures {
                rt.tick()?;
                let capture = match &exec {
                    Exec::Match(spans, _) => match spans.get(i as usize) {
                        Some((s, e)) => Value::String(rt.substring(string, s, e)?),
                        None => Value::Undefined,
                    },
                    Exec::Object(object) => {
                        let key = rt.index_key(i)?;
                        rt.get_value((*object).into(), key)?
                    }
                    Exec::Null => Value::Undefined,
                };
                rt.push_value(array, capture)?;
                count += 1;
                if count == limit {
                    return Ok(Some(true));
                }
                i += 1.0;
            }
            position = piece_start;
            Ok(Some(false))
        })?;
        match step {
            Some(true) => return Ok(()),
            Some(false) => {}
            None => {
                let text = rt.heap.text(string)?;
                position = advance_string_index(text, position as f64, unicode) as usize;
            }
        }
    }
    let piece = rt.substring(string, piece_start, size)?;
    rt.push_value(array, piece.into())
}

/// Steps 14 to 21 for a pristine `RegExp` without `y`: each next match is
/// searched from `q` with the object's program (the splitter would try
/// each position in turn and find the same match). The legacy static
/// properties get the last match that the splitter would have found.
fn split_search(
    rt: &mut Runtime,
    array: Gc<Object>,
    string: Gc<JsString>,
    regex: &Regex,
    limit: u32,
) -> VmResult<()> {
    let size = rt.string_len(string)?;
    let unicode =
        regex.flags().contains(Flags::UNICODE) || regex.flags().contains(Flags::UNICODE_SETS);
    if size == 0 {
        let found = rt.regexp_run(regex, string, 0)?;
        match found {
            Some(captures) => rt.note_last_match(string, &captures),
            None => rt.push_value(array, string.into())?,
        }
        return Ok(());
    }
    let mut count = 0u32;
    let mut piece_start = 0usize;
    let mut position = 0usize;
    let mut last: Option<Captures> = None;
    while position < size {
        rt.tick()?;
        let Some(captures) = rt.regexp_run(regex, string, position)? else {
            break;
        };
        let Some((at, end)) = captures.get(0) else {
            break;
        };
        if at >= size {
            break;
        }
        let end_index = end.min(size);
        if end_index == piece_start {
            last = Some(captures);
            position = advance_string_index(rt.heap.text(string)?, at as f64, unicode) as usize;
            continue;
        }
        let done = rt.with_scope(|rt| {
            let piece = rt.substring(string, piece_start, at)?;
            rt.push_value(array, piece.into())?;
            count += 1;
            if count == limit {
                return Ok(true);
            }
            for i in 1..captures.len() {
                let capture = match captures.get(i) {
                    Some((s, e)) => Value::String(rt.substring(string, s, e)?),
                    None => Value::Undefined,
                };
                rt.push_value(array, capture)?;
                count += 1;
                if count == limit {
                    return Ok(true);
                }
            }
            Ok(false)
        })?;
        piece_start = end_index;
        position = piece_start;
        last = Some(captures);
        if done {
            if let Some(last) = &last {
                rt.note_last_match(string, last);
            }
            return Ok(());
        }
    }
    if let Some(last) = &last {
        rt.note_last_match(string, last);
    }
    let piece = rt.substring(string, piece_start, size)?;
    rt.push_value(array, piece.into())
}
