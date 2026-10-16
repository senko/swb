//! The JSON text parser of `JSON.parse` and `JSON.rawJSON` (ECMA-262
//! §25.5.1 `JSON.parse`, §25.5.1.1 `ParseJSON`, ECMA-404 for the grammar;
//! the parse records of the source text access proposal,
//! <https://tc39.es/proposal-json-parse-with-source/>).
//!
//! The parser is a loop over an explicit stack of the open arrays and
//! objects, so the nesting depth costs no Rust stack: V8 parses a million
//! levels too (the heap limit and the time limit end a script that makes
//! more than the machine has). A value goes into its parent as soon as it
//! is complete (`CreateDataProperty`: a `"__proto__"` key is an own data
//! property and a duplicate key keeps the position of the first and the
//! value of the last). The open containers and the pending keys are
//! recorded in the handle scope of the native function; a collection runs
//! only at the safepoints before an allocation, where no completed value
//! is waiting for its parent.
//!
//! The text is copied out of the heap once (the parser allocates while it
//! reads it). Errors are V8's `SyntaxError`s of Chromium 148: a message
//! for what the grammar expected at that place, with the position and the
//! line and column, or `Unexpected token`, which shows the source (the
//! whole text up to 20 units, else the 10 units on each side of the
//! token). A message holds Rust text, so an unpaired surrogate in it shows
//! as U+FFFD (V8 keeps it).
//!
//! With a reviver, the parser also makes the parse records that
//! `InternalizeJSONProperty` needs ([`Records`]): for each value its
//! source range, and for arrays and objects the records of the elements
//! and members in source order.

use swb_js_text::{CodeUnit, Str16, String16};

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult};

/// What the parser expected or found, for the message of a `SyntaxError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// The text ended where a value was expected.
    End,
    /// A character that no value starts with (or a literal that does not
    /// continue).
    Token,
    /// After `{`.
    PropertyNameOrBrace,
    /// After `,` in an object.
    DoubleQuotedName,
    /// After an element.
    CommaOrBracket,
    /// After a property name.
    Colon,
    /// After a property value.
    CommaOrBrace,
    /// Text after the value.
    NonWhitespace,
    /// `-` without a digit.
    NoNumber,
    /// A digit after a leading zero, or a number where a literal goes on.
    UnexpectedNumber,
    /// A string where a literal goes on.
    UnexpectedString,
    /// `.` without a digit.
    Fractional,
    /// `e` without a digit.
    Exponent,
    /// A string without a closing quote.
    Unterminated,
    /// A control character in a string.
    Control,
    /// A backslash and a character that no escape uses.
    Escape,
    /// `\u` without four hexadecimal digits.
    Unicode,
}

impl Kind {
    /// The part of the message before " at position".
    fn text(self) -> &'static str {
        match self {
            Kind::End => "Unexpected end of JSON input",
            Kind::Token => "Unexpected token",
            Kind::PropertyNameOrBrace => "Expected property name or '}' in JSON",
            Kind::DoubleQuotedName => "Expected double-quoted property name in JSON",
            Kind::CommaOrBracket => "Expected ',' or ']' after array element in JSON",
            Kind::Colon => "Expected ':' after property name in JSON",
            Kind::CommaOrBrace => "Expected ',' or '}' after property value in JSON",
            Kind::NonWhitespace => "Unexpected non-whitespace character after JSON",
            Kind::NoNumber => "No number after minus sign in JSON",
            Kind::UnexpectedNumber => "Unexpected number in JSON",
            Kind::UnexpectedString => "Unexpected string in JSON",
            Kind::Fractional => "Unterminated fractional number in JSON",
            Kind::Exponent => "Exponent part is missing a number in JSON",
            Kind::Unterminated => "Unterminated string in JSON",
            Kind::Control => "Bad control character in string literal in JSON",
            Kind::Escape => "Bad escaped character in JSON",
            Kind::Unicode => "Bad Unicode escape in JSON",
        }
    }
}

/// The sources for which V8 words the error as `"text" is not valid JSON`
/// (the results of `String(undefined)`, `String(NaN)`, `String(Infinity)`
/// and `String({})`). Each is shorter than [`MIN_LENGTH_FOR_CONTEXT`].
const SHORT_FORM_SOURCES: [&str; 4] = ["undefined", "NaN", "Infinity", "[object Object]"];

/// A source of at most this many units is shown whole in a message
/// (V8: twice the context of 10 units, plus one).
const MIN_LENGTH_FOR_CONTEXT: usize = 21;

/// The units of context on each side of the token in a message.
const CONTEXT_UNITS: usize = 10;

/// The parse checks the size of its records against the heap limit
/// after this many records.
const RECORDS_CHECK_EVERY: usize = 1024;

/// The records of a parse, for the reviver.
#[derive(Default)]
pub(super) struct Records {
    nodes: Vec<RecNode>,
    /// The records of the elements and members of all containers; a
    /// container's range is contiguous.
    kids: Vec<u32>,
    /// The completed records that wait for their container.
    pending: Vec<u32>,
}

/// The kind of a parse record.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecKind {
    /// A string, number, `true`, `false` or `null`.
    Primitive,
    /// An array.
    Array,
    /// An object.
    Object,
}

/// A JSON Parse Record: the parsed value, its source range, and for a
/// member its key.
pub(super) struct RecNode {
    pub(super) kind: RecKind,
    pub(super) value: Value,
    pub(super) start: u32,
    pub(super) end: u32,
    /// The key of an object member.
    pub(super) key: Option<PropertyKey>,
    kids_start: u32,
    kids_len: u32,
}

impl Records {
    /// The record `id`.
    pub(super) fn node(&self, id: u32) -> Option<&RecNode> {
        self.nodes.get(id as usize)
    }

    /// The root record (the last one made).
    pub(super) fn root(&self) -> Option<u32> {
        u32::try_from(self.nodes.len().checked_sub(1)?).ok()
    }

    /// The records of the elements and members of container `id`, in
    /// source order.
    pub(super) fn children(&self, id: u32) -> &[u32] {
        let Some(node) = self.node(id) else {
            return &[];
        };
        let start = node.kids_start as usize;
        self.kids
            .get(start..start + node.kids_len as usize)
            .unwrap_or(&[])
    }

    /// The bytes of the record buffers. They are outside the heap, so the
    /// parse checks them against the heap limit as they grow.
    fn bytes(&self) -> usize {
        self.nodes.capacity() * size_of::<RecNode>()
            + (self.kids.capacity() + self.pending.capacity()) * size_of::<u32>()
    }

    fn push_primitive(&mut self, value: Value, start: usize, end: usize) {
        self.push(RecKind::Primitive, value, start, end, 0);
    }

    fn push(&mut self, kind: RecKind, value: Value, start: usize, end: usize, base: usize) {
        let kids_start = self.kids.len();
        let mut kids_len = 0;
        if kind != RecKind::Primitive {
            let members = self.pending.split_off(base);
            kids_len = members.len();
            self.kids.extend(members);
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(RecNode {
            kind,
            value,
            start: start as u32,
            end: end as u32,
            key: None,
            kids_start: kids_start as u32,
            kids_len: kids_len as u32,
        });
        self.pending.push(id);
    }

    /// Sets the key of the record that was made last (a member value).
    fn set_last_key(&mut self, key: PropertyKey) {
        if let Some(id) = self.pending.last()
            && let Some(node) = self.nodes.get_mut(*id as usize)
        {
            node.key = Some(key);
        }
    }
}

/// An open array or object.
enum Frame {
    Array {
        object: Gc<Object>,
        length: u32,
        start: usize,
        base: usize,
    },
    Object {
        object: Gc<Object>,
        key: PropertyKey,
        members: u32,
        start: usize,
        base: usize,
    },
}

/// The text of a string literal: a range of the source (no escapes) or
/// built text.
enum Scanned {
    Plain(usize, usize),
    Built(String16),
}

/// The result of a parse.
pub(super) struct Parsed {
    pub(super) value: Value,
    pub(super) records: Option<Records>,
}

/// `ParseJSON` (§25.5.1.1) of the string `text`, which the caller has
/// rooted. `want_records` makes the parse records.
pub(super) fn parse(
    rt: &mut Runtime,
    text: Gc<JsString>,
    realm: u32,
    want_records: bool,
) -> VmResult<Parsed> {
    let units = copy_source(rt, text)?;
    match units {
        Source::Narrow(bytes) => run(rt, &bytes, realm, want_records),
        Source::Wide(units) => run(rt, &units, realm, want_records),
    }
}

/// The text of a string outside the heap.
enum Source {
    Narrow(Vec<u8>),
    Wide(Vec<u16>),
}

/// Copies the code units of `text` out of the heap: the parser
/// allocates while it reads them. The copy counts against the heap
/// limit and the time.
fn copy_source(rt: &mut Runtime, text: Gc<JsString>) -> VmResult<Source> {
    let len = rt.string_len(text)?;
    let bytes = len
        * if rt.heap.string(text)?.is_latin1() {
            1
        } else {
            2
        };
    rt.heap.reserve(bytes, &rt.vm)?;
    rt.charge_units(len)?;
    Ok(match rt.flat_text(text)? {
        Str16::Latin1(bytes) => Source::Narrow(bytes.to_vec()),
        Str16::Wide(units) => Source::Wide(units.to_vec()),
    })
}

/// `JSON.rawJSON` steps 3 to 6: parses `text` as one primitive value
/// (no object or array) and returns whether the whole text was a single
/// value. A malformed text is the `SyntaxError` of `JSON.parse`.
pub(super) fn check_primitive(rt: &mut Runtime, text: Gc<JsString>) -> VmResult<bool> {
    match copy_source(rt, text)? {
        Source::Narrow(bytes) => primitive_only(rt, &bytes),
        Source::Wide(units) => primitive_only(rt, &units),
    }
}

fn primitive_only<T: CodeUnit>(rt: &mut Runtime, src: &[T]) -> VmResult<bool> {
    let mut parser = Parser { src, pos: 0 };
    let Some(first) = parser.peek() else {
        return Err(parser.fail(Kind::End, 0));
    };
    match first {
        0x22 => {
            parser.pos += 1;
            parser.scan_string()?;
        }
        0x2d | 0x30..=0x39 => {
            parser.scan_number(rt)?;
        }
        0x74 => parser.expect_literal(b"true")?,
        0x66 => parser.expect_literal(b"false")?,
        0x6e => parser.expect_literal(b"null")?,
        _ => return Err(parser.fail(Kind::Token, 0)),
    }
    Ok(parser.pos == src.len())
}

fn run<T: CodeUnit>(
    rt: &mut Runtime,
    src: &[T],
    realm: u32,
    want_records: bool,
) -> VmResult<Parsed> {
    let mut run = Run {
        parser: Parser { src, pos: 0 },
        stack: Vec::new(),
        records: want_records.then(Records::default),
        hints: Hints::default(),
        cache: KeyCache::new(),
        object_proto: rt.intrinsic(realm, Intrinsic::ObjectPrototype)?,
        array_proto: rt.intrinsic(realm, Intrinsic::ArrayPrototype)?,
    };
    loop {
        rt.tick()?;
        if let Some(records) = &run.records
            && records.nodes.len() % RECORDS_CHECK_EVERY == 0
        {
            rt.heap.check_room(records.bytes())?;
        }
        let Started::Value(value) = run.start_value(rt)? else {
            continue;
        };
        if let Some(value) = run.complete(rt, value)? {
            return Ok(Parsed {
                value,
                records: run.records,
            });
        }
    }
}

/// What the start of a value gave.
enum Started {
    /// A complete value.
    Value(Value),
    /// A container was opened: its first member is next.
    Opened,
}

/// The state of a parse.
struct Run<'a, T: CodeUnit> {
    parser: Parser<'a, T>,
    /// The open arrays and objects, innermost last.
    stack: Vec<Frame>,
    records: Option<Records>,
    hints: Hints,
    cache: KeyCache,
    object_proto: Gc<Object>,
    array_proto: Gc<Object>,
}

impl<T: CodeUnit> Run<'_, T> {
    /// Reads the start of a value: a scalar in full, or a container up to
    /// its first member.
    fn start_value(&mut self, rt: &mut Runtime) -> VmResult<Started> {
        self.parser.skip_whitespace();
        let start = self.parser.pos;
        let Some(first) = self.parser.peek() else {
            return Err(self.parser.fail(Kind::End, start));
        };
        match first {
            0x7b => self.open_object(rt, start),
            0x5b => self.open_array(rt, start),
            0x22 => {
                self.parser.pos += 1;
                let value = self.parser.string_value(rt)?;
                if let Some(records) = self.records.as_mut() {
                    rt.heap.record(value);
                    records.push_primitive(value, start, self.parser.pos);
                }
                Ok(Started::Value(value))
            }
            0x2d | 0x30..=0x39 => {
                let value = Value::number(self.parser.scan_number(rt)?);
                self.primitive_record(value, start);
                Ok(Started::Value(value))
            }
            0x74 => self.literal(b"true", Value::Bool(true), start),
            0x66 => self.literal(b"false", Value::Bool(false), start),
            0x6e => self.literal(b"null", Value::Null, start),
            _ => Err(self.parser.fail(Kind::Token, start)),
        }
    }

    fn literal(&mut self, word: &[u8], value: Value, start: usize) -> VmResult<Started> {
        self.parser.expect_literal(word)?;
        self.primitive_record(value, start);
        Ok(Started::Value(value))
    }

    fn primitive_record(&mut self, value: Value, start: usize) {
        if let Some(records) = self.records.as_mut() {
            records.push_primitive(value, start, self.parser.pos);
        }
    }

    fn open_object(&mut self, rt: &mut Runtime, start: usize) -> VmResult<Started> {
        self.parser.pos += 1;
        self.parser.skip_whitespace();
        rt.heap.safepoint(&rt.vm)?;
        let object = rt.heap.new_object(Some(self.object_proto))?;
        rt.heap.record(object);
        let base = self.records.as_ref().map_or(0, |r| r.pending.len());
        if self.parser.peek() == Some(0x7d) {
            self.parser.pos += 1;
            let end = self.parser.pos;
            finish_container(
                &mut self.records,
                RecKind::Object,
                object,
                (start, end),
                base,
            );
            return Ok(Started::Value(Value::Object(object)));
        }
        let key = self
            .parser
            .read_member_name(rt, Kind::PropertyNameOrBrace, &mut self.cache)?;
        rt.heap
            .presize(object, self.hints.object(self.stack.len()), 0)?;
        self.stack.push(Frame::Object {
            object,
            key,
            members: 0,
            start,
            base,
        });
        Ok(Started::Opened)
    }

    fn open_array(&mut self, rt: &mut Runtime, start: usize) -> VmResult<Started> {
        self.parser.pos += 1;
        self.parser.skip_whitespace();
        rt.heap.safepoint(&rt.vm)?;
        let array = rt.heap.new_array(Some(self.array_proto), 0)?;
        rt.heap.record(array);
        let base = self.records.as_ref().map_or(0, |r| r.pending.len());
        if self.parser.peek() == Some(0x5d) {
            self.parser.pos += 1;
            let end = self.parser.pos;
            finish_container(&mut self.records, RecKind::Array, array, (start, end), base);
            return Ok(Started::Value(Value::Object(array)));
        }
        rt.heap
            .presize(array, 0, self.hints.array(self.stack.len()))?;
        self.stack.push(Frame::Array {
            object: array,
            length: 0,
            start,
            base,
        });
        Ok(Started::Opened)
    }

    /// A value is complete: hands it to the open containers, closing
    /// those whose last member it was. Returns the root value when the
    /// stack is empty; `None` when the next member follows.
    fn complete(&mut self, rt: &mut Runtime, mut value: Value) -> VmResult<Option<Value>> {
        loop {
            let depth = self.stack.len();
            let Some(frame) = self.stack.last_mut() else {
                self.parser.skip_whitespace();
                if self.parser.pos < self.parser.src.len() {
                    return Err(self.parser.fail(Kind::NonWhitespace, self.parser.pos));
                }
                return Ok(Some(value));
            };
            let closed = match frame {
                Frame::Array {
                    object,
                    length,
                    start,
                    base,
                } => {
                    rt.heap.push_fresh_element(*object, value, &rt.vm)?;
                    *length += 1;
                    self.parser.skip_whitespace();
                    match self.parser.peek() {
                        Some(0x2c) => {
                            self.parser.pos += 1;
                            return Ok(None);
                        }
                        Some(0x5d) => {
                            self.hints.set_array(depth - 1, *length);
                            (RecKind::Array, *object, *start, *base)
                        }
                        _ => {
                            return Err(self.parser.fail(Kind::CommaOrBracket, self.parser.pos));
                        }
                    }
                }
                Frame::Object {
                    object,
                    key,
                    members,
                    start,
                    base,
                } => {
                    rt.heap
                        .create_fresh_data_property(*object, *key, value, &rt.vm)?;
                    *members += 1;
                    if let Some(records) = self.records.as_mut() {
                        records.set_last_key(*key);
                    }
                    self.parser.skip_whitespace();
                    match self.parser.peek() {
                        Some(0x2c) => {
                            self.parser.pos += 1;
                            self.parser.skip_whitespace();
                            *key = self.parser.read_member_name(
                                rt,
                                Kind::DoubleQuotedName,
                                &mut self.cache,
                            )?;
                            return Ok(None);
                        }
                        Some(0x7d) => {
                            self.hints.set_object(depth - 1, *members);
                            (RecKind::Object, *object, *start, *base)
                        }
                        _ => {
                            return Err(self.parser.fail(Kind::CommaOrBrace, self.parser.pos));
                        }
                    }
                }
            };
            self.parser.pos += 1;
            self.stack.pop();
            let (kind, object, start, base) = closed;
            let end = self.parser.pos;
            finish_container(&mut self.records, kind, object, (start, end), base);
            value = Value::Object(object);
        }
    }
}

/// Makes the record of a closed container.
fn finish_container(
    records: &mut Option<Records>,
    kind: RecKind,
    object: Gc<Object>,
    (start, end): (usize, usize),
    base: usize,
) {
    if let Some(records) = records.as_mut() {
        records.push(kind, Value::Object(object), start, end, base);
    }
}

/// The sizes of the last closed array and object at each depth: a builder
/// that makes many alike (the rows of a table) reserves their buffers
/// once.
#[derive(Default)]
struct Hints {
    objects: Vec<u32>,
    arrays: Vec<u32>,
}

/// The most slots or elements that a hint reserves.
const MAX_HINT: u32 = 64;

impl Hints {
    fn object(&self, depth: usize) -> usize {
        self.objects.get(depth).map_or(0, |&n| n as usize)
    }

    fn array(&self, depth: usize) -> usize {
        self.arrays.get(depth).map_or(0, |&n| n as usize)
    }

    fn set_object(&mut self, depth: usize, count: u32) {
        Self::set(&mut self.objects, depth, count);
    }

    fn set_array(&mut self, depth: usize, count: u32) {
        Self::set(&mut self.arrays, depth, count);
    }

    fn set(list: &mut Vec<u32>, depth: usize, count: u32) {
        if depth >= 4096 {
            return;
        }
        if list.len() <= depth {
            list.resize(depth + 1, 0);
        }
        if let Some(slot) = list.get_mut(depth) {
            *slot = count.min(MAX_HINT);
        }
    }
}

/// The member names of the current parse that were seen, by a hash of
/// their text: a table repeats the same few names, and finding one costs
/// less than interning it. Every key in the cache is recorded in the
/// handle scope of the parse, so it stays alive.
struct KeyCache {
    entries: Vec<Option<PropertyKey>>,
}

/// The entries of the key cache (a power of two).
const KEY_CACHE_ENTRIES: usize = 256;

/// The longest name that the key cache keeps.
const KEY_CACHE_MAX_UNITS: usize = 32;

impl KeyCache {
    fn new() -> Self {
        KeyCache {
            entries: vec![None; KEY_CACHE_ENTRIES],
        }
    }

    fn slot(text: Str16<'_>) -> usize {
        let mut hash: u32 = text.len() as u32;
        for unit in text.units() {
            hash = (hash ^ u32::from(unit)).wrapping_mul(0x0100_0193);
        }
        (hash ^ hash >> 15) as usize % KEY_CACHE_ENTRIES
    }

    /// The key of a name that is in the cache.
    fn find(&self, rt: &Runtime, text: Str16<'_>) -> Option<PropertyKey> {
        if text.len() > KEY_CACHE_MAX_UNITS {
            return None;
        }
        let key = (*self.entries.get(Self::slot(text))?)?;
        match key {
            PropertyKey::String(atom) => (rt.heap.text(atom).ok()? == text).then_some(key),
            _ => None,
        }
    }

    /// Adds the key of a name (an atom).
    fn add(&mut self, text: Str16<'_>, key: PropertyKey) {
        if text.len() > KEY_CACHE_MAX_UNITS || !matches!(key, PropertyKey::String(_)) {
            return;
        }
        if let Some(entry) = self.entries.get_mut(Self::slot(text)) {
            *entry = Some(key);
        }
    }
}

/// The reader of the text.
struct Parser<'a, T: CodeUnit> {
    src: &'a [T],
    pos: usize,
}

/// The decimal powers that are exact doubles.
const POWERS_OF_TEN: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// 2^53: a mantissa below it is an exact double.
const TWO_POW_53: u64 = 1 << 53;

impl<T: CodeUnit> Parser<'_, T> {
    #[inline]
    fn peek(&self) -> Option<u16> {
        self.src.get(self.pos).map(|unit| unit.value())
    }

    #[inline]
    fn at(&self, index: usize) -> Option<u16> {
        self.src.get(index).map(|unit| unit.value())
    }

    /// Skips the whitespace of the grammar: space, tab, line feed and
    /// carriage return.
    #[inline]
    fn skip_whitespace(&mut self) {
        while let Some(0x20 | 0x09 | 0x0a | 0x0d) = self.peek() {
            self.pos += 1;
        }
    }

    /// The `SyntaxError` for `kind` at `position`.
    fn fail(&self, kind: Kind, position: usize) -> VmError {
        VmError::Raise {
            kind: crate::error::ThrowKind::SyntaxError,
            message: error_message(T::view(self.src), kind, position).into(),
        }
    }

    /// A member name and the colon after it. `kind` is the error for a
    /// name that does not start with a quote.
    fn read_member_name(
        &mut self,
        rt: &mut Runtime,
        kind: Kind,
        cache: &mut KeyCache,
    ) -> VmResult<PropertyKey> {
        if self.peek() != Some(0x22) {
            return Err(self.fail(kind, self.pos));
        }
        self.pos += 1;
        rt.heap.safepoint(&rt.vm)?;
        let key = match self.scan_string()? {
            Scanned::Plain(start, end) => {
                let text = T::view(self.src.get(start..end).unwrap_or(&[]));
                if let Some(key) = cache.find(rt, text) {
                    key
                } else {
                    let key = rt.heap.key_from_str16(text)?;
                    rt.heap.record(key);
                    cache.add(text, key);
                    key
                }
            }
            Scanned::Built(text) => {
                let key = rt.heap.key_from_str16(text.as_str16())?;
                rt.heap.record(key);
                key
            }
        };
        self.skip_whitespace();
        if self.peek() != Some(0x3a) {
            return Err(self.fail(Kind::Colon, self.pos));
        }
        self.pos += 1;
        Ok(key)
    }

    /// A string value after its opening quote.
    fn string_value(&mut self, rt: &mut Runtime) -> VmResult<Value> {
        rt.heap.safepoint(&rt.vm)?;
        let string = match self.scan_string()? {
            Scanned::Plain(start, end) if start == end => rt.vm.atoms.empty,
            Scanned::Plain(start, end) => {
                let text = self.src.get(start..end).unwrap_or(&[]);
                rt.heap.alloc_string(T::view(text).to_string16())?
            }
            Scanned::Built(text) => rt.heap.alloc_string(text)?,
        };
        Ok(Value::String(string))
    }

    /// Reads a string literal after its opening quote up to and including
    /// the closing quote (§25.5.1.1 and ECMA-404: `\u` escapes make code
    /// units, so a lone surrogate stays).
    fn scan_string(&mut self) -> VmResult<Scanned> {
        let start = self.pos;
        let len = self.src.len();
        let mut index = start;
        // The common case: no escape.
        while index < len {
            let unit = self.src.get(index).map_or(0, |u| u.value());
            if unit == 0x22 {
                self.pos = index + 1;
                return Ok(Scanned::Plain(start, index));
            }
            if unit == 0x5c || unit < 0x20 {
                break;
            }
            index += 1;
        }
        let mut built = String16::new();
        built.push_units(self.src.get(start..index).unwrap_or(&[]));
        while index < len {
            let unit = self.src.get(index).map_or(0, |u| u.value());
            match unit {
                0x22 => {
                    self.pos = index + 1;
                    return Ok(Scanned::Built(built));
                }
                0x5c => {
                    index += 1;
                    let Some(escape) = self.at(index) else {
                        return Err(self.fail(Kind::End, index));
                    };
                    let decoded = match escape {
                        0x22 | 0x5c | 0x2f => escape,
                        0x62 => 0x08,
                        0x66 => 0x0c,
                        0x6e => 0x0a,
                        0x72 => 0x0d,
                        0x74 => 0x09,
                        0x75 => {
                            let mut code = 0u16;
                            for offset in 1..=4 {
                                let digit = self
                                    .at(index + offset)
                                    .and_then(|u| char::from_u32(u32::from(u))?.to_digit(16));
                                let Some(digit) = digit else {
                                    return Err(self.fail(Kind::Unicode, index + offset));
                                };
                                code = code << 4 | digit as u16;
                            }
                            index += 4;
                            code
                        }
                        // V8 reads the text of a string with a unit above 255
                        // as two-byte text, and there a unit above 255 after
                        // the backslash is an unexpected token.
                        0x100.. => return Err(self.fail(Kind::Token, index)),
                        _ => return Err(self.fail(Kind::Escape, index)),
                    };
                    built.push(decoded);
                    index += 1;
                }
                0..=0x1f => return Err(self.fail(Kind::Control, index)),
                _ => {
                    // A run of plain units.
                    let run_start = index;
                    while index < len {
                        let unit = self.src.get(index).map_or(0, |u| u.value());
                        if unit == 0x22 || unit == 0x5c || unit < 0x20 {
                            break;
                        }
                        index += 1;
                    }
                    built.push_units(self.src.get(run_start..index).unwrap_or(&[]));
                }
            }
        }
        Err(self.fail(Kind::Unterminated, len))
    }

    /// Reads `true`, `false` or `null` (its first unit matched).
    fn expect_literal(&mut self, word: &[u8]) -> VmResult<()> {
        for (offset, &expected) in word.iter().enumerate().skip(1) {
            let position = self.pos + offset;
            match self.at(position) {
                None => return Err(self.fail(Kind::End, position)),
                Some(unit) if unit == u16::from(expected) => {}
                // V8 names the token that it found: a string or a number
                // where the rest of the word was expected.
                Some(0x22) => return Err(self.fail(Kind::UnexpectedString, position)),
                Some(0x2d | 0x30..=0x39) => {
                    return Err(self.fail(Kind::UnexpectedNumber, position));
                }
                Some(_) => return Err(self.fail(Kind::Token, position)),
            }
        }
        self.pos += word.len();
        Ok(())
    }

    /// Reads a number (ECMA-404 `number`) and returns its value. The
    /// common forms (up to 19 digits, a decimal exponent of at most 22
    /// in magnitude, a mantissa below 2^53) are exact doubles by one
    /// multiplication or division (Clinger); the others go through the
    /// exact `StringToNumber` of `js-text`.
    fn scan_number(&mut self, rt: &mut Runtime) -> VmResult<f64> {
        let start = self.pos;
        let mut index = start;
        let negative = self.at(index) == Some(0x2d);
        if negative {
            index += 1;
        }
        let is_digit = |unit: Option<u16>| matches!(unit, Some(0x30..=0x39));
        let mut mantissa: u64 = 0;
        let mut digits = 0usize;
        let mut exact = true;
        let mut exponent: i64 = 0;
        let add_digit = |unit: u16, mantissa: &mut u64, digits: &mut usize, exact: &mut bool| {
            let digit = u64::from(unit - 0x30);
            if *digits == 0 && digit == 0 {
                return;
            }
            *digits += 1;
            if *digits <= 19 {
                *mantissa = *mantissa * 10 + digit;
            } else {
                *exact = false;
            }
        };
        match self.at(index) {
            Some(0x30) => {
                index += 1;
                if is_digit(self.at(index)) {
                    return Err(self.fail(Kind::UnexpectedNumber, index));
                }
            }
            Some(unit @ 0x31..=0x39) => {
                add_digit(unit, &mut mantissa, &mut digits, &mut exact);
                index += 1;
                while let Some(unit @ 0x30..=0x39) = self.at(index) {
                    add_digit(unit, &mut mantissa, &mut digits, &mut exact);
                    index += 1;
                }
            }
            _ => return Err(self.fail(Kind::NoNumber, index)),
        }
        if self.at(index) == Some(0x2e) {
            index += 1;
            if !is_digit(self.at(index)) {
                return Err(self.fail(Kind::Fractional, index));
            }
            while let Some(unit @ 0x30..=0x39) = self.at(index) {
                add_digit(unit, &mut mantissa, &mut digits, &mut exact);
                if digits <= 19 {
                    exponent -= 1;
                }
                index += 1;
            }
        }
        if let Some(0x65 | 0x45) = self.at(index) {
            index += 1;
            let mut exponent_negative = false;
            if let Some(sign @ (0x2b | 0x2d)) = self.at(index) {
                exponent_negative = sign == 0x2d;
                index += 1;
            }
            if !is_digit(self.at(index)) {
                return Err(self.fail(Kind::Exponent, index));
            }
            let mut written: i64 = 0;
            while let Some(unit @ 0x30..=0x39) = self.at(index) {
                written = (written * 10 + i64::from(unit - 0x30)).min(1_000_000);
                index += 1;
            }
            exponent += if exponent_negative { -written } else { written };
        }
        self.pos = index;
        if exact && mantissa < TWO_POW_53 && (-22..=22).contains(&exponent) {
            let magnitude = if exponent >= 0 {
                mantissa as f64 * POWERS_OF_TEN[exponent as usize]
            } else {
                mantissa as f64 / POWERS_OF_TEN[(-exponent) as usize]
            };
            return Ok(if negative { -magnitude } else { magnitude });
        }
        let text = self.src.get(start..index).unwrap_or(&[]);
        let value = swb_js_text::number::string_to_number(T::view(text));
        rt.charge_number_scan(text.len())?;
        rt.charge_decimal_digits(text.len())?;
        Ok(value)
    }
}

// --- Messages ---

/// The message of the `SyntaxError` for `kind` at `position` of `src`.
fn error_message(src: Str16<'_>, kind: Kind, position: usize) -> String {
    match kind {
        Kind::End => kind.text().to_owned(),
        Kind::Token => token_message(src, position),
        _ => {
            let (line, column) = line_and_column(src, position);
            format!(
                "{} at position {position} (line {line} column {column})",
                kind.text()
            )
        }
    }
}

/// `Unexpected token 'x', "source" is not valid JSON`.
fn token_message(src: Str16<'_>, position: usize) -> String {
    let length = src.len();
    let lossy = |start: usize, end: usize| {
        src.slice(start, end)
            .map(Str16::to_string_lossy)
            .unwrap_or_default()
    };
    let token = lossy(position, position + 1);
    // Only a short source is shown whole (all short forms are short).
    if length < MIN_LENGTH_FOR_CONTEXT {
        let whole = lossy(0, length);
        if SHORT_FORM_SOURCES.contains(&whole.as_str()) {
            return format!("\"{whole}\" is not valid JSON");
        }
        return format!("Unexpected token '{token}', \"{whole}\" is not valid JSON");
    }
    let start = position.saturating_sub(CONTEXT_UNITS);
    let end = (position + CONTEXT_UNITS).min(length);
    // V8 also marks a window that starts at the beginning when the token
    // is at the tenth unit.
    let before = if position >= CONTEXT_UNITS { "..." } else { "" };
    let after = if end < length { "..." } else { "" };
    format!(
        "Unexpected token '{token}', {before}\"{}\"{after} is not valid JSON",
        lossy(start, end)
    )
}

/// The line and column (both from 1) of a position: a line ends at a line
/// feed, a carriage return or a carriage return and line feed.
fn line_and_column(src: Str16<'_>, position: usize) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    let mut index = 0;
    while index < position.min(src.len()) {
        match src.get(index) {
            Some(0x0a) => {
                line += 1;
                line_start = index + 1;
            }
            Some(0x0d) => {
                if src.get(index + 1) == Some(0x0a) && index + 1 < position {
                    index += 1;
                }
                line += 1;
                line_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    (line, position - line_start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str, kind: Kind, position: usize) -> String {
        let text = String16::from(text);
        error_message(text.as_str16(), kind, position)
    }

    #[test]
    fn context_window() {
        assert_eq!(
            message("[1,2,3,4,5,6,7,8,9,x]", Kind::Token, 19),
            "Unexpected token 'x', ...\"5,6,7,8,9,x]\" is not valid JSON"
        );
        assert_eq!(
            message("xabcdefghijklmnopqrstuvwxyz", Kind::Token, 0),
            "Unexpected token 'x', \"xabcdefghi\"... is not valid JSON"
        );
        assert_eq!(
            message("abcdefghijxabcdefghi", Kind::Token, 0),
            "Unexpected token 'a', \"abcdefghijxabcdefghi\" is not valid JSON"
        );
        assert_eq!(
            message("undefined", Kind::Token, 0),
            "\"undefined\" is not valid JSON"
        );
    }

    #[test]
    fn lines_and_columns() {
        assert_eq!(
            line_and_column(String16::from("ab\r\ncd").as_str16(), 5),
            (2, 2)
        );
        assert_eq!(
            line_and_column(String16::from("\r\rx").as_str16(), 2),
            (3, 1)
        );
        assert_eq!(
            line_and_column(String16::from("\n\rx").as_str16(), 2),
            (3, 1)
        );
    }
}
