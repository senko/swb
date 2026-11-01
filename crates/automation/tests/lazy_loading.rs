//! M9 feature 4 (ADR 0032): lazy loading of images against Chromium. Every
//! case of `tools/probes/web-lazy-loading.json` runs in the engine against
//! a simulated server (as the probe server answers: the `resources` of a
//! case under `/case-N/`, a delay per resource, `404` for the rest, and
//! `__log`, the requests since the page loaded), with the values of
//! Chromium 148 (`cases.rs`, written by `swbtools lazy-cases` from the
//! output of `swbtools probe --json`, three equal runs; docs/testing.md,
//! "Lazy loading tests"). `DEPARTURES` lists the queries where swb differs
//! on purpose.
//!
//! The cases measure the distances from the viewport (3000 px above and
//! below, 1500 px left and right), the boxes and containers that decide
//! (`display: none`, clipping, scroll containers, `details`), scrolling by
//! script, wheel and keyboard, the state of an element (src changes before
//! and after the load, `loading` toggles), `img.loading`, `complete` and
//! `currentSrc` of an image that has not started, the `load` event of the
//! window, the order of the `load` event of an image and a page
//! `IntersectionObserver`, `sizes="auto"`, `<picture>`, and a document
//! without scripting (nothing is lazy).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::on_page_thread;
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

#[path = "lazy_loading/cases.rs"]
mod cases;

const MAIN: &str = "http://main.test";

/// A resource of a case.
pub(crate) struct Res {
    pub(crate) path: &'static str,
    pub(crate) text: &'static str,
    pub(crate) delay_ms: u64,
    pub(crate) content_type: Option<&'static str>,
}

/// A case of the probe file with the values of Chromium.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) viewport: (f32, f32),
    pub(crate) javascript: bool,
    /// The input steps (JSON), run after the load.
    pub(crate) steps: &'static str,
    pub(crate) resources: &'static [Res],
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// A query where swb's value differs on purpose: (case, query index,
/// swb's value, why).
type Departure = (&'static str, usize, &'static str, &'static str);

const DEPARTURES: &[Departure] = &[
    (
        "scroll-key-end",
        0,
        "[175,100,3580]",
        "Chromium scrolls to the end with an animation and checks the images at each frame on the way (all 400 load); swb jumps to the end, and the images 3000 px above the last viewport are out of range",
    ),
    (
        "kinds-load",
        0,
        r#"["k-bogus-far.png","k-contents.png","k-cv-auto.png","k-cv-hidden.png","k-details-open.png","k-eager-far.png","k-fixed.png","k-none-far.png","k-opacity-0.png","k-picture-source.png","k-plain.png","k-right-2000.png","k-space-far.png","k-top-2000.png","k-upper.png","k-visibility-hidden.png","k-zero-css.png","k-zero.png"]"#,
        "swb has no `content-visibility`: the content of `content-visibility: hidden` has boxes, so its image loads (backlog)",
    ),
    (
        "detached-attr-order",
        0,
        "[]",
        "Chromium decides lazy or eager in a microtask after `src` is set, so `loading = 'lazy'` in a later microtask comes too late; swb decides when it processes the DOM changes, after the microtasks of the task",
    ),
    (
        "sizes-auto",
        0,
        r#"["sa-attr500-800.png","sa-block-400.png","sa-fallback-400.png","sa-nowidth-400.png","sa-w100-200.png","sa-w300-400.png","sa-w700-800.png"]"#,
        "after the first source failed (404), the box of an image without width has no width, and Chromium selects the 100vw source (800w) then; swb keeps the first selection",
    ),
    (
        "sizes-auto",
        1,
        r#"["sa-attr500-800.png","sa-block-400.png","sa-fallback-400.png","sa-far-400.png","sa-nowidth-400.png","sa-w100-200.png","sa-w300-400.png","sa-w700-800.png"]"#,
        "as the first query of this case",
    ),
    (
        "sizes-auto-resize",
        1,
        r#"["sz-400.png"]"#,
        "the headless runner paints nothing, so no layout follows the script's width change, and the source is selected again at the next layout (ADR 0032, the check); the GUI lays out for every frame with a DOM change",
    ),
];

/// The simulated probe server of one case.
struct Sim {
    folder: String,
    case: &'static Case,
    log: Mutex<Vec<String>>,
}

impl Fetcher for Sim {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let prefix = format!("/{}/", self.folder);
        let path = request.url.path().to_owned();
        let reply = |status: u16, content_type: &str, body: Vec<u8>| Response {
            url: request.url.clone(),
            status,
            headers: [("content-type".to_owned(), content_type.to_owned())]
                .into_iter()
                .collect::<Headers>(),
            body,
            redirected: false,
        };
        let Some(name) = path.strip_prefix(&prefix) else {
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        if name == format!("{}.html", self.folder) {
            return Ok(reply(200, "text/html", self.case.html.as_bytes().to_vec()));
        }
        if name == "__log" {
            let entries: Vec<Value> = self
                .log
                .lock()
                .unwrap()
                .iter()
                .map(|p| serde_json::json!({ "path": p, "method": "GET" }))
                .collect();
            return Ok(reply(
                200,
                "application/json",
                Value::Array(entries).to_string().into_bytes(),
            ));
        }
        self.log.lock().unwrap().push(name.to_owned());
        let Some(resource) = self.case.resources.iter().find(|r| r.path == name) else {
            return Ok(reply(404, "text/html", b"<!DOCTYPE HTML>".to_vec()));
        };
        // A real server needs some time even without a delay.
        std::thread::sleep(Duration::from_millis(resource.delay_ms.max(30)));
        let by_extension = match name.rsplit('.').next() {
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            _ => "text/plain",
        };
        Ok(reply(
            200,
            resource.content_type.unwrap_or(by_extension),
            resource.text.as_bytes().to_vec(),
        ))
    }
}

/// The value of a query of a case without scripting, from the log of the
/// server (Chromium's page asks the server with a synchronous request).
fn log_query(sim: &Sim, query: &str) -> Option<Value> {
    if query.contains("document.readyState") {
        return Some(Value::String("complete".to_owned()));
    }
    if !query.contains("__log") {
        return None;
    }
    let mut paths = sim.log.lock().unwrap().clone();
    if query.contains("new Set") {
        for path in &mut paths {
            // `name-400.png` -> `name`
            if let Some(cut) = path.rfind('-') {
                path.truncate(cut);
            }
        }
        paths.dedup();
    }
    paths.sort();
    paths.dedup();
    Some(Value::Array(paths.into_iter().map(Value::String).collect()))
}

fn evaluate(page: &mut Page, query: &str) -> Value {
    match page.evaluate(query, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Some(Evaluation::Error(text)) => Value::String(format!("error: {text}")),
        None => Value::String("no runtime".to_owned()),
    }
}

/// The failures of one case: a query whose value differs from Chromium's
/// and is not a departure.
fn run_case(case: &'static Case) -> Vec<String> {
    let folder = format!("case-{}", case.name);
    let sim = Arc::new(Sim {
        folder: folder.clone(),
        case,
        log: Mutex::new(Vec::new()),
    });
    let config = PageConfig {
        fetcher: Arc::clone(&sim) as Arc<dyn Fetcher>,
        notify: Arc::new(|| {}),
        network_threads: 3,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(case.viewport.0, case.viewport.1),
        1.0,
    );
    page.set_javascript(case.javascript);
    page.navigate(Url::parse(&format!("{MAIN}/{folder}/{folder}.html")).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    let steps: Vec<Value> = serde_json::from_str(case.steps).unwrap();
    run_steps(&mut page, &steps).unwrap();
    let mut out = Vec::new();
    for (index, (query, expected)) in case.queries.iter().enumerate() {
        let want = DEPARTURES
            .iter()
            .find(|(c, i, _, _)| *c == case.name && *i == index)
            .map_or(*expected, |d| d.2);
        let want: Value = serde_json::from_str(want).unwrap();
        let got = if case.javascript {
            evaluate(&mut page, query)
        } else {
            log_query(&sim, query).unwrap_or(Value::Null)
        };
        if got != want {
            out.push(format!(
                "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    out
}

#[test]
fn lazy_loading_probe_cases_match_chromium() {
    on_page_thread(|| {
        let runs: Vec<_> = cases::CASES
            .iter()
            .map(|case| {
                std::thread::Builder::new()
                    .stack_size(16 << 20)
                    .spawn(move || run_case(case))
                    .unwrap()
            })
            .collect();
        let failures: Vec<String> = runs
            .into_iter()
            .flat_map(|run| run.join().unwrap())
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
