//! Inline `<svg>` in pages (ADR 0023): sizing, the box dump of the
//! element and its descendants, painting with the device pixel ratio,
//! styles from the page, clipping, and hit testing of the box. The pages
//! are files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

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

const WHITE: (u8, u8, u8) = (255, 255, 255);
const RED: (u8, u8, u8) = (255, 0, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);

/// A 2x2 checkerboard in a 20x20 viewport: red squares at the top left and
/// bottom right.
const CHECKER: &str = "<svg width=20 height=20 viewBox='0 0 2 2' fill=red>\
    <title>Checker</title><rect width=1 height=1 /><rect x=1 y=1 width=1 height=1 /></svg>";

#[test]
fn inline_svg_paints_at_the_device_pixel_ratio() {
    let site = Site::new("inline-svg-scale");
    let url = site.page(
        "page.html",
        &format!("<!DOCTYPE html><body style='margin:0'><div>{CHECKER}</div>"),
    );
    for scale in [1.0, 2.0, 3.0] {
        let (page, p) = screenshot(url.clone(), scale);
        let boxes = swb_engine::element_boxes(&page);
        let svg = boxes.iter().find(|b| b.tag == "svg").unwrap();
        assert_eq!(
            svg.rect.map(|r| (r.x, r.y, r.width, r.height)),
            Some((0.0, 0.0, 20.0, 20.0))
        );
        // The descendants have no boxes (part 2 adds them as Chromium
        // reports them), and the title is not text of the page.
        for tag in ["title", "rect"] {
            assert!(
                boxes
                    .iter()
                    .filter(|b| b.tag == tag)
                    .all(|b| b.rect.is_none()),
                "{tag}"
            );
        }
        let edge = (10.0 * scale) as u32;
        let middle = (5.0 * scale) as u32;
        assert_eq!(rgb(&p, edge - 1, middle), RED, "scale {scale}");
        assert_eq!(rgb(&p, edge, middle), WHITE, "scale {scale}");
        assert_eq!(rgb(&p, middle, edge), WHITE, "scale {scale}");
        assert_eq!(rgb(&p, edge, edge), RED, "scale {scale}");
    }
}

#[test]
fn page_styles_apply_and_content_is_clipped() {
    let site = Site::new("inline-svg-styles");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><style>.icon { width: 40px; height: 40px; color: blue; \
         padding: 5px; display: block } .icon path { fill: currentColor }</style>\
         <body style='margin:0'><svg class=icon viewBox='0 0 10 10'>\
         <path d='M-5 -5h20v20h-20z' fill=red /></svg>",
    );
    let (_, p) = screenshot(url, 1.0);
    // The page's rule beats the presentation attribute; the content is
    // clipped to the content box (inside the padding).
    assert_eq!(rgb(&p, 25, 25), BLUE);
    assert_eq!(rgb(&p, 6, 6), BLUE);
    assert_eq!(rgb(&p, 3, 3), WHITE);
    assert_eq!(rgb(&p, 47, 25), WHITE);
}

/// `<style>` in the SVG namespace is a document style sheet (measured in
/// Chromium, `tools/probes/inline-svg.json`, case `style-element`): it
/// styles SVG and HTML elements, ignores a `type` other than `text/css`,
/// and honors `media`.
#[test]
fn svg_style_elements_are_document_style_sheets() {
    let site = Site::new("inline-svg-style-element");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <svg width=20 height=20><style>.a{fill:red} .h{background:blue;height:10px}</style>\
         <style type='text/foo'>.b{fill:blue}</style>\
         <style media='print'>.c{fill:blue}</style>\
         <style type='TEXT/CSS' media='screen'>.d{fill:red}</style>\
         <rect class=a width=5 height=5 /><rect class=b x=5 width=5 height=5 fill=green />\
         <rect class=c x=10 width=5 height=5 fill=green />\
         <rect class=d x=15 width=5 height=5 /></svg><div class=h></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 2, 2), RED);
    assert_eq!(rgb(&p, 7, 2), (0, 128, 0));
    assert_eq!(rgb(&p, 12, 2), (0, 128, 0));
    assert_eq!(rgb(&p, 17, 2), RED);
    // The rule also styles the HTML element below the `<svg>`. (The
    // `<svg>` is an inline box on a line of 20 px plus the descender.)
    let (_, p2) = screenshot(
        site.page(
            "html.html",
            "<!DOCTYPE html><body style='margin:0'><svg width=1 height=1><style>.h{background:blue;\
             height:10px}</style></svg><div class=h></div>",
        ),
        1.0,
    );
    assert!((0..40).any(|y| rgb(&p2, 100, y) == BLUE));
}

/// The `type` rule of SVG `<style>` holds for HTML `<style>` too: a type
/// other than empty or `text/css` (ASCII case-insensitive) turns the
/// element off (measured in Chromium, case `style-element`).
#[test]
fn html_style_elements_with_another_type_are_ignored() {
    let site = Site::new("html-style-type");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <style type='text/foo'>.a{background:red}</style>\
         <style type='Text/CSS'>.b{background:blue}</style>\
         <style type=''>.c{background:blue}</style>\
         <div class=a style='height:10px'></div><div class=b style='height:10px'></div>\
         <div class=c style='height:10px'></div>",
    );
    let (_, p) = screenshot(url, 1.0);
    assert_eq!(rgb(&p, 5, 5), WHITE);
    assert_eq!(rgb(&p, 5, 15), BLUE);
    assert_eq!(rgb(&p, 5, 25), BLUE);
}

#[test]
fn inline_svg_is_an_atomic_inline_on_the_baseline() {
    let site = Site::new("inline-svg-inline");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin:0; font: 20px/40px sans-serif'>x<a href=next.html>\
         <svg width=20 height=20><rect width=20 height=20 fill=red /></svg></a>\
         <svg width=20 height=60></svg>y",
    );
    let (mut page, p) = screenshot(url, 1.0);
    let boxes = swb_engine::element_boxes(&page);
    let svgs: Vec<_> = boxes.iter().filter(|b| b.tag == "svg").collect();
    // As measured in Chromium (tools/probes/inline-svg.json): the bottom
    // of each box is on the baseline.
    let bottoms: Vec<f32> = svgs
        .iter()
        .map(|b| b.rect.map_or(0.0, |r| r.y + r.height))
        .collect();
    assert_eq!(bottoms, vec![60.0, 60.0]);
    let first = svgs[0].rect.unwrap();
    assert_eq!(
        rgb(&p, (first.x + 10.0) as u32, (first.y + 10.0) as u32),
        RED
    );
    // Hit testing finds the `<svg>` box inside the link.
    let link = page
        .hit_test(first.x + 10.0, first.y + 10.0)
        .and_then(|hit| hit.link)
        .unwrap();
    assert!(link.path().ends_with("/next.html"), "{link}");
}
