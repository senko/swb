//! M14 feature 6b (ADR 0047 section 10) with the feature `media`: the parts
//! of the playback API that the probe rows do not decide: `autoplay` with
//! JavaScript off, removal and re-insertion, the late failure of a damaged
//! file, floods of `play()`, `pause()` and seeks in one task, many playing
//! videos through 16 places, many media in one page, and the same events on
//! every run while the machine is busy. Run by `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::media_probe::evaluate_strict as evaluate;
use common::media_site::{FOLDER, MP4, MediaSite, VIEW, open_page};
use common::{node, on_page_thread, on_page_threads, with_busy_threads};
use serde_json::Value;
use swb_engine::{ClockMode, Evaluation, JsonForm, Page};
const MP3: &str = "fixtures/media/mp3.mp3";
const WAV: &str = "fixtures/media/wav-pcm-s16.wav";
const CORRUPT: &str = "fixtures/media/mp4-h264-high-aac-corrupt.mp4";
const LONG: &str = "fixtures/media/mp4-h264-aac-long.mp4";

fn page_for(html: &str, files: &[(&str, &str)], javascript: bool) -> (Page, Arc<MediaSite>) {
    let mut site = MediaSite::new(FOLDER, html);
    for (name, path) in files {
        site.add_file(name, path);
    }
    open_page(site, VIEW, ClockMode::Virtual, |page| {
        page.set_javascript(javascript);
    })
}

/// The helpers of the probe pages: `evs()` (the events of `watch(m)` with
/// their states), `once(m, name)` and `sleep(ms)`.
const HELPERS: &str = "<script>\
    const EVO = [];\
    const q4 = x => Math.round(x * 4) / 4;\
    const sleep = ms => new Promise(r => setTimeout(r, ms));\
    const once = (m, n, ms = 8000) => new Promise(r => { const to = setTimeout(() => r('timeout'), ms); m.addEventListener(n, () => { clearTimeout(to); r(n); }, {once: true}); });\
    function watch(m, tag = '') { ['loadstart','loadeddata','canplay','canplaythrough','playing','waiting','seeking','seeked','ended','timeupdate','play','pause','error','emptied','abort','volumechange'].forEach(n => m.addEventListener(n, () => { EVO.push(n + ' rs=' + m.readyState + ' ct=' + q4(m.currentTime) + (m.paused ? ' P' : '')); })); }\
    </script>";

#[test]
fn without_scripts_a_muted_autoplay_video_plays_and_an_unmuted_one_does_not() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html>\
            <video id=a src=a.mp4 autoplay muted></video>\
            <video id=b src=a.mp4 autoplay></video>\
            <audio id=c src=a.mp3 autoplay muted></audio>\
            <video id=d src=a.mp4 autoplay muted preload=none loop></video>";
        let (mut page, site) = page_for(html, &[("a.mp4", MP4), ("a.mp3", MP3)], false);
        page.run_event_loop(Duration::from_millis(1100), Duration::from_secs(30));
        let info = |page: &mut Page, id: &str| {
            let n = node(page, id);
            page.media_info(n).unwrap()
        };
        // The muted video plays; its position moves with page time.
        let a = info(&mut page, "a");
        assert!(!a.paused && !a.ended, "{a:?}");
        assert!((a.current_time - 1.1).abs() < 0.05, "{}", a.current_time);
        // The unmuted `autoplay` video and the muted audio do not (no
        // activation: the policy), but they loaded.
        for id in ["b", "c"] {
            let v = info(&mut page, id);
            assert!(
                v.paused && v.ready_state == 4 && v.current_time == 0.0,
                "#{id} {v:?}"
            );
        }
        // `autoplay` wins over `preload=none`; `loop` goes on past the end.
        let d = info(&mut page, "d");
        assert!(!d.paused, "{d:?}");
        page.run_event_loop(Duration::from_secs(3), Duration::from_secs(30));
        let a = info(&mut page, "a");
        assert!(a.paused && a.ended && a.current_time == 2.0, "{a:?}");
        let d = info(&mut page, "d");
        assert!(!d.paused && !d.ended, "{d:?}");
        assert_eq!(d.played, vec![(0.0, 2.0)], "{d:?}");
        // The request log: each file once per element that loaded.
        assert_eq!(
            site.log_lines()
                .iter()
                .filter(|l| l.starts_with("a.mp4"))
                .count(),
            3
        );
        // A user activation (a click) does not start what the page did not
        // ask for again: `autoplay` decided at the first data.
        page.click(10.0, 10.0);
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(30));
        assert!(info(&mut page, "b").paused);
    });
}

#[test]
fn a_removed_element_pauses_and_play_opens_its_suspended_pipeline_again() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
            <script>watch(v)</script>"
        );
        let (mut page, _site) = page_for(&html, &[("a.mp4", MP4)], true);
        let out = evaluate(
            &mut page,
            "(async () => { const m = v; \
                await once(m, 'canplaythrough'); m.play(); await once(m, 'playing'); await sleep(600); \
                const n = EVO.length; m.remove(); await sleep(400); \
                const removed = [m.paused, q4(m.currentTime), m.played.length]; \
                const during = EVO.slice(n); \
                await sleep(20000); \
                document.body.appendChild(m); await sleep(100); \
                const back = [m.paused, q4(m.currentTime)]; \
                const p = m.play(); const r = await Promise.race([p.then(() => 'resolved'), sleep(500).then(() => 'pending')]); \
                await once(m, 'ended', 5000); \
                return {removed, during, back, r, ended: m.ended, ct: m.currentTime, played: m.played.length}; })()",
        );
        assert_eq!(out["removed"], serde_json::json!([true, 0.5, 1]));
        assert_eq!(
            out["during"],
            serde_json::json!(["timeupdate rs=4 ct=0.5 P", "pause rs=4 ct=0.5 P"])
        );
        // Re-insertion does not resume the playback; `play()` does, from
        // where it stopped, through the pipeline that suspended at removal.
        assert_eq!(out["back"], serde_json::json!([true, 0.5]));
        assert_eq!(out["r"], "resolved");
        assert_eq!(out["ended"], true);
        assert_eq!(out["ct"], 2.0);
        assert_eq!(out["played"], 1);
        assert!(page.media_pipelines() <= 1);
    });
}

/// At the end of a looping media the playback seeks to the start in the next
/// unit: `seeking` with `currentTime` 0, no `ended`, no `pause`. A playback
/// that stays at the end spins in virtual time, so the evaluation has a short
/// wall-clock limit: the test fails in seconds, not after the limit of the
/// probe tests.
#[test]
fn a_looping_media_seeks_to_the_start_at_its_end() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted loop></video>\
            <script>watch(v)</script>"
        );
        let (mut page, _site) = page_for(&html, &[("a.mp4", MP4)], true);
        let started = Instant::now();
        let out = match page.evaluate_with_timeout(
            "(async () => { \
                await once(v, 'canplaythrough'); v.currentTime = 1.5; await once(v, 'seeked'); \
                const n = EVO.length; v.play(); const r = await once(v, 'seeking', 3000); await sleep(100); \
                return {r, ev: EVO.slice(n), ended: v.ended, paused: v.paused}; })()",
            JsonForm::Strict,
            Duration::from_secs(5),
        ) {
            Some(Evaluation::Value(text)) => serde_json::from_str::<Value>(&text).unwrap(),
            other => panic!("the loop does not restart: {other:?}"),
        };
        assert_eq!(out["r"], "seeking", "{out}");
        let events: Vec<&str> = out["ev"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap())
            .collect();
        // The `timeupdate` at the end (`currentTime` 2), then the seek.
        let seeking = events.iter().position(|e| e.starts_with("seeking"));
        assert!(
            seeking.is_some_and(|i| events[i].contains("ct=0")),
            "{events:?}"
        );
        assert!(!events.iter().any(|e| e.starts_with("ended")), "{events:?}");
        assert_eq!(out["ended"], false);
        assert_eq!(out["paused"], false);
        assert!(started.elapsed() < Duration::from_secs(5));
    });
}

#[test]
fn a_damaged_file_fails_at_play_and_a_seek_of_the_failed_element_ends() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
            <script>watch(v)</script>"
        );
        let (mut page, _site) = page_for(&html, &[("a.mp4", CORRUPT)], true);
        let out = evaluate(
            &mut page,
            "(async () => { \
                await once(v, 'canplaythrough'); await sleep(500); \
                const n = EVO.length; v.play(); await once(v, 'error', 3000); await sleep(300); \
                const first = [v.error && v.error.code, v.paused, q4(v.currentTime), EVO.slice(n).join(',')]; \
                const m = EVO.length; v.currentTime = 1; const seeked = await once(v, 'seeked', 3000); await sleep(50); \
                const seek = [seeked, v.seeking, q4(v.currentTime), EVO.slice(m).join(',')]; \
                const k = EVO.length; v.play(); await once(v, 'error', 3000); await sleep(100); \
                const again = [v.error && v.error.code, v.paused, q4(v.currentTime), EVO.slice(k).join(',')]; \
                return {first, seek, again}; })()",
        );
        assert_eq!(
            out["first"],
            serde_json::json!([
                3,
                true,
                0.75,
                "play rs=4 ct=0,playing rs=4 ct=0,timeupdate rs=4 ct=0.25,timeupdate rs=4 ct=0.5,error rs=4 ct=0.75 P,timeupdate rs=4 ct=0.75 P,pause rs=4 ct=0.75 P"
            ])
        );
        assert_eq!(
            out["seek"],
            serde_json::json!([
                "seeked",
                false,
                1,
                "seeking rs=1 ct=1 P,timeupdate rs=4 ct=1 P,seeked rs=4 ct=1 P,canplay rs=4 ct=1 P,canplaythrough rs=4 ct=1 P"
            ])
        );
        // Playing again from a position past the damage fails at once, at
        // that position (swb's rule: the playback never fails before where
        // it is).
        assert_eq!(out["again"][0], 3);
        assert_eq!(out["again"][1], true);
        assert_eq!(out["again"][2], 1);
    });
}

#[test]
fn many_play_and_pause_calls_in_one_task_settle_every_promise() {
    on_page_thread(|| {
        let html =
            format!("<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=none muted></video>");
        let (mut page, _site) = page_for(&html, &[("a.mp4", MP4)], true);
        let started = Instant::now();
        let out = evaluate(
            &mut page,
            "(async () => { \
                await sleep(50); const settled = []; \
                for (let i = 0; i < 30000; i++) { settled.push(v.play().then(() => 'resolved', e => e.name)); v.pause(); } \
                const names = await Promise.all(settled); \
                const counts = {}; for (const n of names) counts[n] = (counts[n] || 0) + 1; \
                return {counts, paused: v.paused}; })()",
        );
        // Every `pause()` rejects the promise before it.
        assert_eq!(out["counts"]["AbortError"], 30000, "{out}");
        assert_eq!(out["paused"], true);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    });
}

#[test]
fn more_promises_than_the_bound_reject_the_rest_at_once() {
    on_page_thread(|| {
        let html =
            format!("<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=none muted></video>");
        let (mut page, _site) = page_for(&html, &[("a.mp4", MP4)], true);
        let out = evaluate(
            &mut page,
            "(async () => { \
                await sleep(50); const settled = []; \
                for (let i = 0; i < 70000; i++) settled.push(v.play().then(() => 'resolved', e => e.name + ': ' + e.message)); \
                const early = await Promise.race([settled[69999], sleep(10).then(() => 'pending')]); \
                v.pause(); \
                const names = await Promise.all(settled); \
                const counts = {}; for (const n of names) counts[n] = (counts[n] || 0) + 1; \
                return {early, counts}; })()",
        );
        // The numbers of the promises that wait are bounded; the others
        // reject at once.
        assert_eq!(
            out["early"],
            "AbortError: Too many play() requests are waiting."
        );
        assert_eq!(
            out["counts"]["AbortError: The play() request was interrupted by a call to pause(). https://goo.gl/LdLk22"],
            65536
        );
        assert_eq!(
            out["counts"]["AbortError: Too many play() requests are waiting."],
            70000 - 65536
        );
    });
}

#[test]
fn a_flood_of_seeks_in_one_task_ends_with_the_last_one() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
            <script>watch(v)</script>"
        );
        let (mut page, _site) = page_for(&html, &[("a.mp4", MP4)], true);
        let started = Instant::now();
        let out = evaluate(
            &mut page,
            "(async () => { \
                await once(v, 'canplaythrough'); let seekings = 0; \
                v.addEventListener('seeking', () => seekings++); \
                for (let i = 0; i < 200000; i++) v.currentTime = (i % 2) ? 0.5 : 1.5; \
                v.currentTime = 1.25; \
                await once(v, 'seeked'); await sleep(100); \
                return {ct: v.currentTime, seeking: v.seeking, rs: v.readyState, seekings}; })()",
        );
        assert_eq!(out["ct"], 1.25);
        assert_eq!(out["seeking"], false);
        assert_eq!(out["rs"], 4);
        // The queue of events is bounded: not every `seeking` is delivered.
        assert!(out["seekings"].as_u64().unwrap() <= 70000, "{out}");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    });
}

#[test]
fn a_thousand_muted_videos_play_through_sixteen_places() {
    on_page_thread(|| {
        let mut html = String::from("<!DOCTYPE html>");
        for _ in 0..1000 {
            html.push_str("<video src=a.mp3 autoplay muted preload=metadata></video>");
        }
        let (mut page, _site) = page_for(&html, &[("a.mp3", MP3)], false);
        let started = Instant::now();
        // 1,000 elements of 2 s, 16 at a time: every element ends.
        let mut ended = 0;
        for _ in 0..200 {
            page.run_event_loop(Duration::from_secs(10), Duration::from_secs(60));
            assert!(page.media_pipelines() <= 16);
            let nodes = page.query_selector_all("video").unwrap();
            ended = nodes
                .iter()
                .filter(|&&n| page.media_info(n).unwrap().ended)
                .count();
            if ended == 1000 {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(90), "{ended} ended");
        }
        assert_eq!(ended, 1000);
    });
}

#[test]
fn many_media_play_one_after_the_other_in_one_page() {
    // A pipeline that has ended keeps its place until it suspends; the time
    // of a due point goes through the page clock as milliseconds. Rounding
    // down made the 9th element wait for a point that was one microsecond
    // away, for ever.
    on_page_thread(|| {
        let html = format!("<!DOCTYPE html>{HELPERS}");
        let (mut page, _site) = page_for(&html, &[("a.wav", WAV)], true);
        let out = evaluate(
            &mut page,
            "(async () => { const ends = []; \
                for (let i = 0; i < 12; i++) { \
                    const m = document.createElement('audio'); m.muted = true; document.body.appendChild(m); m.src = 'a.wav'; \
                    m.play(); ends.push(await once(m, 'ended', 5000) + ' ' + m.currentTime); } \
                return ends; })()",
        );
        assert_eq!(out, serde_json::json!(vec!["ended 2"; 12]));
    });
}

/// The bytes of `data` with the damage of `damage()` in
/// `tools/media/generate.py`: 600 bytes of a linear congruential generator
/// (seed 16) at `fraction` of the file. The 10 s file is damaged here, as its
/// damaged copy is too big to commit.
fn damaged(data: &[u8], fraction: f64) -> Vec<u8> {
    let mut x: u64 = 16;
    let noise = (0..600).map(|_| {
        x = (x * 1_103_515_245 + 12_345) & 0x7FFF_FFFF;
        ((x >> 16) & 0xFF) as u8
    });
    let at = (data.len() as f64 * fraction) as usize;
    let mut out = data[..at].to_vec();
    out.extend(noise);
    out.extend_from_slice(&data[(at + 600).min(data.len())..]);
    out
}

/// A page with a `video` of the damaged copy of `mp4` and a playback to its
/// failure: `error` events with their `currentTime`, `timeupdate` times and
/// the end state.
fn playback_to_the_damage(bytes: Vec<u8>) -> Value {
    let mut site = MediaSite::new(
        FOLDER,
        &format!(
            "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
        <script>watch(v)</script>"
        ),
    );
    site.add_bytes("a.mp4", bytes);
    let (mut page, _site) = open_page(site, VIEW, ClockMode::Virtual, |page| {
        page.set_javascript(true);
    });
    evaluate(
        &mut page,
        "(async () => { \
            await once(v, 'canplaythrough'); await sleep(300); \
            const times = []; let at = null; \
            v.addEventListener('timeupdate', () => times.push(v.currentTime)); \
            v.addEventListener('error', () => { at = v.currentTime; }); \
            const n = EVO.length; v.play(); await once(v, 'error', 30000); await sleep(500); \
            return {at, last: Math.max(...times), code: v.error && v.error.code, paused: v.paused, \
                played: Array.from({length: v.played.length}, (_, i) => [v.played.start(i), v.played.end(i)]), \
                ev: EVO.slice(n)}; })()",
    )
}

/// The time of the `error` of a damaged file (a damage at a percent of the
/// file) and Chromium 148's (`late-failure-damage-*` of
/// `media-playback-2.json`, which the probe test compares within 0.26 s; the
/// rule of swb's playback is `READY_AHEAD_US` (0.2 s) before the place where
/// its decoder stopped, which is within 0.1 s of Chromium's here).
const DAMAGE_ROWS: [(&str, f64, f64); 4] = [
    ("30", 0.3, 0.12),
    ("60", 0.6, 0.81),
    ("80", 0.8, 1.22),
    ("90", 0.9, 1.44),
];

/// A late failure (the damage lies after the metadata) comes where the
/// decoder stopped, at a time that does not depend on the threads: a later
/// damage never fails earlier, `currentTime` never passes the failure before
/// its `error`, and the time is near Chromium's.
#[test]
fn late_failures_follow_the_damage() {
    on_page_thread(|| {
        let whole = std::fs::read(common::repo_root().join(MP4)).unwrap();
        let mut last = -1.0;
        for (percent, fraction, chromium) in DAMAGE_ROWS {
            // The Rust damage is the file of the fixture.
            let file = format!("fixtures/media/mp4-h264-high-aac-damaged-{percent}.mp4");
            let bytes = damaged(&whole, fraction);
            assert_eq!(
                bytes,
                std::fs::read(common::repo_root().join(&file)).unwrap()
            );
            let out = playback_to_the_damage(bytes);
            let at = out["at"].as_f64().unwrap();
            assert_eq!(out["code"], 3, "{percent}: {out}");
            assert_eq!(out["paused"], true);
            assert!(
                (at - chromium).abs() <= 0.1,
                "{percent}: {at}, Chromium {chromium}"
            );
            assert!(at > last, "{percent}: {at} does not come after {last}");
            assert!(
                out["last"].as_f64().unwrap() <= at + 1e-9,
                "{percent}: {out}"
            );
            last = at;
        }
    });
}

/// The 10 s file (a keyframe every second) damaged at 70 %: Chromium 148's
/// `error` comes at 6.64 s (`late-failure-long-70`, three equal runs; at 60 %
/// it is 5.64 s, and swb's 5.70 s, see the departures in ADR 0047). The failure
/// arrives while the video plays, at a time that depends on the threads; the
/// events do not (twelve pages at once, three times, a busy thread per core).
/// The damage at 70 % hits one decoder: at 40, 60 and 90 % it hits the video
/// and the audio at nearly the same time, and the decoder that reports first
/// decides the place (two values 0.02 s apart).
#[test]
fn a_late_failure_while_playing_has_the_same_events_on_every_run_also_under_load() {
    let whole = Arc::new(std::fs::read(common::repo_root().join(LONG)).unwrap());
    let bytes = Arc::clone(&whole);
    let alone = on_page_thread(move || playback_to_the_damage(damaged(&bytes, 0.7)));
    let at = alone["at"].as_f64().unwrap();
    assert_eq!(alone["code"], 3, "{alone}");
    assert!((at - 6.64).abs() <= 0.1, "{at}");
    assert!(alone["last"].as_f64().unwrap() <= at + 1e-9, "{alone}");
    let runs = with_busy_threads(|| {
        (0..3)
            .flat_map(|_| {
                let whole = Arc::clone(&whole);
                on_page_threads((0..12).map(move |_| {
                    let whole = Arc::clone(&whole);
                    move || playback_to_the_damage(damaged(&whole, 0.7))
                }))
            })
            .collect::<Vec<Value>>()
    });
    for run in runs {
        assert_eq!(run, alone);
    }
}

/// A page with a `video` that starts at a `#t=1` fragment of the file damaged
/// at 60 %: `readyState` and the events after `loadeddata` (or its timeout).
fn a_fragment_start_on_a_damaged_file() -> Value {
    let html = format!(
        "<!DOCTYPE html>{HELPERS}<video id=v src='a.mp4#t=1.0' preload=auto muted></video>\
        <script>watch(v)</script>"
    );
    let (mut page, _site) = page_for(
        &html,
        &[("a.mp4", "fixtures/media/mp4-h264-high-aac-damaged-60.mp4")],
        true,
    );
    evaluate(
        &mut page,
        "(async () => { await once(v, 'loadeddata', 8000); await sleep(300); \
            return {rs: v.readyState, ev: EVO}; })()",
    )
}

/// The start seek of a `#t=` fragment holds the note of the first data
/// until the seek ends (`Fetch::seek`). When the pipeline fails during that
/// seek, the seek never ends, and the note must still come: the element
/// reaches `HAVE_ENOUGH_DATA` with `loadeddata` on every page, also when the
/// first frame came before the seek (a race that needs load: sixteen pages
/// at once, twice, and a busy thread per core).
#[test]
fn a_failure_during_the_start_seek_still_gives_the_first_data() {
    let check = |out: &Value| {
        assert_eq!(out["rs"], 4, "{out}");
        let events = out["ev"].as_array().unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.as_str().unwrap().starts_with("loadeddata")),
            "{out}"
        );
    };
    check(&on_page_thread(a_fragment_start_on_a_damaged_file));
    let runs: Vec<Value> = with_busy_threads(|| {
        (0..2)
            .flat_map(|_| on_page_threads((0..16).map(|_| a_fragment_start_on_a_damaged_file)))
            .collect()
    });
    for run in &runs {
        check(run);
    }
}

/// The events of two playing media and a seek in one page, as `evs` shows
/// them (JavaScript on, virtual time).
fn events_of_a_playback() -> Value {
    let html = format!(
        "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted loop></video>\
        <audio id=a src=a.mp3 preload=auto></audio>\
        <script>watch(v); watch(a);</script>"
    );
    let (mut page, _site) = page_for(&html, &[("a.mp4", MP4), ("a.mp3", MP3)], true);
    evaluate(
        &mut page,
        "(async () => { await once(v, 'canplaythrough'); await once(a, 'canplaythrough'); \
            v.play(); a.play(); await sleep(700); v.currentTime = 1.5; a.currentTime = 1; await sleep(1000); \
            v.pause(); await once(a, 'ended', 4000); return EVO; })()",
    )
}

/// A run gives the same events and values on every run, also under load
/// (twelve pages at once, three times, and a busy thread per core).
#[test]
fn the_events_of_a_playback_are_the_same_on_every_run_also_under_load() {
    let alone = on_page_thread(events_of_a_playback);
    assert!(alone.as_array().unwrap().len() > 20, "{alone}");
    let runs: Vec<Value> = with_busy_threads(|| {
        (0..3)
            .flat_map(|_| on_page_threads((0..12).map(|_| events_of_a_playback)))
            .collect()
    });
    for run in runs {
        assert_eq!(run, alone);
    }
}
