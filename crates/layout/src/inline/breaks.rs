//! Soft wrap opportunities of an inline formatting context (CSS Text 3 §5,
//! <https://www.w3.org/TR/css-text-3/#line-breaking>), with Chromium's rules
//! at element boundaries. The rules below were measured in Chromium 148;
//! `tests/layout/text-break-boundaries.html` has the cases.
//!
//! [`swb_text::line_breaks`] finds the opportunities in the text. Then:
//!
//! - An opportunity inside text that does not wrap (`white-space`), or
//!   between atomic inlines in such text, does not count. An opportunity
//!   where such text ends counts only if an inline box ends there and the
//!   text before does not end with a space (the parent of the box
//!   decides). So a break after `<span style="white-space: nowrap">a-</span>b`
//!   is allowed, and one before a wrapping box inside text that does not
//!   wrap is not.
//! - There is no opportunity at the start of an inline box with a
//!   `unicode-bidi` other than `normal` (Wikipedia's reference links,
//!   `<bdi>`, elements with `dir`), except after a space. For this, the
//!   text for line breaking gets a bidi control character (line break
//!   class CM) at the start and the end of each such box: UAX #14 does not
//!   break before CM, and an opportunity directly after an opening control
//!   does not count.
//! - A run of spaces that continues from text that wraps into text that does
//!   not wrap (preserved spaces at its start) breaks before the text that
//!   does not wrap.
//!
//! Not done: Chromium also allows a break before a space that follows the
//! end of a wrapping box inside text that does not wrap
//! (`<nobr><span>a</span> b</nobr>`); that needs the removal of spaces at
//! the start of a line.

use std::borrow::Cow;
use std::ops::Range;

use swb_style::{ComputedStyle, Hyphens, UnicodeBidi, WordBreak};
use swb_text::{BreakKind, BreakRules};

use crate::box_tree::{InlineFormattingContext, InlineItem};

/// The opening and closing bidi controls for line breaking (all controls
/// have line break class CM, so the kind does not matter).
const OPEN_CONTROL: char = '\u{2068}';
const CLOSE_CONTROL: char = '\u{2069}';

/// The soft wrap opportunities of a context, and where its shaping
/// context ends.
pub(crate) struct Breaks {
    /// Byte offsets in the context's text before which a line may break,
    /// in increasing order. Offset 0 is never included.
    pub(crate) opportunities: Vec<usize>,
    /// The offsets where the context for shaping ends, in increasing order:
    /// the edges of isolating boxes (`unicode-bidi: isolate`,
    /// `isolate-override` or `plaintext`), and forced line breaks (see
    /// `shaping::shaping_context`).
    pub(crate) context_ends: Vec<usize>,
}

/// The soft wrap opportunities of `ifc` and where its shaping context ends.
pub(crate) fn breaks(ifc: &InlineFormattingContext) -> Breaks {
    let layout = BoundaryLayout::new(ifc);
    Breaks {
        opportunities: wrap_opportunities(ifc, &layout),
        context_ends: layout.context_ends(),
    }
}

/// See [`Breaks::opportunities`].
fn wrap_opportunities(ifc: &InlineFormattingContext, layout: &BoundaryLayout) -> Vec<usize> {
    let text = layout.break_text(&ifc.text);
    // The break rules of each text item, in text order. The opportunity
    // before a character follows the text before it (measured in Chromium
    // 148: `<span style="word-break: break-all">aaa</span>bbb` breaks
    // before `b`, `aaa<span style="word-break: break-all">bbb</span>` does
    // not).
    let rules: Vec<(Range<usize>, BreakRules)> = ifc
        .items
        .iter()
        .filter_map(|item| match item {
            InlineItem::Text { style, range, .. } => Some((range.clone(), break_rules(style))),
            _ => None,
        })
        .collect();
    let rules_at = |position: usize| {
        let offset = layout.original(position).0;
        let i = rules.partition_point(|(r, _)| r.end < offset);
        rules
            .get(i)
            .filter(|(r, _)| r.start < offset)
            .map_or_else(BreakRules::default, |(_, rules)| *rules)
    };
    let mut out: Vec<usize> = Vec::new();
    for (position, kind) in swb_text::line_breaks(&text, rules_at) {
        let (offset, after_open) = layout.original(position);
        if kind != BreakKind::Allowed
            || after_open
            || offset == 0
            || offset >= ifc.text.len()
            || out.last() == Some(&offset)
            || !layout.wraps_at(&ifc.text, offset)
        {
            continue;
        }
        out.push(offset);
    }
    let before_preserved = layout.breaks_before_preserved_spaces(&ifc.text);
    if !before_preserved.is_empty() {
        out.extend(before_preserved);
        out.sort_unstable();
        out.dedup();
    }
    out
}

/// The line break rules of text in `style` (CSS Text 3 §5.2). `word-break:
/// break-word` breaks like `normal`; it only changes what happens to a word
/// that does not fit.
fn break_rules(style: &ComputedStyle) -> BreakRules {
    BreakRules {
        word_break: match style.word_break {
            WordBreak::Normal | WordBreak::BreakWord => swb_text::WordBreak::Normal,
            WordBreak::BreakAll => swb_text::WordBreak::BreakAll,
            WordBreak::KeepAll => swb_text::WordBreak::KeepAll,
        },
        soft_hyphens: style.hyphens != Hyphens::None,
    }
}

/// A bidi control inserted at an offset of the context's text.
#[derive(Clone, Copy, Debug)]
struct Control {
    offset: usize,
    open: bool,
}

/// What happens at the element boundaries of a context: where text does
/// not wrap, where inline boxes end, and where bidi controls go.
struct BoundaryLayout {
    /// The ranges of text that does not wrap, merged where they touch, in
    /// text order.
    no_wrap: Vec<Range<usize>>,
    /// The offsets where an inline box ends, in increasing order.
    box_ends: Vec<usize>,
    /// The bidi controls, in text order.
    controls: Vec<Control>,
    /// The position of each control in the break text.
    positions: Vec<usize>,
    /// The offsets of the edges of isolating boxes, in increasing order.
    isolate_edges: Vec<usize>,
    /// The offsets of forced line breaks, in increasing order.
    line_breaks: Vec<usize>,
}

impl BoundaryLayout {
    fn new(ifc: &InlineFormattingContext) -> Self {
        let mut no_wrap: Vec<Range<usize>> = Vec::new();
        let mut box_ends = Vec::new();
        let mut controls = Vec::new();
        let mut line_breaks = Vec::new();
        let mut isolate_edges = Vec::new();
        // Whether each open inline box gets bidi controls, and whether it
        // isolates.
        let mut open: Vec<(bool, bool)> = Vec::new();
        let mut offset = 0;
        for item in &ifc.items {
            match item {
                InlineItem::Text { style, range, .. } => {
                    if !style.white_space.wraps() {
                        add_range(&mut no_wrap, range.clone());
                    }
                    offset = range.end;
                }
                InlineItem::Atomic { offset: at, inner } => {
                    offset = at + OBJECT_REPLACEMENT_LEN;
                    // Its own `white-space` is the one around it, unless it
                    // sets one.
                    if !inner.base.style.white_space.wraps() {
                        add_range(&mut no_wrap, *at..offset);
                    }
                }
                InlineItem::StartBox { base, .. } => {
                    let bidi = base.style.unicode_bidi;
                    let control = bidi != UnicodeBidi::Normal;
                    let isolates = matches!(
                        bidi,
                        UnicodeBidi::Isolate
                            | UnicodeBidi::IsolateOverride
                            | UnicodeBidi::Plaintext
                    );
                    if control {
                        controls.push(Control { offset, open: true });
                    }
                    if isolates {
                        isolate_edges.push(offset);
                    }
                    open.push((control, isolates));
                }
                InlineItem::EndBox { .. } => {
                    let (control, isolates) = open.pop().unwrap_or_default();
                    if control {
                        controls.push(Control {
                            offset,
                            open: false,
                        });
                    }
                    if isolates {
                        isolate_edges.push(offset);
                    }
                    if box_ends.last() != Some(&offset) {
                        box_ends.push(offset);
                    }
                }
                InlineItem::LineBreak(_) => line_breaks.push(offset),
                InlineItem::Float(_) | InlineItem::AbsolutelyPositioned(_) => {}
            }
        }
        BoundaryLayout {
            isolate_edges,
            line_breaks,
            ..Self::from_parts(no_wrap, box_ends, controls)
        }
    }

    fn from_parts(
        no_wrap: Vec<Range<usize>>,
        box_ends: Vec<usize>,
        controls: Vec<Control>,
    ) -> Self {
        // Each control is `CONTROL_LEN` bytes long.
        let positions = controls
            .iter()
            .enumerate()
            .map(|(i, c)| c.offset + CONTROL_LEN * i)
            .collect();
        BoundaryLayout {
            no_wrap,
            box_ends,
            controls,
            positions,
            isolate_edges: Vec::new(),
            line_breaks: Vec::new(),
        }
    }

    /// See [`Breaks::context_ends`].
    fn context_ends(&self) -> Vec<usize> {
        let mut out = self.isolate_edges.clone();
        out.extend_from_slice(&self.line_breaks);
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The text for line breaking: the context's text with the bidi
    /// controls.
    fn break_text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.controls.is_empty() {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len() + self.controls.len() * CONTROL_LEN);
        let mut copied = 0;
        for control in &self.controls {
            out.push_str(text.get(copied..control.offset).unwrap_or(""));
            copied = control.offset.max(copied);
            out.push(if control.open {
                OPEN_CONTROL
            } else {
                CLOSE_CONTROL
            });
        }
        out.push_str(text.get(copied..).unwrap_or(""));
        Cow::Owned(out)
    }

    /// The offset in the context's text of `position` in the break text,
    /// and whether it directly follows an opening control.
    fn original(&self, position: usize) -> (usize, bool) {
        let before = self.positions.partition_point(|&p| p < position);
        let after_open = before > 0
            && self.controls.get(before - 1).is_some_and(|c| c.open)
            && self.positions.get(before - 1).map(|p| p + CONTROL_LEN) == Some(position);
        (position.saturating_sub(CONTROL_LEN * before), after_open)
    }

    /// The starts of text that does not wrap and starts with preserved
    /// spaces, after a space of text that wraps (`a <span
    /// style="white-space: pre"> b</span>`): the break after the run of
    /// spaces would be inside the text that does not wrap, and Chromium
    /// breaks before it instead (measured in Chromium 148).
    fn breaks_before_preserved_spaces(&self, text: &str) -> Vec<usize> {
        self.no_wrap
            .iter()
            .map(|r| r.start)
            .filter(|&start| {
                start > 0
                    && text.get(..start).is_some_and(|t| t.ends_with([' ', '\t']))
                    && text
                        .get(start..)
                        .is_some_and(|t| t.starts_with([' ', '\t']))
            })
            .collect()
    }

    /// True if the line may break at `offset` as far as `white-space` is
    /// concerned.
    fn wraps_at(&self, text: &str, offset: usize) -> bool {
        let i = self.no_wrap.partition_point(|r| r.end < offset);
        let Some(range) = self.no_wrap.get(i) else {
            return true;
        };
        if range.start >= offset {
            return true;
        }
        if range.end > offset {
            return false;
        }
        // The end of text that does not wrap: the parent of an inline box
        // that ends here decides.
        self.box_ends.binary_search(&offset).is_ok()
            && !text
                .get(..offset)
                .is_some_and(|t| t.ends_with([' ', '\t', '\n']))
    }
}

/// Adds `range` to `ranges` (in text order), merged with the last range if
/// they touch.
fn add_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range),
    }
}

/// The length of U+FFFC (an atomic inline) and of a bidi control in UTF-8.
const OBJECT_REPLACEMENT_LEN: usize = '\u{FFFC}'.len_utf8();
const CONTROL_LEN: usize = OPEN_CONTROL.len_utf8();
const _: () = assert!(CLOSE_CONTROL.len_utf8() == CONTROL_LEN);

#[cfg(test)]
mod tests {
    use super::*;

    fn control(offset: usize, open: bool) -> Control {
        Control { offset, open }
    }

    #[test]
    fn break_text_maps_back_to_the_context_text() {
        // `<i>ab</i><i><i>cd</i></i>`: a control at offset 0, and a closing
        // and two opening controls at offset 2.
        let controls = vec![
            control(0, true),
            control(2, false),
            control(2, true),
            control(2, true),
            control(4, false),
            control(4, false),
        ];
        let layout = BoundaryLayout::from_parts(Vec::new(), Vec::new(), controls);
        let text = layout.break_text("abcd");
        assert_eq!(text, "\u{2068}ab\u{2069}\u{2068}\u{2068}cd\u{2069}\u{2069}");
        assert_eq!(layout.original(0), (0, false));
        assert_eq!(layout.original(3), (0, true));
        assert_eq!(layout.original(5), (2, false));
        assert_eq!(layout.original(8), (2, false));
        assert_eq!(layout.original(11), (2, true));
        assert_eq!(layout.original(14), (2, true));
        assert_eq!(layout.original(15), (3, false));
        assert_eq!(layout.original(22), (4, false));
    }

    #[test]
    fn white_space_at_box_ends() {
        // "a-b c": "a-" does not wrap and its box ends at 2; "c" does not
        // wrap and no box ends at 4.
        let layout = BoundaryLayout::from_parts(vec![0..2, 4..5], vec![2], Vec::new());
        assert!(!layout.wraps_at("a-b c", 1));
        assert!(layout.wraps_at("a-b c", 2));
        assert!(layout.wraps_at("a-b c", 3));
        assert!(layout.wraps_at("a-b c", 4));
        let layout =
            BoundaryLayout::from_parts(std::iter::once(0..2).collect(), Vec::new(), Vec::new());
        assert!(!layout.wraps_at("a-b", 2));
        let layout =
            BoundaryLayout::from_parts(std::iter::once(0..2).collect(), vec![2], Vec::new());
        assert!(!layout.wraps_at("a b", 2));
    }

    #[test]
    fn breaks_before_preserved_spaces_after_a_space() {
        // "hh " then " ii" that does not wrap, "jj" that wraps, then "kk"
        // without a space before it.
        let text = "hh  iijjkk";
        let layout = BoundaryLayout::from_parts(vec![3..6, 8..10], Vec::new(), Vec::new());
        assert_eq!(layout.breaks_before_preserved_spaces(text), vec![3]);
        let layout =
            BoundaryLayout::from_parts(std::iter::once(2..6).collect(), Vec::new(), Vec::new());
        assert_eq!(
            layout.breaks_before_preserved_spaces(text),
            Vec::<usize>::new()
        );
    }
}
