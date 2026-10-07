//! Shaping of an inline formatting context's text, and splitting it into
//! pieces at soft wrap opportunities.
//!
//! The result does not depend on the available width, so it is computed
//! once per inline formatting context and cached.

use std::ops::Range;

use swb_style::{ComputedStyle, WhiteSpace};
use swb_text::{BreakKind, FontContext, FontId, GlyphId, ShapeOptions};

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
    let opportunities = wrap_opportunities(ifc);
    let mut runs_by_item = shape_all_text(fonts, ifc);
    let mut runs = Vec::new();
    let mut pieces = Vec::new();
    let mut break_before = Vec::new();
    let mut box_stack: Vec<usize> = Vec::new();

    for (index, item) in ifc.items.iter().enumerate() {
        match item {
            InlineItem::Text { style, .. } => {
                let first_run = runs.len();
                runs.append(&mut runs_by_item[index]);
                for (offset, run) in runs[first_run..].iter().enumerate() {
                    split_run(
                        run,
                        first_run + offset,
                        &ifc.text,
                        style,
                        &opportunities,
                        &mut pieces,
                        &mut break_before,
                    );
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

/// Soft wrap opportunities: byte offsets in the context's text before which
/// a line may break. Offset 0 is never included.
fn wrap_opportunities(ifc: &InlineFormattingContext) -> Vec<usize> {
    // White-space of the text at each offset decides whether breaking is
    // allowed there. The ranges are in text order and do not overlap.
    let no_wrap: Vec<Range<usize>> = ifc
        .items
        .iter()
        .filter_map(|item| match item {
            InlineItem::Text { style, range, .. } if !style.white_space.wraps() => {
                Some(range.clone())
            }
            _ => None,
        })
        .collect();
    let in_no_wrap = |offset: usize| {
        // The first range that ends at or after `offset`.
        let i = no_wrap.partition_point(|r| r.end < offset);
        no_wrap.get(i).is_some_and(|r| r.start < offset)
    };
    swb_text::line_breaks(&ifc.text)
        .filter(|&(offset, kind)| {
            offset > 0
                && offset < ifc.text.len()
                && kind == BreakKind::Allowed
                && !in_no_wrap(offset)
        })
        .map(|(offset, _)| offset)
        .collect()
}

/// Shapes all text of the context. Consecutive text items with the same
/// font properties are shaped together, as Blink does, so that kerning and
/// ligatures work across inline element boundaries (for example the space
/// before a link and its first letter). Shaping does not continue across
/// inline boxes with horizontal margins, borders or padding, or across
/// atomic inlines and line breaks. Returns the runs of each item, indexed
/// by item.
fn shape_all_text(fonts: &mut FontContext, ifc: &InlineFormattingContext) -> Vec<Vec<Run>> {
    let mut out: Vec<Vec<Run>> = ifc.items.iter().map(|_| Vec::new()).collect();
    let mut group: Vec<usize> = Vec::new();
    let mut open_boxes: Vec<&ComputedStyle> = Vec::new();
    for (index, item) in ifc.items.iter().enumerate() {
        match item {
            InlineItem::Text { style, range, .. } => {
                if let Some(&last) = group.last()
                    && let InlineItem::Text {
                        style: last_style,
                        range: last_range,
                        ..
                    } = &ifc.items[last]
                    && (last_range.end != range.start || !same_font(last_style, style))
                {
                    shape_group(fonts, ifc, &std::mem::take(&mut group), &mut out);
                }
                group.push(index);
            }
            InlineItem::StartBox { base, .. } => {
                if has_inline_edges(&base.style) {
                    shape_group(fonts, ifc, &std::mem::take(&mut group), &mut out);
                }
                open_boxes.push(&base.style);
            }
            InlineItem::EndBox { .. } => {
                if open_boxes.pop().is_some_and(has_inline_edges) {
                    shape_group(fonts, ifc, &std::mem::take(&mut group), &mut out);
                }
            }
            _ => shape_group(fonts, ifc, &std::mem::take(&mut group), &mut out),
        }
    }
    shape_group(fonts, ifc, &group, &mut out);
    out
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
            && a.letter_spacing == b.letter_spacing
            && a.word_spacing == b.word_spacing)
}

fn has_inline_edges(style: &ComputedStyle) -> bool {
    has_inline_start_edge(style) || has_inline_end_edge(style)
}

/// Shapes the text of consecutive text items (`group`, contiguous in the
/// context's text, same font properties) and stores each item's runs.
fn shape_group(
    fonts: &mut FontContext,
    ifc: &InlineFormattingContext,
    group: &[usize],
    out: &mut [Vec<Run>],
) {
    let item_range = |i: usize| match &ifc.items[i] {
        InlineItem::Text { range, .. } => range.clone(),
        _ => 0..0,
    };
    let (Some(&first), Some(&last)) = (group.first(), group.last()) else {
        return;
    };
    let InlineItem::Text { style, .. } = &ifc.items[first] else {
        return;
    };
    let range = item_range(first).start..item_range(last).end;
    let text = &ifc.text[range.clone()];
    let families = fonts::family_names(&style.font_family);
    let query = fonts::query(style, &families);
    let options = ShapeOptions::default();
    for font_run in fonts.itemize(text, &query) {
        let sub = &text[font_run.range.clone()];
        let shaped = fonts.shape(font_run.font, style.font_size, sub, &options);
        let base = range.start + font_run.range.start;
        let metrics = fonts::line_metrics(fonts, font_run.font, style.font_size);
        // Split the glyphs among the items by cluster offset.
        let mut current: Option<(usize, Vec<Glyph>)> = None;
        let flush = |current: Option<(usize, Vec<Glyph>)>, out: &mut [Vec<Run>]| {
            if let Some((item, mut glyphs)) = current {
                let r = item_range(item);
                let start = r.start.max(base);
                let end = r.end.min(base + sub.len());
                apply_spacing(&mut glyphs, &ifc.text, style);
                out[item].push(Run {
                    item,
                    font: font_run.font,
                    font_size: style.font_size,
                    metrics,
                    glyphs,
                    text: start..end,
                });
            }
        };
        for g in &shaped.glyphs {
            let cluster = base + g.cluster as usize;
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

/// Adds `letter-spacing` to every cluster and `word-spacing` to spaces.
fn apply_spacing(glyphs: &mut [Glyph], text: &str, style: &ComputedStyle) {
    if style.letter_spacing == 0.0 && style.word_spacing == 0.0 {
        return;
    }
    let mut i = 0;
    while i < glyphs.len() {
        let cluster = glyphs[i].cluster;
        let mut last = i;
        while last + 1 < glyphs.len() && glyphs[last + 1].cluster == cluster {
            last += 1;
        }
        let mut extra = style.letter_spacing;
        if matches!(text[cluster..].chars().next(), Some(' ' | '\u{A0}')) {
            extra += style.word_spacing;
        }
        glyphs[last].advance += extra;
        i = last + 1;
    }
}

/// Splits a run into pieces at wrap opportunities.
fn split_run(
    run: &Run,
    run_index: usize,
    text: &str,
    style: &ComputedStyle,
    opportunities: &[usize],
    pieces: &mut Vec<Piece>,
    break_before: &mut Vec<bool>,
) {
    let hangs = matches!(
        style.white_space,
        WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine | WhiteSpace::PreWrap
    );
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
        let glyphs = &run.glyphs[start..end];
        let width: f32 = glyphs.iter().map(|g| g.advance).sum();
        // Preserved tabs at the end of a line hang too (CSS Text 3 §4.1.3;
        // the other styles that hang have no tabs left).
        let trailing_space = if hangs {
            glyphs
                .iter()
                .rev()
                .take_while(|g| text[g.cluster..].starts_with([' ', '\t']))
                .map(|g| g.advance)
                .sum()
        } else {
            0.0
        };
        pieces.push(Piece::Text {
            run: run_index,
            glyphs: start..end,
            text: start_offset..end_offset,
            width,
            trailing_space,
        });
        break_before.push(opportunities.binary_search(&start_offset).is_ok());
        start = end;
    }
}
