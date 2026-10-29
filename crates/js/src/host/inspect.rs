//! What the host's structured clone needs from the engine (HTML
//! "`StructuredSerializeInternal`" and "`StructuredDeserialize`",
//! <https://html.spec.whatwg.org/multipage/structured-data.html>; ADR 0028
//! sections 6 and 13): the internal slots of an object ([`Slots`]), the
//! contents of a collection or a buffer, strings as code units, and
//! constructors for the objects that a deserialization makes. The bindings
//! (`swb-web`) walk the object graph themselves; they cannot read slots
//! such as `[[DateValue]]` or `[[MapData]]` otherwise.
//!
//! Nothing here runs script code. The values that these methods return are
//! recorded in the current handle scope (inside a native function), like
//! the results of `ops.rs`.

use crate::error::ThrowKind;
use crate::heap::Gc;

use crate::object::{BufferView, ElementType, Object, ObjectKind, TypedArrayData};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::{BigInt, Value};
use crate::vm::{Intrinsic, VmError, VmResult};

/// The kind of an object and the slots that structured clone reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Slots {
    /// An ordinary object (its own properties are the data).
    Ordinary,
    /// An array exotic object with its `length`.
    Array {
        /// The value of `length`.
        length: u32,
    },
    /// A Date object: `[[DateValue]]`.
    Date(f64),
    /// A `RegExp` object: `[[OriginalSource]]` as code units, and its flags.
    RegExp {
        /// The pattern text.
        source: Vec<u16>,
        /// The flag letters in the engine's order.
        flags: String,
    },
    /// A `Map`.
    Map,
    /// A `Set`.
    Set,
    /// An `ArrayBuffer`.
    ArrayBuffer {
        /// The byte length (0 when detached).
        byte_length: usize,
        /// `[[ArrayBufferMaxByteLength]]` of a resizable buffer.
        max_byte_length: Option<usize>,
        /// True if the buffer is detached.
        detached: bool,
    },
    /// A typed array or a `DataView`.
    View {
        /// The element type name (`Int8`, `Uint8Clamped`, `Float64`,
        /// `BigInt64`, ...), or `None` for a `DataView`.
        element: Option<&'static str>,
        /// The viewed `ArrayBuffer`.
        buffer: Gc<Object>,
        /// The offset of the first byte.
        byte_offset: usize,
        /// The fixed byte length, or `None` for a view that tracks the
        /// length of a resizable buffer. 0 for a view that is out of
        /// bounds.
        byte_length: Option<usize>,
    },
    /// An error object (`[[ErrorData]]`).
    Error,
    /// A Boolean, Number, String or `BigInt` object, or a Symbol object: the
    /// wrapped primitive.
    Primitive(Value),
    /// A callable object (function, bound function, class).
    Function,
    /// Another kind that does not clone: a proxy, a promise, a weak
    /// collection, an iterator, a generator, a host object, ...
    Other,
}

impl Runtime {
    /// The kind of `object` and its slots for structured clone.
    pub fn slots_of(&mut self, object: Gc<Object>) -> VmResult<Slots> {
        self.tick()?;
        let record = self.heap.object(object)?;
        Ok(match &record.kind {
            ObjectKind::Ordinary => Slots::Ordinary,
            ObjectKind::Array { length, .. } => Slots::Array { length: *length },
            ObjectKind::Date(t) => Slots::Date(*t),
            ObjectKind::Map(_) => Slots::Map,
            ObjectKind::Set(_) => Slots::Set,
            ObjectKind::Error(_) => Slots::Error,
            ObjectKind::NumberWrapper(n) => Slots::Primitive(Value::number(*n)),
            ObjectKind::BooleanWrapper(b) => Slots::Primitive(Value::Bool(*b)),
            ObjectKind::StringWrapper(s) => Slots::Primitive(Value::String(*s)),
            ObjectKind::BigIntWrapper(b) => Slots::Primitive(Value::BigInt(*b)),
            ObjectKind::SymbolWrapper(s) => Slots::Primitive(Value::Symbol(*s)),
            ObjectKind::Function(_) | ObjectKind::Native(_) | ObjectKind::Bound(_) => {
                Slots::Function
            }
            ObjectKind::RegExp(data) => {
                let source = data.source;
                let flags: String = data.regex.flags().letters().collect();
                Slots::RegExp {
                    source: self.string_units(source)?,
                    flags,
                }
            }
            ObjectKind::ArrayBuffer(data) => Slots::ArrayBuffer {
                byte_length: data.byte_length(),
                max_byte_length: data.max_byte_length(),
                detached: data.is_detached(),
            },
            ObjectKind::TypedArray(data) => {
                let data = **data;
                self.view_slots(Some(data.ty.name()), &data.view)?
            }
            ObjectKind::DataView(view) => {
                let view = **view;
                self.view_slots(None, &view)?
            }
            _ => Slots::Other,
        })
    }

    /// The slots of a view: its range in the current buffer, `None` for a
    /// view that tracks the length.
    fn view_slots(&self, element: Option<&'static str>, view: &BufferView) -> VmResult<Slots> {
        let buffer_length = self
            .heap
            .array_buffer(view.buffer)?
            .and_then(|data| data.bytes())
            .map_or(0, <[u8]>::len);
        let byte_length = view.byte_length;
        let in_bounds = view.byte_offset <= buffer_length
            && byte_length.is_none_or(|b| view.byte_offset + b <= buffer_length);
        Ok(Slots::View {
            element,
            buffer: view.buffer,
            byte_offset: view.byte_offset,
            byte_length: if in_bounds { byte_length } else { Some(0) },
        })
    }

    /// The entries of a `Map` (key and value) or the values of a `Set` (key
    /// and `undefined`) in insertion order, without the deleted ones. An
    /// empty list for another object.
    pub fn collection_entries(&mut self, object: Gc<Object>) -> VmResult<Vec<(Value, Value)>> {
        let Some(table) = self.heap.collection_table(object)? else {
            return Ok(Vec::new());
        };
        let entries: Vec<(Value, Value)> = (0..table.entry_len())
            .filter_map(|position| table.entry(position))
            .filter(|entry| !matches!(entry.key, Value::Empty))
            .map(|entry| (entry.key, entry.value))
            .collect();
        self.charge(entries.len())?;
        for (key, value) in &entries {
            self.heap.record(*key);
            self.heap.record(*value);
        }
        Ok(entries)
    }

    /// The bytes of an `ArrayBuffer` (a copy); empty if it is detached or
    /// not a buffer.
    pub fn buffer_bytes(&mut self, buffer: Gc<Object>) -> VmResult<Vec<u8>> {
        let bytes = self
            .heap
            .array_buffer(buffer)?
            .and_then(|data| data.bytes())
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        self.charge_bytes(bytes.len())?;
        Ok(bytes)
    }

    /// The number of code units of a string (no copy, no flattening).
    pub fn string_unit_count(&self, string: Gc<JsString>) -> VmResult<usize> {
        Ok(self.heap.string(string)?.len())
    }

    /// The code units of a string.
    pub fn string_units(&mut self, string: Gc<JsString>) -> VmResult<Vec<u16>> {
        let units: Vec<u16> = self.heap.string_units(string).collect();
        self.charge_units(units.len())?;
        Ok(units)
    }

    /// A new string value of code units (unpaired surrogates stay).
    pub fn string_from_units(&mut self, units: &[u16]) -> VmResult<Value> {
        self.charge_units(units.len())?;
        let text = swb_js_text::String16::from_units(units);
        let string = self.heap.alloc_string(text)?;
        Ok(self.keep(Value::String(string)))
    }

    /// The property key of a text of code units: an array index or an atom.
    pub fn key_from_units(&mut self, units: &[u16]) -> VmResult<crate::string::PropertyKey> {
        let text = swb_js_text::String16::from_units(units);
        Ok(self.heap.key_from_str16(text.as_str16())?)
    }

    /// The sign and the 64-bit limbs of a `BigInt`, least significant
    /// first.
    pub fn bigint_parts(&self, value: Gc<BigInt>) -> VmResult<(bool, Vec<u64>)> {
        let bigint = self.heap.bigint(value)?;
        Ok((bigint.is_negative(), bigint.limbs().to_vec()))
    }

    /// A new `BigInt` value from a sign and limbs.
    pub fn bigint_from_parts(&mut self, negative: bool, limbs: Vec<u64>) -> VmResult<Value> {
        let bigint = BigInt::from_limbs(negative, limbs)
            .map_err(|_| VmError::range_error("Maximum BigInt size exceeded"))?;
        let handle = self.heap.alloc_bigint(bigint)?;
        Ok(self.keep(Value::BigInt(handle)))
    }

    /// A new Date object with the time value `time` (already clipped).
    pub fn create_date(&mut self, realm: u32, time: f64) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::DatePrototype)?;
        let object = self
            .heap
            .new_object_with_kind(Some(proto), ObjectKind::Date(time))?;
        Ok(self.keep(object))
    }

    /// A new `RegExp` object for a pattern and flags that a `RegExp` object had
    /// (`RegExpCreate`); a `SyntaxError` if they do not compile.
    pub fn create_regexp(
        &mut self,
        realm: u32,
        source: &[u16],
        flags: &str,
    ) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::RegExpPrototype)?;
        self.scoped(|rt| {
            let pattern = rt.string_from_units(source)?;
            let flags = rt.string_value(flags)?;
            let (text, regex) = rt.regexp_parse(pattern, flags)?;
            rt.new_regexp_object(proto, text, regex)
        })
        .map(|object| self.keep(object))
    }

    /// A new empty `Map` (`map` true) or `Set`.
    pub fn create_collection(&mut self, realm: u32, map: bool) -> VmResult<Gc<Object>> {
        let which = if map {
            Intrinsic::MapPrototype
        } else {
            Intrinsic::SetPrototype
        };
        let proto = self.intrinsic(realm, which)?;
        let object = self.heap.new_collection(Some(proto), map)?;
        Ok(self.keep(object))
    }

    /// Adds an entry to a `Map` (`Map.prototype.set` without the call) or
    /// a value to a `Set` (`value` is ignored). The key is canonicalized
    /// (`-0` becomes `+0`).
    pub fn collection_add(
        &mut self,
        collection: Gc<Object>,
        key: Value,
        value: Value,
    ) -> VmResult<()> {
        let key = self.collection_key(key)?;
        let is_map = matches!(self.heap.object(collection)?.kind, ObjectKind::Map(_));
        let value = if is_map { value } else { Value::Undefined };
        self.heap.collection_put(collection, key, value)?;
        Ok(())
    }

    /// A new `ArrayBuffer` that owns `bytes`, resizable up to
    /// `max_byte_length` if given.
    pub fn create_array_buffer(
        &mut self,
        realm: u32,
        bytes: Vec<u8>,
        max_byte_length: Option<usize>,
    ) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::ArrayBufferPrototype)?;
        if !self.heap.reserve_buffer(bytes.len(), &self.vm) {
            return Err(VmError::range_error("Array buffer allocation failed"));
        }
        let buffer = self
            .heap
            .new_array_buffer(Some(proto), bytes, max_byte_length)?;
        Ok(self.keep(buffer))
    }

    /// A new typed array (`element` as [`Slots::View`] names it) or, for
    /// `None`, a `DataView` over `buffer`. `byte_length` `None` tracks the
    /// length of a resizable buffer. The range must fit the buffer.
    pub fn create_view(
        &mut self,
        realm: u32,
        element: Option<&str>,
        buffer: Gc<Object>,
        byte_offset: usize,
        byte_length: Option<usize>,
    ) -> VmResult<Gc<Object>> {
        let view = BufferView {
            buffer,
            byte_offset,
            byte_length,
        };
        let object = match element {
            None => {
                let proto = self.intrinsic(realm, Intrinsic::DataViewPrototype)?;
                self.heap.new_data_view(Some(proto), view)?
            }
            Some(name) => {
                let ty = element_type(name)
                    .ok_or(VmError::invariant("an unknown typed array element type"))?;
                let (_, proto) = self.typed_array_intrinsics(realm, ty)?;
                self.heap
                    .new_typed_array(Some(proto), TypedArrayData { view, ty })?
            }
        };
        Ok(self.keep(object))
    }

    /// `ToObject(primitive)` in `realm` for a Boolean, Number, String or
    /// `BigInt` value.
    pub fn create_wrapper(&mut self, primitive: Value) -> VmResult<Gc<Object>> {
        let object = self.scoped(|rt| rt.to_object(primitive))?;
        Ok(self.keep(object))
    }

    /// A new error object of `kind` that has no `message` property unless
    /// `message` is given, with the text `stack` as its `stack` (what an
    /// error that structured clone made has; the frames of the running code
    /// are not captured). Own properties in the order `stack`, `message`.
    pub fn create_error(
        &mut self,
        kind: ThrowKind,
        message: Option<&[u16]>,
        stack: Option<&[u16]>,
    ) -> VmResult<Gc<Object>> {
        let object = self.scoped(|rt| {
            let realm = rt.current_realm();
            let proto = rt.intrinsic(realm, crate::vm::error_prototype(kind))?;
            let object = rt
                .heap
                .new_object_with_kind(Some(proto), ObjectKind::Error(None))?;
            rt.heap.record(object);
            rt.install_stack(object, None)?;
            let stack_value = match stack {
                Some(units) => rt.string_from_units(units)?,
                None => Value::Undefined,
            };
            if let ObjectKind::Error(slot) = &mut rt.heap.object_mut(object)?.kind {
                *slot = Some(Box::new(crate::vm::ErrorData::with_stack(stack_value)));
            }
            if let Some(units) = message {
                let text = rt.string_from_units(units)?;
                let key = crate::string::PropertyKey::String(rt.vm.atoms.message);
                rt.define(object, key, text, true, false, true)?;
            }
            Ok(object)
        })?;
        Ok(self.keep(object))
    }

    /// The text that names an object that does not clone, as Chromium words
    /// it in a `DataCloneError` (V8's no-side-effect string): a function's
    /// whole source, else the text of the engine's messages.
    pub fn uncloneable_text(&self, value: Value) -> String {
        if let Value::Object(object) = value
            && let Ok(ObjectKind::Function(closure)) = self.heap.object(object).map(|o| &o.kind)
            && let Some(source) = closure.compiled.source_text()
        {
            return source.as_str16().to_string_lossy();
        }
        self.display(value)
    }
}

/// The element type of a name that [`Slots::View`] gives.
fn element_type(name: &str) -> Option<ElementType> {
    Some(match name {
        "Int8" => ElementType::Int8,
        "Uint8" => ElementType::Uint8,
        "Uint8Clamped" => ElementType::Uint8Clamped,
        "Int16" => ElementType::Int16,
        "Uint16" => ElementType::Uint16,
        "Int32" => ElementType::Int32,
        "Uint32" => ElementType::Uint32,
        "BigInt64" => ElementType::BigInt64,
        "BigUint64" => ElementType::BigUint64,
        "Float16" => ElementType::Float16,
        "Float32" => ElementType::Float32,
        "Float64" => ElementType::Float64,
        _ => return None,
    })
}
