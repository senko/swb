//! The license of the linked FFmpeg (ADR 0047 section 2): every library is
//! "LGPL version 2.1 or later", and the configure line is the pinned one,
//! without GPL or non-free parts.
#![cfg(feature = "ffmpeg")]

use swb_media_ffi::ffmpeg::{self, Library, pinned};

#[test]
fn every_library_is_lgpl_2_1_or_later() {
    for library in Library::ALL {
        assert_eq!(
            ffmpeg::license(library),
            "LGPL version 2.1 or later",
            "{}",
            library.name()
        );
    }
}

#[test]
fn the_configure_line_is_the_pinned_one() {
    let configuration = ffmpeg::configuration();
    assert!(!configuration.contains("--enable-gpl"), "{configuration}");
    assert!(
        !configuration.contains("--enable-nonfree"),
        "{configuration}"
    );
    assert!(
        !configuration.contains("--enable-version3"),
        "{configuration}"
    );
    // `--prefix` names the install directory, which differs per checkout;
    // configure quotes the values with commas (`--enable-demuxer='mov,...'`).
    let options: Vec<String> = configuration
        .split_whitespace()
        .filter(|o| !o.starts_with("--prefix="))
        .map(|o| o.replace('\'', ""))
        .collect();
    assert_eq!(options.join(" "), pinned::CONFIGURE);
}

#[test]
fn the_version_is_the_pinned_one() {
    assert_eq!(ffmpeg::version(), pinned::VERSION);
    assert_eq!(pinned::SHA256.len(), 64);
}
