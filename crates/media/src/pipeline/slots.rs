//! The bound on open pipelines (ADR 0047 section 6): at most 16 pipelines
//! of a process have an open demuxer, decoders and threads; the others wait
//! in a FIFO queue without threads. A pipeline that closes or suspends gives
//! its place to the first one that waits.
//!
//! A run whose threads made no progress for the no-progress limit is
//! detached (ADR 0047 section 11): its place goes to the next pipeline at
//! once, and its threads count as detached until they end. While 16 or more
//! detached threads run, new pipelines fail.
//!
//! **Handover** (M14 feature 6a). A run that ends (the pipeline suspends,
//! closes or fails) keeps its place until its threads have ended; the last
//! one hands it to the first waiter. From the end of the run until the
//! waiter has the place, the place is *in transit*
//! ([`PipelineSlots::in_transit`]): a waiter then has pending work
//! ([`super::Pipeline::awaits_handover`]), so virtual time does not move
//! while the threads end, and the waiter's events come at the same page
//! time on every run. When a transit ends, every waiter's owner is notified
//! (it may have no pending work any more). A place that stays in transit
//! for the no-progress limit (a thread blocked in FFmpeg) is detached.
//!
//! **The frame budget** (M14 feature 10a, ADR 0047 "Implementation notes
//! (M14 feature 10a)"). The pipelines of the slots share a budget of bytes
//! for their decoded frames (the RGBA pixmaps of the frame queue, the
//! current frame, the frame being converted and the one that paint holds):
//! [`FRAME_BUDGET_BYTES`] for the process. A run reserves its frames when
//! its demux thread knows the frame size ([`RunPlace::reserve_frames`]): as
//! many as its queue wants, or fewer (never below the minimum that playback
//! needs); when even the minimum does not fit, the run waits in a FIFO with
//! its place and without pending work, as a pipeline that waits for a place.
//! The reservation belongs to the run's place and goes back with it (when
//! the run's threads have ended, or at a detach), so the handover of the
//! freed bytes to the waiters happens in the place's transit and has the
//! same page time on every run in virtual time. A run whose decoded frames
//! are larger than the size it reserved for (the stream changes its frame
//! size) grows its reservation at once from the free bytes, or fails
//! ([`RunPlace::grow_frames`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::Notify;

/// The most open pipelines per process (ADR 0047 section 6).
pub const MAX_OPEN_PIPELINES: usize = 16;

/// The bytes of decoded frames that the pipelines of the process hold at
/// most (M14 feature 10a): 2 GiB. Sixteen 1080p pipelines (74.6 MB each)
/// and twelve 4K ones (166 MB) fit; three at the frame size limit (713 MB).
pub const FRAME_BUDGET_BYTES: u64 = 2 << 30;

/// A wake-up of a run's demux thread that waits for frame bytes (it clears
/// the pipeline's wait flag under the pipeline's lock and notifies the
/// thread); called without the slots' lock.
pub(crate) type FrameWake = Box<dyn Fn() + Send + Sync>;

/// The answer of [`RunPlace::reserve_frames`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reserve {
    /// The bytes that the run holds now.
    Granted(u64),
    /// The run waits in the FIFO.
    Waiting,
    /// The minimum is larger than the whole budget.
    Refused,
}

/// A run that waits for frame bytes.
struct FrameWaiter {
    /// The pipeline's id.
    owner: u64,
    place: Weak<RunPlace>,
    /// The fewest bytes that the run can play with, and the bytes it wants.
    min: u64,
    want: u64,
    /// The owner's notify and the thread's wake-up.
    notify: Notify,
    wake: Arc<FrameWake>,
}

/// The most detached threads per process (ADR 0047 section 11): while this
/// many run, new pipelines fail.
pub const MAX_DETACHED_THREADS: usize = 16;

/// A pipeline that waits for a place: it gets the place, or gives it back
/// (`Some`) when it no longer wants it (closed or suspended meanwhile).
pub(crate) type Waiter = Box<dyn FnOnce(Slot) -> Option<Slot> + Send>;

/// The places of open pipelines, shared by the pipelines that use it.
/// [`PipelineSlots::global`] is the process's; tests make their own. Two
/// handles are equal when they share their places.
#[derive(Clone)]
pub struct PipelineSlots(Arc<Inner>);

impl PartialEq for PipelineSlots {
    fn eq(&self, other: &PipelineSlots) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PipelineSlots {}

struct Inner {
    max: usize,
    state: Mutex<State>,
    /// Pipeline threads (demux and video) that run now.
    threads: AtomicUsize,
    /// The most threads that ran at once.
    peak_threads: AtomicUsize,
    /// Threads of detached runs that run now.
    detached: AtomicUsize,
    /// Places in transit: their run ended, and they are not yet with the
    /// next waiter (or free).
    transit: AtomicUsize,
    /// Tests: the last thread of an ended run waits this long (ms) before
    /// it gives its place back ([`PipelineSlots::set_handover_delay`]).
    handover_delay_ms: AtomicU64,
    /// The budget of decoded frames, in bytes.
    frame_budget: u64,
}

#[derive(Default)]
struct State {
    open: usize,
    peak_open: usize,
    /// The waiters with the id and the notify of their pipeline.
    waiting: VecDeque<(u64, Notify, Waiter)>,
    /// The frame bytes that the places hold now, and the most at once.
    frames: u64,
    peak_frames: u64,
    /// The runs that wait for frame bytes, in order.
    frame_waiting: VecDeque<FrameWaiter>,
}

impl State {
    /// Counts `n` more frame bytes in the places and keeps the peak.
    fn add_frame_bytes(&mut self, n: u64) {
        self.frames += n;
        self.peak_frames = self.peak_frames.max(self.frames);
    }
}

impl PipelineSlots {
    /// A bound of `max` open pipelines (at least 1), with the frame budget
    /// of the process ([`FRAME_BUDGET_BYTES`]).
    pub fn new(max: usize) -> PipelineSlots {
        PipelineSlots::with_frame_budget(max, FRAME_BUDGET_BYTES)
    }

    /// A bound of `max` open pipelines whose decoded frames share
    /// `frame_budget` bytes (tests set a small budget).
    pub fn with_frame_budget(max: usize, frame_budget: u64) -> PipelineSlots {
        PipelineSlots(Arc::new(Inner {
            max: max.max(1),
            state: Mutex::new(State::default()),
            threads: AtomicUsize::new(0),
            peak_threads: AtomicUsize::new(0),
            detached: AtomicUsize::new(0),
            transit: AtomicUsize::new(0),
            handover_delay_ms: AtomicU64::new(0),
            frame_budget,
        }))
    }

    /// The budget of decoded frames, in bytes.
    pub fn frame_budget(&self) -> u64 {
        self.0.frame_budget
    }

    /// The bytes of decoded frames that runs hold now.
    pub fn frame_bytes(&self) -> u64 {
        self.lock().frames
    }

    /// The most bytes of decoded frames that runs held at once.
    pub fn peak_frame_bytes(&self) -> u64 {
        self.lock().peak_frames
    }

    /// Runs that wait for frame bytes.
    pub fn frame_waiting_count(&self) -> usize {
        self.lock().frame_waiting.len()
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

    /// Places whose run ended (suspended, closed or failed) and whose
    /// threads have not yet handed them to the next waiter or freed them.
    pub fn in_transit(&self) -> usize {
        self.0.transit.load(Ordering::SeqCst)
    }

    /// Exists for tests only: the last thread of a run that ended waits
    /// `delay` before it gives the place back, as a slow thread would (the
    /// handover must not depend on it). On the process's slots
    /// ([`PipelineSlots::global`]) it does nothing, so that no product code
    /// can slow the places of all pages.
    pub fn set_handover_delay(&self, delay: Duration) {
        if *self == PipelineSlots::global() {
            return;
        }
        let ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
        self.0.handover_delay_ms.store(ms, Ordering::SeqCst);
    }

    /// Gives `waiter` (of the pipeline `owner`, whose owner `notify` wakes)
    /// a place now, or queues it until one is free.
    pub(crate) fn acquire(&self, owner: u64, notify: Notify, waiter: Waiter) {
        let slot = {
            let mut st = self.lock();
            if st.open < self.0.max {
                st.open += 1;
                st.peak_open = st.peak_open.max(st.open);
                Some(Slot::new(self.clone()))
            } else {
                st.waiting.push_back((owner, notify, waiter));
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
        let gone: VecDeque<(u64, Notify, Waiter)> = {
            let mut st = self.lock();
            let (gone, keep) = std::mem::take(&mut st.waiting)
                .into_iter()
                .partition(|(o, _, _)| *o == owner);
            st.waiting = keep;
            gone
        };
        drop(gone);
    }

    /// Removes the frame waiters of the pipeline `owner` (its run ended);
    /// the runs behind it may get their bytes now.
    pub(crate) fn forget_frames(&self, owner: u64) {
        let woken = {
            let mut st = self.lock();
            if !st.frame_waiting.iter().any(|w| w.owner == owner) {
                return;
            }
            st.frame_waiting.retain(|w| w.owner != owner);
            self.grant_frames(&mut st)
        };
        wake_granted(woken);
    }

    /// Gives frame bytes to the waiters in order while the first one's
    /// minimum fits (under the lock). Returns the wake-ups and notifies of
    /// the runs that got bytes, to call without the lock.
    fn grant_frames(&self, st: &mut State) -> Vec<(Arc<FrameWake>, Notify)> {
        let budget = self.0.frame_budget;
        let mut woken = Vec::new();
        while let Some(first) = st.frame_waiting.front() {
            let free = budget.saturating_sub(st.frames);
            // A waiter's minimum is within the budget (`reserve_frames`).
            if free < first.min {
                break;
            }
            let Some(w) = st.frame_waiting.pop_front() else {
                break;
            };
            let grant = w.want.min(free);
            // A run whose place is gone (detached) gets nothing.
            if w.place.upgrade().is_some_and(|p| p.add_frames(grant, true)) {
                st.add_frame_bytes(grant);
                woken.push((w.wake, w.notify));
            }
        }
        woken
    }

    /// Gives back the frame bytes of a place; the waiters may get them.
    fn release_frames(&self, bytes: u64) -> Vec<(Arc<FrameWake>, Notify)> {
        let mut st = self.lock();
        st.frames = st.frames.saturating_sub(bytes);
        self.grant_frames(&mut st)
    }

    /// Hands a freed place to the waiters in order, or frees it.
    fn release(&self) {
        loop {
            let waiter = {
                let mut st = self.lock();
                let Some((_, _, w)) = st.waiting.pop_front() else {
                    st.open = st.open.saturating_sub(1);
                    return;
                };
                w
            };
            match waiter(Slot::new(self.clone())) {
                None => return,
                // The waiter is gone: try the next one with the same place.
                Some(mut back) => back.armed = false,
            }
        }
    }

    /// The notifies of the waiters' owners (for a place and for frame
    /// bytes).
    fn waiter_notifies(&self) -> Vec<Notify> {
        let st = self.lock();
        st.waiting
            .iter()
            .map(|(_, notify, _)| Arc::clone(notify))
            .chain(st.frame_waiting.iter().map(|w| Arc::clone(&w.notify)))
            .collect()
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
    /// The place is in transit (counted in `Inner::transit`).
    transit: bool,
    /// The frame bytes that the run of the place holds.
    frames: u64,
}

impl Slot {
    fn new(slots: PipelineSlots) -> Slot {
        Slot {
            slots,
            armed: true,
            transit: false,
            frames: 0,
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // The waiters' owners that may wait for this place or its frame
        // bytes, taken before the place goes to the first of them.
        let notifies = if self.transit {
            self.slots.waiter_notifies()
        } else {
            Vec::new()
        };
        let woken = if self.frames > 0 {
            self.slots.release_frames(std::mem::take(&mut self.frames))
        } else {
            Vec::new()
        };
        if self.armed {
            self.armed = false;
            self.slots.release();
        }
        // The runs that got frame bytes no longer wait, before the transit
        // ends (an owner in virtual time then sees their pending work).
        for (wake, _) in &woken {
            wake();
        }
        if self.transit {
            // The transit ends after the next pipeline has its place.
            self.transit = false;
            self.slots.0.transit.fetch_sub(1, Ordering::SeqCst);
            for notify in notifies {
                notify();
            }
        }
        for (_, notify) in woken {
            notify();
        }
    }
}

/// Calls the wake-ups and then the notifies of runs that got frame bytes.
fn wake_granted(woken: Vec<(Arc<FrameWake>, Notify)>) {
    for (wake, _) in &woken {
        wake();
    }
    for (_, notify) in woken {
        notify();
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
    /// Frame bytes that the run got while it waited, until its demux thread
    /// takes the number ([`RunPlace::take_frame_grant`]).
    frame_grant: Option<u64>,
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
                frame_grant: None,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, PlaceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reserves between `min` and `want` bytes of decoded frames for the
    /// run: [`Reserve::Granted`] at once when no run waits before it and
    /// `min` fits; else [`Reserve::Waiting`], and the run waits in the FIFO:
    /// when bytes come, `wake` is called (without the slots' lock) and
    /// [`RunPlace::take_frame_grant`] gives the number. The bytes go back
    /// with the place. `owner` is the pipeline's id
    /// ([`PipelineSlots::forget_frames`]), `notify` its owner's.
    ///
    /// A `min` above the whole budget is [`Reserve::Refused`]: such a run
    /// could never play within the budget (with the process's budget and
    /// the frame size limit it does not happen: the minimum at the limit is
    /// 713 MB of 2 GiB; a test's small budget can make it).
    pub(crate) fn reserve_frames(
        self: &Arc<RunPlace>,
        owner: u64,
        min: u64,
        want: u64,
        notify: Notify,
        wake: FrameWake,
    ) -> Reserve {
        let budget = self.slots.0.frame_budget;
        if min > budget {
            return Reserve::Refused;
        }
        let mut st = self.slots.lock();
        let free = budget.saturating_sub(st.frames);
        if st.frame_waiting.is_empty() && free >= min {
            let grant = want.max(min).min(free);
            if self.add_frames(grant, false) {
                st.add_frame_bytes(grant);
            }
            return Reserve::Granted(grant);
        }
        st.frame_waiting.push_back(FrameWaiter {
            owner,
            place: Arc::downgrade(self),
            min,
            want: want.max(min),
            notify,
            wake: Arc::new(wake),
        });
        Reserve::Waiting
    }

    /// Makes the run's reservation `min` to `want` bytes when it holds
    /// fewer (a decoded frame is larger than the reservation's frame size;
    /// review of 10a): at once, without a wait in the FIFO (a run in the
    /// middle of its playback does not wait), from the bytes that are free
    /// now. Returns the reservation, or `None` when `min` does not fit (or
    /// the place is gone: the run was detached).
    pub(crate) fn grow_frames(&self, min: u64, want: u64) -> Option<u64> {
        let budget = self.slots.0.frame_budget;
        let mut st = self.slots.lock();
        let mut place = self.lock();
        let slot = place.slot.as_mut()?;
        let held = slot.frames;
        if held >= min {
            return Some(held);
        }
        let free = budget.saturating_sub(st.frames);
        let extra = want.max(min).saturating_sub(held).min(free);
        if held.saturating_add(extra) < min {
            return None;
        }
        slot.frames += extra;
        st.add_frame_bytes(extra);
        Some(slot.frames)
    }

    /// Adds `bytes` of frames to the place (`granted`: they came to a run
    /// that waits); false when the place is gone (the run was detached).
    fn add_frames(&self, bytes: u64, granted: bool) -> bool {
        let mut st = self.lock();
        let Some(slot) = st.slot.as_mut() else {
            return false;
        };
        slot.frames += bytes;
        if granted {
            st.frame_grant = Some(bytes);
        }
        true
    }

    /// The frame bytes that came to the run while it waited (once).
    pub(crate) fn take_frame_grant(&self) -> Option<u64> {
        self.lock().frame_grant.take()
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

    /// The run ended (the pipeline suspended, closed or failed): the place
    /// is in transit until its threads have ended and handed it on. One
    /// that is still in transit after `limit` is detached (the watchdog).
    /// Call it under the pipeline's lock, in the same critical section
    /// that ends the run, so that no owner sees the run ended before the
    /// place counts as in transit.
    pub(crate) fn end(self: &Arc<RunPlace>, limit: Duration) {
        if self.begin_transit() {
            super::watchdog::watch_place(self, Instant::now() + limit);
        }
    }

    /// Like [`RunPlace::end`], for a run that the caller detaches right
    /// after (the watchdog's own check): no watchdog entry, as the place
    /// does not stay in transit.
    pub(crate) fn end_before_detach(&self) {
        self.begin_transit();
    }

    /// Puts the place in transit; false if it is already (or has no slot).
    fn begin_transit(&self) -> bool {
        let mut st = self.lock();
        let Some(slot) = st.slot.as_mut() else {
            return false;
        };
        if slot.transit {
            return false;
        }
        slot.transit = true;
        self.slots.0.transit.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// The watchdog's check of a place that went into transit a limit ago:
    /// if it is still in transit, a thread of the run is stuck; the run is
    /// detached, so that the place goes on.
    pub(crate) fn check_transit(&self) {
        let stuck = self.lock().slot.as_ref().is_some_and(|s| s.transit);
        if stuck {
            log::warn!("media pipeline: a thread of a run that ended does not end; detached");
            self.detach();
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
        if slot.as_ref().is_some_and(|s| s.transit) {
            let delay = self.place.slots.0.handover_delay_ms.load(Ordering::SeqCst);
            if delay > 0 {
                std::thread::sleep(Duration::from_millis(delay));
            }
        }
        drop(slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn nop() -> Notify {
        Arc::new(|| {})
    }

    #[test]
    fn places_go_to_the_waiters_in_order() {
        let slots = PipelineSlots::new(2);
        let held: Arc<StdMutex<Vec<(usize, Slot)>>> = Arc::default();
        for i in 0..5 {
            let held = Arc::clone(&held);
            slots.acquire(
                i as u64,
                nop(),
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
            nop(),
            Box::new({
                let keep = Arc::clone(&keep);
                move |s| {
                    keep.lock().expect("lock").push(s);
                    None
                }
            }),
        );
        gone.acquire(1, nop(), Box::new(Some));
        gone.acquire(
            2,
            nop(),
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
        slots.acquire(1, nop(), take(&held));
        // Pipeline 2 waits, suspends and waits again, many times.
        for _ in 0..100 {
            slots.acquire(2, nop(), take(&held));
            slots.forget(2);
        }
        slots.acquire(3, nop(), take(&held));
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
            nop(),
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
            nop(),
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

    /// Review of 10a: a minimum above the whole budget is refused (it never
    /// fits); a reservation grows at once from the free bytes, or not at
    /// all when its new minimum does not fit, and goes back with the place.
    #[test]
    fn reservations_are_refused_above_the_budget_and_grow_from_free_bytes() {
        let slots = PipelineSlots::with_frame_budget(4, 100);
        let held: Arc<StdMutex<Vec<Slot>>> = Arc::default();
        let place = |id: u64| {
            let h = Arc::clone(&held);
            slots.acquire(
                id,
                nop(),
                Box::new(move |s| {
                    h.lock().expect("lock").push(s);
                    None
                }),
            );
            RunPlace::new(held.lock().expect("lock").remove(0))
        };
        let a = place(1);
        let refused = a.reserve_frames(1, 101, 200, nop(), Box::new(|| {}));
        assert_eq!(refused, Reserve::Refused);
        assert_eq!(slots.frame_waiting_count(), 0);
        let got = a.reserve_frames(1, 10, 30, nop(), Box::new(|| {}));
        assert_eq!(got, Reserve::Granted(30));
        let b = place(2);
        assert_eq!(
            b.reserve_frames(2, 40, 60, nop(), Box::new(|| {})),
            Reserve::Granted(60)
        );
        // a grows: 10 bytes are free, its new minimum needs 35 more.
        assert_eq!(a.grow_frames(65, 90), None);
        assert_eq!(slots.frame_bytes(), 90);
        // A minimum that fits: as much of the wish as is free.
        assert_eq!(a.grow_frames(35, 90), Some(40));
        // Already enough: nothing changes.
        assert_eq!(a.grow_frames(20, 90), Some(40));
        assert_eq!(slots.frame_bytes(), 100);
        assert_eq!(slots.peak_frame_bytes(), 100);
        drop(a);
        drop(b);
        assert_eq!(slots.frame_bytes(), 0);
    }

    /// A run that ended keeps its place in transit while its threads end
    /// (here: a thread that the test holds, as a slow one); the transit
    /// ends after the waiter has the place, and every waiter's owner is
    /// notified then.
    #[test]
    fn a_place_is_in_transit_until_the_waiter_has_it() {
        let slots = PipelineSlots::new(1);
        let held: Arc<StdMutex<Vec<(u64, Slot)>>> = Arc::default();
        let wakes = Arc::new(AtomicUsize::new(0));
        let waiter = |id: u64| -> Waiter {
            let held = Arc::clone(&held);
            Box::new(move |s| {
                held.lock().expect("lock").push((id, s));
                None
            })
        };
        // A woken owner reads the state at once: the transit must be over
        // by then, else it sees pending work and nothing wakes it again.
        let counting = || -> Notify {
            let wakes = Arc::clone(&wakes);
            let slots = slots.clone();
            Arc::new(move || {
                assert_eq!(slots.in_transit(), 0, "the transit ended before the wake");
                wakes.fetch_add(1, Ordering::SeqCst);
            })
        };
        slots.acquire(1, nop(), waiter(1));
        slots.acquire(2, counting(), waiter(2));
        slots.acquire(3, counting(), waiter(3));
        let (_, slot) = held.lock().expect("lock").remove(0);
        let place = RunPlace::new(slot);
        let thread = place.enter();
        assert_eq!(slots.in_transit(), 0);
        place.end(Duration::from_secs(3600));
        place.end(Duration::from_secs(3600));
        assert_eq!(slots.in_transit(), 1, "once");
        assert!(held.lock().expect("lock").is_empty(), "the thread holds it");
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
        drop(thread);
        assert_eq!(slots.in_transit(), 0);
        let got: Vec<u64> = held.lock().expect("lock").iter().map(|(i, _)| *i).collect();
        assert_eq!(got, [2]);
        // Both waiters' owners were woken (2 got the place, 3 waits on).
        assert_eq!(wakes.load(Ordering::SeqCst), 2);
        // A place that stays in transit is detached by its check.
        let (_, slot) = held.lock().expect("lock").remove(0);
        let place = RunPlace::new(slot);
        let thread = place.enter();
        place.end(Duration::from_secs(3600));
        place.check_transit();
        assert_eq!(slots.in_transit(), 0);
        assert_eq!(slots.detached_threads(), 1);
        let got: Vec<u64> = held.lock().expect("lock").iter().map(|(i, _)| *i).collect();
        assert_eq!(got, [3]);
        drop(thread);
        assert_eq!(slots.detached_threads(), 0);
    }
}
