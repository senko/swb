//! M14 feature 6a (ADR 0047 section 5, diagnosis section 5): what the media
//! element fetches and the state it reaches, against Chromium. The cases of
//! `tools/probes/media-fetch.json` run in the engine in virtual time against
//! the simulated case server (`common/media_site.rs`); the part of each
//! probe query that loads (the 1.2 s after the load: the requests and
//! `readyState`, `networkState`, `duration`, `buffered`) is compared with
//! Chromium 148's answers (`tests/data/media-fetch.json`: three runs of
//! `swbtools probe --json tools/probes/media-fetch.json`, equal except
//! `fetch-ignore-range-accept-ranges-long`, whose request sizes and
//! buffered end vary, like `media-real-files.json`). The two cases of a
//! server that ignores `Range` and claims ranges also run their seek (feature
//! 6b: `currentTime` to 60 %, `seeked`, the state and the requests after it);
//! the seek in a response that ignores `Range` is tested at the byte source
//! too (`swb-net`, `swb-media` `tests/net.rs`).
//!
//! The files of the long MP4 (`mp4-h264-aac-long.mp4`, 300 KB) are read
//! partly by Chromium and its requests vary: only its state is compared.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::media_probe;
use common::media_site::{MediaSite, RangeMode};
use common::on_page_threads;
use common::probe_rows::load_cases;
use serde_json::{Value, json};
use swb_engine::{ClockMode, Evaluation, JsonForm, PageClock, Size, Url};
use swb_net::Fetcher;

const ANSWERS: &str = include_str!("data/media-fetch.json");
const PROBE: &str = "tools/probes/media-fetch.json";

/// Where swb differs from Chromium on purpose: (case, path, reason).
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "fetch-ogg-vorbis-no-ranges-preload-metadata",
        "s0.dur",
        "a resource that cannot seek: Chromium knows the size from `Content-Length` and estimates 1.986757 s; swb gives FFmpeg no size for a stream, so the duration is `Infinity` (backlog: give FFmpeg the length of a 200 as `avio_size`)",
    ),
    (
        "fetch-ogg-vorbis-no-ranges-preload-auto",
        "s0.dur",
        "an Ogg file that cannot seek (see above)",
    ),
    (
        "fetch-mp3-no-ranges-preload-metadata",
        "s0.dur",
        "an MP3 file that cannot seek: Chromium has 2 from the size, swb `Infinity` (see above)",
    ),
    (
        "fetch-mp3-no-ranges-preload-auto",
        "s0.dur",
        "an MP3 file that cannot seek (see above)",
    ),
    (
        "fetch-wav-no-ranges-preload-metadata",
        "s0.dur",
        "a WAV file that cannot seek: Chromium has 2 from the size, swb `Infinity` (see above)",
    ),
    (
        "fetch-wav-no-ranges-preload-auto",
        "s0.dur",
        "a WAV file that cannot seek (see above)",
    ),
];

fn answers() -> &'static str {
    media_probe::answers(ANSWERS)
}

fn folder_of(name: &str) -> String {
    media_probe::folder_of(PROBE, name)
}

/// The numbers of a `buffered` text (`0-2,3-4`; a start can be negative).
fn range_numbers(text: &str) -> Option<Vec<f64>> {
    let mut numbers = Vec::new();
    for range in text.split(',').filter(|r| !r.is_empty()) {
        let at = range.get(1..)?.find('-')? + 1;
        numbers.push(range[..at].parse().ok()?);
        numbers.push(range[at + 1..].parse().ok()?);
    }
    Some(numbers)
}

/// True if two `buffered` texts have the same ranges within 0.02 s
/// (Chromium's buffered ends depend on `preload` and on AAC priming).
fn same_buffered(a: &str, b: &str) -> bool {
    match (range_numbers(a), range_numbers(b)) {
        (Some(a), Some(b)) => {
            a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 0.02)
        }
        _ => false,
    }
}

fn run_case(case: &'static Value, answer: &'static Value) -> Vec<(String, String)> {
    let name = case["name"].as_str().unwrap();
    let folder = folder_of(name);
    let mut site = MediaSite::new(&folder, case["html"].as_str().unwrap());
    for (served, path) in case["files"].as_object().unwrap() {
        site.add_file(served, path.as_str().unwrap());
    }
    site.mode = match &case["ranges"] {
        Value::Bool(false) => RangeMode::Ignore,
        Value::String(s) if s == "ignore" => RangeMode::ClaimButIgnore,
        _ => RangeMode::Honor,
    };
    let site = Arc::new(site);
    let mut page = common::new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(&MediaSite::page_url(&folder)).unwrap());
    assert!(
        page.wait_until_loaded(Duration::from_secs(30)),
        "{name}: load"
    );
    // The first part of the probe's query: the requests and the state after
    // 1.2 s. The two cases of a server that ignores `Range` and claims
    // ranges run all of their query (M14 feature 6b): a seek to 60 % and the
    // state and requests after it.
    let seek_case = name.contains("ignore-range");
    let query = if seek_case {
        case["measure"][0]["js"].as_str().unwrap().to_owned()
    } else {
        "(async () => { const out = {}; let n = 0; \
        const take = async () => { const l = await log(); const r = l.slice(n); n = l.length; return r; }; \
        await sleep(1200); out.load = await take(); \
        out.s0 = {rs: v.readyState, ns: v.networkState, dur: v.duration, buf: rng(v.buffered)}; \
        return out; })()"
            .to_owned()
    };
    let mut got: Value =
        match page.evaluate_with_timeout(&query, JsonForm::Strict, Duration::from_secs(60)) {
            Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
            other => panic!("{name}: {other:?}"),
        };
    let chromium = &answer["results"][0]["value"];
    let mut want = if seek_case {
        chromium.clone()
    } else {
        json!({ "load": chromium["load"], "s0": chromium["s0"] })
    };
    if name.contains("long") {
        // Chromium's requests and buffered end vary: the state only.
        for part in [&mut want, &mut got] {
            let after = &part["afterSeek"];
            *part = json!({
                "s0": { "rs": part["s0"]["rs"], "ns": part["s0"]["ns"], "dur": part["s0"]["dur"] },
                "seeked": part["seeked"],
                "afterSeek": { "rs": after["rs"], "ct": after["ct"], "seeking": after["seeking"], "err": after["err"] },
            });
        }
    }
    // `buf` within 0.02 s counts as equal.
    if let (Some(w), Some(g)) = (want["s0"]["buf"].as_str(), got["s0"]["buf"].as_str())
        && same_buffered(w, g)
    {
        want["s0"]["buf"] = Value::Null;
        got["s0"]["buf"] = Value::Null;
    }
    let mut out = Vec::new();
    common::json_differences("", &got, &want, &mut out);
    out
}

#[test]
fn the_requests_and_states_of_a_load_match_chromium() {
    let cases = load_cases(PROBE, answers());
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(case, answer)| move || (case["name"].as_str().unwrap(), run_case(case, answer)))
        .collect();
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    for (name, differences) in on_page_threads(runs) {
        for (path, text) in differences {
            let path = path.trim_start_matches('.').to_owned();
            if DEPARTURES.iter().any(|(c, p, _)| *c == name && *p == path) {
                seen.push((name, path));
            } else {
                failures.push(format!("{name} {path}: {text}"));
            }
        }
    }
    for (case, path, _) in DEPARTURES {
        if !seen.iter().any(|(c, p)| c == case && p == path) {
            failures.push(format!("{case} {path}: the departure no longer occurs"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
