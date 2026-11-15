//! The rows of the Chromium probe cases that several engine tests check
//! (`cascade_layers.rs`, `container_queries.rs`, `css_small_features.rs`,
//! `shadow_rendering.rs`, `top_layer.rs`, `backdrop_filter.rs`,
//! `inline_alignment.rs`): `boxes` rows (border boxes, within a tolerance),
//! `style` rows (computed values), and the loading of a probe file with
//! Chromium's answers. Each test keeps its own `js` and `pixels` rows and its
//! lists of departures.

// Test helpers unwrap too.
#![allow(clippy::unwrap_used)]

use serde_json::Value;
use swb_engine::{Page, Size};

use super::{answer, repo_root};

/// A number of a JSON value.
pub(crate) fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The viewport of the answers of a case.
pub(crate) fn viewport_of(results: &Value) -> Size {
    let viewport = results["viewport"].as_array().unwrap();
    Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32)
}

/// The failure of a `boxes` row, if swb's box differs from Chromium's by
/// more than `tolerance`: the border box of the element with the index in
/// document order, or no box (`rect` is null).
pub(crate) fn box_row(
    case: &str,
    row: usize,
    page: &mut Page,
    result: &Value,
    tolerance: f64,
) -> Option<String> {
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
                            (a.as_f64().unwrap_or(f64::NAN) - number(b)).abs() <= tolerance
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

/// The failure of a `boxes` row of a probe whose boxes all exist: the border
/// box of the element with the index in document order, each number within
/// `tolerance`.
pub(crate) fn existing_box_row(
    case: &str,
    row: usize,
    page: &mut Page,
    result: &Value,
    tolerance: f64,
) -> Vec<String> {
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
                .all(|(a, b)| (a.as_f64().unwrap_or(f64::NAN) - b).abs() <= tolerance)
    });
    if ok {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row} {}: swb {got}, Chromium {} {want:?}",
        result["label"], result["tag"]
    )]
}

/// The failures of a `style` row: the computed value of each property of
/// the `n`th match of the selector in the label `SELECTOR[N]`.
pub(crate) fn style_row(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
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

/// The failure of a `style` row with one query for all properties: the label
/// is `SELECTOR[N]` or `(SELECTOR LIST)[N]`.
pub(crate) fn style_row_at_once(
    case: &str,
    row: usize,
    page: &mut Page,
    result: &Value,
) -> Option<String> {
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

/// The cases of the probe file `probe` (relative to the repository) that
/// the Chromium answers `answers` (the text of a `probe --json` output)
/// name, each with its answer.
pub(crate) fn load_cases(
    probe: &str,
    answers: &'static str,
) -> Vec<(&'static Value, &'static Value)> {
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
