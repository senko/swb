//! Web fonts (`@font-face`) in a page: loading only what text needs, each
//! URL once, `src` fallback, URLs relative to the style sheet, waiting for
//! fonts before the page counts as loaded, and a new face set for each
//! document. Pages are files in a temporary directory; the widths are
//! Chromium 148's (`tools/probes/web-fonts.json`, case
//! `basic-and-shadowing`).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{LoadState, Page, Size, Url};
use swb_net::{Fetcher, NetError, NetworkFetcher, Request, Response};

mod common;
use common::Site;

/// "Hamburgefonstiv 0123" at 16px in `DejaVu Sans`, and in the fallback
/// family `Liberation Sans`.
const DEJAVU_WIDTH: f32 = 183.91;
const SANS_WIDTH: f32 = 160.98;

/// Fetches local files and records the file names of the requests.
struct Recorder {
    inner: NetworkFetcher,
    fetched: Mutex<Vec<String>>,
}

impl Recorder {
    fn fonts(&self) -> Vec<String> {
        self.fetched
            .lock()
            .unwrap()
            .iter()
            .filter(|name| matches!(name.rsplit('.').next(), Some("ttf" | "woff2")))
            .cloned()
            .collect()
    }
}

impl Fetcher for Recorder {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let name = request.url.path().rsplit('/').next().unwrap_or("");
        self.fetched.lock().unwrap().push(name.to_owned());
        self.inner.fetch(request)
    }
}

fn site(test: &str) -> Site {
    let site = Site::new(test);
    let font = std::fs::read(common::repo_root().join("fixtures/fonts/DejaVuSans.ttf")).unwrap();
    site.file("dv.ttf", &font);
    site.file("other.ttf", &font);
    site
}

fn load(url: &Url) -> (Page, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        inner: NetworkFetcher::new(),
        fetched: Mutex::new(Vec::new()),
    });
    let mut page = common::new_page(recorder.clone(), 2, Size::new(800.0, 600.0));
    page.navigate(url.clone());
    common::finish_loading(&mut page, Duration::from_secs(30));
    (page, recorder)
}

fn width(page: &mut Page, id: &str) -> f32 {
    (common::rect(page, id).width * 100.0).round() / 100.0
}

fn text_page(css: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><style>{css} body{{margin:0;font:16px sans-serif}} \
         span{{white-space:pre}}</style>{body}"
    )
}

const TEXT: &str = "Hamburgefonstiv 0123";

#[test]
fn a_used_face_loads_before_the_page_is_loaded() {
    let site = site("used");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:W;src:url(dv.ttf)} \
             @font-face{font-family:Unused;src:url(other.ttf)}",
            &format!("<span id=w style='font-family:W, sans-serif'>{TEXT}</span>"),
        ),
    );
    let (mut page, recorder) = load(&url);
    assert_eq!(page.load_state(), LoadState::Complete);
    assert_eq!(width(&mut page, "w"), DEJAVU_WIDTH);
    assert_eq!(recorder.fonts(), ["dv.ttf"]);
}

#[test]
fn one_url_loads_once_for_many_faces() {
    let site = site("once");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:W;src:url(dv.ttf);font-weight:400} \
             @font-face{font-family:W;src:url(dv.ttf);font-weight:700}",
            &format!(
                "<span id=a style='font-family:W'>{TEXT}</span><br>\
                 <span id=b style='font-family:W;font-weight:700'>{TEXT}</span>"
            ),
        ),
    );
    let (mut page, recorder) = load(&url);
    assert_eq!(width(&mut page, "a"), DEJAVU_WIDTH);
    // Synthetic bold does not change the advances.
    assert_eq!(width(&mut page, "b"), DEJAVU_WIDTH);
    assert_eq!(recorder.fonts(), ["dv.ttf"]);
}

#[test]
fn sources_are_tried_in_order() {
    let site = site("fallback");
    site.file("bad.ttf", b"not a font");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:A;src:local(Nothing),url(missing.ttf),url(bad.ttf),url(dv.ttf)} \
             @font-face{font-family:B;src:url(missing.ttf)}",
            &format!(
                "<span id=a style='font-family:A, sans-serif'>{TEXT}</span><br>\
                 <span id=b style='font-family:B, sans-serif'>{TEXT}</span>"
            ),
        ),
    );
    let (mut page, recorder) = load(&url);
    assert_eq!(width(&mut page, "a"), DEJAVU_WIDTH);
    assert_eq!(width(&mut page, "b"), SANS_WIDTH);
    let mut fonts = recorder.fonts();
    fonts.sort();
    assert_eq!(fonts, ["bad.ttf", "dv.ttf", "missing.ttf"]);
}

#[test]
fn urls_resolve_against_the_style_sheet() {
    let site = site("sheet");
    let font = std::fs::read(common::repo_root().join("fixtures/fonts/DejaVuSans.ttf")).unwrap();
    let dir = site.file("x", b"").to_file_path().unwrap();
    std::fs::create_dir_all(dir.parent().unwrap().join("css")).unwrap();
    site.file("css/f.ttf", &font);
    site.file("css/s.css", b"@font-face{font-family:W;src:url(f.ttf)}");
    let url = site.page(
        "p.html",
        &format!(
            "<!DOCTYPE html><link rel=stylesheet href=css/s.css>\
             <style>body{{margin:0;font:16px sans-serif}} span{{white-space:pre}}</style>\
             <span id=w style='font-family:W'>{TEXT}</span>"
        ),
    );
    let (mut page, recorder) = load(&url);
    assert_eq!(width(&mut page, "w"), DEJAVU_WIDTH);
    assert_eq!(recorder.fonts(), ["f.ttf"]);
}

#[test]
fn each_document_has_its_own_faces() {
    let site = site("documents");
    let first = site.page(
        "a.html",
        &text_page(
            "@font-face{font-family:W;src:url(dv.ttf)}",
            &format!("<span id=w style='font-family:W, sans-serif'>{TEXT}</span>"),
        ),
    );
    let second = site.page(
        "b.html",
        &text_page(
            "",
            &format!("<span id=w style='font-family:W, sans-serif'>{TEXT}</span>"),
        ),
    );
    let (mut page, _) = load(&first);
    assert_eq!(width(&mut page, "w"), DEJAVU_WIDTH);
    page.navigate(second);
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(width(&mut page, "w"), SANS_WIDTH);
}
