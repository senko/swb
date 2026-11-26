//! M14 feature 6b (ADR 0047 section 10): the playback API of the media
//! element against Chromium. The cases of `tools/probes/media-playback.json`
//! (30) and `media-playback-2.json` (5: removal and play again, play after a
//! seek to the end, `autoplay` cancelled by `pause()` at `loadedmetadata` and
//! at `canplay`, `pause()` during a seek; cases that no row of the first file
//! decides) run in the engine in virtual time against a simulated case server
//! (`common/media_site.rs`), and their answers are compared with Chromium
//! 148's (`tests/data/media-playback.json` and `media-playback-2.json`:
//! `swbtools probe --json tools/probes/FILE`, three runs each; the only
//! differences between the runs are a `*` of a run of `timeupdate` events and
//! a `ct` within 0.04 s). The comparison is fuzzy in time
//! (`common/media_probe.rs`): Chromium plays in real time, swb in virtual
//! time. `DEPARTURES` lists what differs on purpose.
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

const ANSWERS: &str = include_str!("data/media-playback.json");
const PROBE: &str = "tools/probes/media-playback.json";
const ANSWERS_2: &str = include_str!("data/media-playback-2.json");
const PROBE_2: &str = "tools/probes/media-playback-2.json";

/// Where swb differs from Chromium on purpose: (case, path in the answer,
/// reason). A path that does not differ fails the test too.
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "new-audio",
        "#0.ev[1]",
        "`progress` is dispatched with `readyState` 0 in Chromium and 1 in swb: the unit of the metadata (feature 6a) sets `readyState` 1 before its events `progress`, `suspend`, `durationchange` and `loadedmetadata` are dispatched",
    ),
    (
        "new-audio",
        "#0.ev[2]",
        "`suspend` after `progress` (see above)",
    ),
];

/// Runs every case of `probe` and returns the failures.
fn failures_of(probe: &'static str, answers_text: &'static str) -> Vec<String> {
    let cases = load_cases(probe, answers(answers_text));
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, answer)| {
            move || {
                (
                    case["name"].as_str().unwrap(),
                    run_case(probe, case, answer),
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
    // A departure of this file's cases that no longer occurs.
    for (case, path, _) in DEPARTURES {
        let ours = std::fs::read_to_string(common::repo_root().join(probe))
            .unwrap()
            .contains(&format!("\"name\": \"{case}\""));
        if ours && !seen_departures.iter().any(|(c, p)| c == case && p == path) {
            failures.push(format!("{case} {path}: the departure no longer occurs"));
        }
    }
    failures
}

#[test]
fn the_playback_of_the_media_element_matches_chromium() {
    let failures = failures_of(PROBE, ANSWERS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn removal_play_after_a_seek_to_the_end_autoplay_and_pause_during_a_seek_match_chromium() {
    let failures = failures_of(PROBE_2, ANSWERS_2);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
