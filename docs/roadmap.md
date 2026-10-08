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

## M4: Ars Technica — in progress

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
   a link follows the link), limits and hostile-page cases.
7. Final pass on the page (`implementer`, scope from a new comparison):
   the largest remaining differences, links, hover states and the search
   form; then report target 4 as done.

Not in scope (not visible on the page with JavaScript off, or not
used): `box-shadow`, `filter`, `text-shadow`, `-webkit-line-clamp`,
`backdrop-filter`. Style takes 27 ms on this page (Wikipedia: 10 ms for
twice the elements); look at it in the final pass only if it grows.

## M5: BBC — planned

Target 5, `https://www.bbc.com/` (fixture `bbc`), JavaScript off.
Baseline on 2026-10-08: geometry 0.0290, pixels 0.9583 (2,745
elements). Most differences are web fonts (BBC Reith) and inline SVG
(logo, icons), which M4 adds. Known items for M5, to plan after M4:

- Closed `<details>`: Chromium reports boxes for the content
  (`::details-content` with `content-visibility: hidden`); swb has none
  (229 missing elements in the no-JavaScript menu).
- `:not()` with complex selectors (`a:not(.x a)::before`), `quotes`,
  `::-webkit-scrollbar` (Chromium accepts it, so the rule stays valid).

## Pending decisions

- Clean-room rewrites of the code derived from Chromium (table layout
  column constraints, width distribution, row heights; grid track
  sizing; sticky offsets; two float helpers), Skia data and Servo
  (`collapsed_margin.rs`, MPL-2.0); kept with attribution for now
  (ADR 0021).

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
  that goes away, but keeps its entry. Reuse or remove the entries.
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
  Chromium lets `r: 40px` override the attribute), `shape-rendering`
  (BBC sets `crispEdges` on 120 icon paths; swb anti-aliases them),
  nested `svg` and `symbol`, gradients and patterns (`url()` paints use
  their fallback), `text`, `image`, markers, `paint-order`,
  `vector-effect`, `context-fill`, the `miter-clip` and `arcs` joins.
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
  `justify-items: legacy`; `safe` and `unsafe` (parsed, ignored); absolutely positioned
  items placed by grid lines; fragmentation; `display: grid` on
  `<button>`; subgrid; masonry. The intrinsic pass resolves a
  percentage `height` against an indefinite size, so `repeat(auto-fill)`
  rows can get another count there.
- Paint: `z-index` on non-positioned flex and grid items does not create
  a stacking context.
- Layout: block layout ignores `width: min-content | max-content |
  fit-content` outside flex and grid items.
- Layout, `aspect-ratio` on boxes that are not replaced: flex and grid
  containers do not grow to the height of their content (`min-height:
  auto`, CSS Sizing 4 §5.1.1); `display: table` ignores the ratio; grid
  items that stretch in the block axis do not pass the stretched height as
  a transferred width contribution to the column sizing (Chromium sizes
  `1fr 2fr` columns with ratio items differently). `vertical-align: top` and `bottom` on atomic inlines
  act as `baseline` (known failure `vertical-align-top-bottom`).
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
- Paint: `border-radius` does not clip background images, gradients,
  replaced images or overflow;
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
- JavaScript (needs the owner's decision first, see ground rules).
