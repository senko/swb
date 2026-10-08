//! Flex layout (CSS Flexible Box Layout Level 1, §9):
//! <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
//!
//! Supported: row and column directions (and reverse), wrapping, `order`,
//! flex-grow/shrink/basis, automatic minimum sizes, the container's own
//! min-height and max-height, `justify-content`, `align-items`/`align-self`
//! (start, end, center, stretch), `align-content` (start, end, center,
//! space-*, stretch), `gap`, auto margins on the main axis, and the
//! container's first baseline (§8.5).
//!
//! Absolutely positioned children get a placeholder (see
//! `positioned.rs`). Not supported yet: auto margins on the cross axis,
//! baseline alignment (items with `align-self: baseline` are aligned at
//! the cross start), `flex-wrap: wrap-reverse` (laid out as `wrap`).
//! Known bug: `justify-content: normal` packs `row-reverse` and
//! `column-reverse` items at the main end instead of the main start.

use swb_style::{Alignment, ComputedStyle, FlexBasis, FlexWrap, Gap, LengthPercentageOrAuto};

use crate::LayoutContext;
use crate::align::resolve_self_alignment;
use crate::block::{
    BoxEdges, ContainingBlock, SizeLimits, apply_relative_position, clamp_height, clamp_width,
    layout_flex_item, resolve_max_size, resolve_size,
};
use crate::box_tree::IndependentBox;
use crate::fragment::{BoxFragment, Fragment};
use crate::intrinsic;
use crate::positioned::{StaticParent, add_placeholders};

/// The result of laying out a flex container's contents.
pub(crate) struct FlexLayout {
    /// Item fragments relative to the container's content box.
    pub(crate) fragments: Vec<Fragment>,
    /// The container's content height, with its min-height and max-height
    /// applied.
    pub(crate) content_height: f32,
    /// The first baseline, from the top of the content box (§8.5).
    pub(crate) first_baseline: Option<f32>,
}

/// The main and cross axes of a flex container, and the space in them.
struct Axes {
    row: bool,
    reverse: bool,
    /// The container's inner main size, if definite.
    main_available: Option<f32>,
    /// The container's inner cross size, if definite.
    cross_available: Option<f32>,
    main_gap: f32,
    cross_gap: f32,
}

impl Axes {
    fn new(container: &ComputedStyle, cb: ContainingBlock) -> Self {
        let row = container.flex_direction.is_row();
        let main_available = if row { Some(cb.width) } else { cb.height };
        let cross_available = if row { cb.height } else { Some(cb.width) };
        let (main_gap, cross_gap) = if row {
            (&container.column_gap, &container.row_gap)
        } else {
            (&container.row_gap, &container.column_gap)
        };
        Axes {
            row,
            reverse: container.flex_direction.is_reverse(),
            main_available,
            cross_available,
            main_gap: gap(main_gap, main_available.unwrap_or(0.0)),
            cross_gap: gap(cross_gap, cross_available.unwrap_or(0.0)),
        }
    }
}

/// The used size of a gap; percentages resolve against `basis`.
pub(crate) fn gap(g: &Gap, basis: f32) -> f32 {
    match g {
        Gap::Normal => 0.0,
        Gap::LengthPercentage(lp) => lp.resolve(basis),
    }
}

struct Item<'a> {
    box_: &'a IndependentBox,
    style: &'a ComputedStyle,
    /// Main-axis padding + border.
    main_edges: f32,
    cross_edges: f32,
    margin_main_start: Option<f32>,
    margin_main_end: Option<f32>,
    margin_cross_start: Option<f32>,
    margin_cross_end: Option<f32>,
    base_size: f32,
    min_main: f32,
    max_main: f32,
    hypothetical: f32,
    target: f32,
    frozen: bool,
    cross_size: f32,
    fragment: Option<BoxFragment>,
}

impl Item<'_> {
    fn main_margins(&self) -> f32 {
        self.margin_main_start.unwrap_or(0.0) + self.margin_main_end.unwrap_or(0.0)
    }

    fn cross_margins(&self) -> f32 {
        self.margin_cross_start.unwrap_or(0.0) + self.margin_cross_end.unwrap_or(0.0)
    }

    fn outer_hypothetical(&self) -> f32 {
        self.hypothetical + self.main_edges + self.main_margins()
    }

    fn outer_target(&self) -> f32 {
        self.target + self.main_edges + self.main_margins()
    }

    fn outer_cross(&self) -> f32 {
        self.cross_size + self.cross_edges + self.cross_margins()
    }

    fn align_self(&self, container: &ComputedStyle) -> Alignment {
        resolve_self_alignment(self.style.align_self, container.align_items)
    }

    /// True if the item is stretched in the cross axis: `align-self:
    /// stretch`, an `auto` cross size and no `auto` cross margin (§9.4
    /// step 11).
    fn stretches(&self, container: &ComputedStyle, row: bool) -> bool {
        let cross_size = if row {
            &self.style.height
        } else {
            &self.style.width
        };
        matches!(
            self.align_self(container),
            Alignment::Stretch | Alignment::Normal
        ) && cross_size.is_auto()
            && self.margin_cross_start.is_some()
            && self.margin_cross_end.is_some()
    }
}

/// Lays out flex items inside a container whose content box is `cb`.
/// `cb.height` is the container's definite height, if any, already clamped
/// by its min-height and max-height (`limits`). Tables inside do not use
/// column percentages for their intrinsic widths (see
/// [`crate::table::TableCache::percent_free`]).
pub(crate) fn layout_flex(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    children: &[IndependentBox],
    cb: ContainingBlock,
    limits: SizeLimits,
) -> FlexLayout {
    crate::table::TableCache::percent_free(ctx, |ctx| {
        layout_flex_items(ctx, container, children, cb, limits)
    })
}

/// [`layout_flex`] without the table setting.
fn layout_flex_items(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    children: &[IndependentBox],
    cb: ContainingBlock,
    limits: SizeLimits,
) -> FlexLayout {
    let axes = Axes::new(container, cb);
    let mut items = collect_items(ctx, container, children, &axes, cb);
    // §9.3 step 5: a column container with an indefinite height breaks
    // lines at its max-height (at least its min-height).
    let line_limit = axes.main_available.unwrap_or(limits.max());
    let lines = collect_lines(container, &items, &axes, line_limit);

    // §9.7: resolve flexible lengths, per line.
    let container_main = axes.main_available.unwrap_or_else(|| {
        // §9.2 step 4, indefinite main size (a column container without a
        // height): the largest sum of the hypothetical sizes of a line,
        // clamped by the container's min-height and max-height.
        limits.clamp(
            lines
                .iter()
                .map(|l| line_main_size(&items[l.clone()], axes.main_gap, Item::outer_hypothetical))
                .fold(0.0, f32::max),
        )
    });
    for line in &lines {
        resolve_flexible_lengths(&mut items[line.clone()], container_main, axes.main_gap);
    }

    let cross = determine_cross_sizes(ctx, container, &mut items, &lines, &axes, cb, limits);
    let mut layout = place_items(
        container,
        &mut items,
        &lines,
        &cross,
        container_main,
        &axes,
        cb,
    );
    add_placeholders(ctx, children, &mut layout.fragments, StaticParent::Flex);
    layout
}

/// §9.2: the flex base size and hypothetical main size of every in-flow
/// child, in `order`.
fn collect_items<'a>(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    children: &'a [IndependentBox],
    axes: &Axes,
    cb: ContainingBlock,
) -> Vec<Item<'a>> {
    let mut ordered: Vec<&IndependentBox> = children
        .iter()
        .filter(|c| !c.base.style.is_absolutely_positioned())
        .collect();
    ordered.sort_by_key(|c| c.base.style.order);
    ordered
        .into_iter()
        .map(|b| new_item(ctx, container, b, axes, cb))
        .collect()
}

fn new_item<'a>(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    b: &'a IndependentBox,
    axes: &Axes,
    cb: ContainingBlock,
) -> Item<'a> {
    let style = b.base.style.as_ref();
    let edges = BoxEdges::resolve(style, cb.width);
    let (main_edges, cross_edges) = if axes.row {
        (edges.sum().horizontal(), edges.sum().vertical())
    } else {
        (edges.sum().vertical(), edges.sum().horizontal())
    };
    let margin = |m: &LengthPercentageOrAuto| m.resolve(cb.width);
    let (main_start, main_end, cross_start, cross_end) = if axes.row {
        (
            &style.margin_left,
            &style.margin_right,
            &style.margin_top,
            &style.margin_bottom,
        )
    } else {
        (
            &style.margin_top,
            &style.margin_bottom,
            &style.margin_left,
            &style.margin_right,
        )
    };
    let mut item = Item {
        box_: b,
        style,
        main_edges,
        cross_edges,
        margin_main_start: margin(main_start),
        margin_main_end: margin(main_end),
        margin_cross_start: margin(cross_start),
        margin_cross_end: margin(cross_end),
        base_size: 0.0,
        min_main: 0.0,
        max_main: f32::INFINITY,
        hypothetical: 0.0,
        target: 0.0,
        frozen: false,
        cross_size: 0.0,
        fragment: None,
    };

    let (main_size, min_prop, max_prop) = if axes.row {
        (&style.width, &style.min_width, &style.max_width)
    } else {
        (&style.height, &style.min_height, &style.max_height)
    };
    let definite_main = resolve_size(main_size, axes.main_available, style.box_sizing, main_edges);
    let basis = match &style.flex_basis {
        FlexBasis::Size(swb_style::Size::Auto) => definite_main,
        FlexBasis::Size(s) => resolve_size(s, axes.main_available, style.box_sizing, main_edges),
        FlexBasis::Content => None,
    };
    let (min_content, max_content) = if axes.row {
        let sizes = intrinsic::independent_content_sizes(ctx, b);
        let max = crate::replaced::flex_width(b, cb, &edges).unwrap_or(sizes.max);
        (
            sizes
                .min
                .max(stretched_ratio_width(&item, container, axes, cb)),
            max,
        )
    } else {
        // The content height at the item's cross size.
        let width = column_cross_size(ctx, &item, container, axes, cb);
        let cross = column_cross_is_definite(&item, container, axes, cb).then_some(width);
        let h = crate::replaced::column_flex_height(b, cross, cb, &edges).unwrap_or_else(|| {
            let f = layout_flex_item(ctx, b, width, None, cb);
            (f.border_rect.height - main_edges).max(0.0)
        });
        (h, h)
    };
    item.base_size = basis.unwrap_or(max_content);
    item.max_main = resolve_max_size(max_prop, axes.main_available, style.box_sizing, main_edges)
        .unwrap_or(f32::INFINITY);
    item.min_main = match min_prop {
        swb_style::Size::Auto => {
            // Automatic minimum size (§4.5): the content size suggestion,
            // capped by the specified size suggestion. Scroll containers
            // (but not `overflow: clip` boxes) have no automatic minimum;
            // neither have items with size containment (measured in
            // Chromium 148, also for `contain-intrinsic-size`).
            let overflow = if axes.row {
                style.overflow_x
            } else {
                style.overflow_y
            };
            if overflow.is_scroll_container() || style.contain.size {
                0.0
            } else {
                let content = min_content.min(item.max_main);
                definite_main.map_or(content, |d| content.min(d))
            }
        }
        other => {
            resolve_size(other, axes.main_available, style.box_sizing, main_edges).unwrap_or(0.0)
        }
    };
    item.hypothetical = item.base_size.min(item.max_main).max(item.min_main);
    item.target = item.hypothetical;
    item
}

/// The transferred size suggestion of a row item with an `aspect-ratio`
/// that stretches in a container with a definite height (§4.5): the
/// stretched height through the ratio. It is the item's automatic minimum
/// width, as in Chromium, even if the content is narrower. 0 for other
/// items.
fn stretched_ratio_width(
    item: &Item<'_>,
    container: &ComputedStyle,
    axes: &Axes,
    cb: ContainingBlock,
) -> f32 {
    let Some(cross) = axes.cross_available else {
        return 0.0;
    };
    if !crate::aspect::applies_to(item.box_) || !item.stretches(container, true) {
        return 0.0;
    }
    let height = clamp_height(
        item.style,
        (cross - item.cross_edges - item.cross_margins()).max(0.0),
        cb.height,
        item.cross_edges,
    );
    let edges = BoxEdges::resolve(item.style, cb.width);
    crate::aspect::transferred_width(item.style, height, Some(cb.width), &edges).unwrap_or(0.0)
}

/// The content-box width of an item of a column container: its `width` if
/// definite, the available width if it stretches, else fit-content; then
/// clamped by `min-width` and `max-width`.
fn column_cross_size(
    ctx: &mut LayoutContext<'_>,
    item: &Item<'_>,
    container: &ComputedStyle,
    axes: &Axes,
    cb: ContainingBlock,
) -> f32 {
    let style = item.style;
    let available = axes
        .cross_available
        .map(|c| (c - item.cross_edges - item.cross_margins()).max(0.0));
    let width = resolve_size(
        &style.width,
        Some(cb.width),
        style.box_sizing,
        item.cross_edges,
    )
    .unwrap_or_else(|| match available {
        Some(available) if item.stretches(container, false) => available,
        available => {
            let sizes = intrinsic::independent_content_sizes(ctx, item.box_);
            available.map_or(sizes.max, |a| sizes.max.min(a.max(sizes.min)))
        }
    });
    clamp_width(style, width, cb.width, item.cross_edges)
}

/// True if the width of an item of a column container is definite or
/// stretched, not fit-content.
fn column_cross_is_definite(
    item: &Item<'_>,
    container: &ComputedStyle,
    axes: &Axes,
    cb: ContainingBlock,
) -> bool {
    let style = item.style;
    resolve_size(
        &style.width,
        Some(cb.width),
        style.box_sizing,
        item.cross_edges,
    )
    .is_some()
        || (axes.cross_available.is_some() && item.stretches(container, false))
}

/// §9.3: collects the items into flex lines of at most `line_limit` (the
/// outer hypothetical main sizes and the gaps; a line has at least one
/// item).
fn collect_lines(
    container: &ComputedStyle,
    items: &[Item<'_>],
    axes: &Axes,
    line_limit: f32,
) -> Vec<std::ops::Range<usize>> {
    if is_single_line(container) {
        return std::iter::once(0..items.len()).collect();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, item) in items.iter().enumerate() {
        let size = item.outer_hypothetical();
        let with_gap = if i > start {
            used + axes.main_gap + size
        } else {
            size
        };
        if i > start && with_gap > line_limit {
            lines.push(start..i);
            start = i;
            used = size;
        } else {
            used = with_gap;
        }
    }
    lines.push(start..items.len());
    lines
}

/// The outer main size of a line: `size` of each item plus the gaps.
fn line_main_size<'a>(items: &[Item<'a>], gap: f32, size: impl Fn(&Item<'a>) -> f32) -> f32 {
    items.iter().map(size).sum::<f32>() + gap * gap_count(items.len())
}

/// True if the container is single-line (`flex-wrap: nowrap`). A
/// multi-line container that has only one line is not single-line.
fn is_single_line(container: &ComputedStyle) -> bool {
    container.flex_wrap == FlexWrap::Nowrap
}

/// The cross sizes of a container's lines and of the container.
struct CrossSizes {
    lines: Vec<f32>,
    /// The container's inner cross size.
    container: f32,
    /// The container's cross size minus the lines and the gaps between
    /// them (negative if they overflow).
    free: f32,
}

/// §9.4: lays out every item at its main size, determines the cross size
/// of every line and of the container, grows the lines for
/// `align-content: stretch`, and stretches items.
fn determine_cross_sizes(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    lines: &[std::ops::Range<usize>],
    axes: &Axes,
    cb: ContainingBlock,
    limits: SizeLimits,
) -> CrossSizes {
    measure_cross_sizes(ctx, container, items, axes, cb);
    let cross = line_cross_sizes(container, items, lines, axes, limits);
    stretch_items(ctx, container, items, lines, &cross.lines, axes, cb);
    cross
}

/// §9.4 step 7: the hypothetical cross size of every item, from a layout at
/// its main size.
fn measure_cross_sizes(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    axes: &Axes,
    cb: ContainingBlock,
) {
    for item in items.iter_mut() {
        let (w, h) = if axes.row {
            (item.target, None)
        } else {
            // An image with a fit-content width takes its width from its
            // flexed height.
            let edges = BoxEdges::resolve(item.style, cb.width);
            let ratio_width = (!column_cross_is_definite(item, container, axes, cb))
                .then(|| crate::replaced::column_flex_width(item.box_, item.target, cb, &edges))
                .flatten();
            let cross =
                ratio_width.unwrap_or_else(|| column_cross_size(ctx, item, container, axes, cb));
            (cross, Some(item.target))
        };
        let fragment = layout_flex_item(ctx, item.box_, w, h, cb);
        item.cross_size = if axes.row {
            fragment.border_rect.height - item.cross_edges
        } else {
            fragment.border_rect.width - item.cross_edges
        };
        item.fragment = Some(fragment);
    }
}

/// §9.4 steps 8, 15 and 9: the cross size of every line and of the
/// container, then `align-content: stretch`.
fn line_cross_sizes(
    container: &ComputedStyle,
    items: &[Item<'_>],
    lines: &[std::ops::Range<usize>],
    axes: &Axes,
    limits: SizeLimits,
) -> CrossSizes {
    // Step 8: the largest outer cross size of the line's items. A
    // single-line container uses its definite cross size, or else clamps
    // the line by its min and max cross size. (Only a row container's cross
    // size, its height, can be indefinite.)
    let mut line_cross: Vec<f32> = lines
        .iter()
        .map(|l| {
            items[l.clone()]
                .iter()
                .map(Item::outer_cross)
                .fold(0.0, f32::max)
        })
        .collect();
    let single_line = is_single_line(container);
    if single_line && let Some(first) = line_cross.first_mut() {
        *first = axes.cross_available.unwrap_or_else(|| limits.clamp(*first));
    }

    // Step 15, needed by step 9: the container's inner cross size.
    let gaps = axes.cross_gap * gap_count(lines.len());
    let total = line_cross.iter().sum::<f32>() + gaps;
    let container_cross = axes.cross_available.unwrap_or_else(|| limits.clamp(total));

    // Step 9: `align-content: stretch` (and `normal`) grows the lines of a
    // multi-line container equally to fill the container, before the items
    // stretch. The specification says this only for a definite cross size;
    // Chromium also does it for an indefinite one that min-height makes
    // larger than the lines.
    if !single_line
        && matches!(
            container.align_content,
            Alignment::Stretch | Alignment::Normal
        )
        && container_cross > total
    {
        let extra = (container_cross - total) / line_cross.len().max(1) as f32;
        for c in &mut line_cross {
            *c += extra;
        }
    }
    let free = container_cross - (line_cross.iter().sum::<f32>() + gaps);
    CrossSizes {
        lines: line_cross,
        container: container_cross,
        free,
    }
}

/// §9.4 step 11: items with an auto cross size and `align-self: stretch`
/// take the cross size of their line (`line_cross`).
fn stretch_items(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    lines: &[std::ops::Range<usize>],
    line_cross: &[f32],
    axes: &Axes,
    cb: ContainingBlock,
) {
    for (li, line) in lines.iter().enumerate() {
        for item in &mut items[line.clone()] {
            if !item.stretches(container, axes.row) {
                continue;
            }
            let target_cross = (line_cross[li] - item.cross_edges - item.cross_margins()).max(0.0);
            let target_cross = if axes.row {
                clamp_height(item.style, target_cross, cb.height, item.cross_edges)
            } else {
                clamp_width(item.style, target_cross, cb.width, item.cross_edges)
            };
            if (target_cross - item.cross_size).abs() > 0.01 {
                let (w, h) = if axes.row {
                    (item.target, Some(target_cross))
                } else {
                    (target_cross, Some(item.target))
                };
                item.fragment = Some(layout_flex_item(ctx, item.box_, w, h, cb));
                item.cross_size = target_cross;
            }
        }
    }
}

/// The number of gaps between `count` items or lines.
fn gap_count(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

/// §9.5 and §9.6: `align-content`, then main-axis and cross-axis
/// alignment of the items in each line.
fn place_items(
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    lines: &[std::ops::Range<usize>],
    cross: &CrossSizes,
    container_main: f32,
    axes: &Axes,
    cb: ContainingBlock,
) -> FlexLayout {
    let line_cross = &cross.lines;
    // `align-content` applies only to multi-line containers. Stretched lines
    // already fill the container (see `line_cross_sizes`).
    let (mut cross_pos, cross_between) = if is_single_line(container) {
        (0.0, 0.0)
    } else {
        distribute(container.align_content, cross.free, lines.len())
    };

    let mut fragments = Vec::with_capacity(items.len());
    let mut baseline = BaselineChoice::default();
    for (li, line) in lines.iter().enumerate() {
        let line_items = &mut items[line.clone()];
        let mut free =
            container_main - line_main_size(line_items, axes.main_gap, Item::outer_target);
        let auto_margins = line_items
            .iter()
            .map(|i| {
                usize::from(i.margin_main_start.is_none())
                    + usize::from(i.margin_main_end.is_none())
            })
            .sum::<usize>();
        let auto_margin = if auto_margins > 0 && free > 0.0 {
            let m = free / auto_margins as f32;
            free = 0.0;
            m
        } else {
            0.0
        };
        let justify = if axes.reverse {
            flip(container.justify_content)
        } else {
            container.justify_content
        };
        let (mut main_pos, between) = distribute(justify, free, line_items.len());
        let order: Vec<usize> = if axes.reverse {
            (0..line_items.len()).rev().collect()
        } else {
            (0..line_items.len()).collect()
        };
        for (n, &idx) in order.iter().enumerate() {
            let item = &mut line_items[idx];
            if n > 0 {
                main_pos += axes.main_gap + between;
            }
            main_pos += item.margin_main_start.unwrap_or(auto_margin);
            let free_in_line = line_cross[li] - item.outer_cross();
            let cross_offset = match item.align_self(container) {
                Alignment::End | Alignment::FlexEnd | Alignment::SelfEnd => free_in_line,
                Alignment::Center => free_in_line / 2.0,
                _ => 0.0,
            };
            let cross_margin = item.margin_cross_start.unwrap_or(0.0);
            if let Some(mut fragment) = item.fragment.take() {
                let (x, y) = if axes.row {
                    (main_pos, cross_pos + cross_offset + cross_margin)
                } else {
                    (cross_pos + cross_offset + cross_margin, main_pos)
                };
                fragment.border_rect.x = x;
                fragment.border_rect.y = y;
                if li == 0 {
                    baseline.add(item, &fragment, container, axes.row);
                }
                apply_relative_position(&mut fragment, cb);
                fragments.push(Fragment::Box(fragment));
            }
            main_pos += item.target + item.main_edges + item.margin_main_end.unwrap_or(auto_margin);
        }
        cross_pos += line_cross[li] + axes.cross_gap + cross_between;
    }

    FlexLayout {
        fragments,
        content_height: if axes.row {
            cross.container
        } else {
            container_main
        },
        first_baseline: baseline.get(),
    }
}

/// Chooses the first baseline of a flex container (§8.5,
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>) from the items of
/// its first line, in visual order (as Chromium: in `row-reverse` and
/// `column-reverse` the last item in order-modified document order comes
/// first).
#[derive(Default)]
struct BaselineChoice {
    /// The baseline of the first item.
    first_item: Option<f32>,
    /// The baseline of the first item that participates in baseline
    /// alignment.
    aligned: Option<f32>,
}

impl BaselineChoice {
    /// Adds an item of the first line, placed at its final position before
    /// relative positioning (relative offsets do not move the baseline).
    /// An item without a baseline gets one from its bottom border edge.
    fn add(
        &mut self,
        item: &Item<'_>,
        fragment: &BoxFragment,
        container: &ComputedStyle,
        row: bool,
    ) {
        let baseline = fragment.border_rect.y
            + fragment
                .first_baseline
                .unwrap_or(fragment.border_rect.height);
        self.first_item.get_or_insert(baseline);
        // Only items of a row container with no `auto` cross margin
        // participate in baseline alignment.
        let participates = row
            && item.align_self(container) == Alignment::Baseline
            && item.margin_cross_start.is_some()
            && item.margin_cross_end.is_some();
        if participates {
            self.aligned.get_or_insert(baseline);
        }
    }

    /// The container's first baseline: a baseline-aligned item's, else the
    /// first item's; `None` without items.
    fn get(&self) -> Option<f32> {
        self.aligned.or(self.first_item)
    }
}

fn flip(a: Alignment) -> Alignment {
    match a {
        Alignment::FlexStart => Alignment::FlexEnd,
        Alignment::FlexEnd => Alignment::FlexStart,
        other => other,
    }
}

/// Returns (offset before the first item, extra space between items).
/// The space distributions fall back to `center` when there is no free
/// space (or for `space-between`, to start).
fn distribute(align: Alignment, free: f32, count: usize) -> (f32, f32) {
    let n = count as f32;
    match align {
        Alignment::End | Alignment::FlexEnd | Alignment::Right => (free, 0.0),
        Alignment::SpaceBetween if count > 1 && free > 0.0 => (0.0, free / (n - 1.0)),
        Alignment::SpaceAround if count > 0 && free > 0.0 => (free / n / 2.0, free / n),
        Alignment::SpaceEvenly if count > 0 && free > 0.0 => (free / (n + 1.0), free / (n + 1.0)),
        Alignment::Center | Alignment::SpaceAround | Alignment::SpaceEvenly => (free / 2.0, 0.0),
        _ => (0.0, 0.0),
    }
}

/// §9.7 "Resolving Flexible Lengths" for one line. Flex factors are summed
/// and divided in `f64`, so that huge factors do not overflow to infinity.
fn resolve_flexible_lengths(items: &mut [Item<'_>], container_main: f32, gap: f32) {
    let gaps = gap * gap_count(items.len());
    let sum_hypothetical: f32 = items.iter().map(Item::outer_hypothetical).sum::<f32>() + gaps;
    let grow = sum_hypothetical < container_main;
    let factor = |i: &Item<'_>| {
        f64::from(if grow {
            i.style.flex_grow
        } else {
            i.style.flex_shrink
        })
    };
    for item in items.iter_mut() {
        item.target = item.base_size;
        if factor(item) == 0.0
            || (grow && item.base_size > item.hypothetical)
            || (!grow && item.base_size < item.hypothetical)
        {
            item.target = item.hypothetical;
            item.frozen = true;
        } else {
            item.frozen = false;
        }
    }
    let initial_free = |items: &[Item<'_>]| {
        container_main
            - gaps
            - items
                .iter()
                .map(|i| {
                    let size = if i.frozen { i.target } else { i.base_size };
                    size + i.main_edges + i.main_margins()
                })
                .sum::<f32>()
    };
    let start_free = f64::from(initial_free(items));
    for _ in 0..=items.len() {
        if items.iter().all(|i| i.frozen) {
            break;
        }
        let mut free = f64::from(initial_free(items));
        let factor_sum: f64 = items.iter().filter(|i| !i.frozen).map(factor).sum();
        if factor_sum < 1.0 {
            let scaled = start_free * factor_sum;
            if scaled.abs() < free.abs() {
                free = scaled;
            }
        }
        if grow {
            for item in items.iter_mut().filter(|i| !i.frozen) {
                if factor_sum > 0.0 {
                    let share = free * factor(item) / factor_sum;
                    item.target = item.base_size + share as f32;
                }
            }
        } else {
            let scaled_sum: f64 = items
                .iter()
                .filter(|i| !i.frozen)
                .map(|i| factor(i) * f64::from(i.base_size))
                .sum();
            for item in items.iter_mut().filter(|i| !i.frozen) {
                if scaled_sum > 0.0 {
                    let ratio = factor(item) * f64::from(item.base_size) / scaled_sum;
                    item.target = item.base_size + (free * ratio) as f32;
                }
            }
        }
        // Fix min/max violations.
        let mut total_violation = 0.0;
        for item in items.iter_mut().filter(|i| !i.frozen) {
            let clamped = item.target.min(item.max_main).max(item.min_main).max(0.0);
            total_violation += clamped - item.target;
            item.target = clamped;
        }
        for item in items.iter_mut().filter(|i| !i.frozen) {
            let at_min = item.target <= item.min_main && total_violation > 0.0;
            let at_max = item.target >= item.max_main && total_violation < 0.0;
            if total_violation == 0.0 || at_min || at_max {
                item.frozen = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::FragmentRef;
    use crate::test_support::{TestLayout, body, layout_html};

    /// The first baseline of the element `id`, from its border box top.
    fn first_baseline(l: &TestLayout, id: &str) -> Option<f32> {
        let node = l.node(id);
        let mut baseline = None;
        l.tree.walk(|f, _| {
            if let FragmentRef::Box(b) = f
                && b.node == Some(node)
            {
                baseline = b.first_baseline;
            }
        });
        baseline
    }

    #[test]
    fn single_line_is_clamped_by_the_container_min_and_max_height() {
        let l = layout_html(&body(
            "<div id=min style='display:flex; min-height:60px; align-items:center'>\
             <div id=a style='width:10px; height:20px'></div></div>\
             <div id=max style='display:flex; max-height:30px; align-items:flex-end'>\
             <div id=b style='width:10px; height:50px'></div></div>\
             <div id=s style='display:flex; min-height:40px'><div id=c>x</div></div>",
        ));
        assert_eq!(l.rect("min").height, 60.0);
        assert_eq!(l.rect("a").y, 20.0);
        let max = l.rect("max");
        assert_eq!(max.height, 30.0);
        assert_eq!(l.rect("b").y, max.y - 20.0);
        assert_eq!(l.rect("c").height, 40.0);
    }

    #[test]
    fn definite_height_is_clamped_before_flexing() {
        let l = layout_html(&body(
            "<div style='display:flex; flex-direction:column; height:300px; max-height:100px'>\
             <div id=g style='flex:1'></div><div style='height:20px'></div></div>\
             <div id=r style='display:flex; height:10px; min-height:40px'><div id=s>x</div></div>",
        ));
        assert_eq!(l.rect("g").height, 80.0);
        assert_eq!(l.rect("r").height, 40.0);
        assert_eq!(l.rect("s").height, 40.0);
    }

    #[test]
    fn column_main_size_is_clamped_by_the_container_min_height() {
        let l = layout_html(&body(
            "<div style='display:flex; flex-direction:column; min-height:100px'>\
             <div id=g style='flex:1'></div><div style='height:20px'></div></div>",
        ));
        assert_eq!(l.rect("g").height, 80.0);
    }

    #[test]
    fn column_lines_break_at_the_container_max_height() {
        let l = layout_html(&body(
            "<div id=c style='display:flex; flex-direction:column; flex-wrap:wrap; \
             max-height:60px; width:200px; align-content:flex-start'>\
             <div style='width:40px; height:25px'></div><div style='width:40px; height:25px'></div>\
             <div id=third style='width:40px; height:25px'></div></div>",
        ));
        // The container is as high as its longest line.
        assert_eq!(l.rect("c").height, 50.0);
        let third = l.rect("third");
        assert_eq!((third.x, third.y), (40.0, 0.0));

        // A min-height above the max-height wins, also for line breaking.
        let l = layout_html(&body(
            "<div id=c style='display:flex; flex-direction:column; flex-wrap:wrap; \
             min-height:100px; max-height:50px; width:200px; align-content:flex-start'>\
             <div style='width:40px; height:25px'></div><div style='width:40px; height:25px'></div>\
             <div id=third style='width:40px; height:25px'></div></div>",
        ));
        assert_eq!(l.rect("c").height, 100.0);
        let third = l.rect("third");
        assert_eq!((third.x, third.y), (0.0, 50.0));
    }

    #[test]
    fn lines_grow_for_align_content_before_items_stretch() {
        let l = layout_html(&body(
            "<div style='display:flex; flex-wrap:wrap; width:100px; min-height:100px'>\
             <div id=a style='width:100px'>a</div><div id=b style='width:100px'>b</div></div>\
             <div style='display:flex; flex-wrap:wrap; height:60px'><div id=c>c</div></div>",
        ));
        assert_eq!(l.rect("a").height, 50.0);
        assert_eq!((l.rect("b").y, l.rect("b").height), (50.0, 50.0));
        // A multi-line container with one line stretches it too.
        assert_eq!(l.rect("c").height, 60.0);
    }

    #[test]
    fn baseline_comes_from_the_first_item_of_the_first_line() {
        let l = layout_html(&body(
            "<div id=empty style='display:flex'><div style='width:10px; height:30px'></div>\
             <div style='margin-top:4px'>text</div></div>\
             <div id=reverse style='display:flex; flex-direction:row-reverse'>\
             <div style='width:10px; height:30px'></div><div style='margin-top:4px'>text</div></div>\
             <div id=column style='display:flex; flex-direction:column'>\
             <div style='height:12px; margin-top:3px; border-bottom:2px solid'></div><div>text</div></div>\
             <div id=aligned style='display:flex'><div style='width:10px; height:30px'></div>\
             <div style='margin-top:4px; align-self:baseline'>text</div></div>\
             <div id=relative style='display:flex'>\
             <div style='width:10px; height:30px; position:relative; top:5px'></div></div>\
             <div id=none style='display:flex'></div>",
        ));
        // An item without a baseline gets one at its bottom border edge.
        assert_eq!(first_baseline(&l, "empty"), Some(30.0));
        let text = first_baseline(&l, "reverse").expect("the text item has a baseline");
        assert!(text > 4.0 && text < 24.0, "{text}");
        assert_eq!(first_baseline(&l, "column"), Some(17.0));
        assert_eq!(first_baseline(&l, "aligned"), Some(text));
        assert_eq!(first_baseline(&l, "relative"), Some(30.0));
        assert_eq!(first_baseline(&l, "none"), None);
    }

    #[test]
    fn column_items_use_their_width() {
        let l = layout_html(
            "<!DOCTYPE html><body style='margin:0'>\
             <div style='display:flex; flex-direction:column; width:600px'>\
             <div id=s style='width:200px; height:20px'></div></div>\
             <div style='display:flex; flex-direction:column; align-items:center; width:600px'>\
             <div id=c style='width:200px; height:20px'></div></div>",
        );
        assert_eq!(l.rect("s").width, 200.0);
        let c = l.rect("c");
        assert_eq!((c.x, c.width), (200.0, 200.0));
    }

    #[test]
    fn nested_flex_containers_lay_out_items_a_bounded_number_of_times() {
        const DEPTH: usize = 30;
        let column = "<div style='display:flex; flex-direction:column'>".repeat(DEPTH);
        let l = layout_html(&format!("<!DOCTYPE html><body>{column}x"));
        assert!(
            l.uncached_layouts <= 4 * DEPTH,
            "{} layouts",
            l.uncached_layouts
        );

        // Row containers whose first item is shorter than its sibling: the
        // item is stretched (laid out again) at every level.
        let mut html = String::from("x");
        for i in 0..DEPTH {
            html = format!(
                "<div style='display:flex'><div>{html}</div><div style='height:{}px'>b</div></div>",
                100 + 10 * i
            );
        }
        let l = layout_html(&format!("<!DOCTYPE html><body>{html}"));
        assert!(
            l.uncached_layouts <= 8 * DEPTH,
            "{} layouts",
            l.uncached_layouts
        );
    }

    #[test]
    fn nested_flex_containers_with_much_content_stay_linear() {
        // Many items inside deeply nested containers: the cache must keep
        // every level's results (cheap, because subtrees are shared).
        const DEPTH: usize = 20;
        let column = "<div style='display:flex; flex-direction:column'>".repeat(DEPTH);
        let content = "<div>x</div>".repeat(2000);
        let l = layout_html(&format!("<!DOCTYPE html><body>{column}{content}"));
        assert!(
            l.uncached_layouts <= 4 * (DEPTH + 2000),
            "{} layouts",
            l.uncached_layouts
        );
    }
}
