//! Pixels that a raster keeps for the next one (ADR 0044 section 19): the
//! coverage of clip paths and the content of composited layers. An entry is
//! used only when every input of its pixels is equal (its key), so a raster
//! with the cache draws the pixels that it draws without it, bit for bit.
//!
//! - A key holds every input of the pixels: the items (compared by value),
//!   the media keys of their images, the matrix to the pixmap and its size.
//! - The budgets of a raster decide what it draws. An entry is kept only if
//!   no budget left anything out while it was made, and it records what it
//!   charged ([`Charges`]); a hit charges the same amounts, and is used only
//!   if they fit into what is left, so the later items of the raster meet
//!   the budgets that they meet without the cache.
//! - The cache has a memory limit and an entry limit, and drops the least
//!   recently used entries to make room. A coverage is kept only when its
//!   key missed before (in one of the last [`RECENT_MISSES`] misses), so a
//!   clip that moves every frame keeps nothing; a composited layer is kept
//!   at once, in place of the entry of its *slot* (the same area and
//!   scale), so content that changes every frame keeps one entry. The
//!   owner drops the cache with the document and on a change of the
//!   viewport or the scale.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use swb_layout::svg::ClipPath;
use swb_layout::{Matrix, Rect};
use tiny_skia::{IntSize, Mask, Pixmap};

use super::{Rasterizer, Skipped};
use crate::display_list::DisplayItem;

/// The memory limit of a cache (the pixels of all entries): 48 MiB.
const LIMIT_BYTES: usize = 48 * 1024 * 1024;

/// The most entries of a cache.
const MAX_ENTRIES: usize = 256;

/// The number of recent misses of coverages that the cache remembers (by a
/// fingerprint of the key): a coverage is kept on its second miss.
const RECENT_MISSES: usize = 64;

/// The most composited layers whose misses in a row the cache counts.
const MAX_STREAKS: usize = 64;

/// What making an entry charged to the budgets of the raster.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Charges {
    /// The most layer pixels that were open above those at the start.
    pub(super) layer_peak: u64,
    /// The mask work.
    pub(super) mask: u64,
    /// The filter work.
    pub(super) filter: u64,
    /// The pixels of transform layers.
    pub(super) transform: u64,
    /// The items of frames.
    pub(super) frame_items: u64,
    /// The charges of path work, in order (so that the sum is the same
    /// floating-point sum as when they were made).
    pub(super) path_work: Vec<f64>,
}

/// The inputs of the coverage of a clip path (`raster/svg_clip.rs`).
#[derive(Clone, Debug)]
pub(super) struct CoverageKey {
    /// The clip path.
    pub(super) clip: Arc<ClipPath>,
    /// The matrix from the clip path's coordinates to layer px (bits).
    pub(super) matrix: [u32; 6],
    /// The layer size in px.
    pub(super) size: (u32, u32),
}

impl CoverageKey {
    /// The key of `clip` drawn with `matrix` into a layer of `size`.
    pub(super) fn new(clip: &Arc<ClipPath>, matrix: &Matrix, size: (u32, u32)) -> Self {
        CoverageKey {
            clip: Arc::clone(clip),
            matrix: matrix_bits(matrix),
            size,
        }
    }

    /// A number that equal keys share (and few others do).
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.matrix.hash(&mut h);
        self.size.hash(&mut h);
        self.clip.shapes.len().hash(&mut h);
        self.clip.outer.is_some().hash(&mut h);
        for shape in &self.clip.shapes {
            let b = shape.path.bounds();
            [b.x, b.y, b.width, b.height].map(f32::to_bits).hash(&mut h);
            matrix_bits(&shape.transform).hash(&mut h);
        }
        h.finish()
    }

    fn matches(&self, other: &CoverageKey) -> bool {
        self.matrix == other.matrix
            && self.size == other.size
            && (Arc::ptr_eq(&self.clip, &other.clip) || *self.clip == *other.clip)
    }
}

/// The inputs of the content of a composited layer (`raster/composited.rs`)
/// other than its items.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayerParts {
    /// The media keys of the items (images, frames and masks), with the
    /// index of their item.
    pub(super) media: Vec<(usize, [u64; 2])>,
    /// The area of the group that the layer covers (bits).
    pub(super) area: [u32; 4],
    /// Layer px per CSS px (bits).
    pub(super) scale: u32,
    /// The viewport scroll offset and the origin of fixed content (bits),
    /// if the content has fixed or sticky boxes; else zero.
    pub(super) viewport: [u32; 4],
    /// The nesting of transform layers and frames around the layer.
    pub(super) nesting: (usize, u32, u32),
}

/// The inputs of the content of a composited layer.
#[derive(Clone, Debug)]
pub(super) struct LayerKey {
    /// The items of the content.
    pub(super) items: Arc<[DisplayItem]>,
    /// The other inputs.
    pub(super) parts: LayerParts,
}

impl LayerKey {
    fn slot(&self) -> Slot {
        Slot(self.parts.area, self.parts.scale, self.parts.nesting)
    }

    fn matches(&self, items: &[DisplayItem], parts: &LayerParts) -> bool {
        self.parts == *parts && *self.items == *items
    }
}

/// The place and size of a composited layer: a new layer replaces the
/// layer of its slot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot([u32; 4], u32, (usize, u32, u32));

/// The coverage of a clip path for a whole layer.
#[derive(Debug)]
pub(super) struct Coverage {
    /// One value per layer pixel.
    pub(super) mask: Mask,
    /// True if every value is 0: the clip hides the group.
    pub(super) hidden: bool,
}

impl Coverage {
    /// The coverage of `values` (one per pixel, row by row) for a layer of
    /// `size`; `None` if the counts differ.
    pub(super) fn new(values: Vec<u8>, size: IntSize) -> Option<Coverage> {
        let hidden = values.iter().all(|&v| v == 0);
        Some(Coverage {
            mask: Mask::from_vec(values, size)?,
            hidden,
        })
    }
}

/// The pixels of an entry.
#[derive(Clone, Debug)]
pub(super) enum Kept {
    /// The coverage of a clip path.
    Coverage(Arc<Coverage>),
    /// The content of a composited layer.
    Layer(Arc<Pixmap>),
}

impl Kept {
    fn bytes(&self) -> usize {
        match self {
            Kept::Coverage(coverage) => coverage.mask.data().len(),
            Kept::Layer(pixmap) => pixmap.data().len(),
        }
    }
}

#[derive(Debug)]
enum Key {
    Coverage(CoverageKey),
    Layer(LayerKey),
}

impl Key {
    /// The bytes that the key holds besides its shared values: the copy of
    /// a layer's items and its media keys.
    fn bytes(&self) -> usize {
        match self {
            Key::Coverage(_) => 0,
            Key::Layer(k) => {
                k.items.len() * size_of::<DisplayItem>()
                    + k.parts.media.len() * size_of::<(usize, [u64; 2])>()
            }
        }
    }

    fn slot(&self) -> Option<Slot> {
        match self {
            Key::Coverage(_) => None,
            Key::Layer(k) => Some(k.slot()),
        }
    }
}

#[derive(Debug)]
struct Entry {
    key: Key,
    kept: Kept,
    charges: Charges,
    last_use: u64,
    /// The bytes that count against the limit: the kept pixels, the key
    /// and the charges.
    bytes: usize,
}

#[derive(Debug, Default)]
struct State {
    entries: Vec<Entry>,
    /// The composited layers whose content missed in the last rasters that
    /// drew them, with the number of misses in a row (at most
    /// [`MAX_STREAKS`] slots).
    streaks: Vec<(Slot, u32)>,
    /// The fingerprints of the last coverage misses, oldest first.
    misses: std::collections::VecDeque<u64>,
    bytes: usize,
    clock: u64,
    stats: CacheStats,
}

/// Counts of a [`RasterCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// The bytes of the entries now.
    pub bytes: usize,
    /// The most bytes that the entries held at one time.
    pub peak_bytes: usize,
    /// The entries now.
    pub entries: usize,
    /// Lookups that found an entry.
    pub hits: u64,
    /// Lookups that found none.
    pub misses: u64,
}

/// Pixels of rasters kept for the next raster of the same document (or of
/// the documents of one page): see the module documentation. Shared through
/// `&self` (the rasterizer gets it from its [`super::ImageSource`]), so it
/// locks a mutex.
#[derive(Debug)]
pub struct RasterCache {
    state: Mutex<State>,
    /// False while the cache is switched off (the oracles draw frames
    /// without it): it finds nothing and keeps nothing.
    enabled: std::sync::atomic::AtomicBool,
    /// The pixmap of the last raster of parts (`rasterize_region`), for the
    /// next one of the same size: its pixels outside the parts do not
    /// matter, so it need not be cleared.
    scratch: Mutex<Option<Pixmap>>,
    limit: usize,
    max_entries: usize,
}

impl Default for RasterCache {
    fn default() -> Self {
        RasterCache::with_limits(LIMIT_BYTES, MAX_ENTRIES)
    }
}

impl RasterCache {
    /// A cache that holds at most `limit` bytes in at most `max_entries`
    /// entries.
    pub fn with_limits(limit: usize, max_entries: usize) -> Self {
        RasterCache {
            state: Mutex::new(State::default()),
            enabled: std::sync::atomic::AtomicBool::new(true),
            scratch: Mutex::new(None),
            limit,
            max_entries,
        }
    }

    /// A pixmap of `width` x `height` for a raster of parts, with any pixels:
    /// the one that [`RasterCache::put_scratch`] returned, if it has that
    /// size.
    pub(super) fn take_scratch(&self, width: u32, height: u32) -> Option<Pixmap> {
        let mut scratch = self.scratch.lock().unwrap_or_else(PoisonError::into_inner);
        match scratch.take() {
            Some(p) if p.width() == width && p.height() == height => Some(p),
            _ => Pixmap::new(width, height),
        }
    }

    /// Keeps the pixmap of a raster of parts for the next one.
    pub(super) fn put_scratch(&self, pixmap: Pixmap) {
        *self.scratch.lock().unwrap_or_else(PoisonError::into_inner) = Some(pixmap);
    }

    /// Switches the cache on or off: while it is off, it finds nothing and
    /// keeps nothing (a raster draws as without a cache; for the oracle
    /// that compares the two). Its entries stay.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Drops every entry (a new viewport or scale: no entry can match).
    pub fn clear(&self) {
        *self.scratch.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let mut state = self.lock();
        state.entries.clear();
        state.misses.clear();
        state.streaks.clear();
        state.bytes = 0;
        state.stats.bytes = 0;
        state.stats.entries = 0;
    }

    /// The counts of the cache.
    pub fn stats(&self) -> CacheStats {
        self.lock().stats
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The coverage kept for `key`, with its charges.
    pub(super) fn coverage(&self, key: &CoverageKey) -> Option<(Arc<Coverage>, Charges)> {
        let found = self
            .find(|k| matches!(k, Key::Coverage(c) if c.matches(key)))
            .and_then(|(kept, charges)| match kept {
                Kept::Coverage(values) => Some((values, charges)),
                Kept::Layer(_) => None,
            });
        if found.is_none() && self.is_enabled() {
            let mut state = self.lock();
            if state.misses.len() == RECENT_MISSES {
                state.misses.pop_front();
            }
            state.misses.push_back(key.fingerprint());
        }
        found
    }

    /// The layer content kept for the content `items` and `parts`, with its
    /// charges. Also counts the misses in a row of the layer's slot
    /// ([`RasterCache::layer_misses`]).
    pub(super) fn layer(
        &self,
        items: &[DisplayItem],
        parts: &LayerParts,
    ) -> Option<(Arc<Pixmap>, Charges)> {
        let found = self
            .find(|k| matches!(k, Key::Layer(l) if l.matches(items, parts)))
            .and_then(|(kept, charges)| match kept {
                Kept::Layer(pixmap) => Some((pixmap, charges)),
                Kept::Coverage(_) => None,
            });
        if !self.is_enabled() {
            return None;
        }
        let slot = Slot(parts.area, parts.scale, parts.nesting);
        let mut state = self.lock();
        let at = state.streaks.iter().position(|(s, _)| *s == slot);
        match (found.is_some(), at) {
            (true, Some(at)) => {
                state.streaks.swap_remove(at);
            }
            (true, None) => {}
            (false, Some(at)) => {
                if let Some((_, count)) = state.streaks.get_mut(at) {
                    *count = count.saturating_add(1);
                }
            }
            (false, None) => {
                if state.streaks.len() == MAX_STREAKS {
                    state.streaks.remove(0);
                }
                state.streaks.push((slot, 1));
            }
        }
        found
    }

    /// The misses in a row of the layer of `parts`' slot (the last lookup
    /// included): content that changes in every raster.
    pub(super) fn layer_misses(&self, parts: &LayerParts) -> u32 {
        let slot = Slot(parts.area, parts.scale, parts.nesting);
        self.lock()
            .streaks
            .iter()
            .find(|(s, _)| *s == slot)
            .map_or(0, |(_, count)| *count)
    }

    /// Keeps the coverage of `key` if it missed before (its last miss is
    /// not the only one among the recent misses).
    pub(super) fn keep_coverage(
        &self,
        key: CoverageKey,
        coverage: Arc<Coverage>,
        charges: Charges,
    ) {
        let fingerprint = key.fingerprint();
        let seen = self
            .lock()
            .misses
            .iter()
            .filter(|&&m| m == fingerprint)
            .count();
        if seen >= 2 {
            self.keep(Key::Coverage(key), Kept::Coverage(coverage), charges);
        }
    }

    /// Keeps the layer content of `key`.
    pub(super) fn keep_layer(&self, key: LayerKey, pixmap: Arc<Pixmap>, charges: Charges) {
        self.keep(Key::Layer(key), Kept::Layer(pixmap), charges);
    }

    fn find(&self, matches: impl Fn(&Key) -> bool) -> Option<(Kept, Charges)> {
        if !self.is_enabled() {
            return None;
        }
        let mut state = self.lock();
        state.clock += 1;
        let now = state.clock;
        let found = state.entries.iter_mut().find(|e| matches(&e.key)).map(|e| {
            e.last_use = now;
            (e.kept.clone(), e.charges.clone())
        });
        if found.is_some() {
            state.stats.hits += 1;
        } else {
            state.stats.misses += 1;
        }
        found
    }

    /// Keeps an entry in place of the entry of its slot, dropping the least
    /// recently used entries to make room. An entry larger than half of the
    /// limit (pixels, key and charges) is not kept.
    fn keep(&self, key: Key, kept: Kept, charges: Charges) {
        let bytes = kept.bytes() + key.bytes() + charges.path_work.len() * size_of::<f64>();
        if bytes > self.limit / 2 || !self.is_enabled() {
            return;
        }
        let mut state = self.lock();
        if let Some(slot) = key.slot()
            && let Some(at) = state
                .entries
                .iter()
                .position(|e| e.key.slot() == Some(slot))
        {
            let old = state.entries.swap_remove(at);
            state.bytes -= old.bytes;
        }
        while state.bytes + bytes > self.limit || state.entries.len() >= self.max_entries {
            let Some(oldest) = state
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_use)
                .map(|(i, _)| i)
            else {
                break;
            };
            let old = state.entries.swap_remove(oldest);
            state.bytes -= old.bytes;
        }
        state.clock += 1;
        let last_use = state.clock;
        state.entries.push(Entry {
            key,
            kept,
            charges,
            last_use,
            bytes,
        });
        state.bytes += bytes;
        state.stats.bytes = state.bytes;
        state.stats.peak_bytes = state.stats.peak_bytes.max(state.bytes);
        state.stats.entries = state.entries.len();
    }
}

/// The state of the budgets when an entry starts to be made
/// ([`Rasterizer::mark_charges`]).
pub(super) struct Mark {
    layer_pixels: u64,
    layer_high: u64,
    mask: u64,
    filter: u64,
    transform: u64,
    frame_items: u64,
    path_log: Option<Vec<f64>>,
    /// The flags of the budgets before the mark; they are reset while the
    /// entry is made, so that a skip inside it shows even when an earlier
    /// item set the same flag.
    skipped: Skipped,
    vectors: (f64, u64, u32),
}

impl Rasterizer<'_> {
    /// Starts to record what the making of an entry charges.
    pub(super) fn mark_charges(&mut self) -> Mark {
        let b = &mut self.budget;
        let mark = Mark {
            layer_pixels: b.layer_pixels,
            layer_high: b.layer_high,
            mask: b.mask_pixels,
            filter: b.filter_pixels,
            transform: b.transform_pixels,
            frame_items: b.frame_items,
            path_log: b.path_log.replace(Vec::new()),
            skipped: std::mem::take(&mut b.skipped),
            vectors: (
                self.vectors.left().0,
                self.vectors.left().1,
                self.vectors.inexact(),
            ),
        };
        b.layer_high = b.layer_pixels;
        mark
    }

    /// Ends the record that `mark` started: the charges since then, or
    /// `None` if a budget left something out or an SVG image was rendered
    /// or an image level reduced (the entry is then not kept).
    pub(super) fn end_charges(&mut self, mark: Mark) -> Option<Charges> {
        let b = &mut self.budget;
        let log = b.path_log.take().unwrap_or_default();
        let charges = Charges {
            layer_peak: b.layer_high.saturating_sub(mark.layer_pixels),
            mask: b.mask_pixels.saturating_sub(mark.mask),
            filter: b.filter_pixels.saturating_sub(mark.filter),
            transform: b.transform_pixels.saturating_sub(mark.transform),
            frame_items: b.frame_items.saturating_sub(mark.frame_items),
            path_work: log.clone(),
        };
        b.layer_high = b.layer_high.max(mark.layer_high);
        b.path_log = mark.path_log.map(|mut outer| {
            outer.extend(log);
            outer
        });
        let vectors = (
            self.vectors.left().0,
            self.vectors.left().1,
            self.vectors.inexact(),
        );
        let inside = std::mem::take(&mut self.budget.skipped);
        let complete = !inside.any() && vectors == mark.vectors;
        self.budget.skipped = mark.skipped;
        self.budget.skipped.merge(inside);
        complete.then_some(charges)
    }

    /// Charges what an entry charged when it was made, if all of it fits
    /// into what the budgets have left: then a raster without the entry
    /// would also have drawn everything, with the same charges. Returns
    /// false (and charges nothing) if it does not fit.
    pub(super) fn replay_charges(&mut self, charges: &Charges) -> bool {
        let b = &self.budget;
        let fits = |used: u64, add: u64, max: u64| used.saturating_add(add) <= max;
        let mut path = b.path_work;
        let mut path_fits = true;
        for &work in &charges.path_work {
            let total = path + work;
            if total.is_nan() || total > b.max_path_work {
                path_fits = false;
                break;
            }
            path = total;
        }
        let all = path_fits
            && fits(b.layer_pixels, charges.layer_peak, b.max_layer_pixels)
            && fits(b.mask_pixels, charges.mask, b.max_mask_pixels)
            && fits(b.filter_pixels, charges.filter, b.max_filter_pixels)
            && fits(
                b.transform_pixels,
                charges.transform,
                b.max_transform_pixels,
            )
            && fits(
                b.frame_items,
                charges.frame_items,
                super::frame::MAX_FRAME_ITEMS,
            );
        if !all {
            return false;
        }
        let b = &mut self.budget;
        b.layer_high = b.layer_high.max(b.layer_pixels + charges.layer_peak);
        b.mask_pixels += charges.mask;
        b.filter_pixels += charges.filter;
        b.transform_pixels += charges.transform;
        b.frame_items += charges.frame_items;
        b.path_work = path;
        if let Some(log) = &mut b.path_log {
            log.extend_from_slice(&charges.path_work);
        }
        true
    }
}

/// The bits of the six numbers of `m`.
pub(super) fn matrix_bits(m: &Matrix) -> [u32; 6] {
    [m.a, m.b, m.c, m.d, m.e, m.f].map(f32::to_bits)
}

/// The bits of the four numbers of `r`.
pub(super) fn rect_bits(r: &Rect) -> [u32; 4] {
    [r.x, r.y, r.width, r.height].map(f32::to_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(n: u32, v: u8) -> Arc<Coverage> {
        let size = IntSize::from_wh(n, 1).expect("a size");
        Arc::new(Coverage::new(vec![v; n as usize], size).expect("a coverage"))
    }

    fn clip() -> Arc<ClipPath> {
        Arc::new(ClipPath {
            shapes: Vec::new(),
            outer: None,
        })
    }

    /// Keeps the coverage of `key` (after the miss that it needs first).
    fn keep(cache: &RasterCache, key: &CoverageKey, kept: Arc<Coverage>, charges: Charges) {
        assert!(cache.coverage(key).is_none());
        cache.keep_coverage(key.clone(), kept, charges);
    }

    #[test]
    fn a_coverage_is_found_by_its_whole_key() {
        let cache = RasterCache::default();
        let key = CoverageKey::new(&clip(), &Matrix::translate(1.0, 2.0), (4, 3));
        let charges = Charges {
            path_work: vec![1.5, 2.0],
            ..Charges::default()
        };
        // The first miss keeps nothing.
        keep(&cache, &key, values(12, 7), charges.clone());
        assert_eq!(cache.stats().entries, 0);
        keep(&cache, &key, values(12, 7), charges.clone());
        assert_eq!(cache.stats().entries, 1);
        // Another `Arc` with an equal clip path hits.
        let equal = CoverageKey::new(&clip(), &Matrix::translate(1.0, 2.0), (4, 3));
        let (kept, got) = cache.coverage(&equal).expect("a hit");
        assert_eq!(
            (kept.mask.data().len(), kept.hidden, got),
            (12, false, charges)
        );
        // Each part of the key misses.
        let moved = CoverageKey::new(&clip(), &Matrix::translate(1.5, 2.0), (4, 3));
        let larger = CoverageKey::new(&clip(), &Matrix::translate(1.0, 2.0), (4, 4));
        let other = Arc::new(ClipPath {
            shapes: Vec::new(),
            outer: Some(swb_layout::svg::ClipRegion::Rect(Rect::new(
                0.0, 0.0, 1.0, 1.0,
            ))),
        });
        let other = CoverageKey::new(&other, &Matrix::translate(1.0, 2.0), (4, 3));
        for key in [moved, larger, other] {
            assert!(cache.coverage(&key).is_none(), "{key:?}");
        }
        let stats = cache.stats();
        assert_eq!((stats.hits, stats.misses, stats.entries), (1, 5, 1));
    }

    #[test]
    fn a_coverage_that_changes_every_time_is_not_kept() {
        let cache = RasterCache::default();
        for x in 0..100 {
            let key = CoverageKey::new(&clip(), &Matrix::translate(x as f32 * 0.25, 0.0), (8, 8));
            keep(&cache, &key, values(64, 1), Charges::default());
        }
        assert_eq!(cache.stats().peak_bytes, 0);
    }

    #[test]
    fn the_least_recently_used_entries_make_room() {
        let cache = RasterCache::with_limits(100, 3);
        let key = |x: f32| CoverageKey::new(&clip(), &Matrix::translate(x, 0.0), (5, 5));
        for x in 0..3 {
            for _ in 0..2 {
                keep(&cache, &key(x as f32), values(25, 0), Charges::default());
            }
        }
        // Use the first: the second is now the oldest.
        assert!(cache.coverage(&key(0.0)).is_some());
        for _ in 0..2 {
            keep(&cache, &key(3.0), values(25, 0), Charges::default());
        }
        assert!(cache.coverage(&key(1.0)).is_none());
        assert!(cache.coverage(&key(0.0)).is_some());
        // 40 bytes: more than the limit allows with three entries left.
        for _ in 0..2 {
            keep(&cache, &key(4.0), values(40, 0), Charges::default());
        }
        let stats = cache.stats();
        assert!(stats.bytes <= 100 && stats.entries <= 3, "{stats:?}");
        // Larger than half of the limit: not kept.
        for _ in 0..2 {
            keep(&cache, &key(5.0), values(51, 0), Charges::default());
        }
        assert!(cache.coverage(&key(5.0)).is_none());
        cache.clear();
        assert_eq!(cache.stats().bytes, 0);
        assert!(cache.coverage(&key(0.0)).is_none());
    }
}
