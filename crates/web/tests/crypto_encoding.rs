//! `crypto`, `atob`, `btoa`, `TextEncoder` and `TextDecoder` (M8 feature
//! 8a2, ADR 0028 section 13): the cases of the Chromium probe
//! `tools/probes/web-crypto-encoding.json` (the integer arrays of
//! `getRandomValues` and its errors, UUIDs, the forgiving base64 of the
//! HTML standard, UTF-8 encoding, the labels, the streaming, the BOM and the
//! fatal flag of the decoder), each with and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "crypto_encoding/cases.rs"]
mod cases;
mod common;

use common::{Departure, probe_failures_in};
use swb_web::ClockMode;

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "crypto",
        25,
        r#""undefined""#,
        "swb has no `crypto.subtle` (ADR 0028 section 13)",
    ),
    (
        "text-decoder",
        53,
        r#""RangeError: Failed to execute 'decode' on 'TextDecoder': The buffer of 104857600 bytes is too large to decode (the limit is 67108864).""#,
        "swb decodes at most 64 MiB in one call (the result is one string on the script heap); Chromium decodes 300 MiB",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
