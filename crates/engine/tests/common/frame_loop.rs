//! The GUI's frame loop without a window, for the media tests: `drive` runs
//! a page in real time as `about_to_wait` does and counts what it did.

// Test helpers unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use swb_engine::{Page, Pixmap};

/// What a drive of the loop did.
#[derive(Debug, Default)]
pub(crate) struct Run {
    /// Frames drawn.
    pub(crate) renders: usize,
    /// Different pictures among them.
    pub(crate) distinct: usize,
    /// Due times of the page that the loop woke up for (each time once).
    pub(crate) wakeups: usize,
    /// Iterations where a due time had passed and nothing changed: the loop
    /// would have run again at once (a busy loop).
    pub(crate) spins: usize,
    /// Frames drawn with their damage, in device pixels.
    pub(crate) pixels: Vec<u64>,
}

fn hash_of(data: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Drives the page for `duration` as `about_to_wait` does: the pump, the
/// frame decision, a render of the damage if the frame is outdated, then a
/// wait until the page's next due time (at most 5 ms: the test polls the
/// network where the window would be woken).
pub(crate) fn drive(page: &mut Page, target: &mut Pixmap, duration: Duration) -> Run {
    let end = Instant::now() + duration;
    let mut run = Run::default();
    let mut seen = HashSet::new();
    // The due time that the loop waits for; a wake-up is the moment it
    // passes.
    let mut armed: Option<Instant> = None;
    while Instant::now() < end {
        page.process_network();
        if armed.is_some_and(|t| t <= Instant::now()) {
            run.wakeups += 1;
            armed = None;
        }
        let pumped = page.pump();
        let outdated = page.frame_outdated();
        if outdated {
            let damage = page.render_update(target);
            run.renders += 1;
            run.pixels.push(damage.pixels());
            seen.insert(hash_of(target.data()));
        }
        let now = Instant::now();
        let next = page.next_task_instant();
        match next {
            Some(t) if t <= now => {
                // Due already: the loop would run again at once.
                if !pumped.changed && !outdated {
                    run.spins += 1;
                } else if armed.is_none() {
                    run.wakeups += 1;
                }
            }
            Some(t) => {
                armed = Some(t);
                std::thread::sleep((t - now).min(Duration::from_millis(5)));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    run.distinct = seen.len();
    run
}
