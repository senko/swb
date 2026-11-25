//! The bound on open pipelines (ADR 0047 section 6): at most 16 pipelines
//! of a process have an open demuxer, decoders and threads; the others wait
//! in a FIFO queue without threads. A pipeline that closes or suspends gives
//! its place to the first one that waits.
//!
//! A run whose threads made no progress for the no-progress limit is
//! detached (ADR 0047 section 11): its place goes to the next pipeline at
//! once, and its threads count as detached until they end. While 16 or more
//! detached threads run, new pipelines fail.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

/// The most open pipelines per process (ADR 0047 section 6).
pub const MAX_OPEN_PIPELINES: usize = 16;

/// The most detached threads per process (ADR 0047 section 11): while this
/// many run, new pipelines fail.
pub const MAX_DETACHED_THREADS: usize = 16;

/// A pipeline that waits for a place: it gets the place, or gives it back
/// (`Some`) when it no longer wants it (closed or suspended meanwhile).
pub(crate) type Waiter = Box<dyn FnOnce(Slot) -> Option<Slot> + Send>;

/// The places of open pipelines, shared by the pipelines that use it.
/// [`PipelineSlots::global`] is the process's; tests make their own.
#[derive(Clone)]
pub struct PipelineSlots(Arc<Inner>);

struct Inner {
    max: usize,
    state: Mutex<State>,
    /// Pipeline threads (demux and video) that run now.
    threads: AtomicUsize,
    /// The most threads that ran at once.
    peak_threads: AtomicUsize,
    /// Threads of detached runs that run now.
    detached: AtomicUsize,
}

#[derive(Default)]
struct State {
    open: usize,
    peak_open: usize,
    /// The waiters with the id of their pipeline.
    waiting: VecDeque<(u64, Waiter)>,
}

impl PipelineSlots {
    /// A bound of `max` open pipelines (at least 1).
    pub fn new(max: usize) -> PipelineSlots {
        PipelineSlots(Arc::new(Inner {
            max: max.max(1),
            state: Mutex::new(State::default()),
            threads: AtomicUsize::new(0),
            peak_threads: AtomicUsize::new(0),
            detached: AtomicUsize::new(0),
        }))
    }

    /// The process's bound of [`MAX_OPEN_PIPELINES`].
    pub fn global() -> PipelineSlots {
        static GLOBAL: OnceLock<PipelineSlots> = OnceLock::new();
        GLOBAL
            .get_or_init(|| PipelineSlots::new(MAX_OPEN_PIPELINES))
            .clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pipelines that hold a place now.
    pub fn open_count(&self) -> usize {
        self.lock().open
    }

    /// The most pipelines that held a place at once.
    pub fn peak_open(&self) -> usize {
        self.lock().peak_open
    }

    /// Pipelines that wait for a place.
    pub fn waiting_count(&self) -> usize {
        self.lock().waiting.len()
    }

    /// Pipeline threads (demux and video, not FFmpeg's own) that run now.
    pub fn live_threads(&self) -> usize {
        self.0.threads.load(Ordering::SeqCst)
    }

    /// The most pipeline threads that ran at once.
    pub fn peak_threads(&self) -> usize {
        self.0.peak_threads.load(Ordering::SeqCst)
    }

    /// Threads of detached runs (no progress within the limit) that still
    /// run.
    pub fn detached_threads(&self) -> usize {
        self.0.detached.load(Ordering::SeqCst)
    }

    /// Gives `waiter` (of the pipeline `owner`) a place now, or queues it
    /// until one is free.
    pub(crate) fn acquire(&self, owner: u64, waiter: Waiter) {
        let slot = {
            let mut st = self.lock();
            if st.open < self.0.max {
                st.open += 1;
                st.peak_open = st.peak_open.max(st.open);
                Some(Slot {
                    slots: self.clone(),
                    armed: true,
                })
            } else {
                st.waiting.push_back((owner, waiter));
                return;
            }
        };
        if let Some(slot) = slot
            && let Some(back) = waiter(slot)
        {
            drop(back);
        }
    }

    /// Removes the waiters of the pipeline `owner` (it closed or
    /// suspended), so that a pipeline that suspends and resumes many times
    /// does not fill the queue.
    pub(crate) fn forget(&self, owner: u64) {
        let gone: VecDeque<(u64, Waiter)> = {
            let mut st = self.lock();
            let (gone, keep) = std::mem::take(&mut st.waiting)
                .into_iter()
                .partition(|(o, _)| *o == owner);
            st.waiting = keep;
            gone
        };
        drop(gone);
    }

    /// Hands a freed place to the waiters in order, or frees it.
    fn release(&self) {
        loop {
            let waiter = {
                let mut st = self.lock();
                let Some((_, w)) = st.waiting.pop_front() else {
                    st.open = st.open.saturating_sub(1);
                    return;
                };
                w
            };
            let slot = Slot {
                slots: self.clone(),
                armed: true,
            };
            match waiter(slot) {
                None => return,
                // The waiter is gone: try the next one with the same place.
                Some(mut back) => back.armed = false,
            }
        }
    }

    /// Counts a pipeline thread until the guard drops.
    fn thread_guard(&self) -> ThreadGuard {
        let n = self.0.threads.fetch_add(1, Ordering::SeqCst) + 1;
        self.0.peak_threads.fetch_max(n, Ordering::SeqCst);
        ThreadGuard {
            slots: self.clone(),
        }
    }
}

/// A place of an open pipeline; dropping it gives the place to the next
/// waiter.
pub(crate) struct Slot {
    slots: PipelineSlots,
    armed: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            self.slots.release();
        }
    }
}

/// Counts one running pipeline thread.
struct ThreadGuard {
    slots: PipelineSlots,
}

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        self.slots.0.threads.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The place of one run and the threads of the run that hold it. The place
/// goes back when the last of them ends, or at once when the run is
/// detached; the threads of a detached run count as detached until they
/// end.
pub(crate) struct RunPlace {
    slots: PipelineSlots,
    state: Mutex<PlaceState>,
}

struct PlaceState {
    slot: Option<Slot>,
    threads: usize,
    detached: bool,
}

impl RunPlace {
    /// The run's place.
    pub(crate) fn new(slot: Slot) -> Arc<RunPlace> {
        Arc::new(RunPlace {
            slots: slot.slots.clone(),
            state: Mutex::new(PlaceState {
                slot: Some(slot),
                threads: 0,
                detached: false,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, PlaceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts a thread of the run until the guard drops (call it before the
    /// thread starts, so that the place is held from then on).
    pub(crate) fn enter(self: &Arc<RunPlace>) -> PlaceGuard {
        let mut st = self.lock();
        st.threads += 1;
        if st.detached {
            self.slots.0.detached.fetch_add(1, Ordering::SeqCst);
        }
        drop(st);
        PlaceGuard {
            place: Arc::clone(self),
            count: Some(self.slots.thread_guard()),
        }
    }

    /// Detaches the run: the place goes to the next pipeline now, and the
    /// run's threads count as detached until they end.
    pub(crate) fn detach(&self) {
        let slot = {
            let mut st = self.lock();
            if st.detached {
                return;
            }
            st.detached = true;
            self.slots
                .0
                .detached
                .fetch_add(st.threads, Ordering::SeqCst);
            st.slot.take()
        };
        drop(slot);
    }
}

/// One thread of a run (see [`RunPlace::enter`]).
pub(crate) struct PlaceGuard {
    place: Arc<RunPlace>,
    count: Option<ThreadGuard>,
}

impl Drop for PlaceGuard {
    fn drop(&mut self) {
        // The thread no longer counts before its place goes to the next
        // pipeline, so that at most two threads run per place.
        drop(self.count.take());
        let slot = {
            let mut st = self.place.lock();
            st.threads = st.threads.saturating_sub(1);
            if st.detached {
                self.place.slots.0.detached.fetch_sub(1, Ordering::SeqCst);
            }
            if st.threads == 0 {
                st.slot.take()
            } else {
                None
            }
        };
        drop(slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn places_go_to_the_waiters_in_order() {
        let slots = PipelineSlots::new(2);
        let held: Arc<StdMutex<Vec<(usize, Slot)>>> = Arc::default();
        for i in 0..5 {
            let held = Arc::clone(&held);
            slots.acquire(
                i as u64,
                Box::new(move |slot| {
                    held.lock().expect("lock").push((i, slot));
                    None
                }),
            );
        }
        assert_eq!(slots.open_count(), 2);
        assert_eq!(slots.waiting_count(), 3);
        let first = held.lock().expect("lock").remove(0);
        assert_eq!(first.0, 0);
        drop(first);
        let order: Vec<usize> = held.lock().expect("lock").iter().map(|(i, _)| *i).collect();
        assert_eq!(order, [1, 2]);
        assert_eq!(slots.open_count(), 2);
        // A waiter that is gone gives its place to the next one.
        let gone = PipelineSlots::new(1);
        let keep: Arc<StdMutex<Vec<Slot>>> = Arc::default();
        gone.acquire(
            0,
            Box::new({
                let keep = Arc::clone(&keep);
                move |s| {
                    keep.lock().expect("lock").push(s);
                    None
                }
            }),
        );
        gone.acquire(1, Box::new(Some));
        gone.acquire(
            2,
            Box::new({
                let keep = Arc::clone(&keep);
                move |s| {
                    keep.lock().expect("lock").push(s);
                    None
                }
            }),
        );
        assert_eq!(gone.waiting_count(), 2);
        let s = keep.lock().expect("lock").remove(0);
        drop(s);
        assert_eq!(keep.lock().expect("lock").len(), 1);
        assert_eq!(gone.open_count(), 1);
        let rest = std::mem::take(&mut *keep.lock().expect("lock"));
        drop(rest);
        assert_eq!(gone.open_count(), 0);
        // Dropping 1 and 2 gives their places to 3 and 4.
        let first_two = std::mem::take(&mut *held.lock().expect("lock"));
        drop(first_two);
        let order: Vec<usize> = held.lock().expect("lock").iter().map(|(i, _)| *i).collect();
        assert_eq!(order, [3, 4]);
        let last = std::mem::take(&mut *held.lock().expect("lock"));
        drop(last);
        assert_eq!(slots.open_count(), 0);
        assert_eq!(slots.peak_open(), 2);
    }

    #[test]
    fn a_forgotten_pipeline_leaves_the_queue() {
        let slots = PipelineSlots::new(1);
        let held: Arc<StdMutex<Vec<Slot>>> = Arc::default();
        let take = |held: &Arc<StdMutex<Vec<Slot>>>| -> Waiter {
            let held = Arc::clone(held);
            Box::new(move |s| {
                held.lock().expect("lock").push(s);
                None
            })
        };
        slots.acquire(1, take(&held));
        // Pipeline 2 waits, suspends and waits again, many times.
        for _ in 0..100 {
            slots.acquire(2, take(&held));
            slots.forget(2);
        }
        slots.acquire(3, take(&held));
        assert_eq!(slots.waiting_count(), 1);
        let first = std::mem::take(&mut *held.lock().expect("lock"));
        drop(first);
        // The place went to pipeline 3.
        assert_eq!(held.lock().expect("lock").len(), 1);
        assert_eq!(slots.open_count(), 1);
    }

    #[test]
    fn a_detached_run_gives_its_place_back_at_once() {
        let slots = PipelineSlots::new(1);
        let held: Arc<StdMutex<Vec<Slot>>> = Arc::default();
        let h = Arc::clone(&held);
        slots.acquire(
            1,
            Box::new(move |s| {
                h.lock().expect("lock").push(s);
                None
            }),
        );
        let slot = held.lock().expect("lock").remove(0);
        let place = RunPlace::new(slot);
        let a = place.enter();
        let b = place.enter();
        assert_eq!(slots.live_threads(), 2);
        place.detach();
        assert_eq!(slots.open_count(), 0, "the place is free");
        assert_eq!(slots.detached_threads(), 2);
        drop(a);
        assert_eq!(slots.detached_threads(), 1);
        drop(b);
        assert_eq!(slots.detached_threads(), 0);
        assert_eq!(slots.live_threads(), 0);
        assert_eq!(slots.open_count(), 0);
        // Without a detach, the last thread gives the place back.
        let h = Arc::clone(&held);
        slots.acquire(
            2,
            Box::new(move |s| {
                h.lock().expect("lock").push(s);
                None
            }),
        );
        let place = RunPlace::new(held.lock().expect("lock").remove(0));
        let a = place.enter();
        let b = place.enter();
        drop(a);
        assert_eq!(slots.open_count(), 1);
        drop(b);
        assert_eq!(slots.open_count(), 0);
    }
}
