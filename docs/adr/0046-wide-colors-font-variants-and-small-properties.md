# ADR 0046: Wide colors, font variants, gradient tiles and small properties

- Status: accepted (2026-11-21)
- Date: 2026-11-21

## Context

M13 feature 2b (the other static CSS of videojs.org) adds what feature 2a
left: computed `oklch()`, `oklab()`, `lab()`, `lch()` and `color()` values
that keep their space (ADR 0044, section 4: color transitions must mix in
Oklab, and Tailwind 4 writes every color of the page as `oklch()`),
`font-variant-ligatures` and `font-variant-numeric`, `line-break`, the
properties without a visible effect, and the cause of the gradient over the
player that "darkens too early". Every rule below comes from measurements in
Chromium 148 (`tools/probes/css-videojs-2b.json`, 26 cases; three equal
runs; tests in `crates/automation/tests/videojs_static.rs` and
`crates/engine/tests/static_css_pixels.rs`) unless it says otherwise.

## Decisions

- **Wide colors.** The parser of `lab()`, `lch()`, `oklab()`, `oklch()` and
  `color()` returns a `Wide` color of `values/color_mix.rs` (the type that
  `color-mix()` already used) instead of an 8-bit `Rgba`. A `Wide` is a space,
  three `f32` coordinates, an alpha and a bit mask of the `none` components
  (bits 0 to 2 the coordinates, bit 3 the alpha). The process-wide table
  holds one entry per distinct color (ADR 0038 section 7b): at most 2^20
  wide colors, and, in addition, at most 2^16 mixes with `currentcolor`, so
  that a page that fills the room of the wide colors cannot invalidate the
  `color-mix()` values of later pages. When the wide colors are full, a new
  color becomes the 8-bit `Rgba` of the same color (paint is right, only the
  digits that `getComputedStyle` shows differ) and swb logs a warning once
  per document. The table is never freed: a full table takes about 200 MB
  (the entries and the map that finds them). The hostile case
  `lab-1-100-000-distinct-colors` (a script sets 1.1 million distinct
  colors) runs in about 3 s with 450 MiB. A table per document is in the
  backlog (the table is process-wide, so one page that fills it makes the
  later wide colors of the session 8-bit). The spaces are `srgb`,
  `srgb-linear`, `display-p3`, `a98-rgb`, `prophoto-rgb`, `rec2020`, `xyz`
  (`xyz-d65`), `xyz-d50`, `lab`, `lch`, `oklab`, `oklch`; `color-mix()` takes
  the new ones too. Legacy syntaxes (`rgb()`, `hsl()`, `hwb()`, hex, names)
  stay 8-bit (`rgb(none 0 0)` is `rgb(0, 0, 0)` in Chromium too).
- **Parsing rules.** The lightness of `lab()` and `lch()` is clamped to 0 to
  100, that of `oklab()` and `oklch()` to 0 to 1; a chroma is at least 0; a
  hue is normalized to 0 up to 360 (`-30` is 330, `390` is 30); `a` and `b`
  and the coordinates of `color()` are not clamped (`color(srgb 2 -1 0.5)`
  stays); alpha is clamped to 0 to 1; percentages scale as Chromium's
  (`100%` of `lab()` a is 125, of `lch()` chroma 150, of `oklab()` and
  `oklch()` 0.4, of `color()` 1); `none` is kept and written back; `calc()`
  in the number channels works; the function name is case-insensitive.
- **Serialization** (computed values, `element.style`, registered
  `<color>` custom properties): the space name and six significant digits
  (`oklch(0.123457 0.123457 123.457)`), fixed notation from 1e-6 up to
  1e6, else scientific with six digits and no zeros trimmed (`1.00000e-7`,
  `1.23457e+6`, `-1.00000e+6`; case `c-number-format`); `/ alpha` only below
  1, `/ none` for a missing alpha; `color(xyz ...)` is written `xyz-d65`.
  `light-dark()` returns its first color; `currentcolor`, `light-dark()`
  and `color-mix()` with `currentcolor` keep their earlier forms.
- **Paint** is the 8-bit sRGB color clipped to the gamut (`Wide::to_rgba`),
  as before; the pixels of all fixtures did not change (snapshot of every
  page and layout test equal before and after). Interpolation is not in 2b;
  feature 3a added it for animations (`animation/interpolate/`), not for
  gradient stops (still open at the end of M13, backlog): a gradient
  between `oklch()` colors interpolates in sRGB in swb, in Oklab in
  Chromium (CSS Color 4 section 12: the default space is
  Oklab as soon as one color is not legacy); the pixel case `color-paint`
  has no such gradient.
- **Mixing.** `color-mix()` converts a polar color to the rectangular space
  of the same family (`oklch` to `oklab`, `lch` to `lab`) and back with the
  exact formulas, not through linear light (which rounded the fifth
  digit). A missing component (`none`) of a color in the space of the mix
  takes the other color's; a missing chroma does not make the hue powerless
  (`color-mix(in oklch, oklch(0.5 none 30), oklch(0.7 0.1 50))` is
  `oklch(0.6 0.1 40)`). A missing alpha takes the other's. Departure: the
  conversions between the spaces use the matrices of CSS Color 4, and
  Chromium's results differ in the fifth digit (`color-mix(in oklch, red,
  blue)` is `oklch(0.539985 0.285449 326.643)` in swb and `oklch(0.539974
  0.285457 326.643)` in Chromium; the red channel of `oklch(0.5 0.1 30)` in
  sRGB is 0.57920 and 0.57916). The test compares numbers with a tolerance
  of 1e-4 (5e-4 of the number for large ones).
- **`font-variant-ligatures` and `font-variant-numeric`.** Computed values
  are `FontVariantLigatures` (four tri-state groups; `none` is all four
  `no-` keywords, which Chromium also writes as `none`) and
  `FontVariantNumeric` (figure, spacing, fraction, `ordinal`,
  `slashed-zero`). Each group may appear once, in any order; the computed
  text has a fixed order (ligatures: common, discretionary, historical,
  contextual; numeric: figure, spacing, fraction, ordinal, slashed-zero);
  `element.style` keeps the order as written. They are inherited. The
  OpenType features (`liga clig dlig hlig calt`; `lnum onum pnum tnum frac
  afrc ordn zero`) go before `font-feature-settings` in the feature list of
  shaping, so the settings win (measured: `font-variant-ligatures: none`
  with `font-feature-settings: "liga" 1` ligates). In the test fonts only
  DejaVu Sans has ligatures; no test font changes digit widths with `tnum`
  (case `variant-widths`: 4 fonts, 26 styles, widths equal). `font-variant`
  is the shorthand of ligatures, caps and numeric: `none` is
  `font-variant-ligatures: none`; `none` or `normal` with another keyword is
  invalid; the keywords of east asian, position and alternates are accepted
  and ignored (swb has no such longhands: `font-variant-east-asian`,
  `-position` and `-alternates` are still missing). `font` resets ligatures
  and numeric. The computed shorthand is empty where it cannot be written.
- **Properties without a visible effect**: `overflow-anchor` (also accepts
  `visible`, as Chromium), `overscroll-behavior-x`, `-y`, `-inline`, `-block`
  and the shorthand `overscroll-behavior` (one or two values; the logical
  longhands are not mapped to the physical ones), `-webkit-tap-highlight-color`
  (inherited, initial `rgba(0, 0, 0, 0.18)`), `text-size-adjust` with the alias
  `-webkit-text-size-adjust` (inherited; `none` computes to `100%`, a
  percentage may be `calc()`), `-webkit-font-smoothing` (inherited),
  `-webkit-line-clamp` (`none` or an integer from 1; the computed number
  is written as `%g`: `1e+06`, `2.14748e+09`; no layout effect yet),
  `-webkit-box-orient` (`inline-axis` and `block-axis` are the old names).
  `-webkit-background-clip` is an alias of `background-clip`, which accepts
  `text` (computed `text`; painted as `border-box` for now). Chromium does
  not enumerate `overscroll-behavior`, `-webkit-text-size-adjust` and `all`
  in `getComputedStyle`; swb does not either (shorthands and aliases).
- **`line-break`**: the five values parse and compute. Only `anywhere`
  changes line breaks of Latin text in Chromium (cases `line-break-effects`
  and `line-break-anywhere`: 5 values, 8 Latin, punctuation and Japanese
  texts at 60 px). It allows a break at every grapheme cluster boundary,
  also before punctuation and between Japanese characters, whatever
  `word-break` and `white-space: pre-wrap` say; `word-break: break-all`
  differs there (it keeps punctuation with the letter before it). The
  min-content size uses the same opportunities, as `overflow-wrap:
  anywhere` (case `line-break-min-content`: floats in a box of 0 px, 10
  classes and 3 texts). `loose`, `normal` and `strict` give the breaks of
  `auto` on those texts.
- **`all`** works in the cascade since earlier milestones; 2b adds it to
  the CSSOM table (`CSS.supports`, `element.style.all`, `cssText`) as a
  shorthand of every longhand but `direction` and `unicode-bidi`. Departure:
  `style.length` counts the longhands (Chromium's block has the one entry
  `all`), and a block with `all` and a later longhand does not write
  Chromium's expanded text.
- **`lh` and `rlh`** units: `lh` is the computed `line-height` of the
  element itself for every property except `line-height` and `font-size`,
  which use the parent's; `rlh` is the computed `line-height` of the root
  element (for the root's own properties, the initial `normal`, as for
  `rem`); a change of the root's line height restyles everything, as a
  change of its font size does. `normal` is 1.15 times the font size,
  rounded (18 px at 16 px; the style has no font metrics, as for `ex` and
  `ch`). Case `lh-units` (with `html { line-height: 1.5 }`: `1rlh` is 24
  px; own `line-height: 40px` and `width: 1lh` is 40 px; parent 30 px and
  own 50 px gives 50 px; `font-size: 2lh` under a parent of 30 px gives
  60 px).
- **Gradient layers use `background-size`, `-position` and `-repeat`.**
  This was the cause of the backlog item "the gradient over the player
  darkens too early" (it is not the color space: the gradient stops are
  black with different alphas, equal in sRGB and Oklab). The player's
  `.video-controls-backdrop` has `background-size: 100% max(50%, 240px)` at
  the bottom with no repeat; swb painted every gradient over the whole
  positioning area ("gradients are not tiled yet" in the builder), so the
  overlay covered the whole video. `background::gradient_tiling` gives the
  first tile (`auto` is the positioning area, `space` and `round` paint as
  `repeat`, as for images) and whether the painting area needs copies. The
  display list holds one `LinearGradient` item per layer with the area
  and the tile (as `Image` has `rect` and `tile`), and the rasterizer
  repeats the tile: up to 16 visible copies are drawn one by one (exact,
  also for hard stops), more are rendered once as an image and repeated
  (the code that gradient masks already used). The memory of a layer is
  constant, so no bound on tiles is needed (an earlier version made one
  item per tile and stopped at 1,024; 60,000 boxes of three small-tile
  layers took 12 GiB). A layer whose only tile is the positioning area
  paints as before, so the other pages do not change. Pixel cases
  `bg-size-max`, `gradient-tiles` (8 boxes, 9 layers) and
  `gradient-many-tiles` (tiles of 1, 2, 2.5, 3, 4 by 3, 4, 5, 6 and 7 px in
  boxes of 100 and 300 px, a stripe of 1 px by 300) equal Chromium within 1
  per channel. A rendered tile has whole device pixels and is scaled to the
  tile size, so a fractional tile with more than 16 visible copies differs
  at the tile edges: a 6.4 px tile by up to 15 per channel, a 5.5 px tile
  with a hard stop by 3 to 4 (measured in the review of the fixes). Not
  done: `space` and `round` paint as `repeat`, `background-origin:
  content-box` is `padding-box`, `background-attachment: fixed` is ignored
  (earlier gaps that the tiled path inherits). The clip of gradients by
  `border-radius` and `corner-shape` is in 2c.

## Not done in 2b, with plans

- **`background-clip: text`**: done in 2c (below).
- **`-webkit-line-clamp`** with `display: -webkit-box` and
  `-webkit-box-orient: vertical`. `-webkit-box` parses to `flex` in swb (the
  computed `display` is `flex`, Chromium's is `-webkit-box`). Plan: add
  `Display::WebkitBox` and `InlineWebkitBox` (the 21 places that name
  `Display::Flex` list them too), lay out a vertical box as a block
  container whose children stack, and give the inline layout a line limit
  of `line-clamp` lines: lines after the last one are not laid out, the box
  height is that of the last line, and the last line ends in `…` as
  `text-overflow: ellipsis` does. Measure first: the ellipsis position and
  width, the height with padding, what happens to a block child after the
  last line, `overflow` other than `hidden`, `line-clamp: 1`. The standard
  `line-clamp` was not measured; the page uses the `-webkit-` form only
  (`.line-clamp-2` in the Tailwind sheet; no element of the static page
  uses it).
- **`grid-template-rows: subgrid`** (the sponsors section: a `grid` with
  `grid-rows-subgrid` and `row-span-2`). The value is invalid in swb (the
  initial `none`), so the section's rows are its own. The first view does
  not differ at the sponsors (no box of the section is among the worst
  matches). Plan (more than 600 lines; grid track sizing changes): a
  `TrackList::Subgrid { names }` value for rows and columns, with
  `subgrid [a] [b]`, `repeat()` and the computed text measured here
  (`props-7`); in grid placement, an item that is a grid container with a
  subgrid axis takes the tracks of the parent that it spans; the track
  sizing of the parent counts the subgrid's items as its own items (their
  spans mapped to the parent's lines, the subgrid's padding, border and
  margin added to its first and last tracks); gaps of the subgrid add to
  the parent's; named lines merge. Then a second layout of the subgrid
  with the final track sizes.
- **Interpolation** of gradients with non-legacy colors in Oklab (feature
  3a): `linear-gradient(to right, oklch(0.9 0.1 90), oklch(0.5 0.2 270))` is
  `145,157,191` at the middle in Chromium and `153,150,178` in swb.

## Feature 2c: `background-clip: text` and gradients in rounded boxes

Probe file `tools/probes/css-videojs-2c.json` (8 cases; three equal
Chromium 148 runs). Measured rules are marked (M); the rest is the design.

- **The mask of `background-clip: text`** (M). The layers that say `text`
  (and the background color, when the bottom layer says it) paint only
  where the text of the box is. The mask is the union of: the glyphs of
  the text whatever the fill (`-webkit-text-fill-color: transparent` is
  the usual case; with a visible fill the text paints over its layer), the
  stroke of `-webkit-text-stroke` (the area under a stroke counts when
  the width is above 0, also with a transparent stroke color: the outer
  half of a 6 px stroke has the layer under it), and the decoration
  lines (underline, overline, line-through). `text-shadow` is not in the
  mask. The box paints inside its border box, so text outside it (an
  overflow) shows no layer; the positioning area is the one that
  `background-origin` selects (`padding-box` by default), the gradient
  moves with `background-position`, and a transform, the opacity and the
  `border-radius` of the box apply as for any background. A box without
  text paints nothing for these layers (also not its color); layers that
  say something else paint as before.
- **Which text counts** (M). The text of the box itself and of all
  descendants that paint in the box's layer: inline boxes, block children,
  floats, inline blocks, flex and grid items, table cells, `::before` and
  `::after`, `display: contents`, and boxes with `opacity: 1`. The
  `overflow` clip of a descendant (also `auto` and with `border-radius`)
  does not cut the shapes. Not in the mask: the text of positioned boxes
  (`relative`, `absolute`, `sticky`, `fixed`), of boxes with `transform`
  (also `translate(0, 0)`), `translate`, `rotate` or `scale`, `opacity`
  below 1, `filter`, `mask`, `clip-path`, `backdrop-filter`, `isolation`,
  `mix-blend-mode`, `contain: paint` or `layout`; text with `visibility:
  hidden`; the text of an `input` and of SVG `<text>`. `will-change:
  transform` alone keeps the text in the mask. swb uses the stacking
  context test of the builder (`forms_stacking_context` or a `position`
  other than `static`).
- **Paint.** A mask group (`PushMask`, ADR 0018) holds the layers that say
  `text`; its one mask layer is `MaskLayerImage::Content`, the items
  `Text`, `TextStroke` and `Rect` in white. The rasterizer draws them into
  the temporary layer of the mask like any other mask layer and multiplies
  the group by their alpha, so overlapping shapes do not add up. The
  builder collects the items in a walk of the box's descendants before it
  paints the background (`display_list/builder/text_clip.rs`): the text
  is painted later, in the foreground phase, so the mask cannot be filled
  by painting. The first plan (a new group kind) was not needed: the mask
  group already has the budgets (layer pixels, mask work: each glyph
  counts 16 pixels), the bounds and the clips. Consecutive layers with the
  same clips share their groups (`display_list/builder/layers.rs`).
- **Bounds.** A box collects at most 100,000 shapes and a display list
  1,000,000 (nested boxes each collect the text below them). A glyph counts
  1 (a stroked glyph 2), a decoration line rectangle counts 1, and the
  count is checked before each push; a walk gets what is left of the total
  as its limit, so it stops early. Beyond a limit the layers paint as
  `border-box` and swb logs a warning once per list. The decoration
  lines of all ancestors are white shapes, so a fragment gets at most one
  rectangle per line kind (the union of the ancestors' lines). Without
  this, 250 nested boxes with three lines each and 3,900 `<b>x</b>` took
  20.5 s and 31.5 GB (review of 2c); now 0.30 s and 554 MiB. Hostile
  cases: 100,001 glyphs (0.05 s, 82 MiB), 99,000 visible glyphs (0.05 s,
  82 MiB), 20,000 small boxes (0.20 s, 148 MiB), 250 nested boxes with
  5,000 glyphs below (0.15 s, 69 MiB), 250 nested boxes with decorations
  (0.30 s, 554 MiB).
- **Where it does not apply.** The canvas background (root element) and
  the backgrounds of table rows and columns paint `text` as `border-box`.
  A `<tr>`, a column or a root with `background-clip: text` is not
  measured.
- **Shimmer.** The static box of `css-animations.json` `shimmer` with
  `background-position: 200% 0` (the value at time 0): the three points of
  its pixel row equal Chromium's animated values (229,231,235; 201,201,207;
  255,255,255), and 4,300 points of `shimmer-static` equal Chromium's
  within 2 levels away from glyph edges (the edge pixels differ by up to
  21 levels; the pixel test asserts at most 24 at all of the points, and
  that a shift of 1 px breaks it).
- **Gradients, images and colors in rounded boxes.** Chromium clips each
  background layer by the rounded clip box (M): the border radii for
  `border-box` and `text`, the radii minus the border widths for
  `padding-box`, minus border and padding for `content-box`. swb painted
  gradients and images as rectangles (the corner of a `border-radius:
  40px` box at (6,6) was 17, Chromium's 255). Now the layers of a box with
  a rounded clip box paint inside a clip group of the rounded path
  (`PushSvgClip`, as the rounded `overflow` clip; `corner-shape` uses the
  path of ADR 0045), also the tiled path. A group needs a layer of the
  size of its content, and one group around the hero of videojs.org (a
  gradient of 1,180 x 570 px in a box with a radius of 24 px) took 2.5 ms
  of 29 ms of raster per frame. So the layers are split at the rows that
  hold the corners (the larger vertical radius at the top and at the
  bottom): the part between them paints without a group, the top and the
  bottom part in a group each (the items are copies with the `clip` of
  the band), and the layer of a group is as large as the corner rows. The
  cost on videojs is 0.5 to 1 ms of 29 ms of raster. The group's
  rectangle is snapped to device pixels as the fills are (at scale 2
  too); an unsnapped group blended the top and bottom row of a box at a
  fractional position (up to 64 levels, case `rounded-fractional`). The
  snap is in CSS space, before the transform (a fractional `translate`
  moves the rounded edge apart from the fill: backlog). A box whose
  corner rows cover its height has one group, and so has a box with
  `corner-shape` or a text mask. When the box has image layers, the
  background color paints in the same group as a square rectangle
  (M: a color drawn as its own anti-aliased rounded rectangle under the
  layers showed at the curve, up to 73 levels off). The color of a
  `padding-box` or `content-box` clip uses the inner radii. Not rounded:
  the color that keeps out of opaque borders (ADR 0043) and the canvas.
  `background-clip: content-box` now clips to the content box (the origin
  stays the padding box). A plain gradient whose clip is smaller than its
  positioning area (`content-box`) kept the clip as its box and squeezed
  the gradient; its box is now the positioning area. Edge pixels at the
  curve differ from Chromium by up to 40 levels (anti-aliasing), the
  inside by at most 2.
- **SVG with a viewport of size 0.** The 8 `path` boxes of the player's
  buffering spinner were the worst geometry matches of the JS-off first
  view (zero size). The icon is an `<svg viewBox="0 0 18 18">` with
  `max-width: 100%` in a box of width 0: its viewport has width 0 and
  height 18 (M). Chromium gives no `viewBox` transform to a viewport with
  a width or height of 0: the user space is the viewport's, whatever the
  `viewBox` origin and `preserveAspectRatio` (229 rows of `svg-zero-
  viewport` and `svg-css-size`, with the CSS sizes, attribute sizes,
  `auto` and `var()`). swb scaled by 0 and collapsed the paths to a
  point. Fix: `SvgContent::space`. The percentages of the content still
  refer to the `viewBox` size in such a viewport (M, `svg-percent-zero`:
  a `rect` of 50% has the box 9 x 9 in a viewBox of 18 x 18 and a
  viewport of 0 x 18, 18 x 0 or 0 x 0); without a `viewBox` they refer
  to the viewport. JS-off geometry of `videojs`: 0.9759 -> 1.0000; JS
  on: size 0.9132 -> 0.9244, relative 0.8964 -> 0.9188.
- **Departures.** A list marker is in the mask (Chromium: not;
  swb's marker is a text fragment of the item); the underline and
  line-through of 60 px text lie lower in swb than in Chromium (existing
  decoration geometry, so the mask has them there); SVG `<text>` is not
  drawn. A repeating image in a rounded box whose tile is taller than a
  corner row, with a fractional position, may differ by a pixel between the
  bands: the rasterizer snaps a single tile and not a repeated one, and a
  band can hold one tile where the middle holds several.

## Departures and open points

- `font-variant-east-asian`, `-position` and `-alternates` do not exist;
  `font-variant` ignores their keywords.
- `rgb(from ...)` and `oklch(from ...)` (relative colors) are invalid in swb,
  valid in Chromium 148. The player's stylesheet has a branch under
  `@supports (color: oklch(from red l c h))` that Chromium takes; swb takes
  the `color-mix()` branch, which gives the same alphas.
- `calc(30deg + 10deg)` in a hue is invalid (as for other angles).
- The `caret-color`, `column-rule-color`, `text-emphasis-color`,
  `accent-color`, `stop-color`, `flood-color` and `lighting-color`
  properties do not exist in swb (none is on the page).
- `background-origin: content-box` is painted as `padding-box` (older
  limit); `background-repeat: round` paints as `repeat`.
- `CSS.supports('all', 'unset !important')` is true in Chromium and false
  in swb.
