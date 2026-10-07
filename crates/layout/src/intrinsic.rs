//! Intrinsic (content-based) widths: min-content and max-content.
//!
//! <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>. Used for
//! shrink-to-fit widths (floats, inline-blocks, absolutely positioned
//! boxes), for flex base sizes and for grid track sizes.

use swb_style::{BoxSizing, Clear, ComputedStyle, Size};

use crate::LayoutContext;
use crate::block::BoxEdges;
use crate::box_tree::{BlockContainer, BlockLevelBox, IndependentBox, IndependentContents};
use crate::inline;

/// Min-content and max-content widths.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContentSizes {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl ContentSizes {
    fn max_with(self, other: ContentSizes) -> ContentSizes {
        ContentSizes {
            min: self.min.max(other.min),
            max: self.max.max(other.max),
        }
    }

    fn add(self, v: f32) -> ContentSizes {
        ContentSizes {
            min: self.min + v,
            max: self.max + v,
        }
    }
}

/// Content-box sizes of an independent box's contents.
pub(crate) fn independent_content_sizes(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
) -> ContentSizes {
    match &ib.contents {
        IndependentContents::Flow(container) => {
            container_content_sizes(ctx, container, &ib.base.style)
        }
        IndependentContents::Flex(items) => flex_content_sizes(ctx, &ib.base.style, items),
        IndependentContents::Grid(items) => crate::grid::content_sizes(ctx, ib, items),
        IndependentContents::Replaced(r) => {
            let edges = BoxEdges::resolve(&ib.base.style, 0.0);
            let w = crate::replaced::natural_content_width(&ib.base.style, r, &edges);
            ContentSizes { min: w, max: w }
        }
        IndependentContents::Table(table) => {
            // The table's contribution without margins, border and padding.
            let style = &ib.base.style;
            let outer = crate::table::table_outer_sizes(ctx, ib, table);
            let edges = BoxEdges::resolve(style, 0.0).sum().horizontal() + fixed_margins(style);
            ContentSizes {
                min: (outer.min - edges).max(0.0),
                max: (outer.max - edges).max(0.0),
            }
        }
        IndependentContents::Control(control) => {
            crate::control::content_sizes(ctx, &ib.base.style, control)
        }
    }
}

/// Content-box sizes of a flex container with style `style` and `items`.
pub(crate) fn flex_content_sizes(
    ctx: &mut LayoutContext<'_>,
    style: &ComputedStyle,
    items: &[IndependentBox],
) -> ContentSizes {
    let row = style.flex_direction.is_row();
    let mut sizes = ContentSizes::default();
    let in_flow = items
        .iter()
        .filter(|item| !item.base.style.is_absolutely_positioned());
    let mut count = 0_usize;
    for item in in_flow {
        count += 1;
        let s =
            crate::table::TableCache::percent_free(ctx, |ctx| independent_outer_sizes(ctx, item));
        if row {
            sizes.min = sizes.min.max(s.min);
            sizes.max += s.max;
        } else {
            sizes = sizes.max_with(s);
        }
    }
    if row && count > 1 {
        // The gaps between the items of one line; percentages resolve
        // against 0 for intrinsic sizes.
        sizes.max += crate::flex::gap(&style.column_gap, 0.0) * (count - 1) as f32;
    }
    sizes
}

/// Margin-box sizes of an independent box, honoring a fixed `width`.
pub(crate) fn independent_outer_sizes(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
) -> ContentSizes {
    let style = &ib.base.style;
    if let IndependentContents::Table(table) = &ib.contents {
        return crate::table::table_outer_sizes(ctx, ib, table);
    }
    if let IndependentContents::Replaced(r) = &ib.contents {
        // Percentages count as `auto`; with a percentage `width` or
        // `max-width`, the min-content contribution is the `min-width`
        // (Chromium: compressible replaced elements, CSS Sizing 3 §5.2.2).
        let edges = BoxEdges::resolve(style, 0.0);
        let extra = edges.sum().horizontal() + fixed_margins(style);
        let max = crate::replaced::intrinsic_width(style, r, &edges) + extra;
        let percentage = |lp: Option<&swb_style::LengthPercentage>| {
            lp.is_some_and(swb_style::LengthPercentage::has_percentage)
        };
        let min = if percentage(style.width.as_length_percentage())
            || percentage(style.max_width.as_length_percentage())
        {
            let edge_sum = edges.sum().horizontal();
            let min_width =
                crate::block::resolve_size(&style.min_width, None, style.box_sizing, edge_sum)
                    .unwrap_or(0.0);
            min_width + extra
        } else {
            max
        };
        return ContentSizes { min, max };
    }
    outer_sizes(style, || independent_content_sizes(ctx, ib))
}

fn block_level_outer_sizes(ctx: &mut LayoutContext<'_>, b: &BlockLevelBox) -> ContentSizes {
    match b {
        BlockLevelBox::Block { base, contents, .. } => outer_sizes(&base.style, || {
            container_content_sizes(ctx, contents, &base.style)
        }),
        BlockLevelBox::Independent(ib) | BlockLevelBox::Float(ib) => {
            independent_outer_sizes(ctx, ib)
        }
        BlockLevelBox::AbsolutelyPositioned(_) => ContentSizes::default(),
        BlockLevelBox::InInline(b) => block_level_outer_sizes(ctx, &b.block),
    }
}

pub(crate) fn container_content_sizes(
    ctx: &mut LayoutContext<'_>,
    container: &BlockContainer,
    style: &ComputedStyle,
) -> ContentSizes {
    match container {
        BlockContainer::Inline(ifc) => inline::content_sizes(ctx, ifc, style),
        BlockContainer::Blocks(children) => blocks_content_sizes(ctx, children),
    }
}

/// The content sizes of block-level children, with floats as in Chromium's
/// `BlockLayoutAlgorithm::ComputeMinMaxSizes`: floats and a box that
/// establishes a BFC after them share a "line", so their max-content
/// widths add up; a float or BFC root with `clear` starts a new line on the
/// cleared sides, and any other in-flow box ends the line.
///
/// Parts of this function are derived from Chromium's
/// `third_party/blink/renderer/core/layout/block_layout_algorithm.cc`
/// (`ComputeMinMaxSizes`; Copyright The Chromium Authors, BSD-3-Clause);
/// see `THIRD_PARTY_NOTICES.md`.
fn blocks_content_sizes(ctx: &mut LayoutContext<'_>, children: &[BlockLevelBox]) -> ContentSizes {
    let mut sizes = ContentSizes::default();
    let mut left = 0.0_f32;
    let mut right = 0.0_f32;
    for child in children {
        let child = match child {
            BlockLevelBox::InInline(b) => &b.block,
            other => other,
        };
        if matches!(child, BlockLevelBox::AbsolutelyPositioned(_)) {
            continue;
        }
        let floating = matches!(child, BlockLevelBox::Float(_));
        let new_fc = matches!(child, BlockLevelBox::Independent(_));
        let child_sizes = block_level_outer_sizes(ctx, child);
        sizes.min = sizes.min.max(child_sizes.min);
        if floating || new_fc {
            let clear = child_style(child).map_or(Clear::None, |s| s.clear);
            if clear != Clear::None {
                sizes.max = sizes.max.max(left + right);
            }
            if matches!(clear, Clear::Left | Clear::Both) {
                left = 0.0;
            }
            if matches!(clear, Clear::Right | Clear::Both) {
                right = 0.0;
            }
        }
        if floating {
            if child_style(child).is_some_and(|s| s.float == swb_style::Float::Right) {
                right += child_sizes.max;
            } else {
                left += child_sizes.max;
            }
            sizes.max = sizes.max.max(left + right);
        } else {
            let line = if new_fc { left + right } else { 0.0 };
            sizes.max = sizes.max.max(line + child_sizes.max);
            left = 0.0;
            right = 0.0;
        }
    }
    sizes
}

/// The style of a block-level box.
fn child_style(b: &BlockLevelBox) -> Option<&ComputedStyle> {
    match b {
        BlockLevelBox::Block { base, .. } => Some(&base.style),
        BlockLevelBox::Independent(ib)
        | BlockLevelBox::Float(ib)
        | BlockLevelBox::AbsolutelyPositioned(ib) => Some(&ib.base.style),
        BlockLevelBox::InInline(b) => child_style(&b.block),
    }
}

/// Adds padding, border and fixed margins to content sizes; a fixed
/// `width` replaces the content sizes.
fn outer_sizes(style: &ComputedStyle, content: impl FnOnce() -> ContentSizes) -> ContentSizes {
    let edges = BoxEdges::resolve(style, 0.0);
    let edge_sum = edges.sum().horizontal();
    // A fixed size property as a content-box width.
    let content_box = |w: f32| match style.box_sizing {
        BoxSizing::ContentBox => w,
        BoxSizing::BorderBox => (w - edge_sum).max(0.0),
    };
    let inner = match &style.width {
        Size::LengthPercentage(lp) if !lp.has_percentage() => {
            let w = content_box(lp.resolve(0.0));
            ContentSizes { min: w, max: w }
        }
        _ => content(),
    };
    let min_width = match &style.min_width {
        Size::LengthPercentage(lp) if !lp.has_percentage() => content_box(lp.resolve(0.0)),
        _ => 0.0,
    };
    let max_width = match &style.max_width {
        swb_style::MaxSize::LengthPercentage(lp) if !lp.has_percentage() => {
            content_box(lp.resolve(0.0))
        }
        _ => f32::INFINITY,
    };
    let clamp = |v: f32| v.min(max_width).max(min_width);
    ContentSizes {
        min: clamp(inner.min),
        max: clamp(inner.max),
    }
    .add(edge_sum + fixed_margins(style))
}

/// The sum of the left and right margins that do not depend on the
/// containing block (percentages and `auto` count as 0), for intrinsic
/// sizes.
pub(crate) fn fixed_margins(style: &ComputedStyle) -> f32 {
    let m = |lp: &swb_style::LengthPercentageOrAuto| match lp.non_auto() {
        Some(lp) if !lp.has_percentage() => lp.resolve(0.0),
        _ => 0.0,
    };
    m(&style.margin_left) + m(&style.margin_right)
}
