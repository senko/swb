//! The ordered hash table of `Map` and `Set` (ECMA-262 §24.1, §24.2,
//! <https://tc39.es/ecma262/#sec-map-objects>; ADR 0026 section 15).
//!
//! The specification models `[[MapData]]` and `[[SetData]]` as a list in
//! insertion order. A delete leaves an empty entry, `clear` empties all
//! entries, an iterator continues over entries that are added later. This
//! module keeps the list as a vector of entries and finds the entry of a
//! key through an index:
//!
//! - `entries`: the entries in insertion order. A deleted entry has the key
//!   `Value::Empty` and stays in the vector, so the positions of the other
//!   entries do not change.
//! - `index`: open addressing with linear probing over entry positions
//!   (`u32`, [`NONE`] for a free slot). Its length is a power of two and
//!   at least twice the length of `entries` (deleted entries included), so
//!   a probe always ends. The entry stores its 64-bit hash, which is
//!   compared before the key. A delete takes the entry's position out of
//!   the index with the deletion algorithm for linear probing (Knuth,
//!   TAOCP vol. 3, §6.4, Algorithm R), so the index holds only live
//!   entries and a key that is deleted and added again does not lengthen
//!   the probes while the compaction waits.
//! - Compaction: when at least half of the entries are deleted, the vector
//!   and the index are rebuilt without them (in place), so memory stays in
//!   proportion to the live entries. Compaction moves entries, so the
//!   positions of the iterators that are not finished are corrected.
//!
//! Iterators and native loops (`forEach`, the set methods) hold a position
//! in the table, not in the iterator object: `cursors` is the list of the
//! unfinished iterators (their handles are not traced; a handle that the
//! collector freed is stale and is dropped when the list is pruned) with
//! their positions, `loops` is a stack of positions of native loops. A
//! compaction maps each position to the number of live entries before it.
//! `clear` bumps `epoch`: a cursor from an older epoch is at position 0
//! (every entry it had seen is gone). Compaction runs only when the
//! deleted entries are at least as many as the cursors and loops, so its
//! cost (entries plus cursors) is paid for by the deletions that caused it.
//! Until then, `head` counts the deleted entries at the start of the
//! vector and every scan starts at it, so a table used as a queue (delete
//! the first key, append a new one, read the first key through a new
//! iterator) costs O(1) per step even while unfinished iterators block the
//! compaction; deleted entries further on hold skips to later positions
//! ([`Table::scan`]), so a full iteration costs about the live entries.
//!
//! The keys are `SameValueZero` normalized by the caller
//! ([`crate::vm::Runtime::collection_key`]: `-0` is `+0`, strings are
//! flat). The hash of a number is the keyed hash of the bits of its `f64`
//! value (the integer and double forms agree), of a string the keyed
//! `SipHash` of the atom table over its code units, of an object, symbol
//! or `BigInt` the keyed hash of its handle or limbs. The keys of the
//! hashers are random per process, so content from the network cannot
//! choose collisions. `WeakMap` and `WeakSet` (M7 feature 9) reuse
//! [`Heap::key_hash`].
//!
//! The structure is written from the specification text and from the
//! general description of open addressing (Knuth, TAOCP vol. 3, §6.4) and
//! of ordered hash tables with a dense entry array (a common design); no
//! code of another engine was read (docs/credits.md).

use crate::error::{Error, Result, Termination};
use crate::heap::{Gc, Tracer};
use crate::object::Object;
use crate::value::Value;

/// The free slot of the index.
const NONE: u32 = u32::MAX;
/// The smallest index length (four entries).
const MIN_INDEX: usize = 8;
/// A table with fewer entries is not compacted after a delete.
const MIN_COMPACT: usize = 32;
/// The smallest length of the cursor list at which it is pruned.
const MIN_PRUNE: usize = 32;
/// The most entries of a table (positions are `u32`).
const MAX_ENTRIES: usize = (u32::MAX / 4) as usize;
/// The most live entries of a `Map` or `Set`: Chromium 148 throws a
/// `RangeError` ("Map maximum size exceeded") when an entry is added to
/// a table of 2^24 entries (measured).
#[cfg(not(test))]
pub(super) const MAX_LIVE: usize = 1 << 24;
#[cfg(test)]
pub(super) const MAX_LIVE: usize = 1 << 18;

/// An entry of the table: a key and a value (a `Set` stores `undefined`
/// as the value and uses the key as its element).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    /// The key; `Empty` for a deleted entry.
    pub(crate) key: Value,
    /// The value.
    pub(crate) value: Value,
    /// The hash of the key; for a deleted entry, a later position up to
    /// which all entries are deleted (the skip of [`Table::scan`]).
    hash: u64,
}

/// The position of an unfinished iterator.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    /// The iterator object (not traced).
    pub(super) owner: Gc<Object>,
    /// The position of the next entry to look at.
    pos: u32,
    /// The `clear` epoch of `pos`.
    epoch: u64,
}

/// The data of a `Map` or `Set`.
#[derive(Debug, Default)]
pub struct Table {
    entries: Vec<Entry>,
    index: Vec<u32>,
    live: usize,
    /// The entries before this position are all deleted.
    head: usize,
    cursors: Vec<Cursor>,
    loops: Vec<u32>,
    epoch: u64,
    prune_at: usize,
    /// The number of collections at the last prune of the cursor list.
    pruned_at: u64,
}

fn oom() -> Error {
    Error::from(Termination::OutOfMemory)
}

impl Table {
    /// An empty table.
    pub(crate) fn new() -> Self {
        Table::default()
    }

    /// The number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// The bytes that the table owns (the collector counts them).
    pub(crate) fn heap_size(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>()
            + self.index.capacity() * size_of::<u32>()
            + self.cursors.capacity() * size_of::<Cursor>()
            + self.loops.capacity() * size_of::<u32>()
    }

    /// Reports the keys and values to the collector. The cursors are not
    /// traced.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in &self.entries {
            if !entry.key.is_empty() {
                tracer.value(entry.key);
                tracer.value(entry.value);
            }
        }
    }

    /// The index length for `count` entries.
    fn index_len_for(count: usize) -> usize {
        count
            .saturating_mul(2)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX)
            .max(MIN_INDEX)
    }

    /// The bytes that a table of `count` live entries owns after
    /// [`Table::copy_live`] (for the check of the heap limit).
    pub(crate) fn bytes_for(count: usize) -> usize {
        let index = Table::index_len_for(count + 1);
        (index / 2) * size_of::<Entry>() + index * size_of::<u32>()
    }

    /// The entry at a position.
    pub(crate) fn entry(&self, pos: usize) -> Option<&Entry> {
        self.entries.get(pos)
    }

    /// Sets the value of the entry at a position.
    pub(crate) fn set_value(&mut self, pos: usize, value: Value) {
        if let Some(entry) = self.entries.get_mut(pos) {
            entry.value = value;
        }
    }

    /// The position of the live entry whose key satisfies `is_key`, among
    /// those with this hash.
    pub(crate) fn find(&self, hash: u64, is_key: impl Fn(Value) -> bool) -> Option<usize> {
        if self.index.is_empty() {
            return None;
        }
        let mask = self.index.len() - 1;
        let mut slot = hash as usize & mask;
        loop {
            let position = *self.index.get(slot)?;
            if position == NONE {
                return None;
            }
            let entry = self.entries.get(position as usize)?;
            if entry.hash == hash && !entry.key.is_empty() && is_key(entry.key) {
                return Some(position as usize);
            }
            slot = (slot + 1) & mask;
        }
    }

    /// The bytes that the next insert adds (0 if the entry fits or a
    /// compaction makes room). The caller checks the heap limit before it
    /// inserts.
    pub(crate) fn insert_growth(&self) -> usize {
        if self.entries.len() < self.index.len() / 2 {
            return 0;
        }
        if self.can_compact() {
            return 0;
        }
        let index = (self.index.len() * 2).max(MIN_INDEX);
        let entries = index / 2;
        let before = self.heap_size();
        let after = entries * size_of::<Entry>() + index * size_of::<u32>();
        after.saturating_sub(before)
    }

    /// Appends an entry for a key that is not in the table.
    pub(crate) fn insert(&mut self, hash: u64, key: Value, value: Value) -> Result<()> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(oom());
        }
        if self.entries.len() >= self.index.len() / 2 {
            if self.can_compact() {
                self.compact()?;
            } else {
                self.rebuild((self.index.len() * 2).max(MIN_INDEX))?;
            }
        }
        let position = u32::try_from(self.entries.len()).map_err(|_| oom())?;
        self.entries.push(Entry { key, value, hash });
        self.place(hash, position);
        self.live += 1;
        Ok(())
    }

    /// Takes an entry position out of the index (Knuth, TAOCP vol. 3,
    /// §6.4, Algorithm R): the slot becomes free, and each later entry of
    /// the same run whose home slot is not between the free slot and its
    /// own slot moves back into the free slot.
    fn unplace(&mut self, hash: u64, position: u32) {
        if self.index.is_empty() {
            return;
        }
        let mask = self.index.len() - 1;
        let mut hole = hash as usize & mask;
        loop {
            match self.index.get(hole) {
                Some(&taken) if taken == position => break,
                Some(&taken) if taken != NONE => hole = (hole + 1) & mask,
                _ => return,
            }
        }
        let mut slot = hole;
        loop {
            slot = (slot + 1) & mask;
            let Some(&taken) = self.index.get(slot) else {
                break;
            };
            if taken == NONE {
                break;
            }
            let Some(entry) = self.entries.get(taken as usize) else {
                break;
            };
            let home = entry.hash as usize & mask;
            let stays = if hole <= slot {
                hole < home && home <= slot
            } else {
                hole < home || home <= slot
            };
            if !stays {
                if let Some(free) = self.index.get_mut(hole) {
                    *free = taken;
                }
                hole = slot;
            }
        }
        if let Some(free) = self.index.get_mut(hole) {
            *free = NONE;
        }
    }

    /// Puts an entry position into the index (the index has room).
    fn place(&mut self, hash: u64, position: u32) {
        let mask = self.index.len() - 1;
        let mut slot = hash as usize & mask;
        while let Some(&taken) = self.index.get(slot) {
            if taken == NONE {
                break;
            }
            slot = (slot + 1) & mask;
        }
        if let Some(free) = self.index.get_mut(slot) {
            *free = position;
        }
    }

    /// Whether the deleted entries justify a compaction: at least half of
    /// the entries, and at least as many as the cursors and loops.
    fn can_compact(&self) -> bool {
        let deleted = self.entries.len() - self.live;
        deleted > 0
            && deleted * 2 >= self.entries.len()
            && deleted >= self.cursors.len() + self.loops.len()
    }

    /// Whether the deleted entries would justify a compaction if the
    /// cursor list held only live iterators: at least half of the
    /// entries, but fewer than the cursors and loops.
    pub(crate) fn compaction_blocked(&self) -> bool {
        let deleted = self.entries.len() - self.live;
        deleted > 0
            && deleted * 2 >= self.entries.len()
            && deleted < self.cursors.len() + self.loops.len()
    }

    /// Whether no prune of the cursor list has happened since the
    /// collection count was `collections`.
    pub(crate) fn pruned_before(&self, collections: u64) -> bool {
        self.pruned_at != collections
    }

    /// Compacts a table with many deleted entries (after a prune of the
    /// cursor list made it possible).
    pub(crate) fn compact_if_due(&mut self) {
        if self.entries.len() >= MIN_COMPACT && self.can_compact() {
            // A failed compaction only keeps the memory a little longer.
            let _ = self.compact();
        }
    }

    /// Rebuilds the index with `index_len` slots and room for
    /// `index_len / 2` entries.
    fn rebuild(&mut self, index_len: usize) -> Result<()> {
        let wanted = index_len / 2;
        if wanted > self.entries.capacity() {
            self.entries
                .try_reserve_exact(wanted - self.entries.len())?;
        } else if self.entries.capacity() > wanted * 2 {
            self.entries.shrink_to(wanted);
        }
        self.index.clear();
        if index_len > self.index.capacity() {
            self.index.try_reserve_exact(index_len)?;
        } else if self.index.capacity() > index_len * 2 {
            self.index.shrink_to(index_len);
        }
        self.index.resize(index_len, NONE);
        for position in 0..self.entries.len() {
            let Some(entry) = self.entries.get(position) else {
                break;
            };
            if !entry.key.is_empty() {
                let hash = entry.hash;
                self.place(hash, position as u32);
            }
        }
        Ok(())
    }

    /// Removes the deleted entries, corrects the positions of the cursors
    /// and loops, and rebuilds the index for the live entries.
    fn compact(&mut self) -> Result<()> {
        let old_len = self.entries.len();
        let track = !self.cursors.is_empty() || !self.loops.is_empty();
        let mut remap: Vec<u32> = Vec::new();
        if track {
            remap.try_reserve_exact(old_len + 1)?;
        }
        let mut write = 0;
        for read in 0..old_len {
            if track {
                remap.push(write as u32);
            }
            if let Some(&entry) = self.entries.get(read)
                && !entry.key.is_empty()
            {
                if let Some(slot) = self.entries.get_mut(write) {
                    *slot = entry;
                }
                write += 1;
            }
        }
        if track {
            remap.push(write as u32);
        }
        self.entries.truncate(write);
        self.head = 0;
        if track {
            let epoch = self.epoch;
            let map = |pos: u32| remap.get(pos as usize).copied().unwrap_or(write as u32);
            for cursor in &mut self.cursors {
                if cursor.epoch != epoch {
                    cursor.pos = 0;
                    cursor.epoch = epoch;
                }
                cursor.pos = map(cursor.pos);
            }
            for position in &mut self.loops {
                *position = map(*position);
            }
        }
        self.rebuild(Table::index_len_for(write + 1))
    }

    /// Deletes the entry at a position (§24.1.3.3): its key becomes
    /// empty. Compacts a table with many deleted entries.
    pub(crate) fn remove(&mut self, position: usize) {
        let Some(entry) = self.entries.get_mut(position) else {
            return;
        };
        if entry.key.is_empty() {
            return;
        }
        entry.key = Value::Empty;
        entry.value = Value::Undefined;
        let hash = entry.hash;
        self.live -= 1;
        self.unplace(hash, position as u32);
        if let Some(entry) = self.entries.get_mut(position) {
            entry.hash = position as u64 + 1;
        }
        if position == self.head {
            while self
                .entries
                .get(self.head)
                .is_some_and(|e| e.key.is_empty())
            {
                self.head += 1;
            }
        }
        self.compact_if_due();
    }

    /// Empties the table (§24.1.3.1): all entries are gone, the iterators
    /// continue with the entries added later, and the memory is freed.
    pub(crate) fn clear(&mut self) {
        self.entries = Vec::new();
        self.index = Vec::new();
        self.live = 0;
        self.head = 0;
        self.epoch += 1;
        self.loops.fill(0);
    }

    /// A copy of the live entries in their order (`a copy of
    /// [[SetData]]`), without cursors. The caller checked the room with
    /// [`Table::bytes_for`].
    pub(crate) fn copy_live(&self) -> Result<Table> {
        let mut copy = Table::new();
        let index_len = Table::index_len_for(self.live + 1);
        copy.entries.try_reserve_exact(index_len / 2)?;
        copy.index.try_reserve_exact(index_len)?;
        copy.index.resize(index_len, NONE);
        for entry in &self.entries {
            if !entry.key.is_empty() {
                let position = copy.entries.len() as u32;
                copy.entries.push(*entry);
                copy.place(entry.hash, position);
            }
        }
        copy.live = copy.entries.len();
        Ok(copy)
    }

    // --- Positions: loops and cursors ---

    /// The next live entry at or after `from`: the entry, the position
    /// after it (or the end), and the number of positions looked at. The
    /// scan starts at `head` at the earliest and jumps over runs of
    /// deleted entries: a deleted entry holds a later position up to which
    /// all entries are deleted, and each jump shortens the path for the
    /// next scan (path splitting, as in the "find" of a union-find
    /// structure: Tarjan and van Leeuwen, "Worst-case analysis of set
    /// union algorithms", 1984). Entries never become live again before a
    /// compaction, so a skip stays valid while the vector grows. So
    /// deleted entries in the middle that wait for a compaction cost each
    /// scan about a logarithm, not their number.
    fn scan(&mut self, from: u32) -> (Option<Entry>, u32, usize) {
        let len = self.entries.len();
        let start = (from as usize).max(self.head);
        let mut position = start;
        let mut work = 1;
        while let Some(entry) = self.entries.get(position).copied() {
            if !entry.key.is_empty() {
                return (Some(entry), (position + 1) as u32, work);
            }
            let next = (entry.hash as usize).max(position + 1).min(len);
            if let Some(after) = self.entries.get(next)
                && after.key.is_empty()
            {
                let further = (after.hash as usize).max(next + 1).min(len);
                if let Some(slot) = self.entries.get_mut(position) {
                    slot.hash = further as u64;
                }
            }
            position = next;
            work += 1;
        }
        let end = len.max(start).min(u32::MAX as usize);
        (None, end as u32, work)
    }

    /// Starts a native loop over the entries (it sees the entries added
    /// later). Returns its id; loops end in the reverse order of their
    /// start ([`Table::loop_end`]).
    pub(crate) fn loop_begin(&mut self) -> Result<usize> {
        self.loops.try_reserve(1)?;
        self.loops.push(0);
        Ok(self.loops.len() - 1)
    }

    /// The next entry of a native loop and the work done.
    pub(crate) fn loop_next(&mut self, id: usize) -> (Option<Entry>, usize) {
        let Some(&from) = self.loops.get(id) else {
            return (None, 1);
        };
        let (entry, next, work) = self.scan(from);
        if let Some(slot) = self.loops.get_mut(id) {
            *slot = next;
        }
        (entry, work)
    }

    /// Ends the native loop `id`.
    pub(crate) fn loop_end(&mut self, id: usize) {
        self.loops.truncate(id);
    }

    /// Whether the cursor list should be pruned: it is long and either
    /// has doubled since the last prune or a collection has run since
    /// (which may have freed iterators). `collections` is the number of
    /// collections so far.
    pub(crate) fn needs_prune(&self, collections: u64) -> bool {
        self.cursors.len() >= MIN_PRUNE
            && (self.cursors.len() >= self.prune_at || collections != self.pruned_at)
    }

    /// Takes the cursor list out for pruning ([`Heap::prune_cursors`]).
    pub(super) fn take_cursors(&mut self) -> Vec<Cursor> {
        std::mem::take(&mut self.cursors)
    }

    /// Puts the pruned cursor list back and sets the next prune at twice
    /// its length.
    pub(super) fn set_cursors(&mut self, cursors: Vec<Cursor>, collections: u64) {
        self.prune_at = cursors.len().saturating_mul(2);
        self.pruned_at = collections;
        self.cursors = cursors;
    }

    /// Adds a cursor for the iterator `owner` at position 0; returns its
    /// slot.
    pub(crate) fn cursor_add(&mut self, owner: Gc<Object>) -> Result<u32> {
        self.cursors.try_reserve(1)?;
        let slot = u32::try_from(self.cursors.len()).map_err(|_| oom())?;
        self.cursors.push(Cursor {
            owner,
            pos: 0,
            epoch: self.epoch,
        });
        Ok(slot)
    }

    /// The next entry of the iterator `owner` at `slot` and the work
    /// done; `None` if the slot is not the iterator's.
    pub(crate) fn cursor_next(
        &mut self,
        slot: u32,
        owner: Gc<Object>,
    ) -> Option<(Option<Entry>, usize)> {
        let epoch = self.epoch;
        let cursor = self.cursors.get(slot as usize).copied()?;
        if cursor.owner != owner {
            return None;
        }
        let from = if cursor.epoch == epoch { cursor.pos } else { 0 };
        let (entry, next, work) = self.scan(from);
        if let Some(cursor) = self.cursors.get_mut(slot as usize) {
            cursor.pos = next;
            cursor.epoch = epoch;
        }
        Some((entry, work))
    }

    /// Removes the cursor at `slot` (the iterator finished). The last
    /// cursor takes its place: returns that cursor's iterator and its new
    /// slot.
    pub(crate) fn cursor_remove(
        &mut self,
        slot: u32,
        owner: Gc<Object>,
    ) -> Option<(Gc<Object>, u32)> {
        let cursor = self.cursors.get(slot as usize)?;
        if cursor.owner != owner {
            return None;
        }
        self.cursors.swap_remove(slot as usize);
        self.cursors
            .get(slot as usize)
            .map(|moved| (moved.owner, slot))
    }

    /// The number of cursors (for tests).
    #[cfg(test)]
    pub(crate) fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    // --- Weak tables (`weak.rs`) ---

    /// The number of entries, deleted ones included (the positions of
    /// [`Table::entry`]).
    pub(crate) fn entry_len(&self) -> usize {
        self.entries.len()
    }

    /// The entries at the positions `start..end` (fewer at the end).
    pub(crate) fn entries_in(&self, start: usize, end: usize) -> &[Entry] {
        let end = end.min(self.entries.len());
        self.entries.get(start..end).unwrap_or_default()
    }

    /// The sweep of a `WeakMap` or `WeakSet` (which have no cursors and no
    /// loops): removes the live entries whose key `dead` accepts, then
    /// compacts the table, so the memory of the dead entries returns in
    /// the same collection. Returns the number of removed entries.
    pub(crate) fn remove_dead(&mut self, dead: impl Fn(Value) -> bool) -> usize {
        let mut removed = 0;
        for (position, entry) in self.entries.iter_mut().enumerate() {
            if !entry.key.is_empty() && dead(entry.key) {
                entry.key = Value::Empty;
                entry.value = Value::Undefined;
                entry.hash = position as u64 + 1;
                removed += 1;
            }
        }
        if removed > 0 {
            self.live -= removed;
            // This compaction allocates nothing, so it cannot fail: a weak
            // table has no cursors or loops to remap, and with at least one
            // entry removed, the new index is not longer than the old one.
            // A failure would leave the index stale (lookups could miss live
            // keys), so it is an internal error.
            if self.compact().is_err() {
                log::error!("internal error: a weak table could not be compacted");
            }
        }
        removed
    }
}

#[cfg(test)]
impl Table {
    /// The number of entry slots, deleted ones included (for tests).
    pub(super) fn slots(&self) -> usize {
        self.entries.len()
    }

    /// The number of used index slots (for tests).
    pub(super) fn indexed(&self) -> usize {
        self.index.iter().filter(|&&slot| slot != NONE).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_keep_the_index_exact_under_collisions() {
        // Hashes with few distinct values make long runs; the backward
        // shift must keep every live key findable and nothing else.
        let mut table = Table::new();
        let mut present = std::collections::BTreeSet::new();
        let mut state = 0x2545_f491u32;
        let hash_of = |k: i32| (k % 5) as u64 * 0x9e37_79b9;
        for step in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let k = (state % 64) as i32;
            let found = table.find(hash_of(k), |key| matches!(key, Value::Int(x) if x == k));
            assert_eq!(
                found.is_some(),
                present.contains(&k),
                "step {step}, key {k}"
            );
            if let Some(position) = found {
                table.remove(position);
                present.remove(&k);
            } else {
                table
                    .insert(hash_of(k), Value::Int(k), Value::Undefined)
                    .unwrap();
                present.insert(k);
            }
            assert_eq!(table.indexed(), present.len(), "step {step}");
        }
        for k in 0..64 {
            let found = table.find(hash_of(k), |key| matches!(key, Value::Int(x) if x == k));
            assert_eq!(found.is_some(), present.contains(&k), "key {k}");
        }
    }

    #[test]
    fn scans_with_skips_find_the_next_live_entry() {
        // Random deletes anywhere (no compaction: a loop position is
        // held), then scans from random positions against a naive scan.
        let mut table = Table::new();
        let id = table.loop_begin().unwrap();
        let mut state = 0x1357_9bdfu32;
        let mut next = move |n: u32| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state % n
        };
        // Fewer deletes than half the entries: no compaction.
        for k in 0..6000 {
            table
                .insert(k as u64, Value::Int(k), Value::Undefined)
                .unwrap();
        }
        for round in 0..2900 {
            if let Some(position) = table.find(u64::from(next(6000)), |_| true) {
                table.remove(position);
            }
            let from = next(6001);
            let naive =
                (from as usize..table.entries.len()).find(|&p| !table.entries[p].key.is_empty());
            let (entry, after, _) = table.scan(from);
            match naive {
                Some(p) => {
                    assert_eq!(after as usize, p + 1, "round {round}");
                    assert!(matches!(entry, Some(e) if !e.key.is_empty()));
                }
                None => assert!(entry.is_none(), "round {round}"),
            }
        }
        assert_eq!(table.slots(), 6000, "no compaction ran");
        table.loop_end(id);
    }
}
