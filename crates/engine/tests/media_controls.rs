//! M14 feature 9 (ADR 0047 "Implementation notes (M14 feature 9)"): the
//! default controls of `<video>` and `<audio>` respond. In virtual time
//! against the simulated case server (`common/media_site.rs`), with the
//! probe cases of `tools/probes/media-controls-input.json` (the events and
//! the state that a click, a drag or a key of the controls gives, against
//! Chromium 148's rows in `tests/data/media-controls-input.json`) and
//! `tools/probes/media-controls-states.json` (the pictures: the controls'
//! pixels against the screenshots in `tests/data/media-controls/`). The
//! tests need the feature `media` (`just test-media`).
//!
//! The data: `swbtools probe --json tools/probes/media-controls-input.json
//! --case NAME... > tests/data/media-controls-input.json` (the cases that
//! need no local file, see the probe file) and `swbtools probe --shots-dir
//! crates/engine/tests/data/media-controls tools/probes/media-controls-
//! states.json` (the screenshots are `CASE-p.png`). Chromium's runs differ in
//! the `ct` of events by up to 0.04 s and in the pixels of a playing video's
//! timeline by the position of its played part. A departure rectangle has a
//! cap per state on the pixels beyond the levels (`PIXEL_CAPS`; regenerate
//! with `SWB_PRINT_CAPS=1`).

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::media_probe::{FIRST_QUERY_MS, answers, load_case, run_case, run_steps};
use common::on_page_threads;
use common::probe_rows::load_cases;
use serde_json::Value;
use swb_engine::Pixmap;

const INPUT: &str = include_str!("data/media-controls-input.json");
const INPUT_PROBE: &str = "tools/probes/media-controls-input.json";
const STATES_PROBE: &str = "tools/probes/media-controls-states.json";

/// Where swb's rows differ from Chromium's on purpose: (case, path, reason).
/// A path that does not differ fails the test too.
const INPUT_DEPARTURES: &[(&str, &str, &str)] = &[];

#[test]
fn the_events_and_state_of_clicks_and_drags_on_the_controls_match_chromium() {
    let cases = load_cases(INPUT_PROBE, answers(INPUT));
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, answer)| {
            move || {
                (
                    case["name"].as_str().unwrap(),
                    run_case(INPUT_PROBE, case, answer),
                )
            }
        })
        .collect();
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    for (name, differences) in on_page_threads(runs) {
        for (path, text) in differences {
            if INPUT_DEPARTURES
                .iter()
                .any(|(c, p, _)| *c == name && *p == path)
            {
                seen.push((name, path));
            } else {
                failures.push(format!("{name} {path}: {text}"));
            }
        }
    }
    for (case, path, _) in INPUT_DEPARTURES {
        if !seen.iter().any(|(c, p)| c == case && p == path) {
            failures.push(format!("{case} {path}: the departure no longer occurs"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ----- The pictures -----

/// The levels that a pixel of the controls may differ from Chromium's
/// (anti-aliasing of icons and glyphs, the rounding of the gradient and of
/// the composite of translucent layers).
const LEVELS: i32 = 12;

/// A rectangle (x, y, width, height in px of the element) in which swb's
/// pixels differ from Chromium's by more than `LEVELS` on purpose, with the
/// reason. A rectangle in which no case differs fails the test too.
type Rect = (i32, i32, i32, i32);

/// A pixel that differs: its place, swb's color and Chromium's.
type BadPixel = (i32, i32, [u8; 3], [u8; 3]);

const GLYPHS: &str = "the anti-aliasing of the glyphs of the time text (swb's rasterizer and \
    Chromium's differ at the edges: up to 40 levels); with the volume slider open Chromium's \
    video label is \"0:00 0:10\" (no slash) and swb's has it (the caps of those states are \
    higher: ADR 0047, backlog)";
const ICON: &str = "the edge coverage of a vector icon (the shapes are measured from \
    Chromium's pixels; its rasterizer and swb's differ at the edges)";
const CAPS: &str = "the round end of a bar (coverage of a 2 px radius)";
const THUMB: &str = "the edge of the 12 px thumb (Chromium centers it 0.5 px higher)";
const PLAYED_END: &str = "Chromium plays in real time, with a start-up of several hundred \
    ms: its position at the picture differs from swb's virtual time (the time text and the \
    end of the played part)";
const SPINNER: &str = "the loading arc turns: its angle depends on real time";

/// The rectangles of the video states.
const VIDEO_DEPARTURES: &[(Rect, &str)] = &[
    ((46, 184, 72, 16), GLYPHS),
    ((14, 184, 18, 16), ICON),
    ((190, 182, 20, 20), ICON),
    ((240, 184, 16, 16), ICON),
    ((292, 183, 8, 18), ICON),
    ((14, 214, 4, 8), CAPS),
    ((302, 214, 4, 8), CAPS),
    ((123, 189, 4, 6), CAPS),
    ((172, 189, 6, 6), CAPS),
];

/// The rectangles of the audio states.
const AUDIO_DEPARTURES: &[(Rect, &str)] = &[
    ((45, 20, 70, 15), GLYPHS),
    ((20, 19, 10, 16), ICON),
    ((232, 18, 20, 18), ICON),
    ((270, 18, 8, 18), ICON),
    ((127, 23, 4, 8), CAPS),
    ((206, 23, 4, 8), CAPS),
    ((139, 23, 8, 8), CAPS),
    ((172, 23, 6, 8), CAPS),
    ((222, 23, 6, 8), CAPS),
    (
        (155, 8, 10, 38),
        "the round left end of the volume pill (coverage)",
    ),
];

/// The rectangles of single states.
const STATE_DEPARTURES: &[(&str, Rect, &str)] = &[
    ("st-paused-initial", (130, 70, 70, 75), SPINNER),
    ("st-paused-hover-timeline", (14, 210, 16, 16), THUMB),
    ("st-paused-seek-5s", (152, 210, 16, 16), THUMB),
    ("st-paused-seek-5s-away", (152, 210, 16, 16), THUMB),
    ("st-playing-shown", (30, 214, 40, 8), PLAYED_END),
    ("st-playing-hover-pause", (30, 214, 40, 8), PLAYED_END),
    ("st-volume-half", (142, 184, 16, 16), THUMB),
    ("st-volume-zero", (122, 184, 16, 16), THUMB),
    ("st-audio-seek-5s", (162, 19, 16, 16), THUMB),
];

/// The most pixels that may differ by more than `LEVELS` in a departure
/// rectangle of a state: (state, rectangle, cap). Each cap is the count that
/// swb's pictures give (measured against the screenshots; the run is exact:
/// virtual time, the same rasterizer) with a margin of 10 % and 2 pixels.
/// A rectangle without an entry has a cap of 0. A wrong icon, a wrong digit
/// or an icon that moves by a pixel or two raises a count above its cap,
/// though it stays inside the rectangle. `SWB_PRINT_CAPS=1` prints the
/// counts of a run.
const PIXEL_CAPS: &[(&str, Rect, usize)] = &[
    ("st-audio-hover-play", (127, 23, 4, 8), 5),
    ("st-audio-hover-play", (20, 19, 10, 16), 20),
    ("st-audio-hover-play", (206, 23, 4, 8), 5),
    ("st-audio-hover-play", (232, 18, 20, 18), 96),
    ("st-audio-hover-play", (270, 18, 8, 18), 42),
    ("st-audio-hover-play", (45, 20, 70, 15), 120),
    ("st-audio-muted", (127, 23, 4, 8), 5),
    ("st-audio-muted", (139, 23, 8, 8), 5),
    ("st-audio-muted", (155, 8, 10, 38), 11),
    ("st-audio-muted", (20, 19, 10, 16), 20),
    ("st-audio-muted", (232, 18, 20, 18), 133),
    ("st-audio-muted", (270, 18, 8, 18), 42),
    ("st-audio-muted", (45, 20, 70, 15), 120),
    ("st-audio-paused", (127, 23, 4, 8), 5),
    ("st-audio-paused", (20, 19, 10, 16), 20),
    ("st-audio-paused", (206, 23, 4, 8), 5),
    ("st-audio-paused", (232, 18, 20, 18), 96),
    ("st-audio-paused", (270, 18, 8, 18), 42),
    ("st-audio-paused", (45, 20, 70, 15), 120),
    ("st-audio-playing", (127, 23, 4, 8), 8),
    ("st-audio-playing", (139, 23, 8, 8), 11),
    ("st-audio-playing", (20, 19, 10, 16), 29),
    ("st-audio-playing", (206, 23, 4, 8), 5),
    ("st-audio-playing", (232, 18, 20, 18), 96),
    ("st-audio-playing", (270, 18, 8, 18), 42),
    ("st-audio-playing", (45, 20, 70, 15), 111),
    ("st-audio-seek-5s", (127, 23, 4, 8), 8),
    ("st-audio-seek-5s", (155, 8, 10, 38), 5),
    ("st-audio-seek-5s", (162, 19, 16, 16), 63),
    ("st-audio-seek-5s", (172, 23, 6, 8), 22),
    ("st-audio-seek-5s", (20, 19, 10, 16), 20),
    ("st-audio-seek-5s", (206, 23, 4, 8), 5),
    ("st-audio-seek-5s", (232, 18, 20, 18), 96),
    ("st-audio-seek-5s", (270, 18, 8, 18), 42),
    ("st-audio-seek-5s", (45, 20, 70, 15), 121),
    ("st-audio-volume-open", (127, 23, 4, 8), 5),
    ("st-audio-volume-open", (139, 23, 8, 8), 5),
    ("st-audio-volume-open", (155, 8, 10, 38), 11),
    ("st-audio-volume-open", (172, 23, 6, 8), 5),
    ("st-audio-volume-open", (20, 19, 10, 16), 20),
    ("st-audio-volume-open", (222, 23, 6, 8), 5),
    ("st-audio-volume-open", (232, 18, 20, 18), 95),
    ("st-audio-volume-open", (270, 18, 8, 18), 42),
    ("st-audio-volume-open", (45, 20, 70, 15), 120),
    ("st-ended", (14, 184, 18, 16), 20),
    ("st-ended", (14, 214, 4, 8), 6),
    ("st-ended", (190, 182, 20, 20), 66),
    ("st-ended", (240, 184, 16, 16), 38),
    ("st-ended", (292, 183, 8, 18), 42),
    ("st-ended", (302, 214, 4, 8), 5),
    ("st-ended", (46, 184, 72, 16), 149),
    ("st-muted", (14, 184, 18, 16), 20),
    ("st-muted", (14, 214, 4, 8), 6),
    ("st-muted", (190, 182, 20, 20), 90),
    ("st-muted", (240, 184, 16, 16), 38),
    ("st-muted", (292, 183, 8, 18), 42),
    ("st-muted", (302, 214, 4, 8), 5),
    ("st-muted", (46, 184, 72, 16), 335),
    ("st-muted-open", (14, 184, 18, 16), 20),
    ("st-muted-open", (14, 214, 4, 8), 6),
    ("st-muted-open", (190, 182, 20, 20), 90),
    ("st-muted-open", (240, 184, 16, 16), 38),
    ("st-muted-open", (292, 183, 8, 18), 42),
    ("st-muted-open", (302, 214, 4, 8), 5),
    ("st-muted-open", (46, 184, 72, 16), 335),
    ("st-no-audio-track", (14, 184, 18, 16), 21),
    ("st-no-audio-track", (14, 214, 4, 8), 5),
    ("st-no-audio-track", (190, 182, 20, 20), 29),
    ("st-no-audio-track", (240, 184, 16, 16), 37),
    ("st-no-audio-track", (292, 183, 8, 18), 42),
    ("st-no-audio-track", (302, 214, 4, 8), 5),
    ("st-no-audio-track", (46, 184, 72, 16), 173),
    ("st-paused-hover-play", (14, 184, 18, 16), 21),
    ("st-paused-hover-play", (14, 214, 4, 8), 6),
    ("st-paused-hover-play", (190, 182, 20, 20), 66),
    ("st-paused-hover-play", (240, 184, 16, 16), 38),
    ("st-paused-hover-play", (292, 183, 8, 18), 42),
    ("st-paused-hover-play", (302, 214, 4, 8), 5),
    ("st-paused-hover-play", (46, 184, 72, 16), 158),
    ("st-paused-hover-timeline", (14, 184, 18, 16), 20),
    ("st-paused-hover-timeline", (14, 210, 16, 16), 22),
    ("st-paused-hover-timeline", (14, 214, 4, 8), 8),
    ("st-paused-hover-timeline", (190, 182, 20, 20), 66),
    ("st-paused-hover-timeline", (240, 184, 16, 16), 38),
    ("st-paused-hover-timeline", (292, 183, 8, 18), 42),
    ("st-paused-hover-timeline", (302, 214, 4, 8), 5),
    ("st-paused-hover-timeline", (46, 184, 72, 16), 158),
    ("st-paused-initial", (130, 70, 70, 75), 292),
    ("st-paused-initial", (14, 184, 18, 16), 20),
    ("st-paused-initial", (14, 214, 4, 8), 6),
    ("st-paused-initial", (190, 182, 20, 20), 66),
    ("st-paused-initial", (240, 184, 16, 16), 38),
    ("st-paused-initial", (292, 183, 8, 18), 42),
    ("st-paused-initial", (302, 214, 4, 8), 5),
    ("st-paused-initial", (46, 184, 72, 16), 158),
    ("st-paused-seek-5s", (14, 184, 18, 16), 20),
    ("st-paused-seek-5s", (14, 214, 4, 8), 8),
    ("st-paused-seek-5s", (152, 210, 16, 16), 26),
    ("st-paused-seek-5s", (190, 182, 20, 20), 66),
    ("st-paused-seek-5s", (240, 184, 16, 16), 38),
    ("st-paused-seek-5s", (292, 183, 8, 18), 42),
    ("st-paused-seek-5s", (302, 214, 4, 8), 5),
    ("st-paused-seek-5s", (46, 184, 72, 16), 160),
    ("st-paused-seek-5s-away", (14, 184, 18, 16), 20),
    ("st-paused-seek-5s-away", (14, 214, 4, 8), 8),
    ("st-paused-seek-5s-away", (152, 210, 16, 16), 26),
    ("st-paused-seek-5s-away", (190, 182, 20, 20), 66),
    ("st-paused-seek-5s-away", (240, 184, 16, 16), 38),
    ("st-paused-seek-5s-away", (292, 183, 8, 18), 42),
    ("st-paused-seek-5s-away", (302, 214, 4, 8), 5),
    ("st-paused-seek-5s-away", (46, 184, 72, 16), 160),
    ("st-playing-hover-pause", (14, 184, 18, 16), 29),
    ("st-playing-hover-pause", (14, 214, 4, 8), 8),
    ("st-playing-hover-pause", (190, 182, 20, 20), 66),
    ("st-playing-hover-pause", (240, 184, 16, 16), 38),
    ("st-playing-hover-pause", (292, 183, 8, 18), 42),
    ("st-playing-hover-pause", (30, 214, 40, 8), 22),
    ("st-playing-hover-pause", (302, 214, 4, 8), 5),
    ("st-playing-hover-pause", (46, 184, 72, 16), 201),
    ("st-playing-shown", (14, 184, 18, 16), 29),
    ("st-playing-shown", (14, 214, 4, 8), 8),
    ("st-playing-shown", (190, 182, 20, 20), 66),
    ("st-playing-shown", (240, 184, 16, 16), 38),
    ("st-playing-shown", (292, 183, 8, 18), 42),
    ("st-playing-shown", (30, 214, 40, 8), 53),
    ("st-playing-shown", (302, 214, 4, 8), 5),
    ("st-playing-shown", (46, 184, 72, 16), 201),
    ("st-volume-half", (123, 189, 4, 6), 5),
    ("st-volume-half", (14, 184, 18, 16), 20),
    ("st-volume-half", (14, 214, 4, 8), 6),
    ("st-volume-half", (142, 184, 16, 16), 28),
    ("st-volume-half", (190, 182, 20, 20), 67),
    ("st-volume-half", (240, 184, 16, 16), 38),
    ("st-volume-half", (292, 183, 8, 18), 42),
    ("st-volume-half", (302, 214, 4, 8), 5),
    ("st-volume-half", (46, 184, 72, 16), 335),
    ("st-volume-open", (123, 189, 4, 6), 5),
    ("st-volume-open", (14, 184, 18, 16), 20),
    ("st-volume-open", (14, 214, 4, 8), 6),
    ("st-volume-open", (172, 189, 6, 6), 5),
    ("st-volume-open", (190, 182, 20, 20), 67),
    ("st-volume-open", (240, 184, 16, 16), 38),
    ("st-volume-open", (292, 183, 8, 18), 42),
    ("st-volume-open", (302, 214, 4, 8), 5),
    ("st-volume-open", (46, 184, 72, 16), 335),
    ("st-volume-zero", (122, 184, 16, 16), 26),
    ("st-volume-zero", (123, 189, 4, 6), 9),
    ("st-volume-zero", (14, 184, 18, 16), 20),
    ("st-volume-zero", (14, 214, 4, 8), 6),
    ("st-volume-zero", (190, 182, 20, 20), 90),
    ("st-volume-zero", (240, 184, 16, 16), 38),
    ("st-volume-zero", (292, 183, 8, 18), 42),
    ("st-volume-zero", (302, 214, 4, 8), 5),
    ("st-volume-zero", (46, 184, 72, 16), 335),
];

fn cap_of(name: &str, rect: Rect) -> usize {
    PIXEL_CAPS
        .iter()
        .find(|(n, r, _)| *n == name && *r == rect)
        .map_or(0, |(.., cap)| *cap)
}

fn probe_cases() -> Vec<&'static Value> {
    let text = std::fs::read_to_string(common::repo_root().join(STATES_PROBE)).unwrap();
    let file: &'static Value = Box::leak(Box::new(serde_json::from_str::<Value>(&text).unwrap()));
    file["cases"].as_array().unwrap().iter().collect()
}

fn expected_png(name: &str) -> Pixmap {
    let path = common::repo_root()
        .join("crates/engine/tests/data/media-controls")
        .join(format!("{name}-p.png"));
    Pixmap::decode_png(&std::fs::read(&path).unwrap()).unwrap()
}

/// swb's screenshot at the end of the steps of `case`.
fn swb_picture(case: &'static Value) -> Pixmap {
    let (mut page, _site) = load_case(STATES_PROBE, case);
    page.run_event_loop(
        Duration::from_millis(FIRST_QUERY_MS),
        Duration::from_secs(60),
    );
    page.note_user_activation();
    run_steps(&mut page, case);
    page.screenshot(true).unwrap()
}

/// The size of the element of a state case (a video 320 x 240, audio 300 x 54).
fn element_size(case: &Value) -> (i32, i32) {
    if case["html"].as_str().unwrap().contains("<audio") {
        (300, 54)
    } else {
        (320, 240)
    }
}

/// The pixels of `got` and `want` in `size` that differ by more than
/// `LEVELS` outside the departure rectangles (their places), and for each
/// departure rectangle the count of such pixels in it.
fn differing(
    name: &str,
    got: &Pixmap,
    want: &Pixmap,
    size: (i32, i32),
) -> (Vec<BadPixel>, Vec<(Rect, usize)>) {
    let kind: &[(Rect, &str)] = if size.0 == 300 {
        AUDIO_DEPARTURES
    } else {
        VIDEO_DEPARTURES
    };
    let mut bad = Vec::new();
    let mut hit: Vec<(Rect, usize)> = Vec::new();
    for y in 0..size.1 {
        for x in 0..size.0 {
            let (g, w) = (
                got.pixel(x as u32, y as u32).unwrap().demultiply(),
                want.pixel(x as u32, y as u32).unwrap().demultiply(),
            );
            let (g, w) = (
                [g.red(), g.green(), g.blue()],
                [w.red(), w.green(), w.blue()],
            );
            let worst = g
                .iter()
                .zip(&w)
                .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
                .max()
                .unwrap();
            if worst <= LEVELS {
                continue;
            }
            let inside =
                |(rx, ry, rw, rh): Rect| (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y);
            let covered = kind
                .iter()
                .map(|(r, _)| *r)
                .chain(
                    STATE_DEPARTURES
                        .iter()
                        .filter(|(c, ..)| *c == name)
                        .map(|(_, r, _)| *r),
                )
                .find(|r| inside(*r));
            match covered {
                Some(rect) => match hit.iter_mut().find(|(r, _)| *r == rect) {
                    Some((_, count)) => *count += 1,
                    None => hit.push((rect, 1)),
                },
                None => bad.push((x, y, g, w)),
            }
        }
    }
    (bad, hit)
}

#[test]
fn the_pictures_of_the_controls_match_chromium() {
    let dump = std::env::var_os("SWB_DUMP_DIR");
    let runs: Vec<_> = probe_cases()
        .into_iter()
        .map(|case| {
            move || {
                let name = case["name"].as_str().unwrap();
                (name, swb_picture(case), element_size(case))
            }
        })
        .collect();
    let mut failures = Vec::new();
    let mut used: Vec<(&str, Rect)> = Vec::new();
    for (name, got, size) in on_page_threads(runs) {
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).unwrap();
            got.save_png(std::path::Path::new(dir).join(format!("{name}.png")))
                .unwrap();
        }
        let want = expected_png(name);
        let (bad, hit) = differing(name, &got, &want, size);
        for (rect, count) in hit {
            used.push((name, rect));
            if std::env::var_os("SWB_PRINT_CAPS").is_some() {
                eprintln!("    (\"{name}\", {rect:?}, {count}),");
            }
            let cap = cap_of(name, rect);
            if count > cap {
                failures.push(format!(
                    "{name} {rect:?}: {count} pixels differ by more than {LEVELS} levels, \
                     the cap is {cap}"
                ));
            }
        }
        if !bad.is_empty() {
            let (x0, y0) = bad
                .iter()
                .fold((i32::MAX, i32::MAX), |(a, b), p| (a.min(p.0), b.min(p.1)));
            let (x1, y1) = bad
                .iter()
                .fold((0, 0), |(a, b), p| (a.max(p.0), b.max(p.1)));
            let samples: Vec<String> = bad
                .iter()
                .step_by((bad.len() / 6).max(1))
                .map(|(x, y, g, w)| format!("({x},{y}) swb {g:?} Chromium {w:?}"))
                .collect();
            failures.push(format!(
                "{name}: {} pixels differ by more than {LEVELS} levels in ({x0},{y0})-({x1},{y1}): {}",
                bad.len(),
                samples.join("; ")
            ));
        }
    }
    for (case, rect, reason) in STATE_DEPARTURES {
        if !used.iter().any(|(c, r)| c == case && r == rect) {
            failures.push(format!(
                "{case} {rect:?} ({reason}) no longer differs: remove it"
            ));
        }
    }
    for (audio, rects) in [(false, VIDEO_DEPARTURES), (true, AUDIO_DEPARTURES)] {
        for (rect, reason) in rects {
            if !used
                .iter()
                .any(|(c, r)| c.contains("audio") == audio && r == rect)
            {
                failures.push(format!(
                    "{rect:?} ({reason}) no longer differs in any {} state: remove it",
                    if audio { "audio" } else { "video" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
