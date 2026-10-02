//! Glyph rasterization into anti-aliased coverage masks.
//!
//! Outlines are unhinted. Synthetic styles follow Skia, which Chromium uses:
//!
//! - Synthetic bold strokes the outline and fills both (`useStrokeForFakeBold`
//!   in `SkScalerContext.cpp`). The stroke width is the font size times
//!   1/24 at 9 px and below, 1/32 at 36 px and above, linear in between.
//! - Synthetic oblique shears the outline by -0.25 in Skia's y-down space
//!   (Blink `FontPlatformData::SetupSkFont`: `setSkewX(-1/4)`).

use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{MetadataProvider, color::ColorGlyphFormat};
use tiny_skia::{FillRule, LineJoin, Mask, Path, PathBuilder, Stroke, Transform};

use crate::GlyphId;
use crate::context::Synthesis;
use crate::face::LoadedFace;

/// Largest mask width or height in pixels. Larger glyphs are not
/// rasterized.
const MAX_MASK_SIDE: u32 = 4096;

/// An 8-bit coverage mask of one glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphMask {
    /// Horizontal offset from the pen position to the left edge of the mask.
    pub left: i32,
    /// Vertical offset from the baseline to the top edge of the mask (y
    /// down, so usually negative).
    pub top: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Coverage values, row by row, `width * height` bytes.
    pub data: Vec<u8>,
}

/// The glyph is a color glyph, which is not supported yet.
#[derive(Debug)]
pub(crate) struct ColorGlyph;

/// Rasterizes a glyph. `Ok(None)` means the glyph has nothing to draw.
pub(crate) fn rasterize(
    face: &LoadedFace,
    coords: &[NormalizedCoord],
    glyph: GlyphId,
    size: f32,
    subpixel: u8,
    synthesis: Synthesis,
) -> Result<Option<GlyphMask>, ColorGlyph> {
    let Some(font) = face.font() else {
        return Ok(None);
    };
    let glyph_id = skrifa::GlyphId::new(glyph.0);
    if face.has_color
        && font
            .color_glyphs()
            .get_with_format(glyph_id, ColorGlyphFormat::ColrV1)
            .or_else(|| {
                font.color_glyphs()
                    .get_with_format(glyph_id, ColorGlyphFormat::ColrV0)
            })
            .is_some()
    {
        return Err(ColorGlyph);
    }
    let Some(outline) = font.outline_glyphs().get(glyph_id) else {
        // Bitmap-only fonts (CBDT, sbix) have no outlines.
        return if face.has_color {
            Err(ColorGlyph)
        } else {
            Ok(None)
        };
    };
    if size <= 0.0 {
        return Ok(None);
    }
    let mut pen = PathPen(PathBuilder::new());
    let settings = DrawSettings::unhinted(Size::new(size), LocationRef::new(coords));
    if outline.draw(settings, &mut pen).is_err() {
        return Ok(None);
    }
    let Some(path) = pen.0.finish() else {
        return Ok(None);
    };
    Ok(fill(path, size, subpixel, synthesis))
}

/// Applies synthesis and the subpixel offset and fills the path.
fn fill(path: Path, size: f32, subpixel: u8, synthesis: Synthesis) -> Option<GlyphMask> {
    let stroke = if synthesis.bold {
        let stroke = Stroke {
            width: size * fake_bold_scale(size),
            line_join: LineJoin::Miter,
            ..Stroke::default()
        };
        path.stroke(&stroke, 1.0)
    } else {
        None
    };
    let skew = if synthesis.oblique { -0.25 } else { 0.0 };
    let transform = Transform::from_row(1.0, 0.0, skew, 1.0, f32::from(subpixel) / 4.0, 0.0);
    let path = path.transform(transform)?;
    let stroke = stroke.and_then(|s| s.transform(transform));

    let mut bounds = path.bounds();
    if let Some(stroke) = &stroke {
        bounds = bounds.join(&stroke.bounds())?;
    }
    let left = bounds.left().floor();
    let top = bounds.top().floor();
    let width = (bounds.right().ceil() - left) as u32;
    let height = (bounds.bottom().ceil() - top) as u32;
    if width == 0 || height == 0 || width > MAX_MASK_SIDE || height > MAX_MASK_SIDE {
        return None;
    }
    // Thin horizontal or vertical shapes have zero-size bounds in one
    // direction; tiny-skia cannot fill them.
    if bounds.width() <= f32::EPSILON || bounds.height() <= f32::EPSILON {
        return None;
    }
    let mut mask = Mask::new(width, height)?;
    let to_mask = Transform::from_translate(-left, -top);
    mask.fill_path(&path, FillRule::Winding, true, to_mask);
    if let Some(stroke) = &stroke {
        mask.fill_path(stroke, FillRule::Winding, true, to_mask);
    }
    Some(GlyphMask {
        left: left as i32,
        top: top as i32,
        width,
        height,
        data: mask.take(),
    })
}

/// Skia's fake bold stroke width as a fraction of the font size.
fn fake_bold_scale(size: f32) -> f32 {
    const SMALL: (f32, f32) = (9.0, 1.0 / 24.0);
    const LARGE: (f32, f32) = (36.0, 1.0 / 32.0);
    if size <= SMALL.0 {
        SMALL.1
    } else if size >= LARGE.0 {
        LARGE.1
    } else {
        let t = (size - SMALL.0) / (LARGE.0 - SMALL.0);
        SMALL.1 + t * (LARGE.1 - SMALL.1)
    }
}

/// Builds a tiny-skia path with y pointing down.
struct PathPen(PathBuilder);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(cx0, -cy0, x, -y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(cx0, -cy0, cx1, -cy1, x, -y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_bold_scale_interpolates() {
        assert_eq!(fake_bold_scale(8.0), 1.0 / 24.0);
        assert_eq!(fake_bold_scale(40.0), 1.0 / 32.0);
        let mid = fake_bold_scale(22.5);
        assert!(mid < 1.0 / 24.0 && mid > 1.0 / 32.0);
    }

    #[test]
    fn square_fills_mask() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, -4.0);
        builder.line_to(4.0, -4.0);
        builder.line_to(4.0, 0.0);
        builder.line_to(0.0, 0.0);
        builder.close();
        let path = builder.finish().unwrap();
        let mask = fill(path, 10.0, 0, Synthesis::default()).unwrap();
        assert_eq!(
            (mask.left, mask.top, mask.width, mask.height),
            (0, -4, 4, 4)
        );
        assert!(mask.data.iter().all(|&a| a == 255));
    }
}
