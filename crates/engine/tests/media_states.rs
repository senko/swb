//! M14 feature 6a (ADR 0047 section 10): the states, values and events of
//! the media element against Chromium. The cases of
//! `tools/probes/media-states.json` run in the engine in virtual time
//! against a simulated case server (`common/media_site.rs`), with the
//! queries of the probe file, and their answers are compared with Chromium
//! 148's (`tests/data/media-states.json`: three equal runs of
//! `swbtools probe --json tools/probes/media-states.json`; the `NaN` of a
//! duration is read as `null`).
//!
//! Feature 6b runs the cases that play (`format-play-to-end-table`, the
//! `play` and `after` parts of the `err-*` answers, `#t=` in
//! `sel-fragment-time` and `sel-currentSrc-url`), with times compared as
//! `common/media_probe.rs` does (Chromium plays in real time). Skipped (layout
//! of feature 7): the `boxes` rows of the `size-*` cases. `DEPARTURES` lists
//! what differs from Chromium on purpose.
//!
//! The tests need the feature `media` (`just test-media`); without it
//! `tests/media_without_feature.rs` checks the element.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::media_site::MediaSite;
use common::on_page_threads;
use common::probe_rows::load_cases;
use serde_json::Value;
use swb_engine::{ClockMode, JsonForm, PageClock, Size, Url};
use swb_net::Fetcher;

const ANSWERS: &str = include_str!("data/media-states.json");
const PROBE: &str = "tools/probes/media-states.json";

/// The cases that this test does not run, with the reason.
const SKIPPED: &[(&str, &str)] = &[("size-video-no-metadata", "only a `boxes` row (feature 7)")];

/// Where swb differs from Chromium on purpose: (case, path in the answer,
/// reason). A path that does not differ fails the test too.
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "err-corrupt-mp4",
        "#0.s.err",
        "the message of a failure that the playback meets later: Chromium names the packet (`Failed to send audio packet for decoding: {timestamp=...}`), swb has `video decode error!` (code 3 in both)",
    ),
    (
        "format-play-to-end-table",
        "#0[3][6]",
        "the M4A file: Chromium's `buffered` starts at -0.02 (the priming of the AAC), swb's at 0 (the `buffered` rows of `format-metadata-table` compare within 0.02 s)",
    ),
    (
        "sel-fragment-time",
        "#0.ev[1]",
        "`#t=1.0`: the unit of the metadata seeks to the start position before its events `progress` and `suspend` are dispatched, so they read `currentTime` 1 where Chromium's read 0 (`durationchange` and the later events read 1 in both)",
    ),
    (
        "sel-fragment-time",
        "#0.ev[2]",
        "`suspend` after `progress` (see above)",
    ),
    (
        "format-timeline-table",
        "#0[1][4]",
        "the MP4 with a start time of 1.5 s: at `loadedmetadata` Chromium has `currentTime` 0 and the duration 2.023991 (the track's own), and 150 ms later 1.476009 and 3.496145 (the rule of `swb_media::timeline`, which the other 25 rows and this one agree with); swb has the later values at `loadedmetadata`",
    ),
    (
        "load-webm-preload-metadata",
        "#0.ev",
        "Chromium fires a second `suspend` after `canplaythrough` for the 78 KB WebM file (not for the 56 KB MP4 or the 16 KB MP3); swb fires one",
    ),
    (
        "load-webm-preload-default",
        "#0.ev",
        "the second `suspend` of the WebM file at `preload` `metadata` (see above)",
    ),
    (
        "sel-type-skips-unsupported",
        "#0.ev",
        "the second `suspend` of the WebM file (the case loads `b.webm`)",
    ),
    (
        "sel-media-attribute-no-match",
        "#0.ev",
        "the second `suspend` of the WebM file (the case loads `b.webm`)",
    ),
    (
        "sel-src-attribute-wins",
        "#0.ev",
        "the second `suspend` of the WebM file (the case loads `b.webm`)",
    ),
];

/// The answers with `NaN` read as `null`.
fn answers() -> &'static str {
    common::media_probe::answers(ANSWERS)
}

/// The folder of a probe case on the probe's server (`case-N`).
fn folder_of(name: &str) -> String {
    common::media_probe::folder_of(PROBE, name)
}

/// The simulated server of a probe case.
fn site_of(case: &Value) -> MediaSite {
    common::media_probe::site_of(PROBE, case)
}

/// What this test compares for the case: the query of each `js` row and the
/// part of Chromium's answer that it compares.
fn rows(case: &Value, answer: &Value) -> Vec<(String, Value)> {
    let mut rows = Vec::new();
    for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
        let Some(js) = query["js"].as_str() else {
            continue;
        };
        rows.push((js.to_owned(), answer["results"][index]["value"].clone()));
    }
    rows
}

/// The differences of one case: (path, text).
fn run_case(case: &'static Value, answer: &'static Value) -> Vec<(String, String)> {
    let name = case["name"].as_str().unwrap();
    let site = Arc::new(site_of(case));
    let mut page = common::new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(&MediaSite::page_url(&folder_of(name))).unwrap());
    assert!(
        page.wait_until_loaded(Duration::from_secs(30)),
        "{name}: load"
    );
    let mut out = Vec::new();
    for (index, (js, want)) in rows(case, answer).into_iter().enumerate() {
        let got = match page.evaluate_with_timeout(&js, JsonForm::Strict, Duration::from_secs(60)) {
            Some(swb_engine::Evaluation::Value(text)) => {
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            }
            Some(swb_engine::Evaluation::Error(text)) => Value::String(format!("error: {text}")),
            None => Value::String("no runtime".to_owned()),
        };
        let mut differences = Vec::new();
        common::media_probe::fuzzy_differences(&format!("#{index}"), &got, &want, &mut differences);
        out.extend(
            differences
                .into_iter()
                .filter(|(path, text)| !same_ranges(path, text)),
        );
    }
    out
}

/// The numbers of a `buffered` or `seekable` text (`0-2,3-4`).
fn range_numbers(text: &str) -> Option<Vec<f64>> {
    text.trim_matches('"')
        .split([',', '-'])
        .map(|part| part.trim().parse().ok())
        .collect()
}

/// True for a difference of `buf` or `seek` texts whose numbers are within
/// 0.02 s: Chromium's buffered end of the same file depends on `preload` (an
/// MP3 of 2 s is `0-2` at `metadata` and `0-2.01` at `auto`), and its
/// seekable end rounds the duration of Opus and ADTS files up.
fn same_ranges(path: &str, text: &str) -> bool {
    let last = path.rsplit('.').next().unwrap_or_default();
    if last != "buf" && last != "seek" {
        return false;
    }
    let Some(rest) = text.strip_prefix("swb ") else {
        return false;
    };
    let Some((swb, chromium)) = rest.split_once(", Chromium ") else {
        return false;
    };
    match (range_numbers(swb), range_numbers(chromium)) {
        (Some(a), Some(b)) => {
            a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 0.02)
        }
        _ => false,
    }
}

#[test]
fn the_states_of_the_media_element_match_chromium() {
    let cases = load_cases(PROBE, answers());
    let runs: Vec<_> = cases
        .into_iter()
        .filter(|(case, _)| {
            let name = case["name"].as_str().unwrap();
            !SKIPPED.iter().any(|(skipped, _)| *skipped == name)
        })
        .map(|(case, answer)| move || (case["name"].as_str().unwrap(), run_case(case, answer)))
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
