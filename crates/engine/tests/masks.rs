//! CSS masks in pages: mask images from files and `data:` URLs, the layer
//! geometry, failed images, stacking, hit testing, rendering at the device
//! pixel ratio, and a reduction of Wikipedia's icons. The expected results
//! were measured with Chromium 148. The pages are files in a temporary
//! directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::NetworkFetcher;
use swb_paint::{DisplayItem, MaskLayerImage};

mod common;
use common::Site;

/// A 20x20 SVG: black left half.
const LEFT_HALF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="10" height="20"/></svg>"#;

/// A 10x10 SVG: a black square.
const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;

/// Wikipedia's menu icon (`image=menu` of `skins.vector.icons`).
const MENU: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20"><title>menu</title><g><path d="M1 3v2h18V3zm0 8h18V9H1zm0 6h18v-2H1z"/></g></svg>"#;

const WHITE: (u8, u8, u8) = (255, 255, 255);
const BLUE: (u8, u8, u8) = (0, 0, 255);
const RED: (u8, u8, u8) = (255, 0, 0);

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

/// A `data:` URL of an SVG image, for `url('...')`.
fn data_url(svg: &str) -> String {
    let encoded: String = svg
        .chars()
        .map(|c| match c {
            '"' => "%22".to_string(),
            '#' => "%23".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            c => c.to_string(),
        })
        .collect();
    format!("data:image/svg+xml,{encoded}")
}

/// A page with the shared style: 50x50 blue boxes (`.b`).
fn page(site: &Site, body: &str) -> Url {
    site.page(
        "page.html",
        &format!(
            "<!DOCTYPE html><style>body {{ margin: 0 }} \
             .b {{ width: 50px; height: 50px; background: blue }}</style>{body}"
        ),
    )
}

#[test]
fn mask_images_from_files_and_data_urls() {
    let site = Site::new("mask-images");
    site.file("half.svg", LEFT_HALF.as_bytes());
    let url = page(
        &site,
        &format!(
            "<div class=b style='mask-image: url(half.svg); mask-size: 50px'></div>\
             <div class=b style=\"-webkit-mask: url('{}') 0 0 / 50px no-repeat\"></div>",
            data_url(LEFT_HALF)
        ),
    );
    let (_, p) = screenshot(url, 1.0);
    for y in [10, 60] {
        assert_eq!(rgb(&p, 5, y), BLUE, "y {y}");
        assert_eq!(rgb(&p, 24, y), BLUE, "y {y}");
        assert_eq!(rgb(&p, 26, y), WHITE, "y {y}");
        assert_eq!(rgb(&p, 45, y), WHITE, "y {y}");
    }
}

#[test]
fn failed_mask_images_hide_the_box_but_not_from_hit_testing() {
    let site = Site::new("mask-failed");
    let url = page(
        &site,
        "<div class=b id=missing style='mask-image: url(missing.svg)'></div>\
         <div class=b id=reference style='mask-image: url(#nothing)'></div>",
    );
    let (mut page, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 25, 25), WHITE);
    assert_eq!(rgb(&p, 25, 75), WHITE);
    // Masks do not change hit testing (as in Chromium).
    let missing = common::node(&page, "missing");
    let reference = common::node(&page, "reference");
    assert_eq!(page.hit_test(25.0, 25.0).map(|h| h.node), Some(missing));
    assert_eq!(page.hit_test(25.0, 75.0).map(|h| h.node), Some(reference));
}

#[test]
fn hit_testing_ignores_transparent_mask_areas() {
    let site = Site::new("mask-hit");
    site.file("square.svg", SQUARE.as_bytes());
    let url = page(
        &site,
        "<div class=b id=m style='mask: url(square.svg) no-repeat'></div>",
    );
    let (mut page, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 5, 5), BLUE);
    assert_eq!(rgb(&p, 40, 40), WHITE);
    let m = common::node(&page, "m");
    assert_eq!(page.hit_test(40.0, 40.0).map(|h| h.node), Some(m));
}

#[test]
fn layers_use_mask_origin_and_clip() {
    let site = Site::new("mask-boxes");
    site.file("square.svg", SQUARE.as_bytes());
    let url = page(
        &site,
        // The default origin is the border box (backgrounds: the padding
        // box). With `content-box`, the tile starts at (15, 15).
        // The second box starts at y = 70.
        "<div class=b style='border: 10px solid red; mask: url(square.svg) no-repeat'></div>\
         <div class=b style='border: 10px solid red; padding: 5px;\
         mask: url(square.svg) no-repeat content-box padding-box'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 5, 5), RED, "border at the tile");
    assert_eq!(rgb(&p, 15, 5), WHITE, "outside the tile");
    assert_eq!(rgb(&p, 5, 75), WHITE, "border outside the clip");
    assert_eq!(rgb(&p, 12, 82), WHITE, "padding outside the tile");
    assert_eq!(rgb(&p, 17, 87), BLUE, "content in the tile");
}

#[test]
fn descendants_are_masked_and_clipped_to_the_border_box() {
    let site = Site::new("mask-descendants");
    site.file("square.svg", SQUARE.as_bytes());
    let url = page(
        &site,
        // A gradient covers the whole border box, but not the overflow.
        "<div class=b style='mask-image: linear-gradient(black, black)'>\
         <div style='width: 100px; height: 10px; background: red'></div></div>\
         <div class=b style='mask: url(square.svg) no-repeat'>\
         <div style='position: relative; z-index: 5; left: 20px; width: 20px; height: 20px;\
         background: red'></div></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 5, 5), RED);
    assert_eq!(
        rgb(&p, 75, 5),
        WHITE,
        "overflow outside the mask painting area"
    );
    assert_eq!(rgb(&p, 5, 55), BLUE);
    assert_eq!(rgb(&p, 25, 55), WHITE, "positioned child masked");
}

#[test]
fn layers_composite_with_mask_composite() {
    let site = Site::new("mask-composite");
    site.file("square.svg", SQUARE.as_bytes());
    site.file("half.svg", LEFT_HALF.as_bytes());
    let url = page(
        &site,
        "<div class=b style='mask: url(square.svg) no-repeat, url(half.svg) no-repeat;\
         mask-composite: exclude'></div>\
         <div class=b style='mask: url(square.svg) no-repeat, url(half.svg) no-repeat;\
         -webkit-mask-composite: source-in'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    // exclude: only where exactly one layer is opaque.
    assert_eq!(rgb(&p, 5, 5), WHITE);
    assert_eq!(rgb(&p, 5, 15), BLUE);
    assert_eq!(rgb(&p, 15, 5), WHITE);
    // source-in (intersect): only where both are opaque.
    assert_eq!(rgb(&p, 5, 55), BLUE);
    assert_eq!(rgb(&p, 5, 65), WHITE);
}

#[test]
fn vector_masks_are_sharp_at_any_scale() {
    let site = Site::new("mask-scale");
    site.file("half.svg", LEFT_HALF.as_bytes());
    let url = page(
        &site,
        "<div class=b style='mask-image: url(half.svg); mask-size: 50px'></div>",
    );
    for scale in [1.0, 2.0, 3.0] {
        let (_, p) = screenshot(url.clone(), scale);
        let edge = (25.0 * scale) as u32;
        let y = (10.0 * scale) as u32;
        assert_eq!(rgb(&p, edge - 1, y), BLUE, "scale {scale}");
        assert_eq!(rgb(&p, edge, y), WHITE, "scale {scale}");
    }
}

/// A reduction of Wikipedia's header: the menu button with a
/// `.vector-icon`, using the rules of its stylesheet (`@supports` with
/// both property names, `var()` in `mask-size`, the 1x1 PNG placeholder
/// that the icon class overrides).
#[test]
fn wikipedia_menu_icon() {
    let site = Site::new("mask-wikipedia");
    site.file("menu.svg", MENU.as_bytes());
    let placeholder = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
    let url = site.page(
        "page.html",
        &format!(
            "<!DOCTYPE html><style>\
             body {{ margin: 0 }}\
             .vector-icon {{ -webkit-mask-image: url({placeholder}); mask-image: url({placeholder});\
               min-width: 10px; min-height: 10px; width: calc(var(--font-size-medium, 1rem) + 4px);\
               height: calc(var(--font-size-medium, 1rem) + 4px); display: inline-block;\
               vertical-align: text-bottom; background-color: var(--color-base, #202122) }}\
             @supports not ((-webkit-mask-image: none) or (mask-image: none)) {{\
               .vector-icon {{ background-position: center; background-repeat: no-repeat;\
               opacity: 0.87; background-color: red }} }}\
             @supports (-webkit-mask-image: none) or (mask-image: none) {{\
               .vector-icon {{ -webkit-mask-position: center; mask-position: center;\
               -webkit-mask-repeat: no-repeat; mask-repeat: no-repeat;\
               -webkit-mask-size: calc(max(calc(var(--font-size-medium, 1rem) + 4px), 10px));\
               mask-size: calc(max(calc(var(--font-size-medium, 1rem) + 4px), 10px)) }} }}\
             .vector-icon.mw-ui-icon-wikimedia-menu {{ -webkit-mask-image: url(menu.svg);\
               mask-image: url(menu.svg) }}\
             label {{ display: flex; padding: 5px }}\
             </style><label><span class='vector-icon mw-ui-icon-wikimedia-menu'></span></label>"
        ),
    );
    let (_, p) = screenshot(url, 1.0);
    // The icon is 20x20 at (5, 5): bars at y = 3..5, 9..11 and 15..17 of
    // the icon, from x = 1 to 19.
    let base = (0x20, 0x21, 0x22);
    for bar in [3, 9, 15] {
        assert_eq!(rgb(&p, 15, 5 + bar), base, "bar at {bar}");
        assert_eq!(rgb(&p, 15, 5 + bar + 1), base, "bar at {bar}");
    }
    for gap in [0, 6, 13, 19] {
        assert_eq!(rgb(&p, 15, 5 + gap), WHITE, "gap at {gap}");
    }
    assert_eq!(rgb(&p, 5, 9), WHITE, "left of the bars");
}

#[test]
fn display_list_bounds_and_layers() {
    let site = Site::new("mask-display-list");
    let url = page(
        &site,
        "<div class=b style='margin: 10px; mask: linear-gradient(black, black) 5px 5px / 20px 10px no-repeat'></div>",
    );
    let viewport = Size::new(200.0, 100.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let list = common::display_list(&mut page);
    let (bounds, layers) = list
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::PushMask { bounds, layers } => Some((*bounds, Arc::clone(layers))),
            _ => None,
        })
        .unwrap();
    // The group shows only the gradient tile.
    assert_eq!(bounds, swb_engine::Rect::new(15.0, 15.0, 20.0, 10.0));
    let [layer] = &layers[..] else {
        panic!("{layers:?}");
    };
    assert!(matches!(layer.image, MaskLayerImage::Gradient { .. }));
    let pushes = list
        .items
        .iter()
        .filter(|i| matches!(i, DisplayItem::PushMask { .. }))
        .count();
    let pops = list
        .items
        .iter()
        .filter(|i| matches!(i, DisplayItem::PopMask))
        .count();
    assert_eq!((pushes, pops), (1, 1));
}

/// Hostile masks: thousands of layers, deep nesting, and many overlapping
/// viewport-sized masks with many layers. The limits (32 layers per box,
/// the layer memory and the mask work budgets of the rasterizer) keep the
/// time and memory bounded.
#[test]
fn hostile_masks_are_bounded() {
    let site = Site::new("mask-hostile");
    let layers = vec!["linear-gradient(black, black)"; 10_000].join(", ");
    let nested = "<div class=m>".repeat(300) + &"</div>".repeat(300);
    let siblings = "<div class=m style='margin-top: -600px'></div>".repeat(500);
    let url = site.page(
        "page.html",
        &format!(
            "<!DOCTYPE html><style>body {{ margin: 0 }}\
             .m {{ min-height: 600px; background: blue; mask: {layers} }}</style>\
             {nested}{siblings}"
        ),
    );
    let viewport = Size::new(800.0, 600.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.set_viewport(viewport, 2.0);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(60));
    let started = std::time::Instant::now();
    let p = page.screenshot(false).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    assert_eq!((p.width(), p.height()), (1600, 1200));
}

/// Tiled gradients whose tiles are much larger than the visible part of
/// the mask: the rendered tiles are bounded by the visible area and
/// counted in the mask work budget. (Before, ten such boxes took 52 s.)
#[test]
fn hostile_tiled_gradient_masks_are_bounded() {
    let site = Site::new("mask-hostile-tiles");
    let layers = vec!["linear-gradient(black, transparent) 0 0 / 5000px 5001px"; 32].join(", ");
    let boxes = "<div style='width: 2px; height: 2px; overflow: hidden'>\
                 <div class=m></div></div>"
        .repeat(10);
    let url = site.page(
        "page.html",
        &format!(
            "<!DOCTYPE html><style>body {{ margin: 0 }}\
             .m {{ width: 10000px; height: 10000px; background: blue; mask: {layers} }}</style>\
             {boxes}"
        ),
    );
    let started = std::time::Instant::now();
    let (_, p) = screenshot(url, 1.0);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    // The top of the first tile is opaque.
    assert_eq!(rgb(&p, 0, 0), BLUE);
}

#[test]
fn no_clip_includes_the_outline() {
    let site = Site::new("mask-no-clip");
    let url = page(
        &site,
        "<div style='margin: 20px; width: 50px; height: 50px; background: blue;\
         outline: 5px solid red; mask: linear-gradient(black, black) no-clip'></div>\
         <div style='margin: 0 20px; width: 50px; height: 50px; background: blue;\
         outline: 5px solid red; mask: linear-gradient(black, black)'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 17, 40), RED, "outline inside the no-clip area");
    assert_eq!(rgb(&p, 40, 40), BLUE);
    // The second box starts at y = 90.
    assert_eq!(rgb(&p, 17, 95), WHITE, "outline outside the border box");
    assert_eq!(rgb(&p, 40, 95), BLUE);
}

#[test]
fn none_layers_are_transparent_black() {
    let site = Site::new("mask-none");
    site.file("square.svg", SQUARE.as_bytes());
    let url = page(
        &site,
        "<div class=b style='mask-image: url(square.svg), none'></div>\
         <div class=b style='mask-image: url(square.svg), none; mask-composite: intersect'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 5, 5), BLUE);
    assert_eq!(rgb(&p, 15, 15), BLUE, "the square repeats");
    assert_eq!(rgb(&p, 5, 55), WHITE, "intersect with transparent black");
}

#[test]
fn luminance_unsupported_images_and_opacity() {
    let grey = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="rgb(128,128,128)"/></svg>"#;
    let site = Site::new("mask-luminance");
    site.file("grey.svg", grey.as_bytes());
    site.file("square.svg", SQUARE.as_bytes());
    let url = page(
        &site,
        "<div class=b style='display: inline-block; mask: url(grey.svg) luminance'></div>\
         <div class=b style='display: inline-block; mask: url(grey.svg)'></div>\
         <div class=b style='display: inline-block; mask: radial-gradient(white, white) luminance'></div>\
         <div class=b style='opacity: 0.5; mask: url(square.svg) no-repeat'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    let half = |c: (u8, u8, u8)| c.2 == 255 && (125..=130).contains(&c.0) && c.0 == c.1;
    // Chromium: half coverage for luminance, opaque for alpha.
    assert!(half(rgb(&p, 25, 25)), "{:?}", rgb(&p, 25, 25));
    assert_eq!(rgb(&p, 75, 25), BLUE);
    // An image that swb cannot draw shows the box unmasked (here as in
    // Chromium; a gradient with transparent parts would differ).
    assert_eq!(rgb(&p, 125, 25), BLUE);
    // Mask and opacity on one box.
    let y = 54;
    assert!(half(rgb(&p, 5, y + 5)), "{:?}", rgb(&p, 5, y + 5));
    assert_eq!(rgb(&p, 40, y + 40), WHITE);
}

/// Full-page screenshots of a long page whose content is inside a mask
/// group with three layers, inside two opacity groups and a mask group,
/// inside four and five opacity groups, and inside a mask group with five
/// layers. Each group is as high as the page (30,000 device rows). The
/// screenshot is rasterized in two equal strips of 15,000 rows with equal
/// budgets, so both sides of the strip boundary look the same.
#[test]
fn full_page_screenshots_of_page_high_groups() {
    let site = Site::new("mask-full-page");
    let content = "<p style='height: 100px; margin: 0; background: blue'></p>".repeat(150);
    let gradient = "linear-gradient(black, black)";
    let pages = [
        (
            format!("<div style='mask-image: {gradient}, {gradient}, {gradient}'>{content}</div>"),
            BLUE,
        ),
        (
            format!(
                "<div style='opacity: 0.99'><div style='opacity: 0.99'>\
                 <div style='mask-image: {gradient}'>{content}</div></div></div>"
            ),
            (5, 5, 255),
        ),
        (
            format!(
                "<div style='opacity: 0.9'><div style='opacity: 0.9'><div style='opacity: 0.9'>\
                 <div style='opacity: 0.9'>{content}</div></div></div></div>"
            ),
            (88, 88, 255),
        ),
        (
            format!(
                "<div style='opacity: 0.9'><div style='opacity: 0.9'><div style='opacity: 0.9'>\
                 <div style='opacity: 0.9'><div style='opacity: 0.9'>{content}</div></div>\
                 </div></div></div>"
            ),
            (105, 105, 255),
        ),
        (
            format!(
                "<div style='mask-image: {gradient}, {gradient}, {gradient}, {gradient}, \
                 {gradient}'>{content}</div>"
            ),
            BLUE,
        ),
    ];
    let viewport = Size::new(400.0, 200.0);
    for (i, (body, expected)) in pages.iter().enumerate() {
        let url = site.page(
            &format!("page{i}.html"),
            &format!("<!DOCTYPE html><body style='margin: 0'>{body}"),
        );
        let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
        page.set_viewport(viewport, 2.0);
        page.navigate(url);
        common::finish_loading(&mut page, Duration::from_secs(30));
        let p = page.screenshot(true).unwrap();
        assert_eq!((p.width(), p.height()), (800, 30_000), "page {i}");
        for y in [5, 399, 400, 14_999, 15_000, 29_995] {
            let c = rgb(&p, 400, y);
            let close = |a: u8, b: u8| a.abs_diff(b) <= 1;
            assert!(
                close(c.0, expected.0) && close(c.1, expected.1) && c.2 == expected.2,
                "page {i}, row {y}: {c:?}"
            );
        }
    }
}

/// With `no-clip`, a repeating mask layer covers the bounding box of the
/// border box and the positioned descendants with their own overflow; the
/// overflow of in-flow descendants outside that box stays hidden
/// (Chromium 148).
#[test]
fn no_clip_covers_positioned_descendants() {
    let site = Site::new("mask-no-clip-positioned");
    let url = page(
        &site,
        "<div style='margin: 20px; width: 60px; height: 60px; background: red;\
         mask: linear-gradient(black, black) no-clip'>\
         <div style='position: relative; left: 45px; width: 30px; height: 20px; background: blue'>\
         <div style='width: 70px; height: 10px; background: lime'></div></div>\
         <div style='width: 90px; height: 10px; background: blue'></div>\
         <div style='width: 130px; height: 10px; margin-top: 15px; background: blue'></div></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 90, 35), BLUE, "positioned child outside the box");
    assert_eq!(rgb(&p, 130, 25), (0, 255, 0), "its own overflow");
    assert_eq!(
        rgb(&p, 95, 45),
        BLUE,
        "in-flow overflow inside the bounding box"
    );
    assert_eq!(
        rgb(&p, 130, 70),
        BLUE,
        "in-flow overflow inside the bounding box"
    );
    assert_eq!(rgb(&p, 145, 70), WHITE, "in-flow overflow outside it");
    // Children with opacity have their own layer too, with the positioned
    // descendants inside them.
    let url = site.page(
        "opacity.html",
        "<!DOCTYPE html><body style='margin: 0'>\
         <div style='margin: 20px; width: 60px; height: 60px; background: red;\
         mask: linear-gradient(black, black) no-clip'>\
         <div style='opacity: 0.99; width: 110px; height: 20px; background: lime'></div>\
         <div style='opacity: 0.99; width: 30px; height: 20px'>\
         <div style='position: relative; left: 100px; width: 20px; height: 10px;\
         background: blue'></div></div>\
         <div style='width: 130px; height: 10px; margin-top: 5px; background: black'></div></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 110, 30), (3, 255, 3), "child with opacity");
    assert_eq!(rgb(&p, 125, 45), (3, 3, 255), "positioned grandchild");
    assert_eq!(
        rgb(&p, 135, 65),
        (0, 0, 0),
        "in-flow overflow inside the box"
    );
    assert_eq!(rgb(&p, 145, 65), WHITE, "in-flow overflow outside it");
    // A descendant's area is clipped by its own overflow clip (Chromium).
    for (i, layer) in ["position: relative", "opacity: 0.99"].iter().enumerate() {
        let url = site.page(
            &format!("clip{i}.html"),
            &format!(
                "<!DOCTYPE html><body style='margin: 0'>\
                 <div style='margin: 20px; width: 60px; height: 60px; background: red;\
                 mask: linear-gradient(black, black) no-clip'>\
                 <div style='{layer}; overflow: hidden; width: 30px; height: 20px;\
                 background: blue'><div style='width: 200px; height: 10px; background: lime'>\
                 </div></div>\
                 <div style='width: 150px; height: 10px; margin-top: 5px; background: black'>\
                 </div></div>"
            ),
        );
        let (_, p) = screenshot(url, 1.0);
        assert_eq!(rgb(&p, 70, 45), (0, 0, 0), "{layer}: in-flow child");
        assert_eq!(rgb(&p, 130, 45), WHITE, "{layer}: in-flow overflow");
        assert_eq!(rgb(&p, 130, 25), WHITE, "{layer}: clipped overflow");
    }
}

/// A mask and opacity of the root element also apply to the canvas
/// background (Chromium 148); a mask of the body does not.
#[test]
fn root_masks_mask_the_canvas() {
    let site = Site::new("mask-root");
    site.file("half.svg", LEFT_HALF.as_bytes());
    let root = site.page(
        "root.html",
        "<!DOCTYPE html><html style='background: yellow; height: 60px;\
         mask: url(half.svg) 0 0 / 100% 100% no-repeat'>\
         <body style='margin: 20px; background: red; height: 20px'>",
    );
    let (_, p) = screenshot(root, 1.0);
    let yellow = (255, 255, 0);
    assert_eq!(
        rgb(&p, 10, 10),
        yellow,
        "canvas in the left half of the root"
    );
    assert_eq!(rgb(&p, 150, 10), WHITE, "canvas in the right half");
    assert_eq!(rgb(&p, 10, 80), WHITE, "canvas below the root");
    assert_eq!(rgb(&p, 50, 30), RED, "body in the left half");
    assert_eq!(rgb(&p, 150, 30), WHITE, "body in the right half");
    let body = site.page(
        "body.html",
        "<!DOCTYPE html><html style='height: 60px'><body style='margin: 20px;\
         background: yellow; height: 20px; mask: url(half.svg) 0 0 / 100% 100% no-repeat'>\
         <div style='background: red; height: 10px'></div>",
    );
    let (_, p) = screenshot(body, 1.0);
    assert_eq!(rgb(&p, 150, 10), yellow, "the canvas is not masked");
    assert_eq!(rgb(&p, 30, 25), RED);
    assert_eq!(rgb(&p, 150, 25), yellow, "the div is masked");
    let opacity = site.page(
        "opacity.html",
        "<!DOCTYPE html><html style='background: yellow; height: 60px; opacity: 0.5'>\
         <body style='margin: 20px; background: red; height: 20px'>",
    );
    let (_, p) = screenshot(opacity, 1.0);
    // Chromium: (255, 255, 126) inside and outside the root box.
    for (x, y) in [(10, 10), (10, 80)] {
        let (r, g, b) = rgb(&p, x, y);
        assert!(
            r == 255 && g == 255 && (125..=129).contains(&b),
            "({x}, {y}): {r} {g} {b}"
        );
    }
}

/// Nested mask groups whose gradient layers render large tiles: the tiles
/// are part of each group's work when it starts, so the budget stops the
/// chain (before, all tiles of all groups were rendered: 45.7 s).
#[test]
fn nested_tiled_gradient_masks_are_bounded() {
    let site = Site::new("mask-nested-tiles");
    let layers = vec!["linear-gradient(black, transparent) 0 0 / 64px 16384px"; 32].join(", ");
    let nested = "<div class=m>".repeat(250) + &"</div>".repeat(250);
    let url = site.page(
        "page.html",
        &format!(
            "<!DOCTYPE html><style>body {{ margin: 0 }}\
             .m {{ height: 2px; background: blue; mask: {layers} }}</style>{nested}"
        ),
    );
    let viewport = Size::new(1280.0, 800.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let started = std::time::Instant::now();
    let p = page.screenshot(false).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    // The first group renders: opaque at the top of its tiles.
    assert_eq!(rgb(&p, 1, 0), BLUE);
}

/// A rectangle thinner than half a pixel just above a strip boundary is
/// snapped into the next strip's first row, and drawn there.
#[test]
fn thin_rectangles_at_strip_boundaries() {
    let site = Site::new("mask-thin-strips");
    // 60,000 rows in two strips of 30,000 rows.
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin: 0'><div style='height: 29999.8px'></div>\
         <div style='height: 0.2px; background: black'></div><div style='height: 30000px'></div>",
    );
    let viewport = Size::new(400.0, 300.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let p = page.screenshot(true).unwrap();
    assert_eq!(p.height(), 60_000);
    assert_eq!(rgb(&p, 200, 30_000), (0, 0, 0));
    assert_eq!(rgb(&p, 200, 29_999), WHITE);
}
