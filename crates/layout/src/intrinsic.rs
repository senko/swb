//! Intrinsic (content-based) widths: min-content and max-content.
//!
//! <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>. Used for
//! shrink-to-fit widths (floats, inline-blocks, absolutely positioned
//! boxes), for flex base sizes and for grid track sizes.

use swb_style::{Clear, ComputedStyle, FlexBasis, FlexWrap, Size};

use crate::LayoutContext;
use crate::block::{BoxEdges, content_size, resolve_max_size, resolve_size};
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
    // Size containment: hidden contents have no intrinsic size.
    if ib.base.style.contents_hidden {
        return ContentSizes::default();
    }
    // A block, flex or grid container with an aspect ratio and a definite
    // height is as wide as the ratio makes it (CSS Sizing 4 §5.1; Chromium).
    if crate::aspect::applies_to(ib)
        && let Some(w) = crate::aspect::width_of_definite_height(&ib.base.style)
    {
        return ContentSizes { min: w, max: w };
    }
    match &ib.contents {
        IndependentContents::Flow(container) => {
            let sizes = container_content_sizes(ctx, container, &ib.base.style);
            with_inside_marker(ctx, sizes, container, ib.marker.as_ref())
        }
        IndependentContents::Flex(items) => flex_content_sizes(ctx, &ib.base.style, items),
        IndependentContents::Grid(items) => crate::grid::content_sizes(ctx, &ib.base, items),
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
            crate::control::content_sizes(ctx, &ib.base, control)
        }
    }
}

/// Content-box sizes of a flex container with style `style` and `items`.
///
/// Chromium's rules (measured, `tools/probes/flex-intrinsic.json`), which
/// follow <https://drafts.csswg.org/css-flexbox-1/#intrinsic-main-sizes>
/// only in part:
///
/// - A single-line row container: the min-content size is the sum of the
///   items' min-content contributions, the max-content size the sum of
///   their max-content contributions, plus the gaps (percentage gaps count
///   as 0). The spec's flex-fraction algorithm is not used.
/// - A multi-line row container: the min-content size is the largest
///   min-content contribution (no gaps); the max-content size is the sum
///   as above, but at least the min-content size.
/// - A column container: the largest contribution of each kind.
pub(crate) fn flex_content_sizes(
    ctx: &mut LayoutContext<'_>,
    style: &ComputedStyle,
    items: &[IndependentBox],
) -> ContentSizes {
    let row = style.flex_direction.is_row();
    let single_line = style.flex_wrap == FlexWrap::Nowrap;
    let mut sizes = ContentSizes::default();
    let in_flow = items
        .iter()
        .filter(|item| !item.base.style.is_absolutely_positioned());
    let mut count = 0_usize;
    for item in in_flow {
        count += 1;
        let c =
            crate::table::TableCache::percent_free(ctx, |ctx| flex_item_contribution(ctx, item));
        if !row {
            sizes = sizes.max_with(c.plain);
        } else if single_line {
            sizes.min += c.clamped.min;
            sizes.max += c.clamped.max;
        } else {
            sizes.min = sizes.min.max(c.plain.min);
            sizes.max += c.clamped.max;
        }
    }
    if row && count > 1 {
        // The gaps between the items of one line; percentages resolve
        // against 0 for intrinsic sizes.
        let gaps = crate::flex::gap(&style.column_gap, 0.0) * (count - 1) as f32;
        sizes.max += gaps;
        if single_line {
            sizes.min += gaps;
        }
    }
    if row && !single_line {
        // The limited maxima can add up to less than the largest unlimited
        // minimum; the max-content size is never below the min-content size
        // (Chromium 148 in a `max-content` grid track). The users of these
        // sizes in swb do not go below the min-content size either, so no
        // test shows this line.
        sizes.max = sizes.max.max(sizes.min);
    }
    sizes
}

/// The contribution of a flex item to the intrinsic sizes of its row
/// container (margin box).
struct FlexContribution {
    /// The item's min-content and max-content contributions.
    plain: ContentSizes,
    /// `plain` adjusted by a definite `flex-basis`.
    clamped: ContentSizes,
}

impl FlexContribution {
    /// A contribution that no `flex-basis` limits.
    fn unclamped(plain: ContentSizes) -> FlexContribution {
        FlexContribution {
            plain,
            clamped: plain,
        }
    }
}

/// The contribution of `item` to its row container. If the item has a
/// definite `flex-basis`, the contribution is limited by it
/// ([`limit_by_basis`]), and the minimum width undoes a reduction below
/// the plain contribution (CSS Flexbox 1 §9.9.3 "clamped by its flex base
/// size",
/// <https://drafts.csswg.org/css-flexbox-1/#intrinsic-item-contributions>).
/// With `flex-basis: auto`, `content` or a percentage, no limit applies
/// (measured in Chromium: `flex-shrink: 0` text items contribute their
/// min-content size, not their max-content size).
fn flex_item_contribution(ctx: &mut LayoutContext<'_>, item: &IndependentBox) -> FlexContribution {
    let style = &item.base.style;
    let mut content = None;
    let plain = match &item.contents {
        IndependentContents::Table(_) | IndependentContents::Replaced(_) => {
            independent_outer_sizes(ctx, item)
        }
        _ => outer_sizes(style, || {
            let c = independent_content_sizes(ctx, item);
            content = Some(c);
            c
        }),
    };
    let edge_sum = BoxEdges::resolve(style, 0.0).sum().horizontal();
    // A `flex-basis` that does not depend on the container (not `auto`,
    // `content`, a keyword or a percentage).
    let FlexBasis::Size(basis) = &style.flex_basis else {
        return FlexContribution::unclamped(plain);
    };
    let Some(basis) = resolve_size(basis, None, style.box_sizing, edge_sum) else {
        return FlexContribution::unclamped(plain);
    };
    let outer = edge_sum + fixed_margins(style);
    let max_width = resolve_max_size(&style.max_width, None, style.box_sizing, edge_sum)
        .unwrap_or(f32::INFINITY);
    let limit = |c: f32| limit_by_basis(style, c, basis + outer, max_width + outer);
    let mut clamped = ContentSizes {
        min: limit(plain.min),
        max: limit(plain.max),
    };
    // The minimum width undoes a reduction below the plain contribution
    // (it wins over `max-width`, as in Chromium 148). It can need the
    // content sizes, so compute it only then.
    if clamped.min < plain.min || clamped.max < plain.max {
        let mut floor = flex_min_width(ctx, item, content, edge_sum, max_width) + outer;
        if matches!(item.contents, IndependentContents::Table(_)) {
            // A table is never narrower than its min-content width, whatever
            // its basis, `width`, `min-width` and `max-width` (measured in
            // Chromium 148).
            floor = floor.max(plain.min);
        }
        let raise = |v: f32, c: f32| if v < c { v.max(floor) } else { v };
        clamped.min = raise(clamped.min, plain.min);
        clamped.max = raise(clamped.max, plain.max);
    }
    FlexContribution { plain, clamped }
}

/// The basis limits of §9.9.3 for a contribution `c` (margin box) of an
/// item with the definite basis `basis_outer` and the `max-width`
/// `max_outer` (both margin box): at most the basis if the item cannot
/// grow, at least the basis if it cannot shrink, then `max-width`. The
/// caller applies the minimum width.
fn limit_by_basis(style: &ComputedStyle, c: f32, basis_outer: f32, max_outer: f32) -> f32 {
    let mut v = c;
    if style.flex_grow <= 0.0 {
        v = v.min(basis_outer);
    }
    if style.flex_shrink <= 0.0 {
        v = v.max(basis_outer);
    }
    v.min(max_outer)
}

/// The used minimum width (content box) of a row flex item, for the lower
/// limit of its contribution: `min-width`, or for `auto` the automatic
/// minimum size (CSS Flexbox 1 §4.5) with the item's min-content size
/// (`content` if known), a definite `width` and `max_width` (content box).
fn flex_min_width(
    ctx: &mut LayoutContext<'_>,
    item: &IndependentBox,
    content: Option<ContentSizes>,
    edge_sum: f32,
    max_width: f32,
) -> f32 {
    let style = &item.base.style;
    match &style.min_width {
        Size::Auto => {
            let width = resolve_size(&style.width, None, style.box_sizing, edge_sum);
            let min_content = || {
                content
                    .unwrap_or_else(|| independent_content_sizes(ctx, item))
                    .min
            };
            crate::flex::automatic_min_main(style, true, min_content, max_width, width)
        }
        other => resolve_size(other, None, style.box_sizing, edge_sum).unwrap_or(0.0),
    }
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
                resolve_size(&style.min_width, None, style.box_sizing, edge_sum).unwrap_or(0.0);
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
        BlockLevelBox::Block {
            base,
            contents,
            marker,
        } => outer_sizes(&base.style, || {
            if let Some(w) = crate::aspect::width_of_definite_height(&base.style) {
                return ContentSizes { min: w, max: w };
            }
            let sizes = container_content_sizes(ctx, contents, &base.style);
            with_inside_marker(ctx, sizes, contents, marker.as_ref())
        }),
        BlockLevelBox::Independent(ib) | BlockLevelBox::Float(ib) => {
            independent_outer_sizes(ctx, ib)
        }
        BlockLevelBox::AbsolutelyPositioned(_) => ContentSizes::default(),
        BlockLevelBox::InInline(b) => block_level_outer_sizes(ctx, &b.block),
    }
}

/// `sizes`, the content sizes of the list item contents `container`, with
/// the width of an inside marker. It is in the first line: with inline
/// content it adds to the max-content width; with block content, the first
/// block's line is as wide as the marker and its text, which the maximum
/// of the widths approximates. The min-content width is at least the
/// marker's. An outside marker takes no room in the content box.
fn with_inside_marker(
    ctx: &mut LayoutContext<'_>,
    sizes: ContentSizes,
    container: &BlockContainer,
    marker: Option<&crate::box_tree::Marker>,
) -> ContentSizes {
    let Some(marker) = marker.filter(|m| !m.outside) else {
        return sizes;
    };
    let width = crate::list_marker::shape_marker(ctx, marker).width;
    let max = match container {
        BlockContainer::Inline(_) => sizes.max + width,
        BlockContainer::Blocks(_) => sizes.max.max(width),
    };
    ContentSizes {
        min: sizes.min.max(width),
        max,
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
    let content_box = |w: f32| content_size(w, style.box_sizing, edge_sum);
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

#[cfg(test)]
mod tests {
    // The widths in these tests were measured with Chromium 148
    // (`tools/probes/flex-intrinsic.json`).

    /// The width of the flex container `#f` as a float in a box of
    /// `avail` px, that is its min-content size for 0 and its max-content
    /// size for a large `avail`.
    fn width(container_style: &str, items: &str, avail: u32) -> f32 {
        let html = format!(
            "<style>.a {{ width: 50px; height: 10px }}</style>\
             <div style='width: {avail}px'><div style='float: left'>\
             <div id=f style='display: flex; {container_style}'>{items}</div></div></div>"
        );
        let l = crate::test_support::layout_html(&crate::test_support::body(&html));
        l.rect("f").width
    }

    fn items(n: usize, style: &str) -> String {
        format!("<div class=a style='{style}'></div>").repeat(n)
    }

    #[test]
    fn a_single_line_row_adds_the_contributions_and_gaps() {
        assert_eq!(width("", &items(3, ""), 0), 150.0);
        assert_eq!(width("", &items(3, "margin: 0 5px"), 0), 180.0);
        assert_eq!(width("gap: 10px", &items(3, ""), 0), 170.0);
        assert_eq!(width("gap: 10%", &items(3, ""), 0), 150.0);
        assert_eq!(
            width("flex-direction: row-reverse", &items(3, ""), 0),
            150.0
        );
    }

    #[test]
    fn a_multi_line_row_and_a_column_take_the_largest_contribution() {
        assert_eq!(width("flex-wrap: wrap", &items(3, ""), 0), 50.0);
        assert_eq!(width("flex-wrap: wrap; gap: 10px", &items(3, ""), 0), 50.0);
        assert_eq!(
            width("flex-wrap: wrap; gap: 10px", &items(3, ""), 3000),
            170.0
        );
        assert_eq!(width("flex-direction: column", &items(3, ""), 0), 50.0);
        assert_eq!(width("flex-direction: column", &items(3, ""), 3000), 50.0);
    }

    #[test]
    fn a_definite_flex_basis_limits_the_contributions() {
        // Cannot grow: at most the basis.
        assert_eq!(width("", &items(3, "flex: 0 1 20px"), 0), 60.0);
        assert_eq!(width("", &items(3, "flex: 0 1 20px"), 3000), 60.0);
        // Cannot shrink: at least the basis.
        assert_eq!(width("", &items(3, "flex: 0 0 100px"), 0), 300.0);
        assert_eq!(width("", &items(3, "flex: 0 0 100px"), 3000), 300.0);
        // Can grow and shrink: no limit.
        assert_eq!(width("", &items(3, "flex: 1 1 20px"), 0), 150.0);
        // A wrapping container: the largest minimum, the sum of the limited maxima.
        assert_eq!(
            width("flex-wrap: wrap", &items(3, "flex: 0 0 100px"), 0),
            50.0
        );
        assert_eq!(
            width("flex-wrap: wrap", &items(3, "flex: 0 0 100px"), 3000),
            300.0
        );
        assert_eq!(
            width("flex-wrap: wrap", &items(3, "flex-basis: 0"), 3000),
            50.0
        );
        // `min-width: auto` keeps a text item at its min-content size.
        let text = "<span style='flex-basis: 0'>aaaa bbbb</span>".repeat(3);
        assert_eq!(width("", &text, 3000), width("", &text, 0));
        // An automatic basis or a percentage does not limit.
        assert_eq!(width("", &items(3, "flex: 0 0 auto"), 0), 150.0);
        assert_eq!(width("", &items(3, "flex: 0 0 50%"), 0), 150.0);
    }

    #[test]
    fn max_width_limits_a_basis_and_min_width_wins_over_it() {
        // A basis that the item cannot shrink below, then `max-width`.
        assert_eq!(
            width("", &items(3, "flex: 0 0 200px; max-width: 80px"), 0),
            240.0
        );
        // `min-width` above `max-width`: the used width is `min-width`,
        // whatever the basis does.
        for flex in ["0 1 25px", "0 0 25px", "1 1 25px"] {
            let style = format!("flex: {flex}; min-width: 20px; max-width: 10px");
            assert_eq!(width("", &items(3, &style), 0), 60.0, "{flex}");
            assert_eq!(width("", &items(3, &style), 3000), 60.0, "{flex}");
        }
        let style = "flex: 0 1 25px; min-width: 20px; max-width: 10px";
        assert_eq!(width("flex-wrap: wrap", &items(3, style), 0), 20.0);
        assert_eq!(width("flex-wrap: wrap", &items(3, style), 3000), 60.0);
    }

    #[test]
    fn a_multi_line_row_is_at_least_its_min_content_size() {
        // A `max-content` grid track takes the max-content size as it is (a
        // float would not go below the min-content size anyway).
        let in_track = |container_style: &str, items: &str| {
            let html = format!(
                "<style>.a {{ width: 50px; height: 10px }}</style>\
                 <div style='display: grid; grid-template-columns: max-content; width: 3000px'>\
                 <div id=f style='display: flex; {container_style}'>{items}</div></div>"
            );
            let l = crate::test_support::layout_html(&crate::test_support::body(&html));
            l.rect("f").width
        };
        // The limited maxima (3 x 10 px, gaps 2 x 5 px) are below the
        // largest unlimited item (50 px).
        let narrow = items(3, "flex: 0 1 10px; overflow: hidden");
        assert_eq!(in_track("flex-wrap: wrap; column-gap: 5px", &narrow), 50.0);
        assert_eq!(in_track("column-gap: 5px", &narrow), 40.0);
    }

    #[test]
    fn a_table_item_is_not_narrower_than_its_min_content_width() {
        let table = |style: &str| {
            format!("<table style='border-spacing: 0; {style}'><td style='padding: 0'>aaaaaaaa</td></table>")
                .repeat(2)
        };
        let plain = width("", &table(""), 0);
        assert!(plain > 100.0, "{plain}");
        for style in [
            "flex: 0 1 10px; max-width: 20px",
            "flex: 0 0 40px; max-width: 20px",
            "flex: 0 1 10px; min-width: 0",
            "flex: 0 1 10px; width: 30px",
        ] {
            assert_eq!(width("", &table(style), 0), plain, "{style}");
            assert_eq!(width("", &table(style), 3000), plain, "{style}");
        }
        assert_eq!(width("", &table("flex: 0 0 200px"), 0), 400.0);
    }

    #[test]
    fn the_minimum_width_undoes_a_reduction() {
        assert_eq!(
            width("", &items(3, "flex: 0 1 20px; min-width: 30px"), 0),
            90.0
        );
        let wide = "<div style='flex: 0 1 20px'>aaaaaaaaaaaa</div>".repeat(2);
        let clamped = "<div style='flex: 0 1 20px; overflow: hidden'>aaaaaaaaaaaa</div>".repeat(2);
        assert!(width("", &wide, 0) > 40.0);
        assert_eq!(width("", &clamped, 0), 40.0);
    }
}
