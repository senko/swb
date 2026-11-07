//! `backdrop-filter` (M9 feature 5, ADR 0033): the cases of the Chromium
//! probe `tools/probes/backdrop-filter.json`, with the answers that
//! Chromium 148 gave (`tests/data/web-backdrop-filter.json`: `just probe
//! --json tools/probes/backdrop-filter.json`, three equal runs).
//!
//! - `js` rows: the syntax (what `CSS.supports` accepts), `element.style`
//!   and `getComputedStyle` of about 400 values, the cascade, and that
//!   `-webkit-backdrop-filter` is not a property in Chromium 148.
//! - `boxes` rows: the boxes and offset parents of the children of boxes
//!   with a `backdrop-filter` (the containing block of absolutely
//!   positioned and fixed boxes, the root element, inline boxes).
//! - `pixels` rows: the colors in a screenshot at points: the edge of a
//!   blur (the backdrop is the border box, mirrored at its edges), the
//!   functions on the backdrop, the backdrop root (what ends the
//!   backdrop), the box's own opacity, filter, clip path and mask, rounded
//!   corners and the other areas, the paint order, nesting, transforms,
//!   scrolled content and fixed boxes.
//!
//! The values of the rows where swb departs from Chromium on purpose are in
//! [`SWB_LINES`] and [`SWB_PIXELS`], each with its reason (the pixels are
//! not compared there).

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

/// Lines of the answers of `js` rows that test many values (one value per
/// line) where swb departs from Chromium on purpose: (case, row, line,
/// swb's line, why).
const SWB_LINES: &[(&str, usize, usize, &str, &str)] = &[
    (
        "bf-syntax-hue-rotate",
        0,
        9,
        r#""hue-rotate(calc(1deg + 2deg))" => false [] {}"#,
        "swb's `calc()` has no angles",
    ),
    (
        "bf-syntax-drop-shadow",
        0,
        31,
        r#""drop-shadow(1px 2px 3px color-mix(in srgb, red, blue))" => true [drop-shadow(color-mix(in srgb, red, blue) 1px 2px 3px)] {drop-shadow(rgb(128, 0, 128) 1px 2px 3px)}"#,
        "Chromium writes a computed color of `color-mix()` as `color(srgb 0.5 0 0.5)`; swb has `rgb()`",
    ),
];

/// The `pixels` rows where swb departs from Chromium on purpose: (case,
/// row, why). The colors are not compared there.
const SWB_PIXELS: &[(&str, usize, &str)] = &[
    (
        "bf-edge-page-right",
        0,
        "the box reaches beyond the page: Chromium reads the canvas beyond the page edge, swb mirrors the image at the edge of the page (ADR 0033)",
    ),
    (
        "bf-transform-scale1.5",
        0,
        "a scaled box: swb draws the transformed group in a layer without the page behind it, so its backdrop is empty (ADR 0033)",
    ),
    (
        "bf-cb-willchange-pixels-transform",
        0,
        "Chromium 148 makes a containing block of the fixed child for `will-change: transform` (and `perspective`, and for absolute boxes `position`); swb's `will-change` is inert for them (ADR 0033)",
    ),
    (
        "bf-fn-drop-shadow-edge-blur",
        0,
        "Chromium filters one image extended by the mirror, so a later function sees the shadow's tail beyond the box edge; swb mirrors each blur by itself and crops after each function (ADR 0033)",
    ),
    (
        "bf-fn-drop-shadow-mirror-read",
        0,
        "the shadow of a moved backdrop reads the mirrored image beyond the box edge in Chromium; swb reads transparent pixels there (ADR 0033)",
    ),
    ("bf-transform-rotate45", 0, "as bf-transform-scale1.5"),
    ("bf-transform-scale2", 0, "as bf-transform-scale1.5"),
    (
        "bf-transform-scale.5",
        0,
        "Chromium 148 shows the black half of the backdrop mirrored over the whole scaled-down box (the profile of a lower scale is also wrong); swb does not reproduce it (ADR 0033)",
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
    let mut want = result["value"].clone();
    if let Some(text) = want.as_str() {
        let mut lines: Vec<&str> = text.lines().collect();
        for d in SWB_LINES.iter().filter(|d| d.0 == case && d.1 == row) {
            lines[d.2] = d.3;
        }
        want = Value::String(lines.join("\n"));
    }
    let got = answer(page, query);
    if got == want {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row}: {}\n{}",
        &query[..query.len().min(120)],
        differences(&want, &got)
    )]
}

/// The `boxes` row: the border box of the element with the index in
/// document order.
fn check_box(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let index = result["index"].as_u64().unwrap();
    let expression = format!(
        "(() => {{ const e = document.querySelectorAll('*')[{index}]; const r = e.getBoundingClientRect(); return [e.localName, r.x + scrollX, r.y + scrollY, r.width, r.height]; }})()"
    );
    let got = answer(page, &expression);
    let want: Vec<f64> = result["rect"]
        .as_array()
        .unwrap()
        .iter()
        .map(number)
        .collect();
    let ok = got.as_array().is_some_and(|g| {
        g.first() == Some(&result["tag"])
            && g.iter()
                .skip(1)
                .zip(&want)
                .all(|(a, b)| (a.as_f64().unwrap_or(f64::NAN) - b).abs() <= 1.0)
    });
    if ok {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row} {}: swb {got}, Chromium {} {want:?}",
        result["label"], result["tag"]
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
    if SWB_PIXELS.iter().any(|d| d.0 == case && d.1 == row) {
        return Vec::new();
    }
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
        "{case} #{row} {}: {} of {} points differ, e.g. {}",
        result["label"],
        wrong.len(),
        points.len(),
        wrong.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
    )]
}

fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    // The answers hold the viewport of the case, also when it is the
    // default of the file.
    let viewport = results["viewport"].as_array().unwrap();
    let size = Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32);
    let javascript = case["javascript"].as_bool().unwrap_or(false);
    let mut page = open(case["html"].as_str().unwrap(), size, javascript);
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
fn backdrop_filter_matches_chromium() {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/backdrop-filter.json"))
                .unwrap(),
        )
        .unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(
        serde_json::from_str(include_str!("data/web-backdrop-filter.json")).unwrap(),
    ));
    let cases = probe["cases"].as_array().unwrap();
    let answers = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), answers.len());
    let runs = on_page_threads(cases.iter().zip(answers).map(|(case, answer)| {
        assert_eq!(case["name"], answer["name"]);
        move || run_case(case, answer)
    }));
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
