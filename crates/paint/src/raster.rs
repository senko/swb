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
//! - A filter group (`raster/filter_layer.rs`, `crate::filter`) draws into a
//!   layer without the enclosing clips and changes its pixels when it ends
//!   (color functions, blur, drop shadow); the clips apply when the result
//!   is composited. A filter work budget bounds the steps.
//! - Gradients (`raster/gradient.rs`) are interpolated in premultiplied
//!   space: tiny-skia interpolates colors without the alpha, so
//!   `premultiplied_stops` cuts the pairs of stops that need it into
//!   sections.
//! - A tall target (a full-page screenshot) can be rasterized in strips
//!   ([`rasterize_in_strips`]); the budgets apply to each strip.
//! - Glyphs come from the `text` crate as alpha masks and are blended by
//!   [`blit_mask`].
//! - Vector (SVG) images are rendered at the device pixel size of one tile
//!   and then drawn like raster images. Large raster images are drawn from
//!   reduced levels (`crate::reduce`) within a work budget per frame.
//! - Inline SVG (ADR 0023) draws path items (fills and strokes, in
//!   `raster/path.rs`) with tiny-skia, directly in every frame. Each path
//!   is charged an estimate of its work (`crate::path_cost`) against the
//!   path work budget of the strip ([`path::MAX_PATH_WORK`]) and is not
//!   drawn if it does not fit; a stroke with too many dashes is drawn
//!   solid.
//! - An SVG clip group (`raster/svg_clip.rs`, ADR 0023 and ADR 0024) draws
//!   into a layer that is multiplied by the coverage of its clip path. A
//!   rounded overflow clip is such a group; when its layer or coverage
//!   does not fit into the budgets, it falls back to the rectangle clip
//!   instead of hiding its content.
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
//! - A frame (`raster/frame.rs`, ADR 0031 section 4) draws the display list
//!   of its document in place, inside the groups that are open at its item:
//!   the budgets of the strip cover it. [`frame::MAX_FRAME_ITEMS`] bounds
//!   the items of the frames of a strip.

use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{GroupTransform, Matrix, Point, Rect, StickyCache};
use swb_style::{BorderStyle, Rgba};
use swb_text::FontContext;
use tiny_skia::{
    FillRule, FilterQuality, IntSize, Mask, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapMut,
    PixmapPaint, Shader, SpreadMode, Stroke, Transform,
};

use crate::display_list::{DisplayItem, DisplayList, ImageRef, Radii};
use crate::group_bounds::transform_ends;
use crate::image::{DecodedImage, ImageKind, MAX_DIMENSION};
use crate::mask::{self, MaskLayer, MaskLayerImage};
use crate::reduce::Reductions;
use crate::svg::{FrameBudget, MAX_RENDER_PIXELS, VectorCache};

mod backdrop;
mod border;
mod filter_layer;
mod frame;
mod gradient;
mod masks;
mod path;
mod svg_clip;

use filter_layer::FilterLayer;
pub use frame::FrameContent;
use gradient::{degenerate_color, gradient_line, gradient_shader, gradient_tile};
use svg_clip::ClipResult;

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

/// Supplies the resources of a document to the rasterizer: its decoded
/// images and the content of its frames.
pub trait ImageSource {
    /// The decoded image, if it is loaded.
    fn image(&self, image: &ImageRef) -> Option<&DecodedImage>;

    /// The cache for vector images rendered at their drawn size. Without a
    /// cache, vector images are rendered for every draw.
    fn vector_cache(&self) -> Option<&VectorCache>;

    /// The content of the frame of the `iframe` element `node`, if it has
    /// one to draw. The default has no frames.
    fn frame(&self, _node: NodeId) -> Option<FrameContent<'_>> {
        None
    }
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

/// The filter work of one strip, in pixels of one color pass (see
/// `crate::filter::work`): 128 Mpx, about 0.3 s (a blur counts six passes), or
/// eight times the strip's pixels if that is more. A filter group starts
/// only if its work fits into the rest of the budget; a group that does not
/// fit draws its content without the filter.
const MAX_FILTER_PIXELS: u64 = 128 * 1024 * 1024;

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
/// opacity, mask, transform and clip groups share a memory budget
/// ([`MAX_GROUP_LAYER_PIXELS`]); masks ([`MAX_MASK_PIXELS`]), transform
/// layers ([`MAX_TRANSFORM_PIXELS`]) and the paths and clip paths of inline
/// SVG ([`path::MAX_PATH_WORK`]) have work budgets.
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
/// layers, masks, transform layers and paths apply to each strip, and
/// opacity and mask layers are at most a strip high. Fixed and sticky boxes
/// are placed for `params.scroll` in every strip. The strips have equal
/// heights (the last one can be lower, by fewer rows than there are
/// strips), and all get the budgets of that height. Each strip is drawn in
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
    let mut vectors = FrameBudget::new();
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
        skipped.merge(s);
    }
    if vectors.skipped() {
        log::warn!(
            "SVG images: the rendering budget of this frame ran out; some images \
             use a rendering of another size or are not drawn"
        );
    }
    if vectors.reduce.skipped() {
        log::warn!(
            "images: the work budget for reduced copies ran out in this frame; some images \
             are drawn from a larger level (coarser), a later repaint continues"
        );
    }
    skipped.warn();
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
    /// A filter group did not fit into [`MAX_FILTER_PIXELS`].
    filters: bool,
    /// A transform group was nested deeper than [`MAX_TRANSFORM_DEPTH`] or
    /// did not fit into [`MAX_TRANSFORM_PIXELS`].
    transforms: bool,
    /// An SVG path did not fit into [`path::MAX_PATH_WORK`].
    paths: bool,
    /// An SVG stroke had more than [`crate::path_cost::MAX_DASHES`] dashes.
    dashes: bool,
    /// A frame did not fit into [`frame::MAX_FRAME_ITEMS`], or its groups
    /// nested too deep.
    frames: bool,
}

impl Skipped {
    /// Adds what `other` left out.
    fn merge(&mut self, other: Skipped) {
        self.layers |= other.layers;
        self.masks |= other.masks;
        self.filters |= other.filters;
        self.transforms |= other.transforms;
        self.paths |= other.paths;
        self.dashes |= other.dashes;
        self.frames |= other.frames;
    }

    /// Logs a warning for each budget that left something out.
    fn warn(self) {
        let table = [
            (
                self.layers,
                "opacity, mask and transform groups: the layer memory budget ran out; some \
                 opacity groups are drawn without their opacity, some groups are not drawn",
            ),
            (
                self.masks,
                "masks: the work budget of this frame ran out; some masked boxes are not drawn",
            ),
            (
                self.filters,
                "filters: the work budget of this frame ran out; some filtered boxes are drawn \
                 without their filter",
            ),
            (
                self.transforms,
                "transforms: the nesting depth or the work budget of layers ran out; some \
                 transformed boxes are not drawn",
            ),
            (
                self.paths,
                "inline SVG: the path work budget ran out; some shapes are not drawn",
            ),
            (
                self.dashes,
                "inline SVG: a stroke has too many dashes; it is drawn solid",
            ),
            (
                self.frames,
                "frames: the display lists of the visible frames are too long for one frame of \
                 the page, or nest too deep; some frames or groups in them are not drawn",
            ),
        ];
        for (skipped, message) in table {
            if skipped {
                log::warn!("{message}");
            }
        }
    }
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
    /// The filter work so far (see [`MAX_FILTER_PIXELS`]).
    filter_pixels: u64,
    /// The budget of `filter_pixels`.
    max_filter_pixels: u64,
    /// The pixels of the transform layers so far (see
    /// [`MAX_TRANSFORM_PIXELS`]).
    transform_pixels: u64,
    /// The budget of `transform_pixels`.
    max_transform_pixels: u64,
    /// The work of SVG paths so far (see [`path::MAX_PATH_WORK`]).
    path_work: f64,
    /// The budget of `path_work`.
    max_path_work: f64,
    /// The items of the frames drawn so far (see
    /// [`frame::MAX_FRAME_ITEMS`]).
    frame_items: u64,
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
            max_filter_pixels: MAX_FILTER_PIXELS.max(pixels.saturating_mul(8)),
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
        fixed_origin: params.viewport_scroll,
        viewport_clip: None,
        depth: 0,
        frames: FrameNesting::default(),
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
    /// What the group does to its pixels when it ends.
    kind: LayerKind,
    /// True if the layer did not fit into the budget and the group draws
    /// directly with a rectangle clip on `clips` (the fallback of a
    /// rounded overflow clip, see [`SvgClipLayer`]).
    rect_clip: bool,
}

/// The kind of a group.
enum LayerKind {
    /// An opacity group (also the temporary layer of a mask layer).
    Opacity,
    /// The group is multiplied by the mask layers.
    Mask(Arc<[MaskLayer]>),
    /// The group is multiplied by the coverage of a clip path.
    SvgClip(SvgClipLayer),
    /// The `filter` functions change the group's pixels.
    Filter(Box<FilterLayer>),
}

/// An SVG clip path with the matrix from its coordinates to the list's,
/// and whether the group falls back to a rectangle clip when its layer or
/// coverage does not fit into the budgets (a rounded overflow clip),
/// instead of hiding its content.
struct SvgClipLayer {
    clip: Arc<swb_layout::svg::ClipPath>,
    /// The matrix from the clip path's coordinates to the display list's.
    transform: Matrix,
    rect_fallback: bool,
    /// True if the group is a backdrop root (see
    /// [`DisplayItem::PushSvgClip`]).
    backdrop_root: bool,
}

/// A group that [`Rasterizer::push_layer`] starts.
struct NewLayer {
    opacity: f32,
    /// The area that the group draws in (list coordinates).
    bounds: Rect,
    /// The area of the group's content fixed to the viewport, at the
    /// viewport scroll offset.
    fixed_bounds: Option<Rect>,
    kind: LayerKind,
    /// True if the group contains content fixed to the viewport, which the
    /// enclosing clips do not clip.
    escapes_clips: bool,
}

impl NewLayer {
    /// An opaque group of `kind` in `bounds`.
    fn new(bounds: Rect, kind: LayerKind) -> NewLayer {
        NewLayer {
            opacity: 1.0,
            bounds,
            fixed_bounds: None,
            kind,
            escapes_clips: false,
        }
    }
}

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
    /// of the layer instead. Inside a frame: the frame's viewport.
    viewport_scroll: Point,
    /// Where the top-left corner of the viewport is, in the coordinates
    /// that `params` maps to device px: content fixed to the viewport is
    /// drawn from here, whatever the enclosing translations are.
    /// `viewport_scroll` for the page, the corner of the frame's content
    /// box inside a frame.
    fixed_origin: Point,
    /// The clip of the viewport of the enclosing frame (target device px,
    /// inside the clips around the frame): content fixed to the viewport
    /// does not leave it. `None` outside frames (the target is the viewport).
    viewport_clip: Option<Rect>,
    /// The number of enclosing layers of transform groups.
    depth: usize,
    /// The nesting of frames and of the groups in them.
    frames: FrameNesting,
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

/// The nesting of frames at a point of the drawing: all the rasterizers of
/// a strip share it.
#[derive(Clone, Copy, Default)]
struct FrameNesting {
    /// The number of enclosing frames.
    frames: u32,
    /// The number of enclosing transform groups inside frames (each adds
    /// to the stack of the rasterizer).
    groups: u32,
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
                let m = match transform {
                    GroupTransform::Fixed => {
                        Matrix::translate(self.fixed_origin.x, self.fixed_origin.y)
                    }
                    other => other.resolve_with(&outer, self.viewport_scroll, &mut self.sticky),
                };
                // Content fixed to the viewport in the group moves with the
                // viewport scroll offset.
                let fixed_visible = fixed_bounds.is_some_and(|f| {
                    f.translate(self.fixed_origin)
                        .intersection(&self.visible)
                        .is_some()
                });
                let visible = m.is_finite()
                    && (m.map_rect(bounds).intersection(&self.visible).is_some() || fixed_visible);
                if visible && self.enter_group() {
                    if m.is_translation() {
                        let saved = self.translation;
                        self.translation = Point::new(m.e, m.f);
                        self.run(items, i + 1..end, ends, fonts, images);
                        self.translation = saved;
                    } else {
                        self.transform_layer(items, i + 1..end, ends, &m, *bounds, fonts, images);
                    }
                    self.leave_group();
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
                fixed_origin: self.fixed_origin,
                viewport_clip: None,
                depth: self.depth + 1,
                frames: self.frames,
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
            DisplayItem::PushClip(rect) => self.push_rect_clip(*rect),
            // The target is the viewport (or a strip of it): the clip
            // hides nothing there. In a frame it is the frame's viewport.
            DisplayItem::PushViewportClip => {
                let (width, height) = self.target_size;
                let clip = self
                    .viewport_clip
                    .unwrap_or_else(|| Rect::new(0.0, 0.0, width as f32, height as f32));
                self.clips.push(clip);
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
            } => self.push_layer(NewLayer {
                opacity: *opacity,
                bounds: *bounds,
                fixed_bounds: *fixed_bounds,
                kind: LayerKind::Opacity,
                escapes_clips: *escapes_clips,
            }),
            DisplayItem::PushFilter {
                filter,
                current_color,
                bounds,
                fixed_bounds,
                escapes_clips,
            } => self.push_filter(
                filter,
                *current_color,
                *bounds,
                *fixed_bounds,
                *escapes_clips,
            ),
            // The mask is drawn inside the enclosing clips, so content
            // fixed to the viewport in the group cannot escape them.
            DisplayItem::PushMask { bounds, layers } => {
                let kind = LayerKind::Mask(Arc::clone(layers));
                self.push_layer(NewLayer::new(*bounds, kind));
            }
            DisplayItem::BackdropFilter { .. } => self.backdrop_item(item),
            DisplayItem::PopOpacity
            | DisplayItem::PopFilter
            | DisplayItem::PopMask
            | DisplayItem::PopSvgClip => {
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
            DisplayItem::Frame { node, rect, .. } => self.frame(*node, *rect, fonts, images),
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
        self.visible_in(self.surface_rect()?)
    }

    /// The part of `surface` (target device px) inside the clip.
    fn visible_in(&self, surface: Rect) -> Option<Rect> {
        match self.clips.last() {
            Some(clip) => clip.intersection(&surface),
            None => Some(surface),
        }
    }

    /// The part of the current surface that content fixed to the viewport
    /// can reach, which the enclosing clips do not clip (target device px):
    /// the surface, in a frame inside the frame's viewport.
    fn fixed_area(&self) -> Option<Rect> {
        let surface = self.surface_rect()?;
        match &self.viewport_clip {
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
        if self
            .visible_in(surface)
            .and_then(|v| v.intersection(&bounds))
            .is_none()
        {
            return;
        }
        let needs_mask = clip.is_some_and(|c| !c.contains_rect(&bounds, 0.0));
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

    /// Starts a clip to `rect` (list coordinates), inside the open clip.
    fn push_rect_clip(&mut self, rect: Rect) {
        let device = snap(self.to_device(rect));
        let clip = match self.clips.last() {
            Some(c) => c.intersection(&device).unwrap_or_default(),
            None => device,
        };
        self.clips.push(clip);
        self.mask = None;
    }

    // ----- Opacity and mask groups -----

    /// Starts an opacity, mask or clip group.
    fn push_layer(&mut self, new: NewLayer) {
        let NewLayer {
            opacity,
            bounds,
            fixed_bounds: fixed,
            kind,
            escapes_clips,
        } = new;
        self.mask = None;
        let visible = if escapes_clips {
            self.fixed_area()
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
            let at = self.to_device(fixed.translate(self.fixed_origin));
            self.translation = saved;
            device = Some(device.map_or(at, |d| d.union(&at)));
        }
        let area = visible
            .zip(device)
            .and_then(|(v, d)| snap_out(d).intersection(&v));
        // A mask group starts only if its work fits into the budget.
        let work = match (&kind, area) {
            (LayerKind::Mask(layers), Some(a)) => self.mask_work(a, layers),
            _ => 0,
        };
        let is_mask = matches!(kind, LayerKind::Mask(_));
        let area = area.filter(|_| !is_mask || self.mask_work_fits(work));
        let plain = matches!(kind, LayerKind::Opacity);
        let fallback = matches!(&kind, LayerKind::SvgClip(c) if c.rect_fallback);
        let layer = match area {
            Some(a) if opacity > 0.0 => {
                let pixmap = self.layer_pixmap(a.width as u32, a.height as u32);
                if pixmap.is_some() {
                    self.add_mask_work(work);
                }
                // A rounded overflow clip without a layer draws its content
                // directly, clipped to the rectangle.
                let rect_clip = pixmap.is_none() && fallback;
                if rect_clip {
                    self.push_rect_clip(bounds);
                }
                Layer {
                    direct: pixmap.is_none() && (plain || rect_clip),
                    pixmap,
                    origin: (a.x as i32, a.y as i32),
                    opacity,
                    kind,
                    rect_clip,
                }
            }
            _ => Layer {
                pixmap: None,
                direct: false,
                origin: (0, 0),
                opacity,
                kind,
                rect_clip: false,
            },
        };
        self.layers.push(layer);
    }

    fn pop_layer(&mut self, images: &dyn ImageSource) {
        self.mask = None;
        let Some(mut layer) = self.layers.pop() else {
            return;
        };
        if let LayerKind::Filter(_) = layer.kind {
            let LayerKind::Filter(filter) = std::mem::replace(&mut layer.kind, LayerKind::Opacity)
            else {
                return;
            };
            self.pop_filter(layer, *filter);
            return;
        }
        let Some(mut pixmap) = layer.pixmap else {
            if layer.rect_clip {
                self.clips.pop();
                self.mask = None;
            }
            return;
        };
        let visible = match &layer.kind {
            LayerKind::Opacity | LayerKind::Filter(_) => true,
            LayerKind::Mask(layers) => self.apply_mask(&mut pixmap, layer.origin, layers, images),
            LayerKind::SvgClip(c) => {
                match self.apply_svg_clip(&mut pixmap, layer.origin, &c.clip, &c.transform) {
                    ClipResult::Applied => true,
                    ClipResult::Hidden => false,
                    // The content stays inside the layer, which is the
                    // rectangle of the clip: only the corners stay square.
                    ClipResult::OverBudget => c.rect_fallback,
                }
            }
        };
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
            ImageKind::Raster(pixmap) => {
                return self.image(pixmap, Some(image.reductions()), (area, tile, clip));
            }
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
            self.image(&pixmap, None, (area, tile, clip));
        }
    }

    /// Draws `pixmap` as `tile` inside `area` and `clip`. With `levels`, a
    /// large reduction is drawn from a reduced level (see [`crate::reduce`]).
    fn image(
        &mut self,
        pixmap: &Pixmap,
        levels: Option<&Reductions>,
        (area, tile, clip): (Rect, Rect, Rect),
    ) {
        if tile.width <= 0.0 || tile.height <= 0.0 {
            return;
        }
        let Some(area) = area.intersection(&clip) else {
            return;
        };
        let tiled = is_tiled(area, tile);
        let tile = self.to_device(tile);
        // Chromium draws a single image into its pixel-snapped rectangle.
        let tile = if tiled { tile } else { snap(tile) };
        let area = snap(self.to_device(area));
        let Some(r) = skia_rect(area) else {
            return;
        };
        // A large reduction is drawn from an averaged level: bilinear
        // sampling of the original skips most source pixels and aliases
        // (thin lines drop out).
        let reduced = levels.and_then(|levels| {
            levels.level(
                pixmap,
                tile.width / pixmap.width() as f32,
                tile.height / pixmap.height() as f32,
                &mut self.vectors.reduce,
            )
        });
        let pixmap = reduced.as_deref().unwrap_or(pixmap);
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
            // A single stop or degenerate line.
            if let Some(c) = degenerate_color(gradient) {
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
                    self.image(&pixmap, None, (area, tile, area));
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
    const K: f32 = swb_layout::KAPPA;
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
mod tests;
