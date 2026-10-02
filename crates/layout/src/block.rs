//! Block layout: block formatting contexts, widths and heights of block
//! boxes, and vertical margin collapsing.
//!
//! CSS 2.2 §8.3.1 (margin collapsing), §10.3 (widths), §10.6 (heights):
//! <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>,
//! <https://www.w3.org/TR/CSS22/visudet.html>.
//!
//! Floats and absolutely positioned boxes are approximated until float
//! layout and positioned layout exist: a float is placed at the current
//! position on its side and does not affect the flow; an absolutely
//! positioned box is placed at its static position with a shrink-to-fit
//! width, and its offsets (`top`, `left`, ...) are ignored. Floats and
//! absolutely positioned boxes that start inside inline content are not
//! laid out at all (`InlineItem::Float` and
//! `InlineItem::AbsolutelyPositioned` produce no fragments), unless the
//! inline content is only collapsible white space (then box construction
//! moves them to block level).

use std::sync::Arc;

use swb_style::{BoxSizing, ComputedStyle, LengthPercentageOrAuto, MaxSize, Size};

use crate::box_tree::{
    BlockContainer, BlockLevelBox, BoxBase, IndependentBox, IndependentContents, Marker,
};
use crate::fragment::{BoxContent, BoxFragment, Fragment};
use crate::geom::{Edges, Rect};
use crate::list_marker::{PendingMarker, markers_at_baseline, shift_markers};
use crate::{LayoutContext, LayoutKey, inline};

/// The containing block of a box: the content box of its parent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContainingBlock {
    /// Width in px.
    pub(crate) width: f32,
    /// Height in px, if definite.
    pub(crate) height: Option<f32>,
}

/// Collapsing margins: the largest positive and the most negative margin
/// (CSS 2.2 §8.3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CollapsedMargin {
    max_positive: f32,
    min_negative: f32,
}

impl CollapsedMargin {
    pub(crate) fn new(margin: f32) -> Self {
        CollapsedMargin {
            max_positive: margin.max(0.0),
            min_negative: margin.min(0.0),
        }
    }

    pub(crate) fn adjoin(self, other: CollapsedMargin) -> Self {
        CollapsedMargin {
            max_positive: self.max_positive.max(other.max_positive),
            min_negative: self.min_negative.min(other.min_negative),
        }
    }

    pub(crate) fn solve(self) -> f32 {
        self.max_positive + self.min_negative
    }
}

/// The margins a laid-out block-level box exposes to its parent for
/// collapsing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BlockMargins {
    /// The top margin, collapsed with any child margins it adjoins.
    pub(crate) start: CollapsedMargin,
    /// The bottom margin, collapsed with any child margins it adjoins.
    pub(crate) end: CollapsedMargin,
    /// True if the top and bottom margins adjoin (an empty box).
    pub(crate) collapsed_through: bool,
}

/// A laid-out block-level box, before its parent positions it.
pub(crate) struct LaidOutBlock {
    /// The fragment; `border_rect.y` is still 0 and `border_rect.x` is the
    /// left margin.
    pub(crate) fragment: BoxFragment,
    pub(crate) margins: BlockMargins,
}

/// Resolved padding and border widths.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BoxEdges {
    pub(crate) padding: Edges,
    pub(crate) border: Edges,
}

impl BoxEdges {
    pub(crate) fn resolve(style: &ComputedStyle, cb_width: f32) -> Self {
        BoxEdges {
            padding: Edges::new(
                style.padding_top.resolve(cb_width),
                style.padding_right.resolve(cb_width),
                style.padding_bottom.resolve(cb_width),
                style.padding_left.resolve(cb_width),
            ),
            border: Edges::new(
                style.border_top_width,
                style.border_right_width,
                style.border_bottom_width,
                style.border_left_width,
            ),
        }
    }

    pub(crate) fn sum(&self) -> Edges {
        self.padding + self.border
    }
}

/// Resolves a `Size` to a content-box length, if it is definite.
pub(crate) fn resolve_size(
    size: &Size,
    basis: Option<f32>,
    box_sizing: BoxSizing,
    edges: f32,
) -> Option<f32> {
    let v = size.as_length_percentage()?.resolve_opt(basis)?;
    Some(content_size(v, box_sizing, edges))
}

/// Resolves a `MaxSize` to a content-box length, if it is definite.
pub(crate) fn resolve_max_size(
    size: &MaxSize,
    basis: Option<f32>,
    box_sizing: BoxSizing,
    edges: f32,
) -> Option<f32> {
    let v = size.as_length_percentage()?.resolve_opt(basis)?;
    Some(content_size(v, box_sizing, edges))
}

fn content_size(v: f32, box_sizing: BoxSizing, edges: f32) -> f32 {
    match box_sizing {
        BoxSizing::ContentBox => v.max(0.0),
        BoxSizing::BorderBox => (v - edges).max(0.0),
    }
}

/// Clamps a content-box width between `min-width` and `max-width`.
pub(crate) fn clamp_width(style: &ComputedStyle, width: f32, cb_width: f32, edges: f32) -> f32 {
    let max = resolve_max_size(&style.max_width, Some(cb_width), style.box_sizing, edges);
    let min =
        resolve_size(&style.min_width, Some(cb_width), style.box_sizing, edges).unwrap_or(0.0);
    let w = max.map_or(width, |m| width.min(m));
    w.max(min)
}

/// Clamps a content-box height between `min-height` and `max-height`.
pub(crate) fn clamp_height(
    style: &ComputedStyle,
    height: f32,
    cb_height: Option<f32>,
    edges: f32,
) -> f32 {
    let max = resolve_max_size(&style.max_height, cb_height, style.box_sizing, edges);
    let min = resolve_size(&style.min_height, cb_height, style.box_sizing, edges).unwrap_or(0.0);
    let h = max.map_or(height, |m| height.min(m));
    h.max(min)
}

/// A margin; `auto` is 0.
fn margin_or_zero(m: &LengthPercentageOrAuto, cb_width: f32) -> f32 {
    m.resolve(cb_width).unwrap_or(0.0)
}

/// Used width and horizontal margins of a block-level box in normal flow
/// (CSS 2.2 §10.3.3 and §10.4). `width` is the content width if already
/// known (for example from shrink-to-fit); otherwise it comes from the
/// style. Returns (content width, margin-left, margin-right).
pub(crate) fn block_width_and_margins(
    style: &ComputedStyle,
    cb: ContainingBlock,
    edges: &BoxEdges,
    width: Option<f32>,
) -> (f32, f32, f32) {
    let edge_sum = edges.sum().horizontal();
    let specified =
        width.or_else(|| resolve_size(&style.width, Some(cb.width), style.box_sizing, edge_sum));
    let margin_left = style.margin_left.resolve(cb.width);
    let margin_right = style.margin_right.resolve(cb.width);

    let solve = |w: Option<f32>| -> (f32, f32, f32) {
        match w {
            None => {
                let ml = margin_left.unwrap_or(0.0);
                let mr = margin_right.unwrap_or(0.0);
                ((cb.width - ml - mr - edge_sum).max(0.0), ml, mr)
            }
            Some(w) => {
                let remaining = cb.width - w - edge_sum;
                // If the box does not fit, `auto` margins are 0. The values
                // are then over-constrained and margin-right is ignored
                // (left-to-right).
                let ml = match (margin_left, margin_right) {
                    (None, None) => (remaining / 2.0).max(0.0),
                    (None, Some(mr)) => (remaining - mr).max(0.0),
                    (Some(ml), _) => ml,
                };
                (w, ml, remaining - ml)
            }
        }
    };

    let (w, ml, mr) = solve(specified);
    let clamped = clamp_width(style, w, cb.width, edge_sum);
    if (clamped - w).abs() > f32::EPSILON {
        solve(Some(clamped))
    } else {
        (w, ml, mr)
    }
}

/// The result of laying out the children of a block container.
pub(crate) struct ChildrenLayout {
    /// The child fragments, relative to the container's content box.
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) content_height: f32,
    /// Margin collapsed through the top of the container (if allowed).
    pub(crate) start_margin: CollapsedMargin,
    /// Margin collapsed through the bottom of the container (if allowed).
    pub(crate) end_margin: CollapsedMargin,
    /// True if every child collapsed through (no content).
    pub(crate) collapsed_through: bool,
    pub(crate) baselines: Baselines,
}

/// The first and last baselines of a box, relative to some origin.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Baselines {
    pub(crate) first: Option<f32>,
    pub(crate) last: Option<f32>,
}

impl Baselines {
    fn offset(self, dy: f32) -> Baselines {
        Baselines {
            first: self.first.map(|b| b + dy),
            last: self.last.map(|b| b + dy),
        }
    }

    /// The baselines a child contributes to its parent. A scroll
    /// container's last baseline is its block-end margin edge (its content
    /// can be scrolled out of view), as in Chromium and CSS Align 3 §9.1.
    fn of(fragment: &BoxFragment, margin_bottom: f32) -> Baselines {
        let scroll_container = fragment.style.overflow_x.is_scroll_container()
            || fragment.style.overflow_y.is_scroll_container();
        Baselines {
            first: fragment.first_baseline,
            last: if scroll_container {
                Some(fragment.border_rect.height + margin_bottom)
            } else {
                fragment.last_baseline
            },
        }
    }
}

/// Options for laying out a block container's children.
#[derive(Clone, Copy)]
pub(crate) struct ChildOptions {
    pub(crate) collapse_with_parent_start: bool,
    pub(crate) collapse_with_parent_end: bool,
}

/// Lays out the children of a block container: returns the child
/// fragments, the content height, and the margins that escaped through the
/// container's top and bottom edges. `markers` are the list markers that
/// wait for a line box (see [`crate::list_marker`]).
pub(crate) fn layout_block_container<'a>(
    ctx: &mut LayoutContext<'_>,
    container: &'a BlockContainer,
    container_style: &Arc<ComputedStyle>,
    cb: ContainingBlock,
    options: ChildOptions,
    markers: &mut Vec<PendingMarker<'a>>,
) -> ChildrenLayout {
    match container {
        BlockContainer::Inline(ifc) => {
            let lines = inline::layout_inline(ctx, ifc, container_style, cb.width, markers);
            ChildrenLayout {
                fragments: lines.fragments,
                content_height: lines.height,
                start_margin: CollapsedMargin::default(),
                end_margin: CollapsedMargin::default(),
                collapsed_through: lines.line_count == 0,
                baselines: Baselines {
                    first: lines.first_baseline,
                    last: lines.last_baseline,
                },
            }
        }
        BlockContainer::Blocks(children) => {
            layout_block_children(ctx, children, cb, options, markers)
        }
    }
}

fn layout_block_children<'a>(
    ctx: &mut LayoutContext<'_>,
    children: &'a [BlockLevelBox],
    cb: ContainingBlock,
    options: ChildOptions,
    markers: &mut Vec<PendingMarker<'a>>,
) -> ChildrenLayout {
    let mut fragments = Vec::with_capacity(children.len());
    let mut y = 0.0;
    let mut pending = CollapsedMargin::default();
    let mut start_margin = CollapsedMargin::default();
    let mut at_start = options.collapse_with_parent_start;
    let mut all_collapsed_through = true;
    let mut baselines = Baselines::default();

    for child in children {
        let laid_out = match child {
            BlockLevelBox::Block {
                base,
                contents,
                marker,
            } => layout_block_box(ctx, base, contents, marker.as_ref(), cb, markers),
            BlockLevelBox::Independent(ib) => layout_independent_block_level(ctx, ib, cb),
            BlockLevelBox::Float(ib) | BlockLevelBox::AbsolutelyPositioned(ib) => {
                let fragment = layout_out_of_flow(ctx, ib, cb, y + pending.solve());
                fragments.push(Fragment::Box(fragment));
                continue;
            }
        };
        let LaidOutBlock {
            mut fragment,
            margins,
        } = laid_out;

        if margins.collapsed_through {
            let combined = margins.start.adjoin(margins.end);
            if at_start {
                start_margin = start_margin.adjoin(combined);
                fragment.border_rect.y = y;
            } else {
                pending = pending.adjoin(combined);
                fragment.border_rect.y = y + pending.solve();
            }
            apply_relative_position(&mut fragment, cb);
            fragments.push(Fragment::Box(fragment));
            continue;
        }

        all_collapsed_through = false;
        if at_start {
            start_margin = start_margin.adjoin(margins.start);
            at_start = false;
        } else {
            pending = pending.adjoin(margins.start);
            y += pending.solve();
        }
        fragment.border_rect.y = y;
        let margin_bottom = fragment
            .style
            .margin_bottom
            .resolve(cb.width)
            .unwrap_or(0.0);
        let child_baselines = Baselines::of(&fragment, margin_bottom).offset(y);
        if baselines.first.is_none() {
            baselines.first = child_baselines.first;
        }
        if child_baselines.last.is_some() {
            baselines.last = child_baselines.last;
        }
        // A marker waiting for a line box aligns with the baseline of a
        // child with its own formatting context.
        if matches!(child, BlockLevelBox::Independent(_))
            && !markers.is_empty()
            && let Some(baseline) = child_baselines.first
        {
            fragments.extend(markers_at_baseline(ctx, markers, baseline));
            markers.clear();
        }
        y += fragment.border_rect.height;
        pending = margins.end;
        apply_relative_position(&mut fragment, cb);
        fragments.push(Fragment::Box(fragment));
    }

    let (start_margin, end_margin, trailing) =
        escaping_margins(start_margin, pending, all_collapsed_through, options);
    ChildrenLayout {
        fragments,
        content_height: y + trailing,
        start_margin,
        end_margin,
        collapsed_through: all_collapsed_through,
        baselines,
    }
}

/// The margins that escape through the top and bottom of a container after
/// its children, and the margin that stays inside at the end. `start` holds
/// the margins collapsed with the container's top, `pending` the margins
/// after the last child with content.
fn escaping_margins(
    start: CollapsedMargin,
    pending: CollapsedMargin,
    all_collapsed_through: bool,
    options: ChildOptions,
) -> (CollapsedMargin, CollapsedMargin, f32) {
    let none = CollapsedMargin::default();
    if all_collapsed_through {
        // Nothing had content: the margins collapsed so far escape through
        // the top if allowed, else through the bottom if allowed, else they
        // stay inside.
        let through = start.adjoin(pending);
        if options.collapse_with_parent_start {
            (through, none, 0.0)
        } else if options.collapse_with_parent_end {
            (none, through, 0.0)
        } else {
            (none, none, through.solve())
        }
    } else if options.collapse_with_parent_end {
        (start, pending, 0.0)
    } else {
        (start, none, pending.solve())
    }
}

/// Lays out a float or an absolutely positioned box among block-level
/// siblings, at the current position `y` (see the module comment).
fn layout_out_of_flow(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
    y: f32,
) -> BoxFragment {
    let mut lb = layout_independent_shrink_to_fit(ctx, ib, cb);
    let style = &ib.base.style;
    if style.is_floating() && style.float == swb_style::Float::Right {
        let margin_right = margin_or_zero(&style.margin_right, cb.width);
        lb.fragment.border_rect.x = cb.width - lb.fragment.border_rect.width - margin_right;
    }
    lb.fragment.border_rect.y = y + lb.margins.start.solve();
    apply_relative_position(&mut lb.fragment, cb);
    lb.fragment
}

/// Places pending markers that found no line box in a line box at the top
/// of a list item's content box, if `own` (the list item's marker) is
/// among them. The line box then is the first line box of the content:
/// updates the content height, `collapsed_through` and the baselines.
fn place_unplaced_markers(
    ctx: &mut LayoutContext<'_>,
    own: Option<&Marker>,
    style: &ComputedStyle,
    markers: &mut Vec<PendingMarker<'_>>,
    children: &mut ChildrenLayout,
) {
    let Some(own) = own else {
        return;
    };
    if !markers.iter().any(|m| std::ptr::eq(m.marker, own)) {
        return;
    }
    let line = inline::marker_line(ctx, style, markers);
    markers.clear();
    children.fragments.extend(line.fragments);
    children.content_height = children.content_height.max(line.height);
    children.collapsed_through = false;
    if children.baselines.first.is_none() {
        children.baselines = Baselines {
            first: Some(line.baseline),
            last: Some(line.baseline),
        };
    }
}

/// Lays out an in-flow block box that does not establish a new block
/// formatting context. `marker` is its own list marker; `markers` are the
/// markers of ancestors that wait for a line box.
pub(crate) fn layout_block_box<'a>(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    contents: &'a BlockContainer,
    marker: Option<&'a Marker>,
    cb: ContainingBlock,
    markers: &mut Vec<PendingMarker<'a>>,
) -> LaidOutBlock {
    let style = &base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let (width, margin_left, _margin_right) = block_width_and_margins(style, cb, &edges, None);
    let vertical_edges = edges.sum().vertical();
    let specified_height = resolve_size(&style.height, cb.height, style.box_sizing, vertical_edges);
    let margin_top = margin_or_zero(&style.margin_top, cb.width);
    let margin_bottom = margin_or_zero(&style.margin_bottom, cb.width);

    let collapse_start = edges.border.top == 0.0 && edges.padding.top == 0.0;
    let no_bottom_edge = edges.border.bottom == 0.0 && edges.padding.bottom == 0.0;
    let min_height_zero = resolve_size(
        &style.min_height,
        cb.height,
        style.box_sizing,
        vertical_edges,
    )
    .unwrap_or(0.0)
        <= 0.0;
    // CSS 2.2 §8.3.1: the bottom margin of the last child adjoins the
    // box's bottom margin only if the box has `auto` height.
    let collapse_end = no_bottom_edge && specified_height.is_none() && min_height_zero;

    let child_cb = ContainingBlock {
        width,
        height: specified_height,
    };
    let content_offset = margin_left + edges.sum().left;
    shift_markers(markers, content_offset);
    if let Some(own) = marker {
        markers.push(PendingMarker {
            marker: own,
            inset: 0.0,
        });
    }
    let mut children = layout_block_container(
        ctx,
        contents,
        style,
        child_cb,
        ChildOptions {
            collapse_with_parent_start: collapse_start,
            collapse_with_parent_end: collapse_end,
        },
        markers,
    );
    place_unplaced_markers(ctx, marker, style, markers, &mut children);
    shift_markers(markers, -content_offset);

    let content_height = specified_height.unwrap_or(children.content_height);
    let height = clamp_height(style, content_height, cb.height, vertical_edges);

    let mut start = CollapsedMargin::new(margin_top);
    if collapse_start {
        start = start.adjoin(children.start_margin);
    }
    let mut end = CollapsedMargin::new(margin_bottom);
    if collapse_end {
        end = end.adjoin(children.end_margin);
    }
    // The top and bottom margins of an empty box adjoin: zero (or `auto`)
    // height, zero min-height, no vertical border or padding, and no line
    // boxes or in-flow content.
    let collapsed_through =
        collapse_start && no_bottom_edge && height == 0.0 && children.collapsed_through;

    let border_box_height = height + vertical_edges;
    let fragment = finish_fragment(
        base,
        Rect::new(
            margin_left,
            0.0,
            width + edges.sum().horizontal(),
            border_box_height,
        ),
        &edges,
        children.fragments,
        children.baselines.offset(edges.sum().top),
    );
    LaidOutBlock {
        fragment,
        margins: BlockMargins {
            start,
            end,
            collapsed_through,
        },
    }
}

/// Builds a box fragment at (0, 0): offsets the children (positioned
/// relative to the content box) to the border box. `baselines` are
/// relative to the border box. The caller positions the fragment and then
/// applies relative positioning.
pub(crate) fn finish_fragment(
    base: &BoxBase,
    border_rect: Rect,
    edges: &BoxEdges,
    mut children: Vec<Fragment>,
    baselines: Baselines,
) -> BoxFragment {
    let offset = edges.sum();
    for child in &mut children {
        child.move_by(offset.left, offset.top);
    }
    BoxFragment {
        node: base.node,
        pseudo: base.pseudo,
        style: Arc::clone(&base.style),
        border_rect,
        border: edges.border,
        padding: edges.padding,
        content: BoxContent::None,
        children: Arc::new(children),
        first_baseline: baselines.first,
        last_baseline: baselines.last,
        is_inline: false,
    }
}

/// Shifts a relatively positioned box (CSS 2.2 §9.4.3,
/// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>). Call
/// after the fragment has its final position. A percentage `top` or
/// `bottom` is `auto` if the containing block height is not definite.
pub(crate) fn apply_relative_position(fragment: &mut BoxFragment, cb: ContainingBlock) {
    let (dx, dy) = relative_offset(&fragment.style, cb);
    fragment.border_rect.x += dx;
    fragment.border_rect.y += dy;
}

/// The offset of a relatively positioned box from its normal position;
/// zero for other boxes.
pub(crate) fn relative_offset(style: &ComputedStyle, cb: ContainingBlock) -> (f32, f32) {
    if style.position != swb_style::Position::Relative {
        return (0.0, 0.0);
    }
    let dx = match (style.left.resolve(cb.width), style.right.resolve(cb.width)) {
        (Some(l), _) => l,
        (None, Some(r)) => -r,
        (None, None) => 0.0,
    };
    let vertical =
        |v: &LengthPercentageOrAuto| v.non_auto().and_then(|lp| lp.resolve_opt(cb.height));
    let dy = match (vertical(&style.top), vertical(&style.bottom)) {
        (Some(t), _) => t,
        (None, Some(b)) => -b,
        (None, None) => 0.0,
    };
    (dx, dy)
}

/// Lays out a box that establishes an independent formatting context,
/// with a known content-box width. The height is `content_height` if given,
/// else the specified height, else the content height; then clamped. The
/// returned fragment is at (0, 0).
pub(crate) fn layout_sized(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    content_width: f32,
    content_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let base = &ib.base;
    let style = &base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let edge_sum = edges.sum();
    let specified_height = content_height.or_else(|| {
        resolve_size(
            &style.height,
            cb.height,
            style.box_sizing,
            edge_sum.vertical(),
        )
    });
    let child_cb = ContainingBlock {
        width: content_width,
        height: specified_height,
    };
    // The list item's own marker waits for a line box in its content.
    let mut markers: Vec<PendingMarker<'_>> = ib
        .marker
        .iter()
        .map(|marker| PendingMarker { marker, inset: 0.0 })
        .collect();
    let mut children = layout_contents(ctx, ib, child_cb, &mut markers);
    place_unplaced_markers(ctx, ib.marker.as_ref(), style, &mut markers, &mut children);
    let height = match content_height {
        Some(h) => h,
        None => clamp_height(
            style,
            specified_height.unwrap_or(children.content_height),
            cb.height,
            edge_sum.vertical(),
        ),
    };
    let mut fragment = finish_fragment(
        base,
        Rect::new(
            0.0,
            0.0,
            content_width + edge_sum.horizontal(),
            height + edge_sum.vertical(),
        ),
        &edges,
        children.fragments,
        children.baselines.offset(edge_sum.top),
    );
    if let IndependentContents::Replaced(r) = &ib.contents {
        fragment.content = BoxContent::Image(r.node);
    }
    fragment
}

/// Lays out the contents of an independent box in its content box `cb`,
/// with the formatting context that the box establishes.
fn layout_contents<'a>(
    ctx: &mut LayoutContext<'_>,
    ib: &'a IndependentBox,
    cb: ContainingBlock,
    markers: &mut Vec<PendingMarker<'a>>,
) -> ChildrenLayout {
    let style = &ib.base.style;
    match &ib.contents {
        IndependentContents::Flow(container) => layout_block_container(
            ctx,
            container,
            style,
            cb,
            ChildOptions {
                collapse_with_parent_start: false,
                collapse_with_parent_end: false,
            },
            markers,
        ),
        IndependentContents::Flex(items) => {
            let layout = crate::flex::layout_flex(ctx, style, items, cb);
            ChildrenLayout {
                fragments: layout.fragments,
                content_height: layout.content_height,
                start_margin: CollapsedMargin::default(),
                end_margin: CollapsedMargin::default(),
                collapsed_through: false,
                // Flex layout computes only the first baseline. It is also
                // the last baseline.
                baselines: Baselines {
                    first: layout.first_baseline,
                    last: layout.first_baseline,
                },
            }
        }
        IndependentContents::Replaced(_) => ChildrenLayout {
            fragments: Vec::new(),
            content_height: 0.0,
            start_margin: CollapsedMargin::default(),
            end_margin: CollapsedMargin::default(),
            collapsed_through: true,
            baselines: Baselines::default(),
        },
    }
}

/// Lays out a block-level box that establishes an independent formatting
/// context, in normal flow: its width fills the containing block unless
/// specified.
fn layout_independent_block_level(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
) -> LaidOutBlock {
    let style = &ib.base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let (width, margin_left, content_height) =
        if let IndependentContents::Replaced(r) = &ib.contents {
            let (w, h) = crate::replaced::used_size(style, r, cb, &edges);
            let (_, ml, _) = block_width_and_margins(style, cb, &edges, Some(w));
            (w, ml, Some(h))
        } else {
            let (w, ml, _) = block_width_and_margins(style, cb, &edges, None);
            (w, ml, None)
        };
    let mut fragment = layout_sized(ctx, ib, width, content_height, cb);
    fragment.border_rect.x = margin_left;
    LaidOutBlock {
        fragment,
        margins: own_margins(style, cb),
    }
}

/// Lays out the root element's box in the initial containing block, at its
/// margin position.
pub(crate) fn layout_root(
    ctx: &mut LayoutContext<'_>,
    root: &IndependentBox,
    icb: ContainingBlock,
) -> BoxFragment {
    let laid_out = layout_independent_block_level(ctx, root, icb);
    let mut fragment = laid_out.fragment;
    fragment.border_rect.y = laid_out.margins.start.solve();
    apply_relative_position(&mut fragment, icb);
    fragment
}

fn own_margins(style: &ComputedStyle, cb: ContainingBlock) -> BlockMargins {
    BlockMargins {
        start: CollapsedMargin::new(margin_or_zero(&style.margin_top, cb.width)),
        end: CollapsedMargin::new(margin_or_zero(&style.margin_bottom, cb.width)),
        collapsed_through: false,
    }
}

/// Lays out an independent box with shrink-to-fit width when its width is
/// `auto` (floats, absolutely positioned boxes, inline-blocks). The
/// fragment's x is its left margin.
pub(crate) fn layout_independent_shrink_to_fit(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
) -> LaidOutBlock {
    let style = &ib.base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let edge_sum = edges.sum().horizontal();
    let margin_left = margin_or_zero(&style.margin_left, cb.width);
    let margin_right = margin_or_zero(&style.margin_right, cb.width);
    let (width, content_height) = if let IndependentContents::Replaced(r) = &ib.contents {
        let (w, h) = crate::replaced::used_size(style, r, cb, &edges);
        (w, Some(h))
    } else {
        let width = resolve_size(&style.width, Some(cb.width), style.box_sizing, edge_sum)
            .unwrap_or_else(|| {
                let sizes = crate::intrinsic::independent_content_sizes(ctx, ib);
                let available = (cb.width - margin_left - margin_right - edge_sum).max(0.0);
                sizes.max.min(available).max(sizes.min)
            });
        (clamp_width(style, width, cb.width, edge_sum), None)
    };
    let mut fragment = layout_sized(ctx, ib, width, content_height, cb);
    fragment.border_rect.x = margin_left;
    LaidOutBlock {
        fragment,
        margins: own_margins(style, cb),
    }
}

/// Lays out a flex item with a fixed content-box width and, optionally, a
/// fixed content-box height. The fragment is at (0, 0).
///
/// Flex layout lays out an item several times (to measure it, then at its
/// final size). Results are cached per item and constraints, so that nested
/// flex containers do not take exponential time.
pub(crate) fn layout_flex_item(
    ctx: &mut LayoutContext<'_>,
    item: &IndependentBox,
    content_width: f32,
    content_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let key = LayoutKey::new(item.base.id, content_width, content_height, cb);
    if let Some(fragment) = ctx.flex_items.get(&key) {
        return fragment.clone();
    }
    ctx.flex_item_layouts += 1;
    let height = match &item.contents {
        IndependentContents::Replaced(r) if content_height.is_none() => {
            let edges = BoxEdges::resolve(&item.base.style, cb.width);
            let (w, h) = crate::replaced::used_size(&item.base.style, r, cb, &edges);
            // Keep the aspect ratio for a stretched width.
            Some(if w > 0.0 { h * content_width / w } else { h })
        }
        _ => content_height,
    };
    let fragment = layout_sized(ctx, item, content_width, height, cb);
    ctx.flex_items.insert(key, fragment.clone());
    fragment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{layout_html, rects_of_text};

    fn body(html: &str) -> String {
        format!("<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>{html}")
    }

    #[test]
    fn auto_margin_is_zero_when_the_box_does_not_fit() {
        let l = layout_html(&body(
            "<div style='width:100px'><div id=c style='width:200px; height:10px; \
             margin-left:auto; margin-right:0'></div></div>",
        ));
        assert_eq!(l.rect("c").x, 0.0);
    }

    #[test]
    fn empty_box_with_zero_height_collapses_through() {
        let l = layout_html(&body(
            "<div style='height:10px'></div><div style='height:0; margin:10px 0'></div>\
             <div id=b style='height:10px'></div>",
        ));
        assert_eq!(l.rect("b").y, 20.0);
    }

    #[test]
    fn margins_of_empty_children_escape_through_the_bottom() {
        let l = layout_html(&body(
            "<div id=p style='border-top:1px solid'><div style='margin:10px 0'></div></div>\
             <div id=after style='height:10px'></div>",
        ));
        assert_eq!(l.rect("p").height, 1.0);
        assert_eq!(l.rect("after").y, 11.0);
    }

    #[test]
    fn float_margins() {
        let l = layout_html(&body(
            "<div style='width:400px'>\
             <div id=r style='float:right; width:100px; height:10px; margin-left:50px'></div>\
             <div id=l style='float:left; width:100px; height:10px; margin-top:20px'></div></div>",
        ));
        assert_eq!(l.rect("r").x, 300.0);
        assert_eq!(l.rect("l").y, 20.0);
    }

    #[test]
    fn markers_of_nested_list_items_share_the_first_line() {
        let l = layout_html(&body(
            "<ul style='margin:0'><li><ul style='margin:0'><li>inner</li></ul></li></ul>",
        ));
        let texts = l.texts();
        let inner = rects_of_text(&texts, "inner")[0];
        let disc = rects_of_text(&texts, "\u{2022} ");
        let circle = rects_of_text(&texts, "\u{25E6} ");
        assert_eq!((disc.len(), circle.len()), (1, 1));
        assert_eq!(disc[0].y, inner.y);
        assert_eq!(circle[0].y, inner.y);
        assert!(disc[0].right() <= 40.0 && circle[0].right() <= 80.0 && circle[0].x > 40.0);
    }

    #[test]
    fn marker_aligns_with_the_baseline_of_a_flex_child() {
        let l = layout_html(&body(
            "<ul><li><div style='display:flex; height:40px; align-items:flex-end'>flex</div>\
             </li></ul>",
        ));
        let texts = l.texts();
        let flex = rects_of_text(&texts, "flex")[0];
        let disc = rects_of_text(&texts, "\u{2022} ");
        assert_eq!(disc.len(), 1);
        assert_eq!(disc[0].bottom(), flex.bottom());
    }

    #[test]
    fn marker_skips_an_empty_first_child() {
        let l = layout_html(&body(
            "<ul><li id=li><div id=e></div><div>text</div></li></ul>",
        ));
        let texts = l.texts();
        let text = rects_of_text(&texts, "text")[0];
        assert_eq!(rects_of_text(&texts, "\u{2022} ")[0].y, text.y);
        assert_eq!(l.rect("e").height, 0.0);
        assert_eq!(l.rect("li").height, 20.0);
    }

    #[test]
    fn marker_of_a_list_item_without_line_boxes_sits_at_its_top() {
        let l = layout_html(&body(
            "<ul><li id=li><div style='height:5px'></div></li></ul>",
        ));
        let li = l.rect("li");
        assert_eq!(li.height, 20.0);
        let disc = rects_of_text(&l.texts(), "\u{2022} ")[0];
        assert!(disc.y >= li.y && disc.bottom() <= li.bottom());
    }

    #[test]
    fn margin_collapsing_math() {
        let a = CollapsedMargin::new(10.0).adjoin(CollapsedMargin::new(20.0));
        assert_eq!(a.solve(), 20.0);
        let b = CollapsedMargin::new(10.0).adjoin(CollapsedMargin::new(-5.0));
        assert_eq!(b.solve(), 5.0);
        let c = CollapsedMargin::new(-10.0).adjoin(CollapsedMargin::new(-5.0));
        assert_eq!(c.solve(), -10.0);
    }
}
