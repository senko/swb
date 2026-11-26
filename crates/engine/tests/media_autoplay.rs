//! M14 feature 6b (ADR 0047 section 10, `docs/media-diagnosis.md` section 4):
//! the autoplay policy and the user activation against Chromium. The cases of
//! `tools/probes/media-autoplay.json` (27) run in the engine in virtual time
//! against a simulated case server, with their steps (`click`, `hover`,
//! `key`, `wait`, `eval`), and their answers are compared with Chromium
//! 148's (`tests/data/media-autoplay.json`: `swbtools probe --json
//! tools/probes/media-autoplay.json`, three runs). One case differs between
//! the runs of Chromium: `attr-autoplay-muted-then-unmuted-by-script` (the
//! element is paused after the unmute in 2 of 3 runs, playing in the third).
//! The data holds the first outcome and swb gives it on every run, so
//! `DEPARTURES` is empty: swb is pinned to one of the two outcomes of
//! Chromium, not different from it. Like the probe, the cases run their scripts at
//! load before any query, and each query and `eval` step gives the page a
//! user activation (`Page::set_evaluation_activation`, on by default; ADR
//! 0047 section 10).
//!
//! The tests need the feature `media` (`just test-media`); without it
//! `tests/media_without_feature.rs` checks the element.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use common::media_probe::{answers, run_case};
use common::on_page_threads;
use common::probe_rows::load_cases;

const ANSWERS: &str = include_str!("data/media-autoplay.json");
const PROBE: &str = "tools/probes/media-autoplay.json";

/// Where swb differs from Chromium on purpose: (case, path in the answer,
/// reason). A path that does not differ fails the test too.
const DEPARTURES: &[(&str, &str, &str)] = &[];

#[test]
fn the_autoplay_policy_matches_chromium() {
    let cases = load_cases(PROBE, answers(ANSWERS));
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, answer)| {
            move || {
                (
                    case["name"].as_str().unwrap(),
                    run_case(PROBE, case, answer),
                )
            }
        })
        .collect();
    let mut failures = Vec::new();
    let mut seen_departures = Vec::new();
    for (name, differences) in on_page_threads(runs) {
        for (path, text) in differences {
            if DEPARTURES.iter().any(|(c, p, _)| *c == name && *p == path) {
                seen_departures.push((name, path));
            } else {
                failures.push(format!("{name} {path}: {text}"));
            }
        }
    }
    for (case, path, _) in DEPARTURES {
        if !seen_departures.iter().any(|(c, p)| c == case && p == path) {
            failures.push(format!("{case} {path}: the departure no longer occurs"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
