//! The entry points of the rasterizer: a whole target, a tall target in
//! strips, and parts of a target (a region), and the rasterization of one
//! strip with its own budgets.

use swb_layout::{Point, Rect, StickyCache};
use swb_style::Rgba;
use swb_text::FontContext;
use tiny_skia::Pixmap;

use super::region::Region;
use super::{
    Budget, FrameNesting, ImageSource, MAX_FILTER_PIXELS, MAX_GROUP_LAYER_PIXELS, MAX_MASK_PIXELS,
    MAX_TRANSFORM_PIXELS, PixelRect, REGION_SLACK, ROW_COST_PIXELS, RasterParams, RasterReport,
    Rasterizer, Skipped, path, to_skia_color,
};
use crate::display_list::DisplayList;
use crate::group_bounds::transform_ends;
use crate::svg::FrameBudget;

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
) -> RasterReport {
    let rows = target.height();
    rasterize_in_strips(list, target, params, rows, fonts, images)
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
) -> RasterReport {
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
        let s = rasterize_strip(
            list,
            &ends,
            strip,
            params,
            None,
            &mut vectors,
            fonts,
            images,
        );
        skipped.merge(s);
    }
    finish_report(&vectors, skipped)
}

/// Logs what the budgets left out, and makes the report.
fn finish_report(vectors: &FrameBudget, skipped: Skipped) -> RasterReport {
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
    RasterReport {
        incomplete: vectors.skipped() || vectors.reduce.skipped() || skipped.any(),
    }
}

/// Rasterizes the parts `rects` of `target` (device px; clamped to the
/// target) and leaves its other pixels as they are (ADR 0044 section 18).
/// Each part is filled with `background` first (`None`: the part keeps the
/// target's pixels below the list, for lists drawn over the page) and
/// drawn as a full [`rasterize`] of the target draws it: the pixels inside
/// the parts equal those of a full raster bit for bit, if the target's
/// pixels there were those of the full raster below the list.
///
/// How: overlapping parts become their bounding box, and all parts are
/// drawn in one pass into a scratch pixmap of the target's size, so that
/// every pixmap that the rasterizer uses (the target, the layers of groups)
/// has the size and position that it has in a full raster; only the items
/// are culled to the parts, and the drawing whose pixels do not depend on
/// its extent (rectangles, images, gradients, glyphs, the composite of
/// layers) is limited to them. Anti-aliased paths, clip paths, masks and
/// the layers of transforms are drawn as in a full raster, so their pixels
/// do not depend on the parts. A filter draws its content in the parts
/// grown by what the filter reads. The pass gets the budgets of the whole
/// target, so it draws everything a full raster draws (it draws fewer
/// items). A caller must make each part that touches the box of a
/// `backdrop-filter` hold all of it (`crate::damage`): the filter reads the
/// whole box.
pub fn rasterize_region(
    list: &DisplayList,
    target: &mut Pixmap,
    params: RasterParams,
    rects: &[PixelRect],
    background: Option<Rgba>,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) -> RasterReport {
    let (width, height) = (target.width(), target.height());
    let parts: Vec<PixelRect> = rects
        .iter()
        .map(|r| r.clamp_to(width, height))
        .filter(|r| !r.is_empty())
        .collect();
    if parts.is_empty() {
        return RasterReport::default();
    }
    // The pixmap of the last raster of parts, if it has this size: its
    // pixels outside the parts do not matter.
    let cache = images.raster_cache();
    let scratch = match cache {
        Some(cache) => cache.take_scratch(width, height),
        None => Pixmap::new(width, height),
    };
    let Some(mut scratch) = scratch else {
        return RasterReport { incomplete: true };
    };
    // One pass for all parts, so that the work of a drawing that does not
    // depend on the parts (a layer's clip path, a filter) is done once. The
    // parts are made disjoint, so that each pixel is drawn once.
    let Some(region) = Region::new(parts.iter().map(|p| p.to_rect()).collect()) else {
        if let Some(cache) = cache {
            cache.put_scratch(scratch);
        }
        return RasterReport::default();
    };
    let parts: Vec<PixelRect> = region
        .pieces(region.bounds())
        .into_iter()
        .map(pixel_part)
        .collect();
    for &part in &parts {
        match background {
            Some(color) => fill_part(&mut scratch, part, color),
            None => copy_part(target, &mut scratch, part),
        }
    }
    let mut vectors = FrameBudget::new();
    let ends = transform_ends(&list.items);
    let strip = Strip {
        data: scratch.data_mut(),
        width,
        height,
        top: 0,
        budget_rows: height,
    };
    let skipped = rasterize_strip(
        list,
        &ends,
        strip,
        params,
        Some(region),
        &mut vectors,
        fonts,
        images,
    );
    for &part in &parts {
        copy_part(&scratch, target, part);
    }
    if let Some(cache) = cache {
        cache.put_scratch(scratch);
    }
    finish_report(&vectors, skipped)
}

/// A rectangle of whole device px as a [`PixelRect`].
fn pixel_part(r: Rect) -> PixelRect {
    let edge = |v: f32| v.max(0.0) as u32;
    PixelRect::from_edges(edge(r.x), edge(r.y), edge(r.right()), edge(r.bottom()))
}

/// Fills the pixels of `part` (inside `target`) with `color`.
fn fill_part(target: &mut Pixmap, part: PixelRect, color: Rgba) {
    let pixel = to_skia_color(color).premultiply().to_color_u8();
    let bytes = [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()];
    let width = target.width() as usize;
    let data = target.data_mut();
    for row in part.y..part.bottom() {
        let from = (row as usize * width + part.x as usize) * 4;
        let to = from + part.width as usize * 4;
        if let Some(line) = data.get_mut(from..to) {
            for px in line.as_chunks_mut::<4>().0 {
                *px = bytes;
            }
        }
    }
}

/// Copies the pixels of `part` from `from` to `to` (two pixmaps of one
/// size).
fn copy_part(from: &Pixmap, to: &mut Pixmap, part: PixelRect) {
    let width = from.width() as usize;
    if to.width() as usize != width || to.height() != from.height() {
        return;
    }
    let src = from.data();
    let dst = to.data_mut();
    for row in part.y..part.bottom() {
        let start = (row as usize * width + part.x as usize) * 4;
        let end = start + part.width as usize * 4;
        if let (Some(a), Some(b)) = (src.get(start..end), dst.get_mut(start..end)) {
            b.copy_from_slice(a);
        }
    }
}

/// The height of equal strips for a target of `height` rows and strips of
/// at most `max_rows` rows: `height` split into the fewest strips, as
/// equal as possible (the last one can be lower).
pub(super) fn equal_strip_rows(height: u32, max_rows: u32) -> u32 {
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

/// Rasterizes one strip of the target. `ends` comes from [`transform_ends`].
/// With `region` (device px of the strip), only the pixels there matter
/// (see [`rasterize_region`]).
#[allow(clippy::too_many_arguments)]
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
    region: Option<Region>,
    vectors: &mut FrameBudget,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) -> Skipped {
    // The strip (or the region) in CSS px, 1 device px larger on every
    // side: snapping can move a thin rectangle into the next row.
    let s = params.scale;
    let viewport = match region.as_ref().map(Region::bounds) {
        None => Rect::new(
            params.scroll.x - 1.0 / s,
            params.scroll.y + (top as f32 - 1.0) / s,
            (width as f32 + 2.0) / s,
            (height as f32 + 2.0) / s,
        ),
        // A region culls with more slack (see `REGION_SLACK`).
        Some(area) => {
            let m = REGION_SLACK + 1.0;
            Rect::new(
                params.scroll.x + (area.x - m) / s,
                params.scroll.y + (top as f32 + area.y - m) / s,
                (area.width + 2.0 * m) / s,
                (area.height + 2.0 * m) / s,
            )
        }
    };
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
        region,
        viewport_scroll: params.viewport_scroll,
        fixed_origin: params.viewport_scroll,
        viewport_clip: None,
        depth: 0,
        frames: FrameNesting::default(),
        sticky: StickyCache::default(),
        image_blend: None,
    };
    r.run(&list.items, 0..list.items.len(), ends, fonts, images);
    // Close unbalanced groups.
    while !r.layers.is_empty() {
        r.pop_layer(images, fonts);
    }
    r.budget.skipped
}
