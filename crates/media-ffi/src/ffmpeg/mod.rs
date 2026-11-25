//! Safe Rust types over the C boundary of `csrc/ffmpeg.c` (ADR 0047
//! section 4).
//!
//! Every C object is owned by exactly one Rust value (a `NonNull` inside)
//! whose `Drop` frees it; no handle is `Clone`. Methods that use a handle
//! take `&mut self`. Types that FFmpeg lets one thread use at a time are
//! `Send`; none is `Sync`. No raw pointer and no handle is public. The
//! information of packets and frames is read once, when they are made.

mod convert;
mod io;
mod sys;

use std::ffi::{c_char, c_int};
use std::ptr::{self, NonNull};
use std::sync::Once;

pub use convert::{Resampler, Scaler, YuvMatrix};
pub use io::Input;

/// An error of an FFmpeg call: its negative error code and a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} ({code})")]
pub struct Error {
    /// FFmpeg's error code (an `AVERROR` value), or -1 for errors of the
    /// Rust side.
    pub code: i32,
    /// What failed, with FFmpeg's text for the code when there is one.
    pub message: String,
    /// The byte source failed during the call (a network error or a
    /// cancellation, not an error of the bytes).
    pub source_failed: bool,
}

impl Error {
    fn new(code: c_int, message: impl Into<String>) -> Error {
        Error {
            code,
            message: message.into(),
            source_failed: false,
        }
    }

    fn of_source(code: c_int, what: &str, error: &std::io::Error) -> Error {
        Error {
            code,
            message: format!("{what}: the byte source failed: {error}"),
            source_failed: true,
        }
    }

    /// An error of the Rust side of the boundary (a size that does not fit,
    /// a buffer that is too small).
    fn local(message: impl Into<String>) -> Error {
        Error::new(-1, message)
    }
}

/// FFmpeg's log levels (`AV_LOG_ERROR`, `AV_LOG_WARNING`).
const AV_LOG_ERROR: c_int = 16;
const AV_LOG_WARNING: c_int = 24;

unsafe extern "C" fn log_sink(level: c_int, line: *const c_char) {
    if line.is_null() {
        return;
    }
    // SAFETY: the C side passes a NUL-terminated line that lives for the
    // duration of the call.
    let line = unsafe { std::ffi::CStr::from_ptr(line) };
    let _ = std::panic::catch_unwind(|| {
        let text = line.to_string_lossy();
        let text = text.trim_end();
        // FFmpeg's messages are about the bytes of a page's media; the
        // pipeline reports the failure that results.
        if level <= AV_LOG_ERROR {
            log::debug!("ffmpeg: {text}");
        } else {
            log::trace!("ffmpeg: {text}");
        }
    });
}

/// Sets up FFmpeg for the process: its log goes to the `log` crate, and no
/// single allocation may exceed `max_alloc` bytes (ADR 0047 section 11).
/// Only the first call has an effect; every constructor of this module
/// calls it with the default.
pub fn init(max_alloc: usize) {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // SAFETY: sets process-wide values of FFmpeg before any other call
        // of this module (the `Once`); `log_sink` is valid for the life of
        // the process.
        unsafe { sys::swb_ffmpeg_init(Some(log_sink), AV_LOG_WARNING, max_alloc) };
    });
}

/// The largest single allocation of FFmpeg unless `init` set another
/// (ADR 0047 section 11: 512 MiB).
pub const DEFAULT_MAX_ALLOC: usize = 512 << 20;

fn ensure_init() {
    init(DEFAULT_MAX_ALLOC);
}

/// A library whose license string `license` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Library {
    /// libavcodec.
    AvCodec,
    /// libavformat.
    AvFormat,
    /// libavutil.
    AvUtil,
    /// libswscale.
    SwScale,
    /// libswresample.
    SwResample,
}

impl Library {
    /// All five libraries that swb links.
    pub const ALL: [Library; 5] = [
        Library::AvCodec,
        Library::AvFormat,
        Library::AvUtil,
        Library::SwScale,
        Library::SwResample,
    ];

    /// The library's name.
    pub fn name(self) -> &'static str {
        match self {
            Library::AvCodec => "libavcodec",
            Library::AvFormat => "libavformat",
            Library::AvUtil => "libavutil",
            Library::SwScale => "libswscale",
            Library::SwResample => "libswresample",
        }
    }
}

fn library_string(which: c_int) -> String {
    let mut buf = vec![0 as c_char; 1024];
    loop {
        // SAFETY: `buf` has `buf.len()` writable bytes; the C side writes at
        // most that many, with a NUL.
        let n = unsafe { sys::swb_ffmpeg_string(which, buf.as_mut_ptr(), buf.len()) };
        let Ok(n) = usize::try_from(n) else {
            return String::new();
        };
        if n < buf.len() || buf.len() >= 1 << 20 {
            return c_string(&buf);
        }
        buf = vec![0; n + 1];
    }
}

/// FFmpeg's version (`av_version_info()`), for example "9.0.2".
pub fn version() -> String {
    library_string(sys::SWB_STR_VERSION)
}

/// The configure line of the linked FFmpeg (`avcodec_configuration()`).
pub fn configuration() -> String {
    library_string(sys::SWB_STR_CONFIGURATION)
}

/// The license string of a library (`avcodec_license()` and the others).
pub fn license(library: Library) -> String {
    library_string(match library {
        Library::AvCodec => sys::SWB_STR_LICENSE_AVCODEC,
        Library::AvFormat => sys::SWB_STR_LICENSE_AVFORMAT,
        Library::AvUtil => sys::SWB_STR_LICENSE_AVUTIL,
        Library::SwScale => sys::SWB_STR_LICENSE_SWSCALE,
        Library::SwResample => sys::SWB_STR_LICENSE_SWRESAMPLE,
    })
}

/// The pinned version and configure options this crate was built against
/// (from `tools/ffmpeg/`, through the build script).
pub mod pinned {
    /// The pinned FFmpeg version.
    pub const VERSION: &str = env!("SWB_FFMPEG_VERSION");
    /// The SHA-256 of the pinned source tarball.
    pub const SHA256: &str = env!("SWB_FFMPEG_SHA256");
    /// The pinned configure options, without `--prefix`, joined by spaces.
    pub const CONFIGURE: &str = env!("SWB_FFMPEG_CONFIGURE");
}

/// Reads a NUL-terminated string from a fixed array that the C side
/// filled; stops at the end of the array if there is no NUL.
fn c_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn time(us: i64) -> Option<i64> {
    (us != sys::SWB_NO_TS).then_some(us)
}

fn stream_index(stream: usize) -> Result<c_int, Error> {
    c_int::try_from(stream).map_err(|_| Error::local("stream index out of range"))
}

/// The limits of a demuxer (ADR 0047 section 11).
#[derive(Debug, Clone, Copy)]
pub struct DemuxerLimits {
    /// A file with more streams fails to open.
    pub max_streams: u32,
    /// The bytes read to find the streams (`probesize`).
    pub probe_bytes: u64,
    /// The media time read to find the streams, in microseconds
    /// (`max_analyze_duration`).
    pub max_analyze_us: i64,
}

/// The container of an open demuxer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatInfo {
    /// The demuxer's name (`mov,mp4,m4a,3gp,3g2,mj2`, `matroska,webm`, ...).
    pub format_name: String,
    /// The duration, in microseconds, when known.
    pub duration_us: Option<i64>,
    /// The start time of the presentation, in microseconds, when known.
    pub start_us: Option<i64>,
    /// Whether the byte source can seek.
    pub seekable: bool,
}

/// What a stream holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// Video.
    Video,
    /// Audio.
    Audio,
    /// Anything else (subtitles, data, attachments).
    Other,
}

/// The YCbCr matrix of a color tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixTag {
    /// No tag (or a reserved value).
    Unspecified,
    /// BT.709.
    Bt709,
    /// BT.601 (BT.470BG or SMPTE 170M).
    Bt601,
    /// RGB (no matrix).
    Rgb,
    /// Another matrix (BT.2020, SMPTE 240M, FCC, `YCgCo`, ...).
    Other,
}

impl MatrixTag {
    fn from_c(v: c_int) -> MatrixTag {
        match v {
            sys::SWB_MATRIX_BT709 => MatrixTag::Bt709,
            sys::SWB_MATRIX_BT601 => MatrixTag::Bt601,
            sys::SWB_MATRIX_RGB => MatrixTag::Rgb,
            sys::SWB_MATRIX_OTHER => MatrixTag::Other,
            _ => MatrixTag::Unspecified,
        }
    }
}

/// The range of a color tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeTag {
    /// No tag.
    Unspecified,
    /// Limited ("TV", "MPEG") range.
    Limited,
    /// Full ("PC", "JPEG") range.
    Full,
}

impl RangeTag {
    fn from_c(v: c_int) -> RangeTag {
        match v {
            sys::SWB_RANGE_LIMITED => RangeTag::Limited,
            sys::SWB_RANGE_FULL => RangeTag::Full,
            _ => RangeTag::Unspecified,
        }
    }
}

/// One stream of a container, as the demuxer and the codec parameters
/// describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    /// Video, audio or other.
    pub kind: StreamKind,
    /// Whether this build has a decoder for the codec.
    pub has_decoder: bool,
    /// FFmpeg's codec name (`h264`, `vp8`, `aac`, `pcm_s16le`, ...).
    pub codec: String,
    /// The codec profile's name (`High`, `LC`, ...), or empty.
    pub profile_name: String,
    /// The codec profile, when known.
    pub profile: Option<i32>,
    /// The codec level, when known.
    pub level: Option<i32>,
    /// Video: the coded width and height.
    pub width: u32,
    /// See `width`.
    pub height: u32,
    /// Video: the sample aspect ratio, when known.
    pub sample_aspect: Option<(u32, u32)>,
    /// Video: the matrix tag of the stream (container or bitstream).
    pub matrix: MatrixTag,
    /// Video: the range tag of the stream.
    pub range: RangeTag,
    /// Video: FFmpeg's pixel format name (`yuv420p`, ...), or empty.
    pub pixel_format: String,
    /// Video: the average frame rate as a fraction, when known.
    pub frame_rate: Option<(u32, u32)>,
    /// Audio: samples per second.
    pub sample_rate: u32,
    /// Audio: the number of channels.
    pub channels: u32,
    /// Audio: FFmpeg's sample format name (`fltp`, `s16`, ...), or empty.
    pub sample_format: String,
    /// The stream's start time in microseconds, when known.
    pub start_us: Option<i64>,
    /// The stream's duration in microseconds, when known.
    pub duration_us: Option<i64>,
}

impl StreamInfo {
    fn from_c(s: &sys::StreamInfo) -> StreamInfo {
        let positive = |v: c_int| u32::try_from(v).ok().filter(|v| *v > 0);
        let pair = |n: c_int, d: c_int| Some((positive(n)?, positive(d)?));
        StreamInfo {
            kind: match s.kind {
                sys::SWB_MEDIA_VIDEO => StreamKind::Video,
                sys::SWB_MEDIA_AUDIO => StreamKind::Audio,
                _ => StreamKind::Other,
            },
            has_decoder: s.has_decoder != 0,
            codec: c_string(&s.codec_name),
            profile_name: c_string(&s.profile_name),
            profile: (s.profile >= 0).then_some(s.profile),
            level: (s.level >= 0).then_some(s.level),
            width: positive(s.width).unwrap_or(0),
            height: positive(s.height).unwrap_or(0),
            sample_aspect: pair(s.sar_num, s.sar_den),
            matrix: MatrixTag::from_c(s.matrix),
            range: RangeTag::from_c(s.range),
            pixel_format: c_string(&s.pixel_format),
            frame_rate: pair(s.frame_rate_num, s.frame_rate_den),
            sample_rate: positive(s.sample_rate).unwrap_or(0),
            channels: positive(s.channels).unwrap_or(0),
            sample_format: c_string(&s.sample_format),
            start_us: time(s.start_us),
            duration_us: time(s.duration_us),
        }
    }
}

/// A demuxer that reads from an `Input` through read and seek callbacks.
/// FFmpeg never gets a file name and never opens a second resource.
pub struct Demuxer {
    raw: NonNull<sys::Opaque>,
    source: io::Source,
    format: FormatInfo,
    streams: Vec<StreamInfo>,
}

// SAFETY: the demuxer and its input are used by one thread at a time
// (`&mut self` for every call that reads); the input is `Send`; FFmpeg's
// demuxers keep no thread-local state.
unsafe impl Send for Demuxer {}

impl Demuxer {
    /// Opens the container that `input` holds and reads its stream
    /// information.
    pub fn open(input: Box<dyn Input>, limits: &DemuxerLimits) -> Result<Demuxer, Error> {
        ensure_init();
        let mut source = io::Source::new(input);
        let lim = sys::DemuxerLimits {
            max_streams: c_int::try_from(limits.max_streams).unwrap_or(c_int::MAX),
            probe_bytes: i64::try_from(limits.probe_bytes).unwrap_or(i64::MAX),
            max_analyze_us: limits.max_analyze_us,
        };
        let mut out = ptr::null_mut();
        let mut err = [0 as c_char; 256];
        let seek: Option<sys::SeekFn> = if source.seekable() {
            Some(io::seek_callback)
        } else {
            None
        };
        // SAFETY: `source.opaque()` stays valid until `source` is dropped,
        // which is after `swb_demuxer_close` (in `Drop`) or right here on
        // failure, after the C side closed everything; the callbacks match
        // the C signatures; `err` has `err.len()` bytes.
        let r = unsafe {
            sys::swb_demuxer_open(
                source.opaque(),
                io::read_callback,
                seek,
                &raw const lim,
                &raw mut out,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        let raw = match NonNull::new(out) {
            Some(raw) if r == sys::SWB_OK => raw,
            _ => {
                return Err(match source.take_error() {
                    Some(e) => Error::of_source(r, "open", &e),
                    None => Error::new(r, c_string(&err)),
                });
            }
        };
        // An error that FFmpeg tolerated while probing (a failed seek at the
        // end, say) must not turn the first read into a source failure.
        let _ = source.take_error();
        let mut demuxer = Demuxer {
            raw,
            source,
            format: FormatInfo {
                format_name: String::new(),
                duration_us: None,
                start_us: None,
                seekable: false,
            },
            streams: Vec::new(),
        };
        demuxer.read_info();
        Ok(demuxer)
    }

    fn read_info(&mut self) {
        let mut info = sys::FormatInfo::default();
        // SAFETY: `raw` is a live demuxer; `info` is a valid out struct.
        let r = unsafe { sys::swb_demuxer_info(self.raw.as_ptr(), &raw mut info) };
        if r != sys::SWB_OK {
            return;
        }
        self.format = FormatInfo {
            format_name: c_string(&info.format_name),
            duration_us: time(info.duration_us),
            start_us: time(info.start_us),
            seekable: info.seekable != 0,
        };
        let count = usize::try_from(info.stream_count).unwrap_or(0);
        for index in 0..count {
            let mut s = sys::StreamInfo::default();
            let Ok(idx) = c_int::try_from(index) else {
                break;
            };
            // SAFETY: `raw` is a live demuxer; the C side checks the index;
            // `s` is a valid out struct.
            let r = unsafe { sys::swb_demuxer_stream_info(self.raw.as_ptr(), idx, &raw mut s) };
            if r != sys::SWB_OK {
                break;
            }
            self.streams.push(StreamInfo::from_c(&s));
        }
    }

    /// The container's information, read at the open.
    pub fn format(&self) -> &FormatInfo {
        &self.format
    }

    /// The streams, read at the open, by index.
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Reads only the packets of these streams; the demuxer discards the
    /// others.
    pub fn select(&mut self, video: Option<usize>, audio: Option<usize>) -> Result<(), Error> {
        let index = |s: Option<usize>| s.map_or(Ok(-1), stream_index);
        let (video, audio) = (index(video)?, index(audio)?);
        // SAFETY: `raw` is a live demuxer used by this thread only; the C
        // side ignores indices that are out of range.
        let r = unsafe { sys::swb_demuxer_select(self.raw.as_ptr(), video, audio) };
        if r == sys::SWB_OK {
            Ok(())
        } else {
            Err(Error::new(r, "select streams"))
        }
    }

    /// The next packet, or `None` at the end of the container.
    pub fn read(&mut self) -> Result<Option<Packet>, Error> {
        let mut out = ptr::null_mut();
        // SAFETY: `raw` is a live demuxer used by this thread only; the
        // callbacks it calls reach the input through `source`, which no
        // Rust code uses during the call (`&mut self`).
        let r = unsafe { sys::swb_demuxer_read(self.raw.as_ptr(), &raw mut out) };
        match (r, NonNull::new(out)) {
            (sys::SWB_OK, Some(raw)) => Ok(Some(Packet::new(raw))),
            (sys::SWB_EOF, _) => Ok(None),
            _ => Err(self.error(r, "read")),
        }
    }

    /// Seeks so that the next packet of `stream` is the key frame at or
    /// before `target_us`.
    pub fn seek(&mut self, stream: usize, target_us: i64) -> Result<(), Error> {
        let idx = stream_index(stream)?;
        // SAFETY: `raw` is a live demuxer used by this thread only; the C
        // side checks the index.
        let r = unsafe { sys::swb_demuxer_seek(self.raw.as_ptr(), idx, target_us) };
        if r < 0 {
            Err(self.error(r, "seek"))
        } else {
            Ok(())
        }
    }

    /// The error of the byte source since the last call, if it failed.
    /// FFmpeg sees a failed read as an I/O error; this tells a failure of
    /// the source (network, cancellation) from one of the bytes.
    pub fn take_source_error(&mut self) -> Option<std::io::Error> {
        self.source.take_error()
    }

    /// Whether the demuxer's context refuses to open a second resource
    /// (a file or URL that a container refers to) with swb's own refusal
    /// (`EPERM`), not because the build lacks the protocol. A self-check
    /// for tests: no enabled demuxer asks for one with swb's options (ADR
    /// 0047 section 11).
    pub fn refuses_second_resource(&self) -> bool {
        /// `EPERM` on Linux, the error that `io_open_refused` returns.
        const EPERM: c_int = 1;
        // SAFETY: `raw` is a live demuxer; the call opens nothing when it
        // refuses.
        unsafe { sys::swb_demuxer_try_io_open(self.raw.as_ptr()) == -EPERM }
    }

    fn error(&mut self, code: c_int, what: &str) -> Error {
        match self.source.take_error() {
            Some(e) => Error::of_source(code, what, &e),
            None => Error::new(code, what),
        }
    }

    /// A decoder for `stream`, with `threads` frame and slice threads (0 is
    /// FFmpeg's choice) and at most `max_pixels` pixels per frame. The
    /// decoder copies the stream's parameters and keeps no reference to
    /// the demuxer.
    pub fn decoder(
        &mut self,
        stream: usize,
        threads: u32,
        max_pixels: u64,
    ) -> Result<Decoder, Error> {
        let idx = stream_index(stream)?;
        let threads = c_int::try_from(threads).unwrap_or(c_int::MAX);
        let max_pixels = i64::try_from(max_pixels).unwrap_or(i64::MAX);
        let mut out = ptr::null_mut();
        let mut err = [0 as c_char; 256];
        // SAFETY: `raw` is a live demuxer; the C side checks the index;
        // `err` has `err.len()` bytes.
        let r = unsafe {
            sys::swb_decoder_open(
                self.raw.as_ptr(),
                idx,
                threads,
                max_pixels,
                &raw mut out,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        match NonNull::new(out) {
            Some(raw) if r == sys::SWB_OK => Ok(Decoder { raw }),
            _ => Err(Error::new(r, c_string(&err))),
        }
    }
}

impl Drop for Demuxer {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and is not used after this. The input that
        // the callbacks reach is dropped after this call (the fields are
        // dropped after `drop` returns).
        unsafe { sys::swb_demuxer_close(self.raw.as_ptr()) };
    }
}

/// The information of a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketInfo {
    /// The stream index.
    pub stream: usize,
    /// Whether the packet starts a key frame.
    pub key: bool,
    /// The presentation time, in microseconds.
    pub pts_us: Option<i64>,
    /// The decode time, in microseconds.
    pub dts_us: Option<i64>,
    /// The duration, in microseconds.
    pub duration_us: Option<i64>,
    /// The size of the data in bytes.
    pub size: usize,
}

/// A packet of compressed data of one stream. It holds reference-counted
/// data and can move to another thread and outlive its demuxer.
pub struct Packet {
    raw: NonNull<sys::Opaque>,
    info: PacketInfo,
}

// SAFETY: an `AVPacket` holds reference-counted data and no thread-bound
// state; the packet is used by one thread at a time.
unsafe impl Send for Packet {}

impl Packet {
    fn new(raw: NonNull<sys::Opaque>) -> Packet {
        let mut i = sys::PacketInfo::default();
        // SAFETY: `raw` is a live packet that this function now owns.
        let r = unsafe { sys::swb_packet_info(raw.as_ptr(), &raw mut i) };
        if r != sys::SWB_OK {
            i = sys::PacketInfo::default();
        }
        let info = PacketInfo {
            stream: usize::try_from(i.stream).unwrap_or(usize::MAX),
            key: i.key != 0,
            pts_us: time(i.pts_us),
            dts_us: time(i.dts_us),
            duration_us: time(i.duration_us),
            size: usize::try_from(i.size).unwrap_or(0),
        };
        Packet { raw, info }
    }

    /// The packet's information.
    pub fn info(&self) -> &PacketInfo {
        &self.info
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and owned by this value.
        unsafe { sys::swb_packet_free(self.raw.as_ptr()) };
    }
}

/// The result of `Decoder::send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// The decoder took the packet.
    Accepted,
    /// The decoder is full: receive frames, then send the packet again.
    Full,
    /// The decoder was already drained.
    Ended,
}

/// The result of `Decoder::receive`.
pub enum Received {
    /// A decoded frame.
    Frame(Frame),
    /// The decoder needs more packets.
    NeedsInput,
    /// The decoder is drained.
    Ended,
}

/// A video or audio decoder of one stream.
pub struct Decoder {
    raw: NonNull<sys::Opaque>,
}

// SAFETY: a codec context is used by one thread at a time (`&mut self`);
// FFmpeg's own decode threads belong to the context.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Sends a packet; `None` starts the drain at the end of the stream.
    pub fn send(&mut self, packet: Option<&Packet>) -> Result<Sent, Error> {
        let p = packet.map_or(ptr::null(), |p| p.raw.as_ptr().cast_const());
        // SAFETY: `raw` is live and used by this thread only; `p` is null or
        // a live packet, of which the decoder takes its own reference.
        let r = unsafe { sys::swb_decoder_send(self.raw.as_ptr(), p) };
        match r {
            sys::SWB_OK => Ok(Sent::Accepted),
            sys::SWB_AGAIN => Ok(Sent::Full),
            sys::SWB_EOF => Ok(Sent::Ended),
            _ => Err(Error::new(r, "decode")),
        }
    }

    /// Receives the next decoded frame.
    pub fn receive(&mut self) -> Result<Received, Error> {
        let mut out = ptr::null_mut();
        // SAFETY: `raw` is live and used by this thread only.
        let r = unsafe { sys::swb_decoder_receive(self.raw.as_ptr(), &raw mut out) };
        match (r, NonNull::new(out)) {
            (sys::SWB_OK, Some(raw)) => Ok(Received::Frame(Frame::new(raw))),
            (sys::SWB_AGAIN, _) => Ok(Received::NeedsInput),
            (sys::SWB_EOF, _) => Ok(Received::Ended),
            _ => Err(Error::new(r, "decode")),
        }
    }

    /// Drops the decoder's state and buffered frames (after a seek).
    pub fn flush(&mut self) {
        // SAFETY: `raw` is live and used by this thread only.
        unsafe { sys::swb_decoder_flush(self.raw.as_ptr()) };
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and owned by this value.
        unsafe { sys::swb_decoder_close(self.raw.as_ptr()) };
    }
}

/// The information of a decoded video frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrameInfo {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The frame's matrix tag (from the bitstream).
    pub matrix: MatrixTag,
    /// The frame's range tag.
    pub range: RangeTag,
    /// Whether the pixel format has alpha.
    pub has_alpha: bool,
    /// FFmpeg's pixel format name.
    pub pixel_format: String,
    /// The presentation time (best effort), in microseconds.
    pub pts_us: Option<i64>,
    /// The duration, in microseconds.
    pub duration_us: Option<i64>,
}

/// The information of a decoded audio frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFrameInfo {
    /// Samples per channel.
    pub samples: u32,
    /// Samples per second.
    pub sample_rate: u32,
    /// The number of channels.
    pub channels: u32,
    /// The presentation time (best effort), in microseconds.
    pub pts_us: Option<i64>,
}

/// A decoded frame. It holds reference-counted buffers and can move to
/// another thread and outlive its decoder.
pub struct Frame {
    raw: NonNull<sys::Opaque>,
    video: Option<VideoFrameInfo>,
    audio: Option<AudioFrameInfo>,
}

// SAFETY: a decoded `AVFrame` holds reference-counted buffers and no
// thread-bound state; the frame is used by one thread at a time.
unsafe impl Send for Frame {}

impl Frame {
    fn new(raw: NonNull<sys::Opaque>) -> Frame {
        let mut v = sys::VideoFrameInfo::default();
        // SAFETY: `raw` is a live frame that this function now owns.
        let rv = unsafe { sys::swb_frame_video_info(raw.as_ptr(), &raw mut v) };
        let mut a = sys::AudioFrameInfo::default();
        // SAFETY: as above.
        let ra = unsafe { sys::swb_frame_audio_info(raw.as_ptr(), &raw mut a) };
        let positive = |v: c_int| u32::try_from(v).ok().filter(|v| *v > 0);
        let video = (rv == sys::SWB_OK)
            .then(|| {
                Some(VideoFrameInfo {
                    width: positive(v.width)?,
                    height: positive(v.height)?,
                    matrix: MatrixTag::from_c(v.matrix),
                    range: RangeTag::from_c(v.range),
                    has_alpha: v.has_alpha != 0,
                    pixel_format: c_string(&v.pixel_format),
                    pts_us: time(v.pts_us),
                    duration_us: time(v.duration_us),
                })
            })
            .flatten();
        let audio = (ra == sys::SWB_OK)
            .then(|| {
                Some(AudioFrameInfo {
                    samples: positive(a.samples)?,
                    sample_rate: positive(a.sample_rate)?,
                    channels: positive(a.channels)?,
                    pts_us: time(a.pts_us),
                })
            })
            .flatten();
        Frame { raw, video, audio }
    }

    /// The information of a video frame (`None` for audio or an invalid
    /// frame).
    pub fn video(&self) -> Option<&VideoFrameInfo> {
        self.video.as_ref()
    }

    /// The information of an audio frame (`None` for video or an invalid
    /// frame).
    pub fn audio(&self) -> Option<&AudioFrameInfo> {
        self.audio.as_ref()
    }

    fn as_ptr(&self) -> *const sys::Opaque {
        self.raw.as_ptr().cast_const()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and owned by this value.
        unsafe { sys::swb_frame_free(self.raw.as_ptr()) };
    }
}
