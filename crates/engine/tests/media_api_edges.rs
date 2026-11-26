//! M14 feature 6b (ADR 0047 section 10) with the feature `media`: the edges of
//! the playback API against Chromium. The case `api-edges` of
//! `tools/probes/media-api-edges.json` (about 100 rows in one query: the
//! setters of `currentTime`, `volume` and the rates with the values that
//! throw, the supported range of the rates with its messages, the `ratechange`
//! events, `muted`, `new Audio()`, `played` of an element without a source)
//! runs in the engine and every row is compared with Chromium 148's
//! (`tests/data/media-api-edges.json`: `swbtools probe --json
//! tools/probes/media-api-edges.json`, three equal runs). Times have the
//! tolerance of `common/media_probe.rs`; every other value is equal.
//!
//! The tests need the feature `media` (`just test-media`).

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use common::media_probe::{answers, run_case};
use common::on_page_thread;
use common::probe_rows::load_cases;

const ANSWERS: &str = include_str!("data/media-api-edges.json");
const PROBE: &str = "tools/probes/media-api-edges.json";

#[test]
fn the_edges_of_the_playback_api_match_chromium() {
    let cases = load_cases(PROBE, answers(ANSWERS));
    assert_eq!(cases.len(), 1);
    let failures: Vec<String> = on_page_thread(move || {
        let mut failures = Vec::new();
        for (case, answer) in cases {
            for (path, text) in run_case(PROBE, case, answer) {
                failures.push(format!("{path}: {text}"));
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
