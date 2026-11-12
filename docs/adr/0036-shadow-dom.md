# ADR 0036: Shadow DOM

- Status: accepted
- Date: 2026-11-09

## Context

M10 targets the front page of github.com/senko/swb with JavaScript on
(roadmap, M10; [github-diagnosis.md](../github-diagnosis.md), rows A5 and A8).
Since feature 4 (ADR 0035) GitHub's 74 custom element definitions succeed and
its 10 parsed custom elements are upgraded. The 23 `relative-time` elements
fail: their constructor calls `attachShadow`, which swb lacks, so
`createElement` returns a failed `HTMLUnknownElement` and the page shows the
fallback date text ("Oct 18, 2026") where Chromium shows "4 days ago".
Feature 5 adds shadow trees: `attachShadow`, slots, the flat tree in style,
layout, paint and hit testing, style scoping, constructable style sheets and
`adoptedStyleSheets`, event retargeting, focus and the Tab order through
shadow trees.

What exists (read in swb's code at 96586aa):

- **DOM** (`crates/dom/src/tree.rs`). One arena per document, nodes never
  freed, `NodeId(NonZeroU32)`. A node has intrusive links (`parent`,
  `first_child`, `last_child`, `prev_sibling`, `next_sibling`), child counts,
  the node document (`owner`) and `NodeData` (`Document`, `DocumentFragment`,
  `Doctype`, `Element(ElementData)`, `Text`, `Comment`,
  `ProcessingInstruction`). Measured: `size_of::<Node>()` 104,
  `NodeData` 72 (the three strings of `Doctype`), `ElementData` 64 (pinned by
  the test `element_data_keeps_its_size`; two bytes of padding are free after
  `custom_state`). A field of 4 bytes in `ElementData` makes it 72,
  `NodeData` 80 and `Node` 112 (measured with a temporary test). Template
  contents are a `DocumentFragment` of the same arena with an inert owner
  document; `Document::template_hosts` maps the fragment to its template
  (the host-including ancestor check). Walk helpers: `parent`, `children`,
  `element_children`, `ancestors`, `descendants` (preorder, never into
  template contents), `root`; `is_connected` is "the root is a document
  node" (its comment says "swb has no shadow trees").
- **Bindings.** `ShadowRoot` is an interface object without members
  (`dom.idl`, M9 feature 1: Alpine.js tests `instanceof ShadowRoot`);
  `HTMLSlotElement` has `name`; `getRootNode` exists and ignores `composed`;
  `Element.slot`, `assignedSlot`, `attachShadow`, `shadowRoot`,
  `adoptedStyleSheets` and the `CSSStyleSheet` constructor, `replace` and
  `replaceSync` do not exist (`cssom.idl` says so). `state::class_of` picks a
  wrapper class by `NodeData`.
- **Events** (`crates/web/src/events/dispatch.rs`). The path is the target,
  its DOM ancestors and the window. `PathItem` already has the fields of
  DOM's struct (`in_shadow_tree`, `shadow_adjusted_target`,
  `root_of_closed_tree`, `slot_in_closed_tree`), always false or `None`;
  `run_phases` treats a struct with an adjusted target as `AT_TARGET`, and
  `composed_path` already implements the hidden levels of closed trees. No
  per-struct `relatedTarget`; `relatedTarget` is never retargeted.
- **Focus** (`crates/engine/src/focus.rs`, `crates/web/src/html/focus.rs`).
  `next_in_order` is one `descendants(DOCUMENT)` pass sorted by (tabindex
  group, tree position); its module comment says "no focus navigation
  scopes". `:focus-within`, `:hover` and `:active` use chains of DOM
  ancestors (`StateSet::new_with`, `crates/style/src/element.rs`).
- **Style** (`crates/style/src/cascade.rs`). `Styler::run` walks
  `element_children` from the root element in preorder and pushes the
  parent's style with each child, so inheritance follows the DOM; a Bloom
  filter holds the ancestors of the walk. One `Stylist` per document, built
  from the document's sheets in tree order (`Sheets`,
  `crates/engine/src/resources.rs`; owners are `style` and `link` elements
  with `root == DOCUMENT`). `:host`, `:host()` and `:host-context()` parse and
  never match; `::slotted()` and `::part()` are invalid selectors. Any DOM
  change in the document restyles everything (`finish_batch`,
  `crates/engine/src/document/changes.rs`). The UA sheet has
  `slot { display: contents }`.
- **Box tree** (`crates/layout/src/box_tree.rs`). Rebuilt on every layout from
  the root element; `push_children` iterates `doc.children(parent)` and gives
  text nodes the style of their DOM parent. Table construction
  (`table/build.rs`, `for_each_child`) walks `doc.children` as well.
- **Paint and hit testing** work on fragments only; a fragment carries the
  `NodeId` of its element or text node. `element_of` maps a hit text node to
  its parent element; `elementFromPoint` returns the hit element.
- **Custom elements** (ADR 0035): the upgrade candidates of `define` and
  `upgrade(root)` walk `descendants`; the sink walks inserted and removed
  subtrees with `descendants`; `Definition::disable_shadow` is read and kept
  (`#[allow(dead_code)]`, "checked with feature 5").
- **Parser.** html5ever 0.40.1. Its `TreeSink` trait (public API
  documentation of markup5ever 0.40) has two provided methods for
  declarative shadow roots: `allow_declarative_shadow_roots(&self,
  intended_parent) -> bool` and `attach_declarative_shadow(&self, location,
  template, attrs) -> bool` ("Attempt to attach a declarative shadow root at
  the given location"). swb's sink overrides neither.

Sources: the DOM Standard (§4.2.2 shadow trees, §4.2.2.1 slots, §4.2.2.2
slottables, §4.2.2.3 to 4.2.2.5 finding and assigning slots and slottables,
"signal a slot change"; §4.8 `ShadowRoot`; §4.9 `attachShadow`; §2.9
dispatching events with "get the parent", retargeting, `relatedTarget`,
"clear targets"; §2.3 `composedPath`; §4.2.6 "shadow-including tree order",
`getRootNode`), the HTML Standard (§6.6.3 focus navigation scope owners and
"flattened tabindex-ordered focus navigation scope", §6.6.4 focus delegates
and `delegatesFocus`, §6.6.6 `DocumentOrShadowRoot.activeElement`, §13.2.6.5
declarative shadow roots in the tree builder), CSS Scoping 1 (§3.1 cascade
contexts, §3.2 the shadow host in the cascade and `:host`, `:host()`,
`:host-context()`, §3.2.1 `::slotted()`, §3.3 the flat tree and inheritance),
CSS Shadow Parts 1 (`::part()`, `exportparts`), CSSOM (§6.1.2 the
`CSSStyleSheet` constructor, `replace`, `replaceSync`; `adoptedStyleSheets` of
`DocumentOrShadowRoot`), Web IDL (`ObservableArray`). Text of 2026-11-09.
Measurements in Chromium 148 (below). The session that wrote this ADR read
no browser or engine source code; it read swb's own code and markup5ever's
API documentation.

### What GitHub needs

Measured in Chromium's replay of `fixtures/js/github-swb` with an init script
that wraps `attachShadow`, the `adoptedStyleSheets` setters of `Document` and
`ShadowRoot`, the `CSSStyleSheet` constructor, `replace`, `replaceSync`,
`getRootNode`, `composedPath`, `addEventListener` (for `slotchange` and for
targets in shadow trees), `dispatchEvent` in shadow trees, the slot members,
`shadowRoot`, and `ShadowRoot.innerHTML`, and that listens at the window in
the capture phase for 26 event types to count the events whose path has a
shadow root (`swbtools replay github-swb --init-script FILE --steps FILE
--record EXPR`; step groups: a wait of 2 s, a hover over the relative time of
the latest-commit line, three Tab presses; three runs). The record
expression reads every shadow host's root (options, `innerHTML`, style
sheets, slots and their assigned nodes, parts, rendered text, rects). The
three runs are equal apart from one count: `getRootNode` has 18, 18 and 17
calls; all come from one function of `react-dom` on `div` containers after
`load`, so the number follows the timing of React's scheduled work (not
investigated further; no call uses `composed`, none returns a shadow root).

| Item | Chromium replay |
|---|---|
| `attachShadow` | 26 calls, all `{mode: "open"}`, none fails: `tool-tip` 1 (at its upgrade in `define`, `readyState` `interactive`, connected); `relative-time` 25 (in the constructor that `createElement` runs, after `load`, not connected); 24 roots live (23 `relative-time` are inserted) |
| Options and flags of the 24 roots | `mode` open, `delegatesFocus` false, `slotAssignment` named, `clonable` false, `serializable` false; no declarative root; no nested host; no `exportparts` |
| `relative-time` shadow content | `<span part="root">4 days ago</span>`: no style sheet, no slot, no adopted sheet; the light child (the date text of the HTML, "Oct 18, 2026") is not rendered. Texts of the 23 hosts at the page clock: "4 days ago" 7, "5 days ago" 1, "last week" 1, "2 weeks ago" 7, "3 weeks ago" 7 (the list in document order is in the replay records). The visible one of the commit line: 59 x 18 px at (754, 331); the file list's at x 840 to 861, 16 px high; the first host is in a hidden container (0 x 0) |
| `tool-tip` shadow content | one `<style>` (`:host` with 20 declarations and `var()`, `:host(:is(.tooltip-n, ...))`, `:host(.tooltip-w)`, `:host(.tooltip-e)`, `:host:after`, `:host(.tooltip-s):after` and the other directions, `@keyframes tooltip-appear`, `:host(:popover-open)`, `:host(:popover-open):before`, `:host(.\:popover-open)`, `@media (forced-colors: active)` with `:host` and `:host:before`) and a default `<slot>` with the light text "Dismiss alert". The host is a closed `popover=manual` with `class="sr-only position-absolute"`: `display: none`, no box; the button it labels is in a hidden alert (0 x 0) |
| `adoptedStyleSheets`, `new CSSStyleSheet`, `replaceSync`, `replace` | none by the page. The one adopted sheet, constructor call and `replaceSync` are the tools' own (`browser.stop_animations`, `tools/swbtools/browser.py`); the diagnosis counted them as the page's |
| `::slotted`, `::part`, `exportparts`, `:host-context` in any sheet | none (the `::part` name `root` is never styled) |
| `getRootNode` | 17 or 18 (above), all without `composed`, all on `div` elements whose root is the document |
| `composedPath`, `slotchange` listeners, `assignedNodes`, `assignedElements`, `assign`, `assignedSlot` | 0 |
| `shadowRoot` reads | 64 by the page during the load |
| Listeners on shadow roots or shadow nodes, `dispatchEvent` in shadow trees, `ShadowRoot.innerHTML` writes | 0 (the components fill their roots with DOM methods) |
| `delegatesFocus` | not used; no focusable element in any shadow root |
| Events that cross a boundary | the hover over the commit line's relative time: `pointerover`, `mouseover`, `pointermove`, `mousemove` with `composedPath()[0]` the shadow `span` and `target` the `relative-time` host. The load and three Tab presses: none (`:focus-visible` on the third Tab's `button`, outside shadow trees) |

Code that runs in swb and not in Chromium (polyfills that test for features
that Chromium 148 has and swb lacks; read in the fixture's files):

- A `command` polyfill (`CommandEvent` and `commandForElement` are missing in
  swb) wraps `Element.prototype.attachShadow` and
  `HTMLElement.prototype.attachInternals`: for each new shadow root it adds a
  capturing `click` listener to the root and observes the root with a
  `MutationObserver` (`attributeFilter: ["oncommand"]`). It is the wrapper
  that ADR 0035's 4a notes saw failing (`attachShadow` undefined).
- The `popover` polyfill (until feature 6 adds `popover`) also wraps
  `attachShadow` and puts its CSS (`:popover-open` rewritten as the class
  `.\:popover-open`) into the document and into each new shadow root: with
  `new CSSStyleSheet()` and `replaceSync` into `adoptedStyleSheets` (`root.
  adoptedStyleSheets = [sheet, ...root.adoptedStyleSheets]`), or, if the
  constructor throws, as a `<style>` element prepended to the root. Its
  helpers walk `assignedSlot`, `parentElement`, `getRootNode()` and `host`.
- Catalyst's lazy definitions use `adoptedStyleSheets` only with a feature
  flag that the page has off (ADR 0035, "What GitHub needs").

What the exit needs ("GitHub's `relative-time` and `tool-tip` render as in
Chromium"):

1. `attachShadow({mode: "open"})` on an autonomous custom element during its
   construction (detached) and at its upgrade (connected); `shadowRoot`; DOM
   methods on a shadow root (`append`, `appendChild`, `replaceChildren`,
   `textContent`) and on its children. Then the constructor succeeds and
   ADR 0035's callback counts reach the replay's (connected 35,
   attributeChanged 76).
2. The flat tree in the box tree: a host's shadow children replace its light
   children; a default slot shows its assigned nodes. `relative-time` then
   renders "4 days ago" in an inline box of the shadow `span`.
3. `tool-tip` renders nothing while it is a closed popover, in both browsers
   (swb's UA sheet already hides `[popover]`). Its `:host` styles matter when
   it opens (hover or focus on its button), which the replay's first view
   does not show.
4. The polyfills of swb's run: `MutationObserver.observe(shadowRoot)`,
   listeners on shadow roots, `adoptedStyleSheets` on shadow roots or
   `<style>` elements in shadow roots (scoped, so that the polyfill's CSS
   does not leak).

The other "works" items: item 3 to 8 (menus, popovers, links) need events
from shadow content to reach the host and the document (a click on a
`relative-time` inside a link must activate the link: the hit is the shadow
`span`); item 9 is the exit; item 10 (the Tab order) has no focusable element
in a shadow root, so the order only needs the flat walk to skip nothing.

Other targets (grep of `fixtures/js` and `fixtures/pages`):

| Fixture | Shadow DOM text in its files |
|---|---|
| Ars Technica (JS) | `attachShadow` 1, `composedPath` 2, `getRootNode` 2, `assignedElements` 1, `assignedSlot` 1, `:host` 2, `composed:` 2 |
| BBC (JS) | `attachShadow` 1, `getRootNode` 1 (the video player's feature test, ADR 0035) |
| Craigslist posting and search (JS) | `composedPath` 2 and 1, `getRootNode` 2, `:host` 1 |
| Wikipedia (JS) | `attachShadow` 1 (Vue's custom element support), `getRootNode` 2, `<slot` 1 |
| Ars Technica (JS off) | `:host` 1: Tailwind's preflight `:host,html{...}` in a document sheet, which matches nothing in a document |
| All | no declarative shadow root (`shadowrootmode`, `shadowroot=`, `setHTMLUnsafe`, `parseHTMLUnsafe`: 0) |

Whether these paths run in the targets' first views is not measured here;
the targets' scores with JavaScript on are the check of each sub-feature.

### Measurements

Two new probe files: `tools/probes/web-shadow-dom.json` (JavaScript values; 21
cases, 119 queries; the logging header of `web-custom-elements.json` with a
`k()` that names shadow roots `root(host)`, the document `doc` and the window
`win`, `S(host, html, init)` that attaches and fills a root, and `cs()` for
computed values) and `tools/probes/shadow-dom.json` (boxes for `--with-swb`;
16 cases, boxes of hosts and light children: the probe's `boxes` query and
swb's dump see the light tree only, so shadow content shows in the hosts'
sizes). Every case has a control query. Chromium 148, three runs of each
file, byte-identical values.

| Question | Case | Chromium |
|---|---|---|
| Members | `api-surface` | `ShadowRoot.prototype`: `activeElement, adoptedStyleSheets, clonable, delegatesFocus, elementFromPoint, elementsFromPoint, fullscreenElement, getAnimations, getHTML, getSelection, host, innerHTML, mode, onslotchange, pictureInPictureElement, pointerLockElement, serializable, setHTML, setHTMLUnsafe, slotAssignment, styleSheets, customElementRegistry`; `HTMLSlotElement.prototype`: `assign, assignedElements, assignedNodes, name`; `Element.prototype` has `attachShadow, shadowRoot, slot, assignedSlot, part, getHTML, setHTMLUnsafe`; `Text.prototype.assignedSlot`; lengths `attachShadow` 1, `assignedNodes` 0, `assign` 0, `CSSStyleSheet` 0; `shadowRoot` getter only; `document.adoptedStyleSheets` getter and setter, `Array.isArray` true, prototype `Array.prototype`. `CSS.supports('selector(...)')`: `:host`, `:host(.a)`, `:host-context(.a)`, `::slotted(span)`, `::part(x)`, `::slotted(span:hover)`, `::part(x):hover`, `::slotted(span)::before` true; `:has-slotted` false |
| `attachShadow` hosts and errors | `attach-errors` | works: `div span article aside blockquote body footer h1 h6 header main nav p section` and a valid custom element name; `NotSupportedError: Failed to execute 'attachShadow' on 'Element': This element does not support attachShadow` for `a img input button template slot video li ul table textarea iframe font-face foo`, an SVG `g`, a `div` in another namespace and `createElementNS(HTML, 'X-A')`; a second root: `NotSupportedError: ...: Shadow root cannot be created on a host which already hosts a shadow tree.`; `{}`: `TypeError: ...: Failed to read the 'mode' property from 'ShadowRootInit': Required member is undefined.`; no argument: `...: 1 argument required, but only 0 present.`; `mode: 'bogus'`: `...: The provided value 'bogus' is not a valid enum value of type ShadowRootMode.`; `null`: `...: The provided value is not of type 'ShadowRootInit'.`; `slotAssignment: 'bogus'`: `... of type SlotAssignmentMode.`; booleans convert (`delegatesFocus: 1` true) |
| The disable-shadow flag | `attach-errors` | `new D()` and `createElement` of a definition with `disabledFeatures = ['shadow']`, and its constructor: `NotSupportedError: ...: attachShadow() is disabled by disabledFeatures static field.`; an `undefined` element with a root, upgraded by `define`: the upgrade fails with the report `NotSupportedError: The element already has a ShadowRoot though it is disabled by disabledFeatures static field.` (no prefix; the element stays undefined, no constructor runs) |
| Root properties | `root-properties` | `nodeType` 11, `nodeName` `#document-fragment`, `String` `[object ShadowRoot]`, `parentNode` null, `isConnected` follows the host; a closed root: `host.shadowRoot` null; `getRootNode()` of a shadow node is the root, with `composed: true` the document (a detached host: the host); `document.contains(inner)` and `host.contains(inner)` false; a child of the root has `parentElement` null; `host.innerHTML` and `textContent` are the light tree's; `document.getElementById` and `querySelector` do not see shadow nodes, the root's do; `r.appendChild(host)`: `HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent.`; `cloneNode` of a root: `NotSupportedError: ...: ShadowRoot nodes are not clonable.`; `importNode`: `...: The node provided is a shadow root, which may not be imported.`; `adoptNode`: `HierarchyRequestError: ...: The node provided is a shadow root, which may not be adopted.`; appending a root to an element moves the root's children (a fragment insertion); `cloneNode(true)` and `importNode` copy a `clonable` root only; `getHTML({serializableShadowRoots: true})` writes a `serializable` root as `<template shadowrootmode="open" shadowrootserializable="">` |
| Named slots | `slot-assignment-named` | the default slot gets the slottables without `slot` (elements and text, also whitespace text; not comments); a named slot gets the first matches in tree order; a later slot of the same name gets nothing; changing `slot`, `name` or inserting a slot reassigns at once (no microtask); `assignedNodes({flatten: true})` of a slot without assigned nodes is its fallback content; a slot outside a shadow tree: both lists empty; a nested slot's flattened list has the outer host's children; `assignedSlot` is null for a closed root |
| Manual slots | `slot-assignment-manual` | `assign(b, a)` keeps that order; assigning a node to a second slot removes it from the first; a node that is not a child of the host is kept and assigned when it becomes one; `assign()` empties; `assign(1)` and `assign(comment)`: `TypeError: Failed to execute 'assign' on 'HTMLSlotElement': The provided value is not of type '(Element or Text)'.`; a removed slot keeps its manual list and gets it back when it returns |
| `slotchange` timing | `slotchange-timing` | one event per slot per microtask checkpoint, in the mutation observer microtask after the observers' callbacks (`sync mo:1 sc mt`); `bubbles` true, `composed` false, `cancelable` false, trusted; fired for a slot inserted with matches, for both slots of a `slot` change, for slots of detached trees, when the fallback content of a slot without assigned nodes changes, and for an empty text node; not for a comment; a nested slot's event reaches the outer slot by the flat path (`sc:os@is`), and the inner slot gets its own event only when its own assigned nodes change; `assign()` with an unchanged list: none |
| `composedPath`, open and closed | `composed-path-open`, `composed-path-closed` | listeners outside see `target` = host; inside, the shadow node; a host's capture listeners run at phase 2 in the capture pass (before the root's), its bubble listeners at phase 2; `composed: false` stops at the root and leaves `target` null after the dispatch; after a composed dispatch from inside, `target` is the host; a closed root: outside listeners' `composedPath()` stops at the host (`div#h>div#o>body>html>doc>win`); a slotted light node's path goes through the slot and the shadow tree, which listeners outside a closed tree do not see |
| `relatedTarget` | `retarget-related` | target and related target in one shadow tree: listeners in the tree only, then both null; related target outside: retargeting of the target only; target outside and related inside: the related target becomes the host; related target the host: the event stays in the tree |
| Focus | `focus-shadow` | a focused shadow `input`: `document.activeElement` the host, `root.activeElement` the input, the host matches `:focus` and `:focus-within`, `:focus-visible` only the input; `focus` and `focusin` at the document have the host as `target`; a move between two inputs of one tree is not seen outside the tree (target and `relatedTarget` retarget to the host); nested trees: each root's `activeElement` is its retargeted element; `delegatesFocus`: `host.focus()` focuses the `autofocus` element of the tree, else the first focusable, else nothing (`tabIndex` stays −1); a focused slotted element: the slot and its shadow ancestors match `:focus-within` (the flat tree) |
| Tab order | `tab-order`, `tab-order-backward` | forward: `b3 b1 s3 s2 s1 b2 h2 s4 b4 s6 l1 s7 l2 s8 b5`: positive `tabindex` per scope (`s3`=1, `s2`=2 before `s1`=0 within the root); a focusable host before its tree; a host with `tabindex=-1` skips its tree; slotted elements in the slot's place; a `delegatesFocus` host is not itself in the order; backward is the reverse |
| Style scoping | `style-scoping` | document `#h1 {color: red}` beats `:host {color: green}`; `:host {color: green !important}` beats the document's `!important`; document rules do not match shadow nodes, shadow rules do not match light nodes; `::part(root)` from the document and through `exportparts`; `::slotted(span)` styles a slotted element but a document rule on it wins; `::slotted` matches only the assigned elements, not their descendants; slotted elements inherit from the slot; shadow children inherit from the host; `:host(.a)`, `:host(:not(.a))`, `:host span`, `:host(.a) > span`, `:host-context(.dark)` match; `:host::after` generates content; custom properties inherit into the tree; slot `display: contents`; an element that is not slotted has a computed style, no box and no client rects |
| Order and specificity | `style-specificity-and-order` | two `<style>` of a root in tree order; adopted sheets after them; `:host(.x)` (0,2,0) beats `:host` (0,1,0) |
| Constructable sheets | `constructable-sheets` | `new CSSStyleSheet()`: no rules, `href` and `ownerNode` null, media and `disabled` from the options; `replaceSync` drops `@import` and invalid rules; `replace` returns a promise of the sheet and drops `@import`; `replaceSync` on a sheet of an element: `NotAllowedError: Failed to execute 'replaceSync' on 'CSSStyleSheet': Can't call replaceSync on non-constructed CSSStyleSheets.`; adopting one: `NotAllowedError: Failed to set the 'adoptedStyleSheets' property on 'Document': Can't adopt non-constructed stylesheets.`; a sheet of another document: `... Sharing constructed stylesheets in multiple documents is not allowed`; `insertRule('@import ...')`: `SyntaxError: ...: Can't insert @import rules into a constructed stylesheet.`; `[1]`: `TypeError: ...: Failed to convert value to 'CSSStyleSheet'.`; `5`: `... The provided value cannot be converted to a sequence.`; `push` on the array works; changes of an adopted sheet apply in every root that adopts it; `document.styleSheets` and `root.styleSheets` exclude adopted sheets |
| Rendering reads | `render-reads` | `host.innerText` is `""` (the light text is not rendered), an ancestor's `innerText` leaves the shadow text out (`"by  end"`), a shadow element's `innerText` is its text; `elementFromPoint` over shadow text gives the host, `root.elementFromPoint` the shadow element, `elementsFromPoint` likewise; a shadow element's `offsetParent` is the light `body` |
| Mutation observers | `mutation-observers` | an observer of the document's subtree sees no change inside a shadow tree and none for `attachShadow`; an observer of the root sees them |
| Custom elements | `custom-elements-in-shadow` | `define` upgrades in shadow-including tree order (a host's shadow tree before the host's following siblings, nested trees inside); inserting into a connected root runs `connectedCallback`; removing a host disconnects its shadow-including descendants in that order; `upgrade(host)` upgrades a detached host's shadow tree |
| UI events | `ui-events-through-shadow` | a click on shadow text: `target` the host, `composedPath()[0]` the shadow element, for `pointerover mouseover mousedown mouseup click`; the host and the shadow element match `:hover` |
| Declarative roots | `declarative-js-on`, layout `declarative-js-off` | the main parser attaches `<template shadowrootmode>` (also with JavaScript off: same boxes); the template element disappears; `shadowrootdelegatesfocus`, `shadowrootclonable`, `shadowrootserializable` set the flags; a bogus mode leaves the template; a second template on the same host stays a template; `innerHTML` and `DOMParser` do not attach, `setHTMLUnsafe` and `Document.parseHTMLUnsafe` do; `attachShadow` on a declarative host returns the same root, emptied |
| Depth | `nesting-depth` | 1,000 nested shadow trees render and are connected |
| Boxes | `shadow-dom.json` | the light children of a host have no box; slotted children sit between the shadow siblings of their slot; named slots order by the slots; fallback renders without assigned nodes; `relative-time` with "4 days ago" in a flex row is 78.28 px wide; a host in a paragraph wraps its shadow text with the line; `:host {display: block}` with padding and border; slot `display: block`; slots inside flex and table containers act as `display: contents` (the slotted items are the flex items and table rows); nested slot pass-through; `:host {display: none}` |

## Decision

### 1. Shadow roots in `swb_dom`

- **The node.** A new variant `NodeData::ShadowRoot(ShadowRootData)` with
  the host (`NodeId`), the mode (`open`, `closed`), the slot assignment
  (`named`, `manual`) and the flags `delegates_focus`, `clonable`,
  `serializable`, `declarative`. It is a document fragment for the DOM's
  rules (`is_fragment()` answers true for both variants; the match arms that
  name `DocumentFragment` get the new variant, so the compiler lists every
  place to check). The root has no parent; its node document is the host's.
  A new variant, not a side table on a `DocumentFragment`: the bindings pick
  the `ShadowRoot` class by `NodeData`, and the many fragment paths must
  decide case by case (a root is never cloned, imported or adopted:
  measured messages).
- **The host's link to its root.** `ElementData` gets a `shadow_flags: u8`
  in its padding (size stays 64; the test stays): `HOST` (the element has a
  shadow root) and `SLOT` (a `slot` in a shadow tree with a non-empty
  assigned list). The root's `NodeId` and the slot data live in a per-arena
  `Document::shadow: Option<Box<ShadowState>>` (`None` until the first
  `attachShadow`): `roots: HashMap<host, root>`, the slots of each root in
  tree order, the assigned nodes of each slot, the assigned slot of each
  slottable, the manual assignments, and the signal slots. Readers test the
  byte (already in cache with the element) before any lookup; a document
  without shadow roots pays one `Option` test per walk. Rejected: a field
  in `ElementData` (measured: `Node` grows from 104 to 112 bytes for every
  node of every page); moving `parser_form` out to make room (a change of
  the parser for a field that the flag byte makes unnecessary).
- **Attach** (`Document::attach_shadow(host, init) -> Result<NodeId,
  DomError>`): DOM "attach a shadow root" with the checks in Chromium's
  order and texts (table): HTML namespace and a valid shadow host name (a
  valid custom element name or the 16 names of the DOM Standard), the
  custom element state and the definition's disable-shadow flag (the web
  layer passes the flag, since definitions are host data), an existing
  root (a declarative root of the same mode is emptied and returned:
  measured). No journal entry and no mutation record (measured); the
  document's `shadow_changed` flag (like 4a's `defined_changed`) tells the
  engine's drain to restyle when the host is connected.
- **Connectedness and roots.** `root(node)` stays the DOM root (a shadow
  node's root is its shadow root). New: `shadow_including_root(node)` (from
  a root to its host's root, repeated) and `is_connected` = "the
  shadow-including root is a document node". Fast path: a node whose root
  is a document node or a plain fragment answers as today; only a root of
  kind `ShadowRoot` loops.
- **Host-including ancestors** (`is_host_including_inclusive_ancestor`, the
  pre-insertion check) follow `ShadowRoot::host` as they follow
  `template_hosts` (measured message for `r.appendChild(host)`).
- **Adoption and node documents.** `set_node_document` walks the
  shadow-including descendants (roots included), so a host adopted into
  another document takes its tree along; `adoptNode` and `importNode` of a
  root throw (measured).
- **Clones.** "Clone a node" copies a `clonable` root with its children
  (measured: `cloneNode(true)`, `cloneNode(false)` and `importNode` of a
  host with a clonable root give a copy with the root's content; a root that
  is not clonable is not copied).
- **Transplant between arenas** (`transplant.rs`, frames): `Subtree` takes the
  shadow roots of its hosts with their content (as `contents_of` takes
  template contents), and `put_subtree` rebuilds the moved hosts'
  `ShadowState` entries and recomputes their slot assignments.
- **The journal and the mutation sink.** Changes inside shadow trees are
  ordinary `Children`, `Attribute` and `Text` changes of the arena. The
  engine's drain decides "in the document" with `is_connected` instead of
  `root == DOCUMENT` (`changes.rs`, `in_document`), so a change in a
  connected shadow tree restyles. The mutation observer registry needs no
  change: its walk of inclusive ancestors stops at a root (no parent),
  which is the DOM's rule (measured: the document's observer sees nothing
  of a shadow tree; an observer of the root sees it).

### 2. Slots and slot assignment

- **Named and manual.** Both. Manual assignment is small on top of named
  (a list per slot, a "manual slot" per slottable, `assign()`), and the
  measured behaviour is complete.
- **When.** Assignment runs eagerly, in the change functions of `swb_dom`,
  as the DOM Standard says: at an insertion into a host ("assign a slot"),
  at the insertion or removal of a slot in a shadow tree and at a change of
  a slot's `name` ("assign slottables for a tree"), at a change of a
  slottable's `slot` attribute, at a removal from a host. The steps run only
  when the arena has a `ShadowState` and the parent is a host or the node
  is in a shadow tree (the flag byte), so a page without shadow roots pays
  one test per change. Measured: reassignment is visible at once
  (`slot-assignment-named`), which a lazy cache would also give, but the
  signals of `slotchange` must be made at the change ("signal a slot
  change" when the assigned list differs), so eager assignment gives both
  with one mechanism.
- **Find a slot** looks up the root's slots by name in tree order: the
  root keeps its slots in a `Vec` in tree order (inserted with a tree-order
  comparison, `Document::precedes`) and a map name → first slot. GitHub has
  one slot in 24 roots.
- **Signal a slot change.** `ShadowState::signal_slots: Vec<NodeId>` (no
  duplicates) and a flag that the web layer reads after each native and at
  each microtask checkpoint (as ADR 0035's sink sets `microtask_wanted`): it
  queues the mutation observer microtask, whose last step fires `slotchange`
  at each signalled slot, in order, after the observers' callbacks
  (measured). The DOM's insert and remove steps also signal a slot whose
  assigned list is empty when its children change (measured: a fallback
  change fires `slotchange`).
- **The flattened lists** (`assignedNodes({flatten: true})`, the flat tree)
  are computed from the stored lists: a slot child of an assigned list is
  replaced by its own flattened list; a slot without assigned nodes gives
  its fallback children (only in a shadow tree: measured empty lists for a
  slot outside one).

### 3. Shadow-including walks

`swb_dom` gets `shadow_including_descendants(node)` (preorder; a host's
shadow tree right after the host, before its light children: DOM
"shadow-including tree order", measured with `define`) and
`shadow_including_ancestors(node)`. Every walk that means "the document"
changes to it:

| Walk | Today | Change |
|---|---|---|
| Custom elements: upgrade candidates of `define`, `upgrade(root)` | `descendants` | shadow-including, measured order |
| Custom elements: the sink's walks of inserted, removed and adopted subtrees | `descendants`; `is_connected(parent)` | shadow-including; connectedness through hosts (measured: removing a host disconnects its tree) |
| Style sheet owners (`Sheets::rediscover`, `collect_subtree`, `in_document`) | `descendants(DOCUMENT)`, `root == DOCUMENT` | shadow-including; each slot records its tree scope (section 5) |
| Image loads (`loads.rs`), frames (`document/frames.rs`), forms, `boxes.rs`, the rendering update's walks | `descendants(DOCUMENT)` | shadow-including where the element must work inside a shadow tree (images, frames, form controls); each call site is checked in 5a |
| The id index, `getElementById`, `querySelector*` of the document, mutation observers | DOM | unchanged (measured: they do not see shadow nodes) |

The disable-shadow flag: `attachShadow` fails for a `precustomized` or
`custom` element whose definition has it (measured message); the upgrade
fails when the element already has a root (measured report without a
prefix; ADR 0035 section 4's failure steps). The upgrade candidates and
`upgrade(root)` cost the size of the shadow-including subtree, charged per
node as in ADR 0035.

### 4. The flat tree

**Definition** (CSS Scoping §3.3, DOM "flat tree"): the children of a host
are its shadow root's children; the children of a slot in a shadow tree are
its flattened assigned nodes, or its fallback children when it has none; a
light child of a host that no slot takes is not in the flat tree. The flat
parent of a node is the inverse.

**API in `swb_dom`** (new module `flat.rs`), no cached tree:

```rust
impl Document {
    pub fn flat_children(&self, node: NodeId) -> FlatChildren<'_>;
    pub fn flat_element_children(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_;
    pub fn flat_parent(&self, node: NodeId) -> Option<NodeId>;   // None: no parent or not in the flat tree
    pub fn flat_parent_element(&self, node: NodeId) -> Option<NodeId>;
    pub fn flat_ancestors(&self, node: NodeId) -> FlatAncestors<'_>;
    pub fn in_flat_tree(&self, node: NodeId) -> bool;
}
pub enum FlatChildren<'a> { Dom(Children<'a>), Listed(std::slice::Iter<'a, NodeId>) }
```

- `flat_children(n)`: if `self.shadow` is `None` → `Dom(children(n))` (the
  fast path: one branch). Else, for an element: `HOST` flag → the root's
  children; `SLOT` flag → the stored flattened list (`Listed`); otherwise
  `Dom`. A slot in a shadow tree without assigned nodes falls through to
  its DOM children (the fallback). The flattened list of a slot is kept
  with the slot's assigned list and updated by the assignment steps, so
  the walk does no work for slots.
- `flat_parent(n)`: `parent(n)`; if it is an element with `HOST`: the
  assigned slot of `n` (`None` if it has none: not in the flat tree), and
  if that slot is itself assigned onward, the walk uses the innermost slot
  whose flattened list holds `n`; if the parent is a shadow root: its host;
  else the parent.
- Iterative, like the existing walks; depth needs no recursion.

Rejected: a cached flat tree with invalidation (a second set of links for
every node, kept by every change function; the fast path would then still
need the test, and the box tree is rebuilt from scratch on every layout
anyway).

**Who walks it:**

| Stage | Today | With the flat tree |
|---|---|---|
| Style (`Styler::run`) | `element_children`, parent style pushed per child | `flat_element_children`; the stack also carries the child's tree scope (section 5). Inheritance follows the flat tree with no further change (measured: slotted elements inherit from the slot, shadow children from the host). The Bloom filter holds the flat ancestors: a superset of the DOM ancestors that a light-tree selector can see, and of the shadow-tree ancestors, so it never rejects a rule wrongly |
| `compute_element_styles` (`getComputedStyle`, a pseudo-element without a box) | the chain of DOM ancestors | the chain of flat ancestors for an element in the flat tree; for one outside it (measured: computed values exist) the DOM chain as today |
| Counters and list numbering (`counters.rs`) | `element_children`, `parent` | flat children and flat parent (CSS Lists: counters follow the flat tree) |
| Box tree (`push_children`, `push_flattened`, `table/build.rs` `for_each_child`) | `doc.children`, text style of the DOM parent | `flat_children`; a text node's style is the flat parent's (the slot's for slotted text, measured). Light children outside the flat tree get no box (measured) |
| Paint | fragments | no change |
| Hit testing | `element_of`: the hit node or its parent element; `link_around`: DOM ancestors | `element_of` uses the flat parent element; `link_around` and the cursor walk flat ancestors (a click on a `relative-time` inside a link finds the link) |
| `:hover`, `:active`, `:focus-within` chains (`StateSet::new_with`) | DOM ancestors | flat ancestors (measured: the host and the slot match) |
| `elementFromPoint`, `elementsFromPoint` | the hit element | retargeted against the document (measured: the host); `ShadowRoot.elementFromPoint` retargets against the root |
| `innerText` (`inner_text.rs`) | DOM children, "being rendered" from boxes and the DOM parent's style | DOM children as today (measured: an ancestor's `innerText` leaves shadow text out); a text node outside the flat tree is not being rendered (measured: a host's `innerText` is `""`) |
| Selection and copy (`selection.rs`) | `TreeOrder` of `descendants(DOCUMENT)`, `selected_text` over DOM children | unchanged in M10 (orchestrator's decision, Consequences): shadow text is not selected or copied. Later: `TreeOrder` and `selected_text` in flat tree order, so that a drag over a relative time selects and copies "4 days ago" (swb has no `getSelection`; this is the engine's own selection) |
| Find in page | (swb has none) | |
| Sequential focus navigation | `descendants(DOCUMENT)` | section 8 |

### 5. Style scoping

**Tree scopes.** Each style sheet belongs to a tree scope: the document or
one shadow root. Its rules match only elements of that scope, with three
exceptions that cross one boundary: `:host` rules (from a shadow root, on its
host), `::slotted()` rules (from a shadow root, on the elements assigned to
its slots, after flattening), and `::part()` rules (from an outer scope, on
elements of an inner scope that have the part name).

**Sheets of a shadow root.** `<style>` and `<link rel=stylesheet>` in a
connected shadow tree (shadow-including walk of section 3; each `SheetSlot`
gets its scope `NodeId`), then the root's adopted sheets. The engine keeps
the document's `Stylist` as today, and a scoped rule index per distinct
list of sheets (a key of the sheets' identities): the 23 `relative-time`
roots have no sheets and need no index; GitHub needs one index (`tool-tip`).
A root's index is built when a style pass first needs it and dropped with
the document's stylist (`invalidate_style`). Before 5c (section "Plan"),
sheets in shadow trees are not applied at all, so nothing leaks into the
document.

**Matching an element E of scope S** (in the walk of section 4):

1. UA (and quirks and hints) rules, as today.
2. The author rules of S, matched with DOM ancestors inside S (the
   existing `parent_element` stops at the root, which is not an element;
   `is_root` stays "the parent is a document node"). When a compound must
   match the host (`:host`, `:host()` in a combinator chain: `:host span`,
   `:host(.a) > span`, measured), the matcher reaches the host as the parent
   of the root's children and lets only `:host`, `:host()`,
   `:host-context()` match it (the host is "featureless" in its own tree).
3. If E is a host: the rules of E's root whose subject compound is
   `:host`, `:host(sel)` or `:host-context(sel)` (and pseudo-elements of
   them: `:host::after`, measured), matched with E as the host.
4. If E is assigned to slots (after flattening, each slot's scope from the
   innermost outward): the `::slotted(compound)` rules of those slots'
   scopes whose argument matches E (a compound only; measured: descendants
   of slotted elements do not match).
5. `::part(name)` rules of outer scopes on an element with that part name
   (`part` attribute), and through `exportparts` (both measured in
   Chromium; section 11 and open question 4 for the scope).

The selector model: `::slotted(compound)` and `::part(ident+)` become
pseudo-elements of `swb_css` with an argument (today functional
pseudo-elements are invalid), allowed with the user-action pseudo-classes
after them (`::slotted(span:hover)`, `::part(x):hover`: measured valid) and
`::before`/`::after` after `::slotted()`. `:host`, `:host()` and
`:host-context()` get a matching context: a flag in the matcher that says
"E is the host of the scope whose rules are being matched". `:host-context`
is valid and works in Chromium 148 (measured), so swb matches it: the host
or a flat ancestor of it matches the argument.

**The cascade** (CSS Cascade 5 §6.1, "context"; CSS Scoping §3.1): between
origin and importance on one side and layers on the other, each
declaration gets a context rank, the depth of its scope in the shadow-
including tree (document 0, a root of a host in the document 1, ...). For
normal declarations the shallower context wins; for `!important` ones the
deeper wins (measured both). `::slotted` and `:host` rules carry the depth
of the shadow root they come from; `::part` rules the depth of their
outer scope. Each scope has its own cascade layer order (`layers.rs` gets a
layer tree per scope); the document's adopted sheets join the document's
layers after its own sheets (measured in feature 1; the roadmap's backlog
item).

**Invalidation.** Unchanged in kind: every change in a connected tree,
an attach (`shadow_changed`), a manual `assign()` and a sheet change in a
root invalidate all styles (`invalidate_styles`, `invalidate_style` for
sheet changes, which also drop the scoped indexes). Per-element invalidation
does not exist today and is not added.

### 6. Constructable style sheets and `adoptedStyleSheets`

What swb has: the CSSOM model (`swb_style::cssom::sheet::StyleSheet` with
`owner: Option<NodeId>`, `parse_in`, `reparse`, `insert_rule`, `delete_rule`)
and its bindings (`cssom_sheets.rs`), no constructor.

- **`new CSSStyleSheet(options)`**: a sheet without owner, marked
  constructed, with its constructor document (the current window's
  document), `media` (string or `MediaList` text), `disabled`, and
  `baseURL` (parsed against the document's base URL; the error of an
  invalid URL is to be measured in 5c). Its memory counts in the
  document's `StyleMemory` (ADR 0028 section 16).
- **`replaceSync(text)`**: `reparse` without `@import` rules (dropped, with
  a `debug!` line); `NotAllowedError` with Chromium's text on a sheet that
  is not constructed (measured). **`replace(text)`**: the same parse, then a
  promise resolved with the sheet in a task (swb loads no `@import`, so
  nothing waits); `insertRule('@import ...')` on a constructed sheet throws
  the measured `SyntaxError`.
- **`adoptedStyleSheets`** on `Document` and `ShadowRoot`: Web IDL's
  `ObservableArray<CSSStyleSheet>`. swb's generator parses the type and
  generates nothing for it. Decision: a hand-written attribute whose getter
  returns one `Proxy` over a real `Array` per document or root
  (`[SameObject]` side table); the proxy's `set`, `defineProperty` and
  `deleteProperty` traps are natives that check each value (a constructed
  `CSSStyleSheet` of the same constructor document; measured `TypeError` and
  `NotAllowedError` texts), write the backing array and update the Rust
  list of the document or root, which the engine reads at style time
  (`EngineHost::adopted_sheets_changed`, then `invalidate_style`). `Array.
  isArray` is true for a proxy of an array, as Chromium's value is
  (measured: `Array.isArray(document.adoptedStyleSheets)` true);
  `push`, `pop`, `splice` and index writes go through the traps (measured:
  `push` works). The setter converts a sequence, checks every item, and
  replaces the backing array's content. Open question 6 has the simpler
  alternative.
- **Order**: a scope's adopted sheets come after its sheets of elements
  (measured), in array order; one sheet may be adopted by several roots and
  the document (measured: a change applies in all of them); `styleSheets`
  leaves them out (measured).

### 7. Events

DOM §2.9 "dispatch", with the parts swb leaves out today:

- **Get the parent.** `build_path` replaces `dom.ancestors` with the DOM's
  "get the parent": a slottable that is assigned → its assigned slot (the
  slot in a closed tree marks `slot_in_closed_tree`); a shadow root → null
  if the event is not composed and the root is the root of the first
  struct's target, else its host; a node → its parent; the document → the
  window (unless `load`). Each struct gets `in_shadow_tree` (the target's
  root is a shadow root) and `root_of_closed_tree`.
- **Retargeting.** "Retarget A against B" (A's root is a shadow root that
  is not a shadow-including inclusive ancestor of B → A's root's host,
  repeated) gives each struct its `shadow_adjusted_target` where it
  differs from the previous struct's; the existing `run_phases` and
  `composed_path` already read those fields. Measured order of the phases:
  a host's capture listeners run at phase 2 in the capture pass, before the
  root's capture listeners.
- **`relatedTarget`.** `PathItem` gets `related_target: Option<Gc<Object>>`;
  the struct is skipped where the retargeted target equals the retargeted
  related target (unless the event's own related target equals its target),
  as measured in the four `retarget-related` shapes; `event.relatedTarget`
  reads the current struct's value. `FocusEvent` and `MouseEvent` (scripts'
  and the engine's) both go through it.
- **Clear targets.** After the dispatch, `target` and `relatedTarget`
  become null when the last struct's adjusted target or related target is
  in a shadow tree (measured: `composed: false` → null; composed from
  inside → the host).
- **`composed`** exists on `Event`; the engine's UI events are composed
  (`EventInterface` already sets it for focus events; mouse, pointer,
  keyboard, input and wheel events are composed per UI Events); `slotchange`,
  `load`, `error` and `toggle` are not.
- **`window.event`** stays unset for listeners in shadow trees (already in
  `inner_invoke`).
- **Activation behavior.** The click's activation target search walks the
  path, which now goes from the shadow `span` through the host to the link:
  a click on a relative time inside a link navigates.

### 8. Focus and sequential navigation

- **`document.activeElement`** and **`ShadowRoot.activeElement`**: HTML's
  `DocumentOrShadowRoot` steps: retarget the focused element against the
  document or the root, and null for a root whose tree does not hold it
  (measured).
- **Focusing.** `focus()` of an element in a shadow tree works as for light
  elements (the "focusable" test changes from "node document is the main
  document" to "connected in the main document"). The engine's focused
  element stays the real element; the focus events fire at it and are
  retargeted by section 7 (measured: a move inside one tree is not seen
  outside it).
- **`:focus`** matches the focused element and every host whose shadow tree
  holds it (shadow-including ancestors that are hosts; measured);
  **`:focus-within`** matches the flat ancestors (measured: the slot);
  **`:focus-visible`** only the focused element.
- **`delegatesFocus`**: HTML "focus delegate" for `focus()` on the host
  (the `autofocus` element of the tree first, else the first focusable
  area in flat tree order; measured), a click on non-focusable content of
  such a host (the click target search falls back to the host's delegate),
  and `:focus` on the host. The host itself is not in the Tab order unless
  its `tabindex` says so (measured: `s8` but not `h5`).
- **The Tab order** (HTML "flattened tabindex-ordered focus navigation
  scope"): `next_in_order` keeps its interface (`from`, `backward`,
  `rendered`) and builds the sequence recursively, without recursion in
  Rust: the scope of the document, then of each scope owner (a host, a slot
  in a shadow tree) met in flat tree order; within a scope the positive
  `tabindex` groups come first, then the zero group, by flat tree order;
  an owner that is itself focusable comes right before its scope's
  sequence, else its scope's sequence stands in its place; an owner with a
  negative `tabindex` drops its scope (measured `h3`). The fast path: an arena
  without `ShadowState` takes today's code unchanged. Iframes stay as in ADR
  0031 (they are not scope owners in swb; unchanged).

### 9. Bindings

- **Wrappers.** `class_of` gives `SHADOW_ROOT` for `NodeData::ShadowRoot`.
  The root's wrapper is made on first use; a closed root's wrapper is still
  reachable from the code that attached it (the returned value) and is
  never exposed by `shadowRoot` or `assignedSlot`.
- **IDL.** `Element`: `attachShadow(ShadowRootInit)` (no `[CEReactions]`:
  it enqueues no reaction), `shadowRoot` (open roots),
  `slot` (`[Reflect]`, `[CEReactions]`), `assignedSlot` (the `Slottable`
  mixin on `Element` and `Text`), and `part` with 5c's `::part`.
  `ShadowRoot`: `mode`, `delegatesFocus`, `slotAssignment`, `clonable`,
  `serializable`, `host`, `onslotchange`, `innerHTML` (fragment parsing into
  the root, the existing `markup.rs` path with the root as context), and
  `DocumentOrShadowRoot`'s `activeElement`, `styleSheets`,
  `adoptedStyleSheets`, `elementFromPoint`, `elementsFromPoint`.
  `HTMLSlotElement`: `assignedNodes(options)`, `assignedElements(options)`,
  `assign(...nodes)`. `Node.getRootNode({composed})`. `Document.
  adoptedStyleSheets`. `CSSStyleSheet`: constructor, `replace`,
  `replaceSync`. The enums `ShadowRootMode`, `SlotAssignmentMode` and the
  dictionaries `ShadowRootInit`, `GetRootNodeOptions`,
  `AssignedNodesOptions` with Chromium's messages (measured).
- **Not in M10**: `getHTML`, `setHTML`, `setHTMLUnsafe`,
  `Document.parseHTMLUnsafe` (they come with declarative roots, section 10),
  `getSelection`, `getAnimations`, `fullscreenElement`,
  `pictureInPictureElement`, `pointerLockElement` (swb lacks them on
  `Document` as well), `customElementRegistry` (scoped registries, ADR 0035
  section 9).

### 10. Declarative shadow roots

- No fixture has one (grep above). html5ever 0.40 calls
  `TreeSink::allow_declarative_shadow_roots` and
  `attach_declarative_shadow` (public API); swb's sink would implement them
  with section 1's attach and the template's flags.
- With JavaScript off, Chromium attaches them (measured: the boxes of
  `declarative-js-off` equal those with JavaScript on), so pages that use
  them render their shadow content in Chromium and their light content in
  swb today.
- Recommendation: out of M10, a backlog item with the measured cases
  (`declarative-js-on`, `declarative-js-off`). No target needs them, and
  they bring `setHTMLUnsafe`, `parseHTMLUnsafe`, `getHTML` and the
  serialization of roots with them. Open question 7.

### 11. Scope and departures

In M10: open and closed roots; named and manual slots; `slotchange`; the
flat tree in style, layout, paint, hit testing, counters, selection; style
scoping with `:host`, `:host()`, `:host-context()`, `::slotted()`,
`::part()` (without `exportparts`, open question 4); constructable sheets
and `adoptedStyleSheets`; retargeting, `composedPath`, `relatedTarget`;
`activeElement`, `:focus`, `:focus-within`, `delegatesFocus`, the Tab order.

Out (backlog): declarative shadow roots and the HTML serialization of roots
(`getHTML`, `setHTMLUnsafe`, `parseHTMLUnsafe`); `exportparts`; scoped custom
element registries; `ElementInternals.shadowRoot`; `:has-slotted` (Chromium
148: invalid); `slotchange` timing relative to Chromium's other microtasks
beyond the measured cases; `@import` in constructed sheets (dropped, as
measured, but swb loads no `@import` anyway).

### 12. Budgets

Chromium has no limits here that the cases found (1,000 nested trees
render). swb's bounds:

| Resource | Limit | Result |
|---|---|---|
| Shadow roots | one node each: the arena's node limit (1,048,576) | `attachShadow` fails as every node creation past the limit does (the arena's existing error) |
| Nesting depth of shadow trees | none of its own; the flat depth meets the existing limits: the box tree flattens below 256 levels (`MAX_BOX_DEPTH`), the walks are iterative | content deeper than 256 boxes is flattened, as for deep light trees |
| Event path length | the flat ancestors (bounded by the node limit); one wrapper per struct, charged to the heap | heap limit |
| Slot assignment work (find a slot, assign slottables for a tree) | the task's time limit, charged per visited node through the arena's work counter (as the custom element sink, `take_overdue`) | termination of the task |
| Signal slots | one entry per slot (no duplicates) | |
| `slotchange` loops (a listener that changes the assignment) | the microtask checkpoint's time limit (as mutation observer loops) | termination |
| Scoped rule indexes | one per distinct list of sheets; their parsed data counts in the document's `StyleMemory` (128 MiB) | sheets past the budget are not parsed, one warning (ADR 0028 section 16) |
| Adopted sheets per document or root | 65,536 entries (Chromium: none) | the setter or trap throws `NotAllowedError` (`swb: too many adopted style sheets`), one warning |
| Constructed sheets | `StyleMemory` | as above |

Hostile cases (`tools/swbtools/hostile_cases/shadow_dom.py`, JavaScript on):
`shadow-nested-100000` (100,000 nested trees, each a host in the previous
root: style, layout, a click and Tab at the bottom), `shadow-hosts-200000`
(200,000 hosts with one text each), `shadow-slots-same-name-20000` (20,000
slots of one name in one root, 20,000 children, then each slot renamed),
`shadow-slotchange-loop` (a `slotchange` listener that moves a child to the
other slot each time), `shadow-assign-flood` (manual `assign()` of 100,000
nodes 100 times), `shadow-styles-100000` (100,000 roots, each with a
distinct `<style>` of 1 KB), `shadow-adopted-many` (one sheet adopted
65,537 times, and 10,000 sheets of 10 KB adopted by 10,000 roots),
`shadow-event-path-deep` (a composed event dispatched at the bottom of
10,000 nested trees, 1,000 times), `shadow-tab-scopes-100000` (100,000
hosts with one button each, then Tab 200 times), `shadow-transplant` (a
host with a large tree moved into an iframe's document and back 1,000
times).

### 13. Performance

The cost for documents without shadow roots must stay near zero: most
targets have none.

- **Fast path.** `Document::shadow` is `None` until the first
  `attachShadow` in the arena. `flat_children` and `flat_parent` then cost
  one branch more than `children` and `parent`; the assignment steps in
  the change functions one branch per change; `is_connected` one kind test
  of the root; `next_in_order` and `build_path` take today's code. With
  shadow roots, the per-element cost of the walks is a test of the
  `shadow_flags` byte; hash lookups happen only at hosts and slots.
- **Measure** (base: the commit before each sub-feature; release builds):
  - `just snapshot` before and after each sub-feature: no change with
    JavaScript off and on for the targets other than GitHub.
  - `swbtools perf` on the JS-off targets (stage times: style, layout):
    within the run-to-run noise; then callgrind totals of `swb --headless`
    for Wikipedia, Ars Technica and BBC with JavaScript off (the style and
    layout passes dominate): budget +0.3 %.
  - The binding rows of ADR 0035 section 11 (callgrind, instructions per
    iteration; `appendChild` with `remove`, `textContent`, `innerHTML`,
    `setAttribute`): +1 % without shadow roots (the change functions' new
    branch).
  - GitHub with JavaScript on: style and layout time per pass with 24
    roots, and the `load` time, which goes to feature 8's budget (1.56 s).
  - New rows with shadow roots (reported, not a gate): `attachShadow` +
    `append` of a `span` (the `relative-time` pattern) per iteration; a
    style pass over 1,000 hosts with a slot each.

### 14. Tests

- Probe cases: `tools/probes/web-shadow-dom.json` runs in Rust against
  Chromium's values (`crates/engine/tests/probe_net.rs`, a generated table as
  for custom elements, with departures and "later" rows per sub-feature);
  the cases with `steps` (`tab-order*`, `ui-events-through-shadow`) run in
  `crates/automation/tests/real_input.rs`'s way with the input steps.
  `tools/probes/shadow-dom.json` with `just probe --with-swb`, and its boxes
  as layout tests (`crates/engine/tests/`, like `flex_intrinsic.rs`).
- Unit tests: `swb_dom` (attach checks, shadow-including order, the flat
  children and parent of hosts, slots, fallback, nested slots, assignment at
  each change, signal slots without duplicates, `is_connected` through
  hosts, clones and adoption of hosts, transplant with roots, the
  `ElementData` size); `swb_css` (parsing `::slotted()`, `::part()`,
  `:host*` in and out of compounds, specificity); `swb_style` (context
  ranks for normal and important, `:host` matching only from its own
  root, `::slotted` only for assigned elements, layers per scope);
  `crates/web` (retargeting, `relatedTarget`, clear targets, the
  `ObservableArray` traps); `crates/engine` (Tab order scopes, hit test
  through a host to a link).
- The GitHub test (`crates/automation/tests/github.rs`): the callback counts
  of ADR 0035 reach the replay's 35 and 76; the 23 `relative-time` roots
  exist and their texts equal the replay's list; `tool-tip` has a root with
  one `<style>` and a slot.
- Hostile cases of section 12.

### The plan of sub-features

Four sessions, each with a review and a commit. The roadmap's split (5a DOM
and flat tree, 5b styles, 5c events and focus) is changed: the DOM part
alone (node kind, attach, slots, shadow-including walks in four crates,
custom elements, bindings) fills one session, and the flat tree in style,
layout and hit testing is a second piece with its own performance risk.
GitHub's `relative-time` renders after 5b.

**5a. Shadow trees and slots in the DOM** (`implementer-hard`).
Scope: sections 1 to 3 and 9 without the style and event parts:
`NodeData::ShadowRoot`, `ShadowState`, the flag byte, `attachShadow` with all
checks and messages (also the disable-shadow flag and the upgrade failure),
connectedness through hosts, host-including ancestors, adoption, clones of
clonable roots, transplant; named and manual slots with eager assignment,
signal slots and `slotchange` (dispatched with the plain path: the slot's
ancestors up to the root, which is what `composed: false` gives); the
shadow-including walks of section 3 (custom elements, sheet owners, images,
frames, forms); the bindings (`ShadowRoot` members without
`adoptedStyleSheets` and the `activeElement` retargeting, `HTMLSlotElement`,
`Element.attachShadow`, `shadowRoot`, `slot`, `assignedSlot`,
`getRootNode({composed})`). Shadow-tree style sheets are collected but not
applied. Until 5d, an event from a shadow node stops at its root (today's
path of DOM ancestors).
Files: `crates/dom/src/tree.rs`, `mutation.rs`, `algorithms.rs`,
`transplant.rs`, new `shadow.rs` and `slots.rs`; `crates/web/idl/dom.idl`,
`html.idl`; `crates/web/src/state.rs`, `dom/mod.rs`, `dom/markup.rs` (root
`innerHTML`), new `dom/shadow.rs`, `html/slot.rs`; `custom_elements/
upgrade.rs`, `sink.rs`, `registry.rs`; `crates/engine/src/document/
changes.rs`, `loads.rs`, `frames.rs`, `forms/`.
Tests: probe cases `api-surface` (DOM rows), `attach-errors`,
`root-properties`, `slot-assignment-*`, `slotchange-timing` (all but the
nested case's flat path), `mutation-observers`, `custom-elements-in-shadow`;
unit tests; hostile cases `shadow-hosts-200000`,
`shadow-slots-same-name-20000`, `shadow-slotchange-loop`,
`shadow-assign-flood`, `shadow-transplant`.
Exit: the cases match Chromium; GitHub's callback counts reach 35 and 76
and its 24 roots have the replay's content (`--eval`); no target changes
(JS on and off); the binding rows within +1 %; `just check`.
Risk: high (a new node kind in every match on `NodeData`; eager assignment
in the change functions; the walks in four crates).

**5b. The flat tree in rendering** (`implementer-hard`).
Scope: section 4: `flat.rs`, the style walk with inheritance and the Bloom
filter, `compute_element_styles`, counters, the box tree (also tables and
the depth flattening), hit testing (`element_of`, links, the cursor),
`elementFromPoint` retargeting, the `:hover`, `:active` and `:focus-within`
chains, `innerText` (not the engine's selection: backlog); the
performance measurements of section 13.
Files: new `crates/dom/src/flat.rs`; `crates/style/src/cascade.rs`,
`counters.rs`, `element.rs`, `bloom.rs`; `crates/layout/src/box_tree.rs`,
`table/build.rs`; `crates/engine/src/hit_test.rs`, `document/queries.rs`,
`document/geometry.rs`, `document/resolved.rs`, `document/inner_text.rs`,
`document/pipeline.rs`; `crates/web/src/dom/document.rs`
(`elementFromPoint`).
Tests: `tools/probes/shadow-dom.json` (all boxes but the declarative cases);
the probe cases `render-reads`, `nesting-depth`, the `:hover` row of
`ui-events-through-shadow`; unit tests of `flat.rs`; hostile case
`shadow-nested-100000`; `just snapshot` and the performance rows.
Exit: the boxes match Chromium; GitHub's relative times render with
Chromium's text (the integration test and the compare scores with
JavaScript on, which must not get worse); the JS-off targets unchanged and
within the performance budget; `just check`.
Risk: high (every style and layout pass; inheritance; the cost of the fast
path).

**5c. Style scoping and constructable sheets** (`implementer-hard`).
Scope: sections 5 and 6: tree scopes of sheets, scoped rule indexes,
`:host`, `:host()`, `:host-context()`, `::slotted()`, `::part()`, the context
step of the cascade, layers per scope, the `CSSStyleSheet` constructor,
`replace`, `replaceSync`, `adoptedStyleSheets` with the observable array
(document and roots), the budgets.
Files: `crates/css/src/selector/` (parse, matching, specificity);
`crates/style/src/stylist.rs`, `cascade.rs`, `layers.rs`, `element.rs`,
`cssom/sheet.rs`; `crates/engine/src/resources.rs`, `document/changes.rs`,
`document/pipeline.rs`, `document/cssom.rs`; `crates/web/idl/cssom.idl`,
`crates/web/src/cssom_sheets.rs`, new `adopted.rs`.
Tests: probe cases `style-scoping`, `style-specificity-and-order`,
`constructable-sheets`, `api-surface` (CSS rows); unit tests; hostile cases
`shadow-styles-100000`, `shadow-adopted-many`; the `tool-tip` styles with a
case that opens a copy of it (popover from feature 6 is not needed: a copy
without `popover`).
Exit: the cases match Chromium; GitHub's popover polyfill puts its sheet into
the 24 roots without leaking (the scores with JavaScript on); the targets
unchanged; `just check`.
Risk: medium-high (the cascade order; the selector model gets functional
pseudo-elements).

**5d. Events and focus** (`implementer`).
Scope: sections 7 and 8: "get the parent", retargeting, `relatedTarget` per
struct, clear targets, activation through hosts; `activeElement` of the
document and of roots; `:focus` on hosts; `delegatesFocus`; the Tab order
with scopes.
Files: `crates/web/src/events/dispatch.rs`, `events/mod.rs`, `events/user.rs`,
`html/focus.rs`, `html/activation.rs`; `crates/engine/src/focus.rs`,
`page/frame_focus.rs`, `crates/style/src/element.rs` (`:focus` on hosts).
Tests: probe cases `composed-path-*`, `retarget-related`, `focus-shadow`,
`tab-order*`, `ui-events-through-shadow`, the nested row of
`slotchange-timing`; hostile cases `shadow-event-path-deep`,
`shadow-tab-scopes-100000`.
Exit: the cases match Chromium; a click on a relative time inside a link
navigates; GitHub's Tab order unchanged (no focusable shadow content);
`just check`.
Risk: medium (the dispatch is central, but its data model was made for
this).

Feature 5's exit after 5d: GitHub's `relative-time` texts and `tool-tip`
roots as in Chromium's replay; `tool-tip` renders nothing while closed, as in
Chromium; the targets keep their scores with JavaScript on and off.

## Open questions

The orchestrator decided them on 2026-11-09 (see Consequences): all as
recommended except 8.

1. **Four sub-features instead of three.** Recommendation: four (5a DOM, 5b
   flat tree, 5c styles, 5d events and focus), for the session size and to
   measure the flat tree's cost alone. GitHub's relative times render after
   5b.
2. **Where the host's root and the slot data live.** Recommendation: the
   `shadow_flags` byte in `ElementData`'s padding and a per-arena
   `ShadowState` with maps (section 1). Alternatives: a `NodeId` field in
   `ElementData` (measured: `Node` from 104 to 112 bytes for every node), or
   `parser_form` moved to a side table to make room.
3. **Eager slot assignment.** Recommendation: eager, in the change
   functions, behind the arena's `ShadowState` test, as the DOM Standard
   says; it gives `slotchange` signals and the stored flattened lists for
   the flat tree. Alternative: a lazy cache per root with a dirty flag
   (cheaper for bulk insertions into hosts, but the signals need the old
   and new lists anyway).
4. **`::part()` and `exportparts`.** GitHub styles no part; Chromium supports
   both (measured). Recommendation: `::part()` in 5c (one more crossing rule
   set, like `::slotted`), `exportparts` to the backlog.
5. **`:host-context()`.** Chromium 148 supports it (measured); the CSS
   Scoping draft marks it at risk and other engines lack it.
   Recommendation: implement it (a walk of the host's flat ancestors), since
   Chromium is the reference.
6. **`adoptedStyleSheets` as an observable array.** Recommendation: a
   `Proxy` over a real array with native traps (section 6): `push` and index
   writes work as in Chromium (measured). Alternative: an array that the
   setter copies (a `FrozenArray`-like snapshot), where `push` has no effect;
   simpler, and enough for GitHub (it only assigns), but a silent
   difference for pages that push.
7. **Declarative shadow roots.** Recommendation: backlog (section 10); no
   target has them, and they bring the HTML serialization APIs.
8. **Selection through shadow trees.** Recommendation: the engine's
   selection and copy in flat tree order (section 4), in 5b. Alternative:
   leave shadow text out of selections (smaller; a user cannot copy "4 days
   ago").
9. **The polyfills that swb runs and Chromium does not** (`command` until a
   later feature, `popover` until feature 6) observe and style every shadow
   root. Recommendation: accept them as the test of 5a and 5c (they use
   observers, listeners and adopted sheets on roots); add `CommandEvent` and
   `commandfor` to the backlog, so that the `command` polyfill stops running.
10. **Budgets of section 12.** Recommendation: accept (65,536 adopted sheets
    per document or root; the rest reuses existing limits).

## Consequences

- `attachShadow`, `ShadowRoot`, slots, `slotchange`, `adoptedStyleSheets`
  and the `CSSStyleSheet` constructor appear for every page with JavaScript
  on; pages that tested for them take other paths (BBC's video player,
  Wikipedia's Vue custom elements, the polyfills of GitHub).
- `swb_dom` gets a node kind and a per-arena shadow state; `is_connected`,
  the host-including ancestor check, adoption, clones and transplants
  follow hosts.
- Style, layout, hit testing, counters, selection and the Tab order walk
  the flat tree; inheritance follows it. Documents without shadow roots
  take the old paths behind one test.
- The cascade gets the context step and one layer order per tree scope;
  `swb_css` gets functional pseudo-elements (`::slotted()`, `::part()`) and
  matching for `:host*`.
- The event path follows "get the parent" with retargeting and per-struct
  related targets; ADR 0009's interaction targets (hover, click, focus)
  are elements of shadow trees where the hit is inside one.
- The diagnosis's "`adoptedStyleSheets` is set once on the document" was the
  tools' own sheet (`stop_animations`); GitHub's page uses no
  `adoptedStyleSheets` in Chromium.
- Backlog: declarative shadow roots and `getHTML`, `setHTMLUnsafe`,
  `parseHTMLUnsafe`; `exportparts`; `CommandEvent` and `commandfor` (GitHub's
  `command` polyfill); `ElementInternals.shadowRoot` (with ADR 0035's
  `ElementInternals` item); the engine's selection and copy in flat tree
  order.
- Orchestrator decisions on the open questions (2026-11-09), after
  re-measuring six JavaScript cases (`style-scoping`, `slotchange-timing`,
  `focus-shadow`, `tab-order`, `retarget-related`, `constructable-sheets`)
  and the 16 layout cases in Chromium 148 (all equal to the values above):
  - Four sub-features, 5a, 5b, 5c, 5d, in this order.
  - The flag byte and the per-arena `ShadowState`; eager slot assignment.
  - `::part()` in 5c, `exportparts` to the backlog; `:host-context()` is
    implemented.
  - `adoptedStyleSheets` as a `Proxy` over a real array.
  - Declarative shadow roots to the backlog.
  - Question 8 changed: the engine's selection and copy through shadow
    trees go to the backlog, not into 5b. The owner's list of what must
    work on GitHub does not need them, and 5b carries the riskiest part,
    the flat tree in every style and layout pass. Until then the engine's
    selection keeps its DOM order and leaves shadow text out.
  - The polyfills that swb runs and Chromium skips stay; `CommandEvent`
    and `commandfor` go to the backlog.
  - The budgets of section 12 are accepted.

## 5a implementation notes

Facts of sub-feature 5a (shadow trees and slots in the DOM) that differ from
the decision above or that 5b to 5d need. Code: `crates/dom/src/shadow.rs`,
`slots.rs` (new), `tree.rs`, `mutation.rs`, `algorithms.rs`, `transplant.rs`,
`id_index.rs`; `crates/web/src/dom/shadow.rs`, `html/slot.rs` (new),
`mutation_observer.rs`, `custom_elements/`, `cssom_sheets.rs`, `dom/adopt.rs`;
`crates/engine/src/document/changes.rs`, `resources.rs`, `forms/`,
`frames/`, `page/frames.rs`; the generator (`[SwbNullIsInvalid]`).

**The node and the state (section 1).** `NodeData::ShadowRoot(ShadowRootData)`
(host, mode, slot assignment, the four flags); `NodeData::is_fragment()` is
true for it and for `DocumentFragment`. `ElementData::shadow_flags` (`HOST`,
`SLOT`) sits in the padding: the size stays 64 bytes (the test stays).
`Document::shadow: Option<Box<ShadowState>>` holds the roots by host, the
slots of each root by name in tree order (`RootSlots`), the assigned nodes of
each slot, the assigned slot of each node, the manual lists in both
directions, the signal slots, a work count, the flag of an attach to a
connected host (`take_shadow_changed`: the drain restyles, as with
`take_defined_changed`) and the attention cell; the `Document` itself got
only the one field (a temporary arena of each fragment parse is made and
dropped: each field costs there, measured in the `innerHTML` row).
`is_connected` goes through hosts; the old meaning "the root is a document"
is `in_document_tree`, which the id index uses (the document's
`getElementById` does not see shadow nodes; `get_element_by_id` with a
shadow root walks its tree).

**The host check (section 1, measured).** Chromium accepts the 18 names of
the DOM's list in any namespace (an SVG, MathML or namespace-less `div`, an
XLink `span`) and a valid custom element name only in the HTML namespace
(case `attach-namespaces`, added in 5a); the standard requires the HTML
namespace for both. swb follows Chromium. The disable-shadow flag is the one
of the definition that the registry of the host's node document has for the
host's local name (`custom_elements::shadow_disabled`; measured for a
`createElement` of a defined name).

**Slot assignment (section 2).** The steps run in `link` and `unlink_with`
(insertion and removal, also of the children of a fragment) and after the
journal entry of an attribute change, when the arena has a `ShadowState`.
DOM "assign slottables for a tree" visits every slot of a root; swb works on
the names that a change touches: a root keeps the first slot of each name at
its last computation (`RootSlots::first`), and a change of the slots changes
an assignment only when that first slot changes. An append to a host extends
the list in place and a removal from a host takes the node out of it, so that
building a host's children stays linear. `assign()` takes each node out of
its earlier slot's list in one pass per earlier slot (the hostile case
`shadow-assign-flood`). Two measured departures from the DOM's text: a
manually assigned node is assigned when it becomes a child of the host (DOM
assigns a slot at an insertion only for named roots), and a slot that leaves
its tree is signalled before the tree's slots whose lists it changed (DOM
runs "assign slottables for a tree" of the tree first; case
`slotchange-order`). The flattened lists are computed from the stored lists
when asked (`flattened_assigned_nodes`), not stored: 5b's `flat_children`
needs them per slot and can keep them with the assigned list then.

**`slotchange`.** The arena's attention cell is the runtime's signal cell
(`set_shadow_attention`), installed at `attachShadow`, at `assign()` and
when a transplant brings roots into an arena. `mutation_observer::
after_request` then queues the mutation observer microtask if the current
arena has signal slots, and checks the time limit when the shadow steps
visited 4,096 nodes (`take_shadow_work`). `deliver` takes the signal slots of
every lent document before the observers' callbacks and fires `slotchange`
after them (bubbles, not composed, not cancelable, trusted), with the path of
DOM ancestors up to the root until 5d. A cleared job queue sets the signal
again.

**Walks (section 3).** Shadow-including: the upgrade candidates of `define`,
`customElements.upgrade(root)`, the sink's walks of inserted, removed and
adopted subtrees, adoption's node document, the drain's `rediscover` and
`collect_subtree`, image source selection, the frames of `iframe` elements,
the form controls (the radio group key has the tree; a `form` attribute is
looked up in the control's tree), the entry list of a submission. An upgrade
of an element that has a root with a definition that has the disable-shadow
flag fails before the constructor with the measured report.

**Sheets.** `SheetSlot::scope` is the shadow root of the owner's tree (else
the document node). The stylist skips the other scopes, `document.styleSheets`
lists the document's scope only, and `ShadowRoot.styleSheets` lists the
root's (`EngineHost::shadow_style_sheet_count`, `shadow_style_sheet`). The
list of sheets is kept in shadow-including tree order.

**Frames (measured).** An `iframe` in a connected shadow tree gets a frame
with its document, but it is no "document-tree child navigable":
`window.length`, `window[i]` and the named access skip it
(`Frame::in_shadow_tree`, set when the frame is made; a container that moves
is removed first, which destroys the frame). Case `walks-in-shadow`.

**Bindings (section 9).** As planned, without `adoptedStyleSheets` (5c) and
`elementFromPoint` (5b); `ShadowRoot.activeElement` retargets the focused
element against the root (DOM "retarget", `dom/shadow.rs`; 5d does
`document.activeElement`). The root's `innerHTML` is parsed with the host as
context. `attachShadow(null)` needs the new dictionary attribute
`[SwbNullIsInvalid]` (Chromium: "The provided value is not of type
'ShadowRootInit'."; Web IDL would convert `null` to an empty dictionary).

**Tests.** The probe file has 25 cases and 137 queries (5a added
`attach-namespaces`, `walks-in-shadow`, `slotchange-order` and, in the fix
round, `forms-in-shadow`, each with a control query; Chromium 148, runs 1 and 3 equal, run 2 differs only in
`focus-shadow` #1, a 5d case whose focus events depend on an earlier query).
64 queries match, 4 are departures (`api-surface` 1, 3, 4: members that 5b,
5c or the backlog add; `attach-namespaces` 1: no `MathMLElement`), 69 are
later (13 cases of 5b, 5c, 5d and declarative roots; `api-surface` 5 and 6,
`root-properties` 6 and 10, `slotchange-timing` 6). Mutation checks: the
host-including fast path without the host bit, the signal order of a
removal, and the plain walks of `define` and of the sink each fail a test.
GitHub (`crates/automation/tests/github.rs`): the callbacks reach the
replay's 35 and 76 (12 and 7 from upgrades, 23 and 69 from the sink for the
`relative-time` elements), the page has 24 hosts, the texts of the 23
`relative-time` roots equal the replay's list at the page clock, and
`tool-tip`'s root has its `:host` style (and the popover polyfill's) and a
default slot that gets "Dismiss alert". Hostile cases (release build):
`shadow-hosts-200000` 1.5 s, 590 MiB; `shadow-slots-same-name-20000` ends at
the time limit of 5 s (each rename moves 20,000 nodes); `shadow-slotchange-loop`
ends at the time limit (10.5 s after the fix round, see below);
`shadow-assign-flood` 2.1 s; `shadow-transplant` 0.6 s
with a tree of 200 nodes (1,000 round trips; a tree of 2,000 nodes reaches the
node limit, because a transplant leaves the taken nodes in their arena, as
for a light tree). The whole set after the fix round: 449 cases, 447 pass; `nest-div-100000` is the known failure, and `large-message-to-a-frame-in-a-loop-js-on` is inactive (it is also inactive with the 5cddedc and 5a builds, so it does not come from this work).

**Fix round (review of 5a).**
(1) The form code had its own `in_document` (the top of the ancestors is the
document), so a control in a connected shadow tree counted as detached: a
script write to `checked` made a seed with stamp 0, and `r2.checked = true`
left `r1.checked` true until the next rebuild. It is now
`forms::in_window_document` (the shadow-including root is `NodeId::DOCUMENT`;
the name differs from `changes::in_document`, which says the same). Case
`forms-in-shadow` (radio groups with and without a `form` owner, a select,
validity, a checkbox and a text input in an open root, and the light-tree
control) equals Chromium.
(2) `index_slot` kept the slots of a name in a `Vec` and compared a new slot
with the siblings by a forward scan, so prepending slots to a root was
quadratic (20,000 slots: over 5 s). `RootSlots::by_name` holds a `VecDeque`;
a slot is tried against both ends with a bounded two-way sibling scan
(`Document::precedes_within`, 8 steps) before the binary search, and every
step is counted as work. Slots in the middle cost a binary search of
scans, counted.
(3) `shadow_inserted` and `shadow_removed` walked the whole moved subtree for
slots in every arena with shadow state. `slots_below` walks at most 32 nodes
first; only a larger subtree makes it look for the root of the parent (the
walk up is counted), and the full walk follows only if that root is a shadow
root. A chain of 30,000 divs built bottom-up after one `attachShadow` took
1.9 s (2.6 s in the hostile case); it takes 0.65 s.
(4) Removing assigned nodes: the lists are `AssignedList` (a vector and a
start offset), the search goes from both ends, and a removal at the front
moves the offset; the moved entries are counted (16 per step). 100,000
children removed from the end or the front take 0.7 s in total (2.1 s).
(5) `deliver` gives the signal slots back to their documents
(`Document::restore_slot_signals`) when a termination stops the observers or
a `slotchange` listener, the interrupted slot included, as the observers'
records are restored. A loop that signals again at each `slotchange` thus
runs two tasks into the time limit before the scripts of the document stop
(`shadow-slotchange-loop` 10.5 s, was 5.6 s; the case limit is 20 s).
(6) `forget_shadow_nodes` visits the moved nodes and the lists they are in,
not every map (20,000 hosts and 10,000 adoptions of a one-node element:
5.7 s at the time limit before, 0.8 s).
(7) A unit test checks `assignedSlot` (`find_slot` with `open`) of a closed
root directly.
Each new test fails when its fix is removed (mutation checks: the old form
check, no end test in `index_slot`, no bound on the slot walk, removal at the
front without the offset, no restore in either path of `deliver`, no cleanup
of the manual lists, no closed-root test).
New hostile cases: `shadow-slots-prepend-20000` (before: 5.6 s, the time
limit; after: 0.6 s), `shadow-detached-chain-30000` (2.6 s; 0.65 s),
`shadow-assigned-remove-100000` (2.1 s; 0.7 s) and
`shadow-adopt-many-hosts` (5.7 s, the time limit; 0.8 s).

**GitHub's console** (`swb --headless --js --replay fixtures/js/github-swb
--now <capture time> --settle-ms 1500`): the base had 47 lines, 46 failed
custom element constructors and one callback, all "Cannot read properties
of undefined (reading 'call')" from the polyfills that wrap the missing
`attachShadow`; 5a has none.

**"In the document" in the engine.** `Document::is_connected` is the DOM's
(any document node of the arena at the top of the shadow-including tree);
the engine's checks of the window's document (`in_document` of the drain,
the frames' `in_frame_tree`, `frames::connected` of the bindings) compare the
shadow-including root with `NodeId::DOCUMENT`, so a `createHTMLDocument`
document still gets no sheets or frames (the first version used
`is_connected` there; `tests/cssom_sheets.rs` caught it).

**Performance (section 13).** The binding rows of ADR 0035 section 11 under
callgrind (`swb --headless --js`, instructions per iteration as the
difference of 40,000 and 20,000 iterations; release builds; base 5cddedc):

| Binding | base | 5a | 5a / base |
|---|---|---|---|
| `x = i` (the loop) | 644 | 632 | 0.981 |
| `e.setAttribute('a', i)` | 4,408 | 4,411 | 1.001 |
| `e.id = i` | 3,887 | 3,897 | 1.003 |
| `t.appendChild(n); n.remove()` | 4,400 | 4,372 | 0.994 |
| `e.textContent = 'x'` | 1,945 | 1,941 | 0.998 |
| `e.innerHTML = '<b></b>'` | 17,249 | 17,382 | 1.008 |
| `document.createElement('div')` | 4,221 | 4,232 | 1.003 |
| `e.classList.add('c')` | 8,400 | 8,433 | 1.004 |

(Measured after the fix round.) The loop row has no binding in it, so its
ratio shows the noise between builds. The first version had `innerHTML` at 1.011: three new fields of
`Document` cost at the drop of the temporary arena of each fragment parse,
`push_node` noted HTML `slot` elements (a flag that limited the subtree walks
to arenas that made a slot), and the drain looked up a shadow root for every
visited node; the fields moved into `ShadowState`, the flag went (the walks
run only in arenas with shadow state anyway), and the drain tests
`has_shadow_state` first. With shadow roots (not a gate): `div.attachShadow`
plus `append` of a `span` per iteration (the `relative-time` pattern) costs
22,191 instructions.

**The targets** (compare with JavaScript on, all 8 fixtures of `fixtures/js`,
and off, `fixtures/pages`, against a release build of 5cddedc): every score
is the base's (GitHub JS on geometry 0.8904, pixels 0.9884: the shadow
content renders with 5b; Ars Technica's pixels 0.9901 in both); the JS-off
snapshot is byte-identical. Only GitHub attaches shadow roots after the load
(24 open roots; the other targets' scripts test for `attachShadow` and
attach none, counted with `--eval`). The warnings of the logs are the base's
apart from GitHub, which lost its 24 (the failed `relative-time`
constructors).

## 5b implementation notes

Facts of sub-feature 5b (the flat tree in rendering) that differ from the
decision above or that 5c and 5d need. Code: `crates/dom/src/flat.rs` (new),
`shadow.rs` (`retarget`), `slots.rs`; `crates/style/src/cascade.rs`,
`stylist.rs`, `counters.rs`, `element.rs`; `crates/layout/src/box_tree.rs`,
`table/build.rs`; `crates/engine/src/hit_test.rs`, `scrollers.rs`,
`page/events.rs`, `page/forms.rs`, `document/queries.rs`, `geometry.rs`,
`resolved.rs`, `inner_text.rs`, `changes.rs`; `crates/web/src/cssom_view.rs`,
`dom/shadow.rs`, `idl/cssom-view.idl`.

**The flat-tree model (section 4 is wrong on one point).** CSS Scoping §3.3
builds the flat tree with "find slottables", that is with each slot's
*assigned* nodes, not with its flattened list: a slot that is assigned to a
slot of a nested host stays in the flat tree, as a child of that slot, and
its own assigned nodes are its flat children. Section 4's "the innermost
slot whose flattened list holds n" would skip it. Measured in Chromium 148
(case `nested-slot-inherit` of `tools/probes/shadow-dom.json`, three equal
runs): the outer slot, assigned onward, has `font-size: 24px` and the inner
slot none; the light `b` has `24px` and a 27 px high box, so it inherits from
the outer slot. swb follows the specification and Chromium: `flat_children`
of a slot with assigned nodes is its stored assigned list (`Listed`), and
`flat_parent` of a slotted node is its assigned slot. Each step is O(1), so a
chain of N pass-through slots costs O(N) in a style pass
(`shadow-slot-passthrough-10000`). The flattened lists are not used in
rendering.

**`getComputedStyle` outside the flat tree (differs from section 4's table).**
Measured (cases `unslotted-computed-style` and
`unused-fallback-computed-style`): a light child of a host that no slot takes
has computed values, but they inherit from nothing: `font-size: 16px`, black,
`display: inline` (not blockified) under a host with `20px` and green; its
child inherits from it; unused fallback content of a slot the same. So
`compute_element_styles` walks the flat ancestors and stops at the first node
without a flat parent, which inherits from the initial style; it is not the
DOM chain. A node that is not in the window's document still has no style.

**Style.** `Styler::run` walks `flat_element_children`; inheritance and the
`layout_parent` follow it; the Bloom filter holds the flat ancestors (a
superset of the DOM ancestors in the element's own tree, so it never rejects
a rule wrongly). Each stack entry carries the element's tree scope (the
document node or a shadow root): a host's children get its root, a slot's
assigned nodes get the scope of the slot's host (recorded when the walk
entered the host; `swb_dom::FlatScopes`, which `getComputedStyle` and
`innerText` also use). The scope test of 5b: the document's author rules match
only elements of the document's scope (`matching_rules(.., author)`, one bool
per element; measured: document rules do not match shadow nodes). A shadow
element gets the user-agent rules, presentational hints, its `style`
attribute and inheritance; sheets in shadow trees stay unapplied until 5c.
Counters and list numbering walk the flat tree (CSS Lists). The `:hover`,
`:active` and `:focus-within` chains are the flat ancestors (measured: the
host and the slot match `:hover`).

**Layout.** `push_children`, `push_flattened` (below `MAX_BOX_DEPTH`) and the
table builder's `for_each_child` take `flat_children`; a text node's style is
its flat parent's (the slot's for slotted text). Light children outside the
flat tree get no box. The other walks of `crates/layout` and `crates/style`
stay DOM walks on purpose: SVG content, `source` children of media, the root
and body of background propagation, `details` and `colgroup` children (they
cannot be hosts or slots), selector matching within a scope, the disabled and
editing states, table hints, SVG `use`, `querySelector`.

**Hit testing and the interim event target.** `element_of` takes the flat
parent element of a text node (hover, cursor, `elementsFromPoint`);
`link_around` and the scroll chain (`Scrollers::chain`, used by wheel,
keys and `scrollIntoView`) walk flat ancestors; `offsetParent` walks flat
ancestors and is retargeted against the element (measured: the light `body`
for a shadow `span`). `document.elementFromPoint` and `elementsFromPoint`
retarget each element against the document, and the new
`ShadowRoot.elementFromPoint` and `elementsFromPoint` against the root
(measured, case `hit-test-through-host`: the document gets `h, l, p, body,
html`, the root `in, h, l, p, body, html`). After the retargeting, an
element that repeats the one before it is dropped, but a later repeat stays
(the review found this; case `hit-test-retarget-repeats`: a slotted light
element between two shadow boxes of host `h` gives the document `h, x, h,
html`). Interim
rule until 5d builds the composed path: the pointer, mouse, wheel and click
events of a hit in a shadow tree go to the hit element retargeted against the
document (the outermost host, which is Chromium's `target` outside the tree,
measured in `ui-events-through-shadow`), and the engine's activation walk
(input without scripts) starts there too, so a click on a relative time in a
link follows the link and reaches the document's and React's listeners. The
`:hover` and `:active` chains keep the real hit node. Focus is unchanged.
Known gap until 5d: a link or a button inside a shadow tree shows as the
hovered link, but a click on it goes to the host and does not activate it.

**`innerText`.** The walk follows the flat tree with a tree scope per step;
only the text of the asked element's tree gives characters, the shadow text
between changes the white-space state as layout does, and a collapsible
space of the light text before shadow text stays. Measured (`render-reads`):
the `innerText` of `<p>by <relative-time>..</relative-time> end</p>` is
`"by  end"` (two spaces), a host's is `""` when no slot takes its children,
a shadow element's is its text.

**Invalidation.** Every change of a connected slot's assigned list
(`set_assigned`: insertions, removals, `slot` and `name` changes, `assign()`)
sets the flag of `take_shadow_changed` and bumps the arena's generation, and
the engine's drain invalidates all styles (`assign()` makes no journal entry
and changed no generation before).

**Departures.** `nesting-depth` #1: 1,000 nested hosts exceed the 256 box
levels (`MAX_BOX_DEPTH`, section 12); the content below is flattened into
text, so the deepest `div` has no box (Chromium: left 1007, a box). The
probe case `counter-through-slot` was changed before it became a test: it
used `width: max-content` on blocks (not supported in swb, roadmap backlog)
and digits of equal width; it now uses `display: table` and
`counter-reset: n 9` (three equal Chromium runs).

**Tests.** `crates/engine/tests/shadow_rendering.rs` runs every case of
`tools/probes/shadow-dom.json` (the boxes within 0.5 px, the `js` rows) but
the four that need shadow-tree `<style>` (5c) and the two declarative cases,
with Chromium's values in `tests/data/shadow-dom.json`, plus engine tests (a
click on shadow text in a link, with and without scripts; the cursor of a
slot; `assign()` lays out again; scroll containers of shadow boxes).
`probe_net.rs`: `render-reads` matches, `nesting-depth` matches but the
departure above; the `:hover` row of `ui-events-through-shadow` runs with its
steps in `crates/automation/tests/real_input.rs`. Unit tests in `flat.rs`,
`cascade.rs` (`shadow_tests`), `box_tree.rs`. The GitHub test reads the text
fragments in the 23 `relative-time` hosts: 22 show the replay's text; the
first is in a `span.d-sm-none`, which the page hides at this width (as in
Chromium); no light date text has a fragment.

**Two quadratic costs that the hostile cases found.** (1) `attach_shadow`
asked `is_connected` of the host (a walk through every host above it) to set
the restyle flag, so building 100,000 nested trees was quadratic (5a code:
10,000 levels 0.22 s, 40,000 levels 3.4 s). The flag now uses
`may_be_connected` (the roots of the node's tree and of its host's tree; a
deeper nesting answers true, which costs at most one restyle), also for the
slot changes above: 40,000 levels 0.04 s. (2) The `:hover`, `:active` and
`:focus-within` chains were searched linearly for each element; with the
flat ancestors of a hit 100,000 trees deep that was quadratic in the style
pass. The chains are sorted and searched by bisection.

**Hostile cases** (release build; base 2808bc1 / 5b): `shadow-nested-100000`
(new: 100,000 nested trees, a link at the bottom, a click and a Tab) base
inactive (the script ran into its time limit), 5b 1.36 s, 551 MiB;
`shadow-slot-passthrough-10000` (new) 1.76 s, 97 MiB / 1.61 s, 179 MiB;
`shadow-hosts-200000` 1.46 s, 587 MiB / 1.91 s, 809 MiB (the 200,000 shadow
texts now have boxes); the other seven 5a cases within 0.05 s and 5 MiB of
the base.

**Performance (section 13).** JavaScript off: `just snapshot` is
byte-identical to the base; callgrind totals of `swb --headless`: Wikipedia
646.81 M / 647.85 M instructions (+0.16 %), Ars Technica 1,968.52 M /
1,968.39 M (−0.01 %), BBC 7,329.23 M / 7,329.60 M (+0.01 %); `swbtools perf`
stage times within the run-to-run noise (two runs each). The binding rows of
ADR 0035 section 11 (instructions per iteration, base / 5b): loop 650 / 649,
`setAttribute` 4,407 / 4,396, `id` 3,889 / 3,893, `appendChild` with `remove`
4,380 / 4,399 (1.004), `textContent` 1,959 / 1,941, `innerHTML` 17,378 /
17,398, `createElement` 4,237 / 4,231, `classList.add` 8,420 / 8,422; the
`relative-time` pattern 22,142. GitHub with JavaScript on (`--bench 20`,
three runs): style 24.9 / 25.4 ms per pass, layout 4.93 / 5.05 ms, the
headless load 0.48 s in both (budget 1.56 s). A pass over 1,000 hosts with a
slot each (5,004 elements in the flat tree): style 2.39 ms, layout 6.86 ms;
the same content as light elements (4,003): 1.78 ms and 6.74 ms.

**The targets** (compare against Chromium, base / 5b): GitHub with
JavaScript on geometry 0.8904 / 0.9115, pixels 0.9884 / 0.9934; every other
score with JavaScript on and off is the base's.

## 5c-1 implementation notes

Facts of sub-feature 5c-1 (style scoping) that differ from the decision above
or that 5c-2 and 5d need. The orchestrator split 5c in two sessions: 5c-1 is
section 5 (sheets of shadow trees, `:host`, `:host()`, `:host-context()`,
`::slotted()`, `::part()`, the context step, layers per scope); 5c-2 is
section 6 (the `CSSStyleSheet` constructor, `replace`, `replaceSync`,
`adoptedStyleSheets`). Code: `crates/css/src/selector/` (`mod.rs`,
`parse.rs`, `matching.rs`, `specificity.rs`, `display.rs`);
`crates/style/src/stylist.rs`, `cascade.rs`, `element.rs`; new
`crates/engine/src/document/shadow_styles.rs`, `resources.rs`, `pipeline.rs`,
`changes.rs`; `crates/dom/src/shadow.rs`.

**Measured answers** (Chromium 148, three equal runs; ten new cases in
`tools/probes/web-shadow-dom.json`):

| Question | Case | Chromium |
|---|---|---|
| `:host-context()`: which ancestors | `host-context-ancestors` | the flat tree's: a host slotted below a `.x` of another tree matches; a host outside the flat tree whose only `.x` is a shadow-including ancestor does not; the host itself counts |
| `::slotted()` of two nested slots, equal specificity | `slotted-nested-cascade` | normal: the outer tree's rule wins; `!important`: the inner tree's; the inner tree's rule matches the element slotted through the outer slot |
| A host's `style` attribute and `:host` | `host-attribute-and-hints` | the attribute beats `:host` for normal declarations; `:host !important` beats the attribute, also the attribute's `!important` |
| A host's presentational hint and `:host` | `host-attribute-and-hints` | `:host { text-align: left }` beats `<div align=right>`: hints stay below every author context |
| `::part()` and the part's `style` attribute | `host-attribute-and-hints`, `part-depth` | the outer `::part()` beats the part's attribute and its tree's rules for normal declarations; their `!important` beats the outer `!important` |
| `@layer` of one name in a root and in the document | `layers-per-scope` | each tree has its own layer order |
| `::part()` two trees down without `exportparts` | `part-depth` | no match; the middle tree's `::part()` matches |
| Syntax and `selectorText` | `scoped-selector-syntax` | valid: `::slotted(span:hover)`, `::slotted(span)::before`, `::slotted(span)::marker`, `::part(x y)`, `::part(x):hover`, `::part(x)::before`, `::part(x):is(:hover)`, `:host.a`, `.a:host`, `div > :host`, `:host::part(x)`; invalid: `::slotted(span):hover`, `::slotted(span .a)`, `::slotted()`, `::slotted(span).a`, `::part(x).a`, `::part(x)::part(y)`, `::part(x):first-child`, `::part(x)::after:hover`; `::SLOTTED(SPAN)` serializes as `::slotted(span)`, `::part(X)` keeps its case, `::before` as `::before` (no `*`) |
| Specificity | `scoped-specificity` | `::slotted()` is a pseudo-element plus its argument; `::part()` a pseudo-element (the names do not count) |
| The featureless host | `host-featureless` | `:host`, `:is(:host)`, `:host(div.a)`, `:host(.a:not(.b))` match; `.a:host`, `:host.a`, `*:host`, `div:host`, `:host:not(.b)` do not; the document's `:host` and `::slotted(*)` match nothing |
| Pseudo-elements | `scoped-pseudo-elements` | `:host::before`, `::slotted(span)::after`, `::part(p)::before` generate content |
| `revert-layer` in two contexts | `revert-layer-contexts` | a shadow tree's `span { color: revert-layer !important }` rolls back its `span { color: green }` also when the document's `::part()` has `color: revert-layer` (normal or `!important`): each context keeps its own floor; the same for a custom property; the document's `revert-layer` alone leaves the shadow tree's rule (green), its `revert` does not (black) |
| Pseudo-classes after `::part()` | `revert-layer-contexts` | `:not()` only with valid pseudo-classes (`:not(:hover)`; `:not(:first-child)`, `:not(.a)` are invalid); `:is()` and `:where()` drop the other arguments (`::part(x):is(.a)` is `::part(x):is()`); `:lang()`, `:dir()`, `:checked`, `:defined` are valid; `:has()`, `:empty`, `:root`, `:host` are not |

**The selector model.** `::slotted(<compound>)` and `::part(<ident>+)` with
the pseudo-classes after it are `Selector::shadow` (`ShadowPseudo`), beside the
components, which then match the slot or the shadow host. A compound with only
a pseudo-element stores no implicit `*` (Chromium's `selectorText`).
`Selector::host_relation` tells whether the subject can be the featureless
host (`:host*` in the subject compound, or only inside `:is()`/`:where()`).
Matching: `matches_in_tree(selector, element, host)` treats `host` as
featureless (only `:host*`, also inside `:is()`/`:where()`, match it; it has no
parent and no siblings; it is the parent of the tree's top elements);
`matches_slotted` and `matches_part` match the argument against the element
of the other tree and the components against the slot or host. The matcher has
a const parameter `SHADOW`: for the document's rules the host's steps compile
away (the first version cost Wikipedia +1.5 % of instructions).
`:host-context()` walks `Element::flat_parent_element`; the answers are shared
in a style pass (`MatchingContext`, keyed by the argument and the element),
so N nested hosts cost O(N).

**Stylists per scope (section 5 "Sheets of a shadow root").** The document's
`Stylist` holds the scoped stylists (`Stylist::new_scoped`: author rules only,
its own `LayerTree`, `@font-face` ignored) and the root of each
(`set_root_scope`). Each stylist sorts its rules into rule sets: the tree's
own rules, rules whose subject is the host, `::slotted()` rules and
`::part()` rules (`ShadowMaps`, made at the first rule); the document's
stylist keeps no `:host` and `::slotted()` rules (they match nothing there).
The engine (`document/shadow_styles.rs`) builds them with the document's
stylist and drops them with it: roots are grouped by their applying sheets
(the key of a sheet is its text and base URL, or, for a sheet whose rules a
script changed, its identity), and each group gets one stylist built from
its first root's sheets; the other roots' sheets are not parsed until a
script reads them. **GitHub's fixture**: the popover polyfill prepends its
`<style>` (an `@layer popover-polyfill` block) only to `tool-tip`'s root; the
23 `relative-time` roots have no sheet (read with `github.rs`), so the page
has one scoped stylist with two sheets.

**Matching an element (steps 2 to 5).** `Styler::match_scoped` adds, when the
document has shadow roots and the stylist has scoped stylists or `::part()`
rules (one test per element otherwise): the rules of the element's own tree;
`::part()` rules of the tree around its host and `:host::part()` rules of its
own tree; `::slotted()` rules of the trees of the slot it is assigned to and
of each slot that slot is assigned to, at most 256 slots
(`MAX_SLOT_CHAIN`: a chain of N pass-through slots would cost O(N²); the box
tree flattens below 256 levels anyway); the `:host` rules of its own shadow
root. The style sharing key includes these rules.

**The cascade context (differs from section 5).** Section 5 ranks contexts by
depth. CSS Scoping §3.1 orders them by shadow-including tree order of their
roots; swb gives each element's contexts a number in that order: the tree
around its host (`::part()`), its own tree, the trees of its slots (the
assigned slot's first, each next one nested in it), its own shadow tree
(`:host`). Along one chain of nested trees this is the depth; it differs for
an element that is both slotted and a host (its own root comes last). Normal
declarations: outer contexts first; `!important`: inner first. The `style`
attribute belongs to its element's context and ranks above that context's
rules; the presentational hints stay below all author declarations
(measured). `revert-layer` positions carry the context, so a `revert-layer`
rolls back layers of its own context only; each context keeps its own floor
for each property (measured, `revert-layer-contexts`).

**Budgets (section 12).** The CSSOM sheets of shadow trees are parsed as
counted style data (as scripts' sheets) and each scoped stylist holds
`SCOPE_BYTES` (4 KiB) plus 256 bytes per rule of the document's
`StyleMemory`; past 128 MiB sheets are not parsed and stylists not built,
with one warning.

**Two quadratic costs of the 5a sheet drain** (found with
`shadow-styles-100000`; also in the base: a page that builds 20,000, 40,000
and 80,000 roots with a `<style>` each took 0.95, 3.9 and 14.9 s):
`shadow_including_precedes` compared siblings by a forward scan only, so the
check that a sorted list of sheets is sorted scanned to the end of the
siblings for each pair (now a scan both ways, as `Document::precedes_within`);
`placed_in_order` looked up each placed sheet with a linear scan (now a map
when more than 16 were placed). After: 0.90, 1.74 and 3.49 s, with the
stylist builds.

**Departures.** `style-scoping` #2: `exportparts` is backlog (section 11).
`Element.part` (`DOMTokenList`) is not added (`api-surface` #3 stays a
departure with the backlog members). The rows that need `adoptedStyleSheets`
or `new CSSStyleSheet()` (`api-surface` #6, `root-properties` #6,
`style-specificity-and-order` #2 and #3, `constructable-sheets`) wait for
5c-2. `MAX_SLOT_CHAIN`. `revert-layer-contexts` #2: two gaps outside shadow
DOM (`selector()` of `CSS.supports` parses `:is()` as a forgiving list;
`:lang(en)` serializes as `:lang("en")`); both are in the roadmap backlog.

**Tests.** `probe_net.rs`: the ten new cases (but `revert-layer-contexts`
#2), `style-scoping` (but #2),
`style-specificity-and-order` #1 and `api-surface` #5 match Chromium.
`shadow_rendering.rs`: the four cases that waited for a shadow `<style>`,
the new case `tool-tip-style` of `tools/probes/shadow-dom.json` (GitHub's
`tool-tip` style and the polyfill's style copied from the fixture's root, on
two hosts without `popover`; boxes and 18 computed values with `::after`
equal Chromium's) and `shadow_sheet_changes_restyle` (`insertRule`,
`deleteRule`, a new text, a `<style>` inserted and removed). Unit tests in
`crates/css` (parse, serialization, specificity, keys, matching),
`crates/style/src/cascade/scoping_tests.rs` and
`crates/engine/src/document/tests.rs`. `github.rs`: the roots' sheets and
`tool-tip`'s `:host` values (`text-align: center`, `opacity: 0`, no box).
Each new test failed when the code it checks was broken (21 mutations, and
7 for the review fixes).

**Hostile cases** (release builds, base 4b4f480 / 5c-1):
`shadow-styles-100000` 21.78 s, 924 MiB, inactive (the script's time limit)
/ 4.57 s, 1,004 MiB; `shadow-host-context-nested-10000` 4.07 s, 135 MiB /
4.01 s, 135 MiB; `shadow-slotted-passthrough-10000` 6.62 s, 237 MiB / 7.82 s,
274 MiB; the 11 earlier `shadow-*` cases within 0.01 s and 5 MiB of the base.
Most of the time of the two nested cases is the 5a drain's tree order of
sheets in nested trees (O(depth) per comparison), in both builds.

**Performance (section 13).** JavaScript off: `just snapshot` is
byte-identical to the base; callgrind totals of `swb --headless`:
Wikipedia 647.76 M / 649.66 M instructions (+0.29 %), Ars Technica
1,968.63 M / 1,970.09 M (+0.07 %), BBC 7,331.10 M / 7,332.03 M (+0.01 %; a
first run gave 7,261 M with fewer web font decodes, a timing effect of the
loader threads);
`swbtools perf` stage times within the run-to-run noise
(two runs each). GitHub with JavaScript on (`--bench 20`, three runs): style
24.9 / 24.7 ms per pass, layout 5.00 / 5.07 ms, the headless load 0.47 s in
both (budget 1.56 s). A style pass over 1,000 hosts whose roots have the same
`<style>` of 20 rules: style 1.94 ms (the base, which applies no shadow
sheet: 1.35 ms), stylist build 0.39 ms; with 1,000 distinct sheets: style
6.2 ms, stylist build 14.4 ms (1,000 scoped stylists).

After the review fixes, callgrind totals of `swb --headless --replay`
(without a screenshot, so the totals differ from the ones above; mean of
two runs, base / 5c-1): Wikipedia 866.73 M / 868.33 M (+0.19 %), Ars
Technica 2,134.81 M / 2,136.22 M (+0.07 %), BBC 7,479.73 M / 7,482.74 M
(+0.04 %).

**The targets** (compare against Chromium): every score with JavaScript on
and off is the base's (GitHub with JavaScript on: geometry 0.9115, pixels
0.9934).

**Review fixes.** (1) One `revert-layer` floor per property: a
`revert-layer` of a second context replaced the floor of the first, so the
first context's lower rules came back (Chromium: they stay rolled back;
case `revert-layer-contexts`). The cascade now keeps one floor per property
and context, also for custom properties. (2) The arguments of `:host()` and
`:host-context()` were not in the selector's state dependencies, so with
`:host(:hover)` as the page's only `:hover` rule a hover did not restyle.
(3) Pseudo-classes after `::part()`: `:not()`, `:is()` and `:where()`
accepted any argument; they now follow Chromium (table above). (4) The
`:host-context()` memo never stores an answer that the match budget cut
short (the old code returned before the store; the check is now explicit).

## 5c-2 implementation notes

Facts of sub-feature 5c-2 (constructable style sheets and
`adoptedStyleSheets`) that differ from section 6 or that later work needs.
Code: `crates/style/src/cssom/sheet.rs` (constructed sheets); new
`crates/web/src/adopted.rs`, `cssom_sheets.rs`, `idl/cssom.idl`,
`state.rs`, `state/moved.rs`; `crates/js/src/host/ops.rs` (`new_proxy` and
the internal methods that the traps need); new
`crates/engine/src/document/adopted_sheets.rs`, `shadow_styles.rs`,
`pipeline.rs`; `crates/dom/src/tree.rs`, `mutation.rs`, `transplant.rs`
(`ShadowRootData::adoptions`).

**Measured answers** (Chromium 148, three equal runs; four new cases in
`tools/probes/web-shadow-dom.json`, and two queries added while the work
went on):

| Question | Case | Chromium |
|---|---|---|
| Layer order with adopted sheets | `adopted-layer-order` | a fresh `[t, s]` gives the specification's order (t's statement first), also in a root and with the document's or the root's own `@layer` statements first; but after a style update with `[s]`, `[t, s]` keeps s's order. The rule that fits all 18 rows: Chromium keeps the layer order of the previous style update while no layer that holds style rules appears that the update before did not have. It holds for `<style>` elements too (a statement sheet inserted before a layers sheet changes nothing), and depends on when style updates run (no read between `[s]` and `[t, s]` gives the specification's order) |
| `baseURL` | `constructed-sheet-options` | an invalid URL (`http://[`) does not throw; the option has no effect: relative `url()` values resolve against the document base URL at the construction (a `<base>` inserted later does not change a sheet made before; a sheet made after keeps that base when the `<base>` goes) |
| `replace()` on a sheet of an element | `constructed-sheet-options` | a promise rejected with `NotAllowedError` "Failed to execute 'replace' on 'CSSStyleSheet': Can't call replace on non-constructed CSSStyleSheets." |
| `replace()` timing | `constructed-sheet-options` | the rules change at once; the promise is resolved when `replace` returns (its reaction runs before those of promises resolved after the call, before timers and messages); `insertRule`, `replaceSync`, `deleteRule` and `media` work while it "waits" (no visible disallow-modification flag) |
| A host moved to another document | `adopted-host-moves` | `remove()` keeps the root's array; a move into a `createHTMLDocument` document (append or `adoptNode`) or a frame's document empties it (the same array object); moving back keeps it empty; a move into the document's template contents and back keeps it |
| The same sheet twice | `adopted-array` | the last place counts: `[C, D, C]` gives C, `[B, A, B]` B |
| The observable array | `adopted-array` | the same object each time and after the setter; descriptors of an array (`length` writable, not enumerable or configurable); `Array.isArray` true, prototype the document's realm's `Array.prototype`; a write past the end and a longer `length` fail silently in sloppy code; `delete` of the last index only; `defineProperty` with an accessor or `enumerable: false` is refused; `preventExtensions` and `freeze` throw (V8's proxy messages); other keys are ordinary; trap errors have no context prefix ("Failed to convert value to 'CSSStyleSheet'.", "Can't adopt non-constructed stylesheets."), the setter's have it; a setter that fails on its second value keeps the first |
| A sheet of another document | `adopted-array`, `constructable-sheets` | `NotAllowedError` "Sharing constructed stylesheets in multiple documents is not allowed" (a frame's sheet into the main document, the main document's into a frame's document, a `createHTMLDocument` document or a root in it), a `DOMException` of the array's realm; a root of a detached element of the document accepts the sheet |

**Differences from section 6.** (1) `baseURL` is neither checked nor used
(Chromium; the specification parses it and throws for an invalid URL). The
sheet keeps the document base URL of the construction. (2) `replace()`
resolves its promise at once, not in a task. (3) The layer order is the
specification's at every stylist build: swb rebuilds the stylist from all
sheets after each change and keeps no order across builds, so the rows of
`adopted-layer-order` that depend on an earlier style update, and
`constructable-sheets` #9, are departures (`LAYER_HISTORY`). (4) After a
reorder of the same sheets by index writes (`reverse()`), Chromium's cascade
keeps the old order until an entry goes or the setter runs; swb follows the
array (departure, `adopted-array` #2).

**The observable array.** As section 6 planned: a `Proxy` whose target is
an `Array` that always holds the list, and a handler with native traps
`set`, `defineProperty`, `deleteProperty` and `preventExtensions` (Web IDL
§3.11; `get`, `has`, `ownKeys` and `getOwnPropertyDescriptor` need no trap,
since the target answers them as Web IDL's traps would). The handler is a
host object of the class `ObservableArrayHandler` (registered after the
generated classes, no prototype) whose word names the scope, so a trap finds
the scope's document after a move as a node's wrapper does; the traps are
made once per realm. Each change of the target is mirrored into the engine's
list of the scope (`EngineHost::adopted_sheets`, a `Vec<SheetRef>`; `None`
for a scope that the engine does not style), followed by
`style_sheets_changed`. The generator has no `ObservableArray`: the IDL
declares `attribute any adoptedStyleSheets` and the getter and setter call
`adopted.rs`. `swb_js` got the public operations that the traps need:
`new_proxy`, `property_key_of`, `property_descriptor_of`, `define_own`,
`set_with_receiver_of`, `delete_key`.

**Moves.** `ShadowRootData::adoptions` counts the moves of a root to another
document (but between a document and its template contents owner); a
transplant to another arena counts one. The bindings' array and the engine's
list keep the count of their last write. A move within the arena adds the
moved roots to a list of the arena (`Document::take_moved_roots`) and sets
the runtime's signal, so the bindings empty the arrays of those roots after
the call (`adopted::forget_moved`, one lookup per moved root); a transplant
to another arena empties them in `state::move_node_state`, where the array
moves with its node. A script that
holds an array sees it empty at once (measured: Chromium gives 0 for
`arr.length` read before any use of the getter). The stylist build ignores
(and drops) the list of a root that moved; `ensure_stylist` drops a stylist
that applied the sheets of a root that moved, or when the connected roots
with adopted sheets are others than at its build (a DOM change keeps the
stylist otherwise). It looks at the roots only after a change of a child
list since its last look (the arena's structure generation), and walks up
to the document (`is_connected`) only for roots that the build did not
apply.

**Cascade.** The document's adopted sheets are added to its stylist after its
other sheets, so they join its layer tree (the feature-1 backlog item); a
root's adopted sheets come after its tree's sheets in the root's scoped
stylist, and the key of an adopted sheet is its identity: GitHub's popover
polyfill adopts one sheet in 24 roots, which share one scoped stylist (the
23 `relative-time` roots; `tool-tip` has its own `<style>` first). A sheet
that is in an array more than once is added once, at its last place.

**Lifetime.** The arrays live in the document's binding state, traced, as
the other `[SameObject]` objects of nodes; a sheet lives as long as an array
(or a script) holds its wrapper. The engine's lists hold `Rc`s of the sheets;
`root_sheets` drops the lists of nodes that are no shadow root of the
document any more. Tested with and without the GC stress mode
(`crates/web/tests/adopted_sheets.rs`).

**Budgets (section 12).** 65,536 entries per array: a write past it throws
`NotAllowedError` "swb: too many adopted style sheets" (one warning per
document). A constructed sheet counts 512 bytes (`CONSTRUCTED_SHEET_BYTES`)
in the document's style memory while they fit, and its rules as the rules of
any sheet that a script made: past 128 MiB `replace`/`replaceSync` leave it
without rules (one warning). The constructor does not fail at the limit (a
first version threw `QuotaExceededError`, which stopped the script of
`shadow-adopted-many`).

**Departures.** `adopted-layer-order` #3, #6, #9, #11, #13 to #16 and
`constructable-sheets` #9 (`LAYER_HISTORY`, above); `adopted-array` #2 (the
stale order after a reorder); `constructed-sheet-options` #0 (`MediaList`
writes `bogus(` as `bogus()`, a gap outside shadow DOM, roadmap backlog);
`root-properties` #6 (`ShadowRoot.getSelection`, section 9). Not measured: a
sheet adopted twice is applied once, so the layer statements of its first
place do not count.

**Tests.** `probe_net.rs`: the probe file has 39 cases and 203 queries; the
four new cases, `constructable-sheets`, `style-specificity-and-order` #2 and
#3 and `api-surface` #6 match but the departures above. Unit tests in
`crates/style/src/cssom/sheet/tests.rs`, `crates/dom/src/shadow/tests.rs`,
`crates/engine/src/document/adopted_sheets.rs` and
`crates/engine/src/document/tests.rs`; `crates/web/tests/adopted_sheets.rs`
(with and without the GC stress mode: the host's lists follow the arrays
through every trap and the setter, sheets live as long as an array holds
them and none stays after, moves, the limit). `github.rs`: the popover
polyfill adopts one sheet in the document and the 24 roots (it no longer
prepends a `<style>` to `tool-tip`'s root); `tool-tip` keeps its `:host`
values and has no box; the relative times render as before; the load logs
no warning (as the base). Each new test failed when the code it checks was
broken (17 mutations; three first missed and got tests: `delete` of an
index that is not the last, a root inserted after the stylist build, the
array of a root that moved into a frame's document, case
`adopted-host-moves` #2).

**Hostile cases** (release builds, base 9d980aa / 5c-2): new
`shadow-adopted-many` 1.36 s, 230 MiB (inactive in the base: no
constructor) and `shadow-adopted-push-pop-loop` 3.56 s, 92 MiB (a style read
after each change rebuilds the stylist, about 0.14 ms on its page; 100,000
rounds with reads end at the task's time limit, so the case has 20,000);
the 14 earlier `shadow-*` cases within 0.3 s of the base (memory within the
run-to-run noise: `shadow-styles-100000` gave 1,020 to 1,083 MiB in four
runs of both builds).

**Performance (section 13).** JavaScript off: `just snapshot` is
byte-identical to the base; callgrind totals of `swb --headless --replay`
(mean of two runs, base / 5c-2): Wikipedia 868.44 M / 868.46 M (+0.00 %),
Ars Technica 2,136.20 M / 2,135.95 M (−0.01 %), BBC 7,482.82 M / 7,482.69 M
(−0.00 %). The binding rows of ADR 0035 section 11 (instructions per
iteration, base / 5c-2): loop 649 / 646, `setAttribute` 4,575 / 4,583, `id`
4,089 / 4,067, `appendChild` with `remove` 4,404 / 4,410, `textContent`
1,948 / 1,938, `innerHTML` 17,794 / 17,781, `createElement` 4,228 / 4,230,
`classList.add` 8,594 / 8,589 (all within 0.6 %). GitHub with JavaScript on
(`--bench 20`, three runs): style 24.2 / 24.6 ms per pass (the 24 roots now
match the polyfill's rules), layout 5.21 / 5.24 ms, the headless load (best
of 5) 0.595 / 0.594 s (budget 1.56 s).

**The targets** (compare against Chromium): every score with JavaScript on
and off is the base's (GitHub with JavaScript on: geometry 0.9115, pixels
0.9934).

**Review fixes.** (1) `ensure_stylist` looked at every root with adopted
sheets on each style pass and query, with an `is_connected` walk each:
8,000 roots with one adopted sheet and 3,000 `getComputedStyle` calls took
338 ms (21 ms without the sheets), 6,000 nested roots with 1,500 calls
passed the script time limit. Now the look waits for a change of a child
list (above, "Moves"): 24 ms and 539 ms (505 ms without the sheets). (2) A
script that held a root's array saw the old sheets after the root moved to
another document, until a use of the getter; the arrays are emptied at the
move now (above), with a new query in `adopted-host-moves` (a frame's
document and `adoptNode`; Chromium's values, three runs, two equal: the
third differs only in `focus-shadow` #1, a 5d case) and a binding test.
The delta review found that the first version of this fix looked at every
array of the document at each move (N arrays and M moves cost N x M
lookups); the arena now lists the moved roots. Not changed: by the review's reading, a change of a sheet by a script of
another window invalidates the calling window's document, not the sheet's
(not tested: the binding tests have no frames; backlog).
