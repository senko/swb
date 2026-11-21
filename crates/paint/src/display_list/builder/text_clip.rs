//! `background-clip: text` (CSS Backgrounds 4 §3.1,
//! <https://drafts.csswg.org/css-backgrounds-4/#background-clip>): the
//! shapes of the text of a box, which clip the layers that say `text`
//! (`layers.rs` applies them; ADR 0046).
//!
//! The text paints in the foreground phase, after the background, so the
//! builder collects the shapes in a walk of the box's descendants before it
//! paints the background ([`Builder::text_mask`]). The shapes are the
//! glyphs of the text, their strokes (when `-webkit-text-stroke-width` is
//! above 0, whatever the stroke color) and the decoration lines, in white;
//! the rasterizer draws them into the mask of a group
//! ([`MaskLayerImage::Content`]). Text shadows are not part of the mask.
//! Measured with Chromium 148 (`tools/probes/css-videojs-2c.json`): the
//! text of a descendant counts, with its decorations, unless the
//! descendant is positioned or forms a stacking context (opacity,
//! transform, filter, mask, `clip-path`, `isolation`, `mix-blend-mode`,
//! containment: its text is not in the mask; `will-change` alone does not
//! take it out); the `overflow` clips of descendants do not clip the
//! shapes; floats, inline blocks, flex and grid items, table cells and
//! pseudo-elements count; a box without text paints no layer that says
//! `text`.

use std::sync::Arc;

use swb_layout::{BoxFragment, Fragment, Point, Rect, ScrollState, TextFragment};
use swb_style::{Position, Rgba, TextDecorationLine, Visibility};

use super::{Builder, Decoration, DisplayItem, child_decorations, decoration_rect};

/// The most shapes of the text that one group clips by. A glyph counts 1
/// (2 if stroked) and each decoration line rectangle counts 1. A box with
/// more paints its layers as `border-box`.
pub(super) const MAX_SHAPES: usize = 100_000;

/// The most shapes that all groups of a display list clip by: the nested
/// boxes of a deep tree each collect the text below them.
pub(super) const MAX_SHAPES_TOTAL: usize = 1_000_000;

/// The shapes of the text of a box.
pub(super) struct TextMask {
    /// `Text`, `TextStroke` and `Rect` items, all in white.
    pub(super) items: Arc<[DisplayItem]>,
    /// An area that contains everything that the items draw.
    pub(super) ink: Rect,
}

/// The shapes found so far.
struct Collected {
    items: Vec<DisplayItem>,
    ink: Option<Rect>,
    /// The shapes counted so far (see [`MAX_SHAPES`]).
    count: usize,
    /// The most shapes that this walk may count: the per-box bound or what
    /// is left of the total, whichever is less.
    limit: usize,
    /// True if the limit is exceeded.
    over: bool,
}

impl Collected {
    /// Counts `n` more shapes; false (and `over` set) if the limit is
    /// exceeded. Call it before the push.
    fn reserve(&mut self, n: usize) -> bool {
        self.count = self.count.saturating_add(n);
        if self.count > self.limit {
            self.over = true;
        }
        !self.over
    }

    fn push(&mut self, item: DisplayItem) {
        if let Some(b) = item.bounds() {
            self.ink = Some(self.ink.map_or(b, |ink| ink.union(&b)));
        }
        self.items.push(item);
    }
}

/// True if the box paints in a layer of its own, apart from its parent's
/// content.
fn paints_in_own_layer(b: &BoxFragment) -> bool {
    b.style.position != Position::Static || swb_layout::forms_stacking_context(b)
}

impl Builder<'_> {
    /// The shapes of the text of `b`, whose parent's border box starts at
    /// `origin`, or `None` if the text has too many shapes (the layers then
    /// paint as `border-box`).
    pub(super) fn text_mask(&mut self, b: &BoxFragment, origin: Point) -> Option<TextMask> {
        let rect = b.border_rect.translate(origin);
        let (child_origin, scroll) = self.scroll.enter(b, rect.origin(), self.offsets);
        let mut found = Collected {
            items: Vec::new(),
            ink: None,
            count: 0,
            limit: MAX_SHAPES.min(MAX_SHAPES_TOTAL.saturating_sub(self.text_clip_shapes)),
            over: false,
        };
        let decorations = child_decorations(b, &[]);
        self.collect_text(b, child_origin, scroll, &decorations, &mut found);
        self.text_clip_shapes = self.text_clip_shapes.saturating_add(found.count);
        if found.over {
            if !self.text_clip_warned {
                self.text_clip_warned = true;
                log::warn!(
                    "background-clip: text: more than {MAX_SHAPES} text shapes in a box \
                     or {MAX_SHAPES_TOTAL} in the document; painting the border box"
                );
            }
            return None;
        }
        Some(TextMask {
            items: found.items.into(),
            ink: found.ink.unwrap_or_default(),
        })
    }

    /// Adds the shapes of the text of the children of `b` (their origin is
    /// `origin`) to `found`.
    fn collect_text(
        &self,
        b: &BoxFragment,
        origin: Point,
        scroll: ScrollState,
        decorations: &[Decoration],
        found: &mut Collected,
    ) {
        // The decoration lines of all ancestors are white shapes: the union
        // paints the same pixels as the lines one by one.
        let lines = decorations
            .iter()
            .fold(TextDecorationLine::empty(), |all, d| all | d.lines);
        for child in b.children.iter() {
            if found.over {
                return;
            }
            match child {
                Fragment::Text(t) => collect_fragment(t, origin, lines, found),
                Fragment::Box(cb) if !paints_in_own_layer(cb) => {
                    let rect = cb.border_rect.translate(origin);
                    let (child_origin, child_scroll) =
                        scroll.enter(cb, rect.origin(), self.offsets);
                    let own = child_decorations(cb, decorations);
                    self.collect_text(cb, child_origin, child_scroll, &own, found);
                }
                Fragment::Box(_) => {}
            }
        }
    }
}

/// Adds the shapes of one text fragment: decoration lines (the union of
/// those of the ancestors, `lines`), the glyphs and their stroke.
fn collect_fragment(
    t: &TextFragment,
    origin: Point,
    lines: TextDecorationLine,
    found: &mut Collected,
) {
    let style = &t.style;
    if style.visibility != Visibility::Visible || t.glyphs.is_empty() {
        return;
    }
    let stroked = style.text_stroke_width > 0.0;
    let shapes = t.glyphs.len() * if stroked { 2 } else { 1 } + lines.bits().count_ones() as usize;
    if !found.reserve(shapes) {
        return;
    }
    let rect = t.rect.translate(origin);
    let baseline = Point::new(rect.x, rect.y + t.baseline);
    for line in [
        TextDecorationLine::UNDERLINE,
        TextDecorationLine::OVERLINE,
        TextDecorationLine::LINE_THROUGH,
    ] {
        if lines.contains(line) {
            found.push(DisplayItem::Rect {
                rect: decoration_rect(line, rect, baseline.y, t.font_size),
                radii: [(0.0, 0.0); 4],
                color: Rgba::WHITE,
            });
        }
    }
    found.push(DisplayItem::Text {
        origin: baseline,
        font: t.font,
        size: t.font_size,
        glyphs: Arc::clone(&t.glyphs),
        color: Rgba::WHITE,
    });
    if stroked {
        found.push(DisplayItem::TextStroke {
            origin: baseline,
            font: t.font,
            size: t.font_size,
            glyphs: Arc::clone(&t.glyphs),
            width: style.text_stroke_width,
            color: Rgba::WHITE,
        });
    }
}
