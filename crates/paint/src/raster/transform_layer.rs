//! Transform groups drawn through a layer (see the module documentation of
//! `raster`): the layer's place and size on the grid of layer pixels, the
//! part of it that a raster of parts reads, and the drawing of the layer
//! with the group's transform.

use swb_layout::{Matrix, Point, Rect};
use tiny_skia::{FilterQuality, Paint, Pattern, Pixmap, SpreadMode, Transform};

use super::region::Region;
use super::{
    FontContext, ImageSource, MAX_TRANSFORM_DEPTH, MAX_TRANSFORM_LAYER_PIXELS,
    MAX_TRANSFORM_LAYER_SIDE, RasterParams, Rasterizer, snap_out,
};
use crate::display_list::DisplayItem;

/// `area` (a part of `bounds`, in a group's coordinates) grown by two
/// layer pixels on every side for the scale `k` (layer px per CSS px),
/// inside `bounds`, with its top-left corner on the grid of layer pixels
/// that starts at the top-left corner of `bounds`. The layers of a group in
/// different strips then have pixels at the same positions, and the
/// filtering at the layer's edges reads pixels of the group's content.
pub(super) fn grid_area(area: Rect, bounds: Rect, k: f32) -> Rect {
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

/// The part of the layer of a transform group that the pixels `region`
/// (target device px) of a raster of parts read: `region` grown by 2 device
/// px (the filtering and the anti-aliased edges), mapped to the group's
/// coordinates (`to_device` maps them to the target), grown by 2 layer px,
/// inside `area` (the layer, at `k` layer px per CSS px). Returns it as the
/// visible area of the layer's rasterizer (the group's coordinates) and as
/// its region (layer px); `None` if the region does not reach the layer.
pub(super) fn layer_part(
    to_device: &Matrix,
    area: Rect,
    k: f32,
    region: Rect,
) -> Option<(Rect, Option<Region>)> {
    let grown = Rect::new(
        region.x - 2.0,
        region.y - 2.0,
        region.width + 4.0,
        region.height + 4.0,
    );
    let local = to_device.invert()?.map_rect(&grown);
    let m = 2.0 / k;
    let local = Rect::new(
        local.x - m,
        local.y - m,
        local.width + 2.0 * m,
        local.height + 2.0 * m,
    )
    .intersection(&area)?;
    let layer = snap_out(Rect::new(
        (local.x - area.x) * k,
        (local.y - area.y) * k,
        local.width * k,
        local.height * k,
    ));
    Some((local, Region::new(vec![layer])))
}

/// The scale (layer px per CSS px of the group) of the layer of a
/// transform group that covers `area` (the group's coordinates), for the
/// scale `k` of the device resolution: limited so that the layer has sides
/// of at most [`MAX_TRANSFORM_LAYER_SIDE`] and at most
/// [`MAX_TRANSFORM_LAYER_PIXELS`] pixels. `None` for a scale that is not
/// finite and positive.
pub(super) fn transform_scale(area: Rect, mut k: f32) -> Option<f32> {
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
/// it is within the limits of [`transform_scale`] (with room for rounding).
pub(super) fn transform_size(area: Rect, k: f32) -> Option<(u32, u32)> {
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
    /// The cost for [`super::MAX_TRANSFORM_PIXELS`].
    work: u64,
}

/// The items of a group: `items[range]`, with the group ends of
/// [`transform_ends`].
pub(super) struct GroupItems<'a> {
    pub(super) items: &'a [DisplayItem],
    pub(super) range: std::ops::Range<usize>,
    pub(super) ends: &'a [usize],
}

impl Rasterizer<'_> {
    /// Draws `group`, a transform group whose items `m` maps to the
    /// coordinates of `params` and that draws in `bounds` (its own
    /// coordinates), through a layer (see the module documentation).
    pub(super) fn transform_layer(
        &mut self,
        group: GroupItems<'_>,
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
        // In a raster of parts, the layer has its place and size of a full
        // raster (its resampling depends on them), but only its pixels that
        // the part reads are drawn.
        let (visible, region) = match &self.region {
            None => (area, None),
            Some(region) => match layer_part(&to_device, area, k, region.bounds()) {
                Some(part) => part,
                None => return,
            },
        };
        let Some(mut layer) = self.layer_pixmap(layer_width, layer_height) else {
            return;
        };
        self.budget.transform_pixels += work;
        self.draw_nested(
            &mut layer,
            (area.origin(), k),
            (visible, region),
            group,
            (fonts, images),
        );
        // Layer px to target device px.
        let to_target =
            to_device.multiply(&Matrix::new(1.0 / k, 0.0, 0.0, 1.0 / k, area.x, area.y));
        self.draw_transformed(&layer, &to_target, 1.0);
        self.release(&layer);
    }

    /// Draws `group` into `layer` with a rasterizer of its own: `origin` is
    /// the group's coordinate of the layer's first pixel and `scale` the
    /// layer px per CSS px; `visible` and `region` are the part that is
    /// drawn (in the group's coordinates and in layer px). The groups
    /// inside the layer share the budgets of the strip. Fixed and sticky
    /// groups inside it still move with the viewport.
    pub(super) fn draw_nested(
        &mut self,
        layer: &mut Pixmap,
        (origin, scale): (Point, f32),
        (visible, region): (Rect, Option<Region>),
        group: GroupItems<'_>,
        (fonts, images): (&mut FontContext, &dyn ImageSource),
    ) {
        let budget = std::mem::take(&mut self.budget);
        let size = (layer.width(), layer.height());
        self.budget = {
            let mut nested = Rasterizer {
                target: layer.data_mut(),
                target_size: size,
                offset_y: 0.0,
                params: RasterParams {
                    scroll: origin,
                    viewport_scroll: self.viewport_scroll,
                    scale,
                },
                layers: Vec::new(),
                budget,
                clips: Vec::new(),
                mask: None,
                vectors: &mut *self.vectors,
                translation: Point::default(),
                visible,
                region,
                viewport_scroll: self.viewport_scroll,
                fixed_origin: self.fixed_origin,
                viewport_clip: None,
                depth: self.depth + 1,
                frames: self.frames,
                sticky: std::mem::take(&mut self.sticky),
                image_blend: None,
            };
            nested.run(group.items, group.range, group.ends, fonts, images);
            while !nested.layers.is_empty() {
                nested.pop_layer(images, fonts);
            }
            self.sticky = std::mem::take(&mut nested.sticky);
            nested.budget
        };
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
        let k = transform_scale(visible_part, s * m.max_scale())?;
        let k = transform_scale(grid_area(visible_part, bounds, k), k)?;
        let area = grid_area(visible_part, bounds, k);
        let (width, height) = transform_size(area, k)?;
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
    /// (`draw_pixmap` does not anti-alias them), with the opacity `alpha`.
    pub(super) fn draw_transformed(&mut self, layer: &Pixmap, to_target: &Matrix, alpha: f32) {
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
                alpha,
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
}
