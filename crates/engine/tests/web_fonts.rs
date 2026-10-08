//! Web fonts (`@font-face`) in a page: loading only what text needs, each
//! URL once, `src` fallback, URLs relative to the style sheet, waiting for
//! fonts before the page counts as loaded, and a new face set for each
//! document. Pages are files in a temporary directory; the widths are
//! Chromium 148's (`tools/probes/web-fonts.json`, case
//! `basic-and-shadowing`).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
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

// ----- Part 2 (`tools/probes/web-fonts-2.json`) -----

/// A site with the variable test font as `v.ttf`.
fn variable_site(test: &str) -> Site {
    let site = site(test);
    let font =
        std::fs::read(common::repo_root().join("crates/text/tests/webfonts/swb-variable.ttf"))
            .unwrap();
    site.file("v.ttf", &font);
    site
}

fn height(page: &mut Page, id: &str) -> f32 {
    (common::rect(page, id).height * 100.0).round() / 100.0
}

/// A page with 100 px text, one `div` of "A" per entry of `divs` (id and
/// style).
fn big_page(css: &str, divs: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (id, style) in divs {
        write!(
            body,
            "<div id={id} style=\"float:left;clear:left;white-space:pre;{style}\">A</div>"
        )
        .unwrap();
    }
    format!("<!DOCTYPE html><style>{css} body{{margin:0;font:100px serif}}</style>{body}")
}

#[test]
fn font_variation_settings_change_the_axes() {
    let site = variable_site("fvs");
    let url = site.page(
        "p.html",
        &big_page(
            "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900;font-stretch:75% 125%}",
            &[
                ("a", "font-family:V"),
                ("b", "font-family:V;font-weight:700"),
                ("c", "font-family:V;font-weight:700;font-variation-settings:'wght' 660"),
                ("d", "font-family:V;font-variation-settings:'wght' 1000"),
                ("e", "font-family:V;font-variation-settings:'wght' 900,'wght' 100"),
                ("f", "font-family:V;font-stretch:75%;font-variation-settings:'wdth' 125"),
                ("g", "font-family:V;font-variation-settings:'wght' 900;font:100px V"),
                // An invalid declaration is ignored.
                ("h", "font-family:V;font-variation-settings:'wght' 900;font-variation-settings:'wgh' 1"),
            ],
        ),
    );
    let (mut page, _) = load(&url);
    let widths: Vec<f32> = "abcdefgh"
        .chars()
        .map(|c| width(&mut page, &c.to_string()))
        .collect();
    // Chromium 148: 60, 84, 80.81 (not 84), 100, 40, 80, 60 (`font` resets
    // the setting), 100.
    assert_eq!(widths, [60.0, 84.0, 80.81, 100.0, 40.0, 80.0, 60.0, 100.0]);
}

#[test]
fn font_variation_settings_are_inherited() {
    let site = variable_site("fvs-inherit");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900} \
             #p{font:100px V;font-variation-settings:'wght' 900} \
             #c{font-variation-settings:inherit}",
            "<div id=p><span id=c style='white-space:pre'>A</span>\
             <span id=n style='white-space:pre;font-variation-settings:normal'>A</span></div>",
        ),
    );
    let (mut page, _) = load(&url);
    assert_eq!(width(&mut page, "c"), 100.0);
    assert_eq!(width(&mut page, "n"), 60.0);
}

#[test]
fn font_feature_settings_change_shaping() {
    let site = site("ffs");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:W;src:url(dv.ttf)} \
             @font-face{font-family:D;src:url(dv.ttf);font-feature-settings:'liga' 0} \
             span{font-size:100px}",
            "<span id=a style='font-family:W'>fi fl ffi AV To</span><br>\
             <span id=b style='font-family:W;font-feature-settings:\"liga\" 0'>fi fl ffi AV To</span><br>\
             <span id=c style='font-family:W;font-feature-settings:\"kern\" off, \"liga\" off'>fi fl ffi AV To</span><br>\
             <span id=d style='font-family:D'>fi fl ffi AV To</span><br>\
             <span id=e style='font-family:D;font-feature-settings:\"liga\" on'>fi fl ffi AV To</span>",
        ),
    );
    let (mut page, _) = load(&url);
    // Chromium 148.
    assert_eq!(width(&mut page, "a"), 585.5);
    assert_eq!(width(&mut page, "b"), 587.02);
    assert_eq!(width(&mut page, "c"), 610.41);
    assert_eq!(width(&mut page, "d"), 587.02);
    assert_eq!(width(&mut page, "e"), 585.5);
}

#[test]
fn metric_descriptors_change_the_line_box() {
    let site = variable_site("metrics");
    let marker = "<i id=m style='display:inline-block;width:1px;height:0'></i>";
    let url = site.page(
        "p.html",
        &format!(
            "<!DOCTYPE html><style>\
             @font-face{{font-family:P;src:url(v.ttf)}} \
             @font-face{{font-family:H;src:url(v.ttf);size-adjust:50%}} \
             @font-face{{font-family:O;src:url(v.ttf);size-adjust:50%;ascent-override:50%;\
             descent-override:50%;line-gap-override:20%}} \
             @font-face{{font-family:Z;src:url(v.ttf);size-adjust:150%}} \
             body{{margin:0;font:100px serif}} div{{float:left;clear:left;white-space:pre}}\
             </style>\
             <div id=p style='font-family:P'>A</div>\
             <div id=h style='font-family:H'>A</div>\
             <div id=o style='font-family:O'>A{marker}</div>\
             <div id=z style='font-family:Z'>A</div>"
        ),
    );
    let (mut page, _) = load(&url);
    // Chromium 148 (probe case `metric-descriptors`).
    assert_eq!(
        (width(&mut page, "p"), height(&mut page, "p")),
        (60.0, 100.0)
    );
    assert_eq!(
        (width(&mut page, "h"), height(&mut page, "h")),
        (30.0, 50.0)
    );
    assert_eq!(
        (width(&mut page, "z"), height(&mut page, "z")),
        (90.0, 150.0)
    );
    // Ascent 25, descent 25 and a line gap of 10 (ratios of the adjusted
    // size 50 px) make a line of 60 px with the baseline at 25 + 5.
    let o = common::rect(&mut page, "o");
    let m = common::rect(&mut page, "m");
    assert_eq!(o.height, 60.0);
    assert_eq!(m.y - o.y, 30.0);
}

#[test]
fn local_sources_need_no_request() {
    let site = site("local");
    let url = site.page(
        "p.html",
        &text_page(
            "@font-face{font-family:A;src:local('DejaVu Sans'),url(other.ttf)} \
             @font-face{font-family:B;src:local('Arial'),url(dv.ttf)} \
             @font-face{font-family:C;src:local('Nope')}",
            &format!(
                "<span id=a style='font-family:A, sans-serif'>{TEXT}</span><br>\
                 <span id=b style='font-family:B, sans-serif'>{TEXT}</span><br>\
                 <span id=c style='font-family:C, sans-serif'>{TEXT}</span>"
            ),
        ),
    );
    let (mut page, recorder) = load(&url);
    assert_eq!(page.load_state(), LoadState::Complete);
    assert_eq!(width(&mut page, "a"), DEJAVU_WIDTH);
    // `local(Arial)` does not match (Arial is an alias, not a full name),
    // so the second source loads.
    assert_eq!(width(&mut page, "b"), DEJAVU_WIDTH);
    assert_eq!(width(&mut page, "c"), SANS_WIDTH);
    assert_eq!(recorder.fonts(), ["dv.ttf"]);
}
