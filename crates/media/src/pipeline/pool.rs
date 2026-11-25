//! The pool of frame images (ADR 0047 section 6): the video thread
//! converts each frame into a pixmap from the pool, and a frame that is no
//! longer shown gives its pixmap back. Steady playback allocates no pixmap
//! per frame.
//!
//! The images are `Arc<Pixmap>`: the owner may hand the current frame to
//! paint, which keeps a clone while it draws. A pixmap is reused only when
//! the pool holds the only reference.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tiny_skia::Pixmap;

/// The most free pixmaps that the pool keeps.
const MAX_FREE: usize = 8;

/// Pixmaps to reuse.
#[derive(Default)]
pub(crate) struct FramePool {
    free: Mutex<Vec<Arc<Pixmap>>>,
    allocated: AtomicU64,
}

impl FramePool {
    /// A pixmap of `width` x `height` that nobody else holds: a free one of
    /// that size, else a new one (`None` if the size cannot be allocated).
    pub(crate) fn take(&self, width: u32, height: u32) -> Option<Arc<Pixmap>> {
        {
            let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
            // Pixmaps of another size are of no use any more.
            free.retain(|p| (p.width(), p.height()) == (width, height));
            if let Some(i) = free.iter().position(|p| Arc::strong_count(p) == 1) {
                return Some(free.swap_remove(i));
            }
        }
        let pixmap = Pixmap::new(width, height)?;
        self.allocated.fetch_add(1, Ordering::Relaxed);
        Some(Arc::new(pixmap))
    }

    /// Gives a pixmap back.
    pub(crate) fn give(&self, pixmap: Arc<Pixmap>) {
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        if free.len() >= MAX_FREE {
            free.remove(0);
        }
        free.push(pixmap);
    }

    /// Drops the free pixmaps (a suspended pipeline holds none).
    pub(crate) fn clear(&self) {
        self.free
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// The pixmaps allocated since the pool was made.
    pub(crate) fn allocated(&self) -> u64 {
        self.allocated.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixmap_is_reused_once_nobody_holds_it() {
        let pool = FramePool::default();
        let a = pool.take(4, 2).expect("a pixmap");
        let painted = Arc::clone(&a);
        pool.give(a);
        // Paint still holds it: a new one.
        let b = pool.take(4, 2).expect("a pixmap");
        assert_eq!(pool.allocated(), 2);
        drop(painted);
        let c = pool.take(4, 2).expect("a pixmap");
        assert_eq!(pool.allocated(), 2);
        pool.give(b);
        pool.give(c);
        // Another size drops the old ones.
        let _d = pool.take(8, 8).expect("a pixmap");
        assert_eq!(pool.allocated(), 3);
        assert!(pool.free.lock().expect("lock").is_empty());
    }
}
