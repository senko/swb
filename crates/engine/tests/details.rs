//! `details` and `summary`: opening and closing with a click and the
//! keyboard, the hidden contents of a closed `details` (hit testing, focus,
//! the selection), and the default summary. Pages come from an in-memory
//! fetcher; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Key, Modifiers, Page, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

mod common;
use common::{node, rect};

/// Serves one page for every URL and records the requested URLs.
struct Site {
    html: String,
    requests: Mutex<Vec<String>>,
}

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.url.as_str().to_owned());
        let headers: Headers = [("content-type", "text/html; charset=utf-8")]
            .into_iter()
            .collect();
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: self.html.clone().into_bytes(),
            redirected: false,
        })
    }
}

const BODY: &str = "<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>\
                    <style>summary{display:block}</style>";

fn open(html: &str) -> (Page, Arc<Site>) {
    let site = Arc::new(Site {
        html: format!("{BODY}{html}"),
        requests: Mutex::new(Vec::new()),
    });
    let fetcher: Arc<dyn Fetcher> = Arc::clone(&site) as Arc<dyn Fetcher>;
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(Url::parse("https://site.test/").unwrap());
    common::finish_loading(&mut page, Duration::from_secs(20));
    (page, site)
}

fn click(page: &mut Page, id: &str) -> bool {
    let r = rect(page, id);
    page.click(r.x + 2.0, r.y + r.height / 2.0)
}

fn key(page: &mut Page, key: &Key) -> bool {
    page.key_down(key, Modifiers::NONE)
}

fn is_open(page: &Page, id: &str) -> bool {
    page.document()
        .unwrap()
        .element(node(page, id))
        .unwrap()
        .has_attr("open")
}

const SIMPLE: &str = "<details id=d><summary id=s>Sum</summary><p id=p>para</p></details>\
                      <div id=after>after</div>";

#[test]
fn a_click_on_the_summary_toggles_details() {
    let (mut page, _) = open(SIMPLE);
    assert!(!is_open(&page, "d"));
    let closed = rect(&mut page, "after").y;
    assert_eq!(closed, 20.0);
    assert!(click(&mut page, "s"));
    assert!(is_open(&page, "d"));
    // The paragraph has 16 px margins; the last one collapses through.
    assert_eq!(rect(&mut page, "p").y, 20.0 + 16.0);
    assert_eq!(rect(&mut page, "after").y, 20.0 + 16.0 + 20.0 + 16.0);
    assert!(click(&mut page, "s"));
    assert!(!is_open(&page, "d"));
    assert_eq!(rect(&mut page, "after").y, closed);
}

#[test]
fn the_open_state_matches_open_and_the_marker_changes() {
    let (mut page, _) = open(
        "<style>summary{display:list-item} details:open #t{color:rgb(1,2,3)} details[open] #u{color:rgb(4,5,6)}</style>\
         <details id=d><summary id=s>Sum</summary><b id=t>t</b><b id=u>u</b></details>",
    );
    let colors = |page: &mut Page| {
        page.update_layout();
        let styles = page.styles().unwrap();
        (
            styles.get(node(page, "t")).unwrap().color,
            styles.get(node(page, "u")).unwrap().color,
        )
    };
    let marker = |page: &mut Page| {
        page.update_layout();
        let mut text = String::new();
        page.fragments().unwrap().walk(|f, _| {
            if let swb_layout::FragmentRef::Text(t) = f
                && t.text.contains(['\u{25B8}', '\u{25BE}'])
            {
                text.push_str(&t.text);
            }
        });
        text
    };
    assert_eq!(marker(&mut page), "\u{25B8} ");
    let closed = colors(&mut page);
    click(&mut page, "s");
    assert_eq!(marker(&mut page), "\u{25BE} ");
    let opened = colors(&mut page);
    assert_ne!(closed.0, opened.0);
    assert_ne!(closed.1, opened.1);
    assert_eq!(opened.0, swb_style::Rgba::new(1, 2, 3, 255));
    assert_eq!(opened.1, swb_style::Rgba::new(4, 5, 6, 255));
}

#[test]
fn enter_and_space_on_the_focused_summary_toggle_details() {
    let (mut page, _) = open(SIMPLE);
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "s")));
    assert!(key(&mut page, &Key::Enter));
    assert!(is_open(&page, "d"));
    assert!(key(&mut page, &Key::Character(" ".into())));
    assert!(!is_open(&page, "d"));
    // Other keys, and keys with Ctrl, do nothing.
    page.key_down(&Key::Enter, common::CTRL);
    assert!(!is_open(&page, "d"));
}

#[test]
fn only_the_first_summary_toggles_details() {
    let (mut page, _) = open(
        "<details id=d><p id=p>x</p><summary id=s>one</summary><summary id=s2>two</summary>\
         </details><div id=after>after</div>",
    );
    click(&mut page, "s2");
    assert!(!is_open(&page, "d"));
    click(&mut page, "s");
    assert!(is_open(&page, "d"));
}

#[test]
fn a_click_on_a_link_or_a_field_in_the_summary_does_not_toggle() {
    let (mut page, site) = open(
        "<details id=d><summary id=s><a id=a href='/go'>link</a> \
         <input id=i style='width:100px'></summary>body</details>",
    );
    click(&mut page, "i");
    assert!(!is_open(&page, "d"));
    click(&mut page, "a");
    common::finish_loading(&mut page, Duration::from_secs(20));
    assert!(!is_open(&page, "d"));
    let requests = site.requests.lock().unwrap().clone();
    assert_eq!(requests.last().unwrap(), "https://site.test/go");
}

#[test]
fn closed_contents_are_not_hit_focused_or_selected() {
    let (mut page, _) = open(
        "<details id=d><summary id=s>Sum</summary>\
         <a id=a href='/x' style='display:block'>link text</a></details>\
         <button id=b>button</button><p>visible text</p>",
    );
    // The link has a box (laid out), at the position of the content.
    let a = rect(&mut page, "a");
    assert_eq!(a.y, 20.0);
    // The hit test finds the button there, not the link; Tab skips it.
    let hit = page.hit_test(a.x + 5.0, a.y + 5.0).map(|h| h.node);
    assert_ne!(hit, Some(node(&page, "a")));
    key(&mut page, &Key::Tab);
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "b")));
    // The selected text has only the visible text.
    page.select_all();
    let text = page.selected_text();
    assert!(
        text.contains("Sum") && text.contains("visible text"),
        "{text:?}"
    );
    assert!(!text.contains("link text"), "{text:?}");
    // Open: the link is found and reached with Tab.
    click(&mut page, "s");
    let hit = page.hit_test(a.x + 5.0, a.y + 5.0).map(|h| h.node);
    assert_eq!(
        hit.and_then(|n| page.document().unwrap().parent(n)),
        Some(node(&page, "a"))
    );
    let (mut page, _) = open(
        "<details id=d open><summary id=s>Sum</summary>\
         <a id=a href='/x' style='display:block'>link text</a></details>",
    );
    key(&mut page, &Key::Tab);
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "a")));
}

#[test]
fn clicking_the_summary_moves_the_focus_out_of_the_contents() {
    let (mut page, _) = open(
        "<details id=d open><summary id=s>Sum</summary>\
         <input id=i style='display:block'></details>",
    );
    click(&mut page, "i");
    assert_eq!(page.focused_element(), Some(node(&page, "i")));
    click(&mut page, "s");
    assert!(!is_open(&page, "d"));
    assert_eq!(page.focused_element(), Some(node(&page, "s")));
}

#[test]
fn closing_details_removes_a_selection_in_its_contents() {
    let (mut page, _) =
        open("<details id=d open><summary id=s>Sum</summary><p>inside</p></details>");
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "s")));
    page.select_all();
    assert!(page.selection().is_some());
    assert!(key(&mut page, &Key::Enter));
    assert!(!is_open(&page, "d"));
    assert!(page.selection().is_none());
}

#[test]
fn hidden_contents_do_not_extend_the_scrollable_area() {
    let (mut page, _) = open(
        "<details id=d><summary id=s>Sum</summary><div style='height:5000px'>tall</div></details>",
    );
    page.update_layout();
    let size = page.fragments().unwrap().scroll_size;
    assert_eq!(size.height, 600.0);
    click(&mut page, "s");
    page.update_layout();
    assert!(page.fragments().unwrap().scroll_size.height > 5000.0);
}

#[test]
fn the_default_summary_toggles_details() {
    let (mut page, _) = open(
        "<details id=d><p id=p>para</p></details><div id=after>after</div>\
         <details id=e open><p id=q style='margin:0;height:100px'>x</p></details>",
    );
    // The default summary is one line high.
    assert_eq!(rect(&mut page, "after").y, 20.0);
    assert!(page.click(10.0, 10.0));
    assert!(is_open(&page, "d"));
    // The paragraph is in the content, below the summary.
    assert_eq!(rect(&mut page, "p").y, 20.0 + 16.0);
    // A click on the content of an open `details` without summary does
    // not toggle it.
    let q = rect(&mut page, "q");
    page.click(q.x + 5.0, q.y + 50.0);
    assert!(is_open(&page, "e"));
    // A click on its default summary closes it.
    let e = rect(&mut page, "e");
    page.click(e.x + 5.0, e.y + 5.0);
    assert!(!is_open(&page, "e"));
}

#[test]
fn nested_details_open_independently() {
    let (mut page, _) = open(
        "<details id=d open><summary id=s>one</summary>\
         <details id=d2><summary id=s2>two</summary><p id=p>deep</p></details></details>",
    );
    assert_eq!(rect(&mut page, "s2").y, 20.0);
    click(&mut page, "s2");
    assert!(is_open(&page, "d2") && is_open(&page, "d"));
    click(&mut page, "s");
    assert!(!is_open(&page, "d") && is_open(&page, "d2"));
}
