//! The frame budget of the pipelines (M14 feature 10a, ADR 0047
//! "Implementation notes (M14 feature 10a)"): the pipelines of one set of
//! places share a budget of bytes for their decoded frames. A run reserves
//! its queue and the pixmaps besides it when it knows the frame size; a
//! shorter queue when the budget is short; it waits (as for a place, without
//! pending work) when even the shortest does not fit, and gets the bytes
//! when another run gives them back. The counters, not the RSS, show it.
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::driver::{HANG, Waker, options, play_virtual, settle_update};
use common::fixture;
use swb_media::pipeline::{Pipeline, PipelineSlots, Report, SourceOpener};

/// The bytes of one RGBA frame of the 320 x 240 test files.
const SMALL: u64 = 320 * 240 * 4;
/// The pixmaps of a run besides its frame queue.
const EXTRA: u64 = 3;

fn open_in(name: &str, slots: &PipelineSlots, waker: &Arc<Waker>) -> Pipeline {
    let mut o = options(waker);
    o.slots = slots.clone();
    o.capture_audio = false;
    Pipeline::open(SourceOpener::file(fixture(name)), o)
}

/// Waits until `cond` holds, at most `HANG`.
fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(started.elapsed() < HANG, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Six pipelines over a budget of two full queues (6 frames and 3 pixmaps
/// each) and a short one: two runs get 6 frames, one gets 3, the others wait
/// without pending work; the bytes that runs hold never pass the budget,
/// and the pixmaps of each run stay within its share. A run that ends gives
/// its bytes to the next waiter, which plays to the end.
#[test]
fn the_budget_bounds_the_decoded_frames_of_all_pipelines() {
    let budget = 2 * (6 + EXTRA) * SMALL + (3 + EXTRA) * SMALL;
    let slots = PipelineSlots::with_frame_budget(16, budget);
    let waker = Arc::new(Waker::default());
    let mut pipelines: Vec<Pipeline> = (0..6)
        .map(|_| open_in("mp4-h264-high-aac.mp4", &slots, &waker))
        .collect();
    wait_until("three runs reserve, three wait", || {
        slots.frame_waiting_count() == 3 && slots.frame_bytes() == budget
    });
    let mut reports = Vec::new();
    let (running, waiting): (Vec<usize>, Vec<usize>) =
        (0..6).partition(|&i| !pipelines[i].waits_for_frame_budget());
    assert_eq!((running.len(), waiting.len()), (3, 3));
    for &i in &waiting {
        let p = &mut pipelines[i];
        assert!(p.is_waiting(), "waits as for a place");
        assert!(
            !p.has_pending_work(),
            "a wait for frames is not pending work"
        );
        assert!(p.update(0).is_empty(), "no metadata before the bytes");
    }
    for &i in &running {
        settle_update(&mut pipelines[i], &waker, 0, &mut reports);
        assert!(reports.contains(&Report::FirstFrame));
    }
    let mut caps: Vec<usize> = running.iter().map(|&i| pipelines[i].frame_cap()).collect();
    caps.sort_unstable();
    assert_eq!(caps, [3, 6, 6]);
    // Each run plays to its end within its share of pixmaps.
    for &i in &running {
        let cap = pipelines[i].frame_cap() as u64;
        let played = play_virtual(&mut pipelines[i], &waker, 0, i64::MAX);
        assert_eq!(played.ended, Some(2_000_000));
        let stats = pipelines[i].stats();
        assert!(
            stats.pixmaps_live as u64 <= cap + EXTRA,
            "{} pixmaps for a cap of {cap}",
            stats.pixmaps_live
        );
        assert_eq!(stats.frames_over_budget, 0);
    }
    assert!(slots.peak_frame_bytes() <= budget);
    // A run that ends gives its bytes to the first waiter.
    let first = running[0];
    pipelines[first].close();
    wait_until("one waiter gets bytes", || slots.frame_waiting_count() == 2);
    let next = waiting
        .iter()
        .copied()
        .find(|&i| !pipelines[i].waits_for_frame_budget())
        .expect("a waiter got the bytes");
    let played = play_virtual(&mut pipelines[next], &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    assert!(slots.peak_frame_bytes() <= budget);
    // Closing the waiters leaves the queue empty and gives every byte back.
    drop(pipelines);
    wait_until("every byte comes back", || {
        slots.frame_bytes() == 0 && slots.live_threads() == 0
    });
    assert_eq!(slots.frame_waiting_count(), 0);
}

/// A run that waits for frame bytes and is suspended leaves the queue; the
/// bytes go to the next waiter, and the suspended pipeline waits again when
/// it plays.
#[test]
fn a_suspended_waiter_leaves_the_queue_of_frame_bytes() {
    let budget = (6 + EXTRA) * SMALL;
    let slots = PipelineSlots::with_frame_budget(16, budget);
    let waker = Arc::new(Waker::default());
    let mut a = open_in("mp4-h264-high-aac.mp4", &slots, &waker);
    wait_until("a reserves", || slots.frame_bytes() == budget);
    let mut b = open_in("mp4-h264-high-aac.mp4", &slots, &waker);
    let mut c = open_in("mp4-h264-high-aac.mp4", &slots, &waker);
    wait_until("b and c wait", || slots.frame_waiting_count() == 2);
    b.suspend(0);
    assert_eq!(slots.frame_waiting_count(), 1);
    a.close();
    wait_until("c gets the bytes", || !c.waits_for_frame_budget());
    let played = play_virtual(&mut c, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    // b plays after c is gone.
    b.play(0);
    wait_until("b waits", || b.waits_for_frame_budget());
    drop(c);
    let played = play_virtual(&mut b, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
}

/// A run that waits for frame bytes is not failed by the no-progress limit
/// (review of 10a): its owner asked for pending work once while the run
/// opened (which arms the limit) and not again for several limits. The
/// owner is notified when the wait begins, and the run plays when the
/// bytes come.
#[test]
fn a_waiter_for_frame_bytes_is_not_failed_by_the_no_progress_limit() {
    let name = "mp4-h264-high-aac.mp4";
    let budget = (6 + EXTRA) * SMALL;
    let slots = PipelineSlots::with_frame_budget(16, budget);
    let waker = Arc::new(Waker::default());
    let mut a = open_in(name, &slots, &waker);
    wait_until("a holds the budget", || slots.frame_bytes() == budget);
    // b opens only when the test lets it, so that the owner's one question
    // comes before b reaches the wait.
    let (go, gate) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    let path = fixture(name);
    let opener = SourceOpener::new(
        move || {
            // The first open waits for the test; a reopen does not.
            let _ = gate
                .lock()
                .expect("lock")
                .recv_timeout(Duration::from_secs(10));
            Ok(Box::new(swb_media::FileSource::open(&path)?) as Box<dyn swb_media::ByteSource>)
        },
        None,
    );
    let limit = Duration::from_millis(200);
    let mut o = options(&waker);
    o.slots = slots.clone();
    o.capture_audio = false;
    o.stall_limit = limit;
    let mut b = Pipeline::open(opener, o);
    assert!(
        b.has_pending_work(),
        "b opens: pending work, the limit armed"
    );
    let seen = waker.count();
    go.send(()).expect("b's opener waits");
    drop(go);
    wait_until("b waits for frame bytes", || b.waits_for_frame_budget());
    assert!(waker.count() > seen, "the owner learns of the wait");
    std::thread::sleep(limit * 5);
    assert!(
        !b.has_unread_reports(),
        "no failure while b waits: {:?}",
        b.update(0)
    );
    assert!(b.waits_for_frame_budget());
    a.close();
    wait_until("b gets the bytes", || !b.waits_for_frame_budget());
    let played = play_virtual(&mut b, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000), "{:?}", played.reports);
}

/// A stream whose frames grow (640 x 360, then 1280 x 720; the container
/// declares the first size) grows its run's reservation at once to the
/// share of the larger frames and plays to its end without a frame over
/// its share (review of 10a).
#[test]
fn a_video_whose_frames_grow_grows_its_reservation_and_plays_on() {
    let slots = PipelineSlots::new(16);
    let waker = Arc::new(Waker::default());
    let mut p = open_in("webm-vp8-640x360-to-1280x720.webm", &slots, &waker);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    let small = (6 + EXTRA) * 640 * 360 * 4;
    assert_eq!(slots.frame_bytes(), small, "the declared size first");
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(800_000), "{:?}", played.reports);
    let large = (6 + EXTRA) * 1280 * 720 * 4;
    assert_eq!(slots.frame_bytes(), large, "the reservation grew");
    assert_eq!(p.frame_cap(), 6);
    let stats = p.stats();
    assert_eq!(stats.frames_over_budget, 0, "{stats:?}");
    assert!(stats.pixmap_bytes <= large, "{stats:?}");
    // Every frame was converted at its size: 10 small, 10 large.
    assert_eq!(stats.frames_converted, 20, "{stats:?}");
    drop(p);
    wait_until("the bytes come back", || slots.frame_bytes() == 0);
}

/// A stream that declares 16 x 16 and switches to 8192 x 4352 frames: the
/// share of the large frames (5 of them, 713 MB) does not fit in the free
/// bytes of the budget, so the run fails with a decode error (code 3) and
/// converts no large frame; the bytes that runs hold stay within the
/// budget (review of 10a: before, the pool's limit counted pixmaps and the
/// run held 643 MiB on a reservation for 16 x 16 frames).
#[test]
fn a_video_whose_frames_grow_past_the_free_budget_fails() {
    let budget = 64 << 20;
    let slots = PipelineSlots::with_frame_budget(16, budget);
    let waker = Arc::new(Waker::default());
    let mut p = open_in("webm-vp8-16x16-to-8192x4352.webm", &slots, &waker);
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert!(p.is_failed(), "{:?}", played.reports);
    let error = played.reports.iter().find_map(|r| match r {
        Report::Error(e) => Some(e.clone()),
        _ => None,
    });
    let error = error.expect("an error report");
    assert_eq!(error.element_code(), Some(3), "{error}");
    assert!(error.to_string().contains("frame budget"), "{error}");
    assert!(slots.peak_frame_bytes() <= budget);
    let stats = p.stats();
    assert!(stats.pixmap_bytes <= 16 * 16 * 4 * (6 + EXTRA), "{stats:?}");
    assert!(stats.pixmaps_allocated <= 6 + EXTRA, "{stats:?}");
}

/// The budget of the process holds sixteen 1080p runs with full queues: a
/// 1080p video that plays while 15 others hold their frames gets its whole
/// queue, converts every frame and drops none on a clock that runs with
/// real time (the threads get at most 30 ms per 10 ms step of the clock).
#[test]
fn a_1080p_video_keeps_its_queue_and_drops_no_frame_while_others_hold_frames() {
    let name = "mp4-h264-untagged-bt709-1920x1080.mp4";
    let slots = PipelineSlots::new(16);
    let waker = Arc::new(Waker::default());
    let mut reports = Vec::new();
    let mut others: Vec<Pipeline> = (0..15).map(|_| open_in(name, &slots, &waker)).collect();
    for p in &mut others {
        settle_update(p, &waker, 0, &mut reports);
    }
    let frame = 1920 * 1080 * 4;
    assert!(
        slots.frame_bytes() >= 15 * (6 + EXTRA) * frame,
        "the others hold frames"
    );
    let mut p = open_in(name, &slots, &waker);
    settle_update(&mut p, &waker, 0, &mut reports);
    assert_eq!(p.frame_cap(), 6);
    assert!(slots.peak_frame_bytes() <= slots.frame_budget());
    p.play(0);
    let mut now = 0;
    while !p.is_ended() && now < 5_000_000 {
        let started = Instant::now();
        // The threads have until the frame after the next one is decoded,
        // at most 30 ms.
        while p.has_pending_work() && started.elapsed() < Duration::from_millis(30) {
            let seen = waker.count();
            waker.wait_past(seen, Duration::from_millis(5));
        }
        now += 10_000;
        reports.extend(p.update(now));
    }
    assert!(p.is_ended(), "{reports:?}");
    let quality = p.quality();
    let stats = p.stats();
    assert_eq!(quality.dropped_frames, 0, "{quality:?} {stats:?}");
    assert_eq!(stats.frames_over_budget, 0);
    assert_eq!(stats.frames_converted, 15);
}
