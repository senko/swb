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

`engine` drives this pipeline for one page. It also handles input,
scrolling, navigation and history. `engine` has no windowing code, so it runs
the same way in headless mode and in the GUI.

Design records per stage: text in [ADR 0006](adr/0006-text-stack.md),
style in [ADR 0007](adr/0007-style-system.md).

## Crates

All crates are in `crates/`. The package name is `swb-<dir>`.

| Crate        | Responsibility                                                                 | Depends on                    |
|--------------|--------------------------------------------------------------------------------|-------------------------------|
| `net`        | Fetch resources: `http`, `https`, `file`, `data`, `about`. Replay from fixtures. | —                             |
| `dom`        | Arena-based DOM tree. HTML parsing (html5ever tree builder into our tree).      | —                             |
| `css`        | CSS syntax: tokenizer, rule and declaration parser, selector parser and matcher. | —                             |
| `style`      | Property definitions, value parsing, cascade, inheritance, computed values.    | `css`, `dom`                  |
| `text`       | Font discovery and matching, fallback, shaping, glyph outlines and masks.      | —                             |
| `layout`     | Box tree construction, layout algorithms, fragment tree.                       | `dom`, `style`, `text`        |
| `paint`      | Display list, rasterization, image decoding.                                   | `layout`, `text`, `style`     |
| `engine`     | Page lifecycle: loading, pipeline, input, hit testing, navigation, history.    | all of the above              |
| `automation` | Remote-control protocol (planned for M1; the crate is empty).                  | `engine`                      |
| `swb`        | The binary: CLI, window, browser UI, headless runner.                          | `engine`, `net`, `paint`; `dom`, `layout`, `style`, `text` for the toolbar and debugging dumps |

Rules:

- Lower crates must not depend on higher crates. `css` and `text` do not know
  about the DOM. `css` matches selectors through a trait that `style`
  implements for DOM elements.
- Only `swb` depends on windowing libraries.
- Only `net` does network I/O.

## Key data structures

- **DOM** (`dom`): nodes are stored in a `Vec` and addressed by `NodeId`
  (a `u32` index). Nodes link to parent, siblings and children by `NodeId`.
  This makes the tree easy to traverse without reference counting. It also
  gives a stable handle for the automation API and, later, for JavaScript.
  The parser limits the tree depth to 512 (as Blink does), because later
  stages recurse over the tree.
- **Computed style** (`style`): one `Arc<ComputedStyle>` per element. Elements
  that share a style share the `Arc`.
- **Box tree** (`layout`): built from the DOM and computed styles. It contains
  anonymous boxes (for example, anonymous block wrappers around inline
  content).
- **Fragment tree** (`layout`): the output of layout. It contains positioned
  boxes, line boxes and glyph runs in CSS pixels. It is immutable. The engine
  rebuilds it when the input changes.
- **Display list** (`paint`): a flat list of drawing commands (rectangles,
  borders, glyph runs, images, clips, opacity groups with their bounds) in
  paint order. The rasterizer consumes it. The rasterizer can be replaced
  without changes to layout. The list also contains hit regions, so hit
  testing finds what is painted on top.
- **Session history** (`engine`): one entry per committed document or
  fragment, with the scroll position to restore. A navigation is pending
  until its document arrives; starting a navigation cancels all running
  requests (`Loader::cancel_all`), and only a committed navigation adds a
  history entry.

## Units

Layout uses `f32` CSS pixels. Paint multiplies by the device pixel ratio
(HiDPI scale factor) only at rasterization.

## Threads

- The engine runs on one thread.
- `net` runs fetches on a pool of worker threads. Results return to the engine
  through a channel. The engine processes them when it is polled.
- The GUI shell runs the window event loop on the main thread and calls the
  engine. Network completions wake the event loop.
- The automation server reads requests on its own thread and sends them to the
  engine thread through a channel.

## Layout details

- The box tree (`layout/src/box_tree.rs`) is rebuilt for every layout.
  Inline content is a flat list of items (text, inline box start/end,
  atomic inlines, line breaks); white space is collapsed while the list is
  built.
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
- Flex item layouts are cached per layout pass by box and constraints, so
  nested flex containers do not cost exponential time. Fragment children
  are shared (`Arc`), so a cached subtree is stored once and copying a
  fragment is cheap.
- The overflow of the root (or of the body, if the root's is `visible`)
  applies to the viewport (CSS Overflow 3 §3.3); the source element then
  has a used overflow of `visible`.
- List markers wait for the first line box of the list item, also inside
  nested blocks and independent formatting contexts.
- Paint order: each stacking context (root, positioned boxes, opacity < 1)
  paints its normal-flow content in tree order, then its positioned
  descendants in z-index order (negative z-index before the content). A
  positioned box keeps the overflow clips of the boxes between it and its
  stacking context (except absolutely positioned boxes, whose containing
  block is outside those boxes).
- Rasterization works in device pixels. Rectangles without rounded corners
  and clips are snapped to whole pixels; an opacity group draws into a layer
  that covers only its visible bounds.

## Testing

See [testing.md](testing.md) and [ADR 0005](adr/0005-testing-strategy.md).
Python tools for fixtures and Chromium comparisons are in `tools/`.
