//! The audio output with a real pipeline (M14 feature 8, ADR 0047 sections
//! 7 and 8): the fake device pulls frames from the mixer, the pipeline's
//! media time follows the audio clock, an empty ring is an underrun, a busy
//! page thread does not starve the ring, the end of the audio is seen.
//! No PipeWire: the device is `output::fake`.
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::driver::{HANG, TestWaker, open_fixture, open_settled, settle_update};
use swb_media::ClockSource;
use swb_media::output::fake::FakeDevice;
use swb_media::output::{ManualClock, MonoClock, Output, Source, lead_frames};
use swb_media::pipeline::{AudioOutput, Pipeline, Report};

const CYCLE: usize = 512;
/// The time of one cycle on the manual clock (512 frames at 48 kHz).
const CYCLE_NS: u64 = 10_666_667;

struct Rig {
    output: Arc<Output>,
    device: FakeDevice,
    clock: Arc<ManualClock>,
}

fn rig() -> Rig {
    let device = FakeDevice::new();
    let clock = Arc::new(ManualClock::default());
    clock.advance_ms(1);
    let output = Output::manual(
        Box::new(device.clone()),
        Arc::clone(&clock) as Arc<dyn MonoClock>,
    );
    output.request(None, true);
    assert!(output.is_ready());
    Rig {
        output,
        device,
        clock,
    }
}

/// A pipeline that plays `name` from 0 on the audio clock of a source in the
/// mixer (what the engine's `Fetch` does at the start of a playback).
fn playing(rig: &Rig, name: &str, waker: &Arc<TestWaker>) -> (Pipeline, Source) {
    let mut p = open_fixture(name, waker);
    let mut reports = Vec::new();
    settle_update(&mut p, waker, 0, &mut reports);
    p.play(0);
    // The data for the first 200 ms is decoded before the device starts.
    settle_update(&mut p, waker, 0, &mut reports);
    assert!(p.is_clock_running());
    let ring = p.samples().expect("an audio track");
    p.set_output(AudioOutput::External);
    let lead = lead_frames(&ring, p.position());
    let source = rig.output.attach(ring, lead).expect("joins");
    p.rebase_clock(ClockSource::Audio, 0, source.clock_us());
    (p, source)
}

fn us(frames: i64) -> i64 {
    frames * 1_000_000 / 48_000
}

#[test]
fn the_media_time_follows_the_frames_that_the_mixer_took() {
    let waker = Arc::new(TestWaker::default());
    let r = rig();
    let (mut p, source) = playing(&r, "mp3-long.mp3", &waker);
    let start = p.position();
    for cycle in 1..=40i64 {
        r.clock.advance_ns(CYCLE_NS);
        r.device.pull(CYCLE, 512).expect("runs");
        let reports = p.update(source.clock_us());
        // The frames taken, less the 512 that are in the device.
        let expected = start + us((cycle * 512 - 512).max(0));
        let got = p.position();
        assert!(
            (got - expected).abs() <= 1,
            "cycle {cycle}: media time {got}, expected {expected}; clock {}; {reports:?}",
            source.clock_us()
        );
    }
    assert!(!p.is_underrun());
}

#[test]
fn a_busy_page_thread_does_not_starve_the_ring() {
    let waker = Arc::new(TestWaker::default());
    let r = rig();
    let (p, source) = playing(&r, "mp3-long.mp3", &waker);
    let before = p.stats().demux_wakeups;
    // The page thread is busy for 3 s: nobody calls `update`; the device
    // pulls 512 frames every 10.7 ms of real time. A ring of 1 s would run
    // dry after 1 s if only the owner refilled it.
    let started = Instant::now();
    let mut cycles = 0u64;
    let mut starved = 0u64;
    while started.elapsed() < Duration::from_millis(3000) {
        r.clock.advance_ns(CYCLE_NS);
        r.device.pull(CYCLE, 512).expect("runs");
        cycles += 1;
        if source.is_starved() {
            starved += 1;
        }
        std::thread::sleep(Duration::from_micros(10_000));
    }
    assert!(cycles > 200, "{cycles} cycles");
    assert_eq!(
        starved, 0,
        "the ring ran dry in {starved} of {cycles} cycles"
    );
    // What it costs: the demux thread's wake-ups per second (timed waits).
    let wakeups = p.stats().demux_wakeups - before;
    let per_second = wakeups as f64 / started.elapsed().as_secs_f64();
    println!("demux thread: {wakeups} wake-ups in 3 s ({per_second:.1} a second)");
    assert!(per_second < 10.0, "{per_second:.1} wake-ups a second");
    // The page thread is back: the media time is where the frames are.
    let mut p = p;
    p.update(source.clock_us());
    assert!(p.position() > 1_500_000, "{}", p.position());
    assert!(!p.is_underrun());
}

#[test]
fn an_empty_ring_is_an_underrun_and_the_clock_goes_on_from_where_it_stopped() {
    let waker = Arc::new(TestWaker::default());
    let r = rig();
    let (mut p, source) = playing(&r, "mp3-long.mp3", &waker);
    // The device takes frames much faster than real time, so faster than the
    // demux thread refills the ring of 1 s: the ring runs dry. The owner
    // looks after every few cycles (the demux thread writes at the same
    // time, so the ring is empty at one of the looks).
    let mut stopped_at = None;
    for _ in 0..2000 {
        for _ in 0..8 {
            r.clock.advance_ns(CYCLE_NS);
            r.device.pull(CYCLE, 512).expect("runs");
        }
        let at = source.clock_us();
        for report in p.update(at) {
            if let Report::Underrun { media_us } = report {
                stopped_at = Some((media_us, at));
            }
        }
        if stopped_at.is_some() {
            break;
        }
    }
    let (stopped_at, at) = stopped_at.expect("an underrun");
    assert!(p.is_underrun() && !p.is_clock_running());
    assert_eq!(stopped_at, at);
    // The clock stopped where the sound stopped: at the audio clock, which
    // stands at the end of the frames that the device took, less its delay.
    assert!(stopped_at > 300_000, "{stopped_at}");
    // The decoder refills the ring; when 250 ms are there, playback goes on
    // where it stopped.
    let started = Instant::now();
    let mut resumed = None;
    while resumed.is_none() {
        assert!(started.elapsed() < HANG, "no resume");
        std::thread::sleep(Duration::from_millis(20));
        for report in p.update(source.clock_us()) {
            if let Report::Resumed { media_us } = report {
                resumed = Some(media_us);
            }
        }
    }
    let resumed = resumed.expect("resumed");
    assert!(
        (resumed - stopped_at).abs() <= 1,
        "{resumed} vs {stopped_at}"
    );
    assert!(p.is_clock_running() && !p.is_underrun());
    // And it goes on: the next cycles move the media time.
    for _ in 0..10 {
        r.clock.advance_ns(CYCLE_NS);
        r.device.pull(CYCLE, 512).expect("runs");
    }
    p.update(source.clock_us());
    let moved = p.position() - resumed;
    assert!((moved - us(10 * 512)).abs() <= 25_000, "moved {moved}");
}

/// A loop: after the playback ended and the element seeks to the start, the
/// audio track has not finished (it plays again) until it is decoded to its
/// end again.
#[test]
fn the_audio_has_not_finished_after_a_seek_to_the_start() {
    let mut failed = Vec::new();
    for name in [
        "wav-pcm-s16.wav",
        "mp4-h264-high-aac.mp4",
        "m4a-aac.m4a",
        "ogg-vorbis.ogg",
        "flac.flac",
        "mp3-long.mp3",
        "mp3.mp3",
        "aac-adts.aac",
    ] {
        let (mut p, waker, mut reports) = open_settled(name);
        // Play to the end in virtual time (the silent output consumes the ring).
        let played = common::driver::play_virtual(&mut p, &waker, 0, i64::MAX);
        assert!(played.ended.is_some(), "{name}");
        p.seek(0);
        settle_update(&mut p, &waker, played.now, &mut reports);
        p.play(played.now);
        settle_update(&mut p, &waker, played.now, &mut reports);
        if p.audio_finished() {
            failed.push(name);
        }
    }
    assert!(
        failed.is_empty(),
        "the audio decodes again after the seek, it has not finished: {failed:?}"
    );
}

/// A loop in virtual time: every file decodes as many samples in its second
/// round as in its first. (MP3 and ADTS decoded none: their demuxers refuse
/// a seek to the negative time `0 - preroll`, and the demuxer stayed at its
/// end.)
#[test]
fn a_looping_file_decodes_as_many_samples_in_the_second_round() {
    for name in [
        "mp3-long.mp3",
        "mp3.mp3",
        "mp3-truncated.mp3",
        "aac-adts.aac",
        "wav-pcm-s16.wav",
        "ogg-vorbis.ogg",
        "ogg-opus.opus",
        "flac.flac",
        "m4a-aac.m4a",
    ] {
        let (mut p, waker, mut reports) = open_settled(name);
        let first = common::driver::play_virtual(&mut p, &waker, 0, i64::MAX);
        assert!(first.ended.is_some(), "{name}");
        let round1 = p.take_captured_audio().len();
        assert!(round1 > 10_000, "{name}: round 1 has {round1} samples");
        p.seek(0);
        settle_update(&mut p, &waker, first.now, &mut reports);
        let second = common::driver::play_virtual(&mut p, &waker, first.now, i64::MAX);
        assert!(second.ended.is_some(), "{name}: round 2 ends");
        let round2 = p.take_captured_audio().len();
        assert!(
            round2.abs_diff(round1) * 20 <= round1,
            "{name}: round 1 has {round1} samples, round 2 has {round2}"
        );
    }
}

/// A seek to 0.1 s of an MP3 (under the 200 ms preroll) lands at 0.1 s: the
/// samples after it are the samples of a play from 0, from the frame 4800 on.
#[test]
fn a_seek_to_100_ms_in_an_mp3_lands_at_100_ms() {
    for name in ["mp3-long.mp3", "mp3.mp3", "aac-adts.aac"] {
        let waker = Arc::new(TestWaker::default());
        let mut whole = open_fixture(name, &waker);
        let mut reports = Vec::new();
        settle_update(&mut whole, &waker, 0, &mut reports);
        let played = common::driver::play_virtual(&mut whole, &waker, 0, i64::MAX);
        assert!(played.ended.is_some(), "{name}");
        let reference = whole.take_captured_audio();

        let mut p = open_fixture(name, &waker);
        settle_update(&mut p, &waker, 0, &mut reports);
        p.seek(100_000);
        settle_update(&mut p, &waker, 0, &mut reports);
        assert_eq!(p.position(), 100_000, "{name}");
        let played = common::driver::play_virtual(&mut p, &waker, 0, i64::MAX);
        assert!(played.ended.is_some(), "{name}");
        let after = p.take_captured_audio();
        let skipped = 4800 * 2;
        assert!(
            reference.len() > skipped + 20_000 && after.len() > 20_000,
            "{name}: {} and {} samples",
            reference.len(),
            after.len()
        );
        // The same samples (the decoders start from the same packet).
        let n = 20_000;
        let worst = (0..n)
            .map(|i| (after[i] - reference[skipped + i]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.01, "{name}: the samples differ by {worst}");
        assert!(
            (after.len() + skipped).abs_diff(reference.len()) <= 4800 * 2,
            "{name}: {} + {skipped} vs {}",
            after.len(),
            reference.len()
        );
    }
}

#[test]
fn the_end_of_the_audio_track_is_seen_when_the_ring_is_empty() {
    let waker = Arc::new(TestWaker::default());
    let r = rig();
    let (p, source) = playing(&r, "wav-pcm-s16.wav", &waker);
    assert!(!p.audio_finished());
    // 2 s of audio: pull until the mixer has taken it all.
    let started = Instant::now();
    while !p.audio_finished() {
        assert!(started.elapsed() < HANG, "the audio does not end");
        r.clock.advance_ns(CYCLE_NS);
        r.device.pull(CYCLE, 512).expect("runs");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Everything was taken, once: 2 s of frames (+ the last cycle's rest).
    let taken = source.clock_us() + us(512);
    assert!((taken - 2_000_000).abs() < 30_000, "{taken}");
}
