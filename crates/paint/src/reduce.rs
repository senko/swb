//! Reduced copies of raster images (ADR 0024): the mip levels that a
//! large reduction is drawn from.
//!
//! Bilinear sampling of an image that is drawn much smaller than it is skips
//! most source pixels and aliases (thin lines drop out). A reduction goes
//! through averaged halvings first, in each axis whose scale is below 0.5,
//! until the scale is at least 0.5; the bilinear draw starts from that
//! level (as GPU texture filtering picks a mip level).
//!
//! Averaging costs time proportional to the source pixels, so the levels
//! are kept with the decoded image and made lazily, one halving at a time.
//! A frame may average [`FRAME_PIXELS`] source pixels in all; past that the
//! image is drawn from the nearest level that exists, and a later frame
//! continues. An image keeps levels up to the size of its own pixels, so
//! the levels at most double its memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tiny_skia::Pixmap;

/// The source pixels that the reductions of one frame may average in
/// total: 64 Mpx, about 90 ms. Measured (release build): the three halvings
/// of a 4000 x 3000 image average 16 Mpx of source pixels in 22 ms, about
/// 1.4 ns per pixel. It is also the largest image that a reduction
/// averages (larger images are sampled as they are), so that the first
/// halving of any image that is reduced fits into the budget of an empty
/// frame; such an image can use most of that budget.
const FRAME_PIXELS: u64 = 64 << 20;

/// The reduced levels of one raster image. A level is named by the number
/// of halvings in each axis.
#[derive(Debug, Default)]
pub(crate) struct Reductions {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    levels: HashMap<(u8, u8), Arc<Pixmap>>,
    /// The bytes of `levels`.
    bytes: usize,
}

/// What a frame may still average.
#[derive(Debug)]
pub(crate) struct ReduceBudget {
    pixels: u64,
    skipped: bool,
}

impl ReduceBudget {
    /// The budget of a new frame.
    pub(crate) fn new() -> Self {
        ReduceBudget {
            pixels: FRAME_PIXELS,
            skipped: false,
        }
    }

    /// True if the frame left a level out.
    pub(crate) fn skipped(&self) -> bool {
        self.skipped
    }

    #[cfg(test)]
    pub(crate) fn with_pixels(pixels: u64) -> Self {
        ReduceBudget {
            pixels,
            skipped: false,
        }
    }

    fn take(&mut self, pixels: u64) -> bool {
        if pixels <= self.pixels {
            self.pixels -= pixels;
            true
        } else {
            self.skipped = true;
            false
        }
    }
}

impl Reductions {
    /// The level of `source` to draw from when it is drawn at the scale
    /// `sx` by `sy`: made now if the frame's `budget` allows, else the
    /// nearest level that exists. `None` if the source itself is the one
    /// to draw from (no reduction is needed, the image is too large, or
    /// no level exists and the budget has none).
    pub(crate) fn level(
        &self,
        source: &Pixmap,
        mut sx: f32,
        mut sy: f32,
        budget: &mut ReduceBudget,
    ) -> Option<Arc<Pixmap>> {
        if !(sx < 0.5 || sy < 0.5)
            || source.width() as usize * source.height() as usize > FRAME_PIXELS as usize
        {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let keep_bytes = source.data().len();
        let mut current: Option<Arc<Pixmap>> = None;
        let mut halvings = (0u8, 0u8);
        while sx < 0.5 || sy < 0.5 {
            let from = current.as_deref().unwrap_or(source);
            let (halve_x, halve_y) = (sx < 0.5 && from.width() > 1, sy < 0.5 && from.height() > 1);
            if !(halve_x || halve_y) {
                break;
            }
            halvings = (
                halvings.0 + u8::from(halve_x),
                halvings.1 + u8::from(halve_y),
            );
            let kept = state.levels.get(&halvings).map(Arc::clone);
            let next = if let Some(level) = kept {
                level
            } else {
                if !budget.take(u64::from(from.width()) * u64::from(from.height())) {
                    break;
                }
                let level = Arc::new(halve(from, halve_x, halve_y)?);
                if state.bytes + level.data().len() <= keep_bytes {
                    state.bytes += level.data().len();
                    state.levels.insert(halvings, Arc::clone(&level));
                }
                level
            };
            if halve_x {
                sx *= from.width() as f32 / next.width() as f32;
            }
            if halve_y {
                sy *= from.height() as f32 / next.height() as f32;
            }
            current = Some(next);
        }
        current
    }

    /// The bytes of the kept levels.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bytes
    }
}

/// `source` with every pair of columns (`across`) and rows (`down`)
/// averaged (in premultiplied color); an odd last column or row is kept as
/// it is.
fn halve(source: &Pixmap, across: bool, down: bool) -> Option<Pixmap> {
    let (width, height) = (source.width() as usize, source.height() as usize);
    let out_width = if across { width.div_ceil(2) } else { width };
    let out_height = if down { height.div_ceil(2) } else { height };
    let mut out = Pixmap::new(out_width as u32, out_height as u32)?;
    let src = source.data();
    let dst = out.data_mut();
    // The source indices of output index `i` along an axis.
    let pair = |halved: bool, i: usize, len: usize| {
        if halved {
            [2 * i, (2 * i + 1).min(len - 1)]
        } else {
            [i, i]
        }
    };
    for row in 0..out_height {
        let rows = pair(down, row, height);
        for col in 0..out_width {
            let cols = pair(across, col, width);
            let mut sum = [0u32; 4];
            for source_row in rows {
                for source_col in cols {
                    let at = (source_row * width + source_col) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(src.get(at + channel).copied().unwrap_or(0));
                    }
                }
            }
            let at = (row * out_width + col) * 4;
            for (channel, total) in sum.iter().enumerate() {
                if let Some(byte) = dst.get_mut(at + channel) {
                    *byte = ((total + 2) / 4) as u8;
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(width: u32, height: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        pixmap
    }

    /// A 1-pixel line in a 64 x 64 image is not lost when the image is
    /// reduced: the average of the blocks keeps its coverage.
    #[test]
    fn reduction_averages_thin_lines() {
        let mut pixmap = white(64, 64);
        let width = pixmap.width() as usize;
        for y in 0..64 {
            if let Some(p) = pixmap.pixels_mut().get_mut(y * width + 37) {
                *p = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).unwrap();
            }
        }
        let levels = Reductions::default();
        let mut budget = ReduceBudget::new();
        assert!(levels.level(&pixmap, 0.5, 0.5, &mut budget).is_none());
        // The reduction stops at the level whose scale is at least 0.5.
        let reduced = levels
            .level(&pixmap, 1.0 / 16.0, 1.0 / 16.0, &mut budget)
            .unwrap();
        assert_eq!((reduced.width(), reduced.height()), (8, 8));
        // Column 37 is in block 4 (columns 32 to 39): 1/8 of it is black.
        let line = reduced.pixel(4, 1).unwrap();
        assert!((i32::from(line.red()) - 223).abs() <= 2, "{}", line.red());
        assert_eq!(reduced.pixel(0, 1).unwrap().red(), 255);
        // Only the axis that needs it is reduced; odd sizes keep the last
        // column.
        let wide = levels.level(&white(5, 3), 0.2, 1.0, &mut budget).unwrap();
        assert_eq!((wide.width(), wide.height()), (2, 3));
    }

    /// A second request for a level reuses it and costs no budget.
    #[test]
    fn levels_are_kept_and_reused() {
        let pixmap = white(256, 256);
        let levels = Reductions::default();
        let mut first = ReduceBudget::new();
        let a = levels.level(&pixmap, 0.1, 0.1, &mut first).unwrap();
        let spent = FRAME_PIXELS - first.pixels;
        // 256^2 + 128^2 + 64^2 source pixels for three halvings.
        assert_eq!(spent, 256 * 256 + 128 * 128 + 64 * 64);
        assert!(levels.bytes() > 0);
        let mut second = ReduceBudget::with_pixels(0);
        let b = levels.level(&pixmap, 0.1, 0.1, &mut second).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!second.skipped());
    }

    /// Without budget the nearest level that exists is used, and the
    /// frame records the skip; a later frame continues.
    #[test]
    fn the_frame_budget_bounds_the_work() {
        let pixmap = white(256, 256);
        let levels = Reductions::default();
        let mut none = ReduceBudget::with_pixels(0);
        assert!(levels.level(&pixmap, 0.1, 0.1, &mut none).is_none());
        assert!(none.skipped());
        // Enough for the first halving only.
        let mut some = ReduceBudget::with_pixels(256 * 256);
        let level = levels.level(&pixmap, 0.1, 0.1, &mut some).unwrap();
        assert_eq!((level.width(), level.height()), (128, 128));
        assert!(some.skipped());
        let mut later = ReduceBudget::new();
        let level = levels.level(&pixmap, 0.1, 0.1, &mut later).unwrap();
        assert_eq!((level.width(), level.height()), (32, 32));
        assert!(!later.skipped());
    }

    /// The levels of an image never take more bytes than the image.
    #[test]
    fn kept_levels_are_bounded_by_the_image() {
        let pixmap = white(64, 64);
        let levels = Reductions::default();
        let mut budget = ReduceBudget::new();
        for (sx, sy) in [(0.2, 1.0), (1.0, 0.2), (0.2, 0.2), (0.01, 0.4), (0.4, 0.01)] {
            levels.level(&pixmap, sx, sy, &mut budget);
        }
        assert!(levels.bytes() <= pixmap.data().len());
    }
}
