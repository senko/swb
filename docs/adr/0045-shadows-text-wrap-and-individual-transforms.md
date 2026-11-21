# ADR 0045: Shadows, text wrapping styles, individual transforms and corner shapes

- Status: accepted (2026-11-20)
- Date: 2026-11-20

## Context

The first view of videojs.org (M13 feature 2a) needs `box-shadow`,
`text-shadow`, `text-wrap: balance`, `-webkit-text-stroke`,
`translate`/`rotate`/`scale`, `isolation` and `corner-shape: squircle`, and an
inline SVG logo with `height: 100%` in a flex item. swb parsed none of them
(`CSS.supports('box-shadow', 'none')` was false). M13 feature 3 interpolates
these properties, so the computed values keep the form that the
specifications give. Every rule below comes from measurements in Chromium 148
(`tools/probes/css-videojs-2a.json`, `css-videojs-static.json`; three equal
runs; tests in `crates/automation/tests/videojs_static.rs`,
`crates/engine/tests/static_css_pixels.rs`, `crates/layout/src/inline/balance.rs`).

## Decisions

- **Computed values.** `box-shadow` and `text-shadow` are lists (`BoxShadow`,
  `TextShadow`: color, offsets, blur, spread, inset; `currentcolor` stays);
  `translate` is two length-percentages and a z length, `rotate` an axis and
  an angle in degrees, `scale` three factors; `corner-*-shape` is the
  parameter `k` of `superellipse(k)` (`squircle` 2, `bevel` 0, `scoop` -1,
  infinities for `square` and `notch`). `text-wrap-style` is a keyword. The
  serialization of `getComputedStyle` follows Chromium (`rotate: 0 0 1 90deg`
  is `90deg`, `2 0 0` is `x`, `scale: 2 2` is `2`, `thin` stroke is
  `0.333333px`, a shadow starts with the color and writes all lengths).
- **Individual transforms** apply before `transform`: the matrix is
  `translate(origin) * translate * rotate * scale * transform *
  translate(-origin)`. A value other than `none` (also an identity) makes a
  stacking context and a containing block, as `transform` does. A rotation
  around an axis other than z is projected to 2D (the upper left 2 x 2 of the
  3D matrix: `rotate: x 90deg` squashes the height to 0, as Chromium shows).
- **`text-wrap`** is the shorthand of `text-wrap-mode` and `text-wrap-style`.
- **`white-space`** is the shorthand of `white-space-collapse` (`collapse |
  preserve | preserve-breaks | break-spaces`) and `text-wrap-mode` (CSS Text
  4 §4.1). Each longhand wins in the cascade on its own and sets only its
  half of the one computed `ComputedStyle::white_space`, which has all eight
  pairs (`WhiteSpace::PreLineNowrap` and `BreakSpacesNowrap` are the two
  that have no CSS 2 keyword; their computed `white-space` is `preserve-breaks
  nowrap` and `break-spaces nowrap`, as in Chromium). `inherit` and `initial`
  of one longhand copy only its half. The grammar is Chromium 148's: a
  legacy keyword alone (`normal`, `pre`, `pre-wrap`, `pre-line`, `nowrap`,
  `break-spaces`), or `<white-space-collapse> || <text-wrap-mode>` with no
  legacy keyword inside; `preserve-spaces`, the trim keywords, `-moz-pre-wrap`
  and `-webkit-nowrap` are invalid. Measured: the cases `ws-*` of
  `tools/probes/css-videojs-2a.json` (the cascade of one element and of two
  rules, inheritance, the combinations, `element.style` and its `cssText`
  with `text-wrap`, `getComputedStyle`).
- **Balance and pretty** (`crates/layout/src/inline/balance.rs`). CSS Text 4
  leaves the algorithm open. The rules were fitted to 213 paragraph and
  width pairs of Chromium's line breaks and are not Chromium's algorithm.
  `balance`: a paragraph of 2 to 6 lines (greedy count; longer ones break as
  `auto`, as in Chromium) keeps its number of lines and breaks where the sum
  of the squares of the line widths is smallest. `pretty`: if the last greedy
  line is one word narrower than a third of the width, the last four lines
  break again with the smallest sum of the squares of the free space of the
  lines before the last, and a last line of two words or more. The planner
  works on the unbreakable groups of the line builder, per paragraph between
  forced breaks, with no floats; more than 4,096 groups in a paragraph or 256
  on a line are not planned (the work is bounded by their product). The plan
  is a list of line starts that `fill_line` obeys.
- **Shadows are display items built from existing groups** (ADR 0033,
  ADR 0023), no new rasterizer code (`crates/paint/src/box_shadow.rs`). A
  blur is a `blur()` filter group with half the blur radius as the standard
  deviation. An outer shadow is the rounded rectangle of the border box moved
  by the offsets and grown by the spread, in the blur group, inside an SVG
  clip group that excludes the border box (a path of both shapes, even-odd).
  The clip is left out if the box has an opaque background color (not with
  a `corner-shape`: the corners of the border box stay uncovered), and a
  shadow without blur that surrounds the box (offsets 0, spread not negative,
  the ring of Tailwind) is one even-odd path. An inner shadow is the even-odd
  path of a large rectangle and the "hole" (the padding box moved and
  shrunk), in a blur group, inside a clip to the padding box. The spread
  adjusts radii by the factor of CSS Backgrounds 3 §7.2. A text shadow is the
  text in the shadow color at the offset, in a blur group. All of them reuse
  the layer, filter-work and blur limits of ADR 0033: a group that does not
  fit draws without the blur. At most 64 shadows of a box or text fragment
  paint (the first ones; Chromium paints all, and a list of 10,000 shadows
  costs 10,000 layers).
- **Text stroke and fill.** `-webkit-text-stroke-width` and `-color` are
  inherited; the stroke is the stroked outline of each glyph
  (`FontContext::glyph_stroke_mask`, cached like the fill, miter joins)
  painted after the fill. `-webkit-text-fill-color` replaces `color` for the
  fill. `-webkit-text-stroke: red` has width 0 (as Chromium computes it).
  Work bound: the used width in paint is at most twice the font size (at
  least 16 px), and a width of 8,191 px or more paints no stroke. Chromium
  148 draws the stroke with a width up to 8,100 px (the ink of "Hi" at 30
  px grows by the width: 265 px wide at 240 px, a box of 2,400 px filled at
  4,000 px) and none from 8,191 px (probe rows `ink` and `pixels`,
  `text-stroke-limit`), so strokes wider than twice the font size are
  narrower in swb. Without the bound, 5,000 words with a 1,000 px stroke took
  9.4 s (a mask of (size + width)^2 pixels per glyph); now 0.1 s.
- **`corner-shape`** (`crates/paint/src/corner_shape.rs`): the outline of a
  box with superellipse corners is a polyline path (24 segments per
  corner; `|k|` above 16 is a square or a notch). Backgrounds (the color),
  uniform solid borders (the ring between two outlines) and the clip of
  `overflow` use it. Background images, gradients and non-uniform borders
  keep the rounded rectangle. The outer shadow of such a box has the
  shape of the box (the hole, and the shadow with radii grown by the
  spread). Chromium offsets the outline itself by the spread (measured:
  bevel, squircle and scoop); swb's shadow of a scoop has a slightly larger
  notch (the pixel at (12, 12) of the probe case `corner-shape-shadow`).
- **The logo.** The percentage height of a replaced child in the content
  sizes of a container (`intrinsic_height_base`) resolves against the
  container's definite content height, as Chromium does; an inline SVG
  with a viewBox and `height: 100%` gets its width from the ratio. A
  percentage `height` of the container itself resolves against the base of
  its parent (`pct-height-in-pct-height`), and a flex container's items
  get the container's definite height as the base.
- **`isolation: isolate`** makes a stacking context (`forms_stacking_context`);
  the isolated group of blended descendants already follows from ADR 0043.

## Departures and open points

- `calc()` in an angle (`rotate: calc(10deg + 5deg)`) is invalid (swb's
  `calc()` has no angles, as in `filter`).
- `element.style` serializes the specified shadow as written, not in
  Chromium's order (color first, trailing zeros dropped).
- Individual transform properties do not apply to SVG elements.
- The outer shadow of a box with an opaque background has no clip: at the
  anti-aliased corner pixel of a ring the color differs from Chromium's
  (246 against 207 of 255 at one pixel of the probe case).
- `text-shadow` does not apply to text decorations.
- The rules of balance and pretty come from paragraphs of Arial 16 px with
  words of different lengths. Measured later and equal to Chromium (cases
  `balance-*` and `pretty-inline`): inline boxes (`<b>`, padded `<i>`), an
  atomic inline, `text-align: center`, `text-indent`, `<br>`, a shrink-to-fit
  float, a flex item, and a float beside the paragraph (the plan does not
  apply with floats; the result equals Chromium's). Bidi text was not
  measured.
