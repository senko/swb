//! The match of a `RegExp` (ECMA-262 §22.2.7): `RegExpBuiltinExec`,
//! `RegExpExec`, the result array with `index`, `input`, `groups` and the
//! `d` indices (`MakeMatchIndicesIndexPairArray`), `AdvanceStringIndex`,
//! and the driver of the matcher of `js-regexp`.
//!
//! The driver runs [`Matcher::run`] in slices of [`SLICE_STEPS`] steps
//! and runs the time check, the termination request and a safepoint
//! between two slices (ADR 0026 section 9). The input is a flat string
//! that the caller roots; positions are code-unit indices, so a
//! collection between slices changes nothing. The backtrack stack and the
//! undo log may use the bytes that the heap limit still allows, at most
//! [`BACKTRACK_LIMIT`]; beyond that the match fails with V8's `RangeError`
//! ("Maximum call stack size exceeded", measured in Chromium 148 at about
//! ten million iterations of `(?:a|b)*`). When the heap limit is
//! the smaller bound, the driver collects once and starts the match again
//! before it fails.
//!
//! `RegExpExec` of an object whose `exec` is the built-in one runs the
//! match without a call and returns the captures, not an array: the
//! symbol methods read the parts of a result array that no script can
//! see, so they take them from the captures ([`Exec`]).

use std::rc::Rc;

use swb_js_regexp::{Captures, Error as RegexError, Flags, Matcher, Outcome, Regex};

use crate::heap::Gc;
use crate::object::{LastMatch, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::to_length;
use crate::vm::{Intrinsic, STACK_OVERFLOW, VmError, VmResult};

/// The steps of the matcher between two time checks.
const SLICE_STEPS: u32 = 1 << 15;

/// The matcher steps that cost one step of the time countdown (a matcher
/// step takes a few nanoseconds; a countdown step stands for about 12).
const STEPS_PER_TICK: u64 = 4;

/// The most bytes of the backtrack stack and the undo log of one match.
/// V8 fails at about ten million iterations of `(?:a|b)*`. A choice point
/// of `js-regexp` takes 32 bytes and that loop takes two per iteration, so
/// this bound fails at about four million iterations (ADR 0026 section
/// 15).
pub(super) const BACKTRACK_LIMIT: usize = 256 << 20;

/// The most bytes of matcher vectors that the VM keeps between matches.
const KEPT_MATCHER_BYTES: usize = 64 << 10;

/// The result of `RegExpExec`.
pub(super) enum Exec {
    /// No match (`null`).
    Null,
    /// A match of the built-in `exec`, as captures (no array was made).
    Match(Captures, Rc<Regex>),
    /// The result of a script's `exec`: an object (rooted).
    Object(Gc<Object>),
}

impl Runtime {
    /// Runs `regex` on the flat, rooted string `input` from `start`: the
    /// time check between slices, the memory bound, a collection and a
    /// second try when the heap limit was the bound. The matcher's vectors
    /// are kept in the VM for the next match (no match runs during
    /// another: no script code runs between the slices).
    pub(super) fn regexp_run(
        &mut self,
        regex: &Regex,
        input: Gc<JsString>,
        start: usize,
    ) -> VmResult<Option<Captures>> {
        let mut matcher = match self.vm.regexp_matcher.take() {
            Some(mut matcher) => {
                matcher.reset(regex, start);
                matcher
            }
            None => Matcher::new(regex, start),
        };
        let result = self.drive_matcher(regex, &mut matcher, input, start);
        // Keep small vectors only: a big backtrack stack goes now.
        if matcher.heap_bytes() <= KEPT_MATCHER_BYTES {
            self.vm.regexp_matcher = Some(matcher);
        }
        result
    }

    fn drive_matcher(
        &mut self,
        regex: &Regex,
        matcher: &mut Matcher,
        input: Gc<JsString>,
        start: usize,
    ) -> VmResult<Option<Captures>> {
        let mut collected = false;
        let mut charged = 0u64;
        loop {
            let room = self
                .heap
                .config()
                .limit
                .saturating_sub(self.heap.heap_size());
            let limit = room.min(BACKTRACK_LIMIT);
            let outcome = matcher.run(regex, self.heap.text(input)?, SLICE_STEPS, limit);
            let ticks = matcher.steps() / STEPS_PER_TICK;
            self.charge(usize::try_from(ticks - charged).unwrap_or(usize::MAX))?;
            charged = ticks;
            match outcome {
                Ok(Outcome::Match(captures)) => return Ok(Some(captures)),
                Ok(Outcome::NoMatch) => return Ok(None),
                Ok(Outcome::Pending) => {
                    self.heap.safepoint(&self.vm)?;
                }
                Err(RegexError::MemoryLimit) if limit < BACKTRACK_LIMIT && !collected => {
                    collected = true;
                    self.collect();
                    matcher.reset(regex, start);
                    charged = 0;
                }
                Err(RegexError::MemoryLimit) => return Err(VmError::range_error(STACK_OVERFLOW)),
                Err(_) => return Err(VmError::invariant("a resumed match lost its input")),
            }
        }
    }

    /// `ToLength(Get(R, "lastIndex"))`.
    pub(super) fn last_index(&mut self, regexp: Gc<Object>) -> VmResult<f64> {
        let key = PropertyKey::String(self.vm.atoms.last_index);
        let value = match self.own_last_index(regexp)? {
            Some(value) => value,
            None => self.get_value(regexp.into(), key)?,
        };
        let number = match value {
            Value::Int(i) => f64::from(i),
            Value::Double(d) => d,
            other => self.to_number(other)?,
        };
        Ok(to_length(number))
    }

    /// `Set(R, "lastIndex", value, true)`.
    pub(super) fn set_last_index(&mut self, regexp: Gc<Object>, value: f64) -> VmResult<()> {
        let value = Value::number(value);
        if self.write_own_last_index(regexp, value)? {
            return Ok(());
        }
        let key = PropertyKey::String(self.vm.atoms.last_index);
        self.set_or_throw(regexp, key, value)
    }

    /// Steps 1 to 16 of `RegExpBuiltinExec` (§22.2.7.2): the read of
    /// `lastIndex` (before `[[RegExpMatcher]]` and the flags are read, so
    /// a `valueOf` that calls `compile` changes them), the match and the
    /// write of `lastIndex`. Updates the legacy static properties after a
    /// match. `string` is rooted.
    pub(super) fn regexp_builtin_match(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
    ) -> VmResult<Option<(Captures, Rc<Regex>)>> {
        let length = self.string_len(string)?;
        let last_index = self.last_index(regexp)?;
        let regex = self.regexp_program(regexp)?;
        let flags = regex.flags();
        let global_or_sticky = flags.contains(Flags::GLOBAL) || flags.contains(Flags::STICKY);
        let start = if global_or_sticky { last_index } else { 0.0 };
        if start > length as f64 {
            if global_or_sticky {
                self.set_last_index(regexp, 0.0)?;
            }
            return Ok(None);
        }
        self.flatten(string)?;
        let found = self.regexp_run(&regex, string, start as usize)?;
        let Some(captures) = found else {
            if global_or_sticky {
                self.set_last_index(regexp, 0.0)?;
            }
            return Ok(None);
        };
        if global_or_sticky {
            let end = captures.get(0).map_or(0, |(_, end)| end);
            self.set_last_index(regexp, end as f64)?;
        }
        self.note_last_match(string, &captures);
        Ok(Some((captures, regex)))
    }

    /// `RegExpBuiltinExec(R, S)` (§22.2.7.2): `null` or the result array.
    pub(super) fn regexp_builtin_exec(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
    ) -> VmResult<Value> {
        match self.regexp_builtin_match(regexp, string)? {
            None => Ok(Value::Null),
            Some((captures, regex)) => Ok(self.match_array(string, &captures, &regex)?.into()),
        }
    }

    /// `RegExpExec(R, S)` (§22.2.7.1). The built-in `exec` runs without a
    /// call and gives the captures.
    pub(super) fn regexp_exec(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
    ) -> VmResult<Exec> {
        if !self.has_builtin_exec(regexp)? {
            let key = PropertyKey::String(self.vm.atoms.exec);
            let exec = self.get_value(regexp.into(), key)?;
            let builtin = self.intrinsic(self.current_realm(), Intrinsic::RegExpExec)?;
            if exec != Value::Object(builtin) && self.is_callable(exec)? {
                return self.call_exec(regexp, exec, string);
            }
            if !matches!(self.heap.object(regexp)?.kind, ObjectKind::RegExp(_)) {
                return Err(self.incompatible_receiver("RegExp.prototype.exec", regexp.into()));
            }
        }
        Ok(match self.regexp_builtin_match(regexp, string)? {
            None => Exec::Null,
            Some((captures, regex)) => Exec::Match(captures, regex),
        })
    }

    /// Steps 2.a to 2.c of `RegExpExec`: a call of a script's `exec`.
    fn call_exec(
        &mut self,
        regexp: Gc<Object>,
        exec: Value,
        string: Gc<JsString>,
    ) -> VmResult<Exec> {
        let result = self.call(exec, regexp.into(), &[string.into()])?;
        match result {
            Value::Null => Ok(Exec::Null),
            Value::Object(object) => Ok(Exec::Object(self.heap.record(object))),
            _ => Err(VmError::type_error(
                "RegExp exec method returned something other than an Object or null",
            )),
        }
    }

    /// `RegExpExec(R, S)` as a value: `null` or the result object (an
    /// array for the built-in `exec`).
    pub(super) fn regexp_exec_value(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
    ) -> VmResult<Value> {
        Ok(match self.regexp_exec(regexp, string)? {
            Exec::Null => Value::Null,
            Exec::Match(captures, regex) => self.match_array(string, &captures, &regex)?.into(),
            Exec::Object(object) => object.into(),
        })
    }

    /// Whether `RegExpExec(R, S)` is not `null` (`test`).
    pub(super) fn regexp_exec_matches(
        &mut self,
        regexp: Gc<Object>,
        string: Gc<JsString>,
    ) -> VmResult<bool> {
        Ok(!matches!(self.regexp_exec(regexp, string)?, Exec::Null))
    }

    /// Steps 17 to 35 of `RegExpBuiltinExec`: the result array of a match
    /// (`0` to `n`, `index`, `input`, `groups`, and `indices` with the `d`
    /// flag). Recorded in the handle scope.
    pub(super) fn match_array(
        &mut self,
        string: Gc<JsString>,
        captures: &Captures,
        regex: &Regex,
    ) -> VmResult<Gc<Object>> {
        let mut values = Vec::with_capacity(captures.len());
        for span in captures.spans() {
            values.push(match *span {
                Some((start, end)) => {
                    let piece = self.substring(string, start, end)?;
                    Value::String(self.heap.record(piece))
                }
                None => Value::Undefined,
            });
        }
        let index = Value::number(captures.get(0).map_or(0, |(start, _)| start) as f64);
        let groups = self.groups_object(regex, &values)?;
        let mut slots = vec![index, string.into(), groups];
        let which = if regex.flags().contains(Flags::HAS_INDICES) {
            let indices = self.indices_array(captures, regex)?;
            slots.push(indices.into());
            1
        } else {
            0
        };
        self.shaped_array(which, values, slots)
    }

    /// An array of `elements` with the named properties `slots` in the
    /// order of the result shape `which` (0: a match array, 1: one with
    /// `indices`, 2: an `indices` array). The first array of each kind is
    /// built with the general definitions, and its shape is kept for the
    /// next ones. The values are rooted; the result is recorded.
    fn shaped_array(
        &mut self,
        which: usize,
        elements: Vec<Value>,
        slots: Vec<Value>,
    ) -> VmResult<Gc<Object>> {
        let realm = self.current_realm() as usize;
        let shape = self
            .vm
            .realms
            .get(realm)
            .and_then(|r| r.regexp.result_shapes.get(which).copied().flatten());
        if let Some(shape) = shape {
            let length = u32::try_from(elements.len())
                .map_err(|_| VmError::invariant("too many captures"))?;
            self.heap.reserve(
                (elements.len() + slots.len()) * size_of::<Value>(),
                &self.vm,
            )?;
            let kind = ObjectKind::Array {
                length,
                length_writable: true,
            };
            let array = self
                .heap
                .new_object_with_shape(shape, slots, elements, kind)?;
            return Ok(self.heap.record(array));
        }
        let array = self.array_from_values(&elements)?;
        let names: &[Gc<JsString>] = match which {
            2 => &[self.vm.atoms.groups],
            _ => &[
                self.vm.atoms.index,
                self.vm.atoms.input,
                self.vm.atoms.groups,
            ],
        };
        let names = names.to_vec();
        for (name, value) in names.into_iter().zip(slots.iter().copied()) {
            self.define(array, PropertyKey::String(name), value, true, true, true)?;
        }
        if which == 1
            && let Some(&indices) = slots.get(3)
        {
            let key = self.heap.key_from_str("indices")?;
            self.define(array, key, indices, true, true, true)?;
        }
        let shape = self.heap.object(array)?.shape;
        if let Some(record) = self.vm.realms.get_mut(realm)
            && let Some(slot) = record.regexp.result_shapes.get_mut(which)
        {
            *slot = Some(shape);
        }
        Ok(array)
    }

    /// The `groups` object of a match (steps 30 to 33.d): `undefined`
    /// without named groups, else an object without prototype with one
    /// property per name in the order of first appearance; for a name of
    /// several groups (in different alternatives), the value of the one
    /// that matched. `values` are the captured values (index 0: the
    /// match).
    pub(super) fn groups_object(&mut self, regex: &Regex, values: &[Value]) -> VmResult<Value> {
        if !regex.has_named_groups() {
            return Ok(Value::Undefined);
        }
        let groups = self.heap.new_object(None)?;
        self.heap.record(groups);
        for (i, name) in regex.group_names().iter().enumerate() {
            let Some(name) = name else { continue };
            let value = values.get(i + 1).copied().unwrap_or_default();
            let key = self.heap.key_from_str16(units_text(name))?;
            self.heap.record(key);
            // A later group of the same name overwrites only an earlier
            // one that did not match (at most one of them matched).
            let existing = self.heap.get_own_property(groups, key)?;
            if existing.is_some() && value.is_undefined() {
                continue;
            }
            self.heap
                .create_fresh_data_property(groups, key, value, &self.vm)?;
        }
        Ok(groups.into())
    }

    /// `MakeMatchIndicesIndexPairArray` (§22.2.7.8): `[start, end]` per
    /// group (or `undefined`) and its own `groups`.
    fn indices_array(&mut self, captures: &Captures, regex: &Regex) -> VmResult<Gc<Object>> {
        let mut pairs = Vec::with_capacity(captures.len());
        for span in captures.spans() {
            pairs.push(match *span {
                Some((start, end)) => self
                    .array_from_values(&[Value::number(start as f64), Value::number(end as f64)])?
                    .into(),
                None => Value::Undefined,
            });
        }
        let groups = self.groups_object(regex, &pairs)?;
        self.shaped_array(2, pairs, vec![groups])
    }

    /// Keeps a match for the legacy static properties of the current realm
    /// (V8 updates them after every successful match, also of subclass
    /// instances; measured).
    pub(super) fn note_last_match(&mut self, subject: Gc<JsString>, captures: &Captures) {
        let span = |i: usize| captures.get(i).map(|(s, e)| (s as u32, e as u32));
        let mut groups = [None; 9];
        for (i, group) in groups.iter_mut().enumerate() {
            *group = span(i + 1);
        }
        let count = captures.len().saturating_sub(1);
        let last = if count == 0 { None } else { span(count) };
        let realm = self.current_realm();
        if let Some(realm) = self.vm.realms.get_mut(realm as usize) {
            realm.regexp.last_match = Some(LastMatch {
                subject,
                whole: span(0).unwrap_or((0, 0)),
                groups,
                last,
            });
            realm.regexp.input = Some(subject);
        }
    }
}

/// A group name (UTF-16 units) as text.
pub(super) fn units_text(units: &[u16]) -> swb_js_text::Str16<'_> {
    swb_js_text::Str16::Wide(units)
}

/// `AdvanceStringIndex(S, index, unicode)` (§22.2.7.3) on a flat text.
pub(super) fn advance_string_index(text: swb_js_text::Str16<'_>, index: f64, unicode: bool) -> f64 {
    if !unicode || index + 1.0 >= text.len() as f64 {
        return index + 1.0;
    }
    match text.code_point_at(index as usize) {
        Some((_, next)) => next as f64,
        None => index + 1.0,
    }
}
