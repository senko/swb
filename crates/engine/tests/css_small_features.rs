//! The small CSS features of M10 sub-feature 7b (ADR 0038, section "7b"):
//! `@property`, `field-sizing`, `color-mix()` with `currentcolor`,
//! `content-visibility` and `contain: layout`. Every case of the Chromium
//! probe `tools/probes/css-small-features.json` and four cases of
//! `tools/probes/github-css.json`, with the answers that Chromium 148 gave
//! (`tests/data/css-small-features.json` and `css-small-site.json`:
//! `swbtools probe --json`, three equal runs).
//!
//! - `boxes` rows: the border box of an element (or none), within 0.5 px.
//! - `style` rows: `getComputedStyle` values.
//! - `js` rows: the expression's value (computed registered properties,
//!   the CSSOM of `CSSPropertyRule`, serialized colors, client rectangles).
//! - `pixels` rows: colors at points of the element's border box in a
//!   screenshot (within 2 per channel).
//!
//! All cases run with JavaScript on (the queries need it). The cases
//! `cv-auto-*` waited for the frame that computes the relevance to the user
//! in Chromium. [`DEPARTURES`] lists the rows where swb differs on purpose,
//! [`LATER`] those that wait for a later feature.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_threads, repo_root, settle};

/// Rows where swb differs on purpose: (case, rows, why).
const DEPARTURES: &[(&str, &[usize], &str)] = &[
    (
        "ap-color",
        &[0],
        "swb converts `oklch()`, `lab()` and `color(display-p3 ...)` to 8-bit sRGB; Chromium keeps them as written",
    ),
    (
        "ap-universal",
        &[0],
        "swb collapses runs of white space, writes `/**/` between tokens that would merge and drops comments",
    ),
    ("ap-angle", &[0], "`calc()` in `<angle>` is not supported"),
    ("ap-time", &[0], "`calc()` in `<time>` is not supported"),
    (
        "ap-computed-style-enumeration",
        &[0],
        "swb lists the custom properties sorted; Chromium lists the element's own first, then the registered ones",
    ),
    (
        "ap-supports",
        &[0],
        "`CSS.supports('at-rule(@property)')`: swb has no `at-rule()` in `@supports` (as in 7a)",
    ),
    (
        "cv-hidden-boxes",
        &[14],
        "`scrollHeight` of a box with skipped contents: Chromium reports the overflow of the skipped content, swb 0 (the children are out of the fragment tree)",
    ),
    (
        "ap-font-size-cycle-rlh",
        &[0],
        "swb has no `rlh`, `lh`, `cap` and `ic` units: the value is invalid (the initial value); Chromium computes `rlh` against the root's line height",
    ),
    (
        "cm-form-calc-percent-sum",
        &[0],
        "`element.style` text: Chromium simplifies `calc(20% + 30%)` to `calc(50%)`, swb keeps the text",
    ),
    (
        "cm-form-calc-first",
        &[0],
        "`element.style` text: Chromium writes the percentage after the color, swb keeps the order",
    ),
    (
        "cm-form-calc-nan",
        &[0],
        "swb has no `NaN` in `calc()`: the mix is invalid (Chromium: 0%)",
    ),
    (
        "cm-form-none-hsl",
        &[0],
        "swb has no `none` components in `hsl()`: it is 0",
    ),
    (
        "cm-form-none-lab",
        &[0],
        "swb has no `none` components in `lab()`: it is 0, and `lab()` is 8-bit sRGB",
    ),
    (
        "cm-form-powerless-hue",
        &[0],
        "swb writes the hue of a gray as a number, Chromium as `none`",
    ),
    (
        "cm-form-out-of-gamut",
        &[0],
        "swb clamps `color(srgb 2 0 -1)` to 8-bit sRGB when it parses it; Chromium keeps the coordinates",
    ),
];

/// Rows that wait for a later feature: (case, rows, why).
const LATER: &[(&str, &[usize], &str)] = &[
    (
        "cm-prop-caret_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-box_shadow",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-text_shadow",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-column_rule_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-webkit_text_fill_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-webkit_text_stroke_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-text_emphasis_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-shorthand-column-rule",
        &[0],
        "swb has no computed value for this shorthand (backlog)",
    ),
    (
        "cm-shorthand-text-emphasis",
        &[0],
        "swb has no computed value for this shorthand (backlog)",
    ),
    (
        "cm-prop-accent_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-stop_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-flood_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-lighting_color",
        &[0],
        "swb has no computed value for this property (backlog)",
    ),
    (
        "cm-prop-background_image",
        &[0],
        "gradient stops: swb writes a `color-mix()` with `currentcolor` unresolved in `background-image`",
    ),
    (
        "cm-prop-border_image_source",
        &[0],
        "gradient stops: swb writes a `color-mix()` with `currentcolor` unresolved",
    ),
    (
        "cm-inheritance-caret",
        &[0],
        "swb has no computed `caret-color` and `-webkit-text-fill-color` (backlog)",
    ),
    (
        "cm-currentcolor-nested",
        &[0],
        "a `color-mix()` inside a `color-mix()` with `currentcolor` is not supported (the declaration is invalid)",
    ),
    (
        "ap-register-property",
        &[0],
        "`CSS.registerProperty` (backlog: GitHub's scripts do not call it)",
    ),
    (
        "ap-register-property-sheet-conflict",
        &[0],
        "`CSS.registerProperty` (backlog)",
    ),
    (
        "ap-sheet-after-register",
        &[0],
        "`CSS.registerProperty` (backlog)",
    ),
    (
        "content-visibility-and-text-wrap",
        &[2, 3],
        "`text-wrap` is not implemented (backlog)",
    ),
    (
        "cv-hidden-interaction",
        &[8],
        "`Element.checkVisibility` is not implemented (backlog)",
    ),
    (
        "cv-auto-state",
        &[1],
        "`Element.checkVisibility` is not implemented (backlog)",
    ),
    (
        "cv-details-content",
        &[4],
        "`getComputedStyle(details, '::details-content')` gives the initial style (backlog)",
    ),
    (
        "cv-auto-offscreen",
        &[3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        "off-screen `content-visibility: auto` boxes are sized by `contain-intrinsic-size` in Chromium; swb does not skip them (backlog with the measurements, ADR 0038 \"7b\")",
    ),
    (
        "cv-auto-scroll-into-view",
        &[0],
        "the remembered size of `contain-intrinsic-size: auto` (backlog, with the skipping of `auto`)",
    ),
    (
        "fs-select-multiple-sizes",
        &[0, 1, 2, 3, 4, 5],
        "swb has no list boxes (`select` with `multiple` or `size` above 1)",
    ),
];

/// The largest difference of a box coordinate from Chromium's, in px.
const TOLERANCE: f64 = 0.5;

/// The largest difference of a channel that counts as equal.
const PIXEL_TOLERANCE: i32 = 2;

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(true);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    settle(&mut page);
    page
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
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
        (Value::String(x), Value::String(y)) => x == y || same_colors(x, y),
        _ => a == b,
    }
}

/// True if two strings are the same text except for numbers that differ by
/// at most 0.2% (or 0.0012): swb converts through other matrices than
/// Chromium for Lab, Lch and Oklab (the fifth significant digit), and it
/// keeps the color functions that it mixes in 8 bits (`hsl()`: the third
/// digit); both are departures of ADR 0038, section "7b". Only strings
/// with a color function are compared so.
fn same_colors(a: &str, b: &str) -> bool {
    if !["color(", "lab(", "lch(", "oklab(", "oklch("]
        .iter()
        .any(|f| a.contains(f))
    {
        return false;
    }
    let (skeleton_a, numbers_a) = split_numbers(a);
    let (skeleton_b, numbers_b) = split_numbers(b);
    skeleton_a == skeleton_b
        && numbers_a.len() == numbers_b.len()
        && numbers_a.iter().zip(&numbers_b).all(|(x, y)| {
            let tolerance = 1.2e-3 + 2e-3 * x.abs().max(y.abs());
            (x - y).abs() <= tolerance
        })
}

/// The text without its numbers, and the numbers.
fn split_numbers(text: &str) -> (String, Vec<f64>) {
    let mut skeleton = String::new();
    let mut numbers = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let starts_number = c.is_ascii_digit()
            || (matches!(c, '-' | '.')
                && text[i + 1..].starts_with(|n: char| n.is_ascii_digit() || n == '.'));
        if !starts_number {
            skeleton.push(c);
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, n)) = chars.peek() {
            let exponent = matches!(n, '-' | '+') && text[..j].ends_with('e');
            if n.is_ascii_digit() || n == '.' || n == 'e' || exponent {
                end = j + n.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        numbers.push(text[i..end].parse::<f64>().unwrap_or(f64::NAN));
        skeleton.push('#');
    }
    (skeleton, numbers)
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

/// A `style` row: the label is `SELECTOR[N]` or `(SELECTOR LIST)[N]`.
fn check_style(case: &str, row: usize, page: &mut Page, result: &Value) -> Option<String> {
    let label = result["label"].as_str().unwrap();
    let (selector, index) = label.rsplit_once('[').unwrap();
    let index: usize = index.trim_end_matches(']').parse().unwrap();
    let selector = selector
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(selector);
    let want = result["style"].as_object().unwrap();
    let props: Vec<&String> = want.keys().collect();
    let expression = format!(
        "(() => {{ const s = getComputedStyle(document.querySelectorAll({selector:?})[{}]); return {props:?}.map(p => s.getPropertyValue(p)); }})()",
        index - 1
    );
    let got = answer(page, &expression);
    let want_values: Vec<Value> = want.values().cloned().collect();
    (got != Value::Array(want_values.clone())).then(|| {
        format!("{case} #{row} {label}: swb {got}, Chromium {want_values:?} for {props:?}")
    })
}

/// The `pixels` row: swb's colors at the points against Chromium's.
fn check_pixels(
    case: &str,
    row: usize,
    shot: &swb_engine::Pixmap,
    result: &Value,
) -> Option<String> {
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
            .any(|(g, w)| (i32::from(*g) - w).abs() > PIXEL_TOLERANCE)
        {
            wrong.push(format!(
                "({}, {}): swb {got:?}, Chromium {want:?}",
                point[0], point[1]
            ));
        }
    }
    (!wrong.is_empty()).then(|| {
        format!(
            "{case} #{row} {}: {} of {} points differ: {}",
            result["label"],
            wrong.len(),
            points.len(),
            wrong.join("; ")
        )
    })
}

fn viewport_of(results: &Value) -> Size {
    let viewport = results["viewport"].as_array().unwrap();
    Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32)
}

/// The probe file and Chromium's answers of the cases named in `answers`.
fn load(probe: &str, answers: &'static str) -> Vec<(&'static Value, &'static Value)> {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(&std::fs::read_to_string(repo_root().join(probe)).unwrap()).unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(serde_json::from_str(answers).unwrap()));
    let cases = probe["cases"].as_array().unwrap();
    expected["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|answer| {
            let case = cases
                .iter()
                .find(|c| c["name"] == answer["name"])
                .unwrap_or_else(|| panic!("no case {}", answer["name"]));
            (case, answer)
        })
        .collect()
}

fn listed(list: &[(&str, &[usize], &str)], case: &str, row: usize) -> bool {
    list.iter()
        .any(|&(c, rows, _)| c == case && rows.contains(&row))
}

/// Runs a case: its rows against Chromium's answers.
fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let mut page = open(case["html"].as_str().unwrap(), viewport_of(results));
    let rows = results["results"].as_array().unwrap();
    let wants_pixels = rows.iter().any(|r| r["kind"] == "pixels");
    let shot = wants_pixels.then(|| page.screenshot(true).unwrap());
    let mut failures = Vec::new();
    for (row, result) in rows.iter().enumerate() {
        let failure = match result["kind"].as_str().unwrap() {
            "js" => check_js(name, row, &mut page, result),
            "boxes" => check_box(name, row, &mut page, result),
            "style" => check_style(name, row, &mut page, result),
            "pixels" => check_pixels(name, row, shot.as_ref().unwrap(), result),
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
fn small_feature_probe_cases_match_chromium() {
    let mut cases = load(
        "tools/probes/css-small-features.json",
        include_str!("data/css-small-features.json"),
    );
    cases.extend(load(
        "tools/probes/github-css.json",
        include_str!("data/css-small-site.json"),
    ));
    let runs = on_page_threads(
        cases
            .into_iter()
            .map(|(case, answer)| move || run_case(case, answer)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
