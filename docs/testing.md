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
| JavaScript engine tests | `crates/js/tests/vm.rs` (scripts and their expected output, compared with Node.js 22), `vm_limits.rs` (limits, termination, the time limit), `gc.rs` (the collector, roots, weak tables, accounting), `limits.rs` (the heap limit, no-GC regions, kind payloads), `objects.rs` (shapes, elements, the internal methods), with `tests/common/`. Each case runs with and without `HeapConfig::stress` (a collection at every safepoint) and fails if a collection finds a stale root. Front end: unit tests in `crates/js-syntax/src/`, `js-text` and `js-regexp`; regular expressions: `crates/js-regexp/tests/` (see "Regular expression tests") and the `RegExp` built-in: `crates/js/tests/builtins_regexp.rs`, `regexp_hostile.rs` (see "RegExp built-in tests"); `Proxy`: `builtins_proxy.rs`, `proxy_hostile.rs` (see "Proxy tests"); the weak collections: `builtins_weak.rs`, `weak.rs`, `weak_hostile.rs` (see "Weak collection tests"); `BigInt`: `builtins_bigint.rs`, `bigint_hostile.rs` (see "BigInt tests"); unit tests of the Python generators in `tools/tests/test_js_unicode_tables.py` | no |
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
| `just capture-missing NAME`  | `capture-missing NAME`                 | Adds only the responses that the fixture does not have. |
| `just substitute NAME...`    | `substitute NAME... [--check]`         | Replaces photos with placeholders and non-free fonts with free ones in a fixture (see "Substitute copyrighted content"). |
| `just reference [NAME...]`   | `reference NAME... \| --all`           | Writes Chromium's `reference/boxes.json` and `reference/screenshot.png`. |
| `just compare [NAME...]`     | `compare NAME... \| --all`             | Runs swb on fixtures and compares with the references. `just` builds swb first. |
| `just compare NAME --full-page`    | `compare NAME --full-page`       | Also compares the full-page screenshots and lists the differing regions (see "Compare swb with Chromium"). |
| `just compare NAME --click SEL` | `compare NAME --click SELECTOR` | Compares the page after clicking the first element of SELECTOR in both browsers (repeatable). |
| `just update-scores`         | `compare --all --update-scores`        | Also writes the scores to `fixtures/scores.json`. |
| `just layout-refs [NAME...]` | `layout-refs [NAME...]`                | Writes `tests/layout/NAME.boxes.json` with Chromium. |
| `just perf [NAME...]`        | `perf [NAME...] [--runs N]`            | Times swb's pipeline stages per fixture ([performance.md](performance.md)). |
| `just tools measure [NAME...]` | `measure [NAME...]`                  | Measures the Chromium behaviour that some of swb's data comes from (see "Measure Chromium behaviour"). `font-size-sweep` runs only when named. |
| `just probe FILE...`         | `probe FILE... [--case NAME]... [--with-swb] [--tolerance PX] [--json]` | Asks Chromium (and with `--with-swb`, swb) about the cases of JSON files: boxes, line fragments, computed styles, JS values (see "Probe Chromium behaviour"). |
| `just hostile [NAME...]`     | `hostile [NAME...] [--swb PATH] [--list] [--keep]` | Runs swb on the hostile-page set and checks time and memory limits (see "Hostile-page set"). `just` builds swb first. |
| `just test262 [ARGS]`        |                                        | Runs the test262 subset in `swb-js` and compares the pass counts with `crates/js/test262/scores.json`; `--parse-only` runs only the front end over `test/language/`, `test/built-ins/` and `test/annexB/` against `parse-scores.json` (see "JavaScript engine tools"). |
| `just jsdiff FILE...`        | `jsdiff FILE...`                       | Runs files in `swb-js` and in Node.js and diffs stdout and the uncaught error. |
| `just jsbench [DIR]`         | `jsbench [DIR] [--runs N]`             | The engine's benchmark programs against `node --jitless`; for a directory of scripts also lexing, parse and compile ([performance.md](performance.md)). |
| `just tools list`            | `list`                                 | Lists the fixtures, their entry counts and sizes. |
| `just tools linebreaks`      | `linebreaks [--out DIR]`               | Measures Chromium's line break opportunities into `crates/text/tests/linebreak/` (see below). |
| `just tools linebreak-tables` | `linebreak-tables`                    | Writes `crates/text/src/linebreak/tables.rs` from the UCD and `latin1.txt`. |
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
  Default: use swb if the binary exists.
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
  boxes and to scroll.
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
stored headers (`content-type`, `location`) and
`Access-Control-Allow-Origin: *`. The fixture does not store CORS headers;
without this header Chromium would block cross-origin fonts. swb does not
check CORS.

In replay, a request for a URL that is not in the fixture fails, and the
tool logs the URL.

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
`rects`, `style` and `js` are Chromium only.
Exit status 1 if a compared box is outside the tolerance, a tag differs, or
swb fails; so `just probe --with-swb FILE` works as a quick check.

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
limit acts), `swb_args` (for example `--full-page`, `--scale 8`) and
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
`$262` has `evalScript`, `gc` and `global`; `createRealm`,
`detachArrayBuffer` and `agent` are not available. Options:
`--heap-limit MIB`, `--stress` (GC at every safepoint, fewer after the
first 65,536; ADR 0026 section 8), `--time-limit MS`,
`--disassemble` (bytecode listing), `--compile-only`. The promise jobs
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
  directory, for example `test/language/expressions/addition`.
- Every test gets `sta.js` and `assert.js`, then the files of its
  `includes` in the test's order (each file once; the runner reads each
  file once per run). The accepted files are `propertyHelper.js`,
  `compareArray.js`, `isConstructor.js`, `fnGlobalObject.js`, `nans.js`,
  `decimalToHexString.js`, `nativeErrors.js`, `asyncHelpers.js`,
  `promiseHelper.js`, `regExpUtils.js`, `compareIterator.js` and
  `proxyTrapsHelper.js` (`HARNESS_INCLUDES` in `run.rs`). A test with the
  flag `async` gets `doneprintHandle.js` before its includes; the runner
  runs the jobs after the test (as the shell does) and passes it only when
  it printed `Test262:AsyncTestComplete` and no
  `Test262:AsyncTestFailure:` line. A promise rejected without a handler
  does not fail a test. A test is
  skipped if it needs another harness file, has the flag `module`
  or `CanBlockIsTrue`, has a
  feature outside the list, uses `$262.createRealm`, `$262.agent` or
  `$262.detachArrayBuffer`, or expects an error in the `resolution` phase.
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
