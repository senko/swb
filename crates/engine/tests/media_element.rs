//! M14 feature 6a (ADR 0047 sections 6 and 10) with the feature `media`: the
//! media element with JavaScript off (fetches per `preload`, the states in
//! the engine), the suspend of an idle pipeline after 15 s, the bound on
//! open pipelines, the hostile media of `fixtures/media/` and the same
//! events on every run (also while the machine is busy). Run by
//! `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::media_site::MediaSite;
use common::{new_page, node, on_page_thread, on_page_threads};
use serde_json::{Value, json};
use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::Fetcher;

const FOLDER: &str = "case-1";

fn page_for(site: MediaSite, javascript: bool) -> (Page, Arc<MediaSite>) {
    page_with(site, javascript, ClockMode::Virtual, Duration::ZERO)
}

/// A page of `site` with the clock `mode`; the last thread of a pipeline
/// run that ended waits `handover_delay` before it hands its place on.
fn page_with(
    site: MediaSite,
    javascript: bool,
    mode: ClockMode,
    handover_delay: Duration,
) -> (Page, Arc<MediaSite>) {
    let site = Arc::new(site);
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.isolate_media(handover_delay);
    page.set_javascript(javascript);
    page.set_clock(PageClock::new(mode, None));
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    (page, site)
}

fn site_with(html: &str, files: &[(&str, &str)]) -> MediaSite {
    let mut site = MediaSite::new(FOLDER, html);
    for (name, path) in files {
        site.add_file(name, path);
    }
    site
}

fn sorted(mut lines: Vec<String>) -> Vec<String> {
    lines.sort();
    lines
}

fn evaluate(page: &mut Page, js: &str) -> Value {
    match page.evaluate_with_timeout(js, JsonForm::Strict, Duration::from_secs(60)) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{js}: {other:?}"),
    }
}

const MP4: &str = "fixtures/media/mp4-h264-high-aac.mp4";
const LONG_MP4: &str = "fixtures/media/mp4-h264-aac-long.mp4";

#[test]
fn without_scripts_the_element_fetches_per_preload_and_reaches_its_states() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html>\
            <video id=m src=a.mp4 preload=metadata></video>\
            <video id=d src=b.mp4></video>\
            <audio id=au src=a.mp3 preload=auto controls></audio>\
            <video id=n src=c.mp4 preload=none></video>\
            <video id=s><source src=missing.mp4><source src=e.mp4 type='video/x-foo'><source src=f.mp4></video>";
        let site = site_with(
            html,
            &[
                ("a.mp4", MP4),
                ("b.mp4", MP4),
                ("c.mp4", MP4),
                ("f.mp4", MP4),
                ("a.mp3", "fixtures/media/mp3.mp3"),
            ],
        );
        let (mut page, site) = page_for(site, false);
        page.run_event_loop(Duration::from_secs(3), Duration::from_secs(30));
        let info = |page: &mut Page, id: &str| {
            let n = node(page, id);
            page.media_info(n).unwrap()
        };
        for id in ["m", "d", "s"] {
            let v = info(&mut page, id);
            assert_eq!(
                (
                    v.network_state,
                    v.ready_state,
                    v.duration,
                    v.video_width,
                    v.video_height
                ),
                (1, 4, 2.0, 320, 240),
                "#{id}"
            );
            assert_eq!(v.buffered, vec![(0.0, 2.0)], "#{id}");
            assert_eq!(v.seekable, vec![(0.0, 2.0)], "#{id}");
            assert!(v.error.is_none() && v.paused && !v.seeking, "#{id}");
        }
        let au = info(&mut page, "au");
        assert_eq!((au.network_state, au.ready_state), (1, 4));
        assert_eq!((au.video_width, au.video_height), (0, 0));
        assert!((au.duration - 2.0).abs() < 0.05, "{}", au.duration);
        // `preload=none`: nothing is fetched; the network is idle.
        let n = info(&mut page, "n");
        assert_eq!((n.network_state, n.ready_state), (1, 0));
        assert!(n.current_src.ends_with("/c.mp4"), "{}", n.current_src);
        assert!(n.duration.is_nan() && n.buffered.is_empty());
        // `source` children: the first fails (404), the second is skipped
        // for its type (no request), the third plays.
        let s = info(&mut page, "s");
        assert!(s.current_src.ends_with("/f.mp4"), "{}", s.current_src);
        assert_eq!(
            sorted(site.log_lines()),
            [
                "a.mp3 bytes=0- -> 206 16508",
                "a.mp4 bytes=0- -> 206 55902",
                "b.mp4 bytes=0- -> 206 55902",
                "f.mp4 bytes=0- -> 206 55902",
                "missing.mp4 bytes=0- -> 404 0",
            ]
        );
    });
}

#[test]
fn an_idle_pipeline_suspends_after_15_seconds_and_keeps_its_state() {
    on_page_thread(|| {
        let site = site_with(
            "<!DOCTYPE html><video id=m src=a.mp4></video>",
            &[("a.mp4", MP4)],
        );
        let (mut page, _site) = page_for(site, false);
        page.run_event_loop(Duration::from_secs(10), Duration::from_secs(30));
        assert_eq!(page.media_pipelines(), 1, "open before 15 s");
        page.run_event_loop(Duration::from_secs(10), Duration::from_secs(30));
        assert_eq!(page.media_pipelines(), 0, "suspended after 15 s");
        let m = node(&page, "m");
        let info = page.media_info(m).unwrap();
        assert_eq!(
            (info.network_state, info.ready_state, info.duration),
            (1, 4, 2.0)
        );
        assert_eq!(info.buffered, vec![(0.0, 2.0)]);
    });
}

#[test]
fn a_removed_element_suspends_its_pipeline_at_once() {
    on_page_thread(|| {
        let site = site_with(
            "<!DOCTYPE html><video id=m src=a.mp4></video>",
            &[("a.mp4", MP4)],
        );
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        assert_eq!(page.media_pipelines(), 1);
        evaluate(
            &mut page,
            "globalThis.m = document.getElementById('m'); m.remove(); 1",
        );
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        assert_eq!(page.media_pipelines(), 0);
        // The element keeps what it knows.
        assert_eq!(
            evaluate(&mut page, "[m.readyState, m.networkState, m.duration]"),
            json!([4, 1, 2])
        );
    });
}

#[test]
fn a_thousand_elements_load_through_sixteen_places_without_a_hang() {
    on_page_thread(|| {
        let mut html = String::from("<!DOCTYPE html>");
        for i in 0..1000 {
            let _ = write!(
                html,
                "<video id=v{i} src=a.mp4?{i} preload=metadata></video>"
            );
        }
        let mut site = MediaSite::new(FOLDER, &html);
        let mp4 = std::fs::read(common::repo_root().join(MP4)).unwrap();
        // `?N` names the same file 1,000 times; the server ignores the
        // query: register the path only.
        site.add_bytes("a.mp4", mp4);
        let started = Instant::now();
        let (mut page, site) = page_for(site, false);
        // 1,000 elements, 16 at a time, each idle for 15 s of virtual time
        // before it gives its place away: 63 rounds, about 950 s.
        let mut peak = 0;
        for _ in 0..80 {
            page.run_event_loop(Duration::from_secs(20), Duration::from_secs(120));
            peak = peak.max(page.media_pipelines());
            if page.media_pipelines() == 0 {
                break;
            }
        }
        assert!(peak <= 16, "{peak} pipelines were open at once");
        let mut loaded = 0;
        for i in 0..1000 {
            let n = node(&page, &format!("v{i}"));
            let info = page.media_info(n).unwrap();
            if info.ready_state == 4 && info.network_state == 1 && info.duration == 2.0 {
                loaded += 1;
            }
        }
        assert_eq!(loaded, 1000, "elements that loaded");
        assert_eq!(site.log_lines().len(), 1000);
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "took {:?}",
            started.elapsed()
        );
    });
}

/// Review S1 of feature 6a: the place of a pipeline that suspends goes to a
/// waiting one before virtual time moves on, however slowly the suspended
/// pipeline's threads end (here the last thread of each waits 300 ms of real
/// time before it hands its place on). 16 elements load at 0 s and suspend
/// at 15 s; the next 16 get their places at 15 s and load then.
#[test]
fn a_place_goes_to_the_next_pipeline_before_virtual_time_moves() {
    on_page_thread(|| {
        let mut html = String::from("<!DOCTYPE html>");
        for i in 0..32 {
            let _ = write!(
                html,
                "<video id=v{i} src=a.mp4?{i} preload=metadata></video>"
            );
        }
        let mut site = MediaSite::new(FOLDER, &html);
        site.add_bytes(
            "a.mp4",
            std::fs::read(common::repo_root().join(MP4)).unwrap(),
        );
        let (mut page, _site) =
            page_with(site, false, ClockMode::Virtual, Duration::from_millis(300));
        page.run_event_loop(Duration::from_millis(15_500), Duration::from_secs(60));
        let states: Vec<(u16, u16)> = (0..32)
            .map(|i| {
                let n = node(&page, &format!("v{i}"));
                let info = page.media_info(n).unwrap();
                (info.network_state, info.ready_state)
            })
            .collect();
        assert_eq!(states, vec![(1, 4); 32]);
        assert_eq!(page.media_pipelines(), 16);
    });
}

/// Review S2 of feature 6a: an element removed while it loads goes on
/// loading (HTML pauses it at removal, feature 6b, and does not stop the
/// load), also when it leaves in the task that set `src`, 0 ms later, or at
/// `loadstart`; inserted again, it has its data.
#[test]
fn an_element_removed_while_it_loads_goes_on_and_reaches_its_states() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><div id=box><video id=a></video><video id=b></video><video id=c></video></div><script>\
            const els = [a, b, c];\
            a.src = 'a.mp4?a'; a.remove();\
            b.src = 'a.mp4?b'; setTimeout(() => b.remove(), 0);\
            c.addEventListener('loadstart', () => c.remove()); c.src = 'a.mp4?c';\
            </script>";
        let site = site_with(html, &[("a.mp4", MP4)]);
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(&mut page, "els.map(e => e.isConnected)"),
            json!([false, false, false])
        );
        evaluate(&mut page, "box.append(...els); 1");
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(
                &mut page,
                "els.map(e => [e.networkState, e.readyState, e.duration])"
            ),
            json!([[1, 4, 2], [1, 4, 2], [1, 4, 2]])
        );
    });
}

/// The events of the load of `file` with their state and page time, over a
/// link that is `slow` (see `MediaSite::slow`) or fast.
fn events_over(
    slow: Option<(usize, Duration)>,
    file: &str,
    mode: ClockMode,
    seconds: u64,
) -> Value {
    let html = "<!DOCTYPE html><script>\
        const EVO = [];\
        const rng = t => Array.from({length: t.length}, (_, i) => t.start(i).toFixed(2) + '-' + t.end(i).toFixed(2)).join(',');\
        function watch(m) { ['loadstart','progress','suspend','durationchange','resize','loadedmetadata','loadeddata','canplay','canplaythrough','error'].forEach(n => m.addEventListener(n, () => EVO.push(n + ' rs=' + m.readyState + ' ns=' + m.networkState + ' buf=' + rng(m.buffered) + ' t=' + Math.round(performance.now())))); }\
        </script><video id=v src=a.mp4></video><script>watch(v)</script>";
    let mut site = MediaSite::new(FOLDER, html);
    site.slow = slow;
    site.add_file("a.mp4", file);
    let (mut page, _site) = page_with(site, true, mode, Duration::ZERO);
    page.run_event_loop(Duration::from_secs(seconds), Duration::from_secs(30));
    evaluate(&mut page, "EVO")
}

/// Review S3 of feature 6a (and mutation M6): in virtual time, the events
/// of a load of the 300 KB file over a slow link (16 KiB every 20 ms; the
/// demuxer opens after about half of it) are those over a fast one:
/// `loadedmetadata` waits until the byte source has settled (here: the
/// whole file), `loadeddata` until the data is enough.
#[test]
fn over_a_slow_link_the_events_in_virtual_time_are_those_of_a_fast_one() {
    on_page_thread(|| {
        // The events without their page times.
        let strip = |events: Value| -> Value {
            events
                .as_array()
                .unwrap()
                .iter()
                .map(|e| json!(e.as_str().unwrap().split(" t=").next().unwrap()))
                .collect()
        };
        let fast = strip(events_over(None, LONG_MP4, ClockMode::Virtual, 3));
        let link = (16384, Duration::from_millis(20));
        let slow = strip(events_over(Some(link), LONG_MP4, ClockMode::Virtual, 3));
        assert_eq!(slow, fast);
        let names: Vec<&str> = fast
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap().split(' ').next().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "loadstart",
                "progress",
                "suspend",
                "durationchange",
                "resize",
                "loadedmetadata",
                "loadeddata",
                "canplay",
                "canplaythrough"
            ],
            "{fast}"
        );
    });
}

/// Review S3 of feature 6a: in real time, `loadedmetadata` and `loadeddata`
/// come when the demuxer has opened and the first frame is decoded, while
/// a slow link (16 KiB every 100 ms: 1.9 s for the 300 KB file) still
/// loads, as Chromium fires them at the parse and the first frame (here at
/// about 0.3 s). `suspend` comes when the network goes idle, after them.
#[test]
fn over_a_slow_link_in_real_time_the_metadata_comes_before_the_network_settles() {
    on_page_thread(|| {
        let link = (16384, Duration::from_millis(100));
        let events = events_over(Some(link), LONG_MP4, ClockMode::Real, 4);
        let names: Vec<String> = events
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap().to_owned())
            .collect();
        let at = |prefix: &str| {
            names
                .iter()
                .position(|e| e.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix}: {names:?}"))
        };
        assert!(names[at("loadedmetadata")].contains(" ns=2 "), "{names:?}");
        assert!(at("loadeddata") < at("suspend"), "{names:?}");
        // `loadeddata` comes well before the whole file has arrived (1.9 s),
        // with the state `HAVE_ENOUGH_DATA`: the gates of virtual time (the
        // byte source settled, the data enough) do not hold it back. The
        // gate on enough data alone cannot be told apart here: the first
        // 64 KiB block of every fixture holds the first frame and over a
        // second of media, so the data is enough when the demuxer opens.
        let loadeddata = &names[at("loadeddata")];
        assert!(loadeddata.contains(" rs=4 "), "{names:?}");
        let time: u64 = loadeddata
            .rsplit("t=")
            .next()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| panic!("no time in {loadeddata}"));
        assert!(time < 1000, "loadeddata at {time} ms: {names:?}");
    });
}

/// The sample rate of [`wav`]: stereo 16-bit samples at this rate are 1 MiB
/// per second, the bit rate that the byte source assumes before the
/// metadata, so its read-ahead in bytes is the same before and after the
/// metadata (else it depends on when the metadata comes).
const WAV_RATE: u32 = 262_144;

/// A WAV file of `seconds` of silence (stereo, 16 bits, [`WAV_RATE`]).
fn wav(seconds: u32) -> Vec<u8> {
    let data = seconds * WAV_RATE * 4;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&WAV_RATE.to_le_bytes());
    out.extend_from_slice(&(WAV_RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.resize(44 + data as usize, 0);
    out
}

/// Review S5 of feature 6a (and mutation M8): a missing `preload` reads
/// ahead as `metadata` does, not as `auto`. Chromium
/// (`tests/data/media-preload-default.json`, the `real-preload-*` cases of
/// `tools/probes/media-real-files.json`, three equal runs): without the
/// attribute `preload` is "metadata" and the 4K file has under 10 s
/// buffered after 1.5 s, as with `metadata`; with `auto` over 10 s. In swb,
/// a 10 s WAV file of 10 MiB (1 MiB per second): `metadata` reads ahead 3 s
/// of the read position, `auto` 15 s (the whole file), so only `auto`
/// buffers it all.
#[test]
fn a_missing_preload_reads_ahead_as_metadata_does() {
    let chromium: Value =
        serde_json::from_str(include_str!("data/media-preload-default.json")).unwrap();
    let value = |name: &str| {
        chromium["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .map(|c| c["results"][0]["value"].clone())
            .unwrap()
    };
    let default = value("real-preload-default-mux-highest-mp4");
    assert_eq!(default, value("real-preload-metadata-mux-highest-mp4"));
    assert_ne!(
        default["buffered"],
        value("real-preload-auto-mux-highest-mp4")["buffered"]
    );
    on_page_thread(|| {
        let mut site = MediaSite::new(
            FOLDER,
            "<!DOCTYPE html><audio id=d src=a.wav?d></audio><audio id=m src=a.wav?m preload=metadata></audio><audio id=a src=a.wav?a preload=auto></audio>",
        );
        site.add_bytes("a.wav", wav(10));
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(3), Duration::from_secs(30));
        let got = evaluate(
            &mut page,
            "[d, m, a].map(e => [e.preload, e.readyState, e.buffered.length ? e.buffered.end(e.buffered.length - 1) : 0])",
        );
        let rows = got.as_array().unwrap();
        assert_eq!(rows[0][0], "metadata");
        assert_eq!(rows[0], rows[1], "{got}");
        assert_eq!(rows[2][1], 4, "{got}");
        assert_eq!(rows[2][2], 10, "{got}");
        assert!(rows[0][2].as_f64().unwrap() < 10.0, "{got}");
    });
}

/// Review N1 of feature 6a: the pointer of the resource selection in the
/// `source` children (HTML: insertions at the pointer go after it; when
/// the node before it is removed it does not move relative to the others).
/// `s0` is skipped (`media`), `s1` is fetched; at `loadstart` the script
/// inserts `n0` before `s0` (before the pointer: never tried), removes `s1`
/// (the node before the pointer) and inserts `n1` where it was (after the
/// pointer: tried next, and skipped). `s1` fails (404), `n1` is skipped,
/// `s2` loads.
#[test]
fn the_selection_pointer_keeps_its_place_when_sources_come_and_go() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>const EVO = [];</script>\
            <video id=v><source id=s0 src=a.mp4?s0 media='not all'><source id=s1 src=missing.mp4><source id=s2 src=a.mp4?s2></video><script>\
            for (const s of v.children) s.addEventListener('error', () => EVO.push('error@' + s.id));\
            v.addEventListener('loadstart', () => {\
              EVO.push('loadstart');\
              const n0 = document.createElement('source'); n0.id = 'n0'; n0.src = 'a.mp4?n0';\
              const n1 = document.createElement('source'); n1.id = 'n1'; n1.src = 'a.mp4?n1'; n1.media = 'not all';\
              n1.addEventListener('error', () => EVO.push('error@n1'));\
              v.insertBefore(n0, s0); s1.remove(); v.insertBefore(n1, s2);\
            });\
            v.addEventListener('loadedmetadata', () => EVO.push('loadedmetadata ' + v.currentSrc.split('/').pop()));\
            </script>";
        let site = site_with(html, &[("a.mp4", MP4)]);
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(&mut page, "EVO"),
            json!([
                "loadstart",
                "error@s0",
                "error@s1",
                "error@n1",
                "loadedmetadata a.mp4?s2"
            ])
        );
    });
}

/// The hostile media of `fixtures/media/` (ADR 0047 section 11): what the
/// element makes of them at load. Files that open load (their damage shows
/// when the media plays: feature 6b); the others give their error code.
#[test]
fn the_hostile_media_give_their_error_codes_at_load() {
    on_page_thread(|| {
        // (file, error code at load or 0, message).
        let table: &[(&str, u16, &str)] = &[
            (
                "garbage.mp4",
                4,
                "PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open context failed",
            ),
            (
                "mp4-h264-high-aac-no-moov.mp4",
                4,
                "PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open context failed",
            ),
            (
                "webm-vp9-16400x16.webm",
                4,
                "PipelineStatus::DEMUXER_ERROR_NO_SUPPORTED_STREAMS: FFmpegDemuxer: no supported streams",
            ),
            (
                "webm-vp9-8192x4416.webm",
                4,
                "PipelineStatus::DEMUXER_ERROR_NO_SUPPORTED_STREAMS: FFmpegDemuxer: no supported streams",
            ),
            ("mp4-h264-high-aac-truncated.mp4", 0, ""),
            ("mp4-h264-high-aac-corrupt.mp4", 0, ""),
            ("webm-vp8-vorbis-truncated.webm", 0, ""),
            ("webm-vp8-vorbis-corrupt.webm", 0, ""),
            ("mp3-truncated.mp3", 0, ""),
            ("mka-pcm-17-streams.mka", 0, ""),
        ];
        let mut html = String::from("<!DOCTYPE html>");
        let mut files = Vec::new();
        for (i, (file, _, _)) in table.iter().enumerate() {
            let audio = ["mp3", "mka"].contains(&file.rsplit('.').next().unwrap_or_default());
            let tag = if audio { "audio" } else { "video" };
            let _ = write!(html, "<{tag} id=h{i} src={file} preload=auto></{tag}>");
            files.push((*file, format!("fixtures/media/{file}")));
        }
        let mut site = MediaSite::new(FOLDER, &html);
        for (name, path) in &files {
            site.add_file(name, path);
        }
        let (mut page, _site) = page_for(site, false);
        page.run_event_loop(Duration::from_secs(3), Duration::from_secs(60));
        for (i, (file, code, message)) in table.iter().enumerate() {
            let n = node(&page, &format!("h{i}"));
            let info = page.media_info(n).unwrap();
            match (code, &info.error) {
                (0, None) => assert_eq!(
                    (info.network_state, info.ready_state),
                    (1, 4),
                    "{file}: loads"
                ),
                (code, Some(error)) if *code != 0 => {
                    assert_eq!(
                        (error.code, error.message.as_str()),
                        (*code, *message),
                        "{file}"
                    );
                    assert_eq!((info.network_state, info.ready_state), (3, 0), "{file}");
                }
                (code, error) => panic!("{file}: expected code {code}, got {error:?}"),
            }
        }
    });
}

#[test]
fn load_and_a_preload_change_start_the_load_that_preload_none_holds_back() {
    on_page_thread(|| {
        let site = site_with(
            "<!DOCTYPE html><video id=v src=a.mp4 preload=none></video>\
             <video id=w src=a.mp4 preload=none></video>\
             <video id=x src=a.mp4 preload=none></video>",
            &[("a.mp4", MP4)],
        );
        let (mut page, site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert!(site.log_lines().is_empty(), "nothing is fetched yet");
        assert_eq!(
            evaluate(
                &mut page,
                "[v, w, x].map(e => [e.readyState, e.networkState])"
            ),
            json!([[0, 1], [0, 1], [0, 1]])
        );
        // `preload` away from `none` starts the load; so does `load()`; a
        // `src` change under `none` does not.
        evaluate(
            &mut page,
            "v.preload = 'auto'; w.load(); x.src = 'a.mp4?2'; 1",
        );
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(
                &mut page,
                "[v, w, x].map(e => [e.readyState, e.networkState])"
            ),
            json!([[4, 1], [4, 1], [0, 1]])
        );
        assert_eq!(site.log_lines().len(), 2, "{:?}", site.log_lines());
    });
}

/// A script that sets `src` and reads nothing back: the events of the
/// synchronous part of the load algorithm (`abort`, `emptied`,
/// `timeupdate`) are delivered with `NETWORK_NO_SOURCE`, before `loadstart`
/// with `NETWORK_LOADING` (Chromium; the probe rows read the state first).
#[test]
fn a_src_that_nobody_reads_back_delivers_abort_before_the_selection_runs() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>const EVO = []; \
            function watch(m) { ['loadstart', 'abort', 'emptied', 'timeupdate', 'loadedmetadata'].forEach(n => m.addEventListener(n, () => EVO.push(n + ' ns=' + m.networkState + ' rs=' + m.readyState))); }\
            </script><video id=v src=a.mp4></video><script>watch(v)</script>";
        let site = site_with(html, &[("a.mp4", MP4), ("b.mp4", MP4)]);
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(evaluate(&mut page, "EVO.length"), json!(2));
        // A task that only assigns.
        evaluate(&mut page, "setTimeout(() => { v.src = 'b.mp4'; }, 10); 1");
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(&mut page, "EVO.slice(2)"),
            json!([
                "abort ns=3 rs=0",
                "emptied ns=3 rs=0",
                "timeupdate ns=3 rs=0",
                "loadstart ns=2 rs=0",
                "loadedmetadata ns=1 rs=1"
            ])
        );
    });
}

/// HTML "resource selection algorithm", children mode: when the candidates
/// are used up the element waits for a `source` that is inserted later, and
/// goes on with it without a second `loadstart`.
#[test]
fn a_source_inserted_while_the_element_waits_is_tried() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>const EVO = []; \
            function watch(m, tag) { ['loadstart', 'error', 'loadedmetadata', 'canplaythrough'].forEach(n => m.addEventListener(n, () => EVO.push(n + (tag ? '@' + tag : '') + ' ns=' + v.networkState))); }\
            </script><video id=v><source id=s1 src=missing.mp4></video><script>watch(v); watch(s1, 's1')</script>";
        let site = site_with(html, &[("a.mp4", MP4)]);
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(&mut page, "[EVO, v.networkState, v.readyState]"),
            json!([["loadstart ns=2", "error@s1 ns=3"], 3, 0])
        );
        evaluate(
            &mut page,
            "const s = document.createElement('source'); s.src = 'a.mp4'; v.appendChild(s); 1",
        );
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(
                &mut page,
                "[EVO.slice(2), v.networkState, v.readyState, v.currentSrc.slice(-5)]"
            ),
            json!([
                ["loadedmetadata ns=1", "canplaythrough ns=1"],
                1,
                4,
                "a.mp4"
            ])
        );
    });
}

#[test]
fn the_media_error_is_one_object_until_the_engine_records_a_new_one() {
    on_page_thread(|| {
        let site = site_with("<!DOCTYPE html><video id=v src=missing.mp4></video>", &[]);
        let (mut page, _site) = page_for(site, true);
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(
                &mut page,
                "[v.error === v.error, v.error.code, v.error.message, v.error instanceof MediaError, v.error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED, v.networkState]"
            ),
            json!([true, 4, "MEDIA_ELEMENT_ERROR: Format error", true, true, 3])
        );
        evaluate(
            &mut page,
            "globalThis.first = v.error; v.src = 'also-missing.mp4'; 1",
        );
        // The load algorithm clears the error at once.
        assert_eq!(evaluate(&mut page, "v.error"), json!(null));
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(30));
        assert_eq!(
            evaluate(
                &mut page,
                "[v.error !== null, v.error !== first, first.code]"
            ),
            json!([true, true, 4])
        );
    });
}

/// The events of the load of `a.mp4` in one page (JavaScript on, virtual
/// time), as the probe's `evs()` shows them.
fn events_of_a_load() -> Value {
    let html = "<!DOCTYPE html><script>\
        const EVO = [];\
        function watch(m) { ['loadstart','progress','suspend','abort','error','emptied','durationchange','resize','loadedmetadata','loadeddata','canplay','canplaythrough'].forEach(n => m.addEventListener(n, () => EVO.push(n + ' rs=' + m.readyState + ' ns=' + m.networkState + ' d=' + m.duration + ' buf=' + m.buffered.length))); }\
        </script><video id=v src=a.mp4 preload=metadata></video><audio id=a src=b.mp3 preload=auto></audio><script>watch(v); watch(a)</script>";
    let site = site_with(html, &[("a.mp4", MP4), ("b.mp3", "fixtures/media/mp3.mp3")]);
    let (mut page, _site) = page_for(site, true);
    page.run_event_loop(Duration::from_secs(3), Duration::from_secs(60));
    evaluate(&mut page, "EVO")
}

/// A run gives the same events and values on every run, also under load
/// (twelve pages at once, three times, and a busy thread per core).
#[test]
fn the_events_are_the_same_on_every_run_also_under_load() {
    let alone = on_page_thread(events_of_a_load);
    assert_eq!(alone.as_array().unwrap().len(), 17, "{alone}");
    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism().map_or(4, usize::from);
    let busy: Vec<_> = (0..cores)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 1u64;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    std::hint::black_box(x);
                }
            })
        })
        .collect();
    // Three rounds of twelve pages.
    let runs: Vec<Value> = (0..3)
        .flat_map(|_| on_page_threads((0..12).map(|_| events_of_a_load)))
        .collect();
    stop.store(true, Ordering::Relaxed);
    for thread in busy {
        thread.join().unwrap();
    }
    for run in runs {
        assert_eq!(run, alone);
    }
}
