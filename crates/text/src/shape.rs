//! Text shaping with harfrust (a Rust port of `HarfBuzz`).
//!
//! Positions are computed in font units and scaled to pixels without
//! rounding, like Blink's `HarfBuzz` integration with subpixel positioning.

use std::str::FromStr;

use harfrust::{Language, ShapePlan, ShapePlanKey, UnicodeBuffer, script};

use crate::context::{FontContext, Instance, sanitize_size};
use crate::{FontId, GlyphId};

/// Text direction of a run.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum Direction {
    /// Left to right.
    #[default]
    Ltr,
    /// Right to left. Glyphs are still returned in visual (left to right)
    /// order, so clusters decrease.
    Rtl,
}

/// An OpenType feature setting for a whole run, for example
/// `Feature { tag: *b"kern", value: 0 }` to disable kerning.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Feature {
    /// The feature tag.
    pub tag: [u8; 4],
    /// 0 disables the feature, 1 enables it; larger values select
    /// alternates.
    pub value: u32,
}

/// Shaping parameters for one run.
#[derive(Copy, Clone, Debug, Default)]
pub struct ShapeOptions<'a> {
    /// The direction of the run.
    pub direction: Direction,
    /// The content language (BCP 47), which selects language-specific
    /// glyph forms.
    pub language: Option<&'a str>,
    /// Feature settings on top of the defaults.
    pub features: &'a [Feature],
}

/// One positioned glyph. Values are in pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// The glyph in the run's font.
    pub glyph: GlyphId,
    /// Byte offset in the shaped text of the first character of the
    /// cluster that produced this glyph.
    pub cluster: u32,
    /// Horizontal advance.
    pub x_advance: f32,
    /// Horizontal offset from the pen position.
    pub x_offset: f32,
    /// Vertical offset from the baseline, y down.
    pub y_offset: f32,
    /// True if the text must be shaped again when a line breaks before
    /// this glyph's cluster (`HarfBuzz` `UNSAFE_TO_BREAK`).
    pub unsafe_to_break: bool,
}

/// The result of shaping one run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    /// Glyphs in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Sum of the glyph advances.
    pub advance: f32,
}

impl FontContext {
    /// Shapes `text` with one font in one direction, at `size` pixels.
    /// Clusters are byte offsets into `text`. Results are not cached.
    pub fn shape(
        &mut self,
        font: FontId,
        size: f32,
        text: &str,
        options: &ShapeOptions<'_>,
    ) -> ShapedRun {
        let size = sanitize_size(size);
        let Some(instance) = self.instances.get_mut(font.0 as usize) else {
            return ShapedRun::default();
        };
        shape_with(instance, size, text, options).unwrap_or_else(|| placeholder_shape(text, size))
    }
}

fn shape_with(
    instance: &mut Instance,
    size: f32,
    text: &str,
    options: &ShapeOptions<'_>,
) -> Option<ShapedRun> {
    let face = instance.face.clone()?;
    let font = face.font()?;
    let shaper = face
        .shaper
        .shaper(&font)
        .instance(instance.variation.as_ref())
        .build();

    let direction = match options.direction {
        Direction::Ltr => harfrust::Direction::LeftToRight,
        Direction::Rtl => harfrust::Direction::RightToLeft,
    };
    let language = options.language.and_then(|l| Language::from_str(l).ok());
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(direction);
    if let Some(language) = &language {
        buffer.set_language(language.clone());
    }
    buffer.guess_segment_properties();
    let script = Some(buffer.script()).filter(|s| *s != script::UNKNOWN);

    let features: Vec<harfrust::Feature> = options
        .features
        .iter()
        .map(|f| harfrust::Feature::new(harfrust::Tag::new(&f.tag), f.value, ..))
        .collect();
    let key = ShapePlanKey::new(script, direction)
        .language(language.as_ref())
        .instance(instance.variation.as_ref())
        .features(&features);
    let plan_index = if let Some(index) = instance.plans.iter().position(|p| key.matches(p)) {
        index
    } else {
        let plan = ShapePlan::new(&shaper, direction, script, language.as_ref(), &features);
        instance.plans.push(plan);
        instance.plans.len() - 1
    };
    let output = shaper.shape(
        buffer,
        harfrust::ShapeOptions::new()
            .plan(instance.plans.get(plan_index))
            .features(&features),
    );

    let scale = size / f32::from(face.units_per_em);
    let mut run = ShapedRun {
        glyphs: Vec::with_capacity(output.len()),
        advance: 0.0,
    };
    for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        let x_advance = position.x_advance as f32 * scale;
        run.advance += x_advance;
        run.glyphs.push(ShapedGlyph {
            glyph: GlyphId(info.glyph_id),
            cluster: info.cluster,
            x_advance,
            x_offset: position.x_offset as f32 * scale,
            y_offset: position.y_offset.saturating_neg() as f32 * scale,
            unsafe_to_break: info.unsafe_to_break(),
        });
    }
    Some(run)
}

/// Shaping for the placeholder font: glyph 0 for each character, half an
/// em wide.
fn placeholder_shape(text: &str, size: f32) -> ShapedRun {
    let glyphs: Vec<ShapedGlyph> = text
        .char_indices()
        .map(|(i, _)| ShapedGlyph {
            glyph: GlyphId(0),
            cluster: i as u32,
            x_advance: size * 0.5,
            x_offset: 0.0,
            y_offset: 0.0,
            unsafe_to_break: false,
        })
        .collect();
    ShapedRun {
        advance: glyphs.iter().map(|g| g.x_advance).sum(),
        glyphs,
    }
}
