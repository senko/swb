//! `console.log` (WHATWG Console Standard, "Logger" with the formatting of
//! Node.js, measured as a black box): the arguments joined by spaces,
//! written as one line to the host's console sink.
//!
//! Strings print raw; other values print in Node.js's short form
//! (`util.inspect` defaults): `[ 1, 2, 3 ]`, `{ a: 1, b: 'x' }`,
//! `[Function: f]`, `[Number: 5]`, `Map(1) { 1 => 2 }`, `Set(2) { 1, 2 }`,
//! `<ref *1> { self: [Circular *1] }`. The iterators of `Map` and `Set` print as
//! ordinary objects (`Object [Map Iterator] {}`; Node.js lists their entries).
//! Formatting never runs script code: properties are read without
//! calling getters (`[Getter]`), and no `toString` runs. Bounds: nesting
//! depth 2 (deeper objects print as `[Object]` and `[Array]`), the first
//! 100 elements or properties of an object, cycles.
//!
//! Deviations from Node.js: no format specifiers (`%s`) in the first
//! argument; arrays of more than six elements are not grouped into
//! columns; objects list at most 100 properties; property names longer
//! than 10,000 code units are cut like string values (Node.js prints
//! them whole, so 100 keys of 1M units would print 105 MB); once the
//! text of one call passes 4M units, the rest of the structure prints as
//! `...` (Node.js has no such limit and would use gigabytes for a nest
//! of 100 x 100 x 100 long strings).
//!
//! Time: the formatter counts the keys it lists and the units it writes;
//! `console.log` charges them to the time countdown after the call, so a
//! loop of calls reaches the time check.

use std::fmt::Write;

#[cfg(test)]
use swb_js_text::Str16;

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmResult, WellKnown};

/// The deepest level whose objects are shown (Node.js's default depth).
const MAX_DEPTH: usize = 2;

/// The most elements of an array (and properties of an object) shown.
const MAX_ENTRIES: usize = 100;

/// The most code units of formatted text per call before the rest of a
/// structure prints as `...`.
const MAX_OUTPUT: usize = 4 << 20;

/// The longest line of an object or array on one line (measured in
/// Node.js 22, at nesting level 0; two less per level).
const LINE_WIDTH: usize = 71;

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let console = rt.heap.new_object(Some(proto))?;
    rt.heap.record(console);
    super::method(rt, realm, console, "log", 0, log)?;
    super::global(rt, realm, "console", console.into())
}

/// `console.log(...data)`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn log(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut line = String::new();
    let mut steps = 0;
    for i in 0..call.argc() {
        if i > 0 {
            line.push(' ');
        }
        match rt.arg(call, i) {
            Value::String(string) => line.push_str(&rt.name_text(string)),
            value => {
                let (text, work) = inspect(rt, value);
                line.push_str(&text);
                steps += work;
            }
        }
    }
    steps += line.len() / crate::vm::time::UNITS_PER_STEP;
    rt.console_line(&line);
    rt.charge(steps)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// A Number as `console.log` shows it: like `ToString`, but `-0` for
/// negative zero.
fn format_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        "-0".to_owned()
    } else {
        crate::vm::number::number_to_string(n)
    }
}

/// The text of a value as `console.log` shows it inside a structure
/// (strings quoted), and the work it did, in steps of the time countdown
/// (one per key listed or element visited, one per 64 units written).
fn inspect(rt: &Runtime, value: Value) -> (String, usize) {
    let mut inspector = Inspector {
        rt,
        stack: Vec::new(),
        circular: Vec::new(),
        steps: 0,
        units: 0,
    };
    let text = inspector.value(value, 0, 0);
    let work = inspector.steps + inspector.units / crate::vm::time::UNITS_PER_STEP;
    (text, work)
}

struct Inspector<'a> {
    rt: &'a Runtime,
    /// The objects being formatted (for cycles).
    stack: Vec<Gc<Object>>,
    /// The objects that a cycle refers to, in the order of their numbers.
    circular: Vec<Gc<Object>>,
    /// Keys listed and elements visited.
    steps: usize,
    /// Code units of text produced for strings and keys.
    units: usize,
}

impl Inspector<'_> {
    /// The text of a value at nesting `depth`, indented by `indent`.
    fn value(&mut self, value: Value, depth: usize, indent: usize) -> String {
        if self.units > MAX_OUTPUT {
            return "...".to_owned();
        }
        self.steps += 1;
        match value {
            Value::String(string) => {
                let quoted = quote_string(&self.rt.heap, string);
                self.units += quoted.len();
                quoted
            }
            Value::Double(d) => format_number(d),
            Value::Object(object) => self.object(object, depth, indent),
            _ => self.rt.primitive_text(value),
        }
    }

    fn object(&mut self, object: Gc<Object>, depth: usize, indent: usize) -> String {
        if self.stack.contains(&object) {
            let index = if let Some(index) = self.circular.iter().position(|&o| o == object) {
                index
            } else {
                self.circular.push(object);
                self.circular.len() - 1
            };
            return format!("[Circular *{}]", index + 1);
        }
        let Ok(record) = self.rt.heap.object(object) else {
            return "[Object]".to_owned();
        };
        let is_array = matches!(record.kind, ObjectKind::Array { .. });
        let collection = match record.kind {
            ObjectKind::Map(_) => Some("Map"),
            ObjectKind::Set(_) => Some("Set"),
            _ => None,
        };
        let base = self
            .base(object, &record.kind)
            .map(|base| self.with_function_tag(object, &record.kind, base));
        let entries_shown = depth <= MAX_DEPTH;
        if !entries_shown {
            if let Some(base) = base {
                return base;
            }
            return match (is_array, collection) {
                (true, _) => "[Array]".to_owned(),
                (_, Some(name)) => format!("[{name}]"),
                _ => "[Object]".to_owned(),
            };
        }
        self.stack.push(object);
        let entries = if is_array {
            self.array_entries(object, depth, indent)
        } else if collection.is_some() {
            self.collection_entries(object, depth, indent)
        } else {
            self.property_entries(object, depth, indent, false)
        };
        self.stack.pop();
        let prefix = self.prefix(object, &record.kind);
        let mut text = match (base, entries.is_empty()) {
            (Some(base), true) => base,
            (base, _) => {
                let (open, close) = if is_array { ("[", "]") } else { ("{", "}") };
                let mut start = String::new();
                if let Some(base) = &base {
                    start.push_str(base);
                    start.push(' ');
                }
                start.push_str(&prefix);
                start.push_str(open);
                if entries.is_empty() {
                    format!("{start}{close}")
                } else {
                    let single = format!("{start} {} {close}", entries.join(", "));
                    let limit = LINE_WIDTH + usize::from(base.is_some());
                    if !single.contains('\n') && indent + single.chars().count() <= limit {
                        single
                    } else {
                        let pad = " ".repeat(indent + 2);
                        let inner = entries.join(&format!(",\n{pad}"));
                        format!("{start}\n{pad}{inner}\n{}{close}", " ".repeat(indent))
                    }
                }
            }
        };
        if let Some(index) = self.circular.iter().position(|&o| o == object) {
            text = format!("<ref *{}> {text}", index + 1);
        }
        text
    }

    /// The text that replaces the braces of an object, or stands before
    /// them: functions, boxed primitives, errors.
    fn base(&self, object: Gc<Object>, kind: &ObjectKind) -> Option<String> {
        let rt = self.rt;
        // Node.js shows the prototype object of a primitive wrapper as an
        // ordinary object (`{}`), not as a wrapper.
        if matches!(
            kind,
            ObjectKind::NumberWrapper(_)
                | ObjectKind::BooleanWrapper(_)
                | ObjectKind::StringWrapper(_)
                | ObjectKind::SymbolWrapper(_)
        ) && rt.is_wrapper_prototype(object)
        {
            return None;
        }
        Some(match kind {
            // Node.js shows a class with the name of its parent, if that
            // has one.
            ObjectKind::Function(closure) if closure.compiled.kind.is_class_constructor() => {
                let name = rt
                    .own_data_string(object, rt.vm.atoms.name)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "(anonymous)".to_owned());
                let parent = rt
                    .heap
                    .get_prototype_of(object)
                    .ok()
                    .flatten()
                    .filter(|&parent| rt.is_callable(parent.into()).unwrap_or(false))
                    .and_then(|parent| rt.own_data_string(parent, rt.vm.atoms.name))
                    .filter(|name| !name.is_empty());
                match parent {
                    Some(parent) => format!("[class {name} extends {parent}]"),
                    None => format!("[class {name}]"),
                }
            }
            ObjectKind::Function(closure) => {
                let kind = match closure.compiled.kind {
                    crate::bytecode::CodeKind::Generator => "GeneratorFunction",
                    crate::bytecode::CodeKind::Async => "AsyncFunction",
                    crate::bytecode::CodeKind::AsyncGenerator => "AsyncGeneratorFunction",
                    _ => "Function",
                };
                match rt.own_data_string(object, rt.vm.atoms.name) {
                    Some(name) if !name.is_empty() => format!("[{kind}: {name}]"),
                    _ => format!("[{kind} (anonymous)]"),
                }
            }
            ObjectKind::Native(_) | ObjectKind::Bound(_) => {
                match rt.own_data_string(object, rt.vm.atoms.name) {
                    Some(name) if !name.is_empty() => format!("[Function: {name}]"),
                    _ => "[Function (anonymous)]".to_owned(),
                }
            }
            ObjectKind::NumberWrapper(n) => format!("[Number: {}]", format_number(*n)),
            ObjectKind::BooleanWrapper(b) => format!("[Boolean: {b}]"),
            ObjectKind::StringWrapper(s) => format!("[String: {}]", quote_string(&rt.heap, *s)),
            ObjectKind::SymbolWrapper(s) => {
                format!("[Symbol: {}]", rt.key_text(PropertyKey::Symbol(*s)))
            }
            ObjectKind::Error(_) => {
                // Without a `stack` property (M7), Node.js shows
                // `[Name: message]`.
                let name = rt
                    .chain_data_string(object, rt.vm.atoms.name)
                    .unwrap_or_else(|| "Error".to_owned());
                match rt.chain_data_string(object, rt.vm.atoms.message) {
                    Some(message) if !message.is_empty() => format!("[{name}: {message}]"),
                    _ => format!("[{name}]"),
                }
            }
            _ => return None,
        })
    }

    /// A function's base text with its `@@toStringTag` (`[Function: f]
    /// [Q]`), when the tag differs from the constructor name.
    fn with_function_tag(&self, object: Gc<Object>, kind: &ObjectKind, base: String) -> String {
        if !matches!(
            kind,
            ObjectKind::Function(_) | ObjectKind::Native(_) | ObjectKind::Bound(_)
        ) {
            return base;
        }
        let Some(tag) = self.string_tag(object) else {
            return base;
        };
        let constructor = match self.rt.heap.get_prototype_of(object) {
            Ok(Some(proto)) => self.rt.chain_constructor_name(proto),
            _ => None,
        };
        if constructor.as_deref() == Some(tag.as_str()) || base.starts_with(&format!("[{tag}")) {
            base
        } else {
            format!("{base} [{tag}]")
        }
    }

    /// The text before the braces of an ordinary object: `Foo ` for an
    /// object whose constructor is not `Object`, `[Object: null
    /// prototype] `, `Object [Generator] `, `[Arguments] `, and
    /// `Object [Tag] ` for an object with a `@@toStringTag` (Node.js omits
    /// the tag when it is an enumerable own property, because the
    /// property is listed anyway).
    fn prefix(&self, object: Gc<Object>, kind: &ObjectKind) -> String {
        match kind {
            ObjectKind::Generator(state) => {
                return match state.kind {
                    crate::vm::generator::GeneratorKind::AsyncGenerator(_) => {
                        "Object [AsyncGenerator] ".to_owned()
                    }
                    _ => "Object [Generator] ".to_owned(),
                };
            }
            ObjectKind::Arguments => return "[Arguments] ".to_owned(),
            // `Map(2) `, `Set(1) `, and `Foo(1) [Map] ` for a subclass.
            ObjectKind::Map(table) | ObjectKind::Set(table) => {
                let builtin = if matches!(kind, ObjectKind::Map(_)) {
                    "Map"
                } else {
                    "Set"
                };
                let name = match self.rt.heap.get_prototype_of(object) {
                    Ok(Some(proto)) => self.rt.chain_constructor_name(proto),
                    _ => None,
                }
                .filter(|name| !name.is_empty());
                return match name {
                    Some(name) if name != builtin => {
                        format!("{name}({}) [{builtin}] ", table.len())
                    }
                    _ => format!("{builtin}({}) ", table.len()),
                };
            }
            ObjectKind::Ordinary
            | ObjectKind::Array { .. }
            | ObjectKind::ArrayIterator(_)
            | ObjectKind::StringIterator(_) => {}
            _ => return String::new(),
        }
        let rt = self.rt;
        let tag = self.string_tag(object);
        // An array names its length: `Array(1)`, `A(2)`.
        let sized = |name: &str| match kind {
            ObjectKind::Array { .. } => {
                let length = rt.heap.array_length(object).map_or(0, |(l, _)| l);
                format!("{name}({length})")
            }
            _ => name.to_owned(),
        };
        let default = if matches!(kind, ObjectKind::Array { .. }) {
            "Array"
        } else {
            "Object"
        };
        let Ok(Some(proto)) = rt.heap.get_prototype_of(object) else {
            let start = format!("[{}: null prototype] ", sized(default));
            return tag.map_or_else(|| start.clone(), |tag| format!("{start}[{tag}] "));
        };
        match (rt.chain_constructor_name(proto), tag) {
            (Some(name), Some(tag)) if !name.is_empty() && name != tag => {
                format!("{} [{tag}] ", sized(&name))
            }
            (Some(name), _) if name != default && !name.is_empty() => {
                format!("{} ", sized(&name))
            }
            _ => String::new(),
        }
    }

    /// The `@@toStringTag` of an object as a plain data property that is
    /// not an enumerable own property (no getter runs).
    fn string_tag(&self, object: Gc<Object>) -> Option<String> {
        let key = self.rt.symbol_key(WellKnown::ToStringTag);
        if let Ok(Some(Property::Data {
            enumerable: true, ..
        })) = self.rt.heap.get_own_property(object, key)
        {
            return None;
        }
        match self.rt.chain_data_value_of(object, key)? {
            Value::String(tag) => Some(self.rt.name_text(tag)).filter(|tag| !tag.is_empty()),
            _ => None,
        }
    }

    /// The elements of an array (holes as `<N empty items>`), then its
    /// other enumerable properties.
    fn array_entries(&mut self, array: Gc<Object>, depth: usize, indent: usize) -> Vec<String> {
        let length = self.rt.heap.array_length(array).map_or(0, |(l, _)| l);
        let mut entries = Vec::new();
        let mut holes = 0u64;
        let mut index = 0u32;
        let mut shown = 0;
        let flush = |entries: &mut Vec<String>, holes: &mut u64| {
            if *holes > 0 {
                let s = if *holes == 1 { "" } else { "s" };
                entries.push(format!("<{holes} empty item{s}>"));
                *holes = 0;
            }
        };
        while index < length {
            if shown >= MAX_ENTRIES {
                flush(&mut entries, &mut holes);
                let rest = length - index;
                let s = if rest == 1 { "" } else { "s" };
                entries.push(format!("... {rest} more item{s}"));
                break;
            }
            if let Ok(Some(property)) = self
                .rt
                .heap
                .get_own_property(array, PropertyKey::Index(index))
            {
                flush(&mut entries, &mut holes);
                entries.push(self.property_value(&property, depth, indent));
                index += 1;
            } else {
                // A run of holes counts as one entry; skip to the next
                // element without a step per hole.
                let next = self.next_element(array, index, length);
                holes += u64::from(next - index);
                index = next;
            }
            shown += 1;
        }
        flush(&mut entries, &mut holes);
        entries.extend(self.property_entries(array, depth, indent, true));
        entries
    }

    /// The entries of a `Map` (`key => value`) or the elements of a `Set`
    /// in order, at most 100, then the other enumerable properties.
    fn collection_entries(
        &mut self,
        object: Gc<Object>,
        depth: usize,
        indent: usize,
    ) -> Vec<String> {
        let mut entries = Vec::new();
        let is_map = matches!(
            self.rt.heap.object(object).map(|o| &o.kind),
            Ok(ObjectKind::Map(_))
        );
        let Ok(Some(table)) = self.rt.heap.table(object) else {
            return entries;
        };
        let total = table.len();
        let mut position = 0;
        let mut live = Vec::new();
        while let Some(entry) = table.entry(position) {
            position += 1;
            if entry.key.is_empty() {
                continue;
            }
            if live.len() >= MAX_ENTRIES {
                break;
            }
            live.push((entry.key, entry.value));
        }
        self.steps += position;
        for (key, value) in live {
            let key = self.value(key, depth + 1, indent + 2);
            if is_map {
                let value = self.value(value, depth + 1, indent + 2);
                entries.push(format!("{key} => {value}"));
            } else {
                entries.push(key);
            }
        }
        if total > MAX_ENTRIES {
            let rest = total - MAX_ENTRIES;
            let s = if rest == 1 { "" } else { "s" };
            entries.push(format!("... {rest} more item{s}"));
        }
        entries.extend(self.property_entries(object, depth, indent, false));
        entries
    }

    /// The first index at or after `from` (below `length`) that has an
    /// element, or `length`.
    fn next_element(&mut self, array: Gc<Object>, from: u32, length: u32) -> u32 {
        let Ok(keys) = self.rt.heap.own_property_keys(array) else {
            return length;
        };
        self.steps += keys.len();
        keys.iter()
            .filter_map(|key| match key {
                PropertyKey::Index(i) if *i >= from && *i < length => Some(*i),
                _ => None,
            })
            .min()
            .unwrap_or(length)
    }

    /// The enumerable own properties (`key: value`); for arrays only the
    /// keys that are not indices.
    fn property_entries(
        &mut self,
        object: Gc<Object>,
        depth: usize,
        indent: usize,
        skip_indices: bool,
    ) -> Vec<String> {
        let Ok(keys) = self.rt.heap.own_property_keys(object) else {
            return Vec::new();
        };
        self.steps += keys.len();
        let mut entries = Vec::new();
        for key in keys {
            if skip_indices && matches!(key, PropertyKey::Index(_)) {
                continue;
            }
            let Ok(Some(property)) = self.rt.heap.get_own_property(object, key) else {
                continue;
            };
            let enumerable = match property {
                Property::Data { enumerable, .. } | Property::Accessor { enumerable, .. } => {
                    enumerable
                }
            };
            if !enumerable {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                entries.push("...".to_owned());
                break;
            }
            let value = self.property_value(&property, depth, indent);
            let key = self.key(key);
            self.units += key.len();
            entries.push(format!("{key}: {value}"));
        }
        entries
    }

    /// The text of a property's value (accessors are not called).
    fn property_value(&mut self, property: &Property, depth: usize, indent: usize) -> String {
        match *property {
            Property::Data { value, .. } => self.value(value, depth + 1, indent + 2),
            Property::Accessor { get, set, .. } => match (get.is_undefined(), set.is_undefined()) {
                (false, false) => "[Getter/Setter]",
                (false, true) => "[Getter]",
                (true, false) => "[Setter]",
                (true, true) => "undefined",
            }
            .to_owned(),
        }
    }

    /// A property key: unquoted if it is a plain identifier, a symbol in
    /// brackets, otherwise quoted.
    fn key(&self, key: PropertyKey) -> String {
        match key {
            PropertyKey::Symbol(_) => format!("[{}]", self.rt.key_text(key)),
            PropertyKey::Index(index) => format!("'{index}'"),
            PropertyKey::String(name) => {
                let Ok(string) = self.rt.heap.string(name) else {
                    return "''".to_owned();
                };
                // A long name is quoted, which cuts it (see the module
                // documentation).
                if string.len() > MAX_STRING {
                    return quote_string(&self.rt.heap, name);
                }
                let text = self.rt.name_text(name);
                let mut chars = text.chars();
                let plain = chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
                if plain {
                    text
                } else {
                    quote_string(&self.rt.heap, name)
                }
            }
        }
    }
}

/// The longest string shown inside a structure (Node.js's default); the
/// rest is counted.
const MAX_STRING: usize = 10_000;

/// A string in quotes as Node.js shows it: single quotes, or double
/// quotes if the text has a single quote and no double quote, or
/// backquotes if it has both; control characters and lone surrogates
/// escaped; at most [`MAX_STRING`] code units.
#[cfg(test)]
fn quote(text: Str16<'_>) -> String {
    quote_units(text.len(), || text.units())
}

/// [`quote`] of a script string (a rope is read leaf by leaf, without a
/// flatten).
fn quote_string(heap: &crate::heap::Heap, string: Gc<JsString>) -> String {
    let len = heap.string(string).map_or(0, JsString::len);
    quote_units(len, || heap.string_units(string))
}

/// [`quote`] of the `len` code units that `units` gives (each call gives
/// them from the start).
fn quote_units<I: Iterator<Item = u16>>(len: usize, units: impl Fn() -> I) -> String {
    let has = |c: char| units().any(|u| u32::from(u) == c as u32);
    let quote = if !has('\'') {
        '\''
    } else if !has('"') {
        '"'
    } else if !has('`') {
        '`'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    let mut units = units().take(MAX_STRING).peekable();
    while let Some(unit) = units.next() {
        let mut code = u32::from(unit);
        if (0xd800..0xdc00).contains(&code)
            && let Some(&low) = units.peek()
            && (0xdc00..0xe000).contains(&u32::from(low))
        {
            units.next();
            code = 0x10000 + ((code - 0xd800) << 10) + (u32::from(low) - 0xdc00);
        }
        let Some(c) = char::from_u32(code) else {
            // A lone surrogate.
            let _ = write!(out, "\\u{code:04X}");
            continue;
        };
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            '\\' => out.push_str("\\\\"),
            _ if c == quote => {
                out.push('\\');
                out.push(c);
            }
            _ if code < 0x20 || code == 0x7f => {
                let _ = write!(out, "\\x{code:02X}");
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    let rest = len.saturating_sub(MAX_STRING);
    if rest > 0 {
        let s = if rest == 1 { "" } else { "s" };
        let _ = write!(out, "... {rest} more character{s}");
    }
    out
}

#[cfg(test)]
mod tests {
    use swb_js_text::String16;

    use super::quote;

    #[test]
    fn quotes_like_node() {
        let q = |s: &str| quote(String16::from(s).as_str16());
        assert_eq!(q("a\nb"), "'a\\nb'");
        assert_eq!(q("it's"), "\"it's\"");
        assert_eq!(q("both ' and \""), "`both ' and \"`");
        assert_eq!(q("\u{1}\u{7f}"), "'\\x01\\x7F'");
        assert_eq!(q("back\\slash"), "'back\\\\slash'");
        let lone = String16::from_units(&[0x61, 0xd800]);
        assert_eq!(quote(lone.as_str16()), "'a\\uD800'");
        let pair = String16::from_units(&[0xd83d, 0xde00]);
        assert_eq!(quote(pair.as_str16()), "'\u{1f600}'");
    }
}
