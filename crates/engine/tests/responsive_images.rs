//! Responsive images: the image source that `srcset`, `sizes` and
//! `<picture>` select at other device pixel ratios and viewport sizes than
//! the layout tests use (800x600 at scale 1), checked by the color of the
//! drawn image; the dimension attributes of a selected `<source>` after a
//! restyle; selection again after a viewport or scale change. The expected
//! choices were measured with Chromium 148.
//! The pages are files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

const RED: (u8, u8, u8) = (255, 0, 0);
const GREEN: (u8, u8, u8) = (0, 128, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
const WHITE: (u8, u8, u8) = (255, 255, 255);

/// A site with three SVG images of one color each: `red.svg` (100x50),
/// `green.svg` (200x100) and `blue.svg` (300x150).
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

/// Loads `url` with the viewport `width` x 300 at `scale`.
fn load(url: &Url, width: f32, scale: f32) -> Page {
    let viewport = Size::new(width, 300.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.set_viewport(viewport, scale);
    page.navigate(url.clone());
    common::finish_loading(&mut page, Duration::from_secs(30));
    page
}

/// The color at (x, y) in CSS px of a screenshot taken at `scale`.
fn color_at(pixmap: &Pixmap, x: f32, y: f32, scale: f32) -> (u8, u8, u8) {
    let p = pixmap
        .pixel((x * scale) as u32, (y * scale) as u32)
        .unwrap()
        .demultiply();
    (p.red(), p.green(), p.blue())
}

/// The size of `#i` and the color at its center.
fn image(page: &mut Page, scale: f32) -> ((f32, f32), (u8, u8, u8)) {
    let r = common::rect(page, "i");
    let pixmap = page.screenshot(false).unwrap();
    let color = color_at(&pixmap, r.x + r.width / 2.0, r.y + r.height / 2.0, scale);
    ((r.width, r.height), color)
}

#[test]
fn density_descriptors_follow_the_scale() {
    let site = site("srcset-density");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <img id=i srcset='red.svg 1x, green.svg 2x, blue.svg 3x' style='display:block'>",
    );
    for (scale, color) in [
        (1.0, RED),
        (1.5, GREEN),
        (2.0, GREEN),
        (2.5, BLUE),
        (4.0, BLUE),
    ] {
        let mut page = load(&url, 400.0, scale);
        assert_eq!(
            image(&mut page, scale),
            ((100.0, 50.0), color),
            "scale {scale}"
        );
    }
}

#[test]
fn width_descriptors_follow_sizes_and_the_viewport() {
    let site = site("srcset-width");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <img id=i srcset='red.svg 100w, green.svg 200w, blue.svg 300w' \
         sizes='(min-width: 600px) 150px, 50vw' style='display:block'>",
    );
    // 150px: densities 0.67, 1.33, 2. 50vw of 400px: 0.5, 1, 1.5. The
    // natural width is the source size.
    let cases = [
        (800.0, 1.0, (150.0, 75.0), GREEN),
        (800.0, 2.0, (150.0, 75.0), BLUE),
        (800.0, 3.0, (150.0, 75.0), BLUE),
        (400.0, 1.0, (200.0, 100.0), GREEN),
        (400.0, 2.0, (200.0, 100.0), BLUE),
    ];
    for (width, scale, size, color) in cases {
        let mut page = load(&url, width, scale);
        assert_eq!(
            image(&mut page, scale),
            (size, color),
            "{width}px at {scale}"
        );
    }
}

#[test]
fn picture_sources_follow_the_viewport_and_the_scale() {
    let site = site("picture-media");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'><picture>\
         <source media='(min-resolution: 2dppx)' srcset='blue.svg 3x'>\
         <source media='(min-width: 500px)' srcset='green.svg' width='84' height='29'>\
         <img id=i src='red.svg' style='display:block'></picture>",
    );
    let cases = [
        (800.0, 1.0, (84.0, 29.0), GREEN),
        (400.0, 1.0, (100.0, 50.0), RED),
        (400.0, 2.0, (100.0, 50.0), BLUE),
        (800.0, 2.0, (100.0, 50.0), BLUE),
    ];
    for (width, scale, size, color) in cases {
        let mut page = load(&url, width, scale);
        assert_eq!(
            image(&mut page, scale),
            (size, color),
            "{width}px at {scale}"
        );
    }
}

#[test]
fn source_dimensions_survive_a_restyle() {
    let site = site("picture-restyle");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><style>body { margin: 0 } a:hover { color: red }</style>\
         <a id=a href='#'>link</a><picture>\
         <source srcset='green.svg' width='84' height='29'>\
         <img id=i src='red.svg' width='25' height='25' style='display:block'></picture>",
    );
    let mut page = load(&url, 400.0, 1.0);
    assert_eq!(image(&mut page, 1.0), ((84.0, 29.0), GREEN));
    let link = common::rect(&mut page, "a");
    assert!(page.mouse_move(link.x + 2.0, link.y + 2.0));
    assert_eq!(image(&mut page, 1.0), ((84.0, 29.0), GREEN));
}

#[test]
fn sources_are_selected_again_after_a_viewport_or_scale_change() {
    let site = site("reselect");
    let density = site.page(
        "density.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <img id=i srcset='red.svg 1x, green.svg 2x' style='display:block'>",
    );
    let mut page = load(&density, 400.0, 1.0);
    assert_eq!(image(&mut page, 1.0), ((100.0, 50.0), RED));
    page.set_viewport(Size::new(400.0, 300.0), 2.0);
    // Until the new image arrives, the current one stays.
    assert_eq!(image(&mut page, 2.0), ((100.0, 50.0), RED));
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(image(&mut page, 2.0), ((100.0, 50.0), GREEN));
    // Back to scale 1: the loaded image applies at once.
    page.set_viewport(Size::new(400.0, 300.0), 1.0);
    assert_eq!(image(&mut page, 1.0), ((100.0, 50.0), RED));

    let picture = site.page(
        "picture.html",
        "<!DOCTYPE html><body style='margin:0'><picture>\
         <source media='(min-width: 500px)' srcset='green.svg' width='84' height='29'>\
         <img id=i src='red.svg' style='display:block'></picture>",
    );
    let mut page = load(&picture, 800.0, 1.0);
    assert_eq!(image(&mut page, 1.0), ((84.0, 29.0), GREEN));
    page.set_viewport(Size::new(400.0, 300.0), 1.0);
    // The dimension source changes at once; the green image stays until
    // the red one arrives.
    assert_eq!(image(&mut page, 1.0), ((200.0, 100.0), GREEN));
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(image(&mut page, 1.0), ((100.0, 50.0), RED));
}

#[test]
fn video_posters_stay_after_a_scale_change() {
    let site = site("poster-reselect");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <video id=i poster='green.svg' style='display:block'></video>\
         <img srcset='red.svg 1x, blue.svg 2x'>",
    );
    let mut page = load(&url, 400.0, 1.0);
    let before = common::rect(&mut page, "i");
    page.set_viewport(Size::new(400.0, 300.0), 2.0);
    page.update_layout();
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(common::rect(&mut page, "i"), before);
    assert_eq!((before.width, before.height), (200.0, 100.0));
    let p = page.screenshot(false).unwrap();
    // The controls darken the poster a little.
    let (r, g, b) = color_at(&p, 100.0, 5.0, 2.0);
    assert!(r < 10 && g > 100 && b < 10, "({r}, {g}, {b})");
}

#[test]
fn object_fit_uses_the_density_corrected_size() {
    // Measured in Chromium 148: the 200x100 image at 2x is drawn at 100x50,
    // centered (`none`) or at the top left (`object-position: 0 0`).
    let site = site("object-fit-density");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0;background:white'>\
         <img srcset='green.svg 2x' style='display:block;width:200px;height:200px;object-fit:none'>\
         <img srcset='green.svg 2x' style='display:block;width:200px;height:200px;\
         object-fit:none;object-position:0 0'>",
    );
    let mut page = load(&url, 400.0, 1.0);
    let p = page.screenshot(false).unwrap();
    let cases = [
        ((20.0, 100.0), WHITE),
        ((49.0, 100.0), WHITE),
        ((51.0, 100.0), GREEN),
        ((149.0, 100.0), GREEN),
        ((151.0, 100.0), WHITE),
        ((100.0, 74.0), WHITE),
        ((100.0, 76.0), GREEN),
        ((10.0, 210.0), GREEN),
        ((99.0, 210.0), GREEN),
        ((101.0, 210.0), WHITE),
        ((10.0, 249.0), GREEN),
        ((10.0, 251.0), WHITE),
    ];
    for ((x, y), color) in cases {
        assert_eq!(color_at(&p, x, y, 1.0), color, "({x}, {y})");
    }
}
