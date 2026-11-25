//! Audio: the samples of every format are the test sines (440 Hz on the
//! left, 660 Hz on the right) at 48 kHz stereo, also after a seek.
#![cfg(feature = "ffmpeg")]

mod common;

use common::open;
use swb_media::{Decoded, MediaFile, OUTPUT_RATE};

/// Decodes audio from the current position until `seconds` of samples;
/// returns the time of the first sample and the interleaved samples.
fn samples(file: &mut MediaFile, seconds: f64) -> (i64, Vec<f32>) {
    let want = (seconds * f64::from(OUTPUT_RATE)) as usize * 2;
    let mut first = None;
    let mut out = Vec::new();
    while out.len() < want {
        match file.decode_next().expect("decodes") {
            Some(Decoded::Audio(c)) => {
                first = first.or(Some(c.pts_us));
                out.extend_from_slice(&c.samples);
            }
            Some(Decoded::Video(_)) => {}
            None => break,
        }
    }
    (first.expect("audio samples"), out)
}

/// The frequency of one channel by its zero crossings, and its RMS.
fn tone(samples: &[f32], channel: usize) -> (f64, f64) {
    let ch: Vec<f32> = samples.iter().skip(channel).step_by(2).copied().collect();
    let crossings = ch
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    let seconds = ch.len() as f64 / f64::from(OUTPUT_RATE);
    let rms = (ch.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / ch.len() as f64).sqrt();
    (crossings as f64 / 2.0 / seconds, rms)
}

/// Checks the two sines in `samples`.
fn check_sines(name: &str, samples: &[f32]) {
    for (channel, hz) in [(0, 440.0), (1, 660.0)] {
        let (freq, rms) = tone(samples, channel);
        assert!(
            (freq - hz).abs() < hz * 0.02,
            "{name} channel {channel}: {freq:.1} Hz"
        );
        // `sine` has the amplitude 1/8: RMS 0.088.
        assert!(
            (0.07..0.1).contains(&rms),
            "{name} channel {channel}: RMS {rms:.3}"
        );
    }
}

const AUDIO_FILES: [&str; 10] = [
    "aac-adts.aac",
    "flac.flac",
    "m4a-aac.m4a",
    "mp3.mp3",
    "mp4-h264-high-aac.mp4",
    "ogg-flac.oga",
    "ogg-opus.opus",
    "ogg-vorbis.ogg",
    "wav-pcm-s16.wav",
    "webm-vp9-opus.webm",
];

#[test]
fn every_format_gives_the_sines() {
    for name in AUDIO_FILES {
        let mut file = open(name).expect("opens");
        file.set_tracks(false, true).expect("tracks");
        // Skip the first 0.1 s (encoder delay and fade-in), then 0.5 s.
        let _ = samples(&mut file, 0.1);
        let (_, s) = samples(&mut file, 0.5);
        check_sines(name, &s);
        assert!(s.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{name}");
    }
}

/// The largest difference between `got` (interleaved) and the reference
/// from the sample `offset` on; `None` when the reference is too short.
fn worst_diff(got: &[f32], reference: &[f32], offset: usize) -> Option<f32> {
    let span = reference.get(offset * 2..offset * 2 + got.len())?;
    Some(
        got.iter()
            .zip(span)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max),
    )
}

#[test]
fn a_seek_starts_the_samples_at_the_target() {
    // (file, video on, target, the largest shift of the content that the
    // container's time resolution allows, in samples at 48 kHz). WebM
    // times its blocks in whole milliseconds, so after a seek the first
    // sample can be 0.5 ms (24 samples) from its true time; the other
    // containers keep exact times.
    let cases = [
        ("m4a-aac.m4a", false, 1_000_000, 0),
        ("ogg-vorbis.ogg", false, 1_000_000, 0),
        ("flac.flac", false, 1_000_000, 0),
        ("mp3-long.mp3", false, 4_440_000, 0),
        ("webm-vp8-vorbis.webm", false, 1_000_000, 24),
        // With the video on, the demuxer seeks to the video's key frame.
        ("mp4-h264-aac-long.mp4", true, 4_440_000, 0),
        ("webm-vp8-vorbis.webm", true, 1_300_000, 24),
    ];
    for (name, video, target, max_shift) in cases {
        // The same audio decoded in order from the start.
        let mut reference = open(name).expect("opens");
        reference.set_tracks(false, true).expect("tracks");
        let (ref_start, ref_samples) = samples(&mut reference, 1000.0);
        let mut file = open(name).expect("opens");
        file.set_tracks(video, true).expect("tracks");
        // Read a little, then seek; the Ogg demuxer goes back to the start
        // for small files, and the samples before the target are dropped.
        let _ = samples(&mut file, 0.2);
        file.seek(target).expect("seeks");
        let (pts, s) = samples(&mut file, 0.3);
        // Within one sample of the target.
        assert!((pts - target).abs() <= 21, "{name}: first sample at {pts}");
        check_sines(name, &s);
        // The samples are those of the in-order decode at that time: a
        // shift of the content against the times fails here. A shift of
        // one sample of the 660 Hz sine at amplitude 1/8 changes a sample by
        // up to 0.0108. A new resampler after the seek starts at another
        // fractional phase than the in-order one (44.1 kHz to 48 kHz: up
        // to half a sample, 0.0054), so the tolerance is 0.007.
        let offset = ((pts - ref_start) as f64 * f64::from(OUTPUT_RATE) / 1e6).round() as usize;
        let best = (offset.saturating_sub(max_shift)..=offset + max_shift)
            .filter_map(|o| worst_diff(&s, &ref_samples, o))
            .fold(f32::MAX, f32::min);
        assert!(
            best < 0.007,
            "{name}: differs from the in-order decode by {best} (shift up to {max_shift})"
        );
    }
}

#[test]
fn audio_and_video_come_in_container_order() {
    let mut file = open("webm-vp8-vorbis.webm").expect("opens");
    let (mut frames, mut chunks) = (0, 0);
    let mut last_audio = i64::MIN;
    while let Some(d) = file.decode_next().expect("decodes") {
        match d {
            Decoded::Video(_) => frames += 1,
            Decoded::Audio(c) => {
                assert!(c.pts_us > last_audio);
                last_audio = c.pts_us;
                chunks += 1;
            }
        }
    }
    assert_eq!(frames, 50);
    assert!(chunks > 10);
}
