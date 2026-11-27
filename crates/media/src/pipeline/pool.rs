//! The pool of frame images (ADR 0047 section 6): the video thread
//! converts each frame into a pixmap from the pool, and a frame that is no
//! longer shown gives its pixmap back. Steady playback allocates no pixmap
//! per frame.
//!
//! The images are `Arc<Pixmap>`: the owner may hand the current frame to
//! paint, which keeps a clone while it draws. The pool keeps a reference to
//! each pixmap of the run, so a pixmap that only the pool holds is free; the
//! video thread takes it out of the pool to convert into it (it must hold
//! the only reference) and gives it back ([`FramePool::track`]) before it
//! queues the frame.
//!
//! **The limit** (M14 feature 10a): the run's share of the frame budget
//! (`slots.rs`), in bytes; the pool allocates no pixmap that would take the
//! bytes of the run's pixmaps (in the queue, current, held by paint, free,
//! and the one being converted) over the limit. Bytes, not pixmaps: the
//! frame size can change in the stream (review of 10a).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tiny_skia::Pixmap;

/// The most free pixmaps that the pool keeps.
const MAX_FREE: usize = 8;

/// What [`FramePool::take`] gives.
pub(crate) enum Taken {
    /// A pixmap that nobody else holds.
    Pixmap(Arc<Pixmap>),
    /// The run holds its limit of pixmaps and none is free: the frame is
    /// not converted.
    AtLimit,
    /// The size cannot be allocated.
    Failed,
}

/// Pixmaps to reuse.
pub(crate) struct FramePool {
    state: Mutex<PoolState>,
    allocated: AtomicU64,
}

struct PoolState {
    /// The pixmaps of the run (free when the pool holds the only
    /// reference), except one that is taken out for a conversion.
    all: Vec<Arc<Pixmap>>,
    /// The bytes of a pixmap that is taken out and not yet given back (0:
    /// none).
    out: u64,
    /// The most bytes of the run's pixmaps at once.
    limit: u64,
}

/// The bytes of a pixmap of `width` x `height`.
fn bytes(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) * 4
}

impl PoolState {
    /// The bytes of the run's pixmaps now.
    fn live_bytes(&self) -> u64 {
        self.all
            .iter()
            .map(|p| bytes(p.width(), p.height()))
            .sum::<u64>()
            + self.out
    }
}

impl Default for FramePool {
    fn default() -> FramePool {
        FramePool {
            state: Mutex::new(PoolState {
                all: Vec::new(),
                out: 0,
                limit: u64::MAX,
            }),
            allocated: AtomicU64::new(0),
        }
    }
}

impl FramePool {
    fn lock(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A pixmap of `width` x `height` that nobody else holds, taken out of
    /// the pool: a free one of that size, else a new one while the run's
    /// pixmaps stay within its limit of bytes. Give it back with
    /// [`FramePool::track`].
    pub(crate) fn take(&self, width: u32, height: u32) -> Taken {
        let mut st = self.lock();
        // Pixmaps of another size are of no use any more.
        st.all
            .retain(|p| (p.width(), p.height()) == (width, height) || Arc::strong_count(p) > 1);
        if let Some(i) = st
            .all
            .iter()
            .position(|p| (p.width(), p.height()) == (width, height) && Arc::strong_count(p) == 1)
        {
            st.out = bytes(width, height);
            return Taken::Pixmap(st.all.swap_remove(i));
        }
        if st.live_bytes().saturating_add(bytes(width, height)) > st.limit {
            return Taken::AtLimit;
        }
        let Some(pixmap) = Pixmap::new(width, height) else {
            return Taken::Failed;
        };
        self.allocated.fetch_add(1, Ordering::Relaxed);
        st.out = bytes(width, height);
        Taken::Pixmap(Arc::new(pixmap))
    }

    /// Gives a pixmap of the run (or the current frame of an earlier run)
    /// back to the pool; it is free again once nobody else holds it.
    pub(crate) fn track(&self, pixmap: &Arc<Pixmap>) {
        let mut st = self.lock();
        st.out = 0;
        if !st.all.iter().any(|p| Arc::ptr_eq(p, pixmap)) {
            st.all.push(Arc::clone(pixmap));
        }
        // Keep at most `MAX_FREE` free ones.
        let mut free = st.all.iter().filter(|p| Arc::strong_count(p) == 1).count();
        st.all.retain(|p| {
            if free > MAX_FREE && Arc::strong_count(p) == 1 && !Arc::ptr_eq(p, pixmap) {
                free -= 1;
                false
            } else {
                true
            }
        });
    }

    /// Gives a pixmap back (a frame that is not shown any more): the pool
    /// keeps it, and the caller's reference goes.
    pub(crate) fn give(&self, pixmap: Arc<Pixmap>) {
        self.track(&pixmap);
        drop(pixmap);
    }

    /// Forgets the pixmaps of the run that ended (a suspended pipeline holds
    /// only its current frame, outside the pool); the next run sets its
    /// limit.
    pub(crate) fn clear(&self) {
        let mut st = self.lock();
        st.all.clear();
        st.out = 0;
        st.limit = u64::MAX;
    }

    /// Sets the most bytes of the run's pixmaps at once.
    pub(crate) fn set_limit(&self, limit: u64) {
        self.lock().limit = limit;
    }

    /// The pixmaps of the run now (with one taken out for a conversion).
    pub(crate) fn live(&self) -> usize {
        let st = self.lock();
        st.all.len() + usize::from(st.out > 0)
    }

    /// The bytes of the run's pixmaps now (with one taken out).
    pub(crate) fn live_bytes(&self) -> u64 {
        self.lock().live_bytes()
    }

    /// The pixmaps allocated since the pool was made.
    pub(crate) fn allocated(&self) -> u64 {
        self.allocated.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap(taken: Taken) -> Arc<Pixmap> {
        match taken {
            Taken::Pixmap(p) => p,
            Taken::AtLimit => panic!("at the limit"),
            Taken::Failed => panic!("failed"),
        }
    }

    /// Takes a pixmap as the video thread does: unique, then given back.
    fn convert(pool: &FramePool, w: u32, h: u32) -> Arc<Pixmap> {
        let mut p = pixmap(pool.take(w, h));
        assert!(Arc::get_mut(&mut p).is_some(), "the only reference");
        pool.track(&p);
        p
    }

    #[test]
    fn a_pixmap_is_reused_once_nobody_holds_it() {
        let pool = FramePool::default();
        let a = convert(&pool, 4, 2);
        let painted = Arc::clone(&a);
        pool.give(a);
        // Paint still holds it: a new one.
        let b = convert(&pool, 4, 2);
        assert_eq!(pool.allocated(), 2);
        drop(painted);
        let c = convert(&pool, 4, 2);
        assert_eq!(pool.allocated(), 2);
        pool.give(b);
        pool.give(c);
        // Another size drops the old ones.
        let _d = convert(&pool, 8, 8);
        assert_eq!(pool.allocated(), 3);
        assert_eq!(pool.live(), 1);
    }

    /// M14 feature 10a: the run's pixmaps stay within its limit; one that
    /// nobody else holds is reused.
    #[test]
    fn the_pool_allocates_no_more_than_its_limit() {
        let pool = FramePool::default();
        pool.set_limit(2 * bytes(4, 2));
        let a = convert(&pool, 4, 2);
        let b = convert(&pool, 4, 2);
        assert!(matches!(pool.take(4, 2), Taken::AtLimit));
        assert_eq!(pool.live(), 2);
        // A frame that paint still holds: still in use.
        let held = Arc::clone(&a);
        drop(a);
        assert!(matches!(pool.take(4, 2), Taken::AtLimit));
        drop(held);
        let a = convert(&pool, 4, 2);
        assert_eq!(pool.allocated(), 2);
        drop((a, b));
        // A new run has no limit until it sets one.
        pool.clear();
        assert_eq!(pool.live(), 0);
        let _d: Vec<_> = (0..4).map(|_| convert(&pool, 4, 2)).collect();
        assert_eq!(pool.allocated(), 6);
    }

    /// Review of 10a: the limit counts bytes, so frames that grow in the
    /// stream do not pass it (a limit of pixmaps let 16 x 16 frames reserve
    /// and 8192 x 4352 frames take the same number of pixmaps).
    #[test]
    fn the_limit_counts_bytes_when_the_frame_size_grows() {
        let pool = FramePool::default();
        pool.set_limit(3 * bytes(8, 8));
        let small = convert(&pool, 4, 4);
        let held = convert(&pool, 4, 4);
        pool.give(small);
        // The free small one goes (another size); the held one counts.
        let a = convert(&pool, 8, 8);
        assert_eq!(pool.live_bytes(), bytes(4, 4) + bytes(8, 8));
        let b = convert(&pool, 8, 8);
        // A third large one would make 3 large and 1 small.
        assert!(matches!(pool.take(8, 8), Taken::AtLimit));
        drop(held);
        let c = convert(&pool, 8, 8);
        assert_eq!(pool.live_bytes(), 3 * bytes(8, 8));
        assert!(matches!(pool.take(16, 16), Taken::AtLimit));
        drop((a, b, c));
    }
}
