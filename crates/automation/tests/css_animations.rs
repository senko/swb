//! CSS animations (M13, ADR 0044): the cases of the Chromium probes
//! `tools/probes/css-animations.json` and `tools/probes/css-animations-3a.json`
//! run here, and the values of their `js` queries must be the ones that
//! Chromium 148 gave (`tests/data/css-animations.json` and
//! `tests/data/css-animations-3a.json`: `uv run --project tools swbtools probe
//! --json FILE`, three equal runs, then `jq '.cases | map({(.name):
//! [.results[] | (.value // null)]}) | add'`).
//!
//! Feature 3a has the model only: the `animation-*` properties, their
//! `element.style` and `getComputedStyle` texts and `CSS.supports`. The rows
//! that need a running animation (`document.getAnimations()`, seeking, the
//! animated values, events) are skipped; [`NEEDS_RUNNING`] lists them for
//! feature 3b, which checks them.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{
    DataUrls, OnePage, SKIP_QUERY as SKIP, on_page_thread, open_script_page, run_probe_file_with,
};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

/// The queries of `css-animations.json` that run in feature 3a: (case,
/// query index). Every other query needs a running animation (3b): the
/// animated values, `document.getAnimations()`, the `Animation` and
/// `KeyframeEffect` members and the events.
const RUNS_WITHOUT_ANIMATIONS: &[(&str, usize)] = &[
    // `getComputedStyle(...).animationTimingFunction` of 20 easings.
    ("easings", 0),
    // The `animation` shorthand and its longhands, also through `var()`.
    ("animation-shorthand", 0),
    ("animation-shorthand", 1),
    // An element with `animation: none` from a custom property, and the
    // timing longhands of a spinner segment.
    ("spinner-steps", 4),
    ("spinner-steps", 5),
];

/// The cases of `css-animations-3a.json` that need a running animation
/// (they read `getKeyframes()` or an animated value): feature 3b. Unit
/// tests cover them in 3a (`animation/keyframes/tests.rs`,
/// `animation/interpolate/transform.rs`, `animation/interpolate/custom/tests.rs`
/// for the `registered-*` cases).
const NEEDS_RUNNING: &[&str] = &[
    "keyframes-resolution",
    "keyframes-conditions",
    "matrix-percent",
    "registered-custom",
    "registered-tl-1",
    "registered-tl-2",
    "registered-tl-3",
    "registered-tl-4",
    "registered-currentcolor",
];

/// The cases of `css-animations-3a.json` that read members swb does not
/// have yet: `CSSKeyframesRule.cssRules`, `appendRule()` and
/// `CSSKeyframeRule` (backlog; no target page uses them).
const NOT_IMPLEMENTED: &[&str] = &["keyframes-rule-members"];

/// The values where swb departs from Chromium on purpose in
/// `css-animations-3a.json`: (case, query index, part of the value as
/// Chromium gives it, the part as swb gives it; every occurrence).
const SWB_LINES: &[(&str, usize, &str, &str)] = &[
    // An infinite math function in `cubic-bezier()` or `linear()` is the
    // largest `f64`, written as the number `1.79769e+308`. The declaration
    // that `element.style.setProperty()` sets reaches swb's cascade as
    // that text, a literal, which saturates to the `f32` range. Values
    // from style sheets keep Chromium's value (unit tests in
    // `animation/easing/tests.rs`).
    ("number-ranges", 7, "1.79769e+308", "3.40282e+38"),
    // swb has no `animation-trigger` (Chromium 148 parses it; no page of
    // the targets uses it).
    (
        "aliases",
        0,
        "animationTrigger=true",
        "animationTrigger=false",
    ),
    (
        "aliases",
        1,
        "animation-trigger:none=true",
        "animation-trigger:none=false",
    ),
    (
        "aliases",
        2,
        "animation-trigger=[none]",
        "animation-trigger=[]",
    ),
    // `transition-timing-function` takes `linear()` in feature 5.
    (
        "aliases",
        1,
        "transition-timing-function:linear(0, 1)=true",
        "transition-timing-function:linear(0, 1)=false",
    ),
    // When the layered lists differ in length, Chromium's `cssText` writes
    // an `animation` that does not parse back to the same longhands (its
    // block keeps the longhands). swb's cascade parses `cssText` again, so
    // swb writes the longhands there; `getPropertyValue('animation')` is
    // Chromium's.
    (
        "shorthand-blocks",
        0,
        "css{animation: 3s ease 0s 1 normal none running a, ease 0s 1 normal none running b;}",
        "css{animation-timing-function: ease, ease; animation-delay: 0s, 0s; \
         animation-iteration-count: 1, 1; animation-direction: normal, normal; \
         animation-fill-mode: none, none; animation-play-state: running, running; \
         animation-name: a, b; animation-timeline: auto; animation-range: normal; \
         animation-duration: 3s;}",
    ),
    (
        "shorthand-blocks",
        1,
        "css{animation: 1s ease 1s 1 normal none running a, 2s;}",
        "css{animation-duration: 1s; animation-timing-function: ease; \
         animation-iteration-count: 1; animation-direction: normal; \
         animation-fill-mode: none; animation-play-state: running; \
         animation-name: a; animation-timeline: auto; animation-range: normal; \
         animation-delay: 1s, 2s;}",
    ),
];

/// The expected value of a query: Chromium's, with the departures, or
/// [`SKIP`].
fn expected(file: &str, name: &str, index: usize, chromium: &Value) -> Value {
    let skipped = match file {
        "css-animations" => !RUNS_WITHOUT_ANIMATIONS.contains(&(name, index)),
        _ => NEEDS_RUNNING.contains(&name) || NOT_IMPLEMENTED.contains(&name),
    };
    if skipped {
        return Value::String(SKIP.to_owned());
    }
    let Some(text) = chromium.as_str() else {
        return chromium.clone();
    };
    let mut text = text.to_owned();
    for &(case, query, chromium_part, swb_part) in SWB_LINES {
        if file == "css-animations-3a" && case == name && query == index {
            text = text.replace(chromium_part, swb_part);
        }
    }
    Value::String(text)
}

fn same(want: &Value, got: &Value) -> bool {
    want.as_str() == Some(SKIP) || want == got
}

fn open(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
    let default = serde_json::json!([400, 300]);
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

/// The rows of `css-animations.json` that need no running animation.
#[test]
fn animation_probe_rows_without_running_animations() {
    on_page_thread(|| {
        let failures = run_probe_file_with(
            "css-animations",
            open,
            |name, index, value| expected("css-animations", name, index, value),
            same,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The properties: grammar, `element.style`, `getComputedStyle`, the
/// shorthand and its serialization, the aliases (`css-animations-3a.json`).
#[test]
fn animation_properties() {
    on_page_thread(|| {
        let failures = run_probe_file_with(
            "css-animations-3a",
            open,
            |name, index, value| expected("css-animations-3a", name, index, value),
            same,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
