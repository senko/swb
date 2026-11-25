//! Byte sources: short reads, errors, a source that cannot seek, a source
//! that panics; and the limits of ADR 0047 section 11.
#![cfg(feature = "ffmpeg")]

mod common;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{decode_all, fixture_bytes, open};
use swb_media::{ByteSource, Decoded, MediaError, MediaFile, MemorySource};

/// Reads at most 7 bytes at a time, and fails every 5th read with
/// `Interrupted` (which the boundary retries).
struct ShortReads {
    inner: MemorySource,
    reads: u32,
}

impl ByteSource for ShortReads {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        if self.reads.is_multiple_of(5) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let n = buf.len().min(7);
        self.inner.read(&mut buf[..n])
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.inner.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.inner.size()
    }
}

/// Fails every read after `limit` bytes, as a broken connection.
struct FailsAfter {
    inner: MemorySource,
    left: usize,
}

impl ByteSource for FailsAfter {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "connection reset",
            ));
        }
        let n = buf.len().min(self.left);
        let n = self.inner.read(&mut buf[..n])?;
        self.left -= n;
        Ok(n)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.inner.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.inner.size()
    }
}

/// Fails the `n`th read once and then works again, as a connection that
/// recovers.
struct FailsOnce {
    inner: MemorySource,
    reads: Arc<AtomicU32>,
    n: u32,
}

impl ByteSource for FailsOnce {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if count == self.n {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "one timeout"));
        }
        self.inner.read(buf)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.inner.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.inner.size()
    }
}

/// Panics on the first read.
struct Panics;

impl ByteSource for Panics {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("a source that panics");
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        Ok(pos)
    }

    fn size(&mut self) -> Option<u64> {
        Some(1000)
    }
}

/// The video frames' times and the first frame's pixels.
fn video_frames(file: &mut MediaFile) -> (Vec<Option<i64>>, Vec<u8>) {
    let mut times = Vec::new();
    let mut first = Vec::new();
    while let Some(d) = file.decode_next().expect("decodes") {
        if let Decoded::Video(f) = d {
            if times.is_empty() {
                first = f.image.data().to_vec();
            }
            times.push(f.pts_us);
        }
    }
    (times, first)
}

#[test]
fn short_reads_give_the_same_frames() {
    for name in ["mp4-h264-high-aac-moov-at-end.mp4", "webm-vp9-opus.webm"] {
        let mut plain = open(name).expect("opens");
        let expected = video_frames(&mut plain);
        let source = ShortReads {
            inner: MemorySource::new(fixture_bytes(name)),
            reads: 0,
        };
        let mut file = MediaFile::open(Box::new(source)).expect("opens with short reads");
        assert_eq!(video_frames(&mut file), expected, "{name}");
    }
}

#[test]
fn a_failing_source_is_a_source_error() {
    let name = "mp4-h264-high-aac.mp4";
    // During the open.
    let source = FailsAfter {
        inner: MemorySource::new(fixture_bytes(name)),
        left: 1000,
    };
    match MediaFile::open(Box::new(source)) {
        Err(MediaError::Source(m)) => assert!(m.contains("connection reset"), "{m}"),
        other => panic!(
            "expected a source error, got {:?}",
            other.map(|f| f.info().clone())
        ),
    }
    // While decoding: the index is first, the data fails in the middle.
    let source = FailsAfter {
        inner: MemorySource::new(fixture_bytes(name)),
        left: 30_000,
    };
    let mut file = MediaFile::open(Box::new(source)).expect("the index is in the first bytes");
    match decode_all(&mut file) {
        Err(e @ MediaError::Source(_)) => assert_eq!(e.element_code(), Some(2)),
        other => panic!("expected a source error, got {other:?}"),
    }
}

/// A read error that FFmpeg tolerates during the open (the open succeeds)
/// is not reported again by the first read after it.
#[test]
fn an_error_that_the_open_survived_is_forgotten() {
    let mut survived = 0;
    for name in [
        "mp4-h264-high-aac.mp4",
        "mp4-h264-high-aac-moov-at-end.mp4",
        "webm-vp8-vorbis.webm",
        "ogg-vorbis.ogg",
        "mp3.mp3",
    ] {
        for n in 1..12 {
            let reads = Arc::new(AtomicU32::new(0));
            let source = FailsOnce {
                inner: MemorySource::new(fixture_bytes(name)),
                reads: Arc::clone(&reads),
                n,
            };
            let Ok(mut file) = MediaFile::open(Box::new(source)) else {
                continue;
            };
            // Only a failure inside the open counts: a later one is a real
            // source error.
            if reads.load(Ordering::SeqCst) < n {
                continue;
            }
            survived += 1;
            // The failure was in the past; what follows is not a source
            // error (the demuxer may end early at worst).
            loop {
                match file.decode_next() {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(e) => panic!("{name} (read {n}): {e:?}"),
                }
            }
        }
    }
    assert!(survived > 0);
}

#[test]
fn a_panicking_source_is_a_source_error() {
    match MediaFile::open(Box::new(Panics)) {
        Err(MediaError::Source(m)) => assert!(m.contains("panicked"), "{m}"),
        other => panic!(
            "expected a source error, got {:?}",
            other.map(|f| f.info().clone())
        ),
    }
}

#[test]
fn a_source_without_seeking_plays_but_does_not_seek() {
    for name in ["webm-vp8-vorbis.webm", "ogg-vorbis.ogg", "mp3.mp3"] {
        let mut file =
            MediaFile::open(Box::new(MemorySource::stream(fixture_bytes(name)))).expect("opens");
        assert!(!file.info().seekable, "{name}");
        let totals = decode_all(&mut file).expect("decodes");
        assert!(totals.audio_frames > 90_000, "{name}: {totals:?}");
        assert!(
            matches!(file.seek(500_000), Err(MediaError::Seek(_))),
            "{name}"
        );
    }
}

#[test]
fn videos_over_the_size_limits_fail_at_the_metadata() {
    for name in ["webm-vp9-16400x16.webm", "webm-vp9-8192x4416.webm"] {
        match open(name) {
            Err(e @ MediaError::Unsupported(_)) => assert_eq!(e.element_code(), Some(4)),
            other => panic!("{name}: {:?}", other.map(|f| f.info().clone())),
        }
    }
}

#[test]
fn at_most_16_streams() {
    let file = open("mka-pcm-17-streams.mka").expect("opens");
    assert_eq!(file.info().stream_count, 16);
}

#[test]
fn empty_and_tiny_sources_are_not_media() {
    for bytes in [Vec::new(), vec![0u8; 1], b"<html>".to_vec()] {
        let result = MediaFile::open(Box::new(MemorySource::new(bytes)));
        assert!(matches!(result, Err(MediaError::NotMedia(_))));
    }
}
