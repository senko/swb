//! Plain arrays for `ArraySpeciesCreate` and `IsConcatSpreadable` (M8
//! feature 11a, ADR 0026 section 15; ECMA-262 §10.4.2.3, §23.1.3.1.1).
//!
//! For an array of the running realm whose `constructor` is the realm's
//! `Array` with the built-in `@@species` getter, `ArraySpeciesCreate`
//! reads two properties without effects, calls the getter (which returns
//! `this`) and constructs `Array`: the result is `ArrayCreate(length)`.
//! For such an array without `@@isConcatSpreadable` on its chain,
//! `IsConcatSpreadable` is `IsArray`, which is true. `concat`, `slice`,
//! `filter`, `map`, `splice` and `flat` call these for every call, so the
//! conditions are cached, as for array patterns (`array_pattern.rs`):
//!
//! - the value is an Array (the kind: `Object.create(Array.prototype)` has
//!   the same shape) whose shape is the one that the cache holds: shared,
//!   with `Array.prototype` of the running realm as its prototype, and
//!   neither `constructor` nor `@@isConcatSpreadable` as own keys;
//! - `Array.prototype.constructor` is a data property that holds the
//!   realm's `Array`;
//! - `Array[@@species]` is an accessor whose getter is the realm's
//!   built-in one;
//! - `Array.prototype` has `%Object.prototype%` as its prototype, which
//!   has `null`, and neither has `@@isConcatSpreadable`.
//!
//! The watch epoch (`object/cache.rs`) stands for the keys, attributes
//! and prototypes of `Array.prototype`, `Object.prototype` and `Array`:
//! they are watched, and the cache is valid only at the epoch of its walk.
//! The values of `constructor` and of the getter change without the
//! epoch, so each use reads their slots again.

use crate::error::Result;
use crate::heap::Gc;
use crate::object::{Object, ObjectKind, Shape};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, WellKnown};

/// What a walk found out about the arrays of one shape (see the module
/// comment); valid while the watch epoch is `epoch`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlainArrayCache {
    epoch: u32,
    realm: u32,
    /// The shape of the arrays that it is valid for.
    shape: Gc<Shape>,
    /// `Array.prototype` and the slot of its `constructor`.
    array_proto: Gc<Object>,
    constructor_slot: u32,
    /// `Array`, the slot of the getter of its `@@species`, and the
    /// built-in getter.
    array: Gc<Object>,
    species_slot: u32,
    getter: Gc<Object>,
}

impl Runtime {
    /// Whether `object` is a plain array of the running realm (see the
    /// module comment): `ArraySpeciesCreate(object, n)` is
    /// `ArrayCreate(n)` and `IsConcatSpreadable(object)` is true. Takes
    /// the walk when the cache does not fit; `false` when it fails.
    pub(crate) fn plain_array(&mut self, object: Gc<Object>) -> bool {
        let Ok(record) = self.heap.object(object) else {
            return false;
        };
        if !matches!(record.kind, ObjectKind::Array { .. }) {
            return false;
        }
        let shape = record.shape;
        let epoch = self.heap.watch_epoch();
        let realm = self.current_realm();
        let fits = |c: &PlainArrayCache| c.epoch == epoch && c.shape == shape && c.realm == realm;
        if !self.vm.plain_array.as_ref().is_some_and(fits) {
            self.vm.plain_array = self.walk_plain_array(shape, realm).ok().flatten();
        }
        match self.vm.plain_array {
            Some(cache) => self.plain_values_hold(&cache),
            None => false,
        }
    }

    /// Whether `constructor` and the getter still hold the built-in
    /// functions.
    fn plain_values_hold(&self, cache: &PlainArrayCache) -> bool {
        let holds = |holder: Gc<Object>, slot: u32, function: Gc<Object>| {
            self.heap
                .object(holder)
                .ok()
                .and_then(|record| record.slots.get(slot as usize).copied())
                .is_some_and(|v| matches!(v, Value::Object(o) if o == function))
        };
        holds(cache.array_proto, cache.constructor_slot, cache.array)
            && holds(cache.array, cache.species_slot, cache.getter)
    }

    /// The walk of the module comment for arrays with `shape`; `None`
    /// when a condition fails. Marks the three objects watched.
    fn walk_plain_array(
        &mut self,
        shape: Gc<Shape>,
        realm: u32,
    ) -> Result<Option<PlainArrayCache>> {
        let epoch = self.heap.watch_epoch();
        if epoch == u32::MAX {
            return Ok(None);
        }
        let (Ok(array_proto), Ok(object_proto), Ok(array), Ok(getter)) = (
            self.intrinsic(realm, Intrinsic::ArrayPrototype),
            self.intrinsic(realm, Intrinsic::ObjectPrototype),
            self.intrinsic(realm, Intrinsic::ArrayConstructor),
            self.intrinsic(realm, Intrinsic::ArraySpeciesGetter),
        ) else {
            return Ok(None);
        };
        let constructor_key = PropertyKey::String(self.vm.atoms.constructor);
        let spreadable_key = self.symbol_key(WellKnown::IsConcatSpreadable);
        let species_key = self.symbol_key(WellKnown::Species);
        let heap = &self.heap;
        let array_shape = heap.arenas.shapes.get(shape)?;
        if array_shape.is_dictionary()
            || array_shape.proto != Some(array_proto)
            || array_shape
                .find(constructor_key, &heap.arenas.key_lists)?
                .is_some()
            || array_shape
                .find(spreadable_key, &heap.arenas.key_lists)?
                .is_some()
        {
            return Ok(None);
        }
        let Some(constructor_slot) = heap.own_data_slot(array_proto, constructor_key)? else {
            return Ok(None);
        };
        let array_record = heap.object(array)?;
        let Some(species) = heap
            .arenas
            .shapes
            .get(array_record.shape)?
            .find(species_key, &heap.arenas.key_lists)?
            .filter(|entry| entry.flags.accessor())
        else {
            return Ok(None);
        };
        if heap.get_prototype_of(array_proto)? != Some(object_proto)
            || heap.get_prototype_of(object_proto)?.is_some()
            || heap.has_shape_key(array_proto, spreadable_key)?
            || heap.has_shape_key(object_proto, spreadable_key)?
        {
            return Ok(None);
        }
        for object in [array_proto, object_proto, array] {
            self.heap.watch(object)?;
        }
        Ok(Some(PlainArrayCache {
            epoch,
            realm,
            shape,
            array_proto,
            constructor_slot,
            array,
            species_slot: species.slot,
            getter,
        }))
    }
}
