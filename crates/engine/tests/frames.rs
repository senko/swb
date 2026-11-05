//! Frames (ADR 0031 sections 2 to 4 and 11, M9 feature 10a): the frame
//! tree, the loading of frame documents, the load state, the `iframe` box
//! and the frame viewports; `frames/paint.rs` (feature 10b): what frames
//! paint.
//!
//! The cases of `tools/probes/web-frames.json` that feature 10a covers run
//! here with Chromium's values (`tests/data/web-frames.json`: `just probe
//! --json tools/probes/web-frames.json` with the cases `geometry`,
//! `about-blank`, `load-order`, `cookies-frames`, `javascript-off`,
//! `limits`, `self-embed` and `sandbox`; three equal runs apart from the
//! server ports). The tests here read frame documents through the engine API
//! (`Page::frame_document`) where JavaScript is off; `frames/scripted.rs` runs
//! the queries of the probe with scripts in frames (feature 12b1), and
//! `QUERIES_LATER` lists what waits for later features.
//!
//! `Sim` serves the cases as the probe server does: the case page and its
//! resources under `/case/`, on the main origin (`127.0.0.1:8000`), a
//! second port (`127.0.0.1:8001`, same site) and `localhost:8001` (cross
//! site), with a delay per resource and a cookie jar.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use swb_engine::{
    DEFAULT_FRAME_VIEWPORT, FrameId, MAX_DEPTH, MAX_FRAMES, NodeId, Page, Rect, Size, Url,
};
use swb_net::{CookieJar, Fetcher, Headers, NetError, Request, Response};

mod common;
#[path = "frames/input.rs"]
mod input;
#[path = "frames/paint.rs"]
mod paint;
#[path = "frames/scripted.rs"]
mod scripted;
#[path = "frames/window_proxy.rs"]
mod window_proxy;
use common::{finish_loading, new_page, on_page_thread, value};

const MAIN: &str = "http://127.0.0.1:8000";
const OTHER: &str = "http://127.0.0.1:8001";
const LOCAL: &str = "http://localhost:8001";

/// The queries of the covered cases that need scripts in frames or other
/// later features, with the feature. The input cases (`focus`, `input-*`)
/// run in `crates/automation/tests/frame_input.rs`; their rows here are the
/// queries that read the log of the frame's own scripts.
const QUERIES_LATER: &[(&str, &str)] = &[
    (
        "about-blank",
        "document.open, write and close on a frame document: feature 15",
    ),
    (
        "cookies-frames",
        "document.cookie, fetch() and postMessage in frames: features 13 and 14",
    ),
    (
        "input-pointer-events",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-press-in-frame",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-press-in-parent",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-click-in-frame",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-wheel-events",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-pointer-events-none",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-visibility-hidden",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-covered",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-opacity-0",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-transform",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-hit-border",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-tab-forward",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "input-tab-backward",
        "what the frame's own scripts logged (the `c:` entries: its pointer, wheel, focus and blur events): feature 12b2; the page's side of the case runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "focus",
        "the frame window's and the frame element's focus and blur events in the log (`c:` entries): feature 12b2; the page's side runs in `crates/automation/tests/frame_input.rs`",
    ),
    (
        "sandbox",
        "the script of a frame with an opaque origin, postMessage, the message of a SecurityError: feature 13",
    ),
];

// ----- The simulated server -----

/// A resource: body, content type, extra headers, delay.
#[derive(Clone)]
struct Res {
    body: Vec<u8>,
    content_type: String,
    headers: Vec<(String, String)>,
    delay_ms: u64,
}

impl Res {
    fn html(body: &str) -> Res {
        Res {
            body: body.as_bytes().to_vec(),
            content_type: "text/html; charset=utf-8".to_owned(),
            headers: Vec::new(),
            delay_ms: 0,
        }
    }
}

/// Serves the resources by path on all three origins, with a cookie jar;
/// records each request with the `Cookie` header that it carried.
#[derive(Default)]
struct Sim {
    resources: HashMap<String, Res>,
    jar: CookieJar,
    log: Mutex<Vec<(String, Option<String>, Request)>>,
}

impl Sim {
    fn new(resources: &[(&str, Res)]) -> Arc<Sim> {
        Arc::new(Sim {
            resources: resources
                .iter()
                .map(|(p, r)| ((*p).to_owned(), r.clone()))
                .collect(),
            ..Sim::default()
        })
    }

    /// The requests so far: URL, `Cookie` header.
    fn requests(&self) -> Vec<(String, Option<String>)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(u, c, _)| (u.clone(), c.clone()))
            .collect()
    }

    fn requests_full(&self) -> Vec<Request> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, r)| r.clone())
            .collect()
    }
}

impl Fetcher for Sim {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let cookie = self.jar.cookie_header(request);
        self.log
            .lock()
            .unwrap()
            .push((request.url.to_string(), cookie, request.clone()));
        let Some(res) = self.resources.get(request.url.path()) else {
            return Ok(Response {
                url: request.url.clone(),
                status: 404,
                headers: [("content-type", "text/plain")].into_iter().collect(),
                body: b"not found".to_vec(),
                redirected: false,
            });
        };
        if res.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(res.delay_ms));
        }
        let mut headers: Headers = [("content-type", res.content_type.as_str())]
            .into_iter()
            .collect();
        for (name, value) in &res.headers {
            headers.append(name, value);
        }
        self.jar.store_response_cookies(request, &headers);
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: res.body.clone(),
            redirected: false,
        })
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        Some(&self.jar)
    }
}

// ----- Chromium's values and the probe's cases -----

fn probe_file() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tools/probes/web-frames.json")).unwrap()
}

fn chromium() -> serde_json::Value {
    serde_json::from_str(include_str!("data/web-frames.json")).unwrap()
}

/// The markup of a case of the probe file.
fn case_html(name: &str) -> String {
    let probe = probe_file();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    case["html"].as_str().unwrap().to_owned()
}

/// A resource of a case of the probe file.
fn case_resource(name: &str, path: &str) -> Res {
    let probe = probe_file();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    let r = &case["resources"][path];
    let mut res = Res::html(&r["text"].as_str().unwrap().replace("$FOLDER", "case"));
    res.delay_ms = r["delay_ms"].as_u64().unwrap_or(0);
    if let Some(headers) = r["headers"].as_object() {
        for (name, value) in headers {
            let values: Vec<String> = match value {
                serde_json::Value::Array(a) => a
                    .iter()
                    .map(|v| v.as_str().unwrap().replace("$FOLDER", "case"))
                    .collect(),
                v => vec![v.as_str().unwrap().to_owned()],
            };
            for v in values {
                if name.eq_ignore_ascii_case("content-type") {
                    res.content_type = v;
                } else {
                    res.headers.push((name.clone(), v));
                }
            }
        }
    }
    res
}

/// The results of a case in Chromium's output.
fn results(name: &str) -> Vec<serde_json::Value> {
    let data = chromium();
    data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no Chromium values for {name}"))["results"]
        .as_array()
        .unwrap()
        .clone()
}

/// The value of the `index`th `js` result of a case.
fn js_value(name: &str, index: usize) -> serde_json::Value {
    results(name)
        .into_iter()
        .filter(|r| r["kind"] == "js")
        .nth(index)
        .unwrap()["value"]
        .clone()
}

/// The `boxes` results of a case: (tag, rect).
fn chromium_boxes(name: &str) -> Vec<(String, Rect)> {
    results(name)
        .into_iter()
        .filter(|r| r["kind"] == "boxes")
        .map(|r| {
            let v: Vec<f32> = r["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap() as f32)
                .collect();
            (
                r["tag"].as_str().unwrap().to_owned(),
                Rect::new(v[0], v[1], v[2], v[3]),
            )
        })
        .collect()
}

// ----- Pages -----

/// Opens `MAIN/case/case.html` with `html` and `resources` (paths under
/// `/case/`), JavaScript as given, and waits until it loaded.
fn open(html: &str, resources: &[(&str, Res)], javascript: bool) -> (Page, Arc<Sim>) {
    let mut all: Vec<(&str, Res)> = vec![("/case/case.html", Res::html(html))];
    all.extend(resources.iter().cloned());
    let sim = Sim::new(&all);
    let mut page = new_page(sim.clone(), 4, Size::new(800.0, 600.0));
    page.set_javascript(javascript);
    page.navigate(url(&format!("{MAIN}/case/case.html")));
    finish_loading(&mut page, Duration::from_secs(20));
    (page, sim)
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

/// A new detached `iframe` element.
fn new_iframe(dom: &mut swb_dom::Document) -> NodeId {
    let name = swb_dom::QualName::new(None, swb_dom::ns!(html), "iframe".into());
    dom.create_element(name, Vec::new()).unwrap()
}

/// The child frames of the top-level frame.
fn top_children(page: &Page) -> Vec<FrameId> {
    page.frame_children(FrameId::TOP)
}

/// The `iframe` elements of the page's document, in tree order.
fn iframes(page: &Page) -> Vec<NodeId> {
    page.query_selector_all("iframe").unwrap()
}

/// The frame of the `iframe` with ID `id` in the page's document.
fn frame(page: &Page, id: &str) -> FrameId {
    let node = common::node(page, id);
    page.frame_of(FrameId::TOP, node)
        .unwrap_or_else(|| panic!("#{id} has no frame"))
}

fn frame_url(page: &Page, frame: FrameId) -> String {
    page.frame_document(frame).unwrap().url().to_string()
}

fn close(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() <= 1.0
        && (a.y - b.y).abs() <= 1.0
        && (a.width - b.width).abs() <= 1.0
        && (a.height - b.height).abs() <= 1.0
}

// ----- Cases of the probe -----

/// `geometry`: the `iframe` boxes (304×154 with the UA border, the
/// attributes and the CSS sizes) and the frame viewports, quirks modes and
/// URLs.
#[test]
fn geometry_matches_chromium() {
    let (mut page, _) = open(&case_html("geometry"), &[], false);
    page.update_layout();
    let want = chromium_boxes("geometry");
    let mut got = vec![("div".to_owned(), common::rect(&mut page, "r1"))];
    for node in iframes(&page) {
        got.push(("iframe".to_owned(), page.element_box(node).unwrap()));
    }
    assert_eq!(want.len(), got.len());
    for ((wt, w), (gt, g)) in want.iter().zip(&got) {
        assert_eq!(wt, gt);
        assert!(close(*w, *g), "{wt}: Chromium {w:?}, swb {g:?}");
    }
    // [id, innerWidth, innerHeight, clientWidth, clientHeight, compatMode,
    //  display, border style, width, color, overflow, vertical-align,
    //  body margin, scrollHeight, URL]
    let rows = js_value("geometry", 0);
    for row in rows.as_array().unwrap() {
        let f: Vec<&str> = row.as_str().unwrap().split(' ').collect();
        let id = frame(&page, f[0]);
        let viewport = page.frame_viewport(id).unwrap();
        assert_eq!(viewport.width.to_string(), f[1], "{} width", f[0]);
        assert_eq!(viewport.height.to_string(), f[2], "{} height", f[0]);
        let doc = page.frame_document(id).unwrap();
        let quirks = doc.dom().quirks_mode == swb_dom::QuirksMode::Quirks;
        assert_eq!(quirks, f[5] == "BackCompat", "{} mode", f[0]);
        assert_eq!(doc.url().as_str(), *f.last().unwrap(), "{} URL", f[0]);
        // The frame document lays out in its viewport: the root's box is
        // as wide as the viewport.
        let fragments = doc.fragments().expect("the frame document is laid out");
        assert_eq!(
            fragments.scroll_size.width.to_string(),
            f[3],
            "{} clientWidth",
            f[0]
        );
    }
}

/// `about-blank`: the initial document of a frame made by insertion.
#[test]
fn initial_about_blank_document_matches_chromium() {
    let (mut page, _) = open("<!DOCTYPE html><body><p>x</p>", &[], false);
    // Chromium: "after:true:about:blank:complete:about:blank:<html>...:O:UTF-8:BackCompat:text/html"
    let log = js_value("about-blank", 0);
    let after = log
        .as_array()
        .unwrap()
        .iter()
        .find_map(|v| v.as_str().unwrap().strip_prefix("after:"))
        .unwrap()
        .to_owned();
    let mut parts = after.splitn(6, ':');
    assert_eq!(parts.next(), Some("true"));
    let rest = parts.collect::<Vec<_>>().join(":");
    // URL, readyState, location.href, markup, origin, charset, mode, type.
    let expected = rest.replacen("about:blank:complete:about:blank:", "", 1);
    let markup_end = expected.find("</html>").unwrap() + "</html>".len();
    let (markup, tail) = expected.split_at(markup_end);
    let tail: Vec<&str> = tail.trim_start_matches(':').split(':').collect();
    page.change_dom(|dom| {
        let body = dom.body().unwrap();
        let iframe = new_iframe(dom);
        dom.append_child(body, iframe).unwrap();
    });
    let children = top_children(&page);
    assert_eq!(children.len(), 1);
    let id = children[0];
    let doc = page.frame_document(id).unwrap();
    assert_eq!(doc.url().as_str(), "about:blank");
    assert!(!page.frame_is_loading(id), "readyState complete");
    let root = doc.dom().document_element().unwrap();
    assert_eq!(swb_dom::outer_html(doc.dom(), root), markup);
    assert_eq!(tail[0], "O");
    assert_eq!(
        doc.origin_string(),
        url(MAIN).origin().ascii_serialization()
    );
    assert_eq!(
        doc.dom().quirks_mode == swb_dom::QuirksMode::Quirks,
        tail[2] == "BackCompat"
    );
    assert_eq!(doc.content_type(), tail[3]);
    assert_eq!(tail[1], "UTF-8");
}

/// `load-order` with JavaScript on: the parent's `DOMContentLoaded`, the
/// `load` of the `iframe` elements, then the parent's `load` (Chromium's
/// order without the entries of the frames' own scripts, which do not run
/// in frames yet).
#[test]
fn load_events_in_chromium_order() {
    on_page_thread(|| {
        let chromium: Vec<String> = js_value("load-order", 0)
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .filter(|e| {
                e.starts_with("parent dcl") || e.contains("iframe load") || e == "parent load"
            })
            .map(|e| e.trim_end_matches(" complete").to_owned())
            .collect();
        assert_eq!(
            chromium,
            [
                "parent dcl",
                "srcdoc iframe load",
                "iframe load",
                "parent load"
            ]
        );
        // The events without the entries of the frame's own script, which
        // `frames/scripted.rs` compares with the probe's page.
        let html = "<!DOCTYPE html><script>window.log = []; \
            addEventListener('DOMContentLoaded', () => log.push('parent dcl')); \
            addEventListener('load', () => log.push('parent load'));</script>\
            <iframe id=a src=child.html onload=\"log.push('iframe load')\"></iframe>\
            <iframe id=b srcdoc=\"<p>s\" onload=\"log.push('srcdoc iframe load')\"></iframe>";
        let (mut page, _) = open(
            html,
            &[(
                "/case/child.html",
                case_resource("load-order", "child.html"),
            )],
            true,
        );
        let log = value(
            &mut page,
            "log.filter(e => !e.startsWith('child')).join(',')",
        );
        assert_eq!(log, format!("\"{}\"", chromium.join(",")));
        assert!(page.frame_load_fired(frame(&page, "a")));
    });
}

/// `cookies-frames`, the response part: a cross-site frame document sets
/// and sends only the `SameSite=None; Secure` cookie; a same-site one all.
#[test]
fn cookies_of_frame_documents_follow_the_site_for_cookies() {
    let chromium = js_value("cookies-frames", 0);
    let frames = chromium[0].as_str().unwrap();
    // The cookies of the responses (`tpa`, `tpb`) that each frame sees.
    assert!(frames.contains(r#""localhost":{"host":"localhost","cookie":"tpa=1; tpc=1""#));
    assert!(frames.contains(r#""cookie":"tpa=1; tpb=1; tpc=1; tpd=1""#));
    let tp = case_resource("cookies-frames", "tp.html");
    let html = format!(
        "<!DOCTYPE html><iframe id=f1 src={LOCAL}/case/tp.html></iframe>\
         <iframe id=f2 src={OTHER}/case/tp.html></iframe>"
    );
    let (mut page, sim) = open(&html, &[("/case/tp.html", tp)], false);
    let jar = page.cookie_jar().unwrap();
    let mut names: Vec<(String, String)> = jar
        .cookies()
        .into_iter()
        .map(|c| (c.domain, c.name))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("127.0.0.1".to_owned(), "tpa".to_owned()),
            ("127.0.0.1".to_owned(), "tpb".to_owned()),
            ("localhost".to_owned(), "tpa".to_owned()),
        ]
    );
    // The request of each frame document: destination `iframe`, mode
    // `navigate`, the parent's origin as initiator.
    for request in sim.requests_full() {
        if request.url.path() == "/case/tp.html" {
            assert_eq!(request.destination, swb_net::Destination::IFrame);
            assert_eq!(request.mode, swb_net::RequestMode::Navigate);
            assert_eq!(request.initiator, Some(url(MAIN).origin()));
        }
    }
    // Loaded again, each frame document sends what it may.
    for id in ["f1", "f2"] {
        let node = common::node(&page, id);
        page.change_dom(|dom| {
            let src = dom.element(node).unwrap().attr("src").unwrap().to_owned();
            dom.set_attr(node, "src", &src).unwrap();
        });
    }
    finish_loading(&mut page, Duration::from_secs(20));
    // The two requests run in parallel: their order is not fixed.
    let mut sent: Vec<(String, Option<String>)> = sim.requests().into_iter().skip(3).collect();
    sent.sort();
    assert_eq!(
        sent,
        [
            (
                format!("{OTHER}/case/tp.html"),
                Some("tpa=1; tpb=1".to_owned())
            ),
            (format!("{LOCAL}/case/tp.html"), Some("tpa=1".to_owned())),
        ]
    );
}

/// `javascript-off`: the boxes of two frames and their documents: the
/// `noscript` content is parsed as markup, no script ran.
#[test]
fn frames_with_javascript_off() {
    let html = case_html("javascript-off");
    let (mut page, _) = open(
        &html,
        &[("/case/c.html", case_resource("javascript-off", "c.html"))],
        false,
    );
    page.update_layout();
    let want = chromium_boxes("javascript-off");
    let got: Vec<Rect> = iframes(&page)
        .into_iter()
        .map(|n| page.element_box(n).unwrap())
        .collect();
    assert_eq!(want.len(), got.len());
    for ((_, w), g) in want.iter().zip(&got) {
        assert!(close(*w, *g), "Chromium {w:?}, swb {g:?}");
    }
    // [frames.length, body.innerHTML.length of each frame]
    let js = js_value("javascript-off", 0);
    let js: Vec<u64> = js
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(page.frame_count() as u64, js[0]);
    for (id, want) in top_children(&page).into_iter().zip(&js[1..]) {
        let doc = page.frame_document(id).unwrap();
        let body = doc.dom().body().unwrap();
        let markup = swb_dom::inner_html(doc.dom(), body);
        assert_eq!(markup.encode_utf16().count() as u64, *want, "{markup}");
        // The noscript content has a box in the frame's layout.
        let div = doc
            .dom()
            .descendants(NodeId::DOCUMENT)
            .find(|&n| doc.dom().is_html_element(n, &swb_dom::local_name!("div")))
            .unwrap();
        let boxes = doc.fragments().unwrap().element_boxes();
        assert_eq!(boxes[&div].width, 50.0);
    }
}

/// `limits`: the 1,001st `iframe` gets no frame; removal destroys them;
/// frames nest at most `MAX_DEPTH` deep (Chromium: 300; departure).
#[test]
fn frame_limits() {
    let chromium = js_value("limits", 0);
    let chromium: Vec<i64> = chromium
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(&chromium[..4], [1000, 100, 1000, 0]);
    assert_eq!(MAX_FRAMES, 1000);
    let html = format!("<!DOCTYPE html><body>{}", "<iframe></iframe>".repeat(1100));
    let (mut page, _) = open(&html, &[], false);
    assert_eq!(page.frame_count(), MAX_FRAMES);
    let without: Vec<usize> = iframes(&page)
        .into_iter()
        .enumerate()
        .filter(|&(_, n)| page.frame_of(FrameId::TOP, n).is_none())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(without.len() as i64, chromium[1]);
    assert_eq!(without[0] as i64, chromium[2]);
    let some = top_children(&page)[5];
    page.change_dom(|dom| {
        let body = dom.body().unwrap();
        let children: Vec<NodeId> = dom.children(body).collect();
        for child in children {
            dom.remove_child(body, child).unwrap();
        }
    });
    assert_eq!(page.frame_count() as i64, chromium[3]);
    assert!(
        page.frame_document(some).is_none(),
        "a stale id finds nothing"
    );
    assert!(page.frame_parent(some).is_none());
    // A chain of srcdoc frames 100 deep stops at the depth limit.
    let mut nested = String::new();
    for _ in 0..100 {
        nested = format!("<iframe srcdoc=\"{}\"></iframe>", escape(&nested));
    }
    let (page, _) = open(&format!("<!DOCTYPE html>{nested}"), &[], false);
    assert_eq!(page.frame_count(), MAX_DEPTH as usize);
    let deepest = *page.frame_ids().last().unwrap();
    let mut depth = 0;
    let mut f = Some(deepest);
    while let Some(p) = f.and_then(|f| page.frame_parent(f)) {
        depth += 1;
        f = Some(p);
    }
    assert_eq!(depth, MAX_DEPTH);
}

/// HTML attribute escaping for nested `srcdoc`.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// `self-embed`: a page that embeds its own URL loads once more; the
/// second level stays `about:blank` (two ancestors have the URL).
#[test]
fn self_embedding_stops_at_the_second_level() {
    let chromium = js_value("self-embed", 0);
    let chromium: Vec<&str> = chromium.as_array().unwrap()[1..]
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        chromium,
        ["O/case-7/case-7.html complete", "about:blank complete"]
    );
    let html = "<!DOCTYPE html><iframe id=f src=case.html></iframe>";
    let (page, sim) = open(html, &[], false);
    let first = top_children(&page);
    assert_eq!(first.len(), 1);
    assert_eq!(frame_url(&page, first[0]), format!("{MAIN}/case/case.html"));
    let second = page.frame_children(first[0]);
    assert_eq!(second.len(), 1);
    assert_eq!(frame_url(&page, second[0]), "about:blank");
    assert_eq!(page.frame_children(second[0]).len(), 0);
    assert!(!page.frame_is_loading(second[0]));
    assert_eq!(sim.requests().len(), 2, "the page and one frame document");
}

/// `sandbox`: without `allow-same-origin` the document's origin is
/// opaque; without `allow-scripts` its scripts do not run (the text of
/// `c`, which Chromium shows).
#[test]
fn sandboxed_frames_have_opaque_origins() {
    let chromium = js_value("sandbox", 0);
    let values = chromium.as_array().unwrap();
    assert_eq!(values[1], "null");
    assert_eq!(values[2], "null");
    let html = case_html("sandbox");
    let (page, _) = open(&html, &[], false);
    let origin = |id: &str| {
        page.frame_document(frame(&page, id))
            .unwrap()
            .origin_string()
    };
    assert_eq!(origin("a"), "null");
    assert_eq!(origin("b"), "null");
    assert_eq!(origin("c"), url(MAIN).origin().ascii_serialization());
    let c = page.frame_document(frame(&page, "c")).unwrap();
    let body = c.dom().body().unwrap();
    assert_eq!(c.dom().text_content(body), values[3].as_str().unwrap());
    assert!(page.frame_sandbox(frame(&page, "b")).unwrap().allow_scripts);
}

/// `sandbox-nested`: a frame inside a sandboxed frame has the container's
/// flags and the parent frame's (HTML "determining the creation
/// sandboxing flags"): without `allow-same-origin` at any level the origin
/// is opaque.
#[test]
fn nested_frames_inherit_the_sandbox() {
    let chromium: Vec<String> = js_value("sandbox-nested", 0)
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        chromium,
        ["a1 null", "b1 null", "b2 O", "b3 O", "c1 null"],
        "Chromium's values"
    );
    let names = ["outer1.html", "outer2.html", "inner.html"];
    let resources: Vec<(String, Res)> = names
        .iter()
        .map(|n| (format!("/case/{n}"), case_resource("sandbox-nested", n)))
        .collect();
    let resources: Vec<(&str, Res)> = resources
        .iter()
        .map(|(p, r)| (p.as_str(), r.clone()))
        .collect();
    let html = case_html("sandbox-nested");
    let (page, _) = open(&html, &resources, false);
    let main = url(MAIN).origin().ascii_serialization();
    let mut got: Vec<String> = page
        .frame_ids()
        .into_iter()
        .filter_map(|id| {
            let name = page.frame_name(id).filter(|n| n.len() == 2)?;
            let origin = page.frame_document(id)?.origin_string();
            let origin = if origin == main { "O" } else { origin.as_str() };
            Some(format!("{name} {origin}"))
        })
        .collect();
    got.sort();
    assert_eq!(got, chromium);
    // The flags of the nested frames: scripts only where every level allows.
    let b1 = page
        .frame_ids()
        .into_iter()
        .find(|&id| page.frame_name(id) == Some("b1"))
        .unwrap();
    let flags = page.frame_sandbox(b1).unwrap();
    assert!(flags.opaque_origin() && flags.scripts_allowed());
}

// ----- The frame tree -----

/// Insertion makes a frame, a `src` change navigates it, a `srcdoc`
/// change makes an `about:srcdoc` document, removal destroys it with its
/// descendants.
#[test]
fn frame_tree_follows_the_dom() {
    let inner = "<!DOCTYPE html><iframe id=n srcdoc='<p>deep'></iframe>";
    let (mut page, sim) = open(
        "<!DOCTYPE html><div id=box><iframe id=a src=one.html></iframe></div>",
        &[
            ("/case/one.html", Res::html(inner)),
            ("/case/two.html", Res::html("<!DOCTYPE html><p>two")),
        ],
        false,
    );
    let a = frame(&page, "a");
    assert_eq!(frame_url(&page, a), format!("{MAIN}/case/one.html"));
    let nested = page.frame_children(a);
    assert_eq!(nested.len(), 1);
    assert_eq!(frame_url(&page, nested[0]), "about:srcdoc");
    assert_eq!(page.frame_ids(), [FrameId::TOP, a, nested[0]]);
    let node = common::node(&page, "a");
    // `src` navigates: the frame stays, its document and its frames go.
    page.change_dom(|dom| dom.set_attr(node, "src", "two.html").unwrap());
    assert!(page.frame_is_loading(a));
    assert!(page.is_loading());
    finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(frame(&page, "a"), a);
    assert_eq!(frame_url(&page, a), format!("{MAIN}/case/two.html"));
    assert!(page.frame_document(nested[0]).is_none());
    // Setting the same value navigates again.
    let before = sim.requests().len();
    page.change_dom(|dom| dom.set_attr(node, "src", "two.html").unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(sim.requests().len(), before + 1);
    // `srcdoc` wins over `src`.
    page.change_dom(|dom| dom.set_attr(node, "srcdoc", "<p id=s>s").unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(frame_url(&page, a), "about:srcdoc");
    let doc = page.frame_document(a).unwrap();
    assert_eq!(doc.dom().quirks_mode, swb_dom::QuirksMode::NoQuirks);
    assert_eq!(
        doc.origin_string(),
        url(MAIN).origin().ascii_serialization()
    );
    // A move destroys the frame and makes a new one.
    let box_node = common::node(&page, "box");
    page.change_dom(|dom| {
        let body = dom.body().unwrap();
        dom.append_child(body, node).unwrap();
    });
    let moved = frame(&page, "a");
    assert_ne!(moved, a);
    assert!(page.frame_document(a).is_none());
    // Removal of an ancestor destroys it.
    page.change_dom(|dom| {
        let body = dom.body().unwrap();
        dom.append_child(box_node, node).unwrap();
        dom.remove_child(body, box_node).unwrap();
    });
    assert_eq!(page.frame_count(), 0);
    assert!(page.frame_document(moved).is_none());
}

/// A frame's request is cancelled when the frame goes; a navigation of
/// the page destroys all frames.
#[test]
fn removed_frames_stop_loading() {
    let mut slow = Res::html("<p>slow");
    slow.delay_ms = 300;
    let (mut page, _) = open(
        "<!DOCTYPE html><iframe id=a src=slow.html></iframe><iframe id=b src=slow.html></iframe>",
        &[
            ("/case/slow.html", slow),
            ("/case/next.html", Res::html("<p>next")),
        ],
        false,
    );
    assert_eq!(page.frame_count(), 2);
    let node = common::node(&page, "a");
    page.change_dom(|dom| dom.set_attr(node, "src", "slow.html?2").unwrap());
    assert!(page.is_loading());
    page.change_dom(|dom| {
        let parent = dom.parent(node).unwrap();
        dom.remove_child(parent, node).unwrap();
    });
    assert!(
        !page.is_loading(),
        "the removed frame does not delay the page"
    );
    page.navigate(url(&format!("{MAIN}/case/next.html")));
    finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(page.frame_count(), 0);
}

/// The load state: frames delay the page; frames made after the page
/// loaded do not.
#[test]
fn frames_delay_the_load_until_they_loaded() {
    let mut slow = Res::html("<!DOCTYPE html><iframe src=slower.html></iframe>");
    slow.delay_ms = 150;
    let mut slower = Res::html("<p>x");
    slower.delay_ms = 150;
    let sim = Sim::new(&[
        (
            "/case/case.html",
            Res::html("<!DOCTYPE html><iframe src=slow.html></iframe>"),
        ),
        ("/case/slow.html", slow),
        ("/case/slower.html", slower),
    ]);
    let mut page = new_page(sim.clone(), 4, Size::new(800.0, 600.0));
    page.navigate(url(&format!("{MAIN}/case/case.html")));
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    // Both frame documents arrived before the page counted as loaded.
    assert_eq!(sim.requests().len(), 3);
    assert!(page.is_fully_loaded());
    let ids = page.frame_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().skip(1).all(|&id| page.frame_load_fired(id)));
    // A frame made after the load does not make the page load again.
    page.change_dom(|dom| {
        let body = dom.body().unwrap();
        let iframe = new_iframe(dom);
        dom.set_attr(iframe, "src", "slow.html?late").unwrap();
        dom.append_child(body, iframe).unwrap();
    });
    assert!(!page.is_loading());
    assert!(page.is_fully_loaded());
    assert_eq!(page.frame_count(), 3);
}

/// Recursion: a frame may load its parent's URL once, not twice.
#[test]
fn recursion_is_refused_at_two_ancestors() {
    let (mut page, _) = open("<!DOCTYPE html><iframe id=a></iframe>", &[], false);
    let node = common::node(&page, "a");
    let self_url = format!("{MAIN}/case/case.html#x");
    page.change_dom(|dom| dom.set_attr(node, "src", &self_url).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    let a = frame(&page, "a");
    assert_eq!(frame_url(&page, a), self_url);
    let inner = page.frame_children(a)[0];
    assert_eq!(frame_url(&page, inner), "about:blank");
}

/// The viewport of a frame is its container's content box; a frame
/// without a box keeps the last one; a change of the container's size
/// lays the frame out again.
#[test]
fn frame_viewports_follow_the_content_box() {
    let (mut page, _) = open(
        "<!DOCTYPE html><style>body{margin:0}</style>\
         <iframe id=a style='width:200px;height:100px;padding:5px;border:3px solid'></iframe>\
         <iframe id=h style=display:none></iframe>\
         <iframe id=big style='width:100000px;height:10px'></iframe>",
        &[],
        false,
    );
    page.update_layout();
    let a = frame(&page, "a");
    assert_eq!(page.frame_viewport(a), Some(Size::new(200.0, 100.0)));
    assert_eq!(
        page.frame_viewport(frame(&page, "h")),
        Some(DEFAULT_FRAME_VIEWPORT)
    );
    assert_eq!(
        page.frame_viewport(frame(&page, "big")),
        Some(Size::new(swb_engine::MAX_VIEWPORT_SIDE, 10.0))
    );
    let node = common::node(&page, "a");
    page.change_dom(|dom| {
        dom.set_attr(node, "style", "width:120px;height:80px")
            .unwrap();
    });
    page.update_layout();
    assert_eq!(page.frame_viewport(a), Some(Size::new(120.0, 80.0)));
    let doc = page.frame_document(a).unwrap();
    assert_eq!(doc.fragments().unwrap().scroll_size, Size::new(120.0, 80.0));
    // Hidden: the viewport stays.
    page.change_dom(|dom| dom.set_attr(node, "style", "display:none").unwrap());
    page.update_layout();
    assert_eq!(page.frame_viewport(a), Some(Size::new(120.0, 80.0)));
    assert_eq!(
        page.frame_viewport(FrameId::TOP),
        Some(Size::new(800.0, 600.0))
    );
}

/// A frame that reloads itself with `meta refresh 0` stops after
/// `MAX_NAVIGATIONS` navigations in 10 s of page time.
#[test]
fn a_refreshing_frame_stops_at_the_navigation_limit() {
    let (mut page, sim) = open(
        "<!DOCTYPE html><iframe src=r.html></iframe>",
        &[(
            "/case/r.html",
            Res::html("<meta http-equiv=refresh content=0><p>r"),
        )],
        false,
    );
    page.set_clock(swb_engine::PageClock::new(
        swb_engine::ClockMode::Virtual,
        None,
    ));
    page.run_event_loop(Duration::from_secs(5), Duration::from_secs(30));
    let loads = sim
        .requests()
        .iter()
        .filter(|(u, _)| u.ends_with("/r.html"))
        .count();
    assert_eq!(loads, swb_engine::MAX_NAVIGATIONS);
    assert!(page.is_idle(Duration::from_millis(10)));
}

/// `page.info`-style accessors and stale ids.
#[test]
fn stale_frame_ids_find_nothing() {
    let (mut page, _) = open("<!DOCTYPE html><iframe id=a name=n1></iframe>", &[], false);
    let a = frame(&page, "a");
    assert_eq!(page.frame_name(a), Some("n1"));
    assert_eq!(page.frame_parent(a), Some(FrameId::TOP));
    assert_eq!(FrameId::from_u64(a.to_u64()), a);
    let node = common::node(&page, "a");
    // A later change of the attribute does not rename the browsing context
    // (measured, case `scripted-window-members`).
    page.change_dom(|dom| dom.set_attr(node, "name", "n2").unwrap());
    assert_eq!(page.frame_name(a), Some("n1"));
    page.navigate(url(&format!("{MAIN}/case/case.html")));
    finish_loading(&mut page, Duration::from_secs(20));
    let b = frame(&page, "a");
    assert_ne!(a, b, "the new frame has a new generation");
    assert!(page.frame_document(a).is_none());
    assert!(page.frame_viewport(a).is_none());
    assert!(page.frame_name(a).is_none());
    assert_eq!(page.frame_children(a).len(), 0);
    assert!(page.frame_document(FrameId::TOP).is_some());
}

// ----- Review fixes of feature 10a -----

/// A resource that answers after `ms`.
fn slow(ms: u64, body: &str) -> Res {
    let mut res = Res::html(body);
    res.delay_ms = ms;
    res
}

/// Opens `html` like [`open`] with JavaScript on but returns as soon as the
/// page has `frames` frames, without waiting for the load.
fn start(html: &str, resources: &[(&str, Res)], frames: usize) -> (Page, Arc<Sim>) {
    let mut all: Vec<(&str, Res)> = vec![("/case/case.html", Res::html(html))];
    all.extend(resources.iter().cloned());
    let sim = Sim::new(&all);
    let mut page = new_page(sim.clone(), 4, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.navigate(url(&format!("{MAIN}/case/case.html")));
    let end = Instant::now() + Duration::from_secs(10);
    while page.frame_count() < frames {
        assert!(Instant::now() < end, "the frames did not appear");
        page.process_network();
        page.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    (page, sim)
}

/// Runs the event loop for `ms` of real time.
fn pump_for(page: &mut Page, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        page.process_network();
        page.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// `geometry`, the style of the `iframe` elements in the parent: display,
/// border style, width and color, overflow and vertical-align (fields 6 to
/// 13 of Chromium's rows, the color has three tokens) run with JavaScript
/// on.
#[test]
fn iframe_computed_style_matches_chromium() {
    on_page_thread(|| {
        let rows = js_value("geometry", 0);
        let want: Vec<String> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let f: Vec<&str> = row.as_str().unwrap().split(' ').collect();
                format!("{} {}", f[0], f[6..14].join(" "))
            })
            .collect();
        let (mut page, _) = open(&case_html("geometry"), &[], true);
        let got = value(
            &mut page,
            "[...document.querySelectorAll('iframe')].map(f => { const s = getComputedStyle(f); \
             return [f.id, s.display, s.borderTopStyle, s.borderTopWidth, s.borderTopColor, \
             s.overflow, s.verticalAlign].join(' '); })",
        );
        let got: Vec<String> = serde_json::from_str(&got).unwrap();
        assert_eq!(got, want);
    });
}

/// Every case that `QUERIES_LATER` names is in the probe file.
#[test]
fn later_queries_name_cases_of_the_probe_file() {
    let probe = probe_file();
    for (case, what) in QUERIES_LATER {
        assert!(
            probe["cases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["name"] == *case),
            "{case}: {what}"
        );
    }
}

/// `srcdoc-base` and `srcdoc-no-base`: the base URL of an `about:srcdoc`
/// document is its container document's (HTML "fallback base URL"), so a
/// relative image and style sheet request the URLs that Chromium requests.
#[test]
fn srcdoc_documents_resolve_urls_against_the_parent_base() {
    for name in ["srcdoc-base", "srcdoc-no-base"] {
        let chromium = js_value(name, 0);
        let want_paths: Vec<String> = chromium[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let html = case_html(name).replace("$FOLDER", "case");
        let (page, sim) = open(&html, &[], false);
        let mut got: Vec<String> = sim
            .requests()
            .into_iter()
            .filter_map(|(u, _)| {
                let path = url(&u).path().to_owned();
                let extension = path.rsplit('.').next().unwrap_or_default();
                matches!(extension, "png" | "css")
                    .then(|| path.trim_start_matches("/case/").to_owned())
            })
            .collect();
        got.sort();
        assert_eq!(got, want_paths, "{name}: requests");
        // `baseURI` of the frame document: "O/case-N/..." in Chromium.
        let doc = page.frame_document(frame(&page, "a")).unwrap();
        let base = doc.base_url().as_str().replace(MAIN, "O");
        let want_base = chromium[0].as_str().unwrap();
        let folder = want_base[2..].split('/').next().unwrap();
        let want_base = want_base.replace(folder, "case");
        assert_eq!(base, want_base, "{name}: base URL");
        assert_eq!(doc.url().as_str(), "about:srcdoc");
    }
}

/// The site for cookies through the frame chain (ADR 0031 section 3): top
/// `127.0.0.1:8000`, a `localhost:8001` frame, a `127.0.0.1:8001` frame in
/// it. The leaf and its image are cross-site for cookies (an opaque site,
/// as in Chromium); a same-site frame at depth 1 and its image get the
/// top-level site.
#[test]
fn the_site_for_cookies_follows_the_frame_chain() {
    let html = format!(
        "<!DOCTYPE html><iframe src={LOCAL}/case/mid.html></iframe>\
         <iframe src={OTHER}/case/same.html></iframe>"
    );
    let mid = format!("<!DOCTYPE html><iframe src={OTHER}/case/leaf.html></iframe>");
    let (page, sim) = open(
        &html,
        &[
            ("/case/mid.html", Res::html(&mid)),
            ("/case/leaf.html", Res::html("<img src=leaf.png>")),
            ("/case/same.html", Res::html("<img src=same.png>")),
            ("/case/leaf.png", Res::html("x")),
            ("/case/same.png", Res::html("x")),
        ],
        false,
    );
    assert_eq!(page.frame_count(), 3);
    let top = url(MAIN).origin();
    let site = |path: &str| {
        sim.requests_full()
            .into_iter()
            .find(|r| r.url.path() == path)
            .unwrap_or_else(|| panic!("no request for {path}"))
            .site_for_cookies
    };
    // Frame document requests: the parent's chain decides.
    assert_eq!(site("/case/mid.html"), Some(top.clone()));
    assert_eq!(site("/case/same.html"), Some(top.clone()));
    let opaque = |o: Option<swb_net::Origin>| o.is_some_and(|o| !o.is_tuple());
    assert!(opaque(site("/case/leaf.html")), "the leaf's document");
    assert!(opaque(site("/case/leaf.png")), "the leaf's image");
    assert_eq!(site("/case/same.png"), Some(top), "the same-site image");
}

/// A frame that delays the load is removed: the parent's `load` fires at
/// once, as in Chromium (JavaScript on), by `change_dom` and by a script,
/// with and without a `pump` before the wait.
#[test]
fn removing_the_last_delaying_frame_fires_load() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>window.log = []; \
            addEventListener('load', () => log.push('load'));</script>\
            <iframe id=a src=slow.html></iframe>";
        for (by_script, pump) in [(false, false), (false, true), (true, false), (true, true)] {
            let (mut page, _) = start(html, &[("/case/slow.html", slow(1500, "<p>slow"))], 1);
            let began = Instant::now();
            assert_eq!(value(&mut page, "log.length"), "0");
            if by_script {
                value(&mut page, "a.remove(); 1");
            } else {
                let node = common::node(&page, "a");
                page.change_dom(|dom| {
                    let parent = dom.parent(node).unwrap();
                    dom.remove_child(parent, node).unwrap();
                });
            }
            assert_eq!(page.frame_count(), 0);
            if pump {
                page.pump();
            }
            assert!(
                page.wait_until_loaded(Duration::from_secs(5)),
                "by_script {by_script}, pump {pump}"
            );
            assert!(began.elapsed() < Duration::from_millis(1000), "at once");
            assert_eq!(value(&mut page, "log.join()"), "\"load\"");
        }
    });
}

/// The completion of a removed frame's request, after the removal, changes
/// nothing (JavaScript on and off).
#[test]
fn a_completion_after_removal_is_dropped() {
    on_page_thread(|| {
        for javascript in [false, true] {
            let mut page_html = String::from("<!DOCTYPE html><body><div id=d></div>");
            if javascript {
                page_html.push_str(
                    "<script>window.n = 0; addEventListener('load', () => n++);</script>",
                );
            }
            let (mut page, sim) = open(
                &page_html,
                &[("/case/slow.html", slow(300, "<p>slow"))],
                javascript,
            );
            let node = page
                .change_dom(|dom| {
                    let body = dom.body().unwrap();
                    let iframe = new_iframe(dom);
                    dom.set_attr(iframe, "src", "slow.html").unwrap();
                    dom.append_child(body, iframe).unwrap();
                    iframe
                })
                .unwrap();
            assert_eq!(page.frame_count(), 1);
            page.change_dom(|dom| {
                let parent = dom.parent(node).unwrap();
                dom.remove_child(parent, node).unwrap();
            });
            pump_for(&mut page, 600);
            assert_eq!(page.frame_count(), 0);
            assert!(!page.is_loading());
            assert!(page.wait_until_loaded(Duration::from_secs(2)));
            drop(sim);
            if javascript {
                assert_eq!(value(&mut page, "n"), "1", "load fired once");
            }
        }
    });
}

/// A move (removal and insertion in one task) makes a new frame with one
/// more request; `iframe` elements in a fragment, a `template` or another
/// document get no frame.
#[test]
fn moves_and_detached_iframes() {
    on_page_thread(|| {
        let (mut page, sim) = open(
            "<!DOCTYPE html><body><div id=d><iframe id=a src=s2.html></iframe></div>",
            &[("/case/s2.html", Res::html("<p>two</p>"))],
            true,
        );
        let before = sim.requests().len();
        let old = frame(&page, "a");
        value(
            &mut page,
            "var f = a; d.removeChild(f); d.appendChild(f); 1",
        );
        let new = frame(&page, "a");
        assert_ne!(old, new);
        assert_eq!(page.frame_count(), 1);
        pump_for(&mut page, 200);
        assert_eq!(sim.requests().len(), before + 1, "one request for the move");
        value(
            &mut page,
            "var fr = document.createDocumentFragment(); \
             fr.appendChild(document.createElement('iframe')); \
             var t = document.createElement('template'); \
             t.innerHTML = '<iframe src=s2.html></iframe>'; \
             var d2 = document.implementation.createHTMLDocument(''); \
             d2.body.appendChild(d2.createElement('iframe')); 1",
        );
        pump_for(&mut page, 100);
        assert_eq!(page.frame_count(), 1, "no frames for detached iframes");
        assert_eq!(sim.requests().len(), before + 1);
        // The fragment's iframe gets a frame when it is inserted.
        value(&mut page, "d.appendChild(fr); 1");
        pump_for(&mut page, 100);
        assert_eq!(page.frame_count(), 2);
    });
}

/// A container that removes itself in its own `load` listener, and one that
/// an earlier listener removed and inserted again: no `load` for the old
/// frame, one for the new frame after it loaded.
#[test]
fn load_events_survive_removal_in_listeners() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>window.log = []; \
            addEventListener('load', () => log.push('parent load'));</script>\
            <div id=d>\
            <iframe id=a srcdoc=x onload=\"log.push('a load'); const x = document.getElementById('b'); x.remove(); d.appendChild(x)\"></iframe>\
            <iframe id=b srcdoc=y onload=\"log.push('b load')\"></iframe>\
            <iframe id=c srcdoc=z onload=\"log.push('c load'); this.remove()\"></iframe></div>";
        let (mut page, _) = open(html, &[], true);
        pump_for(&mut page, 100);
        let log = value(&mut page, "log.join()");
        let log: String = serde_json::from_str(&log).unwrap();
        let events: Vec<&str> = log.split(',').collect();
        assert_eq!(
            events.iter().filter(|e| **e == "b load").count(),
            1,
            "{log}"
        );
        assert!(
            events.contains(&"a load") && events.contains(&"c load"),
            "{log}"
        );
        assert!(events.contains(&"parent load"), "{log}");
        assert!(page.frame_load_fired(frame(&page, "b")));
        assert_eq!(page.frame_count(), 2, "c removed itself: {log}");
    });
}

/// The budget for the source of frame documents (ADR 0031 section 11):
/// 70 `srcdoc` frames of 1 MiB; the first 64 MiB fit, the others become
/// error documents with one warning and still finish loading. A frame that
/// navigates again pays only the difference, and destroyed frames release
/// their bytes.
#[test]
fn frame_documents_share_a_source_budget() {
    on_page_thread(|| {
        let script = "<script>const t = 'x'.repeat(1 << 20); \
            for (let i = 0; i < 70; i++) { const f = document.createElement('iframe'); \
            f.id = 'f' + i; f.srcdoc = t; document.body.appendChild(f); }</script>";
        let (mut page, _) = open(&format!("<!DOCTYPE html><body>{script}"), &[], true);
        assert_eq!(page.frame_count(), 70);
        let budget = swb_engine::MAX_SOURCE_BYTES;
        assert_eq!(page.frame_source_bytes(), budget);
        let errors = top_children(&page)
            .into_iter()
            .filter(|&id| {
                let doc = page.frame_document(id).unwrap();
                let body = doc.dom().body().unwrap();
                doc.dom()
                    .text_content(body)
                    .contains("Too many frame documents")
            })
            .count();
        assert_eq!(errors, 6);
        assert!(
            top_children(&page)
                .iter()
                .all(|&id| page.frame_load_fired(id))
        );
        assert!(!page.is_loading());
        // A frame with a small document frees room for a big one.
        let first = frame(&page, "f0");
        let last = frame(&page, "f69");
        value(&mut page, "f0.srcdoc = 'small'; 1");
        pump_for(&mut page, 100);
        assert_eq!(page.frame_source_bytes(), budget - (1 << 20) + 5);
        // One byte more than the room does not fit; the room itself does.
        value(&mut page, "f69.srcdoc = 'x'.repeat((1 << 20) - 4); 1");
        pump_for(&mut page, 100);
        assert_eq!(page.frame_source_bytes(), budget - (1 << 20) + 5, "too big");
        value(&mut page, "f69.srcdoc = 'x'.repeat((1 << 20) - 5); 1");
        pump_for(&mut page, 100);
        assert_eq!(page.frame_source_bytes(), budget);
        assert!(page.frame_document(first).is_some() && page.frame_document(last).is_some());
        // Removal releases the bytes.
        value(&mut page, "document.body.textContent = ''; 1");
        pump_for(&mut page, 50);
        assert_eq!(page.frame_count(), 0);
        assert_eq!(page.frame_source_bytes(), 0);
    });
}

/// The nodes of frame documents share a budget of `MAX_NODES` (the node
/// cost is by far the largest per byte of source): of four frames with
/// 300,000 nodes each, the first three fit, the fourth shows an error.
#[test]
fn frame_documents_share_a_node_budget() {
    // `hidden` keeps the nodes out of style and layout, so the test stays
    // small.
    let doc = format!("<body hidden>{}", "<p>".repeat(300_000));
    let mut html = String::from("<!DOCTYPE html>");
    for i in 0..4 {
        let _ = write!(html, "<iframe id=f{i} srcdoc=\"{doc}\"></iframe>");
    }
    let (page, _) = open(&html, &[], false);
    assert_eq!(page.frame_count(), 4);
    let shows_error = |i: usize| {
        let doc = page.frame_document(frame(&page, &format!("f{i}"))).unwrap();
        let body = doc.dom().body().unwrap();
        doc.dom()
            .text_content(body)
            .contains("Too many frame documents")
    };
    assert_eq!(
        (0..4).map(shows_error).collect::<Vec<_>>(),
        [false, false, false, true]
    );
    let nodes = page.frame_node_count();
    assert!(
        (900_000..=swb_engine::MAX_NODES).contains(&nodes),
        "{nodes} nodes"
    );
}

/// After a document did not fit, the parse of the next documents stops early.
/// A document that reaches that limit shows the error document and not its
/// first nodes (second review of feature 10a: `b` had 16,385 nodes and no
/// `#last`); a document below the limit loads.
#[test]
fn a_document_that_reaches_the_limited_parse_shows_the_error() {
    let hidden = "<body hidden>";
    let too_big = "<p>".repeat(1 << 21);
    let cut_off = format!("{hidden}{}<i id=last></i>", "<p>".repeat(30_000));
    let small = format!("{hidden}{}<i id=last></i>", "<p>".repeat(1_000));
    let html = format!(
        "<!DOCTYPE html><iframe id=a srcdoc=\"{hidden}{too_big}\"></iframe>\
         <iframe id=b srcdoc=\"{cut_off}\"></iframe>\
         <iframe id=c srcdoc=\"{small}\"></iframe>"
    );
    let (page, _) = open(&html, &[], false);
    let text = |name: &str| {
        let doc = page.frame_document(frame(&page, name)).unwrap();
        doc.dom().text_content(doc.dom().body().unwrap())
    };
    assert!(text("a").contains("Too many frame documents"));
    assert!(text("b").contains("Too many frame documents"));
    assert!(!text("c").contains("Too many frame documents"));
    let c = page.frame_document(frame(&page, "c")).unwrap();
    assert!(
        c.dom()
            .get_element_by_id(NodeId::DOCUMENT, "last")
            .is_some(),
        "c is complete"
    );
}

/// The external style sheets of frame documents share a budget of
/// `MAX_STYLE_BYTES` (8 MiB): of two frames with a sheet of 5 MiB, one gets
/// its sheet, the other does not. The top-level document has no such
/// budget.
#[test]
fn frame_style_sheets_share_a_budget() {
    let css = format!("/*{}*/p{{color:rgb(255,0,0)}}", " ".repeat(5 << 20));
    let frame_doc = "<!DOCTYPE html><link rel=stylesheet href=big.css><p id=p>x";
    let (page, _) = open(
        "<!DOCTYPE html><link rel=stylesheet href=big.css><p id=p>x\
         <iframe id=a src=a.html></iframe><iframe id=b src=b.html></iframe>",
        &[
            ("/case/a.html", Res::html(frame_doc)),
            ("/case/b.html", Res::html(frame_doc)),
            ("/case/big.css", Res::html(&css)),
        ],
        false,
    );
    let red = |doc: &swb_engine::DocumentState| {
        let p = doc
            .dom()
            .descendants(NodeId::DOCUMENT)
            .find(|&n| doc.dom().is_html_element(n, &swb_dom::local_name!("p")))
            .unwrap();
        doc.styles().unwrap().get(p).unwrap().color.r == 255
    };
    assert!(red(page.frame_document(FrameId::TOP).unwrap()), "top");
    let in_frames = ["a", "b"]
        .iter()
        .filter(|id| red(page.frame_document(frame(&page, id)).unwrap()))
        .count();
    assert_eq!(in_frames, 1, "one of two sheets fits the budget");
}

/// A loop that replaces the document of one frame many times does not use
/// up the budget over time.
#[test]
fn replacing_frame_documents_does_not_use_up_the_budget() {
    on_page_thread(|| {
        let (mut page, _) = open("<!DOCTYPE html><body><iframe id=f></iframe>", &[], true);
        for i in 0..200 {
            value(
                &mut page,
                &format!("f.srcdoc = 'x'.repeat(1 << 20) + {i}; 1"),
            );
            pump_for(&mut page, 5);
        }
        pump_for(&mut page, 100);
        assert!(page.frame_source_bytes() <= (1 << 20) + 3);
        let doc = page.frame_document(frame(&page, "f")).unwrap();
        let body = doc.dom().body().unwrap();
        assert!(!doc.dom().text_content(body).contains("Too many"));
    });
}
