//! Input in frames (ADR 0031 section 8, M9 feature 11): hit testing through
//! frames, hover per document, presses and drags that stay in a frame,
//! links and forms with their targets, the wheel and key scroll chains,
//! focus and Tab, keys, selection. The values come from Chromium 148 where
//! the probe file has them (`input-*` cases; `tests/frame_input.rs` of the
//! automation crate runs those with the steps of the probe); the tests here
//! cover what a probe cannot see and the hostile pages.

use std::fmt::Write as _;
use std::time::Duration;

use swb_engine::{FrameId, Key, Modifiers, MouseButton, NodeId, Page, Point};

use super::common::{CTRL, SHIFT, click, node, on_page_thread, rect, value};
use super::{Res, Sim, frame, open};

/// A parent with the frame `f` (`src` `f.html`) at (100, 100), 300×200,
/// without a border.
const PARENT: &str = "<!doctype html><style>body{margin:0} #f{position:absolute;left:100px;top:100px;\
    width:300px;height:200px;border:0}</style><iframe id=f src=f.html></iframe>";

fn html(body: &str) -> Res {
    Res::html(body)
}

/// Loads `PARENT` with the frame document `frame_html` and more resources.
fn parent_with(frame_html: &str, more: &[(&str, Res)], js: bool) -> Page {
    let mut resources = vec![("/case/f.html", html(frame_html))];
    resources.extend(more.iter().map(|(p, r)| (*p, r.clone())));
    let (mut page, _) = open(PARENT, &resources, js);
    settle(&mut page);
    page
}

/// Finishes the loads of the frames and lays everything out.
fn settle(page: &mut Page) {
    assert!(page.wait_until_idle(Duration::from_millis(30), Duration::from_secs(20)));
    page.update_layout();
    page.screenshot(false).unwrap();
}

/// The node of `#id` in the document of `frame`.
fn frame_node(page: &Page, frame: FrameId, id: &str) -> NodeId {
    page.frame_document(frame)
        .unwrap()
        .dom()
        .element_by_id(id)
        .unwrap_or_else(|| panic!("no #{id} in {frame}"))
}

fn key(page: &mut Page, key: &Key) -> bool {
    page.key_down(key, Modifiers::NONE)
}

fn space() -> Key {
    Key::Character(" ".to_owned())
}

fn ch(c: char) -> Key {
    Key::Character(c.to_string())
}

// ----- Hit testing -----

#[test]
fn a_hit_goes_into_the_content_of_a_frame() {
    on_page_thread(|| {
        let mut page = parent_with(
            "<!doctype html><style>body{margin:0;height:200px}#a{position:absolute;left:10px;top:20px;width:100px;height:50px}</style><div id=a>a</div>",
            &[],
            false,
        );
        let f = frame(&page, "f");
        let a = frame_node(&page, f, "a");
        // Inside the box `a` of the frame.
        let hit = page.hit_test(150.0, 140.0).unwrap();
        assert_eq!((hit.frame, hit.node), (f, a));
        // In the frame, outside `a`: the body of the frame's document.
        let hit = page.hit_test(300.0, 250.0).unwrap();
        assert_eq!(hit.frame, f);
        assert_ne!(hit.node, a);
        // Outside the frame: the parent's own document.
        assert!(
            page.hit_test(50.0, 50.0)
                .is_none_or(|h| h.frame == FrameId::TOP)
        );
    });
}

#[test]
fn a_hit_on_the_border_of_an_iframe_finds_the_iframe() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0}#f{position:absolute;left:100px;top:100px;\
            width:300px;height:100px;border:5px solid red}</style><iframe id=f srcdoc=\"<p id=a>x</p>\"></iframe>";
        let (mut page, _) = open(html, &[], false);
        settle(&mut page);
        let iframe = node(&page, "f");
        let hit = page.hit_test(102.0, 150.0).unwrap();
        assert_eq!((hit.frame, hit.node), (FrameId::TOP, iframe));
        let f = frame(&page, "f");
        assert_eq!(page.hit_test(150.0, 150.0).unwrap().frame, f);
    });
}

#[test]
fn pointer_events_visibility_and_covers_decide_what_a_frame_hits() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0}#u{position:absolute;left:100px;top:100px;\
            width:300px;height:100px}iframe{position:absolute;left:100px;top:100px;width:300px;height:100px;border:0}\
            #c{position:absolute;left:100px;top:100px;width:150px;height:100px}</style>\
            <div id=u></div><iframe id=f srcdoc=\"<style>body{margin:0;height:100px}</style><p id=a>x</p>\"></iframe><div id=c></div>";
        let (mut page, _) = open(html, &[], false);
        settle(&mut page);
        let f = frame(&page, "f");
        // The cover is above the frame on the left; the frame shows on the right.
        assert_eq!(page.hit_test(150.0, 150.0).unwrap().node, node(&page, "c"));
        assert_eq!(page.hit_test(350.0, 150.0).unwrap().frame, f);
        // `pointer-events: none` and `visibility: hidden` let it through.
        value_set(&mut page, "document.getElementById('c').remove()");
        page.change_dom(|d| {
            let iframe = d.element_by_id("f").unwrap();
            d.set_attr(iframe, "style", "pointer-events:none").unwrap();
        });
        settle(&mut page);
        assert_eq!(page.hit_test(350.0, 150.0).unwrap().node, node(&page, "u"));
        page.change_dom(|d| {
            let iframe = d.element_by_id("f").unwrap();
            d.set_attr(iframe, "style", "visibility:hidden").unwrap();
        });
        settle(&mut page);
        assert_eq!(page.hit_test(350.0, 150.0).unwrap().node, node(&page, "u"));
    });
}

/// Runs `source` if the page has scripts; the tests without scripts change
/// the DOM through `Page::change_dom`.
fn value_set(page: &mut Page, source: &str) {
    if page.document_has_scripting() {
        value(page, source);
    } else if let Some(id) = source
        .strip_prefix("document.getElementById('")
        .and_then(|s| s.split('\'').next())
    {
        page.change_dom(|d| {
            if let Some(n) = d.element_by_id(id) {
                d.remove(n);
            }
        });
    }
}

#[test]
fn a_hit_goes_through_a_transform_and_a_rounded_clip() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0}#f{position:absolute;left:100px;top:100px;\
            width:200px;height:100px;border:0;transform:rotate(90deg);border-radius:50px}</style>\
            <iframe id=f srcdoc=\"<style>body{margin:0}#a{position:absolute;left:0;top:0;width:60px;height:60px}</style><div id=a></div>\"></iframe>";
        let (mut page, _) = open(html, &[], false);
        settle(&mut page);
        let f = frame(&page, "f");
        let a = frame_node(&page, f, "a");
        // The frame is 200x100 around (200, 150); rotated by 90 degrees it
        // covers x 150..250, y 50..250. Its top left corner (0, 0) goes to
        // the top right: a point inside the corner arc shows `a`, one
        // outside the rounded clip only the `iframe`.
        let hit = page.hit_test(220.0, 80.0).unwrap();
        assert_eq!((hit.frame, hit.node), (f, a));
        let hit = page.hit_test(248.0, 52.0).unwrap();
        assert_eq!((hit.frame, hit.node), (FrameId::TOP, node(&page, "f")));
    });
}

#[test]
fn nested_frames_hit_to_their_depth() {
    on_page_thread(|| {
        let outer = "<!doctype html><style>body{margin:0}iframe{position:absolute;left:10px;top:10px;width:150px;height:100px;border:0}</style>\
            <iframe id=g src=g.html></iframe>";
        let inner = "<!doctype html><style>body{margin:0}#z{width:100px;height:100px}</style><div id=z></div>";
        let mut page = parent_with(outer, &[("/case/g.html", html(inner))], false);
        let f = frame(&page, "f");
        let g_node = frame_node(&page, f, "g");
        let g = page.frame_of(f, g_node).unwrap();
        let z = frame_node(&page, g, "z");
        let hit = page.hit_test(120.0, 120.0).unwrap();
        assert_eq!((hit.frame, hit.node), (g, z));
        assert_eq!(page.frame_children(FrameId::TOP), vec![f]);
    });
}

// ----- Pointer: hover, cursor, links -----

#[test]
fn hover_is_per_document_and_the_cursor_comes_from_the_frame() {
    on_page_thread(|| {
        let mut page = parent_with(
            "<!doctype html><style>body{margin:0}#a{position:absolute;left:0;top:0;width:100px;height:50px;cursor:pointer;background:#eee}#a:hover{background:#f00}</style>\
             <div id=a></div><a id=l href=t.html style='position:absolute;left:0;top:100px;display:block;width:100px;height:20px'>l</a>",
            &[],
            false,
        );
        let f = frame(&page, "f");
        let a = frame_node(&page, f, "a");
        let iframe = node(&page, "f");
        page.mouse_move(150.0, 120.0);
        assert_eq!(page.document().unwrap().element_by_id("f"), Some(iframe));
        assert_eq!(
            page.document_state().unwrap().hovered_element(),
            Some(iframe)
        );
        assert_eq!(page.frame_document(f).unwrap().hovered_element(), Some(a));
        assert_eq!(page.cursor(), swb_engine::Cursor::Pointer);
        // The link in the frame.
        page.mouse_move(120.0, 210.0);
        assert_eq!(
            page.hovered_link().map(|u| u.path().to_owned()),
            Some("/case/t.html".to_owned())
        );
        // Leaving the frame clears its hover state.
        page.mouse_move(10.0, 10.0);
        assert_ne!(
            page.document_state().unwrap().hovered_element(),
            Some(iframe)
        );
        assert_eq!(page.frame_document(f).unwrap().hovered_element(), None);
        assert_eq!(page.hovered_link(), None);
        // And leaving the page.
        page.mouse_move(150.0, 120.0);
        assert!(page.frame_document(f).unwrap().hovered_element().is_some());
        page.mouse_leave();
        assert_eq!(page.frame_document(f).unwrap().hovered_element(), None);
        assert_eq!(page.document_state().unwrap().hovered_element(), None);
    });
}

#[test]
fn a_hover_rule_in_a_frame_repaints_the_frame() {
    on_page_thread(|| {
        let mut page = parent_with(
            "<!doctype html><style>body{margin:0;height:200px;background:rgb(0,0,255)}body:hover{background:rgb(0,255,0)}</style>",
            &[],
            false,
        );
        let color = |page: &mut Page| {
            let p = page.screenshot(false).unwrap();
            let i = (150 + 150 * p.width() as usize) * 4;
            (p.data()[i], p.data()[i + 1], p.data()[i + 2])
        };
        assert_eq!(color(&mut page), (0, 0, 255));
        page.mouse_move(150.0, 150.0);
        assert_eq!(color(&mut page), (0, 255, 0));
        page.mouse_move(10.0, 10.0);
        assert_eq!(color(&mut page), (0, 0, 255));
    });
}

#[test]
fn a_press_in_a_frame_stays_in_it_until_the_release() {
    on_page_thread(|| {
        let mut page = parent_with(
            "<!doctype html><style>body{margin:0;font:16px/20px sans-serif}p{margin:0 0 0 10px}</style><p id=p>alpha beta gamma</p>",
            &[("/case/t.html", html("t"))],
            false,
        );
        let f = frame(&page, "f");
        page.mouse_move(120.0, 110.0);
        page.mouse_down(120.0, 110.0, MouseButton::Primary, Modifiers::NONE, 1);
        let iframe = node(&page, "f");
        assert_eq!(
            page.document_state().unwrap().pressed_element(),
            Some(iframe)
        );
        assert!(page.frame_document(f).unwrap().pressed_element().is_some());
        // The pointer leaves the frame: the frame keeps the drag.
        page.mouse_move(160.0, 112.0);
        assert_eq!(page.frame_selected_text(f), "lpha b");
        page.mouse_move(10.0, 10.0);
        assert!(page.frame_document(f).unwrap().hovered_element().is_some());
        assert_eq!(
            page.document_state().unwrap().hovered_element(),
            Some(iframe)
        );
        // The pointer outside the frame: the selection reaches the start of
        // the frame's document (measured in Chromium, case `ex-sel-drag3`).
        assert_eq!(page.frame_selected_text(f), "a");
        // The release outside ends the press in the frame.
        page.mouse_up(10.0, 10.0, MouseButton::Primary);
        assert_eq!(page.document_state().unwrap().pressed_element(), None);
        assert_eq!(page.frame_document(f).unwrap().pressed_element(), None);
    });
}

// ----- Links and forms -----

const LINKS: &str = "<!doctype html><style>body{margin:0}a{position:absolute;left:10px;width:200px;height:20px;display:block}</style>\
    <a id=self href='t.html?self' style='top:10px'>self</a>\
    <a id=top target=_top href='t.html?top' style='top:40px'>top</a>\
    <a id=par target=_parent href='t.html?par' style='top:70px'>par</a>\
    <a id=named target=other href='t.html?named' style='top:100px'>named</a>\
    <a id=frag href='#z' style='top:130px'>frag</a>\
    <a id=blank target=_blank href='t.html?blank' style='top:160px'>blank</a>\
    <div style='height:600px'></div><div id=z>Z</div>";

const LINKS_PARENT: &str = "<!doctype html><style>body{margin:0}iframe{position:absolute;border:0;width:300px;height:200px;top:100px}</style>\
    <iframe id=f name=fn src=l.html style='left:100px'></iframe><iframe id=g name=other src=about:blank style='left:450px'></iframe>\
    <a id=pl target=other href='t.html?pnamed' style='position:absolute;left:10px;top:400px;width:100px;height:20px;display:block'>pl</a>\
    <a id=pf target=fn href='t.html?pf' style='position:absolute;left:10px;top:430px;width:100px;height:20px;display:block'>pf</a>\
    <form id=fm target=other action=t.html style='position:absolute;left:10px;top:500px'><input name=a value=1><input type=submit id=sb value=go></form>";

fn links_page() -> (Page, std::sync::Arc<Sim>) {
    let (mut page, sim) = open(
        LINKS_PARENT,
        &[
            ("/case/l.html", html(LINKS)),
            (
                "/case/t.html",
                html("<!doctype html><title>t</title><p>target"),
            ),
        ],
        false,
    );
    settle(&mut page);
    (page, sim)
}

fn frame_url_of(page: &Page, id: &str) -> String {
    let f = frame(page, id);
    let url = page.frame_document(f).unwrap().url().clone();
    format!(
        "{}{}",
        url.path().trim_start_matches("/case/"),
        url.query().map_or(String::new(), |q| format!("?{q}"))
            + &url.fragment().map_or(String::new(), |f| format!("#{f}"))
    )
}

/// Clicks the link `id` of the frame `f`.
fn click_in_frame(page: &mut Page, f: FrameId, id: &str) {
    let n = frame_node(page, f, id);
    let b = page
        .frame_document(f)
        .unwrap()
        .fragments()
        .unwrap()
        .element_boxes()[&n];
    let at = rect(page, "f");
    page.click(at.x + b.x + 5.0, at.y + b.y + 5.0);
    settle(page);
}

#[test]
fn a_link_in_a_frame_navigates_the_frame() {
    on_page_thread(|| {
        let (mut page, _) = links_page();
        let f = frame(&page, "f");
        click_in_frame(&mut page, f, "self");
        assert_eq!(frame_url_of(&page, "f"), "t.html?self");
        // The parent did not navigate; the frame keeps its id.
        assert_eq!(page.url().unwrap().path(), "/case/case.html");
        assert_eq!(frame(&page, "f"), f);
    });
}

#[test]
fn a_blank_target_in_a_frame_navigates_the_frame() {
    on_page_thread(|| {
        let (mut page, _) = links_page();
        let f = frame(&page, "f");
        click_in_frame(&mut page, f, "blank");
        assert_eq!(frame_url_of(&page, "f"), "t.html?blank");
    });
}

#[test]
fn a_link_with_target_top_or_parent_navigates_the_page() {
    on_page_thread(|| {
        for id in ["top", "par"] {
            let (mut page, _) = links_page();
            let f = frame(&page, "f");
            click_in_frame(&mut page, f, id);
            assert!(page.wait_until_loaded(Duration::from_secs(10)));
            assert_eq!(page.url().unwrap().query(), Some(id), "{id}");
        }
    });
}

#[test]
fn a_link_with_a_frame_name_navigates_that_frame() {
    on_page_thread(|| {
        let (mut page, _) = links_page();
        let f = frame(&page, "f");
        click_in_frame(&mut page, f, "named");
        assert_eq!(frame_url_of(&page, "g"), "t.html?named");
        assert_eq!(frame_url_of(&page, "f"), "l.html");
        // The parent's own link and form, by the frame's name.
        let (mut page, _) = links_page();
        click(&mut page, "pl");
        settle(&mut page);
        assert_eq!(frame_url_of(&page, "g"), "t.html?pnamed");
        let (mut page, _) = links_page();
        click(&mut page, "pf");
        settle(&mut page);
        assert_eq!(frame_url_of(&page, "f"), "t.html?pf");
        let (mut page, _) = links_page();
        click(&mut page, "sb");
        settle(&mut page);
        assert_eq!(frame_url_of(&page, "g"), "t.html?a=1");
    });
}

#[test]
fn a_fragment_link_scrolls_the_frame() {
    on_page_thread(|| {
        let (mut page, _) = links_page();
        let f = frame(&page, "f");
        click_in_frame(&mut page, f, "frag");
        assert!(page.frame_document(f).unwrap().scroll_position().y > 100.0);
        assert_eq!(frame_url_of(&page, "f"), "l.html#z");
        // No new document: the frame keeps its state.
        assert_eq!(page.url().unwrap().fragment(), None);
    });
}

// ----- Wheel -----

const TALL: &str = "<!doctype html><style>body{margin:0}</style><div style='height:1000px'></div>";

fn tall_pair() -> Page {
    let html = "<!doctype html><style>body{margin:0;height:3000px}iframe{position:absolute;top:100px;width:300px;height:200px;border:0}</style>\
        <iframe id=f src=tall.html style='left:100px'></iframe><iframe id=g scrolling=no src=tall.html style='left:450px'></iframe>";
    let (mut page, _) = open(html, &[("/case/tall.html", Res::html(TALL))], false);
    settle(&mut page);
    page
}

#[test]
fn the_wheel_scrolls_the_frame_then_the_parent() {
    on_page_thread(|| {
        let mut page = tall_pair();
        let f = frame(&page, "f");
        let top = |page: &Page| page.scroll_position().y;
        let inner = |page: &Page| page.frame_document(f).unwrap().scroll_position().y;
        page.mouse_move(200.0, 200.0);
        page.wheel(200.0, 200.0, 0.0, 100.0);
        assert_eq!((top(&page), inner(&page)), (0.0, 100.0));
        // A wheel past the end of the frame stops at its end.
        page.wheel(200.0, 200.0, 0.0, 2000.0);
        assert_eq!((top(&page), inner(&page)), (0.0, 800.0));
        // The next one, with the frame at its end, scrolls the parent.
        page.wheel(200.0, 200.0, 0.0, 100.0);
        assert_eq!((top(&page), inner(&page)), (100.0, 800.0));
    });
}

#[test]
fn scrolling_no_stops_the_user_from_scrolling_the_frame() {
    on_page_thread(|| {
        let mut page = tall_pair();
        let g = frame(&page, "g");
        page.wheel(500.0, 200.0, 0.0, 100.0);
        assert_eq!(page.frame_document(g).unwrap().scroll_position().y, 0.0);
        assert_eq!(page.scroll_position().y, 100.0);
        // A script can still scroll it.
        assert_eq!(
            page.frame_document(g).unwrap().scroll_position(),
            Point::new(0.0, 0.0)
        );
    });
}

#[test]
fn the_wheel_chains_through_nested_frames() {
    on_page_thread(|| {
        let outer = "<!doctype html><style>body{margin:0}iframe{position:absolute;left:0;top:0;width:200px;height:100px;border:0}</style>\
            <iframe id=g src=tall.html></iframe><div style='height:600px'></div>";
        let html = "<!doctype html><style>body{margin:0;height:3000px}iframe{position:absolute;top:100px;left:100px;width:300px;height:200px;border:0}</style>\
            <iframe id=f src=outer.html></iframe>";
        let (mut page, _) = open(
            html,
            &[
                ("/case/outer.html", Res::html(outer)),
                ("/case/tall.html", Res::html(TALL)),
            ],
            false,
        );
        settle(&mut page);
        let f = frame(&page, "f");
        let g = page.frame_of(f, frame_node(&page, f, "g")).unwrap();
        let state = |page: &Page| {
            (
                page.scroll_position().y,
                page.frame_document(f).unwrap().scroll_position().y,
                page.frame_document(g).unwrap().scroll_position().y,
            )
        };
        let mut seen = Vec::new();
        for _ in 0..5 {
            page.wheel(150.0, 150.0, 0.0, 400.0);
            seen.push(state(&page));
        }
        // Measured in Chromium (case `ex-wheel-nested`): 400, 800, 900 in
        // the inner frame, then the outer frame to its end (400), then the
        // parent.
        assert_eq!(
            seen,
            vec![
                (0.0, 0.0, 400.0),
                (0.0, 0.0, 800.0),
                (0.0, 0.0, 900.0),
                (0.0, 400.0, 900.0),
                (400.0, 400.0, 900.0)
            ]
        );
    });
}

#[test]
fn scroll_keys_act_on_the_focused_frame_and_chain_to_the_parent() {
    on_page_thread(|| {
        let mut page = tall_pair();
        let f = frame(&page, "f");
        page.click(300.0, 200.0);
        let state = |page: &Page| {
            (
                page.scroll_position().y,
                page.frame_document(f).unwrap().scroll_position().y,
            )
        };
        key(&mut page, &Key::ArrowDown);
        assert_eq!(state(&page), (0.0, 40.0));
        key(&mut page, &Key::PageDown);
        assert_eq!(state(&page), (0.0, 215.0));
        key(&mut page, &Key::End);
        assert_eq!(state(&page), (0.0, 800.0));
        // At its end: the parent scrolls.
        key(&mut page, &Key::End);
        assert_eq!(state(&page), (2400.0, 800.0));
        key(&mut page, &Key::Home);
        assert_eq!(state(&page), (2400.0, 0.0));
        // A frame with `scrolling=no` hands the keys to the parent.
        let g = frame(&page, "g");
        page.click(600.0, 200.0);
        page.scroll_to(Point::new(0.0, 0.0));
        key(&mut page, &Key::ArrowDown);
        assert_eq!(page.frame_document(g).unwrap().scroll_position().y, 0.0);
        assert_eq!(page.scroll_position().y, 40.0);
    });
}

// ----- Focus and Tab -----

fn fields(ids: &[(&str, i32, i32)]) -> String {
    let mut out = String::new();
    for (id, x, y) in ids {
        let _ = write!(
            out,
            "<input id={id} style='position:absolute;left:{x}px;top:{y}px;width:60px'>"
        );
    }
    out
}

fn tab_page() -> Page {
    let parent = format!(
        "<!doctype html><style>body{{margin:0}}</style>{}<iframe id=f src=f.html style='position:absolute;left:10px;top:50px;width:300px;height:100px;border:0'></iframe>{}",
        fields(&[("p1", 10, 10)]),
        fields(&[("p2", 10, 200)])
    );
    let frame_doc = format!(
        "<!doctype html><style>body{{margin:0}}</style>{}",
        fields(&[("c1", 10, 10), ("c2", 100, 10)])
    );
    let (mut page, _) = open(&parent, &[("/case/f.html", html(&frame_doc))], false);
    settle(&mut page);
    page
}

/// `document.activeElement.id` of the page and of the frame, as the probe
/// reads them (the `id` of the `iframe` for the page when the frame has the
/// focus; `BODY` for no element).
fn active(page: &mut Page, frame: Option<FrameId>) -> (String, String) {
    let name = |page: &Page, f: FrameId, e: Option<NodeId>| {
        let doc = page.frame_document(f).unwrap();
        e.map_or("BODY".to_owned(), |n| {
            doc.dom()
                .element(n)
                .and_then(|el| el.attr("id"))
                .unwrap_or("?")
                .to_owned()
        })
    };
    let top = page.frame_document(FrameId::TOP).unwrap();
    let top_active = top.focus_element().or(top.focused_frame_element());
    let a = name(page, FrameId::TOP, top_active);
    let b = match frame {
        Some(f) => name(page, f, page.frame_document(f).unwrap().focus_element()),
        None => "BODY".to_owned(),
    };
    (a, b)
}

#[test]
fn tab_goes_through_the_content_of_a_frame_and_back() {
    on_page_thread(|| {
        let mut page = tab_page();
        let f = frame(&page, "f");
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        page.focus(Some(node(&page, "p1")), true);
        let mut seen = Vec::new();
        for _ in 0..4 {
            key(&mut page, &Key::Tab);
            seen.push(active(&mut page, Some(f)));
        }
        // Measured (case `ex-tab-forward`): p1, f/c1, f/c2, p2, then out.
        assert_eq!(
            seen,
            vec![
                pair("f", "c1"),
                pair("f", "c2"),
                pair("p2", "BODY"),
                pair("BODY", "BODY")
            ]
        );
        page.focus(Some(node(&page, "p2")), true);
        let mut seen = Vec::new();
        for _ in 0..4 {
            page.key_down(&Key::Tab, SHIFT);
            seen.push(active(&mut page, Some(f)));
        }
        assert_eq!(
            seen,
            vec![
                pair("f", "c2"),
                pair("f", "c1"),
                pair("p1", "BODY"),
                pair("BODY", "BODY")
            ]
        );
    });
}

#[test]
fn a_frame_without_a_focusable_element_is_a_stop_and_hidden_frames_are_skipped() {
    on_page_thread(|| {
        let parent = format!(
            "<!doctype html><style>body{{margin:0}}iframe{{position:absolute;top:50px;width:100px;height:50px;border:0}}</style>{}\
             <iframe id=empty src=e.html style='left:10px'></iframe>\
             <iframe id=hid src=f.html style='left:150px;visibility:hidden'></iframe>\
             <iframe id=none src=f.html style='display:none'></iframe>\
             <iframe id=neg tabindex=-1 src=f.html style='left:300px'></iframe>\
             <iframe id=pos tabindex=2 src=f.html style='left:450px'></iframe>{}",
            fields(&[("p1", 10, 10)]),
            fields(&[("p2", 10, 200)])
        );
        let (mut page, _) = open(
            &parent,
            &[
                ("/case/e.html", html("<!doctype html><p>nothing here</p>")),
                (
                    "/case/f.html",
                    html(&format!("<!doctype html>{}", fields(&[("d1", 10, 10)]))),
                ),
            ],
            false,
        );
        settle(&mut page);
        let empty = frame(&page, "empty");
        let pos = frame(&page, "pos");
        let state = |page: &mut Page| {
            let top = page.frame_document(FrameId::TOP).unwrap();
            let a = top.focus_element().or(top.focused_frame_element());
            let id = a
                .and_then(|n| top.dom().element(n))
                .and_then(|e| e.attr("id"))
                .unwrap_or("BODY")
                .to_owned();
            let inner = page.frame_document(pos).unwrap().focus_element().is_some();
            (id, inner, page.focused_frame_id() == empty)
        };
        page.focus(Some(node(&page, "p1")), true);
        // Measured (case `ex-tab-kinds`): p1, empty, p2, out, pos/d1, p1.
        let mut seen = Vec::new();
        for _ in 0..5 {
            key(&mut page, &Key::Tab);
            seen.push(state(&mut page));
        }
        assert_eq!(
            seen,
            vec![
                ("empty".to_owned(), false, true),
                ("p2".to_owned(), false, false),
                ("BODY".to_owned(), false, false),
                ("pos".to_owned(), true, false),
                ("p1".to_owned(), false, false),
            ]
        );
    });
}

#[test]
fn tab_enters_nested_frames() {
    on_page_thread(|| {
        let parent = format!(
            "<!doctype html><style>body{{margin:0}}</style>{}<iframe id=f src=n.html style='position:absolute;left:10px;top:50px;width:400px;height:200px;border:0'></iframe>{}",
            fields(&[("p1", 10, 10)]),
            fields(&[("p2", 10, 300)])
        );
        let n = format!(
            "<!doctype html><style>body{{margin:0}}</style>{}<iframe id=g src=g.html style='position:absolute;left:10px;top:50px;width:300px;height:100px;border:0'></iframe>{}",
            fields(&[("n1", 10, 10)]),
            fields(&[("n2", 10, 160)])
        );
        let g = format!(
            "<!doctype html>{}",
            fields(&[("m1", 10, 10), ("m2", 100, 10)])
        );
        let (mut page, _) = open(
            &parent,
            &[("/case/n.html", html(&n)), ("/case/g.html", html(&g))],
            false,
        );
        settle(&mut page);
        let f = frame(&page, "f");
        let gf = page.frame_of(f, frame_node(&page, f, "g")).unwrap();
        let name = |page: &Page, frame: FrameId| {
            let doc = page.frame_document(frame).unwrap();
            doc.focus_element()
                .and_then(|n| doc.dom().element(n))
                .and_then(|e| e.attr("id").map(str::to_owned))
                .unwrap_or_else(|| "-".to_owned())
        };
        let state = |page: &Page| (name(page, FrameId::TOP), name(page, f), name(page, gf));
        page.focus(Some(node(&page, "p1")), true);
        let mut seen = Vec::new();
        for _ in 0..6 {
            key(&mut page, &Key::Tab);
            seen.push(state(&page));
        }
        let s = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
        // Measured (case `ex-tab-nested`).
        assert_eq!(
            seen,
            vec![
                s("-", "n1", "-"),
                s("-", "-", "m1"),
                s("-", "-", "m2"),
                s("-", "n2", "-"),
                s("p2", "-", "-"),
                s("-", "-", "-"),
            ]
        );
        page.focus(Some(node(&page, "p2")), true);
        let mut seen = Vec::new();
        for _ in 0..6 {
            page.key_down(&Key::Tab, SHIFT);
            seen.push(state(&page));
        }
        assert_eq!(
            seen,
            vec![
                s("-", "n2", "-"),
                s("-", "-", "m2"),
                s("-", "-", "m1"),
                s("-", "n1", "-"),
                s("p1", "-", "-"),
                s("-", "-", "-"),
            ]
        );
    });
}

#[test]
fn a_click_into_a_frame_focuses_its_element_and_the_iframe() {
    on_page_thread(|| {
        let mut page = tab_page();
        let f = frame(&page, "f");
        let iframe = node(&page, "f");
        page.click(30.0, 20.0);
        assert_eq!(page.focused_element(), Some(node(&page, "p1")));
        // A click on an input of the frame.
        page.click(30.0, 70.0);
        assert_eq!(page.focused_element(), None);
        assert_eq!(page.focused_frame_id(), f);
        assert_eq!(
            page.document_state().unwrap().focused_frame_element(),
            Some(iframe)
        );
        assert_eq!(
            page.frame_document(f).unwrap().focus_element(),
            Some(frame_node(&page, f, "c1"))
        );
        // Typing goes to the frame's field.
        for c in ['a', 'b'] {
            page.insert_text(&c.to_string());
        }
        let c1 = frame_node(&page, f, "c1");
        assert_eq!(
            page.frame_document(f).unwrap().control_value(c1).as_deref(),
            Some("ab")
        );
        // A click on the parent's field takes the focus back.
        page.click(30.0, 20.0);
        assert_eq!(page.focused_frame_id(), FrameId::TOP);
        assert_eq!(page.focused_element(), Some(node(&page, "p1")));
        assert_eq!(page.frame_document(f).unwrap().focus_element(), None);
        assert_eq!(page.document_state().unwrap().focused_frame_element(), None);
    });
}

#[test]
fn the_focus_stays_with_a_frame_that_navigates_and_falls_back_when_it_is_removed() {
    on_page_thread(|| {
        let mut page = tab_page();
        let f = frame(&page, "f");
        page.click(30.0, 70.0);
        assert_eq!(page.focused_frame_id(), f);
        // Navigating the frame: its new document has no focused element, the
        // frame keeps the focus (measured, case `ex-focus-navigate`).
        page.change_dom(|d| {
            let iframe = d.element_by_id("f").unwrap();
            d.set_attr(iframe, "src", "g.html").unwrap();
        });
        settle(&mut page);
        assert_eq!(page.focused_frame_id(), f);
        assert_eq!(page.frame_document(f).unwrap().focus_element(), None);
        // Removing it: the page's document has the focus.
        page.change_dom(|d| {
            let iframe = d.element_by_id("f").unwrap();
            d.remove(iframe);
        });
        assert_eq!(page.focused_frame_id(), FrameId::TOP);
        assert_eq!(page.document_state().unwrap().focused_frame_element(), None);
        key(&mut page, &Key::Tab);
        assert!(page.focused_element().is_some());
    });
}

// ----- Keys in a frame -----

const FORM: &str = "<!doctype html><style>body{margin:0}*{position:absolute}</style>\
    <form action=r.html method=get>\
    <input id=q name=q style='left:10px;top:10px;width:100px'>\
    <input id=c type=checkbox name=c checked style='left:10px;top:50px'>\
    <select id=s name=s style='left:10px;top:80px'><option value=a>a<option value=b selected>b<option value=c>c</select>\
    <input id=go type=submit name=go value=go style='left:10px;top:120px;width:60px;height:25px'>\
    </form>";

fn form_page() -> Page {
    let parent = "<!doctype html><style>body{margin:0}</style><iframe id=f src=form.html style='position:absolute;left:100px;top:100px;width:300px;height:200px;border:0'></iframe>";
    let (mut page, _) = open(
        parent,
        &[
            ("/case/form.html", html(FORM)),
            (
                "/case/r.html",
                html("<!doctype html><title>r</title><p>result"),
            ),
        ],
        false,
    );
    settle(&mut page);
    page
}

fn query_of(page: &Page, f: FrameId) -> String {
    page.frame_document(f)
        .unwrap()
        .url()
        .query()
        .unwrap_or("")
        .to_owned()
}

#[test]
fn typing_and_enter_in_a_frame_field_submit_the_form_into_the_frame() {
    on_page_thread(|| {
        let mut page = form_page();
        let f = frame(&page, "f");
        page.click(130.0, 120.0);
        for c in "hi".chars() {
            page.insert_text(&c.to_string());
        }
        let q = frame_node(&page, f, "q");
        assert_eq!(
            page.frame_document(f).unwrap().control_value(q).as_deref(),
            Some("hi")
        );
        key(&mut page, &Key::Enter);
        settle(&mut page);
        assert_eq!(page.frame_document(f).unwrap().url().path(), "/case/r.html");
        assert_eq!(query_of(&page, f), "q=hi&c=on&s=b&go=go");
        assert_eq!(page.url().unwrap().path(), "/case/case.html");
    });
}

#[test]
fn a_checkbox_and_a_select_in_a_frame_work_with_mouse_and_keys() {
    on_page_thread(|| {
        let mut page = form_page();
        let f = frame(&page, "f");
        let c = frame_node(&page, f, "c");
        let s = frame_node(&page, f, "s");
        let checked = |page: &Page| page.frame_document(f).unwrap().control_checked(c);
        page.click(115.0, 155.0);
        assert_eq!(checked(&page), Some(false));
        key(&mut page, &space());
        page.key_up(&space(), Modifiers::NONE);
        assert_eq!(checked(&page), Some(true));
        // Tab to the select, ArrowDown picks the next option.
        key(&mut page, &Key::Tab);
        assert_eq!(page.frame_document(f).unwrap().focus_element(), Some(s));
        key(&mut page, &Key::ArrowDown);
        assert_eq!(
            page.frame_document(f).unwrap().control_value(s).as_deref(),
            Some("c")
        );
        // Tab to the button and press Space: the form goes into the frame.
        key(&mut page, &Key::Tab);
        key(&mut page, &space());
        page.key_up(&space(), Modifiers::NONE);
        settle(&mut page);
        assert_eq!(query_of(&page, f), "q=&c=on&s=c&go=go");
    });
}

#[test]
fn a_click_on_a_submit_button_in_a_frame_navigates_the_frame() {
    on_page_thread(|| {
        let mut page = form_page();
        let f = frame(&page, "f");
        page.click(130.0, 120.0);
        page.insert_text("x y");
        page.click(130.0, 235.0);
        settle(&mut page);
        assert_eq!(query_of(&page, f), "q=x+y&c=on&s=b&go=go");
    });
}

// ----- Selection -----

const TEXT: &str = "<!doctype html><style>body{margin:0;font:16px/20px sans-serif}p{margin:0 0 0 10px;width:280px}</style>\
    <p id=p1>alpha beta gamma</p><p id=p2>delta epsilon zeta</p>\
    <input id=i style='position:absolute;left:10px;top:80px;width:150px' value='field text here'>";

fn text_page() -> Page {
    let parent = "<!doctype html><style>body{margin:0;font:16px/20px sans-serif}</style>\
        <p id=pp style='position:absolute;left:10px;top:250px;margin:0'>parent text words</p>\
        <iframe id=f src=sel.html style='position:absolute;left:100px;top:100px;width:300px;height:150px;border:0'></iframe>";
    let (mut page, _) = open(parent, &[("/case/sel.html", html(TEXT))], false);
    settle(&mut page);
    page
}

#[test]
fn a_double_click_selects_a_word_in_the_frame() {
    on_page_thread(|| {
        let mut page = text_page();
        let f = frame(&page, "f");
        page.mouse_move(130.0, 110.0);
        page.mouse_down(130.0, 110.0, MouseButton::Primary, Modifiers::NONE, 2);
        page.mouse_up(130.0, 110.0, MouseButton::Primary);
        assert_eq!(page.frame_selected_text(f), "alpha");
        assert_eq!(page.selected_text(), "alpha");
        page.mouse_down(130.0, 130.0, MouseButton::Primary, Modifiers::NONE, 3);
        page.mouse_up(130.0, 130.0, MouseButton::Primary);
        assert_eq!(page.selected_text(), "delta epsilon zeta");
        assert!(page.selection().is_none());
    });
}

#[test]
fn a_drag_selection_stays_in_the_frame() {
    on_page_thread(|| {
        let mut page = text_page();
        let f = frame(&page, "f");
        page.mouse_move(115.0, 110.0);
        page.mouse_down(115.0, 110.0, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_move(150.0, 130.0);
        assert_eq!(page.frame_selected_text(f), "lpha beta gamma\n\ndelta");
        page.mouse_move(200.0, 112.0);
        assert_eq!(page.frame_selected_text(f), "lpha beta g");
        // A point outside the frame (below it) selects to the end of the
        // text, as for a document (measured: case `input-select-drag`).
        page.mouse_move(300.0, 400.0);
        let all = "lpha beta gamma\n\ndelta epsilon zeta";
        assert_eq!(page.frame_selected_text(f), all);
        page.mouse_up(300.0, 400.0, MouseButton::Primary);
        assert_eq!(page.frame_selected_text(f), all);
        assert!(page.selection().is_none());
    });
}

#[test]
fn select_all_and_copy_use_the_focused_document() {
    on_page_thread(|| {
        let mut page = text_page();
        let f = frame(&page, "f");
        // The page's text.
        page.click(20.0, 260.0);
        page.key_down(&ch('a'), CTRL);
        assert_eq!(page.selected_text(), "parent text words");
        assert_eq!(page.frame_selected_text(f), "");
        // The frame's: click blank, select all.
        page.click(250.0, 175.0);
        page.key_down(&ch('a'), CTRL);
        assert_eq!(
            page.selected_text(),
            "alpha beta gamma\n\ndelta epsilon zeta"
        );
        // The field in the frame: its own selection.
        page.click(150.0, 190.0);
        page.key_down(&ch('a'), CTRL);
        assert_eq!(page.selected_text(), "field text here");
        assert!(page.has_editable_focus());
    });
}

// ----- Robustness -----

#[test]
fn a_thousand_frames_over_each_other_take_input() {
    on_page_thread(|| {
        let mut body = String::from(
            "<!doctype html><style>body{margin:0}iframe{position:absolute;left:0;top:0;width:200px;height:100px;border:0}</style>",
        );
        for i in 0..1000 {
            let _ = write!(body, "<iframe srcdoc=\"<input id=i{i}>\"></iframe>");
        }
        let (mut page, _) = open(&body, &[], false);
        settle(&mut page);
        let start = std::time::Instant::now();
        for i in 0..100 {
            page.mouse_move(10.0 + i as f32, 20.0);
        }
        page.click(20.0, 20.0);
        page.wheel(20.0, 20.0, 0.0, 10.0);
        page.key_down(&Key::Tab, Modifiers::NONE);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        let hit = page.hit_test(20.0, 20.0).unwrap();
        assert_ne!(hit.frame, FrameId::TOP);
    });
}

// ----- More: lists, origins, POST, removal -----

#[test]
fn a_scroll_of_a_frame_builds_no_new_list_for_the_parent() {
    on_page_thread(|| {
        let mut page = tall_pair();
        let f = frame(&page, "f");
        page.screenshot(false).unwrap();
        let runs = |page: &Page, id: FrameId| {
            page.frame_document(id)
                .unwrap()
                .flush_counters()
                .display_list_runs
        };
        let (parent_before, frame_before) = (runs(&page, FrameId::TOP), runs(&page, f));
        page.wheel(200.0, 200.0, 0.0, 100.0);
        page.screenshot(false).unwrap();
        assert_eq!(page.frame_document(f).unwrap().scroll_position().y, 100.0);
        assert_eq!(runs(&page, FrameId::TOP), parent_before);
        assert_eq!(runs(&page, f), frame_before);
    });
}

#[test]
fn a_cross_origin_frame_does_not_navigate_the_top_level() {
    on_page_thread(|| {
        let parent = "<!doctype html><style>body{margin:0}</style>\
            <iframe id=f src='http://127.0.0.1:8001/case/l.html' style='position:absolute;left:100px;top:100px;width:300px;height:200px;border:0'></iframe>";
        let links = "<!doctype html><style>body{margin:0}a{display:block;width:200px;height:20px}</style>\
            <a id=top target=_top href='http://127.0.0.1:8000/case/t.html'>top</a>";
        let (mut page, _) = open(
            parent,
            &[
                ("/case/l.html", html(links)),
                ("/case/t.html", html("<!doctype html><p>t")),
            ],
            false,
        );
        settle(&mut page);
        page.click(110.0, 110.0);
        settle(&mut page);
        // Chromium navigates after the user's click; swb does not (ADR 0031
        // section 3) and logs it.
        assert_eq!(page.url().unwrap().path(), "/case/case.html");
    });
}

#[test]
fn a_post_form_in_a_frame_posts_into_the_frame() {
    on_page_thread(|| {
        let form = "<!doctype html><style>body{margin:0}</style>\
            <form method=post action=r.html><input name=a value=1><input type=submit id=go value=go style='width:80px;height:30px'></form>";
        let (mut page, sim) = open(
            PARENT,
            &[
                ("/case/f.html", html(form)),
                ("/case/r.html", html("<!doctype html><p>posted")),
            ],
            false,
        );
        settle(&mut page);
        let f = frame(&page, "f");
        let go = frame_node(&page, f, "go");
        let b = page
            .frame_document(f)
            .unwrap()
            .fragments()
            .unwrap()
            .element_boxes()[&go];
        page.click(100.0 + b.x + 5.0, 100.0 + b.y + 5.0);
        settle(&mut page);
        assert_eq!(page.frame_document(f).unwrap().url().path(), "/case/r.html");
        let posted = sim
            .requests_full()
            .into_iter()
            .find(|r| r.method == swb_net::Method::Post)
            .expect("a POST request");
        assert_eq!(posted.destination, swb_net::Destination::IFrame);
        assert_eq!(posted.body.as_deref(), Some(&b"a=1"[..]));
    });
}

#[test]
fn a_press_in_a_frame_that_goes_away_ends_without_a_panic() {
    on_page_thread(|| {
        let mut page = parent_with("<!doctype html><p>text</p>", &[], false);
        page.mouse_move(120.0, 110.0);
        page.mouse_down(120.0, 110.0, MouseButton::Primary, Modifiers::NONE, 1);
        page.change_dom(|d| {
            let iframe = d.element_by_id("f").unwrap();
            d.remove(iframe);
        });
        page.mouse_move(130.0, 112.0);
        page.mouse_up(130.0, 112.0, MouseButton::Primary);
        key(&mut page, &Key::Tab);
        assert_eq!(page.focused_frame_id(), FrameId::TOP);
    });
}

// ----- Lazy images in frames (ADR 0032) -----

#[test]
fn a_lazy_image_in_a_frame_loads_when_the_frame_scrolls_near_it() {
    on_page_thread(|| {
        let frame_doc = "<!doctype html><style>body{margin:0}</style><div style='height:5000px'></div>\
                         <img src=lazy.png loading=lazy width=10 height=10>";
        let (mut page, sim) = open(PARENT, &[("/case/f.html", html(frame_doc))], true);
        settle(&mut page);
        let asked = |sim: &Sim| sim.requests().iter().any(|(u, _)| u.ends_with("/lazy.png"));
        assert!(
            !asked(&sim),
            "the image is 4,800 px below the frame's viewport"
        );
        page.wheel(200.0, 200.0, 0.0, 2500.0);
        settle(&mut page);
        assert!(
            asked(&sim),
            "the frame scrolled to within 3,000 px of the image"
        );
    });
}
