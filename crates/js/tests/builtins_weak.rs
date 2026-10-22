//! Cases for `WeakMap`, `WeakSet`, `WeakRef` and `FinalizationRegistry`
//! (M7 feature 9b): the measured Chromium cases (`tools/probes/js-weak.json`:
//! the shape of the four constructors and their prototypes, the
//! constructors with iterables and adders, every method with valid and
//! invalid keys, V8's messages, symbols as keys, proxies as keys, the
//! upsert methods and the uses of the target scripts), and the
//! `console.log` of the weak objects. Each case also runs in the GC stress
//! mode. The collector cases are in `weak.rs`.

mod common;

use common::{check_cases, check_chromium};

/// The snippets of `tools/probes/js-weak.json` with Chromium 148's
/// results (`data/weak_cases.expected`; the script and the expected output
/// are made by `data/gen-weak-cases.js` from the output of `just probe`),
/// normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/weak_cases.js"),
        include_str!("data/weak_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with 100,000 to 300,000 entries, cells or
/// references, without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/weak_cases_heavy.js"),
        include_str!("data/weak_cases_heavy.expected"),
        &[false],
    );
}

/// `console.log` of the weak objects, as Node.js 22 prints them.
#[test]
fn console_log() {
    check_cases(&[(
        include_str!("data/weak_console.js"),
        include_str!("data/weak_console.expected").trim_end_matches('\n'),
    )]);
}
