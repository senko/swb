# ADR 0041: Style invalidation for element states

- Status: accepted (2026-11-15)
- Date: 2026-11-15

## Context

A change of an element state (`ElementStates` in
`crates/style/src/element.rs`: hover, active, focus, focus-visible, and
focus-within, which follows from focus) restyles the whole document when
any selector depends on the changed state
(`DocumentState::update_states` -> `restyle()`; the only filter is
`Stylist::state_dependencies()`, the union of the state flags of all
selectors). Then `StyleMap::same_styles` compares all styles, and layout
runs again if one differs. A script's `focus()` sets `styles_stale`, and
the next flush restyles the whole document.

After M11 feature 4 the style run is most of a hover change on GitHub
(about 23.5 of 40.5 ms, 82 % of it selector matching) and a quarter of
one on Ars Technica (6 of 22 ms); on Wikipedia the style stage is 9.6 of
13.1 ms (`docs/performance.md`, "Cheaper style comparison and
pseudo-elements"). The M11 exit asks for a hover change with its render
of at most 20 ms on GitHub, 15 ms on Ars Technica and 8 ms on Wikipedia.

Most of a full restyle is wasted: a hover change flips the state of a
few elements (the parts of the old and new hover chains below their
common ancestor), and few selectors can see that flip. This ADR decides
how swb finds the elements whose style a state change can affect, how it
restyles only those, and when it keeps the full restyle. It is the first
part of ADR 0041; restyle after DOM changes (M11 feature 6) is a later
part.

The analysis is derived from Selectors Level 4
(<https://www.w3.org/TR/selectors-4/>): the definitions of compound and
complex selectors and of the combinators (§16), the logical combinations
`:is()`, `:where()`, `:not()` (§4) and the relational pseudo-class
`:has()` (§4.5), and from CSS Scoping 1 for `:host()`, `:host-context()`
and `::slotted()`, and CSS Shadow Parts 1 for `::part()`. No paper and no
other engine's design was used. The prototype counts in section 7 come
from a throw-away measurement on the page fixtures (`tmp/`, not
committed).

## Decision

### 1. The analysis

**Terms.** A state change flips a state flag S on a set of elements: the
*flipped elements*. For each flipped element E, the analysis gives the
elements X whose matched rules can change because S changed on E. The
flags of this ADR are `:hover`, `:active`, `:focus`, `:focus-visible` and
`:focus-within`.

The flipped elements of one change, from the old and new
`ElementStates` (the same sets that `StateSet` uses for matching):

- hover and active: the symmetric difference of the old and new chains
  (the element and its flat ancestor elements);
- focus-within: the symmetric difference of the old and new focus chains;
- focus: the old and new focused elements, and the symmetric difference
  of the old and new hosts whose shadow tree holds the focus;
- focus-visible: the old and new focused elements, when the focus or
  `focus_visible` changed.

An `iframe` never matches `:focus` but is still listed as flipped: the
sets may be larger than needed, never smaller.

**Where a pseudo-class can occur, and what it reaches.** A complex
selector is compounds C0 (the subject), C1, ..., Cn, read right to left,
with a combinator between each pair. If S occurs in Ci, the element that
matches Ci is E, and the subject X is related to E by the combinators
between Ci and C0 (Selectors 4 §16). Let k be the combinator directly to
the right of Ci, and "a later down step" any descendant or child
combinator further right:

| Position of S | Reach (X relative to E) |
|---|---|
| In C0 | **self**: E itself and its pseudo-elements |
| k is `>`, no later down step | **children**: E's children |
| k is ` ` or `>`, otherwise | **descendants**: E's descendants |
| k is `+` or `~`, no later down step | **following siblings** of E |
| k is `+` or `~`, a later down step | **descendants of following siblings** |

Reasons: a descendant or child step moves into E's subtree, and sibling
steps after it stay in that subtree (siblings share the parent, which is
E or below E). A sibling step moves to a following sibling F, and a later
down step into F's subtree. `+` is not told apart from `~` (a superset).

"Children" are E's element children in the DOM and in the flat tree,
"descendants" its flat-tree descendants. The flat descendants hold every
DOM descendant that has a style (an element outside the flat tree has
none), the shadow tree of a host (`:host(:hover) .x`) and the elements
assigned to a slot. "Siblings" are DOM siblings in E's tree.

**Nested selectors.** An occurrence inside an argument selector A of
`:is()`, `:where()`, `:not()` or `:host()` in compound Ci: the reach r1
from E to the element Y that A's subject matches (the same table, for A),
followed by the reach r2 of Ci's position from Y to X. The two compose:

| r1 then r2 | self | children | descendants | following siblings | desc. of following siblings |
|---|---|---|---|---|---|
| **self** | self | children | descendants | following siblings | desc. of f. s. |
| **children** | children | descendants | descendants | children | descendants |
| **descendants** | descendants | descendants | descendants | descendants | descendants |
| **following siblings** | following siblings | desc. of f. s. | desc. of f. s. | following siblings | desc. of f. s. |
| **desc. of f. s.** | desc. of f. s. | desc. of f. s. | desc. of f. s. | desc. of f. s. | desc. of f. s. |

`:not()` needs no special case: a flip changes the result of `:not(A)`
exactly when it changes the result of A. Nesting has the parser's depth
limit, so the composition is bounded.

**Other places:**

- `:host-context(A)`: matches the host if the host or a flat ancestor
  matches A, so E can be the host or any flat ancestor of it: reach
  **self and descendants**, composed with the position of the compound.
- `::slotted(A)`: an occurrence in A has reach self (A matches the slotted
  element X). The compounds before `::slotted()` match the slot and its
  tree: their reach is composed with **children** (X is a flat child of
  the slot).
- `::part(name)`: the pseudo-classes after it match the part element
  (self). The compounds before it match the host of the part's shadow
  tree: composed with **descendants**.
- Pseudo-elements (`::before`, `::after`, `::marker`, `::placeholder`,
  `::backdrop`): the originating element is the subject; restyling an
  element always restyles its pseudo-elements.
- `:nth-child(An+B of S)` and `:nth-last-child(... of S)`: a flip on E
  changes the index of E's siblings in both directions. Reach
  **everything** (section 4). No fixture has such a selector.

**`:has()`** (Selectors 4 §4.5). In `C:has(R)`, the anchor A matches C,
and R is a relative selector `k0 S1 k1 ... km Sm`. Only one shape is
analysed: S occurs in Sm, the subject compound of R, so the element that
Sm matches is E. Then the anchors that can change are:

| R | Anchors A |
|---|---|
| k0 is ` ` or `>` | the proper ancestors of E |
| k0 is `+` or `~`, k1 ... km all `+` or `~` | the preceding siblings of E |
| k0 is `+` or `~`, otherwise | the preceding siblings of E's inclusive ancestors |

Reasons: with a leading down step, A's subtree holds S1, and E is in
S1's subtree or in the subtree of a later sibling of S1 below A; so A is
an ancestor of E. With a leading sibling step, A is a preceding sibling of
S1, and S1 is E, a preceding sibling of E, or an inclusive ancestor of E
(or a preceding sibling of one). The anchors are then composed with the
position of C: only **self** is supported (the `:has()` compound is the
subject, also inside `:is()` at the subject). Every other shape has reach
**everything**: S in a compound of R other than Sm (`:has(.a:hover .b)`,
where E can be above or below A), and `:has()` in a non-subject compound
(`.x:has(:hover) .y`: the descendants of every ancestor of E).

**Keys.** Like the rule buckets (`Selector::bucket_key`), each occurrence
gets two keys, an ID, a class or a local name, or none:

- The **E key**: the key of the compound that holds S. For an occurrence
  at the subject of an argument of `:is()`, `:where()`, `:not()`,
  `:host()`, the argument compound's key, else the enclosing compound's
  key (E matches both). A compound with neither, but with an `:is()` or
  `:where()` whose arguments all have subject keys, gives the set of those
  keys (E has one of them). A flipped element can change X only if it has
  the E key: a flip on an element without it changes no match.
- The **subject key**: the key of C0 (of the `::slotted()` argument for
  `::slotted()` rules; none for `::part()` and `:host` subjects). Only an
  X with the subject key can match.

Both keys are necessary conditions of a match (the same reasoning as the
rule buckets), so filtering by them never drops an affected element. The
prototype shows why both matter: on GitHub, without the subject key, a
hover change marks 641 of 1,258 elements; with it, 24 (section 7).

**Where it lives.** The analysis is a function of the css crate
(`Selector::state_reach`, beside `Selector::state_dependencies`): for one
flag, the list of (reach, E key, subject key). The `Stylist` builds, per
flag, an index of these entries by E key (buckets of IDs, classes, local
names and a universal list, as `RuleMap`; quirks mode folds case), with
equal entries merged. It covers the rules of all rule sets and of the
scoped stylists of shadow trees (a shadow tree's entries apply to all
elements with the key: a superset). The index is built at the first
state change that needs it (a `OnceLock`), so a page that never changes a
state pays nothing, and its cost is linear in the selectors that hold one
of the five flags.

**Conservative choices.** The analysis may restyle too much, never too
little: `+` as `~`, children as DOM and flat children, keys as filters
only, scoped entries for all elements, and "everything" for each shape
that this ADR does not analyse.

### 2. The restyle

**Marking.** For each flipped element E and each flipped flag, the index
gives the entries whose E key E has (a lookup by E's ID, classes, local
name and the universal list). Per reach:

- self, children, following siblings: the elements of the reach that
  have one of the subject keys are marked;
- descendants and descendants of following siblings: E (or each
  following sibling) becomes a *root* with the set of subject keys;
- `:has()` anchors: the ancestors (or preceding siblings) of E that have
  a subject key of the entries are marked;
- everything: full restyle (section 4).

**The pruned walk.** One pre-order walk over the flat tree from the
document element, the same walk as `Styler::run` (and the same code for
the per-element step), which visits only:

- the ancestors of marked elements and of roots (each ancestor is marked
  "has work below" once, walking up until a node that has the mark: linear
  in the marked elements plus the depth);
- the marked elements and the subtrees of roots;
- the children of elements whose style changed (below).

An element is *restyled* (matched and cascaded again) if it is marked, if
it is in a root's subtree and has one of the active subject keys (the
union of the keys of the roots on the path, one hash lookup per ID, class
and local name), or if its parent's style changed. Other visited elements
keep their style; the walk uses it as the parent style of their children.

Because it is the same walk from the root:

- **Bloom filter**: the walk pushes and pops the visited ancestors' keys
  as now, so the filter holds exactly the flat ancestors of each restyled
  element.
- **Tree scope**: the walk computes the scope of each visited element
  with `FlatScopes`, as now.
- **Query containers**: the walk pushes each visited ancestor that is a
  query container (with its kept or new style), as now. The flag of
  container units in `style` attributes (`attribute_units`) starts true:
  the walk does not see the attributes of the elements it skips. This
  only turns on the tracking of container unit reads; results do not
  change.
- **Root font size**: the root is always visited; `rem` uses its kept or
  new style.
- **Style sharing cache**: a new cache per pass, as now. The key holds
  the parent's address; kept parents live in the old map, so addresses
  stay unique.
- **Custom properties**: the pool is seeded from the old map, as in
  feature 4, so an element whose map did not change gets the old `Arc`.
  If the measurement shows that the seeding pass matters, the pool takes
  only the old maps of the restyled elements and their parents.
- **Equal styles**: a restyled element whose new style equals the old one
  keeps the old `Arc`.

**Descendants.** The children of a restyled element are restyled when
its computed style changed (any property, not only inherited ones) or
when the display that it passes to its children as their parent box
(`layout_parent`, which is not in its own style for `display: contents`)
changed. A child that does not change stops the propagation. Reasons:
a child depends on more than the inherited values: `inherit` and `all:
inherit` on non-inherited properties, `justify-items: legacy`,
`text-align: match-parent`, `user-select: auto` and blockification read
the parent's other values. Listing these is error-prone; one level of
children of each changed element is cheap (section 7).

**`display: none`.** An element whose new style has `display: none`
keeps no styles below it: the walk does not enter it, and the styles,
pseudo-element styles and ordinals of its old subtree are removed. An
element that was `none` and is not any more has children without old
styles: they count as changed, so the whole new subtree is styled. A
marked element below a `none` element that stays `none` gets no style, as
in a full restyle.

**Pseudo-elements.** A restyled element's pseudo-elements (`::before`,
`::after`, `::marker`, `::placeholder`, `::backdrop`, the default
summary and `::details-content`) are removed and styled again by the same
code as in the full walk, so a pseudo-element can appear or go away. The
ordinal 0 of a default summary is set again with it.

**No writes before the end.** The pass collects the new element and
pseudo-element styles and the removals in a list and applies them to the
`StyleMap` only at the end, after the checks of section 4. So a late
fallback still has the old map for its comparison and its custom-property
pool.

**The two passes over the whole map.**

- `counters::resolve` replaces `counter()` in pseudo-element content with
  text and computes the ordinals of list items, from `display`, the
  counter properties, the pseudo-elements that have boxes and their
  content. The pass records whether any restyled element or
  pseudo-element changed one of these (`display` between none, contents,
  list-item and other values; `counter-reset`, `counter-increment`,
  `counter-set`; a `::before`, `::after` or `::marker` that appears or
  goes away), and whether a new pseudo-element style has `counter()` or
  `counters()` in its content. Then:
  - nothing of this: the ordinals and the resolved texts stay;
  - only new content with counters, and no change of the counter inputs:
    `resolve` runs again on the whole map. This is exact: the counter
    values at each point are those of the last pass, and content that
    was resolved before holds no `counter()` any more, so it does not
    change. The ordinals start from the default summaries' entries.
    Exception: if the last pass hit the limit of counter text (4 MiB,
    ADR 0007), full restyle;
  - a change of the counter inputs, and the map holds no resolved
    counter text: `resolve` runs again as above;
  - a change of the counter inputs, and the map holds resolved counter
    text: full restyle (the old texts of other pseudo-elements may be
    wrong, and their `counter()` items are gone).

  The `StyleMap` gets two flags for this: whether `resolve` wrote counter
  text, and whether it hit the limit. On the fixtures, Wikipedia has
  counters in content (references); its hover changes keep the counter
  inputs, so they take the second case at most (the walk of `resolve` is
  about 0.3 ms there).
- `expand_uses` (SVG `use` instances): an instance's styles come from
  the cascade of the original elements (matched in place, with the
  current states) with the `use` element's style as the parent. The pass
  runs again, from empty instance tables, when a restyled, added or
  removed element is a `use` element or an element that an instance
  copies (a set made from the instance tables, only when the map has
  instances). Otherwise the instances stay: their originals' matched
  rules and their `use` elements' styles did not change. No page fixture
  has a `use` element.

**The container log** (ADR 0038). A full restyle writes the log of the
reads of query containers. The targeted pass keeps the old log, with each
entry's size set to the current size from `ContainerSizes` (what a full
restyle at this point records; the engine replaced the sizes after the
last layout only when the log held). This is exact when no restyled
element read a container, in the old pass or in this one, and no query
container changed (section 4). For the old reads, the `StyleMap` keeps
the set of elements whose style or pseudo-element styles read a container
(a query condition or a container unit); the new reads are the log of
the targeted pass.

**Other bookkeeping.** `StyleMap::has_hidden_contents` is computed again
from all styles when a restyled style had `skips_contents()` and the new
one does not. The engine starts the image loads of the `url()` values of
the new styles only (the others started before). For a frame document,
the used color scheme of the root is checked as after a full restyle
(`update_canvas_text`).

**API.** The style crate gets one entry point, for example
`restyle_states(doc, stylist, env, base_url, sizes, old_states,
new_states, map) -> Restyle`, which runs the targeted pass or, on a
fallback, the full restyle (`compute_styles_reusing` with the old map),
and reports whether a style changed, which path ran and how many elements
it restyled. `compute_styles_reusing` stays for the first styles, the
container passes and the fallbacks.

### 3. The comparison and the layout

The pass compares while it walks: for each restyled element, the old and
new style (`Arc::ptr_eq` first, then deep equality: feature 4's pointer
checks of custom-property maps apply), the set and styles of its
pseudo-elements, and the removed and added styles under a `display`
change. If `resolve` or `expand_uses` ran again, it compares the ordinals
and the instance tables too. Elements that were not restyled are not
compared: their inputs did not change. Layout is dropped
(`invalidate_layout`) only if one of these differs; otherwise layout and
the display list stay, as after an unchanged full restyle now.

The flush counters count a targeted pass as a style run, so `--bench`
keeps its definition of a changing step; a separate counter records the
targeted runs and the restyled elements, for `docs/performance.md`.

### 4. Fallbacks

Each fallback runs today's full restyle, which gives the reference
result. Early fallbacks are decided before any work, from data that is
at hand:

1. A flag other than the five changed (`:target`, the control states,
   `:defined`, ...), alone or together with the five. Those keep the
   full restyle; they are rare and not in the scope of this feature.
2. The styles are not current: no styles, no stylist, or the stylist must
   be rebuilt (`ensure_stylist` would make a new one), image sources are
   outdated. A DOM change, a new sheet, a viewport or color scheme change
   already drop the styles (`invalidate_styles`), so the remaining case
   is the stylist.
3. A flipped element has the E key of an entry with reach "everything"
   (the shapes that section 1 does not analyse). The check is part of
   the marking, which is a few index lookups per flipped element.

Late fallbacks are found by the pass itself; the pass is dropped
(nothing was written) and the full restyle runs. They cost a targeted
pass plus a full one, so they are for rare cases:

4. The root element was restyled and its `font-size` changed (`rem` in
   every element), or, in a frame, the root's used color scheme changed.
5. A restyled element read a query container in the old pass or in this
   one, or a restyled element is or was a query container and its style
   changed (its size, type, name or fonts decide the queries of its
   subtree). On GitHub (two query containers), 10 of 120 prototype changes
   restyled an element inside a container; how many of those read one is
   measured in phase B.
6. Counters: the two cases of section 2.

`use` instances never fall back; they are computed again.

### 5. The script paths

- **Focus by script.** `script_set_focus` keeps its behaviour: it changes
  the states, sets `styles_stale` and drops the layout when a selector
  depends on the changed flags, so a loop of `focus()` calls does not
  restyle at each call. The `DocumentState` keeps the states that the
  current styles were computed with. The flush (`update_style` with
  `styles_stale`) and the next user state change both run the targeted
  restyle from those states to the current ones: the net change of all
  calls since the last styles. A full restyle in between (DOM change,
  fallback) resets them.
- **The style of one element on demand** (`query_element_style`,
  `element_styles`, `compute_element_styles`): unchanged. With current
  styles it reads the map, which equals a full restyle (the oracle,
  section 6). With `styles_stale` it computes the element and its
  ancestors as now; a state change still clears `element_styles`.
- **`getComputedStyle` after a state change**: after a user's change the
  restyle has run, and the map equals a full restyle; after a script's
  focus, the path above. The pseudo-element path
  (`compute_pseudo_element_style`) is unchanged. So scripts read the same
  values as now.

### 6. Tests

- **Analysis unit tests** (css crate), one table: selector and flag to
  the expected (reach, E key, subject key) list, for the subject; each
  combinator alone and mixed (`.a:hover > .b ~ .c`, `.a:hover ~ .b .c`);
  `:is()`, `:where()`, `:not()` nested two deep; the keys from `:is()`
  arguments; each `:has()` shape of section 1 and the "everything"
  shapes; `:nth-child(of S)`; `:host()`, `:host-context()`, `::slotted()`
  (argument and slot side), `::part()`; pseudo-elements.
- **The oracle** (style crate, all page fixtures, like
  `restyle_reuse_matches_a_fresh_restyle`): a seeded random series of
  several hundred state changes per fixture: hover over random elements,
  active on and off, focus by keyboard (`focus_visible`) and by click,
  focus-visible alone, focus moved to nothing. After each change, the
  targeted result (from the previous map) is compared with
  `compute_styles` of the new states: every element and pseudo-element
  style by deep equality, the ordinals, the `use` instance tables, the
  container log (a test helper compares it entry by entry), the hidden
  contents flag. The series also runs with container sizes (GitHub's two
  containers) and counts how many changes took each path, so that a
  fixture that only falls back does not pass silently.
- **The engine oracle**: a test-only check mode of `DocumentState` that
  runs a full restyle after each targeted one and asserts deep equality.
  A test plays the `--bench` hover grid, presses, Tab and Shift+Tab on
  all page fixtures, and on the JS fixtures github-swb, ars-technica and
  wikipedia-web-browser when `fixtures/js` exists; a small page with a
  script adds `focus()` and `blur()` loops and `getComputedStyle` reads
  between them.
- **Unit tests on small documents**, one per path: each reach; a `display`
  change to and from `none` under hover; a child with `inherit` of a
  non-inherited property; `display: contents` passing a new layout parent;
  counters in each of the four cases; a `use` element and its original
  under hover; a query container read and not read; the root's font size;
  slotted, host and part rules; the top layer; each fallback, with the
  path that ran.
- **Hostile cases** (`tools/swbtools/hostile_cases/`, with hover steps and
  the tool's time bounds; also as style-crate tests with a time bound):
  a 20,000-deep tree with `div:hover > div`, `.a:hover .b` and
  `:has(:hover)` rules, hover moving between the deepest element and the
  root (marking, the "has work below" marks and the root key sets must
  stay linear); 100,000 sibling `li` with `li:hover ~ li` and
  `li:hover + li`; 10,000 rules `.cN:has(:hover)` and 10,000 rules
  `.cN:hover .dN` (index size and lookup).
- Existing tests stay: `restyle_reuse_matches_a_fresh_restyle`,
  `element_styles_equal_the_full_styles`, `frame_oracle`, the JS-off
  snapshot (identical) and `compare --all --js` (unchanged).

### 7. Expected numbers and risks

**Prototype count** (release build, JS off, page fixtures; 120 changes
per fixture from seeded random elements: 80 hover moves, 20 active, 20
focus; "restyled" is the marked elements plus the children of the
elements whose style changed, from two full restyles; "changed" is the
elements whose style or pseudo-elements differ):

| Fixture | Styled elements | Flipped per change | Restyled, mean / max | Changed, mean | Full restyle |
|---|---|---|---|---|---|
| github-swb | 1,258 | 26.7 | 25.7 / 68 | 1.1 | 23.8 ms |
| ars-technica | 1,424 | 14.2 | 14.2 / 37 | 5.4 | 7.7 ms |
| wikipedia-web-browser | 3,829 | 21.7 | 1.8 / 5 | 1.6 | 9.2 ms |
| bbc | 2,639 | 17.6 | 2.8 / 15 | 2.2 | 4.4 ms |
| hacker-news | 812 | 8.7 | 0.9 / 3 | 0.4 | 1.4 ms |
| senko-net | 59 | 5.1 | 1.0 / 2 | 0.8 | 0.1 ms |
| craigslist-posting | 96 | 6.6 | 0.4 / 3 | 0.2 | 0.2 ms |

No change needed "everything", and in all 840 changes every element whose
style changed was in the restyled set (a check of the analysis against
full restyles). The GitHub entries (HOVER: 1,028; FOCUS: 414;
FOCUS_VISIBLE: 373) are mostly self (two thirds of the hover entries)
and descendants (30 %); three `:has()` anchor entries; no "everything".

**Expected time.** A restyled element costs about 19 µs on GitHub (23.5
ms for 1,258 elements), so the cascade of 26 elements is about 0.5 ms;
with the per-pass setup (`StateSet`, the pool seed, the marking, the
pruned walk) the style run is estimated at about 1 ms instead of 23.5 ms.
A hover change with its render (`--bench` change median):

| Fixture | Now (feature 4) | Expected | M11 exit |
|---|---|---|---|
| github-swb, JS off / on | 40.5 / 43.4 ms | about 18 / 20 ms | 20 ms |
| ars-technica, JS off / on | 22.0 / 20.0 ms | about 16 / 15 ms | 15 ms |
| wikipedia-web-browser, JS off / on | 13.1 / 13.4 ms | about 4 / 5 ms | 8 ms |

What remains is layout (when a style changed) and the render: GitHub
about 3.5 ms layout and 13 ms display list and raster; Ars Technica about
5 ms and 11 ms.

**Risks.**

- A missed dependency gives a stale style, which shows as wrong
  rendering after a hover. The analysis is conservative by construction,
  and the oracles compare with full restyles on every fixture; the
  prototype found no miss in 840 changes.
- The GitHub and Ars Technica targets are at the edge: after this
  feature, layout and the render are most of a hover change. Paint-only
  changes without layout (backlog) and damage rectangles (backlog) are
  the next steps if the exit is missed.
- Late fallbacks cost a targeted pass plus a full one. Their rate on the
  fixtures (containers on GitHub, counters on Wikipedia) is measured in
  phase B and reported per path.
- The pruned walk is a second walk beside `Styler::run`. It shares the
  per-element step (`style_element`, the pseudo-elements, the container
  stack, the filter); only the choice of the children to visit differs.
- `StateSet::new` and the pool seed are linear in the document. If they
  show in the profile, the lazy `StateSet` (as the queries use) and a
  pool of the restyled elements' maps replace them.

## Consequences

- A hover, active or focus change costs about the cascade of the
  elements it can affect plus a walk down to them, instead of a full
  restyle; the layout still runs in full when a style changed.
- The `Stylist` holds a second index (state entries by E key), built at
  the first state change; the `StyleMap` holds the set of container
  readers and two counter flags.
- The selector analysis must follow new selector features: a new
  pseudo-class with a selector argument, or a new combinator, must get a
  reach in `Selector::state_reach` (or "everything"); the oracle catches
  a missing one on the fixtures only, so the unit table is the guard.
- Full restyles stay for the other states, for DOM changes (until M11
  feature 6), for the shapes and cases of section 4, and for the first
  styles of a document.

## Implementation notes (M11 feature 5, 2026-11-15)

Accepted with three answers of the orchestrator: children are restyled
when the parent's style changed in any property (section 2); a near miss of
the GitHub and Ars Technica targets is acceptable for this feature; the
container rule of section 4 stays conservative, with its rate measured.

**Where the parts are.**

- The analysis: `crates/css/src/selector/reach.rs`
  (`Selector::state_reach`, `StateReach`, `StateDependency`), with the
  table of section 6 as unit tests.
- The index and the marking: `crates/style/src/invalidation.rs`
  (`StateIndex`, built by `Stylist::state_index` at the first use; `flipped`,
  `plan`, `KeySet`).
- The pruned walk and the end of the pass:
  `crates/style/src/cascade/targeted.rs` (`restyle_states`, `Restyle`,
  `FullReason`); it shares `style_element` and `style_pseudo_elements`
  with `Styler::run`. Unit tests: `cascade/targeted_tests.rs`.
- The style map keeps the readers of query containers (filled by
  `Styler::run` from `Containers::take_read`) and two flags of the counter
  pass (`note_counters`); `StyleMap::same_result` and `first_difference`
  serve the oracles.
- The engine: `DocumentState::restyle` runs `targeted_restyle` and falls
  back to `compute_styles_from`; `style_states` holds the states of the
  current styles (set by every full pass and every targeted one);
  `update_style` with `styles_stale` (a script's focus) calls `restyle`.
  `FlushCounters::state_restyles` counts the paths; `--bench` prints them
  (`interaction.hover.state_restyles`, `flushes.state_restyles`), `swbtools
  perf` as a third table, `swbtools gui-latency` as a line (from the debug
  lines `targeted restyle after a state change: N elements restyled` and
  `style: full restyle after a state change (REASON)`).
- The check mode (`document/restyle_check.rs`, only with debug
  assertions): `swb_engine::set_restyle_check(true)` makes every targeted
  restyle compare itself with a full restyle and panic on a difference.

**Departures from the decision text.**

- The slot side of `::slotted()` is composed with *descendants*, not
  children: with slots passed on through nested trees, the rules of an
  outer slot match elements that are flat grandchildren of it.
- The early fallback "the styles are not current" also covers a change of
  the dimension sources of images (`ElementStates::changes` does not report
  it); the dimension sources compare by pointer, then by value.
- The targeted pass uses the lazy `StateSet` (no pass over the document)
  and seeds the custom-property pool only with the old map of each element
  that it restyles. Section 7 named both as replacements if they showed:
  on GitHub the full `StateSet` cost 0.14 ms and the seed of the whole pool
  0.1 ms of a 1.2 ms pass. What is left outside the walk is `Styler::new`
  (0.12 ms on GitHub: the evaluation of the `@media` lists, the Bloom
  filter).
- A flag that no selector depends on is masked out before the "other
  states" check, so a change of `:target` on a page without `:target` rules
  does not force a full restyle of a later hover.
- `counters::resolve` notes "counter text written" and "limit reached";
  both flags stay set once set (a targeted pass that removes the only
  counter text leaves the first one set: a superset, only for the choice
  of the fallback).
- No unit test of the top layer (section 6): the review's own cases with
  dialogs, popovers and `details` found no difference from a full
  restyle.
- The active key set of the walk (the subject keys of the descendant
  roots above an element) holds at most 256 keys; a larger set becomes
  "any element". Without the limit, a chain of nested roots that each add
  a key (`.cN:hover .dN` on each level) copied a growing set at each root:
  quadratic in the depth (review of the feature: 10,000 levels took 3.4 s
  against 0.63 s for a full restyle in a debug build).
- The instance trees of `use` elements are made again (`expand_uses`) when
  a restyled or cleared element is a `use` element, when an element that
  an instance copies was restyled, or when a descendant reach of the walk
  ends at a `display: none` element (its subtree has no styles in the map,
  but an instance may copy it: a hidden sprite sheet). The first version
  looked only at the `use` elements and copies of the old map, so a `use`
  that got a style (an ancestor changed from `display: none` on hover)
  kept no instance and its icon was not drawn (found in the review).

**Results** (release build, Intel Core i5-13500; base = 04f5421 built
in the same session; `docs/performance.md`, "Restyle of the elements that
a state change affects"): a hover change with its render, change median,
JavaScript off / on: GitHub 45.9 / 49.9 ms to 18.2 / 18.8 ms, Ars Technica
25.5 / 23.3 ms to 15.4 / 12.5 ms, Wikipedia 16.0 / 16.3 ms to 2.1 / 3.1
ms. The style run of a hover change: GitHub 28.6 ms to 1.2 ms (12
elements restyled on average), Ars Technica 9.0 to 0.23 ms, Wikipedia 11.6
to 0.05 ms. What remains is layout (GitHub 3.4 ms in 58 % of the changes,
Ars Technica 6.2 ms in 81 %) and the display list and raster (about 13 and
10 ms). No fallback in the `--bench` hover grids of the page fixtures and
of the three JS fixtures; in the GitHub owner trace (JavaScript on), 2 of
229 restyles fell back for containers (0.9 %).

**Tests.** The analysis table (`reach.rs`, 7 tests); the style-crate unit
tests of each reach, `display: none` both ways, inheritance of reset
properties and layout parents, pseudo-elements, the counter cases, `use`
instances, containers, the root font size, focus states, shadow trees and
four hostile documents with time bounds (`targeted_tests.rs`, 18 tests);
the fixture oracle (`crates/style/tests/fixtures.rs`,
`targeted_restyle_matches_a_full_restyle`: 120 seeded random changes on
each of the 7 page fixtures; `targeted_restyle_with_container_sizes`:
GitHub with sizes for its query containers); the engine oracle
(`crates/engine/tests/restyle_oracle.rs`: the hover grid, presses, Tab
and Shift+Tab on the page fixtures and the three JS fixtures in check
mode, and a page whose script moves the focus and reads
`getComputedStyle`); hostile cases `hover-deep-tree-3000`,
`hover-20000-siblings-next`, `hover-5000-siblings-subsequent` and
`hover-many-has-rules` (`tools/swbtools/hostile_cases/style_invalidation.py`).

**Changed by ADR 0042** (M11 feature 6, style invalidation for DOM
changes): `Selector::state_reach` became `Selector::dependencies` (one
analysis for states and the DOM) and `StateIndex` became
`InvalidationIndex`; `:has()` in a compound that is not the subject is no
longer "everything": its entries carry an anchor key, and the anchors with
that key start the compound's position; a dependency in any compound of the
relative selector has the anchors of section 1 (not only one in its
subject); `:target` and the control states are targeted too
(`FullReason::OtherStates` remains for the image dimension sources). See ADR
0042, section 1 and its implementation notes.

**Open.** Once, in the first run of the engine oracle (the JS fixtures and
the page fixtures in parallel), the check mode reported a difference on
the GitHub JS fixture after a hover; 16 later runs (one with a 40 x 30 grid,
2,400 changes per fixture) passed. The message of that run did not say what
differed (`first_difference` came after it). Candidates: a state of the
styles that the full path would also leave behind (container passes that
stop at `MAX_CONTAINER_PASSES`), or a change of the page's scripts that does
not invalidate the styles, which the full restyle of every hover hid
before. The `use` bug above is a third candidate (GitHub's scripts create
SVG). The review ran the engine oracle 36 more times, two or three in
parallel, without a failure.
