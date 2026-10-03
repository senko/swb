//! The collapsing border model (CSS 2.2 §17.6.2,
//! <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>), as
//! Chromium implements it (`TableBorders`, `TablePainter`).
//!
//! The grid has one edge on the left and one on the top of every slot
//! (row × column), plus the right and bottom edges of the grid. The
//! borders of cells (in grid order), rows, row groups, columns, column
//! groups and the table are merged into the edges in that order; a border
//! replaces the edge's border if it is more specific (CSS 2.2 §17.6.2.1):
//! `hidden` wins, then the wider border, then the style (`double` >
//! `solid` > `dashed` > `dotted` > `ridge` > `outset` > `groove` >
//! `inset`); on a tie the earlier source stays.
//!
//! For layout, each cell gets half of the widest border on each of its
//! sides, and the table half of the widest border on each side of the
//! grid. The table paints the borders, centered on the grid lines.

use std::sync::Arc;

use swb_style::{BorderStyle, ComputedStyle, Rgba};

use super::TableBox;
use super::grid::Grid;
use crate::fragment::CollapsedEdge;
use crate::geom::{Edges, Rect};

/// The largest number of edges that conflict resolution handles in one
/// layout pass (all tables together). Tables beyond it (hostile content)
/// use each box's own borders.
const MAX_EDGES: usize = 2_000_000;

/// The edges that conflict resolution can still use in a layout pass (see
/// [`MAX_EDGES`]).
#[derive(Default)]
pub(crate) struct EdgeBudget {
    /// The edges of the tables so far.
    used: usize,
    /// True once a table was over the limit (the warning is logged once
    /// per layout pass).
    warned: bool,
}

/// The width, style and color of a border.
type Border = (f32, BorderStyle, Rgba);

/// The positions of the grid lines for painting.
struct Lines<'a> {
    /// The x position of each column edge.
    columns: &'a [f32],
    /// The top and height of each row.
    rows: &'a [(f32, f32)],
    column_count: usize,
    row_count: usize,
}

impl Lines<'_> {
    /// The y position of the grid line above row `r`: the bottom of the
    /// row before it (also if an empty row group is between them, as in
    /// Chromium), or the top of the first row.
    fn row_line(&self, r: usize) -> f32 {
        match r.min(self.row_count).checked_sub(1) {
            Some(above) => self
                .rows
                .get(above)
                .map_or(0.0, |&(top, height)| top + height),
            None => self.rows.first().map_or(0.0, |&(top, _)| top),
        }
    }

    /// True if row `r` starts where the row before it ends (not after an
    /// empty row group with a height).
    fn touches_previous(&self, r: usize) -> bool {
        r > 0
            && r < self.row_count
            && self
                .rows
                .get(r)
                .is_some_and(|&(top, _)| (top - self.row_line(r)).abs() < 0.01)
    }
}

/// A side of a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// The border that won an edge: a style (an index into
/// [`CollapsedBorders::styles`]) and the side of that box.
#[derive(Clone, Copy, Debug, Default)]
struct Edge {
    winner: Option<(u32, Side)>,
    /// True for an edge inside a spanning cell; no border fills it.
    inside_cell: bool,
}

/// The resolved borders of a table with collapsing borders.
pub(crate) struct CollapsedBorders {
    rows: usize,
    columns: usize,
    edges: Vec<Edge>,
    styles: Vec<Arc<ComputedStyle>>,
    table: Edges,
    cells: Vec<Edges>,
}

impl CollapsedBorders {
    /// The table's border widths for layout: half of its outer borders.
    pub(crate) fn table_border(&self) -> Edges {
        self.table
    }

    /// The border widths of each cell of the grid for layout: half of the
    /// borders on its edges.
    pub(crate) fn cell_borders(&self) -> Vec<Edges> {
        self.cells.clone()
    }

    /// The border segments to paint, relative to the table's border box.
    /// `columns` are the x positions of the column edges (one more than
    /// columns), `rows` the top and height of each row. Consecutive edges
    /// of a grid line with the same border are one segment. Vertical
    /// segments come first: horizontal ones are on top at the joints.
    pub(crate) fn paint(&self, columns: &[f32], rows: &[(f32, f32)]) -> Vec<CollapsedEdge> {
        let mut out = Vec::new();
        if self.edges.is_empty() || columns.is_empty() {
            return out;
        }
        // The grid of layout has the same size; if not, paint what both
        // have.
        let lines = Lines {
            columns,
            rows,
            column_count: self.columns.min(columns.len() - 1),
            row_count: self.rows.min(rows.len()),
        };
        for c in 0..=lines.column_count {
            self.paint_column_line(&lines, c, &mut out);
        }
        for r in 0..=lines.row_count {
            self.paint_row_line(&lines, r, &mut out);
        }
        out
    }

    /// The horizontal segments on the top of row `r`, extended over the
    /// joints with the widest crossing vertical border.
    fn paint_row_line(&self, lines: &Lines<'_>, r: usize, out: &mut Vec<CollapsedEdge>) {
        let y = lines.row_line(r);
        let joint = |column: usize| {
            let above = r
                .checked_sub(1)
                .map_or(0.0, |a| self.width(a, column, false));
            let below = if r < lines.row_count {
                self.width(r, column, false)
            } else {
                0.0
            };
            above.max(below) / 2.0
        };
        let mut run: Option<(usize, Border)> = None;
        for c in 0..=lines.column_count {
            let border = if c < lines.column_count {
                self.paintable(r, c, true)
            } else {
                None
            };
            if let Some((start, (width, style, color))) = run
                && border != run.map(|r| r.1)
            {
                let left = lines.columns[start] - joint(start);
                let right = lines.columns[c] + joint(c);
                if right > left {
                    out.push(CollapsedEdge {
                        rect: Rect::new(left, y - width / 2.0, right - left, width),
                        vertical: false,
                        style,
                        color,
                    });
                }
                run = None;
            }
            if run.is_none() {
                run = border.map(|b| (c, b));
            }
        }
    }

    /// The vertical segments on the left of column `c`. A segment ends
    /// where an empty row group leaves a gap between rows (Chromium paints
    /// the edges of each row group separately).
    fn paint_column_line(&self, lines: &Lines<'_>, c: usize, out: &mut Vec<CollapsedEdge>) {
        let x = lines.columns[c];
        let mut run: Option<(usize, Border)> = None;
        for r in 0..=lines.row_count {
            let border = if r < lines.row_count {
                self.paintable(r, c, false)
            } else {
                None
            };
            if let Some((start, (width, style, color))) = run
                && (border != run.map(|r| r.1) || !lines.touches_previous(r))
            {
                let top = lines.rows[start].0;
                let bottom = lines.row_line(r);
                if bottom > top {
                    out.push(CollapsedEdge {
                        rect: Rect::new(x - width / 2.0, top, width, bottom - top),
                        vertical: true,
                        style,
                        color,
                    });
                }
                run = None;
            }
            if run.is_none() {
                run = border.map(|b| (r, b));
            }
        }
    }

    fn index(&self, row: usize, column: usize, horizontal: bool) -> usize {
        (row * (self.columns + 1) + column) * 2 + usize::from(horizontal)
    }

    /// The width, style and color of an edge, if it has a border that is
    /// painted.
    fn paintable(&self, row: usize, column: usize, horizontal: bool) -> Option<Border> {
        let edge = self.edges.get(self.index(row, column, horizontal))?;
        let (style, side) = edge.winner?;
        let style = self.styles.get(style as usize)?;
        let (width, border_style) = border(style, side);
        if width <= 0.0 || matches!(border_style, BorderStyle::None | BorderStyle::Hidden) {
            return None;
        }
        Some((width, border_style, color(style, side)))
    }

    /// The paintable width of an edge (0 without a border).
    fn width(&self, row: usize, column: usize, horizontal: bool) -> f32 {
        self.paintable(row, column, horizontal).map_or(0.0, |e| e.0)
    }

    /// Half of the widest border on each side of an area of the grid.
    fn half_borders(&self, row: usize, column: usize, rowspan: usize, colspan: usize) -> Edges {
        let mut edges = Edges::ZERO;
        for r in row..(row + rowspan).min(self.rows) {
            edges.left = edges.left.max(self.width(r, column, false));
            edges.right = edges.right.max(self.width(r, column + colspan, false));
        }
        for c in column..(column + colspan).min(self.columns) {
            edges.top = edges.top.max(self.width(row, c, true));
            edges.bottom = edges.bottom.max(self.width(row + rowspan, c, true));
        }
        Edges::new(
            edges.top / 2.0,
            edges.right / 2.0,
            edges.bottom / 2.0,
            edges.left / 2.0,
        )
    }

    /// Merges the borders of a box that covers an area of the grid.
    fn merge(
        &mut self,
        area: (usize, usize, usize, usize),
        style: &Arc<ComputedStyle>,
        is_cell: bool,
    ) {
        let (row, column, rowspan, colspan) = area;
        let rowspan = rowspan.min(self.rows.saturating_sub(row));
        let colspan = colspan.min(self.columns.saturating_sub(column));
        let sides = [Side::Top, Side::Right, Side::Bottom, Side::Left];
        if rowspan == 0
            || colspan == 0
            || (!is_cell
                && sides
                    .iter()
                    .all(|&side| border(style, side).1 == BorderStyle::None))
        {
            return;
        }
        let index = u32::try_from(self.styles.len()).unwrap_or(u32::MAX);
        self.styles.push(Arc::clone(style));
        for c in column..column + colspan {
            self.merge_edge(self.index(row, c, true), style, index, Side::Top);
            self.merge_edge(
                self.index(row + rowspan, c, true),
                style,
                index,
                Side::Bottom,
            );
        }
        for r in row..row + rowspan {
            self.merge_edge(self.index(r, column, false), style, index, Side::Left);
            self.merge_edge(
                self.index(r, column + colspan, false),
                style,
                index,
                Side::Right,
            );
        }
        if is_cell && (rowspan > 1 || colspan > 1) {
            self.mark_inside(row, column, rowspan, colspan);
        }
    }

    fn merge_edge(&mut self, index: usize, style: &ComputedStyle, style_index: u32, side: Side) {
        let (width, border_style) = border(style, side);
        if border_style == BorderStyle::None {
            return;
        }
        let current = match self.edges.get(index) {
            Some(edge) if edge.inside_cell => return,
            Some(edge) => edge
                .winner
                .and_then(|(s, side)| self.styles.get(s as usize).map(|style| border(style, side))),
            None => return,
        };
        if is_more_specific(width, border_style, current)
            && let Some(edge) = self.edges.get_mut(index)
        {
            edge.winner = Some((style_index, side));
        }
    }

    /// Marks the edges inside a spanning cell, so that no later border
    /// fills them.
    fn mark_inside(&mut self, row: usize, column: usize, rowspan: usize, colspan: usize) {
        for r in row..row + rowspan {
            for c in column..column + colspan {
                if c > column {
                    let i = self.index(r, c, false);
                    if self.edges[i].winner.is_none() {
                        self.edges[i].inside_cell = true;
                    }
                }
                if r > row {
                    let i = self.index(r, c, true);
                    if self.edges[i].winner.is_none() {
                        self.edges[i].inside_cell = true;
                    }
                }
            }
        }
    }
}

/// True if a border with `width` and `style` replaces the edge's current
/// border (Chromium's `IsSourceMoreSpecificThanEdge`).
fn is_more_specific(width: f32, style: BorderStyle, current: Option<(f32, BorderStyle)>) -> bool {
    let Some((current_width, current_style)) = current else {
        return true;
    };
    if style == BorderStyle::Hidden {
        return true;
    }
    if current_style == BorderStyle::Hidden {
        return false;
    }
    if width != current_width {
        return width > current_width;
    }
    rank(style) > rank(current_style)
}

/// The precedence of border styles in conflict resolution.
fn rank(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::None => 0,
        BorderStyle::Hidden => 1,
        BorderStyle::Inset => 2,
        BorderStyle::Groove => 3,
        BorderStyle::Outset => 4,
        BorderStyle::Ridge => 5,
        BorderStyle::Dotted => 6,
        BorderStyle::Dashed => 7,
        BorderStyle::Solid => 8,
        BorderStyle::Double => 9,
    }
}

/// The width and style of one side of a box's border. In the collapsing
/// model `inset` acts as `ridge` and `outset` as `groove` (CSS 2.2
/// §17.6.2).
fn border(style: &ComputedStyle, side: Side) -> (f32, BorderStyle) {
    let (width, border_style) = match side {
        Side::Top => (style.border_top_width, style.border_top_style),
        Side::Right => (style.border_right_width, style.border_right_style),
        Side::Bottom => (style.border_bottom_width, style.border_bottom_style),
        Side::Left => (style.border_left_width, style.border_left_style),
    };
    let border_style = match border_style {
        BorderStyle::Inset => BorderStyle::Ridge,
        BorderStyle::Outset => BorderStyle::Groove,
        other => other,
    };
    (width, border_style)
}

fn color(style: &ComputedStyle, side: Side) -> Rgba {
    let color = match side {
        Side::Top => style.border_top_color,
        Side::Right => style.border_right_color,
        Side::Bottom => style.border_bottom_color,
        Side::Left => style.border_left_color,
    };
    color.resolve(style.color)
}

/// The number of columns of the grid: the columns that cells start in
/// (span, in fixed layout) and the columns up to the last column element
/// that takes space.
fn column_count(table: &TableBox, grid: &Grid<'_>, fixed: bool) -> usize {
    let cells = grid
        .cells
        .iter()
        .map(|c| {
            if fixed {
                c.column + c.colspan
            } else {
                c.column + 1
            }
        })
        .max()
        .unwrap_or(0);
    cells.max(super::columns::element_column_count(&table.columns, fixed))
}

/// Resolves the borders of a table with `border-collapse: collapse`.
/// `budget` counts the edges of the layout pass.
pub(crate) fn compute(
    style: &Arc<ComputedStyle>,
    table: &TableBox,
    grid: &Grid<'_>,
    fixed: bool,
    budget: &mut EdgeBudget,
) -> CollapsedBorders {
    let rows = grid.rows.len();
    let columns = column_count(table, grid, fixed);
    let edge_count = (rows + 1).saturating_mul(columns + 1).saturating_mul(2);
    if edge_count > MAX_EDGES.saturating_sub(budget.used) {
        if !budget.warned {
            budget.warned = true;
            log::warn!(
                "collapsed borders of tables with more than {MAX_EDGES} edges in total \
                 are not resolved; their cells use their own borders"
            );
        }
        return own_borders(style, grid, rows, columns);
    }
    budget.used += edge_count;
    let mut borders = CollapsedBorders {
        rows,
        columns,
        edges: vec![Edge::default(); edge_count],
        styles: Vec::new(),
        table: Edges::ZERO,
        cells: Vec::new(),
    };
    for cell in &grid.cells {
        let area = (cell.row, cell.column, cell.rowspan, cell.colspan);
        borders.merge(area, &cell.cell.inner.base.style, true);
    }
    for (index, row) in grid.rows.iter().enumerate() {
        borders.merge((index, 0, 1, columns), &row.row.base.style, false);
    }
    for section in &grid.sections {
        let area = (section.rows.start, 0, section.rows.len(), columns);
        borders.merge(area, &section.section.base.style, false);
    }
    merge_columns(&mut borders, table, rows, columns);
    borders.merge((0, 0, rows, columns), style, false);
    borders.table = borders.half_borders(0, 0, rows, columns);
    borders.cells = grid
        .cells
        .iter()
        .map(|c| borders.half_borders(c.row, c.column, c.rowspan, c.colspan))
        .collect();
    borders
}

/// Merges the borders of columns, then of column groups.
fn merge_columns(borders: &mut CollapsedBorders, table: &TableBox, rows: usize, columns: usize) {
    let mut groups = Vec::new();
    let mut index = 0;
    for column in &table.columns {
        if index >= columns {
            break;
        }
        if !column.is_group {
            let span = column.span.min(columns - index);
            for c in index..index + span {
                borders.merge((0, c, rows, 1), &column.base.style, false);
            }
            index += span;
            continue;
        }
        let start = index;
        if column.children.is_empty() {
            index += column.span.min(columns - index);
        }
        for child in &column.children {
            let span = child.span.min(columns.saturating_sub(index));
            for c in index..index + span {
                borders.merge((0, c, rows, 1), &child.base.style, false);
            }
            index += span;
        }
        groups.push((start, index - start, &column.base.style));
    }
    for (start, span, style) in groups {
        borders.merge((0, start, rows, span), style, false);
    }
}

/// The fallback for very large grids: every box gets half of its own
/// borders, and none are painted.
fn own_borders(
    style: &ComputedStyle,
    grid: &Grid<'_>,
    rows: usize,
    columns: usize,
) -> CollapsedBorders {
    let half = |s: &ComputedStyle| {
        Edges::new(
            s.border_top_width / 2.0,
            s.border_right_width / 2.0,
            s.border_bottom_width / 2.0,
            s.border_left_width / 2.0,
        )
    };
    CollapsedBorders {
        rows,
        columns,
        edges: Vec::new(),
        styles: Vec::new(),
        table: half(style),
        cells: grid
            .cells
            .iter()
            .map(|c| half(&c.cell.inner.base.style))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_resolution() {
        let solid = BorderStyle::Solid;
        assert!(is_more_specific(1.0, solid, None));
        assert!(is_more_specific(
            3.0,
            solid,
            Some((2.0, BorderStyle::Double))
        ));
        assert!(!is_more_specific(
            1.0,
            BorderStyle::Double,
            Some((2.0, solid))
        ));
        assert!(is_more_specific(
            2.0,
            BorderStyle::Double,
            Some((2.0, solid))
        ));
        assert!(!is_more_specific(2.0, solid, Some((2.0, solid))));
        assert!(is_more_specific(
            0.0,
            BorderStyle::Hidden,
            Some((5.0, solid))
        ));
        assert!(!is_more_specific(
            9.0,
            solid,
            Some((0.0, BorderStyle::Hidden))
        ));
    }
}
