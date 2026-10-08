//! CPU rasterization of a display list with tiny-skia.
//!
//! Coordinates in the display list are CSS px relative to the document.
//! The rasterizer converts them to device px of the target (it subtracts
//! the scroll offset and multiplies by the device scale factor) and draws
//! in device px.
//!
//! - Rectangles without rounded corners, clips and the edges of borders
//!   without rounded corners are snapped to device pixels and filled
//!   without anti-aliasing, so that adjacent boxes have no seams.
//! - An opacity group draws into a layer that covers only the visible part
//!   of the group's bounds; the layer is then composited with the group
//!   opacity. A mask group does the same; before it is composited, the
//!   layer is multiplied by the mask, whose layers are rendered into
//!   layers of the same size (`crate::mask`). Group layers have a memory
//!   budget and masks a work budget.
//! - A tall target (a full-page screenshot) can be rasterized in strips
//!   ([`rasterize_in_strips`]); the budgets apply to each strip.
//! - Glyphs come from the `text` crate as alpha masks and are blended by
//!   [`blit_mask`].
//! - Vector (SVG) images are rendered at the device pixel size of one tile
//!   and then drawn like raster images.
//! - Transform groups that only translate (fixed and sticky boxes,
//!   `translate()`) move the coordinates of their items, so they are
//!   exact. A group with another transform (rotation, scale, skew) is
//!   drawn into a layer in its own coordinates, at the device resolution
//!   times the largest scale factor of the transform, and the layer is
//!   then drawn with the transform and bilinear filtering (ADR 0016). The
//!   layer covers only the part of the group that can be visible in the
//!   strip, has at most [`MAX_TRANSFORM_LAYER_PIXELS`] pixels and sides of
//!   at most [`MAX_TRANSFORM_LAYER_SIDE`] pixels (a larger layer gets a
//!   lower resolution). Such layers nest at most [`MAX_TRANSFORM_DEPTH`]
//!   deep. A transform layer is an open layer for the memory budget of
//!   group layers while it is drawn, and the layers of one strip have a
//!   work budget ([`MAX_TRANSFORM_PIXELS`]); groups beyond these limits
//!   are not drawn.

use std::sync::Arc;

use swb_layout::{Matrix, Point, Rect, StickyCache};
use swb_style::{BorderStyle, Rgba};
use swb_text::FontContext;
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, IntSize, LinearGradient, Mask, Paint, Path, PathBuilder,
    Pattern, Pixmap, PixmapMut, PixmapPaint, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::display_list::{DisplayItem, DisplayList, ImageRef, Radii};
use crate::group_bounds::transform_ends;
use crate::image::{DecodedImage, ImageKind, MAX_DIMENSION};
use crate::mask::{self, MaskLayer, MaskLayerImage};
use crate::svg::{FrameBudget, MAX_RENDER_PIXELS, VectorCache};

mod path;
mod svg_clip;

/// The most pixels of a layer for a transform group (a larger layer gets
/// a lower resolution).
const MAX_TRANSFORM_LAYER_PIXELS: f32 = 4096.0 * 1024.0;

/// The longest side of a layer for a transform group, in pixels.
const MAX_TRANSFORM_LAYER_SIDE: f32 = 16_384.0;

/// The work of the layers of transform groups in one strip (see
/// [`rasterize_in_strips`]), in pixels: 64 Mpx, or eight times the strip's
/// pixels if that is more. A layer costs its pixels (drawing its content)
/// and the device pixels that it covers (drawing it with filtering). A
/// group whose layer does not fit into the rest of the budget is not
/// drawn.
const MAX_TRANSFORM_PIXELS: u64 = 64 * 1024 * 1024;

/// The deepest nesting of layers for transform groups.
const MAX_TRANSFORM_DEPTH: usize = 8;

/// Supplies decoded images to the rasterizer.
pub trait ImageSource {
    /// The decoded image, if it is loaded.
    fn image(&self, image: &ImageRef) -> Option<&DecodedImage>;

    /// The cache for vector images rendered at their drawn size. Without a
    /// cache, vector images are rendered for every draw.
    fn vector_cache(&self) -> Option<&VectorCache>;
}

/// Where and how to rasterize.
#[derive(Clone, Copy, Debug)]
pub struct RasterParams {
    /// The scroll offset in CSS px: this document point maps to the target's
    /// top-left corner.
    pub scroll: Point,
    /// The scroll offset of the viewport that fixed and sticky boxes are
    /// placed for: `scroll` for a viewport; a full-page screenshot draws
    /// the page from its top (`scroll` 0) with them where they are at the
    /// page's scroll offset (as Chromium).
    pub viewport_scroll: Point,
    /// Device pixels per CSS px.
    pub scale: f32,
}

/// The most pixels that the layers of open opacity, mask and transform
/// groups may hold together in one strip (see [`rasterize_in_strips`]): 64
/// Mpx (256 MiB), or four times the strip if that is more, so that a mask
/// group, the temporary layer of its mask and two enclosing groups always
/// fit. An opacity or mask layer is at most as large as the strip, a
/// transform layer at most [`MAX_TRANSFORM_LAYER_PIXELS`]. When a layer
/// does not fit, an opacity group draws its content directly (without the
/// opacity) and a mask or transform group draws nothing, so that deeply
/// nested groups cannot exhaust memory. While a mask is applied, its
/// coverage buffers take 2 more bytes per pixel of the group.
const MAX_GROUP_LAYER_PIXELS: u64 = 64 * 1024 * 1024;

/// The mask work of one strip, in pixels: 64 Mpx, about 0.4 s (measured:
/// about 6.4 ns per pixel), or four times the strip's pixels and two row
/// costs per row if that is more. A mask group starts only if all its
/// work fits into the rest of the budget: for each mask layer, the
/// group's layer pixels, its rows (see [`ROW_COST_PIXELS`]), and the
/// gradient tiles that it draws or renders (see [`Rasterizer::mask_work`]).
/// So a strip-sized group with up to four mask layers (images, or
/// gradients that are not repeated) renders when it is the strip's first
/// mask group. A group
/// that does not fit gets no layer and draws nothing, so that many large
/// masks with many layers cannot hang the engine. Nothing is charged
/// after a group starts, so a strip never does more mask work than the
/// budget. Wikipedia's icons need about 0.02 Mpx.
const MAX_MASK_PIXELS: u64 = 64 * 1024 * 1024;

/// The work of one row in pixels, for the mask budget. Drawing a row has a
/// fixed cost: a 1 px wide tile takes about 82 ns per pixel instead of
/// 6.4 ns (measured).
const ROW_COST_PIXELS: u64 = 16;

/// The most visible tiles of a gradient mask layer that are drawn one by
/// one; with more, one tile is rendered and repeated as an image.
const MAX_GRADIENT_TILES: i64 = 16;

/// Rasterizes a display list into `target`. The target is not cleared.
///
/// New renderings of SVG images share one work budget per call. When it is
/// used up, later SVG images are drawn from a cached rendering of another
/// size, or not at all; a later call (a repaint) renders them if its budget
/// allows. Normal pages stay far below the budget. The layers of nested
/// opacity, mask and transform groups share a memory budget
/// ([`MAX_GROUP_LAYER_PIXELS`]); masks ([`MAX_MASK_PIXELS`]) and transform
/// layers ([`MAX_TRANSFORM_PIXELS`]) have work budgets.
pub fn rasterize(
    list: &DisplayList,
    target: &mut Pixmap,
    params: RasterParams,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) {
    let rows = target.height();
    rasterize_in_strips(list, target, params, rows, fonts, images);
}

/// [`rasterize`] in strips of at most `strip_rows` device rows (at least
/// 1), as if each strip were a viewport of its own: the budgets of group
/// layers, masks and transform layers apply to each strip, and opacity and
/// mask layers are at most a strip high. Fixed and sticky boxes are placed
/// for `params.scroll` in every strip. The strips have equal heights (the last one can be lower
/// by fewer rows than there are strips), and all get the budgets of that
/// height. Each strip is drawn in
/// place in the target's rows. The SVG rendering budget is shared by all
/// strips. Device coordinates are computed as in one pass and then
/// shifted by the strip's row, so rounding is the same; results can still
/// differ by a few levels in single pixels: anti-aliased edges of paths
/// that cross a strip boundary (tiny-skia clips them to the strip), and
/// filtered images and gradients (positions in the image or along the
/// gradient are computed relative to the strip).
pub fn rasterize_in_strips(
    list: &DisplayList,
    target: &mut Pixmap,
    params: RasterParams,
    strip_rows: u32,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) {
    let mut vectors = images
        .vector_cache()
        .map_or_else(FrameBudget::new, VectorCache::begin_frame);
    let (width, height) = (target.width(), target.height());
    let rows = equal_strip_rows(height, strip_rows);
    let row_bytes = width as usize * 4;
    let mut skipped = Skipped::default();
    let ends = transform_ends(&list.items);
    for (k, data) in target
        .data_mut()
        .chunks_mut(rows as usize * row_bytes)
        .enumerate()
    {
        let strip = Strip {
            height: data_rows(data.len(), row_bytes),
            data,
            width,
            top: k as u32 * rows,
            budget_rows: rows,
        };
        let s = rasterize_strip(list, &ends, strip, params, &mut vectors, fonts, images);
        skipped.layers |= s.layers;
        skipped.masks |= s.masks;
        skipped.transforms |= s.transforms;
        skipped.paths |= s.paths;
        skipped.dashes |= s.dashes;
    }
    if vectors.skipped() {
        log::warn!(
            "SVG images: the rendering budget of this frame ran out; some images \
             use a rendering of another size or are not drawn"
        );
    }
    if skipped.layers {
        log::warn!(
            "opacity, mask and transform groups: the layer memory budget ran out; some \
             opacity groups are drawn without their opacity, some groups are not drawn"
        );
    }
    if skipped.masks {
        log::warn!("masks: the work budget of this frame ran out; some masked boxes are not drawn");
    }
    if skipped.transforms {
        log::warn!(
            "transforms: the nesting depth or the work budget of layers ran out; some \
             transformed boxes are not drawn"
        );
    }
    if skipped.paths {
        log::warn!("inline SVG: the path work budget ran out; some shapes are not drawn");
    }
    if skipped.dashes {
        log::warn!("inline SVG: a stroke has too many dashes; it is drawn solid");
    }
}

/// The height of equal strips for a target of `height` rows and strips of
/// at most `max_rows` rows: `height` split into the fewest strips, as
/// equal as possible (the last one can be lower).
fn equal_strip_rows(height: u32, max_rows: u32) -> u32 {
    let height = height.max(1);
    let count = height.div_ceil(max_rows.clamp(1, height));
    height.div_ceil(count)
}

/// The number of rows in `bytes` bytes of rows of `row_bytes` bytes.
fn data_rows(bytes: usize, row_bytes: usize) -> u32 {
    u32::try_from(bytes / row_bytes.max(1)).unwrap_or(u32::MAX)
}

/// One strip of the target: its rows, in place.
struct Strip<'a> {
    /// The strip's pixels (premultiplied RGBA, rows of `width` pixels).
    data: &'a mut [u8],
    width: u32,
    height: u32,
    /// The strip's first row in the target.
    top: u32,
    /// The rows of a strip for the budgets (all strips of a target get the
    /// same budgets).
    budget_rows: u32,
}

/// What the budgets of a strip left out.
#[derive(Clone, Copy, Default)]
struct Skipped {
    /// A group layer did not fit into [`MAX_GROUP_LAYER_PIXELS`].
    layers: bool,
    /// A mask group did not fit into [`MAX_MASK_PIXELS`].
    masks: bool,
    /// A transform group was nested deeper than [`MAX_TRANSFORM_DEPTH`] or
    /// did not fit into [`MAX_TRANSFORM_PIXELS`].
    transforms: bool,
    /// An SVG path did not fit into [`path::MAX_PATH_WORK`].
    paths: bool,
    /// An SVG stroke had more than [`path::MAX_DASHES`] dashes.
    dashes: bool,
}

/// The budgets of one strip. The rasterizer of a transform layer takes
/// them over while it draws, so that the groups inside the layer count
/// too.
#[derive(Default)]
struct Budget {
    /// The pixels of the open layers' pixmaps, together: group layers,
    /// transform layers and temporary mask layers.
    layer_pixels: u64,
    /// The budget of `layer_pixels` (see [`MAX_GROUP_LAYER_PIXELS`]).
    max_layer_pixels: u64,
    /// The mask work so far (see [`MAX_MASK_PIXELS`]).
    mask_pixels: u64,
    /// The budget of `mask_pixels`.
    max_mask_pixels: u64,
    /// The pixels of the transform layers so far (see
    /// [`MAX_TRANSFORM_PIXELS`]).
    transform_pixels: u64,
    /// The budget of `transform_pixels`.
    max_transform_pixels: u64,
    /// The work of SVG paths so far (see [`path::MAX_PATH_WORK`]).
    path_work: f64,
    /// The budget of `path_work`.
    max_path_work: f64,
    /// What the budgets left out.
    skipped: Skipped,
}

/// Rasterizes one strip of the target. `ends` comes from [`transform_ends`].
fn rasterize_strip(
    list: &DisplayList,
    ends: &[usize],
    Strip {
        data,
        width,
        height,
        top,
        budget_rows,
    }: Strip<'_>,
    params: RasterParams,
    vectors: &mut FrameBudget,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) -> Skipped {
    // The strip in CSS px, 1 device px larger on every side: snapping can
    // move a thin rectangle into the next row.
    let s = params.scale;
    let viewport = Rect::new(
        params.scroll.x - 1.0 / s,
        params.scroll.y + (top as f32 - 1.0) / s,
        (width as f32 + 2.0) / s,
        (height as f32 + 2.0) / s,
    );
    let rows = u64::from(budget_rows);
    let pixels = rows.saturating_mul(u64::from(width));
    let mask_budget = rows
        .saturating_mul(u64::from(width) + 2 * ROW_COST_PIXELS)
        .saturating_mul(4);
    let mut r = Rasterizer {
        target: data,
        target_size: (width, height),
        offset_y: top as f32,
        params,
        layers: Vec::new(),
        budget: Budget {
            max_layer_pixels: MAX_GROUP_LAYER_PIXELS.max(pixels.saturating_mul(4)),
            max_mask_pixels: MAX_MASK_PIXELS.max(mask_budget),
            max_transform_pixels: MAX_TRANSFORM_PIXELS.max(pixels.saturating_mul(8)),
            max_path_work: path::MAX_PATH_WORK,
            ..Budget::default()
        },
        clips: Vec::new(),
        mask: None,
        vectors,
        translation: Point::default(),
        visible: viewport,
        viewport_scroll: params.viewport_scroll,
        depth: 0,
        sticky: StickyCache::default(),
    };
    r.run(&list.items, 0..list.items.len(), ends, fonts, images);
    // Close unbalanced groups.
    while !r.layers.is_empty() {
        r.pop_layer(images);
    }
    r.budget.skipped
}

/// The layer of an open opacity or mask group.
struct Layer {
    /// The pixels; `None` if the group draws nothing visible (opacity 0,
    /// or bounds outside the clip) or draws directly. Drawing is then
    /// skipped, unless `direct`.
    pixmap: Option<Pixmap>,
    /// True if the group draws onto the surface below it (an opacity group
    /// whose layer did not fit into the layer budget).
    direct: bool,
    /// The position of the layer's top-left pixel on the target.
    origin: (i32, i32),
    opacity: f32,
    /// The mask layers of a mask group.
    mask: Option<Arc<[MaskLayer]>>,
    /// The clip path of an SVG clip group, and the matrix from its
    /// coordinates to the display list's.
    svg_clip: Option<SvgClip>,
}

/// An SVG clip path with the matrix from its coordinates to the list's.
type SvgClip = (Arc<swb_layout::svg::ClipPath>, Matrix);

struct Rasterizer<'a> {
    /// The pixels of the target (or of a strip of it), premultiplied RGBA.
    target: &'a mut [u8],
    /// The width and height of `target`.
    target_size: (u32, u32),
    /// The target's first device row in the whole target (of a strip):
    /// subtracted after scaling, so that rounding is as in one pass.
    offset_y: f32,
    params: RasterParams,
    /// Open opacity and mask groups, innermost last.
    layers: Vec<Layer>,
    /// The budgets of the strip.
    budget: Budget,
    /// Clip rectangles in target device px, snapped to pixels; each entry
    /// is already intersected with the previous one.
    clips: Vec<Rect>,
    /// The mask of the innermost clip for the current surface, created on
    /// demand.
    mask: Option<Mask>,
    /// The rendering budget of this frame for SVG images.
    vectors: &'a mut FrameBudget,
    /// The translation of the enclosing transform groups, in CSS px.
    translation: Point,
    /// The area that can be visible, in CSS px, in the coordinates that
    /// `params` maps to device px (before `translation`).
    visible: Rect,
    /// The scroll offset of the viewport (the offset of fixed and sticky
    /// boxes depends on it); inside a layer, `params.scroll` is the origin
    /// of the layer instead.
    viewport_scroll: Point,
    /// The number of enclosing layers of transform groups.
    depth: usize,
    /// The offsets of sticky boxes at `viewport_scroll`, shared by the
    /// groups of the strip (the rasterizer of a transform layer takes it
    /// over while it draws).
    sticky: StickyCache,
}

/// `area` (a part of `bounds`, in a group's coordinates) grown by two
/// layer pixels on every side for the scale `k` (layer px per CSS px),
/// inside `bounds`, with its top-left corner on the grid of layer pixels
/// that starts at the top-left corner of `bounds`. The layers of a group in
/// different strips then have pixels at the same positions, and the
/// filtering at the layer's edges reads pixels of the group's content.
fn grid_area(area: Rect, bounds: Rect, k: f32) -> Rect {
    let k64 = f64::from(k);
    let snap = |v: f32, origin: f32| {
        let origin = f64::from(origin);
        let steps = (((f64::from(v) - origin) * k64).floor() - 2.0).max(0.0);
        (origin + steps / k64) as f32
    };
    let x = snap(area.x, bounds.x);
    let y = snap(area.y, bounds.y);
    let right = (area.right() + 2.0 / k).min(bounds.right());
    let bottom = (area.bottom() + 2.0 / k).min(bounds.bottom());
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// The scale (layer px per CSS px of the group) of the layer of a
/// transform group that covers `area` (the group's coordinates), for the
/// scale `k` of the device resolution: limited so that the layer has sides
/// of at most [`MAX_TRANSFORM_LAYER_SIDE`] and at most
/// [`MAX_TRANSFORM_LAYER_PIXELS`] pixels. `None` for a scale that is not
/// finite and positive.
fn layer_scale(area: Rect, mut k: f32) -> Option<f32> {
    if !(k.is_finite() && k > 0.0) {
        return None;
    }
    let side = area.width.max(area.height) * k;
    if side > MAX_TRANSFORM_LAYER_SIDE {
        k *= MAX_TRANSFORM_LAYER_SIDE / side;
    }
    let pixels = (area.width * k).max(1.0) * (area.height * k).max(1.0);
    if pixels > MAX_TRANSFORM_LAYER_PIXELS {
        k *= (MAX_TRANSFORM_LAYER_PIXELS / pixels).sqrt();
    }
    k.is_normal().then_some(k)
}

/// The size in pixels of a layer that covers `area` at the scale `k`, if
/// it is within the limits of [`layer_scale`] (with room for rounding).
fn layer_size(area: Rect, k: f32) -> Option<(u32, u32)> {
    let width = (area.width * k).ceil().max(1.0);
    let height = (area.height * k).ceil().max(1.0);
    let fits = width <= MAX_TRANSFORM_LAYER_SIDE + 4.0
        && height <= MAX_TRANSFORM_LAYER_SIDE + 4.0
        && width * height <= MAX_TRANSFORM_LAYER_PIXELS * 2.0;
    fits.then_some((width as u32, height as u32))
}

/// Where and how large the layer of a transform group is.
struct LayerPlan {
    /// From the group's coordinates to target device px.
    to_device: Matrix,
    /// The part of the group that the layer covers (the group's
    /// coordinates).
    area: Rect,
    /// Layer px per CSS px of the group.
    scale: f32,
    width: u32,
    height: u32,
    /// The cost for [`MAX_TRANSFORM_PIXELS`].
    work: u64,
}

impl Rasterizer<'_> {
    /// Draws `items[range]`. `ends` comes from [`transform_ends`].
    fn run(
        &mut self,
        items: &[DisplayItem],
        range: std::ops::Range<usize>,
        ends: &[usize],
        fonts: &mut FontContext,
        images: &dyn ImageSource,
    ) {
        let mut i = range.start;
        while i < range.end {
            let Some(item) = items.get(i) else {
                break;
            };
            if let DisplayItem::PushTransform {
                transform,
                bounds,
                fixed_bounds,
                ..
            } = item
            {
                let end = ends.get(i).copied().unwrap_or(range.end).min(range.end);
                let outer = Matrix::translate(self.translation.x, self.translation.y);
                let m = transform.resolve_with(&outer, self.viewport_scroll, &mut self.sticky);
                // Content fixed to the viewport in the group moves with the
                // viewport scroll offset.
                let fixed_visible = fixed_bounds.is_some_and(|f| {
                    f.translate(self.viewport_scroll)
                        .intersection(&self.visible)
                        .is_some()
                });
                let visible = m.is_finite()
                    && (m.map_rect(bounds).intersection(&self.visible).is_some() || fixed_visible);
                if visible && m.is_translation() {
                    let saved = self.translation;
                    self.translation = Point::new(m.e, m.f);
                    self.run(items, i + 1..end, ends, fonts, images);
                    self.translation = saved;
                } else if visible {
                    self.transform_layer(items, i + 1..end, ends, &m, *bounds, fonts, images);
                }
                i = end + 1;
                continue;
            }
            let culled = item.bounds().is_some_and(|b| {
                b.translate(self.translation)
                    .intersection(&self.visible)
                    .is_none()
            });
            if !culled {
                self.item(item, fonts, images);
            }
            i += 1;
        }
    }

    /// Draws `items[range]`, a transform group whose items `m` maps to the
    /// coordinates of `params` and that draws in `bounds` (its own
    /// coordinates), through a layer (see the module documentation).
    #[allow(clippy::too_many_arguments)]
    fn transform_layer(
        &mut self,
        items: &[DisplayItem],
        range: std::ops::Range<usize>,
        ends: &[usize],
        m: &Matrix,
        bounds: Rect,
        fonts: &mut FontContext,
        images: &dyn ImageSource,
    ) {
        if self.depth >= MAX_TRANSFORM_DEPTH {
            self.budget.skipped.transforms = true;
            return;
        }
        let Some(plan) = self.plan_layer(m, bounds) else {
            return;
        };
        if self.budget.transform_pixels.saturating_add(plan.work) > self.budget.max_transform_pixels
        {
            self.budget.skipped.transforms = true;
            return;
        }
        let LayerPlan {
            to_device,
            area,
            scale: k,
            width: layer_width,
            height: layer_height,
            work,
        } = plan;
        let Some(mut layer) = self.layer_pixmap(layer_width, layer_height) else {
            return;
        };
        self.budget.transform_pixels += work;
        // The groups inside the layer share the budgets of the strip. Fixed
        // and sticky groups inside it still move with the viewport.
        let budget = std::mem::take(&mut self.budget);
        self.budget = {
            let mut nested = Rasterizer {
                target: layer.data_mut(),
                target_size: (layer_width, layer_height),
                offset_y: 0.0,
                params: RasterParams {
                    scroll: area.origin(),
                    viewport_scroll: self.viewport_scroll,
                    scale: k,
                },
                layers: Vec::new(),
                budget,
                clips: Vec::new(),
                mask: None,
                vectors: &mut *self.vectors,
                translation: Point::default(),
                visible: area,
                viewport_scroll: self.viewport_scroll,
                depth: self.depth + 1,
                sticky: std::mem::take(&mut self.sticky),
            };
            nested.run(items, range, ends, fonts, images);
            while !nested.layers.is_empty() {
                nested.pop_layer(images);
            }
            self.sticky = std::mem::take(&mut nested.sticky);
            nested.budget
        };
        // Layer px to target device px.
        let to_target =
            to_device.multiply(&Matrix::new(1.0 / k, 0.0, 0.0, 1.0 / k, area.x, area.y));
        self.draw_transformed(&layer, &to_target);
        self.release(&layer);
    }

    /// The layer of a transform group whose items `m` maps to the
    /// coordinates of `params` and that draws in `bounds` (its own
    /// coordinates): the part that can be visible, on the grid of layer
    /// pixels of `bounds` (see [`grid_area`]). `None` if nothing is
    /// visible or the transform cannot be inverted.
    fn plan_layer(&self, m: &Matrix, bounds: Rect) -> Option<LayerPlan> {
        let s = self.params.scale;
        let scroll = self.params.scroll;
        let to_device =
            Matrix::new(s, 0.0, 0.0, s, -scroll.x * s, -scroll.y * s - self.offset_y).multiply(m);
        let visible = self.visible_area()?;
        let visible_part = to_device
            .invert()?
            .map_rect(&visible)
            .intersection(&bounds)?;
        // The scale for the visible part; the margin of the grid can lower
        // it a little, and the area is snapped with the final scale.
        let k = layer_scale(visible_part, s * m.max_scale())?;
        let k = layer_scale(grid_area(visible_part, bounds, k), k)?;
        let area = grid_area(visible_part, bounds, k);
        let (width, height) = layer_size(area, k)?;
        let covered = to_device
            .map_rect(&area)
            .intersection(&visible)
            .map_or(0, |r| (r.width as u64).saturating_mul(r.height as u64));
        let work = (u64::from(width) * u64::from(height)).saturating_add(covered);
        Some(LayerPlan {
            to_device,
            area,
            scale: k,
            width,
            height,
            work,
        })
    }

    /// Draws `layer` with the transform `to_target` (layer px to target
    /// device px), bilinear filtering and anti-aliased edges
    /// (`draw_pixmap` does not anti-alias them).
    fn draw_transformed(&mut self, layer: &Pixmap, to_target: &Matrix) {
        let (width, height) = (layer.width() as f32, layer.height() as f32);
        let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, width, height) else {
            return;
        };
        let device_bounds = to_target.map_rect(&Rect::new(0.0, 0.0, width, height));
        let paint = Paint {
            shader: Pattern::new(
                layer.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                1.0,
                Transform::identity(),
            ),
            anti_alias: true,
            ..Paint::default()
        };
        let layer_transform = Transform::from_row(
            to_target.a,
            to_target.b,
            to_target.c,
            to_target.d,
            to_target.e,
            to_target.f,
        );
        self.draw(device_bounds, |p, t, mask| {
            p.fill_rect(rect, &paint, t.pre_concat(layer_transform), mask);
        });
    }

    fn item(&mut self, item: &DisplayItem, fonts: &mut FontContext, images: &dyn ImageSource) {
        match item {
            DisplayItem::Rect { rect, radii, color } => self.fill_rect(*rect, radii, *color),
            DisplayItem::Border {
                rect,
                widths,
                colors,
                styles,
                radii,
            } => self.border(*rect, *widths, *colors, *styles, radii),
            DisplayItem::Text {
                origin,
                font,
                size,
                glyphs,
                color,
            } => self.text(*origin, *font, *size, glyphs, *color, fonts),
            DisplayItem::Image {
                image,
                rect,
                tile,
                clip,
            } => {
                if let Some(decoded) = images.image(image) {
                    self.decoded_image(decoded, images.vector_cache(), *rect, *tile, *clip);
                }
            }
            DisplayItem::LinearGradient {
                rect,
                clip,
                gradient,
                current_color,
            } => self.linear_gradient(*rect, *clip, gradient, *current_color),
            DisplayItem::PushClip(rect) => {
                let device = snap(self.to_device(*rect));
                let clip = match self.clips.last() {
                    Some(c) => c.intersection(&device).unwrap_or_default(),
                    None => device,
                };
                self.clips.push(clip);
                self.mask = None;
            }
            // The target is the viewport (or a strip of it): the clip
            // hides nothing there.
            DisplayItem::PushViewportClip => {
                let (width, height) = self.target_size;
                self.clips
                    .push(Rect::new(0.0, 0.0, width as f32, height as f32));
                self.mask = None;
            }
            DisplayItem::PopClip => {
                self.clips.pop();
                self.mask = None;
            }
            DisplayItem::PushOpacity {
                opacity,
                bounds,
                fixed_bounds,
                escapes_clips,
            } => self.push_layer(
                *opacity,
                (*bounds, *fixed_bounds),
                None,
                None,
                *escapes_clips,
            ),
            // The mask is drawn inside the enclosing clips, so content
            // fixed to the viewport in the group cannot escape them.
            DisplayItem::PushMask { bounds, layers } => {
                self.push_layer(1.0, (*bounds, None), Some(Arc::clone(layers)), None, false);
            }
            DisplayItem::PopOpacity | DisplayItem::PopMask | DisplayItem::PopSvgClip => {
                self.pop_layer(images);
            }
            DisplayItem::Polyline {
                points,
                width,
                color,
            } => self.polyline(points, *width, *color),
            DisplayItem::Polygon { points, color } => self.polygon(points, *color),
            DisplayItem::FillPath { .. }
            | DisplayItem::StrokePath { .. }
            | DisplayItem::PushSvgClip { .. } => self.svg_item(item),
            // `run` handles transform groups.
            DisplayItem::HitRegion { .. }
            | DisplayItem::HitShape { .. }
            | DisplayItem::PushTransform { .. }
            | DisplayItem::PopTransform => {}
        }
    }

    /// Strokes a line through `points` with butt caps and miter joins.
    fn polyline(&mut self, points: &[Point], width: f32, color: Rgba) {
        if color.is_transparent() || width <= 0.0 || points.len() < 2 {
            return;
        }
        let Some(path) = self.device_path(points, false) else {
            return;
        };
        let stroke = Stroke {
            width: width * self.params.scale,
            ..Stroke::default()
        };
        let bounds = path.bounds();
        let grow = stroke.width * 2.0;
        let device = Rect::new(
            bounds.x() - grow,
            bounds.y() - grow,
            bounds.width() + 2.0 * grow,
            bounds.height() + 2.0 * grow,
        );
        let paint = solid_paint(color, true);
        self.draw(device, |p, t, m| {
            p.stroke_path(&path, &paint, &stroke, t, m);
        });
    }

    /// Fills the polygon through `points` (non-zero winding rule).
    fn polygon(&mut self, points: &[Point], color: Rgba) {
        if color.is_transparent() || points.len() < 3 {
            return;
        }
        let Some(path) = self.device_path(points, true) else {
            return;
        };
        let bounds = path.bounds();
        let device = Rect::new(
            bounds.x() - 1.0,
            bounds.y() - 1.0,
            bounds.width() + 2.0,
            bounds.height() + 2.0,
        );
        let paint = solid_paint(color, true);
        self.draw(device, |p, t, m| {
            p.fill_path(&path, &paint, FillRule::Winding, t, m);
        });
    }

    /// The path through `points` in device coordinates, closed if
    /// `close`. `None` if it is empty or not finite.
    fn device_path(&self, points: &[Point], close: bool) -> Option<Path> {
        let mut pb = PathBuilder::new();
        for (i, p) in points.iter().enumerate() {
            let d = self.to_device_point(*p);
            if i == 0 {
                pb.move_to(d.x, d.y);
            } else {
                pb.line_to(d.x, d.y);
            }
        }
        if close {
            pb.close();
        }
        pb.finish()
    }

    // ----- Coordinates and surfaces -----

    fn to_device(&self, r: Rect) -> Rect {
        let p = self.to_device_point(r.origin());
        let s = self.params.scale;
        Rect::new(p.x, p.y, r.width * s, r.height * s)
    }

    fn to_device_point(&self, p: Point) -> Point {
        let s = self.params.scale;
        Point::new(
            (p.x + self.translation.x - self.params.scroll.x) * s,
            (p.y + self.translation.y - self.params.scroll.y) * s - self.offset_y,
        )
    }

    /// The current drawing surface in target device px, or `None` while
    /// drawing is skipped. Layers that draw directly are not surfaces.
    fn surface_rect(&self) -> Option<Rect> {
        match self.layers.iter().rev().find(|l| !l.direct) {
            None => Some(Rect::new(
                0.0,
                0.0,
                self.target_size.0 as f32,
                self.target_size.1 as f32,
            )),
            Some(layer) => layer.pixmap.as_ref().map(|p| {
                Rect::new(
                    layer.origin.0 as f32,
                    layer.origin.1 as f32,
                    p.width() as f32,
                    p.height() as f32,
                )
            }),
        }
    }

    fn surface_mut(&mut self) -> Option<PixmapMut<'_>> {
        let (width, height) = self.target_size;
        match self.layers.iter_mut().rev().find(|l| !l.direct) {
            None => PixmapMut::from_bytes(self.target, width, height),
            Some(layer) => layer.pixmap.as_mut().map(Pixmap::as_mut),
        }
    }

    /// The part of the current surface inside the clip, in target device
    /// px.
    fn visible_area(&self) -> Option<Rect> {
        let surface = self.surface_rect()?;
        match self.clips.last() {
            Some(clip) => clip.intersection(&surface),
            None => Some(surface),
        }
    }

    /// Draws on the current surface if a part of `bounds` (target device
    /// px) is visible. `paint` gets the surface, the transform from target
    /// device px to surface px, and the clip mask (`None` if `bounds` is
    /// inside the clip).
    fn draw(
        &mut self,
        bounds: Rect,
        paint: impl FnOnce(&mut PixmapMut<'_>, Transform, Option<&Mask>),
    ) {
        let Some(surface) = self.surface_rect() else {
            return;
        };
        let clip = self.clips.last().copied();
        let visible = match clip {
            Some(c) => c.intersection(&surface),
            None => Some(surface),
        };
        if visible.and_then(|v| v.intersection(&bounds)).is_none() {
            return;
        }
        let needs_mask = clip.is_some_and(|c| !contains(&c, &bounds));
        if needs_mask && self.mask.is_none() {
            self.mask = clip.and_then(|c| clip_mask(c, surface));
            if self.mask.is_none() {
                return;
            }
        }
        let transform = Transform::from_translate(-surface.x, -surface.y);
        let Rasterizer {
            target,
            target_size,
            layers,
            mask,
            ..
        } = self;
        let pixmap = match layers.iter_mut().rev().find(|l| !l.direct) {
            None => PixmapMut::from_bytes(target, target_size.0, target_size.1),
            Some(layer) => layer.pixmap.as_mut().map(Pixmap::as_mut),
        };
        let Some(mut pixmap) = pixmap else {
            return;
        };
        let mask = if needs_mask { mask.as_ref() } else { None };
        paint(&mut pixmap, transform, mask);
    }

    // ----- Opacity and mask groups -----

    /// Starts an opacity or mask group that draws in `bounds`. If
    /// `escapes_clips`, the group contains content fixed to the viewport,
    /// which the enclosing clips do not clip.
    fn push_layer(
        &mut self,
        opacity: f32,
        (bounds, fixed): (Rect, Option<Rect>),
        mask: Option<Arc<[MaskLayer]>>,
        svg_clip: Option<SvgClip>,
        escapes_clips: bool,
    ) {
        self.mask = None;
        let visible = if escapes_clips {
            self.surface_rect()
        } else {
            self.visible_area()
        };
        // The group's content, and its content fixed to the viewport where
        // it is at the viewport scroll offset (not moved by the enclosing
        // translations). Empty areas do not count.
        let non_empty = |r: &Rect| r.width > 0.0 && r.height > 0.0;
        let mut device = Some(bounds).filter(non_empty).map(|b| self.to_device(b));
        if let Some(fixed) = fixed.filter(non_empty) {
            let saved = std::mem::take(&mut self.translation);
            let at = self.to_device(fixed.translate(self.viewport_scroll));
            self.translation = saved;
            device = Some(device.map_or(at, |d| d.union(&at)));
        }
        let area = visible
            .zip(device)
            .and_then(|(v, d)| snap_out(d).intersection(&v));
        // A mask group starts only if its work fits into the budget.
        let work = match (&mask, area) {
            (Some(layers), Some(a)) => self.mask_work(a, layers),
            _ => 0,
        };
        let area = area.filter(|_| mask.is_none() || self.mask_work_fits(work));
        let plain = mask.is_none() && svg_clip.is_none();
        let layer = match area {
            Some(a) if opacity > 0.0 => {
                let pixmap = self.layer_pixmap(a.width as u32, a.height as u32);
                if pixmap.is_some() {
                    self.add_mask_work(work);
                }
                Layer {
                    direct: pixmap.is_none() && plain,
                    pixmap,
                    origin: (a.x as i32, a.y as i32),
                    opacity,
                    mask,
                    svg_clip,
                }
            }
            _ => Layer {
                pixmap: None,
                direct: false,
                origin: (0, 0),
                opacity,
                mask,
                svg_clip,
            },
        };
        self.layers.push(layer);
    }

    fn pop_layer(&mut self, images: &dyn ImageSource) {
        self.mask = None;
        let Some(layer) = self.layers.pop() else {
            return;
        };
        let Some(mut pixmap) = layer.pixmap else {
            return;
        };
        let mut visible = match &layer.mask {
            Some(layers) => self.apply_mask(&mut pixmap, layer.origin, layers, images),
            None => true,
        };
        if let Some((clip, transform)) = &layer.svg_clip {
            visible = visible && self.apply_svg_clip(&mut pixmap, layer.origin, clip, transform);
        }
        self.release(&pixmap);
        let Some(parent) = self.surface_rect().filter(|_| visible) else {
            return;
        };
        let x = layer.origin.0 - parent.x as i32;
        let y = layer.origin.1 - parent.y as i32;
        let paint = PixmapPaint {
            opacity: layer.opacity,
            ..PixmapPaint::default()
        };
        if let Some(mut surface) = self.surface_mut() {
            surface.draw_pixmap(x, y, pixmap.as_ref(), &paint, Transform::identity(), None);
        }
    }

    /// Multiplies the pixels of a mask group's layer (at `origin` on the
    /// target) by the mask. The bottom layer's operator is ignored (CSS
    /// Masking 1 §7.8). Returns false if the mask is transparent
    /// everywhere or its temporary layer does not fit into the layer
    /// budget: the group then draws nothing. (Its work was charged when
    /// the group started.)
    fn apply_mask(
        &mut self,
        pixmap: &mut Pixmap,
        origin: (i32, i32),
        layers: &[MaskLayer],
        images: &dyn ImageSource,
    ) -> bool {
        let Some(size) = IntSize::from_wh(pixmap.width(), pixmap.height()) else {
            return false;
        };
        let pixels = u64::from(size.width()) * u64::from(size.height());
        // The temporary layer of each mask layer.
        if self.budget.layer_pixels + pixels > self.budget.max_layer_pixels {
            self.budget.skipped.layers = true;
            return false;
        }
        let mut result: Option<Vec<u8>> = None;
        for layer in layers.iter().rev() {
            let source = match self.mask_layer_pixels(layer, origin, size, images) {
                LayerPixels::Transparent => None,
                LayerPixels::Pixels(p) => Some(mask::coverage(p.data(), layer.luminance)),
                LayerPixels::OverBudget => return false,
            };
            result = Some(match result {
                None => source.unwrap_or_else(|| vec![0; pixmap.data().len() / 4]),
                Some(mut destination) => {
                    mask::composite(layer.composite, source.as_deref(), &mut destination);
                    destination
                }
            });
        }
        let Some(values) = result.filter(|v| v.iter().any(|&a| a != 0)) else {
            return false;
        };
        let Some(coverage) = Mask::from_vec(values, size) else {
            return false;
        };
        pixmap.apply_mask(&coverage);
        true
    }

    /// Renders a mask layer into a layer of `size` at `origin` (target
    /// device px).
    fn mask_layer_pixels(
        &mut self,
        layer: &MaskLayer,
        origin: (i32, i32),
        size: IntSize,
        images: &dyn ImageSource,
    ) -> LayerPixels {
        if matches!(layer.image, MaskLayerImage::Empty) {
            return LayerPixels::Transparent;
        }
        // Drawing goes to the innermost layer: push a temporary one.
        let Some(pixmap) = self.layer_pixmap(size.width(), size.height()) else {
            return LayerPixels::OverBudget;
        };
        self.layers.push(Layer {
            pixmap: Some(pixmap),
            direct: false,
            origin,
            opacity: 1.0,
            mask: None,
            svg_clip: None,
        });
        self.mask = None;
        match &layer.image {
            MaskLayerImage::Empty => {}
            // White: opaque for alpha and for luminance.
            MaskLayerImage::Opaque(area) => {
                self.fill_rect(*area, &[(0.0, 0.0); 4], Rgba::WHITE);
            }
            MaskLayerImage::Image { image, area, tile } => {
                if let Some(decoded) = images.image(image) {
                    self.decoded_image(decoded, images.vector_cache(), *area, *tile, *area);
                }
            }
            MaskLayerImage::Gradient {
                gradient,
                current_color,
                area,
                tile,
            } => self.gradient_tiles(gradient, *current_color, *area, *tile),
        }
        self.mask = None;
        let Some(pixmap) = self.layers.pop().and_then(|l| l.pixmap) else {
            return LayerPixels::OverBudget;
        };
        self.release(&pixmap);
        LayerPixels::Pixels(pixmap)
    }

    /// Adds `pixels` to the mask work of this strip (see
    /// [`MAX_MASK_PIXELS`]).
    fn add_mask_work(&mut self, pixels: u64) {
        self.budget.mask_pixels = self.budget.mask_pixels.saturating_add(pixels);
    }

    /// The work of a mask group whose layer is `area` (device px), for the
    /// mask budget: for each mask layer, the layer's pixels and rows, and
    /// for gradients the tiles that they render or draw (the same plan as
    /// `gradient_tiles`, which then charges nothing).
    fn mask_work(&self, area: Rect, layers: &[MaskLayer]) -> u64 {
        let pixels = (area.width as u64).saturating_mul(area.height as u64);
        let rows = (area.height as u64).saturating_mul(ROW_COST_PIXELS);
        layers
            .iter()
            .map(|layer| {
                let tiles = match &layer.image {
                    MaskLayerImage::Gradient {
                        area: layer_area,
                        tile,
                        ..
                    } => area
                        .intersection(&self.to_device(*layer_area))
                        .map_or(0, |visible| {
                            self.tile_plan(*layer_area, *tile, visible).work()
                        }),
                    _ => 0,
                };
                pixels.saturating_add(rows).saturating_add(tiles)
            })
            .fold(0, u64::saturating_add)
    }

    /// True if `work` fits into the rest of the mask budget. A mask group
    /// whose work does not fit draws nothing.
    fn mask_work_fits(&mut self, work: u64) -> bool {
        let fits = self.budget.mask_pixels.saturating_add(work) <= self.budget.max_mask_pixels;
        if !fits {
            self.budget.skipped.masks = true;
        }
        fits
    }

    /// A new transparent pixmap for a layer, if it fits into the budget of
    /// layer pixels.
    fn layer_pixmap(&mut self, width: u32, height: u32) -> Option<Pixmap> {
        let pixels = u64::from(width) * u64::from(height);
        if self.budget.layer_pixels + pixels > self.budget.max_layer_pixels {
            self.budget.skipped.layers = true;
            return None;
        }
        let pixmap = Pixmap::new(width, height)?;
        self.budget.layer_pixels += pixels;
        Some(pixmap)
    }

    /// Returns the pixels of a layer's pixmap to the budget.
    fn release(&mut self, pixmap: &Pixmap) {
        let pixels = u64::from(pixmap.width()) * u64::from(pixmap.height());
        self.budget.layer_pixels = self.budget.layer_pixels.saturating_sub(pixels);
    }

    // ----- Rectangles and borders -----

    fn fill_rect(&mut self, rect: Rect, radii: &Radii, color: Rgba) {
        if color.is_transparent() {
            return;
        }
        let device = self.to_device(rect);
        if has_radius(radii) {
            let radii = scale_radii(radii, self.params.scale);
            let Some(path) = rounded_rect_path(device, &radii) else {
                return;
            };
            let paint = solid_paint(color, true);
            self.draw(device, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::Winding, t, m);
            });
        } else {
            let snapped = snap(device);
            let Some(r) = skia_rect(snapped) else {
                return;
            };
            let paint = solid_paint(color, false);
            self.draw(snapped, |p, t, m| p.fill_rect(r, &paint, t, m));
        }
    }

    fn border(
        &mut self,
        rect: Rect,
        widths: [f32; 4],
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
        radii: &Radii,
    ) {
        let s = self.params.scale;
        let geometry = BorderGeometry::new(
            self.to_device(rect),
            widths.map(|w| w.max(0.0) * s),
            scale_radii(radii, s),
        );
        let has_width = |i: usize| widths[i] > 0.0 && !styles[i].has_no_width();
        let first = (0..4).find(|&i| has_width(i));
        let Some(first) = first else {
            return;
        };
        let uniform = (0..4)
            .filter(|&i| has_width(i))
            .all(|i| colors[i] == colors[first] && styles[i] == styles[first]);
        if uniform && matches!(styles[first], BorderStyle::Solid | BorderStyle::Double) {
            self.uniform_border(&geometry, colors[first], styles[first]);
        } else {
            self.border_sides(&geometry, colors, styles);
        }
    }

    /// A border whose sides have one color and a solid or double style:
    /// one ring, or two rings for `double`.
    fn uniform_border(&mut self, geometry: &BorderGeometry, color: Rgba, style: BorderStyle) {
        if color.is_transparent() {
            return;
        }
        // Like browsers, draw `double` as solid if the lines would be
        // thinner than one pixel.
        let thinnest = geometry
            .widths
            .iter()
            .copied()
            .filter(|w| *w > 0.0)
            .fold(f32::INFINITY, f32::min);
        let bands: &[(f32, f32)] = if style == BorderStyle::Double && thinnest >= 3.0 {
            &[(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0)]
        } else {
            &[(0.0, 1.0)]
        };
        let paint = solid_paint(color, true);
        for &(from, to) in bands {
            let Some(path) = geometry.ring(from, to) else {
                continue;
            };
            self.draw(geometry.outer, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::EvenOdd, t, m);
            });
        }
    }

    /// A border with different sides: each side is painted separately, with
    /// the 3D shading of `inset`, `outset`, `groove` and `ridge`. `double`
    /// is painted as solid here.
    fn border_sides(
        &mut self,
        geometry: &BorderGeometry,
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
    ) {
        // Whole sides, the outer halves of grooves and ridges, and their
        // inner halves.
        let mut whole = [None; 4];
        let mut outer_half = [None; 4];
        let mut inner_half = [None; 4];
        for side in 0..4 {
            let (color, style) = (colors[side], styles[side]);
            if geometry.widths[side] <= 0.0 || color.is_transparent() || style.has_no_width() {
                continue;
            }
            match style {
                BorderStyle::Dashed | BorderStyle::Dotted => {
                    self.dashed_side(geometry, side, color, style);
                }
                BorderStyle::Groove => {
                    outer_half[side] = Some(shade(color, BorderStyle::Inset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Outset, side));
                }
                BorderStyle::Ridge => {
                    outer_half[side] = Some(shade(color, BorderStyle::Outset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Inset, side));
                }
                _ => whole[side] = Some(shade(color, style, side)),
            }
        }
        self.fill_sides(geometry, (0.0, 1.0), whole);
        self.fill_sides(geometry, (0.0, 0.5), outer_half);
        self.fill_sides(geometry, (0.5, 1.0), inner_half);
    }

    /// Fills the band `from..to` (fractions of the border width, 0 is the
    /// outer edge) of the given sides (see [`side_polygons`]). With rounded
    /// corners, the sides are drawn into a temporary layer and cut to the
    /// rounded ring.
    fn fill_sides(
        &mut self,
        geometry: &BorderGeometry,
        band: (f32, f32),
        colors: [Option<Rgba>; 4],
    ) {
        if colors.iter().all(Option::is_none) {
            return;
        }
        let (outer, _) = geometry.edge(band.0);
        let (inner, _) = geometry.edge(band.1);
        let painted = colors.map(|c| c.is_some());
        let polygons = side_polygons(outer, inner, geometry.has_radius, painted);
        // Sides of the same color are filled as one path, so that there is
        // no anti-aliasing seam on the diagonal between them.
        let mut by_color: Vec<(Rgba, PathBuilder)> = Vec::new();
        for (polygon, color) in polygons.iter().zip(colors) {
            let Some(color) = color else {
                continue;
            };
            let index = by_color
                .iter()
                .position(|(c, _)| *c == color)
                .unwrap_or_else(|| {
                    by_color.push((color, PathBuilder::new()));
                    by_color.len() - 1
                });
            push_polygon(&mut by_color[index].1, polygon);
        }
        let sides: Vec<(Path, Paint<'static>)> = by_color
            .into_iter()
            .filter_map(|(color, pb)| Some((pb.finish()?, solid_paint(color, true))))
            .collect();
        if !geometry.has_radius {
            for (path, paint) in &sides {
                self.draw(outer, |p, t, m| {
                    p.fill_path(path, paint, FillRule::Winding, t, m);
                });
            }
            return;
        }
        let Some(ring) = geometry.ring(band.0, band.1) else {
            return;
        };
        let Some(area) = self
            .visible_area()
            .and_then(|v| snap_out(outer).intersection(&v))
        else {
            return;
        };
        let (Some(mut layer), Some(mut ring_mask)) = (
            Pixmap::new(area.width as u32, area.height as u32),
            Mask::new(area.width as u32, area.height as u32),
        ) else {
            return;
        };
        let to_layer = Transform::from_translate(-area.x, -area.y);
        for (path, paint) in &sides {
            layer.fill_path(path, paint, FillRule::Winding, to_layer, None);
        }
        ring_mask.fill_path(&ring, FillRule::EvenOdd, true, to_layer);
        layer.apply_mask(&ring_mask);
        self.draw(area, |p, t, m| {
            p.draw_pixmap(
                area.x as i32,
                area.y as i32,
                layer.as_ref(),
                &PixmapPaint::default(),
                t,
                m,
            );
        });
    }

    /// A dashed or dotted side: a stroke along the middle of the side.
    /// Rounded corners are not followed.
    fn dashed_side(
        &mut self,
        geometry: &BorderGeometry,
        side: usize,
        color: Rgba,
        style: BorderStyle,
    ) {
        let rect = geometry.outer;
        let width = geometry.widths[side];
        let half = width / 2.0;
        let (x0, y0, x1, y1) = match side {
            0 => (rect.x, rect.y + half, rect.right(), rect.y + half),
            1 => (
                rect.right() - half,
                rect.y,
                rect.right() - half,
                rect.bottom(),
            ),
            2 => (
                rect.right(),
                rect.bottom() - half,
                rect.x,
                rect.bottom() - half,
            ),
            _ => (rect.x + half, rect.bottom(), rect.x + half, rect.y),
        };
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        let Some(path) = pb.finish() else {
            return;
        };
        let dash = if style == BorderStyle::Dotted {
            StrokeDash::new(vec![width, width], 0.0)
        } else {
            StrokeDash::new(vec![width * 3.0, width * 3.0], 0.0)
        };
        let stroke = Stroke {
            width,
            dash,
            ..Stroke::default()
        };
        let paint = solid_paint(color, true);
        self.draw(rect, |p, t, m| p.stroke_path(&path, &paint, &stroke, t, m));
    }

    // ----- Text -----

    fn text(
        &mut self,
        origin: Point,
        font: swb_text::FontId,
        size: f32,
        glyphs: &[swb_layout::PositionedGlyph],
        color: Rgba,
        fonts: &mut FontContext,
    ) {
        let (Some(surface), Some(visible)) = (self.surface_rect(), self.visible_area()) else {
            return;
        };
        let scale = self.params.scale;
        let device_size = size * scale;
        let start = self.to_device_point(origin);
        let baseline_y = start.y.round();
        // The clip in surface px.
        let clip = Rect::new(
            visible.x - surface.x,
            visible.y - surface.y,
            visible.width,
            visible.height,
        );
        let Some(mut pixmap) = self.surface_mut() else {
            return;
        };
        for g in glyphs {
            let x = start.x + g.x * scale;
            let y = baseline_y + (g.y * scale).round();
            // Skip glyphs that cannot reach the visible area (the same
            // margins as `DisplayItem::bounds`) before rasterizing them.
            if x - device_size > visible.right()
                || x + 4.0 * device_size < visible.x
                || y - 3.0 * device_size > visible.bottom()
                || y + 2.0 * device_size < visible.y
            {
                continue;
            }
            let (pixel_x, subpixel) = subpixel_position(x);
            let Some(mask) = fonts.glyph_mask(font, g.id, device_size, subpixel) else {
                continue;
            };
            let left = (pixel_x as i64).saturating_add(i64::from(mask.left)) - surface.x as i64;
            let top = (y as i64).saturating_add(i64::from(mask.top)) - surface.y as i64;
            let alpha = AlphaMask {
                data: &mask.data,
                width: mask.width,
                height: mask.height,
            };
            blit_mask(&mut pixmap, &alpha, (left, top), color, clip);
        }
    }

    // ----- Images and gradients -----

    /// Draws a raster image, or a vector image rendered for the device
    /// pixel size of `tile`.
    fn decoded_image(
        &mut self,
        image: &DecodedImage,
        cache: Option<&VectorCache>,
        area: Rect,
        tile: Rect,
        clip: Rect,
    ) {
        let svg = match image.kind() {
            ImageKind::Raster(pixmap) => return self.image(pixmap, area, tile, clip),
            ImageKind::Vector(svg) => svg,
        };
        let Some(size) = vector_render_size(self.to_device(tile)) else {
            return;
        };
        let concrete = (tile.width, tile.height);
        let pixmap = match cache {
            Some(cache) => cache.get(svg, size, concrete, self.vectors),
            None if self.vectors.take(svg.render_work(size, concrete)) => {
                svg.render(size, concrete).map(Arc::new)
            }
            None => None,
        };
        if let Some(pixmap) = pixmap {
            self.image(&pixmap, area, tile, clip);
        }
    }

    fn image(&mut self, pixmap: &Pixmap, area: Rect, tile: Rect, clip: Rect) {
        if tile.width <= 0.0 || tile.height <= 0.0 {
            return;
        }
        let Some(area) = area.intersection(&clip) else {
            return;
        };
        let tiled = is_tiled(area, tile);
        let tile = self.to_device(tile);
        let area = snap(self.to_device(area));
        let Some(r) = skia_rect(area) else {
            return;
        };
        let sx = tile.width / pixmap.width() as f32;
        let sy = tile.height / pixmap.height() as f32;
        let quality = if (sx - 1.0).abs() < 0.01 && (sy - 1.0).abs() < 0.01 {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bilinear
        };
        // A single image clamps at its edges; repeating one would blend
        // the opposite edge into the border pixels.
        let spread = if tiled {
            SpreadMode::Repeat
        } else {
            SpreadMode::Pad
        };
        let paint = Paint {
            shader: Pattern::new(
                pixmap.as_ref(),
                spread,
                quality,
                1.0,
                Transform::from_row(sx, 0.0, 0.0, sy, tile.x, tile.y),
            ),
            anti_alias: false,
            ..Paint::default()
        };
        self.draw(area, |p, t, m| p.fill_rect(r, &paint, t, m));
    }

    /// Fills the part of `rect` inside `clip` with a linear gradient. The
    /// gradient line goes through the center of `rect` at the gradient
    /// angle, with the length that CSS Images 3 §3.1.1 defines
    /// (<https://www.w3.org/TR/css-images-3/#linear-gradient-syntax>).
    fn linear_gradient(
        &mut self,
        rect: Rect,
        clip: Rect,
        gradient: &swb_style::LinearGradient,
        current: Rgba,
    ) {
        let n = gradient.stops.len();
        let Some(fill) = rect.intersection(&clip) else {
            return;
        };
        if n == 0 {
            return;
        }
        let (start, end, line_len) = gradient_line(rect, gradient.angle_deg);
        let start = self.to_device_point(start);
        let end = self.to_device_point(end);
        let shader = gradient_shader(
            gradient,
            current,
            line_len,
            (start, end),
            Transform::identity(),
        );
        let Some(shader) = shader else {
            // A single stop or degenerate line: fill with the first color.
            if let Some((c, _)) = gradient.stops.first() {
                self.fill_rect(fill, &[(0.0, 0.0); 4], c.resolve(current));
            }
            return;
        };
        let area = snap(self.to_device(fill));
        let Some(r) = skia_rect(area) else {
            return;
        };
        let paint = Paint {
            shader,
            anti_alias: false,
            ..Paint::default()
        };
        self.draw(area, |p, t, m| p.fill_rect(r, &paint, t, m));
    }

    /// Fills `area` with tiles of a linear gradient: each tile contains
    /// the whole gradient (CSS Images 3 §3.1). A few visible tiles are
    /// drawn one by one; more are rendered once and drawn like an image
    /// (charged to the mask work).
    fn gradient_tiles(
        &mut self,
        gradient: &swb_style::LinearGradient,
        current: Rgba,
        area: Rect,
        tile: Rect,
    ) {
        let Some(visible) = self
            .visible_area()
            .and_then(|v| v.intersection(&self.to_device(area)))
        else {
            return;
        };
        match self.tile_plan(area, tile, visible) {
            TilePlan::Copies { columns, rows, .. } => {
                for row in rows.0..rows.1 {
                    for column in columns.0..columns.1 {
                        let copy = Rect::new(
                            tile.x + column as f32 * tile.width,
                            tile.y + row as f32 * tile.height,
                            tile.width,
                            tile.height,
                        );
                        if let Some(part) = copy.intersection(&area) {
                            self.linear_gradient(copy, part, gradient, current);
                        }
                    }
                }
            }
            TilePlan::Rendered { size, .. } => {
                if let Some(pixmap) = gradient_tile(gradient, current, tile, size) {
                    self.image(&pixmap, area, tile, area);
                }
            }
            TilePlan::Nothing => {}
        }
    }

    /// How a gradient layer with `tile`s in `area` (CSS px) is drawn where
    /// `visible` (device px) shows it: at most [`MAX_GRADIENT_TILES`]
    /// visible tiles one by one (exact, also for hard stops), else one
    /// rendered tile repeated as an image. The rendered tile has at most
    /// four times the visible pixels, but may always have 1 Mpx (a tile
    /// can be much longer than the area in one direction, and most of the
    /// area can be outside the layer); a smaller rendering is scaled up.
    fn tile_plan(&self, area: Rect, tile: Rect, visible: Rect) -> TilePlan {
        let device_tile = self.to_device(tile);
        if !is_tiled(area, tile) {
            return TilePlan::Copies {
                columns: (0, 1),
                rows: (0, 1),
                visible_rows: visible.height,
            };
        }
        // The visible tiles, by column and row (saturating: NaN gives 0).
        let index = |v: f32| v as i64;
        let columns = (
            index(((visible.x - device_tile.x) / device_tile.width).floor()),
            index(((visible.right() - device_tile.x) / device_tile.width).ceil()),
        );
        let rows = (
            index(((visible.y - device_tile.y) / device_tile.height).floor()),
            index(((visible.bottom() - device_tile.y) / device_tile.height).ceil()),
        );
        let (column_count, row_count) = (
            columns.1.saturating_sub(columns.0),
            rows.1.saturating_sub(rows.0),
        );
        if column_count <= 0 || row_count <= 0 {
            return TilePlan::Nothing;
        }
        if column_count.saturating_mul(row_count) <= MAX_GRADIENT_TILES {
            return TilePlan::Copies {
                columns,
                rows,
                visible_rows: visible.height,
            };
        }
        let floor = (device_tile.width * device_tile.height).min(1024.0 * 1024.0);
        let max_pixels = (4.0 * visible.width * visible.height)
            .max(floor)
            .clamp(1.0, MAX_RENDER_PIXELS as f32) as u32;
        match render_size(device_tile, max_pixels) {
            Some(size) => TilePlan::Rendered {
                size,
                visible_rows: visible.height,
            },
            None => TilePlan::Nothing,
        }
    }
}

/// How a gradient mask layer is drawn (see [`Rasterizer::tile_plan`]).
enum TilePlan {
    /// One gradient per visible tile: the columns and rows (end
    /// exclusive) relative to the first tile.
    Copies {
        columns: (i64, i64),
        rows: (i64, i64),
        /// The visible rows (device px).
        visible_rows: f32,
    },
    /// One tile rendered at `size` and repeated.
    Rendered { size: IntSize, visible_rows: f32 },
    /// Nothing to draw.
    Nothing,
}

impl TilePlan {
    /// The work beyond the layer's pixels, for the mask budget: the rows
    /// of each drawn copy, or the rendered tile (pixels and rows) and the
    /// rows of the image drawing.
    fn work(&self) -> u64 {
        let rows = |r: f32| (r.max(0.0) as u64).saturating_mul(ROW_COST_PIXELS);
        match self {
            TilePlan::Copies {
                columns,
                rows: copy_rows,
                visible_rows,
            } => {
                let copies = columns
                    .1
                    .saturating_sub(columns.0)
                    .saturating_mul(copy_rows.1.saturating_sub(copy_rows.0));
                rows(*visible_rows).saturating_mul(u64::try_from(copies).unwrap_or(0))
            }
            TilePlan::Rendered { size, visible_rows } => {
                let tile = u64::from(size.height())
                    .saturating_mul(u64::from(size.width()).saturating_add(ROW_COST_PIXELS));
                tile.saturating_add(rows(*visible_rows))
            }
            TilePlan::Nothing => 0,
        }
    }
}

/// A rendered mask layer.
enum LayerPixels {
    /// Transparent black everywhere.
    Transparent,
    /// The rendered layer.
    Pixels(Pixmap),
    /// Not rendered: the temporary layer did not fit into the layer
    /// budget.
    OverBudget,
}

/// True if `area` extends past `tile`, so that the tile repeats.
fn is_tiled(area: Rect, tile: Rect) -> bool {
    // How far the area may extend past the tile and still count as one
    // tile (CSS px).
    const EPSILON: f32 = 0.01;
    area.x < tile.x - EPSILON
        || area.y < tile.y - EPSILON
        || area.right() > tile.right() + EPSILON
        || area.bottom() > tile.bottom() + EPSILON
}

/// The gradient line of a linear gradient whose box is `rect`: its start
/// and end points and its length. The line goes through the center of
/// `rect` at the gradient angle, with the length that CSS Images 3 §3.1.1
/// defines (<https://www.w3.org/TR/css-images-3/#linear-gradient-syntax>).
fn gradient_line(rect: Rect, angle_deg: f32) -> (Point, Point, f32) {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let half_len = f32::midpoint(rect.width * sin.abs(), rect.height * cos.abs());
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    (
        Point::new(cx - sin * half_len, cy + cos * half_len),
        Point::new(cx + sin * half_len, cy - cos * half_len),
        half_len * 2.0,
    )
}

/// The shader of a linear gradient whose gradient line goes from
/// `line.0` to `line.1` (mapped by `transform` to device px) and is
/// `line_len` CSS px long; `None` for a degenerate gradient (a single stop
/// or a zero-length line).
fn gradient_shader(
    gradient: &swb_style::LinearGradient,
    current: Rgba,
    line_len: f32,
    (start, end): (Point, Point),
    transform: Transform,
) -> Option<Shader<'static>> {
    let n = gradient.stops.len();
    let stops: Vec<GradientStop> = gradient
        .stops
        .iter()
        .enumerate()
        .map(|(i, (color, pos))| {
            let t = pos.as_ref().map_or_else(
                || {
                    if n > 1 {
                        i as f32 / (n - 1) as f32
                    } else {
                        0.0
                    }
                },
                |p| {
                    if line_len > 0.0 {
                        p.resolve(line_len) / line_len
                    } else {
                        0.0
                    }
                },
            );
            let c = color.resolve(current);
            GradientStop::new(t.clamp(0.0, 1.0), to_skia_color(c))
        })
        .collect();
    let spread = if gradient.repeating {
        SpreadMode::Repeat
    } else {
        SpreadMode::Pad
    };
    LinearGradient::new(
        tiny_skia::Point::from_xy(start.x, start.y),
        tiny_skia::Point::from_xy(end.x, end.y),
        stops,
        spread,
        transform,
    )
}

/// Renders a linear gradient whose box is `tile` (CSS px) into a pixmap of
/// `size`.
fn gradient_tile(
    gradient: &swb_style::LinearGradient,
    current: Rgba,
    tile: Rect,
    size: IntSize,
) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(size.width(), size.height())?;
    let (width, height) = (size.width() as f32, size.height() as f32);
    // The gradient is computed in CSS px and scaled to the pixmap, so its
    // angle stays right when the pixmap has other proportions.
    let to_pixmap = Transform::from_scale(width / tile.width, height / tile.height);
    let (start, end, line_len) = gradient_line(
        Rect::new(0.0, 0.0, tile.width, tile.height),
        gradient.angle_deg,
    );
    let paint = match gradient_shader(gradient, current, line_len, (start, end), to_pixmap) {
        Some(shader) => Paint {
            shader,
            anti_alias: false,
            ..Paint::default()
        },
        None => solid_paint(gradient.stops.first()?.0.resolve(current), false),
    };
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, width, height)?;
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    Some(pixmap)
}

/// The geometry of a border box in device px.
struct BorderGeometry {
    /// The outer edge, snapped to pixels if there are no rounded corners.
    outer: Rect,
    /// Widths: top, right, bottom, left.
    widths: [f32; 4],
    /// Outer corner radii.
    radii: Radii,
    has_radius: bool,
}

impl BorderGeometry {
    fn new(outer: Rect, widths: [f32; 4], radii: Radii) -> Self {
        let has_radius = has_radius(&radii);
        BorderGeometry {
            outer: if has_radius { outer } else { snap(outer) },
            widths,
            radii,
            has_radius,
        }
    }

    /// The edge at `fraction` of the border width from the outer edge (0
    /// is the outer edge, 1 the inner edge), with its corner radii.
    fn edge(&self, fraction: f32) -> (Rect, Radii) {
        let [top, right, bottom, left] = self.widths.map(|w| w * fraction);
        let o = self.outer;
        let rect = Rect::new(
            o.x + left,
            o.y + top,
            (o.width - left - right).max(0.0),
            (o.height - top - bottom).max(0.0),
        );
        if !self.has_radius {
            return (snap(rect), [(0.0, 0.0); 4]);
        }
        let r = self.radii;
        let radii = [
            ((r[0].0 - left).max(0.0), (r[0].1 - top).max(0.0)),
            ((r[1].0 - right).max(0.0), (r[1].1 - top).max(0.0)),
            ((r[2].0 - right).max(0.0), (r[2].1 - bottom).max(0.0)),
            ((r[3].0 - left).max(0.0), (r[3].1 - bottom).max(0.0)),
        ];
        (rect, radii)
    }

    /// The area between the edges at `from` and `to` (see [`Self::edge`]),
    /// filled with the even-odd rule.
    fn ring(&self, from: f32, to: f32) -> Option<Path> {
        let (outer, outer_radii) = self.edge(from);
        let (inner, inner_radii) = self.edge(to);
        let mut pb = PathBuilder::new();
        push_rounded_rect(&mut pb, outer, &outer_radii);
        push_rounded_rect(&mut pb, inner, &inner_radii);
        pb.finish()
    }
}

/// The area of each side (top, right, bottom, left) between the outer and
/// inner border edges, split at the lines through the corners. Without
/// rounded corners the sides are trapezoids. With rounded corners the
/// sides are extended to the center of the box, so that together they
/// cover the whole rounded ring, including the curved corner areas inside
/// the inner rectangle. A corner next to a side that is not painted
/// (`painted[i]` is false) belongs entirely to the painted side, so its
/// tapering end is drawn as in browsers.
fn side_polygons(
    outer: Rect,
    inner: Rect,
    to_center: bool,
    painted: [bool; 4],
) -> [Vec<(f32, f32)>; 4] {
    let outer_corners = [
        (outer.x, outer.y),
        (outer.right(), outer.y),
        (outer.right(), outer.bottom()),
        (outer.x, outer.bottom()),
    ];
    let inner_corners = [
        (inner.x, inner.y),
        (inner.right(), inner.y),
        (inner.right(), inner.bottom()),
        (inner.x, inner.bottom()),
    ];
    if !to_center {
        return [0, 1, 2, 3].map(|side| {
            let (start, end) = (side, (side + 1) % 4);
            vec![
                outer_corners[start],
                outer_corners[end],
                inner_corners[end],
                inner_corners[start],
            ]
        });
    }
    let center = (
        f32::midpoint(inner.x, inner.right()),
        f32::midpoint(inner.y, inner.bottom()),
    );
    // The point where `side` ends at `corner` (`side` or `side + 1`): the
    // inner corner, or, if the other side at that corner is not painted,
    // the point of the outer edge level with the center.
    let corner_end = |side: usize, corner: usize| {
        let other = if corner == side {
            (side + 3) % 4
        } else {
            (side + 1) % 4
        };
        if painted[other] {
            inner_corners[corner]
        } else if side.is_multiple_of(2) {
            // Top and bottom sides extend along the left/right edge.
            (outer_corners[corner].0, center.1)
        } else {
            // Left and right sides extend along the top/bottom edge.
            (center.0, outer_corners[corner].1)
        }
    };
    [0, 1, 2, 3].map(|side| {
        let (start, end) = (side, (side + 1) % 4);
        vec![
            outer_corners[start],
            outer_corners[end],
            corner_end(side, end),
            center,
            corner_end(side, start),
        ]
    })
}

/// Appends a closed polygon to a path.
fn push_polygon(pb: &mut PathBuilder, points: &[(f32, f32)]) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
}

/// The pixel size to render a vector image at for a tile of `device` px:
/// the tile size, rounded. A size above [`MAX_RENDER_PIXELS`] or
/// [`MAX_DIMENSION`] is reduced, keeping its proportions; the drawing then
/// scales the rendering up. `None` for an empty tile.
fn vector_render_size(device: Rect) -> Option<IntSize> {
    render_size(device, MAX_RENDER_PIXELS)
}

/// [`vector_render_size`] with at most `max_pixels` pixels.
fn render_size(device: Rect, max_pixels: u32) -> Option<IntSize> {
    let valid = |v: f32| v > 0.0 && v.is_finite();
    if !(valid(device.width) && valid(device.height)) {
        return None;
    }
    // f64: the product of two large f32 sizes overflows f32.
    let mut width = f64::from(device.width).round().max(1.0);
    let mut height = f64::from(device.height).round().max(1.0);
    let max_dimension = f64::from(MAX_DIMENSION);
    let reduction = (f64::from(max_pixels) / (width * height))
        .sqrt()
        .min(max_dimension / width)
        .min(max_dimension / height);
    if reduction < 1.0 {
        width = (width * reduction).floor().max(1.0);
        height = (height * reduction).floor().max(1.0);
    }
    IntSize::from_wh(width as u32, height as u32)
}

/// A mask that covers `clip` (target device px) on a surface at `surface`.
fn clip_mask(clip: Rect, surface: Rect) -> Option<Mask> {
    let mut mask = Mask::new(surface.width as u32, surface.height as u32)?;
    if let Some(rect) = tiny_skia::Rect::from_xywh(
        clip.x - surface.x,
        clip.y - surface.y,
        clip.width,
        clip.height,
    ) {
        mask.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
    }
    Some(mask)
}

/// True if `outer` contains all of `inner`.
fn contains(outer: &Rect, inner: &Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

/// Rounds the edges of a rectangle to whole pixels. A rectangle that is
/// not empty stays at least one pixel wide and high.
fn snap(r: Rect) -> Rect {
    let snap_axis = |start: f32, size: f32| {
        let a = start.round();
        let mut b = (start + size).round();
        if size > 0.0 && b <= a {
            b = a + 1.0;
        }
        (a, (b - a).max(0.0))
    };
    let (x, width) = snap_axis(r.x, r.width);
    let (y, height) = snap_axis(r.y, r.height);
    Rect::new(x, y, width, height)
}

/// The smallest rectangle with whole-pixel edges that contains `r`.
fn snap_out(r: Rect) -> Rect {
    let x = r.x.floor();
    let y = r.y.floor();
    Rect::new(x, y, r.right().ceil() - x, r.bottom().ceil() - y)
}

fn skia_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height)
}

/// The pixel column and the subpixel offset in quarter pixels (0 to 3) of
/// a glyph at device x position `x`.
fn subpixel_position(x: f32) -> (f32, u8) {
    let floor = x.floor();
    let quarters = ((x - floor) * 4.0).round();
    if quarters >= 4.0 {
        (floor + 1.0, 0)
    } else {
        (floor, quarters as u8)
    }
}

fn has_radius(radii: &Radii) -> bool {
    radii.iter().any(|(h, v)| *h > 0.0 || *v > 0.0)
}

fn scale_radii(radii: &Radii, s: f32) -> Radii {
    radii.map(|(h, v)| (h * s, v * s))
}

fn to_skia_color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn solid_paint(c: Rgba, anti_alias: bool) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(to_skia_color(c)),
        anti_alias,
        ..Paint::default()
    }
}

/// Shading for 3D border styles, as browsers do it: the top and left sides
/// of `inset`/`groove` are darker, the others lighter (and the reverse for
/// `outset`/`ridge`).
fn shade(c: Rgba, style: BorderStyle, side: usize) -> Rgba {
    let top_left = side == 0 || side == 3;
    let darken = match style {
        BorderStyle::Inset | BorderStyle::Groove => top_left,
        BorderStyle::Outset | BorderStyle::Ridge => !top_left,
        _ => return c,
    };
    if darken {
        Rgba::new(c.r / 3 * 2, c.g / 3 * 2, c.b / 3 * 2, c.a)
    } else {
        c
    }
}

fn rounded_rect_path(rect: Rect, radii: &Radii) -> Option<Path> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    push_rounded_rect(&mut pb, rect, radii);
    pb.finish()
}

/// Appends a rectangle with elliptical corners to `pb`.
fn push_rounded_rect(pb: &mut PathBuilder, r: Rect, radii: &Radii) {
    // Control point factor for a quarter ellipse with cubic Béziers.
    const K: f32 = 0.552_284_8;
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let [(tlx, tly), (trx, try_), (brx, bry), (blx, bly)] = *radii;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    pb.move_to(x0 + tlx, y0);
    pb.line_to(x1 - trx, y0);
    if trx > 0.0 || try_ > 0.0 {
        pb.cubic_to(
            x1 - trx + trx * K,
            y0,
            x1,
            y0 + try_ - try_ * K,
            x1,
            y0 + try_,
        );
    }
    pb.line_to(x1, y1 - bry);
    if brx > 0.0 || bry > 0.0 {
        pb.cubic_to(x1, y1 - bry + bry * K, x1 - brx + brx * K, y1, x1 - brx, y1);
    }
    pb.line_to(x0 + blx, y1);
    if blx > 0.0 || bly > 0.0 {
        pb.cubic_to(x0 + blx - blx * K, y1, x0, y1 - bly + bly * K, x0, y1 - bly);
    }
    pb.line_to(x0, y0 + tly);
    if tlx > 0.0 || tly > 0.0 {
        pb.cubic_to(x0, y0 + tly - tly * K, x0 + tlx - tlx * K, y0, x0 + tlx, y0);
    }
    pb.close();
}

/// An alpha mask: one coverage byte per pixel, row by row.
struct AlphaMask<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
}

/// Blends `mask` in `color` into a premultiplied RGBA pixmap, with the
/// mask's top-left corner at `origin` (pixmap px), clipped to `clip`.
fn blit_mask(
    pixmap: &mut PixmapMut<'_>,
    mask: &AlphaMask<'_>,
    origin: (i64, i64),
    color: Rgba,
    clip: Rect,
) {
    let (left, top) = origin;
    let width = i64::from(pixmap.width());
    let height = i64::from(pixmap.height());
    let x_min = left.max(clip.x.floor() as i64).max(0);
    let y_min = top.max(clip.y.floor() as i64).max(0);
    let x_max = left
        .saturating_add(i64::from(mask.width))
        .min(clip.right().ceil() as i64)
        .min(width);
    let y_max = top
        .saturating_add(i64::from(mask.height))
        .min(clip.bottom().ceil() as i64)
        .min(height);
    if x_min >= x_max || y_min >= y_max {
        return;
    }
    let alpha = u32::from(color.a);
    let (red, green, blue) = (u32::from(color.r), u32::from(color.g), u32::from(color.b));
    let mask_width = mask.width as usize;
    let data = pixmap.data_mut();
    // All indices below are inside the mask and the pixmap: the ranges
    // were clamped to both above.
    for row in y_min..y_max {
        let mask_row = (row - top) as usize * mask_width;
        let pixel_row = (row * width) as usize * 4;
        for column in x_min..x_max {
            let Some(&coverage) = mask.data.get(mask_row + (column - left) as usize) else {
                continue;
            };
            let a = u32::from(coverage) * alpha / 255;
            if a == 0 {
                continue;
            }
            let i = pixel_row + column as usize * 4;
            let inverse = 255 - a;
            let blend =
                |src: u32, dst: u8| ((src * a + u32::from(dst) * inverse + 127) / 255) as u8;
            data[i] = blend(red, data[i]);
            data[i + 1] = blend(green, data[i + 1]);
            data[i + 2] = blend(blue, data[i + 2]);
            data[i + 3] = ((a * 255 + u32::from(data[i + 3]) * inverse + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_blends_and_clips() {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        let data = [255u8, 128, 0, 255];
        let mask = AlphaMask {
            data: &data,
            width: 2,
            height: 2,
        };
        let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
        blit_mask(&mut pixmap.as_mut(), &mask, (3, 3), Rgba::BLACK, clip);
        // Only the top-left mask pixel lands inside the 4x4 pixmap.
        let px = pixmap.pixel(3, 3).unwrap();
        assert_eq!((px.red(), px.alpha()), (0, 255));
        let untouched = pixmap.pixel(2, 2).unwrap();
        assert_eq!(untouched.red(), 255);
    }

    #[test]
    fn blit_far_away_does_not_overflow() {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        let mask = AlphaMask {
            data: &[255; 4],
            width: 2,
            height: 2,
        };
        let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
        blit_mask(
            &mut pixmap.as_mut(),
            &mask,
            (i64::MAX - 1, 0),
            Rgba::BLACK,
            clip,
        );
        blit_mask(
            &mut pixmap.as_mut(),
            &mask,
            (0, i64::MAX),
            Rgba::BLACK,
            clip,
        );
        assert!(pixmap.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn half_coverage_is_grey() {
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        let mask = AlphaMask {
            data: &[128],
            width: 1,
            height: 1,
        };
        blit_mask(
            &mut pixmap.as_mut(),
            &mask,
            (0, 0),
            Rgba::BLACK,
            Rect::new(0.0, 0.0, 1.0, 1.0),
        );
        let px = pixmap.pixel(0, 0).unwrap();
        assert!((126..=128).contains(&px.red()));
    }

    #[test]
    fn subpixel_positions_round_to_the_next_pixel() {
        assert_eq!(subpixel_position(10.0), (10.0, 0));
        assert_eq!(subpixel_position(10.3), (10.0, 1));
        assert_eq!(subpixel_position(10.6), (10.0, 2));
        assert_eq!(subpixel_position(10.8), (10.0, 3));
        assert_eq!(subpixel_position(10.9), (11.0, 0));
        assert_eq!(subpixel_position(-0.05), (0.0, 0));
    }

    struct NoImages;

    impl ImageSource for NoImages {
        fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
            None
        }

        fn vector_cache(&self) -> Option<&VectorCache> {
            None
        }
    }

    struct OneImage(DecodedImage, Option<VectorCache>);

    impl ImageSource for OneImage {
        fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
            Some(&self.0)
        }

        fn vector_cache(&self) -> Option<&VectorCache> {
            self.1.as_ref()
        }
    }

    const BLUE: Rgba = Rgba::rgb(0, 0, 255);
    const RED: Rgba = Rgba::rgb(255, 0, 0);

    /// Rasterizes `items` on a white 100x100 target at `scale`.
    fn render_with(items: Vec<DisplayItem>, scale: f32, images: &dyn ImageSource) -> Pixmap {
        let size = (100.0 * scale) as u32;
        let mut target = Pixmap::new(size, size).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let params = RasterParams {
            scroll: Point::default(),
            viewport_scroll: Point::default(),
            scale,
        };
        let mut fonts = FontContext::for_tests();
        rasterize(
            &DisplayList { items },
            &mut target,
            params,
            &mut fonts,
            images,
        );
        target
    }

    fn render(items: Vec<DisplayItem>) -> Pixmap {
        render_with(items, 1.0, &NoImages)
    }

    fn rgb(p: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
        let c = p.pixel(x, y).unwrap().demultiply();
        (c.red(), c.green(), c.blue())
    }

    fn border(widths: [f32; 4], colors: [Rgba; 4], style: BorderStyle, radius: f32) -> DisplayItem {
        DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 60.0),
            widths,
            colors,
            styles: [style; 4],
            radii: [(radius, radius); 4],
        }
    }

    #[test]
    fn polygons_fill_and_skip_degenerate_input() {
        let polygon = |points: &[(f32, f32)], color| DisplayItem::Polygon {
            points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
            color,
        };
        let triangle = [(10.0, 10.0), (10.0, 90.0), (90.0, 50.0)];
        let p = render(vec![polygon(&triangle, BLUE)]);
        assert_eq!(rgb(&p, 30, 50), (0, 0, 255), "inside");
        assert_eq!(rgb(&p, 80, 20), (255, 255, 255), "outside");
        // At scale 2 the same triangle covers twice the device pixels.
        let p = render_with(vec![polygon(&triangle, BLUE)], 2.0, &NoImages);
        assert_eq!(rgb(&p, 60, 100), (0, 0, 255));
        // Fewer than three points, non-finite points and transparent
        // colors draw nothing (and do not panic).
        let nothing = [
            polygon(&[(10.0, 10.0), (90.0, 90.0)], BLUE),
            polygon(&[(10.0, 10.0), (f32::NAN, 90.0), (90.0, 10.0)], BLUE),
            polygon(&[(10.0, 10.0), (f32::INFINITY, 90.0), (90.0, 10.0)], BLUE),
            polygon(&triangle, Rgba::new(0, 0, 255, 0)),
        ];
        for item in nothing {
            let p = render(vec![item]);
            assert_eq!(rgb(&p, 30, 30), (255, 255, 255));
        }
    }

    #[test]
    fn rounded_border_keeps_the_color_of_each_side() {
        // Only the bottom side has a width; the top color must not be used.
        let p = render(vec![border(
            [0.0, 0.0, 10.0, 0.0],
            [Rgba::BLACK, Rgba::BLACK, BLUE, Rgba::BLACK],
            BorderStyle::Solid,
            4.0,
        )]);
        assert_eq!(rgb(&p, 50, 65), (0, 0, 255));
        assert_eq!(rgb(&p, 50, 30), (255, 255, 255));
        // The rounded corner cuts the bottom-left pixel.
        assert_eq!(rgb(&p, 10, 69), (255, 255, 255));
    }

    #[test]
    fn rounded_border_has_no_gaps_at_the_corners() {
        // The spinner pattern: a circle with one side in another color. The
        // ring at 45° lies inside the inner rectangle's corner, outside the
        // side trapezoids.
        let item = DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 80.0),
            widths: [10.0; 4],
            colors: [BLUE, RED, RED, RED],
            styles: [BorderStyle::Solid; 4],
            radii: [(40.0, 40.0); 4],
        };
        let p = render(vec![item]);
        // A point on the ring's center line at 45° (top-left corner).
        let d = 35.0 * std::f32::consts::FRAC_1_SQRT_2;
        let (x, y) = ((50.0 - d) as u32, (50.0 - d) as u32);
        assert_ne!(
            rgb(&p, x, y),
            (255, 255, 255),
            "corner pixel ({x}, {y}) is painted"
        );
        // The bottom-right corner belongs to the right/bottom sides.
        let (x, y) = ((50.0 + d) as u32, (50.0 + d) as u32);
        assert_eq!(rgb(&p, x, y), (255, 0, 0));
    }

    #[test]
    fn rounded_corner_belongs_to_the_painted_side() {
        // Only the top side has a width: its rounded ends taper into the
        // corners, which must be painted although they lie below the inner
        // corner line.
        let item = DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 60.0),
            widths: [4.0, 0.0, 0.0, 0.0],
            colors: [BLUE; 4],
            styles: [BorderStyle::Solid; 4],
            radii: [(20.0, 20.0); 4],
        };
        let p = render(vec![item]);
        // Near the left end of the taper: the ring at x = 12.5 spans
        // y = 21.3..23.0, below the line from the inner corner (10, 14) to
        // the center, which used to give this area to the left side.
        let (r, _, b) = rgb(&p, 12, 22);
        assert!(
            b == 255 && r < 200,
            "taper is painted: {:?}",
            rgb(&p, 12, 22)
        );
        // Further down the left edge nothing is painted.
        assert_eq!(rgb(&p, 11, 50), (255, 255, 255));
    }

    #[test]
    fn uniform_inset_border_is_shaded() {
        let gray = Rgba::rgb(150, 150, 150);
        let p = render(vec![border([10.0; 4], [gray; 4], BorderStyle::Inset, 0.0)]);
        assert_eq!(rgb(&p, 50, 14), (100, 100, 100), "top is darker");
        assert_eq!(rgb(&p, 50, 65), (150, 150, 150), "bottom keeps the color");
    }

    #[test]
    fn double_border_has_two_lines() {
        let p = render(vec![border([9.0; 4], [BLUE; 4], BorderStyle::Double, 0.0)]);
        assert_eq!(rgb(&p, 50, 11), (0, 0, 255), "outer line");
        assert_eq!(rgb(&p, 50, 14), (255, 255, 255), "gap");
        assert_eq!(rgb(&p, 50, 17), (0, 0, 255), "inner line");
    }

    #[test]
    fn adjacent_rects_have_no_seam_at_fractional_scale() {
        let rect = |y: f32| DisplayItem::Rect {
            rect: Rect::new(0.0, y, 50.0, 13.0),
            radii: [(0.0, 0.0); 4],
            color: Rgba::BLACK,
        };
        let p = render_with(vec![rect(0.0), rect(13.0)], 1.25, &NoImages);
        for y in 0..32 {
            assert_eq!(rgb(&p, 10, y), (0, 0, 0), "row {y}");
        }
    }

    #[test]
    fn scaled_image_does_not_bleed_at_its_edges() {
        // 4x4: black, with a white bottom row.
        let mut image = Pixmap::new(4, 4).unwrap();
        image.fill(tiny_skia::Color::BLACK);
        for x in 0..4 {
            let i = (3 * 4 + x) * 4;
            image.data_mut()[i..i + 4].copy_from_slice(&[255; 4]);
        }
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let item = DisplayItem::Image {
            image: ImageRef::Url("x".into()),
            rect,
            tile: rect,
            clip: rect,
        };
        let p = render_with(
            vec![item],
            1.0,
            &OneImage(DecodedImage::from_pixmap(image), None),
        );
        assert_eq!(rgb(&p, 50, 0), (0, 0, 0));
    }

    fn svg(source: &str) -> DecodedImage {
        crate::decode_with_type(source.as_bytes(), Some(crate::SVG_MIME_TYPE)).unwrap()
    }

    fn image_item(tile: Rect, area: Rect) -> DisplayItem {
        DisplayItem::Image {
            image: ImageRef::Url("x".into()),
            rect: area,
            tile,
            clip: area,
        }
    }

    #[test]
    fn vector_images_are_sharp_at_any_scale() {
        // A 1x1 checkerboard of four squares: rendered at the device size,
        // the edge between the squares is a sharp pixel edge, also at 3x.
        let image = svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2">
            <rect width="1" height="1"/><rect x="1" y="1" width="1" height="1"/></svg>"#,
        );
        let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        let images = OneImage(image, Some(VectorCache::default()));
        for scale in [1.0, 2.0, 3.0] {
            let p = render_with(vec![image_item(rect, rect)], scale, &images);
            let at = |x: f32, y: f32| rgb(&p, (x * scale) as u32, (y * scale) as u32);
            let edge = 20.0 * scale;
            assert_eq!(rgb(&p, edge as u32 - 1, (15.0 * scale) as u32), (0, 0, 0));
            assert_eq!(rgb(&p, edge as u32, (15.0 * scale) as u32), (255, 255, 255));
            assert_eq!(at(25.0, 25.0), (0, 0, 0));
            assert_eq!(at(5.0, 5.0), (255, 255, 255), "outside the image");
        }
    }

    #[test]
    fn vector_images_tile_at_the_tile_size() {
        // A 10x10 tile: black left half.
        let image = svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="2" height="4"/></svg>"#,
        );
        let tile = Rect::new(0.0, 0.0, 10.0, 10.0);
        let area = Rect::new(0.0, 0.0, 40.0, 10.0);
        let p = render_with(vec![image_item(tile, area)], 1.0, &OneImage(image, None));
        for x in [0, 4, 10, 14, 30, 34] {
            assert_eq!(rgb(&p, x, 5), (0, 0, 0), "x = {x}");
            assert_eq!(rgb(&p, x + 5, 5), (255, 255, 255), "x = {}", x + 5);
        }
        assert_eq!(rgb(&p, 45, 5), (255, 255, 255), "outside the area");
    }

    #[test]
    fn render_sizes_are_bounded() {
        let size = |w: f32, h: f32| {
            vector_render_size(Rect::new(0.0, 0.0, w, h)).map(|s| (s.width(), s.height()))
        };
        assert_eq!(size(10.4, 0.2), Some((10, 1)));
        assert_eq!(size(0.0, 10.0), None);
        assert_eq!(size(f32::NAN, 10.0), None);
        assert_eq!(size(f32::INFINITY, 10.0), None);
        for huge in [1e9, 1e30] {
            let (w, h) = size(huge, huge).unwrap();
            assert!(w * h <= MAX_RENDER_PIXELS && w == h && w > 1000, "{w}x{h}");
        }
        let (w, h) = size(1e6, 2.0).unwrap();
        assert_eq!((w, h), (MAX_DIMENSION, 1));
    }

    #[test]
    fn opacity_group_is_composited() {
        let rect = Rect::new(20.0, 20.0, 10.0, 10.0);
        let p = render(vec![
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
        ]);
        let (r, g, b) = rgb(&p, 25, 25);
        assert_eq!(r, 255);
        assert!((126..=129).contains(&g) && g == b, "{g} {b}");
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    }

    #[test]
    fn transparent_group_draws_nothing() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let p = render(vec![
            DisplayItem::PushOpacity {
                opacity: 0.0,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
        ]);
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    }

    #[test]
    fn clip_applies_inside_opacity_layers() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let p = render(vec![
            DisplayItem::PushClip(Rect::new(0.0, 0.0, 50.0, 100.0)),
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
            DisplayItem::Rect {
                rect,
                radii: [(4.0, 4.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
            DisplayItem::PopClip,
        ]);
        assert_ne!(rgb(&p, 25, 50), (255, 255, 255));
        assert_eq!(rgb(&p, 75, 50), (255, 255, 255));
    }

    #[test]
    fn snapping_keeps_thin_rects_visible() {
        assert_eq!(
            snap(Rect::new(0.4, 1.6, 2.2, 0.3)),
            Rect::new(0.0, 2.0, 3.0, 1.0)
        );
        assert_eq!(snap(Rect::new(1.0, 1.0, 0.0, 0.0)).width, 0.0);
    }

    // ----- Masks -----

    /// A 20x20 SVG: black left half.
    const LEFT_HALF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="10" height="20"/></svg>"#;

    fn mask_layer(image: MaskLayerImage) -> MaskLayer {
        MaskLayer {
            image,
            luminance: false,
            composite: swb_style::CompositeOperator::SourceOver,
        }
    }

    fn image_layer(area: Rect, tile: Rect) -> MaskLayer {
        mask_layer(MaskLayerImage::Image {
            image: ImageRef::Url("x".into()),
            area,
            tile,
        })
    }

    /// A red square at (20, 20) of 40x40, masked by `layers`.
    fn masked_square(layers: Vec<MaskLayer>) -> Vec<DisplayItem> {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        vec![
            DisplayItem::PushMask {
                bounds: rect,
                layers: Arc::from(layers),
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopMask,
        ]
    }

    #[test]
    fn mask_multiplies_by_the_image_alpha() {
        let images = OneImage(svg(LEFT_HALF), Some(VectorCache::default()));
        // One 40x40 tile: the left half of the square stays.
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        for scale in [1.0, 2.0] {
            let p = render_with(masked_square(vec![image_layer(rect, rect)]), scale, &images);
            let at = |x: f32, y: f32| rgb(&p, (x * scale) as u32, (y * scale) as u32);
            assert_eq!(at(25.0, 30.0), (255, 0, 0), "scale {scale}");
            assert_eq!(at(39.0, 30.0), (255, 0, 0), "scale {scale}");
            assert_eq!(at(41.0, 30.0), (255, 255, 255), "scale {scale}");
            assert_eq!(at(10.0, 10.0), (255, 255, 255), "scale {scale}");
        }
    }

    #[test]
    fn unavailable_layers_hide_the_group() {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        // An image that is not loaded, and an empty layer.
        for layer in [image_layer(rect, rect), mask_layer(MaskLayerImage::Empty)] {
            let p = render_with(masked_square(vec![layer]), 1.0, &NoImages);
            assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
        }
        // An opaque layer shows the part inside its area.
        let p = render(masked_square(vec![mask_layer(MaskLayerImage::Opaque(
            Rect::new(20.0, 20.0, 10.0, 40.0),
        ))]));
        assert_eq!(rgb(&p, 25, 40), (255, 0, 0));
        assert_eq!(rgb(&p, 35, 40), (255, 255, 255));
    }

    #[test]
    fn luminance_masks_use_the_colors() {
        let grey = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="rgb(128,128,128)"/></svg>"#;
        let images = OneImage(svg(grey), None);
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        let mut layer = image_layer(rect, rect);
        layer.luminance = true;
        let p = render_with(masked_square(vec![layer]), 1.0, &images);
        // Red at half coverage over white (Chromium: the same).
        let (r, g, b) = rgb(&p, 40, 40);
        assert_eq!(r, 255);
        assert!((126..=128).contains(&g) && g == b, "{g} {b}");
        // The alpha of the same image is opaque.
        let p = render_with(masked_square(vec![image_layer(rect, rect)]), 1.0, &images);
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    }

    #[test]
    fn layers_composite_from_the_bottom() {
        use swb_style::CompositeOperator as Op;
        let images = OneImage(svg(LEFT_HALF), None);
        let square = Rect::new(20.0, 20.0, 40.0, 40.0);
        // Top: the left half (an image tile of the square); bottom: the
        // top half (opaque).
        let top_half = mask_layer(MaskLayerImage::Opaque(Rect::new(20.0, 20.0, 40.0, 20.0)));
        let quadrants = |op: Op| -> [bool; 4] {
            let mut top = image_layer(square, square);
            top.composite = op;
            let p = render_with(masked_square(vec![top, top_half.clone()]), 1.0, &images);
            // Top left, top right, bottom left, bottom right.
            [(30, 30), (50, 30), (30, 50), (50, 50)].map(|(x, y)| rgb(&p, x, y) == (255, 0, 0))
        };
        assert_eq!(quadrants(Op::SourceOver), [true, true, true, false]);
        assert_eq!(quadrants(Op::SourceIn), [true, false, false, false]);
        assert_eq!(quadrants(Op::SourceOut), [false, false, true, false]);
        assert_eq!(quadrants(Op::Xor), [false, true, true, false]);
        assert_eq!(quadrants(Op::DestinationOut), [false, true, false, false]);
        // The operator of the bottom layer is ignored.
        let mut bottom = top_half.clone();
        bottom.composite = Op::SourceIn;
        let p = render(masked_square(vec![bottom]));
        assert_eq!(rgb(&p, 30, 30), (255, 0, 0));
    }

    #[test]
    fn gradient_masks_tile() {
        use swb_style::{Color, LinearGradient};
        // Opaque on the left, transparent on the right of each tile.
        let gradient = Arc::new(LinearGradient {
            angle_deg: 90.0,
            stops: vec![
                (Color::Rgba(Rgba::BLACK), None),
                (Color::Rgba(Rgba::TRANSPARENT), None),
            ],
            repeating: false,
        });
        let layer = |tile: Rect| {
            mask_layer(MaskLayerImage::Gradient {
                gradient: Arc::clone(&gradient),
                current_color: Rgba::BLACK,
                area: Rect::new(20.0, 20.0, 40.0, 40.0),
                tile,
            })
        };
        // Four 10 px wide tiles.
        let p = render(masked_square(vec![layer(Rect::new(
            20.0, 20.0, 10.0, 40.0,
        ))]));
        for tile in 0..4 {
            let x = 20 + tile * 10;
            let left = rgb(&p, x, 40);
            let right = rgb(&p, x + 9, 40);
            assert!(left.1 < 40, "tile {tile} left {left:?}");
            assert!(right.1 > 200, "tile {tile} right {right:?}");
        }
        // One tile: the gradient spans the square.
        let p = render(masked_square(vec![layer(Rect::new(
            20.0, 20.0, 40.0, 40.0,
        ))]));
        assert!(rgb(&p, 21, 40).1 < 20);
        assert!((100..160).contains(&rgb(&p, 40, 40).1));
        assert!(rgb(&p, 59, 40).1 > 230);
        // Twenty 2 px wide tiles: one tile is rendered and repeated.
        let p = render(masked_square(vec![layer(Rect::new(20.0, 20.0, 2.0, 40.0))]));
        for x in [20, 30, 58] {
            assert!(rgb(&p, x, 40).1 < 128, "x {x} {:?}", rgb(&p, x, 40));
            assert!(
                rgb(&p, x + 1, 40).1 > 128,
                "x {} {:?}",
                x + 1,
                rgb(&p, x + 1, 40)
            );
        }
        // A hard stop in a tile much larger than the visible area stays
        // sharp (each visible tile is drawn as a gradient).
        let hard = Arc::new(LinearGradient {
            angle_deg: 90.0,
            stops: vec![
                (
                    Color::Rgba(Rgba::BLACK),
                    Some(swb_style::LengthPercentage::Percent(0.5)),
                ),
                (
                    Color::Rgba(Rgba::TRANSPARENT),
                    Some(swb_style::LengthPercentage::Percent(0.5)),
                ),
            ],
            repeating: false,
        });
        let layer = mask_layer(MaskLayerImage::Gradient {
            gradient: hard,
            current_color: Rgba::BLACK,
            area: Rect::new(-500.0, -500.0, 2000.0, 2000.0),
            tile: Rect::new(20.0, 20.0, 40.0, 500.0),
        });
        let p = render(masked_square(vec![layer]));
        assert_eq!(rgb(&p, 39, 30), (255, 0, 0));
        assert_eq!(rgb(&p, 40, 30), (255, 255, 255));
    }

    /// A rasterizer for `target` at scale 1 with the given budgets.
    fn rasterizer<'a>(
        target: &'a mut Pixmap,
        vectors: &'a mut FrameBudget,
        max_layer_pixels: u64,
        max_mask_pixels: u64,
    ) -> Rasterizer<'a> {
        let target_size = (target.width(), target.height());
        Rasterizer {
            target: target.data_mut(),
            target_size,
            offset_y: 0.0,
            params: RasterParams {
                scroll: Point::default(),
                viewport_scroll: Point::default(),
                scale: 1.0,
            },
            layers: Vec::new(),
            budget: Budget {
                max_layer_pixels,
                max_mask_pixels,
                max_transform_pixels: MAX_TRANSFORM_PIXELS,
                max_path_work: path::MAX_PATH_WORK,
                ..Budget::default()
            },
            clips: Vec::new(),
            mask: None,
            vectors,
            translation: Point::default(),
            visible: Rect::new(0.0, 0.0, target_size.0 as f32, target_size.1 as f32),
            viewport_scroll: Point::default(),
            depth: 0,
            sticky: StickyCache::default(),
        }
    }

    /// A gradient mask layer of opaque-to-transparent tiles in `area`.
    fn gradient_layer(area: Rect, tile: Rect) -> MaskLayer {
        use swb_style::{Color, LinearGradient};
        mask_layer(MaskLayerImage::Gradient {
            gradient: Arc::new(LinearGradient {
                angle_deg: 90.0,
                stops: vec![
                    (Color::Rgba(Rgba::BLACK), None),
                    (Color::Rgba(Rgba::TRANSPARENT), None),
                ],
                repeating: false,
            }),
            current_color: Rgba::BLACK,
            area,
            tile,
        })
    }

    #[test]
    fn rendered_gradient_tiles_are_part_of_the_admitted_work() {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        // Twenty 2 px wide tiles: one tile is rendered and repeated.
        let layers = vec![gradient_layer(rect, Rect::new(20.0, 20.0, 2.0, 40.0))];
        let mut target = Pixmap::new(100, 100).unwrap();
        let mut vectors = FrameBudget::new();
        let r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        let work = r.mask_work(rect, &layers);
        assert!(work > 1600 + 40 * ROW_COST_PIXELS, "{work}");
        let masked = masked_square(layers);
        let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, work, 0);
        assert!(!skipped);
        let (r, g, _) = rgb(&p, 20, 40);
        assert!(r == 255 && g < 128, "drawn: {:?}", rgb(&p, 20, 40));
        let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, work - 1, 0);
        assert!(skipped);
        assert_eq!(rgb(&p, 20, 40), (255, 255, 255));
    }

    #[test]
    fn hostile_tile_plans_do_not_overflow() {
        let mut target = Pixmap::new(100, 100).unwrap();
        let mut vectors = FrameBudget::new();
        let r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        let visible = Rect::new(0.0, 0.0, 100.0, 1.0);
        let cases = [
            Rect::new(400.0, -33_000_000.0, 1e-7, 1.0),
            Rect::new(400.0, -33_000_000.0, 1.0, 1e-7),
            Rect::new(-3e7, -3e7, 1e-30, 1e-30),
            Rect::new(f32::NAN, 0.0, 1.0, 1.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 1.0),
        ];
        for tile in cases {
            let plan = r.tile_plan(Rect::new(0.0, 0.0, 100.0, 1.0), tile, visible);
            let _ = plan.work();
            if let TilePlan::Copies { columns, rows, .. } = plan {
                let count = (columns.1 - columns.0) * (rows.1 - rows.0);
                assert!((1..=MAX_GRADIENT_TILES).contains(&count), "{tile:?}");
            }
        }
    }

    #[test]
    fn strips_round_like_one_pass() {
        // Rectangles with edges at many fractional device positions, at a
        // fractional scale: strips of 7 rows give the same pixels.
        let items: Vec<DisplayItem> = (0..60)
            .map(|i| DisplayItem::Rect {
                rect: Rect::new(
                    1.0 + i as f32 * 0.37,
                    0.5 + i as f32 * 1.13,
                    5.0 + i as f32 * 0.21,
                    0.4 + (i % 7) as f32 * 0.3,
                ),
                radii: [(0.0, 0.0); 4],
                color: if i % 2 == 0 { RED } else { BLUE },
            })
            .collect();
        let list = DisplayList { items };
        let render = |strip_rows: u32| {
            let mut target = Pixmap::new(90, 210).unwrap();
            target.fill(tiny_skia::Color::WHITE);
            let params = RasterParams {
                scroll: Point::new(0.0, 0.3),
                viewport_scroll: Point::new(0.0, 0.3),
                scale: 2.6,
            };
            let mut fonts = FontContext::for_tests();
            rasterize_in_strips(
                &list,
                &mut target,
                params,
                strip_rows,
                &mut fonts,
                &NoImages,
            );
            target
        };
        let whole = render(210);
        for rows in [7, 13, 100] {
            assert!(whole.data() == render(rows).data(), "strips of {rows} rows");
        }
    }

    /// A list with a rotated group (with an opacity group and a mask group
    /// inside) and a fixed group, for strips.
    fn transformed_and_fixed_groups() -> DisplayList {
        use swb_layout::GroupTransform;
        let (sin, cos) = 25f32.to_radians().sin_cos();
        let (cx, cy) = (45.0, 60.0);
        let rotate = Matrix::new(
            cos,
            sin,
            -sin,
            cos,
            cx - cos * cx + sin * cy,
            cy - sin * cx - cos * cy,
        );
        let rect = |x, y, w, h, color| DisplayItem::Rect {
            rect: Rect::new(x, y, w, h),
            radii: [(0.0, 0.0); 4],
            color,
        };
        let mut items = vec![
            DisplayItem::PushTransform {
                transform: GroupTransform::Matrix(rotate),
                bounds: Rect::new(20.0, 20.0, 50.0, 100.0),
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            },
            rect(30.0, 40.0, 30.0, 80.0, BLUE),
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: Rect::new(25.0, 90.0, 40.0, 20.0),
                fixed_bounds: None,
                escapes_clips: false,
            },
            rect(25.0, 90.0, 40.0, 20.0, RED),
            DisplayItem::PopOpacity,
        ];
        let opaque = mask_layer(MaskLayerImage::Opaque(Rect::new(20.0, 20.0, 20.0, 40.0)));
        items.extend(masked_square(vec![opaque]));
        items.extend([
            DisplayItem::PopTransform,
            DisplayItem::PushViewportClip,
            DisplayItem::PushTransform {
                transform: GroupTransform::Fixed,
                bounds: Rect::new(10.0, 70.0, 20.0, 40.0),
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            },
            rect(10.0, 70.0, 20.0, 40.0, Rgba::rgb(0, 128, 0)),
            DisplayItem::PopTransform,
            DisplayItem::PopClip,
        ]);
        DisplayList { items }
    }

    /// Strips give the pixels of one pass (within rounding) for transform
    /// layers, and fixed groups are at the same place in every strip.
    #[test]
    fn transform_and_fixed_groups_in_strips() {
        let list = transformed_and_fixed_groups();
        let render = |strip_rows: u32| {
            let mut target = Pixmap::new(90, 210).unwrap();
            target.fill(tiny_skia::Color::WHITE);
            let params = RasterParams {
                scroll: Point::new(0.0, 10.0),
                viewport_scroll: Point::new(0.0, 10.0),
                scale: 1.5,
            };
            let mut fonts = FontContext::for_tests();
            rasterize_in_strips(
                &list,
                &mut target,
                params,
                strip_rows,
                &mut fonts,
                &NoImages,
            );
            target
        };
        let whole = render(210);
        // The fixed group moves with the scroll offset in every strip.
        let green = (0, 128, 0);
        assert_eq!(rgb(&whole, 30, 106), green);
        assert_eq!(rgb(&whole, 30, 164), green);
        assert_ne!(rgb(&whole, 30, 103), green);
        assert_ne!(rgb(&whole, 30, 166), green);
        for rows in [7, 13, 100] {
            let strips = render(rows);
            let diff: Vec<u8> = whole
                .data()
                .iter()
                .zip(strips.data())
                .map(|(a, b)| a.abs_diff(*b))
                .collect();
            let max = diff.iter().copied().max().unwrap_or(0);
            let count = diff.chunks(4).filter(|p| p.iter().any(|&d| d > 0)).count();
            // Transform layers have their pixels at the same positions in
            // every strip; rounding differs in a few pixels.
            assert!(max <= 16 && count <= 10, "strips of {rows} rows");
            if rows == 100 {
                assert_eq!(count, 0, "strips of {rows} rows");
            }
        }
    }

    /// Runs `items` on a white 100x100 target with the given budgets of
    /// group layers and transform layers, and returns the target and the
    /// rasterizer's budget afterwards.
    fn run_with_budgets(
        items: &[DisplayItem],
        max_layer_pixels: u64,
        max_transform_pixels: u64,
    ) -> (Pixmap, Budget) {
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let budget = {
            let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, MAX_MASK_PIXELS);
            r.budget.max_transform_pixels = max_transform_pixels;
            let ends = transform_ends(items);
            let mut fonts = FontContext::for_tests();
            r.run(items, 0..items.len(), &ends, &mut fonts, &NoImages);
            assert!(r.layers.is_empty());
            r.budget
        };
        (target, budget)
    }

    /// A 40x40 red square at (30, 30) rotated by 45 degrees about its
    /// center, with an opacity group and a mask group inside.
    fn rotated_square() -> Vec<DisplayItem> {
        use swb_layout::GroupTransform;
        let (sin, cos) = 45f32.to_radians().sin_cos();
        let rotate = Matrix::new(
            cos,
            sin,
            -sin,
            cos,
            50.0 - cos * 50.0 + sin * 50.0,
            50.0 - sin * 50.0 - cos * 50.0,
        );
        let rect = Rect::new(30.0, 30.0, 40.0, 40.0);
        let mut items = vec![
            DisplayItem::PushTransform {
                transform: GroupTransform::Matrix(rotate),
                bounds: rect,
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            },
            DisplayItem::PushOpacity {
                opacity: 1.0,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
        ];
        items.extend(masked_square(vec![mask_layer(MaskLayerImage::Opaque(
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ))]));
        items.extend([
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
            DisplayItem::PopTransform,
        ]);
        items
    }

    #[test]
    fn transform_layers_use_the_budgets_of_the_strip() {
        let items = rotated_square();
        // The layer, the opacity layer and the mask layers are released;
        // the work is the layer's pixels and the device pixels it covers.
        let (p, budget) = run_with_budgets(&items, MAX_GROUP_LAYER_PIXELS, MAX_TRANSFORM_PIXELS);
        assert_eq!(rgb(&p, 50, 50), (255, 0, 0));
        assert_eq!(budget.layer_pixels, 0);
        assert!(
            (3200..10_000).contains(&budget.transform_pixels),
            "{}",
            budget.transform_pixels
        );
        assert!(!budget.skipped.transforms && !budget.skipped.layers);
        let work = budget.transform_pixels;
        // Not enough work budget: not drawn.
        let (p, budget) = run_with_budgets(&items, MAX_GROUP_LAYER_PIXELS, work - 1);
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
        assert_eq!(budget.transform_pixels, 0);
        assert!(budget.skipped.transforms);
        // Not enough layer memory for the transform layer: not drawn.
        let (p, budget) = run_with_budgets(&items, 100, MAX_TRANSFORM_PIXELS);
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
        assert_eq!(budget.layer_pixels, 0);
        assert!(budget.skipped.layers);
    }

    #[test]
    fn transform_layers_nest_up_to_a_depth() {
        use swb_layout::GroupTransform;
        let rotate = GroupTransform::Matrix(Matrix::new(0.0, 1.0, -1.0, 0.0, 100.0, 0.0));
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let nested = |depth: usize| {
            let mut items: Vec<DisplayItem> = (0..depth)
                .map(|_| DisplayItem::PushTransform {
                    transform: rotate.clone(),
                    bounds: rect,
                    hit_bounds: Rect::default(),
                    fixed_bounds: None,
                })
                .collect();
            items.push(DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            });
            items.extend((0..depth).map(|_| DisplayItem::PopTransform));
            items
        };
        let (p, budget) = run_with_budgets(
            &nested(MAX_TRANSFORM_DEPTH),
            MAX_GROUP_LAYER_PIXELS,
            MAX_TRANSFORM_PIXELS,
        );
        assert_eq!(rgb(&p, 50, 50), (255, 0, 0));
        assert!(!budget.skipped.transforms);
        assert_eq!(budget.layer_pixels, 0);
        let (p, budget) = run_with_budgets(
            &nested(MAX_TRANSFORM_DEPTH + 1),
            MAX_GROUP_LAYER_PIXELS,
            MAX_TRANSFORM_PIXELS,
        );
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
        assert!(budget.skipped.transforms);
        assert_eq!(budget.layer_pixels, 0);
    }

    #[test]
    fn transform_layer_geometry_is_bounded() {
        let area = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(layer_scale(area, f32::NAN), None);
        assert_eq!(layer_scale(area, 0.0), None);
        assert_eq!(layer_scale(area, f32::INFINITY), None);
        // Sides and pixels are limited.
        let k = layer_scale(Rect::new(0.0, 0.0, 1e6, 1.0), 1.0).unwrap();
        assert!(1e6 * k <= MAX_TRANSFORM_LAYER_SIDE * 1.001);
        let k = layer_scale(Rect::new(0.0, 0.0, 1e4, 1e4), 1.0).unwrap();
        assert!(1e8 * k * k <= MAX_TRANSFORM_LAYER_PIXELS * 1.001);
        assert_eq!(
            layer_size(Rect::new(0.0, 0.0, f32::NAN, 1.0), 1.0),
            Some((1, 1))
        );
        assert_eq!(layer_size(Rect::new(0.0, 0.0, 1e9, 1.0), 1.0), None);
        // The grid starts at the corner of the bounds; two pixels of
        // margin, inside the bounds.
        let bounds = Rect::new(10.0, 10.0, 100.0, 100.0);
        let grid = grid_area(Rect::new(30.3, 40.7, 10.0, 10.0), bounds, 2.0);
        assert_eq!((grid.x, grid.y), (29.0, 39.5));
        assert!((grid.right() - 41.3).abs() < 1e-4 && (grid.bottom() - 51.7).abs() < 1e-4);
        assert_eq!(grid_area(bounds, bounds, 2.0), bounds);
        let nan = grid_area(Rect::new(f32::NAN, 0.0, 1.0, 1.0), bounds, 2.0);
        assert!(nan.width >= 0.0 || nan.width.is_nan());
    }

    #[test]
    fn strips_have_equal_heights() {
        assert_eq!(equal_strip_rows(30_000, 20_972), 15_000);
        assert_eq!(equal_strip_rows(30_001, 20_972), 15_001);
        assert_eq!(equal_strip_rows(100, 100), 100);
        assert_eq!(equal_strip_rows(100, 1000), 100);
        assert_eq!(equal_strip_rows(100, 0), 1);
        assert_eq!(equal_strip_rows(0, 5), 1);
        assert_eq!(equal_strip_rows(10, 3), 3);
    }

    #[test]
    fn layer_pixels_are_bounded() {
        let mut target = Pixmap::new(1, 1).unwrap();
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(&mut target, &mut vectors, 100, MAX_MASK_PIXELS);
        r.budget.layer_pixels = 90;
        let small = r.layer_pixmap(2, 5).unwrap();
        assert_eq!(r.budget.layer_pixels, 100);
        assert!(r.layer_pixmap(1, 1).is_none());
        assert!(r.budget.skipped.layers);
        r.release(&small);
        assert!(r.layer_pixmap(1, 1).is_some());
    }

    /// Rasterizes `items` on a white 100x100 target with the given
    /// budgets, after `mask_work` pixels of mask work.
    fn render_with_budgets(
        items: &[DisplayItem],
        max_layer_pixels: u64,
        max_mask_pixels: u64,
        mask_work: u64,
    ) -> (Pixmap, bool) {
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, max_mask_pixels);
        r.budget.mask_pixels = mask_work;
        let mut fonts = FontContext::for_tests();
        for item in items {
            r.item(item, &mut fonts, &NoImages);
        }
        assert!(r.layers.is_empty());
        assert_eq!(r.budget.layer_pixels, 0, "all layers are released");
        let masks_skipped = r.budget.skipped.masks;
        (target, masks_skipped)
    }

    fn render_with_layer_budget(items: &[DisplayItem], max_layer_pixels: u64) -> Pixmap {
        render_with_budgets(items, max_layer_pixels, MAX_MASK_PIXELS, 0).0
    }

    #[test]
    fn mask_groups_start_if_their_work_fits() {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        let opaque = || mask_layer(MaskLayerImage::Opaque(rect));
        // Three layers of 40x40 px: (1600 + 40 rows * 16) * 3 = 6720 px of
        // work.
        let masked = masked_square(vec![opaque(), opaque(), opaque()]);
        let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, 6720, 0);
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
        assert!(!skipped);
        // It does not fit after other work: no layer, nothing drawn.
        let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, 6720, 1);
        assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
        assert!(skipped);
        // A second group (20x20 px: 400 + 20 * 16 = 720) after the first.
        let small = Rect::new(70.0, 70.0, 20.0, 20.0);
        let mut both = masked.clone();
        both.extend([
            DisplayItem::PushMask {
                bounds: small,
                layers: Arc::from([mask_layer(MaskLayerImage::Opaque(small))]),
            },
            DisplayItem::Rect {
                rect: small,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopMask,
        ]);
        let (p, skipped) = render_with_budgets(&both, MAX_GROUP_LAYER_PIXELS, 7439, 0);
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
        assert_eq!(rgb(&p, 80, 80), (255, 255, 255));
        assert!(skipped);
        let (p, _) = render_with_budgets(&both, MAX_GROUP_LAYER_PIXELS, 7440, 0);
        assert_eq!(rgb(&p, 80, 80), (255, 0, 0));
    }

    #[test]
    fn strips_render_like_one_pass() {
        // A rounded rectangle, an opacity group and a masked group across
        // the strip boundaries.
        let images = OneImage(svg(LEFT_HALF), Some(VectorCache::default()));
        let rect = Rect::new(10.0, 15.0, 70.0, 70.0);
        let items = vec![
            DisplayItem::Rect {
                rect: Rect::new(5.0, 5.0, 90.0, 90.0),
                radii: [(9.0, 9.0); 4],
                color: BLUE,
            },
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
            DisplayItem::PushMask {
                bounds: rect,
                layers: Arc::from([image_layer(rect, Rect::new(10.0, 15.0, 20.0, 20.0))]),
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopMask,
            DisplayItem::PopOpacity,
        ];
        let render = |strip_rows: u32| {
            let params = RasterParams {
                scroll: Point::default(),
                viewport_scroll: Point::default(),
                scale: 2.0,
            };
            let mut target = Pixmap::new(200, 200).unwrap();
            target.fill(tiny_skia::Color::WHITE);
            let mut fonts = FontContext::for_tests();
            let list = DisplayList {
                items: items.clone(),
            };
            rasterize_in_strips(&list, &mut target, params, strip_rows, &mut fonts, &images);
            target
        };
        let whole = render(200);
        // Exact where no anti-aliased curve crosses a strip boundary (the
        // rounded corners are rows 10..28 and 172..190).
        for rows in [64, 199] {
            assert!(whole.data() == render(rows).data(), "strips of {rows} rows");
        }
        // Otherwise tiny-skia clips the path edges to the strip, which
        // changes the coverage of single anti-aliased edge pixels of the
        // corners. Everything else (the opacity and mask groups, rows
        // 30..170) is the same.
        for rows in [1, 7] {
            let strips = render(rows);
            let differing: Vec<usize> = whole
                .pixels()
                .iter()
                .zip(strips.pixels())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| i / 200)
                .collect();
            assert!(
                differing.iter().all(|row| !(30..170).contains(row)),
                "strips of {rows}"
            );
            assert!(
                differing.len() < 200,
                "strips of {rows}: {}",
                differing.len()
            );
        }
    }

    #[test]
    fn groups_beyond_the_layer_budget() {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        let square = DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        };
        let opacity = vec![
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
            square.clone(),
            DisplayItem::PopOpacity,
        ];
        // Within the budget: composited with the opacity.
        let p = render_with_layer_budget(&opacity, 10_000);
        assert_ne!(rgb(&p, 40, 40), (255, 0, 0));
        // Beyond it: drawn directly, without the opacity.
        let p = render_with_layer_budget(&opacity, 100);
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
        // A mask group needs its layer and a temporary one; beyond the
        // budget it draws nothing.
        let opaque = mask_layer(MaskLayerImage::Opaque(rect));
        let mut masked = masked_square(vec![opaque]);
        let p = render_with_layer_budget(&masked, 2 * 1600);
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
        let p = render_with_layer_budget(&masked, 2 * 1600 - 1);
        assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
        // A masked group inside an opacity group: three layers.
        masked.insert(
            0,
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
                fixed_bounds: None,
                escapes_clips: false,
            },
        );
        masked.push(DisplayItem::PopOpacity);
        let p = render_with_layer_budget(&masked, 3 * 1600);
        let (r, g, b) = rgb(&p, 40, 40);
        assert!(
            r == 255 && (120..=135).contains(&g) && g == b,
            "{r} {g} {b}"
        );
        let p = render_with_layer_budget(&masked, 3 * 1600 - 1);
        assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
    }

    #[test]
    fn opaque_layers_are_opaque_for_luminance() {
        let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
        let mut layer = mask_layer(MaskLayerImage::Opaque(rect));
        layer.luminance = true;
        let p = render(masked_square(vec![layer]));
        assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    }

    fn svg_path(points: &[(f32, f32)]) -> Arc<swb_layout::svg::SvgPath> {
        use swb_layout::svg::{PathSegment, SvgPath};
        let mut segments: Vec<PathSegment> = points
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| {
                let p = Point::new(x, y);
                if i == 0 {
                    PathSegment::MoveTo(p)
                } else {
                    PathSegment::LineTo(p)
                }
            })
            .collect();
        segments.push(PathSegment::Close);
        Arc::new(SvgPath::from_segments(&segments).expect("a path"))
    }

    fn fill_path(
        path: Arc<swb_layout::svg::SvgPath>,
        transform: Matrix,
        color: Rgba,
    ) -> DisplayItem {
        DisplayItem::FillPath {
            path,
            transform,
            color,
            rule: swb_style::FillRule::NonZero,
            anti_alias: true,
        }
    }

    fn stroke(width: f32, dashes: Option<&[f32]>) -> Arc<swb_layout::svg::StrokeStyle> {
        Arc::new(swb_layout::svg::StrokeStyle {
            width,
            cap: swb_style::StrokeLinecap::Butt,
            join: swb_style::StrokeLinejoin::Miter,
            miter_limit: 4.0,
            dashes: dashes.map(Arc::from),
            dash_offset: 0.0,
        })
    }

    #[test]
    fn svg_paths_fill_and_stroke_with_their_transform() {
        // A 10x10 square scaled by 4 and moved to (10, 10).
        let square = svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let m = Matrix::new(4.0, 0.0, 0.0, 4.0, 10.0, 10.0);
        let p = render(vec![fill_path(Arc::clone(&square), m, RED)]);
        assert_eq!(rgb(&p, 12, 12), (255, 0, 0));
        assert_eq!(rgb(&p, 48, 48), (255, 0, 0));
        assert_eq!(rgb(&p, 52, 30), (255, 255, 255));
        // The stroke scales with the transform: 2 units are 8 px.
        let p = render(vec![DisplayItem::StrokePath {
            path: square,
            transform: m,
            color: BLUE,
            stroke: stroke(2.0, None),
            anti_alias: true,
        }]);
        assert_eq!(rgb(&p, 7, 30), (0, 0, 255));
        assert_eq!(rgb(&p, 13, 30), (0, 0, 255));
        assert_eq!(rgb(&p, 30, 30), (255, 255, 255));
        // At scale 2 the same items cover twice the device pixels.
        let p = render_with(
            vec![fill_path(
                svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]),
                Matrix::IDENTITY,
                RED,
            )],
            2.0,
            &NoImages,
        );
        assert_eq!(rgb(&p, 18, 2), (255, 0, 0));
        assert_eq!(rgb(&p, 2, 18), (255, 255, 255));
    }

    #[test]
    fn svg_paths_with_hostile_geometry_draw_nothing() {
        let square = svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        for m in [
            Matrix::new(1e30, 0.0, 0.0, 1e30, 0.0, 0.0),
            Matrix::new(f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0),
            Matrix::new(0.0, 0.0, 0.0, 0.0, 10.0, 10.0),
            Matrix::new(1e12, 0.0, 0.0, 1e12, -1e13, -1e13),
        ] {
            let p = render(vec![
                fill_path(Arc::clone(&square), m, RED),
                DisplayItem::StrokePath {
                    path: Arc::clone(&square),
                    transform: m,
                    color: RED,
                    stroke: stroke(1e30, Some(&[1e-30, 1e-30])),
                    anti_alias: true,
                },
            ]);
            // Nothing or a fill of the whole target; no panic.
            let _ = rgb(&p, 50, 50);
        }
    }

    #[test]
    fn svg_strokes_with_too_many_dashes_are_solid() {
        use swb_layout::svg::{PathSegment, SvgPath};
        let line = Arc::new(
            SvgPath::from_segments(&[
                PathSegment::MoveTo(Point::new(0.0, 50.0)),
                PathSegment::LineTo(Point::new(100.0, 50.0)),
            ])
            .expect("a path"),
        );
        let item = |dashes: &[f32]| DisplayItem::StrokePath {
            path: Arc::clone(&line),
            transform: Matrix::IDENTITY,
            color: RED,
            stroke: stroke(10.0, Some(dashes)),
            anti_alias: true,
        };
        // 10 on, 10 off: the gap at x = 15 is white.
        let p = render(vec![item(&[10.0, 10.0])]);
        assert_eq!(rgb(&p, 5, 50), (255, 0, 0));
        assert_eq!(rgb(&p, 15, 50), (255, 255, 255));
        // 100 / 0.001 * 2 dashes, more than the limit.
        let tiny = 0.0005;
        assert!(100.0 / (2.0 * tiny) * 2.0 > path::MAX_DASHES);
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        let items = [item(&[tiny, tiny])];
        let mut fonts = FontContext::for_tests();
        r.run(&items, 0..1, &transform_ends(&items), &mut fonts, &NoImages);
        assert!(r.budget.skipped.dashes);
        assert_eq!(rgb(&target, 15, 50), (255, 0, 0));
    }

    #[test]
    fn svg_paths_beyond_the_work_budget_are_not_drawn() {
        let square = svg_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
        let items = [
            fill_path(Arc::clone(&square), Matrix::IDENTITY, RED),
            fill_path(square, Matrix::translate(0.0, 50.0), BLUE),
        ];
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        // The first fill costs its fixed part and about 100 x 100 pixels at 8 units.
        r.budget.max_path_work = 100_000.0;
        let mut fonts = FontContext::for_tests();
        r.run(&items, 0..2, &transform_ends(&items), &mut fonts, &NoImages);
        assert!(r.budget.skipped.paths);
        assert_eq!(rgb(&target, 50, 75), (255, 0, 0));
    }

    /// A clip shape of `points` in a clip path of the SVG content.
    fn clip_shape(
        points: &[(f32, f32)],
        transform: Matrix,
        clip: Option<swb_layout::svg::ClipRegion>,
    ) -> swb_layout::svg::ClipShape {
        swb_layout::svg::ClipShape {
            path: svg_path(points),
            transform,
            rule: swb_style::FillRule::NonZero,
            clip,
        }
    }

    /// The items of `content` inside an SVG clip group at (`dx`, 0).
    fn clipped(clip: swb_layout::svg::ClipPath, dx: f32, content: DisplayItem) -> Vec<DisplayItem> {
        vec![
            DisplayItem::PushSvgClip {
                clip: Arc::new(clip),
                transform: Matrix::translate(dx, 0.0),
                bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            content,
            DisplayItem::PopSvgClip,
        ]
    }

    fn full_square() -> DisplayItem {
        let square = svg_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
        fill_path(square, Matrix::IDENTITY, RED)
    }

    #[test]
    fn svg_clip_groups_show_the_union_of_the_clip_shapes() {
        use swb_layout::svg::{ClipPath, ClipRegion};
        let id = Matrix::IDENTITY;
        let left = clip_shape(
            &[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)],
            id,
            None,
        );
        let right = clip_shape(
            &[(60.0, 0.0), (100.0, 0.0), (100.0, 40.0), (60.0, 40.0)],
            id,
            None,
        );
        let clip = ClipPath {
            shapes: vec![left, right],
            outer: None,
        };
        let p = render(clipped(clip, 0.0, full_square()));
        assert_eq!(rgb(&p, 20, 20), (255, 0, 0));
        assert_eq!(rgb(&p, 80, 20), (255, 0, 0));
        assert_eq!(rgb(&p, 50, 20), (255, 255, 255));
        assert_eq!(rgb(&p, 20, 60), (255, 255, 255));
        // The `transform` of the group moves the clip shapes with it.
        let shifted = ClipPath {
            shapes: vec![clip_shape(
                &[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)],
                id,
                None,
            )],
            outer: None,
        };
        let p = render(clipped(shifted, 50.0, full_square()));
        assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
        assert_eq!(rgb(&p, 70, 20), (255, 0, 0));
        // The `outer` region is intersected: here the left half of the
        // shapes only.
        let outer = ClipPath {
            shapes: vec![clip_shape(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 40.0), (0.0, 40.0)],
                id,
                None,
            )],
            outer: Some(ClipRegion::Rect(Rect::new(0.0, 0.0, 50.0, 100.0))),
        };
        let p = render(clipped(outer, 0.0, full_square()));
        assert_eq!(rgb(&p, 20, 20), (255, 0, 0));
        assert_eq!(rgb(&p, 80, 20), (255, 255, 255));
        // A shape with its own clip: the part inside it.
        let own = ClipPath {
            shapes: vec![clip_shape(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
                id,
                Some(ClipRegion::Rect(Rect::new(50.0, 50.0, 50.0, 50.0))),
            )],
            outer: None,
        };
        let p = render(clipped(own, 0.0, full_square()));
        assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
        assert_eq!(rgb(&p, 70, 70), (255, 0, 0));
    }

    #[test]
    fn an_empty_svg_clip_hides_the_group() {
        use swb_layout::svg::ClipPath;
        let clip = ClipPath {
            shapes: Vec::new(),
            outer: None,
        };
        let p = render(clipped(clip, 0.0, full_square()));
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    }

    #[test]
    fn svg_clip_groups_are_bounded_by_the_layer_budget() {
        use swb_layout::svg::ClipPath;
        let id = Matrix::IDENTITY;
        let clip = ClipPath {
            shapes: vec![clip_shape(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
                id,
                None,
            )],
            outer: None,
        };
        let items = clipped(clip, 0.0, full_square());
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        // The group needs the layer and a temporary one for the coverage.
        let mut r = rasterizer(&mut target, &mut vectors, 100 * 100, MAX_MASK_PIXELS);
        let mut fonts = FontContext::for_tests();
        r.run(&items, 0..3, &transform_ends(&items), &mut fonts, &NoImages);
        assert!(r.budget.skipped.layers);
        assert_eq!(r.budget.layer_pixels, 0);
        // The group draws nothing when its clip cannot be made.
        assert_eq!(rgb(&target, 50, 50), (255, 255, 255));
    }

    #[test]
    fn svg_fills_without_anti_aliasing_cover_the_pixels_with_centers_inside() {
        // A circle of radius 15.3 around (20, 20): 732 pixel centers are
        // inside (the count that Chromium draws with `crispEdges`).
        use swb_layout::svg::{PathSegment, SvgPath};
        let (cx, cy, r, k) = (20.0_f32, 20.0_f32, 15.3_f32, 0.552_284_8 * 15.3);
        let p = Point::new;
        let circle = Arc::new(
            SvgPath::from_segments(&[
                PathSegment::MoveTo(p(cx + r, cy)),
                PathSegment::CubicTo(p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r)),
                PathSegment::CubicTo(p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy)),
                PathSegment::CubicTo(p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r)),
                PathSegment::CubicTo(p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy)),
                PathSegment::Close,
            ])
            .expect("a path"),
        );
        let item = |anti_alias| DisplayItem::FillPath {
            path: Arc::clone(&circle),
            transform: Matrix::IDENTITY,
            color: Rgba::BLACK,
            rule: swb_style::FillRule::NonZero,
            anti_alias,
        };
        let count = |anti_alias| {
            let pixmap = render(vec![item(anti_alias)]);
            let mut exact = 0;
            let mut partial = 0;
            for y in 0..40 {
                for x in 0..40 {
                    match rgb(&pixmap, x, y) {
                        (0, 0, 0) => exact += 1,
                        (255, 255, 255) => {}
                        _ => partial += 1,
                    }
                }
            }
            (exact, partial)
        };
        assert_eq!(count(false), (732, 0));
        assert!(count(true).1 > 20);
    }
}
