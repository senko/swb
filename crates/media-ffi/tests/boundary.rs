//! The checks of the boundary: limits (`max_pixels`, `max_streams`), bad
//! arguments (stream indices, buffer sizes), and frames that outlive their
//! decoder (ADR 0047 sections 4 and 11).
#![cfg(feature = "ffmpeg")]

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use swb_media_ffi::ffmpeg::{
    Decoder, Demuxer, DemuxerLimits, Frame, Input, Received, Resampler, Scaler, Sent, StreamKind,
    YuvMatrix,
};

struct FileInput(File, u64);

impl Input for FileInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.0.seek(SeekFrom::Start(pos))
    }

    fn size(&mut self) -> Option<u64> {
        Some(self.1)
    }
}

const LIMITS: DemuxerLimits = DemuxerLimits {
    max_streams: 16,
    probe_bytes: 5_000_000,
    max_analyze_us: 5_000_000,
};

fn open(name: &str, limits: &DemuxerLimits) -> Result<Demuxer, swb_media_ffi::ffmpeg::Error> {
    let path = format!("{}/../../fixtures/media/{name}", env!("CARGO_MANIFEST_DIR"));
    let file = File::open(path).expect("the test media are in fixtures/media");
    let size = file.metadata().expect("metadata").len();
    Demuxer::open(Box::new(FileInput(file, size)), limits)
}

fn video_stream(d: &Demuxer) -> usize {
    d.streams()
        .iter()
        .position(|s| s.kind == StreamKind::Video)
        .expect("a video stream")
}

/// Decodes the first frame of `stream`.
fn first_frame(d: &mut Demuxer, dec: &mut Decoder, stream: usize) -> Result<Frame, String> {
    loop {
        match dec.receive().map_err(|e| e.to_string())? {
            Received::Frame(f) => return Ok(f),
            Received::Ended => return Err("ended".to_owned()),
            Received::NeedsInput => {}
        }
        match d.read().map_err(|e| e.to_string())? {
            Some(p) if p.info().stream == stream => {
                if dec.send(Some(&p)).map_err(|e| e.to_string())? == Sent::Full {
                    return Err("full".to_owned());
                }
            }
            Some(_) => {}
            None => {
                dec.send(None).map_err(|e| e.to_string())?;
            }
        }
    }
}

#[test]
fn max_pixels_stops_larger_frames() {
    let mut d = open("mp4-h264-high-aac.mp4", &LIMITS).expect("opens");
    let v = video_stream(&d);
    let mut large = d.decoder(v, 1, 8192 * 4352).expect("the decoder opens");
    let frame = first_frame(&mut d, &mut large, v).expect("a frame");
    let info = frame.video().expect("video");
    assert_eq!((info.width, info.height), (320, 240));

    let mut d = open("mp4-h264-high-aac.mp4", &LIMITS).expect("opens");
    // The decoder fails at the open (the stream's size) or at the first
    // frame.
    if let Ok(mut small) = d.decoder(v, 1, 320 * 240 - 1) {
        assert!(first_frame(&mut d, &mut small, v).is_err());
    }
}

#[test]
fn max_streams_bounds_the_streams() {
    let one = DemuxerLimits {
        max_streams: 1,
        ..LIMITS
    };
    // FFmpeg's demuxers either fail or keep the first streams.
    for name in [
        "mp4-h264-high-aac.mp4",
        "webm-vp8-vorbis.webm",
        "mka-pcm-17-streams.mka",
    ] {
        if let Ok(d) = open(name, &one) {
            assert!(d.streams().len() <= 1, "{name}");
        }
    }
}

/// FFmpeg's context refuses a second resource, so a container cannot make
/// swb open a file or a URL (ADR 0047 section 11). No enabled demuxer asks
/// for one with swb's options (the MOV demuxer needs `enable_drefs`), so the
/// refusal is checked on the context directly. The build has no protocols,
/// so FFmpeg's default would fail as well: the check is for swb's own error.
#[test]
fn the_demuxer_refuses_to_open_a_second_resource() {
    for name in ["mp4-h264-high-aac.mp4", "webm-vp8-vorbis.webm", "mp3.mp3"] {
        let d = open(name, &LIMITS).expect("opens");
        assert!(d.refuses_second_resource(), "{name}");
    }
}

#[test]
fn bad_arguments_are_errors() {
    let mut d = open("webm-vp8-vorbis.webm", &LIMITS).expect("opens");
    assert!(d.decoder(99, 1, 0).is_err());
    assert!(d.decoder(usize::MAX, 1, 0).is_err());
    assert!(d.seek(99, 0).is_err());
    assert!(
        d.select(Some(99), None).is_ok(),
        "unknown indices discard all"
    );

    let mut d = open("webm-vp8-vorbis.webm", &LIMITS).expect("opens");
    let v = video_stream(&d);
    let mut dec = d.decoder(v, 1, 0).expect("the decoder opens");
    let frame = first_frame(&mut d, &mut dec, v).expect("a frame");
    let mut scaler = Scaler::new().expect("a scaler");
    let mut small = vec![0u8; 320 * 4 * 239];
    assert!(
        scaler
            .rgba(
                &frame,
                YuvMatrix::Bt601,
                false,
                (320, 240),
                &mut small,
                320 * 4
            )
            .is_err()
    );
    let mut buf = vec![0u8; 320 * 4 * 240];
    assert!(
        scaler
            .rgba(
                &frame,
                YuvMatrix::Bt601,
                false,
                (320, 240),
                &mut buf,
                319 * 4
            )
            .is_err()
    );
    assert!(
        scaler
            .rgba(&frame, YuvMatrix::Bt601, false, (0, 240), &mut buf, 0)
            .is_err()
    );
    scaler
        .rgba(
            &frame,
            YuvMatrix::Bt601,
            false,
            (320, 240),
            &mut buf,
            320 * 4,
        )
        .expect("converts");
    // A video frame is no audio frame.
    let mut resampler = Resampler::new(48_000, 2).expect("a resampler");
    let mut out = Vec::new();
    assert!(resampler.convert(Some(&frame), &mut out).is_err());
    assert_eq!(out.len(), 0);
    assert!(Resampler::new(48_000, 9).is_err());
    assert!(Resampler::new(0, 2).is_err());
}

/// `swscale` writes only the rows of the destination: the bytes after the
/// last row stay as they were (the sanitizers do not see FFmpeg's writes).
/// Without the scratch buffer for widths that are not a multiple of 64, the
/// 8-bit 4:2:0 and 4:2:2 files overrun the destination by up to 60 bytes (M14
/// feature 7a, fix round 2).
#[test]
fn the_conversion_stays_in_its_buffer() {
    for (name, w, h) in [
        ("webm-vp9-321x241.webm", 321u32, 241u32),
        ("webm-vp8-321x241.webm", 321, 241),
        ("webm-vp8-321x240.webm", 321, 240),
        ("webm-vp8-320x241.webm", 320, 241),
        ("mp4-h264-high-aac.mp4", 320, 240),
        ("mp4-h264-fullrange-bt709.mp4", 320, 240),
        ("mp4-h264-854x480.mp4", 854, 480),
        ("mp4-h264-322x242.mp4", 322, 242),
        ("mp4-h264-322x242-444.mp4", 322, 242),
        ("mp4-h264-322x242-422.mp4", 322, 242),
        ("mp4-h264-322x242-10bit.mp4", 322, 242),
        ("mp4-h264-untagged-bt709-1920x1080.mp4", 1920, 1080),
    ] {
        let mut d = open(name, &LIMITS).expect("opens");
        let v = video_stream(&d);
        let mut dec = d.decoder(v, 1, 0).expect("the decoder opens");
        let frame = first_frame(&mut d, &mut dec, v).expect("a frame");
        let stride = w as usize * 4;
        let size = stride * h as usize;
        for (matrix, full) in [(YuvMatrix::Bt601, false), (YuvMatrix::Bt709, true)] {
            let mut buf = vec![0xABu8; size + 256];
            Scaler::new()
                .expect("a scaler")
                .rgba(&frame, matrix, full, (w, h), &mut buf, stride)
                .expect("converts");
            assert!(buf[size..].iter().all(|b| *b == 0xAB), "{name}");
            assert!(buf[..size].chunks(4).all(|p| p[3] == 255), "{name}");
        }
    }
}

/// A scaler that is reused for another destination size, matrix or range
/// gives what a new scaler gives (the coefficients are set again).
#[test]
fn a_reused_scaler_follows_its_parameters() {
    let mut d = open("mp4-h264-high-aac.mp4", &LIMITS).expect("opens");
    let v = video_stream(&d);
    let mut dec = d.decoder(v, 1, 0).expect("the decoder opens");
    let frame = first_frame(&mut d, &mut dec, v).expect("a frame");
    let convert = |scaler: &mut Scaler, matrix, full, (w, h): (u32, u32)| {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        scaler
            .rgba(&frame, matrix, full, (w, h), &mut buf, w as usize * 4)
            .expect("converts");
        buf
    };
    let mut reused = Scaler::new().expect("a scaler");
    let steps = [
        (YuvMatrix::Bt709, false, (320, 240)),
        (YuvMatrix::Bt709, false, (160, 120)),
        (YuvMatrix::Bt709, false, (320, 240)),
        (YuvMatrix::Bt601, false, (320, 240)),
        (YuvMatrix::Bt601, true, (320, 240)),
        (YuvMatrix::Bt601, true, (200, 100)),
    ];
    for (matrix, full, size) in steps {
        let got = convert(&mut reused, matrix, full, size);
        let fresh = convert(&mut Scaler::new().expect("a scaler"), matrix, full, size);
        assert!(got == fresh, "{matrix:?} {full} {size:?}");
    }
}

#[test]
fn frames_and_packets_outlive_their_owners() {
    let mut d = open("webm-vp9-opus.webm", &LIMITS).expect("opens");
    let v = video_stream(&d);
    let mut dec = d.decoder(v, 2, 0).expect("the decoder opens");
    let frame = first_frame(&mut d, &mut dec, v).expect("a frame");
    let packet = d.read().expect("reads").expect("a packet");
    drop(dec);
    drop(d);
    // On another thread, after the decoder and the demuxer are gone.
    let handle = std::thread::spawn(move || {
        let mut scaler = Scaler::new().expect("a scaler");
        let mut buf = vec![0u8; 320 * 4 * 240];
        scaler
            .rgba(
                &frame,
                YuvMatrix::Bt709,
                false,
                (320, 240),
                &mut buf,
                320 * 4,
            )
            .expect("converts");
        (packet.info().size, buf.chunks(4).all(|p| p[3] == 255))
    });
    let (size, opaque) = handle.join().expect("the thread runs");
    assert!(size > 0);
    assert!(opaque);
}
