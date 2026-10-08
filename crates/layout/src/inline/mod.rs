//! Inline layout: line breaking and line box construction.
//!
//! CSS 2.2 §9.4.2 (inline formatting contexts) and §10.8 (line height
//! calculations): <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
//!
//! The context's content is shaped once (see [`shaping`]) and split at soft
//! wrap opportunities (see [`breaks`]). Pieces are grouped into unbreakable
//! groups that start at soft wrap opportunities. A group that does not fit
//! on a line by itself is cut if `overflow-wrap` or `word-break` allow it
//! (see [`overflow`]).
//! Lines are filled greedily, one at a time, in the layout opportunity
//! next to the floats of the block formatting context (CSS 2.2 §9.5:
//! line boxes are shortened by floats). Then each line is built: inline
//! boxes get their vertical position from `vertical-align`, the line box
//! height is the extent of all boxes, and fragments are produced.
//!
//! Floats next to and inside lines follow Chromium's `LayoutNG`: a line is
//! tried in each layout opportunity in order (`floats.rs`) and moves down
//! when its content or its height does not fit. A float that starts a line
//! is placed before the line. A float after content on a line is placed on
//! that line if it fits beside the content before it (the line becomes
//! shorter), otherwise below the line. A `<br>` with `clear` moves the
//! next line below the cleared floats.
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

mod breaks;
mod caps;
mod overflow;
mod shaping;

use std::sync::Arc;

use swb_style::{ComputedStyle, Display, TextAlign, VerticalAlign, VerticalAlignKeyword};

use crate::LayoutContext;
use crate::block::{
    BoxEdges, ContainingBlock, Flow, FlowY, LaidOutBlock, apply_relative_position, layout_float,
    layout_independent_shrink_to_fit, margin_or_zero, relative_offset,
};
use crate::box_tree::{
    InlineFormattingContext, InlineItem, has_inline_end_edge, has_inline_start_edge,
};
use crate::floats::{EPSILON, FloatBox, Opportunity, PendingFloat};
use crate::fonts::{self, LineMetrics};
use crate::fragment::{BoxContent, BoxFragment, Caret, Fragment, PositionedGlyph, TextFragment};
use crate::geom::{Point, Rect, clamp_length};
use crate::intrinsic::ContentSizes;
use crate::list_marker::{PendingMarker, shape_marker};
use crate::positioned::{StaticParent, placeholder};

use overflow::{Cut, GroupText, Purpose};
use shaping::{Piece, Run, snapped_width};
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
    /// The right and bottom margin edges of the floats as in-flow content
    /// in the scrollable overflow of a scroll container (as measured in
    /// Chromium 148): in a context without line boxes, the floats count
    /// like floats among blocks; next to line boxes, only the floats before
    /// the content of a line count, and only with their right margin edge.
    /// The other floats count only as descendants.
    pub(crate) in_flow_floats: crate::geom::Size,
}

/// Lays out an inline formatting context in a container of `width` whose
/// content box is at `flow` in the block formatting context. The pending
/// list markers are placed on the first line box that is not empty; if
/// there is none, they stay in `markers`.
pub(crate) fn layout_inline(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    container_style: &Arc<ComputedStyle>,
    cb: ContainingBlock,
    markers: &mut Vec<PendingMarker<'_>>,
    flow: Flow,
) -> InlineLayout {
    let shaped = ctx.shaped(ifc);
    let width = cb.width;
    let atomics = layout_atomics(ctx, ifc, cb);
    let floats = layout_floats(ctx, ifc, cb, flow.x);
    let mut groups = build_groups(ifc, &shaped, &shaped.break_before, &atomics, width);
    let indent = clamp_length(container_style.text_indent.resolve(width));

    let root_metrics = strut_metrics(ctx, container_style);
    let mut builder = LineBuilder {
        ctx,
        ifc,
        shaped: &shaped,
        atomics,
        floats,
        container_style,
        width,
        cb,
        flow_x: flow.x,
        root_metrics,
        open: Vec::new(),
        fragments: Vec::new(),
        y: 0.0,
        line_count: 0,
        first_baseline: None,
        last_baseline: None,
        content_right: 0.0,
        in_flow_floats: crate::geom::Size::default(),
    };
    if ifc.is_empty() {
        builder.layout_empty(&groups, indent, flow);
    } else {
        // The first line box resolves the position of the container.
        let content_y = match flow.y {
            FlowY::Resolved(y) => y,
            FlowY::Pending { strut, .. } => builder.ctx.bfc().resolve(strut),
        };
        builder.layout_lines(&mut groups, indent, content_y, markers);
    }
    InlineLayout {
        fragments: builder.fragments,
        height: builder.y,
        line_count: builder.line_count,
        first_baseline: builder.first_baseline,
        last_baseline: builder.last_baseline,
        content_right: builder.content_right,
        in_flow_floats: builder.in_flow_floats,
    }
}

/// [`layout_inline`] for a container that establishes a new block
/// formatting context (the text of a form control).
pub(crate) fn layout_inline_root(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    container_style: &Arc<ComputedStyle>,
    width: f32,
) -> InlineLayout {
    ctx.push_bfc();
    let cb = ContainingBlock {
        width,
        height: None,
    };
    let layout = layout_inline(ctx, ifc, container_style, cb, &mut Vec::new(), Flow::ROOT);
    ctx.pop_bfc();
    layout
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
    let floats = float_content_sizes(ctx, ifc);
    let mut sizes = ContentSizes::default();
    // Min-content: the widest unbreakable group, with the atomic inlines at
    // their min-content widths, or the widest float. Groups with text that
    // breaks anywhere count with their widest grapheme cluster.
    for group in &build_groups(ifc, &shaped, &break_before, &min_atomics, 0.0) {
        let whole = group.width - group.trailing_space;
        let min = if group.cuttable {
            let text = GroupText {
                ifc,
                shaped: &shaped,
                pieces: group.pieces.clone(),
                head: None,
            };
            let width_of = |i| piece_extent(ifc, &shaped, &min_atomics, 0.0, i);
            text.min_content(group.width, group.trailing_space, width_of)
                .unwrap_or(whole)
        } else {
            whole
        };
        sizes.min = sizes.min.max(min);
    }
    for float in floats.iter().flatten() {
        sizes.min = sizes.min.max(float.min);
    }
    // Max-content: the longest line between forced breaks; the floats on a
    // line add their widths, and a float that clears an earlier float on
    // the line starts a new line (Chromium).
    let mut line = indent;
    let mut line_floats = LineFloats::default();
    for group in &build_groups(ifc, &shaped, &break_before, &max_atomics, 0.0) {
        line += group.width;
        if !floats.is_empty() {
            for piece in group.pieces.clone() {
                if let Some(Piece::OutOfFlow(item)) = shaped.pieces.get(piece)
                    && let Some(ended) = line_floats.add(ifc, &floats, *item)
                {
                    sizes.max = sizes.max.max(line + ended);
                }
            }
        }
        sizes.max = sizes
            .max
            .max(line - group.trailing_space + line_floats.width());
        if group.forced_break_after {
            line = 0.0;
            line_floats = LineFloats::default();
        }
    }
    sizes
}

/// The max-content widths of the left and right floats on a line, for
/// [`content_sizes`].
#[derive(Default)]
struct LineFloats {
    left: f32,
    right: f32,
}

impl LineFloats {
    fn width(&self) -> f32 {
        self.left + self.right
    }

    /// Adds item `item` to the line if it is a float (`floats` holds the
    /// sizes of the floats by item). A float that clears an earlier float
    /// of the line starts a new line (Chromium): then returns the width of
    /// the floats of the line it ends.
    fn add(
        &mut self,
        ifc: &InlineFormattingContext,
        floats: &[Option<ContentSizes>],
        item: usize,
    ) -> Option<f32> {
        let (Some(Some(float)), Some(InlineItem::Float(inner))) =
            (floats.get(item), ifc.items.get(item))
        else {
            return None;
        };
        let style = &inner.base.style;
        let clears = match style.clear {
            swb_style::Clear::None => false,
            swb_style::Clear::Left => self.left > 0.0,
            swb_style::Clear::Right => self.right > 0.0,
            swb_style::Clear::Both => self.width() > 0.0,
        };
        let ended = clears.then(|| self.width());
        if clears {
            *self = LineFloats::default();
        }
        if style.float == swb_style::Float::Right {
            self.right += float.max;
        } else {
            self.left += float.max;
        }
        ended
    }
}

/// The margin-box content sizes of the floats of an inline formatting
/// context, indexed by item.
fn float_content_sizes(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
) -> Vec<Option<ContentSizes>> {
    if !has_floats(ifc) {
        return Vec::new();
    }
    ifc.items
        .iter()
        .map(|item| match item {
            InlineItem::Float(inner) => Some(crate::intrinsic::independent_outer_sizes(ctx, inner)),
            _ => None,
        })
        .collect()
}

/// True if an inline formatting context has floats.
fn has_floats(ifc: &InlineFormattingContext) -> bool {
    ifc.items
        .iter()
        .any(|item| matches!(item, InlineItem::Float(_)))
}

/// Lays out every float of an inline formatting context once, with its
/// shrink-to-fit width in `cb` (whose content box starts at BFC x `cb_x`).
/// A float moves with the relatively positioned inline boxes around it
/// (its exclusion does not).
fn layout_floats(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    cb: ContainingBlock,
    cb_x: f32,
) -> Vec<Option<Box<(BoxFragment, FloatBox)>>> {
    if !has_floats(ifc) {
        return Vec::new();
    }
    // The relative offsets of the open inline boxes, and whether they are
    // positioned.
    let mut open: Vec<((f32, f32), bool)> = Vec::new();
    ifc.items
        .iter()
        .map(|item| match item {
            InlineItem::StartBox { base, .. } => {
                let positioned = base.style.position != swb_style::Position::Static;
                open.push((relative_offset(&base.style, cb), positioned));
                None
            }
            InlineItem::EndBox { .. } => {
                open.pop();
                None
            }
            InlineItem::Float(inner) => {
                let (mut fragment, mut float) = layout_float(ctx, inner, cb, cb_x);
                for ((dx, dy), _) in &open {
                    float.relative.x += dx;
                    float.relative.y += dy;
                }
                // Inside a positioned inline box, that box is the containing
                // block of the absolutely positioned boxes in the float.
                fragment.in_positioned_inline = open.iter().any(|&(_, positioned)| positioned);
                Some(Box::new((fragment, float)))
            }
            _ => None,
        })
        .collect()
}

/// Lays out every atomic inline once, for the available width.
fn layout_atomics(
    ctx: &mut LayoutContext<'_>,
    ifc: &InlineFormattingContext,
    cb: ContainingBlock,
) -> Atomics {
    ifc.items
        .iter()
        .map(|item| match item {
            InlineItem::Atomic { inner, .. } => {
                let laid_out = layout_independent_shrink_to_fit(ctx, inner, cb);
                Some(Box::new(AtomicLayout::new(
                    laid_out,
                    &inner.base.style,
                    cb.width,
                )))
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
) -> (Atomics, Atomics) {
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
            min.push(Some(Box::new(sized(sizes.min))));
            max.push(Some(Box::new(sized(sizes.max))));
        } else {
            min.push(None);
            max.push(None);
        }
    }
    (min, max)
}

/// The laid-out atomic inlines of a context, indexed by item (boxed: most
/// items are not atomic inlines).
type Atomics = Vec<Option<Box<AtomicLayout>>>;

/// A laid-out atomic inline.
#[derive(Clone)]
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
        let margin_right = clamp_length(margin_or_zero(&style.margin_right, cb_width));
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
            margin_left: clamp_length(margin_or_zero(&style.margin_left, cb_width)),
            margin_right: clamp_length(margin_or_zero(&style.margin_right, cb_width)),
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

/// The width that piece `i` takes on a line; `atomics` hold the sizes of
/// the atomic inlines, and percentages of box edges resolve against
/// `cb_width`.
fn piece_extent(
    ifc: &InlineFormattingContext,
    shaped: &ShapedText,
    atomics: &[Option<Box<AtomicLayout>>],
    cb_width: f32,
    i: usize,
) -> f32 {
    match shaped.pieces.get(i) {
        Some(Piece::Text { width, .. }) => *width,
        Some(Piece::Atomic(item)) => atomics
            .get(*item)
            .and_then(Option::as_ref)
            .map_or(0.0, |a| a.margin_width),
        Some(Piece::StartBox(item)) if !is_continued(ifc, *item) => {
            inline_box_edges(ifc, *item, cb_width).map_or(0.0, |e| e.start())
        }
        Some(Piece::EndBox {
            start,
            split: false,
        }) => inline_box_edges(ifc, *start, cb_width).map_or(0.0, |e| e.end()),
        _ => 0.0,
    }
}

/// An unbreakable sequence of pieces.
#[derive(Debug, Default)]
struct Group {
    pieces: std::ops::Range<usize>,
    /// Where the group starts inside its first piece: the rest of a group
    /// that was cut because it did not fit on a line (see `overflow.rs`).
    head: Option<Cut>,
    width: f32,
    trailing_space: f32,
    forced_break_after: bool,
    /// The last out-of-flow piece (float or absolutely positioned box): a
    /// group without one needs no float handling.
    last_out_of_flow: Option<usize>,
    /// True if text in the group may be cut when it does not fit (see
    /// `overflow.rs`).
    cuttable: bool,
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
            Piece::StartBox(_) | Piece::EndBox { .. } | Piece::OutOfFlow(_) => continue,
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
    atomics: &[Option<Box<AtomicLayout>>],
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
                run,
                width,
                trailing_space,
                ..
            } => {
                group.width += width;
                group.trailing_space = *trailing_space;
                group.cuttable |= shaped.runs.get(*run).is_some_and(|run| {
                    matches!(ifc.items.get(run.item), Some(InlineItem::Text { style, .. })
                        if overflow::breaks_anywhere(style, Purpose::Layout))
                });
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
            Piece::OutOfFlow(_) => group.last_out_of_flow = Some(i),
            Piece::StartBox(_) => {}
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
    /// Where the line ends inside its last group, which was cut because it
    /// does not fit (see `overflow.rs`), and the width before the cut.
    tail: Option<(Cut, f32)>,
}

/// Breaks the groups into lines of `width` (for contexts without line
/// boxes, which do not avoid floats).
fn break_lines(groups: &[Group], width: f32, indent: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = indent;
    let mut has_content = false;
    for (i, group) in groups.iter().enumerate() {
        let fits = used + group.width - group.trailing_space <= width + FIT_TOLERANCE;
        if has_content && !fits {
            lines.push(Line {
                groups: start..i,
                tail: None,
            });
            start = i;
            used = 0.0;
        }
        used += group.width;
        has_content = true;
        if group.forced_break_after {
            lines.push(Line {
                groups: start..i + 1,
                tail: None,
            });
            start = i + 1;
            used = 0.0;
            has_content = false;
        }
    }
    if start < groups.len() {
        lines.push(Line {
            groups: start..groups.len(),
            tail: None,
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
    atomics: Atomics,
    /// The laid-out floats, indexed by item.
    floats: Vec<Option<Box<(BoxFragment, FloatBox)>>>,
    container_style: &'a Arc<ComputedStyle>,
    /// The width of the container's content box.
    width: f32,
    /// The containing block of the inline-level boxes: the container's
    /// content box.
    cb: ContainingBlock,
    /// The BFC x of the container's content box.
    flow_x: f32,
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
    /// See [`InlineLayout::in_flow_floats`].
    in_flow_floats: crate::geom::Size,
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
    /// True once text or an atomic inline is on the line.
    has_content: bool,
    /// The static position rules of the placeholders on the line, in
    /// tree order.
    placeholders: Vec<StaticRule>,
    /// The piece being placed.
    piece: usize,
    /// The piece at which each fragment of `top_level` was added: the
    /// floats of the line go among them in tree order (see
    /// `LineBuilder::insert_line_floats`).
    top_level_pieces: Vec<usize>,
}

/// The result of filling one line (see `LineBuilder::fill_line`).
struct Fill {
    /// The end of the line's groups.
    end: usize,
    /// The free range of the line in BFC x, after the floats on it.
    left: f32,
    right: f32,
    /// True if the content is wider than the free range.
    overflow: bool,
    /// The cut of the line's only group, if it does not fit and can be cut.
    tail: Option<(Cut, f32)>,
    placed: Vec<LineFloat>,
    /// Floats to place below the line.
    queued: Vec<FloatAt>,
    /// The state before the first float placed on the line.
    checkpoint: Option<crate::floats::Checkpoint>,
    /// The state before the first float placed in the current group.
    group_checkpoint: Option<crate::floats::Checkpoint>,
}

/// A float placed on a line that is being tried.
struct LineFloat {
    item: usize,
    /// Its piece (for its place among the line's fragments).
    piece: usize,
    /// The position of its border box in the container's content box.
    position: Point,
}

/// A float of a line and its piece, which decides its place among the
/// line's fragments (see `LineBuilder::insert_line_floats`).
#[derive(Clone, Copy)]
struct FloatAt {
    item: usize,
    piece: usize,
}

/// Where a line starts.
struct LineStart {
    /// The first group of the line.
    group: usize,
    /// The first piece after the floats placed before the line (see
    /// `LineBuilder::place_leading_floats`).
    leading_end: usize,
    /// The text indent of the line.
    indent: f32,
    /// The BFC y of the container's content box.
    content_y: f32,
}

/// Where a float is found on a line that is being filled.
#[derive(Clone, Copy)]
struct FloatSite {
    /// The float's piece.
    piece: usize,
    /// The width of the content before it on the line.
    position: f32,
    /// The width of the hanging white space at the end of that content.
    trailing_space: f32,
    /// True if content (see `LineBuilder::piece_has_content`) is on the
    /// line before its group.
    content_before: bool,
}

/// Why a line does not fit in a layout opportunity.
enum Reject {
    /// Its content is wider than the opportunity; `need` is the width that
    /// the line needs at least (see `LineBuilder::min_line_width`).
    TooWide { need: f32 },
    /// It is taller than the opportunity: the float segment with this
    /// index narrows the free space within its height (see
    /// [`crate::floats::Bfc::narrowing_below`]).
    TooTall(usize),
}

/// Content (and a float beside it) fits on a line if it is at most this
/// much wider than the line: 1/64 px (measured in Chromium 148: `ab cd` in
/// a block 1/64 px narrower than its width stays on one line, 2/64 px
/// narrower it wraps).
const FIT_TOLERANCE: f32 = 1.0 / 64.0;

/// The most layout opportunities that one line is tried in; then it goes
/// below all floats of the BFC. Opportunities narrower than the line's
/// first word are skipped without a try and do not count.
const MAX_LINE_ATTEMPTS: usize = 64;

/// The work budget units that a line attempt that does not fit costs, per
/// piece of the line (see [`crate::floats::WORK_BUDGET`]).
const LINE_RETRY_COST: u64 = 50;

/// A line that fits in a layout opportunity, built but not yet added.
struct TriedLine {
    state: LineState,
    /// The inline boxes that continue on the next line.
    open: Vec<(usize, Arc<ComputedStyle>)>,
    line: Line,
    empty: bool,
    /// The BFC y of the line's top.
    top: f32,
    /// The free range of the line in BFC x.
    left: f32,
    right: f32,
    placed: Vec<LineFloat>,
    queued: Vec<FloatAt>,
}

impl LineState {
    /// The height of the line box.
    fn height(&self) -> f32 {
        let extent = self.extent.or_zero();
        extent.bottom - extent.top
    }
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
            has_content: false,
            placeholders: Vec::new(),
            piece: 0,
            top_level_pieces: Vec::new(),
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
        if let Some(open) = self.stack.last_mut() {
            open.children.push(fragment);
        } else {
            self.top_level.push(fragment);
            self.top_level_pieces.push(self.piece);
        }
    }
}

impl LineBuilder<'_, '_> {
    /// The pieces of a line. A line that ends at a cut ends with the piece
    /// of the cut.
    fn piece_range(groups: &[Group], line: &Line) -> std::ops::Range<usize> {
        if line.groups.is_empty() {
            return 0..0;
        }
        let start = groups[line.groups.start].pieces.start;
        match line.tail {
            Some((cut, _)) if cut.glyph.is_some() => start..cut.piece + 1,
            Some((cut, _)) => start..cut.piece,
            None => start..groups[line.groups.end - 1].pieces.end,
        }
    }

    /// True if the line is not empty (CSS 2.2 §9.4.2): it has text other
    /// than collapsible spaces, an atomic inline, a forced break, or an
    /// inline box edge with a non-zero margin, border or padding.
    fn has_content(&self, groups: &[Group], line: &Line) -> bool {
        Self::piece_range(groups, line).any(|i| self.piece_has_content(i))
    }

    /// True if piece `i` makes a line not empty (see `has_content`). In
    /// quirks mode, margins do not make a line non-empty.
    fn piece_has_content(&self, i: usize) -> bool {
        match self.shaped.pieces.get(i) {
            Some(Piece::Text { run, text, .. }) => {
                let collapses = self
                    .text_style(&self.shaped.runs[*run])
                    .white_space
                    .collapses_spaces();
                let text = self.ifc.text.get(text.clone()).unwrap_or("");
                !text.is_empty() && (!collapses || text.chars().any(|c| c != ' '))
            }
            Some(Piece::Atomic(_) | Piece::LineBreak(_)) => true,
            Some(Piece::StartBox(item)) => {
                !is_continued(self.ifc, *item)
                    && inline_box_style(self.ifc, *item).is_some_and(|s| {
                        has_quirky_start_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_start_edge(s))
                    })
            }
            Some(Piece::EndBox { start, split }) => {
                !split
                    && inline_box_style(self.ifc, *start).is_some_and(|s| {
                        has_quirky_end_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_end_edge(s))
                    })
            }
            Some(Piece::OutOfFlow(_)) | None => false,
        }
    }

    /// The width that piece `i` takes on a line.
    fn piece_width(&self, i: usize) -> f32 {
        piece_extent(self.ifc, self.shaped, &self.atomics, self.width, i)
    }

    /// The cuts of `group` where it may break if it does not fit, with the
    /// width before each, up to the first one after `limit`.
    fn cuts(&self, group: &Group, limit: f32) -> Vec<(Cut, f32)> {
        if !group.cuttable {
            return Vec::new();
        }
        GroupText {
            ifc: self.ifc,
            shaped: self.shaped,
            pieces: group.pieces.clone(),
            head: group.head,
        }
        .cuts(Purpose::Layout, limit, |i| self.piece_width(i))
    }

    /// The float at piece `i`, if it is one.
    fn float_at(&self, i: usize) -> Option<usize> {
        match self.shaped.pieces.get(i) {
            Some(Piece::OutOfFlow(item)) if self.floats.get(*item).is_some_and(Option::is_some) => {
                Some(*item)
            }
            _ => None,
        }
    }

    /// Lays out a context without line boxes (no content, maybe floats): its
    /// lines are empty, and its floats are placed at the top of the
    /// container, or wait for the container's position.
    fn layout_empty(&mut self, groups: &[Group], indent: f32, flow: Flow) {
        // The floats are placed first: the empty lines start after them, if
        // the position is known (then the static positions of the
        // placeholders on the lines follow from there).
        if let FlowY::Resolved(y) = flow.y {
            for slot in self.floats.iter_mut().flatten() {
                let (fragment, float) = &mut **slot;
                let parent = Point::new(self.flow_x, y);
                let position = self.ctx.bfc().place_in(float, y, parent);
                fragment.border_rect.x = position.x;
                fragment.border_rect.y = position.y;
                let end = float_margin_box_end(fragment, self.cb);
                self.in_flow_floats.width = self.in_flow_floats.width.max(end.width);
                self.in_flow_floats.height = self.in_flow_floats.height.max(end.height);
            }
        }
        let left = match flow.y {
            FlowY::Resolved(y) => {
                let cx1 = self.flow_x + self.width;
                self.ctx.bfc().exclusions.range_at(y, self.flow_x, cx1).0 - self.flow_x
            }
            FlowY::Pending { .. } => 0.0,
        };
        // The floats go among the fragments of the lines in tree order.
        let floats: Vec<FloatAt> = self
            .shaped
            .pieces
            .iter()
            .enumerate()
            .filter_map(|(piece, p)| match p {
                Piece::OutOfFlow(item) if self.floats.get(*item).is_some_and(Option::is_some) => {
                    Some(FloatAt { item: *item, piece })
                }
                _ => None,
            })
            .collect();
        let line_start = self.fragments.len();
        let mut pieces = Vec::new();
        for line in break_lines(groups, self.width, indent) {
            let (state, open) = self.build_line(groups, &line, 0.0, &[], self.open.clone());
            self.open = open;
            pieces.extend(self.finish_empty_line(state, left));
        }
        let waiting = matches!(flow.y, FlowY::Pending { .. });
        self.insert_line_floats(line_start, &pieces, floats, waiting);
    }

    /// Lays out the lines of a context with line boxes, in a container
    /// whose content box is at BFC y `content_y`.
    fn layout_lines(
        &mut self,
        groups: &mut [Group],
        indent: f32,
        content_y: f32,
        markers: &mut Vec<PendingMarker<'_>>,
    ) {
        let mut next = 0;
        let mut first = true;
        while next < groups.len() {
            let line_top = content_y + self.y;
            let start = LineStart {
                group: next,
                leading_end: self.place_leading_floats(groups, next, line_top, content_y),
                indent: if first { indent } else { 0.0 },
                content_y,
            };
            let Some(line) = self.fit_line(groups, &start, line_top, markers) else {
                return;
            };
            next = self.commit_line(groups, line, content_y, markers);
            first = false;
        }
    }

    /// Finds the first layout opportunity at or below `line_top` where the
    /// line from `start` fits, and builds the line there (Chromium's
    /// `InlineLayoutAlgorithm::Layout`). The opportunities are produced one
    /// at a time ([`crate::floats::Bfc`]): at each position, the widest
    /// first; if the line is too tall for it, the next narrower one that
    /// reaches lower; if the line is too wide, the next position where the
    /// free space changes. Once the line was too wide, opportunities
    /// narrower than its first word are skipped. After
    /// [`MAX_LINE_ATTEMPTS`] attempts, the line goes below all floats.
    fn fit_line(
        &mut self,
        groups: &[Group],
        start: &LineStart,
        line_top: f32,
        markers: &[PendingMarker<'_>],
    ) -> Option<TriedLine> {
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        if !self.ctx.bfc().exclusions.is_empty() {
            let mut attempts = 0;
            // The width the line needs, once it was too wide somewhere.
            let mut need = 0.0;
            let mut top = line_top;
            'positions: loop {
                let mut candidate = self.ctx.bfc().opportunity_at(top, cx0, cx1);
                while let Some(o) = candidate {
                    if !o.full_width && o.width() + FIT_TOLERANCE < need {
                        // Too wide here too, and in the narrower ones.
                        break;
                    }
                    candidate = match self.try_line(groups, start, &o, markers) {
                        Ok(line) => return Some(line),
                        Err(Reject::TooTall(at)) => self.ctx.bfc().narrower(&o, at, cx0, cx1),
                        Err(Reject::TooWide { need: n }) => {
                            need = n;
                            None
                        }
                    };
                    attempts += 1;
                    if attempts >= MAX_LINE_ATTEMPTS {
                        break 'positions;
                    }
                }
                match self.ctx.bfc().next_top(top) {
                    Some(next) => top = next,
                    None => break,
                }
            }
        }
        // The last opportunity always takes the line.
        let last = self.ctx.bfc().exclusions.below_all(line_top, cx0, cx1);
        self.try_line(groups, start, &last, markers).ok()
    }

    /// Places the floats at the start of the line that starts with group
    /// `start` (before its first content), at the line's top. Returns the
    /// index of the first piece after them.
    fn place_leading_floats(
        &mut self,
        groups: &[Group],
        start: usize,
        line_top: f32,
        content_y: f32,
    ) -> usize {
        let Some(group) = groups.get(start) else {
            return 0;
        };
        if self.floats.is_empty() {
            return group.pieces.start;
        }
        let mut end = group.pieces.start;
        for i in group.pieces.clone() {
            if self.piece_has_content(i) {
                break;
            }
            if let Some(item) = self.float_at(i) {
                self.place_float_now(item, line_top, content_y);
                if let Some(Fragment::Box(b)) = self.fragments.last() {
                    let right = float_margin_box_end(b, self.cb).width;
                    self.in_flow_floats.width = self.in_flow_floats.width.max(right);
                }
            }
            end = i + 1;
        }
        end
    }

    /// Places float `item` at or below `origin` and adds its fragment.
    fn place_float_now(&mut self, item: usize, origin: f32, content_y: f32) {
        self.place_float(item, origin, content_y);
        self.push_float(item, false);
    }

    /// Places float `item` at or below `origin` (its fragment stays in
    /// `floats` until [`LineBuilder::push_float`]).
    fn place_float(&mut self, item: usize, origin: f32, content_y: f32) {
        let Some(Some(slot)) = self.floats.get_mut(item) else {
            return;
        };
        let (fragment, float) = &mut **slot;
        let parent = Point::new(self.flow_x, content_y);
        let position = self.ctx.bfc().place_in(float, origin, parent);
        fragment.border_rect.x = position.x;
        fragment.border_rect.y = position.y;
    }

    /// Adds the fragment of float `item`; with `waiting`, the float waits
    /// for the container's position (as the child with its index).
    fn push_float(&mut self, item: usize, waiting: bool) {
        let Some((fragment, float)) = self.floats.get_mut(item).and_then(Option::take).map(|b| *b)
        else {
            return;
        };
        if waiting {
            self.ctx.bfc().add_pending(PendingFloat {
                path: vec![u32::try_from(self.fragments.len()).unwrap_or(u32::MAX)],
                float,
                origin_x: self.flow_x,
            });
        }
        self.fragments.push(Fragment::Box(fragment));
    }

    /// Puts the floats of a line among its fragments (from `line_start`,
    /// added at the pieces `pieces`) in tree order: before the first
    /// fragment added at a later piece. The paint order of positioned boxes
    /// with equal z-index follows the fragment order. A float inside an
    /// inline box goes before that box's fragment (which is added at the
    /// box's end).
    fn insert_line_floats(
        &mut self,
        line_start: usize,
        pieces: &[usize],
        mut floats: Vec<FloatAt>,
        waiting: bool,
    ) {
        if floats.is_empty() {
            return;
        }
        floats.sort_by_key(|f| f.piece);
        let line = self
            .fragments
            .split_off(line_start.min(self.fragments.len()));
        let mut floats = floats.into_iter().peekable();
        for (k, fragment) in line.into_iter().enumerate() {
            let piece = pieces.get(k).copied().unwrap_or(usize::MAX);
            while let Some(float) = floats.next_if(|f| f.piece < piece) {
                self.push_float(float.item, waiting);
            }
            self.fragments.push(fragment);
        }
        for float in floats {
            self.push_float(float.item, waiting);
        }
    }

    /// Fills and builds the line from `start` in opportunity `o`. If it
    /// does not fit there, returns why; then nothing changed, and the
    /// attempt is charged to the work budget.
    fn try_line(
        &mut self,
        groups: &[Group],
        start: &LineStart,
        o: &Opportunity,
        markers: &[PendingMarker<'_>],
    ) -> Result<TriedLine, Reject> {
        let fill = self.fill_line(groups, start, o);
        let line = Line {
            groups: start.group..fill.end,
            tail: fill.tail,
        };
        let empty = !self.has_content(groups, &line);
        // A line that is too wide moves down, unless no float narrows the
        // containing block here (Chromium's
        // `IsEqualToAvailableFloatInlineSize`) or the container does not
        // wrap lines (`ShouldWrapLine`).
        if fill.overflow && !empty && !o.full_width && self.container_style.white_space.wraps() {
            let need = self.min_line_width(groups, start);
            return Err(self.reject(groups, &line, fill, Reject::TooWide { need }));
        }
        let line_markers: &[PendingMarker<'_>] = if empty { &[] } else { markers };
        let (state, open) =
            self.build_line(groups, &line, start.indent, line_markers, self.open.clone());
        // The line's own floats do not count (they are beside it).
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        let before = fill.checkpoint.as_ref();
        if !empty
            && let Some(at) = self
                .ctx
                .bfc()
                .narrowing_below(o, state.height(), cx0, cx1, before)
        {
            return Err(self.reject(groups, &line, fill, Reject::TooTall(at)));
        }
        Ok(TriedLine {
            state,
            open,
            line,
            empty,
            top: o.top,
            left: fill.left,
            right: fill.right,
            placed: fill.placed,
            queued: fill.queued,
        })
    }

    /// The width that the line from `start` needs at least: its text indent
    /// and its first group, which it always takes; 0 if that group has no
    /// content (the line may then be empty, and an empty line does not
    /// overflow).
    fn min_line_width(&self, groups: &[Group], start: &LineStart) -> f32 {
        groups
            .get(start.group)
            .filter(|g| g.pieces.clone().any(|i| self.piece_has_content(i)))
            .map_or(0.0, |g| {
                // A group that can be cut needs room for its first part.
                let first = self.cuts(g, 0.0).first().map(|&(_, before)| before);
                start.indent + first.unwrap_or(g.width - g.trailing_space)
            })
    }

    /// Undoes the floats of a line attempt that does not fit, and charges
    /// the attempt to the work budget. Returns `reason`.
    fn reject(&mut self, groups: &[Group], line: &Line, fill: Fill, reason: Reject) -> Reject {
        if let Some(checkpoint) = fill.checkpoint {
            self.ctx.bfc().restore(checkpoint);
        }
        let pieces = u64::try_from(Self::piece_range(groups, line).len()).unwrap_or(u64::MAX);
        self.ctx
            .bfc()
            .charge(LINE_RETRY_COST.saturating_mul(pieces.saturating_add(1)));
        reason
    }

    /// Fills a line greedily from `start` in opportunity `o`, and places
    /// the floats on it that fit beside the content before them (Chromium's
    /// `LineBreaker::HandleFloat`); the others are queued for below the
    /// line.
    fn fill_line(&mut self, groups: &[Group], start: &LineStart, o: &Opportunity) -> Fill {
        let mut fill = Fill {
            end: start.group,
            left: o.left,
            right: o.right,
            overflow: false,
            tail: None,
            placed: Vec::new(),
            queued: Vec::new(),
            checkpoint: None,
            group_checkpoint: None,
        };
        let mut used = start.indent;
        // True once a group with content (see `piece_has_content`) is on
        // the line.
        let mut content_before = false;
        let mut trailing = 0.0;
        for (index, group) in groups.iter().enumerate().skip(start.group) {
            let width = used + group.width - group.trailing_space;
            if fill.end > start.group && width > fill.right - fill.left + FIT_TOLERANCE {
                break;
            }
            if !self.floats.is_empty() && group.last_out_of_flow.is_some() {
                let site = FloatSite {
                    piece: 0,
                    position: used,
                    trailing_space: 0.0,
                    content_before,
                };
                if !self.group_floats(group, site, start, o, &mut fill) {
                    break;
                }
            }
            if !self.floats.is_empty() {
                content_before =
                    content_before || group.pieces.clone().any(|i| self.piece_has_content(i));
            }
            used += group.width;
            trailing = group.trailing_space;
            fill.end = index + 1;
            if group.forced_break_after {
                break;
            }
        }
        let available = fill.right - fill.left;
        fill.overflow = used - trailing > available + FIT_TOLERANCE;
        // A first group that does not fit is cut if it can be: at the last
        // cut where its start fits, or else at the first one.
        if fill.overflow
            && fill.end == start.group + 1
            && let Some(group) = groups.get(start.group)
        {
            let room = available - start.indent + FIT_TOLERANCE;
            let cuts = self.cuts(group, room);
            let cut = cuts.iter().rev().find(|(_, before)| *before <= room);
            if let Some(&(cut, before)) = cut.or(cuts.first()) {
                fill.tail = Some((cut, before));
                fill.overflow = before > room;
            }
        }
        fill
    }

    /// Handles the floats in `group`, which starts at `site.position` on
    /// the line. Returns false if a float inside the group (no break
    /// opportunity around it) leaves no room for the rest of the group:
    /// then the group's floats are undone, the line ends before the group,
    /// and the floats are placed again on the next line (Chromium rewinds
    /// the line breaker).
    fn group_floats(
        &mut self,
        group: &Group,
        mut site: FloatSite,
        start: &LineStart,
        o: &Opportunity,
        fill: &mut Fill,
    ) -> bool {
        let used = site.position;
        let (left, right) = (fill.left, fill.right);
        let (placed, queued) = (fill.placed.len(), fill.queued.len());
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        for i in group.pieces.clone() {
            if let Some(item) = self.float_at(i) {
                if i >= start.leading_end {
                    self.handle_float(item, FloatSite { piece: i, ..site }, start, o, fill);
                    let (l, r) = self.ctx.bfc().exclusions.range_at(o.top, cx0, cx1);
                    fill.left = fill.left.max(l);
                    fill.right = fill.right.min(r).max(fill.left);
                }
                continue;
            }
            site.position += self.piece_width(i);
            site.trailing_space = match self.shaped.pieces.get(i) {
                Some(Piece::Text { trailing_space, .. }) => *trailing_space,
                _ => 0.0,
            };
        }
        if let Some(checkpoint) = fill.group_checkpoint.take()
            && used + group.width - group.trailing_space > fill.right - fill.left + FIT_TOLERANCE
        {
            self.ctx.bfc().restore(checkpoint);
            (fill.left, fill.right) = (left, right);
            fill.placed.truncate(placed);
            fill.queued.truncate(queued);
            return false;
        }
        true
    }

    /// Places float `item`, found at `site` on a line at the top of `o`, on
    /// the line if it fits, else queues it for below the line. The state
    /// before the first float placed on the line is saved, for a retry in
    /// another opportunity; if the line already has content, also the
    /// state before the first float of the group.
    fn handle_float(
        &mut self,
        item: usize,
        site: FloatSite,
        start: &LineStart,
        o: &Opportunity,
        fill: &mut Fill,
    ) {
        let line_top = o.top;
        let Some((_, float)) = self.floats.get(item).and_then(Option::as_deref) else {
            return;
        };
        let width = float.width.max(0.0);
        let available = fill.right - fill.left;
        let fits = site.position + width <= available + FIT_TOLERANCE
            || site.position - site.trailing_space + width <= available + FIT_TOLERANCE;
        let exclusions = &self.ctx.bfc().exclusions;
        let below = !fits
            || exclusions.last_float_top() > line_top + EPSILON
            || exclusions
                .clearance(float.clear)
                .is_some_and(|c| c > line_top + EPSILON)
            || !fill.queued.is_empty();
        if below {
            fill.queued.push(FloatAt {
                item,
                piece: site.piece,
            });
            return;
        }
        let float = *float;
        // The last opportunity always takes the line, so nothing to undo
        // there; once the work budget is spent, every opportunity is the
        // last and nothing is copied. The rewind of a word is skipped then
        // too.
        if fill.checkpoint.is_none() && !o.last {
            fill.checkpoint = Some(self.ctx.bfc().checkpoint());
        }
        if site.content_before && fill.group_checkpoint.is_none() && self.ctx.bfc().budget > 0 {
            fill.group_checkpoint = Some(self.ctx.bfc().checkpoint());
        }
        let parent = Point::new(self.flow_x, start.content_y);
        let position = self.ctx.bfc().place_in(&float, line_top, parent);
        fill.placed.push(LineFloat {
            item,
            piece: site.piece,
            position,
        });
    }

    /// Adds a tried line to the layout; places the floats queued for below
    /// it. Returns the first group of the next line.
    fn commit_line(
        &mut self,
        groups: &mut [Group],
        line: TriedLine,
        content_y: f32,
        markers: &mut Vec<PendingMarker<'_>>,
    ) -> usize {
        let TriedLine {
            state,
            open,
            line,
            empty,
            top,
            left,
            right,
            placed,
            queued,
        } = line;
        self.open = open;
        let mut floats = Vec::with_capacity(placed.len() + queued.len());
        for LineFloat {
            item,
            piece,
            position,
        } in placed
        {
            if let Some(Some(slot)) = self.floats.get_mut(item) {
                slot.0.border_rect.x = position.x;
                slot.0.border_rect.y = position.y;
            }
            floats.push(FloatAt { item, piece });
        }
        let line_start = self.fragments.len();
        let pieces = if empty {
            self.finish_empty_line(state, left - self.flow_x)
        } else {
            markers.clear();
            self.y = top - content_y;
            let soft_wrap = self.ends_at_soft_wrap(Self::piece_range(groups, &line));
            self.finish_line(state, left - self.flow_x, right - left, soft_wrap)
        };
        let bottom = content_y + self.y;
        for float in queued {
            self.place_float(float.item, bottom, content_y);
            floats.push(float);
        }
        self.insert_line_floats(line_start, &pieces, floats, false);
        // A `<br>` with `clear` moves the next line below the floats.
        if let Some(clear) = self.line_break_clear(groups, &line)
            && let Some(c) = self.ctx.bfc().exclusions.clearance(clear)
        {
            self.y = self.y.max(c - content_y);
        }
        // The rest of a cut group starts the next line.
        if let Some((cut, before)) = line.tail
            && let Some(index) = line.groups.end.checked_sub(1)
            && let Some(group) = groups.get_mut(index)
        {
            *group = Group {
                pieces: cut.piece..group.pieces.end,
                head: cut.glyph.is_some().then_some(cut),
                width: group.width - before,
                trailing_space: group.trailing_space,
                forced_break_after: group.forced_break_after,
                last_out_of_flow: group.last_out_of_flow.filter(|&p| p >= cut.piece),
                cuttable: group.cuttable,
            };
            return index;
        }
        line.groups.end
    }

    /// Builds a line; `open` are the inline boxes that continue from the
    /// previous line. Returns the line and the boxes that continue on the
    /// next line.
    fn build_line(
        &mut self,
        groups: &[Group],
        line: &Line,
        indent: f32,
        markers: &[PendingMarker<'_>],
        open: Vec<(usize, Arc<ComputedStyle>)>,
    ) -> (LineState, Vec<(usize, Arc<ComputedStyle>)>) {
        self.ctx.layout_units += 1;
        let quirky =
            self.ctx.line_height_quirks && self.container_style.display != Display::ListItem;
        let mut state = LineState::new(self.root_metrics, indent, quirky);
        // Re-open boxes that continue from the previous line.
        for (item, style) in open {
            let (baseline, strut) = state.parent();
            let open = self.open_box(item, &style, state.x, baseline, &strut, true);
            state.push_open(open, false);
        }

        let piece_range = Self::piece_range(groups, line);
        // A line that ends at a cut has no trailing spaces.
        let trim = match line.tail {
            Some(_) => None,
            None => self.trailing_space_to_remove(piece_range.clone()),
        };
        let head = groups.get(line.groups.start).and_then(|g| g.head);
        let tail = line.tail.map(|(cut, _)| cut);
        let end = piece_range.end;
        for i in piece_range {
            state.piece = i;
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
                Piece::Text { glyphs, offset, .. } => {
                    let (first, base) = Cut::glyph_in(head, i).unwrap_or((glyphs.start, *offset));
                    let last = Cut::glyph_in(tail, i).map_or(glyphs.end, |(g, _)| g);
                    if first < last {
                        let trim_end = trim.filter(|(piece, _)| *piece == i).map(|(_, end)| end);
                        self.place_text(&mut state, i, (first..last, base), trim_end);
                    }
                }
                Piece::Atomic(item) => self.place_atomic(&mut state, *item),
                Piece::LineBreak(item) => self.place_line_break(&mut state, *item),
                Piece::OutOfFlow(item) => self.place_out_of_flow(&mut state, *item),
            }
        }

        // Close boxes that continue on the next line.
        state.piece = end;
        let mut continuing = Vec::new();
        while let Some(open) = state.stack.pop() {
            continuing.insert(0, (open.item, Arc::clone(&open.style)));
            let fragment = self.close_box(open, &mut state.x, true);
            state.push_child(Fragment::Box(fragment));
        }
        for pending in markers {
            self.place_marker(*pending, &mut state);
        }
        (state, continuing)
    }

    /// The `clear` of the `<br>` that ends a line, if any.
    fn line_break_clear(&self, groups: &[Group], line: &Line) -> Option<swb_style::Clear> {
        let last = Self::piece_range(groups, line).last()?;
        match self.shaped.pieces.get(last) {
            Some(Piece::LineBreak(item)) => match self.ifc.items.get(*item) {
                Some(InlineItem::LineBreak(Some(base)))
                    if base.style.clear != swb_style::Clear::None =>
                {
                    Some(base.style.clear)
                }
                _ => None,
            },
            _ => None,
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
                    Piece::StartBox(_) | Piece::EndBox { .. } | Piece::OutOfFlow(_)
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

    /// Places glyphs `part.0` of a text piece (all of them, unless the
    /// piece is cut); `part.1` is the advance of the item before them.
    /// `trim_end` is the end of its glyphs without trailing spaces, if they
    /// are removed at the end of the line.
    fn place_text(
        &mut self,
        state: &mut LineState,
        piece: usize,
        part: (std::ops::Range<usize>, f64),
        trim_end: Option<usize>,
    ) {
        let (part, base) = part;
        let Some(Piece::Text { run, .. }) = self.shaped.pieces.get(piece) else {
            return;
        };
        let run = &self.shaped.runs[*run];
        let Some(placed) = placed_text(
            &self.shaped.pieces[piece],
            run,
            part.clone(),
            base,
            trim_end,
        ) else {
            return;
        };
        let (baseline, _) = state.parent();
        let style = self.text_style(run);
        // Text whose glyphs are all removed at the end of the line (only
        // collapsible spaces) does not count for the line height (Chromium
        // skips text items of length 0).
        if trim_end != Some(part.start) {
            let strut = strut_for(&style, run.metrics);
            state
                .extent
                .include(baseline - strut.above, baseline + strut.below);
            state.include_parent_strut(false);
            state.has_content = true;
        }
        let fragment = text_fragment(
            self.ifc,
            run,
            placed.glyphs,
            placed.text,
            &style,
            Point::new(state.x, baseline),
            placed.width,
        );
        state.x += placed.width;
        state.trailing_space = placed.trailing_space;
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
        state.has_content = true;
        let Some(atomic) = self.atomics.get(item).and_then(|a| a.as_deref()).cloned() else {
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
            apply_relative_position(&mut fragment, self.cb);
            state.push_child(Fragment::Box(fragment));
        }
        state.x += atomic.margin_width;
    }

    /// Places the placeholder of an absolutely positioned box at the
    /// current position (floats are not placed). Its vertical position
    /// (and for a box that was block-level, its horizontal one) is set
    /// when the line box is known: see [`move_placeholders`].
    fn place_out_of_flow(&mut self, state: &mut LineState, item: usize) {
        let Some(InlineItem::AbsolutelyPositioned(ib)) = self.ifc.items.get(item) else {
            return;
        };
        let at = Point::new(state.x, 0.0);
        let inline_level = ib.base.style.original_display.is_inline_level();
        // The static-position rectangle has no width; it gets the height
        // of the line box in `move_placeholders`.
        let extent = crate::geom::Size::default();
        let fragment = placeholder(self.ctx, ib, at, StaticParent::Inline, extent);
        state.push_child(fragment);
        state.placeholders.push(if inline_level {
            StaticRule::Inline
        } else {
            StaticRule::Block {
                after_content: state.has_content,
            }
        });
    }

    /// Aligns the line horizontally in its free range (`left` px from the
    /// content box's left edge, `width` px wide), positions it at `self.y`
    /// and appends its fragments. Outside list markers go to the left of
    /// the free range, as in Chromium. `soft_wrap` is true if the line ends
    /// at a soft wrap opportunity.
    fn finish_line(
        &mut self,
        mut state: LineState,
        left: f32,
        width: f32,
        soft_wrap: bool,
    ) -> Vec<usize> {
        state.extent = state.extent.or_zero();
        let line_width = state.x - state.trailing_space;
        let offset = left + self.align_offset(line_width, width);
        self.content_right = self.content_right.max(offset + line_width);
        let line_top = self.y;
        let baseline_y = line_top - state.extent.top;
        for fragment in &mut state.top_level {
            fragment.move_by(offset, baseline_y);
        }
        for fragment in &mut state.outside_markers {
            fragment.move_by(left, baseline_y);
        }
        let line_height = state.extent.bottom - state.extent.top;
        let cb = self.cb;
        // The white space at the end hangs at a soft wrap, and at a forced
        // break or the end of the content only if it does not fit (CSS
        // Text 3 §4.1.3, as Chromium 148).
        let hangs = state.trailing_space > 0.0 && (soft_wrap || state.x > width);
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
        if !state.placeholders.is_empty() {
            let line = (line_top, line_top + line_height);
            move_placeholders(&mut state.top_level, &state.placeholders, line);
        }
        let mut pieces = std::mem::take(&mut state.top_level_pieces);
        pieces.resize(pieces.len() + state.outside_markers.len(), usize::MAX);
        self.fragments.append(&mut state.top_level);
        self.fragments.append(&mut state.outside_markers);
        self.y = line_top + line_height;
        pieces
    }

    /// Appends the fragments of an empty line: zero height at the current
    /// position, `left` px from the content box's left edge (Chromium gives
    /// its inline boxes zero-height fragments).
    fn finish_empty_line(&mut self, mut state: LineState, left: f32) -> Vec<usize> {
        for fragment in &mut state.top_level {
            collapse_to_line_top(fragment);
            fragment.move_by(left, self.y);
        }
        if !state.placeholders.is_empty() {
            let line = (self.y, self.y);
            move_placeholders(&mut state.top_level, &state.placeholders, line);
        }
        self.fragments.append(&mut state.top_level);
        state.top_level_pieces
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
        apply_relative_position(&mut fragment, self.cb);
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
            let start = state.top_level.len();
            state.top_level.extend(shaped.place(0.0, 0.0));
            let added = state.top_level.len() - start;
            state
                .top_level_pieces
                .extend(std::iter::repeat_n(state.piece, added));
        }
    }

    /// The horizontal offset of a line for `text-align`. `justify` is not
    /// supported and aligns to the start.
    fn align_offset(&self, line_width: f32, width: f32) -> f32 {
        let free = width - line_width;
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

/// The right and bottom margin edges of a float in the content box (see
/// [`crate::scroll::InflowExtent`]).
fn float_margin_box_end(b: &BoxFragment, cb: ContainingBlock) -> crate::geom::Size {
    let mut extent = crate::scroll::InflowExtent::default();
    extent.add(b, cb, false);
    extent.finish(0.0)
}

/// How the static position of an absolutely positioned box in inline
/// content follows from its line (as in Chromium's
/// `InlineLayoutAlgorithm::PlaceOutOfFlowObjects`).
#[derive(Clone, Copy, Debug)]
enum StaticRule {
    /// A box that was inline-level: where it is on the line, at the top
    /// of the line box.
    Inline,
    /// A box that was block-level: at the start of the line; below the
    /// line box if content comes before it on the line, else at its top.
    Block { after_content: bool },
}

/// Moves the placeholders in the fragments of a finished line (positioned
/// in the coordinates of the inline formatting context) to their static
/// positions. `rules` has the rule of each placeholder in tree order;
/// `line` is the top and bottom of the line box.
fn move_placeholders(fragments: &mut [Fragment], rules: &[StaticRule], line: (f32, f32)) {
    fn walk(
        fragments: &mut [Fragment],
        origin: Point,
        rules: &mut std::slice::Iter<'_, StaticRule>,
        line: (f32, f32),
    ) {
        for fragment in fragments {
            let Fragment::Box(b) = fragment else {
                continue;
            };
            if let BoxContent::Placeholder(p) = &mut b.content {
                match rules.next() {
                    Some(StaticRule::Inline) => {
                        b.border_rect.y = line.0 - origin.y;
                        p.set_extent(crate::geom::Size::new(0.0, line.1 - line.0));
                    }
                    Some(&StaticRule::Block { after_content }) => {
                        // Below the line if the box follows content on it,
                        // with the line's height (Chromium 148).
                        b.border_rect.x = -origin.x;
                        let y = if after_content { line.1 } else { line.0 };
                        b.border_rect.y = y - origin.y;
                        p.set_extent(crate::geom::Size::new(0.0, line.1 - line.0));
                    }
                    None => {}
                }
            } else if b.is_inline {
                let inner = origin + b.border_rect.origin();
                walk(
                    Arc::make_mut(&mut b.children).as_mut_slice(),
                    inner,
                    rules,
                    line,
                );
            }
        }
    }
    walk(fragments, Point::default(), &mut rules.iter(), line);
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

/// The part of a text piece that goes on a line.
struct PlacedText {
    glyphs: std::ops::Range<usize>,
    text: std::ops::Range<usize>,
    width: f32,
    /// The width of the white space at its end that hangs.
    trailing_space: f32,
}

/// The part of text `piece` (of `run`) on a line: glyphs `part` (all its
/// glyphs unless the piece is cut; its text item has advance `base` before
/// them), up to `trim_end` if trailing spaces are removed.
fn placed_text(
    piece: &Piece,
    run: &Run,
    part: std::ops::Range<usize>,
    base: f64,
    trim_end: Option<usize>,
) -> Option<PlacedText> {
    let Piece::Text {
        glyphs,
        text,
        width,
        trailing_space,
        ..
    } = piece
    else {
        return None;
    };
    if part == *glyphs && trim_end.is_none() {
        return Some(PlacedText {
            glyphs: glyphs.clone(),
            text: text.clone(),
            width: *width,
            trailing_space: *trailing_space,
        });
    }
    let kept = part.start..trim_end.unwrap_or(part.end).max(part.start);
    let advance = shaping::advance_of(run.glyphs.get(kept.clone()).unwrap_or_default());
    // The trailing spaces of the piece hang where the piece ends.
    let hanging = if trim_end.is_none() && part.end == glyphs.end {
        *trailing_space
    } else {
        0.0
    };
    // A cut piece covers the text of its glyphs.
    let cluster = |i: usize, whole: usize| {
        (i != whole)
            .then(|| run.glyphs.get(i).map(|g| g.cluster))
            .flatten()
    };
    let text = cluster(part.start, glyphs.start).unwrap_or(text.start)
        ..cluster(part.end, glyphs.end).unwrap_or(text.end);
    Some(PlacedText {
        glyphs: kept,
        text,
        width: snapped_width(base, advance),
        trailing_space: hanging,
    })
}

/// The text fragment of glyphs `glyphs` of `run` (`width` wide), with its
/// left end on the baseline at `origin`.
fn text_fragment(
    ifc: &InlineFormattingContext,
    run: &Run,
    glyphs: std::ops::Range<usize>,
    text: std::ops::Range<usize>,
    style: &Arc<ComputedStyle>,
    origin: Point,
    width: f32,
) -> TextFragment {
    let (x, baseline) = (origin.x, origin.y);
    let mut pen = 0.0;
    let positioned: Vec<PositionedGlyph> = run
        .glyphs
        .get(glyphs.clone())
        .unwrap_or_default()
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
            width,
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
                // The fragment's width is rounded up to 1/64 px; the carets
                // are not.
                let (start, end) = t.x_range(0, 2).unwrap();
                assert_eq!(start, 0.0);
                assert!(end <= t.rect.width && t.rect.width - end < 1.0 / 64.0);
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

    /// The height of `#p`, laid out 1px wide with 20px lines.
    fn narrow_height(content: &str, style: &str) -> f32 {
        let html = format!(
            "<!DOCTYPE html><p id=p style='margin:0;width:1px;line-height:20px;{style}'>{content}</p>"
        );
        layout_html(&html).rect("p").height
    }

    #[test]
    fn breaks_at_nowrap_and_isolate_boundaries() {
        // The parent decides where a nowrap box ends.
        let nowrap = "<span style='white-space:nowrap'>a-</span>b";
        assert_eq!(narrow_height(nowrap, ""), 40.0);
        assert_eq!(
            narrow_height("a-<span style='white-space:nowrap'>b</span>", ""),
            40.0
        );
        // No break before a wrapping box inside text that does not wrap.
        let inner = "a-<span style='white-space:normal'>b</span>";
        assert_eq!(narrow_height(inner, "white-space:nowrap"), 20.0);
        // No break at the start of an isolated box, except after a space.
        let isolated = "a-<span style='unicode-bidi:isolate'>b</span>";
        assert_eq!(narrow_height(isolated, ""), 20.0);
        let after_space = "a <span style='unicode-bidi:isolate'>b</span>";
        assert_eq!(narrow_height(after_space, ""), 40.0);
        // Atomic inlines in nowrap text do not break.
        let atomics = "<img style='width:5px;height:5px'><img style='width:5px;height:5px'>";
        assert_eq!(narrow_height(atomics, "white-space:nowrap"), 20.0);
    }

    #[test]
    fn long_words_are_cut_one_grapheme_per_line() {
        // A line holds at least one grapheme cluster; each line looks only
        // at its own part of the word.
        let word = "a".repeat(20_000);
        let l = layout_html(&format!(
            "<p style='width:1px;overflow-wrap:anywhere'>{word}</p>"
        ));
        let texts = l.texts();
        assert_eq!(texts.len(), 20_000);
        assert!(texts.iter().all(|(_, t)| t == "a"));
    }

    #[test]
    fn cut_words_keep_their_inline_boxes() {
        let l = layout_html(
            "<p style='width:40px;overflow-wrap:break-word;font:16px sans-serif'>\
             <span id=s>abcdefghijkl</span></p>",
        );
        let texts: Vec<String> = l.texts().into_iter().map(|(_, t)| t).collect();
        assert_eq!(texts, vec!["abcd", "efghij", "kl"]);
        // The span has a fragment on each line.
        let rects: Vec<Rect> = l.texts().into_iter().map(|(r, _)| r).collect();
        let span = l.rect("s");
        assert_eq!(span.y, rects[0].y);
        assert_eq!(span.y + span.height, rects[2].y + rects[2].height);
    }

    #[test]
    fn arabic_joins_across_boxes_as_in_chromium() {
        // Measured in Chromium 148 (the width of the middle word at 16 px
        // between Arabic words): 20.95 px when it is isolated (no joining
        // with the words around it), 14.2 px when it joins, also in a box
        // with `unicode-bidi: embed`. The text of a `bidi-override` box
        // with `direction: ltr` is shaped left to right: 13.58 px with the
        // words around it, 24.19 px when it is isolated too. With
        // `direction: rtl` it is shaped right to left, as the others.
        let word = |style: &str| {
            format!(
                "<div style='font:16px sans-serif'>\u{645}\u{631}\u{62d}\u{628}\u{627}\
                 <span id=s style='{style}'>\u{628}\u{627}\u{644}</span>\
                 \u{639}\u{627}\u{644}\u{645}</div>"
            )
        };
        let width_of = |style: &str| {
            let l = layout_html(&word(style));
            (l.rect("s").width * 100.0).round() / 100.0
        };
        let cases = [
            ("", 14.2),
            ("unicode-bidi:embed", 14.2),
            ("unicode-bidi:embed;direction:rtl", 14.2),
            ("direction:rtl", 14.2),
            ("unicode-bidi:isolate", 20.95),
            ("unicode-bidi:plaintext", 20.95),
            ("unicode-bidi:bidi-override", 13.58),
            ("unicode-bidi:isolate-override", 24.19),
            ("unicode-bidi:bidi-override;direction:rtl", 14.2),
            ("unicode-bidi:isolate-override;direction:rtl", 20.95),
        ];
        for (style, expected) in cases {
            assert_eq!(width_of(style), expected, "{style}");
        }
        let bdi = layout_html(
            "<div style='font:16px sans-serif'>\u{645}\u{631}\u{62d}\u{628}\u{627}\
             <bdi id=i>\u{628}\u{627}\u{644}</bdi>\u{639}\u{627}\u{644}\u{645}</div>",
        );
        assert_eq!((bdi.rect("i").width * 100.0).round() / 100.0, 20.95);
    }

    #[test]
    fn bidi_override_of_a_block_and_of_the_innermost_box() {
        // Measured in Chromium 148 (the width of the box at 16 px between
        // Arabic words): an inline block with the override shapes its own
        // text left to right, and the innermost box with a `unicode-bidi`
        // other than `normal` decides. (Not done: Chromium also applies the
        // override of a block to the inline boxes inside it, 24.19 px for
        // `<b>` in the first case.)
        let width_of = |style: &str, content: &str| {
            let l = layout_html(&format!(
                "<div style='font:16px sans-serif'>\u{645}\u{631}\u{62d}\u{628}\u{627}\
                 <span id=s style='{style}'>{content}</span>\u{639}\u{627}\u{644}\u{645}</div>"
            ));
            (l.rect("s").width * 100.0).round() / 100.0
        };
        let word = "\u{628}\u{627}\u{644}";
        let block = "display:inline-block;";
        let cases = [
            (
                format!("{block}unicode-bidi:bidi-override"),
                word.to_owned(),
                24.19,
            ),
            (
                format!("{block}unicode-bidi:isolate-override"),
                word.to_owned(),
                24.19,
            ),
            (
                format!("{block}unicode-bidi:bidi-override;direction:rtl"),
                word.to_owned(),
                20.95,
            ),
            (
                "unicode-bidi:bidi-override".to_owned(),
                format!("<i style='unicode-bidi:embed'>{word}</i>"),
                14.2,
            ),
        ];
        for (style, content, expected) in cases {
            assert_eq!(width_of(&style, &content), expected, "{style} {content}");
        }
    }

    #[test]
    fn arabic_joins_across_floats_but_not_line_breaks() {
        // Measured in Chromium 148 (two beh letters each): joined across a
        // float (9.28 and 20.55 px), not across a `<br>` (20.17 px), and
        // across the start of a box with padding through the context
        // (15.2 px).
        let l = layout_html(
            "<div style='font:16px sans-serif'><span id=a>\u{628}\u{628}</span>\
             <span style='float:left;width:5px;height:5px'></span>\
             <span id=b>\u{628}\u{628}</span></div>\
             <div style='font:16px sans-serif'><span id=c>\u{628}\u{628}</span><br>\
             <span id=d>\u{628}\u{628}</span></div>\
             <div style='font:16px sans-serif'>\u{645}\u{631}\u{62d}\u{628}\u{627}\
             <span id=e style='padding-left:1px'>\u{628}\u{627}\u{644}</span>\
             \u{639}\u{627}\u{644}\u{645}</div>",
        );
        let width = |id: &str| (l.rect(id).width * 100.0).round() / 100.0;
        assert_eq!((width("a"), width("b")), (9.28, 20.55));
        assert_eq!((width("c"), width("d")), (20.17, 20.17));
        assert_eq!(width("e"), 15.2);
    }
}
