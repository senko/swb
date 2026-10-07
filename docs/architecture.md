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

Design records per stage: text in [ADR 0006](adr/0006-text-stack.md),
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
| `style`      | Property definitions, value parsing, cascade, inheritance, computed values, CSS counters and list item numbers. | `css`, `dom`                  |
| `text`       | Font discovery and matching, fallback, shaping, glyph outlines and masks.      | —                             |
| `layout`     | Box tree construction, layout algorithms, fragment tree.                       | `dom`, `style`, `text`        |
| `paint`      | Display list, rasterization, image decoding (raster formats; SVG with resvg, ADR 0011). | `dom`, `layout`, `style`, `text` |
| `engine`     | Page lifecycle: loading, pipeline, input, focus, selection, form controls, hit testing, navigation, history. | all of the above |
| `automation` | Remote-control protocol: WebSocket server, methods, headless runner, Rust client. | `engine`, `net`, `dom`       |
| `swb`        | The binary: CLI, window, browser UI, clipboard, headless runner, benchmark.    | `engine`, `automation`, `net`, `paint`; `dom`, `layout`, `style`, `text` for the toolbar and debugging dumps |

Rules:

- Lower crates must not depend on higher crates. `css` and `text` do not know
  about the DOM. `css` matches selectors through a trait that `style`
  implements for DOM elements.
- Only `swb` depends on windowing and clipboard libraries.
- Only `net` does network I/O to load pages. `automation` listens on a
  local socket for the remote-control protocol.

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
  clips, opacity, mask and transform groups with their bounds) in paint
  order. A mask group carries its mask layers: images or gradients
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
  estimate of the rendering time and memory (`cost.rs`) sets the
  resolution. SVG images load only `data:` URLs. They are rendered at the
  device pixel size of each tile and cached per document (`VectorCache`
  in the engine's `Images`, 128 MiB). One frame renders new renderings up
  to a work budget; after that, images use the closest cached rendering
  or are not drawn until a later repaint.
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
  box boundaries.
- Inline layout shapes the text of an inline formatting context once
  (cached per layout pass), splits it into pieces at soft wrap
  opportunities, groups pieces into unbreakable groups, fills lines
  greedily, and then builds each line: vertical alignment, line box height,
  fragments. Font metrics and leading are rounded as Blink does, so line
  heights match Chromium. Text is shaped across inline element boundaries
  when the font does not change, as Blink does.
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
  (`layout/src/control.rs`); their sizes follow Chromium.
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
  and clips are snapped to whole pixels; an opacity or mask group draws
  into a layer that covers only its visible bounds, and a mask group's
  layer is multiplied by the mask when the group ends. The rasterizer
  works in strips: a window is one strip; a full-page screenshot is split
  into equal strips of at most the viewport height or 16 Mpx, drawn in
  place. Per strip, the layers of open groups share a memory budget
  (max(64 Mpx, 4 × strip pixels); beyond it, opacity groups draw directly
  and mask groups draw nothing), and mask groups share a work budget: a
  group starts only if all its work fits (layer pixels, a cost per row,
  gradient tiles). The SVG rendering budget is shared by all strips.
  Full-page screenshots keep the layout of the viewport and draw at
  scroll offset 0, as Chromium does. The root
  element's opacity and mask also apply to the canvas background.

## Engine modules

- `page/mod.rs`: page state, navigation, history, links, accessors.
- `page/loading.rs`: network completions, documents (HTML, text, image,
  error pages), stylesheets and images.
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
  and selects sources again after viewport or scale changes.
- `selection.rs`: text positions, the position at a point, words and
  blocks, the highlight for paint, and the selected text (`innerText`
  rules).
- `input.rs` (the input event types `Key`, `Modifiers`, `MouseButton`),
  `hit_test.rs`, `history.rs`, `resources.rs`, `boxes.rs`.

## Testing

See [testing.md](testing.md) and [ADR 0005](adr/0005-testing-strategy.md).
Python tools for fixtures and Chromium comparisons are in `tools/`.
