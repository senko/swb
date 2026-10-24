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

## M7: JavaScript language core — done

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
     without the run-time checks of B.3.2.2 (done in 3f).
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
   `switch`, unmapped `arguments` and `this` in both modes work since
   the spike and feature 1c; the mapped `arguments` object of 1c has
   no aliasing yet (3f). Measured before the split (2026-10-14, the test262
   counts are tests, both modes as one): in `test/language/` (all
   directories), the compiler rejects about 1,920 tests at `class`,
   1,710 at destructuring, 560 at `for`-`of`, 375 at `for`-`in`, 315 at
   destructuring assignment, 150 at default values, 115 at `with`, 90
   at spread, 50 at accessors in literals, 50 at object spread, 45 at
   `new Function`, 35 at tagged templates, 15 at optional chaining and
   10 at `new.target`; 3,000 more tests skip on the class features;
   370 fail because `eval` is not defined. In the built-ins groups of
   the subset, about 1,420 tests stop at `for`-`in` and 105 at
   `new Function`. `yield*` (43 tests) belongs to feature 7. Six
   sessions (five in the first plan), in this order; each removes its constructs from
   `compiler/support.rs`, adds its test262 features to the subset and
   updates the scores:
   - 3a. `for`-`in` and the object literal: `for`-`in` with
     EnumerateObjectProperties (§14.7.5.9; the order of
     `for-in-order`, deleted and shadowed keys, keys added during the
     loop, prototype chains, String objects, a `for`-`in` over
     `null` and `undefined`), with an identifier or member target;
     getters and setters in object literals; object spread
     (CopyDataProperties, §7.3.25); the run-time names of methods and
     accessors with computed keys (SetFunctionName with symbols and
     the `get `/`set ` prefixes); optional chaining (calls keep
     `this`; `delete` of a chain); tagged templates (the template
     objects of §13.2.8.4, one per site and realm, frozen, with
     `raw`); `new.target` in functions and arrows. A pattern as the
     target of `for`-`in` stays rejected until 3b. Exit: the built-ins
     groups run `propertyHelper.js` without the patched copy. Done
     2026-10-14: all of the above. Where §14.7.5.9 leaves a choice,
     `for`-`in` does what V8 does (measured in Node 22): it collects
     the keys of the object and its prototypes at the start, skips a
     key that `[[HasProperty]]` no longer finds, and does not visit
     keys added during the loop. The key list is an internal heap
     object, reserved before it is built and counted in the heap; each
     key and each prototype walked costs a time step. Template objects
     are cached in the code object per site and realm. Source positions
     and callee texts of chains, tagged templates and spread follow
     Chromium; an assignment to a call gives V8's message for its
     context (assignment, prefix, postfix, `for` loop). Runtime
     test262: 10,065 pass (was 8,429): the built-ins groups 1,341 more
     (1,289 of the 1,428 tests that stopped at `for`-`in`; the other
     139 need `Array.prototype` methods, `Date`, `RegExp`, `JSON`,
     `String.prototype` methods or `new Function`), `test/language/`
     295 more, two new groups (`expressions/new.target`,
     `expressions/tagged-template`). Parse-only unchanged. 16 of the 84
     BBC and Ars scripts compile; the others stop at destructuring (38),
     regular expression literals (9), default values (9), spread (4),
     `async` (4), classes (2) and `for`-`of` (2). Known failures: tests
     that use `eval` or `new Function` (3e), `String.prototype` methods
     (feature 5). Open: destructuring targets of `for`-`in` (3b); a
     `for`-`in` with a key on each level of a long prototype chain takes
     quadratic time, as in V8 (the time limit holds); 5,000 nested
     `for`-`in` loops, `?.[0]` links or chained templates exceed the
     compiler's recursion budget (`RangeError`; Node runs them);
     `for`-`in` and spread on small objects are about 10x slower than
     `node --jitless` (an enum cache keyed by shape and a clone path for
     `{ ...o }`, feature 13).
   - 3b. Iteration and destructuring: `for`-`of` with iterator closing
     on `break`, `return`, `throw` and `continue` to an outer label
     (§7.4.11, ADR 0026 section 5); spread in array literals, calls
     and `new`; destructuring in declarations, parameters, `catch`
     and the heads of `for`-`in` and `for`-`of`; destructuring
     assignment with any target; default values (with the separate
     scope of parameter expressions and the TDZ of later parameters);
     array rest and object rest (CopyDataProperties with excluded
     keys). The thrown values that the closing holds go through
     `hold_error` (ADR 0026 section 15). Also: a non-simple parameter
     list takes one register per parameter, not two (the open item of
     1a); a fast path for `for`-`of` over arrays with the built-in
     iterator, if a measurement shows the need. Done 2026-10-14: all of
     the above. `for`-`of` and array patterns close their iterator in an
     implicit `finally` block of the existing completion machinery (also
     on `continue` and `break` to outer labels and on the return of a
     suspended generator); a failure of the iterator itself does not
     close it. Iterator records live in three registers; the call of
     `next` is a frame, so iterating a generator does not recurse in
     Rust. The built-in Array Iterator steps without result objects, and
     spread and array rest copy an array without holes in bulk (not
     observable: a patched `@@iterator` or `next` is called). A
     parameter that is a whole element (`a`, `a = 1`, `...a`) keeps its
     argument's register: 60,000 parameters with defaults work, 100,000
     give V8's `SyntaxError`. V8's messages for destructuring and
     iteration, Chromium's positions. Runtime test262: 12,958 pass (was
     10,065), the same in stress mode; new groups
     `arguments-object/unmapped`, `destructuring/binding`,
     `rest-parameters`. Parse-only unchanged. 38 of the 84 BBC and Ars
     scripts compile (were 16); the others stop at `async` (20),
     regular expression literals (15) and classes (11). Speed against
     `node --jitless`: `for`-`of` over 1M elements 62 ms (23 ms),
     `[...a]` of 1M elements 3 ms (1.4 ms), `f(...a)` of 100,000
     elements 0.6 ms (0.06 ms), `var [p, q] = [i, 1]` about 6x (a fast
     `GetIterator` for arrays, feature 13). Known failures: iteration
     of strings (feature 5), mapped `arguments` aliasing (3e), `eval`
     in parameter expressions (3e), a closure in a `catch` parameter
     default sees the block's `let` (backlog). Open: positions of an
     error from `return()` on `break` or `return`, and of array
     patterns in parameters and loop heads, differ from Chromium; a
     parameter with a pattern still takes two registers (33,000 such
     parameters fail; V8 runs them); some "not iterable" messages give
     the type (`object is not iterable`) where V8 renders the source
     (`{} is not iterable`).
   - 3c. Classes without private names: declarations and expressions
     (the inner name binding, the TDZ of the heritage), constructors
     and the default constructors, derived constructors with
     `super(...)` and the `this` TDZ, `super` properties in classes
     and in object literal methods (home objects), methods, getters,
     setters and static members with computed keys, public instance
     and static fields (the order of initialization, computed keys
     evaluated once), static blocks, the class-specific `TypeError`s
     (calling a class, `super` twice, a heritage that is not a
     constructor). `Function.prototype.toString` uses the class span.
     Runner: the features `class`, `class-fields-public`,
     `class-static-fields-public`, `class-static-block`, `super`.
     Done 2026-10-15: all of the above. A class definition compiles to
     one instruction per step of §15.7.14 in the enclosing function;
     fields are defined by the two initializer functions of the parser,
     and computed field keys are evaluated at the definition into an
     internal array. Closures of methods, class constructors and
     initializers carry class slots (home object, `[[Fields]]`, the key
     array). Base constructors define their fields at their start,
     derived ones after `super(...)`, both as frames: 10,000 derived
     levels end at the frame limit with V8's `RangeError`, without Rust
     recursion. Default derived constructors pass their arguments
     without the array iterator. The heritage and computed keys of a
     class in sloppy code run as strict code (instruction ranges in the
     code object). V8's messages and stack frames (`new C`, `C.m`,
     `<instance_members_initializer>`, `<static_initializer>`);
     `Function.prototype.toString` of a class gives its source. Runtime
     test262: 15,881 pass (was 12,958), the same in stress mode; new
     groups `computed-property-names/class`, `expressions/super`;
     `statements/class` 1,396 more, `expressions/class` 1,222 more.
     Parse-only unchanged. 39 of the 84 BBC and Ars scripts compile
     (were 38); no script stops at a class now, the others stop at
     `async` (27) and regular expression literals (18). Speed: `new C()`
     with three fields 396 ms per million (`node --jitless` 62 ms), a
     method call 128 ms (43 ms), the same as the equivalent function
     constructors (object creation, feature 13). Known failures: tests
     that use `eval` or `new Function` (3e), private names (3d), `async`
     (feature 7), missing built-ins. Open: V8 reports the call of a
     static initializer and a heritage error at the position of an
     earlier statement, swb at the class and the heritage; when a
     derived constructor runs off its end, V8 reports the position of
     the last expression, swb the start of the last statement.
   - 3d. Private names: private fields, methods, getters and setters,
     instance and static, the brand checks and their `TypeError`s,
     `#x in o`, private names in nested classes and in `eval` inside a
     class (with 3e, then a known failure). Each private name no longer
     takes a register of the enclosing function (the open item of 1b):
     the limit of about 64,500 per class goes away or becomes a
     `SyntaxError` with V8's message. Runner: the private class
     features. Done 2026-10-15: all of the above except private names in
     `eval` code (3e). A class body with private names keeps its private
     environment (an internal object made when the body scope is
     entered) in one binding; private names are indices into it, so a
     class takes one register whatever the number of its names (100,000
     work; V8 has no limit either). Private Names are private symbols;
     private fields are shape entries under them (shared shapes, traced
     and counted; every object kind, also frozen and non-extensible
     objects), and `[[OwnPropertyKeys]]` leaves them out. Private
     methods and accessors live in the environment: an instance gets one
     brand entry before its fields, and static ones are checked by the
     constructor's identity, as in V8. V8's messages, frames and
     positions. Runtime test262: 18,550 pass (was 15,881), the same in
     stress mode; `statements/class` 1,263 more, `expressions/class`
     1,194 more. Parse-only unchanged. 39 of the 84 BBC and Ars scripts
     compile; none uses private names. Speed: a private field read costs
     about 2.3x a public read net of the loop (`node --jitless`: 2x); a
     private method call costs about the same as a public one. Known
     failures: private names in `eval` code and the class tests that use
     `eval` (3e), `Proxy` (feature 9).
   - 3e. `eval` and `with`: direct and indirect `eval` (PerformEval,
     EvalDeclarationInstantiation with its conflict checks, var
     declarations of sloppy eval code in the caller's variable
     environment, strict eval code in its own), the dynamic
     environments of the scope analysis at run time (`with` object
     environments with `@@unscopables`, the var scopes of sloppy
     functions with a direct `eval`), the run-time checks of Annex
     B.3.2.3 in eval code, private names in eval code inside a class
     (from 3d), `eval` in parameter expressions (from 3b). The global
     `eval` comes in this session. Done 2026-10-15: all of the above.
     PerformEval with the early errors of the call site,
     EvalDeclarationInstantiation in Rust with its conflict checks and
     Annex B.3.2.3 and B.3.4, the vars of sloppy eval code in the
     caller's variable environment, `with` with `@@unscopables`, the
     dynamic references of `with` bodies and of functions with a sloppy
     direct `eval`, the caller's `this`, `new.target`, `super`,
     `arguments` and private names in eval code, exact completion
     values, V8's stack frames of eval code. A function with a direct
     `eval` keeps a scope description; eval code captures the caller's
     cells like a closure and runs as a frame (eval recursion ends at
     the frame limit; compiled eval code is garbage after the call).
     Code without `eval` or `with` compiles as before and runs as fast.
     Runtime test262: 20,296 pass (was 18,550), the same in stress
     mode; new groups `eval-code/direct`, `eval-code/indirect`,
     `annexB/language/eval-code`, `annexB/language/function-code`,
     `annexB/language/global-code`; `statements/with` 120 more.
     Parse-only unchanged. 39 of 84 target scripts compile (unchanged);
     3 use direct `eval`, 7 the `Function` constructor, and `with`
     occurs only in `new Function` bodies. Known failures:
     `statements/with` needs `parseInt` (feature 4); tests that use the
     `Function` constructor (3f) or `async` (feature 7). Deviations from
     V8 that follow the spec: an assignment or update in a `with` body
     finds its record before the right side runs; `eval('var e')` in
     `catch ({ e })` is allowed. Open: a caller's cell that a function
     of eval code uses is captured by every function around it, and a
     function that calls `eval` gets every name of the caller: a compile
     past 2^20 such captures throws a `RangeError` (V8 has no limit),
     below it a compile takes up to 0.6 s; the parse of eval code checks
     8 bytes per code unit against the heap limit before the tree
     exists, so dense code can allocate a few times that before the
     exact check.
   - 3f. `new Function`, Annex B.3.2.2 and mapped `arguments`: the
     `Function` constructor (CreateDynamicFunction for normal
     functions; the generator and `async` forms come with feature 7;
     until 3f it throws "not supported yet"), the run-time checks of
     Annex B.3.2.2 for global block functions, the aliasing of the
     mapped `arguments` object and the parameters (§10.4.4.7, also
     through `eval`). 3e was split in two on 2026-10-15: the remaining
     work did not fit one session. Done 2026-10-15: all of the above.
     The `Function` constructor builds V8's source `(function
     anonymous(P\n) {\nbody\n})` and parses it once; position checks
     do the work of the spec's separate parses of parameters and body,
     with V8's messages ("Arg string terminates parameters early",
     "Unexpected end of arg string", "Single function literal
     required"); `anonymous` is not bound; the parse and compile are
     charged like eval code; the function owns its source for the heap
     limit; the prototype comes from `new.target`. V8's stack frames
     (`at eval (eval at <anonymous> (file:1:9), <anonymous>:3:7)`).
     Mapped arguments objects keep the parameter's cell in each mapped
     element, so the map costs no memory; reads, writes, `delete` and
     `defineProperty` follow §10.4.4, also through `eval` and closures.
     Annex B.3.2.2: the var of a global block function is created only
     when no global lexical declaration has the name and
     `CanDeclareGlobalVar` allows it, never with an error, and the
     evaluation of the declaration checks again. Runtime test262:
     20,639 pass (was 20,296), the same in stress mode;
     `built-ins/Function` 127 more, the Annex B groups of eval and
     global code 88 more, `arguments-object/mapped` 21 more; new groups
     `annexB/built-ins/Function`, `expressions/this`. Parse-only
     unchanged. 39 of 84 target scripts compile (unchanged); the 7 that
     use the `Function` constructor stop earlier at regular expression
     literals (5) or `async` (2), and their uses of it run as in Node.
     Code without `arguments` runs as fast as before; 1M calls that
     create an arguments object take 558 ms (`node --jitless` 50 ms;
     feature 13). Deviations from V8 that follow the spec and test262: a
     global `let` or `const` with a block function's name does not make
     the script fail, and the hoisted var is not configurable. Known
     failures: `GeneratorFunction` and `AsyncFunction` (feature 7),
     `nativeFunctionMatcher.js` (feature 8), `Date` (feature 12). Open:
     the parameters of a mapped function are limited by the register
     limit (about 64,500); three rejected parameter and body pairs give
     another `SyntaxError` message than V8 (`Function('/*', '*/')`:
     "Unexpected token '}'", V8 "Unexpected end of arg string"); the
     `Array.prototype` methods that write through `arguments`
     (`reverse`, `sort`, `splice`, `fill`) are untested until feature 6
     adds them.
   Feature 3 is complete (2026-10-15, six sessions 3a to 3f): the VM
   runs all syntax of feature 1 except `yield*`, `async`, `for await`,
   modules, `import()`, `import.meta`, regular expression literals and
   `BigInt` (features 7, 8, 10 and 14). Runtime test262 went from 8,429
   to 20,639 passes; the target scripts that compile went from 3 of 84
   (feature 1 parsed all of them) to 39; the others stop at `async`
   (27) and regular expression literals (18).
   Exit of feature 3: no construct of feature 1 except generators,
   `async`, modules, regular expressions and `BigInt` is rejected; each
   session records its known failures with the feature that removes
   them.
4. Numbers (`implementer-hard` for the exact conversions, ADR 0026
   section 7). `Number`, `Math`, `parseInt`, `parseFloat`,
   StringToNumber, number-to-string in all radixes, `toFixed`,
   `toExponential`, `toPrecision`. Done 2026-10-15 in one session:
   `Number` with its value properties and static functions
   (`Number.parseFloat` and `Number.parseInt` are the global
   functions); `toFixed`, `toExponential` and `toPrecision` exact on
   multi-precision integers (a tie picks the larger value), with V8's
   messages; `toString(radix)` with the radix 10 rule of §6.1.6.1.20
   applied to the radix (the shortest digits that read back as the
   number, the closest of them; Burger and Dybvig's algorithm in exact
   arithmetic); `toLocaleString` as
   Chromium's en-US default, arguments ignored (no ECMA-402);
   `parseInt` (exact in all radixes), `parseFloat` and an exact
   StringToNumber in one bounded pass (a 16-million-digit string needs
   less than a kilobyte); all of
   `Math` with `f16round` and `sumPrecise` (both in Chromium 148) and
   `random` (xorshift128+, seeded per runtime). The number code is in
   `js-text` (`number/`). Runtime test262: 21,621 pass (was 20,639),
   the same in stress mode; new groups `built-ins/Number`, `Math`,
   `parseInt`, `parseFloat`, `NaN`, `Infinity`, `undefined`;
   `statements/with` 33 more. Parse-only unchanged. Known failures:
   `String.fromCharCode` (3 tests, feature 5), `Date` (2, feature 12),
   a `BigInt` literal (1, feature 10), `Float16Array` and
   `byteConversionValues.js` (5 `f16round` tests skipped, feature 11).
   Provenance: the first version of `toString(radix)` and of `parseInt`
   in radixes that are not powers of two had the step structure of V8's
   code; the owner decided to replace both with algorithms from the
   literature (ADR 0026 section 15). Their output differs from
   Chromium's by design: V8 gives other digits for 56 % of random
   values in random radixes, and in 76 % of those its string does not
   read back as the number; V8's `parseInt` differs for 21 % of random
   strings of 1 to 800 digits. Other deviations from V8: the
   transcendental `Math` functions use the
   platform's libm and differ in the last bit for 1 to 15 % of
   arguments (`atanh` 67 of 426); `Math.pow` and `**` differ in the
   last bit for some arguments (V8: `10 ** -5` is
   0.000009999999999999999). Speed: a call of `Math.floor` or
   `parseInt` costs about 70 ns (`node --jitless` about 10 ns), the
   cost of the call path (feature 13); `toFixed` about 290 ns.
5. Strings and JSON (`implementer`). `String` without the regular
   expression methods, ropes, the string iterator, `normalize`, case
   mapping, the URI functions, `JSON` with its depth limit. Annex B:
   `escape`, `unescape`, `substr`, the HTML methods (`anchor` and
   others), `trimLeft` and `trimRight`. With the string iterator, check
   `Math.sumPrecise` of strings (Chromium: `''` gives -0, `'ab'` throws
   "Type of 'Iterator value' must be 'number', found 'string'"; swb
   throws "[object String] is not iterable" until then).
   Split 2026-10-15 into four sessions, one commit each. Only the
   `String` constructor, `toString` and `valueOf` exist, and a loop of
   200,000 `s += 'ab'` takes 2 s (Node.js 8 ms), so ropes come first and
   the later sessions use their access API:
   - 5a Ropes (`implementer-hard`, ADR 0026 section 7, memo 4.2): a
     concatenation of long strings gives a rope; a depth limit and
     rebalancing; an operation that needs the code units flattens the
     rope and keeps the flat result in the same string; the collector
     traces the children; heap accounting; one access API for the
     existing call sites. Exit: the append loop above is linear.
     Done 2026-10-15: a concatenation of more than 1,024 code units
     makes a rope node; a short piece merges into the edge leaf of the
     other operand; at depth 256 the operand is rebalanced in place
     (its leaves joined into chunks of at least 1/64 of the length).
     An operation that needs the code units flattens the rope in place
     (iterative, heap limit checked first, time charged); `length`, the
     width, equality and order need no flatten. Keys and atoms are
     flat. 200,000 appends take 0.07 s (was 1.9 s; `node --jitless`
     0.03 s), 2 million take 0.57 s (was over 120 s), 1 million
     alternating appends and prepends 0.35 s (was 19.6 s); `s += s`
     ends with V8's `RangeError` after 28 doublings. Runtime test262
     unchanged at 21,621 (stress mode the same), parse-only 41,882.
     The review found no bugs. Open: equality and order of long
     strings charge no time, and an index read cannot collect before
     its flatten (both backlog); the `vm_bench` case "new Error and read stack" is 5 % slower
     (instructions +1 %, the rest instruction-cache misses from the
     changed code layout).
   - 5b `String` (`implementer`): `fromCharCode`, `fromCodePoint`,
     `raw`; the methods of `String.prototype` without regular
     expressions (`at`, `charAt`, `charCodeAt`, `codePointAt`, `concat`,
     `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `isWellFormed`,
     `toWellFormed`, `padStart`, `padEnd`, `repeat`, `slice`,
     `startsWith`, `substring`, `trim`, `trimStart`, `trimEnd`); `replace`,
     `replaceAll` and `split` as the specification defines them (their
     dispatch through `Symbol.replace` and `Symbol.split` is generic, so
     8b only adds the `RegExp` side; `match`, `matchAll` and `search`
     create a `RegExp` and stay for 8b); the string iterator; the URI
     functions; Annex B `substr`, the HTML methods, `trimLeft`,
     `trimRight`, `escape`, `unescape`.
     Done 2026-10-15: all of the above, with V8's messages measured in
     Chromium 148 (318 cases in `tools/probes/js-strings.json`, also a
     test). `GetSubstitution` takes captures and named captures, for
     8b. `replace`, `replaceAll` and `split` look up `Symbol.replace`
     and `Symbol.split` only on objects (test262 and Chromium 148;
     Node.js 22 also on primitives). `repeat`, `padStart` and `padEnd`
     make ropes by doubling. A needle of fewer than 16 units is found by
     a scan for its first unit, a longer one by Knuth-Morris-Pratt; all
     search, copy and URI work is charged to the time countdown.
     `Math.sumPrecise` of strings now gives Chromium's results. Runtime
     test262: 22,888 pass (was 21,621), the same in stress mode (whose
     default time limit is now 30 times longer); new groups
     `built-ins/String` 891 of 1,195, `StringIteratorPrototype` 7, the
     URI functions 161 of 173, `annexB/built-ins/String` 105, `escape`
     15, `unescape` 18. Parse-only unchanged. Known failures:
     `RegExp` and the methods that use it (8b), case mapping,
     `normalize` and `localeCompare` (5d; also 12 URI tests that call
     `toUpperCase`), `BigInt` (10), `Array.prototype.lastIndexOf` (6).
     Speed against `node --jitless`: `indexOf` 4.3 times slower,
     `slice` with `charCodeAt` 7.3 times, `split` into 8 pieces 26
     times (each piece goes through the general property definition;
     feature 6). The review found one bug: a replacement template with
     many `$<` was scanned once per `$<` without a time charge (fixed:
     one charged scan per template).
   - 5c `JSON` (`implementer`): `parse` with the reviver, `stringify`
     with replacer, indentation, `toJSON`, well-formed output, V8's
     message for cycles and the depth limit; the source text access of
     `JSON.parse` (`rawJSON`, `isRawJSON`) if Chromium 148 has it.
     Done 2026-10-15: `JSON.parse` with the reviver and the source text
     access (`context.source`, `rawJSON`, `isRawJSON`, all in Chromium
     148), `JSON.stringify` with all its arguments, V8's messages
     (the `SyntaxError` texts with their context window, the cycle
     message with the property path). The three walks are loops over a
     vector of open levels. Depth limits from Chromium: `parse` has none,
     as in V8; `stringify` ends at 100,000 levels (with a gap, a replacer
     or a property list at 4,000) and the reviver walk at 4,000, with
     V8's `RangeError`, where V8's recursion ends at 3,000 to 5,000.
     806 of the 824 measured cases (`tools/probes/js-json.json`) are a
     test; the others need `BigInt`, `Date`, `Proxy`, or show the known
     deviation: the constructor name in the cycle message comes from the
     prototype chain (V8: the function that made the object). Runtime
     test262: 23,081 pass (was 22,888), the same in stress mode;
     `built-ins/JSON` 137 of 166 (skipped: `Proxy` 18, `BigInt` 7,
     another realm 1; failing: `Array.prototype.reverse` and `shift`,
     feature 6; a regular expression literal, feature 8). Speed for a
     1.4 MB text, against `node --jitless`: `parse` 5.8 ms (3.1),
     `stringify` 5.0 ms (1.9). The review found no blocking bug; fixed:
     the parse records of the reviver are checked against the heap
     limit, `stringify` reserves by the buffer's width so that an output
     too long for a string is V8's `RangeError`, an error message no
     longer copies the whole source.
   - 5d Unicode (`implementer`): `toLowerCase`, `toUpperCase` and the
     `toLocale` forms (full mappings, `SpecialCasing` without
     conditions, final sigma; no `Intl`, so the default locale only),
     `normalize` (NFC, NFD, NFKC, NFKD), `localeCompare` (the
     specification's requirements: a total order, canonically
     equivalent strings compare equal; ICU collation as in Chromium is
     not in scope), tables from the UCD 17 by `swbtools
     js-unicode-tables`.
     Done 2026-10-16: the case functions (the Default Case Conversion
     with `Final_Sigma`; a Latin-1 string of up to 16,384 units converts
     in one pass without tables, and an unchanged string comes back as
     the same string), `normalize` (UAX #15: fully expanded
     decomposition tables, a stable sort of each run of marks, one-pass
     composition, a quick check that returns the same string),
     `localeCompare` (on the canonical decomposition: base letters by
     simple case folding, then marks, then case with lowercase first,
     then code points), tables from UCD 17.0.0 (`case_tables.rs`,
     `norm_tables.rs`). All 1,114,112 code points give Chromium 148's
     results for `toLowerCase`, `toUpperCase` and the four forms
     (Chromium 148 has Unicode 17.0); `NormalizationTest.txt` passes in
     full (20,034 lines; a sample is a test). `localeCompare` agrees
     with Chromium on 600 random pairs of 225 Latin, Greek and Cyrillic
     words, and orders 16 of the 25,200 pairs of Chromium's sorted list
     the other way (all with Ђ). Known differences: the `toLocale`
     forms ignore the locale (Chromium: `'I'.toLocaleLowerCase('tr')`
     is `'ı'`); `localeCompare` ignores `locales` and `options`, orders
     other scripts by code point, and does not ignore control
     characters. Runtime test262: 23,222 pass (was 23,081), the same in
     stress mode; `built-ins/String` 1,013 of 1,195, the rest wait for
     `RegExp` (8b), `BigInt` (10) and `Array` (6). Speed against `node
     --jitless`, 10-unit strings: `toLowerCase` 124 ns (24 ns),
     `normalize('NFC')` 142 ns (43 ns), `localeCompare` 159 ns (40 ns).
     The review found one bug: the one-pass Latin-1 path of the case
     functions had no length check and no heap reservation (fixed: only
     for short strings; longer ones take the sliced path). Also fixed:
     the string builder of 5b and 5d reserved 4 bytes per unit, so a
     result too long for a string ended at the heap limit where V8
     throws a `RangeError`; the generator now also downloads
     `NormalizationTest.txt`; `THIRD_PARTY_NOTICES.md` has the text of
     the Unicode License v3.
   Feature 5 is complete (2026-10-16): runtime test262 went from
   21,621 to 23,222.
6. Arrays and collections (`implementer`). `Array` with dense and sparse
   elements, the iterator helpers, `Map`, `Set` (with the ES2025 set
   methods).
   Split 2026-10-16 into three sessions, one commit each. Of `Array`,
   only `isArray`, `push`, `join`, `indexOf`, `slice`, `forEach`, `map`,
   `toString` and the iterator methods exist: `built-ins/Array` passes
   981 of 3,083 tests, and `Map`, `Set`, `Iterator` and their iterator
   prototypes (not in the subset yet) pass 11 of 1,263.
   - 6a `Array` (`implementer-hard`, because it sets the element access
     of the natives that later features use): `Array.from`, `Array.of`
     and the rest of `Array.prototype`, with a stable `sort`; generic
     over array-likes with a length up to 2^53 − 1; fast paths for dense
     elements; on sparse elements, the methods visit only the present
     indices where the result is the same; a bulk append to a dense
     array for natives (`split` uses it). Keys in Chromium 148's order.
     Done 2026-10-16: `Array.from` (iterables, array-likes, `this` as the
     constructor), `Array.of` and all of `Array.prototype`, with V8's
     messages and Chromium 148's key order (615 cases in
     `tools/probes/js-arrays.json`, 608 of them a test, also in stress
     mode). The element access of the natives
     (`object/element_access.rs`) has three levels: a dense array whose
     prototypes store no elements is read and changed in its vector where
     the steps cannot fail; a prototype chain of ordinary objects
     (everything but String objects and host objects; later also not
     proxies) takes the steps only at the stored indices, also at named
     keys from 2^32 − 1 to 2^53 − 2, and checks again after each
     callback; any other chain takes the steps at every index. So with
     `a = []; a.length = 2 ** 32 - 1`, `reverse`, `shift`, `unshift`,
     `splice`, `copyWithin` and `sort` take less than a millisecond (V8:
     37 s to minutes); the copying methods of such an array end at the
     heap limit. The dense builder (one reservation, no property
     definition) serves `split`, `Object.keys`, `values` and `entries`,
     the constructor and the copying methods; `split` also keeps its last
     result: 100,000 splits into 8 pieces take 20 ms (were 99 ms;
     `node --jitless` 3.8 ms, also with a cache). `sort`: binary
     insertion of runs and a bottom-up merge of an index permutation
     (Knuth §5.2.1; Sedgewick and Wayne §2.2), stable, with the values
     rooted while comparators run; 100,000 integers 15 ms (Node.js 19
     ms), with a comparator 113 ms (23 ms; each call re-enters the
     interpreter, feature 13). `flat` keeps its levels in a vector and
     ends at 9,000 levels (V8 between 8,000 and 10,000). `reverse`,
     `sort`, `splice` and `fill` write through mapped arguments (tested).
     Runtime test262: 25,091 pass (was 23,222), the same in stress mode;
     `built-ins/Array` 2,804 of 3,083 (was 966); parse-only unchanged.
     Known failures: `RegExp` (8), `Date` (12), `Proxy` (9), typed arrays
     and `ArrayBuffer` (11), `BigInt` (10), async tests and
     `Array.fromAsync` (7), another realm. Differences from Chromium:
     `concat` throws the specification's `TypeError` for a spreadable
     array-like of length 2^53 − 1 followed by an item (V8 does not), and
     the `RangeError` of the `length` write for a result above 2^32 − 1
     (V8 returns length 4294967295); `Object.freeze([1]).sort()` throws
     the specification's `TypeError` (V8 returns the array);
     `toLocaleString` passes no arguments (no `Intl`); `join` of arrays
     nested 500 deep ends with a `RangeError` (V8: 1,000 work; unchanged
     since 5d). Also changed: an invalid `length` written with `[[Set]]`
     gives Chromium 148's message ("Failed to set the 'length' property
     on 'Array': Invalid array length"); a safepoint also collects near
     the heap limit (a rope loop ended at a 13 MiB limit with 3 MB live,
     already at 8b30241). The review found no correctness bug in about
     1,200 cases of mutating callbacks and receiver states; fixed: sorted
     input cost more comparator calls than in V8 (8 elements: 13 calls,
     V8 7; now n − 1). Open: `shift` moves the whole dense vector
     (backlog).
   - 6b `Map` and `Set` (`implementer`): an ordered hash table (insertion
     order, iteration that sees additions and skips deletions, a keyed
     hash, compaction), `Map`, `Set`, their iterators, `Map.groupBy`,
     the ES2025 set methods, `Map.prototype.getOrInsert` and
     `getOrInsertComputed` (in Chromium 148). `WeakMap` and `WeakSet`
     stay for feature 9.
     Done 2026-10-16: an ordered hash table (`object/table.rs`): entries
     in insertion order, an open-addressing index with a keyed hash
     (SipHash for strings), compaction at half deleted entries that
     corrects the positions of the live iterators and native loops, and
     a list of the unfinished iterators that the collector does not
     trace and the table prunes. `Map` and `Set` with the
     specification's adder lookup, `Map.groupBy` (shared with
     `Object.groupBy`), the ES2025 set methods with `GetSetRecord`,
     `getOrInsert` and `getOrInsertComputed`, V8's messages and Chromium
     148's key order (1,499 cases in `tools/probes/js-collections.json`,
     1,490 of them a test, also in stress mode). A table stops at 2^24
     entries with V8's `RangeError`; 200,000 inserts and deletes in a
     table of 10 entries use less than 8 KiB; a million abandoned
     iterators run under a heap limit of 24 MiB. Compiled `for`-`of` and
     the natives that consume iterables step the built-in Array, Map and
     Set iterators without a result object. `console.log` shows maps
     and sets as Node.js does. Runtime test262: 25,672 pass (was
     25,091), the same in stress mode; `Map`, `Set` and their iterator
     prototypes 561 tests, no failure (skipped or unsupported: `WeakMap`
     and `WeakSet` 20, feature 9; `BigInt` 26, feature 10; another realm
     2). Speed against `node --jitless`: `set` and `get` of 100,000
     integer keys 21 ms (8.8 ms), string keys 36 ms (12 ms); `for`-`of`
     over a set 2 times slower, over a map with `[k, v]` 5 times (the
     destructuring, feature 13). The review found two quadratic cases
     while unfinished iterators block the compaction (until the next
     collection): the LRU idiom `m.delete(m.keys().next().value)` scanned
     the deleted entries at the start with each new iterator (400,000
     steps: 13.7 s), and a key deleted and added again lengthened its
     probe in the index at each step (3 million steps: more than 2
     minutes). Fixed: scans start after the deleted entries at the start
     (`head`), and a delete takes the entry out of the index (Knuth's
     Algorithm R for linear probing); now 0.9 s for three forms of the
     idiom at 400,000 steps each, and 2.1 s (`node --jitless` 0.15 s and
     0.23 s). The review of that fix found a third case: a full iteration
     per step walked the deleted entries in the middle (400,000 steps:
     14 s). Fixed: a deleted entry holds a skip to a later position, with
     path splitting as in union-find; now 0.43 s. Difference from
     Chromium: `Map.groupBy` and `Object.groupBy` close the iterator of
     an array when the callback throws, as the specification says (V8
     does not call `return` there).
   - 6c Iterator helpers (`implementer`): the `Iterator` constructor,
     `Iterator.from`, `Iterator.concat` (in Chromium 148; `zip`,
     `zipKeyed`, `chunks`, `windows`, `join` and `includes` are not),
     the methods of `Iterator.prototype` (`map`, `filter`, `take`,
     `drop`, `flatMap`, `reduce`, `toArray`, `forEach`, `some`, `every`,
     `find`), `%IteratorHelperPrototype%`,
     `%WrapForValidIteratorPrototype%`.
     `Iterator.prototype[Symbol.dispose]` waits for explicit resource
     management (backlog).
     Done 2026-10-16: the abstract `Iterator` constructor with `from` and
     `concat`, the `constructor` and `@@toStringTag` accessors and the 11
     methods of `Iterator.prototype` in Chromium 148's key order,
     `%IteratorHelperPrototype%` and `%WrapForValidIteratorPrototype%`.
     The helpers are native state machines with the generator states;
     the closure variables of the specification are fields of the helper
     object, which the collector traces. A step over a built-in Array,
     Map or Set iterator or over another helper takes no result object,
     and `toArray` uses the dense builder. Each helper-to-helper step
     charges 2 KiB of the recursion budget (measured 0.7 to 1.2 KiB of
     stack per level), so chains deeper than about 2,000 helpers end
     with a catchable `RangeError` (Chromium: 5,000 work, 10,000 fail).
     499 cases in `tools/probes/js-iterators.json`, all a test, also in
     stress mode. Runtime test262: 26,091 pass (was 25,672), the same in
     stress mode; `built-ins/Iterator` 415 pass, no failure (7 need
     `BigInt`, feature 10; the proposals that Chromium 148 lacks are
     skipped). Speed against `node --jitless`, 100,000 elements:
     `values().map().filter().toArray()` 5.9 times slower,
     `Iterator.from` of a user iterator 9.3 times (the calls of its
     `next`, feature 13). Differences from Chromium: no `Symbol.dispose`;
     a finite `take` or `drop` limit above 2^53 − 1 throws `RangeError`
     (test262; Chromium accepts it); `return` through a chain ends at
     about 400 levels; two closing details where swb follows the
     specification (ADR 0026 section 15). The review found no blocking
     bug; it found a crash that existed before (backlog: bound
     functions).
   Feature 6 is complete (2026-10-16): runtime test262 went from 23,222
   to 26,091.
   Robustness fix after feature 6 (2026-10-16): a chain of natives that
   call their argument, bound to each other (`f =
   Function.prototype.call.bind(f)` 10,000 times, also `apply` and
   `Reflect.apply`), recursed in Rust without a limit and aborted the
   process (found in the 6c review). The deferred call of a native and
   the call of a bound function's target are now tail calls in a loop
   (ADR 0026 section 15): 200,000 levels take 0.2 s, as in Chromium
   148, which also returns the result.
7. Generators, promises, `async` (`implementer-hard`). Generator objects,
   `Promise`, the job queue and its host hooks, `async` functions, async
   generators, `for await`, async-from-sync iterators,
   `Array.fromAsync`.
   Split 2026-10-16 into four sessions, one commit each. Generator
   objects run since M6 (`vm/generator.rs`); `yield*`, `async` and
   `Promise` do not exist. test262 skips 2,645 tests for the `async`
   flag, 646 for `async-iteration` and 215 for `async-functions`, and
   rejects 100 for `yield*` and 90 for `async function`; `Promise` (732
   tests) and the groups of the generator, async function and async
   generator objects are not in the subset yet. Of the 84 target
   scripts, 39 compile; 27 stop at `async function` (the others at
   regular expression literals, feature 8).
   - 7a Generators complete: `yield*` (§15.5.5, the `next`, `throw` and
     `return` protocol of the delegate and its closing rules), the
     `GeneratorFunction` constructor (through 3f's
     `CreateDynamicFunction`), `%GeneratorFunction.prototype%` and its
     `prototype`, all of `%GeneratorPrototype%`.
     Done 2026-10-17: `yield*` is a loop of three new instructions over
     a record of four registers. The delegate's `next`, `throw` and
     `return` run as ordinary frames, the delegate's result object
     passes to the caller unchanged, and the messages are V8's, by the
     form of the operand. `GeneratorFunction` uses
     `CreateDynamicFunction` with a generator kind;
     `%GeneratorFunction.prototype%` and `%GeneratorPrototype%` have
     Chromium's key order. A chain of delegating generators costs
     frames, not Rust stack: 100,000 levels end with a catchable
     `RangeError`, as in Chromium 148. Runtime test262: 26,277 pass (was
     26,091), the same in stress mode; new groups
     `built-ins/GeneratorFunction` (20) and
     `built-ins/GeneratorPrototype` (61); parse-only unchanged (41,882).
     1M values through `for`-`of` over a generator take 340 ms
     (`node --jitless` 46 ms). Open: V8's message form "(intermediate
     value)(intermediate value)" for some call operands is not
     reproduced.
   - 7b `Promise` and the job queue: the constructor, `then`, `catch`,
     `finally`, `resolve`, `reject`, `all`, `allSettled`, `any`, `race`,
     `withResolvers`, `try`; the job queue (`HostEnqueuePromiseJob`)
     with a host call that runs the jobs (the shell runs them after each
     script), `HostPromiseRejectionTracker` with Chromium's report of an
     unhandled rejection; the test262 runner's async tests (`$DONE`,
     `doneprintHandle.js`, `asyncHelpers.js`, `promiseHelper.js`). The
     browser's event loop comes with the engine integration (ADR 0025).
     Done 2026-10-17: a promise is an object kind with reaction pairs.
     A handler is a function or an internal continuation, which is how
     7c attaches `await`. `NewPromiseCapability(%Promise%)` makes only
     the promise when no script can see its resolving functions. The
     job queue is a FIFO of the runtime; it is a GC root and counts as
     live heap. `Runtime::run_jobs` drains it; the shell calls it after
     each script. Each job is a step of the countdown with a safepoint
     after it, and a termination in a job reaches the host as a
     termination. The shell reports an unhandled rejection as
     `Uncaught (in promise) ...` and exits with 1. 256 Chromium 148
     cases (shape, messages, job order) give the same output, also in
     stress mode. A million `then` calls, 100,000 nested resolutions and
     `Promise.all` over a million elements run without Rust recursion.
     Runtime test262: 26,919 pass (was 26,277), the same in stress mode;
     the new group `built-ins/Promise` has 635 passes, 359 of them async
     tests; parse-only unchanged (41,882). 100k `then` steps take 29 ms
     (`node --jitless` 4 ms). Open: nested executors stop at about 400
     levels (Chromium 3,000).
   - 7c `async` functions: functions, arrows and methods, `await` as a
     suspension of the frame (on the generator machinery), the
     `AsyncFunction` constructor. Exit: the 27 target scripts that stop
     at `async function` compile, or each that does not has a reason.
     Done 2026-10-17: async functions, arrows and methods run on the
     generator machinery. The first instruction makes the result promise
     and an internal generator object. `await` is `PromiseResolve` and
     an internal reaction that resumes the frame from a job: one job for
     a native promise, as in Chromium 148, and no Rust stack for a chain
     of awaiting functions. One handler over the whole code rejects the
     promise on a throw, also in the parameters; a termination never
     becomes a rejection. `AsyncFunction` uses `CreateDynamicFunction`
     with an async kind. Stack traces show the awaiting functions
     (`at async f`), as in Chromium. 275 Chromium 148 cases (shape,
     messages, job order, stacks) give the same output, also in stress
     mode. Runtime test262: 27,701 pass (was 26,919), the same in stress
     mode; new group `built-ins/AsyncFunction` (16); 886 async-flag
     tests pass (were 359); parse-only unchanged (41,882). Of the 27
     target scripts that stopped at `async function`, 7 compile, 19 stop
     at regular expression literals (feature 8) and 1 at an async
     generator (7d); 46 of 84 compile. 100k `await`s take 18 ms
     (`node --jitless` 5 ms). Open: async stack traces do not follow
     `Promise.all`.
   - 7d Async iteration: async generators (§27.6, the queue of
     requests), `for await`, `yield*` in async generators,
     `%AsyncFromSyncIteratorPrototype%`, `%AsyncIteratorPrototype%`,
     `AsyncGeneratorFunction`, `Array.fromAsync`.
     Done 2026-10-17: async generators are generator objects with the
     queue of requests of §27.6. `next`, `return` and `throw` resume the
     frame in the caller's interpreter loop, and the frame gives the
     request's promise to the caller when it suspends. `yield` awaits its
     operand, the value of a return request is awaited before the frame
     resumes, and the end of the body completes the request and drains
     the queue. `for await` and `yield*` in async generators are the
     loops of 7a with awaits and an awaited close; sync iterables go
     through the Async-from-Sync Iterator, with the 2023 close on a
     rejected value. `Array.fromAsync` is a state machine of internal
     reactions. A chain of 100,000 `yield*` levels ends with a
     `RangeError`; a million values through `for await` and a million
     queued requests run without Rust recursion. 329 Chromium 148 cases
     give the same output, also in stress mode. swb follows the
     specification where V8 differs: a rejected return value at `yield*`
     and four error paths of `Array.fromAsync`. Runtime test262: 32,775
     pass (was 27,701), the same in stress mode; new groups for async
     generator statements and expressions, `for-await-of` and four
     built-in objects; 4,835 async-flag tests pass (were 886); parse-only
     unchanged (41,882). No target script stops at an async generator or
     `for await`; 46 of 84 compile, the other 38 stop at regular
     expression literals (feature 8).
   Feature 7 done 2026-10-17: generators complete, promises and the job
   queue, async functions, async iteration. Runtime test262 went from
   26,091 to 32,775 passes. The runner skipped all 2,645 async-flag tests
   before 7b; with the new groups in the subset, 4,835 pass now. Of the
   84 target scripts, 39 compiled before 7a and 46 now; none stops at
   `async`, `await`, async generators or `for await`.
8. Regular expressions (ADR 0026 section 10, memo 5.1).
   Split 2026-10-17 into three sessions, one commit each. `js-regexp`
   checks only the flags, and `compiler/support.rs` rejects regular
   expression literals: 38 of the 84 target scripts stop there. test262
   has 1,879 tests in `built-ins/RegExp` (613 of them property escapes),
   240 in `language/literals/regexp` (parse-only: 73 pass), 339 for the
   string methods that take a regular expression and 62 in
   `annexB/built-ins/RegExp`. bbc-js/s3.js reads `RegExp.$1`; no target
   script uses `\p{...}`, lookbehind or the `v` flag.
   - 8a The engine (`implementer-hard`; `js-regexp`, `js-syntax`): the
     pattern parser for the Unicode grammar and for the web grammar of
     Annex B.1.2, with all early errors (also the property names and
     values of §22.2.2.9), named groups (duplicates in different
     alternatives), modifiers (`(?i:...)`) and the class syntax of the
     `v` flag; the tree and its analyses; the compiler to a backtracking
     program with counter registers; the matcher as a loop with an
     explicit backtrack stack and an undo log, which follows §22.2.2
     exactly (capture reset and empty check per iteration, lookahead,
     lookbehind in backward direction, backreferences, the `i`, `m`,
     `s`, `u`, `v` and `y` semantics, both string widths, code points in
     `u` and `v` mode). Case-insensitive matching without `u` and `v`
     (Canonicalize with toUppercase, the tables of 5d). A time-step
     callback, the backtrack stack within a memory limit, caps on
     nesting, program size and capture count. The early errors of
     regular expression literals in `js-syntax`. The code point sets of
     `\p{...}` and the simple case folding of `u` and `v` with `i` come
     in 8c. Tests: unit tests, match results measured in Chromium 148
     through the probe tool, parse-only test262.
     Done 2026-10-17: the parser covers both grammars with all early
     errors and V8's messages; it keeps open groups and nested `v`
     classes on an explicit stack, and `check_literal` checks without
     building the tree. The compiler emits enum instructions with
     counter registers, one-instruction loops for one-character atoms
     and backward code for lookbehinds. The matcher stops after a given
     number of steps and continues on the next call, so 8b can run the
     time and heap checks between calls; its backtrack memory stops at
     the caller's limit. Case-insensitive matching canonicalizes classes
     at compile time in both modes: with `u` and `v` it already uses the
     simple case folding of 5d, so 8c needs only the property sets.
     `\p{...}` names and values are checked from tables generated from
     UCD 17.0.0; `Regex::new` rejects a pattern with `\p{...}` as not
     supported until 8c. 475 matches, 491 patterns and 46 literals give
     Chromium 148's results, also when a match runs in one-step slices,
     apart from seven cases where swb follows the specification (ADR 0026
     section 15). The review found three kinds of pattern whose compile
     took seconds to minutes and up to 10 GB (large `v` classes, many
     `\k<name>` of a duplicate name, many `\W` with `iu`); they now take
     milliseconds.
     test262 parse-only: 42,240 pass (was 41,882; all of
     `built-ins/RegExp` and `language/literals/regexp`); runtime: 32,805
     (was 32,775; negative literal tests), the same in stress mode. All
     84 target scripts parse, and their 1,142 literals compile. Most
     benchmark patterns run at 0.3 to 1.5 times the time of
     `node --jitless`.
   - 8b The `RegExp` built-in (`implementer-hard`; `js`): literals and
     the constructor (`RegExpCreate`, `RegExpInitialize`, `IsRegExp`,
     species), `lastIndex`, `exec` and `RegExpBuiltinExec` with the `d`
     indices and `groups`, `test`, `toString`, `source` with its escapes,
     the flag getters and `flags`, `Symbol.match`, `Symbol.matchAll` with
     `%RegExpStringIteratorPrototype%`, `Symbol.replace`, `Symbol.search`,
     `Symbol.split`, and the string methods that dispatch to them
     (`match`, `matchAll`, `replace`, `replaceAll`, `search`, `split`;
     `get_substitution` of 5b takes the captures and named captures),
     `RegExp.escape`, Annex B `RegExp.prototype.compile` and the legacy
     static properties (`RegExp.$1` to `$9`, `input`, `lastMatch`,
     `lastParen`, `leftContext`, `rightContext`, with their aliases),
     measured in Chromium. A match runs within the time and heap limits
     of the script, and a collection during a match changes nothing.
     `builtins/string.rs` installs the methods of `String.prototype` in
     V8's key order; add `match` and `matchAll` between `localeCompare`
     and `normalize`, and `search` after `replaceAll`. Exit: no target
     script stops at a regular expression literal, or each that does
     has a reason.
     Done 2026-10-17: a literal is compiled at its first evaluation and
     the program is kept in the literal's constant; each evaluation makes
     a new object. The constructor, `exec` with the `d` indices and
     `groups`, `test`, `toString`, `source`, the flags, the five symbol
     methods with the RegExp String Iterator, `match`, `matchAll` and
     `search` of `String.prototype`, `RegExp.escape`, Annex B `compile`
     and the legacy static properties as V8 has them. A match runs in
     slices of 32,768 steps with the time check and a safepoint between
     two slices. A backtrack stack beyond 256 MiB, or beyond the room
     under the heap limit, is V8's `RangeError` "Maximum call stack size
     exceeded" (at about 4M iterations of a loop; Chromium about 10M;
     backlog). The global loops of `match`, `replace` and `split` run
     natively when the `RegExp` object, `RegExp.prototype` and `RegExp`
     keep their recorded shapes, so the steps they skip cannot be
     observed. 453 snippets measured in Chromium 148 give the same
     output, also in stress mode. test262 runtime: 34,707 pass (was
     32,805), the same in stress mode; 531 `\p{...}` tests are
     unsupported until 8c; parse-only unchanged (42,240). All 84 target
     scripts compile (were 46).
   - 8c Unicode data (`implementer`; `js-text`, `js-regexp`): the code
     point sets of `\p{...}`, generated from the Unicode Character
     Database 17.0.0 (the version of the test262 property tests and of
     the 5d tables): General_Category, Script, Script_Extensions, the
     binary properties and the properties of strings of the `v` flag,
     with case folding of property sets in `v` mode with `i`. (The
     simple case folding of `u` and `v` with `i` came with 8a.) Tests: test262
     `built-ins/RegExp/property-escapes` and `unicodeSets`.
     Done 2026-10-17: a generator writes the sets of General_Category
     (with the groups), Script, Script_Extensions, the binary properties
     and the properties of strings from the UCD and emoji files 17.0.0,
     as delta-encoded ranges in `js-text` (about 82 KB in the binary).
     The parser builds each set once per property, `\P` and `i`, so
     10,000 `\p{L}` with `iu` compile in 1 to 2 ms. With `v` and `i` a
     lone General_Category value such as `\p{Lu}` is case folded as in
     Chromium, although the spec text leaves it unfolded (ADR 0026
     section 15). Classes with more than 8 strings and the properties of
     strings compile to a trie with a sorted table per node: after the
     review, `/\p{RGI_Emoji}/v` over a million characters takes 5 ms
     (was 43 s). 299 probe cases with `\p` give Chromium 148's results,
     except six cases of `\p{ASCII}` with `iv` and U+017F or U+212A,
     where swb follows the spec. test262 runtime: 35,233 pass (was 34,707), the same in stress mode;
     `built-ins/RegExp` 1,854 pass, 1 fail (needs `Date`); parse-only
     unchanged (42,240). The stress mode now collects less often after
     the first 65,536 safepoints of a run: the generated property tests
     loop a million times, and the stress run took 75 minutes (now
     about 10).
     Feature 8 done 2026-10-17: test262 runtime 32,775 → 35,233; the
     target scripts that compile 46 → 84.
9. Proxy and weak collections (`implementer-hard`). `Proxy` with all
   invariant checks, `WeakMap`, `WeakSet` and their ephemeron marking,
   `WeakRef`, `FinalizationRegistry`, the kept-alive list.
   Split 2026-10-17 into two sessions, one commit each. test262 has 311
   tests in `built-ins/Proxy`, 141 in `WeakMap`, 85 in `WeakSet`, 29 in
   `WeakRef` and 47 in `FinalizationRegistry`; in the current subset,
   123 tests are skipped for the feature `Proxy` and 21 fail with
   "Proxy is not defined". No test needs a forced collection. Target
   scripts: 4 use `new Proxy` (ars-js/s06.js, a reactive store with
   `get`, `set`, `has` and `ownKeys`; bbc-js/s2.js, a lazy object that
   forwards all 13 traps, and an `apply` trap on a function;
   bbc-js/s56.js and s57.js, a module namespace with `get` and
   `ownKeys`), 14 use `WeakMap`, 6 `WeakSet`, none `WeakRef` or
   `FinalizationRegistry`.
   - 9a Proxy (`implementer-hard`; `js`): `Proxy` and
     `Proxy.revocable`, the 13 internal methods of §10.5 with all
     invariant checks, callable and constructor proxies, revocation.
     Every path that reads an object through its internal methods must
     see a proxy: property access in the interpreter, `in`, `delete`,
     `instanceof`, `for-in` (EnumerateObjectProperties through
     `ownKeys` and `getOwnPropertyDescriptor`), spread, destructuring,
     `Object`, `Reflect`, `IsArray`, the `Array` methods and the native
     element access, `JSON`, `Function.prototype.toString`,
     `Object.prototype.toString`, `typeof`. Trap recursion charges the
     recursion budget (ADR 0026 section 9), so a chain of proxies of
     any length does not overflow the Rust stack. The shell's
     `console.log` shows a proxy without running its traps, as Node
     does. Tests: `built-ins/Proxy` and the tests with the feature
     `Proxy` (the harness file `proxyTrapsHelper.js`), probe cases for
     the uses of the target scripts, hostile cases.
     Done 2026-10-17: `Proxy` and `Proxy.revocable`; a proxy object kind
     with its target, its handler and the `[[Call]]` and `[[Construct]]`
     of its target. The heap stays free of script code: its walks for
     `[[Get]]`, `[[Set]]` and `[[HasProperty]]` stop at a proxy before
     any effect, and the runtime then runs the 13 methods of §10.5
     (`vm/proxy.rs`) in the spec order, with every invariant check and
     V8's messages. Heap methods that change objects refuse a proxy.
     `[[Call]]` and `[[Construct]]` without a trap are tail calls; the
     other methods charge 2 KiB of the recursion budget per level, so a
     chain of a million proxies gives V8's `RangeError`, also in the
     debug build on an 8 MiB thread. The duplicate and invariant checks
     of `ownKeys` use a hash set (linear for a million keys). `for-in`
     over proxies follows the order measured in Chromium. 672 snippets
     give Chromium 148's output, also in stress mode, except five where
     swb follows the spec (four `with` cases) or has its own message.
     test262 runtime: 35,666 pass (was 35,233), the same in stress mode;
     `built-ins/Proxy` 273 pass, 0 fail (36 cross-realm tests skipped);
     parse-only unchanged (42,240). A proxy read takes 90 ns without a
     trap and 200 ns with one (`node --jitless`: 16 and 28 ns).
   - 9b Weak collections (`implementer-hard`; `js`, the heap): `WeakMap`
     and `WeakSet` with the upsert methods and symbols as keys
     (`symbols-as-weakmap-keys`); ephemeron marking, linear in the
     number of entries also for long chains of ephemerons; `WeakRef`
     with the kept-alive list (AddToKeptObjects, ClearKeptObjects at the
     end of each job); `FinalizationRegistry` with `register` and
     `unregister`, and cleanup as a host job, never during a
     collection. Tests: the four `built-ins` directories, unit tests of
     the collector (chains of ephemerons, values that reference their
     keys, stress mode), hostile cases.
     Done 2026-10-18: `WeakMap` and `WeakSet` (with `getOrInsert` and
     `getOrInsertComputed`) over the hash table of `Map`, keyed by
     objects and by symbols that are not registered (well-known symbols
     too, as in the specification and Chromium 148). `WeakRef` with the
     kept set: `AddToKeptObjects` at construction and at `deref`,
     `ClearKeptObjects` at the end of `run_jobs`, once per microtask
     checkpoint as HTML does, not after each job. `FinalizationRegistry`
     with `register` and `unregister` (an index by token), and cleanup
     as a host task (`Runtime::run_finalization_cleanup`) that never
     runs during a collection; the queue holds its registries weakly, so
     a registry that script dropped calls nothing. The collector marks
     the ephemerons in a second phase, only when a `WeakMap` is live, in
     time linear in the entries: a value whose key is not marked yet
     waits in a list under the key (tests: chains of 100,000 links across
     1,000 tables in both orders, and of a million links in reverse
     order). The sweep removes dead entries, targets and tokens before
     the arena frees their slots. The first marking phase has no new
     check, and the drain skips the kind dispatch for plain objects and
     arrays: `Heap::collect` executes 7 to 12 % fewer instructions on
     heaps without weak collections. 318 snippets give Chromium 148's
     results, with no difference from the specification. test262
     runtime: 36,000 pass (was 35,666), the same in stress mode;
     `built-ins/WeakMap` 140, `WeakSet` 84, `WeakRef` 28,
     `FinalizationRegistry` 46 pass, 0 fail (4 cross-realm tests
     skipped); parse-only unchanged (42,240).
     Feature 9 done 2026-10-18: test262 runtime 35,233 → 36,000.
10. `BigInt` (`implementer-hard`). Split 2026-10-18 into two sessions,
    one commit each: the arithmetic first, then the language. test262:
    77 tests in `built-ins/BigInt`, 59 in `language/literals/bigint`; in
    the current subset, 177 tests are skipped for the feature `BigInt`
    and 62 are unsupported (BigInt literals). Target scripts: ars-js/s02.js
    (protobuf's 64-bit integers) calls `BigInt(...)` and
    `BigInt.asIntN`, and uses `<<`, `>>`, `|`, `&` and `Number(...)` on
    BigInts, after a `typeof BigInt` check with a fallback. No target
    script has a BigInt literal.
    - 10a Arithmetic (`implementer-hard`; `js`): the representation
      (sign and magnitude in 64-bit limbs, normalized), addition,
      subtraction, multiplication, division and remainder (truncating;
      long division after Knuth's Algorithm D), exponentiation, the
      shifts (`>>` rounds towards minus infinity), the bitwise
      operators on the infinite two's complement form, `asIntN`,
      `asUintN`, comparison with BigInts and with Numbers (exact, also
      for fractions, infinities and NaN), the conversion to a double
      (rounded to nearest, ties to even) and from an integral double,
      `StringToBigInt`, the literal forms, and `toString` in the radixes
      2 to 36. A size limit in bits and a time charge per limb operation
      bound the time of every operation (ADR 0026 section 9); every
      allocation is fallible. Tests: a table of results from an
      arbitrary-precision reference outside swb, edge values, hostile
      sizes.
      Done 2026-10-18: `js-text/src/bigint/`, without heap or engine
      types; the engine's `BigInt` type is the library's. Schoolbook
      multiplication; Algorithm D and the one-limb division with a
      precomputed reciprocal (Möller and Granlund 2011) instead of the
      hardware's 128-by-64 division; powers by squaring with the result
      size estimated first; shifts and two's complement bitwise
      operators that read only the limbs the result depends on;
      `asIntN`, `asUintN`; exact conversions and comparisons with
      doubles; `StringToBigInt`; literals; `toString` in the radixes 2
      to 36. The size limit is 2^18 bits (Chromium 148: 2^30 − 64; a
      KNOWN difference): at the limit, `toString` in radix 10 takes
      32 ms, a product or a quotient 3 ms; with 2^19 bits `toString`
      would take 126 ms. Every result above the limit fails before work
      in proportion to its size. `bigint::cost` gives the work of each
      operation for the time countdown. Tests: 6,636 reference cases
      from Python's `int` (40 of them need Algorithm D's add-back step);
      the review compared 40,352 more cases of its own with Python: no
      difference. test262 unchanged (36,000).
    - 10b The language (`implementer-hard`; `js`): BigInt literals in
      the compiler (also as property keys), `typeof`, the operators
      with `ToNumeric` and V8's messages for mixed types, comparisons
      and loose equality with Numbers and strings, `++` and `--`,
      `ToBigInt`, `ToBigInt64`, `ToBigUint64`, the conversions and
      their `TypeError`s (`ToNumber`, unary `+`, `>>>`, `Math`, `JSON`),
      the `BigInt` function with `asIntN`, `asUintN` and the prototype
      methods, wrapper objects, `console.log` as Node prints BigInts.
      The fast paths for Numbers stay as fast as before. Tests:
      `built-ins/BigInt`, `language/literals/bigint`, the tests with the
      feature `BigInt` in the subset, probe cases from Chromium, the
      int64 paths of ars-js/s02.js, hostile cases.
      Done 2026-10-18: literals are constants of the code object (`-1n`
      is one constant) and give their decimal text as keys; a literal
      above 2^18 bits is a `SyntaxError` at compile time, as V8 gives one
      above its limit (measured, also in functions that never run).
      `typeof`, all operators with `ToNumeric` and V8's messages, `==`
      and the relational operators with Numbers and strings, `++` and
      `--`, `ToBigInt`, `ToBigInt64`, `ToBigUint64`, the conversions and
      their `TypeError`s, `BigInt` with `asIntN`, `asUintN` and the
      prototype methods, wrapper objects, `console.log` as Node. Every
      operation and every decimal text, also one in a message, is
      charged to the time countdown before the work. The number fast
      paths did not get slower (`sum 10M integers` −5 % instructions).
      850 Chromium snippets equal (KNOWN: the size limit, ECMA-402
      locales). test262 runtime 36,000 → 36,369 (`built-ins/BigInt` 75
      of 77, `language/literals/bigint` 59 of 59, 62 unsupported → 0).
      ars-js/s02.js takes its BigInt branch; the 84 target scripts
      behave as before.
     Feature 10 done 2026-10-18: test262 runtime 36,000 → 36,369.
11. Binary data. Split 2026-10-18 into three sessions, one commit each.
    test262: 221 tests in `built-ins/ArrayBuffer`, 561 in `DataView`,
    1,453 in `TypedArray`, 738 in `TypedArrayConstructors`, 70 in
    `Uint8Array` (base64 and hex), 389 in `Atomics`; 356 tests of these
    groups need `SharedArrayBuffer`. 2,067 tests include
    `testTypedArray.js` and 302 `detachArrayBuffer.js`, which the runner
    does not load yet. Target scripts: `Uint8Array` in 13 files,
    `Uint32Array` in 8, `ArrayBuffer` in 7, `DataView` in 6, the other
    typed arrays in 4 to 6; none uses `BigInt64Array`, `Float16Array` or
    `Atomics`. `SharedArrayBuffer` stays out: Chromium exposes it only to
    cross-origin isolated pages (confirm with a probe). `TextEncoder`,
    `TextDecoder` and `crypto.getRandomValues` are Web APIs (the
    bindings, after M7).
    - 11a Buffers (`implementer-hard`; `js`): `ArrayBuffer` with its
      bytes outside the collected arenas but in the heap accounting
      (fallible allocation, the heap limit), resizable buffers
      (`maxByteLength`, `resize`), detach, `transfer`,
      `transferToFixedLength`, `slice`, `isView`, `@@species`;
      `DataView` with all getters and setters (`BigInt64` and
      `BigUint64` with `ToBigInt64` and `ToBigUint64` of 10b, `Float16`
      with `Math.f16round`: round to nearest, ties to even), views that
      track the length of a resizable buffer and the out-of-bounds
      checks; the runner: `$262.detachArrayBuffer` and the harness files
      of the feature. Measure whether Chromium 148 has the immutable
      `ArrayBuffer` (`transferToImmutable`, 61 tests); add it only if it
      does. This session sets the buffer API that 11b uses.
      Done 2026-10-18: `ArrayBuffer` (resizable, `transfer`,
      `transferToFixedLength`, `slice` with species, `isView`, detach)
      with its bytes in a vector outside the arenas, counted in the heap
      size; a buffer above the heap limit is a `RangeError` ("Array
      buffer allocation failed", as V8), not a termination. Zero-fills
      and copies are charged to the time countdown. `DataView` with the
      22 getters and setters (binary16 included) and length-tracking
      views; `$262.detachArrayBuffer` in the shell and the runner. 724
      Chromium snippets equal in normal and stress mode (KNOWN: the
      order of `ToIndex` in two places, NaN bits of operators,
      `Float16Array`). Chromium 148 has no `SharedArrayBuffer` in a
      normal page and no immutable buffers, so swb has neither. test262
      runtime 36,369 → 37,011 (`built-ins/ArrayBuffer` 149 of 173,
      `built-ins/DataView` 480 of 481; the 25 new failures call typed
      array constructors without the feature tag).
    - 11b Typed arrays (`implementer-hard`; `js`): `%TypedArray%` and the
      12 constructors (`Int8` to `Float64`, `Uint8Clamped`, `Float16`,
      `BigInt64`, `BigUint64`) with all argument forms, the TypedArray
      exotic object (the internal methods for canonical numeric keys),
      element access in the interpreter (a fast path, with `vm_bench`
      rows), the accessors (`buffer`, `byteLength`, `byteOffset`,
      `length`, `@@toStringTag`), length tracking and out-of-bounds,
      `from`, `of`, `set`, `subarray`, `slice`, the iterators,
      `TypedArraySpeciesCreate`.
      Done 2026-10-18: `%TypedArray%` (abstract) with `from`, `of` and
      `@@species`; the 12 constructors with all argument forms, V8's
      messages and V8's limit (2^35 − 1 bytes); the exotic object for
      every canonical numeric key (also "-0", "1.5", "NaN"); length
      tracking and out-of-bounds over resizable buffers; the accessors,
      `set` (also overlapping in one buffer), `subarray`, `slice`, the
      iterators; `console.log` as Node.js 22. `ta[i]` loops run as fast
      as `Array` loops; the `Array` and property rows changed by at most
      +0.7 % instructions. 887 Chromium snippets equal in normal and
      stress mode (KNOWN: 4 places where V8 does not follow the spec; 10
      snippets need 11c). test262 runtime 37,011 → 38,347
      (`TypedArray` 444 of 447 in the 11b directories,
      `TypedArrayConstructors` 658 of 658, `ArrayBuffer` and `DataView`
      without failures). The runner accepts `dir` lines below a group,
      so the 11c method tests stay out until 11c. ars-js/s01.js now
      builds its `Uint32Array` table and stops later, at `window`.
    - 11c Methods (`implementer`; `js`): the other methods of
      `%TypedArray%.prototype`, after the patterns of the `Array`
      methods (`at`, `copyWithin`, `every`, `fill`, `filter`, the `find`
      methods, `forEach`, `includes`, `indexOf`, `join`, `lastIndexOf`,
      `map`, `reduce`, `reduceRight`, `reverse`, `some`, `sort` with the
      numeric default order, `toLocaleString`, `toReversed`, `toSorted`,
      `with`, `toString`); `Atomics` on non-shared integer arrays
      (`wait` throws, `notify` returns 0); the base64 and hex methods of
      `Uint8Array`.
      Done 2026-10-18: the 25 other methods of `%TypedArray%.prototype`
      (`toString` is the function of `Array.prototype`); `Atomics` on
      `ArrayBuffer`s (the 12 functions, and `waitAsync` and `pause`,
      which Chromium 148 has; `wait` and `waitAsync` throw the TypeError
      for a buffer that is not shared, `notify` returns 0); the base64
      and hex methods of `Uint8Array`. Loops without a callback work on
      the bytes of the buffer in chunks that charge the time countdown.
      The default sort counts one-byte types and two-byte types of
      262,144 elements or more, and merges sorted runs of keys for the
      others;
      the sort with a comparator is a merge sort whose comparator calls
      equal Chromium's. 8,263 Chromium snippets (and the 897 of 11b)
      equal in normal and stress mode; KNOWN: the `toLocaleString`
      arguments (ECMA-402), the content check of the species result of
      `map` and `filter`, `isLockFree(1.5)`, `setFromBase64` into a full
      target (the proposal text and test262), the text of some objects
      in messages. test262 runtime 38,347 → 39,515 (`TypedArray` 1,438
      of 1,438 that run, `Atomics` 106 of 106 that run, `Uint8Array` 68
      of 68 that run; the runner skips the tests that make a
      `SharedArrayBuffer`). `indexOf` over 1M elements takes as long as
      in `node --jitless`, the default sort of 1M doubles 0.4 of its
      time, `map` with a callback 12.6 times its time.
      Feature 11 done 2026-10-18: buffers, `DataView`, the 12 typed
      arrays with all methods, `Atomics` and base64/hex;
      `SharedArrayBuffer` stays out, as in Chromium 148. test262 runtime
      36,369 → 39,515 over the three sessions.
12. `Date` (`implementer`; the time zone library under ADR 0003), in
    two sessions. Measured 2026-10-18: test262 has 594 tests in
    `built-ins/Date` and 24 in `annexB/built-ins/Date` (8 need
    `Temporal`, 3 `cross-realm`); 155 tests in other directories fail
    only with "Date is not defined". Target scripts: `new Date` in 27
    files, `Date.now` in 27, `getTime` in 18, `toISOString` in 11,
    `toUTCString` in 13 and `toGMTString` in 5 (cookie expiry dates),
    the local getters and setters in date formatters, `getTimezoneOffset`
    in 4, `Date.parse` in 2 (one with free-form text and "EDT" or "EST"
    at the end). `toLocaleDateString` with locales and options
    (`"en-GB"`, `timeZone: "UTC"`) and `Intl.DateTimeFormat` need
    `Intl`, which is outside M7: without `Intl`, the `toLocale*String`
    methods give Chromium's output for its default locale (en-US) and
    ignore their arguments, and the `Intl` uses go to the backlog.
    - 12a Time values and the built-in (`implementer`; `js`): the time
      value operations of §21.4.1 (`Day`, `YearFromTime`, `MakeTime`,
      `MakeDay`, `MakeDate`, `TimeClip`); the local time zone through a
      time zone library (the offset of a UTC time, and of a local time
      with the rules for skipped and repeated local times, §21.4.1.25
      and §21.4.1.26); a host hook for the clock and the time zone, so
      that tests use a fixed zone and a fixed clock; the constructor
      with all argument forms, `Date.UTC`, `Date.now`, all getters and
      setters (local and UTC), `getTimezoneOffset`, `toISOString`,
      `toJSON`, `toUTCString`, `toString`, `toDateString`,
      `toTimeString`, `valueOf`, `@@toPrimitive`; `toLocaleString`,
      `toLocaleDateString` and `toLocaleTimeString` as Chromium's en-US
      output without arguments; `Date.parse` of the
      date time string format (§21.4.1.32) and of the output of
      `toString` and `toUTCString`; Annex B `getYear`, `setYear`,
      `toGMTString`; the runner's harness files
      (`assertRelativeDateMs.js`, `dateConstants.js`). An ADR for the
      time zone source (the library, the system zone, the host hook).
      Done 2026-10-18: the time value operations, exact over the whole
      range; the local zone through `jiff` (ADR 0027; it was already in
      the lock file; +310 KB in `swb-js`): the offset of a UTC time,
      `UTC(t)` with the offset before a transition, the system zone at
      first use, UTC with a warning when there is no zone database;
      the host hooks `Runtime::set_clock` and `set_time_zone`, and the
      shell options `--now` and `--time-zone`. The `Date` constructor,
      `Date.UTC`, `Date.now`, `Date.parse` (the date time string format
      and the output of `toString` and `toUTCString`), all getters and
      setters in the ES2025 order, the text forms, `toJSON`,
      `@@toPrimitive`, Annex B, en-US `toLocale*String`. 11,670 Chromium
      results in seven zones equal in normal and stress mode (KNOWN
      where swb follows the spec: `getTimezoneOffset` with seconds,
      `setFullYear` reads the time value first, `Date.parse` rejects
      days past the end of the month, `MakeDay` beyond a million years;
      LATER: Chromium's other `Date.parse` forms and the long zone
      names, which 12b adds; until then `toString` prints the zone
      abbreviation). test262 runtime 39,515 → 40,286 (`built-ins/Date`
      583 of 583 that run, `annexB/built-ins/Date` 24 of 24; the 155
      tests that failed with "Date is not defined" pass); the runner
      uses the fixed zone `EST5EDT,M3.2.0,M11.1.0`. bbc-js/s5.js now
      runs to its end.
    - 12b Chromium's forms (`implementer`; `js`): `Date.parse` of the
      other forms that Chromium 148 accepts (month and weekday names,
      slashes, RFC 2822, two-digit years, zone abbreviations such as
      EST and EDT, `UT`, `GMT`, `Z`), from black-box probes only; the
      zone name in `toString` (Chromium prints the long English name,
      such as "Central European Summer Time": English names from CLDR
      under the Unicode license if the data is small, otherwise
      KNOWN).
      Done 2026-10-21, in two sessions: the reader of the first session
      was withdrawn because its design came from the author's memory of
      V8's date parser (ADR 0025); the second session wrote a grammar
      of named forms with sources. `Date.parse` reads the variants of
      ISO 8601 that the date time string format does not take, RFC 5322
      and its obsolete syntax, RFC 850, `asctime`, English and numeric
      dates with a clock and a zone, and the en-US `toLocale*String`
      output (`iso_variants.rs`, `written.rs`; one pass, no allocation,
      the scan is charged). Each rule comes from a standard or a probe
      in Chromium 148; a text outside the forms is NaN. Compared with
      Chromium on about 600,000 texts: no text where swb gives a value
      and Chromium NaN, and none with a different value; 501 snippets
      where Chromium reads a quirk are KNOWN (six classes, ADR 0027
      section 8). Days past the end of the month roll over as in
      Chromium. `toString` prints the long English zone name of CLDR 48
      (19.4 KB of data; the metazone of the present, the GMT format
      without a name, the saving flag at an equivalent year from a
      measured table of 14 years); 22 zones in the snippet tests.
      test262 runtime unchanged (40,286); `swb-js` +56 KB.
    Annex B: `getYear`, `setYear`, `toGMTString` (12a).
13. Performance (`implementer-hard`). Inline caches, a cache for
    global access, cached shapes for object literals, native callbacks
    without re-entry into the interpreter (`forEach` and similar),
    one byte per character for the kept source text of ASCII scripts,
    elision of temporal-dead-zone checks, and lazy built-ins or lazy
    compilation if measurements on the target scripts show the need.
    The spike measured calls 5x, property access 5x and object
    literals 11x slower than `node --jitless` (docs/js-spike-report.md).
    Baseline 2026-10-21 (e9badd9; `just jsbench out/bbc-js`, 95
    programs): geometric mean 3.4 times `node --jitless`, 43 programs at
    5 times or more. The slow rows have five causes: property access
    (`o.x = o.x + o.y` 4.8x, a `for` loop over an array 7.8x, the sum of
    a `Float64Array` 8.5x), object creation (1M small objects 10.7x),
    script-to-script calls (fib 5.9x, calls that read `arguments`
    12.8x), calls of natives from script (`Date.now` 3.3x, `DataView`
    5.3x, string methods 6 to 7x), and callbacks from natives (`forEach`
    8.7x, `reduce` 11x, typed-array `map` 12.5x, iterator helpers 6 to
    9.6x, promise jobs 7 to 9x, proxy traps 7 to 8.8x). Startup is 2 ms
    (empty script), so lazy built-ins are not needed. The 60 BBC scripts
    compile in 172 ms into 27.9 MB of code (7.9 bytes per source byte).
    Sessions, each measured with `jsbench` before and after, test262 in
    normal and stress mode:
    - 13a Property access (`implementer-hard`): monomorphic inline
      caches in the property sites (get and set; own data properties,
      data and accessor properties on the prototype chain, `length` of
      arrays and typed arrays), a cache for global access (the global
      object and the global lexical declarations), cached shapes for
      object literals.
      Done 2026-10-22: every named access, global access and object
      literal has a site of its own with one cache entry (28 bytes, a
      `Cell` in a table that the code object allocates at the first
      fill): own properties, the prototype chain (validated by a watch
      epoch that each change of the keys, attributes or prototype of a
      prototype or global object increments), adds by transition, array
      and typed-array `length`; global lexical bindings by position,
      global-object properties by the epoch and the count of lexical
      bindings; literals with plain data properties built in their final
      shape (`NewLiteral`, `InitSlot`). Shape handles in entries carry
      the generation. `jsbench`: `o.x` loop 367 → 135 ms, 1M small
      objects 239 → 76 ms, `for` loop over an array 124 → 65 ms,
      `Float64Array` sum 182 → 88 ms, fib 63 → 42 ms; geometric mean over
      95 programs 0.86 of base. Compile time +2 to 3 %, code +3.7 % (one
      site per access). test262 unchanged (40,286; stress the same).
    - 13b Calls (`implementer-hard`): the frame setup of script calls,
      the arguments object (a cached shape, a bulk element store), the
      path of native calls from script, and the code alignment of the
      interpreter loop (fib became 8 % slower with 7d at 3 % fewer
      instructions).
      Done 2026-10-22: a call reads the callee's code, `this` mode and
      realm in one lookup, binds a sloppy `this` in place, checks the
      frame and stack limits together and drops the frame record in
      place at the return; the check for a due collection is one
      comparison with a bound that changes only with the live size; a
      native call pushes one handle scope; `Value` has an 8-byte tag (a
      copy is two moves instead of five; `Root` keeps 16 bytes because
      it records property keys as values); arguments objects after the
      first of a realm are made from a template (shape and slots) with
      all elements at once. `jsbench`: fib 42.6 → 29.8 ms, closure
      counter 64 → 47 ms, calls that read `arguments` 513 → 214 ms,
      push and pop 136 → 107 ms, `Date.now` 83 → 80 ms; geometric mean
      over 95 programs 0.85 of 13a (2.5 times `node --jitless`). The
      interpreter loop is not sensitive to code alignment (five builds
      within 2 %), and a `match` with only the frequent instructions
      was 3 to 7 % slower, so the layout stays. test262 unchanged
      (40,286; stress the same).
    - 13c Callbacks from natives (`implementer-hard`): calls of script
      functions from native loops (`forEach`, `map`, `filter`, `reduce`,
      `sort`, typed-array methods, iterator helpers, `replace` with a
      function, proxy traps, promise jobs) without the cost of a new
      interpreter entry for each call; one shape for iterator results;
      `for`-`of` over a generator without the result object.
      Done 2026-10-22: natives that call script functions in a loop are
      native loops (`vm/native_loop.rs`): the native returns its state
      with its heap values in registers on the value stack, a step
      pushes the next call, and the call is a frame of the running
      interpreter loop whose return runs the next step; an exception
      reaches the loop's `on_error` before the handlers below it.
      Converted: the array and typed-array callback methods, `sort` and
      `toSorted` with a comparator (the merge sort as a state machine
      with the same comparisons), `replace` and `replaceAll` with a
      function. A dense element skips the search of the prototype chain
      for the next index. Iterator results have one shape per realm;
      `for`-`of` takes no result objects from generators with the
      built-in `next` and from String and `RegExp` String Iterators.
      Measured and kept on re-entry: proxy traps, promise jobs and the
      consumers of iterator helpers (as loops they were not faster).
      `jsbench`: `forEach` 66 → 51 ms, `reduce` 71 → 49, `filter` 77 →
      57, typed `map` 82 → 70, `sort` with a comparator 89 → 80,
      `matchAll` 150 → 88, `for`-`of` over a generator 266 → 69;
      geometric mean over 95 programs 0.97 of 13b (2.35 times
      `node --jitless`). Recursion through callbacks ends at the frame
      limit (4,999 levels; 408 before). test262 unchanged (40,286;
      stress the same).
    - 13d Strings and collections (`implementer`): a table of the 256
      one-unit Latin-1 strings, the main costs of the string-method rows
      (`slice` and `charCodeAt` 6.4x, `replace`/`trim`/`padStart` 5.5x,
      `toLowerCase` 5x, `split` 4.5x, `indexOf` 3.8x), a cached hash of
      `Map` string keys, a fast path of `toFixed` for few digits, a start
      offset for `shift` on dense arrays, integer results of `|` in
      arrays.
      Done 2026-10-22: strings of one code unit below 256 are atoms of a
      table that the heap keeps, so `s[i]`, `charAt`, `split`, the String
      Iterator and every substring of one unit allocate nothing; method
      reads on strings, numbers and booleans and the `length` of strings
      have a site cache entry (the realm's prototype and its shape);
      local fixes in the string methods found with callgrind (exact-size
      copies, one-pass case conversion of Latin-1 strings, `padStart` in
      one buffer, the `replace` capacity, a slice scan in `indexOf`, the
      `split` cache copy). `Map` and `Set` string keys keep 32 bits of
      their keyed hash in the padding of the string (no extra memory).
      `toFixed` with up to 20 digits computes in 128-bit integers (290 →
      121 ns; a differential test against the exact path passed 30
      million random doubles). Dense elements have a start offset
      (`DenseVec`), so `shift` takes constant time (100,000 shifts 3,500
      → 11 ms; Node.js 513 ms). Integer results of `|` on doubles were
      measured and rejected: they made the integer paths of the bitwise
      operators 25 % slower for a gain of 1 to 3 %. `jsbench`: `indexOf` 186 → 111 ms, `slice` and
      `charCodeAt` 235 → 168, case conversion 478 → 279, `replace`,
      `trim`, `padStart` 144 → 92, `match` with `g` 119 → 65, `for`-`of`
      over a string 69 → 34; geometric mean 0.86 of 13c (2.21 times
      `node --jitless` over 102 programs). test262 unchanged (40,286;
      stress the same).
    - 13e Compiler and memory (`implementer`): elision of
      temporal-dead-zone checks, the kept source text of ASCII scripts in
      one byte per character, array destructuring of dense arrays with
      the built-in iteration without an Array Iterator
      (`for (const [k, v] of map)` 4.6x), and the open check of
      `out/notes/stress-9b-r1.js` (GC stress mode with `WeakMap`
      callbacks ran more than 13 hours).
      Done 2026-10-22: a reference in a closure that is created after the
      declarator (a function expression, arrow or method; not a function
      declaration, not in a `switch` scope) has no temporal-dead-zone
      check (ADR 0026 section 4 has the rule and its reasons). The kept
      source text of a script with few units above 255 has one byte per
      unit and a list of the wide units (`SourceText`; the 60 BBC
      scripts 4.71 → 3.54 MB; scripts with only Latin-1 units had one
      byte per unit before). Array patterns step a plain array without
      an Array Iterator while `@@iterator`, `next` and `return` are the
      built-in ones (the watch epoch of 13a plus two slot reads); a
      `return` that appears while the pattern runs gets an Array
      Iterator at the stop index. `jsbench`: `const [a, b] = pair` 288 →
      52 ms, `function f([a, b])` 351 → 90, `for (const [k, v] of map)`
      136 → 66, three captured constants in a closure loop 138 → 127;
      geometric mean 0.957 of 13d over 108 programs (1.00 without the
      seven new rows). `stress-9b-r1.js` runs in 0.2 s with `--stress`
      at 13d and 13e; stress mode collects at each allocation, so its
      time grows with the live heap, and the weak structures add
      nothing. test262 unchanged (40,286; stress the same).
    - 13f Lazy compilation: not started (decision 2026-10-22). The
      targets do not need it: the 60 BBC scripts (16,653 functions)
      compile in 176 ms at 30 MiB peak, and the parse with the scope
      analysis (75 ms) stays with lazy compilation. Backlog item below.
    Feature 13 done 2026-10-22: geometric mean 2.05 times
    `node --jitless` over 107 programs (3.4 times over 95 at the start).
14. Modules (`implementer-hard`), when a target uses
    `<script type="module">`. Postponed 2026-10-22: no target needs
    module loading. Of the script tags of the five target pages, only
    one BBC script has `type="module"` (`dotcom-bootstrap.js`, the ads
    and cookie prompt loader, 75 KB). It has no `import`, `export`,
    `import()` or `import.meta`: a bundle in a strict-mode function,
    which gives the same result as a classic script. BBC also has one
    `nomodule` script (a Next.js chunk). None of the 84 target scripts
    uses `import()`. For the engine integration: run a module script
    without imports as strict code with `this` undefined, after the
    document is parsed (as `defer`), and skip `nomodule` scripts.

M7 exit (2026-10-22). test262 subset: 40,286 pass, 43 fail, the same in
stress mode; parse-only 42,240. All groups pass 95 % or more, except
seven groups of 13 to 38 tests where one or two tests fail. The failures
and their reasons:

- `null[key] op= v`, `null[key]++` and `null[key] &&= v` convert the
  key before the check of the base (backlog, M7 3e): 22 tests of
  `compound-assignment`, 2 each of `prefix-increment`,
  `prefix-decrement`, `postfix-increment` and `postfix-decrement`, 3
  of `logical-assignment`.
- `(fn) = function () {}` gives the function the name `fn`; a
  parenthesized target must not name it
  (`expressions/assignment/fn-name-lhs-cover.js`).
- Destructuring with a computed key evaluates the target reference in
  another order
  (`destructuring/binding/keyed-destructuring-property-reference-target-evaluation-order-with-bindings.js`).
- GlobalDeclarationInstantiation throws a `SyntaxError` where test262
  wants a `TypeError` (backlog, M7 3e): 2 tests of `global-code`.
- `undeclared = (this.undeclared = 5)` in strict code: swb resolves the
  global at the store, as V8 does; the spec resolves it before the
  right side (`identifier-resolution/assign-to-global-undefined.js`).
- The completion value of `try` in `eval` (backlog, M6:
  `statements/try/completion-values.js`); the catch parameter and its
  block share one scope (backlog, M7 3b:
  `statements/try/scope-catch-block-lex-open.js`).
- `Promise.try`: two tests of a 2026 change that Chromium 148 does not
  have (backlog, M7 7b).
- `%ThrowTypeError%` of arguments objects with non-simple parameters
  (backlog, M7 13b: `ThrowTypeError/unique-per-realm-non-simple.js`).

Skipped tests in the scope of ES2025: 174 `cross-realm` tests (the
shell has no `$262.createRealm`), 68 tests that include
`nativeFunctionMatcher.js`, the module tests (feature 14), 240
`SharedArrayBuffer` and 112 `atomicsHelper.js` tests (out of scope,
ADR 0026). The other skipped tests are proposals after ES2025.

After M7: the bindings and the engine integration (layers 2 and 3 of
ADR 0025) are M8.

## M8: JavaScript in the browser — in progress

Owner decisions (2026-10-22, ground rules):

- Targets: Hacker News, Wikipedia (without its video player) and a
  Craigslist posting, with JavaScript on. Each renders like Chromium
  with JavaScript on, no script error breaks it, and the JavaScript
  features in the lists of step 3 work. senko.net has no scripts and
  must stay the same. Later: Ars Technica and BBC (their scripts are
  mostly ads and consent dialogs), a Craigslist search page and GitHub.
  Stack Overflow is dropped: Cloudflare answers headless Chromium and
  swb with a challenge page. Frames and media playback are not in M8.
- The fixtures with JavaScript on (`fixtures/js/<name>/`) contain the
  sites' scripts, also commercial code. They stay on the local machine:
  the directory is git-ignored and never committed. Tests that need it
  skip when it is missing. The fixtures with JavaScript off, their
  references and their scores do not change.
- JavaScript stays off by default (ADR 0025).
- The W3C Software and Document License is allowed for IDL data files
  (ADR 0003 update). The defaults of ADR 0028 are accepted: the CORS
  check for requests from scripts, cookies and `localStorage` in memory,
  5 s per task, the page on the GUI's main thread.

Steps:

1. Survey (`implementer`, tools only): `capture --js` records a target
   with JavaScript on into `fixtures/js/`; `js-survey` replays it in
   Chromium and records the scripts, the Web APIs that they use per
   phase (parse, load, after load), the DOM changes, the network
   requests, the errors in replay, Chromium's script, style and layout
   times (with and without JIT) and the screenshot difference between
   JavaScript on and off. Report: `docs/js-survey.md`. Done 2026-10-22
   (4fff117) for the five targets; then for the candidates GitHub,
   Craigslist (a search page and a posting) and Stack Overflow
   (blocked), in the section "Candidates" of the report.
2. Design (`implementer-hard`, docs only). Done 2026-10-22:
   [ADR 0028](adr/0028-javascript-in-the-browser.md). The crates `web`
   (bindings, event loop, script processing) and `webidl` (a generator
   of the binding glue from WebIDL; the output is checked in); one
   runtime per document; the engine lends the document state to the
   runtime for each task; a resumable parser, a change journal and
   nodes that live as long as their document; one wrapper per node; the
   HTML event loop with a limit of 5 s per task; style and layout run
   when a query or a frame needs them, incremental style only if the
   measurements need it; the CORS check; frames and media out of scope.
   The owner approved its decisions on 2026-10-22.
3. Features, in this order. Design: ADR 0028. Each feature ends with a
   review and a commit. At every commit: JavaScript is off by default,
   `just snapshot` shows no change for the JS-off fixtures, and their
   scores do not change. Each feature adds its hostile-page cases
   (ADR 0028 section 16) and, where Chromium can observe the behaviour,
   probe cases with `"javascript": true` whose Chromium values are the
   expected values of the Rust tests. Target fixtures with JavaScript on
   (`fixtures/js/NAME`) are local; tests that need them skip when they
   are missing.

   What "works" means, per target (the owner confirms this list):

   - senko.net: no scripts. With JavaScript on, the same boxes and
     pixels as with JavaScript off and as Chromium with JavaScript on.
   - Hacker News (logged out, front page): the same rendering as
     Chromium with JavaScript on; `hn.js` runs at the end of the body
     without errors; its `click` listener on `document` is registered
     and runs on every click; links, the "more" link and the search
     form work as with JavaScript off. The logged-in features (vote
     with XHR, hide with `fetch` and `insertAdjacentHTML`, comment
     collapse) need a session and are not tested in replay; their APIs
     are in features 6 and 10.
   - Wikipedia (Web browser article): the rendering of Chromium with
     JavaScript on (the `client-js` styles, the pinned table of contents
     and Appearance panel with their "hide" buttons, the collapse
     chevrons of the table of contents); no script error in replay
     (Chromium has none); and these features:
     1. Appearance: color theme (light, dark, automatic), text size and
        page width, by the radio buttons; the choice is kept in the
        client preference cookie and `localStorage` within the session.
     2. Pin and unpin the table of contents and the Tools and
        Appearance menus (moved between the sidebar and the dropdown).
     3. Collapse and expand the sections of the table of contents.
     4. Collapsible tables and navboxes (`show`/`hide`, by click and by
        keyboard).
     5. The sticky header with search and tools after scrolling down.
     6. Reference previews on hover.
     7. The search box loads its typeahead code on focus; the
        suggestions come from the live API, so they work only on the
        live site (not tested in replay).
     8. Anchors and back/forward (`hashchange`).
     Not in M8: the video player (no media playback; the poster and
     play button may appear, since they are DOM, but nothing plays),
     find-in-page expansion (`beforematch`).
     Load-time budget: the `load` event within 0.7 s (ADR 0028
     section 16: three times Chromium's jitless task time, 217 ms, plus
     the JS-off pipeline time, 37 ms).
   - Craigslist posting (a for-sale posting with four photos and a map,
     `craigslist-posting`): with JavaScript off the page shows only the
     title, one sentence and a notice that JavaScript is needed. With
     JavaScript on: the rendering of Chromium with JavaScript on (the
     navigation bar with the area and category links, the toolbar with
     reply, favorite, hide, flag, share, the posting age and print, the
     heading with the price, the main photo, four thumbnails, the map
     with its tiles, the radius circle and the zoom and fullscreen
     buttons, the attributes, the text, the post id row); no script
     error in replay (Chromium has none); and these features:
     1. A click on a thumbnail shows that photo as the main photo (the
        local fixture has the four large photos).
     2. The zoom buttons and a drag move the map (tiles that the
        fixture lacks stay empty, as in Chromium's replay).
     3. The toolbar buttons do in swb what they do in Chromium's replay
        (feature 11 checks each in Chromium first).
     4. Reply opens its panel; the contact data comes from a request
        that only the live site answers (not tested in replay).
     Load-time budget: the `load` event within 0.45 s (three times
     Chromium's jitless task time, 149 ms, plus the JS-off pipeline
     time, 2 ms, measured with `perf` on 2026-10-22).

   Foundations:

   1. Test tools with JavaScript on (`implementer`, tools only).
      Scope: the probe option `"javascript": true`; `--with-swb` reads
      `js` queries from swb (`--eval EXPR`, which prints JSON; until
      feature 4c the swb side reports "not supported"); `reference --js
      NAME` and `compare --js NAME` over `fixtures/js/NAME`, with scores
      and references in `out/`; the Chromium routing answers CORS
      preflights and reflects the request origin with
      `Access-Control-Allow-Credentials` (ADR 0028 section 12), also in
      `capture --js`; a replay rule for a path segment that changes
      with time (Craigslist requests `/static/d/<version>/www/manifest.js`,
      and the version counts up; the local fixture holds 460 alias
      entries for now); a fixed start time in Chromium for `compare
      --js`.
      Tests: `tools/tests/` for the new options, with a small local
      fixture.
      Exit: `reference --js` runs for the M8 fixtures (senko-net,
      hacker-news, wikipedia-web-browser, craigslist-posting) in
      Chromium; a new `js-survey` run shows no CORS failures in replay
      (the Ars Technica and Craigslist fixtures had them).
      Done 2026-10-22. The `Fetch` domain pauses preflights, so the
      routing answers them itself. The rule for the changing path
      segment is `replay_patterns` in `fixture.json` (Python replay
      only; swb's replay reads it in 10a). The page clock starts at
      the capture time through an init script (a `Date` with a constant
      offset), because Playwright's clock replaces the timers. Three
      runs of `reference --js` give the same boxes on the four
      fixtures; no CORS errors in the new survey of Ars Technica and
      both Craigslist pages. Baseline of swb without JavaScript
      (geometry / pixels): senko-net 1.0000 / 0.9990, hacker-news
      1.0000 / 0.9986, wikipedia-web-browser 0.1810 / 0.9735,
      craigslist-posting 0.0000 / 0.6897.
   2. The JavaScript setting and the scripting flag (`implementer`).
      Scope: `--js` on the command line, the session option and
      `page.setJavaScript` of the automation API, `page.info`
      reporting it, a toolbar toggle and a shortcut in the GUI; the
      setting applies to the next document. With it on: the parser's
      scripting flag (`<noscript>` as raw text, its serialization),
      `@media (scripting)` matches `enabled`, media elements show
      controls only with `controls` (ADR 0020), `<canvas>` is a
      replaced element that paints nothing. No script runs yet. The GUI
      reads the process stack limit and keeps JavaScript off below
      8 MiB; the headless runner and server run the page on a thread
      with a 16 MiB stack (ADR 0028 section 2).
      Tests: probe cases with `"javascript": true` for `<noscript>` in
      `head` and `body`, `@media (scripting)`, `<video>` without
      `controls` and `<canvas>`, compared with `--with-swb`;
      automation tests for the setting.
      Exit: `just snapshot` unchanged; the cases match Chromium.
      Done 2026-10-22. The flag is data of the document
      (`Document::scripting`), copied from the page's setting when the
      response is parsed. With it on, `noscript` gets no box (Chromium
      builds none even with an author `display`; swb computes `display:
      none`, which `getComputedStyle` will show in 7a), and a video
      without `controls` paints only its poster. GUI: a "JS" toolbar
      button and Ctrl+Shift+J; below an 8 MiB stack limit the GUI
      ignores the toggle and `page.setJavaScript`. The 26 probe cases
      of `tools/probes/web-scripting.json` match Chromium; `compare --js`
      (geometry / pixels): senko-net 1.0000 / 0.9990, hacker-news
      1.0000 / 0.9986, wikipedia-web-browser 0.1810 / 0.9735,
      craigslist-posting 0.0000 / 0.7747.
   3. The DOM and the engine for scripts (`implementer-hard`), two
      sessions. No script runs yet; JavaScript off must not change.
      - 3a `dom`: the resumable parser (html5ever tokenizer with a
        `BufferQueue`, a step that stops at a script end tag, the
        document taken out of and put back into the sink), the change
        journal, the generation counter, the id index, fragment parsing
        with a context element and the move of the nodes into the
        arena, more document and fragment nodes in one arena with the
        node document of each node, the node limit and the DOM byte
        limit (ADR 0028 sections 3 and 16). Tests: unit tests for each;
        the journal of a parse of each JS-off fixture lists every node
        once; a pause at every script end tag and a resume give the same
        tree as one parse (all fixtures).
        Done 2026-10-23. `HtmlParser` pauses at HTML script end tags;
        the caller keeps the document and lends it to each step. The
        change functions follow the DOM algorithms and return `DomError`;
        each change goes to the journal, to an installed mutation sink
        and to the generation. The parse without scripts (`parse_html*`)
        records no journal (2 to 3 % of the parse time); its journal is
        incomplete, so 3b walks the new document once with JS off. The
        id index starts at the first lookup; with more than eight
        elements of one id, a lookup walks the tree. `parse_fragment`,
        node documents and inert template contents owner documents. The
        byte limit counts text, names, attribute values and a record
        size per attribute; the journal overflows past 16 MiB of old
        attribute values. ADR 0028 section 3 records the four changes of
        the design. Parse time +1 to +4 % against M7, snapshot
        unchanged, hostile set ok with `dom-node-limit` and
        `dom-byte-limit`.
      - 3b `engine`: the split of `Page` into the page and a boxed
        `DocumentState`; the parse in steps; the loads of stylesheets,
        images and posters started from the journal at insertion
        instead of the walk after the parse; derived data (tree order,
        form controls with dirty flags, disabled elements, title, base
        URL) rebuilt by generation; the parsed-stylesheet cache, so a
        stylist rebuild does not parse CSS again. Tests: engine tests
        that change the DOM through the API and check style, layout and
        loads; `just snapshot` and `just perf` (JS off) within noise.
        Done 2026-10-23. `Page` holds a boxed, `'static`
        `DocumentState` (DOM and parser, sheets, stylist, styles, layout,
        images, web fonts, requests, forms, scroll offsets, selection,
        element states); it shares the loader through an `Rc`, and the
        font context is an argument of the methods that lay out.
        `take_document_state` and `return_document_state` lend it; while
        it is lent, network completions stay queued. `change_dom`
        changes the DOM and drains the journal. With JavaScript on, the
        page parses with `HtmlParser` in steps; each drain reconciles the
        touched `style`, `link`, `img`, `video`, `picture` source, `base`
        and `title` elements in tree order and marks styles or the
        stylist dirty; flushes run on demand and do nothing without a
        new generation. The stylist keeps each sheet parsed (the
        parsed-stylesheet cache of 7a's plan). Title, base URL, tree
        order and forms (with dirty flags) are `Derived<T>` by
        generation. Snapshot unchanged with and without `--js`; perf
        within 3 % (stylesheets 10 % faster on large pages); hostile set
        ok with four new JS-on cases.
      Exit: snapshot unchanged, the JS-off perf table within 5 %, the
      hostile set passes.
   4. Embedding (`implementer-hard`), four sessions.
      - 4a `js`: host classes with parents, prototypes and interface
        objects per realm, the brand check; host hooks for the internal
        methods; the host-defined global object; host data that the
        collector traces as a root; termination requested by a native;
        event handler functions with an object-environment scope chain;
        the real size of source text in the heap accounting (ADR 0028
        section 6, items 1 to 6 and 8). Tests: unit tests in `js`
        with a test host (a class tree, a legacy indexed object, a
        named-property object), the GC stress mode, test262 unchanged.
        Done 2026-10-23. Host objects of classes without hooks
        (wrappers, the global object) are ordinary for the inline
        caches, a departure from ADR 0028 section 6 item 2; a class
        with hooks makes exotic objects that take the paths of proxies.
        Host data is a word (a `NodeId`) or traced generic data. The
        global object becomes a `Window` host object (`init_global`;
        dictionary mode afterwards); global reads +3 % on a micro row.
        A native's termination repeats until the outermost run returns,
        and a dropped one still ends the task. Event handlers compile as
        direct eval code with a captured chain of object environments.
        test262 unchanged in normal and stress mode; jsbench geomean
        −0.4 % (`toLowerCase` and a regexp row +15 to 20 % from a moved
        collection: the real source size, as the M7 13e item predicted).
      - 4b `webidl`: the IDL parser for the subset, the generator of
        the glue (section 5), the conversions, `DOMException`,
        `[Reflect]`, the freshness test, `just bindings`; the IDL files
        with their license headers (after the owner's decision on the
        W3C license). Tests: generator unit tests; generated code for
        `Node` and `EventTarget` compiles and passes the conversion
        tests (each WebIDL type, measured against Chromium where a
        conversion is observable: `setTimeout('x', '12abc')`,
        `[Clamp]`, `[EnforceRange]` errors).
        Done 2026-10-23. The owner had already allowed the W3C license
        for IDL files (ADR 0003, 2026-10-22). The generator writes a
        trait per interface for the hand-written functions and glue
        generic over the host type; node interfaces convert to
        `NodeId`, others to the object and its host data. Chromium's
        messages with the member prefix; overloads by argument count and
        by the Web IDL type test ("Overload resolution failed."); unions
        as generated enums; `[Reflect]` with swb's keyword attributes
        for enumerated attributes; `DOMException` inherits from
        `Error.prototype`, as in Chromium. `Event` is in `dom.idl` too
        (`EventTarget` needs it). Property order as in Chromium:
        attributes, constants, sorted operations, `constructor`. Not
        generated yet: indexed and named properties, `iterable`,
        `[LegacyFactoryFunction]`. test262 unchanged; jsbench within
        noise.
      - 4c1 `webidl` (split from 4c after 4b): the generator for
        legacy platform objects (Web IDL §3.9): indexed and named
        getters, setters and deleters through the 4a hooks, with
        `[LegacyOverrideBuiltIns]` and
        `[LegacyUnenumerableNamedProperties]`; `iterable` (value and
        pair iterators: `keys`, `values`, `entries`, `forEach`,
        `@@iterator`); the `js` hooks for `[[PreventExtensions]]` and
        `[[SetPrototypeOf]]` (legacy platform objects refuse to become
        non-extensible; `Window` and `WindowProperties` have immutable
        prototypes); Chromium's messages for refused indexed and named
        writes. Tests: the test IDL and a probe of `NodeList`,
        `HTMLCollection` and `DOMStringMap` in Chromium (property
        names, descriptors, `in`, strict-mode writes, `Object.freeze`,
        iteration).
        Done 2026-10-23. The special operations become trait functions
        of the declaring interface and hooks on every class whose
        instances have indexed or named properties (also by
        inheritance). Refused writes, definitions and deletions throw
        Chromium's messages in strict code; sloppy writes stay silent.
        Value iterators use the `Array.prototype` functions; pair
        iterators get generated "X Iterator" classes. `js` gained the
        `[[PreventExtensions]]` and refusal hooks and a `set` hook that
        can hand the property to the ordinary steps. Departure from the
        scope: `[[SetPrototypeOf]]` is not a hook but an
        immutable-prototype mark on any object, because a class with
        hooks makes its objects exotic and the global object must stay
        cacheable; Web IDL and HTML only need SetImmutablePrototype.
        `NodeList` is generated. Where Chromium departs from Web IDL
        (ADR 0028 section 5), swb follows Web IDL. test262 unchanged in
        both modes; jsbench geomean −0.6 %.
      - 4c2 `web`: the runtime per document, the lending of
        `DocumentState`, the wrapper vector and side table, `Window` as
        the global object, `Document`, `Node`, `Element` and
        `CharacterData` with their read-only tree accessors, `NodeList`
        for `childNodes`, `console`
        to the log and to the automation console buffer,
        `page.evaluate`, `page.consoleMessages`, `swb --eval`. Classic
        inline scripts run at the end of the parse (the real script
        processing is feature 5c). Tests: wrapper identity, expandos,
        `instanceof` along the interface chain, brand-check errors as in
        Chromium (`Illegal invocation`), prototype chains and property
        descriptors of `Node.prototype` against Chromium (probe).
        Done 2026-10-23. `swb_web::ScriptRuntime` per document; the
        engine lends `DocumentState` as `Box<dyn EngineHost>` for each
        task and takes it back by downcast. The wrapper vector and the
        side table are traced. The `Window` global has `WindowProperties`
        in its chain (named access by id and by `name`; a name of several
        elements gives one element, not an `HTMLCollection`). Departures
        from the scope: `Node` keeps only its read-only tree accessors
        (the tree-changing members return with 6a, so that feature
        detection does not find stubs); some read-only members of 6a's
        list are already here (`Document.URL`, `characterSet`,
        `compatMode`, `contentType`, `doctype`, `documentElement`,
        `Element` names, the `ParentNode` and `NonDocumentTypeChildNode`
        accessors), plus the `document.title`, `document.body` and
        `CharacterData.data` setters. HTML elements are `HTMLElement`
        (`head`: `HTMLHeadElement`) and there is no `HTMLDocument` yet.
        The live `childNodes` caches its children by DOM generation.
        `console` has five levels in Chromium's shape (a `js` change);
        `page.evaluate`, `page.consoleMessages`, `swb --eval`; inline
        scripts run after the parse; from 5b, three time-limit stops end
        a document's scripts. `PageConfig::scripts_allowed` refuses
        JavaScript on a small stack in the engine. The probe
        `web-dom-tree.json` (74 values) matches Chromium except the
        missing element interfaces.
      Exit: `swb --headless --js --eval 'document.title'` on the
      fixtures gives Chromium's value; a runtime per document is dropped
      on navigation (a test checks the memory returns); the time to
      create a runtime with the bindings is measured and in
      docs/performance.md.
      Feature 4 done 2026-10-23: `document.title` gives Chromium's value
      on all six page fixtures (a test); after a navigation the RSS
      returns (20 → 117 → 52 MiB, a test); a runtime with the bindings
      takes 1.25 ms (median) and 360 KiB of heap to create. With
      JavaScript off the snapshot is unchanged and page loads are within
      2 %; test262 unchanged.
   5. Events, the event loop and scripts in the parser
      (`implementer-hard`), three sessions.
      - 5a: the event core (ADR 0028 section 11): `EventTarget`
        (`addEventListener` with `capture`, `once` and `passive`,
        `removeEventListener`, `dispatchEvent`), `Event`,
        `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`,
        `PageTransitionEvent`; the dispatch algorithm (the path,
        capture and bubble, `stopPropagation`,
        `stopImmediatePropagation`, `preventDefault`, `composedPath`,
        `window.event`), exceptions of listeners reported without
        stopping the dispatch. Tests: dispatch order and phases,
        `once`, `passive`, against Chromium (probe cases); hostile:
        recursion through nested `dispatchEvent`, an endless loop in a
        listener.
      - 5b: the task queues, `Page::pump`, the microtask checkpoint
        (after tasks and after callbacks), `setTimeout`,
        `setInterval`, their `clear*`, the nesting clamp (Chromium's
        threshold measured with a probe), `queueMicrotask`, the
        deadline per task and the stop after three terminations, the
        heap-limit stop with the drop of the runtime, finalization
        cleanup tasks, uncaught errors as `error` events at the window
        and console lines, `unhandledrejection`, the page clock with
        real and virtual time, `--now` (`compare --js` then passes the
        fixture's capture time, as Chromium gets it),
        `page.waitForIdle` (the event loop until no task is due within
        a quiet time, or a time-out) and the headless `--settle-ms`.
        Tests: order of tasks, microtasks and timers against Chromium
        (probe cases that log an order); hostile cases: endless loops
        in a script, a timer and a microtask chain, `setInterval(0)`.
      - 5c: script processing (ADR 0028 section 8): parser-blocking
        classic scripts with the pause and resume, inline scripts,
        style sheets that block scripts, `defer`, `async`, scripts
        inserted by scripts (async and ordered; insertion through the
        minimal `createElement` and `appendChild` of this session,
        completed in 6a), `nomodule` skipped, types that are not
        JavaScript skipped, module scripts not run (warning), `load`
        and `error` at script elements, `document.currentScript`,
        `readyState`, `DOMContentLoaded`, `load`, `pageshow`,
        `pagehide`, the load event's delays, the new meaning of
        `page.waitForLoad`. Tests: the execution order of a page with
        all script kinds and stylesheets (with delayed responses from a
        test fetcher) against Chromium; a script that reads the DOM
        below itself (the read-only tree accessors of 4c) sees nothing;
        hostile: a script that inserts scripts in a chain.
      Exit: senko.net with JavaScript on matches its JS-off boxes and
      pixels; `hn.js` runs at the end of the Hacker News body (its error
      is the missing `getElementsByClassName`, fixed in 6b).
   6. DOM core and input events (`implementer`), four sessions.
      - 6a: tree changes (`appendChild`, `insertBefore`,
        `removeChild`, `replaceChild`, `remove`, `append`, `prepend`,
        `before`, `after`, `replaceWith`, `cloneNode`, `textContent`,
        `nodeValue`, `data`, `normalize`), `contains`,
        `compareDocumentPosition`, `isConnected`, `getRootNode`,
        `isEqualNode`; attributes (`getAttribute`, `setAttribute`,
        `removeAttribute`, `hasAttribute`, `toggleAttribute`,
        `attributes`, `Attr`), `id`, `className`, `classList`,
        `dataset`, `tagName`, `localName`, `namespaceURI`,
        `children`, the element sibling accessors; creation
        (`createElement`, `createElementNS`, `createTextNode`,
        `createComment`, `createDocumentFragment`), `getElementById`;
        the `Document` accessors (`body`, `head`, `documentElement`,
        `title`, `URL`, `referrer`, `characterSet`, `compatMode`,
        `defaultView`). Tests: probe cases for each group; hostile:
        DOM growth past the node limit, deep `appendChild` nesting.
      - 6b: queries and markup: `getElementsByTagName`,
        `getElementsByClassName`, `getElementsByName`, `scripts`,
        `forms` (live collections), `querySelector`,
        `querySelectorAll` (with `:scope`), `matches`, `closest`,
        `innerHTML`, `outerHTML`, `insertAdjacentHTML`,
        `insertAdjacentElement`, `DOMParser`,
        `DOMImplementation.createHTMLDocument`, `template.content`.
        Tests: probe cases; live collections after changes; hostile:
        huge `innerHTML`, a selector that matches every node of a
        large tree.
      - 6c: the HTML element interfaces with `[Reflect]` attributes for
        the elements and attributes that the M8 targets use (`a`,
        `img`, `script`, `link`, `meta`, `input`, `button`, `form`,
        `table` parts, `style`, `time`, `iframe` as an empty element)
        and the SVG element members that Leaflet uses. Tests: probe
        cases for reflection (enumerated attributes, URLs, numbers).
      - 6d: input as DOM events (ADR 0028 section 11): `UIEvent`,
        `MouseEvent`, `PointerEvent`, `KeyboardEvent`, `FocusEvent`,
        `InputEvent`, `WheelEvent`; `on*` attributes and properties;
        the engine's input as DOM events with default actions after
        the dispatch (mouse, keys, focus, the checkbox pre-activation,
        `submit` and `reset`), `click()`, `focus()`, `blur()`,
        `activeElement`, `hasFocus`; `javascript:` URLs. Tests:
        canceled default actions (a canceled `click` on a link does not
        navigate; a canceled `keydown` in a text field inserts
        nothing), handler attribute scope (`onclick="f(value)"` in a
        form), the event order of a click and of typing against
        Chromium.
      Exit (Hacker News): the Hacker News checks of the list above in
      an integration test on the local fixture; `compare --js
      hacker-news` gives the same scores as the JS-off comparison;
      load time within 50 ms of JS off.
   7. Style and layout from scripts (`implementer-hard`), two sessions.
      - 7a: invalidation from the journal, the flushes for queries
        (ADR 0028 section 9), counters of forced style and layout runs
        per load (in `--bench` and `page.info`); `element.style`
        (`CSSStyleDeclaration` over the style attribute: properties,
        `setProperty`, `removeProperty`, `getPropertyValue`,
        `cssText`, camel-case names); `getComputedStyle` with resolved
        values serialized as Chromium does, for the properties that
        `style` computes; `CSS.supports`. Tests: probe cases for the
        serialization of each property group (colors, lengths,
        percentages resolved by layout, `auto`, shorthands from
        `style`), style changes that change layout (class toggles,
        `hidden`, inserted style elements).
      - 7b: geometry (`getBoundingClientRect`, `getClientRects`,
        `offsetTop/Left/Width/Height/Parent`, `client*`, `scroll*`),
        scrolling from scripts (`scrollTop` and `scrollLeft` setters,
        `scrollTo`, `scrollBy`, `scrollIntoView`, `window.scroll*`,
        `scrollX`, `scrollY`, `pageYOffset`), `innerWidth`,
        `innerHeight`, `outerWidth`, `devicePixelRatio`, `screen`,
        `elementFromPoint`, `innerText`; `matchMedia` with `change`
        events; the rendering update with `requestAnimationFrame` and
        `cancelAnimationFrame`, `resize` and `scroll` events. Tests:
        probe cases for the geometry of each box kind (inline, block,
        positioned, transformed, scrolled, in tables); a rAF loop that
        counts frames in virtual time; hostile: a rAF loop that changes
        layout every frame.
      Exit: the counters for Wikipedia are recorded in
      docs/performance.md; the probe files pass.
   8. Window and document APIs (`implementer`), two sessions.
      - 8a: `location` (reads, `assign`, `replace`, `reload`, the
        `href` and `hash` setters), `history` (`pushState`,
        `replaceState`, `state`, `back`, `forward`, `go`), `popstate`,
        `hashchange`, `structuredClone`; `navigator` (ADR 0028 section
        13), `performance` (`now`, `timeOrigin`, `timing`, `mark`,
        `measure`, `getEntries*`, the navigation entry),
        `PerformanceObserver` (marks and measures only),
        `crypto.getRandomValues`, `randomUUID`, `URL`,
        `URLSearchParams`, `atob`, `btoa`, `TextEncoder`,
        `TextDecoder`, `requestIdleCallback`, `document.hidden` and
        `visibilityState` (`visible`), `visibilitychange`. Tests:
        probe cases (URL parsing and serialization, history entries and
        events, `structuredClone` of each type and the errors).
      - 8b: `document.cookie` with the jar (also in replay, ADR 0028
        section 12), `localStorage` and `sessionStorage` (named
        properties, quota, the `storage` event), the in-memory storage
        per origin. Tests: cookie attributes through `document.cookie`
        (path, expiry, `HttpOnly` invisible), storage across documents
        of one origin in one session; hostile: storage past its quota,
        thousands of cookies.
   9. Observers and forms (`implementer`), two sessions.
      - 9a: `MutationObserver` (all options, `takeRecords`,
        `disconnect`, records from the parser), `IntersectionObserver`
        (root, margins, thresholds, the first notification),
        `ResizeObserver` (content and border boxes, the loop limit and
        its error). Tests: record lists against Chromium; observers in
        virtual time while scrolling; hostile: a mutation observer that
        mutates in its callback without end.
      - 9b: forms from scripts: `value`, `defaultValue`, `checked`,
        `defaultChecked`, `selected`, `selectedIndex`, `disabled`,
        `form`, `elements`, `type`, with the dirty flags of ADR 0013;
        controls that scripts insert or remove; `input` and `change`
        from typing and clicks; `form.submit()`, `requestSubmit()`,
        `reset()`; `focusin` and `focusout`. Tests: probe cases for
        the IDL attributes and the event order of typing and of
        radio-button clicks.
   10. Network from scripts (`implementer`), two sessions (ADR 0028
      section 12).
      - 10a: credentials modes and the CORS check with preflights in
        `net` (more methods in `net::Method`); replay in swb that
        answers preflights and reflects the origin, as the Chromium
        routing does, and reads `replay_patterns` from `fixture.json`
        (then the alias entries of the local Craigslist fixture can
        go); `fetch` with `Request`, `Response`, `Headers`,
        `AbortController`, `AbortSignal` (`timeout`, and the `signal`
        option of `addEventListener`); at most 64 requests in flight
        per document. Tests: a local HTTP test server (as the cookie tests
        use) with CORS cases (simple, preflighted, credentialed,
        denied) whose outcomes match Chromium against the same server;
        hostile: thousands of pending fetches.
      - 10b: asynchronous `XMLHttpRequest`; `navigator.sendBeacon`;
        `FormData`, `Blob`, `FileReader` (data URL and text);
        `Image()`, `load` and `error` at images and stylesheets
        inserted by scripts. Tests: XHR event order and states against
        Chromium; image `load` and `error` order.
   11. Craigslist posting (`implementer`).
      Scope: what the fixture with JavaScript on still lacks after
      features 1 to 10. The session lists the gaps first; if they need
      more than one session, it splits them by area (11a, 11b) in this
      section before it starts. Expected: paths of jQuery 3 and jQuery Migrate
      (selectors through `querySelectorAll` and `matches`, event
      delegation, style writes), Leaflet (`style.transform` with
      `translate3d`, `offsetWidth`, `getBoundingClientRect`, tile images
      with `load` and `error`, mouse, touch, `dblclick`, `contextmenu`
      and `wheel` events, an SVG circle that the script creates, with
      `createElementNS` and the SVG element members it uses), the
      relative date (`time.timeago`), `screen.orientation`,
      `matchMedia`, and the hidden storage frame (ADR 0028 section 14:
      `contentWindow` is null; the page's call is inside `try`). An
      integration test on the local fixture drives each feature of the
      list above.
      Exit: the list works in the test; no script error that Chromium
      does not have; `compare --js craigslist-posting` geometry and
      pixels at least 0.98; the load-time budget is met.
   12. Wikipedia (`implementer`).
      Scope: what the Wikipedia fixture with JavaScript on still lacks
      after features 1 to 11 (expected: gaps in members of the HTML
      element interfaces, `eval` paths of ResourceLoader, details of
      events). As in 11, the session lists the gaps first and splits the
      feature if needed. An integration test on the local fixture drives each
      feature of the list above (click a theme radio and compare the
      root classes and a pixel sample with Chromium; pin and unpin;
      collapse; scroll to 2,000 px and check the sticky header; hover a
      reference; press back after an anchor).
      Exit: the list above works in the test; no script error that
      Chromium does not have; `compare --js wikipedia-web-browser`
      within 2 points of the JS-off scores; the load-time budget is
      met. If forced style runs take more than 20 % of the budget, 12b
      follows first.
      - 12b (only if 12 shows the need; `implementer-hard`): the style
        of one element on demand, then the restyle of dirty subtrees
        (ADR 0028 section 9).

   Later (not in M8):

   - A Craigslist search page: no special HTML features, but its result
     grid creates and removes about 140,000 elements, and Chromium lays
     it out 1,119 times while it loads and scrolls. It needs incremental
     style and layout, and replay of its credentialed API requests.
   - GitHub (github.com/senko/swb): custom elements (75 definitions),
     shadow DOM (24 hosts), `<template>`, `popover`, module scripts that
     load about 130 files with `import()` (M7 feature 14), and `fetch`
     of fragments for the first view. A milestone for web components.
   - Media playback (Wikipedia's video player) and frames.
   - Ars Technica (ADR 0028 designs the parts): `document.write` during
     the parse, the canvas 2D context stub, `getComputedStyle` in large
     numbers during the parse (the style of one element on demand), the
     Fides consent dialog with credentialed CORS `fetch` calls,
     `AbortController` and `FileReader`.
   - BBC: module scripts without imports (ADR 0028 section 6, item 7),
     `CSSStyleSheet.insertRule` at the rate of a CSS-in-JS library,
     `MessageChannel` and `postMessage`, `customElements.whenDefined`,
     XHR analytics, and probably incremental layout (Chromium lays it
     out about 100 times). Its consent modal is in a cross-origin
     frame, so its first viewport differs from Chromium's until frames
     come.

## Pending decisions

- Clean-room rewrites of the code derived from Chromium (table layout
  column constraints, width distribution, row heights; grid track
  sizing; sticky offsets; two float helpers), Skia data and Servo
  (`collapsed_margin.rs`, MPL-2.0); kept with attribution for now
  (ADR 0021).

## Backlog from M8

- Canvas with JavaScript off (found in feature 2): Chromium gives a
  block-level `<canvas>` with `width` and `height` attributes the
  `aspect-ratio: auto w / h` hint also with scripting off (an empty
  canvas with `width: 60px` and `width=2 height=1` is 30 px high); swb
  adds the hint only with scripting on. A small change in `hints.rs`,
  but it changes JS-off rendering, so check the fixtures.
- `noscript` with JavaScript off (found in feature 2): an inline
  `noscript` with block children gets another union box than in
  Chromium (block-in-inline; Chromium y=42 h=134, swb y=26 h=150).
- CSS (found by the GitHub candidate, 2026-10-22): the header
  navigation of github.com/senko/swb is 402 px wide in swb and 627 px
  in Chromium, because the menu buttons have no side padding. The rule
  is `padding: var(--base-size-8)` inside `@media (width>=1012px)`,
  and the custom properties are in a `:root` rule after `@layer`
  statements. Find which of the three swb gets wrong. The JS-off
  capture is in the git-ignored `out/candidates/github-swb/`.
- `compare`: the Chromium reference of the GitHub capture is scrolled
  by 394 px (the box dump starts at y = 394). Find the cause before
  GitHub becomes a target.
- `substitute` fails on raster `data:` URLs in CSS (GitHub has two
  3x2 px PNGs; they were replaced by hand). Replace them with an SVG
  `data:` URL of the same size.
- DOM (found in feature 3a): `element_ids()` (SVG `use` references,
  layout) still scans the whole tree; it could use the id index.
- DOM, feature 6a: `getElementById` in a script loop with many elements
  of one id walks the tree at each call; a cache of the result keyed by
  the generation would help. Hostile case: 100,000 elements with one id
  and `getElementById` in a loop.
- DOM, feature 6a: the fragment parser takes its scripting flag from the
  context's document. Probe Chromium for `<noscript>` in `innerHTML`
  with JavaScript on.
- DOM, feature 6a: elements of a parsed fragment get no form element
  pointer; the form owner must come from the ancestors at insertion
  (HTML "reset the form owner").
- DOM: only the document of `NodeId::DOCUMENT` has a quirks mode; other
  documents of the arena are in no-quirks mode (matters for
  `DOMParser`).
- DOM: the parser can create a few nodes past the node limit (the token
  at hand finishes; at most about the list of active formatting
  elements). The public change functions are exact.
- DOM: with the id index active, inserting or removing a node with an
  id or with children costs O(depth) (`is_connected`), and the parser's
  depth check is O(depth) when a script moved the open element deep.
  A script that builds a very deep chain is quadratic until the 5 s
  task limit stops it. Cache connectedness or cap the depth of
  script-built trees if a page needs it.
- DOM: `Document::nodes` never shrinks, and the node limit counts every
  node ever created (immortal nodes, ADR 0028 section 3). A page that
  re-renders for hours reaches the limit; node reclamation is the fix.
- Engine (found in feature 3b): a change in a detached subtree or in
  template contents marks the styles dirty (the drain does not check
  that a change is in the document). Fix when 7a counts forced restyles.
- Engine, 6a/6b: `img` elements that are not in the document do not load
  (`new Image()` with `src` loads in browsers).
- Engine: the text of `style` and `title` elements is their descendant
  text; HTML uses the child text content.
- Engine, forms: after a rebuild, when a script sets `checked` on a
  radio button whose group has a dirty checked button, the last in tree
  order wins; HTML says the newly checked one wins.
- Engine: when the focused element is removed, focus becomes none, but
  `focus_start` and `last_press` can still name the removed node (a
  start for Tab and keyboard scrolling). A selection whose end is
  removed goes away; DOM ranges move the boundary instead.
- Engine: a `link` that is removed and inserted again in another drain
  fetches its sheet again; a small cache of sheet text by URL (as
  Chromium's memory cache) would avoid it.
- Engine: `traverse_to_post` cancels the loader's requests without
  clearing the old document's request map or forgetting its loading
  images; `start_navigation` does not forget cancelled images. Harmless
  while the old document is replaced at commit.
- Engine: the loader has no priorities; with the parse in steps, the
  images of one step start before the sheets of the next. Sheets first
  would load pages faster on slow networks.
- `link` with the `disabled` attribute still applies (older than M8).
- Tools: `snapshot` has no `--js` option (feature 3b used a copy of the
  script). The `parse` stage of `--bench` is bimodal on small pages
  (senko-net 0.06 or 0.09 ms).
- `js`, feature 4c: an empty handler attribute (`onclick=""`) gives
  `function onclick() { [native code] }` in Chromium; swb would give
  `function onclick(event) {\n\n}`.
- `js`: stack frames of event handlers show no function name (Chromium:
  `at HTMLDivElement.onclick`), and their positions count from the
  synthesized source, not from the attribute's line.
- `js`: a handler body that closes the function early and continues
  (`}); x; ({`) gives "Single function literal required"; Chromium
  parses the body alone and says "Unexpected token '}'".
- `js`: the global object is a dictionary after `init_global`. Global
  reads stay cached, but `window.x` and `globalThis.x` reads are not
  (dictionary receivers never are). If pages read `window.foo` in hot
  loops, an entry for the global object keyed by its epoch would cache
  them.
- `js`: `HOOK_WEIGHT` is the proxy weight (2 KiB), not measured.
- `js`: `Runtime::heap_mut` and `Heap::new_host_object` are a wide
  `pub` surface; narrow them before 4b and 4c depend on them.
- `js`: the source text of a script is charged only while the script's
  code object lives; its functions keep the shared text alive after
  that, uncharged (older than M8).
- `webidl`, 6a/6b (found in feature 4b): `[LegacyFactoryFunction]`
  (`Image`, `Option`) is not generated; it needs a `js` API for a
  constructor function with a given `prototype`.
- `web`: `DOMException`s that the bindings throw have no own `stack`
  (Chromium: an own string `stack` whose first line is `Name: message`).
- `web`: overload resolution selects the overload before it converts
  the earlier arguments (Web IDL: after). Not observable with the real
  IDL files (no overloads yet); revisit when an overload has its
  distinguishing argument after the first.
- `web`: sequences are capped at 2^24 items; a 10^9-length array-like
  runs 9 s (debug build) before the `RangeError`, and each item is
  recorded in the handle scope. A lower cap or a time charge per item
  would end such scripts sooner.
- `web`: `[SecureContext]` members are exposed everywhere; `[Exposed]`
  other than `Window` or `*` exposes nothing (no workers).
- `web`: `[Reflect]` of `double` (needs the HTML float parser) and of
  element references is not generated. The setter message of a
  positive reflected integer ("The value provided is 0, which is an
  invalid size.") is a guess; measure `input.size = 0`.
- `web`: Chromium lists the attributes of `Event.prototype` and
  `DOMException.prototype` in its own order; swb uses the
  specifications' order.
- `webidl`: the checks accept duplicate enum values, unions of
  indistinguishable members, overloads such as `f(any)` with `f(long)`,
  a repeated `stringifier;` and `1e999`; nesting of about 1,000 levels
  overflows the stack. The IDL files are ours, so these are checks
  against our own mistakes.
- `web`, tests: no `sequence<object>`, `sequence` of callbacks, record of
  objects, callback-typed dictionary member, or `Promise` followed by
  more arguments in `test.idl`.
- `web` uses `Runtime::heap_mut` in three places, and its `keep` helper
  repeats `Runtime::keep`. The generated code has long one-line
  statements; `freshness.rs` repeats the stale-file logic of `main.rs`.
- `js`: V8 says "Class constructor X cannot be invoked without 'new'"
  (with the class name); swb says "Class constructors cannot be invoked
  without 'new'".
- `jsbench`: the best of 5 runs depends on where collections fall (the
  real source size in 4a made `toLowerCase` +17 % and a regexp row
  +15 %). A collection before each timed run, or the median, would make
  such rows stable.
- `web` (found in feature 4c1): where Chromium departs from Web IDL §3.9,
  swb follows Web IDL. `Reflect.defineProperty` of a refused index or
  name answers `true` in Chromium; `Object.create(list)[0] = x` writes in
  Chromium (Web IDL refuses); `Object.create(collection).f1 = x` is
  refused in Chromium (Web IDL defines it on the receiver); Chromium's
  `[[OwnPropertyKeys]]` lists stored strings before the names, symbols
  first for `DOMStringMap`, all supported names (also hidden ones such as
  `length`), and no names for `HTMLFormElement`; `storage.getItem = x`
  makes an own property in Chromium (Web IDL: the named setter). Change
  only if a page depends on one.
- `web`: the message prefixes of exceptions from indexed and named
  getters and from the name enumeration ("Failed to read an indexed
  property [0] from 'X': ") are guesses in Chromium's form; a probe needs
  an interface whose getter throws.
- `web`, 4c2: `Object.preventExtensions(window)` throws in Chromium (the
  WindowProxy refuses); swb's global object becomes non-extensible. A
  mark like the immutable-prototype one would do (a hook would make the
  global object exotic).
- `web`, 4c2: the cost of legacy platform objects. Every property read on
  an object with named properties converts the key to a string and calls
  `is_supported_name` (also `c.length`); every indexed read calls
  `index_count`; `Object.keys(c)` is O(n²) with the default
  `is_supported_name`; a pair iterator's `next` calls `pair_at` per step.
  Live collections need the cache keyed by the DOM generation
  (`childNodes` has it since 4c2; `HTMLCollection` in 6b).
- `web`, 4c2: the named property visibility algorithm does not skip a
  named properties object on the prototype chain (Web IDL §3.9.7); add
  it with the `WindowProperties` hooks.
- `web`, 6x: `Location` needs the immutable-prototype mark; Chromium's
  message names it `'[object Location]'`, swb's `object_text` would give
  `#<Location>`.
- `js`: `[[OwnPropertyKeys]]` of a host object with hooks lists stored
  index keys before stored strings; Web IDL lists the stored strings in
  creation order. Not observable while indexed definitions are refused.
- Engine (found in feature 4c2): a document without a declared encoding
  (a `file:` page) is decoded as UTF-8; Chromium uses windows-1252
  (`document.characterSet` in the probe). This is the HTML sniffing
  default and changes JS-off rendering of such files, so check the
  fixtures before changing it.
- `web`, 6x: the element interfaces (`HTMLDivElement`,
  `HTMLHtmlElement`, ..., `SVGElement` and its subclasses) and
  `HTMLDocument`; the probe `web-dom-tree.json` has Chromium's chains.
- `web`, 6b: `window[name]` of several elements must give an
  `HTMLCollection`. The `name` cache rebuilds with a tree walk at the
  first miss after each DOM change: a loop that changes the DOM and
  reads a missing global is O(n) per step (hostile case
  `window-named-lookups-after-changes-js-on`, 0.35 s for 1,000 × 50,000).
- `web`: the `Document`, `Text` and `Comment` constructors;
  `Text.splitText`, `wholeText`; `template.content`.
- `web`: any change in the arena invalidates every `childNodes` cache
  (one generation counter); a generation per parent would avoid the
  rebuild of unrelated lists.
- `web`, 5b: `page.evaluate` of a promise that settles later (timers)
  needs the event loop; now it is an error.
- `web`: `Object.getOwnPropertyNames` of `WindowProperties` lists no
  names (as in Chromium, measured); Web IDL with
  `[LegacyUnenumerableNamedProperties]` would list them.
- Engine, 5b: after a heap-limit stop, drop the runtime (ADR 0028
  section 7); now only the scripts stop.
- `swb --eval` with JavaScript off prints an error line per expression;
  the probe contract does not say what a swb without JavaScript does.
- `js`: `console.log` and the other levels take no format specifiers
  (`%s`, `%o`); older than M8.
- `dom` (found in the 4c2 review): `CharacterData.data` and
  `document.title` hold Rust strings, so `t.data = '\ud800'` reads back
  U+FFFD (Chromium keeps the lone surrogate). A limit of the DOM string
  type.
- `web`: wrappers stay alive for the document's life (ADR 0028
  section 4); the wrapper vector, the side table and the window name
  cache are not counted in `heap_size`, so the heap limit does not see
  them (the DOM node limit bounds them; the hostile wrapper case reaches
  366 MiB RSS). With the mutators of 6a, drop stale `childNodes` caches
  when the generation changes.
- `web`: every `console.error` and `console.warn` line (up to 4 MiB) goes
  to the log at error or warn level, which the default logger prints to
  stderr; `for (;;) console.error(x)` writes stderr for up to 15 s.
  Rate-limit the lines or log them at a lower level.
- `web`, 5b: the time limit is per task (5 s; three hits stop the
  scripts). A page of 1,000 inline scripts of 4.9 s each runs 80
  minutes; 5b needs a budget per document.
- `web`, 5c: the inline scripts are collected before the first runs, so
  a script that removes a later `<script>` (possible after 6a) does not
  stop it.

## Backlog from M6

From the maintenance review of the JavaScript crates. Items that an M7
feature already covers (regular expression patterns, loop-driven
`forEach` and `map`, pre-sized array literals, `toString(radix)`) are
not repeated here; the open items of each spike session are in M6 step
3 above.

- The scope analysis's capture map: check that a crafted script cannot
  drive it into long probe chains (it has the per-process key now).
- The test262 runner recognizes "not supported yet" (module code,
  `import()`, `import.meta`) by the message text of the script error;
  make it a structured field of the compile error.
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
- Time charges along prototype chains (M7 3a): property reads, `in`
  and `[[Set]]` do not charge a step per prototype. With a 100 ms
  limit, `while (true) top.missing;` on a chain of 100,000 objects
  ran 7 s; building such a chain with `o.k = v` is quadratic (each set
  walks the chain for setters). The `for`-`in` walk is charged.
- `js-syntax` (M7 3b): the parameter of a `catch` clause and its block
  share one scope, so a closure in a default of a parameter pattern
  sees the block's lexical declarations (test262
  `statements/try/scope-catch-block-lex-open.js`). A fix needs a
  separate block scope and moves the early errors of §14.15.1 and
  Annex B.3.4 that now rely on the shared scope. The bindings of a
  catch parameter pattern also have no dead zone:
  `try { throw [] } catch ([e = (() => f)(), f]) {}` throws a
  `ReferenceError` in Node.js and nothing in swb (13e review).
- Calls (M7 13b):
  - Arguments objects of sloppy functions with non-simple parameters
    (defaults, rest, patterns) have `callee` as a data property; the
    spec (§10.4.4.6, `CreateUnmappedArgumentsObject`) and V8 give the
    `%ThrowTypeError%` accessor.
  - An arguments object costs about 0.21 µs (`node --jitless` 0.04 to
    0.06 µs): three allocations and one cell per mapped parameter;
    `arguments[i]` takes the generic `[[Get]]` (about 230
    instructions), a fast path for dense elements would halve it.
  - The interpreter loop spills the frame base, the stack length and the
    operands: about 12 instructions of dispatch per bytecode instruction
    and 9 per register access. Not tried: a local slice of the
    instructions with an outer loop for frame changes. The frame record
    is 128 bytes; `extra_args` and `new_target` could be smaller.
  - A call of a chain of 100,000 bound functions ends with "Invalid
    string length"; Node calls it (13b review; also at 13a).
- Callbacks from natives (M7 13c):
  - Iterator helpers step synchronously and call the mapper, the
    predicate and a script's `next` by re-entry. Native loops for the
    consumers alone were slower (instruction cache misses from the
    re-entries inside a step). A fix needs resumable helper steps: a
    pending state per helper (waiting for the underlying `next` or for
    the callback) and a consumer loop that keeps the stack of helpers
    in progress, with the rules for `return()`, `Executing` and
    `IfAbruptCloseIterator`.
  - Proxy traps and promise jobs use re-entry: as native loops (a boxed
    state, a record and two steps per call) they were slower (proxy
    `get` 36 %, 100k awaits 10 %). A state stored in the record, or a
    `ReturnTo` that checks a trap result, would make loops with one call
    cheaper.
  - Typed-array `sort` with a comparator uses re-entry; a state machine
    like `SortMachine` would convert it.
  - The `find` family reads a run of indices that no object of the
    chain stores as `undefined`, also when a callback adds an element
    there during the run (as before 13c); `[[Get]]` would see it.
  - The re-entry path has about 1 % more instructions than at 13b; the
    iterator-helper rows that call script functions are 2 to 6 %
    slower, the proxy `get` trap row 1 to 6 %.
  - Recursion through callbacks ends at the frame limit (4,999 levels
    of `f → forEach → callback`); V8 stops at about 2,800 for
    `forEach` and 705 for `replace`.
  - Stack traces show no frames of natives (V8: `at Array.forEach`).
- Strings and collections (M7 13d):
  - Every string buffer is a `malloc` (about 220 instructions when
    cold). Strings of up to 8 units stored in the slot would remove it;
    a leaf that `s += x` extends in place while no other value shares
    it would fix the row of 20k appends (6.7 times `node --jitless`).
  - `Object.keys` and other conversions of array indices to keys make a
    decimal string each time; a table for 0 to 255, like the one-unit
    strings, would help.
  - The site cache of primitives covers own properties of the realm's
    `String`, `Number` and `Boolean` prototypes only; properties further
    up the chain (`'x'.hasOwnProperty`) take the general `[[Get]]`.
  - `padStart` and `padEnd` with more than 256 units still make a rope
    of the filler.
- Compiler and memory (M7 13e):
  - Lazy compilation (13f, not started): it would save at most the code
    generation (BBC 100 ms of 176 ms) and part of the code objects (BBC
    21.7 MB, 1.3 KB per function) of functions that never run. Decide
    with the engine integration, when a page has a load-time budget or a
    memory ceiling; cheaper first steps are a smaller instruction (12
    bytes) and shared line tables. To measure the share of functions
    that never run, add a `swb-js` option that counts first calls.
  - The heap accounting of a script's code charges two bytes per source
    unit, more than `SourceText` uses. The real size moved a collection
    and made unrelated rows 13 to 25 % slower (same instruction counts);
    change it together with the memory budget of the engine
    integration.
  - A captured binding whose references all lost the dead-zone check
    still stores the marker at each scope entry.
  - `for`-`of` and spread over a plain array could use the record of
    array patterns and skip the Array Iterator (one allocation per
    loop). An array rest over an array with holes copies per element.
  - The cache of array patterns keeps one array shape. Patterns that
    alternate between two shapes (a pair and a `RegExp` match result)
    walk the prototypes at each use: 265 ms for 1M iterations against
    93 ms with one shape (710 ms at 13d). Two entries or a check that
    does not depend on the shape would fix it.
  - Layout noise: rows move by up to 7 % between builds that differ in
    code they do not run, with equal instruction counts. Try function
    alignment or `codegen-units = 1` in the release profile.
  - `language_hostile::copies_of_huge_sparse_arrays_end_at_a_limit` is
    flaky in single runs: `a.push.apply(a, a)` can reach "Maximum call
    stack size exceeded" before the deadline, and `is_limit` does not
    accept that `RangeError`.
- Maintenance (M7):
  - Left for measurement (hot paths): the line terminator test of the
    regexp matcher (`js-regexp/src/exec.rs`) and the digit value of a
    code unit (five copies: the lexer, the number parser, the regexp
    parser, JSON, URI) could use the `js-text` functions; the
    PromiseResolve sequence of `await` is in five places.
  - ADR 0026 has 3,400 lines; sections 14 and 15 are the record of
    the spike and of M7. A split needs the references by section number
    (roadmap, devlog, code comments) updated.
  - Long files: `bytecode/mod.rs` 2,108 lines (`Insn` and
    `FunctionCode`), `compiler/expr.rs` 1,421.
  - `general_category_name` and `ScopeTree::dynamic_envs` have only
    test users.
- `[[VarNames]]` (M7 3f, older): the global environment does not record
  var names, so a later `let` of a name whose configurable property
  existed before a `var` (or a B.3.2.2 var) is allowed; the spec throws
  a `SyntaxError`.
- test262 groups `expressions/comma` and `expressions/concatenation`
  meet the rule for `dir` lines and are not in the subset.
- Time charges of closures (M7 3e, older than M7): creating a closure
  charges no step per capture. A loop that creates a closure with
  60,000 captures ran 1.85 s past a 100 ms limit. Fix together with
  the prototype-chain charges above (one step per 64 captures is
  enough).
- Global declarations (M7 3e): GlobalDeclarationInstantiation throws a
  `SyntaxError` (V8's choice) where test262 wants a `TypeError`
  (`global-code/script-decl-func-err-non-*.js`).
- `null[key] op= v` converts the key before the null check (M7 3e,
  `compound-assignment/S11.13.2_A7.*_T1`, about 30 tests).
- The parse of scripts (`Runtime::compile`) has no heap check before
  the tree exists; eval code has one since M7 3e.
- Stack traces (M7 3e): V8 shows built-in frames (`at eval
  (<anonymous>)`, `at Array.map`); swb does not (since 2c).
- Stack traces (M7 3c): a static method of an anonymous class is
  `Function.m` in V8 and `m` in swb.
- Termination (M7 3d): after a heap-limit termination, the runtime
  cannot run `var keep = undefined;` to release the heap (each later
  script terminates again). The host needs a way to drop a document's
  globals; decide in the integration ADR.
- `cargo doc` warns about intra-doc links in `js/src/object/ops.rs`
  (`[[Get]]`, `[[Set]]`) and a private link in `js-syntax/src/dump.rs`.
- Messages (M7 3c): "Cannot assign to read only property 'y' of object
  '#<Object>'" names every ordinary object `#<Object>`; V8 uses the
  constructor's name for instances of classes and functions (`#<K>`).
- Numbers (M7 4): the last bits of `Math.pow` and `**` (V8 has its own
  pow) and of the transcendental functions (the platform's libm). If a
  target depends on V8's results, write own code from the published
  algorithms; it also gives the same results on all platforms.
- Numbers (M7 4): `toFixed` computes on multi-precision integers for
  each call (about 290 ns); a fast path for few digits (feature 13).
  `Math.random()` costs about 185 ns a call (`node --jitless` 16 ns),
  most of it outside the generator. `toString(radix)` on the
  multi-precision path clones a number for each digit (`MAX_VALUE` in
  radix 2: 7 µs).
- Numbers (M7 4): the lexer gives the digits of a decimal literal to
  Rust's float parser as one string, so a literal of 16 million digits
  builds a 16 MB string; `js_text::number` parses without the copy.
- Heap policy (M7 5a): the string arena keeps free slots in the middle
  and counts them as live, and the collection threshold is twice the
  live size. A workload that makes many small garbage strings keeps a
  slot vector of several times its live data (2 million appends of
  `'ab'` keep 7.3 MB after a collection for 4 MB of text). Compact the
  free list or leave free slots out of the threshold.
- Time charges of string comparisons (M7 5a, older): `strictly_equal`
  and `compare_values` charge no time for long strings, flat or rope.
  One comparison of two ropes of 2^28 units runs 0.1 to 0.5 s past the
  deadline.
- Index reads of ropes (M7 5a): `s[i]` flattens with
  `flatten_no_gc`, because `string_own_property` has callers that do
  not root the string, so it cannot collect first. After 2 million
  appends of `'ab'`, `s[5]` ends at a heap limit of 13 MiB, where
  `parseInt(s)` (which collects) runs. Make the element read of the
  interpreter a safepoint, with the base in its register.
- Strings (M7 5b): `split` is 26 times slower than `node --jitless`,
  because each piece goes through the general property definition; a
  bulk append to a dense array for natives (feature 6). `for`-`of` over
  a string calls the generic `next` (a compiled path as for arrays), and
  each string of one code unit is a new allocation (a table of the 256
  Latin-1 ones; feature 13).
- Messages (M7 5b): the message for an incompatible receiver shows
  `#<Object>` where V8 shows `[object Array Iterator]` (the object text
  does not read `Symbol.toStringTag`).
- Numbers and JSON (M7 5c): `number_to_string` takes three formatting
  passes; `json_stringify.rs` has a faster path for numbers of at most
  15 digits (`short_decimal`). Move it into `js-text` and use it for
  `Number.prototype.toString` too (about 3 times faster for prices and
  coordinates); `POWERS_OF_TEN` is in both `json_parse.rs` and
  `json_stringify.rs`.
- JSON speed (M7 5c): `stringify` finds each member by its key (about
  10 % of its time; a snapshot of keys and slots while the shape is
  unchanged); `parse` spends most of its time on allocation (a `Vec`
  per object and per string) and copies the source text out of the
  heap once.
- Messages (M7 5c): `object_text` shows `[object Object]` for an object
  whose prototype is `null`; V8 also does so for an object whose
  prototype chain has no constructor
  (`Object.create(Object.create(null))`). Keys and sources in JSON
  messages are clipped at 1,024 units; V8's limit is not measured.
- `localeCompare` (M7 5d): a small table of alphabet positions for the
  extended Cyrillic letters (Ђ, Ѓ, Ї and others; ICU puts Ђ after д)
  and for other scripts, which now go by code point. `locales` and
  `options` (`sensitivity`, `numeric`) need `Intl.Collator`.
- Case functions (M7 5d): `first_change` decodes every code point of a
  wide string; a skip over runs of ASCII would speed up long wide
  strings.
- `encodeURI` checks the room for the worst case of the rest of its
  input, so near the length limit it throws "Invalid string length" up
  to about 36 KiB early (M7 5b).
- Bound function names (M7 2a open item): the 32,768-unit limit stops
  the 5,462nd `bind` of a `bind` result with "Invalid string length";
  Chromium 148 binds 100,000 times (measured 2026-10-16). Without the
  limit, the names are ropes, but the rebalance at depth 256 gives each
  rebalanced name its own flat copy, so 100,000 binds need 128 to 256 MiB
  (O(n²/256) code units). A rope form that shares a repeated prefix, or
  a name made when it is first read, would remove the limit.
- `shift`, `unshift` and `splice(0, 1)` on a large dense array move the
  whole vector, at about 0.8 ns per element: a queue of 100,000 elements
  emptied with `shift` takes 4.1 s (`node --jitless` 0.59 s). A start
  offset in the dense vector would make `shift` O(1); breadth-first
  queues on pages use this pattern (M7 6a review; feature 13).
- `Map` and `Set` (M7 6b): a string key costs about 100 ns per
  operation, about 30 ns of it for the hash; a hash cached in
  `JsString` would save that at 8 bytes per string. Array destructuring
  (`for (const [k, v] of map)`) costs about 300 ns per pair, so that loop
  is 5 times slower than `node --jitless`; a fast path for a dense array
  with the built-in Array Iterator (feature 13). A table of 1 to 4
  entries costs about 300 bytes. `console.log` of Map and Set iterators
  shows them as ordinary objects (Node.js: `[Map Iterator] { 1, 2 }`),
  and a Map without a prototype as `Map(1) {…}` (Node.js:
  `[Map(1): null prototype] {…}`). A table of 2^24 entries with one
  deleted grows its index to 2^26 slots on the next insert, and with the
  default heap limit of 1 GiB that ends the script (Chromium adds the
  entry).
- Iterator helpers (M7 6c): a helper over a user iterator costs about
  480 ns per element (the call of `next`, the result object, two reads;
  feature 13). A chain of helpers costs about 1 KiB of stack per level;
  smaller frames on that path would allow chains deeper than 2,000.
  `console.log` does not show the `@@toStringTag` of a prototype
  (Node.js: `Object [Iterator Helper] {}`).
- `join` and `String` of arrays nested 500 deep end with a `RangeError`
  (400 work), because each level re-enters the interpreter through
  `ToPrimitive`, `toString` and `join`; V8 handles 1,000 (M7 6a).
- No-GC paths near the heap limit (M7 6a): the rope rebalance and other
  paths reserve with `check_room` and cannot collect, so near the limit
  they could end the script while garbage remains. Since 6a a safepoint
  also collects near the limit, which hides the case; a safepoint
  before the rebalance would remove it.
- Built-ins of Chromium 148 that no M7 feature names (found 2026-10-16):
  explicit resource management (`using`, `await using`,
  `DisposableStack`, `AsyncDisposableStack`, `SuppressedError`,
  `Symbol.dispose`, `Symbol.asyncDispose`,
  `Iterator.prototype[Symbol.dispose]`; test262 feature
  `explicit-resource-management`) and `Error.isError`. Add them when a
  target uses them, or after M7.
- Generators (M7 7a): a `for`-`of` step over a generator with the
  built-in `next` makes a result object and reads `done` and `value`,
  about 340 ns per step; the loop could take the value directly
  (feature 13). Chromium ends a recursion of 5,000 generators through
  `yield*` with a `RangeError`; swb runs it, because its frame limit is
  10,000.
- Heap accounting (found in M7 7a): `live_bytes` counts each arena's
  slot vector with its whole capacity, so one live string at a high slot
  after a burst of garbage can raise the collection threshold up to
  10 times. The `toLowerCase` row of `vm_bench` has 1 or 7 collections
  by this effect, and its time varies by about 8 %. With a heap limit
  of 64 MiB, 240,000 objects that become garbage stop a later
  allocation of 12 arrays of 250,000 elements, which fits without them
  (M7 7b review).
- Promises (M7 7b):
  - An executor runs through a re-entry of the interpreter (10 KiB of
    the recursion budget), so `new Promise` nested in executors settles
    about 400 levels; Chromium 148 settles 3,000. Deeper levels reject
    with a `RangeError`. A frame-based call of the executor would remove
    the limit.
  - V8 names `Object.create(Promise.prototype)` `#<Promise>` in
    messages, by the constructor on the prototype chain; swb says
    `#<Object>`. Check the rule on more receivers (class instances)
    before a change of `object_text`.
  - Speed: 7 to 9 times slower than `node --jitless`, mostly the native
    call from script, closures with a `prototype` and the job. A
    `Promise.all` element takes about 450 bytes at the peak, 3 times
    Node.js (feature 13).
  - test262 `built-ins/Promise/try/avoids-wrap*.js` test a 2026 change
    that Chromium 148 does not have; check again when Chromium has it.
  - For the engine integration ADR: the tracker reports only "unhandled
    at take"; there is no `rejectionhandled` for a handler added after
    the report and no `unhandledrejection` event. The realm of a job is
    stored but not used (one realm). The incumbent settings object of
    HostMakeJobCallback is not modeled.
- Async functions (M7 7c):
  - Stack traces follow the `await` of an async function, but not
    `Promise.all`: V8's line `at async Promise.all (index N)` is
    missing. They also stop at a `then` and at `Promise.race`: for
    `await Promise.race([a()])`, Chromium shows `at a`, `at async b`;
    swb shows only `at a` (7c review).
  - Speed: 3.6 to 6.7 times slower than `node --jitless` (100k `await`s
    18 ms, Node.js 5 ms; feature 13).
  - For the engine integration ADR: a termination at the tick inside
    the resume of an async function drops that job, so the function
    stays suspended for good.
- Async iteration (M7 7d):
  - Messages of `for await` for operands that swb does not render
    (`{}`, `1 + 2`, `(0, 5)`): V8 renders the expression; swb says
    `undefined is not a function`.
  - `%AsyncIteratorPrototype%[Symbol.asyncDispose]` (Chromium 148) waits
    for explicit resource management (see above).
  - Speed: 5.5 to 9.3 times slower than `node --jitless` (100k values
    through `for await` over an async generator 81 ms, Node.js 15 ms):
    two jobs and 3 to 4 allocations per step (feature 13).
- Regular expressions (M7 8a):
  - `Matcher::new` allocates the capture and counter vectors for each
    match; the global loops of 8b (`replace`, `matchAll`, `split`) could
    reuse them.
  - Each plain character is a tree node, so a pattern of more than 4M
    characters is "too large" (V8 accepts at least 1M). Runs of plain
    characters could be one node.
  - An alternation of single characters (`a|b|c`) could compile to one
    set (a million alternatives give 3M instructions).
  - Nesting stops at 32,768 levels; V8 accepts 1,000,000 nested `(?:`.
  - No memoization and no linear-time fallback (ADR 0026 section 10).
  - Messages: a `-` right after a range in a `v` class gives "Invalid
    character in character class"; Chromium gives "Invalid character
    class" (`[a-b-c]`, `[a-b-\w]`) or, at the end of the pattern,
    "Unterminated character class" (`[a-b-`). Both reject. Add the
    cases and the modifier cases of ADR 0026 section 15 to the probe
    file.
  - The case closure of a class with `i` costs about 40 to 70 µs when
    the class is not in the cache (more than 16 ranges, or a new set):
    30,000 such classes take about 1.3 s to compile
    (docs/performance.md).
- The `RegExp` built-in (M7 8b):
  - Speed against `node --jitless`: `matchAll` 16x and literal
    evaluation 7.8x slower. Iterator results and exec results use the
    general object creation; a shaped iterator result for all built-in
    iterators would help (feature 13). One-character strings are
    allocated each time (V8 keeps them): `match` with `g` 7x.
  - `new RegExp(str)` in a loop compiles each time; V8 keeps a cache by
    source and flags.
  - The matcher uses 32 bytes per choice point, and `(?:a|b)*` takes two
    per iteration: the `RangeError` comes at about 4M iterations
    (Chromium about 10M), and earlier under a small heap limit. A
    smaller choice point, or one per iteration, would match Chromium
    without a larger bound.
  - Error messages that quote a pattern with lone surrogates show
    U+FFFD (the messages are Rust strings).
  - V8 rejects a literal of 100,000 characters as "Regular expression
    too large"; swb compiles it.
  - `console.log` of a RegExp String Iterator and of other built-in
    iterators prints `{}`; Node.js prints `Object [RegExp String
    Iterator] {}`.
  - `replace` and `split` with a non-callable `Symbol.replace` or
    `Symbol.split` give the generic message; measure Chromium's.
  - A call of `next` on a RegExp String Iterator from inside its own
    `exec` runs; not measured in V8.
  - The realm checks of the legacy RegExp features proposal are not
    modelled (V8 does not have them; no cross-realm tests in the
    runner).
  - Not RegExp, seen in the 8b review and not checked: after a
    collection, 300 strings made by `'a'.repeat(1000) + i` still counted
    235 KB (about 0.8 KB each). The note does not say if the strings
    were still reachable. Repeat the case and find out what the count
    holds.
- Unicode property sets (M7 8c):
  - The lone General_Category value with `v` and `i` (ADR 0026 section
    15): check whether the spec text is a known editorial error, and
    follow the text if it is changed.
  - The emoji ZWJ sequences take 25 KB of the 76 KB of data; a small
    dictionary for U+200D and U+FE0F would save about 7 KB.
  - A class such as `[\p{L}]` costs about 10 µs (union and hash); a
    cache for one-item classes would skip that.
  - A class with a property of strings, such as `[\p{RGI_Emoji}x]`,
    builds its strings in a `BTreeSet<Vec<u32>>`, about 1 ms per
    occurrence; a sorted vector would be faster.
  - The trie of a string list is built again for each occurrence (about
    0.3 ms for `\p{RGI_Emoji}`); a cache per list would skip that.
- Proxy (M7 9a):
  - Destructuring: the compiler resolves a keyed `var` binding target
    after `GetV`; the spec resolves the binding first (test262
    `keyed-destructuring-property-reference-target-evaluation-order-with-bindings.js`,
    which a `with` proxy makes observable).
  - A global read through a proxy on the prototype chain of the global
    object runs the traps without saving the pc, so the stack positions
    of errors in those traps are wrong.
  - A `construct` trap that is not callable: V8's message is "is not a
    function", swb's "is not a constructor".
  - A sloppy assignment to an undeclared name skips the `has` trap of a
    proxy on the prototype chain of the global object
    (`vm/global.rs`); ResolveBinding calls HasProperty first, and the
    strict path does.
  - A chain of about 2,000 proxies, or 1,500 with a script `get` trap,
    gives the `RangeError`. Measure Chromium's limit.
  - Speed: proxy operations are 5 to 8 times slower than in
    `node --jitless`. Possible: a cached "no trap" per handler shape, no
    handle scope per read.
  - `vm_bench` runs all programs in one runtime, so a change in the
    number of objects of a realm moves collections between rows (the
    `exec` row). Consider a new runtime per program.
  - Remove `Proxy` from the EXCLUDED lists of the older probe generators
    at their next re-probe (1 snippet in `js-arrays.json`).
- Weak collections (M7 9b):
  - The host asks `Runtime::pending_finalization_cleanups` after its
    tasks (polling); the host-hook trait of ADR 0026 section 12 has no
    "schedule finalization cleanup" yet. The integration ADR decides.
    The host must also call `run_jobs` or `clear_kept_objects` after a
    script or a job that threw.
  - The per-collection cost of a weak table or a registry is linear in
    its size, also when nothing in it died; a generational or
    incremental collector needs remembered sets for weak tables.
  - The collector's memory for the ephemerons (the waiting lists, the
    bit vectors) is allocated without the heap-limit check. It is at
    most proportional to the entries with unmarked keys.
  - `heap_bench`: the pause for 1M reachable objects is about 5 % slower
    in wall time (16.9 → 17.7 ms) with 7.5 % fewer instructions. Measure
    cache misses if it persists.
  - `console.log`: Node prints an object with no entries in full at any
    depth (`{ a: { b: { c: {} } } }`, `MM(0) [Map] {}`); swb prints
    `[Object]` and `[Map]` past depth 2 (done for `WeakRef` and
    `FinalizationRegistry` only).
  - Spread or `for`-`of` over `new WeakSet()` or `new WeakMap()`:
    Chromium 148's `TypeError` names the expression (`WeakSet is not a
    function or its return value is not iterable`); swb gives its
    generic message. Check other `new X()` expressions.
  - Remove `WeakMap` from the EXCLUDED lists of `gen-proxy-cases.js`,
    `gen-collection-cases.js` and `gen-iterator-cases.js` (no snippet
    there uses it).
- BigInt arithmetic (M7 10a):
  - `toString` and the decimal parse are quadratic. A divide-and-conquer
    `toString` (split by powers of the radix) would be about 4.5 times
    faster and would allow a limit of 2^19 bits.
  - No Karatsuba multiplication (a product at the limit takes 3 ms;
    V8 is faster from about 100 limbs on).
  - `from_f64` of a double from 2^53 to 2^64 and the left shifts reserve
    one limb more than the result needs.
- BigInt in the language (M7 10b):
  - An operation on one-limb values costs about 70 ns (the
    multiply-add-modulo row of `vm_bench`: 280 ms, `node --jitless`
    80 ms). The library grows its result vectors
    (`RawVec::finish_grow`); with `malloc` and `free` that is about a
    third of the instructions. Size the result before the work.
  - The compiler's decimal text of a literal key and the size check of
    long literals are quadratic and not charged: 50 keys of 78,000
    digits (3.9 MB of source) take 1.9 s to compile. The time limit does
    not cover the compiler.
  - The callee text of a call with a computed key that is not a literal
    (`o[Symbol.iterator]()`): V8 shows the key expression, swb
    `(intermediate value)`. Not specific to `BigInt`.
  - The shell prints an uncaught `5n` as `Uncaught 5` (Node: `5n`).
  - `console.log` ignores format specifiers (`%d`, `%s`, `%o`), which
    Node applies. Not specific to `BigInt`.
  - The longer messages of an array `length` write come from a match on
    the message text (`length_write_message`); an error kind would be
    more robust.
  - Garbage-heavy loops under the default heap limit of 1 GiB (the heap
    accounting item of M7 7a): 12 million temporary objects reach
    311 MiB RSS, 12 million strings 554 MiB, 3 million `BigInt` steps
    242 MiB. A collection is not interrupted at the deadline: a loop
    that keeps rejected promises ends up to 170 ms after a 600 ms
    `--time-limit`, also with Numbers.
- Binary data (M7 11a):
  - A `DataView` get or set costs about 85 ns (1M `getInt32` reads:
    85 ms after 13b, `node --jitless` 23 ms). The call is about 100
    instructions now; the body about 300 (`ToIndex`, two lookups of the
    buffer, a generic decode). One buffer lookup and a decode per
    element type would cut it.
  - A new buffer is zero-filled at once, so every page is touched: 40
    buffers of 64 MiB take 271 ms (Node 21 ms, with lazy zero pages).
    std has no fallible `calloc`; a chunked lazy zero-fill would help.
  - `resize` grows the vector to the exact length, so a loop of
    `resize(length + 1)` copies each time (charged). Grow by a factor,
    capped at `maxByteLength`.
  - The RegExp and Promise species code can use the generic
    `Runtime::species_constructor`.
  - NaN from `0/0` has the hardware bits (0xFFF8…); V8 gives 0x7FF8….
    The spec leaves NaN bits to the implementation (KNOWN).
- Typed arrays (M7 11b):
  - `ta.length` walks two prototypes to its accessor on each read (the
    call itself is skipped): the Float64Array sum row takes 178 ms, the
    `Array` loop 127 ms. Inline caches, feature 13.
  - Spread and `Array.from` of a typed array take one iterator step per
    element; a bulk path in `spread_array_iterator` would help. The
    generic `Array.prototype` methods take the per-index steps on typed
    arrays.
  - The heap's `get_own_property` of a `BigInt` element is an internal
    error (it cannot allocate behind `&self`); all runtime paths go
    through `Runtime::own_property`. A heap read API that may allocate
    would remove the trap.
  - `Array.from.call(Uint8Array, [5, 6])` throws "Invalid typed array
    index" (the message of `Object.defineProperty`); Chromium says
    "Cannot redefine property: 0" for `CreateDataPropertyOrThrow`. Check
    class fields too.
  - Not specific to typed arrays: `console.log([].values())` prints `{}`
    (Node.js: `Object [Array Iterator] {}`); the message of `null[obj]`
    contains the key text (V8 leaves it out); a strict write to a
    non-extensible object whose prototype has a writable property says
    "Cannot assign to read only property" (V8: "Cannot add property x,
    object is not extensible").
- Typed-array methods (M7 11c):
  - The callback methods (`map`, `forEach`, `filter`, ...) cost one call
    of the interpreter per element: `map` over 1M elements takes 104 ms,
    `node --jitless` 8 ms. Feature 13 (native callbacks).
  - The default sort of 4- and 8-byte types keeps 2n keys of the
    element width (the keys and the merge buffer). An in-place merge
    would halve it.
  - Memory outside the heap (the sort keys, the list of `filter`, the
    decoder output) is checked with `reserve_buffer` but not counted
    while it lives, so two such buffers alive at once can exceed the
    limit by their sizes (the allocations use `try_reserve`, so no
    abort). The output of `fromBase64` and `fromHex` grows by doubling
    and can end up to twice the decoded size (inside the limit check).
  - V8's object text in messages (`NoSideEffectsToString`) differs from
    `object_text` (vm/messages.rs) for objects with their own `toString`,
    instances of anonymous constructors and length-tracking typed arrays
    (KNOWN in gen-typed-cases.js).
  - Intl: V8 passes `(locales, options)` to the element's
    `toLocaleString` (ECMA-402); swb passes no arguments, as for arrays.
  - Test data: the 11c cases are in the repository three times
    (`typed-method-snippets.txt` with the Chromium results, the derived
    `typed_methods_cases.js` and `tools/probes/js-typed.json`). The test
    can build the script from the snippets file (0.6 MB less). The
    Python generator of the snippets file is not in the repository; the
    snippets file is the source now.
- Date (M7 12a):
  - `Intl`: `toLocaleDateString("en-GB", {day: "numeric", month:
    "short", year: "numeric", timeZone: "UTC"})` and similar calls in
    the BBC scripts, and `Intl.DateTimeFormat().resolvedOptions()
    .timeZone`. Without `Intl`, swb ignores the arguments and prints
    the en-US form (12/31/2020; Chromium 31/12/2020 for en-GB).
  - The KNOWN items where swb follows the spec and pages see V8:
    fractional `getTimezoneOffset` for old local mean times, the
    `toString` round trip for years below 100. (12b reads
    `"2000-02-30"` as March 1, as V8 does.)
  - The zone-dependent snippets use instants after 2026 (2100, 2400,
    1e14, the `realworld` loop to 2040): a tzdata change of the future
    rules of New York, Berlin, Adelaide or Lord Howe would break the
    test although swb is right. Move them to a `future` group that runs
    in UTC, Kolkata, Apia and New York only, with a comment on the
    tzdata-sensitive groups. The tests need the system tzdata with the
    22 zones of 12b.
  - Test data: the seven expected files repeat the same snippets
    (450 KB each); the `setters` group is half of the snippet bytes,
    and running it in four zones instead of seven cuts about 20 %.
  - `toString` (1M calls 593 ms after 12b, which took the
    allocation of the name away; a zone without a CLDR name still
    builds `GMT+01:00` for each call). `Date.now()` is 3.3 times and
    `new Date(t).getFullYear()` 2.7 times slower than `node --jitless`
    (call and allocation costs, feature 13).
  - The fallback to UTC without a zone database has no test, and the
    shell installs no logger, so its warning is not visible there.
- Property caches (M7 13a):
  - Not cached: receivers in dictionary mode (`window.foo` on a global
    object in dictionary mode, objects with more than 128 keys),
    primitive receivers (`s.length`, `s.charCodeAt`, `n.toFixed`),
    `TypeofGlobal` and `DeleteGlobal`; no polymorphic entries (add them
    only if the target scripts show many sites with two to four shapes).
    `DefineNamed` of literals with computed keys or spread could use the
    add entry.
  - A write through a proxy with a `set` trap runs three lookups (2.8 %
    more instructions than before 13a).
- Heap (found in 13a): objects that a script allocates past the end of a
  long slot vector before the first collection keep the capacity of the
  vector counted as live, so a large reservation after a burst of
  garbage fails early (`typed_hostile.rs`
  `endless_sources_end_at_the_deadline` now collects before each
  source). The effect is older, but 13a makes more garbage in the same
  time: without the collection the test fails in every debug run (an
  800 MB `BigInt64Array` gives "Array buffer allocation failed"). A page
  can see it as a failed allocation of hundreds of MB after a burst of
  garbage. Options: allocate from the free list only after a
  collection, or compact the tail when a reservation fails.
- Date (M7 12b):
  - The `toString` parser of 12a reads any name in parentheses that
    ends with `)`; Chromium needs balanced parentheses
    (`(Eastern Sta)ndard Time)` is NaN there).
  - Chromium's quirks that the named forms do not read (KNOWN, ADR 0027
    section 8): a number alone as a month or a year (`Date.parse("5")`,
    `new Date("5")`), minute or second 60, offsets above 23:59, `h:`
    clocks, stray tokens after a date. Add a form only when a target
    needs it.
  - The end of the equivalent-year range (2^31 seconds) is not proved by
    a probe: no zone with year-dependent rules changes near 2038-01-19.
  - The names depend on the system tzdata: where its offsets differ from
    ICU's data (local mean times, `Africa/Casablanca` 1984 and 1985,
    `America/Vancouver`), the names differ too.
  - Alias zone names (`US/Eastern`, `Asia/Calcutta`, `GB`, `Japan`,
    `Europe/Kiev`) are unknown to `TimeZone::from_spec` when the system
    tzdata has no legacy links (Debian 13 without `tzdata-legacy`);
    Chromium knows them. The CLDR alias table of `zone_names_data.rs`
    could map them to a canonical name first.
  - Chromium reads more free text than swb: a word after a month name
    and before the first number (`Jul T 1 2024`), `_` and `[` as word
    characters (`_ Jul 1 2024`), a sign after free text as nothing
    (` -000001 01`). All are NaN in swb (KNOWN class).
- Flaky test (found 2026-10-18, also at 788905a):
  `long_digit_strings_parse_in_bounded_memory_and_time` in
  `language_hostile.rs` fails about 1 run in 6 when the whole file runs
  (its tests in parallel): a `while (true) parseInt(d)` or
  `parseFloat(z)` loop over 16M units ends 213 to 243 ms after start,
  against a 100 ms deadline plus 100 ms slack. Alone it passes. The
  cause is not checked yet (the length of one call in the dev profile
  under load, or the granularity of the step charge). The 12a review
  saw the same with `time_limit_object_keys_of_a_big_string_object` in
  `vm_limits.rs` (a 250 ms bound) at a load average of 12; it passes
  alone and with `--test-threads=1`. The 13b session saw
  `long_strings_to_parse` in `date_hostile.rs`
  (`Date.parse('x'.repeat(100_000_000))`) reach its deadline while
  test262 ran on all CPUs.
- Dependencies (2026-10-18): `ttf-parser` is unmaintained
  (RUSTSEC-2026-0192, informational; no known vulnerability). swb does
  not use it; winit's Wayland title bar (`sctk-adwaita`, through
  `ab_glyph`) does. Options: a winit release without it, or winit
  without the feature `wayland-csd-adwaita` (then a compositor without
  server-side decorations, such as GNOME, shows no title bar). `just
  check` runs the offline subset of `cargo deny`; run `just audit` at
  each milestone end.

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
