//! M14 feature 7a (ADR 0047 sections 6 and 9) with the feature `media`: what
//! the probe rows do not decide about the frames of a `<video>` in paint. The
//! frame is the image of the element, a new frame damages the video's box and
//! leaves the layout and the display list alone, the frame at the media time
//! of the page clock is the same on every run (also with several pages and
//! busy threads), a video without a box converts no frame and shows the frame
//! at its position when it gets one, and the default poster covers only a video
//! without a frame. Run by `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::media_site::MediaSite;
use common::{new_page, node, on_page_thread, on_page_threads};
use serde_json::Value;
use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Pixmap, Size, Url};
use swb_net::Fetcher;

const FOLDER: &str = "case-1";
const MP4: &str = "fixtures/media/mp4-h264-high-aac.mp4";

fn page_for(html: &str) -> (Page, Arc<MediaSite>) {
    let mut site = MediaSite::new(FOLDER, html);
    site.add_file("a.mp4", MP4);
    let site = Arc::new(site);
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    (page, site)
}

fn evaluate(page: &mut Page, js: &str) -> Value {
    match page.evaluate_with_timeout(js, JsonForm::Strict, Duration::from_secs(60)) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{js}: {other:?}"),
    }
}

const HELPERS: &str = "<script>\
    const sleep = ms => new Promise(r => setTimeout(r, ms));\
    const once = (m, n, ms = 8000) => new Promise(r => { const to = setTimeout(() => r('timeout'), ms); m.addEventListener(n, () => { clearTimeout(to); r(n); }, {once: true}); });\
    </script>";

/// `[r, g, b]` of the pixel `(x, y)` of a screenshot (opaque pages).
fn rgb(shot: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = shot.pixel(x, y).unwrap().demultiply();
    [p.red(), p.green(), p.blue()]
}

/// The bytes of a screenshot, for equality.
fn bytes(shot: &Pixmap) -> Vec<u8> {
    shot.data().to_vec()
}

/// A page with a muted video at the top left that plays (320 x 240), after
/// `ready`.
const PLAYING: &str = "<video id=v src=a.mp4 preload=auto muted loop></video>";

/// Waits for the data of `#v`.
const READY: &str =
    "(async () => { if (v.readyState < 2) await once(v, 'loadeddata'); return v.readyState; })()";

/// Runs `f` while a thread per core keeps the machine busy.
fn with_busy_threads<T>(f: impl FnOnce() -> T) -> T {
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
    let out = f();
    stop.store(true, Ordering::Relaxed);
    for thread in busy {
        thread.join().unwrap();
    }
    out
}

/// The screenshot of a playing video after `ms` of page time of playback.
fn playing_shot(ms: u64) -> Pixmap {
    let (mut page, _site) = page_for(&format!("<!DOCTYPE html>{HELPERS}{PLAYING}"));
    evaluate(&mut page, READY);
    evaluate(&mut page, "v.play().then(() => 1)");
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(60));
    page.screenshot(false).unwrap()
}

/// A new frame is a new generation of the video's image: the frame is
/// outdated, the damage of the next frame is the video's box, and the layout
/// and the display list stay (ADR 0047 section 9, "The display list does not
/// change when a frame changes").
#[test]
fn a_new_frame_damages_the_video_only_and_keeps_the_layout() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}\
            video{{display:block;margin:100px 0 0 200px}}</style>{HELPERS}{PLAYING}"
        );
        let (mut page, _site) = page_for(&html);
        evaluate(&mut page, READY);
        page.update_layout();
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        assert!(!page.frame_outdated());
        let layouts = page.flush_counters().layout_runs;
        evaluate(&mut page, "v.play().then(() => 1)");
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(60));
        assert!(page.frame_outdated(), "a new frame outdates the frame");
        let before = bytes(&target);
        let damage = page.render_update(&mut target);
        assert!(!damage.full, "{damage:?}");
        // The video's box, grown by a pixel on each side (the damage snaps
        // outwards).
        assert!(
            damage.pixels() > 0 && damage.pixels() <= 322 * 242,
            "{damage:?}"
        );
        // The pixels changed in the box, and only there.
        let after = bytes(&target);
        assert_ne!(before, after);
        let (mut inside, mut outside) = (0, 0);
        for (i, (a, b)) in before.chunks(4).zip(after.chunks(4)).enumerate() {
            if a != b {
                let (x, y) = ((i % 800) as u32, (i / 800) as u32);
                if (200..520).contains(&x) && (100..340).contains(&y) {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
        assert!(inside > 1000, "{inside} pixels changed in the box");
        assert_eq!(outside, 0);
        assert_eq!(
            page.flush_counters().layout_runs,
            layouts,
            "a new frame needs no layout"
        );
        assert!(!page.frame_outdated());
    });
}

/// The screenshot shows the frame at the media time of the page clock, and
/// a paused video shows the frame at its position: playing for 440 ms shows
/// the frame that a seek to its `currentTime` shows.
#[test]
fn a_screenshot_shows_the_frame_at_the_media_time_of_the_page_clock() {
    on_page_thread(|| {
        let playing = playing_shot(600);
        let early = playing_shot(100);
        assert_ne!(bytes(&playing), bytes(&early));
        // The same time, paused: seek to where the playing page was.
        let (mut page, _site) = page_for(&format!("<!DOCTYPE html>{HELPERS}{PLAYING}"));
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        page.run_event_loop(Duration::from_millis(600), Duration::from_secs(60));
        let t = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        evaluate(&mut page, "v.pause(); 1");
        let paused = page.screenshot(false).unwrap();
        assert_eq!(bytes(&paused), bytes(&playing), "paused at {t}");
        // And a seek to the same time on a page that never played.
        let (mut page, _site) = page_for(&format!("<!DOCTYPE html>{HELPERS}{PLAYING}"));
        evaluate(&mut page, READY);
        evaluate(
            &mut page,
            &format!(
                "(async () => {{ v.currentTime = {t}; await once(v, 'seeked'); return 1; }})()"
            ),
        );
        assert_eq!(bytes(&page.screenshot(false).unwrap()), bytes(&playing));
    });
}

/// In virtual time the pipeline has settled before a screenshot: the same
/// pixels on every run, with several pages at once and busy threads.
#[test]
fn screenshots_of_playing_videos_are_the_same_on_every_run_also_under_load() {
    let want = on_page_thread(|| bytes(&playing_shot(730)));
    for _round in 0..3 {
        let runs =
            with_busy_threads(|| on_page_threads((0..8).map(|_| || bytes(&playing_shot(730)))));
        for (i, run) in runs.iter().enumerate() {
            assert!(*run == want, "page {i} differs");
        }
    }
}

/// A video without a box converts no frame (`display: none`: its first
/// layout says so), keeps its clock, and shows the frame at its position
/// when it gets a box (the backlog item of feature 5: a paused video had no
/// image until the next frame or a seek).
#[test]
fn a_hidden_video_converts_no_frame_and_shows_its_frame_when_it_gets_a_box() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
            {HELPERS}<video id=v src=a.mp4 preload=auto muted loop style='display:none'></video>"
        );
        let (mut page, _site) = page_for(&html);
        // The layout before the pipeline opens: the element has no box.
        page.update_layout();
        evaluate(&mut page, READY);
        let v = node(&page, "v");
        // Paused at 0.44 s while hidden: the frame is decoded, not converted.
        evaluate(
            &mut page,
            "(async () => { v.currentTime = 0.44; await once(v, 'seeked'); return 1; })()",
        );
        assert_eq!(page.media_frames_converted(v), Some(0));
        // Playing hidden: the clock goes on, no frame is converted.
        evaluate(&mut page, "v.play().then(() => 1)");
        page.run_event_loop(Duration::from_millis(800), Duration::from_secs(60));
        page.update_layout();
        assert_eq!(page.media_frames_converted(v), Some(0));
        let t = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(t > 1.0, "the hidden video plays on: {t}");
        evaluate(&mut page, "v.pause(); 1");
        // It gets a box: the frame at its position shows (the screenshot waits
        // for the conversion in virtual time).
        evaluate(&mut page, "v.style.display = 'block'; 1");
        let shown = page.screenshot(false).unwrap();
        assert!(page.media_frames_converted(v).unwrap() >= 1);
        // The same frame, from a page whose video had a box all along.
        let (mut page, _site) = page_for(&format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
            {HELPERS}<video id=v src=a.mp4 preload=auto muted loop></video>"
        ));
        evaluate(&mut page, READY);
        evaluate(
            &mut page,
            &format!(
                "(async () => {{ v.currentTime = {t}; await once(v, 'seeked'); return 1; }})()"
            ),
        );
        let visible = page.screenshot(false).unwrap();
        assert_eq!(bytes(&shown), bytes(&visible), "the frame at {t}");
        // It is a frame, not the page background.
        assert_ne!(rgb(&shown, 10, 10), [204, 204, 204]);
    });
}

/// A paused video that was hidden after it showed a frame shows the frame
/// again when it is shown: the pipeline's quiet seek brings the pixels back.
#[test]
fn a_paused_video_that_is_hidden_and_shown_again_keeps_its_frame() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
            {HELPERS}{PLAYING}"
        );
        let (mut page, _site) = page_for(&html);
        evaluate(&mut page, READY);
        evaluate(
            &mut page,
            "(async () => { v.currentTime = 0.44; await once(v, 'seeked'); return 1; })()",
        );
        let first = bytes(&page.screenshot(false).unwrap());
        let v = node(&page, "v");
        evaluate(&mut page, "v.style.display = 'none'; 1");
        let hidden = bytes(&page.screenshot(false).unwrap());
        assert_ne!(first, hidden);
        // Hidden and then shown, with a seek while hidden (its frame is not
        // converted) and back.
        evaluate(
            &mut page,
            "(async () => { v.currentTime = 1.2; await once(v, 'seeked'); v.currentTime = 0.44; await once(v, 'seeked'); return 1; })()",
        );
        let converted = page.media_frames_converted(v).unwrap();
        evaluate(&mut page, "v.style.display = 'block'; 1");
        let shown = bytes(&page.screenshot(false).unwrap());
        assert_eq!(first, shown);
        assert!(page.media_frames_converted(v).unwrap() > converted);
    });
}

/// The default poster (#333) of a video with controls and no `poster`
/// attribute shows until a frame comes; with a frame the letterbox is
/// transparent (measured in Chromium, `frames-controls-letterbox`).
#[test]
fn the_default_poster_covers_a_video_without_a_frame() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}\
            video{{display:block;width:400px;height:200px;object-fit:contain}}</style>\
            {HELPERS}<video id=v src=a.mp4 preload=none muted controls></video>"
        );
        let (mut page, _site) = page_for(&html);
        // Nothing loads under `preload=none`: the default poster.
        let shot = page.screenshot(false).unwrap();
        assert_eq!(rgb(&shot, 20, 60), [0x33, 0x33, 0x33]);
        evaluate(&mut page, "v.load(); 1");
        evaluate(&mut page, READY);
        let shot = page.screenshot(false).unwrap();
        // The frame in the middle, the page background at the sides.
        assert_eq!(rgb(&shot, 20, 60), [204, 204, 204]);
        assert_ne!(rgb(&shot, 100, 60), [204, 204, 204]);
        assert_ne!(rgb(&shot, 100, 60), [0x33, 0x33, 0x33]);
    });
}

/// A poster shows until `play()` (the show poster flag); the frame shows
/// after it, also when the video is paused again.
#[test]
fn the_poster_shows_until_play_and_the_frame_after_it() {
    on_page_thread(|| {
        let mut site = MediaSite::new(
            FOLDER,
            &format!(
                "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
                {HELPERS}<video id=v src=a.mp4 poster=p.png preload=auto muted></video>"
            ),
        );
        site.add_file("a.mp4", MP4);
        site.add_file("p.png", "fixtures/media/poster-320x240.png");
        let site = Arc::new(site);
        let mut page = new_page(
            Arc::clone(&site) as Arc<dyn Fetcher>,
            2,
            Size::new(800.0, 600.0),
        );
        page.set_javascript(true);
        page.set_clock(PageClock::new(ClockMode::Virtual, None));
        page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        evaluate(&mut page, READY);
        let poster = page.screenshot(false).unwrap();
        // The poster's flat color (49, 100, 151), not a frame.
        assert_eq!(rgb(&poster, 100, 60), [49, 100, 151]);
        evaluate(
            &mut page,
            "(async () => { v.play(); await once(v, 'playing'); await sleep(100); v.pause(); return 1; })()",
        );
        let frame = page.screenshot(false).unwrap();
        assert_ne!(rgb(&frame, 100, 60), [49, 100, 151]);
        assert_ne!(rgb(&frame, 10, 10), [49, 100, 151]);
        // `load()` shows the poster again.
        evaluate(
            &mut page,
            "(async () => { v.load(); await once(v, 'canplaythrough'); return 1; })()",
        );
        let again = page.screenshot(false).unwrap();
        assert_eq!(bytes(&again), bytes(&poster));
    });
}

/// A page of `site` in the clock mode `mode`.
fn page_of(site: MediaSite, mode: ClockMode) -> Page {
    let site = Arc::new(site);
    let mut page = new_page(site as Arc<dyn Fetcher>, 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.set_clock(PageClock::new(mode, None));
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The page of a poster video: `video` is the element's markup.
fn poster_site(video: &str) -> MediaSite {
    let mut site = MediaSite::new(
        FOLDER,
        &format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
            {HELPERS}{video}"
        ),
    );
    site.add_file("a.mp4", MP4);
    site.add_file("g.mp4", "fixtures/media/garbage.mp4");
    site.add_file("p.png", "fixtures/media/poster-320x240.png");
    site
}

/// The poster's flat color in the probe rows (`poster-320x240.png`).
const POSTER: [u8; 3] = [49, 100, 151];

/// While the current load has no frame, a loaded poster shows also when
/// `play()` cleared the show poster flag (Finding 1 of the review of 7a;
/// Chromium rows `poster-play-no-src`, `poster-play-garbage-file`): a video
/// without a source, a video whose file fails, and a video whose first frame
/// is slow to come (a slow link in real time: the poster shows between
/// `play()` and the frame).
#[test]
fn a_loaded_poster_shows_after_play_until_the_video_has_a_frame() {
    on_page_thread(|| {
        // No source: `play()` waits for one, nothing ever comes.
        let site = poster_site("<video id=v poster=p.png width=320 height=240></video>");
        let mut page = page_of(site, ClockMode::Virtual);
        evaluate(&mut page, "v.play(); 1");
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(60));
        let shot = page.screenshot(false).unwrap();
        assert_eq!(rgb(&shot, 100, 60), POSTER, "no src");
        // A file that fails to open.
        let site = poster_site(
            "<video id=v poster=p.png src=g.mp4 width=320 height=240 preload=none muted></video>",
        );
        let mut page = page_of(site, ClockMode::Virtual);
        evaluate(&mut page, "v.play(); 1");
        page.run_event_loop(Duration::from_millis(800), Duration::from_secs(60));
        assert_eq!(evaluate(&mut page, "v.error.code").as_i64(), Some(4));
        let shot = page.screenshot(false).unwrap();
        assert_eq!(rgb(&shot, 100, 60), POSTER, "failed file");
        // A slow first frame, in real time: the poster, then the frame.
        let mut site = poster_site(
            "<video id=v poster=p.png src=a.mp4 width=320 height=240 preload=none muted></video>",
        );
        site.slow = Some((4096, Duration::from_millis(300)));
        let mut page = page_of(site, ClockMode::Real);
        page.render(&mut Pixmap::new(800, 600).unwrap());
        evaluate(&mut page, "v.play(); 1");
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        assert_eq!(rgb(&target, 100, 60), POSTER, "right after play()");
        // The frame comes (a bound, not a fixed wait).
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while rgb(&target, 100, 60) == POSTER && std::time::Instant::now() < deadline {
            page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
            page.render(&mut target);
        }
        assert_ne!(rgb(&target, 100, 60), POSTER, "the frame came");
        assert_ne!(rgb(&target, 100, 60), [204, 204, 204], "not a blank box");
    });
}

/// A render in real time polls the pipelines at the page clock: with no
/// other step in between (no `pump`), a render after the media time passed
/// a frame shows the newer frame (mutation m14 of the review of 7a: no
/// `sync_media_frames` in `render_frame`).
#[test]
fn a_render_in_real_time_shows_the_frame_that_the_clock_reached() {
    on_page_thread(|| {
        let mut page = page_of(
            poster_site(&format!(
                "<style>video{{margin:100px 0 0 200px}}</style>{PLAYING}"
            )),
            ClockMode::Real,
        );
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        let first = bytes(&target);
        // Only renders from here: no event loop, no events, no steps.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut changed = false;
        while !changed && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            page.render(&mut target);
            changed = bytes(&target) != first;
        }
        assert!(changed, "no newer frame in 10 s of renders");
    });
}

/// A screenshot settles the media without running the page: a page whose
/// `loadstart` listener reloads the video from a timer keeps the media
/// pending, and the screenshot returns in bounded time with the page's
/// timers not run (Finding 2 of the review of 7a: it took 30 s and ran the
/// page's tasks).
#[test]
fn a_screenshot_runs_no_page_task_and_ends_in_bounded_time() {
    on_page_thread(|| {
        let mut site = MediaSite::new(
            FOLDER,
            &format!(
                "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
                <script>let reloads = 0, loads = 0;\
                v.addEventListener('loadstart', () => {{ loads++; setTimeout(() => {{ reloads++; v.load(); }}, 0); }});\
                </script>"
            ),
        );
        site.add_file("a.mp4", MP4);
        let mut page = page_of(site, ClockMode::Virtual);
        let reloads = |page: &mut Page| evaluate(page, "reloads");
        let before = reloads(&mut page);
        let started = std::time::Instant::now();
        page.screenshot(false).unwrap();
        let took = started.elapsed();
        // The media events that the steps queue are delivered (a step waits
        // for them), so `loadstart` listeners run; no timer does.
        assert_eq!(reloads(&mut page), before, "the screenshot ran a timer");
        assert!(took < Duration::from_secs(10), "{took:?}");
        // A listener that reloads the video from its own event: the steps
        // and the time bound the settle.
        let mut site = MediaSite::new(
            FOLDER,
            &format!(
                "<!DOCTYPE html>{HELPERS}<video id=v src=a.mp4 preload=auto muted></video>\
                <script>let loads = 0; v.addEventListener('loadstart', () => {{ loads++; v.load(); }});</script>"
            ),
        );
        site.add_file("a.mp4", MP4);
        let mut page = page_of(site, ClockMode::Virtual);
        let started = std::time::Instant::now();
        page.screenshot(false).unwrap();
        let took = started.elapsed();
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert!(evaluate(&mut page, "loads").as_i64() > Some(10));
    });
}
