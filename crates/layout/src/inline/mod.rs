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
mod lines;
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
    let shaped = ctx.shaped(ifc, container_style.white_space.wraps());
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
    let shaped = ctx.shaped(ifc, container_style.white_space.wraps());
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
    /// Its entry in [`LineState::relative`] if it has `vertical-align: top`
    /// or `bottom`.
    relative: Option<usize>,
    children: Vec<Fragment>,
}

/// The edge of the line box (or of the enclosing aligned subtree) that a
/// box with `vertical-align: top` or `bottom` aligns with.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineEdge {
    Top,
    Bottom,
}

impl LineEdge {
    fn of(align: &VerticalAlign) -> Option<LineEdge> {
        match align {
            VerticalAlign::Keyword(VerticalAlignKeyword::Top) => Some(LineEdge::Top),
            VerticalAlign::Keyword(VerticalAlignKeyword::Bottom) => Some(LineEdge::Bottom),
            _ => None,
        }
    }
}

/// The tallest aligned subtree with `vertical-align: top` or `bottom` in a
/// line box or in an enclosing such subtree. The first of equal heights
/// counts (measured in Chromium 148: `top` 80 px then `bottom` 80 px puts
/// the rest of the line at the top, the other order at the bottom).
#[derive(Clone, Copy, Debug, Default)]
struct Tallest {
    height: f32,
    edge: Option<LineEdge>,
}

impl Tallest {
    fn add(&mut self, edge: LineEdge, height: f32) {
        if height > self.height {
            *self = Tallest {
                height,
                edge: Some(edge),
            };
        }
    }

    /// `extent` grown to hold the tallest subtree: away from the edge that
    /// the subtree aligns with, so that the subtree fits and the rest stays
    /// at that edge (measured in Chromium 148: a line box with a 100 px
    /// `top` box and a 60 px `bottom` box keeps its text at the top; with
    /// 60 px and 100 px, at the bottom, in either order in the line).
    fn extend(self, mut extent: Extent) -> Extent {
        if self.height > extent.bottom - extent.top {
            match self.edge {
                Some(LineEdge::Top) => extent.bottom = extent.top + self.height,
                Some(LineEdge::Bottom) => extent.top = extent.bottom - self.height,
                None => {}
            }
        }
        extent
    }
}

/// A box with `vertical-align: top` or `bottom` on a line: an inline box
/// with its aligned subtree, or an atomic inline (CSS 2.2 §10.8.1).
#[derive(Clone, Copy, Debug)]
struct LineRelative {
    edge: LineEdge,
    /// The enclosing box with `top` or `bottom` (an index into
    /// [`LineState::relative`]), whose aligned subtree it aligns with; none
    /// for the line box.
    within: Option<usize>,
    /// The extent of its aligned subtree relative to its own baseline,
    /// grown for the line-relative boxes inside it.
    extent: Extent,
    /// The tallest line-relative boxes directly inside it.
    tallest: Tallest,
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
#[derive(Clone, Copy, Debug)]
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
    /// The boxes with `vertical-align: top` or `bottom` on the line, in the
    /// order in which they start. Their aligned subtrees do not count in
    /// `extent`.
    relative: Vec<LineRelative>,
    /// The `extent` of the content around each open line-relative box.
    saved_extents: Vec<Extent>,
    /// The open line-relative boxes (indices into `relative`).
    open_relative: Vec<usize>,
    /// The tallest line-relative boxes directly in the line box.
    tallest: Tallest,
    /// The baseline shift of each line-relative box from its place with
    /// `vertical-align: baseline`, by index; empty on the first pass (see
    /// `LineBuilder::build_line`).
    relative_offsets: Vec<f32>,
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
            relative: Vec::new(),
            saved_extents: Vec::new(),
            open_relative: Vec::new(),
            tallest: Tallest::default(),
            relative_offsets: Vec::new(),
        }
    }

    /// Starts a line-relative box: its aligned subtree collects its own
    /// extent from here. Returns its index and its baseline shift.
    fn start_relative(&mut self, edge: LineEdge) -> (usize, f32) {
        let index = self.relative.len();
        self.relative.push(LineRelative {
            edge,
            within: self.open_relative.last().copied(),
            extent: Extent::EMPTY,
            tallest: Tallest::default(),
        });
        self.saved_extents
            .push(std::mem::replace(&mut self.extent, Extent::EMPTY));
        self.open_relative.push(index);
        (index, self.relative_offset(index))
    }

    /// Ends the line-relative inline box `index` whose baseline is at
    /// `baseline`.
    fn end_relative(&mut self, index: usize, baseline: f32) {
        let outer = self.saved_extents.pop().unwrap_or(Extent::EMPTY);
        let mut own = std::mem::replace(&mut self.extent, outer);
        if own.top > own.bottom {
            // Nothing counted (the line height quirks): zero height at the
            // box's baseline.
            own = Extent {
                top: baseline,
                bottom: baseline,
            };
        }
        self.open_relative.pop();
        let Some(entry) = self.relative.get_mut(index) else {
            return;
        };
        let grown = entry.tallest.extend(own);
        entry.extent = Extent {
            top: grown.top - baseline,
            bottom: grown.bottom - baseline,
        };
        let (edge, within, height) = (entry.edge, entry.within, grown.bottom - grown.top);
        self.add_tallest(within, edge, height);
    }

    /// Adds a line-relative atomic inline whose margin box extends `above`
    /// its baseline and `below` it. Returns its baseline shift.
    fn add_relative_atomic(&mut self, edge: LineEdge, above: f32, below: f32) -> f32 {
        let index = self.relative.len();
        let within = self.open_relative.last().copied();
        self.relative.push(LineRelative {
            edge,
            within,
            extent: Extent {
                top: -above,
                bottom: below,
            },
            tallest: Tallest::default(),
        });
        self.add_tallest(within, edge, above + below);
        self.relative_offset(index)
    }

    fn add_tallest(&mut self, within: Option<usize>, edge: LineEdge, height: f32) {
        match within.and_then(|w| self.relative.get_mut(w)) {
            Some(outer) => outer.tallest.add(edge, height),
            None => self.tallest.add(edge, height),
        }
    }

    fn relative_offset(&self, index: usize) -> f32 {
        self.relative_offsets.get(index).copied().unwrap_or(0.0)
    }

    /// The extent of the line box: the content, grown for the tallest
    /// line-relative box.
    fn line_extent(&self) -> Extent {
        self.tallest.extend(self.extent.or_zero())
    }

    /// The baseline shift of each line-relative box from its place with
    /// `vertical-align: baseline`. Measured in Chromium 148: a box aligns
    /// its aligned subtree with the top or bottom of the line box (or of
    /// the enclosing line-relative box's subtree), moved by the baseline
    /// shifts of its ancestors in between (a `top` box inside a `super`
    /// span sits above the line box's top).
    fn compute_relative_offsets(&self) -> Vec<f32> {
        let line = self.line_extent();
        self.relative
            .iter()
            .map(|r| {
                let reference = r
                    .within
                    .and_then(|w| self.relative.get(w))
                    .map_or(line, |w| w.extent);
                match r.edge {
                    LineEdge::Top => reference.top - r.extent.top,
                    LineEdge::Bottom => reference.bottom - r.extent.bottom,
                }
            })
            .collect()
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
/// `top` and `bottom` give 0: they are aligned with the line box after it
/// is built (see `LineBuilder::build_line`).
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

    /// The tallest line-relative box decides the line height and the side
    /// where the rest of the line stays; the first of two equal ones counts
    /// (the boxes of `va-top-*` in `tools/probes/inline-alignment.json`).
    #[test]
    fn tallest_top_or_bottom_box_decides_the_line_box() {
        let line = |a: (&str, u32), b: (&str, u32)| {
            let l = layout_html(&body(&format!(
                "<div id=d style='font:16px/20px sans-serif'><span id=x>X</span>\
                 <span id=a style='display:inline-block;width:5px;vertical-align:{};height:{}px'></span>\
                 <span id=b style='display:inline-block;width:5px;vertical-align:{};height:{}px'></span></div>",
                a.0, a.1, b.0, b.1
            )));
            let d = l.rect("d");
            (
                l.rect("d").height,
                l.rect("x").y - d.y,
                l.rect("a").y - d.y,
                l.rect("b").y - d.y,
            )
        };
        // The text's box (17 px high) starts 1 px below its line's top.
        assert_eq!(line(("top", 100), ("bottom", 60)), (100.0, 1.0, 0.0, 40.0));
        assert_eq!(line(("top", 60), ("bottom", 100)), (100.0, 81.0, 0.0, 0.0));
        assert_eq!(line(("bottom", 100), ("top", 60)), (100.0, 81.0, 0.0, 0.0));
        assert_eq!(line(("top", 80), ("bottom", 80)), (80.0, 1.0, 0.0, 0.0));
        assert_eq!(line(("bottom", 80), ("top", 80)), (80.0, 61.0, 0.0, 0.0));
        // Short boxes do not change the line box.
        assert_eq!(line(("top", 5), ("bottom", 5)), (20.0, 1.0, 0.0, 15.0));
    }

    /// A line-relative box inside a shifted inline box moves with that
    /// box's shift; one inside a line-relative box aligns with that box's
    /// aligned subtree (`va-top-inside-super`, `va-top-nested-bottom`).
    #[test]
    fn line_relative_boxes_in_shifted_and_aligned_boxes() {
        let l = layout_html(&body(
            "<div id=d style='font:16px/20px sans-serif'>X<span style='vertical-align:-20px'>s\
             <span id=t style='display:inline-block;width:5px;vertical-align:top;height:50px'></span>\
             </span></div>",
        ));
        let d = l.rect("d");
        assert_eq!(d.height, 50.0);
        assert_eq!(l.rect("t").y - d.y, 20.0);
        let l = layout_html(&body(
            "<div id=d style='font:16px/20px sans-serif'>X<span id=t \
             style='vertical-align:top;line-height:40px'>a<span id=c \
             style='display:inline-block;width:5px;vertical-align:bottom;height:70px'></span>\
             </span></div>",
        ));
        let d = l.rect("d");
        assert_eq!(d.height, 70.0);
        assert_eq!(l.rect("c").y - d.y, 0.0);
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
    fn the_parent_white_space_decides_around_atomic_inlines() {
        let img = |own: &str| format!("<img style='width:5px;height:5px;white-space:{own}'>");
        // The atomic inlines' own `white-space` does not count.
        let two = |own: &str| format!("{}{}", img(own), img(own));
        assert_eq!(narrow_height(&two("nowrap"), ""), 40.0);
        assert_eq!(narrow_height(&two("normal"), "white-space:nowrap"), 20.0);
        // The inline box around them is their parent.
        let boxed = |own: &str| format!("<b>{}</b><b>{}</b>", img(own), img(own));
        assert_eq!(narrow_height(&boxed("nowrap"), ""), 40.0);
        let in_nowrap = format!("<span style='white-space:nowrap'>{}</span>", two("normal"));
        assert_eq!(narrow_height(&in_nowrap, ""), 20.0);
        // An isolated box after an atomic inline starts at an opportunity.
        let isolated = |before: &str| {
            format!(
                "{before}<span style='unicode-bidi:isolate'>{}</span>",
                img("normal")
            )
        };
        assert_eq!(narrow_height(&isolated(&img("normal")), ""), 40.0);
        assert_eq!(narrow_height(&isolated("a"), ""), 20.0);
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
