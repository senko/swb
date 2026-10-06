//! Inline layout: line breaking and line box construction.
//!
//! CSS 2.2 §9.4.2 (inline formatting contexts) and §10.8 (line height
//! calculations): <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
//!
//! The context's content is shaped once (see [`shaping`]). Pieces are
//! grouped into unbreakable groups that start at soft wrap opportunities.
//! Lines are filled greedily. Then each line is built: inline boxes get
//! their vertical position from `vertical-align`, the line box height is
//! the extent of all boxes, and fragments are produced.
//!
//! A line without text, atomic inlines, forced breaks and inline box edges
//! is empty (CSS 2.2 §9.4.2): it has zero height, its inline boxes get
//! zero-height fragments, and it does not count as a line box.
//!
//! In quirks and limited-quirks mode, the line height quirks apply as in
//! Chromium (<https://quirks.spec.whatwg.org/#the-line-height-calculation-quirk>,
//! <https://quirks.spec.whatwg.org/#the-blocks-ignore-line-height-quirk>):
//! the strut of the root inline box and of an inline box counts only if
//! the box contains text or a forced line break directly, or (for an
//! inline box) has a border or padding on its start or end side (margins
//! do not count). Lines of list items always have the root strut.

mod shaping;

use std::sync::Arc;

use swb_style::{ComputedStyle, Display, TextAlign, VerticalAlign, VerticalAlignKeyword};

use crate::LayoutContext;
use crate::block::{
    BoxEdges, ContainingBlock, LaidOutBlock, apply_relative_position,
    layout_independent_shrink_to_fit, relative_offset,
};
use crate::box_tree::{
    InlineFormattingContext, InlineItem, has_inline_end_edge, has_inline_start_edge,
};
use crate::fonts::{self, LineMetrics};
use crate::fragment::{BoxContent, BoxFragment, Caret, Fragment, PositionedGlyph, TextFragment};
use crate::geom::{Rect, clamp_length};
use crate::intrinsic::ContentSizes;
use crate::list_marker::{PendingMarker, shape_marker};

use shaping::{Piece, Run};
pub(crate) use shaping::{ShapedText, shape as shape_ifc};

/// The result of laying out an inline formatting context.
pub(crate) struct InlineLayout {
    /// Fragments relative to the container's content box.
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) height: f32,
    /// The number of line boxes that are not empty.
    pub(crate) line_count: usize,
    pub(crate) first_baseline: Option<f32>,
    pub(crate) last_baseline: Option<f32>,
    /// The largest end of the content of a line box (after `text-align`),
    /// relative to the content box: the inline extent of the line boxes
    /// in the scrollable overflow of a scroll container.
    pub(crate) content_right: f32,
}

/// Lays out an inline formatting context in a container of `width`. The
/// pending list markers are placed on the first line box that is not
/// empty; if there is none, they stay in `markers`.
pub(crate) fn layout_inline(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    container_style: &Arc<ComputedStyle>,
    width: f32,
    markers: &mut Vec<PendingMarker<'_>>,
) -> InlineLayout {
    let shaped = ctx.shaped(ifc);
    let cb = ContainingBlock {
        width,
        height: None,
    };
    let atomics = layout_atomics(ctx, ifc, cb);
    let groups = build_groups(ifc, &shaped, &shaped.break_before, &atomics, width);
    let indent = clamp_length(container_style.text_indent.resolve(width));
    let lines = break_lines(&groups, width, indent);

    let root_metrics = strut_metrics(ctx, container_style);
    let mut builder = LineBuilder {
        ctx,
        ifc,
        shaped: &shaped,
        atomics,
        container_style,
        width,
        root_metrics,
        open: Vec::new(),
        fragments: Vec::new(),
        y: 0.0,
        line_count: 0,
        first_baseline: None,
        last_baseline: None,
        content_right: 0.0,
    };
    for (index, line) in lines.iter().enumerate() {
        let empty = !builder.has_content(&groups, line);
        let line_markers = if empty {
            Vec::new()
        } else {
            std::mem::take(markers)
        };
        let line_indent = if index == 0 { indent } else { 0.0 };
        builder.build_line(&groups, line, line_indent, &line_markers, empty);
    }
    InlineLayout {
        fragments: builder.fragments,
        height: builder.y,
        line_count: builder.line_count,
        first_baseline: builder.first_baseline,
        last_baseline: builder.last_baseline,
        content_right: builder.content_right,
    }
}

/// A line box that holds only list markers.
pub(crate) struct MarkerLine {
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) height: f32,
    pub(crate) baseline: f32,
}

/// Lays out a line box that holds only list markers, for a list item
/// without line boxes (the markers then sit at its top).
pub(crate) fn marker_line(
    ctx: &mut LayoutContext<'_>,
    container_style: &ComputedStyle,
    markers: &[PendingMarker<'_>],
) -> MarkerLine {
    let root = strut_metrics(ctx, container_style);
    let mut extent = Extent {
        top: -root.above,
        bottom: root.below,
    };
    let mut placed = Vec::new();
    for pending in markers {
        let shaped = shape_marker(ctx, pending.marker);
        extent.include(-shaped.above, shaped.below);
        let x = pending.x(shaped.width);
        placed.push((shaped, x));
    }
    let baseline = -extent.top;
    MarkerLine {
        fragments: placed
            .into_iter()
            .flat_map(|(shaped, x)| shaped.place(x, baseline))
            .collect(),
        height: extent.bottom - extent.top,
        baseline,
    }
}

/// Min-content and max-content widths of an inline formatting context.
pub(crate) fn content_sizes(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    container_style: &ComputedStyle,
) -> ContentSizes {
    let shaped = ctx.shaped(ifc);
    let (min_atomics, max_atomics) = atomic_content_sizes(ctx, ifc);
    // The table cell width calculation quirk
    // (https://quirks.spec.whatwg.org/#the-table-cell-width-calculation-quirk):
    // in a cell with an auto width, images do not wrap, as if they were
    // no-break spaces (Chromium's sticky images quirk).
    let sticky_images = ctx.quirks
        && container_style.display == Display::TableCell
        && container_style.width.is_auto();
    let break_before = if sticky_images {
        sticky_image_breaks(ifc, &shaped)
    } else {
        shaped.break_before.clone()
    };
    let indent = clamp_length(container_style.text_indent.resolve(0.0));
    let mut sizes = ContentSizes::default();
    // Min-content: the widest unbreakable group, with the atomic inlines at
    // their min-content widths.
    for group in &build_groups(ifc, &shaped, &break_before, &min_atomics, 0.0) {
        sizes.min = sizes.min.max(group.width - group.trailing_space);
    }
    // Max-content: the longest line between forced breaks.
    let mut line = indent;
    for group in &build_groups(ifc, &shaped, &break_before, &max_atomics, 0.0) {
        line += group.width;
        sizes.max = sizes.max.max(line - group.trailing_space);
        if group.forced_break_after {
            line = 0.0;
        }
    }
    sizes
}

/// Lays out every atomic inline once, for the available width.
fn layout_atomics(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    cb: ContainingBlock,
) -> Vec<Option<AtomicLayout>> {
    ifc.items
        .iter()
        .map(|item| match item {
            InlineItem::Atomic { inner, .. } => {
                let laid_out = layout_independent_shrink_to_fit(ctx, inner, cb);
                Some(AtomicLayout::new(laid_out, &inner.base.style, cb.width))
            }
            _ => None,
        })
        .collect()
}

/// For content sizes: atomic inlines contribute their own content sizes,
/// represented as `AtomicLayout`s whose width is the min-content width
/// (first list) or the max-content width (second list).
fn atomic_content_sizes(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
) -> (Vec<Option<AtomicLayout>>, Vec<Option<AtomicLayout>>) {
    let sized = |width: f32| AtomicLayout {
        fragment: None,
        margin_width: width,
        margin_top: 0.0,
        margin_height: 0.0,
        baseline: 0.0,
    };
    let mut min = Vec::with_capacity(ifc.items.len());
    let mut max = Vec::with_capacity(ifc.items.len());
    for item in &ifc.items {
        if let InlineItem::Atomic { inner, .. } = item {
            let sizes = crate::intrinsic::independent_outer_sizes(ctx, inner);
            min.push(Some(sized(sizes.min)));
            max.push(Some(sized(sizes.max)));
        } else {
            min.push(None);
            max.push(None);
        }
    }
    (min, max)
}

/// A laid-out atomic inline.
struct AtomicLayout {
    fragment: Option<BoxFragment>,
    /// Width of the margin box.
    margin_width: f32,
    margin_top: f32,
    /// Height of the margin box.
    margin_height: f32,
    /// Baseline from the top of the margin box.
    baseline: f32,
}

impl AtomicLayout {
    fn new(laid_out: LaidOutBlock, style: &ComputedStyle, cb_width: f32) -> Self {
        let fragment = laid_out.fragment;
        let margin_left = fragment.border_rect.x;
        let margin_right = clamp_length(style.margin_right.resolve(cb_width).unwrap_or(0.0));
        let margin_top = laid_out.margins.start.solve();
        let margin_bottom = laid_out.margins.end.solve();
        let border_height = fragment.border_rect.height;
        // CSS 2.2 §10.8.1: the baseline of an inline-block is the baseline
        // of its last line box, unless it has none or it is a scroll
        // container (`overflow: clip` is not one); then it is the bottom
        // margin edge. An inline flex or grid container uses its first
        // baseline (CSS Flexbox 1 §8.5, CSS Grid 2 §11.6); as a scroll
        // container, clamped to its border box (as in Chromium). An inline
        // table uses the baseline of its first row (CSS 2.2 §17.5.3), also
        // with `overflow` set (Chromium does not make tables scroll
        // containers).
        let flex_or_grid = matches!(style.display, Display::InlineFlex | Display::InlineGrid);
        let table = style.display == Display::InlineTable;
        let content_baseline = if flex_or_grid || table {
            fragment.first_baseline
        } else {
            fragment.last_baseline
        };
        let scroll_container = !table
            && (style.overflow_x.is_scroll_container() || style.overflow_y.is_scroll_container());
        let baseline = match (&fragment.content, content_baseline) {
            (BoxContent::None | BoxContent::Table(_) | BoxContent::Control(_), Some(b))
                if flex_or_grid && scroll_container =>
            {
                margin_top + b.clamp(0.0, border_height.max(0.0))
            }
            (BoxContent::None | BoxContent::Table(_) | BoxContent::Control(_), Some(b))
                if !scroll_container =>
            {
                margin_top + b
            }
            _ => margin_top + border_height + margin_bottom,
        };
        AtomicLayout {
            margin_width: margin_left + fragment.border_rect.width + margin_right,
            margin_top,
            margin_height: margin_top + border_height + margin_bottom,
            baseline,
            fragment: Some(fragment),
        }
    }
}

/// The resolved horizontal edges of an inline box. Line breaking (which
/// measures) and line building (which places) both use it.
#[derive(Clone, Copy, Debug)]
struct InlineBoxEdges {
    margin_left: f32,
    margin_right: f32,
    edges: BoxEdges,
}

impl InlineBoxEdges {
    fn resolve(style: &ComputedStyle, cb_width: f32) -> Self {
        InlineBoxEdges {
            margin_left: clamp_length(style.margin_left.resolve(cb_width).unwrap_or(0.0)),
            margin_right: clamp_length(style.margin_right.resolve(cb_width).unwrap_or(0.0)),
            edges: BoxEdges::resolve(style, cb_width),
        }
    }

    /// The width before the content: left margin, border and padding.
    fn start(&self) -> f32 {
        self.margin_left + self.edges.sum().left
    }

    /// The width after the content: right border, padding and margin.
    fn end(&self) -> f32 {
        self.edges.sum().right + self.margin_right
    }
}

/// The edges of the inline box that starts at item `item`, if it is one.
fn inline_box_edges(
    ifc: &InlineFormattingContext,
    item: usize,
    cb_width: f32,
) -> Option<InlineBoxEdges> {
    inline_box_style(ifc, item).map(|s| InlineBoxEdges::resolve(s, cb_width))
}

/// An unbreakable sequence of pieces.
#[derive(Debug, Default)]
struct Group {
    pieces: std::ops::Range<usize>,
    width: f32,
    trailing_space: f32,
    forced_break_after: bool,
}

/// The soft wrap opportunities of `shaped` with the sticky images quirk:
/// no break before an image unless after a space, and none after an image.
fn sticky_image_breaks(ifc: &InlineFormattingContext, shaped: &ShapedText) -> Vec<bool> {
    let mut breaks = shaped.break_before.clone();
    let is_image = |piece: &Piece| match piece {
        Piece::Atomic(item) => matches!(
            ifc.items.get(*item),
            Some(InlineItem::Atomic { inner, .. })
                if matches!(inner.contents, crate::box_tree::IndependentContents::Replaced(_))
        ),
        _ => false,
    };
    let mut after_image = false;
    let mut after_space = false;
    for (i, piece) in shaped.pieces.iter().enumerate() {
        match piece {
            Piece::StartBox(_) | Piece::EndBox { .. } | Piece::OutOfFlow => continue,
            _ => {}
        }
        if (after_image || (is_image(piece) && !after_space))
            && let Some(b) = breaks.get_mut(i)
        {
            *b = false;
        }
        after_image = is_image(piece);
        after_space = match piece {
            Piece::Text { text, .. } => {
                ifc.text.get(text.clone()).is_some_and(|t| t.ends_with(' '))
            }
            _ => false,
        };
    }
    breaks
}

fn build_groups(
    ifc: &InlineFormattingContext,
    shaped: &ShapedText,
    break_before: &[bool],
    atomics: &[Option<AtomicLayout>],
    cb_width: f32,
) -> Vec<Group> {
    // Every piece belongs to exactly one group, in order. Start-box pieces
    // belong to the group of the content that follows them; end-box pieces
    // to the group of the content before them.
    let mut groups: Vec<Group> = Vec::new();
    let mut current: Option<Group> = None;
    // Start-box pieces waiting for the content they precede.
    let mut pending_start: Option<usize> = None;
    let mut pending_width = 0.0;

    for (i, piece) in shaped.pieces.iter().enumerate() {
        if let Piece::StartBox(item) = piece {
            pending_start.get_or_insert(i);
            if !is_continued(ifc, *item) {
                pending_width += inline_box_edges(ifc, *item, cb_width).map_or(0.0, |e| e.start());
            }
            continue;
        }
        let is_content = matches!(piece, Piece::Text { .. } | Piece::Atomic(_));
        if is_content
            && break_before.get(i).copied().unwrap_or(false)
            && let Some(group) = current.take()
        {
            groups.push(group);
        }
        let group = current.get_or_insert_with(|| {
            let start = pending_start.unwrap_or(i);
            Group {
                pieces: start..start,
                ..Group::default()
            }
        });
        pending_start = None;
        group.width += pending_width;
        pending_width = 0.0;
        match piece {
            Piece::Text {
                width,
                trailing_space,
                ..
            } => {
                group.width += width;
                group.trailing_space = *trailing_space;
            }
            Piece::Atomic(item) => {
                group.width += atomics[*item].as_ref().map_or(0.0, |a| a.margin_width);
                group.trailing_space = 0.0;
            }
            Piece::EndBox { start, split } => {
                if !split {
                    group.width += inline_box_edges(ifc, *start, cb_width).map_or(0.0, |e| e.end());
                }
            }
            Piece::LineBreak(_) => group.forced_break_after = true,
            Piece::StartBox(_) | Piece::OutOfFlow => {}
        }
        group.pieces.end = i + 1;
        if group.forced_break_after
            && let Some(group) = current.take()
        {
            groups.push(group);
        }
    }
    // Start-box pieces at the very end (an empty inline box).
    if let Some(start) = pending_start {
        let group = current.get_or_insert_with(|| Group {
            pieces: start..start,
            ..Group::default()
        });
        group.width += pending_width;
        group.pieces.end = shaped.pieces.len();
    }
    if let Some(group) = current {
        groups.push(group);
    }
    groups
}

fn inline_box_style(ifc: &InlineFormattingContext, item: usize) -> Option<&Arc<ComputedStyle>> {
    match ifc.items.get(item) {
        Some(InlineItem::StartBox { base, .. }) => Some(&base.style),
        _ => None,
    }
}

fn is_continued(ifc: &InlineFormattingContext, item: usize) -> bool {
    matches!(
        ifc.items.get(item),
        Some(InlineItem::StartBox {
            continued: true,
            ..
        })
    )
}

/// A line: a range of groups.
#[derive(Debug, Default, Clone)]
struct Line {
    groups: std::ops::Range<usize>,
}

fn break_lines(groups: &[Group], width: f32, indent: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = indent;
    let mut has_content = false;
    for (i, group) in groups.iter().enumerate() {
        let fits = used + group.width - group.trailing_space <= width + 0.01;
        if has_content && !fits {
            lines.push(Line { groups: start..i });
            start = i;
            used = 0.0;
        }
        used += group.width;
        has_content = true;
        if group.forced_break_after {
            lines.push(Line {
                groups: start..i + 1,
            });
            start = i + 1;
            used = 0.0;
            has_content = false;
        }
    }
    if start < groups.len() {
        lines.push(Line {
            groups: start..groups.len(),
        });
    }
    lines
}

/// The strut of a box: ascent and descent including half-leading, and the
/// font metrics of its content area.
#[derive(Clone, Copy, Debug)]
struct Strut {
    /// Ascent including half-leading.
    above: f32,
    /// Descent including half-leading.
    below: f32,
    metrics: LineMetrics,
    /// The font size of the box.
    font_size: f32,
    /// The used line height of the box.
    line_height: f32,
}

fn strut_metrics(ctx: &mut LayoutContext<'_>, style: &ComputedStyle) -> Strut {
    let font = fonts::primary_font(ctx.fonts, style);
    let metrics = fonts::line_metrics(ctx.fonts, font, style.font_size);
    strut_for(style, metrics)
}

/// The ascent and descent, both including half-leading, of text with style
/// `style` in a font with `metrics`: its contribution to the line height.
pub(crate) fn text_extent(style: &ComputedStyle, metrics: LineMetrics) -> (f32, f32) {
    let strut = strut_for(style, metrics);
    (strut.above, strut.below)
}

/// The strut of a box with style `style` whose text uses a font with
/// `metrics`. Also used for the line-height contribution of text.
fn strut_for(style: &ComputedStyle, metrics: LineMetrics) -> Strut {
    let content = metrics.ascent + metrics.descent;
    let line_height = clamp_length(style.used_line_height(metrics.normal_line_height()));
    let leading = line_height - content;
    // Blink gives the ascent side the floor of half the leading and the
    // descent side the rest (`CalculateLeadingSpace` in LayoutNG).
    let half = (leading / 2.0).floor();
    Strut {
        above: metrics.ascent + half,
        below: metrics.descent + (leading - half),
        metrics,
        font_size: style.font_size,
        line_height,
    }
}

/// An inline box that is open while a line is built.
struct OpenBox {
    item: usize,
    style: Arc<ComputedStyle>,
    /// Border-box left edge in line coordinates.
    start_x: f32,
    /// Baseline position relative to the root baseline (down is positive).
    baseline: f32,
    strut: Strut,
    edges: InlineBoxEdges,
    /// True if the box started on an earlier line.
    continued: bool,
    /// True once something on this line counts for the box's height (its
    /// strut, text, an atomic inline, a child box); for the line height
    /// quirks.
    has_metrics: bool,
    /// True once the box's strut counts on this line (for the line height
    /// quirks).
    has_strut: bool,
    children: Vec<Fragment>,
}

struct LineBuilder<'a, 'b> {
    ctx: &'a mut LayoutContext<'b>,
    ifc: &'a InlineFormattingContext,
    shaped: &'a ShapedText,
    atomics: Vec<Option<AtomicLayout>>,
    container_style: &'a Arc<ComputedStyle>,
    width: f32,
    root_metrics: Strut,
    /// Inline boxes open at the end of the previous line.
    open: Vec<(usize, Arc<ComputedStyle>)>,
    fragments: Vec<Fragment>,
    y: f32,
    line_count: usize,
    first_baseline: Option<f32>,
    last_baseline: Option<f32>,
    /// See [`InlineLayout::content_right`].
    content_right: f32,
}

/// Vertical extent of the line content relative to the root baseline.
#[derive(Clone, Copy)]
struct Extent {
    top: f32,
    bottom: f32,
}

impl Extent {
    /// An extent that includes nothing yet.
    const EMPTY: Extent = Extent {
        top: f32::INFINITY,
        bottom: f32::NEG_INFINITY,
    };

    fn include(&mut self, top: f32, bottom: f32) {
        self.top = self.top.min(top);
        self.bottom = self.bottom.max(bottom);
    }

    /// The extent, or zero at the baseline if it includes nothing.
    fn or_zero(self) -> Extent {
        if self.top > self.bottom {
            Extent {
                top: 0.0,
                bottom: 0.0,
            }
        } else {
            self
        }
    }
}

/// The line being built. Positions are relative to the line start (x) and
/// the root baseline (y).
struct LineState {
    root: Strut,
    extent: Extent,
    /// True if struts count only for boxes with text (the line height
    /// quirks).
    quirky: bool,
    /// [`OpenBox::has_metrics`] for the root inline box.
    root_has_metrics: bool,
    /// [`OpenBox::has_strut`] for the root inline box.
    root_has_strut: bool,
    /// Fragments that are not inside an open inline box.
    top_level: Vec<Fragment>,
    /// Fragments of outside list markers; `text-align` does not move them.
    outside_markers: Vec<Fragment>,
    /// Inline boxes open at the current position.
    stack: Vec<OpenBox>,
    x: f32,
    /// Width of hanging white space at the current end of the line.
    trailing_space: f32,
}

impl LineState {
    fn new(root: Strut, indent: f32, quirky: bool) -> Self {
        LineState {
            root,
            extent: if quirky {
                Extent::EMPTY
            } else {
                Extent {
                    top: -root.above,
                    bottom: root.below,
                }
            },
            quirky,
            root_has_metrics: !quirky,
            root_has_strut: !quirky,
            top_level: Vec::new(),
            outside_markers: Vec::new(),
            stack: Vec::new(),
            x: indent,
            trailing_space: 0.0,
        }
    }

    /// The baseline and strut of the innermost open box (or the root).
    fn parent(&self) -> (f32, Strut) {
        self.stack
            .last()
            .map_or((0.0, self.root), |b| (b.baseline, b.strut))
    }

    /// Opens an inline box; its strut counts unless the line height
    /// quirks apply and `quirky_strut` is false.
    fn push_open(&mut self, mut open: OpenBox, quirky_strut: bool) {
        if !self.quirky || quirky_strut {
            self.include_strut(&mut open);
        }
        self.stack.push(open);
    }

    fn include_strut(&mut self, open: &mut OpenBox) {
        self.extent.include(
            open.baseline - open.strut.above,
            open.baseline + open.strut.below,
        );
        open.has_metrics = true;
        open.has_strut = true;
    }

    /// With the line height quirks: includes the strut of the innermost
    /// open box (or the root), which has text or a line break directly.
    /// For a line break, only if nothing counted for the box yet.
    fn include_parent_strut(&mut self, line_break: bool) {
        if !self.quirky || (line_break && self.parent_has_metrics()) {
            return;
        }
        let (baseline, strut) = self.parent();
        self.extent
            .include(baseline - strut.above, baseline + strut.below);
        self.set_parent_has_metrics();
        match self.stack.last_mut() {
            Some(open) => open.has_strut = true,
            None => self.root_has_strut = true,
        }
    }

    fn parent_has_strut(&self) -> bool {
        self.stack
            .last()
            .map_or(self.root_has_strut, |b| b.has_strut || !self.quirky)
    }

    fn parent_has_metrics(&self) -> bool {
        self.stack
            .last()
            .map_or(self.root_has_metrics, |b| b.has_metrics)
    }

    fn set_parent_has_metrics(&mut self) {
        match self.stack.last_mut() {
            Some(open) => open.has_metrics = true,
            None => self.root_has_metrics = true,
        }
    }

    fn push_child(&mut self, fragment: Fragment) {
        match self.stack.last_mut() {
            Some(open) => open.children.push(fragment),
            None => self.top_level.push(fragment),
        }
    }
}

impl LineBuilder<'_, '_> {
    /// The pieces of a line.
    fn piece_range(groups: &[Group], line: &Line) -> std::ops::Range<usize> {
        if line.groups.is_empty() {
            0..0
        } else {
            groups[line.groups.start].pieces.start..groups[line.groups.end - 1].pieces.end
        }
    }

    /// True if the line is not empty (CSS 2.2 §9.4.2): it has text other
    /// than collapsible spaces, an atomic inline, a forced break, or an
    /// inline box edge with a non-zero margin, border or padding.
    fn has_content(&self, groups: &[Group], line: &Line) -> bool {
        Self::piece_range(groups, line).any(|i| match &self.shaped.pieces[i] {
            Piece::Text { run, text, .. } => {
                let collapses = self
                    .text_style(&self.shaped.runs[*run])
                    .white_space
                    .collapses_spaces();
                let text = self.ifc.text.get(text.clone()).unwrap_or("");
                !text.is_empty() && (!collapses || text.chars().any(|c| c != ' '))
            }
            Piece::Atomic(_) | Piece::LineBreak(_) => true,
            // In quirks mode, margins do not make a line non-empty.
            Piece::StartBox(item) => {
                !is_continued(self.ifc, *item)
                    && inline_box_style(self.ifc, *item).is_some_and(|s| {
                        has_quirky_start_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_start_edge(s))
                    })
            }
            Piece::EndBox { start, split } => {
                !split
                    && inline_box_style(self.ifc, *start).is_some_and(|s| {
                        has_quirky_end_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_end_edge(s))
                    })
            }
            Piece::OutOfFlow => false,
        })
    }

    fn build_line(
        &mut self,
        groups: &[Group],
        line: &Line,
        indent: f32,
        markers: &[PendingMarker<'_>],
        empty: bool,
    ) {
        let quirky =
            self.ctx.line_height_quirks && self.container_style.display != Display::ListItem;
        let mut state = LineState::new(self.root_metrics, indent, quirky);
        // Re-open boxes that continue from the previous line.
        for (item, style) in std::mem::take(&mut self.open) {
            let (baseline, strut) = state.parent();
            let open = self.open_box(item, &style, state.x, baseline, &strut, true);
            state.push_open(open, false);
        }

        let piece_range = Self::piece_range(groups, line);
        let trim = self.trailing_space_to_remove(piece_range.clone());
        let soft_wrap = self.ends_at_soft_wrap(piece_range.clone());
        for i in piece_range {
            match &self.shaped.pieces[i] {
                Piece::StartBox(item) => self.start_box(&mut state, *item),
                Piece::EndBox { split, .. } => {
                    if let Some(mut open) = state.stack.pop() {
                        if state.quirky && !split && has_quirky_end_edge(&open.style) {
                            state.include_strut(&mut open);
                        }
                        if open.has_metrics {
                            state.set_parent_has_metrics();
                        }
                        let fragment = self.close_box(open, &mut state.x, *split);
                        state.push_child(Fragment::Box(fragment));
                    }
                }
                Piece::Text { .. } => {
                    let trim_end = trim.filter(|(piece, _)| *piece == i).map(|(_, end)| end);
                    self.place_text(&mut state, i, trim_end);
                }
                Piece::Atomic(item) => self.place_atomic(&mut state, *item),
                Piece::LineBreak(item) => self.place_line_break(&mut state, *item),
                Piece::OutOfFlow => {}
            }
        }

        // Close boxes that continue on the next line.
        while let Some(open) = state.stack.pop() {
            self.open.insert(0, (open.item, Arc::clone(&open.style)));
            let fragment = self.close_box(open, &mut state.x, true);
            state.push_child(Fragment::Box(fragment));
        }
        for pending in markers {
            self.place_marker(*pending, &mut state);
        }
        if empty {
            self.finish_empty_line(state);
        } else {
            self.finish_line(state, soft_wrap);
        }
    }

    /// True if the line of the pieces `range` ends at a soft wrap: not at
    /// a forced line break and not at the end of the content.
    fn ends_at_soft_wrap(&self, range: std::ops::Range<usize>) -> bool {
        if range.end >= self.shaped.pieces.len() {
            return false;
        }
        let last = self.shaped.pieces.get(range).and_then(|pieces| {
            pieces.iter().rev().find(|p| {
                !matches!(
                    p,
                    Piece::StartBox(_) | Piece::EndBox { .. } | Piece::OutOfFlow
                )
            })
        });
        !matches!(last, Some(Piece::LineBreak(_)))
    }

    fn start_box(&mut self, state: &mut LineState, item: usize) {
        let Some(style) = inline_box_style(self.ifc, item).cloned() else {
            return;
        };
        let (baseline, strut) = state.parent();
        let continued = is_continued(self.ifc, item);
        let open = self.open_box(item, &style, state.x, baseline, &strut, continued);
        if !continued {
            state.x += open.edges.start();
        }
        state.push_open(open, !continued && has_quirky_start_edge(&style));
        state.trailing_space = 0.0;
    }

    /// Places a text piece. `trim_end` is the end of its glyphs without
    /// trailing spaces, if they are removed at the end of the line.
    fn place_text(&mut self, state: &mut LineState, piece: usize, trim_end: Option<usize>) {
        let Piece::Text {
            run,
            glyphs,
            text,
            width,
            trailing_space,
        } = &self.shaped.pieces[piece]
        else {
            return;
        };
        let run = &self.shaped.runs[*run];
        let (baseline, _) = state.parent();
        let style = self.text_style(run);
        // Text whose glyphs are all removed at the end of the line (only
        // collapsible spaces) does not count for the line height (Chromium
        // skips text items of length 0).
        if trim_end != Some(glyphs.start) {
            let strut = strut_for(&style, run.metrics);
            state
                .extent
                .include(baseline - strut.above, baseline + strut.below);
            state.include_parent_strut(false);
        }
        let (glyphs, width, trailing_space) = match trim_end {
            Some(end) => {
                let kept = glyphs.start..end;
                let w: f32 = run.glyphs[kept.clone()].iter().map(|g| g.advance).sum();
                (kept, w, 0.0)
            }
            None => (glyphs.clone(), *width, *trailing_space),
        };
        let fragment = text_fragment(
            self.ifc,
            run,
            glyphs,
            text.clone(),
            &style,
            state.x,
            baseline,
        );
        state.x += width;
        state.trailing_space = trailing_space;
        state.push_child(Fragment::Text(fragment));
    }

    /// Places a forced line break; a `<br>` gets a box of width 0 with the
    /// content area of its parent's font (Chromium; with the line height
    /// quirks, height 0 at the baseline if the parent's strut does not
    /// count).
    fn place_line_break(&mut self, state: &mut LineState, item: usize) {
        state.include_parent_strut(true);
        let Some(InlineItem::LineBreak(Some(base))) = self.ifc.items.get(item) else {
            return;
        };
        let (baseline, strut) = state.parent();
        let (top, height) = if state.parent_has_strut() {
            (
                baseline - strut.metrics.ascent,
                strut.metrics.ascent + strut.metrics.descent,
            )
        } else {
            (baseline, 0.0)
        };
        let mut fragment = crate::block::finish_fragment(
            base,
            Rect::new(state.x, top, 0.0, height),
            &BoxEdges::default(),
            Vec::new(),
            crate::block::Baselines::default(),
        );
        fragment.is_inline = true;
        state.push_child(Fragment::Box(fragment));
    }

    fn place_atomic(&mut self, state: &mut LineState, item: usize) {
        state.trailing_space = 0.0;
        let Some(atomic) = self.atomics.get_mut(item).and_then(Option::take) else {
            return;
        };
        let InlineItem::Atomic { inner, .. } = &self.ifc.items[item] else {
            return;
        };
        let style = &inner.base.style;
        let (parent_baseline, parent_strut) = state.parent();
        let own_line_height = if matches!(style.vertical_align, VerticalAlign::LengthPercentage(_))
        {
            strut_metrics(self.ctx, style).line_height
        } else {
            0.0
        };
        let baseline = parent_baseline
            + baseline_shift(
                &style.vertical_align,
                &parent_strut,
                own_line_height,
                atomic.baseline,
                atomic.margin_height,
            );
        let top = baseline - atomic.baseline;
        state.extent.include(top, top + atomic.margin_height);
        state.set_parent_has_metrics();
        if let Some(mut fragment) = atomic.fragment {
            fragment.border_rect.x += state.x;
            fragment.border_rect.y = top + atomic.margin_top;
            apply_relative_position(&mut fragment, self.containing_block());
            state.push_child(Fragment::Box(fragment));
        }
        state.x += atomic.margin_width;
    }

    /// The containing block of the inline-level boxes of the line.
    fn containing_block(&self) -> ContainingBlock {
        ContainingBlock {
            width: self.width,
            height: None,
        }
    }

    /// Aligns the line horizontally, positions it below the previous line
    /// and appends its fragments. `soft_wrap` is true if the line ends at a
    /// soft wrap opportunity.
    fn finish_line(&mut self, mut state: LineState, soft_wrap: bool) {
        state.extent = state.extent.or_zero();
        let line_width = state.x - state.trailing_space;
        let offset = self.align_offset(line_width);
        self.content_right = self.content_right.max(offset + line_width);
        let line_top = self.y;
        let baseline_y = line_top - state.extent.top;
        for fragment in &mut state.top_level {
            fragment.move_by(offset, baseline_y);
        }
        for fragment in &mut state.outside_markers {
            fragment.move_by(0.0, baseline_y);
        }
        let line_height = state.extent.bottom - state.extent.top;
        let cb = self.containing_block();
        // The white space at the end hangs at a soft wrap, and at a forced
        // break or the end of the content only if it does not fit (CSS
        // Text 3 §4.1.3, as Chromium 148).
        let hangs = state.trailing_space > 0.0 && (soft_wrap || state.x > self.width);
        for fragment in &mut state.top_level {
            set_line_box(fragment, 0.0, line_top, line_height, cb);
            if hangs {
                mark_hanging(fragment, offset + line_width);
            }
        }
        if self.first_baseline.is_none() {
            self.first_baseline = Some(baseline_y);
        }
        self.last_baseline = Some(baseline_y);
        self.line_count += 1;
        self.fragments.append(&mut state.top_level);
        self.fragments.append(&mut state.outside_markers);
        self.y = line_top + line_height;
    }

    /// Appends the fragments of an empty line: zero height at the current
    /// position (Chromium gives its inline boxes zero-height fragments).
    fn finish_empty_line(&mut self, mut state: LineState) {
        for fragment in &mut state.top_level {
            collapse_to_line_top(fragment);
            fragment.move_by(0.0, self.y);
        }
        self.fragments.append(&mut state.top_level);
    }

    /// The last text piece of a line and the end of its glyphs without the
    /// trailing collapsible spaces, which are removed at the end of a line
    /// (CSS Text 3 §4.1.2,
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>). `None` if
    /// there is nothing to remove.
    fn trailing_space_to_remove(&self, pieces: std::ops::Range<usize>) -> Option<(usize, usize)> {
        for i in pieces.rev() {
            match &self.shaped.pieces[i] {
                Piece::Text { run, glyphs, .. } => {
                    let run_data = &self.shaped.runs[*run];
                    if !self.text_style(run_data).white_space.collapses_spaces() {
                        return None;
                    }
                    let mut end = glyphs.end;
                    while end > glyphs.start
                        && self.ifc.text[run_data.glyphs[end - 1].cluster..].starts_with(' ')
                    {
                        end -= 1;
                    }
                    return (end < glyphs.end).then_some((i, end));
                }
                Piece::Atomic(_) => return None,
                _ => {}
            }
        }
        None
    }

    fn text_style(&self, run: &Run) -> Arc<ComputedStyle> {
        match &self.ifc.items[run.item] {
            InlineItem::Text { style, .. } => Arc::clone(style),
            _ => Arc::clone(self.container_style),
        }
    }

    fn open_box(
        &mut self,
        item: usize,
        style: &Arc<ComputedStyle>,
        start_x: f32,
        parent_baseline: f32,
        parent_strut: &Strut,
        continued: bool,
    ) -> OpenBox {
        let strut = strut_metrics(self.ctx, style);
        let shift = baseline_shift(
            &style.vertical_align,
            parent_strut,
            strut.line_height,
            strut.above,
            strut.above + strut.below,
        );
        let edges = InlineBoxEdges::resolve(style, self.width);
        OpenBox {
            item,
            style: Arc::clone(style),
            start_x: if continued {
                start_x
            } else {
                start_x + edges.margin_left
            },
            baseline: parent_baseline + shift,
            strut,
            edges,
            continued,
            has_metrics: false,
            has_strut: false,
            children: Vec::new(),
        }
    }

    /// Finishes an inline box fragment. `split` is true if the box
    /// continues on the next line (no end edge on this line).
    fn close_box(&self, open: OpenBox, x: &mut f32, split: bool) -> BoxFragment {
        let mut border = open.edges.edges.border;
        let mut padding = open.edges.edges.padding;
        if open.continued {
            border.left = 0.0;
            padding.left = 0.0;
        }
        if split {
            border.right = 0.0;
            padding.right = 0.0;
        } else {
            *x += border.right + padding.right;
        }
        let m = open.strut.metrics;
        let top = open.baseline - m.ascent - border.top - padding.top;
        let height = m.ascent + m.descent + border.vertical() + padding.vertical();
        let rect = Rect::new(open.start_x, top, (*x - open.start_x).max(0.0), height);
        if !split {
            *x += open.edges.margin_right;
        }
        let mut children = open.children;
        for child in &mut children {
            child.move_by(-rect.x, -rect.y);
        }
        let (node, pseudo) = match &self.ifc.items[open.item] {
            InlineItem::StartBox { base, .. } => (base.node, base.pseudo),
            _ => (None, None),
        };
        let mut fragment = BoxFragment {
            node,
            pseudo,
            style: open.style,
            border_rect: rect,
            border,
            padding,
            content: BoxContent::None,
            children: Arc::new(children),
            first_baseline: None,
            last_baseline: None,
            is_inline: true,
            scrollable_overflow: None,
            in_positioned_inline: false,
            hanging_from: None,
        };
        apply_relative_position(&mut fragment, self.containing_block());
        fragment
    }

    /// Places a list marker on the line. An outside marker goes to the left
    /// of its list item's content box; an inside marker is the first
    /// content of the line and moves the rest to the right.
    fn place_marker(&mut self, pending: PendingMarker<'_>, state: &mut LineState) {
        let shaped = shape_marker(self.ctx, pending.marker);
        state.extent.include(-shaped.above, shaped.below);
        // With the line height quirks, a marker brings the root strut.
        state.extent.include(-state.root.above, state.root.below);
        if pending.marker.outside {
            let x = pending.x(shaped.width);
            state.outside_markers.extend(shaped.place(x, 0.0));
        } else {
            for fragment in &mut state.top_level {
                fragment.move_by(shaped.width, 0.0);
            }
            state.x += shaped.width;
            state.top_level.extend(shaped.place(0.0, 0.0));
        }
    }

    /// The horizontal offset of a line for `text-align`. `justify` is not
    /// supported and aligns to the start.
    fn align_offset(&self, line_width: f32) -> f32 {
        let free = self.width - line_width;
        if free <= 0.0 {
            return 0.0;
        }
        match self.container_style.text_align {
            TextAlign::Center | TextAlign::WebkitCenter => free / 2.0,
            TextAlign::Right | TextAlign::WebkitRight | TextAlign::End => free,
            _ => 0.0,
        }
    }
}

/// True if an inline box has a border or padding on its start side; with
/// the line height quirks, only these make the box's strut count (Chromium's
/// `IsInlineBoxStartEmpty`: margins do not count in quirks mode).
fn has_quirky_start_edge(style: &ComputedStyle) -> bool {
    style.border_left_width > 0.0 || !style.padding_left.is_zero()
}

/// The end-side version of [`has_quirky_start_edge`].
fn has_quirky_end_edge(style: &ComputedStyle) -> bool {
    style.border_right_width > 0.0 || !style.padding_right.is_zero()
}

/// Records the line box (`line_top` to `line_top + line_height`, in the
/// coordinates of the fragment's parent, offset by `origin_y`) in the text
/// fragments of a line, also inside inline boxes. The line box moves with
/// relatively positioned inline boxes, as their text does. Atomic inlines
/// have their own lines.
fn set_line_box(
    fragment: &mut Fragment,
    origin_y: f32,
    line_top: f32,
    line_height: f32,
    cb: ContainingBlock,
) {
    match fragment {
        Fragment::Text(t) => {
            t.line_top = line_top - (origin_y + t.rect.y);
            t.line_height = line_height;
        }
        Fragment::Box(b) if b.is_inline => {
            let origin_y = origin_y + b.border_rect.y;
            let line_top = line_top + relative_offset(&b.style, cb).1;
            for child in Arc::make_mut(&mut b.children) {
                set_line_box(child, origin_y, line_top, line_height, cb);
            }
        }
        Fragment::Box(_) => {}
    }
}

/// Marks the inline boxes of a line where white space hangs at the end
/// (`BoxFragment::hanging_from`): their content after the end of the line's
/// content does not count in scrollable overflow. `line_end` is that end
/// in the coordinates of the fragment's parent. As in Chromium 148, it
/// does not move with relatively positioned inline boxes or negative
/// margins: all inline content of the line counts only up to the end of
/// the unshifted line.
fn mark_hanging(fragment: &mut Fragment, line_end: f32) {
    let Fragment::Box(b) = fragment else {
        return;
    };
    if !b.is_inline {
        return;
    }
    let inner = line_end - b.border_rect.x;
    b.hanging_from = Some(inner);
    for child in Arc::make_mut(&mut b.children) {
        mark_hanging(child, inner);
    }
}

/// Gives a fragment of an empty line, and its descendants, zero height at
/// the top of the line (y = 0).
fn collapse_to_line_top(fragment: &mut Fragment) {
    match fragment {
        Fragment::Box(b) => {
            b.border_rect.y = 0.0;
            b.border_rect.height = 0.0;
            for child in Arc::make_mut(&mut b.children) {
                collapse_to_line_top(child);
            }
        }
        Fragment::Text(t) => {
            t.rect.y = 0.0;
            t.rect.height = 0.0;
        }
    }
}

/// The baseline shift of a box relative to its parent's baseline, for
/// `vertical-align` (CSS 2.2 §10.8.1). Down is positive. `line_height` is
/// the box's own used line height (the basis of percentages), `above` the
/// distance from the box's baseline to its top, `height` its total height.
/// `top` and `bottom` are not supported and align as `baseline`.
fn baseline_shift(
    align: &VerticalAlign,
    parent: &Strut,
    line_height: f32,
    above: f32,
    height: f32,
) -> f32 {
    let shift = match align {
        VerticalAlign::Keyword(k) => match k {
            VerticalAlignKeyword::Baseline
            | VerticalAlignKeyword::Top
            | VerticalAlignKeyword::Bottom => 0.0,
            // As Blink: the parent's font size / 5 + 1 down, / 3 + 1 up.
            VerticalAlignKeyword::Sub => parent.font_size / 5.0 + 1.0,
            VerticalAlignKeyword::Super => -(parent.font_size / 3.0 + 1.0),
            VerticalAlignKeyword::TextTop => above - parent.metrics.ascent,
            VerticalAlignKeyword::TextBottom => parent.metrics.descent - (height - above),
            VerticalAlignKeyword::Middle => above - height / 2.0 - parent.metrics.x_height / 2.0,
        },
        VerticalAlign::LengthPercentage(lp) => -lp.resolve(line_height),
    };
    clamp_length(shift)
}

fn text_fragment(
    ifc: &InlineFormattingContext,
    run: &Run,
    glyphs: std::ops::Range<usize>,
    text: std::ops::Range<usize>,
    style: &Arc<ComputedStyle>,
    x: f32,
    baseline: f32,
) -> TextFragment {
    let mut pen = 0.0;
    let positioned: Vec<PositionedGlyph> = run.glyphs[glyphs.clone()]
        .iter()
        .map(|g| {
            let p = PositionedGlyph {
                id: g.id,
                x: pen + g.x_offset,
                y: -g.y_offset,
            };
            pen += g.advance;
            p
        })
        .collect();
    let (node, carets) = match &ifc.items[run.item] {
        InlineItem::Text {
            node,
            range,
            source,
            ..
        } => {
            let carets = source.as_ref().map_or_else(Vec::new, |map| {
                carets(run, glyphs, |offset| {
                    map.node_offset(offset.saturating_sub(range.start))
                })
            });
            (*node, carets)
        }
        _ => (swb_dom::NodeId::DOCUMENT, Vec::new()),
    };
    TextFragment {
        node,
        style: Arc::clone(style),
        rect: Rect::new(
            x,
            baseline - run.metrics.ascent,
            pen,
            run.metrics.ascent + run.metrics.descent,
        ),
        baseline: run.metrics.ascent,
        font: run.font,
        font_size: run.font_size,
        glyphs: positioned.into(),
        text: Arc::from(ifc.text.get(text).unwrap_or("")),
        carets: carets.into(),
        line_top: 0.0,
        line_height: run.metrics.ascent + run.metrics.descent,
    }
}

/// The caret stops of glyphs `glyphs` of `run`: one at the start of each
/// cluster and one at the end. `node_offset` maps an offset in the
/// context's text to an offset in the text node.
fn carets(
    run: &Run,
    glyphs: std::ops::Range<usize>,
    node_offset: impl Fn(usize) -> u32,
) -> Vec<Caret> {
    let mut out: Vec<Caret> = Vec::new();
    let mut pen = 0.0;
    let mut cluster = None;
    for g in run.glyphs.get(glyphs.clone()).unwrap_or_default() {
        if cluster != Some(g.cluster) {
            cluster = Some(g.cluster);
            out.push(Caret {
                offset: node_offset(g.cluster),
                x: pen,
            });
        }
        pen += g.advance;
    }
    // The end: the next glyph's cluster (a removed trailing space, or the
    // next piece), or the end of the run.
    let end = run
        .glyphs
        .get(glyphs.end)
        .map_or(run.text.end, |g| g.cluster);
    out.push(Caret {
        offset: node_offset(end),
        x: pen,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{body, layout_html, rects_of_text};

    #[test]
    fn empty_inline_boxes_make_no_line_box() {
        let l = layout_html(&body(
            "<div id=d> <span id=s> </span> <b></b> </div><p id=p>x",
        ));
        let d = l.rect("d");
        assert_eq!(d.height, 0.0);
        assert_eq!(l.rect("s"), Rect::new(0.0, d.y, 0.0, 0.0));
    }

    #[test]
    fn preserved_tabs_hang_at_the_end_of_a_line() {
        // Chromium 148: the tabs hang, so "b" starts the second line.
        let l = layout_html(&body(
            "<div style='width:100px;white-space:pre-wrap;font:16px monospace'>\
             aaaaa aaaa\t\t<span id=b>b</span></div>",
        ));
        let b = l.rect("b");
        assert_eq!(b.x, 0.0);
        assert!(b.y > 10.0, "{b:?}");
    }

    #[test]
    fn inline_box_with_edges_makes_a_line_box() {
        let l = layout_html(&body(
            "<div><span id=s style='border:2px solid; padding:3px'></span>\
             <p id=p style='margin:0'>x</p></div>",
        ));
        assert_eq!(l.rect("s").width, 10.0);
        assert_eq!(l.rect("p").y, 20.0);
    }

    #[test]
    fn inline_block_baseline_is_its_last_line() {
        let l = layout_html(&body(
            "<div><span style='display:inline-block'>a<br>b</span><span>X</span></div>",
        ));
        let texts = l.texts();
        assert_eq!(
            rects_of_text(&texts, "X")[0].y,
            rects_of_text(&texts, "b")[0].y
        );
    }

    #[test]
    fn relative_positioning_of_inline_level_boxes() {
        let html = |position: &str| {
            body(&format!(
                "<div>a<span id=r style='{position}; top:10px; left:5px'>rel</span>\
                 <span id=ib style='display:inline-block; {position}; top:10px'>ib</span></div>"
            ))
        };
        let moved = layout_html(&html("position:relative"));
        let still = layout_html(&html("position:static"));
        let (r, r0) = (moved.rect("r"), still.rect("r"));
        assert_eq!((r.x - r0.x, r.y - r0.y), (5.0, 10.0));
        let (ib, ib0) = (moved.rect("ib"), still.rect("ib"));
        assert_eq!((ib.x - ib0.x, ib.y - ib0.y), (0.0, 10.0));
    }

    #[test]
    fn sub_and_super_shift_by_the_parent_font_size() {
        let metrics = LineMetrics {
            ascent: 28.0,
            descent: 7.0,
            line_gap: 0.0,
            x_height: 15.0,
        };
        let mut style = ComputedStyle::clone(&ComputedStyle::initial());
        style.font_size = 30.0;
        let parent = strut_for(&style, metrics);
        let sub = VerticalAlign::Keyword(VerticalAlignKeyword::Sub);
        let sup = VerticalAlign::Keyword(VerticalAlignKeyword::Super);
        assert_eq!(baseline_shift(&sub, &parent, 0.0, 0.0, 0.0), 7.0);
        assert_eq!(baseline_shift(&sup, &parent, 0.0, 0.0, 0.0), -11.0);
        let percent = VerticalAlign::LengthPercentage(swb_style::LengthPercentage::Percent(0.5));
        assert_eq!(baseline_shift(&percent, &parent, 40.0, 0.0, 0.0), -20.0);
    }

    /// The text fragments: text and caret offsets.
    fn carets_of(l: &crate::test_support::TestLayout) -> Vec<(String, Vec<u32>)> {
        use crate::FragmentRef;
        let mut out = Vec::new();
        l.tree.walk(|f, _| {
            if let FragmentRef::Text(t) = f {
                let offsets = t.carets.iter().map(|c| c.offset).collect();
                out.push((t.text.to_string(), offsets));
            }
        });
        out
    }

    #[test]
    fn carets_map_to_node_offsets() {
        let l = layout_html("<p>  ab \n cd</p>");
        // One fragment per piece between wrap opportunities.
        assert_eq!(
            carets_of(&l),
            vec![
                ("ab ".to_owned(), vec![2, 3, 4, 7]),
                ("cd".to_owned(), vec![7, 8, 9])
            ]
        );
        let l = layout_html("<p>ab</p>");
        let mut xs = Vec::new();
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Text(t) = f {
                xs = t.carets.iter().map(|c| c.x).collect();
                assert_eq!(t.offset_at(-5.0), Some(0));
                assert_eq!(t.offset_at(t.rect.width + 5.0), Some(2));
                assert_eq!(t.x_range(0, 2), Some((0.0, t.rect.width)));
                assert_eq!(t.x_range(1, 1), None);
            }
        });
        assert!(xs.len() == 3 && xs[0] == 0.0 && xs[0] < xs[1] && xs[1] < xs[2]);
    }

    #[test]
    fn wrapped_lines_end_before_the_removed_space() {
        let l = layout_html("<p style='width:0'>aa bb</p>");
        let carets = carets_of(&l);
        assert_eq!(carets.len(), 2);
        assert_eq!(carets[0].1, vec![0, 1, 2]);
        assert_eq!(carets[1].1, vec![3, 4, 5]);
    }

    #[test]
    fn generated_content_has_no_carets() {
        let l = layout_html("<style>p::before { content: 'x' }</style><ol><li>a</ol><p>b</p>");
        let carets = carets_of(&l);
        let generated: Vec<_> = carets.iter().filter(|(_, c)| c.is_empty()).collect();
        let text: Vec<_> = carets.iter().filter(|(_, c)| !c.is_empty()).collect();
        assert_eq!(generated.len(), 2, "{carets:?}");
        assert_eq!(text.len(), 2, "{carets:?}");
    }

    #[test]
    fn text_fragments_know_their_line_box() {
        // One line: the paragraph's box is the line box.
        let l = layout_html(
            "<p id=p style='margin:0;font-size:16px;line-height:40px'>a <b style='font-size:8px'>b</b></p>",
        );
        let p = l.rect("p");
        let mut lines = Vec::new();
        l.tree.walk(|f, origin| {
            if let crate::FragmentRef::Text(t) = f {
                lines.push((origin.y + t.rect.y + t.line_top, t.line_height));
            }
        });
        assert_eq!(lines.len(), 2);
        for (top, height) in lines {
            assert_eq!((top, height), (p.y, p.height));
        }
    }

    #[test]
    fn line_boxes_move_with_relatively_positioned_text() {
        let l = layout_html(
            "<p id=p style='margin:0;line-height:30px'>aaa <span style='position:relative;top:25px'>bbb</span></p>",
        );
        let p = l.rect("p");
        let mut tops = Vec::new();
        l.tree.walk(|f, origin| {
            if let crate::FragmentRef::Text(t) = f {
                tops.push(origin.y + t.rect.y + t.line_top);
            }
        });
        assert_eq!(tops, vec![p.y, p.y + 25.0]);
    }
}
