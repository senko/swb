//! `JSON` (M7 feature 5c): `parse` with the reviver and the source text
//! access, `stringify`, `rawJSON` and `isRawJSON`. Each case runs normally
//! and in the GC stress mode.
//!
//! - `chromium_cases`: the snippets of `tools/probes/js-json.json` with
//!   Chromium 148's results (`data/json_cases.expected`; the script and the
//!   expected output are made by `data/gen-json-cases.js` from the output
//!   of `just probe`).
//! - The other cases are scripts whose expected output is the output of
//!   Node.js 22 (`just jsdiff`), with collections inside revivers,
//!   replacers, `toJSON` methods and getters (`gc()` is a no-op in Node.js).

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::{check_cases, run};

#[test]
fn chromium_cases() {
    let script = include_str!("data/json_cases.js");
    let expected = include_str!("data/json_cases.expected");
    for stress in [false, true] {
        let actual = run(script, stress);
        let mut failures = Vec::new();
        for (actual, expected) in actual.lines().zip(expected.lines()) {
            if actual != expected {
                failures.push(format!("expected: {expected}\nactual:   {actual}"));
            }
        }
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

/// The snippets that nest 100,000 levels run without the stress mode.
#[test]
fn chromium_cases_heavy() {
    let script = include_str!("data/json_cases_heavy.js");
    let expected = include_str!("data/json_cases_heavy.expected");
    let actual = run(script, false);
    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn collections_inside_callbacks() {
    check_cases(&[(
        include_str!("data/json_gc.js"),
        include_str!("data/json_gc.expected").trim_end_matches('\n'),
    )]);
}

#[test]
fn deep_and_wide_structures() {
    check_cases(&[(
        include_str!("data/json_deep.js"),
        include_str!("data/json_deep.expected").trim_end_matches('\n'),
    )]);
}
