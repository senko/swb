//! Cases for the iterator helpers (M7 feature 6c): the measured Chromium
//! cases (`tools/probes/js-iterators.json`), helpers with allocations and
//! collections inside callbacks and user iterators, and the steps that skip
//! the result object. Each case also runs in the GC stress mode.

mod common;

use common::{check_cases, check_chromium, run};

/// The snippets of `tools/probes/js-iterators.json` with Chromium 148's
/// results (`data/iterators_cases.expected`; the script and the expected
/// output are made by `data/gen-iterator-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/iterators_cases.js"),
        include_str!("data/iterators_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with chains of thousands of helpers, without the
/// stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/iterators_cases_heavy.js"),
        include_str!("data/iterators_cases_heavy.expected"),
        &[false],
    );
}

/// Helpers with allocations and collections inside the callbacks, the
/// `next` of user iterators, the inner iterables of `flatMap` and
/// `Iterator.concat`, and helpers that only other helpers refer to (the
/// expected output is that of Node.js 22; `just jsdiff` compares it).
#[test]
fn helpers_with_collections_in_callbacks_and_iterators() {
    check_cases(&[(
        include_str!("data/iterators_gc.js"),
        include_str!("data/iterators_gc.expected").trim_end_matches('\n'),
    )]);
}

/// Patched `next` methods, subclasses of `Iterator`, `return` before the
/// first `next`, deep chains and the result object (the expected output
/// is that of Node.js 22).
#[test]
fn patched_methods_subclasses_and_chains() {
    check_cases(&[(
        include_str!("data/iterators_misc.js"),
        include_str!("data/iterators_misc.expected").trim_end_matches('\n'),
    )]);
}

/// The string forms of helpers and the realm's intrinsics.
#[test]
fn string_forms_of_helpers() {
    let out = run(
        "print(Object.prototype.toString.call([].values().map(function (x) { return x; })), \
         Object.prototype.toString.call(Iterator.from({ next: function () {} })), \
         Object.prototype.toString.call(Iterator.concat()), \
         String(Iterator));",
        false,
    );
    assert_eq!(
        out,
        "[object Iterator Helper] [object Iterator] [object Iterator Helper] function Iterator() { [native code] }"
    );
}
