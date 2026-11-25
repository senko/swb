//! M14 feature 6a (ADR 0047 sections 3 and 10): `canPlayType()` and the
//! interfaces of the media element against Chromium. Every row of
//! `tools/probes/media-can-play-type.json` (149 types, `video` and `audio`
//! elements) runs in the engine and its answers are compared with Chromium
//! 148's (`tests/data/media-can-play-type.json`: three equal runs of
//! `swbtools probe --json tools/probes/media-can-play-type.json`).
//!
//! With the feature `media` swb answers as Chromium, except for two
//! departures on purpose: the HLS types (`application/vnd.apple.mpegurl`
//! and the like) are `""` (ground rules: until swb has native HLS, sites
//! must use hls.js), and every type with an AV1 codec (`av01...`) is `""`
//! (the FFmpeg build of ADR 0047 section 1 has no AV1 decoder). Without
//! the feature every answer is `""`. The `api-surface` rows that list
//! members of other features (`play`, `Audio`, `MediaSource`, text tracks)
//! are skipped; the rows of `HTMLAudioElement`, `MediaError`, `TimeRanges`
//! and the constants are compared.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{new_page, on_page_thread};
use serde_json::Value;
use swb_engine::{Evaluation, JsonForm, Size, Url};
use swb_net::Fetcher;

const ANSWERS: &str = include_str!("data/media-can-play-type.json");
const PROBE: &str = "tools/probes/media-can-play-type.json";

/// The HLS types: Chromium plays them with its native HLS player.
fn is_hls(mime: &str) -> bool {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "application/vnd.apple.mpegurl"
            | "application/x-mpegurl"
            | "audio/mpegurl"
            | "audio/x-mpegurl"
    )
}

/// swb's answer for a type that Chromium answers `chromium` for.
fn expected(mime: &str, chromium: &str) -> String {
    if !cfg!(feature = "media") || is_hls(mime) || mime.contains("av01") {
        return String::new();
    }
    chromium.to_owned()
}

/// A page with scripts; `TestSite` answers any URL with a small page, and
/// the case's own markup goes in through the DOM.
fn page() -> swb_engine::Page {
    let mut page = new_page(
        Arc::new(common::TestSite::default()) as Arc<dyn Fetcher>,
        1,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.navigate(Url::parse("https://site.test/media.html").unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn answer(page: &mut swb_engine::Page, js: &str) -> Value {
    match page.evaluate(js, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Some(Evaluation::Error(text)) => Value::String(format!("error: {text}")),
        None => Value::String("no runtime".to_owned()),
    }
}

#[test]
fn can_play_type_answers_as_chromium_in_this_build() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(common::repo_root().join(PROBE)).unwrap(),
        )
        .unwrap();
        let chromium: Value =
            serde_json::from_str(&common::media_site::chromium_json(ANSWERS)).unwrap();
        let mut page = page();
        let (mut rows, mut departures) = (0, 0);
        let mut failures = Vec::new();
        for (case, result) in probe["cases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(chromium["cases"].as_array().unwrap())
        {
            let name = case["name"].as_str().unwrap();
            if !name.starts_with("types-") {
                continue;
            }
            // The probe's own query, cut to the two `canPlayType` columns.
            let query = case["measure"][0]["js"].as_str().unwrap();
            let got = answer(&mut page, &format!("({query}).map(r => r.slice(0, 3))"));
            let want = result["results"][0]["value"].as_array().unwrap();
            let got = got.as_array().unwrap_or_else(|| panic!("{name}: {got}"));
            assert_eq!(got.len(), want.len(), "{name}: the rows");
            for (row, chromium_row) in got.iter().zip(want) {
                let mime = chromium_row[0].as_str().unwrap();
                let (video, audio) = (
                    chromium_row[1].as_str().unwrap(),
                    chromium_row[2].as_str().unwrap(),
                );
                assert_eq!(video, audio, "Chromium {mime:?}: video and audio differ");
                let want_swb = expected(mime, video);
                rows += 1;
                departures += usize::from(want_swb != video);
                let expected_row = serde_json::json!([mime, want_swb, want_swb]);
                if *row != expected_row {
                    failures.push(format!(
                        "{name} {mime:?}: swb {row}, expected {expected_row}"
                    ));
                }
            }
        }
        assert_eq!(rows, 149, "the rows of the probe file");
        if cfg!(feature = "media") {
            // 5 HLS types and 3 AV1 types (8) answer `maybe` or `probably` in
            // Chromium; the others are `""` there already.
            assert_eq!(departures, 8, "the departures that the answers show");
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The values of an element that has not started to load (the rows 9 and
/// 10 of `api-surface` that need no member of feature 6b), `TimeRanges`
/// and `MediaError` (Chromium 148, `media-states.json` and
/// `media-can-play-type.json`: the message of an index out of range).
#[test]
fn an_element_that_never_loads_and_the_objects_of_the_core_match_chromium() {
    on_page_thread(|| {
        let mut page = page();
        let got = answer(
            &mut page,
            "(() => { const v = document.createElement('video'); const a = document.createElement('audio'); \
              let end, again; \
              try { v.buffered.end(0); } catch (e) { end = e.name + ': ' + e.message; } \
              try { v.buffered.start(5); } catch (e) { again = e.name + ': ' + e.message; } \
              return [v.preload, a.preload, v.paused, v.autoplay, v.loop, v.controls, v.currentTime, v.duration, \
                v.readyState, v.networkState, v.currentSrc, v.error, v.seeking, v.buffered.length, v.seekable.length, \
                v.videoWidth, v.videoHeight, 'videoWidth' in a, Object.prototype.toString.call(v.buffered), \
                v.buffered === v.buffered, end, again, v.canPlayType('video/mp4'), \
                HTMLMediaElement.prototype.hasOwnProperty('load'), HTMLMediaElement.HAVE_ENOUGH_DATA, MediaError.MEDIA_ERR_DECODE]; })()",
        );
        assert_eq!(
            got,
            serde_json::json!([
                "metadata",
                "metadata",
                true,
                false,
                false,
                false,
                0,
                null,
                0,
                0,
                "",
                null,
                false,
                0,
                0,
                0,
                0,
                false,
                "[object TimeRanges]",
                false,
                "IndexSizeError: Failed to execute 'end' on 'TimeRanges': The index provided (0) is greater than the maximum bound (0).",
                "IndexSizeError: Failed to execute 'start' on 'TimeRanges': The index provided (5) is greater than the maximum bound (0).",
                if cfg!(feature = "media") { "maybe" } else { "" },
                true,
                4,
                3
            ])
        );
    });
}

#[test]
fn the_interfaces_of_the_core_match_chromium() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(common::repo_root().join(PROBE)).unwrap(),
        )
        .unwrap();
        let chromium: Value =
            serde_json::from_str(&common::media_site::chromium_json(ANSWERS)).unwrap();
        let case = probe["cases"]
            .as_array()
            .unwrap()
            .iter()
            .position(|c| c["name"] == "api-surface")
            .unwrap();
        let mut page = page();
        // The `video` and `audio` elements of the case.
        answer(
            &mut page,
            "document.body.innerHTML = '<video id=v></video><audio id=a></audio>'; globalThis.v = document.getElementById('v'); globalThis.a = document.getElementById('a'); 1",
        );
        // `HTMLAudioElement`, `MediaError`, `TimeRanges`, the constants.
        for row in [5, 6, 7, 8] {
            let query = probe["cases"][case]["measure"][row]["js"].as_str().unwrap();
            let want = &chromium["cases"][case]["results"][row]["value"];
            assert_eq!(&answer(&mut page, query), want, "row {row}: {query}");
        }
    });
}
