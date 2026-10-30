//! `DOMMatrix`, `DOMMatrixReadOnly`, `WebKitCSSMatrix`, `DOMPoint` and
//! `DOMPointReadOnly` (M8 feature 11c1, ADR 0028 section 13): the cases of
//! the Chromium probe `tools/probes/web-geometry-matrix.json` (the interface
//! objects and their properties, matrices from strings, sequences,
//! dictionaries and typed arrays, the attributes and their 2D flag, every
//! operation and its `...Self` form, inversion, stringification, `toJSON`,
//! points), each with and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "geometry_matrix/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A matrix string of more than 1 MiB is a syntax error, and a long list of
/// functions inside the limit is multiplied in time.
#[test]
fn matrix_strings_are_bounded() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", false);
        let long = "{ const m = new DOMMatrix('translate(1px) '.repeat(60000)); [m.e, m.is2D] }";
        assert_eq!(answer(&mut rt, long), expected("[60000, true]"));
        let huge = "{ let name = ''; try { new DOMMatrix('scale(1) '.repeat(400000)); } catch (e) { name = e.name; } name }";
        assert_eq!(answer(&mut rt, huge), expected(r#""SyntaxError""#));
    });
}
