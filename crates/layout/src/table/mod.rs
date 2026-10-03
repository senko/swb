//! Table layout: the table box tree and the entry points that block,
//! inline and intrinsic layout call.
//!
//! The algorithms follow Chromium's `LayoutNG` table layout, which
//! implements CSS Tables 3 (<https://www.w3.org/TR/css-tables-3/>) where
//! CSS 2.2 §17 (<https://www.w3.org/TR/CSS22/tables.html>) leaves the
//! details open. See ADR 0010 for the deviations.
//!
//! - `build.rs`: box construction and the anonymous table object fixup.
//! - `grid.rs`: the grid positions of cells (`colspan`, `rowspan`).
//! - `columns.rs`: column constraints, the grid's min-content and
//!   max-content widths, column positions.
//! - `distribute.rs`: the distribution of a width to columns.
//! - `rows.rs`: row heights and the distribution of extra height.
//! - `cells.rs`: cell widths, cell layout, row heights from cells and
//!   vertical alignment.
//! - `layout.rs`: the table width and the steps of table layout.
//! - `fragments.rs`: the fragments of captions, columns, row groups, rows
//!   and cells.
//! - `collapsed.rs`: the collapsing border model.

mod build;
mod cells;
mod collapsed;
mod columns;
mod distribute;
mod fragments;
mod grid;
mod layout;
mod rows;

use crate::box_tree::{BoxBase, IndependentBox};

pub(crate) use build::{TableBuilder, build_table};
pub(crate) use layout::{
    TableCache, layout_block_level, layout_shrink_to_fit, layout_with_width, table_outer_sizes,
};

/// The most columns a table has. Cells that start after the last column
/// are not laid out; spans end at the last column. This bounds the memory
/// and time that hostile `colspan` and `span` values can use (Chromium has
/// no such limit).
pub(crate) const MAX_COLUMNS: usize = 10_000;

/// The largest `colspan` (HTML: 1000).
const MAX_COLSPAN: usize = 1000;

/// The largest `rowspan` (HTML: 65534). `rowspan=0` spans the rest of the
/// row group.
const MAX_ROWSPAN: usize = 65_534;

/// The contents of a table box (CSS 2.2 §17.4: the table wrapper box with
/// its captions and the table grid box).
#[derive(Debug, Default)]
pub(crate) struct TableBox {
    /// Captions in tree order.
    pub(crate) captions: Vec<IndependentBox>,
    /// Column groups and columns in tree order.
    pub(crate) columns: Vec<ColumnBox>,
    /// Row groups in layout order: the first header group, then the other
    /// groups in tree order, then the first footer group (as in Chromium).
    pub(crate) sections: Vec<SectionBox>,
}

/// A column (`display: table-column`) or a column group.
#[derive(Debug)]
pub(crate) struct ColumnBox {
    pub(crate) base: BoxBase,
    /// The number of columns: `span` of a column, or of a group without
    /// column children.
    pub(crate) span: usize,
    /// True for a column group.
    pub(crate) is_group: bool,
    /// The columns of a column group.
    pub(crate) children: Vec<ColumnBox>,
}

/// A row group (`thead`, `tbody`, `tfoot` or anonymous).
#[derive(Debug)]
pub(crate) struct SectionBox {
    pub(crate) base: BoxBase,
    pub(crate) rows: Vec<RowBox>,
    /// False for the header and footer groups that are laid out first and
    /// last (they do not take extra table height before other groups).
    pub(crate) is_body: bool,
}

/// A table row.
#[derive(Debug)]
pub(crate) struct RowBox {
    pub(crate) base: BoxBase,
    pub(crate) cells: Vec<CellBox>,
}

/// A table cell: a block container that establishes a block formatting
/// context.
#[derive(Debug)]
pub(crate) struct CellBox {
    pub(crate) inner: IndependentBox,
    /// The number of columns the cell spans, 1 to [`MAX_COLSPAN`].
    pub(crate) colspan: usize,
    /// The number of rows the cell spans, 0 (the rest of the row group) to
    /// [`MAX_ROWSPAN`].
    pub(crate) rowspan: usize,
    /// True for a `td` or `th` element with a `nowrap` attribute (for the
    /// nowrap minimum width quirk).
    pub(crate) nowrap: bool,
}

#[cfg(test)]
mod tests {
    use crate::fragment::CollapsedEdge;
    use crate::test_support::layout_html;

    fn body(html: &str) -> String {
        format!("<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>{html}")
    }

    #[test]
    fn misparented_cells_share_one_anonymous_table() {
        let l = layout_html(&body(
            "<div><span id=a style='display:table-cell'>a</span>\n  <!-- comment -->\
             <span id=b style='display:table-cell'>b</span></div>",
        ));
        let (a, b) = (l.rect("a"), l.rect("b"));
        assert_eq!(a.y, b.y);
        assert_eq!(
            a.right(),
            b.x,
            "the white space between the cells is dropped"
        );
    }

    #[test]
    fn content_of_a_table_gets_an_anonymous_cell() {
        let l = layout_html(&body(
            "<div id=t style='display:table'>text<div id=c style='display:table-cell'>cell</div></div>",
        ));
        let (t, c) = (l.rect("t"), l.rect("c"));
        assert!(c.x > t.x, "the text's anonymous cell comes first");
        assert_eq!(c.y, t.y);
    }

    #[test]
    fn header_and_footer_groups_are_moved() {
        let l = layout_html(&body(
            "<table><tfoot id=f><tr><td>f</td></tr></tfoot><tbody id=b><tr><td>b</td></tr></tbody>\
             <thead id=h><tr><td>h</td></tr></thead></table>",
        ));
        let (h, b, f) = (l.rect("h"), l.rect("b"), l.rect("f"));
        assert!(h.y < b.y && b.y < f.y);
    }

    #[test]
    fn hostile_spans_are_bounded() {
        // 100 cells that each span 1000 columns: the grid ends at
        // MAX_COLUMNS, later cells are dropped.
        let cells = "<td colspan=1000>x</td>".repeat(100);
        let columns = "<col span=1000>".repeat(100);
        let rows = "<tr><td rowspan=0>r</td></tr>".repeat(200);
        let l = layout_html(&body(&format!(
            "<table>{columns}<tr>{cells}</tr></table><table>{rows}</table>\
             <table style='table-layout:fixed; width:100px'><tr>{cells}</tr></table>"
        )));
        assert!(l.tree.scroll_size.width.is_finite());
    }

    #[test]
    fn rows_after_spans_over_all_columns_are_fast() {
        // Every column is spanned to the end of the group; later cells are
        // dropped without scanning the spans of each column.
        let spans = "<td rowspan=0>s</td>".repeat(super::MAX_COLUMNS);
        let rows = "<tr><td>x</td><td>y</td></tr>".repeat(5000);
        let l = layout_html(&body(&format!(
            "<table style='table-layout:fixed; width:1px'><tr>{spans}</tr>{rows}</table>"
        )));
        assert!(l.tree.scroll_size.height.is_finite());
    }

    #[test]
    fn huge_collapsed_grid_uses_own_borders() {
        // 10,000 columns × 200 rows: about 4 million edges, over the limit.
        let columns = "<col span=1000 style='border:1px solid'>".repeat(10);
        let rows = "<tr><td>x</td></tr>".repeat(200);
        let l = layout_html(&body(&format!(
            "<table style='border-collapse:collapse; table-layout:fixed; width:1px'>\
             {columns}{rows}</table>"
        )));
        assert_eq!(collapsed_segments(&l), vec![0]);
    }

    #[test]
    fn collapsed_border_budget_is_shared_by_the_tables_of_a_pass() {
        // 5,000 columns × 109 rows: about 1.1 million edges per table. The
        // first table uses them; the second is over the limit.
        let columns = "<col span=1000 style='border:1px solid'>".repeat(5);
        let rows = "<tr><td>x</td></tr>".repeat(109);
        let table = format!(
            "<table style='border-collapse:collapse; table-layout:fixed; width:1px'>\
             {columns}{rows}</table>"
        );
        let l = layout_html(&body(&format!("{table}{table}")));
        let segments = collapsed_segments(&l);
        assert_eq!(segments.len(), 2);
        assert!(segments[0] > 0);
        assert_eq!(segments[1], 0);
    }

    #[test]
    fn collapsed_border_segments_are_merged() {
        let row = "<tr><td>a</td><td>b</td><td>c</td></tr>";
        let l = layout_html(&body(&format!(
            "<table style='border-collapse:collapse' border=1>{row}{row}{row}</table>"
        )));
        // One segment per grid line: 4 vertical, then 4 horizontal (on top
        // at the joints).
        let edges = &collapsed_edges(&l)[0];
        let vertical: Vec<bool> = edges.iter().map(|e| e.vertical).collect();
        assert_eq!(vertical, [[true; 4], [false; 4]].concat());
    }

    #[test]
    fn collapsed_borders_stop_at_empty_row_groups() {
        let l = layout_html(&body(
            "<table id=t style='border-collapse:collapse' border=1>\
             <tbody><tr id=r><td>a</td><td>b</td></tr></tbody>\
             <tbody style='height:50px'></tbody>\
             <tbody><tr><td>c</td><td>d</td></tr></tbody></table>",
        ));
        let edges = &collapsed_edges(&l)[0];
        // Each of the 3 vertical lines in two parts, one per row group.
        assert_eq!(edges.iter().filter(|e| e.vertical).count(), 6);
        // The line between the rows is at the bottom of the first row, as
        // in Chromium.
        let horizontal: Vec<&CollapsedEdge> = edges.iter().filter(|e| !e.vertical).collect();
        assert_eq!(horizontal.len(), 3);
        let middle = horizontal[1].rect;
        let bottom = l.rect("r").bottom() - l.rect("t").y;
        assert!((middle.y + middle.height / 2.0 - bottom).abs() < 0.01);
    }

    /// The collapsed border segments of each table.
    fn collapsed_edges(l: &crate::test_support::TestLayout) -> Vec<Vec<CollapsedEdge>> {
        let mut out = Vec::new();
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Box(b) = f
                && let crate::BoxContent::Table(paint) = &b.content
                && let Some(collapsed) = &paint.collapsed
            {
                out.push(collapsed.clone());
            }
        });
        out
    }

    /// The number of collapsed border segments of each table.
    fn collapsed_segments(l: &crate::test_support::TestLayout) -> Vec<usize> {
        collapsed_edges(l).iter().map(Vec::len).collect()
    }

    #[test]
    fn nested_tables_lay_out_cells_a_bounded_number_of_times() {
        const DEPTH: usize = 30;
        let mut html = String::from("x");
        for _ in 0..DEPTH {
            html = format!(
                "<table style='height:10px'><tr><td style='height:5px'>{html}</td><td>y</td></tr>\
                 <tr><td colspan=2>z</td></tr></table>"
            );
        }
        let l = layout_html(&body(&html));
        assert!(
            l.uncached_layouts <= 8 * DEPTH,
            "{} layouts",
            l.uncached_layouts
        );
    }
}
