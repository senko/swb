//! Input in frames against Chromium (M9 feature 11, ADR 0031 section 8):
//! the cases `focus`, `wheel-scroll` and `input-*` of
//! `tools/probes/web-frames.json` run here with the same input steps
//! (`swb_automation::run_steps`, the `input.*` methods) against a
//! simulated server, and the values of their queries are the ones that
//! Chromium 148 gave (`engine/tests/data/web-frames.json`: `uv run
//! --project tools swbtools probe --json tools/probes/web-frames.json
//! --case NAME ...`, three equal runs, merged with the same indent).
//!
//! The scripts of frame documents get the events (M9 feature 12b2: the `c:`
//! entries of the logs). Two things go through the engine API: the selection,
//! because swb has no `getSelection` (the `eval` steps `log.push('slN:' + ...)`
//! are snapshots, and the test pushes the string that Chromium's expression
//! gives), and a cross-origin frame, which has no realm until feature 13.
//! `DEPARTURES` lists the queries where swb's value differs on purpose, with the
//! reason.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::{on_page_thread, value};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{FontContext, FrameId, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

const MAIN: &str = "http://127.0.0.1:8000";
const OTHER: &str = "http://127.0.0.1:8001";

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

/// The string that the snapshot `key` of a case gives in Chromium.
fn snapshot(page: &mut Page, key: &str) -> String {
    let kind = key.trim_end_matches(|c: char| c.is_ascii_digit());
    content_snapshot(page, kind).unwrap_or_else(|| panic!("no snapshot {kind:?} for {key}"))
}

/// The form values and the selections.
fn content_snapshot(page: &mut Page, kind: &str) -> Option<String> {
    let top = FrameId::TOP;
    let f = frame_of(page, top, "f")?;
    Some(match kind {
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

/// For a step that reads a selection, `{"eval": "log.push('slN:' + ...)"}`: the
/// key and the source.
fn snapshot_key(step: &Value) -> Option<String> {
    let source = step.get("eval")?.as_str()?;
    let rest = source.strip_prefix("log.push('sl")?;
    Some(format!("sl{}", rest.split(':').next()?))
}

/// Runs the steps of a case: the snapshot steps push what the engine
/// shows.
fn run(page: &mut Page, case: &Value) -> Result<(), String> {
    for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        if let Some(key) = snapshot_key(step) {
            let text = snapshot(page, &key);
            let literal = serde_json::to_string(&format!("{key}:{text}")).unwrap();
            value(page, &format!("log.push({literal})"));
            continue;
        }
        run_steps(page, std::slice::from_ref(step)).map_err(|e| format!("step {}: {e}", i + 1))?;
    }
    Ok(())
}

/// The value of a query, run in swb (a snapshot query is not).
fn query(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, swb_engine::JsonForm::Strict) {
        Some(swb_engine::Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => Value::String(format!("{other:?}")),
    }
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

/// The cases that this test runs: `focus`, `wheel-scroll` and `input-*`.
fn case_names() -> Vec<String> {
    probe_file()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .filter(|n| {
            n.starts_with("input-")
                || n.starts_with("activation-")
                || n.starts_with("javascript-link-")
                || n.starts_with("window-name-")
                || n == "focus"
                || n == "wheel-scroll"
        })
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
        let want = &want[index];
        let got = match name {
            // A cross-origin frame has no realm until feature 13: the engine
            // tells what the frame shows.
            n if n.starts_with("input-link-cross-origin-") => link_query(&mut page),
            n if n.starts_with("input-target-") && index == 0 => target_query(&mut page),
            _ => normalize(&query(&mut page, source)),
        };
        let want = normalize(want);
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

/// swb's value of the query of the cases `input-link-cross-origin-*`: the state
/// of the page and its frames after the click, as the probe's expression reads
/// it (the location of a cross-origin frame is not readable).
fn link_query(page: &mut Page) -> Value {
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
            let same = f_doc.origin_string() == page.frame_document(top).unwrap().origin_string();
            let f_part = if same {
                last(f_doc.url())
            } else {
                "SecurityError".to_owned()
            };
            format!("{f_part} | {}", last(g_doc.url()))
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
