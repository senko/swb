//! Helpers of the decoding tests: the test media of `fixtures/media/`.
#![allow(dead_code)]

pub(crate) mod driver;

use std::path::PathBuf;

use swb_media::{Decoded, FileSource, MediaError, MediaFile};

/// The path of a file of the repository.
pub(crate) fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// The path of a test media file.
pub(crate) fn fixture(name: &str) -> PathBuf {
    repo_path("fixtures/media").join(name)
}

/// The bytes of a test media file.
pub(crate) fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("the test media are in fixtures/media")
}

/// Opens a test media file through a `FileSource`.
pub(crate) fn open(name: &str) -> Result<MediaFile, MediaError> {
    let source = FileSource::open(fixture(name)).expect("the test media are in fixtures/media");
    MediaFile::open(Box::new(source))
}

/// What decoding a whole file gives.
#[derive(Debug, Default)]
pub(crate) struct Totals {
    pub(crate) video_frames: usize,
    pub(crate) audio_frames: usize,
    pub(crate) first_video_pts: Option<i64>,
    pub(crate) last_video_pts: Option<i64>,
    pub(crate) first_audio_pts: Option<i64>,
}

/// Decodes `file` to its end.
pub(crate) fn decode_all(file: &mut MediaFile) -> Result<Totals, MediaError> {
    let mut t = Totals::default();
    while let Some(d) = file.decode_next()? {
        match d {
            Decoded::Video(f) => {
                t.video_frames += 1;
                t.first_video_pts = t.first_video_pts.or(f.pts_us);
                t.last_video_pts = f.pts_us;
            }
            Decoded::Audio(c) => {
                t.first_audio_pts = t.first_audio_pts.or(Some(c.pts_us));
                t.audio_frames += c.frames();
            }
        }
    }
    Ok(t)
}

/// The RGB of a pixel of an image.
pub(crate) fn rgb(image: &tiny_skia::Pixmap, x: u32, y: u32) -> Option<[u8; 3]> {
    let p = image.pixel(x, y)?;
    Some([p.red(), p.green(), p.blue()])
}
