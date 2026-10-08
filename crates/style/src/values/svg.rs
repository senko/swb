//! Computed values of the SVG paint properties `fill` and `stroke`.
//!
//! <https://svgwg.org/svg2-draft/painting.html#SpecifyingPaint>

use std::sync::Arc;

use super::color::{Color, Rgba};

/// A computed `<paint>`: `none`, a color (which can be `currentColor`,
/// resolved against the element that paints) or a paint server reference.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgPaint {
    /// `none`.
    None,
    /// A color.
    Color(Color),
    /// `url(...)` with an optional fallback. swb has no paint servers yet,
    /// so the fallback paints; without one the paint is `none`, as Chromium
    /// does for a reference to a missing element.
    Url {
        /// The reference as written.
        url: Arc<str>,
        /// The fallback color; `None` for no fallback or `none`.
        fallback: Option<Color>,
    },
}

/// A computed `clip-path`: `none` or a reference to a `clipPath` element.
/// Basic shapes and the other `<clip-source>` forms are not supported
/// (the declaration is invalid).
/// <https://drafts.fxtf.org/css-masking/#the-clip-path>
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ClipPath {
    /// `none`.
    #[default]
    None,
    /// `url(...)`, as written (trimmed).
    Url(Arc<str>),
}

impl SvgPaint {
    /// The initial value of `fill`: black.
    pub const BLACK: SvgPaint = SvgPaint::Color(Color::Rgba(Rgba::BLACK));

    /// The color that this paint uses, given the painting element's
    /// `color`, or `None` if it paints nothing.
    pub fn used_color(&self, current_color: Rgba) -> Option<Rgba> {
        match self {
            SvgPaint::None => None,
            SvgPaint::Color(c) => Some(c.resolve(current_color)),
            SvgPaint::Url { fallback, .. } => fallback.map(|c| c.resolve(current_color)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_colors() {
        let red = Rgba::rgb(255, 0, 0);
        assert_eq!(SvgPaint::BLACK.used_color(red), Some(Rgba::BLACK));
        assert_eq!(
            SvgPaint::Color(Color::CurrentColor).used_color(red),
            Some(red)
        );
        assert_eq!(SvgPaint::None.used_color(red), None);
        let url = |fallback| SvgPaint::Url {
            url: Arc::from("#a"),
            fallback,
        };
        assert_eq!(url(None).used_color(red), None);
        assert_eq!(url(Some(Color::CurrentColor)).used_color(red), Some(red));
    }
}
