//! The `sizes` attribute of `<img>` and `<source>`: a list of media
//! conditions with source sizes, which gives the width that an image is
//! expected to have. `w` descriptors in `srcset` are divided by it.
//!
//! <https://html.spec.whatwg.org/multipage/images.html#parse-a-sizes-attribute>
//!
//! The source size values resolve relative units as media queries do,
//! also inside math functions: `em` and `rem` against the initial font
//! size, `vw` and `vh` against the viewport. (In the media conditions,
//! math functions with relative units stay "unknown", as in stylesheets.)
//! A negative length is invalid, but the result of a math function is
//! clamped to 0 (CSS Values 4 range checking; Chromium 148 does the same).
//!
//! `auto` is the laid-out width of the image, for lazy-loaded images
//! ([`allows_auto`]). Without a width (the image has no layout yet, or the
//! caller does not know it), an `auto` entry without a media condition
//! ends the list with the default of `100vw`. This is what Chromium 148
//! uses when it selects the source of an image that has no layout yet and
//! for images that are not lazy-loaded; the specification skips `auto` and
//! continues with the next entry (measured: `auto, 90px` gives 800 px, as
//! `90px, auto` gives 90 px). `auto` counts only if the value is `auto` or
//! starts with `auto,`, optionally with white space around it; otherwise
//! (`auto ,90px`, `90px, auto`) it is ignored. (Chromium 148 gives a
//! timing-dependent result for the forms with white space: the image has
//! no size containment, so its size depends on its source.) An `auto`
//! entry with a media condition is invalid.

use crate::media::{MediaCondition, MediaEnvironment, length_px};
use crate::values::{ComponentValue, split_on_commas, trim_whitespace};

/// The source size in CSS px that the `sizes` attribute value `sizes`
/// gives in `env`: the size of the first entry whose media condition
/// matches, or `100vw`. `None` (no attribute) also gives `100vw`.
/// `auto_width` is the width of the image, in CSS px, if it is known and
/// the image allows auto-sizes: an `auto` entry gives it.
pub fn source_size(sizes: Option<&str>, env: &MediaEnvironment, auto_width: Option<f32>) -> f32 {
    let default = env.viewport_width;
    let Some(sizes) = sizes else {
        return default;
    };
    let auto_allowed = allows_auto(sizes.trim_matches(|c: char| c.is_ascii_whitespace()));
    let values = crate::parse_component_values(sizes);
    for entry in split_on_commas(&values) {
        let entry = trim_whitespace(entry);
        let Some((last, condition)) = entry.split_last() else {
            continue;
        };
        let condition = trim_whitespace(condition);
        if is_auto(last) {
            if condition.is_empty() && auto_allowed {
                // NaN becomes 0; a huge width stays finite.
                return auto_width.map_or(default, |w| {
                    if w.is_nan() {
                        0.0
                    } else {
                        w.clamp(0.0, f32::MAX)
                    }
                });
            }
            continue;
        }
        let Some(size) = source_size_value(last, env) else {
            continue;
        };
        if condition.is_empty() || MediaCondition::parse(condition).is_some_and(|c| c.matches(env))
        {
            return size;
        }
    }
    default
}

/// True if `sizes` is `auto` or starts with `auto,` (ASCII
/// case-insensitive; no white space is ignored). An `img` allows
/// auto-sizes if it is lazy-loaded and its `sizes` has this form: the form
/// of the user-agent rule `img:is([sizes="auto" i], [sizes^="auto," i])`.
/// (With white space around the value, [`source_size`] still reads `auto`
/// as `100vw`, but the image has no size containment and swb does not
/// wait for its width.)
/// <https://html.spec.whatwg.org/multipage/embedded-content.html#attr-img-sizes>
pub fn allows_auto(sizes: &str) -> bool {
    let Some(head) = sizes.get(..4) else {
        return false;
    };
    head.eq_ignore_ascii_case("auto") && matches!(sizes.as_bytes().get(4), None | Some(b','))
}

/// A valid non-negative `<source-size-value>` other than `auto`, in px.
fn source_size_value(value: &ComponentValue, env: &MediaEnvironment) -> Option<f32> {
    let px = length_px(value, env)?;
    if px.is_nan() {
        return None;
    }
    // `abs` turns -0 into 0: a negative zero would give negative densities.
    if matches!(value, ComponentValue::Function(_)) {
        // The result of a math function is clamped to the allowed range.
        return Some(px.clamp(0.0, f32::MAX).abs());
    }
    (px >= 0.0).then(|| px.min(f32::MAX).abs())
}

fn is_auto(value: &ComponentValue) -> bool {
    matches!(value, ComponentValue::Ident(name) if name.eq_ignore_ascii_case("auto"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: 800.0,
            viewport_height: 600.0,
            ..MediaEnvironment::default()
        }
    }

    /// `sizes` values and their source sizes at 800x600, measured with
    /// Chromium 148 (natural width of a `w` candidate).
    const CASES: &[(&str, f32)] = &[
        ("150px", 150.0),
        ("(min-width: 700px) 100px, 300px", 100.0),
        ("(max-width: 700px) 100px, 300px", 300.0),
        ("calc(10vw + 2em)", 112.0),
        ("(min-width: 100px) 75px", 75.0),
        ("bogus, 60px", 60.0),
        ("(min-width: 100px) -5px, 120px", 120.0),
        ("50%, 130px", 130.0),
        ("0", 0.0),
        ("calc(50px - 100px), 70px", 0.0),
        ("max(10px, 3em)", 48.0),
        ("(width >= 500px) 40px", 40.0),
        ("(min-width: 100px) 75px 10px, 110px", 110.0),
        ("10vh", 60.0),
        ("1em", 16.0),
        ("10rem", 160.0),
        ("1in", 96.0),
        ("(min-width: 10px) 40px 50px, 70px", 70.0),
        ("  ", 800.0),
        ("", 800.0),
        ("(min-width: 10px), 70px", 70.0),
        ("screen 40px, 70px", 70.0),
        ("not (min-width: 10px) 40px, 70px", 70.0),
        (
            "(min-width: 10px) and (max-width: 20px) 40px, (orientation: landscape) or (hover) 75px",
            75.0,
        ),
        ("(unknown: 1) 40px, 70px", 70.0),
        ("40px,", 40.0),
        ("calc(100vw / 8)", 100.0),
        ("foo(40px), 70px", 70.0),
        ("40px 50px", 800.0),
        ("calc(40px + 10%), 66px", 66.0),
        ("auto, 90px", 800.0),
        ("AUTO, 90px", 800.0),
        ("90px, auto", 90.0),
        ("(min-width: 10px) auto, 90px", 90.0),
    ];

    #[test]
    fn source_sizes_follow_chromium() {
        for &(sizes, expected) in CASES {
            assert_eq!(
                source_size(Some(sizes), &env(), None),
                expected,
                "{sizes:?}"
            );
        }
        assert_eq!(source_size(None, &env(), None), 800.0);
    }

    #[test]
    fn auto_uses_the_width_of_the_image() {
        let size = |sizes, width| source_size(Some(sizes), &env(), width);
        assert_eq!(size("auto", Some(384.0)), 384.0);
        assert_eq!(
            size("auto, (max-width: 384px) 100vw, 384px", Some(200.0)),
            200.0
        );
        assert_eq!(size("AUTO, 90px", Some(0.0)), 0.0);
        assert_eq!(size("auto, 90px", None), 800.0);
        assert_eq!(size("90px, auto", Some(384.0)), 90.0);
        // White space around the value is accepted; a space before the
        // comma is not.
        assert_eq!(size(" auto, 90px ", None), 800.0);
        assert_eq!(size("auto ,90px", Some(384.0)), 90.0);
        assert_eq!(size("(min-width: 1px) 20px, auto", Some(384.0)), 20.0);
        assert_eq!(
            size("(min-width: 1e5px) 20px, auto, 50px", Some(384.0)),
            50.0
        );
        // An `auto` entry with a condition is invalid.
        assert_eq!(size("(min-width: 10px) auto, 90px", Some(384.0)), 90.0);
        assert_eq!(size("auto", Some(f32::INFINITY)), f32::MAX);
        assert_eq!(size("auto", Some(f32::NAN)), 0.0);
    }

    #[test]
    fn allows_auto_forms() {
        for yes in ["auto", "AUTO", "auto, 10px", "Auto,10px", "auto,"] {
            assert!(allows_auto(yes), "{yes:?}");
        }
        for no in [
            "",
            " auto",
            "auto ",
            "10px",
            "auto ,10px",
            "auto 10px",
            "autox",
            "10px, auto",
            "aut",
            "аuto",
        ] {
            assert!(!allows_auto(no), "{no:?}");
        }
    }

    #[test]
    fn hostile_values_stay_finite() {
        for sizes in [
            "calc(1e38px * 1e38)",
            "1e39px",
            "calc(-1e38px * 1e38)",
            "calc(1px / 0)",
        ] {
            let size = source_size(Some(sizes), &env(), None);
            assert!(size.is_finite() && size >= 0.0, "{sizes:?}: {size}");
        }
        for sizes in ["-0px", "calc(0px * -1)", "-0"] {
            let size = source_size(Some(sizes), &env(), None);
            assert!(size == 0.0 && size.is_sign_positive(), "{sizes:?}: {size}");
        }
    }
}
