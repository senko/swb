//! The computed value of `filter` (Filter Effects 1 §5,
//! <https://drafts.fxtf.org/filter-effects-1/#FilterProperty>).
//!
//! The functions are kept in order. The amounts are numbers (a percentage
//! is divided by 100), `hue-rotate()` is in degrees and `blur()` and
//! `drop-shadow()` have lengths in px. The ranges are those that Chromium
//! 148 applies when it computes the value (measured,
//! `tools/probes/filter.json`): `grayscale()`, `invert()`, `opacity()` and
//! `sepia()` are at most 1 (also when they are parsed), `brightness()`,
//! `contrast()` and `saturate()` are not limited above, a `blur()` is at
//! most 2^25 px.

use std::sync::Arc;

use super::color::Color;

/// The largest `blur()` or `drop-shadow()` blur radius that is computed, in
/// px (Chromium: 2^25).
pub const MAX_FILTER_BLUR: f32 = 33_554_432.0;

/// The most functions in one `filter` value. A longer value is invalid
/// (Chromium has no limit; ADR 0028 section 16).
pub const MAX_FILTER_FUNCTIONS: usize = 64;

/// One function of a computed `filter` value.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterFunction {
    /// `blur(<length>)`: the standard deviation, in px.
    Blur(f32),
    /// `brightness(<number>)`.
    Brightness(f32),
    /// `contrast(<number>)`.
    Contrast(f32),
    /// `grayscale(<number>)`, 0 to 1.
    Grayscale(f32),
    /// `hue-rotate(<angle>)`, in degrees.
    HueRotate(f32),
    /// `invert(<number>)`, 0 to 1.
    Invert(f32),
    /// `opacity(<number>)`, 0 to 1.
    Opacity(f32),
    /// `saturate(<number>)`.
    Saturate(f32),
    /// `sepia(<number>)`, 0 to 1.
    Sepia(f32),
    /// `drop-shadow(<color>? <length>{2,3})`.
    DropShadow(DropShadow),
    /// `url(...)`: a reference to an SVG `filter` element, as written.
    /// swb does not paint it (the element is drawn as without the
    /// function), but it makes the element a stacking context and a
    /// containing block, as in Chromium.
    Url(Arc<str>),
}

/// A computed `drop-shadow()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropShadow {
    /// The color; `currentColor` is the element's `color`.
    pub color: Color,
    /// The horizontal offset in px.
    pub x: f32,
    /// The vertical offset in px.
    pub y: f32,
    /// The standard deviation of the blur in px.
    pub blur: f32,
}

/// A computed `filter`: `none` (no functions) or a list of functions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter(Option<Arc<[FilterFunction]>>);

impl Filter {
    /// The value `none`.
    pub const NONE: Filter = Filter(None);

    /// A value of the functions in `list` (`none` if it is empty).
    pub fn new(list: Vec<FilterFunction>) -> Filter {
        if list.is_empty() {
            Filter(None)
        } else {
            Filter(Some(Arc::from(list)))
        }
    }

    /// True for `none`.
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// The functions, in order (empty for `none`).
    pub fn functions(&self) -> &[FilterFunction] {
        self.0.as_deref().unwrap_or(&[])
    }

    /// The shared list, for display items.
    pub fn list(&self) -> Option<&Arc<[FilterFunction]>> {
        self.0.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_is_none() {
        assert!(Filter::new(Vec::new()).is_none());
        assert_eq!(Filter::new(Vec::new()), Filter::NONE);
        let f = Filter::new(vec![FilterFunction::Blur(2.0)]);
        assert!(!f.is_none());
        assert_eq!(f.functions(), &[FilterFunction::Blur(2.0)]);
        assert_eq!(Filter::NONE.functions(), []);
    }
}
