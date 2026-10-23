//! The JavaScript setting (ADR 0028 section 15): it applies to the next
//! document; the document's scripting flag changes the parser (`<noscript>`
//! is raw text and loads nothing), `@media (scripting)`, the controls of
//! videos and `<canvas>`. No script runs. Pages come from an in-memory
//! fetcher; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page, node, rect};

const URL: &str = "https://site.test/page.html";

fn site(html: &str) -> Arc<TestSite> {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    Arc::new(site)
}

/// A page on `site` with the setting `javascript`, not yet navigated.
fn page(site: &Arc<TestSite>, javascript: bool) -> Page {
    let mut page = new_page(
        Arc::clone(site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 600.0),
    );
    page.set_javascript(javascript);
    page
}

fn open(page: &mut Page) {
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(page, Duration::from_secs(20));
}

fn load(html: &str, javascript: bool) -> (Arc<TestSite>, Page) {
    let site = site(html);
    let mut page = page(&site, javascript);
    open(&mut page);
    (site, page)
}

fn rgb(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let p = pixmap.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

fn paths(site: &TestSite) -> Vec<String> {
    let mut paths: Vec<String> = site
        .requests()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    paths.sort();
    paths
}

const NOSCRIPT: &str = "<!DOCTYPE html><head><noscript><link rel=stylesheet href=head.css>\
    </noscript></head><body><noscript id=n><img src=body.png><p id=p>inside</p></noscript>\
    <div id=after>after</div>";

#[test]
fn the_setting_is_off_by_default() {
    let site = site(NOSCRIPT);
    let page = page(&site, false);
    assert!(!page.javascript());
    let page = new_page(site as Arc<dyn Fetcher>, 1, Size::new(100.0, 100.0));
    assert!(!page.javascript());
    assert!(!page.document_has_scripting());
}

#[test]
fn noscript_loads_nothing_with_javascript_on() {
    let (site, page) = load(NOSCRIPT, true);
    assert!(page.document_has_scripting());
    assert_eq!(paths(&site), ["/page.html"]);
    // The text is raw: the element has one text child, no `img`, no `p`.
    let dump = swb_dom::dump_tree(page.document().unwrap());
    assert!(
        dump.contains("\"<img src=body.png><p id=p>inside</p>\""),
        "{dump}"
    );
    let doc = page.document().unwrap();
    let element_names: Vec<String> = doc
        .descendants(swb_dom::NodeId::DOCUMENT)
        .filter_map(|n| doc.element(n).map(|e| e.local_name().to_string()))
        .collect();
    assert_eq!(
        element_names,
        ["html", "head", "noscript", "body", "noscript", "div"]
    );
    assert!(doc.element_by_id("p").is_none());
    assert_eq!(
        swb_dom::outer_html(doc, doc.element_by_id("n").unwrap()),
        "<noscript id=\"n\"><img src=body.png><p id=p>inside</p></noscript>"
    );
}

#[test]
fn noscript_content_loads_and_shows_with_javascript_off() {
    let (site, mut page) = load(NOSCRIPT, false);
    assert!(!page.document_has_scripting());
    assert_eq!(paths(&site), ["/body.png", "/head.css", "/page.html"]);
    assert!(page.element_box(node(&page, "p")).is_some());
}

#[test]
fn noscript_has_no_box_with_javascript_on() {
    let html = "<!DOCTYPE html><style>noscript { display: block; height: 80px }</style>\
        <body style='margin:0'><noscript id=n>text</noscript><div id=after>after</div>";
    let (_, mut on) = load(html, true);
    assert_eq!(on.element_box(node(&on, "n")), None);
    assert_eq!(rect(&mut on, "after").y, 0.0);
    let (_, mut off) = load(html, false);
    assert_eq!(rect(&mut off, "n").height, 80.0);
    assert_eq!(rect(&mut off, "after").y, 80.0);
}

const MEDIA: &str = "<!DOCTYPE html><style>#m { width: 10px; height: 10px } \
    @media (scripting: enabled) { #m { width: 20px } } \
    @media (scripting: none) { #m { width: 30px } } \
    @media (scripting) { #m { height: 40px } }</style><div id=m></div>";

#[test]
fn scripting_media_feature_follows_the_document() {
    let (_, mut on) = load(MEDIA, true);
    let r = rect(&mut on, "m");
    assert_eq!((r.width, r.height), (20.0, 40.0));
    let (_, mut off) = load(MEDIA, false);
    let r = rect(&mut off, "m");
    assert_eq!((r.width, r.height), (30.0, 10.0));
}

#[test]
fn the_setting_applies_to_the_next_document() {
    let site = site(MEDIA);
    let mut page = page(&site, false);
    open(&mut page);
    assert_eq!(rect(&mut page, "m").width, 30.0);

    // A change does not touch the current document, also after a new layout.
    page.set_javascript(true);
    assert!(page.javascript());
    assert!(!page.document_has_scripting());
    page.set_viewport(Size::new(500.0, 500.0), 1.0);
    page.update_layout();
    assert_eq!(rect(&mut page, "m").width, 30.0);

    // A reload creates the next document.
    page.reload();
    finish_loading(&mut page, Duration::from_secs(20));
    assert!(page.document_has_scripting());
    assert_eq!(rect(&mut page, "m").width, 20.0);

    // And back.
    page.set_javascript(false);
    assert!(page.document_has_scripting());
    assert_eq!(rect(&mut page, "m").width, 20.0);
    page.reload();
    finish_loading(&mut page, Duration::from_secs(20));
    assert!(!page.document_has_scripting());
    assert_eq!(rect(&mut page, "m").width, 30.0);
}

#[test]
fn two_pages_in_one_process_can_differ() {
    let site = site(MEDIA);
    let mut on = page(&site, true);
    let mut off = page(&site, false);
    open(&mut on);
    open(&mut off);
    assert_eq!(rect(&mut on, "m").width, 20.0);
    assert_eq!(rect(&mut off, "m").width, 30.0);
    assert_ne!(on.document_has_scripting(), off.document_has_scripting());
}

const VIDEO: &str = "<!DOCTYPE html><body style='margin:0'>\
    <video id=a width=200 height=100 style='display:block'></video>\
    <video id=b width=200 height=100 controls style='display:block'></video>";

#[test]
fn video_controls_need_the_attribute_with_javascript_on() {
    // The default poster of a video is dark gray with controls (with their
    // gradient on top).
    let dark = |p: (u8, u8, u8)| p.0 < 64 && p.1 < 64 && p.2 < 64;
    let (_, mut off) = load(VIDEO, false);
    let shot = off.screenshot(false).unwrap();
    assert!(dark(rgb(&shot, 100, 30)), "controls with scripting off");
    assert!(dark(rgb(&shot, 100, 130)));

    let (_, mut on) = load(VIDEO, true);
    let shot = on.screenshot(false).unwrap();
    assert_eq!(rgb(&shot, 100, 30), (255, 255, 255), "no controls: nothing");
    assert!(dark(rgb(&shot, 100, 130)), "controls attribute");
    // Both videos have the same boxes in both modes.
    assert_eq!(rect(&mut on, "a").height, 100.0);
    assert_eq!(rect(&mut on, "b").y, 100.0);
}

const CANVAS: &str = "<!DOCTYPE html><body style='margin:0;font:16px/20px sans-serif'>\
    <canvas id=c1>fallback <b id=b>text</b></canvas><br>\
    <canvas id=c2 width=100 height=50 style='display:block'></canvas>\
    <div id=after>after</div>";

#[test]
fn canvas_is_replaced_and_paints_nothing_with_javascript_on() {
    let (_, mut on) = load(CANVAS, true);
    let c1 = rect(&mut on, "c1");
    assert_eq!((c1.width, c1.height), (300.0, 150.0));
    let c2 = rect(&mut on, "c2");
    assert_eq!((c2.width, c2.height), (100.0, 50.0));
    assert_eq!(on.element_box(node(&on, "b")), None, "no fallback content");
    let shot = on.screenshot(false).unwrap();
    for (x, y) in [(2, 2), (150, 75), (298, 148), (50, 175)] {
        assert_eq!(rgb(&shot, x, y), (255, 255, 255), "{x},{y}");
    }

    // With the setting off, the fallback content shows.
    let (_, mut off) = load(CANVAS, false);
    assert!(off.element_box(node(&off, "b")).is_some());
    assert_ne!(rect(&mut off, "c1").height, 150.0);
}

#[test]
fn a_canvas_with_a_background_paints_its_box() {
    let html = "<!DOCTYPE html><body style='margin:0'>\
        <canvas width=40 height=20 style='background:#f00;display:block'></canvas>";
    let (_, mut on) = load(html, true);
    let shot = on.screenshot(false).unwrap();
    assert_eq!(rgb(&shot, 20, 10), (255, 0, 0));
    assert_eq!(rgb(&shot, 60, 10), (255, 255, 255));
}
