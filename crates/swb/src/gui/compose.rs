//! Frame composition (ADR 0040 section 5): the page's pixels, the toolbar
//! and the status bubble, kept between frames.
//!
//! A [`Composer`] holds two pixmaps for one window size and scale:
//!
//! - `page`: the page area, rasterized by `Page::render` when the layers
//!   are new (or the window changed), else by `Page::render_update` when
//!   the page is outdated: then only the damage of the frame is rasterized
//!   into the kept pixels (ADR 0044 section 18);
//! - `chrome`: window-sized, but only the rows of the toolbar and the
//!   rectangle of the status bubble are used.
//!
//! The toolbar is rasterized into `chrome` only when its state changed, the
//! bubble only when its text changed or the page was drawn again. The
//! window's pixels are the rows of `chrome` (toolbar) and of `page`, with
//! the bubble's rectangle of `chrome` over them; [`Composer::write_buffer`]
//! converts them in one pass. When the bubble goes away, the page's pixels
//! under it are in `page` and need no new raster.
//!
//! [`compose_window`] is the simple path (headless `--with-chrome`
//! screenshots): fresh pixmaps, the page copied with `draw_pixmap`. The tests
//! compare the two.

use std::time::Instant;

use anyhow::{Context, Result};
use swb_engine::{FrameDamage, Page, Pixmap, Size, TextEdit};
use swb_layout::{Point, Rect};
use swb_paint::{DecodedImage, ImageRef, ImageSource, RasterParams, VectorCache};

use super::toolbar::{TOOLBAR_HEIGHT, Toolbar, ToolbarState};

/// Bytes per pixel in `Pixmap` data (RGBA, 8 bits per channel).
const BYTES_PER_PIXEL: usize = 4;

/// Extra pixels around the bubble that are copied from the page before the
/// bubble is drawn (anti-aliased edges blend with them).
const BUBBLE_MARGIN: usize = 2;

struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }
}

/// What the toolbar's pixels depend on: the one list of the toolbar's
/// inputs, for the layer cache and for the frame key.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ToolbarKey {
    state: ToolbarState,
    address: TextEdit,
    focused: bool,
}

impl ToolbarKey {
    /// The inputs of the toolbar for `page` and `toolbar`.
    pub(super) fn of(page: &Page, toolbar: &Toolbar) -> Self {
        ToolbarKey {
            state: toolbar_state(page),
            address: toolbar.address.clone(),
            focused: toolbar.focused,
        }
    }
}

/// A rectangle of window pixels: columns `x0..x1`, rows `y0..y1`.
#[derive(Clone, Copy, Debug)]
struct PixelRect {
    x1: usize,
    y0: usize,
    y1: usize,
}

/// The status bubble in `chrome`.
struct Bubble {
    status: String,
    /// The pixels it covers (from the window's left edge).
    rect: PixelRect,
}

/// How many times each layer was rasterized (for the tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Rasters {
    pub(super) page: u32,
    pub(super) toolbar: u32,
    pub(super) bubble: u32,
}

/// The kept layers of a window of one size and scale.
pub(super) struct Composer {
    size: (u32, u32),
    scale: f32,
    /// Rows of the toolbar (device pixels).
    toolbar_rows: usize,
    /// 1 if the toolbar's edge covers part of the first page row (a scale
    /// for which `TOOLBAR_HEIGHT * scale` is not a whole number of pixels
    /// and rounds down), else 0. That row belongs to `chrome`.
    edge: usize,
    chrome: Pixmap,
    page: Pixmap,
    page_current: bool,
    toolbar_key: Option<ToolbarKey>,
    bubble: Option<Bubble>,
    pub(super) rasters: Rasters,
    /// What the page raster of the last composition drew (`None`: the page
    /// was not drawn), for the frame log.
    pub(super) damage: Option<FrameDamage>,
}

/// The toolbar's height in device pixels.
fn toolbar_rows(scale: f32) -> usize {
    (TOOLBAR_HEIGHT * scale).round() as usize
}

/// The size of a window of `window` CSS px at `scale`, in device pixels.
pub(super) fn device_size(window: Size, scale: f32) -> (u32, u32) {
    (
        (window.width * scale).round() as u32,
        (window.height * scale).round() as u32,
    )
}

impl Composer {
    /// Layers for a window of `size` device pixels at `scale`.
    pub(super) fn new(size: (u32, u32), scale: f32) -> Result<Self> {
        let (width, height) = size;
        let rows = toolbar_rows(scale);
        let content_h = height.saturating_sub(rows as u32).max(1);
        Ok(Composer {
            size,
            scale,
            toolbar_rows: rows,
            edge: usize::from(TOOLBAR_HEIGHT * scale > rows as f32),
            chrome: Pixmap::new(width, height).context("cannot allocate frame")?,
            page: Pixmap::new(width, content_h).context("cannot allocate page")?,
            page_current: false,
            toolbar_key: None,
            bubble: None,
            rasters: Rasters::default(),
            damage: None,
        })
    }

    /// True if these layers are for a window of `size` at `scale`.
    pub(super) fn fits(&self, size: (u32, u32), scale: f32) -> bool {
        self.size == size && self.scale == scale
    }

    fn width(&self) -> usize {
        self.size.0 as usize
    }

    fn height(&self) -> usize {
        self.size.1 as usize
    }

    /// The rows of the toolbar and of the page row that it covers in part.
    fn chrome_rows(&self) -> usize {
        (self.toolbar_rows + self.edge).min(self.height())
    }

    /// Brings the layers up to date with the page and the toolbar. Returns
    /// the time at which the page raster ended (for the frame log).
    pub(super) fn compose(&mut self, page: &mut Page, toolbar: &Toolbar) -> Instant {
        let page_drawn = !self.page_current || page.frame_outdated();
        self.damage = None;
        if page_drawn {
            // The page's pixels hold its last frame once they were drawn:
            // the next frames draw only their damage. A new composer (a new
            // window size or scale) draws everything; so does the page on
            // the first frame of a document.
            if self.page_current {
                self.damage = Some(page.render_update(&mut self.page));
            } else {
                page.render(&mut self.page);
                let pixels = u64::from(self.page.width()) * u64::from(self.page.height());
                self.damage = Some(FrameDamage {
                    full: true,
                    rects: Vec::new(),
                    viewport_pixels: pixels,
                });
            }
            self.page_current = true;
            self.rasters.page += 1;
        }
        let rendered = Instant::now();
        if self.bubble_over_toolbar_changes(page, page_drawn) {
            self.toolbar_key = None;
        }
        let toolbar_drawn = self.draw_toolbar(page, toolbar, page_drawn);
        self.draw_bubble(page, page_drawn || toolbar_drawn);
        rendered
    }

    /// True if the bubble that is shown reaches into the toolbar's rows (a
    /// window lower than the toolbar and the bubble) and is about to go
    /// away, change, or be drawn again over a new page: it was drawn over
    /// the toolbar's pixels in `chrome`, so the toolbar must be drawn again
    /// under it first (review of M11 feature 3).
    fn bubble_over_toolbar_changes(&self, page: &Page, page_drawn: bool) -> bool {
        let Some(bubble) = &self.bubble else {
            return false;
        };
        if bubble.rect.y0 >= self.chrome_rows() {
            return false;
        }
        let status = page.hovered_link().map(swb_engine::Url::as_str);
        page_drawn || status != Some(bubble.status.as_str())
    }

    /// The row `y` of the window's page area, in the page's pixmap.
    fn page_row(&self, y: usize) -> Option<&[u8]> {
        let row_bytes = self.width() * BYTES_PER_PIXEL;
        let from = y.checked_sub(self.toolbar_rows)? * row_bytes;
        self.page.data().get(from..from + row_bytes)
    }

    /// Rasterizes the toolbar into `chrome` if its state changed (or the
    /// page row under its edge did). Returns true if it did.
    fn draw_toolbar(&mut self, page: &mut Page, toolbar: &Toolbar, page_drawn: bool) -> bool {
        let key = ToolbarKey::of(page, toolbar);
        let edge_changed = page_drawn && self.edge > 0;
        if self.toolbar_key.as_ref() == Some(&key) && !edge_changed {
            return false;
        }
        let row_bytes = self.width() * BYTES_PER_PIXEL;
        let chrome_rows = self.chrome_rows();
        // The toolbar draws over transparent pixels (and over the page's
        // first row at its edge), as in a fresh frame.
        let edge_row = self.page_row(self.toolbar_rows).map(<[u8]>::to_vec);
        let data = self.chrome.data_mut();
        data[..chrome_rows * row_bytes].fill(0);
        if let Some(row) = edge_row.filter(|_| self.edge > 0) {
            let from = self.toolbar_rows * row_bytes;
            if let Some(dst) = data.get_mut(from..from + row_bytes) {
                dst.copy_from_slice(&row);
            }
        }
        let (css_w, _) = self.css_size();
        let list = toolbar.display_list(page.fonts(), css_w, &key.state);
        self.rasterize(&list, page);
        self.toolbar_key = Some(key);
        self.rasters.toolbar += 1;
        true
    }

    /// Draws the status bubble of the hovered link if it is new or if the
    /// layers under it were drawn again (`under_changed`). With no link,
    /// the bubble is gone, and the page's pixels show again at once.
    fn draw_bubble(&mut self, page: &mut Page, under_changed: bool) {
        let Some(status) = page.hovered_link().map(|u| u.as_str().to_owned()) else {
            self.bubble = None;
            return;
        };
        if !under_changed && self.bubble.as_ref().is_some_and(|b| b.status == status) {
            return;
        }
        let (css_w, css_h) = self.css_size();
        let (list, rect) = Toolbar::status_list(page.fonts(), &status, css_w, css_h);
        let pixels = self.pixel_rect(rect);
        self.copy_page_under(pixels);
        self.rasterize(&list, page);
        self.bubble = Some(Bubble {
            status,
            rect: pixels,
        });
        self.rasters.bubble += 1;
    }

    /// The window size in CSS px.
    fn css_size(&self) -> (f32, f32) {
        (
            self.size.0 as f32 / self.scale,
            self.size.1 as f32 / self.scale,
        )
    }

    /// The pixels that a rectangle in CSS px touches, with a margin, from
    /// the window's left edge.
    fn pixel_rect(&self, rect: Rect) -> PixelRect {
        let x1 = ((rect.x + rect.width) * self.scale).ceil().max(0.0) as usize + BUBBLE_MARGIN;
        let y0 = (rect.y * self.scale).floor().max(0.0) as usize;
        PixelRect {
            x1: x1.min(self.width()),
            y0: y0.saturating_sub(BUBBLE_MARGIN),
            y1: self.height(),
        }
    }

    /// Copies the page's pixels under `rect` into `chrome`, for the rows
    /// that are below the toolbar.
    fn copy_page_under(&mut self, rect: PixelRect) {
        let row_bytes = self.width() * BYTES_PER_PIXEL;
        let to = rect.x1 * BYTES_PER_PIXEL;
        for y in rect.y0.max(self.chrome_rows())..rect.y1 {
            let Some(src) = self.page_row(y) else {
                continue;
            };
            let src = src[..to].to_vec();
            if let Some(dst) = self
                .chrome
                .data_mut()
                .get_mut(y * row_bytes..y * row_bytes + to)
            {
                dst.copy_from_slice(&src);
            }
        }
    }

    /// Rasterizes a display list of the window chrome into `chrome`.
    fn rasterize(&mut self, list: &swb_paint::DisplayList, page: &mut Page) {
        swb_paint::rasterize(
            list,
            &mut self.chrome,
            RasterParams {
                scroll: Point::default(),
                viewport_scroll: Point::default(),
                scale: self.scale,
            },
            page.fonts(),
            &NoImages,
        );
    }

    /// Calls `put(row, first_column, premultiplied RGBA bytes)` for the
    /// window's pixels: each row, then the bubble's rows again (its
    /// columns), which go over the first.
    fn write_rows(&self, mut put: impl FnMut(usize, usize, &[u8])) {
        let row_bytes = self.width() * BYTES_PER_PIXEL;
        let chrome_rows = self.chrome_rows();
        let chrome = self.chrome.data();
        for y in 0..self.height() {
            let row = if y < chrome_rows {
                chrome.get(y * row_bytes..(y + 1) * row_bytes)
            } else {
                self.page_row(y)
            };
            if let Some(row) = row {
                put(y, 0, row);
            }
        }
        let Some(bubble) = &self.bubble else {
            return;
        };
        let to = bubble.rect.x1 * BYTES_PER_PIXEL;
        for y in bubble.rect.y0..bubble.rect.y1 {
            if let Some(part) = chrome.get(y * row_bytes..y * row_bytes + to) {
                put(y, 0, part);
            }
        }
    }

    /// Converts the window's pixels into a softbuffer buffer (`0RGB`), one
    /// pass over the rows. `out` has one `u32` per window pixel.
    pub(super) fn write_buffer(&self, out: &mut [u32]) {
        let width = self.width();
        self.write_rows(|y, x0, bytes| {
            let (pixels, _) = bytes.as_chunks::<4>();
            let start = y * width + x0;
            if let Some(dst) = out.get_mut(start..start + pixels.len()) {
                for (dst, src) in dst.iter_mut().zip(pixels) {
                    *dst = (u32::from(src[0]) << 16) | (u32::from(src[1]) << 8) | u32::from(src[2]);
                }
            }
        });
    }

    /// The window's pixels as a pixmap (for the tests).
    #[cfg(test)]
    pub(super) fn to_pixmap(&self) -> Pixmap {
        let mut pixmap = Pixmap::new(self.size.0, self.size.1).expect("the layers fit");
        let row_bytes = self.width() * BYTES_PER_PIXEL;
        let data = pixmap.data_mut();
        self.write_rows(|y, x0, bytes| {
            let from = y * row_bytes + x0 * BYTES_PER_PIXEL;
            data[from..from + bytes.len()].copy_from_slice(bytes);
        });
        pixmap
    }
}

/// The toolbar's view of the page's state.
fn toolbar_state(page: &Page) -> ToolbarState {
    ToolbarState {
        can_go_back: page.can_go_back(),
        can_go_forward: page.can_go_forward(),
        loading: page.is_loading(),
        javascript: page.javascript(),
        javascript_allowed: page.javascript_available(),
    }
}

/// Renders a whole window frame the simple way: fresh pixmaps, the page
/// below the toolbar copied with `draw_pixmap`, then the toolbar and the
/// status bubble. Sizes are in device pixels.
pub(super) fn compose_window(
    page: &mut Page,
    toolbar: &Toolbar,
    (width, height): (u32, u32),
    scale: f32,
) -> Result<Pixmap> {
    let mut frame = Pixmap::new(width, height).context("cannot allocate frame")?;
    let toolbar_px = toolbar_rows(scale) as u32;
    let content_h = height.saturating_sub(toolbar_px).max(1);
    let mut content = Pixmap::new(width, content_h).context("cannot allocate page")?;
    page.render(&mut content);
    frame.draw_pixmap(
        0,
        toolbar_px as i32,
        content.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    let css_width = width as f32 / scale;
    let css_height = height as f32 / scale;
    let state = toolbar_state(page);
    let mut lists = vec![toolbar.display_list(page.fonts(), css_width, &state)];
    if let Some(url) = page.hovered_link() {
        let status = url.as_str().to_owned();
        lists.push(Toolbar::status_list(page.fonts(), &status, css_width, css_height).0);
    }
    for list in &lists {
        swb_paint::rasterize(
            list,
            &mut frame,
            RasterParams {
                scroll: Point::default(),
                viewport_scroll: Point::default(),
                scale,
            },
            page.fonts(),
            &NoImages,
        );
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use swb_engine::{PageConfig, Url};

    use super::*;

    const HTML: &str = "<!doctype html><style>body{margin:0;font:16px sans-serif}\
        a{display:block;height:80px;background:#9cf}</style>\
        <a href='https://example.test/target'>link</a><p>text</p>";

    /// A page for `HTML` in a window of `window` CSS px at `scale`, with
    /// the toolbar that the window starts with.
    fn with_window(window: Size, scale: f32, test: impl FnOnce(&mut Page, &mut Toolbar) + Send) {
        // One file per call: tests run in parallel, and a test removes its
        // file at the end (a name per scale let one test remove the file
        // of another before it loaded).
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("swb-compose-{}-{call}.html", std::process::id()));
        std::fs::write(&path, HTML).expect("the temporary directory is writable");
        let url = Url::from_file_path(&path).expect("an absolute path");
        crate::stack::on_page_thread(|| {
            let config = PageConfig {
                fetcher: Arc::new(swb_net::NetworkFetcher::new()),
                notify: Arc::new(|| {}),
                network_threads: 1,
                scripts_allowed: false,
            };
            let mut page = Page::new(config, swb_engine::FontContext::for_tests(), window, scale);
            page.set_scroll_indicators(true);
            page.set_viewport(
                Size::new(window.width, window.height - TOOLBAR_HEIGHT),
                scale,
            );
            page.navigate(url);
            assert!(page.wait_until_loaded(Duration::from_secs(10)));
            let mut toolbar = Toolbar::default();
            toolbar.address.set_text("https://example.test/");
            test(&mut page, &mut toolbar);
        })
        .expect("the page thread runs");
        std::fs::remove_file(&path).ok();
    }

    /// Composes a frame and checks that it equals the simple path's.
    fn compose_and_check(composer: &mut Composer, page: &mut Page, toolbar: &Toolbar) {
        composer.compose(page, toolbar);
        let reference = compose_window(page, toolbar, composer.size, composer.scale)
            .expect("the reference frame fits");
        let frame = composer.to_pixmap();
        assert!(frame.data() == reference.data(), "the frames differ");
        // The buffer conversion gives the same pixels.
        let mut buffer = vec![0xDEAD_BEEF_u32; frame.width() as usize * frame.height() as usize];
        composer.write_buffer(&mut buffer);
        let converted: Vec<u32> = reference
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
            .collect();
        assert!(buffer == converted, "the buffers differ");
    }

    fn rasters(page: u32, toolbar: u32, bubble: u32) -> Rasters {
        Rasters {
            page,
            toolbar,
            bubble,
        }
    }

    /// The first frame, a toolbar change, a hovered link that appears and
    /// goes away, and frames that change nothing.
    fn toolbar_and_bubble(scale: f32) {
        let window = Size::new(400.0, 300.0);
        with_window(window, scale, |page, toolbar| {
            let mut composer =
                Composer::new(device_size(window, scale), scale).expect("a small window");
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 1, 0));

            // Nothing changed: no raster.
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 1, 0));

            // The address field only: the toolbar, not the page.
            toolbar.focused = true;
            toolbar.address.set_text("https://example.test/typed");
            toolbar.address.select_all();
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 2, 0));

            // A hovered link: the bubble over the page's pixels, the page
            // and the toolbar kept (the link changes no pixel of the page).
            page.mouse_move(20.0, 20.0);
            assert!(page.hovered_link().is_some());
            assert!(!page.frame_outdated(), "the link is not a page input");
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 2, 1));
            assert!(composer.bubble.is_some());
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 2, 1), "the same bubble");

            // The bubble goes away: the page's pixels come back, with no
            // raster of the page, the toolbar or the bubble.
            page.mouse_move(20.0, 200.0);
            assert!(page.hovered_link().is_none());
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 2, 1));
            assert!(composer.bubble.is_none());

            // A change of the page draws the page again; a bubble that is
            // shown is drawn again over the new pixels.
            page.mouse_move(20.0, 20.0);
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 2, 2));
            page.set_viewport(Size::new(window.width, window.height - 41.0), scale);
            assert!(page.frame_outdated());
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters.page, 2);
            assert_eq!(composer.rasters.bubble, 3);
        });
    }

    #[test]
    fn toolbar_and_bubble_frames_equal_full_frames() {
        toolbar_and_bubble(1.0);
    }

    #[test]
    fn toolbar_and_bubble_at_a_scale_with_an_edge_row() {
        // 40 * 33/32 = 41.25 px: the toolbar covers a part of the page's
        // first row.
        assert_eq!(toolbar_rows(1.03125), 41);
        toolbar_and_bubble(1.03125);
    }

    #[test]
    fn toolbar_and_bubble_at_a_scale_of_two() {
        toolbar_and_bubble(2.0);
    }

    #[test]
    fn a_new_size_or_scale_makes_new_layers() {
        let window = Size::new(400.0, 300.0);
        with_window(window, 1.0, |page, toolbar| {
            let mut composer = Composer::new((400, 300), 1.0).expect("a small window");
            assert!(composer.fits((400, 300), 1.0));
            assert!(!composer.fits((401, 300), 1.0));
            assert!(!composer.fits((400, 300), 2.0));
            compose_and_check(&mut composer, page, toolbar);

            // A resize: the page's viewport changes, so the page draws.
            page.set_viewport(Size::new(500.0, 200.0), 1.0);
            let mut composer = Composer::new((500, 240), 1.0).expect("a small window");
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 1, 0));

            // A scale change.
            page.set_viewport(Size::new(500.0, 200.0), 1.5);
            let mut composer = Composer::new((750, 360), 1.5).expect("a small window");
            compose_and_check(&mut composer, page, toolbar);
            assert_eq!(composer.rasters, rasters(1, 1, 0));
        });
    }

    #[test]
    fn a_bubble_over_the_toolbar_goes_away_with_its_pixels() {
        // A window of 41 px: the bubble at its bottom reaches into the
        // toolbar's rows (found in the review: it stayed on the toolbar).
        let window = Size::new(200.0, 41.0);
        with_window(window, 1.0, |page, toolbar| {
            let mut composer = Composer::new((200, 41), 1.0).expect("a small window");
            compose_and_check(&mut composer, page, toolbar);
            page.mouse_move(20.0, 0.5);
            assert!(page.hovered_link().is_some());
            compose_and_check(&mut composer, page, toolbar);
            assert!(composer.bubble.is_some());
            // Drawn again over a new page: not over its own old pixels.
            page.set_viewport(Size::new(200.0, 2.0), 1.0);
            compose_and_check(&mut composer, page, toolbar);
            // Gone.
            page.mouse_leave();
            assert!(page.hovered_link().is_none());
            compose_and_check(&mut composer, page, toolbar);
            assert!(composer.bubble.is_none());
        });
    }

    #[test]
    fn a_window_lower_than_the_toolbar_does_not_panic() {
        let window = Size::new(100.0, 300.0);
        with_window(window, 1.0, |page, toolbar| {
            for height in [1, 20, 40, 41, 62] {
                let mut composer = Composer::new((100, height), 1.0).expect("a small window");
                page.mouse_move(20.0, 20.0);
                composer.compose(page, toolbar);
                let mut buffer = vec![0; 100 * height as usize];
                composer.write_buffer(&mut buffer);
            }
        });
    }
}
