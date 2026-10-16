//! `JSON.stringify` (ECMA-262 §25.5.2 with `SerializeJSONProperty`
//! §25.5.2.2, `QuoteJSONString` §25.5.2.3, `SerializeJSONObject`
//! §25.5.2.5 and `SerializeJSONArray` §25.5.2.6; raw JSON objects from
//! <https://tc39.es/proposal-json-parse-with-source/>).
//!
//! The serializer is a loop over an explicit stack of the open arrays and
//! objects, so a structure that is deep costs no Rust stack (Chromium
//! serializes a million levels of plain data too; swb stops at
//! [`MAX_DEPTH`] levels, or [`MAX_DEPTH_SLOW`] with line breaks or a
//! replacer, as the native stack ends Chromium's recursion there, and so
//! ends a `toJSON` or replacer that makes a new object at every level). The
//! cycle check (`stack` of §25.5.2) is the stack itself, plus a set of the
//! open objects once it is deep, so it costs no more than a hash lookup
//! per object however deep the structure is. The code that a stringify
//! runs (`toJSON`, the replacer, getters, `valueOf` and `toString` of
//! wrappers) runs through re-entry, which the recursion budget limits.
//!
//! The output ([`Output`]) is one flat buffer. It stops at V8's maximum
//! string length with a `RangeError` before it grows, checks the heap
//! limit at each doubling, and the time is charged for what is written.
//! Every member step that can run script code or allocate runs in a
//! handle scope of its own; a step that opens an object or array keeps
//! its scope until the container closes, so the open containers and the
//! keys that they still visit are rooted, and a long array does not grow
//! the scope. A member that is a primitive in an own data property of an
//! ordinary object or an array (the common case) is read and written
//! without a scope and without running any code: it is reachable from its
//! holder, which is rooted.
//!
//! A cycle is a `TypeError` with V8's message: the object where the
//! cycle starts and its path as in Chromium 148. The constructor name
//! that the message shows follows V8's rule as far as measurable: the
//! name of the `constructor` data property of the prototype chain (its
//! own `name` if that is a non-empty string, else the name of its code),
//! else, for an object whose constructor is `Object`, a string
//! `@@toStringTag`, else `Object`. V8 uses the function that created the
//! object; swb does not record it.

use std::collections::HashSet;
use std::rc::Rc;

use swb_js_text::{Str16, String16};

use crate::heap::{Gc, HandleScope};
use crate::object::{Object, ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::convert::{MAX_STRING_LENGTH, key_as_value, to_integer_or_infinity};
use crate::vm::{
    Intrinsic, NativeCall, NativeReturn, STACK_OVERFLOW, VmError, VmResult, WellKnown,
};

/// The depth from which the open objects are also kept in a set.
const SET_FROM_DEPTH: usize = 16;

/// The deepest nesting that a plain serialization allows (about 30 MiB of
/// levels). Chromium serializes 1,000,000 levels of plain data.
const MAX_DEPTH: usize = 100_000;

/// The deepest nesting when the output has line breaks or a replacer is
/// in use. Chromium recurses on its native stack in these cases and ends
/// at 3,256 (a property list) to 5,157 levels (measured with arrays,
/// objects and getters); the line breaks and indentation of a deep
/// structure also make an output of quadratic size.
const MAX_DEPTH_SLOW: usize = 4_000;

/// The most units of `space` that are used.
const MAX_GAP: usize = 10;

/// The message of the `TypeError` for a `BigInt`.
const BIGINT_MESSAGE: &str = "Do not know how to serialize a BigInt";

/// The message of the `RangeError` for a text above the maximum length.
const INVALID_STRING_LENGTH: &str = "Invalid string length";

/// A text above this many units reserves its memory in the heap's
/// accounting while it grows.
const RESERVE_FROM: usize = 1 << 16;

/// The entries of the cache of quoted keys.
const KEY_CACHE_SIZE: usize = 64;

/// The property list of the replacer, shared by the objects.
type PropertyList = Option<Rc<Vec<PropertyKey>>>;

/// What a property serializes to, before anything is written.
#[derive(Clone, Copy)]
enum Resolved {
    /// `undefined`: nothing is written (an array writes `null`).
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Gc<JsString>),
    /// The text of a raw JSON object.
    Raw(Gc<JsString>),
    /// An array or an object: it opens a level.
    Container(Gc<Object>),
}

/// The keys of an object level.
enum Keys {
    /// Listed for this object.
    Own(Vec<PropertyKey>),
    /// The property list of the replacer.
    Shared(Rc<Vec<PropertyKey>>),
}

impl Keys {
    fn get(&self, index: usize) -> Option<PropertyKey> {
        match self {
            Keys::Own(keys) => keys.get(index).copied(),
            Keys::Shared(keys) => keys.get(index).copied(),
        }
    }
}

enum Members {
    Elements { length: u32, next: u32 },
    Properties { keys: Keys, next: usize },
}

/// An open array or object.
struct Level {
    object: Gc<Object>,
    members: Members,
    scope: HandleScope,
    /// The key of the object in its holder, for the message of a cycle.
    key: PropertyKey,
    /// Whether the holder is an array.
    in_array: bool,
    /// The length of `indent` outside this container.
    indent_outside: usize,
    /// The number of members written.
    written: usize,
}

/// The text that is being written: one flat buffer in the narrow width
/// until a unit of 256 or above comes.
struct Output {
    narrow: Vec<u8>,
    wide: Vec<u16>,
    is_wide: bool,
    reserved: usize,
}

impl Output {
    fn new() -> Self {
        Output {
            narrow: Vec::new(),
            wide: Vec::new(),
            is_wide: false,
            reserved: RESERVE_FROM,
        }
    }

    fn len(&self) -> usize {
        if self.is_wide {
            self.wide.len()
        } else {
            self.narrow.len()
        }
    }

    /// Makes room for `extra` more units: V8's maximum length, then the
    /// heap limit for the buffer after its next doubling, in bytes of the
    /// buffer's width (a safepoint when a doubling passes the reserved
    /// size; the caller roots its handles). The reservation stops at the
    /// maximum length, so that an output too long for a string is the
    /// `RangeError`, as in V8, and not the heap limit.
    fn check(&mut self, rt: &mut Runtime, extra: usize) -> VmResult<()> {
        let total = self.len().saturating_add(extra);
        if total > MAX_STRING_LENGTH {
            return Err(VmError::range_error(INVALID_STRING_LENGTH));
        }
        if total > self.reserved {
            self.reserved = total.saturating_mul(2).min(MAX_STRING_LENGTH);
            let unit_bytes = if self.is_wide { 2 } else { 1 };
            rt.heap.reserve(self.reserved * unit_bytes, &rt.vm)?;
        }
        Ok(())
    }

    /// Changes to the wide width. The next [`Output::check`] reserves
    /// again for two bytes per unit.
    fn widen(&mut self) {
        if !self.is_wide {
            self.wide = self.narrow.iter().map(|&byte| u16::from(byte)).collect();
            self.narrow = Vec::new();
            self.is_wide = true;
            self.reserved = self.reserved.min(RESERVE_FROM);
        }
    }

    #[inline]
    fn byte(&mut self, byte: u8) {
        if self.is_wide {
            self.wide.push(u16::from(byte));
        } else {
            self.narrow.push(byte);
        }
    }

    #[inline]
    fn ascii(&mut self, text: &[u8]) {
        if self.is_wide {
            self.wide.extend(text.iter().map(|&byte| u16::from(byte)));
        } else {
            self.narrow.extend_from_slice(text);
        }
    }

    fn units(&mut self, text: Str16<'_>) {
        match text {
            Str16::Latin1(bytes) => self.ascii(bytes),
            Str16::Wide(units) => {
                if !self.is_wide && units.iter().any(|&unit| unit > 0xff) {
                    self.widen();
                }
                if self.is_wide {
                    self.wide.extend_from_slice(units);
                } else {
                    self.narrow.extend(units.iter().map(|&unit| unit as u8));
                }
            }
        }
    }

    /// The string, recorded in the handle scope.
    fn finish(self, rt: &mut Runtime) -> VmResult<Gc<JsString>> {
        let len = self.len();
        if len >= RESERVE_FROM {
            rt.heap.reserve(len * 2, &rt.vm)?;
        }
        let text = if self.is_wide {
            String16::from_units(&self.wide)
        } else {
            Str16::Latin1(&self.narrow).to_string16()
        };
        rt.charge_units(len)?;
        let string = rt.heap.alloc_string(text)?;
        Ok(rt.heap.record(string))
    }
}

/// The quoted text of a key and the colon after it (and a space in the
/// pretty form), for a key that is an atom of Latin-1 text.
struct CachedKey {
    atom: Gc<JsString>,
    text: Vec<u8>,
}

struct Serializer {
    replacer: Option<Value>,
    list: PropertyList,
    gap: Vec<u16>,
    /// Whether the output has line breaks (V8 also has them for a `space`
    /// between 0 and 1, with no indentation).
    pretty: bool,
    indent: Vec<u16>,
    /// Whether `indent` holds a unit of 256 or above.
    indent_wide: bool,
    out: Output,
    stack: Vec<Level>,
    open: HashSet<Gc<Object>>,
    to_json: PropertyKey,
    raw_json: PropertyKey,
    key_cache: Vec<Option<CachedKey>>,
    /// The deepest nesting allowed ([`MAX_DEPTH`] or [`MAX_DEPTH_SLOW`]).
    max_depth: usize,
}

/// `JSON.stringify(value, replacer, space)` (§25.5.2).
pub(super) fn stringify(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    let replacer = rt.arg(call, 1);
    let space = rt.arg(call, 2);
    let (replacer_function, list) = read_replacer(rt, replacer)?;
    let gap = read_space(rt, space)?;
    let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
    let wrapper = rt.heap.new_object(Some(proto))?;
    rt.heap.record(wrapper);
    let empty = PropertyKey::String(rt.vm.atoms.empty);
    rt.create_data_property_or_throw(wrapper, empty, value)?;
    let to_json = rt.heap.key_from_str("toJSON")?;
    rt.heap.record(to_json);
    let raw_json = rt.heap.key_from_str("rawJSON")?;
    rt.heap.record(raw_json);
    let pretty = gap.is_some();
    let gap = gap.unwrap_or_default();
    let max_depth = if pretty || replacer_function.is_some() || list.is_some() {
        MAX_DEPTH_SLOW
    } else {
        MAX_DEPTH
    };
    let mut serializer = Serializer {
        replacer: replacer_function,
        list,
        pretty,
        indent_wide: false,
        gap,
        indent: Vec::new(),
        out: Output::new(),
        stack: Vec::new(),
        open: HashSet::new(),
        to_json,
        raw_json,
        key_cache: (0..KEY_CACHE_SIZE).map(|_| None).collect(),
        max_depth,
    };
    if !serializer.run(rt, wrapper, empty)? {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    let text = serializer.out.finish(rt)?;
    Ok(NativeReturn::Value(Value::String(text)))
}

/// Steps 4 and 5 of §25.5.2: a function is the replacer; an array is the
/// property list (`ToString` of the strings, numbers and wrapper objects
/// in it, each once).
fn read_replacer(rt: &mut Runtime, replacer: Value) -> VmResult<(Option<Value>, PropertyList)> {
    let Value::Object(object) = replacer else {
        return Ok((None, None));
    };
    if rt.is_callable(replacer)? {
        return Ok((Some(replacer), None));
    }
    if !matches!(rt.heap.object(object)?.kind, ObjectKind::Array { .. }) {
        return Ok((None, None));
    }
    let length = rt.length_of_array_like(object)?;
    let mut keys: Vec<PropertyKey> = Vec::new();
    let mut seen: HashSet<PropertyKey> = HashSet::new();
    let mut index = 0.0;
    while index < length {
        rt.tick()?;
        let scope = rt.heap.open_scope();
        let key = rt.index_key(index)?;
        let element = rt.get_value(object.into(), key)?;
        let item = match element {
            Value::String(string) => Some(string),
            Value::Int(_) | Value::Double(_) => Some(rt.to_string(element)?),
            Value::Object(wrapper) => match rt.heap.object(wrapper)?.kind {
                ObjectKind::StringWrapper(_) | ObjectKind::NumberWrapper(_) => {
                    Some(rt.to_string(element)?)
                }
                _ => None,
            },
            _ => None,
        };
        let item = match item {
            Some(item) => {
                let key = rt.string_key(item)?;
                seen.insert(key).then_some(key)
            }
            None => None,
        };
        rt.heap.close_scope(scope)?;
        if let Some(key) = item {
            keys.push(key);
            // The list outlives the scope of the step.
            rt.heap.record(key);
        }
        index += 1.0;
    }
    Ok((None, Some(Rc::new(keys))))
}

/// Steps 6 to 9 of §25.5.2: the indentation unit. `None` is no
/// indentation; a gap of no units but line breaks comes from a `space`
/// between 0 and 1 (V8).
fn read_space(rt: &mut Runtime, space: Value) -> VmResult<Option<Vec<u16>>> {
    let mut space = space;
    if let Value::Object(object) = space {
        match rt.heap.object(object)?.kind {
            ObjectKind::NumberWrapper(_) => space = Value::number(rt.to_number(space)?),
            ObjectKind::StringWrapper(_) => {
                let string = rt.to_string(space)?;
                rt.heap.record(string);
                space = Value::String(string);
            }
            _ => {}
        }
    }
    match space {
        Value::Int(_) | Value::Double(_) => {
            let number = space.as_number().unwrap_or(f64::NAN);
            let count = to_integer_or_infinity(number).clamp(0.0, MAX_GAP as f64) as usize;
            if count == 0 && !(number > 0.0 && number < 1.0) {
                return Ok(None);
            }
            Ok(Some(vec![0x20; count]))
        }
        Value::String(string) => {
            let units = rt.flat_text(string)?;
            let count = units.len().min(MAX_GAP);
            if count == 0 {
                return Ok(None);
            }
            Ok(Some(
                units
                    .slice(0, count)
                    .map(|s| s.units().collect())
                    .unwrap_or_default(),
            ))
        }
        _ => Ok(None),
    }
}

/// The value of an own data property of an ordinary object or an array,
/// read without running code: `None` if the property is anything else.
fn fast_get(rt: &Runtime, holder: Gc<Object>, key: PropertyKey) -> Option<Value> {
    let object = rt.heap.object(holder).ok()?;
    if !matches!(object.kind, ObjectKind::Ordinary | ObjectKind::Array { .. }) {
        return None;
    }
    match key {
        PropertyKey::Index(index) => {
            let values = rt.heap.dense_elements(holder).ok()??;
            let value = *values.get(index as usize)?;
            (!value.is_empty() && !matches!(value, Value::Cell(_))).then_some(value)
        }
        PropertyKey::String(_) => match rt.heap.get_own_property(holder, key).ok()?? {
            Property::Data { value, .. } => Some(value),
            Property::Accessor { .. } => None,
        },
        PropertyKey::Symbol(_) => None,
    }
}

/// What a primitive serializes to without any call (no `toJSON` for a
/// primitive other than a `BigInt`); `None` for an object or a `BigInt`.
fn primitive(value: Value) -> Option<Resolved> {
    Some(match value {
        Value::Null => Resolved::Null,
        Value::Bool(flag) => Resolved::Bool(flag),
        Value::String(string) => Resolved::String(string),
        Value::Int(_) | Value::Double(_) => Resolved::Number(value.as_number().unwrap_or(0.0)),
        Value::Undefined | Value::Symbol(_) => Resolved::Undefined,
        Value::BigInt(_) | Value::Object(_) | Value::Empty | Value::Cell(_) => return None,
    })
}

impl Serializer {
    /// Serializes `holder[key]` and everything below it. Returns whether
    /// anything was written (`false` for `undefined`).
    fn run(&mut self, rt: &mut Runtime, holder: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        let scope = rt.heap.open_scope();
        let resolved = self.resolve(rt, holder, key, None)?;
        if matches!(resolved, Resolved::Undefined) {
            rt.heap.close_scope(scope)?;
            return Ok(false);
        }
        self.emit(rt, resolved, key, false, Some(scope))?;
        while !self.stack.is_empty() {
            self.step(rt)?;
        }
        Ok(true)
    }

    /// The next member of the innermost open container, or its end.
    fn step(&mut self, rt: &mut Runtime) -> VmResult<()> {
        rt.tick()?;
        let Some(level) = self.stack.last_mut() else {
            return Ok(());
        };
        let holder = level.object;
        let (key, is_array) = match &mut level.members {
            Members::Elements { length, next } => {
                if *next >= *length {
                    return self.close(rt);
                }
                let key = PropertyKey::Index(*next);
                *next += 1;
                (key, true)
            }
            Members::Properties { keys, next } => {
                let Some(key) = keys.get(*next) else {
                    return self.close(rt);
                };
                *next += 1;
                (key, false)
            }
        };
        // A primitive in a plain data property needs no scope.
        let read = fast_get(rt, holder, key);
        let simple = match read {
            Some(value) if self.replacer.is_none() => primitive(value),
            _ => None,
        };
        let (resolved, scope) = if let Some(resolved) = simple {
            (resolved, None)
        } else {
            let scope = rt.heap.open_scope();
            (self.resolve(rt, holder, key, read)?, Some(scope))
        };
        if is_array {
            self.member_start(rt, None)?;
            if matches!(resolved, Resolved::Undefined) {
                self.out.check(rt, 4)?;
                self.out.ascii(b"null");
                self.end_member();
                return Self::close_member_scope(rt, scope);
            }
        } else {
            if matches!(resolved, Resolved::Undefined) {
                return Self::close_member_scope(rt, scope);
            }
            self.member_start(rt, Some(key))?;
        }
        self.end_member();
        self.emit(rt, resolved, key, is_array, scope)
    }

    fn close_member_scope(rt: &mut Runtime, scope: Option<HandleScope>) -> VmResult<()> {
        match scope {
            Some(scope) => Ok(rt.heap.close_scope(scope)?),
            None => Ok(()),
        }
    }

    /// Counts a member of the innermost container as written.
    fn end_member(&mut self) {
        if let Some(level) = self.stack.last_mut() {
            level.written += 1;
        }
    }

    /// Writes what comes before a member: the `,` (not before the first),
    /// the line break and indentation of the pretty form, and for a
    /// property its quoted key and the colon.
    fn member_start(&mut self, rt: &mut Runtime, key: Option<PropertyKey>) -> VmResult<()> {
        let first = self.stack.last().is_none_or(|level| level.written == 0);
        self.out.check(rt, 2 + self.indent.len())?;
        if !first {
            self.out.byte(b',');
        }
        self.line_break();
        let Some(key) = key else {
            return Ok(());
        };
        match key {
            PropertyKey::String(atom) => self.quoted_key(rt, atom)?,
            PropertyKey::Index(index) => {
                let mut buffer = [0u8; 20];
                let digits = format_integer(i64::from(index), &mut buffer);
                self.out.check(rt, 24)?;
                self.out.byte(b'"');
                self.out.ascii(digits);
                self.out.ascii(if self.pretty { b"\": " } else { b"\":" });
            }
            PropertyKey::Symbol(_) => {
                return Err(VmError::invariant("a symbol key in JSON.stringify"));
            }
        }
        Ok(())
    }

    /// A line break and the indentation (the room is checked).
    fn line_break(&mut self) {
        if !self.pretty {
            return;
        }
        self.out.byte(b'\n');
        if self.indent_wide {
            self.out.units(Str16::Wide(&self.indent));
        } else {
            for &unit in &self.indent {
                self.out.byte(unit as u8);
            }
        }
    }

    /// The quoted key of an atom and the colon, from the cache when the
    /// key is there.
    fn quoted_key(&mut self, rt: &mut Runtime, atom: Gc<JsString>) -> VmResult<()> {
        let slot = atom.index() as usize % KEY_CACHE_SIZE;
        if let Some(Some(cached)) = self.key_cache.get(slot)
            && cached.atom == atom
        {
            self.out.check(rt, cached.text.len())?;
            self.out.ascii(&cached.text);
            return Ok(());
        }
        let before = self.out.len();
        self.quote(rt, atom)?;
        self.out.check(rt, 2)?;
        self.out.ascii(if self.pretty { b": " } else { b":" });
        // Cache the key if it was written in the narrow width.
        if !self.out.is_wide
            && let Some(written) = self.out.narrow.get(before..)
            && let Some(entry) = self.key_cache.get_mut(slot)
        {
            *entry = Some(CachedKey {
                atom,
                text: written.to_vec(),
            });
        }
        Ok(())
    }

    /// Writes a scalar, or opens the container (`scope` becomes the scope
    /// of its level; a scalar closes it).
    fn emit(
        &mut self,
        rt: &mut Runtime,
        resolved: Resolved,
        key: PropertyKey,
        in_array: bool,
        scope: Option<HandleScope>,
    ) -> VmResult<()> {
        match resolved {
            Resolved::Container(object) => {
                let Some(scope) = scope else {
                    return Err(VmError::invariant("a container without a scope"));
                };
                return self.open(rt, object, key, in_array, scope);
            }
            Resolved::Undefined => {}
            Resolved::Null => {
                self.out.check(rt, 4)?;
                self.out.ascii(b"null");
            }
            Resolved::Bool(true) => {
                self.out.check(rt, 4)?;
                self.out.ascii(b"true");
            }
            Resolved::Bool(false) => {
                self.out.check(rt, 5)?;
                self.out.ascii(b"false");
            }
            Resolved::Number(number) => self.number(rt, number)?,
            Resolved::String(string) => self.quote(rt, string)?,
            Resolved::Raw(string) => self.raw(rt, string)?,
        }
        Self::close_member_scope(rt, scope)
    }

    fn number(&mut self, rt: &mut Runtime, number: f64) -> VmResult<()> {
        self.out.check(rt, 24)?;
        if !number.is_finite() {
            self.out.ascii(b"null");
        } else if number == number.trunc() && number.abs() < 1e15 {
            let mut buffer = [0u8; 20];
            let digits = format_integer(number as i64, &mut buffer);
            self.out.ascii(digits);
        } else if let Some((digits, count)) = short_decimal(number) {
            self.out.ascii(digits.get(..count).unwrap_or(&[]));
        } else {
            let text = crate::vm::number::number_to_string(number);
            self.out.ascii(text.as_bytes());
        }
        Ok(())
    }

    /// The text of a raw JSON object, as it is.
    fn raw(&mut self, rt: &mut Runtime, string: Gc<JsString>) -> VmResult<()> {
        rt.flatten(string)?;
        let length = rt.heap.text(string)?.len();
        self.out.check(rt, length)?;
        self.out.units(rt.heap.text(string)?);
        rt.charge_units(length)
    }

    /// `QuoteJSONString` (§25.5.2.3) of a string value: the escapes of the
    /// specification, and `\uXXXX` (lower case) for a lone surrogate.
    fn quote(&mut self, rt: &mut Runtime, string: Gc<JsString>) -> VmResult<()> {
        rt.flatten(string)?;
        let text = rt.heap.text(string)?;
        let length = text.len();
        let extra = escaped_length(text);
        self.out.check(rt, length + extra + 2)?;
        let text = rt.heap.text(string)?;
        self.out.byte(b'"');
        if extra == 0 {
            self.out.units(text);
        } else {
            quote_units(&mut self.out, text);
        }
        self.out.byte(b'"');
        rt.charge_units(length)
    }

    /// `SerializeJSONProperty` (§25.5.2.2) up to the point where the
    /// result is known: steps 1 to 4 (`Get`, `toJSON`, the replacer, the
    /// wrapper objects) and the dispatch of steps 5 to 10. `read` is the
    /// value of the property if the caller has read it. The values that
    /// it returns are recorded in the current handle scope.
    fn resolve(
        &mut self,
        rt: &mut Runtime,
        holder: Gc<Object>,
        key: PropertyKey,
        read: Option<Value>,
    ) -> VmResult<Resolved> {
        let mut value = match read {
            Some(value) => rt.heap.record(value),
            None => rt.get_value(holder.into(), key)?,
        };
        let mut key_value: Option<Value> = None;
        if matches!(value, Value::Object(_) | Value::BigInt(_)) {
            let to_json = rt.get_value(value, self.to_json)?;
            if rt.is_callable(to_json)? {
                let name = key_string(rt, key, &mut key_value)?;
                value = rt.call(to_json, value, &[name])?;
            }
        }
        if let Some(replacer) = self.replacer {
            let name = key_string(rt, key, &mut key_value)?;
            value = rt.call(replacer, holder.into(), &[name, value])?;
        }
        if let Value::Object(object) = value {
            match rt.heap.object(object)?.kind {
                ObjectKind::NumberWrapper(_) => value = Value::number(rt.to_number(value)?),
                ObjectKind::StringWrapper(_) => {
                    let string = rt.to_string(value)?;
                    value = Value::String(rt.heap.record(string));
                }
                ObjectKind::BooleanWrapper(flag) => value = Value::Bool(flag),
                ObjectKind::RawJson => {
                    return match rt.get_value(value, self.raw_json)? {
                        Value::String(text) => Ok(Resolved::Raw(text)),
                        _ => Err(VmError::invariant("a raw JSON object without its text")),
                    };
                }
                _ => {}
            }
        }
        if let Some(resolved) = primitive(value) {
            return Ok(resolved);
        }
        match value {
            Value::BigInt(_) => Err(VmError::type_error(BIGINT_MESSAGE)),
            Value::Object(object) if !rt.is_callable(value)? => Ok(Resolved::Container(object)),
            _ => Ok(Resolved::Undefined),
        }
    }

    /// Opens an array or object (§25.5.2.5, §25.5.2.6, steps 1 to 6):
    /// the cycle check, the indentation, the opening bracket and, for an
    /// object, the list of keys.
    fn open(
        &mut self,
        rt: &mut Runtime,
        object: Gc<Object>,
        key: PropertyKey,
        in_array: bool,
        scope: HandleScope,
    ) -> VmResult<()> {
        if self.is_open(object) {
            return Err(self.circular(rt, object, key));
        }
        if self.stack.len() >= self.max_depth {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let is_array = matches!(rt.heap.object(object)?.kind, ObjectKind::Array { .. });
        let members = if is_array {
            let (length, _) = rt.heap.array_length(object)?;
            Members::Elements { length, next: 0 }
        } else if let Some(list) = &self.list {
            Members::Properties {
                keys: Keys::Shared(Rc::clone(list)),
                next: 0,
            }
        } else {
            Members::Properties {
                keys: enumerable_keys(rt, object)?,
                next: 0,
            }
        };
        self.out.check(rt, 1)?;
        self.out.byte(if is_array { b'[' } else { b'{' });
        let indent_outside = self.indent.len();
        self.indent.extend_from_slice(&self.gap);
        self.indent_wide = self.indent.iter().any(|&unit| unit > 0xff);
        self.stack.push(Level {
            object,
            members,
            scope,
            key,
            in_array,
            indent_outside,
            written: 0,
        });
        if self.stack.len() == SET_FROM_DEPTH {
            self.open
                .extend(self.stack.iter().map(|level| level.object));
        } else if self.stack.len() > SET_FROM_DEPTH {
            self.open.insert(object);
        }
        Ok(())
    }

    /// Closes the innermost container: the closing bracket (after a line
    /// break if it has members).
    fn close(&mut self, rt: &mut Runtime) -> VmResult<()> {
        let Some(level) = self.stack.pop() else {
            return Ok(());
        };
        if self.stack.len() + 1 >= SET_FROM_DEPTH {
            self.open.remove(&level.object);
        }
        if self.stack.len() < SET_FROM_DEPTH {
            self.open.clear();
        }
        self.indent.truncate(level.indent_outside);
        self.indent_wide = self.indent.iter().any(|&unit| unit > 0xff);
        self.out.check(rt, 2 + self.indent.len())?;
        if level.written > 0 {
            self.line_break();
        }
        let is_array = matches!(level.members, Members::Elements { .. });
        self.out.byte(if is_array { b']' } else { b'}' });
        Ok(rt.heap.close_scope(level.scope)?)
    }

    /// Whether `object` is one of the open containers.
    fn is_open(&self, object: Gc<Object>) -> bool {
        if self.stack.len() >= SET_FROM_DEPTH {
            self.open.contains(&object)
        } else {
            self.stack.iter().any(|level| level.object == object)
        }
    }

    // --- The message of a cycle ---

    /// The `TypeError` for a cycle that closes at `object`, reached
    /// through `key` (V8's text: where the cycle starts, the objects on
    /// the way, and the property that closes it).
    fn circular(&self, rt: &Runtime, object: Gc<Object>, key: PropertyKey) -> VmError {
        let start = self
            .stack
            .iter()
            .position(|level| level.object == object)
            .unwrap_or(0);
        let path = self.stack.get(start + 1..).unwrap_or(&[]);
        let closing_in_array = self
            .stack
            .last()
            .is_some_and(|level| matches!(level.members, Members::Elements { .. }));
        let mut text = String::from(
            "Converting circular structure to JSON\n    --> starting at object with constructor '",
        );
        text.push_str(&constructor_name(rt, object));
        text.push('\'');
        let line = |level: &Level| {
            format!(
                "\n    |     {} -> object with constructor '{}'",
                key_description(rt, level.key, level.in_array),
                constructor_name(rt, level.object)
            )
        };
        if path.len() > 3 {
            for level in path.iter().take(2) {
                text.push_str(&line(level));
            }
            text.push_str("\n    |     ...");
            if let Some(level) = path.last() {
                text.push_str(&line(level));
            }
        } else {
            for level in path {
                text.push_str(&line(level));
            }
        }
        text.push_str("\n    --- ");
        text.push_str(&key_description(rt, key, closing_in_array));
        text.push_str(" closes the circle");
        VmError::type_error(text)
    }
}

/// `property 'key'`, `index 3` or `<anonymous>` (an empty key).
fn key_description(rt: &Runtime, key: PropertyKey, in_array: bool) -> String {
    match key {
        PropertyKey::Index(index) if in_array => format!("index {index}"),
        _ => {
            let text = rt.key_text(key);
            if text.is_empty() {
                "<anonymous>".to_owned()
            } else {
                format!("property '{text}'")
            }
        }
    }
}

/// The constructor name that a cycle message shows (see the module
/// comment).
fn constructor_name(rt: &Runtime, object: Gc<Object>) -> String {
    let mut name = None;
    let mut current = rt.heap.get_prototype_of(object).ok().flatten();
    for _ in 0..64 {
        let Some(proto) = current else {
            break;
        };
        if let Ok(Some(Property::Data {
            value: Value::Object(constructor),
            ..
        })) = rt
            .heap
            .get_own_property(proto, PropertyKey::String(rt.vm.atoms.constructor))
        {
            let found = function_name(rt, constructor);
            if !found.is_empty() {
                name = Some(found);
                break;
            }
        }
        current = rt.heap.get_prototype_of(proto).ok().flatten();
    }
    if name.as_deref().is_none_or(|name| name == "Object") {
        let tag = rt.symbol_key(WellKnown::ToStringTag);
        if let Some(Value::String(tag)) = rt.chain_data_value_of(object, tag) {
            return rt.clipped_text(tag);
        }
    }
    name.unwrap_or_else(|| "Object".to_owned())
}

/// The name of a function: its own `name` if that is a non-empty
/// string, else the name in its code.
fn function_name(rt: &Runtime, function: Gc<Object>) -> String {
    if let Some(name) = rt.own_data_string(function, rt.vm.atoms.name)
        && !name.is_empty()
    {
        return name;
    }
    match rt.heap.object(function).map(|o| &o.kind) {
        Ok(ObjectKind::Function(closure)) => rt.code_name(&closure.compiled),
        Ok(ObjectKind::Native(native)) => native
            .name
            .map(|name| rt.clipped_text(name))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// The key as the string argument of `toJSON` and the replacer, made
/// once per property and recorded.
fn key_string(rt: &mut Runtime, key: PropertyKey, made: &mut Option<Value>) -> VmResult<Value> {
    if let Some(value) = *made {
        return Ok(value);
    }
    let value = match key {
        PropertyKey::String(_) => key_as_value(key),
        _ => rt.heap.key_to_value(key)?,
    };
    rt.heap.record(value);
    *made = Some(value);
    Ok(value)
}

/// `EnumerableOwnProperties(object, key)` as far as it matters here:
/// the enumerable own string keys, in `[[OwnPropertyKeys]]` order, rooted.
fn enumerable_keys(rt: &mut Runtime, object: Gc<Object>) -> VmResult<Keys> {
    let bound = rt.heap.own_key_bound(object)?;
    rt.charge(bound)?;
    rt.heap
        .check_room(bound.saturating_mul(size_of::<PropertyKey>()))?;
    let mut keys = Vec::new();
    keys.try_reserve(bound).map_err(crate::error::Error::from)?;
    rt.heap.each_string_key(object, |key, enumerable| {
        if enumerable {
            keys.push(key);
        }
    })?;
    rt.heap.record_keys(&keys);
    Ok(Keys::Own(keys))
}

// --- QuoteJSONString ---

/// Whether a unit is written as itself.
#[inline]
fn plain(unit: u16) -> bool {
    unit >= 0x20 && unit != 0x22 && unit != 0x5c && !(0xd800..=0xdfff).contains(&unit)
}

/// The number of units that the escapes add to a string.
fn escaped_length(text: Str16<'_>) -> usize {
    let mut extra = 0;
    match text {
        Str16::Latin1(bytes) => {
            for &byte in bytes {
                if !plain(u16::from(byte)) {
                    extra += escape_size(u16::from(byte)) - 1;
                }
            }
        }
        Str16::Wide(units) => {
            let mut index = 0;
            while let Some(&unit) = units.get(index) {
                if plain(unit) {
                    index += 1;
                } else if swb_js_text::is_lead_surrogate(unit)
                    && units
                        .get(index + 1)
                        .is_some_and(|&next| swb_js_text::is_trail_surrogate(next))
                {
                    index += 2;
                } else {
                    extra += escape_size(unit) - 1;
                    index += 1;
                }
            }
        }
    }
    extra
}

/// The length of the escape of a unit that is not plain.
fn escape_size(unit: u16) -> usize {
    match unit {
        0x08 | 0x09 | 0x0a | 0x0c | 0x0d | 0x22 | 0x5c => 2,
        _ => 6,
    }
}

/// Appends the text with the escapes of `QuoteJSONString`; the caller
/// made room.
fn quote_units(out: &mut Output, text: Str16<'_>) {
    match text {
        Str16::Latin1(bytes) => {
            let mut run = 0;
            for (index, &byte) in bytes.iter().enumerate() {
                if plain(u16::from(byte)) {
                    continue;
                }
                out.ascii(bytes.get(run..index).unwrap_or(&[]));
                escape_unit(out, u16::from(byte));
                run = index + 1;
            }
            out.ascii(bytes.get(run..).unwrap_or(&[]));
        }
        Str16::Wide(units) => {
            let mut run = 0;
            let mut index = 0;
            while let Some(&unit) = units.get(index) {
                if plain(unit) {
                    index += 1;
                } else if swb_js_text::is_lead_surrogate(unit)
                    && units
                        .get(index + 1)
                        .is_some_and(|&next| swb_js_text::is_trail_surrogate(next))
                {
                    index += 2;
                } else {
                    out.units(Str16::Wide(units.get(run..index).unwrap_or(&[])));
                    escape_unit(out, unit);
                    index += 1;
                    run = index;
                }
            }
            out.units(Str16::Wide(units.get(run..).unwrap_or(&[])));
        }
    }
}

/// The digits of the `\\uXXXX` escapes (lower case).
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Appends the escape of one unit that is not plain.
fn escape_unit(out: &mut Output, unit: u16) {
    let short = match unit {
        0x08 => Some(b'b'),
        0x09 => Some(b't'),
        0x0a => Some(b'n'),
        0x0c => Some(b'f'),
        0x0d => Some(b'r'),
        0x22 => Some(b'"'),
        0x5c => Some(b'\\'),
        _ => None,
    };
    if let Some(letter) = short {
        out.byte(b'\\');
        out.byte(letter);
        return;
    }
    out.ascii(b"\\u");
    for shift in [12, 8, 4, 0] {
        out.byte(HEX[usize::from(unit >> shift & 0xf)]);
    }
}

/// The decimal powers that are exact doubles.
const POWERS_OF_TEN: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// The text of `Number::toString(number)` for a number that is not an
/// integer and has a decimal form of at most 15 digits, in the range of
/// the fixed notation (from 10^-6 to 10^15): the common prices and
/// coordinates, without the general digit generation of `js-text`, which
/// takes three formatting passes. The smallest `k` for which
/// `round(x * 10^k) / 10^k == x` gives the shortest digits; the integer
/// below 10^15 and the power of ten are exact doubles, so the division is
/// one correctly rounded operation (the decimal reads back as `x`), and
/// at most 15 digits identify a double, so the digits are the only
/// shortest ones. `None` for any other number.
fn short_decimal(number: f64) -> Option<([u8; 32], usize)> {
    let x = number.abs();
    if !(1e-6..1e15).contains(&x) || x == x.trunc() {
        return None;
    }
    for (k, &power) in POWERS_OF_TEN.iter().enumerate().skip(1) {
        let scaled = x * power;
        if scaled >= 1e15 {
            return None;
        }
        let y = scaled.round();
        if y >= 1.0 && y / power == x {
            let mut buffer = [0u8; 20];
            let digits = format_integer(y as i64, &mut buffer);
            let mut out = [0u8; 32];
            let mut length = 0;
            let mut push = |byte: u8| {
                if let Some(slot) = out.get_mut(length) {
                    *slot = byte;
                    length += 1;
                }
            };
            if number < 0.0 {
                push(b'-');
            }
            if digits.len() > k {
                let (whole, fraction) = digits.split_at(digits.len() - k);
                for &byte in whole {
                    push(byte);
                }
                push(b'.');
                for &byte in fraction {
                    push(byte);
                }
            } else {
                push(b'0');
                push(b'.');
                for _ in digits.len()..k {
                    push(b'0');
                }
                for &byte in digits {
                    push(byte);
                }
            }
            return Some((out, length));
        }
    }
    None
}

/// The decimal digits of an integer, written at the end of `buffer`.
fn format_integer(value: i64, buffer: &mut [u8; 20]) -> &[u8] {
    let mut position = buffer.len();
    let mut rest = value.unsigned_abs();
    loop {
        position -= 1;
        if let Some(slot) = buffer.get_mut(position) {
            *slot = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 && position > 0 {
        position -= 1;
        if let Some(slot) = buffer.get_mut(position) {
            *slot = b'-';
        }
    }
    buffer.get(position..).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(number: f64) -> Option<String> {
        short_decimal(number).map(|(buffer, length)| {
            String::from_utf8_lossy(buffer.get(..length).unwrap_or(&[])).into_owned()
        })
    }

    /// The fast path gives `Number::toString` of `js-text` whenever it
    /// answers.
    #[test]
    fn short_decimals_equal_number_to_string() {
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut answered = 0;
        for round in 0..400_000 {
            let bits = next();
            let number = match round % 4 {
                // A decimal with few digits.
                0 => (next() % 10_000_000) as f64 / 10f64.powi((next() % 9) as i32),
                // A decimal with many digits (mostly out of reach).
                1 => (next() % 1_000_000_000_000_000_000) as f64 / 10f64.powi((next() % 22) as i32),
                // A sum of two decimals.
                2 => (next() % 100_000) as f64 / 100.0 + (next() % 1000) as f64 / 10.0,
                // Any bit pattern.
                _ => f64::from_bits(bits),
            };
            for number in [number, -number] {
                if !number.is_finite() {
                    continue;
                }
                if let Some(text) = short(number) {
                    answered += 1;
                    assert_eq!(
                        text,
                        crate::vm::number::number_to_string(number),
                        "{number:e}"
                    );
                }
            }
        }
        assert!(answered > 300_000, "{answered}");
    }

    #[test]
    fn short_decimals_edges() {
        assert_eq!(short(0.5).as_deref(), Some("0.5"));
        assert_eq!(short(-0.000_001).as_deref(), Some("-0.000001"));
        assert_eq!(short(0.000_000_1), None);
        assert_eq!(short(0.1 + 0.2), None);
        assert_eq!(short(1.0), None);
        assert_eq!(short(123_456_789_012.5).as_deref(), Some("123456789012.5"));
        assert_eq!(short(4.35).as_deref(), Some("4.35"));
        assert_eq!(short(1e15 + 0.5), None);
    }
}
