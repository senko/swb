/* The C boundary of swb's audio output (ADR 0047 sections 4 and 8): one
 * PipeWire output stream behind three functions. No PipeWire type crosses
 * this header. Rust holds the handle in one owner whose Drop calls
 * swb_audio_close. */
#ifndef SWB_PIPEWIRE_H
#define SWB_PIPEWIRE_H

#include <stddef.h>
#include <stdint.h>

typedef struct SwbAudio SwbAudio;

/* States that swb_audio_state_fn reports (the values of
 * enum pw_stream_state, mapped). */
#define SWB_AUDIO_ERROR (-1)
#define SWB_AUDIO_UNCONNECTED 0
#define SWB_AUDIO_CONNECTING 1
#define SWB_AUDIO_PAUSED 2
#define SWB_AUDIO_STREAMING 3

/* Called on PipeWire's real-time thread, once per cycle: writes `frames`
 * frames of interleaved stereo f32 (48 kHz) into `buf`. `delay_frames` is
 * the delay between the frames of this cycle and the device, at 48 kHz
 * (pw_stream_get_time_n; 0 when PipeWire gives none). It must not block,
 * allocate or log. */
typedef void (*swb_audio_fill_fn)(void *opaque, float *buf, uint32_t frames, int64_t delay_frames);

/* Called on PipeWire's loop thread when the state of the stream changes
 * (not real-time). `state` is one of the SWB_AUDIO_ values. */
typedef void (*swb_audio_state_fn)(void *opaque, int state);

/* Makes the stream (F32 interleaved stereo, 48 kHz, node.latency
 * latency_frames/48000, media.role Movie, application name "swb",
 * AUTOCONNECT | MAP_BUFFERS | RT_PROCESS) and connects it, without waiting
 * for the state STREAMING: swb_audio_state_fn tells. Returns 0, or a
 * negative errno (-EHOSTDOWN without a server, at once); then *out is NULL
 * and err holds a NUL-terminated message. */
int swb_audio_open(uint32_t latency_frames, void *opaque, swb_audio_fill_fn fill,
                   swb_audio_state_fn state, SwbAudio **out, char *err, size_t err_len);

/* Starts (1) or stops (0) the cycles of the stream: while it is not active
 * no process callback runs. */
void swb_audio_set_active(SwbAudio *a, int active);

/* Stops the thread, destroys the stream and frees the handle. After it
 * returns no callback runs. NULL is allowed. */
void swb_audio_close(SwbAudio *a);

#endif
