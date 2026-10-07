# ADR 0006: Text stack and font matching

- Status: accepted
- Date: 2026-10-02
- Updated: 2026-10-02 (M1 maintenance): the generic-family and
  character-fallback steps now describe the implementation in full. The
  decision did not change.
- Updated: 2026-10-04 (M2 maintenance): see "Update (2026-10-04)" at the
  end. The decision did not change.
- Updated: 2026-10-07 (M3, Wikipedia): see "Update (2026-10-07)" at the
  end: line break rules, glyph advances, small capitals and
  right-to-left shaping as in Chromium. The libraries did not change.

## Context

The `text` crate selects fonts, splits text into runs by font, shapes runs,
gives font metrics and rasterizes glyphs. [ADR 0004](0004-engine-structure.md)
chose harfrust, skrifa, fontconfig and unicode-linebreak, but left the
fontconfig binding open.

Reference comparisons run Chromium on Linux. Layout geometry matches only if
both browsers pick the same font files and get the same metrics and
advances. So the text crate copies Chromium's rules where they are known,
not only the CSS specification.

Sources read for the rules: Blink
`font_cache_skia.cc`, `font_cache_linux.cc`, `alternate_font_family.h`,
`font_platform_data.cc`; Chromium `ui/gfx/font_fallback_linux.cc`,
`ui/gfx/linux/fontconfig_util.cc`; Skia `SkFontConfigInterface_direct.cpp`,
`SkFontHost_FreeType.cpp`, `SkScalerContext.cpp`, `SkTextFormatParams.h`.
Two pieces of data are derived from Skia (BSD-3-Clause; see
`THIRD_PARTY_NOTICES.md`): the metric-compatible family classes
(`GetFontEquivClass`) and the synthetic bold stroke widths
(`SkTextFormatParams.h`). No other code was copied. (Corrected
2026-10-07; this sentence said "no code copied".)

## Decision

### Libraries

| Crate                  | Version | License            | Use                                  |
|------------------------|---------|--------------------|--------------------------------------|
| `harfrust`             | 0.13    | MIT                | shaping                              |
| `skrifa`               | 0.46    | MIT OR Apache-2.0  | font tables, metrics, outlines       |
| `fontconfig`           | 0.11    | MIT                | system fonts (Unix except macOS)     |
| `unicode-linebreak`    | 0.1     | Apache-2.0         | Line_Break property (Unicode 15.0)   |
| `unicode-segmentation` | 1       | MIT OR Apache-2.0  | grapheme clusters for itemization    |
| `unicode-normalization` | 0.1    | MIT OR Apache-2.0  | NFC/NFD forms for coverage checks    |
| `tiny-skia`            | 0.12    | BSD-3-Clause       | glyph masks (already used by paint)  |

harfrust 0.13 and skrifa 0.46 both depend on `read-fonts` 0.43, so the
build has one copy of the font parser. When you upgrade one of them, pick
versions of both that use the same `read-fonts` minor version.

### fontconfig binding

Options:

1. The `fontconfig` crate (YesLogic, MIT): a safe wrapper over
   libfontconfig. It exposes pattern creation, `FcConfigSubstitute`,
   `FcDefaultSubstitute`, `FcFontMatch`, `FcFontSort` and `FcFontList`.
2. `fontdb` (MIT) with `fontconfig-parser`: pure Rust. It reads the font
   directories from the configuration files, but it does not implement
   fontconfig's rule engine (aliases, `<match>` rules, bindings, language
   preferences) or its match scoring. We would have to write that, and it
   would still differ from the real library in edge cases.

We use the `fontconfig` crate with its `dlopen` feature. Chromium calls
libfontconfig; calling the same library with the same patterns gives the
same answers, including the distribution's and the user's configuration.
With `dlopen`, the library is loaded at run time: the build needs no
`-dev` package, and without libfontconfig the browser still runs (without
system fonts). The crate needs no `unsafe` code in swb.

The crate has gaps. We work around them:

- It reads only the first value of a property. Skia accepts a match if any
  of its family names fits. We check the other names by listing the
  requested family and looking for the matched file.
- It cannot add boolean values, so the pattern cannot ask for
  `FC_SCALABLE`. We reject matches whose format is not TrueType or CFF
  instead, as Chromium's validity check does.
- It cannot read character sets. Fallback reads the `cmap` table of each
  candidate font file (only that table, not the whole file).

### Font selection in system mode

- Named family (Skia `SkFontConfigInterfaceDirect::matchFamilyName`):
  fontconfig matches a pattern with the family and regular style. The
  match is accepted if its family is the requested name, the first family
  of the pattern after configuration substitution (a strong alias), or a
  metric-compatible replacement from Skia's fixed table (Arial, Arimo and
  Liberation Sans; Times New Roman, Tinos and Liberation Serif; and so on).
  Otherwise the family does not exist and the next CSS family is tried.
  Before that, Blink's alternate names are tried: Arial and Helvetica,
  Times and Times New Roman, Courier and Courier New.
- Generic family: first the family of Chrome's default font settings on
  Linux (serif: Times New Roman, sans-serif: Arial, cursive: Comic Sans
  MS, fantasy: Impact), resolved with the named-family rules above. If it
  does not exist, fontconfig's match for the CSS keyword, which is always
  accepted. When no family of the list exists, Blink uses its standard
  font, which is serif; fontconfig's `sans` is the last resort. (Measured
  with Chromium: a list of only missing families renders in serif.)
- Within the family: the CSS font matching algorithm (font-stretch, then
  font-style, then font-weight) over the faces that fontconfig lists for
  the family. Chromium instead lets fontconfig score weight, slant and
  width. Both give the same face for normal font families.
- Synthetic bold (Blink `CreateFontPlatformData`): if the requested weight
  is more than 200 above the face weight. So `bold` (700) on a regular face
  is synthesized, `600` is not.
- Synthetic oblique: if italic or oblique is requested and the face is
  upright. (Blink synthesizes only for `italic`; we also do it for
  `oblique`, as the CSS specification says.)
- Variable fonts with a `wght` axis get the requested weight, clamped to
  the axis range, for shaping, metrics and outlines.

### Character fallback

Itemization works on extended grapheme clusters, so marks and variation
selectors stay with their base. For each cluster: the fonts of the
requested families, then system fallback, then the requested families
and the fallback font that have the base character, then the first font
(`.notdef`).
A font covers a cluster if it has glyphs for the cluster as written, or
for its NFC or NFD form, because HarfBuzz composes and decomposes during
shaping (so `e` + U+0301 stays in a font that has `é`).

System fallback follows Chromium's `gfx::GetFallbackFontForChar`:
`FcFontSort` with the content language (default `en-us`), without a
family, then the first usable face whose `cmap` has the character. The
sorted list is computed once per language. Results are cached per
(character, language). As in Blink `PlatformFallbackFontForCharacter`, the
fallback face is used as found (normally the regular face) and gets
synthetic bold if the requested weight is at least 600 and the face is not
bold, and synthetic oblique if the request is slanted and the face is not.

### Directory mode

`FontContext::from_directory` reads the fonts of one directory and uses an
explicit `GenericFamilyMap`: the family for each generic family, a default
family, aliases for missing named families, and a fallback order. It never
calls fontconfig, so tests give the same results on every machine. Fallback
tries the regular face of each fallback family, then every face in order of
file path.

The bundled fonts in `fixtures/fonts` have a matching `fonts.conf`, so that
Chromium can run with exactly the same fonts and rules. A test checks that
system mode with that file agrees with directory mode.

### Metrics

Font metrics follow Skia's FreeType port: the OS/2 typographic ascent,
descent and line gap if `USE_TYPO_METRICS` is set; otherwise `hhea`; if both
`hhea` ascender and descender are zero, the OS/2 typographic values, then
the Windows values. Skia checks `USE_TYPO_METRICS` itself, so Chromium uses
the typographic values for such fonts even with FreeType versions that
ignore the flag. skrifa's `Metrics` implements the same order and applies
`MVAR` deltas for variable fonts. x-height and cap height come from OS/2,
or from the outlines of `x` and `H`. Values are not rounded; Blink rounds
ascent and descent for line layout, which is the layout crate's decision.

### Shaping and rasterization

- Shaping runs in font units and scales to pixels without rounding. Shape
  plans are cached per font; shaping results are not cached.
- Glyph masks: unhinted outlines from skrifa, anti-aliased fill with
  tiny-skia, horizontal subpixel offsets in quarter pixels. Synthetic bold
  strokes and fills the outline like Skia (stroke width: font size times
  1/24 at 9 px, 1/32 at 36 px, linear in between). Synthetic oblique shears
  by 0.25. Advances do not change for synthetic bold, as in Skia.
- Color glyphs (COLR, CBDT, sbix) are not supported yet: `glyph_mask`
  returns `None` and logs a warning once per face.

## Consequences

- System mode needs libfontconfig at run time. It is present on every
  desktop Linux system.
- System mode results depend on the machine, as they do in Chromium. Tests
  use directory mode or the bundled `fonts.conf`.
- Chromium on Linux renders glyphs with FreeType and slight hinting. Our
  unhinted masks differ by a few pixels in pixel comparisons, but advances
  and metrics are the same.
- Font data stays in memory after first use. There is no cap yet. Glyph
  masks are cached up to 64 MiB in total (at most 32K entries); when a new
  mask does not fit, the cache is cleared. Masks larger than 256×256
  pixels are not cached; they are rasterized for every use.
- Not done yet: web fonts (`@font-face`), color glyphs, vertical text,
  `font-synthesis`, Blink's "first family at normal style" fallback step
  for bold or italic text, emoji presentation selection.

## Update (2026-10-04)

Corrections to the consequences above, from the M2 maintenance review:

- The glyph mask cache limits a mask by its area, not by its sides:
  masks of more than 65,536 pixels (256 × 256) are not cached. A
  1024 × 64 mask is cached.
- Directory mode (`FontContext::from_directory`, used by tests and
  `swb --test-fonts`) reads every font file when the context is created,
  not on first use. The data stays in memory.
- System mode keeps the `cmap` table of every face that system fallback
  has checked (`FontconfigSource::cmaps`, at most 16 MiB per table). There
  is no limit on the total.

## Update (2026-10-07)

Measurements with Chromium 148 on Wikipedia's references showed that
plain UAX #14 and unrounded advances do not give Chromium's lines.

- **Break opportunities** (`linebreak.rs`): UAX #14 for Unicode 17.0
  (Chromium 148's version: U+2013 is class HH) with Chromium's
  tailorings. `linebreak.rs` and its tools (`swbtools linebreaks` and
  `linebreak-tables`) are written clean-room from the specification, the
  Unicode Character Database and measurements; no browser or ICU code was
  read for them. `swbtools linebreaks` measures Chromium (each string in a box of width
  0, the line of each code point from its rectangle) and writes the
  opportunity of every pair of U+0020..U+00FF, of every pair of about 120
  samples of all classes in several contexts, and of a list of strings,
  for `word-break: normal`, `break-all`, `keep-all` and `hyphens: none`;
  `crates/text/tests/linebreak.rs` checks every measured string and pair
  (about 360,000).
  `unicode-linebreak` gives the classes of Unicode 15.0;
  `swbtools linebreak-tables` generates the 17.0 differences and the
  other properties (East Asian width, Pi/Pf quotes, letters for
  `keep-all`) from the UCD. The rules, in order:
  - A break after a run of spaces and tabs, before anything (also after
    U+3000 that follows spaces: a break before U+200B, U+2060 and U+200D
    too, measured); never before a space, a tab or U+3000. (So the `SP*`
    forms of LB14 to LB17 never prevent a break.)
  - Between two code points of U+0000..U+00FF, a pair table measured in
    Chromium decides (one for `normal`/`keep-all`, one for `break-all`);
    it differs from UAX #14 in many places, for example no break after
    `!`, `/`, `}` or `|` before letters, and no break between `$`, `+`, `%`
    and other punctuation. Context only for `-` before a digit (a break
    after an ASCII letter or digit only: `978-1`, not `(-1)`) and before
    a letter of U+00A0..U+00FF (LB20a, LB21a).
  - Otherwise UAX #14, with CJ as ID, SA as AL (marks as CM), BK and NL
    breaking after them without forcing, and look-behind that stops at
    the last break opportunity (so a hyphen after `?` or `-` is
    word-initial for LB20a).
  - `break-all`: breaks added between letters (AL, AI, HL, NU, SA, not
    XX) and letters, OP, PR, BA and HY, and between some punctuation and
    letters or hyphens (a measured class table); `keep-all`: no break
    between two letter units (General_Category L or N, not SA).
  - `hyphens: none`: no break after U+00AD, except the breaks of
    `break-all`.

  Differences that remain: Chromium breaks Thai and Lao at dictionary
  word boundaries (swb only at spaces); Chromium does not break inside a
  glyph cluster (layout skips such opportunities too) and, with
  `break-all`, where marks are shaped across the position; some control
  characters. 27 measured strings and pairs differ, listed with reasons in
  `crates/text/tests/linebreak/known-differences.txt`.
- **Advances** (`shape.rs`, `chromium_sizes` and `font_scale`), as
  measured in Chromium 148 by `swbtools measure font-size-sweep`
  (`tools/swbtools/font_sizes.py`) for every CSS size from 9.000 to
  24.000 px in steps of 0.001 px (15,001 sizes, each alone on a new
  page; the results are `crates/text/tests/chromium-font-sizes.txt` and
  `chromium-font-widths.txt`): the font size is truncated to 1/100 px in
  `f32` (16.21 px becomes 16.2 px, because 16.21 in `f32` is just below
  it), kerning uses that size, and the nominal advances use it truncated
  again to 1/64 px (16.1875 px; 14.390625 px for 14.4 px). Each part is
  rounded to 16.16 fixed point. The kerning of a pair is its font units
  times the kerning size in 16.16 fixed point, truncated (`HarfBuzz`'s
  font scale), divided by `unitsPerEm` and rounded; the untruncated size
  gives 1/65536 px more or less per pair, and a different width of 200
  glyphs for 10 of 1,154 sizes. The unit test
  `font_size_rule_matches_chromium` checks the two sizes at all 15,001
  sizes; `tests/font_widths.rs` checks the widths of 20 digits in three
  families and of 100 times `AV` in Liberation Sans (strong kerning) at
  1,154 of them.

  An earlier version of this ADR said that all 645 sampled widths were
  equal. That held for the sampled sizes only. A scan of all 15,001
  sizes in one page (as the `font-size` of 15,001 `<div>` elements)
  differs from the rule for 684 of them, 1/64 px in the advance size:
  see "Shared fonts" below.

  Layout adds advances exactly and rounds the width of each text item on a
  line up to 1/64 px, as the difference of the rounded advances from the
  start of the item: exact for an item that starts on the line, up to
  1/64 px less for one that continues from the line before (and for a word
  cut by `overflow-wrap`, whose two parts are measured separately).
  Content fits on a line if it is at most 1/64 px wider. Chromium first
  converts the exact sum of the advances of an item to `f32`, which has
  steps of 1/8192 px from 1,024 px and of 1/4096 px from 2,048 px; for the
  200 glyphs of the `AV` text (1,200 to 3,200 px) that changes the rounded
  width of 41 of 15,001 sizes. Layout does not do this (shorter text is
  affected less).
- **Right-to-left text**: with `Direction::Ltr`, the characters of
  right-to-left scripts (Arabic, Hebrew; with their marks) are shaped
  right to left in their own segments, so that their letters join and
  get their right-to-left positions, and their clusters are returned in
  logical order (there is no bidi reordering yet). The other characters
  are shaped left to right, so brackets are not mirrored.
  `ShapeOptions::pre_context` and `post_context` give `HarfBuzz` the text
  around a run, so that Arabic joins across font fallback and element
  boundaries, as measured in Chromium 148.
  Layout ends the context at the edges of isolating boxes
  (`unicode-bidi: isolate`, `isolate-override`, `plaintext`; `<bdi>`,
  `dir`) and at forced line breaks: measured in Chromium 148, Arabic does
  not join across them (it joins across `embed` boxes, floats and
  absolutely positioned boxes).
  A box with `unicode-bidi: bidi-override` or `isolate-override` and
  `direction: ltr` makes the shaper treat its text as left to right
  (`ShapeOptions::bidi_override`): its Arabic letters then join
  differently. Measured in Chromium 148 for the word `بال` at 16 px in DejaVu
  Sans between Arabic words: 13.58 px with `bidi-override` (right to left:
  14.2 px, as without `unicode-bidi`), and 24.19 px with `isolate-override`
  (right to left, and `isolate`: 20.95 px). With `direction: rtl` the
  override boxes give the widths of right-to-left text. The box's own
  context rules stay as above (`bidi-override` takes the context of the
  words around it, `isolate-override` does not). The innermost box with a
  `unicode-bidi` other than `normal` decides; an override on a block does
  not reach text in inline boxes inside it (not done).
- **`FontContext::has_feature`** tells whether a font has an OpenType
  feature; layout synthesizes small capitals when the primary font has
  no `smcp`, and chooses the fonts for the uppercased text as measured in
  Chromium 148 (see `layout/src/inline/caps.rs`); a fallback font
  without `smcp` synthesizes them itself (not measured: no bundled font
  has `smcp`). `FontContext::covers_cluster`
  tells whether a font has the glyphs of a grapheme cluster.
  For the line height, a fallback font counts with its metrics at the
  size it is shaped at, so small capitals (70 % of the font size, whole
  pixels) in a fallback font make a shorter line than the same letters at
  the font size. Measured in Chromium 148 (`tests/layout/text-small-caps-metrics.html`):
  `font: 16px sans-serif; font-variant: small-caps` with `xաy` (the
  Armenian letter comes from DejaVu Sans) gives a line of 18 px, as
  without the letter, and 19 px without `small-caps`; an Armenian capital
  letter, which stays at the font size, gives 19 px again. The primary
  font keeps the metrics of the font size: its box has them already.
- **Shared fonts** (not done). In one page, Chromium makes a font once for
  each key, which is the font size truncated to 1/100 px twice
  (`trunc(100 * (trunc(100 * s) / 100))` in `f32`), and the first size
  that asks for a key decides the sizes of every later size with that
  key. This changes the result only for the 92 of 1,501 hundredths of a
  pixel between 9.00 and 24.00 px whose `f32` value times 100 is below
  the decimal number (9.11, 9.15, 9.19, 9.23, 9.36, ... 10.23 and 16.05
  ... 20.47): a page that asks for 9.110 px and then for 9.115 px gets the
  sizes of 9.110 px (advance size 582/64 px, not 583/64 px) for both. In a
  scan of all 15,001 sizes in one page (ascending), 684 sizes (4.6 %) get
  the sizes of an earlier size; the model predicts every one of the
  15,001 results. A review scan of this kind found 531 sizes where the
  advance size of swb was 1/64 px larger than that of Chromium; they are
  this effect (and so are the examples 16.211 to 16.220 px, which get the
  sizes of 16.2 px). swb shapes each size alone, which is what a page gets
  whose sizes differ by more than 0.01 px.

Not done: the hyphen that Chromium draws at a break after U+00AD;
dictionary breaks for Thai, Lao and other complex-context scripts;
bidi reordering; the shared fonts above; the `bidi-override` or
`isolate-override` of a block for the text in inline boxes inside it.
Chromium shapes the start and the end of a line again
when the line breaks inside a text item (measured: at a `break-all`
break inside `AVAV…`, the kerning pair across the break is gone); swb
keeps the shaping of the whole text, so at a `break-all` or
`overflow-wrap` break a kerning pair or an Arabic joining across the
break stays (a line can be up to one kerning value narrower than in
Chromium).
