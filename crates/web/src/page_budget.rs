//! The byte budgets that the runtimes of a page's frames share (ADR 0031 section 11).
//! The heap limit does not count these bytes, so a budget per runtime would multiply
//! by the number of runtimes: 32 sandboxed frames with 256 MiB of Blob URLs each hold
//! 8 GiB with their heaps almost empty. The top level's runtime has a count of its
//! own, so that frames cannot take its budget, as with the heap.
//!
//! The counters are in [`crate::AgentShared`]. Each runtime has a [`ByteShare`] of a
//! counter, and what it holds goes back when the runtime goes.

use std::cell::Cell;
use std::rc::Rc;

/// The bytes that the runtimes of a page's frames hold together, per budget.
#[derive(Clone, Debug, Default)]
pub(crate) struct PageBudgets {
    /// [`crate::file_api::MAX_BLOB_URL_BYTES`].
    pub(crate) blob_urls: Rc<Cell<u64>>,
    /// [`crate::file_api::MAX_HELD_BYTES`].
    pub(crate) held_blobs: Rc<Cell<u64>>,
    /// `MAX_BYTES` in `message_port/hub.rs`.
    pub(crate) messages: Rc<Cell<u64>>,
    /// [`crate::mutation_registry::MAX_RECORD_BYTES`].
    pub(crate) mutation_records: Rc<Cell<u64>>,
}

/// One runtime's part of a byte budget of its page.
#[derive(Debug, Default)]
pub(crate) struct ByteShare {
    page: Rc<Cell<u64>>,
    mine: u64,
}

impl ByteShare {
    /// Counts in the budget `page` from now on.
    pub(crate) fn join(&mut self, page: &Rc<Cell<u64>>) {
        self.page.set(self.page.get().saturating_sub(self.mine));
        page.set(page.get().saturating_add(self.mine));
        self.page = Rc::clone(page);
    }

    /// True if `bytes` more stay within `limit` for the count that it is in.
    pub(crate) fn fits(&self, bytes: u64, limit: u64) -> bool {
        self.page.get().saturating_add(bytes) <= limit
    }

    /// The runtime holds `bytes` more.
    pub(crate) fn add(&mut self, bytes: u64) {
        self.mine = self.mine.saturating_add(bytes);
        self.page.set(self.page.get().saturating_add(bytes));
    }

    /// The runtime holds `bytes` less.
    pub(crate) fn remove(&mut self, bytes: u64) {
        let bytes = bytes.min(self.mine);
        self.mine -= bytes;
        self.page.set(self.page.get().saturating_sub(bytes));
    }

    /// The bytes that this runtime holds.
    pub(crate) fn mine(&self) -> u64 {
        self.mine
    }
}

impl Drop for ByteShare {
    fn drop(&mut self) {
        self.page.set(self.page.get().saturating_sub(self.mine));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_runtimes_share_a_budget_and_a_dropped_one_gives_back() {
        let page = Rc::new(Cell::new(0));
        let mut one = ByteShare::default();
        let mut two = ByteShare::default();
        one.join(&page);
        two.join(&page);
        one.add(60);
        assert!(two.fits(40, 100));
        assert!(!two.fits(41, 100));
        two.add(40);
        assert_eq!((one.mine(), two.mine(), page.get()), (60, 40, 100));
        drop(one);
        assert_eq!(page.get(), 40);
        assert!(two.fits(60, 100));
        two.remove(1000);
        assert_eq!((two.mine(), page.get()), (0, 0));
    }

    #[test]
    fn joining_moves_what_the_runtime_holds() {
        let page = Rc::new(Cell::new(5));
        let mut share = ByteShare::default();
        share.add(10);
        share.join(&page);
        assert_eq!(page.get(), 15);
    }
}
