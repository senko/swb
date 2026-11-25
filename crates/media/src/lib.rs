//! Media playback (ADR 0047): byte sources, the metadata of a media file,
//! and its decoding into RGBA frames and 48 kHz stereo `f32` samples.
//!
//! With the feature `ffmpeg` (on in `swb --features media`), `MediaFile`
//! opens a `ByteSource` through FFmpeg (`swb-media-ffi`), reports its
//! metadata and decodes its frames and samples in order and after a seek.
//! Its parts, `Demux`, `VideoDecoder` and `AudioDecoder`, are `Send`, so
//! that the pipeline (M14 feature 5) can run the demuxer and the audio on
//! one thread and the video on another. Without the feature only the byte
//! sources, the metadata types and the color rules exist.
//!
//! This crate is safe Rust; the C boundary and all `unsafe` code are in
//! `swb-media-ffi`.

mod error;
mod info;
mod source;

#[cfg(feature = "ffmpeg")]
mod decode;
#[cfg(feature = "ffmpeg")]
mod file;

pub use error::MediaError;
pub use info::{
    AudioTrack, Codec, ColorTags, Conversion, Matrix, MatrixTag, MediaInfo, RangeTag,
    UNTAGGED_BT709_LINES, VideoTrack, conversion, natural_size,
};
pub use source::{ByteSource, FileSource, MemorySource};

#[cfg(feature = "ffmpeg")]
pub use decode::{AudioChunk, AudioDecoder, Demux, Output, Packet, RawVideoFrame, VideoDecoder};
#[cfg(feature = "ffmpeg")]
pub use file::{Decoded, MediaFile, VideoFrame};

/// The limits for hostile media (ADR 0047 section 11).
pub mod limits {
    /// A file with more streams fails to open (FFmpeg's `max_streams`).
    pub const MAX_STREAMS: u32 = 16;
    /// The bytes that the demuxer reads to find the streams (`probesize`).
    pub const PROBE_BYTES: u64 = 5_000_000;
    /// The media time that the demuxer reads to find the streams
    /// (`max_analyze_duration`), in microseconds.
    pub const MAX_ANALYZE_US: i64 = 5_000_000;
    /// The largest width and height of a video, in pixels.
    pub const MAX_DIMENSION: u32 = 16_384;
    /// The most pixels of a video frame (8192 x 4352).
    pub const MAX_PIXELS: u64 = 8192 * 4352;
    /// The most audio channels of a stream.
    pub const MAX_CHANNELS: u32 = 8;
    /// The lowest sample rate of a stream.
    pub const MIN_SAMPLE_RATE: u32 = 8_000;
    /// The highest sample rate of a stream.
    pub const MAX_SAMPLE_RATE: u32 = 384_000;
    /// The largest single allocation of FFmpeg (`av_max_alloc`).
    pub const MAX_ALLOC: usize = 512 << 20;
    /// Videos below this many pixels decode on one thread; larger ones use
    /// up to `MAX_DECODE_THREADS` (ADR 0047 section 6).
    pub const ONE_THREAD_BELOW_PIXELS: u64 = 1280 * 720;
    /// The most decode threads of a video.
    pub const MAX_DECODE_THREADS: u32 = 8;
}

/// The sample rate of decoded audio.
pub const OUTPUT_RATE: u32 = 48_000;
/// The channels of decoded audio (interleaved).
pub const OUTPUT_CHANNELS: u32 = 2;

/// Whether this build has media support (the feature `ffmpeg`).
pub const ENABLED: bool = cfg!(feature = "ffmpeg");

/// One line per media component of this build, for `swb --version`
/// (ADR 0047 section 2). Empty without media support.
pub fn version_lines() -> Vec<String> {
    #[cfg(feature = "ffmpeg")]
    {
        use swb_media_ffi::ffmpeg;
        // `--prefix` names the directory of the build; the options are the
        // pinned ones.
        let configure: Vec<String> = ffmpeg::configuration()
            .split_whitespace()
            .filter(|o| !o.starts_with("--prefix="))
            .map(|o| o.replace('\'', ""))
            .collect();
        vec![format!(
            "FFmpeg {} (LGPL-2.1-or-later; configure: {})",
            ffmpeg::version(),
            configure.join(" ")
        )]
    }
    #[cfg(not(feature = "ffmpeg"))]
    {
        Vec::new()
    }
}

/// The license texts of the media components of this build, as (title,
/// text), for `swb --licenses`. Empty without media support.
pub fn license_texts() -> Vec<(&'static str, &'static str)> {
    #[cfg(feature = "ffmpeg")]
    {
        vec![(
            "GNU Lesser General Public License, version 2.1 (FFmpeg)",
            include_str!("../../../licenses/LGPL-2.1.txt"),
        )]
    }
    #[cfg(not(feature = "ffmpeg"))]
    {
        Vec::new()
    }
}
