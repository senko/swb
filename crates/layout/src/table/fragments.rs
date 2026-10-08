//! The fragments of a laid-out table: captions, column groups and columns,
//! row groups, rows and cells, and the paint data of cells and the table.
//!
//! Positions follow Chromium's `TableLayoutAlgorithm::GenerateFragment`:
//! top captions, the table's border and padding, then each row group after
//! one border spacing (a group without rows takes no spacing), one border
//! spacing and the border and padding after the last group, then the
//! bottom captions. A specified table height is a minimum for the grid.
//! Column groups and columns cover the area of their cells.

use std::sync::Arc;

use swb_style::{CaptionSide, ComputedStyle, EmptyCells};

use super::cells::{align_cell, has_content, has_definite_height, layout_cell};
use super::columns::ColumnLocation;
use super::layout::{Geometry, Measured, Prepared};
use super::{ColumnBox, TableBox};
use crate::block::{
    Baselines, BoxEdges, ContainingBlock, apply_relative_position, finish_fragment,
    layout_independent_block_level,
};
use crate::box_tree::IndependentBox;
use crate::fragment::{BoxContent, BoxFragment, CellPaint, Fragment, PartBackground, TablePaint};
use crate::geom::{Rect, clamp_length};
use crate::{LayoutContext, has_background};

/// The results of the layout steps that the fragments are built from.
pub(super) struct Assembly<'a, 'b> {
    pub(super) prepared: &'a Prepared<'b>,
    pub(super) geometry: &'a Geometry<'a>,
    pub(super) measured: Measured,
    /// The specified border-box height of the table.
    pub(super) css_height: Option<f32>,
    pub(super) table_width: f32,
}

/// Builds the table fragment at (0, 0).
pub(super) fn assemble(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    table: &TableBox,
    parts: &Assembly<'_, '_>,
) -> BoxFragment {
    let (prepared, geometry) = (parts.prepared, parts.geometry);
    let edges = prepared.edges.sum();
    let caption_cb = ContainingBlock {
        width: parts.table_width,
        height: None,
    };
    let mut children = Vec::new();
    let mut y = 0.0;
    place_captions(
        ctx,
        table,
        CaptionSide::Top,
        caption_cb,
        &mut y,
        &mut children,
    );
    let grid_top = y;
    let layout = SectionLayout::new(prepared, geometry, &parts.measured, grid_top + edges.top);
    let grid_height = (layout.end + edges.bottom - grid_top).max(parts.css_height.unwrap_or(0.0));
    let spacing = geometry.spacing.1;
    let area = Rect::new(
        edges.left,
        grid_top + edges.top + spacing,
        0.0,
        (grid_height - 2.0 * spacing - edges.vertical()).max(0.0),
    );
    let columns = Columns::new(&table.columns, geometry, area);
    children.extend(columns.fragments.iter().cloned().map(Fragment::Box));
    let sections = Parts {
        prepared,
        geometry,
        measured: &parts.measured,
        layout: &layout,
        columns: &columns,
    };
    let baselines = section_fragments(ctx, &sections, &mut children);
    y = grid_top + grid_height;
    place_captions(
        ctx,
        table,
        CaptionSide::Bottom,
        caption_cb,
        &mut y,
        &mut children,
    );
    let grid = Rect::new(0.0, grid_top, parts.table_width, grid_height);
    let mut fragment = finish_fragment(
        &ib.base,
        Rect::new(0.0, 0.0, parts.table_width, y),
        &BoxEdges::default(),
        children,
        baselines,
    );
    fragment.border = prepared.edges.border;
    fragment.padding = prepared.edges.padding;
    let collapsed = prepared
        .collapsed
        .as_ref()
        .map(|c| c.paint(&layout.column_edges(geometry), &layout.rows()));
    fragment.content = BoxContent::Table(Arc::new(TablePaint { grid, collapsed }));
    fragment
}

/// Adds the fragments of the row groups to `children` and returns the
/// table's baselines: the first row's baseline. A table has no last
/// baseline, so that the baseline of an inline-block around it does not
/// come from it (Chromium's `PropagateBaselineFromBlockChild`).
fn section_fragments(
    ctx: &mut LayoutContext<'_>,
    parts: &Parts<'_, '_>,
    children: &mut Vec<Fragment>,
) -> Baselines {
    let mut baselines = Baselines::default();
    for index in 0..parts.prepared.grid.sections.len() {
        let fragment = section_fragment(ctx, parts, index);
        let top = fragment.border_rect.y;
        if baselines.first.is_none() {
            baselines.first = fragment.first_baseline.map(|b| b + top);
        }
        children.push(Fragment::Box(fragment));
    }
    baselines
}

/// Lays out the captions on `side` at `y` and moves `y` below them.
fn place_captions(
    ctx: &mut LayoutContext<'_>,
    table: &TableBox,
    side: CaptionSide,
    cb: ContainingBlock,
    y: &mut f32,
    children: &mut Vec<Fragment>,
) {
    for caption in &table.captions {
        if caption.base.style.caption_side != side {
            continue;
        }
        let laid_out = layout_independent_block_level(ctx, caption, cb);
        let mut fragment = laid_out.fragment;
        *y += laid_out.margins.start.solve();
        fragment.border_rect.y = *y;
        *y += fragment.border_rect.height + laid_out.margins.end.solve();
        apply_relative_position(&mut fragment, cb);
        children.push(Fragment::Box(fragment));
    }
}

/// The vertical positions of row groups and rows, relative to the table's
/// border box.
struct SectionLayout {
    /// The x position of row groups.
    x: f32,
    /// The top and height of each row group.
    sections: Vec<(f32, f32)>,
    /// The top of each row.
    row_tops: Vec<f32>,
    row_heights: Vec<f32>,
    /// The end of the last row group plus one border spacing (if it had
    /// rows).
    end: f32,
}

impl SectionLayout {
    fn new(
        prepared: &Prepared<'_>,
        geometry: &Geometry<'_>,
        measured: &Measured,
        start: f32,
    ) -> Self {
        let spacing = geometry.spacing.1;
        let mut layout = SectionLayout {
            x: prepared.edges.sum().left + geometry.spacing.0,
            sections: Vec::new(),
            row_tops: vec![0.0; measured.rows.len()],
            row_heights: measured.rows.iter().map(|r| r.height).collect(),
            end: start,
        };
        let mut y = start;
        let mut spacing_after = 0.0;
        for (index, section) in prepared.grid.sections.iter().enumerate() {
            if section.rows.is_empty() {
                // A group without rows takes no border spacing.
                let height = measured.sections[index].height;
                layout.sections.push((y, height));
                y += height;
                continue;
            }
            y += spacing;
            let top = y;
            for (n, row) in section.rows.clone().enumerate() {
                if n > 0 {
                    y += spacing;
                }
                layout.row_tops[row] = y;
                y = clamp_length(y + measured.rows[row].height);
            }
            layout.sections.push((top, y - top));
            spacing_after = spacing;
        }
        layout.end = y + spacing_after;
        layout
    }

    /// The x positions of the column edges (for collapsing borders).
    fn column_edges(&self, geometry: &Geometry<'_>) -> Vec<f32> {
        let x = self.x - geometry.spacing.0;
        let mut edges: Vec<f32> = geometry.locations.iter().map(|l| x + l.offset).collect();
        if let Some(last) = geometry.locations.last() {
            edges.push(x + last.offset + last.size);
        }
        edges
    }

    /// The top and height of each row (for collapsing borders).
    fn rows(&self) -> Vec<(f32, f32)> {
        self.row_tops
            .iter()
            .copied()
            .zip(self.row_heights.iter().copied())
            .collect()
    }
}

/// The fragments of the column groups and columns, and the backgrounds
/// that they paint in cells.
struct Columns {
    fragments: Vec<BoxFragment>,
    /// The column groups and columns with a background: style, area in
    /// table coordinates, and whether it is a group.
    parts: Vec<(Arc<ComputedStyle>, Rect, bool)>,
    /// The parts (indices of `parts`) of each column.
    by_column: Vec<Vec<usize>>,
}

impl Columns {
    /// `area` holds the left edge of the column positions, and the top
    /// and height of the columns.
    fn new(columns: &[ColumnBox], geometry: &Geometry<'_>, area: Rect) -> Self {
        let locations = geometry.locations;
        let mut out = Columns {
            fragments: Vec::new(),
            parts: Vec::new(),
            by_column: vec![Vec::new(); locations.len()],
        };
        let mut index = 0;
        for column in columns {
            if index >= locations.len() {
                break;
            }
            if column.is_group {
                index = out.add_group(column, index, locations, area);
                continue;
            }
            let span = column.span.min(locations.len() - index);
            if let Some(r) = column_rect(locations, area, index, span) {
                out.add_background(column, index, span, r);
                out.fragments.push(column_fragment(column, r, Vec::new()));
            }
            index += span;
        }
        out
    }

    /// Adds a column group that starts at column `start` with its columns.
    /// Returns the column after it.
    fn add_group(
        &mut self,
        group: &ColumnBox,
        start: usize,
        locations: &[ColumnLocation],
        area: Rect,
    ) -> usize {
        let limit = locations.len();
        let mut index = start;
        let mut children = Vec::new();
        if group.children.is_empty() {
            index += group.span.min(limit - index);
        }
        for child in &group.children {
            if index >= limit {
                break;
            }
            let span = child.span.min(limit - index);
            if let Some(r) = column_rect(locations, area, index, span) {
                children.push((child, index, span, r));
            }
            index += span;
        }
        let Some(group_rect) = column_rect(locations, area, start, index - start) else {
            return index;
        };
        self.add_background(group, start, index - start, group_rect);
        let mut kids = Vec::new();
        for (child, child_start, span, r) in children {
            self.add_background(child, child_start, span, r);
            let local = r.relative_to(group_rect.origin());
            kids.push(Fragment::Box(column_fragment(child, local, Vec::new())));
        }
        self.fragments
            .push(column_fragment(group, group_rect, kids));
        index
    }

    fn add_background(&mut self, column: &ColumnBox, start: usize, span: usize, rect: Rect) {
        if !has_background(&column.base.style) {
            return;
        }
        let index = self.parts.len();
        self.parts
            .push((Arc::clone(&column.base.style), rect, column.is_group));
        for parts in self.by_column.iter_mut().skip(start).take(span) {
            parts.push(index);
        }
    }

    /// The column group and column backgrounds of the columns that a cell
    /// spans, each once: groups below columns.
    fn backgrounds(&self, column: usize, colspan: usize) -> Vec<(&Arc<ComputedStyle>, Rect)> {
        let mut indices: Vec<usize> = self
            .by_column
            .iter()
            .skip(column)
            .take(colspan)
            .flatten()
            .copied()
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices.sort_by_key(|&i| !self.parts[i].2);
        indices
            .into_iter()
            .map(|i| (&self.parts[i].0, self.parts[i].1))
            .collect()
    }
}

/// The area of `span` columns from `start` (table coordinates); `area`
/// holds the left edge of the column positions, the top and the height.
fn column_rect(
    locations: &[ColumnLocation],
    area: Rect,
    start: usize,
    span: usize,
) -> Option<Rect> {
    let first = locations.get(start)?;
    let last = locations.get((start + span).checked_sub(1)?)?;
    Some(Rect::new(
        area.x + first.offset,
        area.y,
        last.offset + last.size - first.offset,
        area.height,
    ))
}

/// The fragment of a column or column group at `rect`, with `children`
/// (the columns of a group). It has only geometry.
fn column_fragment(column: &ColumnBox, rect: Rect, children: Vec<Fragment>) -> BoxFragment {
    let mut fragment = finish_fragment(
        &column.base,
        rect,
        &BoxEdges::default(),
        children,
        Baselines::default(),
    );
    fragment.content = BoxContent::GeometryOnly;
    fragment
}

/// What building the fragments of row groups, rows and cells needs.
struct Parts<'a, 'b> {
    prepared: &'a Prepared<'b>,
    geometry: &'a Geometry<'a>,
    measured: &'a Measured,
    layout: &'a SectionLayout,
    columns: &'a Columns,
}

/// Builds the fragment of row group `index` with its rows and cells.
fn section_fragment(
    ctx: &mut LayoutContext<'_>,
    parts: &Parts<'_, '_>,
    index: usize,
) -> BoxFragment {
    let Parts {
        prepared,
        geometry,
        layout,
        ..
    } = *parts;
    let section = &prepared.grid.sections[index];
    let (top, height) = layout.sections[index];
    let section_rect = Rect::new(layout.x, top, geometry.section_width, height);
    let mut row_fragments = Vec::with_capacity(section.rows.len());
    let mut baselines = Baselines::default();
    for row in section.rows.clone() {
        let mut fragment = row_fragment(ctx, parts, section_rect, row);
        let row_top = layout.row_tops[row] - top;
        if baselines.first.is_none() {
            baselines.first = fragment.first_baseline.map(|b| b + row_top);
        }
        baselines.last = fragment
            .last_baseline
            .map(|b| b + row_top)
            .or(baselines.last);
        fragment.border_rect.y = row_top;
        row_fragments.push(Fragment::Box(fragment));
    }
    let mut fragment = finish_fragment(
        &section.section.base,
        section_rect,
        &BoxEdges::default(),
        row_fragments,
        baselines,
    );
    fragment.content = BoxContent::TablePart;
    fragment
}

/// Builds the fragment of row `index` with its cells, at x = 0, y = 0.
fn row_fragment(
    ctx: &mut LayoutContext<'_>,
    parts: &Parts<'_, '_>,
    section_rect: Rect,
    index: usize,
) -> BoxFragment {
    let row = &parts.prepared.grid.rows[index];
    let data = &parts.measured.rows[index];
    let row_rect = Rect::new(
        section_rect.x,
        parts.layout.row_tops[index],
        parts.geometry.section_width,
        data.height,
    );
    let cells = row
        .cells
        .clone()
        .map(|cell| Fragment::Box(cell_fragment(ctx, parts, section_rect, row_rect, cell)))
        .collect();
    let baseline = Some(data.baseline);
    let mut fragment = finish_fragment(
        &row.row.base,
        Rect::new(0.0, 0.0, parts.geometry.section_width, data.height),
        &BoxEdges::default(),
        cells,
        Baselines {
            first: baseline,
            last: baseline,
        },
    );
    fragment.content = BoxContent::TablePart;
    fragment
}

/// Builds the fragment of cell `cell_index` in its row at `row_rect`
/// (table coordinates): as tall as the rows it spans, its content aligned.
fn cell_fragment(
    ctx: &mut LayoutContext<'_>,
    parts: &Parts<'_, '_>,
    section_rect: Rect,
    row_rect: Rect,
    cell_index: usize,
) -> BoxFragment {
    let Parts {
        prepared,
        geometry,
        measured,
        layout,
        columns,
    } = *parts;
    let grid = &prepared.grid;
    let cell = &grid.cells[cell_index];
    let row = &grid.rows[cell.row];
    let (x, width) = geometry.cell_span(cell.column, cell.colspan);
    let last = cell.row + cell.rowspan - 1;
    let height = layout.row_tops[last] + layout.row_heights[last] - row_rect.y;
    let (measured_cell, edges) = &measured.cells[cell_index];
    let grew = height > measured_cell.border_rect.height;
    let style = &cell.cell.inner.base.style;
    let mut fragment = if has_definite_height(style, prepared.height_specified, grew) {
        // Lay out again, so that percentage heights inside resolve.
        let content_height = (height - edges.sum().vertical()).max(0.0);
        let width = measured_cell.border_rect.width;
        layout_cell(
            ctx,
            cell,
            edges,
            width,
            geometry.section_width,
            Some(content_height),
        )
    } else {
        measured_cell.clone()
    };
    align_cell(&mut fragment, height, measured.rows[cell.row].baseline);
    fragment.border_rect.x = x;
    fragment.border_rect.width = width;
    let area = Rect::new(row_rect.x + x, row_rect.y, width, height);
    let backgrounds = Backgrounds {
        columns: columns.backgrounds(cell.column, cell.colspan),
        section: (&grid.sections[row.section].section.base.style, section_rect),
        row: (&row.row.base.style, row_rect),
    };
    fragment.content = cell_content(prepared, &fragment, area, backgrounds);
    let cb = ContainingBlock {
        width: geometry.section_width,
        height: None,
    };
    apply_relative_position(&mut fragment, cb);
    fragment
}

/// The table parts whose backgrounds a cell paints, with their areas in
/// table coordinates.
struct Backgrounds<'a> {
    columns: Vec<(&'a Arc<ComputedStyle>, Rect)>,
    section: (&'a Arc<ComputedStyle>, Rect),
    row: (&'a Arc<ComputedStyle>, Rect),
}

/// The paint data of a cell at `cell` (table coordinates): the
/// backgrounds of its column groups, columns, row group and row (CSS 2.2
/// §17.5.1, <https://www.w3.org/TR/CSS22/tables.html#table-layers>),
/// whether it hides its background and border (`empty-cells: hide`), and
/// whether the table paints its borders (collapsing borders).
fn cell_content(
    prepared: &Prepared<'_>,
    fragment: &BoxFragment,
    cell: Rect,
    parts: Backgrounds<'_>,
) -> BoxContent {
    let mut backgrounds = Vec::new();
    let local = |r: Rect| r.relative_to(cell.origin());
    for (style, area) in parts.columns {
        if let Some(clip) = area.intersection(&cell) {
            backgrounds.push(PartBackground {
                style: Arc::clone(style),
                area: local(area),
                clip: local(clip),
            });
        }
    }
    let whole = local(cell);
    for (style, area) in [parts.section, parts.row] {
        if has_background(style) {
            backgrounds.push(PartBackground {
                style: Arc::clone(style),
                area: local(area),
                clip: whole,
            });
        }
    }
    let collapsed = prepared.collapsed.is_some();
    let hidden =
        !collapsed && fragment.style.empty_cells == EmptyCells::Hide && !has_content(fragment);
    if backgrounds.is_empty() && !hidden && !collapsed {
        return BoxContent::None;
    }
    BoxContent::TableCell(Arc::new(CellPaint {
        backgrounds,
        hidden,
        collapsed_borders: collapsed,
    }))
}
