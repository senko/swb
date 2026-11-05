//! Filter groups (`filter`, Filter Effects 1 §5): the group is drawn into a
//! layer, the filter steps (`crate::filter`) change the layer's pixels when
//! the group ends, and the result is composited into the surface below.
//!
//! The content of the group is drawn without the enclosing clips, because
//! the filter applies to the whole element, also to what the clips hide:
//! the blur of a box at the edge of a clip reads the pixels behind the edge.
//! The clips apply to the result. The layer covers the part of the
//! output that can be visible, and around it what the steps read
//! (`crate::filter::backward_reach`), inside the area that the content and
//! the filter can reach. The groups inside the layer draw in a
//! larger visible area than the enclosing one. The steps count against the
//! layer budget (the layer, and the scratch buffer of a blur) and the
//! filter work budget (`MAX_FILTER_PIXELS`); a group that does not fit
//! draws its content without the filter.

use swb_layout::Rect;
use swb_style::{FilterFunction, Rgba};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::{Layer, LayerKind, Rasterizer, snap_out};
use crate::filter::{self, Margins, Op};

/// What a filter group needs when it ends.
pub(super) struct FilterLayer {
    /// The steps, in device px.
    ops: Vec<Op>,
    /// The clips around the group, which apply to its result.
    clips: Vec<Rect>,
    /// The area of the rasterizer that culls the items of the group, before
    /// the group.
    visible: Rect,
    /// True if the group contains content fixed to the viewport, which the
    /// enclosing clips do not clip.
    escapes_clips: bool,
    /// True if the budget holds the pixels of a scratch buffer.
    scratch: bool,
}

impl Rasterizer<'_> {
    /// Starts a filter group of the functions `list`. `bounds` is the
    /// area of its content (list coordinates), `fixed` the area of its
    /// content fixed to the viewport.
    pub(super) fn push_filter(
        &mut self,
        list: &[FilterFunction],
        current_color: Rgba,
        bounds: Rect,
        fixed: Option<Rect>,
        escapes_clips: bool,
    ) {
        self.mask = None;
        let ops = filter::device_ops(list, current_color, self.params.scale);
        if ops.is_empty() {
            self.layers.push(passthrough());
            return;
        }
        let visible = if escapes_clips {
            self.fixed_area()
        } else {
            self.visible_area()
        };
        // The content of the group, and its content fixed to the viewport
        // where it is at the viewport scroll offset.
        let non_empty = |r: &Rect| r.width > 0.0 && r.height > 0.0;
        let mut content = Some(bounds).filter(non_empty).map(|b| self.to_device(b));
        if let Some(fixed) = fixed.filter(non_empty) {
            let saved = std::mem::take(&mut self.translation);
            let at = self.to_device(fixed.translate(self.fixed_origin));
            self.translation = saved;
            content = Some(content.map_or(at, |c| c.union(&at)));
        }
        let content = content.map(snap_out);
        let output = content.map(|c| grow(c, filter::forward_growth(&ops)));
        let shown = visible
            .zip(output)
            .and_then(|(v, o)| o.intersection(&v))
            .filter(|r| r.width >= 1.0 && r.height >= 1.0);
        let (Some(shown), Some(output)) = (shown, output) else {
            self.layers.push(hidden());
            return;
        };
        // The layer holds what the steps read, and the results of the steps
        // on the way (a shadow is blurred before it is moved).
        let working = content.map_or(output, |c| grow(c, filter::peak_extent(&ops)));
        let area = grow(shown, filter::backward_reach(&ops))
            .intersection(&working)
            .unwrap_or(shown);
        match self.filter_layer(&ops, area) {
            Some((pixmap, scratch)) => {
                let layer = FilterLayer {
                    ops,
                    clips: std::mem::take(&mut self.clips),
                    visible: self.visible,
                    escapes_clips,
                    scratch,
                };
                self.visible = self.visible_for(area);
                self.layers.push(Layer {
                    pixmap: Some(pixmap),
                    direct: false,
                    origin: (area.x as i32, area.y as i32),
                    opacity: 1.0,
                    kind: LayerKind::Filter(Box::new(layer)),
                    rect_clip: false,
                });
            }
            None => self.layers.push(passthrough()),
        }
    }

    /// The layer of a group in `area` (target device px) and whether the
    /// budget holds a scratch buffer for it, if the steps fit into the
    /// budgets.
    pub(super) fn filter_layer(&mut self, ops: &[Op], area: Rect) -> Option<(Pixmap, bool)> {
        let pixels = (area.width as u64).saturating_mul(area.height as u64);
        let work = filter::work(ops, pixels);
        let needs_scratch = ops.iter().any(|op| !matches!(op, Op::Colors(_)));
        let reserve = if needs_scratch { pixels } else { 0 };
        if self.budget.filter_pixels.saturating_add(work) > self.budget.max_filter_pixels
            || self
                .budget
                .layer_pixels
                .saturating_add(pixels)
                .saturating_add(reserve)
                > self.budget.max_layer_pixels
        {
            if self.budget.filter_pixels.saturating_add(work) > self.budget.max_filter_pixels {
                self.budget.skipped.filters = true;
            } else {
                self.budget.skipped.layers = true;
            }
            return None;
        }
        let pixmap = self.layer_pixmap(area.width as u32, area.height as u32)?;
        self.budget.layer_pixels += reserve;
        self.budget.filter_pixels += work;
        Some((pixmap, needs_scratch))
    }

    /// `area` (target device px) as the area that culls items: in the
    /// coordinates of the list, as `visible` is, one px larger on every
    /// side.
    fn visible_for(&self, area: Rect) -> Rect {
        let s = self.params.scale;
        let scroll = self.params.scroll;
        Rect::new(
            area.x / s + scroll.x - 1.0 / s,
            (area.y + self.offset_y) / s + scroll.y - 1.0 / s,
            area.width / s + 2.0 / s,
            area.height / s + 2.0 / s,
        )
    }

    /// Ends the filter group `layer`: applies the steps and draws the
    /// result into the surface below, inside the clips around the group.
    pub(super) fn pop_filter(&mut self, mut layer: Layer, filter: FilterLayer) {
        let FilterLayer {
            ops,
            clips,
            visible,
            escapes_clips,
            scratch,
        } = filter;
        self.visible = visible;
        let Some(mut pixmap) = layer.pixmap.take() else {
            self.clips = clips;
            return;
        };
        let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
        let mut buffer = vec![0_u8; if scratch { width * height * 4 } else { 0 }];
        filter::apply(pixmap.data_mut(), &mut buffer, width, height, &ops);
        drop(buffer);
        let pixels = (width as u64) * (height as u64);
        self.release(&pixmap);
        if scratch {
            self.budget.layer_pixels = self.budget.layer_pixels.saturating_sub(pixels);
        }
        if escapes_clips {
            // Only the viewport of an enclosing frame clips the result.
            self.clips.clear();
            self.clips.extend(self.viewport_clip);
            self.mask = None;
            self.composite_filtered(&pixmap, layer.origin);
            self.clips = clips;
        } else {
            self.clips = clips;
            self.composite_filtered(&pixmap, layer.origin);
        }
        self.mask = None;
    }

    /// Draws the layer `pixmap` at `origin` (target device px) into the
    /// surface, inside the open clip.
    fn composite_filtered(&mut self, pixmap: &Pixmap, origin: (i32, i32)) {
        let Some(parent) = self.surface_rect() else {
            return;
        };
        let device = Rect::new(
            origin.0 as f32,
            origin.1 as f32,
            pixmap.width() as f32,
            pixmap.height() as f32,
        );
        let x = origin.0 - parent.x as i32;
        let y = origin.1 - parent.y as i32;
        self.draw(device, |surface, _, mask| {
            surface.draw_pixmap(
                x,
                y,
                pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                mask,
            );
        });
    }
}

/// A layer that draws its content directly (a group without painted
/// steps, or one that did not fit into the budgets).
fn passthrough() -> Layer {
    Layer {
        pixmap: None,
        direct: true,
        origin: (0, 0),
        opacity: 1.0,
        kind: LayerKind::Opacity,
        rect_clip: false,
    }
}

/// A layer that draws nothing (the group is not visible).
fn hidden() -> Layer {
    Layer {
        pixmap: None,
        direct: false,
        origin: (0, 0),
        opacity: 1.0,
        kind: LayerKind::Opacity,
        rect_clip: false,
    }
}

/// `r` grown by `m` px on each side.
pub(super) fn grow(r: Rect, m: Margins) -> Rect {
    Rect::new(
        r.x - m.left as f32,
        r.y - m.top as f32,
        r.width + (m.left + m.right) as f32,
        r.height + (m.top + m.bottom) as f32,
    )
}
