//! The symbol methods of `RegExp.prototype` (ECMA-262 §22.2.6.8 to
//! §22.2.6.14): `@@match`, `@@matchAll` with `%RegExpStringIteratorPrototype%`
//! (§22.2.9), `@@search` and `@@split`; `@@replace` is in `regexp_replace.rs`.
//!
//! Each follows the specification step by step through `RegExpExec`. When
//! `exec` is the built-in one, a result is the captures of the match
//! ([`Exec::Match`]): the reads of a result array that no script can see
//! (`0`, `length`, `index`, the captures, `groups`) come from them.
//!
//! `@@split` of a pristine `RegExp` (`regexp_fast.rs`) without the `y`
//! flag searches each next match from the current position with the
//! object's own program, which gives the matches of the sticky splitter
//! at each position without making the splitter. All other cases make
//! the splitter with the species constructor.

use swb_js_regexp::{Captures, Flags, Regex};

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, RegExpStringIterator};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::to_uint32;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

use super::regexp_exec::{Exec, advance_string_index};
use super::regexp_replace::pristine_global_loop;

pub(super) fn install(rt: &mut Runtime, realm: u32, proto: Gc<Object>) -> VmResult<()> {
    let methods: [(WellKnown, u32, crate::vm::NativeFn); 5] = [
        (WellKnown::Match, 1, symbol_match),
        (WellKnown::MatchAll, 1, symbol_match_all),
        (WellKnown::Replace, 2, super::regexp_replace::symbol_replace),
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
pub(super) fn has_letter(rt: &Runtime, flags: Gc<JsString>, letter: u8) -> VmResult<bool> {
    Ok(rt
        .heap
        .text(flags)?
        .units()
        .any(|unit| unit == u16::from(letter)))
}

impl Runtime {
    /// `ToString(argument 0)`, rooted and flat.
    pub(super) fn flat_string_arg(&mut self, call: &NativeCall) -> VmResult<Gc<JsString>> {
        let string = self.string_arg(call, 0)?;
        self.heap.record(string);
        self.flatten(string)?;
        Ok(string)
    }

    /// Whether `ToString(Get(R, "flags"))` contains `g`, and `u` or `v`.
    pub(super) fn flag_info(&mut self, regexp: Gc<Object>) -> VmResult<(bool, bool)> {
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
    pub(super) fn advance_last_index(
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
    pub(super) fn result_match_string(
        &mut self,
        string: Gc<JsString>,
        exec: &Exec,
    ) -> VmResult<Gc<JsString>> {
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
        let realm = self.current_realm();
        let default = self.intrinsic(realm, Intrinsic::RegExpConstructor)?;
        let getter = self.intrinsic(realm, Intrinsic::RegExpSpeciesGetter)?;
        self.species_constructor(regexp, default, getter)
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
