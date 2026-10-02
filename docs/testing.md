# Testing

This document describes the test infrastructure and how to use it. The
strategy and its reasons are in [ADR 0005](adr/0005-testing-strategy.md).

## Overview

| Level                  | Where                                   | Runs Chromium                     |
|------------------------|-----------------------------------------|-----------------------------------|
| Unit tests             | each crate, `#[cfg(test)]`              | no                                |
| Layout tests           | `tests/layout/*.html` + `*.boxes.json`  | only to regenerate `*.boxes.json` |
| Page fixtures          | `fixtures/pages/<name>/`                | only to capture and to regenerate `reference/` |
| Scores (ratchet)       | `fixtures/scores.json`                  | no                                |
| Automation API tests   | integration tests of `automation` (not written yet) | no                    |

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
| `just reference [NAME...]`   | `reference NAME... \| --all`           | Writes Chromium's `reference/boxes.json` and `reference/screenshot.png`. |
| `just compare [NAME...]`     | `compare NAME... \| --all`             | Runs swb on fixtures and compares with the references. `just` builds swb first. |
| `just update-scores`         | `compare --all --update-scores`        | Also writes the scores to `fixtures/scores.json`. |
| `just layout-refs [NAME...]` | `layout-refs [NAME...]`                | Writes `tests/layout/NAME.boxes.json` with Chromium. |
| `just tools list`            | `list`                                 | Lists the fixtures, their entry counts and sizes. |
| `just tools-check`           |                                        | ruff lint, ruff format check and pytest of `tools/`. |
| `just tools-fmt`             |                                        | Formats `tools/` and applies safe lint fixes. |

Options:

- `capture --force`: replace an existing fixture (deletes its manifest,
  `files/`, `fixture.json` and `reference/`).
- `capture --with-swb` / `--no-swb`: record swb's requests too, or not.
  Default: use swb if the binary exists.
- `capture --swb PATH`, `compare --swb PATH`: the swb binary. Default: the
  `SWB` environment variable, then `target/release/swb`, then
  `target/debug/swb`.
- `--system-fonts` (`capture`, `reference`, `layout-refs`): use the
  machine's fonts instead of the bundled test fonts. Do not commit output
  made with it.
- `compare --tolerance PX` (default 2), `compare --threshold N` (default 32),
  `compare --no-run` (compare the swb output that is already in
  `out/compare/NAME/`; useful to look at a report again or to test the tool).

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
    Chromium dump for the `relative` score and for element paths. swb can
    omit it.

The elements with a box are the elements whose `getClientRects()` is not
empty. In Chromium this includes some cases that need attention:

- `<br>` has a box of width 0. Empty inline elements have a box of width 0.
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

`capture` and `reference` intercept every request with the Chrome DevTools
Protocol `Fetch` domain (`tools/swbtools/routing.py`), not with Playwright's
`page.route`. Playwright calls a route handler only for the first URL of a
redirect chain, and the browser then fetches the redirect target from the
network. With `Fetch`, every hop pauses, so the tools record each hop and
serve each hop from the fixture.

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
4. `just reference example-page`
5. Check the fixture size (`just tools list`). Keep fixtures small: only
   the target page and its direct resources.
6. Look at `fixtures/pages/example-page/reference/screenshot.png`.
7. `just compare example-page`, then `just update-scores`.
8. Commit the fixture, the reference and `fixtures/scores.json`.

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
- `boxes.json`, `screenshot.png`, `swb.log`: swb's output.
- `reference.png`: a copy of the Chromium screenshot. `diff.png`: the
  pixel difference (red).

The element path in the report has the form
`html > body:nth-child(2) > div:nth-child(3)` (`:nth-child` only when the
parent has more than one element child). Paste it into
`document.querySelector()` in a browser to find the element.

When a change improves the scores, run `just update-scores` and commit
`fixtures/scores.json` with the change. The tool logs every score that goes
down. Scores only go up unless the commit message explains why.

### Add a layout test

1. Write `tests/layout/<name>.html`: one feature, a `<!DOCTYPE html>`, a
   comment that says what it tests, fixed sizes where possible, and the
   generic families `serif`, `sans-serif` or `monospace` (or Arial, Times
   New Roman, Courier New, which map to the bundled fonts).
2. `just layout-refs <name>`
3. Check `tests/layout/<name>.boxes.json` (for example, open the HTML in a
   browser and compare a few rectangles).
4. Commit both files.

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
  differently from the live site, in both browsers.
- Chromium loads web fonts from the fixture. swb does not support
  `@font-face` yet, so text with web fonts differs.
- Alignment works on tags only. When swb builds a different DOM (for
  example template contents in the tree, or different parser recovery),
  elements in the differing blocks count as missing or extra.

## Python tools: development

- Package: `tools/swbtools/`. Tests: `tools/tests/`. Run `just tools-check`
  before a commit that changes `tools/`.
- The tests do not use the network. Tests that start Chromium
  (`tools/tests/test_chromium.py`) use local files, a fixture, or an HTTP
  server on 127.0.0.1. They are skipped if Chromium cannot start.
- Dependencies (licenses): playwright (Apache-2.0), pillow (MIT-CMU),
  numpy (BSD-3-Clause). Development: pytest (MIT), ruff (MIT).
