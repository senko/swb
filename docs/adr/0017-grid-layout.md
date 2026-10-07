# ADR 0017: Grid layout

- Status: accepted
- Date: 2026-10-04

## Context

Target 3 (Wikipedia "Web browser", Vector 2022 skin) builds its page
skeleton with CSS grid: `.mw-page-container-inner` (the table of contents
column and the article column, with `grid-template-areas`) and `.mw-body`
(the title bar, the toolbar, the content and the tools column). Without
grid layout, swb laid these containers out as blocks, so the table of
contents was above the article instead of beside it.

The specification is CSS Grid Layout 2
(<https://www.w3.org/TR/css-grid-2/>). It leaves room in some places
(limits of the grid, the order of some steps), and Chromium deviates from
it in a few others. swb compares its geometry with Chromium, so where they
differ, Chromium's LayoutNG grid (`third_party/blink/renderer/core/
layout/grid/`) is the reference. Its source was read for the algorithms;
no code was copied.

## Decision

Implement CSS Grid 2 without subgrid and masonry, in the style crate and
in a new layout module `crates/layout/src/grid/`.

### Style

New longhands: `grid-template-columns`, `grid-template-rows`,
`grid-template-areas`, `grid-auto-columns`, `grid-auto-rows`,
`grid-auto-flow`, `grid-row-start`, `grid-row-end`, `grid-column-start`,
`grid-column-end`, `justify-items`, `justify-self`. New shorthands:
`grid-row`, `grid-column`, `grid-area`, `grid-template`, `grid`;
`place-items` and `place-self` now set `justify-items` and
`justify-self`. The value types are in `style/src/values/grid.rs`, the
parsers in `style/src/parse/grid.rs`.

- Track sizes and track lists are generic over the length type
  (`GenericTrackSize<L>`): declarations hold specified lengths, the
  computed style holds `LengthPercentage`. A track list keeps its
  `repeat()`s as written (with the line names before each entry and inside
  each repetition); layout expands them.
- `<auto-track-list>` rules are checked at parse time: one
  `repeat(auto-fill | auto-fit, ...)` at most, and only `<fixed-size>`
  tracks with it.
- `grid-template-areas` strings are tokenized as in §7.3 (a run of `.` is
  one null cell, other characters that are not name code points make the
  declaration invalid); every row must have the same number of cells, and
  every named area must be a filled rectangle. The computed value lists
  the areas with their lines.
- Grid lines accept the forms that Chromium accepts (`span` first or
  last; `2 span foo` is invalid). Integers are stored as parsed and
  clamped in layout.
- `justify-items: legacy` computes to `normal`; `legacy center`,
  `legacy left` and `legacy right` compute to `center`, `left` and
  `right` (Chromium's `ResolvedSelfAlignment`). The inheritance of
  `legacy` values is not supported.
- `subgrid` and `masonry` are invalid, so `@supports` reports them as
  unsupported and pages take their fallbacks.

### Box tree

`display: grid` and `inline-grid` build `IndependentContents::Grid`. The
children are built exactly like flex items (`build_flex_items`): every
in-flow child becomes an item, text runs get anonymous blocks, floats are
items.

### Layout

The module has five parts:

1. **Template** (`template.rs`): the explicit grid of one axis as
   *segments*, runs of a repeated track list, so that `repeat(10000, ...)`
   is one segment. Implicit tracks take `grid-auto-*` sizes; the first
   one after the explicit track list takes the first size, the last one
   before the explicit grid the last size (§7.6). Tracks of named areas
   beyond the track list are sized like implicit tracks after it, as in
   Chromium. The style crate builds the positions of the line names of
   each declaration once (`LineNameTable`, shared through `Arc` by the
   computed values of all elements), and `GridTemplateAreas` indexes its
   areas by name. The table stores a repeated name of one line (`[a a a]`)
   once. Layout expands the lines of only the names that items use, and
   stops at the first position beyond the explicit grid, so a grid does
   no work for names it does not use (before this, 20,000 areas shared by
   3,000 grids took 90 s to lay out).
2. **Placement** (`placement.rs`, §8): line resolution with named lines,
   named areas and their implicit `-start`/`-end` lines (§8.3), conflict
   handling (§8.3.1), and the auto-placement algorithm (§8.5) with sparse
   and dense packing and both flow directions. Items locked to a row keep
   a cursor per row, as in Chromium. Occupied cells are stored per row as
   sorted, merged intervals, not as a matrix. Lines are numbered from 0 at
   the start of the explicit grid; negative lines are implicit lines
   before it.
3. **Tracks** (`tracks.rs`): Chromium's `GridSizingTrackCollection`. The
   tracks are split into *ranges* at every line where an item starts or
   ends, at the explicit grid's edges and at segment boundaries; a range's
   tracks with the same size form a *set*. All tracks of a set are sized
   alike, so the sizing algorithm works on sets. Empty tracks of
   `repeat(auto-fit, ...)` collapse (no sets, no gaps).
4. **Track sizing** (`sizing.rs`, §12.3–§12.8), as in Chromium's
   `grid_track_sizing_algorithm.cc`:
   - Items are processed by span (all items, also those of span 1, in the
     step "increase sizes to accommodate spanning items"), then the items
     that span a flexible track together, distributing only to flexible
     tracks by flex factor.
   - Extra space is distributed per set with Chromium's "share ratio"
     method: sets sorted by growth potential, equal shares per track,
     then growth beyond limits for the sets that the specification names
     (intrinsic or `max-content` maximums, else all).
   - Deviations that Chromium has too: intrinsic minimums use the minimum
     contributions also under min- and max-content constraints; `auto`
     minimums do not take max-content contributions under a max-content
     constraint; the flex fraction is not recomputed for `min-width` or
     `max-width`; maximizing tracks does not redo the step for
     `max-width`.
5. **Grid sizing and items** (`mod.rs`):
   - §12.1: the columns first (available space: the content width), then
     `justify-content` (so spanning items see the distributed gaps), then
     the rows with the items' heights at their area widths. If the
     container's height is indefinite and a row track is flexible or a
     percentage (or the row gap is a percentage), the rows are sized a
     second time with the resolved height (Chromium's additional pass).
     §12.1 steps 3 and 4 (a second column pass for items whose min-content
     contribution depends on the rows) are not done.
   - The number of automatic repetitions follows Chromium's
     `CalculateAutomaticRepetitions` (with the maximum size if definite,
     else the minimum size).
   - Contributions: the inline-axis contributions are the items'
     margin-box min-content and max-content widths
     (`intrinsic::independent_outer_sizes`); the block-axis contribution
     is the item's height when laid out at its area width. The minimum
     contribution follows Chromium's
     `CalculateIntrinsicMinimumContribution`: the content-based minimum
     (§6.6) only for items that are not scroll containers, span an `auto`
     minimum track and no flexible track if they span more than one,
     clamped by the fixed maximums of the spanned tracks.
   - An image that stretches in the block axis and spans only rows with a
     fixed maximum contributes the width that follows from the height of
     those rows and the gaps between them through its aspect ratio (§12.1
     step 1). Percentage rows count as fixed when the grid has a definite
     height, also for the grid's intrinsic widths. In other rows it
     contributes its natural width, because the second column pass is not
     done.
   - Items: `normal` stretches non-replaced items and `stretch` all items
     with an `auto` size and no `auto` margins (Chromium's implicit and
     explicit stretch). An image stretched in one axis takes the other
     size from its aspect ratio; stretched in both, it ignores the ratio.
     Other values give fit-content widths and content heights, and
     `min-content` and `max-content` sizes give those widths. `auto`
     margins align first and never overflow the start (Chromium's
     `AxisEdgeFromItemPosition` and `AlignmentOffset`). Percentages of
     items (sizes, margins, padding, relative offsets) resolve against the
     grid area; an image with a percentage height takes its width from
     that height through its aspect ratio.
   - `align-self: baseline` aligns the first baselines of the items that
     start in the same row, after track sizing (without Chromium's
     baseline shims in the track sizing algorithm).
   - The grid's first (last) baseline is the shared baseline of the
     baseline-aligned items of the first (last) row that has items (§11.6,
     Chromium's `GridBaselineAccumulator`); without such items, the
     baseline of the item that comes first (ends last) in row-major order,
     or its border-box bottom. An inline grid uses its first baseline,
     also when it is a scroll container. An inline-block that contains a
     grid (or a flex container) as its last child takes the first
     baseline of that child, not the last (Chromium's
     `UseLastBaselineForInlineBaseline`).
   - The container's intrinsic widths are the column sizes under a
     max-content and a min-content constraint (min = the min-content
     size, max = the larger of both).
   - Items paint in order-modified document order (fragment order).
   - Absolutely positioned children get a placeholder (their static
     position: aligned by `justify-self`/`align-self`, or the container's
     `justify-items`/`align-items`, in the padding box if the grid
     container is their containing block, else in the content box); the
     out-of-flow pass
     of ADR 0016 lays them out with their offsets. Grid areas as their
     containing blocks are not supported (ADR 0016).
   - Tables inside grid items do not use column percentages for their
     intrinsic widths (Chromium's `AllowColumnPercentages`, through
     `TableCache::percent_free`).
   - A grid container that is a scroll container gets a scrollable
     overflow rectangle (ADR 0019). Its in-flow content is the area of its
     tracks after content alignment, with the end padding; the items
     count as descendants, with their border boxes and without margins
     (unlike flex items). This is what Chromium does
     (`tests/layout/grid-scroll.html`).

### Caching

- Items are laid out through the layout cache of flex items
  (`block::layout_flex_item`, keyed by box and constraints).
- An item is laid out twice: to measure its height (area height
  indefinite) and at its final size (area height definite). The final
  layout reuses the measurement when it cannot differ: the item has no
  percentage height, and it is not stretched, or stretched to its
  measured height while nothing depends on that height becoming
  definite: its own `min-height` and `max-height` are `auto` (or 0) and
  `none` (otherwise they may have clamped the measured content, and a
  flex or grid item would size its contents against the clamped height),
  it has no percentage `row-gap` (it resolves against the definite
  height), it is not a grid with automatically repeated rows, and nothing
  inside it has a percentage height (or a percentage `flex-basis` in a
  column flex container). Only the item's own style is checked: flex
  layout does not yet make the stretched children of a flex container
  with a definite height definite (Chromium does). Wikipedia nests two
  grids around the whole article; without the reuse, its layout took
  about 25% longer.
- The placement of each grid is cached per layout pass by box and
  numbers of automatic repetitions; the intrinsic widths of each grid
  container per box.

### Limits for hostile content

| Limit | Value | Effect beyond it |
|---|---|---|
| Lines (`MAX_LINE`) | `-10000..=10000` around the explicit grid; the explicit grid has at most 10,000 tracks | Areas are clamped (§5.4): truncated, or moved into the last track on that side. |
| Extra spans per layout pass (`SPAN_BUDGET`) | 4,000,000 tracks | Later items span one track. |
| Placement work per grid (`PLACEMENT_WORK`) | 4,000,000 row checks or marks | The remaining auto-placed items go after all placed items (no overlap, no packing). |
| Named lines per grid (`NAMED_LINE_BUDGET`) | 100,000 expanded lines | Further lines of a name are not found (they count as missing). |

Chromium allows 10,000,000 lines; the specification asks for at least
`[-10000, 10000]` (§5.4), which is also Firefox's limit. Because tracks
are sets, the cost of sizing depends on the number of items and the
length of the track lists, not on the number of tracks: a page with
thousands of grids of `repeat(100000, 1px)` costs one set per range.
The style crate stores the index of the automatic repetition with the
track list, and the number of repetitions looks at no more than 10,001
entries. A track list without `repeat()` costs one set per listed track,
up to 10,000 per grid: 10,000 grids that list 1,000,000 tracks each (a 4.5 MB
style sheet) take about 24 s to lay out.
The first time a limit is reached in a layout pass, one warning is
logged.

### Not supported

Subgrid, masonry, baseline shims in track sizing, `last baseline`
alignment (it aligns first baselines), the exclusion of items whose size
depends on intrinsic tracks from baseline alignment (§11.4), the second
column pass (§12.1 steps 3 and 4), `fit-content` sizes of items
(treated as `fit-content` of the area), the inheritance of
`justify-items: legacy`, `safe`/`unsafe` overflow alignment (parsed and
ignored), `z-index` on grid items that are not positioned (Grid 2 §6.5;
paint does not make them stacking contexts, as for flex items),
absolutely positioned items placed by grid lines, fragmentation, and
`display: grid` on `<button>` (laid out as a block). The container's
intrinsic widths resolve a percentage `height` against an indefinite
size, so with `repeat(auto-fill)` rows they can use another number of
repetitions than the layout.

### Related changes outside grid layout

- Inline flex containers, like inline grids, keep their first baseline
  when they are scroll containers (the bottom margin edge rule of CSS 2.2
  §10.8.1 applies to inline-blocks only), clamped to their border box, as
  in Chromium. An inline table is not a scroll container (`overflow` does
  not apply to tables) and keeps the baseline of its first row.
- The baseline of an inline-block whose last child is a flex or grid
  container is the first baseline of that child (`Baselines::of` in
  `block.rs`).
- Intrinsic sizes: a border-box `min-width` or `max-width` clamps the
  content-box size after the padding and border are subtracted
  (`intrinsic.rs`); before, the padding counted twice.

## Consequences

- Wikipedia's grid containers and items match Chromium (x, widths and
  the order of rows); the table of contents sits left of the article.
  Wikipedia scores: size 0.6175 → 0.8100, relative 0.4587 → 0.7728,
  geometry 0.0099 → 0.0345. Pixels go down (0.8858 → 0.8722): the
  article text is now in its column but does not flow around the floated
  images yet (floats are a separate M3 workstream).
- Thirteen layout tests (`tests/layout/grid-*.html`) match Chromium.
- `IndependentContents` has a `Grid` variant; code that matches on it
  must handle grids (block layout calls `grid::layout` from
  `layout_sized`, intrinsic sizes call `grid::content_sizes`).
