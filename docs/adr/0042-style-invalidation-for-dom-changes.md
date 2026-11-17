# ADR 0042: Style invalidation for DOM changes

- Status: accepted (2026-11-17)
- Date: 2026-11-17

## Context

ADR 0041 restyles only the elements that a change of an element state can
affect. A DOM change still restyles the whole document:
`DocumentState::process_changes` drains the change journal (ADR 0028
section 3) and calls `invalidate_styles`, which drops the styles and the
layout; `defined_changed` and `shadow_changed` do the same; a stylesheet
change also drops the stylist. The next flush computes all styles from
nothing (`compute_styles`, without the old map) and lays out again.

**Measurement** (2026-11-17, release build of 34a1b6f with a throw-away
counting patch; headless, 1280 x 800, test fonts, page clock at the
fixture's capture time, virtual time, 1.5 s settle, then the steps of the
fixture's integration test as `swb --steps`; only the full restyles of the
main document that had old styles, that is, not the first styles):

| Run | Restyles | Time | Per restyle | Attributes only | Stylesheet changes | Changed + new styles, median / max | Connected entries, max |
|---|---|---|---|---|---|---|---|
| GitHub, interaction steps (54 steps) | 48 | 1,257 ms | 26.2 ms | 26 | 0 | 5 / 127 | 248 |
| GitHub, its load | 16 | 399 ms | 24.9 ms | 12 | 3 | 1 / 1,408 | 901 |
| GitHub, Tab steps | 35 | 707 ms | 20.2 ms | 12 | 8 | 2 / 1,408 | 10 |
| Ars Technica, steps | 64 | 602 ms | 9.4 ms | 52 | 1 | 1 / 600 | 174 |
| Ars Technica, its load | 16 | 138 ms | 8.6 ms | 2 | 0 | 0 / 1,138 | 4,261 |
| Wikipedia, steps | 87 | 857 ms | 9.9 ms | 55 | 2 | 5 / 3,513 | 493 |
| BBC, steps | 27 | 134 ms | 5.0 ms | 9 | 9 (CSSOM) | 1 / 176 | 215 |
| Craigslist, steps | 38 | 25 ms | 0.7 ms | 13 | 0 | 5 / 31 | 130 |

"Changed + new" counts the elements whose style differs from the old map or
that had none. The scripts, data and per-step tables are in
`out/m11-f6/phase-a/` of the orchestrator (not committed).

What the GitHub clicks do (the 48 restyles of the interaction steps):

- One to three full restyles per click, each 25 to 28 ms. Opening the Code
  popover: three restyles (52 inserted elements; then class changes that
  change 51 styles; then 2 inserted elements), 80.6 ms in all. A header
  menu: two restyles (`class` and `aria-expanded` on the button and the
  menu; 39 to 127 styles change), about 51 ms. Escape: one, 25 ms.
- By kind: attribute changes only 26 (671 ms); insertions or removals with
  attribute changes 18 (480 ms); top layer 2 (52 ms); 2 restyles after a
  state change that fell back at the early check of `run_targeted` (55
  ms; the adopted sheets or the image sources, not told apart). No
  stylesheet change, no `:defined` change, no shadow change.
- Attribute names (connected elements): `class` 228, `data-component` 129,
  `style` 113, `id` 66, `tabindex` 66, `name` 54, `type` 38,
  `aria-labelledby` 32, `data-size` 29, `role` 29, `aria-hidden` 28, `href`
  22, `aria-expanded` 18, SVG attributes (`d`, `width`, `height`, `fill`,
  `viewBox`). 317 of the 335 class tokens that changed are named in a
  selector.
- Inserted elements per restyle at most 52, removed at most 22; at most 248
  journal entries; text changes 4 in all.

The other fixtures confirm the pattern: on clicks and timers, attribute
changes dominate (Ars Technica: 378 `style` changes, 144 `aria-hidden`;
Wikipedia: `class` 343, `hidden` 92, `lang` 53); insertions are small (at
most 139 elements per restyle). Stylesheet changes occur on BBC clicks (9 of
27 restyles, CSSOM rule changes) and on GitHub's Tab steps (8 of 35); none on
GitHub clicks. Loads insert large parts of the document between styles (Ars
Technica: 4,261 entries and 1,774 inserted elements in one restyle, more than
the 1,477 elements that had styles).

GitHub has 27,591 selectors (8,402 with an attribute selector, 113 with
`:has()`, 40 of them with `:has()` in a compound that is not the subject,
332 with `+` or `~`, 775 with `:first-*`, `:last-*` or `:only-*`, 178 with
`:nth-*()`, 29 with `:empty`, 2
with `:defined`, none with `:nth-child(of S)`); Ars Technica 5,023 (102 with
`:has()`), Wikipedia 2,677 (408 with `:lang()`).

This ADR decides how a DOM change finds the elements whose style it can
affect, how the changes since the last styles are collected until the next
flush, when the full restyle stays, and when the layout can stay. It builds
on ADR 0041 and does not repeat it. The analysis is derived from Selectors
Level 4 (<https://www.w3.org/TR/selectors-4/>: §6 attribute selectors, §14
structural pseudo-classes, §4.5 `:has()`, §16 combinators), DOM (the tree
mutation algorithms, <https://dom.spec.whatwg.org/#mutation-algorithms>) and
HTML (the pseudo-class definitions,
<https://html.spec.whatwg.org/multipage/semantics-other.html#pseudo-classes>).
No paper and no other engine's design was used.

## Decision

### 1. The analysis: dependencies with a generalized key

ADR 0041 analyses the occurrences of one state flag. This ADR generalizes
the same walk over the compounds (the position table, the composition
table, the keys) to every simple selector whose result can change without a
change of the element states. `Selector::state_reach(flag)` becomes
`Selector::dependencies()`, which returns, in one pass, a list of
occurrences `(dependency, reach, element keys, subject key, anchor key)`.
The dependencies:

| Simple selector | Dependency | Reach before the position |
|---|---|---|
| `.c` | `Class(c)` | self |
| `#i` | `Id(i)` | self |
| `[a]`, `[a op v]` | `Attr(a)` (lowercase for HTML elements) | self |
| `:link`, `:any-link`, `:visited` | `Attr(href)` | self |
| `:lang()` | `Attr(lang)` | self and descendants (the nearest `lang` of an ancestor-or-self) |
| `:dir()` | `Attr(dir)` | self and descendants (the same for `dir`) |
| `:empty` | `Empty` | self (E is the element whose children changed) |
| `:first-child`, `:last-child`, `:only-child`, `:nth-child()`, `:nth-last-child()`, `:*-of-type`, `:nth-*-of-type()` | `Structural` | self (E is an element child of the parent whose children changed) |
| a state pseudo-class | `State(flag)`, for every flag (ADR 0041's five and the others) | self |
| every compound | `Presence` (E was inserted or removed) | self |

"Reach before the position" composes with the position of the compound as
in ADR 0041 (`then`): `:lang(en) .x` with `lang` changed on E reaches E's
descendants with the key of `.x`. `:root` and `:scope` (in a stylesheet,
the root) change only when the document element changes, which is a full
restyle (section 5).

**Keys.** For `Class(c)` and `Id(i)` the changed name is the key of the
index, so no element key is needed. For `Attr`, `State`, `Empty`,
`Structural` and `Presence` the element keys are those of ADR 0041 (the
compound's ID, class or type, or the subject keys of an `:is()` argument
list). The subject key is that of ADR 0041.

**`:has()`, revised.** ADR 0041 analysed only a dependency in the subject
compound of the relative selector. The anchors do not depend on that
position. Let `A:has(k0 S1 k1 ... Sm)` match through the elements x1 ... xm.

- If k0 is ` ` or `>`, x1 is a proper descendant of A, and each later step
  stays below A: a down step goes into the subtree of xi, and a sibling step
  keeps the parent of xi, which is A or below A. So every xi is a proper
  descendant of A, and a change on any xi (E) can change only the anchors
  among the **proper ancestors of E**.
- If k0 is `+` or `~`, x1 is a following sibling of A, and each later xi is
  a following sibling of A or a descendant of one. So the anchors are
  among the **preceding siblings of E's inclusive ancestors**; if the
  dependency is in Sm and all combinators are `+` or `~`, among the
  **preceding siblings of E**.

This holds for a dependency in any compound of the relative selector, and
at the subject of an `:is()`, `:where()` or `:not()` argument inside one
(reach self). A dependency in an argument at another position
(`:has(:is(.a .b))`, where `.a` can be above A) stays **everything**.

ADR 0041 made `:has()` in a compound that is not the subject
(`.wrap:has(.label) .input`) reach everything. The entry now also carries
the **anchor key** (the key of the compound that holds `:has()`): the
anchors among E's ancestors (or preceding siblings) that have the anchor
key become the starting points of the compound's position, so a descendant
position makes each such anchor a root with the subject keys, a child
position marks its children, a sibling position its following siblings (or
their subtrees). On GitHub this applies to 40 selectors, on Ars Technica to
16 (`html:has(link[rel="canonical"][href^=...]) #fides-overlay ...`), which
would otherwise make every inserted `link` a full restyle.

What stays **everything**: the dependencies inside `:nth-child(of S)` and
`:nth-last-child(of S)` (no fixture has them), the nested `:has()`
arguments above, and `:host-context()` arguments that hold a sibling
combinator.

**The index.** `StateIndex` becomes `InvalidationIndex`, built by the
`Stylist` at the first targeted restyle that needs it (a `OnceLock`, as
now), from the rules of all rule sets and of the scoped stylists of shadow
trees (a superset). It has maps by class name, by ID (folded in quirks
mode), by attribute name (an HTML map with lowercase names and a map for
other elements), one map per state flag bucketed by element key (ADR 0041),
and maps for `Empty`, `Structural` and `Presence` bucketed by element key.
Equal entries are merged. The `:has()` entries of `Presence` are kept apart
(section 2, insertions). Its size and build time are linear in the
selectors; on GitHub (27,591 selectors) the build must stay below 10 ms
(measured in phase B). If it does not, the dependency list of each selector
is kept with the parsed rule in the sheet cache (ADR 0028 section 9), so
that a stylist rebuild only buckets the lists again.

**Conservative choices** (more elements restyled, never fewer): attribute
values are not looked at (`[a=v]` depends on any change of `a`); `+` as
`~`; structural dependencies of all element children of a changed parent;
the scoped stylists' entries for all elements; everything for the shapes
listed above.

### 2. What each change marks

The marking produces ADR 0041's `Plan` (marked elements, roots with
subject keys, the "has work below" ancestors) and runs the same pruned walk.

**Attribute change** of attribute N on a connected element E, with the value
that it had when the styles were computed (section 3):

1. E is always marked. The cascade reads attributes of E outside selectors
   (`style`, presentational hints, SVG presentation attributes, `attr()` in
   `content`), and one element is cheap.
2. `class`: the tokens in the symmetric difference of the old and the new
   value give `Class` entries. `id`: the old and the new ID give `Id`
   entries. Every N gives the `Attr(N)` entries whose element keys E has,
   with E's keys before and after the change (for `class` and `id`).
3. The states that HTML derives from attributes (`:disabled`, `:enabled`,
   `:checked`, `:default`, `:indeterminate`, `:required`, `:optional`,
   `:read-only`, `:read-write`, `:placeholder-shown`, `:valid`,
   `:invalid`, `:in-range`, `:out-of-range`, `:open`) count as flipped on E
   for every attribute change of E: the `State` entries of these flags
   apply with E as the flipped element. This avoids a table of which
   attribute changes which state, at the cost of a few index lookups.
4. Attributes that the cascade reads on an element other than the one it
   styles, in one table of the style crate (`ATTRIBUTES_READ_BY_OTHERS`),
   each with its reach: `cellpadding` and `border` of a `table` (its cells:
   E becomes a root with the keys `td` and `th`); `link` of the `body` (the
   link colour: E becomes a root with the key `a`); `disabled` of a
   `fieldset` or `optgroup` and `contenteditable` (the states of item 3 on
   E's whole subtree: E becomes a root with the subject keys of those
   flags' entries, and their sibling and anchor reaches are taken from E);
   `part` and `exportparts` when the stylist has `::part()` rules (E and
   its flat descendants); `open` of a `details` (E and its flat
   descendants); `start`, `reversed` and `type` of an `ol` and `value` of
   an `li` (a counter input, section 4). The fixture oracle changes every
   attribute of the table.

**Children change** of a connected parent P (added and removed nodes):

1. Each added node that is connected at the flush is a root with "any key",
   and its subtree has no old styles: the whole subtree is styled.
2. Removed subtrees lose their styles (section 3, "Removed styles").
3. `Empty` entries with P as E.
4. `Structural` entries with each element child of P as E (the index of
   every child can change: `:nth-child()` after the change point,
   `:nth-last-child()` before it, `:first-child` and `:last-child` at both
   ends).
5. `Presence` entries of the added and removed elements (the subtree roots;
   for removed ones the keys recorded at the drain): a sibling position
   reaches the following siblings of an added element, and all element
   children of P for a removed one (the change point is gone); a descendant
   position after a sibling step makes them roots.
6. `:has()`: for any element D of an added or removed subtree, a chain
   through D has every element below the anchor (down-leading) or below a
   following sibling of it (sibling-leading). So if any element of the
   subtree has an element key of any compound of the relative selector (the
   `:has()` part of `Presence`), the anchors are P and its inclusive
   ancestors (down-leading), or the element children of P and the preceding
   siblings of P's inclusive ancestors (sibling-leading), filtered by the
   anchor's subject key, composed with the anchor compound's position as in
   section 1. The subtree is walked once for its keys (it is styled or
   cleared anyway).
7. A `textarea` P: the states of item 3 flip on P (its text is its value).

A move is a removal and an insertion. The journal entries of nodes outside
the document are skipped, except for the keys of removed subtrees.

**Text change** of a node with parent P: `Empty` entries with P as E; a
`textarea` P as above. The text of a `style` element is a stylesheet change
(section 5).

**Top layer change** of E (`Change::TopLayer`): E is marked (its top layer
fix-up and `::backdrop`), and `:popover-open` and `:modal` flip on E.

**State changes** since the styles: ADR 0041's flips of the five
interactive flags (from `style_states` to `states`), the flips of `:target`
(the old and the new target) and the flips of the control states (the
elements whose entry in `ElementStates::controls` differs after
`refresh_forms`) go through the same `State` maps. So a state change that
ADR 0041 sends to a full restyle (`FullReason::OtherStates`) for `:target`
or the control states is targeted too.

### 3. Collecting the changes until the flush

**The drain.** `process_changes` no longer drops the styles of a document
that has them. It moves the map aside (`DocumentState::stale_styles`, kept
until the next style computation; a second drain keeps the first map) and
records the entries in a change set (`DomChanges`):

- per element, the names of the changed attributes and the first old value
  of `class` and `id` (the value of the stale map's time);
- the parents whose children changed, the added roots, and for each
  removed root its parent and the keys of the elements in its subtree
  (current keys and the old values of this drain's attribute entries);
- the parents of changed text; the elements of top layer changes;
- the counts of entries and added elements (section 5); a reason for a
  full restyle once one occurs (then the lists are dropped).

`styles` is `None` after a drain as before, so every reader of the styles
behaves as now: the style of one element on demand (`query_element_style`,
`element_styles`) computes the element and its ancestors, the many-queries
rule (more than `max(256, n / 16)` elements) flushes, and `getComputedStyle`
reads the same values as now.

**The flush.** `update_style` with no styles: if `stale_styles` exists, the
change set has no full reason, and the stylist is current, the targeted
restyle runs on the stale map with the plan of section 2 together with the
state flips; else the full restyle runs, with the stale map as `previous`
(`compute_styles_reusing`, so the custom-property pool is reused; today
this path computes from nothing). Both clear `stale_styles` and the change
set, and set `style_states`.

**Accumulation.** The set holds all changes since the stale map was
computed, across drains, until a style computation: the forced flushes in a
click's handlers (`getBoundingClientRect` and similar) each restyle the
changes since the previous flush. A state change between drains while
`styles` is `None` (`restyle` returns early, as now) is part of the same
pass through `style_states`. `styles_stale` (a script's focus with current
styles) keeps ADR 0041's path. A full restyle (fallback, container pass,
first styles) resets everything.

**Removed styles.** A node that left the document has no style in a full
restyle, so its entries (element and pseudo-element styles, ordinals,
container reader mark, `use` instance roots) must go. At the flush, the
current subtree of every added or removed root that is not connected is
walked, each node once (a visited set, as `collect_subtree`), and its
entries are cleared. This finds every node that left: the last entry that
moved an inclusive ancestor of the node names a root whose current subtree
holds the node, and nothing moved the node after it. The walk is linear in
the nodes of those subtrees.

### 4. The bookkeeping of ADR 0041 section 2 for the new paths

- **`display: none`.** As ADR 0041: an insertion, attribute change or
  marked element below a `none` element gets no style; an element that
  stops being `none` styles its new subtree.
- **Pseudo-elements.** A restyled element's pseudo-elements are styled
  again (ADR 0041); a new element gets them with its style.
- **Counters, quotes and list ordinals.** The counter rule of ADR 0041
  applies with more inputs: an added element whose new style or
  pseudo-elements are counter inputs (`display` none, contents or
  list-item, the counter properties, a `::before`, `::after` or `::marker`,
  quotes in `content`), a removed element whose old style was one (read
  from the stale map before its entries are cleared), and the `ol` and `li`
  attributes of section 2. The four cases of ADR 0041 decide between
  keeping the ordinals, running `counters::resolve` again, and a full
  restyle.
- **`use` instances.** `expand_uses` runs again when an added, removed,
  restyled or attribute-changed element is a `use` element or in a tree that
  an instance copies (ADR 0041 and its implementation notes). GitHub's
  scripts create SVG; the engine check mode runs on its steps.
- **Query containers and the container log.** ADR 0041's late fallback
  stays: a restyled (also a new) element that reads a container, or a
  restyled container whose style changed, makes the pass fall back. The
  rate on the click steps is measured in phase B; per-element container
  reads (backlog of feature 5) are the next step if it is high.
- **The top layer.** Section 2. The walk styles top layer elements as the
  full walk does.
- **Shadow trees and slots.** Entries inside a connected shadow tree work
  the same way; the walk is the flat-tree walk of ADR 0041. A change of the
  flat tree without a journal entry (`shadow_changed`: a shadow root
  attached, the assigned nodes of a slot changed, also by an insertion into
  a host) stays a full restyle (none on the click steps).
- **The root's font size and the frame's colour scheme.** ADR 0041's late
  fallbacks.
- **Frame documents.** Each document has its own change set and stale map;
  `update_canvas_text` is checked as after a full restyle.
- **Image loads.** The loads of the `url()` values of the new and changed
  styles start (ADR 0041).
- **The stylist.** A stylesheet change (a `style` or `link` element, its
  text or attributes, an arriving sheet, CSSOM, adopted sheets that are out
  of date, the base URL, the view) drops the stylist and stays a full
  restyle, now with the stale map as `previous`. On GitHub clicks there is
  none; BBC's CSSOM changes on clicks (9 of 27 restyles, 44 ms) stay full.
- **`has_hidden_contents`** is computed again as in ADR 0041, also when an
  added or removed element had `skips_contents()`.

### 5. Fallbacks

Each runs the full restyle with the stale map as `previous`. Early (before
any work):

1. The journal was incomplete (the parse without scripts, an overflow): the
   drain records a full reason (`rediscover` runs as now).
2. The stylist is not current (section 4, "The stylist"), the image
   dimension sources changed (ADR 0041), or there is no stale map.
3. `defined_changed` (no list of elements; 0 on all click steps) or
   `shadow_changed` (0 on all click steps).
4. The document element or the `body` element was inserted or removed
   (`:root`, the body's `link` colour, the root's font size).
5. A flipped element, changed attribute, changed parent, or added or
   removed element hits an entry with reach everything.
6. Too large: more than 10,000 connected entries since the styles, or more
   added elements than half the elements of the stale map. On the click
   steps of all fixtures the maxima are 493 entries and 139 added elements;
   Ars Technica's load reaches 4,261 entries and 1,774 added elements
   against 1,477 styled elements.
7. The marking visits more elements than the stale map has styles (for
   example the structural siblings of a parent with 100,000 children): a
   full restyle costs less.

Late (found by the walk; nothing is written): ADR 0041's root font size,
containers and counters.

### 6. Layout

Layout stays a full pass (incremental layout is not in M11). The layout and
the display list are kept only when the box tree and the layout cannot have
changed:

- At the drain, the layout is dropped as now, unless every change since the
  styles is an attribute change of a connected element with a name that
  only the style system reads: `class`, `style`, `title`, `role`,
  `tabindex`, `data-*`, `aria-*`. `id` is not in the list (layout resolves
  SVG references such as `url(#id)` and `href="#id"` through it), nor any
  attribute that layout or painting reads (`src`, `type`, `value`,
  `colspan`, `rowspan`, `span`, `width`, `height`, `placeholder`,
  `selected`, `checked`, `disabled`, the SVG geometry attributes, ...).
- After the restyle, the layout is dropped if a restyled element's style
  or pseudo-element styles changed, if ordinals or `use` instances changed,
  or if the set held any other change.

Insertions, removals, text and top layer changes always lay out again. On
the click steps the layout stays in 5 of 46 GitHub restyles, 3 of 65 on Ars
Technica, 13 of 83 on Wikipedia and 9 of 40 on Craigslist (the restyles
with no style change and only listed attributes).

### 7. Tests

- **Analysis unit table** (css crate, beside ADR 0041's): each simple
  selector kind of section 1 to its dependency, reach and keys; `:lang()`
  and `:dir()` composed with positions; `:has()` with the dependency in each
  compound of the relative selector, both leading combinators, in a
  non-subject compound (anchor key), and the shapes that stay everything.
- **Fixture oracle** (style crate, all page fixtures, like ADR 0041's): a
  seeded random series of several hundred DOM changes per fixture: `class`
  tokens added, removed and replaced, `id` set and removed, other
  attributes (names from the selectors of the page, from
  `ATTRIBUTES_READ_BY_OTHERS`, form attributes, `style`, `lang`, `dir`,
  `hidden`, `href`) added, changed and removed, also on elements without
  styles (below `display: none`); subtrees inserted (new elements and
  clones), removed and moved, also as the first and last child and next to
  elements that sibling, structural or `:has()` selectors reach; text
  changed, emptied and filled; `textarea` text. Several changes between two
  flushes. After each flush, the targeted result equals `compute_styles` by
  the deep comparison of ADR 0041 section 6 (styles, pseudo-elements,
  ordinals, instances, container log, hidden contents, no entry for a
  disconnected node). It counts the paths per fixture, so that a fixture
  that only falls back does not pass silently. Also with GitHub's container
  sizes.
- **Engine check mode** (`document/restyle_check.rs`) covers the DOM path:
  after each targeted pass a full restyle must be equal. Run on the steps of
  the JS fixtures' interaction tests (GitHub's interaction steps at least;
  Ars Technica, Wikipedia, BBC and Craigslist when `fixtures/js` exists) and
  on a scripted page that mutates in loops (attributes, insertions,
  removals, moves, text) with `getComputedStyle` and
  `getBoundingClientRect` between the changes.
- **Unit tests** per path of section 2, each fallback of section 5 (with the
  path that ran), the layout rule of section 6 (kept and dropped), removed
  styles of moved and re-removed nodes, and accumulation over several
  drains with a state change in between.
- **Hostile cases** (`tools/swbtools/hostile_cases/`, with the tool's time
  bounds; also as style-crate tests with a time bound): class toggles on
  elements among 100,000 siblings with `li:nth-child(2n)`, `.a ~ li` and
  `.a + li` rules; a 20,000-deep tree with `:has()` rules and changes at the
  bottom; an insertion of 100,000 children (the size fallback); 10,000
  attribute rules `[data-xN]` and changes of those attributes; a script that
  appends one child per forced layout to a parent with structural rules.
- **GUI measurement.** `swbtools gui-latency` gets `--log FILE` (keeps the
  GUI's stderr). `out/m11-f6/clicks.json` is fixed so that each click does
  what its description says (checked with the log or a screenshot) and is
  added as `tools/traces/github-clicks.json`.
- Existing tests stay: ADR 0041's oracles, `element_styles_equal_the_full_
  styles`, `frame_oracle`, the JS-off snapshot (identical) and `compare
  --all --js` (unchanged).

### 8. Expected numbers and risks

**Estimate.** A targeted pass restyles about the changed and new elements,
their children and the marked elements whose style does not change. With
"restyled = 2 x (changed + new) + 20" elements at the page's cost per
element of a full restyle, plus 0.3 ms per pass, and the fallbacks of
section 5 (stylesheet changes stay full):

| Run | Style time now | Expected |
|---|---|---|
| GitHub, Code popover click (3 restyles) | 80.6 ms | about 5 ms |
| GitHub, a header menu click (2 restyles) | 51 ms | about 6 ms |
| GitHub, the 54 interaction steps (48 restyles) | 1,257 ms | about 70 ms |
| GitHub, the restyles of its load after the first styles | 399 ms | about 95 ms |
| Ars Technica, its steps | 602 ms | about 60 ms |
| Wikipedia, its steps | 857 ms | about 110 ms |
| BBC, its steps | 134 ms | about 50 ms |

The feature's exit (style time of the Code popover click at most 15 ms;
`gui-latency github-swb --js --trace github-owner` release to frame median
at most 150 ms, now 168 ms) follows if the estimate holds: each click loses
two to three restyles of 25 ms, and the layouts (5 to 7 ms each) remain.

**Risks.**

- A missed dependency gives a stale style. The analysis is conservative by
  construction; the table of attributes read by others is the weak point,
  so the fixture oracle changes every attribute of it, and the engine check
  mode runs on the JS fixtures' steps.
- Container fallbacks for new subtrees inside GitHub's two query containers
  could keep some clicks at a full restyle; counters can do the same on
  Wikipedia (counter text in references). Both rates are measured in phase
  B and reported per path.
- The index build over all selectors (27,591 on GitHub) after each stylist
  rebuild; the cache per parsed rule (section 1) is the remedy.
- The stale map is held from a drain to the next flush: one extra map per
  document while its styles are dirty (no copy; it is the old map).
- GitHub's Tab steps fall back in 8 of 35 restyles at the early check of
  `run_targeted` (`AdoptedSheets::out_of_date` or
  `images.sources_outdated`) after a focus change. Not examined; outside
  this feature (backlog).

## Consequences

- A DOM change costs about the cascade of the elements it can affect plus
  the marking and the walk, instead of a full restyle; most GitHub clicks
  run no full restyle.
- One selector analysis serves states and DOM changes; the `Stylist` holds
  `InvalidationIndex` instead of `StateIndex`; the engine holds a change
  set and the stale map between a drain and the next flush.
- New selector features need a dependency in `Selector::for_each_dependency`,
  and new attribute reads outside selectors an entry in
  `ATTRIBUTES_READ_BY_OTHERS` or the layout list of section 6.
- Full restyles stay for stylesheet changes, `:defined` and shadow changes,
  incomplete journals, large change sets, the unanalysed shapes and the
  late fallbacks of ADR 0041.

## Implementation notes (M11 feature 6, 2026-11-17)

Accepted with the orchestrator's answers: the layout list of section 6 as
written; the `:has()` revision in this feature (with ADR 0041's analysis
table updated and a fixture oracle on a generated page with rules of each
shape); the index build measured, with a per-rule cache only above 10 ms
on GitHub; BBC's CSSOM changes stay full restyles; the early fallback of
GitHub's Tab and typing steps is a backlog item.

**Where the parts are.**

- The analysis: `crates/css/src/selector/reach.rs`
  (`Selector::for_each_dependency`, and `Selector::dependencies` for the tests,
  `DependencyKey`, `DependencyView`), with the unit table of the
  dependencies, the `:has()` shapes and the anchor keys.
- The index and the marking: `crates/style/src/invalidation.rs`
  (`InvalidationIndex`, built by `Stylist::invalidation_index` at the first
  targeted restyle; `Planner`, `Plan`, `flipped`).
- The change set and the marking of DOM changes:
  `crates/style/src/dom_changes.rs` (`DomChanges::record`, `plan_dom`, the
  table of attributes read by other elements: `read_by_others`).
- The restyle: `crates/style/src/cascade/targeted.rs`
  (`restyle_targeted`; `restyle_states` is the same with no DOM changes;
  `collect_stale` clears the styles of nodes that left or have no box).
- The engine: `DocumentState::stale_styles`, `dom_changes` and
  `styles_full`; the drain records each entry while the document has styles
  (`changes.rs`) and calls `invalidate_styles_for_dom`;
  `compute_styles` runs `run_targeted` (`RestylePath::Dom`) or the full restyle with the
  stale map as `previous` (`pipeline.rs`). `FlushCounters::dom_restyles`
  counts the paths (`--bench`: `flushes.dom_restyles`); the pipeline log has
  `targeted restyle after DOM changes: N elements restyled` and `style: full
  restyle after DOM changes (REASON)`, which `swbtools gui-latency` counts
  (paths starting with `DOM`).

**Conditions of the acceptance.**

- (a) `ATTRIBUTES_READ_BY_OTHERS` from a search of the code (every
  `attr(`, `has_attr(`, `attribute(`, `svg_href`, `attributes()`, `body()`,
  `parent`, `ancestors` and `document_element` in `crates/style/src`, and
  the engine's image dimension sources; the list of places is in the notes
  of the feature): the `link` of the `body` (the link colour hint of `a`
  and `area`), `cellpadding` and `border` of a `table` (the hints of its
  cells), `disabled` of a `fieldset` (with its first `legend`) and of an
  `optgroup` (its `option` children), `contenteditable` (the editing state
  of the subtree), `start` and `reversed` of an `ol` and `value` of an `li`
  (the ordinals of the counter pass), `width` and `height` of a `source` in
  a `picture` (the dimension attributes of its `img`), `href` of a `use` and
  the IDs that it references (the instance trees), `slot` and `name` of a
  slot (the assignment: a shadow change), and `lang` and `dir` of an
  ancestor (`:lang()` and `:dir()`, which the analysis handles). swb reads
  no `rules` or `frame` of a table, its `:dir()` does not read text (`auto`
  counts as `ltr`), and it has no `exportparts`; `part` and the `open` of a
  `details` are read on the element itself (marked anyway).
- (b) Node IDs: `swb_dom` never frees or reuses the `NodeId` of a removed
  node (the arena keeps it detached for the life of the document,
  `crates/dom/src/tree.rs`), so the walk at the flush finds every node that
  left. `dom_tests.rs`, `removals_and_moves_leave_no_style_behind`, and the
  fixture oracles (which compare whole maps with `same_result`) check that
  no entry of a disconnected node stays.
- (c) A change of the `slot` attribute of a host's child, or of the `name`
  of a slot, changes the assigned nodes; `swb_dom` sets the shadow change
  flag (`slots.rs`, the assignment of a connected slot), and the styles are
  computed in full. Test: `crates/engine/tests/restyle_oracle.rs`,
  `slot_changes_restyle_in_full` (the colour from `::slotted()` rules
  follows both changes, and no targeted restyle after DOM changes ran).

**Departures from the decision text.**

- An element inserted or removed between the elements of a `+` step
  changes the match whatever it matches (`.a + .b` with a `span` inserted
  between): each compound left of a `+` also gives a presence dependency
  with no element key, and a `:has()` with a `+` in its relative selector an
  anchored one (found by the oracle on the generated page).
- A change of the children or the text of an element marks the element
  (its pseudo-elements can depend on its children: the default summary of
  a `details`).
- Moved state chains: if an inserted or removed root is an inclusive
  ancestor of the old hovered, active or focused element, the chains
  changed without a state change; the flips of section 2 then cover the
  removal parents with their ancestors and the current chains of those
  elements (found by the oracle: a `tr` moved into the hovered element's
  ancestor `a`).
- Removed styles: the walk of section 3 clears, in the current subtrees of
  all inserted and removed roots, every node with a style that the pass did
  not restyle; this also covers a node moved into an element without a box.
- Counter inputs: an element or a `::before`, `::after` or `::marker` that
  appears or goes away (also a moved one) is a counter input only if it is
  a list item or has counter properties (ADR 0041 counted every one); the
  `ol` and `li` attributes above are counter inputs.
- Top layer changes also mark the root element (the blocking modal dialog
  makes the rest of the document inert, `modal_inert`).
- The kept layout of section 6 is moved aside at the drain
  (`DocumentState::kept_layout`), not left in place: readers of geometry
  (`offsetHeight`, `getBoundingClientRect`, the script layout flush) then
  see no layout and flush, and the restyle puts the layout back only if no
  style changed. With the layout left in place, a script read the geometry
  of before the class change (found by the Wikipedia JS test: the heights
  of the table of contents one step behind). The kept layout is dropped by
  every layout invalidation, a full restyle, styles that cannot be
  computed (sheets not loaded) and the retirement of the document.
- Container sizes: a targeted restyle (also one of ADR 0041) falls back to
  the full restyle (`FullReason::Containers`) when the container log of
  the old styles does not hold the sizes of the last layout. This happens
  after the bound of the container passes (ADR 0038 section 4): the styles
  keep out-of-date query results, and before this feature the next DOM
  change restyled in full and went on from the new sizes. A targeted pass
  would record the new sizes in the log and keep those results (found by
  `a_deep_chain_stops_at_the_bound_and_settles_later`).
- The attribute and type maps of the index are keyed by the lowercase name
  for all elements (a superset for SVG and MathML names). The class and ID
  dependencies carry the compound's keys, which the index does not use.
- The index has no global merge of equal entries; the entries of one
  selector that differ only in their reach are made once. Build time on
  GitHub (27,793 rules, release): 9.1 to 9.7 ms, so no per-rule cache.
- `StyleMap` keeps the pseudo-element styles of an element in one array (one
  lookup per element instead of seven); the restyled elements are a bit set.
- Marking budget: 16 times the nodes of the document plus 10,000 units; past
  it, a full restyle (`FullReason::TooLarge`).
- More than 200,000 nodes walked in removed subtrees for their keys: a full
  restyle.
- `update_style` drops a kept layout when the style sheets have not
  settled (no styles can be computed then).
- Several drains before one flush (found in the review): the keys of a
  removed element include the `class` and `id` that an attribute change of
  an earlier drain recorded as old values, because the styles still have
  them (`.a ~ .b` with `.a` changed to `.c` in one task and removed in the
  next; `.p:has(.x)` alike). The section 2 text "the keys of the removed
  elements" means the keys at the time of the styles.

**Results** (release build; base = 34a1b6f, the main checkout's binary,
measured in the same session; `docs/performance.md`, "Restyle of the
elements that a DOM change affects"):

- GitHub, the click that opens the Code popover (headless): the restyles
  after its DOM changes 78 ms (full) to 8.5 to 10.6 ms (targeted; three
  clicks); with the restyles after state changes of ADR 0041 (about 9 ms,
  unchanged), the style of the click 63 to 88 ms to 16 to 20 ms. The 14
  clicks of the measurement: 709 ms of style to 161 ms; every restyle after
  DOM changes took the targeted path.
- GUI, `gui-latency github-swb --js --trace github-owner`: release to frame
  median 191 and 178 ms (two runs) to 145, 95 and 92 ms (three runs); style
  time of the trace 698 ms to 220 to 320 ms. `--trace github-clicks`:
  release to frame median 178 ms to 96 and 83 ms; style 794 ms to 158 to
  213 ms.
- The final build (the layout moved aside, the container check) runs the
  same number of style and layout runs and the same paths on each of the
  14 clicks as the build before those two fixes; the restyles after the
  Code popover's DOM changes 10.7, 8.7 and 8.6 ms; `github-owner` release
  to frame median 92 ms, `github-clicks` 90 ms.
- The load (`--bench` flushes, JavaScript on): GitHub style 457 to 153 ms
  and layout runs 17 to 11; Ars Technica 150 to 28 ms; BBC 42 to 28 ms;
  Wikipedia 48 to 44 ms. `swbtools perf` stages unchanged within noise.
- The analysis estimate of section 8 held for the click (about 5 ms
  estimated, 8.5 to 10.6 ms measured: GitHub's elements are expensive to
  cascade).

**Known cost.** A change that restyles nearly every element (an append to a
list with `:nth-child` rules, a class at the bottom of a 3,000-deep tree
with `:has()` rules) costs up to about 1.3 to 1.6 times a full restyle: the
comparison of each old and new style and the bookkeeping. The hostile cases
`dom-append-with-layout-1000` and `dom-deep-has-3000` show it (2.4 s and 3.9
s against 2.1 s and 2.9 s for the base, within their limits).

**Tests.** The analysis table (`reach.rs`, 9 tests); `dom_tests.rs` (14:
each path of section 2, removed and moved nodes, class and ID changes with
a removal in a later drain, details, the structure and size fallbacks,
four hostile documents with time bounds); the fixture oracle
`dom_restyle_matches_a_full_restyle` (150 random flushes of one to three
DOM changes, in one drain or in one drain each, with hover changes, on
each page fixture) and
`dom_restyle_matches_on_every_rule_shape` (a generated page with rules of
each shape of section 1, 12 seeds of 300 flushes); the engine check mode on
a scripted page (`script_dom_changes_equal_full_restyles`), on the page and
JS fixtures' hover sessions, and on the interaction steps of the JS
fixtures (`crates/automation/tests/restyle_oracle_dom.rs`);
`slot_changes_restyle_in_full`; the layout rule in
`layout_queries_count_as_forced`; five hostile cases
(`tools/swbtools/hostile_cases/style_invalidation.py`). Tools:
`swbtools gui-latency --log FILE`; the trace `tools/traces/github-clicks.json`
(the orchestrator's `clicks.json` clicked the Platform menu twice, but the
header menus also open on hover, so its second click left the menu open and
the next Code click hit the menu's panel; the trace now uses the Code
button, the branch selector and the "..." button, each click checked with
screenshots in the GUI).
