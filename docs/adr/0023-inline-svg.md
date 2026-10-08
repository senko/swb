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

Part 1 (M4 item 5) is described here. Part 2 (M4 item 6) adds
`clipPath`, `use`, the box dump of SVG descendants and hit testing of
shapes.

### Where the pieces live

| Crate    | Part |
|----------|------|
| `style`  | The fill and stroke properties (`properties/svg.rs`, `values/svg.rs`); presentation attributes (`svg_attributes.rs`); `svg` is a replaced element (`element_kinds.rs`); two rules of the SVG 2 user-agent style sheet (`ua.css`). |
| `layout` | `layout/src/svg/`: the natural size of the outer `<svg>`, `viewBox` and `preserveAspectRatio` (`viewport.rs`); paths in user units, path data and the basic shapes (`path.rs`); the content of an `<svg>`, built with the box tree (`mod.rs`); its drawing commands for a content box (`draw.rs`). |
| `paint`  | `inline_svg.rs` turns the commands into display items (`FillPath`, `StrokePath`, opacity groups); `raster/path.rs` rasterizes them with tiny-skia within a work budget. |

Layout keeps the geometry so that part 2 can report the bounding boxes
of shapes in the box dump from the same data.

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
`foreignObject`, unknown elements. `clip-path="url(#id)"` is ignored
(the Ars icons clip to their own `viewBox`, so they draw correctly
without it).

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
Paths are anti-aliased; `shape-rendering` is not supported.

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

## Consequences

- Ars Technica and BBC show their logos and icons: Ars geometry 0.7740
  → 0.7978 and pixels 0.9898 → 0.9952; BBC geometry 0.7775 → 0.8053 and
  pixels 0.9942 → 0.9985. The `missing` counts grow (Ars 0 → 217, BBC
  229 → 431): Chromium reports boxes for `g` and shapes, swb does not
  until part 2.
- Known gaps, for part 2 and the backlog: the box dump of SVG
  descendants (Chromium reports the bounding box of shapes and groups;
  none for `defs`, `clipPath` and their content, and `title`),
  `clipPath`, `use`, nested `svg`, `symbol`, gradients and patterns
  (`url()` paints), `text`, `image`, markers, the geometry properties in
  CSS (`r: 40px` overrides `r` in Chromium), `vector-effect`,
  `shape-rendering`, `paint-order`, hit testing of shapes (the `<svg>`
  box is hit as a whole).
- The rasterizer's anti-aliasing differs from Chromium's in small
  amounts (thin curved shapes: up to about 7 % of the ink in a probe).
- swb now parses SVG twice: usvg for SVG images, swb's own code for
  inline SVG. They share only `svgtypes`.
