//! The no-progress limit (ADR 0047 section 11): a pipeline whose owner
//! needs data and whose threads made no progress for the limit (10 s) fails
//! with a decode error, and its threads are detached.
//!
//! One thread per process watches the pipelines whose owners need data.
//! The owner registers its pipeline when it starts to need data
//! ([`watch`]); the watchdog sleeps on a condition variable until the
//! earliest check, so it uses no CPU while no pipeline needs data. At a
//! check, a pipeline that no longer needs data leaves the watch, one that
//! made progress is checked again at its last progress plus the limit, and
//! one that made none fails ([`Shared::check_progress`]). The failure calls
//! the owner's notify, so it works also when nothing else would wake the
//! owner (virtual time, where the page clock does not move while work is
//! pending).
//!
//! It also checks the place of each run that ended a limit after the end:
//! a place still in transit then (a thread of the run does not end) is
//! detached, so that the waiters, whose owners wait for it, go on.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::thread;
use std::time::Instant;

use super::shared::Shared;
use super::slots::RunPlace;

/// What to check.
enum Target {
    /// A pipeline whose owner needs data.
    Pipeline(Weak<Shared>),
    /// The place of a run that ended (it must leave its transit).
    Place(Weak<RunPlace>),
}

/// A pipeline or a place to check at `due`.
struct Entry {
    target: Target,
    due: Instant,
}

struct Watchdog {
    entries: Mutex<Vec<Entry>>,
    cv: Condvar,
}

impl Watchdog {
    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The process's watchdog; its thread starts at the first use.
fn watchdog() -> &'static Arc<Watchdog> {
    static WATCHDOG: OnceLock<Arc<Watchdog>> = OnceLock::new();
    WATCHDOG.get_or_init(|| {
        let w = Arc::new(Watchdog {
            entries: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        });
        let thread_w = Arc::clone(&w);
        if let Err(e) = thread::Builder::new()
            .name("swb-media-watchdog".to_owned())
            .spawn(move || run(&thread_w))
        {
            log::warn!("media pipeline: cannot start the watchdog thread: {e}");
        }
        w
    })
}

/// Checks `shared` at `due` (the owner started to need data).
pub(crate) fn watch(shared: &Arc<Shared>, due: Instant) {
    add(Entry {
        target: Target::Pipeline(Arc::downgrade(shared)),
        due,
    });
}

/// Checks at `due` that `place` (of a run that ended) left its transit.
pub(crate) fn watch_place(place: &Arc<RunPlace>, due: Instant) {
    add(Entry {
        target: Target::Place(Arc::downgrade(place)),
        due,
    });
}

fn add(entry: Entry) {
    let w = watchdog();
    w.lock().push(entry);
    w.cv.notify_one();
}

fn run(w: &Watchdog) {
    let mut entries = w.lock();
    loop {
        let now = Instant::now();
        let Some(next) = entries.iter().map(|e| e.due).min() else {
            entries = w.cv.wait(entries).unwrap_or_else(PoisonError::into_inner);
            continue;
        };
        if next > now {
            entries =
                w.cv.wait_timeout(entries, next - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            continue;
        }
        let (due, later): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut *entries)
            .into_iter()
            .partition(|e| e.due <= now);
        *entries = later;
        // The checks take the pipelines' locks and may call their notify:
        // not under the watchdog's lock.
        drop(entries);
        let again: Vec<Entry> = due
            .into_iter()
            .filter_map(|e| match e.target {
                Target::Pipeline(weak) => {
                    let shared = weak.upgrade()?;
                    let due = shared.check_progress(now)?;
                    Some(Entry {
                        target: Target::Pipeline(weak),
                        due,
                    })
                }
                Target::Place(weak) => {
                    weak.upgrade()?.check_transit();
                    None
                }
            })
            .collect();
        entries = w.lock();
        entries.extend(again);
    }
}
