//! `swb --version` and `swb --licenses` (ADR 0047 section 2): the binary
//! tells which third-party components it contains and carries their
//! notices and license texts.

use std::sync::OnceLock;

/// swb's own license.
const LICENSE: &str = include_str!("../../../LICENSE");
/// The notices of the third-party material (`THIRD_PARTY_NOTICES.md`).
const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");

/// The text of `--version`: swb's version, then one line per media
/// component of the build (FFmpeg's version and configure line), or a line
/// that says the build has no media support.
pub(crate) fn long_version() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = env!("CARGO_PKG_VERSION").to_owned();
        let lines = swb_media::version_lines();
        if lines.is_empty() {
            text.push_str("\nmedia: not in this build (built without the `media` feature)");
        }
        for line in lines {
            text.push('\n');
            text.push_str(&line);
        }
        text
    })
}

/// The text of `--licenses`: swb's license, the third-party notices and
/// the license texts of the media components of the build.
pub(crate) fn licenses() -> String {
    let mut text = String::new();
    text.push_str("swb\n===\n\n");
    text.push_str(LICENSE);
    text.push_str("\n\n");
    if !swb_media::ENABLED {
        text.push_str(
            "This build has no media support (no `media` feature): it contains \
             no FFmpeg and no PipeWire code, and their sections below do not \
             apply to it.\n\n",
        );
    }
    text.push_str(NOTICES);
    for (title, license) in swb_media::license_texts() {
        text.push_str("\n\n");
        text.push_str(title);
        text.push('\n');
        text.push_str(&"=".repeat(title.chars().count()));
        text.push_str("\n\n");
        text.push_str(license);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_names_the_media_components() {
        let v = long_version();
        assert!(v.starts_with(env!("CARGO_PKG_VERSION")));
        if swb_media::ENABLED {
            assert!(v.contains("FFmpeg "), "{v}");
            assert!(v.contains("LGPL-2.1-or-later"), "{v}");
        } else {
            assert!(v.contains("media: not in this build"), "{v}");
        }
    }

    #[test]
    fn licenses_carry_the_notices() {
        let l = licenses();
        assert!(l.contains("MIT License"));
        assert!(l.contains("# Third-party notices"));
        assert!(l.contains("FFmpeg"));
        assert_eq!(
            l.contains("GNU LESSER GENERAL PUBLIC LICENSE"),
            swb_media::ENABLED
        );
    }
}
