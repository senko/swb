//! Every file of `fixtures/media/` opens and decodes, or fails with the
//! error kind of ADR 0047 section 11; the metadata against Chromium's
//! (`tools/probes/media-states.json`, case `format-metadata-table`).
#![cfg(feature = "ffmpeg")]

mod common;

use common::{decode_all, open, repo_path};
use swb_media::{Codec, MediaError, timeline};

/// How decoding a file ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// To its end.
    Complete,
    /// The demuxer stops early with an error; the frames before it play.
    Early,
    /// A decoder fails: `MEDIA_ERR_DECODE`.
    DecodeError,
}

/// What a file gives: `Ok((video, audio, end, video frames))` or the error
/// of the open.
type Expect = Result<(bool, bool, End, usize), fn(&MediaError) -> bool>;

fn not_media(e: &MediaError) -> bool {
    matches!(e, MediaError::NotMedia(_))
}

fn unsupported(e: &MediaError) -> bool {
    matches!(e, MediaError::Unsupported(_))
}

const AV: (bool, bool) = (true, true);
const V: (bool, bool) = (true, false);
const A: (bool, bool) = (false, true);

/// The frame count of a cut or corrupt file: not checked.
const HOSTILE: usize = usize::MAX;

#[expect(
    clippy::unnecessary_wraps,
    reason = "a row of the table, which also has errors"
)]
fn ok(tracks: (bool, bool), end: End, frames: usize) -> Expect {
    Ok((tracks.0, tracks.1, end, frames))
}

/// Every file of `fixtures/media/`, with what it gives. The hostile files
/// end as the spike of ADR 0047 found (where Chromium fails or plays).
fn table() -> Vec<(&'static str, Expect)> {
    // No test file ends `Early`: the cut and corrupt WebM files end at the
    // end of their bytes (FFmpeg logs the damage and goes on).
    use End::{Complete, DecodeError};
    vec![
        ("aac-adts.aac", ok(A, Complete, 0)),
        ("flac.flac", ok(A, Complete, 0)),
        ("garbage.mp4", Err(not_media)),
        ("m4a-aac.m4a", ok(A, Complete, 0)),
        // 17 streams: the Matroska demuxer reads the first 16 (`max_streams`).
        ("mka-pcm-17-streams.mka", ok(A, Complete, HOSTILE)),
        ("mp3-long.mp3", ok(A, Complete, 0)),
        ("mp3-truncated.mp3", ok(A, Complete, HOSTILE)),
        ("mp3.mp3", ok(A, Complete, 0)),
        // M14 feature 6a: rows for the timeline rules.
        (
            "mp4-h264-aac-bframes-no-edit-list.mp4",
            ok(AV, Complete, 50),
        ),
        ("mp4-h264-aac-fragmented.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-aac-late-audio.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-aac-long.mp4", ok(AV, Complete, 250)),
        ("mp4-h264-aac-start1500.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-baseline-aac.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-bt601.mp4", ok(V, Complete, 50)),
        ("mp4-h264-bt709.mp4", ok(V, Complete, 50)),
        ("mp4-h264-fullrange-bt709.mp4", ok(V, Complete, 50)),
        ("mp4-h264-high-aac-corrupt.mp4", ok(AV, DecodeError, 0)),
        ("mp4-h264-high-aac-moov-at-end.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-high-aac-no-moov.mp4", Err(not_media)),
        ("mp4-h264-high-aac-truncated.mp4", ok(AV, DecodeError, 0)),
        ("mp4-h264-high-aac.mp4", ok(AV, Complete, 50)),
        ("mp4-h264-untagged-bt709-1280x720.mp4", ok(V, Complete, 50)),
        ("mp4-h264-untagged-bt709-640x480.mp4", ok(V, Complete, 50)),
        ("mp4-h264-untagged-bt709-720x576.mp4", ok(V, Complete, 50)),
        ("mp4-h264-untagged-bt709.mp4", ok(V, Complete, 50)),
        ("mp4-h264-video-only.mp4", ok(V, Complete, 50)),
        ("mp4-opus.mp4", ok(A, Complete, 0)),
        ("ogg-flac.oga", ok(A, Complete, 0)),
        ("ogg-opus.opus", ok(A, Complete, 0)),
        ("ogg-vorbis.ogg", ok(A, Complete, 0)),
        ("poster-200x100.png", Err(not_media)),
        ("poster-320x240.png", Err(not_media)),
        ("wav-pcm-s16.wav", ok(A, Complete, 0)),
        ("webm-vp8-321x241.webm", ok(V, Complete, 50)),
        ("webm-vp8-video-only.webm", ok(V, Complete, 50)),
        ("webm-vp8-vorbis-corrupt.webm", ok(AV, Complete, HOSTILE)),
        ("webm-vp8-vorbis-start1500.webm", ok(AV, Complete, 50)),
        ("webm-vp8-vorbis-truncated.webm", ok(AV, Complete, HOSTILE)),
        ("webm-vp8-vorbis.webm", ok(AV, Complete, 50)),
        ("webm-vp9-16400x16.webm", Err(unsupported)),
        ("webm-vp9-321x241.webm", ok(V, Complete, 50)),
        ("webm-vp9-8192x4416.webm", Err(unsupported)),
        ("webm-vp9-opus.webm", ok(AV, Complete, 50)),
    ]
}

#[test]
fn the_table_lists_every_test_file() {
    let mut names: Vec<String> = std::fs::read_dir(repo_path("fixtures/media"))
        .expect("fixtures/media is readable")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    let listed: Vec<&str> = table().iter().map(|(n, _)| *n).collect();
    assert_eq!(names, listed);
}

#[test]
fn every_test_file_opens_and_decodes_or_fails_as_expected() {
    let mut failures = Vec::new();
    for (name, expect) in table() {
        let got = open(name);
        match (expect, got) {
            (Err(kind), Err(e)) if kind(&e) => {}
            (Err(_), other) => failures.push(format!(
                "{name}: expected an open error, got {:?}",
                other.map(|f| f.info().clone())
            )),
            (Ok(_), Err(e)) => failures.push(format!("{name}: open failed: {e}")),
            (Ok((video, audio, end, frames)), Ok(mut file)) => {
                let info = file.info().clone();
                if info.video.is_some() != video || info.audio.is_some() != audio {
                    failures.push(format!("{name}: tracks {info:?}"));
                }
                let result = decode_all(&mut file);
                let got_end = match &result {
                    Ok(_) if file.error_at_end().is_some() => End::Early,
                    Ok(_) => End::Complete,
                    Err(MediaError::Decode(_)) => End::DecodeError,
                    Err(e) => {
                        failures.push(format!("{name}: {e}"));
                        continue;
                    }
                };
                if got_end != end {
                    failures.push(format!(
                        "{name}: ended {got_end:?}, expected {end:?} ({:?}, {:?})",
                        file.error_at_end(),
                        result.as_ref().err()
                    ));
                }
                if let Ok(t) = result
                    && frames != HOSTILE
                {
                    if t.video_frames != frames {
                        failures.push(format!("{name}: {} video frames", t.video_frames));
                    }
                    // 2 s or more of audio at 48 kHz.
                    if audio && t.audio_frames < 95_000 {
                        failures.push(format!("{name}: {} audio frames", t.audio_frames));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Chromium's metadata row: name, `duration`, `videoWidth x videoHeight`,
/// `currentTime` at the start.
fn chromium_rows() -> Vec<(String, f64, String, f64)> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/format-metadata.json"),
    )
    .expect("tests/data/format-metadata.json is readable");
    let json: serde_json::Value = serde_json::from_str(&text).expect("probe output is JSON");
    let rows = json["cases"][0]["results"][0]["value"]
        .as_array()
        .expect("the case returns rows");
    let field = |row: &[serde_json::Value], prefix: &str| -> String {
        row.iter()
            .filter_map(serde_json::Value::as_str)
            .find_map(|s| s.strip_prefix(prefix))
            .unwrap_or_default()
            .to_owned()
    };
    rows.iter()
        .filter_map(|r| r.as_array())
        .map(|r| {
            let name = r[0].as_str().unwrap_or_default().to_owned();
            let dur: f64 = field(r, "dur=").parse().unwrap_or(f64::NAN);
            let ct: f64 = field(r, "ct=").parse().unwrap_or(f64::NAN);
            (name, dur, r[5].as_str().unwrap_or_default().to_owned(), ct)
        })
        .collect()
}

/// The element's start and duration come from FFmpeg's values through the
/// rules per container (`swb_media::timeline`, ADR 0047 section 7, "The
/// origin"); before them, five rows differed: the duration of the MP4 with
/// a start time (FFmpeg: 2.023991 s from the start; Chromium: 3.496145),
/// the start of the MP3 files (FFmpeg: the encoder delay, 0.025057 s), and
/// the start of WAV and ADTS (FFmpeg: none).
#[test]
fn metadata_matches_chromium() {
    let rows = chromium_rows();
    assert_eq!(rows.len(), 25);
    let mut failures = Vec::new();
    for (name, dur, size, ct) in rows {
        let file = match open(&name) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let info = file.info();
        let got_size = info.video.as_ref().map_or_else(
            || "undefinedxundefined".to_owned(),
            |v| format!("{}x{}", v.natural_size.0, v.natural_size.1),
        );
        if got_size != size {
            failures.push(format!("{name}: size {got_size}, Chromium {size}"));
        }
        let us = |s: f64| (s * 1e6).round() as i64;
        let t = timeline(info);
        if t.duration_us != Some(us(dur)) {
            failures.push(format!(
                "{name} ({}): duration {:?}, Chromium {dur}",
                info.container, t.duration_us
            ));
        }
        if t.start_us != us(ct) {
            failures.push(format!(
                "{name} ({}): start {}, Chromium {ct}",
                info.container, t.start_us
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn codec_information_for_can_play_type() {
    let file = open("mp4-h264-high-aac.mp4").expect("opens");
    let info = file.info();
    assert!(info.container.contains("mp4"), "{}", info.container);
    let video = info.video.as_ref().expect("a video track");
    assert_eq!(video.codec, Codec::H264);
    assert_eq!(video.profile, "High");
    assert_eq!(video.pixel_format, "yuv420p");
    assert_eq!(video.frame_rate, Some((25, 1)));
    let audio = info.audio.as_ref().expect("an audio track");
    assert_eq!(audio.codec, Codec::Aac);
    assert_eq!(audio.profile, "LC");
    assert_eq!((audio.sample_rate, audio.channels), (44_100, 2));

    let codecs = |name: &str| {
        let f = open(name).expect("opens");
        let i = f.info();
        (
            i.video.as_ref().map(|v| v.codec.clone()),
            i.audio.as_ref().map(|a| a.codec.clone()),
        )
    };
    assert_eq!(
        codecs("webm-vp8-vorbis.webm"),
        (Some(Codec::Vp8), Some(Codec::Vorbis))
    );
    assert_eq!(
        codecs("webm-vp9-opus.webm"),
        (Some(Codec::Vp9), Some(Codec::Opus))
    );
    assert_eq!(codecs("mp3.mp3"), (None, Some(Codec::Mp3)));
    assert_eq!(codecs("ogg-flac.oga"), (None, Some(Codec::Flac)));
    assert_eq!(
        codecs("wav-pcm-s16.wav"),
        (None, Some(Codec::Pcm("pcm_s16le".to_owned())))
    );
}
