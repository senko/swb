//! Animation frames (HTML §8.10,
//! <https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#animation-frames>;
//! M8 feature 7b2): `requestAnimationFrame`, `cancelAnimationFrame` and
//! the "run the animation frame callbacks" step of the rendering update,
//! which the engine calls through
//! [`crate::ScriptRuntime::run_animation_frame_callbacks`].
//!
//! The callbacks wait in the window's map of animation frame callbacks by
//! handle. A frame runs the callbacks that are in the map when it starts,
//! in the order of their handles; one that a callback of the frame
//! cancels does not run, and one that it requests runs in the next frame.
//! Each callback runs as a task of its own (its own deadline and a
//! microtask checkpoint after it, ADR 0028 section 7), with the time of the
//! frame as the argument and `undefined` as `this`.
//!
//! Measured in Chromium 148 (`tools/probes/web-render-update.json`):
//! handles start at 1 per document and grow by one, also after a cancel;
//! all callbacks of a frame get the same time, which is not later than
//! `performance.now()` in the callback; an exception of a callback is
//! reported (`window.onerror` sees "Uncaught Error: boom") and the next
//! callback runs; the microtasks of a callback run before the next one.

use std::collections::BTreeMap;

use swb_js::{Gc, Object, Runtime, Tracer, Value};

use crate::bindings as b;
use crate::state::{state, state_mut};

/// The most callbacks that wait at once in a document; one more ends the
/// task (the map lives outside the JavaScript heap).
const MAX_CALLBACKS: usize = 1 << 20;

/// The window's map of animation frame callbacks.
#[derive(Debug, Default)]
pub(crate) struct AnimationFrames {
    /// The last handle given out.
    last: u32,
    callbacks: BTreeMap<u32, Gc<Object>>,
}

impl AnimationFrames {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for callback in self.callbacks.values() {
            tracer.object(*callback);
        }
    }

    /// True if a callback waits for the next frame.
    pub(crate) fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }
}

/// `requestAnimationFrame(callback)`: the handle of the new entry.
pub(crate) fn request(rt: &mut Runtime, callback: Gc<Object>) -> b::Result<u32> {
    if state(rt)?.animation_frames.callbacks.len() >= MAX_CALLBACKS {
        log::warn!("a document has more than {MAX_CALLBACKS} animation frame callbacks");
        return Err(rt.terminate("too many animation frame callbacks").into());
    }
    let frames = &mut state_mut(rt)?.animation_frames;
    // Handles start at 1; 0 is never a handle, also after a wrap.
    frames.last = frames.last.wrapping_add(1).max(1);
    let handle = frames.last;
    frames.callbacks.insert(handle, callback);
    Ok(handle)
}

/// `cancelAnimationFrame(handle)`.
pub(crate) fn cancel(rt: &mut Runtime, handle: u32) -> b::Result<()> {
    state_mut(rt)?.animation_frames.callbacks.remove(&handle);
    Ok(())
}

/// The handles of the callbacks that run in a frame that starts now.
pub(crate) fn handles(rt: &Runtime) -> Vec<u32> {
    state(rt).map_or_else(
        |_| Vec::new(),
        |s| s.animation_frames.callbacks.keys().copied().collect(),
    )
}

/// Runs the callback of `handle` if it was not canceled, with the frame
/// time `now` (a part of a task; the caller reports errors).
pub(crate) fn run(rt: &mut Runtime, handle: u32, now: f64) -> Result<(), swb_js::ScriptError> {
    let callback = state_mut(rt)
        .map_err(crate::runtime::task_error)?
        .animation_frames
        .callbacks
        .remove(&handle);
    let Some(callback) = callback else {
        return Ok(());
    };
    b::keep(rt, Value::Object(callback));
    let result = rt
        .call_function(
            Value::Object(callback),
            Value::Undefined,
            &[Value::number(now)],
        )
        .map(|_| ());
    match &result {
        Ok(()) | Err(swb_js::ScriptError::Terminated(_) | swb_js::ScriptError::Internal(_)) => {
            result
        }
        Err(error) => {
            let site = crate::errors::Site::named("an animation frame callback");
            crate::errors::report(rt, &site, error).map_err(crate::runtime::task_error)
        }
    }
}
