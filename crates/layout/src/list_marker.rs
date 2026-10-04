//! List item markers: their text and their placement.
//!
//! Text: <https://www.w3.org/TR/css-lists-3/#text-markers>. Like Chromium,
//! a symbol marker is followed by a space and a counter marker by ". ".
//!
//! Placement (as in Chromium): a marker sits on the first line box in its
//! list item's subtree. If the list item's first in-flow child before any
//! line box is a box with its own formatting context that has a baseline
//! (a flex container, a block with `overflow: hidden`), the marker is
//! aligned with that baseline. Otherwise, if the list item has no line
//! box, the marker sits in a line box at the list item's top. An outside
//! marker is placed to the left of its list item's content box.

use std::sync::Arc;

use swb_style::ListStyleType;

use crate::LayoutContext;
use crate::box_tree::Marker;
use crate::fonts;
use crate::fragment::{Fragment, PositionedGlyph, TextFragment};
use crate::geom::Rect;

/// A marker that waits for the first line box of its list item.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingMarker<'a> {
    pub(crate) marker: &'a Marker,
    /// The distance from the left edge of the list item's content box to
    /// the left edge of the content box of the container being laid out.
    pub(crate) inset: f32,
}

impl PendingMarker<'_> {
    /// The x of the marker's left edge, relative to the content box of the
    /// container being laid out, for a marker of `width`. An outside marker
    /// ends at the list item's content edge; an inside marker that is not
    /// on a line starts there.
    pub(crate) fn x(&self, width: f32) -> f32 {
        if self.marker.outside {
            -self.inset - width
        } else {
            -self.inset
        }
    }
}

/// Adds `dx` to the inset of every pending marker (when the layout moves
/// into, or with a negative `dx` out of, a box whose content box is `dx`
/// to the right).
pub(crate) fn shift_markers(markers: &mut [PendingMarker<'_>], dx: f32) {
    for m in markers {
        m.inset += dx;
    }
}

/// A shaped marker: text fragments relative to the marker's left edge and
/// its baseline.
pub(crate) struct ShapedMarker {
    fragments: Vec<TextFragment>,
    /// The advance of the marker text.
    pub(crate) width: f32,
    /// The ascent and descent of the marker's line, with half-leading.
    pub(crate) above: f32,
    pub(crate) below: f32,
}

impl ShapedMarker {
    /// The fragments with the marker's left edge at `x` and its baseline
    /// at `baseline`.
    pub(crate) fn place(self, x: f32, baseline: f32) -> impl Iterator<Item = Fragment> {
        self.fragments.into_iter().map(move |mut t| {
            t.rect.x += x;
            t.rect.y += baseline;
            Fragment::Text(t)
        })
    }
}

/// Shapes the text of a marker.
pub(crate) fn shape_marker(ctx: &mut LayoutContext<'_>, marker: &Marker) -> ShapedMarker {
    let style = &marker.base.style;
    let families = fonts::family_names(&style.font_family);
    let query = fonts::query(style, &families);
    let runs = ctx.fonts.itemize(&marker.text, &query);
    let options = swb_text::ShapeOptions::default();
    let mut shaped = ShapedMarker {
        fragments: Vec::new(),
        width: 0.0,
        above: 0.0,
        below: 0.0,
    };
    for run in runs {
        let text = &marker.text[run.range.clone()];
        let glyph_run = ctx.fonts.shape(run.font, style.font_size, text, &options);
        let metrics = fonts::line_metrics(ctx.fonts, run.font, style.font_size);
        let (above, below) = crate::inline::text_extent(style, metrics);
        shaped.above = shaped.above.max(above);
        shaped.below = shaped.below.max(below);
        let mut pen = 0.0;
        let glyphs: Vec<PositionedGlyph> = glyph_run
            .glyphs
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
            .collect();
        shaped.fragments.push(TextFragment {
            node: marker.base.node.unwrap_or(swb_dom::NodeId::DOCUMENT),
            style: Arc::clone(style),
            rect: Rect::new(
                shaped.width,
                -metrics.ascent,
                glyph_run.advance,
                metrics.ascent + metrics.descent,
            ),
            baseline: metrics.ascent,
            font: run.font,
            font_size: style.font_size,
            glyphs: glyphs.into(),
            text: Arc::from(text),
            carets: Arc::from([]),
            line_top: 0.0,
            line_height: metrics.ascent + metrics.descent,
        });
        shaped.width += glyph_run.advance;
    }
    shaped
}

/// The fragments of pending markers aligned with a baseline at
/// `baseline` (relative to the content box of the container being laid
/// out).
pub(crate) fn markers_at_baseline(
    ctx: &mut LayoutContext<'_>,
    markers: &[PendingMarker<'_>],
    baseline: f32,
) -> Vec<Fragment> {
    let mut out = Vec::new();
    for pending in markers {
        let shaped = shape_marker(ctx, pending.marker);
        let x = pending.x(shaped.width);
        out.extend(shaped.place(x, baseline));
    }
    out
}

/// The marker text for a list item with this style and number.
pub(crate) fn marker_text(style: ListStyleType, number: i64) -> String {
    match style {
        ListStyleType::None => String::new(),
        ListStyleType::Disc => "\u{2022} ".to_owned(),
        ListStyleType::Circle => "\u{25E6} ".to_owned(),
        ListStyleType::Square => "\u{25AA} ".to_owned(),
        ListStyleType::DisclosureOpen => "\u{25BE} ".to_owned(),
        ListStyleType::DisclosureClosed => "\u{25B8} ".to_owned(),
        ListStyleType::Decimal => format!("{number}. "),
        ListStyleType::DecimalLeadingZero => {
            if (0..10).contains(&number) {
                format!("0{number}. ")
            } else {
                format!("{number}. ")
            }
        }
        ListStyleType::LowerRoman => format!("{}. ", roman(number).to_lowercase()),
        ListStyleType::UpperRoman => format!("{}. ", roman(number)),
        ListStyleType::LowerAlpha => format!("{}. ", alphabetic(number, b'a')),
        ListStyleType::UpperAlpha => format!("{}. ", alphabetic(number, b'A')),
        ListStyleType::LowerGreek => format!("{}. ", greek(number)),
    }
}

/// Roman numerals for 1..=3999; other values fall back to decimal.
fn roman(n: i64) -> String {
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    if !(1..=3999).contains(&n) {
        return n.to_string();
    }
    let mut n = n;
    let mut out = String::new();
    for (value, symbol) in TABLE {
        while n >= value {
            out.push_str(symbol);
            n -= value;
        }
    }
    out
}

/// Alphabetic numbering: a, b, ..., z, aa, ab, ...; non-positive values
/// fall back to decimal.
fn alphabetic(n: i64, first: u8) -> String {
    if n < 1 {
        return n.to_string();
    }
    let mut n = n;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push(first + (n % 26) as u8);
        n /= 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// Lower Greek numbering (alphabetic over α..ω without final sigma).
fn greek(n: i64) -> String {
    const LETTERS: [char; 24] = [
        'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ',
        'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
    ];
    if n < 1 {
        return n.to_string();
    }
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(LETTERS[(n % 24) as usize]);
        n /= 24;
    }
    out.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        assert_eq!(marker_text(ListStyleType::Decimal, 3), "3. ");
        assert_eq!(marker_text(ListStyleType::Disc, 1), "\u{2022} ");
        assert_eq!(marker_text(ListStyleType::UpperRoman, 1994), "MCMXCIV. ");
        assert_eq!(marker_text(ListStyleType::LowerAlpha, 28), "ab. ");
        assert_eq!(marker_text(ListStyleType::LowerGreek, 2), "β. ");
        assert_eq!(marker_text(ListStyleType::DecimalLeadingZero, 7), "07. ");
    }
}
