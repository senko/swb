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

## M2: Hacker News — done

- Table layout (auto and fixed, separated and collapsing borders),
  `-webkit-center` alignment, quirks-mode line height and table cell
  rules: done (ADR 0010).
- SVG images (`resvg`): `<img>` and CSS backgrounds, natural sizes and
  CSS default sizing, rendering at device resolution, resource limits:
  done (ADR 0011).
- Forms: text fields, text areas, buttons, checkboxes, radio buttons,
  drop-down selects, labels; editing; GET/POST submission with the three
  encodings; POST results in history; automation `input.type` and
  `dom.value`: done (ADR 0013).
- Cookies (RFC 6265bis jar, `SameSite`, Public Suffix List) and `POST`
  requests with `Origin`: done (ADR 0012).
- Target 2 (news.ycombinator.com) done: geometry 1.0 (all 818 element
  boxes within 2 px of Chromium), logo and vote arrows, search form.

## M3: Wikipedia — done

Five workstreams (ADRs 0015–0019), then a final pass on the page:

- Floats, clearance and block formatting contexts: done (ADR 0015).
- Absolute, fixed and sticky positioning; 2D `transform`: done (ADR 0016).
- Grid layout: done (ADR 0017).
- `mask-image` and the other mask properties (icons): done (ADR 0018).
- Scroll containers: `overflow` scrolling inside elements: done (ADR 0019).
- Responsive images (`srcset`, `sizes`, `<picture>`): done (ADR 0011
  update).
- Final pass: flex container min/max sizes and baselines, `<video>` and
  `<audio>` (ADR 0020), CSS counters, line breaking and text measurement:
  done.
- Target 3 (Wikipedia "Web browser") done and accepted by the owner:
  geometry 0.9978 (4,052 element boxes), pixels 0.9936.

`@font-face` and `@import` were planned for M3, but the Wikipedia page
uses neither; they moved to "Later".

## After M3: shared tools, robustness, maintenance

One feature at a time, in this order:

1. Shared tools for the agent workflow (CLAUDE.md, "Agents"): done.
   - Review worktree: `tools/review-tree.sh` (`just review-tree`), a
     persistent worktree with its own target directory.
   - Chromium probe: `swbtools probe` runs case files (HTML documents and
     what to read: boxes, line rectangles, computed values, script
     values) in Chromium with the test settings, and optionally in swb,
     and prints compact results. Replaces ad hoc measurement scripts.
     `measure` and `linebreaks` stay for the data they regenerate.
   - Hostile-page set: `swbtools hostile` (`just hostile`) generates the
     hostile pages behind the limits in ADRs 0007, 0010, 0011 and
     0015–0019, runs swb on each with a time and memory limit, and fails
     on a panic, a timeout or too much memory. Known failures are listed
     with their reason.
   Separate files: the probe and the hostile set are separate modules of
   `tools/swbtools/` and can be built in parallel.
2. Generated content size limit (the high-priority robustness bug in
   "Backlog from M3"): done, 1 MiB per box tree.
3. M3 maintenance: review the whole codebase for duplication, dead code,
   unclear names and outdated docs; `just snapshot` shows no change: done.

## M4: Ars Technica — done

Target 4, `https://arstechnica.com/` (fixture `ars-technica`), JavaScript
off. Baseline on 2026-10-08: geometry 0.0531, pixels 0.7841 (1,943
elements). The page is 2,206 px shorter than in Chromium. Causes, by
effect on the page:

- 61 boxes with `aspect-ratio` (`aspect-video`, `aspect-square`) get no
  height, so the card images are not visible.
- All text uses web fonts (`@font-face`: WOFF2, variable fonts,
  `unicode-range` subsets, `size-adjust` fallback faces).
- 129 inline `<svg>` elements (logo, icons) are not drawn; their
  children get empty boxes.
- The rule `:host,html{...}` (Tailwind's base rule: root font family and
  line height) is dropped, because swb rejects `:host`.
- `sizes="auto"` gives `100vw`, so swb loads 1536w candidates where
  Chromium loads 384w ones.

Features, in this order. Each one ends with a review and a commit.

1. `aspect-ratio` for non-replaced boxes (`implementer`). Scope: CSS
   Sizing 4 §5 for block-level boxes, inline-blocks, flex and grid items
   and absolutely positioned boxes: the ratio gives the size of the
   axis that is `auto` from the other axis, automatic minimum size
   (`min-height: auto` for overflowing content), the `box-sizing` rule
   of the ratio, `auto && <ratio>`. The Ars cases: `aspect-video` and
   `aspect-square` blocks with an absolutely positioned `object-fit:
   cover` image inside, in grid and flex layouts. Done: all 61 boxes
   match; geometry 0.1491, pixels 0.9592.
2. `:host` and `sizes="auto"` (`implementer`). Scope: `:host`,
   `:host()` and `:host-context()` parse and match nothing (there are no
   shadow trees), so a selector list that contains them stays valid.
   `sizes="auto"` on lazy images selects the candidate by the laid-out
   width (HTML "sizes auto"), with the user-agent rule for such images
   (`contain: size` and `contain-intrinsic-size: 300px 150px`, as far as
   replaced elements need them). Then capture the Ars fixture again and
   commit it. Done: geometry 0.1491 → 0.1876. The new capture has the
   same 103 resources: Chromium also loads the `100vw` candidates, for
   the hidden (`display: none`) copies of the card images. After
   `just substitute` the fixture is 4.17 MB. On the new capture:
   geometry 0.1830, pixels 0.9595.
3. Web fonts, part 1 (`implementer-hard`, new ADR). Scope: `@font-face`
   with `font-family`, `src` (`url()` with `format()`; WOFF, WOFF2,
   TrueType and OpenType), `font-weight`, `font-style` and
   `font-stretch` (also ranges), `unicode-range` and `font-display`;
   loading from the style sheet's base URL; a face loads only when text
   needs it (family match and `unicode-range`), as in Chromium; layout
   again when a font arrives; headless mode waits for fonts as it waits
   for images; the CSS Fonts 4 §5 font matching algorithm across the
   faces of a family, then the existing fallback; synthetic bold and
   italic; variable fonts with the `wght`, `wdth`, `slnt` and `ital`
   axes set from `font-weight`, `font-stretch` and `font-style` (both
   Ars families are variable; the default instance of Faustina is
   Light). WOFF2 decoding with a library (candidate: `wuff`, MIT) or own
   code from the W3C specification. Limits for hostile fonts (file and
   decoded size, faces per page, time) and cases for the hostile-page
   set. Done (ADR 0022, `wuff` with swb's own decompressors and
   limits): geometry Ars 0.1830 → 0.6908, BBC 0.0240 → 0.7775. Ars
   pixels went down (0.9595 → 0.9544): the headings use
   `font-variation-settings: "wght" 660` (part 2), so swb draws them at
   `wght` 700, wider, and they wrap differently.
4. Web fonts, part 2 (`implementer`). Scope: `font-variation-settings`
   (Ars headings use `"wght" 660`), `font-feature-settings`, the metric
   descriptors `size-adjust`, `ascent-override`, `descent-override` and
   `line-gap-override`, and `src: local()` (measure what Chromium
   matches with the test fonts). Done (ADR 0022, part 2): both
   properties and the same `@font-face` descriptors, the four metric
   descriptors, `local()` by full name or PostScript name. Ars
   geometry 0.6908 → 0.7740, pixels 0.9544 → 0.9898. BBC is
   unchanged: its `local("Arial")` fallback faces do not match in
   Chromium either.
5. Inline SVG, part 1 (`implementer-hard`, new ADR). swb draws inline
   SVG itself; ADR 0003 allows resvg only for SVG as an image format.
   Scope: `<svg>` in HTML as a replaced box (CSS sizing, `width` and
   `height` attributes, `viewBox`, `preserveAspectRatio`); `g`, `path`,
   `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`; the
   properties and presentation attributes `fill`, `fill-rule`,
   `fill-opacity`, `stroke` and its longhands, `opacity`, `transform`,
   `display`, `visibility`, `color` (`currentColor`); `title`, `desc`
   and `style` inside SVG draw nothing (the BBC logo has a `title`);
   styling by the page's style sheets through the cascade; paint into
   the display list (paths with fill and stroke, rasterized with
   tiny-skia). `svgtypes` parses path data and transforms. Done (ADR
   0023): limits and seven hostile-page cases; nested `svg` is left for
   later. Ars geometry 0.7740 → 0.7978, pixels 0.9898 → 0.9952; BBC
   geometry 0.7775 → 0.8053, pixels 0.9942 → 0.9985. Chromium reports
   boxes for `g` and the shapes; swb does not yet (part 2), so these
   boxes count as missing (Ars 217, BBC 202).
6. Inline SVG, part 2 (`implementer`). Scope: `clipPath` (all Ars icons
   use it), `defs`, `use` with local references, the box dump of SVG
   descendants as Chromium reports them (bounding boxes of shapes and
   groups; none for `defs` and `clipPath`), hit testing (an SVG inside
   a link follows the link), `shape-rendering` (BBC sets `crispEdges`
   on 120 icon paths), limits and hostile-page cases. Done (ADR 0023,
   part 2): `symbol` and nested `svg` are left for later. Ars geometry
   0.7978 → 0.9963 (no missing boxes), BBC 0.8053 → 0.8786 (the 229
   missing boxes are the content of closed `details`, M5); pixels
   unchanged.
7. Raster cost of dense paths (`implementer`). The review of item 6
   found that the cost estimate charges a path segment about 9 ns
   (`SEGMENT` in `paint/src/svg/cost.rs`, used by `raster/path.rs` and
   `raster/svg_clip.rs`), but one tall self-overlapping path of 40,000
   cubic curves in 100×100 px takes about 4 s to fill: the time grows
   with the segments times the scanlines they cross. A page with five
   such fills takes 22 s. Scope: a cost model that bounds this for
   inline SVG paths, clip paths and SVG images (or a cap on segments per
   path), measured, with hostile-page cases. Repro: one `<path>` of
   40,000 cubic curves, each across the full height of a 100×100 px
   `<svg>`, used as five fills or as a clip path referenced five
   times. Done (ADR 0023, part 3): the cost of a path is a model fitted
   to measurements of tiny-skia: the rows that the edges cross, the
   pairs of edges whose bounding boxes overlap (counted in O(n log n),
   `paint/src/path_cost/edges.rs`) and the length of hairline strokes. Inline
   fills and strokes, clip coverage and SVG images (an exact term at the
   rendered size) are charged. Hit tests have a work budget;
   `fill_bounds` is cached. The repro pages take 0.1–0.6 s (before: 22 s
   to more than 120 s); a noisy chart of 40,000 points, 5,000 bars and a
   walk of 60,000 segments draw. The target pages do not change. Known
   gaps are in item 8 and the backlog.
8. Raster cost of anti-aliased fills of separate spans
   (`implementer-hard`). The review of item 7 found that the model
   charges a fill whose edges do not merge into long spans 5 to 15
   times too little. A comb of 2,000 to 4,000 separate full-height
   teeth, 0.25 px wide, in one path on 1,200 × 780 px (about 130 KB of
   HTML) takes 5 to 6 s per frame and is drawn; ten stacked combs of
   3,000 teeth, 78 px each, behave the same; the filled area under a
   noisy chart of 40,000 points takes 3.3 s. Scope: count the spans at
   sample rows (the crossings in x order and the winding under the fill
   rule) when many edges overlap in y, charge their measured cost, keep
   the work of the count bounded for 1,000,000 segments, and add
   hostile-page cases for the comb, the stacked combs and the area. The
   paths of item 7 (charts, bars, walks, images) must still draw. Done
   (ADR 0023, part 3): `paint/src/path_cost/spans.rs` counts the spans inside
   one pixel at sample rows and charges their measured cost, after an
   O(n) bound and only when the count fits the budget. The combs,
   stacked combs and the area are rejected in 0.05 s (before: 5–6 s,
   drawn), and so are dashed strokes of thin dashes (charged from their
   outline); counting spans in SVG images has a budget per document. The
   paths of item 7 still draw; snapshots unchanged.
9. Final pass on the page (`implementer`). A comparison after item 7
   (geometry 0.9963, pixels 0.9952; 4,964 differing pixels) found:
   - The four underline bars of the view selector (`div.absolute` in
     the buttons at y = 412) are 14 px too far right: swb puts the
     static position of an absolutely positioned flex child after the
     icon and the gap; CSS Flexbox 1 §4.1 places it as if it were the
     sole flex item.
   - The 12 round article thumbnails (67 × 67 px): Chromium clips the
     image to a circle, swb draws a square over the ring (3,145 px).
     Find the cause (`border-radius` with `overflow: hidden`,
     `clip-path: circle()` or a mask) and implement the clip that the
     page uses. The image inside also has a different scale (the
     placeholder grid is coarser in swb): check the `srcset` candidate
     and `object-fit`.
   - The header logo (about 700 px) and the "GRID SETTINGS" side of the
     view row (about 240 px) differ with matching boxes: find the
     cause; fix it only if it is not anti-aliasing (backlog otherwise).
   - Links and hover states: check that the `:hover` rules of the
     navigation and article links apply (probe), and that the link
     colors match. The page has no search form with JavaScript off.
   Then report target 4 as done. Done: the box tree put an absolutely
   positioned flex or grid child that follows text into the anonymous
   item of that text; overflow clips follow the rounded padding edge
   (also for deferred positioned descendants); raster images drawn at
   less than half their size are averaged before bilinear sampling;
   inline SVG is painted from the snapped origin of its content box; the
   page's `:hover` rules are tested (ADR 0024 for the clips and the
   reduction, with their budgets). Ars geometry 0.9963 → 1.0000,
   pixels 0.9952 → 0.9992; target 4 done.

Not in scope (not visible on the page with JavaScript off, or not
used): `box-shadow`, `filter`, `text-shadow`, `-webkit-line-clamp`,
`backdrop-filter`. Style takes 27 ms on this page (Wikipedia: 10 ms for
twice the elements); look at it in the final pass only if it grows.

Target 4 is done (geometry 1.0000, pixels 0.9992); the owner accepted
it. End-of-milestone maintenance: two read-only reviews
(paint and layout; the other crates, tools and docs) listed
duplication, dead code, names and outdated docs; one implementer applied
the selected items; `just snapshot` shows no change: done.

## M5: BBC — done

Target 5, `https://www.bbc.com/` (fixture `bbc`), JavaScript off.
Baseline on 2026-10-08: geometry 0.0290, pixels 0.9583 (2,745
elements). After M4 (web fonts, inline SVG), on 2026-10-09: geometry
0.8786, pixels 0.9985; the tag sequences match and swb logs no
warnings. The remaining differences:

- 229 missing boxes: the content of the closed `<details>` menu (the
  navigation for pages without JavaScript: 76 links in nested lists).
  Chromium puts the content of `details` in a `::details-content` box
  that has `content-visibility: hidden` while `details` is closed: the
  content is laid out (the box dump reports it) but not drawn. swb hides
  it with `display: none`. swb also does not toggle `details` when the
  user clicks its `summary`, so the menu does not open.
- The navigation drawer (off screen at x = -320, used with JavaScript):
  its 16 buttons are `display: grid` (`grid-template-columns: 1fr
  auto`). swb lays out the content of `<button>` only as flow or flex,
  so the icon goes to a second line (54 px high buttons instead of
  44 px).
- 1,526 differing pixels, in the text of headings and summaries. The text
  has no whole-pixel offset.
- `compare` scores only the first viewport (800 of 12,697 px) and only
  the initial state of the page.

The page does not need `quotes` (it sets only `quotes: none` and has no
`q`), and `:not()` with complex selectors already works. The page's
`::-webkit-scrollbar` rules each have only that selector, so swb drops
them with no effect.

Features, in this order. Each one ends with a review and a commit.

1. Comparison of the full page and of states (`implementer`; only
   `tools/`). Scope: `compare --full-page` takes Chromium's full-page
   screenshot of the replayed fixture (as `reference` does, after
   `document.fonts.ready`) and swb's `--full-page` screenshot into
   `out/compare/NAME/`, and adds a full-page pixel score and the
   differing regions to the report. Check that the full-page capture does
   not change Chromium's layout (`vh` units, fixed boxes). `compare
   --click SELECTOR` (repeatable) clicks the element in both browsers
   (Playwright; swb's automation API) before the boxes and screenshots
   are taken. The committed references and `scores.json` do not change.
   Then compare the BBC and Ars pages in full; add the BBC findings to
   item 4 and the Ars findings to the backlog. Done: Chromium's full-page
   capture does not change its layout (`vh` stays 800 px, fixed boxes stay
   at their first-viewport positions). Full-page pixels: BBC 0.9981
   (1280 × 12,697), Ars 0.9966 (1280 × 9,626). Neither page shows a
   layout difference or a missing paint feature. About 95 % of the
   differing regions are text: inside a word, swb puts the glyphs from
   some letter on at another sub-pixel position than Chromium. Most other
   regions are 1–2 px lines at the edges of scaled images. One Ars avatar
   shows another image (backlog).
2. `<details>` and `<summary>` (`implementer`). Scope (HTML §4.11.1 and
   §15.5.5, measured in Chromium with the probe tool): the children of
   `details` other than its first `summary` (also text) form the
   `::details-content` block box. While `details` is closed, this box
   acts as `content-visibility: hidden`: its content is laid out (match
   Chromium's boxes) but not drawn, not hit tested, not reached with
   Tab, and not part of the scrollable overflow or the text selection.
   A `details` without a `summary` gets Chromium's default summary.
   Activation: a click on the first `summary` (not on a link or other
   interactive content in it), or Enter or Space on it, toggles the
   `open` attribute and lays out the page again; `[open]` and `:open`
   match and the marker changes. The BBC menu matches Chromium closed
   and open (`compare --click`). Hostile-page cases: deeply nested
   `details`, much hidden content. Not in scope: the `name` attribute
   (exclusive accordions), the `toggle` event, `::details-content` in
   page style sheets, `content-visibility` as a property. Done: the
   content box of a closed `details` has size, layout and paint
   containment (an internal style flag, not the CSS property); after
   layout its children move to `BoxFragment::hidden`, which the box dump
   reads and paint, hit testing, selection and scrolling do not. Closing
   a `details` removes the focus and the selection from its content.
   Inside disclosure markers are 1.0592 em wide (measured) and no longer
   make the line taller. BBC geometry 0.8786 → 0.9660, no missing boxes;
   the open menu (`compare --click`) matches Chromium (pixels 0.9985).
   The remaining differences are the drawer buttons (item 3).
3. Grid buttons and `scrollbar-width` (`implementer`). Scope: `display:
   grid` and `inline-grid` on `<button>` lay out its content as a grid
   (the drawer buttons: `grid-template-columns: 1fr auto`, `gap: 22px`);
   `scrollbar-width: auto | thin | none`, where `none` hides the scroll
   indicator of that scroll container in the GUI (the page's two
   `overflow: scroll` carousels; Chromium shows no scroll bar there).
   Done: grid and flex buttons lay out their content as a plain grid or
   flex container (`grid::layout_contents` is shared with `layout_grid`).
   Measured: Chromium centers only flow content of a button vertically;
   swb also centered flex content, which is fixed. `scrollbar-width` is
   computed; `none` hides the indicator of the container, and the
   viewport follows the root element (CSS Scrollbars 1; the headless
   shell cannot show it). `thin` draws as `auto`. BBC geometry 0.9660 →
   1.0000 (no missing or extra boxes); pixels 0.9985; other fixtures
   unchanged.
4. Final pass on the page (`implementer`). Scope: the sub-pixel glyph
   positions in words (all text on both pages; find the cause, fix it if
   it is not anti-aliasing, backlog otherwise); the 1–2 px lines at the
   edges of scaled images (the same); links
   (also in inline SVG and in the menu) and the page's `:hover` rules
   (engine tests, as for Ars); `just perf bbc` compared with Ars. Then
   report target 5 as done. Done: the glyphs were at the right positions;
   their edges were not. tiny-skia resolves glyph edges to 1/4 px,
   FreeType (Chromium) gives exact coverage. Glyph masks that the cache
   keeps are now filled at 4 times the size and averaged down (1/16 px
   edges; ADR 0006 update). The image lines came from `<img>` boxes at
   fractional positions: Chromium scales the image into the pixel-snapped
   rectangle, swb sampled with the unsnapped one. Engine tests cover the
   page's links (cards, the SVG logo, section titles with SVG, the menu)
   and `:hover` rules; they found no bug. Perf: BBC 25 ms (9 µs per
   element), Ars 40 ms (21 µs). BBC pixels 0.9985 → 0.9987, full page
   0.9981 → 0.9987; Hacker News 0.9981 → 0.9988, senko.net 0.9973 →
   0.9989. Target 5 done.

Target 5 is done (geometry 1.0000, pixels 0.9987); the owner accepted
it. End-of-milestone maintenance: two read-only reviews
(layout, paint and text; the other crates, tools and docs) listed
duplication, names and outdated docs; one implementer applied the
selected items and measured a new performance baseline; `just snapshot`
shows no change: done.

## M6: JavaScript — preparation

Decisions: ground rules (2026-10-09) and ADR 0025. The owner gives
target pages later; this preparation does not depend on them. One step
at a time, in this order:

1. Study: five `analyst` sessions answer the questions in
   [js-study/README.md](js-study/README.md), one memo each (read-only
   research, so they may run in parallel). The orchestrator reviews each
   memo for code, pseudocode and copied structure, and records the
   sources in docs/credits.md. Done 2026-10-09: five memos, reviewed;
   MicroQuickJS added after its license check (MIT).
2. Design: a session that has read only the specifications, the memos
   and the literature writes the architecture ADR of the language core
   (crates, parser, bytecode, heap and GC, values, strings, objects,
   limits) and an ordered feature plan in this file. Done 2026-10-09:
   [ADR 0026](adr/0026-javascript-engine-architecture.md) (crates
   `js-text`, `js-regexp`, `js-syntax`, `js`; AST front end; register machine;
   mark-and-sweep over arenas with generational handles and handle
   scopes); the feature plan is M7 below.
3. Spike on the branch `js-spike`: a vertical slice that tests ADR 0026
   before the main work depends on it. Six sessions, in this order:
   1. Crates, `js-text` and the lexer (`implementer-hard`). The four
      crates (`js-regexp` only a stub that accepts any pattern).
      `js-text`: code units of both widths, UTF-8 conversion, the
      identifier tables (ADR 0003 decision: generated tables or a
      crate). The full lexer: all tokens, templates, regular expression
      literals, numeric literals, escapes, Unicode identifiers, on demand
      with the goal symbol from the parser. Done 2026-10-09: all 60 BBC
      scripts lex (1.13 million tokens, about 125 MB/s in a release build,
      with a previous-token heuristic in the test driver). Open: the lexer
      loses the interner when the source is too long (session 2 decides);
      the interner stores each name twice (performance, later).
   2. A parser for a subset and the scope analysis (`implementer-hard`).
      The AST arena. The subset: `var`, `let`, `const`; function
      declarations and expressions; generator functions with `yield`
      (no `yield*`); arrow functions; `if`, `while`, `do`-`while`, `for`
      (three-part, with one `let` binding per iteration); labels,
      `break`, `continue`, `return`, `throw`, `try`-`catch`-`finally`;
      literals (number, string, template without tag, object, array),
      member access, calls, `new`, `this`, unary, binary, logical,
      conditional, assignment and compound assignment, `typeof`, comma.
      Scope analysis with registers, cells and the temporal dead zone.
      The shared recursion budget in the parser. The early errors of the
      subset. Done 2026-10-09: 72 MB/s for parse and scope analysis of a
      5 MB generated program, about 31 bytes per token in memory; 3 of
      the 60 BBC scripts parse, the others stop at the first construct
      outside the subset (`for`-`in`, destructuring, spread, tagged
      templates, default parameters, `switch`, optional chaining, classes,
      rest; M7 feature 1). Long left-associative chains stop at about
      16,000 links (V8 accepts 1,000,000) and right-nested `?:` chains at
      about 1,360 terms (Node accepts 30,000; minifiers turn `if`-`else`
      chains into `?:` chains). Measure the longest chains in the target
      scripts; if they come near the limits, lower the budget weights of
      these paths or walk such chains without recursion.
   3. Heap, values, strings, objects and GC (`implementer-hard`). Arenas
      with generations, the value enum, flat strings of both widths (no
      ropes), the weak atom table, shapes with root shapes, transitions
      and dictionary mode, dense elements, mark-and-sweep with a work
      list, handle scopes, safepoints, reservations, byte accounting and
      the heap limit, the stress mode. Done 2026-10-09: GC pause 16 ms for
      1 million small objects, 202 ns to allocate `{a, b, c}`, 3.1 ns per
      handle access with the generation check, 4.2 ns to record a handle
      in a scope, 169 bytes per small object; changes to the design are
      in ADR 0026 section 14. Open: arenas reuse free slots but give back
      only the free tail; `own_property_keys` returns an unaccounted
      vector; the realm flag for array prototypes; ephemerons.
   4. Compiler and interpreter core (`implementer-hard`). Register
      allocation, the instruction enum with its size test, property
      sites, the verifier, frames without Rust recursion, calls with the
      argument rules of ADR 0026 section 5, deferred calls, closures with
      cells, and a minimal generator (`yield` and `next()` in a function
      with a captured variable, to test that frames can move). Done
      2026-10-09: 12-byte instructions, property sites, verifier, line
      table; frames, deferred and native calls, closures, generators,
      the realm and the global lexical record, the embedding API; about
      290 scripts compared with Node 22, each also in GC stress mode, and
      5,076 number-to-string cases. Against `node --jitless` after this
      session (current numbers: docs/performance.md): calls 4.7x, an
      integer loop 1.0x, property access 4.7x, closures 3.1x, object
      literals 10.6x slower; code 5.5 bytes per source byte
      (line table 6.5 of 28.6 MB). Open: member and call chains stop in
      the compiler at about 2,700 links (the parser accepts 16,000);
      the parser stops `else if` chains at about 1,360 and nested
      function expressions at about 370 (debug build); the value stack
      is not counted in the heap limit (bounded by the stack limit);
      the script source stays alive, uncounted, while a closure of the
      script lives; the completion value is approximate; each ordinary
      closure allocates its `prototype` object at once.
   5. Exceptions, limits and built-ins (`implementer-hard`). The handler
      table, `finally` completions, termination, the time countdown, the
      frame limit, the recursion budget. Built-ins: `console.log`, the
      `Error` constructors, `Object.keys`, `Array` (index, `length`,
      `push`, `join`, `forEach`, `map`: natives that call back, for the
      handle scopes), `Function.prototype.call` (the deferred call),
      `String()` and `Number()`. Done 2026-10-09: handler tables,
      `finally` with a stored completion, `switch` (added to the
      parser), generator `return` and `throw`, termination (time limit,
      heap limit, host request; uncatchable; the runtime stays usable),
      the time countdown with charges in proportion to the work of
      built-ins, and the listed built-ins plus `Object`,
      `Object.prototype.toString`/`valueOf`/`hasOwnProperty` and
      `Function.prototype.toString`; 210 more scripts compared with
      Node 22, also in GC stress mode; the reviewer compared 6,000 random
      programs with Node without a difference. Against
      `node --jitless`: a `try` that never throws 1.15x, `forEach` over
      1M elements 9.7x (the callbacks re-enter the interpreter), fib
      5.3x (20 % slower than after session 4 because the global object
      grew; the global cache of M7 feature 13 removes the by-name
      lookup). Open: an error that `finally` rethrows reports the
      offset of the `finally` block; catch code sits inline after the
      `try` block (one jump on the normal path); writes to index
      properties of String objects create own properties;
      `Number.prototype.toString(radix)` prints all digits of fractions
      instead of the shortest form; `console.log` has no `%s` and no
      column grouping; errors raised by natives are created in the
      realm of the catching frame.
   6. Tools (`implementer`). The `swb-js` shell with `$262`;
      `just test262` (test262 at a pinned commit in a git-ignored
      directory, a list of features in scope, the scores file; in the
      spike only tests that need no harness file other than `assert.js`
      and `sta.js`, from a list of `language/` directories that the
      subset covers); `just jsdiff` against Node.js; benchmarks against
      `node --jitless`. Done 2026-10-09: the `swb-js` shell (files,
      `-e`, stdin, limits, stress mode, `--disassemble`, exit codes,
      `$262` without `createRealm`); `just test262` (the pinned commit
      in the `justfile`, the subset and scores files in
      `crates/js/test262/`): 98 groups, 3,762 pass, 1,028 fail, 3,476
      unsupported, 1,917 skipped, in 0.3 s, no panics; `just jsdiff`;
      `just jsbench` (docs/performance.md). The harness files needed no
      new built-ins. Open: the error position of `null.x` is the start
      of the statement; `$262.evalScript` loses unpaired surrogates.

   Exit: a report with the measurements that ADR 0026 lists under
   "Consequences" (handle scope overhead, generation checks, instruction
   size, interpreter speed against `node --jitless`, GC pause for one
   million objects, lexing speed on the BBC scripts), the test262 pass
   count of the subset, the generator test, and the hostile cases (deep
   nesting, endless loops, deep recursion, huge allocations). Then
   ADR 0026 gets an update where the spike showed a better choice, and
   the branch merges into `main` as the base of M7, or the design
   changes first. Done 2026-10-09:
   [docs/js-spike-report.md](js-spike-report.md). The design holds;
   the choices of the spike are in ADR 0026 section 14; `js-spike`
   merges into `main`.
4. With the first engine integration: the JavaScript on/off setting
   (ADR 0025; off by default): command line, automation API, browser
   window; the scripting flag in the HTML parser, the serializer,
   `@media (scripting)` and media controls.

When the owner gives targets: survey each target's scripts first
(bundle sizes, the language features they use, and the Web APIs that
they call during load, measured in Chromium), and agree what "works"
means per target (for example menus, carousels, lazy images). That sets
the scope of the bindings, as the CSS feature lists did for M4 and M5.

## M7: JavaScript language core

Starts after the spike (M6 step 3) is merged. Design: ADR 0026. Scope:
ECMA-262 2025 with Annex B, no `Intl`. Each feature ends with a review
and a commit, and raises the test262 scores file for the directories
that its scope names. A test that fails only because it needs a later
feature is a known failure with that reason. At the end of M7, each
area passes at least 95 % of its tests in scope; the rest are known
failures with a reason. Features, in this order:

1. Full syntax (`implementer-hard`, `js-syntax`), in three sessions.
   The whole ES2025 grammar with Annex B, all early errors (those of
   regular expression literals come with feature 8a) and the complete
   scope analysis. Until feature 3, the compiler in `js` rejects the
   new constructs with "not supported yet". Tests in each session: a
   parse-only mode of the test262 runner (a negative test of phase
   `parse` must give a `SyntaxError`, every other test must parse;
   scores per group in their own file, which may only go up), unit
   tests with V8's messages, measured in Node, and the hostile inputs
   of the spike for the new forms. The target scripts are in the
   git-ignored `out/bbc-js/` (60 BBC scripts) and `out/ars-js/` (24
   Ars Technica scripts, external and inline); they are not in the
   repository, and `URLS.txt` in each directory lists their sources.
   - 1a. Patterns and the remaining expressions: destructuring in
     declarations, parameters, `catch` and `for` heads, and
     destructuring assignment through the cover grammar; spread and
     rest; default parameters (with the separate scope of parameter
     expressions); `for`-`in` and `for`-`of` (the `let` and `async of`
     lookahead rules, Annex B.3.5 initializers); optional chaining;
     tagged templates; `new.target`; `BigInt` literals; `yield*`;
     computed keys, methods and accessors in object literals; `with`
     (an early error in strict code). The parse-only mode of the
     runner comes in this session. Done 2026-10-09: all of the above;
     the compiler rejects the new forms with "not supported yet".
     Parse-only test262 (`just test262 --parse-only`, 48,635 tests of
     `language/`, `built-ins/` and `annexB/`): 28,203 pass, 364 fail
     (358 regular expression syntax, feature 8a; 6 Annex B.3.2.4, 1c),
     12,986 need classes, `async` or modules. Runtime test262: 4,152
     pass (was 3,762; negative syntax tests). 34 of 60 BBC and 19 of
     24 Ars scripts parse (were 3 and 10); the others stop at `class`
     or `async`. 47 to 52 MB/s over the scripts that parse. Open: a
     non-simple parameter list takes two registers per parameter, so
     more than about 32,000 such parameters fail (feature 3).
   - 1b. Classes and `async`: class declarations and expressions
     (heritage, constructor, methods, accessors, static members,
     fields, private names and `#x in o`, static blocks), the rules for
     `super` properties and calls, `async` functions, arrows (the
     `async (` cover grammar), methods and generators, `await` and
     `yield` as identifiers by context, `for await`. Done 2026-10-10:
     all of the above with their early errors; the compiler rejects
     the new forms with "not supported yet". Annex B.3.2.4 duplicate
     block declarations of generators and async functions are errors
     now (this was in 1c). Parse-only test262: 40,604 pass, 358 fail
     (all regular expression syntax, feature 8a), 591 use modules.
     Runtime test262: 4,231 pass. All 60 BBC and 24 Ars scripts parse,
     48 MB/s. Open: `await` in field initializers follows V8 (an
     identifier in instance fields, reserved in static fields; test262
     has no test); each private name takes a register of the enclosing
     function, so a class with more than about 64,500 private names
     fails (feature 3).
   - 1c. Modules and the rest of the scope analysis: the Module goal
     (the `import` and `export` forms and their early errors,
     `import.meta`, `import()`, top-level `await`); direct `eval` and
     `with` in the scope analysis (the names in their reach resolve at
     run time); mapped `arguments`; Annex B.3.2 and B.3.3 (function
     declarations in blocks, in `if` statements and labelled statements
     in sloppy code). Exit of feature 1: all BBC and Ars scripts parse
     without errors; the parse-only test262 scores of `test/language/`.
     Done 2026-10-10: the Module goal (imports and exports with
     attributes and string names, the `export default` forms,
     top-level `await`, `import.meta`, the module early errors, the
     module records), `import()` with options, eval code with the early
     errors of its call site, direct `eval` and `with` in the scope
     analysis, the implicit bindings of functions with `eval`, mapped
     `arguments`, Annex B.3.2 and B.3.3. The compiler rejects module
     code, `import()` and `import.meta`, and copies hoisted block
     functions into their var bindings. Parse-only test262: 41,882 pass
     (`language/`: 22,837), 358 fail (all regular expression syntax,
     feature 8a), none unsupported. Runtime test262: 4,235 pass. All 60
     BBC and 24 Ars scripts parse, 49 MB/s. The deepest member chain in
     the target scripts is 16 links, `else if` chain 5, nested function
     expressions 10, far below the compiler limits of the spike. Exit
     of feature 1 reached. Open: module top-level bindings are
     registers (about 64,500 at most) and imports are captures (65,535
     at most; feature 14); global Annex B vars are declared like `var`,
     without the run-time checks of B.3.2.2 (feature 3).
2. Fundamental objects (`implementer`). What the test262 harness and the
   language need first: `Object`, `Function` (`bind`, `call`, `apply`,
   `toString`), `Boolean`, `Symbol` with the well-known symbols, the
   `Error` types with `cause` and Chromium's `stack` format, `Reflect`,
   the global functions, all property-attribute paths
   (`defineProperty`, `freeze` and others), the array iterator and the
   `Array.prototype` methods that the harness files use. Annex B:
   `__proto__`, `__defineGetter__` and the related methods. In three
   sessions. The test262 runner accepts more harness files in each
   session, so groups of `test/language/` that were skipped run too;
   each session updates the scores of all groups it covers. A failure
   that needs a later feature is a known failure with that reason.
   - 2a. `Object`, `Reflect` and the property paths. `Object`: the
     constructor (with `new.target`) and all static functions except
     `fromEntries` and `groupBy` (they need iterators, 2b).
     `Object.prototype`: `hasOwnProperty`, `isPrototypeOf`,
     `propertyIsEnumerable`, `toLocaleString`, `toString` (without
     `@@toStringTag`, 2b), `valueOf`; Annex B: the `__proto__`
     accessor, `__defineGetter__`, `__defineSetter__`,
     `__lookupGetter__`, `__lookupSetter__`. `Reflect`: all 13
     functions. `Function.prototype.bind` and bound function objects
     (§10.4.1). The internal methods `[[GetOwnProperty]]`,
     `[[DefineOwnProperty]]` and `[[OwnPropertyKeys]]` for each object
     kind that exists now: arrays (`length`, ArraySetLength), String
     objects (§10.4.3), functions; `freeze` and `seal` of arrays;
     prototype cycles and non-extensible objects in
     `setPrototypeOf`. Globals: `isNaN`, `isFinite`, and `Math.pow`
     alone (`propertyHelper.js` needs it; the rest of `Math` is
     feature 4). Runner: the harness files `propertyHelper.js`,
     `compareArray.js`, `isConstructor.js`, `fnGlobalObject.js`,
     `nans.js`, `decimalToHexString.js`; the groups `Object`,
     `Reflect`, `isNaN`, `isFinite`, `global` and `ThrowTypeError` of
     `test/built-ins/`. Done 2026-10-10: all of the above. A call of a
     bound function walks the chain in a loop (100,000 levels work);
     `Reflect.construct` and bound constructors end in a deferred
     `[[Construct]]` with `new.target`. The heap answers the internal
     methods of String objects for their characters. Runtime test262:
     6,679 pass (was 4,235): the six new groups pass 1,836, the
     `test/language/` groups 608 more (the harness files, and 10 new
     `dir` lines). Parse-only unchanged. Known failures: 1,153 tests
     of the new groups need `for`-`in` (feature 3), because
     `propertyHelper.js` uses it (with a patched copy of the file the
     six groups pass 2,677); `Boolean`, `Symbol` and `@@toStringTag`
     (2b); the poison pills of `Function.prototype` and
     `Array.prototype.indexOf` (2c); `Math` and `Number` constants
     (feature 4); `Date`, `RegExp`, `JSON`, `Proxy`. Open: the name of
     a bound function is limited to 32,768 code units, because each
     bound function stores a flat copy (a loop of `f = f.bind()` would
     store O(n²) units; ropes, feature 5, remove the limit); argument
     lists of 2^27 values or more are a `RangeError`, as in V8;
     `Heap::get` in the embedding API does not see the characters of a
     String object on a prototype chain (the VM does); a key list that
     does not fit under the heap limit (`Object.keys` of a String
     object of 2^28 characters) ends the script, where V8 throws
     `RangeError: Too many properties to enumerate`; the check does not
     collect first.
   - 2b. Symbols, `Boolean` and iteration. The well-known symbols (one
     set per runtime, §6.1.5.1); `Symbol` (the constructor, `for` and
     `keyFor` with the registry, the well-known symbol properties) and
     `Symbol.prototype` (`description`, `toString`, `valueOf`,
     `@@toPrimitive`, `@@toStringTag`). The symbols in the VM:
     `@@toPrimitive` in ToPrimitive, `@@hasInstance` in `instanceof`
     and `Function.prototype[@@hasInstance]`, `@@toStringTag` in
     `Object.prototype.toString` and on the built-in prototypes.
     `Boolean` with its prototype. Iteration: `%IteratorPrototype%`,
     `%ArrayIteratorPrototype%`, `Array.prototype.keys`, `values`,
     `entries` and `@@iterator`, the iterator operations for natives
     (§7.4), `Object.fromEntries` and `Object.groupBy`. Groups:
     `Symbol`, `Boolean`, `ArrayIteratorPrototype`. Done 2026-10-10:
     all of the above, and `@@iterator` on arguments objects. The
     well-known symbols and the registry belong to the runtime;
     `instanceof` merges InstanceofOperator and OrdinaryHasInstance in
     one loop, so bound chains need no Rust recursion. Runtime test262:
     7,215 pass (was 6,679): `Symbol` 39, `Boolean` 37,
     `ArrayIteratorPrototype` 13, `Object` 155 more, about 40
     `test/language/` groups more (the `Symbol` features no longer
     skip them). Known failures: `Array[@@species]` (2c), `Proxy`,
     `String.prototype` methods and `[@@iterator]` (feature 5), `Date`,
     `Number` constants (feature 4), `eval`, and the tests that use
     `for`-`in`, accessors in literals or destructuring (feature 3).
     Deviations from V8 that follow the spec and test262: an array
     iterator stays done after a getter throws; `Object.fromEntries`
     does not close the iterator when `next` throws or returns a
     non-object; swb cuts a quoted script string in any message at
     1,024 units (V8 cuts only some messages, at 100 units). Open:
     `Array.prototype[@@unscopables]` (2c),
     `GeneratorFunction.prototype.prototype`, a fast path for array
     iterators (feature 3).
   - 2c. Errors and the rest of `Function`. `Error` and the native
     errors with `message`, `cause`, `Error.prototype.toString` and
     `AggregateError`; the `stack` property in Chromium's format
     (measured in Node: own property and attributes, the frame lines),
     and V8's `Error.captureStackTrace` and `Error.stackTraceLimit`,
     which Chromium has and two BBC scripts feature-test
     (`Error.prepareStackTrace` is not in scope).
     `Function.prototype.toString` for all function kinds (§20.2.3.5),
     the `caller` and `arguments` properties of `Function.prototype`
     (§10.2.4). The `Array.prototype` methods that the harness files
     use and that are missing, `Array[@@species]` and
     `Array.prototype[@@unscopables]`. Runner: `nativeErrors.js` (not
     `nativeFunctionMatcher.js`, which needs `RegExp`); the groups
     `Error`, `NativeErrors`, `AggregateError` and `Function` (the
     `Function` constructor is feature 3). Done 2026-10-10: all of the
     above, with `ArraySpeciesCreate`, `indexOf` and `slice`. Every
     error object has an own `stack` accessor; the frames (at most
     `Error.stackTraceLimit`, never more than 200) are captured at
     creation and the text is built at the first read; source
     positions and the names of anonymous functions in traces follow
     V8. Runtime test262: 8,429 pass (was 7,215): `Error` 29,
     `NativeErrors` 44, `AggregateError` 9, `Function` 223, `Array`
     892 (the rest of `Array` is feature 6). Known failures: the tests
     that use `for`-`in`, `new Function` or `eval` (feature 3),
     `nativeFunctionMatcher.js` (`RegExp`, feature 8), `Date`, `JSON`,
     `Number` constants, `Proxy`. Deviations from V8: built-in
     functions, eval code and `async` functions are not frames;
     sloppy functions have no own `caller` and `arguments` (the spec
     has none); `captureStackTrace` on a plain object defines a data
     property. Cost: 1M `throw new Error` take about 440 ms (was
     245 ms; `node --jitless` 1,254 ms). Open: the names of methods
     with computed keys are empty at run time (feature 3);
     `console.log` of an error prints `[Name: message]`, not the
     stack; `Function.prototype.toString` of a class must use the
     class's span when the compiler compiles classes (feature 3).
     End of feature 2.
3. Language semantics (`implementer-hard`, `js`). The VM for everything
   of feature 1 except generators and `async`: destructuring, spread,
   classes and `super`, getters and setters, optional chaining,
   `for`-`in`, `for`-`of` with iterator closing, `switch`, tagged
   templates, `arguments`, `this` in sloppy and strict mode, direct and
   indirect `eval`, `with`, `new Function`, `new.target`, the Annex B
   function semantics. Tests: test262 `language/` without generators,
   `async` and modules.
4. Numbers (`implementer-hard` for the exact conversions, ADR 0026
   section 7). `Number`, `Math`, `parseInt`, `parseFloat`,
   StringToNumber, number-to-string in all radixes, `toFixed`,
   `toExponential`, `toPrecision`.
5. Strings and JSON (`implementer`). `String` without the regular
   expression methods, ropes, the string iterator, `normalize`, case
   mapping, the URI functions, `JSON` with its depth limit. Annex B:
   `escape`, `unescape`, `substr`, the HTML methods (`anchor` and
   others), `trimLeft` and `trimRight`.
6. Arrays and collections (`implementer`). `Array` with dense and sparse
   elements, the iterator helpers, `Map`, `Set` (with the ES2025 set
   methods).
7. Generators, promises, `async` (`implementer-hard`). Generator objects,
   `Promise`, the job queue and its host hooks, `async` functions, async
   generators, `for await`, async-from-sync iterators,
   `Array.fromAsync`.
8. Regular expressions (ADR 0026 section 10), in two parts:
   - 8a (`implementer-hard`, `js-regexp`): the pattern parser with
     Annex B, the compiler and the matcher, the `u`, `v`, `d`, `s`, `y`
     flags, lookbehind, named groups, Unicode properties and case
     folding (tables in `js-text`), and the early errors of regular
     expression literals in `js-syntax`. Because `js-regexp` is a
     separate crate, 8a may run in parallel with features 2 to 7; its
     changes to `js-text` and `js-syntax` merge between features.
   - 8b (`implementer`, `js`, after 5 and 8a): the `RegExp` built-in, the
     string methods that use it (`Symbol.match`, `replace`, `split`,
     `matchAll`), `RegExp.escape`, Annex B `RegExp.prototype.compile`;
     the legacy static properties (`RegExp.$1`) only if a target uses
     them.
9. Proxy and weak collections (`implementer-hard`). `Proxy` with all
   invariant checks, `WeakMap`, `WeakSet` and their ephemeron marking,
   `WeakRef`, `FinalizationRegistry`, the kept-alive list.
10. `BigInt` (`implementer-hard`).
11. Binary data (`implementer`). `ArrayBuffer` (resizable, detach,
    `transfer`), the typed arrays (with `Float16Array`, `BigInt64Array`
    and `BigUint64Array`), `DataView`, `Atomics` on non-shared buffers.
12. `Date` (`implementer`; the time zone library under ADR 0003).
    Annex B: `getYear`, `setYear`, `toGMTString`.
13. Performance (`implementer-hard`). Inline caches, a cache for
    global access, cached shapes for object literals, native callbacks
    without re-entry into the interpreter (`forEach` and similar),
    one byte per character for the kept source text of ASCII scripts,
    elision of temporal-dead-zone checks, and lazy built-ins or lazy
    compilation if measurements on the target scripts show the need.
    The spike measured calls 5x, property access 5x and object
    literals 11x slower than `node --jitless` (docs/js-spike-report.md).
14. Modules (`implementer-hard`), when a target uses
    `<script type="module">`.

After M7 (or interleaved, when the owner gives targets): the bindings
and the engine integration (layers 2 and 3 of ADR 0025), each with its
own ADR, and the on/off setting of M6 step 4.

## Pending decisions

- Clean-room rewrites of the code derived from Chromium (table layout
  column constraints, width distribution, row heights; grid track
  sizing; sticky offsets; two float helpers), Skia data and Servo
  (`collapsed_margin.rs`, MPL-2.0); kept with attribution for now
  (ADR 0021).

## Backlog from M6

From the maintenance review of the JavaScript crates. Items that an M7
feature already covers (regular expression patterns, loop-driven
`forEach` and `map`, pre-sized array literals, `toString(radix)`) are
not repeated here; the open items of each spike session are in M6 step
3 above.

- The scope analysis's capture map: check that a crafted script cannot
  drive it into long probe chains (it has the per-process key now).
- The test262 runner recognizes "not supported yet" by the message
  text; make it a structured field of `ParseError`.
- The lexer scans an identifier with a non-ASCII or escaped part twice;
  the interner copies one-byte names into a scratch buffer on each
  cache miss. Measure before a change.
- `Lexer::new` cuts the source at `MAX_SOURCE_LEN` without an error;
  `check_source_len` is the guard, and the examples skip it.
- `console.log` has no `%s` and no column grouping; errors raised by
  natives are created in the realm of the catching frame; `eval`
  returns an approximate completion value.
- The time-limit tests (`2 x deadline + 50 ms`) can fail on a loaded
  machine.
- `index_key` creates an atom for each index of 2^32 - 1 or more:
  measure a loop over such an array-like.
- `examples/heap_bench.rs` is not part of `jsbench`; its `rss()`
  repeats `peak_rss_kib` of `bench.rs`.
- Legacy `caller` and `arguments` of sloppy functions (M7 2c): V8
  gives sloppy functions own `caller` and `arguments` properties
  (`f.caller` is the calling function or `null`); swb follows the
  spec, so `f.caller` reaches `Function.prototype.caller` and throws a
  `TypeError`. Old code that walks `arguments.callee.caller` breaks.
  No target script uses `.caller`; decide when one does.
- `Error.prepareStackTrace` (V8) is not supported; a BBC script uses
  it only in an error path, behind a feature test.

## Backlog from M5

- Ars: the avatar at (64, 572, 75 × 75) in the full-page comparison shows
  another image in its circle (Chromium: a beige placeholder grid; swb: a
  teal one). Check the selected `srcset` candidate and the decoding.
- Tools, `compare --full-page`: the covering box of a region is only
  geometry, so it can name a box that is not drawn (the closed `details`
  menu on BBC). Chromium waits a fixed 200 ms after each `--click`; pages
  with timers are not covered.
- `details` (M5 item 2): Chromium draws the disclosure marker as a
  triangle, swb as a font glyph. An inside marker before a block child
  of `summary` is on its own line in Chromium (on the block's first line
  in swb; the intrinsic width approximates it); right-to-left inside
  markers are on the left in swb. Inside `disc` markers: Chromium starts
  the text at 22 px (16 px font), swb at 9.6 px. The default summary is
  not focusable. Fragment navigation, find and `scrollIntoView` do not
  open a closed `details`. A fixed box in closed content is placed
  relative to the viewport (Chromium: the content box). `columns` on
  `details` does not apply (the content box is a separate block). Hover
  and active states in content that closes stay until the next mouse
  move. `in_closed_details_content` walks the ancestors for each node in
  focus traversal (O(n · depth)). Two sources decide whether content is
  hidden: the selection reads the `contents_hidden` style, focus and
  activation read the `open` attribute (`in_closed_details_content`);
  they agree today. A `details` with `display: list-item` and no
  `summary` computes its `::marker` style twice, and the second
  overwrites the first (not measured in Chromium) (M5 maintenance
  review).
- Tests: `cargo test --release -p swb-layout --test deep_nesting` overflows
  the 2 MiB stack in `grids_nested_in_flex_containers` (also on d8c1a78;
  the debug build of `just test` passes). The browser lays out on the
  main thread (8 MiB) and the hostile case `grid-flex-nesting` passes,
  but the test's 2 MiB claim does not hold for release builds.
- Paint, images: only single images are drawn into the pixel-snapped
  rectangle. A tiled image, and a `no-repeat` background whose tile is
  smaller than its area, use the unsnapped tile. `background-size: cover`
  and repeated backgrounds differ more: Chromium fades the image edge to
  transparent, swb pads it. An SVG image is rendered at the unsnapped
  size and then drawn into the snapped rectangle (resampled by up to
  1 px).
- Text: after the 1/16 px edges, glyph ink still differs by 1–3 % from
  FreeType's coverage.
- Tools: the Ars full-page score varies between 0.9558 and 0.9966 for
  byte-identical swb output, because Chromium's card placeholder images
  differ between runs. Find out why (it can be the cause of the avatar
  item above).

## Backlog from M4

- Web fonts (ADR 0022), beyond part 2: `font-display` block and swap
  timers (all values act as `swap` with an infinite swap period); the
  angle of `oblique <angle>` in font matching and for the `slnt` axis
  (Chromium 148 picks an italic face over an `oblique 20deg` face for
  `font-style: oblique`); collections with a `#PostScriptName` fragment
  (face 0 is used); loading the first available font for the `ch` and
  `ex` units, as Chromium does.
- Web font face entries and their font instances accumulate across
  navigations in one tab: `FontContext` frees the font data of a face
  that goes away, but keeps its entry. Reuse or remove the entries. The
  table of `font-variation-settings` lists (`MAX_VARIATION_SETS`, 4,096,
  `text/src/context.rs`) is not cleared by `reset_web_fonts` either, so
  after many navigations in one tab later pages lose their variation
  settings (a warning, then no variations), although ADR 0022 calls the
  limit per document (M4 maintenance review).
- Web font sources cross from the engine to `text` as strings
  (`"local(NAME)"` is encoded and parsed back in `text/src/web.rs`);
  give the text API a typed source (M4 maintenance review).
- `ch` units: swb uses 0.5em; Chromium measures the `0` of the first
  available font (`tools/probes/web-fonts.json`,
  `ch-unit-first-available`; not specific to web fonts). The same holds
  for `ex` (the x-height of the first available font; with
  `size-adjust` the adjusted one: 10ex is 250 px for `size-adjust: 50%`
  on a 100 px font with x-height 0.5em, swb gives 500 px).
- `letter-spacing` other than 0 turns off optional ligatures in
  Chromium (`liga` and the like; `'liga' 1` in `font-feature-settings`
  turns them on again): `tools/probes/web-fonts-2.json` case
  `ffs-shaping` measured 602.02 px against swb's 596.5 px (with
  `letter-spacing: 1px` on DejaVu Sans, "fi fl ffi AV To" at 100 px).
- `font-optical-sizing: auto`: Chromium sets the `opsz` axis from the
  font size; swb does not set `opsz` (neither target page has a font
  with that axis).
- `local()`: named instances of variable fonts (`Source Sans 3 Bold`)
  are not matched, only faces that fontconfig lists with index 0 of
  their file; the matching reads the first full name that fontconfig
  has, not the localized ones.
- `FontContext::font_info` reports the `wght` of the matching, not the
  value that `font-variation-settings` sets.
- `width: max-content` on block boxes is not supported (the probe cases
  use floats instead).
- Inline SVG beyond parts 1 and 2 (ADR 0023): the geometry properties
  in CSS (`r`, `cx`, `cy`, `x`, `y`, `width`, `height`, `rx`, `ry`, `d`;
  Chromium lets `r: 40px` override the attribute), nested `svg` and
  `symbol` (also as a `use` target: Chromium gives a `use` of a `symbol`
  with `width` and `height` the viewport of the symbol, and its content
  clips to it), gradients and patterns (`url()` paints use their
  fallback), `text`, `image`, `foreignObject`, `switch`, markers, `mask`
  elements, `paint-order`, `vector-effect`, `context-fill`, the
  `miter-clip` and `arcs` joins. The boxes of `text`, nested `svg`,
  `symbol`, `foreignObject`, `image` and `switch` are missing in the box
  dump (probe cases `box-other`, `box-switch-text-nested` and `box-use`:
  Chromium reports `text` with its glyph box, `foreignObject` and `image`
  with their x, y, width and height, `switch` as a group, and nested
  `svg` and the use of a `symbol` with the box of their content).
- `clip-path` beyond ADR 0023: the basic shapes (`circle()`, `inset()`,
  `polygon()`, `path()`) on SVG elements (Chromium 148 clips an SVG shape
  to `circle(30px at 50px 50px)`; probe case `clip-refs`) and on HTML
  boxes, and `clip-path: url(#id)` on HTML elements, which Chromium
  resolves against an inline `clipPath` (case `clip-other-svg`, the
  `div`). A clip rectangle of an SVG clip path is snapped to device
  pixels, not anti-aliased (case `clip-frac`).
- SVG links are not in the tab order (an `a` with `href` in SVG is
  focusable in Chromium); the HTML `pointer-events: none` is not applied
  to HTML boxes (swb parses it for shapes only).
- Raster cost of dense paths (ADR 0023, part 3), beyond items 7 and 8.
  The model charges at least the measured time for all measured families,
  but slanted edges whose spans change between the sample rows of item 8
  are estimated, not bounded. It can reject a path that draws within the
  budget:
  - Item 8 cannot see rounding and sub-pixel position: diagonal hatching
    of 1,000 thin filled lines (106 ms) and the area under a chart of
    20,000 points (0.6 s) are rejected; paths of many small dots or
    rectangles are charged 1.7 times more than after item 7. SVG images
    count spans without a pixel grid: `svg-many-dense-paths` renders at
    33,000 instead of 120,000 px. An SVG image with a stroke of more than
    100,000 dash array entries is rejected (inline, such a stroke is drawn
    solid).
  - Decoding an SVG image of very many segments (parse, sweep, bound)
    costs about 40 ms per 250,000 segments outside the counting budget,
    with no limit per document: 100 such images take about 4 s. The
    counting budget goes to the images in load order.
  - Charged too much (false positives): the weights are rounded up to the
    slowest case of each kind, so a path is typically charged 1.5 to 3
    times its time, and up to 10 times for some kinds: 20,000 horizontal
    hairlines of 1,200 px (428 ms real, charged 4.8 billion units, budget
    2 billion) are rejected, because vertical and curved hairlines cost 60 ns per
    pixel and a horizontal one 18 ns.
  - An SVG image counts every stroke as an outline, because the hairline
    case depends on the rendering size: a noisy chart of 40,000 points with
    a thin stroke in an `<img>` is rejected (20,000 points get a lower
    resolution), while inline it draws in 0.4 s.
  - Constants come from one machine and tiny-skia 0.12.0; measure again
    after an update. The measuring programs of items 7 and 8 are not in
    the repository: add one to `tools/` (`CLOCK_THREAD_CPUTIME_ID`, the
    counts and work of `edges.rs` and `spans.rs` next to the time).
    `just hostile` still has
    `inline-svg-opacity-layers` at 2 s (limit 5 s).
  - Hit testing has a work budget per call (40 ms), but no bound per frame:
    cache the last result per (point, display list) if pointer events over a
    dense clip path become a problem.
- Ars Technica after item 9: the word "LIST" in the view selector is
  0.36 px narrower than in Chromium (45.22 vs 45.58 px; check advance
  rounding of uppercase text in the variable web font), so the next
  buttons are drawn at another sub-pixel position. Inline SVG: Chromium
  may also snap the size, not only the origin (not measured). The probe
  tool has no hover step, so `:hover` is tested only in engine tests.
  When a frame runs out of the image reduction budget (ADR 0024), some
  images stay coarse until the next repaint; the engine does not
  schedule one.
- Inline SVG anti-aliasing: tiny-skia's coverage differs from
  Chromium's on thin curved shapes (the Ars ring icon has 7 % more ink in
  `tools/probes/inline-svg.json`, case `ars-icon`), and a group opacity
  of 0.5 composites at 128/255 where Chromium uses 129/255 (also for CSS
  `opacity`).
- Paths (inline SVG, polygons) whose device bounds exceed the 32-bit
  range are not drawn: tiny-skia rejects them. Clip such paths to the
  visible area before rasterizing, if a page needs it.
- Opacity layers in HTML: each `opacity` below 1 makes a layer of the
  size of the box's paint bounds, with no limit on their number or total
  area (2,000 absolutely positioned 1200x800 divs with `opacity: .5`
  take 7.6 s). Inline SVG limits its layers (256 per document, ADR 0023);
  HTML needs a limit on the layer area per frame, with a fallback (no
  layer, or the opacity on each child) for the rest.
- Counters: `renders_children` (`style/src/element_kinds.rs`) is still
  true for `svg`, so counter properties of SVG descendants count,
  although the descendants have no boxes; not measured in Chromium.
- Fixtures: the wikipedia-web-browser fixture (pushed before the
  substitution rule) holds Wikimedia images under several free licenses
  without a list of their authors and licenses; list them in
  `THIRD_PARTY_NOTICES.md` or run `just substitute` on the fixture.

## Backlog from M3

- Masks: SVG `<mask>` references (they hide the box, as a missing target
  does in Chromium); radial and conic gradients as masks (shown
  unmasked); `-webkit-mask-box-image`; `mask-clip: text` (painted as
  `border-box`); masks of inline boxes split over lines (each fragment
  uses its own positioning area); CORS mode for mask images;
  `mask-repeat: space | round` (painted as `repeat`).
- Grid: baseline shims in track sizing, `last baseline`, excluding items
  whose size depends on intrinsic tracks from baseline alignment; §12.1
  steps 3 and 4 (a second column pass for items whose width depends on
  their height); `fit-content(<length>)` item sizes; inheritance of
  `justify-items: legacy`; `safe` and `unsafe` (parsed, ignored);
  absolutely positioned items placed by grid lines; fragmentation;
  subgrid; masonry. The intrinsic pass resolves a percentage `height`
  against an indefinite size, so `repeat(auto-fill)` rows can get another
  count there.
- Paint: `z-index` on non-positioned flex and grid items does not create
  a stacking context.
- Layout: block layout ignores `width: min-content | max-content |
  fit-content` outside flex and grid items.
- Layout, `aspect-ratio` on boxes that are not replaced: flex and grid
  containers do not grow to the height of their content (`min-height:
  auto`, CSS Sizing 4 §5.1.1); `display: table` ignores the ratio; grid
  items that stretch in the block axis do not pass the stretched height as
  a transferred width contribution to the column sizing (Chromium sizes
  `1fr 2fr` columns with ratio items differently). `vertical-align: top`
  and `bottom` on atomic inlines act as `baseline` (known failure
  `vertical-align-top-bottom`); the M5 review also saw it for a button
  next to a 60 px button or `inline-flex` box (Chromium y = 0, swb 20 or
  40).
- Scrolling: wheel-gesture latching; `overscroll-behavior`, smooth
  scrolling, snapping, `scroll-padding` and `scroll-margin`; classic
  scrollbars that take space, `scrollbar-gutter`, dragging the
  indicators; `background-attachment: local`; table cells as scroll
  containers; keyboard-focusable scrollers; element offsets in history;
  autoscroll while drag-selecting; right-to-left, vertical writing modes
  and the start-edge overflow of reverse flex containers; scrolling of
  pseudo-elements. Absolutely positioned boxes inside a nested clipping
  box, whose containing block is outside it, do not extend the outer
  scroll range (the walk stops at clipping boxes to stay linear).
  `page.navigate` to a fragment of the current URL loads the document
  again (Chromium navigates within the document).
- Text: `white-space: break-spaces` does not wrap; with `text-align:
  right | center`, trailing `pre-wrap` spaces that fit are left out of
  the line width.
- Floats: `shape-outside`; right-to-left; fragmentation. Collapsed
  spaces lose their soft wrap opportunity before a float (only an empty
  span's 0×0 box differs). A float inside an inline box paints its
  positioned descendants before that box (Chromium: after). An abspos
  box after a float, on an empty line of a block whose position is not
  known yet, ignores the float. Floats inside a positioned inline box
  with `z-index` paint in the container's float phase. Limits: more
  than 10,000 floats in one BFC, or a spent float work budget, put
  floats and boxes below all floats.
- Paint: non-positioned opacity groups paint in the inline content
  phase; outlines paint after each box, not in a last phase; the image
  of a block-level replaced element paints with its background.
- Memory: inline layout of short words takes about 230 bytes per byte
  of text (1 MiB of `y z y z ...` in one block: about 250 MB, 4 MiB:
  940 MB; hostile case `content-text-bomb-block`).
- Text: the hyphen glyph at a break after U+00AD; a break before a space
  that follows a wrapping box inside nowrap text; `break-all` after a
  hyphen at the start of a line; Thai and Lao dictionary breaks; no bidi
  reordering and no `direction: rtl` alignment; no reshaping at line
  edges inside a text item (a kerning pair or Arabic joining across a
  `break-all` or `overflow-wrap` break stays); the space in a
  fallback-font run takes the fallback font (0.64 px too wide); clusters
  that no font covers fully; floats inside a word cut by
  `overflow-wrap`; letter-spacing is applied to cursive scripts, and
  optional ligatures stay on with letter-spacing; `break-spaces` breaks
  after the space run, not after every space; multi-column layout (only
  the block formatting context of multicol containers exists).
- Text: with a fixed `line-height`, a fallback font adds its own
  half-leading (Chromium: the line keeps the fixed height); item widths
  are rounded from the exact sum (Chromium converts the sum to `f32`
  first; 41 of 15,001 sizes differ for a 200-glyph text); Chromium shares
  one font between nearby sizes on a page (font cache key truncated to
  1/100 px twice; not modelled); a block's `bidi-override` does not reach
  the inline boxes inside it.
- Layout box dump: an inline element without decorations reports its
  own line box, Chromium the union of its children (culled inlines;
  Wikipedia navbox lists, 8 boxes).
- Counters: `contain: style` (counter scopes and list owners); markers
  of `::before`/`::after` with `display: list-item` (they take numbers
  but are not drawn); `@counter-style` and the other predefined styles
  (`armenian` and others fall back to decimal); inside symbol markers are
  narrower than in Chromium; layout renders the children of `progress`,
  `meter` and of `option` outside a `select` (Chromium does not; their
  counters already have no text); Chromium generates `::before`/`::after`
  for `canvas`, `object` and list-box `select`, and not for `svg` and
  `math`; Chromium numbers a failed `img` with `alt` and `input
  type=image` with `display: list-item` (an empty marker).
- Responsive images: `sizes="auto"` on a `<source>` gives 100vw, and for
  images that are not lazy-loaded `auto` gives 100vw as in Chromium (the
  specification skips it); Chromium's
  preference for a denser candidate in its memory cache; AVIF sources
  are skipped (no decoder); `ex` and `ch` in `sizes` are 0.5em;
  superseded image loads are not cancelled, and each selection walks the
  whole document.
- Containment: only size containment of replaced elements has an effect
  (`img` with `sizes="auto"`, `contain-intrinsic-size`; a flex item with
  size containment has no automatic minimum size). Size containment of
  non-replaced boxes (intrinsic size from `contain-intrinsic-size`, 0 for
  `none`; the box ignores its content for `auto` sizes), `inline-size`
  containment, layout, paint and style containment, and the `auto` form
  of `contain-intrinsic-*` (the remembered size of `content-visibility:
  auto`) are not implemented. Ars uses `contain: layout style size` only
  on a hidden `.pswp`.
- Lazy loading: `loading=lazy` images load at once (no deferral until near
  the viewport).
- Selectors: `::slotted()` is invalid in swb; Chromium accepts it. `:host`,
  `:host()` and `:host-context()` parse and never match (no shadow trees).
- Media: the `aspect-ratio` hint of `width`/`height` on `<img>` and
  image buttons; controls: the overlay play button, the loading spinner,
  interaction, hover and focus states; the first video frame for
  `preload` (swb shows only posters); audio controls measured only at
  300×54; the poster clip ignores `overflow: visible`.
- Positioning: an abspos box directly before inline content is
  block-level in the box tree (aligned as in block flow); abspos
  children of table rows get the static position of an anonymous cell;
  `unsafe` alignment is ignored; block-level `justify-self` and
  `justify-items` for in-flow blocks (Chromium aligns them, which moves
  static positions); swb accepts `fit-content(<length>)` for `width` and
  `height` (Chromium rejects it); opacity layers have no work budget
  (many large overlapping opacity groups are slow); `hit_test`
  allocates the transform ends on each hit test that skips a group;
  grid areas as containing blocks (abspos children of a grid with
  `grid-row`/`grid-column`); abspos boxes inside floats inside
  positioned inline boxes use the next containing block; text selection
  in transformed, stuck sticky and scrolled fixed boxes uses layout
  positions; rotated, skewed and scaled text is a resampled layer (glyph
  outlines would keep it sharp); non-positioned boxes with opacity < 1
  paint in normal-flow order, not with the positioned boxes at z-index
  0; `z-index` on static flex items is ignored; 3D transforms,
  `perspective`, the `translate`/`rotate`/`scale` properties,
  `transform-box`; `will-change`, `filter` and `contain` as containing
  blocks for fixed boxes; `clip-path`.
- Flexbox: baseline alignment (`align-self: baseline` aligns at the
  cross start); `flex-wrap: wrap-reverse` is laid out as `wrap`; auto
  margins on the cross axis; `justify-content: normal` packs
  `row-reverse`/`column-reverse` items at the main end (Chromium: main
  start); in wrapping column containers, items with auto width get the
  container width instead of fit-content (§9.4 step 7).
- Block layout: percentage heights resolve against the unclamped
  specified height (`height: 300px; max-height: 100px`: a `50%` child is
  150 px, Chromium 50 px).
- Style: `background-position-x/-y` accept `x-start`, `x-end`,
  `y-start` and `y-end` (Chromium rejects them; affects `@supports`
  only); `background` with thousands of layers computes every layer on
  every element (masks keep only 32). `tab-size`, `pointer-events`,
  `background-attachment`, `text-overflow` and `text-decoration-style`
  are parsed and computed, but layout, paint and hit testing do not use
  them.
- CSS: media query and `sizes` lengths do not accept the units `svmin`,
  `lvmin`, `dvmin`, `svmax`, `lvmax`, `dvmax`, `vi`, `vb`, `lh`, `rlh`,
  `cap` and `ic` (style accepts them), so `@media (min-width: 10dvmin)`
  does not parse (found by the M3 maintenance review; not compared with
  Chromium).

## Backlog from the M2 maintenance review

Bugs found by the end-of-milestone review. The maintenance commit did not
change behaviour, so they are not fixed yet.

- Style: `:required` and `:optional` match `range` and `color` inputs
  (the `required` attribute does not apply to them); `background: ...
  text` sets `background-origin` to `border-box` (`text` sets only
  `background-clip`); ignored properties (`text-underline-offset`,
  `text-decoration-thickness`, ...) accept any value, so `@supports` with
  an invalid value for them is true.
- Paint: with `background-clip: padding-box`, the background color uses
  the outer border radii instead of the inner ones.
- Forms: `readonly` blocks editing of range and color fields (shown as
  text fields); it does not apply to them.
- Selection: keyboard focus on a text field does not clear the page
  selection (both are highlighted).

## Backlog from M2

- Tables: `visibility: collapse`; fragmentation (repeated headers); the
  quirky margins of the first and last children of cells and of the body;
  the body and html fill-viewport quirks; collapsed-border joints
  (horizontal edges always cover the joint; Chromium decides per joint);
  column percentages are ignored only inside cells and flex containers,
  not inside grid items (not compared with Chromium); inset/outset
  border shading; `width:
  min-content` keywords on inline-blocks. Limits: 10,000 columns;
  collapsed grids over 2,000,000 edges (`MAX_EDGES`, all tables of a
  layout pass together) use each box's own borders and paint none;
  inline wrapper boxes only for the 8 innermost inline boxes.
- Cookies: persistence; `Partitioned`, `Priority`, `__Http-` prefixes,
  Lax+POST; the redirect-tainted `Origin`; `Sec-Fetch-*` and `Referer`
  headers; raw bytes in cookie values (now re-encoded as UTF-8);
  Chromium's 30-day protection in global eviction.
- Forms: list boxes (`<select multiple>` or `size` > 1 look like
  drop-downs) and the popup list of a select (the keyboard changes the
  selection); date, time, color, range and file inputs (text fields or a
  button); image buttons show their `alt` text; the legend of a fieldset
  sits inside the border; `wrap=hard`; validation other than `required`
  and its messages; IME composition; middle-click paste into a field;
  mouse-wheel scrolling of a text area; ArrowUp/Down in a text area move
  by logical lines; `dir=auto` for `dirname`; every keystroke lays out
  the page again, and select option labels are measured again on every
  layout (cache their widths); the native look is decided from computed
  values (an author value equal to the default keeps the native look).
- SVG images: `<text>` is not drawn (no fonts in resvg); non-UTF-8
  sources are rejected; `list-style-image` is not drawn (also for raster
  images); percentages inside an SVG without `viewBox` and absolute size
  resolve against 300×150; `ex` is half an `em`; decoding and rendering
  run on the page thread (up to about 0.6 s per rendering and 1.2 s per
  frame for hostile files); whole tiles are rendered, not only the
  visible part. On a hostile page, images beyond the per-frame budget
  stay blank or blurred until a repaint. The limits mirror resvg, usvg,
  roxmltree, svgtypes and simplecss as pinned in `Cargo.toml` (kurbo
  through `Cargo.lock`): an update of any of them needs the same
  adversarial review.

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
- Automation: events (`page.loaded`, `page.navigated`); cancel a pending
  `page.waitForLoad` when its client disconnects.
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
- Engine: a click on a visible child of a `visibility: hidden` focusable
  element focuses the hidden element; after a navigation
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

- Layout: `vertical-align: top/bottom`; min-content of nowrap row flex
  containers; tab stops; inside markers in line
  breaking (and `text-indent`); column flex items with a definite height
  cannot shrink; empty lines ignore `text-align` and relative offsets;
  percentage `top`/`bottom` on inline boxes; `capitalize` across element
  boundaries inside words (`don<b>'t</b>`).
- Paint: `border-radius` does not clip background images, gradients or
  replaced images (overflow clips follow it since M4 item 9);
  `content-box` background clip/origin; gradients ignore background size
  and position, repeating gradients and implicit stop positions.
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

## Later

- `@import` (the parser supports the rule; nothing loads it).
  `@font-face` is in M4.
- Find in page, tabs, bookmarks.
- Bidirectional text.
- Incremental style and layout; GPU rasterization if needed.
- JavaScript: see M6.
- An optional ad and tracker blocker (a possible target, ground rules
  2026-10-09). It works with lists of ad and tracker URLs, not by
  origin. Check the licenses of the lists before swb ships or downloads
  any (ADR 0003).
