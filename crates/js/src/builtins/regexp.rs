//! `RegExp` (ECMA-262 §22.2.4 to §22.2.6,
//! <https://tc39.es/ecma262/#sec-regexp-regular-expression-objects>): the
//! constructor with `RegExpCreate`, `RegExpAlloc` and `RegExpInitialize`
//! (§22.2.3), `RegExp.escape` (§22.2.5.1), `RegExp[@@species]`, the
//! accessors of `RegExp.prototype` (the flags, `flags` and `source` with
//! `EscapeRegExpPattern`), `exec`, `test`, `toString`, Annex B
//! `compile` (§B.2.4), and the evaluation of regular expression literals
//! (§13.2.7.3). The match itself is in [`super::regexp_exec`], the
//! symbol methods in [`super::regexp_symbol`], the legacy static
//! properties in [`super::regexp_legacy`].
//!
//! The keys of `RegExp` and `RegExp.prototype` are in Chromium 148's
//! order. The messages are V8's (measured with
//! `tools/probes/js-regexp-builtin.json`).
//!
//! Deliberate differences from Chromium 148: the getter `flags` reads
//! `unicodeSets` before `sticky`, as §22.2.6.4 says (V8 reads `sticky`
//! first); messages that quote a pattern with lone surrogates show
//! U+FFFD for them.

use std::rc::Rc;

use swb_js_regexp::{Error as RegexError, Flags, Regex};
use swb_js_text::{Str16, String16};

use crate::bytecode::{Constant, FunctionCode};
use crate::error::ThrowKind;
use crate::heap::Gc;
use crate::object::{Object, ObjectKind, PropertyDescriptor, RegExpData};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

/// The flag getters of `RegExp.prototype` in Chromium 148's key order
/// (`flags` and `source` are between them).
const FLAG_GETTERS: [(&str, NativeFn); 8] = [
    ("dotAll", dot_all),
    ("global", global),
    ("hasIndices", has_indices),
    ("ignoreCase", ignore_case),
    ("multiline", multiline),
    ("sticky", sticky),
    ("unicode", unicode),
    ("unicodeSets", unicode_sets),
];

/// The properties that `get flags` reads, with their letters, in the
/// order of §22.2.6.4.
const FLAG_PROPERTIES: [(&str, u8); 8] = [
    ("hasIndices", b'd'),
    ("global", b'g'),
    ("ignoreCase", b'i'),
    ("multiline", b'm'),
    ("dotAll", b's'),
    ("unicode", b'u'),
    ("unicodeSets", b'v'),
    ("sticky", b'y'),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::RegExpPrototype)?;
    let ctor = super::constructor(rt, realm, "RegExp", 2, regexp_constructor, proto)?;
    rt.set_intrinsic(realm, Intrinsic::RegExpConstructor, ctor);
    super::regexp_legacy::install(rt, realm, ctor)?;
    super::method(rt, realm, ctor, "escape", 1, escape)?;
    let species = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    rt.set_intrinsic(realm, Intrinsic::RegExpSpeciesGetter, species);
    let key = rt.symbol_key(WellKnown::Species);
    let desc = PropertyDescriptor::accessor(species.into(), Value::Undefined, false, true);
    rt.define_own_property(ctor, key, desc)?;

    let exec = super::method(rt, realm, proto, "exec", 1, exec)?;
    rt.set_intrinsic(realm, Intrinsic::RegExpExec, exec);
    for (name, func) in FLAG_GETTERS {
        getter(rt, realm, proto, name, func)?;
        match name {
            "dotAll" => getter(rt, realm, proto, "flags", flags)?,
            "multiline" => getter(rt, realm, proto, "source", source)?,
            _ => {}
        }
    }
    super::method(rt, realm, proto, "compile", 2, compile)?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    super::method(rt, realm, proto, "test", 1, test)?;
    super::regexp_symbol::install(rt, realm, proto)?;
    super::regexp_fast::record_shapes(rt, realm)
}

/// Defines an accessor with a getter only (non-enumerable, configurable).
fn getter(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    name: &str,
    func: NativeFn,
) -> VmResult<()> {
    let function = rt.new_builtin(realm, &format!("get {name}"), 0, func, false)?;
    let key = rt.heap.key_from_str(name)?;
    let desc = PropertyDescriptor::accessor(function.into(), Value::Undefined, false, true);
    rt.define_own_property(target, key, desc)?;
    Ok(())
}

/// `get RegExp[@@species]` (§22.2.5.2): the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

// --- Creation ---

/// A `SyntaxError` with a message.
pub(super) fn syntax_error(message: String) -> VmError {
    VmError::Raise {
        kind: ThrowKind::SyntaxError,
        message: message.into(),
    }
}

impl Runtime {
    /// The data of a `RegExp` object, or `None` for another value.
    pub(super) fn regexp_data(&self, value: Value) -> VmResult<Option<&RegExpData>> {
        let Value::Object(object) = value else {
            return Ok(None);
        };
        Ok(match &self.heap.object(object)?.kind {
            ObjectKind::RegExp(data) => Some(data),
            _ => None,
        })
    }

    /// The compiled pattern of a `RegExp` object (`[[RegExpMatcher]]`).
    pub(super) fn regexp_program(&self, object: Gc<Object>) -> VmResult<Rc<Regex>> {
        match &self.heap.object(object)?.kind {
            ObjectKind::RegExp(data) => Ok(Rc::clone(&data.regex)),
            _ => Err(VmError::invariant("a RegExp operation on another object")),
        }
    }

    /// Steps 1 to 16 of `RegExpInitialize` (§22.2.3.3) before the object
    /// changes: `ToString` of the pattern and the flags (`undefined` is the
    /// empty string), the check of the flags and the compile. The source
    /// is rooted.
    pub(super) fn regexp_parse(
        &mut self,
        pattern: Value,
        flags: Value,
    ) -> VmResult<(Gc<JsString>, Rc<Regex>)> {
        let source = match pattern {
            Value::Undefined => self.vm.atoms.empty,
            value => self.to_string(value)?,
        };
        self.heap.record(source);
        let flags_text = match flags {
            Value::Undefined => self.vm.atoms.empty,
            value => self.to_string(value)?,
        };
        self.heap.record(flags_text);
        self.flatten(source)?;
        self.flatten(flags_text)?;
        let parsed = swb_js_regexp::parse_flags(self.heap.text(flags_text)?);
        let Ok(flag_set) = parsed else {
            return Err(syntax_error(format!(
                "Invalid flags supplied to RegExp constructor '{}'",
                self.heap.string_lossy(flags_text, usize::MAX)
            )));
        };
        let regex = self.regexp_compile(source, flag_set, flags_text)?;
        Ok((source, regex))
    }

    /// Compiles a flat pattern. The program's memory is reserved first (an
    /// estimate from the pattern's length) and charged afterwards.
    pub(super) fn regexp_compile(
        &mut self,
        source: Gc<JsString>,
        flags: Flags,
        flags_text: Gc<JsString>,
    ) -> VmResult<Rc<Regex>> {
        let length = self.string_len(source)?;
        // The tree and the program take some tens of bytes per character.
        self.heap.reserve(length.saturating_mul(64), &self.vm)?;
        let compiled = Regex::new(self.heap.text(source)?, flags);
        self.charge_units(length.saturating_mul(16))?;
        match compiled {
            Ok(regex) => {
                self.heap.charge(regex.heap_bytes());
                Ok(Rc::new(regex))
            }
            Err(error) => Err(self.regexp_error(&error, source, flags_text)),
        }
    }

    /// The `SyntaxError` of a pattern that does not compile.
    fn regexp_error(
        &self,
        error: &RegexError,
        source: Gc<JsString>,
        flags_text: Gc<JsString>,
    ) -> VmError {
        let pattern = self.heap.string_lossy(source, usize::MAX);
        let flags = self.heap.string_lossy(flags_text, usize::MAX);
        match error {
            RegexError::Syntax(kind) => syntax_error(format!(
                "Invalid regular expression: /{pattern}/{flags}: {}",
                kind.message()
            )),
            other => syntax_error(format!(
                "Invalid regular expression: /{pattern}/{flags}: {other}"
            )),
        }
    }

    /// `RegExpAlloc` (§22.2.3.2) and the end of `RegExpInitialize` for a
    /// new object: a `RegExp` object with `proto`, its slots and
    /// `lastIndex` = 0 (writable, not enumerable, not configurable).
    /// Recorded in the handle scope.
    pub(super) fn new_regexp_object(
        &mut self,
        proto: Gc<Object>,
        source: Gc<JsString>,
        regex: Rc<Regex>,
    ) -> VmResult<Gc<Object>> {
        let kind = ObjectKind::RegExp(Box::new(RegExpData { source, regex }));
        if let Some(shape) = self.instance_shape_for(proto)? {
            let object =
                self.heap
                    .new_object_with_shape(shape, vec![Value::Int(0)], Vec::new(), kind)?;
            return Ok(self.heap.record(object));
        }
        let object = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(object);
        let key = PropertyKey::String(self.vm.atoms.last_index);
        self.define(object, key, Value::Int(0), true, false, false)?;
        Ok(object)
    }

    /// `RegExpCreate(P, F)` (§22.2.3.1) in the current realm.
    pub(super) fn regexp_create(&mut self, pattern: Value, flags: Value) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(self.current_realm(), Intrinsic::RegExpPrototype)?;
        let (source, regex) = self.regexp_parse(pattern, flags)?;
        self.new_regexp_object(proto, source, regex)
    }

    /// The evaluation of a regular expression literal (§13.2.7.3): a new
    /// object from `RegExpCreate` with the literal's source and flags. The
    /// first evaluation compiles the pattern; the constant keeps the
    /// program for the later ones. Runs inside a handle scope.
    #[inline(never)]
    pub(crate) fn regexp_literal(
        &mut self,
        code: &Rc<FunctionCode>,
        index: u32,
        realm: u32,
    ) -> VmResult<Value> {
        let Some(Constant::RegExp(literal)) = code.constants.get(index as usize) else {
            return Err(VmError::invariant(
                "a regular expression constant that is not one",
            ));
        };
        let source = literal.source;
        let cached = literal.program.borrow().clone();
        let regex = if let Some(regex) = cached {
            regex
        } else {
            let letters: String = literal.flags.letters().collect();
            let flags_text = self.heap.intern_str(&letters)?;
            self.heap.record(flags_text);
            let regex = self.regexp_compile(source, literal.flags, flags_text)?;
            *literal.program.borrow_mut() = Some(Rc::clone(&regex));
            regex
        };
        let proto = self.intrinsic(realm, Intrinsic::RegExpPrototype)?;
        Ok(self.new_regexp_object(proto, source, regex)?.into())
    }
}

/// `RegExp(pattern, flags)` (§22.2.4.1).
fn regexp_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let pattern = rt.arg(call, 0);
    let flags = rt.arg(call, 1);
    let pattern_is_regexp = rt.is_regexp(pattern)?;
    if call.new_target().is_undefined() && pattern_is_regexp && flags.is_undefined() {
        let key = PropertyKey::String(rt.vm.atoms.constructor);
        let pattern_constructor = rt.get_value(pattern, key)?;
        if pattern_constructor == Value::Object(call.callee()) {
            return Ok(NativeReturn::Value(pattern));
        }
    }
    let (p, f) = if let Some(data) = rt.regexp_data(pattern)? {
        let source = data.source;
        let f = if flags.is_undefined() {
            let letters: String = data.regex.flags().letters().collect();
            let text = rt.heap.intern_str(&letters)?;
            Value::String(rt.heap.record(text))
        } else {
            flags
        };
        (Value::String(source), f)
    } else if pattern_is_regexp {
        let key = PropertyKey::String(rt.vm.atoms.source);
        let p = rt.get_value(pattern, key)?;
        rt.heap.record(p);
        let f = if flags.is_undefined() {
            let key = PropertyKey::String(rt.vm.atoms.flags);
            let f = rt.get_value(pattern, key)?;
            rt.heap.record(f)
        } else {
            flags
        };
        (p, f)
    } else {
        (pattern, flags)
    };
    let proto = rt.prototype_from(call, Intrinsic::RegExpPrototype)?;
    let (source, regex) = rt.regexp_parse(p, f)?;
    let object = rt.new_regexp_object(proto, source, regex)?;
    Ok(NativeReturn::Value(object.into()))
}

// --- RegExp.escape ---

/// `RegExp.escape(S)` (§22.2.5.1): every character that could have a
/// meaning in a pattern becomes an escape; an initial letter or digit
/// becomes `\xHH`.
fn escape(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::String(string) = rt.arg(call, 0) else {
        return Err(VmError::type_error("input argument must be a string"));
    };
    rt.flatten(string)?;
    let length = rt.string_len(string)?;
    let mut out = super::string::Builder::new();
    let mut index = 0;
    while index < length {
        // At most six units per code point, checked per chunk.
        let end = (index + 4096).min(length);
        out.check(rt, (end - index) * 6)?;
        let start = index;
        let text = rt.heap.text(string)?;
        while index < end {
            let (cp, next) = text.code_point_at(index).unwrap_or((0, index + 1));
            escape_code_point(out.text_mut(), cp, index == 0);
            index = next;
        }
        rt.charge_units(index - start)?;
    }
    let result = out.finish(rt)?;
    Ok(NativeReturn::Value(result.into()))
}

/// Steps 4.a to 4.c of `RegExp.escape` for one code point
/// (`EncodeForRegExpEscape`, §22.2.5.1.1).
fn escape_code_point(out: &mut String16, cp: u32, first: bool) {
    let ascii = u8::try_from(cp).ok().filter(u8::is_ascii);
    if first && ascii.is_some_and(|c| c.is_ascii_alphanumeric()) {
        push_hex(out, b'x', cp, 2);
        return;
    }
    if let Some(c) = ascii {
        if b"^$\\.*+?()[]{}|/".contains(&c) {
            out.push(u16::from(b'\\'));
            out.push(u16::from(c));
            return;
        }
        let control = match c {
            b'\t' => Some(b't'),
            b'\n' => Some(b'n'),
            0x0B => Some(b'v'),
            0x0C => Some(b'f'),
            b'\r' => Some(b'r'),
            _ => None,
        };
        if let Some(letter) = control {
            out.push(u16::from(b'\\'));
            out.push(u16::from(letter));
            return;
        }
        if b",-=<>#&!%:;@~'`\"".contains(&c) || c == b' ' {
            push_hex(out, b'x', cp, 2);
            return;
        }
        out.push(u16::from(c));
        return;
    }
    let special = swb_js_text::unicode::is_whitespace(cp)
        || swb_js_text::unicode::is_line_terminator(cp)
        || (0xD800..=0xDFFF).contains(&cp);
    if special {
        if cp <= 0xFF {
            push_hex(out, b'x', cp, 2);
        } else {
            push_hex(out, b'u', cp, 4);
        }
        return;
    }
    out.push_code_point(cp);
}

/// Appends `\x` or `\u` and `digits` lowercase hexadecimal digits.
fn push_hex(out: &mut String16, letter: u8, value: u32, digits: u32) {
    out.push(u16::from(b'\\'));
    out.push(u16::from(letter));
    for shift in (0..digits).rev() {
        let digit = (value >> (shift * 4)) & 0xF;
        let c = char::from_digit(digit, 16).unwrap_or('0');
        out.push(c as u16);
    }
}

// --- The accessors of RegExp.prototype ---

/// `RegExpHasFlag(R, flag)` (§22.2.6.4.1) with V8's message.
fn has_flag(
    rt: &mut Runtime,
    call: &NativeCall,
    flag: Flags,
    name: &str,
) -> VmResult<NativeReturn> {
    let this = call.this();
    if let Some(data) = rt.regexp_data(this)? {
        return Ok(NativeReturn::Value(Value::Bool(
            data.regex.flags().contains(flag),
        )));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::RegExpPrototype)?;
    if this == Value::Object(proto) {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    Err(VmError::type_error(format!(
        "RegExp.prototype.{name} getter called on non-RegExp object"
    )))
}

fn dot_all(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::DOT_ALL, "dotAll")
}

fn global(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::GLOBAL, "global")
}

fn has_indices(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::HAS_INDICES, "hasIndices")
}

fn ignore_case(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::IGNORE_CASE, "ignoreCase")
}

fn multiline(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::MULTILINE, "multiline")
}

fn sticky(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::STICKY, "sticky")
}

fn unicode(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::UNICODE, "unicode")
}

fn unicode_sets(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    has_flag(rt, call, Flags::UNICODE_SETS, "unicodeSets")
}

/// `get RegExp.prototype.flags` (§22.2.6.4): eight reads in the order of
/// the specification. A `RegExp` object whose prototype and getters are
/// the built-in ones gives its flags without the reads (they could not
/// be observed).
fn flags(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = call.this() else {
        return Err(VmError::type_error(format!(
            "RegExp.prototype.flags getter called on non-object {}",
            rt.primitive_text(call.this())
        )));
    };
    let text = rt.flags_text(object)?;
    Ok(NativeReturn::Value(text.into()))
}

impl Runtime {
    /// `ToString(Get(R, "flags"))` for the symbol methods, or the result
    /// of the getter steps for an object (see [`flags`]). Rooted.
    pub(super) fn flags_text(&mut self, object: Gc<Object>) -> VmResult<Gc<JsString>> {
        if let Some(flags) = self.pristine_flags(object)? {
            let letters: String = flags.letters().collect();
            let text = self.heap.intern_str(&letters)?;
            return Ok(self.heap.record(text));
        }
        let mut letters = String::new();
        for (name, letter) in FLAG_PROPERTIES {
            let key = self.heap.key_from_str(name)?;
            let value = self.get_value(object.into(), key)?;
            if self.to_boolean(value)? {
                letters.push(char::from(letter));
            }
        }
        let text = self.heap.intern_str(&letters)?;
        Ok(self.heap.record(text))
    }

    /// `ToString(Get(R, "flags"))` (step 7 of `@@replace` and the other
    /// symbol methods), with the shortcut of [`Runtime::flags_text`] when
    /// the `flags` getter is the built-in one.
    pub(super) fn get_flags_string(&mut self, object: Gc<Object>) -> VmResult<Gc<JsString>> {
        if self.pristine_flags(object)?.is_some() {
            return self.flags_text(object);
        }
        let key = PropertyKey::String(self.vm.atoms.flags);
        let value = self.get_value(object.into(), key)?;
        let text = self.to_string(value)?;
        Ok(self.heap.record(text))
    }
}

/// `get RegExp.prototype.source` (§22.2.6.13).
fn source(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = call.this();
    if let Some(data) = rt.regexp_data(this)? {
        let (source, flags) = (data.source, data.regex.flags());
        let text = rt.escape_pattern(source, flags)?;
        return Ok(NativeReturn::Value(text.into()));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::RegExpPrototype)?;
    if this == Value::Object(proto) {
        let text = rt.heap.intern_str("(?:)")?;
        return Ok(NativeReturn::Value(text.into()));
    }
    Err(VmError::type_error(
        "RegExp.prototype.source getter called on non-RegExp object",
    ))
}

impl Runtime {
    /// `EscapeRegExpPattern(P, F)` (§22.2.6.13.1): the source in the form
    /// of a literal. A `/` outside a class becomes `\/`, a line terminator
    /// its escape, the empty pattern `(?:)` (V8's text, measured).
    fn escape_pattern(&mut self, source: Gc<JsString>, flags: Flags) -> VmResult<Gc<JsString>> {
        self.flatten(source)?;
        let length = self.string_len(source)?;
        if length == 0 {
            return Ok(self.heap.intern_str("(?:)")?);
        }
        let text = self.heap.text(source)?;
        let needs_escape = text.units().any(|u| {
            u == u16::from(b'/')
                || u == u16::from(b'\n')
                || u == u16::from(b'\r')
                || u == 0x2028
                || u == 0x2029
        });
        if !needs_escape {
            return Ok(source);
        }
        let out = escape_pattern_text(text, flags);
        self.charge_units(length)?;
        let result = self.heap.alloc_string(out)?;
        Ok(self.heap.record(result))
    }
}

/// The text of `EscapeRegExpPattern` for a non-empty pattern (see
/// [`Runtime::escape_pattern`]); also for `console.log`.
pub(super) fn escape_pattern_text(text: Str16<'_>, flags: Flags) -> String16 {
    let nested = flags.contains(Flags::UNICODE_SETS);
    let mut out = String16::new();
    let mut depth = 0u32;
    let mut index = 0;
    while let Some(unit) = text.get(index) {
        index += 1;
        match unit {
            0x5C => {
                out.push(unit);
                if let Some(next) = text.get(index) {
                    index += 1;
                    push_escaped_unit(&mut out, next);
                }
            }
            0x2F if depth == 0 => {
                out.push(u16::from(b'\\'));
                out.push(unit);
            }
            0x5B => {
                if nested || depth == 0 {
                    depth += 1;
                }
                out.push(unit);
            }
            0x5D => {
                depth = depth.saturating_sub(1);
                out.push(unit);
            }
            _ => push_escaped_unit(&mut out, unit),
        }
    }
    out
}

/// Appends a unit, a line terminator as its escape (`\n`, `\r`,
/// ` `, ` `).
fn push_escaped_unit(out: &mut String16, unit: u16) {
    let escape: Option<&[u8]> = match unit {
        0x0A => Some(b"\\n"),
        0x0D => Some(b"\\r"),
        0x2028 => Some(b"\\u2028"),
        0x2029 => Some(b"\\u2029"),
        _ => None,
    };
    match escape {
        Some(text) => {
            for &b in text {
                out.push(u16::from(b));
            }
        }
        None => out.push(unit),
    }
}

// --- The methods of RegExp.prototype ---

impl Runtime {
    /// The `RegExp` object `this` of a method that needs
    /// `[[RegExpMatcher]]`, with V8's message.
    pub(super) fn this_regexp(&self, call: &NativeCall, method: &str) -> VmResult<Gc<Object>> {
        if let Value::Object(object) = call.this()
            && matches!(self.heap.object(object)?.kind, ObjectKind::RegExp(_))
        {
            return Ok(object);
        }
        Err(self.incompatible_receiver(method, call.this()))
    }

    /// `this` of a symbol method, `toString` or `test`: an object.
    pub(super) fn this_regexp_object(
        &self,
        call: &NativeCall,
        method: &str,
    ) -> VmResult<Gc<Object>> {
        match call.this() {
            Value::Object(object) => Ok(object),
            other => Err(self.incompatible_receiver(method, other)),
        }
    }
}

/// `RegExp.prototype.exec(string)` (§22.2.6.2).
fn exec(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp(call, "RegExp.prototype.exec")?;
    let string = rt.string_arg(call, 0)?;
    rt.heap.record(string);
    let result = rt.regexp_builtin_exec(regexp, string)?;
    Ok(NativeReturn::Value(result))
}

/// `RegExp.prototype.test(S)` (§22.2.6.16).
fn test(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.test")?;
    let string = rt.string_arg(call, 0)?;
    rt.heap.record(string);
    let found = rt.regexp_exec_matches(regexp, string)?;
    Ok(NativeReturn::Value(Value::Bool(found)))
}

/// `RegExp.prototype.toString()` (§22.2.6.17).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp_object(call, "RegExp.prototype.toString")?;
    let key = PropertyKey::String(rt.vm.atoms.source);
    let pattern = rt.get_value(regexp.into(), key)?;
    let pattern = rt.to_string(pattern)?;
    rt.heap.record(pattern);
    let key = PropertyKey::String(rt.vm.atoms.flags);
    let flags = rt.get_value(regexp.into(), key)?;
    let flags = rt.to_string(flags)?;
    rt.heap.record(flags);
    let mut out = super::string::Builder::new();
    out.check(rt, 2)?;
    out.push_unit(u16::from(b'/'));
    out.push_string(rt, pattern)?;
    out.check(rt, 1)?;
    out.push_unit(u16::from(b'/'));
    out.push_string(rt, flags)?;
    let result = out.finish(rt)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `RegExp.prototype.compile(pattern, flags)` (Annex B §B.2.4.1). V8
/// (measured) does not check the realm or `[[LegacyFeaturesEnabled]]` of
/// the legacy features proposal, so objects of subclasses compile too.
fn compile(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let regexp = rt.this_regexp(call, "RegExp.prototype.compile")?;
    let pattern = rt.arg(call, 0);
    let flags = rt.arg(call, 1);
    let (p, f) = if let Some(data) = rt.regexp_data(pattern)? {
        if !flags.is_undefined() {
            return Err(VmError::type_error(
                "Cannot supply flags when constructing one RegExp from another",
            ));
        }
        let source = data.source;
        let letters: String = data.regex.flags().letters().collect();
        let text = rt.heap.intern_str(&letters)?;
        rt.heap.record(text);
        (Value::String(source), Value::String(text))
    } else {
        (pattern, flags)
    };
    // The new program was charged when it was compiled.
    let (source, regex) = rt.regexp_parse(p, f)?;
    if let ObjectKind::RegExp(data) = &mut rt.heap.object_mut(regexp)?.kind {
        data.source = source;
        data.regex = regex;
    }
    let key = PropertyKey::String(rt.vm.atoms.last_index);
    rt.set_or_throw(regexp, key, Value::Int(0))?;
    Ok(NativeReturn::Value(regexp.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escaped(text: &str) -> String {
        let mut out = String16::new();
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut index = 0;
        while let Some((cp, next)) = Str16::Wide(&units).code_point_at(index) {
            escape_code_point(&mut out, cp, index == 0);
            index = next;
        }
        out.to_string_lossy()
    }

    #[test]
    fn escape_follows_the_specification() {
        assert_eq!(escaped("abc"), "\\x61bc");
        assert_eq!(escaped("a.b*c"), "\\x61\\.b\\*c");
        assert_eq!(escaped(" -_"), "\\x20\\x2d_");
        assert_eq!(escaped("\u{2028}\u{a0}\u{feff}"), "\\u2028\\xa0\\ufeff");
        assert_eq!(escaped("x0"), "\\x780");
        assert_eq!(escaped("\t\n"), "\\t\\n");
    }
}
