# Architecture

This document describes the current structure of swb. Update it when the
structure changes. Decisions and their reasons are in [adr/](adr/).

## Pipeline

```
URL ──► net ──► bytes ──► dom (html5ever) ──► Document
                                                │
          stylesheets (net) ──► css ──► style ◄─┘
                                          │  computed styles
                                          ▼
                              layout (uses text) ──► fragment tree
                                                        │
                                                        ▼
                                  paint ──► display list ──► pixels
```

`engine` drives this pipeline for one page. It also handles input (pointer,
keyboard, focus, text selection), scrolling, navigation and history.
`engine` has no windowing code, so it runs the same way in headless mode,
in the GUI and under the automation server.

Design records per stage: text in [ADR 0006](adr/0006-text-stack.md)
and web fonts in [ADR 0022](adr/0022-web-fonts.md),
style in [ADR 0007](adr/0007-style-system.md), interaction in
[ADR 0009](adr/0009-interaction.md), automation in
[ADR 0008](adr/0008-automation-protocol.md) and
[automation.md](automation.md).

## Crates

All crates are in `crates/`. The package name is `swb-<dir>`.

| Crate        | Responsibility                                                                 | Depends on                    |
|--------------|--------------------------------------------------------------------------------|-------------------------------|
| `net`        | Fetch resources: `http`, `https`, `file`, `data`, `about`. Cookie jar (RFC 6265bis). Replay from fixtures. | — |
| `dom`        | Arena-based DOM tree. HTML parsing (html5ever tree builder into our tree), character encoding detection, `outerHTML` serialization, the tree dump. | — |
| `css`        | CSS syntax: tokenizer, rule and declaration parser, serializer, selector parser and matcher, media queries, `@supports` conditions, the `sizes` attribute. | — |
| `style`      | Property definitions, value parsing, cascade, inheritance, computed values, CSS counters and list item numbers, `@font-face` descriptors, `font-variation-settings` and `font-feature-settings`. | `css`, `dom`                  |
| `text`       | Font discovery and matching, web font faces (`@font-face`) and their decoding (WOFF, WOFF2), fallback, shaping, glyph outlines and masks. | —                             |
| `layout`     | Box tree construction, layout algorithms, fragment tree, the geometry of inline SVG (ADR 0023). | `dom`, `style`, `text`        |
| `paint`      | Display list, rasterization, image decoding (raster formats; SVG images with resvg, ADR 0011), inline SVG drawing commands, hit testing of SVG shapes, group bounds, image reduction, and the cost model of path rasterization (`path_cost`). | `dom`, `layout`, `style`, `text` |
| `engine`     | Page lifecycle: loading, pipeline, input, focus, selection, form controls, hit testing, navigation, history. | all of the above |
| `automation` | Remote-control protocol: WebSocket server, methods, headless runner, Rust client. | `engine`, `net`, `dom`       |
| `js-text`    | JavaScript text: code units of both widths (`Str16`, `String16`), code points, UTF-8 conversion, the Unicode character classes of the lexer (generated tables). ADR 0026. | — |
| `js-regexp`  | Regular expressions. In the spike only the flag check; later the pattern parser, compiler and matcher. | `js-text` |
| `js-syntax`  | JavaScript front end: lexer (on demand, by goal symbol), name interner; later the parser, AST, early errors and scope analysis. | `js-text`, `js-regexp` |
| `js`         | JavaScript engine (a skeleton in the spike): later the compiler, interpreter, heap, built-in objects and embedding API. | `js-text`, `js-syntax`, `js-regexp` |
| `swb`        | The binary: CLI, window, browser UI, clipboard, headless runner, benchmark.    | `engine`, `automation`, `net`, `paint`; `dom`, `layout`, `style`, `text` for the toolbar and debugging dumps |

Rules:

- Lower crates must not depend on higher crates. `css` and `text` do not know
  about the DOM. `css` matches selectors through a trait that `style`
  implements for DOM elements.
- Only `swb` depends on windowing and clipboard libraries.
- The `js-*` crates and `js` depend on no other swb crate (ADR 0026); the
  bindings to the DOM come in a later crate.
- Only `net` does network I/O to load pages. `automation` listens on a
  local socket for the remote-control protocol.
- Lengths and `calc()` exist twice, on purpose: `css` (`media.rs`)
  resolves media query and `sizes` lengths when it parses them, with the
  media environment; `style` (`values/length.rs`, `parse/length.rs`)
  keeps property lengths and `calc()` expressions until computed values.
  Each has its own unit list; keep them in step.
- HTML attribute microsyntaxes (integers, non-negative integers, floats)
  are in `dom` (`microsyntax.rs`), for all crates.

## Key data structures

- **DOM** (`dom`): nodes are stored in a `Vec` and addressed by `NodeId`
  (a `u32` index). Nodes link to parent, siblings and children by `NodeId`.
  This makes the tree easy to traverse without reference counting. It also
  gives a stable handle for the automation API and, later, for JavaScript.
  The parser limits the tree depth to 512 (as Blink does), because later
  stages recurse over the tree.
- **Computed style** (`style`): one `Arc<ComputedStyle>` per element. Elements
  that share a style share the `Arc`. `StyleMap` also holds each list
  item's ordinal value; after the cascade, `counters.rs` replaces the
  counters in pseudo-element `content` with text (ADR 0007).
- **Box tree** (`layout`): built from the DOM and computed styles. It contains
  anonymous boxes (for example, anonymous block wrappers around inline
  content).
- **Fragment tree** (`layout`): the output of layout. It contains positioned
  boxes, line boxes and glyph runs in CSS pixels. It is immutable. The engine
  rebuilds it when the input changes.
- **Display list** (`paint`): a flat list of drawing commands (rectangles,
  borders, glyph runs, images, linear gradients, polylines, polygons,
  the filled and stroked paths of inline SVG and their hit areas, clips,
  opacity, mask, SVG clip path and transform groups with their bounds) in paint order. A mask group carries its mask layers: images or gradients
  positioned like background layers
  (`paint/src/mask.rs`, ADR 0018). The builder collects items in chunks
  (`paint/src/rope.rs`), so moving the items of positioned boxes copies
  each item a bounded number of times, and computes the bounds of all
  groups in one pass at the end (`paint/src/group_bounds.rs`). Form
  controls with the native look are drawn by `paint/src/control.rs`. The
  rasterizer consumes it. The rasterizer can be replaced without changes
  to layout. The list also
  contains hit regions, so hit testing finds what is painted on top.
- **Text fragments** (`layout`): a glyph run on one line, with caret
  stops: the offset in the DOM text node and the x position of each glyph
  cluster boundary. White-space processing records a source map from the
  processed text to the node's data (`layout/src/source_map.rs`).
- **Selection** (`engine`): a range between two positions in text nodes
  (node and byte offset). It refers to the DOM, so it survives a new
  layout. The tree order of nodes is computed once per document for
  comparisons.
- **Element states** (`engine` → `style`): hovered, active, focused and
  target elements. A change restyles only if a selector depends on that
  state; the layout stays if no style changed (ADR 0009).
- **Scroll offsets** (`engine`, ADR 0019): one per scroll container,
  keyed by `NodeId`, in whole CSS px, clamped after each layout. Layout
  stores each scroll container's scrollable overflow in
  `BoxFragment::scrollable_overflow` (`layout/src/scroll.rs`);
  `swb_layout::ScrollState` applies the offsets in tree walks, paint
  and hit testing. A scroll rebuilds only the display list.
- **Session history** (`engine`): one entry per committed document or
  fragment, with the scroll position to restore. A navigation is pending
  until its document arrives; starting a navigation cancels all running
  requests (`Loader::cancel_all`), and only a committed navigation adds a
  history entry. Entries record their document number, the initiator of
  the request and, for `POST` results, the body.
- **Form controls** (`engine/src/forms/`, ADR 0013): per-document `Forms`
  keyed by `NodeId` (value as `TextEdit`, checkedness, select options,
  form owner). Layout gets them through `LayoutInput::controls`; style
  gets `:checked`, `:placeholder-shown`, `:valid` and `:invalid` through
  `ElementStates::controls`. The disabled state of all elements comes
  from one tree walk (`DisabledElements`, linear time).
- **Natural sizes** (`layout`): `NaturalSize` has an optional width,
  height and aspect ratio (SVG images, videos without a loaded poster and
  audio can lack any of them). Replaced elements and backgrounds use the
  CSS default sizing rules; the default object size is 300×150. The
  natural size of an `<img>` is divided by the pixel density of its
  chosen `srcset` candidate.
- **SVG images** (`paint/src/svg/`, ADR 0011): usvg and resvg behind
  limits that are checked before conversion: source size, XML entities,
  XML nodes and depth, style sheets (`css.rs`), the size of the render
  tree with copies, reference depth and cycles (`expansion.rs`). An
  estimate of the rendering time and memory (`cost.rs`, with the cost of
  dense paths and of spans narrower than a pixel from `path_cost/`, see
  below) sets the resolution. SVG images load only
  `data:` URLs. They are rendered at the device pixel size of each tile
  and cached per document (`VectorCache` in the engine's `Images`,
  128 MiB). One frame renders new renderings up to a work budget; after
  that, images use the closest cached rendering or are not drawn until a
  later repaint. Counting the spans of a decoded SVG image draws on a
  budget per document (`CountBudget` in `Images`, 2 billion units, ADR
  0023 part 3); past it, images use the bound without counting.
- **Rounded overflow clips and image reduction** (ADR 0024): a box with
  `overflow` clipping and `border-radius` clips to the rounded padding
  edge. The display list emits it as an SVG clip group (`PushSvgClip` with
  a rounded-rectangle `ClipPath`, `rect_fallback` set), which the
  rasterizer draws on a layer and multiplies by the coverage
  (`paint/src/raster/svg_clip.rs`). The layer and the coverage count
  against the layer budget and the path work budget. When they do not fit,
  the group draws its content clipped to the rectangle (square corners)
  instead of hiding it. A raster image drawn at a scale below 0.5 is drawn
  from an averaged mip level (`paint/src/reduce.rs`): 2x2 averaging per
  axis until the scale is at least 0.5, then bilinear. The levels are kept
  with the decoded image (at most its own size) and made lazily. One frame
  averages at most 64 Mpx of source pixels; past that, images use the
  nearest level that exists, and a later repaint continues.
- **Inline SVG** (ADR 0023): an `svg` element in HTML is a replaced box
  (`style::is_replaced_element`); its natural size comes from its
  `width`, `height` and `viewBox` attributes, which are also
  presentation attributes for `width` and `height`. Style computes the
  fill and stroke properties for all SVG elements and maps their
  presentation attributes as author-level hints
  (`style/src/svg_attributes.rs`). When the box tree is built,
  `layout/src/svg/` walks the descendants once into an `SvgContent`
  (groups and shapes with their computed styles; `path`, `polyline` and
  `polygon` data parsed into user units with `svgtypes`; `use` as groups
  around a copy, whose styles inherit from the `use` element:
  `style/src/cascade/use_instances.rs`; `clip-path` references to
  `clipPath` elements, `layout/src/svg/clip.rs`). The descendants get no
  boxes of their own: `FragmentTree::element_boxes_scrolled` adds the
  bounding boxes of `g`, shapes and `use` that `SvgContent::element_boxes`
  computes. `BoxContent::Svg` carries the content to paint, which asks it
  for its drawing commands for the content box size (percentages and the
  `viewBox` transform depend on it: clip regions, `FillPath` and
  `StrokePath` items with a matrix from user units and the hit areas of
  shapes, `paint/src/inline_svg.rs`). A clip path that is not a clip
  rectangle becomes a `PushSvgClip` group, which the rasterizer multiplies
  by the coverage of the clip shapes (`paint/src/raster/svg_clip.rs`).
  `DisplayList::hit_test` finds shapes by their geometry
  (`paint/src/hit_path.rs`, within a work budget per hit test), so an SVG
  `a` works as a link. The rasterizer draws paths with tiny-skia within a
  work budget per strip (`paint/src/raster/path.rs`; the cost of a path
  includes the rows that its edges cross and the pairs of edges whose
  boxes overlap, `paint/src/path_cost/edges.rs`, and for anti-aliased
  fills the spans narrower than a pixel, `paint/src/path_cost/spans.rs`,
  ADR 0023 part 3; the weights are in `paint/src/path_cost/mod.rs` and
  the estimate for SVG images uses the same model). Limits per
  document: 50,000 shapes and 50,000 groups, 1,000,000 path segments,
  groups 64 deep, 256 opacity layers and 256 clip layers, 20,000
  elements in the copies of `use` elements.
- **Web fonts** (ADR 0022): the stylist keeps the `@font-face` rules
  (`swb_style::FontFace`) with their `@media` chain. The engine gives the
  applicable faces to the `FontContext` (`set_web_fonts`), which owns the
  face set: families, composite fonts by `unicode-range`, the load state
  of each face and the requests that layout makes
  (`text/src/web.rs`). After layout the engine takes the requests and
  loads the sources (`engine/src/page/fonts.rs`; each URL once,
  `engine/src/web_fonts.rs`); `text/src/decode.rs` decodes WOFF and
  WOFF2 (`wuff`) within size limits. A font that arrives invalidates the
  layout. A font is a face with synthesis flags and variation axis
  values (`FontId`); caches key on it. The axis values are the ones that
  matching sets, then the `@font-face` descriptor, then
  `font-variation-settings` (the text crate interns the list). A
  `local()` source asks the font source for a face by full name or
  PostScript name and loads at once. `size-adjust` and the metric
  overrides are properties of the loaded face: the text crate scales the
  size for shaping, metrics and glyph masks, so layout is unchanged.
  `font-feature-settings` go to the shaper (`ShapeOptions::features`).
- **Cookie jar** (`net`): one `CookieJar` per `NetworkFetcher` (so one per
  swb process), inside the HTTP client. It adds `Cookie` to every HTTP
  request and stores `Set-Cookie` on every redirect hop. Cookies are
  grouped by registrable domain (Public Suffix List, `net/src/site.rs`).
  Each `Request` carries its `initiator` (the origin of the document that
  started it); with `Destination::Document` (a top-level navigation) this
  gives the `SameSite` context. In memory only; fixture replay has no
  cookies (ADR 0012).

## Units

Layout uses `f32` CSS pixels. Paint multiplies by the device pixel ratio
(HiDPI scale factor) only at rasterization.

## Threads

- The engine runs on one thread.
- `net` runs fetches on a pool of worker threads. Results return to the engine
  through a channel. The engine processes them when it is polled. The
  workers share the cookie jar (a mutex).
- The GUI shell runs the window event loop on the main thread and calls the
  engine. Network completions wake the event loop.
- The automation server accepts connections on its own thread and runs one
  thread per connection. Each request goes to the page thread through a
  channel and wakes it (the GUI through its event loop proxy, the headless
  runner through its wake-up channel); the response comes back through a
  reply channel. `page.waitForLoad` stays pending on the page thread until
  the page is loaded or its timeout expires.

## Layout details

- The box tree (`layout/src/box_tree.rs`) is rebuilt for every layout.
  Inline content is a flat list of items (text, inline box start/end,
  atomic inlines, line breaks); white space is collapsed when the inline
  formatting context is finished, because spaces collapse across inline
  box boundaries. Generated text (`content`, list markers) is limited to
  1 MiB per box tree (`MAX_GENERATED_TEXT`, ADR 0007).
- `details` (HTML §15.5.5): style computation gives every `details` a
  `::details-content` pseudo-element style (`PseudoKind::DetailsContent`)
  and, without a `summary` child, a default summary style
  (`DetailsSummary`: a list item with the text "Details"). Box
  construction puts all children except the first `summary` into the
  `::details-content` box. While `details` is closed, its style has
  `contents_hidden` (Chromium's `content-visibility: hidden` on that box):
  the box is a block formatting context and the containing block of
  absolutely positioned boxes, has no height, intrinsic size or baseline,
  and does not add to scrollable overflow. After layout, `hide_contents`
  moves its children to `BoxFragment::hidden`: paint, hit testing, the
  selection and scrolling see only `children`, while the box dump
  (`FragmentTree::element_boxes`) also walks `hidden`. The engine toggles
  the `open` attribute on a click on the summary or Enter/Space on it
  (`page/forms.rs`), restyles, and skips elements in closed contents in
  the focus order (`Document::in_closed_details_content`).
- Flex layout (`flex.rs`) follows CSS Flexbox 1 §9: lines, flexible
  lengths, cross sizes, `align-content`, first baselines and automatic
  minimum sizes. Item layouts are cached in `LayoutContext::layouts`.
  `align.rs` resolves `auto` self-alignment and the edge an item aligns
  to, for flex, grid and positioned layout.
- `replaced.rs` sizes replaced elements (CSS 2.2 §10.3.2, §10.6.2, with
  the min/max rules of §10.4); `intrinsic.rs` computes min-content and
  max-content widths for shrink-to-fit boxes, flex base sizes, grid
  tracks and tables. Margin collapsing (CSS 2.2 §8.3.1) uses the
  `CollapsedMargin` type in `collapsed_margin.rs` (derived from Servo,
  MPL-2.0, ADR 0021).
- `aspect.rs`: the `aspect-ratio` of blocks, flex containers and grid
  containers (CSS Sizing 4 §5; replaced elements stay in `replaced.rs`).
  An `auto` axis takes its size from the other axis through the ratio,
  which applies to the content box or, with `box-sizing: border-box`, to
  the border box. Min and max sizes transfer with the table of CSS 2.2
  §10.4. A box whose height came from the ratio grows to its content
  unless it is a scroll container or has an explicit `min-height`; the
  ratio-derived height is definite for percentage heights of children.
  Block, shrink-to-fit, flex, grid and absolutely positioned layout each
  call into it where they resolve a width or a height; `intrinsic.rs` uses
  the width from a definite height as the content size.
- Inline layout shapes the text of an inline formatting context once
  (cached per layout pass), splits it into pieces at soft wrap
  opportunities, groups pieces into unbreakable groups, fills lines
  greedily, and then builds each line: vertical alignment, line box height,
  fragments. Font metrics and leading are rounded as Blink does, so line
  heights match Chromium. Text is shaped across inline element
  boundaries when the font does not change; shaping breaks at box edges
  as measured in Chromium. Break opportunities come from
  `text/src/linebreak.rs` (UAX #14 with tailorings measured in Chromium,
  tables generated from the measurements) and from
  `layout/src/inline/breaks.rs` at element boundaries (`white-space` at
  box ends, isolating boxes). A group that does not fit on a line by
  itself is cut at grapheme boundaries when `overflow-wrap` or
  `word-break` allow it (`inline/overflow.rs`). Text item widths are
  rounded up to 1/64 px; small capitals are synthesized in
  `inline/caps.rs`.
- Depth limits: the HTML parser nests at most 512 elements deep (deeper
  elements are appended to the ancestor at the limit, as in Blink); box
  construction nests at most 256 boxes deep (deeper content keeps its
  text but not its boxes). Together they bound the recursion of all later
  stages.
- Lengths are clamped to ±33,554,431 px (the range of Blink's
  `LayoutUnit`), so hostile values cannot produce infinite geometry.
- Flex item, grid item and table cell layouts are cached per layout pass
  by box and constraints (`LayoutContext::layouts`), so nested flex
  containers, grids and tables do not cost exponential time. Fragment
  children are shared (`Arc`), so a cached subtree is stored once and
  copying a fragment is cheap.
- Grid containers (`layout/src/grid/`, ADR 0017): the explicit grid of
  each axis as segments of repeated track lists (`template.rs`),
  placement and auto-placement with per-row interval occupancy
  (`placement.rs`), tracks grouped into ranges and sets as in Chromium
  (`tracks.rs`), the track sizing algorithm on sets (`sizing.rs`), and
  the grid sizing algorithm, item contributions, alignment and baselines
  (`mod.rs`). Placements and intrinsic widths are cached per layout pass
  (`LayoutContext::grids`). Lines are clamped to ±10,000.
- The overflow of the root (or of the body, if the root's is `visible`)
  applies to the viewport (CSS Overflow 3 §3.3); the source element then
  has a used overflow of `visible`.
- List markers wait for the first line box of the list item, also inside
  nested blocks and independent formatting contexts.
- Tables (`layout/src/table/`, ADR 0010) follow Chromium's LayoutNG
  algorithm: box construction with the anonymous table fixup
  (`build.rs`), the grid (`grid.rs`), column constraints and the grid's
  intrinsic widths (`columns.rs`), width distribution (`distribute.rs`),
  row heights (`rows.rs`, `cells.rs`), placement and fragments
  (`layout.rs`, `fragments.rs`), collapsing borders (`collapsed.rs`).
  The `<table>` box contains its captions; row groups, rows, cells,
  columns and column groups have fragments. `BoxContent` tells paint how
  to paint table parts: the table's background and border fill the grid;
  cells paint the backgrounds of their columns, row groups and rows; the
  table paints collapsed borders after its cells.
- `text-align: -webkit-left/center/right` aligns block-level children
  without `auto` margins (`<center>`).
- Quirks mode: the line height quirks and the table cell quirks are in
  layout (`LayoutContext::quirks`, `line_height_quirks`).
- Media elements (`layout/src/media.rs`, ADR 0020): `<video>` and
  `<audio>` are replaced boxes; layout chooses the visible parts of
  their controls (Chromium's size thresholds) and shapes the time text;
  `BoxContent::Media` carries them to paint (`paint/src/media.rs`).
  Posters load like `<img>` sources; media resources are never fetched.
- Form controls are atomic boxes with generated content
  (`layout/src/control.rs`); their sizes follow Chromium. A `<button>`
  with `display: flex` or `display: grid` lays out its DOM content as an
  ordinary flex or grid container (`grid::layout_contents`); its flow
  content is centered vertically.
- `<br>` elements and inline boxes around block-level children get boxes
  for the box dump; the latter are `BoxContent::GeometryOnly`.
- Absolutely positioned boxes (`layout/src/positioned.rs`, ADR 0016)
  get a placeholder fragment at their static position (block, inline,
  flex and grid layout). After layout, one walk lays each box out in its
  containing block (the nearest positioned or transformed ancestor; a
  positioned inline box from its first to its last fragment) and
  replaces the placeholder, so fragments stay relative to their parents
  and in tree order. The walk then recomputes the scrollable overflow of
  scroll containers that contain placed boxes. Sticky offsets and
  transforms do not change layout: paint applies them (`Ancestry`,
  `GroupTransform`), and `FragmentTree::element_boxes_scrolled` gives
  the painted boxes.
- Floats (`layout/src/floats.rs`, ADR 0015): each block formatting
  context has an exclusion space; `LayoutContext` keeps a stack of BFCs.
  Block layout knows the BFC position of its boxes; boxes whose top
  margin can still collapse wait in a chain of frames, and the floats in
  them wait for its position. Lines and BFC roots take layout
  opportunities produced one at a time; all float work of a layout pass
  shares a work budget.
- Paint order: each stacking context (root; positioned boxes with an
  integer z-index; fixed, sticky and transformed boxes; opacity < 1;
  masked boxes) paints its normal-flow content in three phases
  (backgrounds and borders of in-flow blocks, floats, inline content;
  CSS 2.2 Appendix E), then its positioned and transformed descendants
  in z-index order and, for equal values, tree order (a pre-order index
  of the fragment tree; negative z-index before the content). Floats,
  atomic inline-level boxes and flex and grid items paint as a unit.
  Relative and absolute boxes with `z-index: auto` paint in that order
  but form no stacking context. A
  positioned box repeats the clips between its stacking context and
  itself that belong to its containing block chain; a box fixed to the
  viewport replaces all clips (`PushViewportClip`) except `clip`
  rectangles.
- Transform groups (`PushTransform`/`PopTransform`) wrap a box's opacity
  and mask groups. Fixed and sticky boxes are translation groups that
  the rasterizer and hit testing resolve at the viewport scroll offset,
  so a viewport scroll does not rebuild the display list. Translation
  groups move their items exactly; other transforms draw into a bounded
  layer (anti-aliased edges, per-strip budgets, at most 8 deep).
- The content of scroll containers is translated by their offsets in
  the display list; `paint/src/scroll_indicator.rs` draws the GUI's
  overlay scroll indicators (not in headless screenshots).
- The display list contains the selection highlight (a rectangle behind
  the selected glyphs, then the glyphs again in the selection color) and
  outlines; `outline-style: auto` is Chromium's two-ring focus ring.
- Rasterization works in device pixels. Rectangles without rounded corners
  and clips are snapped to whole pixels, and a single image is drawn into
  its pixel-snapped rectangle (tiled images are not snapped); an opacity or mask group draws
  into a layer that covers only its visible bounds, and a mask group's
  layer is multiplied by the mask when the group ends. The rasterizer
  works in strips: a window is one strip; a full-page screenshot is split
  into equal strips of at most the viewport height or 16 Mpx, drawn in
  place. Per strip, the layers of open groups share a memory budget
  (max(64 Mpx, 4 × strip pixels); beyond it, opacity groups draw directly
  and mask groups draw nothing), and mask groups share a work budget: a
  group starts only if all its work fits (layer pixels, a cost per row,
  gradient tiles). SVG paths share a work budget per strip; a path
  that does not fit is not drawn. The SVG rendering budget is shared by
  all strips.
  Full-page screenshots keep the layout of the viewport and draw at
  scroll offset 0, as Chromium does. The root
  element's opacity and mask also apply to the canvas background.

## Engine modules

- `page/mod.rs`: page state, navigation, history, links, accessors.
- `page/loading.rs`: network completions, documents (HTML, text, image,
  error pages), stylesheets and images.
- `page/fonts.rs`, `web_fonts.rs`: web fonts: the face set of the
  document, loading the faces that layout requested (`url()` through the
  loader, `local()` from the installed fonts), font files by URL.
- `page/pipeline.rs`: style, layout, display list and raster with
  per-stage timings; restyles after state changes; viewport and
  screenshots.
- `page/scroll.rs`: scrolling of the viewport and of scroll containers
  (wheel, keys, scroll into view), scrolling to a fragment, `:target`.
- `scrollers.rs`: scroll offsets per element, scroll ranges, scroll
  chains, alignment for scroll into view.
- `page/input.rs`: mouse and key events, element states, cursor, focus
  navigation, selection by mouse and keyboard.
- `page/forms.rs`: editing, activation and submission of form controls;
  `forms/` (state, view for layout, select, value sanitization, the
  entry list and submission, encodings); `edit.rs` (`TextEdit`, the
  editing model, also used by the GUI address bar).
- `focus.rs`: focusable elements and the sequential focus order.
- `image_source.rs`: image source selection for `<img>` (`srcset`,
  `sizes`, `<picture>` and `<source>`, the source of the dimension
  attributes); `resources.rs` keeps each image's URL and pixel density
  and selects sources again after viewport or scale changes;
  `page/auto_sizes.rs` selects the sources of lazy images with
  `sizes="auto"` after each layout, by the width of their boxes.
- `selection.rs`: text positions, the position at a point, words and
  blocks, the highlight for paint, and the selected text (`innerText`
  rules).
- `input.rs` (the input event types `Key`, `Modifiers`, `MouseButton`),
  `hit_test.rs`, `history.rs`, `resources.rs`, `boxes.rs`.

## Testing

See [testing.md](testing.md) and [ADR 0005](adr/0005-testing-strategy.md).
Python tools for fixtures and Chromium comparisons are in `tools/`.
