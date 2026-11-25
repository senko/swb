/* The narrow C API between swb and FFmpeg (ADR 0047 section 4).
 *
 * Rust sees only opaque handles, structs of plain integers and the
 * callbacks below. No FFmpeg struct or enum value crosses the boundary:
 * colors, kinds and errors are swb's own values, so a change of FFmpeg's
 * layouts needs no change in Rust. Times are in microseconds; SWB_NO_TS
 * means "unknown".
 *
 * Threads: each handle is used by one thread at a time. A packet can move
 * from the demuxer's thread to a decoder's thread. The log callback runs on
 * any thread, also FFmpeg's own decode threads.
 *
 * Every function checks the arguments that come from Rust (null handles,
 * stream indices, sizes, strides) and every value from FFmpeg before it
 * uses it.
 *
 * This file is swb's own (MIT). It includes FFmpeg's headers; a binary that
 * links FFmpeg has the obligations of ADR 0047 section 2.
 */
#ifndef SWB_FFMPEG_H
#define SWB_FFMPEG_H

#include <stddef.h>
#include <stdint.h>

/* Results besides FFmpeg's negative error codes. */
#define SWB_OK 0
#define SWB_EOF 1
#define SWB_AGAIN 2

/* The `offset` of a seek callback that asks for the size. */
#define SWB_SEEK_SIZE (-1)
/* An unknown time. */
#define SWB_NO_TS INT64_MIN

/* Read up to `size` bytes into `buf`. Returns the count (at most `size`), 0
 * at the end of the resource, or a negative value on error or
 * cancellation. */
typedef int (*swb_read_fn)(void *opaque, uint8_t *buf, int size);
/* Seek to the absolute byte offset `offset`, or, with `offset` ==
 * SWB_SEEK_SIZE, return the size of the resource. Returns the new offset
 * (or the size), or a negative value when it cannot. */
typedef int64_t (*swb_seek_fn)(void *opaque, int64_t offset);
/* Log sink: one formatted line of FFmpeg's log (AV_LOG_* level). */
typedef void (*swb_log_fn)(int level, const char *line);

typedef struct SwbDemuxer SwbDemuxer;
typedef struct SwbPacket SwbPacket;
typedef struct SwbDecoder SwbDecoder;
typedef struct SwbFrame SwbFrame;
typedef struct SwbScaler SwbScaler;
typedef struct SwbResampler SwbResampler;

/* Strings of the library (swb_ffmpeg_string). */
enum {
    SWB_STR_VERSION = 0,       /* av_version_info() */
    SWB_STR_CONFIGURATION = 1, /* avcodec_configuration() */
    SWB_STR_LICENSE_AVCODEC = 2,
    SWB_STR_LICENSE_AVFORMAT = 3,
    SWB_STR_LICENSE_AVUTIL = 4,
    SWB_STR_LICENSE_SWSCALE = 5,
    SWB_STR_LICENSE_SWRESAMPLE = 6
};

enum { SWB_MEDIA_OTHER = 0, SWB_MEDIA_VIDEO = 1, SWB_MEDIA_AUDIO = 2 };

/* The YCbCr matrix of a tag (AVColorSpace mapped to swb's values). */
enum {
    SWB_MATRIX_UNSPECIFIED = 0,
    SWB_MATRIX_BT709 = 1,
    SWB_MATRIX_BT601 = 2, /* BT.470BG and SMPTE 170M */
    SWB_MATRIX_RGB = 3,
    SWB_MATRIX_OTHER = 4  /* BT.2020, SMPTE 240M, FCC, YCgCo, ... */
};

/* The range of a tag. */
enum { SWB_RANGE_UNSPECIFIED = 0, SWB_RANGE_LIMITED = 1, SWB_RANGE_FULL = 2 };

typedef struct {
    int max_streams;          /* refuse files with more streams */
    int64_t probe_bytes;      /* bytes read to find the streams */
    int64_t max_analyze_us;   /* media time read to find the streams */
} SwbDemuxerLimits;

typedef struct {
    int64_t duration_us;      /* SWB_NO_TS when unknown */
    int64_t start_us;         /* SWB_NO_TS when unknown */
    int stream_count;
    int seekable;             /* the byte source can seek */
    char format_name[64];
} SwbFormatInfo;

typedef struct {
    int kind;                 /* SWB_MEDIA_* */
    int has_decoder;          /* this build has a decoder for the codec */
    char codec_name[32];      /* avcodec_get_name() */
    char profile_name[32];    /* avcodec_profile_name(), or "" */
    int profile, level;       /* the codec's values, or < 0 when unknown */
    /* video */
    int width, height;
    int sar_num, sar_den;     /* sample aspect ratio, 0/1 when unknown */
    int matrix, range;        /* SWB_MATRIX_*, SWB_RANGE_* */
    int color_space_raw;      /* the AVColorSpace value, for logs */
    char pixel_format[32];    /* av_get_pix_fmt_name(), or "" */
    int frame_rate_num, frame_rate_den;
    /* audio */
    int sample_rate, channels;
    char sample_format[16];   /* av_get_sample_fmt_name(), or "" */
    int64_t start_us, duration_us;
} SwbStreamInfo;

typedef struct {
    int stream;
    int key;
    int64_t pts_us, dts_us, duration_us; /* SWB_NO_TS when unknown */
    int size;
} SwbPacketInfo;

typedef struct {
    int width, height;
    int matrix, range;        /* the frame's tags: SWB_MATRIX_*, SWB_RANGE_* */
    int has_alpha;
    char pixel_format[32];
    int64_t pts_us;           /* best effort timestamp, SWB_NO_TS when unknown */
    int64_t duration_us;
} SwbVideoFrameInfo;

typedef struct {
    int samples;              /* per channel */
    int sample_rate, channels;
    int64_t pts_us;
} SwbAudioFrameInfo;

/* Sets the log sink and level, and the largest single allocation
 * (av_max_alloc). Process-wide: call once, before any other function. */
void swb_ffmpeg_init(swb_log_fn log, int level, size_t max_alloc);
/* Copies a string of the library (SWB_STR_*) into `buf` with a NUL.
 * Returns its length (without the NUL; it may exceed `len - 1`, then the
 * copy is cut), or -1 for an unknown `which`. */
int swb_ffmpeg_string(int which, char *buf, size_t len);

/* Opens a demuxer that reads through `read` and `seek` (`seek` NULL: the
 * source cannot seek). FFmpeg never opens a file or a second resource. On
 * failure `err` holds a message. */
int swb_demuxer_open(void *opaque, swb_read_fn read, swb_seek_fn seek,
                     const SwbDemuxerLimits *limits, SwbDemuxer **out,
                     char *err, size_t err_len);
int swb_demuxer_info(const SwbDemuxer *d, SwbFormatInfo *out);
int swb_demuxer_stream_info(const SwbDemuxer *d, int stream, SwbStreamInfo *out);
/* Reads only the packets of `video` and `audio` (each a stream index or
 * -1); the demuxer discards the other streams. */
int swb_demuxer_select(SwbDemuxer *d, int video, int audio);
/* SWB_OK with *out set, SWB_EOF, or a negative error. */
int swb_demuxer_read(SwbDemuxer *d, SwbPacket **out);
/* Seeks so that the next packet of `stream` is the key frame at or before
 * `target_us`. */
int swb_demuxer_seek(SwbDemuxer *d, int stream, int64_t target_us);
/* For tests: asks the demuxer's context to open a second resource. Returns
 * a negative error when it refuses (it always should). */
int swb_demuxer_try_io_open(const SwbDemuxer *d);
void swb_demuxer_close(SwbDemuxer *d);

int swb_packet_info(const SwbPacket *p, SwbPacketInfo *out);
void swb_packet_free(SwbPacket *p);

/* A decoder for `stream`, with `threads` threads (frame and slice threads;
 * 0 is FFmpeg's choice) and at most `max_pixels` pixels per frame. */
int swb_decoder_open(const SwbDemuxer *d, int stream, int threads,
                     int64_t max_pixels, SwbDecoder **out,
                     char *err, size_t err_len);
/* `p` NULL drains. Does not take ownership of `p`. SWB_OK, SWB_AGAIN
 * (receive frames first, then send `p` again), SWB_EOF (drained), or an
 * error. */
int swb_decoder_send(SwbDecoder *dec, const SwbPacket *p);
/* SWB_OK with *out set, SWB_AGAIN (send more), SWB_EOF, or an error. */
int swb_decoder_receive(SwbDecoder *dec, SwbFrame **out);
void swb_decoder_flush(SwbDecoder *dec);
void swb_decoder_close(SwbDecoder *dec);

int swb_frame_video_info(const SwbFrame *f, SwbVideoFrameInfo *out);
int swb_frame_audio_info(const SwbFrame *f, SwbAudioFrameInfo *out);
void swb_frame_free(SwbFrame *f);

SwbScaler *swb_scaler_new(void);
/* Converts (and scales) a video frame to RGBA with alpha 255 into `dst`
 * (`dst_h` rows of `dst_stride` bytes, `dst_len` bytes in all). `matrix` is
 * SWB_MATRIX_BT601 or SWB_MATRIX_BT709, `full_range` 0 or 1. For limited
 * range the Cb-to-B coefficient is capped at 2.0 (ADR 0047 section 9).
 * Bilinear, without accurate rounding or full chroma interpolation. */
int swb_scaler_rgba(SwbScaler *s, const SwbFrame *f, int matrix, int full_range,
                    int dst_w, int dst_h, uint8_t *dst, int dst_stride,
                    size_t dst_len);
void swb_scaler_free(SwbScaler *s);

/* A resampler to interleaved f32 at `out_rate` with `out_channels`. */
SwbResampler *swb_resampler_new(int out_rate, int out_channels);
/* Prepares for `f` (or for a flush when `f` is NULL) and returns the most
 * samples per channel that swb_resampler_f32 can write for it, or an
 * error. */
int swb_resampler_max_out(SwbResampler *r, const SwbFrame *f);
/* Converts `f` (NULL: the samples that the resampler still holds) into
 * `dst` (`dst_samples` samples per channel of room). Returns the samples
 * per channel written, or an error. */
int swb_resampler_f32(SwbResampler *r, const SwbFrame *f, float *dst, int dst_samples);
void swb_resampler_free(SwbResampler *r);

#endif
