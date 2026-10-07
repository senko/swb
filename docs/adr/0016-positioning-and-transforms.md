# ADR 0016: Absolute, fixed and sticky positioning and 2D transforms

- Status: accepted
- Date: 2026-10-04

## Context

Target 3 (Wikipedia, "Web browser") uses absolutely positioned boxes for
small decorations and controls (the 1 px line under the page title, the
magnify icons of thumbnails, the search icon, the dropdown menus and their
transparent checkboxes, visually hidden skip links with `clip`), sticky
positioning for the sidebar table of contents, and `transform:
translateY(-50%)` to center icons. Before this work, swb placed an
absolutely positioned box at its static position with a shrink-to-fit
width and ignored its offsets, dropped those that started in inline
content or in flex containers (542 missing element boxes on Wikipedia),
painted fixed boxes like absolute ones (clipped by their ancestors, moving
with the page), and ignored `transform`, `transform-origin` and `clip`.

Specifications: CSS 2.2 §9.3, §9.6, §10.1, §10.3.7, §10.3.8, §10.6.4,
§10.6.5 and §11.1.2; CSS Positioned Layout 3; CSS Transforms 1; CSS
Flexbox 1 §4.1. Chromium 148 is the reference where they leave room.

## Decision

### Style

- `transform` (2D functions; `translate3d()`, `scale3d()` and
  `rotateZ()` are accepted as their 2D effect, `translateZ()` and
  `scaleZ()` as the identity, so that `transform: translateZ(0)` still
  makes a stacking context and a containing block; other 3D functions
  make the declaration invalid),
  `transform-origin` (the z part is ignored), `clip: rect()` (with and
  without commas), and the aliases `-webkit-transform` and
  `-webkit-transform-origin`. Code: `style/src/properties/transform.rs`,
  `style/src/values/transform.rs`.
- `ComputedStyle::original_display`: `display` before blockification. The
  static position of an absolutely positioned box in inline content
  depends on it (Chromium's `IsOriginalDisplayInlineType`).

### Layout of absolutely positioned boxes (`layout/src/positioned.rs`)

Absolutely positioned boxes are laid out after the normal flow, in two
steps:

1. Block, inline and flex layout put a *placeholder* fragment (size 0,
   `BoxContent::Placeholder`) at the static position. It moves with its
   parent like any fragment, also when the layout cache of flex items and
   table cells reuses a fragment.
2. After the whole document is laid out, one top-down walk of the
   fragment tree (`place_out_of_flow`) keeps the containing block of
   absolutely positioned descendants (the padding box of the nearest
   positioned or transformed ancestor, else the initial containing block)
   and of fixed descendants (the nearest transformed ancestor, else the
   viewport), in the coordinates of the fragment it visits. All sizes are
   known at that time, so at a placeholder it lays out the box and puts
   its fragment *in place of the placeholder*, positioned relative to the
   placeholder's parent. Then it walks the new fragment.

Consequences of this design:

- Every fragment stays positioned relative to its parent fragment, so the
  walkers of the fragment tree (box dumps, selection, focus, scrolling)
  need no change.
- The fragment of a positioned box is at its place in tree order, so
  paint order among equal z-index values is tree order.
- Each absolutely positioned box is laid out once per layout pass, even
  inside flex items that flex layout lays out several times: only the
  final tree has its placeholder.
- The containing block's height can be `auto`: it is known before the
  walk.
- The walk is skipped when layout made no placeholder.

A positioned inline box is the containing block of its absolutely
positioned descendants: from the top-left padding edge of its first
fragment to the bottom-right padding edge of its last fragment in the same
block container (CSS 2.2 §10.1 item 4); a negative width or height is 0.
Box construction keeps an absolutely positioned box in the inline content
while an inline box is open, also when it is the first content of the
inline box or the only content of the block (otherwise it would move to
block level, out of the inline box).

Static positions (all checked against Chromium in `tests/layout/`):

- Block flow: the parent's content edge, at the current position after
  the collapsed margins of the preceding siblings.
- Inline content (Chromium's `PlaceOutOfFlowObjects`): a box that was
  inline-level is where it would be on the line, at the top of the line
  box; a box that was block-level starts the next line: the line start,
  below the line box if text or an atomic inline precedes it on the line,
  else at the top of the line box.
- Flex containers (CSS Flexbox 1 §4.1): as the only flex item, aligned
  in the content box by `justify-content` and `align-self` (with
  `space-between` as `flex-start`, `space-around` and `space-evenly` as
  `center`, `stretch` as `flex-start`, `baseline` as `start`; reverse
  directions and `wrap-reverse` swap the flex edges, not `start` and
  `end`, as in Chromium). The static position records
  which edge of the margin box is at the point (start, center, end); with
  `left` and `right` both `auto`, the available space is the space on the
  side of that edge (CSS Position 3 §4.1).
- Grid containers (ADR 0017): the static-position rectangle is the
  padding box if the grid container is the containing block (the area of
  `auto` lines, CSS Grid 2 §9.4), else the content box (§10.2), as in
  Chromium. Grid layout puts the placeholders of its absolutely
  positioned children between the item fragments in document order (as
  flex layout does), and a grid scroll container gets them in its
  scrollable overflow like the others.

Self-alignment (CSS Position 3 §4.4, CSS Align 3; measured with Chromium
148 in `tests/layout/abspos-align.html`): `justify-self` and `align-self`
apply to absolutely positioned boxes in every container.

- With both insets of an axis set, the box is aligned in the
  inset-modified containing block; `auto` there is `normal`. `normal` and
  `stretch` let an `auto` size fill it (CSS 2.2); another value gives the
  `auto` size the shrink-to-fit size and places the margin box at the
  start, center or end. A box that overflows stays at the start (safe;
  `safe` and `unsafe` are parsed and ignored, so `unsafe` overflow is a
  deviation). `auto` margins take the free space first.
- With both insets `auto`, the box is aligned in its static-position
  rectangle (unsafe, as in Chromium; `Placeholder::extent`): in block
  flow, the content box's width at the static position and no height; in
  inline content, no width and the height of the line box (for a box
  that was block-level and follows content on its line, from the bottom
  of that line, as in Chromium); in a grid, the rectangle above. `auto` takes the parent's
  `justify-items` (not in inline content), and in a grid also its
  `align-items` (a block container's `align-items` does not count, as in
  Chromium). Flex containers keep their rule above. Deviation: a box that
  comes directly before inline content (at the start of its container or
  after a block-level box) is a block-level box in swb's box tree, so it
  is aligned as in block flow (Chromium: as in inline content).
- With one inset set, the box is not aligned in that axis.

Sizes and positions follow CSS 2.2 §10.3.7 and §10.6.4 (§10.3.8 and
§10.6.5 for images): the six rules for `auto` values, `auto` margins that
center (not negative horizontally), over-constrained values that ignore
`right` or `bottom`, shrink-to-fit widths (the available space is set by
the insets, or by the static position), heights from the content or from
`top` and `bottom`, and `min-*`/`max-*` sizes that solve the equation
again with the clamped size. `width: min-content | max-content |
fit-content` count as specified widths (Chromium; the Wikipedia dropdown
menus use `max-content`); the same keywords for `height` take the height
of the content, so the box does not stretch between `top` and `bottom`
and `auto` margins center it (Chromium: the centered dialog idiom
`inset: 0; margin: auto; height: fit-content`). `fit-content(<length>)`
is the content height too (CSS Sizing 3: between the min-content and the
max-content height, which are both the content height). Deviation:
Chromium 148 accepts the function for neither `width` nor `height`, so
it uses the declaration before it; swb accepts it. An absolutely positioned root
element is placed in the initial containing block in the same way (as
Chromium). A table sizes as fit-content in both axes (as in Chromium):
it does not stretch between its insets, and its `auto` margins center
it. Percentages of insets refer to the containing block's
size, percentages of margins to its width.

### Fixed and sticky positioning

- A fixed box without a transformed ancestor is positioned in the
  viewport (layout gives viewport coordinates); paint, hit testing and the
  painted element boxes add the scroll offset. It is not clipped by the
  overflow clips of its ancestors (also not by those of stacking contexts
  that contain it), only by the `clip` rectangles of its ancestors (as in
  Chromium); it does not extend the scrollable area. With a transformed
  ancestor, it behaves as an absolutely positioned box of that ancestor
  (and the `clip` rectangles between them also apply).
- A sticky box is laid out at its normal position. Its offset follows
  Chromium's `StickyPositionScrollingConstraints`: per inset (`top` wins
  over `bottom`, `left` over `right`), it moves into the scrollport inset
  by the inset, but its margin box stays in the content box of its
  containing block: the nearest ancestor that is not an inline box, an
  anonymous box, a table row or a row group (so a sticky cell sticks in
  its table, and a sticky inline box in an anonymous block in the
  element's block, as in Chromium). Percentage insets refer to the
  scrollport. The scrollport is the padding box of the nearest scroll
  container ancestor without its padding (as Chromium; measured with
  Chromium 148), else the viewport. For a box whose containing block is
  that scroll container, the area it stays in is the scroll container's
  scrollable overflow without the padding, which moves with the content
  (measured). Sticky ancestors in the same
  scroll container (and not outside a fixed box) move a sticky box before
  its own offset is computed, as in Chromium: the nearest one between the
  box and its containing block moves the box by its accumulated offset,
  and the nearest one that is the containing block or contains it moves
  both the box and the containing block (`StickyConstraints::shift_box`
  and `shift_containing_block`; the offsets of a chain are computed once
  each). In a scroll container, the walks apply the element scroll
  offsets first (`ScrollState`, ADR 0019) and give `Ancestry` the
  scrolled rectangles, so the constraints of a sticky box in a scroll
  container are fixed for the display list (which a scroll rebuilds); in
  the viewport, the offset depends on the viewport scroll offset, which
  the rasterizer and hit testing apply.

### Scroll containers (ADR 0019)

- `ScrollState` and the engine's scroll chain (`Scrollers::chain`) follow
  the containing blocks of this ADR: transformed boxes contain absolutely
  positioned boxes, and a fixed box moves with the scroll containers
  above its nearest transformed ancestor (none without one). A fixed box
  in a scroll container is not clipped by it (paint's clip rule above),
  and a transformed scroll container moves the fixed boxes it contains.
- Scrollable overflow: fixed boxes count when a transformed box between
  them and the scroll container contains them; a transformed box counts
  with the transformed bounds of itself and its content, as in Chromium.
  The out-of-flow pass runs after the scroll containers computed their
  scrollable overflow, so it adds the boxes that it places to the
  rectangles of the scroll containers above them
  (`scroll::with_out_of_flow`; Chromium: a positioned scroll container
  with an absolutely positioned child at `top: 200px` has a
  `scrollHeight` of 230). The viewport's scroll size counts absolutely
  positioned boxes whose containing block is outside a box that clips
  them (the walk does not stop at such a box).
- Block in inline: the blocks inside a positioned inline box carry
  `BoxFragment::in_positioned_inline` (from scrolling). The out-of-flow
  pass uses it: the containing block of the absolutely positioned boxes
  in such a block is the inline box (the rectangle from its first to its
  last fragment in the block container, also through the anonymous
  blocks), found among the inline box parts (`GeometryOnly`) right before
  the block. Absolutely positioned boxes directly in an inline box stay
  inline (they are in its fragments), so they need no mark. Paint's
  `absolute_clips` treat such a block as the containing block, without
  its own clip; the selection clip does the same.
- Full-page screenshots keep the layout of the viewport, so the offsets
  of scroll containers stay as they are (no temporary viewport to save
  them for).
- Scroll into view (focus, fragment navigation, automation clicks) uses
  the painted boxes and scrollports (`FragmentTree::walk_painted`), and
  does not scroll the viewport for a box fixed to it (Chromium does not
  either: a viewport scroll does not move the box). A URL with a
  fragment that equals the document's apart from the fragment scrolls to
  the fragment also
  when it is typed (`Page::navigate`; HTML "navigate to a fragment"),
  except while a load is in progress or on an error page (which is not
  the document of its URL).

### Transforms

- Transforms apply to block-level and atomic boxes, not to inline boxes
  (CSS Transforms 1 §3). They do not change layout. A transformed box is
  a containing block for absolutely positioned and fixed descendants and
  forms a stacking context; without `position` it paints with the
  positioned boxes at z-index 0.
- `transform_matrix`: translate to the `transform-origin`, the functions
  from left to right, translate back; percentages refer to the border
  box. A matrix with infinite or NaN entries maps everything to one point
  (invisible, never hit).
- The element boxes (`FragmentTree::element_boxes_scrolled`, used for
  box dumps and `dom.box`) are the bounding boxes of the painted border
  boxes, as `getClientRects()`: the scroll offsets of scroll containers,
  transforms of the box and its ancestors, the scroll offset of fixed
  boxes, sticky offsets (`FragmentTree::walk_painted`, which scroll into
  view and its scrollports use too). The scrollable area includes
  transformed bounding boxes (as Chromium).

### Paint and hit testing

- The display list has transform groups (`PushTransform` /
  `PopTransform`) of three kinds (`swb_layout::GroupTransform`): a
  matrix, `Fixed` (the viewport scroll offset; it replaces the
  translations of the enclosing groups, which can only be fixed and
  sticky groups) and `Sticky` (the constraints; the offset is computed
  for the scroll offset). The list does not change when the page
  scrolls. `Ancestry::group_transforms` decides the groups of a box for
  both paint and the painted element boxes.
- Stacking: a positioned box with `z-index: auto` and `position: relative`
  or `absolute` (and no transform or opacity) no longer forms a stacking
  context (it did before as a simplification): its positioned descendants
  take part in the enclosing context (CSS 2.2 Appendix E). Fixed, sticky
  and transformed boxes always form one.
- Clips: a stacking context paints its positioned descendants outside
  its own overflow clip; each positioned box repeats the clips between
  its stacking context and itself (the context root's included) that
  belong to its containing block chain: an absolutely positioned box the
  clips of its containing block and the boxes above it, a fixed box with
  a transformed containing block those of that box and above, plus the
  `clip` rectangles. A box fixed to the viewport starts with
  `PushViewportClip`, which replaces all enclosing clips, and then repeats
  only the `clip` rectangles of its ancestors. An opacity layer around
  content fixed to the viewport covers the whole surface, so that the
  enclosing clips do not crop it, but only as large as its content: the
  content fixed to the viewport has its bounds in viewport coordinates
  (`fixed_bounds`), which the rasterizer moves by the scroll offset.
  `clip: rect()` clips an absolutely
  positioned box and its descendants, fixed ones included (`auto` edges
  are the border box edges).
- Hit testing maps the point into each group (the inverse of its
  transform at the current scroll offset) and tests clips and regions
  there; a group that cannot be inverted is not hit, and a group whose
  hit bounds (`hit_bounds`: its painted area and its hit regions) the
  point misses is skipped with the groups inside it. The rasterizer uses
  the painted area only (`bounds`), so that hit regions, which cover
  every border box, do not make transform layers larger.
  `DisplayList::hit_test` takes the scroll offset.
- Sticky offsets: a chain of sticky boxes is computed once per frame,
  hit test or walk (`StickyCache`, keyed by the address of the
  constraints; the walk keeps them alive), not once per group.
- Rasterization: groups that only translate (fixed, sticky, `translate()`)
  move the coordinates of their items, so they are exact: rectangles are
  snapped to pixels after the translation. A group with another transform
  is drawn into a layer in its own coordinates, at the device resolution
  times the largest scale factor of the transform, and the layer is drawn
  with the transform and bilinear filtering. So rotated, skewed or
  scaled text is a resampled image of text rasterized at a suitable
  resolution, not text drawn from outlines: slightly softer than in
  Chromium, but the geometry is exact. Glyph outlines would need a new
  text API; no target page needs rotated text (Wikipedia's rotated icons
  are images, Hacker News scales its vote arrows only on narrow screens).
  The layer is drawn as an anti-aliased rectangle filled with its pixels
  (tiny-skia's `draw_pixmap` does not anti-alias the edges of a
  transformed pixmap).
- Masks (ADR 0018): the transform groups of a box enclose its opacity and
  mask groups, so the mask is in the box's own coordinates and turns with
  the box; mask and opacity groups inside a transformed box are drawn in
  its layer. An opacity group that contains content fixed to the
  viewport is not cropped by the enclosing clips (`escapes_clips`). A
  mask group is: the mask is drawn inside the enclosing clips, so they
  clip the fixed content in the group too, as in Chromium (measured: a
  fixed box in a masked box inside an `overflow: hidden` box). The
  `no-clip`
  area of a mask maps the ink of transform groups by their transform, and
  transformed descendants count as descendants with their own layer, as
  in Chromium. Fixed and sticky groups count at scroll offset 0
  (simplification: the area does not follow the scroll offset).
- Strips (ADR 0018): fixed and sticky groups resolve against the scroll
  offset of the page, not of the strip, so they are at the same place in
  every strip. The pixels of a transform layer lie on a grid that starts
  at the corner of the group's bounds, and the layer reaches 2 pixels
  beyond the visible part of the group: the layers of one group in
  different strips then sample the same positions. In a test, strips of
  7 and 13 rows differ from one pass in 4 and 9 pixels by at most 16
  levels (rounding); strips of 100 rows give the same pixels. A group
  whose layer gets a lower resolution (more than
  `MAX_TRANSFORM_LAYER_PIXELS` visible in a strip) can get another
  resolution in each strip, which can show at the strip boundary.
- Full-page screenshots keep the layout of the viewport and draw the
  page from its top, as Chromium (Playwright `full_page`) does: fixed and
  sticky boxes are where they are at the page's current scroll offset
  (`RasterParams::viewport_scroll`; at 700 px, a fixed header is at 700),
  `vh` units and media queries keep the viewport's height, and the
  viewport does not clip fixed boxes. Before, `Page::screenshot` laid the
  page out again in a viewport as high as the page, which put
  `bottom: 0` fixed boxes at the end of the page.

### Limits for hostile content

- Layout: one placeholder and one layout per absolutely positioned box and
  layout pass; the out-of-flow walk visits each fragment once; the scan
  for positioned inline boxes visits each inline fragment once per block
  container. Recursion depth is bounded by the box depth limit (256), as
  for all layout (`tests/deep_nesting.rs` covers nested absolute, fixed,
  sticky, transformed and inline positioned boxes on a 2 MiB stack). All
  results are clamped to ±33,554,431 px. Tested: 20,000 absolutely
  positioned siblings.
- Transforms: infinite or NaN matrices are collapsed to a point; bounding
  boxes are clamped; the inverse uses `f64` and rejects determinants below
  1e-12.
- Raster: a transform layer covers only the part of the group that can be
  visible in the strip (the inverse image of the visible device area,
  intersected with the group's bounds, see "Strips"), has sides of at
  most 16,384 pixels and at most 4,194,304 pixels (16 MiB,
  `MAX_TRANSFORM_LAYER_PIXELS`); a larger layer gets a lower resolution.
  Layers nest at most 8 deep (`MAX_TRANSFORM_DEPTH`). The budgets are
  those of a strip (ADR 0018), in one design for all layers: while a
  transform layer is drawn, it is an open layer for the strip's layer
  memory budget (`MAX_GROUP_LAYER_PIXELS`, shared with opacity and mask
  layers); the transform layers of a strip have a work budget of
  67,108,864 pixels, or eight times the strip's pixels if that is more
  (`MAX_TRANSFORM_PIXELS`): a layer costs its pixels (its content is
  drawn into it) plus the device pixels that it covers (it is drawn with
  filtering). The rasterizer of a transform layer takes over the strip's
  budgets while it draws, so the opacity, mask and transform layers
  inside it count too. A group beyond these limits is not drawn (logged
  once per frame). A full-page screenshot has at most 8 strips, so its
  transform work is at most 8 × max(64 Mpx, 8 × strip pixels).
  Tested: 300 nested rotated boxes, 2,000 viewport-sized rotated boxes,
  `scale(1e30)`, `scale(1e30, 1e-6)`, `scale(0)`, `matrix(1e38, …)`, under
  an 8 GB address space limit.
- Display list: each item is copied a bounded number of times, also for
  deeply nested positioned, transformed, sticky and fixed boxes. The
  builder's list is a list of chunks (`paint/src/rope.rs`): the items of
  a positioned box are split off without copying the items of its own
  positioned descendants, and they move back into the list of their
  stacking context as whole chunks. The chunk headers (at most a few per
  positioned box) move once per nesting level: chunks × depth, with the
  depth at most 256 (measured: 250 nested stacking contexts around 50,000
  positioned spans 36 ms, the same spans in one context 12 ms). The
  final list is built in the allocation of the largest chunk (a new
  large allocation on every build cost page faults: twice the build time
  on the Wikipedia fixture). The bounds of
  all groups are computed in one pass over the finished list
  (`paint/src/group_bounds.rs`): each item adds its area to the innermost
  group, a group its mapped area to the enclosing one. A sticky group
  counts with the range of its offsets at any scroll offset
  (`StickyConstraints::offset_range_with`); content fixed to the viewport
  counts apart, in viewport coordinates (`fixed_bounds`). Exception: a
  mask with `no-clip` reads the items of its content once for its area
  (`mask::group_area`), so nested `no-clip` masks cost items × nesting
  depth, at most 256 (the box tree depth limit). Measured with
  240 nested boxes around 50,000 spans: `translateX` 1,064 ms before, 31
  ms now (baseline without positioning 13 ms); sticky 1,223 ms before, 31
  ms now (baseline 362 ms). 2,000 opacity groups around tiny fixed boxes:
  raster 7.8 s per frame before, 2 ms now.

### Not supported

- 3D transforms, `perspective`, `transform-box`, the individual
  `translate`, `rotate` and `scale` properties; `will-change`, `filter`
  and `contain` (in Chromium they make containing blocks of absolutely
  positioned and fixed boxes, and stacking contexts).
- An absolutely positioned child of a table row, row group or table is
  wrapped in an anonymous cell by the table fixup (Chromium places it at
  the start of the part): its static position differs. It needs
  placeholders in table parts.
- Sticky boxes inside a viewport-fixed box are computed against the
  static viewport rectangle.
- Opacity layers have a memory budget (ADR 0018) but no work budget:
  many large overlapping opacity groups are slow to rasterize (as before
  positioning; a group around content fixed to the viewport is now as
  large as its content).
- An absolutely positioned box inside a float in a positioned inline box
  uses the next containing block (no inline box parts come before the
  float to find the inline box); it scrolls with the inline box
  (`in_positioned_inline`). A block inside more nested inline boxes than
  get parts (box construction bounds them) does the same if the
  positioned one has no part.
- Grid areas as containing blocks (CSS Grid 2 §9.4: an absolutely
  positioned child of a grid container that is its containing block uses
  the area of its grid lines, `auto` and lines outside the grid giving
  the padding edges). It needs the line names of the grid (inside
  `grid::placement` now), a resolution of the lines of absolutely
  positioned children that adds no implicit tracks, and the positions of
  the lines after content alignment.
- A `clip` rectangle of an ancestor outside a sticky or fixed box, which
  is repeated for a fixed descendant inside that box, moves with that
  box's offset (Chromium keeps it in place).
- If a stacking context root lies between an absolutely positioned box
  and its containing block, the overflow clips of the boxes between the
  containing block and that root still clip the box (they enclose the
  whole stacking context); the root's own clip does not. The selection
  clip follows the same rule (`forms_stacking_context`, shared with
  paint).
- Text selection uses layout positions in transformed, stuck sticky and
  scrolled fixed boxes (ADR 0009).
- `clip-path`; `background-attachment: fixed` stays as before.

## Consequences

- Wikipedia: the missing element boxes go from 542 to 0, `size` from 0.6175
  to 0.7665 and `relative` from 0.4587 to 0.6071. The pixel score drops by
  0.0012 (0.8858 to 0.8846): the title bar line, the tab underlines and
  the search icon are now painted, but where the rest of the layout puts
  them; grid layout moves them to Chromium's places. Hacker News and
  senko.net keep geometry 1.0 and their pixel scores.
- Layout tests for absolute positioning, fixed, sticky and transforms,
  and reductions of the Wikipedia structures; `flex-abspos-child` passes.
  Engine tests check masks with transforms, fixed and sticky boxes in
  masked boxes, and a full-page screenshot with fixed, sticky, rotated
  and masked boxes across the boundary of two strips, against colors
  measured in Chromium screenshots of the same pages.
- The layout of the page includes the content of absolutely positioned
  boxes that was dropped before (Wikipedia's hidden dropdown menus,
  including the list of 145 languages): layout takes 1.4 to 2 ms more on
  Wikipedia (about 9.0 to 10.6 ms), of which the out-of-flow walk is about
  0.1 ms; the display list and rasterization about 0.2 ms more each.
  Pages without absolutely positioned boxes do not pay for the
  out-of-flow walk (their layout time is unchanged). The display list
  costs each box the transform-group and scroll-state checks: on the
  fixtures 0.107 ms instead of 0.093 ms (Hacker News), 0.014 ms instead
  of 0.012 ms (senko.net), 0.92 ms instead of 0.78 ms (Wikipedia, with
  the positioned boxes that it now paints).
