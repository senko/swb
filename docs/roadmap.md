# Roadmap

Milestones are ordered. Each milestone ends with one or more commits that
leave the browser usable. Update this file when plans change; record
completed work in [devlog.md](devlog.md).

## M0: Foundation (vertical slice) — done

Open a URL in a window and read the page.

- Repository, documentation, ADRs, tooling (`just check`, CI, cargo-deny).
- `net`: `http`, `https`, `file`, `data`, `about` URLs; redirects;
  compression; worker-thread loading; fixture replay and recording.
- `dom`: arena DOM, html5ever tree builder, encoding detection.
- `css`: tokenizer, parser, selectors (Level 4 incl. `:has()`), media
  queries, `@supports`.
- `style`: cascade, ~110 longhands with shorthands, custom properties,
  user-agent stylesheet, presentational hints, quirks mode.
- `text`: fontconfig font selection with Chromium's rules, fallback,
  harfrust shaping, glyph rasterization; bundled test fonts.
- `layout`: block flow with margin collapsing, inline layout (line
  breaking, vertical-align, shaping across elements), list markers, replaced
  elements, flexbox, relative positioning.
- `paint`: backgrounds (colors, images, linear gradients), borders,
  text and decorations, stacking order, opacity, overflow clipping.
- `engine`: page loading, history, scrolling, hit testing, link clicks.
- `swb`: window with toolbar and address bar; headless screenshot and
  dump modes.
- Test tooling: fixture capture, Chromium references, comparison reports,
  scores with ratchet, layout tests.

## M1: senko.net, automation API — done

- Automation API (WebSocket, JSON-RPC style) with a Rust and a Python
  client; integration tests drive a headless browser
  ([automation.md](automation.md), ADR 0008).
- Element states: `:hover`, `:active`, `:focus`, `:focus-visible`,
  `:target` restyles; cursor shapes; Tab focus navigation with Chromium's
  focus ring; Enter follows the focused link (ADR 0009).
- Text selection (drag, double and triple click, Shift+click, Ctrl+A),
  highlight, copy to the clipboard and the primary selection.
- `capture-missing`: adds only the requests missing from a fixture.
- Performance baseline per fixture and stage
  ([performance.md](performance.md)).
- Target 1 (senko.net) done.

## M2: Hacker News

- Table layout (auto and fixed, separated and collapsing borders),
  `-webkit-center` alignment, quirks-mode line height and table cell
  rules: done (ADR 0010). Hacker News geometry 0.9876; the rest is the
  search `<input>` (forms).
- SVG images (`resvg`).
- Forms: text inputs, buttons, GET/POST submission.
- Cookies (RFC 6265bis jar, `SameSite`, Public Suffix List) and `POST`
  requests with `Origin`: done (ADR 0012).
- Target 2 (news.ycombinator.com) done.

## M3: Wikipedia

- Floats and clearance, absolute and fixed positioning.
- Grid layout.
- `mask-image` (icons), `@font-face`, `@import`.
- `overflow` scrolling inside elements.
- Target 3 (Wikipedia "Web browser") done.

## Backlog from M2

- Tables: `visibility: collapse`; fragmentation (repeated headers); the
  quirky margins of the first and last children of cells and of the body;
  the body and html fill-viewport quirks; collapsed-border joints
  (horizontal edges always cover the joint; Chromium decides per joint);
  column percentages are ignored only inside cells and flex containers
  (grid does not exist yet); inset/outset border shading; `width:
  min-content` keywords on inline-blocks. Limits: 10,000 columns;
  collapsed grids over 4M edges use each box's own borders and paint
  none; inline wrapper boxes only for the 8 innermost inline boxes.
- Cookies: keep the initiator in the history entry, so that reload and
  back/forward after a cross-site link do not send `SameSite=Strict`
  cookies; persistence; `Partitioned`, `Priority`, `__Http-` prefixes,
  Lax+POST; the redirect-tainted `Origin`; `Sec-Fetch-*` and `Referer`
  headers; raw bytes in cookie values (now re-encoded as UTF-8);
  Chromium's 30-day protection in global eviction.
- Owner decision: the Public Suffix List data is MPL-2.0 (ADR 0012).

## Backlog from M1

- Selection: the highlight of line ends inside the selection (Chromium
  paints a space-wide box); words across element boundaries for double
  clicks; extend by words after a double click; auto-scroll while
  dragging beyond the viewport; `::selection` styles; `text-transform` in
  the copied text; the tint of selected images (Chromium turns white into
  the selection color; the blend is not a plain alpha overlay).
- Hover: update `:hover` after a navigation commits and after a scroll
  ends without waiting for a mouse movement (Chromium uses a timer).
- Focus rings of inline elements split over lines: one outline around the
  union, as Chromium draws `outline-style: auto`.
- Automation: events (`page.loaded`, `page.navigated`); `input.type` with
  form inputs (M2); cancel a pending `page.waitForLoad` when its client
  disconnects.
- Incremental restyle for state changes (only the elements whose matched
  rules change); a hover on a page with `:hover` rules restyles all
  elements and often lays out again.

## Backlog from the M1 maintenance review

Bugs found by the end-of-milestone review. The maintenance commit did not
change behaviour, so they are not fixed yet.

- CSS: an at-rule that the parser drops (`@media screen;`, `@supports`
  with an invalid condition) ends the part of the sheet where `@import`
  is allowed (not checked against Blink).
- Style: the `background` and `list-style` shorthands reject
  `cross-fade()`, `element()` and `paint()` images (`looks_like_image`);
  `text-indent` accepts repeated keywords and `aspect-ratio` accepts
  `auto auto`; image URLs of pseudo-elements are requested in hash-map
  order, not in document order.
- Layout: a float that follows inline content in a flex container is
  dropped; it should be a flex item.
- Engine: a click on a visible child of a `visibility: hidden` focusable
  element focuses the hidden element; the disabled state of `option` and
  `optgroup` does not follow the HTML spec (a disabled `fieldset` should
  not disable them, a disabled `optgroup` should); after a navigation
  starts and the user stops it, the old page's cancelled images stay
  "loading" and do not load until a reload.
- Paint: `background-repeat: space` and `round` are painted as `repeat`,
  `background-attachment: fixed` as `scroll`; text decoration thickness
  comes from the font size, not from the font's underline metrics.
- Style: the substitution budget of custom properties is not what ADR
  0007 says: a nested `var()` reference costs only the size of its result
  (`Resolver::resolve` in `custom.rs`), so one element can copy up to
  about 128 × 100,000 component values. Still bounded.
- `swb --test-fonts` panics when the source tree (with
  `fixtures/fonts`) is not present; `swbtools perf` does not catch a swb
  timeout.

## Backlog from the M0 review

Found by the reviews before the first commit and not fixed yet. Layout
issues also have a test in `tests/layout/` listed in `known-failures.txt`.

- Layout: floats and absolutely positioned boxes inside inline content are
  not placed; absolutely positioned children of flex containers are
  dropped; flex container min/max sizes; `align-content: stretch` order; replaced flex items with
  only `height`; `vertical-align: top/bottom`; min-content of inline-blocks
  and row flex containers; `box-sizing` in intrinsic min/max; tab stops;
  `white-space: nowrap` around atomic inlines; inside markers in line
  breaking (and `text-indent`); column flex items with a definite height
  cannot shrink; empty lines ignore `text-align` and relative offsets;
  percentage `top`/`bottom` on inline boxes; `capitalize` across element
  boundaries inside words (`don<b>'t</b>`).
- Paint: `border-radius` does not clip backgrounds, images or overflow;
  `content-box` background clip/origin; gradients ignore background size
  and position, repeating gradients and implicit stop positions; block
  backgrounds should paint before inline content (Appendix E steps 4/7).
- Paint: a `position: fixed` box inside `overflow: hidden` is clipped
  (fixed positioning is not implemented yet).
- Engine: `<meta charset>` after the first 1024 bytes and charset for
  `text/*` documents; keep parsed stylesheets across resizes (only
  re-evaluate media queries); decode images off the UI thread; a total
  memory budget for decoded images; a fragment link to a `display: none`
  target scrolls to the following content.
- DOM: html5ever is quadratic for very deeply nested `<div>`s (its scope
  checks walk the stack of open elements).
- Text: spaces after fallback characters use the fallback font;
  `FcFontSort` for named families with old fontconfig; bounded caches for
  languages and features.
- Scrolling inside elements (needed when `html, body { height: 100% }` and
  the body has `overflow: auto`).

## Later

- Find in page, tabs, bookmarks.
- Bidirectional text.
- Incremental style and layout; GPU rasterization if needed.
- JavaScript (needs the owner's decision first, see ground rules).
