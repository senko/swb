//! M14 feature 8 (ADR 0047 sections 7 and 8) with the feature `media`: the
//! audio output of the media element, with a fake device that a thread of the
//! test drives in real time (no PipeWire, no sound). A playing element with
//! audio joins the mixer and takes its media time from it; it leaves at
//! pause, seek and end; `volume` and `muted` change its gain; 32 sources are
//! mixed and the rest play on the silent output; a failed device or no
//! device leaves the elements on the page clock without a jump; a page that
//! goes away frees its slots; a busy page thread does not starve the audio.
//! Run by `just test-media`.

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::media_probe::evaluate_strict as evaluate;
use common::media_site::{MP4, MediaSite, VIEW};
use common::{new_page, on_page_thread};
use serde_json::json;
use swb_engine::{ClockMode, Page, PageClock, Url};
use swb_media::output::fake::FakeDevice;
use swb_media::output::{DeviceState, ManualClock, MonoClock, Output, Phase, SystemClock};
use swb_net::Fetcher;

const FOLDER: &str = "audio-1";
// `MP4` (320 x 240, 25 fps, 2 s, H.264 and AAC) is a sine of 440 Hz left and
// 660 Hz right.
/// 10 s of the same sines.
const MP3: &str = "fixtures/media/mp3-long.mp3";

const HELPERS: &str = "<script>\
    const sleep = ms => new Promise(r => setTimeout(r, ms));\
    const once = (m, n, ms = 12000) => new Promise(r => { const to = setTimeout(() => r('timeout'), ms); m.addEventListener(n, () => { clearTimeout(to); r(n); }, {once: true}); });\
    </script>";

/// The fake device with a thread that pulls a cycle of 512 frames every
/// `period` of real time, as PipeWire's real-time thread does.
struct Device {
    fake: FakeDevice,
    output: Arc<Output>,
    /// Frames pulled so far.
    pulled: Arc<AtomicU64>,
    /// The peak of the last cycle, as `f32` bits.
    peak: Arc<AtomicU32>,
    /// The period of a cycle, in microseconds.
    period_us: Arc<AtomicU64>,
    /// Cycles in which the first source was starved.
    starved: Arc<AtomicU64>,
    /// The delay that the device reports, in frames.
    delay: Arc<AtomicU64>,
    /// The thread pulls a cycle with this lock held: a test that holds it
    /// stops the device between two cycles.
    gate: Arc<Mutex<()>>,
    /// The frames of the cycles pulled while this is `Some`.
    recording: Arc<Mutex<Option<Vec<f32>>>>,
    /// A manual clock that the thread advances by one period per cycle (the
    /// output's clock, so that the audio clock does not move while the
    /// device is held); `None`: the output has its own clock.
    clock: Option<Arc<ManualClock>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Device {
    fn start() -> Device {
        let fake = FakeDevice::new();
        let output = Output::new(Box::new(fake.clone()), Arc::new(SystemClock));
        Device::with(fake, output, None).run()
    }

    /// A device on an output with a manual clock that the device thread
    /// advances (no timer thread: the page's polls tick the output), and
    /// the delay `delay` frames.
    fn manual(delay: u64) -> Device {
        let fake = FakeDevice::new();
        let clock = Arc::new(ManualClock::default());
        clock.advance_ms(1);
        let output = Output::manual(
            Box::new(fake.clone()),
            Arc::clone(&clock) as Arc<dyn MonoClock>,
        );
        let device = Device::with(fake, output, Some(clock));
        device.delay.store(delay, Ordering::Relaxed);
        device.run()
    }

    fn with(fake: FakeDevice, output: Arc<Output>, clock: Option<Arc<ManualClock>>) -> Device {
        Device {
            fake,
            output,
            pulled: Arc::new(AtomicU64::new(0)),
            peak: Arc::new(AtomicU32::new(0)),
            period_us: Arc::new(AtomicU64::new(10_667)),
            starved: Arc::new(AtomicU64::new(0)),
            delay: Arc::new(AtomicU64::new(512)),
            gate: Arc::new(Mutex::new(())),
            recording: Arc::new(Mutex::new(None)),
            clock,
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    /// Stops the device between two cycles until the guard goes.
    fn hold(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap()
    }

    fn run(mut self) -> Device {
        let fake = self.fake.clone();
        let output = Arc::clone(&self.output);
        let (pulled, peak, period, starved, stop) = (
            Arc::clone(&self.pulled),
            Arc::clone(&self.peak),
            Arc::clone(&self.period_us),
            Arc::clone(&self.starved),
            Arc::clone(&self.stop),
        );
        let (delay, gate, recording, clock) = (
            Arc::clone(&self.delay),
            Arc::clone(&self.gate),
            Arc::clone(&self.recording),
            self.clock.clone(),
        );
        self.thread = Some(std::thread::spawn(move || {
            let mut next = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                next += Duration::from_micros(period.load(Ordering::Relaxed));
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
                let _cycle = gate.lock().unwrap();
                if let Some(clock) = &clock {
                    clock.advance_ns(period.load(Ordering::Relaxed) * 1000);
                }
                let delay = i64::try_from(delay.load(Ordering::Relaxed)).unwrap();
                if let Some(buf) = fake.pull(512, delay) {
                    if let Some(rec) = recording.lock().unwrap().as_mut() {
                        rec.extend_from_slice(&buf);
                    }
                    pulled.fetch_add(512, Ordering::Relaxed);
                    let max = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    peak.store(max.to_bits(), Ordering::Relaxed);
                    // The first source of the table (slot 0, attachment 1).
                    if output
                        .mix()
                        .reading(0, 1)
                        .is_some_and(|r| r.starved && r.taken > 0)
                    {
                        starved.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
        self
    }

    fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }

    fn pulled(&self) -> u64 {
        self.pulled.load(Ordering::Relaxed)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

/// A page in real time with the output of `device`.
fn page_with(device: Option<&Device>, body: &str) -> Page {
    let mut site = MediaSite::new(
        FOLDER,
        &format!("<!DOCTYPE html><style>body{{margin:0}}</style>{HELPERS}{body}"),
    );
    site.add_file("a.mp4", MP4);
    site.add_file("a.mp3", MP3);
    let site = Arc::new(site);
    let mut page = new_page(site as Arc<dyn Fetcher>, 2, VIEW);
    page.set_javascript(true);
    page.set_paints_frames(true);
    page.set_clock(PageClock::new(ClockMode::Real, None));
    page.isolate_media(Duration::ZERO);
    if let Some(device) = device {
        page.set_audio_output(Some(Arc::clone(&device.output)));
    }
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// Drives the page as the GUI's loop does, for `duration`.
fn drive(page: &mut Page, duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        page.process_network();
        page.pump();
        let now = Instant::now();
        match page.next_task_instant() {
            Some(t) if t > now => std::thread::sleep((t - now).min(Duration::from_millis(3))),
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(3)),
        }
    }
}

/// Drives until `done` holds, at most `limit`.
fn drive_until(page: &mut Page, limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() {
            return true;
        }
        drive(page, Duration::from_millis(5));
    }
    done()
}

/// Drives until the script `js` gives true, at most `limit`.
fn drive_until_js(page: &mut Page, limit: Duration, js: &str) -> bool {
    let end = Instant::now() + limit;
    loop {
        if evaluate(page, js) == json!(true) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        drive(page, Duration::from_millis(5));
    }
}

const READY: &str =
    "(async () => { if (v.readyState < 2) await once(v, 'loadeddata'); return v.readyState; })()";

/// The element follows the device: with the device at half speed, a 2 s
/// video ends after 4 s, when the device has pulled 2 s of frames. On the
/// page clock it would end after 2 s (mutation: without the audio clock).
#[test]
fn the_clock_of_an_element_is_the_device() {
    on_page_thread(|| {
        let device = Device::start();
        device.period_us.store(21_334, Ordering::Relaxed);
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp4 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        let started = Instant::now();
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        let mut ended = json!(false);
        while !ended.as_bool().unwrap() && started.elapsed() < Duration::from_secs(10) {
            drive(&mut page, Duration::from_millis(50));
            ended = evaluate(&mut page, "v.ended");
        }
        let elapsed = started.elapsed();
        assert_eq!(ended, json!(true));
        assert!(
            elapsed > Duration::from_millis(3600),
            "ended after {elapsed:?} at half speed"
        );
        // The device pulled at least the 2 s of the media (less its delay).
        assert!(device.pulled() >= 96_000 - 1024, "{}", device.pulled());
        // The element left the mixer at the end.
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 0));
    });
}

/// `currentTime` of a playing element is the frames that the device took,
/// less its delay: at each look, within the interpolation and the cycle.
#[test]
fn current_time_follows_the_pulled_frames() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        let before = device.pulled();
        for _ in 0..8 {
            drive(&mut page, Duration::from_millis(150));
            let low = device.pulled();
            let time = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
            let high = device.pulled();
            // The element started at about `before` frames ago: the time it
            // shows is the frames pulled since it joined, less the 512 in
            // the device and the interpolation (at most a cycle).
            let at_least = (low.saturating_sub(before) as f64 - 2048.0) / 48_000.0;
            let at_most = (high.saturating_sub(before) as f64 + 2048.0) / 48_000.0;
            assert!(
                time >= at_least.max(0.0) - 0.05 && time <= at_most + 0.05,
                "currentTime {time}, pulled since the join {}..{}",
                low - before,
                high - before
            );
        }
        // The device stops for 400 ms (a stall of the sound server): the
        // time stands still, within the delay's interpolation (about 11 ms).
        // The page clock would move 400 ms.
        let t0 = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        {
            let _held = device.hold();
            drive(&mut page, Duration::from_millis(400));
            let t1 = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
            assert!(
                (0.0..=0.04).contains(&(t1 - t0)),
                "the device did not pull for 400 ms: the time moved {} ms",
                (t1 - t0) * 1000.0
            );
        }
        // The device at half speed: the time follows the frames (about half
        // of the wall time), not the page clock.
        device.period_us.store(21_334, Ordering::Relaxed);
        drive(&mut page, Duration::from_millis(100));
        let (pulled0, t0, wall0) = (
            device.pulled(),
            evaluate(&mut page, "v.currentTime").as_f64().unwrap(),
            Instant::now(),
        );
        drive(&mut page, Duration::from_millis(1500));
        let (pulled1, t1, wall1) = (
            device.pulled(),
            evaluate(&mut page, "v.currentTime").as_f64().unwrap(),
            Instant::now(),
        );
        let frames_s = (pulled1 - pulled0) as f64 / 48_000.0;
        let wall_s = (wall1 - wall0).as_secs_f64();
        assert!(
            (t1 - t0 - frames_s).abs() < 0.06 && t1 - t0 < wall_s * 0.7,
            "the time moved {} s, the device pulled {frames_s} s in {wall_s} s",
            t1 - t0
        );
    });
}

/// The sound is aligned with the clock after a pause and a resume (the
/// element leaves the mixer, and its ring is ahead of the clock by the
/// frames that the device had not played and the fade consumed): the ring's
/// first frame plays `lead` frames after the join, `lead` = the fade's 512
/// frames of the cycle + the delay (at least a cycle), so that the sound at
/// the clock's time is the media at that time. Without the lead the sound is
/// that much early; without the one-cycle floor of the delay it is 512
/// frames too early on a device that reports 0 (a null sink).
fn aligned_after_resume(reported_delay: u64) {
    on_page_thread(move || {
        let device = Device::manual(reported_delay);
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto loop></video>",
        );
        page.note_user_activation();
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        drive(&mut page, Duration::from_millis(300));
        // Pause between two cycles: the source starts to fade out; its
        // clock (the media time of the pause) is read with the device held.
        {
            let _held = device.hold();
            evaluate(&mut page, "v.pause(); 1");
            let stats = device.output.stats();
            assert_eq!((stats.attached, stats.retired), (0, 1), "{stats:?}");
        }
        // The fade ends (the mixer's callback frees nothing: the owner
        // does, at a tick).
        assert!(drive_until(&mut page, Duration::from_secs(2), || {
            device.output.tick();
            device.output.stats().in_use == 0
        }));
        // Resume between two cycles: the join (inside `play`) and the first
        // frames after it are recorded.
        let held = device.hold();
        *device.recording.lock().unwrap() = Some(Vec::new());
        evaluate(&mut page, "v.play().then(() => 1)");
        assert_eq!(device.output.stats().attached, 1, "joins inside play()");
        drop(held);
        drive(&mut page, Duration::from_millis(400));
        let recorded = device.recording.lock().unwrap().take().unwrap();
        let first = recorded
            .chunks(2)
            .position(|f| f[0].abs() + f[1].abs() > 1e-4)
            .expect("the sound comes");
        let expected = 512 + reported_delay.max(512) as usize;
        assert!(
            first.abs_diff(expected) <= 64,
            "delay {reported_delay}: the first sound is at frame {first} after the join, \
             expected {expected}"
        );
    });
}

#[test]
fn the_sound_is_aligned_after_a_resume_on_a_device_with_a_delay() {
    aligned_after_resume(4800);
}

#[test]
fn the_sound_is_aligned_after_a_resume_on_a_device_that_reports_no_delay() {
    aligned_after_resume(0);
}

/// Pause, seek and the end take the element out of the mixer; the device
/// stops while nothing plays; play and a finished seek bring it back. The
/// time never jumps.
#[test]
fn an_element_leaves_the_mixer_at_pause_seek_and_end() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        // A sample of the media time every 4 ms, to look for jumps.
        evaluate(
            &mut page,
            "window.samples = []; setInterval(() => samples.push([performance.now(), v.currentTime, v.paused]), 4); 1",
        );
        evaluate(&mut page, "v.play().then(() => 1)");
        let attached = |d: &Device| d.output.stats().attached;
        assert!(drive_until(&mut page, Duration::from_secs(2), || attached(
            &device
        ) == 1));
        drive(&mut page, Duration::from_millis(300));
        // Pause: the source fades out, its slot is freed, the device stops.
        evaluate(&mut page, "v.pause(); 1");
        assert!(drive_until(&mut page, Duration::from_secs(2), || {
            device.output.stats().in_use == 0
        }));
        assert!(drive_until(&mut page, Duration::from_secs(2), || {
            device.output.phase() == Phase::Parked && !device.fake.is_active()
        }));
        let paused_at = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        drive(&mut page, Duration::from_millis(200));
        assert_eq!(
            evaluate(&mut page, "v.currentTime").as_f64().unwrap(),
            paused_at
        );
        // Play again: the device runs, the source joins, time goes on.
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(2), || attached(
            &device
        ) == 1));
        drive(&mut page, Duration::from_millis(300));
        assert!(evaluate(&mut page, "v.currentTime").as_f64().unwrap() > paused_at + 0.2);
        // A seek: the element leaves, then joins again where it seeked to.
        evaluate(&mut page, "v.currentTime = 6; 1");
        assert!(drive_until_js(
            &mut page,
            Duration::from_secs(5),
            "!v.seeking"
        ));
        assert!(drive_until(&mut page, Duration::from_secs(2), || attached(
            &device
        ) == 1));
        drive(&mut page, Duration::from_millis(300));
        let after_seek = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!((6.2..6.9).contains(&after_seek), "{after_seek}");
        // The end: the element leaves.
        evaluate(&mut page, "v.currentTime = v.duration - 0.4; 1");
        assert!(drive_until_js(&mut page, Duration::from_secs(8), "v.ended"));
        assert!(drive_until(&mut page, Duration::from_secs(2), || {
            device.output.stats().in_use == 0
        }));
        // No jump in the samples that the page took: forward at most 40 ms
        // between two samples (the timer's own delays included), except at
        // the seeks (a set position).
        let steps = evaluate(
            &mut page,
            "(() => { let big = []; for (let i = 1; i < samples.length; i++) { \
               const [t0, c0] = samples[i - 1], [t1, c1] = samples[i]; \
               if (Math.abs(c1 - c0) > 0.1 && Math.abs(c1 - c0) < 3) big.push([c0, c1, t1 - t0]); \
               if (c1 < c0 - 0.001 && Math.abs(c1 - c0) < 3) big.push(['back', c0, c1]); } \
               return big; })()",
        );
        // Samples taken across a pause (the page did not run for 100 ms) can
        // show a larger step; a step that is not in the time that passed is
        // a jump.
        for step in steps.as_array().unwrap() {
            let step = step.as_array().unwrap();
            assert!(step[0] != json!("back"), "the time went back: {step:?}");
            let (c0, c1, dt) = (
                step[0].as_f64().unwrap(),
                step[1].as_f64().unwrap(),
                step[2].as_f64().unwrap(),
            );
            assert!(
                (c1 - c0) * 1000.0 <= dt + 60.0,
                "a jump of {} ms in {dt} ms at {c0}",
                (c1 - c0) * 1000.0
            );
        }
    });
}

/// `volume` and `muted` change the gain with a ramp; a muted element stays in
/// the mixer (its clock stays the audio clock).
#[test]
fn volume_and_muted_change_the_gain() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto loop></video>",
        );
        page.note_user_activation();
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        drive(&mut page, Duration::from_millis(300));
        let full = device.peak();
        assert!(full > 0.05, "the sines play: peak {full}");
        evaluate(&mut page, "v.volume = 0.5; 1");
        drive(&mut page, Duration::from_millis(200));
        let half = device.peak();
        assert!((half / full - 0.5).abs() < 0.12, "{half} vs {full}");
        evaluate(&mut page, "v.muted = true; 1");
        drive(&mut page, Duration::from_millis(200));
        assert!(device.peak() < 1e-4, "muted: {}", device.peak());
        // Still mixed, and the time goes on.
        assert_eq!(device.output.stats().attached, 1);
        let t0 = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        drive(&mut page, Duration::from_millis(300));
        let t1 = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(t1 - t0 > 0.2, "{t0} -> {t1}");
        evaluate(&mut page, "v.muted = false; v.volume = 1; 1");
        drive(&mut page, Duration::from_millis(200));
        assert!((device.peak() / full - 1.0).abs() < 0.12);
    });
}

/// A device that goes away while an element plays: the element goes on from
/// its time on the page clock, without a jump, and its events still come.
#[test]
fn an_error_while_playing_goes_on_without_a_jump() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp4 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        evaluate(
            &mut page,
            "window.samples = []; setInterval(() => samples.push([performance.now(), v.currentTime]), 4); \
             window.ended = false; v.addEventListener('ended', () => ended = true); 1",
        );
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        drive(&mut page, Duration::from_millis(500));
        let before = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        // The server goes away.
        device.fake.report(DeviceState::Error);
        drive(&mut page, Duration::from_millis(300));
        assert_eq!(device.output.stats().phase, Phase::Failed);
        let after = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(
            after > before + 0.2 && after < before + 0.6,
            "{before} -> {after} in 300 ms"
        );
        // The events still come: the video ends.
        assert!(drive_until_js(&mut page, Duration::from_secs(5), "ended"));
        let jumps = evaluate(
            &mut page,
            "(() => { let worst = 0; for (let i = 1; i < samples.length; i++) { \
               const dt = samples[i][0] - samples[i-1][0]; const dc = (samples[i][1] - samples[i-1][1]) * 1000; \
               worst = Math.max(worst, Math.abs(dc - dt)); } return worst; })()",
        );
        assert!(jumps.as_f64().unwrap() < 80.0, "{jumps}");
        assert_eq!(device.output.stats().failures, 1);
    });
}

/// No server (`EHOSTDOWN`): the element plays on the page clock at once,
/// the output tried once; a new try at the next `play()` after 30 s is the
/// output's rule (tested with a manual clock in `swb-media`).
#[test]
fn no_server_leaves_the_element_on_the_page_clock() {
    on_page_thread(|| {
        let device = Device::start();
        device.fake.refuse(Some("Host is down (-112)"));
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp4 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        let started = Instant::now();
        drive(&mut page, Duration::from_millis(200));
        let stats = device.output.stats();
        assert_eq!(
            (stats.phase, stats.failures, stats.attached),
            (Phase::Failed, 1, 0)
        );
        assert!(drive_until_js(&mut page, Duration::from_secs(6), "v.ended"));
        // 2 s of media on the page clock.
        let elapsed = started.elapsed();
        assert!(
            elapsed > Duration::from_millis(1800) && elapsed < Duration::from_millis(3500),
            "{elapsed:?}"
        );
        assert_eq!(device.fake.connects(), 0);
        assert_eq!(device.pulled(), 0);
        // The polls of the playing element did not ask a failed output
        // again (a lock, a tick and a wake-up of the timer thread each):
        // the `play()` asked, once.
        let requests = device.output.stats().requests;
        assert!(requests <= 2, "{requests} requests in 2 s of play");
    });
}

/// After a failure the next `play()` 30 s later tries the device again (the
/// output's clock is a manual one here: no test waits 30 s), and the element
/// joins the mixer when it streams.
#[test]
fn the_next_play_after_30_seconds_tries_the_device_again() {
    on_page_thread(|| {
        let fake = FakeDevice::new();
        fake.refuse(Some("Host is down (-112)"));
        let clock = Arc::new(ManualClock::default());
        clock.advance_ms(1);
        let output = Output::manual(
            Box::new(fake.clone()),
            Arc::clone(&clock) as Arc<dyn MonoClock>,
        );
        let device = Device::with(fake.clone(), Arc::clone(&output), None).run();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto muted loop></video>",
        );
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, Duration::from_millis(300));
        let stats = output.stats();
        assert_eq!(
            (stats.phase, stats.failures, stats.attached),
            (Phase::Failed, 1, 0)
        );
        // 29 s later the element still plays on the page clock, and the
        // device is not tried.
        clock.advance_ms(29_000);
        evaluate(&mut page, "v.pause(); 1");
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, Duration::from_millis(300));
        assert_eq!((output.stats().phase, fake.connects()), (Phase::Failed, 0));
        // The server is back and 30 s have passed: an element that plays
        // goes on as it was (no try without a play()); the next play()
        // joins.
        fake.refuse(None);
        clock.advance_ms(1_000);
        drive(&mut page, Duration::from_millis(300));
        assert_eq!((output.stats().phase, fake.connects()), (Phase::Failed, 0));
        evaluate(&mut page, "v.pause(); 1");
        evaluate(&mut page, "v.play().then(() => 1)");
        assert!(drive_until(&mut page, Duration::from_secs(3), || {
            output.stats().attached == 1
        }));
        assert_eq!(fake.connects(), 1);
    });
}

/// A page thread that a script holds for 2 s does not starve the audio: the
/// demux thread refills the ring by itself, and the clock goes on.
#[test]
fn a_busy_page_thread_does_not_starve_the_audio() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(
            Some(&device),
            "<video id=v src=a.mp3 preload=auto muted></video>",
        );
        evaluate(&mut page, READY);
        evaluate(
            &mut page,
            "window.waits = 0; v.addEventListener('waiting', () => waits++); v.play().then(() => 1)",
        );
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        drive(&mut page, Duration::from_millis(300));
        let pulled_before = device.pulled();
        let starved_before = device.starved.load(Ordering::Relaxed);
        // The script holds the page thread for 2 s.
        evaluate(
            &mut page,
            "(() => { const t = performance.now(); while (performance.now() - t < 2000); return 1; })()",
        );
        let pulled = device.pulled() - pulled_before;
        assert!(
            pulled >= 90_000,
            "the device pulled {pulled} frames in the 2 s"
        );
        assert_eq!(
            device.starved.load(Ordering::Relaxed),
            starved_before,
            "the ring ran dry"
        );
        drive(&mut page, Duration::from_millis(200));
        assert_eq!(evaluate(&mut page, "waits"), json!(0), "no `waiting`");
        let time = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(time > 2.2, "{time}");
    });
}

/// 32 sources are mixed, and the other elements play on the silent output:
/// 42 elements in 3 pages (each page has 16 places for pipelines).
#[test]
fn thirty_two_elements_are_mixed_and_the_rest_play_silently() {
    on_page_thread(|| {
        let device = Device::start();
        let body = "<video src=a.mp3 preload=auto muted loop></video>".repeat(14);
        let mut pages: Vec<Page> = (0..3).map(|_| page_with(Some(&device), &body)).collect();
        for page in &mut pages {
            evaluate(
                page,
                "Promise.all([...document.querySelectorAll('video')].map(async v => { \
                   if (v.readyState < 2) await once(v, 'loadeddata'); await v.play(); })).then(() => 1)",
            );
        }
        let all = |pages: &mut Vec<Page>, ms: u64| {
            let end = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < end {
                for page in pages.iter_mut() {
                    page.process_network();
                    page.pump();
                }
                std::thread::sleep(Duration::from_millis(3));
            }
        };
        all(&mut pages, 1500);
        let stats = device.output.stats();
        assert_eq!((stats.attached, stats.in_use), (32, 32), "{stats:?}");
        // All 42 play: the mixed ones on the device's clock, the others on
        // the page clock.
        all(&mut pages, 500);
        let mut playing = 0;
        for page in &mut pages {
            let n = evaluate(
                page,
                "[...document.querySelectorAll('video')].filter(v => !v.paused && v.currentTime > 0.5).length",
            );
            playing += n.as_u64().unwrap();
        }
        assert_eq!(playing, 42);
        // The 10 elements without a slot do not ask the output at every
        // poll (and do not move between the outputs): the requests are the
        // joins of the 32 and a few of the others, not one per poll of 42
        // elements for 2 s.
        let requests = device.output.stats().requests;
        assert!(requests < 150, "{requests} requests");
        // The pages go away: every slot is freed.
        drop(pages);
        let started = Instant::now();
        while device.output.stats().in_use != 0 {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                device.output.stats()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let stats = device.output.stats();
        assert_eq!((stats.attached, stats.retired), (0, 0));
    });
}

/// A page without an output never touches one, and a page that gives it back
/// leaves the mixer without a jump.
#[test]
fn without_an_output_the_elements_use_the_silent_output() {
    on_page_thread(|| {
        let device = Device::start();
        let mut page = page_with(None, "<video id=v src=a.mp4 preload=auto muted></video>");
        evaluate(&mut page, READY);
        evaluate(&mut page, "v.play().then(() => 1)");
        drive(&mut page, Duration::from_millis(500));
        assert_eq!(device.fake.connects(), 0);
        assert_eq!(device.output.phase(), Phase::Closed);
        // Turned on while it plays: the element joins and goes on.
        let before = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        page.set_audio_output(Some(Arc::clone(&device.output)));
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 1));
        let after = evaluate(&mut page, "v.currentTime").as_f64().unwrap();
        assert!(
            after >= before && after < before + 0.8,
            "{before} -> {after}"
        );
        // Turned off while it plays: it leaves.
        page.set_audio_output(None);
        assert!(drive_until(&mut page, Duration::from_secs(2), || device
            .output
            .stats()
            .attached
            == 0));
    });
}
