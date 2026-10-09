//! The rendering pipeline: style, layout, display list and raster, with
//! per-stage timings; restyles after state changes; the viewport, its
//! accepted limits, and screenshots.

use std::time::Instant;

use swb_css::{MediaEnvironment, MediaQueryList};
use swb_layout::{BoxContent, FragmentRef, LayoutInput, Point, Rect, Size};
use swb_net::Url;
use swb_paint::{DisplayList, NoHighlights, Pixmap, RasterParams, Scrolling};
use swb_style::Stylist;

use super::{LoadState, Page, ScrollTarget, StageTimings, about_blank};

use crate::forms::LayoutControls;
use crate::selection::Highlight;

/// The most layout passes of one [`Page::update_layout`]: web fonts that
/// are already loaded make layout run again.
const MAX_LAYOUT_PASSES: usize = 4;

/// What [`Page::render_in_strips`] draws.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rendering {
    /// The viewport at the page's scroll offset, with the scroll
    /// indicators of the viewport.
    Viewport,
    /// The page from its top-left corner, without the viewport's scroll
    /// indicators (full-page screenshots).
    Page,
}

impl Page {
    /// Drops the parsed stylesheets, the styles and everything after them.
    pub(super) fn invalidate_style(&mut self) {
        self.stylist = None;
        self.web_fonts.env = None;
        self.styles = None;
        self.invalidate_layout();
    }

    /// Drops the layout and the display list.
    pub(super) fn invalidate_layout(&mut self) {
        self.fragments = None;
        self.display_list = None;
    }

    /// Recomputes the styles after a change of the element states. The
    /// layout stays if no style changed.
    pub(super) fn restyle(&mut self) {
        let Some(old) = self.styles.take() else {
            return;
        };
        self.compute_styles();
        if !self
            .styles
            .as_ref()
            .is_some_and(|new| new.same_styles(&old))
        {
            self.invalidate_layout();
        }
    }

    pub(super) fn media_environment(&self) -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: self.viewport.width,
            viewport_height: self.viewport.height,
            device_pixel_ratio: self.scale,
            ..MediaEnvironment::default()
        }
    }

    /// Brings styles and layout up to date. Rendering waits for
    /// stylesheets: until they are loaded, there is no layout. After the
    /// first layout of a document, scrolls to the fragment or to the
    /// restored position.
    ///
    /// Layout can request web fonts. A font whose file is already loaded
    /// (another face of the same file) is used at once and invalidates the
    /// layout; layout then runs again, up to [`MAX_LAYOUT_PASSES`] times.
    pub fn update_layout(&mut self) {
        if self.document.is_none() || !self.stylesheets_settled() {
            return;
        }
        if self.styles.is_none() {
            self.compute_styles();
        }
        let mut laid_out = false;
        // After the last pass, `fragments` can be `None` again: a font that
        // loaded at once invalidated the layout. This is correct: every
        // caller (paint, scroll, hit testing) calls `update_layout` before
        // it uses the fragments, so the next call lays out again. The pass
        // limit only bounds the work of one call.
        for _ in 0..MAX_LAYOUT_PASSES {
            if self.fragments.is_some() || !self.run_layout() {
                break;
            }
            laid_out = true;
            self.start_font_loads();
        }
        if laid_out && !self.requests.is_empty() && self.state == LoadState::Complete {
            self.state = LoadState::LoadingResources;
        }
        if laid_out && self.fragments.is_some() {
            // The scroll target is applied after every layout until the
            // page is loaded, because images that arrive later move it.
            // Scrolling by the user cancels it.
            match self.pending_scroll.clone() {
                Some(ScrollTarget::Fragment(fragment)) => {
                    self.reveal_fragment(&fragment);
                }
                Some(ScrollTarget::Position(position)) => self.scroll = position,
                None => {}
            }
            self.clamp_scroll();
            if !self.is_loading() {
                self.pending_scroll = None;
            }
        }
    }

    /// Lays out the document, if it has styles. Returns false if not.
    fn run_layout(&mut self) -> bool {
        let (Some(doc), Some(styles)) = (&self.document, &self.styles) else {
            return false;
        };
        let controls = LayoutControls {
            doc,
            forms: &self.forms,
            focus: self.input.states.focus,
        };
        let input = LayoutInput {
            document: doc,
            styles,
            viewport: self.viewport,
            replaced: &self.images,
            controls: &controls,
        };
        let started = Instant::now();
        let fragments = swb_layout::layout(&input, &mut self.fonts);
        self.timings.layout = started.elapsed();
        log::debug!("layout: {:?}", self.timings.layout);
        self.scrollers.update(&fragments);
        self.fragments = Some(fragments);
        self.keep_control_scroll();
        self.select_auto_sized_images();
        true
    }

    /// Stores the scroll offsets of the text in form controls that layout
    /// used, so that the next layout starts from them.
    fn keep_control_scroll(&mut self) {
        let Some(tree) = &self.fragments else {
            return;
        };
        let forms = &mut self.forms;
        tree.walk(|fragment, _| {
            if let FragmentRef::Box(b) = fragment
                && let (Some(node), BoxContent::Control(c)) = (b.node, &b.content)
                && let Some(state) = forms.get_mut(node)
            {
                state.scroll = c.scroll;
            }
        });
    }

    /// Brings the display list up to date.
    pub(super) fn update_display_list(&mut self) {
        self.update_layout();
        if self.display_list.is_none()
            && let (Some(fragments), Some(doc)) = (&self.fragments, &self.document)
        {
            let started = Instant::now();
            let page = self.selection().map(|s| s.ordered(&self.tree_order));
            let control = self.control_selection();
            let scrolling = Scrolling {
                offsets: self.scrollers.offsets(),
                indicators: self.scroll_indicators,
                scale: self.scale,
            };
            let list = if page.is_some() || control.is_some() {
                let highlight = Highlight {
                    page,
                    control,
                    order: &self.tree_order,
                    doc,
                };
                swb_paint::build_display_list(fragments, &self.images, &highlight, &scrolling)
            } else {
                swb_paint::build_display_list(fragments, &self.images, &NoHighlights, &scrolling)
            };
            self.timings.display_list = started.elapsed();
            log::debug!("display list: {:?}", self.timings.display_list);
            self.display_list = Some(list);
        }
    }

    fn compute_styles(&mut self) {
        if self.images.sources_outdated {
            self.select_image_sources();
        }
        let Some(doc) = &self.document else {
            return;
        };
        let env = self.media_environment();
        if self.stylist.is_none() {
            let started = Instant::now();
            let mut stylist = Stylist::new(doc.quirks_mode);
            for sheet in &self.sheets {
                let Some(css) = &sheet.css else {
                    continue;
                };
                if let Some(media) = &sheet.media
                    && !MediaQueryList::parse_str(media).matches(&env)
                {
                    continue;
                }
                stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &sheet.base_url);
            }
            self.stylist = Some(stylist);
            self.timings.stylesheets = started.elapsed();
        }
        let Some(stylist) = &self.stylist else {
            return;
        };
        let started = Instant::now();
        let base = self.base_url.clone().unwrap_or_else(about_blank);
        let styles = swb_style::compute_styles(doc, stylist, &env, &self.input.states, &base);
        self.timings.style = started.elapsed();
        log::debug!("style: {:?}", self.timings.style);
        let image_urls = styles.image_urls();
        self.styles = Some(styles);
        self.update_web_font_faces();
        for url in image_urls {
            if let Ok(url) = Url::parse(&url) {
                self.start_image(url);
            }
        }
        if !self.requests.is_empty() && self.state == LoadState::Complete {
            self.state = LoadState::LoadingResources;
        }
    }

    /// Renders the viewport into `target` (device pixels). The target size
    /// should be the viewport size times the scale factor.
    pub fn render(&mut self, target: &mut Pixmap) {
        let rows = target.height();
        self.render_in_strips(target, rows, Rendering::Viewport);
    }

    /// Renders `what` into `target` in strips of `strip_rows` device rows,
    /// each with the rasterizer's budgets of a viewport
    /// ([`swb_paint::rasterize_in_strips`]). Fixed and sticky boxes are
    /// where they are at the page's scroll offset.
    fn render_in_strips(&mut self, target: &mut Pixmap, strip_rows: u32, what: Rendering) {
        swb_paint::fill(target, swb_style::Rgba::WHITE);
        self.update_display_list();
        // The document point at the top-left corner of the target.
        let origin = match what {
            Rendering::Viewport => self.scroll,
            Rendering::Page => Point::default(),
        };
        if let Some(list) = &self.display_list {
            let params = RasterParams {
                scroll: origin,
                viewport_scroll: self.scroll,
                scale: self.scale,
            };
            let started = Instant::now();
            swb_paint::rasterize_in_strips(
                list,
                target,
                params,
                strip_rows,
                &mut self.fonts,
                &self.images,
            );
            self.timings.raster = started.elapsed();
        }
        // The viewport indicators belong on a rendering of the viewport.
        if self.scroll_indicators && what == Rendering::Viewport {
            self.render_viewport_indicators(target);
        }
    }

    /// Draws the overlay scroll indicators of the viewport over the page
    /// (the same indicators as those of scroll containers).
    fn render_viewport_indicators(&mut self, target: &mut Pixmap) {
        let Some(fragments) = &self.fragments else {
            return;
        };
        if fragments.viewport_scrollbar_hidden {
            return;
        }
        let port = Rect::new(0.0, 0.0, self.viewport.width, self.viewport.height);
        // No indicator on axes with `overflow: hidden`.
        let (user_x, user_y) = self.viewport_user_axes();
        let size = fragments.scroll_size;
        let items = swb_paint::scroll_indicators(port, size, self.scroll, user_x, user_y);
        if items.is_empty() {
            return;
        }
        let params = RasterParams {
            scroll: Point::default(),
            viewport_scroll: Point::default(),
            scale: self.scale,
        };
        let list = DisplayList { items };
        swb_paint::rasterize(&list, target, params, &mut self.fonts, &self.images);
    }

    /// The duration of each pipeline stage, the last time it ran.
    pub fn timings(&self) -> StageTimings {
        self.timings
    }

    /// Discards the parsed stylesheets, the styles, the layout and the
    /// display list, so that the next render runs all stages again. For
    /// benchmarks.
    pub fn restart_pipeline(&mut self) {
        self.invalidate_style();
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
        let (viewport, scale) = (self.viewport, self.scale);
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
        if viewport == self.viewport && (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.viewport = viewport;
        self.scale = scale;
        // Image sources (selected at the next style computation) and media
        // queries may change, so styles are recomputed.
        self.images.sources_outdated = true;
        self.invalidate_style();
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
