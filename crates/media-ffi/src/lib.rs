//! The C boundary of swb's media playback (ADR 0047 section 4): the only
//! crate with `unsafe` code and with C code.
//!
//! With the feature `ffmpeg`, it compiles `csrc/ffmpeg.c` (a narrow C API
//! over FFmpeg's public API) and links the static FFmpeg build of
//! `just ffmpeg`, and the module `ffmpeg` gives safe types over it: a
//! demuxer that reads from a Rust byte source, packets, decoders, frames,
//! the conversion to RGBA and the resampler. Without the feature the crate
//! is empty and its build script does nothing, so the default build needs
//! no C library. (The feature `pipewire` of ADR 0047 section 3 comes with
//! the PipeWire output, M14 feature 8.)
//!
//! Rules (ADR 0047 section 4): every `unsafe` block has a `SAFETY` comment
//! and one unsafe operation; each C handle is owned by one Rust value whose
//! `Drop` frees it; no raw pointer or handle is public; callbacks catch
//! panics. The crate's lint tables are a copy of the workspace's with
//! `unsafe` allowed and four stricter lints; `lint_tables` tests the copy.

#[cfg(feature = "ffmpeg")]
pub mod ffmpeg;

#[cfg(test)]
mod lint_tables;
