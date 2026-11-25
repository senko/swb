//! Conversions of decoded frames: video to RGBA with `swscale` (ADR 0047
//! section 9), audio to interleaved `f32` with `swresample` (section 6).

use std::ffi::c_int;
use std::ptr::NonNull;

use super::{Error, Frame, ensure_init, sys};

/// The YCbCr matrix of a conversion to RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YuvMatrix {
    /// BT.601 (SMPTE 170M, BT.470BG).
    Bt601,
    /// BT.709.
    Bt709,
}

/// Converts video frames to RGBA (`swscale`). It keeps its context
/// between frames of the same size and format.
pub struct Scaler {
    raw: NonNull<sys::Opaque>,
}

// SAFETY: a scale context is used by one thread at a time (`&mut self`).
unsafe impl Send for Scaler {}

impl Scaler {
    /// A new scaler. Fails only when the allocation fails.
    pub fn new() -> Result<Scaler, Error> {
        ensure_init();
        // SAFETY: allocates a small struct; no arguments.
        let raw = unsafe { sys::swb_scaler_new() };
        NonNull::new(raw)
            .map(|raw| Scaler { raw })
            .ok_or_else(|| Error::local("out of memory"))
    }

    /// Converts `frame` to RGBA with alpha 255, scaled to `width` x
    /// `height`, into `dst` (rows of `stride` bytes). For limited range the
    /// Cb-to-B coefficient is capped at 2.0 (Chromium's colors, ADR 0047
    /// section 9); the chroma is bilinear, without accurate rounding.
    pub fn rgba(
        &mut self,
        frame: &Frame,
        matrix: YuvMatrix,
        full_range: bool,
        size: (u32, u32),
        dst: &mut [u8],
        stride: usize,
    ) -> Result<(), Error> {
        let (width, height) = size;
        let row = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(4))
            .ok_or_else(|| Error::local("width out of range"))?;
        let rows = usize::try_from(height).map_err(|_| Error::local("height out of range"))?;
        if stride < row
            || stride
                .checked_mul(rows)
                .is_none_or(|needed| needed > dst.len())
        {
            return Err(Error::local("the destination is too small"));
        }
        let (Ok(c_width), Ok(c_height), Ok(c_stride)) = (
            c_int::try_from(width),
            c_int::try_from(height),
            c_int::try_from(stride),
        ) else {
            return Err(Error::local("size out of range"));
        };
        let c_matrix = match matrix {
            YuvMatrix::Bt601 => sys::SWB_MATRIX_BT601,
            YuvMatrix::Bt709 => sys::SWB_MATRIX_BT709,
        };
        // SAFETY: `raw` and `frame` are live; `dst` has `dst.len()` writable
        // bytes, at least `stride * height` (checked above, and again by the
        // C side).
        let r = unsafe {
            sys::swb_scaler_rgba(
                self.raw.as_ptr(),
                frame.as_ptr(),
                c_matrix,
                c_int::from(full_range),
                c_width,
                c_height,
                dst.as_mut_ptr(),
                c_stride,
                dst.len(),
            )
        };
        if r < 0 {
            Err(Error::new(r, "convert to RGBA"))
        } else {
            Ok(())
        }
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and owned by this value.
        unsafe { sys::swb_scaler_free(self.raw.as_ptr()) };
    }
}

/// The most samples per channel that one `Resampler::convert` call sizes a
/// buffer for (about 87 s at 48 kHz; real frames hold some thousands).
const MAX_SAMPLES_PER_CALL: usize = 1 << 22;

/// Converts audio frames to interleaved `f32` at one rate and channel count
/// (`swresample`; more channels are mixed down).
pub struct Resampler {
    raw: NonNull<sys::Opaque>,
    channels: usize,
}

// SAFETY: a resample context is used by one thread at a time (`&mut self`).
unsafe impl Send for Resampler {}

impl Resampler {
    /// A resampler to `rate` samples per second and `channels` channels
    /// (1 to 8).
    pub fn new(rate: u32, channels: u32) -> Result<Resampler, Error> {
        ensure_init();
        let (Ok(r), Ok(c)) = (c_int::try_from(rate), c_int::try_from(channels)) else {
            return Err(Error::local("rate or channels out of range"));
        };
        // SAFETY: allocates a small struct; the C side checks the values.
        let raw = unsafe { sys::swb_resampler_new(r, c) };
        let raw = NonNull::new(raw).ok_or_else(|| Error::local("bad rate or channel count"))?;
        Ok(Resampler {
            raw,
            channels: usize::try_from(channels).unwrap_or(1),
        })
    }

    /// Converts `frame` (or, with `None`, the samples that the resampler
    /// still holds, at the end of a stream) and appends the interleaved
    /// samples to `out`. Returns the samples per channel appended.
    pub fn convert(&mut self, frame: Option<&Frame>, out: &mut Vec<f32>) -> Result<usize, Error> {
        let f = frame.map_or(std::ptr::null(), Frame::as_ptr);
        // SAFETY: `raw` is live; `f` is null or a live frame.
        let max = unsafe { sys::swb_resampler_max_out(self.raw.as_ptr(), f) };
        let max = usize::try_from(max).map_err(|_| Error::new(max, "resample"))?;
        if max == 0 {
            return Ok(0);
        }
        // One frame never needs more (a frame of 1 Hz audio is the extreme
        // that `swr_get_out_samples` could size to a huge count).
        if max > MAX_SAMPLES_PER_CALL {
            return Err(Error::local("too many samples"));
        }
        let room = max
            .checked_mul(self.channels)
            .ok_or_else(|| Error::local("too many samples"))?;
        let start = out.len();
        out.resize(start + room, 0.0);
        let cap = c_int::try_from(max).unwrap_or(c_int::MAX);
        let dst = out.get_mut(start..).ok_or_else(|| Error::local("buffer"))?;
        // SAFETY: `raw` is live; `f` is null or a live frame; `dst` holds
        // `max * channels` floats, room for `cap` samples per channel.
        let r = unsafe { sys::swb_resampler_f32(self.raw.as_ptr(), f, dst.as_mut_ptr(), cap) };
        let written = usize::try_from(r).map_err(|_| Error::new(r, "resample"));
        let written = match written {
            Ok(n) if n <= max => n,
            Ok(_) => Err(Error::local("the resampler wrote too much"))?,
            Err(e) => {
                out.truncate(start);
                return Err(e);
            }
        };
        out.truncate(start + written * self.channels);
        Ok(written)
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        // SAFETY: `raw` is live and owned by this value.
        unsafe { sys::swb_resampler_free(self.raw.as_ptr()) };
    }
}
