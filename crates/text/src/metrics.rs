//! Font-wide metrics in pixels.
//!
//! The values follow Skia's `FreeType` port
//! (`SkScalerContext_FreeType::generateFontMetrics`), which Chromium uses on
//! Linux:
//!
//! - ascent, descent and line gap: the OS/2 typographic values if
//!   `fsSelection` has `USE_TYPO_METRICS`; otherwise the `hhea` values; if
//!   both `hhea` ascender and descender are zero, the OS/2 typographic values,
//!   then the OS/2 Windows values. (`FreeType` itself ignores
//!   `USE_TYPO_METRICS` in older versions, but Skia checks the flag.)
//! - x-height and cap height: OS/2 `sxHeight` and `sCapHeight` (version 2 and
//!   later) if not zero, otherwise measured from the outlines of `x` and `H`.
//!   Without both, the x-height is 0.56 times the ascent, as in Blink
//!   (`SimpleFontData::PlatformInit`), and the cap height is the ascent.
//! - underline: the `post` table; strikeout: the OS/2 table.
//!
//! All values are unrounded. Blink rounds ascent and descent to integers
//! for line layout on Linux; that is the caller's decision.

use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::DrawSettings;
use skrifa::outline::pen::ControlBoundsPen;
use skrifa::{FontRef, MetadataProvider};

use crate::face::LoadedFace;

/// Font metrics in pixels for one font size. Vertical offsets use y down.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontMetrics {
    /// Distance from the baseline up to the top of the line box area
    /// (positive).
    pub ascent: f32,
    /// Distance from the baseline down to the bottom (positive).
    pub descent: f32,
    /// Recommended extra space between lines.
    pub line_gap: f32,
    /// Height of lowercase `x`. Falls back to 0.56 times the ascent.
    pub x_height: f32,
    /// Height of uppercase `H`. Falls back to the ascent.
    pub cap_height: f32,
    /// Offset from the baseline to the top of the underline; positive is
    /// below the baseline.
    pub underline_offset: f32,
    /// Thickness of the underline.
    pub underline_thickness: f32,
    /// Offset from the baseline to the top of the strikeout line; negative
    /// is above the baseline.
    pub strikeout_offset: f32,
    /// Thickness of the strikeout line.
    pub strikeout_thickness: f32,
    /// Font design units per em.
    pub units_per_em: u16,
}

impl FontMetrics {
    /// Metrics for the placeholder font that is used when no fonts exist.
    pub(crate) fn placeholder(size: f32) -> Self {
        let thickness = size / 14.0;
        FontMetrics {
            ascent: size * 0.8,
            descent: size * 0.2,
            line_gap: 0.0,
            x_height: size * 0.5,
            cap_height: size * 0.7,
            underline_offset: size * 0.1,
            underline_thickness: thickness,
            strikeout_offset: -(size * 0.25 + thickness / 2.0),
            strikeout_thickness: thickness,
            units_per_em: 1000,
        }
    }
}

/// Computes the metrics of a face at `size` pixels.
pub(crate) fn compute(face: &LoadedFace, coords: &[NormalizedCoord], size: f32) -> FontMetrics {
    let Some(font) = face.font() else {
        return FontMetrics::placeholder(size);
    };
    let location = LocationRef::new(coords);
    let m = font.metrics(Size::new(size), location);
    let ascent = m.ascent;
    let x_height = x_height(
        m.x_height,
        || measure_height(face, &font, 'x', size, coords),
        ascent,
    );
    let cap_height = m
        .cap_height
        .filter(|h| *h > 0.0)
        .or_else(|| measure_height(face, &font, 'H', size, coords))
        .unwrap_or(ascent);
    let default_thickness = size / 14.0;
    let (underline_offset, underline_thickness) = match m.underline {
        Some(u) if u.thickness > 0.0 => (-u.offset, u.thickness),
        Some(u) => (-u.offset, default_thickness),
        None => (size * 0.1, default_thickness),
    };
    let (strikeout_offset, strikeout_thickness) = match m.strikeout {
        Some(s) if s.thickness > 0.0 => (-s.offset, s.thickness),
        _ => (
            -(x_height / 2.0 + underline_thickness / 2.0),
            underline_thickness,
        ),
    };
    FontMetrics {
        ascent,
        descent: -m.descent,
        line_gap: m.leading,
        x_height,
        cap_height,
        underline_offset,
        underline_thickness,
        strikeout_offset,
        strikeout_thickness,
        units_per_em: face.units_per_em,
    }
}

/// The x-height: the OS/2 value if it is positive, else the measured
/// height of `x`, else Blink's approximation of 0.56 times the ascent.
fn x_height(os2: Option<f32>, measure: impl FnOnce() -> Option<f32>, ascent: f32) -> f32 {
    os2.filter(|h| *h > 0.0)
        .or_else(measure)
        .unwrap_or(ascent * 0.56)
}

/// The top of the outline of the glyph for `c`, in pixels above the
/// baseline.
fn measure_height(
    face: &LoadedFace,
    font: &FontRef<'_>,
    c: char,
    size: f32,
    coords: &[NormalizedCoord],
) -> Option<f32> {
    let glyph = face.glyph(c)?;
    let outline = font.outline_glyphs().get(glyph)?;
    let mut pen = ControlBoundsPen::default();
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::new(coords)),
            &mut pen,
        )
        .ok()?;
    pen.bounding_box().map(|b| b.y_max).filter(|h| *h > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_height_sources_in_order() {
        // The OS/2 value wins; the outline is not measured.
        let measured = || -> Option<f32> { panic!("not measured") };
        assert_eq!(x_height(Some(5.0), measured, 10.0), 5.0);
        // A zero OS/2 value means "not set".
        assert_eq!(x_height(Some(0.0), || Some(4.0), 10.0), 4.0);
        assert_eq!(x_height(None, || Some(4.0), 10.0), 4.0);
        // Neither: Blink's 0.56 * ascent.
        assert_eq!(x_height(None, || None, 10.0), 10.0 * 0.56);
    }
}
