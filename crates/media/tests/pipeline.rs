//! The media pipeline (M14 feature 5, ADR 0047 sections 6 and 7) in
//! virtual time: the frames shown at the due times are the frames of the
//! in-order decode (`MediaFile`), `timeupdate` comes every 250 ms of media
//! time, playback ends at the duration, the silent output consumes the
//! samples of the in-order decode; seeks; the same results on every run.
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::driver::{HANG, Played, Waker, open_fixture, play_virtual, settle, settle_update};
use common::{fixture_bytes, open};
use swb_media::pipeline::{Pipeline, PlaybackQuality, Report, SourceOpener};
use swb_media::{Decoded, timeline};

/// The frames (time, pixels) and the samples of the in-order decode.
struct Reference {
    frames: Vec<(i64, Vec<u8>)>,
    samples: Vec<f32>,
    /// The media time of the first sample.
    first_sample_us: Option<i64>,
    offset_us: i64,
}

fn reference(name: &str) -> Reference {
    let mut file = open(name).expect("opens");
    let offset_us = timeline(file.info()).offset_us;
    let mut r = Reference {
        frames: Vec::new(),
        samples: Vec::new(),
        first_sample_us: None,
        offset_us,
    };
    while let Some(d) = file.decode_next().expect("decodes") {
        match d {
            Decoded::Video(f) => r.frames.push((
                f.pts_us.expect("a time") - offset_us,
                f.image.data().to_vec(),
            )),
            Decoded::Audio(c) => {
                r.first_sample_us = r.first_sample_us.or(Some(c.pts_us - offset_us));
                r.samples.extend_from_slice(&c.samples);
            }
        }
    }
    r
}

impl Reference {
    /// The frame shown at media time `t`: the last one at or before it, or
    /// the first one.
    fn frame_at(&self, t: i64) -> Option<&(i64, Vec<u8>)> {
        self.frames
            .iter()
            .rev()
            .find(|f| f.0 <= t)
            .or(self.frames.first())
    }
}

/// The files that play to their end (every format of the build).
const PLAYABLE: &[&str] = &[
    "aac-adts.aac",
    "flac.flac",
    "m4a-aac.m4a",
    "mp3.mp3",
    "mp4-h264-aac-late-audio.mp4",
    "mp4-h264-aac-start1500.mp4",
    "mp4-h264-baseline-aac.mp4",
    "mp4-h264-high-aac-moov-at-end.mp4",
    "mp4-h264-high-aac.mp4",
    "mp4-h264-video-only.mp4",
    "ogg-flac.oga",
    "ogg-opus.opus",
    "ogg-vorbis.ogg",
    "wav-pcm-s16.wav",
    "webm-vp8-321x241.webm",
    "webm-vp8-video-only.webm",
    "webm-vp8-vorbis-start1500.webm",
    "webm-vp8-vorbis.webm",
    "webm-vp9-321x241.webm",
    "webm-vp9-opus.webm",
];

#[test]
fn every_format_plays_in_virtual_time_like_the_in_order_decode() {
    let mut failures = Vec::new();
    for name in PLAYABLE {
        let waker = Arc::new(Waker::default());
        let mut p = open_fixture(name, &waker);
        let played = play_virtual(&mut p, &waker, 0, i64::MAX);
        let reference = reference(name);
        let meta = p.metadata().expect("metadata").clone();
        let start = meta.timeline.start_us;
        let duration = meta.timeline.duration_us.expect("a duration");
        // The end at the duration.
        if played.ended != Some(duration) {
            failures.push(format!(
                "{name}: ended {:?}, duration {duration}",
                played.ended
            ));
        }
        // `timeupdate` every 250 ms of media time from the start.
        let expected: Vec<i64> = (start + 250_000..duration).step_by(250_000).collect();
        if played.timeupdates != expected {
            failures.push(format!("{name}: timeupdate at {:?}", played.timeupdates));
        }
        // The frame at each due time.
        for (_, media, pts) in &played.steps {
            let want = reference.frame_at(*media).map(|f| f.0);
            if *pts != want {
                failures.push(format!(
                    "{name}: at {media} frame {pts:?}, expected {want:?}"
                ));
                break;
            }
        }
        if let Some(img) = p.current_image() {
            let last = reference.frame_at(duration).expect("frames");
            if img.data() != last.1.as_slice() {
                failures.push(format!("{name}: the last frame's pixels differ"));
            }
        }
        // Every frame was shown (none dropped in virtual time).
        let q = p.quality();
        if q.dropped_frames != 0 || q.total_frames as usize != reference.frames.len() {
            failures.push(format!(
                "{name}: {q:?}, {} frames decoded",
                reference.frames.len()
            ));
        }
        // The silent output consumed the samples of the in-order decode.
        let audio = p.take_captured_audio();
        if !reference.samples.is_empty() {
            let first = reference.first_sample_us.expect("a first sample");
            let frames = audio.len() / 2;
            let data_end = first + (reference.samples.len() / 2) as i64 * 1_000_000 / 48_000;
            let expect_frames = ((duration.min(data_end) - first) as f64 * 48e-3).round() as usize;
            if frames.abs_diff(expect_frames) > 1 {
                failures.push(format!(
                    "{name}: consumed {frames} frames, expected {expect_frames}"
                ));
            }
            let n = audio.len().min(reference.samples.len());
            if audio[..n] != reference.samples[..n] {
                failures.push(format!("{name}: the samples differ"));
            }
        }
        assert!(reference.offset_us >= 0);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Seeks `p` (paused) to `target` and waits for the end of the seek.
fn seek(p: &mut Pipeline, waker: &Waker, target: i64) -> Vec<Report> {
    p.seek(target);
    assert_eq!(p.position(), target, "the position is the target at once");
    let mut reports = Vec::new();
    settle_update(p, waker, 0, &mut reports);
    without_buffered(reports)
}

/// The reports without `Buffered` (their timing depends on the demuxer's
/// read-ahead).
fn without_buffered(reports: Vec<Report>) -> Vec<Report> {
    reports
        .into_iter()
        .filter(|r| !matches!(r, Report::Buffered(_)))
        .collect()
}

/// The largest difference between `got` and `reference` from the frame
/// `offset` on, over shifts of up to `max_shift` frames.
fn best_diff(got: &[f32], reference: &[f32], offset: usize, max_shift: usize) -> f32 {
    (offset.saturating_sub(max_shift)..=offset + max_shift)
        .filter_map(|o| {
            let span = reference.get(o * 2..o * 2 + got.len())?;
            Some(
                got.iter()
                    .zip(span)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f32, f32::max),
            )
        })
        .fold(f32::MAX, f32::min)
}

#[test]
fn seeks_show_the_frame_at_the_target() {
    let name = "mp4-h264-high-aac.mp4";
    let reference = reference(name);
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture(name, &waker);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    // Inside a frame: the frame at 1.0 s shows at 1.01 s.
    let r = seek(&mut p, &waker, 1_010_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 1_010_000
        }]
    );
    assert_eq!(p.position(), 1_010_000);
    let current = p.current_frame().expect("a frame");
    assert_eq!(current.pts_us, 1_000_000);
    let image = current.image.as_ref().expect("converted");
    assert_eq!(
        image.data(),
        reference.frame_at(1_000_000).expect("frame").1
    );
    // Back.
    let r = seek(&mut p, &waker, 300_000);
    assert_eq!(r, [Report::SeekDone { media_us: 300_000 }]);
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(280_000));
    // Several in a row: only the last one runs and reports.
    p.seek(500_000);
    p.seek(1_200_000);
    let r = seek(&mut p, &waker, 1_700_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 1_700_000
        }]
    );
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_680_000));
    // Past the end: the last frame; the position goes to the end.
    let r = seek(&mut p, &waker, 5_000_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 5_000_000
        }]
    );
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_960_000));
    let _ = p.update(0);
    assert_eq!(p.position(), 2_000_000);
    // Play from a seek: the frames and the samples after the target.
    let r = seek(&mut p, &waker, 1_234_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 1_234_000
        }]
    );
    let _ = p.take_captured_audio();
    let played = play_virtual(&mut p, &waker, 10_000_000, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    for (_, media, pts) in &played.steps {
        assert_eq!(*pts, reference.frame_at(*media).map(|f| f.0), "at {media}");
    }
    assert_eq!(
        played.timeupdates,
        [1_484_000, 1_734_000, 1_984_000],
        "every 250 ms from the target"
    );
    let audio = p.take_captured_audio();
    assert_eq!(audio.len() / 2, 36_768, "1.234 s to 2 s at 48 kHz");
    let offset = (1_234_000 * 48 / 1000) as usize;
    let diff = best_diff(&audio[..9600], &reference.samples, offset, 0);
    assert!(diff < 0.007, "the samples after the seek differ by {diff}");
}

#[test]
fn a_seek_before_the_metadata_runs_after_it() {
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture("webm-vp8-vorbis.webm", &waker);
    p.seek(1_000_000);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    let reports = without_buffered(reports);
    assert!(matches!(reports[0], Report::Metadata(_)), "{reports:?}");
    assert_eq!(
        reports[1..],
        [
            Report::SeekDone {
                media_us: 1_000_000
            },
            Report::FirstFrame
        ]
    );
    // WebM frames are at 3 ms + 40 ms steps.
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(963_000));
    assert_eq!(p.position(), 1_000_000);
}

#[test]
fn seeks_in_a_file_with_a_start_time() {
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture("webm-vp8-vorbis-start1500.webm", &waker);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    // The first frame (1.5 s) shows at the start (1.497 s).
    assert_eq!(p.position(), 1_497_000);
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_500_000));
    let r = seek(&mut p, &waker, 2_000_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 2_000_000
        }]
    );
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_980_000));
    // Before the start: the first frame.
    let r = seek(&mut p, &waker, 0);
    assert_eq!(r, [Report::SeekDone { media_us: 0 }]);
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_500_000));
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(3_500_000));
    // The frames change from 1.5 s on.
    let first_step = played.steps.first().expect("steps");
    assert_eq!(first_step.1, 250_000, "the first due time is a timeupdate");
}

#[test]
fn a_stream_that_cannot_seek_plays_again_from_the_start() {
    let name = "mp4-h264-high-aac.mp4";
    let waker = Arc::new(Waker::default());
    let mut p = Pipeline::open(
        SourceOpener::memory(fixture_bytes(name), false),
        common::driver::options(&waker),
    );
    let played = play_virtual(&mut p, &waker, 0, 1_000_000);
    assert!(!p.metadata().expect("metadata").info.seekable);
    assert!(played.steps.len() > 10);
    p.pause(played.now);
    let r = seek(&mut p, &waker, 1_500_000);
    assert_eq!(r, [Report::SeekDone { media_us: 0 }]);
    assert_eq!(p.position(), 0);
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(0));
    let played = play_virtual(&mut p, &waker, 5_000_000, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000), "{played:?}");
    assert_eq!(played.timeupdates.len(), 7);
}

/// Plays `name` to 0.6 s, seeks to 1.3 s while playing, plays to the end;
/// returns what both parts showed (without the `Buffered` reports).
fn scenario(name: &str) -> Vec<Played> {
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture(name, &waker);
    let mut first = play_virtual(&mut p, &waker, 0, 600_000);
    p.seek(1_300_000);
    let mut second = play_virtual(&mut p, &waker, first.now, i64::MAX);
    for played in [&mut first, &mut second] {
        played.reports = without_buffered(std::mem::take(&mut played.reports));
    }
    vec![first, second]
}

#[test]
fn results_are_the_same_on_every_run_under_load() {
    let names = [
        "mp4-h264-high-aac.mp4",
        "webm-vp9-opus.webm",
        "webm-vp8-vorbis-start1500.webm",
        "mp3.mp3",
        "ogg-vorbis.ogg",
        "mp4-h264-video-only.mp4",
    ];
    let expected: Vec<Vec<Played>> = names.iter().map(|n| scenario(n)).collect();
    for (name, played) in names.iter().zip(&expected) {
        assert!(played[1].reports.contains(&Report::SeekDone {
            media_us: 1_300_000
        }));
        assert!(played[1].ended.is_some(), "{name}: {played:?}");
    }
    // Load: busy threads, and four runs of each file at once.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cores = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let busy: Vec<_> = (0..cores)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
                }
            })
        })
        .collect();
    let runs: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(move || names.iter().map(|n| scenario(n)).collect::<Vec<_>>()))
        .collect();
    let results: Vec<_> = runs
        .into_iter()
        .map(|r| r.join().expect("a run ends"))
        .collect();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for b in busy {
        b.join().expect("a busy thread ends");
    }
    for result in results {
        for ((name, got), want) in names.iter().zip(&result).zip(&expected) {
            for (part, (g, w)) in got.iter().zip(want).enumerate() {
                assert_eq!(g.steps, w.steps, "{name} part {part}: steps");
                assert_eq!(g.timeupdates, w.timeupdates, "{name} part {part}");
                assert_eq!(g.ended, w.ended, "{name} part {part}");
                assert_eq!(g.reports, w.reports, "{name} part {part}");
            }
        }
    }
}

#[test]
fn hostile_files_play_to_where_they_can_or_fail() {
    // (file, the error code, or `None`: plays to `ended`).
    let cases: &[(&str, Option<u16>)] = &[
        ("garbage.mp4", Some(4)),
        ("mp4-h264-high-aac-no-moov.mp4", Some(4)),
        ("poster-200x100.png", Some(4)),
        ("webm-vp9-16400x16.webm", Some(4)),
        ("webm-vp9-8192x4416.webm", Some(4)),
        ("mp4-h264-high-aac-corrupt.mp4", Some(3)),
        ("mp4-h264-high-aac-truncated.mp4", Some(3)),
        ("webm-vp8-vorbis-corrupt.webm", None),
        ("webm-vp8-vorbis-truncated.webm", None),
        ("mp3-truncated.mp3", None),
        ("mka-pcm-17-streams.mka", None),
    ];
    for (name, code) in cases {
        let waker = Arc::new(Waker::default());
        let mut p = open_fixture(name, &waker);
        let played = play_virtual(&mut p, &waker, 0, i64::MAX);
        let error = played.reports.iter().find_map(|r| match r {
            Report::Error(e) => Some(e.element_code()),
            _ => None,
        });
        if let Some(c) = code {
            assert_eq!(error, Some(Some(*c)), "{name}: {:?}", played.reports);
            assert!(p.is_failed());
            assert_eq!(played.ended, None, "{name}");
        } else {
            assert_eq!(error, None, "{name}: {:?}", played.reports);
            // A file whose data ends early (cut, damaged) plays on to its
            // duration over silence and the last frame, as Chromium's
            // `currentTime` is the duration at `ended` (M14 feature 6b: the
            // cut-file rule is gone).
            let duration = p.metadata().and_then(|m| m.timeline.duration_us);
            match duration {
                Some(d) => assert_eq!(played.ended, Some(d), "{name}: {:?}", played.reports),
                None => assert!(played.ended.is_some(), "{name}: {:?}", played.reports),
            }
        }
        drop(p);
    }
}

/// M14 feature 6b (review fix): a decode failure after the metadata has a
/// place in the media time that depends on the data, so the element can play
/// on to it. A later damage lies at a later time. When the video and the
/// audio decoder both fail, the first to report decides (the 80 % file gives
/// 1.2625 s or 1.28 s under load; backlog in `docs/roadmap.md`), so the test
/// allows a spread of 25 ms; a file where one decoder fails has one place.
#[test]
fn a_decode_failure_has_the_same_place_on_every_run() {
    let names = [
        "mp4-h264-high-aac-damaged-30.mp4",
        "mp4-h264-high-aac-damaged-60.mp4",
        "mp4-h264-high-aac-damaged-80.mp4",
        "mp4-h264-high-aac-damaged-90.mp4",
    ];
    let mut places = Vec::new();
    for name in names {
        let mut seen = Vec::new();
        for _ in 0..12 {
            let waker = Arc::new(Waker::default());
            let mut p = open_fixture(name, &waker);
            let played = play_virtual(&mut p, &waker, 0, i64::MAX);
            assert!(p.is_failed(), "{name}: {:?}", played.reports);
            // In virtual time the playback has not passed the place when the
            // failure arrives.
            let at = p.failed_at().unwrap_or(i64::MIN);
            assert!(p.position() <= at, "{name}: at {} after {at}", p.position());
            seen.push(p.failed_at().unwrap_or_else(|| panic!("{name}: no place")));
        }
        let (low, high) = (
            seen.iter().copied().min().unwrap(),
            seen.iter().copied().max().unwrap(),
        );
        assert!(high - low <= 25_000, "{name}: {seen:?}");
        places.push(low);
    }
    // A later damage lies later, inside the 2 s of the file.
    assert!(places.windows(2).all(|w| w[0] < w[1]), "{places:?}");
    assert!(
        places.iter().all(|&p| (0..2_000_000).contains(&p)),
        "{places:?}"
    );
}

#[test]
fn a_video_that_is_not_painted_decodes_without_converting() {
    let waker = Arc::new(Waker::default());
    // Not painted from the open: the threads convert no frame before the
    // test could say so.
    let mut o = common::driver::options(&waker);
    o.painted = false;
    let mut p = Pipeline::open(
        SourceOpener::file(common::fixture("mp4-h264-high-aac.mp4")),
        o,
    );
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(2_000_000));
    let stats = p.stats();
    assert_eq!(stats.frames_decoded, 50);
    assert_eq!(stats.frames_converted, 0);
    assert_eq!(stats.pixmaps_allocated, 0);
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_960_000));
    assert!(p.current_image().is_none());
    // Painted again: the next frames are converted.
    p.set_painted(true);
    let mut reports = Vec::new();
    p.seek(500_000);
    settle_update(&mut p, &waker, played.now, &mut reports);
    assert!(p.current_image().is_some());
}

#[test]
fn playback_reuses_the_pixmaps() {
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture("mp4-h264-aac-long.mp4", &waker);
    let played = play_virtual(&mut p, &waker, 0, i64::MAX);
    assert_eq!(played.ended, Some(10_000_000));
    let stats = p.stats();
    assert_eq!(stats.frames_converted, 250);
    // The queue (6), the current frame and the one being converted.
    assert!(
        stats.pixmaps_allocated <= 9,
        "{} pixmaps for 250 frames",
        stats.pixmaps_allocated
    );
}

/// The file whose audio (3 s to 5 s) comes after all of its video (0 s to
/// 2 s) in the file.
const LATE_AUDIO: &str = "mp4-h264-aac-late-audio.mp4";
/// Its duration (FFmpeg's: the end of the audio).
const LATE_AUDIO_END: i64 = 4_999_229;

/// Opens `name` with a video packet bound of `packet_bytes`.
fn open_bounded(name: &str, waker: &Arc<Waker>, packet_bytes: usize) -> Pipeline {
    let mut o = common::driver::options(waker);
    o.packet_bytes = packet_bytes;
    Pipeline::open(SourceOpener::file(common::fixture(name)), o)
}

/// What a playback in virtual time shows, without the `Buffered` reports,
/// with the quality and the samples consumed.
fn play_bounded(name: &str, packet_bytes: usize) -> (Played, PlaybackQuality, Vec<f32>) {
    let waker = Arc::new(Waker::default());
    let mut p = open_bounded(name, &waker, packet_bytes);
    let mut played = play_virtual(&mut p, &waker, 0, i64::MAX);
    played.reports = without_buffered(std::mem::take(&mut played.reports));
    (played, p.quality(), p.take_captured_audio())
}

#[test]
fn tracks_far_apart_in_the_file_play_in_virtual_time() {
    let reference = reference(LATE_AUDIO);
    // A bound of 4 KiB (1 KiB for the audio backlog): the video packets
    // fill the queue and the frames fill theirs before any audio is read
    // (both threads wait for the owner: an audio gap from the start), and
    // at the end of the video the ring and the backlog are full while the
    // decoder still holds the last frames (they come late).
    let (played, quality, audio) = play_bounded(LATE_AUDIO, 4096);
    assert_eq!(played.ended, Some(LATE_AUDIO_END), "{:?}", played.reports);
    let expected: Vec<i64> = (250_000..LATE_AUDIO_END).step_by(250_000).collect();
    assert_eq!(played.timeupdates, expected);
    assert!(
        !played
            .reports
            .iter()
            .any(|r| matches!(r, Report::Underrun { .. })),
        "{:?}",
        played.reports
    );
    for (_, media, pts) in &played.steps {
        if *media < 1_800_000 {
            assert_eq!(*pts, reference.frame_at(*media).map(|f| f.0), "at {media}");
        }
    }
    // The two frames that the decoder held when the threads waited for the
    // owner came late and were dropped.
    assert_eq!(
        quality,
        PlaybackQuality {
            total_frames: 50,
            dropped_frames: 2
        }
    );
    // The samples of the in-order decode, from 2.976 s on.
    let first = reference.first_sample_us.expect("a first sample");
    assert_eq!(first, 2_976_009);
    let want = ((LATE_AUDIO_END - first) as f64 * 48e-3).round() as usize;
    assert!(audio.len() / 2 >= want - 1, "{} frames", audio.len() / 2);
    let n = audio.len().min(reference.samples.len());
    assert!(audio[..n] == reference.samples[..n], "the samples differ");
    // The same on every run.
    for _ in 0..3 {
        let (again, q, a) = play_bounded(LATE_AUDIO, 4096);
        assert_eq!(again.steps, played.steps);
        assert_eq!(again.reports, played.reports);
        assert_eq!(q, quality);
        assert_eq!(a.len(), audio.len());
    }
    // With the default bound the demuxer reads on while the ring is full:
    // every frame shows at its time.
    let (played, quality, _) = play_bounded(LATE_AUDIO, 16 << 20);
    assert_eq!(played.ended, Some(LATE_AUDIO_END));
    for (_, media, pts) in &played.steps {
        assert_eq!(*pts, reference.frame_at(*media).map(|f| f.0), "at {media}");
    }
    assert_eq!(
        quality,
        PlaybackQuality {
            total_frames: 50,
            dropped_frames: 0
        }
    );
}

#[test]
fn tracks_far_apart_in_the_file_play_in_real_time() {
    for packet_bytes in [4096, 16 << 20] {
        let waker = Arc::new(Waker::default());
        let mut p = open_bounded(LATE_AUDIO, &waker, packet_bytes);
        let mut reports = Vec::new();
        settle_update(&mut p, &waker, 0, &mut reports);
        p.play(0);
        // Real time on a clock of the test: 40 ms per step, and at most
        // 10 ms for the threads.
        let mut now = 0;
        let mut positions = Vec::new();
        while !p.is_ended() && now < 20_000_000 {
            let seen = waker.count();
            now += 40_000;
            reports.extend(p.update(now));
            positions.push(p.position());
            waker.wait_past(seen, Duration::from_millis(10));
        }
        assert!(
            p.is_ended(),
            "{packet_bytes}: at {}: {reports:?}",
            p.position()
        );
        assert_eq!(p.position(), LATE_AUDIO_END);
        // The position passed the gap (2 s to 3 s) on the clock, with at
        // most a short stop at the start (before the threads wait for the
        // owner).
        let stopped = positions.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(stopped < 25, "{packet_bytes}: stood {stopped} steps");
        assert!(now < 6_000_000, "{packet_bytes}: ended at clock {now}");
    }
}

/// Opens the late-audio file with a 4 KiB packet bound (the threads stall
/// on the owner: no audio before 3 s in the file) and a no-progress limit
/// that never comes in a test (the watchdog would wake a thread that missed
/// a wake-up, and hide the miss).
fn open_late_audio_stalling(waker: &Arc<Waker>) -> Pipeline {
    let mut o = common::driver::options(waker);
    o.packet_bytes = 4096;
    o.stall_limit = Duration::from_secs(3600);
    Pipeline::open(SourceOpener::file(common::fixture(LATE_AUDIO)), o)
}

/// Waits until the video thread has decoded `n` frames.
fn wait_decoded(p: &Pipeline, n: u64) {
    let started = Instant::now();
    while p.stats().frames_decoded < n {
        assert!(
            started.elapsed() < HANG,
            "{} frames decoded",
            p.stats().frames_decoded
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn taking_the_first_frame_wakes_the_video_thread() {
    // The owner takes the first frame only after the frame queue (6 frames
    // of 320 x 240) is full and the video thread waits for room with the
    // 7th.
    let waker = Arc::new(Waker::default());
    let mut p = open_late_audio_stalling(&waker);
    wait_decoded(&p, 7);
    p.play(0);
    let reports = p.update(0);
    assert!(reports.contains(&Report::FirstFrame), "{reports:?}");
    // The update took a frame: the video thread fills the queue again and
    // the threads wait for the owner. Without the video thread's wake-up
    // the pipeline stays pending (the samples for the next frame).
    settle(&p, &waker);
    assert_eq!(p.stats().frames_decoded, 8);
}

#[test]
fn ending_a_seek_wakes_the_video_thread() {
    let waker = Arc::new(Waker::default());
    let mut p = open_late_audio_stalling(&waker);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    // The seek's frames fill the queue; the threads wait for the owner
    // (no audio), so the seek ends with what is there.
    p.seek(1_000_000);
    settle(&p, &waker);
    let decoded = p.stats().frames_decoded;
    let reports = p.update(0);
    assert!(
        reports.contains(&Report::SeekDone {
            media_us: 1_000_000
        }),
        "{reports:?}"
    );
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(1_000_000));
    // The update took the seek's frame: the video thread fills the queue
    // again.
    p.play(0);
    settle(&p, &waker);
    assert_eq!(p.stats().frames_decoded, decoded + 1);
}

#[test]
fn a_video_only_file_plays_past_its_packet_bound() {
    // A bound of 2 KiB: the demux thread waits for room that only the
    // video thread makes (no ring, no owner wakes it).
    let name = "mp4-h264-video-only.mp4";
    let reference = reference(name);
    let (played, quality, _) = play_bounded(name, 2048);
    assert_eq!(played.ended, Some(2_000_000), "{:?}", played.reports);
    for (_, media, pts) in &played.steps {
        assert_eq!(*pts, reference.frame_at(*media).map(|f| f.0), "at {media}");
    }
    assert_eq!(
        quality,
        PlaybackQuality {
            total_frames: 50,
            dropped_frames: 0
        }
    );
}

#[test]
fn many_seeks_while_the_video_thread_works_show_the_last_one() {
    let name = "mp4-h264-aac-long.mp4";
    let reference = reference(name);
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture(name, &waker);
    let played = play_virtual(&mut p, &waker, 0, 1_000_000);
    p.pause(played.now);
    // Seeks while the threads decode and convert: each one replaces the
    // last; the frames of the replaced ones must not show.
    for i in 0..40i64 {
        let seen = waker.count();
        p.seek((i * 2_370_000) % 9_500_000);
        waker.wait_past(seen, Duration::from_millis(1));
    }
    let r = seek(&mut p, &waker, 7_310_000);
    assert_eq!(
        r,
        [Report::SeekDone {
            media_us: 7_310_000
        }]
    );
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(7_280_000));
    let image = p.current_image().expect("an image");
    assert_eq!(
        image.data(),
        reference.frame_at(7_310_000).expect("a frame").1
    );
    let _ = p.take_captured_audio();
    let shown_before = p.quality().total_frames;
    let played = play_virtual(&mut p, &waker, played.now, i64::MAX);
    assert_eq!(played.ended, Some(10_000_000));
    for (_, media, pts) in &played.steps {
        assert_eq!(*pts, reference.frame_at(*media).map(|f| f.0), "at {media}");
    }
    let q = p.quality();
    assert_eq!(q.dropped_frames, 0, "{q:?}");
    // 7.28 s to 9.96 s: 68 frames, the first one shown at the seek.
    assert_eq!(q.total_frames - shown_before, 67, "{q:?}");
    let last = reference.frame_at(10_000_000).expect("a frame");
    assert_eq!(p.current_image().expect("an image").data(), last.1);
    let audio = p.take_captured_audio();
    let frames = audio.len() / 2;
    assert!(
        frames.abs_diff(129_120) <= 1,
        "{frames} frames, 7.31 s to 10 s"
    );
    let offset = (7_310_000 * 48 / 1000) as usize;
    let diff = best_diff(&audio[..9600], &reference.samples, offset, 0);
    assert!(diff < 0.007, "the samples after the seek differ by {diff}");
}

#[test]
fn reports_come_in_order() {
    let waker = Arc::new(Waker::default());
    let mut p = open_fixture("mp4-h264-high-aac.mp4", &waker);
    let mut reports = Vec::new();
    settle_update(&mut p, &waker, 0, &mut reports);
    assert!(
        matches!(reports.first(), Some(Report::Metadata(_))),
        "{reports:?}"
    );
    assert!(reports.contains(&Report::FirstFrame), "{reports:?}");
    let meta = p.metadata().expect("metadata");
    assert_eq!(meta.natural_size(), Some((320, 240)));
    assert_eq!(p.current_frame().map(|f| f.pts_us), Some(0));
    assert_eq!(p.position(), 0);
    assert!(p.next_due().is_none(), "paused: nothing is due");
}

/// M14 feature 6a: `has_enough_data` is false until the first data was
/// reported and the threads have settled (the whole small file is read, or
/// the decoded queues are full), and true after, for every format; it needs
/// no `play`. A seek makes it false again.
#[test]
fn enough_data_comes_when_the_threads_have_settled() {
    for name in PLAYABLE {
        let waker = Arc::new(Waker::default());
        let mut p = open_fixture(name, &waker);
        assert!(!p.has_enough_data(), "{name}: before anything is decoded");
        let started = Instant::now();
        let mut reports = Vec::new();
        loop {
            let seen = waker.count();
            reports.extend(p.update(0));
            if p.has_enough_data() {
                break;
            }
            assert!(started.elapsed() < HANG, "{name}: no enough data");
            waker.wait_past(seen, Duration::from_secs(1));
        }
        assert!(reports.contains(&Report::FirstFrame), "{name}: {reports:?}");
        assert!(!p.has_pending_work(), "{name}");
        // It stays true while nothing changes, and the ranges are final.
        let buffered = p.buffered();
        std::thread::sleep(Duration::from_millis(30));
        p.update(0);
        assert!(p.has_enough_data(), "{name}");
        assert_eq!(p.buffered(), buffered, "{name}");
        p.seek(500_000);
        assert!(!p.has_enough_data(), "{name}: during a seek");
    }
}

/// M14 feature 6a: between a thread's report and the owner's `update`,
/// `has_pending_work` is false but the owner must not take that for "no
/// work": `has_unread_reports` is true.
#[test]
fn a_report_that_waits_for_the_update_is_visible() {
    let waker = Arc::new(Waker::default());
    let p = open_fixture("mp4-h264-high-aac.mp4", &waker);
    let started = Instant::now();
    // The open ends: a thread event waits for `update`.
    while !p.has_unread_reports() {
        assert!(started.elapsed() < HANG, "the open does not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!p.has_pending_work(), "events wait for the update");
}
