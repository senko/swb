//! What a media file holds: the container, the selected video and audio
//! tracks, their codecs, sizes and color tags, and the rule that picks the
//! color conversion of a frame (ADR 0047 section 9).

/// The metadata of an open media file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    /// FFmpeg's name of the container format (`mov,mp4,m4a,3gp,3g2,mj2`,
    /// `matroska,webm`, `ogg`, `mp3`, `flac`, `wav`, `aac`).
    pub container: String,
    /// The duration in microseconds, when known (FFmpeg's value; the
    /// element's rule per container is feature 5's).
    pub duration_us: Option<i64>,
    /// The start time of the presentation in microseconds, when known.
    pub start_us: Option<i64>,
    /// Whether the byte source can seek.
    pub seekable: bool,
    /// The number of streams in the container.
    pub stream_count: usize,
    /// The selected video track: the first video stream that this build
    /// can decode.
    pub video: Option<VideoTrack>,
    /// The selected audio track: the first audio stream that this build
    /// can decode.
    pub audio: Option<AudioTrack>,
}

/// A codec, as far as swb's build knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    /// H.264 (AVC).
    H264,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// AAC.
    Aac,
    /// MP3 (MPEG-1/2 audio layer III).
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// FLAC.
    Flac,
    /// PCM in one of the WAV formats of the build (FFmpeg's name).
    Pcm(String),
    /// Another codec (FFmpeg's name).
    Other(String),
}

impl Codec {
    /// The codec of FFmpeg's codec name.
    pub fn from_name(name: &str) -> Codec {
        match name {
            "h264" => Codec::H264,
            "vp8" => Codec::Vp8,
            "vp9" => Codec::Vp9,
            "aac" => Codec::Aac,
            "mp3" => Codec::Mp3,
            "opus" => Codec::Opus,
            "vorbis" => Codec::Vorbis,
            "flac" => Codec::Flac,
            n if n.starts_with("pcm_") => Codec::Pcm(n.to_owned()),
            n => Codec::Other(n.to_owned()),
        }
    }
}

/// The video track of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoTrack {
    /// The stream index in the container.
    pub stream: usize,
    /// The codec.
    pub codec: Codec,
    /// The codec profile's name (`High`, `Baseline`, ...), or empty.
    pub profile: String,
    /// The coded width and height in pixels.
    pub coded_size: (u32, u32),
    /// The natural size (`videoWidth`, `videoHeight`): the coded size with
    /// the sample aspect ratio applied (ADR 0047 section 9).
    pub natural_size: (u32, u32),
    /// The color tags of the stream (container or bitstream).
    pub colors: ColorTags,
    /// FFmpeg's pixel format name (`yuv420p`, ...), or empty.
    pub pixel_format: String,
    /// The average frame rate, when known.
    pub frame_rate: Option<(u32, u32)>,
    /// The stream's start time in microseconds, when known.
    pub start_us: Option<i64>,
    /// The stream's duration in microseconds, when known.
    pub duration_us: Option<i64>,
}

/// The audio track of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    /// The stream index in the container.
    pub stream: usize,
    /// The codec.
    pub codec: Codec,
    /// The codec profile's name (`LC`, ...), or empty.
    pub profile: String,
    /// Samples per second of the stream.
    pub sample_rate: u32,
    /// Channels of the stream.
    pub channels: u32,
    /// The stream's start time in microseconds, when known.
    pub start_us: Option<i64>,
    /// The stream's duration in microseconds, when known.
    pub duration_us: Option<i64>,
}

/// The YCbCr matrix of a color tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixTag {
    /// No tag.
    Unspecified,
    /// BT.709.
    Bt709,
    /// BT.601 (BT.470BG or SMPTE 170M).
    Bt601,
    /// RGB (no matrix).
    Rgb,
    /// Another matrix (BT.2020, SMPTE 240M, ...).
    Other,
}

/// The range of a color tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeTag {
    /// No tag.
    Unspecified,
    /// Limited range.
    Limited,
    /// Full range.
    Full,
}

/// The color tags of a stream or a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorTags {
    /// The matrix.
    pub matrix: MatrixTag,
    /// The range.
    pub range: RangeTag,
}

impl ColorTags {
    /// No tags.
    pub const UNSPECIFIED: ColorTags = ColorTags {
        matrix: MatrixTag::Unspecified,
        range: RangeTag::Unspecified,
    };
}

/// The matrix of a conversion to RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    /// BT.601.
    Bt601,
    /// BT.709.
    Bt709,
}

/// The conversion of a frame to RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conversion {
    /// The matrix.
    pub matrix: Matrix,
    /// Full range input (else limited).
    pub full_range: bool,
}

/// The first line count of an untagged stream that is converted as BT.709.
/// Measured in Chromium 148 (M14 feature 7a, `frames-color-untagged-bt709-*`
/// of `tools/probes/media-frames.json`, three equal runs): the height
/// decides, not the width and not the area. 718 lines (1280 x 718) are
/// BT.601, 720 lines (720 x 720, 960 x 720, 1280 x 720) are BT.709; 1280 x
/// 544 and 1280 x 480 (wide) are BT.601, as 1920 x 1080 is BT.709 (tall and
/// narrow, wide and low: only the height moves the result).
pub const UNTAGGED_BT709_LINES: u32 = 720;

/// The conversion of a frame (ADR 0047 section 9): the matrix of the
/// stream's tag, else the frame's tag, else BT.601 below 720 lines and
/// BT.709 from 720 (`height` is the height of the stream, the cropped one:
/// 1280 x 718 gives 718); the range of the stream,
/// else the frame's, else limited. The stream's tag (the container's or the
/// bitstream's, from the codec parameters) wins over the frame's: FFmpeg's
/// VP8 decoder tags every frame BT.601, and Chromium shows the Wikipedia
/// VP8 file, whose container says BT.709, as BT.709 (measured, feature 7a).
pub fn conversion(stream: ColorTags, frame: ColorTags, height: u32) -> Conversion {
    let tagged = |m: MatrixTag| match m {
        MatrixTag::Bt709 => Some(Matrix::Bt709),
        MatrixTag::Bt601 => Some(Matrix::Bt601),
        _ => None,
    };
    let matrix = tagged(stream.matrix)
        .or_else(|| tagged(frame.matrix))
        .unwrap_or(if height >= UNTAGGED_BT709_LINES {
            Matrix::Bt709
        } else {
            Matrix::Bt601
        });
    let range = match stream.range {
        RangeTag::Unspecified => frame.range,
        r => r,
    };
    Conversion {
        matrix,
        full_range: range == RangeTag::Full,
    }
}

/// The natural size of a video: the coded size with the sample aspect
/// ratio applied. A ratio above 1 widens the picture, a ratio below 1 makes
/// it taller; the size never shrinks. Measured in Chromium (feature 7a, the
/// files `mp4-h264-sar-1x2.mp4` and `-2x1.mp4`): `videoWidth` and
/// `videoHeight` follow this rule.
pub fn natural_size(coded: (u32, u32), sample_aspect: Option<(u32, u32)>) -> (u32, u32) {
    let (w, h) = coded;
    let Some((num, den)) = sample_aspect.filter(|(n, d)| *n > 0 && *d > 0 && n != d) else {
        return coded;
    };
    let scale = |v: u32, n: u32, d: u32| {
        let scaled = (u64::from(v) * u64::from(n) + u64::from(d) / 2) / u64::from(d);
        u32::try_from(scaled).unwrap_or(u32::MAX).max(1)
    };
    if num > den {
        (scale(w, num, den), h)
    } else {
        (w, scale(h, den, num))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn tags(matrix: MatrixTag, range: RangeTag) -> ColorTags {
        ColorTags { matrix, range }
    }

    #[test]
    fn the_stream_tag_wins_over_the_frame_tag() {
        let stream = tags(MatrixTag::Bt709, RangeTag::Limited);
        let frame = tags(MatrixTag::Bt601, RangeTag::Full);
        let c = conversion(stream, frame, 240);
        assert_eq!(c.matrix, Matrix::Bt709);
        assert!(!c.full_range);
    }

    #[test]
    fn the_frame_tag_applies_to_an_untagged_stream() {
        // FFmpeg's VP8 decoder tags its frames BT.601.
        let frame = tags(MatrixTag::Bt601, RangeTag::Unspecified);
        let c = conversion(ColorTags::UNSPECIFIED, frame, 1080);
        assert_eq!(c.matrix, Matrix::Bt601);
    }

    #[test]
    fn untagged_frames_use_the_line_count() {
        let none = ColorTags::UNSPECIFIED;
        assert_eq!(conversion(none, none, 240).matrix, Matrix::Bt601);
        assert_eq!(conversion(none, none, 576).matrix, Matrix::Bt601);
        assert_eq!(conversion(none, none, 720).matrix, Matrix::Bt709);
        let other = tags(MatrixTag::Other, RangeTag::Full);
        let c = conversion(other, none, 1080);
        assert_eq!(c.matrix, Matrix::Bt709);
        assert!(c.full_range);
    }

    /// The boundary is the height (Chromium, measured in feature 7a): the
    /// function takes no width, and 718 lines are still BT.601.
    #[test]
    fn the_untagged_boundary_is_the_height() {
        let none = ColorTags::UNSPECIFIED;
        for (height, matrix) in [
            (480, Matrix::Bt601),
            (544, Matrix::Bt601),
            (576, Matrix::Bt601),
            (600, Matrix::Bt601),
            (640, Matrix::Bt601),
            (704, Matrix::Bt601),
            (718, Matrix::Bt601),
            (720, Matrix::Bt709),
            (1080, Matrix::Bt709),
        ] {
            assert_eq!(conversion(none, none, height).matrix, matrix, "{height}");
        }
    }

    #[test]
    fn natural_size_applies_the_sample_aspect_ratio() {
        assert_eq!(natural_size((321, 241), None), (321, 241));
        assert_eq!(natural_size((321, 241), Some((1, 1))), (321, 241));
        assert_eq!(natural_size((720, 576), Some((16, 15))), (768, 576));
        assert_eq!(natural_size((720, 480), Some((8, 9))), (720, 540));
        assert_eq!(natural_size((1, 1), Some((u32::MAX, 1))), (u32::MAX, 1));
    }
}
