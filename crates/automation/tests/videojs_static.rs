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
