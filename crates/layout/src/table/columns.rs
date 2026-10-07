//! Column widths: column constraints from columns and cells, their
//! distribution, and the grid's min-content and max-content widths.
//!
//! CSS Tables 3 §3.8 (<https://www.w3.org/TR/css-tables-3/#content-measure>)
//! as Chromium's `LayoutNG` implements it (`table_layout_utils.cc`,
//! `table_layout_algorithm_types.cc`). Percentages are stored as in
//! Chromium: 100% is 100.0.
//!
//! A column is *mergeable* if no cell starts in it (in automatic layout):
//! it gets no width and no border spacing, as if it did not exist.
//!
//! Parts of this file are derived from Chromium's `LayoutNG` table code
//! (`third_party/blink/renderer/core/layout/table/`:
//! `table_layout_utils.cc`, `table_layout_algorithm_types.cc` and `.h`,
//! `table_layout_algorithm.cc`; Copyright The Chromium Authors,
//! BSD-3-Clause); see `THIRD_PARTY_NOTICES.md`.

use swb_style::{BoxSizing, ComputedStyle, LengthPercentage, MaxSize, Size};

use super::distribute::distribute_auto;
use super::grid::Grid;
use super::{ColumnBox, MAX_COLUMNS};
use crate::geom::{Edges, clamp_length};
use crate::intrinsic::ContentSizes;

/// The width constraints of one column.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Column {
    /// The min-content width, if a cell or column element gave one.
    pub(crate) min: Option<f32>,
    /// The max-content width (or the specified width), if known.
    pub(crate) max: Option<f32>,
    /// A percentage width (100% is 100.0).
    pub(crate) percent: Option<f32>,
    /// Border and padding added to a resolved percentage (fixed layout).
    pub(crate) percent_border_padding: f32,
    /// True if a cell or column of it has a specified (non-percentage)
    /// width.
    pub(crate) constrained: bool,
    /// True if no cell starts in this column (automatic layout).
    pub(crate) mergeable: bool,
    /// True if the column belongs to a table with fixed layout.
    pub(crate) fixed_table: bool,
}

impl Column {
    /// The min-content width (0 if unknown).
    pub(crate) fn min(&self) -> f32 {
        self.min.unwrap_or(0.0)
    }

    /// The max-content width (0 if unknown).
    pub(crate) fn max(&self) -> f32 {
        self.max.unwrap_or(0.0)
    }

    /// The width of a percentage column when the table's assignable width
    /// is `basis`.
    pub(super) fn percent_width(&self, basis: f32) -> f32 {
        let percent = self.percent.unwrap_or(0.0);
        self.min()
            .max(percent * basis / 100.0 + self.percent_border_padding)
    }

    /// True for a column with a specified (non-percentage) width.
    pub(super) fn is_fixed(&self) -> bool {
        self.constrained && self.percent.is_none() && self.max.is_some()
    }

    /// Widens the column for a cell that spans only this column.
    fn encompass(&mut self, cell: &CellWidths) {
        // Columns with a width in fixed layout ignore their cells.
        if self.constrained && self.fixed_table {
            return;
        }
        if !self.fixed_table {
            self.mergeable = false;
        }
        if let Some(min) = self.min {
            self.min = Some(min.max(cell.min));
            let max = self.max.unwrap_or(0.0);
            self.max = Some(if !self.constrained || cell.constrained {
                max.max(cell.max)
            } else {
                max.max(cell.min)
            });
        } else {
            self.min = Some(cell.min);
            self.max = Some(cell.max);
        }
        if let (Some(min), Some(max)) = (self.min, self.max) {
            self.max = Some(max.max(min));
        }
        if cell.percent > self.percent {
            self.percent = cell.percent;
            self.percent_border_padding = cell.percent_border_padding;
        }
        self.constrained |= cell.constrained;
    }
}

/// The width constraints of one cell (CSS Tables 3 §3.8.2,
/// <https://www.w3.org/TR/css-tables-3/#computing-cell-measures>).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CellWidths {
    /// The outer min-content width.
    pub(crate) min: f32,
    /// The outer max-content width.
    pub(crate) max: f32,
    /// A percentage width (100% is 100.0).
    pub(crate) percent: Option<f32>,
    /// Border and padding added to a resolved percentage (fixed layout).
    pub(crate) percent_border_padding: f32,
    /// True if the cell has a specified (non-percentage) width.
    pub(crate) constrained: bool,
}

impl CellWidths {
    /// Combines two cells that start in the same column and span one
    /// column.
    fn encompass(&mut self, other: &CellWidths) {
        self.min = self.min.max(other.min);
        self.max = if self.constrained == other.constrained {
            self.max.max(other.max)
        } else if self.constrained {
            self.max.max(other.min)
        } else {
            self.min.max(other.max)
        };
        self.constrained |= other.constrained;
        if other.percent > self.percent {
            self.percent = other.percent;
            self.percent_border_padding = other.percent_border_padding;
        }
    }
}

/// The outer (border-box) size of a specified size `v` of a box with
/// `border_padding`: with `box-sizing: border-box`, at least the border and
/// padding.
pub(crate) fn outer_size(style: &ComputedStyle, v: f32, border_padding: f32) -> f32 {
    match style.box_sizing {
        BoxSizing::ContentBox => v + border_padding,
        BoxSizing::BorderBox => v.max(border_padding),
    }
}

/// Specified inline sizes of a box: (width, min-width, max-width,
/// percentage width). Lengths include `border_padding` (with
/// `box-sizing: content-box`; with `border-box` they are at least it).
pub(crate) fn sizes_from_style(
    style: &ComputedStyle,
    border_padding: f32,
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    let outer = |v: f32| outer_size(style, v, border_padding);
    let fixed = |lp: Option<&LengthPercentage>| match lp {
        Some(LengthPercentage::Px(v)) => Some(outer(*v)),
        _ => None,
    };
    let width = fixed(style.width.as_length_percentage());
    let min = fixed(style.min_width.as_length_percentage());
    let max = fixed(style.max_width.as_length_percentage()).map(|m| m.max(min.unwrap_or(m)));
    let mut percent = match &style.width {
        Size::LengthPercentage(LengthPercentage::Percent(p)) => Some(p * 100.0),
        _ => None,
    };
    if let (Some(p), MaxSize::LengthPercentage(LengthPercentage::Percent(m))) =
        (percent, &style.max_width)
    {
        percent = Some(p.min(m * 100.0));
    }
    (width, min, max, percent)
}

/// The constraints of a column element (`col`, `colgroup`) with
/// `default_width` from its group (CSS Tables 3 §3.8.3,
/// <https://www.w3.org/TR/css-tables-3/#computing-column-measures>).
fn element_column(style: &ComputedStyle, default_width: Option<f32>, fixed: bool) -> Column {
    let (width, min, _, percent) = sizes_from_style(style, 0.0);
    let mut width = width.or(default_width);
    if let (Some(w), Some(m)) = (width, min) {
        width = Some(w.max(m));
    }
    let percent = percent.filter(|&p| p != 0.0);
    Column {
        min: Some(min.unwrap_or(0.0)),
        max: width,
        percent,
        percent_border_padding: 0.0,
        constrained: width.is_some(),
        mergeable: !fixed && width.unwrap_or(0.0) == 0.0 && percent.unwrap_or(0.0) == 0.0,
        fixed_table: fixed,
    }
}

/// The constraints that column elements give, one per column (at most
/// [`MAX_COLUMNS`]).
fn element_columns(columns: &[ColumnBox], fixed: bool) -> Vec<Column> {
    let limit = MAX_COLUMNS;
    let mut out = Vec::new();
    for column in columns {
        if out.len() >= limit {
            break;
        }
        let room = limit - out.len();
        if !column.is_group {
            let c = element_column(&column.base.style, None, fixed);
            out.extend(std::iter::repeat_n(c, column.span.min(room)));
            continue;
        }
        let group = element_column(&column.base.style, None, fixed);
        if column.children.is_empty() {
            out.extend(std::iter::repeat_n(group, column.span.min(room)));
            continue;
        }
        let default = if fixed { None } else { group.max };
        for child in &column.children {
            let room = limit.saturating_sub(out.len());
            let c = element_column(&child.base.style, default, fixed);
            out.extend(std::iter::repeat_n(c, child.span.min(room)));
        }
    }
    out
}

/// The number of columns up to the last column element that is not
/// mergeable (that has a width).
pub(crate) fn element_column_count(columns: &[ColumnBox], fixed: bool) -> usize {
    let constraints = element_columns(columns, fixed);
    constraints
        .iter()
        .rposition(|c| !c.mergeable)
        .map_or(0, |i| i + 1)
}

/// A cell with its constraints and grid position, for column constraints.
struct MeasuredCell {
    column: usize,
    colspan: usize,
    widths: CellWidths,
}

/// Computes the column constraints of a table (CSS Tables 3 §3.8.3,
/// Chromium's `ComputeColumnConstraints`). `measure(cell index)` returns the
/// constraints of a cell of the grid; in fixed layout only the cells of
/// the first row are measured.
pub(crate) fn column_constraints(
    columns: &[ColumnBox],
    grid: &Grid<'_>,
    fixed: bool,
    border_spacing: f32,
    measure: &mut dyn FnMut(usize) -> CellWidths,
) -> Vec<Column> {
    let mut constraints = element_columns(columns, fixed);
    let mut cells: Vec<Option<CellWidths>> = Vec::new();
    let mut spanning: Vec<MeasuredCell> = Vec::new();
    let first_row = grid
        .sections
        .iter()
        .find(|s| !s.rows.is_empty())
        .map(|s| s.rows.start);
    for (index, cell) in grid.cells.iter().enumerate() {
        // Automatic layout makes one column per cell start; fixed layout one
        // per spanned column.
        let end = if fixed {
            cell.column + cell.colspan
        } else {
            cell.column + 1
        };
        if cells.len() < end {
            cells.resize(end, None);
        }
        if fixed && Some(cell.row) != first_row {
            continue;
        }
        let widths = measure(index);
        if cell.colspan == 1 {
            match &mut cells[cell.column] {
                Some(existing) => existing.encompass(&widths),
                slot @ None => *slot = Some(widths),
            }
        } else {
            spanning.push(MeasuredCell {
                column: cell.column,
                colspan: cell.colspan,
                widths,
            });
        }
    }
    apply_cells(
        &mut constraints,
        &cells,
        &mut spanning,
        fixed,
        border_spacing,
    );
    constraints
}

/// Applies cell constraints to the column constraints (Chromium's
/// `ApplyCellConstraintsToColumnConstraints`).
fn apply_cells(
    columns: &mut Vec<Column>,
    cells: &[Option<CellWidths>],
    spanning: &mut [MeasuredCell],
    fixed: bool,
    border_spacing: f32,
) {
    if columns.len() < cells.len() {
        let default = Column {
            fixed_table: fixed,
            mergeable: !fixed,
            ..Column::default()
        };
        columns.resize(cells.len(), default);
    } else {
        // Remove mergeable columns after the last cell.
        while columns.len() > cells.len() && columns.last().is_some_and(|c| c.mergeable) {
            columns.pop();
        }
    }
    // Every spanning cell needs a column that can take width.
    for cell in spanning.iter() {
        let end = (cell.column + cell.colspan).min(columns.len());
        if let Some(first) = columns
            .get_mut(cell.column..end)
            .and_then(|c| c.first_mut())
        {
            first.mergeable = false;
        }
    }
    for (column, cell) in columns.iter_mut().zip(cells) {
        if let Some(cell) = cell {
            column.encompass(cell);
        }
    }
    spanning.sort_by_key(|c| (c.colspan, c.column));
    for cell in spanning.iter() {
        if fixed {
            distribute_spanning_fixed(cell, border_spacing, columns);
        } else {
            distribute_spanning_auto(cell, border_spacing, columns);
        }
    }
    // The percentages add up to at most 100%.
    let mut total = 0.0;
    for column in columns.iter_mut() {
        if let Some(p) = column.percent {
            let p = if !fixed && p + total > 100.0 {
                100.0 - total
            } else {
                p
            };
            column.percent = Some(p);
            total += p;
        }
        column.min = Some(column.min());
        column.max = Some(column.max());
    }
    if fixed && total > 100.0 {
        for column in columns.iter_mut() {
            if let Some(p) = &mut column.percent {
                *p = *p * 100.0 / total;
            }
        }
    }
}

/// The inner border spacing of the columns a cell spans (between its
/// non-mergeable columns).
fn inner_spacing(columns: &[Column], border_spacing: f32) -> f32 {
    let count = columns.iter().filter(|c| !c.mergeable).count();
    border_spacing * count.saturating_sub(1) as f32
}

/// Distributes a spanning cell to its columns in fixed layout: evenly.
fn distribute_spanning_fixed(cell: &MeasuredCell, border_spacing: f32, columns: &mut [Column]) {
    let end = (cell.column + cell.colspan).min(columns.len());
    let Some(span) = columns.get_mut(cell.column..end) else {
        return;
    };
    let spacing = inner_spacing(span, border_spacing);
    let count = span.iter().filter(|c| !c.mergeable).count();
    if count == 0 {
        return;
    }
    let widths = cell.widths;
    let min = if widths.constrained {
        (widths.min - spacing).max(0.0)
    } else {
        0.0
    };
    let max = (widths.max - spacing).max(0.0);
    let (each_min, each_max) = (min / count as f32, max / count as f32);
    let percent = widths.percent.map(|p| p / count as f32);
    for column in span.iter_mut().filter(|c| !c.mergeable) {
        if column.min.is_none() {
            column.constrained |= widths.constrained;
            column.min = Some(each_min);
        }
        if column.max.is_none() {
            column.constrained |= widths.constrained;
            column.max = Some(each_max);
        }
        // Percentages go only to auto columns.
        if column.percent.is_none() && !column.constrained {
            column.percent = percent;
        }
    }
}

/// Distributes a spanning cell to its columns in automatic layout: the
/// cell's percentage to the columns without one (in proportion to their
/// max-content widths), then its min-content and max-content widths with
/// the width distribution algorithm.
fn distribute_spanning_auto(cell: &MeasuredCell, border_spacing: f32, columns: &mut [Column]) {
    let end = (cell.column + cell.colspan).min(columns.len());
    let Some(span) = columns.get_mut(cell.column..end) else {
        return;
    };
    if span.is_empty() {
        return;
    }
    for column in span.iter_mut() {
        column.min = Some(column.min());
        column.max = Some(column.max());
    }
    let spacing = inner_spacing(span, border_spacing);
    let widths = cell.widths;
    if let Some(cell_percent) = widths.percent {
        distribute_spanning_percent(cell_percent, span);
    }
    let min = (widths.min - spacing).max(0.0);
    let max = (widths.max - spacing).max(0.0);
    let mins = distribute_auto(min, span, true);
    for (column, size) in span.iter_mut().zip(mins) {
        column.min = Some(column.min().max(size));
    }
    let maxes = distribute_auto(max, span, widths.constrained);
    for (column, size) in span.iter_mut().zip(maxes) {
        column.max = Some(column.min().max(column.max()).max(size));
    }
}

/// Gives the part of a spanning cell's percentage that its columns do not
/// have yet to its columns without a percentage, in proportion to their
/// max-content widths (evenly if they are all 0).
fn distribute_spanning_percent(cell_percent: f32, span: &mut [Column]) {
    let active = || span.iter().filter(|c| !c.mergeable);
    let columns_percent: f32 = active().filter_map(|c| c.percent).sum();
    let all = active().count();
    let percent_count = active().filter(|c| c.percent.is_some()).count();
    let other_count = all - percent_count;
    let other_max: f32 = active()
        .filter(|c| c.percent.is_none())
        .map(Column::max)
        .sum();
    let surplus = cell_percent - columns_percent;
    if surplus <= 0.0 || other_count == 0 {
        return;
    }
    for column in span
        .iter_mut()
        .filter(|c| c.percent.is_none() && !c.mergeable)
    {
        column.percent = Some(if other_max == 0.0 {
            surplus / other_count as f32
        } else {
            surplus * column.max() / other_max
        });
    }
}

/// The grid's min-content and max-content widths (CSS Tables 3 §3.9.1,
/// Chromium's `ComputeGridInlineMinMax`), including `undistributable`
/// (border, padding and border spacing). With `allow_percent`, percentage
/// columns widen the max-content width so that their percentages can be
/// met.
pub(crate) fn grid_min_max(
    columns: &[Column],
    undistributable: f32,
    fixed: bool,
    allow_percent: bool,
) -> ContentSizes {
    let mut sizes = ContentSizes::default();
    let mut percent_estimate = 0.0f32;
    let mut non_percent_max = 0.0;
    let mut percent_sum = 0.0;
    for c in columns {
        sizes.min += if fixed && c.is_fixed() {
            c.max()
        } else {
            c.min()
        };
        match c.percent.filter(|&p| p > 0.0) {
            Some(p) => {
                if c.max() > 0.0 {
                    percent_estimate =
                        percent_estimate.max(100.0 / p * (c.max() - c.percent_border_padding));
                }
            }
            None => non_percent_max += c.max(),
        }
        sizes.max += c.max();
        percent_sum += c.percent.unwrap_or(0.0);
    }
    let percent_sum: f32 = f32::min(percent_sum, 100.0);
    if percent_sum > 0.0 && allow_percent {
        let from_percent = if non_percent_max == 0.0 {
            0.0
        } else if percent_sum == 100.0 {
            TABLE_MAX_WIDTH
        } else {
            100.0 / (100.0 - percent_sum) * non_percent_max
        };
        let from_percent = from_percent.min(TABLE_MAX_WIDTH);
        sizes.max = sizes
            .max
            .max(from_percent)
            .max(percent_estimate.min(TABLE_MAX_WIDTH));
    }
    sizes.max = sizes.max.max(sizes.min);
    ContentSizes {
        min: sizes.min + undistributable,
        max: sizes.max + undistributable,
    }
}

/// The largest table width that percentages can ask for (Chromium's
/// `kTableMaxInlineSize`).
pub(crate) const TABLE_MAX_WIDTH: f32 = 1_000_000.0;

/// The border, padding and border spacing that do not belong to columns
/// (CSS Tables 3 §3.8.1,
/// <https://www.w3.org/TR/css-tables-3/#computing-undistributable-space>;
/// Chromium's `ComputeUndistributableTableSpace`).
pub(crate) fn undistributable_space(columns: &[Column], border_padding: f32, spacing: f32) -> f32 {
    let active = columns.iter().filter(|c| !c.mergeable).count();
    border_padding + (active.max(1) + 1) as f32 * spacing
}

/// The position of a column in the grid, relative to the table's padding
/// box (the first column starts after one border spacing).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ColumnLocation {
    pub(crate) offset: f32,
    pub(crate) size: f32,
}

/// Column positions for column widths `sizes`. Empty mergeable columns
/// take no space and no border spacing.
pub(crate) fn column_locations(
    columns: &[Column],
    sizes: &[f32],
    spacing: f32,
) -> Vec<ColumnLocation> {
    let mut offset = spacing;
    let mut first = true;
    columns
        .iter()
        .zip(sizes)
        .map(|(c, &size)| {
            if c.mergeable && size == 0.0 {
                return ColumnLocation { offset, size: 0.0 };
            }
            if !first {
                offset = clamp_length(offset + spacing);
            }
            first = false;
            let size = clamp_length(size);
            let location = ColumnLocation { offset, size };
            offset = clamp_length(offset + size);
            location
        })
        .collect()
}

/// The horizontal border and padding of a cell, for measuring: padding
/// percentages count as 0.
pub(crate) fn cell_border_padding(style: &ComputedStyle, border: Edges) -> f32 {
    border.horizontal() + style.padding_left.resolve(0.0) + style.padding_right.resolve(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(min: f32, max: f32) -> Column {
        Column {
            min: Some(min),
            max: Some(max),
            ..Column::default()
        }
    }

    fn percent(min: f32, max: f32, p: f32) -> Column {
        Column {
            percent: Some(p),
            ..auto(min, max)
        }
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
    }

    #[test]
    fn mergeable_columns_get_nothing() {
        let mut merged = auto(0.0, 0.0);
        merged.mergeable = true;
        let columns = [auto(10.0, 20.0), merged];
        assert!(close(
            &distribute_auto(100.0, &columns, true),
            &[100.0, 0.0]
        ));
        let locations = column_locations(&columns, &[100.0, 0.0], 2.0);
        assert_eq!(locations[1].offset, 102.0);
        assert_eq!(undistributable_space(&columns, 0.0, 2.0), 4.0);
    }

    #[test]
    fn spanning_cell_widens_its_columns() {
        let mut columns = vec![auto(10.0, 10.0), auto(10.0, 30.0)];
        let cell = MeasuredCell {
            column: 0,
            colspan: 2,
            widths: CellWidths {
                min: 50.0,
                max: 100.0,
                percent: None,
                percent_border_padding: 0.0,
                constrained: false,
            },
        };
        distribute_spanning_auto(&cell, 0.0, &mut columns);
        let (a, b) = (&columns[0], &columns[1]);
        assert!((a.min() + b.min() - 50.0).abs() < 0.01);
        assert!((a.max() + b.max() - 100.0).abs() < 0.01);
        // In proportion to the max-content widths.
        assert!((b.max() / a.max() - 3.0).abs() < 0.01);
    }

    #[test]
    fn grid_widths_with_percentages() {
        let columns = [auto(10.0, 100.0), percent(10.0, 10.0, 50.0)];
        let sizes = grid_min_max(&columns, 0.0, false, true);
        assert_eq!(sizes.min, 20.0);
        // 100px must be 50% of the table.
        assert_eq!(sizes.max, 200.0);
        assert_eq!(grid_min_max(&columns, 0.0, false, false).max, 110.0);
    }
}
