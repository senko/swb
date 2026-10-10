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
| JavaScript engine tests | `crates/js/tests/vm.rs` (scripts and their expected output, compared with Node.js 22), `vm_limits.rs` (limits, termination, the time limit), `gc.rs` (the collector, roots, weak tables, accounting), `limits.rs` (the heap limit, no-GC regions, kind payloads), `objects.rs` (shapes, elements, the internal methods), with `tests/common/`. Each case runs with and without `HeapConfig::stress` (a collection at every safepoint) and fails if a collection finds a stale root. Front end: unit tests in `crates/js-syntax/src/`, `js-text` and `js-regexp`; unit tests of the Python generators in `tools/tests/test_js_unicode_tables.py` | no |
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
| `just tools js-unicode-tables` | `js-unicode-tables`                  | Writes `crates/js-text/src/unicode/tables.rs` (ID_Start, ID_Continue, Zs) from the UCD. |
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
`--heap-limit MIB`, `--stress` (GC at every safepoint), `--time-limit MS`,
`--disassemble` (bytecode listing), `--compile-only`. Exit codes: 0
success; 1 uncaught error (`Uncaught TypeError: message` and
`    at FILE:LINE:COLUMN` on stderr); 2 compile error; 3 termination (time
or heap limit); 64 bad command line.

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
  `decimalToHexString.js` and `nativeErrors.js` (`HARNESS_INCLUDES` in
  `run.rs`). A test is
  skipped if it needs another harness file, has the flag `module`, `async`
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
  `--time-limit MS` (default 10000), `--heap-limit MIB` (default 256),
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
  unsupported, apart from failures. A negative test of regular expression
  syntax that parses fails with the cause "regular expression syntax, M7
  feature 8a", so the table of failure causes shows their count.
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
