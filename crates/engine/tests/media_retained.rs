//! The budget of retained frames (M14 feature 10a, ADR 0047
//! "Implementation notes (M14 feature 10a)"): a video whose pipeline holds no
//! place keeps its current frame; a document keeps at most its budget of such
//! frames. Over the budget the frames of videos without a box go first, then
//! those of the least recently painted ones, then the oldest; a dropped frame
//! comes back when its video is painted again (the quiet seek opens the
//! suspended pipeline). Counters, not the RSS. Run by `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::media_probe::evaluate_strict as evaluate;
use common::media_site::{FOLDER, MP4, MediaSite, open_page};
use common::{node, on_page_thread};
use swb_engine::{ClockMode, Page, Size};
/// The bytes of one RGBA frame of the test file (320 x 240).
const FRAME: u64 = 320 * 240 * 4;
/// Page time in which every pipeline of a few videos loads and suspends
/// (after 15 s idle).
const SUSPENDED: Duration = Duration::from_secs(20);

fn page_for(html: &str) -> (Page, Arc<MediaSite>) {
    page_with(html, MP4, Size::new(800.0, 600.0))
}

/// A page of `html` whose `a.mp4` is the test file `file`.
fn page_with(html: &str, file: &str, viewport: Size) -> (Page, Arc<MediaSite>) {
    let mut site = MediaSite::new(FOLDER, html);
    site.add_file("a.mp4", file);
    open_page(site, viewport, ClockMode::Virtual, |page| {
        page.isolate_media(Duration::ZERO);
        page.set_javascript(true);
    })
}

/// Runs the page for `time` of page time, then draws it (the paint syncs
/// the images of all videos).
fn run(page: &mut Page, time: Duration) {
    page.run_event_loop(time, Duration::from_secs(120));
    page.screenshot(false).unwrap();
}

/// `count` videos `#v0`, `#v1`, ... of the test file at `preload=metadata`,
/// with `attrs`.
fn videos(count: usize, attrs: &str) -> String {
    use std::fmt::Write;
    let mut html = String::new();
    for i in 0..count {
        let _ = write!(
            html,
            "<video id=v{i} src=a.mp4 preload=metadata {attrs}></video>"
        );
    }
    html
}

/// The videos `#v0`, `#v1`, ... that show a frame.
fn showing(page: &Page, count: usize) -> Vec<usize> {
    (0..count)
        .filter(|i| page.media_shows_frame(node(page, &format!("v{i}"))))
        .collect()
}

/// 1,000 paused videos that decoded their first frame (through the 16
/// places, each suspends 15 s after its load): the frames that they keep
/// stay within the default budget (256 MiB; 1,000 frames of 320 x 240 are
/// 307 MB) because the budget keeps reduced copies for their 32 x 24 boxes
/// (40 x 30, 4.8 KB); none is dropped and every video shows its frame
/// (review fixes of 10a).
#[test]
fn the_retained_frames_of_1000_paused_videos_stay_within_the_budget() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html><script>let n = 0; document.addEventListener('loadeddata', \
             () => n++, true);</script>{}",
            videos(1000, "width=32 height=24")
        );
        let (mut page, _site) = page_for(&html);
        // 1,000 / 16 rounds of 15 s.
        run(&mut page, Duration::from_secs(1000));
        assert_eq!(evaluate(&mut page, "n"), 1000);
        let (bytes, dropped) = page.media_retained_frames();
        let budget = 256 << 20;
        assert!(bytes <= budget, "{bytes} bytes retained");
        assert_eq!(dropped, 0);
        assert!(page.media_reduced_frames() > 0);
        assert_eq!(showing(&page, 1000).len(), 1000);
        let sizes: std::collections::HashSet<_> = (0..1000)
            .filter_map(|i| page.media_frame_size(node(&page, &format!("v{i}"))))
            .collect();
        assert!(sizes.contains(&(40, 30)), "{sizes:?}");
    });
}

/// The review case of 10a: 100 visible paused 1080p videos in boxes of
/// 120 x 68 hold 830 MB of frames when each keeps its full frame (the
/// budget held about 30, and 64 videos showed nothing). The budget keeps
/// reduced copies (240 x 135, 130 KB) of the least recently painted ones:
/// every video shows its frame, each box looks like a lone video of the
/// same frame within 1 in a color channel, and the retained bytes stay
/// within the budget.
#[test]
fn a_hundred_visible_paused_1080p_videos_all_show_their_frame() {
    on_page_thread(|| {
        const FILE: &str = "fixtures/media/mp4-h264-untagged-bt709-1920x1080.mp4";
        let (w, h) = (120u32, 68u32);
        let grid = |count: usize| {
            format!(
                "<!DOCTYPE html><style>body {{ margin: 0 }} div {{ display: grid; \
                 grid-template-columns: repeat(10, {w}px) }} video {{ display: block; \
                 width: {w}px; height: {h}px }}</style><div>{}</div>",
                videos(count, "")
            )
        };
        // One video alone: the reference picture.
        let (mut one, _site) = page_with(&grid(1), FILE, Size::new(1200.0, 680.0));
        run(&mut one, SUSPENDED);
        assert_eq!(showing(&one, 1), [0]);
        let reference = one.screenshot(false).unwrap();
        drop(one);
        let (mut page, _site) = page_with(&grid(100), FILE, Size::new(1200.0, 680.0));
        // 100 / 16 rounds of 15 s.
        run(&mut page, Duration::from_secs(150));
        assert_eq!(showing(&page, 100).len(), 100);
        let (bytes, dropped) = page.media_retained_frames();
        assert!(bytes <= 256 << 20, "{bytes} bytes retained");
        assert_eq!(dropped, 0);
        assert!(page.media_reduced_frames() > 0);
        // Paint reduces the full frames over a few rasters (its budget of
        // reductions per raster).
        for _ in 0..3 {
            page.screenshot(false).unwrap();
        }
        let shot = page.screenshot(false).unwrap();
        for i in 0..100u32 {
            let (x0, y0) = ((i % 10) * w, (i / 10) * h);
            let mut most = 0;
            for y in 0..h {
                for x in 0..w {
                    let a = shot.pixel(x0 + x, y0 + y).unwrap();
                    let b = reference.pixel(x, y).unwrap();
                    for (p, q) in [
                        (a.red(), b.red()),
                        (a.green(), b.green()),
                        (a.blue(), b.blue()),
                    ] {
                        most = most.max(p.abs_diff(q));
                    }
                }
            }
            assert!(most <= 1, "video {i} differs by {most}");
        }
    });
}

/// A reduced frame whose box grows past the copy comes back at full size
/// (the quiet seek opens the suspended pipeline); the element's state does
/// not change.
#[test]
fn a_reduced_frame_comes_back_at_full_size_when_its_box_grows() {
    on_page_thread(|| {
        let (mut page, _site) = page_for(&format!(
            "<!DOCTYPE html>{}",
            videos(2, "width=80 height=60")
        ));
        page.set_retained_frame_budget(FRAME + FRAME / 4);
        run(&mut page, SUSPENDED);
        assert_eq!(page.media_retained_frames(), (FRAME + FRAME / 4, 0));
        let sizes =
            |page: &Page| [0, 1].map(|i| page.media_frame_size(node(page, &format!("v{i}"))));
        // The older frame is reduced for its 80 x 60 box.
        assert_eq!(sizes(&page), [Some((160, 120)), Some((320, 240))]);
        let converted = page.media_frames_converted(node(&page, "v0")).unwrap();
        // A box of 200 x 150 needs more than 160 x 120.
        evaluate(&mut page, "v0.width = 200; v0.height = 150; 1");
        run(&mut page, Duration::from_secs(1));
        assert_eq!(sizes(&page)[0], Some((320, 240)), "the full frame is back");
        assert!(page.media_frames_converted(node(&page, "v0")).unwrap() > converted);
        assert_eq!(
            evaluate(&mut page, "[v0.readyState, v0.currentTime, v0.paused]"),
            serde_json::json!([4, 0, true])
        );
        // A box that the copy covers keeps the copy: v1's frame is reduced
        // once v0 suspends again, and a box of 150 x 113 needs no more.
        run(&mut page, SUSPENDED);
        assert_eq!(sizes(&page)[1], Some((160, 120)));
        let converted = page.media_frames_converted(node(&page, "v1")).unwrap();
        evaluate(&mut page, "v1.width = 150; v1.height = 113; 1");
        run(&mut page, Duration::from_secs(1));
        assert_eq!(sizes(&page)[1], Some((160, 120)));
        assert_eq!(
            page.media_frames_converted(node(&page, "v1")).unwrap(),
            converted
        );
    });
}

/// A frame that the budget dropped while its video was painted comes back
/// when the video is painted again: the quiet seek opens the suspended
/// pipeline, which converts the frame at its position again.
#[test]
fn a_dropped_frame_comes_back_when_its_video_is_painted_again() {
    on_page_thread(|| {
        let (mut page, _site) = page_for(&format!("<!DOCTYPE html>{}", videos(4, "")));
        page.set_retained_frame_budget(2 * FRAME);
        run(&mut page, SUSPENDED);
        let (bytes, dropped) = page.media_retained_frames();
        assert_eq!((bytes, dropped), (2 * FRAME, 2));
        let gone: Vec<usize> = (0..4).filter(|i| !showing(&page, 4).contains(i)).collect();
        assert_eq!(gone.len(), 2);
        let back = gone[0];
        let id = format!("v{back}");
        let converted = page.media_frames_converted(node(&page, &id)).unwrap();
        // Not painted, then painted again.
        evaluate(&mut page, &format!("{id}.style.display = 'none'; 1"));
        run(&mut page, Duration::from_millis(100));
        evaluate(&mut page, &format!("{id}.style.display = ''; 1"));
        run(&mut page, Duration::from_secs(1));
        assert!(
            page.media_shows_frame(node(&page, &id)),
            "the frame is back"
        );
        assert!(page.media_frames_converted(node(&page, &id)).unwrap() > converted);
        // The element's state did not change (a quiet seek).
        assert_eq!(
            evaluate(
                &mut page,
                &format!("[{id}.readyState, {id}.currentTime, {id}.paused]")
            ),
            serde_json::json!([4, 0, true])
        );
        // It suspends again and the budget holds.
        run(&mut page, SUSPENDED);
        assert!(page.media_retained_frames().0 <= 2 * FRAME);
        assert!(
            page.media_shows_frame(node(&page, &id)),
            "the newest frame stays"
        );
    });
}

/// The steps of the budget with boxes smaller than the frames (80 x 60,
/// copies of 160 x 120, a quarter of a frame): (a) a video without a box
/// loses its frame; (b) the frames of the others are reduced, least
/// recently painted first (a video with `visibility: hidden`), then the
/// oldest; (c) only then frames go, in the same order.
#[test]
fn the_budget_drops_boxless_frames_then_reduces_then_drops() {
    on_page_thread(|| {
        let (mut page, _site) = page_for(&format!(
            "<!DOCTYPE html>{}",
            videos(4, "width=80 height=60")
        ));
        run(&mut page, SUSPENDED);
        assert_eq!(page.media_retained_frames(), (4 * FRAME, 0));
        let sizes = |page: &Page| {
            (0..4)
                .map(|i| page.media_frame_size(node(page, &format!("v{i}"))))
                .collect::<Vec<_>>()
        };
        let (full, small) = (Some((320, 240)), Some((160, 120)));
        evaluate(&mut page, "v1.style.visibility = 'hidden'; 1");
        run(&mut page, Duration::from_millis(100));
        evaluate(&mut page, "v2.style.display = 'none'; 1");
        run(&mut page, Duration::from_millis(100));
        // (a): v2 has no box.
        page.set_retained_frame_budget(3 * FRAME);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(page.media_retained_frames(), (3 * FRAME, 1));
        assert_eq!(sizes(&page), [full, full, None, full]);
        // (b): v1 was painted least recently.
        page.set_retained_frame_budget(3 * FRAME - FRAME / 2);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(sizes(&page), [full, small, None, full]);
        // (b): then the oldest frame first.
        page.set_retained_frame_budget(FRAME + FRAME / 2);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(sizes(&page), [small, small, None, full]);
        page.set_retained_frame_budget(FRAME);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(sizes(&page), [small, small, None, small]);
        assert_eq!(page.media_retained_frames(), (3 * FRAME / 4, 1));
        assert_eq!(page.media_reduced_frames(), 3);
        // (c): v1, then v0.
        page.set_retained_frame_budget(FRAME / 4);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(sizes(&page), [None, None, None, small]);
        assert_eq!(page.media_retained_frames(), (FRAME / 4, 3));
    });
}

/// The order of the drops: a video without a box loses its frame first,
/// although it was painted more recently than a hidden one with a box; then
/// the least recently painted (the hidden one), then the oldest frame.
#[test]
fn videos_without_a_box_lose_their_frames_first() {
    on_page_thread(|| {
        let (mut page, _site) = page_for(&format!("<!DOCTYPE html>{}", videos(4, "")));
        run(&mut page, SUSPENDED);
        assert_eq!(showing(&page, 4), [0, 1, 2, 3]);
        assert_eq!(page.media_retained_frames(), (4 * FRAME, 0));
        // v1 keeps its box but is not painted from now on.
        evaluate(&mut page, "v1.style.visibility = 'hidden'; 1");
        run(&mut page, Duration::from_millis(100));
        // v2 was painted until now, and loses its box.
        evaluate(&mut page, "v2.style.display = 'none'; 1");
        run(&mut page, Duration::from_millis(100));
        page.set_retained_frame_budget(3 * FRAME);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(page.media_retained_frames(), (3 * FRAME, 1));
        assert_eq!(showing(&page, 4), [0, 1, 3], "v2 has no box");
        page.set_retained_frame_budget(2 * FRAME);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(showing(&page, 4), [0, 3], "v1 was painted least recently");
        page.set_retained_frame_budget(FRAME);
        run(&mut page, Duration::from_millis(100));
        assert_eq!(page.media_retained_frames(), (FRAME, 3));
        assert_eq!(showing(&page, 4), [3], "then the oldest frame");
    });
}

/// The bytes of one RGBA 1080p frame.
const FRAME_1080P: u64 = 1920 * 1080 * 4;
/// The cap of one sync (`REDUCE_PIXELS_PER_SYNC`), in source pixels.
const SYNC_PIXELS: u64 = 8 << 20;
/// The margin over the budget while reductions wait (`RETAINED_MARGIN`).
const MARGIN: u64 = 64 << 20;

/// 24 visible paused 1080p videos in boxes of 120 x 68 that each keep their
/// full frame (24 x 7.9 MiB, within the default budget).
fn loaded_1080p_grid() -> Page {
    const FILE: &str = "fixtures/media/mp4-h264-untagged-bt709-1920x1080.mp4";
    let html = format!(
        "<!DOCTYPE html><style>body {{ margin: 0 }} div {{ display: grid; \
         grid-template-columns: repeat(6, 120px) }} video {{ display: block; \
         width: 120px; height: 68px }}</style><div>{}</div>",
        videos(24, "")
    );
    let (mut page, _site) = page_with(&html, FILE, Size::new(800.0, 600.0));
    // 24 / 16 rounds of 15 s.
    run(&mut page, Duration::from_secs(60));
    assert_eq!(showing(&page, 24).len(), 24);
    assert_eq!(page.media_retained_frames(), (24 * FRAME_1080P, 0));
    assert_eq!(page.media_reduced_frames(), 0);
    page
}

/// What a document with 24 large frames over its budget does (round 2 of
/// the fixes of 10a): one sync converts at most `SYNC_PIXELS` source pixels
/// (4 frames of 1080p), and later syncs finish the reductions without any
/// other trigger (the loops wake for the backlog's due time), the same on
/// every run (virtual time).
fn reduce_in_steps() -> ((u64, u64), u64, (u64, u64)) {
    let mut page = loaded_1080p_grid();
    // 24 frames are 190 MiB: the first sync leaves 159 MiB, under the budget
    // and the margin (164 MiB), so no frame goes.
    page.set_retained_frame_budget(100 << 20);
    // One sync (the loop pumps once, then the page time ends).
    page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
    let (max, syncs) = page.media_reduce_work();
    assert!(
        (1..=SYNC_PIXELS).contains(&max),
        "{max} source pixels in a sync"
    );
    assert!(page.media_retained_frames().0 > 100 << 20, "work is left");
    // No paint and no other trigger: the due times finish the rest.
    page.run_event_loop(Duration::from_millis(500), Duration::from_secs(60));
    let (bytes, dropped) = page.media_retained_frames();
    assert!(bytes <= 100 << 20, "{bytes} bytes retained");
    assert_eq!(dropped, 0, "the margin was not reached");
    let (max, syncs_done) = page.media_reduce_work();
    assert!(max <= SYNC_PIXELS, "{max} source pixels in a sync");
    assert!(syncs_done >= 3 && syncs_done > syncs, "{syncs_done} syncs");
    assert_eq!(showing(&page, 24).len(), 24);
    (
        (max, syncs_done),
        page.media_reduced_frames(),
        (bytes, dropped),
    )
}

#[test]
fn a_sync_reduces_a_bounded_number_of_large_frames_and_later_syncs_finish() {
    on_page_thread(|| {
        let first = reduce_in_steps();
        assert!(first.1 >= 12, "{} frames reduced", first.1);
        // The counts are the same on every run.
        assert_eq!(reduce_in_steps(), first);
    });
}

/// Over the budget by more than the margin while reductions wait: frames
/// go (the reduced ones stay), so the bytes are within budget + margin
/// after the first sync, not only after the last.
#[test]
fn frames_go_when_the_reductions_that_wait_leave_the_total_past_the_margin() {
    on_page_thread(|| {
        let mut page = loaded_1080p_grid();
        let budget = FRAME_1080P;
        page.set_retained_frame_budget(budget);
        page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
        let (bytes, dropped) = page.media_retained_frames();
        assert!(bytes <= budget + MARGIN, "{bytes} bytes retained");
        assert!(dropped > 0, "no frame went");
        assert!(page.media_reduce_work().0 <= SYNC_PIXELS);
        // The rest finishes through the due times.
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(60));
        let (bytes, dropped) = page.media_retained_frames();
        assert!(bytes <= budget, "{bytes} bytes retained");
        assert!(dropped > 0);
    });
}
