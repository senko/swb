//! `navigator` and the page visibility (M8 feature 8a2, ADR 0028 section
//! 13): the cases of the Chromium probe `tools/probes/web-navigator.json`
//! (the interface and its members, the values that swb chooses, the
//! members that it does not have, `document.hidden` and `visibilityState`),
//! each with and without the GC stress mode, in virtual time.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "navigator/cases.rs"]
mod cases;
mod common;

use common::{Departure, probe_failures_in};
use swb_web::ClockMode;

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "navigator-departures",
        0,
        r#""""#,
        "swb does not claim to be Chromium: `vendor` is empty (ADR 0028 section 13)",
    ),
    (
        "navigator-departures",
        1,
        "false",
        "`webdriver` is true only under WebDriver control; swb's automation API is not WebDriver",
    ),
    (
        "navigator-departures",
        2,
        "false",
        "the user agent string of swb is not Chromium's (ADR 0025)",
    ),
    (
        "navigator-departures",
        3,
        "false",
        "no `userAgentData`: swb does not claim to be Chromium",
    ),
    (
        "navigator-departures",
        4,
        r#""undefined""#,
        "`sendBeacon` comes with the network from scripts (feature 10b)",
    ),
    (
        "navigator-departures",
        5,
        r#"["undefined","undefined","undefined","undefined","undefined"]"#,
        "members that pages use only for feature detection stay absent (ADR 0028 section 13); `plugins` and `mimeTypes` are on the backlog",
    ),
    (
        "navigator-departures",
        6,
        "false",
        "no `clientInformation` (a Chromium alias of `navigator`; backlog)",
    ),
    (
        "visibility",
        8,
        "null",
        "no `webkitHidden` (a prefixed Chromium member)",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
