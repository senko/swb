//! The top layer in rendering (M10 sub-feature 6b, ADR 0037 section 8):
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
//! The cases with modal dialogs come with sub-feature 6d ([`LATER`]).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_threads, repo_root};

/// The largest difference of a channel that counts as equal (the probe tool
/// uses 2 too).
const TOLERANCE: i32 = 2;

/// The largest difference of a box edge or size, in px.
const BOX_TOLERANCE: f64 = 0.5;

/// The cases that a later sub-feature of ADR 0037 runs: (case, why).
const LATER: &[(&str, &str)] = &[
    (
        "dialog-modal-ua",
        "`showModal()` and modal dialogs: sub-feature 6d",
    ),
    (
        "dialog-modal-big",
        "`showModal()` and modal dialogs: sub-feature 6d",
    ),
    (
        "dialog-modal-scrolled",
        "the script calls `showModal()` before it shows the popover: sub-feature 6d \
         (`popover-scrolled` has the popover part)",
    ),
    (
        "escape-ancestors-modal",
        "`showModal()` and modal dialogs: sub-feature 6d",
    ),
    (
        "backdrop-paint",
        "the backdrop of a modal dialog: sub-feature 6d",
    ),
    (
        "backdrop-author",
        "the backdrop of a modal dialog: sub-feature 6d",
    ),
    (
        "dialog-modal-in-iframe",
        "`showModal()` in a frame: sub-feature 6d",
    ),
    (
        "overlay-and-states",
        "`showModal()` and `show()`: sub-feature 6d",
    ),
    (
        "backdrop-inheritance",
        "the backdrop of a modal dialog: sub-feature 6d (`backdrop-popover` has the popover part)",
    ),
    (
        "top-layer-not-rendered-object",
        "swb renders the fallback children of an `object` with a `type` or `data`, Chromium \
         does not (measured: a child `div` has no client rect; backlog, not the top layer)",
    ),
    (
        "top-layer-not-rendered-content-visibility",
        "swb has no `content-visibility` (backlog; the closed `details` case covers the rule)",
    ),
];

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size, javascript: bool) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(javascript);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// A number of a JSON value.
fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The `js` row: swb's answer to the expression against Chromium's.
fn check_js(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let query = result["label"].as_str().unwrap();
    let got = answer(page, query);
    if got == result["value"] {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row}: {}\n{}",
        &query[..query.len().min(120)],
        differences(&result["value"], &got)
    )]
}

/// The `boxes` row: the border box of the element with the index in
/// document order, or no box.
fn check_box(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let index = result["index"].as_u64().unwrap();
    let expression = format!(
        "(() => {{ const e = document.querySelectorAll('*')[{index}]; const r = e.getBoundingClientRect(); return [e.localName, e.getClientRects().length, r.x + scrollX, r.y + scrollY, r.width, r.height]; }})()"
    );
    let got = answer(page, &expression);
    let tag_ok = got.get(0) == Some(&result["tag"]);
    let ok = match result["rect"].as_array() {
        None => tag_ok && got.get(1).and_then(Value::as_u64) == Some(0),
        Some(want) => {
            tag_ok
                && got.as_array().is_some_and(|g| {
                    g.iter().skip(2).zip(want).all(|(a, b)| {
                        (a.as_f64().unwrap_or(f64::NAN) - number(b)).abs() <= BOX_TOLERANCE
                    })
                })
        }
    };
    if ok {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row} {}: swb {got}, Chromium {} {}",
        result["label"], result["tag"], result["rect"]
    )]
}

/// The `style` row: the computed values of the properties.
fn check_style(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let label = result["label"].as_str().unwrap();
    let (selector, number) = label.rsplit_once('[').unwrap();
    let number: usize = number.trim_end_matches(']').parse().unwrap();
    let mut failures = Vec::new();
    for (property, want) in result["style"].as_object().unwrap() {
        let expression = format!(
            "getComputedStyle(document.querySelectorAll({selector:?})[{}]).getPropertyValue({property:?})",
            number - 1
        );
        let got = answer(page, &expression);
        if &got != want {
            failures.push(format!(
                "{case} #{row} {label} {property}: swb {got}, Chromium {want}"
            ));
        }
    }
    failures
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
            "js" => check_js(name, row, &mut page, result),
            "boxes" => check_box(name, row, &mut page, result),
            "style" => check_style(name, row, &mut page, result),
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
