//! The threads of the media pipeline (M14 feature 5, ADR 0047 sections 6
//! and 7): an underrun with a source that stalls (the reports and the
//! stopped clock), closing while a thread blocks in the source, the bound
//! of 16 open pipelines with suspend and reopen, and no thread work while
//! paused.
#![cfg(feature = "ffmpeg")]

mod common;

use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::driver::{
    HANG, TestWaker, open_fixture, open_settled, options, play_virtual, settle, settle_update,
    settle_within, wait_until,
};
use common::fixture_bytes;
use swb_media::pipeline::{
    AudioOutput, Pipeline, PipelineSlots, PipelineStats, Report, SourceOpener,
};
use swb_media::{ByteSource, MemorySource};

/// A gate that a stalling source waits at. A cancelled gate fails the
/// waiting read; one that ignores cancels holds it until it opens (a read
/// that does not return).
#[derive(Default)]
struct Gate {
    /// (open, cancelled).
    state: Mutex<(bool, bool)>,
    cv: Condvar,
    reached: std::sync::atomic::AtomicBool,
}

impl Gate {
    fn open(&self) {
        self.state.lock().expect("lock").0 = true;
        self.cv.notify_all();
    }

    fn cancel(&self) {
        self.state.lock().expect("lock").1 = true;
        self.cv.notify_all();
    }

    /// Whether a read has waited at the gate.
    fn reached(&self) -> bool {
        self.reached.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Waits until the gate opens; fails when it is cancelled.
    fn pass(&self) -> io::Result<()> {
        self.reached
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let guard = self.state.lock().expect("lock");
        let guard = self
            .cv
            .wait_while(guard, |(open, cancelled)| !*open && !*cancelled)
            .expect("lock");
        if guard.1 {
            // Not `Interrupted`: that one means "read again".
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "cancelled",
            ));
        }
        Ok(())
    }
}

/// Bytes in memory whose reads past `limit` wait at the gate.
struct Stalling {
    inner: MemorySource,
    pos: u64,
    limit: u64,
    gate: Arc<Gate>,
}

impl ByteSource for Stalling {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = self.limit.saturating_sub(self.pos);
        let buf = if room == 0 {
            self.gate.pass()?;
            buf
        } else {
            let n = usize::try_from(room).unwrap_or(usize::MAX).min(buf.len());
            &mut buf[..n]
        };
        let n = self.inner.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.pos = self.inner.seek(pos)?;
        Ok(self.pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.inner.size()
    }
}

/// An opener of `name` whose reads stall after `limit` bytes.
fn stalling(name: &str, limit: u64, gate: &Arc<Gate>) -> SourceOpener {
    let cancel_gate = Arc::clone(gate);
    stalling_with(
        name,
        limit,
        gate,
        Some(Arc::new(move || cancel_gate.cancel())),
    )
}

/// An opener of `name` whose reads stall after `limit` bytes, with `cancel`.
fn stalling_with(
    name: &str,
    limit: u64,
    gate: &Arc<Gate>,
    cancel: Option<Arc<dyn Fn() + Send + Sync>>,
) -> SourceOpener {
    let data: Arc<[u8]> = fixture_bytes(name).into();
    let open_gate = Arc::clone(gate);
    SourceOpener::new(
        move || {
            Ok(Box::new(Stalling {
                inner: MemorySource::new(Arc::clone(&data)),
                pos: 0,
                limit,
                gate: Arc::clone(&open_gate),
            }) as Box<dyn ByteSource>)
        },
        cancel,
    )
}

/// The element code of the first error report (0 for an error without
/// one).
fn error_code(reports: &[Report]) -> Option<u16> {
    reports.iter().find_map(|r| match r {
        Report::Error(e) => Some(e.element_code().unwrap_or(0)),
        _ => None,
    })
}

#[test]
fn a_stalled_source_underruns_and_resumes() {
    let name = "mp4-h264-high-aac.mp4";
    let size = fixture_bytes(name).len() as u64;
    let gate = Arc::new(Gate::default());
    let waker = Arc::new(TestWaker::default());
    let mut p = Pipeline::open(stalling(name, size * 6 / 10, &gate), options(&waker));
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    p.play(0);
    // Real time on a clock of the test: the clock moves on also when the
    // data is not there.
    let mut now = 0;
    let mut underrun = None;
    for _ in 0..200 {
        settle_within(&p, &waker, Duration::from_millis(200));
        now += 40_000;
        for r in p.update(now) {
            if let Report::Underrun { media_us } = r {
                underrun = Some(media_us);
            }
        }
        if underrun.is_some() {
            break;
        }
    }
    let at = underrun.expect("an underrun");
    assert!((300_000..2_000_000).contains(&at), "underrun at {at}");
    assert!(p.is_underrun());
    // The clock stands; nothing is due.
    for _ in 0..5 {
        now += 100_000;
        let r = p.update(now);
        assert!(r.iter().all(|r| matches!(r, Report::Buffered(_))), "{r:?}");
        assert_eq!(p.position(), at);
        assert_eq!(p.next_due(), None);
    }
    gate.open();
    let started = Instant::now();
    let resumed = loop {
        let seen = waker.count();
        let r = p.update(now);
        if let Some(m) = r.iter().find_map(|r| match r {
            Report::Resumed { media_us } => Some(*media_us),
            _ => None,
        }) {
            break m;
        }
        assert!(started.elapsed() < HANG, "no resume");
        waker.wait_past(seen, Duration::from_millis(100));
    };
    assert_eq!(resumed, at, "playback resumes where it stopped");
    let played = play_virtual(&mut p, &waker, now, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
}

#[test]
fn closing_ends_threads_that_block_in_the_source() {
    for (name, limit) in [
        // Blocks in the open.
        ("mp4-h264-high-aac.mp4", 2_000),
        // Blocks while it reads ahead (within the first second).
        ("mp4-h264-high-aac.mp4", 20_000),
        ("mp4-h264-aac-long.mp4", 25_000),
        ("ogg-vorbis.ogg", 5_000),
    ] {
        let gate = Arc::new(Gate::default());
        let waker = Arc::new(TestWaker::default());
        let slots = PipelineSlots::new(4);
        let mut o = options(&waker);
        o.slots = slots.clone();
        let p = Pipeline::open(stalling(name, limit, &gate), o);
        wait_until(&format!("a read of {name} blocks at {limit}"), || {
            gate.reached()
        });
        std::thread::sleep(Duration::from_millis(20));
        if limit == 2_000 {
            assert!(p.has_pending_work(), "blocked in the open");
        }
        drop(p);
        wait_until(&format!("the threads of {name} ({limit}) end"), || {
            slots.live_threads() == 0
        });
        assert_eq!(slots.open_count(), 0);
    }
}

#[test]
fn a_thousand_pipelines_open_through_sixteen_places() {
    let data: Arc<[u8]> = fixture_bytes("wav-pcm-s16.wav").into();
    let slots = PipelineSlots::new(16);
    let waker = Arc::new(TestWaker::default());
    let mut pipelines: Vec<Pipeline> = (0..1000)
        .map(|_| {
            let mut o = options(&waker);
            o.slots = slots.clone();
            o.capture_audio = false;
            Pipeline::open(SourceOpener::memory(Arc::clone(&data), true), o)
        })
        .collect();
    assert_eq!(slots.open_count(), 16);
    assert_eq!(slots.waiting_count(), 984);
    assert!(pipelines[999].is_waiting());
    assert!(!pipelines[999].has_pending_work(), "waiting is not pending");
    for p in &mut pipelines {
        let mut reports = Vec::new();
        let started = Instant::now();
        while !reports.contains(&Report::FirstFrame) {
            let seen = waker.count();
            reports.extend(p.update(0));
            assert!(started.elapsed() < HANG, "no first data");
            if !reports.contains(&Report::FirstFrame) {
                waker.wait_past(seen, Duration::from_millis(20));
            }
        }
        // The element suspends an idle pipeline (after 15 s; here at once).
        p.suspend(0);
        assert!(p.is_suspended());
    }
    wait_until("the threads end", || slots.live_threads() == 0);
    assert_eq!(slots.open_count(), 0);
    assert_eq!(slots.peak_open(), 16);
    // One demux thread per open pipeline (no video).
    assert!(
        slots.peak_threads() <= 16,
        "{} threads",
        slots.peak_threads()
    );
    // A suspended pipeline plays again.
    let p = &mut pipelines[500];
    assert_eq!(
        p.metadata().map(|m| m.timeline.duration_us),
        Some(Some(2_000_000))
    );
    let played = play_virtual(p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
}

#[test]
fn a_suspended_pipeline_keeps_its_frame_and_position_and_reopens() {
    let name = "mp4-h264-high-aac.mp4";
    let slots = PipelineSlots::new(2);
    let waker = Arc::new(TestWaker::default());
    let open = || {
        let mut o = options(&waker);
        o.slots = slots.clone();
        open_fixture_with(name, o)
    };
    let mut p0 = open();
    let mut p1 = open();
    let p2 = open();
    assert!(p2.is_waiting());
    let mut reports = Vec::new();
    settle_update(&mut p0, &waker, 0, &mut reports);
    p0.seek(1_010_000);
    settle_update(&mut p0, &waker, 0, &mut reports);
    assert!(reports.contains(&Report::SeekDone {
        media_us: 1_010_000
    }));
    p0.suspend(0);
    assert!(p0.is_suspended());
    let frame = p0.current_frame().expect("the frame stays").clone();
    assert_eq!(frame.pts_us, 1_000_000);
    assert!(frame.image.is_some());
    assert_eq!(p0.position(), 1_010_000);
    assert!(p0.metadata().is_some());
    // p0's place went to p2.
    wait_until("p2 opens", || !p2.is_waiting());
    // Play: p0 waits for a place, without pending work.
    p0.play(0);
    assert!(p0.is_waiting());
    assert!(!p0.has_pending_work());
    p1.suspend(0);
    // p0 reopens and seeks to its position, without new reports.
    let mut reports = Vec::new();
    settle_update(&mut p0, &waker, 0, &mut reports);
    assert!(
        reports
            .iter()
            .all(|r| matches!(r, Report::Buffered(_) | Report::TimeUpdate { .. })),
        "{reports:?}"
    );
    assert_eq!(p0.current_frame().map(|f| f.pts_us), Some(1_000_000));
    let played = play_virtual(&mut p0, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    assert_eq!(played.timeupdates.first(), Some(&1_260_000));
    drop(p2);
}

fn open_fixture_with(name: &str, o: swb_media::pipeline::PipelineOptions) -> Pipeline {
    Pipeline::open(SourceOpener::file(common::fixture(name)), o)
}

/// Waits until the thread counters stop changing; returns them.
fn quiet(p: &Pipeline) -> PipelineStats {
    let started = Instant::now();
    loop {
        let a = p.stats();
        std::thread::sleep(Duration::from_millis(100));
        if p.stats() == a {
            return a;
        }
        assert!(started.elapsed() < HANG, "the threads do not go quiet");
    }
}

#[test]
fn paused_and_ended_pipelines_do_no_thread_work() {
    // Paused after the load.
    let (mut p, waker, _) = open_settled("mp4-h264-aac-long.mp4");
    let before = quiet(&p);
    let notified = waker.count();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(p.stats(), before, "no wake-ups while paused");
    assert_eq!(waker.count(), notified);
    assert_eq!(p.next_due(), None);
    // Paused while playing.
    let played = play_virtual(&mut p, &waker, 0, 3_000_000);
    p.pause(played.now);
    let before = quiet(&p);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(p.stats(), before, "no wake-ups while paused");
    assert_eq!(p.next_due(), None);
    // Ended.
    let played = play_virtual(&mut p, &waker, played.now, i64::MAX);
    assert_eq!(played.ended, Some(10_000_000));
    let before = quiet(&p);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(p.stats(), before, "no wake-ups after the end");
    assert_eq!(p.next_due(), None);
    assert!(!p.has_pending_work());
}

#[test]
fn a_seek_drops_the_frame_that_waits_for_room_unconverted() {
    // Paused after the load: the frame queue is full (6 frames of 320 x
    // 240) and the video thread holds the next decoded frame, waiting for
    // room. A seek makes room; that frame is of the old epoch and must not
    // be converted.
    let (mut p, waker, mut reports) = open_settled("mp4-h264-aac-long.mp4");
    let before = quiet(&p);
    // The current frame and the queue.
    assert_eq!(before.frames_converted, 7);
    p.seek(5_010_000);
    settle_update(&mut p, &waker, 0, &mut reports);
    assert!(reports.contains(&Report::SeekDone {
        media_us: 5_010_000
    }));
    let after = quiet(&p);
    // The seek's frame and the queue again: nothing of the old epoch.
    assert_eq!(after.frames_converted - before.frames_converted, 7);
}

#[test]
fn a_source_that_stalls_forever_fails_after_the_limit() {
    let name = "mp4-h264-high-aac.mp4";
    let size = fixture_bytes(name).len() as u64;
    let limit = Duration::from_millis(300);
    // Virtual time: the clock does not move while work is pending, and no
    // thread wakes the owner; the watchdog does.
    let gate = Arc::new(Gate::default());
    let waker = Arc::new(TestWaker::default());
    let slots = PipelineSlots::new(4);
    let mut o = options(&waker);
    o.slots = slots.clone();
    o.stall_limit = limit;
    let mut p = Pipeline::open(stalling(name, size * 6 / 10, &gate), o);
    let started = Instant::now();
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert!(started.elapsed() >= limit);
    assert_eq!(error_code(&played.reports), Some(3), "{played:?}");
    assert!(p.is_failed());
    assert!(played.ended.is_none());
    // The cancel ended the blocked read: the threads end.
    wait_until("the threads end", || slots.live_threads() == 0);
    assert_eq!(slots.open_count(), 0);
    assert_eq!(slots.detached_threads(), 0);
    // Real time: an underrun, then the error.
    let gate = Arc::new(Gate::default());
    let mut o = options(&waker);
    o.stall_limit = limit;
    let mut p = Pipeline::open(stalling(name, size * 6 / 10, &gate), o);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    p.play(0);
    let mut now = 0;
    let started = Instant::now();
    while error_code(&reports).is_none() {
        assert!(started.elapsed() < HANG, "no error: {reports:?}");
        let seen = waker.count();
        now += 40_000;
        reports.extend(p.update(now));
        waker.wait_past(seen, Duration::from_millis(20));
    }
    assert_eq!(error_code(&reports), Some(3));
    let underrun = reports
        .iter()
        .position(|r| matches!(r, Report::Underrun { .. }))
        .expect("an underrun first");
    let error = reports
        .iter()
        .position(|r| matches!(r, Report::Error(_)))
        .expect("an error");
    assert!(underrun < error);
    assert!(started.elapsed() >= limit);
}

#[test]
fn stuck_threads_are_detached_and_bounded() {
    // Sources whose reads ignore the cancel: the threads stay blocked in
    // the open after their pipelines failed.
    let name = "mp4-h264-high-aac.mp4";
    let gate = Arc::new(Gate::default());
    let waker = Arc::new(TestWaker::default());
    let slots = PipelineSlots::new(16);
    let open = || {
        let mut o = options(&waker);
        o.slots = slots.clone();
        o.stall_limit = Duration::from_millis(200);
        Pipeline::open(stalling_with(name, 0, &gate, None), o)
    };
    let mut stuck: Vec<Pipeline> = (0..16).map(|_| open()).collect();
    let mut failed = vec![None; stuck.len()];
    let started = Instant::now();
    while failed.iter().any(Option::is_none) {
        assert!(started.elapsed() < HANG, "{failed:?}");
        let seen = waker.count();
        for (p, f) in stuck.iter_mut().zip(&mut failed) {
            if f.is_none() {
                // Opening: pending work (the owner needs data).
                let _ = p.has_pending_work();
                *f = error_code(&p.update(0));
            }
        }
        waker.wait_past(seen, Duration::from_millis(20));
    }
    assert!(failed.iter().all(|f| *f == Some(3)), "{failed:?}");
    // The places are free; the threads still run, detached.
    assert_eq!(slots.open_count(), 0);
    assert_eq!(slots.detached_threads(), 16);
    assert_eq!(slots.live_threads(), 16);
    // At the bound, a new pipeline fails at once, without a thread.
    let mut q = open();
    let mut reports = Vec::new();
    settle_update(&mut q, &waker, 0, &mut reports);
    assert_eq!(error_code(&reports), Some(3), "{reports:?}");
    assert_eq!(slots.live_threads(), 16);
    // The reads return: the detached threads end.
    gate.open();
    wait_until("the detached threads end", || slots.live_threads() == 0);
    assert_eq!(slots.detached_threads(), 0);
    drop(stuck);
    // A new pipeline plays.
    let mut r = open();
    let played = play_virtual(&mut r, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000), "{played:?}");
}

#[test]
fn an_external_consumer_reads_the_ring() {
    let waker = Arc::new(TestWaker::default());
    let mut o = options(&waker);
    o.output = AudioOutput::External;
    // Audio alone: no video thread wakes the demux thread.
    let mut p = open_fixture_with("mp3-long.mp3", o);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    let ring = p.samples().expect("a ring");
    wait_until("the ring fills", || ring.available() == ring.capacity());
    let base = ring.base().expect("a base");
    assert_eq!(base.epoch, 0);
    // Playing: the pipeline does not consume (one consumer only).
    p.play(0);
    let read = ring.read_position();
    let _ = p.update(200_000);
    assert_eq!(ring.read_position(), read);
    assert_eq!(p.take_captured_audio().len(), 0);
    // The consumer takes 0.5 s; the next update wakes the demux thread,
    // which fills the ring again.
    let mut buf = vec![0.0f32; 2 * 24_000];
    assert_eq!(ring.pop(&mut buf), 24_000);
    assert_eq!(
        base.media_time(ring.read_position()),
        base.media_us + 500_000
    );
    let _ = p.update(400_000);
    wait_until("the ring fills again", || {
        ring.available() == ring.capacity()
    });
}

#[test]
fn an_external_consumer_that_reads_before_the_owner_looks() {
    // The consumer reads between the owner's `update` and its
    // `has_pending_work`: the demux thread waits for room that is there,
    // and only the owner can wake it.
    let waker = Arc::new(TestWaker::default());
    let mut o = options(&waker);
    o.output = AudioOutput::External;
    // The watchdog would wake the thread too, and hide a missing wake-up.
    o.stall_limit = Duration::from_secs(3600);
    let mut p = open_fixture_with("mp3-long.mp3", o);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    let ring = p.samples().expect("a ring");
    wait_until("the ring fills", || ring.available() == ring.capacity());
    quiet(&p);
    p.play(0);
    // The next due time (the `timeupdate` at 1.15 s) is past the samples
    // in the ring (1 s); the full ring is enough while nobody reads.
    let _ = p.update(900_000);
    assert!(!p.has_pending_work());
    let mut buf = vec![0.0f32; 2 * 24_000];
    assert_eq!(ring.pop(&mut buf), 24_000);
    // Now the samples for 1.15 s are missing; `has_pending_work` wakes the
    // demux thread, which writes them.
    assert!(
        settle_within(&p, &waker, HANG),
        "the pipeline does not settle"
    );
    let r = p.update(1_150_000);
    assert!(
        !r.iter().any(|r| matches!(r, Report::Underrun { .. })),
        "{r:?}"
    );
}

#[test]
fn a_pipeline_stalled_on_its_owner_survives_a_busy_owner() {
    // The owner asks once while the pipeline opens (pending work), then
    // does not look for three limits while the threads fill the queues and
    // wait for it (a 4 KiB packet bound and no audio before 3 s in the
    // file): waiting for the owner is not a stall.
    let name = "mp4-h264-aac-late-audio.mp4";
    let gate = Arc::new(Gate::default());
    let waker = Arc::new(TestWaker::default());
    let mut o = options(&waker);
    o.packet_bytes = 4096;
    o.stall_limit = Duration::from_millis(200);
    // Every read waits at the gate until it opens.
    let mut p = Pipeline::open(stalling(name, 0, &gate), o);
    assert!(p.has_pending_work(), "opening");
    gate.open();
    wait_until("the frame queue fills", || p.stats().frames_decoded >= 7);
    std::thread::sleep(Duration::from_millis(600));
    let reports = p.update(0);
    assert_eq!(error_code(&reports), None, "{reports:?}");
    assert!(reports.contains(&Report::FirstFrame), "{reports:?}");
}

#[test]
fn the_limit_counts_from_when_the_owner_needs_data() {
    // Audio alone: after the first data the demux thread blocks in a read
    // of the source (at 60 % of the file), so nothing makes progress. The
    // owner idles for three limits, then plays: when it reaches the end of
    // the data, the limit counts from then, not from the open.
    let name = "mp4-h264-high-aac.mp4";
    let size = fixture_bytes(name).len() as u64;
    let limit = Duration::from_millis(200);
    let gate = Arc::new(Gate::default());
    let waker = Arc::new(TestWaker::default());
    let mut o = options(&waker);
    o.video = false;
    o.stall_limit = limit;
    let mut p = Pipeline::open(stalling(name, size * 6 / 10, &gate), o);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    wait_until("a read blocks", || gate.reached());
    std::thread::sleep(3 * limit);
    assert_eq!(error_code(&p.update(0)), None);
    let started = Instant::now();
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(error_code(&played.reports), Some(3), "{played:?}");
    assert!(played.timeupdates.len() >= 2, "{played:?}");
    assert!(
        started.elapsed() >= limit,
        "failed after {:?}",
        started.elapsed()
    );
}

#[test]
fn a_seek_after_the_end_is_pending_until_its_frame_comes() {
    // At the end the demux thread has read everything and the video thread
    // has drained: both wait. A seek is pending work at once (the threads
    // wait no longer for the owner) until its frame comes; the update must
    // not end it without the frame. (Under load the threads may finish the
    // seek before this thread asks; then there is no pending work, and the
    // frame is there.)
    let waker = Arc::new(TestWaker::default());
    let mut p = open_fixture("mp4-h264-high-aac.mp4", &waker);
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    for (target, frame) in [(500_000, 480_000), (1_210_000, 1_200_000), (40_000, 40_000)] {
        quiet(&p);
        p.seek(target);
        if p.has_pending_work() {
            settle(&p, &waker);
        }
        let mut reports = p.update(0);
        assert!(
            reports.contains(&Report::SeekDone { media_us: target }),
            "{reports:?}"
        );
        assert_eq!(p.current_frame().map(|f| f.pts_us), Some(frame));
        // Back to the end of the data.
        p.seek(5_000_000);
        settle_update(&mut p, &waker, 0, &mut reports);
    }
}

#[test]
fn a_dropped_pipeline_gives_its_place_back() {
    let slots = PipelineSlots::new(1);
    let waker = Arc::new(TestWaker::default());
    let mut o = options(&waker);
    o.slots = slots.clone();
    let p = open_fixture_with("mp4-h264-high-aac.mp4", o.clone());
    let mut q = open_fixture_with("ogg-opus.opus", o);
    assert!(q.is_waiting());
    drop(p);
    let played = play_virtual(&mut q, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_006_500));
    drop(q);
    wait_until("the threads end", || slots.live_threads() == 0);
}

/// How long the last thread of an ended run waits before it hands its place
/// on, in the tests of the handover.
const HANDOVER_DELAY: Duration = Duration::from_millis(1000);

/// Follows the handover to the waiting pipeline `b`: its place is in
/// transit from the end of another run (which the caller has caused) until
/// `b` has it, and `b` waits for it with `awaits_handover` true throughout
/// (M14 feature 6a: an owner in virtual time does not move the clock then).
fn follow_handover(b: &Pipeline, slots: &PipelineSlots) {
    let started = Instant::now();
    while !b.awaits_handover() {
        assert!(b.is_waiting(), "the place came without a transit");
        assert!(started.elapsed() < HANG, "the run does not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    while b.awaits_handover() {
        assert!(started.elapsed() < HANG, "the place does not come");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!b.is_waiting(), "the transit ended without the place");
    // The transit ends just after `b` has the place (`Slot::drop` hands the
    // place on, then ends the transit and wakes the waiters), so this thread
    // may look between the two steps (slow threads, the sanitizers).
    wait_until("the transit ends", || slots.in_transit() == 0);
}

#[test]
fn a_run_that_fails_hands_its_place_on_through_a_transit() {
    // The demux thread fails to open the file (`Shared::fail`): the run
    // ends and the place stays in transit while the thread ends.
    let slots = PipelineSlots::new(1);
    slots.set_handover_delay(HANDOVER_DELAY);
    let waker_a = Arc::new(TestWaker::default());
    let waker_b = Arc::new(TestWaker::default());
    let mut oa = options(&waker_a);
    oa.slots = slots.clone();
    let mut a = open_fixture_with("garbage.mp4", oa);
    let mut ob = options(&waker_b);
    ob.slots = slots.clone();
    let b = open_fixture_with("mp4-h264-high-aac.mp4", ob);
    assert!(b.is_waiting());
    follow_handover(&b, &slots);
    let mut reports = Vec::new();
    settle_update(&mut a, &waker_a, 0, &mut reports);
    assert!(error_code(&reports).is_some(), "{reports:?}");
    drop(b);
}

#[test]
fn a_run_that_the_owner_suspends_hands_its_place_on_through_a_transit() {
    let slots = PipelineSlots::new(1);
    slots.set_handover_delay(HANDOVER_DELAY);
    let waker_a = Arc::new(TestWaker::default());
    let waker_b = Arc::new(TestWaker::default());
    let mut oa = options(&waker_a);
    oa.slots = slots.clone();
    let mut a = open_fixture_with("mp4-h264-high-aac.mp4", oa);
    let mut ob = options(&waker_b);
    ob.slots = slots.clone();
    let b = open_fixture_with("mp4-h264-high-aac.mp4", ob);
    assert!(b.is_waiting());
    a.suspend(0);
    follow_handover(&b, &slots);
    drop(b);
}

#[test]
fn a_run_that_the_watchdog_detaches_wakes_every_waiter() {
    // The run's thread blocks in the source (it opens), and the owner
    // waits for data: the watchdog fails the run and detaches it, so the
    // place goes on at once, and the owners of the waiters are woken, also
    // the one that does not get the place.
    let name = "mp4-h264-high-aac.mp4";
    let slots = PipelineSlots::new(1);
    slots.set_handover_delay(HANDOVER_DELAY);
    let gate = Arc::new(Gate::default());
    let waker_a = Arc::new(TestWaker::default());
    let waker_b = Arc::new(TestWaker::default());
    let waker_c = Arc::new(TestWaker::default());
    let mut oa = options(&waker_a);
    oa.slots = slots.clone();
    oa.stall_limit = Duration::from_millis(200);
    let a = Pipeline::open(stalling(name, 0, &gate), oa);
    let open_waiter = |waker: &Arc<TestWaker>| {
        let mut o = options(waker);
        o.slots = slots.clone();
        open_fixture_with(name, o)
    };
    let b = open_waiter(&waker_b);
    let c = open_waiter(&waker_c);
    assert!(b.is_waiting() && c.is_waiting());
    assert!(a.has_pending_work(), "opening");
    wait_until("b has the place", || !b.is_waiting());
    wait_until("c is woken", || waker_c.count() > 0);
    assert!(c.is_waiting());
    assert_eq!(slots.in_transit(), 0);
    drop((a, b, c));
}
