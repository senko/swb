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

use std::hash::{BuildHasher, Hasher};

use swb_js_text::hash::BuildKeyedHasher;

use crate::error::{Error, Result, Termination};
use crate::heap::{Arena, Gc, Heap, Tracer};
use crate::object::{IterationKind, Object, ObjectKind};
use crate::string::JsString;
use crate::value::{BigInt, Value};

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
const MAX_LIVE: usize = 1 << 24;
#[cfg(test)]
const MAX_LIVE: usize = 1 << 18;

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
struct Cursor {
    /// The iterator object (not traced).
    owner: Gc<Object>,
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
    fn take_cursors(&mut self) -> Vec<Cursor> {
        std::mem::take(&mut self.cursors)
    }

    /// Puts the pruned cursor list back and sets the next prune at twice
    /// its length.
    fn set_cursors(&mut self, cursors: Vec<Cursor>, collections: u64) {
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

// --- SameValueZero and the hash ---

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
    pub(crate) fn table(&self, object: Gc<Object>) -> Result<Option<&Table>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Map(table) | ObjectKind::Set(table) => Some(table),
            _ => None,
        })
    }

    /// The table of a `Map` or `Set` object, for writing.
    pub(crate) fn table_mut(&mut self, object: Gc<Object>) -> Result<&mut Table> {
        match &mut self.object_mut(object)?.kind {
            ObjectKind::Map(table) | ObjectKind::Set(table) => Ok(table),
            _ => Err(Error::invariant("a collection operation on another object")),
        }
    }

    fn table_ref(&self, object: Gc<Object>) -> Result<&Table> {
        self.table(object)?
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
        let table = self.table_ref(object)?;
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
        let table = self.table_ref(object)?;
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
            .table_ref(object)?
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
            self.table_mut(object)?.set_value(position, value);
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
        if self.table_ref(object)?.len() >= MAX_LIVE {
            return Err(Error::range_error(
                if matches!(self.object(object)?.kind, ObjectKind::Map(_)) {
                    "Map maximum size exceeded"
                } else {
                    "Set maximum size exceeded"
                },
            ));
        }
        let mut growth = self.table_ref(object)?.insert_growth();
        if growth > 0 {
            if self.unblock_compaction(object)? {
                growth = self.table_ref(object)?.insert_growth();
            }
            self.check_room(growth)?;
        }
        let table = self.table_mut(object)?;
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
        self.table_mut(object)?.remove(position);
        self.unblock_compaction(object)?;
        Ok(true)
    }

    /// A table whose compaction waits for more deleted entries because
    /// of its cursor list may hold the handles of iterators that the
    /// collector freed: prunes the list (at most once per collection) and
    /// compacts if that made it due. Returns whether it pruned.
    fn unblock_compaction(&mut self, object: Gc<Object>) -> Result<bool> {
        let collections = self.stats().collections;
        let table = self.table_ref(object)?;
        if !table.compaction_blocked() || !table.pruned_before(collections) {
            return Ok(false);
        }
        self.prune_cursors(object)?;
        self.table_mut(object)?.compact_if_due();
        Ok(true)
    }

    /// A copy of the entries of `source` in a new table for `target` (an
    /// empty `Set` object that the caller made): `a copy of [[SetData]]`.
    /// Checks the heap limit first.
    pub(crate) fn collection_copy(&mut self, source: Gc<Object>, target: Gc<Object>) -> Result<()> {
        let live = self.table_ref(source)?.len();
        let bytes = Table::bytes_for(live);
        self.check_room(bytes)?;
        let copy = self.table_ref(source)?.copy_live()?;
        self.charge(copy.heap_size());
        *self.table_mut(target)? = copy;
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
            .table_ref(collection)?
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
        let table = self.table_mut(collection)?;
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
        let mut cursors = self.table_mut(collection)?.take_cursors();
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
        self.table_mut(collection)?
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
        let step = self.table_mut(collection)?.cursor_next(slot, iterator);
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
        let moved = self.table_mut(collection)?.cursor_remove(slot, iterator);
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
        self.table_mut(collection)?.loop_begin()
    }

    /// The next entry of the loop `id` and the work done.
    pub(crate) fn collection_loop_next(
        &mut self,
        collection: Gc<Object>,
        id: usize,
    ) -> Result<(Option<Entry>, usize)> {
        Ok(self.table_mut(collection)?.loop_next(id))
    }

    /// Ends the loop `id`.
    pub(crate) fn collection_loop_end(&mut self, collection: Gc<Object>, id: usize) {
        if let Ok(table) = self.table_mut(collection) {
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
impl Table {
    /// The number of entry slots, deleted ones included (for tests).
    fn slots(&self) -> usize {
        self.entries.len()
    }

    /// The number of used index slots (for tests).
    fn indexed(&self) -> usize {
        self.index.iter().filter(|&&slot| slot != NONE).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots, RootSource};

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
        heap.table(m).unwrap().unwrap()
    }

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
        heap.table_mut(m).unwrap().clear();
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
