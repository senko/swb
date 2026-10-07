//! Scroll containers (ADR 0019): wheel and keyboard scrolling with scroll
//! chaining, programmatic scrolling, scroll into view for focus and
//! fragments, hit testing, text selection and painting of scrolled content,
//! and the overlay scroll indicators. The pages are files in a temporary
//! directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use swb_engine::{Key, Modifiers, MouseButton, Page, Point, Size, Url};
use swb_net::{Fetcher, NetError, NetworkFetcher, Request, Response};

mod common;
use common::{Site, node, rect, tiny_png};

/// Loads `html` into an 800×600 page with the test fonts.
fn open(site: &Site, html: &str) -> (Page, Url) {
    let url = site.page("page.html", html);
    let fetcher = Arc::new(NetworkFetcher::new());
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(url.clone());
    common::finish_loading(&mut page, Duration::from_secs(20));
    (page, url)
}

/// The scroll offset of the element with `id`.
fn offset(page: &mut Page, id: &str) -> Point {
    let n = node(page, id);
    page.element_scroll(n).unwrap().offset
}

fn key(page: &mut Page, key: &Key) -> bool {
    page.key_down(key, Modifiers::NONE)
}

/// The pixels of a rendering of the page (800×600 at scale 1).
fn pixels(page: &mut Page) -> Vec<[u8; 4]> {
    let mut pixmap = swb_engine::Pixmap::new(800, 600).unwrap();
    page.render(&mut pixmap);
    pixmap.data().as_chunks::<4>().0.to_vec()
}

fn pixel(pixels: &[[u8; 4]], x: usize, y: usize) -> [u8; 3] {
    let p = pixels[y * 800 + x];
    [p[0], p[1], p[2]]
}

/// The address of the root fragment's children: it changes when the page
/// is laid out again.
fn layout_identity(page: &mut Page) -> *const () {
    page.update_layout();
    let root = page.fragments().unwrap().root.as_ref().unwrap();
    Arc::as_ptr(&root.children).cast()
}

/// A 200×100 scroll container with six 50 px rows (a scroll range of
/// 200 px), followed by a tall page.
const ROWS: &str = "<!DOCTYPE html><style>body { margin: 0; font: 20px/20px sans-serif }\
    #s { overflow: auto; width: 200px; height: 100px; background: rgb(0, 0, 255) }\
    .row { height: 50px }</style>\
    <div id=s><div id=r0 class=row style='background:rgb(255,0,0)'>alpha</div>\
    <div id=r1 class=row style='background:rgb(0,255,0)'>beta</div>\
    <div id=r2 class=row>gamma</div><div id=r3 class=row>delta</div>\
    <div id=r4 class=row>epsilon</div><div id=r5 class=row>zeta</div></div>\
    <div style='height:2000px'></div>";

#[test]
fn the_wheel_scrolls_the_innermost_container_then_chains_to_the_viewport() {
    let site = Site::new("scroll-wheel");
    let (mut page, _) = open(&site, ROWS);
    assert!(page.wheel(50.0, 50.0, 0.0, 30.0));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 30.0));
    assert_eq!(page.scroll_position(), Point::default());
    // The rest of a delta does not go to the next scroller.
    page.wheel(50.0, 50.0, 0.0, 500.0);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 200.0));
    assert_eq!(page.scroll_position(), Point::default());
    // At the end of its range, the viewport scrolls.
    page.wheel(50.0, 50.0, 0.0, 40.0);
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 200.0));
    // Back up: the container again (the pointer is still over it).
    page.wheel(50.0, 50.0, 0.0, -50.0);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 150.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
    // Outside the container: the viewport. It has no horizontal range.
    page.wheel(500.0, 300.0, 0.0, 10.0);
    assert_eq!(page.scroll_position(), Point::new(0.0, 50.0));
    assert!(!page.wheel(500.0, 300.0, 10.0, 0.0));
    // Deltas that are not finite do nothing.
    assert!(!page.wheel(50.0, 50.0, f32::NAN, f32::INFINITY));
}

#[test]
fn overflow_hidden_scrolls_only_programmatically() {
    let site = Site::new("scroll-hidden");
    let html = ROWS.replace("overflow: auto", "overflow: hidden");
    let (mut page, _) = open(&site, &html);
    page.wheel(50.0, 50.0, 0.0, 30.0);
    assert_eq!(offset(&mut page, "s"), Point::default());
    assert_eq!(page.scroll_position(), Point::new(0.0, 30.0));
    let s = node(&page, "s");
    assert_eq!(
        page.scroll_element_to(s, Point::new(0.0, 70.0)),
        Some(Point::new(0.0, 70.0))
    );
    assert_eq!(rect(&mut page, "r1").y, 50.0 - 70.0);
}

#[test]
fn element_scroll_reports_offsets_and_sizes() {
    let site = Site::new("scroll-info");
    let (mut page, _) = open(&site, ROWS);
    let s = node(&page, "s");
    // Clamped, also for values that are not finite.
    assert_eq!(
        page.scroll_element_to(s, Point::new(f32::NAN, f32::INFINITY)),
        Some(Point::new(0.0, 200.0))
    );
    let info = page.element_scroll(s).unwrap();
    assert!(info.scrollable);
    assert_eq!(info.scroll_size, Size::new(200.0, 300.0));
    assert_eq!(info.client_size, Size::new(200.0, 100.0));
    // The root element is the viewport.
    let root = page.document().unwrap().document_element().unwrap();
    page.scroll_element_to(root, Point::new(0.0, 100.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 100.0));
    let info = page.element_scroll(root).unwrap();
    assert_eq!(info.client_size, Size::new(800.0, 600.0));
    assert_eq!(info.offset, Point::new(0.0, 100.0));
    // Other elements do not scroll.
    let row = node(&page, "r0");
    assert_eq!(
        page.scroll_element_to(row, Point::new(5.0, 5.0)),
        Some(Point::default())
    );
    let info = page.element_scroll(row).unwrap();
    assert!(!info.scrollable);
    assert_eq!(info.client_size, Size::new(200.0, 50.0));
}

#[test]
fn keys_scroll_the_container_of_the_last_click() {
    let site = Site::new("scroll-keys-container");
    let (mut page, _) = open(&site, ROWS);
    // Without a click or focus, the viewport scrolls.
    assert!(key(&mut page, &Key::ArrowDown));
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
    page.scroll_to(Point::default());
    page.mouse_down(50.0, 10.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(50.0, 10.0, MouseButton::Primary);
    key(&mut page, &Key::ArrowDown);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 40.0));
    // A page is 87.5 % of the scrollport, truncated (Chromium 148: 127).
    key(&mut page, &Key::PageDown);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 127.0));
    key(&mut page, &Key::End);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 200.0));
    assert_eq!(page.scroll_position(), Point::default());
    // At the end, the key scrolls the viewport.
    key(&mut page, &Key::ArrowDown);
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
    key(&mut page, &Key::Home);
    assert_eq!(offset(&mut page, "s"), Point::default());
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
}

#[test]
fn scrolled_content_is_hit_tested_and_painted_at_its_offset() {
    let site = Site::new("scroll-paint");
    let (mut page, _) = open(&site, ROWS);
    let before = pixels(&mut page);
    assert_eq!(pixel(&before, 150, 10), [255, 0, 0]);
    let layout = layout_identity(&mut page);
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 50.0));
    // Only the display list is built again.
    assert_eq!(layout_identity(&mut page), layout);
    let after = pixels(&mut page);
    assert_eq!(pixel(&after, 150, 10), [0, 255, 0]);
    // The rows end at 300 - 50 px: the container's own background stays.
    page.scroll_element_to(s, Point::new(0.0, 200.0));
    assert_eq!(rect(&mut page, "r5").y, 50.0);
    // Content outside the scrollport is clipped.
    let end = pixels(&mut page);
    assert_ne!(pixel(&end, 150, 110), [0, 255, 0]);
    let r1 = node(&page, "r1");
    page.scroll_element_to(s, Point::new(0.0, 50.0));
    let hit = page.hit_test(150.0, 10.0).unwrap();
    let element = page.document().unwrap().parent_element(hit.node);
    assert!(hit.node == r1 || element == Some(r1), "{hit:?}");
    assert_eq!(
        rect(&mut page, "r1"),
        swb_engine::Rect::new(0.0, 0.0, 200.0, 50.0)
    );
}

#[test]
fn absolutely_positioned_boxes_with_an_outer_containing_block_do_not_scroll() {
    let site = Site::new("scroll-abspos");
    // The box sits at its static position below the rows (y 300); the
    // container is not positioned, so the box is neither clipped nor
    // scrolled. A wheel over it scrolls the viewport.
    let html = ROWS.replace(
        "<div id=r5 class=row>zeta</div>",
        "<div id=r5 class=row>zeta</div><div id=abs style='position:absolute;width:20px;\
         height:20px;background:rgb(255,0,255)'></div>",
    );
    let (mut page, _) = open(&site, &html);
    let s = node(&page, "s");
    assert_eq!(rect(&mut page, "abs").y, 300.0);
    page.scroll_element_to(s, Point::new(0.0, 100.0));
    assert_eq!(rect(&mut page, "abs").y, 300.0);
    let p = pixels(&mut page);
    assert_eq!(pixel(&p, 5, 305), [255, 0, 255]);
    assert!(page.wheel(5.0, 305.0, 0.0, 10.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 10.0));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 100.0));
}

#[test]
fn focus_scrolls_every_container_and_the_viewport() {
    let site = Site::new("scroll-focus");
    // A scroll container far down, with a link at the end of its content.
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px/20px sans-serif'>\
         <div style='height:1000px'></div>\
         <div id=s style='overflow:auto;height:100px;width:300px'>\
         <div style='height:500px'></div><a id=a href=x>link</a></div>\
         <div style='height:2000px'></div>",
    );
    key(&mut page, &Key::Tab);
    let a = rect(&mut page, "a");
    let s = rect(&mut page, "s");
    let scroll = page.scroll_position();
    assert!(
        a.y >= s.y && a.y + a.height <= s.y + s.height,
        "{a:?} in {s:?}"
    );
    assert!(
        a.y >= scroll.y && a.y + a.height <= scroll.y + 600.0,
        "{a:?} {scroll:?}"
    );
    assert!(offset(&mut page, "s").y > 0.0);
}

#[test]
fn fragments_scroll_their_target_to_the_top_of_each_container() {
    let site = Site::new("scroll-fragment");
    let html = ROWS.replace("<div id=s>", "<div style='height:1000px'></div><div id=s>");
    let (mut page, url) = open(&site, &html);
    let mut link = url.clone();
    link.set_fragment(Some("r3"));
    assert!(page.follow_link(link));
    // r3 starts at 150 in the container; the container at 1000.
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 150.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 1000.0));
    assert_eq!(rect(&mut page, "r3").y, 1000.0);
}

#[test]
fn a_url_with_a_fragment_scrolls_the_container_on_load() {
    let site = Site::new("scroll-fragment-load");
    let url = site.page("page.html", ROWS);
    let mut with_fragment = url.clone();
    with_fragment.set_fragment(Some("r2"));
    let fetcher = Arc::new(NetworkFetcher::new());
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(with_fragment);
    common::finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 100.0));
}

#[test]
fn offsets_survive_a_relayout_and_are_clamped_to_the_new_range() {
    let site = Site::new("scroll-relayout");
    let html = ROWS.replace("height: 100px", "height: 20vh");
    let (mut page, _) = open(&site, &html);
    let s = node(&page, "s");
    // 20vh is 120 px: a range of 180 px.
    page.scroll_element_to(s, Point::new(0.0, 150.0));
    page.set_viewport(Size::new(700.0, 600.0), 1.0);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 150.0));
    // 20vh is 300 px: no range.
    page.set_viewport(Size::new(700.0, 1500.0), 1.0);
    assert_eq!(offset(&mut page, "s"), Point::default());
}

#[test]
fn a_body_scroll_container_scrolls_when_the_root_does_not() {
    // The M0 backlog case: `html, body { height: 100% }`, the root's
    // `overflow: hidden` goes to the viewport, and the body scrolls.
    let site = Site::new("scroll-body");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><style>html, body { height: 100%; margin: 0 } html { overflow: hidden }\
         body { overflow: auto }</style><body id=body><div style='height:3000px'></div>",
    );
    assert!(page.wheel(100.0, 100.0, 0.0, 120.0));
    assert_eq!(offset(&mut page, "body"), Point::new(0.0, 120.0));
    assert_eq!(page.scroll_position(), Point::default());
    key(&mut page, &Key::End);
    // Without a click, keys scroll the viewport, which cannot scroll here.
    assert_eq!(offset(&mut page, "body"), Point::new(0.0, 120.0));
    page.mouse_down(100.0, 100.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(100.0, 100.0, MouseButton::Primary);
    key(&mut page, &Key::End);
    assert_eq!(offset(&mut page, "body"), Point::new(0.0, 2400.0));
}

#[test]
fn text_is_selected_at_its_scrolled_position() {
    let site = Site::new("scroll-select");
    let (mut page, _) = open(&site, ROWS);
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 50.0));
    page.mouse_down(10.0, 10.0, MouseButton::Primary, Modifiers::NONE, 2);
    page.mouse_up(10.0, 10.0, MouseButton::Primary);
    assert_eq!(page.selected_text(), "beta");
    // At 90 px, the text of r4 ("epsilon") starts 9 px below the
    // scrollport and is hidden. Below the container it would be the
    // nearest text; the nearest visible text is "delta".
    page.scroll_element_to(s, Point::new(0.0, 90.0));
    page.mouse_down(10.0, 105.0, MouseButton::Primary, Modifiers::NONE, 2);
    page.mouse_up(10.0, 105.0, MouseButton::Primary);
    assert_eq!(page.selected_text(), "delta");
}

#[test]
fn keys_scroll_the_container_of_the_focused_element() {
    let site = Site::new("scroll-keys-focus");
    let html = ROWS.replace(
        "<div id=r0 class=row style='background:rgb(255,0,0)'>alpha</div>",
        "<div id=r0 class=row style='background:rgb(255,0,0)'><a href=x>alpha</a></div>",
    );
    let (mut page, _) = open(&site, &html);
    key(&mut page, &Key::Tab);
    assert!(page.focused_element().is_some());
    key(&mut page, &Key::ArrowDown);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 40.0));
    assert_eq!(page.scroll_position(), Point::default());
}

#[test]
fn the_wheel_does_not_move_a_hidden_axis() {
    // Wikipedia's table of contents: `overflow: hidden auto` with wide
    // content; a diagonal touchpad gesture scrolls it only vertically.
    let site = Site::new("scroll-wheel-hidden-axis");
    let html = ROWS
        .replace("overflow: auto", "overflow: hidden auto")
        .replace(
            ".row { height: 50px }",
            ".row { height: 50px; width: 500px }",
        );
    let (mut page, _) = open(&site, &html);
    assert!(page.wheel(50.0, 50.0, 30.0, 30.0));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 30.0));
    // Horizontally only: the container cannot move; the page cannot either.
    assert!(!page.wheel(50.0, 50.0, 30.0, 0.0));
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 30.0));
}

#[test]
fn a_fragment_target_without_a_box_scrolls_to_the_text_after_it() {
    let site = Site::new("scroll-fragment-no-box");
    let (mut page, url) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px/20px sans-serif'>\
         <div id=s style='overflow:auto;height:100px'><div style='height:300px'></div>\
         <span id=t style='display:none'></span>target text</div>\
         <div style='height:2000px'></div>",
    );
    let mut link = url.clone();
    link.set_fragment(Some("t"));
    page.follow_link(link);
    // The text follows the 300 px block. Its content area (ascent and
    // descent of a 20 px font) is taller than its 20 px line, so the range
    // is 221 px (Chromium 148: also 221).
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 221.0));
    // The text is then about 78 px down in the container; `block: "start"`
    // puts that part at the top of the viewport.
    let y = page.scroll_position().y;
    assert!((77.0..79.0).contains(&y), "{y}");
}

#[test]
fn quirks_mode_body_scroll_containers_scroll_as_elements() {
    // In quirks mode the body is `document.scrollingElement` only if it
    // is not a scroll container; the root element does not scroll. (A
    // fixed height: swb does not have the quirks-mode percentage height
    // rules.)
    let site = Site::new("scroll-quirks-body");
    let (mut page, _) = open(
        &site,
        "<html><style>html { overflow: hidden } body { margin: 0; height: 600px; \
         overflow: auto }</style><body id=body><div style='height:3000px'></div>",
    );
    let body = node(&page, "body");
    assert_eq!(
        page.scroll_element_to(body, Point::new(0.0, 100.0)),
        Some(Point::new(0.0, 100.0))
    );
    assert_eq!(page.scroll_position(), Point::default());
    let info = page.element_scroll(body).unwrap();
    assert_eq!(info.client_size, Size::new(800.0, 600.0));
    assert_eq!(info.scroll_size, Size::new(800.0, 3000.0));
    let root = page.document().unwrap().document_element().unwrap();
    assert_eq!(
        page.scroll_element_to(root, Point::new(0.0, 50.0)),
        Some(Point::default())
    );
    assert!(!page.element_scroll(root).unwrap().scrollable);
}

#[test]
fn quirks_mode_body_without_overflow_is_the_viewport() {
    let site = Site::new("scroll-quirks-viewport");
    let (mut page, _) = open(
        &site,
        "<html><body id=body style='margin:0'><div style='height:3000px'></div>",
    );
    let body = node(&page, "body");
    page.scroll_element_to(body, Point::new(0.0, 100.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 100.0));
    assert!(page.element_scroll(body).unwrap().scrollable);
}

#[test]
fn indicators_are_drawn_only_when_enabled() {
    let site = Site::new("scroll-indicators");
    let html = ROWS.replace("rgb(0, 0, 255)", "rgb(255, 255, 255)");
    let (mut page, _) = open(&site, &html);
    // The thumbs: 5 px wide, 2 px from the right edge.
    let plain = pixels(&mut page);
    assert_eq!(pixel(&plain, 195, 120), [255, 255, 255]);
    page.set_scroll_indicators(true);
    let shown = pixels(&mut page);
    let dark = |p: [u8; 3]| p.iter().all(|&c| c < 200);
    // The container's thumb: 32 px (96 × 100 / 300) at the top, over the
    // red first row.
    assert!(!dark(pixel(&plain, 195, 10)));
    assert!(dark(pixel(&shown, 195, 10)), "{:?}", pixel(&shown, 195, 10));
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 100.0));
    let scrolled = pixels(&mut page);
    // The thumb moved down: 100 of 200 px.
    assert!(
        dark(pixel(&scrolled, 195, 50)),
        "{:?}",
        pixel(&scrolled, 195, 50)
    );
    assert!(!dark(pixel(&scrolled, 195, 5)));
    // The viewport's thumb at the right edge of the window.
    assert!(dark(pixel(&shown, 795, 10)), "{:?}", pixel(&shown, 795, 10));
    assert!(!dark(pixel(&plain, 795, 10)));
}

#[test]
fn nested_containers_scroll_a_target_into_view() {
    let site = Site::new("scroll-nested");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px/20px sans-serif'>\
         <div id=outer style='overflow:auto;height:150px;width:300px'>\
         <div style='height:400px'></div>\
         <div id=inner style='overflow:auto;height:100px;width:250px'>\
         <div style='height:600px'></div><a id=a href=x>deep</a></div>\
         <div style='height:400px'></div></div>",
    );
    let a = node(&page, "a");
    page.scroll_into_view(a);
    let ra = rect(&mut page, "a");
    let inner = rect(&mut page, "inner");
    let outer = rect(&mut page, "outer");
    assert!(ra.y >= inner.y && ra.y + ra.height <= inner.y + inner.height);
    assert!(inner.y + inner.height > outer.y && inner.y < outer.y + outer.height);
    assert!(ra.y >= outer.y && ra.y + ra.height <= outer.y + outer.height);
    assert!(offset(&mut page, "outer").y > 0.0 && offset(&mut page, "inner").y > 0.0);
    // The link is inside the area that both containers leave visible.
    let clip = page.scroll_clip(a).unwrap().unwrap();
    assert_eq!(clip.intersection(&ra), Some(ra));
    let outer_node = node(&page, "outer");
    assert_eq!(page.scroll_clip(outer_node), None);
}

#[test]
fn huge_content_gives_finite_scroll_ranges() {
    let site = Site::new("scroll-huge");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><div id=s style='overflow:auto;height:100px'>\
         <div style='height:1e39px;width:1e39px;margin:1e39px;padding:1e39px'></div></div>",
    );
    let s = node(&page, "s");
    let to = page
        .scroll_element_to(s, Point::new(f32::MAX, f32::MAX))
        .unwrap();
    assert!(to.x.is_finite() && to.y.is_finite() && to.y > 0.0);
    page.wheel(10.0, 10.0, 0.0, -1e30);
    assert_eq!(offset(&mut page, "s").y, 0.0);
    let info = page.element_scroll(s).unwrap();
    assert!(info.scroll_size.width.is_finite() && info.scroll_size.height.is_finite());
    let _ = pixels(&mut page);
}

#[test]
fn offsets_are_dropped_with_the_document() {
    let site = Site::new("scroll-reload");
    let (mut page, url) = open(&site, ROWS);
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 100.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(offset(&mut page, "s"), Point::default());
}

/// A fetcher that holds back requests whose query is `block` until the
/// test releases them.
struct Blocking {
    inner: NetworkFetcher,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Fetcher for Blocking {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if request.url.query() == Some("block") {
            let _ = self.release.lock().unwrap().recv();
        }
        self.inner.fetch(request)
    }
}

#[test]
fn a_user_scroll_while_loading_cancels_the_fragment_scroll() {
    let site = Site::new("scroll-cancel-fragment");
    site.file("img.png", &tiny_png());
    let html = ROWS.replace(
        "<div id=s>",
        "<img src='img.png?block' style='display:block;width:10px'><div id=s>",
    );
    let mut url = site.page("page.html", &html);
    url.set_fragment(Some("r3"));
    let (release, receiver) = mpsc::channel();
    let fetcher = Arc::new(Blocking {
        inner: NetworkFetcher::new(),
        release: Mutex::new(receiver),
    });
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(url);
    let deadline = Instant::now() + Duration::from_secs(20);
    while page.document().is_none() {
        assert!(Instant::now() < deadline, "the document did not arrive");
        page.process_network();
        std::thread::sleep(Duration::from_millis(5));
    }
    page.update_layout();
    assert!(page.is_loading());
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 150.0));
    // The user scrolls the container back up before the image arrives.
    page.wheel(50.0, 50.0, 0.0, -1000.0);
    release.send(()).unwrap();
    common::finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(offset(&mut page, "s"), Point::default());
}

#[test]
fn keys_after_a_resize_scroll_the_container() {
    // The resize drops the styles; the key lays the page out again first.
    let site = Site::new("scroll-keys-resize");
    let (mut page, _) = open(&site, ROWS);
    page.mouse_down(50.0, 10.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(50.0, 10.0, MouseButton::Primary);
    page.set_viewport(Size::new(700.0, 600.0), 1.0);
    key(&mut page, &Key::ArrowDown);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 40.0));
    assert_eq!(page.scroll_position(), Point::default());
}

#[test]
fn the_wheel_updates_the_hovered_link_at_its_position() {
    let site = Site::new("scroll-wheel-hover");
    let html = ROWS.replace(
        "<div id=r1 class=row style='background:rgb(0,255,0)'>beta</div>",
        "<div id=r1 class=row style='background:rgb(0,255,0)'><a href=beta>beta</a></div>",
    );
    let (mut page, _) = open(&site, &html);
    assert!(page.hovered_link().is_none());
    // No mouse movement before: the wheel event gives the position.
    page.wheel(10.0, 10.0, 0.0, 50.0);
    assert!(page.hovered_link().is_some());
}

#[test]
fn text_that_ends_at_the_scrollport_edge_is_hidden() {
    // The container starts at 300 px, with a padding that puts the bottom
    // of the text of r1 on a whole pixel (scroll offsets are whole pixels).
    let page_with = |site: &Site, padding: f32| {
        let html = ROWS.replace(
            "<div id=s>",
            &format!("<div style='height:300px'></div><div id=s style='padding-top:{padding}px'>"),
        );
        open(site, &html).0
    };
    // The bottom of the text of r1 in the container's content.
    let text_bottom = |page: &mut Page| {
        let r1 = node(page, "r1");
        let text = page.document().unwrap().children(r1).next().unwrap();
        let mut bottom = None;
        page.fragments().unwrap().walk(|f, origin| {
            if let swb_layout::FragmentRef::Text(t) = f
                && t.node == text
            {
                bottom = Some(t.rect.translate(origin).bottom() - 300.0);
            }
        });
        bottom.unwrap()
    };
    let site = Site::new("scroll-clip-edge");
    let bottom = text_bottom(&mut page_with(&site, 0.0));
    let mut page = page_with(&site, bottom.ceil() - bottom);
    let bottom = text_bottom(&mut page);
    assert_eq!(bottom, bottom.round());
    // Scrolled so that the text ends exactly at the top of the scrollport.
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, bottom));
    page.mouse_down(10.0, 295.0, MouseButton::Primary, Modifiers::NONE, 2);
    page.mouse_up(10.0, 295.0, MouseButton::Primary);
    assert_eq!(page.selected_text(), "gamma");
}

#[test]
fn scroll_offsets_are_whole_pixels() {
    // Chromium 148 rounds scroll offsets to whole CSS px (halves up), for
    // elements and the viewport.
    let site = Site::new("scroll-whole-pixels");
    let (mut page, _) = open(&site, ROWS);
    let s = node(&page, "s");
    assert_eq!(
        page.scroll_element_to(s, Point::new(0.0, 10.4)),
        Some(Point::new(0.0, 10.0))
    );
    assert_eq!(
        page.scroll_element_to(s, Point::new(0.0, 10.5)),
        Some(Point::new(0.0, 11.0))
    );
    page.scroll_to(Point::new(0.0, 258.5));
    assert_eq!(page.scroll_position(), Point::new(0.0, 259.0));
}

#[test]
fn a_root_with_overflow_hidden_scrolls_only_programmatically() {
    // Chromium 148: `documentElement.scrollTop = 500` gives 500, and a
    // fragment navigation scrolls; the wheel and the keys do not.
    let site = Site::new("scroll-hidden-root");
    let (mut page, url) = open(
        &site,
        "<!DOCTYPE html><html style='overflow:hidden'><body style='margin:0'>\
         <div style='height:3000px'></div><div id=t>target</div>",
    );
    assert!(!page.wheel(100.0, 100.0, 0.0, 100.0));
    assert!(!page.wheel_page(0.0, 100.0));
    key(&mut page, &Key::PageDown);
    assert_eq!(page.scroll_position(), Point::default());
    let root = page.document().unwrap().document_element().unwrap();
    assert_eq!(
        page.scroll_element_to(root, Point::new(0.0, 500.0)),
        Some(Point::new(0.0, 500.0))
    );
    let mut link = url.clone();
    link.set_fragment(Some("t"));
    page.follow_link(link);
    let y = page.scroll_position().y;
    assert!(y > 2400.0 && y == y.round(), "{y}");
}

#[test]
fn a_hidden_viewport_axis_does_not_move_with_the_wheel_and_has_no_indicator() {
    // `overflow-x: hidden` on the body applies to the viewport: a diagonal
    // wheel scrolls only vertically.
    let site = Site::new("scroll-hidden-axis-viewport");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;overflow-x:hidden'>\
         <div style='width:3000px;height:3000px'></div>",
    );
    assert!(page.wheel(100.0, 100.0, 50.0, 50.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 50.0));
    page.set_scroll_indicators(true);
    let p = pixels(&mut page);
    let dark = |p: [u8; 3]| p.iter().all(|&c| c < 200);
    // The vertical thumb, but no horizontal one.
    assert!(dark(pixel(&p, 795, 20)));
    assert!(!dark(pixel(&p, 20, 595)));
}

#[test]
fn the_end_of_a_fractional_range_is_a_whole_pixel_and_survives_a_relayout() {
    // Chromium 148: content of 200.4 px in a 100 px scrollport scrolls to
    // 100.
    let site = Site::new("scroll-fractional-range");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'><div id=s style='overflow:auto;width:100px;\
         height:100px'><div style='height:200.4px'></div></div>",
    );
    let s = node(&page, "s");
    assert_eq!(
        page.scroll_element_to(s, Point::new(0.0, 9999.0)),
        Some(Point::new(0.0, 100.0))
    );
    page.set_viewport(Size::new(700.0, 600.0), 1.0);
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 100.0));
    // At the end, the wheel goes to the page.
    assert!(!page.wheel(50.0, 50.0, 0.0, 10.0));
}

#[test]
fn scrolled_boxes_in_a_positioned_inline_box_are_clipped_by_the_scroll_container() {
    // The containing block of the box is the positioned inline box, which
    // is inside the scroll container: the box moves with it and is
    // clipped by it (as in Chromium).
    let site = Site::new("scroll-inline-cb-clip");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:16px/20px monospace'>\
         <div style='height:100px'></div>\
         <div id=s style='overflow:auto;width:200px;height:100px'>\
         <span style='position:relative'>text <span style='display:block'>\
         <span style='display:block'>block</span><div id=badge style='position:absolute;\
         width:30px;height:30px;background:rgb(255,0,255)'></div></span></span>\
         <div style='height:300px'></div></div>",
    );
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 50.0));
    // The badge starts at 40 px in the content: 90 px in the document.
    assert_eq!(rect(&mut page, "badge").y, 90.0);
    let p = pixels(&mut page);
    assert_eq!(pixel(&p, 5, 110), [255, 0, 255]);
    assert_ne!(pixel(&p, 5, 95), [255, 0, 255]);
}

#[test]
fn a_full_page_screenshot_keeps_the_scroll_offsets() {
    // The full-page screenshot keeps the layout of the viewport (as in
    // Chromium), where the `vh`-sized container has a scroll range.
    let site = Site::new("scroll-full-page-shot");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'><div id=s style='overflow:auto;\
         max-height:50vh;width:300px'><div style='height:1000px'></div></div>\
         <div style='height:2000px'></div>",
    );
    let s = node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 500.0));
    page.scroll_to(Point::new(0.0, 100.0));
    page.screenshot(true).unwrap();
    assert_eq!(offset(&mut page, "s"), Point::new(0.0, 500.0));
    assert_eq!(page.scroll_position(), Point::new(0.0, 100.0));
}
