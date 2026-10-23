# Testing

This document describes the test infrastructure and how to use it. The
strategy and its reasons are in [ADR 0005](adr/0005-testing-strategy.md).

## Overview

| Level                  | Where                                   | Runs Chromium                     |
|------------------------|-----------------------------------------|-----------------------------------|
| Unit tests             | each crate, `#[cfg(test)]`              | no                                |
| Layout tests           | `tests/layout/*.html` + `*.boxes.json`  | only to regenerate `*.boxes.json` |
| Line break tests       | `crates/text/tests/linebreak.rs` + `crates/text/tests/linebreak/` | only to measure again |
| Font size and width tests | `font_size_rule_matches_chromium` in `crates/text/src/shape.rs` + `crates/text/tests/chromium-font-sizes.txt`; `crates/text/tests/font_widths.rs` + `chromium-font-widths.txt` | only to measure again |
| Page fixtures          | `fixtures/pages/<name>/`                | only to capture and to regenerate `reference/` |
| Scores (ratchet)       | `fixtures/scores.json`                  | no                                |
| Engine tests           | `crates/engine/tests/page.rs` (navigation, history, fragments, cancellation, hit testing, display list) | no |
| Interaction tests      | `crates/engine/tests/interaction.rs`    | no                                |
| Form tests             | `crates/engine/tests/forms.rs` (editing, activation, submission, POST history; a recording in-memory fetcher) | no |
| Details tests          | `crates/engine/tests/details.rs` (`details` and `summary`: toggling with the mouse and keyboard, hidden contents, default summary) | no |
| SVG image tests        | `crates/engine/tests/svg_images.rs`     | no                                |
| Inline SVG tests       | `crates/engine/tests/inline_svg.rs` (sizes, the box dump of the `<svg>` and its descendants, painting at other scales, page styles, `<style>` in the SVG, the content-box clip, the baseline, links, `clipPath`, `use`, `shape-rendering`, hit testing of shapes); `tests/layout/inline-svg-boxes.html` (the boxes of `g`, shapes and `use` against Chromium) | no |
| Scrolling tests        | `crates/engine/tests/scrolling.rs` (scroll containers: wheel, keys, scroll into view, paint, hit testing) | no |
| Positioning tests      | `crates/engine/tests/positioning.rs` (fixed and sticky boxes while scrolling, clips, z-index order, transforms, `clip`; hit testing and pixels) | no |
| Media tests            | `crates/engine/tests/media.rs` (video posters and their requests, `object-fit`, controls and the default poster in pixels, hit testing) | no |
| Responsive image tests | `crates/engine/tests/responsive_images.rs` (`srcset`, `sizes` and `<picture>` at other scales and viewports, selection again after a viewport or scale change, `object-fit` with density; by the color of the drawn image) | no |
| Web font tests         | `crates/text/tests/web_fonts.rs` (the face set, loading requests, composite fonts, matching, variation axes, synthesis); `crates/engine/tests/web_fonts.rs` (loading in a page: each URL once, `src` fallback, URLs relative to the style sheet, one face set per document) | no |
| Auto-size tests        | `crates/engine/tests/auto_sizes.rs` (`sizes="auto"`: selection after layout by the box width, `100vw` for eager images, the user-agent `contain: size` rule, selection again after a viewport or scale change; a recording fetcher) | no |
| JavaScript shell tests | `crates/js/tests/shell.rs` (`swb-js`: output, exit codes, `$262`, `--dump-ast`, limits, the test262 runner on a tiny directory of our own); `crates/js/src/test262/` unit tests (frontmatter, skip rules, scores) | no |
| JavaScript engine tests | `crates/js/tests/vm.rs` (scripts and their expected output, compared with Node.js 22), `vm_limits.rs` (limits, termination, the time limit), `gc.rs` (the collector, roots, weak tables, accounting), `limits.rs` (the heap limit, no-GC regions, kind payloads), `objects.rs` (shapes, elements, the internal methods), with `tests/common/`. Each case runs with and without `HeapConfig::stress` (a collection at every safepoint) and fails if a collection finds a stale root. Front end: unit tests in `crates/js-syntax/src/`, `js-text` and `js-regexp`; regular expressions: `crates/js-regexp/tests/` (see "Regular expression tests") and the `RegExp` built-in: `crates/js/tests/builtins_regexp.rs`, `regexp_hostile.rs` (see "RegExp built-in tests"); `Proxy`: `builtins_proxy.rs`, `proxy_hostile.rs` (see "Proxy tests"); the weak collections: `builtins_weak.rs`, `weak.rs`, `weak_hostile.rs` (see "Weak collection tests"); `BigInt`: `builtins_bigint.rs`, `bigint_hostile.rs` (see "BigInt tests"); `ArrayBuffer` and `DataView`: `builtins_binary.rs`, `binary_hostile.rs` (see "ArrayBuffer and DataView tests"); the typed arrays: `builtins_typed.rs`, `typed_hostile.rs` (see "Typed array tests"); `Date`: `builtins_date.rs`, `date_hostile.rs` (see "Date tests"); unit tests of the Python generators in `tools/tests/test_js_unicode_tables.py` and `test_js_time_zone_names.py` | no |
| test262 (ratchet)      | `crates/js/test262/` (`subset.txt`, `scores.json`; parse-only mode: `parse-skip.txt`, `parse-scores.json`); `just test262`, `just test262 --parse-only`, not part of `just check` | no |
| Automation API tests   | `crates/automation/tests/headless.rs`; Python client: `tools/tests/test_automation.py` | no |

`cargo test` needs no network, no Python and no Chromium. Python and
Chromium are needed only to capture fixtures, to regenerate the expected
files, and to compare swb with Chromium (`compare`).

The Python tools are in `tools/` (package `swbtools`).

## Setup

- Python 3.13 and [uv](https://docs.astral.sh/uv/).
- `cd tools && uv sync` installs the dependencies into `tools/.venv`.
- Playwright is pinned to 1.60.0. It uses Chromium revision 1223
  (Chromium 148.0.7778.96, the `chromium-headless-shell` build). If the
  browser is not in `~/.cache/ms-playwright/`, run
  `cd tools && uv run playwright install chromium`.

When you upgrade Playwright, the Chromium version changes. Regenerate all
references and layout test expectations in the same commit, and check the
scores.

## Commands

Run the tools with `just` from the repository root, or with
`uv run swbtools ...` in `tools/`. `uv run swbtools <command> --help` shows
all options. `-v` (before the command) prints progress, `-vv` debug output.

| just                         | swbtools                               | What it does |
|------------------------------|----------------------------------------|--------------|
| `just capture URL NAME`      | `capture URL NAME`                     | Downloads a page into `fixtures/pages/NAME/`. |
| `just capture URL NAME --js` | `capture --js URL NAME`                | Downloads a page with JavaScript on into `fixtures/js/NAME/` (local only; see "Capture with JavaScript on"). |
| `just js-survey [NAME...]`   | `js-survey NAME... \| --all [--runs N]` | Replays `fixtures/js/NAME` in Chromium with JavaScript on and measures what the scripts do (see "Survey the scripts of a page"). |
| `just capture-missing NAME`  | `capture-missing NAME`                 | Adds only the responses that the fixture does not have. |
| `just substitute NAME...`    | `substitute NAME... [--check]`         | Replaces photos with placeholders and non-free fonts with free ones in a fixture (see "Substitute copyrighted content"). |
| `just reference [NAME...]`   | `reference NAME... \| --all`           | Writes Chromium's `reference/boxes.json` and `reference/screenshot.png`. |
| `just tools reference --js NAME...` | `reference --js NAME... \| --all` | Writes Chromium's output with JavaScript on for `fixtures/js/NAME` into `out/js-reference/NAME/` (local only; see "Compare with JavaScript on"). |
| `just compare [NAME...]`     | `compare NAME... \| --all`             | Runs swb on fixtures and compares with the references. `just` builds swb first. |
| `just compare NAME --full-page`    | `compare NAME --full-page`       | Also compares the full-page screenshots and lists the differing regions (see "Compare swb with Chromium"). |
| `just compare NAME --click SEL` | `compare NAME --click SELECTOR` | Compares the page after clicking the first element of SELECTOR in both browsers (repeatable). |
| `just compare --js NAME...`  | `compare --js NAME...`                 | Runs swb with JavaScript on over `fixtures/js/NAME` and compares with `out/js-reference/NAME/` (see "Compare with JavaScript on"). |
| `just update-scores`         | `compare --all --update-scores`        | Also writes the scores to `fixtures/scores.json`. |
| `just layout-refs [NAME...]` | `layout-refs [NAME...]`                | Writes `tests/layout/NAME.boxes.json` with Chromium. |
| `just perf [NAME...]`        | `perf [NAME...] [--runs N]`            | Times swb's pipeline stages per fixture ([performance.md](performance.md)). |
| `just tools measure [NAME...]` | `measure [NAME...]`                  | Measures the Chromium behaviour that some of swb's data comes from (see "Measure Chromium behaviour"). `font-size-sweep` runs only when named. |
| `just probe FILE...`         | `probe FILE... [--case NAME]... [--with-swb] [--tolerance PX] [--json]` | Asks Chromium (and with `--with-swb`, swb) about the cases of JSON files: boxes, line fragments, computed styles, JS values; cases with `"javascript": true` run their scripts (see "Probe Chromium behaviour"). |
| `just hostile [NAME...]`     | `hostile [NAME...] [--swb PATH] [--list] [--keep]` | Runs swb on the hostile-page set and checks time and memory limits (see "Hostile-page set"). `just` builds swb first. |
| `just test262 [ARGS]`        |                                        | Runs the test262 subset in `swb-js` and compares the pass counts with `crates/js/test262/scores.json`; `--parse-only` runs only the front end over `test/language/`, `test/built-ins/` and `test/annexB/` against `parse-scores.json` (see "JavaScript engine tools"). |
| `just jsdiff FILE...`        | `jsdiff FILE...`                       | Runs files in `swb-js` and in Node.js and diffs stdout and the uncaught error. |
| `just jsbench [DIR]`         | `jsbench [DIR] [--runs N]`             | The engine's benchmark programs against `node --jitless`; for a directory of scripts also lexing, parse and compile ([performance.md](performance.md)). |
| `just tools list`            | `list`                                 | Lists the fixtures, their entry counts and sizes. |
| `just tools linebreaks`      | `linebreaks [--out DIR]`               | Measures Chromium's line break opportunities into `crates/text/tests/linebreak/` (see below). |
| `just tools linebreak-tables` | `linebreak-tables`                    | Writes `crates/text/src/linebreak/tables.rs` from the UCD and `latin1.txt`. |
| `just tools js-time-zone-names` | `js-time-zone-names`             | Writes `crates/js/src/date/zone_names_data.rs` (the long English time zone names: the standard and daylight names of 191 metazones, three zone names, the metazone of each of 573 zone names with the aliases) from CLDR 48 (`common/main/en.xml`, `common/supplemental/metaZones.xml`, `common/bcp47/timezone.xml`, from `https://unicode.org/Public/cldr/48/cldr-common-48.zip`, downloaded into `out/cldr/48/`). `just fmt` formats the file. |
| `just tools js-unicode-tables` | `js-unicode-tables`                  | Writes `crates/js-text/src/unicode/tables.rs` (ID_Start, ID_Continue, Zs), `case_tables.rs` (case mapping, case folding, Cased, Case_Ignorable), `norm_tables.rs` (combining classes, decompositions, compositions, quick check), `property_names.rs` (the names and aliases of the properties of `\p{...}`) and `property_data.rs` (their code point sets and the emoji strings, `js_property_sets.py`) from the UCD 17.0.0; downloads `UnicodeData.txt`, `SpecialCasing.txt`, `CaseFolding.txt`, `DerivedCoreProperties.txt`, `DerivedNormalizationProps.txt`, `PropertyAliases.txt`, `PropertyValueAliases.txt`, `DerivedGeneralCategory.txt`, `Scripts.txt`, `ScriptExtensions.txt`, `PropList.txt`, `DerivedBinaryProperties.txt`, `emoji-data.txt`, `emoji-sequences.txt` and `emoji-zwj-sequences.txt` (the last two from the `emoji/` directory of 17.0.0) into `out/ucd/17.0.0/`. The generator checks its case runs against the source mappings. The generated files are formatted by `just fmt` (the committed `case_tables.rs`, `norm_tables.rs` and `property_names.rs` differ from the raw output only in formatting; `property_data.rs` is stable under `rustfmt`). |
| `just tools-check`           |                                        | ruff lint, ruff format check and pytest of `tools/`. |
| `just tools-fmt`             |                                        | Formats `tools/` and applies safe lint fixes. |
| `just snapshot DIR`          |                                        | Writes swb's rendering of all fixtures and layout tests to `DIR` (`tools/snapshot.sh`; no Python packages, no Chromium). |

`reference`, `layout-refs` and `probe` wait for `document.fonts.ready`
before they read boxes or take screenshots, so that web fonts are loaded.

Options:

- `capture --force`: replace an existing fixture (deletes its manifest,
  `files/`, `fixture.json` and `reference/`).
- `capture --with-swb` / `--no-swb`: record swb's requests too, or not.
  Default: use swb if the binary exists (with `--js`: do not use it).
- `capture --js`: JavaScript on; the fixture goes to `fixtures/js/NAME/`.
- `--swb PATH` (`capture`, `capture-missing`, `compare`, `perf`, `probe`): the
  swb binary. Default: the
  `SWB` environment variable, then `target/release/swb`, then
  `target/debug/swb`.
- `--system-fonts` (`capture`, `reference`, `layout-refs`): use the
  machine's fonts instead of the bundled test fonts. Do not commit output
  made with it.
- `compare --tolerance PX` (default 2), `compare --threshold N` (default 32),
  `compare --no-run` (compare the swb output that is already in
  `out/compare/NAME/`; useful to look at a report again or to test the tool;
  with `--full-page` or `--click` it also reuses Chromium's live capture there).
- `just compare` passes its arguments to a shell without quotes. A selector
  that contains a space or one of `#>[]:*()'"` needs a second level of
  quotes: `just compare bbc --click "'details > summary'"`,
  `just compare bbc --click "'#menu'"`.
- `reference --js`, `compare --js`: the fixtures of `fixtures/js/` with
  JavaScript on (`--all` means all of them). `compare --js` writes
  `out/compare/js-NAME/`, and `--update-scores` writes `out/js-scores.json`,
  never `fixtures/scores.json`. Not allowed with `--full-page` or `--click`.
- `compare --full-page`: also take full-page screenshots in both browsers and
  compare them (score, differing regions).
- `compare --click SELECTOR` (repeatable, applied in order): compare the page
  after the clicks. The output files get a state suffix. Not allowed with
  `--update-scores`.

The environment variable `SWB_ROOT` sets the repository root for the tools.
Normally they find it themselves.

## Files

### Page fixture

```
fixtures/pages/<name>/
  manifest.json      recorded responses (format: crates/net/src/fixture/mod.rs)
  files/             response bodies, named by SHA-256
  fixture.json       capture metadata
  reference/
    boxes.json       Chromium box dump (format below)
    screenshot.png   Chromium screenshot of the first viewport
```

The manifest format is specified in the module documentation of
`swb_net::fixture` (`crates/net/src/fixture/mod.rs`). The Python writer
produces byte-identical files: the same entry order, JSON formatting, body
file names and stored headers. Tests in `tools/tests/test_manifest.py` check
this against the expected file of the Rust test `manifest_format_is_exact`.

`fixture.json`:

```json
{
  "url": "https://senko.net/",
  "captured": "2026-10-02T12:44:02Z",
  "viewport": [1280, 800],
  "chromium": "148.0.7778.96"
}
```

- `url`: the captured URL as Chromium serialized it. `reference` and
  `compare` load this URL.
- `captured`: capture time, UTC, ISO 8601.
- `viewport`: viewport size in CSS px.
- `chromium`: the Chromium version used for the capture.
- `javascript` (optional): `true` for a fixture in `fixtures/js/`.
- `replay_patterns` (optional, a list of strings): URL patterns for requests
  that the fixture lacks. In Python replay, a request that is not in the
  manifest and matches a pattern gets the response of a recorded URL that
  matches the same pattern. `*` matches one path segment (one or more
  characters other than `/`, `?` and `#`), the rest is literal, the whole URL
  must match, and the method must be the same. Of several recorded URLs
  the one with the greatest URL gets served, with numbers compared as
  numbers. Use it for a segment that changes with time. Example: Craigslist
  requests `https://www.craigslist.org/static/d/<N>/www/manifest.js` with
  `N = floor(Date.now() / 300000)`, and the local `craigslist-posting`
  fixture has `"replay_patterns":
  ["https://www.craigslist.org/static/d/*/www/manifest.js"]`. The rule
  applies to `reference --js`, `js-survey` and the other Chromium replays
  (`Fixture.lookup`), not to `capture` (which needs exact matches to record).
  **swb's Rust replay does not read `replay_patterns` yet**; until it does,
  swb needs one manifest entry per URL (the fixture has 460 alias entries
  for `manifest.js`; do not delete them).

### Box dump

Chromium (`reference`, `layout-refs`) and swb (`--dump-boxes FILE`) write
this format. `compare` and the Rust layout tests read it.

```json
{
  "url": "https://senko.net/",
  "viewport": [1280, 800],
  "elements": [
    {"tag": "html", "rect": [0.0, 0.0, 1280.0, 878.55], "parent": null},
    {"tag": "head", "rect": null, "parent": 0}
  ]
}
```

- `url`: the URL that was loaded (the requested URL, before redirects). For
  layout tests: the file path relative to the repository root.
- `viewport`: `[width, height]` in CSS px. Integers or floats.
- `elements`: every element of the document in tree order: exactly the
  order of `document.querySelectorAll('*')`. Template contents are not
  included (they are not in the document tree).
  - `tag`: the local name in lowercase. Readers lowercase it again, so
    `clipPath` and `clippath` are equal.
  - `rect`: `[x, y, width, height]` of the element's border box in CSS px,
    in document coordinates (relative to the top left of the document, not
    of the viewport), rounded to 2 decimals (halves away from zero). If the
    element has several boxes (an inline element split over lines), the
    union (bounding box) of all of them. `null` if the element has no box.
  - `parent` (optional): the index of the parent element in `elements`,
    `null` for the root. Chromium dumps have it. `compare` uses it from the
    Chromium dump for the `relative` score and for element paths. swb
    writes it too. Readers must accept dumps without it.

The elements with a box are the elements whose `getClientRects()` is not
empty. In Chromium this includes some cases that need attention:

- `<br>` has a box of width 0 (the content area of its parent's font).
  Empty inline elements have a box of width 0. An inline element around a
  block-level child has a box as wide as its containing block (Chromium's
  block-in-inline).
- `display: none` elements and their descendants, `display: contents`
  elements, `<head>` and its content, `<wbr>`, `<template>`, `<input
  type=hidden>`, `<audio>` without `controls`, `<option>` and `<optgroup>`
  of a drop-down `<select>`, SVG `<defs>` and gradients have no box.
- `visibility: hidden` elements have a box.
- `<col>` and `<colgroup>` have a box (the area of their cells).
- SVG shapes, `<g>` and `<text>` have their bounding box.
- The content of a closed `<details>` element has boxes (Chromium
  implements the closed state with `content-visibility: hidden`).
- `<area>` has an empty box at the position of its map's image (or 0, 0).
- An `<img>` that failed to load and has no `alt` text has a 0×0 box; with
  `alt` text it is an inline box with the text.
- `<noscript>` content is parsed and displayed (JavaScript is disabled).

The writer puts one element per line, so diffs stay small. Readers accept
any JSON formatting.

### Scores

`fixtures/scores.json`:

```json
{
  "senko-net": {
    "geometry": 0.8123,
    "pixels": 0.9012,
    "relative": 0.9301,
    "size": 0.8501
  }
}
```

Keys are sorted, the indent is 2 spaces, and the file ends with a newline.
Values are rounded **down** to 4 decimals, so the stored value is never
higher than the exact value. A Rust test reads this file and fails if a
fixture's geometry score is lower than the stored value.

### Layout tests

`tests/layout/<name>.html` is a small HTML document that tests one feature.
`tests/layout/<name>.boxes.json` is its box dump from Chromium (viewport
800×600). Both are committed. The Rust test
(`crates/engine/tests/layout_refs.rs`) loads the HTML in swb and compares
the boxes with a tolerance of 1 px. Every layout test must pass, except the
ones listed in `tests/layout/known-failures.txt` (one name per line, `#`
comments allowed). A listed test that passes fails the run, so that the list
stays current.

An element with a `data-scroll="X Y"` attribute is scrolled to that offset
before the boxes are read, in Chromium (`swbtools layout-refs`) and in swb
(the Rust test), in tree order. The boxes of its content then show the
offset; a large offset (`data-scroll="9999 9999"`) tests that the scroll
range (the scrollable overflow) matches Chromium's.

The fixture ratchet test is `crates/engine/tests/fixture_scores.rs`. It
loads each fixture in `fixtures/scores.json` from its manifest and fails if
the geometry score is lower than the stored value (minus 0.002, because its
tag-sequence diff can pair elements differently from the Python tool).

To check the browser user interface without a display, render the whole
window (toolbar included) to a PNG:
`swb --headless --with-chrome --screenshot out.png --size 1100x700 URL`.

## Chromium settings

All Chromium runs (`capture`, `reference`, `layout-refs`) use the same
settings (`tools/swbtools/browser.py`):

- Headless shell, viewport 1280×800 for pages and 800×600 for layout tests,
  device scale factor 1.
- JavaScript disabled (`java_script_enabled=False`). swb does not run
  JavaScript. Playwright's `evaluate` still works: the tools use it to read
  boxes and to scroll. Only `capture --js`, `js-survey`, `reference --js`
  and the probe cases with `"javascript": true` turn JavaScript on
  (`new_context(javascript=True)`). `reference --js` also fixes the start
  time of the page clock (see "Compare with JavaScript on").
- Locale `en-US`, `prefers-color-scheme: light`,
  `prefers-reduced-motion: no-preference`, service workers blocked.
  Playwright's headless defaults give `hover: hover` and `pointer: fine`,
  and `scripting: none` matches because JavaScript is disabled. These are
  the values that swb's media queries use.
- Scrollbars are hidden (Playwright's headless default), so the layout
  viewport is the full 1280 px. swb has no scrollbar in its layout either.
- Before the tool reads the boxes and takes the screenshot, it turns off CSS
  animations and transitions (`animation: none; transition: none` in a
  constructed stylesheet, so that no element is added). Every element then
  has its base style, as in swb, which does not run animations. The
  screenshot hides the text caret.

### Fonts

By default Chromium uses only the bundled test fonts in `fixtures/fonts/`,
the same fonts as swb's `--test-fonts` (`FontContext::for_tests()`). Results
then do not depend on the fonts of the machine.

The tools set `FONTCONFIG_FILE` for Chromium to a generated file,
`out/fontconfig/fonts.conf`. It includes `fixtures/fonts/fonts.conf` (the
fonts and the family rules) and adds rendering settings:

```xml
<match target="font">
  <edit name="antialias" mode="assign"><bool>true</bool></edit>
  <edit name="hinting" mode="assign"><bool>false</bool></edit>
  <edit name="hintstyle" mode="assign"><const>hintnone</const></edit>
  <edit name="rgba" mode="assign"><const>none</const></edit>
  <edit name="embeddedbitmap" mode="assign"><bool>false</bool></edit>
</match>
```

Reasons, from measurements with Chromium 148 (the width of the same text in
Liberation Sans 16 px):

| Setup                                                   | Width       |
|---------------------------------------------------------|-------------|
| `fonts.conf` alone (fontconfig default: full hinting)   | 260 (whole pixels) |
| `fonts.conf` + `hintslight` or `hintnone`               | 262.328125  |
| System fonts (Arial, Debian default `hintslight`)       | 262.328125  |

With full hinting, Chromium rounds each glyph advance to whole pixels, so
text widths differ from the unrounded advances that swb computes. With
slight hinting or no hinting, the advances are linear (not rounded). Slight
hinting and no hinting give the same geometry; they differ only in the
glyph pixels. swb draws unhinted outlines, so the tools choose no hinting,
which gives smaller pixel differences. Grayscale anti-aliasing (`rgba
none`) matches swb.

Chromium command-line switches (`CHROMIUM_ARGS`):

- `--font-render-hinting=none`: without it, the headless shell still
  hints the advances of fallback glyphs (for example a `★` from DejaVu
  Sans: 14 instead of 14.34375), even with the settings above.
- `--disable-lcd-text`: grayscale anti-aliasing (no subpixel colors).
- `--force-color-profile=srgb`: plain sRGB output, no color management
  differences between machines.

Note: when no family of a `font-family` list exists, Chromium uses its
standard font, which is serif ("Times New Roman", here Liberation Serif),
not sans-serif. The test `test_layout_dump_uses_the_bundled_fonts` in
`tools/tests/test_chromium.py` checks this.

### Request interception

`capture`, `capture-missing` and `reference` intercept every request with
the Chrome DevTools Protocol `Fetch` domain (`tools/swbtools/routing.py`),
not with Playwright's `page.route`. Playwright calls a route handler only
for the first URL of a redirect chain, and the browser then fetches the
redirect target from the network. With `Fetch`, every hop pauses, so the
tools record each hop and serve each hop from the fixture.

Chromium gets the same response headers in capture and in replay: the
stored headers (`content-type`, `location`) and CORS headers. The fixture
does not store CORS headers, and swb's replay is to add the same ones
(ADR 0028 section 12). Without them Chromium would block cross-origin fonts
and requests from scripts.

- A request without an `Origin` header (a request in `no-cors` mode: images,
  classic scripts, stylesheets) gets `Access-Control-Allow-Origin: *`.
- A request with an `Origin` header (a CORS-mode request: web fonts,
  `fetch`, `XMLHttpRequest`, `crossorigin` elements) gets
  `Access-Control-Allow-Origin: <that origin>`,
  `Access-Control-Allow-Credentials: true`, `Vary: Origin` and
  `Access-Control-Expose-Headers` with the names of the stored headers.
  `*` would fail a request with the `include` credentials mode.
- A CORS preflight (`OPTIONS` with `Access-Control-Request-Method`) is
  answered by the tool with `204`, in replay and in capture: it is never
  fetched from the network and never recorded. The answer allows the
  requested origin (with credentials), method and headers.

**The `Fetch` domain pauses preflights.** Measured with Chromium 148 and a
page at `https://a.test/` (all `.test` names never resolve, so a request that
escaped the interception would fail) that runs
`fetch('https://b.test/data', {credentials: 'include', headers: {'X-Custom':
'1'}})`: `Fetch.requestPaused` fires for the document, then for
`OPTIONS https://b.test/data` (resource type `XHR`, with `Origin`,
`Access-Control-Request-Method: GET` and `Access-Control-Request-Headers:
x-custom`), then for the `GET`. Answering the `OPTIONS` with
`Fetch.fulfillRequest` (204) lets the `GET` proceed; the page gets the body.
So no other mechanism is needed.

In replay, a request for a URL that is not in the fixture (and that no
`replay_patterns` rule serves) fails, and the tool logs the URL.

## Workflows

### Capture a new target page

1. Add the URL to `docs/targets.md` with a fixture name.
2. Build swb (`just build`), so that the capture also records the resources
   that swb requests. Without the binary, only Chromium's requests are
   recorded.
3. `just capture https://example.com/page example-page`
4. `just substitute example-page` (see "Substitute copyrighted content"
   below). The repository is public: do not commit a fixture before this
   step.
5. `just reference example-page`
6. Check the fixture size (`just tools list`). Keep fixtures small: only
   the target page and its direct resources.
7. Look at `fixtures/pages/example-page/reference/screenshot.png`.
8. `just compare example-page`, then `just update-scores`.
9. Commit the fixture, the reference and `fixtures/scores.json`.

What `capture` does:

1. Chromium loads the URL from the network, scrolls down the whole page (so
   that lazy images load) and waits until the network is idle. The tool
   records every response, including each redirect hop, and passes it to
   the page.
2. If swb exists: `swb --headless --record fixtures/pages/<name> <url>`.
   swb's `RecordingFetcher` merges into the manifest. It replaces entries
   for URLs that it fetches again, so a dynamic page can get a newer HTML.
3. If swb ran: Chromium loads the page again. It gets known URLs from the
   fixture and fetches only missing URLs from the network. So the fixture
   has everything that Chromium needs for the final HTML.
4. Body files that no entry uses are deleted.

### Capture with JavaScript on

`just capture URL NAME --js` (`capture --js URL NAME`) captures a page as
above, with these differences:

- Chromium runs the page's scripts. `fixture.json` gets
  `"javascript": true`. Readers that do not know the field ignore it.
- The fixture goes to `fixtures/js/NAME/`, not `fixtures/pages/NAME/`. The
  directory is git-ignored and **never committed**: the fixture contains the
  sites' scripts, which are commercial code, and the repository is public.
  `substitute` does not apply to it. `reference`, `compare` and the other
  commands do not see it (they list `fixtures/pages/`).
- swb recording is off unless `--with-swb` is given (swb runs no scripts).
- Scripts can keep the network busy for ever (ads, analytics), and a page can
  grow while the tool scrolls. The scroll stops after 60 steps, the wait for
  network idle after the scroll is capped at 15 s, and the log (level
  `warning`) lists the requests that were still pending.
- The recording has the responses of one run. Scripts that request URLs with
  random or time-based parts (timestamps, session ids) get other URLs in
  replay, and replay fails them. A segment that counts up with time can be
  served by a `replay_patterns` rule of `fixture.json` (see "Page fixture").
- Cross-origin requests from scripts work in the capture and in the replay:
  the tool answers CORS preflights itself and reflects the request's
  origin (see "Request interception").

### Survey the scripts of a page

`just js-survey NAME...` (`js-survey --all`) measures what the scripts of
`fixtures/js/NAME` do, in Chromium with the test settings and JavaScript on
(`tools/swbtools/js_survey.py`). Every request is answered from the fixture;
requests that the fixture lacks fail and the survey lists them. There are
four runs per page (`--runs N` sets the runs of the timing measurement,
default 3):

1. Traced run. An init script (`js_tracer.js`, added with
   `context.add_init_script`, so it runs before any page script) wraps the
   members of the Web API interfaces with proxies and counts calls, reads
   and writes by phase: `parse` (until `DOMContentLoaded`), `load` (until
   the `load` event) and `after` (the scroll and the wait that follow). It
   also records arguments of a few members (event types and targets, tag
   names, selectors, timer delays, hosts of requests), scripts and iframes
   that scripts insert, the timer and animation-frame callbacks that ran,
   and DOM mutations (a `MutationObserver` from document start). Outside the
   page, the tool records the scripts that Chromium compiled (CDP
   `Debugger.scriptParsed`), console errors, uncaught exceptions and the
   requests by resource type.
2. Untraced run with JIT: Chrome DevTools Protocol `Performance.getMetrics`
   (script, task, layout and style durations and counts, nodes, heap) and
   the screenshot of the first viewport.
3. The same with `--js-flags=--jitless` (a separate browser launch). swb's
   interpreter is about 2 times slower than `node --jitless`, so this run
   predicts the script time of swb.
4. JavaScript off, from the same fixture: element count and screenshot. The
   tool writes the fraction of differing pixels.

Each run stops after 300 s with an error (a page that blocks its main
thread would otherwise stop the survey); the next page continues.

The output is in `out/js-survey/NAME/`: `survey.json` (all data),
`js-on.png`, `js-off.png`, `js-diff.png` and `summary.md` (also printed). The
results of the target pages are in [js-survey.md](js-survey.md).

Limits: a wrapper counts the calls of page scripts only (not those of the
engine, and not the members that the page cannot see); non-configurable
members (`window.location`, `document`, `top`) cannot be wrapped and are
listed by name; named-property access (`localStorage.key = value`) is not
counted; only the top frame is traced; the phase of a mutation is the phase
at the time of delivery; replay misses requests with new URLs.

### Substitute copyrighted content

`just substitute NAME...` (`tools/swbtools/substitute.py`) changes a
fixture so that the public repository does not publish photos or
commercial fonts:

- Every raster image (JPEG, PNG, WebP, GIF, BMP, ICO) becomes a
  placeholder of the same pixel size and format: a gradient with a 32 px
  grid, colored by a hash of the URL. Layout does not depend on the
  pixels; the grid shows scaling and position errors in the pixel score.
- Every font whose `name` table does not name a free license (SIL Open
  Font License, Apache License, Bitstream Vera license) becomes DejaVu
  Sans (Bold for weight 600 or more) in the same format. Not Liberation:
  the test fonts map the generic families and Arial, Helvetica and Times
  to Liberation, so a browser that ignored the web font would still
  match the reference.

Images are found by content type and by their first bytes. Animated
images become one frame, ICO files one image of the largest size. HTML,
CSS and SVG files do not change. The command is idempotent. Run it again
after `capture-missing`, then `just reference`. Free fonts that stay in a
fixture are listed in `THIRD_PARTY_NOTICES.md`.

After the replacement the command checks every body again and fails if
something may still not be published: an image that is not a
placeholder (Pillow cannot read it, or it has more than 100 million
pixels), an image format that the tool cannot write (AVIF, TIFF, JPEG
XL), a font without a free license, or a raster `data:` URL in a text
body. Replace such content by hand. `just substitute --check NAME` only
checks. The license check trusts the font's own `name` table.

`tools/tests/test_fixture_content.py` (part of `just check`) runs the
check on every fixture, so a fixture with photos or commercial fonts
cannot be committed by mistake. senko-net, hacker-news and
wikipedia-web-browser are exempt (the owner's site, a spacer GIF,
Wikimedia media under free licenses).

### Add missing resources to a fixture

When swb starts to load resources that it did not load at capture time
(for example after support for SVG images or `@font-face`), replay fails
for them and swb logs `no entry for GET ...`. A new capture would download
the page again, and a dynamic page (Hacker News) would change.
`just capture-missing NAME` keeps all existing entries:

1. `swb --headless --record-missing fixtures/pages/NAME URL` serves known
   URLs from the fixture and fetches only the missing ones from the
   network (`swb_net::ExtendingFetcher`). They are added to the manifest.
2. Chromium loads the page from the fixture and fetches only what is
   missing (normally nothing: the reference was made from the fixture).

If entries were added, run `just substitute NAME`, check whether
Chromium uses them and run `just reference NAME` if the rendering can
change.

### Regenerate references

Regenerate after a change of the Chromium settings, a Playwright upgrade,
or a change of the fonts: `just reference` (all fixtures) and
`just layout-refs` (all layout tests). Review the diff of the box dumps. The
fixture data (`manifest.json`, `files/`) does not change.

Box dumps are byte-identical between runs. Screenshots are not always: in
the Wikipedia fixture, 75 pixels of the search icon (an SVG) alternate
between two values that differ by 1. So `reference` keeps an existing
screenshot when the new one has the same size and no channel differs by
more than 2.

### Compare swb with Chromium

`just compare` (all fixtures) or `just compare senko-net`. For each fixture,
the tool runs:

```
swb --headless --replay fixtures/pages/<name> --test-fonts --size 1280x800 \
    --dump-boxes out/compare/<name>/boxes.json \
    --screenshot out/compare/<name>/screenshot.png <url>
```

(`tools/swbtools/swb.py` builds all swb command lines.) It prints one line
per fixture:

```
senko-net: geometry 0.8621 size 0.8793 relative 0.8621 pixels 0.9767 missing 5 extra 3  out/compare/senko-net/report.html
```

`out/compare/<name>/` then contains:

- `report.html`: the scores, the screenshots side by side with a diff
  image, the tag sequence differences, the worst-matching elements (path,
  both rectangles, deltas), and the elements with a box in only one
  browser.
- `boxes.json`, `screenshot.png`, `swb.log`: swb's output. With
  `--full-page` also `fullpage.png`, `reference-fullpage.png`,
  `reference-boxes.json`, `diff-fullpage.png`.
- `reference.png`: a copy of the Chromium screenshot. `diff.png`: the
  pixel difference (red).

The element path in the report has the form
`html > body:nth-child(2) > div:nth-child(3)` (`:nth-child` only when the
parent has more than one element child). Paste it into
`document.querySelector()` in a browser to find the element.

#### Full page: `compare --full-page`

Chromium's `reference` screenshot and the pixel score cover only the first
viewport. With `--full-page`, the tool also:

- replays the fixture in Chromium as `reference` does (same viewport, fonts,
  lazy-image scroll, wait for `document.fonts.ready`, animations off) and
  saves a full-page screenshot, `reference-fullpage.png`, and the box dump
  of that run, `reference-boxes.json`. It writes into `out/compare/NAME/`,
  never into `fixtures/`;
- runs swb a second time with `--full-page --screenshot`
  (`fullpage.png`);
- compares both images over the union of their areas with the same
  threshold as the viewport score. If the sizes differ, the report and the
  summary line say so, the score covers the larger area, and the part
  outside the smaller image counts as different. `diff-fullpage.png`
  shows the difference (red) on the union area;
- lists the differing regions: connected areas of differing pixels, where
  pixels within 8 px of each other (8-connected blocks of 8 px) form one
  region. Each row has the tight bounding rectangle (x, y, w&times;h in
  px), the number of differing pixels and the path of the smallest
  Chromium box that contains the rectangle. The table shows the 50 regions
  with the most differing pixels. The box is only geometry: it can be a box that is
  not drawn, for example the closed `details` menu of the BBC fixture, whose
  hidden boxes overlap the page.

The summary line adds `fullpage SCORE regions N`. The score is not stored in
`fixtures/scores.json`.

**Does Playwright's full-page capture change Chromium's layout?** No.
Measured on the `bbc` fixture (12,697 px high, 2,745 elements) and on a
synthetic page with `height: 100vh`, `position: fixed; bottom: 0` and
`position: fixed; top: 10px` boxes: the box dump, `innerHeight` (800),
the computed `max-height: 80vh` and the positions of the fixed and sticky
boxes are identical before and after the capture. The image shows the page
from its top with the layout of the viewport: `100vh` stays 800 px and fixed
boxes are drawn at their positions in the first viewport, as in swb's
`--full-page`. `compare --full-page` reads the box dump again after the
capture and logs a warning if it differs.

#### States: `compare --click SELECTOR`

With one or more `--click`, the tool loads the page in both browsers, clicks
the first element that matches each selector in turn (Playwright in Chromium;
`dom.querySelector` and `input.click` with `nodeId` in swb, through the
automation API), waits until the page settles (Chromium: 200 ms and the fonts;
swb: `page.waitForLoad` and the load state), scrolls to the top and takes the
boxes and the screenshots. The state replaces the plain run: the scores are
those of the state. Chromium's boxes and screenshot are a live capture, not
the committed reference, because the reference has only the initial state.
`--full-page` works together with it. It fails if a selector matches nothing
or a click is not possible (Chromium waits 5 s for the element). With
JavaScript off, the clicks only change what the browser does by itself: toggle
`details`, check a checkbox, focus, follow a link.

All files of a state run end with `-click-SLUG-HASH` (SLUG from the
selectors, HASH from the whole list), so that they do not replace the plain
run's files: `boxes-click-details-summary-5799ee.json`,
`screenshot-...png`, `fullpage-...png`, `reference-...png` (Chromium's
viewport screenshot), `reference-boxes-...json`, `reference-fullpage-...png`,
`diff-...png`, `diff-fullpage-...png`, `report-...html`. The summary line
prints the report path.

To see which images swb requests (for example to check image source
selection), run swb with `RUST_LOG=swb_engine=debug` and look for the
`image request:` lines.

When a change improves the scores, run `just update-scores` and commit
`fixtures/scores.json` with the change. The tool logs every score that goes
down. Scores only go up unless the commit message explains why.

### Compare with JavaScript on

`reference --js NAME...` and `compare --js NAME...` do for the fixtures of
`fixtures/js/` (local only, see "Capture with JavaScript on") what
`reference` and `compare` do for `fixtures/pages/`: Chromium with JavaScript
on renders the replay, swb renders the same replay, and the tool scores swb
against Chromium. The references are not committed. They are in
`out/js-reference/NAME/` (`boxes.json`, `screenshot.png`), and the scores are
in `out/js-scores.json` (`compare --js --update-scores`; never
`fixtures/scores.json`). `compare --js` writes `out/compare/js-NAME/`; the
report and the scoring are the same as for the other fixtures (tolerance,
threshold, first viewport), and the summary line starts with `js-NAME`.
`--full-page` and `--click` do not work with `--js` yet.

**Chromium side** (`reference --js`; `tools/swbtools/reference.py`). Test
settings (fonts, viewport from `fixture.json`, no animations, caret hidden),
JavaScript on, every request answered from the fixture (CORS answers,
`replay_patterns`; see "Request interception" and "Page fixture"). The tool
logs the requests that the fixture lacks and the console errors and uncaught
exceptions of the page. The page is done when:

1. the `load` event has fired;
2. the network has been idle for 500 ms, waiting at most 15 s (a warning
   says if the cap was reached);
3. 1 s has passed (`JS_SETTLE_MS`), for timers and layout that scripts start
   late;
4. the web fonts have loaded and CSS animations and transitions are off.

The page is not scrolled: scripts react to scrolling (sticky headers, lazy
content), and swb does not scroll either. Then the tool writes the box dump
and the first-viewport screenshot. Measured on 2026-10-22 with the four M8
fixtures (senko-net, hacker-news, wikipedia-web-browser, craigslist-posting),
three runs each, the last from empty output: the box dumps are
byte-identical (65, 821, 4169 and 283 elements) and the screenshots are
identical (pixel score 1.0000).

**Start time.** Both browsers must see the same `Date`, or the pages differ
by the date (Craigslist shows the posting age, and requests `manifest.js`
with a number from the clock). The page clock of `reference --js` starts at
the `captured` time of `fixture.json` and then runs at real speed:
`Date.now()` is the real time plus a constant offset
(`browser.fix_start_time`, an init script that wraps `Date` in a `Proxy`).
Playwright's `page.clock` is not used. Measured with Chromium 148
(`clock.install`, `clock.set_fixed_time` and a plain page, checking the
timer functions, `Date`, `performance.now` and `requestAnimationFrame` over
300 ms): `install` replaces `setTimeout` and the other timer functions,
`requestAnimationFrame`, `Date` and `Date.prototype.constructor` with
JavaScript implementations (`setTimeout.toString()` shows the wrapper) that
run the timers on Playwright's fake clock; `set_fixed_time` replaces `Date` the
same way and freezes it (`Date.now()` does not advance in 300 ms, so
elapsed-time code sees 0). The order of the timer callbacks stayed the same
in the test, but the page cannot tell that for all its timers, so the tool
changes only `Date`. The wrapper keeps the statics, the prototype,
`instanceof`, subclassing and `Date.prototype.constructor === Date`
(`tools/tests/test_js_tools.py`). Not covered: `Intl.DateTimeFormat().format()`
without an argument, `Temporal`, and `performance.timeOrigin`. When swb gets
`--now` (M8 feature 5b), `compare --js` passes the same `captured` time to
swb; it does not pass it now.

**swb side** (`compare --js`). `swb --help` lists the supported options (the
result is cached per binary, `swb.supported_flags`). With `--js` in the help,
the tool passes `--js` to swb. Without it (before M8 feature 2), the tool
prints a warning, runs swb without JavaScript, and appends
`(swb ran without JavaScript: no --js option)` to the summary line, so the
scores of such a run are a baseline of the JavaScript-off swb against the
JavaScript-on Chromium.

Baseline of 2026-10-22 (swb without `--js`; geometry, pixels):
senko-net 1.0000, 0.9990; hacker-news 1.0000, 0.9986;
wikipedia-web-browser 0.1810, 0.9735; craigslist-posting 0.0000, 0.6897.

Since M8 feature 2 swb has `--js` (the scripting flag; no script runs
yet). Scores of 2026-10-22 with it: senko-net 1.0000, 0.9990; hacker-news
1.0000, 0.9986; wikipedia-web-browser 0.1810, 0.9735; craigslist-posting
0.0000, 0.7747.

### Add a layout test

1. Write `tests/layout/<name>.html`: one feature, a `<!DOCTYPE html>`, a
   comment that says what it tests, fixed sizes where possible, and the
   generic families `serif`, `sans-serif` or `monospace` (or Arial, Times
   New Roman, Courier New, which map to the bundled fonts). A quirks-mode
   test has no doctype; make its content taller than the viewport,
   because swb does not implement the body and html fill-viewport quirks.
2. `just layout-refs <name>`
3. Check `tests/layout/<name>.boxes.json` (for example, open the HTML in a
   browser and compare a few rectangles).
4. Commit both files.

### Measure line break opportunities

swb's line breaking (`crates/text/src/linebreak.rs`) follows UAX #14 with
the tailorings that Chromium has. The rules come from measurements:

1. `uv run --project tools swbtools linebreaks` (about one minute) renders
   strings in Chromium, each as the text of a `<div>` with `width: 0`,
   `white-space: pre-wrap`, 10 px font and 100 px line height. The line
   breaks at every opportunity. The line of each code point is the
   vertical center of its last rectangle (`Range.getClientRects()`), so the
   tool finds the positions where a new line starts. The same text with
   `width: 100000px` gives the forced breaks and the width of each code
   point: positions inside text of zero width are unknown, because
   Chromium does not need to break there. A text whose number of lines
   does not match is unknown too.
2. It writes the matrices of character pairs and the list of strings to
   `crates/text/tests/linebreak/` (format: `README.md` there). The case
   sets are in `tools/swbtools/linebreaks.py`; add strings there.
3. `uv run --project tools swbtools linebreak-tables` writes
   `crates/text/src/linebreak/tables.rs`: Unicode 17.0 data (downloaded
   into `out/ucd/`) and the Latin-1 pair tables from `latin1.txt`.
4. `cargo test -p swb-text --test linebreak` compares swb with every
   measured position. A difference must be listed with its reason in
   `known-differences.txt`.

After a Chromium upgrade, measure again, generate the tables, and check the
differences.

### Check that a refactoring does not change rendering

`just snapshot DIR` writes the layout dump (`--dump-layout`), the DOM dump
and full-page screenshots (1280×800, 700×900, and 1280×800 at scale 2) of
every page fixture, and the layout dump and a screenshot of every layout
test. The output is deterministic.

1. Before the change: `just snapshot /tmp/before`.
2. After the change: `just snapshot /tmp/after`.
3. `diff -rq /tmp/before /tmp/after` must print nothing.

The layout dump rounds to 2 decimals; the screenshots catch smaller
differences that reach the pixels. Interaction, forms and history are not
covered: their tests must pass.

### Measure Chromium behaviour

`measure` regenerates data that is checked into the repository. To answer
a question during development, use `probe` (next section), not a new
script.

Some data in swb comes from black-box measurements of Chromium, not from
its source code (agents do not read Chromium's source: ADR 0021; data from
its LGPL files is not allowed: ADR 0003).
`swbtools measure` (`tools/swbtools/measure.py`) repeats these
measurements. Each one loads generated pages in Chromium with the
settings above (bundled fonts, JavaScript disabled) and prints the
result. The swb code next to the data names the measurement.

| Measurement           | What it measures | Data in swb |
|-----------------------|------------------|-------------|
| `text-field-families` | For about 450 `font-family` values (the generic families, common Windows, macOS and Linux family names, names that start with `#` or `.`, other spellings): the content width of `<input size=20>` at 16px, compared with the same field with an unknown family first (the average character width rule) and with 20 times the width of `0`. Prints the families whose fields use the width of `0`. | `ZERO_WIDTH_FAMILIES` in `crates/layout/src/control.rs` |
| `font-size-keywords`  | The computed `font-size` of `xx-small` to `xxx-large` with `serif` (medium: 16px) and `monospace` (medium: 13px), in standards mode and in quirks mode. | `KEYWORD_SIZES_*` in `crates/style/src/values/keywords.rs` |
| `ua-styles`           | The computed styles of form controls, `option`, `label`, `fieldset`, `marquee`, `meter`, `progress`, `output`, `ruby`, `rt` and `map` (the properties whose values differ from a `<span>` next to the element, in a parent with unusual inherited values), their `::placeholder`, the `frameset` and `frame` of a frameset document, and the system colors. | The rules of `crates/style/src/ua.css` that say "measured" (form controls, widgets, ruby, framesets) |
| `picture-sources`     | The image that a `<picture>` shows when its `<source>` has a `media` that does not match, an unsupported `type`, or both. | `source_candidate` in `crates/engine/src/image_source.rs` |
| `font-size-sweep`     | The sizes that Chromium shapes text with, for every CSS font size from 9.000 to 24.000 px in steps of 0.001 px (15,001 sizes, about 2 minutes): each size alone on a new page in a new browser context, with 20 times `0` in three families and 100 times `AV` in Liberation Sans. From the widths it derives the kerning size and the advance size. Then one page with all sizes shows the shared font cache (see ADR 0006). Writes the two data files, and prints how many sizes differ from swb's rule. Runs only when named. | `chromium_sizes` and `font_scale` in `crates/text/src/shape.rs`; the files `crates/text/tests/chromium-font-sizes.txt` (ranges of CSS sizes with the same sizes) and `chromium-font-widths.txt` (the widths of every 13th size) |

The pages are files in a temporary directory, loaded by navigation: a
quirks mode page from Playwright's `set_content` after a standards mode
page in the same tab can keep the standards mode font size table. `tools/tests/test_measure.py` runs the measurements and compares
the family list and the keyword tables with swb's source code, so `just
tools-check` fails when they no longer match Chromium (for example after a
Playwright upgrade). It also measures samples of the font sizes and
compares them with the two data files, which the Rust tests
(`font_size_rule_matches_chromium`, `font_widths.rs`) compare with swb.

### Probe Chromium behaviour

`swbtools probe` (`tools/swbtools/probe.py`) loads small HTML documents in
Chromium and prints what you ask for. Do not write one-off Playwright
scripts: add a case to a JSON file. Chromium has the settings above (bundled
fonts, JavaScript disabled, device scale factor 1). Each page is a file in a
temporary directory, loaded by navigation.

```json
{"viewport": [800, 600],
 "cases": [
   {"name": "bfc-next-to-float",
    "html": "<!DOCTYPE html><style>...</style><div id=a>...</div>",
    "viewport": [800, 600],
    "measure": [
      {"boxes": "#a, #a > p"},
      {"rects": "span.x"},
      {"style": "#a", "props": ["width", "line-height"]},
      {"js": "document.scrollingElement.scrollHeight"}
    ]}]}
```

- `viewport` (file and case, optional): default 800x600.
- `html`: the whole document, used verbatim. Without a doctype the page is
  in quirks mode, on purpose.
- `boxes`: the border box of each element that the CSS selector matches, in
  document coordinates, as in the box dump (union of the client rects,
  rounded to 2 decimals).
- `rects`: each `getClientRects()` rectangle of each match (the line
  fragments of an inline element).
- `style`: the `getComputedStyle` values of `props`.
- `js`: the JSON value of an expression.
- `ink`: what Chromium paints in each matching element's border box (a
  screenshot of the area): `ink`, the sum of the darkness of its pixels
  (by luminance, 1 for black), and `bbox`, the box of the pixels that are
  not white, relative to the element. It shows synthetic bold (more ink)
  and slanted glyphs where the geometry does not change.
- `files` (case, optional): files to copy next to the page, as
  `{"published name": "source path"}`; a relative source path is relative
  to the repository root (for example the fonts of `@font-face` rules:
  `tools/probes/web-fonts.json`).
- `time_zone` (file and case, optional): the IANA name of the time zone of
  the page (`"Europe/Berlin"`), for the results of `Date`. Without it the
  page has the machine's zone, so a case that shows `Date` results must
  set it. Chromium gets it as the `timezone_id` of the browser context;
  the cases of a zone share one context (`tools/probes/js-date.json`
  runs in seven zones).
- `javascript` (file and case, optional): `true` runs the scripts of the
  page (default `false`). The case runs in a Chromium context with JavaScript
  on; the contexts are keyed by time zone and this flag. The queries run
  after the `load` event and the web fonts; a script that works later needs
  a `js` query that waits (a returned promise is awaited). `@media
  (scripting)` matches. The clock is not fixed: a case must not depend on
  the date. Without the flag, scripts do not run and `js` queries only read
  the DOM that the HTML made.

Output, one line per value: `CASE  KIND  LABEL  VALUES`. A label is the
selector and the 1-based index among its matches (`#a > p[2]`; a selector
list is in parentheses: `(#a, #b)[2]`). `rects` adds `#N` for the fragment.

```
bfc-next-to-float  boxes  (#f, #b, #b > p)[2]  x=108 y=8 w=684 h=52
bfc-next-to-float  swb    (#f, #b, #b > p)[2]  x=108 y=8 w=684 h=52  dx=0 dy=0 dw=0 dh=0
```

`--json` prints the same data as JSON. `--case NAME` (repeatable) runs only
the named cases. A bad selector or expression gives an `error:` value, the
other queries still run, and the exit status is 1.

`--with-swb` also renders each case in swb (`--headless --test-fonts
--size WxH --dump-boxes`; binary: `--swb PATH`, `$SWB`,
`target/release/swb`, `target/debug/swb`). For each `boxes` element it
reads the same index of `document.querySelectorAll('*')` from swb's dump
and prints swb's box and the deltas below the Chromium line, with `!` if a delta is above
`--tolerance` (default 1 px). If the tag at that index differs, or only one
side has a box, it says so. For `ink` queries swb also writes a
full-page screenshot, and the probe computes swb's ink and bounding box
in Chromium's border box of the element: a line `swb ink=.. bbox=..
dink=..` follows, with `!` if the ink differs by more than 5 % (at
least 2) or an edge of the bounding box by more than the tolerance.
`rects` and `style` are Chromium only, and so is `js` in a case without
`"javascript": true`.
Exit status 1 if a compared box is outside the tolerance, a tag differs, or
swb fails; so `just probe --with-swb FILE` works as a quick check.

**JavaScript cases with `--with-swb`.** swb runs the case with `--js`. A swb
without `--js` does not run the case at all: the probe prints `not supported`
for each swb line of the case (`boxes` and `js` rows), and this is not a
failure (exit status 0 if nothing else differs). The `js` queries of the case
also go to swb, as `--eval=EXPR` (one argument per query, in the order of the
queries; the `=` form keeps an expression that starts with `-`, such as `-0`,
from being read as an option), and the probe prints swb's value under the Chromium line, with `!` and exit
status 1 if the printed values differ. A swb with `--js` but without
`--eval` prints `not supported` for the `js` rows only.

```
case  js     document.title  "done"
case  swb    document.title  "done"
case  js     1 + 1  2
case  swb    1 + 1  3  !
```

**The `--eval` contract** (what M8 feature 4c implements; ADR 0028
section 17). `swb --headless --js [--eval=EXPR]... URL`. swb must accept the
`--eval=EXPR` form with any EXPR, also one that starts with `-`. Directly
after the page's `load` event (Chromium's probe evaluates when navigation
returns at `load`, with no settle time; tasks that are due later, such as
timers, may not have run, so a case that needs them waits in the expression
with a promise), swb evaluates each EXPR in order as a classic script in the
global scope of the page (the value is that of the last expression
statement, as for Chromium's `Runtime.evaluate`; the evaluation is not in a
function), and, if the result is a promise, waits for it (as `await`: a
settled promise gives its value, a rejected one gives an error). It prints
exactly one line to stdout per EXPR, in order, and nothing else to stdout
(logs go to stderr):

- `{"value": V}` for a value, or
- `{"error": "NAME: message"}` for a thrown value (or a rejected promise).
  The probe compares the text after removing Playwright's `Page.evaluate: `
  prefix from Chromium's message, so `TypeError: x is not a function` must
  be the same in both; other thrown values (not an `Error`) are not
  specified.

An `error` answer does not change swb's exit status. If the page does not
load, swb prints no lines and exits with a status other than 0. The probe
reads stdout after swb exits, so buffering does not matter.

V is JSON text for the same domain that the probe prints for Chromium:
`undefined` and `null` are `null`; `true`, `false`; a string with JSON
escapes (non-ASCII characters may be literal); an array (a hole or an
`undefined` element is `null`); a plain object (keys in the order of
`Object.keys`: integer keys ascending, then insertion order). Numbers are
the shortest round-trip decimal as `Number.prototype.toString` prints them
(`1`, `1.5`, `1e+21`), with these forms outside JSON: `NaN`, `Infinity`,
`-Infinity`, and `-0.0` for negative zero. The probe reads the line with
Python's JSON reader, which accepts these tokens, and prints the value in
its own form, so spacing and key quoting do not matter. The Chromium side
prints (Playwright's serialization): `undefined` as `null`, `NaN`,
`Infinity`, `-Infinity`, `-0` as `-0.0`, `1e21` as `1e+21`, a BigInt as an
integer, a function as `1` (measured), a symbol as `null`, a `Map` as `{}`;
a `Date` or an `Error` object gives an `error: the value is not JSON` row.
Queries must therefore return primitives, arrays and plain objects of these (convert other values in the expression: `String(x)`,
`JSON.stringify(x)`, `[...map]`); values outside this domain are not
compared. A line that does not follow the contract is printed as `invalid
answer: LINE` and counts as a difference, and so does a wrong number of
lines (`swb printed N answers for M --eval expressions`).

Case files that other people should reuse go in `tools/probes/`
(`example.json` shows the format). Scratch case files can live anywhere.
`tools/tests/test_probe.py` runs the example file.

## Hostile-page set

swb must never panic, hang or exhaust memory on content from the network.
The set in `tools/swbtools/hostile_cases.py` has one page for each limit
that an ADR documents (floats, tables, grid, masks, transforms, scroll
containers, SVG images, inline SVG, counters, custom properties, box depth, web
fonts), each sized
just past the limit so that the limit acts, and some generic pages (deep
nesting, very long words, `1e30px` lengths, thousands of `:has()` rules, long
`var()` chains, huge lists).

```
just hostile                    # all cases; builds the release binary first
just hostile floats-many        # named cases
uv run swbtools hostile --list  # cases, limits and descriptions
uv run swbtools hostile --keep  # keep the pages of passing cases
```

For each case the runner writes the page to `out/hostile/NAME/` (`index.html`,
extra files, `swb.log`, the box dump and the screenshot), runs
`swb --headless --test-fonts --size 1280x800 --dump-boxes ... --screenshot ...`
on it, one case at a time, and prints one line: name, wall time, peak
resident memory, result. The directories of passing cases are deleted unless
`--keep` is given.

| Result          | Meaning |
|-----------------|---------|
| `ok`            | Exit status 0, within the time and memory limits. |
| `slow`          | Over the time limit (default 5 s), or killed at twice the limit. |
| `memory`        | Over the memory limit (default 1 GiB), or killed at the cap. |
| `panic`         | Exit status 101 or "panicked" in swb's log (also panics that swb catches). |
| `error`         | Another failure (a crash by a signal, a non-zero exit status). |
| `inactive`      | The case sets `expect_log` and swb did not log that warning: the limit did not act (the limit changed, or the case does not reach it). |
| `known failure` | The case is marked as a known failure and fails. This is a pass. |
| `unexpected pass` | A known failure that now passes. This is a failure: remove the mark. |

The exit status is 1 if a case fails that is not a known failure, or a known
failure passes (like `tests/layout/known-failures.txt`). A watchdog polls
`/proc/PID/status` every 50 ms and kills swb (the whole process group) after
twice the time limit, or when its memory exceeds twice the memory limit (at
most 4 GiB), so that a bug cannot take the machine down. Times have the
resolution of the poll interval. The peak memory is the kernel's high-water
mark of the resident set; for runs shorter than a few polls it is the
`ru_maxrss` of the child, which includes the runner's own memory (about
50 to 90 MiB too high). A run of the whole set takes about 30 s on a release
build.

The limits depend on the machine, so the set is not part of `just check`.
Implementers run `just hostile` after a change to layout, style, paint or
the DOM, and reviewers run it as part of the review checklist. A failing case
is either a regression or a limit that the change should keep.

To add a case, write a function in `hostile_cases.py` that returns a
`Page(html, files)` and decorate it with `@case(description, ...)`. The
description names the limit and the ADR. Size the page just past the limit
(at most 20 MB; `tools/tests/test_hostile.py` checks this and that names are
unique). Options: `time_limit_s` and `rss_limit_mib` where an ADR documents
a higher cost, `expect_log` (a part of the warning that swb logs when the
limit acts), `swb_args` (for example `--full-page`, `--scale 8`, `--js`) and
`known_failure="reason"` for an open bug. Put the bug in the backlog in
`docs/roadmap.md`; when it is fixed, `just hostile` reports `unexpected pass`
and the mark goes.

## Scores

`compare` computes the scores in `tools/swbtools/scoring.py`.

1. **Alignment.** If the tag sequences of both dumps are equal, element *i*
   of Chromium pairs with element *i* of swb. Otherwise the tool aligns the
   sequences: the common prefix and suffix pair directly; the rest is
   aligned with Myers' difference algorithm (the fewest insertions and
   deletions; on ties, deletions from the Chromium sequence come first).
   Only elements with equal tags pair. If the rest needs more than 2000
   edits, `difflib.SequenceMatcher` aligns it instead (faster, but it can
   pair the wrong copies of repeated structures such as table rows). The
   report lists the blocks that differ.
2. **Element scores.** Each Chromium element that has a box counts once.
   It passes if its swb partner has a box and the values differ by at most
   the tolerance (default 2 px):
   - `geometry`: x, y, width and height.
   - `size`: width and height.
   - `relative`: x and y relative to the nearest ancestor (in the Chromium
     tree) that has a box in both browsers, and width and height. One early
     vertical shift then fails only the element that moved, not every
     element below it. Without such an ancestor, absolute positions are
     compared.

   Each score is the fraction of passing elements. With no boxes, the
   scores are 1.
3. **missing**: Chromium elements with a box whose partner has no box, or
   that have no partner. **extra**: swb elements with a box whose partner
   has no box, or that have no partner.
4. **pixels**: the fraction of pixels of the first viewport (Chromium's
   screenshot size) where no color channel differs by more than 32 (0 to
   255). Pixels that are outside swb's screenshot count as different.

Fonts and anti-aliasing differ between the browsers, so `pixels` does not
reach 1. `geometry` is the main metric.

## Known limitations

- Dynamic pages (Hacker News) change between requests. A capture is one
  snapshot. swb's recording pass can replace the HTML that Chromium
  recorded; the third capture step makes the fixture complete again.
- The fixture stores only `content-type` and `location`. Responses that
  depend on other headers (for example `Content-Security-Policy`) behave
  differently from the live site, in both browsers. Replays have no
  cookies.
- Record fixtures logged out: a fixture stores the page as the recording
  session saw it, and a page recorded after a login contains session data
  (on Hacker News, the `auth=` tokens of the `logout` and `vote` links).
- Chromium loads web fonts from the fixture, and so does swb (ADR 0022).
  `local()` sources and the metric descriptors (`size-adjust` and the
  overrides) are not supported yet, so fallback faces that use them
  differ.
- The `--full-page` score of `ars-technica` is not reproducible: the
  placeholder images of the article cards differ between Chromium runs
  (0.9558 to 0.9966 on the same swb output; swb's screenshots are
  identical). Compare its text and layout regions, not the score.
- Glyph pixels differ slightly from Chromium's (FreeType): about 1–3 % of
  the ink of a glyph, in both directions, and edges up to about 0.03 px
  away. Glyph positions (advances, kerning, letter-spacing, the quarter
  pixel steps) match; see the `glyph-positions` probe cases.
- Alignment works on tags only. When swb builds a different DOM (for
  example template contents in the tree, or different parser recovery),
  elements in the differing blocks count as missing or extra.

## Automation tests

`crates/automation/tests/headless.rs` starts a `HeadlessBrowser` on a free
port in a thread of the test process and drives it with
`swb_automation::Client` over WebSocket: the senko.net fixture (navigation,
hover, focus, selection, scrolling, screenshots, the box dump), typing
into a form in a local file and submitting it, `cookies.get` and
`cookies.clear`, `page.waitForLoad` timeouts, the error cases of the
protocol, the `Origin` check and the server limits (connections, request
size). `crates/engine/tests/interaction.rs` tests the same interactions
on the engine API directly, with small pages.

`tools/tests/test_automation.py` tests the Python client against the swb
binary (skipped if it does not exist). `just tools-check` (part of
`just check`) builds the release binary first, so the test uses the
current source. The protocol is documented in
[automation.md](automation.md).

## JavaScript engine tools

The shell `swb-js` is a binary of the `js` crate (ADR 0026 section 13).
`cargo run --release -p swb-js -- [OPTIONS] FILE...` or `-e CODE` runs
scripts in one global scope. `print` and `console.log` write to stdout.
`$262` has `evalScript`, `gc`, `detachArrayBuffer` and `global`;
`createRealm` and `agent` are not available. Options:
`--heap-limit MIB`, `--stress` (GC at every safepoint, fewer after the
first 65,536; ADR 0026 section 8), `--time-limit MS`,
`--disassemble` (bytecode listing), `--compile-only`,
`--time-zone ZONE` (the local zone of `Date`: `UTC`, an IANA name or a
POSIX rule; default the system zone), `--now MS` (a fixed clock for
`Date.now()`; default the system clock). The subcommands are `test262`,
`bench-compile DIR` (parse and compile time of the scripts in DIR) and
`--dump-ast FILE` (the syntax tree, scopes and references). The promise jobs
run after each script, under the same time limit. A promise that was
rejected without a handler and still has none after the jobs is
reported on stderr as Chromium's console shows it, `Uncaught (in
promise) TypeError: message` and the lines of the error's stack (`Uncaught
(in promise) 42` for other values), and ends the run with exit code 1,
after all of them are reported (Node.js 22 also exits with 1). Exit
codes: 0 success; 1 uncaught error (`Uncaught TypeError: message` and
`    at FILE:LINE:COLUMN` on stderr) or unhandled rejection; 2 compile
error; 3 termination (time or heap limit); 64 bad command line.

### test262

`just test262 [PATH...]` runs the tests with the runner `swb-js test262`.
test262 lives in `out/test262` (ignored by the repository); if the
directory is missing, the recipe clones it and checks out the commit named
in the `justfile` (`test262_commit`, the only place). `TEST262_DIR` uses
another directory and `TEST262_URL` another repository.

- `crates/js/test262/subset.txt`: the features in scope and the groups
  (`dir` lines) that run. A group is the first four components of a test's
  directory, for example `test/language/expressions/addition`. A `dir`
  line can also name a directory or a file below a group
  (`test/built-ins/TypedArray/prototype/set`): then only the tests at or
  below it run, and they count for their group in `scores.json` (M7 11b
  and 11c: the group `test/built-ins/TypedArray` runs in parts, one `dir`
  line for each method of `%TypedArray%.prototype`).
- Every test gets `sta.js` and `assert.js`, then the files of its
  `includes` in the test's order (each file once; the runner reads each
  file once per run). The accepted files are `propertyHelper.js`,
  `compareArray.js`, `isConstructor.js`, `fnGlobalObject.js`, `nans.js`,
  `decimalToHexString.js`, `nativeErrors.js`, `asyncHelpers.js`,
  `promiseHelper.js`, `regExpUtils.js`, `compareIterator.js`,
  `proxyTrapsHelper.js`, `detachArrayBuffer.js`,
  `byteConversionValues.js`, `testTypedArray.js`,
  `resizableArrayBufferUtils.js`, `testAtomics.js`, `assertRelativeDateMs.js`
  and `dateConstants.js` (`HARNESS_INCLUDES` in `run.rs`; the
  test262 commit has no `testBigIntTypedArray.js`, its functions are in
  `testTypedArray.js`). A test with the
  flag `async` gets `doneprintHandle.js` before its includes; the runner
  runs the jobs after the test (as the shell does) and passes it only when
  it printed `Test262:AsyncTestComplete` and no
  `Test262:AsyncTestFailure:` line. A promise rejected without a handler
  does not fail a test. A test is
  skipped if it needs another harness file, has the flag `module`
  or `CanBlockIsTrue`, has a
  feature outside the list, uses `$262.createRealm`, `$262.agent` or
  `new SharedArrayBuffer` (Chromium 148 has none on a normal page; the
  tests of `Atomics` that make one without the feature tag), or expects an
  error in the `resolution` phase.
  Every run has the local time zone `EST5EDT,M3.2.0,M11.1.0` (`TEST_ZONE`
  in `run.rs`: the US Eastern rule for every year, no database needed,
  so the scores do not depend on the machine; ADR 0027) and the system
  clock.
  The others run in sloppy and strict mode as the flags say (`onlyStrict`,
  `noStrict`, `raw`); a test passes if all of its modes pass. A negative
  test checks the phase (`parse`/`early`: a compile error; `runtime`: an
  uncaught error) and the name of the thrown value's constructor.
- Results per group: pass, fail, unsupported (the engine said "not
  supported yet") and skipped. The runner starts `--jobs` worker threads (default: the number of
  CPUs), each with an 8 MiB stack. A worker takes the next test, runs it
  in each mode with a fresh runtime, and reuses its thread for the next
  test; a panic is caught, counts as a failure named "panic" and there must
  be none. A test file that cannot be read is a failure with the I/O
  message. Options: `--all` (all of `test/language/`, to choose the subset),
  `--report FILE` (one TSV line per test and mode), `--stress`, `--jobs`,
  `--time-limit MS` (default 10000; with `--stress` 300000, because the
  collections make a loop of a million calls slow),
  `--heap-limit MIB` (default 256),
  `--top N`.
- The groups of `test/built-ins/` have three components
  (`test/built-ins/Array`); all others four.
- `crates/js/test262/scores.json` has the pass count per group. The run
  prints the change and exits with 1 if a count went down (or a test
  panicked, or no test was found, or the file is corrupt; a missing file
  starts empty with a warning). Only the groups that the run covers
  completely are compared, so a run of one file never reports a
  regression. `just test262 --update` writes the counts of the groups of
  the subset and refuses a partial run (a path to a file or below a
  group). The file may only go up; commit it with the change that raised it.

Parse-only mode: `just test262 --parse-only [PATH...]` (`swb-js test262
--parse-only`) runs only the parser, the early errors and the scope
analysis of `js-syntax`, without the harness files. Without PATH it runs
all of `test/language/`, `test/built-ins/` and `test/annexB/`.

- A negative test of phase `parse` passes if the parse fails with a
  `SyntaxError`; every other test passes if it parses, also a negative
  test of phase `resolution` (a link error, which the parse does not
  see). A test runs in sloppy and strict mode as its flags say (strict:
  `"use strict";` before the source, except with `raw`); a test with the
  flag `module` runs once, with the Module goal (`parse_module`).
- Skipped: tests with a feature of `crates/js/test262/parse-skip.txt`
  (the proposals of test262's `features.txt` and the features
  standardized after ECMA-262 2025).
- "not supported yet" (the constructs of later M7 features) counts as
  unsupported, apart from failures.
- Scores: `crates/js/test262/parse-scores.json`, with the same ratchet:
  the run fails if the pass count of a group that it covers completely
  went down. `--update` writes the counts of all groups of the run (and
  refuses a partial run). A whole run takes about a second in a release
  build.

### jsdiff and jsbench

`just jsdiff FILE...` runs each file in `swb-js` and in Node.js (a classic
script; `print` joins the `String` of its arguments with spaces, like
the `print` of `swb-js`) and prints a unified diff of stdout and
of the first line of the uncaught-error report (a compile error is
`SyntaxError: ...` on both sides). The stack-trace lines of Node's own frames
below the script (`node:` and `[eval]`) are removed from Node's output, so
that printed `stack` texts compare; both engines get the file name as given.
Exit status 1 if any file differs. It
needs `node` and a built `swb-js`. A run that exceeds 60 s is reported
as `timeout: FILE (ENGINE)` and counts as a difference.

`just jsbench [DIR]` is described in [performance.md](performance.md).

### Number format table

`crates/js-text/tests/data/number_formats.tsv` holds 4,724 outputs of
`toFixed`, `toExponential` and `toPrecision` from Node.js 22, of
`toString(radix)` (radixes other than 10) from
`crates/js-text/tests/data/radix-reference.js`, and of `toLocaleString`
from Chromium 148; `crates/js-text/tests/number_formats.rs` compares
`js-text`'s formats with it. The radix reference is swb's definition
(ADR 0026 section 15: the fewest significant digits that read back, the
closest of them, on a tie the one whose digits form an even integer),
written with `BigInt`
as a search over the digit count, independent of the algorithm in
`js-text`; it differs from Node.js in about a third of the cases. To
write the table again:

```
just probe --json --case locale tools/probes/js-numbers.json > locale.json
node crates/js-text/tests/data/gen-number-table.js locale.json crates/js-text/tests/data/number_formats.tsv
```

`crates/js-text/tests/data/parse_int_radix.tsv` holds 1,209 cases of
`parseInt` in the radixes that are not 10 and not powers of two, with
the exact value from the same reference (`Number` of a `BigInt`):
random strings of 1 to 800 digits, leading zeros, signs, the ties at
2^53 and the boundary to infinity. `crates/js-text/tests/parse_int_radix.rs`
compares with it; `node crates/js-text/tests/data/gen-parse-int-table.js
crates/js-text/tests/data/parse_int_radix.tsv` writes it again.

`crates/js-text/tests/data/bigint_reference.tsv` holds 6,636 cases of
the `BigInt` arithmetic of `js-text` (M7 10a) with the results of
Python's `int`, an arbitrary-precision implementation outside swb used
as a black box: addition, subtraction, multiplication, comparison,
division and remainder (with 40 cases where Algorithm D needs its
add-back step, found by a simulation of the quotient estimate in the
script; a unit test in `bigint/div.rs` checks that each of them takes
the step), exponentiation, the shifts, the bitwise operators, `asIntN`
and `asUintN`, the conversions to and from doubles (ties to even, the
boundary to infinity), the exact comparison with doubles, `toString` in
all radixes, `StringToBigInt` (all white space, signs, prefixes,
separators, leading zeros) and the literals, and results at and just
above the size limit. Operands have 0 to 40 limbs, a few 100 to 300,
with runs of zero and all-one limbs and edge values (±2^63, ±2^64 ± 1,
powers of two). `crates/js-text/tests/bigint_reference.rs` compares with
it and checks the texts and operations at the limit, which are too long
for the table. To write it again (it does not need Node.js):

```
python3 crates/js-text/tests/data/gen-bigint-table.py crates/js-text/tests/data/bigint_reference.tsv
```

`tools/probes/js-bigint.json` measures Chromium's size limit (1,073,741,760
bits, by bisection), its messages, and the times of `toString`, `*` and
`/` on a value of a million bits; the snippet cases of the language are in
"BigInt tests".

The scripts take their random values from a fixed seed, so the same
Node.js and Chromium versions give the same tables. The other cases of
`js-numbers.json` show Chromium's `Math.sumPrecise`, `Math.f16round`,
`toLocaleString` and the property order of `Math` and `Number`.

### Unicode string tests

`tools/probes/js-unicode.json` has four cases for Chromium 148: `snippets`
(134 expressions with their results and error messages, in the format of
`js-strings.json`), `case-all` (`toLowerCase` and `toUpperCase` of every
code point; one entry for each code point where one differs), `normalize-all`
(the four forms of every code point) and the word lists of `localeCompare`
(`collation-sorted`: 225 words in Chromium's order; `collation-words`: 600
random pairs with the result). `crates/js/tests/data/gen-unicode-cases.js`
turns the probe output into the data of `crates/js/tests/builtins_unicode.rs`:

```
just probe --json tools/probes/js-unicode.json > probe.json
node crates/js/tests/data/gen-unicode-cases.js probe.json crates/js/tests/data
```

The case results (3,037 lines) are kept; the normalization results (567 KB)
are a count and an FNV-1a hash. A difference shows with the probe output:
run the `js` expression of the case in `swb-js` and compare the lines. The
order of `localeCompare` is not Chromium's (ICU): the test counts the pairs
of the sorted list that swb orders the other way (16 of 25,200).
`NormalizationTest.txt` runs in `crates/js-text/tests/normalization.rs`: a
committed sample always; the whole file with `cargo test -p swb-js-text
--test normalization -- --ignored` (it reads `out/ucd/17.0.0/`, which
`just tools js-unicode-tables` fills).

### Regular expression tests

`tools/probes/js-regexp.json` has the cases of
`crates/js-regexp/tests/data/regexp-cases.js` for Chromium 148 in three
lists: `exec` (660 matches: `new RegExp(pattern, flags + "d").exec(input)`
from a `lastIndex`, with the index, the capture indices and the group
names), `syntax` (503 patterns for `new RegExp`: "ok" or the message)
and `literals` (46 literal source texts parsed with `new Function`).
`crates/js-regexp/tests/data/gen-regexp-cases.js` makes the probe file
and, from its output, `regexp_cases.json` (strings with lone surrogates
are arrays of code units):

```
node crates/js-regexp/tests/data/gen-regexp-cases.js probe \
  crates/js-regexp/tests/data/regexp-cases.js tools/probes/js-regexp.json
just probe --json tools/probes/js-regexp.json > probe.json
node crates/js-regexp/tests/data/gen-regexp-cases.js cases probe.json \
  crates/js-regexp/tests/data/regexp_cases.json
```

`crates/js-regexp/tests/cases.rs` runs the parser and the matcher
directly on each case: each match also in slices of one step (the
resumed matcher) and, for Latin-1 inputs, on the one-byte width; each
pattern through both `check_literal` (no tree) and `Regex::new`. 299 of
the cases have `\p{...}` or `\P{...}` (186 matches, 110 patterns, 3
literals: General_Category, Script, Script_Extensions, binary properties
and properties of strings, with `u`, `v`, `i`, negation, `&&` and `--`;
`\p{Lu}` with `iv` against Chromium; 33 of the matches test the trie of
the strings of a property: the longest string first and shorter ones after
a failing tail, flags, keycaps, `iv`, lookbehind, classes with `\q{...}`
of more than 8 strings). `crates/js-regexp/tests/hostile.rs` has the limits:
nesting 100,000 deep, `a{0,4294967295}`, `(?:a?){1000000}`, a million
alternatives, inputs of 10,000,000 units, catastrophic patterns that run
in slices of 100,000 steps (each call does at most its steps and returns
within 0.5 s, the popping of choice points included), a backtrack stack
at the memory limit, slots and undo log inside the limit, and the cases
that were quadratic or large (classes of 100,000 characters with `v`,
`\q{...}` with 100,000 alternatives, 20,000 duplicate group names with
references, `\W` 50,000 times with `iu`, 100,000 classes with `i`, 10,000 times
`\p{L}`, `\P{L}`, `\p{Any}`, `[\p{L}--\p{Lu}]` with `u`, `v`, `iu` and `iv`,
60 different properties three times each, `\p{RGI_Emoji}` 20 and 100,000
times, `\p{RGI_Emoji}` with `v` and `iv` over a million units of `a` and
200,000 emoji (a trie, not one attempt per string), `\p{RGI_Emoji}+x` over
200,000 emoji in slices, `[\p{RGI_Emoji}--\q{...}]` and lookbehinds with
many strings, 20,000 different classes with `\p{L}`, a literal check of 100,000
property escapes) with generous time and `heap_bytes()` bounds. `cases.rs` has a list `KNOWN`
of cases where swb differs from Chromium on purpose (the case folding of
single characters in `\q{...}` with `iv`, the script value `Hrkt`, and
six cases of `\p{ASCII}` with `iv` and U+017F or U+212A). `bench.rs` has
two rows with `\p{RGI_Emoji}`.
`crates/js-syntax/tests/target_regexps.rs` (ignored) compiles and runs
every regular expression literal of the target scripts:
`SWB_TARGET_SCRIPTS=out/bbc-js:out/ars-js cargo test -p swb-js-syntax
--test target_regexps -- --ignored --nocapture`.

### RegExp built-in tests

`tools/probes/js-regexp-builtin.json` has 490 snippets for Chromium 148
in the format of `js-collections.json` (its `show` also prints a regular
expression as `re(source,flags,lastIndex)`, the named properties of an
array after its elements and an object without prototype with `N`).
The snippets are in `crates/js/tests/data/regexp-builtin-snippets.txt`,
one per line in groups (`## name`); a group whose name ends in `-heavy`
has large inputs. The groups: the legacy static properties (attributes,
names, values after each kind of match, the setters), the messages of
receivers and arguments, the shape of `RegExp`, `RegExp.prototype` and
`%RegExpStringIteratorPrototype%` (keys, attributes, names, lengths), the
constructor (`RegExp(re)` returning its argument, `IsRegExp` objects,
the order of the reads and conversions, subclasses, `new.target`),
`source` and `toString` with the escapes, `flags` and its reads, `exec`
(result keys, `d` indices, duplicate named groups, `lastIndex` in every
form, surrogate pairs, a `valueOf` that calls `compile`), `test`,
`match`, `search`, `replace` (templates, named references, replacer
functions, a script's `exec` with odd results and getters), `split`
(limits, captures, species constructors, sticky and empty matches),
`matchAll`, `RegExp.escape`, `compile`, literals, property escapes
(`\p{...}` with `u`, `v` and `i`, properties of strings, errors), and
the heavy group
(the backtrack stack bound, global loops over a million units).
`crates/js/tests/data/gen-regexp-builtin-cases.js` makes the probe file
from the snippets and, from its output, `regexp_builtin_cases.js` and
`.expected` (and `regexp_builtin_cases_heavy.*`) for
`crates/js/tests/builtins_regexp.rs`:

```
node crates/js/tests/data/gen-regexp-builtin-cases.js probe \
  crates/js/tests/data/regexp-builtin-snippets.txt tools/probes/js-regexp-builtin.json
just probe --json tools/probes/js-regexp-builtin.json > probe.json
node crates/js/tests/data/gen-regexp-builtin-cases.js cases probe.json crates/js/tests/data
```

Chromium runs each probe case in its own page; swb-js runs all snippets
in one realm, so a snippet must not depend on the matches of an earlier
group (the legacy static properties) or change shared built-ins. The
script leaves out the snippets with `Proxy` or `BigInt` and
two where swb follows the specification (V8 reads `sticky` before
`unicodeSets` in `flags`, and a RegExp String Iterator goes on after its
`exec` threw); 487 remain. `regexp_gc.js` has
regular expressions, results, collected results of a script's `exec` and
iterators with collections in callbacks, getters, `exec` and `valueOf`
(the expected output is that of Node.js 22). The hostile tests in
`regexp_hostile.rs` cover global `replace`, `split` and `match` on ten
million units, a million empty matches in `matchAll` and `replace`,
catastrophic backtracking at the deadline (also inside the symbol
methods) and at a termination request, backtrack stacks beyond V8's
bound and at a small heap limit (a `RangeError`), the collected matches
of a replacer function at the heap limit, collections between the
slices of a match in the stress mode, a `valueOf` of `lastIndex` that
calls `compile`, and 5,000 literals in one script.

### Array tests

`tools/probes/js-arrays.json` has 615 snippets for Chromium 148 in the
format of `js-strings.json` (`SNIPPET => ok VALUE` or `=> err NAME:
MESSAGE`; its `show` prints holes as `_`): the receiver and callback
messages of every method, the length limits, frozen and sealed arrays,
species, `Array.from` and `Array.of`, `sort`, mapped arguments, the key
order, and the limits group (one snippet per case: `flat` depth, the
`length` messages, array-likes of length 2^53 − 1).
`crates/js/tests/data/gen-array-cases.js` turns the probe output into
`arrays_cases.js` and `.expected` (and `arrays_cases_heavy.*`, the deep
nesting cases, which run without the stress mode) for
`crates/js/tests/builtins_array.rs`:

```
just probe --json tools/probes/js-arrays.json > probe.json
node crates/js/tests/data/gen-array-cases.js probe.json crates/js/tests/data
```

The script leaves out the snippets that need `Proxy`, `BigInt` or
`Array.fromAsync`, and three known differences (see the script).
Chromium loops over the length of a sparse array in the moving methods
(minutes for a length of 2^32 − 1), so these cases are in
`arrays_gc.js` and `language_hostile.rs` with results checked against
the specification's steps.

### Collection tests

`tools/probes/js-collections.json` has 1,499 snippets for Chromium 148 in
the format of `js-arrays.json`; its `show` prints a `Map` as
`Map{k=>v,...}` and a `Set` as `Set{v,...}`. The groups: the receiver
messages of every `Map` and `Set` method with 14 kinds of receivers
(including `Map.prototype`, `Set.prototype` and iterators of the other
kind), the shape (key order, `length`, `name`, attributes, the shared
function objects of `keys`, `values`, `entries` and `@@iterator`), the
constructors (iterables, adders, subclasses, `Reflect.construct`), the
keys (`-0`, `NaN`, integers and doubles, strings by content, objects,
symbols), iteration while the table changes (`for`-`of`, `forEach`,
`clear`, deletes that compact), `getOrInsert` and `getOrInsertComputed`,
`Map.groupBy`, `GetSetRecord` with 55 bad arguments for each of the seven
set methods, the set methods with logging arguments (the order of the
reads, the branch that iterates the smaller set, a `has` or `keys` that
changes the receiver) and small GC-heavy cases.
`crates/js/tests/data/gen-collection-cases.js` turns the probe output
into `collections_cases.js` and `.expected` (and `collections_cases_heavy.*`)
for `crates/js/tests/builtins_collections.rs`:

```
just probe --json tools/probes/js-collections.json > probe.json
node crates/js/tests/data/gen-collection-cases.js probe.json crates/js/tests/data
```

The script leaves out the snippets that need `Proxy`, `WeakMap` or
`BigInt`, and two that run until Chromium's limit of 2^24 entries
(`RangeError: Set maximum size exceeded`; the table unit test checks the
limit with a smaller constant). `collections_gc.js` holds the cases with
collections inside callbacks, adders, `has`, `keys` and `next`; its
expected output is that of Node.js 22 (`just jsdiff` compares it).
`collections_fuzz.js` runs random operations (inserts, deletes, `clear`,
iterators advanced by a few steps, `forEach` and `for`-`of` that change
the table, the set methods) and prints the length and a checksum of its
trace, which are those of Node.js 22 (the seed is fixed).
The hostile tests (`language_hostile.rs`) cover insert and delete churn
under a heap limit of 24 MiB, millions of abandoned iterators, 100,000
live iterators during a million table changes, tables that grow to the
heap limit, keys that a weak hash would put in few buckets, and loops that
add entries as they go. The table has unit tests in `object/table.rs`
(order, `SameValueZero`, compaction with live iterators, `clear`, the
pruning of the cursor list, memory after churn).

### Iterator helper tests

`tools/probes/js-iterators.json` has 499 snippets for Chromium 148 in the
format of `js-collections.json`: the shape of `Iterator`, `Iterator.prototype`
and the helper prototypes (key order, attributes, names, lengths), the
constructor and subclasses, the accessors `constructor` and `@@toStringTag`,
the receiver and argument messages of every method, the order of reads and
closes on bad arguments, `return` in every state, re-entrant `next` and
`return`, `flatMap` closing, `Iterator.from` and its wrapper,
`Iterator.concat`, the consuming methods and chains of 1,000 and 100,000
helpers. `crates/js/tests/data/gen-iterator-cases.js` turns the output into
`iterators_cases.js` and `.expected` (and `iterators_cases_heavy.*`) for
`crates/js/tests/builtins_iterators.rs`:

```
just probe --json tools/probes/js-iterators.json > probe.json
node crates/js/tests/data/gen-iterator-cases.js probe.json crates/js/tests/data
```

The probe file leaves out what swb does differently (see ADR 0026 section
15): `Symbol.dispose`, chains of 5,000 to 50,000 helpers, finite `take` and
`drop` limits above 2^53 - 1, BigInt. `iterators_gc.js` (callbacks, user
iterators and inner iterables that allocate and collect) and
`iterators_misc.js` (patched `next`, subclasses, deep chains) have the
output of Node.js 22 as expected output (`just jsdiff` compares; Node.js
22 lacks `Iterator.concat`, so `iterators_gc.js` polyfills it). The hostile
tests (`language_hostile.rs`) cover chains of 100,000 helpers, deeply
nested `flatMap`, wide `Iterator.concat`, endless iterators with every
consumer, re-entrant `next` and `return` and loops of throwing callbacks.

### Generator tests

`tools/probes/js-generators.json` has 251 snippets for Chromium 148 in the
format of `js-iterators.json`, in five cases: the shape of
`GeneratorFunction`, `%GeneratorFunction.prototype%`,
`%GeneratorPrototype%` and of generator functions and methods (key order,
attributes, `prototype` and its replacement, `new`), the `GeneratorFunction`
constructor (source text, early errors, `new.target`, subclasses), the
`next`, `return` and `throw` protocol of generators, `yield*` (the order of
the `Get`s and calls on a logging delegate, the result object that passes
through, the messages for each form of the operand, `throw` and `return`
with and without the delegate's methods, re-entrant calls) and deep chains.
`crates/js/tests/data/gen-generator-cases.js` turns the output into
`generators_cases.js` and `.expected` (and `generators_cases_heavy.*`, the
chains and recursions of thousands of generators, which run without the
stress mode) for `crates/js/tests/generators.rs`:

```
just probe --json tools/probes/js-generators.json > probe.json
node crates/js/tests/data/gen-generator-cases.js probe.json crates/js/tests/data
```

The script leaves out V8's third message form for some call operands
("yield* (intermediate value)(intermediate value) is not iterable") and the
recursion of 5,000 generators, which Chromium 148 ends with a `RangeError`
and swb runs (see ADR 0026 section 15). `generators_gc.js` (delegates whose
methods and result getters allocate and collect, delegates and received
values that only a suspended generator holds, chains with collections at
every level) has the output of Node.js 22 as expected output (`just
jsdiff` compares). The hostile tests (`language_hostile.rs`) cover chains
and recursions of 100,000 generators with `next`, `return`, `throw`,
spread and `for`-`of`, delegates that throw or return non-objects in hot
loops, re-entrant `next`, `throw` and `return` through a delegate, and
endless delegates under the time and heap limits.

### Promise tests

`tools/probes/js-promises.json` has 257 snippets for Chromium 148 in four
cases: the shape of `Promise`, `Promise.prototype` and of the functions
that the engine makes (resolving functions, the executor of
`NewPromiseCapability`, the element functions of the combinators, the
closures of `finally`), the messages (the constructor, the receiver
checks, `NewPromiseCapability`, `GetPromiseResolve`, `SpeciesConstructor`,
the self-resolution, `AggregateError`), the order of the jobs (chains,
`then` on settled promises, resolution with promises and thenables,
getters of `then`, `finally`, the combinators, subclasses, the `Get`s of
`constructor`, `Promise.try`) and chains of thousands of promises. The
probe's pages have JavaScript disabled, so no timer runs; only the jobs of
promises do. The runner of the file therefore uses promises alone: it
evaluates each snippet with an indirect eval, attaches handlers to a
promise result, lets a chain of `ticks` jobs run (50 by default; a
snippet sets more) and records the result and the lines that the snippet
and its jobs pushed to `log`, in order. `just probe` awaits the runner's
promise. The same runner works unchanged in swb-js, where the harness of
the tests runs the jobs after the script.
`crates/js/tests/data/gen-promise-cases.js` turns the output into
`promises_cases.js` and `.expected` (and `promises_cases_heavy.*`, the
long chains, which run without the stress mode) for
`crates/js/tests/promises.rs`:

```
just probe --json tools/probes/js-promises.json > probe.json
node crates/js/tests/data/gen-promise-cases.js probe.json crates/js/tests/data
```

The script leaves out one snippet where swb names an object differently
in a message (see the script). `promises_gc.js` (executors, handlers,
thenables, `finally` callbacks, subclasses and the combinators that
allocate and collect while only the queue or the reactions of pending
promises hold their values) has the output of Node.js 22 as expected
output (`just jsdiff` compares; Node.js 22 lacks `Promise.try`, so the
file has a fallback for it). `promises.rs` also tests the embedding API:
the order of the drain, the rejection tracker, host reactions, an error
and a termination inside a job. The hostile tests (`promises_hostile.rs`)
cover a chain of a million `then` calls, promises resolved with promises
100,000 levels deep, thenables that resolve with thenables, `Promise.all`,
`allSettled`, `any` and `race` over a million elements (and under a heap
limit of 64 MiB), endless job loops at the deadline, queues, reaction
lists and rejection lists that grow to the heap limit, recursive
executors and a termination that skips the `catch` handlers.

### Async function tests

`tools/probes/js-async.json` has 279 snippets for Chromium 148 in five
cases, with the runner of `js-promises.json` (it waits with a chain of
`then` jobs): the shape of async functions, arrows and methods
(prototypes, no `prototype`, `length`, `name`, the source text, not
constructors), `AsyncFunction` and `%AsyncFunction.prototype%` (key
order, descriptors, the source that the constructor builds, `new.target`,
subclasses), the messages (the early errors of `await` and of the
constructor's source, throws before the first `await`, in the
parameters, rejections caught in `try`, `catch` and `finally`, thenables
that throw, a `constructor` getter that throws), the order of the jobs
(an `await` of a native promise, a thenable, a subclass and a
non-promise; `return` and `return await`; two functions that interleave;
functions that await each other; re-entrant calls), `arguments`, `this`
in async arrows, `super` in async methods, `await` in loops, `for`-`of`
(with the closing of the iterator), destructuring defaults, template
literals and the other operators, the async frames of `stack` (`at async
f`, their positions and `Error.stackTraceLimit`), and recursion and chains
of 100,000 functions. The case files are made as for the promises:

```
just probe --json tools/probes/js-async.json > probe.json
node crates/js/tests/data/gen-async-cases.js probe.json crates/js/tests/data
```

`crates/js/tests/async_functions.rs` runs `async_cases.js` (267 snippets,
normal and stress mode) and `async_cases_heavy.js` (11, normal mode).
The script leaves out one snippet, which shows V8's `at async Promise.all
(index 0)` line (swb has no lines for built-in functions). `async_gc.js` (suspended frames that hold
registers, `arguments`, cells, `finally` state and iterators while the
functions allocate and collect) has the output of Node.js 22 as expected
output (`just jsdiff`). The file also tests the full text of `stack` with
async frames, that a suspended function waiting for a promise that no
one can settle is collected, a call of an async function from the host,
and terminations: an endless `await` loop ends at the deadline, the
result promise stays pending, no handler runs, and a function that was
suspended resumes in a later task. The hostile tests in
`promises_hostile.rs` cover async recursion 100,000 deep (a `RangeError`
rejection, as in Chromium), chains of 100,000 functions that await each
other, endless `await` loops at the deadline and sets of suspended
functions that grow to the heap limit.

### Async iteration tests

`tools/probes/js-async-iteration.json` has 341 snippets for Chromium 148
in seven cases, with the runner of `js-async.json`: the shape of async
generator functions and methods (prototype chains, `prototype`, key order
and descriptors of `%AsyncGeneratorFunction.prototype%`,
`%AsyncGeneratorPrototype%`, `%AsyncIteratorPrototype%`,
`AsyncGeneratorFunction` and its source text, `Array.fromAsync`), the
queue of requests (several `next` calls before the first settles;
`return` and `throw` on generators that are suspended at the start, at a
`yield`, executing, awaiting and completed; `return` of a promise, a
rejected promise and a thenable; a `constructor` getter; the job order of
each), `for await` (over async generators, sync iterables of promises
with a rejecting one and the close of the sync iterator, objects with
`Symbol.asyncIterator`; `break`, `return`, `throw` and labels with the
awaited close; `return` methods that throw, reject or give a non-object;
results that are not objects; destructuring and lexical bindings),
`yield*` in async generators (async and sync delegates, `throw` and
`return` forwarded or missing, the messages by the form of the operand),
`Array.fromAsync` (iterables, array-likes, async iterables, the mapping
function and `thisArg`, `this` constructors, errors and the close of the
iterator), the async frames of `stack`, and long loops, queues and
`yield*` chains. The case files are made as for the async functions:

```
just probe --json tools/probes/js-async-iteration.json > probe.json
node crates/js/tests/data/gen-async-iteration-cases.js probe.json crates/js/tests/data
```

`crates/js/tests/async_iteration.rs` runs `async_iteration_cases.js` (320
snippets, normal and stress mode) and `async_iteration_cases_heavy.js` (9,
normal mode). The script leaves out 12 snippets where swb follows the
specification or has its own limits, with the reasons in
`gen-async-iteration-cases.js`: V8 resumes a `yield*` delegate with
`next(reason)` when a return value rejects, closes iterators in more error
paths of `Array.fromAsync`, shows the frames of built-in `next` methods,
renders every operand of `for await` in its message, and ends a chain of
5,000 delegating generators. The probe file leaves out the cases where
Chromium's `Array.fromAsync` never settles (a `return` that throws or
rejects during the close; V8 calls it again in every job) and runs no
endless loops. `async_iteration_gc.js` (frames, queues, loop records,
Async-from-Sync Iterators and `Array.fromAsync` states that are held only
by generator objects and reactions while the code allocates and
collects) has the output of Node.js 22 as expected output (`just
jsdiff`). The file also tests that unreachable async generators
(suspended at the start, at a `yield` with waiting requests, at an
`await` that no one can settle) are collected, `next` called from the
host, terminations (an endless `for await` loop ends at the deadline, no
handler or `finally` block runs, a generator suspended during the
termination resumes later, one whose frame was removed is completed),
and the single close of `Array.fromAsync` when `return` throws. The
hostile tests in `async_iteration_hostile.rs` cover a chain of 100,000
`yield*` levels (a `RangeError` rejection at the frame limit) and a chain
of 4,000, `for await` over a million values (an async generator, an
array, an async iterator), a queue of a million requests, `Array.fromAsync`
over a million values, endless async iteration at the deadline and
queues and frames that grow to the heap limit.

### Proxy tests

`tools/probes/js-proxy.json` has 672 snippets for Chromium 148 in 31
cases, made from `crates/js/tests/data/proxy-snippets.txt` by
`crates/js/tests/data/gen-proxy-cases.js` (the format of
`js-regexp-builtin.json`, with a prelude). The prelude defines
`tracer(log, name, target)`: a proxy whose handler is itself a proxy that
logs each trap it is asked for, with its key, and forwards it with
`Reflect`, so a snippet shows which internal methods an operation calls
and in which order. The groups: the shape of `Proxy` and
`Proxy.revocable`, the errors of `new Proxy`, every operation on a revoked
proxy, traps that are not callable, each of the 13 traps with every
invariant violation and V8's message, the trap order of `Object`,
`Reflect`, `JSON` (`stringify`, the `parse` reviver), `for`-`in`, `with`,
destructuring, spread, `instanceof`, `Function.prototype.bind`, the
`Array` methods with a proxy `this` and with a proxy on the prototype
chain of arrays, `IsArray`, `ArraySpeciesCreate`, `IsConcatSpreadable`,
`Object.prototype.toString` and `Function.prototype.toString` of proxies,
the uses of the target scripts (the reactive store and the `with` scope of
ars-js/s06.js, the lazy forwarding object and the `apply` trap of
bbc-js/s2.js, the module namespace of bbc-js/s56.js and s57.js), traps
that allocate, chains of 100 to 1,000,000 proxies and `ownKeys` results of
100,000 keys. The case files:

```
node crates/js/tests/data/gen-proxy-cases.js probe \
  crates/js/tests/data/proxy-snippets.txt tools/probes/js-proxy.json
just probe --json tools/probes/js-proxy.json > probe.json
node crates/js/tests/data/gen-proxy-cases.js cases probe.json crates/js/tests/data
```

`crates/js/tests/builtins_proxy.rs` runs `proxy_cases.js` (653 snippets,
normal and stress mode) and `proxy_cases_heavy.js` (9, normal mode, on an
8 MiB thread). The script leaves out the snippets with `Date` or `WeakMap`
and the five of `KNOWN` in `gen-proxy-cases.js`: in a `with` body V8 skips
the second `HasProperty` of GetBindingValue and SetMutableBinding and
resolves an assignment's target after its right side (swb follows the
specification), and V8 words a `construct` trap that is not callable "is
not a function". `proxy_gc.js` (traps that allocate and collect while the
engine holds targets, handlers, keys, descriptors and results) and
`proxy_console.js` (`console.log` shows the target and runs no trap; a
revoked proxy is `<Revoked Proxy>`) have the output of Node.js 22 as
expected output. The hostile tests in `proxy_hostile.rs` (on an 8 MiB
thread, in the debug build of `cargo test`) cover a chain of a million
proxies with every internal method (V8's `RangeError`; `IsArray` and
`[[Call]]` are loops and work), traps that call their own proxy or revoke
it, `getPrototypeOf` traps that make `instanceof`, `for`-`in` and
`isPrototypeOf` endless (the deadline), `ownKeys` results of a huge
`length` (the deadline or the heap limit), a million keys against a
non-extensible target with a million properties and the duplicate check
over a million keys (linear), and trap results that grow to the heap
limit. The test262 subset has `feature Proxy`, `feature
proxy-missing-checks` and the group `test/built-ins/Proxy`; the tests that
include `proxyTrapsHelper.js` run (the tests with `$262.createRealm` stay
skipped).

### Weak collection tests

`tools/probes/js-weak.json` has 318 snippets for Chromium 148 in 11 cases,
made from `crates/js/tests/data/weak-snippets.txt` by
`crates/js/tests/data/gen-weak-cases.js` (the format of `js-proxy.json`,
without a prelude): the shape of `WeakMap`, `WeakSet`, `WeakRef` and
`FinalizationRegistry` (key order, `length`, `name`, attributes, tags), the
constructors (iterables, adders, closing of the iterator, subclasses,
`Reflect.construct` and the order of the `prototype` read), every method
with valid and invalid keys and V8's messages, the receivers, the upsert
methods (the callable check before the key check, callbacks that change
the map), symbols as keys (registered symbols fail, well-known symbols
work), proxies as keys and targets, the uses of the target scripts (the
module cache of bbc-js/s14.js, the core-js checks of bbc-js/s12.js, the
circular-structure checks with `WeakSet` of bbc-js/s18.js and s47.js, the
reactive store of ars-js/s06.js and others) and large tables, registries
and chains. Nothing in them depends on when a collection runs. The case
files:

```
node crates/js/tests/data/gen-weak-cases.js probe \
  crates/js/tests/data/weak-snippets.txt tools/probes/js-weak.json
just probe --json tools/probes/js-weak.json > probe.json
node crates/js/tests/data/gen-weak-cases.js cases probe.json crates/js/tests/data
```

`crates/js/tests/builtins_weak.rs` runs `weak_cases.js` (311 snippets,
normal and stress mode), `weak_cases_heavy.js` (7, normal mode) and
`weak_console.js` (`console.log` of the weak objects; the expected output
is that of Node.js 22). No snippet differs from the specification, so the
`KNOWN` list of the script is empty. The collector cases are swb-only and
force collections from Rust (`crates/js/tests/weak.rs`, normal and stress
mode; the stress mode uses 5,000 instead of 100,000 entries for the
large cases): a value that references its own key, the memory of dead
entries, chains of 100,000 ephemerons in one table and across 1,000
tables inserted forwards and backwards (a bound on the pause), keys that
only another table's value reaches, `WeakRef` targets before and after
`ClearKeptObjects` (and at the end of `run_jobs`), symbol keys, revoked
proxies, cleanup callbacks (once per target, after the collection, in
order, never for unregistered cells, dead registries or dropped queued
registries, a callback that throws, callbacks that collect and register).
The hostile tests in `weak_hostile.rs` (on an 8 MiB thread) cover a
million entries whose keys die under a heap limit of 32 MiB, a chain of a
million ephemerons inserted backwards, a chain, a table, a kept set and a
registry that grow to the heap limit, a callback that registers a new
target each time (2,000 rounds, bounded memory) and the shell's loop of
cleanup tasks in the stress mode (the deadline ends it), 10,000 callbacks
that throw (one error each) and `getOrInsertComputed` callbacks that
change the map or re-enter without end (V8's `RangeError`). The unit tests
of `object/weak.rs` cover the cells of a registry (reuse, stale
references, tokens). The test262 subset has the features `WeakMap`,
`WeakSet`, `WeakRef`, `FinalizationRegistry` and `symbols-as-weakmap-keys`
and the four groups `test/built-ins/WeakMap`, `WeakSet`, `WeakRef` and
`FinalizationRegistry`; no test there needs a forced collection, and the
four cross-realm tests stay skipped.

### BigInt tests

`tools/probes/js-bigint.json` has 868 snippets for Chromium 148 in 23
cases, made from `crates/js/tests/data/bigint-snippets.txt` by
`crates/js/tests/data/gen-bigint-cases.js` (the format of `js-weak.json`;
a BigInt result shows as `1n`), and five hand-written cases of 10a and
10b whose names start with `limit` (the size limit, the messages, the
times, a literal above the limit, the quoting of long texts in
messages), which the generator keeps: literals in all radixes and with
separators, literals as keys of object literals, classes and patterns,
every operator with every type combination (Numbers, strings, booleans,
`undefined`, `null`, symbols, objects with `valueOf`, wrapper objects),
compound assignment, `++` and `--` in all forms, the relational operators
and `==` with Numbers, strings and booleans, `===`, `Object.is`,
`switch`, `includes`, `indexOf`, `Map` and `Set`, property definitions on
frozen objects, the conversions (`String`, template literals, `Number`,
`parseInt`, `JSON.stringify` with and without `toJSON`, `sort`), the
`TypeError` of the Number functions, the `BigInt` function, `asIntN` and
`asUintN` (with `ToIndex`), the prototype methods and their receivers,
property access on primitives, and V8's messages. The case files:

```
node crates/js/tests/data/gen-bigint-cases.js probe \
  crates/js/tests/data/bigint-snippets.txt tools/probes/js-bigint.json
just probe --json tools/probes/js-bigint.json > probe.json
node crates/js/tests/data/gen-bigint-cases.js cases probe.json crates/js/tests/data
```

`crates/js/tests/builtins_bigint.rs` runs `bigint_cases.js` (850
snippets, normal and stress mode), `bigint_console.js` (`console.log` of
`BigInt` values and wrapper objects; the expected output is that of
Node.js 22), both string widths of `StringToBigInt` and of the
comparisons with strings (also ropes), and uses in loops. The group
`sizes` (values beyond swb's limit of 2^18 bits) and the `KNOWN`
snippets of the generator (ECMA-402 locales, `new Date`, the callee text
of `o[Symbol.iterator]()`) are left out. `crates/js/tests/bigint_hostile.rs`
(on an 8 MiB thread) covers endless loops of the slowest operations on
values at the limit against a deadline of 200 ms (multiplication,
division, `toString` in radixes 10 and 36, powers, `BigInt` of a long
text, shifts, bitwise operators, `asUintN`, comparisons with long
strings, `toLocaleString`), values that grow until a heap limit of
64 MiB, a million temporaries under 32 MiB, `BigInt` of a valid and an
invalid text of 50 million units, 240,000 operations that throw,
endless loops of errors whose messages quote a value at the limit (`big
in big`, `Object.create(big)` and others), literals at and above the
limit (a `SyntaxError` at compile time, also in a function that never
runs, as a key and in a pattern) and literal constants in the stress
mode. The test262 subset has the feature `BigInt` and the groups
`test/built-ins/BigInt` and `test/language/literals/bigint`.

### ArrayBuffer and DataView tests

`tools/probes/js-binary.json` has 733 snippets for Chromium 148 in 21
cases, made from `crates/js/tests/data/binary-snippets.txt` by
`crates/js/tests/data/gen-binary-cases.js` (the format of `js-bigint.json`;
the global helpers `hex`, `filled` and `nt` of the generator read the
bytes with a `DataView`, make a buffer from bytes and make a constructor
whose `prototype` getter runs a function), and four hand-written cases
whose names start with `facts`, which the generator keeps: the
properties of Chromium 148 (`SharedArrayBuffer` and the immutable buffers
are not defined), the allocation limits and their messages, and V8's
largest length and `maxByteLength` by bisection. The snippets cover the
constructors and their conversions (also with `valueOf` side effects and
the order of the checks around `OrdinaryCreateFromConstructor`),
subclasses, the properties and their order, the getters, `isView`,
`slice` with every case of `SpeciesConstructor`, `resize`, `transfer`
and `transferToFixedLength`, views that track a resizable buffer and go
out of bounds, every getter and setter in both byte orders with edge
values (NaN, ±0, ±Infinity, subnormals, values that wrap, `BigInt`s out
of range, binary16 ties), detaching and shrinking in the conversions, the
NaN bits, and V8's messages. The case files:

```
node crates/js/tests/data/gen-binary-cases.js probe \
  crates/js/tests/data/binary-snippets.txt tools/probes/js-binary.json
just probe --json tools/probes/js-binary.json > probe.json
node crates/js/tests/data/gen-binary-cases.js cases probe.json crates/js/tests/data
```

`crates/js/tests/builtins_binary.rs` runs `binary_cases.js` (725
snippets, normal and stress mode; the `KNOWN` snippets of the generator
are left out: the order of `ToIndex` in `new ArrayBuffer` and in
`resize`, the NaN bits of `0 / 0` and of `undefined`), `binary_console.js` (`console.log` of buffers and
views; the expected output is that of Node.js 22) and the forms Node.js
cannot print, the `DataView` paths of the target scripts, and views
across collections. `crates/js/tests/binary_hostile.rs` (on an 8 MiB
thread) covers buffers at and above a heap limit of 64 MiB (a
`RangeError`, also for `resize`, `transfer` and `slice`), buffers kept
until the limit (a `RangeError`, not a termination), a loop that drops
its buffers after the `RangeError` and runs to the deadline, garbage,
detached and sliced buffers that are given back (normal and stress
mode), endless loops of 64 MiB allocations,
slices, transfers, resizes and views against a deadline of 200 ms,
`binary_detach.js` (detaching, shrinking, growing and collecting in
every conversion callback of the 22 `DataView` methods, of the
constructor and of the `ArrayBuffer` methods; expected output from
Node.js 22), and the heap accounting of the bytes (a new buffer, a
transfer, a detached buffer, a collection). The test262 runner has
`$262.detachArrayBuffer` and the harness files `detachArrayBuffer.js` and
`byteConversionValues.js`; the subset has the features `ArrayBuffer`,
`DataView`, the `DataView.prototype.*` names, `resizable-arraybuffer`,
`arraybuffer-transfer`, `Float16Array` and
`align-detached-buffer-semantics-with-web-reality`, and the groups
`test/built-ins/ArrayBuffer` and `test/built-ins/DataView`.

### Typed array tests

`tools/probes/js-typed.json` has 9,245 snippets for Chromium 148, made
from `crates/js/tests/data/typed-snippets.txt` (901 snippets, feature 11b)
and `typed-method-snippets.txt` (8,344 snippets, feature 11c; described
below) by `crates/js/tests/data/gen-typed-cases.js` (the format of
`js-binary.json`;
the global helpers `list`, `rab`, `desc`, `keys`, `det`, `nt` and `sp` of
the generator read the elements by index, make a resizable buffer, show a
property descriptor and the own keys, detach a buffer with `transfer`,
and make a `new.target` with a `prototype` getter and a `constructor`
with a `@@species` getter; the helpers of 11c are described in the
generator), two hand-written cases whose names start with
`facts` (the properties of Chromium 148 and V8's length limits; an
array-like source of length 2^35 is not measured, V8 reads all its
elements), and the cases `detach` and `detach-methods`, which the generator
makes from `crates/js/tests/data/typed_detach.js` and
`typed_methods_detach.js`. The snippets of 11b cover the
properties and their order, the abstract `%TypedArray%`, subclasses,
every form of the constructors with the conversions of the length, the
offset and the elements (all element types with the values of
`byteConversionValues.js`), the order of the observable steps, length
tracking and out-of-bounds arrays on resizable buffers, detaching in the
conversions, the exotic internal methods with odd keys ("-0", "1.5",
"1e+21", "NaN", "4294967295", non-canonical strings) through `in`,
`delete`, `Reflect`, `Object.defineProperty`, `Object.freeze`, proxies,
receivers and prototype chains, the generic `Array.prototype` methods on
typed arrays, the accessors, `set` (also overlapping within one buffer),
`subarray`, `slice` and `from` and `of` with every case of the species
constructor, the iterators, and V8's messages. The case files:

```
node crates/js/tests/data/gen-typed-cases.js probe \
  crates/js/tests/data/typed-snippets.txt \
  crates/js/tests/data/typed-method-snippets.txt tools/probes/js-typed.json
just probe --json tools/probes/js-typed.json > probe.json
node crates/js/tests/data/gen-typed-cases.js cases probe.json crates/js/tests/data
```

`crates/js/tests/builtins_typed.rs` runs `typed_cases.js` (897 snippets,
normal and stress mode; left out: the `KNOWN` snippets of the generator,
`Object.isSealed` of a non-extensible array with elements, the iterator
after a `TypeError` and the arguments of `subarray`'s species
constructor), `typed_methods_cases.js` (feature 11c, 8,263 snippets,
normal and stress mode; see below), `typed_console.js` (`console.log` of typed arrays; the
expected output is that of Node.js 22), the forms that Node.js 22 cannot
show (`Float16Array`, the cut after 100 elements), and the typed array
paths of the target scripts (expected output from Node.js 22).
`crates/js/tests/typed_hostile.rs` (on an 8 MiB thread) covers arrays at
and above a heap limit of 64 MiB (a `RangeError` before any work in
proportion to the length, also for array-like sources, `from`, a copy of
another type, `slice` and an overlapping `set`), V8's length messages and
offsets and lengths up to 2^53 − 1, arrays kept until the limit (a
`RangeError`, normal and stress mode), a loop that drops its arrays after
the `RangeError`, endless and huge sources (array-likes, iterators,
proxies, `from` with a mapping function, `set`, iteration, `for`-`in`,
spread, `Object.keys`, `JSON.stringify`) and endless loops of 64 MiB
copies, conversions, allocations and element writes against a deadline
of 200 ms, a `slice` whose `start` shrinks the array to no element,
`typed_detach.js` (detaching, shrinking, growing and collecting
in every callback of the typed arrays for four element types; the
expected output is Chromium 148's, from the probe case `detach`, except 8
KNOWN lines where swb reads the source of `new T(typedArray)` after the
`prototype` getter of `new.target`, as the specification says), the
overlapping copies of `set` and `slice` within one buffer against a model
written in the script, and the heap accounting (the buffer counts, a
`subarray` adds no bytes, a dropped array gives its buffer back). The
test262 runner loads `testTypedArray.js`, `resizableArrayBufferUtils.js`
and `testAtomics.js`; the subset has the features `TypedArray` and the 11
constructor names (with `Float16Array` of 11a), `test/built-ins/TypedArray`
(with a `dir` line for each method of the prototype),
`test/built-ins/TypedArrayConstructors`, `test/built-ins/Atomics` and
`test/built-ins/Uint8Array`.

The snippets of feature 11c (`typed-method-snippets.txt`, made by a
script that is not in the repository; the text file is the artifact)
cover, in groups whose names start with `m-`: the properties and the
key order of `%TypedArray%.prototype`; the receivers of every method (an
object, `undefined`, an array, a `DataView`, a detached or out-of-bounds
array, a `Proxy`) and the order of the receiver check and the argument
conversions; callbacks that are not functions (the values and V8's three
wordings); every method with the 12 element types (four types get all
cases, the others a third of them plus the conversions) for the values
that differ: −0, NaN, the ends of the integer ranges, `BigInt` elements
and operands, `Float16Array`; the species cases of `map` and `filter` and
`TypedArrayCreateSameType` of `toReversed`, `toSorted` and `with`; the
arguments that detach, shrink, grow or empty a resizable buffer in every
method (`ach` in the generator); the sequence of the calls of a
comparator for sizes 2 to 100 and eight comparators (`sc`); the
`Array.prototype` generics on typed arrays; `Atomics` with each operation
and type, the conversion order, the revalidation, `wait`, `waitAsync`,
`notify`, `isLockFree` and `pause`; and the base64 and hex methods with
the options, the ends of the input, whitespace, padding, the three
`lastChunkHandling` values, and targets of 0 to 8 bytes. The groups
`m-known-*` are probed but not run (swb differs on purpose: the generator
says why for each). `typed_methods_detach.js` runs the callback methods,
the comparators, the argument conversions, the species constructors, the
elements' `toLocaleString`, `Atomics` operands and the option getters of
the base64 methods with the four actions (detach, shrink, grow, collect) for
four element types (648 lines; 8 KNOWN lines).
`crates/js/tests/typed_methods_hostile.rs` (on an 8 MiB thread) runs 40
loops over arrays of 64 MiB (`fill`, `reverse`, `copyWithin`, the
searches, the sorts, `join`, `toBase64`, `fromBase64`, the callback
methods, `Atomics`) against a deadline of 200 ms, single calls of the
sort and `join` over 8 M elements, arrays and strings at a heap limit of
64 MiB (`RangeError` for the new arrays, the sort scratch memory, the list
of `filter` and the output of the decoders and encoders; the end of the
script for the text of `join`; "Invalid string length" above the string
limit), a test at 32 MiB that the memory follows the data (a default sort
of 8 MiB of floats with NaNs, a counting sort of 6 MiB, and 20 MiB of
whitespace or bad characters for the base64 and hex decoders: no
`RangeError`), malformed and huge base64 and hex input (16 M characters of
whitespace, 16 M valid characters then a bad one, an error after a full
target), and collections inside the callbacks and comparators with
`BigInt` elements in the GC stress mode.

### Date tests

`tools/probes/js-date.json` has the snippet cases of `Date` for Chromium
148, made from `crates/js/tests/data/date-snippets.txt` (about 5,500
snippets, in groups) by `crates/js/tests/data/gen-date-cases.js` (the
format of `js-bigint.json`, with a `time_zone` for each case). A group runs
in seven zones: UTC, Europe/Berlin, America/New_York, Asia/Kolkata (a
half-hour offset, no summer time), Australia/Adelaide (a half-hour offset,
southern summer time), Australia/Lord_Howe (a 30-minute change) and
Pacific/Apia (the day 2011-12-30 does not exist); a line `## NAME @ Zone,
Zone` restricts a group, also to the fifteen `EXTRA_ZONES` of the generator
(Europe/London, Europe/Dublin, America/Los_Angeles,
America/Argentina/Buenos_Aires, Asia/Kathmandu, Africa/Casablanca,
Africa/Windhoek, Antarctica/Troll, Asia/Almaty; and, for the equivalent
year of the names, Africa/Cairo, America/Sao_Paulo, Asia/Tehran,
Asia/Amman, America/Santiago, Pacific/Fiji) that only the groups
`names-more`, `names-years`, `names-edge` and `names-edge-1970` use. A
line that starts with `!known ` marks a snippet where swb differs from
Chromium on purpose (ADR 0027 section 8: for `Date.parse`, 501 texts of
the groups `parse-fallback*` and `parse-forms` in six classes, and 27 of
`parse-free`), `!later ` one that needs a feature that is not there yet
(both are probed, not run; no snippet has `!later` since 12b). No
snippet depends on the current time. The groups: `shape` (the properties, their
order and attributes, `Date()` as a function, subclasses and `new.target`),
`methods` and `receivers` (the length, the name and V8's message of every
method for a wrong receiver), `toprimitive`, `ctor` and `utc` (every form
of the arguments, the order of the conversions, huge and non-finite
values, two-digit years, the ends of the range), `getters` and `strings`
(the getters and the string forms of 105 instants from year -271821 to
275760, also the round trip through `Date.parse`), `names` and `names-more`
(the long zone names and the GMT format, in 16 zones, at times that
include the years before 1970 and after 2038), `transitions` (the local times around the changes of
each zone, in both directions, and the setters across them), `setters`
(every setter with every kind of argument on eleven dates, the order of
the conversions, a `valueOf` that changes the date), `parse` (the date
time string format with the forms around each rule, and the output of
`toString` and `toUTCString`), `parse-fallback`, `parse-fallback-more`,
`parse-fallback-long` and `parse-forms` (Chromium's other forms in New
York and Berlin: a table of dates, clocks and zones, a deterministic
sample of random token mixes, long runs, units that are white space or
not, what the constructor and the string forms of `Date` read back; in
`parse-forms` the probes of the separators, the order of the fields, the
clocks and the zones that set the rules of ADR 0027 section 5a, and every
97th text of a generated list of 129,000 dates x clocks x zones),
`parse-free` (the free text before the date and the expanded year at the
start, from the review of 12b),
`names-years` (the zone names on 122 days of 28 years before 1970 and
after 2037, in seven zones with year-dependent rules: the table of
equivalent years), `names-edge` and `names-edge-1970` (where the
equivalent year starts) and `realworld`.

```
node crates/js/tests/data/gen-date-cases.js probe \
  crates/js/tests/data/date-snippets.txt tools/probes/js-date.json
just probe --json tools/probes/js-date.json > probe.json
node crates/js/tests/data/gen-date-cases.js cases probe.json \
  crates/js/tests/data/date-snippets.txt crates/js/tests/data
```

`crates/js/tests/builtins_date.rs` runs `date_cases.js` (the snippets once
and the index list of each zone) in each zone, normally and in the GC
stress mode, against `date_cases_ZONE.expected` (about 18,600 results in
22 zones). It needs
the time zone database of the system (`/usr/share/zoneinfo`) with the 16
zones; the old offsets of the cases come from tzdata and agree with the
ICU data of Chromium 148 in these zones (Europe/Zagreb does not agree
before 1884 in Debian's tzdata, so Berlin is the northern zone). It also
tests the host clock, a POSIX rule as the zone and `console.log` of dates.
The zone names depend on the database in two ways: the saving flag for
the zones of `names-more` (the database must have Dublin's winter time
as `GMT` with a negative saving, or in rearguard form: `zone.rs` handles
both), and the offsets of years before 1900.
`date_hostile.rs` runs: strings of up to 100 M units to `Date.parse` and
the constructor (also as ropes and as the end of a `toString` text), loops
of long parses and of every string form and setter against a deadline of
300 ms, 900 pairs of huge and non-finite values in every position of the
constructor, `Date.UTC` and the setters, the ends of the time range in
six zones, conversions that change the date or collect (also in the GC
stress mode), and a loop of 200,000 dates that the collector frees; for
the written forms: runs of 20 M units of digits, words, white space,
parentheses, NUL units and lone surrogates, and loops of long texts that
end at the deadline (the scan is charged). The
unit tests of `crates/js/src/date/` check the calendar over the range,
`MakeDay` and `MakeTime` at their edges, the zone with the skipped and
repeated times and the eras beyond the range of `jiff`, the formats, the
exact parsers, each form of the ISO variants and the written forms
(`iso_variants.rs`, `written.rs`), the table of equivalent years and the
zone names (`zone_names.rs`, `zone.rs`). The names were also compared with Chromium for
all 484 zones of the system database at 18 instants each (not in the
repository: a probe file made by a script of the scratchpad; the 8,000
results with equal offsets are equal, see ADR 0027 section 6). The data of
the names is made by `just tools js-time-zone-names` (next table). test262 runs `test/built-ins/Date` and
`test/annexB/built-ins/Date` and loads `assertRelativeDateMs.js` and
`dateConstants.js`; the 155 tests of other directories that use `Date`
run too.

## Python tools: development

- Package: `tools/swbtools/`. Tests: `tools/tests/`. Run `just tools-check`
  before a commit that changes `tools/`.
- The tests do not use the network. Tests that start Chromium
  (`tools/tests/test_chromium.py`) use local files, a fixture, or an HTTP
  server on 127.0.0.1. They are skipped if Chromium cannot start.
- Dependencies (licenses): playwright (Apache-2.0), pillow (MIT-CMU),
  numpy (BSD-3-Clause), websockets (BSD-3-Clause). Development: pytest
  (MIT), ruff (MIT).

## Review worktree

Reviews and verifications build in a persistent git worktree next to the
main repository, `../swb-review` (or `$SWB_REVIEW_TREE`). It has its own
`target/` directory, so builds there are incremental and never share
output with another tree. `tools/review-tree.sh` (`just review-tree`)
prepares it:

| Command                          | What it does |
|----------------------------------|--------------|
| `just review-tree path`          | Prints the path of the worktree. |
| `just review-tree reset [COMMIT]`| Checks out COMMIT (default: `HEAD` of the main repository) and removes local changes and untracked files. Ignored files (`target/`, `out/`, `tools/.venv`) stay. |
| `just review-tree apply PATCH`   | Resets to `HEAD`, then applies the patch (`git apply --index`). |
| `just review-tree staged`        | Resets to `HEAD`, then applies the staged diff of the main repository. |

The script creates the worktree when it does not exist and marks it with
a file `swb-review-tree` in its git directory
(`git -C ../swb-review rev-parse --absolute-git-dir`). It changes only a
marked worktree, so a wrong `$SWB_REVIEW_TREE` cannot discard the work in
another tree. In the worktree, run commands with `env -u CARGO_TARGET_DIR`
so that cargo uses the worktree's own `target/`. Reset the worktree when
the review is done.

Build output and the Python environment contain absolute paths. After you
move or rename the worktree (`git worktree move`), delete its `target/`
and `tools/.venv`.
Sub-agents that implement a feature work in their own worktrees
(`.claude/worktrees/`), each with its own target directory.
