# Development log

Newest entries first. One entry per working session or milestone. Record what
was done, what was learned, and what is next. Keep entries short; details go
in commit messages, ADRs and other docs.

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
