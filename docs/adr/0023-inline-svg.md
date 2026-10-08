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
  assumes 0.125 ns for opaque pixels, which holds for resvg's cached
  rendering, not here) and its dashes, with the other weights of the SVG
  image estimate (`paint/src/svg/cost.rs`, used as they are). A strip has a budget of about
  0.6 s; a path that does not fit is not drawn, and a warning is logged.
  This differs from SVG images, which are rendered once at a resolution
  that their estimate allows and then cached: inline paths are drawn
  directly for every frame, so the budget decides which paths draw, not
  the resolution. A stroke with more than 100,000 dashes (estimated
  from the length of its control polygon) is drawn solid.
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

## Consequences

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
