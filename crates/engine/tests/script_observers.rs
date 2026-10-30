//! `IntersectionObserver` and `ResizeObserver` in the rendering update (M8
//! feature 9a, ADR 0028 sections 7 and 13): the cases of the Chromium probes
//! `tools/probes/web-intersection-observer.json` and `web-resize-observer.json`
//! (the interfaces, their options and errors, the entries for the first
//! notification, scrolling in the viewport and in scroll containers, clipping,
//! explicit roots, margins and thresholds, sizes and boxes, the loop limit and
//! its error, the order against animation frame callbacks, timers and
//! microtasks), in virtual time; and tests of frames that observers need.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::Fetcher;

mod common;
#[path = "observers_io/cases.rs"]
mod io_cases;
#[path = "observers_ro/cases.rs"]
mod ro_cases;
use common::{TestSite, finish_loading, new_page};

/// A probe case with Chromium's values: its page and its queries (query,
/// value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// A value where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
type Departure = (&'static str, usize, &'static str, &'static str);

const URL: &str = "https://site.test/page.html";

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A page with JavaScript on and a virtual clock that shows `html` in a
/// viewport of 400 by 200 px (the size of the probes).
fn load(html: &str) -> Page {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(400.0, 200.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(
        ClockMode::Virtual,
        Some(1_767_225_600_000.0),
    ));
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

#[allow(dead_code)]
fn value(page: &mut Page, source: &str) -> String {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    }
}

/// Runs the cases; a departure replaces Chromium's value.
fn run_cases(cases: &'static [Case], departures: &'static [Departure]) {
    on_page_thread(move || {
        let mut failures = Vec::new();
        for case in cases {
            let mut page = load(case.html);
            for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                let got = match page.evaluate(query, JsonForm::Strict) {
                    Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
                    Some(Evaluation::Error(text)) => {
                        serde_json::Value::String(format!("error: {text}"))
                    }
                    None => serde_json::Value::String("error: no script runtime".to_owned()),
                };
                let want_text = departures
                    .iter()
                    .find(|d| d.0 == case.name && d.1 == index)
                    .map_or(chromium, |d| d.2);
                let want = if want_text.starts_with("error: ") {
                    serde_json::Value::String(want_text.to_owned())
                } else {
                    serde_json::from_str(want_text).unwrap()
                };
                if got != want {
                    failures.push(format!(
                        "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                        case.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

const IO_DEPARTURES: &[Departure] = &[(
    "io-clip",
    8,
    r#"["cb 1","c true 1 10,20,100,100 10,20,100,100 0,0,400,200 number"]"#,
    "`clip-path` does not clip the intersection (Chromium clips to the clip path's box)",
)];

const RO_DEPARTURES: &[Departure] = &[
    (
        "ro-basic",
        6,
        r#"["false,true,true,1,true,false,true,true,number,true"]"#,
        "`contentBoxSize` and the other arrays are new at each read (Chromium keeps one array per entry)",
    ),
    (
        "ro-basic",
        16,
        r#"["cb 1","e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50","-- writing mode"]"#,
        "swb has no `writing-mode`: the inline size stays the width",
    ),
];

#[test]
fn intersection_observer_probe_cases() {
    run_cases(io_cases::CASES, IO_DEPARTURES);
}

#[test]
fn resize_observer_probe_cases() {
    run_cases(ro_cases::CASES, RO_DEPARTURES);
}

/// Runs the event loop for `ms` of page time.
fn run(page: &mut Page, ms: u64) {
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(20));
}

const TALL: &str = "<!DOCTYPE html><style>html,body{margin:0} #t{position:absolute;left:0;top:500px;width:50px;height:50px}
  #box{position:absolute;left:0;top:700px;width:100px;height:100px;overflow:auto} #in{height:500px} #far{height:40px;margin-top:300px}</style>
  <div id=t></div><div id=box><div id=in><div id=far></div></div></div><div style='height:3000px'></div>
  <script>window.log = [];
  new IntersectionObserver((es) => es.forEach((e) => log.push('page ' + e.isIntersecting + ' ' + e.intersectionRatio + ' ' + e.boundingClientRect.y)), {threshold: [0, 1]}).observe(document.getElementById('t'));
  new IntersectionObserver((es) => es.forEach((e) => log.push('box ' + e.isIntersecting + ' ' + e.intersectionRatio + ' ' + e.boundingClientRect.y)), {root: document.getElementById('box'), threshold: [0, 1]}).observe(document.getElementById('far'));
  </script>";

#[test]
fn scrolling_the_viewport_notifies_in_the_next_frame() {
    on_page_thread(|| {
        let mut page = load(TALL);
        run(&mut page, 100);
        assert_eq!(
            value(&mut page, "log.join('|')"),
            r#""page false 0 500|box false 0 1000""#
        );
        page.scroll_to(swb_engine::Point::new(0.0, 400.0));
        run(&mut page, 100);
        assert_eq!(
            value(&mut page, "log.slice(2).join('|')"),
            r#""page true 1 100""#
        );
        page.scroll_to(swb_engine::Point::new(0.0, 0.0));
        run(&mut page, 100);
        assert_eq!(
            value(&mut page, "log.slice(3).join('|')"),
            r#""page false 0 500""#
        );
    });
}

#[test]
fn scrolling_a_scroll_container_notifies_its_observers() {
    on_page_thread(|| {
        let mut page = load(TALL);
        run(&mut page, 100);
        page.scroll_to(swb_engine::Point::new(0.0, 650.0));
        run(&mut page, 100);
        // The container is in view, the target in it is not yet: it is 300
        // px down in a container of 100 px.
        assert_eq!(value(&mut page, "log.slice(2).join('|')"), r#""""#);
        value(
            &mut page,
            "document.getElementById('box').scrollTop = 280, 1",
        );
        run(&mut page, 100);
        assert_eq!(
            value(&mut page, "log.slice(2).join('|')"),
            r#""box true 1 70""#
        );
    });
}

#[test]
fn a_style_change_in_a_timer_reaches_the_observers_without_animation_frames() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><style>html,body{margin:0}#a{width:100px;height:20px}</style><div id=a></div>
             <script>window.log = [];
             new ResizeObserver((es) => log.push('ro ' + es[0].contentRect.width)).observe(a);
             new IntersectionObserver((es) => log.push('io ' + es[0].isIntersecting)).observe(a);
             setTimeout(() => { a.style.width = '60px'; a.style.display = 'none'; }, 500);</script>",
        );
        run(&mut page, 300);
        assert_eq!(value(&mut page, "log.join()"), r#""ro 100,io true""#);
        run(&mut page, 500);
        assert_eq!(
            value(&mut page, "log.join()"),
            r#""ro 100,io true,ro 0,io false""#
        );
    });
}

#[test]
fn a_page_with_observers_goes_idle() {
    on_page_thread(|| {
        let mut page = load(TALL);
        assert!(page.wait_until_idle(Duration::from_millis(200), Duration::from_secs(20)));
        // Observers that wait for changes do not keep frames running.
        let before = value(&mut page, "log.length");
        run(&mut page, 1000);
        assert_eq!(value(&mut page, "log.length"), before);
    });
}

#[test]
fn a_resize_observer_that_resizes_its_target_without_end_is_bounded() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><style>html,body{margin:0}#a{width:100px;height:20px}</style><div id=a></div>
             <script>window.n = 0; window.errors = 0;
             window.addEventListener('error', (e) => { errors++; e.preventDefault(); });
             new ResizeObserver(() => { n++; a.style.width = (a.offsetWidth + 1) + 'px'; }).observe(a);</script>",
        );
        run(&mut page, 500);
        let n: i64 = value(&mut page, "n").parse().unwrap();
        let errors: i64 = value(&mut page, "errors").parse().unwrap();
        // One callback and one loop error per frame (60 frames a second).
        assert!((20..=40).contains(&n), "n = {n}");
        assert!((n - errors).abs() <= 1, "n = {n}, errors = {errors}");
    });
}

/// Many targets: the first computation and a scroll finish in reasonable
/// time (one frame computes all).
#[test]
fn many_intersection_targets_are_computed_in_a_frame() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><style>html,body{margin:0}.t{height:2px}</style><div id=h></div>
             <script>window.hits = 0;
             const f = document.createDocumentFragment();
             for (let i = 0; i < 20000; i++) { const d = document.createElement('div'); d.className = 't'; f.appendChild(d); }
             document.getElementById('h').appendChild(f);
             const o = new IntersectionObserver((es) => { hits += es.length; });
             for (const d of document.querySelectorAll('.t')) o.observe(d);</script>",
        );
        let start = std::time::Instant::now();
        run(&mut page, 200);
        page.scroll_to(swb_engine::Point::new(0.0, 20000.0));
        run(&mut page, 200);
        assert!(value(&mut page, "hits").parse::<u32>().unwrap() >= 20000);
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "{:?}",
            start.elapsed()
        );
    });
}
