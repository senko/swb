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

mod shaping;

use std::sync::Arc;

use swb_style::{ComputedStyle, Display, TextAlign, VerticalAlign, VerticalAlignKeyword};

use crate::LayoutContext;
use crate::block::{
    BoxEdges, ContainingBlock, LaidOutBlock, apply_relative_position,
    layout_independent_shrink_to_fit,
};
use crate::box_tree::{
    InlineFormattingContext, InlineItem, has_inline_end_edge, has_inline_start_edge,
};
use crate::fonts::{self, LineMetrics};
use crate::fragment::{BoxContent, BoxFragment, Fragment, PositionedGlyph, TextFragment};
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
    let groups = build_groups(ifc, &shaped, &atomics, width);
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
    let atomics = atomic_content_sizes(ctx, ifc);
    let groups = build_groups(ifc, &shaped, &atomics, 0.0);
    let indent = clamp_length(container_style.text_indent.resolve(0.0));
    let mut sizes = ContentSizes::default();
    let mut line = indent;
    for group in &groups {
        sizes.min = sizes.min.max(group.width - group.trailing_space);
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

/// For content sizes: atomic inlines contribute their own content sizes.
/// Represented as an `AtomicLayout` whose width is the max-content width.
fn atomic_content_sizes(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
) -> Vec<Option<AtomicLayout>> {
    ifc.items
        .iter()
        .map(|item| match item {
            InlineItem::Atomic { inner, .. } => {
                let sizes = crate::intrinsic::independent_outer_sizes(ctx, inner);
                Some(AtomicLayout {
                    fragment: None,
                    margin_width: sizes.max,
                    margin_top: 0.0,
                    margin_height: 0.0,
                    baseline: 0.0,
                })
            }
            _ => None,
        })
        .collect()
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
        // margin edge. An inline flex container uses its first baseline
        // (CSS Flexbox 1 §8.5).
        let content_baseline = if style.display == Display::InlineFlex {
            fragment.first_baseline
        } else {
            fragment.last_baseline
        };
        let baseline = match (&fragment.content, content_baseline) {
            (BoxContent::None, Some(b))
                if !style.overflow_x.is_scroll_container()
                    && !style.overflow_y.is_scroll_container() =>
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

fn build_groups(
    ifc: &InlineFormattingContext,
    shaped: &ShapedText,
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
            && shaped.break_before[i]
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
            Piece::LineBreak => group.forced_break_after = true,
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
}

/// Vertical extent of the line content relative to the root baseline.
#[derive(Clone, Copy)]
struct Extent {
    top: f32,
    bottom: f32,
}

impl Extent {
    fn include(&mut self, top: f32, bottom: f32) {
        self.top = self.top.min(top);
        self.bottom = self.bottom.max(bottom);
    }
}

/// The line being built. Positions are relative to the line start (x) and
/// the root baseline (y).
struct LineState {
    root: Strut,
    extent: Extent,
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
    fn new(root: Strut, indent: f32) -> Self {
        LineState {
            root,
            extent: Extent {
                top: -root.above,
                bottom: root.below,
            },
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

    fn push_open(&mut self, open: OpenBox) {
        self.extent.include(
            open.baseline - open.strut.above,
            open.baseline + open.strut.below,
        );
        self.stack.push(open);
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
            Piece::Atomic(_) | Piece::LineBreak => true,
            Piece::StartBox(item) => {
                !is_continued(self.ifc, *item)
                    && inline_box_style(self.ifc, *item).is_some_and(|s| has_inline_start_edge(s))
            }
            Piece::EndBox { start, split } => {
                !split && inline_box_style(self.ifc, *start).is_some_and(|s| has_inline_end_edge(s))
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
        let mut state = LineState::new(self.root_metrics, indent);
        // Re-open boxes that continue from the previous line.
        for (item, style) in std::mem::take(&mut self.open) {
            let (baseline, strut) = state.parent();
            let open = self.open_box(item, &style, state.x, baseline, &strut, true);
            state.push_open(open);
        }

        let piece_range = Self::piece_range(groups, line);
        let trim = self.trailing_space_to_remove(piece_range.clone());
        for i in piece_range {
            match &self.shaped.pieces[i] {
                Piece::StartBox(item) => self.start_box(&mut state, *item),
                Piece::EndBox { split, .. } => {
                    if let Some(open) = state.stack.pop() {
                        let fragment = self.close_box(open, &mut state.x, *split);
                        state.push_child(Fragment::Box(fragment));
                    }
                }
                Piece::Text { .. } => {
                    let trim_end = trim.filter(|(piece, _)| *piece == i).map(|(_, end)| end);
                    self.place_text(&mut state, i, trim_end);
                }
                Piece::Atomic(item) => self.place_atomic(&mut state, *item),
                Piece::LineBreak | Piece::OutOfFlow => {}
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
            self.finish_line(state);
        }
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
        state.push_open(open);
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
        let strut = strut_for(&style, run.metrics);
        state
            .extent
            .include(baseline - strut.above, baseline + strut.below);
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
    /// and appends its fragments.
    fn finish_line(&mut self, mut state: LineState) {
        let line_width = state.x - state.trailing_space;
        let offset = self.align_offset(line_width);
        let line_top = self.y;
        let baseline_y = line_top - state.extent.top;
        for fragment in &mut state.top_level {
            translate(fragment, offset, baseline_y);
        }
        for fragment in &mut state.outside_markers {
            translate(fragment, 0.0, baseline_y);
        }
        if self.first_baseline.is_none() {
            self.first_baseline = Some(baseline_y);
        }
        self.last_baseline = Some(baseline_y);
        self.line_count += 1;
        self.fragments.append(&mut state.top_level);
        self.fragments.append(&mut state.outside_markers);
        self.y = line_top + (state.extent.bottom - state.extent.top);
    }

    /// Appends the fragments of an empty line: zero height at the current
    /// position (Chromium gives its inline boxes zero-height fragments).
    fn finish_empty_line(&mut self, mut state: LineState) {
        for fragment in &mut state.top_level {
            collapse_to_line_top(fragment);
            translate(fragment, 0.0, self.y);
        }
        self.fragments.append(&mut state.top_level);
    }

    /// The last text piece of a line and the end of its glyphs without the
    /// trailing collapsible spaces, which are removed at the end of a line
    /// (CSS Text 3 §4.1.2). `None` if there is nothing to remove.
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
            translate(child, -rect.x, -rect.y);
        }
        let node = match &self.ifc.items[open.item] {
            InlineItem::StartBox { base, .. } => (base.node, base.pseudo),
            _ => (None, None),
        };
        let mut fragment = BoxFragment {
            node: node.0,
            pseudo: node.1,
            style: open.style,
            border_rect: rect,
            border,
            padding,
            content: BoxContent::None,
            children: Arc::new(children),
            first_baseline: None,
            last_baseline: None,
            is_inline: true,
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
        if pending.marker.outside {
            let x = pending.x(shaped.width);
            state.outside_markers.extend(shaped.place(x, 0.0));
        } else {
            for fragment in &mut state.top_level {
                translate(fragment, shaped.width, 0.0);
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

/// Moves a fragment by (`dx`, `dy`).
fn translate(fragment: &mut Fragment, dx: f32, dy: f32) {
    match fragment {
        Fragment::Box(b) => {
            b.border_rect.x += dx;
            b.border_rect.y += dy;
        }
        Fragment::Text(t) => {
            t.rect.x += dx;
            t.rect.y += dy;
        }
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
    let positioned: Vec<PositionedGlyph> = run.glyphs[glyphs]
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
    let node = match &ifc.items[run.item] {
        InlineItem::Text { node, .. } => *node,
        _ => swb_dom::NodeId::DOCUMENT,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{layout_html, rects_of_text};

    fn body(html: &str) -> String {
        format!("<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>{html}")
    }

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
}
