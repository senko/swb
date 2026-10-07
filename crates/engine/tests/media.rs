//! Media elements in pages: the poster of a video loads like an image and
//! the media resource is never requested; the poster is painted with
//! `object-fit`; the controls and the default poster are painted; hit
//! testing finds the video, not its children. Pages come from an
//! in-memory fetcher that records the requests; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::{Destination, Fetcher, Headers, NetError, Request, Response};
use swb_style::Rgba;

mod common;
use common::{node, rect};

const ORIGIN: &str = "https://site.test/";

/// Serves fixed responses and records every request.
#[derive(Default)]
struct TestSite {
    responses: HashMap<String, (&'static str, Vec<u8>)>,
    requests: Mutex<Vec<Request>>,
}

impl TestSite {
    fn with(mut self, path: &str, content_type: &'static str, body: Vec<u8>) -> Self {
        self.responses
            .insert(format!("{ORIGIN}{path}"), (content_type, body));
        self
    }

    fn requests(&self) -> Vec<(String, Destination)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.url.path().to_owned(), r.destination))
            .collect()
    }
}

impl Fetcher for TestSite {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.requests.lock().unwrap().push(request.clone());
        let (content_type, body) = self
            .responses
            .get(request.url.as_str())
            .cloned()
            .unwrap_or(("text/plain", Vec::new()));
        let status = if self.responses.contains_key(request.url.as_str()) {
            200
        } else {
            404
        };
        let headers: Headers = [("content-type", content_type)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A solid red PNG of `width` × `height` px.
fn red_png(width: u32, height: u32) -> Vec<u8> {
    let mut pixmap = Pixmap::new(width, height).unwrap();
    swb_paint::fill(&mut pixmap, Rgba::rgb(255, 0, 0));
    pixmap.encode_png().unwrap()
}

/// Loads `html` (served as `/page.html`, with a 100×50 red PNG at
/// `/poster.png`; other URLs answer 404) in a 600×600 viewport.
fn load(html: &str) -> (Arc<TestSite>, Page, Pixmap) {
    let site = Arc::new(
        TestSite::default()
            .with("page.html", "text/html", html.as_bytes().to_vec())
            .with("poster.png", "image/png", red_png(100, 50)),
    );
    let mut page = common::new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 600.0),
    );
    page.navigate(Url::parse(&format!("{ORIGIN}page.html")).unwrap());
    common::finish_loading(&mut page, Duration::from_secs(20));
    let pixmap = page.screenshot(false).unwrap();
    (site, page, pixmap)
}

fn rgb(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let p = pixmap.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

const PAGE: &str = "<!DOCTYPE html><body style='margin:0'>\
    <video id=v poster=poster.png width=200 height=150 preload=none controls style='display:block; background:blue'>\
    <source id=s1 src=movie.webm type=video/webm><source id=s2 src=movie.mp4>\
    <track src=subtitles.vtt>fallback <a id=fallback href=x>text</a></video>\
    <video id=d width=250 height=159 style='display:block'></video>\
    <video id=b poster=missing.png width=100 height=100 style='display:block; background:blue'></video>\
    <video id=a src=autoplay.webm preload=auto autoplay width=100 height=50 style='display:block'></video>";

#[test]
fn poster_loads_as_an_image_and_media_is_never_requested() {
    let (site, mut page, _) = load(PAGE);
    let mut requests = site.requests();
    // The two posters load in parallel. The `src` of the autoplay video
    // with `preload=auto` is not requested either.
    requests[1..].sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        requests,
        [
            ("/page.html".to_owned(), Destination::Document),
            ("/missing.png".to_owned(), Destination::Image),
            ("/poster.png".to_owned(), Destination::Image),
        ]
    );
    // The video takes the attributes' size; its children have no boxes.
    assert_eq!(rect(&mut page, "v").width, 200.0);
    assert_eq!(rect(&mut page, "v").height, 150.0);
    for id in ["s1", "s2", "fallback"] {
        let n = node(&page, id);
        assert_eq!(page.element_box(n), None, "#{id}");
    }
}

#[test]
fn poster_is_contained_and_controls_are_painted() {
    let (_, _, p) = load(PAGE);
    // object-fit: contain: the 100×50 poster fills the width (200×100),
    // centered vertically; the background shows above it.
    assert_eq!(rgb(&p, 100, 10), (0, 0, 255), "letterbox");
    assert_eq!(rgb(&p, 100, 30), (255, 0, 0), "poster");
    // The gradient darkens the bottom of the poster.
    let (r, g, b) = rgb(&p, 100, 115);
    assert!(r < 200 && g == 0 && b == 0, "{:?}", (r, g, b));
    // The white play triangle, centered 48 px above the bottom (y 102).
    assert_eq!(rgb(&p, 24, 102), (255, 255, 255), "play");
    // The timeline: white at 30 % over the gradient (over the blue
    // background there), 20 to 24 px above the bottom edge.
    let (r, g, b) = rgb(&p, 100, 128);
    assert!(r > 60 && r == g && b >= g, "timeline {:?}", (r, g, b));
    assert!(rgb(&p, 100, 132).0 < 10, "below the timeline");
}

#[test]
fn video_without_poster_shows_the_default_poster() {
    let (_, mut page, p) = load(PAGE);
    let d = rect(&mut page, "d");
    assert_eq!((d.x, d.y, d.width, d.height), (0.0, 150.0, 250.0, 159.0));
    assert_eq!(rgb(&p, 125, 160), (0x33, 0x33, 0x33));
    // No source: the play button is dimmed (white at 30 % over the
    // darkened gray).
    let (r, ..) = rgb(&p, 24, 150 + 159 - 48);
    assert!(r > 60 && r < 140, "dimmed play {r}");
}

/// A poster that fails to load is not the default poster: the background
/// shows (Chromium 148).
#[test]
fn broken_poster_shows_the_background() {
    let (_, mut page, p) = load(PAGE);
    let b = rect(&mut page, "b");
    assert_eq!((b.y, b.width, b.height), (309.0, 100.0, 100.0));
    // The gradient covers the whole 100 px high video, but it is almost
    // transparent at the top.
    let top = rgb(&p, 50, 310);
    let lower = rgb(&p, 50, 340);
    assert!(top.0 == 0 && top.1 == 0 && top.2 > 200, "{top:?}");
    assert!(lower.2 < top.2, "the gradient darkens it: {lower:?}");
}

#[test]
fn hit_testing_finds_the_video() {
    let (_, mut page, _) = load(PAGE);
    let v = node(&page, "v");
    assert_eq!(page.hit_test(100.0, 40.0).map(|h| h.node), Some(v));
    // Over the controls, and where the fallback link would be.
    assert_eq!(page.hit_test(24.0, 102.0).map(|h| h.node), Some(v));
    let hit = page.hit_test(5.0, 5.0).unwrap();
    assert_eq!(hit.node, v);
    assert_eq!(hit.link, None);
}
