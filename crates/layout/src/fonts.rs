//! Bridges computed font properties to the `text` crate, and computes the
//! font metrics that line layout uses.

use swb_style::{ComputedStyle, FontFamily, GenericFamily};
use swb_text::{FamilyName, FontContext, FontId, FontQuery, FontStyle as TextFontStyle};

use crate::fragment::PositionedGlyph;

/// Font metrics as line layout uses them, in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LineMetrics {
    /// Distance from the baseline to the top of the content area.
    pub(crate) ascent: f32,
    /// Distance from the baseline to the bottom of the content area.
    pub(crate) descent: f32,
    /// The font's line gap.
    pub(crate) line_gap: f32,
    /// The x-height.
    pub(crate) x_height: f32,
}

impl LineMetrics {
    /// The height of `line-height: normal`.
    pub(crate) fn normal_line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

/// Converts a computed `font-family` list to `text` crate family names.
pub(crate) fn family_names(families: &[FontFamily]) -> Vec<FamilyName<'_>> {
    families
        .iter()
        .map(|f| match f {
            FontFamily::Named(name) => FamilyName::Named(name),
            FontFamily::Generic(g) => FamilyName::Generic(generic(*g)),
        })
        .collect()
}

fn generic(g: GenericFamily) -> swb_text::GenericFamily {
    match g {
        GenericFamily::Serif => swb_text::GenericFamily::Serif,
        GenericFamily::SansSerif => swb_text::GenericFamily::SansSerif,
        GenericFamily::Monospace => swb_text::GenericFamily::Monospace,
        GenericFamily::Cursive => swb_text::GenericFamily::Cursive,
        GenericFamily::Fantasy => swb_text::GenericFamily::Fantasy,
        GenericFamily::SystemUi => swb_text::GenericFamily::SystemUi,
        GenericFamily::Math => swb_text::GenericFamily::Math,
        GenericFamily::Emoji => swb_text::GenericFamily::Emoji,
    }
}

/// Builds a font query for a style. `families` must come from
/// [`family_names`] for the same style.
pub(crate) fn query<'a>(style: &'a ComputedStyle, families: &'a [FamilyName<'a>]) -> FontQuery<'a> {
    FontQuery {
        families,
        weight: style.font_weight,
        style: match style.font_style {
            swb_style::FontStyle::Normal => TextFontStyle::Normal,
            swb_style::FontStyle::Italic => TextFontStyle::Italic,
            swb_style::FontStyle::Oblique => TextFontStyle::Oblique,
        },
        stretch: style.font_stretch,
        variations: &style.font_variation_settings,
        language: None,
    }
}

/// The primary font of a style (the first available family).
pub(crate) fn primary_font(fonts: &mut FontContext, style: &ComputedStyle) -> FontId {
    let families = family_names(&style.font_family);
    fonts.select(&query(style, &families))
}

/// The glyphs of a shaped run, positioned relative to the pen position at
/// the start of the run on the baseline.
pub(crate) fn positioned_glyphs(run: &swb_text::ShapedRun) -> Vec<PositionedGlyph> {
    let mut pen = 0.0;
    run.glyphs
        .iter()
        .map(|g| {
            let p = PositionedGlyph {
                id: g.glyph,
                x: pen + g.x_offset,
                y: -g.y_offset,
            };
            pen += g.x_advance;
            p
        })
        .collect()
}

/// Metrics of `font` at `size`, rounded the way Blink rounds them on Linux
/// (`SimpleFontData::PlatformInit`): ascent, descent and line gap are
/// rounded to whole pixels.
pub(crate) fn line_metrics(fonts: &FontContext, font: FontId, size: f32) -> LineMetrics {
    let m = fonts.metrics(font, size);
    LineMetrics {
        ascent: m.ascent.round(),
        descent: m.descent.round(),
        line_gap: m.line_gap.round(),
        x_height: m.x_height,
    }
}
