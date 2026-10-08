//! Error type of the text crate.

use std::path::PathBuf;

/// Errors from font loading.
#[derive(Debug, thiserror::Error)]
pub enum TextError {
    /// A font file or directory could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A font file could not be parsed.
    #[error("invalid font {path} (face {index}): {reason}")]
    InvalidFont {
        /// The font file.
        path: PathBuf,
        /// The face index in the file.
        index: u32,
        /// What is wrong.
        reason: String,
    },
    /// No installed font has the name of a `local()` source.
    #[error("no installed font named {0:?}")]
    NoLocalFont(String),
    /// A font directory contains no usable fonts.
    #[error("no usable fonts in {0}")]
    NoFonts(PathBuf),
}
