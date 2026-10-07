# ADR 0018: CSS masks

- Status: accepted
- Date: 2026-10-04

## Context

Target 3 (Wikipedia "Web browser") draws its icons with masks: an element
with `background-color: currentColor` (or a theme color) and a
`mask-image` of an SVG icon. This covers `.vector-icon` spans (menu,
search, language, ellipsis, ...) and `::before`/`::after` icons (the
dropdown chevrons, the search field's magnifier). The stylesheets choose
between masks and a fallback with `@supports ((-webkit-mask-image: none)
or (mask-image: none))`. Without masks, `@supports` was false, so swb
used the fallback, and the `.vector-icon` placeholder rule (a 1x1 PNG
mask with a solid background color) painted the icons as solid squares.

Wikipedia uses `mask-image` and `-webkit-mask-image` (`url()` to
`load.php?modules=skins.vector.icons&image=...` SVGs, `data:image/svg+xml`
and `data:image/png` URLs), `mask-size` with `calc(max(var(...)))`,
`mask-position: center` and `mask-repeat: no-repeat`, each also with its
`-webkit-` name.

The specification is CSS Masking 1 §6-7
(<https://www.w3.org/TR/css-masking-1/#positioned-masks>). Chromium is the
reference where browsers deviate or the specification leaves room.

## Decision

### Scope

Masks with images: `mask-image` (`none`, `url()`, `linear-gradient()`
and the other images of `background-image`), `mask-mode`,
`mask-repeat`, `mask-position`, `mask-size`, `mask-origin`, `mask-clip`,
`mask-composite`, the `mask` shorthand, and the `-webkit-` names that
Chromium 148 supports. References to SVG `<mask>` elements (`url(#id)`)
are out of scope (swb has no inline SVG); they paint as an image that
failed to load, as in Chromium when the element does not exist.

### Properties (Chromium's property model)

The names and syntaxes were checked with `CSS.supports()` and
`element.style` in Chromium 148 (the test `supports_like_chromium` in
`style/src/properties/mask.rs` has about 110 cases; the layout test
`mask-supports` checks `@supports` against Chromium).

- Longhands: `mask-image`, `mask-mode`, `-webkit-mask-position-x`,
  `-webkit-mask-position-y`, `mask-size`, `mask-repeat`, `mask-origin`,
  `mask-clip`, `mask-composite`. As in Chromium, `mask-position` is a
  shorthand of the two `-webkit-` position longhands; `mask-position-x`
  does not exist.
- Aliases with the same syntax: `-webkit-mask-image`, `-webkit-mask-size`,
  `-webkit-mask-repeat`.
- Legacy names with another syntax that set the same longhands
  (implemented as shorthands): `-webkit-mask` and `-webkit-mask-clip`
  accept `border`, `padding`, `content` and `text`, not `no-clip` or
  `fill-box`; `-webkit-mask-origin` accepts the first three;
  `-webkit-mask-composite` accepts the Porter-Duff keywords
  (`source-over`, `source-in`, `xor`, `copy`, `clear`,
  `destination-out`, `plus-lighter`, ...), not `add`, `subtract`,
  `intersect` or `exclude`; `-webkit-mask-position` accepts the
  three-value `<bg-position>`, which `mask-position` (`<position>`)
  rejects. The `mask` shorthand also accepts three values.
- `mask` sets the nine longhands. One box keyword sets the origin and
  the clip, two set the origin and then the clip, `no-clip` (or `text` in
  `-webkit-mask`) sets only the clip.
- `fill-box` is stored as `content-box`, `stroke-box` and `view-box` as
  `border-box` (their used values for CSS boxes, §7.5). `-webkit-mask-clip:
  text` is stored as `border-box` (swb does not clip to text, as for
  `background-clip: text`).
- `mask-type` (for `<mask>` elements) is validated (`alpha | luminance`)
  and ignored, so that `@supports` answers as in Chromium. `mask-border`
  is not supported (also not in Chromium); `-webkit-mask-box-image` is not
  supported (Chromium supports it).
- A `mask-image` layer is `None`, an image, `Failed` (an empty URL or a
  fragment-only URL, which never renders) or `Unsupported` (radial and
  conic gradients, `-webkit-gradient()`: valid, but swb cannot draw them).
  Like Chromium, `element()`, `paint()` and `cross-fade()` are invalid.
- An element is masked if a layer is not `none`
  (`ComputedStyle::has_mask`).

### Loading

Mask images load like background images: `StyleMap::image_urls` adds
their URLs, with the same requests, decoding (raster and SVG, ADR 0011),
SVG limits, device-resolution rendering and per-document `VectorCache`.

Chromium fetches mask images in CORS mode (a `file:` page cannot use a
`file:` mask image in Chromium; the fixture replay sends
`Access-Control-Allow-Origin: *`). swb does not check CORS for any
resource, so it loads them like backgrounds. This is a deliberate
deviation; it does not change the fixtures, whose mask images are
same-origin or `data:` URLs.

### Painting

A masked box opens a group in the display list (`PushMask { bounds,
layers }` ... `PopMask`), inside its opacity group, and is a stacking
context, as with `opacity` below 1 (§7.1). The group contains the box's
background, border, content, descendants (positioned ones included) and
outline. Like opacity stacking contexts in swb, a masked box that is not
positioned paints as a unit in the inline content phase of its stacking
context (a float in the float phase, ADR 0015), not in the z-index 0
layer (CSS 2.2 Appendix E; a simplification of `display_list.rs`). The
mask and the opacity of the root element also apply to the canvas
background, which the root's groups then paint first (as in Chromium; a
mask of the body does not mask the canvas).

The layers are computed when the group closes (`paint/src/mask.rs`):
each layer is positioned, sized, tiled and clipped like a background
layer (`background::tile`), with `mask-origin` as the positioning area
(initial: the border box, unlike backgrounds) and `mask-clip` as the
painting area. A layer whose image is not loaded, failed, is `none` or
`Failed` is transparent black (§7.1: "still counts as an image layer of
transparent black"); an `Unsupported` image is opaque in its painting
area (for alpha and for luminance), so the box shows unmasked there
instead of disappearing. Linear
gradients have no natural size and fill each tile (unlike background
gradients, which ignore size and position).

The rasterizer draws the group into a layer that covers only the visible
part of `bounds` (the content inside the union of the layer areas). When
the group ends, it renders each layer into a temporary layer of the same
size with the normal image and gradient code (so SVG masks use the
vector cache and the frame budget), takes the alpha, or the luminance
times the alpha for `mask-mode: luminance` (the `luminanceToAlpha`
coefficients on sRGB values, §7.10.1), and composites the layers from the
bottom up with `mask-composite` (the source is the layer, the destination
the layers below; the bottom layer's operator is ignored, §7.8). The
group's pixels are multiplied by the result. Repeated gradient tiles are
rendered once and drawn as an image pattern.

Chromium behaviors that were measured (Chromium 148, pixel probes of 30
small pages; the engine tests in `crates/engine/tests/masks.rs` repeat
the main ones):

- A mask image that fails to load, and `url(#missing)`, hide the box.
- Masks do not change hit testing: a masked-out or hidden box still gets
  clicks. `DisplayList::hit_test` ignores masks.
- A masked box is not a containing block for absolutely or fixed
  positioned descendants (unlike `filter` or `transform`).
- With `no-clip`, the mask painting area is the bounding box of the
  border box, the outline (for the focus ring, its area around the
  descendants) and the descendants that have their own layer in
  Chromium, with their own overflow: positioned descendants, and
  descendants with opacity or a mask together with the positioned
  descendants inside them. Each such descendant's area is clipped by its
  own overflow clip, not by those of the boxes around it. In swb: the
  ink of their display items inside the clips that those items open
  themselves (for text, the glyph origins grown by one font size to the
  right and above and a third below). In-flow descendants that overflow
  outside this box stay hidden; inside it, they are covered like the
  rest. Exception: swb paints every positioned box as a stacking
  context, so a positioned descendant inside a positioned box with
  `overflow: hidden` (and `z-index: auto`) is clipped by that box's
  clip; Chromium does not clip its area.
- The mask and the opacity of the root element apply to the canvas
  background (inside and outside the root's box).
- `none` layers count as transparent black (`url(a), none` with
  `intersect` hides the box); a single layer with `intersect` is visible.
- Luminance uses sRGB values: a mask of `rgb(128, 128, 128)` gives half
  coverage.
- Gradients respect `mask-size` and `mask-position`.

### Limits for hostile input

- At most 32 mask layers per box (`MAX_MASK_LAYERS`): the computed value
  of every mask property keeps its first 32 entries. Layer `i` uses entry
  `i` modulo the list length, so the dropped entries never matter for the
  painted layers. Without this, `* { mask: <10,000 layers> }` made every
  element compute and store 10,000 entries of nine lists (2 GB for 800
  elements in a test). Declarations with more layers are logged at debug
  level (a value with `var()` is parsed again for every element).
- The rasterizer works in strips (`rasterize_in_strips`): a window is
  one strip; a full-page screenshot is split into the fewest equal
  strips of at most the viewport's height or 16 Mpx, whichever is more
  (`MIN_STRIP_PIXELS` in the engine); equal strips can be smaller than
  that. Each strip is rasterized as if it were a viewport, in place in
  the screenshot's rows. All strips get the budgets of the common strip
  height (the last one can be lower by fewer rows than there are
  strips), so the budgets do not change at a strip boundary. The group
  layer and mask budgets below apply to each strip, and a group layer is
  at most a strip high. Transform layers (ADR 0016) count as open layers
  for the group layer budget and have a work budget per strip of their
  own. The budgets have a floor per strip, so the number of strips
  bounds their total: the largest screenshot (128 Mpx) has at most 8
  strips. The SVG rendering budget (ADR 0011) is shared by all strips;
  renderings that do not fit into the cache (more than 128 MiB of them)
  are rendered again in each strip that needs them, within that shared
  budget. Culling uses the strip grown by 1 device px on every side,
  because snapping can move a thin rectangle into the next row.
- Strips draw as one pass would: device coordinates are computed as in
  one pass and then shifted by the strip's integer row, so rounding is
  the same. They can still differ where tiny-skia clips a path to the
  strip (single anti-aliased edge pixels of curves that cross a strip
  boundary), where it filters an image or interpolates a gradient
  (positions are computed relative to the strip: up to a few levels),
  where glyph ink reaches outside the estimated text bounds that culling
  uses, and where a budget runs out.
  Measured against one pass: Wikipedia, 1280×800 at scale 2: 400 pixels
  differ by 1 level, all in images (external link icons, a thumbnail);
  700×900 at scale 2.5: 32 pixels, at most 6 levels, in images; Hacker
  News (700×900 at 2.5) and senko.net (1280×800 at 1.25): none.
- Full-page strips are larger than windows, so their mask budget per
  pixel is smaller: strips of 8 to 16 Mpx allow four to eight mask layers
  over the whole strip, a window of 1 Mpx (1280×800) about 64. A page
  whose masks render in the window can lose them in a full-page
  screenshot (for example a page-high box with more mask layers than its
  strips allow).
- All layers of open opacity and mask groups of a strip together hold at
  most 64 Mpx (256 MiB), or four times the strip's pixels if that is more
  (`MAX_GROUP_LAYER_PIXELS`), so a mask group, the temporary layer of its
  mask and two enclosing groups always fit. When a layer does not fit, an
  opacity group draws its content directly, without the opacity, and a
  mask group draws nothing. This bounds deeply nested groups (up to 256
  levels of boxes), for opacity too, which had no bound. While a mask is
  applied, its coverage buffers take 2 more bytes per pixel of the group.
  Worst case in bytes: 256 MiB of layers for strips up to 16 Mpx (all
  full-page screenshot strips of normal windows, and windows such as
  1280×800 at scale 4), plus up to 2 bytes per pixel of one group; for
  larger strips, four layers of the strip (a 1280×1600 window at scale 8:
  125 Mpx per strip, 1.95 GiB of layers). A full-page screenshot needs
  the screenshot (up to 512 MiB) and the layers of one strip; strips are
  not copied. Measured: a 1280×1600 full page at scale 8 with an opacity
  group around a mask group peaks at 1.1 GB (2.2 GB when each strip was
  copied and the last strip had other budgets).
- Mask work per strip, in pixels: at most 64 Mpx, or four times the
  strip's pixels plus two row costs per row if that is more
  (`MAX_MASK_PIXELS`; 64 Mpx take about 0.4 s at the measured 6.4 ns per
  pixel). A mask group starts only if all its work fits into the rest of
  the budget: for each mask layer, the group's layer pixels, its rows (a
  row costs as much as 16 pixels, `ROW_COST_PIXELS`: a 1 px wide tile
  takes about 82 ns per pixel), and for gradients the tiles that it draws
  or renders (planned as when they are drawn). So a strip-sized group
  with up to four mask layers (images, or gradients that are not
  repeated) renders if it is the strip's first mask group. A group that does not fit gets
  no layer, and its content is not painted (it is invisible anyway); its
  work is charged only when its layer is allocated. Nothing is charged
  after a group starts, so a strip never does more mask work than its
  budget, also with nested mask groups. A full-page screenshot of the
  largest size has at most 8 strips of at most 16 Mpx, so its mask work
  is at most 8 × max(64 Mpx, 4 × (strip pixels + 32 × strip rows)):
  about 530 Mpx (about 3.5 s) at 1280 px wide; narrower screenshots have
  more rows per pixel and a higher bound. Wikipedia uses about 0.02 Mpx.
- A gradient layer with at most 16 visible tiles draws each of them as a
  gradient (exact, also for hard stops). With more, one tile is rendered
  and repeated; the rendering has at most four times the pixels of the
  visible part of the area it covers, or the tile's own size up to 1 Mpx
  if that is more, and at most 16 Mpx; a smaller rendering is scaled up.
  So a tile that is much longer than its area in one direction, or a mask
  that is mostly outside the visible area, stays cheap.
- Mask layer geometry uses the clamped layout lengths and the background
  tiling code, which limits ratios and sizes to finite values.

The tests `hostile_masks_are_bounded` (10,000 layers, 300 nested masked
boxes, 500 overlapping viewport-sized masked boxes, scale 2; about 0.4 s
and 54 MB), `hostile_tiled_gradient_masks_are_bounded` (ten
10000×10000 px boxes with 32 tiled gradient layers, visible in 2×2 px
each; took 52 s before the visible-area bound) and
`nested_tiled_gradient_masks_are_bounded` (250 nested masked boxes with
32 gradient layers that each render a 1 Mpx tile: 0.2 s, 45.7 s when
tiles were charged after the groups started) check the limits.
`full_page_screenshots_of_page_high_groups` checks that groups as high as
a 30,000-row page (a mask with three or five layers; two opacity groups
and a mask; four and five opacity groups) render in a full-page
screenshot, the same on both sides of the strip boundary. Measured
with the release build: 200 overlapping viewport-sized boxes with 32 SVG
mask layers at 1280×1600 and scale 8 are skipped (each needs more than
the budget) in 0.9 s; 12.9 s when skipped groups still painted their
content. A full-page screenshot of three nested page-high masked boxes
with 21 layers each (1280×800, 104,000 px) takes 1.0 s (50.5 s with
strips of the viewport's height and a floor per strip).

## Consequences

- Wikipedia's mask icons render as in Chromium (menu, language, chevrons,
  ellipsis); their positions depend on layout work of other workstreams
  and on two flexbox bugs (the min-height of flex containers, and the
  baseline of an inline-flex button that starts with an empty icon).
  Update 2026-10-07: both bugs are fixed (`layout/src/flex.rs`); the menu,
  language and tools icons in Wikipedia's header and page toolbar are at
  Chromium's positions.
- Not supported: SVG `<mask>` references (they hide the box), radial and
  conic gradients as masks (the box shows unmasked), `mask-clip: text`
  (border box), `-webkit-mask-box-image` and `mask-border`, `mask-repeat:
  space | round` (painted as `repeat`, as for backgrounds), masks of
  inline boxes split over lines (each fragment is masked with its own
  positioning area; with `box-decoration-break: slice` the specification
  positions the layers in the box of all fragments together), CORS checks
  for mask images.
- A mask image that is still loading hides the box until it arrives: it
  is treated like an image that is not available (not measured in
  Chromium; screenshots and the fixtures are taken after loading).
- Masks cost one layer per masked box plus one temporary layer per mask
  layer while the group closes; for small icons this is negligible.
