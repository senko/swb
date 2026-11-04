# ADR 0033: backdrop-filter

- Status: accepted
- Date: 2026-11-04

## Context

`backdrop-filter` (Filter Effects Module Level 2,
[the property](https://drafts.fxtf.org/filter-effects-2/#BackdropFilterProperty),
[backdrop root](https://drafts.fxtf.org/filter-effects-2/#BackdropRoot))
applies the `filter` functions to what is painted behind a box. The
Ars Technica dialogs (grid settings, sign-in, the third overlay) use
`bg-slate-900/80 backdrop-blur` on a `fixed inset-0` box: the page behind
the dialog is dimmed and blurred. swb treated the property as inert, so the
dialog showed the sharp page (pixels of the dialog scene differed from
Chromium's around the dialog). The GitHub fixture and the BBC page also
declare it.

The specification leaves several details open (the area, the edges of a
blur, the order with the box's own effects, the backdrop root of some
properties). This ADR records what Chromium 148 does (black-box
measurements, `tools/probes/backdrop-filter.json`, 131 cases, three equal
runs; no Chromium source was read, ADR 0021) and what swb does. The filter
steps are those of ADR 0028 (feature 12c notes); this ADR adds the paint
model.

## Decision

### Style

- `backdrop-filter` has the grammar and the computed value of `filter`
  (shared code: `properties/filter.rs`, `cssom/filter.rs`;
  `ComputedStyle::backdrop_filter`). `none` is the initial value; the
  property is not inherited. At most 64 functions (ADR 0028 section 16).
  `CSS.supports`, `element.style`, `getComputedStyle` and the cascade
  agree with Chromium on 400 values (`bf-syntax-*`, `bf-cascade`).
- `-webkit-backdrop-filter` is not a property in Chromium 148
  (`CSS.supports` is false, a declaration is dropped, `webkitBackdropFilter`
  on a style object is a plain expando). swb has no alias. (Ars Technica's
  script sets it on a style object and relies on nothing.)
- A value other than `none` (also `url()` and identity functions) makes the
  element a stacking context and the containing block of its absolutely
  positioned and fixed descendants, as `filter` does, except for an inline
  box and the root element (`bf-cb-*`, same as `fcb-*` of ADR 0028).
  `ComputedStyle::has_filter_effect` is the test for `filter` or
  `backdrop-filter`; layout, paint, `offsetParent` and `DOMMatrix`
  geometry use it. This changes layout and painting with JavaScript off.
- `will-change` with `opacity`, `filter`, `backdrop-filter`, a mask or
  `clip-path` makes a stacking context (CSS Will Change 1 §3). `filter` and
  `backdrop-filter` in `will-change` also make the containing block of
  absolutely positioned and fixed descendants (same exceptions: inline
  box, root element); `opacity`, `clip-path`, `mask` and `mask-image` do
  not (`bf-cb-willchange-*`, measured with the boxes, `offsetParent` and
  pixels; a list such as `top, filter` counts). The style computes both
  facts once when it computes `will-change` (`WillChangeEffects` in
  `properties/inert.rs`; `ComputedStyle::will_change_makes_stacking_context`
  and `will_change_makes_containing_block`); layout, `offsetParent` and the
  IntersectionObserver geometry use `is_filter_containing_block`.
  `will-change: transform` is still inert: Chromium 148 also makes a
  containing block for `transform` and `perspective`, and for absolute
  boxes for `position` (departure, backlog).

### The backdrop

- The backdrop of a box is what is painted before it inside its backdrop
  root, clipped to the **border box** (the border is inside:
  `bf-area-border-box`), and drawn through the steps. Children and later
  siblings are not in it; boxes with a lower or equal z-order painted
  before are (`bf-order-zindex`). `visibility: hidden` and `opacity: 0`
  draw nothing.
- **Backdrop roots** (measured, `bf-rootkind-*`): the root element,
  `opacity` below 1, any `filter` (also `invert(0)`, `blur(0)`, a missing
  `url()`), a mask, a `clip-path` shape, `mix-blend-mode`, a
  `backdrop-filter` (also `brightness(1)`), and `will-change` of one of
  these. Not roots: `transform`, `will-change: transform`, `isolation`,
  `position: fixed`, `overflow`, `contain: paint`, `z-index`, `opacity: 1`.
  A root hides what is painted outside it: the backdrop of a box inside is
  what the root painted before the box. A root with a `backdrop-filter`
  does not put its own filtered backdrop into the backdrop of its
  descendants (`bf-root-own-backdrop`: the descendant sees the root's
  background only).
- **Edges.** A blur does not fade at the border of the box and does not
  read the backdrop beyond it: the image is mirrored at its edges
  (`abcd|dcba`, `bf-edge-*`). The mirror is made once, before the three
  box passes of the blur (`bf-edge-stripe-*`: mirroring before each pass
  differs by 2 to 8 levels for an even window). Chromium takes the content
  beyond the viewport edge when the box reaches beyond it
  (`bf-edge-page-right`); a box that ends at the viewport edge (Ars
  Technica) mirrors there as swb does.
- **Compositing.** The filtered backdrop is drawn *over* the backdrop root
  (`opacity(0)` leaves the backdrop, a semi-transparent backdrop stays
  semi-transparent: `bf-root-opacity-0`), then the box paints. The area
  has the rounded corners of the border box (anti-aliased). The box is
  snapped to device pixels (the area of a rounded box takes the pixels it
  touches).
- **The box's own groups.** `clip-path`, a mask and `filter` on the box
  apply to the filtered backdrop and the box together (`bf-self-*`: the
  clip path cuts the backdrop, `filter: invert(1)` inverts the filtered
  backdrop). The `opacity` of the box applies to the filtered backdrop and
  to the box **separately**: the backdrop is drawn with the opacity over
  the page, then the box's opacity group over that
  (`bf-self-opacity.5-bg`: 208 where one opacity group gives 225). The
  display list puts the backdrop item before the opacity group (with the
  opacity as `alpha`) when the box has no mask, clip path or filter; with
  one of them the group holds both (departure, not measured).
- **Transforms.** The blur follows the transform of the box: the filter
  is applied in the box's coordinates, so `scale(1.5)` has a blur of 1.5
  times the radius (`bf-transform-scale1.5`).

### Paint

- `DisplayItem::BackdropFilter` (`paint/src/display_list.rs`): the
  functions, the `color` of the element, the border box, its radii (if
  rounded), `alpha` and the number of groups that the box started before
  (`layers`). The builder pushes it after the box's groups and its `clip`
  property clip and before its own painting (`open_backdrop`, called by
  `box_contents` and `inline_box`). The root element has none.
- A backdrop root that has no layer of its own but has a `backdrop-filter`
  somewhere below gets one: the builder finds these boxes in one walk
  (`backdrop_roots`) and wraps the box's content, after its own backdrop
  item, in an opacity group with opacity 1.0. A page without a
  `backdrop-filter` pays for the walk only.
- `raster/backdrop.rs`: finds the backdrop root surface (the nearest
  layer below the `layers` innermost ones that is not a direct layer, or
  the target; a rounded `overflow` clip layer is no root: its pixels are
  drawn over the root's), copies the part of the border box that is
  inside it and visible, grown by the reach of the steps, runs
  `crate::filter::apply_mirrored`, and draws the result over
  the current surface with a pattern fill inside the clips (a rounded path
  with anti-aliasing for rounded corners).
- `Edge::Mirror` (`paint/src/filter.rs`, entry `apply_mirrored`, which
  needs no scratch buffer; `apply` with `Edge::Transparent` needs one):
  `box_blur_mirrored` extends the lines by the reach of the blur with the
  mirrored pixels, blurs them and crops the result: `blur_rows_mirrored`
  for the rows, then `blur_columns_mirrored` for the columns
  (`blur_columns_padded`, or `blur_columns_blocked` for a large image).
  The memory is bounded (two buffers of at most 4 Mpx for the vertical
  passes; a larger image uses blocks of 16 columns).
- Budgets: the backdrop counts as a filter group (`filter_layer`): its
  pixels count against the layer budget (twice for a blur), its work
  against the filter work budget (128 Mpx or eight times the strip). A
  backdrop that does not fit stays unfiltered and the rasterizer logs the
  usual warning once per frame. The standard deviation is limited to 256
  device px as in `filter`.
- The GUI renders the viewport with the same path in every frame, so a
  scroll under a fixed box repaints its blurred backdrop.

### Departures

- A box inside a transform group that rotates, scales or skews (a layer
  with its own rasterizer) sees an empty backdrop and draws nothing; the
  page stays sharp there (`bf-transform-*`, in `SWB_PIXELS` of the test).
  Chromium blurs the page behind the transformed box. Translations (fixed
  and sticky boxes, `translate()`) are exact. Backlog.
- `scale(.5)` of a box: Chromium 148 mirrors the black half of the
  backdrop over the box (a rendering fault: `bf-transform-scale.5`, and
  `scale(.75)` has a wrong profile); swb does not reproduce it.
- The backdrop stops at the edge of the surface: the viewport in a window
  or viewport screenshot, the page in a full-page screenshot. A full-page
  screenshot also stops at the boundary of its strips (the strips are the
  height of the viewport or more), so a box across a boundary mirrors
  there. Chromium reads the content beyond the viewport when the box
  reaches beyond it, so a viewport frame of swb differs from Chromium and
  from swb's own full-page screenshot of the same page. In the GUI, a box
  taller than the window shows a band of mirrored blur at the window
  edges that changes while the page scrolls (review case: 30 px stripes,
  a 200 by 1500 px box with `blur(8px)`, viewport 400 by 300, pixel
  (100, 295): viewport frame 255, full page 183). A box that ends at the
  viewport edge (Ars Technica's `fixed inset-0` overlay) is not affected.
  A fix paints the backdrop root beyond the visible area for the reach of
  the blur (backlog).
- `drop-shadow()` at the edge of the box. Chromium 148 mirrors the
  backdrop once and runs the whole function list on the extended image
  (measured: `bf-fn-drop-shadow-mirror-read`, a shadow moved by -30 px
  shows the mirrored square beyond the right edge; `bf-fn-drop-shadow-edge-blur`,
  `drop-shadow(30px 0 6px blue) blur(2px)`: the shadow's tail beyond the
  edge feeds the following blur, 105 against swb's 94 at 2 px from the
  edge). swb mirrors each blur by itself and crops after each function,
  so a shadow reads transparent pixels beyond the edge, and a function
  after a moved shadow sees a cropped tail. `blur()` lists and a
  `drop-shadow()` without an offset match (`bf-fn-drop-shadow-edge-zero`).
  A fix extends the image once by `backward_reach` of the whole list and
  works on the extended square; that costs the corners (a 60 px header
  with `blur(20px)` would work on 3 times its pixels) and a second budget
  check, so it is not done for a rare function (backlog; the two rows are
  in `SWB_PIXELS`).
- `mix-blend-mode` is not a property in swb, so it is no backdrop root.
- The layer of a rounded `overflow` clip is no root (as in Chromium), but a
  clip layer's pixels are drawn without the rounded mask into the backdrop
  (the corners).
- With a mask, clip path or filter on the same box as `opacity`, one
  opacity group holds the backdrop and the box (see above).
- `box-shadow` is not implemented (not part of this feature; it was left
  out of the probe).
- Inline boxes: the backdrop item is per box fragment (one line); a box of
  several lines was not measured (backlog).

## Tests

- `crates/engine/tests/backdrop_filter.rs`: every case of
  `tools/probes/backdrop-filter.json` against Chromium's values
  (`crates/engine/tests/data/web-backdrop-filter.json`, tolerance 2). The
  eight rows where swb departs are listed with their reasons.
- `crates/paint/src/raster/tests/backdrop.rs` and
  `crates/paint/src/filter/tests.rs` (unit tests of the rasterizer and of
  the mirrored blur, with Chromium's edge values).
- `crates/automation/tests/ars_technica.rs`: the pixels of the dimmed and
  blurred page around the grid settings dialog (21 points, within 3 levels
  of Chromium).
- Hostile pages: `backdrop-filter-nested`, `backdrop-filter-viewport-huge`,
  `backdrop-filter-many-in-long-page`.

## Consequences

- Ars Technica's dialogs match Chromium's pixels. The frame with a
  full-viewport `blur(8px)` takes 36 ms (release, 1280 by 800; 6.5 ms
  without the dialog): the six passes of the blur are scalar and take
  about 3 ns per pixel and pass.
- The JS-off snapshot of all fixtures is identical to the one before; the
  six JS fixtures score as before.
