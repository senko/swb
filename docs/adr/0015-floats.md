# ADR 0015: Floats, clearance and block formatting contexts

- Status: accepted
- Date: 2026-10-04
- Updated: 2026-10-07 (provenance): the exclusion space is swb's own
  segment model; an earlier version of this ADR called it "Chromium's
  shelves", which was wrong. The decision did not change.

## Context

Target 3 (Wikipedia "Web browser") uses floats for thumbnails
(`figure` with `display: table; float: right; clear: right`), the
infobox, the page header and the footer lists; headings with
`display: flow-root` and `overflow: hidden` next to thumbnails;
paragraphs, hatnotes and lists that wrap around floats; and clearfix
`::after { clear: both }` boxes. Until M3, floats were placed at the
current position without any effect on the flow, floats inside inline
content were dropped, and `clear` was ignored.

The specs: CSS 2.2 §9.4.1 (block formatting contexts), §9.5 (floats),
§9.5.1 (placement rules), §9.5.2 (clearance), §10.3.5 (float widths),
§10.6.7 (heights of BFC roots), CSS Display 3 (`flow-root`), Appendix E
(paint order). Where they leave room, Chromium (LayoutNG) is the
reference. Its behaviour was measured with small probe pages; the
layout tests `float-*` record the results.

## Decision

### Exclusion space (`layout/src/floats.rs`)

Each block formatting context (BFC) has an exclusion space in the
coordinates of its root's content box: two step functions over the block
axis, the rightmost right margin edge of the left floats and the
leftmost left margin edge of the right floats. A sorted vector of
segments holds them (swb's own model, not Chromium's data structure). It
also records the top of the last float (rule 5) and the bottom of the
lowest left and right float (clearance). The free range at a block
position, clipped to a containing block, is one interval.

Layout opportunities are produced one at a time (`Bfc::opportunity_at`,
`narrowing_below`, `narrower`, `next_top`): first the free range at a
position. How far down it reaches is checked only for the height of the
line or box that is placed, so a line visits only the segments beside
it, not all floats below it. If the line or box is too tall, the next
opportunity at the same position is narrower and reaches past the
segment that narrowed the first one; if it is too wide, the next
position is where the free space changes. Each visited segment costs
one unit of the work budget, and a search for a position the segments
that a binary search visits (see the limits below).

`LayoutContext` keeps a stack of BFCs. Every box that establishes a BFC
(floats, inline-blocks, table cells, flex items, scroll containers,
`flow-root`, controls, the root) pushes one for its contents
(`block::layout_flow_root`); the auto height of such a box includes the
bottom of its floats (§10.6.7).

### Float placement

A float is laid out once with its shrink-to-fit width. It is placed at
the first block position at or below its origin, the top of the last
float and its clearance offset where its margin box fits beside the
floats; a left float goes to the left edge of the free range, a right
float to the right edge. Because no float starts below the top of the
last float, the free range at a position does not narrow further down,
so only that position is checked. If no position has room, the float
goes below all floats that narrow the containing block (it may then be
wider than the containing block). A float among blocks has its origin
at the next border edge (the position after the margins so far, as in
Chromium's `NextBorderEdge`). A float with a negative margin-box width
(negative margins) is placed by that width but excludes nothing.

### Positions of boxes whose margins can still collapse

Line boxes and BFC roots need their position in the BFC while their
container is laid out, but margin collapsing (§8.3.1) decides a box's
position only at its first content. Block layout therefore passes each
child the position where its margins start and the margins so far
(`ChildPlace`, `FlowY::Pending`). A block without a top border or
padding pushes a frame on the BFC and stays unresolved; the first
content (a non-empty line box, a box with a top border, a BFC root, a
block that ends with content or height) resolves the whole chain of
unresolved frames at once (`Bfc::resolve`).

Floats inside unresolved containers wait in their frame. When the chain
resolves, they are placed at its position, outer frames first; their
fragments are moved by a path of child indices. If the chain ends in an
empty block (its margins collapse through it), its floats are placed at
the position of that block, which is where it would be with a bottom
border (§8.3.1); if its parent is unresolved too, they move to the
parent's frame. This gives the same results as Chromium, which places
such floats speculatively and lays the subtree out again when the BFC
block offset changes, but without the second layout.

The top border edge of an empty block now uses only the margins before
it and its own top margin (not its bottom margin), as in Chromium.

### Clearance

A block with `clear` gets the clearance offset of the floats on the
cleared sides. When its chain resolves, the frames are checked from the
inside out: a frame needs clearance if its clearance offset is below its
hypothetical position, which is the position after all margins, or,
if an inner frame needs clearance, the position before that frame's
margins. The outermost frame that needs clearance is placed at its
clearance offset (the frames inside it at the same position or lower
at their own), and the containers outside it end before its margin.

If the block (or BFC root) would clear a float that waits in the chain
(Chromium's `HasClearancePastAdjoiningFloats`), the chain is resolved
before the block's margins, which places the waiting floats, and the
block is placed exactly at its clearance offset, or at the position
before its margins if that is lower (Chromium's forced BFC block
offset): its top margin, and the margins of its descendants that
collapse with it, have no effect. For example, a block with
`margin-top: 80px` and `clear: left` after a 50 px float in the same
unresolved container is at 50, not at 80. If the block with `clear` is
inside another unresolved block, that block is placed first (after its
own margins), then the float, then the block at the float's bottom.

Chromium behaviours that the spec does not state:

- An empty block with `clear` is placed at its clearance offset or its
  hypothetical position, whichever is lower, whenever floats on the
  cleared side end below the top of its container, even if it needs no
  clearance. The margins before it then do not collapse with the
  margins after it, its own margins collapse with the following ones
  from its top margin edge, and they do not collapse with its parent's
  bottom margin (clearfix boxes keep the last child's bottom margin
  inside the container).
- `<br clear>` moves the next line below the cleared floats.

### Boxes that establish a BFC next to floats

An in-flow BFC root (overflow other than `visible`/`clip`, `flow-root`,
flex containers, tables, block-level replaced elements and controls)
does not overlap floats. As in Chromium's `HandleNewFormattingContext`,
it tries the layout opportunities at and below its hypothetical
position in order and is laid out for each one: an `auto` width fills
the opportunity; its margins overlap the floats and only reduce the
available width where they reach past them; `auto` margins center it in
the opportunity; percentages still resolve against the containing
block; `-webkit-center` and `-webkit-right` align in the opportunity.
It takes the first opportunity where its border box fits: not wider or
taller than the opportunity, and beside the floats on both sides (a
margin can move the border box into a float on the other side). A box
that fits nowhere goes into the last opportunity, where no float narrows
the containing block from that position down, and overflows there. If
floats (not clearance alone) push it down and its container's position
was unknown, its top margin no longer collapses: the container is
placed before that margin and the box is placed at the first opportunity
from there (Chromium's `abort_if_cleared`). These layouts are cached per
box and width; a box is laid out for at most 32 widths, then it goes
below all floats.

### Line boxes

Inline layout breaks one line at a time. Each line is tried in the
layout opportunities at its position, widest first, as in Chromium's
`InlineLayoutAlgorithm`. If its first word does not fit, the line moves
to the next position where the free space changes, unless no float
narrows the containing block at the line's top (Chromium's
`IsEqualToAvailableFloatInlineSize`) or the container does not wrap
lines (`white-space: nowrap` or `pre`; Chromium's `ShouldWrapLine`):
then the line stays and overflows. If its height reaches a float that
narrows the space, the line moves to the next narrower opportunity at
the same position (also in containers that do not wrap); the floats that
the line places itself do not count. Once a line was too wide,
opportunities narrower than its first word are skipped without a try.
Floats at the start of the line are placed before the line. A float
after content is placed on the line if it fits beside the content before
it (the line becomes shorter); otherwise, or if an earlier float of the
line is queued, it is placed below the line (Chromium's
`LineBreaker::HandleFloat`). `text-align` works in the free range, and
outside list markers sit at its left edge, as in Chromium. A float
inside a word (no break opportunity around it) that leaves no room for
the rest of the word ends the line before the word, and is placed again
on the next line (Chromium rewinds its line breaker). A float moves with
the relatively positioned inline boxes around it (its exclusion does
not). Floats and atomic inlines resolve percentages against the
container's content box, its height included. A float belongs to the
inline formatting context around it, also at its start; only if that
content is empty does it become block-level.

### Intrinsic sizes

The min-content contribution of floats is their own. For max-content,
Chromium's rules: floats on one "line" add up; a float or BFC root with
`clear` starts a new line on the cleared sides; a BFC root adds to the
floats before it; any other block ends the line. In inline content,
floats add to the line they are on, and a float that clears an earlier
float of the line starts a new one.

### Box tree and style

- `float` does not apply to flex and grid items: the cascade sets it to
  `none` for them (CSS Flexbox 1 §4), so a float after text in a flex
  container is a flex item.
- An empty inline box (not split by a block) keeps its anonymous block
  and line, so that it has a box, as in Chromium.
- A float inside a positioned inline box carries the mark
  `in_positioned_inline` (ADR 0019), as a float that becomes block-level
  there and as a float in inline content: scrolling and paint's clips
  then treat the inline box as the containing block of the absolutely
  positioned boxes in the float. Positioned layout does not (it finds the
  inline box only from the inline box parts before a block, and a float
  has none): it places those boxes against the next containing block
  (the limitation in ADR 0016).
- An absolutely positioned box among blocks next to floats gets its
  placeholder (ADR 0016) at the current position; floats do not move
  its static position. After a float, an absolutely positioned box stays
  in the inline content (and keeps the anonymous block if the content is
  otherwise empty): it keeps its place in tree order, and an inline-level
  one gets its static position next to the float. The layout test
  `abspos-floats` records the cases.

### Scroll containers

The floats of a scroll container are in-flow content of its scrollable
overflow (ADR 0019) with their margin boxes, as floats among blocks, also
when they are in an inline formatting context without line boxes. Next
to line boxes, Chromium 148 counts a float at the start of a line only
with its right margin edge, and a float after content on a line only
with its border box, as a descendant (Chromium makes it a child of the
line box). The line boxes count up to the end of their content, which
starts after the floats beside them. The layout test `scroll-floats`
records 12 cases.

### Paint order (`paint/src/display_list.rs`)

A stacking context paints its normal flow in three phases (Appendix E
steps 4, 5 and 7): backgrounds and borders of in-flow blocks, then
floats, then inline content. Floats, atomic inline-level boxes, and flex
and grid items (CSS Flexbox 1 §5.4) paint all their phases as a unit,
floats in the float phase and the others in the inline content phase;
their positioned descendants belong to the enclosing stacking context.
The positioned boxes of a stacking context are sorted by z-index and
then by tree order, so that a positioned box inside a float, an
inline-block or an inline box keeps its place in tree order among the
others. The tree order is the pre-order index of the box in the fragment
tree, which a pass before painting records for the positioned boxes (a
path of child indices per box, the first version, doubled the display
list time on deep trees). For this the floats of a line go among the
line's fragments by their place in the content, so that the line's
top-level fragments and its floats are in tree order. A float inside an
inline box is a sibling of the box's fragment, so no place among the
siblings is right for all cases: it goes before the box's fragment (see
"Not supported"). An inline box with opacity < 1 or a mask paints its
own content in a group, like a block. Hit regions follow the same order.

With positioning (ADR 0016), a positioned box reserves its place in the
list of its stacking context when the phase walk reaches it; the walks
carry the scroll state and the ancestry of containing blocks, so that an
in-flow block painted phase by phase (a scroll container among them)
moves its content by its scroll offset and keeps the clips of the
containing block chain. A transformed box paints like a positioned one.
The display list collects the items in chunks (ADR 0016), so the phases
add no copies when positioned boxes move to their stacking context.

An earlier version painted flex and grid items in the background phase,
because painting them later made glibc move the large display lists of
Wikipedia and doubled the display list time. Since the mask changes
(ADR 0018) the display list grows differently and the effect is gone.

### Limits for hostile content

- A BFC places at most `MAX_FLOATS` (10,000) floats with the rules;
  later floats go below all floats of the BFC. It also keeps at most
  10,000 floats that wait for the position of their container; later
  ones are not placed (they stay at the top left of their container and
  exclude nothing).
- All exclusion space work of a layout pass (segment visits, segment
  moves when a float is added, copies of the exclusion space for a
  retry) shares a budget of 20,000,000 (`WORK_BUDGET`). Each further
  layout of a BFC root at another opportunity costs 1,000 per box and
  line box laid out, so that nested BFC roots next to floats, and BFC
  roots with much text, do not take exponential or unbounded time and
  memory; a BFC root is laid out for at most 32 widths. A line is tried
  in at most 64 opportunities (`MAX_LINE_ATTEMPTS`; opportunities skipped
  because they are narrower than the line's first word do not count),
  then it goes below all floats of its BFC; each attempt where it does
  not fit costs 50 per piece of the line (`LINE_RETRY_COST`).
  When the budget is spent, floats, lines and BFC roots are placed below
  all floats of their BFC. The warning is logged once per process.
- A line copies the exclusion space only when it places a float in an
  opportunity that is not the last one (the copy undoes the float if the
  line moves on), and for the rewind of a word only while budget is
  left. After the budget is spent, every opportunity is the last one, so
  nothing is copied.
- The chain of unresolved frames and the paths of waiting floats are
  bounded by the box depth limit (256). A path is stored innermost
  first, so that moving a waiting float to the frame of the parent
  appends one index. The BFC counts its waiting floats per side, so
  that a block with `clear` checks in constant time whether it clears
  one.

Measured in release builds, for the whole page: 100,000 floats in one
BFC 0.2 s; 3,000 floats of 0.01 px with 3,000 paragraphs that negative
margins pull back up 0.04 s; 200 nested BFC roots that each need two
opportunities 0.02 s (budget spent; up to 22 levels the result is
exact); a staircase of 5,000 floats of 0.02 px with 1,000 paragraphs of
100 px lines pulled back over it 0.08 s (28.7 s before the line attempt
limit); 8,000 floated links in a column next to 8,000 paragraphs 0.23 s
(budget not spent; lists of all opportunities at a position spent it);
300,000 floats waiting in 250 nested empty blocks 0.8 s and 330 MB
(10.1 s and 860 MB before the limit on waiting floats; the same page
with blocks instead of floats takes 0.7 s and 200 MB, the rest is the
per-item tables of the inline formatting context that holds the floats).

### Not supported

- Shapes (`shape-outside`), fragmentation, writing modes and right-to-left
  (`inline-start` and `inline-end` map to left and right).
- An empty inline box in an empty line whose container's position is not
  known yet is not moved past the floats before it (Chromium moves it).
- Opacity groups and masked boxes (ADR 0018) that are not positioned
  paint as a unit in the inline content phase (floats in the float
  phase), so their group wraps all their phases; Appendix E paints them
  with the positioned boxes. Outlines paint after each box, and the image
  of a block-level replaced element with its background (Appendix E has
  later phases for them).
- The static position of an absolutely positioned box after a float on
  an empty line of a block whose position is not known yet ignores the
  float (the float waits for the position; the placeholder does not).
- A float inside an inline box paints its positioned descendants before
  the inline box (if it is positioned) and before the positioned boxes
  in the inline box that come before the float: with equal z-index,
  Chromium paints the float's after them.
- An absolutely positioned box with self-alignment after a float in
  inline content without line boxes aligns in the zero-height
  static-position rectangle of the empty line; Chromium does not align
  it.
- A float inside a positioned inline box with a z-index paints in the
  float phase of the inline box's container, not in the inline box's
  stacking context: the float's fragment is a child of the block
  container, not of the inline box.
- An inline box with opacity or a mask that is split over several lines
  gets a group for each line's fragment, not one for the element as in
  Chromium: where the fragments overlap they composite separately, and
  the mask is positioned in each fragment, not in the box that joins
  them.
- `width: min-content` and the other sizing keywords (not floats, but
  layout tests with them fail).

## Consequences

- Block layout knows positions in the BFC; most of the cost is only paid
  when floats exist (lines without floats take the whole width without a
  query; BFCs are reused from a pool).
- The paint phases walk the tree three times per stacking context: on
  Wikipedia the display list takes 1.02 ms instead of 0.83 ms (on
  7581bcd); pages of 64,000 positioned boxes take 1.4 to 1.7 times as
  long as without the phases, and the time grows linearly.
- Later work that adds formatting contexts must push a BFC for the
  contents of their boxes (`layout_flow_root` does it for flow content,
  also for grid items), and new stacking-context properties must make
  `paint_kind` treat such boxes as a unit (as for opacity and masks) or
  as positioned (as for transforms).
