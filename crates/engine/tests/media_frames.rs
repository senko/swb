//! M14 feature 7a (ADR 0047 section 9): what a `<video>` shows in a
//! screenshot, against Chromium. The placement cases and the first frame and
//! the end (`tools/probes/media-frames.json`, the 42 cases that the data holds
//! (the probe file has more: the colors and times are in the `swb-media`
//! tests): `object-fit`, `object-position`, padding and border, rounded
//! clips, `clip-path`, transforms, scales, opacity, a filter, overlap, a
//! 321 x 241 video with a dense row of 560 points), and the poster rules
//! (`tools/probes/media-poster.json`, 24 cases) run in the engine
//! in virtual time against a simulated case server (`common/media_site.rs`),
//! and the `pixels` rows (a screenshot at the end of the case's script) are
//! compared with Chromium 148's: **3 levels** on every point (the frame's
//! conversion), except the points that `DEPARTURES` lists with their reason.
//! The `js` rows (the state at each step) are compared as in the playback
//! tests (`common/media_probe.rs`), the `boxes` rows within 0.5 px.
//!
//! Chromium's data: `tests/data/media-frames.json` and `media-poster.json`
//! (`swbtools probe --json tools/probes/FILE`, three equal runs; the poster
//! rows of `poster-play-pause-at-start` and `poster-play-and-run` differ
//! between Chromium's runs by the frame that its real-time clock shows, so
//! `media-poster-variants.json` holds the other runs: a row passes when swb
//! equals one of the three).
//!
//! The tests need the feature `media` (`just test-media`).

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::media_probe::{
    FIRST_QUERY_MS, answers, evaluate, fuzzy_differences, load_case, run_steps,
};
use common::on_page_threads;
use common::probe_rows::{box_row, load_cases};
use serde_json::Value;
use swb_engine::{Page, Pixmap};

const FRAMES: &str = include_str!("data/media-frames.json");
const FRAMES_PROBE: &str = "tools/probes/media-frames.json";
const POSTER: &str = include_str!("data/media-poster.json");
const POSTER_VARIANTS: &str = include_str!("data/media-poster-variants.json");
const POSTER_PROBE: &str = "tools/probes/media-poster.json";

/// The levels that a point may differ: the conversion of the frame (ADR
/// 0047 section 9).
const LEVELS: i32 = 3;

/// A point of a `pixels` row where swb differs from Chromium by more than
/// [`LEVELS`] on purpose: the case, the row, the point (relative to the
/// element), the most levels that it may differ, and why. A point that
/// matches within [`LEVELS`] does not belong here: the test fails for it.
type Departure = (&'static str, usize, (i64, i64), i32, &'static str);

const CURVE_EDGE: &str = "the anti-aliased edge of a circle (coverage rounding)";
const REDUCTION: &str = "a frame drawn at 0.37 of its size: swb draws it from an averaged \
    level (`swb_paint::reduce`), Chromium from its own filter; a point at the edge of a bar \
    differs by 14 levels";

/// The points that `DEPARTURES` allows to differ. The odd 321 x 241 frames
/// need none since the review of 7a: the conversion pads an odd frame to
/// even chroma sizes (ADR 0047 section 9), and the dense row
/// `frames-dense-contain-odd` (560 points) matches within [`LEVELS`].
const DEPARTURES: &[Departure] = &[
    ("frames-clip-path-circle", 1, (60, 120), 7, CURVE_EDGE),
    ("frames-scale-down-0.37x", 1, (39, 22), 15, REDUCTION),
];

/// What a `pixels` row compared: the points that differ too much, and the
/// listed points that did not need their allowance.
struct Compared {
    wrong: Vec<String>,
    unneeded: Vec<String>,
}

/// The colors at the points of a `pixels` row of a screenshot, against the
/// values `want` of Chromium: the points that differ by more than `LEVELS`
/// (or the allowance of `DEPARTURES` for the point).
fn differing_points(
    shot: &Pixmap,
    name: &str,
    row: usize,
    result: &Value,
    want: &Value,
) -> Compared {
    let area = result["area"].as_array().unwrap();
    let (left, top) = (
        area[0].as_f64().unwrap().round() as i64,
        area[1].as_f64().unwrap().round() as i64,
    );
    let points = result["points"].as_array().unwrap();
    let mut wrong = Vec::new();
    let mut unneeded = Vec::new();
    for (point, want) in points.iter().zip(want["pixels"].as_array().unwrap()) {
        let allowance = DEPARTURES
            .iter()
            .find(|(c, r, p, _, _)| {
                *c == name
                    && *r == row
                    && *p == (point[0].as_i64().unwrap(), point[1].as_i64().unwrap())
            })
            .map(|d| d.3);
        let (x, y) = (
            point[0].as_i64().unwrap() + left,
            point[1].as_i64().unwrap() + top,
        );
        // Outside the screenshot the page is white (as in the probe).
        let got = match (u32::try_from(x), u32::try_from(y)) {
            (Ok(ux), Ok(uy)) => shot.pixel(ux, uy).map_or([255; 3], |p| {
                let p = p.demultiply();
                [p.red(), p.green(), p.blue()]
            }),
            _ => [255; 3],
        };
        let want: Vec<i32> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let worst = got
            .iter()
            .zip(&want)
            .map(|(g, w)| (i32::from(*g) - w).abs())
            .max()
            .unwrap_or(0);
        let text = format!(
            "({}, {}): swb {got:?}, Chromium {want:?}",
            point[0], point[1]
        );
        match allowance {
            Some(_) if worst <= LEVELS => unneeded.push(text),
            Some(limit) if worst > limit => wrong.push(text),
            None if worst > LEVELS => wrong.push(text),
            _ => {}
        }
    }
    Compared { wrong, unneeded }
}

/// The screenshot of the page now (virtual time: the media have settled).
fn shot(page: &mut Page) -> Pixmap {
    page.screenshot(true).unwrap()
}

/// What a case gives: the failures of its rows. `others` are the same case in
/// the other runs of Chromium: a `pixels` row passes with any of them.
fn run_case(
    probe: &'static str,
    case: &'static Value,
    answer: &'static Value,
    others: &[&'static Value],
) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let (mut page, _site) = load_case(probe, case);
    page.run_event_loop(
        Duration::from_millis(FIRST_QUERY_MS),
        Duration::from_secs(60),
    );
    page.note_user_activation();
    run_steps(&mut page, case);
    let measures = case["measure"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut js_index = 0;
    for (row, result) in answer["results"].as_array().unwrap().iter().enumerate() {
        let mut failure: Vec<String> = Vec::new();
        match result["kind"].as_str().unwrap() {
            "js" => {
                // The rows of Chromium's answer keep the order of `measure`.
                let query = measures
                    .iter()
                    .filter(|m| m["js"].is_string())
                    .nth(js_index)
                    .unwrap();
                js_index += 1;
                let got = evaluate(&mut page, query["js"].as_str().unwrap());
                let mut out = Vec::new();
                fuzzy_differences(&format!("#{row}"), &got, &result["value"], &mut out);
                failure.extend(out.into_iter().map(|(p, t)| format!("{name} {p}: {t}")));
            }
            "boxes" => {
                failure.extend(box_row(name, row, &mut page, result, 0.5));
            }
            "pixels" => {
                let screenshot = shot(&mut page);
                let mut best = differing_points(&screenshot, name, row, result, result);
                for other in others {
                    if best.wrong.is_empty() {
                        break;
                    }
                    let alternative = &other["results"][row];
                    let compared = differing_points(&screenshot, name, row, result, alternative);
                    if compared.wrong.len() < best.wrong.len() {
                        best = compared;
                    }
                }
                if !best.wrong.is_empty() {
                    failure.push(format!(
                        "{name} #{row}: {} points differ: {}",
                        best.wrong.len(),
                        best.wrong.join("; ")
                    ));
                }
                for text in best.unneeded {
                    failure.push(format!(
                        "{name} #{row} {text} matches Chromium now: remove it from DEPARTURES"
                    ));
                }
            }
            other => failure.push(format!("{name} #{row}: unknown kind {other}")),
        }
        failures.extend(failure);
    }
    failures
}

/// Runs every case of `probe` against its answers; `variants` (the text of a
/// data file with entries by case name) holds other answers of Chromium for
/// the cases whose rows vary between its runs.
fn failures_of(
    probe: &'static str,
    text: &'static str,
    variants: Option<&'static str>,
) -> Vec<String> {
    let cases = load_cases(probe, answers(text));
    let variants: Vec<&'static Value> = variants
        .map(|t| {
            let value: &'static Value =
                Box::leak(Box::new(serde_json::from_str::<Value>(answers(t)).unwrap()));
            value["cases"].as_array().unwrap().iter().collect()
        })
        .unwrap_or_default();
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, answer)| {
            let alternatives: Vec<&'static Value> = variants
                .iter()
                .copied()
                .filter(|v| v["name"] == answer["name"])
                .collect();
            move || run_case(probe, case, answer, &alternatives)
        })
        .collect();
    on_page_threads(runs).into_iter().flatten().collect()
}

#[test]
fn the_placement_of_a_video_frame_matches_chromium() {
    let failures = failures_of(FRAMES_PROBE, FRAMES, None);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_poster_rules_match_chromium() {
    let failures = failures_of(POSTER_PROBE, POSTER, Some(POSTER_VARIANTS));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
