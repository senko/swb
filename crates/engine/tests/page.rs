//! Page behavior: navigation, session history, fragments, scrolling, hit
//! testing, cancellation of requests, and the display list of real pages.
//! The pages are files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use swb_engine::{LoadState, Page, Point, Size, Url};
use swb_net::{Destination, Fetcher, NetError, NetworkFetcher, Request, Response};
use swb_paint::{DisplayItem, DisplayList};
use swb_style::Rgba;

mod common;
use common::{Site, tiny_png};

/// Fetches local files. A URL whose query starts with `slow` takes 200 ms;
/// a URL with the query `block` waits until [`TestFetcher::release`].
/// Records every fetched URL.
struct TestFetcher {
    inner: NetworkFetcher,
    release: Mutex<mpsc::Receiver<()>>,
    releaser: Mutex<mpsc::Sender<()>>,
    fetched: Mutex<Vec<Url>>,
}

impl TestFetcher {
    fn new() -> Arc<TestFetcher> {
        let (releaser, release) = mpsc::channel();
        Arc::new(TestFetcher {
            inner: NetworkFetcher::new(),
            release: Mutex::new(release),
            releaser: Mutex::new(releaser),
            fetched: Mutex::new(Vec::new()),
        })
    }

    fn release(&self) {
        self.releaser.lock().unwrap().send(()).unwrap();
    }

    fn fetch_count(&self, url: &Url) -> usize {
        self.fetched
            .lock()
            .unwrap()
            .iter()
            .filter(|u| *u == url)
            .count()
    }
}

impl Fetcher for TestFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.fetched.lock().unwrap().push(request.url.clone());
        let query = request.url.query().unwrap_or("");
        if query.starts_with("slow") {
            std::thread::sleep(Duration::from_millis(200));
        } else if query == "block" {
            let _ = self.release.lock().unwrap().recv();
        }
        self.inner.fetch(request)
    }
}

fn new_page(fetcher: Arc<dyn Fetcher>) -> Page {
    common::new_page(fetcher, 2, Size::new(800.0, 600.0))
}

fn local_page() -> Page {
    new_page(Arc::new(NetworkFetcher::new()))
}

fn load(page: &mut Page, url: Url) {
    page.navigate(url);
    finish(page);
}

fn finish(page: &mut Page) {
    common::finish_loading(page, Duration::from_secs(20));
}

fn with_query(url: &Url, query: &str) -> Url {
    let mut u = url.clone();
    u.set_query(Some(query));
    u
}

fn with_fragment(url: &Url, fragment: &str) -> Url {
    let mut u = url.clone();
    u.set_fragment(Some(fragment));
    u
}

/// The last path segment of the link at (x, y), if any.
fn link_at(page: &mut Page, x: f32, y: f32) -> Option<String> {
    let link = page.hit_test(x, y)?.link?;
    link.path_segments()?.next_back().map(str::to_owned)
}

const LONG_PAGE: &str = "<!DOCTYPE html><body style='margin:0'>\
    <div style='height:3000px'>top</div><h1 id='sec' style='margin:0'>sec</h1>\
    <div style='height:3000px'></div>";

// ----- Scrolling -----

#[test]
fn scroll_position_survives_layout_invalidation() {
    let site = Site::new("scroll");
    let mut page = local_page();
    load(&mut page, site.page("long.html", LONG_PAGE));
    page.scroll_to(Point::new(0.0, 2000.0));
    // A resize (or an image that arrives) drops the layout; scrolling
    // before the next frame must not lose the position.
    page.set_viewport(Size::new(800.0, 601.0), 1.0);
    page.scroll_by(0.0, 10.0);
    assert_eq!(page.scroll_position(), Point::new(0.0, 2010.0));
    page.scroll_by(0.0, 1e9);
    let max = page.content_size().height - 601.0;
    assert_eq!(page.scroll_position().y, max);
}

// ----- Fragments and history -----

#[test]
fn fragment_scroll_follows_images_that_load_later() {
    let site = Site::new("fragment-late-image");
    site.file("img.png", &tiny_png());
    let url = site.page(
        "p.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <img src='img.png?block' style='display:block;width:500px'>\
         <h1 id='sec' style='margin:0'>sec</h1><div style='height:3000px'></div>",
    );
    let fetcher = TestFetcher::new();
    let mut page = new_page(fetcher.clone());
    page.navigate(with_fragment(&url, "sec"));
    // Lay out once while the image is still loading: it has no size yet.
    let deadline = Instant::now() + Duration::from_secs(20);
    while page.document().is_none() {
        assert!(Instant::now() < deadline, "document did not load");
        page.process_network();
        std::thread::sleep(Duration::from_millis(5));
    }
    page.update_layout();
    assert_eq!(page.scroll_position().y, 0.0);
    // The image arrives (500 px wide, so taller than 0); the target moves
    // down and the scroll position follows it.
    fetcher.release();
    finish(&mut page);
    let sec = common::node(&page, "sec");
    let target_y = page.fragments().unwrap().border_boxes(sec)[0].y;
    assert!(target_y > 0.0);
    assert_eq!(page.scroll_position().y, target_y);
    // After the load, user scrolling is not undone by later layouts.
    page.scroll_to(Point::new(0.0, 100.0));
    page.set_viewport(Size::new(800.0, 601.0), 1.0);
    page.update_layout();
    assert_eq!(page.scroll_position().y, 100.0);
}

#[test]
fn loading_a_url_with_a_fragment_scrolls_to_it() {
    let site = Site::new("load-fragment");
    let url = site.page("long.html", LONG_PAGE);
    let mut page = local_page();
    load(&mut page, with_fragment(&url, "sec"));
    assert_eq!(page.scroll_position().y, 3000.0);
}

#[test]
fn fragment_navigation_and_history_do_not_reload() {
    let site = Site::new("fragment-history");
    let url = site.page("long.html", LONG_PAGE);
    let mut page = local_page();
    load(&mut page, url.clone());
    page.scroll_to(Point::new(0.0, 100.0));
    assert!(page.follow_link(with_fragment(&url, "sec")));
    assert!(!page.is_loading());
    assert_eq!(page.scroll_position().y, 3000.0);

    // Back restores the position that the entry had.
    assert!(page.go_back());
    assert!(!page.is_loading(), "a fragment change must not reload");
    assert_eq!(page.scroll_position().y, 100.0);
    assert_eq!(page.url(), Some(&url));

    assert!(page.go_forward());
    assert!(!page.is_loading());
    assert_eq!(page.scroll_position().y, 3000.0);
    assert!(!page.can_go_forward());
}

#[test]
fn empty_fragment_and_top_scroll_to_the_top() {
    let site = Site::new("fragment-top");
    let url = site.page("long.html", LONG_PAGE);
    let mut page = local_page();
    load(&mut page, url.clone());
    for fragment in ["", "top", "TOP"] {
        page.scroll_to(Point::new(0.0, 500.0));
        page.follow_link(with_fragment(&url, fragment));
        assert_eq!(page.scroll_position().y, 0.0, "#{fragment}");
    }
}

#[test]
fn empty_anchor_scrolls_to_the_following_content() {
    let site = Site::new("fragment-anchor");
    let url = site.page(
        "anchor.html",
        "<!DOCTYPE html><body style='margin:0'><div style='height:3000px'></div>\
         <a name='sec'></a><h2 style='margin:0'>sec</h2><div style='height:3000px'></div>",
    );
    let mut page = local_page();
    load(&mut page, url.clone());
    page.follow_link(with_fragment(&url, "sec"));
    assert_eq!(page.scroll_position().y, 3000.0);
}

#[test]
fn percent_encoded_fragments_find_ids() {
    let site = Site::new("fragment-encoded");
    let url = site.page(
        "encoded.html",
        "<!DOCTYPE html><body style='margin:0'><div style='height:3000px'></div>\
         <p id='č' style='margin:0'>x</p><div style='height:3000px'></div>",
    );
    let mut page = local_page();
    load(&mut page, url.clone());
    page.follow_link(with_fragment(&url, "č"));
    assert_eq!(page.scroll_position().y, 3000.0);
}

#[test]
fn back_restores_the_scroll_position_of_the_previous_document() {
    let site = Site::new("back-scroll");
    let a = site.page("a.html", LONG_PAGE);
    let b = site.page("b.html", "<!DOCTYPE html><title>B</title>b");
    let mut page = local_page();
    load(&mut page, a.clone());
    page.scroll_to(Point::new(0.0, 1234.0));
    load(&mut page, b);
    assert_eq!(page.scroll_position().y, 0.0);
    assert!(page.go_back());
    finish(&mut page);
    assert_eq!(page.url(), Some(&a));
    assert_eq!(page.scroll_position().y, 1234.0);
}

#[test]
fn reload_keeps_the_scroll_position() {
    let site = Site::new("reload");
    let mut page = local_page();
    load(&mut page, site.page("long.html", LONG_PAGE));
    page.scroll_to(Point::new(0.0, 777.0));
    page.reload();
    finish(&mut page);
    assert_eq!(page.scroll_position().y, 777.0);
    assert!(!page.can_go_back(), "a reload does not add an entry");
}

#[test]
fn navigating_to_the_same_url_replaces_the_entry() {
    let site = Site::new("same-url");
    let url = site.page("a.html", "a");
    let mut page = local_page();
    load(&mut page, url.clone());
    load(&mut page, url);
    assert!(!page.can_go_back());
}

// ----- Pending navigations and cancellation -----

#[test]
fn uncommitted_navigation_leaves_no_entry_and_late_responses_are_ignored() {
    let site = Site::new("uncommitted");
    let a = site.page("a.html", "<!DOCTYPE html><title>A</title>a");
    let b = with_query(
        &site.page("b.html", "<!DOCTYPE html><title>B</title>b"),
        "block",
    );
    let c = site.page("c.html", "<!DOCTYPE html><title>C</title>c");
    let fetcher = TestFetcher::new();
    let mut page = new_page(fetcher.clone());
    load(&mut page, a.clone());

    page.navigate(b);
    assert_eq!(page.load_state(), LoadState::LoadingDocument);
    load(&mut page, c);
    assert_eq!(page.title(), "C");

    // B never arrived, so back goes to A.
    assert!(page.go_back());
    finish(&mut page);
    assert_eq!(page.title(), "A");
    assert_eq!(page.url(), Some(&a));

    // B's response arrives late and is ignored.
    fetcher.release();
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        assert!(!page.process_network(), "a stale response changed the page");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(page.title(), "A");
    assert!(page.can_go_forward());
}

#[test]
fn a_new_navigation_does_not_wait_for_old_requests() {
    let site = Site::new("cancel");
    let image = site.file("image.png", &tiny_png());
    let images: Vec<String> = (0..20)
        .map(|i| format!("<img src='{}'>", with_query(&image, &format!("slow&{i}"))))
        .collect();
    let images = images.concat();
    let a = site.page("a.html", &format!("<!DOCTYPE html>{images}"));
    let b = site.page("b.html", "<!DOCTYPE html><title>B</title>b");
    let fetcher = TestFetcher::new();
    let mut page = new_page(fetcher);
    page.navigate(a);
    while page.document().is_none() {
        page.wait_until_loaded(Duration::from_millis(10));
    }
    // 20 images of 200 ms on 2 threads would delay B by 2 s.
    let started = Instant::now();
    load(&mut page, b);
    assert_eq!(page.title(), "B");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn stop_cancels_the_pending_navigation() {
    let site = Site::new("stop");
    let a = site.page("a.html", "<!DOCTYPE html><title>A</title>a");
    let b = with_query(&site.page("b.html", "b"), "block");
    let fetcher = TestFetcher::new();
    let mut page = new_page(fetcher.clone());
    load(&mut page, a.clone());
    page.navigate(b.clone());
    assert_eq!(page.url(), Some(&b));
    page.stop();
    assert!(!page.is_loading());
    assert_eq!(page.url(), Some(&a));
    fetcher.release();
}

#[test]
fn image_documents_fetch_the_image_once() {
    let site = Site::new("image-document");
    let image = site.file("image.png", &tiny_png());
    let fetcher = TestFetcher::new();
    let mut page = new_page(fetcher.clone());
    load(&mut page, image.clone());
    assert_eq!(page.load_state(), LoadState::Complete);
    assert_eq!(fetcher.fetch_count(&image), 1);
    let boxes = swb_engine::element_boxes(&page);
    let img = boxes.iter().find(|b| b.tag == "img").unwrap();
    assert_eq!(img.rect.map(|r| (r.width, r.height)), Some((2.0, 3.0)));
}

// ----- Links and hit testing -----

#[test]
fn javascript_and_mailto_links_are_ignored() {
    let site = Site::new("links");
    let url = site.page(
        "links.html",
        "<!DOCTYPE html><title>L</title><body style='margin:0'>\
         <a href='javascript:void(0)' style='display:block;height:50px'>js</a>\
         <a href='mailto:a@b.test' style='display:block;height:50px'>mail</a>",
    );
    let mut page = local_page();
    load(&mut page, url.clone());
    assert!(!page.click(10.0, 10.0));
    assert!(!page.click(10.0, 60.0));
    assert!(!page.is_loading());
    assert_eq!(page.url(), Some(&url));
    assert_eq!(page.title(), "L");
    assert!(!page.can_go_back());
}

#[test]
fn hit_testing_follows_paint_order() {
    let site = Site::new("hit-order");
    let url = site.page(
        "hit.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='position:relative;z-index:1;height:0'>\
           <a href='top.html' style='display:block;width:100px;height:50px'>top</a></div>\
         <a href='below.html' style='display:block;height:50px'>below</a>\
         <div style='position:relative;z-index:-1;height:0;margin-top:-50px'>\
           <a href='under.html' style='display:block;height:50px'>under</a></div>",
    );
    let mut page = local_page();
    load(&mut page, url);
    // The z-index 1 box is painted over the later normal-flow link.
    assert_eq!(link_at(&mut page, 10.0, 25.0).as_deref(), Some("top.html"));
    // The z-index -1 box is painted under the normal-flow link.
    assert_eq!(
        link_at(&mut page, 200.0, 25.0).as_deref(),
        Some("below.html")
    );
}

#[test]
fn hit_testing_respects_overflow_clips() {
    let site = Site::new("hit-clip");
    let url = site.page(
        "clip.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='overflow:hidden;width:100px;height:100px'>\
           <a href='inner.html' style='position:relative;display:block;width:300px;height:300px'>x</a>\
         </div>",
    );
    let mut page = local_page();
    load(&mut page, url);
    assert_eq!(
        link_at(&mut page, 50.0, 50.0).as_deref(),
        Some("inner.html")
    );
    assert_eq!(link_at(&mut page, 150.0, 50.0), None);
    assert_eq!(link_at(&mut page, 50.0, 150.0), None);
}

#[test]
fn transparent_links_can_be_clicked() {
    let site = Site::new("hit-transparent");
    let url = site.page(
        "t.html",
        "<!DOCTYPE html><body style='margin:0'>\
         <a href='t2.html' style='display:block;height:50px;opacity:0'>x</a>",
    );
    let mut page = local_page();
    load(&mut page, url);
    assert_eq!(link_at(&mut page, 10.0, 10.0).as_deref(), Some("t2.html"));
}

#[test]
fn links_in_table_cells_and_around_blocks_are_hit() {
    let site = Site::new("table-links");
    let url = site.page(
        "t.html",
        "<!DOCTYPE html><body style='margin:0'><table cellspacing=0 cellpadding=0><tr>\
         <td><a href='vote.html'><div style='width:10px;height:10px;margin:3px'></div></a></td>\
         <td><a href='title.html'>title</a></td></tr></table>",
    );
    let mut page = local_page();
    load(&mut page, url);
    assert_eq!(link_at(&mut page, 5.0, 5.0).as_deref(), Some("vote.html"));
    assert_eq!(link_at(&mut page, 25.0, 8.0).as_deref(), Some("title.html"));
}

#[test]
fn mouse_leave_clears_the_hovered_link() {
    let site = Site::new("hover");
    let url = site.page(
        "h.html",
        "<!DOCTYPE html><body style='margin:0'><a href='x.html' style='display:block;height:50px'>x</a>",
    );
    let mut page = local_page();
    load(&mut page, url);
    assert!(page.mouse_move(10.0, 10.0));
    assert!(page.hovered_link().is_some());
    assert!(page.mouse_leave());
    assert!(page.hovered_link().is_none());
}

// ----- Display list -----

fn display_list(html: &str, test: &str) -> DisplayList {
    let site = Site::new(test);
    let mut page = local_page();
    load(&mut page, site.page("page.html", html));
    common::display_list(&mut page)
}

/// The indices of the rectangles filled with `color`.
fn rects_of(list: &DisplayList, color: Rgba) -> Vec<usize> {
    list.items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, DisplayItem::Rect { color: c, .. } if *c == color))
        .map(|(i, _)| i)
        .collect()
}

/// The clip rectangles that are active at item `index`.
fn clips_at(list: &DisplayList, index: usize) -> Vec<swb_engine::Rect> {
    let mut clips = Vec::new();
    for item in &list.items[..index] {
        match item {
            DisplayItem::PushClip(r) => clips.push(*r),
            DisplayItem::PopClip => {
                clips.pop();
            }
            _ => {}
        }
    }
    clips
}

const MARK: Rgba = Rgba::rgb(1, 2, 3);
const MARK2: Rgba = Rgba::rgb(4, 5, 6);

#[test]
fn text_decorations_are_painted_once() {
    let list = display_list(
        "<!DOCTYPE html><p style='text-decoration:underline;text-decoration-color:rgb(1,2,3)'>x</p>\
         <p>a <span style='text-decoration:line-through;text-decoration-color:rgb(4,5,6)'>b</span></p>",
        "dl-decorations",
    );
    assert_eq!(rects_of(&list, MARK).len(), 1);
    assert_eq!(rects_of(&list, MARK2).len(), 1);
}

#[test]
fn body_background_is_painted_once_on_the_canvas() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:30px;background:rgb(1,2,3)'><p>x</p>",
        "dl-canvas",
    );
    let rects = rects_of(&list, MARK);
    assert_eq!(rects.len(), 1);
    let DisplayItem::Rect { rect, .. } = &list.items[rects[0]] else {
        unreachable!();
    };
    assert_eq!((rect.x, rect.y), (0.0, 0.0));
    assert!(rect.width >= 800.0 && rect.height >= 600.0);
}

#[test]
fn positioned_boxes_keep_the_clips_of_their_ancestors() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='overflow:hidden;width:100px;height:100px'>\
           <div style='position:relative;width:300px;height:300px;background:rgb(1,2,3)'></div>\
         </div>",
        "dl-clip",
    );
    let rects = rects_of(&list, MARK);
    assert_eq!(rects.len(), 1);
    let clips = clips_at(&list, rects[0]);
    assert!(
        clips.iter().any(|c| c.width == 100.0 && c.height == 100.0),
        "{clips:?}"
    );
}

#[test]
fn negative_z_index_children_are_clipped_by_their_stacking_context() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='position:relative;z-index:0;overflow:hidden;width:100px;height:100px;background:rgb(4,5,6)'>\
           <div style='position:relative;z-index:-1;width:300px;height:300px;background:rgb(1,2,3)'></div>\
         </div>",
        "dl-negative",
    );
    let child = rects_of(&list, MARK)[0];
    let parent = rects_of(&list, MARK2)[0];
    assert!(
        parent < child,
        "the child is painted over its context's background"
    );
    assert!(!clips_at(&list, child).is_empty(), "the child is clipped");
}

#[test]
fn positioned_boxes_paint_in_z_index_order() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='position:relative;z-index:2;height:10px;background:rgb(1,2,3)'></div>\
         <div style='position:relative;z-index:1;height:10px;background:rgb(4,5,6)'></div>\
         <div style='height:10px;background:rgb(7,8,9)'></div>",
        "dl-order",
    );
    let first = rects_of(&list, MARK)[0];
    let second = rects_of(&list, MARK2)[0];
    let flow = rects_of(&list, Rgba::rgb(7, 8, 9))[0];
    assert!(flow < second && second < first);
}

#[test]
fn opacity_groups_record_their_bounds() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <div style='margin:10px;opacity:0.5;width:50px;height:20px;background:red'></div>",
        "dl-opacity",
    );
    let bounds = list.items.iter().find_map(|item| match item {
        DisplayItem::PushOpacity { bounds, .. } => Some(*bounds),
        _ => None,
    });
    assert_eq!(bounds, Some(swb_engine::Rect::new(10.0, 10.0, 50.0, 20.0)));
}

/// The rectangle of a `DisplayItem::Rect`.
fn rect_at(list: &DisplayList, index: usize) -> swb_engine::Rect {
    match &list.items[index] {
        DisplayItem::Rect { rect, .. } => *rect,
        other => panic!("not a rectangle: {other:?}"),
    }
}

#[test]
fn row_and_column_backgrounds_are_painted_in_cells() {
    // Border spacing 10px: the row's background is not painted between the
    // cells; the column's only in its cell.
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <table style='border-spacing:10px'><col><col style='background:rgb(4,5,6)'>\
         <tr style='background:rgb(1,2,3)'><td style='width:30px;padding:0'>a</td>\
         <td style='width:40px;padding:0'>b</td></tr></table>",
        "dl-table-backgrounds",
    );
    // The row's color fills each cell, without a clip.
    let rows = rects_of(&list, MARK);
    assert_eq!(rows.len(), 2, "one rectangle per cell");
    let x_width = |index: usize| (rect_at(&list, index).x, rect_at(&list, index).width);
    assert_eq!(x_width(rows[0]), (10.0, 30.0));
    assert_eq!(x_width(rows[1]), (50.0, 40.0));
    assert_eq!(clips_at(&list, rows[1]).len(), 0);
    let columns = rects_of(&list, MARK2);
    assert_eq!(columns.len(), 1);
    assert_eq!(rect_at(&list, columns[0]).x, 50.0);
    assert!(columns[0] < rows[1], "the column is below the row");
}

#[test]
fn row_gradients_continue_across_cells() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'><table style='border-spacing:10px'>\
         <tr style='background:linear-gradient(white, black)'><td style='width:30px;padding:0'>a</td>\
         <td style='width:40px;padding:0'>b</td></tr></table>",
        "dl-row-gradient",
    );
    // Each cell paints the gradient of the whole row, in the cell only.
    let gradients: Vec<(swb_engine::Rect, swb_engine::Rect)> = list
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::LinearGradient { rect, clip, .. } => Some((*rect, *clip)),
            _ => None,
        })
        .collect();
    assert_eq!(gradients.len(), 2);
    assert!(
        gradients
            .iter()
            .all(|(r, _)| r.x == 10.0 && r.width == 80.0)
    );
    assert_eq!((gradients[0].1.x, gradients[0].1.width), (10.0, 30.0));
    assert_eq!((gradients[1].1.x, gradients[1].1.width), (50.0, 40.0));
    assert!(
        !list
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::PushClip(_))),
        "no clip per cell"
    );
}

#[test]
fn a_float_makes_a_cell_non_empty() {
    let list = display_list(
        "<!DOCTYPE html><table style='empty-cells:hide'><tr>\
         <td style='background:rgb(1,2,3)'><div style='float:left;width:5px;height:5px'></div></td>\
         </tr></table>",
        "dl-empty-cells-float",
    );
    assert_eq!(rects_of(&list, MARK).len(), 1);
}

#[test]
fn column_group_background_is_painted_once_per_cell() {
    let list = display_list(
        "<!DOCTYPE html><table><colgroup span=3 style='background:rgb(1,2,3)'></colgroup>\
         <tr><td>a</td><td>b</td><td>c</td></tr><tr><td colspan=3>wide</td></tr></table>",
        "dl-colgroup",
    );
    assert_eq!(rects_of(&list, MARK).len(), 4, "one per cell");
}

#[test]
fn table_background_fills_the_grid_without_captions() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <table style='background:rgb(1,2,3)'><caption style='height:30px'>c</caption>\
         <tr><td>a</td></tr></table>",
        "dl-table-caption",
    );
    let rects = rects_of(&list, MARK);
    assert_eq!(rects.len(), 1);
    assert_eq!(rect_at(&list, rects[0]).y, 30.0);
}

#[test]
fn empty_cells_hide_paints_no_background() {
    let list = display_list(
        "<!DOCTYPE html><table style='empty-cells:hide'>\
         <tr><td style='background:rgb(1,2,3)'></td><td style='background:rgb(4,5,6)'>x</td></tr>\
         </table>",
        "dl-empty-cells",
    );
    assert_eq!(rects_of(&list, MARK), Vec::<usize>::new());
    assert_eq!(rects_of(&list, MARK2).len(), 1);
}

#[test]
fn the_table_paints_collapsed_borders() {
    let list = display_list(
        "<!DOCTYPE html><body style='margin:0'>\
         <table style='border-collapse:collapse;border:4px solid rgb(1,2,3)'>\
         <tr><td style='border:2px solid rgb(4,5,6)'>a</td><td>b</td></tr></table>",
        "dl-collapsed",
    );
    let borders: Vec<(swb_engine::Rect, Rgba)> = list
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Border { rect, colors, .. } => Some((*rect, colors[0])),
            _ => None,
        })
        .collect();
    // Each segment is one border item; the table's 4px border wins on the
    // outside, the cell's 2px border between the cells.
    assert!(borders.iter().all(|(_, c)| *c == MARK || *c == MARK2));
    let inner: Vec<_> = borders.iter().filter(|(_, c)| *c == MARK2).collect();
    assert_eq!(inner.len(), 1, "{borders:?}");
    assert_eq!(inner[0].0.width, 2.0);
    let outer = borders.iter().filter(|(_, c)| *c == MARK).count();
    assert_eq!(outer, 4, "one segment per side");
}

// ----- Security -----

/// Serves one page at `https://a.test/` and fetches everything else from
/// the local file system, recording every requested URL.
struct HttpPageFetcher {
    html: String,
    fetched: Mutex<Vec<Url>>,
}

impl Fetcher for HttpPageFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if request.url.as_str() == "https://a.test/" {
            let mut headers = swb_net::Headers::new();
            headers.append("content-type", "text/html");
            return Ok(Response {
                url: request.url.clone(),
                status: 200,
                headers,
                body: self.html.clone().into_bytes(),
                redirected: false,
            });
        }
        self.fetched.lock().unwrap().push(request.url.clone());
        NetworkFetcher::new().fetch(request)
    }
}

#[test]
fn web_pages_cannot_load_local_files() {
    let site = Site::new("no-file-from-http");
    let image = site.file("img.png", &tiny_png());
    let sheet = site.file("style.css", b"body { margin: 0 }");
    let html = format!(
        "<!DOCTYPE html><link rel=stylesheet href='{sheet}'><img src='{image}'>\
         <a href='{image}' style='display:block;height:50px'>local</a>"
    );
    let fetcher = Arc::new(HttpPageFetcher {
        html,
        fetched: Mutex::new(Vec::new()),
    });
    let mut page = new_page(fetcher.clone());
    load(&mut page, Url::parse("https://a.test/").unwrap());
    assert!(
        fetcher.fetched.lock().unwrap().is_empty(),
        "no file: requests"
    );
    assert!(!page.follow_link(image));
    // A file: page may load other local files.
    let local = site.page(
        "local.html",
        &format!("<img src='{}'>", site.file("i.png", &tiny_png())),
    );
    load(&mut page, local);
    assert_eq!(fetcher.fetched.lock().unwrap().len(), 2);
}

// ----- Request initiators -----

/// Serves `html` at `https://a.test/` and 404 for every other URL. Records
/// every request.
struct RequestLog {
    html: String,
    requests: Mutex<Vec<Request>>,
}

impl Fetcher for RequestLog {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.requests.lock().unwrap().push(request.clone());
        let (status, body) = if request.url.as_str() == "https://a.test/" {
            (200, self.html.clone())
        } else {
            (404, String::new())
        };
        let mut headers = swb_net::Headers::new();
        headers.append("content-type", "text/html");
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body: body.into_bytes(),
            redirected: false,
        })
    }
}

/// Cookies decide `SameSite` from the initiator: subresources and links
/// carry the document's origin, typed addresses carry none.
#[test]
fn requests_carry_their_initiator() {
    let fetcher = Arc::new(RequestLog {
        html: "<!DOCTYPE html><link rel=stylesheet href='https://b.test/s.css'>\
               <img src='https://b.test/i.png'>"
            .to_owned(),
        requests: Mutex::new(Vec::new()),
    });
    let mut page = new_page(fetcher.clone());
    load(&mut page, Url::parse("https://a.test/").unwrap());
    assert!(page.follow_link(Url::parse("https://c.test/").unwrap()));
    finish(&mut page);

    let a_test = Some(Url::parse("https://a.test/").unwrap().origin());
    let requests = fetcher.requests.lock().unwrap();
    let initiator = |url: &str, destination: Destination| {
        let request = requests
            .iter()
            .find(|r| r.url.as_str() == url)
            .unwrap_or_else(|| panic!("no request for {url}"));
        assert_eq!(request.destination, destination, "{url}");
        request.initiator.clone()
    };
    assert_eq!(initiator("https://a.test/", Destination::Document), None);
    assert_eq!(
        initiator("https://b.test/s.css", Destination::Style),
        a_test
    );
    assert_eq!(
        initiator("https://b.test/i.png", Destination::Image),
        a_test
    );
    assert_eq!(initiator("https://c.test/", Destination::Document), a_test);
    drop(requests);

    // Back, forward and reload use the initiator of the history entry
    // (ADR 0012): the typed address has none, the link has a.test.
    let last_initiator = |page: &mut Page, go: fn(&mut Page)| {
        go(page);
        finish(page);
        let requests = fetcher.requests.lock().unwrap();
        let last = requests.iter().rev().find(|r| r.is_top_level_navigation());
        last.unwrap().initiator.clone()
    };
    assert_eq!(last_initiator(&mut page, |p| assert!(p.go_back())), None);
    assert_eq!(
        last_initiator(&mut page, |p| assert!(p.go_forward())),
        a_test
    );
    assert_eq!(last_initiator(&mut page, Page::reload), a_test);
}
