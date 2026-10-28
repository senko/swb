# ADR 0028: JavaScript in the browser: Web API bindings and engine integration

- Status: accepted
- Date: 2026-10-22

## Context

ADR 0025 splits JavaScript support into three layers: the language core,
the Web API bindings and the engine integration. M7 built the language
core (ADR 0026). This ADR is the architecture of layers 2 and 3. The
features of M8 (docs/roadmap.md) build on it.

Inputs:

- ADR 0026 section 12 (the embedding API) and memo 5.3 (embedding:
  host classes, the three kinds of edges between the DOM and the heap,
  wrapper identity, the job queue).
- The survey of the target pages with JavaScript on
  ([docs/js-survey.md](../js-survey.md)). What scripts do on real pages:
  they read the DOM much more than they write it (`Node.nodeType`,
  `parentNode`, `nextSibling` are the most frequent members); they add
  listeners, set timers and observers, read and write cookies and
  `localStorage`, insert scripts and style rules, and read layout
  (`getComputedStyle`, `offsetWidth`, `getBoundingClientRect`). One
  target calls `getComputedStyle` 3,141 times while the document parses;
  another inserts 813 style rules and Chromium lays it out about 100
  times. The decisions below do not depend on one target; the survey
  gives the scale.
- The M8 targets (owner, 2026-10-22): Hacker News, Wikipedia without
  its video player, and a Craigslist posting, all with JavaScript on
  (senko.net has no scripts). Ars Technica, BBC, a Craigslist search
  page and GitHub come later; their scripts need frames, media, much
  faster style and layout, or custom elements and shadow DOM. The
  survey has a section on these candidates.
- The HTML, DOM, WebIDL, CSSOM, CSSOM View, Fetch, XHR, URL, UI Events
  and observer specifications, and black-box measurements in Chromium
  (ADR 0021, ADR 0025).

The browser today (M7):

- `dom`: an arena of nodes addressed by `NodeId`; nodes are never
  freed. html5ever parses a whole document in one call, with the
  scripting flag off. No record of changes; `element_by_id` scans.
- `engine`: `Page` parses the document, then walks it once to start
  stylesheets and images. Style and layout run for the whole document;
  `invalidate_style` parses all stylesheets again. Derived data (tree
  order, form control state, disabled elements) is built once per
  document.
- `net`: fetches run on worker threads; the engine polls completions.
  Fixture replay has no cookie jar, stores only `Content-Type` and
  `Location`, and swb does not check CORS.
- Threads: the engine runs on the GUI's main thread or on the headless
  runner's thread. ADR 0026 section 9 requires a stack of at least
  8 MiB for JavaScript.
- `js`: one realm, `eval_named`, `call_function`, `run_jobs`,
  deadlines, termination, persistent roots, one value of host data, and
  `ObjectKind::Host` objects. No host class registry, brand checks, host
  hooks for internal methods or host-defined global object.

Constraints: no `unsafe`; content from the network never causes a panic
or a hang; JavaScript off is exactly the old behaviour (ADR 0025); the
`js-*` crates depend on no browser crate.

## Decision

The owner decided on 2026-10-22:

- The W3C Software and Document License is allowed for IDL data files
  (section 5; ADR 0003 update).
- The defaults of this ADR: the CORS check for requests from scripts
  (section 12); cookies and `localStorage` in memory for the life of
  the process (section 12); 5 s per task and the stop of a document's
  scripts after three time-outs (section 7); the page on the GUI's main
  thread in M8 (section 2).

### 1. Crates

Two new crates:

| Crate | Responsibility | Depends on |
|-------|----------------|------------|
| `web` (`swb-web`) | The Web API bindings and their implementations, the wrappers, the event loop's queues and timers, the processing of script elements, the rendering update steps that run script (animation frames, observers, events). | `js`, `dom`, `css`, `style`, `net` |
| `webidl` (`swb-webidl`) | A parser for the subset of WebIDL that swb uses and the generator of the binding glue (section 5). A binary that writes Rust source into `web`; not a dependency of the browser. | — |

`engine` depends on `web`. `web` knows nothing of `engine`: it defines
a trait, `EngineHost`, for what scripts need from the engine (the
document, style and layout queries, scrolling, focus, form control
state, fetches, navigation, the parser for `document.write`), and the
engine implements it. `js`, `js-syntax`, `js-regexp` and `js-text` stay
independent of all browser crates.

Reasons: the bindings will be the largest part of M8 and grow with each
target; in their own crate they build and test without layout and
paint, and CLAUDE.md allows parallel work in separate crates. The
dependency points from the engine to the bindings because the engine
drives the page (network, input, rendering) and calls into scripts at
fixed points; a trait in the other direction would turn every engine
entry point into a callback.

### 2. Threads, runtimes and ownership

**One runtime per document.** The engine creates a `js::Runtime` when it
commits a document with JavaScript on, and drops it when the document
goes away (navigation, reload). The runtime has one realm, whose global
object is the `Window`. Same-origin frames would add realms to this
runtime (section 14).

Reasons: everything that scripts create dies with the document in one
step, so leaks through the DOM (section 3) are bounded by the life of a
document; a document whose runtime failed (heap-limit termination,
section 7) can drop it at once; there is no state to clear between
documents. The cost of a runtime is the creation of its realm and the
binding prototypes, about once per navigation (to measure in feature 4c).

**Threads.** The runtime is not `Send` (ADR 0026), so it lives on the
thread of its page: the event loop runs on that thread, between the
engine's other work.

- The headless runner and the headless automation server run the page
  on a thread that swb creates with a 16 MiB stack.
- The GUI keeps the page on the main thread in M8 (winit needs the main
  thread for the window). At start, swb reads the stack limit of the
  process (`getrlimit` through a safe crate: `rustix` with its
  `process` feature is in the tree through winit; feature 2 makes it a
  direct workspace dependency after the ADR 0003 check). No limit
  (`RLIM_INFINITY`) counts as enough. Below 8 MiB, the GUI runs with
  JavaScript off and logs a warning (the toolbar toggle, the shortcut and
  `page.setJavaScript` then keep it off). This replaces the statement of
  ADR 0026 section 9 that the main thread's stack cannot be checked in
  safe Rust. (Feature 4c2: the refusal is in the engine. The embedder
  states in `PageConfig::scripts_allowed` whether the thread that runs
  the page has a big enough stack: swb's page threads say yes, the GUI
  the result of the stack check, `HeadlessBrowser::new` takes it as an
  argument. A page without it keeps the setting off in
  `Page::set_javascript`, whatever calls it, so no path creates a
  runtime on a small stack.)
- A page thread for the GUI (the window thread sends input and receives
  frames) is later work. Until then a script that runs to the end of its
  time budget (section 7) freezes the window for that time.

**Ownership: the engine lends the document to the runtime.** Native
functions are plain functions that receive `&mut Runtime`; they reach
the host only through the runtime's host data, which must be `'static`.
So the engine cannot pass a borrowed `&mut Page`. Instead:

- The engine splits `Page` into the page itself (history, the pending
  navigation, the loader, the viewport, the UI state) and a boxed
  `DocumentState` (the DOM, the parser, stylesheets, stylist, styles,
  fragments, images, forms, scroll offsets, element states). Style,
  layout and the queries become methods of `DocumentState`.
- Before it runs a task, the engine moves the box (and the shared parts
  that queries need: the font context, a loader handle) into the
  runtime's host data; after the task it takes them back. A move of a
  box costs a pointer copy. (Feature 3b: the loader handle is an
  `Rc<Loader>` that the box holds itself, shared with the page, and the
  box keeps a copy of the viewport; the font context stays in the page,
  and the methods of `DocumentState` that lay out or paint take it as an
  argument, so a runtime holds it next to the box for the task. The
  viewport scroll position and the selection are in the box too: scripts
  read and change them. `Page::take_document_state` and
  `return_document_state` lend the box; the return drains the journal of
  the task. While the box is out, the page keeps a `lent` flag: the
  requests map is in the box, so the page cannot match a completion to
  its request. `process_network` then reads nothing from the loader and
  `wait_until_loaded` returns at once; the completions stay queued in the
  loader and are handled in their order after the box returns. `title()`
  and `scroll_position()` give empty and zero values, and a navigation or
  reload in this time saves scroll 0 into the history entry, so a script
  task must end before a navigation commits. A box that comes back after
  a navigation is dropped.)
- A native function gets the host through a closure that borrows the
  runtime's host data (`with_host(rt, |host| ...)`). While the closure
  runs, the runtime is borrowed, so the Rust compiler rejects any call
  into script code from inside it. Bindings therefore copy what they
  need out of the DOM, release it, and only then call script (for
  example, event dispatch collects the listeners first). This rule
  follows from the types; it needs no run-time check. The same holds for
  allocation: a binding that creates a wrapper looks up the wrapper
  vector, releases the host data, allocates the object, and then stores
  it.

**Dependency check (feature 2, ADR 0003).** `rustix` 1.x (licence
`Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`; maintained, widely
used) with the features `std` and `process`: the safe `getrlimit`. It is
already in the tree through winit (version 1.1.5), so it adds no new
crate; the feature `process` adds only the `prctl` constants of
`linux-raw-sys`. It is a direct workspace dependency of `swb` on Unix.
`cargo deny` passes. It is not in the scope exclusions of ADR 0003.

### 3. The DOM for scripts

Changes in `dom`:

- **Resumable parser.** The engine drives html5ever's tokenizer
  directly: a `BufferQueue` holds the decoded input, and `feed` returns
  when the tree builder reaches the end tag of a `script` element
  (html5ever reports it as a script result). The parser object keeps
  the tokenizer, the tree builder and its sink. The caller keeps the
  `Document` and lends it to each step (`HtmlParser::resume(&mut doc)`
  moves it into the sink for the step and back), so scripts and the
  engine have the document while the parser keeps its state (the stack
  of open elements holds `NodeId`s, which stay valid). A step with
  another arena is refused. The scripting flag is an option of the
  parse. (Feature 3a: the borrow replaces "take the document out of the
  sink and put it back"; it is the same move, and the parser cannot be
  fed without the document.)
- **Change journal.** Every change of the tree, an attribute or a text
  node appends an entry to a journal of the document: the kind, the
  target, the added and removed nodes with their siblings, the attribute
  name and the old value. The parser's changes are journaled too. The
  engine drains it (resource loading, style invalidation, derived
  data). With JavaScript off, the engine is the only consumer and drains
  once after the parse. (Feature 3a: the parse without scripts,
  `parse_html*`, does not record; its journal is *incomplete*, which
  tells the consumer to treat the whole document as changed, so the
  engine walks the new document once, as before. Recording costs 2 to
  3 % of the parse time of the fixtures (measured 2026-10-23: a journaled
  parse is 3.7 to 4.8 % slower than the parser of M7, an unjournaled one
  1.2 to 3 %), which leaves too little room in the 5 % budget of a JS-off
  feature. `HtmlParser`, which the engine uses with JavaScript on,
  records. A journal also becomes incomplete when it overflows at
  1,048,576 entries.)
- **Mutation sink.** Mutation observers cannot wait for a drain: DOM
  "queue a mutation record" finds the interested observers on the tree
  at each primitive change, and a removal adds transient registered
  observers to the removed node, while one call (`innerHTML`,
  `replaceChild`, adoption) makes several changes. So the change
  functions of `dom` also report each primitive change, as it happens,
  to a sink that the caller passes (the bindings' observer registry;
  nothing when no observer exists). The parser reports through the same
  sink. (Feature 3a: the caller passes the sink by installing it in the
  document, `Document::set_mutation_sink`, as a `'static` trait object,
  not as an argument of each change function: html5ever's tree sink lives
  in the parser across steps and cannot hold a borrowed sink without
  `unsafe`, and the document carries the sink to every change function
  and to the parser. The sink holds only Rust data, the registered
  observers by `NodeId` and the queued records; the callbacks stay in the
  binding state, keyed by observer number. The sink gets the document in
  its state after each change, and the old value of attributes and text.)
- **Generation.** A counter that grows with every change. Derived data
  (tree order, disabled elements, the id map, live collections, cached
  query results) records the generation it was built for and is rebuilt
  lazily when the generation differs. (Feature 3b: the engine's derived
  data is `Derived<T>` in `engine/src/document/derived.rs`. The base URL
  and the title are rebuilt at the drain of the journal when a change
  touched a `base` or `title` element, and confirmed for the new
  generation otherwise, because the page reads them without `&mut`. The
  form controls with their dirty flags and the disabled elements are
  rebuilt together at the next flush; the tree order when a selection
  needs it.)
- **Id index.** A map from `id` values to elements, kept up to date by
  the attribute and tree changes; with several elements of one id, the
  first in tree order is found by a walk of only those elements.
  (Feature 3a: the index is built at the first lookup, with one walk, and
  kept up to date from then on; a parse without lookups does not pay for
  it.)
- **Fragment parsing** with a context element (`innerHTML`,
  `insertAdjacentHTML`, `outerHTML`, `DOMParser`,
  `createContextualFragment`): html5ever parses into a temporary
  document, and the nodes move into the target arena.
- **More documents in one arena.** `createHTMLDocument`, `DOMParser`
  and template contents create document or fragment nodes in the same
  arena; each node knows its node document. Moving a node between these
  documents (adoption) changes that field only. The engine renders only
  the tree of `NodeId::DOCUMENT`.

**Node lifetime.** Nodes stay immortal in M8: a node lives until its
document's arena is dropped (section 2: with the document). Wrappers are
strong (section 4). So no cycle between the DOM and the heap can be
collected too early, and none needs to be collected within a document.
The cost is memory for nodes that a script detaches and drops; on the
surveyed pages it is small (the most that scripts add to one page is
16,500 elements). Limits stop hostile growth (section 16). A page that
rebuilds its DOM for hours reaches the limit; node reclamation is later
work: `NodeId` gets a generation, the wrapper map becomes a weak table
(ADR 0026 section 8), wrappers of attached nodes stay roots, a live
wrapper of a detached node keeps its whole detached tree alive, and
detached trees without live wrappers or engine references are freed
after a collection.

**Strings.** The DOM stores UTF-8. A JavaScript string with an unpaired
surrogate becomes U+FFFD when it enters the DOM (a deviation for
`DOMString`; `USVString` requires it). This keeps one string form in the
DOM, style and layout.

### 4. Wrappers and host objects

**One wrapper per node.** The binding state has a vector indexed by
`NodeId` that holds the wrapper object of each node that a script has
seen. A lookup creates the wrapper at the first access, with the
prototype of the node's most derived interface (`HTMLDivElement`,
`SVGPathElement`, `Text`). The vector is traced as a root (section 6).
Identity, expandos and `WeakMap` keys therefore work as WebIDL
requires (memo 5.3).

**Objects that belong to a node** and are `[SameObject]`
(`classList`, `dataset`, `style`, `childNodes`, `children`,
`attributes`) are created once and kept in a side table keyed by
(`NodeId`, slot), also traced.

**Host data.** A wrapper's host data is the `NodeId`. Other platform
objects are host objects whose data is a Rust value with a trace method
(`GenericData`). Views of the DOM keep only what identifies them and
read the DOM at each access: a `DOMTokenList` keeps the element and the
attribute name, a live collection its root and filter (with a cache
keyed by the generation), a static `NodeList` its nodes. Values that
such an object holds (callbacks, listeners) are traced through its
data, so cycles through them are collected as ordinary heap cycles
(memo 5.3, edge 3).

**Event listeners** of a node are in a table of the binding state keyed
by `NodeId`; the listeners of other event targets are in their host
data. Both are traced.

**Host classes and brand checks.** Each interface is a host class with a
number, its parent class and a prototype per realm. A brand check
(WebIDL "implements") compares the object's class with the expected one
along the inheritance: each class stores its depth and its ancestors, so
the check is one comparison at the expected class's depth. A failed
check throws `TypeError: Illegal invocation`, as Chromium does.

**The global object.** The realm's global object is a host object of
class `Window`, with the prototype chain `Window.prototype`,
`WindowProperties`, `EventTarget.prototype`, `Object.prototype`. Members
of `Window` are own properties of the global object, as WebIDL
`[Global]` requires. `window`, `self`, `globalThis` and `frames` return
the global object itself: without frames, nothing can keep a
`WindowProxy` across navigations, so the proxy and the window are the
same object in M8 (a deviation that only frames can observe).

**Legacy platform objects** (indexed and named properties: `NodeList`,
`HTMLCollection`, `DOMTokenList`, `NamedNodeMap`,
`CSSStyleDeclaration`, `DOMStringMap`, `Storage`, `HTMLFormElement`)
use the host hooks for internal methods (section 6), following
WebIDL §3.9.

### 5. Bindings generated from WebIDL

The glue between JavaScript and Rust is generated; the behaviour is
written by hand.

- **Source.** IDL files in `crates/web/idl/`, one per specification,
  copied from the specifications' IDL blocks and cut to the members that
  swb implements. A member that is not in the files does not exist for
  scripts, so feature detection (`'IntersectionObserver' in window`)
  answers truthfully.
- **Generator.** `swb-webidl` parses the files and writes one Rust
  module per interface into `crates/web/src/generated/`, which is
  checked in, as the generated Unicode and time zone tables are. A unit
  test runs the generator in memory and fails if the checked-in output
  differs. `just bindings` regenerates.
- **What it generates:** the class registration (name, parent, number);
  the interface object and the prototype with each attribute as an
  accessor pair and each operation as a method, with the right `length`,
  `name`, attributes and order; constants; the brand check of `this`;
  the conversion of each argument (WebIDL §3.2) into a Rust type; the
  call of a hand-written function `fn(cx, this, args...) -> Result<T,
  Exception>`; the conversion of the result; exceptions as `TypeError`,
  `RangeError` or `DOMException`.
- **Types in scope:** the primitive types (integers with `[Clamp]` and
  `[EnforceRange]`), the string types, interfaces, nullable and
  optional types with defaults, variadic arguments, `sequence`,
  `record`, dictionaries, enumerations, callback functions and callback
  interfaces (`EventListener` with `handleEvent`), `Promise`, `any`,
  `object`, and the unions that interfaces in scope use. Overloads that
  the argument count or a simple type test distinguishes; anything
  harder is written by hand (marked `[SwbCustom]`).
- **Extended attributes in scope:** `[Global]`, `[Exposed=Window]` (no
  workers), `[LegacyUnforgeable]`, `[Replaceable]`, `[PutForwards]`,
  `[SameObject]`, `[NewObject]`, `[LegacyNullToEmptyString]`,
  `[LegacyFactoryFunction]` (`Image`, `Option`), the named-property
  attributes of the legacy platform objects, and `[Reflect]` of the
  HTML IDL (generated getters and setters that reflect content
  attributes, with the keyword tables of enumerated attributes).
  `[CEReactions]` is ignored (no custom elements).
- **License of the IDL.** WHATWG standards are CC BY 4.0, and "to the
  extent portions of it are incorporated into source code, such
  portions in the source code are licensed under the BSD 3-Clause
  License instead" (WHATWG IPR policy 7.1.1); BSD-3-Clause is in the
  ADR 0003 list. W3C specifications (CSSOM, CSSOM View, UI Events,
  Intersection Observer, Resize Observer, Pointer Events, Performance
  Timeline, High Resolution Time) use the W3C Software and Document
  License (2023): permissive, with the condition to keep the notice and
  to mark changes. The owner allowed it for IDL data files (ADR 0003
  update, 2026-10-22). Each IDL file starts with the source URL, the
  date and the license, and says that swb cut it to the members it
  implements; `docs/credits.md` and `THIRD_PARTY_NOTICES.md` list them.

Reasons: the surveyed pages use 505 distinct members of 100 interfaces
(the three M8 targets 226 members of 53; counted from `survey.json`,
where the members of `window` count as one interface), and feature
detection observes where members are and how
they are defined. A generator makes the property definitions, the
argument conversions and the brand checks uniform and keeps the
hand-written code to the behaviour. Checked-in output can be read and
reviewed and needs no build step.

Feature 4b notes:

- (Feature 4b: how the glue calls the hand-written functions.) Each
  generated interface module has a trait `NameImpl: Host` with one
  associated function per member: `fn(cx: &mut Cx, this, args...) ->
  Result<T, Exception>` for attributes (getter `name`, setter
  `set_name`), operations (overloads `name_1`, `name_2`, ... in IDL
  order), the constructor (`constructor`, which returns the new object;
  `Cx::new_instance` makes it with the prototype of `new.target` and the
  `[LegacyUnforgeable]` members) and `stringifier;` (`stringify`). The
  native functions of the glue are generic over the host type `H` and
  call `<H as NameImpl>::member`; `mod.rs` has `define_classes::<H>` and
  `create_interfaces::<H>`, and the trait `Bindings` that bundles all
  traits of a set. 4c implements the traits for its binding state; the
  glue for `EventTarget`, `Event`, `Node` and `DOMException` compiles
  without that and registers with a test host
  (`crates/web/tests/interfaces.rs`). `[SwbCustom]` members are
  hand-written native functions (`fn(&mut Runtime, &NativeCall)`) that the
  trait declares and the glue defines as they are.
- (Feature 4b: the `cx` interface.) `Cx` holds `&mut Runtime`, the realm,
  the class of the member and the native call; hand-written code reaches
  its state through `cx.rt` (4c: `with_host`). What the glue needs from
  4c is the trait `Host` (on the runtime's host data): `wrap_node` (the one
  wrapper of a node, created with the most derived interface), the
  content attributes of `[Reflect]` (`reflect_get`, `reflect_set`,
  `reflect_remove`, `reflect_url`) and `glue_state` (the templates of
  `[LegacyUnforgeable]` members, traced by the host state). Interface
  types that inherit from `Node` convert to `NodeId` (the wrapper's word
  is the node index, `node_data`); other interfaces to `Platform` (the
  object and its host data) as arguments and to the object as results.
- (Feature 4b: messages.) Hand-written code and conversions return an
  `Exception` with a bare message; the glue adds Chromium's context
  ("Failed to execute 'x' on 'Y': ", "Failed to set the 'x' property on
  'Y': ", "Failed to construct 'Y': ") to `TypeError`, `RangeError` and
  `DOMException` messages. A value of the wrong type is worded by its
  place: "parameter N is not of type 'T'." for an argument, "Failed to
  convert value to 'T'." in a dictionary member, a sequence or record
  element or an attribute value, "The provided value is not of type
  '(A or B)'." for a union (members sorted, as Chromium prints them).
  Errors that script code throws pass unchanged. Measured in
  `tools/probes/web-bindings.json`.
- (Feature 4b: overloads and unions.) The generator computes the effective
  overload set per argument count: fewer arguments than the shortest
  overload is "N argument(s) required, but only M present."; a count
  without an overload is "Overload resolution failed." (Chromium's
  words); one overload is called directly; several overloads of one count
  are told apart at the distinguishing index by the runtime test of Web
  IDL §3.6 step 12 (`bindings::overload::select`, without buffer and
  `bigint` types), which also fails with "Overload resolution failed.".
  The selection runs before the arguments before the distinguishing index
  are converted (Web IDL converts those first; the difference shows only
  with side effects of those conversions together with an `@@iterator`
  getter). Each union type becomes a Rust enum with a converter that
  follows §3.2.25 for its member types, generated per union.
- (Feature 4b: property order.) As measured in Chromium: attributes in IDL
  order, then constants, then operations sorted by name, then
  `constructor` (the generator calls `Runtime::begin_interface`, defines
  the members, then `end_interface`). Chromium's attribute order comes
  from its own IDL files where they differ from the specifications
  (`Event`, `DOMException`); swb keeps the specifications' order.
- (Feature 4b: `[Reflect]`.) The HTML attributes `[Reflect]`,
  `[ReflectURL]`, `[ReflectNonNegative]`, `[ReflectPositive]`,
  `[ReflectPositiveWithFallback]`, `[ReflectRange]`, `[ReflectDefault]`
  and `[ReflectSetter]` for `DOMString`, `DOMString?`, `USVString`,
  `boolean`, `long` and `unsigned long`. HTML gives the keywords of
  enumerated attributes in prose, so swb's IDL files add them with
  extended attributes of their own: `[SwbKeywords="ltr rtl auto"]`
  (`alias=canonical` maps a keyword to its state's keyword, `=canonical`
  is the empty keyword), `[SwbMissing="..."]` and `[SwbInvalid="..."]`
  (the canonical keywords of the missing and invalid value defaults).
- (Feature 4b: where the test IDL lives.) `crates/web/tests/idl/test.idl`
  (swb's own, with a stand-in `Node` and a `[Global] Window`), generated
  with `webidl.idl` into `crates/web/tests/generated/` and compiled by
  `crates/web/tests/bindings.rs`. `swb_webidl::binding_sets` names both
  sets; the freshness test (`crates/webidl/tests/freshness.rs`) checks
  both.
- (Feature 4b: not generated yet.) Static attributes, `[Reflect]` of
  `double` and of element references: the generator refuses them with a
  message and the line. (Indexed and named properties and `iterable` came
  with feature 4c1; `[LegacyFactoryFunction]` with feature 6c2.)

Feature 4c1 notes:

- (Feature 4c1: legacy platform objects.) The interface that declares a
  special operation gets trait functions: `index_count` (the supported
  property indices are `0..n`), `supported_names` and
  `is_supported_name` (a default body searches the names; every property
  access of an object with named properties asks, so a live collection
  should answer from a cache), and `indexed_getter`, `indexed_setter`,
  `named_getter`, `named_setter`, `named_deleter` for special operations
  without an identifier; one with an identifier (`getter Node? item(...)`)
  calls its regular operation. Every class whose instances support
  indexed or named properties, also through inheritance (hooks are not
  inherited in `js`), gets hooks that follow Web IDL §3.9
  (`bindings/legacy.rs`): `[[GetOwnProperty]]`, `[[Set]]` (the setters
  when the object is the receiver, else the ordinary steps with the own
  property without the named ones), `[[DefineOwnProperty]]`, `[[Delete]]`,
  `[[PreventExtensions]]` (refused: `Object.freeze` throws "Cannot
  freeze") and `[[OwnPropertyKeys]]` (indices, visible names, then the
  stored keys), with `[LegacyOverrideBuiltIns]` and
  `[LegacyUnenumerableNamedProperties]`. Chromium's messages of refused
  writes, definitions and deletions in strict code ("Failed to set an
  indexed property [0] on 'NodeList': Indexed property setter is not
  supported.", "Index property setter" for `defineProperty`, "Named
  property setter", "Index/Named property deleter", "Accessor properties
  are not allowed.") and the prefixes of exceptions of the special
  operations ("Failed to set a named property 'a-b' on 'DOMStringMap': ")
  were measured (`tools/probes/web-legacy.json`). Where Chromium departs
  from Web IDL, swb follows Web IDL: `Reflect.defineProperty` of a
  refused index or name is `false` (Chromium: `true`); a write through
  another receiver (`Object.create(list)[0] = x`) follows §3.9.2;
  `[[OwnPropertyKeys]]` lists the visible names before the stored keys
  and leaves out the names that the prototype hides (Chromium lists
  stored keys first, every supported name, and no names of a form); a
  named setter takes `storage.getItem = x` (Chromium makes an own
  property). One Chromium behaviour is followed: a definition with a
  generic descriptor (`{ enumerable: true }`) calls the setter with
  `undefined` (Web IDL refuses it).
- (Feature 4c1: iterable declarations.) A value iterator puts
  `%Array.prototype%`'s `entries`, `keys`, `values` and `forEach` first
  on the prototype; every interface that declares an indexed property
  getter gets `@@iterator` = `%Array.prototype.values%` (not
  enumerable). A pair iterator adds the trait function `pair_at(index)`
  (the value pairs to iterate over, read at each step), a class "X
  Iterator" registered after the interface classes, an iterator prototype
  per realm (`next`, `@@toStringTag`, `[[Prototype]]`
  `%Iterator.prototype%`, kept in the glue state), and `entries`,
  `forEach`, `keys`, `values` after the operations, with `@@iterator` =
  `entries`. Iterator results list `done` before `value`, as Chromium's
  do.
- (Feature 4c1: `[Global]`.) The prototypes of a `[Global]` interface and
  of its ancestors are immutable prototype objects
  (`glue::immutable_prototypes` in `create_interfaces`, Web IDL §3.7.3),
  and the generated `init_global` makes the global object one (§3.8.1);
  Chromium's message: "Immutable prototype object '#<Window>' cannot
  have their prototype set".

Feature 4c2 notes:

- (Feature 4c2: the runtime.) `swb_web::ScriptRuntime` is the runtime of
  a document: `js::Runtime` with a 256 MiB heap limit, the generated
  classes first, the binding state (`WebState`) as host data, the class
  `WindowProperties` (hooks; registered after the generated classes),
  the interfaces, the named properties object linked between
  `Window.prototype` and `EventTarget.prototype` (immutable prototype,
  `@@toStringTag` "WindowProperties"), then the generated `init_global`.
  The engine's `DocumentState` implements the trait `EngineHost` (the DOM,
  the URL, the base URL, URL parsing, the encoding, the content type),
  and the page lends the box as `Box<dyn EngineHost>` and gets it back
  with a downcast (`into_any`); `web` stays independent of `engine`. A
  task sets the 5 s deadline, runs the script or the evaluation, then a
  microtask checkpoint; uncaught errors and unhandled rejections go to
  the console as Chromium words them (`Uncaught TypeError: ...`,
  `Uncaught (in promise) ...`). The runtime is created when a document
  with scripting on commits and dropped at the next commit. After three
  tasks that reach the time limit, or one that reaches the heap limit,
  the document runs no more scripts (the rule of section 7, brought
  forward from feature 5b so that a page of endless inline scripts ends;
  the drop of the runtime after a heap-limit stop stays in 5b). The
  global object and the named properties object carry the host data
  `Word(u64::MAX)`, which is no node index (the word 0 is the document
  node), so that `node_of` never takes them for a node.
- (Feature 4c2: wrappers.) The wrapper vector is indexed by `NodeId`;
  the side table is keyed by `(NodeId, Slot)` (`Slot::ChildNodes` for
  now). Both are traced from the host state, so identity, expandos and
  `WeakMap` keys hold for the life of the document. A wrapper gets the
  most derived interface that swb has: `Document`, `DocumentType`,
  `DocumentFragment`, `HTMLHeadElement` for `head`, `HTMLElement` for the
  other HTML elements, `Element` for SVG and MathML, `Text`, `Comment`,
  `ProcessingInstruction` (a departure from Chromium until the element
  interfaces exist: Chromium has `HTMLDivElement`, `SVGSVGElement`, and
  `HTMLDocument` for the document).
- (Feature 4c2: `childNodes`.) The list keeps its parent and a cache of
  the children with the DOM generation it was built for; `length`,
  `item` and the indexed getter read the cache, so a loop over the list
  walks the children once while the DOM does not change. The generation
  is one counter per arena: any change anywhere makes every list stale.
  (Feature 6c2 fix: the lists of children use a second counter that only
  child-list changes bump; see the 6c2 notes.)
- (Feature 4c2: the window.) Without frames, `window`, `self`, `frames`,
  `top` and `parent` return the global object and `length` is 0; the IDL
  has `typedef Window WindowProxy`. The named properties object answers
  `window[name]` for element ids and the `name` of `embed`, `form`, `img`
  and `object` elements, after the named property visibility algorithm
  (the global object and the prototypes other than the named properties
  object hide a name). Ids come from the DOM's id index; the `name`
  attributes are cached by DOM generation (one walk per generation, at
  the first miss of the id index). A name of several elements gives one
  element, the one with the id first (HTML: an `HTMLCollection`, which
  comes with feature 6b);
  `[[OwnPropertyKeys]]` lists no names, as in Chromium.
- (Feature 4c2: the IDL cut.) `Node` has its read-only tree accessors
  (`ownerDocument`, `parentNode`, `parentElement`, `childNodes`, the
  siblings and children, `getRootNode`, `hasChildNodes`, `contains`,
  `isSameNode`, `isConnected`, `baseURI`); the members that change the
  tree (`appendChild`, `textContent`, `nodeValue`, `normalize`,
  `cloneNode`) and the comparisons and namespace lookups go to feature 6a
  and are not in the IDL until then, so feature detection answers
  truthfully. `Document` has `URL`, `documentURI`, `compatMode`,
  `characterSet`, `charset`, `inputEncoding`, `contentType`, `doctype`,
  `documentElement`, HTML's `title` (with the setter), `body` (with the
  setter and Chromium's messages) and `head`, and `ParentNode`'s element
  accessors. `CharacterData` has `data` (with the setter) and `length`.
  `Event` and `EventTarget` are stand-ins until feature 5a: listeners are
  accepted and never called.
- (Feature 4c2: evaluation.) `ScriptRuntime::evaluate` runs the source as
  a classic script, settles a promise with a microtask checkpoint (a
  promise still pending is an error until the event loop exists), and
  writes the value with a serializer that is compiled before any page
  script and keeps the built-ins it uses (`JSON.stringify`,
  `Array.isArray`, `Object.keys`, `String`). Two forms: the probe's form
  of docs/testing.md (`NaN`, `Infinity`, `-0.0`) for `swb --eval`, and
  valid JSON for `page.evaluate`.
- (Feature 4c2: `console`.) The `js` crate's `console` has `log`,
  `info`, `warn`, `error` and `debug` in Chromium's shape (enumerable
  methods, `@@toStringTag` "console", an empty object as its prototype);
  the sink gets the level. `ScriptRuntime` writes each line to the log
  (target `console`) and to a buffer of the last 1,000 messages (16 KiB
  each), which `page.consoleMessages` returns.
- (Feature 4c2: inline scripts.) Until feature 5c, the inline classic
  scripts of a document run in tree order after the parse, each as a
  task (`scripts.rs`: the type string and the JavaScript MIME type
  essence match of HTML "prepare the script element", `nomodule`
  skipped, scripts with `src` skipped). A script therefore sees the whole
  document, not only the part above it.

Feature 5a notes (the event core; see also section 11):

- (Feature 5a: where listeners live.) A node's listener list is in the
  binding state by `NodeId` (the node has a wrapper, and both live as long
  as the document); the window's list is in the binding state too; a
  target made by `new EventTarget()` (or a script subclass) keeps its list
  in its generic data and dies with the object. The kinds are told apart
  by class (`Node`, `Window`, else the generic data of `EventTarget`); a
  later interface that inherits from `EventTarget` without being a node
  adds its list to its own data and an arm to `events::target::kind_of`.
  Each list is kept per event type in the order of addition, with an
  index by callback and capture from 8 listeners of a type (shorter
  lists are searched), so that the duplicate check and the removal are
  O(1) and a dispatch clones only the listeners of its type. The lists
  of nodes and of the window live outside the JavaScript heap: at most
  262,144 of them per document, one more ends the task (a
  termination, as the DOM limits of section 16). The lists of constructed
  targets count as heap bytes of their objects.
- (Feature 5a: dispatch.) DOM §2.9 with the path structs of the
  specification (the shadow-tree fields stay false), `eventPhase`, the
  capture listeners at the target before the others, the window last in
  the bubble path (not for `load`, and only for the document of the
  window), `composedPath` as specified, the dispatch flag
  (`InvalidStateError` "The event is already being dispatched." and "The
  event provided is uninitialized."), `once`, `passive` and its default
  for touch and wheel events at the window, the document, its element and
  its body, `window.event` (`[Replaceable]`, restored after each
  listener), and listeners added or removed during the dispatch (the list
  is cloned per target at its turn; removed listeners are skipped by
  their flag). A listener is called with `Runtime::call_function`: from
  the engine it is an outermost run, from `dispatchEvent` a re-entry that
  charges the recursion budget (a recursion through nested dispatches
  ends with a `RangeError` at a depth of about 400 in swb's release
  build). An exception is reported as HTML "report the exception" would
  write it to the console (`Uncaught TypeError: ...`; feature 5b adds the
  `error` event at the window) and the dispatch goes on; a termination
  ends the dispatch after the flags are reset. Not done: the legacy
  `webkit` renames of trusted animation events, activation behaviour
  (feature 6d), `relatedTarget` and touch target lists, the `signal`
  option (no `AbortSignal` yet).
- (Feature 5a: the interfaces.) `Event` (with `initEvent`, `returnValue`,
  `cancelBubble`, `srcElement`, `timeStamp` from the runtime's time origin
  coarsened to 0.1 ms), `CustomEvent` (with `initCustomEvent`),
  `ErrorEvent`, `PromiseRejectionEvent` (`promise` is an `object` as in
  HTML; Chromium takes any value), `PageTransitionEvent`, and
  `document.createEvent` for the names of the interfaces that swb has
  (`Event`, `Events`, `HTMLEvents`, `SVGEvents`, `CustomEvent`; others
  give Chromium's `NotSupportedError`). The generator learned unions with
  `undefined` (`(Event or undefined)`).
- (Feature 5a: the engine's entry.) `ScriptRuntime::dispatch_event(target,
  &TrustedEvent)` fires a trusted `Event` or `PageTransitionEvent` at a
  node or the window (with the legacy target override for the window's
  `load`) as a task: the deadline, the dispatch in a handle scope, the
  microtask checkpoint, the stop rule; `Page::dispatch_event` lends the
  document state for it. The checkpoint after each listener when the
  stack is empty (HTML "clean up after running a callback") comes with
  feature 5b.

Feature 5b notes (the event loop; see also section 7):

- (Feature 5b: the queue.) The binding state has one task queue, ordered
  by due time in page time and then by the order of queuing: a timer is
  due when its timeout has passed, any other task (the notification of
  rejected promises, a finalization cleanup) when it is queued. A task
  queued at time t therefore runs after the timers that were due at t and
  were scheduled before it; this is the order that Chromium 148 shows
  (measured: the `unhandledrejection` notification of a task runs after
  the `setTimeout(0)` timers that the task scheduled and before a
  `setTimeout(1)` timer). The task sources of section 7 (user
  interaction first) come when the engine has tasks of more than one
  source (feature 5c's parser tasks, 6d's input). `ScriptRuntime::run_due_tasks`
  runs the due tasks for a budget of real time; `next_task_time` says
  when the next is due. `Page::pump` is one turn: the network
  completions, the due tasks for at most 16 ms, and the real time of the
  next task. The GUI calls it in `about_to_wait` and waits with
  `ControlFlow::WaitUntil` for the next task or input; the headless
  automation server calls it in its loop and waits for the next task, a
  request or a network completion; `Page::wait_until_loaded` runs due
  tasks while the page loads. No loop busy-waits.
- (Feature 5b: a task.) Each task: the deadline (the time limit, or less
  near the end of the document's budget), a handle scope, the body, the
  microtask checkpoint, the cleanup tasks of finalization registries
  (one task per registry, queued after each task while
  `pending_finalization_cleanups` is above 0), the stop rules. The
  checkpoint runs the job queue to the end; an exception of a job is
  reported and the next job runs; a termination drops the remaining jobs.
  At its end the promises that the tracker reported are noted, and one
  task fires `unhandledrejection` for those that still have no handler
  (`Runtime::promise_is_handled`). The checkpoint also runs after each
  listener of an event that the engine fires while no script runs (HTML
  "clean up after running a callback"; `Page::dispatch_event`), after the
  callback of a timer (before an interval repeats) and never inside a
  checkpoint. The `error` event of an exception report has no checkpoint
  between its listeners: Chromium runs their microtasks only after all
  of them (measured).
- (Feature 5b: timers.) `setTimeout`, `setInterval`, `clearTimeout`,
  `clearInterval` with string handlers (a classic script named after the
  document URL), extra arguments and the window as `this`; ids from 1, one
  more per call for both kinds, as in Chromium. A cleared timer leaves the
  queue at once. The nesting clamp follows HTML and Chromium (section 7).
  An interval keeps the grid of its first due time, as in Chromium
  (measured): after a slow callback the next run is due at the first beat
  after the start of the late task, so the missed beats are skipped and
  the later ones do not shift (HTML schedules from the end of the
  callback). `queueMicrotask` enqueues a job that calls the callback
  without arguments (`Runtime::queue_microtask`); `reportError` reports
  its argument. At most 1,048,576 active timers and extra arguments and
  32 MiB of string handlers per document (section 16).
- (Feature 5b: error reports.) HTML "report an exception": an uncaught
  exception of a script, a timer, a listener, a microtask or a cleanup
  fires a cancelable `ErrorEvent` at the window (message `Uncaught
  Name: message` as Chromium words it, `error` the thrown value, a
  `SyntaxError` object for a compile error); a console line follows only
  if no listener canceled it. While the window is in error reporting mode
  (an `error` listener threw), the report goes to the console only.
  `filename`, `lineno` and `colno` give where an error object was created
  (the first frame of its stack, `Runtime::error_location`), else where
  the value was thrown (`Runtime::throw_location`), else the place of a
  compile error; lines count from the start of a script's own text, not
  of the document (a departure for inline scripts until feature 5c knows
  their positions; since feature 5c they count from the document). A listener whose `handleEvent` is not callable gets a
  console line and no `error` event, as in Chromium (measured; HTML would
  fire one). `unhandledrejection` is a cancelable `PromiseRejectionEvent`;
  not canceled, it gives `Uncaught (in promise) ...`. `rejectionhandled`
  is not fired.
- (Feature 5b: the clock.) `PageClock` (one per page, shared by its
  documents) counts page time in real time or in virtual time, from a
  start time (the system time or `swb --now MS`). In virtual time the
  clock moves only in the engine's blocking loops (`run_event_loop` for
  `--settle-ms`, `wait_until_idle` for `page.waitForIdle`, the wait of an
  evaluation), when no task is due and no request is pending; it stands
  still while a task runs (a script that busy-waits on `Date.now()` runs
  into the time limit) and moves 0.01 ms after each task (so that a chain
  of tasks that are due at once does not keep it still). `Date.now()` is
  the wall time of the page time (`Runtime::set_clock`), or in real time
  without `--now` the system time (it counts the time that the machine
  was suspended); `performance.now()`, `performance.timeOrigin`
  and `Event.timeStamp` count from the document's time origin (the page
  time when its runtime is created) in steps of 0.1 ms, as Chromium does
  without cross-origin isolation (measured). `Performance` has `now`
  and `timeOrigin` only (no `toJSON`, no marks).
- (Feature 5b: limits.) A runtime that reached the heap limit is marked
  failed; the engine drops it after the task (the DOM stays, evaluations
  answer that the scripts stopped). The budget of long tasks is in
  section 16.
- (Feature 5b: evaluation.) An evaluated promise gets a host reaction
  (so its rejection is handled) and waits in the binding state;
  `Page::evaluate` runs the event loop until it settles or its time-out
  passes (30 s; `--eval` uses `--timeout`), and the automation server
  answers `page.evaluate` when it settles, without blocking other
  requests in real time.
- (Feature 5b: changes to `js`.) `Runtime::queue_microtask`,
  `promise_is_handled`, `error_location`, `throw_location` and
  `source_location`; the VM keeps the code of the frame of the last throw
  with its offset, boxed so that the `Vm` keeps its size (one more word in
  `Vm` made the `vm_bench` row "1M small objects" 45 % slower, measured).
  Nothing else on the engine's paths changed.

Feature 6a notes (tree changes, attributes and node creation; measured
with `tools/probes/web-dom-mutation.json`: 476 cases, three Chromium runs
with equal results; the Rust tests are `crates/web/tests/dom_mutation.rs`
with the same values and `crates/engine/tests/script_dom.rs`):

- Code: `web/src/dom/` has `tree_ops.rs` (pre-insert, replace, remove and
  the `ParentNode` and `ChildNode` methods), `element.rs`, `attrs.rs`
  (the attribute algorithms, `Attr`, `NamedNodeMap`), `token_list.rs`
  (`classList`), `string_map.rs` (`dataset`), `lists.rs` (`NodeList` and
  `HTMLCollection`), `character_data.rs` and `document.rs`. `dom` has
  `algorithms.rs`: `clone_node`, `is_equal_node`, `compare_position`,
  `normalize` and the reset of form owners, all iterative.
- **Exceptions.** The checks of "ensure pre-insertion validity" run in
  `web` first, in Chromium's order and with its messages (a text node
  in a document, a second element or doctype, an element before a
  doctype, a node that contains the parent, a reference child that is
  not a child); the change functions of `dom` check again. The glue adds
  `Failed to execute 'NAME' on 'INTERFACE': ` to every exception, so the
  hand-written code states only the rest. Mixin methods use the name of
  the interface that includes them (`append` on `Element`).
- **Hooks of scripts (HTML §4.12.1.1).** Every insertion calls
  `scripts::after_insertion` with the nodes it added (the children of a
  fragment, read before the insertion). `scripts::attribute_changed` runs
  for each attribute that a script adds: `async` clears "force async",
  `src` runs the post-connection steps. A change of text in a `script`
  (`data`, `nodeValue`, `textContent`, `appendData` ...) runs the
  children-changed steps of the parent. `cloneNode` copies "already
  started" into the DOM flag of the clone. The search for inserted
  scripts and the test of the connection (a walk up the parent chain)
  advance in turns and stop when one decides (no script, or a root that
  is no document), so an insertion into a detached parent costs
  O(min(depth of the parent, size of the subtree)) and a chain of
  100,000 nested elements built top-down or bottom-up stays linear. The
  engine's journal walk skips a subtree that an earlier entry of the
  same drain visited, for the same reason.
- **Attributes.** The element keeps its attributes; an `Attr` is a view
  (element, name) in a table of the binding state, so
  `el.getAttributeNode('a') === el.attributes[0]`; a removed attribute
  leaves its object detached with its last value. Departure: `Attr`
  extends `EventTarget`, not `Node`. Names follow Chromium, which is looser
  than the DOM standard: `setAttribute`, `toggleAttribute` and
  `createAttribute` refuse only an empty name and ASCII whitespace, NUL,
  `/`, `>` and `=`; `createElement`, `createElementNS` and the NS methods
  use the XML name start rule for each part of a qualified name.
  `setAttribute` lowercases the name for an HTML element.
  `createElement` and `createElementNS` accept `{is}` and ignore it:
  Chromium 148 sets no `is` attribute (measured, also for `{is: null}`).
  Departure from Chromium, as the DOM standard: `setAttribute('Q:C', v)`
  changes an existing attribute `q:c` of another namespace (made with
  `setAttributeNS`); Chromium adds a second attribute (probe case m451,
  `SWB_VALUES`). `textContent = s` keeps the node, as Chromium, when the
  only child is a Text node with the data `s`; every other case makes a
  new Text node.
- **Live objects.** `classList`, `attributes`, `dataset`, `childNodes` and
  `children` are `[SameObject]` (side table). A `DOMTokenList` parses its
  attribute at each call of a change and uses hash sets (`add` of
  100,000 tokens is linear); the reads (`length`, `item`, `contains`,
  indexed access) use the parsed tokens, which the binding state keeps
  for the DOM generation, so an indexed loop is linear; `add()` without a token creates the attribute (Chromium; the
  update steps say not to). `DOMStringMap` has no
  `[LegacyUnenumerableNamedProperties]`: `Object.keys(el.dataset)` lists
  the names, as in Chromium. `HTMLCollection.namedItem` looks at
  `id` before `name` (Chromium). Lists cache their nodes by DOM
  generation; the lists that hold a cache are registered, and the first
  list that builds one after a change drops the stale ones, so the old
  children are not kept alive.
- **Ids.** The id index finds the first element of an id among more
  than 8 by a walk of the tree; the result is kept for the DOM
  generation, so a loop of `getElementById` walks once.
- **Form owners.** The engine computes the form owner from the `form`
  attribute, the parser's form pointer and the ancestors at each
  rebuild. A script that moves a control or its subtree clears the
  pointers in the subtree (HTML "reset the form owner" unsets the parser
  flag), so the owner comes from the ancestors afterwards. Invariant: the
  parser sets the pointers only through crate-private functions, and each
  removal by a public function (`remove`, `remove_child`, `replace_child`,
  `replace_all`, `adopt`) clears them in the removed subtree, so a subtree
  whose root has no parent holds none and the insertion of such a root
  needs no walk (the children of a fragment, and nodes with a parent, are
  walked).
- **Not done:** `[Unscopable]` (the generator has no `@@unscopables`),
  `new Document()`, `createCDATASection`, `createProcessingInstruction`,
  `Attr` as a `Node`, shadow roots, `slot`. The UTF-16 offsets of
  `CharacterData` that fall inside a surrogate pair round up to the end of
  the character (the DOM string cannot hold a lone surrogate).

Feature 6b notes (queries and markup; measured with
`tools/probes/web-dom-query.json`: 78 cases, 406 values, three Chromium runs
with equal results; the Rust tests are `crates/web/tests/dom_query.rs` with
the same values, and two tests in `crates/engine/tests/script_dom.rs`):

- Code: `web/src/dom/` has `queries.rs` (`querySelector`, `querySelectorAll`,
  `matches`, `closest`, `webkitMatchesSelector`), `markup.rs` (`innerHTML`,
  `outerHTML`, `insertAdjacent*`), `implementation.rs` (`DOMImplementation`,
  `DOMParser`, `template.content`) and a `lists.rs` that knows more kinds of
  list. `dom` has `inner_html`, `parse_document`, `count_elements_by_id`, the
  quirks mode and the scripting flag per document. `style` has `query.rs`
  (`Matcher`). `web` now depends on `css` and `style`; the engine passes the
  element states through `EngineHost::element_states`.
- **Selectors.** The parser is `swb_css`'s and the matcher `swb_style`'s;
  there is no second matcher. A `Matcher` costs no pass over the document
  (`StateSet::lazy`: class names are read from the elements, the disabled
  state of an element from its ancestors), so `matches` in a loop is linear
  in the loop. Parsed lists are cached by text (256 entries). The states
  that depend on the form controls (`:checked`, `:valid`, ...) are current
  when the selector reads them (the engine rebuilds the control states
  first, only then); `:hover`, `:focus` and `:target` are the engine's
  current states, `:visited` never matches. `:scope` is the start element
  of an element query; a query on a document or a fragment has none (the
  root element for a document, nothing for a fragment). `:root` is the
  element child of any document node (it was the document element of
  `NodeId::DOCUMENT`). Errors: `SyntaxError` with `'X' is not a valid
  selector.`, or `The provided selector is empty.` for the empty string
  (whitespace is not empty); a pseudo-element matches nothing. The match of
  one element has a budget of 200,000 ancestor steps (`MATCH_BUDGET`), and
  one query stops at 100,000,000 steps in all with a warning and returns
  what it found (`closest` returns null): a descendant selector that fails
  high in a chain of 100,000 levels walks all ancestors of each element, a
  square of the depth (about 0.7 s for 100,000,000 steps). A step is an
  ancestor or sibling step of the matcher, and also each selector of a
  list that a match tries, so a list of 200,000 selectors on thousands of
  elements is bounded too. `closest` and the loops of `querySelector` and
  `querySelectorAll` all check the bound. Departure: swb refuses
  `p::-webkit-foo`, Chromium accepts unknown prefixed pseudo-elements (not
  in the probe).
- **Lists.** `ListKind` has `getElementsByTagName` (the qualified name;
  lowercase for HTML elements), `getElementsByTagNameNS`, `getElementsByClassName`
  (ASCII case-insensitive in quirks mode; an empty class set is an empty
  list), `getElementsByName` (a live `NodeList` of HTML elements), the
  document collections (`images`, `links` with `href`, `forms`, `scripts`,
  `anchors` with `name`, `embeds`, `plugins`, `applets` always empty; HTML
  elements only), and `querySelectorAll` (a static `NodeList`). The
  cache per DOM generation is the one of `children`. Lists made by
  `getElementsBy*` are not registered for the drop of stale caches (the
  registry holds strong handles and these lists can be collected); the
  document collections and `children` are (they live as long as their
  node). A list builds its cache lazily: a read of item `i` walks the tree
  only as far as item `i` and keeps what it found; `length` of a
  `getElementsBy*` list or a document collection walks to the end. The
  cache is valid for one DOM generation (one counter for the whole
  document), so a DOM change drops it. `length` of `children` and
  `childNodes` needs no cache: `dom` keeps the number of children and of
  element children in each node (the link and unlink primitives update
  them), and an item of these two lists is found from the nearer end. So
  the drain loops `while (r.children.length) r.removeChild(r.children[0])`
  (and the same with `childNodes`, or with the last child) cost O(1) per
  step. A loop that reads item `i` of `children` and changes the tree at
  each step (`for (i) r.children[i].innerHTML = ...`) walks `i` siblings at
  each step; a loop `while (l.length) l[0].remove()` over a
  `getElementsByTagName` list walks the tree at each step (its `length`).
  The cost of these loops is not compared with Chromium's. Departure: Chromium returns the same
  collection for the same arguments while it lives; swb makes a new one
  (probe `collections-01`, query 5).
- **`window[name]`.** One element gives the element; several (elements with
  the id, and `embed`, `form`, `img`, `object` elements with the `name`,
  an element whose id equals its name counted once) give a live
  `HTMLCollection`. `Document::count_elements_by_id` asks the id index, and
  the `name` cache holds the first element and the count of each name.
- **Serialization.** The tag name is always the qualified name
  (`x:y`, `s:g` for SVG, `H:P`): Chromium does not use the local name for the
  HTML, SVG and MathML namespaces as the algorithm does. A `pre` gets no
  extra line feed. Attributes are written as the algorithm says (`xml:`,
  `xmlns:`, `xlink:`, the prefix); `<`, `>`, `&`, `"` and U+00A0 are escaped
  in attribute values, `&`, `<`, `>` and U+00A0 in text. The text of
  `script`, `style`, `xmp`, `iframe`, `noembed`, `noframes`, `plaintext` is
  raw, and that of `noscript` only if scripting is enabled for its node.
  Comments with `--` are written as they are; a processing instruction made
  by the HTML parser is a comment. The walk is iterative (a tree of 100,000
  levels reads in 5 ms).
- **Scripting and documents.** Scripting is enabled only for nodes whose
  node document is `NodeId::DOCUMENT` (`Document::scripting_for`). So
  `noscript` in the fragment parser and in the serializer is markup in
  `DOMParser` documents, `createHTMLDocument` documents and template
  contents; also `template.innerHTML = '<noscript>..'` parses markup, as
  Chromium (a `template` context uses the contents' flag). `innerHTML` of
  other elements of the main document keeps `noscript` content as text.
  Each document of the arena has its own quirks mode (`DOMParser` of
  markup without a doctype is in quirks mode, `createHTMLDocument` is not),
  which the class selector, `getElementsByClassName` and the fragment
  parser read.
- **Markup setters.** The fragment parser takes the context element; the
  nodes go into the arena under a new fragment and enter the tree by the
  ordinary insertion (`replace_all`, `replace`, `pre_insert`), so the
  journal, the mutation sink and `scripts::after_insertion` see them. Scripts
  in fragments are "already started" (the sink marks them), so no script
  of markup runs, also after a clone or a move. Nodes are never freed, so
  the setters give the empty fragment back to the document after the
  insertion, and the next parse reuses it (a stand-in `body` context of
  `insertAdjacentHTML` is also one node per document). A write costs the
  nodes of its markup; with a one-element markup (`<p>x`) 2 nodes, so 500,000
  writes fill the arena. A parse that stops at a
  limit of the arena ends the task (node or byte limit). `innerHTML` of
  a `template` fills its contents; `outerHTML` of an element without a
  parent, or whose parent is a document or fragment, throws
  `NoModificationAllowedError` (Chromium does not use a `body` context for
  a fragment parent there); `insertAdjacentHTML` with `beforebegin` or
  `afterend` on a parentless element throws `NoModificationAllowedError:
  The element has no parent.` (also for a document parent), and with a
  fragment parent or an `html` context parses as `body`.
  `insertAdjacentElement` returns null and `insertAdjacentText` does
  nothing without a parent. The position is ASCII case-insensitive, with
  Chromium's `SyntaxError` message. `outerHTML = ...` joins text nodes as
  Chromium does: the last new node (or the previous sibling, if the markup
  made no node) with a text node that follows, then the first new node with
  a text node that precedes (`a<span></span>b` with `outerHTML = 'X'` is
  the one text node `aXb`, the node `a`); the other setters leave adjacent
  text nodes alone.
- **`DOMParser`, `DOMImplementation`, `template.content`.**
  `parseFromString` for `text/html` runs the parser with scripting off into
  a temporary arena and copies the nodes under a new document node (the
  scripts are "already started"; `URL` is the window document's URL, as in
  Chromium; `defaultView` is null). The XML types throw `NotSupportedError`
  (swb has no XML parser; Chromium returns an `XMLDocument`); other values
  throw Chromium's `TypeError` (`SupportedType` enum). `createHTMLDocument`
  makes the doctype, `html`, `head`, `title` if a title is given (also an
  empty one; `undefined` gives none), `body`; its `URL` is `about:blank`.
  `createDocumentType` refuses ASCII whitespace, `>` and NUL in the name
  (measured; `/`, `:`, `<` pass). `hasFeature()` is true. There is no
  `createDocument`. `readyState` of these documents is `complete` (Chromium
  says `loading` for `createHTMLDocument`; not in the probe). `baseURI` of a
  node in these documents is the base URL of the window document; Chromium
  follows a `base` element of the new document (departure, measured). The
  table `innerHTML` with a `thead` first drops a later `tbody`, `tfoot` or
  `thead` token in the fragment parse (a departure that comes from the
  parser dependency; see the backlog). The engine
  renders and loads nothing in these documents: its journal walk and the
  `in_document` filters look at the tree of `NodeId::DOCUMENT` only (engine
  tests `documents_that_scripts_parse_are_not_rendered_or_loaded`), and a
  node that moves into the page is rendered and loads. `template.content`
  returns the fragment of the inert document that `ElementData` holds;
  `content` is read-only.
- **Not done:** the XML types of `DOMParser`, `createDocument`,
  `XMLSerializer`, `Range.createContextualFragment`, `document.all`,
  `shadowRootMode` and the other members of `HTMLTemplateElement`,
  `setHTMLUnsafe` and `getHTML`, the property reflection of `form`, `img`,
  `input` and the other element interfaces (6c), `focus()` and the event
  members (6d), `HTMLDocument` and `Location`.

Feature 6c notes (the HTML element interfaces and form controls; measured
with `tools/probes/web-html-elements.json`: 65 cases, 733 values, three
Chromium runs with equal results; the Rust tests are
`crates/web/tests/html_elements.rs` and `crates/engine/tests/script_html.rs`):

- **Interfaces.** `idl/html.idl` has an interface for every element that
  Chromium has one for, `HTMLUnknownElement` and `HTMLDocument` (swb's own
  line: every document is an `HTMLDocument`). `state.rs` picks the class
  of a wrapper with `html::class_of_html`, a table by tag name (a hash map
  built once). A name outside the table is `HTMLElement` if it is a valid
  custom element name, else `HTMLUnknownElement`. The constructors of the
  element interfaces throw `Illegal constructor` (`[HTMLConstructor]`
  makes the generator write a constructor that throws; swb has no custom
  elements). The obsolete members (`align`, `bgColor`, ...) are not there.
- **Reflection.** The generator got `[ReflectSetter]` for `long` and
  `unsigned long`, `[SwbEnumerableNamedProperties]`, `[HTMLConstructor]`,
  and the empty trait gets its implementation from the generated code
  (no hand-written empty `impl`). The enumerated attributes have
  `[SwbKeywords]` with the missing and invalid value defaults that the
  probe measured. Departures from the standard that Chromium also has:
  `img.width` and `height` reflect the content attribute (the standard
  returns the rendered size), `input.width` and `height` are 0 except for
  `type=image`, `autocomplete` is limited to valid autofill tokens,
  `style.disabled` is not reflected (it belongs to the style sheet).
- **Form state.** The value of a text field, the checkedness of a checkbox
  or radio button and the selectedness of options are the engine's
  (`crates/engine/src/forms`, `document/script_forms.rs`).
  `EngineHost` has `control_value`, `set_control_value`, `control_checked`,
  `set_control_checked`, `selected_index`, `set_selected_index`,
  `option_selected` and `set_option_selected`. A value that a script
  writes to a control without a state (a new `input`, a control in a
  document whose states are not rebuilt) is a *seed*
  (`forms/script.rs`); a rebuild applies the seeds, and keeps the dirty
  state of controls that left the document. A read or write of a value or
  of the checkedness of a checkbox costs O(1) after a DOM change; those of
  a radio button and of a select rebuild the states first (the group and
  the options need them). A write sets `controls_stale`; `refresh_forms`
  (called by every flush) then computes the element states again and
  restyles only if a selector depends on them. Checked radio buttons of a
  group: the newest checkedness wins (a `stamp` per control); among
  equal ones the last in tree order (this closes the backlog item
  "after a rebuild, the last in tree order wins"). A select whose
  options no `selected` attribute decides keeps its selected option when
  options are added (the setting algorithm picks the first option only
  when none is selected).
- **Review fixes (6c1).**
  *Budget of values.* A value copy lives in the engine, outside the heap
  of the runtime and outside the DOM bytes, so it has its own budget:
  `DocumentState::control_bytes` counts the dirty values of the states
  and the values of the seeds (recomputed at each rebuild, adjusted at
  each write). A write past 32 MiB stores nothing, logs a warning and
  returns `ControlWrite::OverLimit`; the bindings end the task
  (`html::write_control_value`). The number is small because a value of
  a visible control costs 70 to 180 bytes per character in layout
  (measured: a 5 MB textarea takes 350 MB, `ab ` repeated 1.1 GB), so a
  larger budget could not be laid out; a page that keeps 32 MiB in
  controls is outside the targets. The layout cost itself stays a
  backlog item.
  *Selectedness outside the document.* A select or an option that left
  the document (or that went into a template or another document) keeps
  what it held: a read or write of a select without a place in the
  document checks whether the old states still have an entry for it, and
  rebuilds first if so (the rebuild turns the dirty state of an orphan,
  and the selectedness of options that left a select that stays, into
  seeds). A select that never had a state costs no rebuild.
  `refresh_forms` is split: `ensure_forms` rebuilds if the DOM changed,
  and only `refresh_forms` (flush, queries) computes the element states;
  a loop of writes does not compute them per write.
  *Cloning steps.* `Cloned::controls` lists the pairs of `input` and
  `textarea`; `EngineHost::copy_control_state` copies a dirty value and a
  dirty checkedness to the copy (`cloneNode`, `importNode`, template
  contents).
  *Radio buttons.* A write of `checked=true` to a radio button in a
  document whose states are outdated is a seed (no rebuild per write).
  A rebuild stamps the seeds: the buttons that were in the document when
  a script checked them come first, in the order of the writes, then the
  buttons that were outside it, in tree order; all are newer than the
  existing checked buttons (an inserted button wins, as in Chromium).
  Departure: a button that a script checked in the document, removed and
  inserted again does not win over a button that was checked in
  between (the order of the insertion is not known without a hook).
  *Option text.* `option.text`, `value` and `label` skip `script`
  descendants of the HTML and SVG namespaces (`text_content_without_scripts`).
  *Range.* The value sanitization of `type=range` clamps to `min` and
  `max` (`max` below `min` counts as `min`), rounds to the nearest step
  (`step` default 1, from `min`; `any` has none) and writes the number
  (`10.0` gives `10`). It also applies to the `value` attribute. Departure:
  Chromium clamps the stored value again when `max` changes; swb keeps the
  text that the script wrote and sanitizes it at each read, so a later `max`
  can give the old text back.
  *Small items measured in Chromium.* `:indeterminate` matches a checkbox
  whose `indeterminate` property is true (`EngineHost::set_control_indeterminate`)
  and a radio button whose group has no checked button; a change of the
  `type` attribute from a type with the value mode `value` to one with the
  mode `default` or `default/on` writes the value to the `value`
  attribute (`dom::set_attribute` calls `before_type_change` and
  `after_type_change`); `select.add(option, before)` throws
  `NotFoundError` unless `before` is a child of the select (the standard
  says descendant); the `host` and `hostname` setters cut the value at
  `/`, `\`, `?` and `#`, and the `hostname` setter ignores a value with a
  port; `inputMode` and `enterKeyHint` are enumerated attributes of
  `HTMLElement`; `tabIndex` is -1 for an element with `contenteditable`
  (the standard says 0). Left as they are: a space in a host
  (Chromium writes `%20`), a disabled option on a select whose state is
  not built yet (the first build sees the attribute; Chromium selected
  the option before), the implicit selection of an option that leaves a
  select whose state no read has built, `setAttribute('selected')` on a
  select that the user changed (no per-option dirty flag), the two radio
  quirks (an ungrouped scripted radio next to an attribute-checked one
  keeps both checked; a renamed checked radio is unchecked in Chromium),
  `img.height = 1e10` (feature 7b).
- **Lists.** `ListKind::Element(ElementList)` in `dom/lists.rs`:
  `table.rows` (head rows, body rows, foot rows), `tBodies`, `tr.cells`,
  `select.options` (`HTMLOptionsCollection`), `selectedOptions` (built
  again at each access: the engine's state is not part of the DOM
  generation), `form.elements` (`HTMLFormControlsCollection`, one pass
  with the nearest form ancestor tracked), `fieldset.elements`,
  `datalist.options`, `map.areas`. A collection finds `namedItem`
  through an index by `id` and `name` that is built with the list of
  supported names. `RadioNodeList` is a list with the nodes of one name.
- **Form named properties.** `form[name]` and `form[index]` read
  `form.elements`. Departure: `form` has no `[LegacyOverrideBuiltIns]`: a
  control named `submit` or `action` does not hide the member of the
  form. With the override every property read of a form (`f.appendChild`)
  has to know whether a control has that name, and the list of controls is
  rebuilt after each change (a loop that adds 8,000 controls took
  seconds; now 18 ms). The standard's `img` elements, image buttons and
  past names are not named properties.
- **Cost.** The interfaces are created when the runtime is created:
  1.55 ms and 464 KB of heap before, 1.57 ms and 725 KB after (senko.net,
  release build). That is below the limit of about 1 ms or 1 MB, so no
  lazy creation.
- **Not done:** the obsolete members, the selection members of `input` and
  `textarea`, constraint validation, `files`, `valueAsNumber`, `labels`,
  `iframe.contentWindow`, `canvas.getContext`, the media playback members,
  `meter` and `progress` values, `naturalWidth`, `complete` and
  `currentSrc` from the loaded image (they are 0, true without a source and
  false with one). `Image`, `Option` and the SVG interfaces came with
  feature 6c2 (next notes).

Feature 6c2 notes (`new Image()`, `new Option()` and the SVG interfaces;
measured with `tools/probes/web-svg-elements.json` (9 cases, 156 values) and
`tools/probes/web-image-option.json` (5 cases, 112 values), three Chromium runs
each with equal results; the Rust tests are `crates/web/tests/svg_elements.rs`,
`image_option.rs`, `crates/engine/tests/script_html/option_cases.rs` and two
tests in `crates/engine/tests/script_dom.rs`):

- **`[LegacyFactoryFunction]`.** The generator accepts
  `[LegacyFactoryFunction=Name(arguments)]` on an interface (Web IDL §3.3.3,
  §3.7.2). For each one the trait gets `legacy_factory_<name>` with the
  arguments, the glue gets `construct_<name>` (a call without `new` throws
  Chromium's message with the factory's name; the arguments convert as for a
  constructor; the context is `Failed to construct 'Image'`), and `define`
  makes the function with `Runtime::define_constructor` and defines it on the
  global object (writable, not enumerable, configurable, as Chromium has
  it). `Runtime::define_constructor(realm, prototype, name, length, func)` in
  `js` is the only change there: a native constructor function whose
  `prototype` property (not writable, not enumerable, not configurable) is the
  interface prototype object, and which sees `new.target`.
  `Image.prototype === HTMLImageElement.prototype`; `Image.name`/`length`
  (`0`), descriptors and calls through `call`, `apply`, `Reflect.apply`
  match Chromium. `new.target` decides the prototype of the new object
  (`class A extends Image`, `Reflect.construct`), so `state.rs` has
  `wrap_node_with_proto`, which makes the wrapper of a node that has none
  with a given prototype. The element is made in the window's document
  (`create_node`), so its node document is the current document.
- **`Image` and `Option`.** `html/factories.rs`. `Image(width, height)` sets
  the `width` and `height` attributes of the new `img` (a value above
  2147483647 sets 0, as Chromium: `new Image(-1)` gives `width="0"`).
  `Option(text, value, defaultSelected, selected)` appends a text node for
  a text that is not empty, sets `value` if the argument is not missing,
  `selected` if `defaultSelected`, and sets the selectedness when it differs from `defaultSelected`, without the
  dirtiness flag (HTML, `Option`; `set_option_initial_selected`):
  `new Option("a", "b", true).selected === false`, and an option added to a
  select with no selected option leaves the first one selected. Because
  the option is not dirty, adding or removing its `selected` attribute
  (`defaultSelected`, `setAttribute`, `removeAttribute`) sets the
  selectedness again (`option_selected_attribute_changed`, called by the
  attribute funnel of `dom/attrs.rs`); a write of `option.selected` makes
  it dirty. The engine keeps the clean selectedness as a seed (before the
  option is in a select) and as a mark (after the select applied it);
  a setting of the same attribute value keeps it. Limit: the engine has
  no dirty flag per option, only per select, so the selectedness of the
  other options of a select that a script or the user changed does not
  follow their attribute. Departure:
  `new Image(undefined, 5)` gives no `width` attribute (Web IDL: an
  undefined optional argument is missing); Chromium sets `width="0"`. An
  `img` that `new Image()` makes and that has a `src` loads when it goes
  into the document, not before: the engine selects the sources of the
  elements in the document, and a detached image has no effect that a script
  can read (`complete`, `naturalWidth` and `load` do not exist yet). The
  backlog item stays.
- **SVG interfaces.** `idl/svg.idl` (the W3C Software and Document License
  allows the copy, ADR 0003): 38 element interfaces and `SVGAnimatedString`.
  `state.rs` picks the class of an SVG element with `svg::class_of_svg`, a
  table by local name (the same lookup as for HTML; a name outside the table,
  also a name with a different case, `SVGElement`). The parser's names
  (`foreignObject`, `clipPath`, `linearGradient`) match. What is cut, and why,
  is the header of the file: the animated value interfaces except
  `SVGAnimatedString` and the members that use them (`x`, `width`,
  `viewBox`, `transform`, `points`, `href`), the members that need layout or
  geometry types that swb has not (`getBBox`, `getCTM`, `getTotalLength`,
  `createSVGRect`, ... : feature 7b), the filter and animation elements
  (`SVGElement`), event handler attributes (6d), `focus` and `blur`.
  `SVGElement` has `className`, `ownerSVGElement`, `viewportElement` and
  the `HTMLOrSVGElement` mixin (`dataset`, `nonce`, `autofocus`, `tabIndex`):
  the same code as `HTMLElement`, so `dataset` and `classList` on SVG
  elements work and `tabIndex` is -1 (0 for `a`, as measured).
- **`className`.** An `SVGAnimatedString` is a platform object whose host
  data is the element (`Slot::ClassName`, one object per element). `baseVal`
  reads the `class` attribute (also a missing one: `""`); a write sets the
  attribute to the string as it is (an empty string leaves
  `class=""`; `null` gives `"null"`); `animVal` reads the same and has no
  setter (without animation it equals `baseVal`). The object is live: it
  follows `setAttribute`, `removeAttribute`, `classList`. The setter of
  `className` does not exist (Chromium: getter only, and a strict-mode
  write throws). `baseVal` is a `DOMString`; the standard has
  `(DOMString or TrustedScriptURL)`.
- **`ownerSVGElement` and `viewportElement`** (measured, not the standard's
  text): `ownerSVGElement` is the nearest `svg` ancestor, also through HTML
  elements (`foreignObject > div > g`); an `svg` element whose parent is not
  an SVG element or is a `foreignObject` (an outermost `svg`; also one in a
  `div`) has none. `viewportElement` is the nearest ancestor that is an `svg`, `symbol` or
  `image` element (not `marker`, `pattern`); a detached element has none.
- **Defaults.** `SVGStyleElement.type` is `text/css` and `media` is `all`
  without the attribute (measured); `SVGScriptElement.type` is `""`.
  `SVGSVGElement.getElementById` finds the first descendant with the id
  (the svg itself and elements outside are not found). `SVGStyleElement.
  disabled` is false and a write has no effect (Chromium: no attribute);
  swb has no sheet state for it.
- **Prototype of `new.target`: a departure for every host constructor.**
  When `new.target.prototype` is not an object, Web IDL (internally create a
  new object implementing the interface) takes the interface prototype
  object of the realm of `new.target`. Chromium takes `Object.prototype`,
  for `Image`, `Option` and `Event` alike (`Reflect.construct(Image, [], F)`
  with `F.prototype = 5`). swb follows the standard, in the one helper
  `prototype_from_new_target` that all host constructors share, so the
  departure applies to all of them. Two cases of `web-image-option.json`
  (`option-02`, 17 and 18) record the Chromium values; the test overrides
  them. The factory functions read `new.target.prototype` before they make
  the node, so a getter that throws leaves no node in the arena.
- **Child lists and the structure generation.** `Document` has a second
  counter, `structure_generation`, that `record` bumps only for a change of
  a child list (`Change::Children`; every insertion and removal goes
  through `record`, and the links that bypass it build copies in nodes that
  have no list yet). The caches of `children`, `childNodes` and the lists
  of children (`tBodies`, section `rows`, row `cells`) record this counter,
  so an attribute or text write keeps them and `for (const e of el.children)
  e.setAttribute(...)` is linear (100,000 children: 5.4 s before, 0.42 s
  after, the same as a loop over `Array.from`). The lists that depend on
  attributes (`getElementsBy*`, the document collections, `form.elements`,
  `select.options` and the others) and the registry of stale caches keep
  the full generation. The supported property names of an
  `HTMLCollection` (`id` and `name` lookups) record the full generation
  separately, because an `id` write changes them without changing the
  nodes. A unit test counts the rebuilds (`REBUILDS`).
- **Cost.** 39 more classes (38 element interfaces and `SVGAnimatedString`)
  and the two factory functions are created with the runtime: 1.51 ms and
  727,173 B of heap before, 1.56 ms and 763,589 B after (senko.net, release
  build, best of 5; the noise is 1.5 to 2.4 ms). That is far below the limit
  of about 1 ms or 1 MB, so no lazy creation.

Feature 6d1 notes (the event interfaces, the event handlers, focus, `click()`
and the form methods; measured with `tools/probes/web-input-events.json` (7
cases, 149 queries) and `tools/probes/web-input-handlers.json` (10 cases, 234
queries), three equal Chromium runs each; the Rust tests are
`crates/web/tests/input_events.rs` and `crates/engine/tests/script_input.rs`).
6d2 (the engine's own input as DOM events) is not part of it.

- **IDL.** Four new files in `crates/web/idl/`, all with the source, the copy
  date, the license and the cuts in their headers: `ui-events.idl` (UI Events,
  W3C Working Draft 21 February 2026: `UIEvent`, `FocusEvent`, `InputEvent`,
  `KeyboardEvent`, `CompositionEvent`, `TextEvent`), `pointer-events.idl`
  (Pointer Events 4, CSSOM View and Pointer Lock 2 for `MouseEvent`;
  `WheelEvent`, `PointerEvent`, the pointer handlers),
  `transition-device-events.idl` (`TransitionEvent`, `DeviceMotionEvent`,
  `DeviceOrientationEvent`) and, in `html.idl`, the three handler mixins,
  `SubmitEvent`, `HashChangeEvent`, `MessageEvent`, `StorageEvent`,
  `BeforeUnloadEvent`, `DragEvent`, `FocusOptions`, `focus()`, `blur()`,
  `click()`, the form methods, `activeElement` and `hasFocus()`. The
  `createEvent` names are those that Chromium 148 accepts: the DOM table
  without `TouchEvent` (Chromium without touch input throws
  `NotSupportedError`; so do `PointerEvent`, `WheelEvent`, `InputEvent` and
  `SubmitEvent`), plus `TransitionEvent`. The names are not case sensitive.
  Where the standards differ from Chromium the IDL follows Chromium: the
  members of `MouseEvent` are in Chromium's order, `keyCode` and `charCode`
  are `long`, `WheelEvent` has `wheelDeltaX`, `wheelDeltaY` and `wheelDelta`
  (and the init members of the first two) and no `momentum`,
  `MessageEvent.ports`, `StorageEvent.storageArea`, `DragEvent.dataTransfer`
  and the device events' objects are `object`, `UIEvent.sourceCapabilities`,
  `InputEvent.dataTransfer` and `getTargetRanges()` are null and empty.
- **The state of the events.** `events/ui.rs`, `mouse.rs` and `misc.rs`. The
  UI family keeps one boxed `Fields` (`Extra::Ui`) with the members of all its
  interfaces and a `Family` that decides `which` and `getModifierState`; the
  other events keep a variant of `Other` (`Extra::Other`). A getter is one
  line (`field_getters!`). Measured: `which` is not initialized from the
  dictionary (0 for `UIEvent`; `button + 1` for mouse events, also
  `PointerEvent` (1); `keyCode` for `KeyboardEvent`); a `MouseEvent` floors
  `screenX`, `clientX` and the others (`-2.5` gives `-3`), a `PointerEvent`
  keeps fractions; `pageX` is `clientX` plus the scroll offset of the viewport
  at the time of the construction, if `view` is the window (not later:
  `EngineHost::scroll_offset`); `movementX` is truncated; `button` is a
  `short` (`-1` reads as 0); `x` and `y` are `clientX` and `clientY`;
  `fromElement` and `toElement` are the related target and the target,
  swapped for `mouseout` and `mouseleave` only (not for `pointerout`);
  `wheelDeltaX` and `wheelDeltaY` follow the truncated deltas unless the
  dictionary gives them, `wheelDelta` is `wheelDeltaY` or, if that is 0,
  `wheelDeltaX`; `PointerEvent` derives `tiltX`, `tiltY` from
  `altitudeAngle` and `azimuthAngle` and the reverse (the algorithms of the
  Pointer Events specification, tilt rounded: `altitudeAngle: 0.5` gives
  `tiltX` 61); a `KeyboardEvent` `initKeyboardEvent` leaves `key` and
  `keyCode` alone (its fifth argument is the old `keyIdentifier`) and the
  `init*` methods of the other events change only the members they name.
  The invalid `view` or `relatedTarget` messages are Chromium's
  (`Failed to read the 'view' property from 'UIEventInit': Failed to convert
  value to 'Window'.`).
- **Departures of the events.** `offsetX`, `offsetY`, `layerX` and `layerY`
  need layout: before the dispatch they are the rounded and floored page
  coordinates (as Chromium), and they stay so after it (Chromium makes them
  relative to the target and its layer). A script task has no font context,
  so no layout; feature 7b may add it. `MessageEvent.ports` is a new array
  at each read, not a frozen array, and holds any object (no `MessagePort`);
  `MessageEvent` has no `userActivation`; `PointerEvent` lists its members
  attributes first (the generator's order).
- **Event handlers.** `events/handlers.rs`, with the names in
  `handler_names.rs` (a unit test compares the table with the IDL). The
  generator writes the glue of the three handler types
  (`EventHandler`, `OnErrorEventHandler`, `OnBeforeUnloadEventHandler`) by
  itself: the getter and the setter call `Host::event_handler` and
  `Host::set_event_handler`; no trait method per attribute exists (that
  would be 480 methods that all do the same). A map from (target, name) to
  a value (`null`, an object, or the text of a content attribute) is in the
  binding state, traced; a non-null value has a listener in the target's
  list whose callback is one marker object and whose `handler` field names
  the handler, so that the dispatch calls `handlers::call` for it. A value
  keeps its place in the list while it changes; `null` removes the listener,
  a new value adds it at the end (Chromium, `handler-attrs` queries 18 to
  21). The setter keeps any object, also one that is not callable (the
  getter returns it; a dispatch ignores it) and makes every other value
  `null` (measured: `d.onclick = {}` reads back `{}`, `5` reads `null`).
  The text of a content attribute compiles at the first need: the getter, or
  the first dispatch (`Runtime::compile_event_handler`, ADR 0028 section 6
  item 6). The scope is the document, the form owner of a form-associated
  element (`form_owner` now answers only for listed elements and `img`), the
  element; the parameters are `event`, `evt` for an SVG element and five for
  `onerror` of the window. `body` and `frameset` forward `onblur`,
  `onerror`, `onfocus`, `onload`, `onresize`, `onscroll` and the
  `WindowEventHandlers` to the window (any `body` element, also one that is
  not in the document: as Chromium). The processing algorithm: `false`
  cancels, `true` cancels an `ErrorEvent` `error` at the window (five
  arguments), `onbeforeunload` cancels for any value but `null`/`undefined`;
  an exception is reported like a listener's (the console line, the `error`
  event). A compile error is reported with Chromium's message, which names
  the call that found it (`Failed to read the 'onclick' property from
  'HTMLElement': Unexpected identifier 'error'` for the getter,
  `Failed to execute 'dispatchEvent' on 'EventTarget': ...` or `'click' on
  'HTMLElement'` for a dispatch inside those calls), and the value becomes
  `null`. Changes in `js`: the lines of a handler count from its body (the
  source has the function header on line 1; `Source::start` is -1 and the
  line sum wraps), so `window.onerror` gets line 1 for an error on the first
  line of the attribute, as Chromium; and a body that ends the function early
  (`}), (function () {`) is `Unexpected token '}'`, as Chromium (which parses
  the body alone), not `Single function literal required` (that stays for the
  `Function` constructor).
- **When the attributes are read.** The parser sets attributes without the
  bindings, and a JavaScript-off page must not pay for handlers. So the
  `on*` attributes of an element are read the first time a script touches its
  listeners (a dispatch through it, `addEventListener`, the IDL attribute),
  in attribute order, and later changes go through the attribute funnel
  (`handlers::attribute_changed`, called by `set_attribute` and
  `remove_attribute` of `dom/attrs.rs`). Listeners of one type on one target
  are only observable after a touch, so the order is the one of Chromium
  (`handler-order`). The window syncs the `body` or `frameset` of the
  document at each touch, not once: a script in the `head` may add a listener
  before the body exists (Chromium: head listener, body attribute, body-script
  listener). Departure: an element that no script touched has no listener;
  `onerror="..."` on `img` fired by the engine's load error before any script
  touched it is dispatched by the engine through the same entry and so syncs.
- **Focus.** `html/focus.rs`, `EngineHost` (`focused_element`, `can_focus`,
  `set_focus`, `request_scroll_into_view`), `engine/src/document/
  script_actions.rs`. The focused element is the engine's
  `ElementStates::focus`. `can_focus` is the engine's rule
  (`focus::is_focusable`) plus "rendered" from the styles of the last flush
  (`display` is not `none` for the element and its ancestors, which have no
  style, and `visibility` is `visible`): no layout, since a script task has
  no font context. A focus change sets the states at once and marks the
  styles stale (`DocumentState::styles_stale`); the next style flush computes
  them. A loop of 100,000 `focus()` calls with a `:focus` rule therefore
  computes the styles once at the end and not 100,000 times (the check of
  `can_focus` uses the styles that exist). The event order is Chromium's,
  with its oddities (module comment of `focus.rs`): `blur`, `focusout`,
  `focus`, `focusin`; `activeElement` is the body during `blur`; a listener
  that moves the focus ends the change in progress (then `focusout` has a
  null `relatedTarget`, and `focusin` of the interrupted element does not
  follow). The events are trusted `FocusEvent`s (`view` the window,
  composed, `detail` 0). `focus({preventScroll})` asks the page for a scroll
  into view when the task ends (the page has the layout and the fonts);
  `focusVisible` defaults to true for a script. `blur()` on an element that
  is not focused does nothing. The "locked for focus" flag stops a `focus()`
  from inside its own events. The HTML focus fixup rule: a focused element
  that is removed, replaced, moved (an insertion takes it from its old
  parent: `appendChild`, `insertBefore`), cleared or disabled, or that gets
  the `hidden` attribute itself (also `until-found`), loses the focus with `blur` and `focusout`
  (`relatedTarget` null) at once, while it is still in the document for a
  removal (`before_removal` in `tree_ops`, `markup`); `display: none` does not
  blur until the next rendering update, and neither does `hidden` on an
  ancestor (Chromium; swb: the engine's fixup). `innerText` has no setter
  in swb yet, so its fixup is not done.
  `document.hasFocus()` is true for the window's document and false for the
  others; the window gets no `focus` or `blur` event. Departure: an
  element with `contenteditable` is not focusable (swb edits only form
  controls). An SVG `a` with a link is focusable now (the engine's rule
  changed).
- **Activation.** `html/activation.rs`, called from `events/dispatch.rs`.
  A `click` event that is a `MouseEvent` (`PointerEvent` too; a plain `Event`
  is not) has an activation target (the target, or with `bubbles` the nearest
  ancestor with an activation behavior: `a`/`area`/SVG `a` with `href`,
  checkbox, radio, submit and image inputs and buttons, reset buttons,
  `label`). Checkboxes and radio buttons run the legacy-pre-activation
  behavior before the listeners (the checkedness changes, `indeterminate`
  becomes false; a radio button remembers the checked button of its group)
  and the legacy-canceled-activation behavior if the event is canceled
  (restore), else `input` (`Event`, bubbles, composed, trusted) and `change`
  (`Event`, bubbles); a radio button that was checked and a control outside
  the document fire neither. All of this also holds for `dispatchEvent` and
  for a disabled checkbox (Chromium; only `click()` stops at disabled
  controls). A link asks the engine for the navigation
  (`EngineHost::request_hyperlink`): the page acts on it when the task
  ends (`Page::act_on_script_requests`: `follow_link`, so a fragment link
  scrolls and a canceled click does nothing). `click()` makes the
  synthetic event that Chromium makes: a `PointerEvent` with `pointerId` -1,
  not trusted, bubbling, cancelable, composed, `detail` 0, and keeps the
  click-in-progress flag per element. A label focuses its control (if it can
  be focused) and dispatches a synthetic click at it, unless the control is
  disabled. The label does nothing if the event came from the control or
  from inside it (HTML), and a label that is clicking its control does not
  start again (a per-label flag), so labels that nest each other or point
  into each other cannot recurse (a hostile case). Not done: the
  `summary`/`details` toggle by `click()` (the engine toggles it for the
  user's click), `input type=file`, `javascript:` links (6d2).
- **Forms.** `form.requestSubmit(submitter)`: a submitter that is not a
  submit button is a `TypeError` (`The specified element is not a submit
  button.`), one owned by another form `NotFoundError`; the submission runs
  "submit": nothing for a form that is not connected or not in the window's
  document, nothing while the form is firing its submission events (the
  flag), the interactive validation (the engine's `invalid_controls`; an
  `invalid` event at each, the first control gets the focus unless its event
  was canceled) unless `novalidate`/`formnovalidate`, the `submit` event
  (`SubmitEvent`, trusted, bubbles, cancelable, not composed), and if it was
  not canceled the engine's request (`EngineHost::request_form_submission`:
  the page navigates with `form_submission`, like a user's submit, when the
  task ends). `form.submit()` skips the validation and the event.
  `form.reset()` and a reset button: the `reset` event (`Event`, trusted,
  bubbles, cancelable), nothing inside its own `reset` event ("locked for
  reset"), and if not canceled `Forms::reset` (`EngineHost::reset_form`;
  the seeds of the form's descendants are forgotten, which also covers a
  form outside the document). A task that makes several navigation requests
  navigates to the last. Departures: the `name` of a control does not hide
  `form.submit` (no `[LegacyOverrideBuiltIns]`, 6c); `javascript:` form
  actions and `target`/`formtarget` are not looked at.
- **Cost.** A runtime costs 368 KB more heap (1,132,705 bytes against
  764,262) and about 0.9 ms more to create (3.1 ms against 2.3 ms, release,
  senko.net, five runs each), for the 480 handler accessors and 20 more
  interfaces. Runtime creation now takes about 1.13 MB and 3.1 ms in total.
  That is past the limit of about 1 ms or 1 MB, which applies to the total.
  The interface objects get lazy creation in a later feature;
  the roadmap backlog holds the item. JavaScript-off pages pay nothing. Hacker News and senko.net with JavaScript on load in 19 and 12
  ms (base 20 and 13), with the boxes and pixels of the JavaScript-off run.

Feature 6d2 notes (the engine's input as DOM events; measured with
`tools/probes/web-input-real.json` (45 cases after the fix round), three equal
Chromium runs, and compared with swb's `--steps` run: 0 differences; the Rust
tests are
`crates/automation/tests/real_input.rs` and `hacker_news.rs`, and
`crates/engine/src/input.rs` for the key table).

- **Task source and priority (the decision).** The input of the user is the
  task source "user interaction" of section 7. It has no queue: the embedder's
  call (`Page::mouse_move`, `mouse_down`, `mouse_up`, `wheel`, `key_down`,
  `key_up`, `insert_text`, `paste_text`, `focus`) is the task, and the page runs
  it at once, before any task of the other sources. The other sources run only
  in `Page::pump`, which the GUI and the servers call after they handled the
  input events that wait; so a due timer never delays a click, and a click
  never waits for a timer. Reasons: the embedders are single threaded and an
  OS event queue (winit's, the automation server's channel) is already the
  queue of this source; a second queue would only add a copy of the events and
  a rule for their order against `pump`. Each event of an input is one
  `dispatch_event` task (the document state is lent for it): the engine
  re-reads the DOM between two events, so a listener that removes the target
  or moves the focus changes what follows, as in Chromium; the microtask
  checkpoint runs after each listener and after each task, so promise jobs
  of a `mousedown` listener run before `mouseup`. A task that runs past the
  time limit ends as always (section 7); the input that follows still works.
  `scroll` is a rendering-update step, not a user task (below). When the
  document's scripts are stopped (the time limits), `Page::script_input()` is
  false and the input works as with JavaScript off: clicks activate links and
  controls, the focus moves, keys edit, all through the engine's own actions.
- **What an input does.** With a script runtime (not stopped, not lent):
  `mouse_move` fires, if the element under the pointer changed, `pointerout`,
  `pointerleave` (innermost first), `pointerover`, `pointerenter` (outermost
  first, the document included), then the four mouse events of the same
  kind, then `pointermove` and `mousemove`. A press first moves the pointer
  there if it is elsewhere; it fires `pointerdown`, `mousedown` (not after a
  canceled `pointerdown`), then the default action, then `selectstart`; the
  secondary button fires `contextmenu` after `mousedown` (Chromium on Linux).
  The release fires `pointerup`, `mouseup`, then `click` (or `auxclick`) at the
  nearest common ancestor of the targets of press and release, then `dblclick`
  for the second click. A disabled form control gets the pointer events only.
  A press and release that moved more than 4 px (a drag) fire no click (a
  departure: Chromium fires it at the common ancestor). A wheel turn fires
  `wheel` and scrolls unless it was canceled. A key fires `keydown`, then
  `keypress` (printable keys and Enter, not with Ctrl or Meta, not for Enter on
  a link), then the default action; `keyup` goes to the focused element or the
  body. The `Key` of the engine has no modifier keys, so Shift, Control, Alt
  and Meta alone fire no key events (departure; backlog).
- **Canceled defaults (measured).** A canceled `pointerdown` also suppresses
  `mousedown` and `mouseup` and the default of the press (no focus change, no
  selection); `click` still fires. A canceled `mousedown` keeps the focus
  where it is and starts no selection; `click` fires. A canceled `selectstart`
  starts no selection. A canceled `keydown` leaves out `keypress` and the
  default (no text, no focus move for Tab, no scroll, no implicit submission,
  no click of Space or Enter); `keyup` fires. A canceled `keypress` leaves
  out `beforeinput` and the insertion. A canceled `beforeinput` leaves out
  the edit and `input`. A canceled `click` leaves out the activation (a link
  does not navigate, a checkbox returns to its old state without `input` and
  `change`, a submit button does not submit).
- **The click and its activation.** The trusted `click` is a `PointerEvent`
  with `pointerId` 1 (the `click()` of a script has -1), `isPrimary` false,
  `isTrusted` true. The activation behavior of 6d1 runs in the dispatch of
  that event, so it replaces the engine's own action; the engine runs none of
  its own with scripts on, with one exception: the default summary of a
  `details` element without a `summary` element (no element to click) toggles
  in the engine after an uncanceled click. `summary` is now an activation
  target (it toggles `open`; `click()` on a `summary` toggles too, which
  closes a backlog item of 6d1). A label or a summary does not act when the
  click came from interactive content inside it (a text field, a select, a
  button, as the engine's rule for JavaScript off). The click that a label
  forwards to its control is trusted and has the `detail` of the click that
  caused it (measured: 1 for a user click, 0 for `label.click()`). A click
  whose target left the document during `mousedown` fires no click (the
  common ancestor is gone); a click listener that removes a link does not stop
  its navigation (measured).
- **Keys.** Enter on a link clicks it after `keydown`, without `keypress`.
  Enter on a button or a summary clicks it after `keypress`. Space on a
  button, a checkbox, a radio button or a summary clicks it when the key
  goes up (`keyup`, `Page::key_up`), only if the focus is still there. Enter
  in a text field first fires `beforeinput` (`insertLineBreak`, cancelable,
  also without a form), then `change` if the value changed, then the
  implicit submission (a trusted click at the default button, or `submit` at
  the form). The simulated clicks of keys are trusted, `PointerEvent`s with
  `pointerId` -1 and `detail` 0. The keyboard attributes are for a US layout
  (`crates/engine/src/input.rs`): `keydown`/`keyup` have `keyCode` 65 to 90
  for letters of both cases, `charCode` 0; `keypress` has the character code
  in `keyCode` and `charCode`; `code` is `KeyA`, `Digit1` (also for `!`),
  `Space`, ...; a character that is not on the layout has an empty `code`
  and `keyCode` 229 in the GUI. The automation API `input.type` inserts such
  a character as text without key events, as Playwright does (the other
  characters are key presses: `keydown`, `keypress`, `beforeinput`, `input`,
  `keyup`).
- **Edits and `change`.** `beforeinput` (cancelable) and `input` are
  `InputEvent`s, composed, bubbling, at the text field. `inputType` and `data`
  as Chromium: `insertText` (data the character), `deleteContentBackward`,
  `deleteContentForward`, `deleteWordBackward`, `deleteWordForward` (Ctrl),
  `insertLineBreak` (Enter in a text area, data null), `insertFromPaste` and
  `deleteByCut` (the GUI's `paste_text` and `cut_selection`). `beforeinput`
  fires also when the edit changes nothing (Backspace in an empty field,
  Delete at the end); `input` only when the value changed. A read-only or
  disabled field fires none. `change` fires when a text control commits: when
  it loses the focus (before `blur`; by a click, Tab, `blur()` or `focus()`
  of another element) and on Enter before the implicit submission, if the
  user edited it and its value now differs from the value before the first
  edit (typing `a` and deleting it fires none). The engine keeps one control
  with its value before the edit (`DocumentState::commit_baseline`, asked
  through `EngineHost::take_pending_change`); a script that sets `.value`
  does not reset it (not measured).
- **Focus.** The user's focus changes (click, Tab, label, the fragment of a
  link) go through the focusing steps of 6d1 (`ScriptRuntime::user_focus`):
  `blur`, `focusout`, `focus`, `focusin` with the related targets, `change`
  first for an edited field; `Page::focus` runs them, so does everything
  that called it. A click on a part of the page that is not focusable
  removes the focus (with the events). When Tab leaves the page after its
  last element (or before the first for Shift+Tab), the element blurs and
  then the window gets `blur`; the next Tab or click gives the window
  `focus` before the element's (measured; 6d1 had no window events). A
  focused link whose fragment target is not focusable blurs after the click
  (measured).
- **Wheel and scroll.** `wheel` is a `WheelEvent` with `deltaX`, `deltaY`
  (CSS px, `deltaMode` 0), `wheelDelta*` = 120 per axis with the sign opposite
  to the delta (measured: any size of delta gives 120), composed, bubbling,
  cancelable only if a listener on the path (the target, its ancestors, the
  window) is not passive; the listeners of the window, the document and
  `body` are passive by default (DOM), so a `preventDefault` there is
  ignored and does not stop the scroll. `scroll` is a plain `Event`: at the
  document (bubbling) when the viewport scrolled, at an element (not
  bubbling) when it scrolled as a scroll container; several elements get
  their events in the order they scrolled (CSSOM View "run the scroll
  steps" takes the targets in the order they were added; measured:
  `scroll-tree-order`; a scroll the page did not note by element, such as a
  scroll into view, comes after, in tree order). The page keeps the
  offsets of the last `scroll` events and a frame time (`Page::note_scroll`
  sets it 1/60 s ahead, in page time); `Page::pump` fires the events when the
  frame is due and `next_task_time` includes it, so virtual time jumps to it.
  Any number of scrolls within one frame make one event per target (and
  none if the position is the same again); `scroll_by` of the keys and
  the wheel, scroll into view and the fragment scroll all note it. Scrolls
  from scripts come with 7b. Chromium animates key scrolls (smooth
  scrolling) and fires `scroll` for each frame of the animation; swb
  scrolls at once, so one event.
- **Coordinates.** `clientX` and `clientY` are the pointer in CSS px;
  `screenX` and `screenY` are the same (the page has no screen offset);
  `pageX` and `pageY` add the viewport scroll; `offsetX` and `offsetY` are
  relative to the border box of the target (departure: Chromium measures
  from the padding edge; transforms are not applied), computed from a
  collection of the element boxes that the page keeps for one layout and
  one scroll position (`BoxCache`, keyed by `DocumentState::layout_serial`),
  so a pointer that moves over a page of 100,000 elements collects the boxes
  once, not once per move;
  `movementX` and `movementY` are the distance from the last pointer event
  (0 for the first). `buttons` and `detail` as Chromium (`detail` is the click
  count for `mousedown`, `mouseup`, `click`, `auxclick`, `dblclick`; 0 for
  the pointer events and `contextmenu`). The pointer events have
  `pointerType` `mouse`, `pointerId` 1, `isPrimary` true (false for `click`,
  `auxclick`, `contextmenu`), `pressure` 0.5 while a button is held (0 for
  `pointerup` and the click family), `button` -1 when no button changed
  (`pointermove`, `pointerover`, ...; 0 for the mouse event of the same
  kind). `pointerenter`, `pointerleave`, `mouseenter` and `mouseleave` do not
  bubble, cannot be canceled and are not composed; the others bubble and are
  composed.
- **`javascript:` URLs.** A click on `<a href="javascript:...">` (also
  `click()` and Enter) and the submission of a form whose `action` (or the
  submitter's `formaction`) is a `javascript:` URL queue the script as a task
  (`Task::JavascriptUrl`, HTML "navigate to a javascript: URL") that runs as
  a classic script in the document with the time limit of the task; it does
  not run inside the click: `a.click(); L('after')` logs `after` first, and
  after a user click the task runs after the `setTimeout(0)` timers that the
  listeners queued (measured: `javascript-link-order-click-method`,
  `javascript-link-order-user-click`). Errors are reported like those
  of a script (`error` event, console). The script does not run when the
  link or the form targets another browsing context (`target` or
  `formtarget` other than empty, `_self`, `_parent`, `_top`; else the
  `target` of the first `base` element; a named context counts as a new one
  because swb has no frames), or when the click had Ctrl (new tab), Shift
  (new window) or Alt (download) held; Meta runs it, a middle click is an
  `auxclick` and runs nothing (measured: `javascript-link-targets`,
  `javascript-link-modifiers`, `javascript-link-base-target`). The value is dropped: a string
  result does not replace the document (the departure of section 11;
  Chromium shows the string as a new document). A canceled click or
  `submit` runs nothing. The URL is parsed against the base URL and its
  path is percent-decoded as UTF-8 (HTML "navigate to a javascript: URL");
  a fragment is cut. JavaScript off ignores such links as before.
- **Departures.** `offsetX`/`offsetY` (above); no key events for modifier
  keys; no `compositionstart` and the other IME events, no `textInput`
  (Chromium fires the legacy `textInput` between `beforeinput` and `input`);
  no `toggle` event after a `summary` click; a drag fires no `click`;
  the `contextmenu` has no default (swb has no menu); a `details` element
  without a `summary` child gets no focus from the click and fires no
  `toggle` (Chromium focuses the details, `focus@details`); `selectstart` is
  fired for a press that focuses nothing (as Chromium) but its target is
  the hit node and a `user-select: none` element fires none; the keys of
  `select` fire no `input` and `change` (backlog); no touch or pen input;
  a key repeat is not generated (`repeat` is false); `Page::pump` cannot
  interrupt a task, so a mousemove listener that runs for 5 s freezes the
  GUI for that time (section 2).
- **Cost.** With scripts on, a pointer move that crosses no element
  boundary fires two events (two tasks); one that crosses a boundary of
  one element fires ten. The boxes of the page are collected once per layout
  and scroll position (for `offsetX`): 200 pointer moves over 100,000
  `<p><b>` take 1.6 s with JavaScript on and 1.4 s with it off (the hostile
  case `hover-100000-paragraphs-js-on`; the load is most of it). JavaScript off pays one `script.is_some()`
  check per input call. Hacker News with JavaScript on loads in 20 ms
  (19 ms with JavaScript off, base 20 and 18); the JavaScript-off perf table
  and snapshot equal the base's.

- **Fix round of 6d2 (review).** A stopped runtime lets the input work as
  with JavaScript off (above). `input type=image` submits the click's point
  with scripts on: the activation takes `offsetX/offsetY` of the trusted
  click when the target is the button itself (`EngineHost::
  request_form_submission` got a `coordinate`; `click()` and the clicks of
  keys and labels send 0,0, as the specification says). The automation
  `input.type` and `input.key` send key events whatever has the focus (the
  body without one) and go on when the target stops being editable; with
  JavaScript off `input.type` still fails without an editable focus. A
  failed step of `--steps` prints its message after the `--eval` lines, and
  `run_steps` runs the due tasks after each step (a `javascript:` task, a
  `setTimeout(0)`), as Chromium does while Playwright waits for its next
  call. After Tab left the page, a click on a part of the page that takes no
  focus gives the window `focus` (measured: `window-focus-click`, `focus@window`
  after `mousedown`). `space_down` is cleared when the focus changes or
  another key goes down.

### 6. Changes to `js`

The embedding API grows by what the bindings need (ADR 0026 section 12
lists the design; these are the parts to build):

1. **Host classes:** a registry per runtime (number, parent, name),
   prototypes and interface objects per realm, the brand check of
   section 4, and objects created with a class and host data.
   (Feature 4a: classes are numbered in the order of
   `Runtime::define_class`, from 0, so generated glue can name them by
   constants. Host data is a word (a `NodeId`) or traced generic data, in
   12 bytes of the object kind, so an object stays 88 bytes. A class
   without hooks makes ordinary objects with a class, which the inline
   caches store (the wrappers of nodes, the global object); only a class
   with hooks makes exotic objects. The brand check takes `undefined` and
   `null` as the global object of the function's realm (Web IDL §3.7.6),
   so `Node.prototype.appendChild.call(undefined)` is an `Illegal
   invocation` as in Chromium. An interface object is a native
   constructor: a call without `new` throws `Illegal constructor` (or,
   for a class with a constructor, Chromium's "Please use the 'new'
   operator" message), `new` without a constructor throws `Failed to
   construct 'Name': Illegal constructor`; a constructor gets the
   prototype of `new.target` with `Runtime::prototype_from_new_target`,
   so script subclasses work.)
   (Feature 4b: `ClassId::from_index` lets the generated glue name its
   classes by constants; `Runtime::begin_interface` and `end_interface`
   split `create_interface` so that `constructor` comes after the
   members; `host/ops.rs` makes the abstract operations that the
   conversions need public: `ToNumber`, `ToBoolean`, `IsCallable`, `GetV`,
   `Set`, `[[GetOwnProperty]]`, `[[OwnPropertyKeys]]`,
   `CreateDataPropertyOrThrow`, the iterator steps, arrays, plain objects,
   `PromiseResolve`, rejected promises and error objects. They call the
   engine's own implementations; nothing on the engine's paths changed.)
2. **Host hooks for internal methods:** optional functions per class
   for `[[GetOwnProperty]]`, `[[DefineOwnProperty]]`, `[[Get]]`,
   `[[Set]]`, `[[Delete]]` and `[[OwnPropertyKeys]]`. A hook may run
   script (a named setter converts its value with `ToString`); such a
   re-entry charges the recursion budget as other native re-entries do.
   Inline caches already treat host objects as uncacheable.
   (Feature 4a: a hook answers for a key or leaves it to the ordinary
   method on the object's stored properties (expandos); `[[Get]]` and
   `[[HasProperty]]` without a hook use the `[[GetOwnProperty]]` hook at
   the object; `[[OwnPropertyKeys]]` merges the hook's keys with the
   stored ones: indices in ascending order, then strings, then symbols,
   the hook's keys first within each group, without duplicates. The
   heap's walks stop at such an object with `Generic`,
   and the runtime's generic internal methods call the hooks where they
   call the traps of a proxy; the inline caches treat these objects as
   proxies, and only these: host objects of classes without hooks are
   cacheable. Each hook call charges the weight of a proxy level and a
   time step. The other internal methods (`[[PreventExtensions]]`,
   `[[SetPrototypeOf]]`) stay ordinary; the refusal of
   `preventExtensions` by legacy platform objects comes with the first
   binding that needs it.)
   (Feature 4c1: a `prevent_extensions` hook (also behind `Object.freeze`,
   `Object.seal` and `Reflect.preventExtensions`) and a `refusal` hook
   that words the `TypeError` of a refused `[[Set]]`,
   `[[DefineOwnProperty]]` or `[[Delete]]`, asked only when the engine
   throws. A `set` hook can hand an own property to the ordinary steps
   (`SetAnswer::WithOwn`, `WithStored`: Web IDL's [[Set]] ignores the
   named properties). The hook's keys keep their order in
   `[[OwnPropertyKeys]]` (a numeric name stays among the names); the
   stored keys follow. `[[SetPrototypeOf]]` has no hook: Web IDL and HTML
   only give objects `SetImmutablePrototype`, and most of those objects
   (the global object, interface prototypes) must stay ordinary and
   cacheable, so `Runtime::set_immutable_prototype` marks any object with
   a flag in its slot (ECMA-262 §10.4.7), which `[[SetPrototypeOf]]`
   checks, as it checks `%Object.prototype%`.)
3. **A host-defined global object** (InitializeHostDefinedRealm with a
   global object and `this` value from the host). Host objects are not
   cacheable today (`object/cache.rs`), and every global read
   (`document`, `Math`) goes through the global object, so the global
   must stay an ordinary, cacheable object that carries the `Window`
   class and host data; only `WindowProperties` (named access to
   elements by id) in its prototype chain is exotic. Feature 4a
   measures global reads before and after.
   (Feature 4a: `Runtime::init_global` turns the realm's global object
   into a host object of a class without hooks with that class's
   prototype, after the interfaces exist; the global `this` is the global
   object (section 4). The change of the prototype puts the global object
   into dictionary mode; the global caches work on dictionaries (holder,
   slot and the watch epoch), so global reads stay cached. A read that
   reaches the exotic `WindowProperties` is never cached. Measured with
   the `vm_bench` row "global: 3 reads of globals per step, 5M steps":
   135.7 ms with the plain global, 140.3 ms with a `Window` global that
   has 300 more own properties and an exotic object on its chain
   (`vm_bench --only global --host-global`, best of 5, three runs each),
   +3 %; `fib(27)`, whose calls read a global, 31.7 and 31.5 ms.)
4. **Host data that the collector traces:** the runtime's host data
   gets a trace method, and the collector treats it as a root; the
   binding state (wrapper vector, listener table, timers) is traced
   through it, without one persistent root per value.
   (Feature 4a: the host data is a value of any type that implements
   `HostState`; it lives in the VM, whose roots include its `trace`.)
5. **Termination from a native function:** a native can end the running
   task with a termination reason (the DOM limits of section 16).
   (Feature 4a: `Err(rt.terminate(reason))` ends the task with
   `Termination::Native(reason)`, which no handler, `finally` block or
   promise sees, as the time limit. The runtime remembers the request
   until the host's outermost run returns, and each time check repeats
   it, so a native that drops the error does not keep the task running;
   a run that ends normally after a dropped request still returns the
   termination. The next run starts without it.)
6. **Event handler functions:** compile a function from source text and
   parameter names with a scope chain of object environments (HTML
   "getting the current value of the event handler": the document, the
   form owner, the element), with the early errors of a function body.
   (Feature 4a: `Runtime::compile_event_handler` builds the source
   `(function NAME(PARAMS) {\n` body `\n})`, so that `toString` gives
   Chromium's text, and parses it with the checks of the `Function`
   constructor as direct eval code (`swb_js_syntax::parse_event_handler`):
   the free names of the function are dynamic references through a
   captured chain of object environment records (with `@@unscopables`, as
   `NewObjectEnvironment(O, true, E)`), then global names. `NAME` is the
   function's `name`, not a binding (`onclick` in the body is the
   element's property, measured in Chromium). An early error is
   `ScriptError::Compile` with the offset in the body.)
7. **Module scripts without imports:** a Module goal script with no
   `import`, `export`, `import()` or `import.meta` runs as strict code
   with `this` undefined and its top-level declarations local to the
   script (M7 feature 14 note). A module with imports is reported as
   unsupported and does not run. This item comes with the first target
   that has such a script; until then module scripts do not run.
8. **The heap accounting of source text** charges its real size
   (`SourceText::byte_size`), together with the per-document heap limit
   (backlog item of M7 13e).
   (Feature 4a: done; `Runtime::set_heap_limit` changes the limit between
   tasks.)

### 7. The event loop

The HTML event loop (§8.1.7), as far as the pages need it.

**Queues.** The binding state has task queues per task source: user
interaction, DOM manipulation (with the parser and element tasks),
networking, timers, posted messages, and the idle callbacks. A task is
a Rust value (which source, what to run). A new engine function,
`Page::pump` (called from the GUI's event loop, the headless loop and
the automation server's loop), does one turn at a time:

1. Take the network completions and turn those of script fetches into
   tasks.
2. Run due tasks: user interaction first, then the oldest task of the
   other queues. After each task, a microtask checkpoint.
3. If a rendering opportunity is due, run the rendering update.
4. If no task ran and nothing is due, run idle callbacks until the
   deadline of the next task, timer or frame.

`pump` runs at most a bounded amount of work (for example 16 ms) and
returns the time of the next due item, so the GUI can wait with
`WaitUntil` and stay responsive between tasks.

**Microtask checkpoint:** `Runtime::run_jobs` after each task, and after
each callback when the JavaScript stack becomes empty ("clean up after
running script"), so microtasks run between two listeners of the same
event. The checkpoint runs under the deadline of its task.

**Timers:** `setTimeout` and `setInterval` (the string form compiles as a
classic script), ordered by due time and then by sequence number. HTML
§8.6 raises a timeout below 4 ms to 4 ms when the nesting level is
greater than 5; feature 5b (the event loop) measures Chromium's
threshold with a probe and uses it.
`queueMicrotask` enqueues a job.
(Feature 5b: Chromium 148 has the HTML threshold: in a chain of
`setTimeout(f, 0)` the seventh timer, the first with a nesting level
above 5, is the first that waits 4 ms, also for `setInterval(f, 0)` and
`setTimeout(f, 2)`. Chromium keeps the nesting level of a timer task in
its microtasks, so a timer scheduled from a promise reaction of a timer
callback nests too; swb does the same. See the feature 5b notes.)

**Rendering update** (§8.1.7.3, "update the rendering"): when a frame is
due (every 16.7 ms of clock time while something needs it: animation
frame callbacks, a dirty style or layout, observers with pending
observations, a window resize), in this order: resize events, scroll
events, media query change events, animation frame callbacks, style
and layout, `ResizeObserver` (the gather and broadcast loop with the
depth rule; the loop error after its limit), `IntersectionObserver`,
paint when the GUI presents. Headless mode runs the same steps without
paint.

**`requestIdleCallback`:** runs in step 4 with an `IdleDeadline` of at
most 50 ms; with a `timeout`, a timer task runs it if it is not idle in
time.

**Time budget per task.** Before each task (and each checkpoint, and
each rendering update callback) the engine sets the runtime's deadline
to now + 5 s. A task that runs past it ends with a termination (ADR
0026 section 5): no `finally` runs, and the task is logged with the
script URL. The document keeps running scripts after one or two
terminations by time; after the third, it stops (timers, listeners and
callbacks no longer run; the page stays as it is). The value is a
setting; tests use smaller ones.

**Items from the M7 backlog:**

- A termination inside the resume of an `async` function drops that
  job; the function stays suspended forever. Accepted: it happens only
  after a task ran for 5 s, and the document's other work continues.
- A heap-limit termination: the runtime cannot recover. The engine
  marks the document as "scripts stopped" and drops the runtime at once
  (this frees the heap and the document's globals). The DOM stays as it
  is.
- Finalization cleanup: after each task the engine asks
  `pending_finalization_cleanups` and queues one task per cleanup (no
  new host hook).
- Unhandled rejections: at the end of a checkpoint the engine takes
  them, logs `Uncaught (in promise)` as Chromium's console does, and
  queues a task that fires `unhandledrejection` at the window.
  `rejectionhandled` needs a report from the tracker of a handler added
  later; it comes when a page listens for it.
- The incumbent settings object is not modeled: with one realm per
  runtime, the current realm stands for it.

**Clocks.** The page has a clock that drives timers, frames,
`performance.now`, `Date.now` (through `Runtime::set_clock`) and event
time stamps. In the GUI it is real time. Tests and `compare` can use
virtual time: the clock jumps to the next timer or frame when no task
is runnable and no request is pending (Chromium's virtual time budget
works the same way), so a test does not wait for real timers and its
results do not depend on the speed of the machine. The start time can
be fixed (`--now`, as in the `swb-js` shell).

### 8. Scripts and the HTML parser

**Parser-inserted scripts** (HTML §4.12.1.1, "prepare the script
element"), at each script result of the parser:

- A classic script without `src` runs at once, unless a style sheet is
  blocking scripts (a stylesheet inserted before it that has not
  loaded); then the parser waits for the sheets.
- A classic script with `src` and without `async` and `defer` is
  parser-blocking: the parser waits until it is fetched and no style
  sheet blocks scripts, then the script runs and the parse continues.
- `defer` scripts and module scripts run in order after the parse, before
  `DOMContentLoaded`. `async` scripts run when they arrive, each in a
  task.
- `nomodule` scripts do not run. Scripts whose `type` is not a
  JavaScript MIME type (JSON, `ld+json`, templates) do not run. Module
  scripts with imports do not run (section 6, item 7) and log a warning.
- The parse between two scripts runs in one step. The parser does not
  yield in the middle of long markup; no speculative fetch of later
  resources (a preload scanner) in M8.

**Scripts inserted by scripts** (`createElement('script')` and
insertion, or markup with scripts in `innerHTML`, which never run): an
external one is async by default, or ordered when its `async` is set to
false ("in order as soon as possible"); an inline one runs during its
insertion. Fetched scripts fire `load` or `error` at the element.
`document.currentScript` is the running classic script element.

**`document.write`** while a parser-inserted script runs: the text goes
into the input in front of the unparsed rest, and the parser processes
it before `write` returns (the engine puts the document back into the
parser for the duration). A script element in the written text that is
inline runs inside `write`; an external one becomes the pending
parser-blocking script, and the parse waits for it after the current
script ends (HTML §13.2.6.4.8). `write` from an external script that
runs without an insertion point (async, deferred, inserted by a script)
is ignored, as the specification says (the ignore-destructive-writes
counter, HTML "document write steps"). `write` after the parse from
other code (a timer, a listener) would run `document.open` and replace
the document; swb ignores it with a warning (a deviation; measure
Chromium before changing it). `document.open` is not supported.

No M8 target calls `document.write` (Ars Technica does), so it comes
with the first target that needs it; the parser design above keeps it
possible.

**Scripting flag:** the parser gets it from the document's setting
(`<noscript>` is raw text in `head` and `body`), and so does
`@media (scripting)` and the controls of media elements (ADR 0025).

**Document lifecycle** (HTML §13.2.6.5, "the end"):
`document.readyState` is `loading` during the parse and becomes
`interactive` when the parse ends; then the deferred scripts run in
order, then `DOMContentLoaded` fires; it becomes `complete` when
nothing delays the load event any more (stylesheets, images, scripts, fonts
inserted before that point); then `load` at the window and `pageshow`.
`page.waitForLoad` of the automation API and the headless runner wait
for the load event; requests that scripts start after it do not count.
Leaving a document fires `pagehide` and `unload` (without prompts;
`beforeunload` never shows a dialog).

**Errors:** an uncaught exception reports to the console in Chromium's
form and fires `error` (an `ErrorEvent` with the message, script URL,
line and column) at the window; `window.onerror` sees it. A compile
error of a script is reported the same way.

**Rendering during the parse:** as now, the first layout waits until
the stylesheets inserted so far have loaded; after that, a rendering
opportunity can show a partly parsed document (as in Chromium).

Feature 5c notes (measured with `tools/probes/web-scripts.json`, which
serves its scripts and style sheets with delays from a local HTTP server):

- (Feature 5c: the parse.) The runtime drives the parse
  (`EngineHost::parse_step`, one step per `script` end tag;
  `document_load.rs`). An inline classic script runs at its end tag as a
  task; a parser-blocking external script (or an inline one while a style
  sheet blocks scripts) becomes the pending parsing-blocking script, and
  the parse waits. "A style sheet that is blocking scripts" is the sheet of
  a `link` that the parser inserted, not arrived or failed, whose media
  match (Chromium does not wait for `media=print`). When the wait ends (the
  script arrived, a sheet arrived), a parser task (`Task::ParserResume`,
  in the one queue of section 7) runs the script; after an external script
  the parse goes on in the next parser task, after an inline one at once
  (as Chromium: a timer of 0 ms that a fetched blocking script set runs
  before the rest of the parse; one that the unblocked inline script set
  runs after it). Timers run while the parser waits.
- (Feature 5c: the end.) At the end of the input the readiness becomes
  "interactive" (`readystatechange`), the deferred scripts run in order
  (each after the sheets that block scripts), and `DOMContentLoaded`
  fires; then the load waits until the scripts that run as soon as
  possible ran, and until the engine finds that nothing delays the load
  event (no request, also after a layout that can start the loads of
  background images and fonts): then, before the next task, the
  readiness becomes "complete", `load` fires at the window (legacy target
  override) and `pageshow` (bubbles and can be canceled, as HTML says).
  Departures from HTML, as Chromium: `DOMContentLoaded` and `load` are not
  queued tasks, so with nothing to wait for they come before a timer of
  0 ms that a script set (a document from a file); but "the end" starts
  in a parser task, after the due tasks, when the document came over
  HTTP(S) (its end of input is a task of its own) or when the parser ever
  waited for a script or a style sheet. Images and scripts that still
  load do not defer it for a file. Measured in Chromium 148, three runs
  each (cases `end-http-*`, `end-file-*`, `timer-vs-dcl*`).
- (Feature 5c: fetches.) The engine fetches the classic scripts
  (`document/scripts.rs`; destination `script`, the document's origin as
  initiator), at most 64 at once with the rest in a queue (section 16).
  An error status, a type `image/*`, `audio/*`, `video/*` or `text/csv`,
  or a type that is not JavaScript with `X-Content-Type-Options: nosniff`
  is a network error (`error` at the element); the text is decoded with
  its BOM, else the response's charset, else the document's encoding (the
  `charset` attribute is ignored). The result goes to
  `ScriptRuntime::script_fetched`: an async script runs at once in its own
  task, the in-order list runs from its front, a parser script sets
  "ready to be parser-executed". A fetch that fails at once (a URL that
  the document may not load) completes in a task.
- (Feature 5c: execution.) "Execute the script element" sets
  `document.currentScript`, runs the script (`Runtime::eval_named_at`
  with the start of an inline script's text in the document, which the
  parser finds, `HtmlParser::script_position`; positions of errors and
  stack frames then count from the document, as in Chromium), runs the
  microtask checkpoint before it restores `currentScript` (Chromium's
  microtasks see the script) and fires `load` at an external script.
  `load` and `error` at script elements neither bubble nor can be
  canceled.
- (Feature 5c: scripts that scripts insert.) `createElement` (HTML names
  in lowercase; the name check of DOM "valid element local name", as
  Chromium 148) and `appendChild` came forward from feature 6a.
  `appendChild` runs the children changed steps of a `script` parent and
  the post-connection steps of the inserted scripts in tree order: an
  inline one runs inside the call (nested; the shared recursion budget
  ends an endless chain with a `RangeError`), an external one is async
  unless its `async` was set to false (then in order). `HTMLScriptElement`
  has `src`, `type`, `noModule`, `async` (force async), `defer` and
  `text` (its setter runs the children changed steps). Feature 6a calls
  `scripts::after_insertion` from its other insertions and adds the
  attribute change steps (a new `src`, an `async` attribute added with
  `setAttribute`).
- (Feature 5c: the unload.) A navigation fires `pagehide` (if `pageshow`
  fired) and `unload` at the old window before it drops the document.
  `Page::stop` aborts the parse: the document counts as loaded, with the
  readiness "complete" and no events (HTML fires `readystatechange`).
- (Feature 5c: waits.) `Page::is_fully_loaded` and `page.waitForLoad`
  wait for the `load` event of a document with a runtime; requests that
  scripts start after it do not count. A runtime that reached the heap
  limit is dropped, and the parse runs to the end without scripts; a
  document whose scripts stopped (three time limits, the budget of long
  tasks) parses on without them: the page abandons the script fetches,
  runs the parser to the end and counts the document as loaded, with the
  readiness "complete" and no events.
- (Feature 5c: departures.) Module scripts, import maps, speculation rules
  and SVG scripts do not run (a warning for modules). A `src` of only
  whitespace fires `error` like an empty one, as Chromium (HTML would
  fetch the document's URL). No preload scanner (as planned): in Chromium
  an async script that a waiting parser has not reached yet can arrive
  early and run before the end of the parse. Errors of cross-origin
  scripts are not muted (HTML "muted errors"). `document.write` is not
  supported.

### 9. Style and layout after changes; layout queries

**Invalidation.** The engine drains the journal after each task and
parser step and before each flush. A change of the tree, an attribute
or a text marks the style as dirty; a change of a `style` or `link`
element, an arriving stylesheet or a CSSOM rule change marks the
stylist as dirty. The stylist keeps each stylesheet parsed, so a
rebuild makes only the rule index again (today `invalidate_style`
parses all sheets again). New elements start their loads at insertion.
(Feature 3b: implemented with the parsed-stylesheet cache, which section
9's plan gave to feature 7a. A drain reconciles the `style`, `link`,
`img`, `video`, `picture` sources, `base` and `title` elements that the
changes touched, in tree order: a moved element keeps its sheet or image
and loads nothing again; a removed one loses its sheet. A changed base
URL marks the stylist dirty too. A changed image source keeps the current
image until the new one has loaded, as the HTML pending request does. The
loads of one drain start in the order stylesheets, posters, images, as
the walk of a complete document does. With the parse in steps, the loads
of successive drains interleave: the images of one step start before the
stylesheets of the next. So the walk and the journal start the same loads
in the same order within each kind, but not across kinds.)

**Flushes.** Style and layout run when something needs them, not after
each change: in the rendering update (section 7); for a style query
(`getComputedStyle` values that do not depend on layout); for a layout
query (`getBoundingClientRect`, `getClientRects`, `offset*`,
`client*`, `scroll*`, `innerText`, `elementFromPoint`,
`getComputedStyle` values resolved from layout, `scrollIntoView`).
A flush does nothing when the generation is the same as at the last
one, so a script that reads many values after one change pays once.
`matches`, `closest`, `innerWidth`, `scrollY` and `matchMedia` need no
flush.

**Costs.** A full restyle costs 1.5 to 21 ms and a full layout 2 to
13 ms on the target pages (docs/performance.md, "Baseline"). Chromium
recalculates style 18 to 130 times and lays out 14 to 100 times while
the larger pages load and scroll. If swb flushed fully that often, a
page would spend up to 3 s in style alone. The plan:

1. Feature 7a builds the full flush with counters of forced style and
   layout runs per load, and the parsed-stylesheet cache. (Feature 3b
   built the flush on demand and the parsed-stylesheet cache; 7a adds the
   counters.)
2. If a page's forced style time is above 20 % of its load-time budget
   (section 16), the next feature adds the two measures that cut style
   time, in this order:
   - **The style of one element on demand:** when the styles are dirty
     and a query asks for one element, compute the cascade of that
     element and its ancestors only, and keep it until the next change.
     This serves `getComputedStyle` in loops that walk the DOM.
   - **Restyle of dirty subtrees:** a change marks its element for a
     restyle of its subtree; the stylist records which attributes
     selectors name (a change of another attribute restyles nothing) and
     whether sibling-dependent selectors (`+`, `~`, `:nth-*`,
     `:first-child`, `:empty`, `:has()`) exist, which widen the restyle
     to the parent's subtree; `StyleMap` is updated in place.
3. Layout stays a full layout in M8. Incremental layout (relayout
   boundaries, keeping fragment subtrees) is a separate design when a
   page's forced layout time is above its share of the budget.

### 10. CSSOM

- `element.style`: a `CSSStyleDeclaration` over the `style` attribute:
  `getPropertyValue`, `setProperty` with priority, `removeProperty`,
  `cssText`, indexed access and the camel-case properties of the
  supported longhands and shorthands, parsed and serialized with the
  `style` crate's property parsers. A change writes the attribute
  (journaled like any attribute change).
- `getComputedStyle(element, pseudo)`: a read-only declaration that
  returns resolved values (CSSOM §9), serialized in Chromium's form,
  for the properties that `style` computes. Each property's
  serialization is measured in Chromium with probe cases.
- `document.styleSheets`, `CSSStyleSheet` (`cssRules`, `insertRule`,
  `deleteRule`), `CSSStyleRule.selectorText` and `style`,
  `HTMLStyleElement.sheet`, `HTMLLinkElement.sheet`: the parsed sheets
  of section 9 are the rule lists; a change marks the stylist as dirty.
  Rule insertion at a rate of hundreds per task (CSS-in-JS libraries)
  must not parse the other rules again. No M8 target uses these (BBC
  does); they come with the first target that needs them.
- `CSS.supports`: through the `@supports` parser of `css`.

Feature 7a notes (7a1: invalidation, counters, `element.style`,
`CSS.supports`; `getComputedStyle` is 7a2. Measured with
`tools/probes/web-cssom-style.json` (12 cases, 95 queries) and
`tools/probes/web-cssom-values.json` (843 property values), three equal
Chromium runs each; the Rust tests are `crates/web/tests/cssom_style.rs`
and `swb_style::cssom` (`cssom/tests.rs`)):

- **Invalidation (section 9).** The journal drain marks the styles dirty
  only if a change was in the document: the parent of a child-list
  change, the element of an attribute change or the node of a text change
  is connected (a walk to the root with the answers kept per drain, so a
  deep detached tree costs each ancestor once). Changes in detached
  subtrees and in template contents keep the styles; their insertion is a
  change in the document. (This closes the backlog item of feature 3b.)
- **The style of one element on demand (section 9, plan item 2).**
  `swb_style::compute_element_styles` runs the cascade of an element and
  its ancestors, from the document element down, with the code of the
  full cascade (`Styler::style_element`, without the Bloom filter and
  without pseudo-elements, `use` instances and counters); the result
  equals the full styles for these elements (unit test). Descendants of
  `display: none` get styles too (the full cascade skips them;
  `getComputedStyle` needs them). `DocumentState::query_element_style`
  reads the full styles when they are current, else this chain, kept in
  `element_styles` until the styles or the element states change. It
  rebuilds the form control states only if a selector depends on them
  (their rebuild walks the document; no rule of the user-agent sheet
  does), and uses the state set without per-document tables
  (`StateSet::lazy`). The
  focusability check of `focus()` uses it after a DOM change: 4,000
  times "append an input to a page of 20,000 elements, `focus()`" took
  the 5 s time limit before (2,000: the limit too, 10.5 s for the process)
  and take 0.28 s for the whole process now (2,000: 0.21 s; hostile case
  `append-focus-loop-js-on`).
- **Counters (section 9, plan item 1).** `DocumentState::flushes`
  (`FlushCounters`): the full style runs, the layout runs, the full runs
  that a script query forced (`query_layout` sets the flag `in_query`
  that marks them), and the element style runs
  (`query_element_style`), each with its time, per document. `--bench`
  prints them as `flushes` (of the load, before the benchmark's own
  runs), `page.info` as `flushes`. In 7a1 nothing calls `query_layout`,
  so the forced counts stay 0; the only runs that a query causes are the
  element style runs of `focus()`. `getComputedStyle` (7a2) and the
  geometry of 7b call `query_element_style` and `query_layout`.
- **`element.style` (section 10).** `swb_style::cssom::StyleDeclarations`
  is the declaration block: longhands only, as Chromium's (a shorthand
  sets each longhand; reading or serializing a shorthand puts it
  together), each value kept as Chromium serializes it, checked with
  swb's own property parsers (by name), so a property or value that swb
  does not parse is ignored. Logical properties are their own longhands
  (Chromium keeps `margin-inline-start` apart from `margin-left`; swb's
  cascade maps them when it parses the attribute). The binding
  (`crates/web/src/cssom.rs`) keeps one block per element with the
  attribute text and the DOM generation it was built for; a read parses
  again only if the attribute changed; a change writes the block's
  `cssText` through the attribute funnel. Nodes live as long as their
  document (section 3), so when 4,096 blocks exist, the blocks of
  elements that are not connected go (review fix: 150,000 thrown-away
  elements with three writes each kept 1.4 KB per element); the next
  limit is twice the blocks that stay. A read builds such a block again
  from the attribute; the parse moves `!important` declarations to the
  end, so the order of their `cssText` can change (departure). The
  property names are named
  properties of the object (`[SwbPropertyNames]` in `idl/cssom.idl`,
  measured: in Chromium `style.hasOwnProperty('color')` is true and
  `CSSStyleDeclaration.prototype` has only `cssText`, `length`,
  `parentRule`, `cssFloat` and the operations). `[SwbPropertyNames]`
  (generator and `bindings/legacy.rs`): the named setter takes only
  supported names (`style.foo = 1` is an expando, as in Chromium), and
  `delete style.color` succeeds without a change. The names listed by
  `Object.keys` are the camel-cased and webkit-cased names (sorted, as
  Chromium); a name exists exactly when swb parses the property, so
  `'transform' in el.style` answers truthfully. No accessor pair per
  property is created, so the interface adds little to the runtime
  (senko.net, release, seven interleaved runs: heap 1,132,705 → 1,140,510
  bytes, creation 3.32 → 3.28 ms median, within noise).
- **Chromium behaviour found.** Values: keywords lower case, numbers
  with six significant digits in `%g` form (`1.23457e+08`, `1e-07`), `0`
  as `0px` (`0deg` in rotations), units lower case, `#hex` and
  `rgb()`/`hsl()`/`hwb()` as `rgb(r, g, b)` or `rgba(..., a)` with the
  alpha in two decimals when they round-trip, else three (`#f008` is
  `0.533`), names stay names, `calc()` sums of one unit computed
  (`calc(15px)`, `max(1px,2px)` is `calc(2px)`), `url("...")`, strings
  in double quotes, font families unquoted when they are one identifier,
  quoted when several or a quoted generic name, and per-property
  rewritings (x/y order of `transform-origin`, `background-repeat:
  repeat no-repeat` is `repeat-x`, `counter-reset: x` is `x 0`,
  `display: inline flex` is `inline-flex`, `aspect-ratio: 2` is
  `2 / 1`, and others in `cssom/longhand.rs`). Shorthands: omitted
  longhands are `initial` (`outline`, `list-style`, `text-decoration`,
  `background`) or the initial value (`border`: `medium`, `none`,
  `currentcolor`; `font`: `normal`; `flex-flow`, `columns`); `border`
  also resets the five `border-image` longhands (swb keeps them as
  longhands that only `border` sets, so `cssText` writes
  `border-image: initial` as Chromium); a shorthand's value leaves out
  the omitted parts (`border: none` reads as `""`, `outline: red solid
  1px` is `red solid 1px` in Chromium's order); `flex: 1` is `1 1 0%`.
  `cssText` parsing: the last declaration of a property wins, an
  important one over the others, and the important declarations come
  last. `setProperty` with an unknown priority does nothing; a property
  write (`style.color = ...`) has no priority, so it drops `!important`;
  `null` empties, `undefined` is the string. `removeProperty` of a
  shorthand returns `""` (CSSOM says its value). `cssText = ...` always
  writes the attribute, also `""`; `removeProperty` of a property that is
  not there does not. In quirks mode unitless lengths take `px` for the
  properties of the quirk.
- **Departures.** `length` and `item()` list swb's longhands (Chromium's
  `font` sets 19, swb's 9; Chromium's `text-decoration` has a thickness);
  `grid`, `grid-template`, `mask`, system fonts in `font`, backgrounds
  of several layers and a `text-decoration` with a thickness
  (`line-through 2px`) are kept as written and their longhands read as
  `""` (as for `var()`); whitespace runs in custom property values
  collapse; `lab()` and the other CSS Color 4 functions keep their units;
  `!important` inside a `var()` value is not taken as the priority; `all`
  is not supported; `Object.keys` lists the names before the expandos
  (Web IDL; Chromium after). Where swb's parsers differ from Chromium's
  (no `clip-path` shapes, no 3D transforms, no hashless colors in quirks
  mode, `position: -webkit-sticky` accepted), the block follows swb's
  parsers: the value list of `web-cssom-values.json` marks these rows.
  `CSS` has `supports` and `escape` only (written by hand: the generator
  has no namespaces); `font-tech()` in a condition is false. A
  declaration in a condition may be `!important` (Chromium, measured:
  `CSS.supports('display: flex !important')` is true, a second
  `!important` makes it false); the two-argument form takes no priority.

### 11. Events from the engine to scripts

**Dispatch** follows DOM §2.9: the event path, capture and bubble,
`once`, `passive`, `signal`, `stopPropagation`,
`stopImmediatePropagation`, `preventDefault` (ignored for passive
listeners and events that cannot be canceled), `composedPath`,
`window.event`, and the exceptions of listeners reported as in section
8 without stopping the dispatch.

**Input.** The engine keeps hit testing and the default actions. With
scripts, each input becomes: hit test, dispatch the DOM events, then
the default action unless an event was canceled.

- Mouse and pointer events (UI Events, Pointer Events), key events
  (`keydown`, `keypress` for printable keys, `keyup`), focus events
  (also from `focus()` and `blur()`), `input` and `change` of form
  controls, `submit` and `reset`, `wheel` before a scroll and `scroll`
  once per frame.
- Checkboxes and radio buttons use the legacy pre-activation behaviour:
  the state changes before `click` and is restored if the click is
  canceled.
- Activation of `<a href="javascript:...">`: the URL's script runs in
  the document; a result that is a string does not replace the document
  in M8 (a deviation; such links almost always return `undefined`).

**Event handlers.** `onclick` and the other `on*` content attributes
are compiled when they are first needed (section 6, item 6) with the
spec's scope chain; the IDL attributes (`element.onclick = f`) set them
directly. `body` and `frameset` forward the window's handlers.
Return value `false` cancels the event (and the special rules of
`onerror` and `onbeforeunload`).

### 12. Network from scripts

All requests go through `net` and the engine's loader, so they share
the worker threads, the cookie jar and the replay of fixtures.

- **`fetch`** with `Request`, `Response`, `Headers` and
  `AbortController`; all methods (`net::Method` grows); string, form,
  blob and buffer bodies. Streams are later work: `Response.body` is
  `null` with a warning.
- **`XMLHttpRequest`**, asynchronous only: a synchronous `open` throws
  `InvalidAccessError` with a warning (it would block the page thread).
  This is a deviation: the specification allows synchronous requests
  in a window unless `responseType`, `timeout` or `withCredentials` is
  set.
- **`navigator.sendBeacon`:** a `POST` whose response nobody reads.
- **Elements inserted by scripts** (`img`, `link`, `script`, `Image()`)
  load as parser-inserted ones do and fire `load` and `error`.
- **Credentials and CORS.** swb follows the Fetch modes: requests from
  scripts to another origin send cookies only with the `include`
  credentials mode (`fetch` default `same-origin`, XHR
  `withCredentials`); cross-origin responses are visible to the script
  only if the CORS check passes (`Access-Control-Allow-Origin`, and
  `-Credentials` for credentialed requests); requests that are not
  simple send a preflight `OPTIONS`. Reason: without this, a page could
  read the user's data from any other site with the user's cookies. The
  check is in `net` (a function over a request and its response), so
  the loader applies it in one place.
- **Replay and CORS.** The fixtures do not store CORS headers or
  preflights. The replay fetcher answers a preflight with a permissive
  response, and adds `Access-Control-Allow-Origin` with the request's
  origin and `Access-Control-Allow-Credentials: true` to replayed
  responses of cross-origin script requests. The Chromium routing of
  the test tools does the same (today it adds `*`, which credentialed
  requests reject), so both browsers see the same server. Requests that
  a fixture lacks fail with a network error in both.
- **Cookies:** `document.cookie` reads and writes the jar for the
  document URL, without `HttpOnly` cookies. With fixture replay, the
  page gets an in-memory jar too, so scripts can store and read
  cookies (replay ignores the request cookies).
- **Storage:** `localStorage` and `sessionStorage` per origin, in
  memory, for the life of the process, 5 MiB per origin (measure
  Chromium's limit); a write past it throws `QuotaExceededError`; the
  `storage` event goes to other documents of the origin (in M8 there
  are none).
- Each document has at most 64 requests from scripts in flight; others
  wait in a queue.

### 13. Other APIs

Each comes with the first page in the plan that needs it. Choices that
are not obvious from the specifications:

- **Location and history** use the engine's session history; a
  navigation from `location` starts after the task; `history.state` is
  a structured clone (plain values, arrays, objects, `Date`, `RegExp`,
  `Map`, `Set`, buffers, errors; other values throw `DataCloneError`).
- **Observers:** `MutationObserver` queues its records from the
  mutation sink of section 3, at each primitive change, with the
  transient registered observers of removed nodes (DOM §4.3).
  `IntersectionObserver` and `ResizeObserver` run in the rendering
  update. `PerformanceObserver` delivers `mark` and `measure` entries
  only.
- **`URL`, `URLSearchParams`** over the `url` crate (WHATWG URL);
  `crypto.getRandomValues` and `randomUUID` from the operating system's
  random source, no `crypto.subtle`; `performance.timing` and the
  `navigation` entry from the engine's load times.
- **`navigator`:** `userAgent` unchanged (ADR 0025); no `userAgentData`
  (swb does not claim to be Chromium).
- **Canvas:** with scripting on, `<canvas>` is a replaced element that
  paints nothing. `getContext('2d')` returns a context whose state
  works but whose drawing does nothing; `getImageData` gives transparent
  pixels, `toDataURL` the PNG of an empty canvas; `webgl` gives `null`.
  Drawing comes when a target shows canvas content. No M8 target calls
  `getContext` (Ars Technica does), so in M8 `<canvas>` is only the
  empty replaced element of feature 2; the context comes with the first
  target that needs it.
- **`console`** writes to the log and to a console buffer of the
  automation API. (Feature 4c2: see the notes of section 5.)
- Members that pages use only for feature detection (`indexedDB`,
  `caches`, `cookieStore`, `trustedTypes`, `navigation`,
  `customElements`) stay absent until a page needs them. Absent is
  safer than a stub that pretends to work.

### 14. Frames and media: out of scope

M8 does not render `iframe`, `frame` or `object` documents and runs no
scripts in them. An `iframe` is an empty box of its size;
`contentWindow` and `contentDocument` are `null`; `window.frames`,
`parent` and `top` are the window itself; `postMessage` to a frame does
nothing. What this leaves for later: nested documents and their
rendering, hit testing and input inside them, a realm per same-origin
frame in the parent's runtime (pages use `about:blank` frames to get
clean built-ins), a runtime per cross-origin frame with structured
clone between runtimes for `postMessage`, `MessageChannel`, and the
`WindowProxy` object. Consent dialogs of some sites live in
cross-origin frames. The Craigslist posting has a hidden same-origin
frame that keeps `localStorage` in step across Craigslist's
subdomains; the page posts to it inside `try`, and in Chromium the
page renders the same when the frame does not load (measured: same
elements, no errors).

Media playback is out of scope too (ADR 0020 stays: no media is
fetched). With scripting on, `HTMLMediaElement` has its attributes
(`paused` true, `readyState` 0), and `play()` returns a promise that
rejects with `NotSupportedError`.

### 15. The on/off setting

As ADR 0025 decided: off by default; a change applies to the next
document.

- Command line: `--js` (`--javascript on|off`).
- Automation: a `javascript` option of the session and the method
  `page.setJavaScript {enabled}`; `page.info` reports it.
- GUI: a toolbar toggle and a keyboard shortcut; a reload applies it.
- Off: no runtime is created; the parser runs with the scripting flag
  off and without pauses; the resource discovery through the journal
  gives the same loads as the old walk. The JS-off fixtures, their
  references and scores do not change; `just snapshot` proves it at
  each feature of M8.

### 16. Budgets and robustness

Per document, all settings with these defaults:

| Resource | Limit | Result |
|---|---|---|
| Time per task, checkpoint or callback | 5 s | termination; after three, scripts stop (section 7) |
| JavaScript heap | 256 MiB | termination; scripts stop, the runtime is dropped |
| Nodes created in a document's arena | 1,048,576 | the task ends (termination) |
| Nodes of one `innerHTML` write (feature 6b) | the nodes of its markup (one fragment is reused) | `<p>x` costs 2 immortal nodes, so about 500,000 writes fill the arena; the arena limit ends the task |
| Steps of one selector query (feature 6b) | 100,000,000 (ancestor steps and selectors tried) | the query stops with a warning |
| DOM bytes: text, names and attributes | 256 MiB | the task ends |
| Values that scripts write to text fields and text areas, in the states and in the seeds (feature 6c) | 32 MiB | the task ends (termination, warning) |
| Event listeners on the nodes and the window of a document (feature 5a) | 262,144 | the task ends (termination) |
| Active timers and their extra arguments per document (feature 5b) | 1,048,576 | the task ends (termination) |
| String handlers of the active timers of a document (feature 5b) | 32 MiB | the task ends (termination) |
| Long tasks (50 ms or more) of a document, their total real time (feature 5b) | 30 s | the document runs no more scripts |
| Nested event dispatch, nested `document.write`, recursion through DOM callbacks | the shared recursion budget (ADR 0026 section 9), charged per re-entry | `RangeError` |
| Script fetches in flight (feature 5c) | 64 | queued |
| Fetched script text that has not run, per document (feature 5c) | 256 MiB | the script fails as a network error (`error` event, warning) |
| `localStorage` per origin | 5 MiB | `QuotaExceededError` |
| Mutation observer records waiting | 1,000,000 | further records dropped with a warning |

The budget of long tasks (feature 5b): the time limit of a task and the
stop after three terminations do not bound a page of many scripts that
each run just under the limit (1,000 inline scripts of 4.9 s run for 80
minutes, and the GUI freezes for that time). So the real time of the
tasks that take 50 ms or more (the threshold of a long task in the Long
Tasks API) adds up per document, and at 30 s the document runs no more
scripts; a task gets a deadline of at most the rest of the budget plus
50 ms. The value is twice the 15 s that three terminations allow: the
local JavaScript fixtures of the targets load with their inline scripts
in at most 1.3 s each with swb's interpreter (measured in feature 4c2),
so 30 s leaves a margin of more than twenty times, and a broken page
stops within half a minute. Tasks shorter than 50 ms never
count: a page that runs an animation or polls in short tasks runs for as
long as it is open, as in Chromium.

DOM algorithms that walk trees (clone, serialization, `textContent`,
adoption, selector queries) are iterative, as the parser and the
serializer are now.

**Load-time budget.** With JavaScript on and a replayed fixture
(release build, test fonts, 1280x800, virtual time), a page reaches its
`load` event within three times Chromium's jitless `TaskDuration`
from the survey, plus the JavaScript-off time of the page; senko.net
and Hacker News within 50 ms more than with JavaScript off. Each target
in the plan states its number. Lazy compilation (M7 13f) stays later
work: compiling the scripts of the largest surveyed page takes 176 ms
and 30 MiB.

**Hostile pages.** The set (testing.md) gets cases for: an endless loop
in an inline script, in a timer, in an event listener and in a
microtask chain; `setInterval(0)` and `requestAnimationFrame` loops;
DOM growth past the node limit; deep nesting through `appendChild`;
recursion through `MutationObserver` callbacks and nested
`dispatchEvent`; huge `innerHTML`; `localStorage` past its quota;
thousands of pending fetches; with `document.write` (later), a script
that writes itself. Each case ends within the limits of the set and
logs the limit that acted.

### 17. Tests

- **Unit tests** in `web` for the conversions, the brand checks, the
  event dispatch, the timers and the journal consumers; the binding
  tests also run with the GC stress mode.
- **Page tests:** small HTML pages with scripts, loaded with
  JavaScript on and virtual time; the test compares values (read with
  `page.evaluate`, a new automation method that returns a JSON value)
  or the box dump with expected results.
- **Chromium as reference:** the probe tool gets a case option
  `"javascript": true` (page scripts run); with `--with-swb` it also
  evaluates the `js` queries in swb (`swb --headless --js --eval EXPR`)
  and compares the JSON values. Each binding feature adds a probe file
  (`tools/probes/web-*.json`), and the expected values in the Rust tests
  come from its Chromium output, as for the `js-*` probes.
- **Target fixtures with JavaScript on** (`fixtures/js/NAME`, local
  only): `reference --js NAME` and `compare --js NAME` render the same
  replay in Chromium and swb with JavaScript on and score the first
  viewport and the box dump as the JS-off comparison does; references
  and scores stay in `out/` (they come from the local fixtures). An
  integration test per target (skipped when the fixture is missing)
  loads the page, checks that no script error occurs that Chromium does
  not also have in replay, checks the load-time budget, and drives the
  target's JavaScript features (click a control, check the DOM change).
- Nondeterminism: both browsers get the same fixed start time; swb uses
  virtual time; `Math.random` and animation timing can still differ, so
  the comparisons keep the tolerances of the JS-off scores.
- `just hostile` with the JavaScript cases; `just snapshot` for the
  JS-off pages at each feature.

## Consequences

- The engine gets the structure that scripts need: a resumable parser,
  a change journal, lazy derived data and a per-document state that can
  be lent. With JavaScript off the behaviour stays the same, and the
  snapshot shows it at each step.
- The bindings are mostly hand-written behaviour behind generated glue.
  Adding a member means adding it to an IDL file, regenerating, and
  writing one Rust function.
- One runtime per document and immortal nodes make lifetime simple and
  safe in M8, at the cost of memory for detached nodes. Long-lived
  pages that rebuild their DOM will need node reclamation (section 3
  gives the design).
- Full style and layout flushes may be too slow for pages that query
  layout often; the plan measures first and names the next two steps.
- A script that runs for 5 s freezes the GUI until the page has its own
  thread.
- Without frames, pages whose features live in cross-origin frames
  (some consent dialogs, embedded players) do not work.
- `docs/architecture.md` gets the new crates, the threads section, the
  lending of the document state, and the journal.
