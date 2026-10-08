# ADR 0023: Inline SVG

- Status: accepted
- Date: 2026-10-08

## Context

Targets 4 (Ars Technica) and 5 (BBC) draw their logos and icons as
inline `<svg>` elements in HTML: Ars has 129 (paths in groups with
`clip-path`, `fill="currentColor"` and fixed colors, `stroke-width` on
some, sized by CSS classes), BBC 103 (paths with `fill-rule="evenodd"`,
sized by `width` and `height` attributes or CSS; some paths have no
`fill` and rely on CSS or the default black). Before this change swb
treated the SVG elements as unknown inline elements: the `<svg>` had no
size and drew nothing.

ADR 0003 allows a library for SVG as an image format only (ADR 0011:
resvg and usvg for `<img>` and CSS images). Inline SVG is part of the
document: its elements take part in the cascade, inherit from HTML
ancestors, are styled by the page's style sheets and are laid out as a
CSS box. That is the browser's own job.

The specifications are SVG 2 (<https://svgwg.org/svg2-draft/>), CSS
Transforms 1, CSS Color 4 and CSS Images 3. Where they leave room,
Chromium 148 is the reference, measured black-box with `just probe`
(`tools/probes/inline-svg.json`; ADR 0021).

## Decision

swb draws inline SVG with its own code. `svgtypes` (already a pinned
dependency, ADR 0011) parses path data, transforms, `viewBox`,
`preserveAspectRatio`, lengths and point lists; tiny-skia rasterizes.
No code comes from resvg, usvg or a browser engine.

Part 1 (M4 item 5) is described first, then part 2 (M4 item 6:
`clipPath`, `defs` and `use`, the box dump of SVG descendants, hit
testing of shapes, `shape-rendering`) in its own section after "Limits".

### Where the pieces live

| Crate    | Part |
|----------|------|
| `style`  | The fill and stroke properties (`properties/svg.rs`, `values/svg.rs`); presentation attributes (`svg_attributes.rs`); `svg` is a replaced element (`element_kinds.rs`); two rules of the SVG 2 user-agent style sheet (`ua.css`). |
| `layout` | `layout/src/svg/`: the natural size of the outer `<svg>`, `viewBox` and `preserveAspectRatio` (`viewport.rs`); paths in user units, path data and the basic shapes (`path.rs`); the content of an `<svg>`, built with the box tree (`mod.rs`); its drawing commands for a content box (`draw.rs`). |
| `paint`  | `inline_svg.rs` turns the commands into display items (`FillPath`, `StrokePath`, opacity groups); `raster/path.rs` rasterizes them with tiny-skia within a work budget. |

Layout keeps the geometry, so the box dump reports the bounding boxes of
shapes from the same data (part 2).

### The outer `<svg>` element

An `svg` element in the SVG namespace is a replaced element. Layout
reaches only outer ones (the content of a replaced box gets no boxes),
so its descendants never produce boxes or text runs, and `::before` and
`::after` do not apply to SVG elements. `display: contents` computes to
`none` for it (CSS Display 3 Appendix B).

Sizing uses the CSS rules for replaced elements (`replaced.rs`) with
these inputs, measured in Chromium 148:

- The `width` and `height` attributes are presentation attributes for
  the `width` and `height` properties: CSS overrides them. Numbers are
  px, white space around the value is ignored, `calc()` works, `auto`
  sets nothing, a negative length is 0, and any other invalid value is
  `100%`.
- The natural width and height are the attributes in absolute units
  (`em` and `ex` too), also when CSS overrides the properties
  (`style="width:auto"` with `width=100` is 100 px wide). Percentages
  and invalid values give no natural dimension.
- The natural ratio is width / height of the attributes if both are
  positive, else the `viewBox` ratio. A `viewBox` with a zero or
  negative size is ignored.
- Without any of them the default object size (300×150) applies; with
  only a ratio, the box fills the available width (inline, block, flex
  items), as for SVG images.
- The user-agent rule `svg:not(:root) { overflow: hidden }` (SVG 2) makes
  the `<svg>` a scroll container for layout, so a flex item has no
  automatic minimum size (measured: a 30 px wide `<svg>` shrinks to
  26.8 px in a full row).

The content is clipped to the content box when `overflow` clips
(Chromium clips there, not at the padding box).

### Elements and properties

Drawn: `g` and `a` (groups), `path`, `rect`, `circle`, `ellipse`,
`line`, `polyline`, `polygon`. Everything else draws nothing with its
subtree: `defs`, `clipPath`, `title`, `desc`, `metadata`,
`symbol`, gradients, nested `svg`, `use`, `text`, `image`,
`foreignObject`, unknown elements. (Part 2 draws `use` and applies
`clip-path`.)

`<style>` elements in the SVG namespace do not draw, but they are
document style sheets, like HTML `<style>` (measured, probe case
`style-element`): their rules apply to the whole document, SVG and HTML
elements alike, and the order is the tree order of all style sheets.
Both kinds load only if `type` is missing, empty or `text/css` (ASCII
case-insensitive), and honor `media`.

Properties (inherited unless noted): `fill` and `stroke` (`none`, a
color, `url()` with an optional fallback; swb has no paint servers yet,
so the fallback paints, and without one the paint is `none`, as Chromium
does for a missing reference), `fill-rule`, `fill-opacity`,
`stroke-width`, `stroke-linecap`, `stroke-linejoin` (`miter`, `round`,
`bevel`), `stroke-miterlimit` (a negative or invalid value is ignored;
a value below 1 is valid and draws a bevel, as 1 does: measured, probe
case `paint-miterlimit-invalid`), `stroke-dasharray`, `stroke-dashoffset`,
`stroke-opacity`, and the existing `opacity` (not inherited), `transform`
(not inherited), `display`, `visibility` and `color`. `currentColor` in
`fill` and `stroke` stays a keyword in the computed value (CSS Color 4)
and is resolved against the `color` of the element that paints
(measured: `<svg style="color:red" fill="currentColor"><rect
style="color:blue">` is blue).

Presentation attributes (SVG 2 §6.6) for these properties and for
`overflow` are author-level declarations with specificity 0 before all
author rules, like the presentational hints of HTML: the page's style
sheets override them, and inheritance works as usual. Their values are
parsed with the CSS grammar of the property; an invalid value is
ignored. The `transform` attribute uses the SVG syntax and becomes one
`matrix()`; an invalid list is ignored as a whole (measured).

Geometry attributes (`x`, `y`, `width`, `height`, `rx`, `ry`, `cx`, `cy`,
`r`, `x1` … `y2`, `points`, `d`) are read from the attributes, not
from CSS properties. Percentages refer to the width, height or
normalized diagonal of the `viewBox`, or of the content box without one
(SVG 2 §8.9). Errors follow SVG 2: path data draws up to the first
error, a negative `width`, `height` or `r` disables the shape, `rx`
and `ry` take each other's value when one is missing and are clamped to
half the size.

`transform` on SVG content uses the view box as reference box
(`transform-box: view-box`, the initial value) and, from the user-agent
rule `svg * { transform-origin: 0 0 }` (SVG 2), the origin of the user
space.

### Painting

A shape fills, then strokes, with its transform from user units to the
content box. `fill-opacity` and `stroke-opacity` apply to each paint;
`opacity` on a group or on a shape with both paints is an opacity group
(the display list's `PushOpacity`, a layer limited to the bounds of
its items, so a small group has a small layer); on a shape with one
paint, and on a group that holds just one such shape, it multiplies the
paint's alpha (the same result without a layer). `visibility` is checked per shape, so a
visible shape in a hidden `<svg>` draws (measured). Strokes use the
transform too (a non-uniform scale gives an elliptical pen), dashes
repeat an odd list twice, and an all-zero list draws a solid stroke.
Paths are anti-aliased (`shape-rendering`: part 2).

The display list items `FillPath` and `StrokePath` carry an
`Arc<SvgPath>` (user units), a matrix to the list's coordinates, the
color and the fill rule or stroke style. The rasterizer composes the
matrix with the device scale and the translation of enclosing groups
and gives the path to tiny-skia, so transformed SVG content needs no
transform layer.

### Limits

Content from the network must not cause unbounded work or memory:

- Per box tree (document): at most 50,000 shapes and 1,000,000 path
  segments (basic shapes count theirs; an arc becomes at most four
  cubic curves, swb's own conversion from SVG 2 Appendix B, so huge
  radii cannot multiply segments). Groups nest at most 64 deep below the
  `<svg>`. Content beyond a limit is not drawn; a warning is logged
  once per layout.
- At most 256 opacity layers per document (groups, and shapes with a
  fill and a stroke, that need a layer). A layer costs memory and time
  in proportion to its area: 2,000 groups with a viewport-size layer
  took 12.5 s before the limit. Past the limit, the opacity multiplies
  the alpha of each paint inside the group, which differs from a layer
  where shapes overlap (an approximation); a warning is logged once per
  layout. HTML `opacity` has the same cost per layer and no limit yet
  (backlog).
- `stroke-dasharray` has at most 256 entries; a longer list is invalid.
- Rasterization: each path costs a fixed part, its segments, the device
  pixels of its bounds inside the visible area (the same weight for
  opaque and translucent colors: tiny-skia's anti-aliased fill takes
  2.6 ns per pixel of either, measured; the estimate for SVG images
  weighs opaque pixels at 1 unit (0.3 ns, rounded up from 0.125 ns
  measured), which holds for resvg's cached rendering, not here) and its
  dashes, with the other weights of the SVG image estimate (now in
  `paint/src/path_cost/mod.rs`, used as they are). A strip has a budget of about
  0.6 s; a path that does not fit is not drawn, and a warning is logged.
  This differs from SVG images, which are rendered once at a resolution
  that their estimate allows and then cached: inline paths are drawn
  directly for every frame, so the budget decides which paths draw, not
  the resolution. A stroke with more than 100,000 dash array entries
  (dashes and gaps, estimated from the length of its control polygon)
  is drawn solid. The edges of a
  dense path cost more than its segments and pixels (part 3).
- Non-finite coordinates end a path; matrices and bounds that are not
  finite draw nothing. tiny-skia does not draw a path whose device
  bounds exceed the 32-bit range.

The hostile-page set has a case for each limit (`inline-svg-*`).

## Part 2

All measurements are from Chromium 148, in `tools/probes/inline-svg.json`
(cases `box-*`, `clip-*`, `hit-*`, `shape-rendering`, `svg-link-*`), and
compared with swb by `just probe tools/probes/inline-svg.json
--with-swb`. The boxes of the elements also have a layout test
(`tests/layout/inline-svg-boxes.html`).

### Where the pieces live (part 2)

| Crate    | Part |
|----------|------|
| `dom`    | `ElementData::svg_href` (`href`, else `xlink:href`); `Document::element_ids` (an `id` index for many lookups). |
| `style`  | The properties `clip-path` (`none` or one `url()`; `values/svg.rs`), `clip-rule`, `shape-rendering` and the SVG values of `pointer-events`, with their presentation attributes; the instance trees of `use` elements (`cascade/use_instances.rs`, `StyleMap::use_instance`); an SVG `a` with `href` matches `:any-link`. |
| `layout` | `svg/clip.rs`: clip path references and their regions; `svg/mod.rs`: `use` as groups around a copy; `svg/draw.rs`: one pass over the nodes gives the matrices, bounding boxes, drawing commands (`SvgContent::draw`) and the box dump (`SvgContent::element_boxes`, used by `FragmentTree::element_boxes_scrolled`). |
| `paint`  | `inline_svg.rs` maps clip regions to a clip rectangle or an SVG clip group (`PushSvgClip`), and hit areas to `HitShape` items; `raster/svg_clip.rs` makes the coverage of a clip group; `hit_path.rs` tests a point against a path; `raster/path.rs` draws without anti-aliasing. |

### `clipPath`

`clip-path` is a property of all elements and a presentation attribute of
SVG elements; swb uses it on `g`, `a`, `use` and shapes. The computed value
is `none` or one `url()` (a list of two is invalid, as in Chromium; the
basic shapes of CSS Masking 1 are not supported, so such a declaration is
invalid and the one before it applies). Measured:

- The reference is looked up in the whole document, also in another
  `<svg>`; the first element with the `id` wins. A missing element, an
  element that is not a `clipPath`, a `clipPath` with `display: none` or
  inside a `display: none` subtree (no style in swb, no layout object in
  Chromium), `url(x)` without `#`, and a value without `url()` leave the
  element unclipped.
- The clip's user space is the clipped element's, with its own
  `transform`. `clipPathUnits="objectBoundingBox"` maps the unit square to
  the element's fill bounding box (a group's: the union of its children's);
  an empty box (zero width or height) clips everything. The `transform` of
  the `clipPath` applies outside the bounding box mapping (`translate(0.5
  0.5)` moves the clip by half a user unit, not by half the box).
- The clip is the union of the fill geometry of the `path`, `rect`,
  `circle`, `ellipse`, `line`, `polyline` and `polygon` children, and of
  `use` children that refer to one of these directly (the `clip-path` of
  the `use` and the `clip-path` of the shape it refers to both apply, probe
  case `r-useclip`). `g`, `text` and the
  others are ignored; children with `display: none` or a `visibility`
  other than `visible` are skipped; `fill`, `stroke` and `opacity` do not
  matter. The fill rule is `clip-rule` (inherited from the `clipPath`'s own
  ancestors, not from the clipped element); `fill-rule` does not matter. A
  clip path without shapes clips everything.
- `clip-path` on a `clipPath` and on a child of it intersect. A reference
  cycle is cut where it closes (the shapes of the clip path that closes it
  still count). The result of a cut depends on the order of the first
  references: layout builds each `clipPath` once and caches it, so a
  `clipPath` in which a cycle was cut keeps the cut version for later
  direct references. Chromium's result for such cycles is not measured; the
  order dependence is accepted, because uncached results would be built
  again at every reference. The edge between pixels is anti-aliased.

Layout resolves a reference when it builds the content (once per
`clipPath`, cached for the document) and the geometry when it makes the
commands (`ClipRegion`). A clip that is one axis-aligned rectangle, at most
scaled and translated (the usual icon: `<clipPath><path d="M0 0h40v40H0z"/>
</clipPath>`), is a clip rectangle. Paint drops a clip rectangle that
contains the content box when the `<svg>` clips its overflow (all Ars
icons) and otherwise emits a plain clip, which the rasterizer snaps to
device pixels. This deviates from Chromium, which anti-aliases the edge: a
clip rectangle with edges inside a pixel differs by up to one pixel row or
column (probe case `clip-frac`: 100 ink against 109 for a 10.4 px square).

Any other clip is an SVG clip group (`PushSvgClip`): paint draws the group
into a layer and the rasterizer multiplies it by the coverage of the shapes
(anti-aliased; the union of the shapes, each multiplied by the coverage of
its own clip, the whole multiplied by the coverage of the `clipPath`'s own
clip). The temporary coverage layers count against the layer pixel budget.
Every pass over pixels counts against the path work budget (`BLEND` per
pixel, as for a blended fill): the fills, the coverage of each clip path
and each clip rectangle, and the product with a shape's own clip. A shape
with its own `clip-path` works on the part of the layer that its bounds
cover, and adds nothing if they miss the layer. A clip that does not fit
hides its group.

### `defs` and `use`

The content of `defs` and `clipPath` is not drawn and has no box (the
builder does not enter them); other elements can refer to it. A `use`
with a local reference (`href` or `xlink:href` with `#id`) draws a copy of
the referenced element, translated by the `use`'s `x` and `y`. The copy's
rules come from the original element (selectors match it in its place);
only inheritance goes through the `use`. The style crate computes the
referenced subtree again with the style of the `use` as the parent
(`StyleMap::use_instance`, `style_in`), once for each `use` and again for a
`use` inside such a copy. The `use` elements inside a copy are collected
while the copy is styled, so only styled elements count: a `display: none`
subtree of the target costs nothing (hostile case
`inline-svg-use-hidden-subtree`). Layout builds two groups for a `use` (the
element, with its `transform`, `opacity` and `clip-path`, and the
translation) and the copy inside. Targets: `g`, `a`, the shapes and `use`;
`symbol` and nested `svg` draw nothing yet (backlog).

Limits: the instance trees of one document hold at most 20,000 elements
(`MAX_INSTANCE_ELEMENTS`; a `use` of a group of `use` elements grows
exponentially) and nest at most 16 deep (`MAX_USE_DEPTH`). A reference to
the `use` itself, to an ancestor, or to an element that an enclosing copy
already copies is a cycle and has no copy. A `use` without a copy (a bad
reference, a cycle, a limit) draws nothing; its box is at its `x`, `y`. A
`use` takes both of its groups from the budget or none.

### The box dump

Chromium reports a box for `g`, `a`, `use` and the shapes, and none for
`defs`, `clipPath`, `title`, `desc`, `style`, gradients, `mask`, `marker`,
`pattern`, unknown elements and what is below them, nor for anything with
`display: none`. The box is the **fill bounding box** under the element's
full matrix: no stroke, no clip (a clipped element has its unclipped box),
no `visibility` (hidden elements have a box); the `<svg>`'s own clip does not
limit it. Curves count by their extrema, not by their control points. A
group's box is its matrix applied to the union of its children's boxes in
the group's space (a rotated group is the bounding box of the rotated
union, not the union of the rotated boxes). Details:

- A shape that draws nothing keeps a box: a `rect` with a zero or negative
  size keeps its position and has size 0; a path without data, a `circle`
  with a zero or negative `r` and a `polyline` without points have an empty
  box at the origin (a path of one point: at that point).
- Degenerate shapes do not widen the group: a `rect`, `circle` or
  `ellipse` of size zero, a path without data, an empty group and a `use`
  without content do not. A path of one point (`M 80 80`, also from a
  `polygon`) and a `line` (also of length zero) do. In path data a move
  that nothing follows adds nothing (`M 10 10 L 50 50 M 80 80` is
  `10 10 40 40`), and two moves in a row count as the last one.
- An empty group has an empty box at the origin of its user space, with its
  `transform`; a `use` without content at its `x`, `y`.
- A `use` has the box of the copy (translated, with its `transform`). The
  elements in the copy have no box (they are not in the document) and the
  referenced element in `defs` has none.
- The box passes through the CSS transforms, scroll offsets and sticky
  offsets of the ancestors, like the box of the `<svg>`.

`SvgContent::element_boxes` computes the boxes with the same pass as the
drawing commands. Chromium also reports boxes for `text`, nested `svg`,
`symbol` (and a `use` of it), `foreignObject`, `image` and `switch`; swb
reports none (backlog).

### Hit testing

Chromium hit-tests the geometry of shapes: `elementFromPoint` returns the
shape for a point in its fill and the `<svg>` for the rest of its box. With
the default `pointer-events` (`visiblePainted`) the fill takes part if
`fill` is not `none` and the shape is visible (`fill: transparent` and
`opacity: 0` count as painted; `visibility: hidden` and `pointer-events:
none` are not hit), and the stroke if `stroke` is not `none`. swb emits a
`HitShape` display item for each shape element (not for the shapes in a
`use` copy) with the fill rule and the stroke width. `DisplayList::hit_test`
finds the topmost one that contains the point: the fill by the winding or
even-odd rule on the flattened path, the stroke within half its width of the
flattened path (round caps and joins stand in for the real ones). All values
of `pointer-events` work for shapes. `clip-path` does not limit the hit
area. A point outside the clip path of a shape does not hit it (probe case
`r-hitclip`): `DisplayList::hit_test` tests the point against the outline
of the clip shapes (`hit_path::clip_contains`: the flattened paths with
their fill rule, the shapes' own clips and the `outer` clip), not against
the anti-aliased coverage that painting uses.

An SVG `a` with `href` or `xlink:href` is a link (`:any-link`, the engine's
`link_target`): the shapes inside follow it and the cursor is the pointer.
Chromium does not colour SVG links, so the user-agent rule `svg
a:any-link` keeps the colour and the text decoration as inherited. A link
around the `<svg>` works as before.

### `shape-rendering`

Inherited. `optimizeSpeed` and `crispEdges` draw without anti-aliasing;
`auto`, `geometricPrecision` and an invalid value draw with it (measured by
ink: a circle of radius 15.3 covers 732 pixels with the first two, which is
the number of pixel centers inside it). For fills, a pixel is in if its
center is inside the outline. tiny-skia's mode without anti-aliasing cuts
curves coarsely and loses edge pixels (716 instead of 732), so swb cuts the
curves into lines of about a pixel before it fills. Strokes use tiny-skia
as it is (the probe case matches).

### Limits (part 2)

- Per document: 50,000 groups (`MAX_GROUPS`); 256 clip groups that need a
  layer (`MAX_CLIP_LAYERS`; clips that are rectangles, usually all of
  them, need none; past the limit the element is not clipped, with a
  warning); `clip-path` references 8 deep (`MAX_CLIP_DEPTH`; a cycle is
  cut); the elements of the instance trees (above).
- Per `<svg>` and drawing pass: 20,000 clip shapes (`MAX_CLIP_SHAPES`; one
  clip path of 10,000 shapes used by 10,000 elements would make 100 million
  shapes); past the limit the elements are not clipped, with a warning
  (once per `<svg>` and drawing pass).
- Rasterization: the coverage layers of clip groups count against the layer
  pixel budget, their fills and passes over pixels against the path work
  budget.

Hostile-page cases: `inline-svg-use-bomb`, `-many-uses`, `-use-cycles`,
`-use-hidden-subtree`, `-clip-chain`, `-clip-layers`, `-clip-shapes`,
`-clip-nested-shapes`.

## Part 3: the raster cost of dense paths

The review of part 2 found that the cost estimate (`SEGMENT`, 30 units, 9 ns)
is wrong for dense paths. One path of 40,000 cubic curves that each reach
across the height of a 100 x 100 px `<svg>` took 2.6 s (anti-aliased) or
1.9 s (not anti-aliased) in tiny-skia on the measuring machine (about 4 s on
the review machine), while the estimate charged 1 ms. Five fills took 22 s,
five clip references 20 s, and 40 shapes in 300 references more than 120 s.

### Measurements

tiny-skia 0.12.0 (the version swb uses), release build, `Pixmap::fill_path`
and `stroke_path` on a pixmap of the given size, best of five, in CPU time of
the thread (the machine was busy, wall time varied by a factor of two). The
measuring program included `path_cost/edges.rs` and printed the counts and the work of
the model next to the time; it is not in the repository yet (roadmap
backlog). The numbers below are the source of the model; the model does not
describe how tiny-skia works.

| Case (fill, AA; 100 px wide, edges 100 rows tall unless noted) | n edges | Time |
|---|---|---|
| Lines in a zigzag (all overlap in y, none cross) | 25,600 | 54 ms |
| Random lines (cross) | 25,600 | 1,107 ms |
| Random cubic curves | 25,600 | 2,154 ms |
| Cubics of the repro (E 61 rows) | 40,000 | 2,818 ms |
| Random lines 4 rows tall on 100 x 1,000 | 200,000 | 906 ms |
| Histogram: bars 0.5 px wide on 1,200 x 780, one path | 5,000 / 10,000 bars | 303 / 885 ms |
| Random walks of 200 segments of 3 px, 1,280 x 800, filled | 200,000 | 96 ms |

| Case (stroke, AA, 1,200 x 780) | Time |
|---|---|
| Noisy line chart, 40,000 points, 1 px | 236 ms |
| The same, 2 px / 10 px | 1,344 / 1,476 ms |
| Random walk of 200,000 segments, 1 px / 2 px | 32 / 244 ms |
| 20,000 vertical lines of 780 px, 1 px / 2 px | 816 / 504 ms |
| 20,000 horizontal lines of 1,200 px, 1 px | 428 ms |
| Random cubics, 100 px pixmap, 6,400 curves, 1 px / 2 px | 32 / 360 ms |

What the numbers show:

- A fill takes time in proportion to the *edge rows*, the sum over edges of
  the rows that they cross: 20 ns (zigzag) to 45 ns (histogram) per row of a
  straight edge, 60 to 300 ns per row of a curve, and up to 300 ns for each
  edge (random walks of short edges). Edge height hardly matters for curves that
  cross: 6,400 random cubics take 78 ms at 3 rows and 101 ms at 100 rows.
- Edges that cross cost more. A pair of random curves that cross costs
  2.6 ns, a pair that overlaps in y without crossing almost nothing: the
  zigzag of 25,600 lines has 328 million pairs that overlap in y and takes
  54 ms, the 25,600 random lines have the same number and take 20 times
  longer. Two edges can cross only if their boxes overlap in x and y, and
  that count separates the cases: 51,000 pairs of boxes for the zigzag, 217
  million for the random lines. The histogram has 163 million pairs that
  overlap in y and 77,000 pairs of boxes.
- A stroke wider than one device pixel has an outline with more edges: about
  1.6 times the cost per row of a line and 5 times the cost per row of a
  curve, and about the same cost per pair of boxes.
- A stroke of one device pixel or less (the SVG default width is 1) costs
  18 ns (horizontal lines) to 60 ns (vertical lines and curves) per pixel of
  length along the longer axis, and nothing that grows with pairs: the chart
  above takes 236 ms as a 1 px stroke and 1,344 ms as a 2 px stroke. The
  width 1.0 is in the cheap class, 1.01 is not. A hairline curve costs up to
  3 us per curve.
- Without anti-aliasing a fill costs 2 to 20 ns per row of a line, and a
  pair of random curves that cross costs as much as with anti-aliasing. A
  curve cut into lines costs 110 to 200 ns per line before any row work. A
  hairline without anti-aliasing costs up to 180 ns per pixel of a slanted
  line, 400 ns per pixel of a curve, and up to 40 us per curve.
- Small paths: a circle of 4 cubics takes 3.9 us at radius 6, 8 us at 12,
  33 us at 50 (anti-aliased).

The fits use 315 timings of 0.5 ms or more (fills and strokes of 0.25 to 10
px, with and without anti-aliasing) in 18 families: zigzags, random lines and
curves, translated and jittered S curves, curves with control points far
outside the box, short edges at many heights, bars, charts, walks, hatches
and combs.

### The model

`paint/src/path_cost/edges.rs` counts, in one sweep over the segments
(`EdgeSweep`), for edges that lie in a window of rows and columns:

- the height of the edges (a curve counts with its exact range and the sum of
  the heights of its monotone parts, which the roots of the derivative give),
  the number of edges (parts), and the length along the longer axis inside
  the window, separately for lines and curves;
- the pairs of edges that overlap in y, and the pairs whose bounding boxes
  overlap in x and y. A sweep over y with two Fenwick trees over x counts
  them in O(n log n), without listing the pairs. Bins (at most `2 * segments
  + 64` in each direction) only raise the counts: edges that share a bin
  count as overlapping.

The work, in the units of `path_cost` (0.3 ns each), is

    height * ROW + parts * PART + row_pairs * ROW_PAIR + box_pairs * BOX_PAIR

with the weights of one of six tables (`path_cost/edges.rs`): fill, stroke, hairline,
each with and without anti-aliasing. For example, a fill with anti-aliasing
has 190 per row of a line, 900 per line, 210 per row of a curve, 600 per
curve part, 1 per pair that overlaps in y and 30 (9 ns, three times the worst
measurement) per pair of boxes. A hairline has 200 per pixel of length, 300
per line and 3,500 per curve part. A fill without anti-aliasing adds
`FLAT_LINE` (500) for each line that a curve is cut into. A stroke is a
hairline if its width times the largest scale of the transform is at most one
pixel, otherwise an outline. The sweep costs up to 270 ns per segment
(`SWEEP_SEGMENT`, 1,000 units): this is charged to the budget at once, also
for a path that does not fit, so that 300 references to a path that is too
expensive cannot repeat the sweep for free (40 shapes in 300 references took
1.7 s of display list and sweeps before this charge, and more than 120 s of
fills).

The weights are rounded up so that the charged time is at least the measured
time for every case of the fits (with the spans below): `real / charged`
is at most 1.0 for fills and strokes with and without anti-aliasing, and at
most 1.02 for hairlines (20,000 exactly vertical lines); the median is 0.1 to
0.7.

The first version of the model counted the pairs that overlap in y instead
of the pairs of boxes, with a weight of 4.5 ns per pair, and treated every
stroke as an outline. It charged 5 to 30 times the real time of the
following paths, which draw in 0.06 to 0.44 s, and rejected them: a noisy
line chart of 10,000 to 40,000 points stroked 1 px wide in one path (1,200 x
780), a histogram of 5,000 bars in one path, and a random walk of 60,000
segments in one stroked path. With the model above they are charged 0.5,
0.9, 1.8, 1.0 and 0.06 billion units (budget: 2) and draw (unit tests: `realistic_dense_paths_are_drawn`). The repro pages
(five fills or clip references of 40,000 curves) are still not drawn.

### Spans narrower than a pixel

The review of the model above found a gap: an anti-aliased fill whose
edges do not merge into long spans took up to 15 times the charged time. A
comb of separate full-height teeth on 1,200 x 780 px took 5 to 6 s with
2,000 to 4,000 teeth (drawn), the filled area under a noisy chart of 40,000
points 1.6 s (CPU time; 3.3 s wall time in the review), and the model
charged 0.2 to 0.7 s. Neither the pairs of boxes nor the pairs that overlap
in y see it. `paint/src/path_cost/spans.rs` charges it.

Measured (same program and method, 230 further timings; `real` is the CPU
time of `fill_path`):

- Anti-aliased coverage has a resolution of a quarter pixel in x and in y:
  the alpha of a thin rectangle steps by 64 per quarter pixel, and a span
  whose ends round to the same quarter draws nothing.
- The extra time comes from *inside spans*: spans of a row (the runs inside
  the path under the fill rule) that lie inside one pixel and cover a part
  of it. 300 teeth 0.4 px wide, 4 px apart, 100 rows: 48 ms; the same teeth
  1 px wide at whole pixels, 2 px wide, or 0.5 px wide at random positions
  (so that some cross a pixel boundary): 1 to 6 ms. Teeth 0.25 px wide are
  slow at any position and slope. The phase matters: 0.4 px teeth at the
  same position in each pixel are slow, at random positions fast.
- A span that crosses a pixel boundary ends a *group* of inside spans. The
  time of a row grows with the inside spans of each group times the pixels
  that the group touches: 2 to 5.3 ns per pair (5.3 for isolated teeth and
  pairs of teeth in a pixel, 2 to 2.7 for dense combs where several teeth
  share a pixel). Groups apart from each other on the row add up as one:
  two runs of 600 teeth 700 px apart cost the same as one run of 1,200. Wide
  spans between the groups (1.5 px bars) do not add pixels. Teeth outside
  the pixmap cost nothing. A slanted tooth touches more pixels in a row: 300
  teeth 0.25 px wide with a slope of 4 px per row cost the same as vertical
  ones, the model counts the pixels that the slope covers.
- The time per row saturates at about 7.7 ms for 1,200 px (3,000 to 10,000
  teeth all take about 6 s for 780 rows). Stacked combs cost per row like
  one comb; the review's "combs of 40 px or less are cheap" holds only for
  teeth so thin that they round to nothing.
- Without anti-aliasing such fills are cheap (3,000 teeth: 16 ms), and so are
  strokes of the same paths (wider than a pixel: covered by the model above).
  The even-odd rule behaves like the non-zero rule for the same spans.

The model, per sample row:

- The crossings of the edges with the row, sorted by x, give the spans under
  the fill rule. A span is a *break* if it crosses a pixel boundary for every
  rounding of its ends to quarter pixels and every position of the sample in
  the row (a margin of an eighth of a pixel, plus the slope of the edge over
  an eighth of a row, plus 1/64 for float error); *neutral* if its ends
  surely round to the same quarter (empty) or to a whole pixel; else
  *inside*. Spans outside the window do not count.
- The work of a row is the sum over groups of inside spans times the pixels
  that the group touches (from each span's start minus its slope to its end
  plus its slope), times `SPAN` = 20 units (6 ns; measured up to 5.3 ns).
- Sample rows are spaced so that the crossings stay within 8 per edge plus
  4,096 (at most 4 million), at least a quarter row apart, in the middle of
  a quarter row. Each sample stands for the rows to the next. An edge that
  crosses no sample row is charged in each row that it crosses as if it
  added a span to the largest group of the samples, plus a quarter of the
  other such edges in its gap (two edges form a span).
- A bound comes first: a row with `a` edges has at most `a / 2` spans, so at
  most `(a / 2)²` work (`Edges::span_bound`, from the sweep's bins in O(n)).
  The charge is the smaller of the bound and the count. If the bound is
  below 2 million units (0.6 ms), or counting costs more than half of it or
  more than the rest of the budget, the bound is charged without counting.
- The count costs 160 units per line and per crossing of a line (measured 30
  to 45 ns), 700 per curve part and crossing (75 to 190 ns; a curve is cut at
  a sample row by 22 steps of bisection). This is charged before it runs.

Fitted on 183 timings (combs of 75 to 10,000 teeth, widths, gaps, heights,
offsets and phases; stacked combs; pairs; teeth with bars between; slanted
and curved teeth; areas under noisy charts of 2,000 to 40,000 points; the
control set below), `real / charged` with the whole model is at most 0.88;
for the slow cases it is 0.4 to 0.88 (combs 0.43, isolated thin teeth 0.85,
the area of 40,000 points 0.25). Before: up to 27.

| Case (fill, AA, 1,200 x 780 unless noted) | Real | Before: charged | After: charged |
|---|---|---|---|
| Comb, 2,000 / 3,000 / 4,000 / 6,000 teeth | 5.0 / 6.0 / 6.1 / 6.1 s | 0.19 / 0.29 / 0.39 / 0.62 s | 11.6 / 13.8 / 14.0 / 14.2 s |
| 10 stacked combs of 3,000 teeth, 78 px | 6.1 s | 0.49 s | 14 s |
| Area under a chart, 40,000 / 10,000 / 5,000 / 2,000 points | 1.6 s / 170 / 61 / 20 ms | 0.63 s / 128 / 61 / 24 ms | 6.5 s / 366 / 113 / 28 ms |
| Control: zigzag of 25,600 lines, 100 x 100 | 44 ms | 0.25 s | 0.36 s |
| Control: histogram of 5,000 bars (unit test) | 0.25 s | 0.44 s | 0.52 s |
| Control: walk fills of 60,000 / 200,000 segments | 18 / 61 ms | 26 / 104 ms | 45 / 198 ms |
| Text-like glyph stems, 30,000 spans | 2.7 ms | 10 ms | 17 ms |
| Bars 2 px wide with 0.4 px gaps, 500 | 33 ms | 45 ms | 47 ms |
| Diagonal hatching, 200 lines 0.5 px wide | 11 ms | 19 ms | 0.21 s |
| Strokes (charts, walks) | unchanged | | |

The paths of `realistic_dense_paths_are_drawn` still draw; the bars use 1.7
of 2 billion units.

Dashed strokes. Dashes are the one way for a stroke wider than a pixel to
have spans narrower than a pixel. A dashed line 780 px wide with dashes and
gaps of 0.2 px is a comb of 3,000 teeth: 6.0 s per frame (0.3: 5.1 s, 0.5:
3.1 s, 0.7: 60 ms, 1.0: 32 ms; 15 lines of 50 px: 5.8 s). `dashed_span_work`
(`raster/path.rs`) makes the outline of the stroke with tiny-skia
(`Path::dash`, then `Path::stroke` without dashes, at the scale of the
transform clamped to 1 to 16), sweeps it as a non-zero fill and charges its
spans like those of a fill. The outline costs 10 ns per segment to make
(measured: 18,000 segments 0.2 ms); it is charged 1,030 units per segment
(`SEGMENT` and `SWEEP_SEGMENT`) after it exists and before it is swept. Its
size is limited by the dash limit (`MAX_DASHES`, 100,000 dash array
entries), which is charged first. A
stroke of a hairline has no outline and is not charged. Strokes without
dashes need no such charge. Measured (tiny-skia 0.12.0, 1,200 x 780 px):
800 to 900 parallel lines of width 1.01, 1.1 and 1.3 px with gaps of 0.15 px
in one path take 40 to 48 ms; zigzags of 3,000 to 50,000 segments of width
1.2 px take 72 ms to 1.1 s (the edge model, item 7, charges those); round
dots of width 1.2 to 3 px, 150 to 700,000 of them, take 0.15 to 1.0 s (the
cost per dot is that of a small circle). A span of a stroke wider than a
pixel is wider than a pixel in x (a stroke of width `w` crossing a row has a
horizontal span of at least `w`), so it ends a group, and only caps and
joins make narrow spans. Images (`add_dash_spans` in `svg/cost.rs`) make the
same outline in the units of the canvas (at the scale of the path's
transform, 1 to 16) and count it at the 17 scales like a fill; a stroke with
more than 100,000 dash array entries (`MAX_DASHES` in `path_cost/mod.rs`, the same
count as the inline limit) is too expensive to look at and the image is
rejected; inline, such a stroke is drawn solid. Before item 8, an image
was charged for at most 1,000,000 dashes and drawn. A
dashed line 780 wide with 0.2 px dashes as an image took 6.1 s.

Remaining limits:

- Over-charge: the classification cannot see rounding and sample positions,
  so some fast fills look like slow ones. Diagonal hatching of thin filled
  lines is charged up to 30 times its time (200 lines 0.5 px wide at 45° draw,
  1,000 do not); the area under a chart of 20,000 points (0.58 s) is
  rejected; combs whose teeth round to nothing are charged as slow.
- A path whose sub-pixel structure lies only between the sample rows is seen
  through the charge for edges that cross no sample row; edges that cross a
  sample row but change their spans between samples (slanted edges) are
  estimated from the samples.
- The count of a path with many short edges takes as long as drawing it
  (200,000 segments of a random walk: about 90 ms, charged).
- Model time on paths at the document limit of 1,000,000 segments: the sweep
  takes 40 to 110 ms; the count does not run, because its work (1.5 to 3.5
  billion units) exceeds the budget. Without a budget it takes 47 ms for a
  comb of 200,000 teeth, 91 ms for a walk, 73 ms for an area and 0.9 s for
  330,000 random cubics (4 million crossings, 64 MB).

Where it applies:

- Inline fills and strokes (`raster/path.rs`, `edge_work`): the rows and
  columns are the visible area of the path's device bounds. A stroke reaches
  half its width (times the miter limit) beyond the points; a hairline does
  not.
- Clip path coverage (`raster/svg_clip.rs`): the same function with the
  layer's rows, for every shape of every clip reference.
- SVG images (`svg/cost.rs`, `add_edges`): the cost is
  `fixed + rows * sqrt(p) + linear * p + quadratic * p^2` for `p` pixels.
  The heights are in canvas units, so the rows that the edges cross at the
  size of the rendering are exactly `height * sqrt(p / canvas)`: the term
  `rows` has its own coefficient, and `max_pixels` finds the size by
  bisection on `sqrt(p)` (80 steps, no iteration limit to tune). Pairs and
  parts are in `fixed`. The pairs are counted in the units of the canvas with
  bins finer than a pixel, and boxes that overlap do so at any resolution, so
  a lower resolution does not change them. A stroke counts as an outline, not
  as a hairline: whether it is a hairline depends on the size of the
  rendering, and the outline costs more. The 40,000-curve path has `fixed`
  above the render budget, and `decode` rejects the image ("too expensive to
  render"). An image of 100 paths of 400 random cubics (inline 0.15 s) is
  charged `fixed` 0.29 billion units: it renders at 100 px in 0.25 s and, at
  1,000 px, at a lower resolution (0.49 s); the same holds for 30 x 1,000, 10
  x 2,000 and 3 x 5,000 curves, and a stroked walk of 40,000 segments draws
  at 1,000 px in full (0.18 s). The hostile case `svg-many-dense-paths` (100
  paths of 400 curves of the repro kind at 1,000 px) renders at 33,000 of
  1,000,000 pixels (120,000 before the spans).
- The spans of SVG images (`add_spans`): the scale of the rendering is not
  known when the estimate is made, so the spans are counted at 17 scales,
  from that of the largest rendering (`MAX_RENDER_PIXELS`) down by halves,
  without a pixel grid: a span is a break only if it is wider than a pixel
  plus the margins, and a group touches at most its spans' pixels and at
  most its extent at twice the scale. At a scale `s` between two of them the
  work is `s` times the rows of the canvas times the count at the lower one
  (a span only gets wider at a larger scale). The work is then not monotone
  in the size, so `max_pixels` takes the requested size and searches down
  through the pieces. Fills count as anti-aliased. The counts of one image
  take at most 1 billion units (0.3 s, once, when it is decoded); paths
  beyond are charged their bound. A nested SVG image adds its largest count
  to every scale. A comb of 3,000 teeth as an image at 1,200 x 780 (6 s
  before) renders at 48,000 pixels. All images of a document share a budget
  for counting, 2 billion units (0.6 s; `CountBudget` in `svg/cost.rs`, one
  per page in `Images`, so a new document starts with a full one): an image
  takes up to its 1 billion and gives back what it does not use. After it is
  used up, images are charged the bound without counting. 30 different
  images of a filled walk of 250,000 segments took 7.8 s of decoding (1.2 s
  without counting) and now take 2.2 s; 100 would have taken 25 s.
- Target pages: the largest path work of a strip is below 0.1 % of the budget
  (2 * 10^9) on Ars Technica and BBC. `just snapshot` before and after is
  identical.

### Hit testing

`hit_path::contains` flattens each curve into 16 lines and tests every line:
1.8 ms for a path of 40,000 curves (fill and stroke), about 1 ms for a clip
shape (fill only). `DisplayList::hit_test` evaluates the clip of every group
whose bounds contain the point, and each clip tests its shapes until one
contains the point: 40 shapes in 300 references would take about 20 s per hit
test.

Each hit test now has a work budget (`HitWork`, 30 million lines, about 40 ms).
A segment costs 1, a curve that is cut into lines 16 more. A curve whose
control points lie wholly above or below the ray (for a fill) or farther than
the pen (for a stroke) is not flattened. A test that runs out of work finds
no hit, and so do all later tests of the same call. A clip is only evaluated
inside an enclosing clip that contains the point. The worst case above now
takes 40 ms (`hit_testing_dense_clip_paths_is_bounded`).

### Layout cost of references

`SvgPath::fill_bounds` walks all segments and every element that refers to a
clip path asked for it again: 250 references to a clip path of 900,000
segments cost 0.58 s of display list, 20,000 would take 46 s. The result is
now cached in the path (`OnceLock`): 3 ms.

### Limits (part 3)

- A path whose edges cost more than the rest of the strip's budget is not
  drawn (a warning is logged once per frame). Clip coverage of such a path
  is empty, which hides the clipped group, as for other over-budget clips.
- An SVG image with a path that is too expensive at any resolution is
  rejected when it is decoded.
- A hit test has at most 30 million lines of work. There is no bound per
  frame or per second: a page that moves the pointer over a dense clip can
  spend 40 ms per event (backlog).

Hostile-page cases: `inline-svg-dense-paths` (four dense paths: fill, fill
without AA, stroke, fill and stroke; 22 s before), `inline-svg-dense-clip` (a
dense clip path, five references; 20 s before),
`inline-svg-dense-clip-references` (40 shapes in 300 references; more than
10 s before), `svg-dense-path` (an `<img>` with the 40,000-curve path),
`svg-many-dense-paths` (100 paths of 400 curves in an image), and for the
spans: `inline-svg-separate-spans` (a comb of 3,000 teeth; 6 s before),
`-separate-spans-stacked` (ten stacked combs), `-separate-spans-area` (the
area under a chart of 40,000 points), `-separate-spans-clip` (a comb as a
clip path, five references) and `svg-separate-spans` (a comb as an image);
for dashes `inline-svg-dashed-thin` (6.0 s before), `-dashed-lines` (15
lines; 5.8 s before) and `svg-dashed-thin` (an image; 6.1 s before); and
`svg-many-counted-images` (30 images of a walk of 250,000 segments; 7.8 s
before).

## Consequences

- The raster cost of a path is a model of the rows that its edges cross,
  the pairs of edges whose boxes overlap, and the length of hairlines (part
  3), and for anti-aliased fills the groups of spans narrower than a
  pixel. It is an upper bound on the families measured in part 3: fills
  (combs, stacked combs, slanted and curved teeth, areas under charts,
  walks, dots, text-like stems, hatching), the dashes of strokes more than
  a pixel wide (`real / charged` at most 0.88 for the fills); it
  over-charges some fills of thin shapes that are fast (diagonal hatching,
  up to 30 times). It does not cover: the time of the model itself (the
  sweep and the count are charged, but a page can still spend up to the
  budgets of the sweep and the count per frame and per document), strokes
  whose joins and caps make narrow spans (round dots; the edge model charges
  them like small circles), and anything but tiny-skia 0.12.0 on one
  machine. The constants are from those; a new tiny-skia needs the
  measurements again.
- Ars Technica and BBC show their logos and icons. Part 1: Ars geometry
  0.7740 → 0.7978 and pixels 0.9898 → 0.9952; BBC geometry 0.7775 → 0.8053
  and pixels 0.9942 → 0.9985; the `missing` counts grew (Ars 0 → 217, BBC
  229 → 431), because Chromium reports boxes for `g` and shapes. Part 2:
  Ars geometry 0.9963 (`missing` 0; the 4 elements left are divs) and BBC
  0.8786 (`missing` 229 again: the closed `details` of the menu, M5);
  pixels are unchanged, except for the 120 `crispEdges` icons of BBC.
- Known gaps, for the backlog: `text`, `image`, `foreignObject`, `switch`,
  nested `svg` and `symbol` (also as a `use` target), gradients and
  patterns (`url()` paints), markers, `mask` elements, the geometry
  properties in CSS (`r: 40px` overrides `r` in Chromium), `vector-effect`,
  `paint-order`, the basic shapes of `clip-path` on SVG elements (Chromium
  clips to `circle(30px at 50px 50px)`; probe case `clip-refs`) and
  `clip-path: url(#id)` on HTML elements (case `clip-other-svg`), the
  anti-aliased edge of clip rectangles, keyboard focus of SVG links.
- The rasterizer's anti-aliasing differs from Chromium's in small
  amounts (thin curved shapes: up to about 7 % of the ink in a probe).
- swb now parses SVG twice: usvg for SVG images, swb's own code for
  inline SVG. They share only `svgtypes`.
