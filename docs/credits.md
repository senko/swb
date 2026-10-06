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
- SVG 2 (coordinate systems; natural dimensions and the `viewBox`
  transform of SVG images) and CSS Images 3 (natural dimensions, the
  default sizing algorithm, the default object size) — `paint/src/svg/`,
  `layout/src/replaced.rs`
- HTML Living Standard, forms: form submission, constructing the entry
  list, implicit submission, value sanitization, radio button groups,
  selectedness, labels, constraint validation (`engine/src/forms/`);
  WHATWG URL `application/x-www-form-urlencoded` serializer; Infra
  newline normalization; RFC 7578 (`multipart/form-data`)
- RFC 6265bis (IETF draft, HTTP State Management Mechanism) — cookie
  parsing, storage, retrieval and `SameSite` (`net/src/cookies/`)
- WHATWG Fetch, "append a request `Origin` header" — the `Origin` header
  of `POST` requests

## Books and articles

- "Web Browser Engineering", Pavel Panchekha and Chris Harrelson —
  https://browser.engineering/ — overall structure of a minimal browser
  (layout tree, display list, the order of pipeline stages).
- Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms" —
  https://howardhinnant.github.io/date_algorithms.html — `days_from_civil`
  (`net/src/cookies/date.rs`).

## Projects (read for ideas only)

- Chromium / Blink (BSD-3-Clause, parts LGPL) — font selection and
  fallback rules, so that swb picks the same fonts as Chromium on Linux
  (`text` crate, ADR 0006). Files read: Blink `font_cache_skia.cc`,
  `font_cache_linux.cc`, `alternate_font_family.h`,
  `font_platform_data.cc`; Chromium `ui/gfx/font_fallback_linux.cc`,
  `ui/gfx/linux/fontconfig_util.cc`. Also Blink's rounding of font metrics
  on Linux (`SimpleFontData`) and the `vertical-align: sub/super` offsets
  (`layout` crate). No code copied.
- Chromium / Blink LayoutNG table layout (BSD-3-Clause): in
  `third_party/blink/renderer/core/layout/table/`:
  `table_layout_utils.cc/.h`, `table_layout_algorithm.cc`,
  `table_layout_algorithm_types.cc/.h`,
  `table_section_layout_algorithm.cc`, `table_row_layout_algorithm.cc`,
  `table_borders.cc/.h`, `table_node.cc`, `layout_table_cell.cc/.h`,
  `layout_table_column_visitor.h`; in `core/layout/`: `length_utils.cc`
  (table width, auto margins, replaced contributions),
  `block_layout_algorithm.cc` (the `-webkit-` text-align offset, baseline
  propagation), `block_layout_algorithm_utils.cc`, `layout_utils.cc`,
  `block_node.cc`, `inline/inline_box_state.cc/.h`,
  `inline/inline_layout_algorithm.cc`, `inline/logical_line_builder.cc`,
  `inline/inline_item.cc`, `inline/inline_node.cc/.h`,
  `inline/line_breaker.cc` (line height quirks); `core/paint/
  table_painters.cc` (collapsed borders). Read for the algorithms
  (`layout/src/table/`, ADR 0010); no code copied.
- Chromium / Blink LayoutNG grid layout (BSD-3-Clause): in
  `third_party/blink/renderer/core/layout/grid/`:
  `grid_layout_algorithm.cc`, `grid_track_sizing_algorithm.cc`,
  `grid_track_collection.cc`, `grid_placement.cc`,
  `grid_layout_utils.cc`, `grid_item.cc`, `grid_baseline_accumulator.h`;
  `core/css/properties/css_parsing_utils.cc`. Read for the ranges and
  sets of tracks, the distribution of extra space, the automatic
  repetitions, the minimum contribution, item alignment, the
  auto-placement cursors, grid baselines, grid value parsing and limits
  (`kGridMaxTracks`), and `TableNode::AllowColumnPercentages`
  (`layout/src/grid/`, `style/src/parse/grid.rs`, ADR 0017). No code
  copied.
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
  87.5% page step; the scrollable overflow of scroll containers
  (`scrollWidth`, `scrollHeight`), scroll chaining of the wheel and keys,
  scroll into view of nested containers, whole-pixel scroll offsets.
  Blink's design names (`ScrollableOverflowCalculator`,
  `ScrollManager::LogicalScroll`, `ScrollRectToVisible`) from
  recollection.
- Chromium / Blink (BSD-3-Clause), forms, from recollection and
  confirmed by measurements with Chromium 148: text field widths
  (`layout_text_control.cc`: `GetAvgCharWidth`, `HasValidAvgCharWidth`
  and its family list, `PreferredContentLogicalWidth`), select option
  widths and optgroup indentation (`LayoutMenuList::UpdateOptionsWidth`),
  the check mark of `NativeThemeBase::PaintCheckbox`, label activation
  (`HTMLLabelElement::DefaultEventHandler`) and line breaks in pasted
  text (`TextFieldInputType`). No code copied.
- Chromium (BSD-3-Clause), cookies: `net/cookies/cookie_monster.cc`
  (limits of 180/150 cookies per domain and 3300/3000 in total, LRU
  eviction), `net/cookies/cookie_util.cc` (`GetCookieDomainWithString`:
  the `Domain` attribute rule; `ComputeSameSiteContext`: the same-site
  contexts), `registry_controlled_domains` (unknown registries have no
  registrable domain). No code copied.
- usvg, resvg, svgtypes, simplecss, roxmltree, kurbo (Apache-2.0 OR MIT,
  MIT) — their source was read to mirror their behaviour in the SVG
  limits (`paint/src/svg/`): usvg's size resolution, viewBox transform,
  id and href resolution, caching and recursion; simplecss's selector
  matching; roxmltree's entity limits; kurbo's arc subdivision count.
  No code copied.
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

## Data in dependencies

- Public Suffix List (Public Suffix List project, Mozilla; **MPL-2.0**) —
  compiled into a table in the `psl` crate (the crate's hand-written code
  is `MIT OR Apache-2.0`; the table is the list, under MPL-2.0) —
  registrable domains for cookies and sites (`net/src/site.rs`). A
  one-off exception to the dependency policy, see ADR 0014 and
  `THIRD_PARTY_NOTICES.md`.
