//! The parts of decoding (ADR 0047 sections 5, 6, 9 and 11): `Demux` reads
//! packets from a byte source, `VideoDecoder` decodes video packets into
//! frames and converts them to RGBA, `AudioDecoder` decodes audio packets
//! into 48 kHz stereo `f32` samples. Each part is `Send`, so the pipeline
//! can run them on different threads; `MediaFile` runs them in order on
//! one.

use std::io;

use swb_media_ffi::ffmpeg as ff;
use tiny_skia::Pixmap;

use crate::info::{
    AudioTrack, Codec, ColorTags, Conversion, Matrix, MatrixTag, MediaInfo, RangeTag, VideoTrack,
    conversion, natural_size,
};
use crate::{ByteSource, MediaError, OUTPUT_CHANNELS, OUTPUT_RATE, limits};

/// The result of a decoder's `receive`.
#[derive(Debug)]
pub enum Output<T> {
    /// A decoded frame or chunk.
    Ready(T),
    /// The decoder needs more packets.
    NeedsInput,
    /// The decoder is drained (after `finish`).
    Ended,
}

/// A packet of compressed data of one stream. It can move to another
/// thread and outlive its demuxer.
pub struct Packet(ff::Packet);

impl Packet {
    /// The stream index.
    pub fn stream(&self) -> usize {
        self.0.info().stream
    }

    /// The presentation time in microseconds, when known.
    pub fn pts_us(&self) -> Option<i64> {
        self.0.info().pts_us
    }

    /// The duration in microseconds, when known.
    pub fn duration_us(&self) -> Option<i64> {
        self.0.info().duration_us
    }

    /// Whether the packet starts a key frame.
    pub fn is_key(&self) -> bool {
        self.0.info().key
    }

    /// The size of the compressed data in bytes.
    pub fn size(&self) -> usize {
        self.0.info().size
    }
}

/// A `ByteSource` as FFmpeg's input.
struct Input(Box<dyn ByteSource>);

impl ff::Input for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.0.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.0.size()
    }
}

fn matrix_tag(m: ff::MatrixTag) -> MatrixTag {
    match m {
        ff::MatrixTag::Unspecified => MatrixTag::Unspecified,
        ff::MatrixTag::Bt709 => MatrixTag::Bt709,
        ff::MatrixTag::Bt601 => MatrixTag::Bt601,
        ff::MatrixTag::Rgb => MatrixTag::Rgb,
        ff::MatrixTag::Other => MatrixTag::Other,
    }
}

fn range_tag(r: ff::RangeTag) -> RangeTag {
    match r {
        ff::RangeTag::Unspecified => RangeTag::Unspecified,
        ff::RangeTag::Limited => RangeTag::Limited,
        ff::RangeTag::Full => RangeTag::Full,
    }
}

fn decode_error(e: &ff::Error) -> MediaError {
    if e.source_failed {
        MediaError::Source(e.message.clone())
    } else {
        MediaError::Decode(e.to_string())
    }
}

/// Whether a video size is within the limits of ADR 0047 section 11.
fn size_allowed(width: u32, height: u32) -> bool {
    width <= limits::MAX_DIMENSION
        && height <= limits::MAX_DIMENSION
        && u64::from(width) * u64::from(height) <= limits::MAX_PIXELS
}

fn video_track(index: usize, s: &ff::StreamInfo) -> VideoTrack {
    VideoTrack {
        stream: index,
        codec: Codec::from_name(&s.codec),
        profile: s.profile_name.clone(),
        coded_size: (s.width, s.height),
        natural_size: natural_size((s.width, s.height), s.sample_aspect),
        colors: ColorTags {
            matrix: matrix_tag(s.matrix),
            range: range_tag(s.range),
        },
        pixel_format: s.pixel_format.clone(),
        frame_rate: s.frame_rate,
        start_us: s.start_us,
        duration_us: s.duration_us,
    }
}

fn audio_track(index: usize, s: &ff::StreamInfo) -> AudioTrack {
    AudioTrack {
        stream: index,
        codec: Codec::from_name(&s.codec),
        profile: s.profile_name.clone(),
        sample_rate: s.sample_rate,
        channels: s.channels,
        start_us: s.start_us,
        duration_us: s.duration_us,
    }
}

/// Checks the selected tracks against the limits of ADR 0047 section 11.
fn check_limits(info: &MediaInfo) -> Result<(), MediaError> {
    if let Some(v) = &info.video {
        let (w, h) = v.coded_size;
        if !size_allowed(w, h) {
            return Err(MediaError::Unsupported(format!(
                "the video is {w} x {h} pixels, over the limit"
            )));
        }
    }
    if let Some(a) = &info.audio {
        if a.channels > limits::MAX_CHANNELS {
            return Err(MediaError::Unsupported(format!(
                "the audio has {} channels, over the limit",
                a.channels
            )));
        }
        let rates = limits::MIN_SAMPLE_RATE..=limits::MAX_SAMPLE_RATE;
        if !rates.contains(&a.sample_rate) {
            return Err(MediaError::Unsupported(format!(
                "the audio sample rate {} Hz is outside the limits",
                a.sample_rate
            )));
        }
    }
    Ok(())
}

/// The demuxer of a media file: reads the packets of the selected tracks
/// from a byte source. It can move to the pipeline's demux thread.
pub struct Demux {
    demuxer: ff::Demuxer,
    info: MediaInfo,
    error_at_end: Option<String>,
}

impl Demux {
    /// Opens the media file that `source` holds, reads its metadata and
    /// selects its first decodable video and audio streams. Fails with
    /// `NotMedia` when no demuxer opens the bytes, `Unsupported` when no
    /// stream can play or a track is over a limit, and `Source` when the
    /// byte source failed.
    pub fn open(source: Box<dyn ByteSource>) -> Result<Demux, MediaError> {
        ff::init(limits::MAX_ALLOC);
        let demux_limits = ff::DemuxerLimits {
            max_streams: limits::MAX_STREAMS,
            probe_bytes: limits::PROBE_BYTES,
            max_analyze_us: limits::MAX_ANALYZE_US,
        };
        let mut demuxer =
            ff::Demuxer::open(Box::new(Input(source)), &demux_limits).map_err(|e| {
                if e.source_failed {
                    MediaError::Source(e.message)
                } else {
                    MediaError::NotMedia(e.message)
                }
            })?;
        let streams = demuxer.streams();
        let first =
            |kind: ff::StreamKind| streams.iter().position(|s| s.kind == kind && s.has_decoder);
        let video = first(ff::StreamKind::Video);
        let audio = first(ff::StreamKind::Audio);
        let format = demuxer.format();
        let info = MediaInfo {
            container: format.format_name.clone(),
            duration_us: format.duration_us,
            start_us: format.start_us,
            seekable: format.seekable,
            stream_count: streams.len(),
            video: video.and_then(|i| Some(video_track(i, streams.get(i)?))),
            audio: audio.and_then(|i| Some(audio_track(i, streams.get(i)?))),
        };
        if info.video.is_none() && info.audio.is_none() {
            return Err(MediaError::Unsupported(
                "no audio or video stream that this build can decode".to_owned(),
            ));
        }
        check_limits(&info)?;
        demuxer
            .select(video, audio)
            .map_err(|e| MediaError::NotMedia(e.to_string()))?;
        Ok(Demux {
            demuxer,
            info,
            error_at_end: None,
        })
    }

    /// The metadata, read at the open.
    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    /// Reads only the packets of the tracks that are on; the demuxer
    /// discards the others.
    pub fn select(&mut self, video: bool, audio: bool) -> Result<(), MediaError> {
        let video = self.info.video.as_ref().filter(|_| video).map(|t| t.stream);
        let audio = self.info.audio.as_ref().filter(|_| audio).map(|t| t.stream);
        self.demuxer
            .select(video, audio)
            .map_err(|e| MediaError::Decode(e.to_string()))
    }

    /// The next packet of a selected track, or `None` at the end. An error
    /// of the demuxer after the open ends the stream (ADR 0047 section 11:
    /// a truncated or corrupt WebM plays to its end); `error_at_end` keeps
    /// its message. A failure of the byte source is an error.
    pub fn read(&mut self) -> Result<Option<Packet>, MediaError> {
        match self.demuxer.read() {
            // A read of the source failed inside the call: the packet (or the
            // end) may be cut short; the failure of the source wins.
            Ok(p) => match self.demuxer.take_source_error() {
                Some(e) => Err(MediaError::Source(e.to_string())),
                None => Ok(p.map(Packet)),
            },
            Err(e) if e.source_failed => Err(MediaError::Source(e.message)),
            Err(e) => {
                log::debug!("media: the demuxer stopped: {e}");
                self.error_at_end = Some(e.to_string());
                Ok(None)
            }
        }
    }

    /// The error of the demuxer that ended the stream, if one did.
    pub fn error_at_end(&self) -> Option<&str> {
        self.error_at_end.as_deref()
    }

    /// Seeks so that the next packet of `stream` is its key frame at or
    /// before `target_us`.
    pub fn seek(&mut self, stream: usize, target_us: i64) -> Result<(), MediaError> {
        self.error_at_end = None;
        self.demuxer.seek(stream, target_us).map_err(|e| {
            if e.source_failed {
                MediaError::Source(e.message)
            } else {
                MediaError::Seek(e.to_string())
            }
        })
    }

    /// A decoder of the video track (`None` without one), with the threads
    /// of ADR 0047 section 6.
    pub fn video_decoder(&mut self) -> Result<Option<VideoDecoder>, MediaError> {
        let Some(track) = self.info.video.clone() else {
            return Ok(None);
        };
        let (w, h) = track.coded_size;
        let pixels = u64::from(w) * u64::from(h);
        let threads = if pixels < limits::ONE_THREAD_BELOW_PIXELS {
            1
        } else {
            let most = if pixels > limits::FEW_THREADS_ABOVE_PIXELS {
                limits::FEW_DECODE_THREADS
            } else {
                limits::MAX_DECODE_THREADS
            };
            std::thread::available_parallelism()
                .map_or(1, |n| u32::try_from(n.get()).unwrap_or(1))
                .min(most)
        };
        let decoder = self
            .demuxer
            .decoder(track.stream, threads, limits::MAX_PIXELS)
            .map_err(|e| MediaError::Unsupported(format!("video decoder: {e}")))?;
        let scaler = ff::Scaler::new().map_err(|e| MediaError::Decode(e.to_string()))?;
        Ok(Some(VideoDecoder {
            decoder,
            scaler,
            stream: track.stream,
            colors: track.colors,
        }))
    }

    /// A decoder of the audio track (`None` without one), with output at
    /// 48 kHz stereo.
    pub fn audio_decoder(&mut self) -> Result<Option<AudioDecoder>, MediaError> {
        let Some(track) = &self.info.audio else {
            return Ok(None);
        };
        let stream = track.stream;
        let decoder = self
            .demuxer
            .decoder(stream, 1, limits::MAX_PIXELS)
            .map_err(|e| MediaError::Unsupported(format!("audio decoder: {e}")))?;
        let resampler = ff::Resampler::new(OUTPUT_RATE, OUTPUT_CHANNELS)
            .map_err(|e| MediaError::Decode(e.to_string()))?;
        Ok(Some(AudioDecoder {
            decoder,
            resampler,
            stream,
            anchor_us: None,
            position: 0,
            start_us: None,
            tail_done: false,
        }))
    }
}

/// Sends a packet to a decoder; `Ok(false)` when the decoder is full.
fn send(decoder: &mut ff::Decoder, packet: Option<&Packet>) -> Result<bool, MediaError> {
    match decoder.send(packet.map(|p| &p.0)) {
        Ok(ff::Sent::Full) => Ok(false),
        Ok(ff::Sent::Accepted | ff::Sent::Ended) => Ok(true),
        Err(e) => Err(decode_error(&e)),
    }
}

/// A decoded video frame before its conversion to RGBA.
pub struct RawVideoFrame {
    frame: ff::Frame,
    width: u32,
    height: u32,
    colors: ColorTags,
    pts_us: Option<i64>,
    duration_us: Option<i64>,
}

impl RawVideoFrame {
    /// The width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The presentation time in microseconds, when known.
    pub fn pts_us(&self) -> Option<i64> {
        self.pts_us
    }

    /// The duration in microseconds, when known.
    pub fn duration_us(&self) -> Option<i64> {
        self.duration_us
    }

    /// The frame's color tags (from the bitstream).
    pub fn colors(&self) -> ColorTags {
        self.colors
    }
}

/// The decoder of a video track. It can move to the pipeline's video
/// thread.
pub struct VideoDecoder {
    decoder: ff::Decoder,
    scaler: ff::Scaler,
    stream: usize,
    colors: ColorTags,
}

impl VideoDecoder {
    /// The stream index of the track.
    pub fn stream(&self) -> usize {
        self.stream
    }

    /// Sends a packet of the track; `Ok(false)` when the decoder is full
    /// (receive frames, then send the packet again).
    pub fn send(&mut self, packet: &Packet) -> Result<bool, MediaError> {
        send(&mut self.decoder, Some(packet))
    }

    /// Starts the drain at the end of the stream: `receive` then returns the
    /// buffered frames and `Ended`.
    pub fn finish(&mut self) -> Result<(), MediaError> {
        send(&mut self.decoder, None).map(|_| ())
    }

    /// Drops the decoder's state and buffered frames (after a seek).
    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    /// The next decoded frame. A frame over the size limits is a decode
    /// error (the stream's header can differ from its frames).
    pub fn receive(&mut self) -> Result<Output<RawVideoFrame>, MediaError> {
        let frame = match self.decoder.receive() {
            Ok(ff::Received::Frame(f)) => f,
            Ok(ff::Received::NeedsInput) => return Ok(Output::NeedsInput),
            Ok(ff::Received::Ended) => return Ok(Output::Ended),
            Err(e) => return Err(decode_error(&e)),
        };
        let Some(info) = frame.video() else {
            return Err(MediaError::Decode(
                "the decoder gave no video frame".to_owned(),
            ));
        };
        if !size_allowed(info.width, info.height) {
            return Err(MediaError::Decode(format!(
                "a frame of {} x {} pixels is over the limit",
                info.width, info.height
            )));
        }
        let raw = RawVideoFrame {
            width: info.width,
            height: info.height,
            colors: ColorTags {
                matrix: matrix_tag(info.matrix),
                range: range_tag(info.range),
            },
            pts_us: info.pts_us,
            duration_us: info.duration_us,
            frame,
        };
        Ok(Output::Ready(raw))
    }

    /// The color conversion of a frame of this track (ADR 0047 section 9).
    pub fn conversion(&self, frame: &RawVideoFrame) -> Conversion {
        conversion(self.colors, frame.colors, frame.height)
    }

    /// Converts a frame to RGBA into `image`, which must have the frame's
    /// size. The pixels are opaque, so straight and premultiplied alpha are
    /// the same.
    pub fn convert(&mut self, frame: &RawVideoFrame, image: &mut Pixmap) -> Result<(), MediaError> {
        if (image.width(), image.height()) != (frame.width, frame.height) {
            return Err(MediaError::Decode(
                "the image does not have the frame's size".to_owned(),
            ));
        }
        let c = self.conversion(frame);
        let matrix = match c.matrix {
            Matrix::Bt601 => ff::YuvMatrix::Bt601,
            Matrix::Bt709 => ff::YuvMatrix::Bt709,
        };
        let stride = frame.width as usize * 4;
        self.scaler
            .rgba(
                &frame.frame,
                matrix,
                c.full_range,
                (frame.width, frame.height),
                image.data_mut(),
                stride,
            )
            .map_err(|e| MediaError::Decode(e.to_string()))
    }
}

/// Interleaved stereo samples at 48 kHz.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    /// The time of the first sample in microseconds.
    pub pts_us: i64,
    /// Interleaved samples, left then right.
    pub samples: Vec<f32>,
}

impl AudioChunk {
    /// Samples per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / OUTPUT_CHANNELS as usize
    }
}

/// The decoder of an audio track with its resampler to 48 kHz stereo `f32`.
/// It stays on the pipeline's demux thread.
pub struct AudioDecoder {
    decoder: ff::Decoder,
    resampler: ff::Resampler,
    stream: usize,
    /// The time of the first output sample since the last start or flush.
    anchor_us: Option<i64>,
    /// Output samples per channel since the anchor, dropped ones included.
    position: u64,
    /// Samples before this time are dropped (after a seek).
    start_us: Option<i64>,
    /// The resampler's last samples were taken after the drain.
    tail_done: bool,
}

/// Microseconds of `samples` at the output rate.
fn samples_to_us(samples: u64) -> i64 {
    i64::try_from(u128::from(samples) * 1_000_000 / u128::from(OUTPUT_RATE)).unwrap_or(i64::MAX)
}

/// Output samples in `us` microseconds (0 for a negative time).
fn us_to_samples(us: i64) -> u64 {
    let us = u128::try_from(us).unwrap_or(0);
    u64::try_from((us * u128::from(OUTPUT_RATE) + 500_000) / 1_000_000).unwrap_or(u64::MAX)
}

impl AudioDecoder {
    /// The stream index of the track.
    pub fn stream(&self) -> usize {
        self.stream
    }

    /// Sends a packet of the track; `Ok(false)` when the decoder is full.
    pub fn send(&mut self, packet: &Packet) -> Result<bool, MediaError> {
        send(&mut self.decoder, Some(packet))
    }

    /// Starts the drain at the end of the stream.
    pub fn finish(&mut self) -> Result<(), MediaError> {
        send(&mut self.decoder, None).map(|_| ())
    }

    /// Drops the decoder's state (after a seek); the next output starts at
    /// the time of the next decoded frame.
    pub fn flush(&mut self) {
        self.decoder.flush();
        self.anchor_us = None;
        self.position = 0;
        self.tail_done = false;
        // The resampler may hold a few samples of before the seek; they go
        // with a new resampler.
        if let Ok(r) = ff::Resampler::new(OUTPUT_RATE, OUTPUT_CHANNELS) {
            self.resampler = r;
        }
    }

    /// Drops the samples before `target_us`, also inside a frame (a seek).
    pub fn start_at(&mut self, target_us: i64) {
        self.start_us = Some(target_us);
    }

    /// The next chunk of samples.
    pub fn receive(&mut self) -> Result<Output<AudioChunk>, MediaError> {
        loop {
            let frame = match self.decoder.receive() {
                Ok(ff::Received::Frame(f)) => f,
                Ok(ff::Received::NeedsInput) => return Ok(Output::NeedsInput),
                Ok(ff::Received::Ended) => {
                    if !self.tail_done {
                        self.tail_done = true;
                        let mut samples = Vec::new();
                        let n = self
                            .resampler
                            .convert(None, &mut samples)
                            .map_err(|e| decode_error(&e))?;
                        if let Some(chunk) = self.take(samples, n) {
                            return Ok(Output::Ready(chunk));
                        }
                    }
                    return Ok(Output::Ended);
                }
                Err(e) => return Err(decode_error(&e)),
            };
            let Some(info) = frame.audio().copied() else {
                return Err(MediaError::Decode(
                    "the decoder gave no audio frame".to_owned(),
                ));
            };
            if info.channels > limits::MAX_CHANNELS {
                return Err(MediaError::Decode(format!(
                    "a frame with {} channels is over the limit",
                    info.channels
                )));
            }
            let rates = limits::MIN_SAMPLE_RATE..=limits::MAX_SAMPLE_RATE;
            if !rates.contains(&info.sample_rate) {
                return Err(MediaError::Decode(format!(
                    "a frame with the sample rate {} Hz is outside the limits",
                    info.sample_rate
                )));
            }
            if self.anchor_us.is_none() {
                self.anchor_us = Some(info.pts_us.unwrap_or(0));
                self.position = 0;
            }
            let mut samples = Vec::new();
            let n = self
                .resampler
                .convert(Some(&frame), &mut samples)
                .map_err(|e| decode_error(&e))?;
            if let Some(chunk) = self.take(samples, n) {
                return Ok(Output::Ready(chunk));
            }
        }
    }

    /// Makes a chunk of `n` samples per channel, without the samples before
    /// the start time; `None` when nothing is left.
    fn take(&mut self, mut samples: Vec<f32>, n: usize) -> Option<AudioChunk> {
        let anchor = self.anchor_us?;
        let first = self.position;
        self.position = self.position.saturating_add(n as u64);
        let mut skip = 0;
        if let Some(start) = self.start_us {
            let start_pos = us_to_samples(start.saturating_sub(anchor));
            if self.position <= start_pos {
                return None;
            }
            skip = start_pos.saturating_sub(first);
            self.start_us = None;
        }
        let skip_values = usize::try_from(skip)
            .unwrap_or(usize::MAX)
            .saturating_mul(OUTPUT_CHANNELS as usize)
            .min(samples.len());
        samples.drain(..skip_values);
        if samples.is_empty() {
            return None;
        }
        Some(AudioChunk {
            pts_us: anchor.saturating_add(samples_to_us(first + skip)),
            samples,
        })
    }
}
