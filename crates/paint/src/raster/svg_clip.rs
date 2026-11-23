//! Rasterization of SVG clip paths (ADR 0023): the coverage of the clip
//! shapes, multiplied into the pixels of a clip group's layer.
//!
//! The coverage of a [`ClipPath`] is the union of its shapes' anti-aliased
//! fills, each multiplied by the coverage of its own `clip-path`, and the
//! whole multiplied by the coverage of the clip path's own `clip-path`
//! (`outer`). Every step works on a temporary pixmap that counts against
//! the layer budget. A shape with its own `clip-path` works on the part of
//! the layer that its bounds cover. Every pass over pixels (the shapes'
//! fills, the pixmaps, the mask products) counts against the path work
//! budget, [`BLEND`] per pixel. A clip whose coverage cannot be made (over
//! budget) hides the group.

use std::sync::Arc;

use swb_layout::svg::{ClipPath, ClipRegion, SvgPath};
use swb_layout::{Matrix, Rect};
use swb_style::{FillRule, Rgba};
use tiny_skia::{IntRect, IntSize, Mask, Pixmap, PixmapPaint, Transform};

use super::cache::{Coverage, CoverageKey};
use super::coverage::multiply;
use super::path::{Scan, skia_path, skia_rule, skia_transform};
use super::{ImageSource, Rasterizer, skia_rect, solid_paint};
use crate::display_list::path_bounds;
use crate::image::mul_255;
use crate::path_cost::BLEND;

/// The deepest nesting of clip paths inside clip paths that is rasterized.
const MAX_NESTING: usize = 20;

/// The result of [`Rasterizer::apply_svg_clip`].
pub(super) enum ClipResult {
    /// The layer is multiplied by the coverage.
    Applied,
    /// Nothing remains visible: the group draws nothing.
    Hidden,
    /// The coverage does not fit into the budgets. An SVG clip group then
    /// draws nothing; a rounded overflow clip keeps its rectangle.
    OverBudget,
}

impl Rasterizer<'_> {
    /// Multiplies the pixels of a clip group's layer `pixmap` (at `origin` on
    /// the target) by the coverage of `clip`, whose coordinates are mapped to
    /// the display list's by `list`. Returns the result with the layer, and
    /// its origin: in a raster of parts, only the part of the layer inside
    /// the region's bounds (the multiplication is the same for each pixel;
    /// the coverage is that of the whole layer, which can come from the
    /// raster cache).
    pub(super) fn apply_svg_clip(
        &mut self,
        pixmap: Pixmap,
        origin: (i32, i32),
        clip: &Arc<ClipPath>,
        list: &Matrix,
        images: &dyn ImageSource,
    ) -> (ClipResult, Pixmap, (i32, i32)) {
        let Some(size) = IntSize::from_wh(pixmap.width(), pixmap.height()) else {
            return (ClipResult::Hidden, pixmap, origin);
        };
        let device = self.device_matrix(list);
        let to_layer = Matrix::translate(-(origin.0 as f32), -(origin.1 as f32)).multiply(&device);
        let Some(coverage) = self.whole_coverage(clip, &to_layer, size, images) else {
            return (ClipResult::OverBudget, pixmap, origin);
        };
        if coverage.hidden {
            return (ClipResult::Hidden, pixmap, origin);
        }
        match self.region_part(&pixmap, origin) {
            Part::Whole => {
                let mut pixmap = pixmap;
                if let Some(all) = IntRect::from_xywh(0, 0, size.width(), size.height()) {
                    multiply(&mut pixmap, coverage.mask.data(), size.width(), all);
                }
                (ClipResult::Applied, pixmap, origin)
            }
            // Nothing of the layer is in the region: its pixels do not
            // matter.
            Part::Outside => (ClipResult::Applied, pixmap, origin),
            Part::Inside(within) => {
                let Some(mut part) = pixmap.clone_rect(within) else {
                    return (ClipResult::Applied, pixmap, origin);
                };
                multiply(&mut part, coverage.mask.data(), size.width(), within);
                let at = (origin.0 + within.x(), origin.1 + within.y());
                (ClipResult::Applied, part, at)
            }
        }
    }

    /// The part of a layer (at `origin` on the target) inside the bounds of
    /// the region of a raster of parts, in layer px.
    fn region_part(&self, pixmap: &Pixmap, origin: (i32, i32)) -> Part {
        let Some(region) = &self.region else {
            return Part::Whole;
        };
        let layer = Rect::new(
            origin.0 as f32,
            origin.1 as f32,
            pixmap.width() as f32,
            pixmap.height() as f32,
        );
        let Some(part) = region.bounds().intersection(&layer) else {
            return Part::Outside;
        };
        if part == layer {
            return Part::Whole;
        }
        match IntRect::from_xywh(
            part.x as i32 - origin.0,
            part.y as i32 - origin.1,
            part.width as u32,
            part.height as u32,
        ) {
            Some(within) => Part::Inside(within),
            None => Part::Outside,
        }
    }

    /// The coverage of `clip` for a whole layer of `size` (see
    /// [`Rasterizer::path_coverage`]): from the raster cache if it has the
    /// same clip, matrix and size and the charges of the coverage fit into
    /// the budgets, else computed (and kept, if no budget left anything
    /// out).
    fn whole_coverage(
        &mut self,
        clip: &Arc<ClipPath>,
        to_layer: &Matrix,
        size: IntSize,
        images: &dyn ImageSource,
    ) -> Option<Arc<Coverage>> {
        let Some(cache) = images.raster_cache() else {
            let values = self.path_coverage(clip, to_layer, size, 0)?;
            return Coverage::new(values, size).map(Arc::new);
        };
        let key = CoverageKey::new(clip, to_layer, (size.width(), size.height()));
        if let Some((coverage, charges)) = cache.coverage(&key)
            && self.replay_charges(&charges)
        {
            return Some(coverage);
        }
        let mark = self.mark_charges();
        let values = self.path_coverage(clip, to_layer, size, 0);
        let charges = self.end_charges(mark);
        let coverage = Arc::new(Coverage::new(values?, size)?);
        if let Some(charges) = charges {
            cache.keep_coverage(key, Arc::clone(&coverage), charges);
        }
        Some(coverage)
    }

    /// The coverage (one byte per layer pixel) of `clip` for a layer of
    /// `size`, with `to_layer` from the clip's coordinates to layer px.
    fn path_coverage(
        &mut self,
        clip: &ClipPath,
        to_layer: &Matrix,
        size: IntSize,
        nesting: usize,
    ) -> Option<Vec<u8>> {
        if nesting > MAX_NESTING || !self.charge_pixels(size.width(), size.height()) {
            return None;
        }
        let mut union = self.layer_pixmap(size.width(), size.height())?;
        let mut ok = true;
        for shape in &clip.shapes {
            let matrix = to_layer.multiply(&shape.transform);
            match &shape.clip {
                None => {
                    self.fill_coverage(&mut union, &shape.path, &matrix, shape.rule);
                }
                Some(region) => {
                    ok = self.clipped_shape(&mut union, shape, region, to_layer, nesting);
                }
            }
            if !ok {
                break;
            }
        }
        let mut values = alpha_of(&union);
        self.release(&union);
        if !ok {
            return None;
        }
        if let Some(outer) = &clip.outer {
            let outer = self.region_coverage(outer, to_layer, size, nesting + 1)?;
            for (value, o) in values.iter_mut().zip(outer) {
                *value = mul_255(u32::from(*value), u32::from(o)) as u8;
            }
        }
        Some(values)
    }

    /// Adds the part of one shape inside its own clip `region` to `union`.
    /// Works on the pixels that the bounds of the shape cover.
    fn clipped_shape(
        &mut self,
        union: &mut Pixmap,
        shape: &swb_layout::svg::ClipShape,
        region: &ClipRegion,
        to_layer: &Matrix,
        nesting: usize,
    ) -> bool {
        let matrix = to_layer.multiply(&shape.transform);
        let layer = pixmap_rect(union);
        let Some(visible) = path_bounds(&shape.path, &matrix, 0.0).intersection(&layer) else {
            // The shape is outside the layer: it adds nothing.
            return true;
        };
        // Whole pixels, with a margin for the anti-aliasing.
        let x0 = (visible.x.floor() as i64 - 1).max(0) as u32;
        let y0 = (visible.y.floor() as i64 - 1).max(0) as u32;
        let x1 = ((visible.x + visible.width).ceil() as i64 + 1).clamp(0, i64::from(union.width()));
        let y1 =
            ((visible.y + visible.height).ceil() as i64 + 1).clamp(0, i64::from(union.height()));
        let (w, h) = (
            (x1 as u32).saturating_sub(x0),
            (y1 as u32).saturating_sub(y0),
        );
        let size = IntSize::from_wh(w, h);
        let (Some(size), true) = (size, self.charge_pixels(w, h)) else {
            return false;
        };
        let Some(mut one) = self.layer_pixmap(w, h) else {
            return false;
        };
        // The part of the layer is its own layer at (x0, y0).
        let local = Matrix::translate(-(x0 as f32), -(y0 as f32)).multiply(to_layer);
        self.fill_coverage(
            &mut one,
            &shape.path,
            &local.multiply(&shape.transform),
            shape.rule,
        );
        let ok = match self.region_coverage(region, &local, size, nesting + 1) {
            Some(values) => match Mask::from_vec(values, size) {
                Some(mask) => {
                    one.apply_mask(&mask);
                    union.draw_pixmap(
                        x0 as i32,
                        y0 as i32,
                        one.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    true
                }
                None => false,
            },
            None => false,
        };
        self.release(&one);
        ok
    }

    /// Charges a pass over `width` x `height` pixels to the path work
    /// budget. False if it does not fit.
    fn charge_pixels(&mut self, width: u32, height: u32) -> bool {
        self.path_work_fits(BLEND * f64::from(width) * f64::from(height))
    }

    /// The coverage of a clip region.
    fn region_coverage(
        &mut self,
        region: &ClipRegion,
        to_layer: &Matrix,
        size: IntSize,
        nesting: usize,
    ) -> Option<Vec<u8>> {
        match region {
            ClipRegion::Path(clip) => self.path_coverage(clip, to_layer, size, nesting),
            ClipRegion::Rect(rect) => {
                if !self.charge_pixels(size.width(), size.height()) {
                    return None;
                }
                let mut pixmap = self.layer_pixmap(size.width(), size.height())?;
                if let Some(path) = rect_path(rect) {
                    let ts = skia_transform(to_layer);
                    pixmap.fill_path(
                        &path,
                        &solid_paint(Rgba::BLACK, true),
                        tiny_skia::FillRule::Winding,
                        ts,
                        None,
                    );
                }
                let values = alpha_of(&pixmap);
                self.release(&pixmap);
                Some(values)
            }
        }
    }

    /// Fills `path` (mapped by `matrix` to the layer) onto `pixmap` in
    /// black: the alpha is the coverage. A path that does not fit into the
    /// path work budget is skipped.
    fn fill_coverage(
        &mut self,
        pixmap: &mut Pixmap,
        path: &SvgPath,
        matrix: &Matrix,
        rule: FillRule,
    ) {
        let bounds = path_bounds(path, matrix, 0.0);
        let layer = pixmap_rect(pixmap);
        let Some(visible) = bounds.intersection(&layer) else {
            return;
        };
        let rows = (0.0, pixmap.height() as f32);
        let columns = (0.0, pixmap.width() as f32);
        let scan = Scan::fill(true, rule);
        let Some(work) = self.fill_work(path, matrix, visible, (rows, columns), scan) else {
            return;
        };
        if !self.path_work_fits(work) {
            return;
        }
        let Some(sk_path) = skia_path(path) else {
            return;
        };
        pixmap.fill_path(
            &sk_path,
            &solid_paint(Rgba::BLACK, true),
            skia_rule(rule),
            skia_transform(matrix),
            None,
        );
    }
}

/// Where a layer is in the region of a raster of parts.
enum Part {
    /// All of it is inside the region's bounds (or there is no region).
    Whole,
    /// None of it.
    Outside,
    /// This part of it (layer px).
    Inside(IntRect),
}

/// The rectangle as a tiny-skia path, or `None` if it is empty.
fn rect_path(rect: &Rect) -> Option<tiny_skia::Path> {
    skia_rect(*rect).map(tiny_skia::PathBuilder::from_rect)
}

/// The rectangle of the whole pixmap.
fn pixmap_rect(pixmap: &Pixmap) -> Rect {
    Rect::new(0.0, 0.0, pixmap.width() as f32, pixmap.height() as f32)
}

/// The alpha channel of `pixmap`, one byte per pixel.
fn alpha_of(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .collect()
}
