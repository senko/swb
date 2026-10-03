//! Flex layout (CSS Flexible Box Layout Level 1, §9):
//! <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
//!
//! Supported: row and column directions (and reverse), wrapping, `order`,
//! flex-grow/shrink/basis, automatic minimum sizes, `justify-content`,
//! `align-items`/`align-self` (start, end, center, stretch, baseline as
//! start), `align-content` (start, end, center, space-*, stretch), `gap`,
//! and auto margins on the main axis.
//!
//! Not supported yet: absolutely positioned children (they are not laid
//! out), the container's min/max size in the flex algorithm, auto margins
//! on the cross axis. `align-content: stretch` grows the lines after the
//! items were stretched, so the items do not grow with their lines.

use swb_style::{Alignment, ComputedStyle, FlexBasis, FlexWrap, Gap, LengthPercentageOrAuto};

use crate::LayoutContext;
use crate::block::{
    BoxEdges, ContainingBlock, apply_relative_position, clamp_height, clamp_width,
    layout_flex_item, resolve_max_size, resolve_size,
};
use crate::box_tree::IndependentBox;
use crate::fragment::{BoxFragment, Fragment};
use crate::intrinsic;

/// The result of laying out a flex container's contents.
pub(crate) struct FlexLayout {
    /// Item fragments relative to the container's content box.
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) content_height: f32,
    pub(crate) first_baseline: Option<f32>,
}

/// The main and cross axes of a flex container, and the space in them.
struct Axes {
    row: bool,
    reverse: bool,
    main_available: Option<f32>,
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

fn gap(g: &Gap, basis: f32) -> f32 {
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
        match self.style.align_self {
            Alignment::Auto => container.align_items,
            a => a,
        }
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
pub(crate) fn layout_flex(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    children: &[IndependentBox],
    cb: ContainingBlock,
) -> FlexLayout {
    let axes = Axes::new(container, cb);
    let mut items = collect_items(ctx, container, children, &axes, cb);
    let lines = collect_lines(container, &items, &axes);

    // §9.7: resolve flexible lengths, per line.
    let container_main = axes.main_available.unwrap_or_else(|| {
        // Indefinite main size (column without height): the largest sum of
        // the hypothetical sizes of a line.
        lines
            .iter()
            .map(|l| line_main_size(&items[l.clone()], axes.main_gap, Item::outer_hypothetical))
            .fold(0.0, f32::max)
    });
    for line in &lines {
        resolve_flexible_lengths(&mut items[line.clone()], container_main, axes.main_gap);
    }

    let line_cross = determine_cross_sizes(ctx, container, &mut items, &lines, &axes, cb);
    place_items(
        container,
        &mut items,
        &lines,
        line_cross,
        container_main,
        &axes,
        cb,
    )
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
        (sizes.min, sizes.max)
    } else {
        // The content height at the item's cross size.
        let width = column_cross_size(ctx, &item, container, axes, cb);
        let f = layout_flex_item(ctx, b, width, None, cb);
        let h = (f.border_rect.height - main_edges).max(0.0);
        (h, h)
    };
    item.base_size = basis.unwrap_or(max_content);
    item.max_main = resolve_max_size(max_prop, axes.main_available, style.box_sizing, main_edges)
        .unwrap_or(f32::INFINITY);
    item.min_main = match min_prop {
        swb_style::Size::Auto => {
            // Automatic minimum size (§4.5): the content size suggestion,
            // capped by the specified size suggestion. Scroll containers
            // (but not `overflow: clip` boxes) have no automatic minimum.
            let overflow = if axes.row {
                style.overflow_x
            } else {
                style.overflow_y
            };
            if overflow.is_scroll_container() {
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

/// §9.3: collects the items into flex lines.
fn collect_lines(
    container: &ComputedStyle,
    items: &[Item<'_>],
    axes: &Axes,
) -> Vec<std::ops::Range<usize>> {
    if container.flex_wrap == FlexWrap::Nowrap {
        return std::iter::once(0..items.len()).collect();
    }
    let line_limit = axes.main_available.unwrap_or(f32::INFINITY);
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
    items.iter().map(size).sum::<f32>() + gap * (items.len().saturating_sub(1)) as f32
}

/// §9.4: lays out every item at its main size, determines the cross size
/// of every line, and stretches items. Returns the line cross sizes.
fn determine_cross_sizes(
    ctx: &mut LayoutContext<'_>,
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    lines: &[std::ops::Range<usize>],
    axes: &Axes,
    cb: ContainingBlock,
) -> Vec<f32> {
    // Hypothetical cross sizes: lay out each item with its main size.
    for item in items.iter_mut() {
        let (w, h) = if axes.row {
            (item.target, None)
        } else {
            let cross = column_cross_size(ctx, item, container, axes, cb);
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

    // The cross size of each line. A single-line container with a definite
    // cross size uses that size.
    let mut line_cross: Vec<f32> = lines
        .iter()
        .map(|l| {
            items[l.clone()]
                .iter()
                .map(Item::outer_cross)
                .fold(0.0, f32::max)
        })
        .collect();
    let single_line_definite = container.flex_wrap == FlexWrap::Nowrap
        && axes.cross_available.is_some()
        && (!axes.row || cb.height.is_some());
    if single_line_definite
        && let (Some(c), Some(first)) = (axes.cross_available, line_cross.first_mut())
    {
        *first = c;
    }

    // Stretch: items with auto cross size and align-self stretch take the
    // line's cross size.
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
    line_cross
}

/// §9.5 and §9.6: the container's cross size and `align-content`, then
/// main-axis and cross-axis alignment of the items in each line.
fn place_items(
    container: &ComputedStyle,
    items: &mut [Item<'_>],
    lines: &[std::ops::Range<usize>],
    mut line_cross: Vec<f32>,
    container_main: f32,
    axes: &Axes,
    cb: ContainingBlock,
) -> FlexLayout {
    let total_cross: f32 =
        line_cross.iter().sum::<f32>() + axes.cross_gap * (lines.len().saturating_sub(1)) as f32;
    let container_cross = if axes.row {
        cb.height.unwrap_or(total_cross)
    } else {
        cb.width
    };
    let free_cross = container_cross - total_cross;
    let (mut cross_pos, cross_between) =
        if lines.len() > 1 || container.flex_wrap != FlexWrap::Nowrap {
            distribute(container.align_content, free_cross, lines.len())
        } else {
            (0.0, 0.0)
        };
    if container.align_content == Alignment::Stretch
        || (container.align_content == Alignment::Normal && lines.len() > 1)
    {
        if free_cross > 0.0 && !lines.is_empty() {
            let extra = free_cross / lines.len() as f32;
            for c in &mut line_cross {
                *c += extra;
            }
        }
        cross_pos = 0.0;
    }

    let mut fragments = Vec::with_capacity(items.len());
    let mut first_baseline = None;
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
                apply_relative_position(&mut fragment, cb);
                if first_baseline.is_none() {
                    first_baseline = fragment.first_baseline.map(|b| b + fragment.border_rect.y);
                }
                fragments.push(Fragment::Box(fragment));
            }
            main_pos += item.target + item.main_edges + item.margin_main_end.unwrap_or(auto_margin);
        }
        cross_pos += line_cross[li] + axes.cross_gap + cross_between;
    }

    let content_height = if axes.row {
        total_cross.max(line_cross.iter().sum::<f32>())
    } else {
        container_main
    };
    FlexLayout {
        fragments,
        content_height,
        first_baseline,
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
    let gaps = gap * (items.len().saturating_sub(1)) as f32;
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
    use crate::test_support::layout_html;

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
