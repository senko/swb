//! The legacy factory functions `Image` and `Option` of feature 6c2 with
//! the runtime of a document: the cases of the Chromium probe
//! `tools/probes/web-image-option.json` (arguments and their conversions,
//! the function object, `new.target`, a call without `new`, the node
//! document), each with and without the GC stress mode. The selectedness
//! of options (`forms-` cases) runs in `crates/engine/tests/script_html.rs`,
//! where the engine's form state exists.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "image_option/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "image-01",
        7,
        r#""|5""#,
        "`new Image(undefined, 5)`: Chromium sets width to 0 (it takes an undefined argument before a given one as given); Web IDL and swb treat it as missing",
    ),
    (
        "option-02",
        17,
        r#""false,true,img""#,
        "`new.target.prototype` is no object: Web IDL takes the interface prototype object of the realm of new.target; Chromium takes Object.prototype (ADR 0028, feature 6c2 notes)",
    ),
    (
        "option-02",
        18,
        r#""false,true,option""#,
        "`new.target.prototype` is no object: Web IDL takes the interface prototype object of the realm of new.target; Chromium takes Object.prototype (ADR 0028, feature 6c2 notes)",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Many `Image` and `Option` objects: the wrappers live as long as the
/// document, and a collection in between keeps their expandos.
#[test]
fn many_factory_objects_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load("<!DOCTYPE html><body>", stress);
            let make = "{ window.list = []; for (let i = 0; i < 200; i++) { const x = i % 2 ? new Image(i, i) : new Option('t' + i, 'v' + i); x.n = i; list.push(x); } list.length }";
            assert_eq!(answer(&mut rt, make), expected("200"));
            rt.collect();
            let check = "{ let ok = 0; for (const x of list) { if (x instanceof HTMLImageElement ? x.getAttribute('width') == x.n : x.value === 'v' + x.n) ok++; } ok }";
            assert_eq!(answer(&mut rt, check), expected("200"), "stress {stress}");
        }
    });
}

/// A loop of 100,000 factory calls stays inside the node limit and the
/// time of a task.
#[test]
fn factory_loops_are_fast() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", false);
        let source = "{ let n = 0; for (let i = 0; i < 100000; i++) { const a = new Image(); const o = new Option('x'); if (a.tagName === 'IMG' && o.text === 'x') n++; } n }";
        assert_eq!(answer(&mut rt, source), expected("100000"));
    });
}
