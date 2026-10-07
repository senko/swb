# ADR 0011: SVG images

- Status: accepted
- Date: 2026-10-03
- Updated: 2026-10-04 (M3): see "Update (2026-10-04, masks)" at the end;
  2026-10-07: "Update (2026-10-07, video posters)". The decision did not
  change.

## Context

Target 2 (Hacker News) shows its logo (`y18.svg`, an `<img>`) and its vote
arrows (`triangle.svg`, a CSS background with `background-size: 10px`) as
SVG images. Target 3 (Wikipedia) has SVG logos and icons. ADR 0003 allows a
library for SVG as an image format; inline `<svg>` in HTML is out of scope.

An SVG image differs from a raster image:

- It can lack a natural width, height or aspect ratio, so layout and
  background sizing need the CSS default sizing rules.
- It must be rendered at the size it is drawn at, in device pixels, to be
  sharp at any scale factor.
- It is a document with references (`<image href>`, `<use>`), entities and
  filters. Untrusted input can make it load local files, or expand to huge
  trees and render times.

## Decision

### Library

`resvg` 0.48 (Apache-2.0 OR MIT): `usvg` converts the document into a
render tree, `resvg` renders it with tiny-skia. It uses tiny-skia 0.12,
the version swb uses, so there is one tiny-skia in the paint path. Features:
`default-features = false` plus `raster-images`.

- No `text`: it would add a second font stack (fontdb, and older harfrust
  and skrifa versions than swb's, see ADR 0006) with a font selection that
  differs from swb's fontconfig rules. SVG `<text>` is not drawn. None of
  the target pages' SVG images has text (logos are paths). Reconsider if a
  target needs it.
- No `system-fonts`, `memmap-fonts` (only with `text`).
- No `svgz`: Chromium decodes gzip only with `Content-Encoding`, which
  `net` handles. Raw gzip data is an error (verified: Chromium shows a
  broken image for a `file:` `.svgz`).
- `raster-images`: PNG, JPEG, GIF and WebP inside SVG images. It adds no
  crates: `gif`, `image-webp` and `zune-jpeg` are already in the tree
  through `image`.

`usvg`, `roxmltree`, `svgtypes` and `simplecss` (the versions usvg uses)
are direct dependencies: swb parses the XML before usvg, parses
attributes (the root's `width`, `height`, `viewBox`,
`preserveAspectRatio`, path data, references) and matches style sheets as
usvg does. The limits below mirror the behavior of these exact versions,
so `Cargo.toml` pins resvg, usvg, roxmltree, svgtypes and simplecss with
`=`. kurbo (arc splitting) is pinned only by `Cargo.lock`. An update of
any of them, also by `cargo update`, needs a new review of the limits.
The crates added to `Cargo.lock`: resvg, usvg, roxmltree, svgtypes,
simplecss, kurbo, euclid, polycool, float-cmp, imagesize, data-url,
pico-args, rgb (all MIT or Apache-2.0 OR MIT).

### Detection

As in Chromium, an image is SVG if and only if its response has the MIME
type `image/svg+xml`. SVG is not sniffed; other types are sniffed as raster
formats. `file:` URLs get the type from the extension (`net`). The root
element must be `svg` in the SVG namespace (Chromium shows a broken image
without `xmlns`).

### Natural size and sizing

Natural dimensions come from the root element
(<https://svgwg.org/svg2-draft/coords.html#SizingSVGInCSS>): `width` and
`height` in absolute units give natural dimensions; percentages, `auto` and
missing values give none. Negative values are 0 and huge values are clamped
to the layout length limit, as in Chromium. The aspect ratio is
`width / height` if both are positive, else the `viewBox` ratio.

`layout` has `NaturalSize` (width, height and ratio, each optional);
`ReplacedSizes` and `ImageSizes` return it. Replaced elements follow CSS
2.2 §10.3.2 and §10.6.2 with the default object size 300×150. An image
with only a ratio and automatic sizes fills the available width (inline,
block and row flex contexts) and contributes 0 to intrinsic sizes, as
Chromium does. A definite height gives any image with a ratio its width,
also as a flex item. Min and max sizes follow the table of CSS 2.2 §10.4,
with the ratio instead of `w / h`, so an image with only a ratio and no
available width still keeps its ratio. In a column flex container, the
content height of an image with a ratio (for its flex base size and
automatic minimum size) is its definite or stretched width through the
ratio, else its automatic height without its own `height`; so a definite
`height` can shrink, and an image that does not stretch takes its width
from its flexed height. Ratios are limited to [1/L, L] and sizes to L,
the largest layout length, so extreme `viewBox`es stay finite.
Backgrounds follow CSS Backgrounds 3 §3.9 with the positioning area as
the default object size (`auto`, one value, `contain`, `cover`). All
cases were measured with Chromium 148 (layout tests `svg-image-sizing`,
`svg-image-contexts`, `replaced-min-max`, `replaced-ratio-transfer` and
`flex-replaced-item`; pixel measurements for backgrounds in the unit
tests of `paint/src/background.rs`).

### Rendering

The rasterizer renders an SVG image for each tile at the tile's device
pixel size and draws it like a raster image. The concrete object size
decides the mapping (measured with Chromium): with a `viewBox`, the view
box maps into the concrete size as `preserveAspectRatio` says, and the
root's `width` and `height` do not matter; without a `viewBox`, each axis
that has a natural dimension is scaled to the concrete size, the other is
not scaled. The rendering's bounds are the root viewport, so content
outside it is clipped.

Renderings are cached in `paint::VectorCache`, keyed by image, pixel size
and concrete size. The engine keeps one cache per document (in `Images`),
so a scroll does not render again. The budget is 128 MiB of pixels and
1024 renderings; the least recently used renderings that the current frame
has not used are dropped first (if that is not enough, a new rendering is
not kept). A rendering has at most 16 Mpx and 16384 px per side; larger
tiles are rendered at a lower resolution and scaled up when drawn.

One frame (one rasterization) renders new renderings up to about 1.2 s of
estimated work, and at most 16 sizes of one image. After that, an image
is drawn from the cached rendering of it whose size is closest (scaled),
or not at all, and `rasterize` logs one warning for the frame. Nothing
requests another frame: a skipped image renders on a later repaint
(scroll, input, a load event) if that frame's budget allows. A headless
screenshot is one frame. The budget is far above what normal pages use:
the 17 SVG files of the fixtures and realistic test files (a D3 chart,
icons, Illustrator and Inkscape exports) render in the first frame at
1280×800 and scale 2, each in less than 0.5 s.

### Security and resource limits

An SVG image loads nothing. `<image>` elements may use `data:` URLs only:
raster data whose header passes the limits of raster images, or SVG data
(at most 4 levels of nesting). usvg's default resolver reads other
references from the file system; swb replaces it. SVG images have no
scripts, fonts or external stylesheets in usvg.

usvg and resvg trust their input more than a browser can: usvg limits
only `<use>` copies (1,000,000 elements) and the nesting depth (1024).
Before usvg converts a document (`paint/src/svg`):

- The source is at most 8 MiB and UTF-8 (a byte order mark is allowed;
  Chromium also accepts UTF-16 and declared encodings).
- At most 100 entity declarations, and entity references may produce at
  most 1 MiB of text. roxmltree allows DTDs (Illustrator files declare
  entities), but its own limits allow "billion laughs" and quadratic
  expansions, and it looks entities up linearly.
- roxmltree parses at most 500,000 nodes, nested at most 256 elements deep.
- Style sheets (`css.rs`): simplecss parses in time proportional to rules
  × text length (100,000 short rules take 35 s), copies a rule's
  declarations into every selector of its list, and matches descendant
  combinators by backtracking; usvg matches every rule against every
  element, `<use>` copies included. All three are bounded before parsing
  or matching. swb then matches the rules that set `url()` references
  itself, with simplecss and the same element adapter as usvg, so the
  references of each element are known.
- An upper bound of the render tree (`expansion.rs`) counts the copies
  that usvg makes: `<use>`, patterns, masks and clip paths for every
  element that refers to them (if their units, also inherited through
  `href`, make them shareable, usvg converts them once but still walks
  them per use: their elements count per use, their segments once),
  marker instances at every vertex (markers in markers multiply), arcs,
  circles and rounded corners with huge radii (kurbo splits them into up
  to millions of curves; percentages resolve against the largest
  viewport, `em` against an unknown font size counts as unbounded), and
  the size of `data:` images. References follow usvg's rules: `<use>`
  takes the first element with the id, `url()` and other `href`s the last
  SVG element, plain `href` comes before `xlink:href`, ids in `url()` are
  parsed as svgtypes does. One budget of 200,000 elements, 2,000,000 path
  segments and 16 Mpx of embedded raster images covers an image and all
  SVG images embedded in it (usvg converts an embedded image again for
  every copy).
- The effective nesting depth is at most 1024: element nesting, `<use>`
  copies, and 16 per reference to a pattern, mask, clip path, filter or
  gradient, because usvg and resvg recurse through such chains. Embedded
  SVG images get the depth that is left. Reference chains at the limit fit
  a 2 MiB stack, the default of Rust threads (tested; in a measurement
  they also fit 512 KiB).
  Reference cycles are rejected unless they consist only of `href` links
  or only of `<use>` copies: usvg removes other cycles of one or two
  references only and recurses forever on longer ones.
- The scan for recursive links in usvg (linked content × possible
  recursive links) is bounded.

Rendering (`cost.rs`): an estimate of the work per rendering (painted
area, layers, filter primitives per pixel, morphology radius squared, a
fixed cost per path and segment, dashes, embedded raster decoding; clip
paths, masks, pattern tiles and `feImage` content per use, as resvg
renders them) and of the layer memory (nested layers, up to 5 × 5
canvases each; pattern tiles) decides the resolution. Blended pixels
(translucent paint, gradients, patterns, images, layers) cost 8 times
opaque ones. The estimate counts fractions of the tree's canvas; when the
canvas covers more pixels than the drawn box (a `viewBox` with another
aspect ratio than the root's size, `slice`, an axis without natural size
that is not scaled), the pixel count is multiplied by that factor. A
rendering above about 0.6 s of estimated work or 128 MiB of layers is
made at a lower resolution; an image that exceeds them at one pixel is
not drawn, and one whose fixed cost (dashes, decoding) exceeds the work
budget is rejected. The weights were measured with resvg 0.48 on one
machine; they are estimates, not guarantees.

usvg and resvg have `unwrap`s on derived values; a panic in them is
caught (`catch_unwind`) and gives a broken or empty image.

A decode failure or a limit logs a warning and gives a broken image, like
other decode failures.

## Consequences

- Hacker News' logo and vote arrows render as in Chromium (within
  anti-aliasing differences).
- SVG `<text>` is not drawn; percentages inside an SVG image without
  `viewBox` and without absolute size resolve against 300×150, not the
  concrete size; `ex` is half an `em`; the `em` of the root `width` is
  16 px. These are rare in images.
- Pathological images draw blurred (lower resolution) or broken instead of
  hanging the engine or exhausting memory. The bounds over-count, so rare
  legitimate images can be rejected (for example cycles of one or two
  references, which usvg would cut, or `em` radii in a document that
  mentions fonts). They mirror usvg 0.48's behavior: three review rounds
  with attack probes found gaps (unbounded reference cycles and chains,
  CSS costs, per-use rendering of shared content) before they held, so an
  update of usvg or resvg needs the same review.
- Decoding and rendering run on the page thread. A worst-case image costs
  about 0.1–0.6 s to decode and up to about 0.6 s per rendering; the new
  renderings of one frame take up to about 1.2 s. On a hostile page,
  images beyond that use a rendering of another size or stay blank until
  a repaint; a screenshot can show them blank.
- If the renderings that one frame needs exceed the cache budget, the
  extra ones are drawn but not kept, and every frame renders them again
  (within the per-frame budget); whole tiles are rendered, not only their
  visible part.
- usvg and resvg are replaceable: only `paint/src/svg` uses them.

## Update (2026-10-04, masks)

Mask images (`mask-image`, ADR 0018) use the same path as background
images: detection by MIME type, the limits above, rendering at the device
pixel size of each tile, the per-document `VectorCache` and the per-frame
rendering budget. The limits did not change.

## Update (2026-10-07, video posters)

The poster of a `<video>` (ADR 0020) is an image of the element, like
the source of an `<img>`: the same request rules, detection by MIME
type, limits and rendering at the device pixel size. An SVG poster with
only an aspect ratio sizes the video as it sizes an `<img>`. The limits
did not change.

## Update (2026-10-07, responsive images)

`<img srcset>`, `sizes` and `<picture>` select the image source
(`engine/src/image_source.rs`, `css/src/sizes.rs`). The parts that affect
this ADR:

- Natural sizes are density-corrected: `ReplacedSizes::natural_size`
  divides the natural width and height of the decoded image (raster or
  SVG) by the pixel density of the chosen candidate; the ratio does not
  change. An SVG image without natural dimensions keeps none. Chromium 148
  does the same, also for SVG images and without a minimum of 1 px.
  Results are clamped to the layout length limit (density 0 gives the
  largest length, as in Chromium). `ImageSizes` gives the same corrected
  size for the image of an element, so `object-fit` and
  `object-position` draw a 2x image at half its pixel size (measured in
  Chromium 148); background and mask images are not affected.
- The `type` attribute of `<source>` accepts the MIME types that swb
  decodes (`swb_paint::is_supported_image_type`: Chromium's names for
  PNG, JPEG, GIF, WebP, BMP, ICO, and `image/svg+xml`). Chromium also
  accepts AVIF, so a page that offers AVIF first gets another source in
  swb.
- The choice among candidates follows Chromium 148 (measured): the
  first candidate, sorted by density, whose density is at least the
  device pixel ratio, else the densest. Chromium also prefers a denser
  candidate that is already in its memory cache; swb does not.
- The engine selects the sources when the subresources of a document
  start to load, and again at the next style computation after a viewport
  or scale change (on Wayland, the GUI can learn the real scale after the
  first navigation). An image keeps its current source until the newly
  selected one has loaded, so that it is not empty in between; if that
  load fails, the image keeps its source. The dimension source changes at
  once. Deviation: the specification ignores a change while a load is
  pending; swb replaces the pending selection, so that a change and its
  reversal (1x, 2x, 1x) end at the right image. Image loads that `stop`
  or a navigation cancelled are requested again at the next selection.
- `sizes="auto"` (lazy images) is not supported: it gives `100vw`, which
  Chromium also uses when it selects. Chromium then uses the laid-out
  width for the density, and its user-agent sheet gives these images
  `contain: size` with an intrinsic size of 300×150; swb has neither.
- Video posters are not affected: they have density 1 and are not
  selected again.
