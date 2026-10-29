//! Geometry and scrolling from scripts (M8 feature 7b, CSSOM View, ADR 0028
//! section 9 and "Feature 7b notes"): every case of the Chromium probes
//! `tools/probes/web-geometry.json` (`getBoundingClientRect`,
//! `getClientRects`, `offset*`, `client*`, `scroll*` of each kind of box,
//! rounding, quirks mode, `DOMRect`, the window's metrics,
//! `elementFromPoint`, `offsetX` of dispatched events, the scroll of
//! `focus()`, `img.width`) and `tools/probes/web-scroll.json` (the scroll
//! methods, clamping, `scrollIntoView`, the order of `scroll` events),
//! loaded with the bundled test fonts in an 800x600 viewport as the probe
//! does; the flush counters of geometry reads; the task deadline in a loop
//! of style writes and geometry reads. Scripts run on a thread with a 16 MiB
//! stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "geometry/cases.rs"]
mod cases;
mod common;
#[path = "geometry/departures.rs"]
mod departures;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

use common::new_page;

/// A probe case with Chromium's values: (query, value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

use departures::SWB_VALUES;

/// The URL of the case pages.
const PAGE: &str = "file:///case.html";

/// Serves one page; other URLs get a 404.
struct OnePage(&'static str);

impl Fetcher for OnePage {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (status, body) = if request.url.as_str() == PAGE {
            (200, self.0.as_bytes().to_vec())
        } else {
            (404, b"not found".to_vec())
        };
        let headers: Headers = [("content-type", "text/html")].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A page with JavaScript on that shows `html`, loaded.
fn open(html: &'static str) -> Page {
    let mut page = new_page(Arc::new(OnePage(html)), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// JSON text with the number tokens of the probe's domain outside JSON
/// (`NaN`, `Infinity`, `-Infinity`) as strings.
fn parse(text: &str) -> serde_json::Value {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if !in_string
            && let Some(token) = ["-Infinity", "Infinity", "NaN"]
                .into_iter()
                .find(|t| rest.starts_with(t))
        {
            out.push('"');
            out.push_str(token);
            out.push('"');
            rest = &rest[token.len()..];
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    serde_json::from_str(&out).unwrap()
}

/// The value of an evaluation as the probe prints it.
fn answer(page: &mut Page, expression: &str) -> serde_json::Value {
    match page.evaluate(expression, JsonForm::Probe) {
        Some(Evaluation::Value(text)) => parse(&text),
        Some(Evaluation::Error(text)) => serde_json::Value::String(format!("error: {text}")),
        None => serde_json::Value::String("error: no script runtime".to_owned()),
    }
}

/// Runs a case: loads it and compares each answer with Chromium's.
fn run_case(case: &'static Case) -> Vec<String> {
    let mut page = open(case.html);
    let mut failures = Vec::new();
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let want: serde_json::Value = if want.starts_with("error: ") {
            serde_json::Value::String(want.to_owned())
        } else {
            parse(want)
        };
        let got = answer(&mut page, query);
        // The tasks that are due run between two queries, as between two
        // calls of the probe (the notification of a rejected promise).
        page.run_event_loop(Duration::from_micros(1), Duration::from_secs(5));
        if got != want {
            failures.push(format!(
                "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
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

/// Runs the cases on threads of their own.
fn run_cases(cases: &'static [Case]) {
    let runs: Vec<_> = cases
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
fn geometry_probe_cases() {
    run_cases(cases::GEOMETRY);
}

#[test]
fn scroll_probe_cases() {
    run_cases(cases::SCROLL);
}

/// A geometry read after a change forces one style and one layout run;
/// reads without a change force nothing more, and a script scroll needs
/// no layout of its own.
#[test]
fn geometry_reads_flush_once_per_change() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><div id=d style='width: 50%; height: 3000px'>a</div><div id=e>b</div>",
        );
        page.update_layout();
        let before = page.flush_counters();
        let values = answer(
            &mut page,
            "d.style.width = '25%'; [d.offsetWidth, d.getBoundingClientRect().width, d.clientWidth, e.offsetTop, e.getClientRects().length]",
        );
        assert_eq!(values, serde_json::json!([196, 196, 196, 3008, 1]));
        let after = page.flush_counters();
        assert_eq!(after.forced_layout_runs, before.forced_layout_runs + 1);
        assert_eq!(after.forced_style_runs, before.forced_style_runs + 1);
        let scrolled = answer(
            &mut page,
            "scrollTo(0, 100); [scrollY, e.getBoundingClientRect().top]",
        );
        assert_eq!(scrolled, serde_json::json!([100, 2908]));
        assert_eq!(
            page.flush_counters().forced_layout_runs,
            after.forced_layout_runs
        );
    });
}

/// A loop that writes a style and reads the geometry in each turn lays out
/// in each turn inside one native call at a time; the binding checks the
/// task deadline after each read, so the loop ends at the time limit.
#[test]
fn style_write_geometry_read_loop_stops_at_the_deadline() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><div id=d>a</div><script>for (let i = 0; i < 3000; i++) document.body.appendChild(document.createElement('p')).textContent = 'x ' + i;</script>",
        );
        if let Some(runtime) = page.script_runtime_mut() {
            runtime.set_time_limit(Duration::from_millis(300));
        }
        let started = std::time::Instant::now();
        let result = answer(
            &mut page,
            "for (let i = 0; ; i++) { d.style.width = (i % 100) + 'px'; d.offsetWidth; } 1",
        );
        assert!(
            result.as_str().is_some_and(|s| s.starts_with("error")),
            "{result}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    });
}

/// A coordinate beyond the `f32` range scrolls to 0, one at the end of the
/// range to the end (Chromium, measured in the 7b1 review).
#[test]
fn scroll_coordinates_beyond_f32_are_zero() {
    on_page_thread(|| {
        let mut page = open("<!DOCTYPE html><style>body{margin:0;height:3000px}</style>x");
        let huge = "(() => { scrollTo(0, 1e300); const a = scrollY; scrollTo(0, 3.4e38); \
                    return [a, scrollY]; })()";
        assert_eq!(answer(&mut page, huge), serde_json::json!([0, 2400]));
    });
}

/// `scroll` events of script scrolls come in the next frame, in the order
/// the targets scrolled (the document after a container that scrolled
/// first), also for a scroll back to the old position.
#[test]
fn script_scrolls_fire_scroll_events_in_order() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><body style='margin:0'><div id=sc style='height:100px;overflow:auto'><div style='height:500px'></div></div><div style='height:3000px'></div>\
             <script>var log = []; addEventListener('scroll', () => log.push('window')); sc.addEventListener('scroll', () => log.push('sc'));</script>",
        );
        answer(
            &mut page,
            "sc.scrollTop = 10; scrollTo(0, 50); scrollTo(0, 0); log.push('sync'); 0",
        );
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(5));
        let log = answer(&mut page, "log");
        assert_eq!(log, serde_json::json!(["sync", "sc", "window"]));
    });
}
