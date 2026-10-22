//! Inline caches of the property sites (ADR 0026 sections 6 and 15, M7
//! 13a; memo 3.3; the monomorphic inline cache of Deutsch and Schiffman,
//! 1984).
//!
//! Each property site of a code object has one entry, a [`SiteCache`]: a
//! small `Copy` value that the code object keeps in a `Cell`
//! (`bytecode.rs`). This module holds the entries' meaning: the checks of
//! a hit, and the walks that compute an entry on a miss together with
//! the result of the access.
//!
//! Validation:
//!
//! - The receiver: its shape handle. Handles carry the generation of
//!   their slot, so a shape that the collector freed (and whose slot holds
//!   another shape now) never matches. Only shared shapes are stored: a
//!   dictionary changes in place, its handle proves nothing.
//! - The prototype chain: a counter, the watch epoch of the heap. Every
//!   object that is or was the prototype of a shape is *watched* (the
//!   flag is set when its root shape is made, and when
//!   `Object.setPrototypeOf` puts it into a dictionary shape), and so is
//!   each realm's global object. A change of the keys, attributes or
//!   prototype of a watched object increments the epoch; a change of a
//!   value does not, because the entries read values from the slots at
//!   each hit. An entry that depends on the chain stores the epoch of its
//!   walk. While the epoch is unchanged and the receiver has the cached
//!   shape (which fixes its first prototype), every object on the path
//!   has the keys, attributes and prototype of the walk, so the walk
//!   would find the same holder and slot.
//! - Kinds: the shape does not fix the kind of an object (an array and
//!   `Object.create(Array.prototype)` share the root shape). Proxies and
//!   host objects never match; a typed array matches an entry of the
//!   chain only when the walk proved that the key is not a canonical
//!   numeric string; `length` is never cached through the chain (an
//!   array's `length` is not in its shape).
//!
//! Exotic objects on the walked path (a proxy, a host object, a typed
//! array with a numeric key, an array's `length`) give no entry; the
//! access then takes the general path as before.

use crate::error::Result;
use crate::heap::{Gc, Heap, RootSource};
use crate::object::{GetResult, Object, ObjectKind, Property, Shape, ShapeKind};
use crate::string::PropertyKey;
use crate::value::Value;

/// The inline cache entry of a property site.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum SiteCache {
    /// Nothing cached.
    #[default]
    Empty,
    /// An own data property of receivers with `shape` at `slot`; for a
    /// write, a writable one.
    Own { shape: Gc<Shape>, slot: u32 },
    /// An own accessor of receivers with `shape`: the getter at `slot`,
    /// the setter at `slot + 1`.
    OwnAccessor { shape: Gc<Shape>, slot: u32 },
    /// A property of `holder` on the prototype chain of receivers with
    /// `shape`: a data value at `slot`, or an accessor (getter at `slot`,
    /// setter at `slot + 1`).
    Proto {
        shape: Gc<Shape>,
        holder: Gc<Object>,
        slot: u32,
        accessor: bool,
        /// Typed arrays may match (the key is not numeric).
        typed: bool,
        epoch: u32,
    },
    /// No property on the chain of receivers with `shape` (a read gives
    /// `undefined`).
    Absent {
        shape: Gc<Shape>,
        typed: bool,
        epoch: u32,
    },
    /// A write that adds a data property: receivers with `shape` (no
    /// setter and no read-only property on the chain) move to `to`.
    Add {
        shape: Gc<Shape>,
        to: Gc<Shape>,
        epoch: u32,
    },
    /// `length` of an array.
    ArrayLength,
    /// `length` of a typed array with `shape`: the accessor of `holder` on
    /// the chain (the getter at `slot`). Only typed arrays match: an array
    /// with the same shape has a `length` of its own.
    TypedLength {
        shape: Gc<Shape>,
        holder: Gc<Object>,
        slot: u32,
        epoch: u32,
    },
    /// The receiver is a proxy: the general path from the start, without
    /// the walk of a miss.
    Proxy,
    /// A global `let`, `const` or `class` binding: its position in the
    /// declarative record of `realm`.
    GlobalLexical { realm: u32, index: u32 },
    /// A property of the global object of `realm` or of its prototype
    /// chain: `holder` at `slot`, a data value or an accessor. Valid while
    /// the realm has `lexicals` global lexical bindings (a new one could
    /// shadow the name) and the epoch is unchanged.
    GlobalProperty {
        realm: u32,
        lexicals: u32,
        holder: Gc<Object>,
        slot: u32,
        accessor: bool,
        epoch: u32,
    },
    /// The final shape of an object literal whose prototype is `proto`.
    Literal { proto: Gc<Object>, shape: Gc<Shape> },
}

impl SiteCache {
    /// Whether the entry depends on a watch epoch that can no longer
    /// change (`u32::MAX`, after four billion changes of prototypes): such
    /// an entry is never stored, because no change would invalidate it.
    pub(crate) fn exhausted(self) -> bool {
        match self {
            SiteCache::Proto { epoch, .. }
            | SiteCache::Absent { epoch, .. }
            | SiteCache::TypedLength { epoch, .. }
            | SiteCache::Add { epoch, .. }
            | SiteCache::GlobalProperty { epoch, .. } => epoch == u32::MAX,
            _ => false,
        }
    }
}

/// What a write through a site does, decided by one walk
/// ([`Heap::set_plan`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SetPlan {
    /// Write the own writable data property at this slot.
    Own(u32),
    /// Call this setter with the receiver.
    Setter(Value),
    /// Add a data property to the receiver (it is extensible and
    /// ordinary, the chain has no setter and no read-only property).
    Add,
    /// Anything else: the general `[[Set]]`.
    General,
}

/// Whether an object of this kind can match an entry of its own
/// properties. Proxies have no named properties in their shape and host
/// objects can have their own internal methods (ADR 0026 section 12).
#[inline]
fn own_cacheable(kind: &ObjectKind) -> bool {
    !matches!(kind, ObjectKind::Proxy(_) | ObjectKind::Host { .. })
}

/// Whether an object of this kind can match an entry of its prototype
/// chain; `typed` says that the key is not a canonical numeric string.
#[inline]
fn chain_cacheable(kind: &ObjectKind, typed: bool) -> bool {
    match kind {
        ObjectKind::Proxy(_) | ObjectKind::Host { .. } => false,
        ObjectKind::TypedArray(_) => typed,
        _ => true,
    }
}

/// The result of reading an accessor's getter: `undefined` without one.
#[inline]
fn getter_result(getter: Value, receiver: Value) -> GetResult {
    if getter.is_undefined() {
        GetResult::Value(Value::Undefined)
    } else {
        GetResult::CallGetter { getter, receiver }
    }
}

impl Heap {
    /// The watch epoch (see the module comment).
    #[inline]
    pub(crate) fn watch_epoch(&self) -> u32 {
        self.watch_epoch
    }

    /// Marks an object as watched: a later change of its keys,
    /// attributes or prototype increments the watch epoch.
    pub(crate) fn watch(&mut self, object: Gc<Object>) -> Result<()> {
        self.object_mut(object)?.watched = true;
        Ok(())
    }

    /// Records a change of the keys, attributes or prototype of `object`.
    #[inline]
    pub(crate) fn layout_changed(&mut self, object: Gc<Object>) -> Result<()> {
        if self.object(object)?.watched {
            // At `u32::MAX` the epoch stays: entries of the chain are no
            // longer made then (`SiteCache::exhausted`).
            self.watch_epoch = self.watch_epoch.saturating_add(1);
        }
        Ok(())
    }

    /// The value of an own data property through an `Own` entry (the
    /// most common hit, kept apart so that it returns one value): `None`
    /// for any other entry and on a miss.
    #[inline]
    pub(crate) fn cached_own_value(
        &self,
        object: Gc<Object>,
        entry: SiteCache,
    ) -> Result<Option<Value>> {
        let SiteCache::Own { shape, slot } = entry else {
            return Ok(None);
        };
        let record = self.object(object)?;
        if record.shape != shape || !own_cacheable(&record.kind) {
            return Ok(None);
        }
        Ok(record.slots.get(slot as usize).copied())
    }

    /// `[[Get]]` of `object` through a cache entry: the result on a hit,
    /// `None` on a miss. The receiver is the object.
    #[inline]
    pub(crate) fn cached_get(
        &self,
        object: Gc<Object>,
        entry: SiteCache,
    ) -> Result<Option<GetResult>> {
        if matches!(entry, SiteCache::Empty) {
            return Ok(None);
        }
        let record = self.object(object)?;
        let receiver = Value::Object(object);
        Ok(match entry {
            // `SiteCache::Own` is `cached_own_value`.
            SiteCache::OwnAccessor { shape, slot }
                if record.shape == shape && own_cacheable(&record.kind) =>
            {
                record
                    .slots
                    .get(slot as usize)
                    .map(|&getter| getter_result(getter, receiver))
            }
            SiteCache::Proto {
                shape,
                holder,
                slot,
                accessor,
                typed,
                epoch,
            } if record.shape == shape
                && epoch == self.watch_epoch
                && chain_cacheable(&record.kind, typed) =>
            {
                let value = self.object(holder)?.slots.get(slot as usize).copied();
                match value {
                    Some(getter) if accessor => Some(getter_result(getter, receiver)),
                    Some(value) => Some(GetResult::Value(value)),
                    None => None,
                }
            }
            SiteCache::Absent {
                shape,
                typed,
                epoch,
            } if record.shape == shape
                && epoch == self.watch_epoch
                && chain_cacheable(&record.kind, typed) =>
            {
                Some(GetResult::Value(Value::Undefined))
            }
            SiteCache::ArrayLength => match record.kind {
                ObjectKind::Array { length, .. } => {
                    Some(GetResult::Value(super::ops::length_value(length)))
                }
                _ => None,
            },
            SiteCache::Proxy => match record.kind {
                ObjectKind::Proxy(_) => Some(GetResult::Generic),
                _ => None,
            },
            SiteCache::TypedLength {
                shape,
                holder,
                slot,
                epoch,
            } if record.shape == shape
                && epoch == self.watch_epoch
                && matches!(record.kind, ObjectKind::TypedArray(_)) =>
            {
                let getter = self.object(holder)?.slots.get(slot as usize).copied();
                getter.map(|getter| getter_result(getter, receiver))
            }
            _ => None,
        })
    }

    /// `[[Get]]` (§10.1.8) of a string or symbol key with the receiver
    /// `object` that also computes the cache entry for the result, in one
    /// walk. The result is the one of [`Heap::get`].
    #[inline]
    pub(crate) fn get_for_cache(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<(GetResult, SiteCache)> {
        let receiver = Value::Object(object);
        let is_length = key == PropertyKey::String(self.names.length);
        let mut receiver_shape = None;
        let mut receiver_typed = false;
        let mut cacheable = true;
        let mut typed = false;
        let mut current = object;
        let mut depth = 0u32;
        loop {
            let record = self.object(current)?;
            if record.kind.walks_exotic() {
                if let Some(found) = self.exotic_get(record, key)? {
                    let entry = match record.kind {
                        ObjectKind::Proxy(_) if depth == 0 => SiteCache::Proxy,
                        _ => SiteCache::Empty,
                    };
                    return Ok((found, entry));
                }
                // A typed array, and the key is not numeric.
                typed = true;
            }
            // A prototype that is not watched cannot happen (see the module
            // comment); it would make the entry unsafe.
            if matches!(record.kind, ObjectKind::Host { .. }) || (depth > 0 && !record.watched) {
                cacheable = false;
            }
            if is_length && let ObjectKind::Array { length, .. } = record.kind {
                let entry = if depth == 0 {
                    SiteCache::ArrayLength
                } else {
                    SiteCache::Empty
                };
                return Ok((GetResult::Value(super::ops::length_value(length)), entry));
            }
            let shape = self.arenas.shapes.get(record.shape)?;
            if depth == 0 {
                receiver_typed = matches!(record.kind, ObjectKind::TypedArray(_));
                receiver_shape = Some(record.shape);
                cacheable &= !shape.is_dictionary();
            }
            let Some(receiver_shape) = receiver_shape else {
                return Err(crate::error::Error::invariant("a walk without a receiver"));
            };
            if let Some(found) = shape.find(key, &self.arenas.key_lists)? {
                let slot = found.slot;
                let first = record.slots.get(slot as usize).copied().ok_or(
                    crate::error::Error::invariant("a shape entry outside the slots"),
                )?;
                let accessor = found.flags.accessor();
                let result = if accessor {
                    getter_result(first, receiver)
                } else {
                    GetResult::Value(first)
                };
                let entry = match (cacheable, depth, accessor) {
                    (false, ..) => SiteCache::Empty,
                    (true, 0, false) => SiteCache::Own {
                        shape: receiver_shape,
                        slot,
                    },
                    (true, 0, true) => SiteCache::OwnAccessor {
                        shape: receiver_shape,
                        slot,
                    },
                    // `length` of a typed array: its accessor on the chain.
                    (true, _, true) if is_length && receiver_typed => SiteCache::TypedLength {
                        shape: receiver_shape,
                        holder: current,
                        slot,
                        epoch: self.watch_epoch,
                    },
                    // An array with the receiver's shape has its own
                    // `length`, which is not in the shape.
                    _ if is_length => SiteCache::Empty,
                    (true, _, accessor) => SiteCache::Proto {
                        shape: receiver_shape,
                        holder: current,
                        slot,
                        accessor,
                        typed,
                        epoch: self.watch_epoch,
                    },
                };
                return Ok((result, entry));
            }
            let Some(proto) = shape.proto else {
                let entry = if cacheable && !is_length {
                    SiteCache::Absent {
                        shape: receiver_shape,
                        typed,
                        epoch: self.watch_epoch,
                    }
                } else {
                    SiteCache::Empty
                };
                return Ok((GetResult::Value(Value::Undefined), entry));
            };
            current = proto;
            depth += 1;
        }
    }

    /// A write of an own writable data property through an `Own` entry
    /// (the most common hit): true if it wrote the value.
    #[inline]
    pub(crate) fn cached_own_write(
        &mut self,
        object: Gc<Object>,
        entry: SiteCache,
        value: Value,
    ) -> Result<bool> {
        let SiteCache::Own { shape, slot } = entry else {
            return Ok(false);
        };
        let record = self.object_mut(object)?;
        if record.shape != shape || !own_cacheable(&record.kind) {
            return Ok(false);
        }
        Ok(match record.slots.get_mut(slot as usize) {
            Some(stored) => {
                *stored = value;
                true
            }
            None => false,
        })
    }

    /// The step of a cached write `object.key = value` (receiver `object`)
    /// for a hit of `entry` other than `Own`: `Some(plan)` on a hit
    /// (`SetPlan::Add` needs [`Heap::add_by_transition`]), `None` on a
    /// miss.
    #[inline]
    pub(crate) fn cached_set(
        &self,
        object: Gc<Object>,
        entry: SiteCache,
    ) -> Result<Option<SetPlan>> {
        if matches!(entry, SiteCache::Empty) {
            return Ok(None);
        }
        let epoch = self.watch_epoch;
        let record = self.object(object)?;
        Ok(match entry {
            // `SiteCache::Own` is `cached_own_write`.
            SiteCache::OwnAccessor { shape, slot }
                if record.shape == shape && own_cacheable(&record.kind) =>
            {
                setter_plan(record.slots.get(slot as usize + 1).copied())
            }
            SiteCache::Proto {
                shape,
                holder,
                slot,
                accessor: true,
                typed,
                epoch: cached,
            } if record.shape == shape
                && cached == epoch
                && chain_cacheable(&record.kind, typed) =>
            {
                setter_plan(self.object(holder)?.slots.get(slot as usize + 1).copied())
            }
            SiteCache::Add {
                shape,
                epoch: cached,
                ..
            } if record.shape == shape
                && cached == epoch
                && record.extensible
                && chain_cacheable(&record.kind, false) =>
            {
                Some(SetPlan::Add)
            }
            SiteCache::Proxy => match record.kind {
                ObjectKind::Proxy(_) => Some(SetPlan::General),
                _ => None,
            },
            _ => None,
        })
    }

    /// The plan of `object.key = value` with the receiver `object`
    /// (`OrdinarySet`, §10.1.9.2), from one walk of the chain, and the
    /// cache entry for it (`SetPlan::Add` gets its entry after the
    /// property is added, see [`Heap::add_entry`]). Nothing is written.
    pub(crate) fn set_plan(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<(SetPlan, SiteCache)> {
        const GENERAL: (SetPlan, SiteCache) = (SetPlan::General, SiteCache::Empty);
        if let PropertyKey::Index(_) = key {
            return Ok(GENERAL);
        }
        let is_length = key == PropertyKey::String(self.names.length);
        let record = self.object(object)?;
        if let ObjectKind::Proxy(_) = record.kind {
            return Ok((SetPlan::General, SiteCache::Proxy));
        }
        if !chain_cacheable(&record.kind, false)
            || (is_length && matches!(record.kind, ObjectKind::Array { .. }))
        {
            return Ok(GENERAL);
        }
        let receiver_shape = record.shape;
        let shape = self.arenas.shapes.get(receiver_shape)?;
        if shape.is_dictionary() {
            return Ok(GENERAL);
        }
        if let Some(found) = shape.find(key, &self.arenas.key_lists)? {
            let slot = found.slot;
            if found.flags.accessor() {
                let setter = record.slots.get(slot as usize + 1).copied();
                return Ok(match setter_plan(setter) {
                    Some(plan) => (
                        plan,
                        SiteCache::OwnAccessor {
                            shape: receiver_shape,
                            slot,
                        },
                    ),
                    None => GENERAL,
                });
            }
            if !found.flags.writable() {
                return Ok(GENERAL);
            }
            return Ok((
                SetPlan::Own(slot),
                SiteCache::Own {
                    shape: receiver_shape,
                    slot,
                },
            ));
        }
        let extensible = record.extensible;
        let mut typed = false;
        let mut next = shape.proto;
        while let Some(current) = next {
            let record = self.object(current)?;
            if !record.watched {
                return Ok(GENERAL);
            }
            match record.kind {
                ObjectKind::Proxy(_) | ObjectKind::Host { .. } => return Ok(GENERAL),
                ObjectKind::TypedArray(_) => {
                    if self.numeric_index(key)?.is_some() {
                        return Ok(GENERAL);
                    }
                    typed = true;
                }
                ObjectKind::Array { .. } if is_length => return Ok(GENERAL),
                _ => {}
            }
            let shape = self.arenas.shapes.get(record.shape)?;
            if let Some(found) = shape.find(key, &self.arenas.key_lists)? {
                if found.flags.accessor() {
                    let setter = record.slots.get(found.slot as usize + 1).copied();
                    return Ok(match setter_plan(setter) {
                        Some(plan) => (
                            plan,
                            SiteCache::Proto {
                                shape: receiver_shape,
                                holder: current,
                                slot: found.slot,
                                accessor: true,
                                typed,
                                epoch: self.watch_epoch,
                            },
                        ),
                        None => GENERAL,
                    });
                }
                if !found.flags.writable() {
                    return Ok(GENERAL);
                }
                break;
            }
            next = shape.proto;
        }
        if !extensible || is_length {
            return Ok(GENERAL);
        }
        Ok((SetPlan::Add, SiteCache::Empty))
    }

    /// Adds the data property `key` with `value` to `object` (writable,
    /// enumerable, configurable; `CreateDataProperty` of an ordinary
    /// object that does not have it) and returns the cache entry of the
    /// transition, if the object stayed in shared shapes and is not
    /// watched. Can collect.
    pub(crate) fn add_entry(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        epoch: u32,
        roots: &dyn RootSource,
    ) -> Result<SiteCache> {
        let record = self.object(object)?;
        let (shape, watched) = (record.shape, record.watched);
        let property = Property::Data {
            value,
            writable: true,
            enumerable: true,
            configurable: true,
        };
        self.add_named(object, key, property, roots)?;
        if watched {
            // The add to a watched object incremented the epoch, so an
            // entry with `epoch` would never hit.
            return Ok(SiteCache::Empty);
        }
        let to = self.object(object)?.shape;
        Ok(match &self.arenas.shapes.get(to)?.kind {
            ShapeKind::Shared(shared) if shared.parent == Some(shape) => {
                SiteCache::Add { shape, to, epoch }
            }
            _ => SiteCache::Empty,
        })
    }

    /// Adds a data property to `object` along the cached transition to
    /// `to` (the object has the transition's parent shape and is
    /// extensible). Returns false, with nothing changed, when the shape
    /// `to` no longer exists (the caller takes the general path). Can
    /// collect when the slots must grow.
    pub(crate) fn add_by_transition(
        &mut self,
        object: Gc<Object>,
        to: Gc<Shape>,
        value: Value,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        let record = self.object(object)?;
        if record.slots.len() == record.slots.capacity() {
            let estimate = self.object_bytes(object)?.max(64);
            self.reserve_with(estimate, roots, &[object.into(), value])?;
        }
        let slot_count = match &self.arenas.shapes.get(to) {
            Ok(Shape {
                kind: ShapeKind::Shared(shared),
                ..
            }) => shared.slot_count as usize,
            _ => return Ok(false),
        };
        let before = self.object_bytes(object)?;
        let record = self.object_mut(object)?;
        if record.slots.len() + 1 != slot_count {
            return Err(crate::error::Error::invariant(
                "a cached transition that does not fit the slots",
            ));
        }
        record.slots.try_reserve(1)?;
        record.slots.push(value);
        record.shape = to;
        // A prototype or a global object can be the receiver.
        self.layout_changed(object)?;
        self.charge_growth(object, before)?;
        Ok(true)
    }

    /// The shape of an object literal with the data properties `keys` (in
    /// order, without duplicates) and the prototype `proto`: the root
    /// shape of `proto` and one transition per key, as the definitions one
    /// by one would make them. `None` if a key is an index or the shape
    /// would be a dictionary. Never collects.
    pub(crate) fn literal_shape(
        &mut self,
        proto: Gc<Object>,
        keys: &[PropertyKey],
    ) -> Result<Option<Gc<Shape>>> {
        if keys.len() > super::shape::DICTIONARY_LIMIT as usize
            || keys.iter().any(|key| matches!(key, PropertyKey::Index(_)))
        {
            return Ok(None);
        }
        let mut shape = self.root_shape(Some(proto))?;
        let flags = super::property::Flags::data(true, true, true);
        for &key in keys {
            shape = self.shape_with_added(shape, key, flags)?;
        }
        Ok(Some(shape))
    }

    /// Whether a shape handle is still alive.
    #[inline]
    pub(crate) fn shape_alive(&self, shape: Gc<Shape>) -> bool {
        self.arenas.shapes.contains(shape)
    }
}

/// The plan for an accessor's setter: `None` without one (a failed
/// write, which the general path reports).
fn setter_plan(setter: Option<Value>) -> Option<SetPlan> {
    match setter {
        Some(setter) if !setter.is_undefined() => Some(SetPlan::Setter(setter)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots};

    fn heap() -> Heap {
        Heap::new(HeapConfig::default())
    }

    fn put(heap: &mut Heap, object: Gc<Object>, name: &str, value: Value) -> PropertyKey {
        let key = heap.key_from_str(name).unwrap();
        assert!(
            heap.create_data_property(object, key, value, &NoRoots)
                .unwrap()
        );
        key
    }

    #[test]
    fn prototypes_are_watched_and_their_changes_count() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        assert!(!heap.object(proto).unwrap().watched);
        let object = heap.new_object(Some(proto)).unwrap();
        assert!(heap.object(proto).unwrap().watched);
        let epoch = heap.watch_epoch();
        // A change of the receiver does not count, one of the prototype
        // does; a value written in place does not.
        put(&mut heap, object, "a", Value::Int(1));
        assert_eq!(heap.watch_epoch(), epoch);
        let key = put(&mut heap, proto, "p", Value::Int(1));
        assert_eq!(heap.watch_epoch(), epoch + 1);
        put(&mut heap, proto, "p", Value::Int(2));
        assert_eq!(heap.watch_epoch(), epoch + 1);
        assert!(heap.delete(proto, key).unwrap());
        assert_eq!(heap.watch_epoch(), epoch + 2);
        // The dictionary branch of [[SetPrototypeOf]] watches the new
        // prototype.
        let other = heap.new_object(None).unwrap();
        assert!(heap.set_prototype_of(object, Some(other)).unwrap());
        assert!(heap.object(other).unwrap().watched);
    }

    #[test]
    fn entries_hit_until_the_chain_changes() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let key = put(&mut heap, proto, "m", Value::Int(7));
        let object = heap.new_object(Some(proto)).unwrap();
        let (found, entry) = heap.get_for_cache(object, key).unwrap();
        assert_eq!(found, GetResult::Value(Value::Int(7)));
        assert!(matches!(entry, SiteCache::Proto { .. }));
        assert_eq!(heap.cached_get(object, entry).unwrap(), Some(found));
        // An own property with the key changes the receiver's shape.
        let other = heap.new_object(Some(proto)).unwrap();
        put(&mut heap, other, "m", Value::Int(1));
        assert_eq!(heap.cached_get(other, entry).unwrap(), None);
        // A new key on the prototype changes the epoch.
        put(&mut heap, proto, "n", Value::Int(1));
        assert_eq!(heap.cached_get(object, entry).unwrap(), None);
        let (_, entry) = heap.get_for_cache(object, key).unwrap();
        assert!(heap.cached_get(object, entry).unwrap().is_some());
    }

    #[test]
    fn an_entry_never_matches_a_shape_in_a_reused_slot() {
        let mut heap = heap();
        let object = heap.new_object(None).unwrap();
        let key = put(&mut heap, object, "x", Value::Int(1));
        let (_, entry) = heap.get_for_cache(object, key).unwrap();
        let SiteCache::Own { shape, .. } = entry else {
            panic!("an own entry");
        };
        heap.collect(&NoRoots);
        assert!(!heap.shape_alive(shape));
        // A new object whose shapes take the freed slots.
        let fresh = heap.new_object(None).unwrap();
        put(&mut heap, fresh, "y", Value::Int(2));
        // (The collection freed the atom of the old key too.)
        let key = put(&mut heap, fresh, "x", Value::Int(3));
        assert_eq!(heap.cached_own_value(fresh, entry).unwrap(), None);
        let (found, _) = heap.get_for_cache(fresh, key).unwrap();
        assert_eq!(found, GetResult::Value(Value::Int(3)));
    }

    #[test]
    fn dictionaries_and_array_length_on_the_chain_give_no_entry() {
        let mut heap = heap();
        let object = heap.new_object(None).unwrap();
        let key = put(&mut heap, object, "x", Value::Int(1));
        heap.make_dictionary(object).unwrap();
        assert_eq!(heap.get_for_cache(object, key).unwrap().1, SiteCache::Empty);
        let array = heap.new_array(None, 3).unwrap();
        let child = heap.new_object(Some(array)).unwrap();
        let length = PropertyKey::String(heap.names.length);
        let (found, entry) = heap.get_for_cache(child, length).unwrap();
        assert_eq!(found, GetResult::Value(Value::Int(3)));
        assert_eq!(entry, SiteCache::Empty);
        assert_eq!(
            heap.get_for_cache(array, length).unwrap().1,
            SiteCache::ArrayLength
        );
    }

    #[test]
    fn entries_stay_small() {
        assert_eq!(size_of::<std::cell::Cell<SiteCache>>(), 28);
    }

    #[test]
    fn an_exhausted_epoch_makes_no_chain_entries() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let key = put(&mut heap, proto, "m", Value::Int(7));
        let object = heap.new_object(Some(proto)).unwrap();
        heap.watch_epoch = u32::MAX - 1;
        put(&mut heap, proto, "n", Value::Int(1));
        put(&mut heap, proto, "o", Value::Int(1));
        assert_eq!(heap.watch_epoch(), u32::MAX);
        let (_, entry) = heap.get_for_cache(object, key).unwrap();
        assert!(entry.exhausted());
    }
}
