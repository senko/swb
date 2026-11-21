//! The static CSS of M13 feature 2a (videojs.org): the cases of the
//! Chromium probe `tools/probes/css-videojs-2a.json` run here, and the
//! values of their `js` queries are the ones that Chromium 148 gave
//! (`tests/data/css-videojs-2a.json`: `uv run --project tools swbtools probe
//! --json tools/probes/css-videojs-2a.json`, three equal runs, then `jq
//! '.cases | map({(.name): [.results[] | (.value // null)]}) | add'`). The
//! cases cover: the width of an inline SVG with `height: 100%` in a flex item
//! (the logo), `translate`, `rotate` and `scale` (computed values, the order
//! with `transform`, the boxes), `box-shadow` and `text-shadow` (grammar and
//! computed values), `text-wrap` with `text-wrap-mode` and `text-wrap-style`
//! (values, the line breaks of `balance` and `pretty` in 213 paragraph and
//! width pairs), `-webkit-text-stroke` and `-webkit-text-fill-color`,
//! `isolation` and `corner-shape` (values). The pixels are in
//! `crates/engine/tests/static_css_pixels.rs`. [`SWB_VALUES`] lists the
//! values where swb departs from Chromium on purpose.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{DataUrls, OnePage, on_page_thread, open_script_page, run_probe_file};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, the list entries `[property, value, computed]` that differ as
/// swb gives them). Each entry replaces the entry with the same property
/// and value.
const SWB_ENTRIES: &[(&str, &str, &str, &str)] = &[
    // swb's `calc()` has no angles (as for `filter`): `hue-rotate()`.
    ("rotate-values", "rotate", "calc(10deg + 5deg)", "invalid"),
    // Feature 2b: `display: -webkit-box` is a flex container in swb
    // (computed `flex`), and `subgrid` is not parsed (plan in ADR 0046).
    ("props-7", "display", "-webkit-box", "flex"),
    ("props-7", "display", "-webkit-inline-box", "inline-flex"),
    ("props-7", "grid-template-rows", "subgrid", "invalid"),
    (
        "props-7",
        "grid-template-rows",
        "subgrid [a] [b]",
        "invalid",
    ),
    (
        "props-7",
        "grid-template-rows",
        "subgrid repeat(2, [x])",
        "invalid",
    ),
    (
        "props-7",
        "grid-template-rows",
        "subgrid [a] repeat(auto-fill, [b])",
        "invalid",
    ),
    (
        "props-7",
        "grid-template-rows",
        "subgrid [a b] [c]",
        "invalid",
    ),
    ("props-7", "grid-template-columns", "subgrid", "invalid"),
    (
        "props-7",
        "grid-template-columns",
        "subgrid [a] [b]",
        "invalid",
    ),
    (
        "props-7",
        "grid-template-columns",
        "subgrid [a b] [c]",
        "invalid",
    ),
    (
        "props-7",
        "grid-template-columns",
        "subgrid repeat(2, [x])",
        "invalid",
    ),
    ("props-7", "grid-template", "subgrid / subgrid", "invalid"),
    ("props-7", "grid-template", "subgrid [a] / 100px", "invalid"),
];

/// The expected value of a query: Chromium's, with the departures.
fn expected_value(name: &str, _index: usize, chromium: &Value) -> Value {
    let mut want = chromium.clone();
    let Some(list) = want.as_array_mut() else {
        return want;
    };
    for &(case, property, value, computed) in SWB_ENTRIES {
        if case != name {
            continue;
        }
        for entry in list.iter_mut() {
            if entry[0] == property && entry[1] == value {
                *entry = serde_json::json!([property, value, computed]);
            }
        }
    }
    want
}

fn open(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
    // The file's viewport if the case has none.
    let default = serde_json::json!([600, 200]);
    let viewport = case.get("viewport").unwrap_or(&default).as_array().unwrap();
    open_script_page(
        Arc::new(fetcher),
        Url::parse(url).unwrap(),
        Size::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
        ),
    )
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let failures = run_probe_file("css-videojs-2a", open, expected_value);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The text of a color with its numbers cut out: the rest, and the numbers.
fn color_parts(text: &str) -> (String, Vec<f64>) {
    let mut rest = String::new();
    let mut numbers = Vec::new();
    let mut token = String::new();
    let flush = |token: &mut String, rest: &mut String, numbers: &mut Vec<f64>| {
        if let Ok(v) = token.parse::<f64>() {
            numbers.push(v);
            rest.push('#');
        } else {
            rest.push_str(token);
        }
        token.clear();
    };
    for c in text.chars() {
        let number_char = c.is_ascii_digit()
            || (matches!(c, '.' | 'e' | '-' | '+') && !token.is_empty())
            || (c == '-' && token.is_empty());
        if number_char {
            token.push(c);
        } else {
            flush(&mut token, &mut rest, &mut numbers);
            rest.push(c);
        }
    }
    flush(&mut token, &mut rest, &mut numbers);
    (rest, numbers)
}

/// True if two serialized colors are equal, or equal but for numbers that
/// differ by at most 1e-4 or 5e-4 of the number: the conversions between
/// color spaces of swb and Chromium differ in the fifth digit (only for the
/// rows that [`converts_between_spaces`] names).
fn colors_close(want: &str, got: &str) -> bool {
    if want == got {
        return true;
    }
    let ((a, na), (b, nb)) = (color_parts(want), color_parts(got));
    a == b
        && na.len() == nb.len()
        && na
            .iter()
            .zip(&nb)
            .all(|(x, y)| (x - y).abs() <= 1e-4_f64.max(5e-4 * x.abs()))
}

/// True if the row `[property, value, computed]` converts between color
/// spaces: the declared `value` is a `color-mix()`, whose result is in
/// another space than its inputs. These rows are the only ones that are
/// compared with [`colors_close`]; all other rows (declared colors, which
/// Chromium serializes as given, and values that are no colors) are
/// compared exactly. In the probe file these rows are the `color-mix(...)`
/// rows of the cases `c-*`, for every property that uses that list.
fn converts_between_spaces(value: &str) -> bool {
    value.contains("color-mix(")
}

/// The comparison of the 2b probe: lists of `[property, value, computed]`
/// compare exactly, except the computed color of a row that converts
/// between spaces ([`converts_between_spaces`]), which uses
/// [`colors_close`].
fn same_2b(want: &Value, got: &Value) -> bool {
    let entry = |w: &Value, g: &Value| match (w.as_array(), g.as_array()) {
        (Some(w), Some(g)) if w.len() == 3 && g.len() == 3 => {
            w[..2] == g[..2]
                && match (w[1].as_str(), w[2].as_str(), g[2].as_str()) {
                    (Some(value), Some(a), Some(b)) if converts_between_spaces(value) => {
                        colors_close(a, b)
                    }
                    _ => w[2] == g[2],
                }
        }
        _ => w == g,
    };
    match (want.as_array(), got.as_array()) {
        (Some(w), Some(g)) if w.len() == g.len() => w.iter().zip(g).all(|(w, g)| entry(w, g)),
        _ => want == got,
    }
}

/// Runs the cases of the probe file of feature 2b (colors that keep their
/// space, `font-variant-*`, `background-clip: text`, `-webkit-line-clamp`,
/// `subgrid`, `line-break` and the properties without a visible effect)
/// against Chromium's values.
#[test]
fn probe_cases_2b() {
    on_page_thread(|| {
        let failures = common::run_probe_file_with("css-videojs-2b", open, expected_value, same_2b);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// True if two texts are equal but for numbers that differ by at most
/// 0.02: swb places boxes in `f32` and Chromium in 1/64 px.
fn same_numbers(want: &Value, got: &Value) -> bool {
    let (Some(want), Some(got)) = (want.as_str(), got.as_str()) else {
        return want == got;
    };
    let ((a, na), (b, nb)) = (color_parts(want), color_parts(got));
    a == b && na.len() == nb.len() && na.iter().zip(&nb).all(|(x, y)| (x - y).abs() <= 0.02)
}

/// Runs the cases of the probe file of feature 2c (ADR 0046): the `js`
/// rows are the geometry of the paths of an inline `<svg>` with a viewport
/// of width or height 0 (the viewBox transform is the identity there: the
/// zero-size icons of the videojs.org player), and with CSS sizes; the
/// pixel rows are in `crates/engine/tests/static_css_pixels.rs`.
#[test]
fn probe_cases_2c() {
    on_page_thread(|| {
        let failures =
            common::run_probe_file_with("css-videojs-2c", open, expected_value, same_numbers);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
