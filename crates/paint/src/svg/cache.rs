//! Renderings of SVG images at the sizes they are drawn at, with a memory
//! budget, and the rendering budget of one frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tiny_skia::{IntSize, Pixmap};

use super::{MAX_FRAME_WORK, SvgImage};

/// The memory budget of a cache: the bytes of all pixmaps.
const BUDGET_BYTES: usize = 128 * 1024 * 1024;

/// The maximum number of renderings in a cache.
const MAX_ENTRIES: usize = 1024;

/// The maximum number of renderings of one image that one frame uses;
/// other sizes reuse the closest one.
const MAX_PER_IMAGE: usize = 16;

/// Vector images rendered at the sizes they were drawn at, so that a
/// repaint (for example a scroll) does not render them again.
///
/// The cache never holds more than its memory budget and entry limit. To
/// make room, it drops the least recently used renderings that the current
/// frame has not used yet. If that is not enough (the frame uses them
/// all), or a rendering is larger than the whole budget, the new rendering
/// is drawn but not kept. A dropped rendering that the same frame needs
/// again is rendered again, within the frame's budget.
///
/// The cache is shared through `&self` (the rasterizer gets its images by
/// reference), so it locks a mutex.
#[derive(Debug)]
pub struct VectorCache {
    state: Mutex<State>,
    budget_bytes: usize,
    max_entries: usize,
}

impl Default for VectorCache {
    fn default() -> Self {
        Self::with_limits(BUDGET_BYTES, MAX_ENTRIES)
    }
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    /// Counts lookups, for the least recently used order.
    clock: u64,
    /// Counts frames.
    frame: u64,
}

/// An image, the pixel size and the concrete object size in CSS px (as
/// bits). The same pixel size can belong to different concrete sizes: an
/// image without `viewBox` is not scaled with the device pixel ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key {
    image: u64,
    width: u32,
    height: u32,
    concrete: (u32, u32),
}

#[derive(Debug)]
struct Entry {
    pixmap: Arc<Pixmap>,
    last_use: u64,
    /// The last frame that used the rendering.
    frame: u64,
}

/// What one frame (one `rasterize` call) may still render. A new rendering
/// whose estimated work does not fit in the rest of [`MAX_FRAME_WORK`] is
/// skipped: the image is drawn from the closest cached rendering of it
/// (scaled), or not at all, until a later repaint has the budget for it.
#[derive(Debug)]
pub(crate) struct FrameBudget {
    work: f64,
    frame: u64,
    skipped: bool,
}

impl FrameBudget {
    /// A budget for a frame without a cache.
    pub(crate) fn new() -> Self {
        FrameBudget {
            work: MAX_FRAME_WORK,
            frame: 0,
            skipped: false,
        }
    }

    /// True if the frame skipped a rendering.
    pub(crate) fn skipped(&self) -> bool {
        self.skipped
    }

    /// Takes `work` from the budget, or records a skipped rendering and
    /// returns false if the budget does not have it.
    pub(crate) fn take(&mut self, work: f64) -> bool {
        if work <= self.work {
            self.work -= work;
            true
        } else {
            self.skipped = true;
            false
        }
    }
}

impl VectorCache {
    fn with_limits(budget_bytes: usize, max_entries: usize) -> Self {
        VectorCache {
            state: Mutex::new(State::default()),
            budget_bytes,
            max_entries,
        }
    }

    /// Starts a frame.
    pub(crate) fn begin_frame(&self) -> FrameBudget {
        let mut state = self.lock();
        state.frame += 1;
        FrameBudget {
            frame: state.frame,
            ..FrameBudget::new()
        }
    }

    /// The rendering of `image` into `size` pixels for a concrete object
    /// size of `concrete` CSS px: from the cache, rendered now within the
    /// frame's budget, or else the closest cached rendering of the image.
    pub(crate) fn get(
        &self,
        image: &SvgImage,
        size: IntSize,
        concrete: (f32, f32),
        frame: &mut FrameBudget,
    ) -> Option<Arc<Pixmap>> {
        let key = Key {
            image: image.id(),
            width: size.width(),
            height: size.height(),
            concrete: (concrete.0.to_bits(), concrete.1.to_bits()),
        };
        {
            let mut state = self.lock();
            state.clock += 1;
            let now = state.clock;
            if let Some(entry) = state.entries.get_mut(&key) {
                entry.last_use = now;
                entry.frame = frame.frame;
                return Some(Arc::clone(&entry.pixmap));
            }
            let used = state
                .entries
                .iter()
                .filter(|(k, e)| k.image == key.image && e.frame == frame.frame)
                .count();
            if used >= MAX_PER_IMAGE {
                return state.closest(key, frame.frame);
            }
        }
        if !frame.take(image.render_work(size, concrete)) {
            return self.lock().closest(key, frame.frame);
        }
        // Render without the lock.
        let pixmap = Arc::new(image.render(size, concrete)?);
        self.insert(key, Arc::clone(&pixmap), frame.frame);
        Some(pixmap)
    }

    /// Keeps a rendering, dropping the least recently used ones that the
    /// current frame has not used. If that is not enough, the rendering is
    /// not kept.
    fn insert(&self, key: Key, pixmap: Arc<Pixmap>, frame: u64) {
        let bytes = pixmap.data().len();
        if bytes > self.budget_bytes {
            return;
        }
        let mut state = self.lock();
        if let Some(old) = state.entries.remove(&key) {
            state.bytes -= old.pixmap.data().len();
        }
        while state.bytes + bytes > self.budget_bytes || state.entries.len() >= self.max_entries {
            let Some(oldest) = state
                .entries
                .iter()
                .filter(|(_, entry)| entry.frame != frame)
                .min_by_key(|(_, entry)| entry.last_use)
                .map(|(key, _)| *key)
            else {
                return;
            };
            if let Some(entry) = state.entries.remove(&oldest) {
                state.bytes -= entry.pixmap.data().len();
            }
        }
        state.clock += 1;
        let last_use = state.clock;
        state.bytes += bytes;
        state.entries.insert(
            key,
            Entry {
                pixmap,
                last_use,
                frame,
            },
        );
    }

    /// The number of renderings and their bytes.
    #[cfg(test)]
    fn usage(&self) -> (usize, usize) {
        let state = self.lock();
        (state.entries.len(), state.bytes)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state stays consistent if a holder panicked: every update
        // completes before the guard is dropped.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// The cached rendering of the image of `key` whose size is closest to
    /// the key's: the smallest `|ln(w / w')| + |ln(h / h')|`, so the aspect
    /// ratio counts; ties go to the smallest key. It counts as used by
    /// `frame`.
    fn closest(&mut self, key: Key, frame: u64) -> Option<Arc<Pixmap>> {
        let axis = |a: u32, b: u32| (f64::from(a).ln() - f64::from(b).ln()).abs();
        let distance = |k: &Key| axis(k.width, key.width) + axis(k.height, key.height);
        let (_, entry) = self
            .entries
            .iter_mut()
            .filter(|(k, _)| k.image == key.image)
            .min_by(|(a, _), (b, _)| distance(a).total_cmp(&distance(b)).then(a.cmp(b)))?;
        entry.last_use = self.clock;
        entry.frame = frame;
        Some(Arc::clone(&entry.pixmap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> SvgImage {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">
            <rect width="1" height="1" fill="red"/></svg>"#;
        super::super::decode(svg).unwrap()
    }

    fn size(w: u32, h: u32) -> IntSize {
        IntSize::from_wh(w, h).unwrap()
    }

    #[test]
    fn renders_once_per_size() {
        let cache = VectorCache::default();
        let image = image();
        let mut frame = cache.begin_frame();
        let a = cache
            .get(&image, size(10, 10), (10.0, 10.0), &mut frame)
            .unwrap();
        let b = cache
            .get(&image, size(10, 10), (10.0, 10.0), &mut frame)
            .unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = cache
            .get(&image, size(20, 20), (10.0, 10.0), &mut frame)
            .unwrap();
        assert_eq!((c.width(), c.height()), (20, 20));
        assert_eq!(cache.usage(), (2, (100 + 400) * 4));
        assert!(!frame.skipped());
    }

    #[test]
    fn drops_the_least_recently_used_renderings() {
        // Room for two 10x10 renderings.
        let cache = VectorCache::with_limits(800, 100);
        let image = image();
        let get = |concrete: f32| {
            let mut frame = cache.begin_frame();
            cache.get(&image, size(10, 10), (concrete, concrete), &mut frame)
        };
        let first = get(1.0).unwrap();
        get(2.0);
        // Use the first again, so the second is the oldest.
        get(1.0);
        get(3.0);
        assert_eq!(cache.usage(), (2, 800));
        let again = get(1.0).unwrap();
        assert!(
            Arc::ptr_eq(&first, &again),
            "the recently used rendering stays"
        );
        // Too large for the budget: rendered, not kept.
        let mut frame = cache.begin_frame();
        let large = cache
            .get(&image, size(20, 20), (1.0, 1.0), &mut frame)
            .unwrap();
        assert_eq!(large.width(), 20);
        assert_eq!(cache.usage(), (2, 800));
    }

    #[test]
    fn limits_the_number_of_renderings() {
        let cache = VectorCache::with_limits(usize::MAX, 3);
        let image = image();
        for i in 0..10 {
            let mut frame = cache.begin_frame();
            cache.get(&image, size(1, 1), (i as f32, 1.0), &mut frame);
        }
        assert_eq!(cache.usage().0, 3);
    }

    #[test]
    fn one_frame_renders_few_sizes_of_one_image() {
        // 200 widths of one image in one frame: at most MAX_PER_IMAGE
        // renderings; the others reuse the closest.
        let cache = VectorCache::default();
        let image = image();
        let mut frame = cache.begin_frame();
        for width in 1..=200 {
            let pixmap = cache.get(&image, size(width, 10), (width as f32, 10.0), &mut frame);
            assert!(pixmap.is_some());
        }
        assert_eq!(cache.usage().0, MAX_PER_IMAGE);
        // The next frame renders sizes it has not used yet.
        let mut next = cache.begin_frame();
        cache.get(&image, size(300, 10), (300.0, 10.0), &mut next);
        assert_eq!(cache.usage().0, MAX_PER_IMAGE + 1);
    }

    #[test]
    fn the_frame_budget_stops_rendering() {
        let cache = VectorCache::default();
        let image = image();
        let mut frame = cache.begin_frame();
        cache.get(&image, size(10, 10), (10.0, 10.0), &mut frame);
        assert!(!frame.skipped());
        frame.work = 0.0;
        // Out of budget: the closest rendering, and the frame records the skip.
        let closest = cache
            .get(&image, size(40, 40), (40.0, 40.0), &mut frame)
            .unwrap();
        assert_eq!(closest.width(), 10);
        assert!(frame.skipped());
        assert_eq!(cache.usage().0, 1);
    }
}
