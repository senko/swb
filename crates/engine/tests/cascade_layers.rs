//! Cascade layers (M10 feature 1, ADR 0007 "Cascade layers"): every case
//! of the Chromium probe `tools/probes/cascade-layers.json`, with the
//! answers that Chromium 148 gave (`tests/data/web-cascade-layers.json`:
//! `just probe --json tools/probes/cascade-layers.json`, three equal runs).
//!
//! - `style` rows: the computed values of `#p` and others after the layer
//!   order of statements, blocks, nested, dotted and anonymous layers,
//!   `@import ... layer`, layers across `style` elements, declarations in
//!   `@media` and `@supports` that do not apply, invalid preludes, the
//!   reversed order of `!important`, the `style` attribute, presentational
//!   hints, custom properties, and `revert-layer`.
//! - `boxes` rows: the boxes of images with `width` and `height`
//!   attributes against layered rules, and of `@font-face` in layers.
//! - `js` rows: the CSSOM objects of the layer rules (`cssRules`, `cssText`,
//!   `name`, `nameList`, `type`, `insertRule` and `deleteRule`, the errors,
//!   the effect on the computed style) and of `@import ... layer`.
//!
//! The rows where swb departs from Chromium on purpose are in
//! [`SWB_DEPARTURES`], each with its reason (the row is then not
//! compared).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_threads, repo_root};

/// The rows where swb departs from Chromium on purpose: (case, row, why).
const SWB_DEPARTURES: &[(&str, usize, &str)] = &[];

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size, javascript: bool) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(javascript);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The `js` row: swb's answer to the expression against Chromium's.
fn check_js(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let query = result["label"].as_str().unwrap();
    let want = &result["value"];
    let got = answer(page, query);
    if &got == want {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row}: {}\n{}",
        &query[..query.len().min(160)],
        differences(want, &got)
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

/// The `style` row: the computed values of the properties of the `n`th
/// match of the selector.
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

fn viewport_of(results: &Value) -> Size {
    let viewport = results["viewport"].as_array().unwrap();
    Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32)
}

/// The probe file and Chromium's answers.
fn load_cases() -> (&'static [Value], &'static [Value]) {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/cascade-layers.json")).unwrap(),
        )
        .unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(
        serde_json::from_str(include_str!("data/web-cascade-layers.json")).unwrap(),
    ));
    let cases = probe["cases"].as_array().unwrap();
    let answers = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), answers.len());
    for (case, answer) in cases.iter().zip(answers) {
        assert_eq!(case["name"], answer["name"]);
    }
    (cases, answers)
}

/// Runs a case with JavaScript on, so that the rows can ask the page.
fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let mut page = open(case["html"].as_str().unwrap(), viewport_of(results), true);
    let mut failures = Vec::new();
    for (row, result) in results["results"].as_array().unwrap().iter().enumerate() {
        if SWB_DEPARTURES.iter().any(|d| d.0 == name && d.1 == row) {
            continue;
        }
        failures.extend(match result["kind"].as_str().unwrap() {
            "js" => check_js(name, row, &mut page, result),
            "boxes" => check_box(name, row, &mut page, result),
            "style" => check_style(name, row, &mut page, result),
            other => vec![format!("{name} #{row}: unknown kind {other}")],
        });
    }
    failures
}

/// Every case with JavaScript on.
#[test]
fn cascade_layers_match_chromium() {
    let (cases, answers) = load_cases();
    let runs = on_page_threads(
        cases
            .iter()
            .zip(answers)
            .map(|(case, answer)| move || run_case(case, answer)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The class of the darkest pixel of the text in `rect`: `r`ed, `g`reen,
/// `b`lue or blac`k` (the text is anti-aliased, so the pixel is a blend
/// with white: the channel that stays at 255 or is the highest names the
/// color).
fn ink_color(shot: &swb_engine::Pixmap, rect: swb_engine::Rect) -> char {
    let mut darkest = (255u8, 255u8, 255u8);
    let sum = |c: (u8, u8, u8)| u32::from(c.0) + u32::from(c.1) + u32::from(c.2);
    let (x0, y0) = (
        rect.x.floor().max(0.0) as u32,
        rect.y.floor().max(0.0) as u32,
    );
    let (x1, y1) = (
        (rect.x + rect.width.min(400.0)).ceil() as u32,
        (rect.y + rect.height).ceil() as u32,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            if x < shot.width() && y < shot.height() {
                let c = common::rgb(shot, x, y);
                if sum(c) < sum(darkest) {
                    darkest = c;
                }
            }
        }
    }
    let (r, g, b) = darkest;
    if r.abs_diff(g) < 8 && g.abs_diff(b) < 8 {
        'k'
    } else if r > g && r > b {
        'r'
    } else if b > r && b > g {
        'b'
    } else {
        'g'
    }
}

/// The class letter of a computed `color` value.
fn color_class(css: &str) -> Option<char> {
    Some(match css {
        "rgb(255, 0, 0)" => 'r',
        "rgb(0, 128, 0)" => 'g',
        "rgb(0, 0, 255)" => 'b',
        "rgb(0, 0, 0)" => 'k',
        _ => return None,
    })
}

/// Runs the cases without JavaScript: no script can ask the page, so the
/// rows are checked on the layout and the paint. A `color` row of `#p`
/// compares the color of its text in the screenshot, a `width` row the
/// width of its box, and a `boxes` row the boxes (by document order). The
/// other rows need a script and are not run here.
fn run_case_without_javascript(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let mut page = open(case["html"].as_str().unwrap(), viewport_of(results), false);
    page.update_layout();
    let boxes = swb_engine::element_boxes(&page);
    let shot = page.screenshot(true).unwrap();
    let mut failures = Vec::new();
    for (row, result) in results["results"].as_array().unwrap().iter().enumerate() {
        match result["kind"].as_str().unwrap() {
            "boxes" => {
                let index = result["index"].as_u64().unwrap() as usize;
                let want: Vec<f64> = result["rect"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(number)
                    .collect();
                let got = boxes.get(index).and_then(|b| b.rect);
                let ok = got.is_some_and(|r| {
                    [r.x, r.y, r.width, r.height]
                        .iter()
                        .zip(&want)
                        .all(|(a, b)| (f64::from(*a) - b).abs() <= 1.0)
                });
                if !ok {
                    failures.push(format!(
                        "{name} #{row} {}: swb {got:?}, Chromium {want:?}",
                        result["label"]
                    ));
                }
            }
            "style" if result["label"] == "#p[1]" => {
                let style = result["style"].as_object().unwrap();
                let rect = common::rect(&mut page, "p");
                if let Some(want) = style.get("color").and_then(Value::as_str) {
                    let want = color_class(want).unwrap();
                    let got = ink_color(&shot, rect);
                    if got != want {
                        failures.push(format!("{name} #{row} color: swb {got}, Chromium {want}"));
                    }
                }
                if let Some(want) = style.get("width").and_then(Value::as_str) {
                    let want: f32 = want.trim_end_matches("px").parse().unwrap();
                    if (rect.width - want).abs() > 1.0 {
                        failures.push(format!(
                            "{name} #{row} width: swb {}, Chromium {want}",
                            rect.width
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    failures
}

/// The cases with JavaScript off (the CSSOM rows of the cases that need
/// scripts are skipped: those cases have no `style` or `boxes` rows).
#[test]
fn cascade_layers_without_javascript() {
    let (cases, answers) = load_cases();
    let runs = on_page_threads(
        cases
            .iter()
            .zip(answers)
            .filter(|(case, _)| case["javascript"] != true)
            .map(|(case, answer)| move || run_case_without_javascript(case, answer)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
