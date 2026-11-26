/* Audio output through one PipeWire stream (ADR 0047 sections 4 and 8).
 *
 * One pw_thread_loop and one pw_stream per SwbAudio. The stream's process
 * callback runs on PipeWire's real-time thread and calls the Rust fill
 * function, which must not block, allocate or log; the state callback runs
 * on the loop thread. Only the public API of PipeWire is used. */

/* PipeWire's headers use POSIX and GNU types (locale_t, ssize_t). */
#define _GNU_SOURCE

#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>

#include "pipewire.h"

#define SWB_CHANNELS 2u
#define SWB_RATE 48000u

struct SwbAudio {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    struct spa_hook listener;
    void *opaque;
    swb_audio_fill_fn fill;
    swb_audio_state_fn state;
};

static pthread_once_t init_once = PTHREAD_ONCE_INIT;

static void init_pipewire(void) {
    pw_init(NULL, NULL);
}

/* The delay of the stream's output to the device, in frames at 48 kHz. */
static int64_t delay_frames(struct pw_stream *stream) {
    struct pw_time t;
    if (pw_stream_get_time_n(stream, &t, sizeof t) != 0 || t.rate.denom == 0 || t.delay < 0)
        return 0;
    return (t.delay * (int64_t)SWB_RATE * (int64_t)t.rate.num) / (int64_t)t.rate.denom;
}

static void on_process(void *data) {
    SwbAudio *a = data;
    struct pw_buffer *b = pw_stream_dequeue_buffer(a->stream);
    if (!b)
        return;
    struct spa_data *d = &b->buffer->datas[0];
    float *dst = d->data;
    uint32_t stride = (uint32_t)sizeof(float) * SWB_CHANNELS;
    uint32_t n = 0;
    if (dst && d->chunk) {
        n = d->maxsize / stride;
        if (b->requested && b->requested < n)
            n = (uint32_t)b->requested;
        if (n > 0)
            a->fill(a->opaque, dst, n, delay_frames(a->stream));
    }
    if (d->chunk) {
        d->chunk->offset = 0;
        d->chunk->stride = (int32_t)stride;
        d->chunk->size = n * stride;
    }
    pw_stream_queue_buffer(a->stream, b);
}

static void on_state_changed(void *data, enum pw_stream_state old, enum pw_stream_state state,
                             const char *error) {
    (void)old;
    (void)error;
    SwbAudio *a = data;
    int mapped;
    switch (state) {
    case PW_STREAM_STATE_UNCONNECTED:
        mapped = SWB_AUDIO_UNCONNECTED;
        break;
    case PW_STREAM_STATE_CONNECTING:
        mapped = SWB_AUDIO_CONNECTING;
        break;
    case PW_STREAM_STATE_PAUSED:
        mapped = SWB_AUDIO_PAUSED;
        break;
    case PW_STREAM_STATE_STREAMING:
        mapped = SWB_AUDIO_STREAMING;
        break;
    default:
        mapped = SWB_AUDIO_ERROR;
        break;
    }
    a->state(a->opaque, mapped);
}

static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS,
    .state_changed = on_state_changed,
    .process = on_process,
};

static void destroy(SwbAudio *a) {
    if (a->loop) {
        pw_thread_loop_lock(a->loop);
        if (a->stream)
            pw_stream_destroy(a->stream);
        a->stream = NULL;
        pw_thread_loop_unlock(a->loop);
        pw_thread_loop_stop(a->loop);
        pw_thread_loop_destroy(a->loop);
    }
    free(a);
}

int swb_audio_open(uint32_t latency_frames, void *opaque, swb_audio_fill_fn fill,
                   swb_audio_state_fn state, SwbAudio **out, char *err, size_t err_len) {
    *out = NULL;
    if (!fill || !state) {
        snprintf(err, err_len, "no callback");
        return -EINVAL;
    }
    pthread_once(&init_once, init_pipewire);
    SwbAudio *a = calloc(1, sizeof *a);
    if (!a) {
        snprintf(err, err_len, "out of memory");
        return -ENOMEM;
    }
    a->opaque = opaque;
    a->fill = fill;
    a->state = state;
    a->loop = pw_thread_loop_new("swb-audio", NULL);
    if (!a->loop) {
        int e = errno ? errno : ENOMEM;
        snprintf(err, err_len, "pw_thread_loop_new: %s", strerror(e));
        free(a);
        return -e;
    }
    char latency[32];
    snprintf(latency, sizeof latency, "%u/%u", (unsigned)latency_frames, SWB_RATE);
    struct pw_properties *props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Playback", PW_KEY_MEDIA_ROLE, "Movie",
        PW_KEY_APP_NAME, "swb", PW_KEY_NODE_NAME, "swb", PW_KEY_NODE_LATENCY, latency, NULL);
    if (!props) {
        snprintf(err, err_len, "pw_properties_new: out of memory");
        destroy(a);
        return -ENOMEM;
    }
    pw_thread_loop_lock(a->loop);
    a->stream = pw_stream_new_simple(pw_thread_loop_get_loop(a->loop), "swb media", props,
                                     &stream_events, a);
    if (!a->stream) {
        int e = errno ? errno : ENOMEM;
        snprintf(err, err_len, "pw_stream_new_simple: %s", strerror(e));
        pw_thread_loop_unlock(a->loop);
        destroy(a);
        return -e;
    }
    uint8_t pod_buf[1024];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(pod_buf, sizeof pod_buf);
    struct spa_audio_info_raw info = {
        .format = SPA_AUDIO_FORMAT_F32,
        .rate = SWB_RATE,
        .channels = SWB_CHANNELS,
    };
    info.position[0] = SPA_AUDIO_CHANNEL_FL;
    info.position[1] = SPA_AUDIO_CHANNEL_FR;
    const struct spa_pod *params[1];
    params[0] = spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &info);
    int r = pw_stream_connect(a->stream, PW_DIRECTION_OUTPUT, PW_ID_ANY,
                              PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS |
                                  PW_STREAM_FLAG_RT_PROCESS,
                              params, 1);
    if (r < 0) {
        snprintf(err, err_len, "pw_stream_connect: %s", strerror(-r));
        pw_thread_loop_unlock(a->loop);
        destroy(a);
        return r;
    }
    r = pw_thread_loop_start(a->loop);
    pw_thread_loop_unlock(a->loop);
    if (r < 0) {
        snprintf(err, err_len, "pw_thread_loop_start: %s", strerror(-r));
        destroy(a);
        return r;
    }
    *out = a;
    return 0;
}

void swb_audio_set_active(SwbAudio *a, int active) {
    if (!a || !a->loop || !a->stream)
        return;
    pw_thread_loop_lock(a->loop);
    pw_stream_set_active(a->stream, active != 0);
    pw_thread_loop_unlock(a->loop);
}

void swb_audio_close(SwbAudio *a) {
    if (a)
        destroy(a);
}
