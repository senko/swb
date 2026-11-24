//! Composited transform groups (ADR 0044 section 19): the transform group
//! of an element whose `transform` or `opacity` is animated draws its
//! content once into a layer at a fixed scale, which the raster cache
//! keeps; each frame draws that layer with the group's transform, with the
//! opacity of the element's own opacity group.
//!
//! - The layer covers the whole group (its bounds), at the device scale
//!   times the group's scale rounded up to a power of two (at least 1), so
//!   that neither the transform nor the scroll offset changes it. A group
//!   whose layer would be larger than the limits of transform layers is
//!   drawn as a transform layer instead.
//! - The element's own opacity group (the first item of the group, ending
//!   with its last item, without a blend mode or fixed content) is not drawn
//!   into the layer: its opacity applies when the layer is drawn. With an
//!   opacity of 0 the layer is made (and kept) but not drawn, so that the
//!   first visible frame of a fade-in only draws the layer.
//! - The layer is drawn as the layer of a transform group is: bilinear
//!   filtering, anti-aliased edges, inside the enclosing clips.
//! - A layer that the cache does not have is drawn whole and kept, so that
//!   the frames after a change of the content (also a change while the
//!   group is invisible, before a fade-in) draw only the layer. A raster of
//!   parts whose layer missed in the last [`MAX_WHOLE_MISSES`] rasters (the
//!   content changes in every frame) draws only the part that the parts
//!   read, as for a transform layer, and keeps nothing.
//! - A group with content fixed to the viewport, a group nested deeper than
//!   the limit of transform layers, and a translation are drawn as before.

use std::sync::Arc;

use swb_layout::{GroupTransform, Matrix, Rect};
use swb_style::MixBlendMode;
use swb_text::FontContext;
use tiny_skia::Pixmap;

use super::cache::{Charges, LayerKey, LayerParts, rect_bits};
use super::region::Region;
use super::transform_layer::GroupItems;
use super::{
    ImageSource, MAX_TRANSFORM_DEPTH, MAX_TRANSFORM_LAYER_PIXELS, MAX_TRANSFORM_LAYER_SIDE,
    Rasterizer,
};
use crate::damage::item_media_key;
use crate::display_list::DisplayItem;

/// The largest power of two that the scale of a composited layer gets
/// from its group's scale.
const MAX_GROUP_SCALE: f32 = 8.0;

/// The misses in a row after which a raster of parts draws only the part
/// of a composited layer that it needs, and does not keep it: the content
/// changes in every frame (a spinner in a fading group), so keeping it
/// would cost a whole layer per frame.
const MAX_WHOLE_MISSES: u32 = 3;

/// What [`Rasterizer::composited_layer`] did.
pub(super) enum Composited {
    /// The group is drawn (or draws nothing).
    Done,
    /// The group cannot be composited: draw it as a transform layer.
    Fallback,
}

/// Where and how large the layer of a composited group is.
struct CompositedPlan {
    /// The group's bounds (its coordinates): the area of the layer.
    bounds: Rect,
    /// Layer px per CSS px of the group.
    scale: f32,
    width: u32,
    height: u32,
    /// From the group's coordinates to target device px.
    to_device: Matrix,
    /// The cost for the work budget of transform layers.
    work: u64,
}

impl CompositedPlan {
    /// From layer px to target device px.
    fn to_target(&self) -> Matrix {
        let k = 1.0 / self.scale;
        self.to_device
            .multiply(&Matrix::new(k, 0.0, 0.0, k, self.bounds.x, self.bounds.y))
    }
}

impl Rasterizer<'_> {
    /// Draws `group`, the content of a composited transform group whose
    /// items `transform` maps to the coordinates of `params` and that
    /// draws in `bounds` (its own coordinates); see the module
    /// documentation.
    pub(super) fn composited_layer(
        &mut self,
        group: GroupItems<'_>,
        transform: &Matrix,
        bounds: Rect,
        fonts: &mut FontContext,
        images: &dyn ImageSource,
    ) -> Composited {
        let Some(plan) = self.plan_composited(transform, bounds) else {
            return Composited::Fallback;
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Composited::Done;
        }
        if self.budget.transform_pixels.saturating_add(plan.work) > self.budget.max_transform_pixels
        {
            self.budget.skipped.transforms = true;
            return Composited::Done;
        }
        let GroupItems { items, range, ends } = group;
        let (content, alpha) = own_opacity(items, range);
        let Some(layer) = self.composited_pixels(items, content, ends, &plan, (fonts, images))
        else {
            return Composited::Done;
        };
        self.budget.transform_pixels += plan.work;
        // An opacity of 0 draws nothing, but the layer is ready for the
        // frames that show it (an element that fades in after a delay).
        if alpha > 0.0 {
            self.draw_transformed(&layer, &plan.to_target(), alpha);
        }
        Composited::Done
    }

    /// The layer of a composited group with the transform `transform` and
    /// the bounds `bounds`, if it is within the limits of transform layers.
    fn plan_composited(&self, transform: &Matrix, bounds: Rect) -> Option<CompositedPlan> {
        if self.depth >= MAX_TRANSFORM_DEPTH {
            return None;
        }
        let device_scale = self.params.scale;
        let scale = group_layer_scale(device_scale, transform)?;
        let (width, height) = group_layer_size(bounds, scale)?;
        let scroll = self.params.scroll;
        let to_device = Matrix::new(
            device_scale,
            0.0,
            0.0,
            device_scale,
            -scroll.x * device_scale,
            -scroll.y * device_scale - self.offset_y,
        )
        .multiply(transform);
        // The work of a transform layer of this size.
        let covered = self.visible_area().map_or(0, |visible| {
            to_device
                .map_rect(&bounds)
                .intersection(&visible)
                .map_or(0, |r| (r.width as u64).saturating_mul(r.height as u64))
        });
        let work = (u64::from(width) * u64::from(height)).saturating_add(covered);
        Some(CompositedPlan {
            bounds,
            scale,
            width,
            height,
            to_device,
            work,
        })
    }

    /// The pixels of the layer of `plan` with the content `items[content]`:
    /// the kept layer, if the raster cache has one with the same inputs and
    /// its charges fit into the budgets; else drawn (and kept when drawn
    /// whole). `None` if nothing is drawn.
    fn composited_pixels(
        &mut self,
        items: &[DisplayItem],
        content: std::ops::Range<usize>,
        ends: &[usize],
        plan: &CompositedPlan,
        (fonts, images): (&mut FontContext, &dyn ImageSource),
    ) -> Option<Arc<Pixmap>> {
        let slice = items.get(content.clone())?;
        let parts = self.layer_parts(slice, plan, images);
        let cache = images.raster_cache();
        // The charges of a kept layer include its own pixels, which are open
        // while a raster without the cache draws its content.
        if let Some((layer, charges)) = cache.and_then(|c| c.layer(slice, &parts))
            && self.replay_charges(&charges)
        {
            return Some(layer);
        }
        // A layer that is not kept is drawn whole and kept; but in a raster
        // of parts, a layer whose content changed in the last rasters too
        // (or without a cache) is drawn only where the parts read it, as the
        // layer of a transform group is.
        let churns = cache.is_none_or(|c| c.layer_misses(&parts) > MAX_WHOLE_MISSES);
        let part = match &self.region {
            Some(region) if churns => Some(super::transform_layer::layer_part(
                &plan.to_device,
                plan.bounds,
                plan.scale,
                region.bounds(),
            )?),
            _ => None,
        };
        let whole = part.is_none();
        let (layer, charges) =
            self.draw_content(items, content, ends, plan, part, (fonts, images))?;
        let layer = Arc::new(layer);
        if let (Some(cache), Some(charges), true) = (cache, charges, whole) {
            let key = LayerKey {
                items: Arc::from(slice),
                parts,
            };
            cache.keep_layer(key, Arc::clone(&layer), charges);
        }
        Some(layer)
    }

    /// The inputs of the layer of `plan` other than its items `slice`.
    fn layer_parts(
        &self,
        slice: &[DisplayItem],
        plan: &CompositedPlan,
        images: &dyn ImageSource,
    ) -> LayerParts {
        let viewport = if depends_on_viewport(slice) {
            let (scroll, origin) = (self.viewport_scroll, self.fixed_origin);
            [scroll.x, scroll.y, origin.x, origin.y].map(f32::to_bits)
        } else {
            [0; 4]
        };
        LayerParts {
            media: slice
                .iter()
                .enumerate()
                .filter_map(|(i, item)| item_media_key(item, images).map(|key| (i, key)))
                .collect(),
            area: rect_bits(&plan.bounds),
            scale: plan.scale.to_bits(),
            viewport,
            nesting: (self.depth, self.frames.frames, self.frames.groups),
        }
    }

    /// Draws `items[content]` into a new layer of `plan`, as the layer of
    /// a transform group is drawn (with `part`, only that part: the visible
    /// area in the group's coordinates and the region in layer px), and
    /// returns it with what the drawing charged to the budgets (`None` if a
    /// budget left something out). `None` if the layer does not fit into
    /// the budget.
    fn draw_content(
        &mut self,
        items: &[DisplayItem],
        content: std::ops::Range<usize>,
        ends: &[usize],
        plan: &CompositedPlan,
        part: Option<(Rect, Option<Region>)>,
        (fonts, images): (&mut FontContext, &dyn ImageSource),
    ) -> Option<(Pixmap, Option<Charges>)> {
        let (visible, region) = part.unwrap_or((plan.bounds, None));
        let mark = self.mark_charges();
        let Some(mut layer) = self.layer_pixmap(plan.width, plan.height) else {
            self.end_charges(mark);
            return None;
        };
        let group = GroupItems {
            items,
            range: content,
            ends,
        };
        self.draw_nested(
            &mut layer,
            (plan.bounds.origin(), plan.scale),
            (visible, region),
            group,
            (fonts, images),
        );
        self.release(&layer);
        let charges = self.end_charges(mark);
        Some((layer, charges))
    }
}

/// The scale of the layer of a composited group (layer px per CSS px of the
/// group) for the device scale `s` and the group's transform `m`: `s` times
/// the group's largest scale factor rounded up to a power of two (1 for
/// scales up to 1, at most [`MAX_GROUP_SCALE`]). `None` for a transform
/// that is not finite or has no scale.
fn group_layer_scale(s: f32, m: &Matrix) -> Option<f32> {
    let scale = m.max_scale();
    if !(scale.is_finite() && scale > 0.0 && s.is_finite() && s > 0.0) {
        return None;
    }
    let mut power = 1.0;
    while power < scale && power < MAX_GROUP_SCALE {
        power *= 2.0;
    }
    Some(s * power)
}

/// The size in px of a layer that covers `bounds` at `k` px per CSS px, if
/// it is within the limits of transform layers.
fn group_layer_size(bounds: Rect, k: f32) -> Option<(u32, u32)> {
    let width = (bounds.width * k).ceil().max(1.0);
    let height = (bounds.height * k).ceil().max(1.0);
    let fits = width.is_finite()
        && height.is_finite()
        && width <= MAX_TRANSFORM_LAYER_SIDE
        && height <= MAX_TRANSFORM_LAYER_SIDE
        && width * height <= MAX_TRANSFORM_LAYER_PIXELS;
    fits.then_some((width as u32, height as u32))
}

/// The content of the group `items[range]` without the element's own
/// opacity group, and that group's opacity: if the first item starts an
/// opacity group without a blend mode or fixed content that ends with the
/// last item. Else all of it, with opacity 1.
fn own_opacity(
    items: &[DisplayItem],
    range: std::ops::Range<usize>,
) -> (std::ops::Range<usize>, f32) {
    let whole = (range.clone(), 1.0);
    let Some(DisplayItem::PushOpacity {
        opacity,
        blend: MixBlendMode::Normal,
        fixed_bounds: None,
        escapes_clips: false,
        ..
    }) = items.get(range.start)
    else {
        return whole;
    };
    let mut depth = 0usize;
    for i in range.clone() {
        match items.get(i) {
            Some(DisplayItem::PushOpacity { .. }) => depth += 1,
            Some(DisplayItem::PopOpacity) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return if i + 1 == range.end {
                        (range.start + 1..i, *opacity)
                    } else {
                        whole
                    };
                }
            }
            _ => {}
        }
    }
    whole
}

/// True if what `items` draw depends on the viewport scroll offset: they
/// have fixed or sticky boxes, or content fixed to the viewport.
fn depends_on_viewport(items: &[DisplayItem]) -> bool {
    items.iter().any(|item| match item {
        DisplayItem::PushTransform {
            transform,
            fixed_bounds,
            ..
        } => !matches!(transform, GroupTransform::Matrix(_)) || fixed_bounds.is_some(),
        DisplayItem::PushOpacity {
            fixed_bounds,
            escapes_clips,
            ..
        }
        | DisplayItem::PushFilter {
            fixed_bounds,
            escapes_clips,
            ..
        } => fixed_bounds.is_some() || *escapes_clips,
        DisplayItem::Frame { .. } => true,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layer_scale_is_a_power_of_two_of_the_group_scale() {
        let scaled = |k: f32| Matrix::new(k, 0.0, 0.0, k, 3.5, 7.25);
        assert_eq!(group_layer_scale(1.0, &scaled(0.99)), Some(1.0));
        assert_eq!(group_layer_scale(1.5, &scaled(0.2)), Some(1.5));
        assert_eq!(group_layer_scale(1.0, &scaled(1.01)), Some(2.0));
        assert_eq!(group_layer_scale(2.0, &scaled(3.0)), Some(8.0));
        assert_eq!(group_layer_scale(1.0, &scaled(1000.0)), Some(8.0));
        assert_eq!(group_layer_scale(1.0, &scaled(f32::NAN)), None);
        assert_eq!(group_layer_scale(1.0, &scaled(0.0)), None);
    }

    #[test]
    fn the_own_opacity_group_is_folded_only_when_it_holds_the_whole_group() {
        let push = |opacity| DisplayItem::opacity_group(opacity, MixBlendMode::Normal);
        let rect = DisplayItem::Rect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            radii: [(0.0, 0.0); 4],
            color: swb_style::Rgba::BLACK,
        };
        let items = vec![
            push(0.5),
            rect.clone(),
            push(0.25),
            rect.clone(),
            DisplayItem::PopOpacity,
            DisplayItem::PopOpacity,
        ];
        assert_eq!(own_opacity(&items, 0..6), (1..5, 0.5));
        // The group ends before the last item.
        let mut more = items.clone();
        more.push(rect.clone());
        assert_eq!(own_opacity(&more, 0..7), (0..7, 1.0));
        // A blend mode stays in the layer.
        let blended = vec![
            DisplayItem::opacity_group(0.5, MixBlendMode::Multiply),
            rect,
            DisplayItem::PopOpacity,
        ];
        assert_eq!(own_opacity(&blended, 0..3), (0..3, 1.0));
    }
}
