# ADR 0007: Style system

- Status: accepted
- Date: 2026-10-02

## Context

The `style` crate turns parsed stylesheets (`css` crate) and the DOM (`dom`
crate) into one computed style per element. Layout, paint and the engine
use its output. The design must:

- follow the CSS specifications (Cascade 4, Values 4, Variables 1, Display 3,
  the property specifications) and the HTML rendering section;
- match Chromium where Chromium differs from a specification, because swb
  compares its rendering with Chromium;
- be fast enough for real pages: the Wikipedia target has about 7,600 nodes
  and 4,000 rules;
- never panic on content from the network.

## Decision

### Data flow

```
Stylist::new(quirks)             UA sheet, quirks sheet, hint rules
Stylist::add_author_sheet(...)   author sheets, in document order
compute_styles(doc, stylist, env, states, base_url) -> StyleMap
```

1. **Parsing** (`properties`, `parse`). When a sheet is added, each
   declaration is parsed into one `PropertyDeclaration` per longhand:
   a specified value (`LonghandValue`, one variant per longhand, generated
   by the `longhands!` macro in `properties/ids.rs`), a CSS-wide keyword, an
   unparsed value with `var()`/`env()` references, or a custom property.
   Shorthands expand at parse time. Logical properties map to physical
   sides for horizontal, left-to-right text (they are aliases).
   Declarations of a block are split by importance and deduplicated.
   Unknown properties are dropped (trace log).
2. **Rule storage** (`stylist`). Each selector of a rule becomes a `Rule`
   with a shared `Arc<DeclarationBlock>`, an origin (user agent,
   presentational hint, author), a sort key (specificity, source order), an
   optional `@media` condition chain and up to four ancestor hashes. Rules
   are bucketed by the key of the rightmost compound selector (ID, class,
   local name, universal). `::before`, `::after` and `::marker` rules have
   their own buckets. `@supports` is evaluated when a sheet is added (a
   declaration is supported if the property parser accepts it); `@media`
   when styles are computed.
3. **Cascade** (`cascade`). The tree is walked iteratively. For each
   element: look up candidate rules in the buckets, skip rules whose
   ancestor hashes are not in the ancestor Bloom filter, match the rest,
   sort them, and pick the winning declaration of each property in the
   order of CSS Cascade 4 (UA `!important` > author `!important` (style
   attribute first) > author normal (style attribute, rules, then
   presentational hints) > UA normal). An author-level `revert` skips the
   remaining author declarations of that property, so the UA value wins.
4. **Computing**. Start from the inherited values of the parent
   (`ComputedStyle::inherit_from`), compute custom properties, then
   `font-family` and `font-size` (other values depend on them), then all
   other longhands. Unparsed values are substituted and parsed here;
   invalid ones become `unset`. Then the fixups: blockification, `float`
   of absolutely positioned boxes, zero border widths for `none` styles,
   the `overflow` pairing rule, and `display: contents` computes to `none`
   for replaced elements and form controls (CSS Display 3, "unbox").
5. **Pseudo-elements**: `::before`/`::after` when a rule sets `content`
   (not on replaced elements), `::marker` for every `display: list-item`
   element.

### Specified and computed values

Keyword properties and colors store their computed value directly. Lengths
are specified as `SpecifiedLengthPercentage` (with `calc()` trees) and
computed with a `LengthContext`. Colors in all CSS Color 4 syntaxes are
converted to sRGB at parse time; `currentColor` stays symbolic. `url()`
values are resolved against the sheet's base URL at parse time.

### Custom properties

Custom properties are inherited through a shared `Arc<HashMap>`. An element
that declares none shares its parent's map. Declared values with `var()`
are resolved with a depth-first search; properties in a cycle become
invalid. Substitution is bounded (100,000 component values counted
recursively, 128 nested references, 64 levels of functions and blocks in a
result) so that exponential expansion cannot hang the browser and values
cannot grow deep enough to overflow the stack.

### User-agent stylesheet and presentational hints

`ua.css` is based on the CSS of the HTML Living Standard's rendering
section (CC BY 4.0, attributed in the file), with Chromium's values where
they differ (marked in the file). `ua-quirks.css` holds the quirks mode
rules. Presentational hints that the HTML specification writes as CSS are
in `hints.css` (added at hint level, specificity 0); the others (legacy
colors, dimensions, `cellpadding`, table `border`, `<font>`) are computed in
`hints.rs`.

### Chromium compatibility

- Absolute font size keywords use Chromium's sizes (`small` is 13px), not
  the CSS Fonts 4 scale factors. (Corrected 2026-10-07: the sizes are
  measured with `swbtools measure font-size-keywords`, not taken from
  Blink's tables.)
- The monospace font size quirk: `ComputedStyle::font_size_origin` records
  whether a font size came from a keyword, is relative to one, or is
  absolute. When the family changes to or from the single generic
  `monospace`, keyword-derived sizes are recomputed for a 13px default
  (Blink's `FontBuilder::CheckForGenericFamilyChange`).
- Border and outline widths snap to whole pixels (device pixel ratio 1).
- `align` attributes use the `-webkit-` text-align values.
- Headings have no special sizes inside sectioning elements (Chromium
  removed those rules).
- `text-align: match-parent` keeps the parent's `start`/`end` (CSS Text
  resolves them to `left`/`right`), so that `th` inside a list item is
  still centered by `-internal-center`.
- In quirks mode, tables inherit `color` (the specification uses the
  body's color).
- `user-select: auto` takes `none`/`all` from the parent in the computed
  value (the specification does this for the used value).
- `img`, `video`, `canvas`, `iframe`, `embed`, `object` and text inputs
  have `overflow: clip`, as in Chromium's user-agent sheet.

### Validation against Chromium

The computed values of about 80 properties for every element of the three
fixture pages were compared with Chromium's `getComputedStyle` (JavaScript
disabled, as in swb). The remaining differences are: resolved values that
are not computed values (`min-width: auto` is reported as `0px`, the
initial `serif` as `"Times New Roman"`), `@supports (mask-image: ...)`
branches (swb has no masks, so it takes the fallback branch), counters in
`content`, and `<source>` elements, which are outside Chromium's flat tree.
The comparison also found that Wikipedia's media queries use `calc()`
(`(min-width: calc(640px - 1px))`); the `css` crate now evaluates math
functions with absolute units in media features.

### Performance

Two optimizations made Wikipedia's style computation 10 times faster:

- **Ancestor Bloom filter** (`bloom`), as in Servo and Blink: a counting
  Bloom filter holds the IDs, classes and tags of the current element's
  ancestors. Most rules with descendant combinators are rejected without
  walking the tree. The `css` crate does not expose selector components, so
  the ancestor keys are read from the selector's canonical serialization. A
  test checks that the filter does not change any computed style on the
  fixture pages.
- **Class lists** are split once per document, not for each `has_class`
  call.

Elements with the same parent style, the same matched rules and no hints,
`style` attribute or `content` rule share one `Arc<ComputedStyle>`.

Measured on the Wikipedia fixture (release build, 1280x800): 8 ms for
`compute_styles`, 3 ms to parse its two stylesheets and build the stylist.

## Consequences

- Adding a longhand means one line in `ids.rs`, a parse arm in
  `longhand.rs`, a compute arm in `compute.rs`, a field in
  `ComputedStyle` with its initial value in `ComputedStyle::new_initial`,
  and an entry in `allows_quirky_length` (`longhand.rs`) if the property
  accepts unitless lengths in quirks mode.
- Values that depend on layout (percentages) stay as `LengthPercentage` in
  the computed style; layout resolves them.
- Not supported yet: `@font-face`, `@import` (the engine loads imports),
  cascade layers order, `@container`, `@keyframes`/animations/transitions,
  transforms, filters, masks, shadows, grid templates,
  `quotes`, `::first-line`/`::first-letter`/`::placeholder`, `:visited`
  styles, `white-space-collapse`/`text-wrap-mode` as separate longhands,
  `tab-size` lengths, string `list-style-type` values, relative color
  syntax, writing modes and right-to-left logical properties.

## Update (2026-10-04)

Changes up to the end of milestone M2 (tables, forms) that the sections
above do not describe:

- `::placeholder` is supported (the "Not supported yet" list above is out
  of date for it). Its rules have their own bucket, like `::before`,
  `::after` and `::marker`. An `input` or `textarea` with a `placeholder`
  attribute always gets a placeholder style (the user-agent sheet has a
  rule for it); layout uses it for the placeholder text.
- Form-control pseudo-classes (`:checked`, `:disabled`, `:enabled`,
  `:placeholder-shown`, `:valid`, `:invalid`, `:read-only`, `:required`,
  ...) come from attributes (`element.rs`), except `CONTROL_STATES`
  (`:checked`, `:placeholder-shown`, `:valid`, `:invalid`), which the page
  passes in `ElementStates::controls` from the current state of the
  controls that it knows (ADR 0013); other elements get them from
  attributes too. `DisabledElements` computes the disabled state of all
  elements in one tree walk.
- Table properties: `border-collapse`, `border-spacing` (stored as two
  internal longhands, `-swb-border-spacing-horizontal` and `-vertical`,
  which are not valid property names in stylesheets), `table-layout`,
  `caption-side` and `empty-cells`. `hints.rs` also maps `cellspacing`,
  `bordercolor`, the body margin attributes, `hr`, `iframe frameborder`
  and the `width`, `height`, `hspace`, `vspace` and `border` attributes
  of replaced elements.
- Chromium compatibility: tables drop the `-webkit-` values of
  `text-align` (Chromium's `StyleAdjuster`), so `<center>` and `align` do
  not align the content of their cells. `overflow: clip` also applies to
  `select` and to all `input` types except `range`, `checkbox` and
  `radio`, not only to text inputs.
- Ancestor Bloom filter: the css crate now gives the ancestor keys of a
  selector (`Selector::ancestor_keys`); the style crate no longer reads
  them from the selector's serialization. The keys are the same.
- Masks are supported (ADR 0018), so `@supports (mask-image: ...)`
  takes the same branch as in Chromium; the validation difference and
  the "Not supported yet" entry above are out of date for masks.

## Update (2026-10-04, M3 grid)

Grid layout ([ADR 0017](0017-grid-layout.md)) added the grid properties
(the "Not supported yet" list above is out of date for grid templates):

- Longhands `grid-template-columns`, `grid-template-rows`,
  `grid-template-areas`, `grid-auto-columns`, `grid-auto-rows`,
  `grid-auto-flow`, `grid-row-start`, `grid-row-end`,
  `grid-column-start`, `grid-column-end`, `justify-items` and
  `justify-self`; shorthands `grid-row`, `grid-column`, `grid-area`,
  `grid-template` and `grid`. `place-items` and `place-self` set the
  `justify-*` longhands too; `justify-items` and `justify-self` are no
  longer in the list of accepted but ignored properties.
- Track lists are generic over the length type (`GenericTrackList<L>`):
  the specified value holds specified lengths, the computed value
  `LengthPercentage`. `repeat()` is kept as written; layout expands it.
  A track list carries the positions of its line names
  (`LineNameTable`) and the index of its automatic repetition, built once
  when the declaration is parsed and shared by all computed values;
  `GridTemplateAreas` indexes its areas by name. Layout then finds a name
  without walking all names of a grid. The fields that this data comes
  from are private (read through accessors), so it cannot get out of
  date.
- `justify-items: legacy` computes to `normal`, `legacy center`,
  `legacy left` and `legacy right` to the position (as Chromium resolves
  them for grid items); `legacy` is not inherited.
- `subgrid` and `masonry` are invalid values, so `@supports` with them is
  false.

## Update (2026-10-07, media elements)

Media elements ([ADR 0020](0020-media-elements.md)) changed three
things:

- `aspect-ratio` keeps `auto`: the computed value is `AspectRatio {
  auto, ratio }` (it was one optional ratio, which lost `auto`).
  `AspectRatio::preferred` gives the ratio that layout uses: with
  `auto`, the natural ratio of a replaced element wins (CSS Sizing 4
  §7.1). A degenerate ratio is kept as no ratio, so the value behaves as
  `auto`.
- The `width` and `height` attributes of `<video>` also map to
  `aspect-ratio: auto w / h` when both are lengths (HTML "map to the
  aspect-ratio property (using dimension rules)"). `<img>` and image
  buttons do not get this hint yet.
- New longhand `object-position` (`<position>`, initial `50% 50%`).
  Paint uses it and `object-fit` for the images of `<img>` and the
  posters of `<video>`.

## Update (2026-10-07, responsive images)

- `ElementStates::dimension_sources` maps an `<img>` in a `<picture>`
  to the selected `<source>` that has a `width` or `height` attribute
  (HTML's "dimension attribute source"). The page computes it when it
  selects image sources (`engine/src/image_source.rs`). `hints.rs` then
  takes both dimension attributes of the image from that source (a
  missing one gives no hint), and `hspace`, `vspace` and `border` from
  the image, as Chromium does. A change needs a full restyle;
  `ElementStates::changes` does not report it.
- The css crate evaluates the `sizes` attribute (`source_size`) with
  media conditions and a length evaluator whose relative units, also in
  math functions, resolve against the `MediaEnvironment` (media queries
  in stylesheets still accept only absolute units in math functions).

## Update (2026-10-07, counters)

CSS counters (CSS Lists 3 §4,
<https://www.w3.org/TR/css-lists-3/#auto-numbering>) are supported.
For the validation difference "counters in `content`": the computed `content` of
a pseudo-element now holds the text of its counters, where Chromium's
`getComputedStyle` reports `counter(...)`; the text matches Chromium's.

- Longhands `counter-reset`, `counter-increment` and `counter-set`
  (`CounterList`: names with integers, at most 256 per value).
  `reversed()` is invalid and `counter()`/`counters()` accept only a
  counter style name (no strings, no `symbols()`), as in Chromium 148.
- **Where counters are computed.** `counters::resolve` runs at the end of
  `compute_styles`, as a second walk over the elements with the finished
  `StyleMap`. It replaces `counter()` and `counters()` in the computed
  `content` of `::marker`, `::before` and `::after` with strings, and it
  stores the ordinal value of each list item
  (`StyleMap::list_item_ordinal`), which layout shows in markers with
  `content: normal`. Style, not box construction, because:
  - the order that counters need (an element, its `::marker`, its
    `::before`, its children, its `::after`, without elements that
    generate no box) follows from the computed styles and the element
    names (lists, replaced elements and form controls); Chromium computes
    counters at the same point (layout tree attachment) from the same
    information;
  - style already resolves the rest of `content` (`attr()`), so layout
    keeps one source of generated text (`content_text`);
  - box construction is split over several builders (blocks, inline
    content, tables, flex and grid items, form controls) and runs for
    every layout; counters change only when styles change, and a change
    of a counter value changes a pseudo-element style or an ordinal, which
    `StyleMap::same_styles` detects.
  The cascade walk is pre-order only and `::after` comes after the
  children, so the counter pass is a separate iterative walk with enter
  and leave steps. It costs about 0.3 ms on the Wikipedia fixture.
- **Cost.** Each counter name has a stack of the counters in scope, and
  each tree depth a list of the counters whose scope ends with the
  element at that depth: linear in elements plus counter operations. The
  nesting of one name is bounded by the tree depth (512), and the text of
  all `counter()` and `counters()` items of a document by 4 MiB (a long
  `counters()` separator in deep nesting could otherwise produce
  gigabytes; each value counts at least one byte; after the limit,
  counters are removed without text). Each run of counters and short strings in
  a `content` value becomes one string; longer strings stay shared. The
  counter properties of an element are combined once per distinct set of
  counter lists (computed values share their lists; at most 1024 cached
  sets). The item counts of reversed lists come from one more walk over
  the document, made only when a reversed list without `start` has an
  item.
- **Chromium compatibility.** Where Chromium 148 differs from CSS Lists
  3, swb follows Chromium (measured with Chromium's DOM snapshots, CDP
  `DOMSnapshot`; Blink's `CountersAttachmentContext` and
  `ListItemOrdinal`, BSD-3, were read for ideas to explain the
  measurements). The module documentation of `counters.rs` lists all
  differences. The main ones:
  - a `counter-reset` on an element that inherits a counter of the same
    name from an ancestor's scope has a scope that ends with the element
    (the specification's heading example gives "B.3", not "B.1");
  - elements and pseudo-elements with `display: contents` do not change
    counters; `::marker` ignores the counter properties; `counter()` of
    a missing counter is 0 and creates no counter; an increment that
    would overflow is ignored;
  - the `list-item` counter comes from HTML elements, not from
    presentational hints: `ol`, `ul`, `menu` and `dir` reset it (`ol`
    from `start`), HTML `li` list items increment it, `value` does not
    change it, and the computed counter properties stay `none`. That a
    reversed `ol` without `start` resets it to 1 and that `value` is
    ignored is what Chromium shows after the first layout of a page (a
    timing artefact): after a later change that makes Chromium update
    counters (measured: inserting an element whose `::before` uses
    `counter()`, setting `counter-increment` on an element, or toggling
    `display` on `body`; inserting a plain `li` is not enough), all lists
    of the page change: a reversed list resets it to its number
    of items plus 1 (the first item shows the number of items), and
    `li value=20` gives 20 (in a nested counter; the next item gives 1).
    swb runs no scripts and keeps the first-layout values;
  - markers do not use the `list-item` counter but the ordinal value of
    the HTML specification, which `value`, `start`, `reversed` (item
    count) and `counter-set`/`counter-increment` of `list-item` on the
    item change. Layout no longer numbers list items. Elements with an
    atomic box are not list items, also with `display: list-item`:
    replaced elements, form controls (also `button`), `iframe`, `embed`,
    `svg`, `fieldset`, `progress`, `meter`, `br` and `wbr` (measured;
    `element_kinds.rs`, whose `is_replaced_element` layout uses too, and
    which also holds the cascade's list of elements without `::before`
    and `::after`). The content of `progress`, `meter` and drop-down
    `select` and the child elements of `option` do not take part in
    counters; the options of a list-box `select` do, and Chromium decides
    "list box" from `size` alone (`multiple size=1` is a drop-down);
    `wbr` ignores its counter properties (measured).
    `::before` and `::after` with `display: list-item` are list items,
    but layout does not draw their markers.
- Not supported: style containment (`contain: style`, also in `content`
  and `strict`), which in Chromium limits the scope of counters and makes
  the element a list owner.
- `start` and `value` follow the HTML rules for parsing integers
  (`start="3x"` is 3); values outside the `i32` range are ignored, as in
  Chromium. The parser (`hints::parse_integer`) now ignores leading
  zeros when it limits the number of digits.
- `details > summary` has `counter-increment: list-item 0` (HTML
  rendering section; Chromium's computed value), so it does not change
  the numbers of the list items after it.
- The `square` symbol is U+25A0 in markers and in `counter()`, as in
  Chromium (CSS Counter Styles 3 has U+25AA).
- `calc()` in an `<integer>` rounds to the nearest integer, halves
  toward positive infinity (CSS Values 4, "Range Checking",
  <https://www.w3.org/TR/css-values-4/#calc-range>; Chromium:
  `calc(-1.5)` is -1).
