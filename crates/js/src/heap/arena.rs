//! Arenas of heap slots and the typed handles into them (ADR 0026
//! section 8, memo 4.4).
//!
//! An arena is a vector of slots. A slot holds the data of one heap
//! thing or is free. A handle ([`Gc`]) is the slot index and the slot's
//! generation at the time of the allocation. Freeing a slot increments
//! its generation, so every older handle to it becomes stale. Every
//! access compares the generations; a stale handle gives an internal
//! error, never a panic and never the data of another thing.
//!
//! A slot whose generation reaches `u32::MAX` is retired: it is never
//! reused, so generations never wrap around.
//!
//! Memory rule (ADR 0026 section 8): the heap limit bounds the capacity of
//! the slot vectors, free slots included, not only the live slots. After a
//! sweep, [`Arena::sweep`] truncates the trailing free slots and shrinks
//! the vector when more than half of its capacity is unused. Free slots
//! inside the vector stay (they are reused). A truncated slot leaves its
//! generation behind in `floor`, so a new slot at the same index never
//! accepts an old handle.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::error::{Error, InternalError, Result, Termination};

/// A handle to a heap thing of type `T`: a slot index and a generation.
///
/// Handles are plain data (`Copy`, 8 bytes). A handle keeps nothing
/// alive: the thing lives while the collector can reach it from a root
/// (ADR 0026 section 8).
pub struct Gc<T> {
    index: u32,
    generation: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> Gc<T> {
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Gc {
            index,
            generation,
            kind: PhantomData,
        }
    }

    /// The slot index.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation of the slot when the thing was allocated.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> Clone for Gc<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Gc<T> {}

impl<T> PartialEq for Gc<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Gc<T> {}

impl<T> Hash for Gc<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64((u64::from(self.generation) << 32) | u64::from(self.index));
    }
}

impl<T> fmt::Debug for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Gc({}#{})", self.index, self.generation)
    }
}

/// The end marker of the free list.
const NO_SLOT: u32 = u32::MAX;

struct Slot<T> {
    generation: u32,
    state: State<T>,
}

enum State<T> {
    Live(T),
    /// A free slot; the index of the next free slot or [`NO_SLOT`].
    Free(u32),
    /// The generation is exhausted; the slot is never used again.
    Retired,
}

/// The slots of one kind of heap thing.
pub(crate) struct Arena<T> {
    /// The kind name, for error messages.
    name: &'static str,
    slots: Vec<Slot<T>>,
    free_head: u32,
    live: usize,
    /// The generation of slots created by growing the vector: at least
    /// the generation of every slot that a sweep truncated.
    floor: u32,
}

impl<T> Arena<T> {
    pub(crate) const fn new(name: &'static str) -> Self {
        Arena {
            name,
            slots: Vec::new(),
            free_head: NO_SLOT,
            live: 0,
            floor: 0,
        }
    }

    /// The bytes that one slot costs, without the data it owns.
    pub(crate) const fn slot_size() -> usize {
        size_of::<Slot<T>>()
    }

    /// The number of live slots.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// The number of slots, live or not (the size of the mark bits).
    pub(crate) fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// The bytes of all slots below the length, live or free, which the
    /// heap limit counts (the data the slots own is counted separately).
    /// The reserved capacity above the length is not counted: its pages
    /// become resident only when slots are written, so counting it would
    /// end scripts with only half the limit in use after the vector
    /// doubles. The sweep gives back the free slots at the end.
    pub(crate) fn slot_bytes(&self) -> usize {
        self.slots.len() * Self::slot_size()
    }

    fn stale(&self) -> Error {
        Error::Internal(InternalError::StaleHandle(self.name))
    }

    /// Stores `value` in a free slot and returns its handle.
    pub(crate) fn insert(&mut self, value: T) -> Result<Gc<T>> {
        if self.free_head != NO_SLOT {
            let index = self.free_head;
            let slot = self
                .slots
                .get_mut(index as usize)
                .ok_or(Error::invariant("free list points outside the arena"))?;
            let State::Free(next) = slot.state else {
                return Err(Error::invariant("free list points to a used slot"));
            };
            self.free_head = next;
            slot.state = State::Live(value);
            self.live += 1;
            return Ok(Gc::new(index, slot.generation));
        }
        let index = u32::try_from(self.slots.len())
            .ok()
            .filter(|&index| index != NO_SLOT)
            .ok_or(Termination::OutOfMemory)?;
        self.slots.try_reserve(1)?;
        let generation = self.floor;
        self.slots.push(Slot {
            generation,
            state: State::Live(value),
        });
        self.live += 1;
        Ok(Gc::new(index, generation))
    }

    /// The data of a live handle.
    #[inline]
    pub(crate) fn get(&self, handle: Gc<T>) -> Result<&T> {
        match self.slots.get(handle.index as usize) {
            Some(Slot {
                generation,
                state: State::Live(value),
            }) if *generation == handle.generation => Ok(value),
            _ => Err(self.stale()),
        }
    }

    /// The data of a live handle, for writing.
    #[inline]
    pub(crate) fn get_mut(&mut self, handle: Gc<T>) -> Result<&mut T> {
        let name = self.name;
        match self.slots.get_mut(handle.index as usize) {
            Some(Slot {
                generation,
                state: State::Live(value),
            }) if *generation == handle.generation => Ok(value),
            _ => Err(Error::Internal(InternalError::StaleHandle(name))),
        }
    }

    /// Whether the handle refers to a live slot.
    pub(crate) fn contains(&self, handle: Gc<T>) -> bool {
        self.get(handle).is_ok()
    }

    /// The data at a slot index if the slot is live, with no generation
    /// check (the marker checked it when it set the mark bit).
    #[inline]
    pub(crate) fn at(&self, index: u32) -> Option<&T> {
        match self.slots.get(index as usize) {
            Some(Slot {
                state: State::Live(value),
                ..
            }) => Some(value),
            _ => None,
        }
    }

    /// The live slots with their indices.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match &slot.state {
                State::Live(value) => Some((index as u32, value)),
                _ => None,
            })
    }

    /// Frees every live slot whose mark bit is clear: drops its data,
    /// increments its generation and puts it on the free list. Returns the
    /// number of freed slots and the bytes of the data that surviving
    /// slots own (`size` of the data; the slots themselves are counted by
    /// [`Arena::slot_bytes`]). Then gives back the trailing free slots.
    pub(crate) fn sweep(&mut self, marks: &MarkBits, size: impl Fn(&T) -> usize) -> (usize, usize) {
        let mut freed = 0;
        let mut live_bytes = 0;
        // Walk from the end, so that the free list hands out low indices
        // first.
        for index in (0..self.slots.len()).rev() {
            let Some(slot) = self.slots.get_mut(index) else {
                continue;
            };
            let State::Live(value) = &slot.state else {
                continue;
            };
            if marks.get(index) {
                live_bytes += size(value);
                continue;
            }
            slot.generation = slot.generation.wrapping_add(1);
            if slot.generation == u32::MAX {
                slot.state = State::Retired;
            } else {
                slot.state = State::Free(self.free_head);
                self.free_head = index as u32;
            }
            freed += 1;
        }
        self.live -= freed;
        self.trim_tail();
        (freed, live_bytes)
    }

    /// Truncates the trailing free slots and shrinks the vector when more
    /// than half of the capacity is unused. Rebuilds the free list.
    fn trim_tail(&mut self) {
        let mut len = self.slots.len();
        let mut floor = self.floor;
        while let Some(Slot {
            generation,
            state: State::Free(_),
        }) = len.checked_sub(1).and_then(|last| self.slots.get(last))
            && *generation < u32::MAX - 1
        {
            floor = floor.max(*generation);
            len -= 1;
        }
        if len < self.slots.len() {
            self.slots.truncate(len);
            self.floor = floor;
            self.rebuild_free_list();
        }
        if self.slots.capacity() > 64 && self.slots.capacity() / 2 > self.slots.len() {
            self.slots
                .shrink_to(self.slots.len() + self.slots.len() / 4);
        }
    }

    /// Links the free slots again, lowest index first.
    fn rebuild_free_list(&mut self) {
        let mut head = NO_SLOT;
        for (index, slot) in self.slots.iter_mut().enumerate().rev() {
            if let State::Free(next) = &mut slot.state {
                *next = head;
                head = index as u32;
            }
        }
        self.free_head = head;
    }
}

/// Mark bits beside an arena: one bit per slot.
#[derive(Default)]
pub(crate) struct MarkBits {
    words: Vec<u64>,
}

impl MarkBits {
    /// Clears all bits and makes room for `len` slots.
    pub(crate) fn reset(&mut self, len: usize) {
        self.words.clear();
        self.words.resize(len.div_ceil(64), 0);
    }

    /// Sets the bit of `index`; returns whether it was clear before.
    #[inline]
    pub(crate) fn set(&mut self, index: usize) -> bool {
        match self.words.get_mut(index / 64) {
            Some(word) => {
                let bit = 1u64 << (index % 64);
                let was_clear = *word & bit == 0;
                *word |= bit;
                was_clear
            }
            None => false,
        }
    }

    /// Whether the bit of `index` is set.
    #[inline]
    pub(crate) fn get(&self, index: usize) -> bool {
        self.words
            .get(index / 64)
            .is_some_and(|word| word & (1u64 << (index % 64)) != 0)
    }

    /// Frees the bit vector (the bits live only during a collection).
    pub(crate) fn release(&mut self) {
        self.words = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freed_slots_make_old_handles_stale() {
        let mut arena = Arena::new("test");
        let a = arena.insert(1u32).unwrap();
        let b = arena.insert(2u32).unwrap();
        let mut marks = MarkBits::default();
        marks.reset(arena.capacity());
        marks.set(b.index() as usize);
        let (freed, _) = arena.sweep(&marks, |_| 0);
        assert_eq!(freed, 1);
        assert_eq!(
            arena.get(a),
            Err(Error::Internal(InternalError::StaleHandle("test")))
        );
        assert_eq!(arena.get(b), Ok(&2));
        // The slot is reused with a new generation; the old handle stays
        // stale.
        let c = arena.insert(3u32).unwrap();
        assert_eq!(c.index(), a.index());
        assert_ne!(c.generation(), a.generation());
        assert!(arena.get(a).is_err());
        assert_eq!(arena.get(c), Ok(&3));
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn exhausted_generations_retire_the_slot() {
        let mut arena = Arena::new("test");
        let a = arena.insert(1u8).unwrap();
        if let Some(slot) = arena.slots.get_mut(0) {
            slot.generation = u32::MAX - 1;
        }
        let marks = MarkBits::default();
        arena.sweep(&marks, |_| 0);
        let b = arena.insert(2u8).unwrap();
        assert_ne!(b.index(), a.index());
        assert!(arena.get(a).is_err());
        assert!(arena.at(0).is_none());
    }

    #[test]
    fn handles_out_of_range_are_stale() {
        let arena: Arena<u8> = Arena::new("test");
        assert!(arena.get(Gc::new(5, 0)).is_err());
    }
}
