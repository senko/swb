//! The glyph mask cache and its memory limits.
//!
//! Policy:
//!
//! - Masks of at most [`MAX_CACHED_MASK_BYTES`] (256 × 256 pixels) are
//!   cached. Larger masks occur only at very large font sizes; they are
//!   rasterized again on every request and are never kept. Skia also stops
//!   caching glyph images above 256 pixels and draws such glyphs as paths.
//! - The cached mask data never exceeds [`MASK_CACHE_BUDGET`] bytes, and the
//!   cache never has more than [`MAX_CACHED_MASKS`] entries (glyphs without
//!   a mask count as entries of zero bytes). If a new entry does not fit,
//!   the cache is cleared first. A page normally uses far fewer glyphs than
//!   the limits allow, so clearing is rare.

use std::collections::HashMap;
use std::sync::Arc;

use crate::raster::GlyphMask;
use crate::{FontId, GlyphId};

/// Largest mask, in bytes (one byte per pixel), that the cache keeps.
pub(crate) const MAX_CACHED_MASK_BYTES: usize = 256 * 256;

/// Largest total size of the cached mask data, in bytes.
pub(crate) const MASK_CACHE_BUDGET: usize = 64 << 20;

/// Largest number of cache entries.
pub(crate) const MAX_CACHED_MASKS: usize = 32 * 1024;

/// Identifies one rasterization result.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct MaskKey {
    pub(crate) font: FontId,
    pub(crate) glyph: GlyphId,
    /// The font size in pixels, as `f32` bits.
    pub(crate) size: u32,
    /// The horizontal offset in quarter pixels, 0 to 3.
    pub(crate) subpixel: u8,
}

/// Glyph masks by font, glyph, size and subpixel offset. `None` entries
/// record glyphs that have nothing to draw.
pub(crate) struct MaskCache {
    entries: HashMap<MaskKey, Option<Arc<GlyphMask>>>,
    /// Sum of the mask data sizes of all entries.
    bytes: usize,
    /// Largest value of `bytes`.
    budget: usize,
}

impl MaskCache {
    /// An empty cache that keeps at most `budget` bytes of mask data.
    pub(crate) fn new(budget: usize) -> Self {
        MaskCache {
            entries: HashMap::new(),
            bytes: 0,
            budget,
        }
    }

    /// The cached result for `key`, or `None` if there is none.
    pub(crate) fn get(&self, key: &MaskKey) -> Option<&Option<Arc<GlyphMask>>> {
        self.entries.get(key)
    }

    /// Stores the result for `key`, unless the mask is too large to cache.
    pub(crate) fn insert(&mut self, key: MaskKey, mask: Option<Arc<GlyphMask>>) {
        let bytes = mask_bytes(mask.as_ref());
        if bytes > MAX_CACHED_MASK_BYTES || bytes > self.budget {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= mask_bytes(old.as_ref());
        }
        if self.entries.len() >= MAX_CACHED_MASKS || self.bytes + bytes > self.budget {
            self.entries.clear();
            self.bytes = 0;
        }
        self.entries.insert(key, mask);
        self.bytes += bytes;
    }
}

fn mask_bytes(mask: Option<&Arc<GlyphMask>>) -> usize {
    mask.map_or(0, |mask| mask.data.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(glyph: u32) -> MaskKey {
        MaskKey {
            font: FontId(0),
            glyph: GlyphId(glyph),
            size: 16.0f32.to_bits(),
            subpixel: 0,
        }
    }

    fn mask(bytes: usize) -> Arc<GlyphMask> {
        Arc::new(GlyphMask {
            left: 0,
            top: 0,
            width: bytes as u32,
            height: 1,
            data: vec![255; bytes],
        })
    }

    #[test]
    fn small_masks_and_empty_glyphs_are_cached() {
        let mut cache = MaskCache::new(1000);
        cache.insert(key(1), Some(mask(100)));
        cache.insert(key(2), None);
        assert_eq!(cache.get(&key(1)), Some(&Some(mask(100))));
        assert_eq!(cache.get(&key(2)), Some(&None));
        assert_eq!(cache.get(&key(3)), None);
        assert_eq!(cache.bytes, 100);
    }

    #[test]
    fn large_masks_are_not_cached() {
        let mut cache = MaskCache::new(MASK_CACHE_BUDGET);
        cache.insert(key(1), Some(mask(MAX_CACHED_MASK_BYTES)));
        cache.insert(key(2), Some(mask(MAX_CACHED_MASK_BYTES + 1)));
        assert!(cache.get(&key(1)).is_some());
        assert_eq!(cache.get(&key(2)), None);
        assert_eq!(cache.bytes, MAX_CACHED_MASK_BYTES);
    }

    #[test]
    fn exceeding_the_budget_clears_the_cache() {
        let mut cache = MaskCache::new(1000);
        cache.insert(key(1), Some(mask(400)));
        cache.insert(key(2), Some(mask(400)));
        assert_eq!(cache.bytes, 800);
        cache.insert(key(3), Some(mask(400)));
        assert_eq!(cache.get(&key(1)), None);
        assert_eq!(cache.get(&key(2)), None);
        assert!(cache.get(&key(3)).is_some());
        assert_eq!(cache.bytes, 400);
        // A mask larger than the whole budget is not cached at all.
        cache.insert(key(4), Some(mask(1001)));
        assert_eq!(cache.get(&key(4)), None);
        assert!(cache.get(&key(3)).is_some());
    }

    #[test]
    fn replacing_an_entry_keeps_the_byte_count() {
        let mut cache = MaskCache::new(1000);
        cache.insert(key(1), Some(mask(300)));
        cache.insert(key(1), Some(mask(200)));
        assert_eq!(cache.bytes, 200);
    }

    #[test]
    fn entry_count_is_limited() {
        let mut cache = MaskCache::new(MASK_CACHE_BUDGET);
        for glyph in 0..=MAX_CACHED_MASKS as u32 {
            cache.insert(key(glyph), None);
        }
        assert!(cache.entries.len() <= MAX_CACHED_MASKS);
        assert_eq!(cache.get(&key(MAX_CACHED_MASKS as u32)), Some(&None));
    }
}
