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
//! `auto` (the size of the laid-out image, for lazy-loaded images) is not
//! supported: an `auto` entry without a media condition ends the list with
//! the default of `100vw`. This is what Chromium 148 uses when it selects
//! the source of an image that has no layout yet. The specification skips
//! `auto` and continues with the next entry. Chromium then uses the
//! laid-out width for the density, and its user-agent sheet gives images
//! whose `sizes` starts with `auto` `contain: size` and an intrinsic size
//! of 300x150; swb does neither.

use crate::media::{MediaCondition, MediaEnvironment, length_px};
use crate::values::{ComponentValue, split_on_commas, trim_whitespace};

/// The source size in CSS px that the `sizes` attribute value `sizes`
/// gives in `env`: the size of the first entry whose media condition
/// matches, or `100vw`. `None` (no attribute) also gives `100vw`.
pub fn source_size(sizes: Option<&str>, env: &MediaEnvironment) -> f32 {
    let default = env.viewport_width;
    let Some(sizes) = sizes else {
        return default;
    };
    let values = crate::parse_component_values(sizes);
    for entry in split_on_commas(&values) {
        let entry = trim_whitespace(entry);
        let Some((last, condition)) = entry.split_last() else {
            continue;
        };
        let condition = trim_whitespace(condition);
        if is_auto(last) {
            if condition.is_empty() {
                return default;
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
            assert_eq!(source_size(Some(sizes), &env()), expected, "{sizes:?}");
        }
        assert_eq!(source_size(None, &env()), 800.0);
    }

    #[test]
    fn hostile_values_stay_finite() {
        for sizes in [
            "calc(1e38px * 1e38)",
            "1e39px",
            "calc(-1e38px * 1e38)",
            "calc(1px / 0)",
        ] {
            let size = source_size(Some(sizes), &env());
            assert!(size.is_finite() && size >= 0.0, "{sizes:?}: {size}");
        }
        for sizes in ["-0px", "calc(0px * -1)", "-0"] {
            let size = source_size(Some(sizes), &env());
            assert!(size == 0.0 && size.is_sign_positive(), "{sizes:?}: {size}");
        }
    }
}
