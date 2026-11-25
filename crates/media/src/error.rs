//! The errors of opening and decoding media, by what the media element
//! makes of them (ADR 0047 section 11).

/// Why a media file cannot be opened or decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// No demuxer of the build opens the bytes (an image, a text, garbage,
    /// an MP4 without its index): `MEDIA_ERR_SRC_NOT_SUPPORTED`.
    #[error("not a media file: {0}")]
    NotMedia(String),
    /// The container opens, but has no stream that swb plays, or a stream
    /// over a limit of ADR 0047 section 11: `MEDIA_ERR_SRC_NOT_SUPPORTED`.
    #[error("unsupported media: {0}")]
    Unsupported(String),
    /// The byte source failed (a network error or a cancellation), not the
    /// bytes: `MEDIA_ERR_NETWORK`, or nothing after a cancellation.
    #[error("the byte source failed: {0}")]
    Source(String),
    /// A decoder failed, or a frame is over a limit: `MEDIA_ERR_DECODE`.
    #[error("decode error: {0}")]
    Decode(String),
    /// A seek failed (the source cannot seek, or the demuxer cannot).
    #[error("seek failed: {0}")]
    Seek(String),
    /// This build has no media support (the `media` feature is off).
    #[error("this build has no media support")]
    NotBuilt,
}

impl MediaError {
    /// The `MediaError.code` of HTML that the element reports (1 aborted,
    /// 2 network, 3 decode, 4 source not supported), or `None` when the
    /// error is not reported as such (a failed seek).
    pub fn element_code(&self) -> Option<u16> {
        match self {
            MediaError::NotMedia(_) | MediaError::Unsupported(_) | MediaError::NotBuilt => Some(4),
            MediaError::Decode(_) => Some(3),
            MediaError::Source(_) => Some(2),
            MediaError::Seek(_) => None,
        }
    }
}
