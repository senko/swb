//! SVG images in pages: `<img>` and CSS backgrounds, rendering at the
//! device pixel ratio, decode failures, and image documents. The pages are
//! files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

/// Hacker News' vote arrow (`triangle.svg`).
const TRIANGLE: &str = r##"<svg height="32" viewBox="0 0 32 16" width="32" xmlns="http://www.w3.org/2000/svg"><path d="m2 27 14-29 14 29z" fill="#999"/></svg>"##;

/// A 2x2 checkerboard: black squares at the top left and bottom right.
const CHECKER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 2 2"><rect width="1" height="1"/><rect x="1" y="1" width="1" height="1"/></svg>"#;

/// Loads `url` in a 200x100 viewport at `scale` and returns a screenshot.
fn screenshot(url: Url, scale: f32) -> (Page, Pixmap) {
    let viewport = Size::new(200.0, 100.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.set_viewport(viewport, scale);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let pixmap = page.screenshot(false).unwrap();
    (page, pixmap)
}

fn rgb(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let p = pixmap.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

const BLACK: (u8, u8, u8) = (0, 0, 0);
const WHITE: (u8, u8, u8) = (255, 255, 255);

#[test]
fn img_renders_at_the_device_pixel_ratio() {
    let site = Site::new("svg-img-scale");
    site.file("checker.svg", CHECKER.as_bytes());
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'><img src='checker.svg' style='display:block'>",
    );
    for scale in [1.0, 2.0, 3.0] {
        let (page, p) = screenshot(url.clone(), scale);
        let boxes = swb_engine::element_boxes(&page);
        let img = boxes.iter().find(|b| b.tag == "img").unwrap();
        assert_eq!(img.rect.map(|r| (r.width, r.height)), Some((20.0, 20.0)));
        // The edge between the squares is a sharp pixel edge.
        let edge = (10.0 * scale) as u32;
        let middle = (5.0 * scale) as u32;
        assert_eq!(rgb(&p, edge - 1, middle), BLACK, "scale {scale}");
        assert_eq!(rgb(&p, edge, middle), WHITE, "scale {scale}");
        assert_eq!(rgb(&p, middle, edge), WHITE, "scale {scale}");
        assert_eq!(rgb(&p, edge, edge), BLACK, "scale {scale}");
    }
}

#[test]
fn img_with_a_border_draws_inside_it() {
    // Hacker News' logo: an 18x18 image with a white 1 px border.
    let site = Site::new("svg-img-border");
    site.file("checker.svg", CHECKER.as_bytes());
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0;background:#f60'>\
         <img src='checker.svg' width='18' height='18' style='border:1px white solid;display:block'>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 0, 0), WHITE, "border");
    assert_eq!(rgb(&p, 1, 1), BLACK, "image");
    assert_eq!(rgb(&p, 18, 18), BLACK, "image");
    assert_eq!(
        rgb(&p, 18, 1),
        (255, 102, 0),
        "transparent part of the image"
    );
    assert_eq!(rgb(&p, 19, 19), WHITE, "border");
    assert_eq!(rgb(&p, 25, 5), (255, 102, 0), "page");
}

#[test]
fn background_svg_is_sized_and_drawn() {
    // As Hacker News draws its vote arrows.
    let site = Site::new("svg-background");
    site.file("triangle.svg", TRIANGLE.as_bytes());
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='width:10px;height:10px;margin:3px 2px 6px;\
         background:url(triangle.svg), linear-gradient(transparent, transparent) no-repeat;\
         background-size:10px'></div>\
         <div style='width:40px;height:10px;background:url(triangle.svg);background-size:10px'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    // One 10x10 tile at (2, 3): grey at the bottom middle, white corners.
    let grey = |c: (u8, u8, u8)| c.0 == c.1 && c.1 == c.2 && c.0 < 200;
    assert!(grey(rgb(&p, 7, 3 + 9)), "{:?}", rgb(&p, 7, 12));
    assert_eq!(rgb(&p, 2, 3), WHITE);
    assert_eq!(rgb(&p, 7, 3), WHITE, "above the apex");
    assert_eq!(rgb(&p, 13, 12), WHITE, "outside the tile");
    // Repeated: four tiles in the 40x10 box at y = 19.
    for tile in 0..4 {
        let x = tile * 10 + 5;
        assert!(grey(rgb(&p, x, 19 + 9)), "tile {tile}");
        assert_eq!(rgb(&p, tile * 10, 19), WHITE, "tile {tile}");
    }
}

#[test]
fn broken_svg_images_are_broken() {
    let site = Site::new("svg-broken");
    site.file(
        "bad.svg",
        b"<svg xmlns='http://www.w3.org/2000/svg'><g></svg>",
    );
    // SVG data with the wrong type is not sniffed as SVG.
    site.file("svg.png", CHECKER.as_bytes());
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <img src='bad.svg'><img src='svg.png'><img src='bad.svg' width='30' height='10'>",
    );
    let (page, _) = screenshot(url, 1.0);
    let boxes = swb_engine::element_boxes(&page);
    let sizes: Vec<_> = boxes
        .iter()
        .filter(|b| b.tag == "img")
        .map(|b| b.rect.map(|r| (r.width, r.height)))
        .collect();
    assert_eq!(
        sizes,
        [Some((0.0, 0.0)), Some((0.0, 0.0)), Some((30.0, 10.0))]
    );
}

#[test]
fn svg_images_do_not_load_files() {
    let site = Site::new("svg-no-files");
    let mut red = Pixmap::new(2, 2).unwrap();
    for pixel in red.data_mut().chunks_mut(4) {
        pixel.copy_from_slice(&[255, 0, 0, 255]);
    }
    site.file("red.png", &red.encode_png().unwrap());
    site.file(
        "image.svg",
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
            <image href="red.png" width="20" height="20"/></svg>"#,
    );
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'><img src='image.svg' style='display:block'>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 10, 10), WHITE);
}

#[test]
fn svg_documents_show_the_image() {
    let site = Site::new("svg-document");
    let url = site.file("checker.svg", CHECKER.as_bytes());
    let (page, p) = screenshot(url, 1.0);
    let boxes = swb_engine::element_boxes(&page);
    let img = boxes.iter().find(|b| b.tag == "img").unwrap();
    assert_eq!(img.rect.map(|r| (r.width, r.height)), Some((20.0, 20.0)));
    assert_eq!(rgb(&p, 5, 5), BLACK);
    assert_eq!(rgb(&p, 15, 5), WHITE);
}

#[test]
fn many_sizes_of_one_image_render_in_one_frame() {
    // 60 widths of one image: one frame renders at most a few sizes of it
    // and reuses the closest for the others; every image is drawn in the
    // first frame.
    let site = Site::new("svg-many-sizes");
    site.file("checker.svg", CHECKER.as_bytes());
    let images = (1..=60)
        .map(|w| format!("<img src='checker.svg' width='{w}' height='4'>"))
        .collect::<Vec<_>>()
        .concat();
    let url = site.page(
        "page.html",
        &format!("<!DOCTYPE html><body style='margin:0;line-height:0'>{images}"),
    );
    let (page, p) = screenshot(url, 1.0);
    // Every image is drawn: each box has dark pixels.
    let boxes = swb_engine::element_boxes(&page);
    for rect in boxes
        .iter()
        .filter(|b| b.tag == "img")
        .filter_map(|b| b.rect)
    {
        let (x0, y0) = (rect.x as u32, rect.y as u32);
        let drawn = (x0..x0 + rect.width as u32)
            .flat_map(|x| (y0..y0 + rect.height as u32).map(move |y| (x, y)))
            .any(|(x, y)| rgb(&p, x, y).0 < 250);
        assert!(drawn, "{rect:?}");
    }
}
