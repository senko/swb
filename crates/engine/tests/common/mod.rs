//! Shared code for the engine tests: directories with test pages, page
//! setup, element lookup, and comparisons of swb's layout with stored
//! Chromium geometry (the scoring rules follow docs/testing.md, "Scores").

#![allow(dead_code)] // Each test binary uses a different subset.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{
    ElementBox, FontContext, Key, Modifiers, NodeId, Page, PageConfig, Pixmap, Rect, Size, Url,
};
use swb_net::{Fetcher, Headers, Method, NetError, NetworkFetcher, Request, Response};
use swb_paint::{DisplayList, ImageRef, ImageSizes, NoHighlights, Scrolling};

/// A directory with test pages, removed at the end of the test.
pub(crate) struct Site {
    dir: PathBuf,
}

impl Site {
    pub(crate) fn new(test: &str) -> Site {
        let dir = std::env::temp_dir().join(format!("swb-page-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cannot create the test directory");
        Site { dir }
    }

    /// Writes a file and returns its URL.
    pub(crate) fn file(&self, name: &str, content: &[u8]) -> Url {
        let path = self.dir.join(name);
        std::fs::write(&path, content).expect("cannot write a test file");
        Url::from_file_path(path).expect("an absolute path")
    }

    pub(crate) fn page(&self, name: &str, html: &str) -> Url {
        self.file(name, html.as_bytes())
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Creates a page with `fetcher`, the bundled test fonts and the scale
/// factor 1.
pub(crate) fn new_page(fetcher: Arc<dyn Fetcher>, network_threads: usize, viewport: Size) -> Page {
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads,
    };
    Page::new(config, FontContext::for_tests(), viewport, 1.0)
}

/// The element with the ID `id`.
pub(crate) fn node(page: &Page, id: &str) -> NodeId {
    page.document()
        .expect("the page has a document")
        .element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

/// The border box of the element with the ID `id` (document coordinates).
pub(crate) fn rect(page: &mut Page, id: &str) -> Rect {
    let node = node(page, id);
    page.element_box(node)
        .unwrap_or_else(|| panic!("#{id} has no box"))
}

/// Shift held.
pub(crate) const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

/// Control held.
pub(crate) const CTRL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};

/// Waits until the page is loaded, then lays it out. Fails the test if
/// loading takes longer than `timeout`.
pub(crate) fn finish_loading(page: &mut Page, timeout: Duration) {
    assert!(page.wait_until_loaded(timeout), "page did not load");
    page.update_layout();
}

/// Loads `url` with `fetcher` and the bundled test fonts, and returns the
/// element boxes. Elements with a `data-scroll="X Y"` attribute are
/// scrolled to that offset first (as `swbtools layout-refs` does in
/// Chromium).
pub(crate) fn load_boxes(fetcher: Arc<dyn Fetcher>, url: Url, viewport: Size) -> Vec<ElementBox> {
    let mut page = new_page(fetcher, 4, viewport);
    page.navigate(url);
    finish_loading(&mut page, Duration::from_secs(60));
    apply_data_scroll(&mut page);
    swb_engine::element_boxes(&page)
}

/// Scrolls the elements that have a `data-scroll="X Y"` attribute, in
/// tree order.
fn apply_data_scroll(page: &mut Page) {
    let doc = page.document().expect("the page has a document");
    let targets: Vec<(NodeId, swb_engine::Point)> = doc
        .descendants(NodeId::DOCUMENT)
        .filter_map(|n| {
            let value = doc.element(n)?.attr("data-scroll")?;
            let mut parts = value.split_whitespace().map(str::parse::<f32>);
            match (parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(y))) => Some((n, swb_engine::Point::new(x, y))),
                _ => panic!("data-scroll needs two numbers: {value:?}"),
            }
        })
        .collect();
    for (node, offset) in targets {
        page.scroll_element_to(node, offset);
    }
}

/// A 2x3 PNG.
pub(crate) fn tiny_png() -> Vec<u8> {
    Pixmap::new(2, 3)
        .expect("a 2x3 pixmap")
        .encode_png()
        .expect("PNG encoding")
}

/// Image sizes when no image is loaded.
pub(crate) struct NoImages;

impl ImageSizes for NoImages {
    fn size(&self, _image: &ImageRef) -> Option<swb_layout::NaturalSize> {
        None
    }
}

/// The display list of the page's layout, without images, without the
/// selection and without scroll offsets.
pub(crate) fn display_list(page: &mut Page) -> DisplayList {
    page.update_layout();
    swb_paint::build_display_list(
        page.fragments().expect("the page has a layout"),
        &NoImages,
        &NoHighlights,
        &Scrolling::NONE,
    )
}

/// Reads a box dump (`*.boxes.json`).
pub(crate) fn read_dump(path: &Path) -> Vec<ElementBox> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    json["elements"]
        .as_array()
        .expect("elements array")
        .iter()
        .map(|e| ElementBox {
            tag: e["tag"].as_str().unwrap_or("").to_ascii_lowercase(),
            rect: e["rect"].as_array().map(|r| {
                let v = |i: usize| r[i].as_f64().unwrap_or(0.0) as f32;
                Rect::new(v(0), v(1), v(2), v(3))
            }),
            parent: e["parent"].as_u64().map(|p| p as usize),
        })
        .collect()
}

/// Geometry comparison result.
#[derive(Debug)]
pub(crate) struct Comparison {
    /// Fraction of reference elements with a box whose swb box matches in
    /// position and size.
    pub(crate) geometry: f64,
    /// Human-readable mismatches (at most 20).
    pub(crate) mismatches: Vec<String>,
}

/// Compares swb's boxes with the reference (Chromium) boxes. Elements pair
/// by index if the tag sequences are equal, otherwise by a diff of the tag
/// sequences.
pub(crate) fn compare(
    reference: &[ElementBox],
    actual: &[ElementBox],
    tolerance: f32,
) -> Comparison {
    let pairs = align(reference, actual);
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut mismatches = Vec::new();
    for (ri, ai) in pairs {
        let Some(expected) = reference[ri].rect else {
            continue;
        };
        total += 1;
        let got = ai.and_then(|i| actual[i].rect);
        let ok = got.is_some_and(|g| {
            (g.x - expected.x).abs() <= tolerance
                && (g.y - expected.y).abs() <= tolerance
                && (g.width - expected.width).abs() <= tolerance
                && (g.height - expected.height).abs() <= tolerance
        });
        if ok {
            passed += 1;
        } else if mismatches.len() < 20 {
            mismatches.push(format!(
                "#{ri} <{}>: expected {:?}, got {:?}",
                reference[ri].tag,
                fmt_rect(expected),
                got.map(fmt_rect)
            ));
        }
    }
    let geometry = if total == 0 {
        1.0
    } else {
        passed as f64 / total as f64
    };
    Comparison {
        geometry,
        mismatches,
    }
}

fn fmt_rect(r: Rect) -> (f32, f32, f32, f32) {
    (r.x, r.y, r.width, r.height)
}

/// Pairs every reference element with an element of `actual`, or `None`.
fn align(reference: &[ElementBox], actual: &[ElementBox]) -> Vec<(usize, Option<usize>)> {
    let ref_tags: Vec<&str> = reference.iter().map(|e| e.tag.as_str()).collect();
    let act_tags: Vec<&str> = actual.iter().map(|e| e.tag.as_str()).collect();
    if ref_tags == act_tags {
        return (0..reference.len()).map(|i| (i, Some(i))).collect();
    }
    let mut paired = vec![None; reference.len()];
    let diff = similar::capture_diff_slices(similar::Algorithm::Myers, &ref_tags, &act_tags);
    for op in diff {
        if let similar::DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        {
            for k in 0..len {
                paired[old_index + k] = Some(new_index + k);
            }
        }
    }
    paired.into_iter().enumerate().collect()
}

/// Serves the pages it knows and, for other URLs, a small page that shows
/// the URL. Records every request.
#[derive(Default)]
pub(crate) struct TestSite {
    pub(crate) pages: HashMap<String, String>,
    /// The `Content-Type` of the pages; `text/html; charset=utf-8` if not
    /// set.
    pub(crate) content_type: Option<&'static str>,
    /// Answer `POST` requests with a 303 redirect to the same URL.
    pub(crate) redirect_posts: bool,
    pub(crate) requests: Mutex<Vec<Request>>,
}

impl Fetcher for TestSite {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.requests
            .lock()
            .expect("the test fetcher lock")
            .push(request.clone());
        if self.redirect_posts && request.method == Method::Post {
            let headers: Headers = [("location", request.url.as_str())].into_iter().collect();
            return Ok(Response {
                url: request.url.clone(),
                status: 303,
                headers,
                body: Vec::new(),
                redirected: false,
            });
        }
        let mut url = request.url.clone();
        url.set_fragment(None);
        let body =
            self.pages.get(url.as_str()).cloned().unwrap_or_else(|| {
                format!("<!DOCTYPE html><title>result</title><p id=url>{url}</p>")
            });
        let content_type = self.content_type.unwrap_or("text/html; charset=utf-8");
        let headers: Headers = [("content-type", content_type)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: body.into_bytes(),
            redirected: false,
        })
    }
}

impl TestSite {
    pub(crate) fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("the test fetcher lock").clone()
    }

    pub(crate) fn last_request(&self) -> Request {
        self.requests()
            .last()
            .cloned()
            .expect("the site received a request")
    }
}

/// Loads `url` with a network fetcher (for `file:` URLs) into an 800×600
/// page with the test fonts.
pub(crate) fn open_url(url: Url) -> Page {
    let mut page = new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(800.0, 600.0));
    page.navigate(url);
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

/// The center of `r`.
pub(crate) fn center(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// Clicks the center of the element with `id` (the page is not scrolled).
pub(crate) fn click(page: &mut Page, id: &str) -> bool {
    let (x, y) = center(rect(page, id));
    page.click(x, y)
}

/// Clicks `dx` px right of the left edge of the element with `id`, at the
/// vertical center.
pub(crate) fn click_offset(page: &mut Page, id: &str, dx: f32) -> bool {
    let r = rect(page, id);
    page.click(r.x + dx, r.y + r.height / 2.0)
}

/// A key press without modifiers.
pub(crate) fn key(page: &mut Page, key: &Key) -> bool {
    page.key_down(key, Modifiers::NONE)
}
