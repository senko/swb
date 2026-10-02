//! Headless mode: load a page without a window, then save a screenshot or
//! print debugging dumps.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use swb_engine::{Page, PageConfig, Pixmap, Size, Url};
use swb_layout::FragmentRef;
use swb_net::Fetcher;
use swb_text::FontContext;

/// What to produce in headless mode.
pub(crate) struct HeadlessOptions<'a> {
    pub(crate) url: Url,
    pub(crate) viewport: Size,
    pub(crate) scale: f32,
    pub(crate) timeout: Duration,
    pub(crate) screenshot: Option<&'a Path>,
    pub(crate) full_page: bool,
    pub(crate) with_chrome: bool,
    pub(crate) dump_dom: bool,
    pub(crate) dump_layout: bool,
    pub(crate) dump_boxes: Option<&'a Path>,
}

/// Loads the page and produces the requested outputs.
pub(crate) fn run(
    fetcher: Arc<dyn Fetcher>,
    fonts: FontContext,
    options: &HeadlessOptions<'_>,
) -> Result<()> {
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads: 6,
    };
    let mut page = Page::new(config, fonts, options.viewport, options.scale);
    page.navigate(options.url.clone());
    if !page.wait_until_loaded(options.timeout) {
        log::warn!("timed out waiting for the page to load");
    }
    page.update_layout();
    if let Some(error) = page.error() {
        log::warn!("{error}");
    }

    if options.dump_dom {
        let doc = page.document().context("no document")?;
        print!("{}", swb_dom::dump_tree(doc));
    }
    if options.dump_layout {
        print!("{}", dump_layout(&page));
    }
    if let Some(path) = options.dump_boxes {
        let json = box_dump(&page, &options.url, options.viewport);
        std::fs::write(path, json).with_context(|| format!("cannot write {}", path.display()))?;
    }
    if let Some(path) = options.screenshot.filter(|_| options.with_chrome) {
        device_size(options.viewport, options.scale)?;
        let pixmap = crate::gui::render_window(&mut page, options.viewport, options.scale)?;
        pixmap
            .save_png(path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        return Ok(());
    }
    if let Some(path) = options.screenshot {
        let size = if options.full_page {
            let size = full_page_size(&mut page, options.viewport.width, options.scale);
            page.set_viewport(size, options.scale);
            size
        } else {
            options.viewport
        };
        let (w, h) = device_size(size, options.scale)?;
        let Some(mut pixmap) = Pixmap::new(w, h) else {
            bail!("cannot allocate a {w}x{h} screenshot");
        };
        page.render(&mut pixmap);
        pixmap
            .save_png(path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        log::info!("saved {}", path.display());
    }
    Ok(())
}

/// The largest screenshot in device pixels: 128 Mpx, 512 MiB of RGBA.
const MAX_SCREENSHOT_PIXELS: f64 = 128.0 * 1024.0 * 1024.0;

/// The size of a full-page screenshot: the content height, limited so that
/// the screenshot stays within [`MAX_SCREENSHOT_PIXELS`].
fn full_page_size(page: &mut Page, width: f32, scale: f32) -> Size {
    let content = page.content_size();
    // One row less than the limit, so that rounding cannot exceed it.
    let device_width = f64::from((width * scale).round().max(1.0));
    let max_height =
        (((MAX_SCREENSHOT_PIXELS / device_width).floor() - 1.0) / f64::from(scale)) as f32;
    if content.height > max_height {
        log::warn!(
            "the page is {} px high; the screenshot shows the first {max_height:.0} px",
            content.height
        );
    }
    Size::new(width, content.height.min(max_height).max(1.0))
}

/// The size in device pixels of a screenshot of `size` CSS px, if it is
/// within [`MAX_SCREENSHOT_PIXELS`]. tiny-skia aborts the process when an
/// allocation fails, so the size is checked before allocating.
fn device_size(size: Size, scale: f32) -> Result<(u32, u32)> {
    let w = (size.width * scale).round().max(1.0);
    let h = (size.height * scale).round().max(1.0);
    if f64::from(w) * f64::from(h) > MAX_SCREENSHOT_PIXELS {
        bail!("a {w}x{h} screenshot is too large");
    }
    Ok((w as u32, h as u32))
}

/// The element box dump used for comparisons with Chromium (format:
/// docs/testing.md): every element in tree order with the union of its
/// border boxes, or `null` if it has none.
fn box_dump(page: &Page, url: &Url, viewport: Size) -> String {
    let round = |v: f32| (f64::from(v) * 100.0).round() / 100.0;
    let elements: Vec<_> = swb_engine::element_boxes(page)
        .into_iter()
        .map(|b| {
            let rect = b.rect.map(|r| {
                serde_json::json!([round(r.x), round(r.y), round(r.width), round(r.height)])
            });
            serde_json::json!({ "tag": b.tag, "rect": rect, "parent": b.parent })
        })
        .collect();
    let dump = serde_json::json!({
        "url": url.as_str(),
        "viewport": [viewport.width, viewport.height],
        "elements": elements,
    });
    serde_json::to_string_pretty(&dump).unwrap_or_default() + "\n"
}

/// One line per box fragment: depth, element, and absolute border box.
fn dump_layout(page: &Page) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let (Some(doc), Some(tree)) = (page.document(), page.fragments()) else {
        return out;
    };
    tree.walk(|fragment, origin| match fragment {
        FragmentRef::Box(b) => {
            let r = b.border_rect.translate(origin);
            let name = b
                .node
                .and_then(|n| doc.element(n))
                .map_or_else(|| "(anonymous)".to_owned(), |e| e.local_name().to_string());
            let pseudo = b.pseudo.map_or(String::new(), |p| format!("::{p:?}"));
            let _ = writeln!(
                out,
                "{name}{pseudo} {:.2},{:.2} {:.2}x{:.2}",
                r.x, r.y, r.width, r.height
            );
        }
        FragmentRef::Text(t) => {
            let r = t.rect.translate(origin);
            let _ = writeln!(
                out,
                "  text {:.2},{:.2} {:.2}x{:.2} {:?}",
                r.x, r.y, r.width, r.height, t.text
            );
        }
    });
    out
}
