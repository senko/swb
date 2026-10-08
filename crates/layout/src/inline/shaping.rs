//! Shaping of an inline formatting context's text, and splitting it into
//! pieces at soft wrap opportunities.
//!
//! The result does not depend on the available width, so it is computed
//! once per inline formatting context and cached.

use std::collections::HashMap;
use std::ops::Range;

use swb_style::{
    ComputedStyle, FontVariantCaps, UnicodeBidi, VerticalAlign, VerticalAlignKeyword, WhiteSpace,
};
use swb_text::{Feature, FontContext, FontId, FontQuery, FontRun, GlyphId, ShapeOptions};
use unicode_segmentation::UnicodeSegmentation;

use super::caps;
use crate::box_tree::{
    InlineFormattingContext, InlineItem, has_inline_end_edge, has_inline_start_edge,
};
use crate::fonts::{self, LineMetrics};

/// A glyph with its advance, in px.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyph {
    pub(crate) id: GlyphId,
    /// Byte offset of the glyph's cluster in the context's text.
    pub(crate) cluster: usize,
    pub(crate) advance: f32,
    pub(crate) x_offset: f32,
    pub(crate) y_offset: f32,
}

/// A run of text in one font.
#[derive(Debug)]
pub(crate) struct Run {
    /// Index of the text item this run belongs to.
    pub(crate) item: usize,
    pub(crate) font: FontId,
    pub(crate) font_size: f32,
    pub(crate) metrics: LineMetrics,
    pub(crate) glyphs: Vec<Glyph>,
    /// The text the run covers, as byte offsets in the context's text.
    pub(crate) text: Range<usize>,
}

/// A unit of inline content for line breaking.
#[derive(Clone, Debug)]
pub(crate) enum Piece {
    /// Glyphs `glyphs` of run `run`, covering `text` in the context's text.
    Text {
        run: usize,
        glyphs: Range<usize>,
        text: Range<usize>,
        /// The advance of the text item's glyphs before this piece (see
        /// [`snapped_width`]).
        offset: f64,
        width: f32,
        /// Width of trailing white space that hangs at the end of a line.
        trailing_space: f32,
    },
    /// Start of the inline box at item index.
    StartBox(usize),
    /// End of an inline box. `start` is the item index of its start;
    /// `split` is true if the box continues after a block-level child.
    EndBox { start: usize, split: bool },
    /// The atomic inline at item index.
    Atomic(usize),
    /// A forced line break at item index.
    LineBreak(usize),
    /// An out-of-flow box (float or absolutely positioned) at item index.
    OutOfFlow(usize),
}

/// The shaped text of an inline formatting context.
#[derive(Debug)]
pub(crate) struct ShapedText {
    pub(crate) runs: Vec<Run>,
    /// Pieces in logical order. A piece with `break_before[i]` true starts
    /// at a soft wrap opportunity.
    pub(crate) pieces: Vec<Piece>,
    pub(crate) break_before: Vec<bool>,
}

/// Shapes the text of `ifc` and splits it into pieces.
pub(crate) fn shape(fonts: &mut FontContext, ifc: &InlineFormattingContext) -> ShapedText {
    let breaks = super::breaks::breaks(ifc);
    let opportunities = breaks.opportunities;
    let mut runs_by_item = shape_all_text(fonts, ifc, &breaks.context_ends);
    let mut runs = Vec::new();
    let mut pieces = Vec::new();
    let mut break_before = Vec::new();
    let mut box_stack: Vec<usize> = Vec::new();

    for (index, item) in ifc.items.iter().enumerate() {
        match item {
            InlineItem::Text { style, .. } => {
                let first_run = runs.len();
                runs.append(&mut runs_by_item[index]);
                let split = Split {
                    text: &ifc.text,
                    style,
                    opportunities: &opportunities,
                };
                let mut offset = 0.0_f64;
                for (k, run) in runs[first_run..].iter().enumerate() {
                    offset = split.run(run, first_run + k, offset, &mut pieces, &mut break_before);
                }
            }
            InlineItem::StartBox { .. } => {
                box_stack.push(index);
                pieces.push(Piece::StartBox(index));
                break_before.push(false);
            }
            InlineItem::EndBox { split } => {
                let start = box_stack.pop().unwrap_or(index);
                pieces.push(Piece::EndBox {
                    start,
                    split: *split,
                });
                break_before.push(false);
            }
            InlineItem::Atomic { offset, .. } => {
                pieces.push(Piece::Atomic(index));
                break_before.push(opportunities.binary_search(offset).is_ok());
            }
            InlineItem::LineBreak(_) => {
                pieces.push(Piece::LineBreak(index));
                break_before.push(false);
            }
            InlineItem::Float(_) | InlineItem::AbsolutelyPositioned(_) => {
                pieces.push(Piece::OutOfFlow(index));
                break_before.push(false);
            }
        }
    }
    ShapedText {
        runs,
        pieces,
        break_before,
    }
}

/// Shapes all text of the context. Consecutive text items with the same
/// font properties are shaped together, so that kerning and ligatures work
/// across inline element boundaries (measured in Chromium 148: the space
/// before a link and its first letter `A` are kerned; see
/// `tests/layout/text-break-boundaries.html`). Shaping does not continue across
/// the start of an inline box with a margin, border or padding on its start
/// side, the end of one with those on its end side (CSS Text 3 §7.3), the
/// ends of boxes for which [`breaks_shaping`] is true, atomic inlines and
/// line breaks. `context_ends` are the offsets where the shaping context
/// ends (see [`shaping_context`]). Returns the runs of each item, indexed
/// by item.
fn shape_all_text(
    fonts: &mut FontContext,
    ifc: &InlineFormattingContext,
    context_ends: &[usize],
) -> Vec<Vec<Run>> {
    let mut shaper = GroupShaper {
        fonts,
        ifc,
        context_ends,
        bidi_override: false,
        line_metrics: HashMap::new(),
        out: ifc.items.iter().map(|_| Vec::new()).collect(),
    };
    let mut group: Vec<usize> = Vec::new();
    let mut open_boxes: Vec<&ComputedStyle> = Vec::new();
    for (index, item) in ifc.items.iter().enumerate() {
        let ends_group = match item {
            InlineItem::Text { style, range, .. } => group.last().is_some_and(|&last| {
                !matches!(&ifc.items[last], InlineItem::Text { style: s, range: r, .. }
                    if r.end == range.start && same_font(s, style))
            }),
            InlineItem::StartBox { base, .. } => {
                open_boxes.push(&base.style);
                breaks_shaping(&base.style) || has_inline_start_edge(&base.style)
            }
            InlineItem::EndBox { .. } => open_boxes
                .pop()
                .is_some_and(|s| breaks_shaping(s) || has_inline_end_edge(s)),
            _ => true,
        };
        if ends_group {
            shaper.shape_group(&std::mem::take(&mut group));
        }
        if let InlineItem::Text { style, .. } = item {
            if group.is_empty() {
                // Boxes with a `unicode-bidi` other than `normal` break the
                // group at both ends, so the group has one setting.
                shaper.bidi_override = ltr_override(&open_boxes, style);
            }
            group.push(index);
        }
    }
    shaper.shape_group(&group);
    shaper.out
}

/// True if text in `text_style`, inside the open boxes `open_boxes`
/// (outermost first), is shaped left to right whatever its script:
/// `unicode-bidi: bidi-override` or `isolate-override` with `direction: ltr`
/// (measured in Chromium 148; see `swb_text::ShapeOptions::bidi_override`).
/// The innermost box with a `unicode-bidi` other than `normal` decides; the
/// style of the text is that of the box it is in, which covers text that is
/// directly in the block. An override on the block itself does not reach
/// text in inline boxes (not done).
fn ltr_override(open_boxes: &[&ComputedStyle], text_style: &ComputedStyle) -> bool {
    let style = open_boxes
        .iter()
        .rev()
        .copied()
        .find(|s| s.unicode_bidi != UnicodeBidi::Normal)
        .unwrap_or(text_style);
    matches!(
        style.unicode_bidi,
        UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride
    ) && style.direction == swb_style::Direction::Ltr
}

/// True if two styles select and space glyphs the same way.
fn same_font(a: &ComputedStyle, b: &ComputedStyle) -> bool {
    std::ptr::eq(a, b)
        || (a.font_family == b.font_family
            && a.font_size == b.font_size
            && a.font_weight == b.font_weight
            && a.font_style == b.font_style
            && a.font_stretch == b.font_stretch
            && a.font_variant_caps == b.font_variant_caps
            && a.font_variation_settings == b.font_variation_settings
            && a.font_feature_settings == b.font_feature_settings
            && a.letter_spacing == b.letter_spacing
            && a.word_spacing == b.word_spacing)
}

/// The OpenType features of `font-feature-settings`.
fn style_features(style: &ComputedStyle) -> Vec<Feature> {
    style
        .font_feature_settings
        .iter()
        .map(|&(tag, value)| Feature::from_setting(tag, value))
        .collect()
}

/// True if both ends of an inline box in `style` break shaping: a
/// `vertical-align` other than `baseline`, or a `unicode-bidi` other than
/// `normal` (measured in Chromium 148: the kerning of ` A` across the start
/// of such a box is gone; see `tests/layout/text-break-boundaries.html`).
fn breaks_shaping(style: &ComputedStyle) -> bool {
    style.vertical_align != VerticalAlign::Keyword(VerticalAlignKeyword::Baseline)
        || style.unicode_bidi != UnicodeBidi::Normal
}

/// Shapes the groups of text items of a context and collects their runs.
struct GroupShaper<'a> {
    fonts: &'a mut FontContext,
    ifc: &'a InlineFormattingContext,
    /// See [`shaping_context`].
    context_ends: &'a [usize],
    /// See [`ltr_override`]; set for each group.
    bidi_override: bool,
    /// The line metrics of the fonts used, by font and font size (bits).
    line_metrics: HashMap<(FontId, u32), LineMetrics>,
    /// The runs of each item.
    out: Vec<Vec<Run>>,
}

/// Where shaped text comes from in the context's text: offsets in the
/// shaped text map to offsets in its source.
#[derive(Clone, Copy)]
struct SourceMap<'a> {
    /// The start and end of the source in the context's text.
    source_start: usize,
    source_end: usize,
    /// The length of the shaped text.
    shaped_len: usize,
    /// See [`caps::CasedText::map`]; empty if the text is not case-mapped.
    map: &'a [(usize, usize)],
}

impl SourceMap<'_> {
    /// The offset in the context's text of `offset` in the shaped text.
    fn source_offset(&self, offset: usize) -> usize {
        if offset >= self.shaped_len {
            self.source_end
        } else {
            self.source_start + caps::source_offset(self.map, offset)
        }
    }
}

/// The fonts for text with synthesized small capitals.
enum CasedFonts<'q, 'f> {
    /// One font (a fallback font that lacks the features of the primary
    /// font).
    One(FontId),
    /// The fonts of `query`, whose primary font is `primary` (see
    /// `GroupShaper::cased_runs`).
    Fallback {
        primary: FontId,
        query: &'q FontQuery<'f>,
    },
}

/// Text of one font and size, to shape.
struct Part<'a> {
    /// The text that is shaped: a range of the shaped text of `source`.
    range: Range<usize>,
    text: &'a str,
    source: SourceMap<'a>,
    font: FontId,
    /// The size the text is shaped at.
    font_size: f32,
    /// The size of the font whose metrics count for the line (see
    /// [`GroupShaper::shape_part`]).
    metrics_size: f32,
    features: &'a [Feature],
}

impl GroupShaper<'_> {
    /// Shapes the text of consecutive text items (`items`, contiguous in
    /// the context's text, same font properties) and stores their runs.
    fn shape_group(&mut self, items: &[usize]) {
        let ifc = self.ifc;
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return;
        };
        let (InlineItem::Text { style, range, .. }, InlineItem::Text { range: end, .. }) =
            (&ifc.items[*first], &ifc.items[*last])
        else {
            return;
        };
        let range = range.start..end.end;
        let text = ifc.text.get(range.clone()).unwrap_or("");
        let families = fonts::family_names(&style.font_family);
        let query = fonts::query(style, &families);
        let caps = style.font_variant_caps;
        // Whether small capitals are synthesized depends on the primary
        // font; the fonts of the case-mapped text are chosen after that
        // (see `cased_runs`).
        if caps != FontVariantCaps::Normal {
            let primary = self.fonts.select(&query);
            if let Some(synthesis) = caps::plan(self.fonts, primary, caps).synthesis {
                let fonts = CasedFonts::Fallback {
                    primary,
                    query: &query,
                };
                self.shape_cased(items, style, range, &fonts, synthesis);
                return;
            }
        }
        for font_run in self.fonts.itemize(text, &query) {
            let source = range.start + font_run.range.start..range.start + font_run.range.end;
            let plan = caps::plan(self.fonts, font_run.font, caps);
            if let Some(synthesis) = plan.synthesis {
                // A fallback font without the features of the primary
                // font synthesizes them itself.
                let fonts = CasedFonts::One(font_run.font);
                self.shape_cased(items, style, source, &fonts, synthesis);
                continue;
            }
            let source_map = SourceMap {
                source_start: source.start,
                source_end: source.end,
                shaped_len: source.len(),
                map: &[],
            };
            // The features of `font-variant-caps` come first: a later
            // setting of a feature wins.
            let mut features = plan.features;
            features.extend(style_features(style));
            let part = Part {
                range: 0..source.len(),
                text: ifc.text.get(source).unwrap_or(""),
                source: source_map,
                font: font_run.font,
                font_size: style.font_size,
                metrics_size: style.font_size,
                features: &features,
            };
            self.shape_part(items, style, &part);
        }
    }

    /// Shapes `source` of the context's text with synthesized small
    /// capitals in `fonts`.
    fn shape_cased(
        &mut self,
        items: &[usize],
        style: &ComputedStyle,
        source: Range<usize>,
        fonts: &CasedFonts<'_, '_>,
        synthesis: caps::Synthesis,
    ) {
        let ifc = self.ifc;
        let source_text = ifc.text.get(source.clone()).unwrap_or("");
        let features = style_features(style);
        let cased = caps::case_text(source_text, synthesis);
        let source_map = SourceMap {
            source_start: source.start,
            source_end: source.end,
            shaped_len: cased.text.len(),
            map: &cased.map,
        };
        let runs = match fonts {
            CasedFonts::One(font) => vec![FontRun {
                range: 0..cased.text.len(),
                font: *font,
            }],
            CasedFonts::Fallback { primary, query } => {
                self.cased_runs(&cased, source_text, *primary, query)
            }
        };
        let primary_font = match fonts {
            CasedFonts::Fallback { primary, .. } => Some(*primary),
            CasedFonts::One(_) => None,
        };
        for segment in &cased.segments {
            let font_size = if segment.small {
                caps::small_size(style.font_size)
            } else {
                style.font_size
            };
            for (range, font) in runs_in(&runs, &segment.range) {
                let part = Part {
                    text: cased.text.get(range.clone()).unwrap_or(""),
                    range,
                    source: source_map,
                    font,
                    font_size,
                    // Measured in Chromium 148: a fallback font counts with
                    // its metrics at the size it is shaped at, so small
                    // capitals in a fallback font make the line shorter
                    // than the same letters at the font size. The primary
                    // font keeps the metrics of the font size: the box's
                    // strut has them already, and with a fixed line height
                    // they decide the line.
                    metrics_size: if Some(font) == primary_font {
                        style.font_size
                    } else {
                        font_size
                    },
                    features: &features,
                };
                self.shape_part(items, style, &part);
            }
        }
    }

    /// The font runs of case-mapped text whose source is `source_text`: the
    /// primary font, and for a cluster that it lacks, the font of the
    /// source character if that font has the cluster; else the primary
    /// font (its `.notdef`). Measured in Chromium 148: small capitals of
    /// `ﬀ` use the primary font's `FF`; of `ɐ`, which the primary font has,
    /// its `.notdef` (not the fallback font's `Ɐ`); of Armenian, the
    /// fallback font.
    fn cased_runs(
        &mut self,
        cased: &caps::CasedText,
        source_text: &str,
        primary: FontId,
        query: &FontQuery<'_>,
    ) -> Vec<FontRun> {
        // The fonts of the source text, found when the primary font lacks
        // a cluster.
        let mut source_runs: Option<Vec<FontRun>> = None;
        let mut runs: Vec<FontRun> = Vec::new();
        for (start, cluster) in cased.text.grapheme_indices(true) {
            let font = if self.fonts.covers_cluster(primary, cluster) {
                primary
            } else {
                let source = caps::source_offset(&cased.map, start);
                let source_runs =
                    source_runs.get_or_insert_with(|| self.fonts.itemize(source_text, query));
                let hint = source_runs
                    .get(source_runs.partition_point(|r| r.range.end <= source))
                    .map_or(primary, |r| r.font);
                if hint != primary && self.fonts.covers_cluster(hint, cluster) {
                    hint
                } else {
                    primary
                }
            };
            let range = start..start + cluster.len();
            match runs.last_mut() {
                Some(run) if run.font == font => run.range.end = range.end,
                _ => runs.push(FontRun { range, font }),
            }
        }
        runs
    }

    /// Shapes `part` and adds its glyphs to the runs of `items`.
    fn shape_part(&mut self, items: &[usize], style: &ComputedStyle, part: &Part<'_>) {
        let ifc = self.ifc;
        let at =
            part.source.source_offset(part.range.start)..part.source.source_offset(part.range.end);
        let (pre_context, post_context) = shaping_context(&ifc.text, self.context_ends, at.clone());
        let options = ShapeOptions {
            features: part.features,
            pre_context,
            post_context,
            bidi_override: self.bidi_override,
            ..ShapeOptions::default()
        };
        let shaped = self
            .fonts
            .shape(part.font, part.font_size, part.text, &options);
        let key = (part.font, part.metrics_size.to_bits());
        let metrics = *self
            .line_metrics
            .entry(key)
            .or_insert_with(|| fonts::line_metrics(self.fonts, part.font, part.metrics_size));
        let shaped_part = ShapedPart {
            font: part.font,
            font_size: part.font_size,
            metrics,
            text: at,
            shaped_start: part.range.start,
            source: part.source,
        };
        shaped_part.split_among_items(ifc, items, style, &shaped.glyphs, &mut self.out);
    }
}

/// Up to five characters of context around `at` in `text` for shaping,
/// so that Arabic joins across element boundaries and font changes. The
/// context ends at the offsets `ends` (in increasing order): the edges of
/// isolating boxes (`unicode-bidi: isolate`, `isolate-override`,
/// `plaintext`), and forced line breaks. Measured in Chromium 148: Arabic
/// does not join across them, but joins across boxes with `unicode-bidi:
/// embed`, floats and absolutely positioned boxes (atomic inlines are
/// U+FFFC in the text, which does not join). The text of a `bidi-override`
/// box takes the context of the words around it too, but it is shaped left
/// to right (see [`ltr_override`]).
fn shaping_context<'a>(text: &'a str, ends: &[usize], at: Range<usize>) -> (&'a str, &'a str) {
    let (pre, post) = swb_text::context_around(text, at.clone());
    let pre_start = at.start - pre.len();
    let before = ends.partition_point(|&b| b <= at.start);
    let pre = match before.checked_sub(1).and_then(|i| ends.get(i)) {
        Some(&b) if b > pre_start => text.get(b..at.start).unwrap_or(""),
        _ => pre,
    };
    let after = ends.partition_point(|&b| b < at.end);
    let post = match ends.get(after) {
        Some(&b) if b < at.end + post.len() => text.get(at.end..b).unwrap_or(""),
        _ => post,
    };
    (pre, post)
}

/// Text of one font and size, shaped, before it is split among the items
/// of its group.
struct ShapedPart<'a> {
    font: FontId,
    font_size: f32,
    metrics: LineMetrics,
    /// The source of the shaped text in the context's text.
    text: Range<usize>,
    /// Where the shaped text starts in the shaped text of `source`.
    shaped_start: usize,
    source: SourceMap<'a>,
}

impl ShapedPart<'_> {
    /// Splits the glyphs among the text items of `group` by cluster offset
    /// and adds them as runs.
    fn split_among_items(
        &self,
        ifc: &InlineFormattingContext,
        group: &[usize],
        style: &ComputedStyle,
        glyphs: &[swb_text::ShapedGlyph],
        out: &mut [Vec<Run>],
    ) {
        let item_range = |i: usize| match &ifc.items[i] {
            InlineItem::Text { range, .. } => range.clone(),
            _ => 0..0,
        };
        let Some(&last) = group.last() else {
            return;
        };
        let mut current: Option<(usize, Vec<Glyph>)> = None;
        let flush = |current: Option<(usize, Vec<Glyph>)>, out: &mut [Vec<Run>]| {
            if let Some((item, mut glyphs)) = current {
                let r = item_range(item);
                let start = r.start.max(self.text.start);
                let end = r.end.min(self.text.end);
                apply_spacing(&mut glyphs, &ifc.text, style);
                out[item].push(Run {
                    item,
                    font: self.font,
                    font_size: self.font_size,
                    metrics: self.metrics,
                    glyphs,
                    text: start..end,
                });
            }
        };
        for g in glyphs {
            let cluster = self
                .source
                .source_offset(self.shaped_start + g.cluster as usize);
            // The items of the group cover contiguous ranges in text order.
            let index = group.partition_point(|&i| item_range(i).end <= cluster);
            let item = group.get(index).copied().unwrap_or(last);
            if current.as_ref().is_none_or(|(i, _)| *i != item) {
                flush(current.take(), out);
                current = Some((item, Vec::new()));
            }
            if let Some((_, glyphs)) = current.as_mut() {
                glyphs.push(Glyph {
                    id: g.glyph,
                    cluster,
                    advance: g.x_advance,
                    x_offset: g.x_offset,
                    y_offset: g.y_offset,
                });
            }
        }
        flush(current.take(), out);
    }
}

/// Adds `letter-spacing` to every cluster and `word-spacing` to spaces,
/// rounded to 16.16 fixed point like the advances, so that their sums stay
/// exact.
fn apply_spacing(glyphs: &mut [Glyph], text: &str, style: &ComputedStyle) {
    if style.letter_spacing == 0.0 && style.word_spacing == 0.0 {
        return;
    }
    let to_fixed = |px: f32| (px * 65536.0).round() / 65536.0;
    let mut i = 0;
    while i < glyphs.len() {
        let cluster = glyphs[i].cluster;
        let mut last = i;
        while last + 1 < glyphs.len() && glyphs[last + 1].cluster == cluster {
            last += 1;
        }
        let mut extra = style.letter_spacing;
        if matches!(
            text.get(cluster..).and_then(|t| t.chars().next()),
            Some(' ' | '\u{A0}')
        ) {
            extra += style.word_spacing;
        }
        glyphs[last].advance += to_fixed(extra);
        i = last + 1;
    }
}

/// The inputs for splitting the runs of one text item into pieces.
struct Split<'a> {
    text: &'a str,
    style: &'a ComputedStyle,
    opportunities: &'a [usize],
}

impl Split<'_> {
    /// Splits a run into pieces at wrap opportunities. `offset` is the
    /// advance of the item's glyphs before the run; returns it after the
    /// run. The sums are exact (`f64` of 16.16 fixed point values): with
    /// `f32` sums, the rounding to 1/64 px gave 1/64 px less than Chromium
    /// 148 for 7 of 645 measured texts; with exact sums, none.
    fn run(
        &self,
        run: &Run,
        run_index: usize,
        mut offset: f64,
        pieces: &mut Vec<Piece>,
        break_before: &mut Vec<bool>,
    ) -> f64 {
        let hangs = matches!(
            self.style.white_space,
            WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine | WhiteSpace::PreWrap
        );
        let opportunities = self.opportunities;
        let mut start = 0;
        while start < run.glyphs.len() {
            let start_offset = run.glyphs[start].cluster;
            // The next opportunity after this piece's start.
            let next_break = opportunities
                .get(opportunities.partition_point(|&o| o <= start_offset))
                .copied()
                .unwrap_or(usize::MAX);
            let mut end = start;
            while end < run.glyphs.len() && run.glyphs[end].cluster < next_break {
                end += 1;
            }
            let end_offset = run.glyphs.get(end).map_or(run.text.end, |g| g.cluster);
            // The end of the glyphs and of the glyphs before the trailing
            // white space that hangs. Preserved tabs at the end of a line
            // hang too (CSS Text 3 §4.1.3; the other styles that hang have
            // no tabs left).
            let mut position = offset;
            let mut content_end = offset;
            for g in &run.glyphs[start..end] {
                position += f64::from(g.advance);
                if !hangs || !self.text[g.cluster..].starts_with([' ', '\t']) {
                    content_end = position;
                }
            }
            let width = (snap_up(position) - snap_up(offset)) as f32;
            break_before.push(starts_at_opportunity(
                pieces.last(),
                start_offset,
                opportunities,
            ));
            pieces.push(Piece::Text {
                run: run_index,
                glyphs: start..end,
                text: start_offset..end_offset,
                offset,
                width,
                trailing_space: (snap_up(position) - snap_up(content_end)) as f32,
            });
            offset = position;
            start = end;
        }
        offset
    }
}

/// The width of glyphs with advance `advance` that start `offset` px after
/// the start of their text item. Chromium gives the part of a text item on
/// a line its advance rounded up to 1/64 px (measured: text widths are
/// multiples of 1/64 px, never less than the advance). This is the
/// difference of the rounded advances from the start of the item, so the
/// widths of the pieces of an item add up to that width when the item
/// starts on the line.
pub(crate) fn snapped_width(offset: f64, advance: f64) -> f32 {
    (snap_up(offset + advance) - snap_up(offset)) as f32
}

/// `x` rounded up to 1/64 px.
fn snap_up(x: f64) -> f64 {
    (x * 64.0).ceil() / 64.0
}

/// The sum of the advances of `glyphs`, exact (see [`Split::run`]).
pub(crate) fn advance_of(glyphs: &[Glyph]) -> f64 {
    glyphs.iter().map(|g| f64::from(g.advance)).sum()
}

/// The parts of `runs` (in text order) inside `range`, with their fonts.
fn runs_in(runs: &[FontRun], range: &Range<usize>) -> impl Iterator<Item = (Range<usize>, FontId)> {
    let (start, end) = (range.start, range.end);
    let first = runs.partition_point(|r| r.range.end <= start);
    runs.get(first..)
        .unwrap_or(&[])
        .iter()
        .take_while(move |r| r.range.start < end)
        .map(move |r| (r.range.start.max(start)..r.range.end.min(end), r.font))
}

/// True if a text piece that starts at `start` (after the piece
/// `previous`) starts at a soft wrap opportunity. A piece that starts at
/// the same offset as the text piece before it does not: the two are parts
/// of one source character, such as `ŉ` uppercased to `ʼN` in small
/// capitals, with its parts in two fonts.
fn starts_at_opportunity(previous: Option<&Piece>, start: usize, opportunities: &[usize]) -> bool {
    let same_start = matches!(previous, Some(Piece::Text { text, .. }) if text.start == start);
    !same_start && opportunities.binary_search(&start).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_break_inside_one_source_character() {
        let text = |text: Range<usize>| Piece::Text {
            run: 0,
            glyphs: 0..1,
            text,
            offset: 0.0,
            width: 1.0,
            trailing_space: 0.0,
        };
        let opportunities = [3];
        assert!(starts_at_opportunity(None, 3, &opportunities));
        assert!(starts_at_opportunity(Some(&text(0..3)), 3, &opportunities));
        assert!(!starts_at_opportunity(Some(&text(3..3)), 3, &opportunities));
        assert!(!starts_at_opportunity(Some(&text(0..2)), 2, &opportunities));
        assert!(starts_at_opportunity(
            Some(&Piece::StartBox(0)),
            3,
            &opportunities
        ));
    }

    #[test]
    fn shaping_context_ends_at_context_ends() {
        let text = "abcdefghij";
        // An end inside the five characters before and after.
        assert_eq!(shaping_context(text, &[3, 8], 5..6), ("de", "gh"));
        // An end at the start or the end of the shaped text.
        assert_eq!(shaping_context(text, &[3, 8], 3..4), ("", "efgh"));
        assert_eq!(shaping_context(text, &[3, 8], 6..8), ("def", ""));
        // Ends outside the five characters do not matter.
        assert_eq!(shaping_context(text, &[0], 7..8), ("cdefg", "ij"));
        assert_eq!(shaping_context(text, &[], 0..1), ("", "bcdef"));
    }
}
