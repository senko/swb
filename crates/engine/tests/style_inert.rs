//! The properties that scripts can read and write but that do not change
//! layout or paint (M8 feature 11c1, ADR 0028 section 10): `transition`
//! with its longhands and the `-webkit-` aliases, `will-change`,
//! `touch-action`, `perspective` and `perspective-origin`. Every case of the
//! Chromium probe `tools/probes/web-style-inert.json`: the values that
//! `element.style` writes, the shorthand and its longhands in a block,
//! feature tests, `CSS.supports`, and the computed values. Scripts run on a
//! thread with a 16 MiB stack, as in swb.
//!
//! The probe's Chromium disables transitions and animations with
//! `transition: none !important` in the page (`stop_animations`), so the
//! computed cases write their values with `!important` inline, which wins.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "style_inert/cases.rs"]
mod cases;
mod common;

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

/// Where swb departs from Chromium on purpose: (case, query index, swb's
/// value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[];

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
            serde_json::from_str(want).unwrap()
        };
        let got = answer(&mut page, query);
        if got != want {
            failures.push(format!(
                "{} #{index}: {}\n{}",
                case.name,
                query.chars().take(120).collect::<String>(),
                differences(&want, &got)
            ));
        }
    }
    failures
}

/// The items of two lists that differ (or both values).
fn differences(want: &serde_json::Value, got: &serde_json::Value) -> String {
    match (want.as_array(), got.as_array()) {
        (Some(w), Some(g)) if w.len() == g.len() => w
            .iter()
            .zip(g)
            .filter(|(w, g)| w != g)
            .map(|(w, g)| format!("  expected: {w}\n  actual:   {g}"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => format!("  expected: {want}\n  actual:   {got}"),
    }
}

#[test]
fn probe_cases() {
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

/// The cascade: sheet rules, `inherit` and `initial` reach the computed
/// style of the element, and a style change shows in a live computed
/// declaration.
#[test]
fn cascade_reaches_computed_style() {
    let page = "<!DOCTYPE html><style>#a { transition: opacity 2s linear 1s; will-change: transform } \
                #b { transition: inherit; will-change: inherit; touch-action: pan-x } \
                .c { transition-duration: 300ms } .d { transition-duration: 1s } \
                #e { -webkit-transition: width 3s } #f { perspective: 2em; font-size: 10px }</style>\
                <div id=a><div id=b></div></div><div id=c class='c d'></div><div id=e></div>\
                <div id=f><div id=g style='perspective: inherit'></div></div>";
    let page: &'static str = Box::leak(page.to_owned().into_boxed_str());
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let mut page = open(page);
            let got = answer(
                &mut page,
                "(() => { const v = (id, ...p) => { const s = getComputedStyle(document.getElementById(id)); return p.map(n => s.getPropertyValue(n)); }; return [v('a', 'transition-property', 'transition-duration', 'transition-timing-function', 'transition-delay', 'will-change'), v('b', 'transition', 'will-change', 'touch-action'), v('c', 'transition-duration'), v('e', 'transition-property', 'transition-duration'), v('f', 'perspective'), v('g', 'perspective')]; })()",
            );
            // The probe's page disables transitions with an important rule;
            // this page does not, so the sheet values show.
            assert_eq!(
                got,
                serde_json::json!([
                    ["opacity", "2s", "linear", "1s", "transform"],
                    ["opacity 2s linear 1s", "transform", "pan-x"],
                    ["1s"],
                    ["width", "3s"],
                    ["20px"],
                    ["20px"]
                ])
            );
            let live = answer(
                &mut page,
                "(() => { const e = document.getElementById('c'); const s = getComputedStyle(e); const a = s.transitionDuration; e.style.transitionDuration = '2s, 4s'; const b = s.transitionDuration; e.style.removeProperty('transition-duration'); return [a, b, s.transitionDuration]; })()",
            );
            assert_eq!(live, serde_json::json!(["1s", "2s, 4s", "1s"]));
        })
        .unwrap()
        .join()
        .unwrap();
}
