//! Container queries (M10 sub-feature 7a, ADR 0038): the cases of the
//! Chromium probe `tools/probes/container-queries.json` and the three
//! `container-*` cases of `tools/probes/github-css.json`, with the answers
//! that Chromium 148 gave (`tests/data/container-queries.json` and
//! `tests/data/github-css-containers.json`: `swbtools probe --json`, three
//! equal runs).
//!
//! - `boxes` rows: the border box of an element (or none).
//! - `style` rows: `getComputedStyle` values.
//! - `js` rows: the expression's value (computed styles of descendants of
//!   containers, of pseudo-elements and of a dialog's backdrop, the CSSOM of
//!   `CSSContainerRule`, `CSS.supports`, a script's change of a container's
//!   size followed by `getComputedStyle` and `getBoundingClientRect`).
//!
//! The cases run with JavaScript on (the queries need it); the cases that
//! Chromium ran with JavaScript off have no scripts. [`DEPARTURES`] lists
//! the rows where swb differs on purpose, [`LATER`] those that wait for a
//! later feature.
//!
//! The engine tests of the passes, the bound and the forced flushes are at
//! the end.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::probe_rows::{box_row, load_cases, number, style_row_at_once, viewport_of};
use common::{OnePage, answer, differences, new_page, on_page_threads, settle};

/// Rows where swb differs on purpose: (case, row, why).
const DEPARTURES: &[(&str, usize, &str)] = &[
    (
        "units-vertical-writing-mode",
        1,
        "swb has no writing-mode: cqi is cqw and the axes are horizontal",
    ),
    (
        "units-vertical-writing-mode",
        2,
        "swb has no writing-mode: cqi is cqw and the axes are horizontal",
    ),
    (
        "supports-container-type",
        2,
        "`CSS.supports('at-rule(@container)')`: swb has no `at-rule()` in `@supports` (backlog)",
    ),
    (
        "cssom-container-rule",
        0,
        "`CSSStyleRule.cssRules` (CSS nesting) is undefined in swb; the rows of the container rules match",
    ),
];

/// Rows that wait for a later feature: (case, row, why).
const LATER: &[(&str, usize, &str)] = &[
    (
        "rule-nesting",
        4,
        "CSS nesting (`#t5 { @container ... }`): swb drops nested rules",
    ),
    (
        "style-query-recorded",
        0,
        "style queries (out of scope: GitHub uses none)",
    ),
    (
        "style-query-recorded",
        1,
        "style queries (out of scope: GitHub uses none)",
    ),
    (
        "style-query-recorded",
        2,
        "style queries (out of scope: GitHub uses none)",
    ),
];

/// The largest difference of a box coordinate from Chromium's, in px.
const TOLERANCE: f64 = 0.5;

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(true);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// True if the values are equal, numbers within 0.05.
fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() <= 0.05
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b))
        }
        _ => a == b,
    }
}

fn check_js(case: &str, row: usize, page: &mut Page, result: &Value) -> Option<String> {
    let query = result["label"].as_str().unwrap();
    let want = &result["value"];
    let got = answer(page, query);
    (!close(&got, want)).then(|| {
        format!(
            "{case} #{row}: {}\n{}",
            &query[..query.len().min(160)],
            differences(want, &got)
        )
    })
}

fn listed(list: &[(&str, usize, &str)], case: &str, row: usize) -> bool {
    list.iter().any(|&(c, r, _)| c == case && r == row)
}

/// Runs a case: its steps (a viewport change), then its rows.
fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let mut page = open(case["html"].as_str().unwrap(), viewport_of(results));
    for step in case["steps"].as_array().into_iter().flatten() {
        if let Some(size) = step["viewport"].as_array() {
            page.set_viewport(
                Size::new(number(&size[0]) as f32, number(&size[1]) as f32),
                1.0,
            );
            settle(&mut page);
        }
    }
    let mut failures = Vec::new();
    for (row, result) in results["results"].as_array().unwrap().iter().enumerate() {
        let failure = match result["kind"].as_str().unwrap() {
            "js" => check_js(name, row, &mut page, result),
            "boxes" => box_row(name, row, &mut page, result, TOLERANCE),
            "style" => style_row_at_once(name, row, &mut page, result),
            other => Some(format!("{name} #{row}: unknown kind {other}")),
        };
        let skipped = listed(DEPARTURES, name, row) || listed(LATER, name, row);
        match (failure, skipped) {
            (Some(f), false) => failures.push(f),
            (None, true) => failures.push(format!(
                "{name} #{row} matches Chromium now: remove it from DEPARTURES or LATER"
            )),
            _ => {}
        }
    }
    failures
}

#[test]
fn container_query_probe_cases_match_chromium() {
    let mut cases = load_cases(
        "tools/probes/container-queries.json",
        include_str!("data/container-queries.json"),
    );
    cases.extend(load_cases(
        "tools/probes/github-css.json",
        include_str!("data/github-css-containers.json"),
    ));
    let runs = on_page_threads(
        cases
            .into_iter()
            .map(|(case, answer)| move || run_case(case, answer)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A page whose container `#c` holds `#t`; `#t` is green while the
/// container is wider than 300px.
const RESIZE_PAGE: &str = "<!DOCTYPE html><style>body{margin:0} #c{container-type:inline-size;width:400px}\
    #t{color:rgb(255,0,0)} @container (width > 300px) { #t{color:rgb(0,128,0)} }\
    #o{width:50px}</style><div id=c><p id=t>t</p></div><div id=o><p id=out>o</p></div>";

/// The first layout styles with unknown sizes, so a query that is true
/// at the real size costs one extra pass; a later change that keeps the
/// results costs none.
#[test]
fn a_true_query_costs_one_extra_pass_and_a_stable_one_none() {
    common::on_page_thread(|| {
        let mut page = open(RESIZE_PAGE, Size::new(800.0, 600.0));
        settle(&mut page);
        let first = page.flush_counters();
        assert_eq!(first.container_passes, 1);
        assert_eq!(
            answer(&mut page, "getComputedStyle(t).color"),
            "rgb(0, 128, 0)"
        );
        // A change of the container's width that keeps the query true.
        page.evaluate(
            "document.getElementById('c').style.width = '350px'",
            swb_engine::JsonForm::Strict,
        );
        settle(&mut page);
        assert_eq!(page.flush_counters().container_passes, 1);
        assert_eq!(page.flush_counters().container_pass_limits, 0);
    });
}

/// `getComputedStyle` of an element inside a container forces a layout
/// (counted as forced); one outside any container does not.
#[test]
fn computed_style_forces_layout_only_inside_containers() {
    common::on_page_thread(|| {
        let mut page = open(RESIZE_PAGE, Size::new(800.0, 600.0));
        settle(&mut page);
        let script = "(() => { const c = document.getElementById('c'); c.style.width = '200px'; \
                      const a = getComputedStyle(document.getElementById('out')).color; \
                      return a; })()";
        let before = page.flush_counters().forced_layout_runs;
        assert_eq!(answer(&mut page, script), "rgb(0, 0, 0)");
        assert_eq!(
            page.flush_counters().forced_layout_runs,
            before,
            "an element outside containers needs no layout"
        );
        let script = "(() => { document.getElementById('c').style.width = '250px'; \
                      return getComputedStyle(document.getElementById('t')).color; })()";
        assert_eq!(answer(&mut page, script), "rgb(255, 0, 0)");
        assert!(
            page.flush_counters().forced_layout_runs > before,
            "an element inside a container forces a layout"
        );
    });
}

/// A page without container rules and units: no extra pass and no forced
/// layout for `getComputedStyle`, also of an element in a container.
#[test]
fn pages_without_container_css_never_loop() {
    common::on_page_thread(|| {
        let html = "<!DOCTYPE html><style>#c{container-type:inline-size;width:400px}</style>\
                    <div id=c><p id=t>t</p></div>";
        let mut page = open(html, Size::new(800.0, 600.0));
        settle(&mut page);
        let before = page.flush_counters();
        assert_eq!(before.container_passes, 0);
        let script = "(() => { document.getElementById('c').style.width = '200px'; \
                      return getComputedStyle(document.getElementById('t')).color; })()";
        assert_eq!(answer(&mut page, script), "rgb(0, 0, 0)");
        let after = page.flush_counters();
        assert_eq!(after.forced_layout_runs, before.forced_layout_runs);
        assert_eq!(after.container_passes, 0);
    });
}

/// A chain of 12 nested containers, each sized by a query of the one
/// above: the passes stop at the bound, and later updates go on from the
/// sizes of the last layout until the chain settles.
#[test]
fn a_deep_chain_stops_at_the_bound_and_settles_later() {
    common::on_page_thread(|| {
        let depth = 12;
        let mut css = String::from(
            "<!DOCTYPE html><style>body{margin:0} .c{container-type:inline-size;width:500px}\
             @container (width > 400px) { .c { width: 300px } }",
        );
        css.push_str("</style>");
        let mut html = css;
        html.push_str("<div class=c id=c0 style='width:1000px'>");
        for i in 1..=depth {
            html += &["<div class=c id=c", &i.to_string(), ">"].concat();
        }
        html.push_str("<p id=leaf>x</p>");
        let mut page = open(&html, Size::new(1200.0, 600.0));
        settle(&mut page);
        let counters = page.flush_counters();
        assert!(counters.container_passes >= 4);
        assert_eq!(counters.container_pass_limits, 1, "the bound holds");
        // Each child of a container wider than 400px is 300px wide: c1
        // (inside the 1000px c0) is 300px, and c2 (inside 300px c1) is
        // 500px, c3 300px, and so on, once the chain has settled.
        for _ in 0..4 {
            page.evaluate("document.body.offsetWidth", swb_engine::JsonForm::Strict);
            page.evaluate(
                "document.body.appendChild(document.createElement('i'))",
                swb_engine::JsonForm::Strict,
            );
            settle(&mut page);
        }
        let widths = answer(
            &mut page,
            "Array.from(document.querySelectorAll('.c')).map(e => e.getBoundingClientRect().width)",
        );
        let want: Vec<f64> = std::iter::once(1000.0)
            .chain((1..=depth).map(|i| if i % 2 == 1 { 300.0 } else { 500.0 }))
            .collect();
        let got: Vec<f64> = widths
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(got, want);
    });
}
