//! Grid layout (CSS Grid Layout 2, <https://www.w3.org/TR/css-grid-2/>),
//! without subgrid and masonry.
//!
//! - `template.rs`: the explicit grid of an axis: track lists with
//!   repetitions, implicit track sizes, named lines and areas.
//! - `placement.rs`: line resolution and auto-placement (§8).
//! - `tracks.rs`: the tracks of an axis as ranges and sets.
//! - `sizing.rs`: the track sizing algorithm (§12.3–§12.8).
//! - This module: the grid sizing algorithm (§12.1), the size
//!   contributions of items (§6.6, §12.5), alignment (§11), the
//!   container's intrinsic sizes and baselines, and the fragments.
//!
//! Chromium's grid layout
//! (`third_party/blink/renderer/core/layout/grid/`) is the reference
//! where the specification leaves room; see
//! ADR 0017. Grid items are laid out through the layout cache of flex
//! items ([`block::layout_flex_item`]), and the placement and the
//! intrinsic sizes of each grid are cached per layout pass, so nested
//! grids do not take exponential time.
//!
//! Limits for hostile content:
//!
//! - Lines are clamped to `-MAX_LINE..=MAX_LINE` around the explicit grid
//!   (the explicit grid has at most [`MAX_LINE`] tracks). Tracks are kept
//!   as sets (`tracks.rs`), so the cost of sizing does not depend on the
//!   number of tracks.
//! - The items of one layout pass may span at most [`SPAN_BUDGET`] extra
//!   tracks together; later items span one track.
//! - Placement and named lines have work budgets (`placement.rs`,
//!   `template.rs`).
//!
//! Not supported: baseline alignment in track sizing (baseline-aligned
//! items are aligned only after sizing), `last baseline`, the second
//! column pass for items whose min-content contribution depends on the
//! row sizes (§12.1 steps 3 and 4), the inheritance of `justify-items:
//! legacy` (see ADR 0017).
//!
//! Absolutely positioned children are laid out by `positioned.rs`: their
//! static position is aligned by `justify-self` and `align-self` in the
//! grid area (ADR 0016).

mod placement;
mod sizing;
mod template;
mod tracks;

use std::collections::HashMap;
use std::rc::Rc;

use swb_style::{
    Alignment, BoxSizing, ComputedStyle, Gap, LengthPercentage, MaxSize, RepeatCount, Size,
    TrackBreadth, TrackList, TrackListValue, TrackSize,
};

use crate::LayoutContext;
use crate::align::{Edge, resolve_self_alignment};
use crate::block::{
    Baselines, BoxEdges, ContainingBlock, apply_relative_position, clamp_height, clamp_width,
    finish_fragment, layout_flex_item, margin_or_zero, outer_size, resolve_max_size, resolve_size,
};
use crate::box_tree::{BoxBase, IndependentBox, IndependentContents};
use crate::fragment::{BoxFragment, Fragment};
use crate::geom::Rect;
use crate::intrinsic::{self, ContentSizes};
use crate::positioned::{StaticParent, add_placeholders};
use placement::{Area, AxisLines, Placement};
use sizing::{Constraint, Contribution, SizingInput, SizingItem};
use template::{LineNameIndex, Template};
use tracks::{MinSizing, Tracks};

/// The limited grid: lines are clamped to `-MAX_LINE..=MAX_LINE` (line 1
/// of the explicit grid is 0), and the explicit grid has at most
/// `MAX_LINE` tracks per axis. CSS Grid 2 §5.4 asks UAs to allow at least
/// `[-10000, 10000]`; Chromium allows 10,000,000.
const MAX_LINE: i32 = 10_000;

/// The most extra tracks (beyond one per axis) that the grid items of
/// one layout pass may span together.
const SPAN_BUDGET: usize = 4_000_000;

/// Grid data of one layout pass.
#[derive(Default)]
pub(crate) struct GridCache {
    /// The placement of each grid, by box and automatic repetitions.
    placements: HashMap<(usize, u32, u32), Rc<Placement>>,
    /// The content sizes of each grid container.
    content_sizes: HashMap<usize, ContentSizes>,
    /// The extra tracks that items spanned so far (see [`SPAN_BUDGET`]).
    spans_used: usize,
    /// True once a limit was logged.
    warned: bool,
}

/// The in-flow items of a grid container, in order-modified document
/// order (§6.3). Absolutely positioned children are not items.
fn in_flow_items(children: &[IndependentBox]) -> Vec<&IndependentBox> {
    let mut items: Vec<&IndependentBox> = children
        .iter()
        .filter(|c| !c.base.style.is_absolutely_positioned())
        .collect();
    items.sort_by_key(|c| c.base.style.order);
    items
}

/// The used size of a gap ([`crate::flex::gap`]); percentages of an
/// indefinite size resolve against 0, as in Chromium
/// (`CalculateGutterSize`).
fn gap(g: &Gap, basis: Option<f32>) -> f32 {
    crate::flex::gap(g, basis.unwrap_or(0.0)).max(0.0)
}

/// The size of a track for the number of automatic repetitions: its fixed
/// sizes (the larger one if both are fixed, percentages of `available`),
/// at least 1 px for `repeated` tracks.
fn repeat_track_size(size: &TrackSize, available: f32, repeated: bool) -> f32 {
    let fixed = |b: &TrackBreadth| match b {
        TrackBreadth::Length(lp) => Some(lp.resolve(available).max(0.0)),
        _ => None,
    };
    let v = match size {
        TrackSize::Breadth(b) => fixed(b).unwrap_or(0.0),
        TrackSize::MinMax(lo, hi) => match (fixed(lo), fixed(hi)) {
            (Some(a), Some(b)) => a.max(b),
            (a, b) => a.or(b).unwrap_or(0.0),
        },
        TrackSize::FitContent(_) => 0.0,
    };
    if repeated { v.max(1.0) } else { v }
}

/// The number of repetitions of `repeat(auto-fill | auto-fit, ...)` in
/// `list` (§7.2.3.2, Chromium's `CalculateAutomaticRepetitions`): as many
/// as fit into the available size (or the maximum size), at least one.
/// With an indefinite size, as many as are needed to reach the minimum
/// size. 0 if the list has no automatic repetition.
///
/// Parts of this function are derived from Chromium's `LayoutNG` grid code
/// (`third_party/blink/renderer/core/layout/grid/grid_layout_utils.cc`,
/// `grid_track_sizing_algorithm.cc`; Copyright The Chromium Authors,
/// BSD-3-Clause); see `THIRD_PARTY_NOTICES.md`.
fn auto_repetitions(
    list: &TrackList,
    gap: f32,
    available: Option<f32>,
    min_available: f32,
    max_available: Option<f32>,
) -> u32 {
    let Some((repeat_len, others)) = template::auto_repeat_shape(list) else {
        return 0;
    };
    let (available, max_available) = match available {
        Some(a) => (a, Some(a)),
        None => (min_available, max_available),
    };
    // The explicit grid has at most `MAX_LINE` tracks.
    let limit = ((i64::from(MAX_LINE) - others).max(1) as f32 / repeat_len as f32).max(1.0);
    if limit <= 1.0 {
        return 1;
    }
    // Here the list has at most `MAX_LINE` entries (see
    // `auto_repeat_shape`).
    let size = |t: &TrackSize, repeated: bool| repeat_track_size(t, available, repeated) + gap;
    let mut repeater = 0.0;
    let mut fixed = 0.0;
    for entry in list.entries() {
        match &entry.value {
            TrackListValue::Track(t) => fixed += size(t, false),
            TrackListValue::Repeat(r) => match r.count {
                RepeatCount::Count(n) => {
                    fixed += r.tracks.iter().map(|t| size(t, false)).sum::<f32>() * n as f32;
                }
                RepeatCount::AutoFill | RepeatCount::AutoFit => {
                    repeater = r.tracks.iter().map(|t| size(t, true)).sum();
                }
            },
        }
    }
    fixed -= gap;
    let count = match max_available {
        Some(max) => ((max - fixed) / repeater).floor(),
        None => ((available - fixed) / repeater).ceil(),
    };
    if count.is_nan() {
        return 1;
    }
    count.clamp(1.0, limit) as u32
}

/// The placement of the in-flow `items` of grid container `base` (cached
/// per layout pass).
fn placement(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    items: &[&IndependentBox],
    column_repeat: u32,
    row_repeat: u32,
) -> Rc<Placement> {
    let key = (base.id, column_repeat, row_repeat);
    if let Some(p) = ctx.grids.placements.get(&key) {
        return Rc::clone(p);
    }
    let style = &base.style;
    let areas = style.grid_template_areas.as_deref();
    let (columns, rows) = templates(style, column_repeat, row_repeat);
    let mut row_names =
        LineNameIndex::new(&style.grid_template_rows, &rows, areas.map(|a| (a, true)));
    let mut column_names = LineNameIndex::new(
        &style.grid_template_columns,
        &columns,
        areas.map(|a| (a, false)),
    );
    let styles: Vec<&ComputedStyle> = items.iter().map(|b| b.base.style.as_ref()).collect();
    let mut budget = SPAN_BUDGET.saturating_sub(ctx.grids.spans_used);
    let before = budget;
    let (placed, warnings) = placement::place(
        &styles,
        style.grid_auto_flow,
        &mut AxisLines {
            names: &mut row_names,
            explicit: rows.explicit_tracks,
        },
        &mut AxisLines {
            names: &mut column_names,
            explicit: columns.explicit_tracks,
        },
        &mut budget,
    );
    ctx.grids.spans_used += before - budget;
    let limited = warnings.spans || warnings.work || row_names.exhausted || column_names.exhausted;
    if limited && !ctx.grids.warned {
        ctx.grids.warned = true;
        log::warn!("grid limits reached: some grid items are not placed as specified");
    }
    let placed = Rc::new(placed);
    ctx.grids.placements.insert(key, Rc::clone(&placed));
    placed
}

/// The min-content and max-content widths of the content box of grid
/// container `base` (cached per layout pass).
pub(crate) fn content_sizes(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    children: &[IndependentBox],
) -> ContentSizes {
    if let Some(sizes) = ctx.grids.content_sizes.get(&base.id) {
        return *sizes;
    }
    let sizes = crate::table::TableCache::percent_free(ctx, |ctx| {
        compute_content_sizes(ctx, base, children)
    });
    ctx.grids.content_sizes.insert(base.id, sizes);
    sizes
}

/// A grid item during layout.
struct Item<'a> {
    box_: &'a IndependentBox,
    style: &'a ComputedStyle,
    area: Area,
    /// The first and the end range of the area in each axis.
    columns: (usize, usize),
    rows: (usize, usize),
    /// The margin-box min-content and max-content widths (computed when
    /// needed).
    outer: Option<ContentSizes>,
    /// The block-size contribution and the area width it is for.
    block: Option<(f32, f32)>,
    /// The height of the area if all its rows have a fixed maximum (the
    /// sum of the maximums and the gaps): the block size that the item
    /// assumes while the columns are sized (§12.1 step 1).
    row_fixed_max: Option<f32>,
}

impl Item<'_> {
    fn is_replaced(&self) -> bool {
        matches!(self.box_.contents, IndependentContents::Replaced(_))
    }

    /// `justify-self`, with `auto` resolved.
    fn justify(&self, container: &ComputedStyle) -> Alignment {
        resolve_self_alignment(self.style.justify_self, container.justify_items)
    }

    /// `align-self`, with `auto` resolved.
    fn align(&self, container: &ComputedStyle) -> Alignment {
        resolve_self_alignment(self.style.align_self, container.align_items)
    }
}

/// The rule for an item's minimum contribution in one axis (§6.6).
#[derive(Clone, Copy, Debug)]
struct MinimumRule {
    /// True if the automatic minimum size is the content-based minimum:
    /// the item spans a track with an `auto` minimum, and no flexible
    /// track if it spans more than one.
    content_based: bool,
    /// The sum of the fixed maximums of the spanned tracks, if all of
    /// them have one: the content-based minimum is clamped to it.
    clamp: Option<f32>,
}

/// The sizing data of the items in one axis.
fn sizing_items(
    tracks: &Tracks,
    spans: &[((usize, usize), u32)],
) -> (Vec<SizingItem>, Vec<MinimumRule>) {
    spans
        .iter()
        .map(|&(ranges, span)| {
            let sets = tracks.sets_of(ranges);
            let item = SizingItem::new(tracks, sets.clone(), span);
            let auto_min = tracks
                .sets
                .get(sets.clone())
                .is_some_and(|s| s.iter().any(|s| s.sizing.min == MinSizing::Auto));
            let rule = MinimumRule {
                content_based: auto_min && !(item.spans_flex && span > 1),
                clamp: tracks.fixed_max_span(sets, span),
            };
            (item, rule)
        })
        .unzip()
}

/// The sizing properties of an item in one axis.
struct AxisProperties<'s> {
    /// The preferred size (`width` or `height`).
    size: &'s Size,
    /// The minimum size.
    min_size: &'s Size,
    box_sizing: BoxSizing,
    /// True if the item is a scroll container in this axis.
    scroll_container: bool,
    /// The padding and border.
    edges: f32,
    /// The margins.
    margins: f32,
}

/// The minimum contribution of an item in one axis (§6.6, Chromium's
/// `CalculateIntrinsicMinimumContribution`); `content` has its
/// min-content and max-content contributions (margin box).
fn minimum_contribution(p: &AxisProperties<'_>, content: ContentSizes, rule: MinimumRule) -> f32 {
    let AxisProperties {
        size,
        min_size,
        box_sizing,
        scroll_container,
        edges,
        margins,
    } = *p;
    let auto_like = match size {
        Size::Auto | Size::FitContent(_) => true,
        Size::LengthPercentage(lp) => lp.has_percentage(),
        Size::MinContent => return content.min,
        Size::MaxContent => return content.max,
    };
    if !auto_like {
        return content.min;
    }
    if !min_size.is_auto() || scroll_container || !rule.content_based {
        // The used minimum size as the preferred size.
        let border_box = match min_size {
            Size::LengthPercentage(lp) => {
                let v = lp.resolve_opt(None).unwrap_or(0.0).max(0.0);
                outer_size(box_sizing, v, edges)
            }
            Size::MinContent => return content.min,
            Size::MaxContent => return content.max,
            Size::Auto | Size::FitContent(_) => edges,
        };
        return border_box + margins;
    }
    match rule.clamp {
        Some(limit) => content.min.min(limit.max(margins + edges)),
        None => content.min,
    }
}

/// The inline-axis contribution of an item (margin box).
fn inline_contribution(
    ctx: &mut LayoutContext<'_>,
    item: &mut Item<'_>,
    kind: Contribution,
    rule: MinimumRule,
    container: &ComputedStyle,
) -> f32 {
    if item.outer.is_none() {
        let mut s = match stretched_replaced_width(item, container) {
            Some(w) => ContentSizes { min: w, max: w },
            None => intrinsic::independent_outer_sizes(ctx, item.box_),
        };
        // `min-content` and `max-content` widths contribute that size
        // in both cases.
        match item.style.width {
            Size::MinContent => s.max = s.min,
            Size::MaxContent => s.min = s.max,
            _ => {}
        }
        item.outer = Some(s);
    }
    let sizes = item.outer.unwrap_or_default();
    match kind {
        Contribution::MinContent => sizes.min,
        Contribution::MaxContent => sizes.max,
        Contribution::Minimum => {
            let style = item.style;
            let edges = BoxEdges::resolve(style, 0.0).sum().horizontal();
            let properties = AxisProperties {
                size: &style.width,
                min_size: &style.min_width,
                box_sizing: style.box_sizing,
                scroll_container: style.overflow_x.is_scroll_container(),
                edges,
                margins: intrinsic::fixed_margins(style),
            };
            minimum_contribution(&properties, sizes, rule)
        }
    }
}

/// The margin-box width of an image that stretches in the block axis of
/// rows that all have a fixed maximum: while the columns are sized, those
/// rows count as their maximum (§12.1 step 1), so the image's width is
/// that height through its aspect ratio. `None` for other items.
fn stretched_replaced_width(item: &Item<'_>, container: &ComputedStyle) -> Option<f32> {
    if !item.is_replaced() {
        return None;
    }
    let area_height = item.row_fixed_max?;
    let height = stretched_height(item, (0.0, area_height), container)?;
    let edges = BoxEdges::resolve(item.style, 0.0);
    let width = crate::replaced::width_from_height(item.box_, height, None, &edges)?;
    Some(width + edges.sum().horizontal() + intrinsic::fixed_margins(item.style))
}

/// The content-box width of an item in an area of `area_width` (and
/// `area_height`, if known): its `width`, or the area minus its margins
/// when it stretches (only an `auto` width), or else fit-content (or the
/// min-content or max-content width for those keywords); then clamped
/// (§11.3, Chromium's auto size behaviors). An image that stretches only
/// in the block axis takes its width from the stretched height through its
/// aspect ratio; other images keep their used width.
fn inline_size(
    ctx: &mut LayoutContext<'_>,
    item: &Item<'_>,
    (area_width, area_height): (f32, Option<f32>),
    container: &ComputedStyle,
) -> f32 {
    let style = item.style;
    let edges = BoxEdges::resolve(style, area_width);
    let h_edges = edges.sum().horizontal();
    let margin_left = style.margin_left.resolve(area_width);
    let margin_right = style.margin_right.resolve(area_width);
    let available =
        (area_width - margin_left.unwrap_or(0.0) - margin_right.unwrap_or(0.0) - h_edges).max(0.0);
    let stretch = stretches_inline(item, container, area_width);
    if let IndependentContents::Replaced(r) = &item.box_.contents {
        if stretch {
            return clamp_width(style, available, area_width, h_edges);
        }
        let cb = ContainingBlock {
            width: area_width,
            height: area_height,
        };
        // An image that stretches in the block axis takes its width from
        // that height through its aspect ratio.
        if let Some(height) =
            area_height.and_then(|h| stretched_height(item, (area_width, h), container))
            && let Some(width) = crate::replaced::column_flex_width(item.box_, height, cb, &edges)
        {
            return width;
        }
        return crate::replaced::used_size(style, r, cb, &edges).0;
    }
    if let Some(w) = resolve_size(&style.width, Some(area_width), style.box_sizing, h_edges) {
        return clamp_width(style, w, area_width, h_edges);
    }
    // A container with an aspect ratio that stretches only in the block
    // axis takes its width from the stretched height (Chromium).
    if !stretch
        && let Some(height) =
            area_height.and_then(|h| stretched_height(item, (area_width, h), container))
        && let Some(width) = crate::replaced::column_flex_width(
            item.box_,
            height,
            ContainingBlock {
                width: area_width,
                height: area_height,
            },
            &edges,
        )
    {
        return width;
    }
    let width = if stretch {
        available
    } else {
        let sizes = intrinsic::independent_content_sizes(ctx, item.box_);
        match style.width {
            Size::MinContent => sizes.min,
            Size::MaxContent => sizes.max,
            _ => sizes.max.min(available.max(sizes.min)),
        }
    };
    clamp_width(style, width, area_width, h_edges)
}

/// True if an item stretches in the inline axis: an `auto` width, no
/// `auto` horizontal margins and a stretching `justify-self`.
fn stretches_inline(item: &Item<'_>, container: &ComputedStyle, area_width: f32) -> bool {
    let style = item.style;
    style.width.is_auto()
        && stretches(item.justify(container), item.is_replaced())
        && style.margin_left.resolve(area_width).is_some()
        && style.margin_right.resolve(area_width).is_some()
}

/// True if an item with `alignment` (resolved `justify-self` or
/// `align-self`) stretches: `normal` stretches non-replaced items,
/// `stretch` all items (Chromium's `kStretchImplicit` and
/// `kStretchExplicit`).
fn stretches(alignment: Alignment, replaced: bool) -> bool {
    match alignment {
        Alignment::Stretch => true,
        Alignment::Normal => !replaced,
        _ => false,
    }
}

/// The content-box height of an item that stretches in the block axis of
/// an area of `(area_width, area_height)`: the area minus the margins,
/// clamped; `None` if it does not stretch (a non-`auto` height or `auto`
/// margins, or an alignment other than stretching).
fn stretched_height(
    item: &Item<'_>,
    (area_width, area_height): (f32, f32),
    container: &ComputedStyle,
) -> Option<f32> {
    let style = item.style;
    let margin_top = style.margin_top.resolve(area_width)?;
    let margin_bottom = style.margin_bottom.resolve(area_width)?;
    if !style.height.is_auto() || !stretches(item.align(container), item.is_replaced()) {
        return None;
    }
    let v_edges = BoxEdges::resolve(style, area_width).sum().vertical();
    let h = (area_height - margin_top - margin_bottom - v_edges).max(0.0);
    Some(clamp_height(style, h, Some(area_height), v_edges))
}

/// The block-axis contribution of an item (margin box): its height when
/// laid out in an area `area_width` wide with an indefinite height.
fn block_contribution(
    ctx: &mut LayoutContext<'_>,
    item: &mut Item<'_>,
    area_width: f32,
    container: &ComputedStyle,
) -> f32 {
    if let Some((w, h)) = item.block
        && w == area_width
    {
        return h;
    }
    let width = inline_size(ctx, item, (area_width, None), container);
    let cb = ContainingBlock {
        width: area_width,
        height: None,
    };
    let fragment = layout_flex_item(ctx, item.box_, width, None, cb);
    let style = item.style;
    let margins = vertical_margins(style, area_width);
    let h = fragment.border_rect.height + margins;
    item.block = Some((area_width, h));
    h
}

/// The sum of the top and bottom margins of `style` (`auto` is 0).
fn vertical_margins(style: &ComputedStyle, width: f32) -> f32 {
    margin_or_zero(&style.margin_top, width) + margin_or_zero(&style.margin_bottom, width)
}

/// The minimum block-axis contribution of an item in an area
/// `area_width` wide.
fn block_minimum(item: &Item<'_>, area_width: f32, contribution: f32, rule: MinimumRule) -> f32 {
    let style = item.style;
    let edges = BoxEdges::resolve(style, area_width).sum().vertical();
    let margins = vertical_margins(style, area_width);
    let content = ContentSizes {
        min: contribution,
        max: contribution,
    };
    let properties = AxisProperties {
        size: &style.height,
        min_size: &style.min_height,
        box_sizing: style.box_sizing,
        scroll_container: style.overflow_y.is_scroll_container(),
        edges,
        margins,
    };
    minimum_contribution(&properties, content, rule)
}

/// The alignment of an item in one axis (Chromium's
/// `AxisEdgeFromItemPosition`): `auto` margins first (they never
/// overflow the start), then the alignment value. Returns the edge and
/// whether the alignment is safe.
fn item_edge(alignment: Alignment, margin_start_auto: bool, margin_end_auto: bool) -> (Edge, bool) {
    match (margin_start_auto, margin_end_auto) {
        (true, true) => return (Edge::Center, true),
        (true, false) => return (Edge::End, true),
        (false, true) => return (Edge::Start, true),
        (false, false) => {}
    }
    let edge = match alignment {
        Alignment::Center => Edge::Center,
        Alignment::End | Alignment::FlexEnd | Alignment::SelfEnd | Alignment::Right => Edge::End,
        Alignment::Baseline => Edge::Baseline,
        _ => Edge::Start,
    };
    (edge, false)
}

/// The offset of an item's border box from the start of its area
/// (Chromium's `AlignmentOffset`).
fn align_offset(
    area: f32,
    size: f32,
    margin_start: f32,
    margin_end: f32,
    (edge, safe): (Edge, bool),
) -> f32 {
    let free = area - size - margin_start - margin_end;
    if safe && free < 0.0 {
        return margin_start;
    }
    margin_start + edge.along(0.0, free)
}

/// The offset of the first track and the space added to every gap for
/// `align-content` or `justify-content` (§11.5, Chromium's
/// `ComputeFirstSetGeometry`), with `free` space (may be negative) and
/// `tracks` tracks.
fn distribute_content(alignment: Alignment, free: f32, tracks: u32) -> (f32, f32) {
    let n = tracks as f32;
    match alignment {
        Alignment::SpaceBetween if tracks >= 2 && free >= 0.0 => (0.0, free / (n - 1.0)),
        Alignment::SpaceAround if free >= 0.0 && tracks == 0 => (free / 2.0, 0.0),
        Alignment::SpaceAround if free >= 0.0 => (free / n / 2.0, free / n),
        Alignment::SpaceEvenly if free >= 0.0 => (free / (n + 1.0), free / (n + 1.0)),
        Alignment::Center => (free / 2.0, 0.0),
        Alignment::End | Alignment::FlexEnd | Alignment::Right => (free, 0.0),
        _ => (0.0, 0.0),
    }
}

/// True if `auto` tracks stretch with this content distribution (§12.8).
fn stretches_tracks(alignment: Alignment) -> bool {
    matches!(alignment, Alignment::Normal | Alignment::Stretch)
}

/// The start and end of each range of `tracks` in a container of `size`
/// (if definite) with content alignment `alignment`.
fn track_positions(tracks: &Tracks, alignment: Alignment, size: Option<f32>) -> Vec<(f32, f32)> {
    let (offset, extra) = match size {
        Some(size) => {
            distribute_content(alignment, size - tracks.total_size(), tracks.track_count())
        }
        None => (0.0, 0.0),
    };
    tracks.positions(offset, tracks.gap + extra)
}

/// The span of an item's area in tracks, and its ranges.
fn spans(items: &[Item<'_>], columns: bool) -> Vec<((usize, usize), u32)> {
    items
        .iter()
        .map(|it| {
            let (ranges, (s, e)) = if columns {
                (it.columns, it.area.columns)
            } else {
                (it.rows, it.area.rows)
            };
            (ranges, u32::try_from(e - s).unwrap_or(1))
        })
        .collect()
}

/// The items with their areas and ranges.
fn grid_items<'a>(
    boxes: &[&'a IndependentBox],
    placement: &Placement,
    columns: &Tracks,
    rows: &Tracks,
) -> Vec<Item<'a>> {
    boxes
        .iter()
        .zip(&placement.areas)
        .map(|(&b, &area)| {
            let row_ranges = (rows.range_at(area.rows.0), rows.range_at(area.rows.1));
            let row_span = u32::try_from(area.rows.1 - area.rows.0).unwrap_or(1);
            Item {
                box_: b,
                style: b.base.style.as_ref(),
                area,
                columns: (
                    columns.range_at(area.columns.0),
                    columns.range_at(area.columns.1),
                ),
                rows: row_ranges,
                outer: None,
                block: None,
                row_fixed_max: rows.fixed_max_span(rows.sets_of(row_ranges), row_span),
            }
        })
        .collect()
}

/// The areas of the items in one axis.
fn axis_areas(placement: &Placement, columns: bool) -> Vec<(i32, i32)> {
    placement
        .areas
        .iter()
        .map(|a| if columns { a.columns } else { a.rows })
        .collect()
}

/// §12.1 step 1 under a min- or max-content constraint: the
/// min-content and max-content widths of the grid's columns.
fn compute_content_sizes(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    children: &[IndependentBox],
) -> ContentSizes {
    let style = base.style.as_ref();
    let boxes = in_flow_items(children);
    let edges = BoxEdges::resolve(style, 0.0).sum();
    let (min_width, max_width) = axis_limits(
        (&style.min_width, &style.max_width),
        None,
        style.box_sizing,
        edges.horizontal(),
    );
    let column_gap = gap(&style.column_gap, None);
    let column_repeat = auto_repetitions(
        &style.grid_template_columns,
        column_gap,
        None,
        min_width,
        max_width,
    );
    // The available block size, as in `size_grid`.
    let height = resolve_size(&style.height, None, style.box_sizing, edges.vertical())
        .map(|h| clamp_height(style, h, None, edges.vertical()));
    let (min_height, max_height) = axis_limits(
        (&style.min_height, &style.max_height),
        None,
        style.box_sizing,
        edges.vertical(),
    );
    let row_gap = gap(&style.row_gap, height);
    let row_repeat = auto_repetitions(
        &style.grid_template_rows,
        row_gap,
        height,
        min_height,
        max_height,
    );
    let placement = placement(ctx, base, &boxes, column_repeat, row_repeat);
    let (column_template, row_template) = templates(style, column_repeat, row_repeat);
    let mut columns = Tracks::new(
        &column_template,
        placement.columns,
        &axis_areas(&placement, true),
        None,
        column_gap,
    );
    // The rows are not sized; they give the items their row ranges and
    // the fixed maximums of their rows (§12.1 step 1).
    let rows = Tracks::new(
        &row_template,
        placement.rows,
        &axis_areas(&placement, false),
        height,
        row_gap,
    );
    let mut items = grid_items(&boxes, &placement, &columns, &rows);
    let mut total = |constraint: Constraint| {
        columns.reset();
        let input = SizingInput {
            constraint,
            available: None,
            min_available: min_width,
            stretch: stretches_tracks(style.justify_content),
        };
        size_columns(ctx, &mut columns, &mut items, &input, style);
        columns.total_size()
    };
    let max = total(Constraint::MaxContent);
    let min = total(Constraint::MinContent);
    ContentSizes {
        min,
        max: max.max(min),
    }
}

/// Runs the track sizing algorithm for the columns.
fn size_columns(
    ctx: &mut LayoutContext<'_>,
    columns: &mut Tracks,
    items: &mut [Item<'_>],
    input: &SizingInput,
    container: &ComputedStyle,
) {
    let (sizing, rules) = sizing_items(columns, &spans(items, true));
    sizing::size_tracks(columns, &sizing, input, &mut |i, kind| {
        inline_contribution(ctx, &mut items[i], kind, rules[i], container)
    });
}

/// Runs the track sizing algorithm for the rows, with the items' heights
/// in areas of `area_widths`.
fn size_rows(
    ctx: &mut LayoutContext<'_>,
    rows: &mut Tracks,
    items: &mut [Item<'_>],
    area_widths: &[f32],
    input: &SizingInput,
    container: &ComputedStyle,
) {
    let (sizing, rules) = sizing_items(rows, &spans(items, false));
    sizing::size_tracks(rows, &sizing, input, &mut |i, kind| {
        let width = area_widths.get(i).copied().unwrap_or(0.0);
        let h = block_contribution(ctx, &mut items[i], width, container);
        match kind {
            Contribution::Minimum => block_minimum(&items[i], width, h, rules[i]),
            _ => h,
        }
    });
}

/// The laid-out content of a grid container: its item fragments relative
/// to the content box.
pub(crate) struct GridContents {
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) content_height: f32,
    /// Relative to the content box.
    pub(crate) baselines: Baselines,
    /// The end of the tracks (the in-flow extent for scrolling).
    tracks_end: crate::geom::Size,
}

/// Lays out the content of a grid container (§12.1 and §11).
pub(crate) fn layout_contents(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    children: &[IndependentBox],
    width: f32,
    given_height: Option<f32>,
    cb: ContainingBlock,
) -> GridContents {
    crate::table::TableCache::percent_free(ctx, |ctx| {
        let style = base.style.as_ref();
        let boxes = in_flow_items(children);
        let grid = size_grid(ctx, base, &boxes, width, given_height, cb);
        let (mut fragments, baselines) = place_items(ctx, &grid, style);
        // Absolutely positioned children are laid out after the document
        // (`positioned.rs`).
        add_placeholders(ctx, children, &mut fragments, StaticParent::Grid);
        GridContents {
            fragments,
            content_height: grid.content_height,
            baselines,
            tracks_end: crate::geom::Size::new(
                tracks_end(&grid.column_positions),
                tracks_end(&grid.row_positions),
            ),
        }
    })
}

/// Lays out a grid container `base` (§12.1 and §11) with the children
/// `children` and a content-box width of `width`; its content-box height
/// is `given_height` if given (a stretched flex item), else from its
/// style or its rows. The fragment is at (0, 0).
pub(crate) fn layout(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    children: &[IndependentBox],
    width: f32,
    given_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let style = base.style.as_ref();
    let edges = BoxEdges::resolve(style, cb.width);
    let contents = layout_contents(ctx, base, children, width, given_height, cb);
    let mut fragment = finish_fragment(
        base,
        Rect::new(
            0.0,
            0.0,
            width + edges.sum().horizontal(),
            contents.content_height + edges.sum().vertical(),
        ),
        &edges,
        contents.fragments,
        contents.baselines.offset(edges.sum().top),
    );
    if crate::scroll::is_scroll_container(style) {
        fragment.scrollable_overflow = Some(crate::scroll::scrollable_overflow(
            &fragment,
            contents.tracks_end,
        ));
    }
    fragment
}

/// The end of the last track of `positions` (relative to the content box),
/// or 0 without tracks. As in Chromium, the in-flow content of a grid
/// container that is a scroll container is its tracks, after content
/// alignment. The margins of its items do not count (unlike flex items);
/// the items count as descendants, with their border boxes.
fn tracks_end(positions: &[(f32, f32)]) -> f32 {
    positions.last().map_or(0.0, |p| p.1)
}

/// A grid after track sizing: its items, the start and end of each
/// range of columns and rows, the width of each item's area, and the
/// content-box height.
struct SizedGrid<'a> {
    items: Vec<Item<'a>>,
    column_positions: Vec<(f32, f32)>,
    row_positions: Vec<(f32, f32)>,
    area_widths: Vec<f32>,
    content_height: f32,
}

/// The templates of the columns and the rows of a grid container with
/// style `style`.
fn templates(
    style: &ComputedStyle,
    column_repeat: u32,
    row_repeat: u32,
) -> (Template<'_>, Template<'_>) {
    let areas = style.grid_template_areas.as_deref();
    let columns = Template::new(
        &style.grid_template_columns,
        &style.grid_auto_columns,
        column_repeat,
        areas.map_or(0, |a| a.columns),
    );
    let rows = Template::new(
        &style.grid_template_rows,
        &style.grid_auto_rows,
        row_repeat,
        areas.map_or(0, |a| a.rows),
    );
    (columns, rows)
}

/// §12.1 step 1: sizes the columns for a content-box width of `width` and
/// applies `justify-content` (so that the rows see the distributed gaps).
/// Returns the positions of the column ranges and each item's area width.
fn layout_columns(
    ctx: &mut LayoutContext<'_>,
    columns: &mut Tracks,
    items: &mut [Item<'_>],
    width: f32,
    container: &ComputedStyle,
) -> (Vec<(f32, f32)>, Vec<f32>) {
    let input = SizingInput {
        constraint: Constraint::Layout,
        available: Some(width),
        min_available: width,
        stretch: stretches_tracks(container.justify_content),
    };
    size_columns(ctx, columns, items, &input, container);
    let positions = track_positions(columns, container.justify_content, Some(width));
    let area_widths = items
        .iter()
        .map(|it| span_size(&positions, it.columns))
        .collect();
    (positions, area_widths)
}

/// The content-box `min-*` (0 if unresolved) and `max-*` (`None` if
/// unresolved) of one axis, against `basis` with padding and border `edges`.
fn axis_limits(
    (min, max): (&Size, &MaxSize),
    basis: Option<f32>,
    box_sizing: BoxSizing,
    edges: f32,
) -> (f32, Option<f32>) {
    (
        resolve_size(min, basis, box_sizing, edges).unwrap_or(0.0),
        resolve_max_size(max, basis, box_sizing, edges),
    )
}

/// Places the items `boxes` of grid container `base` and sizes its tracks
/// (§12.1 steps 1, 2 and 5) for a content-box width of `width` (and a
/// height of `given_height`, if given).
fn size_grid<'a>(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    boxes: &[&'a IndependentBox],
    width: f32,
    given_height: Option<f32>,
    cb: ContainingBlock,
) -> SizedGrid<'a> {
    let style = base.style.as_ref();
    let v_edges = BoxEdges::resolve(style, cb.width).sum().vertical();
    // The available block size: a definite height (clamped), else
    // indefinite (Chromium's `ComputeBlockSizeForFragment` with an
    // indefinite intrinsic size).
    let height = given_height.or_else(|| {
        resolve_size(&style.height, cb.height, style.box_sizing, v_edges)
            .map(|h| clamp_height(style, h, cb.height, v_edges))
    });
    let (min_height, max_height) = axis_limits(
        (&style.min_height, &style.max_height),
        cb.height,
        style.box_sizing,
        v_edges,
    );
    let column_gap = gap(&style.column_gap, Some(width));
    let column_repeat = auto_repetitions(
        &style.grid_template_columns,
        column_gap,
        Some(width),
        0.0,
        None,
    );
    let row_gap = gap(&style.row_gap, height);
    let row_repeat = auto_repetitions(
        &style.grid_template_rows,
        row_gap,
        height,
        min_height,
        max_height,
    );
    let placement = placement(ctx, base, boxes, column_repeat, row_repeat);
    let (column_template, row_template) = templates(style, column_repeat, row_repeat);
    let column_areas = axis_areas(&placement, true);
    let row_areas = axis_areas(&placement, false);
    let extent = placement.columns;
    let mut columns = Tracks::new(
        &column_template,
        extent,
        &column_areas,
        Some(width),
        column_gap,
    );
    let new_rows = |height: Option<f32>| {
        let gap = gap(&style.row_gap, height);
        Tracks::new(&row_template, placement.rows, &row_areas, height, gap)
    };
    let mut rows = new_rows(height);
    let mut items = grid_items(boxes, &placement, &columns, &rows);

    let (column_positions, area_widths) =
        layout_columns(ctx, &mut columns, &mut items, width, style);

    // Step 2: the rows, with the items' heights at their area widths.
    let row_input = |available: Option<f32>| SizingInput {
        constraint: Constraint::Layout,
        available,
        min_available: min_height,
        stretch: stretches_tracks(style.align_content),
    };
    let input = row_input(height);
    size_rows(ctx, &mut rows, &mut items, &area_widths, &input, style);
    let content_height = if let Some(h) = height {
        h
    } else {
        let h = clamp_height(style, rows.total_size(), cb.height, v_edges);
        let percent_gap =
            matches!(&style.row_gap, Gap::LengthPercentage(lp) if lp.has_percentage());
        if rows.depends_on_available || percent_gap {
            // Chromium's additional pass: the rows again with the resolved
            // height, for flexible and percentage tracks.
            rows = new_rows(Some(h));
            let input = row_input(Some(h));
            size_rows(ctx, &mut rows, &mut items, &area_widths, &input, style);
        }
        h
    };
    SizedGrid {
        row_positions: track_positions(&rows, style.align_content, Some(content_height)),
        items,
        column_positions,
        area_widths,
        content_height,
    }
}

/// Lays out the items of a sized grid in their areas and aligns them
/// (§11). Returns their fragments in order-modified document order and
/// the grid's baselines.
fn place_items(
    ctx: &mut LayoutContext<'_>,
    grid: &SizedGrid<'_>,
    container: &ComputedStyle,
) -> (Vec<Fragment>, Baselines) {
    let items = &grid.items;
    let mut placed: Vec<(usize, BoxFragment, f32, Edge)> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let x0 = grid
            .column_positions
            .get(item.columns.0)
            .map_or(0.0, |p| p.0);
        let y0 = grid.row_positions.get(item.rows.0).map_or(0.0, |p| p.0);
        let area = (
            grid.area_widths[i],
            span_size(&grid.row_positions, item.rows),
        );
        let (fragment, y_margin, edge) = layout_item(ctx, item, (x0, y0), area, container);
        placed.push((i, fragment, y_margin, edge));
    }
    let shared = align_baselines(items, &mut placed);
    let baselines = grid_baselines(items, &placed, &shared, &grid.row_positions);
    let fragments = placed
        .into_iter()
        .map(|(i, mut fragment, ..)| {
            let cb = ContainingBlock {
                width: grid.area_widths[i],
                height: Some(span_size(&grid.row_positions, items[i].rows)),
            };
            apply_relative_position(&mut fragment, cb);
            Fragment::Box(fragment)
        })
        .collect();
    (fragments, baselines)
}

/// The size of the ranges `ranges` (an item's area) from their
/// positions.
fn span_size(positions: &[(f32, f32)], ranges: (usize, usize)) -> f32 {
    let start = positions.get(ranges.0).map_or(0.0, |p| p.0);
    let end = ranges
        .1
        .checked_sub(1)
        .and_then(|i| positions.get(i))
        .map_or(start, |p| p.1);
    (end - start).max(0.0)
}

/// Lays out an item in its area at `(x0, y0)` with the size
/// `(area_width, area_height)`. Returns the fragment at
/// its aligned position (before relative positioning), its top margin and
/// its block-axis alignment edge.
fn layout_item(
    ctx: &mut LayoutContext<'_>,
    item: &Item<'_>,
    (x0, y0): (f32, f32),
    (area_width, area_height): (f32, f32),
    container: &ComputedStyle,
) -> (BoxFragment, f32, Edge) {
    let style = item.style;
    let width = inline_size(ctx, item, (area_width, Some(area_height)), container);
    let margin_top = style.margin_top.resolve(area_width);
    let margin_bottom = style.margin_bottom.resolve(area_width);
    let margin_left = style.margin_left.resolve(area_width);
    let margin_right = style.margin_right.resolve(area_width);
    // A container with an aspect ratio and a width that is definite or
    // stretched takes its height from its width, not from the area.
    let height = if crate::aspect::has_ratio(item.box_)
        && (!item.style.width.is_auto() || stretches_inline(item, container, area_width))
    {
        None
    } else {
        stretched_height(item, (area_width, area_height), container)
    };
    let mut fragment = final_layout(ctx, item, width, height, (area_width, area_height));
    let x_edge = item_edge(
        item.justify(container),
        margin_left.is_none(),
        margin_right.is_none(),
    );
    let y_edge = item_edge(
        item.align(container),
        margin_top.is_none(),
        margin_bottom.is_none(),
    );
    let mt = margin_top.unwrap_or(0.0);
    fragment.border_rect.x = x0
        + align_offset(
            area_width,
            fragment.border_rect.width,
            margin_left.unwrap_or(0.0),
            margin_right.unwrap_or(0.0),
            x_edge,
        );
    fragment.border_rect.y = y0
        + align_offset(
            area_height,
            fragment.border_rect.height,
            mt,
            margin_bottom.unwrap_or(0.0),
            y_edge,
        );
    (fragment, mt, y_edge.0)
}

/// Lays out an item with a content-box `width` and `height` (if
/// stretched) in its area. The layout that measured the item's block
/// size is reused (from the layout cache) when the result cannot differ:
/// the item has no percentage heights (which depend on the area's
/// height), and it is not stretched, or stretched to the height that it
/// measured while nothing depends on that height becoming definite (see
/// [`definite_height_is_neutral`]).
fn final_layout(
    ctx: &mut LayoutContext<'_>,
    item: &Item<'_>,
    width: f32,
    height: Option<f32>,
    (area_width, area_height): (f32, f32),
) -> BoxFragment {
    if item.block.is_some_and(|(w, _)| w == area_width) && !has_percentage_height(item.style) {
        let cb = ContainingBlock {
            width: area_width,
            height: None,
        };
        let measured = layout_flex_item(ctx, item.box_, width, None, cb);
        let v_edges = BoxEdges::resolve(item.style, area_width).sum().vertical();
        let same = height.is_none_or(|h| {
            (h + v_edges - measured.border_rect.height).abs() <= 0.01
                && definite_height_is_neutral(item)
                && !subtree_has_percentage_height(&measured)
        });
        if same {
            return measured;
        }
    }
    let cb = ContainingBlock {
        width: area_width,
        height: Some(area_height),
    };
    layout_flex_item(ctx, item.box_, width, height, cb)
}

/// True if giving an item its measured height as a definite height
/// cannot change its layout: its own `min-height` and `max-height` did
/// not clamp the measured height (they are `auto` or 0, and `none`), so
/// the content measured with an indefinite height already had that height
/// (a flex or grid item would size its tracks or flexible items against
/// the clamped height otherwise), it has no percentage `row-gap` (a flex
/// container resolves it against a definite height only), and it is not a
/// grid whose rows repeat automatically (their number depends on a
/// definite height).
///
/// Only the item's own style is checked. This is enough while flex
/// layout does not give the stretched children of a flex container with
/// a definite height a definite height too (Chromium does); when it does,
/// those children need the same checks.
fn definite_height_is_neutral(item: &Item<'_>) -> bool {
    let style = item.style;
    let min_neutral = match &style.min_height {
        Size::Auto => true,
        Size::LengthPercentage(lp) => lp.is_zero(),
        _ => false,
    };
    let percent_gap = matches!(&style.row_gap, Gap::LengthPercentage(lp) if lp.has_percentage());
    let auto_rows = matches!(item.box_.contents, IndependentContents::Grid(_))
        && style.grid_template_rows.auto_repeat().is_some();
    min_neutral && matches!(style.max_height, MaxSize::None) && !percent_gap && !auto_rows
}

/// True if a box has a percentage `height`, `min-height` or `max-height`,
/// or (if relatively positioned) a percentage `top` or `bottom`.
fn has_percentage_height(style: &ComputedStyle) -> bool {
    let percent = |lp: Option<&LengthPercentage>| lp.is_some_and(LengthPercentage::has_percentage);
    percent(style.height.as_length_percentage())
        || percent(style.min_height.as_length_percentage())
        || percent(style.max_height.as_length_percentage())
        || (style.position == swb_style::Position::Relative
            && (percent(style.top.non_auto()) || percent(style.bottom.non_auto())))
}

/// True if a box inside `fragment` has a percentage height (see
/// [`has_percentage_height`]) or is an item of a column flex container
/// with a percentage `flex-basis` (which resolves against the
/// container's height).
fn subtree_has_percentage_height(fragment: &BoxFragment) -> bool {
    let percent_basis = |s: &ComputedStyle| match &s.flex_basis {
        swb_style::FlexBasis::Size(size) => size
            .as_length_percentage()
            .is_some_and(LengthPercentage::has_percentage),
        swb_style::FlexBasis::Content => false,
    };
    let mut stack: Vec<&BoxFragment> = vec![fragment];
    while let Some(f) = stack.pop() {
        let column_flex = matches!(
            f.style.display,
            swb_style::Display::Flex | swb_style::Display::InlineFlex
        ) && !f.style.flex_direction.is_row();
        for child in f.children.iter() {
            if let Fragment::Box(b) = child {
                if has_percentage_height(&b.style) || (column_flex && percent_basis(&b.style)) {
                    return true;
                }
                stack.push(b);
            }
        }
    }
    false
}

/// Aligns the first baselines of the items with `align-self: baseline`
/// that start in the same row (CSS Box Alignment 3 §9.3, applied after
/// track sizing only).
///
/// Returns the shared baseline of each row (by range index) that has such
/// items, from the top of the row.
fn align_baselines(
    items: &[Item<'_>],
    placed: &mut [(usize, BoxFragment, f32, Edge)],
) -> HashMap<usize, f32> {
    let baseline = |f: &BoxFragment| f.first_baseline.unwrap_or(f.border_rect.height);
    let mut shared: HashMap<usize, f32> = HashMap::new();
    for (i, f, margin, edge) in placed.iter() {
        if *edge == Edge::Baseline {
            let b = margin + baseline(f);
            let e = shared.entry(items[*i].rows.0).or_insert(b);
            *e = e.max(b);
        }
    }
    for (i, f, margin, edge) in placed.iter_mut() {
        if *edge == Edge::Baseline
            && let Some(&b) = shared.get(&items[*i].rows.0)
        {
            f.border_rect.y += b - (*margin + baseline(f));
        }
    }
    shared
}

/// The baselines of the grid (§11.6, Chromium's
/// `GridBaselineAccumulator`): the shared baseline of the baseline-aligned
/// items of the first (last) row that has items, else the first (last)
/// baseline of the item that comes first (ends last) in row-major order,
/// synthesized from its border box if it has none. `shared` has the
/// shared baselines of the rows (from [`align_baselines`]).
fn grid_baselines(
    items: &[Item<'_>],
    placed: &[(usize, BoxFragment, f32, Edge)],
    shared: &HashMap<usize, f32>,
    row_positions: &[(f32, f32)],
) -> Baselines {
    let row_baseline = |row: usize| {
        let b = shared.get(&row)?;
        Some(row_positions.get(row)?.0 + b)
    };
    let first_row = placed.iter().map(|(i, ..)| items[*i].rows.0).min();
    let last_row = placed
        .iter()
        .map(|(i, ..)| items[*i].rows.1.saturating_sub(1))
        .max();
    let first = placed.iter().min_by_key(|(i, ..)| {
        let a = items[*i].area;
        (a.rows.0, a.columns.0)
    });
    // `max_by_key` returns the last of equal items, as Chromium prefers.
    let last = placed.iter().max_by_key(|(i, ..)| {
        let a = items[*i].area;
        (a.rows.1, a.columns.1)
    });
    Baselines {
        first: first_row.and_then(row_baseline).or_else(|| {
            first.map(|(_, f, ..)| {
                f.border_rect.y + f.first_baseline.unwrap_or(f.border_rect.height)
            })
        }),
        last: last_row.and_then(row_baseline).or_else(|| {
            last.map(|(_, f, ..)| f.border_rect.y + f.last_baseline.unwrap_or(f.border_rect.height))
        }),
    }
}

#[cfg(test)]
mod tests;
