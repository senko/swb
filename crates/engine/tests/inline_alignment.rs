//! Line box alignment and soft wrap opportunities around atomic inlines
//! (M10 sub-feature 8a): every case of the Chromium probe
//! `tools/probes/inline-alignment.json`, with the boxes that Chromium 148
//! gave (`tests/data/inline-alignment.json`: `swbtools probe --json`, three
//! equal runs).
//!
//! - `va-*`: `vertical-align: top` and `bottom` (CSS 2.2 §10.8.1) on atomic
//!   inlines and inline boxes, nested, inside shifted boxes, next to other
//!   alignments, across lines, in quirks mode, and the line box height that
//!   the tallest such box gives.
//! - `wrap-*`: the `white-space` that decides at the edges of an atomic
//!   inline (its parent's, not its own), and the opportunity at the start
//!   of an inline box with `unicode-bidi` after an atomic inline.
//!
//! Each row is the border box of an element (`getBoundingClientRect`), within
//! 0.5 px; the pages have no scripts.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, new_page, on_page_threads, repo_root};

/// The largest difference of a box coordinate from Chromium's, in px.
const TOLERANCE: f64 = 0.5;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(true);
    page.navigate(Url::parse(common::CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The failure of a `boxes` row, if swb's box differs from Chromium's.
fn check_box(case: &str, row: usize, page: &mut Page, result: &Value) -> Option<String> {
    let index = result["index"].as_u64().unwrap();
    let expression = format!(
        "(() => {{ const e = document.querySelectorAll('*')[{index}]; const r = e.getBoundingClientRect(); return [e.localName, e.getClientRects().length, r.x + scrollX, r.y + scrollY, r.width, r.height]; }})()"
    );
    let got = answer(page, &expression);
    let tag_ok = got.get(0) == Some(&result["tag"]);
    let has_box = got.get(1).and_then(Value::as_u64).unwrap_or(0) > 0;
    let ok = tag_ok
        && match result["rect"].as_array() {
            None => !has_box,
            Some(want) => {
                has_box
                    && got
                        .as_array()
                        .unwrap()
                        .iter()
                        .skip(2)
                        .zip(want)
                        .all(|(a, b)| {
                            (a.as_f64().unwrap_or(f64::NAN) - number(b)).abs() <= TOLERANCE
                        })
            }
        };
    (!ok).then(|| {
        format!(
            "{case} #{row} {}: swb {got}, Chromium {} {}",
            result["label"], result["tag"], result["rect"]
        )
    })
}

/// Runs the rows of a case.
fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let viewport = results["viewport"].as_array().unwrap();
    let size = Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32);
    let mut page = open(case["html"].as_str().unwrap(), size);
    results["results"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(row, result)| match result["kind"].as_str().unwrap() {
            "boxes" => check_box(name, row, &mut page, result),
            other => Some(format!("{name} #{row}: unknown kind {other}")),
        })
        .collect()
}

#[test]
fn inline_alignment_probe_cases_match_chromium() {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/inline-alignment.json"))
                .unwrap(),
        )
        .unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(
        serde_json::from_str(include_str!("data/inline-alignment.json")).unwrap(),
    ));
    let cases = probe["cases"].as_array().unwrap();
    let answers = expected["cases"].as_array().unwrap();
    assert_eq!(
        cases.len(),
        answers.len(),
        "every case has Chromium's answer"
    );
    let runs = on_page_threads(answers.iter().map(|answer| {
        let case = cases
            .iter()
            .find(|c| c["name"] == answer["name"])
            .unwrap_or_else(|| panic!("no case {}", answer["name"]));
        move || run_case(case, answer)
    }));
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
