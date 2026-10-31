//! `contentEditable`, `isContentEditable` and `:read-write` in editing hosts
//! (feature 12b of M8): the cases of the Chromium probe
//! `tools/probes/web-content-editable.json` (the getter and setter, the
//! `SyntaxError` of the setter, the inheritance through the ancestors, the
//! selectors), each with and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "content_editable/cases.rs"]
mod cases;

mod common;

use common::{Departure, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why). None.
const SWB_VALUES: &[Departure] = &[];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
