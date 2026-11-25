//! The block cache of a media resource (ADR 0047 section 5): blocks of
//! [`BLOCK`] bytes at multiples of [`BLOCK`], within a byte limit per
//! resource and a [`ByteBudget`] shared by all resources of a page.
//!
//! When a new block needs room, blocks go in this order: those behind the
//! read position, oldest first, except the newest `keep_behind` bytes (for
//! short seeks back); those ahead of the read position past the first gap,
//! the farthest first; then the rest of those behind. The blocks that run
//! without a gap from the read position are never evicted: the reader needs
//! them next.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use crate::budget::ByteBudget;

/// The size of a cache block and of a network read chunk.
pub const BLOCK: usize = 64 * 1024;

/// [`BLOCK`] as a `u64`.
pub(crate) const BLOCK_U64: u64 = BLOCK as u64;

/// A resource that holds less than this many bytes may reserve past the page
/// budget, so that a resource with an empty cache still progresses when
/// other resources hold the whole budget (at most 16 resources run, so the
/// excess is below 32 MiB).
pub(crate) const FLOOR_BYTES: usize = 2 << 20;

/// The cache of one resource.
pub(crate) struct Cache {
    blocks: BTreeMap<u64, Vec<u8>>,
    /// The bytes taken from the budget: the blocks in the map and the blocks
    /// that a fetch thread fills now ([`Cache::reserve`]).
    reserved: usize,
    peak: usize,
    limit: usize,
    keep_behind: usize,
    budget: Arc<ByteBudget>,
    /// After [`Cache::close`].
    closed: bool,
    /// The bytes that may pass the page budget ([`FLOOR_BYTES`]).
    floor: usize,
}

impl Cache {
    pub(crate) fn new(limit: usize, keep_behind: usize, budget: Arc<ByteBudget>) -> Cache {
        Cache {
            blocks: BTreeMap::new(),
            reserved: 0,
            peak: 0,
            limit,
            keep_behind,
            budget,
            closed: false,
            floor: FLOOR_BYTES,
        }
    }

    /// The block with index `index`.
    pub(crate) fn get(&self, index: u64) -> Option<&[u8]> {
        self.blocks.get(&index).map(Vec::as_slice)
    }

    pub(crate) fn contains(&self, index: u64) -> bool {
        self.blocks.contains_key(&index)
    }

    /// The first block at or after `from` that is not in the cache.
    pub(crate) fn first_missing(&self, from: u64) -> u64 {
        let mut next = from;
        for &index in self.blocks.range(from..).map(|(index, _)| index) {
            if index != next {
                break;
            }
            next += 1;
        }
        next
    }

    /// The most bytes that the cache may hold.
    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    /// The bytes behind the read position that eviction keeps.
    pub(crate) fn keep_behind(&self) -> usize {
        self.keep_behind
    }

    /// The bytes held now (blocks, and blocks being filled).
    pub(crate) fn reserved(&self) -> usize {
        self.reserved
    }

    /// The most bytes held at one time.
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }

    /// Takes `bytes` for a block that a fetch thread is about to fill,
    /// evicting blocks as the module documentation says when the limit of
    /// the resource or the page budget is reached. `read_block` is the block
    /// of the read position. Returns false if there is no room that
    /// eviction can make.
    pub(crate) fn reserve(&mut self, read_block: u64, bytes: usize) -> bool {
        if self.closed {
            return false;
        }
        loop {
            if self.reserved + bytes <= self.limit {
                // The floor: a resource that holds little always gets room,
                // also when other resources hold the whole page budget (at
                // most 16 resources run, so the excess is bounded).
                let within_floor = self.reserved + bytes <= self.floor;
                if self.budget.try_add(bytes) {
                    self.reserved += bytes;
                } else if within_floor {
                    self.budget.add(bytes);
                    self.reserved += bytes;
                } else if !self.evict_one(read_block) {
                    return false;
                } else {
                    continue;
                }
                self.peak = self.peak.max(self.reserved);
                return true;
            }
            if !self.evict_one(read_block) {
                return false;
            }
        }
    }

    /// Gives back `bytes` that [`Cache::reserve`] took and no block holds.
    pub(crate) fn unreserve(&mut self, bytes: usize) {
        if self.closed {
            return;
        }
        self.reserved -= bytes.min(self.reserved);
        self.budget.sub(bytes);
    }

    /// Frees every block and gives all reserved bytes back to the budget at
    /// once, also those of a block that a fetch thread still fills; the
    /// cache takes no more bytes. For a resource whose last owner is gone
    /// while a job thread still holds the cache.
    pub(crate) fn close(&mut self) {
        self.blocks.clear();
        self.budget.sub(self.reserved);
        self.reserved = 0;
        self.closed = true;
    }

    /// Puts the block `index` in the cache. The caller reserved
    /// [`BLOCK`] bytes for it; a shorter block (the end of the resource)
    /// gives the difference back. A block that is there already is kept
    /// and the new one is dropped.
    pub(crate) fn insert(&mut self, index: u64, data: Vec<u8>) {
        if self.closed {
            return;
        }
        self.unreserve(BLOCK - data.len().min(BLOCK));
        let length = data.len().min(BLOCK);
        if let Entry::Vacant(slot) = self.blocks.entry(index) {
            slot.insert(data);
        } else {
            self.unreserve(length);
        }
    }

    /// Removes the block that goes first; false if none may go.
    fn evict_one(&mut self, read_block: u64) -> bool {
        let run_end = self.first_missing(read_block);
        let keep_blocks = (self.keep_behind / BLOCK) as u64;
        let behind_old = self
            .blocks
            .range(..read_block.saturating_sub(keep_blocks))
            .next()
            .map(|(&index, _)| index);
        let ahead_far = self
            .blocks
            .range(run_end..)
            .next_back()
            .map(|(&index, _)| index);
        let behind_near = self
            .blocks
            .range(..read_block)
            .next()
            .map(|(&index, _)| index);
        let Some(victim) = behind_old.or(ahead_far).or(behind_near) else {
            return false;
        };
        if let Some(data) = self.blocks.remove(&victim) {
            self.reserved -= data.len().min(self.reserved);
            self.budget.sub(data.len());
        }
        true
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        self.budget.sub(self.reserved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(byte: u8) -> Vec<u8> {
        vec![byte; BLOCK]
    }

    fn fill(cache: &mut Cache, read_block: u64, indexes: &[u64]) {
        for &index in indexes {
            assert!(cache.reserve(read_block, BLOCK), "room for {index}");
            cache.insert(index, block(index as u8));
        }
    }

    fn indexes(cache: &Cache) -> Vec<u64> {
        cache.blocks.keys().copied().collect()
    }

    #[test]
    fn first_missing_follows_the_run() {
        let budget = ByteBudget::new(1 << 30);
        let mut cache = Cache::new(1 << 30, 0, budget);
        fill(&mut cache, 0, &[2, 3, 4, 7]);
        assert_eq!(cache.first_missing(0), 0);
        assert_eq!(cache.first_missing(2), 5);
        assert_eq!(cache.first_missing(7), 8);
        assert!(cache.get(3).is_some() && cache.get(5).is_none());
    }

    #[test]
    fn behind_blocks_go_first_and_the_newest_stay() {
        let budget = ByteBudget::new(1 << 30);
        // Room for 6 blocks; keep 2 blocks behind the read position.
        let mut cache = Cache::new(6 * BLOCK, 2 * BLOCK, budget);
        fill(&mut cache, 10, &[4, 5, 6, 7, 8, 9]);
        fill(&mut cache, 10, &[10]);
        assert_eq!(indexes(&cache), [5, 6, 7, 8, 9, 10]);
        fill(&mut cache, 10, &[11, 12, 13]);
        // 5, 6 and 7 are beyond the window of 2 blocks (8 and 9 stay).
        assert_eq!(indexes(&cache), [8, 9, 10, 11, 12, 13]);
        // The window goes only when nothing else may: 8 first.
        fill(&mut cache, 10, &[14]);
        assert_eq!(indexes(&cache), [9, 10, 11, 12, 13, 14]);
        assert_eq!(cache.reserved(), 6 * BLOCK);
    }

    #[test]
    fn far_blocks_ahead_go_before_the_window_and_the_run_stays() {
        let budget = ByteBudget::new(1 << 30);
        let mut cache = Cache::new(5 * BLOCK, 4 * BLOCK, budget);
        // The reader is at block 10: 10 and 11 are its run, 20 and 30 are
        // beyond a gap, 9 is behind (inside the window).
        fill(&mut cache, 10, &[9, 10, 11, 20, 30]);
        fill(&mut cache, 10, &[12]);
        assert_eq!(indexes(&cache), [9, 10, 11, 12, 20]);
        fill(&mut cache, 10, &[13]);
        assert_eq!(indexes(&cache), [9, 10, 11, 12, 13]);
        // Nothing but the window and the run: the window goes last.
        fill(&mut cache, 10, &[14]);
        assert_eq!(indexes(&cache), [10, 11, 12, 13, 14]);
        // Only the run is left: no room.
        assert!(!cache.reserve(10, BLOCK));
        assert_eq!(cache.reserved(), 5 * BLOCK);
    }

    #[test]
    fn the_shared_budget_limits_and_gets_its_bytes_back() {
        let budget = ByteBudget::new(3 * BLOCK);
        let mut a = Cache::new(10 * BLOCK, 0, Arc::clone(&budget));
        let mut b = Cache::new(10 * BLOCK, 0, Arc::clone(&budget));
        // No floor: this test is about the limit itself.
        (a.floor, b.floor) = (0, 0);
        fill(&mut a, 0, &[0, 1]);
        fill(&mut b, 0, &[0]);
        assert_eq!(budget.used(), 3 * BLOCK);
        // `b` has only its run: no room while `a` holds two blocks.
        assert!(!b.reserve(0, BLOCK));
        // A short block gives back what it does not use.
        assert!(a.reserve(1, BLOCK));
        a.insert(2, vec![1; 100]);
        assert_eq!(budget.used(), 2 * BLOCK + 100);
        drop(a);
        assert_eq!(budget.used(), BLOCK);
        drop(b);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn the_floor_passes_the_page_budget_for_a_small_cache_only() {
        // The page budget holds one block and is taken already.
        let budget = ByteBudget::new(BLOCK);
        let mut other = Cache::new(10 * BLOCK, 0, Arc::clone(&budget));
        other.floor = 0;
        fill(&mut other, 0, &[0]);
        let mut small = Cache::new(1 << 30, 0, Arc::clone(&budget));
        let blocks = (FLOOR_BYTES / BLOCK) as u64;
        for index in 0..blocks {
            assert!(small.reserve(0, BLOCK), "room for {index} below the floor");
            small.insert(index, block(0));
        }
        assert_eq!(budget.used(), BLOCK + FLOOR_BYTES);
        // Past the floor the budget limits again (and nothing can go: the
        // blocks are the run at the read position).
        assert!(!small.reserve(0, BLOCK));
        drop(small);
        assert_eq!(budget.used(), BLOCK);
    }

    #[test]
    fn close_frees_everything_at_once_and_ignores_later_calls() {
        let budget = ByteBudget::new(1 << 30);
        let mut cache = Cache::new(1 << 30, 0, Arc::clone(&budget));
        fill(&mut cache, 0, &[0, 1]);
        // A block that a fetch thread fills now.
        assert!(cache.reserve(0, BLOCK));
        assert_eq!(budget.used(), 3 * BLOCK);
        cache.close();
        assert_eq!(budget.used(), 0);
        assert_eq!(cache.reserved(), 0);
        assert!(cache.get(0).is_none());
        // The thread ends later: it gives its block back, or inserts it.
        cache.unreserve(BLOCK);
        cache.insert(2, block(2));
        assert!(!cache.reserve(0, BLOCK));
        assert_eq!(budget.used(), 0);
        drop(cache);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn a_duplicate_block_is_dropped_without_a_leak() {
        let budget = ByteBudget::new(1 << 30);
        let mut cache = Cache::new(1 << 30, 0, Arc::clone(&budget));
        fill(&mut cache, 0, &[3]);
        fill(&mut cache, 0, &[3]);
        assert_eq!(cache.reserved(), BLOCK);
        assert_eq!(budget.used(), BLOCK);
        assert_eq!(cache.peak(), 2 * BLOCK);
    }
}
