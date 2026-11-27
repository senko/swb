//! Measurements of the media pipeline for feature 10's budgets (not limits
//! here): decode and convert throughput of the real files, real-time
//! playback on a monotonic clock (dropped frames), and the CPU time while
//! paused. The real files are local fixtures (`fixtures/js/media/`, not
//! committed); the tests are ignored by default. Run them in release mode:
//!
//! `cargo test --release -p swb-media --features ffmpeg --test pipeline_bench -- --ignored --nocapture --test-threads 1`
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::driver::{TestWaker, options, play_virtual, settle_update};
use common::repo_path;
use swb_media::pipeline::{Pipeline, Report, SourceOpener};

const FILES: [&str; 3] = [
    "fixtures/js/media/mux-1080p30-20s.mp4",
    "fixtures/js/media/wikimedia-web-browser-demo.webm",
    "fixtures/js/media/mux-highest.mp4",
];

fn open(path: &str, waker: &Arc<TestWaker>) -> Option<Pipeline> {
    let path = repo_path(path);
    if !path.exists() {
        eprintln!("skipped: {} is not there", path.display());
        return None;
    }
    Some(Pipeline::open(SourceOpener::file(path), options(waker)))
}

/// The CPU time of the process (user and system), from `/proc/self/stat`.
fn cpu_time() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .unwrap_or_default();
    // Fields 14 and 15 of the file; the 12th and 13th after the name.
    let ticks: u64 = fields
        .get(11..13)
        .map_or(0, |f| f.iter().filter_map(|v| v.parse::<u64>().ok()).sum());
    Duration::from_millis(ticks * 10)
}

#[test]
#[ignore = "a measurement with local files; run in release mode"]
fn throughput_in_virtual_time() {
    for path in FILES {
        for painted in [true, false] {
            let waker = Arc::new(TestWaker::default());
            let Some(mut p) = open(path, &waker) else {
                continue;
            };
            p.set_painted(painted);
            let mut reports = Vec::new();
            settle_update(&mut p, &waker, 0, &mut reports);
            let started = Instant::now();
            let cpu = cpu_time();
            let played = play_virtual(&mut p, &waker, 0, 8_000_000);
            let wall = started.elapsed();
            let cpu = cpu_time().saturating_sub(cpu);
            let frames = p.quality().total_frames;
            eprintln!(
                "{path} painted={painted}: {frames} frames to {:.2} s in {:.2} s: {:.0} fps, CPU {:.2} s ({:.0} % of one core per second of media)",
                played.steps.last().map_or(0.0, |s| s.1 as f64 / 1e6),
                wall.as_secs_f64(),
                frames as f64 / wall.as_secs_f64(),
                cpu.as_secs_f64(),
                100.0 * cpu.as_secs_f64() / 8.0,
            );
        }
    }
}

#[test]
#[ignore = "a measurement with local files; run in release mode"]
fn real_time_playback_and_paused_cpu() {
    for path in FILES {
        let waker = Arc::new(TestWaker::default());
        let Some(mut p) = open(path, &waker) else {
            continue;
        };
        let mut reports = Vec::new();
        settle_update(&mut p, &waker, 0, &mut reports);
        let clock = Instant::now();
        let now = || i64::try_from(clock.elapsed().as_micros()).unwrap_or(i64::MAX);
        let cpu = cpu_time();
        p.play(now());
        let mut underruns = 0;
        let mut late_us = 0i64;
        let seconds = 5;
        while clock.elapsed() < Duration::from_secs(seconds) && !p.is_ended() {
            // As the GUI: wait until the due time or a notification.
            let seen = waker.count();
            let due = p.next_due();
            match due {
                Some(due) => {
                    let wait = due - now();
                    if wait > 0
                        && waker.wait_past(seen, Duration::from_micros(wait.unsigned_abs())) > seen
                    {
                        continue;
                    }
                    late_us = late_us.max(now() - due);
                }
                None => {
                    waker.wait_past(seen, Duration::from_millis(100));
                }
            }
            for r in p.update(now()) {
                if matches!(r, Report::Underrun { .. }) {
                    underruns += 1;
                }
            }
        }
        let playing_cpu = cpu_time().saturating_sub(cpu);
        let q = p.quality();
        eprintln!("{path}: {:?}", p.stats());
        p.pause(now());
        // Paused: wait until the threads are quiet, then measure 3 s.
        std::thread::sleep(Duration::from_millis(500));
        let stats = p.stats();
        let cpu = cpu_time();
        std::thread::sleep(Duration::from_secs(3));
        let paused_cpu = cpu_time().saturating_sub(cpu);
        eprintln!(
            "{path}: {seconds} s real time: {} frames, {} dropped, {underruns} underruns, worst lateness {:.1} ms, CPU {:.0} % of one core; paused 3 s: CPU {} ms, thread wake-ups {}",
            q.total_frames,
            q.dropped_frames,
            late_us as f64 / 1000.0,
            100.0 * playing_cpu.as_secs_f64() / seconds as f64,
            paused_cpu.as_millis(),
            {
                let s = p.stats();
                (s.demux_wakeups + s.video_wakeups) - (stats.demux_wakeups + stats.video_wakeups)
            },
        );
    }
}
