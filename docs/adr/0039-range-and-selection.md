# ADR 0039: Live ranges and the selection

- Status: accepted (2026-11-14)
- Date: 2026-11-14

## Context

M10 feature 9b (roadmap, M10 feature 9 and its diagnosis): swb has no
`Range`, `StaticRange`, `AbstractRange`, `Selection`,
`document.createRange`, `getSelection` or the document's `selectionchange`.
On the GitHub target every click logs "Uncaught TypeError:
window.getSelection is not a function" (GitHub's click tracking reads
`getSelection().toString()`), and the page's focus, clipboard and code-view
scripts use ranges. What GitHub's scripts call is listed in
`out/m10-f9/notes.md` ("For 9b"): `createRange` (9 files), `new Range`
(React's focus restore), `selectNodeContents`, `selectNode`, `setStart`,
`setEnd`, the four boundary getters, `commonAncestorContainer`,
`cloneContents`, `cloneRange`, `extractContents`, `deleteContents`,
`insertNode`, `collapse`, `detach`, `getClientRects` of a text range,
`toString`; `removeAllRanges`, `addRange`, `rangeCount`, `getRangeAt`,
`anchorNode`, `anchorOffset`, `focusNode`, `focusOffset`,
`selectAllChildren`, `extend`, `toString`, `type`; `selectionchange` on
`document` (3 listeners) and on a text field. The other JS fixtures use
the same set: Wikipedia's editor reads `getRangeAt(0)` and
`commonAncestorContainer`, React DOM (BBC, GitHub) reads
`anchorNode`/`focusNode` and calls `extend` and `addRange` to restore a
selection, Craigslist's jQuery UI menu saves `getRangeAt(0)` and restores
it with `removeAllRanges` and `addRange`, and BBC's embeds call `createContextualFragment` (3 files).
Nothing in the fixtures calls `Selection.modify`, `getComposedRanges`,
`compareBoundaryPoints` or `surroundContents`.

What exists (read in swb's code at 1e6b95f):

- **The native selection** (`crates/engine/src/selection.rs`, ADR 0009
  "Text selection"): `Selection { anchor, focus }` of two `TextPosition`s
  (a text node and a byte offset), kept in `DocumentState::selection`. The
  user's press, drag, double and triple click, Shift+click and Ctrl+A make
  it (`page/input.rs`); the highlight (`selection::Highlight`, built in
  `document/pipeline.rs`) and the copied text (`selected_text`, the
  `innerText` rules) read it; `DocumentState::set_selection` drops the
  display list. A DOM change that removes a node of the selection clears
  it (`document/changes.rs`, `clean_up_states`). The tree order for
  comparisons is `TreeOrder`, derived data rebuilt after a DOM change.
- **Text controls** have their own selection (`TextEdit`) and the HTML
  selection API with `select` and `selectionchange` at the control
  (`crates/web/src/html/selection.rs`, `selection_events.rs`, which also
  tracks which control "owns" the document's selection).
- **NodeIterator** (ADR 0028 "Feature 15a", `swb_dom::iterator`) keeps live
  positions as plain data in the arena of its root; the change functions
  run the pre-removing steps before a node leaves its parent
  (`Document::unlink_with`); the search for affected iterators from a
  removed subtree takes the cheaper of two walks; dead entries go through a
  shared list; the work is charged to the task's time limit through
  `after_request`; entries move with a root that moves to another arena.
- **Strings.** The DOM stores UTF-8; the API's offsets are UTF-16 code
  units, converted in the bindings (`dom/character_data.rs`; an offset
  inside a surrogate pair rounds up, ADR 0028 section 3).

### Measurements

Chromium 148 with the shared probe tool, three equal runs of each file:
`tools/probes/web-range.json` (61 cases, 92 queries) and
`tools/probes/web-selection.json` (24 cases, 68 queries; the event logs of
the two cases with two clicks are deduplicated and sorted, because
Chromium coalesces `selectionchange` differently when a task runs between
the clicks). Details and every value: `out/m10-f9b/notes.md`,
`range-r1.json`, `sel-r1.json`. The results that shape the design:

- **Objects.** `AbstractRange` (illegal constructor), `Range` (constructor
  with no argument: `(document, 0)` of the current global's document) and
  `StaticRange` (a dictionary constructor; members read in the order
  `endContainer`, `endOffset`, `startContainer`, `startOffset`; only a
  doctype or `Attr` container is refused, offsets are not checked). Chromium
  has the non-standard `Range.prototype.expand`, and
  `Selection.prototype.baseNode`, `baseOffset`, `extentNode`,
  `extentOffset`, `direction`, `getComposedRanges`, `modify`, and
  `ShadowRoot.prototype.getSelection`. `window.getSelection` is an own
  property of the window; `onselectionchange` and `onselectstart` are on
  `Document.prototype`, `HTMLElement.prototype` and the window.
- **Boundary points and errors** follow the DOM standard (§5.2 to §5.5),
  with Chromium's messages ("The offset 5 is larger than the node's length
  (4).", "There is no child at offset 99.", "The node provided is of type
  'html'.", "the given Node has no parent.", "The source range is in a
  different document than this range."). A boundary in another root
  collapses the range there; `comparePoint` checks the root, then the
  doctype, then the offset.
- **Live updates.** One tree, a range per boundary point before and after
  each of 46 changes (removals, insertions, fragments, moves in one
  parent, `textContent`, `innerHTML`, `innerText`, `outerHTML`,
  `replaceChildren`, `insertAdjacentHTML`, the `CharacterData` methods and
  setters, `splitText` at 0, inside and at the end and on a detached node,
  `normalize`, adoption into other documents, clones). The rows checked by
  hand (removals, insertions, moves in one parent, `textContent`,
  `splitText`, `normalize`, adoption) match the standard's steps; the full
  comparison comes with the Rust test of 9b-1. Removals move boundaries to
  `(parent, index)`, insertions shift offsets greater than the index,
  `splitText` moves boundaries after the offset into the new node and
  shifts `(parent, index + 1)`, `normalize` moves boundaries into the
  merged node. A range in a detached tree that goes to another document
  (also a `DOMParser` document) goes with it and stays live there.
  Offsets inside a surrogate pair are kept and `toString` returns a lone
  surrogate.
- **Contents.** Seven operations over all 253 ordered pairs of boundary
  points of a small tree (text, element, comment, nested element), with
  the result, the tree after and the range after. Errors: insertion
  into a comment, a node into itself, a doctype, a text node without a
  parent; `surroundContents` with a partially selected non-text node; an
  `extractContents` over a doctype ("The Range contains a doctype node.").
- **`createContextualFragment`** parses with the start node's element (its
  parent for a text node; `body` rules for a document or fragment); its
  scripts run when the fragment is inserted into a document, also a
  fragment of a `DOMParser` document.
- **Geometry** (CSSOM View §6.2): the border boxes of the elements whose
  parent is not selected, then the rectangles of the selected parts of
  text nodes (also inside the selected elements), in tree order; a
  collapsed range in a text node gives a rectangle of width 0, one at an
  element offset gives none; `display: none` content gives none and
  `getBoundingClientRect` gives `0,0,0,0`; a `<br>` gives a rectangle of
  width 0.
- **The selection.** One `Selection` object per document with a browsing
  context; `null` for `createHTMLDocument` and `DOMParser` documents.
  `addRange` keeps the given object (`getRangeAt(0) === r`) and ignores a
  second range, a range in another document, a range in a detached tree;
  every other method (`collapse`, `extend`, `setBaseAndExtent`,
  `selectAllChildren`, `collapseToStart`, `collapseToEnd`) makes a new
  range object; the old one stays a live range at its old place. A script
  change of the selection's range object changes the selection; a range
  that moves to another root leaves the selection (`rangeCount` 0), and
  moving it back does not return it. `direction` is `none` when
  collapsed, `forward` after `addRange`, and survives DOM changes.
  `setBaseAndExtent` with a `null` anchor empties the selection, with a
  `null` focus collapses at the anchor; nodes in another root are ignored.
  `Selection.toString` is the rendered text (`innerText` rules, no
  `user-select: none` text, no hidden text, tabs between cells);
  `Range.toString` is the concatenated data of the text nodes.
- **`selectionchange`** is a plain `Event` at the document, not bubbling,
  not cancelable, fired in a task that is queued at the first change after
  the last event (a `setTimeout` queued before the change runs first, one
  queued after runs later); later changes before it fires add nothing (3
  changes give 1 event; a change and its undo give 1). A call that changes
  nothing queues nothing. A script change of the selection's range object
  and a data change that moves a boundary queue one; a removal or
  insertion that moves a boundary queues none. `document.onselectionchange`
  runs; the window's and the body's handlers do not.
- **Text controls.** While a text control has the focus, the document's
  selection reports a range collapsed at the control's place in its parent
  (`body,3`), type `Caret` or `Range` from the control's selection,
  `direction` from the control, and `toString` gives the control's
  selected text. Events of the control go to the control (existing code).
- **The user's selection.** A click on text makes a caret (`Caret`, 1
  range) and fires `selectstart` at the text node (bubbles, cancelable),
  then `selectionchange`; a drag gives a forward or backward range in text
  nodes; a double click selects the word; Shift+click extends; Ctrl+A
  selects from the first text node at 0 to `(body, childCount)`; a click on
  the empty area below the content gives a caret at `(body, childCount)`;
  Shift+arrow keys do nothing outside editable content; a canceled
  `selectstart` leaves no selection. A gesture replaces the selection's
  range object.
- **Painting.** A range that a script adds is painted with the native
  highlight color (`rgb(51, 103, 209)`), also with element boundary points;
  a collapsed range paints nothing.
- **Shadow trees.** A script selection inside a shadow tree reports the
  shadow tree's nodes; a user's drag inside a shadow tree reports a range
  collapsed at the host's place to `document.getSelection()` and the inner
  range to `ShadowRoot.getSelection()`. `getComposedRanges()` rescopes the
  boundaries out of shadow roots that are not listed.
- **Frames.** Each document has its own selection and events. A click into
  a frame keeps the parent document's selection (swb clears it today,
  ADR 0009 "Frames").

## Decision

### 1. Live ranges in `swb_dom`

The boundary points of every live range are plain data in the arena that
holds their nodes: a new module `swb_dom::ranges` with a registry
`Option<Box<LiveRanges>>` in `Document`, `None` until the first range of the
arena. A range's start and end always share a root, so one arena holds both.

- **Entry.** `RangeEntry { start: Boundary, end: Boundary, owners: u8 }`,
  keyed by a `u32` id that the runtime hands out (as for iterators).
  `Boundary { node: NodeId, offset: u32 }`: for character data the offset
  is in **bytes** of the UTF-8 data (always a character boundary), for other
  nodes a child index. The bindings convert to and from UTF-16 code units
  at the API (startOffset, setStart, ...); an offset inside a surrogate
  pair rounds up to the end of the character, as `CharacterData` does
  (departure: Chromium keeps it). Bytes are the unit of the change
  functions (`replace_data` takes bytes) and of the native selection, so
  the steps need no conversion; a read of an offset costs a scan of the
  prefix of the node's data.
- **Index.** `by_node: HashMap<NodeId, SmallVec<[u32; 2]>>` from a
  container node to the ids with a boundary there. `owners` has a bit for
  the script object and one for the selection (section 4); an entry goes
  when both are clear.
- **Steps** (DOM §5.5 "live range pre-remove steps" and the steps in
  "insert", "replace data", "split a Text node", `normalize`), called by
  the change functions:
  - insertion of `count` nodes into `parent` at `index`: offsets of
    boundaries at `parent` greater than `index` grow by `count` (lookup of
    `parent` in the index; the index of the child is computed only when
    `parent` has boundaries);
  - removal (in `unlink_with`, next to the iterators' steps, with the same
    `steps` flag): boundaries in the removed subtree go to `(parent,
    index)`; boundaries at `parent` after `index` shrink by one. The
    affected boundaries are found as the iterators' are: with few ranges
    the walk up from each container and the walk through the removed
    subtree take turns; with many, the subtree walk with index lookups.
    That search moves into one function that both registries call (a small
    refactor of `iterator.rs`);
  - `replace_data(node, offset, count, data)`: boundaries in `node` inside
    the replaced part go to `offset`, after it shift by `data.len() -
    count`;
  - `split_text` becomes a change function of `swb_dom` (today it is
    composed in `web/src/dom/character_data.rs` from `replace_data` and
    `insert_before`, which would clamp the boundaries before they move):
    boundaries in the node after the offset move to the new node, and
    `(parent, index + 1)` grows by one; the records keep Chromium's order
    (characterData before childList);
  - `normalize`: the standard's steps for merged and removed nodes;
  - a move to another arena (`transplant`, `state::move_node_state`): the
    entries whose boundaries are in the moved subtree go with it (a root
    that moves without a removal, the measured case of a detached tree
    adopted by a `DOMParser` document).
- **Cost.** A change in an arena without ranges pays one test of the
  `Option`. With ranges, an insertion or data change pays one hash lookup;
  a removal pays the search above. Work units are counted in the shared
  cell that `after_request` charges to the time limit (as 15a).
- **Dead entries.** The data of a `Range` object puts its id on a shared
  list when the collector frees it (the collector cannot reach an arena);
  the arena drops those entries at its next range operation or change that
  consults the registry. There is no limit on live ranges (Chromium has
  none); their objects are charged to the heap.

`StaticRange` is not live: its object holds its four values and no entry.

### 2. `Range` and the contents operations

- The boundary algorithms (set start or end, compare boundary points,
  `comparePoint`, `isPointInRange`, `intersectsNode`,
  `commonAncestorContainer`, `selectNode`, `collapse`, `cloneRange`) are
  functions of `swb_dom::ranges` over node ids, so that they have unit
  tests; tree order comparisons walk the ancestors (no derived order).
- The contents operations (DOM §5.5: `deleteContents`, `extractContents`,
  `cloneContents`, `insertNode`, `surroundContents`) are functions of
  `swb_dom::ranges` built on the existing change functions, so the
  mutation records, the journal, the iterators' and ranges' steps and the
  limits come for free. They are iterative (no recursion over the depth of
  the tree): the standard's recursion over partially contained children
  becomes a loop down the two boundary paths. The bindings add Chromium's
  messages and the custom element work of clones (the upgrades that
  `cloneNode` enqueues, ADR 0035).
- `createContextualFragment` (DOM Parsing) uses the fragment parser with
  the measured context, and leaves the scripts of the fragment runnable
  (not "already started").
- `toString` concatenates the text data (bindings).
- `getClientRects` and `getBoundingClientRect` (CSSOM View §6.2) go through
  a new `EngineHost::range_rects(start, end)`: the engine flushes layout,
  takes the border boxes of the elements selected by the range whose parent
  is not (the existing `client_rects`), then the rectangles of the selected
  byte ranges of text nodes from the caret stops of their fragments
  (`selection.rs` has the walks), in tree order.
- Not implemented: `Range.prototype.expand` (non-standard).

### 3. The selection: one per document, owned by the DOM

The state of the selection lives in the arena of the document, next to the
live ranges: `selection: Option<SelectionState>` with the id of its range
entry (the entry's selection bit set) and the direction (`forward`,
`backward`, `none`). The engine's `DocumentState::selection` field and
`clean_up_states`' clearing go: the live range steps keep the selection in
the document.

- **User gestures → DOM.** The engine keeps computing `TextPosition`s
  (`position_at`, `word_at`, `paragraph_at`) and writes the result with
  `Document::set_selection(anchor, focus)`, which makes a **new** entry
  (measured: a gesture replaces the range object), sets the direction from
  the order of anchor and focus, and marks a change. Ctrl+A writes
  Chromium's boundary points (first text node at 0 to `(body,
  childCount)`). Shift+click extends from the stored anchor.
- **DOM → paint and copy.** The highlight and the copied text read the
  selection's range from the arena. Their code moves from `TextPosition`
  to boundary points: a boundary maps to a key `(rank, byte offset)` in the
  `TreeOrder` (a text container: its own rank and offset; an element
  container `(P, i)`: the rank of child `i`, or of the node after `P`'s
  subtree when `i` is the child count), so a text node is selected from
  `start.offset` if its rank is the start's, from 0 if greater, up to
  `end.offset` or its length likewise. `selected_text` and
  `Highlight::selected` use these keys; `TreeOrder` gets the end of each
  subtree. The display list records the arena's **selection generation**
  (a counter that every change of the selection or of its range bumps) and
  is rebuilt when it differs, so a script change repaints without a call
  from the bindings.
- **Scripts → DOM.** The `Selection` methods change the arena's state
  (DOM §5 and the Selection API, with Chromium's rules above: a new entry
  for every method but `addRange`; a range moved to another root leaves the
  selection; `null` arguments of `setBaseAndExtent`). The `Selection`
  object is one per document with a browsing context (a `[SameObject]`
  entry of `DocState`); it holds the wrapper of its current range strongly,
  so `getRangeAt(0)` returns the same object until the range changes.
- **`selectionchange`.** The arena keeps a "changed" flag. It is set by the
  `Selection` methods, by a change of the selection's range through a
  `Range` method, by a user gesture, and by `replace_data` when it moves a
  boundary of the selection's range; not by insertions and removals
  (measured). The bindings look at the flag after each native call
  (`after_request`) and the engine after each input event; if it is set and
  the document has no scheduled event, they queue the task that fires
  `selectionchange` at the document (the document's "has scheduled
  selectionchange event" flag of the Selection API, which gives the
  measured coalescing). A call that changes nothing does not set the flag.
- **Text controls.** While a text control owns the document's selection
  (`SelectionEvents::owner`), the getters report a range collapsed at the
  control's place in its parent, the type and direction of the control's
  selection, and `toString` gives its selected text (through
  `EngineHost`); a script's change of the document's selection moves the
  ownership back to the document (the control keeps the focus).
- **`Selection.toString`** is the engine's `selected_text` over the range
  (`EngineHost::selection_text`), the code that copy uses.
- **Frames.** Each frame document has its own arena and selection. A
  click into a frame keeps the parent document's selection, painted as
  today (no inactive color), as Chromium does (measured,
  `frames-user-click`). This replaces the rule of ADR 0009 "Frames" that a
  document that loses the focus to another document loses its selection;
  9b-2 updates that paragraph with the code.
- **`containsNode`** follows Chromium, which reports a partly selected
  text node as contained (measured, `collapse-extend` #4; the standard
  says it is not): a measured departure from the standard. 9b-2 adds probe
  cases (three equal runs) and derives the rule from them.
- **Shadow trees.** Script selections inside a shadow tree are kept as
  measured (the nodes are reported as they are). A user's selection inside
  a shadow tree is stored and reported the same way (departure: Chromium
  reports a collapsed range at the host to `document.getSelection()`). A
  selection whose anchor and focus are in different trees (a drag from the
  document into a shadow tree) collapses at the focus (departure: Chromium
  keeps a composed selection; swb's highlight did not cover that case
  before either, since `TreeOrder` does not enter shadow trees).

### 4. Objects and lifetime

- `Range` objects: host data with the range's id, the `DocKey` of the
  arena that holds the entry (updated when the entry moves), and the
  shared dead list; the realm is the one of the method's document
  (`createRange`) or the current global (`new Range`), as Chromium. The
  wrappers of the boundary nodes live as long as their document (ADR 0028
  section 4), so the range traces nothing.
- The selection's range entry has the selection bit; a range object that
  the collector frees while the selection holds the entry clears only the
  object bit, and `getRangeAt` makes a new object for the entry (not
  observable: nothing referenced the old one). (9b-2 keeps the bits in the
  selection's state instead; see "9b-2 implementation notes".)
- `Selection` objects trace their current range wrapper.

### 5. Out of scope

- `Selection.modify` (absent; departure in the API row), `Range.expand`,
  `ShadowRoot.getSelection`, `caretRangeFromPoint` and
  `caretPositionFromPoint` (GitHub's code view tests for them; backlog).
- `document.execCommand` (backlog of 9a).
- `getComposedRanges` (backlog; no fixture uses it).
- The highlight of a selection across shadow boundaries, `::selection`,
  the inactive selection color of a document without focus.

### 6. Budgets and hostile pages

- No limit on live ranges; the work of the steps is charged to the time
  limit (section 1). A loop of insertions into a parent that holds many
  boundaries is O(boundaries) per insertion and ends at the time limit.
- The contents operations are iterative and bounded by the DOM limits
  (nodes, text bytes): `extractContents` of a 100,000-node subtree and of a
  range at the bottom of a 100,000-level chain must not overflow the stack.
- Boundary comparisons are O(depth); a loop of them in a deep chain ends at
  the time limit between calls.
- `selectionchange` is one task per document at most; a loop of
  `addRange`/`removeAllRanges` creates and drops entries (each change
  frees the old entry when no object holds it).
- `toString` and `getClientRects` of a huge range allocate in proportion to
  the DOM, which the DOM limits bound; their strings and lists are charged
  to the heap.

9b-1 review fixes. A batched change runs the steps once for the whole batch,
so that one native call costs children plus boundary points, not their
product: "replace all" (`textContent`, `innerHTML`, `replaceChildren`) and
the children of a fragment that leave one after the other move every
boundary point at or below the parent to `(parent, 0)` in one pass; the
children of a fragment that is inserted shift the offsets once by their
count (as the standard's "insert" does). `normalize` collects its steps per
parent while it changes the tree, in the indices that the children had
before the call (`ranges/normalize.rs`): the removed indices and, for each
run of merged text nodes, its first index and the lengths of the data before
each merge. The index of a node comes from a cursor per parent that only moves
forward. At the end, one pass over the boundary points at each parent maps
an old offset to its place: into the run's first node (steps 6.3 and 6.4)
or to the old offset minus the removed children before it. This equals the
standard's steps node by node, for empty text nodes and runs alike. The
steps of a run are collected after its data went into the first node, and
they also run after a failing append (the text limit), so the later nodes
and their boundary points stay as they are. Not batched: loops of single
removals or insertions; each call costs the boundary points at the parent,
and the time limit ends the loop.

Hostile cases (`tools/swbtools/hostile_cases/range.py`): 100,000 live
ranges and a DOM change; 100,000 ranges in one parent and a loop of
insertions; `extractContents` over a 100,000-node subtree; a range at the
bottom of a 100,000-level chain with `extractContents`, `cloneContents`,
`commonAncestorContainer` and a removal of the top; since the fixes, 100,000 ranges
with `textContent = ''` of a parent of 100,000 children, `normalize` of
100,000 elements followed by 100,000 text nodes, `normalize` of 100,000
empty text nodes with 100,000 ranges at their parent, `normalize` of 50,000
runs with 50,000 ranges at their parent and a run in an element before each,
and a fragment of 100,000 children inserted at the front. Hostile cases of
the selection (`selection.py`, 9b-2): a loop of `addRange` and
`removeAllRanges` and a loop of `collapse`; a `selectionchange` listener
that changes the selection again; a selection over 100,000 text nodes,
painted and copied.

### 7. Performance

JS-off fixtures create no range: each DOM change pays one `Option` test
(insertion, removal, data change). The engine's highlight works as today
for a selection in text nodes. Budget: callgrind +0.3 % on Wikipedia, Ars
Technica and BBC (JS off); GitHub JS-on final DOM time within 1.514 s.

### 8. Tests

- Rust tests with Chromium's values of both probe files, with and without
  the GC stress mode (`crates/web/tests/range.rs`, `selection.rs`; the
  user-input cases through the automation steps, as `real_input.rs`).
- `swb_dom` unit tests of the steps for every kind of change, the
  contents operations and the search over removed subtrees.
- Engine tests: the highlight with element boundary points, a script range
  painted (`paint-*` cases), gestures that become ranges, geometry.
- Hostile cases above; mutation checks of the new tests.

### The plan of sub-features

Each is one session, with its own patch and review.

- **9b-1: live ranges and `Range` without contents and geometry.**
  `swb_dom::ranges` (entries, index, every step, `split_text` in `dom`,
  moves across arenas, dead list, the shared search with the iterators),
  `AbstractRange`, `StaticRange`, `Range` (constructor, boundary members and
  errors, `compareBoundaryPoints`, `comparePoint`, `isPointInRange`,
  `intersectsNode`, `commonAncestorContainer`, `cloneRange`, `detach`,
  `toString`), `Document.createRange`. Tests: the probe cases `api`,
  `static-range`, `boundary`, `live-*`, `range-frames` (the contents and
  geometry queries as departures until 9b-3), dom unit tests, the hostile
  cases of live ranges. Exit: those rows equal Chromium's; `just check`;
  no performance change on the JS-off fixtures.
- **9b-2: `Selection`, the native tie and `selectionchange`.**
  `Selection` and `getSelection` (document and window), the arena's
  selection state, the engine's selection read from and written to the
  arena (highlight and copy with boundary points, Ctrl+A, the display list
  rebuild by generation), `selectionchange` and its handler, the text
  control reporting, `Selection.toString` through `EngineHost`, the frame
  rule above. Tests: all
  of `web-selection.json` except what section 5 leaves out; engine tests of
  the tie; hostile cases of the selection. Exit: GitHub's click steps log
  no `getSelection` error; snapshots unchanged with JavaScript off.
- **9b-3: contents operations, `createContextualFragment`, geometry.**
  The five operations (iterative), `createContextualFragment`,
  `getClientRects` and `getBoundingClientRect` through `EngineHost`. Tests:
  `contents-*`, `contextual-fragment`, `geometry`; hostile cases of the
  contents operations. Exit: those rows equal Chromium's.

9b-2 before 9b-3 because GitHub's click error and React's selection
restore need the selection, and the contents operations are not on
GitHub's paths of the M10 list.

## Decisions on the open questions

The orchestrator's answers of 2026-11-14:

1. The order is 9b-1, 9b-2, 9b-3. 9b-1 may be committed without the
   contents operations and the geometry; their probe rows are `Later`
   until 9b-3, with that reason.
2. `getComposedRanges` goes to the backlog (no fixture uses it); 9b-2 is
   the selection, the native tie and `selectionchange`.
3. Frames: follow Chromium. A click into a frame keeps the parent
   document's selection, painted as today (no inactive color). 9b-2
   updates the "Frames" paragraph of ADR 0009 with the code.
4. `containsNode`: follow Chromium after more probe cases in 9b-2 (three
   equal runs); a measured departure from the standard.
5. A script `collapse` while a text control has the focus: 9b-2 measures
   it again (three equal runs, and cases with `addRange` and
   `selectAllChildren`). If the runs are equal, follow Chromium; else leave
   the control's selection alone and list it as a departure.

## 9b-2 implementation notes

What 9b-2 did differently from section 3, or added after new measurements
(three equal runs each of the 7 cases that 9b-2 added to
`tools/probes/web-selection.json`):

- **Entries.** No owner bits per entry: a script's `Range` object owns the
  entry of its id (the host's ids stay below 2^31), and the selection's state
  (`swb_dom::SelectionState`, `ranges/selection.rs`) says whether an object
  owns its entry too. The arena makes the selection's own entries (a gesture, a
  `Selection` method) with ids from 2^31 up; `getRangeAt` renames such an
  entry to the id of a new `Range` object, which the `Selection` keeps. An
  entry goes when neither owns it. The arena keeps the selection generation
  and the "changed" flag; setting the flag sets the host's request flag, so
  `after_request` queues `selectionchange` after a `Range` method or a data
  change that moved the selection.
- **Text controls.** The control that owns the document's selection is part
  of that state (it replaced `SelectionEvents::owner`): its range is collapsed
  at the control's place in its parent, and the bindings report the control's
  type, direction and text. A script's change of the document's selection
  while the control owns it collapses the control's selection at 0 and ends
  the ownership (the focus stays); `addRange` is ignored then; `extend`
  collapses at the new point; a later change of the focused control's
  selection takes the document's selection back (decision 5: the runs were
  equal, swb follows Chromium).
- **`selectionchange`** at the document waits in the queue of the controls'
  events (`html/selection_events.rs`) under the document node, so a change
  adds nothing while one waits. The bound of that queue (4,096 events) does
  not apply to it: a queue full of `select` events keeps it (review fix).
- **`containsNode`** (decision 4): Chromium answers from the caret positions
  that show the selection. The start moves forward to the first position of
  the next text, the end and a caret back to the last position of the
  previous text, neither across the edge of a block or a form control; a
  point between blocks is the start of the next block. A text node is
  contained when the moved range reaches or touches it; other nodes follow
  the standard's "contained" and "partially contained" over the moved range
  (`ranges/visual.rs`; the engine says which elements are blocks, inline or
  atomic, and which text is rendered: `EngineHost::visual_range`).
- **Frames** (decision 3, measured again): a document whose focus goes into
  one of its frames keeps its selection, unless the selection contains the
  frame's `iframe`; then it collapses before the `iframe` (select all, a
  range over the `iframe`). ADR 0009 "Frames" says so.
- **Gestures.** A click below the content puts the caret at `(body,
  childCount)` and fires `selectstart` at `html`, as in Chromium (the planned
  departure is gone); select all fires `selectstart` at the body; a drag
  after a script replaced the selection starts again at the pointer
  (measured, `user-drag-script-change`). The drag compares a count of the
  selection's replacements, not the entry id: a `getRangeAt` renames the
  entry but does not stop the drag (review fix; Chromium keeps the
  selection, measured).
- **Highlight and copy.** `TreeOrder` follows the shadow-including tree
  order, so a script's selection inside one shadow tree is painted and
  copied. The copied text keeps the spaces on both sides of a form control
  or replaced element ("pre  post") and the line break before a last line of
  controls (select all), as Chromium's `toString`. The display list is
  rebuilt when the selection generation changed and the visible selection
  differs.
- **Departures** (rows in the tests): `getComposedRanges`, `modify`,
  `ShadowRoot.getSelection` are absent; `deleteFromDocument` comes with
  `deleteContents` (9b-3); a selection from the document into a shadow tree
  collapses at the focus; a user's selection in a shadow tree reports the
  shadow tree's nodes; a double click 0.01 px inside a space selects the space
  (Chromium the word after it); the event logs of input steps differ in how
  many `selectionchange` events coalesce (Chromium handles input before the
  event's task, `run_steps` runs the tasks after each step).

## Consequences

- The DOM owns the selection; the engine's input code writes it and its
  paint and copy code read it, so a script's selection is painted and
  copied and a user's selection is seen by scripts, with one state.
- Every change function has one more `Option` test; with ranges, the cost
  per change is a hash lookup or the subtree search.
- The highlight and copy code work with boundary points in elements, not
  only text positions.
- `split_text` moves into `swb_dom`.
- Offsets inside surrogate pairs differ from Chromium.
