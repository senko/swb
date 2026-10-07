//! Breaking words that do not fit: `overflow-wrap: break-word | anywhere`
//! and `word-break: break-word` (CSS Text 3 §5.2 and §5.5,
//! <https://www.w3.org/TR/css-text-3/#overflow-wrap-property>).
//!
//! As measured in Chromium 148 (`tests/layout/text-overflow-wrap.html`):
//! when the first unbreakable group of a line does not fit, and text in it
//! has one of these properties, the group is cut at the last grapheme
//! cluster boundary in such text where the part before it fits. Between
//! two text items, the item before decides. At least one grapheme cluster
//! stays on the line. This also happens in a line box that floats make
//! narrower. A cut is never placed before a space: the space hangs at the
//! end of the line instead.
//!
//! `anywhere` and `word-break: break-word` also make every grapheme cluster
//! boundary in the text a break opportunity for the min-content size;
//! `overflow-wrap: break-word` does not change it.
//!
//! Not done: floats inside a word that is cut are placed as if the whole
//! word were on the line where the cut is.

use std::ops::Range;

use swb_style::{ComputedStyle, OverflowWrap, WordBreak};
use unicode_segmentation::GraphemeCursor;

use super::shaping::{Piece, Run, ShapedText, snapped_width};
use crate::box_tree::{InlineFormattingContext, InlineItem};

/// Where a group is cut: before glyph `glyph` (an index into the glyphs of
/// the piece's run) of text piece `piece`, or before piece `piece` if
/// `glyph` is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cut {
    pub(crate) piece: usize,
    pub(crate) glyph: Option<usize>,
    /// The advance of the text item's glyphs before the cut glyph (the
    /// `offset` of a text piece, see `shaping::snapped_width`).
    pub(crate) offset: f64,
}

impl Cut {
    /// The glyph of piece `piece` where `cut` is, and the advance of the
    /// item before it, if the cut is inside that piece.
    pub(crate) fn glyph_in(cut: Option<Cut>, piece: usize) -> Option<(usize, f64)> {
        cut.filter(|c| c.piece == piece)
            .and_then(|c| c.glyph.map(|g| (g, c.offset)))
    }
}

/// What a cut is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// A line that overflows.
    Layout,
    /// The min-content size.
    MinContent,
}

/// True if text in `style` may be cut for `purpose`.
pub(crate) fn breaks_anywhere(style: &ComputedStyle, purpose: Purpose) -> bool {
    style.white_space.wraps()
        && (style.word_break == WordBreak::BreakWord
            || style.overflow_wrap == OverflowWrap::Anywhere
            || (purpose == Purpose::Layout && style.overflow_wrap == OverflowWrap::BreakWord))
}

/// The pieces of a group and how to measure them.
pub(crate) struct GroupText<'a> {
    pub(crate) ifc: &'a InlineFormattingContext,
    pub(crate) shaped: &'a ShapedText,
    pub(crate) pieces: Range<usize>,
    /// Where the group starts inside its first piece, if it is the rest of
    /// a cut group.
    pub(crate) head: Option<Cut>,
}

impl GroupText<'_> {
    /// The cuts in the group for `purpose`, with the width of the group's
    /// content before each, up to the first one where that width is more
    /// than `limit` (the cost of a line then depends on its length and on
    /// the text before its first cut). `width_of` gives the width of the
    /// pieces that are not text (inline box edges, atomic inlines).
    pub(crate) fn cuts(
        &self,
        purpose: Purpose,
        limit: f32,
        width_of: impl Fn(usize) -> f32,
    ) -> Vec<(Cut, f32)> {
        let mut scan = Scan {
            purpose,
            limit,
            out: Vec::new(),
            width: 0.0,
            content: false,
            after_cuttable: false,
            box_starts: None,
        };
        for i in self.pieces.clone() {
            let Some(piece) = self.shaped.pieces.get(i) else {
                break;
            };
            if let Piece::Text {
                run,
                glyphs,
                offset,
                ..
            } = piece
            {
                let Some(run) = self.shaped.runs.get(*run) else {
                    continue;
                };
                if !scan.text(self, i, run, glyphs.clone(), *offset) {
                    break;
                }
            } else {
                scan.other(piece, i, width_of(i));
            }
        }
        scan.out
    }

    /// The widest part of the group between cuts for the min-content size,
    /// without trailing spaces (`trailing_space` of the group), or `None`
    /// if the group has no cut. `width` is the group's width.
    pub(crate) fn min_content(
        &self,
        width: f32,
        trailing_space: f32,
        width_of: impl Fn(usize) -> f32,
    ) -> Option<f32> {
        let cuts = self.cuts(Purpose::MinContent, f32::INFINITY, width_of);
        if cuts.is_empty() {
            return None;
        }
        let mut widest: f32 = 0.0;
        let mut previous = 0.0;
        for (_, before) in &cuts {
            widest = widest.max(before - previous);
            previous = *before;
        }
        Some(widest.max(width - trailing_space - previous))
    }
}

/// The state of a scan for cuts through the pieces of a group (see
/// [`GroupText::cuts`]).
struct Scan {
    purpose: Purpose,
    limit: f32,
    out: Vec<(Cut, f32)>,
    /// The width of the group's content before the current position.
    width: f32,
    /// True once a grapheme cluster that is not a space is before the
    /// current position.
    content: bool,
    /// True if the text before the current piece may be cut.
    after_cuttable: bool,
    /// The first of the inline box starts right before the current piece
    /// and the width before it: a cut at the start of a text piece goes
    /// before them, so that the boxes start on the next line.
    box_starts: Option<(usize, f32)>,
}

impl Scan {
    /// Adds a cut with the width before it; false once the cuts reach the
    /// limit.
    fn add(&mut self, cut: Cut, before: f32) -> bool {
        if before > self.limit {
            if self.out.is_empty() {
                self.out.push((cut, before));
            }
            return false;
        }
        self.out.push((cut, before));
        true
    }

    /// Passes piece `i`, which is not text and is `width` wide.
    fn other(&mut self, piece: &Piece, i: usize, width: f32) {
        if matches!(piece, Piece::StartBox(_)) {
            self.box_starts.get_or_insert((i, self.width));
        } else {
            self.box_starts = None;
        }
        self.width += width;
        if matches!(piece, Piece::Atomic(_)) {
            self.content = true;
            self.after_cuttable = false;
        }
    }

    /// Scans text piece `i` of `group` (glyphs `glyphs` of `run`, whose text
    /// item has advance `offset` before them). False once the cuts reach
    /// the limit.
    fn text(
        &mut self,
        group: &GroupText<'_>,
        i: usize,
        run: &Run,
        glyphs: Range<usize>,
        offset: f64,
    ) -> bool {
        let cuttable = match group.ifc.items.get(run.item) {
            Some(InlineItem::Text { style, .. }) => breaks_anywhere(style, self.purpose),
            _ => false,
        };
        let (start, base) = Cut::glyph_in(group.head, i)
            .map_or((glyphs.start, offset), |(g, o)| {
                (g.clamp(glyphs.start, glyphs.end), o)
            });
        let text = &group.ifc.text;
        let mut advance = 0.0;
        for k in start..glyphs.end {
            let glyph = run.glyphs[k];
            let space = text[glyph.cluster..].starts_with([' ', '\t']);
            let boundary = k == start || run.glyphs[k - 1].cluster != glyph.cluster;
            let piece_start = k == glyphs.start;
            // Between two text items, the item before decides (measured in
            // Chromium 148: `aaa<span style="overflow-wrap: anywhere">bbb
            // </span>` 20 px wide gives the lines `aaab` and `bb`, and a
            // min-content size of `aaab`).
            let allowed = if piece_start {
                self.after_cuttable
            } else {
                cuttable
            };
            if allowed
                && boundary
                && self.content
                && !space
                && is_grapheme_boundary(text, glyph.cluster)
            {
                let (cut, before) = if piece_start {
                    let (piece, before) = self.box_starts.unwrap_or((i, self.width));
                    let cut = Cut {
                        piece,
                        glyph: None,
                        offset: 0.0,
                    };
                    (cut, before)
                } else {
                    let cut = Cut {
                        piece: i,
                        glyph: Some(k),
                        offset: base + advance,
                    };
                    (cut, self.width + snapped_width(base, advance))
                };
                if !self.add(cut, before) {
                    return false;
                }
            }
            advance += f64::from(glyph.advance);
            self.content |= !space;
        }
        self.width += snapped_width(base, advance);
        self.after_cuttable = cuttable;
        self.box_starts = None;
        true
    }
}

/// True if `offset` in `text` is a grapheme cluster boundary. Only the 64
/// bytes before it are looked at (a longer run of regional indicators
/// counts as a boundary), so that the cost is bounded.
fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    let start = text.floor_char_boundary(offset.saturating_sub(64));
    let end = text.ceil_char_boundary(offset.saturating_add(64).min(text.len()));
    let Some(window) = text.get(start..end) else {
        return true;
    };
    GraphemeCursor::new(offset, text.len(), true)
        .is_boundary(window, start)
        .unwrap_or(true)
}
