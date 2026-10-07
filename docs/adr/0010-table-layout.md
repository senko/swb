# ADR 0010: Table layout

- Status: accepted
- Date: 2026-10-03
- Updated: 2026-10-04 (M2 maintenance): see "Update (2026-10-04)" at the
  end. The decision did not change.
- Updated: 2026-10-04 (M3 floats): see "Update (2026-10-04, floats)" at
  the end.
- Updated: 2026-10-07 (provenance): see "Update (2026-10-07):
  provenance" at the end.

## Context

Target 2 (Hacker News) is built from nested tables in quirks mode, and
target 3 (Wikipedia) uses tables with collapsing borders. swb compares its
geometry with Chromium, so table layout must give Chromium's numbers, not
only a valid reading of the specifications.

CSS 2.2 §17 leaves the automatic table layout algorithm non-normative
(§17.5.2.2) and does not define how extra height goes to rows. CSS Tables 3
is a working draft that defines the column width algorithm, but not all of
the row height distribution. Chromium's LayoutNG table code
(`third_party/blink/renderer/core/layout/table/`) implements CSS Tables 3
with its own choices where the draft is open.

## Decision

Follow Chromium's LayoutNG table layout. The code is in
`crates/layout/src/table/`. Chromium's source was read for the algorithms;
parts of the code are derived from it (see "Update (2026-10-07):
provenance" and `credits.md`).

### Box tree

- `IndependentContents::Table(TableBox)`: a table is an independent
  formatting context. `TableBox` holds the captions (block containers with
  their own formatting context), the column elements (`ColumnBox`: a
  column or a column group with its columns and `span`), and the row
  groups (`SectionBox` → `RowBox` → `CellBox`). A cell is an
  `IndependentBox` with flow contents, so all of block and inline layout
  works inside it.
- Row groups are stored in Chromium's order: the first `thead` first, the
  first `tfoot` last, all other groups (also further headers and footers)
  in tree order. Only the groups that are not the first header or footer
  count as bodies for height distribution.
- Anonymous table objects (CSS 2.2 §17.2.1, CSS Tables 3 §3.1): a table,
  row group and row wrap content that is not a proper child; consecutive
  table-internal siblings outside a table share one anonymous table
  (`inline-table` inside an inline box, else `table`). White space that is
  only collapsible spaces is dropped inside tables, row groups and rows
  (as in Chromium; CSS 2.2 drops it only between table parts), and between
  misparented table parts.
- `colspan` is 1..1000 (0 and invalid values are 1), `rowspan` 0..65534
  (0 spans to the end of the row group), `span` of `col` and `colgroup`
  1..1000, as in Chromium's HTML parsing.
- Each element and each anonymous table box counts toward the box depth
  limit (`MAX_BOX_DEPTH`); deeper table parts keep only their text.
- Replaced elements (images) are never table parts, whatever their
  `display`: they keep their image and are block-level boxes. Inside a
  table, row group or row they go into an anonymous cell, as in
  Chromium.

### Algorithm

1. **Grid** (`grid.rs`): every cell takes the first free column of its
   row; `rowspan` is clamped to the rows of its group. A segment tree of
   the rows until which spanning cells occupy each column finds the free
   column in O(log columns). A spanning cell raises these rows (a range
   maximum): a cell that crosses a column with a longer span from an
   earlier row does not free that column early. The tree has one leaf per
   column that cells can reach (the sum of all colspans, at most
   `MAX_COLUMNS`), and is allocated only for tables with a `rowspan`. The
   positions are cached per table for the layout pass.
2. **Column constraints** (`columns.rs`, Chromium's
   `ComputeColumnConstraints`): columns from `col`/`colgroup` widths, then
   cells: each cell's min-content and max-content width (outer, with
   border and padding), its specified width (constrained) and percentage.
   Cells that span one column merge per column; spanning cells are then
   distributed in order of span, with the width distribution algorithm
   (automatic layout) or evenly (fixed layout). A column in which no cell
   starts is *mergeable*: it gets no width and no border spacing.
   Percentages are clamped to 100% in total. The constraints are cached
   per table for the layout pass.
3. **Table width**: a given width (flex items), the specified width, or
   fit-content between the grid's min-content and max-content widths
   (`ComputeGridInlineMinMax`, where percentage columns can raise the
   max-content width); never less than the grid's min-content width or a
   caption's min-content width; `min-width`/`max-width` apply.
4. **Column widths**: automatic layout uses the width distribution
   algorithm of CSS Tables 3 §3.9.3 with Chromium's guesses (min-content,
   percentage, specified, max-content, above max-content). Above the
   max-content guess, auto columns grow in proportion to their
   max-content widths (the Hacker News header table: 24, 964.91, 81.48 px
   in 1070.39 px). Fixed layout (`table-layout: fixed` with a width) uses
   only the column elements and the first row; fixed columns scale, the
   rest is shared evenly.
5. **Row heights** (`rows.rs`, `cells.rs`): every cell is laid out once at
   its width with an automatic height; its own `height` is ignored for
   that. A row is as tall as its tallest single-row cell, its specified
   height, and the space its baseline-aligned cells need. Spanning cells
   are distributed in Chromium's order (inner before outer). A row
   group's specified height and the table's height are distributed as in
   `DistributeExcessBlockSizeToRows` and
   `DistributeTableBlockSizeToSections` (a table's `min-height` counts
   when its `height` is `auto`): percentage rows first, then
   unconstrained non-empty rows in proportion to their height, then empty
   rows, then all rows.
6. **Cells**: the cell box is as tall as its rows. `top`, `middle` and
   `bottom` move the content in the cell; every other `vertical-align`
   value aligns the cell's first baseline (or the bottom of its content
   box) with the row's baseline. A cell whose height is definite (a fixed
   `height`, or a table height that made it grow) is laid out a second
   time with that height, so that percentage heights inside resolve.
   Cell layouts are cached per cell and constraints (the layout cache
   that flex items use, now `LayoutContext::layouts`), so nested tables
   take linear time.
7. **Fragments** (`fragments.rs`): the table's border box contains its
   captions (as Chromium reports it); the grid follows Chromium's
   `GenerateFragment` (border spacing before every row group with rows,
   none for a group without rows). Row groups, rows, cells, column groups
   and columns get fragments; columns cover the area of their cells. A
   table has no last baseline, so it does not set the baseline of an
   inline-block around it (Chromium's `PropagateBaselineFromBlockChild`);
   an inline table uses its first row's baseline.

### Borders

- Separated model: border spacing around and between the cells; cells
  paint their own borders. `empty-cells: hide` hides the background and
  border of a cell without content.
- Collapsing model (`collapsed.rs`, Chromium's `TableBorders`): an edge
  grid with one vertical and one horizontal edge per slot. Borders are
  merged in the order cells, rows, row groups, columns, column groups,
  table; a border wins if it is `hidden`, wider, or of a stronger style
  (CSS 2.2 §17.6.2.1); ties keep the earlier source. Each cell and the
  table get half of the widest border on each side for layout; there is
  no table padding and no border spacing. The table paints the edges
  centered on the grid lines, after its cells: first the vertical edges,
  then the horizontal ones. Consecutive edges of a grid line with the same
  width, style and color are one segment. As in Chromium, which paints
  each row group separately, the line between two row groups is at the
  bottom of the upper group, and vertical segments stop at the gap that
  an empty row group with a height leaves. The resolved borders are cached
  per table for the layout pass.

### Paint

`BoxContent` gets table variants: `Table` (the grid rectangle, where the
table's background and border are painted, and the collapsed border
segments), `TableCell` (the backgrounds of the column group, column, row
group and row that the cell paints below its own, CSS 2.2 §17.5.1; and the
`empty-cells` and collapsed-border flags), `TablePart` (row groups and
rows paint nothing themselves) and `GeometryOnly` (columns, and inline
boxes around block-level children: geometry for the box dump, nothing to
paint or hit).

A part's background is positioned in the part's area and painted in the
cell. Colors and images get the cell's rectangle as their area or clip;
a gradient item has the part's rectangle (so the gradient continues
across the cells) and the cell's rectangle as its clip. No clip mask is
pushed per cell, so raster work stays proportional to the cell's area.

### Related changes

- `text-align: -webkit-left/center/right` aligns in-flow block-level
  children without `auto` margins (Chromium's
  `WebkitTextAlignAndJustifySelfOffset`). Table elements reset these
  values to `start` (Chromium's `StyleAdjuster`; style crate).
- Quirks mode (Quirks Mode Standard, as Chromium implements it): the line
  height calculation quirk and the blocks ignore line-height quirk (struts
  count only for boxes with text, a line break or a horizontal border or
  padding; margins and spaces removed at the end of a line do not count;
  every line of a list item keeps the root strut, also without a marker,
  as Chromium does), the table
  cell height box sizing quirk, the nowrap minimum width quirk, the table
  cell width calculation quirk (images in auto-width cells do not wrap for
  min-content), and a table without row groups ignores its height. The
  color quirk of tables was already in the style crate. The text
  decoration quirk is not implemented: Chromium 148 propagates text
  decorations into tables in quirks mode too (checked with a screenshot),
  and so does swb.
- `<br>` elements get a box of width 0 (the content area of the parent's
  font), as in Chromium's box dump.
- An inline box that is split by a block-level child gets a geometry-only
  box around that child (the containing block's width, from the child's
  top margin edge to its bottom margin edge), as Chromium reports
  block-in-inline. Only the 8 innermost inline boxes get one, so that
  memory does not grow with the nesting depth times the number of blocks.
- Intrinsic sizes of images: percentages of the containing block count as
  `auto` for the max-content contribution; with a percentage `width` or
  `max-width`, the min-content contribution is the `min-width`
  (Chromium's `ComputeMinAndMaxContentContributionForReplaced`). In inline
  content, atomic inlines now contribute their min-content width to the
  min-content width (before: their max-content width).

### Deviations

- Lengths are `f32`, not Chromium's 1/64 px `LayoutUnit`: results differ
  by less than 0.02 px.
- The order of spanning cells for height distribution is not a total
  order (also in Chromium); swb sorts with its own stable merge sort,
  because the standard sort may panic on such an order.
- Limits for hostile content: at most 10,000 columns (`MAX_COLUMNS`;
  later cells are not laid out); the collapsed-border grids of one layout
  pass have at most 2,000,000 edges together, and a table beyond that
  uses each box's own borders and paints none. Each limit logs one
  warning per layout pass, not one per table.
- Collapsed borders: horizontal edges extend over the joints and are on
  top at every joint. Chromium decides per joint which edge wins (by
  width, style, then the order of the boxes); for example, with borders
  of equal width a vertical edge can be on top at the top of the table.
  Colors at joints can differ.
- Column percentages: they always count in a table's own layout; for its
  intrinsic widths they count only outside table cells and flex
  containers (Chromium's `AllowColumnPercentages` checks the containing
  blocks; swb counts the cells and flex containers that are being laid
  out or measured, `TableCache::percent_free`). Grid layout does not exist
  yet.
- Not supported: `visibility: collapse` for rows and columns, the
  "quirky" margins of the first and last children of cells in quirks mode,
  the body and html fill-viewport quirks, fragmentation (repeated
  headers).

## Consequences

- The Hacker News geometry score went from 0 to 0.9876; the rest comes
  from the search `<input>` (forms, M2).
- Table layout matches Chromium in 23 layout tests (`tests/layout/table-*`,
  `quirks-*`, `webkit-center`, `intrinsic-replaced-percent`).
- Changing table code means checking it against Chromium again: the
  layout tests and `just compare` show regressions.
- `BoxContent` has table variants that paint and hit testing must handle.

## Update (2026-10-04)

Corrections from the M2 maintenance review:

- The anonymous table object fixup is CSS Tables 3 §2.2.1
  (`#fixup-algorithm`), not §3.1.
- The header and footer groups are found by their display type
  (`table-header-group`, `table-footer-group`), not by the element name:
  the first group of each type moves.
- Mergeable columns exist only in automatic layout. In fixed layout every
  column takes width and border spacing.
- Column percentages are clamped to 100% in total only in automatic
  layout. Fixed layout scales them down in proportion when their total is
  more than 100%.
- With forms (ADR 0013), the Hacker News geometry score is 1.0.

## Update (2026-10-04, floats)

With floats (ADR 0015), a block-level table is a box that establishes a
block formatting context and does not overlap floats.
`layout_block_level` takes the available width: the containing block's
width, or a layout opportunity next to floats with the table's margins.
The table's width (fit-content) and its `auto` margins use that width;
percentage widths still resolve against the containing block, so a
`width: 100%` table next to a float moves below it, as in Chromium.
Table cells and captions establish their own block formatting context:
their floats stay inside them and count for their auto height.

## Update (2026-10-07): provenance

A provenance audit compared the table code with the Chromium sources that
`credits.md` lists. Parts of `crates/layout/src/table/columns.rs`,
`distribute.rs`, `rows.rs`, `cells.rs` and `layout.rs` are derived from
Chromium code, not only from its algorithms:
`third_party/blink/renderer/core/layout/table/table_layout_utils.cc`,
`table_layout_algorithm_types.cc` and `.h`, and
`table_layout_algorithm.cc` (BSD-3-Clause). The project owner decided to
keep this code with attribution: a comment in each file and an entry in
`THIRD_PARTY_NOTICES.md` with the Chromium license. A clean-room rewrite
may follow.
