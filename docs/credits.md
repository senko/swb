# Credits and sources of inspiration

This file lists every source that influenced the design or code of swb:
specifications, articles, books, and other projects whose code was read for
ideas. No code was copied from these projects.

Format: source, license (for code), what it influenced.

## Specifications

- WHATWG HTML Living Standard — https://html.spec.whatwg.org/
- WHATWG URL, Fetch, Encoding standards — https://url.spec.whatwg.org/,
  https://fetch.spec.whatwg.org/, https://encoding.spec.whatwg.org/
- CSS specifications (W3C CSS WG) — https://www.w3.org/Style/CSS/specs.en.html
  - CSS 2.2 (visual formatting model, box model, tables, floats)
  - CSS Syntax Level 3 (tokenizer and parser)
  - Selectors Level 4
  - CSS Cascading and Inheritance Level 4
  - CSS Values and Units Level 4
  - CSS Flexible Box Layout Level 1
  - CSS Grid Layout Level 2
  - CSS Cascade 4/5, Values 4/5, Color 4/5, Variables 1, Display 3,
    Backgrounds 3/4, Fonts 4, Text 3/4, UI 4, Align 3, Images 4, Lists 3,
    Content 3, Overflow 3, Sizing 3, Conditional 3/4 (`selector()`),
    Media Queries 4
  - CSSOM: serialization of identifiers, strings and selectors
    (`css/src/serialize.rs`, `css/src/selector/display.rs`)
  - HTML Living Standard, Rendering section (CC BY 4.0): the user-agent
    stylesheets `crates/style/src/ua.css` and `ua-quirks.css` and the
    presentational hints in `crates/style/src/hints.css` are based on its
    CSS
  - HTML Living Standard, the list of attributes whose values selectors
    match ASCII case-insensitively (`css/src/selector/parse.rs`)
  - Quirks Mode Standard — https://quirks.spec.whatwg.org/
  - HTML Living Standard, interaction and DOM sections: focusable areas and
    sequential focus navigation (`engine/src/focus.rs`), the `innerText`
    algorithm (the copied text, `engine/src/selection.rs`), the HTML
    fragment serialization algorithm (`dom/src/serialize.rs`)
- JSON-RPC 2.0 — https://www.jsonrpc.org/specification — message shape and
  error codes of the automation protocol
- UI Events KeyboardEvent key values — https://www.w3.org/TR/uievents-key/ —
  key names of the engine and the automation protocol
- Unicode Standard Annex #14 (line breaking), #9 (bidi)
- RFC 4647 (language tag matching) — `:lang()`

## Books and articles

- "Web Browser Engineering", Pavel Panchekha and Chris Harrelson —
  https://browser.engineering/ — overall structure of a minimal browser
  (layout tree, display list, the order of pipeline stages).

## Projects (read for ideas only)

- Chromium / Blink (BSD-3-Clause, parts LGPL) — font selection and
  fallback rules, so that swb picks the same fonts as Chromium on Linux
  (`text` crate, ADR 0006). Files read: Blink `font_cache_skia.cc`,
  `font_cache_linux.cc`, `alternate_font_family.h`,
  `font_platform_data.cc`; Chromium `ui/gfx/font_fallback_linux.cc`,
  `ui/gfx/linux/fontconfig_util.cc`. Also Blink's rounding of font metrics
  on Linux (`SimpleFontData`) and the `vertical-align: sub/super` offsets
  (`layout` crate). No code copied.
- Chromium / Blink, style: `html.css` and `quirks.css` (user-agent
  defaults, compared with the HTML spec), `font_size_functions.cc` (font
  size keyword tables), `font_builder.cc` (`CheckForGenericFamilyChange`,
  the monospace size rule), `style_builder_converter.cc` (font size
  absoluteness), and LayoutNG's `CalculateLeadingSpace` (half-leading
  rounding). Chrome's default font settings
  (`locale_settings_linux.grd`), checked against Chromium 148. No code
  copied.
- Chromium 148, measured with Playwright (behavior, no code read): the
  focus ring of `outline-style: auto` (two rings, offsets, radii), the
  selection colors, `a:any-link:focus-visible { outline-offset: 1px }` in
  `html.css`, the 4 px drag threshold, 40 px arrow-key scrolling and the
  87.5% page step.
- Servo and Blink — the idea of an ancestor Bloom filter for selector
  matching (`SelectorFilter`), and right-to-left selector matching with
  limited backtracking, which all browser engines use. No code read for
  swb's implementation.
- Björn Ottosson, "A perceptual color space for image processing" — the
  OKLab matrices (`style` crate color conversion), and the sample
  conversion code in CSS Color 4.
- Skia (BSD-3-Clause) — metric selection and synthetic bold/oblique
  parameters (`text` crate). Files read: `SkFontConfigInterface_direct.cpp`,
  `SkFontHost_FreeType.cpp`, `SkScalerContext.cpp`, `SkTextFormatParams.h`.
  No code copied.

## Bundled data

- Liberation fonts 2.1.5 (Sans, Serif, Mono; Regular, Bold, Italic,
  BoldItalic) — SIL Open Font License 1.1 — `fixtures/fonts/`, test fonts.
- DejaVu Sans 2.37 (Regular, Bold) — Bitstream Vera license, DejaVu changes
  in the public domain — `fixtures/fonts/`, test fonts.

See `fixtures/fonts/README.md` for file details.
