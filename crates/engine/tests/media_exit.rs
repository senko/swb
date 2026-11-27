//! M14 feature 10c, the exit test (ADR 0047 "Implementation notes (M14
//! feature 10c)"): each of the eight works items of M14 against Chromium,
//! one module per item. A module starts with the map: the tests and probe
//! files of the earlier features that already check the item, and then the
//! tests that fill what the map found missing. Run it with `just
//! test-media` or `cargo test -p swb-engine --features media --test
//! media_exit`; the tests that need the real files (`fixtures/js/media/`,
//! `fixtures/js/videojs`, git-ignored) print `skipped: ...` and pass when a
//! file is missing. The module of item 7 runs in the build without the
//! feature (`just test`).
//!
//! The comparison rules are those of the playback tests
//! (`common/media_probe.rs`): Chromium plays in real time and swb in virtual
//! time, so times differ by up to 0.26 s (the probe rounds to 0.25 s); every
//! other value is equal. `DEPARTURES` and `EXPECTED` list what differs from
//! Chromium on purpose, with the backlog item of the roadmap ("Backlog from
//! M14"); a departure that no longer occurs fails the test. A departure of
//! `EXPECTED` states swb's own answer, so that no other difference of the
//! same row is hidden.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

/// A departure: the case, the path in the answer, the reason.
#[cfg(feature = "media")]
type Departure = (&'static str, &'static str, &'static str);

/// What the tests of the items share (the build with the feature).
#[cfg(feature = "media")]
mod shared {
    use std::collections::HashSet;
    use std::time::Duration;

    use serde_json::Value;
    use swb_engine::{NodeId, Page};
    use swb_web::MediaInfo;

    use super::Departure;
    use crate::common;
    use common::media_probe::{load_case_with, run_steps_with, same_text};

    /// Splits the differences of the runs into failures and the departures
    /// that occurred (each listed departure of a case in `probe` must occur).
    pub(crate) fn judge(
        probe: &str,
        runs: Vec<(String, Vec<(String, String)>)>,
        departures: &[Departure],
    ) -> Vec<String> {
        let mut failures = Vec::new();
        let mut seen: Vec<(String, String)> = Vec::new();
        for (name, differences) in runs {
            for (path, text) in differences {
                if departures.iter().any(|(c, p, _)| *c == name && *p == path) {
                    seen.push((name.clone(), path));
                } else {
                    failures.push(format!("{name} {path}: {text}"));
                }
            }
        }
        let ours = std::fs::read_to_string(common::repo_root().join(probe)).unwrap();
        for (case, path, _) in departures {
            let in_file = ours.contains(&format!("\"name\": \"{case}\""));
            if in_file && !seen.iter().any(|(c, p)| c == case && p == path) {
                failures.push(format!("{case} {path}: the departure no longer occurs"));
            }
        }
        failures
    }

    /// `Math.floor(x * 2) / 2` of the probe's samples: Chromium plays in real
    /// time, so a time is only known to half a second (the sample times of
    /// the steps lie in the middle of a bucket).
    fn half(x: f64) -> f64 {
        (x * 2.0).floor() / 2.0
    }

    /// The text of a range list as the probe's `rg()` writes it.
    fn ranges(list: &[(f64, f64)]) -> String {
        list.iter()
            .map(|(a, b)| format!("{}-{}", half(*a), half(*b)))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The probe's `ST()` of the element's state.
    pub(crate) fn state_text(info: &MediaInfo) -> String {
        format!(
            "p={} e={} ct={} m={} rs={} ns={} pl={} sk={} err={}",
            u8::from(info.paused),
            u8::from(info.ended),
            half(info.current_time),
            u8::from(info.muted),
            info.ready_state,
            info.network_state,
            ranges(&info.played),
            u8::from(info.seeking),
            info.error.as_ref().map_or(0, |e| e.code),
        )
    }

    /// The name of an `SMP('name')` step of a probe case.
    fn sample_name(js: &str) -> Option<&str> {
        js.strip_prefix("SMP('")?.strip_suffix("')")
    }

    /// The steps of `case` on `page` with JavaScript off (or on): the points
    /// move by `origin`, and each `SMP` step reads the state of `element`
    /// through the engine. Returns the samples as the probe's `SS` row has
    /// them.
    pub(crate) fn run_sampling(
        page: &mut Page,
        case: &Value,
        element: NodeId,
        origin: (f32, f32),
    ) -> Vec<String> {
        let mut samples: Vec<String> = Vec::new();
        run_steps_with(page, case, origin, &mut |page: &mut Page, js: &str| {
            let name = sample_name(js).unwrap_or_else(|| panic!("an unknown step: {js}"));
            let info = page.media_info(element).unwrap();
            samples.push(format!("{name} {}", state_text(&info)));
        });
        samples
    }

    /// The differences of swb's samples from the probe's `SS` row, the
    /// duration and the natural size from the row after it.
    pub(crate) fn sample_differences(
        samples: &[String],
        info: &MediaInfo,
        case: &Value,
        answer: &Value,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let index = case["measure"]
            .as_array()
            .unwrap()
            .iter()
            .position(|m| m["js"] == "SS")
            .unwrap();
        let want: Vec<&str> = answer["results"][index]["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        if samples.len() != want.len() {
            out.push((
                "#SS".to_owned(),
                format!("swb {} samples, Chromium {}", samples.len(), want.len()),
            ));
        }
        for (i, (got, want)) in samples.iter().zip(&want).enumerate() {
            if !same_text(got, want) {
                out.push((
                    format!("#SS[{i}]"),
                    format!("swb {got:?}, Chromium {want:?}"),
                ));
            }
        }
        let dims = &answer["results"][index + 1]["value"];
        let duration = dims[0].as_f64().unwrap();
        if (info.duration - duration).abs() > 0.0005 {
            out.push((
                "#dur".to_owned(),
                format!("swb {}, Chromium {duration}", info.duration),
            ));
        }
        let size = |v: &Value| v.as_u64().unwrap_or(0) as u32;
        if (info.video_width, info.video_height) != (size(&dims[1]), size(&dims[2])) {
            out.push((
                "#size".to_owned(),
                format!(
                    "swb {}x{}, Chromium {}x{}",
                    info.video_width,
                    info.video_height,
                    size(&dims[1]),
                    size(&dims[2])
                ),
            ));
        }
        out
    }

    /// A case with JavaScript off: its markup without the scripts that
    /// observe it, the steps, and the samples read through the engine,
    /// against the probe's `SS` row.
    pub(crate) fn run_without_scripts(
        probe: &str,
        case: &'static Value,
        answer: &'static Value,
    ) -> Vec<(String, String)> {
        let (mut page, _site) = load_case_with(probe, case, false);
        page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
        let element = common::node(&page, "v");
        let samples = run_sampling(&mut page, case, element, (0.0, 0.0));
        let info = page.media_info(element).unwrap();
        sample_differences(&samples, &info, case, answer)
    }

    /// The cases of `probe` that the data file `data` (the text of a probe
    /// output) has no answer for and `skipped` does not name, and the skipped
    /// ones that have an answer. Every case of a probe file is run by some
    /// test or is listed with the reason.
    pub(crate) fn unrun_cases(
        probe: &str,
        data: &'static str,
        skipped: &[(&str, &str)],
    ) -> Vec<String> {
        let text = std::fs::read_to_string(common::repo_root().join(probe)).unwrap();
        let probe_file: Value = serde_json::from_str(&text).unwrap();
        let answered: HashSet<&str> = {
            let parsed: &'static Value = Box::leak(Box::new(
                serde_json::from_str(&common::media_site::chromium_json(data)).unwrap(),
            ));
            parsed["cases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap())
                .collect()
        };
        let mut failures = Vec::new();
        for case in probe_file["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let listed = skipped.iter().any(|(n, _)| *n == name);
            if !answered.contains(name) && !listed {
                failures.push(format!(
                    "{probe}: the case {name} has no answer and no reason"
                ));
            }
            if answered.contains(name) && listed {
                failures.push(format!("{probe}: the case {name} has an answer: unlist it"));
            }
        }
        for (name, _) in skipped {
            if !text.contains(&format!("\"name\": \"{name}\"")) {
                failures.push(format!(
                    "{probe}: the listed case {name} is not in the file"
                ));
            }
        }
        failures
    }
}

// ----- Item 1: the formats, with and without JavaScript, with controls -----

#[cfg(feature = "media")]
mod item1_formats {
    //! Works item 1: `<video>` and `<audio>` play the test media of each
    //! format, with JavaScript on and off, and with `controls` the default
    //! controls work by mouse.
    //!
    //! The map (before this file): `media_states.rs` (`format-metadata-table`,
    //! `format-play-to-end-table`: 25 and 23 files in `<video>` and `<audio>`,
    //! JavaScript on, the end state of a script `play()`), `media_playback.rs`
    //! (`play-to-end`, `play-to-end-audio-mp3`), `media_controls.rs` (the 29
    //! cases of `media-controls-input.json` that need no local file: click on
    //! the play button and the picture, seek by click and drag, mute, the
    //! volume slider, replay, in `<video>` with MP4 and in `<audio>` with MP3,
    //! JavaScript on, events and state against Chromium; and the 22 pictures
    //! of `media-controls-states.json`: the time display, the played part
    //! and the thumb of a playing video), `media_controls_timers.rs`
    //! (`the_actions_work_with_javascript_off`: MP4 only), `swb-media`
    //! `tests/files.rs` and `audio.rs` (every file demuxed and decoded: the
    //! frames and the sines).
    //!
    //! The gaps, filled here (`tools/probes/media-exit.json`): every format
    //! of the build played from the controls (JavaScript on: the events and
    //! the state at sampled points; JavaScript off: the state at the same
    //! points) through play, play to the end, the click on the timeline
    //! after the end, play and pause, mute and unmute, a drag of the
    //! timeline, and play to the end again; `<video>` with audio-only files
    //! (to the first end) and `<audio>` with files that have video; clicks
    //! on the panel of an `<audio>` outside its controls; where the
    //! timeline of an `<audio>` takes a click (`hit-audio-timeline-*`).
    //!
    //! What the module found and what it leaves as departures:
    //! - Fixed in 10c: the frames of a full range H.264 stream after the
    //!   first converted with BT.601 coefficients (`media-ffi` `ffmpeg.c`;
    //!   found by `media_frames.rs`, not by this module); the press on an
    //!   `<audio>` panel outside its parts played or paused (Chromium: no
    //!   event, no action); the timeline of an `<audio>` took a click only on
    //!   its bar and 6 px around (Chromium: a band of 32 px from the time
    //!   label to the mute button).
    //! - Departures: Chromium's duration of the Ogg Vorbis and Ogg FLAC
    //!   files grows after a seek near the end (`OGG_DURATION`); `ended`
    //!   after a seek to the duration of an audio file (`ENDED_AFTER_SEEK`);
    //!   a `<video>` with an audio-only file draws the audio-style bar at
    //!   the bottom of the box in Chromium (a pill of the box's width, 54 px
    //!   high, no black fill) and swb draws the video controls, so the
    //!   `video-tag-*` cases run to the first end only, through the play
    //!   button that both layouts have at the same place (backlog of 10c).

    use serde_json::Value;

    use super::shared::{judge, run_without_scripts, unrun_cases};
    use crate::common::media_probe::{answers, run_case_expecting};
    use crate::common::on_page_threads;
    use crate::common::probe_rows::load_cases;

    const ANSWERS: &str = include_str!("data/media-exit.json");
    const PROBE: &str = "tools/probes/media-exit.json";

    const OGG_DURATION: &str = "Chromium's duration of the Ogg Vorbis and Ogg FLAC test files \
        (2 s: `ffprobe` and swb say 2.0) grows to 2.567 and 2.508 after a seek to 1.59 s, with \
        about 24 `durationchange` events and a `buffered` end that grows with it, so that the \
        playback to the end ends at the new duration (backlog of feature 10c: a Chromium \
        estimate of the Ogg demuxer; swb keeps 2.0)";
    const ENDED_AFTER_SEEK: &str = "after a click on the right end of an audio timeline (a seek \
        to the duration) Chromium's `ended` is true (HTML: the position is the end, the \
        direction is forward); swb's `ended` is set only by a playback that reaches the end \
        (backlog of feature 6b, which probed MP4 only, and of feature 10c)";

    /// Where swb answers differently from Chromium on purpose (JavaScript
    /// on): the case, the path (`#row` or `#row[index]`), what swb answers
    /// there (JSON), the reason. The test replaces Chromium's value at the
    /// path by swb's, so that every other difference of the row still fails,
    /// and a departure that no longer occurs fails too (swb's answer is then
    /// not the listed one). The event log of the Ogg cases (row 0) is
    /// Chromium's without its `durationchange` events after the first.
    type Expected = (&'static str, &'static str, &'static str, &'static str);

    const EXPECTED: &[Expected] = &[
        ("fmt-ogg-vorbis", "#1[10]", OGG_SAMPLE, OGG_DURATION),
        ("fmt-ogg-vorbis", "#2[0]", "2.0", OGG_DURATION),
        ("fmt-ogg-vorbis", "#2[3]", "\"0-2\"", OGG_DURATION),
        ("fmt-ogg-vorbis", "#2[4]", "\"0-2\"", OGG_DURATION),
        ("fmt-ogg-flac", "#1[10]", OGG_SAMPLE, OGG_DURATION),
        ("fmt-ogg-flac", "#2[0]", "2.0", OGG_DURATION),
        ("fmt-ogg-flac", "#2[3]", "\"0-2\"", OGG_DURATION),
        ("fmt-ogg-flac", "#2[4]", "\"0-2\"", OGG_DURATION),
        (
            "fmt-audio-panel-click",
            "#1[3]",
            ENDED_SAMPLE_4,
            ENDED_AFTER_SEEK,
        ),
        (
            "fmt-audio-panel-click",
            "#1[4]",
            ENDED_SAMPLE_5,
            ENDED_AFTER_SEEK,
        ),
    ];

    /// Sample 11 of the Ogg cases: the second play to the end, at 2.0 s.
    const OGG_SAMPLE: &str = "\"11-ended-again p=1 e=1 ct=2 m=0 rs=4 ns=1 pl=0-2 sk=0 err=0\"";
    /// Samples 4 and 5 of the audio panel case: no `ended` (swb).
    const ENDED_SAMPLE_4: &str =
        "\"4-click-timeline-right-end p=1 e=0 ct=2 m=0 rs=4 ns=1 pl= sk=0 err=0\"";
    const ENDED_SAMPLE_5: &str =
        "\"5-drag-label-to-timeline p=1 e=0 ct=2 m=0 rs=4 ns=1 pl= sk=0 err=0\"";

    /// The same with JavaScript off (the paths are those of the samples:
    /// `#SS[index]`, and `#dur` for the duration).
    const EXPECTED_OFF: &[Expected] = &[
        ("fmt-ogg-vorbis", "#SS[10]", OGG_SAMPLE, OGG_DURATION),
        ("fmt-ogg-vorbis", "#dur", "2.0", OGG_DURATION),
        ("fmt-ogg-flac", "#SS[10]", OGG_SAMPLE, OGG_DURATION),
        ("fmt-ogg-flac", "#dur", "2.0", OGG_DURATION),
        (
            "fmt-audio-panel-click",
            "#SS[3]",
            ENDED_SAMPLE_4,
            ENDED_AFTER_SEEK,
        ),
        (
            "fmt-audio-panel-click",
            "#SS[4]",
            ENDED_SAMPLE_5,
            ENDED_AFTER_SEEK,
        ),
    ];

    /// The index in `path` after `[`, if any.
    fn index_of(path: &str) -> Option<usize> {
        path.split_once('[')?.1.strip_suffix(']')?.parse().ok()
    }

    /// Chromium's value of the row or entry named `head` (`#1`, `#SS`,
    /// `#dur`) of `case` with the listed departures replaced by swb's
    /// answer.
    fn expecting(expected: &[Expected], case: &str, head: &str, want: &Value) -> Value {
        let mut want = want.clone();
        for (c, path, json, _) in expected {
            if *c != case || path.split('[').next() != Some(head) {
                continue;
            }
            let swb: Value = serde_json::from_str(json).unwrap();
            match index_of(path) {
                Some(i) => want[i] = swb,
                None => want = swb,
            }
        }
        want
    }

    /// Chromium's event log of an Ogg case without the `durationchange`
    /// events after the first (its duration estimate grows: `OGG_DURATION`).
    fn without_extra_durationchanges(log: &Value) -> Value {
        let mut seen = false;
        let kept: Vec<Value> = log
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                let is = e.as_str().is_some_and(|s| s.starts_with("durationchange"));
                let keep = !is || !seen;
                seen |= is;
                keep
            })
            .cloned()
            .collect();
        Value::Array(kept)
    }

    /// Chromium's answer of row `row` of `case` as swb is expected to give
    /// it (JavaScript on).
    fn expected_row(case: &str, row: usize, want: &Value) -> Value {
        let mut want = want.clone();
        if row == 0 && case.starts_with("fmt-ogg-") {
            want = without_extra_durationchanges(&want);
        }
        expecting(EXPECTED, case, &format!("#{row}"), &want)
    }

    /// The answer of `case` as swb is expected to give it with JavaScript
    /// off (the departures of `EXPECTED_OFF` in the `SS` row and the
    /// duration).
    fn expected_off(case: &Value, answer: &Value) -> &'static Value {
        let name = case["name"].as_str().unwrap();
        let ss = case["measure"]
            .as_array()
            .unwrap()
            .iter()
            .position(|m| m["js"] == "SS")
            .unwrap();
        let mut answer = answer.clone();
        let samples = answer["results"][ss]["value"].clone();
        answer["results"][ss]["value"] = expecting(EXPECTED_OFF, name, "#SS", &samples);
        let dims = answer["results"][ss + 1]["value"].clone();
        let dur = expecting(EXPECTED_OFF, name, "#dur", &dims[0]);
        answer["results"][ss + 1]["value"][0] = dur;
        Box::leak(Box::new(answer))
    }

    fn failures(javascript: bool) -> Vec<String> {
        let cases = load_cases(PROBE, answers(ANSWERS));
        let runs: Vec<_> = cases
            .into_iter()
            .map(|(case, answer)| {
                move || {
                    let name = case["name"].as_str().unwrap().to_owned();
                    // The cases that sample no state (the hit areas of the
                    // audio timeline: they read the page's own log) have
                    // no JavaScript-off form.
                    let samples = case["measure"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|m| m["js"] == "SS");
                    let differences = if javascript {
                        run_case_expecting(PROBE, case, answer, &|row, want| {
                            expected_row(&name, row, want)
                        })
                    } else if samples {
                        run_without_scripts(PROBE, case, expected_off(case, answer))
                    } else {
                        Vec::new()
                    };
                    (name, differences)
                }
            })
            .collect();
        // The departures are in the expected values: no path is excused.
        judge(PROBE, on_page_threads(runs), &[])
    }

    #[test]
    fn every_format_plays_from_its_controls_with_javascript_on() {
        let failures = failures(true);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_format_plays_from_its_controls_with_javascript_off() {
        let failures = failures(false);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The probe cases of the controls that no test runs are listed with the
    /// reason (`in-nodur` needs a local file, `in-click-fullscreen-and-menu`
    /// changes the page in Chromium).
    #[test]
    fn every_probe_case_of_the_controls_has_an_answer_or_a_reason() {
        let mut failures = unrun_cases(
            "tools/probes/media-controls-input.json",
            include_str!("data/media-controls-input.json"),
            &[
                (
                    "in-nodur",
                    "needs `out/media-local/nodur.webm` (a stream without a duration)",
                ),
                (
                    "in-click-fullscreen-and-menu",
                    "the fullscreen button and the menu are not in M14 (\"Not in M14\")",
                ),
            ],
        );
        failures.extend(unrun_cases(PROBE, ANSWERS, &[]));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

// ----- Item 2: scripts -----

#[cfg(feature = "media")]
mod item2_scripts {
    //! Works item 2: `play()` and its promise, `pause()`, seeking with
    //! `currentTime`, `volume`, `muted`, `loop`, `autoplay` and `preload`,
    //! `new Audio()`, `canPlayType`, the ready and network states,
    //! `duration`, `buffered`, `seekable`, `played`, `videoWidth` and
    //! `videoHeight`, `MediaError`; the events in Chromium's order with its
    //! values at sampled points. The map decides: no gap, the tests of the
    //! earlier features cover each part.
    //!
    //! - `media_playback.rs`: `media-playback.json` (30 cases: `play-pause`,
    //!   `play-to-end`, `loop`, `seek-*`, `volume-muted`, `new-audio`,
    //!   `promise-*`, `played-ranges`, `timeupdate-rate`, removal) and
    //!   `media-playback-2.json` (13: play after a seek to the end, a
    //!   `pause()` during a seek, the late failures at 30, 60, 80 and 90 %,
    //!   the pending promises of a bad `src`).
    //! - `media_autoplay.rs`: `media-autoplay.json` (27 cases: the policy for
    //!   `play()` and `autoplay`, muted or not, `new Audio()`, the user
    //!   activation).
    //! - `media_states.rs`: `media-states.json` (65 cases: the events and
    //!   states of the load for each `preload` value, `error` with its
    //!   codes and messages, `<source>` selection, `currentSrc`, the natural
    //!   size, the metadata and play-to-end tables of every format).
    //! - `media_fetch.rs`: `media-fetch.json` (45 cases: the requests and the
    //!   states per `preload` value, servers without `Range`).
    //! - `media_api_edges.rs`: `media-api-edges.json` (about 100 rows: the
    //!   setters with the values that throw, their messages, `ratechange`).
    //! - `media_can_play_type.rs`: 149 `canPlayType` rows, the interfaces
    //!   `MediaError`, `TimeRanges` and the constants.
    //! - `media_playback_api.rs`, `media_element.rs`: JavaScript off,
    //!   removal, floods, 1,000 elements, 36 pages under load, the events of
    //!   a playback the same on every run under load.
    //! - `evaluation_activation.rs`, `focus_activation.rs`: the user
    //!   activation of evaluations and steps.
    //! - Departures that these files list: the second `suspend` of the
    //!   WebM file (backlog "Chromium fires a second `suspend`"), `progress`
    //!   and `suspend` of the metadata unit (backlog "`progress` and `suspend`
    //!   of the metadata unit"), the start of the M4A `buffered`, the MP4
    //!   with a start time at `loadedmetadata`, the HLS error message.
    //!
    //! What this module adds: a check that no probe case of these files is
    //! left without Chromium's answer in the data that the tests above
    //! compare with (a case added to a probe file must get its answer). The
    //! check proves that the data holds an answer, not that a test runs each
    //! case: `media_states.rs` skips `size-video-no-metadata` (a `boxes` row
    //! only, so "65 cases" is 64 run plus that one), and no test of the
    //! script API runs its row; the box of a video without metadata is a
    //! layout matter of feature 7, whose own tests do not run this row
    //! either (backlog).

    use super::shared::unrun_cases;

    #[test]
    fn every_probe_case_of_the_script_api_has_an_answer_or_a_reason() {
        let mut failures = Vec::new();
        for (probe, data, skipped) in [
            (
                "tools/probes/media-states.json",
                include_str!("data/media-states.json"),
                &[][..],
            ),
            (
                "tools/probes/media-playback.json",
                include_str!("data/media-playback.json"),
                &[][..],
            ),
            (
                "tools/probes/media-playback-2.json",
                include_str!("data/media-playback-2.json"),
                &[][..],
            ),
            (
                "tools/probes/media-autoplay.json",
                include_str!("data/media-autoplay.json"),
                &[][..],
            ),
            (
                "tools/probes/media-api-edges.json",
                include_str!("data/media-api-edges.json"),
                &[][..],
            ),
            (
                "tools/probes/media-can-play-type.json",
                include_str!("data/media-can-play-type.json"),
                &[][..],
            ),
            (
                "tools/probes/media-fetch.json",
                include_str!("data/media-fetch.json"),
                &[][..],
            ),
        ] {
            failures.extend(unrun_cases(probe, data, skipped));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

// ----- Item 3: video frames and the poster -----

#[cfg(feature = "media")]
mod item3_frames {
    //! Works item 3: a video shows its poster and its frames by Chromium's
    //! rules; frames at paused times equal Chromium's within a tolerance,
    //! also with `object-fit`, rounded clips and transforms; the natural size
    //! comes from the stream.
    //!
    //! The map:
    //! - `media_frames.rs`: `the_placement_of_a_video_frame_matches_chromium`
    //!   (the 42 cases of `media-frames.json` that `tests/data/media-frames.json`
    //!   holds: `object-fit`, `object-position`, padding and border, rounded
    //!   clips, `clip-path`, rotate, scale, skew, flip, `opacity`, `filter`,
    //!   overlap, scales 0.37 to 3.5, a dense row of 560 points, natural
    //!   sizes from the stream, SAR 2:1 and 1:2, `display: none`; 3 levels,
    //!   two listed departure points), and
    //!   `the_poster_rules_match_chromium` (24 cases of `media-poster.json`).
    //! - `swb-media` `tests/frames.rs`: the conversion at the decoder for
    //!   1,625 points (BT.601, BT.709, full range, the height rule, the
    //!   Wikimedia file, odd widths).
    //! - `media_video_paint.rs`: the frame in paint (damage, the painted
    //!   flag, the frame at the media time of the page clock, the same on
    //!   every run).
    //!
    //! The gap, filled in `media_frames.rs` (it holds the page runner): the
    //! 24 cases of the decoder-level data (`crates/media/tests/data/
    //! media-frames.json`: the frames of H.264 High and Baseline, VP8 and
    //! VP9 at paused times, the color rules of the tagged, untagged, full
    //! range and odd-width files) now also run through the element, the
    //! layout and paint (`the_frames_of_every_codec_and_color_rule_match_
    //! chromium_through_the_element`). The real Wikimedia file's frame is in
    //! the module of item 8.

    use super::shared::unrun_cases;

    /// The probe cases of `media-frames.json` that the engine has no
    /// answer for are the decoder-level ones (their data is `swb-media`'s,
    /// which `media_frames.rs` runs through the element); two have no data
    /// at all.
    #[test]
    fn every_probe_case_of_the_frames_has_an_answer_or_a_reason() {
        let decoder_level = "the data is `swb-media`'s (`tests/data/media-frames.json`); \
            run through the element by `media_frames.rs`";
        let no_data = "no stored answer: the same file as another case, whose pixels \
            (a flat color rule) the other cases of the file decide (see the notes of 10c)";
        let mut skipped: Vec<(&str, &str)> = Vec::new();
        let decoder_cases = [
            "frames-times-mp4-high",
            "frames-times-mp4-baseline",
            "frames-times-webm-vp8",
            "frames-times-webm-vp9",
            "frames-color-default-untagged",
            "frames-color-bt709-tagged",
            "frames-color-bt601-tagged",
            "frames-color-fullrange-bt709",
            "frames-color-untagged-bt709-640x480",
            "frames-color-untagged-bt709-720x576",
            "frames-color-untagged-bt709-1280x720",
            "frames-color-untagged-bt709-1280x544",
            "frames-color-untagged-bt709-720x720",
            "frames-color-untagged-bt709-960x720",
            "frames-color-untagged-bt709-1024x576",
            "frames-color-untagged-bt709-960x540",
            "frames-color-untagged-bt709-1280x480",
            "frames-color-untagged-bt709-1280x600",
            "frames-color-untagged-bt709-1024x640",
            "frames-color-untagged-bt709-1280x704",
            "frames-color-untagged-bt709-1280x718",
            "frames-color-untagged-bt709-1920x1080",
            "frames-color-odd-columns-854x480",
            "frames-color-odd-columns-322x242",
        ];
        skipped.extend(decoder_cases.iter().map(|c| (*c, decoder_level)));
        skipped.push(("frames-color-untagged-bt709-content", no_data));
        skipped.push(("frames-color-vp9-webm-range-tv", no_data));
        let mut failures = unrun_cases(
            "tools/probes/media-frames.json",
            include_str!("data/media-frames.json"),
            &skipped,
        );
        failures.extend(unrun_cases(
            "tools/probes/media-poster.json",
            include_str!("data/media-poster.json"),
            &[],
        ));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

// ----- Items 4 and 5: the GUI (measured, not run here) -----

mod items4_and_5_gui {
    //! Works item 4 (audio through PipeWire, in sync with the video: the
    //! frame on screen at most one frame period from the audio clock, also
    //! after seeks and over a long playback) and item 5 (budgets in the GUI:
    //! a 1080p 30 fps H.264 video without dropped frames; CPU and memory
    //! within the diagnosis' budgets; idle CPU 0 after `pause` or `ended`)
    //! are measurements in a window. This test runs no GUI. The evidence:
    //!
    //! - `swbtools media-gui` (`tools/swbtools/media_gui.py`, ADR 0047 "M14
    //!   feature 10b", `docs/testing.md` "Media in the GUI"): `--suite
    //!   budgets --check` measures the 21 cases against `BUDGETS` (1080p and
    //!   Wikimedia: CPU, memory above the idle page, 0 dropped, the frame
    //!   rate; the 4K file: peak memory and no crash; paused, ended, hidden,
    //!   `display: none`: 0.2 % CPU; the M4A; `loadedmetadata`, first frame
    //!   and seek times; the controls' cost against no controls);
    //!   `--suite sync --audio pw-private --check` runs the 180 s case with
    //!   ten seeks on a private PipeWire daemon (`media - video` at most one
    //!   frame period: `sync_long.max_ms`, `sync_seeks.max_ms`). The test
    //!   `test_the_budgets_cover_the_budgets_of_the_works_items` in
    //!   `tools/tests/test_media_gui.py` fails when a budget of item 4, item
    //!   5 or `docs/media-diagnosis.md` section 6 has no entry in `BUDGETS`
    //!   or a limit above the number of the works item (feature 10b set
    //!   tighter limits).
    //! - The frame loop without a window: `media_frame_loop.rs` (the frame
    //!   decision, no wake-ups when paused, ended or stalled, the painted
    //!   rules) and `frame_oracle.rs`, `frame_outdated.rs`.
    //! - The audio output without a sound card: `media_audio_output.rs`
    //!   (a fake device: the join and leave of the mixer, the audio clock,
    //!   volume, the fallbacks), `swb-media` `src/output/`, `mix_realtime.rs`
    //!   (the real-time callback without locks or allocations),
    //!   `output_pipeline.rs`; the test of the C boundary against a private
    //!   daemon is `#[ignore]` (`swb-media-ffi` `tests/pipewire.rs`).
    //!
    //! Not checked by any test and needing the GUI or the owner: the sound
    //! itself on a sound card (the device delay on real hardware, backlog of
    //! feature 8), the wake-ups of a minimized window on Wayland (backlog of
    //! 7b).
}

// ----- Item 6: hostile media -----

mod item6_hostile {
    //! Works item 6: truncated, corrupt, very large, endless and many media
    //! do not crash or hang swb, and memory stays bounded. `just
    //! hostile-media` runs the 18 cases that play media (release build with
    //! media; the hostile-page set of `docs/testing.md`); the unit and
    //! integration tests of the bounds:
    //!
    //! - `swb-media`: `tests/files.rs` (every test file opens and decodes or
    //!   fails as expected: truncated, corrupt, no index, garbage, 17 and 100
    //!   streams, the frame size limits), `sources.rs` (sources that fail,
    //!   read short, panic, cannot seek), `pipeline.rs`
    //!   (`hostile_files_play_to_where_they_can_or_fail`,
    //!   `a_decode_failure_has_the_same_place_on_every_run`),
    //!   `frame_budget.rs` (2 GiB of decoded frames for the process, waiters,
    //!   growing frames), `pipeline_threads.rs` (16 places, the no-progress
    //!   limit, the watchdog).
    //! - `swb-net` `media/tests.rs`, `swb-media` `tests/net.rs`: `Range`
    //!   lies, endless bodies, the 64 MiB cache, the 512 MiB page budget.
    //! - The engine: `media_element.rs` (`the_hostile_media_give_their_error_
    //!   codes_at_load`, 1,000 elements through 16 places),
    //!   `media_playback_api.rs` (floods of `play()` and seeks, 1,000 playing
    //!   videos, late failures), `media_retained.rs` (the 256 MiB budget of
    //!   retained frames), `media_exit.rs` module `item1_formats` (the files
    //!   with controls).
    //! - Against Chromium (`tools/probes/media-hostile.json`, 18 cases): the
    //!   expected behaviour of each hostile file is recorded in the labels of
    //!   the hostile cases (`tools/swbtools/hostile_cases/media.py`); the
    //!   known differences are in the roadmap's "Backlog from M14" (frame
    //!   sizes above 8192 x 4352, more than 16 streams in MP4 and Ogg, the
    //!   stalled open, endless streams, `Content-Range` lies, a frame size
    //!   that changes in the stream).
    //!
    //! The exit run of `just hostile-media` is in the notes of 10c.
}

// ----- Item 7: without the feature -----

#[cfg(not(feature = "media"))]
mod item7_without_the_feature {
    //! Works item 7: without the `media` feature swb builds and passes its
    //! tests, and media elements behave like a Chromium build without
    //! codecs: `canPlayType` gives `""` and loading fails with
    //! `MEDIA_ERR_SRC_NOT_SUPPORTED`. This module runs in the default build
    //! (`just test`); `just check` builds and tests the whole workspace
    //! without the feature.
    //!
    //! The map: `media_without_feature.rs` (a `src` fails with code 4 and no
    //! request, a `<source type>` is skipped with `error`, a source without
    //! type fails, JavaScript off, `play()` rejects, `autoplay` does nothing,
    //! `load()` fails again, stale markers do not stall the queue),
    //! `media_can_play_type.rs` (every one of the 149 rows answers `""`
    //! without the feature), `media_off.rs` (`Page::set_media(false)`),
    //! `media_element.rs` and `media_states.rs` are media-only.
    //! No Chromium without codecs exists to measure; the rows that decide are
    //! Chromium's own for a resource that fails to load.
    //!
    //! The gap, filled here: the same element with `controls` and a file that
    //! the build cannot play behaves as Chromium's element whose load failed
    //! (probe case `in-error` of `media-controls-input.json`: the events,
    //! then a click on the play button, on the picture, on the timeline and
    //! on the mute button do nothing, `paused` stays true, the error code is
    //! 4), and nothing is requested.

    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::common::media_probe::{
        FIRST_QUERY_MS, answers, evaluate, fuzzy_differences, load_case, run_steps,
    };
    use crate::common::on_page_thread;
    use crate::common::probe_rows::load_cases;

    const INPUT: &str = include_str!("data/media-controls-input.json");
    const PROBE: &str = "tools/probes/media-controls-input.json";

    #[test]
    fn controls_on_an_element_that_cannot_load_its_file_do_what_chromium_does_for_a_failed_load() {
        let (case, answer) = load_cases(PROBE, answers(INPUT))
            .into_iter()
            .find(|(c, _)| c["name"] == "in-error")
            .unwrap();
        // The element's `missing.mp4` is a real MP4 here: Chromium without
        // codecs fails it as it fails a missing file.
        let mut patched = case.clone();
        patched["files"]["missing.mp4"] = json!("fixtures/media/mp4-h264-high-aac.mp4");
        let patched: &'static Value = Box::leak(Box::new(patched));
        let (differences, requests) = on_page_thread(move || {
            let (mut page, site) = load_case(PROBE, patched);
            page.run_event_loop(
                Duration::from_millis(FIRST_QUERY_MS),
                Duration::from_secs(60),
            );
            page.note_user_activation();
            run_steps(&mut page, patched);
            let mut out = Vec::new();
            for (index, query) in patched["measure"].as_array().unwrap().iter().enumerate() {
                let got = evaluate(&mut page, query["js"].as_str().unwrap());
                let want = &answer["results"][index]["value"];
                fuzzy_differences(&format!("#{index}"), &got, want, &mut out);
            }
            (out, site.log_lines())
        });
        assert!(differences.is_empty(), "{differences:?}");
        assert!(
            requests.iter().all(|r| !r.contains("missing.mp4")),
            "a request for the file: {requests:?}"
        );
    }
}

// ----- Item 8: the real files and the media API of videojs.org -----

#[cfg(feature = "media")]
mod item8_real_files {
    //! Works item 8: the Wikipedia video plays after a click on play (JavaScript
    //! off); Mux's `highest.mp4` and `audio.m4a` play; on videojs.org the media
    //! element API exists (the hero player waits for M15). The real files are
    //! local (`fixtures/js/media/`, `fixtures/js/videojs`, git-ignored): each
    //! test prints `skipped: ...` and passes when one is missing.
    //!
    //! The map (before this file): `media-real-files.json` (17 cases against
    //! Chromium: the requests and states of the three files for each
    //! `preload` value, the Wikipedia `<source>` list, the frame colors) is
    //! compared in part: `media_element.rs`
    //! (`a_missing_preload_reads_ahead_as_metadata_does`),
    //! `swb-media` `tests/frames.rs` (`the_wikimedia_file_converts_by_its_
    //! container_tag`: the frame at 16.02 s at 20 points), `tests/net.rs`
    //! (the 161 MB file through the server and the replay), `pipeline_bench.rs`
    //! (the three files play 5 s in real time without a dropped frame),
    //! `swbtools media-gui` (the budgets of the three files),
    //! `videojs_transitions.rs`, `media_off.rs` (the page with media off).
    //!
    //! The gaps, filled here: the three files played from their controls
    //! with JavaScript on and off against Chromium's samples (probe file
    //! `media-exit-real.json`, from the first click to the end); the real
    //! Wikipedia page (`fixtures/pages/wikipedia-web-browser`, JavaScript
    //! off, with the video served from the local file) played by a click on
    //! the play button of its own video box; the Wikimedia frame through the
    //! element; the members of `HTMLMediaElement`, `HTMLVideoElement`,
    //! `HTMLAudioElement`, `MediaError` and `TimeRanges` on the videojs.org
    //! page against Chromium's lists.

    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::Value;
    use swb_engine::{ClockMode, PageClock, Size, Url};
    use swb_net::{Fetcher, NetError, ReplayFetcher, Request, Response, StreamResponse};

    use super::Departure;
    use super::shared::{judge, run_sampling, run_without_scripts, sample_differences};
    use crate::common;
    use crate::common::media_probe::{answers, evaluate, run_case};
    use crate::common::media_site::MediaSite;
    use crate::common::on_page_thread;
    use crate::common::probe_rows::load_cases;

    const REAL_ANSWERS: &str = include_str!("data/media-exit-real.json");
    const REAL_PROBE: &str = "tools/probes/media-exit-real.json";

    /// The local real files that a test needs, or the reason to skip.
    fn missing(files: &[&str]) -> Option<String> {
        let gone: Vec<String> = files
            .iter()
            .filter(|f| {
                !common::repo_root()
                    .join("fixtures/js/media")
                    .join(f)
                    .exists()
            })
            .map(|f| (*f).to_owned())
            .collect();
        if gone.is_empty() {
            None
        } else {
            Some(format!(
                "skipped: fixtures/js/media/{} is not there (local fixture)",
                gone.join(", ")
            ))
        }
    }

    const ALL_REAL_FILES: &[&str] = &[
        "mux-highest.mp4",
        "mux-audio.m4a",
        "wikimedia-web-browser-demo.webm",
    ];

    /// Where swb differs from Chromium on the real files, JavaScript on.
    const DEPARTURES: &[Departure] = &[];

    fn real_failures(javascript: bool, departures: &[Departure]) -> Vec<String> {
        let cases = load_cases(REAL_PROBE, answers(REAL_ANSWERS));
        let runs: Vec<_> = cases
            .into_iter()
            .map(|(case, answer)| {
                move || {
                    let name = case["name"].as_str().unwrap().to_owned();
                    let differences = if javascript {
                        run_case(REAL_PROBE, case, answer)
                    } else {
                        run_without_scripts(REAL_PROBE, case, answer)
                    };
                    (name, differences)
                }
            })
            .collect();
        judge(REAL_PROBE, common::on_page_threads(runs), departures)
    }

    #[test]
    fn the_real_files_play_from_their_controls_with_javascript_on() {
        if let Some(why) = missing(ALL_REAL_FILES) {
            eprintln!("{why}");
            return;
        }
        let failures = real_failures(true, DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn the_real_files_play_from_their_controls_with_javascript_off() {
        if let Some(why) = missing(ALL_REAL_FILES) {
            eprintln!("{why}");
            return;
        }
        let failures = real_failures(false, &[]);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The fixture's page with the video's file served from the local copy:
    /// the replay store has no media (the capture cannot record `Range`
    /// answers, ADR 0047 section 5), so the request for the first `<source>`
    /// of the video goes to a `MediaSite` that serves the real file with
    /// `Range`.
    struct WithVideo {
        inner: Arc<ReplayFetcher>,
        video: Arc<MediaSite>,
    }

    impl WithVideo {
        fn is_video(request: &Request) -> bool {
            request.url.path().ends_with("/c/c1/Web_browser_demo.webm")
        }
    }

    impl Fetcher for WithVideo {
        fn fetch(&self, request: &Request) -> Result<Response, NetError> {
            if Self::is_video(request) {
                self.video.fetch(request)
            } else {
                self.inner.fetch(request)
            }
        }

        fn fetch_stream(&self, request: &Request) -> Result<StreamResponse, NetError> {
            if Self::is_video(request) {
                self.video.fetch_stream(request)
            } else {
                self.inner.fetch_stream(request)
            }
        }

        fn cookie_jar(&self) -> Option<&swb_net::CookieJar> {
            self.inner.cookie_jar()
        }
    }

    /// The Wikipedia article "Web browser" (the fixture with JavaScript off,
    /// as Chromium picks the first source of the video then), a click on the
    /// play button of the article's own video box, and then the steps of the
    /// probe case `real-wikipedia-video-click-play` (the same markup, in its
    /// own page, at the top left): the samples of the state equal Chromium's.
    /// Chromium's reference is the reduced probe case (no `poster`, its own
    /// page), not a measurement of the real page.
    #[test]
    fn the_wikipedia_page_plays_its_video_after_a_click_on_play_with_javascript_off() {
        if let Some(why) = missing(&["wikimedia-web-browser-demo.webm"]) {
            eprintln!("{why}");
            return;
        }
        let dir = common::repo_root().join("fixtures/pages/wikipedia-web-browser");
        if !dir.join("fixture.json").exists() {
            eprintln!("skipped: no fixture in {}", dir.display());
            return;
        }
        let (case, answer) = load_cases(REAL_PROBE, answers(REAL_ANSWERS))
            .into_iter()
            .find(|(c, _)| c["name"] == "real-wikipedia-video-click-play")
            .unwrap();
        let (differences, requests_before, requests_after) = on_page_thread(move || {
            let meta: Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap())
                    .unwrap();
            let url = Url::parse(meta["url"].as_str().unwrap()).unwrap();
            let mut video = MediaSite::new("wikipedia/commons/c/c1", "");
            video.add_file(
                "Web_browser_demo.webm",
                "fixtures/js/media/wikimedia-web-browser-demo.webm",
            );
            let video = Arc::new(video);
            let fetcher = Arc::new(WithVideo {
                inner: Arc::new(ReplayFetcher::load(&dir).unwrap()),
                video: Arc::clone(&video),
            });
            // A viewport as tall as the article, so that the video's box is
            // in it without a scroll.
            let mut page = common::new_page(fetcher, 4, Size::new(1280.0, 8000.0));
            page.set_javascript(false);
            page.set_animations(false);
            page.set_clock(PageClock::new(ClockMode::Virtual, None));
            page.navigate(url);
            assert!(page.wait_until_loaded(Duration::from_secs(60)));
            page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
            let element = common::node(&page, "mwQQ");
            let rect = common::rect(&mut page, "mwQQ");
            assert_eq!((rect.width.round(), rect.height.round()), (250.0, 159.0));
            let before = video.log_lines().len();
            let samples = run_sampling(&mut page, case, element, (rect.x, rect.y));
            let info = page.media_info(element).unwrap();
            (
                sample_differences(&samples, &info, case, answer),
                before,
                video.log_lines().len(),
            )
        });
        assert!(differences.is_empty(), "{differences:?}");
        assert_eq!(requests_before, 0, "preload=none: no request before play");
        assert!(requests_after > 0, "the click on play fetched the file");
    }

    /// The frame of the real Wikimedia file at 16.02 s, through the element
    /// at its natural size (2558 x 1626): the 20 points that
    /// `real-frames-color-wikimedia-vp8` measured in Chromium (data of
    /// `swb-media`'s `tests/frames.rs`), within 5 levels (the same limit as
    /// the decoder-level test: 3 for the conversion, and the points on the
    /// edge of a shape differ by 5).
    #[test]
    fn the_wikimedia_frame_through_the_element_matches_chromium() {
        const DATA: &str = include_str!("../../media/tests/data/media-frames-real.json");
        const PROBE: &str = "tools/probes/media-real-files.json";
        if let Some(why) = missing(&["wikimedia-web-browser-demo.webm"]) {
            eprintln!("{why}");
            return;
        }
        let (case, answer) = load_cases(PROBE, answers(DATA))
            .into_iter()
            .find(|(c, _)| c["name"] == "real-frames-color-wikimedia-vp8")
            .unwrap();
        let worst = on_page_thread(move || {
            let (mut page, _site) = common::media_probe::load_case(PROBE, case);
            // A viewport as large as the frame: the full-page screenshot is
            // as wide as the viewport (Chromium's covers the overflow).
            page.set_viewport(Size::new(2600.0, 1700.0), 1.0);
            page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
            page.note_user_activation();
            let seek = evaluate(&mut page, case["measure"][0]["js"].as_str().unwrap());
            assert_eq!(seek, answer["results"][0]["value"], "the seek row");
            let shot = page.screenshot(true).unwrap();
            let pixels = &answer["results"][1];
            let mut worst = 0_i32;
            for (point, want) in pixels["points"]
                .as_array()
                .unwrap()
                .iter()
                .zip(pixels["pixels"].as_array().unwrap())
            {
                let x = u32::try_from(point[0].as_u64().unwrap()).unwrap();
                let y = u32::try_from(point[1].as_u64().unwrap()).unwrap();
                let p = shot.pixel(x, y).unwrap().demultiply();
                let got = [p.red(), p.green(), p.blue()];
                for (g, w) in got.iter().zip(want.as_array().unwrap()) {
                    worst = worst.max((i32::from(*g) - w.as_i64().unwrap() as i32).abs());
                }
            }
            worst
        });
        assert!(worst <= 5, "worst difference {worst} levels");
    }

    /// The lists of own property names of the prototypes (rows 3 to 7 of
    /// the case `api-surface`) and the rows after them.
    const API_PROBE: &str = "tools/probes/media-can-play-type.json";
    const API_ANSWERS: &str = include_str!("data/media-can-play-type.json");

    const NOT_IN_M14: &str =
        "not in M14 (the roadmap's \"Not in M14\"); M15 checks whether the player needs it";
    const M15_TEXT: &str = "text tracks (M15)";
    const M15_MSE: &str = "Media Source Extensions (M15)";
    const M15_FULLSCREEN: &str = "fullscreen (M15)";
    const PIP: &str = "picture-in-picture is reported as unavailable (\"Not in M14\")";
    const EME: &str = "Encrypted Media Extensions: not planned";
    const BACKLOG_QUALITY: &str = "backlog of feature 7b: `getVideoPlaybackQuality()` is not in the IDL (the GUI's frame log has the counters); M15 checks whether video.js needs it";

    /// What M14 leaves out: the members and interfaces of the Chromium rows
    /// that swb does not have, with the reason (the roadmap's "Not in M14"
    /// and backlog). A member that swb has but Chromium does not, or whose
    /// value differs, is no departure: it fails. A listed name that swb has
    /// now fails too.
    const ABSENT: &[(&str, &str)] = &[
        // Media Source Extensions, workers, text tracks, fullscreen: M15.
        ("MediaSource", M15_MSE),
        ("SourceBuffer", M15_MSE),
        ("TextTrack", M15_TEXT),
        ("VTTCue", M15_TEXT),
        ("textTracks", M15_TEXT),
        ("addTextTrack", M15_TEXT),
        ("requestFullscreen", M15_FULLSCREEN),
        ("webkitRequestFullscreen", M15_FULLSCREEN),
        // Web Audio, `captureStream()`, `requestVideoFrameCallback()`: M15
        // checks whether the player needs them.
        ("AudioContext", NOT_IN_M14),
        ("MediaStream", NOT_IN_M14),
        ("captureStream", NOT_IN_M14),
        ("srcObject", NOT_IN_M14),
        ("requestVideoFrameCallback", NOT_IN_M14),
        ("cancelVideoFrameCallback", NOT_IN_M14),
        // Picture-in-picture.
        ("requestPictureInPicture", PIP),
        ("disablePictureInPicture", PIP),
        ("onenterpictureinpicture", PIP),
        ("onleavepictureinpicture", PIP),
        // Playback quality counters.
        ("VideoPlaybackQuality", BACKLOG_QUALITY),
        ("getVideoPlaybackQuality", BACKLOG_QUALITY),
        ("webkitDecodedFrameCount", BACKLOG_QUALITY),
        ("webkitDroppedFrameCount", BACKLOG_QUALITY),
        ("webkitAudioDecodedByteCount", BACKLOG_QUALITY),
        ("webkitVideoDecodedByteCount", BACKLOG_QUALITY),
        // Encrypted media, remote playback, capabilities, the output device,
        // other `navigator` members: not part of the media element's
        // playback; M15 checks what the player needs.
        ("MediaKeys", EME),
        ("mediaKeys", EME),
        ("setMediaKeys", EME),
        ("onencrypted", EME),
        ("onwaitingforkey", EME),
        ("requestMediaKeySystemAccess", EME),
        ("RemotePlayback", NOT_IN_M14),
        ("remote", NOT_IN_M14),
        ("disableRemotePlayback", NOT_IN_M14),
        ("MediaCapabilities", NOT_IN_M14),
        ("mediaCapabilities", NOT_IN_M14),
        ("MediaSession", NOT_IN_M14),
        ("mediaSession", NOT_IN_M14),
        ("mediaDevices", NOT_IN_M14),
        ("sinkId", NOT_IN_M14),
        ("setSinkId", NOT_IN_M14),
        // Backlog of feature 6b: `playbackRate` is a stored value and
        // `preservesPitch` is missing.
        (
            "preservesPitch",
            "backlog of feature 6b (the pitch is kept at rate 1 only)",
        ),
        (
            "controlsList",
            "backlog of feature 10c: the default controls have no overflow menu (\"Not in M14\")",
        ),
        (
            "loading",
            "backlog of feature 10c: lazy loading of media (no probe of a page that needs it)",
        ),
    ];

    /// The names of the entries of row 9 (the defaults of a created element).
    const ROW9: &[&str] = &[
        "preload",
        "a.preload",
        "Audio.preload",
        "volume",
        "muted",
        "defaultMuted",
        "paused",
        "ended",
        "autoplay",
        "loop",
        "controls",
        "currentTime",
        "duration",
        "readyState",
        "networkState",
        "currentSrc",
        "src",
        "error",
        "playbackRate",
        "defaultPlaybackRate",
        "buffered",
        "seekable",
        "played",
        "seeking",
        "videoWidth",
        "videoHeight",
        "poster",
        "crossOrigin",
        "preservesPitch",
        "disablePictureInPicture",
        "width",
        "height",
        "playsInline",
    ];

    /// The differences of one row of `api-surface`: (the member, what). A
    /// list of names (rows 3 to 7) gives the names that swb lacks (`missing`)
    /// or has in addition (`extra`); the rows of `typeof` strings (0 to 2)
    /// and the defaults (9) give the entries that swb lacks (`missing`:
    /// `undefined` where Chromium has a value) or that differ (`differs`).
    fn row_differences(row: usize, got: &Value, want: &Value) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if (3..=7).contains(&row) {
            let names = |v: &Value| -> BTreeSet<String> {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_str().unwrap().to_owned())
                    .collect()
            };
            let (got, want) = (names(got), names(want));
            out.extend(
                want.difference(&got)
                    .map(|n| (n.clone(), "missing".to_owned())),
            );
            out.extend(
                got.difference(&want)
                    .map(|n| (n.clone(), "extra".to_owned())),
            );
        } else if got != want {
            let (g, w) = (got.as_array().unwrap(), want.as_array().unwrap());
            assert_eq!(g.len(), w.len(), "row {row}");
            for (i, (a, b)) in g.iter().zip(w).enumerate() {
                if a == b {
                    continue;
                }
                let name = match (a.as_str(), b.as_str()) {
                    (Some(_), Some(b)) => b.split(':').next().unwrap().to_owned(),
                    _ => ROW9[i].to_owned(),
                };
                // Missing: `X: undefined` against `X: function`, or the
                // JSON `null` of an `undefined` against a value.
                let missing = a.is_null() && !b.is_null()
                    || a.as_str().is_some_and(|s| s.ends_with(": undefined"));
                let text = if missing {
                    "missing".to_owned()
                } else {
                    format!("differs: swb {a}, Chromium {b}")
                };
                out.push((name, text));
            }
        }
        out
    }

    /// On the videojs.org fixture (JavaScript on, the page's own scripts run
    /// with media on), the media API exists: the surfaces of the rows of the
    /// probe case `api-surface` (which interfaces exist, which members they
    /// have, the constants, the defaults of a created element) equal
    /// Chromium's except what `ABSENT` lists, and the page's own
    /// `<video>` is a working `HTMLVideoElement`.
    #[test]
    fn the_media_element_api_exists_on_videojs_org() {
        let (differences, own_video) = on_page_thread(|| {
            let Some(mut page) =
                common::load_fixture("videojs", true, Size::new(1280.0, 800.0), |p| {
                    p.set_clock(PageClock::new(ClockMode::Virtual, None));
                })
            else {
                return (None, Value::Null);
            };
            let probe: Value = serde_json::from_str(
                &std::fs::read_to_string(common::repo_root().join(API_PROBE)).unwrap(),
            )
            .unwrap();
            let chromium: Value =
                serde_json::from_str(&common::media_site::chromium_json(API_ANSWERS)).unwrap();
            let case = probe["cases"]
                .as_array()
                .unwrap()
                .iter()
                .position(|c| c["name"] == "api-surface")
                .unwrap();
            let mut out = Vec::new();
            for row in 0..=10 {
                let query = probe["cases"][case]["measure"][row]["js"].as_str().unwrap();
                // The rows read `v` (a `<video>`) and `a` (an `<audio>`).
                let wrapped = format!(
                    "((v, a) => ({query}))(document.createElement('video'), document.createElement('audio'))"
                );
                let got = evaluate(&mut page, &wrapped);
                let want = &chromium["cases"][case]["results"][row]["value"];
                out.extend(row_differences(row, &got, want));
            }
            let own = evaluate(
                &mut page,
                "(() => { const v = document.querySelector('video'); return v && [v instanceof HTMLVideoElement, v instanceof HTMLMediaElement, typeof v.play, typeof v.canPlayType, v.canPlayType('video/mp4; codecs=\"avc1.64002a\"'), typeof v.buffered.length, v.readyState]; })()",
            );
            (Some(out), own)
        });
        let Some(differences) = differences else {
            eprintln!("skipped: the videojs.org fixture is not there (local fixture)");
            return;
        };
        let mut failures: Vec<String> = Vec::new();
        for (name, what) in &differences {
            if what != "missing" || !ABSENT.iter().any(|(n, _)| n == name) {
                failures.push(format!("{name}: {what}"));
            }
        }
        for (name, reason) in ABSENT {
            if !differences.iter().any(|(n, w)| n == name && w == "missing") {
                failures.push(format!(
                    "{name} ({reason}) is listed as missing and swb has it now"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // The page's own video element (the hero's tech element).
        let own = own_video.as_array().expect("the page has a video element");
        assert_eq!(own[0], true);
        assert_eq!(own[1], true);
        assert_eq!(own[2], "function");
        assert_eq!(own[3], "function");
        assert_eq!(own[4], "probably");
    }
}
