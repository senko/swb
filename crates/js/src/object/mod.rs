//! Objects (ADR 0026 section 6, memo 3): the object record, its kinds,
//! creation, and the storage of named properties and elements.
//!
//! An object is a slot in the object arena: a shape (which holds the
//! prototype and the named keys), the values of the named properties, the
//! elements, the kind and the extensible flag. The internal methods are
//! in [`ops`] (ordinary objects, §10.1) and [`array`] (array exotic
//! objects, §10.4.2).

mod arguments;
mod array;
mod element_access;
mod elements;
mod iterator_helper;
mod ops;
mod private;
mod property;
mod proxy;
mod regexp;
mod shape;
mod table;

pub(crate) use array::{INVALID_ARRAY_LENGTH, INVALID_LENGTH_WRITE};
pub(crate) use element_access::{ChainSearch, DenseArray, FIRST_NAMED_INDEX};
pub(crate) use iterator_helper::{HelperOp, HelperStatus};
pub use iterator_helper::{IteratorHelper, WrappedIterator};
pub use ops::{GetResult, SetResult};
pub(crate) use property::validate_and_apply_descriptor;
pub use property::{Property, PropertyDescriptor};
pub(crate) use proxy::ProxyData;
pub(crate) use regexp::{LastMatch, RegExpRealm, RegExpShapes, program_share};
pub(crate) use regexp::{RegExpData, RegExpStringIterator};
pub(crate) use shape::KeyList;
pub use shape::Shape;
pub use table::{CollectionIterator, Table};

pub(crate) use elements::Elements;
pub(crate) use shape::{ShapeKind, Transition};

use std::fmt;

use crate::error::Result;
use crate::heap::{Arena, Gc, Generic, Heap, RootSource, Tracer};
use crate::string::{JsString, PropertyKey};
use crate::value::{Symbol, Value};
use crate::vm::{
    BoundFunction, Closure, EnvRecord, ErrorData, GeneratorState, NativeFunction,
    PrivateEnvironment, PromiseState,
};

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
    /// A bound function exotic object (§10.4.1).
    Bound(Box<BoundFunction>),
    /// A generator object (§27.5): its state and, while it is suspended,
    /// its frame and register window.
    Generator(Box<GeneratorState>),
    /// A String object (§10.4.3). `length` is a stored property; the
    /// index properties come from the string and are not stored (the heap
    /// answers `[[HasProperty]]`, `[[Delete]]`, `[[OwnPropertyKeys]]`,
    /// `[[DefineOwnProperty]]` and `[[Set]]` for them; the VM reads the
    /// characters, which are new strings). The string is flat: the code
    /// that makes a String object flattens a rope first.
    StringWrapper(Gc<JsString>),
    /// A Number object (`[[NumberData]]`).
    NumberWrapper(f64),
    /// A Boolean object (`[[BooleanData]]`).
    BooleanWrapper(bool),
    /// A Symbol object (`[[SymbolData]]`).
    SymbolWrapper(Gc<Symbol>),
    /// A raw JSON object (`[[IsRawJSON]]`, `JSON.rawJSON`): ordinary
    /// otherwise.
    RawJson,
    /// An Array Iterator (§23.1.5): the array-like, the next index and
    /// the kind of its results.
    ArrayIterator(Box<ArrayIterator>),
    /// A String Iterator (§22.1.5): the flat string and the next position.
    StringIterator(Box<StringIterator>),
    /// A `Map` (`[[MapData]]`, §24.1): the ordered hash table
    /// (`table.rs`).
    Map(Box<Table>),
    /// A `Set` (`[[SetData]]`, §24.2).
    Set(Box<Table>),
    /// A Map Iterator (§24.1.5): the map, the cursor in its table and the
    /// kind of its results.
    MapIterator(Box<CollectionIterator>),
    /// A Set Iterator (§24.2.6).
    SetIterator(Box<CollectionIterator>),
    /// An iterator helper (§27.1.2.1): the variables of the closure and
    /// its generator state.
    IteratorHelper(Box<IteratorHelper>),
    /// A `RegExp` object (§22.2.3.1 `RegExpAlloc`): its internal slots
    /// and the compiled program (`regexp.rs`).
    RegExp(Box<RegExpData>),
    /// A `RegExp` String Iterator (§22.2.9.1).
    RegExpStringIterator(Box<RegExpStringIterator>),
    /// An object made by `Iterator.from` (`[[Iterated]]`, §27.1.3.2.1.1).
    WrapForValidIterator(Box<WrappedIterator>),
    /// An Async-from-Sync Iterator (`[[SyncIteratorRecord]]`, §27.1.6):
    /// internal, only the records of async iteration hold it.
    AsyncFromSyncIterator(Box<WrappedIterator>),
    /// The variables of a running `Array.fromAsync` (internal, the data of
    /// its reactions; `builtins/array_from_async.rs`).
    ArrayFromAsync(Box<crate::builtins::FromAsync>),
    /// An error object (`[[ErrorData]]`, §20.5): ordinary otherwise. The
    /// payload is the captured stack for the `stack` property (none if
    /// `Error.stackTraceLimit` was not a number at the capture).
    Error(Option<Box<ErrorData>>),
    /// An arguments object: unmapped (§10.4.4.6, ordinary otherwise) or
    /// mapped (§10.4.4.7: its mapped elements hold the cells of their
    /// parameters, `arguments.rs`).
    Arguments,
    /// A promise (§27.2.6): its state, result and reactions
    /// (`vm/promise.rs`).
    Promise(Box<PromiseState>),
    /// The key iterator of a `for`-`in` loop (§14.7.5.9): internal, only
    /// a register of the loop holds it.
    ForInIterator(Box<ForInIterator>),
    /// The private environment of a class evaluation (§9.2): internal,
    /// only the class's `%private` binding and its functions hold it.
    PrivateEnvironment(Box<PrivateEnvironment>),
    /// A dynamic environment record (`vm/env.rs`): the object record of a
    /// `with` statement or the record of the vars of sloppy direct eval
    /// code. Internal: only bindings `%env` hold it.
    Environment(Box<EnvRecord>),
    /// A Proxy exotic object (§10.5): its target and handler (`proxy.rs`;
    /// the internal methods are in `vm/proxy.rs`).
    Proxy(Box<ProxyData>),
}

/// The state of a `for`-`in` loop: the object and the keys that the loop
/// visits, collected when it starts (see `vm/for_in.rs`).
#[derive(Debug)]
pub struct ForInIterator {
    /// The object (after `ToObject`).
    pub(crate) object: Gc<Object>,
    /// The enumerable string keys of the object and its prototypes, in
    /// order, each once; emptied when the loop has visited them all.
    pub(crate) keys: Box<[PropertyKey]>,
    /// The index of the next key.
    pub(crate) next: usize,
}

/// What an Array Iterator returns for each index (§23.1.5.1
/// `CreateArrayIterator`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationKind {
    /// The indices.
    Keys,
    /// The elements.
    Values,
    /// `[index, element]` pairs.
    Entries,
}

/// The state of an Array Iterator.
#[derive(Debug)]
pub struct ArrayIterator {
    /// `[[IteratedArrayLike]]`; `None` once the iterator is exhausted.
    pub(crate) target: Option<Gc<Object>>,
    /// `[[ArrayLikeNextIndex]]`: an integer below 2^53.
    pub(crate) next_index: f64,
    /// `[[ArrayLikeIterationKind]]`.
    pub(crate) kind: IterationKind,
}

/// The state of a String Iterator (§22.1.5.1 `%StringIteratorPrototype%`).
#[derive(Debug)]
pub struct StringIterator {
    /// The iterated string, flat; `None` once the iterator is exhausted.
    pub(crate) string: Option<Gc<JsString>>,
    /// The index of the next code unit.
    pub(crate) position: usize,
}

impl fmt::Debug for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ObjectKind::Ordinary => "Ordinary",
            ObjectKind::Array { .. } => "Array",
            ObjectKind::Host { .. } => "Host",
            ObjectKind::Function(_) => "Function",
            ObjectKind::Native(_) => "Native",
            ObjectKind::Bound(_) => "Bound",
            ObjectKind::Generator(_) => "Generator",
            ObjectKind::StringWrapper(_) => "StringWrapper",
            ObjectKind::NumberWrapper(_) => "NumberWrapper",
            ObjectKind::BooleanWrapper(_) => "BooleanWrapper",
            ObjectKind::SymbolWrapper(_) => "SymbolWrapper",
            ObjectKind::RawJson => "RawJson",
            ObjectKind::ArrayIterator(_) => "ArrayIterator",
            ObjectKind::StringIterator(_) => "StringIterator",
            ObjectKind::Map(_) => "Map",
            ObjectKind::Set(_) => "Set",
            ObjectKind::MapIterator(_) => "MapIterator",
            ObjectKind::SetIterator(_) => "SetIterator",
            ObjectKind::IteratorHelper(_) => "IteratorHelper",
            ObjectKind::RegExp(_) => "RegExp",
            ObjectKind::RegExpStringIterator(_) => "RegExpStringIterator",
            ObjectKind::WrapForValidIterator(_) => "WrapForValidIterator",
            ObjectKind::AsyncFromSyncIterator(_) => "AsyncFromSyncIterator",
            ObjectKind::ArrayFromAsync(_) => "ArrayFromAsync",
            ObjectKind::Error(_) => "Error",
            ObjectKind::Arguments => "Arguments",
            ObjectKind::Promise(_) => "Promise",
            ObjectKind::ForInIterator(_) => "ForInIterator",
            ObjectKind::PrivateEnvironment(_) => "PrivateEnvironment",
            ObjectKind::Environment(_) => "Environment",
            ObjectKind::Proxy(_) => "Proxy",
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
            | ObjectKind::NumberWrapper(_)
            | ObjectKind::BooleanWrapper(_)
            | ObjectKind::RawJson
            | ObjectKind::Arguments => {}
            ObjectKind::Error(data) => {
                if let Some(data) = data {
                    data.trace(tracer);
                }
            }
            ObjectKind::Native(native) => native.trace(tracer),
            ObjectKind::Host { data, .. } => tracer.generic(*data),
            ObjectKind::Function(closure) => closure.trace(tracer),
            ObjectKind::Bound(bound) => bound.trace(tracer),
            ObjectKind::Generator(state) => state.trace(tracer),
            ObjectKind::StringWrapper(string) => tracer.string(*string),
            ObjectKind::SymbolWrapper(symbol) => tracer.symbol(*symbol),
            ObjectKind::ArrayIterator(state) => {
                if let Some(target) = state.target {
                    tracer.object(target);
                }
            }
            ObjectKind::StringIterator(state) => {
                if let Some(string) = state.string {
                    tracer.string(string);
                }
            }
            ObjectKind::Map(table) | ObjectKind::Set(table) => table.trace(tracer),
            ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state) => {
                if let Some(table) = state.table {
                    tracer.object(table);
                }
            }
            ObjectKind::IteratorHelper(state) => state.trace(tracer),
            ObjectKind::RegExp(data) => data.trace(tracer),
            ObjectKind::RegExpStringIterator(state) => state.trace(tracer),
            ObjectKind::WrapForValidIterator(state) | ObjectKind::AsyncFromSyncIterator(state) => {
                iterator_helper::trace_record(&state.record, tracer);
            }
            ObjectKind::ForInIterator(state) => {
                tracer.object(state.object);
                for &key in &state.keys {
                    tracer.key(key);
                }
            }
            ObjectKind::PrivateEnvironment(env) => env.trace(tracer),
            ObjectKind::Environment(env) => env.trace(tracer),
            ObjectKind::Promise(state) => state.trace(tracer),
            ObjectKind::ArrayFromAsync(state) => state.trace(tracer),
            ObjectKind::Proxy(data) => data.trace(tracer),
        }
    }

    /// The bytes that the payload owns outside the object record.
    pub(crate) fn heap_size(&self) -> usize {
        match self {
            ObjectKind::Function(closure) => closure.heap_size(),
            ObjectKind::Native(native) => native.heap_size(),
            ObjectKind::Bound(bound) => bound.heap_size(),
            ObjectKind::Generator(state) => state.heap_size(),
            ObjectKind::ArrayIterator(_) => size_of::<ArrayIterator>(),
            ObjectKind::StringIterator(_) => size_of::<StringIterator>(),
            ObjectKind::Map(table) | ObjectKind::Set(table) => {
                size_of::<Table>() + table.heap_size()
            }
            ObjectKind::MapIterator(_) | ObjectKind::SetIterator(_) => {
                size_of::<CollectionIterator>()
            }
            ObjectKind::IteratorHelper(state) => state.heap_size(),
            ObjectKind::RegExp(data) => data.heap_size(),
            ObjectKind::RegExpStringIterator(_) => size_of::<RegExpStringIterator>(),
            ObjectKind::WrapForValidIterator(_) | ObjectKind::AsyncFromSyncIterator(_) => {
                size_of::<WrappedIterator>()
            }
            ObjectKind::Error(Some(data)) => data.heap_size(),
            ObjectKind::ForInIterator(state) => {
                size_of::<ForInIterator>() + state.keys.len() * size_of::<PropertyKey>()
            }
            ObjectKind::PrivateEnvironment(env) => env.heap_size(),
            ObjectKind::Environment(_) => size_of::<EnvRecord>(),
            ObjectKind::Promise(state) => state.heap_size(),
            ObjectKind::ArrayFromAsync(_) => size_of::<crate::builtins::FromAsync>(),
            ObjectKind::Proxy(_) => size_of::<ProxyData>(),
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

    /// The dense vector of an object's elements (holes are `Empty`), or
    /// `None` when its elements are sparse.
    pub(crate) fn dense_elements(&self, object: Gc<Object>) -> Result<Option<&[Value]>> {
        Ok(match &self.object(object)?.elements {
            Elements::Dense { values, .. } => Some(values),
            Elements::Sparse(_) => None,
        })
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
