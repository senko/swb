# ADR 0038: Container queries

- Status: accepted
- Date: 2026-11-14

## Context

M10 sub-feature 7a (roadmap, M10 feature 7): CSS container size queries
(CSS Containment 3: `container-type`, `container-name`, `container`,
`@container`, the container relative lengths) and `CSSContainerRule`.

**GitHub's use** (fixture `github-swb`, the same six sheets with JavaScript
on and off; no script inserts container CSS):

- 21 `@container` blocks with 41 style rules, in 4 sheets. Names:
  `controlbox`, `blankslate`, `banner`; the others have no name. Features:
  only `width`, in the range syntax (`width<=500px`, `width>=1280px`,
  `width<48rem`, `width<20rem` ... `width<87.5rem`, `width<=34rem`,
  `width<480px`, `width<=768px`, `width>=768px`). No `and`/`or`/`not`, no
  style or scroll-state queries. What the rules set: `display` (19
  declarations: `none`, `flex`, `grid`, some with `!important`), grid
  templates, padding, margins, borders, custom properties
  (`--blankslate-padding`).
- Containers: 8 declarations `container-type: inline-size` and 3
  `container: NAME/inline-size`; never `size`. One `100cqw`
  (`FeatureComparisonTable`). `@supports (container-type: inline-size)`
  guards `container: banner/inline-size`; `@supports not
  (container-type: inline-size)` holds `@media` fallbacks of the code view.
- On the page (Chromium replay, before and after hydration): two containers,
  the `UnderlineNav` wrappers (`nav`, `display: flex`, 1280 px and 860 px
  wide). Their only rule, `@container (width<48rem)` (768 px), is false for
  both, so no rule of a container applies on the first view or after
  hydration; the effect on the page is the containment of the two `nav`
  boxes and the `@supports` switch. The other containers (banner,
  blankslate, timeline, hero, code view) appear on other GitHub pages.

**Measurements** (Chromium 148, `tools/probes/container-queries.json`, 33
cases, three equal runs; rows in the notes of the feature):

1. Size features use the content box: `width`, `height`, `inline-size`,
   `block-size`, `aspect-ratio`, `orientation`, `min-`/`max-` forms, the
   range syntax with one or two comparisons, `calc()`, the boolean form
   `(width)`. Feature names are case-insensitive. `em` and `ex` in a
   condition use the container's font size, `rem` the root's, `vw` the
   viewport (`condition-units`).
2. `and`, `or`, `not` and nesting follow media queries; mixing `and` and
   `or` without parentheses, a missing condition (`@container {`), a bare
   feature (`width > 100px`), `none`/`and`/`not`/`or` as a name and two
   names drop the rule. A unitless length (`width > 100`) is false.
3. **The query container** of an element is the nearest flat tree ancestor
   (for a pseudo-element, the originating element first; for `::backdrop`
   the dialog) that (a) has the queried name in `container-name`, if the
   query has a name, and (b) supports every feature of the query:
   `container-type: inline-size` supports the inline axis only (`width`
   and `inline-size` in horizontal writing), `size` both axes. A query with
   `height` skips an `inline-size` container and finds an outer `size`
   one; `(width > 0) or (height > 0)` with only an `inline-size` container
   is false. A query with an unknown feature (`(foo: 1)`, even under `or`
   or `not`) finds no container and is false. A name alone (`@container
   card`) matches any ancestor with that name, also one of type `normal`.
   An element with `container-type: normal` and no matching name is
   skipped.
4. A selected container without a principal box that size containment
   applies to (`display: none`, `contents`, a non-atomic inline, a table, a
   table part) shadows the outer containers: its queries are unknown, so
   false (also `not (...)`). Inline-block, flex, inline-flex, list items,
   blocks work.
5. Names are not scoped by tree: a light tree rule finds a container named
   in a shadow tree and the reverse; a slotted element queries the
   containers of the flat tree (`shadow-flat-tree`).
6. An element's own styles never query itself (`own-size`): a container's
   own rules query its ancestors, so a query that would change the
   container's size does not loop. Chains resolve in Chromium in one
   rendering: three nested containers, each sized by a query of the one
   above (`nested-chain`), and units through three levels.
7. **Units** `cqw cqh cqi cqb cqmin cqmax` resolve at computed-value time
   (`font-size: 5cqw` computes to `20px`; `--x: 10cqw` stays `10cqw`).
   Each axis uses the nearest ancestor container (for a pseudo-element, the
   originating element first) whose type supports that axis and whose box
   size containment applies to; containers of `display: none`, `contents`,
   inline and table are skipped here (unlike queries, item 4). `cqh` skips
   an `inline-size` container and uses an outer `size` one; `cqmin`/`cqmax`
   combine the two axes found. Without a container: the small viewport
   units.
8. **Containment**: `container-type: inline-size` gives inline-size
   containment (a shrink-to-fit, flex, grid, float, absolute or
   `min-content` container is 0 wide plus padding, or
   `contain-intrinsic-inline-size`), `size` also block-size containment
   (`height: auto` is 0, or `contain-intrinsic-height`). Both make the box
   an independent formatting context (child margins do not collapse
   through it; it contains floats). Neither makes it the containing block
   of absolute or fixed boxes, and the computed `contain` stays `none`.
9. **Dynamic**: after a viewport resize and after a script changes the
   width of the container, of its parent or its padding, `getComputedStyle`
   and `getBoundingClientRect` give the new results at once (a forced
   style and layout).
10. A query result can change `display`, also of a nested container (the
    descendants of a container that becomes `display: none` evaluate as
    unknown).
11. **CSSOM**: `CSSContainerRule` (type 0, prototype `CSSConditionRule`,
    own members `containerName` and `containerQuery`); `conditionText` is
    `name query`; serialization lowercases feature names, puts spaces
    around operators and after colons (`(aspect-ratio: 16 / 9)`), folds
    `calc(1px + 2px)` to `calc(3px)`, keeps `<general-enclosed>` as written
    (`(foo:bar)`); `cssText` is `@container Q {\n  RULE\n}` like
    `@media`. `insertRule` and `deleteRule` work inside it.
    `getComputedStyle`: `container-type`, `container-name` (duplicates
    kept), `container` (`card / size`, `none / inline-size`, `x` for type
    `normal`). `CSS.supports`: `container-type` accepts `normal`, `size`,
    `inline-size`, `scroll-state`; `container-name` rejects `and`, `or`,
    `not`, `default` and `none` in a list; `at-rule(@container)` is true.
12. Chromium also supports `style()` queries (`style-query-recorded`).
    GitHub uses no style and no scroll-state queries.

**What swb has** (cda65d5): `swb_css` drops `@container` (it is only in
`KNOWN_AT_RULES` of the `@import` window); the units parse as viewport
units; `contain` is computed but layout uses it only for replaced elements
and the automatic minimum size of flex items. Style and layout run as two
whole-document stages: `compute_styles` makes a `StyleMap`, then
`swb_layout::layout` builds the box tree and lays it out
(`crates/engine/src/document/pipeline.rs`). There is no partial restyle or
relayout; element state changes restyle the document and keep the layout if
no style changed.

## Decision

### 1. Model in `swb_css`

A new module `container.rs`: `ContainerCondition { name: Option<String>,
query: Option<ContainerQuery> }` and `ContainerQuery`, a condition tree
like the media `Condition` (`not`, `and`, `or`, parentheses,
`<general-enclosed>`) whose leaves are size features (`width`, `height`,
`inline-size`, `block-size`, `aspect-ratio`, `orientation`, with `min-` and
`max-` and the range syntax). The parser and the range comparison of
`media.rs` are shared (the feature parser becomes a parameter); the feature
table and the evaluator are separate, because the environment differs.
`style()` and `scroll-state()` parse as `<general-enclosed>`.

The parsed query keeps what the lookup needs: the physical or logical axes
it reads, and a flag for a `<general-enclosed>` part (such a query never
finds a container, item 3). Evaluation takes a `ContainerSize` (the content
box width and height, each `Option`, the writing mode, the container's font
size for `em`/`ex`, the root font size, the viewport) and returns the
Kleene value; unknown is false at the top.

`CssRule::Container(ContainerRule { condition, rules })`; the parser keeps
`@container` blocks at the top level and inside `@media`, `@supports`,
`@layer`, `@container` and CSS nesting, with the same nesting depth limit.
The serializer writes `conditionText` as measured (item 11).

### 2. Properties and units in `swb_style`

`container-type` (`normal | size | inline-size`; `scroll-state` parses, as
in `CSS.supports`, and acts as `normal`), `container-name` (`none` or a
list of case-sensitive idents without `none`, `and`, `or`, `not`,
`default`), the shorthand `container`. They are reset properties of
`ComputedStyle`; the CSSOM and `getComputedStyle` serialize them as
measured.

New `LengthUnit` variants `Cqw`, `Cqh`, `Cqi`, `Cqb`, `Cqmin`, `Cqmax`.
They resolve to px at computed-value time (item 7), through a
`ContainerUnits` value in `LengthContext` (the px per 1 % of each axis,
from the lookup of item 7, with the viewport as fallback). Lengths inside
a container condition use the container's font size (item 1); container
units inside a condition use the viewport (departure: rare).

### 3. Stylist and cascade

A rule inside `@container` gets the index of its condition chain
(`Rule::container: Option<u32>`, like `Rule::media`); nested `@container`
rules join their conditions with `and`. `@layer` blocks inside
`@container` declare their layers unconditionally (a per-element condition
cannot order layers); `@font-face` and `@keyframes` inside `@container` are
ignored.

The style walk keeps a stack of the flat tree ancestors that are
containers (`container-type` not `normal` or a `container-name`), pushed
after an element's style is computed, popped with the path. A matched
selector whose rule has a container condition is kept only if the
condition is true: the lookup (item 3) walks the stack from the top; its
result is memoized per (stack entry, lookup key), where the lookup key is
(name, axes needed); real pages have few keys (GitHub: 4). The evaluation
is memoized per (container, condition). A pseudo-element sees its
originating element on top of the stack. `compute_element_styles` and
`compute_pseudo_element_style` (the chains of `getComputedStyle`) build the
same stack along their chain.

**Sizes.** The walk takes `ContainerSizes`: the content box of each
container from the last layout (`NodeId` to width and height). A container
without an entry (no layout yet, no eligible box) is unknown. The walk
writes a **container log** into the `StyleMap`: for each container that it
read, the size it used, the conditions it evaluated with their results,
and whether units read the size.

**Sharing.** If the stylist has container units, the style sharing key
also holds the nearest container (`Option<NodeId>`): elements with the
same nearest container have the same container ancestors, so the same
unit values. The query results need no key: they decide the matched rules,
which the key holds. Without units the key is unchanged.

### 4. Pipeline: style and layout passes

Style and layout stay two whole-document stages; container queries make
them a bounded fixed-point loop in `DocumentState::update_layout`:

1. Style with the container sizes of the last layout (none on the first
   rendering: every query is unknown, so false; units use the fallback).
2. Lay out.
3. If the container log is empty, stop. Else read the content box of each
   logged container from the fragment tree (one walk) and check the log:
   for each container whose size changed, evaluate its logged conditions
   again; the log holds if no result changed and no unit read a changed
   size. If it holds, stop.
4. Else keep the new sizes, compute the styles again (whole document), and
   if no style changed (`StyleMap::same_styles`) stop; else lay out again
   and go to 3.

**Bound**: at most `MAX_CONTAINER_PASSES` = 4 extra style and layout runs
per `update_layout` call. The font loop of `MAX_LAYOUT_PASSES` = 4 runs
inside each layout run, so the real bound of one call is 5 style runs and
5 * 4 = 20 layouts; the two bounds stay apart (see "Review fixes"). At the bound the last style and its layout stay
(they agree with each other; some query results may disagree with the
final sizes), a `warn!` line names the case, and the sizes are kept, so a
later update continues from them.

**Why it converges.** A container's size in a queried axis does not depend
on its content (size or inline-size containment), and its own styles do
not depend on its queries (item 6). So the sizes of containers that are
not inside another container are right after the first layout; the query
results of their descendants are right after the second style pass; by
induction, a chain of N nested containers whose queries size the next
container needs N + 1 layouts (`nested-chain`: 3 layouts, then the check
holds). Most pages need one extra pass on the first rendering only if a
result differs from "unknown", and none on later updates: GitHub's queries
are false at the real sizes too, so GitHub gets no extra pass. Loops are
possible only through the axis that containment leaves open (for example a
container whose width comes from a float or an aspect ratio whose height
depends on another container's content); the bound ends them.

Why not interleave style and layout (style a container's subtree when
layout reaches the container): layout takes a complete `StyleMap` and a box
tree built before it; restyling and rebuilding box subtrees in the middle of
layout would change the central layout files and every formatting context.
The loop reuses the existing stages and costs nothing when no result
changes. A partial restyle (recompute only the subtrees of containers whose
results changed, copy the other styles) is a later optimization, if the
measured cost of the extra pass asks for it.

### 5. Containment in layout

A helper on `ComputedStyle` gives the effective size and inline-size
containment: from `container-type` and from `contain`, only for boxes that
size containment applies to (not `display: none`, `contents`, non-atomic
inline, `table`/`inline-table`, table parts, ruby parts). Layout uses it:

- intrinsic inline sizes (min-content, max-content, flex and grid
  contributions, shrink-to-fit) are those of an empty box plus
  `contain-intrinsic-inline-size` (`intrinsic.rs`); the automatic minimum
  size of flex items is 0 for both kinds (`flex.rs`);
- with size containment, an auto block size is that of an empty box plus
  `contain-intrinsic-block-size`; the children are still laid out and
  overflow;
- `container-type` other than `normal` makes the box an independent
  formatting context (`box_tree.rs`, `establishes_bfc`).

`contain: size` and `contain: inline-size` get the same sizing effects, but
no formatting context (measured in the review fix round: margins collapse
through such boxes and floats are not contained). Today only replaced
elements and flex items used `contain: size`. Layout and paint containment of `contain` (containing
block, clipping) stay in the backlog. Style containment (counters and
quotes scoped to the container) is out of scope: GitHub has no counters in
containers.

### 6. Incremental paths

- **DOM, style sheet and element state changes** mark the styles dirty as
  today; the next `update_layout` runs the loop from the last sizes.
- **Viewport resize** recomputes the styles (as today) with the last sizes;
  the loop corrects them after layout. `resize` observers and the
  rendering update see the result, because they flush through
  `update_layout`.
- **Scripts.** Geometry queries already flush layout, so they see the loop's
  result. `getComputedStyle` of a style-only property
  (`query_element_style`) uses the full styles when they are current.
  Otherwise it computes the element's chain; if the stylist has container
  rules or units and the chain has a container, it runs a forced layout
  flush (`script_layout_flush`, counted as forced) and reads the full
  styles; an element without a full style (below `display: none`) gets its
  chain computed with the new sizes, where its `display: none` containers
  are unknown (item 10). Elements outside containers keep the cheap path.
- `innerText` and other style flushes use the last sizes (a later layout
  flush corrects them); departure if a probe shows a difference.

### 7. CSSOM

`RuleKind::Container { condition, rules }` in `cssom/sheet.rs`, with
`insertRule`/`deleteRule` inside and the stylist rebuild after a change;
the binding `CSSContainerRule : CSSConditionRule` (`containerName`,
`containerQuery`, `conditionText`, `cssRules`, `cssText`) in
`crates/web/idl/cssom.idl`. `CSS.supports` follows the property parsers;
`at-rule()` in `@supports` is a general `@supports` feature (backlog).

### 8. Performance and limits

- A page without container rules and units: the stylist flag is false, the
  walk keeps no stack (one field test per element for the container
  properties), the sharing key is unchanged, no log, no size walk. Budget:
  callgrind +0.3 % on Wikipedia, Ars Technica and BBC (they have no
  container CSS).
- A page with N containers and K conditions: the stack and memos cost
  O(nesting) per element in the worst case and O(1) amortized; the log is
  O(containers read + evaluations); the size walk is one walk of the
  fragment tree per pass; an extra pass is one more style and layout run.
- Hostile cases (`tools/swbtools/hostile_cases/container_queries.py`):
  1,000 nested containers each sized by a query of the one above (stops at
  the pass bound); 100,000 sibling containers with a query each (one extra
  pass); 100,000 nested containers with a named query whose name is at the
  root (the memo keeps it linear); a query that toggles `display` of the
  sizing element at each pass (oscillation: stops at the bound); a script
  that changes a container's width and reads `getComputedStyle` 10,000
  times (each read is bounded by the pass bound).

### 9. Tests

- `crates/engine/tests/container_queries.rs` runs the cases of
  `tools/probes/container-queries.json` and the three `container-*` cases
  of `tools/probes/github-css.json` against Chromium's values
  (`tests/data/container-queries.json`, three equal runs); a case that
  does not match is a departure with a reason or `Later` with a reason.
- Unit tests: `swb_css` (parser, serialization, evaluator, lookup keys),
  `swb_style` (rule selection under a condition, the lookup rules of item
  3, units and their fallback, sharing keeps results apart, pseudo-elements,
  the log), `swb_layout` (containment), the engine (the loop and its bound,
  the incremental paths, `getComputedStyle` after a script change).

### 10. Out of scope

Style queries (`style()`, Chromium supports them; they evaluate false),
`scroll-state` containers and queries, container units inside container
conditions (they use the viewport), style containment, `at-rule()` in
`@supports`, vertical writing modes beyond the axis mapping of the lookup.
GitHub uses none of these.

## Plan of phase 2

1. `swb_css`: `container.rs`, `CssRule::Container`, parser, serializer;
   unit tests. Checkpoint `7a-step1.patch`.
2. `swb_style`: the three properties, the units, `RuleKind::Container`,
   `CSSContainerRule`; CSSOM rows of the probe file. Checkpoint.
3. Stylist and cascade: conditions on rules, stack, lookup, memo, sizes,
   log, sharing key, chains for `getComputedStyle`; unit tests. Checkpoint.
4. Layout: containment and the independent formatting context; layout
   tests of the containment cases. Checkpoint.
5. Engine: the pass loop, the size walk, the forced flush of
   `getComputedStyle`; the probe test. Checkpoint.
6. Hostile cases, snapshot, `compare`, callgrind, GitHub `load` time, docs.

## Consequences

- Style can run up to five times per layout flush on a page whose query
  results change; pages without container CSS pay nothing.
- `contain: size` and `inline-size` start to act in layout.
- The pass loop is the place where any later layout-dependent style
  (anchor positioning fallbacks, `scroll-state`) can join.

## 7a implementation notes

What differs from the design above, the departures and the numbers.

- **Units in the cascade.** The cascade computes an element with the
  viewport's sizes for the container units and computes it again with the
  sizes of its query containers only if a value converted a container unit
  (a flag of the thread in `values/length.rs`, set where a unit becomes px;
  units can come from any value, also after `var()`). So the container log
  records "units read this container" only for the elements that use units:
  GitHub has one `100cqw`, and without this every size change of any of its
  containers would cost an extra pass.
  The flag is read only when the rules or the `style` attribute of the
  element or an ancestor have container units and the element has a query
  container.
- **Sharing key**: holds the nearest container only if some rule or an
  ancestor's `style` attribute uses container units; the query results are part of the matched rules
  already. With the container in every key, 100,000 sibling containers
  with a true query took 776 MiB (each child its own style); now 381 MiB
  (327 MiB without the query).
- **The lookup memo** is per (stack entry, condition object) rather than per
  (name, axes): the conditions are `Arc`s in the stylist, and the number of
  entries is bounded by the evaluations of the walk.
- **The log** keeps `Arc`s of the evaluated conditions with their results,
  the size used, the container's axes and font size; `ContainerLog::holds`
  evaluates again only the conditions of containers whose size changed.
- **The engine** reads the content boxes of all query containers whose box
  size containment applies to, in one walk of the fragment tree, after each
  layout whose styles have a non-empty log; a page without container rules
  and units never walks (`FlushCounters::container_passes` and
  `container_pass_limits`, shown by `--bench` and `page.info`).
- **The bound** logs a `warn!` (it changes the result).
- **`getComputedStyle`**: the cheap chain first; if the stylist has container
  rules or units and an ancestor in the chain is a query container, a forced
  layout flush and the full styles.
- **Containment**: `has_size_containment` and `has_inline_size_containment`
  on `ComputedStyle` cover `contain: size`, `inline-size`, `strict` and
  `container-type`. Only `container-type` `size` and `inline-size` make a
  box an independent formatting context (`establishes_bfc`); `contain`
  alone does not. A block that is not a formatting context root with
  inline-size containment has the intrinsic widths of an empty box
  (`block_level_outer_sizes`), and with size containment an auto height of
  `contain-intrinsic-height` (`end_block_box`), its children's margins as
  usual. Replaced elements take size containment from `contain: size` or
  `container-type: size` only.

Departures and later work (rows of `crates/engine/tests/container_queries.rs`):

- No `writing-mode` in swb: `cqi`/`cqb` are `cqw`/`cqh`, and the logical
  features of a vertical container map as horizontal
  (`units-vertical-writing-mode`).
- Container units inside a condition use the viewport (CSS Containment 3:
  the container's own query container) and serialize as viewport units.
- Math functions with relative units in a condition are `<general-enclosed>`.
- CSS nesting (`rule-nesting` row 5) and style queries
  (`style-query-recorded`) wait for later work.
- `@supports at-rule(@container)` is false in swb (Chromium: true).
- Style containment (counters, quotes) is not implemented.
- `rlh` in a condition is `<general-enclosed>`; `lh` uses the container's
  line height, and 1.2 times its font size for `normal` (the style crate has
  no font metrics).

Numbers (release builds, base cda65d5):

- Probe: all rows of `container-queries.json` (42 cases) and the three
  GitHub cases match Chromium but the departures and `Later` rows above.
- Scores: JS off unchanged on all targets (GitHub geometry 0.9221, pixels
  0.9939); JS on unchanged (GitHub 0.9115, 0.9934); `js-ars-technica`
  alternates between pixels 0.9891 / extra 1 and 0.9901 / extra 0 with
  both binaries (not this change).
- `just snapshot`: one change, GitHub at 700x900: the viewport makes the
  repository tab bar narrower than 48rem, so `@container (width<48rem)`
  hides the tab icons and the tabs fit in one row (88 px to 40 px), as in
  Chromium (`github-underline-nav`).
- Extra passes on the JS-off pages Wikipedia, Ars Technica, BBC and on
  GitHub (JS on and off): 0. One extra pass costs one style and one layout
  run: GitHub JS on 23.8 + 4.8 ms; Wikipedia with a true container rule on
  `.mw-body-content` 9.7 + 10.6 ms (the load then has 2 style and 3 layout
  runs instead of 1 and 2).
- GitHub JS on, load (best of 5, wall time with `--dump-boxes`): 0.582 s
  (base 0.578 s).
- Callgrind, JS off (`swb --headless --replay fixtures/pages/X URL`, total
  instructions): Wikipedia 870.65 M to 872.69 M (+0.23 %), Ars Technica
  2,137.48 M to 2,139.28 M (+0.08 %), BBC 7,483.57 M to 7,484.78 M
  (+0.02 %); budget +0.3 %. The cost comes from the larger `ComputedStyle` and
  rule records and the container property checks.
- Hostile cases: `container-chain-1000-nested` and
  `container-display-toggle-1000` 0.05 s, 106 MiB, the bound acts (its
  `warn!`); `container-queries-100000-siblings` 1.5 s, 588 MiB;
  `container-named-lookup-50000-deep` 3.7 s, 149 MiB (the same page without
  the query takes as long); `container-width-read-loop-10000` 0.8 s,
  70 MiB. The whole hostile set: 473 cases, 472 ok, 1 known failure
  (`nest-div-100000`, as before).

### Review fixes

The review of 7a found these; each has a probe case in
`tools/probes/container-queries.json` (42 cases now).

1. `contain: size` and `inline-size` alone made an independent formatting
   context; Chromium has none (`contain-size-bfc`, `contain-nonbfc-intrinsic`).
   Now only `container-type` does (see "Containment" above).
2. A custom property with a container unit in an ancestor's `style`
   attribute (also set by a script's `setProperty`) resolved against the
   viewport. The walk sets a flag when it meets a style attribute with
   container units; ancestors come first, so from then on the cascade reads
   the unit flag and the sharing key holds the nearest container
   (`units_possible`). The pseudo-element and `getComputedStyle` chains scan
   their ancestors' attributes for the same flag (`units-attr-var`,
   `units-attr-var-script`).
3. Query edges with a container height of 0 (`edge-values`): `(aspect-ratio)`
   is true for any non-zero width (the ratio is infinite), `calc(1px / 0)`
   is infinite and `calc(0px * infinity)` is NaN, which is 0 for a length
   (shared math code of `media.rs`, which also accepts `infinity`,
   `-infinity` and `NaN` now), and `lh` works.
4. Layout bound: stated, not shared. One `update_layout` runs at most
   5 style runs and 20 layouts. A shared bound would let the font loop
   (which repeats only when a font file was reused by another face) starve
   the container passes or the reverse, and the worst case needs a page that
   makes both loop; each loop is bounded and cheap when it does not
   repeat. Both bounds are in the comment of `MAX_CONTAINER_PASSES`.
5. Cost without container CSS: `ContainerName` is a thin `Arc<Vec<_>>` (8
   instead of 16 bytes in every computed style), and the sharing key holds
   the container as a plain `u32` (0 without units) instead of an
   `Option<NodeId>` (one hash write and compare less per element). Callgrind
   numbers are in the devlog entry.
6. Docs: `docs/testing.md` lists the real departures (4) and later rows (4).

## 7b: small CSS features

M10 sub-feature 7b adds `@property`, `field-sizing`, `color-mix()` with
`currentcolor`, `content-visibility` and the formatting contexts of
`contain: layout`. This section holds the decisions (no new ADR: they
follow the containment work above). Chromium 148 measurements:
`tools/probes/css-small-features.json` (196 cases, three equal runs: 155 of 7b and 41 of the review fixes) and
four cases of `tools/probes/github-css.json`; the Rust test
`crates/engine/tests/css_small_features.rs` runs every row against them.
The `DEPARTURES` and `LATER` lists of the test hold each row that differs,
with its reason.

### 7b.1 `@property`

- **Rule validity** (`registered.rs`, `Registration::from_rule`). The
  parser keeps an `@property` with a name of `--` and one more character
  (`CssRule::Property`, raw descriptors). The style crate drops a rule that
  has no valid `syntax` string (single or double quotes; `<LENGTH>` is
  invalid, `length` is a keyword), no `inherits: true | false`, an
  `!important` descriptor, or, unless the syntax is `*`, no `initial-value`
  that matches the syntax and is computationally independent (`1em` and
  `var()` are not; `0` and `calc(1px + 2px)` are). The CSSOM does not keep
  an invalid rule and `insertRule` throws `SyntaxError`. The last valid rule
  of a name wins; `@media`, `@supports` and `@layer` around a rule are
  allowed, and `@media not all` keeps the rule in the CSSOM without
  registering it; rules in shadow trees' sheets and in `@container` do not
  register.
- **Syntaxes**: `*`, `<length>`, `<number>`, `<integer>`, `<percentage>`,
  `<length-percentage>`, `<color>`, `<angle>`, `<time>`, `<resolution>`,
  `<custom-ident>`, `<string>`, keywords, `|`, and `+` and `#`. `<url>`,
  `<image>`, `<transform-function>` and `<transform-list>` accept the
  shapes of their values and keep them as written (lengths in transform
  arguments are computed).
- **Computed value**: the value of the custom property after `var()`
  substitution is matched against the syntax and computed (lengths in px
  with six significant digits, `calc()` of lengths evaluated, `<color>` in
  its computed form with `currentcolor` and mixes with it kept, `<angle>` in
  deg, `<time>` in s). A value that does not match is the initial value; a
  failed `var()` is the initial value too, except for `*`, where it is the
  guaranteed-invalid value (measured, `ap-universal-guaranteed-invalid`).
  `em` and `rem` use the element's own font size: the cascade computes the
  custom properties first with the parent's font size and again after the
  font size when the element declares a registered property (one extra
  computation only for those elements). Exception, a dependency cycle
  (`font_cycle.rs`, CSS Properties and Values API 1, "Dependency cycles via
  relative units"; measured, `ap-font-size-cycle-*`): a registered property
  with a font-relative unit (`em`, `ex`, `ch`, `cap`, `ic`, `lh`; on the
  root element also `rem` and `rlh`) that the winning `font-size`
  references through `var()`, directly or through other custom properties,
  has its initial value, and the `font-size` declaration is invalid at
  computed-value time (`unset`). `lh` does the same for `line-height`.
  Properties that no font property references keep the element's own font
  size.
- **Inheritance**: a registered property that is not in an element's map
  has its initial value (`CustomProperties::get`, the names list, `var()`
  substitution): the maps do not copy the registry's initial values, so an
  element that declares one custom property holds one entry, not one per
  registration (the review's page with 10,000 rules and 50,000 declaring
  elements took 31.7 GB when the maps held them all). A registered property
  with an initial value that is invalid on an element (a failed `var()` in a
  `*` syntax) is a `None` entry. A property that does not inherit is reset
  by dropping its entry from the copy of the parent's map; the cost is in
  proportion to the entries of the parent's map, and a map that holds no
  such entry (a flag) is shared when nothing is declared. Pages without
  `@property` pay one `Option` test per element.
- **CSSOM**: `CSSPropertyRule` (`name`, `syntax`, `inherits`, `initialValue`
  or null, `cssText` as `@property --p { syntax: "<length>"; inherits:
  false; initial-value: 0; }`, type 0).
- **Departures**: `CSS.registerProperty` and `CSS.unregisterProperty`
  (backlog: GitHub's scripts do not call them); `oklch()`, `lab()` and
  `color(display-p3 ...)` values of `<color>` are converted to 8-bit sRGB
  (Chromium keeps them as written); `calc()` in `<angle>` and `<time>`;
  white space and comments inside `*` values (as for all custom
  properties); the order of the custom property names in the
  `getComputedStyle` enumeration (swb sorts them);
  `CSS.supports('at-rule(@property)')`.

### 7b.2 `color-mix()`

- **Measured**: Chromium keeps the result of `color-mix()` as a color with
  float coordinates in the mix space, serialized as `color(srgb 0.5 0 0.5)`,
  `oklab(...)`, `lch(...)`, `color(xyz-d65 ...)` (`hsl` and `hwb` mixes are
  in sRGB), with six significant digits and `/ alpha` when the alpha is
  below 1. A mix with `currentcolor` stays a mix until the used value:
  `getComputedStyle` of `border-color` shows the mix resolved with the
  element's own `color`; an inherited mix (the `border-color: inherit` of a
  child with another `color`) follows the child's color; a custom property
  or a registered `<color>` keeps `currentcolor` (`color-mix(in srgb,
  currentcolor, rgba(0, 0, 0, 0))`). The `color` property takes the
  parent's exact color for `currentcolor`.
- **Representation** (`values/color_mix.rs`). `Color` gets one variant,
  `Special(ColorId)`; the id indexes a process-wide table of `Wide` colors
  (space, three `f32`, alpha) and `Mix` values (a mix with `currentcolor`:
  the space, the hue method, two items and the normalized percentages).
  `Color` stays `Copy` and 8 bytes. The table is shared by all documents of
  the process (all pages, frames and tabs) and never shrinks. It holds at
  most 2^20 entries (about 40 MB). When it is full, a `Wide` color (the mix
  of two concrete colors, or the resolved mix of the `color` property)
  becomes the 8-bit `Color::Rgba` of the same color: paint stays right, and
  only the digits that `getComputedStyle` shows differ; only a mix with
  `currentcolor` is invalid. One page can fill the table with distinct mixes
  (the review: 1.1 million mixes in 6 s of script), so a full table must not
  break other pages, and it does not. There is one table, not one per
  document, because a table per document needs a table handle at every
  parse, cascade, CSSOM and paint site, and values move between documents
  (adopted nodes, a moved `style` attribute), so the id of one table would
  be wrong in another (backlog: a color table per document). Mixing follows
  CSS Color 5:
  premultiplied alpha, hue fix-up for `lch`, `oklch`, `hsl` and `hwb`
  (`shorter`, `longer`, `increasing`, `decreasing`; a gray's hue is missing
  and takes the other color's), percentages that add up to less than 100%
  scale the alpha; a sum of 0 or a percentage outside 0 to 100 is invalid.
  A color with a mix is painted as 8-bit sRGB (`Wide::to_rgba`).
- **`color`**: `ComputedStyle::color_exact` keeps the float color when the
  `color` property came from a mix (inherited with `color`), so that
  `getComputedStyle(el).color` and a `currentcolor` mix of a child match.
- **CSSOM** (`element.style`, `cssText`): the mix is written as Chromium
  writes it: `in oklab` is left out, the percentages are normalized (none
  for 50/50, one if they add up to 100, else both), colors are in `rgb()` or
  names in lower case.
- **Percentages**: `calc()` (and `min()`, `max()`, `clamp()`) that gives a
  percentage is accepted, clamped to 0 to 100 (measured: `calc(150%)` is
  100%, `calc(-20%)` is 0%, `calc(infinity * 1%)` is 100%).
- **Departures**: a `color-mix()` inside a `color-mix()` with `currentcolor`
  is invalid (the nested mix of two concrete colors is fine); `none`
  components in `hsl()` and `lab()` are 0 (Chromium keeps them missing, so a
  mix takes the other color's value); the hue of a gray that swb writes in
  a mix result is a number, Chromium writes `none`; a color outside the
  gamut, `color(srgb 2 0 -1)`, is clamped when it is parsed (Chromium keeps
  the coordinates); `calc(NaN * 1%)` is invalid (Chromium: 0%); the
  `element.style` text of a `calc()` percentage is not normalized (Chromium
  simplifies it and writes it after the color); Lab, Lch,
  Oklab and XYZ results differ from Chromium in the fifth significant digit
  (Chromium converts through D50); color functions are 8-bit before they are
  mixed (`hsl(0 50% 50%)` is `rgb(191, 64, 64)`); `caret-color`,
  `-webkit-text-fill-color` and the other properties that swb has no
  computed value for stay empty (also the shorthands `column-rule` and
  `text-emphasis`); a gradient with a mix of `currentcolor` writes the mix
  unresolved in `getComputedStyle`. The shorthands `text-decoration`,
  `background`, `border`, `outline` write a mix color as the longhands
  do.

### 7b.3 `field-sizing`

- `field-sizing: content | fixed` (not inherited). With `content`, a text
  field of the text-like types (`text`, `search`, `url`, `tel`, `email`,
  `password`, `number`), a text area and a select are sized by their
  content (`control.rs`, `fitted_width`): the width of the value, else of
  the placeholder, with at least 1 px of content; a `number` or `search`
  field that shows a value or nothing keeps 15 px for its inner buttons (not
  with a placeholder); `size`, `cols` and `rows` do not count; a text area is
  as high as its lines (a final line feed counts; one line at least); a
  select is as wide as the selected option (not the widest, not rounded up)
  and an empty one is 22 by 4 px. `min-width`, `max-width`, `width`, padding
  and borders apply as usual (the content box). The other input types that
  swb shows as text fields (`range`, `color`, dates) and the buttons keep
  their sizes (`Control::content_fit` is `None`).
- **Departures**: list boxes (`select` with `multiple` or `size` above 1)
  are not supported by swb at all; swb's select is 1 px taller than
  Chromium's at `line-height: normal` (the probe sets the height).

### 7b.4 `content-visibility`

- `content-visibility: visible | auto | hidden` is a property (not
  inherited). The closed `details` flag (`contents_hidden`) stays; the
  property merges with it (`ComputedStyle::skips_contents`, the one
  question that all sites ask), and `getComputedStyle` shows `hidden` for it.
  `hidden` has no effect on a non-atomic inline box and on `display:
  contents` (measured, `cv-inline-hidden`, `cv-contents-hidden`: the text
  is painted, in `innerText`, focusable; containment does not apply there),
  so `skips_contents` is `contents_hidden || (hidden && layout containment
  applies)`. `<span hidden=until-found>` is such an inline box. An
  inline-block with `hidden` skips its contents. `auto` computes
  `contain-intrinsic-width` and `-height` as `auto none` when they are not
  declared (measured).
- **Containment** (`ComputedStyle::has_layout_containment`,
  `has_paint_containment`, `has_size_containment`): `hidden` and `auto` have
  layout, paint and style containment; `hidden` also size containment (the
  size of the box is `contain-intrinsic-size`, or 0). The skipped contents
  are laid out as usual (a script's `getBoundingClientRect` of a descendant
  gives the box that it would have), but they are out of the fragment tree
  for paint, hit testing (the box itself is hit), the selection and
  `innerText` (the skipped element gives no line breaks), the focus (not
  focusable, not in the tab order) and the top layer (an element in them is
  not rendered). A containment box is the containing block of absolute and
  fixed boxes and the offset parent of its descendants.
- **`auto` is not skipped**: in Chromium's JS-off GitHub reference no
  element has `content-visibility: auto` (the rules match elements that the
  scripts make), so skipping changes no target's score. Measured in
  Chromium: an off-screen `auto` box is sized by its `contain-intrinsic-size`
  (0 without), its descendants report their normal boxes, the scroll height
  of the page uses the intrinsic sizes, and the relevance is computed after
  the first frame (the first measurement of a JS-off page shows even
  on-screen `auto` boxes as skipped: two states, 18 or 100 px for the box
  `a` of `content-visibility-and-text-wrap`). Skipping, the remembered size
  of `contain-intrinsic-size: auto`, `contentvisibilityautostatechange`,
  `checkVisibility()` and find in page are backlog. Images under skipped
  contents: measured, an eager `<img>` loads in Chromium and swb, a lazy one
  loads in neither (nothing to do).
- `[hidden=until-found]` has `content-visibility: hidden` in the
  user-agent sheet instead of `display: none` (find in page, which reveals
  it, is backlog).

### 7b.5 `contain: layout` and `paint`

Measured (`ct-*`): `contain: layout`, `paint`, `content` and `strict` make an
independent formatting context (the box contains floats, margins do not
collapse through it, it avoids floats beside it), a stacking context (the
`z-index` of descendants stays inside), the containing block of absolute and
fixed boxes, and a box without a baseline (an inline-block sits on its
bottom margin edge); paint containment also clips to the padding box
(`clips_x`, `clips_y`, and the scrollable overflow does not propagate).
Layout containment does not apply to non-atomic inline boxes and internal
table boxes. `contain: style` and `size` alone have none of these.

### 7b.6 Review fixes

The review of 7b found nine items; the fixes (the cases are in the probe
file, `cv-inline-*`, `cv-contents-hidden`, `cv-until-found-inline`,
`cm-shorthand-*`, `ap-font-size-*`, `ap-line-height-em`, `cm-form-*`):

- The color table: a full table gives the 8-bit color for `Wide` colors
  (7b.2); a unit test with a small limit covers both cases.
- `@property` amplification: the maps do not copy initial values (7b.1);
  hostile case `registered-property-10000-rules-50000-declarers`.
- `content-visibility: hidden` on inline boxes and `display: contents`:
  one question (`skips_contents`) at every site (7b.4).
- `getComputedStyle` of `text-decoration` and `background` with a mix color
  uses the longhands' serialization.
- Font-relative units in registered properties that `font-size` uses are a
  cycle (7b.1).
- `calc()` percentages in `color-mix()`, and the departure list (7b.2).
- A layout test that fails without the containment terms of
  `forms_stacking_context`.
- `StyleMap::has_hidden_contents` is a flag that the cascade sets when it
  stores a style, not a scan of all styles per layout.
- The cascade visits only the longhands that have a winner (a bit set in
  `Winners`, in the order of `LonghandId::ALL`), not all 370 for each
  element. The fixes above cost instructions (the maps of custom properties
  in the cascade of Ars Technica); this change more than pays for them.

### 7b.7 Results

- Probe rows: 196 cases; the test has 16 departure rows and 45 later rows
  (named in the test with reasons); the other rows pass.
- JS off and on: no change in any fixture score (GitHub 0.9221 / 0.9939 and
  0.9115 / 0.9934); `just snapshot` JS off: no file changes.
- GitHub JS on, load: 0.60 s (base 0.60 s; budget 1.56 s).
- Callgrind, JS off, total instructions against the release build of 581abaa
  (the same session): 7b as first reviewed Wikipedia +0.19 %, Ars Technica
  +0.09 %, BBC +0.04 %; with the review fixes Wikipedia -1.48 %, Ars
  Technica -0.37 %, BBC -0.04 % (budget +0.3 %).
- Hostile cases (`small_features.py`): `property-rules-10000` 0.6 s,
  102 MiB; `registered-property-100000-users` 0.8 s, 292 MiB;
  `content-visibility-120000-boxes` 1.5 s, 801 MiB (the same page with
  200,000 boxes takes 1.2 GiB without the property too);
  `color-mix-chain-1000-custom-properties` and `color-mix-nested-1000-deep`
  0.05 s; `color-mix-200000-distinct-colors` 1.8 s, 934 MiB;
  `field-sizing-20000-controls` 1.2 s, 533 MiB; `contain-layout-50000-nested`
  3.1 s, 141 MiB.
