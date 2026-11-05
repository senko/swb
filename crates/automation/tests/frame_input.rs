//! Input in frames against Chromium (M9 feature 11, ADR 0031 section 8):
//! the cases `focus`, `wheel-scroll` and `input-*` of
//! `tools/probes/web-frames.json` run here with the same input steps
//! (`swb_automation::run_steps`, the `input.*` methods) against a
//! simulated server, and the values of their queries are the ones that
//! Chromium 148 gave (`engine/tests/data/web-frames.json`: `uv run
//! --project tools swbtools probe --json tools/probes/web-frames.json
//! --case NAME ...`, three equal runs, merged with the same indent).
//!
//! Frame documents run no scripts in swb yet (features 12 and 13), so the
//! probe's expressions that read a frame (`f.contentDocument`,
//! `f.contentWindow.scrollY`) cannot run. The `eval` steps and queries
//! that read frame state are snapshots (`log.push('KEY:' + ...)`); the test
//! reads the same state through the engine API (`snapshot`) and pushes the
//! string that Chromium's expression gives. What the scripts of the frame
//! documents log (`c:` entries of the log) waits for feature 12: `LATER`
//! lists those queries. `DEPARTURES` lists the queries where swb's value
//! differs on purpose, with the reason.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::{on_page_thread, value};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{FontContext, FrameId, NodeId, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

const MAIN: &str = "http://127.0.0.1:8000";
const OTHER: &str = "http://127.0.0.1:8001";

/// Queries that read what the scripts of the frame documents logged: they
/// wait for feature 12 (scripts in frames). A case name with the index of
/// the query.
const LATER_MARK: &str = "startsWith('c:')";

/// Queries where swb's value differs from Chromium's on purpose: (case,
/// query index, swb's value, why).
type Departure = (&'static str, usize, &'static str, &'static str);

const DEPARTURES: &[Departure] = &[
    (
        "input-focus-remove",
        0,
        "p:blur@win l1:BODY/false p:focus@win p:focus@p1 p:focusin@p1 l2:p1/true",
        "after the removal of the focused frame, Chromium starts sequential navigation where the `iframe` was (the next Tab gives `p2`); swb has no position for a removed node and starts at the first element (backlog)",
    ),
    (
        "input-focus-click-remove",
        0,
        "p:focus@p1 p:focusin@p1 p:blur@p1 p:focusout@p1 p:blur@win l1:BODY/false p:focus@win p:focus@p1 p:focusin@p1 l2:p1/true",
        "as input-focus-remove: Chromium's next Tab gives `p2`, swb's starts at the first element",
    ),
    (
        "input-link-cross-origin-top",
        0,
        "case.html / SecurityError | about:blank",
        "Chromium lets a cross-origin frame navigate the top level after a user activation (the click); swb has no activation tracking and refuses it, with a log entry (ADR 0031 section 3)",
    ),
    (
        "input-link-cross-origin-parent",
        0,
        "case.html / SecurityError | about:blank",
        "as input-link-cross-origin-top: the parent is the top level",
    ),
    (
        "input-target-blank",
        0,
        "case.html | t.html?blank | - | about:blank | - | -",
        "swb has one tab: `_blank` navigates the source frame; Chromium opens a new tab and leaves the frame",
    ),
    (
        "input-target-blank",
        1,
        "2 1",
        "as the first query: the source frame loads the link",
    ),
    (
        "input-target-unknown-name",
        0,
        "case.html | t.html?named | - | about:blank | - | -",
        "as input-target-blank: a name that no frame has",
    ),
    (
        "input-target-unknown-name",
        1,
        "2 1",
        "as the first query: the source frame loads the link",
    ),
    (
        "input-target-name-case",
        0,
        "case.html | t.html?named | - | about:blank | - | -",
        "the name `other` is not the frame's `Other` (compared exactly, as in Chromium), so as input-target-unknown-name",
    ),
    (
        "input-target-name-case",
        1,
        "2 1",
        "as the first query: the source frame loads the link",
    ),
];

/// The simulated server of a case: its files under `/case/`.
struct Sim {
    files: HashMap<String, String>,
}

impl Fetcher for Sim {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let path = request.url.path();
        let (status, body) = match self.files.get(path) {
            Some(text) => (200, text.clone().into_bytes()),
            None => (404, b"not found".to_vec()),
        };
        Ok(Response {
            url: request.url.clone(),
            status,
            headers: [("content-type".to_owned(), "text/html".to_owned())]
                .into_iter()
                .collect::<Headers>(),
            body,
            redirected: false,
        })
    }
}

fn probe_file() -> Value {
    let path = common::repo_root().join("tools/probes/web-frames.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn chromium_file() -> Value {
    let path = common::repo_root().join("crates/engine/tests/data/web-frames.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Replaces the placeholders of the probe server.
fn expand(text: &str) -> String {
    text.replace("$OTHER", OTHER)
        .replace("$ORIGIN", MAIN)
        .replace("$FOLDER", "case")
}

/// The page of a case, loaded (800x600, test fonts, JavaScript on).
fn open(case: &Value) -> Page {
    let mut files = HashMap::new();
    files.insert(
        "/case/case.html".to_owned(),
        expand(case["html"].as_str().unwrap()),
    );
    if let Some(resources) = case["resources"].as_object() {
        for (name, resource) in resources {
            files.insert(
                format!("/case/{name}"),
                expand(resource["text"].as_str().unwrap()),
            );
        }
    }
    let config = PageConfig {
        fetcher: Arc::new(Sim { files }),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(800.0, 600.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(Url::parse(&format!("{MAIN}/case/case.html")).unwrap());
    assert!(page.wait_until_idle(Duration::from_millis(50), Duration::from_secs(30)));
    page.update_layout();
    page
}

// ----- Snapshots: the state of frames through the engine API -----

/// The frame of the `iframe` with ID `id` in the document of `parent`.
fn frame_of(page: &Page, parent: FrameId, id: &str) -> Option<FrameId> {
    let node = page.frame_document(parent)?.dom().element_by_id(id)?;
    page.frame_of(parent, node)
}

/// What `document.activeElement.id || nodeName` gives in the document of
/// `frame`: the element that has the focus, the `iframe` that holds it for
/// a frame, else `BODY`.
fn active(page: &Page, frame: FrameId) -> String {
    let Some(doc) = page.frame_document(frame) else {
        return "?".to_owned();
    };
    let node = doc.focus_element().or(doc.focused_frame_element());
    node.and_then(|n| doc.dom().element(n)).map_or_else(
        || "BODY".to_owned(),
        |e| e.attr("id").unwrap_or("?").to_owned(),
    )
}

/// True if `frame` or one of its descendants holds the page's focus.
fn has_focus(page: &mut Page, frame: FrameId) -> bool {
    let mut at = Some(page.focused_frame_id());
    while let Some(f) = at {
        if f == frame {
            return true;
        }
        at = page.frame_parent(f);
    }
    false
}

/// `element.matches(':hover')` (or `:active`): the element is the state's
/// element or contains it.
fn state_matches(page: &Page, frame: FrameId, element: Option<NodeId>, active: bool) -> bool {
    let Some(doc) = page.frame_document(frame) else {
        return false;
    };
    let state = if active {
        doc.pressed_element()
    } else {
        doc.hovered_element()
    };
    let (Some(element), Some(state)) = (element, state) else {
        return false;
    };
    element == state || doc.dom().ancestors(state).any(|a| a == element)
}

fn by_id(page: &Page, frame: FrameId, id: &str) -> Option<NodeId> {
    page.frame_document(frame)?.dom().element_by_id(id)
}

/// `document.hasFocus()` of the page's document.
fn window_focus(page: &mut Page) -> bool {
    value(page, "document.hasFocus()") == Value::Bool(true)
}

fn flag(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

/// The path and query of a URL as the probe prints them (`t.html?self#z`).
fn short_url(url: &Url) -> String {
    let name = url.path().rsplit('/').next().unwrap_or("");
    format!(
        "{name}{}{}",
        url.query().map_or(String::new(), |q| format!("?{q}")),
        url.fragment().map_or(String::new(), |f| format!("#{f}")),
    )
}

/// The string that the snapshot `key` of a case gives in Chromium.
fn snapshot(page: &mut Page, key: &str, source: &str) -> String {
    let kind = key.trim_end_matches(|c: char| c.is_ascii_digit());
    state_snapshot(page, kind)
        .or_else(|| scroll_snapshot(page, kind))
        .or_else(|| focus_snapshot(page, kind, source))
        .or_else(|| content_snapshot(page, kind))
        .unwrap_or_else(|| panic!("no snapshot {kind:?} for {key}"))
}

fn flags(values: &[bool]) -> String {
    values
        .iter()
        .map(|&v| flag(v))
        .collect::<Vec<_>>()
        .join(",")
}

/// The `:hover` and `:active` chains.
fn state_snapshot(page: &mut Page, kind: &str) -> Option<String> {
    let top = FrameId::TOP;
    let f = frame_of(page, top, "f")?;
    let body =
        |page: &Page, frame: FrameId| page.frame_document(frame).and_then(|d| d.dom().body());
    let f_node = by_id(page, top, "f");
    let a = by_id(page, f, "a");
    Some(match kind {
        // The pointer pages: f, #b, #a, body and root of the frame.
        "h" => {
            let b = by_id(page, top, "b");
            let root = page.frame_document(f)?.dom().document_element();
            let fb = body(page, f);
            flags(&[
                state_matches(page, top, f_node, false),
                state_matches(page, top, b, false),
                state_matches(page, f, a, false),
                state_matches(page, f, fb, false),
                state_matches(page, f, root, false),
            ])
        }
        // The hit pages: f, #under, #a.
        "hh" => {
            let under = by_id(page, top, "under");
            flags(&[
                state_matches(page, top, f_node, false),
                state_matches(page, top, under, false),
                state_matches(page, f, a, false),
            ])
        }
        // `:active`: f, the page's body, #a, the frame's body.
        "a" => {
            let (page_body, fb) = (body(page, top), body(page, f));
            flags(&[
                state_matches(page, top, f_node, true),
                state_matches(page, top, page_body, true),
                state_matches(page, f, a, true),
                state_matches(page, f, fb, true),
            ])
        }
        _ => return None,
    })
}

/// The scroll offsets.
fn scroll_snapshot(page: &mut Page, kind: &str) -> Option<String> {
    let top = FrameId::TOP;
    let y = |page: &Page, frame: Option<FrameId>| {
        frame.map_or(0.0, |f| page.frame_document(f).unwrap().scroll_position().y)
    };
    match kind {
        // The page, #f, and #g (of the page, or of #f).
        "s" | "w" => {
            let f = frame_of(page, top, "f");
            let mut parts = vec![page.scroll_position().y.to_string(), y(page, f).to_string()];
            let inner = f
                .and_then(|f| frame_of(page, f, "g"))
                .or_else(|| frame_of(page, top, "g"));
            if kind == "w" || inner.is_some() {
                parts.push(y(page, inner).to_string());
            }
            Some(parts.join(","))
        }
        // The case `wheel-scroll`: the page, `frames[0]` and `frames[1]`.
        "frames" => {
            let children = page.frame_children(top);
            let child = |i: usize| children.get(i).copied();
            Some(format!(
                "{},{},{}",
                page.scroll_position().y,
                y(page, child(0)),
                y(page, child(1))
            ))
        }
        _ => None,
    }
}

/// The focus: `document.activeElement` of the documents and `hasFocus()`.
fn focus_snapshot(page: &mut Page, kind: &str, source: &str) -> Option<String> {
    let top = FrameId::TOP;
    let f = frame_of(page, top, "f");
    let (page_active, page_focus) = (active(page, top), flag(window_focus(page)));
    Some(match kind {
        // The case `focus`.
        "state" => {
            let f = f?;
            format!(
                "{page_active},{},{page_focus},{}",
                active(page, f),
                flag(has_focus(page, f))
            )
        }
        // After a Tab.
        "fc" => {
            let f = f?;
            format!(
                "{page_active}/{}/{page_focus}/{}",
                active(page, f),
                flag(has_focus(page, f))
            )
        }
        // After a click into a frame: also `:focus` and `:focus-visible`.
        "fx" => {
            let f = f?;
            let c1 = by_id(page, f, "c1");
            let c1_focus = c1.is_some() && page.frame_document(f)?.focus_element() == c1;
            format!(
                "{page_active}/{}/{page_focus}/{}/false/{}/{}",
                active(page, f),
                flag(has_focus(page, f)),
                flag(c1_focus),
                flag(c1_focus),
            )
        }
        // The Tab kinds page: the page, #pos, #empty.
        "fk" => {
            let pos = frame_of(page, top, "pos")?;
            let empty = frame_of(page, top, "empty")?;
            format!(
                "{page_active}/{}/{}",
                active(page, pos),
                active(page, empty)
            )
        }
        // The nested page: the page, #f, #g in #f.
        "fn" => {
            let f = f?;
            let g = frame_of(page, f, "g")?;
            format!("{page_active}/{}/{}", active(page, f), active(page, g))
        }
        // The focus after a frame went: the page and its focus.
        "l" => {
            let mut text = format!("{page_active}/{page_focus}");
            if source.contains("act(f.contentDocument)") {
                text = format!("{text}/{}", active(page, f?));
            }
            text
        }
        _ => return None,
    })
}

/// The form values and the selections.
fn content_snapshot(page: &mut Page, kind: &str) -> Option<String> {
    let top = FrameId::TOP;
    let f = frame_of(page, top, "f")?;
    Some(match kind {
        // The URL of the frame, the values and the active id.
        "v" => {
            let doc = page.frame_document(f)?;
            let get = |id: &str| by_id(page, f, id);
            let value = |id: &str| {
                get(id)
                    .and_then(|n| doc.control_value(n))
                    .unwrap_or_default()
            };
            let checked = |id: &str| {
                get(id)
                    .and_then(|n| doc.control_checked(n))
                    .map_or(String::new(), |c| flag(c).to_owned())
            };
            [
                short_url(doc.url()),
                value("q"),
                checked("c"),
                value("s"),
                active(page, f),
            ]
            .join(" | ")
        }
        // The page's selection, the frame's, the active elements.
        "sl" => {
            let own = page.frame_selected_text(top);
            let frame_text = page.frame_selected_text(f);
            [
                own.trim().to_owned(),
                frame_text.trim().to_owned(),
                active(page, top),
                active(page, f),
            ]
            .join(" | ")
        }
        _ => return None,
    })
}

/// For a step that reads frame state, `{"eval": "log.push('KEY:' + ...)"}`
/// with `contentDocument`, `contentWindow`, `matches(':hover')` of a frame
/// element and the like: the key and the source. The case `wheel-scroll`
/// pushes `frames[...]` values without a key.
fn snapshot_key(step: &Value) -> Option<(String, String)> {
    let source = step.get("eval")?.as_str()?;
    let reads_frame = ["contentDocument", "contentWindow", "f.matches(", "frames["]
        .iter()
        .any(|m| source.contains(m));
    if !reads_frame || source.contains(".scrollTo(") {
        return None;
    }
    if source.starts_with("log.push([scrollY, frames[0]") {
        return Some(("frames".to_owned(), source.to_owned()));
    }
    let rest = source.strip_prefix("log.push('")?;
    let key = rest.split(':').next()?;
    Some((key.to_owned(), source.to_owned()))
}

/// A script's `frames[N].scrollTo(x, y)` of the case `wheel-scroll`: the
/// frame's document scrolls (a script cannot do it in swb yet).
fn frame_scroll_step(page: &mut Page, step: &Value) -> bool {
    let Some(source) = step.get("eval").and_then(Value::as_str) else {
        return false;
    };
    let Some(rest) = source.strip_prefix("frames[") else {
        return false;
    };
    let Some((index, args)) = rest.split_once("].scrollTo(") else {
        return false;
    };
    let numbers: Vec<f32> = args
        .trim_end_matches(')')
        .split(',')
        .filter_map(|n| n.trim().parse().ok())
        .collect();
    let (Ok(index), [x, y]) = (index.parse::<usize>(), numbers.as_slice()) else {
        return false;
    };
    let Some(&frame) = page.frame_children(FrameId::TOP).get(index) else {
        return false;
    };
    let point = swb_engine::Point::new(*x, *y);
    page.with_frame(frame, |p| p.scroll_to(point));
    true
}

/// Runs the steps of a case: the snapshot steps push what the engine
/// shows.
fn run(page: &mut Page, case: &Value) -> Result<(), String> {
    for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        if let Some((key, source)) = snapshot_key(step) {
            let text = snapshot(page, &key, &source);
            let text = if key == "frames" {
                text
            } else {
                format!("{key}:{text}")
            };
            let literal = serde_json::to_string(&text).unwrap();
            value(page, &format!("log.push({literal})"));
            continue;
        }
        if frame_scroll_step(page, step) {
            continue;
        }
        run_steps(page, std::slice::from_ref(step)).map_err(|e| format!("step {}: {e}", i + 1))?;
    }
    Ok(())
}

/// The value of a query, run in swb (a snapshot query is not).
fn query(page: &mut Page, source: &str) -> Value {
    value(page, source)
}

/// The `js` values that Chromium gave for the case.
fn expected(name: &str) -> Vec<Value> {
    chromium_file()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no Chromium values for {name}"))["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "js")
        .map(|r| r["value"].clone())
        .collect()
}

/// The `p:` entries of the log of the case `focus` (the part that the
/// page's own scripts log) and its `state:` entry.
fn page_events(log: &str) -> String {
    log.split(' ')
        .filter(|e| e.starts_with("p:") || e.starts_with("state:"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The cases that this test runs: `focus`, `wheel-scroll` and `input-*`.
fn case_names() -> Vec<String> {
    probe_file()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .filter(|n| n.starts_with("input-") || n == "focus" || n == "wheel-scroll")
        .collect()
}

fn case(name: &str) -> Value {
    probe_file()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}

/// Runs the case and compares each query with Chromium's value.
fn check(name: &str) -> Vec<String> {
    let case = case(name);
    let mut page = open(&case);
    let mut failures = Vec::new();
    if let Err(error) = run(&mut page, &case) {
        failures.push(format!("{name}: {error}"));
        return failures;
    }
    let want = expected(name);
    for (index, q) in case["measure"].as_array().unwrap().iter().enumerate() {
        let source = q["js"].as_str().unwrap();
        if source.contains(LATER_MARK) || source.contains("c:wheel") {
            continue;
        }
        let want = &want[index];
        let got = match name {
            "focus" => focus_query(&mut page, index),
            n if n.starts_with("input-link-cross-origin-") => link_query(&mut page, true),
            n if n.starts_with("input-link-") => link_query(&mut page, false),
            n if n.starts_with("input-target-") && index == 0 => target_query(&mut page),
            _ => query(&mut page, source),
        };
        let (want, got) = match name {
            // The query reads the frame's document and the whole log: the
            // page's events are compared, the frame's wait for feature 12.
            "focus" => (focus_expected(want), got),
            _ => (normalize(want), got),
        };
        let departure = DEPARTURES.iter().find(|d| d.0 == name && d.1 == index);
        match departure {
            Some(d) => {
                // A departure that no longer happens is a failure too.
                if got != Value::String(d.2.to_owned()) {
                    failures.push(format!(
                        "{name} #{index}: expected the departure {:?} ({}), got {got}",
                        d.2, d.3
                    ));
                }
            }
            None if got != want => failures.push(format!(
                "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
            )),
            None => {}
        }
    }
    failures
}

/// swb's value of the query of the cases `input-link-*`: the state of the
/// page and its frames after the click, as the probe's expression reads
/// it. With `cross_origin`, the expression of the cross-origin cases (the
/// location of a cross-origin frame is not readable).
fn link_query(page: &mut Page, cross_origin: bool) -> Value {
    let top = FrameId::TOP;
    let own = value(
        page,
        "location.pathname.split('/').pop() + location.search + location.hash",
    );
    let own = own.as_str().unwrap_or_default().to_owned();
    let rest = match (frame_of(page, top, "f"), frame_of(page, top, "g")) {
        (Some(f), Some(g)) => {
            let f_doc = page.frame_document(f).unwrap();
            let g_doc = page.frame_document(g).unwrap();
            let last = |url: &Url| {
                url.as_str()
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            };
            if cross_origin {
                let same =
                    f_doc.origin_string() == page.frame_document(top).unwrap().origin_string();
                let f_part = if same {
                    last(f_doc.url())
                } else {
                    "SecurityError".to_owned()
                };
                format!("{f_part} | {}", last(g_doc.url()))
            } else {
                let g_url = g_doc.url();
                let g_part = format!(
                    "{}{}",
                    g_url.path().rsplit('/').next().unwrap_or_default(),
                    g_url.query().map_or(String::new(), |q| format!("?{q}"))
                );
                format!(
                    "{} | {g_part} | {}",
                    short_url(f_doc.url()),
                    f_doc.scroll_position().y
                )
            }
        }
        _ => "gone".to_owned(),
    };
    Value::String(format!("{own} / {rest}"))
}

/// The last part of the URL of `frame`, as a script of the page reads it:
/// `SecurityError` if the frame's document has another origin.
fn frame_location(page: &Page, frame: FrameId) -> String {
    let doc = page.frame_document(frame).unwrap();
    let top = page.frame_document(FrameId::TOP).unwrap();
    if doc.origin_string() != top.origin_string() {
        return "SecurityError".to_owned();
    }
    let url = doc.url();
    url.as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// swb's value of the first query of the cases `input-target-*`: the
/// location of the page, of the frames `f` and `g` and their first children,
/// and of the frame `k`.
fn target_query(page: &mut Page) -> Value {
    let top = FrameId::TOP;
    let own = value(page, "location.pathname.split('/').pop() + location.search");
    let own = own.as_str().unwrap_or_default().to_owned();
    let Some(f) = frame_of(page, top, "f") else {
        return Value::String(format!("{own} / gone"));
    };
    let g = frame_of(page, top, "g").unwrap();
    let kid = |page: &Page, frame: FrameId| {
        page.frame_children(frame)
            .first()
            .map_or("-".to_owned(), |&child| frame_location(page, child))
    };
    let k = frame_of(page, top, "k").map_or("-".to_owned(), |k| frame_location(page, k));
    Value::String(
        [
            own,
            frame_location(page, f),
            kid(page, f),
            frame_location(page, g),
            kid(page, g),
            k,
        ]
        .join(" | "),
    )
}

/// Chromium's value of the query of the case `focus` without the entries of
/// the frame's scripts.
fn focus_expected(want: &Value) -> Value {
    let mut items = want.as_array().unwrap().clone();
    if let Some(Value::String(log)) = items.last_mut() {
        *log = page_events(log);
    }
    Value::Array(items)
}

/// swb's value of the query of the case `focus`.
fn focus_query(page: &mut Page, _index: usize) -> Value {
    let top = FrameId::TOP;
    let f = frame_of(page, top, "f").unwrap();
    let doc = page.frame_document(f).unwrap();
    let c = doc.dom().element_by_id("c").unwrap();
    let frame_active = if active(page, f) == "BODY" {
        "BODY".to_owned()
    } else {
        active(page, f)
    };
    let parent_active = value(page, "document.activeElement.id");
    let parent_active: Value = parent_active;
    let log = value(page, "log.join(' ')");
    let log = log.as_str().unwrap_or_default().to_owned();
    serde_json::json!([
        parent_active,
        frame_active,
        window_focus(page),
        has_focus(page, f),
        page.frame_document(f)
            .unwrap()
            .control_value(c)
            .unwrap_or_default(),
        log
    ])
}

/// Chromium's value with the page's file name of the probe (`case-13.html`)
/// as the simulated server names it (`case.html`).
fn normalize(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(strip_case_number(s)),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

fn strip_case_number(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("case-") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 5..]
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        if digits > 0 && rest[at + 5 + digits..].starts_with(".html") {
            out.push_str("case");
            rest = &rest[at + 5 + digits..];
        } else {
            out.push_str("case-");
            rest = &rest[at + 5..];
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn input_cases_match_chromium() {
    on_page_thread(|| {
        let mut failures = Vec::new();
        for name in case_names() {
            failures.extend(check(&name));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn every_case_has_chromium_values() {
    let data = chromium_file();
    let have: Vec<&str> = data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    for name in case_names() {
        assert!(have.contains(&name.as_str()), "no values for {name}");
    }
}

/// A page with a frame `f` that has a frame `g`; `c` is a checkbox of `g`.
fn nested_case() -> Value {
    serde_json::json!({
        "html": "<!DOCTYPE html><style>body{margin:0}iframe{position:absolute;border:0}</style>\
                 <iframe id=f src=f.html style='left:100px;top:50px;width:300px;height:200px'></iframe>\
                 <p id=notframe>x</p>",
        "resources": {
            "f.html": {"text": "<!DOCTYPE html><style>body{margin:0}</style>\
                 <iframe id=g src=g.html style='position:absolute;left:20px;top:30px;width:200px;height:100px;border:0'></iframe>"},
            "g.html": {"text": "<!DOCTYPE html><style>body{margin:0}</style>\
                 <input id=c type=checkbox style='position:absolute;left:10px;top:10px'>"}
        }
    })
}

#[test]
fn frame_paths_find_the_frame_of_a_step_and_of_an_evaluation() {
    on_page_thread(|| {
        let mut page = open(&nested_case());
        let f = swb_automation::resolve_frame(&mut page, "#f").unwrap();
        let g = swb_automation::resolve_frame(&mut page, "#f >> #g").unwrap();
        assert_eq!(page.frame_parent(g), Some(f));
        assert_eq!(page.frame_parent(f), Some(FrameId::TOP));
        // A step with a frame clicks in it.
        let steps: Vec<Value> =
            serde_json::from_str(r##"[{"click": "#c", "frame": "#f >> #g"}]"##).unwrap();
        run_steps(&mut page, &steps).unwrap();
        let c = page
            .frame_document(g)
            .unwrap()
            .dom()
            .element_by_id("c")
            .unwrap();
        assert_eq!(
            page.frame_document(g).unwrap().control_checked(c),
            Some(true)
        );
        // The ways a path fails.
        let error =
            |page: &mut Page, path: &str| swb_automation::resolve_frame(page, path).unwrap_err();
        assert!(error(&mut page, "#nope").contains("no iframe matches"));
        assert!(error(&mut page, "#notframe").contains("is not an iframe"));
        assert!(error(&mut page, "#f >> #g >> #c").contains("is not an iframe"));
        assert!(error(&mut page, "#f >> ").contains("empty selector"));
        assert!(error(&mut page, "##").contains("not a valid selector"));
        // A step with a frame that finds none, and a kind that takes none.
        let bad: Vec<Value> =
            serde_json::from_str(r##"[{"click": "#c", "frame": "#nope"}]"##).unwrap();
        assert!(
            run_steps(&mut page, &bad)
                .unwrap_err()
                .contains("no iframe matches")
        );
        let bad: Vec<Value> = serde_json::from_str(r##"[{"type": "a", "frame": "#f"}]"##).unwrap();
        assert!(
            run_steps(&mut page, &bad)
                .unwrap_err()
                .contains("does not take a frame")
        );
    });
}
