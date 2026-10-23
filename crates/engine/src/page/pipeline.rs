//! Rendering of the page: the document's pipeline (style, layout, display
//! list, raster; in `crate::document`) driven for frames and screenshots;
//! the viewport, its accepted limits, and screenshots.

use swb_layout::Size;
use swb_paint::Pixmap;

use super::{Page, StageTimings};
use crate::document::Rendering;

impl Page {
    /// Brings styles and layout up to date. Rendering waits for
    /// stylesheets: until they are loaded, there is no layout. After the
    /// first layout of a document, scrolls to the fragment or to the
    /// restored position.
    pub fn update_layout(&mut self) {
        if let Some((doc, fonts)) = self.parts() {
            doc.update_layout(fonts);
        }
    }

    /// Renders the viewport into `target` (device pixels). The target size
    /// should be the viewport size times the scale factor.
    pub fn render(&mut self, target: &mut Pixmap) {
        let rows = target.height();
        self.render_in_strips(target, rows, Rendering::Viewport);
    }

    /// Renders `what` into `target` in strips of `strip_rows` device rows
    /// (see [`crate::document::DocumentState`]); white without a document.
    fn render_in_strips(&mut self, target: &mut Pixmap, strip_rows: u32, what: Rendering) {
        match self.parts() {
            Some((doc, fonts)) => doc.render_in_strips(target, strip_rows, what, fonts),
            None => swb_paint::fill(target, swb_style::Rgba::WHITE),
        }
    }

    /// The duration of each pipeline stage, the last time it ran.
    pub fn timings(&self) -> StageTimings {
        self.doc
            .as_ref()
            .map_or_else(StageTimings::default, |d| d.timings)
    }

    /// Discards the parsed stylesheets, the styles, the layout and the
    /// display list, so that the next render runs all stages again. For
    /// benchmarks.
    pub fn restart_pipeline(&mut self) {
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.restart_pipeline();
        }
    }

    /// Renders a screenshot: the viewport, or with `full_page` the whole
    /// content height (limited to [`MAX_SCREENSHOT_PIXELS`] device pixels)
    /// from the top of the page, with the layout of the viewport and fixed
    /// and sticky boxes where they are at the current scroll offset (as
    /// Chromium's full-page screenshots). A full page is rasterized in equal strips of
    /// at most the viewport's height or [`MIN_STRIP_PIXELS`], whichever is
    /// more: the rasterizer's budgets (group layers, masks, transform
    /// layers) apply to each strip as to a window.
    pub fn screenshot(&mut self, full_page: bool) -> Result<Pixmap, ScreenshotError> {
        let (viewport, scale) = (self.view.viewport, self.view.scale);
        let size = if full_page {
            let content = self.content_size();
            // One row less than the limit, so that rounding cannot exceed
            // it.
            let device_width = f64::from((viewport.width * scale).round().max(1.0));
            let max_height =
                (((MAX_SCREENSHOT_PIXELS / device_width).floor() - 1.0) / f64::from(scale)) as f32;
            if content.height > max_height {
                log::warn!(
                    "the page is {} px high; the screenshot shows the first {max_height:.0} px",
                    content.height
                );
            }
            Size::new(viewport.width, content.height.min(max_height).max(1.0))
        } else {
            viewport
        };
        let (w, h) = device_size(size, scale)?;
        // tiny-skia aborts the process when an allocation fails, so the
        // size is checked first (`device_size`).
        let mut pixmap = Pixmap::new(w, h).ok_or(ScreenshotError::TooLarge(w, h))?;
        if size == viewport {
            self.render(&mut pixmap);
        } else {
            // As in Chromium: the layout keeps the viewport, and the page is
            // drawn from its top with fixed and sticky boxes where they are
            // at the current scroll offset; the viewport does not clip them.
            let min_rows = (MIN_STRIP_PIXELS / f64::from(w)).ceil() as f32;
            let strip_rows = (viewport.height * scale).round().max(min_rows).max(1.0) as u32;
            self.render_in_strips(&mut pixmap, strip_rows, Rendering::Page);
        }
        Ok(pixmap)
    }

    /// Changes the viewport size (CSS px) and scale factor.
    pub fn set_viewport(&mut self, viewport: Size, scale: f32) {
        if viewport == self.view.viewport && (scale - self.view.scale).abs() < f32::EPSILON {
            return;
        }
        self.view.viewport = viewport;
        self.view.scale = scale;
        // Image sources (selected at the next style computation) and media
        // queries may change, so styles are recomputed.
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.set_view(self.view);
        }
    }

    /// Shows or hides the overlay scroll indicators of the scroll
    /// containers and the viewport (the GUI shows them; screenshots for
    /// comparisons with Chromium do not).
    pub fn set_scroll_indicators(&mut self, show: bool) {
        self.view.indicators = show;
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.set_view(self.view);
        }
    }
}

/// The largest viewport width or height in CSS px that the front ends
/// accept from users (`--size`, `page.setViewport`).
pub const MAX_VIEWPORT_SIDE: f32 = 16_384.0;

/// The largest scale factor that the front ends accept from users.
pub const MAX_SCALE: f32 = 8.0;

/// A viewport size or scale factor outside the accepted range.
#[derive(Debug, thiserror::Error)]
pub enum ViewportError {
    /// The width or height is not between 1 and [`MAX_VIEWPORT_SIDE`].
    #[error("width and height must be between 1 and {}", MAX_VIEWPORT_SIDE)]
    Size,
    /// The scale factor is 0 or less, or above [`MAX_SCALE`].
    #[error("the scale must be above 0 and at most {}", MAX_SCALE)]
    Scale,
}

/// Checks a viewport size (CSS px) from a user.
pub fn check_viewport_size(size: Size) -> Result<(), ViewportError> {
    let valid = |v: f32| (1.0..=MAX_VIEWPORT_SIDE).contains(&v);
    if valid(size.width) && valid(size.height) {
        Ok(())
    } else {
        Err(ViewportError::Size)
    }
}

/// Checks a scale factor from a user.
pub fn check_scale(scale: f32) -> Result<(), ViewportError> {
    if scale > 0.0 && scale <= MAX_SCALE {
        Ok(())
    } else {
        Err(ViewportError::Scale)
    }
}

/// The largest screenshot in device pixels: 128 Mpx, 512 MiB of RGBA.
pub const MAX_SCREENSHOT_PIXELS: f64 = 128.0 * 1024.0 * 1024.0;

/// The pixels of the strips that a full-page screenshot is split into, if
/// the viewport has fewer (16 Mpx). The rasterizer makes the strips equal,
/// so they can be smaller, but there are no more of them than if they had
/// this size. The rasterizer's budgets have a floor per strip, so the
/// number of strips bounds the total work: the largest screenshot has at
/// most 8 strips.
const MIN_STRIP_PIXELS: f64 = 16.0 * 1024.0 * 1024.0;

/// Why a screenshot failed.
#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    /// The screenshot would be larger than [`MAX_SCREENSHOT_PIXELS`], or
    /// the memory could not be allocated.
    #[error("a {0}x{1} screenshot is too large")]
    TooLarge(u32, u32),
}

/// The size in device pixels of `size` CSS px at a scale factor, if it is
/// within [`MAX_SCREENSHOT_PIXELS`].
pub fn device_size(size: Size, scale: f32) -> Result<(u32, u32), ScreenshotError> {
    let w = (size.width * scale).round().max(1.0);
    let h = (size.height * scale).round().max(1.0);
    if f64::from(w) * f64::from(h) > MAX_SCREENSHOT_PIXELS {
        return Err(ScreenshotError::TooLarge(w as u32, h as u32));
    }
    Ok((w as u32, h as u32))
}
