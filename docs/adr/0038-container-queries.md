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
