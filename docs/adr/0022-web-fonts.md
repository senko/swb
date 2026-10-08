# ADR 0022: Web fonts (`@font-face`)

- Status: accepted
- Date: 2026-10-08

## Context

Targets 4 (Ars Technica) and 5 (BBC) set all their text in web fonts.
Ars has 92 `@font-face` rules for three variable families (Source Sans 3,
Faustina, Exo 2), each split into `unicode-range` subsets, with
`font-display: fallback`; Chromium loads four WOFF2 files. BBC has 30
rules with `font-display: block` (BBC Reith; DejaVu Sans substitutes in
the fixture). Without web fonts the geometry scores were 0.18 (Ars) and
0.02 (BBC).

The specifications are CSS Fonts 4 §4 (`@font-face`), §5 (font
matching) and §7 (feature and variation resolution)
(<https://drafts.csswg.org/css-fonts-4/>), WOFF 1.0
(<https://www.w3.org/TR/WOFF/>) and WOFF 2.0
(<https://www.w3.org/TR/WOFF2/>). Where they leave room, Chromium 148 is
the reference, measured black-box with `just probe`
(`tools/probes/web-fonts.json`; ADR 0021).

This is part 1. Part 2 (roadmap, M4 item 4) adds `local()`,
`font-variation-settings`, `font-feature-settings` and the metric
descriptors (`size-adjust`, `ascent-override`, `descent-override`,
`line-gap-override`).

## Decision

### Descriptors (`style/src/font_face.rs`)

The stylist keeps the valid `@font-face` rules of author sheets with
their `@media` chain (`Stylist::font_faces(env)`); `@supports` acts when
the sheet is added, as for style rules. At most 10,000 rules per
document (`MAX_FONT_FACES`).

- A rule needs a valid `font-family` and `src`. An invalid declaration
  is ignored, so an earlier valid one of the same descriptor stays.
- `font-family`: a string or identifiers; a single generic keyword,
  CSS-wide keyword or `default` is invalid.
- `src`: each entry parses on its own; an entry with a syntax error, an
  unsupported `format()` or an unsupported `tech()` is dropped; no entry
  left makes the descriptor invalid. Supported as in Chromium 148:
  formats `woff2`, `woff`, `truetype`, `opentype`, `collection` and the
  `*-variations` strings; techs `variations`, `palettes`,
  `features-opentype`, `features-aat` and the color techs except
  `color-SVG` (swb does not draw color glyphs yet, but the face still
  gives layout its metrics, as in Chromium). Not supported:
  `embedded-opentype`, `svg`, unknown strings, `color-SVG`,
  `features-graphite`, `incremental`. URLs resolve against the style
  sheet's URL (an inline sheet: the document's base URL).
- `font-weight`, `font-stretch`/`font-width`: `auto`, one value or a
  range (swapped if decreasing); keywords only alone (measured).
  `font-style`: `auto | normal | italic | oblique [angle{1,2}]`.
- `unicode-range`: any invalid range makes the whole descriptor invalid
  (initial value `U+0-10FFFF`).
- `font-display` is parsed but every value acts as `swap` with an
  infinite swap period: text shows in the next family until the face
  arrives. Headless rendering waits for fonts (below), so the final
  rendering is the same as Chromium's after its timers; Chromium's block
  and swap periods are not needed for the fixtures.

### Font file formats and limits (`text/src/decode.rs`)

WOFF 2.0 and WOFF 1.0 are decoded by the `wuff` crate (0.2.9, MIT, a
pure-Rust port of Google's C++ woff2 decoder, MIT), with its default
features off. swb passes its own decompressors to wuff's "bring your own
decompressor" functions: `brotli-decompressor` 6 (already used by `net`;
wuff's own feature would add version 5) and `flate2`. OpenType, TrueType
and collections are used as they are (face 0 of a collection). The
format comes from the file's signature, not from the hint or the content
type (measured: a TrueType file with `format('woff')` loads).

Options considered:

1. An own decoder written from the W3C specification: about 900 lines
   (container, Brotli stream, the `glyf`/`loca` and `hmtx` transforms,
   collections), all of it to be reviewed and fuzzed by us.
2. `wuff`: maintained, verified byte-identical with Google's decoder on
   the Google Fonts collection, small (2,500 lines, one dependency,
   `bytes`, already in the tree), no `unsafe`, and its decompression is
   pluggable.

swb uses `wuff`. The audit (2026-10-08) read all of it: header, table
directory and collection parsing use checked reads; it rejects
implausible compression ratios (over 100) and outputs over 128 MiB before
decompressing; the `glyf` transform checks its streams. Two gaps are
closed in swb:

- WOFF 1.0 tables can overlap or repeat (wuff does not check), so a
  small file could expand to terabytes. swb sums the table sizes from the
  directory first and rejects more than `MAX_DECOMPRESSED_SIZE`.
- The decompressors allocate the declared size. swb's closures refuse
  sizes over the limit before allocating and read at most one byte more
  than declared.

Limits: the file at most 32 MiB (`MAX_FONT_FILE_SIZE`), the decompressed
data at most 32 MiB (`MAX_DECOMPRESSED_SIZE`, checked before
decompression), the decoded font at most 64 MiB (`MAX_DECODED_SIZE`; the
`glyf` transform can expand the stream about 2.5 times). A panic in wuff
is caught and becomes a decoding error (`catch_unwind`, as for SVG
images, ADR 0011). Unit tests decode truncated and randomly corrupted
copies of WOFF and WOFF2 files (60,000 corruptions gave no panic before
the test was reduced to 600) and a WOFF2 that declares a 2 GiB table.

After decoding, skrifa and harfrust read the font as they read system
fonts; both check every read. skrifa bounds composite glyphs: a glyph
with 2^14 nested copies of a 3,000-point contour draws nothing and costs
20 ms (hostile case `font-composite-glyphs`).

### Loading model

The text crate owns the face set (`text/src/web.rs`); the engine owns
the files (`engine/src/web_fonts.rs`, `engine/src/page/fonts.rs`).

1. After each style computation whose stylist or media environment
   changed, the engine gives the font context the applicable faces
   (`FontContext::set_web_fonts`). A face equal to one of the previous
   set keeps its id and state; faces that left the set are unloaded (their
   `FontId`s become placeholder fonts, so ids stay valid). A new document
   starts with an empty set.
2. A face loads only when text needs it (CSS Fonts 4 §4.8.1, §5.2):
   itemization requests a face when it reaches it in matching order and
   its `unicode-range` contains the character; it then goes on to the next
   faces and families, so text renders with them meanwhile. The first
   available font (`FontContext::select`, used for line metrics) is the
   composite's face that covers U+0020 if loaded, else its first loaded
   face; if none is loaded, the face for U+0020 is wanted, and requested
   at the end of the layout only if no other face of that composite font
   was requested or loaded. Measured in Chromium 148: a line box without
   text (an `<img>`, a span in another family) loads the face for U+0020;
   text that needs only a cyrillic face of a latin + cyrillic composite
   does not load the latin face, and its line height then comes from the
   cyrillic face; a face whose range excludes U+0020 never loads for a
   line box; `display: none` text loads nothing, `visibility: hidden`
   text does.
3. After layout the engine takes the requests
   (`take_web_font_requests`) and loads each face's sources in order: a
   `url()` through the loader (fixture replay and recording work; a debug
   line `font request: URL` per fetch), `local()` fails for now. Each URL
   is fetched and decoded once, also when many faces use it (Ars uses one
   variable file for nine weights). A source that fails to fetch, decode
   or parse moves on to the next; when none is left the face fails and is
   no longer part of its family. A family whose faces all failed is
   missing, and no system family of that name is used instead (§5.2).
4. A face that arrives invalidates the layout. When a face can use a file
   that is already loaded, it is available at once; `update_layout` then
   runs layout again, up to four passes.
5. Font fetches are pending requests like images: the page is loading
   until they finish, so `--screenshot`, `--dump-boxes` and
   `page.waitForLoad` see the final fonts. `Page::stop` fails the faces
   that are still loading. A later layout can request other faces, and
   they load, as images do after `stop`.

Limits: at most 1,000 faces per family are considered
(`MAX_FACES_PER_FAMILY`) and at most 1,000 face loads start per
document (`MAX_WEB_FONT_LOADS`; later requests fail; the count does not
go down when a face fails or the media environment changes). A
composite font is checked face by face for each character, so
itemization checks only the last 256 faces of a composite
(`MAX_COMPOSITE_CHECKS`; Google Fonts CJK families have about 120
`unicode-range` slices per face) and keeps the result per distinct
cluster for the duration of one call.

### Matching

Family names match ASCII case-insensitively. A web family shadows a
system family of the same name; generic families never resolve to web
faces (measured: a web font named "Liberation Serif" does not change
`serif`). Within a family, the CSS Fonts 4 §5.2 algorithm (the existing
`matching.rs`, ranges included) selects a face by `font-stretch`,
`font-style` and `font-weight`, with `auto` descriptors matching as
`normal`. All faces with the same three descriptors form a composite
font, checked per cluster in reverse rule order (§4.5.1): a face covers a
character if its `unicode-range` contains it and its `cmap` maps it (the
range is applied to the loaded face, so all coverage checks respect it).
Then the next families and the existing system fallback follow.

### Variation axes

For a web face (CSS Fonts 4 §7.2): `wght` is the used `font-weight`
clamped to the `font-weight` descriptor range (not for `auto`), then to
the axis; `wdth` the same with `font-stretch`; `ital` 1 for `italic` when
the descriptor allows italic and the font has the axis, otherwise `slnt`
from the oblique angle (14 degrees; swb does not keep the angle of
`oblique <angle>` yet), never both. Measured: `font-weight: 400` on
Faustina (default instance 300) renders at `wght` 400 at every weight;
Exo 2 with `auto` follows the used weight up to 900. The coordinates are
part of the font's identity (`Variations` in the instance key), so
shaping (harfrust `ShaperInstance`), metrics and outlines (skrifa
locations) use the same values, and the shape plan and glyph mask caches,
which key on the `FontId`, key on them too. System fonts keep setting
only `wght` (ADR 0006).

### Synthesis

Measured in Chromium 148 with the probe's `ink` query (the darkness and
bounding box of what an element paints):

- Synthetic bold for a web face iff the used weight is at least 600, the
  descriptor weight (unless `auto`) is below 600, and the font's own
  weight is below 600: the `wght` value for a variable font, else OS/2
  `usWeightClass` (the bold bits of `fsSelection` and `macStyle` do not
  count). So `font-weight: 400` on DejaVu Sans Bold is never emboldened,
  and Exo 2 with `font-weight: 500 600` is not emboldened at 900.
- Synthetic oblique iff the used style is italic or oblique, the
  descriptor style is `auto` or `normal`, the font data is not italic or
  oblique, and no slant axis was set. `font-style: italic` on an upright
  file is not slanted.

The rendering of synthetic styles is unchanged (ADR 0006).

## Consequences

- Ars Technica geometry 0.1830 → 0.6908, BBC 0.0240 → 0.7775 (fixture
  scores). swb requests exactly the fonts that the fixtures hold.
- Decoded font data stays in memory for the document; the faces of a
  previous document are dropped when the next one commits.
- Not done (backlog): `local()`, the metric descriptors,
  `font-variation-settings` and `font-feature-settings` (part 2);
  `font-display` timers; the angle of `oblique <angle>` in matching
  (Chromium picks an italic face over an `oblique 20deg` face for
  `font-style: oblique`); collections with a fragment (`#PostScriptName`,
  face 0 is used); loading fonts for the `ch` and `ex` units (Chromium
  loads the first available font for them); `size-adjust` fallback faces
  (BBC, Ars).
