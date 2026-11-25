//! The timeline of a media file as the media element shows it: the media
//! time at the start (`currentTime` after the load), the end (`duration`)
//! and the shift from the stream's times to media times, per container
//! (ADR 0047 section 7, "The origin").
//!
//! The rules come from Chromium's values in the probe rows
//! (`tools/probes/media-states.json`, case `format-metadata-table`; the
//! test data `tests/data/format-metadata.json`), compared with FFmpeg's
//! values of the same files:
//!
//! - The timeline is the stream's: an MP4 or WebM whose first sample is at
//!   1.5 s starts at that time, not at 0.
//! - MP3: FFmpeg's start time is the encoder delay that its demuxer skips
//!   (0.025057 s for the test files); Chromium starts at 0 and ends at
//!   FFmpeg's duration. So the stream's times are shifted by the start
//!   time.
//! - WAV and ADTS AAC have no start time in FFmpeg: the start is 0.
//! - Matroska and WebM: FFmpeg's duration is the end of the segment on the
//!   stream's timeline (3.5 s for the file that starts at 1.497 s), as
//!   Chromium's.
//! - MP4 with a start time above 0: FFmpeg's duration counts from the
//!   start; Chromium's `duration` (3.496145 s for the test file whose audio
//!   starts at 1.476009 s and whose AAC track lasts 2.023220 s, 87 frames
//!   of 1,024 samples and 136 samples) is the start plus the AAC track in
//!   whole frames of 1,024 samples, or the video track's duration when
//!   that is longer. One probe row fits this rule; it is swb's reading of
//!   that row, not a measured mechanism (roadmap backlog: more rows).
//!   An MP4 that starts at 0 ends at FFmpeg's duration (every other row).
//! - A negative start time (FFmpeg gives one for Ogg and Matroska files
//!   whose first packets lie before 0, Opus or Vorbis with a pre-skip, and
//!   for MP4 files whose edit list starts before the first sample): the
//!   start is 0, so `currentTime` does not start below 0 (swb's choice;
//!   no test file has a negative start, Chromium not measured); the media
//!   times stay the stream's, so the samples before 0 are not played, and
//!   the end stays where FFmpeg puts it.

use crate::{Codec, MediaInfo};

/// Where the media element's timeline starts and ends, and how the
/// stream's times map to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeline {
    /// Subtracted from the stream's times (packets, frames, samples) to
    /// give media times; added to a media time to seek the stream.
    pub offset_us: i64,
    /// The media time at the start of the presentation (`currentTime`
    /// after the load).
    pub start_us: i64,
    /// The media time at the end (`duration`), or `None` when the file
    /// does not tell (a stream: `duration` is `Infinity`).
    pub duration_us: Option<i64>,
}

/// The samples of one AAC frame.
const AAC_FRAME_SAMPLES: i64 = 1024;

/// The timeline of a file with the metadata `info`.
pub fn timeline(info: &MediaInfo) -> Timeline {
    let start = info.start_us.unwrap_or(0);
    let container = info.container.as_str();
    if container == "mp3" {
        // The start time is the encoder delay; the element starts at 0.
        return Timeline {
            offset_us: start,
            start_us: 0,
            duration_us: info.duration_us,
        };
    }
    if container.starts_with("matroska") {
        return Timeline {
            offset_us: 0,
            start_us: start.max(0),
            duration_us: info.duration_us,
        };
    }
    if container.starts_with("mov,mp4") && start > 0 {
        return Timeline {
            offset_us: 0,
            start_us: start,
            duration_us: mp4_duration_with_start(info, start),
        };
    }
    // MP4 from 0 (or before), Ogg, FLAC, WAV, ADTS: FFmpeg's duration
    // counts from the start.
    Timeline {
        offset_us: 0,
        start_us: start.max(0),
        duration_us: info.duration_us.map(|d| d.saturating_add(start)),
    }
}

/// The end of an MP4 whose presentation starts at `start` (see the module
/// comment).
fn mp4_duration_with_start(info: &MediaInfo, start: i64) -> Option<i64> {
    let video = info.video.as_ref().and_then(|v| v.duration_us);
    let audio = info.audio.as_ref().and_then(|a| {
        let d = a.duration_us?;
        if a.codec != Codec::Aac || a.sample_rate == 0 {
            return Some(d);
        }
        let rate = i64::from(a.sample_rate);
        let samples = i128::from(d) * i128::from(rate) / 1_000_000;
        let whole = samples / i128::from(AAC_FRAME_SAMPLES) * i128::from(AAC_FRAME_SAMPLES);
        i64::try_from(whole * 1_000_000 / i128::from(rate)).ok()
    });
    let track = match (video, audio) {
        (Some(v), Some(a)) => Some(v.max(a)),
        (v, a) => v.or(a),
    };
    track.or(info.duration_us).map(|d| start.saturating_add(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioTrack, ColorTags, VideoTrack};

    fn info(container: &str, start: Option<i64>, duration: Option<i64>) -> MediaInfo {
        MediaInfo {
            container: container.to_owned(),
            duration_us: duration,
            start_us: start,
            seekable: true,
            stream_count: 1,
            video: None,
            audio: None,
        }
    }

    fn video(duration: i64) -> VideoTrack {
        VideoTrack {
            stream: 0,
            codec: Codec::H264,
            profile: String::new(),
            coded_size: (320, 240),
            natural_size: (320, 240),
            colors: ColorTags::UNSPECIFIED,
            pixel_format: String::new(),
            frame_rate: None,
            start_us: None,
            duration_us: Some(duration),
        }
    }

    fn aac(rate: u32, duration: i64) -> AudioTrack {
        AudioTrack {
            stream: 1,
            codec: Codec::Aac,
            profile: String::new(),
            sample_rate: rate,
            channels: 2,
            start_us: None,
            duration_us: Some(duration),
        }
    }

    #[test]
    fn mp3_starts_at_zero_and_shifts_by_the_encoder_delay() {
        let t = timeline(&info("mp3", Some(25_057), Some(2_000_000)));
        assert_eq!(t.offset_us, 25_057);
        assert_eq!(t.start_us, 0);
        assert_eq!(t.duration_us, Some(2_000_000));
    }

    #[test]
    fn wav_and_adts_without_a_start_time_start_at_zero() {
        for c in ["wav", "aac"] {
            let t = timeline(&info(c, None, Some(2_027_269)));
            assert_eq!((t.offset_us, t.start_us), (0, 0));
            assert_eq!(t.duration_us, Some(2_027_269));
        }
    }

    #[test]
    fn webm_keeps_the_stream_timeline_and_the_segment_duration() {
        let t = timeline(&info("matroska,webm", Some(1_497_000), Some(3_500_000)));
        assert_eq!((t.offset_us, t.start_us), (0, 1_497_000));
        assert_eq!(t.duration_us, Some(3_500_000));
    }

    #[test]
    fn mp4_with_a_start_time_ends_after_the_whole_aac_frames() {
        let mut i = info("mov,mp4,m4a,3gp,3g2,mj2", Some(1_476_009), Some(2_023_991));
        i.video = Some(video(2_000_000));
        i.audio = Some(aac(44_100, 2_023_220));
        let t = timeline(&i);
        assert_eq!(t.start_us, 1_476_009);
        assert_eq!(t.duration_us, Some(3_496_145));
        // A longer video track wins.
        i.video = Some(video(2_100_000));
        assert_eq!(timeline(&i).duration_us, Some(3_576_009));
    }

    #[test]
    fn a_negative_start_starts_at_zero() {
        // Matroska: the duration is the end of the segment.
        let t = timeline(&info("matroska,webm", Some(-7_000), Some(2_008_000)));
        assert_eq!((t.offset_us, t.start_us), (0, 0));
        assert_eq!(t.duration_us, Some(2_008_000));
        // Ogg and MP4: FFmpeg's duration counts from the (negative) start.
        for c in ["ogg", "mov,mp4,m4a,3gp,3g2,mj2"] {
            let t = timeline(&info(c, Some(-6_500), Some(2_013_000)));
            assert_eq!((t.offset_us, t.start_us), (0, 0), "{c}");
            assert_eq!(t.duration_us, Some(2_006_500), "{c}");
        }
    }

    #[test]
    fn mp4_from_zero_and_other_containers_end_at_ffmpeg_duration() {
        let mut i = info("mov,mp4,m4a,3gp,3g2,mj2", Some(0), Some(2_000_000));
        i.audio = Some(aac(44_100, 2_000_000));
        assert_eq!(timeline(&i).duration_us, Some(2_000_000));
        let t = timeline(&info("ogg", Some(0), Some(2_006_500)));
        assert_eq!(t.duration_us, Some(2_006_500));
        assert_eq!(timeline(&info("flac", None, None)).duration_us, None);
    }
}
