# Development log

Newest entries first. One entry per working session or milestone. Record what
was done, what was learned, and what is next. Keep entries short; details go
in commit messages, ADRs and other docs.

## 2026-10-09: final pass on BBC (M5 item 4)

- The text pixel differences on all pages were glyph edge coverage, not
  positions: swb and Chromium use the same advances and quarter-pixel
  offsets, but tiny-skia resolves edges to 1/4 px. Glyph masks that the
  cache keeps are now filled at 4 times the size and averaged down (ADR
  0006 update). Review: large glyphs paid 16 times the cost on every
  use; they are now filled directly, with the hostile case
  `huge-glyphs`.
- Single images are scaled into their pixel-snapped rectangle (the 1–2 px
  lines at the edges of BBC images).
- Engine tests for the BBC links and `:hover` rules; no bug found.
- Scores: BBC pixels 0.9987 (full page 0.9987), Hacker News 0.9988,
  senko.net 0.9989. 148 snapshot images change.
- Target 5 is done. Next: the M5 maintenance.

## 2026-10-09: grid buttons, `scrollbar-width` (M5 item 3)

- `<button>` with `display: grid` or `inline-grid` lays out its content
  as a grid. Measured: Chromium centers only flow content of a button
  vertically; flex and grid content is laid out as in a plain container
  (stretch, `align-content`). swb centered flex content before; fixed.
- `scrollbar-width: auto | thin | none`; `none` hides the GUI scroll
  indicator (the BBC carousels). The viewport follows the root element.
- BBC geometry 0.9660 → 1.0000. Other fixtures unchanged.
- Review: no bugs. A release-only stack overflow in the `deep_nesting`
  test exists on main too (backlog).
- Next: M5 item 4, the final pass.

## 2026-10-09: `details` and `summary` (M5 item 2)

- The content of a closed `details` is laid out in a `::details-content`
  box with size, layout and paint containment, as Chromium's
  `content-visibility: hidden`. After layout the boxes move to
  `BoxFragment::hidden`: the box dump reports them, paint, hit testing,
  selection, Tab and scrolling do not see them.
- A click, Enter or Space on the first `summary` toggles `open` (not a
  click on a link or field in it). A `details` without `summary` gets the
  default summary ("Details").
- Measured: inside disclosure markers are 1.0592 em wide and keep the
  line height.
- Review: no blocking bugs; closing a `details` now also removes a
  selection in its content. The other findings went to the backlog.
- BBC geometry 0.8786 → 0.9660, no missing boxes; the open menu matches
  Chromium. Other fixtures unchanged.
- Next: M5 item 3, grid buttons and `scrollbar-width`.

## 2026-10-09: M5 plan, full-page and state comparison (M5 item 1)

- The owner accepted target 4 (Ars Technica).
- M5 plan (BBC): after M4 the page has geometry 0.8786 and pixels 0.9985.
  What remains: the content of the closed `details` menu (229 missing
  boxes) and its toggle, `display: grid` on `<button>` in the off-screen
  drawer, `scrollbar-width`, and text pixels. `quotes`, complex `:not()`
  and `::-webkit-scrollbar` from the old list are not needed.
- `compare --full-page` compares full-page screenshots (Chromium live,
  swb `--full-page`) with a score over the union of both areas and a
  list of differing regions; `compare --click SELECTOR` compares the
  state after clicks in both browsers (Playwright; swb's automation
  API). The full-page arrays are uint8 and processed in bands of 512
  rows (review: the first version used about 700 MB for BBC).
- Playwright's full-page capture does not change Chromium's layout.
- Full page: BBC 0.9981, Ars 0.9966. No layout difference and no missing
  paint feature below the first viewport. The differences are sub-pixel
  glyph positions inside words (item 4) and 1–2 px lines at the edges
  of scaled images.
- Next: M5 item 2, `details` and `summary`.

## 2026-10-09: M4 maintenance

- Two read-only reviewers (paint and layout; the other crates, tools and
  docs) listed duplication, dead code, names and outdated docs; one
  implementer applied the selected items. `just snapshot` is
  byte-identical before and after (338 files).
- Paint: the path cost model moved from `svg/` to `paint/src/path_cost/`
  (it serves inline SVG and SVG images). One helper makes the dash
  outline for both. Layer pushes take named structs (`NewLayer`,
  `LayerKind`, `SvgClipLayer`). The inline SVG raster tests moved out
  of `raster.rs` (4,257 → 3,662 lines). Module docs list path items,
  SVG clip groups, rounded clips and image reduction.
- Layout: Bezier evaluation is in `layout/src/bezier.rs` (six copies;
  the derivative-root solvers stay apart, because their thresholds
  differ and the cost numbers would change). One circle constant
  (`KAPPA`), one SVG length table, one warn-once counter for the SVG
  limits.
- Other crates: the engine's copy of `element_ids` and a second opacity
  parser are gone; constants that two crates share say so.
- Tools: `reference` and `layout-refs` wait for web fonts like `probe`;
  `substitute` uses the shared MIME parser.
- Docs: README status, roadmap (M4 done), testing.md (`substitute`, font
  waits), ADR 0021 (`wuff` is a library, not an engine port),
  architecture.md (`CountBudget`, module list).
- Backlog: the `font-variation-settings` table is not cleared per
  document; typed web font sources; the performance baseline.

## 2026-10-08: M4 item 9, final pass on Ars Technica

- Four causes behind the last 4,964 differing pixels:
  - The underline bars of the view selector were 14 px too far right.
    The box tree put an absolutely positioned child that follows text
    into the anonymous flex item around that text, so its static
    position came after the icon and the gap. In a flex or grid
    container the child now ends the anonymous item and is a child of
    the container, as in Chromium (layout test `abspos-flex-text`).
  - The round thumbnails use `overflow: hidden` with
    `border-radius: 50%`. Overflow clips now follow the rounded padding
    edge (radii reduced by the border widths), also for deferred
    positioned descendants.
  - The thumbnail grid looked coarser in swb because bilinear sampling
    of a 5× reduction aliases. Raster images drawn at less than half
    their size are now averaged 2×2 until the scale is at least 0.5,
    then sampled bilinearly.
  - Chromium paints inline SVG from the pixel-snapped origin of its
    content box; swb now does too (scale-aware). The logo differs in
    51 px instead of 820.
- The `:hover` rule forms of the page's Tailwind sheet work without a
  change (new engine test). The probe tool cannot hover, so this is not
  compared with Chromium.
- Review fixes (ADR 0024): the averaging first ran at every raster;
  200 large images drawn at 50×50 cost 4.1 s per frame. The reduced
  levels are now kept with the decoded image (at most its own size)
  and made lazily, and one frame averages at most 64 Mpx; 2,000 small
  draws of a 4000×3000 PNG take 0.10 s. A rounded clip is a layer
  group; when the layer budget ran out, 150 nested rounded boxes drew
  nothing. They now fall back to a rectangle clip. Four hostile-page
  cases.
- Ars geometry 0.9963 → 1.0000, pixels 0.9952 → 0.9992. Target 4 is
  done. BBC images below the first viewport are smoother; its scores do
  not change.
- Backlog: the word "LIST" is 0.36 px narrower than in Chromium, which
  shifts the next buttons by a fraction of a pixel.

## 2026-10-08: M4 item 8, raster cost of fills with thin separate spans

- The review of item 7 found that an anti-aliased fill of separate thin
  shapes cost far more than the model charged: a comb of 2,000–4,000
  teeth 0.25 px wide on 1,200 × 780 px took 5–6 s per frame and was
  drawn; the area under a noisy chart of 40,000 points took 1.6 s.
- Measured (tiny-skia public API, 183 new timings): coverage steps in
  quarter pixels in x and y. The extra time comes from spans that lie
  inside one pixel and cover part of it; a span that crosses a pixel
  boundary ends a group. A row costs the inside spans of each group
  times the pixels they touch (2–5.3 ns per pair, at most about 7.7 ms
  per row at 1,200 px). The time grows linearly with the teeth, not with
  their square. Fills without anti-aliasing and strokes are cheap.
- New `paint/src/svg/spans.rs` finds the spans at sample rows (crossings
  in x order, winding under the fill rule), with margins for rounding
  and slope, and charges 6 ns per pair. An O(n) bound from the sweep
  runs first; the count runs only when the bound is large and the count
  fits into the rest of the budget, and its own work is charged. Inline
  fills, clip coverage and SVG images use it (images: counted at 17
  scales; the size search now goes down from the requested size).
- The combs, stacked combs and the area are rejected in 0.05 s. The
  paths of item 7 still draw. Over all timings the real time is at most
  0.88 of the charged time. Five hostile-page cases. Snapshots unchanged.
- Review fixes. Dashes are separate shapes: a dashed stroke wider than
  a pixel is now charged from its outline (tiny-skia's public
  `Path::dash` and `Path::stroke`), counted like a fill. A line 780 px
  wide with 0.2 px dashes took 6 s; it and 15 such lines are rejected in
  0.05–0.15 s, inline and in SVG images. Strokes without dashes need no
  span charge (measured: 800 parallel lines with 0.15 px gaps take
  45 ms). Counting the spans of SVG images has a budget per document
  (2 billion units, 0.6 s): 30 images of a 250,000-segment walk decode
  in 2.2 s instead of 7.8 s. Four more hostile-page cases.
- The item 7 bench read `/proc/thread-self/schedstat`, which on this
  kernel advances in 4 ms steps; the new bench uses
  `CLOCK_THREAD_CPUTIME_ID`.
- Backlog: some fast fills are charged as slow because the model cannot
  see rounding and sub-pixel position (diagonal hatching of 1,000 thin
  filled lines: 106 ms real, rejected; the area of 20,000 points: 0.6 s
  real, rejected). Slanted edges whose spans change between sample rows
  are estimated from the samples, not bounded. Decoding SVG images of
  very many segments costs about 40 ms per 250,000 segments outside the
  counting budget, with no limit per document.

## 2026-10-08: M4 item 7, raster cost of dense paths

- The review of item 6 found pages that take minutes to draw: the path
  cost estimate charged a segment about 9 ns, but one path of 40,000
  curves across 100 rows takes 2.6 s in tiny-skia (the old estimate:
  1 ms).
- Measured (tiny-skia 0.12.0, release build, CPU time of the thread,
  315 timings; tables in ADR 0023 part 3): the fill time fits the edge
  rows (rows that each edge crosses) and the pairs of edges that cross.
  Pairs that overlap in y but do not cross cost almost nothing. Two
  edges can cross only if their bounding boxes overlap, so the model
  counts those pairs.
- New `paint/src/svg/edges.rs` counts rows and box pairs in O(n log n)
  (Fenwick trees over x). A stroke of at most 1 device pixel is a
  hairline: its time follows its length (18–60 ns per pixel), with no
  pair term. Inline fills and strokes, clip coverage and the SVG image
  estimate (ADR 0011; an exact term at the rendered size) use it. Over
  the measurements the real time is at most 1.02 of the charged time,
  except for the known gap below. Hit tests have a work budget (about
  40 ms). `SvgPath::fill_bounds` is cached: each clip reference walked
  all segments.
- A first version counted all pairs that overlap in y. Its review found
  that it rejected realistic paths: noisy line charts of 10,000–40,000
  points, a histogram of 5,000 bars, a random walk of 60,000 segments,
  SVG images of 100 paths of 400 curves. These now draw in 0.06–0.38 s
  (images at 1,000 px at a lower resolution).
- The repro pages take 0.1–0.6 s (before: 22 s to more than 120 s).
  Five hostile-page cases. The target pages do not change; their paths
  use at most 0.1 % of the budget.
- Open: an anti-aliased fill whose edges do not merge into long spans
  is charged 5 to 15 times too little. A comb of 2,000–4,000 separate
  teeth takes 5–6 s per frame and is drawn; the area under a noisy
  chart takes 3.3 s. This is item 8, next. The review of the fix also
  found that one edge far outside the window merged the x columns of
  the pair count, so a chart with a long baseline was rejected; the x
  columns are now cut to the window.
- Backlog: the weights follow the slowest case of each kind, so some
  paths that draw in time are rejected.
- ADR 0021 now also covers ports of the engines it names, such as
  tiny-skia. An agent read tiny-skia's source for the first version of
  this model; no code was copied, and the owner kept the model.

## 2026-10-08: M4 feature 6, inline SVG part 2

- `clip-path: url(#id)` on `g`, `a`, `use` and shapes, as a property or
  a presentation attribute. `clipPath` supports `clipPathUnits`, its own
  `transform`, `clip-rule`, shapes and `use` children, and `clip-path`
  on the clip path (intersection); cycles are cut. As in Chromium, the
  id is looked up in the whole document, and an invalid reference leaves
  the element unclipped. A clip that is one axis-aligned rectangle
  becomes a clip rectangle; it is dropped when it covers the content box
  (all Ars icons). Other clips are a layer multiplied by the coverage of
  the clip shapes (new display item `PushSvgClip`).
- `use` draws a copy of the referenced element whose styles inherit from
  the `use`. The style crate computes these instance trees
  (`style/src/cascade/use_instances.rs`): at most 20,000 elements and
  16 levels per document, no copy for a cycle.
- Box dump: `g`, `a`, `use` and the shapes get their fill bounding boxes
  in page coordinates, as Chromium reports them; `defs`, `clipPath`,
  `title` and elements with `display: none` get none. New layout test
  `tests/layout/inline-svg-boxes.html`.
- Hit testing: a shape is hit by its geometry (`pointer-events`). An
  SVG `a` with `href` is a link with the pointer cursor.
- `shape-rendering: crispEdges` and `optimizeSpeed` draw without
  anti-aliasing (BBC icons).
- Six hostile-page cases (`use` expansion and cycles, clip chains,
  layers and shapes).
- Scores: Ars geometry 0.7978 → 0.9963, no missing boxes; BBC 0.8053 →
  0.8786; pixels unchanged. The 229 boxes still missing on BBC are the
  content of closed `details` (M5).
- Review fixes: the `use` expansion walked `display: none` subtrees
  without counting them (10.4 s → 0.15 s on a test page); clip shapes
  with their own `clip-path` ran full-layer passes that the work budget
  did not count (19.9 s → 0.58 s). A `use` inside a `clipPath` now
  keeps the `clip-path` of its target, and the clipped-out part of a
  shape no longer takes clicks. Two new hostile-page cases.

## 2026-10-08: M4 feature 5, inline SVG part 1

- swb draws inline `<svg>` with its own code (ADR 0023). resvg stays
  for SVG images only. The outer `<svg>` is a replaced box. Its
  `width`/`height` attributes follow Chromium 148 (measured): they act
  as presentation attributes and as the natural size, an invalid value
  is `100%`, and the ratio of the attributes wins over the `viewBox`.
  SVG descendants get no CSS boxes and no text runs.
- Style: the fill and stroke properties, presentation attributes as
  author-level hints with specificity 0 (the `transform` attribute
  through `svgtypes`), two SVG 2 user-agent rules.
- Layout (`layout/src/svg/`): `g`, `a`, `path`, `rect`, `circle`,
  `ellipse`, `line`, `polyline` and `polygon`; path data with swb's own
  arc conversion (SVG 2 implementation notes); `viewBox` and
  `preserveAspectRatio`. Paint: new `FillPath` and `StrokePath` items,
  rasterized with tiny-skia; group opacity as layers; a clip at the
  content box.
- Limits: 50,000 shapes and 1,000,000 path segments per document,
  groups 64 deep, dash arrays of 256 entries, strokes with more than
  100,000 dashes drawn solid, a work budget for paths per strip. Seven
  hostile-page cases.
- The probe tool compares `ink` with swb (`--with-swb`).
  `tools/probes/inline-svg.json` has 16 cases; all sizes match.
- Scores: Ars geometry 0.7740 → 0.7978, pixels 0.9898 → 0.9952; BBC
  geometry 0.7775 → 0.8053, pixels 0.9942 → 0.9985. The count of
  missing boxes went up (Ars 217, BBC 202): Chromium reports boxes for
  `g` and the shapes, which part 2 adds.
- Review fixes: each `<g>` with `opacity` below 1 took a layer, and
  20,000 such groups took 80 s. Now at most 256 layers per document;
  past the limit the group opacity multiplies the alpha of each child
  paint, and the same page takes 1.9 s. A group with one shape and one
  paint needs no layer. Path fills cost about 2.6 ns per pixel, opaque or not,
  so the work budget now counts every pixel as blended. `<style>` in
  SVG is now a document style sheet; as in Chromium, a `type` other
  than empty or `text/css` turns off a `<style>` element, also in HTML.

## 2026-10-08: M4 feature 4, web fonts part 2

- New properties `font-variation-settings` and `font-feature-settings`
  (`style/src/font_settings.rs`), and the same two descriptors in
  `@font-face`. As in Chromium, the computed value is sorted by tag and
  keeps the last value of a repeated tag, and the `font` shorthand
  resets both. At most 64 tags per declaration.
- Axis values come in this order: font matching, the `@font-face`
  descriptor, the property. This also applies to variable system fonts.
  Synthetic bold ignores the property (measured in Chromium 148).
- The metric descriptors `size-adjust`, `ascent-override`,
  `descent-override` and `line-gap-override` apply in the text crate
  (shaping, metrics, glyph masks), so layout code did not change. The
  overrides are ratios of the size after `size-adjust` (measured).
- `src: local()` matches the full name or the PostScript name of a
  system font, ignoring ASCII case and spaces, and loads without a
  request. `local("Arial")` does not match in Chromium either, so the
  fallback faces of both target pages never load.
- Ars geometry 0.6908 → 0.7740, pixels 0.9544 → 0.9898: the headings
  now use `wght` 660. BBC is unchanged. New: probe file
  `tools/probes/web-fonts-2.json` (16 cases), a variable test font made
  for swb (`crates/text/tests/webfonts/swb-variable.ttf`, CC0, with its
  build script), five hostile-page cases.
- Review fix: each distinct `font-feature-settings` list needs its own
  shape plan, and the plan lookup is linear, so 60,000 distinct lists
  took 11.1 s. A font instance now keeps at most 32 plans (0.81 s).
- Tools: `just review-tree reset` failed when the review worktree held
  a patch that touched files changed by the new commit; it now checks
  out with `--force`.
- Backlog: in Chromium, `letter-spacing` other than 0 turns off optional
  ligatures; `ex` uses 0.5em in swb; automatic `opsz`; named instances
  of variable fonts in `local()`.

## 2026-10-08: M4 feature 3, web fonts part 1

- `@font-face` works (ADR 0022). Style parses the descriptors
  (`style/src/font_face.rs`) and keeps the rules with their `@media` and
  `@supports` conditions. The text crate owns the face set
  (`text/src/web.rs`): web families shadow system families of the same
  name, CSS Fonts 4 §5 matching selects a composite font, and its faces
  are checked per character, the last rule first.
- A face loads only when text needs it, and for a line box only the face
  that covers U+0020 (measured in Chromium). The engine loads the faces
  after layout (each URL once, `src` entries in order) and lays out again
  when a font arrives. Font loads count as pending requests, so headless
  rendering waits for them. `local()` fails until part 2. All
  `font-display` values act as `swap` with an infinite swap period.
- WOFF and WOFF2 decoding: the `wuff` crate (MIT) with swb's own Brotli
  and zlib decompressors. A read of its source for robustness found two
  gaps, which swb closes: WOFF 1.0 tables that overlap or repeat, and
  allocation of the declared size before decompression. Limits: 32 MiB
  file, 32 MiB decompressed, 64 MiB decoded.
- Variable fonts get `wght`, `wdth`, `slnt` and `ital` from the font
  properties, clamped to the descriptors (moved from part 2: both Ars
  families are variable). Synthetic bold and oblique follow rules
  measured in Chromium 148 with the probe tool's new `ink` query. The
  probe tool also got `files` (fonts next to the page); 32 cases in
  `tools/probes/web-fonts.json`. Seven new hostile-page cases.
- Review fixes: itemizing text with many loaded faces in one composite
  font took time proportional to characters × faces (49 s for 1,000
  faces and 300,000 characters that no face has). Now itemization keeps
  the result per distinct cluster and checks at most the last 256 faces
  of a composite (0.2 s). The probe tool now waits for
  `document.fonts.ready`, because one Chromium measurement differed
  between runs.
- Scores: Ars geometry 0.1830 → 0.6908, BBC 0.0240 → 0.7775 (pixels
  0.9587 → 0.9942). Ars pixels went down, 0.9595 → 0.9544: the
  headings use `font-variation-settings: "wght" 660` (part 2), so swb
  draws them at `wght` 700, wider, and the cards below move.

## 2026-10-08: M4 feature 2, `:host` and `sizes="auto"`

- `:host`, `:host()` and `:host-context()` parse and match nothing (no
  shadow trees), so Tailwind's base rule `:host,html{...}` now applies
  to html. Chromium rejects `:host()` with a complex selector or a list.
- `sizes="auto"` on lazy images: the candidate is selected after layout
  from the content-box width (`engine/src/page/auto_sizes.rs`) and again
  when the width, viewport or scale changes (at most 8 times per image);
  the old image stays until the new one loads. New in style: `contain`
  and `contain-intrinsic-size` (full syntax); the user-agent rule for
  such images; size containment for replaced elements; the
  `aspect-ratio: auto w/h` hint of `img` attributes (before: `video`
  only).
- Chromium 148, measured: for eager images `auto` is `100vw` and ends
  the list (the spec skips it); swb does the same. With JavaScript off,
  `loading=lazy` defers nothing. For lazy images on a fast local file,
  the chosen source depends on timing (first request against layout);
  the probe file keeps only the cases that do not.
- Correction of the previous entry: the new Ars capture has the same
  103 resources as the first one. Chromium loads the `100vw` candidates
  too, for the hidden (`display: none`) copies of the card images. The
  fixture is in this commit, substituted (86 images; its three font
  families are under the OFL and stay): 4.17 MB instead of 9.25 MB. Ars
  geometry 0.1491 → 0.1876 on the first capture; on the new, substituted
  capture geometry 0.1830, pixels 0.9595.

## 2026-10-08: M4 Ars Technica, `aspect-ratio`

- The owner added two targets, in this order: Ars Technica (target 4,
  M4) and BBC (target 5, M5), both with JavaScript off. Both fixtures
  were captured. Baselines: Ars geometry 0.0531, pixels 0.7841; BBC
  geometry 0.0290, pixels 0.9583. Both pages are close in structure;
  most differences come from web fonts, inline SVG and, on Ars, boxes
  with `aspect-ratio`. The M4 plan is in the roadmap (seven features).
  The BBC fixture is in this commit; the Ars fixture follows after
  `sizes="auto"` (feature 2), because swb now loads 1536w image
  candidates where Chromium loads 384w ones.
- Owner decision: the repository is public, so fixtures must not publish
  photos or commercial fonts. New `swbtools substitute`
  (`just substitute`, docs/testing.md): every raster image becomes a
  generated placeholder of the same size and format; every font without
  a free license becomes DejaVu Sans in the same format (not Liberation,
  the fallback of the test fonts, so the fixture still shows whether a
  browser uses the web font). The BBC fixture is substituted (67
  images, 6 BBC Reith fonts); new baseline geometry 0.0240, pixels
  0.9586.
- Decision: swb draws inline SVG with its own code. ADR 0003 allows
  resvg only for SVG as an image format (`<img>`, CSS images).
- Feature 1, `aspect-ratio` for non-replaced boxes
  (`layout/src/aspect.rs`): blocks, floats, inline-blocks, flex items,
  grid items and absolutely positioned boxes take an `auto` axis from
  the other axis; min and max sizes transfer as for images; a height
  from the ratio is definite for percentage children and grows to the
  content (`min-height: auto`) unless the box is a scroll container.
  Rules measured in Chromium (`tools/probes/aspect-ratio.json`, 22
  cases). On Ars, all 61 `aspect-video` and `aspect-square` boxes now
  match Chromium: geometry 0.0531 → 0.1491, pixels 0.7841 → 0.9592,
  page height 7,420 → 9,505 px (Chromium 9,626).

## 2026-10-08: M3 maintenance

- Two read-only reviewers (layout; the other crates, tools and docs)
  listed duplication, dead code, names and outdated docs; two
  implementers applied the selected items in separate crates. `just
  snapshot` is byte-identical before and after.
- Layout: shared helpers for width/height limits, `auto` self-alignment
  and the alignment edge (new `align.rs`, one `Edge` for grid and
  positioned layout), box-sizing conversion, margins and point/rect
  arithmetic; module docs and a module map in `lib.rs`.
- Other crates: the HTML integer and float microsyntaxes are in
  `dom/src/microsyntax.rs` (five copies removed). As the spec says, `-0`
  now parses as 0 (`maxlength`, `rowspan`, `colspan`, `span`); this is
  the only behaviour change. `escape_html` is in `net`; `mul_255` in
  paint; the shorthand parser is split by property group.
- Tools: `browser.session` and `browser.load_html` replace five copies of
  the Chromium start-up and page loading; Chromium now closes also when
  a page load fails.
- Docs: README status, architecture (flex, replaced and intrinsic
  sizing, the two `calc()` implementations, microsyntaxes), roadmap
  backlog (parsed but unused properties, missing media query units).

## 2026-10-08: Shared tools for the agent workflow

- The owner accepted target 3 (Wikipedia) and closed the question of why
  Chromium source was read (ADR 0021, "Follow-up").
- New tools that the serial agent workflow (CLAUDE.md) relies on, all in
  docs/testing.md:
  - `swbtools probe` (`just probe`): JSON case files with HTML documents
    and queries (boxes, client rects, computed values, script values),
    run in Chromium with the test settings and, with `--with-swb`, in
    swb, compared box by box. It replaces the one-off Playwright scripts
    of M2 and M3. First use: swb and Chromium both give no width to
    columns that only spanning cells cover (a row of `colspan=1000`
    cells is a few px wide in both).
  - `swbtools hostile` (`just hostile`): 64 generated hostile pages, one
    or more for each limit in ADRs 0007, 0010, 0011 and 0015–0019, plus
    generic ones, each run with a time and memory limit and a watchdog.
    Known failures: generated `content` text without a size limit (the
    high-priority bug, next task) and html5ever's quadratic deep nesting
    (100,000 nested `div`s: 13 s).
  - `tools/review-tree.sh` (`just review-tree`): the persistent review
    worktree `../swb-review` with its own target directory.
- CI: `cargo deny --offline` failed on GitHub because the build downloads
  only the crates of the host platform; the workflow now runs
  `cargo fetch --locked` first.
- Generated content limit (the high-priority robustness bug of M3): box
  construction keeps at most 1 MiB of generated text per box tree
  (`MAX_GENERATED_TEXT`; `::before`, `::after`, `::marker` and marker
  numbers) and drops the rest with one warning. 100 KB of `content` on
  3,000 elements took 10 s and 13 GB; now 0.16 s and 84 MiB. At 4 MiB the
  block variant still took 940 MB (inline layout of short words is
  memory-heavy; backlog), so the limit is 1 MiB.

## 2026-10-07: Provenance and attribution

- A provenance audit compared swb with the Chromium, Skia and Servo
  sources that `credits.md` lists. The owner decided to keep the code
  ported from BSD-3 and MPL-2.0 sources with attribution (a clean-room
  rewrite may follow) and to replace data derived from LGPL files with
  measurements.
- New: `swbtools measure` (`tools/swbtools/measure.py`, docs/testing.md)
  measures Chromium 148 black-box. `tools/tests/test_measure.py` compares
  the results with swb's data.
- Text field families (`layout/src/control.rs`): of about 450 candidate
  families, 34 make Chromium size text fields with the width of `0`: the
  31 of the old list and `Kai`, `Lucida Grande` and `#PilGi`. The match is
  on the first family of `font-family` and case-sensitive (`helvetica`
  uses the average width); names that start with `.` are not special.
  swb matched case-insensitively and treated `.` names as special; it
  now does what Chromium does (new layout test
  `forms-zero-width-families`).
- Font size keywords: the measurement confirms the old tables (the 16px
  row is the same in quirks mode). A quirks mode page from
  `page.set_content` after a standards mode page in the same tab kept the
  standards table; the tool loads files instead.
- `ua.css`: the form control rules are rewritten from the HTML Standard
  (spec order, unsupported properties left out) and from measured
  computed values; ruby, frameset, `meter` and `progress` rules cite the
  spec text or the measurement; the `map` and `output` rules (the initial
  value) are gone. Changes where the old rules differed from Chromium:
  disabled checkboxes, radio buttons and image and file inputs keep a
  transparent background; color inputs are `border-box`; range inputs
  have `color: #9d968e`; a text area's placeholder inherits `white-space`
  (it wraps); `marquee` has `text-align: start` (the spec's `initial`)
  and `white-space: nowrap`. `just snapshot` is unchanged. Kept on
  purpose, with comments: the select background (`Field`; Chromium
  computes rgb(239, 239, 239) but draws white) and
  `ButtonBorder` (#767676 in swb, black in Chromium, whose native look
  draws gray).
- Attribution: comments at the derived Chromium code (table layout, grid
  track sizing, sticky offsets, `try_opportunity`, `blocks_content_sizes`,
  the check mark), the Skia data (metric-compatible classes, fake bold
  widths); the Servo-derived `CollapsedMargin` and `BlockMargins` moved
  into their own file with the MPL-2.0 header
  (`layout/src/collapsed_margin.rs`, because the MPL-2.0 applies per
  file); new sections in `THIRD_PARTY_NOTICES.md` with the license
  texts. ADR 0021 records the findings and the decision. Corrected `credits.md`, ADRs 0006, 0010, 0015 and 0017,
  and comments in `floats.rs`, `image_source.rs` (spec order of the
  `<source>` checks, same result as measured), `submit.rs` and
  `raster.rs`.
- Workflow change (owner decision): the M3 workflow (five parallel
  workstreams, stacked reviews, many rebases, everything on the largest
  model) cost far more than the code needed. New rules in CLAUDE.md
  ("Workflow", "Agents"): one feature at a time on `main`, one review per
  feature, typed sub-agents with an explicit model and reasoning effort
  (`.claude/agents/`: `implementer-hard`, `implementer`, `reviewer`,
  `measurer`), short reports, shared measurement tools, and no reading of
  other engines' source code (ADR 0021).

## 2026-10-04: M3 Wikipedia

- M3 runs as five parallel workstreams (floats, positioning, grid, masks,
  scrolling), each in its own worktree. The integrator merges them one
  after another, with a clean-context integration review and a commit
  for each.
- Masks (ADR 0018): `mask-image`, `mask-mode`, `mask-repeat`,
  `mask-position`, `mask-size`, `mask-origin`, `mask-clip`,
  `mask-composite` and `mask`, with Chromium's property model: pure
  aliases (`-webkit-mask-image`, `-size`, `-repeat`), legacy syntaxes
  (`-webkit-mask`, `-webkit-mask-clip`, `-origin`, `-composite`,
  `-position`), and `mask-position` as a shorthand of
  `-webkit-mask-position-x/-y`. `@supports` answers as in Chromium, so
  Wikipedia takes its mask branch. A masked box paints into a layer that
  is multiplied by the mask (alpha or luminance; layers composited with
  `mask-composite`, the bottom layer's operator ignored). Mask images
  load like background images. Measured in Chromium: a failed image
  hides the box, masks do not change hit testing, and a masked box is a
  stacking context but not a containing block. The root element's mask
  and opacity also apply to the canvas background, as in Chromium.
  Limits: 32 layers per box; one memory budget for the layers of all
  open opacity and mask groups (beyond it, opacity groups draw without
  opacity and mask groups draw nothing); a mask work budget (a mask group
  starts only if its work fits). Wikipedia's menu, language, chevron and
  ellipsis icons match the reference (pixels 0.8848 → 0.8859). The
  author's two review rounds found a hang (tiled gradient masks), a
  regression (large opacity groups in full-page screenshots) and smaller
  bugs. The integration review found that full-page screenshots of long
  pages went blank when page-high groups exceeded the budgets: full-page
  screenshots are now rasterized in strips of the viewport height (at
  least 16 Mpx, `swb_paint::rasterize_in_strips`), and the budgets apply
  per strip. It also found that `mask-clip: no-clip` must cover the
  descendants that have their own layer (positioned, opacity, mask). A
  review of those fixes found unbounded gradient-tile work in nested
  masks, a seam at the last strip (it had other budgets), and inexact
  strip offsets at fractional scales: mask groups are now admitted with
  all their work (layer pixels, a cost per row, gradient tiles), the
  strips of a screenshot have equal heights and budgets, and strips are
  drawn in place with the device coordinates of one pass. All fixed with
  tests.
- Scroll containers (ADR 0019): `overflow: auto | scroll | hidden`
  boxes scroll (`hidden` only for scripts and scroll into view). Layout
  computes the scrollable overflow rectangle of each scroll container as
  Chromium does (about 70 cases measured): in-flow margin boxes, line
  boxes and the content height with the end padding, plus the border
  boxes of descendants; hanging `pre-wrap` spaces do not count. The
  engine keeps one offset per element, in whole CSS px and clamped after
  each layout; a scroll rebuilds only the display list. Absolutely
  positioned boxes whose containing block is outside a scroller, and
  fixed boxes, do not move with it (also through block-in-inline
  structures). The wheel scrolls the innermost scrollable container and
  chains outwards; keys use the scroll chain of the focused or last
  clicked node; focus, fragments and automation clicks scroll every
  container into view; selection and hit testing work in scrolled
  content. A root with `overflow: hidden` scrolls for scripts but not
  for the user. The GUI batches wheel events and shows overlay scroll
  indicators. Automation: `dom.scrollTo`, `dom.scrollInfo`,
  `input.wheel`; layout tests can scroll elements first
  (`data-scroll`). Fixture scores are unchanged (nothing on them
  scrolls at 1280×800). Three integration reviews found, among others,
  abspos boxes in positioned inline boxes that did not scroll, a root
  with `overflow: hidden` that scripts could not scroll, one display
  list rebuild per wheel event, and full-page screenshots that reset
  element offsets; all fixed with tests.
- Grid (ADR 0017): CSS Grid Layout 2 without subgrid and masonry.
  Style: the grid longhands and shorthands, `justify-items` and
  `justify-self` (`place-*` set them now), `@supports`. Layout:
  placement and auto-placement (sparse, dense, both flows, named lines
  and areas), Chromium's track collection of ranges and sets, the track
  sizing algorithm as in Chromium, the grid sizing algorithm with
  Chromium's extra row pass, item alignment, intrinsic sizes and
  baselines. Limits: lines clamped to ±10,000, a span budget per layout
  pass, work budgets for placement and named lines; because tracks are
  sets, `repeat(100000, ...)` costs as much as one track. The final
  layout of an item reuses its measuring layout when the result cannot
  differ (without this, Wikipedia's nested grids made layout 25% slower;
  now +5%). Grid scroll containers: as in Chromium, their in-flow
  content is the area of their tracks after content alignment, and items
  count only with their border boxes. Wikipedia: the table of contents
  sits beside the article; size 0.6175 → 0.8100, relative 0.4587 →
  0.7728, geometry 0.0099 → 0.0345; pixels 0.8858 → 0.8722 until floats
  exist (the text is now in its column, where Chromium wraps it around
  floated images). 13 new layout tests match Chromium. The author's
  review found 4 bugs (parser prefixes, image widths with percentage
  heights, measuring-layout reuse with a percentage `flex-basis`,
  stretched size keywords). The integration review found more: the
  measuring layout was reused where a definite height changes the
  result (`min-height`, `max-height`, auto-repeat rows); named areas and
  line names were rebuilt for every layout (20,000 areas shared by 3,000
  grids took 90 s; now they are indexed once per declaration, 0.1 s);
  the grid baseline ignored baseline-aligned items (§11.6); explicit
  `stretch` on images; border-box `min-width`/`max-width` counted the
  padding twice in intrinsic sizes (also outside grid); inline flex and
  grid scroll containers use their first baseline. All fixed with tests.
- Positioning (ADR 0016): absolute, fixed and sticky positioning, 2D
  transforms and `clip: rect()`. Layout puts a placeholder fragment at
  the static position of each absolutely positioned box (block, inline,
  flex and grid layout); after layout, one walk lays out each box in its
  containing block and replaces the placeholder, so each box is laid out
  once, also inside cached flex items and cells. Static positions, the
  CSS 2.2 §10.3.7/§10.6.4 rules, inline containing blocks, flex static
  positions, sticky offsets (Chromium's algorithm, nested sticky boxes,
  sticky boxes in scroll containers) and painted element boxes match
  Chromium in 18 new layout tests, among them reductions of Wikipedia's
  title bar, dropdowns, search field, thumbnails and sticky table of
  contents. Paint: transform groups (matrix, fixed, sticky) in the
  display list, resolved with the scroll offset when rasterizing and
  hit testing; translations are exact, other transforms draw into a
  bounded, anti-aliased layer that shares the per-strip budgets with
  opacity and mask layers. Positioned boxes with `z-index: auto` no
  longer form stacking contexts; positioned boxes keep only the clips of
  their containing block chain; fixed boxes escape all ancestor clips
  except `clip`. Scroll containers: transformed boxes are containing
  blocks for scrolling too, and out-of-flow boxes count in scroll
  ranges. Full-page screenshots keep the viewport layout and draw at
  scroll offset 0, as Chromium does. Wikipedia: geometry 0.0345 →
  0.1577, size 0.8100 → 0.9592, relative 0.7728 → 0.9217, pixels 0.8722
  → 0.8730, no missing element boxes (was 542). Hacker News at 700×900
  now scales its vote arrows (`transform: scale(1.3)` in its narrow
  layout). The integration review found the `inset: 0; margin: auto;
  height: fit-content` centering idiom broken (intrinsic height keywords
  stretched), self-alignment of abspos boxes ignored (now CSS Position 3
  §4.4, measured in Chromium), fragment navigation to a fixed element
  that scrolled the viewport, full-page screenshots of a scrolled page
  that differed from Chromium, and hostile pages that took 1–39 s (an
  opacity group around a tiny fixed box became a full-surface layer;
  sticky offsets were recomputed per group; the display-list build was
  O(items × depth)). Fixed: the display list collects items in chunks
  and computes group bounds in one pass, sticky offsets are cached per
  frame, fixed content has its own bounds; those pages take 2–31 ms.
  Typed URLs that differ only in the fragment scroll without a reload;
  selection honours `clip: rect()`; `html { position: absolute }`
  works.
- Floats (ADR 0015): an exclusion space per block formatting context
  (`layout/src/floats.rs`), the float placement rules of CSS 2.2
  §9.5.1, lines shortened by floats that move down when they do not fit,
  floats in inline content on the line if they fit, clearance with
  Chromium's margin rules (a block that clears floats waiting for its
  container's position goes exactly to the clearance offset), boxes that
  establish a BFC beside floats, auto heights that include floats, and
  intrinsic sizes with floats. Block layout knows positions in the BFC:
  containers whose top margin can still collapse wait in a chain, and
  their floats are placed when the chain resolves, without a second
  layout. Paint phases (CSS 2.2 Appendix E): block backgrounds, floats,
  inline content; positioned boxes keep tree order. Floats count in the
  scrollable overflow of scroll containers, and abspos boxes next to
  floats get Chromium's static positions. Limits: 10,000 floats per
  BFC, a work budget per layout pass, at most 64 attempts per line.
  Wikipedia: size 0.9592 → 0.9631, relative 0.9217 → 0.9377, pixels
  0.8730 → 0.8903. The author's two reviews found 2 resource risks and
  11 layout differences; the integration review found opacity and masks
  on inline boxes ignored, positioned boxes out of tree order, the
  forced clearance offset, lines in `nowrap` containers, and hostile
  pages that took up to 130 s (now under 1 s); all fixed with tests.
- Flexbox: the flex container's own `min-height` and `max-height` take
  part in the flex algorithm (single-line cross size, column main size
  and line breaking, multi-line cross size); `align-content: stretch`
  grows lines before items stretch; the container's baseline comes from
  the visually first item of the first line, synthesized from its
  border box (CSS Flexbox 1 §8.5, measured in Chromium). Wikipedia's
  icon buttons were 36 px lines instead of 32 and the page toolbar 36 px
  instead of 33; the header and toolbar now match. Known failures
  `flex-container-min-size`, `inline-flex-baseline` and
  `flex-align-content-stretch` pass.
- Media elements (ADR 0020): `<video>` and `<audio>` are replaced
  elements. A video's natural size is its poster's; without a loaded
  poster it has none (300×150 without a ratio, as in Chromium). `width`
  and `height` on `<video>` map to `aspect-ratio: auto w/h`, and the
  computed `aspect-ratio` keeps `auto` (fixes an M2 backlog item). New
  `object-position`; `<img>` and posters use `object-fit` and
  `object-position`. Posters load like `<img>` sources; media resources
  are never fetched. The controls approximate Chromium's with scripting
  disabled (always shown); their parts drop out at sizes measured in
  Chromium. Children (`<source>`, `<track>`, fallback content) get no
  boxes. Wikipedia: the video thumbnail now has its size, so the page
  below it is no longer 80 px too high; with floats and the flex fixes,
  geometry 0.2073 → 0.5136, pixels 0.9378 → 0.9766, no extra boxes.
- Responsive images: `<img srcset>` with `x` and `w` descriptors,
  `sizes`, and `<picture>`/`<source>` (`media`, `type`, `sizes`,
  `width`/`height`) (`engine/src/image_source.rs`, `css/src/sizes.rs`;
  ADR 0011 and 0007 updates). Measured in Chromium 148: candidates are
  sorted by density, and the first with density ≥ the device pixel
  ratio wins, else the densest. Natural sizes are divided by the density
  (also for SVG and `object-fit`). A selected `<source>` with `width` or
  `height` replaces both dimension attributes of the image. Sources are
  selected again after viewport or scale changes; an image keeps its
  source until the new one has loaded. Wikipedia's footer icons now have
  Chromium's sizes (size 0.9669 → 0.9694, relative 0.9476 → 0.9501).
  Two reviews found a negative-zero density bug, cancelled loads that
  never retried, and unbounded waiting lists; all fixed. The Wikipedia
  fixture gained the six 2x thumbnails that swb now requests at scale 2,
  and one icon of the 700 px layout (recorded with `--record-missing`).
- CSS counters: `counter-reset`, `counter-increment`, `counter-set`,
  `counter()` and `counters()` in `::before`, `::after` and `::marker`,
  and the `list-item` counter. A pass at the end of style computation
  (`style/src/counters.rs`) resolves counters to text and computes the
  ordinal values of list items; layout no longer numbers list items
  (ADR 0007 update). Where Chromium 148 differs from CSS Lists 3, swb
  follows Chromium (measured with DOM snapshots): the scope of a nested
  reset, `display: contents`, markers that use HTML ordinal values,
  `list-item` without presentational hints. The unit tests are generated
  from Chromium output; a review fuzzed about 13,000 values against
  Chromium. Wikipedia: the backlink letters of the references (geometry
  0.5136 → 0.5382, size 0.9694 → 0.9862). A provenance check against
  the Blink files that were read found no copied code.
- Line breaking and text measurement: break opportunities follow
  Chromium (URLs break only after `-` and `?`, a break after every run of
  spaces, UAX #14 for Unicode 17 with measured tailorings), with
  `word-break: break-all | keep-all`, `hyphens: none`, `overflow-wrap:
  break-word | anywhere` and `word-break: break-word` (in layout and
  min-content); rules at element boundaries (isolating boxes, `nowrap`
  box ends, atomic inlines in nowrap text); text widths as Chromium
  computes them (the font size truncated before shaping, item widths
  rounded up to 1/64 px, a 1/64 px fit tolerance); synthesized small
  capitals; right-to-left scripts shaped right to left with context;
  multicol containers are block formatting contexts. The first line
  breaker was a port of Blink's LGPL `text_break_iterator.cc`; a review
  found it before the commit, and a clean-room agent rewrote
  `text/src/linebreak.rs` from UAX #14, the Unicode data and about
  360,000 measured strings (`swbtools linebreaks`; tables generated from
  the measurements; 27 known differences listed). Wikipedia geometry
  0.5382 → 0.9733, pixels 0.9766 → 0.9936; Hacker News pixels 0.9909 →
  0.9981. The known failure `nowrap-atomic-inlines` passes. A review of
  the combined change found the kerning scale (now the 16.16 scale of the
  size), small-caps line metrics in fallback fonts, `bidi-override`
  shaping and smaller items; all fixed. With the counters, Wikipedia
  geometry is 0.9978 (pixels 0.9936): target 3 is done.
- Text: a variable font with a malformed `wght` axis (min > max) no
  longer panics; the axis range includes the default value, as in
  HarfBuzz.
- `just snapshot DIR` (`tools/snapshot.sh`) writes swb's rendering of
  all fixtures and layout tests, to check that a refactoring changes
  nothing (testing.md). The M2 maintenance pass used it first.

## 2026-10-04: M2 maintenance

- End-of-milestone review of the whole codebase. Five clean-context
  agents each reviewed a group of crates (css and style; layout and text;
  dom and paint; engine; net, automation, swb and the Python tools) in
  their own git worktree; the integrator merged the patches and fixed the
  shared docs.
- Behaviour is unchanged. A snapshot script wrote the layout dump, the
  DOM dump and full-page screenshots (1280×800, 700×900, and 1280×800 at
  scale 2) of every fixture, and the layout dump and a screenshot of
  every layout test. The snapshots before and after the changes are
  byte-identical, for each agent's patch and for the merged result.
- Dead code removed: unused helpers in css (`Declaration::parser`,
  `MatchingContext::clear_caches`, `Stylesheet::parse`), layout
  (`Rect::size`, `Rect::outset`, unused table builder methods and
  parameters), net (`impl Extend for Headers`) and engine
  (`TextEdit::select`).
- Duplication removed: style's Bloom filter keys come from the parsed
  selector (`Selector::ancestor_keys`) instead of serializing and
  re-tokenizing each selector; one shrink-to-fit width; shared table
  helpers; one word lookup for double clicks and text fields; one
  "nearest link" search; one editability rule for form controls; the
  history entry holds its `Commit`; shared test helpers in every crate.
- Narrowed many `pub` and `pub(crate)` items; corrected wrong spec
  section numbers and anchors (tables, form encodings), outdated module
  docs and comments; dated update notes in ADRs 0004 to 0010 and 0013.
- The reviewers found 10 bugs (one panic on a malformed system font).
  They are listed in the roadmap ("Backlog from the M2 maintenance
  review").
- M3 plan: five workstreams (floats, positioning, grid, masks,
  scrolling). `@font-face` and `@import` moved to "Later": the Wikipedia
  page uses neither.

## 2026-10-03: M2 Hacker News

- The owner accepted Hacker News (target 2).
- The owner allowed the Public Suffix List (MPL-2.0) as a one-off license
  exception (ADR 0014). `deny.toml` now declares the real license of the
  `psl` crate (the crate metadata says only MIT/Apache-2.0, and its copy
  of the list has the license header removed) and allows MPL-2.0 for that
  crate only, and only `swb-net` may depend on it. `THIRD_PARTY_NOTICES.md`
  states the license for binaries; it also names the WHATWG-derived
  stylesheets (CC BY 4.0), which binaries contain too. `fixtures/pages`
  has a note that the page snapshots are third-party material.

- Cookies: an RFC 6265bis jar in the `net` HTTP client with Chromium's
  defaults (Lax by default, schemeful sites, a 400-day cap, 180 cookies
  per domain and 3300 in total), the Public Suffix List through `psl`,
  `Request::initiator` for `SameSite`, `Request::post` and the `Origin`
  header, automation `cookies.get` and `cookies.clear` (ADR 0012). The
  list's data is MPL-2.0 (decided later the same day: ADR 0014).
- Tables (ADR 0010): Chromium's LayoutNG algorithm for column widths,
  row heights and cell alignment, automatic and fixed layout, separated
  and collapsing borders, captions, column and row backgrounds, the
  anonymous table fixup; `-webkit-center` for blocks; the quirks-mode line
  height and table cell rules; boxes for `<br>` and for inline elements
  around blocks. Hacker News geometry went from 0 to 0.9876 (the rest is
  the search field, which needs forms); Wikipedia size 0.58 → 0.62. 20
  new layout tests against Chromium. The author's review found three bugs
  (memory of block-in-inline boxes, a `display: none` sibling that split
  anonymous tables, column backgrounds painted per spanned column); the
  integration review found more: trimmed trailing spaces added a strut in
  quirks mode, table `min-height` was ignored, `<img>` with a table
  display lost its image, row gradients restarted in every cell, floats
  did not make a cell non-empty, and hostile tables could cost quadratic
  time (grid placement) or much memory (collapsed borders). All fixed
  with tests; grid placement now uses a segment tree, and collapsed
  borders have a budget per layout pass. Two findings (struts in list
  items, row backgrounds under hidden empty cells) were checked against
  Chromium and are not bugs.
- SVG images (ADR 0011): resvg 0.48 (no `text` feature), detection by
  MIME type as in Chromium, natural sizes and the CSS default sizing
  measured against Chromium (24 `<img>` cases), rendering at device
  resolution with a size cache, only `data:` URLs inside SVG images.
  Three adversarial review rounds with about 75 attack files found
  crashes (reference cycles and chains that overflow the stack in usvg)
  and unbounded costs (`<use>`, markers, arcs, entities, CSS, masks,
  filters); each is bounded before conversion or rendering. Each frame
  has an SVG rendering budget; when it runs out, images use a cached
  rendering of another size or stay blank until a repaint. The Hacker
  News logo and vote arrows now match Chromium (pixels 0.9901 →
  0.9909).
- Two integration reviews of the SVG work found: a cost estimate that
  missed canvas area outside the image box (a crafted file could render
  for tens of seconds), infinite sizes from extreme aspect ratios, NaN in
  the cost bound, a redraw loop in the first per-frame budget design
  (replaced by the simpler budget above), nondeterministic choice of a
  cached rendering, and replaced-element sizing bugs that also affected
  raster images: `max-width` and `max-height` together (now the full CSS
  2.2 §10.4 table), definite heights of images in row and column flex
  containers (the known failure `flex-replaced-item` now passes). All
  fixed with tests; 3 new layout tests against Chromium. Wikipedia pixels went down 0.8854 → 0.8850: its SVG images now
  render, at positions that are wrong until floats and grid exist (M3).
- Forms (ADR 0013): control state outside the DOM (`Forms`, keyed by
  node), one editing model (`TextEdit`) for page fields and the address
  bar, controls as atomic boxes with Chromium's sizes and light theme,
  activation of buttons, checkboxes, radio buttons, labels and links
  around controls, the HTML form submission algorithm with the
  urlencoded, multipart and text/plain encodings, `POST` navigations
  that keep their body and initiator in history ("Confirm form
  resubmission" on back/forward), automation `input.type` and
  `dom.value`. The integration review found a CSRF-relevant bug (a
  reloaded `POST` lost its initiator, so it was sent without `Origin` and
  with same-site cookies), a duplicate `POST` on reload during a pending
  submission, quadratic time in disabled fieldsets, and activation and
  spec details; all fixed with tests. `swb_net::Response::redirected`
  lets a post/redirect/get to the same URL drop the body.
- Flex containers: max-content widths now include `column-gap`
  (Wikipedia geometry 0.0072 → 0.0099; pixels 0.8850 → 0.8848).
- Hacker News is done: geometry 1.0 (818 elements), pixels 0.9909; the
  search form submits to hn.algolia.com, links and history work.
- M2 ran as four parallel workstreams (cookies, tables, SVG images,
  forms), each in its own worktree and with its own commit. Each
  workstream had one or two integration reviews after the merge; they
  found real bugs in every workstream.
- Next: end-of-milestone maintenance for M2, then M3 (Wikipedia).

## 2026-10-02: M1 maintenance

- End-of-milestone review of the whole codebase. Five clean-context
  agents each reviewed a group of crates (css; style; layout and text;
  dom, paint and engine; net, automation, swb and the Python tools) in
  their own git worktree. The integrator merged the patches, made the
  changes that cross crates, and fixed the docs.
- Behaviour is unchanged. The layout agent compared full-precision
  fragment-tree dumps of all layout tests and fixtures (at several
  viewport sizes) before and after its changes: byte-identical. Scores
  and layout tests are unchanged.
- Removed dead code: unused CSS parser entry points and helpers, unused
  style value types (`BackgroundLayer`, `Sides`, `Corners`,
  `ContentItem::Attr`, `GridAutoFlowDirection`), `dump_subtree`, the
  `markup5ever` dependency, an unused manifest helper in the tools.
- Removed duplication: one CSS-wide keyword table; one 1-to-4 value
  expansion for box shorthands; one `@supports` condition-or-declaration
  fallback; `Fragment::move_by`; one `percent_decode` (exported by
  `net`); the viewport limits and the network thread count now live in
  the engine (`check_viewport_size`, `check_scale`,
  `PageConfig::DEFAULT_NETWORK_THREADS`); `Page::is_fully_loaded`
  replaces a copy in the automation crate; shared engine test helpers.
- Split `engine/src/page/mod.rs` (1,220 lines) into `mod.rs`,
  `loading.rs`, `pipeline.rs` and `scroll.rs`. The moved code is
  unchanged apart from visibility (`pub(super)`).
- Narrowed `pub` items, fixed wrong spec section numbers, added spec
  URLs, corrected module docs, ADR 0006 and 0007 details,
  `architecture.md`, `testing.md`, `performance.md` and `credits.md`.
- Fixed a flaky test: `connection_refused_is_an_error` connected to a
  port that a concurrently running test server could take; it now uses
  port 0.
- The reviewers found 12 bugs and 3 undocumented paint simplifications.
  They are listed in the roadmap ("Backlog from the M1 maintenance
  review").

## 2026-10-02: M1 senko.net, automation API

- The owner checked senko.net (hover, Tab, Enter, selection, copy) and
  copy and paste into Wayland applications, including middle-click paste:
  target 1 is accepted.

- Automation: the `automation` crate (tungstenite server, one thread per
  connection, methods executed on the page thread, `HeadlessBrowser`,
  blocking `Client`), `swb --remote-port PORT` in headless and GUI mode, a
  Python client (`swbtools.automation`). ADR 0008 accepted with changes: no
  events yet (a tungstenite connection cannot write while it waits for a
  read), an `Origin` check so web pages cannot drive swb, `browser.close`.
- Interaction (ADR 0009): mouse down/move/up and key events in the
  engine; hover, active, focus, focus-visible and target states with a
  restyle only when a selector depends on the state, and no relayout when
  no style changed; cursor shapes; Tab order per the HTML spec; links follow
  on release, not on press.
- Text selection: the box tree records a source map from processed text to
  DOM offsets, text fragments carry caret stops, and a selection is a pair
  of DOM text positions. Copy uses the `innerText` rules. The GUI copies
  with Ctrl+C (arboard; X11 fallback on GNOME) and sets the primary
  selection.
- Measured in Chromium 148 and matched: selection colors, the focus ring of
  `outline-style: auto` (and the UA rule that links get `outline-offset:
  1px`), the 4 px drag threshold.
- `swb --record-missing DIR` and `just capture-missing NAME`: add only the
  missing responses to a fixture. Added the two footer SVGs that swb
  requests to the Wikipedia fixture (scores unchanged).
- `swb --bench N` and `just perf`: per-stage timings; the baseline is in
  [performance.md](performance.md). Wikipedia: 30 ms for the whole pipeline.
- `dom.outerHtml` needed an HTML serializer (`swb_dom::outer_html`);
  `dom.querySelectorAll` uses `swb_style::query_selector_all`.
- senko.net is done: geometry 1.0, and hover, focus, selection and the
  narrow layout look the same as in Chromium side by side.
- Review before the commit: three clean-context reviewers (layout, paint,
  style and dom; engine interaction; automation, binary, tools and docs)
  reported about 40 findings, no panics. Fixed with tests: a full restyle
  on every scroll step on pages with `:hover` rules (now `:hover` waits for
  the next mouse movement, as in Chromium); the old page's stylesheet
  applied to the new document for `:target`; `user-select: none` text
  selected and copied; tabs between table cells; the cursor one movement
  behind `:hover` rules; focus on hidden elements; skip links; triple
  click selected the whole block instead of the paragraph; the selection
  highlight now fills the line box; focus rings enclose images in links;
  disabled controls in disabled fieldsets; the `browser.close` reply lost
  when swb exited quickly; keepalive pings that broke long requests in the
  Python client; server limits (request size with close code 1009, 16
  connections, a 10 s deadline for the handshake; all tested except the
  deadline).
- A second review of the fixes found 9 more issues, fixed with tests: the
  line box of text in relatively positioned inline boxes, a trailing tab
  in copied text, focus rings around clipped overflow, Chromium's exact
  "center if needed" rules for large elements, `tabindex` with leading
  zeros, and the server tests above.
- Next: M2 (Hacker News): tables, SVG images, forms, cookies.

## 2026-10-02: M0 foundation

- Built all crates of the pipeline. Five crates were written by
  sub-agents in parallel from written specifications (`net`, `css`, `text`,
  `style`, and the Python tools); the integrator wrote `dom`, `layout`,
  `paint`, `engine` and `swb`, and integrated everything.
- Captured fixtures for the three targets and their Chromium references.
- First comparison results (test fonts, 1280×800):
  - senko.net: all 65 element boxes match Chromium within 0.1 px.
  - Hacker News: readable; geometry 0 because tables are laid out as
    blocks (M2).
  - Wikipedia: readable; geometry 0.007 because of missing grid, floats
    and SVG (M3).
- Bugs found by comparing with Chromium and fixed:
  - Anonymous boxes inherited the initial border width (3 px) without the
    "style none means width 0" rule.
  - Blink floors the ascent half of the leading.
  - `overflow: clip` (the UA style of `img`) does not remove the flex
    automatic minimum size; only scroll containers do.
  - Piece grouping dropped the start of an inline box at a line start.
  - Chromium shapes across inline element boundaries (kerning between a
    space and a link's first letter) and removes trailing spaces at line
    ends, so split inline boxes end before them.
  - Nested positioned boxes were painted before their positioned
    ancestor; paint order now uses stacking contexts.
  - With system fonts, Chromium maps `serif` to Times New Roman,
    `sans-serif` to Arial, `cursive` to Comic Sans MS and `fantasy` to
    Impact (Chrome's default font settings), not to fontconfig's choice.
- Added `--with-chrome` to render the whole window headless, so the user
  interface can be checked without a display.
- Review before the first commit: three clean-context reviewers (layout;
  dom/paint/engine/swb; net/text) reported about 70 findings, most verified
  against Chromium. The same agents then fixed the agreed subset with
  regression tests: stack overflows on deep documents, exponential flex
  layout, quadratic text processing, infinite geometry, unbounded memory
  (glyph cache, image decoding, `file:` devices, content-coding chains),
  paint order and clipping of positioned boxes, hit testing in paint order,
  border colors with radii, seams at fractional scales, fragment
  navigation and history, cancelled requests on navigation, and many
  layout details (markers, margins, baselines, `vertical-align`,
  `text-transform`). The rest is in the roadmap backlog; layout items have
  a failing layout test each (`tests/layout/known-failures.txt`).
- A final review of the fixes found a few more problems, fixed with tests:
  the first version of the flex cache stored a deep copy per nesting level
  (memory proportional to depth × page size); fragment children are now
  shared. Also quadratic paths in itemization, box construction and the
  toolbar; gaps in rounded borders; fragment scrolling before images
  load; `file:` loads from web pages are now blocked as in Chromium.
- Next: M1 (automation API, hover, text selection), then tables (M2).

## 2026-10-02: Project start

- Initial Q&A with the owner. Requirements and answers recorded in
  [ground-rules.md](ground-rules.md).
- Installed the Rust toolchain with rustup (Debian's rustc 1.85 is too old
  for current winit and image crates).
- Checked the three initial targets:
  - senko.net: 5.6 KB, inline `<style>`, flexbox, media queries, no scripts.
  - Hacker News: 34 KB, table layout, one stylesheet, SVG logo, one script
    (not needed for reading).
  - Wikipedia "Web browser": 415 KB, two `load.php` stylesheets, Vector 2022
    skin, JavaScript not needed for reading.
- Wrote ADRs 0001–0005: ADR process, Rust, dependency policy, engine
  structure, testing strategy.
- Next: M0 vertical slice (see [roadmap.md](roadmap.md)).
