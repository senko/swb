//! Cases for `Map` and `Set` (M7 feature 6b): the measured Chromium cases
//! (`tools/probes/js-collections.json`), collections inside callbacks,
//! adders, `has`, `keys` and `next` of the set methods' argument, and
//! iterators that outlive compactions. Each case also runs in the GC stress
//! mode.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-collections.json` with Chromium 148's
/// results (`data/collections_cases.expected`; the script and the expected
/// output are made by `data/gen-collection-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/collections_cases.js"),
        include_str!("data/collections_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with loops of thousands of steps, without the stress
/// mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/collections_cases_heavy.js"),
        include_str!("data/collections_cases_heavy.expected"),
        &[false],
    );
}

fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
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

/// Collections inside callbacks, adders, iterators and the argument of the
/// set methods (the expected output is that of Node.js 22).
#[test]
fn collections_in_callbacks_and_iterators() {
    check_cases(&[(
        include_str!("data/collections_gc.js"),
        include_str!("data/collections_gc.expected").trim_end_matches('\n'),
    )]);
}

/// Random operations on maps and sets with live iterators, native loops and
/// the set methods (`data/collections_fuzz.js`): the number of trace
/// lines, their length and a checksum are those of Node.js 22 (60 rounds
/// normally; 6 rounds in the stress mode, which is slow).
#[test]
fn random_operations_agree_with_node() {
    let script = include_str!("data/collections_fuzz.js");
    for (rounds, stress, expected) in [
        (60, false, "15170 106368 -149670935"),
        (6, false, "1447 9509 -946570156"),
        (6, true, "1447 9509 -946570156"),
    ] {
        let actual = run(&format!("var ROUNDS = {rounds};\n{script}"), stress);
        assert_eq!(actual, expected, "rounds {rounds}, stress {stress}");
    }
}

/// `console.log` of maps and sets: `Map(1) { 1 => 2 }`, `Set(2) { 1, 2 }`,
/// subclasses, nesting beyond the depth, more than 100 entries, cycles and
/// line breaks (the expected output is that of Node.js 22).
#[test]
fn console_log_shows_collections() {
    check_cases(&[(
        include_str!("data/collections_console.js"),
        include_str!("data/collections_console.expected").trim_end_matches('\n'),
    )]);
}

/// The string forms of collections and their iterators.
#[test]
fn string_forms_of_collections() {
    let out = run(
        "print(String(new Map([[1, 2]])), String(new Set([1])), Object.prototype.toString.call(new Map().keys()));",
        false,
    );
    assert_eq!(out, "[object Map] [object Set] [object Map Iterator]");
}
