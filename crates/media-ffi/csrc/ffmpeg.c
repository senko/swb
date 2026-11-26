/* The narrow C API between swb and FFmpeg (ADR 0047 section 4); see
 * ffmpeg.h. Uses only FFmpeg's public API (the headers of the build it
 * links). This file is swb's own (MIT). */
#include "ffmpeg.h"

#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
#include <libavutil/channel_layout.h>
#include <libavutil/common.h>
#include <libavutil/imgutils.h>
#include <libavutil/mem.h>
#include <libavutil/pixdesc.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>

/* The buffer of the custom I/O context (ADR 0047 section 5). */
#define IO_BUFFER_SIZE (64 * 1024)
/* The capped Cb-to-B coefficient in 16.16 (ADR 0047 section 9). */
#define CAP_CB_TO_B (2 << 16)

struct SwbDemuxer {
    AVFormatContext *fmt;
    AVIOContext *io;
    void *opaque;
    swb_read_fn read;
    swb_seek_fn seek;
    int64_t pos;
    int seekable;
};

struct SwbPacket {
    AVPacket *pkt;
    AVRational time_base;
};

struct SwbDecoder {
    AVCodecContext *ctx;
    AVRational time_base;
};

struct SwbFrame {
    AVFrame *frame;
    AVRational time_base;
};

struct SwbScaler {
    struct SwsContext *sws;
    /* The parameters that `sws` was set up with. The coefficients are set
     * again when any of them changes (sws_getCachedContext may rebuild the
     * context). */
    int src_w, src_h, src_format, dst_w, dst_h;
    int matrix, full_range;
    /* Buffers kept between frames, freed with the scaler: the source copy
     * padded to a multiple of 64 pixels (`ensure_padded`) and the RGBA
     * scratch (`scratch_rgba`). */
    AVFrame *pad;
    uint8_t *buf;
    size_t buf_size;
};

struct SwbResampler {
    SwrContext *swr;
    int out_rate, out_channels;
    int in_rate, in_format;
    AVChannelLayout in_layout;
};

static swb_log_fn log_sink;

static void log_callback(void *avcl, int level, const char *fmt, va_list vl) {
    if (level > av_log_get_level() || !log_sink)
        return;
    char line[1024];
    int print_prefix = 1;
    if (av_log_format_line2(avcl, level, fmt, vl, line, sizeof line, &print_prefix) < 0)
        return;
    log_sink(level, line);
}

void swb_ffmpeg_init(swb_log_fn log, int level, size_t max_alloc) {
    log_sink = log;
    av_log_set_level(level);
    av_log_set_callback(log_callback);
    if (max_alloc > 0)
        av_max_alloc(max_alloc);
}

int swb_ffmpeg_string(int which, char *buf, size_t len) {
    const char *s;
    switch (which) {
    case SWB_STR_VERSION: s = av_version_info(); break;
    case SWB_STR_CONFIGURATION: s = avcodec_configuration(); break;
    case SWB_STR_LICENSE_AVCODEC: s = avcodec_license(); break;
    case SWB_STR_LICENSE_AVFORMAT: s = avformat_license(); break;
    case SWB_STR_LICENSE_AVUTIL: s = avutil_license(); break;
    case SWB_STR_LICENSE_SWSCALE: s = swscale_license(); break;
    case SWB_STR_LICENSE_SWRESAMPLE: s = swresample_license(); break;
    default: return -1;
    }
    if (!s)
        s = "";
    if (buf && len > 0)
        snprintf(buf, len, "%s", s);
    size_t n = strlen(s);
    return n > INT_MAX ? INT_MAX : (int)n;
}

static int64_t to_us(int64_t ts, AVRational tb) {
    if (ts == AV_NOPTS_VALUE || tb.num <= 0 || tb.den <= 0)
        return SWB_NO_TS;
    return av_rescale_q(ts, tb, AV_TIME_BASE_Q);
}

static void set_error(char *err, size_t len, int code, const char *what) {
    if (!err || len == 0)
        return;
    char buf[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(code, buf, sizeof buf);
    snprintf(err, len, "%s: %s", what, buf);
}

static void copy_name(char *dst, size_t len, const char *src) {
    snprintf(dst, len, "%s", src ? src : "");
}

static int matrix_of(enum AVColorSpace cs) {
    switch (cs) {
    case AVCOL_SPC_UNSPECIFIED:
    case AVCOL_SPC_RESERVED:
        return SWB_MATRIX_UNSPECIFIED;
    case AVCOL_SPC_BT709: return SWB_MATRIX_BT709;
    case AVCOL_SPC_BT470BG:
    case AVCOL_SPC_SMPTE170M:
        return SWB_MATRIX_BT601;
    case AVCOL_SPC_RGB: return SWB_MATRIX_RGB;
    default: return SWB_MATRIX_OTHER;
    }
}

static int range_of(enum AVColorRange r) {
    switch (r) {
    case AVCOL_RANGE_MPEG: return SWB_RANGE_LIMITED;
    case AVCOL_RANGE_JPEG: return SWB_RANGE_FULL;
    default: return SWB_RANGE_UNSPECIFIED;
    }
}

static int clamp_int(int64_t v) {
    if (v < INT_MIN)
        return INT_MIN;
    if (v > INT_MAX)
        return INT_MAX;
    return (int)v;
}

/* ---- I/O callbacks ---- */

static int io_read(void *opaque, uint8_t *buf, int size) {
    SwbDemuxer *d = opaque;
    if (!d || !buf || size <= 0)
        return AVERROR(EINVAL);
    int n = d->read(d->opaque, buf, size);
    if (n == 0)
        return AVERROR_EOF;
    if (n < 0 || n > size)
        return AVERROR(EIO);
    d->pos += n;
    return n;
}

static int64_t io_seek(void *opaque, int64_t offset, int whence) {
    SwbDemuxer *d = opaque;
    if (!d || !d->seek)
        return AVERROR(ENOSYS);
    whence &= ~AVSEEK_FORCE;
    if (whence == AVSEEK_SIZE) {
        int64_t size = d->seek(d->opaque, SWB_SEEK_SIZE);
        return size < 0 ? AVERROR(ENOSYS) : size;
    }
    int64_t target;
    switch (whence) {
    case SEEK_SET: target = offset; break;
    case SEEK_CUR:
        if ((offset > 0 && d->pos > INT64_MAX - offset) ||
            (offset < 0 && d->pos < INT64_MIN - offset))
            return AVERROR(EINVAL);
        target = d->pos + offset;
        break;
    case SEEK_END: {
        int64_t size = d->seek(d->opaque, SWB_SEEK_SIZE);
        if (size < 0)
            return AVERROR(ENOSYS);
        if ((offset > 0 && size > INT64_MAX - offset))
            return AVERROR(EINVAL);
        target = size + offset;
        break;
    }
    default: return AVERROR(EINVAL);
    }
    if (target < 0)
        return AVERROR(EINVAL);
    int64_t r = d->seek(d->opaque, target);
    if (r < 0)
        return AVERROR(EIO);
    d->pos = r;
    return r;
}

/* FFmpeg never opens a second resource (MOV references, playlists). */
static int io_open_refused(struct AVFormatContext *s, AVIOContext **pb, const char *url,
                           int flags, AVDictionary **options) {
    (void)s; (void)pb; (void)url; (void)flags; (void)options;
    return AVERROR(EPERM);
}

/* ---- Demuxer ---- */

int swb_demuxer_open(void *opaque, swb_read_fn read, swb_seek_fn seek,
                     const SwbDemuxerLimits *limits, SwbDemuxer **out,
                     char *err, size_t err_len) {
    if (!out || !read)
        return AVERROR(EINVAL);
    *out = NULL;
    SwbDemuxer *d = av_mallocz(sizeof *d);
    if (!d)
        return AVERROR(ENOMEM);
    d->opaque = opaque;
    d->read = read;
    d->seek = seek;
    d->seekable = seek && seek(opaque, SWB_SEEK_SIZE) >= 0;
    if (!d->seekable)
        d->seek = NULL;
    uint8_t *buf = av_malloc(IO_BUFFER_SIZE);
    if (!buf) {
        av_free(d);
        return AVERROR(ENOMEM);
    }
    d->io = avio_alloc_context(buf, IO_BUFFER_SIZE, 0, d, io_read, NULL,
                               d->seekable ? io_seek : NULL);
    if (!d->io) {
        av_free(buf);
        av_free(d);
        return AVERROR(ENOMEM);
    }
    d->io->seekable = d->seekable ? AVIO_SEEKABLE_NORMAL : 0;
    d->fmt = avformat_alloc_context();
    if (!d->fmt) {
        swb_demuxer_close(d);
        return AVERROR(ENOMEM);
    }
    d->fmt->pb = d->io;
    d->fmt->flags |= AVFMT_FLAG_CUSTOM_IO;
    d->fmt->io_open = io_open_refused;
    if (limits) {
        if (limits->max_streams > 0)
            d->fmt->max_streams = limits->max_streams;
        if (limits->probe_bytes > 0) {
            d->fmt->probesize = limits->probe_bytes;
            d->fmt->format_probesize = clamp_int(limits->probe_bytes);
        }
        if (limits->max_analyze_us > 0)
            d->fmt->max_analyze_duration = limits->max_analyze_us;
    }
    int r = avformat_open_input(&d->fmt, NULL, NULL, NULL);
    if (r < 0) {
        /* avformat_open_input frees the context on failure, not the
         * custom I/O context. */
        d->fmt = NULL;
        set_error(err, err_len, r, "open");
        swb_demuxer_close(d);
        return r;
    }
    r = avformat_find_stream_info(d->fmt, NULL);
    if (r < 0) {
        set_error(err, err_len, r, "stream info");
        swb_demuxer_close(d);
        return r;
    }
    *out = d;
    return SWB_OK;
}

int swb_demuxer_info(const SwbDemuxer *d, SwbFormatInfo *out) {
    if (!d || !d->fmt || !out)
        return AVERROR(EINVAL);
    memset(out, 0, sizeof *out);
    const AVFormatContext *fmt = d->fmt;
    out->duration_us = fmt->duration == AV_NOPTS_VALUE ? SWB_NO_TS : fmt->duration;
    out->start_us = fmt->start_time == AV_NOPTS_VALUE ? SWB_NO_TS : fmt->start_time;
    out->stream_count = fmt->nb_streams > INT_MAX ? INT_MAX : (int)fmt->nb_streams;
    out->seekable = d->seekable;
    copy_name(out->format_name, sizeof out->format_name,
              fmt->iformat ? fmt->iformat->name : NULL);
    return SWB_OK;
}

static const AVStream *stream_at(const SwbDemuxer *d, int stream) {
    if (!d || !d->fmt || stream < 0 || (unsigned)stream >= d->fmt->nb_streams)
        return NULL;
    return d->fmt->streams[stream];
}

int swb_demuxer_stream_info(const SwbDemuxer *d, int stream, SwbStreamInfo *out) {
    if (!out)
        return AVERROR(EINVAL);
    memset(out, 0, sizeof *out);
    const AVStream *st = stream_at(d, stream);
    if (!st || !st->codecpar)
        return AVERROR(EINVAL);
    const AVCodecParameters *par = st->codecpar;
    out->kind = par->codec_type == AVMEDIA_TYPE_VIDEO ? SWB_MEDIA_VIDEO
              : par->codec_type == AVMEDIA_TYPE_AUDIO ? SWB_MEDIA_AUDIO
              : SWB_MEDIA_OTHER;
    out->has_decoder = avcodec_find_decoder(par->codec_id) != NULL;
    copy_name(out->codec_name, sizeof out->codec_name, avcodec_get_name(par->codec_id));
    copy_name(out->profile_name, sizeof out->profile_name,
              avcodec_profile_name(par->codec_id, par->profile));
    out->profile = par->profile;
    out->level = par->level;
    out->width = par->width;
    out->height = par->height;
    /* The demuxer's guess from the stream and the codec parameters. */
    AVRational sar = av_guess_sample_aspect_ratio(d->fmt, (AVStream *)st, NULL);
    if (sar.num > 0 && sar.den > 0) {
        out->sar_num = sar.num;
        out->sar_den = sar.den;
    } else {
        out->sar_num = 0;
        out->sar_den = 1;
    }
    out->matrix = matrix_of(par->color_space);
    out->range = range_of(par->color_range);
    out->color_space_raw = par->color_space;
    if (par->codec_type == AVMEDIA_TYPE_VIDEO)
        copy_name(out->pixel_format, sizeof out->pixel_format,
                  av_get_pix_fmt_name((enum AVPixelFormat)par->format));
    out->frame_rate_num = st->avg_frame_rate.num;
    out->frame_rate_den = st->avg_frame_rate.den;
    out->sample_rate = par->sample_rate;
    out->channels = par->ch_layout.nb_channels;
    if (par->codec_type == AVMEDIA_TYPE_AUDIO)
        copy_name(out->sample_format, sizeof out->sample_format,
                  av_get_sample_fmt_name((enum AVSampleFormat)par->format));
    out->start_us = to_us(st->start_time, st->time_base);
    out->duration_us = to_us(st->duration, st->time_base);
    return SWB_OK;
}

int swb_demuxer_select(SwbDemuxer *d, int video, int audio) {
    if (!d || !d->fmt)
        return AVERROR(EINVAL);
    for (unsigned i = 0; i < d->fmt->nb_streams; i++) {
        int keep = (int)i == video || (int)i == audio;
        d->fmt->streams[i]->discard = keep ? AVDISCARD_DEFAULT : AVDISCARD_ALL;
    }
    return SWB_OK;
}

int swb_demuxer_read(SwbDemuxer *d, SwbPacket **out) {
    if (!d || !d->fmt || !out)
        return AVERROR(EINVAL);
    *out = NULL;
    AVPacket *pkt = av_packet_alloc();
    if (!pkt)
        return AVERROR(ENOMEM);
    int r = av_read_frame(d->fmt, pkt);
    if (r == AVERROR_EOF) {
        av_packet_free(&pkt);
        return SWB_EOF;
    }
    if (r < 0) {
        av_packet_free(&pkt);
        return r;
    }
    if (pkt->stream_index < 0 || (unsigned)pkt->stream_index >= d->fmt->nb_streams) {
        av_packet_free(&pkt);
        return AVERROR_INVALIDDATA;
    }
    SwbPacket *p = av_mallocz(sizeof *p);
    if (!p) {
        av_packet_free(&pkt);
        return AVERROR(ENOMEM);
    }
    p->pkt = pkt;
    p->time_base = d->fmt->streams[pkt->stream_index]->time_base;
    *out = p;
    return SWB_OK;
}

int swb_demuxer_seek(SwbDemuxer *d, int stream, int64_t target_us) {
    const AVStream *st = stream_at(d, stream);
    if (!st)
        return AVERROR(EINVAL);
    if (!d->seekable)
        return AVERROR(ENOSYS);
    AVRational tb = st->time_base;
    if (tb.num <= 0 || tb.den <= 0)
        return AVERROR(EINVAL);
    int64_t ts = av_rescale_q(target_us, AV_TIME_BASE_Q, tb);
    return av_seek_frame(d->fmt, stream, ts, AVSEEK_FLAG_BACKWARD);
}

/* Self-check for tests: asks the open context for a second resource, as a
 * demuxer would for an external reference. */
int swb_demuxer_try_io_open(const SwbDemuxer *d) {
    AVIOContext *pb = NULL;
    if (!d || !d->fmt || !d->fmt->io_open)
        return AVERROR(EINVAL);
    int r = d->fmt->io_open(d->fmt, &pb, "file:/dev/null", AVIO_FLAG_READ, NULL);
    if (r >= 0 && pb && d->fmt->io_close2)
        d->fmt->io_close2(d->fmt, pb);
    return r;
}

void swb_demuxer_close(SwbDemuxer *d) {
    if (!d)
        return;
    if (d->fmt)
        avformat_close_input(&d->fmt);
    if (d->io) {
        av_freep(&d->io->buffer);
        avio_context_free(&d->io);
    }
    av_free(d);
}

/* ---- Packets ---- */

int swb_packet_info(const SwbPacket *p, SwbPacketInfo *out) {
    if (!out)
        return AVERROR(EINVAL);
    memset(out, 0, sizeof *out);
    if (!p || !p->pkt)
        return AVERROR(EINVAL);
    out->stream = p->pkt->stream_index;
    out->key = (p->pkt->flags & AV_PKT_FLAG_KEY) != 0;
    out->pts_us = to_us(p->pkt->pts, p->time_base);
    out->dts_us = to_us(p->pkt->dts, p->time_base);
    out->duration_us = p->pkt->duration > 0 ? to_us(p->pkt->duration, p->time_base) : SWB_NO_TS;
    out->size = p->pkt->size;
    return SWB_OK;
}

void swb_packet_free(SwbPacket *p) {
    if (!p)
        return;
    av_packet_free(&p->pkt);
    av_free(p);
}

/* ---- Decoders ---- */

int swb_decoder_open(const SwbDemuxer *d, int stream, int threads, int64_t max_pixels,
                     SwbDecoder **out, char *err, size_t err_len) {
    if (!out)
        return AVERROR(EINVAL);
    *out = NULL;
    const AVStream *st = stream_at(d, stream);
    if (!st || !st->codecpar || threads < 0)
        return AVERROR(EINVAL);
    const AVCodec *codec = avcodec_find_decoder(st->codecpar->codec_id);
    if (!codec) {
        set_error(err, err_len, AVERROR_DECODER_NOT_FOUND, "decoder");
        return AVERROR_DECODER_NOT_FOUND;
    }
    SwbDecoder *dec = av_mallocz(sizeof *dec);
    if (!dec)
        return AVERROR(ENOMEM);
    dec->ctx = avcodec_alloc_context3(codec);
    if (!dec->ctx) {
        av_free(dec);
        return AVERROR(ENOMEM);
    }
    int r = avcodec_parameters_to_context(dec->ctx, st->codecpar);
    if (r >= 0) {
        dec->ctx->pkt_timebase = st->time_base;
        dec->time_base = st->time_base;
        dec->ctx->thread_count = threads;
        dec->ctx->thread_type = FF_THREAD_FRAME | FF_THREAD_SLICE;
        if (max_pixels > 0)
            dec->ctx->max_pixels = max_pixels;
        r = avcodec_open2(dec->ctx, codec, NULL);
    }
    if (r < 0) {
        set_error(err, err_len, r, "decoder");
        swb_decoder_close(dec);
        return r;
    }
    *out = dec;
    return SWB_OK;
}

int swb_decoder_send(SwbDecoder *dec, const SwbPacket *p) {
    if (!dec || !dec->ctx || (p && !p->pkt))
        return AVERROR(EINVAL);
    int r = avcodec_send_packet(dec->ctx, p ? p->pkt : NULL);
    if (r == AVERROR(EAGAIN))
        return SWB_AGAIN;
    if (r == AVERROR_EOF)
        return SWB_EOF;
    return r < 0 ? r : SWB_OK;
}

int swb_decoder_receive(SwbDecoder *dec, SwbFrame **out) {
    if (!dec || !dec->ctx || !out)
        return AVERROR(EINVAL);
    *out = NULL;
    AVFrame *frame = av_frame_alloc();
    if (!frame)
        return AVERROR(ENOMEM);
    int r = avcodec_receive_frame(dec->ctx, frame);
    if (r < 0) {
        av_frame_free(&frame);
        if (r == AVERROR(EAGAIN))
            return SWB_AGAIN;
        if (r == AVERROR_EOF)
            return SWB_EOF;
        return r;
    }
    SwbFrame *f = av_mallocz(sizeof *f);
    if (!f) {
        av_frame_free(&frame);
        return AVERROR(ENOMEM);
    }
    f->frame = frame;
    f->time_base = dec->time_base;
    *out = f;
    return SWB_OK;
}

void swb_decoder_flush(SwbDecoder *dec) {
    if (dec && dec->ctx)
        avcodec_flush_buffers(dec->ctx);
}

void swb_decoder_close(SwbDecoder *dec) {
    if (!dec)
        return;
    avcodec_free_context(&dec->ctx);
    av_free(dec);
}

/* ---- Frames ---- */

static const AVPixFmtDescriptor *video_desc(const AVFrame *fr) {
    if (!fr || fr->width <= 0 || fr->height <= 0 || fr->format < 0 || !fr->data[0])
        return NULL;
    const AVPixFmtDescriptor *desc = av_pix_fmt_desc_get((enum AVPixelFormat)fr->format);
    if (!desc || (desc->flags & AV_PIX_FMT_FLAG_HWACCEL))
        return NULL;
    return desc;
}

int swb_frame_video_info(const SwbFrame *f, SwbVideoFrameInfo *out) {
    if (!out)
        return AVERROR(EINVAL);
    memset(out, 0, sizeof *out);
    const AVFrame *fr = f ? f->frame : NULL;
    const AVPixFmtDescriptor *desc = video_desc(fr);
    if (!desc)
        return AVERROR(EINVAL);
    out->width = fr->width;
    out->height = fr->height;
    out->matrix = matrix_of(fr->colorspace);
    out->range = range_of(fr->color_range);
    out->has_alpha = (desc->flags & AV_PIX_FMT_FLAG_ALPHA) != 0;
    copy_name(out->pixel_format, sizeof out->pixel_format, desc->name);
    out->pts_us = to_us(fr->best_effort_timestamp, f->time_base);
    out->duration_us = fr->duration > 0 ? to_us(fr->duration, f->time_base) : SWB_NO_TS;
    return SWB_OK;
}

int swb_frame_audio_info(const SwbFrame *f, SwbAudioFrameInfo *out) {
    if (!out)
        return AVERROR(EINVAL);
    memset(out, 0, sizeof *out);
    const AVFrame *fr = f ? f->frame : NULL;
    if (!fr || fr->nb_samples <= 0 || fr->sample_rate <= 0 || fr->ch_layout.nb_channels <= 0 ||
        !fr->extended_data)
        return AVERROR(EINVAL);
    out->samples = fr->nb_samples;
    out->sample_rate = fr->sample_rate;
    out->channels = fr->ch_layout.nb_channels;
    out->pts_us = to_us(fr->best_effort_timestamp, f->time_base);
    return SWB_OK;
}

void swb_frame_free(SwbFrame *f) {
    if (!f)
        return;
    av_frame_free(&f->frame);
    av_free(f);
}

/* ---- Color conversion ---- */

SwbScaler *swb_scaler_new(void) {
    SwbScaler *s = av_mallocz(sizeof(SwbScaler));
    if (s)
        s->matrix = -1;
    return s;
}

/* A frame with subsampled planar chroma whose width is not a multiple of 64
 * or whose height is odd (with vertically subsampled chroma). Two problems:
 *  - With an odd size, the chroma planes have ceil(size / 2) samples.
 *    swscale scales a chroma plane by the ratio of the plane sizes (161 / 321
 *    for 321 px), not by exactly 2, so the chroma drifts against the luma by
 *    up to half a chroma sample across the frame. Chromium upsamples the
 *    chroma exactly twice (measured: the dense row `frames-dense-contain-odd`,
 *    M14 feature 7a review).
 *  - swscale's SIMD paths convert the last columns of a row separately when
 *    the width is not a multiple of the vector width (measured: the last 2 to
 *    6 columns of 322, 854 and 1366 differ by up to 84 levels), and they write
 *    up to 15 pixels past the end of the last row of `dst` for 8-bit 4:2:0 and
 *    4:2:2 (1918 x 1078 by 8 bytes, 2 x 2 by 56).
 * The caller converts a copy padded to a multiple of 64 pixels and an even
 * height (`ensure_padded`), into a scratch buffer, and crops the result.
 * Excluded: packed YUV (uyvy422, yuyv422), RGB, palette and bitstream formats,
 * and formats with fewer than 3 components; the build's decoders (h264, vp8,
 * vp9; tools/ffmpeg/build.sh) output only planar YUV formats. */
static int needs_padding(const AVFrame *fr, const AVPixFmtDescriptor *desc) {
    if (!(desc->flags & AV_PIX_FMT_FLAG_PLANAR) ||
        (desc->flags & (AV_PIX_FMT_FLAG_RGB | AV_PIX_FMT_FLAG_PAL | AV_PIX_FMT_FLAG_BITSTREAM)) ||
        desc->nb_components < 3 || (!desc->log2_chroma_w && !desc->log2_chroma_h))
        return 0;
    return (fr->width & 63) || ((fr->height & 1) && desc->log2_chroma_h);
}

/* `n` rounded up to a multiple of 64 (n > 0, below INT_MAX / 4). */
static int round_up_64(int n) {
    return (n + 63) & ~63;
}

/* Fills `p` (of the size `pw` x `ph`, at least the frame's in each direction)
 * with `fr` and repeats the last column and row of each plane. Returns 0, or
 * an error if a plane does not fit. */
static int fill_padded(AVFrame *p, const AVFrame *fr, int pw, int ph) {
    const AVPixFmtDescriptor *desc = av_pix_fmt_desc_get((enum AVPixelFormat)fr->format);
    if (!desc)
        return AVERROR(EINVAL);
    if (p->width != pw || p->height != ph || pw < fr->width || ph < fr->height)
        return AVERROR(EINVAL);
    int planes = av_pix_fmt_count_planes((enum AVPixelFormat)fr->format);
    for (int i = 0; i < planes; i++) {
        int shift = (i == 1 || i == 2) ? desc->log2_chroma_h : 0;
        int src_rows = AV_CEIL_RSHIFT(fr->height, shift);
        int dst_rows = AV_CEIL_RSHIFT(ph, shift);
        int src_bytes = av_image_get_linesize((enum AVPixelFormat)fr->format, fr->width, i);
        int dst_bytes = av_image_get_linesize((enum AVPixelFormat)fr->format, pw, i);
        if (src_bytes <= 0 || dst_bytes < src_bytes || src_rows <= 0 || !fr->data[i] ||
            !p->data[i] || p->linesize[i] < dst_bytes)
            return AVERROR(EINVAL);
        /* The bytes of one sample of the plane (a pixel's, for packed chroma). */
        int step = src_bytes / AV_CEIL_RSHIFT(fr->width, (i == 1 || i == 2) ? desc->log2_chroma_w : 0);
        if (step <= 0)
            return AVERROR(EINVAL);
        for (int y = 0; y < dst_rows; y++) {
            const uint8_t *src = fr->data[i] + (ptrdiff_t)(y < src_rows ? y : src_rows - 1) *
                                                   (ptrdiff_t)fr->linesize[i];
            uint8_t *dst = p->data[i] + (ptrdiff_t)y * (ptrdiff_t)p->linesize[i];
            memcpy(dst, src, (size_t)src_bytes);
            for (int k = src_bytes; k < dst_bytes; k++)
                dst[k] = src[src_bytes - step + (k - src_bytes) % step];
        }
    }
    return 0;
}

/* The scaler's padded copy of `fr` (size `pw` x `ph`), kept between frames
 * while the format and size stay the same. NULL if the allocation or the copy
 * fails (the scaler then holds no copy). */
static AVFrame *ensure_padded(SwbScaler *s, const AVFrame *fr, int pw, int ph) {
    if (s->pad && (s->pad->format != fr->format || s->pad->width != pw || s->pad->height != ph))
        av_frame_free(&s->pad);
    if (!s->pad) {
        AVFrame *p = av_frame_alloc();
        if (!p)
            return NULL;
        p->format = fr->format;
        p->width = pw;
        p->height = ph;
        if (av_frame_get_buffer(p, 0) < 0) {
            av_frame_free(&p);
            return NULL;
        }
        s->pad = p;
    }
    if (fill_padded(s->pad, fr, pw, ph) < 0) {
        av_frame_free(&s->pad);
        return NULL;
    }
    return s->pad;
}

/* The scaler's RGBA scratch: `rows` rows of `stride` bytes and one more row of
 * slack, so that a write of swscale past the last row stays inside it. NULL if
 * the size is above the allocator's limit. */
static uint8_t *scratch_rgba(SwbScaler *s, int stride, int rows) {
    uint64_t need = (uint64_t)stride * ((uint64_t)rows + 1);
    if (need > (uint64_t)INT_MAX)
        return NULL;
    if (need > s->buf_size) {
        av_free(s->buf);
        s->buf = av_malloc((size_t)need);
        s->buf_size = s->buf ? (size_t)need : 0;
    }
    return s->buf;
}

/* swscale: `fr` to RGBA of the size `dst_w` x `dst_h` into `dst`. */
static int scale_rgba(SwbScaler *s, const AVFrame *fr, int matrix, int full_range, int dst_w,
                      int dst_h, uint8_t *dst, int dst_stride) {
    s->sws = sws_getCachedContext(s->sws, fr->width, fr->height, (enum AVPixelFormat)fr->format,
                                  dst_w, dst_h, AV_PIX_FMT_RGBA, SWS_BILINEAR, NULL, NULL, NULL);
    if (!s->sws) {
        s->matrix = -1;
        return AVERROR(EINVAL);
    }
    if (s->src_w != fr->width || s->src_h != fr->height || s->src_format != fr->format ||
        s->dst_w != dst_w || s->dst_h != dst_h || s->matrix != matrix ||
        s->full_range != full_range) {
        s->matrix = -1; /* until the details are set */
        const int *table = sws_getCoefficients(matrix == SWB_MATRIX_BT709 ? SWS_CS_ITU709
                                                                          : SWS_CS_ITU601);
        /* {crv, cbu, cgu, cgv} in 16.16, for limited range input. Chromium's
         * frames show the Cb-to-B coefficient capped at 2.0 (ADR 0047
         * section 9). */
        int coeffs[4] = {table[0], table[1], table[2], table[3]};
        if (!full_range && coeffs[1] > CAP_CB_TO_B)
            coeffs[1] = CAP_CB_TO_B;
        int r = sws_setColorspaceDetails(s->sws, coeffs, full_range,
                                         sws_getCoefficients(SWS_CS_DEFAULT), 1, 0, 1 << 16,
                                         1 << 16);
        if (r < 0)
            return r;
        s->src_w = fr->width;
        s->src_h = fr->height;
        s->src_format = fr->format;
        s->dst_w = dst_w;
        s->dst_h = dst_h;
        s->matrix = matrix;
        s->full_range = full_range;
    }
    uint8_t *dst_planes[4] = {dst, NULL, NULL, NULL};
    int dst_strides[4] = {dst_stride, 0, 0, 0};
    int r = sws_scale(s->sws, (const uint8_t *const *)fr->data, fr->linesize, 0, fr->height,
                      dst_planes, dst_strides);
    return r < 0 ? r : SWB_OK;
}

int swb_scaler_rgba(SwbScaler *s, const SwbFrame *f, int matrix, int full_range,
                    int dst_w, int dst_h, uint8_t *dst, int dst_stride, size_t dst_len) {
    if (!s || !f || !dst)
        return AVERROR(EINVAL);
    const AVFrame *fr = f->frame;
    const AVPixFmtDescriptor *desc = video_desc(fr);
    if (!desc)
        return AVERROR(EINVAL);
    if (matrix != SWB_MATRIX_BT601 && matrix != SWB_MATRIX_BT709)
        return AVERROR(EINVAL);
    /* The height bound keeps `fr->height + 1` below (the padded rows) in int. */
    if (dst_w <= 0 || dst_h <= 0 || dst_w > INT_MAX / 4 - 64 || dst_h > INT_MAX - 2 ||
        dst_stride < dst_w * 4)
        return AVERROR(EINVAL);
    if ((uint64_t)dst_stride * (uint64_t)dst_h > dst_len)
        return AVERROR(EINVAL);
    full_range = full_range ? 1 : 0;
    /* swscale never writes into `dst` unless the width is a multiple of 64
     * pixels (its SIMD paths write up to a vector past the last row, and the
     * vector width depends on the CPU) and no padding is needed. Otherwise it
     * converts into the scaler's scratch buffer (width rounded up to 64, one
     * slack row) and the visible part is copied. `Decoder::convert` always
     * passes the frame size, so the padded source applies to every frame with
     * subsampled chroma; a scaled destination converts the unpadded frame
     * (and keeps the chroma drift of odd sizes). */
    const AVFrame *src = fr;
    int sw = dst_w, sh = dst_h;
    if (dst_w == fr->width && dst_h == fr->height && needs_padding(fr, desc)) {
        sw = round_up_64(fr->width);
        sh = fr->height + ((fr->height & 1) && desc->log2_chroma_h);
        src = ensure_padded(s, fr, sw, sh);
        if (!src)
            return AVERROR(ENOMEM);
    }
    int r;
    if (src == fr && dst_w % 64 == 0) {
        r = scale_rgba(s, fr, matrix, full_range, dst_w, dst_h, dst, dst_stride);
    } else {
        int stride = round_up_64(sw) * 4;
        uint8_t *rgba = scratch_rgba(s, stride, sh);
        if (!rgba)
            return AVERROR(ENOMEM);
        r = scale_rgba(s, src, matrix, full_range, sw, sh, rgba, stride);
        if (r >= 0) {
            for (int y = 0; y < dst_h; y++)
                memcpy(dst + (size_t)y * (size_t)dst_stride, rgba + (size_t)y * (size_t)stride,
                       (size_t)dst_w * 4);
        }
    }
    if (r < 0)
        return r;
    if (desc->flags & AV_PIX_FMT_FLAG_ALPHA) {
        /* swb shows video opaque (the pixmap is premultiplied). */
        for (int y = 0; y < dst_h; y++) {
            uint8_t *row = dst + (size_t)y * (size_t)dst_stride;
            for (int x = 0; x < dst_w; x++)
                row[(size_t)x * 4 + 3] = 255;
        }
    }
    return SWB_OK;
}

void swb_scaler_free(SwbScaler *s) {
    if (!s)
        return;
    sws_freeContext(s->sws);
    av_frame_free(&s->pad);
    av_free(s->buf);
    av_free(s);
}

/* ---- Resampling ---- */

SwbResampler *swb_resampler_new(int out_rate, int out_channels) {
    if (out_rate <= 0 || out_channels <= 0 || out_channels > 8)
        return NULL;
    SwbResampler *r = av_mallocz(sizeof *r);
    if (!r)
        return NULL;
    r->out_rate = out_rate;
    r->out_channels = out_channels;
    r->in_format = -1;
    return r;
}

/* Configures the resampler for the format of `fr` when it changed. */
static int resampler_prepare(SwbResampler *r, const AVFrame *fr) {
    if (r->swr && fr->sample_rate == r->in_rate && fr->format == r->in_format &&
        av_channel_layout_compare(&fr->ch_layout, &r->in_layout) == 0)
        return SWB_OK;
    /* The samples that the old context still buffers (its filter delay,
     * a few milliseconds) are dropped, not flushed: the API returns the
     * output of one frame in one call, and the formats of the enabled
     * decoders change inside a stream only on a broken or hostile file,
     * where the glitch is acceptable. */
    swr_free(&r->swr);
    av_channel_layout_uninit(&r->in_layout);
    r->in_format = -1;
    AVChannelLayout in_layout;
    int e;
    if (fr->ch_layout.order == AV_CHANNEL_ORDER_UNSPEC) {
        /* Without channel positions there is no downmix matrix: take the
         * default layout of the count. */
        av_channel_layout_default(&in_layout, fr->ch_layout.nb_channels);
        e = 0;
    } else {
        e = av_channel_layout_copy(&in_layout, &fr->ch_layout);
    }
    if (e < 0)
        return e;
    AVChannelLayout out_layout;
    av_channel_layout_default(&out_layout, r->out_channels);
    e = swr_alloc_set_opts2(&r->swr, &out_layout, AV_SAMPLE_FMT_FLT, r->out_rate, &in_layout,
                            (enum AVSampleFormat)fr->format, fr->sample_rate, 0, NULL);
    av_channel_layout_uninit(&out_layout);
    av_channel_layout_uninit(&in_layout);
    if (e < 0)
        return e;
    e = swr_init(r->swr);
    if (e < 0) {
        swr_free(&r->swr);
        return e;
    }
    e = av_channel_layout_copy(&r->in_layout, &fr->ch_layout);
    if (e < 0) {
        swr_free(&r->swr);
        return e;
    }
    r->in_rate = fr->sample_rate;
    r->in_format = fr->format;
    return SWB_OK;
}

static const AVFrame *audio_frame(const SwbFrame *f) {
    const AVFrame *fr = f ? f->frame : NULL;
    if (!fr || fr->nb_samples <= 0 || fr->sample_rate <= 0 || fr->format < 0 ||
        fr->ch_layout.nb_channels <= 0 || !fr->extended_data)
        return NULL;
    return fr;
}

int swb_resampler_max_out(SwbResampler *r, const SwbFrame *f) {
    if (!r)
        return AVERROR(EINVAL);
    if (!f)
        return r->swr ? swr_get_out_samples(r->swr, 0) : 0;
    const AVFrame *fr = audio_frame(f);
    if (!fr)
        return AVERROR(EINVAL);
    int e = resampler_prepare(r, fr);
    if (e < 0)
        return e;
    return swr_get_out_samples(r->swr, fr->nb_samples);
}

int swb_resampler_f32(SwbResampler *r, const SwbFrame *f, float *dst, int dst_samples) {
    if (!r || !dst || dst_samples < 0)
        return AVERROR(EINVAL);
    uint8_t *out[1] = {(uint8_t *)dst};
    if (!f)
        return r->swr ? swr_convert(r->swr, out, dst_samples, NULL, 0) : 0;
    const AVFrame *fr = audio_frame(f);
    if (!fr)
        return AVERROR(EINVAL);
    int e = resampler_prepare(r, fr);
    if (e < 0)
        return e;
    return swr_convert(r->swr, out, dst_samples, (const uint8_t *const *)fr->extended_data,
                       fr->nb_samples);
}

void swb_resampler_free(SwbResampler *r) {
    if (!r)
        return;
    swr_free(&r->swr);
    av_channel_layout_uninit(&r->in_layout);
    av_free(r);
}
