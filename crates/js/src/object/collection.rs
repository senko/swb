//! `Map` and `Set` on the heap: the operations of the heap on the table of
//! a collection object (`SameValueZero` keys and their hash, `get`, `put`,
//! `remove`, copies, iterators and native loops over a table) and the state
//! of a Map Iterator or Set Iterator. The table itself is in `table.rs`
//! (ECMA-262 §24.1, §24.2; ADR 0026 section 15).

use std::hash::{BuildHasher, Hasher};

use swb_js_text::hash::BuildKeyedHasher;

use crate::error::{Error, Result};
use crate::heap::{Arena, Gc, Heap};
use crate::object::table::{Entry, MAX_LIVE, Table};
use crate::object::{IterationKind, Object, ObjectKind};
use crate::string::JsString;
use crate::value::{BigInt, Value};

/// `SameValueZero` (§7.2.10) for normalized keys: strings are flat.
fn same_value_zero(strings: &Arena<JsString>, bigints: &Arena<BigInt>, a: Value, b: Value) -> bool {
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return x == y || (x.is_nan() && y.is_nan());
    }
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::Object(x), Value::Object(y)) => x == y,
        (Value::String(x), Value::String(y)) => {
            x == y
                || match (strings.get(x), strings.get(y)) {
                    (Ok(x), Ok(y)) => x.as_flat().is_some() && x.as_flat() == y.as_flat(),
                    _ => false,
                }
        }
        (Value::BigInt(x), Value::BigInt(y)) => {
            x == y || matches!((bigints.get(x), bigints.get(y)), (Ok(x), Ok(y)) if x == y)
        }
        _ => false,
    }
}

impl Heap {
    /// The keyed hash of a normalized key (`SameValueZero` equal keys
    /// have equal hashes). A string must be flat.
    pub(crate) fn key_hash(&self, key: Value) -> Result<u64> {
        let mut hasher = BuildKeyedHasher::default().build_hasher();
        match key {
            Value::Undefined => hasher.write_u8(1),
            Value::Null => hasher.write_u8(2),
            Value::Bool(value) => hasher.write_u8(3 + u8::from(value)),
            Value::Int(_) | Value::Double(_) => {
                let number = key.as_number().unwrap_or(0.0);
                // `+0` and `-0` are one key, all NaNs are one key.
                let bits = if number == 0.0 {
                    0
                } else if number.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    number.to_bits()
                };
                hasher.write_u64(bits);
            }
            Value::String(string) => {
                // The hash is kept in the string: a key that is used again
                // (the same handle) is not hashed again. It has 32 bits
                // (the table needs the low bits of the hash).
                let record = self.string(string)?;
                if let Some(hash) = record.cached_key_hash() {
                    return Ok(u64::from(hash));
                }
                let text = self.text(string)?;
                let hash = self.atoms.hash(text) as u32;
                record.remember_key_hash(hash);
                return Ok(u64::from(hash.max(1)));
            }
            Value::Symbol(symbol) => {
                hasher.write_u8(5);
                hasher
                    .write_u64((u64::from(symbol.generation()) << 32) | u64::from(symbol.index()));
            }
            Value::Object(object) => {
                hasher.write_u8(6);
                hasher
                    .write_u64((u64::from(object.generation()) << 32) | u64::from(object.index()));
            }
            Value::BigInt(bigint) => {
                let bigint = self.bigint(bigint)?;
                hasher.write_u8(7 + u8::from(bigint.is_negative()));
                for limb in bigint.limbs() {
                    hasher.write_u64(*limb);
                }
            }
            Value::Empty | Value::Cell(_) => {
                return Err(Error::invariant(
                    "an internal value used as a collection key",
                ));
            }
        }
        Ok(hasher.finish())
    }

    /// The table of a `Map` or `Set` object; `None` for any other
    /// object.
    pub(crate) fn collection_table(&self, object: Gc<Object>) -> Result<Option<&Table>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Map(table) | ObjectKind::Set(table) => Some(table),
            _ => None,
        })
    }

    /// The table of a `Map` or `Set` object, for writing.
    pub(crate) fn collection_table_mut(&mut self, object: Gc<Object>) -> Result<&mut Table> {
        match &mut self.object_mut(object)?.kind {
            ObjectKind::Map(table) | ObjectKind::Set(table) => Ok(table),
            _ => Err(Error::invariant("a collection operation on another object")),
        }
    }

    fn collection_table_ref(&self, object: Gc<Object>) -> Result<&Table> {
        self.collection_table(object)?
            .ok_or(Error::invariant("a collection operation on another object"))
    }

    /// A new empty `Map` (`map` true) or `Set` object.
    pub(crate) fn new_collection(
        &mut self,
        proto: Option<Gc<Object>>,
        map: bool,
    ) -> Result<Gc<Object>> {
        let table = Box::new(Table::new());
        let kind = if map {
            ObjectKind::Map(table)
        } else {
            ObjectKind::Set(table)
        };
        self.new_object_with_kind(proto, kind)
    }

    /// The hash of `key` and the position of the live entry with the
    /// normalized key `key`.
    fn collection_locate(&self, object: Gc<Object>, key: Value) -> Result<(u64, Option<usize>)> {
        let table = self.collection_table_ref(object)?;
        let hash = self.key_hash(key)?;
        if table.len() == 0 {
            return Ok((hash, None));
        }
        let strings = &self.arenas.strings;
        let bigints = &self.arenas.bigints;
        let position = table.find(hash, |stored| {
            same_value_zero(strings, bigints, stored, key)
        });
        Ok((hash, position))
    }

    /// The position of the live entry with the normalized key `key`.
    pub(crate) fn collection_find(&self, object: Gc<Object>, key: Value) -> Result<Option<usize>> {
        let table = self.collection_table_ref(object)?;
        if table.len() == 0 {
            return Ok(None);
        }
        Ok(self.collection_locate(object, key)?.1)
    }

    /// The value of the entry with the key `key` (a `Set` stores
    /// `undefined`).
    pub(crate) fn collection_get(&self, object: Gc<Object>, key: Value) -> Result<Option<Value>> {
        let Some(position) = self.collection_find(object, key)? else {
            return Ok(None);
        };
        Ok(self
            .collection_table_ref(object)?
            .entry(position)
            .map(|entry| entry.value))
    }

    /// Sets the value of the entry with the key `key`, or appends an
    /// entry (`Map.prototype.set`, `Set.prototype.add`). Growth checks
    /// the heap limit first and is charged; it never collects.
    pub(crate) fn collection_put(
        &mut self,
        object: Gc<Object>,
        key: Value,
        value: Value,
    ) -> Result<()> {
        let (hash, position) = self.collection_locate(object, key)?;
        if let Some(position) = position {
            self.collection_table_mut(object)?
                .set_value(position, value);
            return Ok(());
        }
        self.collection_append_hashed(object, hash, key, value)
    }

    /// Appends an entry for a key that is not in the table.
    pub(crate) fn collection_append(
        &mut self,
        object: Gc<Object>,
        key: Value,
        value: Value,
    ) -> Result<()> {
        let hash = self.key_hash(key)?;
        self.collection_append_hashed(object, hash, key, value)
    }

    fn collection_append_hashed(
        &mut self,
        object: Gc<Object>,
        hash: u64,
        key: Value,
        value: Value,
    ) -> Result<()> {
        if self.collection_table_ref(object)?.len() >= MAX_LIVE {
            return Err(Error::range_error(
                if matches!(self.object(object)?.kind, ObjectKind::Map(_)) {
                    "Map maximum size exceeded"
                } else {
                    "Set maximum size exceeded"
                },
            ));
        }
        let mut growth = self.collection_table_ref(object)?.insert_growth();
        if growth > 0 {
            if self.unblock_compaction(object)? {
                growth = self.collection_table_ref(object)?.insert_growth();
            }
            self.check_room(growth)?;
        }
        let table = self.collection_table_mut(object)?;
        let before = table.heap_size();
        table.insert(hash, key, value)?;
        let after = table.heap_size();
        self.charge(after.saturating_sub(before));
        Ok(())
    }

    /// Deletes the entry with the key `key`; returns whether there was
    /// one.
    pub(crate) fn collection_remove(&mut self, object: Gc<Object>, key: Value) -> Result<bool> {
        let Some(position) = self.collection_find(object, key)? else {
            return Ok(false);
        };
        self.collection_table_mut(object)?.remove(position);
        self.unblock_compaction(object)?;
        Ok(true)
    }

    /// A table whose compaction waits for more deleted entries because
    /// of its cursor list may hold the handles of iterators that the
    /// collector freed: prunes the list (at most once per collection) and
    /// compacts if that made it due. Returns whether it pruned.
    fn unblock_compaction(&mut self, object: Gc<Object>) -> Result<bool> {
        let collections = self.stats().collections;
        let table = self.collection_table_ref(object)?;
        if !table.compaction_blocked() || !table.pruned_before(collections) {
            return Ok(false);
        }
        self.prune_cursors(object)?;
        self.collection_table_mut(object)?.compact_if_due();
        Ok(true)
    }

    /// A copy of the entries of `source` in a new table for `target` (an
    /// empty `Set` object that the caller made): `a copy of [[SetData]]`.
    /// Checks the heap limit first.
    pub(crate) fn collection_copy(&mut self, source: Gc<Object>, target: Gc<Object>) -> Result<()> {
        let live = self.collection_table_ref(source)?.len();
        let bytes = Table::bytes_for(live);
        self.check_room(bytes)?;
        let copy = self.collection_table_ref(source)?.copy_live()?;
        self.charge(copy.heap_size());
        *self.collection_table_mut(target)? = copy;
        Ok(())
    }

    /// A new iterator object over the table of `collection`, registered
    /// with the table. `kind_of` makes the object kind from the state.
    pub(crate) fn new_collection_iterator(
        &mut self,
        proto: Gc<Object>,
        collection: Gc<Object>,
        kind: IterationKind,
        map: bool,
    ) -> Result<Gc<Object>> {
        if self
            .collection_table_ref(collection)?
            .needs_prune(self.stats().collections)
        {
            self.prune_cursors(collection)?;
        }
        let state = Box::new(CollectionIterator {
            table: Some(collection),
            slot: 0,
            kind,
        });
        let object_kind = if map {
            ObjectKind::MapIterator(state)
        } else {
            ObjectKind::SetIterator(state)
        };
        let iterator = self.new_object_with_kind(Some(proto), object_kind)?;
        let table = self.collection_table_mut(collection)?;
        let before = table.heap_size();
        let slot = table.cursor_add(iterator)?;
        let after = table.heap_size();
        self.charge(after.saturating_sub(before));
        if let ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state) =
            &mut self.object_mut(iterator)?.kind
        {
            state.slot = slot;
        }
        Ok(iterator)
    }

    /// Drops the cursors of iterators that the collector freed and
    /// renumbers the others (their iterators keep the slot).
    fn prune_cursors(&mut self, collection: Gc<Object>) -> Result<()> {
        let mut cursors = self.collection_table_mut(collection)?.take_cursors();
        cursors.retain(|cursor| self.arenas.objects.contains(cursor.owner));
        for (slot, cursor) in cursors.iter().enumerate() {
            if let Ok(object) = self.arenas.objects.get_mut(cursor.owner)
                && let ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state) =
                    &mut object.kind
            {
                state.slot = slot as u32;
            }
        }
        cursors.shrink_to(cursors.len() * 2);
        let collections = self.stats().collections;
        self.collection_table_mut(collection)?
            .set_cursors(cursors, collections);
        Ok(())
    }

    /// The next entry of a Map or Set iterator and the work done; `None`
    /// when the iterator is finished (it stays finished and its cursor is
    /// gone).
    pub(crate) fn collection_iterator_next(
        &mut self,
        iterator: Gc<Object>,
    ) -> Result<(Option<Entry>, IterationKind, usize)> {
        let (ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state)) =
            &self.object(iterator)?.kind
        else {
            return Err(Error::invariant(
                "a step of a collection iterator on another object",
            ));
        };
        let (kind, slot) = (state.kind, state.slot);
        let Some(collection) = state.table else {
            return Ok((None, kind, 0));
        };
        let step = self
            .collection_table_mut(collection)?
            .cursor_next(slot, iterator);
        let (entry, work) = match step {
            Some((Some(entry), work)) => return Ok((Some(entry), kind, work)),
            Some((None, work)) => (None, work),
            None => (None, 1),
        };
        self.finish_collection_iterator(iterator, collection, slot)?;
        Ok((entry, kind, work))
    }

    fn finish_collection_iterator(
        &mut self,
        iterator: Gc<Object>,
        collection: Gc<Object>,
        slot: u32,
    ) -> Result<()> {
        if let ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state) =
            &mut self.object_mut(iterator)?.kind
        {
            state.table = None;
        }
        let moved = self
            .collection_table_mut(collection)?
            .cursor_remove(slot, iterator);
        if let Some((owner, new_slot)) = moved
            && let Ok(object) = self.arenas.objects.get_mut(owner)
            && let ObjectKind::MapIterator(state) | ObjectKind::SetIterator(state) =
                &mut object.kind
        {
            state.slot = new_slot;
        }
        Ok(())
    }

    // --- Native loops ---

    /// Starts a native loop over `collection`; returns its id.
    pub(crate) fn collection_loop_begin(&mut self, collection: Gc<Object>) -> Result<usize> {
        self.collection_table_mut(collection)?.loop_begin()
    }

    /// The next entry of the loop `id` and the work done.
    pub(crate) fn collection_loop_next(
        &mut self,
        collection: Gc<Object>,
        id: usize,
    ) -> Result<(Option<Entry>, usize)> {
        Ok(self.collection_table_mut(collection)?.loop_next(id))
    }

    /// Ends the loop `id`.
    pub(crate) fn collection_loop_end(&mut self, collection: Gc<Object>, id: usize) {
        if let Ok(table) = self.collection_table_mut(collection) {
            table.loop_end(id);
        }
    }
}

/// The state of a Map Iterator or Set Iterator (§24.1.5, §24.2.6).
#[derive(Debug)]
pub struct CollectionIterator {
    /// The iterated `Map` or `Set`; `None` once the iterator is finished.
    pub(crate) table: Option<Gc<Object>>,
    /// The slot of the iterator's cursor in the table.
    pub(crate) slot: u32,
    /// What the iterator returns for an entry.
    pub(crate) kind: IterationKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots, RootSource, Tracer};

    fn heap() -> Heap {
        Heap::new(HeapConfig::default())
    }

    fn map(heap: &mut Heap) -> Gc<Object> {
        heap.new_collection(None, true).unwrap()
    }

    fn put(heap: &mut Heap, m: Gc<Object>, key: i32) {
        heap.collection_put(m, Value::Int(key), Value::Undefined)
            .unwrap();
    }

    fn remove(heap: &mut Heap, m: Gc<Object>, key: i32) {
        heap.collection_remove(m, Value::Int(key)).unwrap();
    }

    /// The numbers of the next `n` steps of an iterator.
    fn take(heap: &mut Heap, iterator: Gc<Object>, n: usize) -> Vec<i32> {
        let mut out = Vec::new();
        for _ in 0..n {
            match heap.collection_iterator_next(iterator).unwrap().0 {
                Some(Entry {
                    key: Value::Int(key),
                    ..
                }) => out.push(key),
                _ => break,
            }
        }
        out
    }

    struct Rooted(Vec<Gc<Object>>);

    impl RootSource for Rooted {
        fn trace_roots(&self, tracer: &mut Tracer<'_>) {
            for object in &self.0 {
                tracer.object(*object);
            }
        }
    }

    fn table(heap: &Heap, m: Gc<Object>) -> &Table {
        heap.collection_table(m).unwrap().unwrap()
    }

    #[test]
    fn a_key_deleted_and_added_again_while_compaction_waits_keeps_one_slot() {
        let mut heap = heap();
        let m = map(&mut heap);
        let proto = heap.new_object(None).unwrap();
        for i in 0..10 {
            put(&mut heap, m, i);
        }
        for i in 0..5_000 {
            // An unfinished iterator per step blocks the compaction.
            heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
            remove(&mut heap, m, i % 10);
            put(&mut heap, m, i % 10);
        }
        let table = table(&heap, m);
        assert!(table.slots() > 1_000, "no compaction ran");
        assert_eq!(table.indexed(), 10);
    }

    #[test]
    fn full_scans_over_deleted_entries_in_the_middle_stay_cheap() {
        // A live key first, then a key added and deleted at each step,
        // an unfinished iterator per step (the compaction waits), and a
        // full native loop per step. Without the skips each loop walks
        // every deleted entry.
        let mut heap = heap();
        let m = map(&mut heap);
        let proto = heap.new_object(None).unwrap();
        put(&mut heap, m, -1);
        let mut work = 0;
        let steps = 5_000;
        for i in 0..steps {
            heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
            put(&mut heap, m, i);
            remove(&mut heap, m, i);
            let id = heap.collection_loop_begin(m).unwrap();
            loop {
                let (entry, cost) = heap.collection_loop_next(m, id).unwrap();
                work += cost;
                if entry.is_none() {
                    break;
                }
            }
            heap.collection_loop_end(m, id);
        }
        assert!(table(&heap, m).slots() > 1_000, "no compaction ran");
        assert!(work < 10 * steps as usize, "{work} positions");
    }

    #[test]
    fn a_queue_with_abandoned_iterators_costs_constant_work_per_step() {
        // The LRU idiom: delete the first key, append one, read the first
        // key through a new iterator that is never finished. The
        // unfinished iterators block the compaction (no collection runs),
        // so the deleted entries pile up at the start of the vector.
        let mut heap = heap();
        let m = map(&mut heap);
        let proto = heap.new_object(None).unwrap();
        for i in 0..10 {
            put(&mut heap, m, i);
        }
        for i in 10..20_000 {
            remove(&mut heap, m, i - 10);
            put(&mut heap, m, i);
            let it = heap
                .new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
            let (entry, _, work) = heap.collection_iterator_next(it).unwrap();
            assert!(matches!(entry, Some(Entry { key: Value::Int(k), .. }) if k == i - 9));
            assert!(work <= 2, "step {i}: {work} positions");
        }
        assert!(table(&heap, m).slots() > 10_000, "no compaction ran");
    }

    #[test]
    fn entries_keep_the_insertion_order_and_deleted_keys_leave() {
        let mut heap = heap();
        let m = map(&mut heap);
        for i in 0..10 {
            heap.collection_put(m, Value::Int(i), Value::Int(i * 2))
                .unwrap();
        }
        assert!(heap.collection_remove(m, Value::Int(3)).unwrap());
        assert!(!heap.collection_remove(m, Value::Int(3)).unwrap());
        heap.collection_put(m, Value::Int(3), Value::Int(99))
            .unwrap();
        heap.collection_put(m, Value::Int(4), Value::Int(-1))
            .unwrap();
        let proto = heap.new_object(None).unwrap();
        let it = heap
            .new_collection_iterator(proto, m, IterationKind::Keys, true)
            .unwrap();
        assert_eq!(take(&mut heap, it, 20), [0, 1, 2, 4, 5, 6, 7, 8, 9, 3]);
        assert_eq!(
            heap.collection_get(m, Value::Int(4)).unwrap(),
            Some(Value::Int(-1))
        );
        assert_eq!(
            heap.collection_get(m, Value::Int(3)).unwrap(),
            Some(Value::Int(99))
        );
        assert_eq!(table(&heap, m).len(), 10);
    }

    #[test]
    fn keys_are_compared_with_same_value_zero() {
        let mut heap = heap();
        let m = map(&mut heap);
        heap.collection_put(m, Value::Int(0), Value::Int(1))
            .unwrap();
        // The integer and double forms, and -0, are one key.
        let get = |heap: &Heap, key: Value| heap.collection_get(m, key).unwrap();
        assert_eq!(get(&heap, Value::Double(0.0)), Some(Value::Int(1)));
        assert_eq!(get(&heap, Value::Double(-0.0)), Some(Value::Int(1)));
        heap.collection_put(m, Value::Double(f64::NAN), Value::Int(2))
            .unwrap();
        heap.collection_put(m, Value::Double(-f64::NAN), Value::Int(3))
            .unwrap();
        assert_eq!(table(&heap, m).len(), 2);
        assert_eq!(get(&heap, Value::Double(f64::NAN)), Some(Value::Int(3)));
        heap.collection_put(m, Value::Double(2_147_483_648.0), Value::Int(4))
            .unwrap();
        assert_eq!(
            get(&heap, Value::Double(2_147_483_648.0)),
            Some(Value::Int(4))
        );
        heap.collection_put(m, Value::Int(5), Value::Int(5))
            .unwrap();
        assert_eq!(get(&heap, Value::Double(5.0)), Some(Value::Int(5)));
        // Strings by content, objects and symbols by identity.
        let a = heap.alloc_str("key").unwrap();
        let b = heap
            .alloc_string(swb_js_text::String16::from_units(&[0x6B, 0x65, 0x79]))
            .unwrap();
        heap.collection_put(m, a.into(), Value::Int(6)).unwrap();
        assert_eq!(get(&heap, b.into()), Some(Value::Int(6)));
        let other = heap.alloc_str("kez").unwrap();
        assert_eq!(get(&heap, other.into()), None);
        let o1 = heap.new_object(None).unwrap();
        let o2 = heap.new_object(None).unwrap();
        heap.collection_put(m, o1.into(), Value::Int(7)).unwrap();
        assert_eq!(get(&heap, o1.into()), Some(Value::Int(7)));
        assert_eq!(get(&heap, o2.into()), None);
        let s1 = heap.alloc_symbol(None).unwrap();
        let s2 = heap.alloc_symbol(None).unwrap();
        heap.collection_put(m, s1.into(), Value::Int(8)).unwrap();
        assert_eq!(get(&heap, s2.into()), None);
        assert_eq!(get(&heap, s1.into()), Some(Value::Int(8)));
        // `undefined`, `null` and the Booleans are keys.
        for (i, key) in [
            Value::Undefined,
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
        ]
        .into_iter()
        .enumerate()
        {
            heap.collection_put(m, key, Value::Int(10 + i as i32))
                .unwrap();
        }
        assert_eq!(get(&heap, Value::Null), Some(Value::Int(11)));
        assert_eq!(get(&heap, Value::Bool(false)), Some(Value::Int(13)));
    }

    #[test]
    fn a_live_iterator_survives_compactions() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let m = map(&mut heap);
        for i in 0..200 {
            put(&mut heap, m, i);
        }
        let it = heap
            .new_collection_iterator(proto, m, IterationKind::Keys, true)
            .unwrap();
        let other = heap
            .new_collection_iterator(proto, m, IterationKind::Keys, true)
            .unwrap();
        assert_eq!(take(&mut heap, it, 3), [0, 1, 2]);
        assert_eq!(take(&mut heap, other, 150), (0..150).collect::<Vec<_>>());
        for i in 0..190 {
            remove(&mut heap, m, i);
        }
        // The compaction left 10 live entries in a short vector.
        assert!(table(&heap, m).slots() < 100);
        assert_eq!(take(&mut heap, it, 3), [190, 191, 192]);
        for i in 200..400 {
            put(&mut heap, m, i);
        }
        for i in 190..390 {
            remove(&mut heap, m, i);
        }
        assert_eq!(take(&mut heap, it, 100), (390..400).collect::<Vec<_>>());
        assert_eq!(take(&mut heap, other, 100), (390..400).collect::<Vec<_>>());
        // A finished iterator stays finished.
        put(&mut heap, m, 1000);
        assert_eq!(take(&mut heap, it, 1), Vec::<i32>::new());
        assert_eq!(table(&heap, m).cursor_count(), 0);
    }

    #[test]
    fn clear_sends_iterators_to_the_new_entries() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let m = map(&mut heap);
        for i in 0..10 {
            put(&mut heap, m, i);
        }
        let it = heap
            .new_collection_iterator(proto, m, IterationKind::Keys, true)
            .unwrap();
        assert_eq!(take(&mut heap, it, 4), [0, 1, 2, 3]);
        heap.collection_table_mut(m).unwrap().clear();
        assert_eq!(table(&heap, m).slots(), 0);
        for i in 20..23 {
            put(&mut heap, m, i);
        }
        assert_eq!(take(&mut heap, it, 10), [20, 21, 22]);
    }

    #[test]
    fn native_loops_follow_compactions() {
        let mut heap = heap();
        let m = map(&mut heap);
        for i in 0..100 {
            put(&mut heap, m, i);
        }
        let id = heap.collection_loop_begin(m).unwrap();
        let mut seen = Vec::new();
        for _ in 0..5 {
            let entry = heap.collection_loop_next(m, id).unwrap().0.unwrap();
            seen.push(entry.key.as_number().unwrap() as i32);
        }
        for i in 0..90 {
            remove(&mut heap, m, i);
        }
        while let Some(entry) = heap.collection_loop_next(m, id).unwrap().0 {
            seen.push(entry.key.as_number().unwrap() as i32);
        }
        assert_eq!(
            seen,
            [0, 1, 2, 3, 4, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99]
        );
        heap.collection_loop_end(m, id);
    }

    #[test]
    fn insert_and_delete_churn_keeps_the_memory_bounded() {
        let mut heap = heap();
        let m = map(&mut heap);
        for i in 0..200_000 {
            put(&mut heap, m, i);
            if i >= 10 {
                remove(&mut heap, m, i - 10);
            }
        }
        let t = table(&heap, m);
        assert_eq!(t.len(), 10);
        assert!(t.slots() <= 64, "{} slots", t.slots());
        assert!(t.heap_size() < 8192, "{} bytes", t.heap_size());
        // Many entries, then almost all deleted: the memory follows.
        for i in 0..100_000 {
            put(&mut heap, m, 1_000_000 + i);
        }
        let big = table(&heap, m).heap_size();
        for i in 0..99_990 {
            remove(&mut heap, m, 1_000_000 + i);
        }
        let small = table(&heap, m).heap_size();
        assert!(small * 20 < big, "{small} against {big}");
    }

    #[test]
    fn abandoned_iterators_do_not_pile_up_in_the_cursor_list() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let m = map(&mut heap);
        put(&mut heap, m, 1);
        let roots = Rooted(vec![m, proto]);
        let mut most = 0;
        for round in 0..20 {
            for _ in 0..10_000 {
                heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                    .unwrap();
            }
            // The collector frees the dropped iterators; the list is
            // pruned at a later creation.
            heap.collect(&roots);
            let it = heap
                .new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
            for _ in 0..40 {
                heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                    .unwrap();
            }
            most = most.max(table(&heap, m).cursor_count());
            assert!(heap.is_live_object(it), "round {round}");
        }
        assert!(most < 30_000, "{most} cursors");
        heap.collect(&roots);
        for _ in 0..100 {
            heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
        }
        assert!(table(&heap, m).cursor_count() <= 200);
    }

    #[test]
    fn dead_cursors_do_not_keep_deleted_entries() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let m = map(&mut heap);
        for i in 0..200 {
            put(&mut heap, m, i);
        }
        for _ in 0..5_000 {
            heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                .unwrap();
        }
        // The iterators are garbage, but the list still has their handles.
        for i in 0..190 {
            remove(&mut heap, m, i);
        }
        assert!(table(&heap, m).slots() >= 200);
        heap.collect(&Rooted(vec![m, proto]));
        // The next delete prunes the list (once) and the table compacts.
        remove(&mut heap, m, 190);
        assert!(
            table(&heap, m).slots() < 20,
            "{} slots",
            table(&heap, m).slots()
        );
        assert_eq!(table(&heap, m).cursor_count(), 0);
        assert_eq!(table(&heap, m).len(), 9);
    }

    #[test]
    fn finishing_iterators_in_any_order_keeps_the_slots_right() {
        let mut heap = heap();
        let proto = heap.new_object(None).unwrap();
        let m = map(&mut heap);
        for i in 0..3 {
            put(&mut heap, m, i);
        }
        let its: Vec<_> = (0..50)
            .map(|_| {
                heap.new_collection_iterator(proto, m, IterationKind::Keys, true)
                    .unwrap()
            })
            .collect();
        // Finish every third one first, advance a third by one.
        for it in its.iter().step_by(3) {
            assert_eq!(take(&mut heap, *it, 10), [0, 1, 2]);
        }
        for it in its.iter().skip(1).step_by(3) {
            assert_eq!(take(&mut heap, *it, 1), [0]);
        }
        put(&mut heap, m, 3);
        for (i, it) in its.iter().enumerate() {
            let rest = take(&mut heap, *it, 10);
            let expected: &[i32] = match i % 3 {
                0 => &[],
                1 => &[1, 2, 3],
                _ => &[0, 1, 2, 3],
            };
            assert_eq!(rest, expected, "iterator {i}");
        }
        assert_eq!(table(&heap, m).cursor_count(), 0);
    }

    #[test]
    fn a_table_stops_at_the_largest_size() {
        let mut heap = heap();
        let m = map(&mut heap);
        for i in 0..MAX_LIVE as i32 {
            put(&mut heap, m, i);
        }
        let error = heap.collection_put(m, Value::Int(-1), Value::Undefined);
        assert!(error.is_err());
        // Existing keys can still change, and deletes make room.
        heap.collection_put(m, Value::Int(5), Value::Int(1))
            .unwrap();
        remove(&mut heap, m, 5);
        put(&mut heap, m, -1);
    }

    #[test]
    fn the_collector_traces_keys_and_values_and_frees_the_rest() {
        let mut heap = heap();
        let m = map(&mut heap);
        let key = heap.alloc_str("a key that is long enough").unwrap();
        let value = heap.new_object(None).unwrap();
        let gone = heap.new_object(None).unwrap();
        heap.collection_put(m, key.into(), value.into()).unwrap();
        heap.collection_put(m, gone.into(), Value::Undefined)
            .unwrap();
        heap.collection_remove(m, gone.into()).unwrap();
        heap.collect(&Rooted(vec![m]));
        assert!(heap.is_live_string(key));
        assert!(heap.is_live_object(value));
        assert!(!heap.is_live_object(gone));
        heap.collect(&NoRoots);
        assert!(!heap.is_live_object(m));
    }

    #[test]
    fn colliding_looking_keys_stay_fast() {
        // Keys that a plain multiplicative hash puts in a few buckets:
        // multiples of a power of two, doubles with equal low bits.
        let mut heap = heap();
        let m = map(&mut heap);
        for i in 0..50_000_i32 {
            heap.collection_put(
                m,
                Value::Double(f64::from(i) * 4_294_967_296.0),
                Value::Undefined,
            )
            .unwrap();
            heap.collection_put(m, Value::Double(f64::from(i) + 0.5), Value::Undefined)
                .unwrap();
        }
        assert_eq!(table(&heap, m).len(), 100_000);
        for i in 0..50_000_i32 {
            assert!(
                heap.collection_find(m, Value::Double(f64::from(i) * 4_294_967_296.0))
                    .unwrap()
                    .is_some()
            );
        }
    }
}
