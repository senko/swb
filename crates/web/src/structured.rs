//! Structured clone (HTML §2.7, "Safe passing of structured data",
//! <https://html.spec.whatwg.org/multipage/structured-data.html>):
//! `structuredClone()`, the state of `history.pushState()` and the data of
//! `popstate` events.
//!
//! [`serialize`] turns a value into a [`Serialized`] record, which owns
//! its data and does not refer to the heap: a table of items (objects,
//! arrays, dates, maps, buffers, errors, ...) that refer to one another by
//! index, which keeps cycles and shared references (the "memory" map of
//! the specification). A history entry keeps the record for as long as
//! the session history lives, also when the entry's document is gone.
//! [`deserialize`] makes new objects in a realm from a record, so one
//! record can be read any number of times.
//!
//! Both walks are iterative: an explicit stack replaces the recursion of
//! `StructuredSerializeInternal`, so a graph that is 100,000 levels deep
//! costs heap, not native stack. The order of the visits is that of the
//! recursive algorithm (a property's value is serialized before the next
//! property is read), so getters run in the order that Chromium's do.
//! Chromium fails a graph deeper than its stack allows (about 4,400 levels,
//! measured); swb fails at [`MAX_DEPTH`] levels with the same
//! `RangeError`.
//!
//! Supported (as Chromium): primitives, the wrapper objects of Boolean,
//! Number, String and `BigInt`, objects (their own enumerable string keys;
//! the prototype becomes `Object.prototype`), arrays (holes and extra
//! properties stay), `Date`, `RegExp` (without `lastIndex`), `Map`, `Set`,
//! `ArrayBuffer` (also resizable), typed arrays and `DataView` (sharing
//! their buffer), errors (name, message, `stack`, `cause`) and
//! `DOMException`. Everything else is a `DataCloneError`. `transfer` moves
//! `ArrayBuffer`s.

use std::collections::HashMap;
use std::rc::Rc;

use swb_js::{
    Gc, JsString, Object, Property, PropertyDescriptor, PropertyKey, Runtime, Slots, ThrowKind,
};
use swb_js::{Value, VmError};

use crate::bindings::{self as b, Cx, Exception, dom_exception};
use crate::generated as g;
use crate::generated::types::StructuredSerializeOptions;

/// The deepest nesting of objects that a clone follows.
pub(crate) const MAX_DEPTH: usize = 4096;

/// The most items (objects, strings, ...) that one record holds.
const MAX_ITEMS: usize = 1 << 22;

/// The most bytes of strings and buffers that one record holds.
const MAX_BYTES: usize = 1 << 29;

/// The most bytes that the state of a history entry holds (the history
/// keeps 50 entries).
const MAX_STATE_BYTES: usize = 4 << 20;

/// A value or a reference to an item of the record.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt { negative: bool, limbs: Vec<u64> },
    String(Vec<u16>),
    Ref(u32),
}

/// A property key.
#[derive(Clone, Debug, PartialEq)]
enum Key {
    Index(u32),
    Name(Vec<u16>),
}

/// An object of the record.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// An ordinary object with its own enumerable string keys.
    Object(Vec<(Key, Val)>),
    /// An array: `length` and the own enumerable properties (the
    /// elements that exist, and extra keys).
    Array {
        length: u32,
        props: Vec<(Key, Val)>,
    },
    Date(f64),
    RegExp {
        source: Vec<u16>,
        flags: String,
    },
    Map(Vec<(Val, Val)>),
    Set(Vec<Val>),
    Buffer {
        bytes: Vec<u8>,
        max_byte_length: Option<usize>,
    },
    View {
        element: Option<&'static str>,
        buffer: u32,
        byte_offset: usize,
        byte_length: Option<usize>,
    },
    Error {
        kind: ErrorKind,
        message: Option<Vec<u16>>,
        stack: Option<Vec<u16>>,
        cause: Option<Val>,
    },
    /// A Boolean, Number, String or `BigInt` object: its primitive.
    Wrapper(Val),
    DomException {
        name: String,
        message: String,
    },
}

/// The error constructors whose prototype a cloned error gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorKind {
    Error,
    Eval,
    Range,
    Reference,
    Syntax,
    Type,
    Uri,
}

impl ErrorKind {
    /// HTML "`StructuredSerializeInternal`", the errors: the name of a
    /// standard error constructor, else `Error`.
    fn of_name(name: &[u16]) -> ErrorKind {
        let text = String::from_utf16_lossy(name);
        match text.as_str() {
            "EvalError" => ErrorKind::Eval,
            "RangeError" => ErrorKind::Range,
            "ReferenceError" => ErrorKind::Reference,
            "SyntaxError" => ErrorKind::Syntax,
            "TypeError" => ErrorKind::Type,
            "URIError" => ErrorKind::Uri,
            _ => ErrorKind::Error,
        }
    }

    fn throw_kind(self) -> ThrowKind {
        match self {
            ErrorKind::Error => ThrowKind::Error,
            ErrorKind::Eval => ThrowKind::EvalError,
            ErrorKind::Range => ThrowKind::RangeError,
            ErrorKind::Reference => ThrowKind::ReferenceError,
            ErrorKind::Syntax => ThrowKind::SyntaxError,
            ErrorKind::Type => ThrowKind::TypeError,
            ErrorKind::Uri => ThrowKind::UriError,
        }
    }
}

/// The serialization of a value (HTML "serialization record" with its
/// memory): independent of any heap.
#[derive(Clone, Debug, PartialEq)]
pub struct Serialized {
    items: Vec<Item>,
    root: Val,
    /// The bytes of strings and buffers that the items hold.
    bytes: usize,
}

impl Serialized {
    /// The record of `null` (the `state` of an entry that has none).
    pub fn null() -> Serialized {
        Serialized {
            items: Vec::new(),
            root: Val::Null,
            bytes: 0,
        }
    }

    /// An estimate of the bytes that the record holds.
    pub fn byte_size(&self) -> usize {
        self.items.len() * size_of::<Item>() + self.bytes
    }
}

// ----- Serialization -----

/// What is left to do for an object that was entered.
enum Frame {
    /// Read the own enumerable properties `keys` of `object` into the
    /// `Object` or `Array` item `item`.
    Props {
        item: u32,
        object: Gc<Object>,
        keys: Vec<PropertyKey>,
        next: usize,
        props: Vec<(Key, Val)>,
    },
    /// Visit the entries of a map (key, value, key, ...).
    Map {
        item: u32,
        entries: Vec<(Value, Value)>,
        next: usize,
        done: Vec<(Val, Val)>,
    },
    /// Visit the values of a set.
    Set {
        item: u32,
        values: Vec<(Value, Value)>,
        next: usize,
        done: Vec<Val>,
    },
}

struct Serializer {
    items: Vec<Item>,
    memory: HashMap<Gc<Object>, u32>,
    stack: Vec<Frame>,
    bytes: usize,
}

/// The error of a value that cannot be cloned (HTML step: throw a
/// `DataCloneError`), worded as Chromium words it.
fn not_cloneable(rt: &Runtime, value: Value) -> Exception {
    let text = match value {
        Value::Object(object) => match platform_text(rt, object) {
            Some(text) => text,
            None => rt.uncloneable_text(value),
        },
        _ => rt.display(value),
    };
    Exception::dom("DataCloneError", format!("{text} could not be cloned."))
}

/// `HTMLBodyElement object` for a platform object, `#<Window>` for the
/// global object; `None` for other objects.
fn platform_text(rt: &Runtime, object: Gc<Object>) -> Option<String> {
    let (class, _) = rt.host_object(object.into())?;
    if class == g::WINDOW {
        return Some("#<Window>".to_owned());
    }
    Some(format!("{} object", rt.class_name(class)?))
}

fn range_error() -> Exception {
    Exception::range_error("Maximum call stack size exceeded")
}

fn too_large() -> Exception {
    Exception::range_error("The value is too large to clone.")
}

impl Serializer {
    fn add_bytes(&mut self, n: usize) -> b::Result<()> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > MAX_BYTES {
            return Err(too_large());
        }
        Ok(())
    }

    /// The code units of a string, counted against the byte limit before
    /// they are copied.
    fn units_of(&mut self, rt: &mut Runtime, s: Gc<JsString>) -> b::Result<Vec<u16>> {
        self.add_bytes(rt.string_unit_count(s)?.saturating_mul(2))?;
        Ok(rt.string_units(s)?)
    }

    /// The `Val` of a value: a primitive at once, an object as a
    /// reference to its item, which is made (and its frame pushed) at the
    /// first visit.
    fn visit(&mut self, rt: &mut Runtime, value: Value) -> b::Result<Val> {
        Ok(match value {
            Value::Undefined | Value::Empty => Val::Undefined,
            Value::Null => Val::Null,
            Value::Bool(b) => Val::Bool(b),
            Value::Int(i) => Val::Number(f64::from(i)),
            Value::Double(d) => Val::Number(d),
            Value::String(s) => Val::String(self.units_of(rt, s)?),
            Value::BigInt(n) => {
                let (negative, limbs) = rt.bigint_parts(n)?;
                self.add_bytes(limbs.len() * 8)?;
                Val::BigInt { negative, limbs }
            }
            Value::Symbol(_) | Value::Cell(_) | Value::Lazy(_) => {
                return Err(not_cloneable(rt, value));
            }
            Value::Object(object) => {
                if let Some(&index) = self.memory.get(&object) {
                    return Ok(Val::Ref(index));
                }
                self.enter(rt, object)?
            }
        })
    }

    /// Makes the item of an object that was not seen and pushes the work
    /// for its contents. The object gets its index first, so that a cycle
    /// through its contents finds it.
    fn enter(&mut self, rt: &mut Runtime, object: Gc<Object>) -> b::Result<Val> {
        if self.items.len() >= MAX_ITEMS {
            return Err(too_large());
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(range_error());
        }
        let value = Value::Object(object);
        let index = self.items.len() as u32;
        self.items.push(Item::Date(0.0));
        self.memory.insert(object, index);
        let item = match rt.host_object(value) {
            Some(_) => Self::host_item(rt, object)?,
            None => self.plain_item(rt, object, index)?,
        };
        if let Some(slot) = self.items.get_mut(index as usize) {
            *slot = item;
        }
        Ok(Val::Ref(index))
    }

    /// The item of an object that is not a platform object.
    fn plain_item(&mut self, rt: &mut Runtime, object: Gc<Object>, index: u32) -> b::Result<Item> {
        let value = Value::Object(object);
        Ok(match rt.slots_of(object)? {
            Slots::Ordinary => {
                let keys = rt.own_keys_of(object)?;
                self.push_props(index, object, keys);
                Item::Object(Vec::new())
            }
            Slots::Array { length } => {
                let keys = rt.own_keys_of(object)?;
                self.push_props(index, object, keys);
                Item::Array {
                    length,
                    props: Vec::new(),
                }
            }
            Slots::Date(t) => Item::Date(t),
            Slots::RegExp { source, flags } => {
                self.add_bytes(source.len() * 2)?;
                Item::RegExp { source, flags }
            }
            Slots::Map => {
                let entries = rt.collection_entries(object)?;
                self.stack.push(Frame::Map {
                    item: index,
                    entries,
                    next: 0,
                    done: Vec::new(),
                });
                Item::Map(Vec::new())
            }
            Slots::Set => {
                let values = rt.collection_entries(object)?;
                self.stack.push(Frame::Set {
                    item: index,
                    values,
                    next: 0,
                    done: Vec::new(),
                });
                Item::Set(Vec::new())
            }
            Slots::ArrayBuffer { detached: true, .. } => return Err(detached()),
            Slots::ArrayBuffer {
                max_byte_length,
                byte_length,
                ..
            } => {
                self.add_bytes(byte_length)?;
                Item::Buffer {
                    bytes: rt.buffer_bytes(object)?,
                    max_byte_length,
                }
            }
            Slots::View {
                element,
                buffer,
                byte_offset,
                byte_length,
            } => match self.visit(rt, Value::Object(buffer))? {
                Val::Ref(buffer) => Item::View {
                    element,
                    buffer,
                    byte_offset,
                    byte_length,
                },
                _ => return Err(not_cloneable(rt, value)),
            },
            Slots::Error => self.error_item(rt, object)?,
            Slots::Primitive(Value::Symbol(_)) | Slots::Function | Slots::Other => {
                return Err(not_cloneable(rt, value));
            }
            Slots::Primitive(primitive) => Item::Wrapper(self.visit(rt, primitive)?),
        })
    }

    fn push_props(&mut self, item: u32, object: Gc<Object>, keys: Vec<PropertyKey>) {
        self.stack.push(Frame::Props {
            item,
            object,
            keys,
            next: 0,
            props: Vec::new(),
        });
    }

    /// The item of a platform object: a `DOMException` clones, the other
    /// platform objects do not.
    fn host_item(rt: &Runtime, object: Gc<Object>) -> b::Result<Item> {
        let value = Value::Object(object);
        if !rt.implements(value, g::DOM_EXCEPTION) {
            return Err(not_cloneable(rt, value));
        }
        let Some((_, swb_js::HostData::Generic(generic))) = rt.host_object(value) else {
            return Err(not_cloneable(rt, value));
        };
        let data = rt
            .heap()
            .generic::<dom_exception::DomExceptionData>(generic)
            .map_err(|_| not_cloneable(rt, value))?;
        Ok(Item::DomException {
            name: data.name.clone(),
            message: data.message.clone(),
        })
    }

    /// The item of an error object: `name` is read with `Get`, `message`
    /// and `cause` from own data properties, `stack` with `Get` (as
    /// Chromium; measured in `tools/probes/web-structured-clone.json`).
    fn error_item(&mut self, rt: &mut Runtime, object: Gc<Object>) -> b::Result<Item> {
        let value = Value::Object(object);
        let name_key = rt.key_from_str("name")?;
        let kind = match rt.get_key(value, name_key)? {
            Value::String(s) => ErrorKind::of_name(&self.units_of(rt, s)?),
            _ => ErrorKind::Error,
        };
        let message_key = rt.key_from_str("message")?;
        let message = match rt.own_property_of(object, message_key)? {
            Some(Property::Data { value, .. }) => Some(self.text_of(rt, value)?),
            _ => None,
        };
        let stack_key = rt.key_from_str("stack")?;
        let stack = match rt.get_key(value, stack_key)? {
            Value::String(s) => Some(self.units_of(rt, s)?),
            _ => None,
        };
        let cause_key = rt.key_from_str("cause")?;
        let cause = match rt.own_property_of(object, cause_key)? {
            Some(Property::Data { value, .. }) => Some(self.visit(rt, value)?),
            _ => None,
        };
        Ok(Item::Error {
            kind,
            message,
            stack,
            cause,
        })
    }

    /// The text of a `message` value.
    fn text_of(&mut self, rt: &mut Runtime, value: Value) -> b::Result<Vec<u16>> {
        match value {
            Value::String(s) => self.units_of(rt, s),
            other => {
                let units: Vec<u16> = rt.to_rust_string(other)?.encode_utf16().collect();
                self.add_bytes(units.len() * 2)?;
                Ok(units)
            }
        }
    }

    /// Runs the stack until it is empty.
    fn run(&mut self, rt: &mut Runtime) -> b::Result<()> {
        let mut steps = 0_u32;
        while !self.stack.is_empty() {
            self.step(rt)?;
            // The time limit of the task also holds in this loop.
            steps = steps.wrapping_add(1);
            if steps.is_multiple_of(256) {
                rt.check_time()?;
            }
        }
        Ok(())
    }

    /// One step of the top frame: the next property, entry or value.
    fn step(&mut self, rt: &mut Runtime) -> b::Result<()> {
        let top = self.stack.len() - 1;
        // Take the work of the frame out, so that `visit` can push.
        match &mut self.stack[top] {
            Frame::Props {
                object, keys, next, ..
            } => {
                let (object, key) = (*object, keys.get(*next).copied());
                *next += 1;
                let Some(key) = key else {
                    {
                        self.finish();
                        return Ok(());
                    }
                };
                if matches!(key, PropertyKey::Symbol(_)) {
                    return Ok(());
                }
                let value = match rt.own_property_of(object, key)? {
                    Some(Property::Data {
                        value,
                        enumerable: true,
                        ..
                    }) => value,
                    Some(Property::Accessor {
                        enumerable: true, ..
                    }) => rt.get_key(Value::Object(object), key)?,
                    _ => return Ok(()),
                };
                let name = self.key_of(rt, key)?;
                let val = self.visit(rt, value)?;
                // The frame is no longer the top if `visit` pushed one: it
                // is at `top`.
                if let Frame::Props { props, .. } = &mut self.stack[top] {
                    props.push((name, val));
                }
            }
            Frame::Map { entries, next, .. } => {
                let entry = entries.get(*next / 2).copied();
                let is_key = *next % 2 == 0;
                *next += 1;
                let Some((key, value)) = entry else {
                    {
                        self.finish();
                        return Ok(());
                    }
                };
                let val = self.visit(rt, if is_key { key } else { value })?;
                if let Frame::Map { done, .. } = &mut self.stack[top] {
                    if is_key {
                        done.push((val, Val::Undefined));
                    } else if let Some(last) = done.last_mut() {
                        last.1 = val;
                    }
                }
            }
            Frame::Set { values, next, .. } => {
                let entry = values.get(*next).copied();
                *next += 1;
                let Some((key, _)) = entry else {
                    {
                        self.finish();
                        return Ok(());
                    }
                };
                let val = self.visit(rt, key)?;
                if let Frame::Set { done, .. } = &mut self.stack[top] {
                    done.push(val);
                }
            }
        }
        Ok(())
    }

    /// The top frame has no more work: stores its result in its item.
    fn finish(&mut self) {
        match self.stack.pop() {
            Some(Frame::Props { item, props, .. }) => {
                if let Some(Item::Object(slot) | Item::Array { props: slot, .. }) =
                    self.items.get_mut(item as usize)
                {
                    *slot = props;
                }
            }
            Some(Frame::Map { item, done, .. }) => {
                if let Some(slot) = self.items.get_mut(item as usize) {
                    *slot = Item::Map(done);
                }
            }
            Some(Frame::Set { item, done, .. }) => {
                if let Some(slot) = self.items.get_mut(item as usize) {
                    *slot = Item::Set(done);
                }
            }
            None => {}
        }
    }

    fn key_of(&mut self, rt: &mut Runtime, key: PropertyKey) -> b::Result<Key> {
        Ok(match key {
            PropertyKey::Index(i) => Key::Index(i),
            PropertyKey::String(s) => Key::Name(self.units_of(rt, s)?),
            PropertyKey::Symbol(_) => Key::Name(Vec::new()),
        })
    }
}

fn detached() -> Exception {
    Exception::dom(
        "DataCloneError",
        "An ArrayBuffer is detached and could not be cloned.",
    )
}

/// `StructuredSerialize` (HTML §2.7.2): the record of `value`. `transfer`
/// are the buffers that the caller detaches afterwards; they clone like
/// the other buffers here.
fn serialize_with(rt: &mut Runtime, value: Value) -> b::Result<Serialized> {
    let mut serializer = Serializer {
        items: Vec::new(),
        memory: HashMap::new(),
        stack: Vec::new(),
        bytes: 0,
    };
    let root = serializer.visit(rt, value)?;
    serializer.run(rt)?;
    Ok(Serialized {
        items: serializer.items,
        root,
        bytes: serializer.bytes,
    })
}

/// `StructuredSerialize` of `value` without transfer.
pub(crate) fn serialize(rt: &mut Runtime, value: Value) -> b::Result<Serialized> {
    serialize_with(rt, value)
}

// ----- Deserialization -----

/// `StructuredDeserialize` (HTML §2.7.4): new objects in `realm` for the
/// record's items; the value of the record's root.
pub(crate) fn deserialize(rt: &mut Runtime, realm: u32, record: &Serialized) -> b::Result<Value> {
    let mut objects: Vec<Option<Gc<Object>>> = vec![None; record.items.len()];
    // The buffers first: the views refer to them.
    for (index, item) in record.items.iter().enumerate() {
        if let Item::Buffer {
            bytes,
            max_byte_length,
        } = item
        {
            let buffer = rt.create_array_buffer(realm, bytes.clone(), *max_byte_length)?;
            objects[index] = Some(buffer);
        }
    }
    for (index, item) in record.items.iter().enumerate() {
        if objects[index].is_none() {
            objects[index] = Some(shell(rt, realm, item, &objects)?);
        }
    }
    for (index, item) in record.items.iter().enumerate() {
        if index.is_multiple_of(1024) {
            rt.check_time()?;
        }
        if let Some(object) = objects[index] {
            fill(rt, realm, object, item, &objects)?;
        }
    }
    value_of(rt, &record.root, &objects)
}

/// The object of an item without its contents (properties, entries, the
/// cause of an error).
fn shell(
    rt: &mut Runtime,
    realm: u32,
    item: &Item,
    objects: &[Option<Gc<Object>>],
) -> b::Result<Gc<Object>> {
    Ok(match item {
        Item::Object(_) => rt.new_plain_object(realm)?,
        Item::Array { length, .. } => {
            let array = rt.new_array_of(realm, &[])?;
            let key = rt.key_from_str("length")?;
            rt.set_key(array.into(), key, Value::number(f64::from(*length)), true)?;
            array
        }
        Item::Date(t) => rt.create_date(realm, *t)?,
        Item::RegExp { source, flags } => rt.create_regexp(realm, source, flags)?,
        Item::Map(_) => rt.create_collection(realm, true)?,
        Item::Set(_) => rt.create_collection(realm, false)?,
        Item::Buffer {
            bytes,
            max_byte_length,
        } => rt.create_array_buffer(realm, bytes.clone(), *max_byte_length)?,
        Item::View {
            element,
            buffer,
            byte_offset,
            byte_length,
        } => {
            let buffer = objects
                .get(*buffer as usize)
                .copied()
                .flatten()
                .ok_or(VmError::type_error("a view of a buffer that is not there"))?;
            rt.create_view(realm, *element, buffer, *byte_offset, *byte_length)?
        }
        Item::Error {
            kind,
            message,
            stack,
            ..
        } => rt.create_error(kind.throw_kind(), message.as_deref(), stack.as_deref())?,
        Item::Wrapper(val) => {
            let primitive = value_of(rt, val, objects)?;
            rt.create_wrapper(primitive)?
        }
        Item::DomException { name, message } => {
            dom_exception::create(rt, realm, g::DOM_EXCEPTION, name, message)?
        }
    })
}

/// The contents of an item: the properties of an object or array, the
/// entries of a map or set, the cause of an error.
fn fill(
    rt: &mut Runtime,
    _realm: u32,
    object: Gc<Object>,
    item: &Item,
    objects: &[Option<Gc<Object>>],
) -> b::Result<()> {
    match item {
        Item::Object(props) | Item::Array { props, .. } => {
            for (key, val) in props {
                let key = match key {
                    Key::Index(i) => PropertyKey::Index(*i),
                    Key::Name(units) => rt.key_from_units(units)?,
                };
                let value = value_of(rt, val, objects)?;
                rt.create_data_property(object, key, value)?;
            }
        }
        Item::Map(entries) => {
            for (key, val) in entries {
                let key = value_of(rt, key, objects)?;
                let val = value_of(rt, val, objects)?;
                rt.collection_add(object, key, val)?;
            }
        }
        Item::Set(values) => {
            for val in values {
                let key = value_of(rt, val, objects)?;
                rt.collection_add(object, key, Value::Undefined)?;
            }
        }
        Item::Error {
            cause: Some(cause), ..
        } => {
            let value = value_of(rt, cause, objects)?;
            let key = rt.key_from_str("cause")?;
            rt.define_property(
                object,
                key,
                PropertyDescriptor::data(value, true, false, true),
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// The value of a `Val`: a primitive is made, a reference gives its object.
fn value_of(rt: &mut Runtime, val: &Val, objects: &[Option<Gc<Object>>]) -> b::Result<Value> {
    Ok(match val {
        Val::Undefined => Value::Undefined,
        Val::Null => Value::Null,
        Val::Bool(b) => Value::Bool(*b),
        Val::Number(n) => Value::number(*n),
        Val::BigInt { negative, limbs } => rt.bigint_from_parts(*negative, limbs.clone())?,
        Val::String(units) => rt.string_from_units(units)?,
        Val::Ref(index) => Value::Object(objects.get(*index as usize).copied().flatten().ok_or(
            VmError::type_error("a reference to an item that is not there"),
        )?),
    })
}

/// The state of `history.pushState()` and `replaceState()` (HTML
/// "shared history push/replace state steps"): a record of `data`, or a
/// `DataCloneError`.
pub(crate) fn serialize_state(rt: &mut Runtime, data: Value) -> b::Result<Rc<Serialized>> {
    let record = serialize(rt, data)?;
    if record.byte_size() > MAX_STATE_BYTES {
        return Err(Exception::dom(
            "DataCloneError",
            "The state object is too large for the session history.",
        ));
    }
    Ok(Rc::new(record))
}

// ----- structuredClone() -----

/// The checks of the `transfer` list (HTML "structured clone", steps of
/// `StructuredSerializeWithTransfer`), worded as Chromium words them.
fn check_transfer(rt: &mut Runtime, transfer: &[Gc<Object>]) -> b::Result<()> {
    for (index, &object) in transfer.iter().enumerate() {
        let Slots::ArrayBuffer { detached, .. } = rt.slots_of(object)? else {
            return Err(Exception::dom(
                "DataCloneError",
                format!("Value at index {index} does not have a transferable type."),
            ));
        };
        if transfer[..index].contains(&object) {
            return Err(Exception::dom(
                "DataCloneError",
                format!("ArrayBuffer at index {index} is a duplicate of an earlier ArrayBuffer."),
            ));
        }
        if detached {
            return Err(Exception::dom(
                "DataCloneError",
                format!("ArrayBuffer at index {index} is already detached."),
            ));
        }
    }
    Ok(())
}

/// `structuredClone(value, options)` (HTML §2.7.1): the clone of `value`,
/// with the buffers of `options.transfer` moved.
pub(crate) fn structured_clone(
    cx: &mut Cx<'_>,
    value: Value,
    options: StructuredSerializeOptions,
) -> b::Result<Value> {
    let transfer = options.transfer;
    check_transfer(cx.rt, &transfer)?;
    let record = serialize_with(cx.rt, value)?;
    let result = deserialize(cx.rt, cx.realm(), &record)?;
    for buffer in transfer {
        cx.rt.detach_array_buffer(buffer.into())?;
    }
    Ok(result)
}
