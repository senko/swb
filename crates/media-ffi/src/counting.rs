//! A global allocator that counts the calls made on tracked threads, for the
//! tests that show that real-time code does not allocate (ADR 0047 section
//! 8: the mixer's callback). It forwards every call to the system allocator.
//! It lives here because a `GlobalAlloc` needs `unsafe`, which only this
//! crate has; the feature `counting-allocator` is for dev-dependencies, and
//! no product code uses it.
//!
//! Use: `#[global_allocator] static A: CountingAllocator = CountingAllocator;`
//! in a test binary, [`track_thread`] on the thread to watch, and
//! [`calls`] (of the calling thread) before and after.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// True on the threads whose allocator calls are counted.
    static TRACKED: Cell<bool> = const { Cell::new(false) };
    /// Allocator calls (allocate, free, reallocate) of this thread while it
    /// was tracked. Per thread, so that tests that run in parallel in one
    /// binary do not see each other's calls.
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

/// Counts the allocator calls of the current thread from now on (`true`) or
/// stops (`false`).
pub fn track_thread(on: bool) {
    TRACKED.with(|t| t.set(on));
}

/// The allocator calls that the current thread made while it was tracked.
pub fn calls() -> usize {
    CALLS.with(Cell::get)
}

fn count() {
    // `try_with`: a thread that is ending has no access; it counts nothing.
    if TRACKED.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
    }
}

/// The counting allocator (see the module comment).
pub struct CountingAllocator;

// SAFETY: every method counts and forwards to `System` with the same
// arguments, so the contract of `GlobalAlloc` holds as it does for `System`.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
