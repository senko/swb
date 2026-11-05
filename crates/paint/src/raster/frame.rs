//! Frames (ADR 0031 section 4): the rasterizer draws the display list of a
//! frame's document when it meets the frame's item, in place.
//!
//! - The list is clipped to the content box of the `iframe` and moved to its
//!   origin minus the frame's scroll offset. The origin is the corner of the
//!   content box snapped to device pixels, as the clip is (the frame's
//!   document then has the same pixels wherever the box is).
//! - The groups that are open at the item stay open: the opacity, transform,
//!   clip, filter and mask of the `iframe` and of its ancestors apply to the
//!   frame. The budgets of the strip cover what the frame draws.
//! - Content fixed to the viewport is fixed to the frame's viewport: it
//!   starts at the corner of the content box and the clip of the frame stays
//!   around it. Fixed and sticky boxes use the frame's scroll offset.
//! - A `backdrop-filter` in the frame reads the surface below (measured:
//!   a transparent frame shows the inverted content of its parent), so the
//!   frame needs no layer of its own.
//! - A frame whose item count does not fit into [`MAX_FRAME_ITEMS`] draws
//!   nothing, with one warning per rasterization.

use swb_dom::NodeId;
use swb_layout::{Point, Rect};
use swb_style::Rgba;
use swb_text::FontContext;

use super::{ImageSource, Rasterizer, snap};
use crate::display_list::{DisplayList, outset};

/// What the rasterizer needs of a frame's document.
pub struct FrameContent<'a> {
    /// The display list of the document.
    pub list: &'a DisplayList,
    /// [`crate::transform_ends`] of `list`.
    pub ends: &'a [usize],
    /// The scroll offset of the document's viewport, in CSS px.
    pub scroll: Point,
    /// The color of the frame's canvas below the list, if it is opaque
    /// (CSS Color Adjustment 1 §2.3); `None` for a transparent canvas.
    pub canvas: Option<Rgba>,
    /// The images and the frames of the document.
    pub source: Box<dyn ImageSource + 'a>,
}

/// The most items of the display lists of frames that one strip draws (a
/// frame is counted once per item of its list, also if it is inside a
/// group; frames that the visible area misses do not count). A frame that
/// does not fit draws nothing. Measured: 1,000 frames over each other with
/// 2.5 million items of boxes with text take 0.52 s (the hostile case
/// `frames-1000-big-lists` with 500 boxes per frame), so a strip takes about
/// 1 s at the budget.
pub(super) const MAX_FRAME_ITEMS: u64 = 4 * 1024 * 1024;

/// The deepest nesting of frames that the rasterizer follows. The page does
/// not make deeper frames (`MAX_DEPTH` of the engine, ADR 0031 section 11);
/// the rasterizer checks because it recurses.
const MAX_FRAME_DEPTH: u32 = 33;

/// The deepest nesting of transform groups inside frames, over all frames
/// of a chain. One document nests at most as deep as its boxes
/// (`MAX_BOX_DEPTH`, 256), and the recursion of the rasterizer goes through
/// the groups of all documents of the chain, so 32 frames could reach 8,192
/// levels.
const MAX_FRAME_GROUP_DEPTH: u32 = 1024;

impl Rasterizer<'_> {
    /// Notes the start of a transform group. False if the group nests too
    /// deep in frames: it is not drawn.
    pub(super) fn enter_group(&mut self) -> bool {
        if self.frames.frames == 0 {
            return true;
        }
        if self.frames.groups >= MAX_FRAME_GROUP_DEPTH {
            self.budget.skipped.frames = true;
            return false;
        }
        self.frames.groups += 1;
        true
    }

    /// Notes the end of a transform group that [`Self::enter_group`]
    /// started.
    pub(super) fn leave_group(&mut self) {
        if self.frames.frames > 0 {
            self.frames.groups -= 1;
        }
    }

    /// Draws the frame of the `iframe` element `node` whose content box is
    /// `rect` (list coordinates).
    pub(super) fn frame(
        &mut self,
        node: NodeId,
        rect: Rect,
        fonts: &mut FontContext,
        images: &dyn ImageSource,
    ) {
        if self.frames.frames >= MAX_FRAME_DEPTH {
            return;
        }
        let Some(content) = images.frame(node) else {
            return;
        };
        let items = content.list.items.len() as u64;
        let total = self.budget.frame_items.saturating_add(items);
        if total > MAX_FRAME_ITEMS {
            self.budget.skipped.frames = true;
            return;
        }
        self.budget.frame_items = total;
        if let Some(color) = content.canvas {
            self.fill_rect(rect, &[(0.0, 0.0); 4], color);
        }
        let s = self.params.scale;
        let margin = 1.0 / s;
        let Some(visible) = outset(rect.translate(self.translation), margin)
            .intersection(&self.visible)
            .filter(|_| !content.list.items.is_empty())
        else {
            return;
        };
        let (open_clips, open_layers) = (self.clips.len(), self.layers.len());
        // The corner of the content box, on the device pixel of the clip,
        // in the coordinates that `params` maps to device px.
        let corner = snap(self.to_device(rect));
        let origin = Point::new(
            corner.x / s + self.params.scroll.x,
            (corner.y + self.offset_y) / s + self.params.scroll.y,
        );
        self.push_rect_clip(rect);
        let outer = Outer {
            translation: self.translation,
            visible: self.visible,
            viewport_scroll: self.viewport_scroll,
            fixed_origin: self.fixed_origin,
            viewport_clip: self.viewport_clip,
            sticky: std::mem::take(&mut self.sticky),
        };
        self.translation = Point::new(origin.x - content.scroll.x, origin.y - content.scroll.y);
        self.visible = visible;
        self.viewport_scroll = content.scroll;
        self.fixed_origin = origin;
        self.viewport_clip = self.clips.last().copied();
        self.frames.frames += 1;
        let source = &*content.source;
        self.run(
            &content.list.items,
            0..content.list.items.len(),
            content.ends,
            fonts,
            source,
        );
        // Close unbalanced groups.
        while self.layers.len() > open_layers {
            self.pop_layer(source);
        }
        self.frames.frames -= 1;
        self.clips.truncate(open_clips);
        self.mask = None;
        self.translation = outer.translation;
        self.visible = outer.visible;
        self.viewport_scroll = outer.viewport_scroll;
        self.fixed_origin = outer.fixed_origin;
        self.viewport_clip = outer.viewport_clip;
        self.sticky = outer.sticky;
    }
}

/// The state of the rasterizer around a frame.
struct Outer {
    translation: Point,
    visible: Rect,
    viewport_scroll: Point,
    fixed_origin: Point,
    viewport_clip: Option<Rect>,
    sticky: swb_layout::StickyCache,
}
