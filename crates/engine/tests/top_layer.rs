//! The top layer in rendering (M10 sub-features 6b and 6d, ADR 0037 section 8):
//! the cases of the Chromium probe `tools/probes/top-layer.json`, with the
//! answers that Chromium 148 gave (`tests/data/top-layer.json`: `just probe
//! --json tools/probes/top-layer.json`, three equal runs).
//!
//! - `boxes` rows: the border boxes of popovers and dialogs (open, closed,
//!   in transformed, clipped and `display: none` ancestors, with author
//!   `position` and `display`, in shadow trees, scrolled), within 0.5 px;
//!   "no box" rows check that the element has no client rectangles.
//! - `style` rows: computed values (the user-agent rules, the `position`
//!   fix-up, `overlay`).
//! - `js` rows: `getComputedStyle(el, '::backdrop')`, `elementFromPoint`,
//!   client rectangles.
//! - `pixels` rows: the paint order of the top layer and of backdrops, and
//!   that no ancestor clips or fades a top-layer box (within 2 per
//!   channel).
//!
//! The cases with modal dialogs run since sub-feature 6d. [`LATER`] lists
//! the departures that are not about the top layer.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::probe_rows::{box_row, number, style_row};
use common::{OnePage, answer, differences, new_page, on_page_threads, repo_root};

/// The largest difference of a channel that counts as equal (the probe tool
/// uses 2 too).
const TOLERANCE: i32 = 2;

/// The largest difference of a box edge or size, in px.
const BOX_TOLERANCE: f64 = 0.5;

/// The cases that a later sub-feature of ADR 0037 runs: (case, why).
const LATER: &[(&str, &str)] = &[(
    "top-layer-not-rendered-object",
    "swb renders the fallback children of an `object` with a `type` or `data`, Chromium \
         does not (measured: a child `div` has no client rect; backlog, not the top layer)",
)];

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size, javascript: bool) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(javascript);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// The `frame` of the `js` query `query` of the probe case `case` (a selector
/// of an `iframe` of the top-level document), if it has one.
fn frame_of_query<'a>(case: &'a Value, query: &str) -> Option<&'a str> {
    case["measure"]
        .as_array()?
        .iter()
        .find(|m| m["js"] == query)?
        .get("frame")?
        .as_str()
}

/// The `js` row: swb's answer to the expression against Chromium's; a query
/// with a `frame` runs in the document of that `iframe`.
fn check_js(
    (case, probe): (&str, &Value),
    row: usize,
    page: &mut Page,
    result: &Value,
) -> Vec<String> {
    let query = result["label"].as_str().unwrap();
    let got = match frame_of_query(probe, query) {
        Some(selector) => {
            let iframe = page.query_selector_all(selector).unwrap()[0];
            let frame = page.frame_of(swb_engine::FrameId::TOP, iframe).unwrap();
            page.with_frame(frame, |p| answer(p, query)).unwrap()
        }
        None => answer(page, query),
    };
    if got == result["value"] {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row}: {}\n{}",
        &query[..query.len().min(120)],
        differences(&result["value"], &got)
    )]
}

/// The `pixels` row: swb's colors at the points against Chromium's.
fn check_pixels(case: &str, row: usize, shot: &swb_engine::Pixmap, result: &Value) -> Vec<String> {
    let area = result["area"].as_array().unwrap();
    let (left, top) = (
        number(&area[0]).round() as i64,
        number(&area[1]).round() as i64,
    );
    let points = result["points"].as_array().unwrap();
    let pixels = result["pixels"].as_array().unwrap();
    let mut wrong = Vec::new();
    for (point, want) in points.iter().zip(pixels) {
        let (x, y) = (
            point[0].as_i64().unwrap() + left,
            point[1].as_i64().unwrap() + top,
        );
        // Outside the screenshot the page is white (as in the probe).
        let got = match (u32::try_from(x), u32::try_from(y)) {
            (Ok(ux), Ok(uy)) => shot.pixel(ux, uy).map_or([255; 3], |p| {
                let p = p.demultiply();
                [p.red(), p.green(), p.blue()]
            }),
            _ => [255; 3],
        };
        let want: Vec<i32> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        if got
            .iter()
            .zip(&want)
            .any(|(g, w)| (i32::from(*g) - w).abs() > TOLERANCE)
        {
            wrong.push(format!(
                "({}, {}): swb {got:?}, Chromium {want:?}",
                point[0], point[1]
            ));
        }
    }
    if wrong.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row} {}: {} of {} points differ: {}",
        result["label"],
        wrong.len(),
        points.len(),
        wrong.join("; ")
    )]
}

fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let viewport = results["viewport"].as_array().unwrap();
    let size = Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32);
    // The queries need a script runtime. The cases without JavaScript have
    // no scripts and no `noscript`, so their pages render as without it.
    let html = case["html"].as_str().unwrap();
    let javascript = case["javascript"].as_bool().unwrap_or(false);
    assert!(
        javascript || !html.contains("<script") && !html.contains("<noscript"),
        "{name}: a case without JavaScript has no scripts"
    );
    let mut page = open(html, size, true);
    let rows = results["results"].as_array().unwrap();
    let wants_pixels = rows.iter().any(|r| r["kind"] == "pixels");
    let shot = wants_pixels.then(|| page.screenshot(true).unwrap());
    let mut failures = Vec::new();
    for (row, result) in rows.iter().enumerate() {
        failures.extend(match result["kind"].as_str().unwrap() {
            "js" => check_js((name, case), row, &mut page, result),
            "boxes" => box_row(name, row, &mut page, result, BOX_TOLERANCE)
                .into_iter()
                .collect(),
            "style" => style_row(name, row, &mut page, result),
            "pixels" => check_pixels(name, row, shot.as_ref().unwrap(), result),
            other => vec![format!("{name} #{row}: unknown kind {other}")],
        });
    }
    failures
}

#[test]
fn top_layer_matches_chromium() {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/top-layer.json")).unwrap(),
        )
        .unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(
        serde_json::from_str(include_str!("data/top-layer.json")).unwrap(),
    ));
    let cases = probe["cases"].as_array().unwrap();
    let answers = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), answers.len());
    for (name, _) in LATER {
        assert!(
            cases.iter().any(|c| c["name"] == *name),
            "{name} is not a case"
        );
    }
    let runs = on_page_threads(
        cases
            .iter()
            .zip(answers)
            .filter(|(case, _)| !LATER.iter().any(|(name, _)| case["name"] == *name))
            .map(|(case, answer)| {
                assert_eq!(case["name"], answer["name"]);
                move || run_case(case, answer)
            }),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
