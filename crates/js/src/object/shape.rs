//! Shapes (hidden classes): the ordered named keys of an object with
//! their attributes and slot positions, and the object's prototype (ADR
//! 0026 section 6, memo 3.1).
//!
//! - Root shapes: each prototype (and `null`) has one empty root shape.
//!   A weak table maps the prototype to it.
//! - Transitions: adding a property moves an object from a shape to a
//!   child shape. The weak transition index maps (parent, key,
//!   attributes) to the child.
//! - Key lists: a shared shape uses a prefix of an ordered key list. A
//!   child that extends the end of its parent's list shares the list; a
//!   second child of the same parent (a branch) copies the prefix.
//! - Lookup: linear search in small shapes; larger ones build a hash
//!   index on their second lookup.
//! - Dictionary mode: an object leaves the shared shapes when a property
//!   is deleted, when an attribute of a property changes, when its
//!   prototype changes while it has properties, or when it has more than
//!   [`DICTIONARY_LIMIT`] named properties. It then has its own shape with
//!   its prototype and an ordered map (insertion order with tombstones,
//!   plus a hash index). It never returns to shared shapes.
//!
//! The values of named properties are in the object's slots in all
//! modes; an accessor property uses two slots (getter, setter).

use std::cell::{Cell, OnceCell};

use crate::error::{Error, Result};
use crate::heap::{Arena, Gc, HandleMap, Heap};
use crate::object::Object;
use crate::object::property::Flags;
use crate::string::PropertyKey;

/// Shapes with at most this many keys are searched linearly.
const LINEAR_SEARCH_LIMIT: u32 = 8;

/// An object with this many named properties goes to dictionary mode
/// when it gets one more (ADR 0026: start at 128, then measure).
pub(crate) const DICTIONARY_LIMIT: u32 = 128;

/// A dictionary compacts when it has at least this many tombstones and
/// they are at least half of its entries.
const COMPACT_MIN: u32 = 8;

/// A named property in a shape: the key, the attributes and the index of
/// its first value slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShapeEntry {
    pub(crate) key: PropertyKey,
    pub(crate) flags: Flags,
    pub(crate) slot: u32,
}

/// An ordered key list that shared shapes use a prefix of.
pub(crate) struct KeyList {
    pub(crate) entries: Vec<ShapeEntry>,
    /// Scratch for the collector: the longest prefix that a live shape
    /// uses.
    pub(crate) live_len: u32,
}

impl KeyList {
    pub(crate) fn heap_size(&self) -> usize {
        self.entries.capacity() * size_of::<ShapeEntry>()
    }
}

/// A shape: the prototype and the named keys of the objects that use it.
pub struct Shape {
    pub(crate) proto: Option<Gc<Object>>,
    pub(crate) kind: ShapeKind,
}

pub(crate) enum ShapeKind {
    Shared(SharedShape),
    Dictionary(Dictionary),
}

/// A shape that many objects can share.
pub(crate) struct SharedShape {
    pub(crate) keys: Gc<KeyList>,
    /// The number of entries of `keys` that this shape uses.
    pub(crate) len: u32,
    /// The number of value slots of an object with this shape.
    pub(crate) slot_count: u32,
    /// The shape this one was derived from; a strong reference, so that a
    /// live shape keeps its chain and its transitions stay canonical.
    pub(crate) parent: Option<Gc<Shape>>,
    /// Key → entry position; built on the second lookup in a large shape.
    /// Boxed, so that shared shapes stay small; most never build it.
    index: OnceCell<Box<HandleMap<PropertyKey, u32>>>,
    lookups: Cell<u8>,
}

/// The own map of an object in dictionary mode.
pub(crate) struct Dictionary {
    /// Entries in insertion order; removed ones are tombstones.
    pub(crate) entries: Vec<ShapeEntry>,
    /// Key → position in `entries`, for live entries.
    pub(crate) index: HandleMap<PropertyKey, u32>,
    /// The number of tombstones in `entries`.
    pub(crate) deleted: u32,
    /// The number of object slots that no entry uses.
    pub(crate) dead_slots: u32,
}

/// The key of the transition index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Transition {
    pub(crate) parent: Gc<Shape>,
    pub(crate) key: PropertyKey,
    pub(crate) flags: Flags,
}

impl Shape {
    /// The prototype of the objects with this shape.
    pub fn proto(&self) -> Option<Gc<Object>> {
        self.proto
    }

    /// Whether this is the own shape of an object in dictionary mode.
    pub fn is_dictionary(&self) -> bool {
        matches!(self.kind, ShapeKind::Dictionary(_))
    }

    /// The number of named properties.
    pub fn property_count(&self) -> usize {
        match &self.kind {
            ShapeKind::Shared(shared) => shared.len as usize,
            ShapeKind::Dictionary(dictionary) => {
                dictionary.entries.len() - dictionary.deleted as usize
            }
        }
    }

    /// The entries in insertion order. A dictionary's tombstones are
    /// included (their flags say so).
    pub(crate) fn entries<'a>(&'a self, lists: &'a Arena<KeyList>) -> Result<&'a [ShapeEntry]> {
        match &self.kind {
            ShapeKind::Shared(shared) => lists
                .get(shared.keys)?
                .entries
                .get(..shared.len as usize)
                .ok_or(Error::invariant("a shape is longer than its key list")),
            ShapeKind::Dictionary(dictionary) => Ok(&dictionary.entries),
        }
    }

    /// The entry of `key`, if the shape has it.
    pub(crate) fn find(
        &self,
        key: PropertyKey,
        lists: &Arena<KeyList>,
    ) -> Result<Option<ShapeEntry>> {
        match &self.kind {
            ShapeKind::Shared(shared) => shared.find(key, lists),
            ShapeKind::Dictionary(dictionary) => Ok(dictionary
                .index
                .get(&key)
                .and_then(|&position| dictionary.entries.get(position as usize))
                .copied()),
        }
    }

    /// The bytes that the shape owns outside its slot (not the shared key
    /// list).
    pub(crate) fn heap_size(&self) -> usize {
        match &self.kind {
            ShapeKind::Shared(shared) => shared
                .index
                .get()
                .map_or(0, |index| index.capacity() * HASH_ENTRY_BYTES),
            ShapeKind::Dictionary(dictionary) => {
                dictionary.entries.capacity() * size_of::<ShapeEntry>()
                    + dictionary.index.capacity() * HASH_ENTRY_BYTES
            }
        }
    }
}

/// The approximate bytes of one entry of a `HashMap<PropertyKey, u32>`.
const HASH_ENTRY_BYTES: usize = size_of::<(PropertyKey, u32)>() + 1;

impl SharedShape {
    fn find(&self, key: PropertyKey, lists: &Arena<KeyList>) -> Result<Option<ShapeEntry>> {
        let entries = lists
            .get(self.keys)?
            .entries
            .get(..self.len as usize)
            .ok_or(Error::invariant("a shape is longer than its key list"))?;
        if self.len > LINEAR_SEARCH_LIMIT {
            if let Some(index) = self.index.get() {
                return Ok(index
                    .get(&key)
                    .and_then(|&position| entries.get(position as usize))
                    .copied());
            }
            let lookups = self.lookups.get().saturating_add(1);
            self.lookups.set(lookups);
            if lookups >= 2 {
                let index = Box::new(
                    entries
                        .iter()
                        .enumerate()
                        .map(|(position, entry)| (entry.key, position as u32))
                        .collect(),
                );
                let index = self.index.get_or_init(|| index);
                return Ok(index
                    .get(&key)
                    .and_then(|&position| entries.get(position as usize))
                    .copied());
            }
        }
        Ok(entries.iter().find(|entry| entry.key == key).copied())
    }
}

impl Dictionary {
    /// Whether enough tombstones or dead slots have collected for a
    /// compaction.
    fn needs_compaction(&self, slot_count: usize) -> bool {
        let (deleted, dead_slots) = (self.deleted as usize, self.dead_slots as usize);
        let min = COMPACT_MIN as usize;
        (deleted >= min && deleted * 2 >= self.entries.len())
            || (dead_slots >= min && dead_slots * 2 >= slot_count)
    }
}

impl Heap {
    /// The root shape of `proto`: the empty shape of the objects that it
    /// is the prototype of.
    pub(crate) fn root_shape(&mut self, proto: Option<Gc<Object>>) -> Result<Gc<Shape>> {
        if let Some(&shape) = self.root_shapes.get(&proto)
            && self.arenas.shapes.contains(shape)
        {
            return Ok(shape);
        }
        self.charge(
            Arena::<KeyList>::slot_size() + Arena::<Shape>::slot_size() + ROOT_TABLE_ENTRY_BYTES,
        );
        let keys = self.arenas.key_lists.insert(KeyList {
            entries: Vec::new(),
            live_len: 0,
        })?;
        let shape = self.arenas.shapes.insert(Shape {
            proto,
            kind: ShapeKind::Shared(SharedShape {
                keys,
                len: 0,
                slot_count: 0,
                parent: None,
                index: OnceCell::new(),
                lookups: Cell::new(0),
            }),
        })?;
        self.root_shapes.insert(proto, shape);
        Ok(shape)
    }

    /// The child of the shared shape `parent` with one more key; follows
    /// or creates the transition.
    pub(crate) fn shape_with_added(
        &mut self,
        parent: Gc<Shape>,
        key: PropertyKey,
        flags: Flags,
    ) -> Result<Gc<Shape>> {
        let transition = Transition { parent, key, flags };
        if let Some(&child) = self.transitions.get(&transition)
            && self.arenas.shapes.contains(child)
        {
            return Ok(child);
        }
        let shape = self.arenas.shapes.get(parent)?;
        let ShapeKind::Shared(shared) = &shape.kind else {
            return Err(Error::invariant("a transition from a dictionary shape"));
        };
        let proto = shape.proto;
        let (list_handle, len, slot_count) = (shared.keys, shared.len, shared.slot_count);
        let entry = ShapeEntry {
            key,
            flags,
            slot: slot_count,
        };
        let list = self.arenas.key_lists.get_mut(list_handle)?;
        let keys = if list.entries.len() == len as usize {
            // The parent ends the list: extend it.
            list.entries.try_reserve(1)?;
            let before = list.entries.capacity();
            list.entries.push(entry);
            let grown = list.entries.capacity() - before;
            self.charge(grown * size_of::<ShapeEntry>());
            list_handle
        } else {
            // A branch: copy the parent's prefix.
            let mut entries = Vec::new();
            entries.try_reserve_exact(len as usize + 1)?;
            entries.extend_from_slice(
                list.entries
                    .get(..len as usize)
                    .ok_or(Error::invariant("a shape is longer than its key list"))?,
            );
            entries.push(entry);
            self.charge(
                Arena::<KeyList>::slot_size() + entries.capacity() * size_of::<ShapeEntry>(),
            );
            self.arenas.key_lists.insert(KeyList {
                entries,
                live_len: 0,
            })?
        };
        self.charge(Arena::<Shape>::slot_size() + TRANSITION_ENTRY_BYTES);
        let child = self.arenas.shapes.insert(Shape {
            proto,
            kind: ShapeKind::Shared(SharedShape {
                keys,
                len: len + 1,
                slot_count: slot_count + flags.width(),
                parent: Some(parent),
                index: OnceCell::new(),
                lookups: Cell::new(0),
            }),
        })?;
        self.transitions.insert(transition, child);
        Ok(child)
    }

    /// Moves an object to dictionary mode (no change if it is already in
    /// it). The slots keep their positions.
    pub(crate) fn make_dictionary(&mut self, object: Gc<Object>) -> Result<()> {
        let shape_handle = self.object(object)?.shape;
        let shape = self.arenas.shapes.get(shape_handle)?;
        if shape.is_dictionary() {
            return Ok(());
        }
        let proto = shape.proto;
        let mut entries = Vec::new();
        let shared = shape.entries(&self.arenas.key_lists)?;
        entries.try_reserve_exact(shared.len())?;
        entries.extend_from_slice(shared);
        let mut index = HandleMap::default();
        index.try_reserve(entries.len())?;
        for (position, entry) in entries.iter().enumerate() {
            index.insert(entry.key, position as u32);
        }
        let dictionary = Shape {
            proto,
            kind: ShapeKind::Dictionary(Dictionary {
                entries,
                index,
                deleted: 0,
                dead_slots: 0,
            }),
        };
        self.charge(Arena::<Shape>::slot_size() + dictionary.heap_size());
        let new_shape = self.arenas.shapes.insert(dictionary)?;
        self.object_mut(object)?.shape = new_shape;
        Ok(())
    }

    /// The dictionary of an object in dictionary mode, and its slots.
    pub(crate) fn dictionary_mut(
        &mut self,
        object: Gc<Object>,
    ) -> Result<(&mut Dictionary, &mut Vec<crate::value::Value>)> {
        let arenas = &mut self.arenas;
        let object = arenas.objects.get_mut(object)?;
        let shape = arenas.shapes.get_mut(object.shape)?;
        match &mut shape.kind {
            ShapeKind::Dictionary(dictionary) => Ok((dictionary, &mut object.slots)),
            ShapeKind::Shared(_) => Err(Error::invariant("the object is not in dictionary mode")),
        }
    }

    /// Adds an entry to an object in dictionary mode. The caller pushes the
    /// values.
    pub(crate) fn dictionary_insert(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        flags: Flags,
    ) -> Result<()> {
        let (dictionary, slots) = self.dictionary_mut(object)?;
        let entry = ShapeEntry {
            key,
            flags,
            slot: slots.len() as u32,
        };
        dictionary.entries.try_reserve(1)?;
        dictionary.index.try_reserve(1)?;
        dictionary
            .index
            .insert(key, dictionary.entries.len() as u32);
        dictionary.entries.push(entry);
        Ok(())
    }

    /// Compacts the dictionary of an object if it has many tombstones or
    /// dead slots: renumbers the entries and the slots.
    pub(crate) fn compact_if_needed(&mut self, object: Gc<Object>) -> Result<()> {
        let (dictionary, slots) = self.dictionary_mut(object)?;
        if !dictionary.needs_compaction(slots.len()) {
            return Ok(());
        }
        let live = dictionary.entries.len() - dictionary.deleted as usize;
        let mut entries = Vec::with_capacity(live);
        let mut new_slots = Vec::with_capacity(slots.len() - dictionary.dead_slots as usize);
        for entry in dictionary
            .entries
            .iter()
            .filter(|entry| !entry.flags.deleted())
        {
            let start = entry.slot as usize;
            let values = slots
                .get(start..start + entry.flags.width() as usize)
                .ok_or(Error::invariant("a dictionary entry outside the slots"))?;
            entries.push(ShapeEntry {
                slot: new_slots.len() as u32,
                ..*entry
            });
            new_slots.extend_from_slice(values);
        }
        dictionary.index.clear();
        for (position, entry) in entries.iter().enumerate() {
            dictionary.index.insert(entry.key, position as u32);
        }
        dictionary.index.shrink_to_fit();
        dictionary.entries = entries;
        dictionary.deleted = 0;
        dictionary.dead_slots = 0;
        *slots = new_slots;
        Ok(())
    }
}

/// The approximate bytes of an entry of the transition index.
const TRANSITION_ENTRY_BYTES: usize = size_of::<(Transition, Gc<Shape>)>() + 1;

/// The approximate bytes of an entry of the root shape table.
const ROOT_TABLE_ENTRY_BYTES: usize = size_of::<(Option<Gc<Object>>, Gc<Shape>)>() + 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots};
    use crate::value::Value;

    fn object_with(heap: &mut Heap, proto: Option<Gc<Object>>, keys: &[&str]) -> Gc<Object> {
        let object = heap.new_object(proto).unwrap();
        for name in keys {
            let key = heap.key_from_str(name).unwrap();
            assert!(
                heap.create_data_property(object, key, Value::Int(1), &NoRoots)
                    .unwrap()
            );
        }
        object
    }

    fn shared(heap: &Heap, object: Gc<Object>) -> &SharedShape {
        let shape = heap.shape(heap.object(object).unwrap().shape).unwrap();
        match &shape.kind {
            ShapeKind::Shared(shared) => shared,
            ShapeKind::Dictionary(_) => panic!("a shared shape"),
        }
    }

    #[test]
    fn children_share_the_key_list_and_branches_copy_it() {
        let mut heap = Heap::new(HeapConfig::default());
        let abc = object_with(&mut heap, None, &["a", "b", "c"]);
        let ab = object_with(&mut heap, None, &["a", "b"]);
        assert_eq!(shared(&heap, abc).keys, shared(&heap, ab).keys);
        assert_eq!(heap.arenas.key_lists.len(), 1);
        // A branch after "a, b" copies the prefix into a new list.
        let abd = object_with(&mut heap, None, &["a", "b", "d"]);
        assert_ne!(shared(&heap, abd).keys, shared(&heap, abc).keys);
        assert_eq!(heap.arenas.key_lists.len(), 2);
        let slots = heap
            .shape(heap.object(abd).unwrap().shape)
            .unwrap()
            .entries(&heap.arenas.key_lists)
            .unwrap()
            .iter()
            .map(|entry| entry.slot)
            .collect::<Vec<_>>();
        assert_eq!(slots, [0, 1, 2]);
    }

    #[test]
    fn collection_trims_key_lists_so_they_extend_again() {
        let mut heap = Heap::new(HeapConfig::default());
        let a = object_with(&mut heap, None, &["a"]);
        object_with(&mut heap, None, &["a", "b", "c"]);
        let root = heap.persist(a.into());
        heap.collect(&NoRoots);
        let list = shared(&heap, a).keys;
        assert_eq!(heap.arenas.key_lists.get(list).unwrap().entries.len(), 1);
        // "a, x" extends the trimmed list instead of copying it.
        let ax = object_with(&mut heap, None, &["a", "x"]);
        assert_eq!(shared(&heap, ax).keys, list);
        assert_eq!(heap.arenas.key_lists.len(), 1);
        heap.release(root).unwrap();
    }

    #[test]
    fn large_shapes_build_an_index_on_the_second_lookup() {
        let mut heap = Heap::new(HeapConfig::default());
        let names: Vec<String> = (0..20).map(|i| format!("k{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let object = object_with(&mut heap, None, &refs);
        let key = heap.key_from_str("k13").unwrap();
        assert!(shared(&heap, object).index.get().is_none());
        heap.get_own_property(object, key).unwrap();
        heap.get_own_property(object, key).unwrap();
        assert!(shared(&heap, object).index.get().is_some());
        let missing = heap.key_from_str("zz").unwrap();
        assert_eq!(heap.get_own_property(object, missing).unwrap(), None);
        assert!(heap.get_own_property(object, key).unwrap().is_some());
    }

    #[test]
    fn root_shapes_are_per_prototype() {
        let mut heap = Heap::new(HeapConfig::default());
        let proto = heap.new_object(None).unwrap();
        let a = heap.new_object(Some(proto)).unwrap();
        let b = heap.new_object(Some(proto)).unwrap();
        let c = heap.new_object(None).unwrap();
        let shape = |heap: &Heap, o| heap.object(o).unwrap().shape;
        assert_eq!(shape(&heap, a), shape(&heap, b));
        assert_eq!(shape(&heap, c), shape(&heap, proto));
        assert_ne!(shape(&heap, a), shape(&heap, c));
        assert_eq!(heap.root_shapes.len(), 2);
    }
}
