//! Table cells: their width constraints, their layout, the row heights
//! and baselines they give, and the vertical alignment of their content.
//!
//! A cell is laid out once at its column width with an automatic height
//! (its `height` is ignored, as in Chromium); its fragment is then made as
//! tall as its rows, and its content moved for `vertical-align`
//! (CSS 2.2 §17.5.3, <https://www.w3.org/TR/CSS22/tables.html#height-layout>).
//! Layouts are cached per cell and width, so that nested tables do not
//! take exponential time.

use std::sync::Arc;

use swb_style::{BoxSizing, ComputedStyle, LengthPercentage, VerticalAlign, VerticalAlignKeyword};

use super::TableCache;
use super::columns::{CellWidths, cell_border_padding, sizes_from_style};
use super::grid::GridCell;
use super::rows::{RowData, RowspanCell};
use crate::block::{BoxEdges, ContainingBlock, finish_fragment, layout_contents};
use crate::fragment::{BoxFragment, Fragment};
use crate::geom::{Edges, Rect, clamp_length};
use crate::{LayoutContext, LayoutKey};

/// The width constraints of a cell (Chromium's
/// `CreateCellInlineConstraint`). In fixed layout the min-content width is
/// 0, and the content is measured only for the max-content width of a cell
/// without a width.
pub(crate) fn cell_widths(
    ctx: &mut LayoutContext<'_>,
    cell: &GridCell<'_>,
    border: Edges,
    fixed: bool,
) -> CellWidths {
    let ib = &cell.cell.inner;
    let style = &ib.base.style;
    let border_padding = cell_border_padding(style, border);
    let (width, min_width, max_width, percent) = sizes_from_style(style, border_padding);
    let mut content = None;
    let mut content_sizes = |ctx: &mut LayoutContext<'_>| {
        *content.get_or_insert_with(|| {
            let sizes = TableCache::percent_free(ctx, |ctx| {
                crate::intrinsic::independent_content_sizes(ctx, ib)
            });
            (sizes.min + border_padding, sizes.max + border_padding)
        })
    };
    let mut min = 0.0;
    if !fixed {
        min = content_sizes(ctx).0.max(min_width.unwrap_or(0.0));
        // The table cell nowrap minimum width quirk:
        // https://quirks.spec.whatwg.org/#the-table-cell-nowrap-minimum-width-calculation-quirk
        if let Some(width) = width
            && ctx.quirks
            && cell.cell.nowrap
        {
            min = min.max(width);
        }
    }
    let mut content_max = match width {
        Some(width) => width,
        None => content_sizes(ctx).1,
    };
    if let Some(max_width) = max_width {
        content_max = content_max.min(max_width);
        min = min.min(max_width);
    }
    let percent_border_padding =
        if fixed && percent.is_some() && style.box_sizing == BoxSizing::ContentBox {
            border_padding
        } else {
            0.0
        };
    CellWidths {
        min,
        max: min.max(content_max),
        percent,
        percent_border_padding,
        constrained: width.is_some(),
    }
}

/// The border and padding of a cell. Padding percentages resolve against
/// `basis` (the width of the table's row groups).
pub(crate) fn cell_edges(style: &ComputedStyle, border: Edges, basis: f32) -> BoxEdges {
    BoxEdges {
        padding: Edges::new(
            style.padding_top.resolve(basis),
            style.padding_right.resolve(basis),
            style.padding_bottom.resolve(basis),
            style.padding_left.resolve(basis),
        ),
        border,
    }
}

/// Lays out a cell with border-box width `width`. The fragment's height
/// is the height of its content (and border and padding). With
/// `content_height`, percentage heights inside resolve against it (a cell
/// with a definite height). The fragment is at (0, 0).
pub(crate) fn layout_cell(
    ctx: &mut LayoutContext<'_>,
    cell: &GridCell<'_>,
    edges: &BoxEdges,
    width: f32,
    basis: f32,
    content_height: Option<f32>,
) -> BoxFragment {
    let ib = &cell.cell.inner;
    let sum = edges.sum();
    let content_width = (width - sum.horizontal()).max(0.0);
    let cb = ContainingBlock {
        width: basis,
        height: None,
    };
    let key = LayoutKey::new(ib.base.id, content_width, content_height, cb);
    if let Some(fragment) = ctx.layouts.get(&key) {
        return fragment.clone();
    }
    ctx.uncached_layouts += 1;
    let content_cb = ContainingBlock {
        width: content_width,
        height: content_height,
    };
    let mut markers = Vec::new();
    let children = TableCache::percent_free(ctx, |ctx| {
        layout_contents(ctx, ib, content_cb, &mut markers)
    });
    let height = children.content_height + sum.vertical();
    let fragment = finish_fragment(
        &ib.base,
        Rect::new(0.0, 0.0, width, height),
        edges,
        children.fragments,
        children.baselines.offset(sum.top),
    );
    ctx.layouts.insert(key, fragment.clone());
    fragment
}

/// How a cell aligns its content vertically (Chromium's
/// `ComputeContentAlignmentForTableCell`): `top`, `middle` and `bottom`
/// align to the cell's content box; all other values align the first
/// baseline with the row's baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellAlign {
    Top,
    Middle,
    Bottom,
    Baseline,
}

impl CellAlign {
    pub(crate) fn of(style: &ComputedStyle) -> Self {
        match style.vertical_align {
            VerticalAlign::Keyword(VerticalAlignKeyword::Top) => CellAlign::Top,
            VerticalAlign::Keyword(VerticalAlignKeyword::Middle) => CellAlign::Middle,
            VerticalAlign::Keyword(VerticalAlignKeyword::Bottom) => CellAlign::Bottom,
            _ => CellAlign::Baseline,
        }
    }
}

/// The baseline of a laid-out cell for row alignment: its first baseline,
/// or the bottom of its content box if it has none.
fn cell_baseline(fragment: &BoxFragment) -> f32 {
    fragment
        .first_baseline
        .unwrap_or(fragment.border_rect.height - fragment.border.bottom - fragment.padding.bottom)
}

/// The baseline of a row from its cells (Chromium's
/// `RowBaselineTabulator`): the largest baseline of the baseline-aligned
/// cells with content; without such cells, the bottom of the content box
/// of the cell with the smallest bottom border and padding.
#[derive(Default)]
pub(crate) struct RowBaseline {
    ascent: Option<f32>,
    descent: Option<f32>,
    fallback_descent: Option<f32>,
}

impl RowBaseline {
    pub(crate) fn add(&mut self, fragment: &BoxFragment, spans_rows: bool) {
        let align = CellAlign::of(&fragment.style);
        if align == CellAlign::Baseline && !fragment.children.is_empty() {
            let baseline = cell_baseline(fragment);
            self.ascent = Some(self.ascent.map_or(baseline, |a| a.max(baseline)));
            let descent = if spans_rows {
                0.0
            } else {
                fragment.border_rect.height - baseline
            };
            self.descent = Some(self.descent.map_or(descent, |d| d.max(descent)));
        }
        if self.ascent.is_none() {
            let bottom = fragment.padding.bottom + fragment.border.bottom;
            self.fallback_descent = Some(self.fallback_descent.map_or(bottom, |d| d.min(bottom)));
        }
    }

    /// The row height for cells of at most `cell_height`.
    pub(crate) fn row_height(&self, cell_height: f32) -> f32 {
        match (self.ascent, self.descent) {
            (Some(a), Some(d)) => cell_height.max(a + d),
            _ => cell_height,
        }
    }

    /// The baseline of a row of height `height`.
    pub(crate) fn baseline(&self, height: f32) -> f32 {
        match (self.ascent, self.fallback_descent) {
            (Some(a), _) => a,
            (None, Some(d)) => (height - d).max(0.0),
            (None, None) => 0.0,
        }
    }
}

/// True if a cell's height is definite for its content (Chromium's
/// `ComputeCellBlockSize`): it has a fixed height, or the table has a
/// height and the cell grew beyond its content.
pub(crate) fn has_definite_height(style: &ComputedStyle, table_height: bool, grew: bool) -> bool {
    matches!(
        style.height.as_length_percentage(),
        Some(LengthPercentage::Px(_))
    ) || (table_height && grew)
}

/// The specified height of a cell as a border-box height (fixed lengths),
/// or a percentage (100% is 100.0). In quirks mode a cell's height
/// includes its border and padding (the table cell height box sizing
/// quirk, <https://quirks.spec.whatwg.org/#the-table-cell-height-box-sizing-quirk>).
fn cell_height(
    style: &ComputedStyle,
    edges: &BoxEdges,
    quirks: bool,
) -> (Option<f32>, Option<f32>) {
    match style.height.as_length_percentage() {
        Some(LengthPercentage::Px(v)) => {
            let border_padding = edges.sum().vertical();
            let height = if quirks || style.box_sizing == BoxSizing::BorderBox {
                border_padding.max(*v)
            } else {
                border_padding + v
            };
            (Some(height), None)
        }
        Some(LengthPercentage::Percent(p)) => (None, Some(p * 100.0)),
        _ => (None, None),
    }
}

/// A row's constraints from its laid-out cells and its own height
/// (Chromium's `ComputeMinimumRowBlockSize`). `cells` are the row's cells
/// with their measured fragments and edges; spanning cells are added to
/// `spanning`.
pub(crate) fn row_data(
    row_style: &ComputedStyle,
    row_index: usize,
    cells: &[(&GridCell<'_>, &BoxFragment, &BoxEdges)],
    quirks: bool,
    spanning: &mut Vec<RowspanCell>,
) -> RowData {
    let mut data = RowData::default();
    let mut tallest: f32 = 0.0;
    let mut baseline = RowBaseline::default();
    for &(cell, fragment, edges) in cells {
        let spans_rows = cell.rowspan > 1;
        baseline.add(fragment, spans_rows);
        let style = &cell.cell.inner.base.style;
        let (css_height, percent) = cell_height(style, edges, quirks);
        let measured = fragment.border_rect.height;
        if spans_rows {
            data.has_rowspan_start = true;
            spanning.push(RowspanCell {
                start_row: row_index,
                rowspan: cell.rowspan,
                min_height: measured.max(css_height.unwrap_or(0.0)),
            });
        } else {
            data.constrained |= css_height.is_some() || percent.is_some();
            if let Some(p) = percent {
                data.percent = Some(data.percent.map_or(p, |q| q.max(p)));
            }
            tallest = tallest.max(measured).max(css_height.unwrap_or(0.0));
        }
    }
    match row_style.height.as_length_percentage() {
        Some(LengthPercentage::Percent(p)) => {
            data.constrained = true;
            let p = p * 100.0;
            data.percent = Some(data.percent.map_or(p, |q| q.max(p)));
        }
        Some(LengthPercentage::Px(v)) => {
            data.constrained = true;
            tallest = tallest.max(*v);
        }
        _ => {}
    }
    data.height = clamp_length(baseline.row_height(tallest));
    data.baseline = baseline.baseline(data.height);
    data
}

/// Makes a measured cell fragment `height` tall and moves its content for
/// its vertical alignment. `row_baseline` is the baseline of its row.
pub(crate) fn align_cell(fragment: &mut BoxFragment, height: f32, row_baseline: f32) {
    let measured = fragment.border_rect.height;
    let free = height - measured;
    let offset = match CellAlign::of(&fragment.style) {
        CellAlign::Top => 0.0,
        CellAlign::Middle => free.max(0.0) / 2.0,
        CellAlign::Bottom => free.max(0.0),
        CellAlign::Baseline if fragment.children.is_empty() => 0.0,
        CellAlign::Baseline => row_baseline - cell_baseline(fragment),
    };
    fragment.border_rect.height = height;
    if offset != 0.0 {
        for child in Arc::make_mut(&mut fragment.children) {
            child.move_by(0.0, offset);
        }
        fragment.first_baseline = fragment.first_baseline.map(|b| b + offset);
        fragment.last_baseline = fragment.last_baseline.map(|b| b + offset);
    }
}

/// True if the cell fragment has content for `empty-cells`: anything but
/// absolutely positioned boxes (floats count, CSS 2.2 §17.6.1.1, and so in
/// Chromium).
pub(crate) fn has_content(fragment: &BoxFragment) -> bool {
    fragment.children.iter().any(|child| match child {
        Fragment::Box(b) => !b.style.is_absolutely_positioned(),
        Fragment::Text(_) => true,
    })
}
