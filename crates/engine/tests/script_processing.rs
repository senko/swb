//! Scripts in the HTML parser (ADR 0028 section 8, M8 feature 5c): every
//! case of the Chromium probe `tools/probes/web-scripts.json` (the order of
//! parser-blocking, inline, deferred, async and inserted scripts with
//! style sheets, `readyState`, `currentScript`, `DOMContentLoaded`, `load`,
//! `pageshow`, script element events, skipped types, error positions),
//! served by a fetcher that delays each response as the probe's server
//! does; the load as `page.waitForLoad` sees it; the limit of script
//! fetches in flight; a stopped load. Scripts run on a thread with a
//! 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "script_processing/cases.rs"]
mod cases;
mod common;

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

use common::new_page;

/// A resource of a probe case: the probe's server sends it after its delay.
pub(crate) struct Resource {
    pub(crate) name: &'static str,
    pub(crate) text: &'static str,
    pub(crate) delay_ms: u64,
    pub(crate) content_type: &'static str,
}

/// A probe case with Chromium's values.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    /// True if the probe loads the case from a file (no `resources`); else
    /// over HTTP. "The end" of the parse differs (`document_load.rs`).
    pub(crate) file: bool,
    pub(crate) resources: &'static [Resource],
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// Where swb departs from Chromium on purpose: (case, query, swb's value,
/// why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[(
    "module-not-run",
    0,
    r#""i:l:i rsc:i dcl:i rsc:c load:c pageshow:c""#,
    "swb runs no module scripts (ADR 0028 section 6, item 7)",
)];

const ORIGIN: &str = "https://site.test/";

/// Serves pages and resources by URL after their delay, with the type of
/// each; other URLs get a 404. Counts the fetches in flight.
#[derive(Default)]
struct DelaySite {
    files: HashMap<String, (Vec<u8>, Duration, String)>,
    /// The base of the URLs; [`ORIGIN`] if empty.
    origin: &'static str,
    in_flight: AtomicUsize,
    most_in_flight: AtomicUsize,
}

impl DelaySite {
    fn origin(&self) -> &'static str {
        if self.origin.is_empty() {
            ORIGIN
        } else {
            self.origin
        }
    }

    fn add(&mut self, name: &str, body: &str, delay_ms: u64, content_type: &str) {
        self.files.insert(
            format!("{}{name}", self.origin()),
            (
                body.as_bytes().to_vec(),
                Duration::from_millis(delay_ms),
                content_type.to_owned(),
            ),
        );
    }
}

impl Fetcher for DelaySite {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_in_flight.fetch_max(now, Ordering::SeqCst);
        let file = self.files.get(request.url.as_str());
        if let Some((_, delay, _)) = file {
            std::thread::sleep(*delay);
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        let (status, body, kind) = match file {
            Some((body, _, kind)) => (200, body.clone(), kind.as_str()),
            None => (404, b"not found".to_vec(), "text/html"),
        };
        let headers: Headers = [("content-type", kind)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// A page with JavaScript on that loads `name` from `site`.
fn open(site: DelaySite, name: &str) -> (Page, Arc<DelaySite>) {
    let url = format!("{}{name}", site.origin());
    let site = Arc::new(site);
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        8,
        Size::new(400.0, 200.0),
    );
    page.set_javascript(true);
    page.navigate(Url::parse(&url).unwrap());
    (page, site)
}

/// The value of an evaluation as the probe prints it.
fn answer(page: &mut Page, expression: &str) -> serde_json::Value {
    match page.evaluate(expression, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        Some(Evaluation::Error(text)) => serde_json::Value::String(format!("error: {text}")),
        None => serde_json::Value::String("error: no script runtime".to_owned()),
    }
}

/// Runs a case: loads it and compares each answer with Chromium's.
fn run_case(case: &'static Case) -> Vec<String> {
    let mut site = DelaySite {
        origin: if case.file { "file:///" } else { ORIGIN },
        ..DelaySite::default()
    };
    site.add("case.html", case.html, 0, "text/html");
    for r in case.resources {
        site.add(r.name, r.text, r.delay_ms, r.content_type);
    }
    let (mut page, _) = open(site, "case.html");
    let mut failures = Vec::new();
    if !page.wait_until_loaded(Duration::from_secs(20)) {
        failures.push(format!("{}: the page did not load", case.name));
    }
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let want: serde_json::Value = if want.starts_with("error: ") {
            serde_json::Value::String(want.to_owned())
        } else {
            serde_json::from_str(want).unwrap()
        };
        let got = answer(&mut page, query);
        if got != want {
            failures.push(format!(
                "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
}

#[test]
fn probe_cases() {
    // The cases wait for their resources in real time; they run in
    // parallel.
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
}

#[test]
fn requests_after_the_load_event_do_not_delay_the_load() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        site.add(
            "page.html",
            "<!DOCTYPE html><script>addEventListener('load', function () { var s = document.createElement('script'); s.src = 'slow.js'; document.head.appendChild(s); });</script><p>x",
            0,
            "text/html",
        );
        site.add("slow.js", "window.slow = 1", 3000, "text/javascript");
        let (mut page, _) = open(site, "page.html");
        let started = Instant::now();
        assert!(page.wait_until_loaded(Duration::from_secs(20)));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert!(page.is_fully_loaded());
        // The script still loads and runs.
        assert!(page.is_loading());
        let value = page.evaluate_with_timeout(
            "new Promise(function (r) { var t = setInterval(function () { if (window.slow) { clearInterval(t); r(window.slow); } }, 20); })",
            JsonForm::Strict,
            Duration::from_secs(10),
        );
        assert_eq!(value, Some(Evaluation::Value("1".to_owned())));
    });
}

#[test]
fn the_load_waits_for_a_parser_blocking_script() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        site.add(
            "page.html",
            "<!DOCTYPE html><script src=b.js></script><p id=after>x</p><script>window.seen = document.body.childNodes.length</script>",
            0,
            "text/html",
        );
        site.add(
            "b.js",
            "window.before = document.body === null",
            300,
            "text/javascript",
        );
        let (mut page, _) = open(site, "page.html");
        let started = Instant::now();
        // While the parser waits, the page loads.
        page.process_network();
        assert!(page.is_loading());
        assert!(!page.is_fully_loaded());
        assert!(page.wait_until_loaded(Duration::from_secs(20)));
        assert!(started.elapsed() >= Duration::from_millis(250));
        assert_eq!(
            answer(&mut page, "[window.before, window.seen]"),
            serde_json::json!([true, 2])
        );
    });
}

#[test]
fn at_most_64_script_fetches_run_at_once() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        let mut html = String::from("<!DOCTYPE html><script>window.n = 0;</script>");
        for i in 0..100 {
            write!(html, "<script async src=s{i}.js></script>").unwrap();
            site.add(&format!("s{i}.js"), "window.n++", 30, "text/javascript");
        }
        site.add("page.html", &html, 0, "text/html");
        // More network threads than the limit, so that only the limit
        // holds the fetches back.
        let site = Arc::new(site);
        let mut page = new_page(
            Arc::clone(&site) as Arc<dyn Fetcher>,
            80,
            Size::new(400.0, 200.0),
        );
        page.set_javascript(true);
        page.navigate(Url::parse(&format!("{ORIGIN}page.html")).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        assert_eq!(answer(&mut page, "window.n"), serde_json::json!(100));
        let most = site.most_in_flight.load(Ordering::SeqCst);
        assert!(most <= 64, "{most} fetches at once");
        assert!(most > 8, "{most} fetches at once");
    });
}

#[test]
fn a_stopped_load_ends_the_parse() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        site.add(
            "page.html",
            "<!DOCTYPE html><p>a</p><script src=never.js></script><p>b</p>",
            0,
            "text/html",
        );
        site.add("never.js", "1", 5000, "text/javascript");
        let (mut page, _) = open(site, "page.html");
        let deadline = Instant::now() + Duration::from_secs(5);
        while page.document().is_none() && Instant::now() < deadline {
            page.process_network();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(page.is_loading());
        page.stop();
        assert!(!page.is_loading());
        assert!(page.is_fully_loaded());
        assert_eq!(
            answer(&mut page, "document.readyState"),
            serde_json::json!("complete")
        );
        // The parse stopped at the script.
        assert_eq!(
            answer(&mut page, "document.body.childNodes.length"),
            serde_json::json!(2)
        );
    });
}

/// Opens `page.html` and sets a time limit of 50 ms for the tasks of its
/// scripts, before any of them runs (the page starts with a blocking
/// script).
fn open_with_short_limit(site: DelaySite) -> Page {
    let (mut page, _) = open(site, "page.html");
    let deadline = Instant::now() + Duration::from_secs(5);
    while page.script_runtime_mut().is_none() && Instant::now() < deadline {
        page.process_network();
        std::thread::sleep(Duration::from_millis(2));
    }
    page.script_runtime_mut()
        .unwrap()
        .set_time_limit(Duration::from_millis(50));
    page
}

#[test]
fn stopped_scripts_while_the_parser_waits_let_the_parse_finish() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        site.add(
            "page.html",
            "<!DOCTYPE html><p>x</p><script src=timers.js></script>\
             <script src=slow.js></script><p id=after>after</p><script>window.ran = 1</script>",
            0,
            "text/html",
        );
        site.add(
            "timers.js",
            "for (let i = 0; i < 3; i++) setTimeout(function () { while (true); }, 0);",
            100,
            "text/javascript",
        );
        site.add("slow.js", "window.slow = 1", 1500, "text/javascript");
        let mut page = open_with_short_limit(site);
        let started = Instant::now();
        assert!(page.wait_until_loaded(Duration::from_secs(20)));
        // The scripts stopped, so the load does not wait for the slow one.
        assert!(started.elapsed() < Duration::from_millis(1400));
        assert!(page.is_fully_loaded());
        assert!(!page.is_loading());
        assert!(page.script_runtime_mut().unwrap().stopped());
        // The parse went on (evaluations of a stopped document fail).
        assert!(page.document().unwrap().element_by_id("after").is_some());
    });
}

#[test]
fn stopped_scripts_before_more_scripts_let_the_parse_finish() {
    on_page_thread(|| {
        let mut site = DelaySite::default();
        site.add(
            "page.html",
            "<!DOCTYPE html><p>x</p><script src=a1.js></script><script src=a2.js></script>\
             <script src=a3.js></script><script src=c.js></script>\
             <script>window.ran = 1</script><p id=after>after</p>",
            0,
            "text/html",
        );
        for name in ["a1.js", "a2.js", "a3.js"] {
            site.add(name, "while (true);", 50, "text/javascript");
        }
        site.add("c.js", "window.c = 1", 100, "text/javascript");
        let mut page = open_with_short_limit(site);
        assert!(page.wait_until_loaded(Duration::from_secs(20)));
        assert!(page.is_fully_loaded());
        assert!(page.script_runtime_mut().unwrap().stopped());
        assert!(page.document().unwrap().element_by_id("after").is_some());
    });
}
