//! The keys of a `for`-`in` loop (§14.7.5.9 `EnumerateObjectProperties`,
//! §14.7.5.10 `CreateForInIterator`).
//!
//! The loop starts with a list of keys: the enumerable string keys of the
//! object and then of each prototype, in the order of
//! `[[OwnPropertyKeys]]`, where a key of an object hides the same key of
//! every later prototype (also when it is not enumerable). Each step
//! takes the next key and skips it if the object no longer has it
//! (`[[HasProperty]]`), so a deleted key that the loop has not reached is
//! not visited. Keys added during the loop are not visited, and a change
//! of the prototype chain or of an attribute during the loop does not
//! change the list. The specification leaves these cases open; this is
//! what V8 does (measured in Node.js 22). test262 (`for-in-order`) checks
//! the order and the deleted and added keys.
//!
//! Cost: a first walk over the chain finds the last object with an
//! enumerable key and an upper bound of the keys (without reading them);
//! the list and the set of hidden keys are reserved against the heap limit
//! before they exist, and the list counts as part of the iterator object.
//! The second walk reads the keys of the objects up to that last one. Both
//! charge a time step per object and per key. Only the objects before
//! the last one add their keys to the set, so a plain object or an array
//! (whose prototypes have no enumerable keys) needs no set.

use std::collections::HashSet;

use swb_js_text::hash::BuildKeyedHasher;

use crate::heap::Gc;
use crate::object::{ForInIterator, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::VmResult;

/// The bytes that the reservation counts for one entry of the set of
/// hidden keys (key, hash table overhead).
const HIDDEN_ENTRY_BYTES: usize = 40;

/// What the first walk over the prototype chain found.
struct ChainCounts {
    /// An upper bound of the keys of the objects up to the last one with
    /// an enumerable key (and so of the list).
    keys: usize,
    /// An upper bound of the keys of the objects before that one (the
    /// size of the set of hidden keys).
    hiding: usize,
    /// The position in the chain of the last object with an enumerable
    /// string key.
    last: usize,
}

impl Runtime {
    /// `ForIn/OfHeadEvaluation` (§14.7.5.6) for `enumerate`: the key
    /// iterator of `ToObject(value)`, or `undefined` for `undefined` and
    /// `null` (the loop does not run). Runs inside a handle scope.
    pub(crate) fn for_in_iterator(&mut self, value: Value) -> VmResult<Value> {
        if value.is_nullish() {
            return Ok(Value::Undefined);
        }
        let object = self.to_object(value)?;
        self.heap.record(object);
        let counts = self.count_chain_keys(object)?;
        let bytes = counts
            .keys
            .saturating_mul(size_of::<PropertyKey>())
            .saturating_add(counts.hiding.saturating_mul(HIDDEN_ENTRY_BYTES));
        self.heap.reserve(bytes, &self.vm)?;
        let keys = self.collect_chain_keys(object, &counts)?;
        let state = ForInIterator {
            object,
            keys: keys.into_boxed_slice(),
            next: 0,
        };
        let iterator = self
            .heap
            .new_object_with_kind(None, ObjectKind::ForInIterator(Box::new(state)))?;
        Ok(iterator.into())
    }

    /// The next key of a `for`-`in` iterator as a string, or `None` at
    /// the end (the list is then released). A key that the object no
    /// longer has is skipped, at a time step each.
    pub(crate) fn for_in_next(&mut self, iterator: Gc<Object>) -> VmResult<Option<Value>> {
        loop {
            let ObjectKind::ForInIterator(state) = &mut self.heap.object_mut(iterator)?.kind else {
                return Err(crate::vm::VmError::invariant(
                    "ForInNext on an object that is not a for-in iterator",
                ));
            };
            let Some(&key) = state.keys.get(state.next) else {
                state.keys = Box::default();
                state.next = 0;
                return Ok(None);
            };
            state.next += 1;
            let object = state.object;
            if self.still_has(object, key)? {
                return Ok(Some(self.heap.key_to_value(key)?));
            }
            self.tick()?;
        }
    }

    /// `[[HasProperty]]` of a key that the loop is about to visit, with a
    /// time step per prototype: a key deep in a long chain costs a walk
    /// in every iteration (as in V8), so the walk must reach the time
    /// check.
    fn still_has(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        let mut current = object;
        loop {
            if self.heap.get_own_property(current, key)?.is_some()
                || self.heap.string_object_unit(current, key)?.is_some()
            {
                return Ok(true);
            }
            match self.heap.get_prototype_of(current)? {
                Some(proto) => {
                    self.tick()?;
                    current = proto;
                }
                None => return Ok(false),
            }
        }
    }

    /// The first walk: the bounds of [`ChainCounts`], a time step per
    /// object and per key.
    fn count_chain_keys(&mut self, object: Gc<Object>) -> VmResult<ChainCounts> {
        let mut counts = ChainCounts {
            keys: 0,
            hiding: 0,
            last: 0,
        };
        let mut total = 0usize;
        let mut current = Some(object);
        let mut position = 0;
        while let Some(o) = current {
            self.tick()?;
            let bound = self.heap.own_key_bound(o)?;
            self.charge(bound)?;
            if self.heap.has_enumerable_string_key(o)? {
                counts.last = position;
                counts.hiding = total;
                counts.keys = total.saturating_add(bound);
            }
            total = total.saturating_add(bound);
            current = self.heap.get_prototype_of(o)?;
            position += 1;
        }
        Ok(counts)
    }

    /// The second walk: the list of keys. The heap has room for it (and
    /// for the set of hidden keys) after the reservation.
    fn collect_chain_keys(
        &mut self,
        object: Gc<Object>,
        counts: &ChainCounts,
    ) -> VmResult<Vec<PropertyKey>> {
        let mut keys = Vec::new();
        keys.try_reserve(counts.keys)
            .map_err(|_| crate::vm::VmError::from(crate::error::Termination::HeapLimit))?;
        let mut hidden: HashSet<PropertyKey, BuildKeyedHasher> = HashSet::default();
        hidden
            .try_reserve(counts.hiding)
            .map_err(|_| crate::vm::VmError::from(crate::error::Termination::HeapLimit))?;
        let mut current = Some(object);
        let mut position = 0;
        while let Some(o) = current {
            if position > counts.last {
                break;
            }
            self.tick()?;
            let check = position > 0 && !hidden.is_empty();
            let record = position < counts.last;
            let mut seen = 0usize;
            self.heap.each_string_key(o, |key, enumerable| {
                seen += 1;
                if enumerable && !(check && hidden.contains(&key)) {
                    keys.push(key);
                }
                if record {
                    hidden.insert(key);
                }
            })?;
            self.charge(seen)?;
            current = self.heap.get_prototype_of(o)?;
            position += 1;
        }
        Ok(keys)
    }
}
