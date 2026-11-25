//! The byte source of a demuxer and the read and seek callbacks that
//! FFmpeg calls (ADR 0047 section 5).

use std::ffi::{c_int, c_void};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::sys;

/// Where a demuxer reads its bytes from. Called on the demuxer's thread; a
/// read may block (a network request) and fails when it is cancelled.
pub trait Input: Send {
    /// Reads up to `buf.len()` bytes; `Ok(0)` at the end.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// Seeks to an absolute offset. Fails for a source that cannot seek.
    fn seek(&mut self, pos: u64) -> io::Result<u64>;
    /// The size of the resource, when it is known and the source can seek.
    /// `None` makes the demuxer read the source as a stream.
    fn size(&mut self) -> Option<u64>;
}

/// The state that the callbacks reach through their `opaque` pointer.
struct State {
    input: Box<dyn Input>,
    /// The last error of the input, for `Demuxer::take_source_error`.
    error: Option<io::Error>,
}

/// The input of a demuxer, at a fixed address for the C side.
///
/// It holds the state as a raw pointer from `Box::into_raw`, so that the
/// callbacks' pointer and Rust's accesses do not alias a `Box`. Rust uses
/// the state only while no C call of its demuxer runs (the demuxer's
/// methods take `&mut self`).
pub(super) struct Source {
    state: *mut State,
    seekable: bool,
}

impl Source {
    pub(super) fn new(mut input: Box<dyn Input>) -> Source {
        let seekable = input.size().is_some();
        let state = Box::into_raw(Box::new(State { input, error: None }));
        Source { state, seekable }
    }

    /// Whether the input can seek (it knows its size).
    pub(super) fn seekable(&self) -> bool {
        self.seekable
    }

    /// The pointer that the callbacks get.
    pub(super) fn opaque(&mut self) -> *mut c_void {
        self.state.cast()
    }

    /// The error of the input since the last call, if any.
    pub(super) fn take_error(&mut self) -> Option<io::Error> {
        // SAFETY: `state` is from `Box::into_raw` and lives until `drop`;
        // no C call of the demuxer runs during `&mut self`, so this is the
        // only reference.
        let state = unsafe { &mut *self.state };
        state.error.take()
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        // SAFETY: `state` is from `Box::into_raw`, freed only here; the
        // demuxer was closed before (`Demuxer::drop` runs before its fields
        // drop), so the C side holds no pointer to it any more.
        drop(unsafe { Box::from_raw(self.state) });
    }
}

// SAFETY: the state holds a `Send` input and an error; it moves with its
// demuxer and is used by one thread at a time.
unsafe impl Send for Source {}

/// FFmpeg's read callback: reads into `buf` (`size` bytes).
pub(super) unsafe extern "C" fn read_callback(
    opaque: *mut c_void,
    buf: *mut u8,
    size: c_int,
) -> c_int {
    let Ok(len) = usize::try_from(size) else {
        return -1;
    };
    if opaque.is_null() || buf.is_null() || len == 0 {
        return -1;
    }
    // SAFETY: `opaque` is the `State` of a live `Source`; FFmpeg calls this
    // on the thread that runs a call of the demuxer, while Rust holds no
    // reference to the state.
    let state = unsafe { &mut *opaque.cast::<State>() };
    // SAFETY: FFmpeg passes a writable buffer of `size` bytes that lives
    // for the call.
    let out = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        loop {
            match state.input.read(out) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                other => return other,
            }
        }
    }));
    match result {
        Ok(Ok(n)) if n <= len => c_int::try_from(n).unwrap_or(-1),
        Ok(Ok(_)) => {
            state.error = Some(io::Error::other("the input read more than asked"));
            -1
        }
        Ok(Err(e)) => {
            state.error = Some(e);
            -1
        }
        Err(_) => {
            state.error = Some(io::Error::other("the input panicked"));
            -1
        }
    }
}

/// FFmpeg's seek callback: seeks to `offset`, or returns the size for
/// `SWB_SEEK_SIZE`.
pub(super) unsafe extern "C" fn seek_callback(opaque: *mut c_void, offset: i64) -> i64 {
    if opaque.is_null() {
        return -1;
    }
    // SAFETY: as in `read_callback`.
    let state = unsafe { &mut *opaque.cast::<State>() };
    let result = catch_unwind(AssertUnwindSafe(|| {
        if offset == sys::SWB_SEEK_SIZE {
            return Ok(state.input.size());
        }
        let pos = u64::try_from(offset).map_err(|_| io::Error::other("negative offset"))?;
        state.input.seek(pos).map(Some)
    }));
    match result {
        Ok(Ok(Some(v))) => i64::try_from(v).unwrap_or(-1),
        Ok(Ok(None)) => -1,
        Ok(Err(e)) => {
            state.error = Some(e);
            -1
        }
        Err(_) => {
            state.error = Some(io::Error::other("the input panicked"));
            -1
        }
    }
}
