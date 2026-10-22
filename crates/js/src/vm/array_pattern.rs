//! Array destructuring without an Array Iterator (M7 13e, ADR 0026
//! section 15; ECMA-262 §8.6.3 `IteratorBindingInitialization`, §7.4
//! iterator operations, §23.1.5 Array Iterator objects).
//!
//! `const [a, b] = pair` creates an Array Iterator, steps it twice and
//! closes it. For an ordinary array whose iteration is the built-in one,
//! none of that can be seen, so the record of the pattern holds the array
//! itself (`GetIterator` with `HINT_PATTERN`) and its state counts the
//! next index (`ARRAY_PATTERN + i`). The conditions, all checked at
//! `GetIterator`:
//!
//! - the value is an Array (the kind, not only the shape: `Object.create
//!   (Array.prototype)` has the same shape) whose shape is the one that the
//!   cache holds: shared, with `Array.prototype` of the running realm as
//!   its prototype and no own `@@iterator`;
//! - `Array.prototype[@@iterator]` is a data property that holds the
//!   intrinsic `%Array.prototype.values%`;
//! - `%ArrayIteratorPrototype%.next` is a data property that holds the
//!   intrinsic `%ArrayIteratorPrototype%.next` (the record caches `next`
//!   at this point, so a later change of it does not matter);
//! - the close would find no `return` method: `%ArrayIteratorPrototype%`,
//!   `%IteratorPrototype%` and `%Object.prototype%` have no own `return`,
//!   and the chain between them is the standard one.
//!
//! The watch epoch (`object/cache.rs`) stands for the keys, the
//! attributes and the prototypes of the four prototype objects: they are
//! watched, and the cache is valid only at the epoch of its walk. The
//! values of the two data properties change without the epoch, so each
//! use reads them again.
//!
//! Every step does what `next` does: it reads `length` of the array, ends
//! the record when the index is past it, and takes the element with
//! `[[Get]]` (holes and indices past the dense part read through the
//! prototype chain; getters run in order). A step that fails leaves the
//! record done, as for the iterator. The close, after the pattern or
//! after an abrupt completion, is a no-op when the epoch is still the
//! cache's (still no `return`); otherwise the array iterator that the
//! record stands for is made now, at the index where the pattern stopped,
//! and closed in the ordinary way, so that `return` sees what it would
//! have seen. A default value or a target can run script code between
//! the steps, so the check at the close is needed.

use crate::error::Result;
use crate::heap::Gc;
use crate::object::{IterationKind, Object, ObjectKind, Shape};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

/// What a walk found out about plain arrays and the iteration objects
/// (see the module comment); valid while the watch epoch is `epoch`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArrayPatternCache {
    epoch: u32,
    realm: u32,
    /// The shape of the arrays that it is valid for.
    shape: Gc<Shape>,
    /// `Array.prototype`, the slot of `@@iterator` and the function it
    /// must hold.
    array_proto: Gc<Object>,
    values_slot: u32,
    values: Gc<Object>,
    /// `%ArrayIteratorPrototype%`, the slot of `next` and the function it
    /// must hold.
    iterator_proto: Gc<Object>,
    next_slot: u32,
    next: Gc<Object>,
}

impl Runtime {
    /// Whether `value` is an array that an array pattern can step
    /// directly (see the module comment). Takes the walk when the cache
    /// does not fit.
    pub(crate) fn plain_array_pattern(&mut self, value: Value) -> bool {
        let Value::Object(array) = value else {
            return false;
        };
        let Ok(record) = self.heap.object(array) else {
            return false;
        };
        if !matches!(record.kind, ObjectKind::Array { .. }) {
            return false;
        }
        let shape = record.shape;
        let epoch = self.heap.watch_epoch();
        let realm = self.current_realm();
        let fits = |c: &ArrayPatternCache| c.epoch == epoch && c.shape == shape && c.realm == realm;
        if !self.vm.array_pattern.as_ref().is_some_and(fits) {
            self.vm.array_pattern = self.walk_array_pattern(shape, realm).ok().flatten();
        }
        match self.vm.array_pattern {
            Some(cache) => self.cache_values_hold(&cache),
            None => false,
        }
    }

    /// Whether the two data properties still hold the built-in functions.
    fn cache_values_hold(&self, cache: &ArrayPatternCache) -> bool {
        let holds = |holder: Gc<Object>, slot: u32, function: Gc<Object>| {
            self.heap
                .object(holder)
                .ok()
                .and_then(|record| record.slots.get(slot as usize).copied())
                .is_some_and(|v| matches!(v, Value::Object(o) if o == function))
        };
        holds(cache.array_proto, cache.values_slot, cache.values)
            && holds(cache.iterator_proto, cache.next_slot, cache.next)
    }

    /// The walk of the module comment for arrays with `shape`; `None`
    /// when a condition fails. Marks the four prototype objects watched.
    fn walk_array_pattern(
        &mut self,
        shape: Gc<Shape>,
        realm: u32,
    ) -> Result<Option<ArrayPatternCache>> {
        let epoch = self.heap.watch_epoch();
        let (Ok(array_proto), Ok(iterator_proto), Ok(iterators), Ok(object_proto)) = (
            self.intrinsic(realm, Intrinsic::ArrayPrototype),
            self.intrinsic(realm, Intrinsic::ArrayIteratorPrototype),
            self.intrinsic(realm, Intrinsic::IteratorPrototype),
            self.intrinsic(realm, Intrinsic::ObjectPrototype),
        ) else {
            return Ok(None);
        };
        let (Ok(values), Ok(next)) = (
            self.intrinsic(realm, Intrinsic::ArrayPrototypeValues),
            self.intrinsic(realm, Intrinsic::ArrayIteratorNext),
        ) else {
            return Ok(None);
        };
        let iterator_key = self.symbol_key(WellKnown::Iterator);
        let next_key = PropertyKey::String(self.vm.atoms.next);
        let return_key = PropertyKey::String(self.vm.atoms.return_);
        let heap = &self.heap;
        // The arrays: a shared shape with the prototype and no own
        // `@@iterator`.
        let array_shape = heap.arenas.shapes.get(shape)?;
        if array_shape.is_dictionary()
            || array_shape.proto != Some(array_proto)
            || array_shape
                .find(iterator_key, &heap.arenas.key_lists)?
                .is_some()
        {
            return Ok(None);
        }
        let Some(values_slot) = heap.own_data_slot(array_proto, iterator_key)? else {
            return Ok(None);
        };
        let Some(next_slot) = heap.own_data_slot(iterator_proto, next_key)? else {
            return Ok(None);
        };
        // The chain of the iterator is the standard one and has no
        // `return`.
        if heap.get_prototype_of(iterator_proto)? != Some(iterators)
            || heap.get_prototype_of(iterators)? != Some(object_proto)
        {
            return Ok(None);
        }
        for holder in [iterator_proto, iterators, object_proto] {
            if heap.has_shape_key(holder, return_key)? {
                return Ok(None);
            }
        }
        // The values must be the built-in functions now (the cache keeps
        // the identity; each use reads the slots again).
        let cache = ArrayPatternCache {
            epoch,
            realm,
            shape,
            array_proto,
            values_slot,
            values,
            iterator_proto,
            next_slot,
            next,
        };
        if epoch == u32::MAX {
            return Ok(None);
        }
        for object in [array_proto, iterator_proto, iterators, object_proto] {
            self.heap.watch(object)?;
        }
        Ok(Some(cache))
    }

    /// `IteratorStepValue` of a pattern record over `array` at `index`:
    /// the element, or `None` when the array has no more. The value is
    /// recorded in the current handle scope.
    pub(crate) fn array_pattern_step(
        &mut self,
        array: Gc<Object>,
        index: i32,
    ) -> VmResult<Option<Value>> {
        let (length, _) = self.heap.array_length(array)?;
        let Ok(index) = u32::try_from(index) else {
            return Err(VmError::invariant("a negative pattern index"));
        };
        if index >= length {
            return Ok(None);
        }
        let k = f64::from(index);
        if let Some(value) = self.heap.dense_element(array, k) {
            return Ok(Some(value));
        }
        let key = self.index_key(k)?;
        Ok(Some(self.get_value(array.into(), key)?))
    }

    /// `IteratorClose` of a pattern record that stopped before the end
    /// of `array` at `index` (see the module comment).
    pub(crate) fn close_array_pattern(
        &mut self,
        array: Gc<Object>,
        index: i32,
        throw: bool,
    ) -> VmResult<()> {
        let realm = self.current_realm();
        let epoch = self.heap.watch_epoch();
        if self
            .vm
            .array_pattern
            .is_some_and(|c| c.epoch == epoch && c.realm == realm)
        {
            return Ok(());
        }
        let iterator =
            crate::builtins::new_array_iterator(self, realm, array, IterationKind::Values)?;
        if let ObjectKind::ArrayIterator(state) = &mut self.heap.object_mut(iterator)?.kind {
            state.next_index = f64::from(index);
        }
        self.close_iterator(iterator.into(), throw)
    }

    /// The bulk copy of `SpreadArray` for a pattern record: appends the
    /// elements of `source` from `index` to `target` when `source` has no
    /// holes, and returns whether it did. No script code runs.
    pub(crate) fn spread_array_pattern(
        &mut self,
        target: Gc<Object>,
        source: Gc<Object>,
        index: i32,
    ) -> VmResult<bool> {
        let start = u32::try_from(index).unwrap_or(u32::MAX);
        let Some(count) = self
            .heap
            .append_array_elements(target, source, start, &self.vm)?
        else {
            return Ok(false);
        };
        self.charge(count as usize)?;
        Ok(true)
    }
}
