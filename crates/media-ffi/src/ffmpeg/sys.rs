//! The declarations of `csrc/ffmpeg.h`: plain structs of integers, opaque
//! handles and the functions. Keep in step with the header; the C compiler
//! checks the C side, and the field order here must match it.

use std::ffi::{c_char, c_int, c_void};

/// An opaque C handle (`SwbDemuxer`, `SwbPacket`, ...). Never dereferenced
/// in Rust.
#[repr(C)]
pub(crate) struct Opaque {
    _private: [u8; 0],
}

pub(crate) const SWB_OK: c_int = 0;
pub(crate) const SWB_EOF: c_int = 1;
pub(crate) const SWB_AGAIN: c_int = 2;
pub(crate) const SWB_SEEK_SIZE: i64 = -1;
pub(crate) const SWB_NO_TS: i64 = i64::MIN;

pub(crate) const SWB_STR_VERSION: c_int = 0;
pub(crate) const SWB_STR_CONFIGURATION: c_int = 1;
pub(crate) const SWB_STR_LICENSE_AVCODEC: c_int = 2;
pub(crate) const SWB_STR_LICENSE_AVFORMAT: c_int = 3;
pub(crate) const SWB_STR_LICENSE_AVUTIL: c_int = 4;
pub(crate) const SWB_STR_LICENSE_SWSCALE: c_int = 5;
pub(crate) const SWB_STR_LICENSE_SWRESAMPLE: c_int = 6;

pub(crate) const SWB_MEDIA_VIDEO: c_int = 1;
pub(crate) const SWB_MEDIA_AUDIO: c_int = 2;

pub(crate) const SWB_MATRIX_BT709: c_int = 1;
pub(crate) const SWB_MATRIX_BT601: c_int = 2;
pub(crate) const SWB_MATRIX_RGB: c_int = 3;
pub(crate) const SWB_MATRIX_OTHER: c_int = 4;

pub(crate) const SWB_RANGE_LIMITED: c_int = 1;
pub(crate) const SWB_RANGE_FULL: c_int = 2;

pub(crate) type ReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
pub(crate) type SeekFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;
pub(crate) type LogFn = unsafe extern "C" fn(c_int, *const c_char);

#[repr(C)]
pub(crate) struct DemuxerLimits {
    pub(crate) max_streams: c_int,
    pub(crate) probe_bytes: i64,
    pub(crate) max_analyze_us: i64,
}

#[repr(C)]
pub(crate) struct FormatInfo {
    pub(crate) duration_us: i64,
    pub(crate) start_us: i64,
    pub(crate) stream_count: c_int,
    pub(crate) seekable: c_int,
    pub(crate) format_name: [c_char; 64],
}

impl Default for FormatInfo {
    fn default() -> FormatInfo {
        FormatInfo {
            duration_us: SWB_NO_TS,
            start_us: SWB_NO_TS,
            stream_count: 0,
            seekable: 0,
            format_name: [0; 64],
        }
    }
}

#[repr(C)]
pub(crate) struct StreamInfo {
    pub(crate) kind: c_int,
    pub(crate) has_decoder: c_int,
    pub(crate) codec_name: [c_char; 32],
    pub(crate) profile_name: [c_char; 32],
    pub(crate) profile: c_int,
    pub(crate) level: c_int,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) sar_num: c_int,
    pub(crate) sar_den: c_int,
    pub(crate) matrix: c_int,
    pub(crate) range: c_int,
    pub(crate) color_space_raw: c_int,
    pub(crate) pixel_format: [c_char; 32],
    pub(crate) frame_rate_num: c_int,
    pub(crate) frame_rate_den: c_int,
    pub(crate) sample_rate: c_int,
    pub(crate) channels: c_int,
    pub(crate) sample_format: [c_char; 16],
    pub(crate) start_us: i64,
    pub(crate) duration_us: i64,
}

impl Default for StreamInfo {
    fn default() -> StreamInfo {
        StreamInfo {
            kind: 0,
            has_decoder: 0,
            codec_name: [0; 32],
            profile_name: [0; 32],
            profile: -1,
            level: -1,
            width: 0,
            height: 0,
            sar_num: 0,
            sar_den: 1,
            matrix: 0,
            range: 0,
            color_space_raw: 0,
            pixel_format: [0; 32],
            frame_rate_num: 0,
            frame_rate_den: 1,
            sample_rate: 0,
            channels: 0,
            sample_format: [0; 16],
            start_us: SWB_NO_TS,
            duration_us: SWB_NO_TS,
        }
    }
}

#[repr(C)]
pub(crate) struct PacketInfo {
    pub(crate) stream: c_int,
    pub(crate) key: c_int,
    pub(crate) pts_us: i64,
    pub(crate) dts_us: i64,
    pub(crate) duration_us: i64,
    pub(crate) size: c_int,
}

impl Default for PacketInfo {
    fn default() -> PacketInfo {
        PacketInfo {
            stream: -1,
            key: 0,
            pts_us: SWB_NO_TS,
            dts_us: SWB_NO_TS,
            duration_us: SWB_NO_TS,
            size: 0,
        }
    }
}

#[repr(C)]
pub(crate) struct VideoFrameInfo {
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) matrix: c_int,
    pub(crate) range: c_int,
    pub(crate) has_alpha: c_int,
    pub(crate) pixel_format: [c_char; 32],
    pub(crate) pts_us: i64,
    pub(crate) duration_us: i64,
}

impl Default for VideoFrameInfo {
    fn default() -> VideoFrameInfo {
        VideoFrameInfo {
            width: 0,
            height: 0,
            matrix: 0,
            range: 0,
            has_alpha: 0,
            pixel_format: [0; 32],
            pts_us: SWB_NO_TS,
            duration_us: SWB_NO_TS,
        }
    }
}

#[repr(C)]
pub(crate) struct AudioFrameInfo {
    pub(crate) samples: c_int,
    pub(crate) sample_rate: c_int,
    pub(crate) channels: c_int,
    pub(crate) pts_us: i64,
}

impl Default for AudioFrameInfo {
    fn default() -> AudioFrameInfo {
        AudioFrameInfo {
            samples: 0,
            sample_rate: 0,
            channels: 0,
            pts_us: SWB_NO_TS,
        }
    }
}

unsafe extern "C" {
    pub(crate) fn swb_ffmpeg_init(log: Option<LogFn>, level: c_int, max_alloc: usize);
    pub(crate) fn swb_ffmpeg_string(which: c_int, buf: *mut c_char, len: usize) -> c_int;

    pub(crate) fn swb_demuxer_open(
        opaque: *mut c_void,
        read: ReadFn,
        seek: Option<SeekFn>,
        limits: *const DemuxerLimits,
        out: *mut *mut Opaque,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    pub(crate) fn swb_demuxer_info(d: *const Opaque, out: *mut FormatInfo) -> c_int;
    pub(crate) fn swb_demuxer_stream_info(
        d: *const Opaque,
        stream: c_int,
        out: *mut StreamInfo,
    ) -> c_int;
    pub(crate) fn swb_demuxer_select(d: *mut Opaque, video: c_int, audio: c_int) -> c_int;
    pub(crate) fn swb_demuxer_read(d: *mut Opaque, out: *mut *mut Opaque) -> c_int;
    pub(crate) fn swb_demuxer_seek(d: *mut Opaque, stream: c_int, target_us: i64) -> c_int;
    pub(crate) fn swb_demuxer_try_io_open(d: *const Opaque) -> c_int;
    pub(crate) fn swb_demuxer_close(d: *mut Opaque);

    pub(crate) fn swb_packet_info(p: *const Opaque, out: *mut PacketInfo) -> c_int;
    pub(crate) fn swb_packet_free(p: *mut Opaque);

    pub(crate) fn swb_decoder_open(
        d: *const Opaque,
        stream: c_int,
        threads: c_int,
        max_pixels: i64,
        out: *mut *mut Opaque,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    pub(crate) fn swb_decoder_send(dec: *mut Opaque, p: *const Opaque) -> c_int;
    pub(crate) fn swb_decoder_receive(dec: *mut Opaque, out: *mut *mut Opaque) -> c_int;
    pub(crate) fn swb_decoder_flush(dec: *mut Opaque);
    pub(crate) fn swb_decoder_close(dec: *mut Opaque);

    pub(crate) fn swb_frame_video_info(f: *const Opaque, out: *mut VideoFrameInfo) -> c_int;
    pub(crate) fn swb_frame_audio_info(f: *const Opaque, out: *mut AudioFrameInfo) -> c_int;
    pub(crate) fn swb_frame_free(f: *mut Opaque);

    pub(crate) fn swb_scaler_new() -> *mut Opaque;
    pub(crate) fn swb_scaler_rgba(
        s: *mut Opaque,
        f: *const Opaque,
        matrix: c_int,
        full_range: c_int,
        dst_w: c_int,
        dst_h: c_int,
        dst: *mut u8,
        dst_stride: c_int,
        dst_len: usize,
    ) -> c_int;
    pub(crate) fn swb_scaler_free(s: *mut Opaque);

    pub(crate) fn swb_resampler_new(out_rate: c_int, out_channels: c_int) -> *mut Opaque;
    pub(crate) fn swb_resampler_max_out(r: *mut Opaque, f: *const Opaque) -> c_int;
    pub(crate) fn swb_resampler_f32(
        r: *mut Opaque,
        f: *const Opaque,
        dst: *mut f32,
        dst_samples: c_int,
    ) -> c_int;
    pub(crate) fn swb_resampler_free(r: *mut Opaque);
}
