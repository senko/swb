//! Text shaping with harfrust (a Rust port of `HarfBuzz`).
//!
//! Positions are computed in font units and scaled to pixels with
//! subpixel precision (16.16 fixed point), which gives Chromium's text
//! widths. The font size is truncated to 1/100 px, and nominal glyph
//! advances use it truncated to 1/64 px, as measured in Chromium (see
//! [`chromium_sizes`]).

use std::ops::Range;
use std::str::FromStr;

use harfrust::{Language, ShapePlan, ShapePlanKey, UnicodeBuffer, script};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};

use crate::context::{FontContext, Instance, sanitize_size};
use crate::{FontId, GlyphId};

/// Text direction of a run.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum Direction {
    /// Left to right. The characters of right-to-left scripts (Arabic,
    /// Hebrew, ...) are still shaped right to left, so that their letters
    /// join, and their clusters are returned in logical order (layout has
    /// no bidi reordering yet).
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

impl Feature {
    /// The feature for a `font-feature-settings` entry (property or
    /// `@font-face` descriptor). A negative value becomes a large unsigned
    /// one, as in `HarfBuzz`.
    #[must_use]
    pub fn from_setting(tag: [u8; 4], value: i32) -> Self {
        Self {
            tag,
            value: value as u32,
        }
    }
}

/// Shaping parameters for one run.
#[derive(Copy, Clone, Debug, Default)]
pub struct ShapeOptions<'a> {
    /// The direction of the run.
    pub direction: Direction,
    /// The content language (BCP 47), which selects language-specific
    /// glyph forms.
    pub language: Option<&'a str>,
    /// Feature settings on top of the defaults. A later setting of a
    /// feature wins over an earlier one; the `font-feature-settings` of a
    /// web font's `@font-face` rule come before these.
    pub features: &'a [Feature],
    /// The text before the shaped text, for contextual shaping (Arabic
    /// joining across font fallback and element boundaries). It is not
    /// shaped itself; `HarfBuzz` uses up to five characters.
    pub pre_context: &'a str,
    /// The text after the shaped text (see `pre_context`).
    pub post_context: &'a str,
    /// Shape the characters of right-to-left scripts left to right too, as
    /// the text of a box with `unicode-bidi: bidi-override` or
    /// `isolate-override` and `direction: ltr` is. Their letters then join
    /// differently: measured in Chromium 148, the Arabic word `بال` is 13.58
    /// px wide at 16 px in the bundled `DejaVu Sans` with Arabic words around
    /// it (14.2 px right to left), and 24.19 px alone (20.95 px right to
    /// left). Without effect for [`Direction::Rtl`].
    pub bidi_override: bool,
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
    /// Glyphs in visual order for [`Direction::Rtl`]; in logical order for
    /// [`Direction::Ltr`] (see there).
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
        let Some(instance) = self.instances.get_mut(font.0 as usize) else {
            return ShapedRun::default();
        };
        // `size-adjust` of a web font face scales the font size.
        let size = sanitize_size(size * instance.size_adjust());
        if options.direction == Direction::Rtl || options.bidi_override || !text.chars().any(is_rtl)
        {
            return shape_with(instance, size, text, options)
                .unwrap_or_else(|| placeholder_shape(text, size));
        }
        shape_segments(instance, size, text, options)
    }
}

/// Shapes `text`, asked for left to right, in direction segments: the
/// characters of right-to-left scripts are shaped right to left, so that
/// they join and are positioned as in a right-to-left run, and returned in
/// logical order (there is no bidi reordering yet); the other characters
/// are shaped left to right, so that brackets are not mirrored.
fn shape_segments(
    instance: &mut Instance,
    size: f32,
    text: &str,
    options: &ShapeOptions<'_>,
) -> ShapedRun {
    let mut run = ShapedRun::default();
    for (range, rtl) in direction_segments(text) {
        let part = &text[range.clone()];
        let (before, after) = context_around(text, range.clone());
        let sub = ShapeOptions {
            direction: if rtl { Direction::Rtl } else { Direction::Ltr },
            pre_context: if range.start == 0 {
                options.pre_context
            } else {
                before
            },
            post_context: if range.end == text.len() {
                options.post_context
            } else {
                after
            },
            ..*options
        };
        let mut shaped =
            shape_with(instance, size, part, &sub).unwrap_or_else(|| placeholder_shape(part, size));
        if rtl {
            reverse_clusters(&mut shaped.glyphs);
        }
        let base = u32::try_from(range.start).unwrap_or(u32::MAX);
        for glyph in &mut shaped.glyphs {
            glyph.cluster = glyph.cluster.saturating_add(base);
        }
        run.advance += shaped.advance;
        run.glyphs.append(&mut shaped.glyphs);
    }
    run
}

/// Splits `text` into segments of characters of right-to-left scripts
/// (`true`) and of other characters. Combining marks and joiners stay with
/// the character before them.
fn direction_segments(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut out: Vec<(Range<usize>, bool)> = Vec::new();
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        match out.last_mut() {
            Some((range, rtl)) if *rtl == is_rtl(c) || joins_previous(c) => range.end = end,
            _ => out.push((i..end, is_rtl(c))),
        }
    }
    out
}

/// True for characters of the right-to-left scripts (Hebrew, Arabic,
/// Syriac, Thaana, N'Ko and others): their blocks.
fn is_rtl(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// Combining marks, ZWJ and ZWNJ, which belong to the character before.
fn joins_previous(c: char) -> bool {
    c == '\u{200D}'
        || unicode_linebreak::break_property(u32::from(c))
            == unicode_linebreak::BreakClass::CombiningMark
}

/// The number of characters of context for shaping (`HarfBuzz` uses up to
/// five).
const CONTEXT_CHARS: usize = 5;

/// The most shape plans kept per font instance. A plan depends on the
/// script, direction, language and features, so real pages need a few.
/// The lookup is linear: without the limit, a page with many distinct
/// `font-feature-settings` lists makes shaping quadratic.
const MAX_PLANS_PER_INSTANCE: usize = 32;

/// Up to five characters of `text` before and after `range`: the context
/// for shaping that part of the text (`ShapeOptions::pre_context` and
/// `post_context`).
pub fn context_around(text: &str, range: Range<usize>) -> (&str, &str) {
    let before = text.get(..range.start).unwrap_or("");
    let start = before
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS - 1)
        .map_or(0, |(i, _)| i);
    let after = text.get(range.end..).unwrap_or("");
    let end = after
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map_or(after.len(), |(i, _)| i);
    (&before[start..], &after[..end])
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
    buffer.set_pre_context(options.pre_context);
    buffer.set_post_context(options.post_context);
    buffer.set_direction(direction);
    if let Some(language) = &language {
        buffer.set_language(language.clone());
    }
    buffer.guess_segment_properties();
    let script = Some(buffer.script()).filter(|s| *s != script::UNKNOWN);

    // The features of the `@font-face` rule come first: a later setting
    // of the same feature wins.
    let features: Vec<harfrust::Feature> = face
        .features
        .iter()
        .chain(options.features)
        .map(|f| harfrust::Feature::new(harfrust::Tag::new(&f.tag), f.value, ..))
        .collect();
    let key = ShapePlanKey::new(script, direction)
        .language(language.as_ref())
        .instance(instance.variation.as_ref())
        .features(&features);
    let plan_index = if let Some(index) = instance.plans.iter().position(|p| key.matches(p)) {
        index
    } else {
        if instance.plans.len() >= MAX_PLANS_PER_INSTANCE {
            instance.plans.clear();
        }
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

    let units_per_em = f64::from(face.units_per_em);
    let (size, advance_size) = chromium_sizes(size);
    let scale = font_scale(size) / units_per_em;
    let advance_scale = f64::from(advance_size) / units_per_em;
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::new(&instance.coords));
    let mut run = ShapedRun {
        glyphs: Vec::with_capacity(output.len()),
        advance: 0.0,
    };
    for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        // The nominal advance at the advance size, and the adjustments of
        // the shaper (kerning) at the font size, each in `HarfBuzz`'s 16.16
        // fixed point (see `chromium_sizes` and `font_scale`). A zero
        // advance (a mark) stays zero.
        let shaped = position.x_advance as f32;
        let x_advance = if position.x_advance == 0 {
            0.0
        } else {
            let nominal = metrics
                .advance_width(skrifa::GlyphId::new(info.glyph_id))
                .unwrap_or(shaped);
            let nominal_advance = to_fixed(f64::from(nominal) * advance_scale);
            let kerning = to_fixed(f64::from(shaped - nominal) * scale);
            (nominal_advance + kerning) as f32
        };
        run.advance += x_advance;
        run.glyphs.push(ShapedGlyph {
            glyph: GlyphId(info.glyph_id),
            cluster: info.cluster,
            x_advance,
            x_offset: (f64::from(position.x_offset) * scale) as f32,
            y_offset: (f64::from(position.y_offset.saturating_neg()) * scale) as f32,
            unsafe_to_break: info.unsafe_to_break(),
        });
    }
    Some(run)
}

/// Reverses the order of the clusters of `glyphs` (in visual order of a
/// right-to-left run) to logical order. The glyphs of each cluster keep
/// their order, so that the offsets of marks stay relative to their base.
fn reverse_clusters(glyphs: &mut Vec<ShapedGlyph>) {
    let mut out = Vec::with_capacity(glyphs.len());
    let mut end = glyphs.len();
    while end > 0 {
        let cluster = glyphs[end - 1].cluster;
        let mut start = end - 1;
        while start > 0 && glyphs[start - 1].cluster == cluster {
            start -= 1;
        }
        out.extend_from_slice(&glyphs[start..end]);
        end = start;
    }
    *glyphs = out;
}

/// The font sizes that Chromium shapes `size` with: the size for GPOS
/// adjustments (kerning), and the size for nominal glyph advances.
///
/// Measured in Chromium 148 (`swbtools measure font-size-sweep`; the data
/// is in `tests/chromium-font-sizes.txt`): for each of the 15,001 CSS font
/// sizes from 9.000 to 24.000 px in steps of 0.001 px, shown alone on a
/// page, kerning uses the size truncated to 1/100 px in `f32` arithmetic,
/// and the nominal advances use that size truncated again to 1/64 px (as
/// `FreeType`'s 26.6 fixed point sizes would give). So at 16.21 px kerning
/// uses 16.2 px (`16.21f32 * 100.0` is just below 1621) and the advances
/// are those of 16.1875 px. The test `font_size_rule_matches_chromium`
/// checks every size.
///
/// Not modelled: when a page has two sizes of one font that are less than
/// 0.01 px apart, Chromium can use the first one for both. Its font cache
/// is keyed by the size truncated to 1/100 px twice, and the first size
/// that asks for a key decides. This changes a result only for the
/// hundredths whose `f32` value is below the decimal value (9.11, 9.15,
/// ...): in a page that has 9.110 px first, 9.111 to 9.119 px get the
/// sizes of 9.110 px. A scan of all sizes in one page differs from the
/// sizes alone for 684 of 15,001 sizes (see `tools/swbtools/font_sizes.py`).
pub(crate) fn chromium_sizes(size: f32) -> (f32, f32) {
    let font = (size * 100.0).trunc() / 100.0;
    (font, (font * 64.0).trunc() / 64.0)
}

/// The scale of the shaper's font, in pixels, for the kerning size `size`:
/// the size in 16.16 fixed point (`HarfBuzz`'s `hb_position_t`), truncated.
///
/// Measured in Chromium 148 (`tests/chromium-font-widths.txt`: 100 times
/// `AV` in Liberation Sans at 1,154 sizes): the kerning of each pair is
/// the font units times this scale, rounded to 16.16. With the size
/// itself as the scale, a pair can differ by 1/65536 px, and the width of
/// the 200 glyphs by 1/64 px (10 of the 1,154 sizes).
fn font_scale(size: f32) -> f64 {
    (f64::from(size) * 65536.0).trunc() / 65536.0
}

/// Rounds a length in pixels to 16.16 fixed point (`hb_position_t`), halves
/// away from zero.
fn to_fixed(px: f64) -> f64 {
    (px * 65536.0).round() / 65536.0
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

#[cfg(test)]
mod tests {
    use super::chromium_sizes;

    /// The sizes of Chromium 148 for each CSS size (`swbtools measure
    /// font-size-sweep`).
    const MEASURED: &str = include_str!("../tests/chromium-font-sizes.txt");

    /// The CSS font size `milli` thousandths of a pixel, parsed from its
    /// decimal text as the CSS tokenizer does.
    fn css_size(milli: u32) -> f32 {
        format!("{}.{:03}", milli / 1000, milli % 1000)
            .parse()
            .expect("a decimal number is a valid f32")
    }

    #[test]
    fn font_size_rule_matches_chromium() {
        let mut count = 0;
        for line in MEASURED.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let numbers: Vec<u32> = line
                .split_whitespace()
                .map(|n| n.parse().expect("the data file has only numbers"))
                .collect();
            let [first, last, kerning, advance] = numbers[..] else {
                panic!("a line has four numbers: {line}");
            };
            for milli in first..=last {
                let (kerning_size, advance_size) = chromium_sizes(css_size(milli));
                assert_eq!(
                    (
                        (kerning_size * 100.0).round() as u32,
                        (advance_size * 64.0).round() as u32
                    ),
                    (kerning, advance),
                    "at {}.{:03} px",
                    milli / 1000,
                    milli % 1000
                );
                count += 1;
            }
        }
        // 9.000 to 24.000 px in steps of 0.001 px.
        assert_eq!(count, 15_001);
    }

    #[test]
    fn examples() {
        // 16.21 in f32 is just below 16.21: kerning uses 16.2 px.
        assert_eq!(chromium_sizes(16.21), (16.2, 1036.0 / 64.0));
        assert_eq!(chromium_sizes(16.22), (16.21, 1037.0 / 64.0));
        assert_eq!(chromium_sizes(16.0), (16.0, 16.0));
        assert_eq!(chromium_sizes(14.4), (14.4, 14.390_625));
    }
}
