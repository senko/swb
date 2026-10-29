//! The rendered text and the rendering update for scripts (M8 feature 7b2,
//! ADR 0028 sections 7 and 9 and "Feature 7b2 notes"): every case of the
//! Chromium probes `tools/probes/web-inner-text.json` (`innerText` and
//! `outerText`, getters and setters), `web-match-media.json` (`matchMedia`,
//! the serialization of media queries, `MediaQueryList` and its `change`
//! events after a resize and a color scheme change) and
//! `web-render-update.json` (`requestAnimationFrame`, the order of the
//! steps of a frame), loaded with the bundled test fonts in an 800x600
//! viewport in virtual time; the flush counters of `innerText`; a
//! `requestAnimationFrame` loop in virtual time; the task deadline of a
//! callback that never returns. Scripts run on a thread with a 16 MiB
//! stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "rendering/cases.rs"]
mod cases;
mod common;
#[path = "rendering/departures.rs"]
mod departures;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

use common::new_page;
use departures::SWB_VALUES;

/// A probe case with Chromium's values: its input steps (kind, JSON
/// argument) and its queries (query, value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) steps: &'static [(&'static str, &'static str)],
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

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

/// A page with JavaScript on and virtual time that shows `html`, loaded.
fn open(html: &'static str) -> Page {
    let mut page = new_page(Arc::new(OnePage(html)), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
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

/// Runs a step of a case as the probe's `--steps` does (the kinds that the
/// cases use).
fn run_step(page: &mut Page, kind: &str, argument: &str) {
    let argument: serde_json::Value = serde_json::from_str(argument).unwrap();
    match kind {
        "wait" => {
            let ms = argument.as_u64().unwrap();
            page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(10));
        }
        "eval" => {
            page.evaluate(argument.as_str().unwrap(), JsonForm::Strict);
        }
        "viewport" => {
            let size = argument.as_array().unwrap();
            let (w, h) = (size[0].as_f64().unwrap(), size[1].as_f64().unwrap());
            page.set_viewport(Size::new(w as f32, h as f32), 1.0);
        }
        "colorScheme" => page.set_color_scheme(argument == "dark"),
        other => panic!("the test has no step {other}"),
    }
    // The tasks that are due run after each step.
    page.run_event_loop(Duration::from_micros(1), Duration::from_secs(5));
}

/// Runs a case: loads it, runs its steps and compares each answer with
/// Chromium's.
fn run_case(case: &'static Case) -> Vec<String> {
    let mut page = open(case.html);
    for &(kind, argument) in case.steps {
        run_step(&mut page, kind, argument);
    }
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
        // calls of the probe.
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
fn inner_text_probe_cases() {
    run_cases(cases::INNER_TEXT);
}

#[test]
fn match_media_probe_cases() {
    run_cases(cases::MATCH_MEDIA);
}

#[test]
fn render_update_probe_cases() {
    run_cases(cases::RENDER_UPDATE);
}

#[test]
fn svg_geometry_probe_cases() {
    run_cases(cases::SVG_GEOMETRY);
}

/// `innerText` after a change forces one style run and no layout; reads
/// without a change force nothing more.
#[test]
fn inner_text_flushes_the_style_once_per_change() {
    on_page_thread(|| {
        let mut page = open("<!DOCTYPE html><div id=d><p>a</p><p>b</p></div>");
        page.update_layout();
        let before = page.flush_counters();
        let values = answer(
            &mut page,
            "d.lastChild.style.display = 'none'; [d.innerText, d.innerText, d.firstChild.innerText]",
        );
        assert_eq!(values, serde_json::json!(["a", "a", "a"]));
        let after = page.flush_counters();
        assert_eq!(after.forced_style_runs, before.forced_style_runs + 1);
        assert_eq!(after.forced_layout_runs, before.forced_layout_runs);
    });
}

/// A `requestAnimationFrame` loop runs one callback per frame of 16.67 ms
/// of virtual time: 60 frames in a second, with the frame times on the
/// grid.
#[test]
fn animation_frame_loop_counts_frames_in_virtual_time() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><script>var frames = 0, times = []; function f(t) { frames++; if (times.length < 4) times.push(t); requestAnimationFrame(f); } requestAnimationFrame(f);</script>",
        );
        let start = answer(&mut page, "frames");
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(20));
        let end = answer(&mut page, "frames");
        let ran = end.as_u64().unwrap() - start.as_u64().unwrap();
        assert!((59..=61).contains(&ran), "{ran} frames in a second");
        // The times are coarsened to 0.1 ms, so a step is 16.6 or 16.7 ms.
        let steps = answer(
            &mut page,
            "times.slice(1).map((t, i) => Math.round((t - times[i]) * 10)).every(d => d === 166 || d === 167)",
        );
        assert_eq!(steps, serde_json::json!(true));
        // A page with a `requestAnimationFrame` loop is idle when nothing
        // else waits.
        assert!(page.wait_until_idle(Duration::from_millis(100), Duration::from_secs(5)));
    });
}

/// A callback that never returns ends at the task's time limit; the next
/// callback of the frame and the next frames still run.
#[test]
fn animation_frame_callback_has_the_task_deadline() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><script>var log = []; requestAnimationFrame(() => { log.push('a'); for (;;) {} }); requestAnimationFrame(() => log.push('b'));</script>",
        );
        if let Some(runtime) = page.script_runtime_mut() {
            runtime.set_time_limit(Duration::from_millis(200));
        }
        let started = std::time::Instant::now();
        let log = answer(
            &mut page,
            "new Promise(r => requestAnimationFrame(() => r(log)))",
        );
        assert_eq!(log, serde_json::json!(["a", "b"]));
        assert!(started.elapsed() < Duration::from_secs(10));
    });
}

/// `change` fires after a resize only at the lists whose result changed,
/// once per frame, after `resize` and before the animation frame
/// callbacks; a list without listeners keeps nothing alive.
#[test]
fn media_query_changes_follow_the_viewport() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><script>var log = []; var m = matchMedia('(max-width: 600px)'); m.onchange = e => log.push('change ' + e.matches); addEventListener('resize', () => log.push('resize')); for (let i = 0; i < 10000; i++) matchMedia('(min-width: ' + i + 'px)');</script>",
        );
        page.set_viewport(Size::new(500.0, 600.0), 1.0);
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        page.set_viewport(Size::new(550.0, 600.0), 1.0);
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        page.set_viewport(Size::new(900.0, 600.0), 1.0);
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        assert_eq!(
            answer(&mut page, "log"),
            serde_json::json!(["resize", "change true", "resize", "resize", "change false"])
        );
    });
}
