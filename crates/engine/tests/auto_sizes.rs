//! `sizes="auto"`: lazy-loaded images choose their source by the laid-out
//! width of their box, after layout, and again when the width changes; the
//! 100vw candidates are not requested. Images that are not lazy-loaded, and
//! images without a box, use `100vw` (Chromium 148). The user-agent rule
//! (`contain: size`) keeps the size of such an image independent of its
//! source. Pages are files in a temporary directory; the sources are SVG
//! files of one color each: `red.svg` (100x50), `green.svg` (200x100) and
//! `blue.svg` (300x150).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Page, Size, Url};
use swb_net::{Fetcher, NetError, NetworkFetcher, Request, Response};

mod common;
use common::Site;

const RED: (u8, u8, u8) = (255, 0, 0);
const GREEN: (u8, u8, u8) = (0, 128, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);

/// Fetches local files and records the file names of the requests.
struct Recorder {
    inner: NetworkFetcher,
    fetched: Mutex<Vec<String>>,
}

impl Recorder {
    fn new() -> Arc<Recorder> {
        Arc::new(Recorder {
            inner: NetworkFetcher::new(),
            fetched: Mutex::new(Vec::new()),
        })
    }

    /// The names of the images requested so far, in order, without
    /// duplicates.
    fn images(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for name in self.fetched.lock().unwrap().iter() {
            if name.contains(".svg") && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
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
    for (name, width, height) in [("red", 100, 50), ("green", 200, 100), ("blue", 300, 150)] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="{name}"/></svg>"#
        );
        site.file(&format!("{name}.svg"), svg.as_bytes());
    }
    site
}

fn load(url: &Url, width: f32) -> (Page, Arc<Recorder>) {
    let recorder = Recorder::new();
    let viewport = Size::new(width, 300.0);
    let mut page = common::new_page(recorder.clone(), 2, viewport);
    page.navigate(url.clone());
    common::finish_loading(&mut page, Duration::from_secs(30));
    (page, recorder)
}

/// The size of `#i` and the color at its center.
fn image(page: &mut Page) -> ((f32, f32), (u8, u8, u8)) {
    let r = common::rect(page, "i");
    let pixmap = page.screenshot(false).unwrap();
    let p = pixmap
        .pixel((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32)
        .unwrap()
        .demultiply();
    ((r.width, r.height), (p.red(), p.green(), p.blue()))
}

const SRCSET: &str = "srcset='red.svg 100w, green.svg 200w, blue.svg 300w'";

fn page_with(site: &Site, img: &str) -> Url {
    site.page(
        "page.html",
        &format!("<!DOCTYPE html><body style='margin:0'>{img}"),
    )
}

#[test]
fn a_lazy_image_uses_the_width_of_its_box() {
    let site = site("auto-lazy");
    let url = page_with(
        &site,
        &format!(
            "<img id=i {SRCSET} sizes=auto loading=lazy width=150 height=75 style='display:block'>"
        ),
    );
    // 150px: densities 0.67, 1.33, 2. At 100vw (400px) it would be blue.
    let (mut page, recorder) = load(&url, 400.0);
    assert_eq!(image(&mut page), ((150.0, 75.0), GREEN));
    assert_eq!(recorder.images(), ["green.svg"]);
}

#[test]
fn the_width_can_come_from_style_and_excludes_padding_and_border() {
    let site = site("auto-style");
    let url = page_with(
        &site,
        &format!(
            "<div style='width:60%'><img id=i {SRCSET} sizes='auto, 10px' loading=LAZY \
             style='display:block;width:100%;padding:20px;border:5px solid;box-sizing:border-box'></div>"
        ),
    );
    // 60% of 400px is 240px, minus 50px: a content width of 190px.
    let (mut page, recorder) = load(&url, 400.0);
    assert_eq!(recorder.images(), ["green.svg"]);
    // The height is 150px (contain-intrinsic-size) plus 50px.
    let (size, _) = image(&mut page);
    assert!((size.0 - 240.0).abs() < 0.01 && size.1 == 200.0, "{size:?}");
}

#[test]
fn images_that_are_not_lazy_use_100vw() {
    let site = site("auto-eager");
    // Viewport 120px: 100vw gives densities 0.83 and 1.67, so green. The
    // box is 300px wide (contain-intrinsic-size) and would give blue.
    for loading in ["", "loading=eager", "loading=bogus"] {
        let url = page_with(
            &site,
            &format!("<img id=i {SRCSET} sizes='auto, 90px' {loading} style='display:block'>"),
        );
        let (mut page, recorder) = load(&url, 120.0);
        assert_eq!(image(&mut page), ((300.0, 150.0), GREEN), "{loading}");
        assert_eq!(recorder.images(), ["green.svg"], "{loading}");
    }
    let url = page_with(
        &site,
        &format!("<img id=i {SRCSET} sizes='auto, 90px' loading=lazy style='display:block'>"),
    );
    let (mut page, recorder) = load(&url, 120.0);
    assert_eq!(image(&mut page), ((300.0, 150.0), BLUE));
    assert_eq!(recorder.images(), ["blue.svg"]);
}

#[test]
fn the_user_agent_rule_gives_the_default_size() {
    let site = site("auto-contain");
    // No dimensions: 300x150 whatever the source. A width or height
    // attribute and an aspect ratio still apply.
    for (attrs, size) in [
        ("", (300.0, 150.0)),
        ("width=100", (100.0, 150.0)),
        ("height=100", (300.0, 100.0)),
        ("width=100 height=100", (100.0, 100.0)),
        ("style='contain-intrinsic-size:50px 25px'", (50.0, 25.0)),
        ("style='aspect-ratio:2/1;width:100px'", (100.0, 50.0)),
    ] {
        let url = page_with(
            &site,
            &format!("<img id=i {SRCSET} sizes=auto loading=lazy {attrs} style='display:block'>"),
        );
        let (mut page, _) = load(&url, 400.0);
        assert_eq!(image(&mut page).0, size, "{attrs}");
    }
}

#[test]
fn an_image_without_a_box_uses_100vw() {
    let site = site("auto-none");
    let url = page_with(
        &site,
        &format!("<img id=i {SRCSET} sizes=auto loading=lazy style='display:none'>"),
    );
    let (_page, recorder) = load(&url, 120.0);
    assert_eq!(recorder.images(), ["green.svg"]);
}

#[test]
fn the_source_changes_with_the_width_and_the_viewport() {
    let site = site("auto-resize");
    let url = page_with(
        &site,
        &format!(
            "<img id=i {SRCSET} sizes='auto, 10px' loading=lazy style='display:block;width:50vw;height:30px'>"
        ),
    );
    // 50vw of 400px is 200px: densities 0.5, 1, 1.5.
    let (mut page, recorder) = load(&url, 400.0);
    assert_eq!(image(&mut page), ((200.0, 30.0), GREEN));
    page.set_viewport(Size::new(160.0, 300.0), 1.0);
    // 80px: densities 1.25, 2.5, 3.75, so red. Until it arrives, green stays.
    assert_eq!(image(&mut page), ((80.0, 30.0), GREEN));
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(image(&mut page), ((80.0, 30.0), RED));
    assert_eq!(recorder.images(), ["green.svg", "red.svg"]);
    // Back: the loaded image applies at once and nothing is requested again.
    page.set_viewport(Size::new(400.0, 300.0), 1.0);
    assert_eq!(image(&mut page), ((200.0, 30.0), GREEN));
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(recorder.images(), ["green.svg", "red.svg"]);
}

#[test]
fn a_new_scale_selects_again_with_the_same_width() {
    let site = site("auto-scale");
    let url = page_with(
        &site,
        &format!(
            "<img id=i {SRCSET} sizes=auto loading=lazy width=150 height=75 style='display:block'>"
        ),
    );
    let (mut page, recorder) = load(&url, 400.0);
    assert_eq!(image(&mut page), ((150.0, 75.0), GREEN));
    // At scale 2 the densities 0.67, 1.33, 2 need blue.
    page.set_viewport(Size::new(400.0, 300.0), 2.0);
    page.update_layout();
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(recorder.images(), ["green.svg", "blue.svg"]);
}

#[test]
fn an_image_inside_a_matching_picture_source_uses_the_source() {
    let site = site("auto-picture");
    let url = page_with(
        &site,
        &format!(
            "<picture><source media='(min-width: 100px)' srcset='red.svg'>\
             <img id=i {SRCSET} sizes=auto loading=lazy width=150 height=75></picture>"
        ),
    );
    let (_page, recorder) = load(&url, 400.0);
    assert_eq!(recorder.images(), ["red.svg"]);
    // Without a matching source, the image selects as before.
    let url = site.page(
        "unmatched.html",
        &format!(
            "<!DOCTYPE html><picture><source media='(max-width: 10px)' srcset='red.svg'>\
             <img id=i {SRCSET} sizes=auto loading=lazy width=150 height=75></picture>"
        ),
    );
    let (_page, recorder) = load(&url, 400.0);
    assert_eq!(recorder.images(), ["green.svg"]);
}

#[test]
fn many_images_select_once_each() {
    let site = site("auto-many");
    let imgs = format!("<img {SRCSET} sizes=auto loading=lazy width=150 height=75>").repeat(2000);
    let url = page_with(&site, &imgs);
    let (_page, recorder) = load(&url, 400.0);
    assert_eq!(recorder.images(), ["green.svg"]);
    assert_eq!(
        recorder
            .fetched
            .lock()
            .unwrap()
            .iter()
            .filter(|n| n.contains(".svg"))
            .count(),
        1
    );
}
