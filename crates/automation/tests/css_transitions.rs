//! CSS transitions (M13 feature 5, ADR 0044): the cases of the Chromium
//! probe `tools/probes/css-transitions.json` run here, and the values of
//! their `js` queries must be the ones that Chromium 148 gave
//! (`tests/data/css-transitions.json`: `uv run --project tools swbtools
//! probe --json FILE`, three equal runs, then `jq '.cases | map({(.name):
//! [.results[] | (.value // null)]}) | add'`), with the tolerances of ADR
//! 0044 section 12 ([`same`]).
//!
//! [`WAITS`] lists the rows that need a later change, with the reason;
//! [`SWB_ROWS`] the deliberate departures; [`EITHER`] the rows whose order
//! Chromium gives two ways.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::animation_values::{open, same};
use common::{SKIP_QUERY as SKIP, on_page_thread, run_probe_file_with, value};

use serde_json::Value;

/// The rows that wait for a later change: (case, query index or `None`
/// for every query of the case, reason).
const WAITS: &[(&str, Option<usize>, &str)] = &[
    // `interpolate-size: allow-keywords` parses and inherits (rows 0 and
    // 4); the interpolation of a length with `auto` or `max-content` needs
    // the keyword's size from layout (`calc-size()`), which is not done
    // (ADR 0044, "Done in feature 5": a second task). Without it the
    // properties change at once, as for `numeric-only`.
    (
        "interpolate-size",
        Some(1),
        "interpolate-size (calc-size in layout)",
    ),
    (
        "interpolate-size",
        Some(2),
        "interpolate-size (calc-size in layout)",
    ),
    (
        "interpolate-size",
        Some(3),
        "interpolate-size (calc-size in layout)",
    ),
];

/// The rows where swb departs from Chromium on purpose: (case, query
/// index, part of Chromium's value, swb's part; every occurrence).
const SWB_ROWS: &[(&str, usize, &str, &str)] = &[
    // `getKeyframes()` of a CSS transition is empty (Chromium: the start
    // and end keyframes; ADR 0044 section 20, backlog).
    (
        "api-after-end-and-cancel",
        0,
        "0 2 [object CSSTransition]",
        "0 0 [object CSSTransition]",
    ),
    // `cancel()` releases the record of a transition: `play()` on the held
    // wrapper does not replay it (Chromium replays it from 0 while the
    // style keeps the end value; ADR 0044 section 20, backlog). So the
    // width stays the after-change value 5px, and removing the class then
    // starts a new transition back to 20px.
    (
        "api-after-end-and-cancel",
        9,
        "running 0 width:20px 1",
        "running 0 width:5px 0",
    ),
    (
        "api-after-end-and-cancel",
        10,
        "0  width:20px",
        "1 running:0 width:5px",
    ),
];

/// Adjacent lines that Chromium gives in either order between runs: (the
/// first line, the second line as recorded); swb's value may have them
/// swapped.
const EITHER: &[(&str, &str)] = &[
    // Three cancels of one style change (ADR 0044 section 6): the cancel by
    // `display: none` (`e2`) has an unresolved time and comes first in every
    // run; Chromium gives the reversal of `e1` and the `transition: none` of
    // `e3` in either order (`e3` first in the recorded run); swb gives
    // composite order (`e1` first).
    (
        "transitioncancel opacity ~ - e3 true true TransitionEvent",
        "transitioncancel opacity ~ - e1 true true TransitionEvent",
    ),
];

/// The expected value of a query: Chromium's, with the departures, or
/// [`SKIP`].
fn expected(name: &str, index: usize, chromium: &Value) -> Value {
    if WAITS
        .iter()
        .any(|&(case, query, _)| case == name && query.is_none_or(|q| q == index))
    {
        return Value::String(SKIP.to_owned());
    }
    let Some(text) = chromium.as_str() else {
        return chromium.clone();
    };
    let mut text = text.to_owned();
    for &(case, query, chromium_part, swb_part) in SWB_ROWS {
        if case == name && query == index {
            text = text.replace(chromium_part, swb_part);
        }
    }
    Value::String(text)
}

/// [`same`], or with the lines of a pair of [`EITHER`] swapped.
fn same_or_either(want: &Value, got: &Value) -> bool {
    same(want, got)
        || EITHER.iter().any(|&(first, second)| {
            let recorded = format!("{first}\n{second}");
            let swapped = format!("{second}\n{first}");
            want.as_str().is_some_and(|w| {
                w.contains(&recorded) && same(&Value::String(w.replace(&recorded, &swapped)), got)
            })
        })
}

/// The rows of `css-transitions.json`: the start, reversal and cancel of
/// transitions by hover, focus, classes, attributes and inline styles, the
/// values at seek times, the events and their order, `CSSTransition` and
/// the properties.
#[test]
fn transition_probe_rows() {
    on_page_thread(|| {
        let failures = run_probe_file_with("css-transitions", open, expected, same_or_either);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// `getAnimations()` of the document and of shadow roots (DocumentOrShadowRoot):
/// each lists the animations of the elements of its own tree, also with
/// nested shadow trees (the probe case `shadow-tree` has one tree).
#[test]
fn shadow_roots_list_the_animations_of_their_trees() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>#d{width:10px;transition:width 1s}\
            #d.on{width:20px}</style><div id=d></div><div id=h></div><script>\
            const css='<style>.t{width:10px;transition:width 1s}.t.on{width:20px}\
            :host{display:block}</style>';\
            const r=document.getElementById('h').attachShadow({mode:'open'});\
            r.innerHTML=css+'<div class=t id=s></div><div id=h2></div>';\
            const r2=r.getElementById('h2').attachShadow({mode:'open'});\
            r2.innerHTML=css+'<div class=t id=n></div>';\
            const all=[document.getElementById('d'),r.getElementById('s'),r2.getElementById('n')];\
            for(const e of all)getComputedStyle(e).width;\
            for(const e of all)e.classList.add('on');\
            function ids(root){return root.getAnimations().map((a)=>a.effect.target.id).join()}\
            </script>";
        let mut page = open(&serde_json::json!({ "html": html }));
        let got = value(
            &mut page,
            "[ids(document), ids(r), ids(r2), r2.getAnimations()[0].transitionProperty].join(' ')",
        );
        assert_eq!(got, "d s n width");
    });
}
