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
        // The title has no box (and is not text of the page); the shapes
        // have their bounding boxes, mapped through the view box.
        assert!(
            boxes
                .iter()
                .filter(|b| b.tag == "title")
                .all(|b| b.rect.is_none())
        );
        let rects: Vec<_> = boxes
            .iter()
            .filter(|b| b.tag == "rect")
            .map(|b| b.rect.map(|r| (r.x, r.y, r.width, r.height)))
            .collect();
        assert_eq!(
            rects,
            [Some((0.0, 0.0, 10.0, 10.0)), Some((10.0, 10.0, 10.0, 10.0))]
        );
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

/// The box in the box dump of the `nth` element with tag `tag`.
fn dump_box(page: &Page, tag: &str, nth: usize) -> Option<(f32, f32, f32, f32)> {
    let boxes = swb_engine::element_boxes(page);
    let b = boxes.iter().filter(|b| b.tag == tag).nth(nth)?;
    b.rect.map(|r| (r.x, r.y, r.width, r.height))
}

const BLACK: (u8, u8, u8) = (0, 0, 0);

/// Loads `html` in a viewport of `width` x `height` at scale 1.
fn render(site: &Site, html: &str, width: f32, height: f32) -> (Page, Pixmap) {
    let url = site.page("page.html", html);
    let viewport = Size::new(width, height);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.set_viewport(viewport, 1.0);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let pixmap = page.screenshot(false).unwrap();
    (page, pixmap)
}

/// The box dump has the bounding boxes of `g`, shapes and `use` (measured
/// in Chromium 148, `tools/probes/inline-svg.json`, cases `box-*`): fill
/// bounding box, transforms applied, not clipped, `display: none` and
/// `defs` content excluded, `visibility: hidden` included.
#[test]
fn svg_descendants_have_bounding_boxes() {
    let site = Site::new("inline-svg-boxes");
    let (page, _) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'><svg width=200 height=100 viewBox='0 0 100 50' \
         style='margin:10px'><defs><clipPath id=c><rect width=5 height=5 /></clipPath>\
         <rect id=r width=10 height=10 /></defs><title>t</title>\
         <g><rect x=10 y=10 width=20 height=10 stroke=red stroke-width=4 />\
         <circle cx=50 cy=20 r=10 clip-path='url(#c)' /></g>\
         <g transform='translate(20 5) scale(2)'><rect width=10 height=10 transform=rotate(45) /></g>\
         <g style='display:none'><rect width=3 height=3 /></g>\
         <g style='visibility:hidden'><rect x=70 width=5 height=5 /></g>\
         <path d='M10 40' /><rect y=45 width=-1 height=5 />\
         <use href='#r' x=60 y=30 /></svg>",
        300.0,
        150.0,
    );
    // The elements in `defs`, and `title`, have no box.
    assert_eq!(dump_box(&page, "defs", 0), None);
    assert_eq!(dump_box(&page, "clippath", 0), None);
    assert_eq!(dump_box(&page, "title", 0), None);
    assert_eq!(dump_box(&page, "rect", 0), None);
    assert_eq!(dump_box(&page, "rect", 1), None);
    // The first group: the union of its shapes, strokes and clips ignored
    // (the circle is 20 x 20 user units, 40 px); the svg is at (10, 10).
    assert_eq!(dump_box(&page, "g", 0), Some((30.0, 30.0, 100.0, 40.0)));
    assert_eq!(dump_box(&page, "rect", 2), Some((30.0, 30.0, 40.0, 20.0)));
    assert_eq!(dump_box(&page, "circle", 0), Some((90.0, 30.0, 40.0, 40.0)));
    // The bounding box of the rotated square is axis-aligned.
    let rotated = dump_box(&page, "rect", 3).unwrap();
    assert!((rotated.2 - 56.57).abs() < 0.01, "{rotated:?}");
    assert_eq!(dump_box(&page, "g", 1), Some(rotated));
    // `display: none`: no box; `visibility: hidden`: a box.
    assert_eq!(dump_box(&page, "g", 2), None);
    assert_eq!(dump_box(&page, "rect", 4), None);
    assert_eq!(dump_box(&page, "g", 3), Some((150.0, 10.0, 10.0, 10.0)));
    // A path of one point has a zero-size box; a negative width is 0.
    assert_eq!(dump_box(&page, "path", 0), Some((30.0, 90.0, 0.0, 0.0)));
    assert_eq!(dump_box(&page, "rect", 6), Some((10.0, 100.0, 0.0, 10.0)));
    // `use`: the box of the copy, translated.
    assert_eq!(dump_box(&page, "use", 0), Some((130.0, 70.0, 20.0, 20.0)));
}

#[test]
fn clip_paths_clip_groups_and_shapes() {
    let site = Site::new("inline-svg-clip");
    let place = |left: u32, content: &str| {
        format!(
            "<svg width=40 height=40 style='display:block;position:absolute;left:{left}px;top:0'>\
             {content}</svg>"
        )
    };
    let html = format!(
        "<!DOCTYPE html><body style='margin:0'>{}{}{}{}{}{}{}",
        place(
            0,
            "<defs><clipPath id=circle><circle cx=20 cy=20 r=10 /></clipPath>\
             <clipPath id=half clipPathUnits=objectBoundingBox><rect width=0.5 height=1 /></clipPath>\
             <clipPath id=moved transform='translate(20 0)'><rect width=20 height=20 /></clipPath>\
             <clipPath id=evenodd><path clip-rule=evenodd d='M0 0H40V40H0Z M10 10H30V30H10Z' /></clipPath>\
             <clipPath id=nested clip-path='url(#circle)'><rect width=20 height=40 /></clipPath>\
             <clipPath id=cycle clip-path='url(#cycle)'><rect width=10 height=10 /></clipPath></defs>\
             <rect width=40 height=40 clip-path='url(#circle)' />"
        ),
        place(
            50,
            "<g clip-path='url(#half)'><rect x=10 y=10 width=20 height=20 /></g>"
        ),
        place(100, "<rect width=40 height=40 clip-path='url(#moved)' />"),
        place(
            150,
            "<rect width=40 height=40 style='clip-path:url(#evenodd)' />"
        ),
        place(200, "<rect width=40 height=40 clip-path='url(#nested)' />"),
        place(250, "<rect width=40 height=40 clip-path='url(#cycle)' />"),
        place(300, "<rect width=40 height=40 clip-path='url(#missing)' />"),
    );
    let (_, p) = render(&site, &html, 400.0, 50.0);
    // A circle of radius 10 in a 40 x 40 box.
    assert_eq!(rgb(&p, 20, 20), BLACK);
    assert_eq!(rgb(&p, 20, 12), BLACK);
    assert_eq!(rgb(&p, 20, 5), WHITE);
    assert_eq!(rgb(&p, 5, 5), WHITE);
    // `objectBoundingBox`: the left half of the group's box (10 to 30).
    assert_eq!(rgb(&p, 50 + 15, 20), BLACK);
    assert_eq!(rgb(&p, 50 + 25, 20), WHITE);
    // The `transform` of the clip path moves it.
    assert_eq!(rgb(&p, 100 + 30, 10), BLACK);
    assert_eq!(rgb(&p, 100 + 10, 10), WHITE);
    assert_eq!(rgb(&p, 100 + 30, 30), WHITE);
    // `clip-rule: evenodd` leaves a hole.
    assert_eq!(rgb(&p, 150 + 5, 20), BLACK);
    assert_eq!(rgb(&p, 150 + 20, 20), WHITE);
    // A clip path with a `clip-path`: the intersection (the left half of
    // the circle).
    assert_eq!(rgb(&p, 200 + 15, 20), BLACK);
    assert_eq!(rgb(&p, 200 + 25, 20), WHITE);
    assert_eq!(rgb(&p, 200 + 15, 3), WHITE);
    // A cycle is cut where it closes: the rectangle still clips.
    assert_eq!(rgb(&p, 250 + 5, 5), BLACK);
    assert_eq!(rgb(&p, 250 + 20, 20), WHITE);
    // A missing reference leaves the element unclipped.
    assert_eq!(rgb(&p, 300 + 30, 30), BLACK);
}

/// A clip path in a different `<svg>` of the document works; one in a
/// `display: none` subtree is not valid (the element is not clipped).
/// Measured in Chromium, case `clip-other-svg`.
#[test]
fn clip_paths_are_found_in_the_whole_document() {
    let site = Site::new("inline-svg-clip-doc");
    let (_, p) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'>\
         <svg width=0 height=0><clipPath id=a><rect width=20 height=20 /></clipPath></svg>\
         <svg width=30 height=30 style='display:none'><clipPath id=b><rect width=20 height=20 /></clipPath></svg>\
         <svg width=40 height=40 style='display:block;position:absolute;left:0;top:0'>\
         <rect width=40 height=40 clip-path='url(#a)' /></svg>\
         <svg width=40 height=40 style='display:block;position:absolute;left:50px;top:0'>\
         <rect width=40 height=40 clip-path='url(#b)' /></svg>",
        200.0,
        50.0,
    );
    assert_eq!(rgb(&p, 10, 10), BLACK);
    assert_eq!(rgb(&p, 30, 30), WHITE);
    assert_eq!(rgb(&p, 50 + 30, 30), BLACK);
}

/// A `use` draws a copy of the element that inherits from the `use`
/// element (SVG 2 §5.6); `x` and `y` translate it.
#[test]
fn use_draws_a_copy_that_inherits_from_the_use_element() {
    let site = Site::new("inline-svg-use");
    let (page, p) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'>\
         <svg width=100 height=40 fill=red><defs><rect id=r width=10 height=10 />\
         <g id=g><use href='#r' x=5 /></g></defs>\
         <use href='#r' x=20 y=5 fill=blue /><use xlink:href='#g' x=50 y=5 />\
         <use href='#missing' /></svg>",
        200.0,
        100.0,
    );
    // The `rect` in `defs` is not drawn in its place.
    assert_eq!(rgb(&p, 5, 5), WHITE);
    assert_eq!(rgb(&p, 25, 10), BLUE);
    // `use` of a group with a `use`: translated twice, red from the `svg`.
    assert_eq!(rgb(&p, 50 + 5 + 5, 10), RED);
    assert_eq!(rgb(&p, 50 + 2, 10), WHITE);
    // The `use` in `defs` has no box; the next has the box of the copy.
    assert_eq!(dump_box(&page, "use", 0), None);
    assert_eq!(dump_box(&page, "use", 1), Some((20.0, 5.0, 10.0, 10.0)));
    // A missing target is an empty box at the origin.
    assert_eq!(dump_box(&page, "use", 3), Some((0.0, 0.0, 0.0, 0.0)));
}

/// Chromium paints the content of an `<svg>` from the pixel-snapped origin
/// of its content box (measured: the Ars Technica logo at y = 297.5 covers
/// whole rows from 298). The size does not change.
#[test]
fn content_starts_at_a_whole_pixel() {
    let site = Site::new("inline-svg-snap");
    let (_, p) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'><div style='height:10.5px'></div>\
         <svg width=20 height=20 style='display:block'><rect width=20 height=20 fill=red /></svg>",
        40.0,
        50.0,
    );
    assert_eq!(rgb(&p, 5, 10), WHITE);
    assert_eq!(rgb(&p, 5, 11), RED);
    assert_eq!(rgb(&p, 5, 30), RED);
    assert_eq!(rgb(&p, 5, 31), WHITE);
}

/// `shape-rendering: crispEdges` and `optimizeSpeed` draw without
/// anti-aliasing (measured in Chromium, case `shape-rendering`).
#[test]
fn shape_rendering_turns_anti_aliasing_off() {
    let site = Site::new("inline-svg-shape-rendering");
    let (_, p) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'>\
         <svg width=40 height=40 style='display:block;position:absolute;left:0;top:0'>\
         <circle cx=20 cy=20 r=15.3 /></svg>\
         <svg width=40 height=40 style='display:block;position:absolute;left:50px;top:0' \
         shape-rendering=crispEdges><circle cx=20 cy=20 r=15.3 /></svg>\
         <svg width=40 height=40 style='display:block;position:absolute;left:100px;top:0'>\
         <circle cx=20 cy=20 r=15.3 shape-rendering=geometricPrecision /></svg>",
        160.0,
        50.0,
    );
    let partial = |x0: u32| {
        (0..40)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let c = rgb(&p, x0 + x, y);
                c != WHITE && c != BLACK
            })
            .count()
    };
    assert!(partial(0) > 20, "anti-aliased: {}", partial(0));
    assert_eq!(partial(50), 0);
    assert!(partial(100) > 20);
    // The covered pixels are those whose centers are inside the circle:
    // 732 of them (the same count as Chromium).
    let covered = (0..40)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .filter(|&(x, y)| rgb(&p, 50 + x, y) == BLACK)
        .count();
    assert_eq!(covered, 732);
}

/// A click on a drawn shape inside a link follows the link, the cursor is
/// the pointer, and `pointer-events` and the geometry decide what is hit
/// (measured in Chromium, `tools/probes/inline-svg.json`, cases `hit-*`).
#[test]
fn shapes_inside_links_are_hit_by_their_geometry() {
    let site = Site::new("inline-svg-hit");
    let (mut page, _) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'><svg width=200 height=100 style='display:block'>\
         <a id=l href='next.html'><rect id=r x=10 y=10 width=30 height=30 />\
         <circle id=c cx=100 cy=25 r=15 fill=none stroke=red stroke-width=6 />\
         <rect id=n x=150 y=10 width=30 height=30 pointer-events=none />\
         <rect id=h x=10 y=60 width=30 height=30 style='visibility:hidden' />\
         <rect id=t x=60 y=60 width=30 height=30 fill=transparent /></a>\
         <a id=x xlink:href='other.html'><rect id=x2 x=110 y=60 width=30 height=30 \
         style='cursor:help' /></a></svg>",
        200.0,
        100.0,
    );
    let hit = |page: &mut Page, x: f32, y: f32| {
        let hit = page.hit_test(x, y).unwrap();
        let id = page
            .document()
            .and_then(|d| d.element(hit.node))
            .and_then(|e| e.attr("id").map(str::to_owned));
        let file = hit
            .link
            .map(|l| l.path().rsplit('/').next().unwrap().to_owned());
        (id, file)
    };
    let some = |s: &str| Some(s.to_owned());
    // Inside a rect: the shape, and the link around it.
    assert_eq!(hit(&mut page, 20.0, 20.0), (some("r"), some("next.html")));
    // Not on any shape: the `<svg>` itself, no link.
    assert_eq!(hit(&mut page, 70.0, 20.0), (None, None));
    // `fill: none`: the stroke is hit, the inside is not.
    assert_eq!(
        hit(&mut page, 100.0 + 15.0, 25.0),
        (some("c"), some("next.html"))
    );
    assert_eq!(hit(&mut page, 100.0, 25.0), (None, None));
    // `pointer-events: none` and `visibility: hidden` are not hit.
    assert_eq!(hit(&mut page, 160.0, 20.0), (None, None));
    assert_eq!(hit(&mut page, 20.0, 70.0), (None, None));
    // A transparent fill is painted.
    assert_eq!(hit(&mut page, 70.0, 70.0), (some("t"), some("next.html")));
    // `xlink:href` links too.
    assert_eq!(
        hit(&mut page, 120.0, 70.0),
        (some("x2"), some("other.html"))
    );
    // The cursor is the link's pointer, or the shape's own.
    page.mouse_move(20.0, 20.0);
    assert_eq!(page.cursor(), swb_engine::Cursor::Pointer);
    page.mouse_move(120.0, 70.0);
    assert_eq!(page.cursor(), swb_engine::Cursor::Help);
    page.mouse_move(70.0, 20.0);
    assert_eq!(page.cursor(), swb_engine::Cursor::Default);
}

/// A shape is hit only inside its clip path (probe cases `r-hitclip` and
/// `r-hitrectclip`).
#[test]
fn clipped_out_parts_of_a_shape_are_not_hit() {
    let site = Site::new("inline-svg-hit-clip");
    let (mut page, _) = render(
        &site,
        "<!DOCTYPE html><body style='margin:0'><svg width=100 height=100 style='display:block'>\
         <defs><clipPath id=c><circle cx=20 cy=20 r=10 /></clipPath>\
         <clipPath id=q><rect width=30 height=30 /></clipPath></defs>\
         <rect id=r width=100 height=100 clip-path='url(#c)' /></svg>\
         <svg width=100 height=100 style='display:block'>\
         <rect id=s width=100 height=100 clip-path='url(#q)' /></svg>",
        100.0,
        200.0,
    );
    let hit = |page: &mut Page, x: f32, y: f32| {
        let hit = page.hit_test(x, y).unwrap();
        page.document()
            .and_then(|d| d.element(hit.node))
            .and_then(|e| e.attr("id").map(str::to_owned))
    };
    let some = |s: &str| Some(s.to_owned());
    assert_eq!(hit(&mut page, 20.0, 20.0), some("r"));
    // Inside the rect, outside the circle (and its bounds).
    assert_eq!(hit(&mut page, 90.0, 90.0), None);
    // Inside the circle's bounds but outside the circle.
    assert_eq!(hit(&mut page, 11.0, 11.0), None);
    assert_eq!(hit(&mut page, 20.0, 120.0), some("s"));
    assert_eq!(hit(&mut page, 90.0, 190.0), None);
}
