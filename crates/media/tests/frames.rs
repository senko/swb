//! Video frames: the RGBA of frames at paused times against Chromium's
//! pixels (`tools/probes/media-frames.json`, ADR 0047 section 9: within 3
//! levels), and seeks to an exact frame.
#![cfg(feature = "ffmpeg")]

mod common;

use common::{open, repo_path, rgb};
use serde_json::Value;
use swb_media::{Decoded, Matrix, VideoFrame};

/// The tolerance of ADR 0047 section 9.
const TOLERANCE: i32 = 3;

fn json(path: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(path).expect("the probe files are readable");
    serde_json::from_str(&text).expect("probe files are JSON")
}

/// One sampled frame of Chromium: the file, the time and the pixels at the
/// points.
struct Sample {
    case: String,
    file: String,
    time: f64,
    points: Vec<((u32, u32), [i32; 3])>,
}

/// The time of a query label `... seekTo(v, 0.44) ...`.
fn seek_time(label: &str) -> Option<f64> {
    let rest = label.split("seekTo(v, ").nth(1)?;
    rest.split(')').next()?.trim().parse().ok()
}

/// Chromium's frames from `tests/data/media-frames.json` (the output of
/// `swbtools probe --json` for the cases of this test; three equal runs).
fn samples() -> Vec<Sample> {
    let probe = json(&repo_path("tools/probes/media-frames.json"));
    let output = json(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/media-frames.json"),
    );
    let mut samples = Vec::new();
    for case in output["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("a case name");
        let def = probe["cases"]
            .as_array()
            .and_then(|cs| cs.iter().find(|c| c["name"] == name))
            .expect("the case is in the probe file");
        let file = def["files"]
            .as_object()
            .and_then(|f| f.values().next())
            .and_then(Value::as_str)
            .and_then(|p| p.strip_prefix("fixtures/media/"))
            .expect("a test media file")
            .to_owned();
        let point_lists: Vec<Vec<(u32, u32)>> = def["measure"]
            .as_array()
            .expect("measure")
            .iter()
            .filter_map(|m| m["points"].as_array())
            .map(|ps| {
                ps.iter()
                    .filter_map(|p| {
                        let x = u32::try_from(p[0].as_u64()?).ok()?;
                        let y = u32::try_from(p[1].as_u64()?).ok()?;
                        Some((x, y))
                    })
                    .collect()
            })
            .collect();
        let mut time = None;
        let mut query = 0;
        for result in case["results"].as_array().expect("results") {
            match result["kind"].as_str() {
                Some("js") => time = result["label"].as_str().and_then(seek_time),
                Some("pixels") => {
                    let points = &point_lists[query % point_lists.len()];
                    query += 1;
                    let pixels = result["pixels"].as_array().expect("pixels");
                    assert_eq!(points.len(), pixels.len(), "{name}");
                    let values = points
                        .iter()
                        .zip(pixels)
                        .map(|(p, v)| {
                            let c = |i: usize| v[i].as_i64().unwrap_or(-1000) as i32;
                            (*p, [c(0), c(1), c(2)])
                        })
                        .collect();
                    samples.push(Sample {
                        case: name.to_owned(),
                        file: file.clone(),
                        time: time.expect("a seek before the pixels"),
                        points: values,
                    });
                }
                _ => {}
            }
        }
    }
    samples
}

/// The frame shown at `time_us`: seek, then the first frame.
fn frame_at(name: &str, time_us: i64) -> VideoFrame {
    let mut file = open(name).expect("the file opens");
    file.seek(time_us).expect("the seek works");
    file.next_video_frame()
        .expect("the file decodes")
        .expect("a frame at the time")
}

#[test]
fn frames_match_chromium_within_3_levels() {
    let samples = samples();
    // 24 cases: the 4 `frames-times-*` cases at 4 times with two point sets
    // (bars and the frame counter) each, the 4 `frames-color-*` cases of the
    // 320 x 240 files, and the 14 untagged files of other sizes (one point
    // set each: the matrix follows the height rule of `conversion`), and the
    // 2 `frames-color-odd-columns-*` cases (854 x 480 and 322 x 242 with
    // smooth gradients; the points include the last 8 and 9 columns, where
    // swscale's own tail differed by up to 84 levels from the padded
    // conversion; M14 feature 7a, fix round 2).
    assert_eq!(samples.len(), 56);
    let mut failures = Vec::new();
    let mut points = 0;
    let mut worst = 0;
    for s in &samples {
        let time_us = (s.time * 1e6).round() as i64;
        let frame = frame_at(&s.file, time_us);
        // Chromium shows the frame at or before the time (frame 11 at 0.44).
        let pts = frame.pts_us.expect("a frame time");
        assert!(
            pts <= time_us && time_us - pts < 40_000,
            "{} t={}: pts {pts}",
            s.case,
            s.time
        );
        for ((x, y), want) in &s.points {
            let Some(got) = rgb(&frame.image, *x, *y) else {
                failures.push(format!(
                    "{} t={}: ({x},{y}) is outside the frame",
                    s.case, s.time
                ));
                continue;
            };
            let diff = (0..3)
                .map(|i| (i32::from(got[i]) - want[i]).abs())
                .max()
                .unwrap_or(0);
            worst = worst.max(diff);
            points += 1;
            if diff > TOLERANCE {
                failures.push(format!(
                    "{} t={} ({x},{y}): swb {got:?}, Chromium {want:?}",
                    s.case, s.time
                ));
            }
        }
    }
    eprintln!("{points} points, worst difference {worst}");
    assert!(points > 500, "{points} points");
    assert!(
        failures.is_empty(),
        "worst difference {worst}:\n{}",
        failures.join("\n")
    );
}

/// M14 feature 10c, found by the exit test: the frames of a full range
/// H.264 stream (`yuvj420p`) after the first were converted with default
/// coefficients, because `sws_getCachedContext` rebuilt the scaler's context
/// at every call (swscale keeps another source format for the deprecated full
/// range formats) and the details were not set again. The frames that the
/// decoder-level test above compares were all the first conversion of a
/// fresh file. Now a frame converted after others is the frame converted
/// first, byte for byte, for a full range and a limited range file.
#[test]
fn a_frame_converts_the_same_after_other_frames_as_first() {
    for name in ["mp4-h264-fullrange-bt709.mp4", "mp4-h264-bt709.mp4"] {
        let mut file = open(name).expect("the file opens");
        let mut later = None;
        for _ in 0..12 {
            later = file.next_video_frame().expect("decodes");
        }
        let later = later.expect("the file has 12 frames");
        let time_us = later.pts_us.expect("a frame time");
        let first = frame_at(name, time_us);
        assert_eq!(first.pts_us, later.pts_us, "{name}");
        assert!(
            first.image.data() == later.image.data(),
            "{name}: the frame at {time_us} differs after other conversions"
        );
    }
}

#[test]
fn frames_are_opaque() {
    let frame = frame_at("webm-vp9-321x241.webm", 1_000_000);
    assert_eq!((frame.image.width(), frame.image.height()), (321, 241));
    assert!(frame.image.pixels().iter().all(|p| p.alpha() == 255));
}

#[test]
fn a_seek_gives_the_exact_frame() {
    let name = "mp4-h264-aac-long.mp4";
    // Decode in order up to 4.44 s (frame 111 at 25 fps).
    let mut file = open(name).expect("opens");
    let mut in_order = None;
    while let Some(f) = file.next_video_frame().expect("decodes") {
        if f.pts_us == Some(4_440_000) {
            in_order = Some(f);
            break;
        }
    }
    let in_order = in_order.expect("the frame at 4.44 s");

    // Seek from after it, to it and to a time inside it.
    for target in [4_440_000, 4_450_000, 4_479_999] {
        file.seek(target).expect("seeks");
        let f = file.next_video_frame().expect("decodes").expect("a frame");
        assert_eq!(f.pts_us, Some(4_440_000), "target {target}");
        assert_eq!(f.image.data(), in_order.image.data(), "target {target}");
        let next = file.next_video_frame().expect("decodes").expect("a frame");
        assert_eq!(next.pts_us, Some(4_480_000));
    }

    // Back to the start, and before it.
    for target in [0, -1_000_000] {
        file.seek(target).expect("seeks");
        let f = file.next_video_frame().expect("decodes").expect("a frame");
        assert_eq!(f.pts_us, Some(0), "target {target}");
    }

    // Past the end: the last frame.
    file.seek(60_000_000).expect("seeks");
    let f = file.next_video_frame().expect("decodes").expect("a frame");
    assert_eq!(f.pts_us, Some(9_960_000));
    assert!(file.next_video_frame().expect("decodes").is_none());
}

/// The decoder of a 1280 x 720 video uses frame threads (several frames in
/// flight); a seek must flush them and still give the exact frame.
#[test]
fn a_seek_with_frame_threads_gives_the_exact_frame() {
    let name = "mp4-h264-untagged-bt709-1280x720.mp4";
    let mut file = open(name).expect("opens");
    let mut frames = Vec::new();
    while let Some(f) = file.next_video_frame().expect("decodes") {
        frames.push(f);
    }
    assert!(frames.len() > 12, "{} frames", frames.len());
    for index in [3, 12, frames.len() - 1] {
        let want = &frames[index];
        let pts = want.pts_us.expect("a frame time");
        // Seek from the end (the decoder holds frames) to the frame, and
        // to a time inside it.
        for target in [pts, pts + 1_000] {
            file.seek(target).expect("seeks");
            let f = file.next_video_frame().expect("decodes").expect("a frame");
            assert_eq!(f.pts_us, Some(pts), "frame {index}, target {target}");
            assert_eq!(f.image.data(), want.image.data(), "frame {index}");
        }
    }
}

#[test]
fn a_seek_in_a_file_with_a_start_time() {
    // The frames are at 1.5 s + 40 ms * n; the timeline is the stream's.
    let mut file = open("webm-vp8-vorbis-start1500.webm").expect("opens");
    for (target, frame) in [(2_020_000, 2_020_000), (2_000_000, 1_980_000)] {
        file.seek(target).expect("seeks");
        let f = file.next_video_frame().expect("decodes").expect("a frame");
        assert_eq!(f.pts_us, Some(frame), "target {target}");
    }
    file.seek(0).expect("seeks");
    let f = file.next_video_frame().expect("decodes").expect("a frame");
    assert_eq!(f.pts_us, Some(1_500_000));
}

#[test]
fn the_color_rule_of_the_test_files() {
    let rule = |name: &str| {
        let mut file = open(name).expect("opens");
        file.set_tracks(true, false).expect("tracks");
        let first = loop {
            match file.decode_next().expect("decodes").expect("a frame") {
                Decoded::Video(f) => break f,
                Decoded::Audio(_) => {}
            }
        };
        drop(first);
        let v = file.info().video.clone().expect("video");
        (v.colors, v.coded_size)
    };
    // The tags of the streams; the matrix follows in `conversion`.
    let (colors, _) = rule("mp4-h264-bt709.mp4");
    assert_eq!(colors.matrix, swb_media::MatrixTag::Bt709);
    let (colors, _) = rule("mp4-h264-bt601.mp4");
    assert_eq!(colors.matrix, swb_media::MatrixTag::Bt601);
    let (colors, _) = rule("mp4-h264-fullrange-bt709.mp4");
    assert_eq!(colors.range, swb_media::RangeTag::Full);
    let (colors, size) = rule("mp4-h264-untagged-bt709-1280x720.mp4");
    assert_eq!(colors.matrix, swb_media::MatrixTag::Unspecified);
    assert_eq!(
        swb_media::conversion(colors, swb_media::ColorTags::UNSPECIFIED, size.1).matrix,
        Matrix::Bt709
    );
}

/// The untagged files of the boundary (feature 7a): the height decides, as
/// in Chromium (the pixels of every one of them are compared in
/// `frames_match_chromium_within_3_levels`).
#[test]
fn the_untagged_files_follow_the_height_rule() {
    for (name, matrix) in [
        ("mp4-h264-untagged-bt709-1280x544.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-720x720.mp4", Matrix::Bt709),
        ("mp4-h264-untagged-bt709-960x720.mp4", Matrix::Bt709),
        ("mp4-h264-untagged-bt709-1024x576.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-960x540.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1280x480.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1280x600.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1024x640.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1280x704.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1280x718.mp4", Matrix::Bt601),
        ("mp4-h264-untagged-bt709-1920x1080.mp4", Matrix::Bt709),
    ] {
        let file = open(name).expect("opens");
        let v = file.info().video.clone().expect("video");
        assert_eq!(v.colors.matrix, swb_media::MatrixTag::Unspecified, "{name}");
        let got =
            swb_media::conversion(v.colors, swb_media::ColorTags::UNSPECIFIED, v.coded_size.1);
        assert_eq!(got.matrix, matrix, "{name}");
    }
}

/// The Wikipedia VP8 file (`fixtures/js/media/`, local only: not
/// committed; the test passes without it): its container says BT.709, and
/// FFmpeg's VP8 decoder tags every frame BT.601. Chromium shows BT.709 (the
/// probe case `real-frames-color-wikimedia-vp8`, three equal runs; data in
/// `tests/data/media-frames-real.json`): the stream's tag wins over the
/// frame's. Frame 398 is the frame at 16.02 s (the file's frame times are
/// not on a grid), and the matrix is told apart by the points where BT.601
/// and BT.709 differ by 8 levels or more.
#[test]
fn the_wikimedia_file_converts_by_its_container_tag() {
    let path = repo_path("fixtures/js/media/wikimedia-web-browser-demo.webm");
    if !path.exists() {
        eprintln!("skipped: {} is not there (local fixture)", path.display());
        return;
    }
    let data = json(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/media-frames-real.json"),
    );
    let case = &data["cases"][0];
    let results = case["results"].as_array().expect("results");
    let pixels = results
        .iter()
        .find(|r| r["kind"] == "pixels")
        .expect("pixels");
    let points = pixels["points"].as_array().expect("points");
    let want = pixels["pixels"].as_array().expect("pixels");
    let source = swb_media::FileSource::open(&path).expect("the file opens");
    let mut file = swb_media::MediaFile::open(Box::new(source)).expect("opens");
    let video = file.info().video.clone().expect("video");
    assert_eq!(video.colors.matrix, swb_media::MatrixTag::Bt709);
    file.seek(16_020_000).expect("seeks");
    let frame = file
        .next_video_frame()
        .expect("decodes")
        .expect("a frame at the time");
    let mut worst = 0;
    for (point, want) in points.iter().zip(want) {
        let (x, y) = (
            u32::try_from(point[0].as_u64().expect("x")).expect("x fits"),
            u32::try_from(point[1].as_u64().expect("y")).expect("y fits"),
        );
        let got = rgb(&frame.image, x, y).expect("inside the frame");
        for i in 0..3 {
            let w = want[i].as_i64().expect("a level") as i32;
            worst = worst.max((i32::from(got[i]) - w).abs());
        }
    }
    // 601 would differ by 8 to 24 levels at these points (23 and more at
    // five of them); the points on the edge of a shape give 5.
    assert!(worst <= 5, "worst difference {worst}");
}
