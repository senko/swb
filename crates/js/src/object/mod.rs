//! Objects (ADR 0026 section 6, memo 3): the object record, its kinds,
//! creation, and the storage of named properties and elements.
//!
//! An object is a slot in the object arena: a shape (which holds the
//! prototype and the named keys), the values of the named properties, the
//! elements, the kind and the extensible flag. The internal methods are
//! in [`ops`] (ordinary objects, §10.1) and [`array`] (array exotic
//! objects, §10.4.2).

mod array;
mod elements;
mod ops;
mod property;
mod shape;

pub(crate) use array::INVALID_ARRAY_LENGTH;
pub use ops::{GetResult, SetResult};
pub use property::{Property, PropertyDescriptor};
pub(crate) use shape::KeyList;
pub use shape::Shape;

pub(crate) use elements::Elements;
pub(crate) use shape::{ShapeKind, Transition};

use std::fmt;

use crate::error::Result;
use crate::heap::{Arena, Gc, Generic, Heap, RootSource, Tracer};
use crate::string::{JsString, PropertyKey};
use crate::value::{Symbol, Value};
use crate::vm::{Closure, GeneratorState, NativeFunction};

/// The kind of an object: which internal methods it has (§10.1, §10.4).
/// Other kinds come with their features.
pub enum ObjectKind {
    /// An ordinary object.
    Ordinary,
    /// An array exotic object (§10.4.2). `length` is a field, not a stored
    /// property.
    Array {
        /// The value of `length`.
        length: u32,
        /// Whether `length` is writable.
        length_writable: bool,
    },
    /// A host object: native data that the embedder owns (ADR 0026
    /// section 12). The host classes of M8 build on it; only a test creates
    /// it today.
    Host {
        /// The class of the host object (chosen by the embedder).
        class: u32,
        /// The native data; the collector traces it with the object.
        data: Gc<Generic>,
    },
    /// A function of script code (§10.2): its code, captured cells and
    /// `this` mode.
    Function(Box<Closure>),
    /// A native function (§10.3).
    Native(Box<NativeFunction>),
    /// A generator object (§27.5): its state and, while it is suspended,
    /// its frame and register window.
    Generator(Box<GeneratorState>),
    /// A String object (§10.4.3). Minimal until M7: `length` is an own
    /// property; the VM reads the index properties from the string.
    StringWrapper(Gc<JsString>),
    /// A Number object (`[[NumberData]]`).
    NumberWrapper(f64),
    /// A Boolean object (`[[BooleanData]]`).
    BooleanWrapper(bool),
    /// A Symbol object (`[[SymbolData]]`).
    SymbolWrapper(Gc<Symbol>),
    /// An error object (`[[ErrorData]]`, §20.5): ordinary otherwise.
    Error,
    /// An arguments object (`[[ParameterMap]]` absent: the unmapped form,
    /// §10.4.4.6): ordinary otherwise.
    Arguments,
}

impl fmt::Debug for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ObjectKind::Ordinary => "Ordinary",
            ObjectKind::Array { .. } => "Array",
            ObjectKind::Host { .. } => "Host",
            ObjectKind::Function(_) => "Function",
            ObjectKind::Native(_) => "Native",
            ObjectKind::Generator(_) => "Generator",
            ObjectKind::StringWrapper(_) => "StringWrapper",
            ObjectKind::NumberWrapper(_) => "NumberWrapper",
            ObjectKind::BooleanWrapper(_) => "BooleanWrapper",
            ObjectKind::SymbolWrapper(_) => "SymbolWrapper",
            ObjectKind::Error => "Error",
            ObjectKind::Arguments => "Arguments",
        };
        f.write_str(name)
    }
}

impl ObjectKind {
    /// Reports the handles in the payload of the kind to the tracer. The
    /// marking loop calls it for every marked object.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        match self {
            ObjectKind::Ordinary
            | ObjectKind::Array { .. }
            | ObjectKind::Native(_)
            | ObjectKind::NumberWrapper(_)
            | ObjectKind::BooleanWrapper(_)
            | ObjectKind::Error
            | ObjectKind::Arguments => {}
            ObjectKind::Host { data, .. } => tracer.generic(*data),
            ObjectKind::Function(closure) => closure.trace(tracer),
            ObjectKind::Generator(state) => state.trace(tracer),
            ObjectKind::StringWrapper(string) => tracer.string(*string),
            ObjectKind::SymbolWrapper(symbol) => tracer.symbol(*symbol),
        }
    }

    /// The bytes that the payload owns outside the object record.
    pub(crate) fn heap_size(&self) -> usize {
        match self {
            ObjectKind::Function(closure) => closure.heap_size(),
            ObjectKind::Native(_) => size_of::<NativeFunction>(),
            ObjectKind::Generator(state) => state.heap_size(),
            _ => 0,
        }
    }
}

/// An object.
pub struct Object {
    pub(crate) shape: Gc<Shape>,
    /// The values of the named properties, in the positions that the shape
    /// gives. An accessor uses two slots (getter, setter).
    pub(crate) slots: Vec<Value>,
    pub(crate) elements: Elements,
    pub(crate) kind: ObjectKind,
    pub(crate) extensible: bool,
}

impl Object {
    /// The kind.
    pub fn kind(&self) -> &ObjectKind {
        &self.kind
    }

    /// The shape.
    pub fn shape(&self) -> Gc<Shape> {
        self.shape
    }

    /// The bytes that the object owns outside its slot (without the
    /// payload of its kind, which [`ObjectKind::heap_size`] counts).
    pub(crate) fn heap_size(&self) -> usize {
        self.slots.capacity() * size_of::<Value>() + self.elements.heap_size()
    }
}

impl Heap {
    /// A new ordinary object with the given prototype
    /// (`OrdinaryObjectCreate`, §10.1.12, without additional slots).
    pub fn new_object(&mut self, proto: Option<Gc<Object>>) -> Result<Gc<Object>> {
        self.new_object_of_kind(proto, ObjectKind::Ordinary)
    }

    /// A new host object with a class and native data (see
    /// [`ObjectKind::Host`]).
    pub fn new_host_object(
        &mut self,
        proto: Option<Gc<Object>>,
        class: u32,
        data: Gc<Generic>,
    ) -> Result<Gc<Object>> {
        self.generic_check(data)?;
        self.new_object_of_kind(proto, ObjectKind::Host { class, data })
    }

    /// A new array with the given prototype and `length` (`ArrayCreate`,
    /// §10.4.2.2). No elements are allocated.
    pub fn new_array(&mut self, proto: Option<Gc<Object>>, length: u32) -> Result<Gc<Object>> {
        self.new_object_of_kind(
            proto,
            ObjectKind::Array {
                length,
                length_writable: true,
            },
        )
    }

    /// A new object of a kind with a payload (functions, generators,
    /// wrappers). The kind's handles must be live.
    pub(crate) fn new_object_with_kind(
        &mut self,
        proto: Option<Gc<Object>>,
        kind: ObjectKind,
    ) -> Result<Gc<Object>> {
        self.new_object_of_kind(proto, kind)
    }

    fn new_object_of_kind(
        &mut self,
        proto: Option<Gc<Object>>,
        kind: ObjectKind,
    ) -> Result<Gc<Object>> {
        if let Some(proto) = proto {
            // A stale prototype fails now, not at the first use.
            self.object(proto)?;
        }
        let shape = self.root_shape(proto)?;
        self.charge(Arena::<Object>::slot_size() + kind.heap_size());
        self.arenas.objects.insert(Object {
            shape,
            slots: Vec::new(),
            elements: Elements::default(),
            kind,
            extensible: true,
        })
    }

    /// The object of a handle, for writing.
    pub(crate) fn object_mut(&mut self, handle: Gc<Object>) -> Result<&mut Object> {
        self.arenas.objects.get_mut(handle)
    }

    /// The shape of a handle.
    pub fn shape(&self, handle: Gc<Shape>) -> Result<&Shape> {
        self.arenas.shapes.get(handle)
    }

    /// The bytes that an object owns: its buffers and, in dictionary mode,
    /// its own shape's buffers.
    pub(crate) fn object_bytes(&self, object: Gc<Object>) -> Result<usize> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        let own_shape = if shape.is_dictionary() {
            shape.heap_size()
        } else {
            0
        };
        Ok(record.heap_size() + own_shape)
    }

    /// Charges the growth of an object since `before` (from
    /// [`Heap::object_bytes`]).
    pub(crate) fn charge_growth(&mut self, object: Gc<Object>, before: usize) -> Result<()> {
        let after = self.object_bytes(object)?;
        self.charge(after.saturating_sub(before));
        Ok(())
    }

    /// The own named property `key` of an object, from its shape and
    /// slots.
    pub(crate) fn named_property(
        &self,
        object: &Object,
        key: PropertyKey,
    ) -> Result<Option<Property>> {
        let shape = self.arenas.shapes.get(object.shape)?;
        let Some(entry) = shape.find(key, &self.arenas.key_lists)? else {
            return Ok(None);
        };
        let slot = entry.slot as usize;
        let first = object.slots.get(slot).copied();
        let second = if entry.flags.accessor() {
            object.slots.get(slot + 1).copied()
        } else {
            Some(Value::Undefined)
        };
        match (first, second) {
            (Some(first), Some(second)) => {
                Ok(Some(Property::from_flags(entry.flags, first, second)))
            }
            _ => Err(crate::error::Error::invariant(
                "a shape entry outside the slots",
            )),
        }
    }

    /// Adds a named property that the object does not have. Can collect:
    /// it reserves the growth of the slot vector first.
    pub(crate) fn add_named(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        property: Property,
        roots: &dyn RootSource,
    ) -> Result<()> {
        let flags = property.flags();
        let width = flags.width() as usize;
        let record = self.object(object)?;
        if record.slots.len() + width > record.slots.capacity() {
            let estimate = self.object_bytes(object)?.max(64);
            let (first, second) = property.slot_values();
            let extra = [
                object.into(),
                key_value(key),
                first,
                second.unwrap_or_default(),
            ];
            self.reserve_with(estimate, roots, &extra)?;
        }
        let before = self.object_bytes(object)?;
        // Reserve the slot memory first: if it fails, the shape (or the
        // dictionary) is still the one that matches the slots.
        self.object_mut(object)?.slots.try_reserve(width)?;
        let record = self.object(object)?;
        let shape = record.shape;
        let shared_len = match &self.arenas.shapes.get(shape)?.kind {
            ShapeKind::Shared(shared) => {
                if shared.slot_count as usize != record.slots.len() {
                    return Err(crate::error::Error::invariant(
                        "slot count differs from the shape",
                    ));
                }
                Some(shared.len)
            }
            ShapeKind::Dictionary(_) => None,
        };
        match shared_len {
            Some(len) if len < shape::DICTIONARY_LIMIT => {
                let child = self.shape_with_added(shape, key, flags)?;
                self.object_mut(object)?.shape = child;
            }
            Some(_) => {
                self.make_dictionary(object)?;
                self.dictionary_insert(object, key, flags)?;
            }
            None => self.dictionary_insert(object, key, flags)?,
        }
        let (first, second) = property.slot_values();
        let slots = &mut self.object_mut(object)?.slots;
        slots.push(first);
        if let Some(second) = second {
            slots.push(second);
        }
        self.charge_growth(object, before)
    }

    /// Replaces an existing named property `old` with `new`. A change of
    /// attributes or kind moves the object to dictionary mode.
    pub(crate) fn replace_named(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        old: Property,
        new: Property,
    ) -> Result<()> {
        let before = self.object_bytes(object)?;
        let (first, second) = new.slot_values();
        if old.flags() == new.flags() {
            let record = self.object(object)?;
            let shape = self.arenas.shapes.get(record.shape)?;
            let entry =
                shape
                    .find(key, &self.arenas.key_lists)?
                    .ok_or(crate::error::Error::invariant(
                        "a replaced property is missing",
                    ))?;
            let slots = &mut self.object_mut(object)?.slots;
            write_slots(slots, entry.slot, first, second)?;
            return Ok(());
        }
        self.make_dictionary(object)?;
        let (dictionary, slots) = self.dictionary_mut(object)?;
        let position = *dictionary
            .index
            .get(&key)
            .ok_or(crate::error::Error::invariant(
                "a replaced property is missing",
            ))?;
        let entry =
            dictionary
                .entries
                .get_mut(position as usize)
                .ok_or(crate::error::Error::invariant(
                    "a dictionary index outside the entries",
                ))?;
        let old_width = entry.flags.width();
        if new.flags().width() > old_width {
            // Data to accessor: two new slots at the end. Reserve them
            // before the entry changes, so that a failure leaves the
            // property as it was.
            slots.try_reserve(2)?;
            entry.flags = new.flags();
            let old_slot = entry.slot;
            entry.slot = slots.len() as u32;
            dictionary.dead_slots += old_width;
            write_slots(slots, old_slot, Value::Undefined, None)?;
            slots.push(first);
            slots.push(second.unwrap_or_default());
        } else {
            entry.flags = new.flags();
            if new.flags().width() < old_width {
                dictionary.dead_slots += old_width - new.flags().width();
                write_slots(slots, entry.slot + 1, Value::Undefined, None)?;
            }
            write_slots(slots, entry.slot, first, second)?;
        }
        self.compact_if_needed(object)?;
        self.charge_growth(object, before)
    }

    /// Removes a named property (moves the object to dictionary mode).
    pub(crate) fn remove_named(&mut self, object: Gc<Object>, key: PropertyKey) -> Result<()> {
        let before = self.object_bytes(object)?;
        self.make_dictionary(object)?;
        let (dictionary, slots) = self.dictionary_mut(object)?;
        let Some(position) = dictionary.index.remove(&key) else {
            return Ok(());
        };
        let entry =
            dictionary
                .entries
                .get_mut(position as usize)
                .ok_or(crate::error::Error::invariant(
                    "a dictionary index outside the entries",
                ))?;
        entry.flags = entry.flags.with_deleted();
        let (slot, width) = (entry.slot, entry.flags.width());
        dictionary.deleted += 1;
        dictionary.dead_slots += width;
        for offset in 0..width {
            write_slots(slots, slot + offset, Value::Undefined, None)?;
        }
        self.compact_if_needed(object)?;
        self.charge_growth(object, before)
    }
}

/// The value that keeps a key alive: its string or symbol.
pub(crate) fn key_value(key: PropertyKey) -> Value {
    match key {
        PropertyKey::Index(_) => Value::Undefined,
        PropertyKey::String(string) => Value::String(string),
        PropertyKey::Symbol(symbol) => Value::Symbol(symbol),
    }
}

/// Writes one or two values at `slot`.
fn write_slots(slots: &mut [Value], slot: u32, first: Value, second: Option<Value>) -> Result<()> {
    let error = crate::error::Error::invariant("a shape entry outside the slots");
    *slots.get_mut(slot as usize).ok_or(error)? = first;
    if let Some(second) = second {
        *slots.get_mut(slot as usize + 1).ok_or(error)? = second;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record sizes that `docs/performance.md` assumes; a change shows up here first.
    #[test]
    fn record_sizes() {
        assert_eq!(size_of::<Object>(), 88);
        assert_eq!(Arena::<Object>::slot_size(), 96);
        assert_eq!(Arena::<Shape>::slot_size(), 96);
        assert_eq!(Arena::<KeyList>::slot_size(), 40);
        assert_eq!(Arena::<JsString>::slot_size(), 48);
        assert_eq!(size_of::<shape::ShapeEntry>(), 20);
        assert_eq!(size_of::<Elements>(), 32);
    }
}
