//! `performance` and `PerformanceObserver` (M8 feature 8a2, ADR 0028
//! section 13): the cases of the Chromium probe
//! `tools/probes/web-performance.json` (the interfaces, `now` and
//! `timeOrigin`, `timing` and `navigation`, the navigation entry, marks,
//! measures with their options and errors, the entry lists and `clear*`, the
//! observer with its delivery as a task), each with and without the GC
//! stress mode, in virtual time.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "performance/cases.rs"]
mod cases;
mod common;

use common::{Departure, probe_failures_in};
use swb_web::ClockMode;

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "navigation-entry",
        8,
        r#""navigation""#,
        "no `visibility-state` entry (Chromium 148 adds one): swb makes only marks, measures and the navigation entry",
    ),
    (
        "navigation-departures",
        0,
        "[true,true]",
        "swb has no previous document to unload: `unloadEventStart` and `unloadEventEnd` are 0 (Chromium: the time of the unload of the blank page that precedes a file)",
    ),
    (
        "navigation-departures",
        1,
        r#"[true,true,false,true,"undefined","undefined","undefined","undefined"]"#,
        "the loader gives no times for the phases of the request (`responseEnd` and the others are 0); no `activationStart`, `notRestoredReasons`, `confidence` and `serverTiming`",
    ),
    (
        "navigation-departures",
        2,
        r#"["undefined","undefined","undefined","undefined"]"#,
        "no `eventCounts`, `memory`, `interactionCount` and `onresourcetimingbufferfull` (Event Timing and Chromium's own members)",
    ),
    (
        "navigation-departures",
        3,
        r#""[\"mark\",\"measure\",\"navigation\"]""#,
        "swb supports the entry types that it makes",
    ),
    (
        "navigation-departures",
        4,
        "false",
        "`supportedEntryTypes` is a new frozen array at each read",
    ),
    (
        "navigation-departures",
        5,
        "39",
        "the navigation entry has 8 members fewer than Chromium's `toJSON`",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
