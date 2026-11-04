//! Backdrop filters (`backdrop-filter`, Filter Effects 2 §2,
//! <https://drafts.fxtf.org/filter-effects-2/#BackdropFilterProperty>): the
//! pixels that the backdrop root holds inside the border box of the box go
//! through the filter steps (`crate::filter`), and the result is drawn over
//! the same area, below the box. ADR 0033 has the design and the
//! measurements.
//!
//! - The backdrop root is the nearest layer below the layers that the box
//!   started itself (mask, clip path and filter apply to the box and its
//!   filtered backdrop together; the opacity too, except that it applies
//!   to the backdrop and the box separately, see `DisplayItem`); without a
//!   layer it is the target. The layers of opacity, masks, filters and
//!   clip paths are backdrop roots, as in Chromium. The layer of a rounded
//!   `overflow` clip is not: its content is drawn over the root's pixels.
//!   Departures: a transform layer (a rotation, a scale, a skew) is a root
//!   (in Chromium the backdrop reaches through it), and the backdrop ends
//!   at the edge of the surface (the viewport or the page): Chromium reads
//!   the content beyond it when the box reaches beyond it.
//! - The backdrop is clipped to the border box, snapped to device pixels
//!   (a rounded box takes the pixels it touches and a coverage mask of its
//!   rounded outline). A blur does not read beyond the box: it mirrors the
//!   image at the edges ([`crate::filter::Edge::Mirror`]).
//! - The result is composited over the backdrop root's pixels (source
//!   over: `opacity(0)` leaves the backdrop as it is), inside the enclosing
//!   clips.
//! - The steps count against the filter work budget and the layer budget as
//!   a filter group does. A backdrop that does not fit stays unfiltered.

use swb_layout::Rect;
use swb_style::{FilterFunction, Rgba};
use tiny_skia::{
    FillRule, FilterQuality, Paint, Pattern, Pixmap, PixmapPaint, SpreadMode, Transform,
};

use super::filter_layer::grow;
use super::{
    Layer, LayerKind, Rasterizer, rounded_rect_path, scale_radii, skia_rect, snap, snap_out,
};
use crate::display_list::{DisplayItem, Radii};
use crate::filter;

/// The pixels of the backdrop root.
struct Root<'a> {
    /// Premultiplied RGBA rows.
    data: &'a [u8],
    /// The width in pixels.
    width: usize,
    /// The root's place on the target (device px).
    rect: Rect,
}

impl Root<'_> {
    /// Copies the part `area` (whole device px inside `rect`) into `into`,
    /// which has the size of `area`.
    fn copy(&self, area: Rect, into: &mut Pixmap) {
        let width = into.width() as usize;
        let rows = into.height() as usize;
        let x = (area.x - self.rect.x) as usize;
        let y = (area.y - self.rect.y) as usize;
        let out = into.data_mut();
        for row in 0..rows {
            let from = ((y + row) * self.width + x) * 4;
            if let (Some(src), Some(dst)) = (
                self.data.get(from..from + width * 4),
                out.get_mut(row * width * 4..(row + 1) * width * 4),
            ) {
                dst.copy_from_slice(src);
            }
        }
    }
}

impl Rasterizer<'_> {
    /// Draws the filtered backdrop that the item `item` (a
    /// [`DisplayItem::BackdropFilter`]) describes.
    pub(super) fn backdrop_item(&mut self, item: &DisplayItem) {
        if let DisplayItem::BackdropFilter {
            filter,
            current_color,
            rect,
            radii,
            alpha,
            layers,
        } = item
        {
            self.backdrop_filter(
                filter,
                *current_color,
                (*rect, radii.as_ref()),
                (*layers, *alpha),
            );
        }
    }

    /// Draws the filtered backdrop of a box: `rect` is its border box
    /// (list coordinates), `radii` its corner radii if it is rounded,
    /// `own_layers` the number of layers that the box started before, and
    /// `alpha` the opacity of the drawing.
    pub(super) fn backdrop_filter(
        &mut self,
        list: &[FilterFunction],
        current_color: Rgba,
        (rect, radii): (Rect, Option<&Radii>),
        (own_layers, alpha): (u8, f32),
    ) {
        let Some(surface) = self.surface_rect() else {
            return;
        };
        let ops = filter::device_ops(list, current_color, self.params.scale);
        if ops.is_empty() {
            return;
        }
        let device = self.to_device(rect);
        let area = if radii.is_some() {
            snap_out(device)
        } else {
            snap(device)
        };
        // The backdrop: the box inside the root. The output: the part that
        // can be seen. The working area adds what the steps read around it.
        let Some(root_rect) = self.backdrop_root(own_layers).map(|(root, _)| root.rect) else {
            return;
        };
        let Some(source) = area.intersection(&root_rect) else {
            return;
        };
        let Some(output) = self
            .visible_in(surface)
            .and_then(|v| v.intersection(&source))
            .filter(|r| r.width >= 1.0 && r.height >= 1.0)
        else {
            return;
        };
        let working = grow(output, filter::backward_reach(&ops))
            .intersection(&source)
            .unwrap_or(output);
        let Some((mut pixmap, scratch)) = self.filter_layer(&ops, working) else {
            return;
        };
        self.read_backdrop(own_layers, working, &mut pixmap);
        let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
        // The mirrored blur needs no buffer from here: it makes its own,
        // about as large as the one that the budget reserved.
        filter::apply_mirrored(pixmap.data_mut(), width, height, &ops);
        self.release(&pixmap);
        if scratch {
            let pixels = (width as u64) * (height as u64);
            self.budget.layer_pixels = self.budget.layer_pixels.saturating_sub(pixels);
        }
        self.draw_backdrop(&pixmap, working, (device, output), (radii, alpha));
    }

    /// The pixels that the backdrop of a box comes from: the nearest layer
    /// below the `own_layers` innermost layers that is a backdrop root and
    /// draws (not a direct one), or the target; and the layers above it
    /// that are not roots (the rounded `overflow` clip of an ancestor),
    /// which the backdrop includes. `None` while drawing is skipped.
    fn backdrop_root(&self, own_layers: u8) -> Option<(Root<'_>, &[Layer])> {
        let below = self.layers.len().saturating_sub(usize::from(own_layers));
        let stack = self.layers.get(..below)?;
        let is_root =
            |l: &Layer| !l.direct && !matches!(&l.kind, LayerKind::SvgClip(c) if !c.backdrop_root);
        let Some(at) = stack.iter().rposition(is_root) else {
            let root = Root {
                data: self.target,
                width: self.target_size.0 as usize,
                rect: Rect::new(
                    0.0,
                    0.0,
                    self.target_size.0 as f32,
                    self.target_size.1 as f32,
                ),
            };
            return Some((root, stack));
        };
        let layer = stack.get(at)?;
        let root = layer.pixmap.as_ref().map(|p| Root {
            data: p.data(),
            width: p.width() as usize,
            rect: Rect::new(
                layer.origin.0 as f32,
                layer.origin.1 as f32,
                p.width() as f32,
                p.height() as f32,
            ),
        })?;
        Some((root, stack.get(at + 1..)?))
    }

    /// Reads the backdrop in `area` (whole device px inside the root) into
    /// `into`: the root's pixels with the layers above it drawn over them.
    fn read_backdrop(&self, own_layers: u8, area: Rect, into: &mut Pixmap) {
        let Some((root, above)) = self.backdrop_root(own_layers) else {
            return;
        };
        root.copy(area, into);
        for layer in above {
            if let (false, Some(pixmap)) = (layer.direct, &layer.pixmap) {
                into.draw_pixmap(
                    layer.origin.0 - area.x as i32,
                    layer.origin.1 - area.y as i32,
                    pixmap.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            }
        }
    }

    /// Draws `pixmap` (whole device px at `at`) over the current surface
    /// with the opacity `alpha`, inside the box `device` (rounded by
    /// `radii`), the part in `output`, inside the clips.
    fn draw_backdrop(
        &mut self,
        pixmap: &Pixmap,
        at: Rect,
        (device, output): (Rect, Rect),
        (radii, alpha): (Option<&Radii>, f32),
    ) {
        let rounded = radii
            .map(|r| scale_radii(r, self.params.scale))
            .and_then(|r| rounded_rect_path(device, &r));
        let paint = Paint {
            shader: Pattern::new(
                pixmap.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Nearest,
                alpha.clamp(0.0, 1.0),
                Transform::from_translate(at.x, at.y),
            ),
            anti_alias: rounded.is_some(),
            ..Paint::default()
        };
        if let Some(path) = rounded {
            self.draw(output, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::Winding, t, m);
            });
        } else if let Some(r) = skia_rect(output) {
            self.draw(output, |p, t, m| p.fill_rect(r, &paint, t, m));
        }
    }
}
