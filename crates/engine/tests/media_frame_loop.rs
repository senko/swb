//! M14 feature 7b (ADR 0047 section 9 and the notes of feature 7b) with the
//! feature `media`: the frame loop of the GUI without a window. A page with
//! `set_paints_frames(true)` in real time, driven as the GUI drives it
//! (`about_to_wait`: `pump`, the frame decision `frame_outdated`, a render,
//! then a sleep until `next_task_instant`): a playing video makes the frame
//! outdated at its frame times and only then; a paused, ended, stalled or not
//! painted video draws nothing and schedules no frame wake-ups; a frame that is
//! late does not spin the loop; and the painted flag follows the viewport
//! (with a margin), `visibility`, `content-visibility`, the frame documents and
//! the hidden window. Run by `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::media_site::MediaSite;
use common::{new_page, node, on_page_thread};
use serde_json::{Value, json};
use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Pixmap, Point, Size, Url};
use swb_net::Fetcher;

const FOLDER: &str = "case-1";
/// 320 x 240, 25 fps, 2 s, H.264 and AAC.
const MP4: &str = "fixtures/media/mp4-h264-high-aac.mp4";
const VIEW: Size = Size::new(800.0, 600.0);

const HELPERS: &str = "<script>\
    const sleep = ms => new Promise(r => setTimeout(r, ms));\
    const once = (m, n, ms = 8000) => new Promise(r => { const to = setTimeout(() => r('timeout'), ms); m.addEventListener(n, () => { clearTimeout(to); r(n); }, {once: true}); });\
    </script>";

/// Waits for the data of `#v`.
const READY: &str =
    "(async () => { if (v.readyState < 2) await once(v, 'loadeddata'); return v.readyState; })()";

/// A page in real time that paints frames, as the GUI's page does.
fn page_of(site: MediaSite) -> Page {
    let site = Arc::new(site);
    let mut page = new_page(site as Arc<dyn Fetcher>, 2, VIEW);
    page.set_javascript(true);
    page.set_paints_frames(true);
    page.set_clock(PageClock::new(ClockMode::Real, None));
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

fn site_with(body: &str) -> MediaSite {
    let mut site = MediaSite::new(
        FOLDER,
        &format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#ccc}}video{{display:block}}</style>\
            {HELPERS}{body}"
        ),
    );
    site.add_file("a.mp4", MP4);
    site
}

fn evaluate(page: &mut Page, js: &str) -> Value {
    match page.evaluate_with_timeout(js, JsonForm::Strict, Duration::from_secs(60)) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{js}: {other:?}"),
    }
}

/// What a drive of the loop did.
#[derive(Debug, Default)]
struct Run {
    /// Frames drawn.
    renders: usize,
    /// Different pictures among them.
    distinct: usize,
    /// Due times of the page that the loop woke up for (each time once).
    wakeups: usize,
    /// Iterations where a due time had passed and nothing changed: the loop
    /// would have run again at once (a busy loop).
    spins: usize,
    /// Frames drawn with their damage, in device pixels.
    pixels: Vec<u64>,
}

fn hash_of(data: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Drives the page for `duration` as `about_to_wait` does: the pump, the
/// frame decision, a render of the damage if the frame is outdated, then a
/// wait until the page's next due time (at most 5 ms: the test polls the
/// network where the window would be woken).
fn drive(page: &mut Page, target: &mut Pixmap, duration: Duration) -> Run {
    let end = Instant::now() + duration;
    let mut run = Run::default();
    let mut seen = HashSet::new();
    // The due time that the loop waits for; a wake-up is the moment it
    // passes.
    let mut armed: Option<Instant> = None;
    while Instant::now() < end {
        page.process_network();
        if armed.is_some_and(|t| t <= Instant::now()) {
            run.wakeups += 1;
            armed = None;
        }
        let pumped = page.pump();
        let outdated = page.frame_outdated();
        if outdated {
            let damage = page.render_update(target);
            run.renders += 1;
            run.pixels.push(damage.pixels());
            seen.insert(hash_of(target.data()));
        }
        let now = Instant::now();
        let next = page.next_task_instant();
        match next {
            Some(t) if t <= now => {
                // Due already: the loop would run again at once.
                if !pumped.changed && !outdated {
                    run.spins += 1;
                } else if armed.is_none() {
                    run.wakeups += 1;
                }
            }
            Some(t) => {
                armed = Some(t);
                std::thread::sleep((t - now).min(Duration::from_millis(5)));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    run.distinct = seen.len();
    run
}

/// A page with the video `body` after the first frame of the video is there
/// and a first render was done.
fn start(body: &str) -> (Page, Pixmap) {
    let mut page = page_of(site_with(body));
    evaluate(&mut page, READY);
    let mut target = Pixmap::new(800, 600).unwrap();
    page.render(&mut target);
    (page, target)
}

/// The frames that a video converted since `before`.
fn converted_since(page: &Page, before: u64) -> u64 {
    page.media_counters().converted - before
}

/// A playing video makes the frame outdated at its frame times, and the loop
/// wakes up for them and for the `timeupdate` points only: 2 s of a 25 fps
/// video are about 50 frames, 50 different pictures, and not many more
/// wake-ups.
#[test]
fn a_playing_video_draws_its_frames_at_their_times() {
    on_page_thread(|| {
        let (mut page, mut target) = start(
            "<video id=v src=a.mp4 preload=auto muted loop style='margin:100px 0 0 200px'></video>",
        );
        evaluate(&mut page, "v.play().then(() => 1)");
        let run = drive(&mut page, &mut target, Duration::from_secs(2));
        assert!((42..=58).contains(&run.renders), "{run:?}");
        assert!(run.distinct >= 40, "{run:?}");
        // The frames, the 8 `timeupdate` points, and a little more.
        assert!(run.wakeups <= run.renders + 14, "{run:?}");
        assert!(run.spins <= 4, "{run:?}");
        // Each frame draws the video's box, not the page.
        let video = 322 * 242;
        assert!(
            run.pixels.iter().all(|&p| p > 0 && p <= video),
            "{:?}",
            run.pixels
        );
    });
}

/// The frame decision does not wait for a poll: with no `pump` and no render
/// between, the frame is outdated once a converted frame is due (mutation:
/// without the check in `frame_outdated`, only a poll that something else
/// ran would bring the frame).
#[test]
fn the_frame_decision_sees_a_due_frame_without_a_poll() {
    on_page_thread(|| {
        let (mut page, mut target) =
            start("<video id=v src=a.mp4 preload=auto muted loop></video>");
        evaluate(&mut page, "v.play().then(() => 1)");
        page.render_update(&mut target);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut outdated = false;
        // Only reads from here: no pump, no render.
        while !outdated && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            outdated = page.frame_outdated();
        }
        assert!(outdated, "no frame became due in 10 s");
        page.render_update(&mut target);
        assert!(!page.frame_outdated() || Instant::now() > deadline);
    });
}

/// A paused video and an ended one draw nothing and schedule no wake-up in
/// the next seconds (the only due time is the suspend after 15 s).
#[test]
fn a_paused_or_ended_video_schedules_no_frame_wakeups() {
    on_page_thread(|| {
        let (mut page, mut target) = start(
            "<video id=v src=a.mp4 preload=auto muted style='margin:100px 0 0 200px'></video>",
        );
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, &mut target, Duration::from_millis(500));
        evaluate(&mut page, "v.pause(); 1");
        // The pause and its events settle.
        drive(&mut page, &mut target, Duration::from_millis(300));
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert_eq!((run.renders, run.wakeups, run.spins), (0, 0, 0), "{run:?}");
        let due = page.next_task_instant();
        assert!(
            due.is_none_or(|t| t > Instant::now() + Duration::from_secs(5)),
            "{due:?}"
        );
        // Ended: play to the end (2 s).
        evaluate(
            &mut page,
            "v.currentTime = v.duration - 0.3; v.play().then(() => 1)",
        );
        let ended = evaluate(
            &mut page,
            "(async () => { if (!v.ended) await once(v, 'ended', 5000); return v.ended; })()",
        );
        assert_eq!(ended, json!(true));
        drive(&mut page, &mut target, Duration::from_millis(300));
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert_eq!((run.renders, run.wakeups, run.spins), (0, 0, 0), "{run:?}");
    });
}

/// A video whose data arrives slowly stalls (`waiting`): the clock stops, and
/// the loop neither spins nor draws while it waits; the frames come when the
/// data does.
#[test]
fn a_video_that_waits_for_data_does_not_spin() {
    on_page_thread(|| {
        let mut site = site_with("<video id=v src=a.mp4 preload=none muted loop></video>");
        site.slow = Some((2048, Duration::from_millis(250)));
        let mut page = page_of(site);
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        evaluate(&mut page, "v.play(); 1");
        let run = drive(&mut page, &mut target, Duration::from_secs(4));
        assert!(run.spins <= 4, "{run:?}");
        // Frames drawn are the frames that came: never more than the
        // video's own 25 per second, and the loop's wake-ups stay bounded.
        assert!(run.renders <= 110, "{run:?}");
        assert!(run.wakeups <= 160, "{run:?}");
    });
}

/// A tall page: the video lies `top` px down.
fn tall(top: u32) -> String {
    format!(
        "<div style='height:{top}px'></div>\
        <video id=v src=a.mp4 preload=auto muted loop></video><div style='height:3000px'></div>"
    )
}

/// A video far below the viewport (outside its margin) converts no frame and
/// draws nothing; it converts and draws when the page scrolls to it (the
/// viewport rule of the GUI; a page without painted frames has none).
#[test]
fn a_video_outside_the_viewport_converts_no_frames() {
    on_page_thread(|| {
        let (mut page, mut target) = start(&tall(3000));
        evaluate(&mut page, "v.play().then(() => 1)");
        // The first layout and the frames of the first moments.
        drive(&mut page, &mut target, Duration::from_millis(600));
        let before = page.media_counters();
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert_eq!(page.media_counters().converted, before.converted, "{run:?}");
        assert_eq!(run.renders, 0, "{run:?}");
        // The loop wakes for the frames in batches (every 3rd frame, about 8
        // a second) and the 4 `timeupdate` points, not for each frame (25
        // wake-ups a second); the frames passed over are no drops (M14
        // feature 7b, `Pipeline::next_wake`).
        assert!(run.wakeups <= 18, "{run:?}");
        assert_eq!(page.media_counters().dropped, before.dropped, "{run:?}");
        // Scrolled so that the video is in view.
        page.scroll_to(Point::new(0.0, 2800.0));
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        // The 6 frames that the pipeline queued while the video was not
        // painted have no pixels: about 19 of the 25 frames convert.
        assert!(converted_since(&page, before) >= 12, "{run:?}");
        assert!(run.renders >= 12, "{run:?}");
    });
}

/// A looping video draws on after its wrap, which is a seek while playing:
/// the video thread dropped the frames after the target frame as late (it
/// compared them with the position from before the seek, the end of the file),
/// and the picture froze for the length of the file (found in 7b: 31 of 50
/// frames dropped, the picture still for 1.3 s). The file is 2 s long; the
/// wrap is at 2 s, in the second drive.
#[test]
fn a_looping_video_goes_on_after_its_wrap() {
    on_page_thread(|| {
        let (mut page, mut target) = start(&tall(0));
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, &mut target, Duration::from_millis(1800));
        let dropped = page.media_counters().dropped;
        let run = drive(&mut page, &mut target, Duration::from_millis(1400));
        // 35 frames in 1.4 s, less the gap of the seek.
        assert!(run.renders >= 25, "{run:?}");
        assert!(page.media_counters().dropped - dropped <= 3);
    });
}

/// A video that scrolls into view shows its frame in the first render where
/// it is visible, or the next one (the margin is one viewport): the page
/// scrolls 60 px per render at 25 renders per second (1,500 px/s).
#[test]
fn a_video_that_scrolls_in_shows_its_frame_when_it_comes_into_view() {
    on_page_thread(|| {
        let (mut page, mut target) = start(&tall(1800));
        evaluate(&mut page, "v.play().then(() => 1)");
        // The flag is off a moment after the first layout.
        drive(&mut page, &mut target, Duration::from_millis(500));
        let converted_at_rest = page.media_counters().converted;
        let mut visible_at = None;
        let mut current_at = None;
        for step in 0..60 {
            let top = 60.0 * (step + 1) as f32;
            page.scroll_to(Point::new(0.0, top));
            let started = Instant::now();
            page.pump();
            page.render_update(&mut target);
            // The video's box (320 x 240 at y = 1800) against the viewport:
            // its first 10 rows are in the viewport.
            let rows = 1800.0 - top;
            if rows < 590.0 && visible_at.is_none() {
                visible_at = Some(step);
            }
            if rows < 590.0 && rows > -230.0 {
                let y = (rows.max(0.0) + 5.0) as u32;
                let pixel = target.pixel(160, y).unwrap().demultiply();
                let shows_frame = (pixel.red(), pixel.green(), pixel.blue()) != (204, 204, 204);
                // A frame that was converted after the flag turned on (the
                // margin), not the old one that the pipeline kept.
                let converted = page.media_counters().converted - converted_at_rest;
                if shows_frame && converted >= 3 && current_at.is_none() {
                    current_at = Some(step);
                }
            }
            std::thread::sleep(Duration::from_millis(40).saturating_sub(started.elapsed()));
        }
        let (visible, current) = (visible_at.unwrap(), current_at.expect("a current frame"));
        eprintln!("scroll-in: visible at render {visible}, its current frame at {current}");
        assert!(
            current <= visible + 1,
            "visible at render {visible}, its current frame at {current}"
        );
    });
}

/// `visibility: hidden` and the skipped contents of `content-visibility:
/// hidden` stop the conversion; a video that is shown again converts again.
#[test]
fn a_hidden_video_converts_no_frames_and_a_shown_one_does() {
    on_page_thread(|| {
        let (mut page, mut target) =
            start("<div id=d><video id=v src=a.mp4 preload=auto muted loop></video></div>");
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, &mut target, Duration::from_millis(300));
        for (hide, show, label) in [
            (
                "v.style.visibility = 'hidden'",
                "v.style.visibility = ''",
                "visibility",
            ),
            (
                "d.style.contentVisibility = 'hidden'; d.style.contain = 'size'; d.style.height = '300px'",
                "d.style.contentVisibility = ''; d.style.contain = ''; d.style.height = ''",
                "content-visibility",
            ),
        ] {
            evaluate(&mut page, &format!("{hide}; 1"));
            // The style change, the layout and the first frames.
            drive(&mut page, &mut target, Duration::from_millis(400));
            let before = page.media_counters().converted;
            let run = drive(&mut page, &mut target, Duration::from_millis(800));
            assert_eq!(converted_since(&page, before), 0, "{label} hidden: {run:?}");
            assert_eq!(run.renders, 0, "{label} hidden: {run:?}");
            evaluate(&mut page, &format!("{show}; 1"));
            drive(&mut page, &mut target, Duration::from_millis(300));
            let before = page.media_counters().converted;
            let run = drive(&mut page, &mut target, Duration::from_millis(800));
            assert!(
                converted_since(&page, before) >= 15,
                "{label} shown: {run:?}"
            );
            assert!(run.renders >= 15, "{label} shown: {run:?}");
        }
    });
}

/// A hidden window (minimized or covered: the GUI tells the page) converts no
/// frames, and the video converts again when the window is shown.
#[test]
fn a_hidden_window_converts_no_frames() {
    on_page_thread(|| {
        let (mut page, mut target) =
            start("<video id=v src=a.mp4 preload=auto muted loop></video>");
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, &mut target, Duration::from_millis(300));
        page.set_window_hidden(true);
        drive(&mut page, &mut target, Duration::from_millis(300));
        let before = page.media_counters().converted;
        let time = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        let run = drive(&mut page, &mut target, Duration::from_millis(800));
        assert_eq!(converted_since(&page, before), 0, "{run:?}");
        assert_eq!(run.renders, 0, "{run:?}");
        // The clock and the events go on while the window is hidden.
        let later = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(later > time + 0.5 || later < time, "{time} {later}");
        page.set_window_hidden(false);
        // The first render after it shows the last converted frame, not a
        // hole: the frames that the pipeline queued meanwhile have no pixels.
        page.pump();
        page.render_update(&mut target);
        let pixel = target.pixel(100, 100).unwrap().demultiply();
        assert_ne!((pixel.red(), pixel.green(), pixel.blue()), (204, 204, 204));
        drive(&mut page, &mut target, Duration::from_millis(300));
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_millis(800));
        assert!(converted_since(&page, before) >= 15, "{run:?}");
        assert!(run.renders >= 15, "{run:?}");
    });
}

/// A paused video that was hidden shows its frame again when the window is
/// shown (the quiet seek of `Pipeline::set_painted`).
#[test]
fn a_paused_video_shows_its_frame_after_the_window_was_hidden() {
    on_page_thread(|| {
        let (mut page, mut target) =
            start("<video id=v src=a.mp4 preload=auto muted style='margin:50px 0 0 50px'></video>");
        evaluate(
            &mut page,
            "(async () => { v.currentTime = 0.8; await once(v, 'seeked'); return 1; })()",
        );
        drive(&mut page, &mut target, Duration::from_millis(300));
        let shown = target.data().to_vec();
        let video_pixel = |target: &Pixmap| {
            let p = target.pixel(100, 100).unwrap().demultiply();
            (p.red(), p.green(), p.blue())
        };
        assert_ne!(video_pixel(&target), (204, 204, 204));
        page.set_window_hidden(true);
        drive(&mut page, &mut target, Duration::from_millis(200));
        page.set_window_hidden(false);
        drive(&mut page, &mut target, Duration::from_millis(800));
        assert_eq!(
            target.data(),
            &shown[..],
            "the frame after the hidden window"
        );
    });
}

/// The frame document of an iframe that lies outside the viewport (and its
/// margin) converts no frames of its video; it does when the page scrolls to
/// it.
#[test]
fn a_video_in_a_frame_follows_the_viewport_of_its_frame() {
    on_page_thread(|| {
        let mut site = site_with(
            "<div style='height:3000px'></div>\
            <iframe id=f src=frame.html width=400 height=300 style='border:0;display:block'></iframe>\
            <div style='height:3000px'></div>",
        );
        site.add_resource(
            "frame.html",
            &json!({
                "text": "<!DOCTYPE html><body style='margin:0'>\
                    <video src=a.mp4 autoplay muted loop style='display:block'></video>",
                "content_type": "text/html",
            }),
        );
        let mut page = page_of(site);
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        // The video of the frame starts (autoplay) and its first layout.
        drive(&mut page, &mut target, Duration::from_millis(1200));
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert_eq!(converted_since(&page, before), 0, "{run:?}");
        assert_eq!(run.renders, 0, "{run:?}");
        page.scroll_to(Point::new(0.0, 2800.0));
        drive(&mut page, &mut target, Duration::from_millis(300));
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert!(converted_since(&page, before) >= 20, "{run:?}");
        assert!(run.renders >= 20, "{run:?}");
        // Scrolled away again: the frame stops converting.
        page.scroll_to(Point::new(0.0, 0.0));
        drive(&mut page, &mut target, Duration::from_millis(300));
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert_eq!(converted_since(&page, before), 0, "{run:?}");
    });
}

/// The look at the frame containers (`Page::update_frames_shown`) and the
/// painted flags are keyed: the renders of a playing video in a frame, where
/// nothing else changes, walk no fragment tree; a scroll does.
#[test]
fn renders_without_a_change_walk_no_fragment_tree() {
    on_page_thread(|| {
        let mut site = site_with(
            "<div style='height:200px'></div>\
            <iframe id=f src=frame.html width=400 height=300 style='border:0;display:block'></iframe>\
            <div style='height:3000px'></div>",
        );
        site.add_resource(
            "frame.html",
            &json!({
                "text": "<!DOCTYPE html><body style='margin:0'>\
                    <video src=a.mp4 autoplay muted loop style='display:block'></video>",
                "content_type": "text/html",
            }),
        );
        let mut page = page_of(site);
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        drive(&mut page, &mut target, Duration::from_millis(1200));
        let walks = page.media_window_walks();
        let before = page.media_counters().converted;
        let run = drive(&mut page, &mut target, Duration::from_secs(1));
        assert!(run.renders >= 20, "frames render: {run:?}");
        assert!(converted_since(&page, before) >= 20, "{run:?}");
        assert_eq!(page.media_window_walks(), walks, "{run:?}");
        // A change of the scroll offset looks again.
        page.scroll_to(Point::new(0.0, 100.0));
        drive(&mut page, &mut target, Duration::from_millis(300));
        assert!(page.media_window_walks() > walks);
    });
}

/// A page that does not paint frames (headless) keeps the rule of feature
/// 7a: a video outside the viewport converts, because a full-page screenshot
/// draws it; `visibility: hidden` and the skipped contents still do not.
#[test]
fn a_headless_page_converts_the_videos_outside_the_viewport() {
    on_page_thread(|| {
        let site = site_with(&tall(3000));
        let site = Arc::new(site);
        let mut page = new_page(site as Arc<dyn Fetcher>, 2, VIEW);
        page.set_javascript(true);
        page.set_clock(PageClock::new(ClockMode::Real, None));
        page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        let v = node(&page, "v");
        let before = page.media_counters().converted;
        drive(&mut page, &mut target, Duration::from_secs(1));
        assert!(converted_since(&page, before) >= 15);
        assert!(page.media_frames_converted(v).unwrap() >= 15);
        evaluate(&mut page, "v.style.visibility = 'hidden'; 1");
        drive(&mut page, &mut target, Duration::from_millis(400));
        let before = page.media_counters().converted;
        drive(&mut page, &mut target, Duration::from_millis(800));
        assert_eq!(converted_since(&page, before), 0);
    });
}
