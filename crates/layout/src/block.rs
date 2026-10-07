//! Block layout: block formatting contexts, widths and heights of block
//! boxes, vertical margin collapsing, floats among blocks, clearance, and
//! the placement of boxes that establish a block formatting context next to
//! floats.
//!
//! CSS 2.2 §8.3.1 (margin collapsing), §9.5 (floats), §10.3 (widths),
//! §10.6 (heights): <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>,
//! <https://www.w3.org/TR/CSS22/visuren.html#floats>,
//! <https://www.w3.org/TR/CSS22/visudet.html>.
//!
//! Block boxes know their position in the block formatting context (BFC)
//! while they lay out their children, because line boxes and boxes that
//! establish a BFC avoid floats. A box whose top margin can still collapse
//! with its children's does not know its position yet; see `floats.rs` for
//! how that position is resolved and how floats wait for it. Floats are
//! placed in the exclusion space of their BFC (`floats.rs`); the auto height
//! of a BFC root includes its floats (§10.6.7).
//!
//! An absolutely positioned box gets a placeholder at its static position;
//! `positioned.rs` lays it out later.

use std::sync::Arc;

use swb_style::{BoxSizing, Clear, ComputedStyle, Display, LengthPercentageOrAuto, MaxSize, Size};

use crate::box_tree::{
    BlockContainer, BlockLevelBox, BoxBase, IndependentBox, IndependentContents, Marker,
};
use crate::floats::{
    EPSILON, FloatBox, Opportunity, PendingFloat, PlacedFloat, Side, move_fragment,
};
use crate::fragment::{BoxContent, BoxFragment, Fragment};
use crate::geom::{Edges, Point, Rect};
use crate::list_marker::{PendingMarker, markers_at_baseline, shift_markers};
use crate::positioned::{StaticParent, placeholder};
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

/// Where the children of a block container are laid out, in the
/// coordinates of the block formatting context.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flow {
    /// The BFC x of the container's content box.
    pub(crate) x: f32,
    pub(crate) y: FlowY,
}

/// The block position of a container's content box.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FlowY {
    /// The BFC y of the content box.
    Resolved(f32),
    /// Not known yet: the container's top margin can still collapse with
    /// its children's. The margins start at `before`; `strut` are the
    /// margins so far, the container's own included.
    Pending { before: f32, strut: CollapsedMargin },
}

impl Flow {
    /// The content box of a BFC root: the origin of its BFC.
    pub(crate) const ROOT: Flow = Flow {
        x: 0.0,
        y: FlowY::Resolved(0.0),
    };

    fn resolved_y(self) -> Option<f32> {
        match self.y {
            FlowY::Resolved(y) => Some(y),
            FlowY::Pending { .. } => None,
        }
    }
}

/// Where an in-flow block-level child starts: the BFC x of its containing
/// block's content box, and where its margins start after the margins
/// `strut` (its own not included).
#[derive(Clone, Copy, Debug)]
struct ChildPlace {
    x: f32,
    before: f32,
    strut: CollapsedMargin,
    /// The BFC y of the containing block's content box, if known.
    container_y: Option<f32>,
    /// The child's border-box top if it clears floats that waited for the
    /// position of its container (see [`ChildCursor::force_clearance`]).
    forced: Option<f32>,
}

/// The container of in-flow block-level children.
#[derive(Clone, Copy)]
struct Container<'s> {
    cb: ContainingBlock,
    flow: Flow,
    style: &'s ComputedStyle,
}

/// A laid-out block box that does not establish a BFC.
struct BlockResult {
    laid_out: LaidOutBlock,
    /// The BFC y of its border-box top, if known: always, unless its
    /// margins collapse through it.
    bfc_y: Option<f32>,
    /// True for an empty box placed by clearance: its margins collapse
    /// with the following ones, but not with its parent's bottom margin.
    cleared: bool,
    /// Floats in it that wait for its position (its margins collapse
    /// through it and its parent's position is not known).
    floats: Vec<PendingFloat>,
}

impl BlockResult {
    /// A box placed at BFC y `top`, with no waiting floats.
    fn placed(laid_out: LaidOutBlock, top: f32) -> Self {
        BlockResult {
            laid_out,
            bfc_y: Some(top),
            cleared: false,
            floats: Vec::new(),
        }
    }
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
    HeightLimits::of(style, cb_height, edges).clamp(height)
}

/// The used `min-height` and `max-height` of a box, as content-box
/// heights. `max` is at least `min`: the minimum wins (CSS 2.2 §10.7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HeightLimits {
    min: f32,
    max: f32,
}

impl HeightLimits {
    /// No limits: `min-height: 0` and `max-height: none`.
    pub(crate) const NONE: HeightLimits = HeightLimits {
        min: 0.0,
        max: f32::INFINITY,
    };

    /// Limits from a minimum and a maximum; the minimum wins.
    pub(crate) fn new(min: f32, max: f32) -> Self {
        HeightLimits {
            min,
            max: max.max(min),
        }
    }

    /// The limits of a box with `style` in a containing block of height
    /// `cb_height` (percentages of an indefinite height are ignored).
    /// `edges` is the vertical padding and border.
    pub(crate) fn of(style: &ComputedStyle, cb_height: Option<f32>, edges: f32) -> Self {
        HeightLimits::new(
            resolve_size(&style.min_height, cb_height, style.box_sizing, edges).unwrap_or(0.0),
            resolve_max_size(&style.max_height, cb_height, style.box_sizing, edges)
                .unwrap_or(f32::INFINITY),
        )
    }

    /// The used max-height (infinite for `none`).
    pub(crate) fn max(self) -> f32 {
        self.max
    }

    /// Clamps `height` (NaN becomes the minimum).
    pub(crate) fn clamp(self, height: f32) -> f32 {
        height.max(self.min).min(self.max)
    }
}

/// A margin; `auto` is 0.
pub(crate) fn margin_or_zero(m: &LengthPercentageOrAuto, cb_width: f32) -> f32 {
    m.resolve(cb_width).unwrap_or(0.0)
}

/// Used width and horizontal margins of a block-level box in normal flow
/// (CSS 2.2 §10.3.3 and §10.4) in a space `available` px wide (the
/// containing block's width, or a layout opportunity next to floats with
/// the box's margins). Percentages resolve against the containing block.
/// `width` is the content width if already known (for example from
/// shrink-to-fit); otherwise it comes from the style. Returns (content
/// width, margin-left, margin-right).
fn block_width_and_margins(
    style: &ComputedStyle,
    cb: ContainingBlock,
    available: f32,
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
                ((available - ml - mr - edge_sum).max(0.0), ml, mr)
            }
            Some(w) => {
                let remaining = available - w - edge_sum;
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
    /// The right and bottom edges of the in-flow content, relative to the
    /// content box: the line boxes up to the end of their content, the
    /// margin boxes of in-flow children and floats before relative
    /// positioning (not of blocks inside inline boxes), and the content
    /// height. A scroll container adds its padding to it for its
    /// scrollable overflow (see `scroll.rs`).
    pub(crate) inflow: crate::geom::Size,
}

/// The first and last baselines of a box, relative to some origin.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Baselines {
    pub(crate) first: Option<f32>,
    pub(crate) last: Option<f32>,
}

impl Baselines {
    pub(crate) fn offset(self, dy: f32) -> Baselines {
        Baselines {
            first: self.first.map(|b| b + dy),
            last: self.last.map(|b| b + dy),
        }
    }

    /// The baselines a child contributes to its parent. A scroll
    /// container's last baseline is its block-end margin edge (its content
    /// can be scrolled out of view), as in Chromium and CSS Align 3 §9.1.
    /// A flex or grid container contributes its first baseline as the last
    /// one too (Chromium's `UseLastBaselineForInlineBaseline`).
    fn of(fragment: &BoxFragment, margin_bottom: f32) -> Baselines {
        let style = &fragment.style;
        let scroll_container =
            style.overflow_x.is_scroll_container() || style.overflow_y.is_scroll_container();
        let flex_or_grid = matches!(
            style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        );
        Baselines {
            first: fragment.first_baseline,
            last: if scroll_container {
                Some(fragment.border_rect.height + margin_bottom)
            } else if flex_or_grid {
                fragment.first_baseline
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

/// Lays out the children of a block container at `flow`: returns the
/// child fragments, the content height, and the margins that escaped
/// through the container's top and bottom edges. `markers` are the list
/// markers that wait for a line box (see [`crate::list_marker`]).
pub(crate) fn layout_block_container<'a>(
    ctx: &mut LayoutContext<'_>,
    container: &'a BlockContainer,
    container_style: &Arc<ComputedStyle>,
    cb: ContainingBlock,
    options: ChildOptions,
    markers: &mut Vec<PendingMarker<'a>>,
    flow: Flow,
) -> ChildrenLayout {
    match container {
        BlockContainer::Inline(ifc) => {
            let lines = inline::layout_inline(ctx, ifc, container_style, cb, markers, flow);
            // The line boxes up to the end of their content, and the floats
            // that count as in-flow content.
            let floats = lines.in_flow_floats;
            let inflow = crate::geom::Size::new(
                lines.content_right.max(floats.width),
                lines.height.max(floats.height),
            );
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
                inflow,
            }
        }
        BlockContainer::Blocks(children) => {
            layout_block_children(ctx, children, container_style, cb, options, markers, flow)
        }
    }
}

/// Lays out the contents of a box that establishes a new block formatting
/// context, in its content box `cb`. The content height includes the
/// floats (CSS 2.2 §10.6.7).
pub(crate) fn layout_flow_root<'a>(
    ctx: &mut LayoutContext<'_>,
    container: &'a BlockContainer,
    style: &Arc<ComputedStyle>,
    cb: ContainingBlock,
    markers: &mut Vec<PendingMarker<'a>>,
) -> ChildrenLayout {
    ctx.push_bfc();
    let options = ChildOptions {
        collapse_with_parent_start: false,
        collapse_with_parent_end: false,
    };
    let mut children =
        layout_block_container(ctx, container, style, cb, options, markers, Flow::ROOT);
    if let Some(bottom) = ctx.pop_bfc() {
        children.content_height = children.content_height.max(bottom);
    }
    children
}

/// The state of the loop over the children of a block container.
struct ChildCursor {
    /// The end of the last child with content, relative to the content
    /// box.
    y: f32,
    /// The margins after it.
    pending: CollapsedMargin,
    /// The margins that collapse with the container's top margin.
    start_margin: CollapsedMargin,
    /// True while every child collapsed through and the container's top
    /// margin collapses with them.
    at_start: bool,
    /// The BFC y of the content box, once known.
    content_y: Option<f32>,
    /// True if every child so far collapsed through.
    all_collapsed_through: bool,
    baselines: Baselines,
    /// True if `pending` holds the margins of an empty box with clearance.
    after_clearance: bool,
}

impl ChildCursor {
    fn new(flow: Flow, options: ChildOptions) -> Self {
        ChildCursor {
            y: 0.0,
            pending: CollapsedMargin::default(),
            start_margin: CollapsedMargin::default(),
            at_start: options.collapse_with_parent_start,
            content_y: flow.resolved_y(),
            all_collapsed_through: true,
            baselines: Baselines::default(),
            after_clearance: false,
        }
    }

    /// Where the next child's margins start, and the margins before it.
    fn place(&self, flow: Flow) -> ChildPlace {
        let (before, strut, container_y) = match (self.content_y, flow.y) {
            (Some(content_y), _) => (content_y + self.y, self.pending, Some(content_y)),
            (None, FlowY::Pending { before, strut }) => {
                (before, strut.adjoin(self.start_margin), None)
            }
            (None, FlowY::Resolved(y)) => (y, self.pending, Some(y)),
        };
        ChildPlace {
            x: flow.x,
            before,
            strut,
            container_y,
            forced: None,
        }
    }

    /// If the next child has `clear` and clears floats that wait for the
    /// position of the container (Chromium's
    /// `HasClearancePastAdjoiningFloats`): resolves that position (and the
    /// position of the containers whose position waits for it) before the
    /// child's margins, which places the floats, and returns the child's
    /// border-box top. That is its clearance offset, or the position
    /// before its margins if that is lower. Its margins, and the margins
    /// that collapse with them, have no effect (Chromium's forced BFC block
    /// offset).
    fn force_clearance(
        &mut self,
        ctx: &mut LayoutContext<'_>,
        flow: Flow,
        clear: Clear,
    ) -> Option<f32> {
        if self.content_y.is_some() || !ctx.bfc().clears_pending(clear) {
            return None;
        }
        let strut = self.place(flow).strut;
        let content_y = ctx.bfc().resolve(strut);
        self.content_y = Some(content_y);
        self.at_start = false;
        self.pending = CollapsedMargin::default();
        Some(cleared_position(ctx, clear, content_y + self.y))
    }

    /// Positions a laid-out in-flow child, the fragment with index `index`
    /// among the container's children. Returns its fragment, and its
    /// baselines (relative to the content box) unless it is empty.
    fn position(
        &mut self,
        ctx: &mut LayoutContext<'_>,
        result: BlockResult,
        index: usize,
        cb: ContainingBlock,
    ) -> (BoxFragment, Option<Baselines>) {
        if self.content_y.is_none() {
            self.content_y = ctx.bfc().resolved();
        }
        let BlockResult {
            laid_out:
                LaidOutBlock {
                    mut fragment,
                    margins,
                },
            bfc_y,
            cleared,
            floats,
        } = result;
        let baselines = match bfc_y {
            None if margins.collapsed_through => {
                self.add_empty(ctx, &mut fragment, margins, floats, index);
                None
            }
            Some(y) if cleared => {
                self.add_cleared(&mut fragment, margins, y);
                None
            }
            _ => Some(self.add(&mut fragment, margins, bfc_y, cb)),
        };
        (fragment, baselines)
    }

    /// Positions an empty child whose margins collapse through it, and
    /// places its waiting floats (or makes them wait in the container, as
    /// child `index`).
    fn add_empty(
        &mut self,
        ctx: &mut LayoutContext<'_>,
        fragment: &mut BoxFragment,
        margins: BlockMargins,
        floats: Vec<PendingFloat>,
        index: usize,
    ) {
        let combined = margins.start.adjoin(margins.end);
        if let Some(content_y) = self.content_y {
            // The top border edge of an empty box is where it would be with
            // a bottom border (CSS 2.2 §8.3.1): after the margins before it
            // and its top margin only.
            let top = self.y + self.pending.adjoin(margins.start).solve();
            fragment.border_rect.y = top;
            place_waiting_floats(ctx, fragment, floats, content_y + top);
            self.pending = self.pending.adjoin(combined);
        } else {
            self.start_margin = self.start_margin.adjoin(combined);
            fragment.border_rect.y = self.y;
            ctx.bfc().adopt_pending(floats, index);
        }
    }

    /// Positions an empty child placed by clearance at BFC y `bfc_y`. Its
    /// margins collapse with the following margins from its top margin
    /// edge (Chromium).
    fn add_cleared(&mut self, fragment: &mut BoxFragment, margins: BlockMargins, bfc_y: f32) {
        self.all_collapsed_through = false;
        self.at_start = false;
        let top = bfc_y - self.content_y.unwrap_or(0.0);
        fragment.border_rect.y = top;
        self.y = top - margins.start.solve();
        self.pending = margins.start.adjoin(margins.end);
        self.after_clearance = true;
    }

    /// Positions a child that is not empty at BFC y `bfc_y` if known.
    /// Returns the child's baselines (relative to the content box).
    fn add(
        &mut self,
        fragment: &mut BoxFragment,
        margins: BlockMargins,
        bfc_y: Option<f32>,
        cb: ContainingBlock,
    ) -> Baselines {
        self.all_collapsed_through = false;
        let top = match bfc_y {
            Some(y) => y - self.content_y.unwrap_or(0.0),
            None => self.y + self.pending.adjoin(margins.start).solve(),
        };
        if self.at_start {
            self.start_margin = self.start_margin.adjoin(margins.start);
        }
        self.at_start = false;
        fragment.border_rect.y = top;
        let margin_bottom = fragment
            .style
            .margin_bottom
            .resolve(cb.width)
            .unwrap_or(0.0);
        let child = Baselines::of(fragment, margin_bottom).offset(top);
        if self.baselines.first.is_none() {
            self.baselines.first = child.first;
        }
        if child.last.is_some() {
            self.baselines.last = child.last;
        }
        self.y = top + fragment.border_rect.height;
        self.pending = margins.end;
        self.after_clearance = false;
        child
    }

    /// The layout of the children, whose fragments are `fragments` and
    /// whose in-flow content reaches `inflow`.
    fn finish(
        mut self,
        fragments: Vec<Fragment>,
        options: ChildOptions,
        inflow: crate::scroll::InflowExtent,
    ) -> ChildrenLayout {
        if self.after_clearance {
            // Those margins do not collapse with the container's bottom
            // margin.
            self.y += self.pending.solve();
            self.pending = CollapsedMargin::default();
        }
        let (start_margin, end_margin, trailing) = escaping_margins(
            self.start_margin,
            self.pending,
            self.all_collapsed_through,
            options,
        );
        ChildrenLayout {
            fragments,
            content_height: self.y + trailing,
            start_margin,
            end_margin,
            collapsed_through: self.all_collapsed_through,
            baselines: self.baselines,
            inflow: inflow.finish(self.y + trailing),
        }
    }
}

/// The block position `y`, or the clearance offset for `clear` if that is
/// lower.
fn cleared_position(ctx: &mut LayoutContext<'_>, clear: Clear, y: f32) -> f32 {
    ctx.bfc()
        .exclusions
        .clearance(clear)
        .map_or(y, |c| y.max(c))
}

fn layout_block_children<'a>(
    ctx: &mut LayoutContext<'_>,
    children: &'a [BlockLevelBox],
    container_style: &ComputedStyle,
    cb: ContainingBlock,
    options: ChildOptions,
    markers: &mut Vec<PendingMarker<'a>>,
    flow: Flow,
) -> ChildrenLayout {
    let mut fragments = Vec::with_capacity(children.len());
    let mut c = ChildCursor::new(flow, options);
    let container = Container {
        cb,
        flow,
        style: container_style,
    };
    let mut inflow = crate::scroll::InflowExtent::default();
    for entry in children {
        let (child, inline_boxes, in_positioned_inline) = match entry {
            BlockLevelBox::InInline(b) => (&b.block, Some(&b.inline_boxes), b.in_positioned_inline),
            other => (other, None, false),
        };
        let content_end = c.y;
        let result = match child {
            BlockLevelBox::Block {
                base,
                contents,
                marker,
            } => {
                let forced = c.force_clearance(ctx, flow, base.style.clear);
                let place = ChildPlace {
                    forced,
                    ..c.place(flow)
                };
                layout_block_box(
                    ctx,
                    base,
                    contents,
                    marker.as_ref(),
                    cb,
                    markers,
                    place,
                    container_style,
                )
            }
            BlockLevelBox::Independent(ib) => place_independent(ctx, ib, &mut c, container),
            BlockLevelBox::Float(ib) => {
                let mut fragment = place_block_level_float(ctx, ib, cb, &c, flow, fragments.len());
                fragment.in_positioned_inline = in_positioned_inline;
                inflow.add(&fragment, cb, false);
                fragments.push(Fragment::Box(fragment));
                continue;
            }
            BlockLevelBox::AbsolutelyPositioned(ib) => {
                // The static-position rectangle: the content box's width.
                let at = Point::new(0.0, c.y + c.pending.solve());
                let extent = crate::geom::Size::new(cb.width, 0.0);
                fragments.push(placeholder(ctx, ib, at, StaticParent::Flow, extent));
                continue;
            }
            // Box construction never nests these.
            BlockLevelBox::InInline(_) => continue,
        };
        let index = fragments.len() + inline_boxes.map_or(0, Vec::len);
        let (mut fragment, baselines) = c.position(ctx, result, index, cb);
        fragment.in_positioned_inline = in_positioned_inline;
        if matches!(child, BlockLevelBox::Independent(_))
            && !markers.is_empty()
            && let Some(baseline) = baselines.and_then(|b| b.first)
        {
            // A marker waiting for a line box aligns with the baseline of a
            // child with its own formatting context.
            fragments.extend(markers_at_baseline(ctx, markers, baseline));
            markers.clear();
        }
        if let Some(boxes) = inline_boxes {
            fragments.extend(inline_box_wrappers(boxes, &fragment, content_end, cb));
        }
        apply_relative_position(&mut fragment, cb);
        inflow.add(&fragment, cb, inline_boxes.is_some());
        fragments.push(Fragment::Box(fragment));
    }
    c.finish(fragments, options, inflow)
}

/// Places the waiting floats of an empty child (`fragment`, whose top is
/// at BFC y `top`) at its position.
fn place_waiting_floats(
    ctx: &mut LayoutContext<'_>,
    fragment: &mut BoxFragment,
    floats: Vec<PendingFloat>,
    top: f32,
) {
    for float in floats {
        let position = ctx.bfc().place_waiting(&float, top);
        move_fragment(
            Arc::make_mut(&mut fragment.children).as_mut_slice(),
            &float.path,
            position,
        );
    }
}

/// Moves the floats of a container that were placed while they waited.
fn move_placed_floats(fragments: &mut [Fragment], placed: &[PlacedFloat]) {
    for float in placed {
        move_fragment(fragments, &float.path, float.position);
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

/// The boxes of the inline boxes around a block-level child (Chromium's
/// block-in-inline): as wide as the containing block, from the child's top
/// margin edge (but not above the end of the previous content, so a
/// collapsed margin does not count) to its bottom margin edge. They only
/// have geometry: Chromium paints no inline box background there.
fn inline_box_wrappers(
    boxes: &[BoxBase],
    child: &BoxFragment,
    content_end: f32,
    cb: ContainingBlock,
) -> Vec<Fragment> {
    let margin = |m: &LengthPercentageOrAuto| margin_or_zero(m, cb.width);
    let top = content_end.max(child.border_rect.y - margin(&child.style.margin_top));
    let bottom = child.border_rect.bottom() + margin(&child.style.margin_bottom);
    boxes
        .iter()
        .map(|base| {
            let rect = Rect::new(0.0, top, cb.width, (bottom - top).max(0.0));
            let mut fragment = finish_fragment(
                base,
                rect,
                &BoxEdges::default(),
                Vec::new(),
                Baselines::default(),
            );
            fragment.content = BoxContent::GeometryOnly;
            fragment.is_inline = true;
            Fragment::Box(fragment)
        })
        .collect()
}

/// The horizontal offset of an in-flow block-level child for the
/// `-webkit-left`, `-webkit-center` and `-webkit-right` values of its
/// parent's `text-align`, if the child has no `auto` margins (Chromium's
/// `WebkitTextAlignAndJustifySelfOffset`; HTML's `<center>` and `align`
/// attributes use them).
fn webkit_align_offset(parent: &ComputedStyle, child: &BoxFragment, cb: ContainingBlock) -> f32 {
    webkit_align_offset_in(parent, &child.style, child.border_rect.width, cb, cb.width)
}

/// [`webkit_align_offset`] for a child with `style` and a border box
/// `width` px wide, in a space `space` px wide (the containing block, or a
/// layout opportunity next to floats with the child's margins).
fn webkit_align_offset_in(
    parent: &ComputedStyle,
    style: &ComputedStyle,
    width: f32,
    cb: ContainingBlock,
    space: f32,
) -> f32 {
    let (Some(left), Some(right)) = (
        style.margin_left.resolve(cb.width),
        style.margin_right.resolve(cb.width),
    ) else {
        return 0.0;
    };
    let free = (space - width - left - right).max(0.0);
    match parent.text_align {
        swb_style::TextAlign::WebkitCenter => free / 2.0,
        swb_style::TextAlign::WebkitRight => free,
        _ => 0.0,
    }
}

/// Lays out a float with its shrink-to-fit width in `cb`, whose content
/// box starts at BFC x `cb_x`. The fragment's position is not set.
pub(crate) fn layout_float(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
    cb_x: f32,
) -> (BoxFragment, FloatBox) {
    let lb = layout_independent_shrink_to_fit(ctx, ib, cb);
    let style = &ib.base.style;
    let margin_left = margin_or_zero(&style.margin_left, cb.width);
    let margin_right = margin_or_zero(&style.margin_right, cb.width);
    let margin_top = lb.margins.start.solve();
    let margin_bottom = lb.margins.end.solve();
    let size = lb.fragment.border_rect;
    let (dx, dy) = relative_offset(style, cb);
    let float = FloatBox {
        side: match style.float {
            swb_style::Float::Right => Side::Right,
            _ => Side::Left,
        },
        clear: style.clear,
        width: margin_left + size.width + margin_right,
        height: margin_top + size.height + margin_bottom,
        offset: Point::new(margin_left, margin_top),
        relative: Point::new(dx, dy),
        cb_x,
        cb_width: cb.width,
    };
    (lb.fragment, float)
}

/// Lays out a float among block-level siblings: placed at the current
/// position if the container's position is known, else it waits (as the
/// child with index `index`).
fn place_block_level_float(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
    c: &ChildCursor,
    flow: Flow,
    index: usize,
) -> BoxFragment {
    let (mut fragment, float) = layout_float(ctx, ib, cb, flow.x);
    match c.content_y {
        Some(content_y) => {
            // The float's top is not above the next border edge (Chromium's
            // `NextBorderEdge`: the margins so far count).
            let place = c.place(flow);
            let origin = place.before + place.strut.solve();
            let parent = Point::new(flow.x, content_y);
            let position = ctx.bfc().place_in(&float, origin, parent);
            fragment.border_rect.x = position.x;
            fragment.border_rect.y = position.y;
        }
        None => ctx.bfc().add_pending(PendingFloat {
            path: vec![u32::try_from(index).unwrap_or(u32::MAX)],
            float,
            origin_x: flow.x,
        }),
    }
    fragment
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
#[expect(clippy::too_many_arguments, reason = "the state of block layout")]
fn layout_block_box<'a>(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    contents: &'a BlockContainer,
    marker: Option<&'a Marker>,
    cb: ContainingBlock,
    markers: &mut Vec<PendingMarker<'a>>,
    place: ChildPlace,
    parent_style: &ComputedStyle,
) -> BlockResult {
    ctx.layout_units += 1;
    let style = &base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let (width, margin_left, _) = block_width_and_margins(style, cb, cb.width, &edges, None);
    let border_width = width + edges.sum().horizontal();
    let margin_left =
        margin_left + webkit_align_offset_in(parent_style, style, border_width, cb, cb.width);
    let specified_height = resolve_size(
        &style.height,
        cb.height,
        style.box_sizing,
        edges.sum().vertical(),
    );
    let options = collapse_options(style, &edges, cb, specified_height);
    let clearance = ctx.bfc().exclusions.clearance(style.clear);
    let margin_top = margin_or_zero(&style.margin_top, cb.width);
    let flow_y = start_block_frame(ctx, place, clearance, margin_top, edges.sum().top);
    let end = EndPlace {
        place,
        clearance,
        margin_left,
        margin_top,
        border_width,
        specified_height,
        options,
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
        ContainingBlock {
            width,
            height: specified_height,
        },
        options,
        markers,
        Flow {
            x: place.x + content_offset,
            y: flow_y,
        },
    );
    place_unplaced_markers(ctx, marker, style, markers, &mut children);
    shift_markers(markers, -content_offset);
    end_block_box(ctx, base, &end, &edges, cb, children)
}

/// Which margins of a block box's children can collapse with its own
/// (CSS 2.2 §8.3.1): the top ones if it has no top border or padding; the
/// bottom ones if it has no bottom border or padding, `auto` height
/// (`specified_height` is `None`) and zero min-height.
fn collapse_options(
    style: &ComputedStyle,
    edges: &BoxEdges,
    cb: ContainingBlock,
    specified_height: Option<f32>,
) -> ChildOptions {
    let min_height = resolve_size(
        &style.min_height,
        cb.height,
        style.box_sizing,
        edges.sum().vertical(),
    );
    ChildOptions {
        collapse_with_parent_start: edges.border.top == 0.0 && edges.padding.top == 0.0,
        collapse_with_parent_end: has_no_bottom_edge(edges)
            && specified_height.is_none()
            && min_height.unwrap_or(0.0) <= 0.0,
    }
}

/// True if a box has no bottom border or padding.
fn has_no_bottom_edge(edges: &BoxEdges) -> bool {
    edges.border.bottom == 0.0 && edges.padding.bottom == 0.0
}

/// Starts the frame of a block box (see `floats.rs`) and returns the
/// position of its content box. Its position in the BFC is known now if
/// its top margin does not collapse with its children's because it has a
/// top border or padding (`top_edge`, CSS 2.2 §8.3.1). Otherwise it waits
/// for the first content.
fn start_block_frame(
    ctx: &mut LayoutContext<'_>,
    place: ChildPlace,
    clearance: Option<f32>,
    margin_top: f32,
    top_edge: f32,
) -> FlowY {
    ctx.bfc()
        .push_frame(place.before, place.strut, clearance, place.forced);
    let own_strut = place.strut.adjoin(CollapsedMargin::new(margin_top));
    if top_edge == 0.0 {
        FlowY::Pending {
            before: place.before,
            strut: own_strut,
        }
    } else {
        FlowY::Resolved(ctx.bfc().resolve(own_strut) + top_edge)
    }
}

/// What [`end_block_box`] needs to know about the start of a block box.
struct EndPlace {
    place: ChildPlace,
    clearance: Option<f32>,
    /// The left margin: the x of the border box in the parent's content
    /// box.
    margin_left: f32,
    margin_top: f32,
    border_width: f32,
    /// The used `height` of the content box, unless `auto`.
    specified_height: Option<f32>,
    options: ChildOptions,
}

/// Ends a block box after its children: its height, its margins (collapsed
/// with the children's margins that adjoin them), its position and its
/// fragment.
fn end_block_box(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    end: &EndPlace,
    edges: &BoxEdges,
    cb: ContainingBlock,
    mut children: ChildrenLayout,
) -> BlockResult {
    let style = &base.style;
    let vertical_edges = edges.sum().vertical();
    let content_height = end.specified_height.unwrap_or(children.content_height);
    let height = clamp_height(style, content_height, cb.height, vertical_edges);
    let mut start = CollapsedMargin::new(end.margin_top);
    if end.options.collapse_with_parent_start {
        start = start.adjoin(children.start_margin);
    }
    let mut end_margin = CollapsedMargin::new(margin_or_zero(&style.margin_bottom, cb.width));
    if end.options.collapse_with_parent_end {
        end_margin = end_margin.adjoin(children.end_margin);
    }
    // The top and bottom margins of an empty box adjoin: zero (or `auto`)
    // height, zero min-height, no vertical border or padding, and no line
    // boxes or in-flow content.
    let collapsed_through = end.options.collapse_with_parent_start
        && has_no_bottom_edge(edges)
        && height == 0.0
        && children.collapsed_through;
    let position = end_block_position(ctx, end, start, collapsed_through, &mut children.fragments);
    let fragment = finish_fragment(
        base,
        Rect::new(
            end.margin_left,
            0.0,
            end.border_width,
            height + vertical_edges,
        ),
        edges,
        children.fragments,
        children.baselines.offset(edges.sum().top),
    );
    BlockResult {
        laid_out: LaidOutBlock {
            fragment,
            margins: BlockMargins {
                start,
                end: end_margin,
                collapsed_through: collapsed_through && !position.cleared,
            },
        },
        bfc_y: position.bfc_y,
        cleared: position.cleared,
        floats: position.floats,
    }
}

/// The position of a block box after its children.
struct EndPosition {
    /// The BFC y of its border box, if known.
    bfc_y: Option<f32>,
    /// True for an empty box placed by clearance.
    cleared: bool,
    /// The floats in it that still wait for a position.
    floats: Vec<PendingFloat>,
}

/// Ends the frame of a block box after its children (whose fragments are
/// `children`): resolves its position if it is still unknown and it has
/// content, or it is empty with clearance; moves the floats placed while
/// they waited. `start` is its top margin collapsed with its children's.
fn end_block_position(
    ctx: &mut LayoutContext<'_>,
    end: &EndPlace,
    start: CollapsedMargin,
    collapsed_through: bool,
    children: &mut [Fragment],
) -> EndPosition {
    // A box with content resolves its position at its end at the latest;
    // an empty box only if it has `clear` and floats on the cleared side
    // end below the top of its container (or its position is forced).
    // Chromium then places it at its clearance offset or its hypothetical
    // position, whichever is lower, and the margins before it no longer
    // collapse with the margins after it, even if it needs no clearance.
    let place = end.place;
    let mut bfc_y = ctx.bfc().resolved();
    let mut cleared = false;
    if bfc_y.is_none() {
        let through = place.strut.adjoin(start);
        let container_top = place.container_y.unwrap_or(place.before);
        if !collapsed_through {
            bfc_y = Some(ctx.bfc().resolve(through));
        } else if place.forced.is_some() || end.clearance.is_some_and(|c| c > container_top) {
            bfc_y = Some(ctx.bfc().resolve(through));
            cleared = true;
        }
    }
    let frame = ctx.bfc().pop_frame();
    move_placed_floats(children, &frame.placed);
    let mut floats = frame.pending;
    // The waiting floats that are this box's children: their parent's
    // origin becomes the border box.
    for float in &mut floats {
        if float.path.len() == 1 {
            float.origin_x = place.x + end.margin_left;
        }
    }
    EndPosition {
        bfc_y,
        cleared,
        floats,
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
        scrollable_overflow: None,
        in_positioned_inline: false,
        hanging_from: None,
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
/// with a known content-box width. The height is `content_height` if given
/// (final, not clamped); else the specified height, else the content
/// height, clamped by `min-height` and `max-height`. The returned fragment
/// is at (0, 0).
pub(crate) fn layout_sized(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    content_width: f32,
    content_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    ctx.layout_units += 1;
    let base = &ib.base;
    if let IndependentContents::Control(control) = &ib.contents {
        return crate::control::layout(ctx, base, control, content_width, content_height, cb);
    }
    let style = &base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let edge_sum = edges.sum();
    if let IndependentContents::Table(table) = &ib.contents {
        let width = content_width + edge_sum.horizontal();
        let height = content_height.map(|h| h + edge_sum.vertical());
        return crate::table::layout_with_width(ctx, ib, table, width, height, cb);
    }
    if let IndependentContents::Grid(children) = &ib.contents {
        return crate::grid::layout(ctx, ib, children, content_width, content_height, cb);
    }
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
    // A given content height is final; otherwise min-height and max-height
    // apply (flex layout uses them too).
    let limits = match content_height {
        Some(_) => HeightLimits::NONE,
        None => HeightLimits::of(style, cb.height, edge_sum.vertical()),
    };
    // The list item's own marker waits for a line box in its content.
    let mut markers: Vec<PendingMarker<'_>> = ib
        .marker
        .iter()
        .map(|marker| PendingMarker { marker, inset: 0.0 })
        .collect();
    let mut children = layout_contents(ctx, ib, child_cb, limits, &mut markers);
    place_unplaced_markers(ctx, ib.marker.as_ref(), style, &mut markers, &mut children);
    let inflow = children.inflow;
    let height = limits.clamp(specified_height.unwrap_or(children.content_height));
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
        fragment.content = match r.media {
            Some(media) => BoxContent::Media(Arc::new(crate::media::content(
                ctx,
                r.node,
                media,
                crate::geom::Size::new(content_width, height),
            ))),
            None => BoxContent::Image(r.node),
        };
    } else if crate::scroll::is_scroll_container(style) {
        fragment.scrollable_overflow = Some(crate::scroll::scrollable_overflow(&fragment, inflow));
    }
    fragment
}

/// Lays out the contents of an independent box in its content box `cb`,
/// with the formatting context that the box establishes. `limits` are the
/// box's min-height and max-height; only flex layout uses them (`cb.height`
/// is the box's height before they apply).
pub(crate) fn layout_contents<'a>(
    ctx: &mut LayoutContext<'_>,
    ib: &'a IndependentBox,
    cb: ContainingBlock,
    limits: HeightLimits,
    markers: &mut Vec<PendingMarker<'a>>,
) -> ChildrenLayout {
    let style = &ib.base.style;
    match &ib.contents {
        IndependentContents::Flow(container) => {
            layout_flow_root(ctx, container, style, cb, markers)
        }
        IndependentContents::Flex(items) => {
            // §9.2 step 2: a definite height is clamped by the container's
            // min-height and max-height.
            let cb = ContainingBlock {
                width: cb.width,
                height: cb.height.map(|h| limits.clamp(h)),
            };
            let layout = crate::flex::layout_flex(ctx, style, items, cb, limits);
            let mut inflow = crate::scroll::margin_box_extent(&layout.fragments, cb);
            inflow.height = inflow.height.max(layout.content_height);
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
                inflow,
            }
        }
        // Tables and grids size their own box (`layout_sized` calls their
        // layout; grid layout also sets the scrollable overflow of a grid
        // scroll container); controls are laid out by `control::layout`.
        IndependentContents::Replaced(_)
        | IndependentContents::Table(_)
        | IndependentContents::Grid(_)
        | IndependentContents::Control(_) => ChildrenLayout {
            fragments: Vec::new(),
            content_height: 0.0,
            start_margin: CollapsedMargin::default(),
            end_margin: CollapsedMargin::default(),
            collapsed_through: true,
            baselines: Baselines::default(),
            inflow: crate::geom::Size::default(),
        },
    }
}

/// Lays out a block-level box that establishes an independent formatting
/// context, in normal flow: its width fills the containing block unless
/// specified (tables size themselves).
pub(crate) fn layout_independent_block_level(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
) -> LaidOutBlock {
    layout_independent_in(ctx, ib, cb, cb.width, false)
}

/// [`layout_independent_block_level`] in a space `available` px wide (a
/// layout opportunity next to floats, with the box's margins); percentages
/// still resolve against `cb`. With `cache`, the layout is cached per box
/// and size (it is tried at several opportunities).
fn layout_independent_in(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
    available: f32,
    cache: bool,
) -> LaidOutBlock {
    if let IndependentContents::Table(table) = &ib.contents {
        return crate::table::layout_block_level(ctx, ib, table, cb, available);
    }
    let style = &ib.base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let (width, margin_left, content_height) =
        if let IndependentContents::Replaced(r) = &ib.contents {
            let (w, h) = crate::replaced::used_size(style, r, cb, &edges);
            let (_, ml, _) = block_width_and_margins(style, cb, available, &edges, Some(w));
            (w, ml, Some(h))
        } else if let IndependentContents::Control(_) = &ib.contents {
            // Controls do not fill their containing block.
            let w = shrink_to_fit_width(ctx, ib, cb, available, &edges);
            let (_, ml, _) = block_width_and_margins(style, cb, available, &edges, Some(w));
            (w, ml, None)
        } else {
            let (w, ml, _) = block_width_and_margins(style, cb, available, &edges, None);
            (w, ml, None)
        };
    let mut fragment = if cache {
        let key = LayoutKey::new(ib.base.id, width, content_height, cb);
        if let Some(fragment) = ctx.layouts.get(&key) {
            fragment.clone()
        } else {
            let fragment = layout_sized(ctx, ib, width, content_height, cb);
            ctx.layouts.insert(key, fragment.clone());
            fragment
        }
    } else {
        layout_sized(ctx, ib, width, content_height, cb)
    };
    fragment.border_rect.x = margin_left;
    LaidOutBlock {
        fragment,
        margins: own_margins(style, cb),
    }
}

/// The work budget units that each box and line box laid out costs when a
/// box that establishes a BFC is tried at another layout opportunity (see
/// [`crate::floats::WORK_BUDGET`]).
const RETRY_COST: u64 = 1000;

/// The most widths that one box that establishes a BFC is laid out for
/// while it looks for a layout opportunity; then it goes below all floats.
const MAX_ATTEMPTS: usize = 32;

/// Places an in-flow block-level box that establishes a BFC: its border
/// box does not overlap the floats of the BFC (CSS 2.2 §9.5). It goes into
/// the first layout opportunity at or below its position where it fits,
/// laid out for the width there, as in Chromium. If floats push it down,
/// its top margin no longer collapses with the margins before it: the
/// containers whose position waits are placed before that margin, and the
/// box at the opportunity (Chromium's `abort_if_cleared`).
fn place_independent(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    c: &mut ChildCursor,
    container: Container<'_>,
) -> BlockResult {
    let style = &ib.base.style;
    let cb = container.cb;
    let forced = c.force_clearance(ctx, container.flow, style.clear);
    let place = c.place(container.flow);
    let margin_top = CollapsedMargin::new(margin_or_zero(&style.margin_top, cb.width));
    let with_margin = place.strut.adjoin(margin_top);
    let unresolved = c.content_y.is_none();
    let floats_possible = forced.is_some()
        || !ctx.bfc().exclusions.is_empty()
        || unresolved && ctx.bfc().has_pending();
    let checkpoint = (unresolved && floats_possible).then(|| ctx.bfc().checkpoint());
    let hypothetical = match forced {
        Some(top) => top,
        None if unresolved => ctx.bfc().resolve(with_margin),
        None => place.before + with_margin.solve(),
    };
    if !floats_possible {
        let mut laid_out = layout_independent_block_level(ctx, ib, cb);
        laid_out.fragment.border_rect.x +=
            webkit_align_offset(container.style, &laid_out.fragment, cb);
        return BlockResult::placed(laid_out, hypothetical);
    }
    let origin = cleared_position(ctx, style.clear, hypothetical);
    let (mut laid_out, mut top) = fit_independent(ctx, ib, container, origin);
    // Pushed down by floats (clearance alone keeps the margin adjoining,
    // as in Chromium).
    if let Some(checkpoint) = checkpoint
        && top > origin + EPSILON
        && margin_top.solve() != 0.0
    {
        ctx.bfc().restore(checkpoint);
        let before_margin = ctx.bfc().resolve(place.strut);
        let origin = cleared_position(ctx, style.clear, before_margin);
        (laid_out, top) = fit_independent(ctx, ib, container, origin);
    }
    laid_out.fragment.border_rect.x -= container.flow.x;
    BlockResult::placed(laid_out, top)
}

/// Finds the first layout opportunity at or below `origin` where a box
/// that establishes a BFC fits beside the floats. Returns the box, laid
/// out for the width there (its fragment's x is the BFC x of its border
/// box), and its BFC y.
///
/// The opportunities are produced one at a time ([`crate::floats::Bfc`]):
/// at each position, the widest first; if the box is too tall for it, the
/// next narrower one that reaches lower; if the box is too wide, the next
/// position where the free space changes.
fn fit_independent(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    container: Container<'_>,
    origin: f32,
) -> (LaidOutBlock, f32) {
    let (cx0, cx1) = (container.flow.x, container.flow.x + container.cb.width);
    let mut attempts = 0;
    let mut y = origin;
    'positions: loop {
        let mut candidate = ctx.bfc().opportunity_at(y, cx0, cx1);
        while let Some(o) = candidate {
            let (laid_out, fits_width) = try_opportunity(ctx, ib, container, &o, &mut attempts);
            let height = laid_out.fragment.border_rect.height;
            if fits_width {
                match ctx.bfc().narrowing_below(&o, height, cx0, cx1, None) {
                    None => return (laid_out, o.top),
                    Some(at) => candidate = ctx.bfc().narrower(&o, at, cx0, cx1),
                }
            } else if o.full_width
                && ctx
                    .bfc()
                    .narrowing_below(&o, f32::INFINITY, cx0, cx1, None)
                    .is_none()
            {
                // The last opportunity: no float narrows the containing
                // block from here down, so the box overflows here.
                return (laid_out, o.top);
            } else {
                // The other opportunities at this position are narrower.
                candidate = None;
            }
            if attempts >= MAX_ATTEMPTS {
                break 'positions;
            }
        }
        match ctx.bfc().next_top(y) {
            Some(next) => y = next,
            None => break,
        }
    }
    let top = ctx.bfc().exclusions.below_all(origin, cx0, cx1).top;
    let mut laid_out = layout_independent_in(ctx, ib, container.cb, container.cb.width, true);
    laid_out.fragment.border_rect.x +=
        cx0 + webkit_align_offset(container.style, &laid_out.fragment, container.cb);
    (laid_out, top)
}

/// Lays out a box that establishes a BFC for the width of the layout
/// opportunity `o`, and positions it there horizontally. Returns the box
/// and whether its border box fits beside the floats. `attempts` counts
/// the layouts that did work (were not cached); each one after the first
/// is charged to the work budget, so that nested boxes and boxes with much
/// content do not take unbounded time and memory.
fn try_opportunity(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    container: Container<'_>,
    o: &Opportunity,
    attempts: &mut usize,
) -> (LaidOutBlock, bool) {
    let Container {
        cb,
        flow,
        style: parent_style,
    } = container;
    let (cx0, cx1) = (flow.x, flow.x + cb.width);
    let style = &ib.base.style;
    let margin_left = margin_or_zero(&style.margin_left, cb.width);
    let margin_right = margin_or_zero(&style.margin_right, cb.width);
    // Margins overlap floats; they only reduce the available width where
    // they reach past the floats (Chromium's `HandleNewFormattingContext`).
    let floats_left = o.left > cx0 + EPSILON;
    let floats_right = o.right < cx1 - EPSILON;
    let (line_left, line_right) = if !floats_left && !floats_right {
        (o.left + margin_left, o.right - margin_right)
    } else {
        (
            o.left.max(cx0 + margin_left.max(0.0)),
            o.right.min(cx1 - margin_right.max(0.0)),
        )
    };
    let available = ((line_right - line_left).max(0.0) + margin_left + margin_right).max(0.0);
    let before = ctx.layout_units;
    let mut laid_out = layout_independent_in(ctx, ib, cb, available, true);
    let work = ctx.layout_units.saturating_sub(before);
    if work > 0 {
        if *attempts > 0 {
            ctx.bfc().charge(work.saturating_mul(RETRY_COST));
        }
        *attempts += 1;
    }
    let size = laid_out.fragment.border_rect;
    let x = line_left - margin_left
        + size.x
        + webkit_align_offset_in(parent_style, style, size.width, cb, available);
    laid_out.fragment.border_rect.x = x;
    // The border box stays beside the floats on both sides.
    let fits = size.width <= o.width() + EPSILON
        && (!floats_left || x >= o.left - EPSILON)
        && (!floats_right || x + size.width <= o.right + EPSILON);
    (laid_out, fits)
}

/// Lays out the root element's box in the initial containing block, at its
/// margin position.
pub(crate) fn layout_root(
    ctx: &mut LayoutContext<'_>,
    root: &IndependentBox,
    icb: ContainingBlock,
) -> BoxFragment {
    // An absolutely positioned (or fixed) root is placed in the initial
    // containing block like any absolutely positioned box (as Chromium).
    if root.base.style.is_absolutely_positioned() {
        return crate::positioned::layout_absolute_root(ctx, root, icb);
    }
    let laid_out = layout_independent_block_level(ctx, root, icb);
    let mut fragment = laid_out.fragment;
    fragment.border_rect.y = laid_out.margins.start.solve();
    apply_relative_position(&mut fragment, icb);
    fragment
}

/// The margins of a box that establishes an independent formatting
/// context: its own top and bottom margins (`auto` is 0), which do not
/// collapse through it.
pub(crate) fn own_margins(style: &ComputedStyle, cb: ContainingBlock) -> BlockMargins {
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
    if let IndependentContents::Table(table) = &ib.contents {
        return crate::table::layout_shrink_to_fit(ctx, ib, table, cb);
    }
    let style = &ib.base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let (width, content_height) = if let IndependentContents::Replaced(r) = &ib.contents {
        let (w, h) = crate::replaced::used_size(style, r, cb, &edges);
        (w, Some(h))
    } else {
        (shrink_to_fit_width(ctx, ib, cb, cb.width, &edges), None)
    };
    let mut fragment = layout_sized(ctx, ib, width, content_height, cb);
    fragment.border_rect.x = margin_or_zero(&style.margin_left, cb.width);
    LaidOutBlock {
        fragment,
        margins: own_margins(style, cb),
    }
}

/// The shrink-to-fit content width of an independent box (CSS 2.2
/// §10.3.5, <https://www.w3.org/TR/CSS22/visudet.html#float-width>) in a
/// space `available` px wide: its specified width, or else its
/// max-content width limited to the available width but at least its
/// min-content width; then clamped by `min-width` and `max-width`.
fn shrink_to_fit_width(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: ContainingBlock,
    available: f32,
    edges: &BoxEdges,
) -> f32 {
    let style = &ib.base.style;
    let edge_sum = edges.sum().horizontal();
    let width = resolve_size(&style.width, Some(cb.width), style.box_sizing, edge_sum)
        .unwrap_or_else(|| {
            let sizes = crate::intrinsic::independent_content_sizes(ctx, ib);
            let margin_left = margin_or_zero(&style.margin_left, cb.width);
            let margin_right = margin_or_zero(&style.margin_right, cb.width);
            let available = (available - margin_left - margin_right - edge_sum).max(0.0);
            sizes.max.min(available).max(sizes.min)
        });
    clamp_width(style, width, cb.width, edge_sum)
}

/// Lays out a flex or grid item with a fixed content-box width and,
/// optionally, a fixed content-box height. The fragment is at (0, 0).
///
/// Flex and grid layout lay out an item several times (to measure it, then
/// at its final size). Results are cached per item and constraints, so that
/// nested containers do not take exponential time.
pub(crate) fn layout_flex_item(
    ctx: &mut LayoutContext<'_>,
    item: &IndependentBox,
    content_width: f32,
    content_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let key = LayoutKey::new(item.base.id, content_width, content_height, cb);
    if let Some(fragment) = ctx.layouts.get(&key) {
        return fragment.clone();
    }
    ctx.uncached_layouts += 1;
    let height = match &item.contents {
        IndependentContents::Replaced(r) if content_height.is_none() => {
            let style = &item.base.style;
            let edges = BoxEdges::resolve(style, cb.width);
            Some(crate::replaced::flex_item_height(
                style,
                r,
                content_width,
                cb,
                &edges,
            ))
        }
        _ => content_height,
    };
    let fragment = layout_sized(ctx, item, content_width, height, cb);
    ctx.layouts.insert(key, fragment.clone());
    fragment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{body, layout_html, rects_of_text};

    #[test]
    fn height_limits_let_the_minimum_win() {
        let limits = HeightLimits::new(50.0, 30.0);
        assert_eq!(limits.clamp(10.0), 50.0);
        assert_eq!(limits.clamp(100.0), 50.0);
        assert_eq!(limits.max(), 50.0);
        assert_eq!(limits.clamp(f32::NAN), 50.0);
        assert_eq!(HeightLimits::NONE.clamp(1e6), 1e6);
        assert_eq!(HeightLimits::NONE.clamp(f32::NAN), 0.0);
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
    fn lines_next_to_floats_are_shorter() {
        let l = layout_html(&body(
            "<div style='width:300px'><div style='float:left; width:100px; height:30px'></div>\
             <div style='float:right; width:50px; height:50px'></div>\
             aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp qqq rrr</div>",
        ));
        let texts = l.texts();
        for (rect, _) in &texts {
            if rect.y < 30.0 {
                assert!(rect.x >= 100.0, "{rect:?}");
            }
            if rect.y < 50.0 {
                assert!(rect.right() <= 250.01, "{rect:?}");
            }
        }
        // Below the left float, lines start at the left edge.
        assert!(texts.iter().any(|(r, _)| r.y >= 30.0 && r.x == 0.0));
    }

    #[test]
    fn waiting_floats_move_with_their_chain_and_relative_ancestors() {
        // A float in 100 nested empty blocks (every tenth relatively
        // positioned) waits until the paragraph's margin, which collapses
        // through the outer block, decides the outer block's position.
        let depth = 100;
        let mut html = String::from("<div id=o>");
        for i in 0..depth {
            html.push_str(if i % 10 == 0 {
                "<div style='position:relative; left:1px'>"
            } else {
                "<div>"
            });
        }
        html.push_str("<div id=f style='float:left; width:10px; height:10px'></div>");
        html.push_str(&"</div>".repeat(depth));
        html.push_str("<p id=p style='margin:30px 0 0'>text</p></div>");
        let l = layout_html(&body(&html));
        assert_eq!(l.rect("o").y, 30.0);
        assert_eq!(l.rect("f"), Rect::new(10.0, 30.0, 10.0, 10.0));
        // The line avoids the float's position in the BFC, which relative
        // positioning does not change.
        assert_eq!(rects_of_text(&l.texts(), "text")[0].x, 10.0);
    }

    #[test]
    fn an_empty_block_places_its_floats_at_its_own_position() {
        let l = layout_html(&body(
            "<div style='border-top:1px solid'><div style='margin-bottom:50px'>\
             <div id=f style='float:left; width:10px; height:10px'></div></div>\
             <p id=p style='margin-top:100px'>x</p></div>",
        ));
        assert_eq!(l.rect("f").y, 1.0);
        assert_eq!(l.rect("p").y, 101.0);
    }

    #[test]
    fn block_formatting_context_roots_avoid_floats() {
        let l = layout_html(&body(
            "<div style='width:400px'><div style='float:left; width:100px; height:50px'></div>\
             <div id=a style='overflow:hidden'>a</div>\
             <div id=b style='display:flow-root; width:350px'>b</div></div>",
        ));
        assert_eq!(l.rect("a"), Rect::new(100.0, 0.0, 300.0, 20.0));
        assert_eq!(l.rect("b").y, 50.0);
    }

    #[test]
    fn clearance_moves_a_block_below_the_floats() {
        let l = layout_html(&body(
            "<div style='float:left; width:10px; height:40px'></div>\
             <div style='float:right; width:10px; height:60px'></div>\
             <div id=l style='clear:left'>l</div><div id=b style='clear:both'>b</div>",
        ));
        assert_eq!(l.rect("l").y, 40.0);
        assert_eq!(l.rect("b").y, 60.0);
    }

    #[test]
    fn the_height_of_a_formatting_context_root_includes_its_floats() {
        let l = layout_html(&body(
            "<div id=a style='overflow:hidden'><div style='float:left; height:70px; \
             margin-bottom:5px'></div></div><div id=b><div style='float:left; height:70px'></div></div>",
        ));
        assert_eq!(l.rect("a").height, 75.0);
        assert_eq!(l.rect("b").height, 0.0);
    }

    #[test]
    fn many_floats_are_placed_in_bounded_time() {
        // More floats than one BFC places with the rules (the container's
        // border places them at once; they do not wait); the rest go below
        // all floats. A staircase of tiny floats, and paragraphs with a word
        // too wide for the gap beside them that negative margins pull back
        // up: each line is tried at many positions before it goes below
        // the floats. Those attempts spend the work budget; then lines go
        // below the floats at once.
        use std::fmt::Write as _;
        let mut floats = String::new();
        for k in 0..crate::floats::MAX_FLOATS + 500 {
            write!(
                floats,
                "<i style='float:left; clear:left; width:{}px; height:0.01px'></i>",
                150.0 + f64::from(u16::try_from(k % 1000).unwrap_or(0)) * 0.001
            )
            .expect("writing to a String does not fail");
        }
        // Paragraphs of a fixed height, each pulled back to the top of the
        // floats by its negative margin.
        let first = "<p style='margin:0; height:20px; line-height:20px'>wordwordwordword</p>";
        let paragraphs =
            "<p style='margin:-20px 0 0; height:20px; line-height:20px'>wordwordwordword</p>"
                .repeat(4000);
        let l = layout_html(&body(&format!(
            "<div style='width:200px; border-top:1px solid'>{floats}{first}{paragraphs}</div>"
        )));
        let mut finite = true;
        l.tree.walk(|f, origin| {
            if let crate::FragmentRef::Box(b) = f {
                finite &= (origin.y + b.border_rect.y).is_finite();
            }
        });
        assert!(finite);
        assert!(l.float_budget_spent);
    }

    #[test]
    fn lines_next_to_many_narrowing_floats_take_bounded_time() {
        // A staircase of thin floats, each wider than the one above, and
        // tall lines that negative margins pull back over it: every float
        // narrows the space, so a line could be tried in thousands of
        // layout opportunities. The attempts per line are limited.
        use std::fmt::Write as _;
        let mut floats = String::new();
        for k in 0..5000 {
            write!(
                floats,
                "<i style='float:left; clear:left; width:{}px; height:0.02px'></i>",
                100.0 + f64::from(k) * 0.01
            )
            .expect("writing to a String does not fail");
        }
        let words = "word ".repeat(30);
        let first = format!("<p style='margin:0; line-height:100px'>{words}</p>");
        let paragraphs =
            format!("<p style='margin:-100px 0 0; line-height:100px'>{words}</p>").repeat(100);
        let l = layout_html(&body(&format!(
            "<div style='width:3000px; padding-top:100px'>{floats}{first}{paragraphs}</div>"
        )));
        assert!(l.root().border_rect.height.is_finite());
        assert!(!l.float_budget_spent);
    }

    #[test]
    fn a_column_of_many_floats_does_not_spend_the_budget() {
        // An ordinary page: a long column of floated links and short
        // paragraphs next to it. Each line visits only the floats beside
        // it.
        use std::fmt::Write as _;
        let mut links = String::new();
        for k in 0..8000 {
            write!(
                links,
                "<a style='float:left; clear:left; margin-right:10px'>link {k}</a>"
            )
            .expect("writing to a String does not fail");
        }
        let paragraphs = "<p style='margin:0'>word word word word</p>".repeat(8000);
        let l = layout_html(&body(&format!(
            "<div style='width:600px'>{links}<div id=p>{paragraphs}</div></div>"
        )));
        assert!(!l.float_budget_spent);
        assert!(l.rect("p").y < 1.0);
    }

    #[test]
    fn floats_that_wait_in_deep_chains_are_bounded() {
        // Floats in the innermost of many empty blocks wait for the
        // position of each block in turn; a BFC keeps at most `MAX_FLOATS`
        // waiting floats.
        let depth = 200;
        let float = "<i style='float:left; width:1px; height:1px'></i>";
        let cap = crate::floats::MAX_FLOATS;
        let l = layout_html(&body(&format!(
            "{}<div id=inner>{}<i id=placed style='float:left; width:1px; height:1px'></i>\
             {}<i id=dropped style='float:left; width:1px; height:1px'></i></div>{}\
             <p id=p style='margin-top:20px'>x</p>",
            "<div>".repeat(depth),
            float.repeat(cap - 1),
            float.repeat(100),
            "</div>".repeat(depth)
        )));
        assert!(!l.float_budget_spent);
        assert_eq!(l.rect("p").y, 20.0);
        // The last float that may wait is placed (on a lower row); a later
        // one stays at the top left of its container.
        let inner = l.rect("inner");
        let (placed, dropped) = (l.rect("placed"), l.rect("dropped"));
        assert!(placed.y > inner.y);
        assert_eq!((dropped.x, dropped.y), (inner.x, inner.y));
    }

    #[test]
    fn nested_formatting_context_roots_next_to_floats_take_bounded_time() {
        // Every level is tried at two layout opportunities; without the
        // budget for repeated layouts this is exponential in the depth.
        let level = "<div style='overflow:hidden; min-width:50%'>\
                     <i style='float:left; width:30%; height:5px'></i>\
                     <i style='float:left; width:30%; height:5px'></i>\
                     <i style='float:right; width:30%; height:9px'></i>";
        let html = format!("{}x{}", level.repeat(100), "</div>".repeat(100));
        let l = layout_html(&body(&html));
        assert!(l.root().border_rect.height.is_finite());
        assert!(l.float_budget_spent);
    }

    #[test]
    fn retries_of_formatting_context_roots_with_much_text_are_bounded() {
        // Gaps of distinct widths between floats; a BFC root with much text
        // does not fit in any of them and negative margins pull copies of it
        // back to the top. Each retry lays out all its lines again: the work
        // budget counts them.
        use std::fmt::Write as _;
        let mut floats = String::new();
        for k in 0..400 {
            write!(
                floats,
                "<i style='float:left; clear:left; width:{}px; height:2px'></i>\
                 <b style='float:right; width:50px; height:1px'></b>",
                150.0 + f64::from(k) * 0.1
            )
            .expect("writing to a String does not fail");
        }
        let text = "word ".repeat(300);
        let roots =
            format!("<div style='overflow:hidden; height:30px; margin-top:-830px'>{text}</div>")
                .repeat(20);
        let l = layout_html(&body(&format!(
            "<div style='width:200px'>{floats}{roots}</div>"
        )));
        assert!(l.root().border_rect.height.is_finite());
        assert!(l.float_budget_spent);
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
    fn inline_boxes_around_blocks_get_a_box() {
        let l = layout_html(&body(
            "<div style='width:300px'><a id=a><div id=b style='width:50px;height:10px;\
             margin:3px 0 6px'></div></a></div>",
        ));
        let (a, b) = (l.rect("a"), l.rect("b"));
        assert_eq!((a.x, a.width), (0.0, 300.0));
        assert_eq!((a.y, a.bottom()), (b.y, b.bottom() + 6.0));
    }

    #[test]
    fn blocks_in_deep_inline_boxes_get_a_bounded_number_of_boxes() {
        let open = "<span>".repeat(50);
        let blocks = "<div>x</div>".repeat(20);
        let l = layout_html(&body(&format!("{open}{blocks}")));
        let mut wrappers = 0;
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Box(b) = f
                && b.content == BoxContent::GeometryOnly
            {
                wrappers += 1;
            }
        });
        assert!(wrappers <= 8 * 20, "{wrappers} boxes");
    }

    #[test]
    fn inline_boxes_continue_after_a_float() {
        // The float does not end the open inline box: the text after it is
        // still inside the span.
        let l = layout_html(&body(
            "<span id=s style='position:relative'><div style='float:left;width:10px;\
             height:10px'></div>text</span>",
        ));
        let span = l.node("s");
        assert_ne!(l.tree.border_boxes(span).len(), 0);
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
