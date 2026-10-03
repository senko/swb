//! The table grid: the row and column of every cell, and its spans
//! (CSS Tables 3 §3.3, <https://www.w3.org/TR/css-tables-3/#table-grid>,
//! and the HTML table model).
//!
//! A cell takes the first column of its row that no cell from an earlier
//! row spans into. A `rowspan` is clamped to the rows of its row group
//! (`rowspan=0` spans all of them). Cells that would start after
//! [`MAX_COLUMNS`] are dropped; spans end at that column.
//!
//! [`place`] computes the positions once per table and layout pass (the
//! table cache keeps them); [`Grid`] joins them with the boxes. Finding the
//! first free column takes O(log columns) per cell, also with many cells
//! that span rows.

use std::ops::Range;

use super::{CellBox, MAX_COLUMNS, MAX_ROWSPAN, RowBox, SectionBox, TableBox};

/// A cell and its place in the grid.
pub(crate) struct GridCell<'a> {
    pub(crate) cell: &'a CellBox,
    /// The index of its row in [`Grid::rows`].
    pub(crate) row: usize,
    pub(crate) column: usize,
    /// The number of columns it spans (at least 1).
    pub(crate) colspan: usize,
    /// The number of rows it spans, within its row group (at least 1).
    pub(crate) rowspan: usize,
}

/// A row and its cells.
pub(crate) struct GridRow<'a> {
    pub(crate) row: &'a RowBox,
    /// The index of its row group in [`Grid::sections`].
    pub(crate) section: usize,
    /// Its cells: a range of [`Grid::cells`].
    pub(crate) cells: Range<usize>,
}

/// A row group and its rows.
pub(crate) struct GridSection<'a> {
    pub(crate) section: &'a SectionBox,
    /// Its rows: a range of [`Grid::rows`].
    pub(crate) rows: Range<usize>,
}

/// The cells of a table in grid order (row groups in layout order, rows,
/// cells).
pub(crate) struct Grid<'a> {
    pub(crate) sections: Vec<GridSection<'a>>,
    pub(crate) rows: Vec<GridRow<'a>>,
    pub(crate) cells: Vec<GridCell<'a>>,
}

/// The grid position of a cell: column, colspan and rowspan (within its
/// row group). `None` for a cell that would start after [`MAX_COLUMNS`].
pub(crate) type Placement = Option<(u32, u32, u32)>;

impl<'a> Grid<'a> {
    /// The cells of `table` at the positions `placements` (from [`place`],
    /// one per cell in tree order).
    pub(crate) fn new(table: &'a TableBox, placements: &[Placement]) -> Self {
        let mut grid = Grid {
            sections: Vec::with_capacity(table.sections.len()),
            rows: Vec::new(),
            cells: Vec::new(),
        };
        let mut placements = placements.iter();
        for section in &table.sections {
            let first_row = grid.rows.len();
            for row in &section.rows {
                let first_cell = grid.cells.len();
                for cell in &row.cells {
                    let Some(&Some((column, colspan, rowspan))) = placements.next() else {
                        continue;
                    };
                    grid.cells.push(GridCell {
                        cell,
                        row: grid.rows.len(),
                        column: column as usize,
                        colspan: colspan as usize,
                        rowspan: rowspan as usize,
                    });
                }
                grid.rows.push(GridRow {
                    row,
                    section: grid.sections.len(),
                    cells: first_cell..grid.cells.len(),
                });
            }
            grid.sections.push(GridSection {
                section,
                rows: first_row..grid.rows.len(),
            });
        }
        grid
    }
}

/// Computes the grid position of every cell of `table`, in tree order.
/// Warns about dropped cells if `warned` is false (once per layout pass),
/// and sets it.
pub(crate) fn place(table: &TableBox, warned: &mut bool) -> Vec<Placement> {
    let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    let mut placements = Vec::new();
    let mut occupied = Occupancy::new(column_bound(table));
    let mut dropped = false;
    // Rows are numbered across row groups; spans end with their group.
    let mut row_number = 0;
    for section in &table.sections {
        for (index, row) in section.rows.iter().enumerate() {
            let rows_left = section.rows.len() - index;
            let mut column = 0;
            for cell in &row.cells {
                column = occupied.first_free(column, row_number);
                if column >= MAX_COLUMNS {
                    // Columns only increase along a row.
                    placements.push(None);
                    dropped = true;
                    continue;
                }
                let colspan = cell.colspan.clamp(1, MAX_COLUMNS - column);
                let rowspan = if cell.rowspan == 0 {
                    MAX_ROWSPAN
                } else {
                    cell.rowspan
                };
                let rowspan = rowspan.clamp(1, rows_left);
                if rowspan > 1 {
                    occupied.occupy(column..column + colspan, row_number + rowspan);
                }
                placements.push(Some((to_u32(column), to_u32(colspan), to_u32(rowspan))));
                column += colspan;
            }
            row_number += 1;
        }
    }
    if dropped && !*warned {
        *warned = true;
        log::warn!("table cells after column {MAX_COLUMNS} are not laid out");
    }
    placements
}

/// The sum of the colspans of all cells: no cell ends after this column
/// (the columns before a cell are covered by other cells).
fn column_bound(table: &TableBox) -> usize {
    table
        .sections
        .iter()
        .flat_map(|section| &section.rows)
        .flat_map(|row| &row.cells)
        .fold(0, |sum: usize, cell| {
            sum.saturating_add(cell.colspan.max(1))
        })
}

/// For every column, the row (exclusive) until which a cell from an
/// earlier row spans into it: a segment tree of minimums with lazy range
/// maximum updates (a cell can span fewer rows than an earlier cell in the
/// same column), allocated at the first cell that spans rows.
struct Occupancy {
    /// The number of leaves: a power of two, at least the number of
    /// columns that cells occupy.
    size: usize,
    /// The minimum of each node (1 is the root; leaves start at `size`).
    min: Vec<usize>,
    /// The value that all leaves below an inner node are raised to (0 for
    /// none).
    pending: Vec<usize>,
}

impl Occupancy {
    /// An empty grid for cells in the first `columns` columns.
    fn new(columns: usize) -> Self {
        Occupancy {
            size: columns.clamp(1, MAX_COLUMNS).next_power_of_two(),
            min: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// The first column at or after `from` that is free in row `row`.
    fn first_free(&mut self, from: usize, row: usize) -> usize {
        if self.min.is_empty() || from >= self.size {
            return from;
        }
        self.find(1, 0..self.size, from, row).unwrap_or(self.size)
    }

    /// Marks `columns` as occupied until row `until`, or later if they
    /// already are.
    fn occupy(&mut self, columns: Range<usize>, until: usize) {
        if self.min.is_empty() {
            self.min = vec![0; 2 * self.size];
            self.pending = vec![0; self.size];
        }
        self.raise(1, 0..self.size, &columns, until);
    }

    fn find(&mut self, node: usize, span: Range<usize>, from: usize, row: usize) -> Option<usize> {
        if span.end <= from || self.min[node] > row {
            return None;
        }
        if span.len() == 1 {
            return Some(span.start);
        }
        self.push_down(node);
        let middle = span.start + span.len() / 2;
        self.find(2 * node, span.start..middle, from, row)
            .or_else(|| self.find(2 * node + 1, middle..span.end, from, row))
    }

    fn raise(&mut self, node: usize, span: Range<usize>, columns: &Range<usize>, value: usize) {
        if columns.end <= span.start || span.end <= columns.start {
            return;
        }
        if columns.start <= span.start && span.end <= columns.end {
            self.apply(node, value);
            return;
        }
        self.push_down(node);
        let middle = span.start + span.len() / 2;
        self.raise(2 * node, span.start..middle, columns, value);
        self.raise(2 * node + 1, middle..span.end, columns, value);
        self.min[node] = self.min[2 * node].min(self.min[2 * node + 1]);
    }

    /// Raises all leaves below `node` to at least `value`. The minimum of
    /// max(x, value) over the leaves is max(minimum, value).
    fn apply(&mut self, node: usize, value: usize) {
        self.min[node] = self.min[node].max(value);
        if node < self.size {
            self.pending[node] = self.pending[node].max(value);
        }
    }

    fn push_down(&mut self, node: usize) {
        let value = std::mem::take(&mut self.pending[node]);
        if value > 0 {
            self.apply(2 * node, value);
            self.apply(2 * node + 1, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rowspans_occupy_columns() {
        let mut occupied = Occupancy::new(16);
        assert_eq!(occupied.first_free(0, 0), 0);
        // Row 0: a cell at column 0 spanning 2 rows, one at 1..3 spanning 3.
        occupied.occupy(0..1, 2);
        occupied.occupy(1..3, 3);
        // Row 1: columns 0 to 2 are occupied.
        assert_eq!(occupied.first_free(0, 1), 3);
        assert_eq!(occupied.first_free(5, 1), 5);
        // Row 2: only the second cell spans into it.
        assert_eq!(occupied.first_free(0, 2), 0);
        assert_eq!(occupied.first_free(1, 2), 3);
        // Row 3: all free.
        assert_eq!(occupied.first_free(1, 3), 1);
    }

    #[test]
    fn a_shorter_span_keeps_a_longer_one() {
        let mut occupied = Occupancy::new(4);
        // Column 1 until row 4, then columns 0 to 2 until row 3.
        occupied.occupy(1..2, 4);
        occupied.occupy(0..3, 3);
        assert_eq!(occupied.first_free(0, 3), 0);
        assert_eq!(occupied.first_free(1, 3), 2);
        assert_eq!(occupied.first_free(1, 4), 1);
    }

    #[test]
    fn full_rows_end_at_the_last_column() {
        let mut occupied = Occupancy::new(MAX_COLUMNS);
        occupied.occupy(0..MAX_COLUMNS, 100);
        assert_eq!(occupied.first_free(0, 1), MAX_COLUMNS);
        assert_eq!(occupied.first_free(0, 100), 0);
    }

    #[test]
    fn the_tree_has_the_size_of_the_columns() {
        assert_eq!(Occupancy::new(0).size, 1);
        assert_eq!(Occupancy::new(3).size, 4);
        assert_eq!(
            Occupancy::new(usize::MAX).size,
            MAX_COLUMNS.next_power_of_two()
        );
    }
}
