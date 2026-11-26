//! Safe Rust type over `csrc/pipewire.c` (ADR 0047 sections 4 and 8): one
//! PipeWire output stream (F32 interleaved stereo at 48 kHz) whose
//! real-time callback pulls samples from a Rust closure.
//!
//! The handle is owned by one [`AudioStream`] whose `Drop` stops the thread
//! and frees it; after the drop no callback runs. The closures are boxed by
//! the owner and outlive the C object. Both callbacks catch panics: a fill
//! that panics gives silence.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};

mod sys {
    use std::ffi::{c_char, c_int, c_void};

    #[repr(C)]
    pub(super) struct Opaque {
        _p: [u8; 0],
    }

    pub(super) type FillFn = unsafe extern "C" fn(*mut c_void, *mut f32, u32, i64);
    pub(super) type StateFn = unsafe extern "C" fn(*mut c_void, c_int);

    pub(super) const SWB_AUDIO_UNCONNECTED: c_int = 0;
    pub(super) const SWB_AUDIO_CONNECTING: c_int = 1;
    pub(super) const SWB_AUDIO_PAUSED: c_int = 2;
    pub(super) const SWB_AUDIO_STREAMING: c_int = 3;

    unsafe extern "C" {
        pub(super) fn swb_audio_open(
            latency_frames: u32,
            opaque: *mut c_void,
            fill: FillFn,
            state: StateFn,
            out: *mut *mut Opaque,
            err: *mut c_char,
            err_len: usize,
        ) -> c_int;
        pub(super) fn swb_audio_set_active(a: *mut Opaque, active: c_int);
        pub(super) fn swb_audio_close(a: *mut Opaque);
    }
}

/// The version of the PipeWire headers that the C file was compiled against
/// (`pkg-config libpipewire-0.3`).
pub const HEADERS_VERSION: &str = env!("SWB_PIPEWIRE_HEADERS_VERSION");

/// The state of a stream, as the loop thread reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Not connected (also after the server went away).
    Unconnected,
    /// Connecting to the server and negotiating.
    Connecting,
    /// Connected, not running (no link yet, or not active).
    Paused,
    /// Cycles run: the fill closure is called.
    Streaming,
    /// The stream failed.
    Error,
}

impl StreamState {
    fn from_c(state: c_int) -> StreamState {
        match state {
            sys::SWB_AUDIO_UNCONNECTED => StreamState::Unconnected,
            sys::SWB_AUDIO_CONNECTING => StreamState::Connecting,
            sys::SWB_AUDIO_PAUSED => StreamState::Paused,
            sys::SWB_AUDIO_STREAMING => StreamState::Streaming,
            _ => StreamState::Error,
        }
    }
}

/// An error of [`AudioStream::open`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} ({code})")]
pub struct OpenError {
    /// The negative errno of the failed call (`-EHOSTDOWN`, -112, without a
    /// server).
    pub code: i32,
    /// What failed.
    pub message: String,
}

/// The real-time fill: writes interleaved stereo frames into the buffer and
/// gets the delay between them and the device, in frames at 48 kHz. It runs
/// on PipeWire's real-time thread: it must not block, allocate or log.
pub type Fill = Box<dyn FnMut(&mut [f32], i64) + Send>;
/// The state callback, on the loop thread.
pub type OnState = Box<dyn Fn(StreamState) + Send + Sync>;

struct Callbacks {
    fill: Fill,
    state: OnState,
}

unsafe extern "C" fn fill_cb(opaque: *mut c_void, buf: *mut f32, frames: u32, delay: i64) {
    let Some(samples) = (frames as usize).checked_mul(2) else {
        return;
    };
    if buf.is_null() {
        return;
    }
    // SAFETY: the C side passes a buffer that holds `frames` frames of two
    // `f32` (it clamps `frames` to the buffer's size), and nothing else
    // uses it during the call.
    let out = unsafe { std::slice::from_raw_parts_mut(buf, samples) };
    // SAFETY: `opaque` is the `Callbacks` that the owning `AudioStream`
    // boxed; it outlives the C object, and only the real-time thread calls
    // this function. The reference is to the `fill` field alone, which the
    // state callback (another thread) does not touch.
    let fill = unsafe { &mut (*opaque.cast::<Callbacks>()).fill };
    if catch_unwind(AssertUnwindSafe(|| fill(out, delay))).is_err() {
        out.fill(0.0);
    }
}

unsafe extern "C" fn state_cb(opaque: *mut c_void, state: c_int) {
    // SAFETY: `opaque` is the `Callbacks` that the owning `AudioStream`
    // boxed; it outlives the C object. The reference is to the `state`
    // field alone (shared), which the fill callback does not touch.
    let state_fn = unsafe { &(*opaque.cast::<Callbacks>()).state };
    let _ = catch_unwind(AssertUnwindSafe(|| state_fn(StreamState::from_c(state))));
}

/// One PipeWire output stream. Dropping it stops the thread and destroys
/// the stream.
pub struct AudioStream {
    raw: NonNull<sys::Opaque>,
    /// The callbacks' state (from `Box::into_raw`, the C side's opaque
    /// pointer): freed in `drop`, after `swb_audio_close`.
    callbacks: *mut Callbacks,
}

// SAFETY: the C handle is used from one thread at a time (every method that
// uses it takes `&mut self`, or the drop); the C side locks PipeWire's
// thread loop where it touches the stream. The callbacks are `Send`.
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// Makes the stream and connects it, without waiting for it to stream:
    /// `on_state` tells (from the loop thread). `latency_frames` is the node
    /// latency (frames at 48 kHz). Fails at once when there is no server.
    pub fn open(
        latency_frames: u32,
        fill: Fill,
        on_state: OnState,
    ) -> Result<AudioStream, OpenError> {
        // The C side keeps this pointer: it comes from `into_raw` (not from
        // a reference to a box that moves), and `from_raw` frees it once,
        // after the C object is closed (in `drop`) or never opened.
        let callbacks = Box::into_raw(Box::new(Callbacks {
            fill,
            state: on_state,
        }));
        let opaque = callbacks.cast::<c_void>();
        let mut out: *mut sys::Opaque = ptr::null_mut();
        let mut err = [0 as c_char; 256];
        // SAFETY: `opaque` points to the leaked callbacks, which this value
        // keeps alive until after `swb_audio_close`; `out` and `err` are
        // valid for the writes (`err` has `err.len()` bytes).
        let code = unsafe {
            sys::swb_audio_open(
                latency_frames,
                opaque,
                fill_cb,
                state_cb,
                &raw mut out,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        match NonNull::new(out) {
            Some(raw) if code == 0 => Ok(AudioStream { raw, callbacks }),
            _ => {
                // SAFETY: the open failed, so the C side keeps no pointer to
                // `callbacks` (it made no stream); this is the only free of
                // the pointer from `Box::into_raw` above.
                drop(unsafe { Box::from_raw(callbacks) });
                // SAFETY: `err` was zeroed, and the C side writes a
                // NUL-terminated string that fits.
                let message = unsafe { CStr::from_ptr(err.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();
                Err(OpenError {
                    code: if code == 0 { -1 } else { code },
                    message,
                })
            }
        }
    }

    /// Starts or stops the cycles: while the stream is not active, the fill
    /// closure is not called.
    pub fn set_active(&mut self, active: bool) {
        // SAFETY: `raw` is a live handle (it is freed only in `drop`).
        unsafe { sys::swb_audio_set_active(self.raw.as_ptr(), c_int::from(active)) };
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        // SAFETY: `raw` is live; the close stops the thread, so no callback
        // runs after it returns.
        unsafe { sys::swb_audio_close(self.raw.as_ptr()) };
        // SAFETY: `callbacks` is the pointer from `Box::into_raw` in `open`;
        // the C object is closed (nothing uses the pointer any more), and
        // `drop` runs once, so this is the only free.
        drop(unsafe { Box::from_raw(self.callbacks) });
    }
}
