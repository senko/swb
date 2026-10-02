# Development log

Newest entries first. One entry per working session or milestone. Record what
was done, what was learned, and what is next. Keep entries short; details go
in commit messages, ADRs and other docs.

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
