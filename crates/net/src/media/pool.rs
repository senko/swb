//! The pool of media fetch threads (ADR 0047 section 5): at most
//! [`MAX_THREADS`] jobs run at a time, apart from the page's loads and the
//! script's fetches, so a slow media server holds neither.
//!
//! A job is a closure that runs on a thread of the pool until it ends. The
//! pool starts a thread when a job arrives and a place is free; the thread
//! takes the next queued job when its job ends, and stops when the queue is
//! empty (no thread waits idle). A job that blocks in a network read that
//! nobody wants any more (a cancelled resource) calls [`MediaPool::release`]:
//! the pool then counts its place as free and starts the next job on another
//! thread, up to [`MAX_DETACHED`] such threads. The released thread stops
//! when its job returns.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use log::error;

/// The most jobs that run at the same time.
pub(crate) const MAX_THREADS: usize = 16;

/// The most threads that run a job whose place was released.
const MAX_DETACHED: usize = 16;

/// The place of a running job. [`MediaPool::release`] marks it. Both flags
/// change only under the pool lock.
#[derive(Debug, Default)]
pub(crate) struct Slot {
    released: AtomicBool,
    /// The job returned (a late `release` must not touch the counters).
    done: AtomicBool,
}

type Job = Box<dyn FnOnce(&Arc<Slot>) + Send>;

#[derive(Default)]
struct PoolState {
    queue: VecDeque<Job>,
    /// Jobs that run and hold their place.
    active: usize,
    /// Threads that run a job whose place was released.
    detached: usize,
}

/// A pool of at most [`MAX_THREADS`] threads for media fetches.
#[derive(Default)]
pub(crate) struct MediaPool {
    state: Mutex<PoolState>,
}

impl MediaPool {
    pub(crate) fn new() -> Arc<MediaPool> {
        Arc::new(MediaPool::default())
    }

    fn state(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `job` on a thread of the pool, or queues it when all places are
    /// taken. A job that cannot start (no thread) is dropped.
    pub(crate) fn submit(self: &Arc<Self>, job: Job) {
        let mut state = self.state();
        if state.active < MAX_THREADS {
            state.active += 1;
            drop(state);
            self.spawn(job);
        } else {
            state.queue.push_back(job);
        }
    }

    /// The job of `slot` waits for the network and nobody needs its result:
    /// its place is free for the next job. Does nothing when too many
    /// threads are in this state (the job keeps its place).
    pub(crate) fn release(self: &Arc<Self>, slot: &Arc<Slot>) {
        let mut state = self.state();
        if slot.done.load(Ordering::Relaxed)
            || slot.released.load(Ordering::Relaxed)
            || state.detached >= MAX_DETACHED
        {
            return;
        }
        slot.released.store(true, Ordering::Relaxed);
        state.detached += 1;
        state.active -= 1;
        if let Some(job) = state.queue.pop_front() {
            state.active += 1;
            drop(state);
            self.spawn(job);
        }
    }

    /// The number of jobs that wait for a place.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.state().queue.len()
    }

    /// Starts a thread for `job` (the caller counted its place).
    fn spawn(self: &Arc<Self>, job: Job) {
        let pool = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("swb-media-fetch".to_owned())
            .spawn(move || pool.run(job));
        if let Err(error) = spawned {
            // The job was moved into the closure and is dropped with it.
            error!("cannot start a media fetch thread: {error}");
            self.state().active -= 1;
        }
    }

    fn run(self: &Arc<Self>, first: Job) {
        let mut job = first;
        loop {
            let slot = Arc::new(Slot::default());
            job(&slot);
            let mut state = self.state();
            // Under the pool lock, before any counter changes: a later
            // `release` of this slot sees `done` and does nothing.
            slot.done.store(true, Ordering::Relaxed);
            if slot.released.load(Ordering::Relaxed) {
                // Its place was given away already.
                state.detached -= 1;
                return;
            }
            let Some(next) = state.queue.pop_front() else {
                state.active -= 1;
                return;
            };
            job = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn at_most_sixteen_jobs_run_and_the_rest_wait() {
        let pool = MediaPool::new();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        for _ in 0..MAX_THREADS + 4 {
            let started = started.clone();
            let release_rx = Arc::clone(&release_rx);
            pool.submit(Box::new(move |_| {
                started.send(()).unwrap();
                let _ = release_rx.lock().unwrap().recv();
            }));
        }
        for _ in 0..MAX_THREADS {
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        assert!(started_rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(pool.queued(), 4);
        for _ in 0..MAX_THREADS + 4 {
            release.send(()).unwrap();
        }
        for _ in 0..4 {
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }

    #[test]
    fn a_released_job_gives_its_place_to_the_next() {
        let pool = MediaPool::new();
        let (release, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let (slots_tx, slots_rx) = mpsc::channel();
        for _ in 0..MAX_THREADS {
            let release_rx = Arc::clone(&release_rx);
            let slots_tx = slots_tx.clone();
            pool.submit(Box::new(move |slot| {
                slots_tx.send(Arc::clone(slot)).unwrap();
                let _ = release_rx.lock().unwrap().recv();
            }));
        }
        let slot = slots_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (done, done_rx) = mpsc::channel();
        pool.submit(Box::new(move |_| done.send(()).unwrap()));
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        pool.release(&slot);
        done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        for _ in 0..MAX_THREADS {
            release.send(()).unwrap();
        }
    }

    #[test]
    fn a_late_release_after_the_job_returned_changes_nothing() {
        let pool = MediaPool::new();
        let (slots_tx, slots_rx) = mpsc::channel();
        pool.submit(Box::new(move |slot| {
            slots_tx.send(Arc::clone(slot)).unwrap();
        }));
        let slot = slots_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        // Wait until the thread has finished the job (its place is free).
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while pool.state().active != 0 {
            assert!(std::time::Instant::now() < deadline, "the job did not end");
            thread::yield_now();
        }
        pool.release(&slot);
        {
            let state = pool.state();
            assert_eq!(state.active, 0);
            assert_eq!(state.detached, 0);
        }
        // The next jobs still run, and the pool still caps at MAX_THREADS.
        let (done, done_rx) = mpsc::channel();
        for _ in 0..3 {
            let done = done.clone();
            pool.submit(Box::new(move |_| done.send(()).unwrap()));
        }
        for _ in 0..3 {
            done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }
}
