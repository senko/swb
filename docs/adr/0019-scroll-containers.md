# ADR 0019: Scroll containers

- Status: accepted
- Date: 2026-10-04

## Context

Until M3, `overflow: auto | scroll | hidden` only clipped: the user could
not scroll the content of an element. Only the viewport scrolled. Target 3
(Wikipedia "Web browser") has scroll containers: the table of contents
(`overflow: hidden auto; max-height: calc(100vh - 48px)`), the dropdowns
(`overflow: hidden auto`, the language list with `max-height: 65vh`),
`pre` with `overflow-x: hidden`, `.vector-body-before-content`, headings
and `blockquote` with `overflow: hidden`, and on narrow screens wide
tables (`display: block; overflow: auto`). Pages that put the whole page
in a scrolling body (`html { overflow: hidden }`, `body { overflow: auto;
height: 100% }`) did not scroll at all (M0 backlog).

Specification: CSS Overflow 3 (<https://www.w3.org/TR/css-overflow-3/>),
CSSOM View for scrolling into view
(<https://drafts.csswg.org/cssom-view/#scroll-a-target-into-view>).
Chromium is the reference where the specification leaves room. The
Chromium references hide scrollbars (headless shell).

## Decision

### Which boxes scroll

- A box is a scroll container if its `overflow-x` or `overflow-y` is
  `hidden`, `scroll` or `auto` (§3). Style already computes `visible`
  to `auto` and `clip` to `hidden` when the other axis is scrollable.
  `overflow: clip` clips but does not scroll.
- Layout gives a scrollable overflow rectangle to block containers,
  flex containers and grid containers. Replaced elements, tables, table cells and form
  controls only clip (text fields and text areas scroll their text
  themselves, ADR 0013). Deviation: Chromium scrolls table cells with
  `overflow: auto`; table layout changes the size and the content
  position of a cell after its layout, so its rectangle would need a
  second computation.
- The root's (or the propagated body's) overflow applies to the viewport,
  as before; that box is not a scroll container. On a viewport axis with
  `hidden` (or `clip`), the scroll size keeps the content extent:
  scripts, fragment navigation and scroll into view scroll it, the wheel
  and the keys do not, and it gets no indicator (as in Chromium;
  `FragmentTree::viewport_overflow`).
- Pseudo-element boxes can be scroll containers in layout, but they do
  not scroll (the engine keys offsets by element).
- `hidden` scrolls only programmatically (`dom.scrollTo`, scroll into
  view); the wheel and the keys skip it, as in Chromium.

### Scrollable overflow (layout)

`BoxFragment::scrollable_overflow` holds the scrollable overflow
rectangle of a scroll container, relative to its border box
(`layout/src/scroll.rs`). Layout computes it when it lays out the box
(`block::layout_sized`), as Chromium computes it (we measured
`scrollWidth`, `scrollHeight` and the largest `scrollTop` in Chromium 148
for about 70 cases). It is the union of:

1. the padding box;
2. the in-flow content with the padding around it (the "end padding" of
   §2.2): the margin boxes of the in-flow children and floats (for
   floats in inline content, see ADR 0015) before relative positioning
   (the right margin of an over-constrained block
   with its specified value), the line boxes up to the end of their
   content (with `text-align`; the end margins of inline boxes count),
   and the auto content height (so the collapsed end margins of the last
   child count). Flex items count with their margin boxes. The in-flow
   content of a grid container is its tracks after content alignment;
   its items count only as descendants, without margins (measured in
   Chromium, `tests/layout/grid-scroll.html`). A block inside
   an inline box counts only as a descendant (Chromium's block-in-inline
   puts it in a line box);
3. the border boxes of the descendants for which the scroll container is
   in the chain of containing blocks, without margins or padding, with
   their own content if their overflow is `visible` (limited to their
   padding box on an axis with `overflow: clip`). Nested scroll
   containers count with their border box. Text counts with its
   rectangle (the content area of the font, which can be taller than the
   line); inline boxes with their border boxes (their vertical padding
   too). The spaces and tabs that hang at the end of a line with
   `white-space: pre-wrap` do not count, also where an inline box covers
   them. They hang at a soft wrap, and at a forced break or the end of the
   content only if they do not fit (CSS Text 3 §4.1.3). On a line where
   they hang, all text and inline boxes of the line count only up to the
   end of the line's content in unshifted line coordinates (also inside
   relatively positioned inline boxes and after negative margins; atomic
   inlines count whole), as in Chromium 148. Inline layout marks the
   inline boxes of such a line (`BoxFragment::hanging_from`). The
   viewport's scroll size uses the same rule. Relatively positioned boxes
   otherwise count at their shifted position. Absolutely positioned boxes
   count only if a positioned box between them and the scroll container
   (or the scroll container itself) contains them; fixed boxes never.
   Boxes with zero width or height do not count in a scroll container;
   the viewport's scroll size counts their position (Chromium counts the
   line boxes of zero-size inline content there, but not a zero-size
   block).

   Deviations: an empty inline box with padding after the hanging spaces,
   or white space with `white-space: pre` after them, makes the hanging
   spaces before it count too (Chromium: nothing after the line's content
   counts; swb's inline layout ends the hanging width at the start of the
   next inline box). The max-content width of `pre-wrap` text leaves out
   its trailing spaces and tabs (Chromium counts them). An absolutely
   positioned box inside a nested scroll container (or another box that
   clips both axes) whose containing block is outside it does not count
   in the outer scroll container (Chromium counts it): the walk stops at
   boxes that clip both axes, so that each fragment is visited once per
   layout of its nearest scroll container. The viewport's scroll size
   counts it (ADR 0016). Zero-size inline content nested in a block inside a scroll
   container does not count (Chromium counts its line box); trailing
   `pre-wrap` white space that fits in a relatively positioned inline box
   does not count (Chromium counts it).

A positioned inline box around blocks (block in inline) is the
containing block of the absolutely positioned boxes in it, but its
fragments are siblings of the blocks, not their ancestors. Box
construction marks the blocks inside a positioned inline box
(`BlockInInline::in_positioned_inline`, copied to
`BoxFragment::in_positioned_inline`; also floats and absolutely
positioned boxes there); the scrollable overflow, the scroll offsets
(`ScrollState`), paint's clips and the selection clip treat such a block
as inside its containing block. For an absolutely positioned box deeper
inside such a block, paint and the selection keep the clips outside that
block (a containing-block clip marker). Positioned layout uses the
mark too (ADR 0016): the inline box is the containing block of the
absolutely positioned boxes in such a block.

The parts above and left of the padding box are the unreachable
scrollable overflow region (swb has only left-to-right, top-to-bottom
writing modes), so the rectangle starts at the padding box and the
scroll range is `0 ..= overflow end - padding box end` on each axis.
Scrollbars take no space (the references hide them), so `overflow:
scroll` lays out like `auto`.

The in-flow extent comes from the layout algorithms through
`ChildrenLayout::inflow` (block flow, inline layout's
`InlineLayout::content_right`, flex). Grid layout builds its own
fragment (like tables) and passes the end of its tracks to
`scroll::scrollable_overflow` itself (ADR 0017). A new formatting context
must provide its in-flow extent in one of these ways.

### Scroll offsets and which boxes move

- The engine keeps one offset per scroll container, keyed by the
  element's `NodeId` (`engine/src/scrollers.rs`). The offsets survive
  every layout (restyle, resize); after each layout the engine clamps
  them to the new ranges and drops the offsets of elements that are no
  longer scroll containers. A new document starts with none. Session
  history keeps only the viewport position. A full-page screenshot keeps
  the layout of the viewport (ADR 0016), so the offsets stay.
- Scroll offsets of elements and of the viewport are whole CSS px
  (halves round up), as in Chromium 148, also at higher device pixel
  ratios. The largest offset is a whole number too: the rounded content
  size minus the rounded scrollport size (`swb_layout::scroll_range`;
  content of 200.4 px in 100 px gives 100, a page of 1200.7 px in a 600 px
  viewport gives 601, as in Chromium).
- The fragment tree stays immutable. `swb_layout::ScrollState` applies
  the offsets during a tree walk (`FragmentTree::walk_scrolled`,
  `element_boxes_scrolled`, paint, the selection walks): the content of a
  scroll container moves by minus its offset; its own background and
  border stay. An absolutely positioned box does not move with the scroll
  containers between it and its containing block (the nearest positioned
  or transformed ancestor); a fixed box moves only with the scroll
  containers above its nearest transformed ancestor (with none without
  one; ADR 0016). The scroll
  chain of a node (`Scrollers::chain`) follows the same rule on the DOM
  ancestors.
- Element boxes (`dom.box`, `dom.boxes`, `--dump-boxes`) have the
  offsets applied, as Chromium's `getClientRects`, with transforms and
  fixed and sticky positioning (ADR 0016). Scroll into view uses the same
  boxes and scrollports. `--dump-layout` prints the positions of the
  layout, without scroll offsets.
- The element whose scroll position is the viewport's
  (`document.scrollingElement`, CSSOM View) is the root element; in quirks
  mode, the body if it is not a scroll container (the root element then
  does not scroll). `dom.scrollTo` and `dom.scrollInfo` on it use the
  viewport.

### Paint and hit testing

- The display list builder translates the children of a scroll container
  by its offset inside its clip (the padding box). The display list is in
  document coordinates, so a scroll offset change rebuilds the display
  list (0.8 ms on the Wikipedia fixture), not the layout. Hit testing uses
  the display list, so it uses the same offsets and clips. The hovered
  link after a scroll needs that hit test, so each scroll builds the
  display list once. The GUI collects the wheel events of a batch of
  window events and scrolls once per batch, before it waits for the next
  events (with whole pixels; the fraction stays for the next batch), and
  redraws only if something scrolled. So a scroll container's display
  list is built at most once per batch.
- `background-attachment: local` is not supported (painted as `scroll`).

### Input

- Mouse wheel (`Page::wheel`, `input.wheel`): the innermost scroll
  container under the pointer that can scroll in the direction of the
  delta gets the whole delta (clamped; an axis with `overflow: hidden`
  does not move); otherwise the next one in the scroll chain; otherwise
  the viewport. The rest of a delta does not go to the next scroller.
  Deviation: Chromium keeps (latches) the scroller for
  a whole wheel gesture; swb picks it again for each event, because the
  engine has no clock. With a touchpad, the outer scroller then starts as
  soon as the inner one reaches its end.
- Keys (arrows 40 px; Page Up/Down and Space 87.5 % of the scrollport
  in whole pixels, truncated, as Chromium's `ScrollableArea::PageStep`;
  Home, End): the first scroller in the chain of the focused element, or
  without one of the node of the last primary-button press, that can
  scroll in the key's direction; otherwise the viewport. Chromium does
  the same (`ScrollManager::LogicalScroll`).
- Scroll into view (`Page::reveal`): each scroll container in the chain,
  from the innermost, then the viewport, aligns the box in its
  scrollport; after each container, the next one reveals the part of the
  box inside the scrollport of the previous one (as Blink's
  `ScrollRectToVisible`). Alignment: focus and automation clicks use
  Chromium's "center if needed" (32 px are enough horizontally); fragment
  navigation uses `block: "start"`, `inline: "nearest"` (the viewport no
  longer jumps to x = 0). `overflow: hidden` containers scroll too.
  Automation clicks on a node scroll when the center of its box is
  outside the viewport or hidden by a scroll container; if the center is
  still hidden after that (a box larger than its scrollport), they click
  the center of the visible part of the box.
- While a page loads, the engine scrolls to the fragment of its URL after
  every layout. A scroll by the user, by `dom.scrollTo` or by scroll into
  view (focus, automation clicks) cancels that, for scroll containers as
  for the viewport.
- Text selection: the position at a point uses the scrolled positions
  and skips text that clipping boxes hide, with the clip rules of paint
  (an absolutely positioned box is clipped by its containing block and
  the boxes above it; a fixed box is not clipped).

### Scroll indicators

The GUI shows overlay scroll indicators (`swb_paint::scroll_indicators`):
a thumb of 5 px, 2 px from the right edge (vertical) and the bottom edge
(horizontal), dark with a light outline, on every axis that the user can
scroll and that has a range. They take no space and cannot be dragged.
Scroll containers get them in the display list (inside their clip, above
their normal-flow content and the positioned descendants of their own
stacking context; positioned descendants that paint in an outer stacking
context paint above the indicator); the viewport gets the same
indicator, drawn over the page
by `Page::render`, so that every scrollable area looks the same. The GUI
had no viewport scrollbar before. Headless screenshots and comparisons
have no indicators (`Page::set_scroll_indicators`, off by default),
because the Chromium references have no scrollbars.

### Automation and tests

- Protocol additions: `dom.scrollInfo`, `dom.scrollTo`, `input.wheel`
  ([automation.md](../automation.md)); Python client methods
  `scroll_info`, `scroll_element_to`, `wheel`.
- Layout tests can scroll elements before the boxes are read
  (`data-scroll="X Y"`, in Chromium and in swb; [testing.md](../testing.md)).
  `tests/layout/scroll-*.html` test the scroll ranges against Chromium
  (scrolled to the end) and reductions of the Wikipedia structures.

### Limits for hostile input

- The scrollable overflow walk of a scroll container visits its subtree
  down to the nested boxes that clip both axes; only the walk of its
  nearest such scroll container visits a fragment, so the work per layout
  of a box is linear in its subtree, like the layout itself (flex and
  table caches bound repeated layouts, ADR 0010). The box depth limit
  (256) bounds the recursion depth.
- The engine clamps offsets to `0 ..= max` (NaN becomes 0); ranges are
  finite (each length is within ±33,554,431 px, but the content of a
  scroll container can add up to more). It stores only the non-zero
  offsets of existing scroll containers.
- A scroll chain is at most the DOM depth (512). Scroll into view and the
  automation visibility check walk the fragment tree once each (linear).
- The engine ignores wheel deltas that are not finite (automation
  rejects them with -32602).

### Not supported

Scroll latching for wheel gestures; `overscroll-behavior`;
`scroll-behavior: smooth` (scrolling is instant); scroll snapping;
`scroll-padding` and `scroll-margin`; classic scrollbars that take space,
`scrollbar-gutter`, `scrollbar-width` and `scrollbar-color`; dragging the
indicators; `background-attachment: local`; `overflow-clip-margin`;
right-to-left and vertical writing modes and the start-edge overflow of
`row-reverse` and `column-reverse` flex containers; scrolling
pseudo-element boxes; keyboard-focusable scroll containers (Chromium
makes scrollers without focusable content Tab stops); restoring element
offsets from the session history; auto-scrolling while a selection drag
leaves a scroller; scrolling text areas and list boxes with the wheel
(backlog of ADR 0013).

## Consequences

- Every layout walks the fragment tree once more (to update the scroll
  ranges); every scroll offset change rebuilds the display list.
- Boxes that a layout algorithm adds to a fragment after
  `block::layout_sized` are not in its scrollable overflow; such an
  algorithm must add them (the out-of-flow pass of ADR 0016 does, with
  `scroll::with_out_of_flow`).
- The containing block rule for scrolling is in `ScrollState` and
  `Scrollers::chain` (absolutely positioned: the nearest positioned or
  transformed ancestor; fixed: the nearest transformed ancestor, or the
  viewport; ADR 0016). Paint's clip rule for positioned boxes is
  separate (ADR 0016: the clips of the containing block chain; a fixed
  box is not clipped by scroll containers).
