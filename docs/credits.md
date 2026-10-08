# Credits and sources of inspiration

This file lists every source that influenced the design or code of swb:
specifications, articles, books, and other projects whose code was read for
ideas. Some parts of swb's code are derived from Chromium, Skia and Servo
code; the entries below say which, and
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) lists them with their
licenses. No other code was copied from these projects.

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
  - CSS Lists 3 and CSS Counter Styles 3 (counters, list item numbers,
    the predefined counter styles in `style/src/counter_style.rs`)
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
- Unicode Standard Annex #14 (line breaking; revisions 51 to 57, for
  Unicode 15.1 to 18.0) and the Unicode Character Database 17.0
  (`LineBreak.txt`, `EastAsianWidth.txt`, `DerivedGeneralCategory.txt`,
  `emoji-data.txt`) — `text/src/linebreak.rs` and its generated
  `linebreak/tables.rs`; CSS Text 3 §5 (`word-break`, `hyphens`); UAX #9
  (bidi)
- RFC 4647 (language tag matching) — `:lang()`
- CSS Fonts 4 §4, §5 and §7 (`@font-face`, font matching, variation
  resolution) — `style/src/font_face.rs`, `text/src/web.rs`; §4 (the descriptors
  `size-adjust`, `ascent-override`, `descent-override`,
  `line-gap-override` and `src: local()`), §6 (`font-feature-settings`)
  and §7 (`font-variation-settings`) — `style/src/font_settings.rs`,
  `style/src/font_face.rs`, `text/src/web.rs`; WOFF 1.0 and
  WOFF 2.0 (W3C) — the container formats and the limits of
  `text/src/decode.rs` (ADR 0022)
- SVG 2 (coordinate systems; natural dimensions and the `viewBox`
  transform of SVG images) and CSS Images 3 (natural dimensions, the
  default sizing algorithm, the default object size) — `paint/src/svg/`,
  `layout/src/replaced.rs`
- SVG 2 for inline SVG (ADR 0023): §6.6 presentation attributes and the
  user-agent style sheet (`style/src/svg_attributes.rs`, `ua.css`); §8.2
  the `viewBox` transform and §8.9 units (`layout/src/svg/viewport.rs`,
  `draw.rs`); §9 path data and its error handling, §10 the equivalent
  paths of the basic shapes, Appendix B.2.4 and B.2.5 (elliptical arc
  conversion to the center parameterization, out-of-range radii)
  (`layout/src/svg/path.rs`); §13 fill and stroke properties
  (`style/src/properties/svg.rs`). CSS Transforms 1 (`transform-box`,
  the initial `view-box` reference box) and CSS Color 4 (`currentColor`
  as a computed keyword). The cubic Bézier approximation of circular arcs
  (control points at 4/3 · tan(θ/4) of the radius) is standard geometry.
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

## Projects

- `wuff` (MIT, Nico Burns; a port of Google's woff2 decoder, MIT) — a
  dependency for WOFF and WOFF2 decoding; its source was read for the
  robustness audit of ADR 0022. No code copied.

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
  (`layout/src/table/`, ADR 0010). Parts of `layout/src/table/columns.rs`,
  `distribute.rs`, `rows.rs`, `cells.rs` and `layout.rs` are derived from
  `table_layout_utils.cc`, `table_layout_algorithm_types.cc/.h` and
  `table_layout_algorithm.cc`; see `THIRD_PARTY_NOTICES.md`.
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
  (`layout/src/grid/`, `style/src/parse/grid.rs`, ADR 0017). Parts of
  `layout/src/grid/sizing.rs` and of `auto_repetitions` in
  `layout/src/grid/mod.rs` are derived from
  `grid_track_sizing_algorithm.cc/.h` and `grid_layout_utils.cc`; see
  `THIRD_PARTY_NOTICES.md`.
- Chromium / Blink, style: `html.css` and `quirks.css` (user-agent
  defaults, compared with the HTML spec; LGPL-2), `font_size_functions.cc`
  (font size keyword tables; LGPL-2), `font_builder.cc`
  (`CheckForGenericFamilyChange`, the monospace size rule),
  `style_builder_converter.cc` (font size absoluteness), and LayoutNG's
  `CalculateLeadingSpace` (half-leading rounding). Chrome's default font
  settings (`locale_settings_linux.grd`), checked against Chromium 148.
  Until 2026-10-07, the font size keyword tables
  (`style/src/values/keywords.rs`) and the form control, ruby and
  frameset rules of `style/src/ua.css` restated data from `html.css` and
  `font_size_functions.cc`. Data derived from LGPL files is not allowed
  (ADR 0003), so they now use the HTML Standard and values measured in
  Chromium 148 (`swbtools measure`). No code copied.
- Chromium 148, measured with Playwright (behavior, no code read): the
  focus ring of `outline-style: auto` (two rings, offsets, radii), the
  selection colors, `a:any-link:focus-visible { outline-offset: 1px }` in
  `html.css`, the 4 px drag threshold, 40 px arrow-key scrolling and the
  87.5% page step; the scrollable overflow of scroll containers
  (`scrollWidth`, `scrollHeight`), scroll chaining of the wheel and keys,
  scroll into view of nested containers, whole-pixel scroll offsets;
  responsive image selection (candidate order and choice for the device
  pixel ratio, `w` densities, `sizes` with `auto` and negative `calc()`,
  `<source type>` matching, a `<source>` with both a `media` that does
  not match and an unsupported `type`, the dimensions of a selected
  `<source>`, density-corrected sizes); with `swbtools measure`
  (docs/testing.md): the font families whose text fields use the width
  of `0`, the font size keyword tables, the computed styles of form
  controls, ruby and frameset elements, and the `<source>` checks; line
  break opportunities for `word-break: normal | break-all | keep-all`
  and `hyphens: none` (`text/src/linebreak.rs`, written from these
  measurements and UAX #14 only; `swbtools linebreaks`); web fonts:
  supported `format()` and `tech()` values, descriptor validity, which
  faces load for text and line boxes, composite font order, variation
  axis values and the rules for synthetic bold and oblique
  (`tools/probes/web-fonts.json`, ADR 0022); web fonts, part 2: which
  names `local()` matches (full name and PostScript name, ignoring case
  and spaces), how the metric descriptors scale and override the line
  box (overrides are ratios of the adjusted size), the order of the axis
  values (matching, `@font-face` descriptor, property), the serialization
  and the validity of the two settings properties, which of them the
  `font` shorthand resets, and that synthetic bold ignores
  `font-variation-settings` (`tools/probes/web-fonts-2.json`, ADR 0022);
  inline SVG: the sizes of the outer `<svg>` from its `width`, `height`
  and `viewBox` (presentation attributes, natural size, invalid values),
  its baseline and flex behaviour, the content-box clip, presentation
  attribute precedence, `currentColor` inheritance, `url()` paints
  without fallback, fill, stroke, dash, opacity and transform results by
  their ink (`tools/probes/inline-svg.json`, ADR 0023).
  Blink's design names (`ScrollableOverflowCalculator`,
  `ScrollManager::LogicalScroll`, `ScrollRectToVisible`) from
  recollection.
- Chromium / Blink, forms (`layout_text_control.cc` and `LayoutMenuList`
  and its successors are LGPL-2; `ui/native_theme/` is BSD-3-Clause),
  from recollection and confirmed by measurements with Chromium 148: text
  field widths (`layout_text_control.cc`: `GetAvgCharWidth`,
  `HasValidAvgCharWidth`, `PreferredContentLogicalWidth`), select option
  widths and optgroup indentation (`LayoutMenuList::UpdateOptionsWidth`),
  the check mark of `NativeThemeBase::PaintCheckbox`, label activation
  (`HTMLLabelElement::DefaultEventHandler`) and line breaks in pasted
  text (`TextFieldInputType`). The list of families whose text fields use
  the width of `0` (`layout/src/control.rs`) comes from a measurement
  (`swbtools measure text-field-families`, 2026-10-07), not from
  `layout_text_control.cc`. The check mark in `paint/src/control.rs` is
  derived from `ui/native_theme/native_theme_base.cc`; see
  `THIRD_PARTY_NOTICES.md`. No other code copied.
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
- Chromium / Blink (BSD-3-Clause), text layout and shaping (M3), read at
  `main` before the owner's rule of 2026-10-07 (ADR 0021):
  `core/layout/inline/line_breaker.cc/.h` (break rules at element
  boundaries, the `overflow-wrap` retry, the fit tolerance),
  `core/layout/inline/inline_node.cc` (where shaping stops at inline box
  edges, the shaping context),
  `platform/fonts/shaping/shaping_line_breaker.cc` and `shape_result.cc`
  (snapped widths); the synthesis of small capitals and the font size
  rules from recollection. Each rule was then measured in Chromium 148,
  and the code cites the measurements (`layout/src/inline/breaks.rs`,
  `overflow.rs`, `caps.rs`, `shaping.rs`; `text/src/shape.rs`). A first
  line breaker, written after reading Blink's `text_break_iterator.cc`
  (LGPL) and `character_property_data_generator.cc`, was discarded before
  it was committed; `text/src/linebreak.rs` was written in a clean room
  from UAX #14 and measurements (ADR 0021).
- Chromium / Blink LayoutNG floats (BSD-3-Clause), from recollection and
  confirmed by measurements with Chromium 148: the exclusion space and
  layout opportunities, `BlockLayoutAlgorithm::HandleFloat`,
  `NextBorderEdge`, `HandleNewFormattingContext` (margins next to
  floats, `abort_if_cleared`), `HasClearancePastAdjoiningFloats` and the
  forced BFC block offset, `InlineLayoutAlgorithm::Layout`,
  `LineBreaker::HandleFloat`, `ShouldWrapLine`,
  `IsEqualToAvailableFloatInlineSize`, `ComputeMinMaxSizes` with floats
  (`layout/src/floats.rs`, ADR 0015). swb's exclusion space is its own
  model (two step functions in a sorted list of segments), not Chromium's
  shelves. `try_opportunity` in `layout/src/block.rs` is derived from
  `HandleNewFormattingContext`, and `blocks_content_sizes` in
  `layout/src/intrinsic.rs` from `ComputeMinMaxSizes`
  (`core/layout/block_layout_algorithm.cc`); see
  `THIRD_PARTY_NOTICES.md`.
- Chromium / Blink (BSD-3-Clause), counters:
  `core/css/counters_attachment_context.cc/.h` and
  `core/html/list_item_ordinal.cc/.h`, read for the counter scope,
  `list-item` counter and ordinal value rules (`style/src/counters.rs`);
  the rules were measured in Chromium 148 (the unit tests are generated
  from its output). No code copied.
- Chromium / Blink (BSD-3-Clause), positioning, from recollection and
  confirmed by measurements with Chromium 148: static positions in
  inline content (`InlineLayoutAlgorithm::PlaceOutOfFlowObjects`,
  `IsOriginalDisplayInlineType`), inline containing blocks, sticky
  offsets with shifting sticky ancestors
  (`StickyPositionScrollingConstraints`), `clip` on fixed descendants
  (`CssClipFixedPosition`) (`layout/src/positioned.rs`, ADR 0016). The
  sticky offsets in `layout/src/positioned.rs` are derived from
  `core/page/scrolling/sticky_position_scrolling_constraints.cc`; see
  `THIRD_PARTY_NOTICES.md`.
- Servo (layout 2020, MPL-2.0) — the idea of keeping a fragment at the
  static position of a hoisted absolutely positioned box; swb replaces
  it with the laid-out fragment. No code copied for it.
- Servo (MPL-2.0) — `layout/src/collapsed_margin.rs` (`CollapsedMargin`,
  `BlockMargins`) is derived from Servo's `CollapsedMargin` and
  `CollapsedBlockMargins` (`components/layout/fragment_tree/fragment.rs`).
  This file is under the MPL-2.0; see `THIRD_PARTY_NOTICES.md`.
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
  The metric-compatible family classes (`METRIC_COMPATIBLE` in
  `text/src/source/fontconfig.rs`) are derived from `GetFontEquivClass`
  in `SkFontConfigInterface_direct.cpp`, and the synthetic bold stroke
  widths (`fake_bold_scale` in `text/src/raster.rs`) from
  `SkTextFormatParams.h`; see `THIRD_PARTY_NOTICES.md`.

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
