# Development log

Newest entries first. One entry per working session or milestone. Record what
was done, what was learned, and what is next. Keep entries short; details go
in commit messages, ADRs and other docs.

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
