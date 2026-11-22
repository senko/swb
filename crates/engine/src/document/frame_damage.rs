//! Frames that rasterize only what changed (ADR 0044 section 18): the
//! document keeps what its last frame drew ([`LastFrame`]: the display
//! list, its media keys, the viewport indicators, the raster parameters),
//! and the next frame into the same pixels compares its list with it
//! (`swb_paint::list_damage`) and rasterizes only the damage
//! (`swb_paint::rasterize_region`).
//!
//! The inputs of a frame that are not in the display list, and their rules:
//!
//! - the scroll offset of the viewport, the viewport size and the scale:
//!   in the raster parameters; a change gives a full raster;
//! - the scroll offsets of scroll containers, the caret, the selection, the
//!   focus ring: in the list (the builder moves the content and draws
//!   them), so the diff sees them;
//! - images: the media key of each `Image` item (the decoded image and its
//!   content generation), so a load, a decode or a new video frame is
//!   damage;
//! - frame documents: the media key of each `Frame` item (the paint stamps
//!   of the frame's documents, their viewports and canvases): a change
//!   damages the frame's rectangle;
//! - the viewport scroll indicators, drawn over the page after the raster:
//!   their items are compared as a second list, and they are drawn again
//!   in every rectangle of damage;
//! - work that a budget left out or drew coarser (`RasterReport`): the
//!   next frame is a full raster;
//! - the pixels of the target: the caller says that they are those of the
//!   last frame of this document (`kept`); else, and on the first frame, a
//!   full raster.

use std::sync::Arc;
use std::time::Instant;

use swb_layout::{Point, Rect};
use swb_paint::{
    Damage, DisplayList, ImageSource, MediaKeys, PixelRect, Pixmap, RasterParams, RasterReport,
    Side,
};
use swb_text::FontContext;

use super::DocumentState;
use crate::frames::paint::FrameSource;
use crate::frames::{FrameId, Frames};

/// What the last frame of a document drew.
pub(crate) struct LastFrame {
    list: Arc<DisplayList>,
    keys: MediaKeys,
    /// The viewport scroll indicators, drawn over the page.
    indicators: DisplayList,
    params: RasterParams,
    /// The target size in device px.
    size: (u32, u32),
    /// A budget left something out: the next frame draws everything.
    incomplete: bool,
}

/// How a frame of the page was drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameDamage {
    /// True if the whole viewport was rasterized.
    pub full: bool,
    /// The rectangles of damage that were rasterized (none for a full
    /// raster and for a frame without damage).
    pub rects: Vec<PixelRect>,
    /// The device pixels of the viewport.
    pub viewport_pixels: u64,
}

impl FrameDamage {
    /// The device pixels that were rasterized.
    pub fn pixels(&self) -> u64 {
        if self.full {
            self.viewport_pixels
        } else {
            self.rects.iter().map(PixelRect::area).sum()
        }
    }
}

impl DocumentState {
    /// Renders the viewport into `target` as the page's frame and keeps
    /// what it drew. With `kept`, `target` holds the pixels of the last
    /// frame of this document: only the damage is rasterized, if the last
    /// frame can be compared (see the module documentation).
    pub(crate) fn render_frame(
        &mut self,
        target: &mut Pixmap,
        kept: bool,
        fonts: &mut FontContext,
        frames: &Frames,
    ) -> FrameDamage {
        self.update_display_list(fonts);
        let params = RasterParams {
            scroll: self.scroll,
            viewport_scroll: self.scroll,
            scale: self.view.scale,
        };
        let size = (target.width(), target.height());
        let Some(list) = self.display_list.clone() else {
            self.last_frame = None;
            swb_paint::fill(target, swb_style::Rgba::WHITE);
            self.draw_indicators(target, None, fonts);
            return full_damage(size);
        };
        let started = Instant::now();
        let indicators = self.viewport_indicators();
        let source = FrameSource::new(&self.images, frames, FrameId::TOP);
        let images: &dyn ImageSource = if frames.count() == 0 {
            &self.images
        } else {
            &source
        };
        let keys = swb_paint::media_keys(&list, images);
        let damage = match self.last_frame.as_deref() {
            Some(last)
                if kept && last.params == params && last.size == size && !last.incomplete =>
            {
                frame_damage(last, (&list, &keys, &indicators), params, size)
            }
            last => {
                log::debug!(
                    "damage: full raster (pixels kept: {kept}, last frame: {}, comparable: {})",
                    last.is_some(),
                    last.is_some_and(|l| l.params == params && l.size == size && !l.incomplete)
                );
                Damage::Full
            }
        };
        self.timings.damage = started.elapsed();
        let started = Instant::now();
        let (report, drawn) = match &damage {
            Damage::None => (
                RasterReport::default(),
                FrameDamage {
                    viewport_pixels: full_damage(size).viewport_pixels,
                    ..FrameDamage::default()
                },
            ),
            Damage::Full => {
                swb_paint::fill(target, swb_style::Rgba::WHITE);
                let report = swb_paint::rasterize(&list, target, params, fonts, images);
                (report, full_damage(size))
            }
            Damage::Rects(rects) => {
                let report = swb_paint::rasterize_region(
                    &list,
                    target,
                    params,
                    rects,
                    Some(swb_style::Rgba::WHITE),
                    fonts,
                    images,
                );
                let drawn = FrameDamage {
                    full: false,
                    rects: rects.clone(),
                    viewport_pixels: full_damage(size).viewport_pixels,
                };
                (report, drawn)
            }
        };
        self.timings.raster = started.elapsed();
        match &damage {
            Damage::None => {}
            Damage::Full => self.draw_indicators(target, None, fonts),
            Damage::Rects(rects) => self.draw_indicators(target, Some(rects), fonts),
        }
        self.last_frame = Some(Box::new(LastFrame {
            list,
            keys,
            indicators,
            params,
            size,
            incomplete: report.incomplete,
        }));
        drawn
    }

    /// The overlay scroll indicators of the viewport (the same indicators
    /// as those of scroll containers), in viewport coordinates; none
    /// without layout or when the GUI does not show them.
    pub(crate) fn viewport_indicators(&self) -> DisplayList {
        let none = DisplayList::default();
        if !self.view.indicators {
            return none;
        }
        let Some(fragments) = &self.fragments else {
            return none;
        };
        if fragments.viewport_scrollbar_hidden {
            return none;
        }
        let viewport = self.view.viewport;
        let port = Rect::new(0.0, 0.0, viewport.width, viewport.height);
        // No indicator on axes with `overflow: hidden`.
        let (user_x, user_y) = self.viewport_user_axes();
        let size = fragments.scroll_size;
        DisplayList {
            items: swb_paint::scroll_indicators(port, size, self.scroll, user_x, user_y),
        }
    }

    /// Draws the viewport scroll indicators over the page: everywhere, or
    /// in `rects` only.
    pub(super) fn draw_indicators(
        &self,
        target: &mut Pixmap,
        rects: Option<&[PixelRect]>,
        fonts: &mut FontContext,
    ) {
        let list = self.viewport_indicators();
        if list.items.is_empty() {
            return;
        }
        let params = indicator_params(self.view.scale);
        match rects {
            None => {
                swb_paint::rasterize(&list, target, params, fonts, &self.images);
            }
            Some(rects) => {
                swb_paint::rasterize_region(
                    &list,
                    target,
                    params,
                    rects,
                    None,
                    fonts,
                    &self.images,
                );
            }
        }
    }

    /// Rasterizes the parts `rects` of the viewport into `target` (see
    /// `Page::render_regions`), with the viewport indicators; the parts
    /// grow to hold the boxes of the backdrop filters that they touch.
    pub(crate) fn render_regions(
        &mut self,
        target: &mut Pixmap,
        rects: &[PixelRect],
        fonts: &mut FontContext,
        frames: &Frames,
    ) {
        self.update_display_list(fonts);
        let Some(list) = self.display_list.clone() else {
            return;
        };
        let params = RasterParams {
            scroll: self.scroll,
            viewport_scroll: self.scroll,
            scale: self.view.scale,
        };
        let source = FrameSource::new(&self.images, frames, FrameId::TOP);
        let images: &dyn ImageSource = if frames.count() == 0 {
            &self.images
        } else {
            &source
        };
        // The parts hold the boxes of the backdrop filters that they touch,
        // as the damage of a frame does.
        let keys = swb_paint::media_keys(&list, images);
        let boxes = swb_paint::backdrop_boxes(&list, &keys, params);
        let size = (target.width(), target.height());
        let rects = swb_paint::hold_backdrop_boxes(rects, &boxes, size);
        let white = Some(swb_style::Rgba::WHITE);
        swb_paint::rasterize_region(&list, target, params, &rects, white, fonts, images);
        self.draw_indicators(target, Some(&rects), fonts);
    }
}

/// The raster parameters of the viewport indicators (viewport
/// coordinates).
fn indicator_params(scale: f32) -> RasterParams {
    RasterParams {
        scroll: Point::default(),
        viewport_scroll: Point::default(),
        scale,
    }
}

/// The damage of a full raster of a target of `size`.
pub(crate) fn full_damage((width, height): (u32, u32)) -> FrameDamage {
    FrameDamage {
        full: true,
        rects: Vec::new(),
        viewport_pixels: u64::from(width) * u64::from(height),
    }
}

/// The damage between the last frame and the new list, its media keys and
/// the new viewport indicators.
fn frame_damage(
    last: &LastFrame,
    (list, keys, indicators): (&DisplayList, &MediaKeys, &DisplayList),
    params: RasterParams,
    size: (u32, u32),
) -> Damage {
    let mut found = swb_paint::list_damage(
        Side {
            list: &last.list,
            keys: &last.keys,
        },
        Side { list, keys },
        params,
    );
    let no_keys = MediaKeys::default();
    let bars = swb_paint::list_damage(
        Side {
            list: &last.indicators,
            keys: &no_keys,
        },
        Side {
            list: indicators,
            keys: &no_keys,
        },
        indicator_params(params.scale),
    );
    if found.full || bars.full {
        log::debug!("damage: full raster (the groups of a list do not nest)");
        return Damage::Full;
    }
    found.rects.extend(bars.rects);
    let boxes = swb_paint::backdrop_boxes(list, keys, params);
    let merged = swb_paint::merge_damage(&found.rects, &boxes, size);
    if merged == Damage::Full {
        log::debug!(
            "damage: full raster ({} rectangles cover more than half of the viewport)",
            found.rects.len()
        );
    }
    merged
}
