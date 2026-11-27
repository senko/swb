//! The exact seek (ADR 0047 section 7, "Seeking"): the demuxer seek that
//! both the pipeline and `MediaFile` use ([`seek_demux`]), and the filter
//! that keeps the last video frame at or before the target
//! ([`SeekFilter`]).

use crate::MediaError;
use crate::decode::{Demux, RawVideoFrame};

/// How much earlier than its target a seek of audio alone starts decoding
/// (the samples before the target are dropped).
const AUDIO_PREROLL_US: i64 = 200_000;

/// Seeks `demux` for an exact seek to `target_us` (stream time) with the
/// tracks that are on: to the video's key frame at or before the target,
/// or, for audio alone, to the audio packet at or before the target minus
/// the preroll (audio decoders need the packets before the target: MP3's
/// bit reservoir, Opus's pre-skip; every audio packet is a key frame).
///
/// The seek time is never below 0 (the MP3 and ADTS demuxers refuse a seek
/// to a negative time, so a loop to the start or a seek under the preroll
/// would fail and leave the demuxer at its end). If a seek fails for another
/// reason than the source, it tries once more at the start of the stream;
/// the caller still drops the samples and frames before the target. If that
/// fails too, the error goes to the caller.
pub(crate) fn seek_demux(
    demux: &mut Demux,
    video_stream: Option<usize>,
    audio_stream: Option<usize>,
    target_us: i64,
) -> Result<(), MediaError> {
    let (stream, demux_target) = match (video_stream, audio_stream) {
        (Some(v), _) => (v, target_us),
        (None, Some(a)) => (a, target_us.saturating_sub(AUDIO_PREROLL_US)),
        (None, None) => return Err(MediaError::Seek("no track is on".to_owned())),
    };
    let demux_target = demux_target.max(0);
    match demux.seek(stream, demux_target) {
        Err(e @ MediaError::Seek(_)) if demux_target > 0 => {
            log::warn!("media: seek to {demux_target} us failed ({e}); trying the start");
            demux.seek(stream, 0)
        }
        result => result,
    }
}

/// The video frames after an exact seek: the frames before the target are
/// dropped except the last one at or before it, which comes out when the
/// first frame after the target (or the end) shows that it is the last.
#[derive(Default)]
pub(crate) struct SeekFilter {
    target: Option<i64>,
    held: Option<RawVideoFrame>,
}

impl SeekFilter {
    /// Starts filtering for a seek to `target_us` (stream time).
    pub(crate) fn start(&mut self, target_us: i64) {
        self.target = Some(target_us);
        self.held = None;
    }

    /// Stops filtering and drops the held frame.
    pub(crate) fn reset(&mut self) {
        self.target = None;
        self.held = None;
    }

    /// Takes a decoded frame; returns the frames that come out, in order:
    /// the held frame (the one at the target), then `frame`.
    pub(crate) fn offer(
        &mut self,
        frame: RawVideoFrame,
    ) -> (Option<RawVideoFrame>, Option<RawVideoFrame>) {
        let Some(target) = self.target else {
            return (None, Some(frame));
        };
        if frame.pts_us().is_none_or(|pts| pts <= target) {
            self.held = Some(frame);
            return (None, None);
        }
        self.target = None;
        (self.held.take(), Some(frame))
    }

    /// The end of the stream: the held frame, if any.
    pub(crate) fn finish(&mut self) -> Option<RawVideoFrame> {
        self.target = None;
        self.held.take()
    }
}
