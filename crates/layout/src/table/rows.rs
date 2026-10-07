//! Row heights: distribution of the heights of spanning cells, row groups
//! and the table to rows (CSS Tables 3 §3.10,
//! <https://www.w3.org/TR/css-tables-3/#height-distribution>, which leaves
//! the algorithm open; this follows Chromium's
//! `DistributeExcessBlockSizeToRows` and
//! `DistributeTableBlockSizeToSections`).
//!
//! Parts of this file are derived from Chromium's `LayoutNG` table code
//! (`third_party/blink/renderer/core/layout/table/`:
//! `table_layout_utils.cc`, `table_layout_algorithm_types.cc` and `.h`,
//! `table_layout_algorithm.cc`; Copyright The Chromium Authors,
//! BSD-3-Clause); see `THIRD_PARTY_NOTICES.md`.

use std::ops::Range;

/// The height constraints of a row.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RowData {
    pub(crate) height: f32,
    /// The baseline of the row, from its top.
    pub(crate) baseline: f32,
    /// A percentage height (100% is 100.0).
    pub(crate) percent: Option<f32>,
    /// True if the row or one of its cells has a specified height.
    pub(crate) constrained: bool,
    /// True if a cell with a `rowspan` greater than 1 starts in the row.
    pub(crate) has_rowspan_start: bool,
}

/// The height constraints of a row group.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SectionData {
    /// Its rows, a range of all rows.
    pub(crate) rows: Range<usize>,
    pub(crate) height: f32,
    pub(crate) percent: Option<f32>,
    pub(crate) constrained: bool,
    /// False for the first header and footer group.
    pub(crate) is_body: bool,
    pub(crate) needs_redistribution: bool,
}

/// A cell that spans several rows, for height distribution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowspanCell {
    pub(crate) start_row: usize,
    pub(crate) rowspan: usize,
    pub(crate) min_height: f32,
}

impl RowspanCell {
    fn end(&self) -> usize {
        self.start_row + self.rowspan
    }

    fn encloses(&self, other: &RowspanCell) -> bool {
        other.start_row >= self.start_row && other.end() <= self.end()
    }

    /// Chromium's distribution order: cells that span the same rows,
    /// larger first; a cell inside another before it; else the lower start
    /// row first.
    fn before(&self, other: &RowspanCell) -> bool {
        if self.start_row == other.start_row && self.rowspan == other.rowspan {
            return self.min_height > other.min_height;
        }
        if other.encloses(self) {
            return true;
        }
        if self.encloses(other) {
            return false;
        }
        self.start_row < other.start_row
    }
}

/// Sorts spanning cells into distribution order with a stable merge sort.
/// The order is not a total order (as in Chromium), so the standard sort,
/// which may panic for such orders, is not used.
pub(crate) fn sort_rowspan_cells(cells: &mut Vec<RowspanCell>) {
    if cells.len() < 2 {
        return;
    }
    let mut right = cells.split_off(cells.len() / 2);
    sort_rowspan_cells(cells);
    sort_rowspan_cells(&mut right);
    let left = std::mem::take(cells);
    let (mut l, mut r) = (left.into_iter().peekable(), right.into_iter().peekable());
    while let (Some(a), Some(b)) = (l.peek(), r.peek()) {
        if b.before(a) {
            cells.extend(r.next());
        } else {
            cells.extend(l.next());
        }
    }
    cells.extend(l);
    cells.extend(r);
}

/// Distributes the extra height of a spanning cell to its rows.
pub(crate) fn distribute_rowspan_cell(cell: &RowspanCell, spacing: f32, rows: &mut [RowData]) {
    let end = cell.end().min(rows.len());
    if let Some(span) = rows.get_mut(cell.start_row..end) {
        distribute_to_rows(span, cell.min_height, true, spacing, None);
    }
}

/// Distributes `desired` (the total height including the border spacing
/// between the rows) to `rows`, if it is more than their height. With
/// `rowspan`, the rows (other than the first) where other spanning cells
/// start take all of it. `percent_basis` resolves percentage rows.
pub(crate) fn distribute_to_rows(
    rows: &mut [RowData],
    desired: f32,
    rowspan: bool,
    spacing: f32,
    percent_basis: Option<f32>,
) {
    if rows.is_empty() {
        return;
    }
    let total: f32 = rows.iter().map(|r| r.height).sum();
    let mut available = desired - spacing * (rows.len() - 1) as f32 - total;
    if available <= 0.0 {
        return;
    }
    available -= grow_percent_rows(rows, available, percent_basis);
    if available <= 0.0 {
        return;
    }
    let is_constrained =
        |row: &RowData| row.constrained && (row.percent.is_none() || percent_basis.is_some());
    if rowspan {
        // Rows (other than the first) where another spanning cell starts.
        let starts: Vec<usize> = (1..rows.len())
            .filter(|&i| rows[i].has_rowspan_start)
            .collect();
        if !starts.is_empty() {
            grow_evenly(rows, &starts, available);
            return;
        }
    }
    let unconstrained: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i].height != 0.0 && !is_constrained(&rows[i]))
        .collect();
    if !unconstrained.is_empty() {
        grow_proportionally(rows, &unconstrained, available);
        return;
    }
    if grow_empty_rows(rows, available, rowspan, is_constrained) {
        return;
    }
    let non_empty: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].height != 0.0).collect();
    let total: f32 = rows.iter().map(|r| r.height).sum();
    if !non_empty.is_empty() && total > 0.0 {
        grow_proportionally(rows, &non_empty, available);
    }
}

/// Grows percentage rows toward their percentage of `percent_basis`, by at
/// most `available` in total. Returns how much they grew.
fn grow_percent_rows(rows: &mut [RowData], available: f32, percent_basis: Option<f32>) -> f32 {
    let deficit = |row: &RowData| match (row.percent, percent_basis) {
        (Some(p), Some(basis)) if p != 0.0 => (p * basis / 100.0 - row.height).max(0.0),
        _ => 0.0,
    };
    let percent_rows: Vec<usize> = (0..rows.len())
        .filter(|&i| deficit(&rows[i]) > 0.0)
        .collect();
    if percent_rows.is_empty() {
        return 0.0;
    }
    let total_deficit: f32 = percent_rows.iter().map(|&i| deficit(&rows[i])).sum();
    let grow = total_deficit.min(available);
    let deltas: Vec<f32> = percent_rows
        .iter()
        .map(|&i| grow * deficit(&rows[i]) / total_deficit)
        .collect();
    for (&i, delta) in percent_rows.iter().zip(deltas) {
        rows[i].height += delta;
    }
    grow
}

/// Gives `available` to empty rows: for a spanning cell only if all rows
/// are empty (to the last one); else if the other rows have a specified
/// height (to the empty rows without one, if any). Returns true if it did.
fn grow_empty_rows(
    rows: &mut [RowData],
    available: f32,
    rowspan: bool,
    is_constrained: impl Fn(&RowData) -> bool,
) -> bool {
    let empty: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].height == 0.0).collect();
    if empty.is_empty() {
        return false;
    }
    let only_empty = empty.len() == rows.len();
    if rowspan {
        if only_empty && let Some(&last) = empty.last() {
            rows[last].height += available;
            return true;
        }
        return false;
    }
    let constrained_non_empty = (0..rows.len())
        .filter(|&i| rows[i].height != 0.0 && is_constrained(&rows[i]))
        .count();
    if !only_empty && empty.len() + constrained_non_empty != rows.len() {
        return false;
    }
    let unconstrained_empty: Vec<usize> = empty
        .iter()
        .copied()
        .filter(|&i| !is_constrained(&rows[i]))
        .collect();
    let grow = if unconstrained_empty.is_empty() {
        &empty
    } else {
        &unconstrained_empty
    };
    grow_evenly(rows, grow, available);
    true
}

/// Adds `amount / indices.len()` to each row in `indices`.
fn grow_evenly(rows: &mut [RowData], indices: &[usize], amount: f32) {
    let each = amount / indices.len().max(1) as f32;
    for &i in indices {
        rows[i].height += each;
    }
}

/// Adds `amount` to the rows in `indices` in proportion to their heights.
fn grow_proportionally(rows: &mut [RowData], indices: &[usize], amount: f32) {
    let total: f32 = indices.iter().map(|&i| rows[i].height).sum();
    if total <= 0.0 {
        grow_evenly(rows, indices, amount);
        return;
    }
    let heights: Vec<f32> = indices.iter().map(|&i| rows[i].height).collect();
    for (&i, height) in indices.iter().zip(heights) {
        rows[i].height += amount * height / total;
    }
}

/// Distributes the table's height (its content height without border,
/// padding and captions) to its row groups, then their rows. Body groups
/// take extra height before headers and footers; auto groups before
/// groups with a fixed height before percentage groups.
pub(crate) fn distribute_table_height(
    spacing: f32,
    table_height: f32,
    sections: &mut [SectionData],
    rows: &mut [RowData],
) {
    if sections.is_empty() {
        return;
    }
    let available = (table_height - (sections.len() + 1) as f32 * spacing).max(0.0);
    let percent_size = |s: &SectionData| {
        s.percent
            .map_or(s.height, |p| s.height.max(p * available / 100.0))
    };
    let mut minimum: f32 = sections.iter().map(|s| s.height).sum();
    if available <= minimum {
        return;
    }
    // Percentage groups grow toward their percentage.
    let percent_guess: f32 = sections.iter().map(percent_size).sum();
    if sections.iter().any(|s| s.percent.is_some()) && percent_guess > minimum {
        let grow = percent_guess.min(available) - minimum;
        let difference = percent_guess - minimum;
        for section in sections.iter_mut().filter(|s| s.percent.is_some()) {
            let delta = grow * (percent_size(section) - section.height) / difference;
            section.height += delta;
            section.needs_redistribution = true;
            minimum += delta;
        }
    }
    let has_body = sections.iter().any(|s| s.is_body);
    let candidates = |pick: fn(&SectionData) -> bool| -> Vec<usize> {
        (0..sections.len())
            .filter(|&i| (!has_body || sections[i].is_body) && pick(&sections[i]))
            .collect()
    };
    let auto = candidates(|s| s.percent.is_none() && !s.constrained);
    let fixed = candidates(|s| s.percent.is_none() && s.constrained);
    let percent = candidates(|s| s.percent.is_some());
    let grow = [auto, fixed, percent]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let left = available - minimum;
    if left > 0.0 && !grow.is_empty() {
        let total: f32 = grow.iter().map(|&i| sections[i].height).sum();
        for &i in &grow {
            let delta = if total > 0.0 {
                left * sections[i].height / total
            } else {
                left / grow.len() as f32
            };
            sections[i].height += delta;
            sections[i].needs_redistribution = true;
        }
    }
    for section in sections.iter() {
        if !section.needs_redistribution {
            continue;
        }
        if let Some(section_rows) = rows.get_mut(section.rows.clone()) {
            distribute_to_rows(
                section_rows,
                section.height,
                false,
                spacing,
                Some(section.height),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights(rows: &[RowData]) -> Vec<f32> {
        rows.iter().map(|r| r.height).collect()
    }

    fn row(height: f32) -> RowData {
        RowData {
            height,
            ..RowData::default()
        }
    }

    #[test]
    fn unconstrained_rows_grow_in_proportion() {
        let mut rows = vec![row(10.0), row(30.0)];
        distribute_to_rows(&mut rows, 82.0, false, 2.0, None);
        assert_eq!(heights(&rows), [20.0, 60.0]);
    }

    #[test]
    fn constrained_rows_keep_their_height() {
        let mut rows = vec![
            row(22.0),
            RowData {
                constrained: true,
                ..row(22.0)
            },
        ];
        // Chromium: a table of 100px with spacing 2px (3 spacings).
        distribute_to_rows(&mut rows, 94.0, false, 2.0, None);
        assert_eq!(heights(&rows), [70.0, 22.0]);
    }

    #[test]
    fn empty_rows_grow_when_others_are_constrained() {
        let mut rows = vec![
            RowData {
                constrained: true,
                ..row(10.0)
            },
            row(0.0),
        ];
        distribute_to_rows(&mut rows, 30.0, false, 0.0, None);
        assert_eq!(heights(&rows), [10.0, 20.0]);
    }

    #[test]
    fn spanning_cell_grows_rows_where_other_spans_start() {
        let mut rows = vec![
            row(10.0),
            RowData {
                has_rowspan_start: true,
                ..row(10.0)
            },
        ];
        distribute_to_rows(&mut rows, 50.0, true, 0.0, None);
        assert_eq!(heights(&rows), [10.0, 40.0]);
    }

    #[test]
    fn rowspan_order() {
        let cell = |start_row, rowspan, min_height| RowspanCell {
            start_row,
            rowspan,
            min_height,
        };
        let mut cells = vec![
            cell(0, 3, 10.0),
            cell(1, 2, 5.0),
            cell(0, 3, 20.0),
            cell(4, 2, 1.0),
        ];
        sort_rowspan_cells(&mut cells);
        assert_eq!(cells[0], cell(1, 2, 5.0));
        assert_eq!(cells[1], cell(0, 3, 20.0));
        assert_eq!(cells[2], cell(0, 3, 10.0));
        assert_eq!(cells[3], cell(4, 2, 1.0));
    }

    #[test]
    fn table_height_goes_to_body_groups() {
        let mut sections = vec![
            SectionData {
                rows: 0..1,
                height: 10.0,
                ..SectionData::default()
            },
            SectionData {
                rows: 1..2,
                height: 10.0,
                is_body: true,
                ..SectionData::default()
            },
        ];
        let mut rows = vec![row(10.0), row(10.0)];
        distribute_table_height(0.0, 100.0, &mut sections, &mut rows);
        assert_eq!(heights(&rows), [10.0, 90.0]);
    }
}
