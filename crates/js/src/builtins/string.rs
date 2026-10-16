//! `String` statics and the prototype methods that need no regular
//! expression (ECMA-262 §22.1.2, §22.1.3): `fromCharCode`,
//! `fromCodePoint`, `raw`, and `at`, `charAt`, `charCodeAt`,
//! `codePointAt`, `concat`, `isWellFormed`, `toWellFormed`, `padStart`,
//! `padEnd`, `repeat`, `slice`, `substring`, `trim`, `trimStart` and
//! `trimEnd`. The searches are in [`super::string_search`], `replace`,
//! `replaceAll` and `split` in [`super::string_replace`], the iterator in
//! [`super::string_iter`], Annex B in [`super::string_annexb`]. Case
//! mapping, `normalize` and `localeCompare` are in
//! [`super::string_unicode`].
//!
//! Every method starts with `RequireObjectCoercible(this)` and
//! `ToString(this)` ([`Runtime::this_string`]), then converts its
//! arguments in the order of the specification. The code units of a rope
//! are read after a flatten, a safepoint: the receiver and every string
//! that is still needed are rooted (arguments and `this` are on the value
//! stack, converted strings are recorded in the handle scope). Results
//! are flat copies (`slice`, `substring`) or built from `concat`
//! (`repeat`, `padStart`, `padEnd`: a doubling that makes a rope of a few
//! nodes, so `'ab'.repeat(2 ** 28)` costs no memory until its code units
//! are read). Builders that copy ([`Builder`]) charge the time in
//! proportion to the copy, stop at V8's maximum length with its
//! `RangeError`, and check the heap limit at each doubling.

use swb_js_text::unicode::{is_line_terminator, is_whitespace};
use swb_js_text::{Str16, String16, combine_surrogates, is_lead_surrogate, is_trail_surrogate};

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::convert::{MAX_STRING_LENGTH, to_integer_or_infinity, to_length, to_uint32};
use crate::vm::number::number_to_string;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

/// The message of the `RangeError` for a string above the maximum length.
pub(super) const INVALID_STRING_LENGTH: &str = "Invalid string length";

/// A text above this many code units reserves its memory in the heap's
/// accounting while it grows (the buffer is Rust memory until the string
/// exists).
const RESERVE_FROM: usize = 1 << 16;

/// Installs the properties of `String` and `String.prototype` in the
/// order of V8's `Reflect.ownKeys` (the methods of other features are
/// installed after these; see the backlog).
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    ctor: Gc<Object>,
    proto: Gc<Object>,
    value_of: NativeFn,
) -> VmResult<()> {
    super::method(rt, realm, ctor, "fromCharCode", 1, from_char_code)?;
    super::method(rt, realm, ctor, "fromCodePoint", 1, from_code_point)?;
    super::method(rt, realm, ctor, "raw", 1, raw)?;
    let methods: [(&str, u32, NativeFn); 44] = [
        ("anchor", 1, super::string_annexb::anchor),
        ("at", 1, at),
        ("big", 0, super::string_annexb::big),
        ("blink", 0, super::string_annexb::blink),
        ("bold", 0, super::string_annexb::bold),
        ("charAt", 1, char_at),
        ("charCodeAt", 1, char_code_at),
        ("codePointAt", 1, code_point_at),
        ("concat", 1, concat),
        ("endsWith", 1, super::string_search::ends_with),
        ("fontcolor", 1, super::string_annexb::fontcolor),
        ("fontsize", 1, super::string_annexb::fontsize),
        ("fixed", 0, super::string_annexb::fixed),
        ("includes", 1, super::string_search::includes),
        ("indexOf", 1, super::string_search::index_of),
        ("isWellFormed", 0, is_well_formed),
        ("italics", 0, super::string_annexb::italics),
        ("lastIndexOf", 1, super::string_search::last_index_of),
        ("link", 1, super::string_annexb::link),
        ("localeCompare", 1, super::string_unicode::locale_compare),
        ("normalize", 0, super::string_unicode::normalize),
        ("padEnd", 1, pad_end),
        ("padStart", 1, pad_start),
        ("repeat", 1, repeat),
        ("replace", 2, super::string_replace::replace),
        ("replaceAll", 2, super::string_replace::replace_all),
        ("slice", 2, slice),
        ("small", 0, super::string_annexb::small),
        ("split", 2, super::string_replace::split),
        ("strike", 0, super::string_annexb::strike),
        ("sub", 0, super::string_annexb::sub),
        ("substr", 2, super::string_annexb::substr),
        ("substring", 2, substring),
        ("sup", 0, super::string_annexb::sup),
        ("startsWith", 1, super::string_search::starts_with),
        ("toString", 0, value_of),
        ("toWellFormed", 0, to_well_formed),
        ("trim", 0, trim),
        ("trimStart", 0, trim_start),
        ("trimEnd", 0, trim_end),
        (
            "toLocaleLowerCase",
            0,
            super::string_unicode::to_locale_lower_case,
        ),
        (
            "toLocaleUpperCase",
            0,
            super::string_unicode::to_locale_upper_case,
        ),
        ("toLowerCase", 0, super::string_unicode::to_lower_case),
        ("toUpperCase", 0, super::string_unicode::to_upper_case),
    ];
    for (name, length, func) in methods {
        let function = super::method(rt, realm, proto, name, length, func)?;
        // Annex B.2.2.15 and B.2.2.16: `trimLeft` and `trimRight` are the
        // same function objects (V8 defines them right after).
        let alias = match name {
            "trimStart" => Some("trimLeft"),
            "trimEnd" => Some("trimRight"),
            _ => None,
        };
        if let Some(alias) = alias {
            let key = rt.heap.key_from_str(alias)?;
            rt.define(proto, key, function.into(), true, false, true)?;
        }
    }
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    super::symbol_method(
        rt,
        realm,
        proto,
        WellKnown::Iterator,
        0,
        super::string_iter::string_iterator,
        (true, true),
    )?;
    super::string_iter::install(rt, realm)
}

// --- Helpers shared by the string modules ---

impl Runtime {
    /// `RequireObjectCoercible(this)` and `ToString(this)` for the
    /// method `method` (with its prefix: `String.prototype.trim`), which
    /// V8 words "String.prototype.trim called on null or undefined". The
    /// string is rooted (`this` is on the value stack, a conversion is
    /// recorded in the handle scope).
    pub(super) fn this_string(
        &mut self,
        call: &NativeCall,
        method: &str,
    ) -> VmResult<Gc<JsString>> {
        match call.this() {
            Value::Undefined | Value::Null => Err(VmError::type_error(format!(
                "{method} called on null or undefined"
            ))),
            Value::String(string) => Ok(string),
            this => self.to_string(this),
        }
    }

    /// `ToIntegerOrInfinity(ToNumber(argument))` of argument `index`.
    pub(super) fn integer_arg(&mut self, call: &NativeCall, index: usize) -> VmResult<f64> {
        let value = self.arg(call, index);
        Ok(to_integer_or_infinity(self.to_number(value)?))
    }

    /// `ToString` of argument `index`, rooted.
    pub(super) fn string_arg(&mut self, call: &NativeCall, index: usize) -> VmResult<Gc<JsString>> {
        let value = self.arg(call, index);
        self.to_string(value)
    }

    /// The number of code units of a string (no flatten).
    pub(super) fn string_len(&self, string: Gc<JsString>) -> VmResult<usize> {
        Ok(self.heap.string(string)?.len())
    }

    /// A new flat string from built text, recorded in the handle scope.
    /// The caller reserved the memory of a long text before it built it.
    pub(super) fn new_string(&mut self, text: String16) -> VmResult<Gc<JsString>> {
        let string = self.heap.alloc_string(text)?;
        Ok(self.heap.record(string))
    }

    /// A string of one code unit.
    pub(super) fn unit_string(&mut self, unit: u16) -> VmResult<Gc<JsString>> {
        let mut text = String16::new();
        text.push(unit);
        self.new_string(text)
    }

    /// The code units `start..end` of `string` as a string. The whole
    /// string and the empty range need no copy. The copy is flat; the
    /// source is flattened first (a safepoint: `string` is rooted by the
    /// caller). The copy is charged.
    pub(super) fn substring(
        &mut self,
        string: Gc<JsString>,
        start: usize,
        end: usize,
    ) -> VmResult<Gc<JsString>> {
        let len = self.string_len(string)?;
        if start >= end || start >= len {
            return Ok(self.vm.atoms.empty);
        }
        let end = end.min(len);
        if start == 0 && end == len {
            return Ok(string);
        }
        self.flatten(string)?;
        let count = end - start;
        if count >= RESERVE_FROM {
            self.heap.reserve(count * 2, &self.vm)?;
        }
        let text = self.heap.text(string)?;
        let piece = text
            .slice(start, end)
            .ok_or(VmError::invariant("a substring range outside the string"))?
            .to_string16();
        let piece = self.heap.alloc_string(piece)?;
        self.charge_units(count)?;
        Ok(piece)
    }

    /// The code unit at `index` of a string, flattening it first.
    pub(super) fn unit_at(&mut self, string: Gc<JsString>, index: usize) -> VmResult<Option<u16>> {
        Ok(self.flat_text(string)?.get(index))
    }
}

/// A relative index clamped to `0..=length` (the start of `slice`, steps
/// 4 to 7 of §22.1.3.24): negative values count from the end.
pub(super) fn clamp_relative(relative: f64, length: usize) -> usize {
    if relative < 0.0 {
        (length as f64 + relative).max(0.0) as usize
    } else {
        relative.min(length as f64) as usize
    }
}

/// An index clamped to `0..=length`.
pub(super) fn clamp_index(index: f64, length: usize) -> usize {
    index.clamp(0.0, length as f64) as usize
}

/// Builds a string by copying: strings and ranges of strings, units and
/// code points. Every append checks V8's maximum length (a `RangeError`
/// before the copy), charges the time in proportion to the copy and
/// reserves the heap memory of a long text at each doubling. A caller
/// that appends many small pieces calls [`Builder::check`] itself.
pub(super) struct Builder {
    text: String16,
    /// The units that the last reservation covers.
    reserved: usize,
    /// Whether the last reservation was for two bytes per unit.
    reserved_wide: bool,
}

impl Builder {
    /// An empty builder.
    pub(super) fn new() -> Self {
        Builder {
            text: String16::new(),
            reserved: RESERVE_FROM,
            reserved_wide: false,
        }
    }

    /// Makes room for `extra` more code units: the maximum length, then
    /// the heap limit for the text after its next doubling, in bytes of
    /// the text's width (a safepoint when a doubling passes the reserved
    /// size or the text became wide; the caller roots its handles). The
    /// reservation stops at the maximum length, so that a text too long
    /// for a string is V8's `RangeError` and not the heap limit.
    pub(super) fn check(&mut self, rt: &mut Runtime, extra: usize) -> VmResult<()> {
        let total = self.text.len().saturating_add(extra);
        if total > MAX_STRING_LENGTH {
            return Err(VmError::range_error(INVALID_STRING_LENGTH));
        }
        let wide = !self.text.is_latin1();
        let widened = wide && !self.reserved_wide && self.reserved > RESERVE_FROM;
        if total > self.reserved || widened {
            self.reserved = total
                .saturating_mul(2)
                .min(MAX_STRING_LENGTH)
                .max(self.reserved);
            self.reserved_wide = wide;
            let unit_bytes = if wide { 2 } else { 1 };
            rt.heap.reserve(self.reserved * unit_bytes, &rt.vm)?;
        }
        Ok(())
    }

    /// Appends a whole string (any rope, without a flatten).
    pub(super) fn push_string(&mut self, rt: &mut Runtime, string: Gc<JsString>) -> VmResult<()> {
        let count = rt.string_len(string)?;
        self.check(rt, count)?;
        rt.heap.push_string(&mut self.text, string)?;
        rt.charge_units(count)
    }

    /// Appends the units `start..end` of a flat string.
    pub(super) fn push_range(
        &mut self,
        rt: &mut Runtime,
        string: Gc<JsString>,
        start: usize,
        end: usize,
    ) -> VmResult<()> {
        if start >= end {
            return Ok(());
        }
        self.check(rt, end - start)?;
        let text = rt.heap.text(string)?;
        let piece = text
            .slice(start, end)
            .ok_or(VmError::invariant("a range outside the string"))?;
        self.text.push_str16(piece);
        rt.charge_units(end - start)
    }

    /// Appends text that does not come from the heap; the caller checked
    /// the room ([`Builder::check`]).
    pub(super) fn push_text(&mut self, text: Str16<'_>) {
        self.text.push_str16(text);
    }

    /// The text built so far, for a conversion that appends to it; the
    /// caller checked the room ([`Builder::check`]).
    pub(super) fn text_mut(&mut self) -> &mut String16 {
        &mut self.text
    }

    /// The number of code units built so far.
    pub(super) fn len(&self) -> usize {
        self.text.len()
    }

    /// The text, without a string (the heap memory is reserved as in
    /// [`Builder::finish`]).
    pub(super) fn into_text(self, rt: &mut Runtime) -> VmResult<String16> {
        if self.text.len() >= RESERVE_FROM {
            rt.heap.reserve(self.text.len() * 2, &rt.vm)?;
        }
        Ok(self.text)
    }

    /// Appends one code unit; the caller checked the room.
    pub(super) fn push_unit(&mut self, unit: u16) {
        self.text.push(unit);
    }

    /// Appends one code point; the caller checked the room.
    pub(super) fn push_code_point(&mut self, cp: u32) {
        self.text.push_code_point(cp);
    }

    /// The string, recorded in the handle scope.
    pub(super) fn finish(self, rt: &mut Runtime) -> VmResult<Gc<JsString>> {
        if self.text.len() >= RESERVE_FROM {
            rt.heap.reserve(self.text.len() * 2, &rt.vm)?;
        }
        rt.new_string(self.text)
    }
}

/// A native's result: a string.
#[allow(clippy::unnecessary_wraps, reason = "the result type of the natives")]
pub(super) fn string_result(string: Gc<JsString>) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::String(string)))
}

// --- Statics ---

/// `String.fromCharCode(...codeUnits)` (§22.1.2.1): `ToUint16` of each
/// argument.
fn from_char_code(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut text = String16::new();
    for index in 0..call.argc() {
        rt.tick()?;
        let value = rt.arg(call, index);
        let number = rt.to_number(value)?;
        text.push(to_uint32(number) as u16);
    }
    let string = rt.heap.alloc_string(text)?;
    string_result(string)
}

/// `String.fromCodePoint(...codePoints)` (§22.1.2.2). V8 words the
/// `RangeError` with the number that `ToNumber` gave.
fn from_code_point(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut text = String16::new();
    for index in 0..call.argc() {
        rt.tick()?;
        let value = rt.arg(call, index);
        let number = rt.to_number(value)?;
        if number.fract() != 0.0 || !(0.0..=1_114_111.0).contains(&number) {
            return Err(VmError::range_error(format!(
                "Invalid code point {}",
                number_to_string(number)
            )));
        }
        text.push_code_point(number as u32);
    }
    let string = rt.heap.alloc_string(text)?;
    string_result(string)
}

/// `String.raw(template, ...substitutions)` (§22.1.2.4).
fn raw(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let cooked = rt.this_object_arg(call, 0)?;
    let key = crate::string::PropertyKey::String(rt.vm.atoms.raw);
    let raw = rt.get_value(cooked.into(), key)?;
    let literals = rt.to_object(raw)?;
    rt.heap.record(literals);
    let count = rt.length_of_array_like(literals)?;
    if count <= 0.0 {
        return string_result(rt.vm.atoms.empty);
    }
    let substitutions = call.argc().saturating_sub(1);
    let mut builder = Builder::new();
    let mut next = 0.0;
    loop {
        rt.tick()?;
        let finished = rt.with_scope(|rt| {
            let key = rt.index_key(next)?;
            let literal = rt.get_value(literals.into(), key)?;
            let literal = rt.to_string(literal)?;
            builder.push_string(rt, literal)?;
            if next + 1.0 == count {
                return Ok(true);
            }
            if next < substitutions as f64 {
                let value = rt.arg(call, next as usize + 1);
                let substitution = rt.to_string(value)?;
                builder.push_string(rt, substitution)?;
            }
            Ok(false)
        })?;
        if finished {
            break;
        }
        next += 1.0;
    }
    string_result(builder.finish(rt)?)
}

// --- Character access ---

/// `String.prototype.at(index)` (§22.1.3.1).
fn at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.at")?;
    let length = rt.string_len(string)?;
    let relative = rt.integer_arg(call, 0)?;
    let index = if relative >= 0.0 {
        relative
    } else {
        length as f64 + relative
    };
    if index < 0.0 || index >= length as f64 {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    match rt.unit_at(string, index as usize)? {
        Some(unit) => string_result(rt.unit_string(unit)?),
        None => Ok(NativeReturn::Value(Value::Undefined)),
    }
}

/// `String.prototype.charAt(pos)` (§22.1.3.2).
fn char_at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.charAt")?;
    let position = rt.integer_arg(call, 0)?;
    let length = rt.string_len(string)?;
    if position < 0.0 || position >= length as f64 {
        return string_result(rt.vm.atoms.empty);
    }
    match rt.unit_at(string, position as usize)? {
        Some(unit) => string_result(rt.unit_string(unit)?),
        None => string_result(rt.vm.atoms.empty),
    }
}

/// `String.prototype.charCodeAt(pos)` (§22.1.3.3).
fn char_code_at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.charCodeAt")?;
    let position = rt.integer_arg(call, 0)?;
    let length = rt.string_len(string)?;
    if position < 0.0 || position >= length as f64 {
        return Ok(NativeReturn::Value(Value::Double(f64::NAN)));
    }
    let unit = rt.unit_at(string, position as usize)?;
    Ok(NativeReturn::Value(
        unit.map_or(Value::Double(f64::NAN), |u| Value::Int(i32::from(u))),
    ))
}

/// `String.prototype.codePointAt(pos)` (§22.1.3.4): `CodePointAt`
/// (§11.1.4).
fn code_point_at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.codePointAt")?;
    let position = rt.integer_arg(call, 0)?;
    let length = rt.string_len(string)?;
    if position < 0.0 || position >= length as f64 {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    let index = position as usize;
    let text = rt.flat_text(string)?;
    let first = text.get(index).unwrap_or(0);
    let cp = match text.get(index + 1) {
        Some(second) if is_lead_surrogate(first) && is_trail_surrogate(second) => {
            combine_surrogates(first, second)
        }
        _ => u32::from(first),
    };
    Ok(NativeReturn::Value(Value::number(f64::from(cp))))
}

// --- Concatenation and padding ---

/// `String.prototype.concat(...args)` (§22.1.3.6): a chain of
/// concatenations, so the result is a rope when it is long.
fn concat(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut result = rt.this_string(call, "String.prototype.concat")?;
    for index in 0..call.argc() {
        rt.tick()?;
        let next = rt.string_arg(call, index)?;
        result = rt.concat(result, next)?;
        rt.heap.record(result);
    }
    string_result(result)
}

/// The string `string` repeated `count` times: by doubling with
/// `concat`, so the result is a rope of O(log count) nodes. The
/// caller checked the length.
fn repeat_string(rt: &mut Runtime, string: Gc<JsString>, count: usize) -> VmResult<Gc<JsString>> {
    let mut result = rt.vm.atoms.empty;
    let mut base = string;
    let mut remaining = count;
    while remaining > 0 {
        rt.tick()?;
        if remaining & 1 == 1 {
            result = rt.concat(result, base)?;
        }
        remaining >>= 1;
        if remaining > 0 {
            base = rt.concat(base, base)?;
        }
    }
    Ok(result)
}

/// `String.prototype.repeat(count)` (§22.1.3.19). V8 words the
/// `RangeError` for a bad count with the number that `ToNumber` gave.
fn repeat(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.repeat")?;
    let value = rt.arg(call, 0);
    let number = rt.to_number(value)?;
    let count = to_integer_or_infinity(number);
    if count < 0.0 || count == f64::INFINITY {
        return Err(VmError::range_error(format!(
            "Invalid count value: {}",
            number_to_string(number)
        )));
    }
    let length = rt.string_len(string)?;
    if count == 0.0 || length == 0 {
        return string_result(rt.vm.atoms.empty);
    }
    if count * length as f64 > MAX_STRING_LENGTH as f64 {
        return Err(VmError::range_error(INVALID_STRING_LENGTH));
    }
    string_result(repeat_string(rt, string, count as usize)?)
}

/// `String.prototype.padEnd(maxLength, fillString)` (§22.1.3.16).
fn pad_end(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.padEnd")?;
    pad(rt, call, string, false)
}

/// `String.prototype.padStart(maxLength, fillString)` (§22.1.3.17).
fn pad_start(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.padStart")?;
    pad(rt, call, string, true)
}

/// `StringPaddingBuiltinsImpl` (§22.1.3.17.1) and `StringPad`
/// (§22.1.3.17.2).
fn pad(
    rt: &mut Runtime,
    call: &NativeCall,
    string: Gc<JsString>,
    at_start: bool,
) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    let max_length = to_length(rt.to_number(value)?);
    let length = rt.string_len(string)?;
    if max_length <= length as f64 {
        return string_result(string);
    }
    let filler = match rt.arg(call, 1) {
        Value::Undefined => rt.heap.intern_str(" ")?,
        value => rt.to_string(value)?,
    };
    rt.heap.record(filler);
    let filler_length = rt.string_len(filler)?;
    if filler_length == 0 {
        return string_result(string);
    }
    if max_length > MAX_STRING_LENGTH as f64 {
        return Err(VmError::range_error(INVALID_STRING_LENGTH));
    }
    let fill = max_length as usize - length;
    let whole = repeat_string(rt, filler, fill / filler_length)?;
    rt.heap.record(whole);
    let partial = rt.substring(filler, 0, fill % filler_length)?;
    rt.heap.record(partial);
    let padding = rt.concat(whole, partial)?;
    rt.heap.record(padding);
    let result = if at_start {
        rt.concat(padding, string)?
    } else {
        rt.concat(string, padding)?
    };
    string_result(result)
}

// --- Well-formed strings ---

/// The position of the first unpaired surrogate (`None`: the text is well
/// formed, `IsStringWellFormedUnicode`, §22.1.3.10 step 3).
fn first_lone_surrogate(text: Str16<'_>) -> Option<usize> {
    let Str16::Wide(units) = text else {
        return None;
    };
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        if is_lead_surrogate(unit) {
            if units.get(index + 1).is_some_and(|&n| is_trail_surrogate(n)) {
                index += 2;
                continue;
            }
            return Some(index);
        }
        if is_trail_surrogate(unit) {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// `String.prototype.isWellFormed()` (§22.1.3.10).
fn is_well_formed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.isWellFormed")?;
    let text = rt.flat_text(string)?;
    let units = text.len();
    let well_formed = first_lone_surrogate(text).is_none();
    rt.charge_units(units)?;
    Ok(NativeReturn::Value(Value::Bool(well_formed)))
}

/// `String.prototype.toWellFormed()` (§22.1.3.33): unpaired surrogates
/// become U+FFFD.
fn to_well_formed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.toWellFormed")?;
    let text = rt.flat_text(string)?;
    let units = text.len();
    if first_lone_surrogate(text).is_none() {
        rt.charge_units(units)?;
        return string_result(string);
    }
    if units >= RESERVE_FROM {
        rt.heap.reserve(units * 2, &rt.vm)?;
    }
    let text = rt.heap.text(string)?;
    let mut out = String16::new();
    let mut points = text.code_points();
    for cp in &mut points {
        out.push_code_point(if (0xD800..0xE000).contains(&cp) {
            0xFFFD
        } else {
            cp
        });
    }
    rt.charge_units(units)?;
    let result = rt.heap.alloc_string(out)?;
    string_result(result)
}

// --- Slices and trimming ---

/// `String.prototype.slice(start, end)` (§22.1.3.24).
fn slice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.slice")?;
    let length = rt.string_len(string)?;
    let from = clamp_relative(rt.integer_arg(call, 0)?, length);
    let to = match rt.arg(call, 1) {
        Value::Undefined => length,
        _ => clamp_relative(rt.integer_arg(call, 1)?, length),
    };
    string_result(rt.substring(string, from, to)?)
}

/// `String.prototype.substring(start, end)` (§22.1.3.25).
fn substring(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.substring")?;
    let length = rt.string_len(string)?;
    let start = clamp_index(rt.integer_arg(call, 0)?, length);
    let end = match rt.arg(call, 1) {
        Value::Undefined => length,
        _ => clamp_index(rt.integer_arg(call, 1)?, length),
    };
    string_result(rt.substring(string, start.min(end), start.max(end))?)
}

/// Whether a code unit is `WhiteSpace` or a `LineTerminator`
/// (`TrimString`, §22.1.3.32.1).
fn is_trimmed(unit: u16) -> bool {
    is_whitespace(u32::from(unit)) || is_line_terminator(u32::from(unit))
}

/// `TrimString(string, where)` (§22.1.3.32.1) for `String.prototype.trim`
/// and its two halves. V8 names `trimStart` and `trimEnd` in the message
/// of their receiver check "trimLeft" and "trimRight".
fn trim_string(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &str,
    start: bool,
    end: bool,
) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, method)?;
    let text = rt.flat_text(string)?;
    let length = text.len();
    let mut from = 0;
    let mut to = length;
    if start {
        while from < to && text.get(from).is_some_and(is_trimmed) {
            from += 1;
        }
    }
    if end {
        while to > from && text.get(to - 1).is_some_and(is_trimmed) {
            to -= 1;
        }
    }
    rt.charge_units(from + (length - to))?;
    string_result(rt.substring(string, from, to)?)
}

/// `String.prototype.trim()` (§22.1.3.30).
fn trim(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    trim_string(rt, call, "String.prototype.trim", true, true)
}

/// `String.prototype.trimStart()` (§22.1.3.32).
fn trim_start(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    trim_string(rt, call, "String.prototype.trimLeft", true, false)
}

/// `String.prototype.trimEnd()` (§22.1.3.31).
fn trim_end(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    trim_string(rt, call, "String.prototype.trimRight", false, true)
}
