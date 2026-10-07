//! Table layout: the table width, column widths and row heights, and the
//! entry points that block, inline and intrinsic layout call.
//!
//! The steps follow Chromium's `TableLayoutAlgorithm`
//! (CSS Tables 3 §3.2–§3.10, <https://www.w3.org/TR/css-tables-3/#table-layout-algorithm>):
//!
//! 1. Column constraints from columns and cells (`columns.rs`), cached per
//!    table for the layout pass.
//! 2. The table width: specified, or fit-content between the grid's
//!    min-content and max-content widths; never less than the grid's
//!    min-content width or the captions' min-content widths.
//! 3. Column widths: the assignable width (without border, padding and
//!    border spacing) distributed to the columns.
//! 4. Row heights: every cell laid out at its width; spanning cells, row
//!    group heights and the table height distributed to rows (`rows.rs`).
//! 5. Fragments (`fragments.rs`). The table's border box contains its
//!    captions (as in Chromium); its border and background are painted
//!    around the grid.

use std::collections::HashMap;
use std::rc::Rc;

use swb_style::{BorderCollapse, BoxSizing, ComputedStyle, LengthPercentage, Size, TableLayout};

use super::TableBox;
use super::cells::{cell_edges, cell_widths, fixed_or_percent, layout_cell, row_data};
use super::collapsed::{self, CollapsedBorders, EdgeBudget};
use super::columns::{
    Column, ColumnLocation, TABLE_MAX_WIDTH, column_constraints, column_locations, grid_min_max,
    outer_size, sizes_from_style, undistributable_space,
};
use super::distribute::{distribute_auto, distribute_fixed};
use super::fragments::{Assembly, assemble};
use super::grid::{Grid, GridSection, Placement, place};
use super::rows::{
    RowData, SectionData, distribute_rowspan_cell, distribute_table_height, distribute_to_rows,
    sort_rowspan_cells,
};
use crate::LayoutContext;
use crate::block::{
    BoxEdges, ContainingBlock, LaidOutBlock, margin_or_zero, own_margins, resolve_max_size,
    resolve_size,
};
use crate::box_tree::IndependentBox;
use crate::fragment::BoxFragment;
use crate::geom::{Edges, clamp_length};
use crate::intrinsic::{ContentSizes, fixed_margins, independent_outer_sizes};

/// Table data that is kept for one layout pass.
#[derive(Default)]
pub(crate) struct TableCache {
    /// Grid positions of the cells per table box (by box number).
    placements: HashMap<usize, Rc<Vec<Placement>>>,
    /// True once a table of the layout pass dropped cells (the warning is
    /// logged once).
    dropped_cells: bool,
    /// Resolved collapsing borders per table box.
    collapsed: HashMap<usize, Rc<CollapsedBorders>>,
    /// The edges of all collapsing-border grids of the layout pass.
    collapsed_edges: EdgeBudget,
    /// Column constraints per table box.
    columns: HashMap<usize, Rc<Vec<Column>>>,
    /// The number of table cells, flex containers and grid containers
    /// that are being measured or laid out. The intrinsic widths of tables
    /// inside them do not use column percentages (Chromium's
    /// `AllowColumnPercentages`).
    percent_free_depth: usize,
}

impl TableCache {
    /// Runs `f` inside a table cell, a flex container or a grid container
    /// (see [`TableCache::percent_free_depth`]).
    pub(crate) fn percent_free<T>(
        ctx: &mut LayoutContext<'_>,
        f: impl FnOnce(&mut LayoutContext<'_>) -> T,
    ) -> T {
        ctx.tables.percent_free_depth += 1;
        let result = f(ctx);
        ctx.tables.percent_free_depth -= 1;
        result
    }
}

/// Lays out a block-level table in normal flow, in a space `available` px
/// wide (the containing block's width, or a layout opportunity next to
/// floats with the table's margins): its width from its content, its
/// horizontal position from its margins. The fragment's x is its left
/// margin.
pub(crate) fn layout_block_level(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    cb: ContainingBlock,
    available: f32,
) -> LaidOutBlock {
    let mut laid_out = layout_in(ctx, ib, table, cb, available);
    let style = &ib.base.style;
    let width = laid_out.fragment.border_rect.width;
    let remaining = available - width;
    let margin_left = match (
        style.margin_left.resolve(cb.width),
        style.margin_right.resolve(cb.width),
    ) {
        (None, None) => (remaining / 2.0).max(0.0),
        (None, Some(right)) => (remaining - right).max(0.0),
        (Some(left), _) => left,
    };
    laid_out.fragment.border_rect.x = margin_left;
    laid_out
}

/// Lays out a table whose width comes from its content and the available
/// width (inline tables, floats, absolutely positioned tables). Auto
/// margins are 0. The fragment's x is its left margin.
pub(crate) fn layout_shrink_to_fit(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    cb: ContainingBlock,
) -> LaidOutBlock {
    layout_in(ctx, ib, table, cb, cb.width)
}

/// [`layout_shrink_to_fit`] in a space `available` px wide; percentages
/// still resolve against `cb`.
fn layout_in(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    cb: ContainingBlock,
    available: f32,
) -> LaidOutBlock {
    let style = &ib.base.style;
    let margin_left = margin_or_zero(&style.margin_left, cb.width);
    let available = available - margin_left - margin_or_zero(&style.margin_right, cb.width);
    let mut fragment = layout_table(ctx, ib, table, TableWidth::Available(available), None, cb);
    fragment.border_rect.x = margin_left;
    LaidOutBlock {
        fragment,
        margins: own_margins(style, cb),
    }
}

/// Lays out a table with a given border-box width and, optionally, a
/// border-box height (flex items). The fragment is at (0, 0).
pub(crate) fn layout_with_width(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    width: f32,
    height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    layout_table(ctx, ib, table, TableWidth::Fixed(width), height, cb)
}

/// The min-content and max-content contributions of a table (margin
/// box), for intrinsic sizing of its container. A specified width counts,
/// but not less than the table's min-content width.
pub(crate) fn table_outer_sizes(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
) -> ContentSizes {
    let style = &ib.base.style;
    let prepared = Prepared::new(ctx, ib, table, 0.0);
    let caption = caption_min_width(ctx, table);
    let intrinsic = prepared.intrinsic_sizes(ctx, style, caption, false);
    let border_padding = prepared.edges.sum().horizontal();
    let (width, min_width, max_width, _) = sizes_from_style(style, border_padding);
    let (mut min, mut max) = width.map_or((intrinsic.min, intrinsic.max), |w| (w, w));
    if let Some(max_width) = max_width {
        (min, max) = (min.min(max_width), max.min(max_width));
    }
    if let Some(min_width) = min_width {
        (min, max) = (min.max(min_width), max.max(min_width));
    }
    let margins = fixed_margins(style);
    ContentSizes {
        min: min.max(intrinsic.min) + margins,
        max: max.max(intrinsic.min) + margins,
    }
}

/// How the table's width is decided.
#[derive(Clone, Copy)]
enum TableWidth {
    /// The border-box width is given.
    Fixed(f32),
    /// The width comes from the table, with this much space available.
    Available(f32),
}

/// What all steps of table layout need: the grid, the borders and the
/// column constraints.
pub(super) struct Prepared<'a> {
    pub(super) grid: Grid<'a>,
    /// The table's border and padding.
    pub(super) edges: BoxEdges,
    /// The border of each cell of the grid.
    cell_borders: Vec<Edges>,
    pub(super) collapsed: Option<Rc<CollapsedBorders>>,
    /// True if the table has a specified height (cells that grow then have
    /// a definite height).
    pub(super) height_specified: bool,
    /// Horizontal and vertical border spacing. Placement uses
    /// [`Geometry::spacing`], which is 0 without columns (as in Chromium).
    pub(super) spacing: (f32, f32),
    fixed: bool,
    columns: Rc<Vec<Column>>,
}

impl<'a> Prepared<'a> {
    fn new(
        ctx: &mut LayoutContext<'_>,
        ib: &IndependentBox,
        table: &'a TableBox,
        cb_width: f32,
    ) -> Self {
        let style = &ib.base.style;
        let id = ib.base.id;
        let tables = &mut ctx.tables;
        let placements = Rc::clone(
            tables
                .placements
                .entry(id)
                .or_insert_with(|| Rc::new(place(table, &mut tables.dropped_cells))),
        );
        let grid = Grid::new(table, &placements);
        let fixed = style.table_layout == TableLayout::Fixed && !style.width.is_auto();
        let (edges, cell_borders, collapsed, spacing) =
            if style.border_collapse == BorderCollapse::Collapse {
                let collapsed = if let Some(collapsed) = ctx.tables.collapsed.get(&id) {
                    Rc::clone(collapsed)
                } else {
                    let used = &mut ctx.tables.collapsed_edges;
                    let collapsed = Rc::new(collapsed::compute(style, table, &grid, fixed, used));
                    ctx.tables.collapsed.insert(id, Rc::clone(&collapsed));
                    collapsed
                };
                let edges = BoxEdges {
                    padding: Edges::ZERO,
                    border: collapsed.table_border(),
                };
                let cells = collapsed.cell_borders();
                (edges, cells, Some(collapsed), (0.0, 0.0))
            } else {
                let edges = BoxEdges::resolve(style, cb_width);
                let cells = grid
                    .cells
                    .iter()
                    .map(|c| BoxEdges::resolve(&c.cell.inner.base.style, 0.0).border)
                    .collect();
                let spacing = (
                    style.border_spacing_horizontal,
                    style.border_spacing_vertical,
                );
                (edges, cells, None, spacing)
            };
        let columns = if let Some(columns) = ctx.tables.columns.get(&ib.base.id) {
            Rc::clone(columns)
        } else {
            let columns = Rc::new(column_constraints(
                &table.columns,
                &grid,
                fixed,
                spacing.0,
                &mut |i| cell_widths(ctx, &grid.cells[i], cell_borders[i], fixed),
            ));
            ctx.tables.columns.insert(ib.base.id, Rc::clone(&columns));
            columns
        };
        Prepared {
            height_specified: !style.height.is_auto(),
            grid,
            edges,
            cell_borders,
            collapsed,
            spacing,
            fixed,
            columns,
        }
    }

    fn undistributable(&self) -> f32 {
        undistributable_space(&self.columns, self.edges.sum().horizontal(), self.spacing.0)
    }

    /// The table's min-content and max-content widths (border box), with
    /// `caption` the widest caption min-content width. In the table's own
    /// layout (`layout_pass`), column percentages always count; for its
    /// intrinsic widths, not inside cells and flex containers.
    fn intrinsic_sizes(
        &self,
        ctx: &LayoutContext<'_>,
        style: &ComputedStyle,
        caption: f32,
        layout_pass: bool,
    ) -> ContentSizes {
        let allow_percent = layout_pass || ctx.tables.percent_free_depth == 0;
        let grid = grid_min_max(
            &self.columns,
            self.undistributable(),
            self.fixed,
            allow_percent,
        );
        let percent_width = style
            .width
            .as_length_percentage()
            .is_some_and(LengthPercentage::has_percentage);
        let max = if self.fixed && percent_width {
            TABLE_MAX_WIDTH
        } else {
            grid.max.max(caption)
        };
        ContentSizes {
            min: grid.min.max(caption),
            max,
        }
    }
}

/// The largest min-content contribution of the captions.
fn caption_min_width(ctx: &mut LayoutContext<'_>, table: &TableBox) -> f32 {
    table
        .captions
        .iter()
        .map(|caption| independent_outer_sizes(ctx, caption).min)
        .fold(0.0, f32::max)
}

/// The used border-box width of a table whose width is not given
/// (CSS Tables 3 §3.9.1,
/// <https://www.w3.org/TR/css-tables-3/#computing-the-table-width>;
/// Chromium's `ComputeUsedInlineSizeForTableFragment`).
fn used_width(
    style: &ComputedStyle,
    edges: &BoxEdges,
    grid: ContentSizes,
    available: f32,
    cb: ContainingBlock,
) -> f32 {
    let border_padding = edges.sum().horizontal();
    let outer = |v: f32| outer_size(style, v, border_padding);
    let fit_content = grid.max.min(available.max(grid.min));
    let width = match &style.width {
        Size::LengthPercentage(lp) => outer(lp.resolve(cb.width)),
        Size::MinContent => grid.min,
        Size::MaxContent => grid.max,
        Size::Auto | Size::FitContent(_) => fit_content,
    };
    let max = resolve_max_size(&style.max_width, Some(cb.width), BoxSizing::ContentBox, 0.0);
    let min = resolve_size(&style.min_width, Some(cb.width), BoxSizing::ContentBox, 0.0);
    let width = max.map_or(width, |m| width.min(outer(m)));
    let width = min.map_or(width, |m| width.max(outer(m)));
    width.max(grid.min)
}

/// The width of a table without columns (Chromium's
/// `ComputeEmptyTableInlineSize`).
fn empty_table_width(
    style: &ComputedStyle,
    assignable: f32,
    undistributable: f32,
    caption_min: f32,
    edges: &BoxEdges,
    collapsed: bool,
) -> f32 {
    if !style.width.is_auto() || !style.min_width.is_auto() {
        return assignable + undistributable;
    }
    if caption_min > 0.0 {
        return caption_min.max(edges.sum().horizontal());
    }
    if collapsed {
        return 0.0;
    }
    assignable + edges.sum().horizontal()
}

/// Lays out a table. The fragment is at (0, 0).
fn layout_table(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    width: TableWidth,
    height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let style = &ib.base.style;
    let prepared = Prepared::new(ctx, ib, table, cb.width);
    let (locations, table_width) = column_widths(ctx, &prepared, style, table, width, cb);
    let spacing = if locations.is_empty() {
        (0.0, 0.0)
    } else {
        prepared.spacing
    };
    let section_width =
        (table_width - prepared.edges.sum().horizontal() - 2.0 * spacing.0).max(0.0);
    let geometry = Geometry {
        locations: &locations,
        spacing,
        section_width,
    };
    let mut measured = measure_rows(ctx, &prepared, &geometry);
    let css_height = table_height(style, &prepared, height, cb, ctx.quirks);
    if let Some(css_height) = css_height {
        // The height of the grid without the table's border and padding.
        let available = (css_height - prepared.edges.sum().vertical()).max(0.0);
        distribute_table_height(
            prepared.spacing.1,
            available,
            &mut measured.sections,
            &mut measured.rows,
        );
    }
    let parts = Assembly {
        prepared: &prepared,
        geometry: &geometry,
        measured,
        css_height,
        table_width,
    };
    assemble(ctx, ib, table, &parts)
}

/// The column positions and the table's border-box width: the
/// assignable width (without border, padding and border spacing)
/// distributed to the columns.
fn column_widths(
    ctx: &mut LayoutContext<'_>,
    prepared: &Prepared<'_>,
    style: &ComputedStyle,
    table: &TableBox,
    width: TableWidth,
    cb: ContainingBlock,
) -> (Vec<ColumnLocation>, f32) {
    let caption_min = caption_min_width(ctx, table);
    let undistributable = prepared.undistributable();
    let assignable = match width {
        TableWidth::Fixed(w) => (w - undistributable).max(0.0),
        TableWidth::Available(available) => {
            let grid = prepared.intrinsic_sizes(ctx, style, caption_min, true);
            let used = used_width(style, &prepared.edges, grid, available, cb).max(caption_min);
            (used - undistributable).max(0.0)
        }
    };
    let columns = &prepared.columns;
    let sizes = if prepared.fixed {
        distribute_fixed(assignable, columns)
    } else {
        distribute_auto(assignable, columns, true)
    };
    let locations = column_locations(columns, &sizes, prepared.spacing.0);
    let from_columns = match locations.last() {
        Some(last) => {
            last.offset + last.size + prepared.edges.sum().horizontal() + prepared.spacing.0
        }
        None => empty_table_width(
            style,
            assignable,
            undistributable,
            caption_min,
            &prepared.edges,
            prepared.collapsed.is_some(),
        ),
    };
    let table_width = clamp_length(match width {
        TableWidth::Fixed(w) => w,
        TableWidth::Available(_) => from_columns.max(caption_min),
    });
    (locations, table_width)
}

/// Column positions and the size of row groups.
pub(super) struct Geometry<'a> {
    pub(super) locations: &'a [ColumnLocation],
    /// Border spacing (0 without columns).
    pub(super) spacing: (f32, f32),
    /// The width of row groups and rows.
    pub(super) section_width: f32,
}

impl Geometry<'_> {
    /// The x position (relative to a row) and the width of a cell.
    pub(super) fn cell_span(&self, column: usize, colspan: usize) -> (f32, f32) {
        let Some(first) = self.locations.get(column) else {
            return (0.0, 0.0);
        };
        let last_index = (column + colspan - 1).min(self.locations.len() - 1);
        let last = self.locations[last_index];
        (
            first.offset - self.spacing.0,
            last.offset + last.size - first.offset,
        )
    }
}

/// The measured cells and the row and row group heights.
pub(super) struct Measured {
    /// The fragment and edges of every cell of the grid (measured).
    pub(super) cells: Vec<(BoxFragment, BoxEdges)>,
    pub(super) rows: Vec<RowData>,
    pub(super) sections: Vec<SectionData>,
}

/// Lays out every cell at its width and computes the row heights
/// (Chromium's `ComputeSectionMinimumRowBlockSizes`).
fn measure_rows(
    ctx: &mut LayoutContext<'_>,
    prepared: &Prepared<'_>,
    geometry: &Geometry<'_>,
) -> Measured {
    let grid = &prepared.grid;
    let cells: Vec<(BoxFragment, BoxEdges)> = grid
        .cells
        .iter()
        .zip(&prepared.cell_borders)
        .map(|(cell, &border)| {
            let style = &cell.cell.inner.base.style;
            let edges = cell_edges(style, border, geometry.section_width);
            let (_, width) = geometry.cell_span(cell.column, cell.colspan);
            let fragment = layout_cell(ctx, cell, &edges, width, geometry.section_width, None);
            (fragment, edges)
        })
        .collect();
    let spacing = prepared.spacing.1;
    let mut rows = Vec::with_capacity(grid.rows.len());
    let mut sections = Vec::with_capacity(grid.sections.len());
    for section in &grid.sections {
        let mut spanning = Vec::new();
        let mut percent_total = 0.0;
        for index in section.rows.clone() {
            let row = &grid.rows[index];
            let row_cells: Vec<_> = row
                .cells
                .clone()
                .map(|i| (&grid.cells[i], &cells[i].0, &cells[i].1))
                .collect();
            let style = &row.row.base.style;
            let mut data = row_data(style, index, &row_cells, ctx.quirks, &mut spanning);
            // The percentages of a group's rows add up to at most 100%.
            if let Some(p) = data.percent {
                let p = p.min(100.0 - percent_total);
                data.percent = Some(p);
                percent_total += p;
            }
            rows.push(data);
        }
        sort_rowspan_cells(&mut spanning);
        for cell in &spanning {
            distribute_rowspan_cell(cell, spacing, &mut rows);
        }
        sections.push(section_data(section, &mut rows, spacing));
    }
    Measured {
        cells,
        rows,
        sections,
    }
}

/// The height constraints of a row group from its rows; its specified
/// height (if larger) is distributed to the rows.
fn section_data(section: &GridSection<'_>, rows: &mut [RowData], spacing: f32) -> SectionData {
    let style = &section.section.base.style;
    let (fixed_height, percent) = fixed_or_percent(&style.height);
    let section_rows = rows.get_mut(section.rows.clone()).unwrap_or_default();
    let count = section_rows.len();
    let mut height: f32 = section_rows.iter().map(|r| r.height).sum::<f32>()
        + spacing * count.saturating_sub(1) as f32;
    if let Some(fixed_height) = fixed_height
        && fixed_height > height
    {
        distribute_to_rows(
            section_rows,
            fixed_height,
            false,
            spacing,
            Some(fixed_height),
        );
        height = fixed_height;
    }
    SectionData {
        rows: section.rows.clone(),
        height,
        percent,
        constrained: fixed_height.is_some() || percent.is_some(),
        is_body: section.section.is_body,
        needs_redistribution: false,
    }
}

/// The table's specified border-box height, if any (`forced` from a flex
/// container wins; CSS Tables 3 §3.10.1,
/// <https://www.w3.org/TR/css-tables-3/#computing-the-table-height>). In
/// quirks mode a table without row groups ignores its height.
fn table_height(
    style: &ComputedStyle,
    prepared: &Prepared<'_>,
    forced: Option<f32>,
    cb: ContainingBlock,
    quirks: bool,
) -> Option<f32> {
    if quirks && prepared.grid.sections.is_empty() {
        return None;
    }
    if forced.is_some() {
        return forced;
    }
    let border_padding = prepared.edges.sum().vertical();
    let outer = |v: f32| outer_size(style, v, border_padding);
    let min = resolve_size(&style.min_height, cb.height, BoxSizing::ContentBox, 0.0).map(outer);
    let height = style
        .height
        .as_length_percentage()
        .and_then(|lp| lp.resolve_opt(cb.height))
        .map(outer);
    let Some(height) = height else {
        // An automatic height: the `min-height` is the grid's height, if
        // it is definite (Chromium's `ComputeRows`).
        return min;
    };
    let max = resolve_max_size(&style.max_height, cb.height, BoxSizing::ContentBox, 0.0);
    let height = max.map_or(height, |m| height.min(outer(m)));
    Some(min.map_or(height, |m| height.max(m)))
}
