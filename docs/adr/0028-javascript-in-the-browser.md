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
  Fixture replay has a cookie jar for scripts only (feature 8b), stores
  only `Content-Type` and `Location`, and swb does not check CORS.
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
(Lazy interfaces: most interfaces are created at their first use now;
the numbers are in "Lazy interfaces notes", section 5.)

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
  `[LegacyFactoryFunction]` (`Image`, `Option`),
  `[LegacyWindowAlias]` (`WebKitCSSMatrix`; feature 11c1: the alias is
  another property of the global object that follows the interface
  object), `[SwbLateMembers=(a, b)]` (swb's own: the named operations,
  and `toString` of a stringifier, are defined after `constructor`, as
  Chromium installs the members that have their own `[Exposed]`), the
  named-property
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
  select that the user changed (closed by feature 9b: per-option dirtiness), the two radio
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
  lazy creation. (Superseded: "Lazy interfaces notes" at the end of this
  section has the numbers of the lazy creation.)
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
  of about 1 ms or 1 MB, so no lazy creation. (Superseded: see "Lazy
  interfaces notes".)

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
  the roadmap backlog holds the item. (Done: "Lazy interfaces notes" has
  the new numbers.) JavaScript-off pages pay nothing. Hacker News and senko.net with JavaScript on load in 19 and 12
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

Lazy interfaces notes (M8, before feature 8a; the engine part is section 6,
item 9; the tests are `crates/js/tests/lazy_globals.rs` and
`crates/web/tests/lazy_interfaces.rs`):

- **What the runtime creates at once.** The realm, the classes, `Window`
  and its ancestor `EventTarget` (the global object needs their
  prototypes, and the members of `Window` are own properties of the global
  object, whose key order they fix), the named properties object, the
  `CSS` namespace (about 1.5 KB: kept eager; a lazy value for one object
  is not worth a hook) and the serializer (15 KB, 0.12 ms: it must capture
  the built-ins before any page script, so it stays eager). Every other
  interface (the interface object, the prototype with its members, the
  `[LegacyUnforgeable]` template, the legacy factory functions) is created
  at its first use, after its ancestors.
- **The global properties.** The generated `create_interfaces` defines the
  properties of all exposed interfaces in the order of the eager creation:
  the interface objects of `Window`'s chain, and a lazy property
  (`Runtime::define_lazy_global`) for every other interface object and
  every legacy factory function (`Image` before `HTMLImageElement`, as
  before). The generated `GLOBALS` lists them; the number of a lazy
  property is its index. A lazy property has its attributes from the
  start (writable, not enumerable, configurable), so
  `Object.getOwnPropertyNames(window)`, `Reflect.ownKeys`, `in`,
  `hasOwnProperty`, `for`-`in` and `Object.keys` create nothing, and
  `delete`, an assignment and a definition with a value or an accessor
  replace the property without a creation. A read (an identifier,
  `typeof`, `window.X`, `globalThis[X]`, `with`, `Reflect.get`,
  `getOwnPropertyDescriptor(s)`, a proxy over the window) calls the
  generated `global_value`, which creates the interface and gives its
  interface object, or the factory function that the interface's `define`
  kept in the glue state. A definition that keeps the value
  (`{ enumerable: true }`, `Object.freeze(window)`) creates it first.
  `Object.getOwnPropertyDescriptors(window)` and `Object.assign({},
  window)` create every interface (they read every property).
- **Interfaces of objects.** `new_host_object`,
  `prototype_from_new_target` and `Cx::new_instance` call the generated
  `create_interface` (a dispatch from the class to its module's `define`,
  after the parent's), so node wrappers, events, exceptions, collections
  and constructors create what they need. Each class has one interface
  object per realm, in the realm's table; `global_value` reads it from
  there. So `document.body.constructor === HTMLBodyElement` holds in
  either order, and a wrapper of a node whose interface name a script
  deleted or replaced still gets the whole prototype chain.
- **Built-in objects.** What `define` read from the global object before
  (`Error.prototype` for `DOMException`, `Array.prototype.entries`,
  `keys`, `values` and `forEach` for value iterators, `Iterator.prototype`
  for pair iterators) is read once when the runtime is created
  (`glue::keep_builtins`) and kept in the glue state: a script can replace
  the global properties before an interface is created, and Web IDL names
  the intrinsics.
- **Failures.** A creation runs in a no-GC region (section 6, item 9). The
  heap limit during a creation ends the task (the check after the region,
  and the termination repeats until the task ends); the runtime is then
  dropped (section 16). A failed creation forgets the interfaces that it
  registered in the realm's table, so a later use creates them again from
  the start; the glue replaces the `[LegacyUnforgeable]` template, the
  iterator prototype and the factory functions of such a class when it
  creates them again. (The time limit can meet a creation only at a time
  charge, for example the key list of a template copy; a creation takes
  well under 1 ms.)
- **Cost** (release, senko.net, the log line "script runtime created in",
  seven interleaved runs of the build before and after): heap 1,197,385
  bytes before, 445,466 after; creation median 3.27 ms before, 1.52 ms
  after (two series: 3.47 and 1.56 ms). A page pays the rest at the
  first use of each interface: all interfaces together are the 0.75 MB
  and about 1.5 ms of before. The script heap after the `load` event
  (debug log line "load event fired"): Hacker News 797,904 bytes,
  Wikipedia 1,431,347, the Craigslist posting 1,087,271, senko.net
  591,103. The loads of the three JavaScript targets did not change
  (best of five: 21/93/17 ms before, 21/92/21 ms after in one series,
  24/99/20 and 22/95/20 ms in another).
  Departures: none that a script can observe; the base list of the
  global object's own keys with their attributes is the test's
  `lazy_interfaces/globals.txt`.

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
9. **Lazy creation of interfaces** (M8, lazy interfaces; the design is in
   "Lazy interfaces notes", section 5; `host/lazy.rs`). The host registers
   two hooks, `Runtime::set_lazy_hooks(LazyHooks { interface, global })`:
   `interface(rt, realm, class)` creates the interface of a class with its
   ancestors; `global(rt, realm, id)` gives the value of a lazy property
   of the global object. `Runtime::interface_or_create` is
   `Runtime::interface` with the hook for a missing interface;
   `new_host_object` and `prototype_from_new_target` (and so
   `Cx::new_instance`) use it, `Runtime::interface` stays the query
   without creation. `Runtime::define_lazy_global(realm, name, id)` defines
   a writable, not enumerable, configurable data property whose value is
   a third internal value variant, `Value::Lazy(id)` (beside `Empty` and
   `Cell`, ADR 0026 section 7), which never reaches a script: the heap's
   `[[Get]]` walks (`Heap::get`, `get_for_cache`) answer `Generic` for it,
   so the runtime takes its generic `[[Get]]`; the runtime's
   `[[GetOwnProperty]]` (`own_property`, `own_property_stored`) and the
   global name lookup (`get_global`) call the hook, store the value in the
   slot (the attributes stay) and go on with it; a definition that keeps
   the value (no `[[Value]]`, no accessor) resolves it first, and the heap
   refuses such a definition on an unresolved marker as an internal error;
   `global_entry` and `get_for_cache` make no cache entry for a slot that
   holds it, and `cached_global_get` misses on one; `stored_own_property`
   (the expandos of legacy platform objects) refuses it, and the message
   and JSON helpers that read raw slots skip it. Only the host writes the
   marker, before any script; a slot never gets it back. A hook runs in a
   no-GC region and a handle scope (`lazy_region`): a creation allocates
   in the middle of a property read or a wrapper creation, so the caller's
   values need no roots; the heap limit is checked at the end
   (`check_room`). After a failure the region forgets the interfaces that
   the hook registered in the realm (a later use creates them again), and
   a termination is kept in `terminating`, so every time check repeats it
   until the task ends. The hooks run no script code.
10. **Reading and making the objects that structured clone handles**
    (M8, feature 8a1; `host/inspect.rs`). `Runtime::slots_of(object)` gives
    the kind of an object and the internal slots that structured clone
    reads (`Slots`: ordinary, array with its length, `[[DateValue]]`, the
    source and flags of a `RegExp`, `Map`, `Set`, `ArrayBuffer` with its
    maximum length and detached state, a typed array or `DataView` with
    its buffer, offset and fixed or tracking length, an error, the wrapped
    primitive of a Boolean, Number, String, BigInt or Symbol object, a
    function, and `Other` for the kinds that do not clone). It runs no
    script code. `collection_entries`, `buffer_bytes`, `string_units`,
    `bigint_parts` read the contents; `create_date`, `create_regexp`,
    `create_collection` with `collection_add`, `create_array_buffer`,
    `create_view`, `create_wrapper`, `create_error` (an error with the
    given message and `stack` text, without captured frames, and no
    `message` property unless given), `string_from_units`,
    `key_from_units` and `bigint_from_parts` make them, with the
    prototypes of a realm. Strings travel as code units (`Vec<u16>`), so
    unpaired surrogates survive. `uncloneable_text` words the object in a
    `DataCloneError` (a function's whole source, else the engine's
    message text). `Runtime::display` of a host object with a `toString`
    other than `Object.prototype.toString` (`Location`, `URL`) is
    `[object Name]`, as V8 words it.

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
paint. (Feature 9a: while an intersection or resize observer has a target, a
frame is also due after a change that the observers can see, so they need no
animation frame loop; see "Feature 9a notes".)

**`requestIdleCallback`:** runs in step 4 with an `IdleDeadline` of at
most 50 ms; with a `timeout`, a timer task runs it if it is not idle in
time. (Feature 8a2: the idle period ends at the next task or frame, and the
next one starts when the last ended; see "Feature 8a2 notes", section 13.)

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

Feature 7a2 notes (`getComputedStyle` and the layout flush of a script
task. Measured with `tools/probes/web-computed-style.json` (23 cases,
316 queries; its value cases read the 843 values of
`web-cssom-values.json` through `getComputedStyle`), three equal Chromium
runs; the Rust test is `crates/engine/tests/computed_style.rs`, the
serializer's tests are in `swb_style::cssom::resolved`):

- **Layout from a script task (section 2).** The page lends its font
  context with the document state: `Page::take_document_state` moves it
  into the state (`DocumentState::lent_fonts`) and leaves an empty context
  (`FontContext::empty`, which allocates nothing) in the page;
  `return_document_state` moves it back, also when the state comes back
  after a navigation. While the state is out, the page lays out nothing (it
  has no document), so the empty context is never used.
  `DocumentState::with_lent_fonts` gives the context to a query, and
  `script_layout_flush` is the layout flush of a task (`query_layout`,
  counted as forced). Web fonts that a layout in the task finds start
  through the loader of the state, as in a frame; their completions wait
  in the loader until the task ends. 7b's geometry uses the same path.
- **The time limit.** A flush can take long in one native call, which the
  countdown of the interpreter does not see: a loop of style writes and
  layout reads on a page of 50,000 elements ran past 30 s. The binding
  calls `Runtime::check_time` (now public in `js`) after each resolved
  value, so such a loop ends at the deadline (hostile case
  `computed-style-layout-loop-js-on`: 5.4 s).
- **Resolved values (CSSOM §9).** `swb_style::cssom::resolved` serializes
  the computed values of `ComputedStyle` in Chromium's form, and the values
  that layout resolves from a `UsedBox` (border box, paddings, borders,
  the containing block, the kind of box). The engine builds the used boxes
  of all elements and pseudo-elements in one walk of the fragment tree at
  the first query after a layout (`document/resolved.rs`, keyed by the
  layout serial), so reading the width of every element costs one layout
  and one walk (hostile case `computed-style-every-element-js-on`: 100,000
  elements in 1.0 s). A value that does not need layout needs only a
  style flush: the element style on demand of 7a1, or the full styles
  when they are current; when one task asks for the styles of more than
  1/16 of the nodes (at least 256) without a change, one full restyle
  serves the rest (a script that reads every element). The style of a
  pseudo-element is its own cascade, also without a box
  (`compute_pseudo_element_style`), kept until the styles change.
- **The object.** `getComputedStyle` is `[NewObject]` in `idl/cssom.idl`
  (Chromium returns a new object at each call); the declaration is the
  `CSSStyleDeclaration` of `element.style` with another target, live
  (each read asks the engine), and read-only.
- **Chromium behaviour found.** Colors `rgb()`/`rgba()` with
  `currentcolor` resolved (`transparent` is `rgba(0, 0, 0, 0)`); lengths in
  `px` with six significant digits; values from layout in layout units,
  1/64 px truncated (`calc(100%/3)` of 400px is `133.328px`, `1cm` as a
  width `37.7812px`; at most `3.35544e+07px`); `width` and `height` are the
  used size (the border box with `box-sizing: border-box`), margins and
  paddings used values, `auto` margins resolved, but a margin, padding or
  inset of a length without a percentage is the computed value, not in
  layout units (review fix: `padding: 0.4em` at 16px is `6.4px`, `left:
  5.3px` is `5.3px`, while `width: 5.3px` is `5.29688px`), for elements whose box is
  not a part of an inline box (a `span` keeps `width: 100px`, `margin: 10%`
  as computed; `display: none` and `contents` too); insets of `absolute`
  and `fixed` boxes from layout (an `auto` one is the rest of the
  containing block, both `auto` the static position), of `relative` boxes
  the specified side resolved and the other its negation (without layout
  units: `-23.15px` against `23.1406px`), of `sticky` boxes resolved
  against the scrollport with `auto` kept; `transform` is a `matrix()`
  with percentages of the border box (a part of an inline box has a box of
  0; without a box `none`), `transform-origin` in px (without a box as
  computed); `line-height` numbers in px, `normal` stays; `min-width:
  auto` is `0px` except for flex and grid items; `letter-spacing: 0` is
  `normal`, `word-spacing: 0` is `0px`; `font-family` initial is `"Times
  New Roman"`, an explicit `serif` stays; `content: none` on an element is
  `normal`, `content: normal` on `::before` and `::after` is `none`,
  adjacent strings are joined; background and mask lists are cut or
  repeated to the number of images; outline width stays with style
  `none`. Shorthands: box sides with one to four values, `border` empty
  unless the four sides are equal, `outline` as color style width,
  `background` with every part of each layer (`rgba(0, 0, 0, 0) none repeat
  scroll 0% 0% / auto padding-box border-box`), `font` without its normal
  parts (`700 12px / 18px Arial, sans-serif`), `list-style` as position
  image type, `text-decoration` without `solid` and `currentcolor`,
  `grid` as six parts, `grid-area` and `grid-row` without the parts the
  shorthand would fill in. The object: `cssText` is `""`; writes throw
  `NoModificationAllowedError` ("These styles are computed, and therefore
  the 'color' property is read-only.", in a named setter after "Failed to
  set a named property 'backgroundColor' on 'CSSStyleDeclaration': "; the
  `cssText` setter "...computed, and therefore read-only."), also in
  sloppy mode; expandos work and `delete` of a property name succeeds
  without a change. An element that is not in the document, in template
  contents or in another document gives `""` for every property and
  `length` 0; once inserted the same object shows values. `pseudoElt`:
  `::before`, `:before`, `::BEFORE` and `before` name `::before`; `' ::before'`
  and `foo` mean the element; `'::before '`, `::foo`, `:hover` and
  `::before::after` give an empty declaration. The list of properties is
  alphabetical, the `-webkit-` names after the others, the custom
  properties of the element last.
- **Departures.** `length` and `item()` list swb's 207 longhands
  (Chromium 456). The track sizes of grid containers stay as specified
  (Chromium gives the used sizes; swb's fragments do not keep them).
  `::marker` and `::placeholder` give their style but no layout values;
  `::first-line`, `::first-letter`, `::selection` and the other
  pseudo-elements that swb does not style inherit from the element (their
  rules do not apply). The auto margins of flex items in the main axis and
  of grid items are `0px`. Properties that swb accepts and does not
  compute (`text-decoration-thickness`, `text-underline-offset`,
  `mask-type`, the `border-image-*` longhands) give their initial values.
  Values that swb's parsers or computed values do not keep: CSS Color 4
  functions as `rgb()`, `aspect-ratio` as one number over 1, no oblique
  angle, no `cursor` images, no `safe`, `last baseline` and `legacy`, no
  3D transforms, no radial or conic gradients, no `clip-path` shapes;
  `-apple-system` is `system-ui`. Where swb's layout differs from
  Chromium's (intrinsic widths of blocks, the body in quirks mode, the
  centering of an empty flex item with `margin: auto`), the values follow
  swb's layout (backlog).

Feature 7b notes (7b1: geometry, scrolling from scripts, the window's
metrics, `elementFromPoint`, `offsetX` of dispatched events, the scroll of
`focus()`, `img.width`; `innerText`, `matchMedia`, the rendering update
with `requestAnimationFrame` and the SVG geometry methods are 7b2.
Measured with `tools/probes/web-geometry.json` (25 cases, 162 queries) and
`tools/probes/web-scroll.json` (7 cases, 113 queries), three equal
Chromium runs each; the Rust test is `crates/engine/tests/geometry.rs`
with the cases in `geometry/cases.rs` and the departures in
`geometry/departures.rs`):

- **Where it lives.** The IDL is `idl/cssom-view.idl` (the partial
  `Window`, `Document`, `Element` and `HTMLElement` of CSSOM View, `Screen`
  and `ScreenOrientation`) and `idl/geometry.idl` (`DOMRectReadOnly`,
  `DOMRect`, `DOMRectList`); the bindings are `crates/web/src/cssom_view.rs`;
  the engine side is `document/geometry.rs` behind new `EngineHost` methods
  (`client_rects`, `offset_metrics`, `client_metrics`, `scroll_metrics`,
  `scroll_element`, `scroll_viewport`, `scroll_into_view`,
  `scrolling_element`, `viewport`, `elements_from_point`, `content_size`,
  `event_origins`).
- **Flushes and the deadline.** Each query that needs layout calls
  `script_layout_flush` (a forced run in `flushes`) and the binding calls
  `Runtime::check_time` after it: a loop of `style.width` writes and
  `offsetWidth` reads on a page of 50,000 elements ends at the task's time
  limit. The window's metrics, `scrollX` and `scrollY` need no flush.
- **Two indexes per layout.** The boxes of the layout (first border box,
  union, borders, paddings, whether it is a part of an inline box, and
  where the first fragment is, for its scrolling area) are built by one walk
  per layout serial; the client rectangles (each box fragment as painted:
  transforms, sticky and fixed positioning, the scroll offsets of scroll
  containers; the boxes inside an `<svg>`; the content of a closed
  `details`) by one walk per layout serial and scroll state. Reading the
  geometry of every element of a page costs one layout and one or two
  walks (hostile case `bounding-rect-every-element-js-on`).
- **Units (Chromium 148).** Rectangles are in layout units (1/64 px) where
  no transform applies; the content size, each padding and each border
  are snapped separately (a box of 100.4 px with paddings of 0.3 px is
  100.984375 px wide). Through a transform, the four corners are mapped in
  `f32` and the size is the difference of the edges (translateX(3.3px) of
  a box at 8 px gives x 11.300000190734863 and width 7.999999046325684;
  rotate(45deg) gives Chromium's values exactly). `offset*`, `client*`,
  `scrollWidth` and `scrollHeight` are the layout-unit values rounded,
  halves up (each on its own: `offsetWidth` of 100.890625 at x 46.5 is
  101). Scroll offsets are whole px, halves up (`scrollTo(0, 10.5)` gives
  11), clamped; NaN and the infinities are 0.
- **Chromium behaviour found.** `getBoundingClientRect` is the union of
  the client rectangles that are not empty (an empty `span` gives its zero
  width rectangle); a new `DOMRect` at each call, `getClientRects` a new
  `DOMRectList`. `offsetParent`: the nearest positioned or transformed
  ancestor, the body, or for a static element a `td`, `th` or `table`;
  `null` for the root, the body, fixed boxes and boxes without a box. With
  the body as offset parent the offsets are relative to the border box of
  the root element (with `html { margin: 5px }` a box at 23 px has
  `offsetLeft` 18); with none, to the initial containing block. A `br` has
  an offset parent but `offset*` 0 (Chromium lays it out as text), while
  its client rectangle is the line's. A stuck sticky box has its sticky
  offset in `offsetTop`. Inline boxes have `client*` and `scroll*` 0. The
  root element in standards mode and the body in quirks mode have the
  viewport as client area; both the root and (in quirks mode) the body
  have the viewport's scrolling area as `scrollWidth`; other boxes the
  padding box with the overflow of their descendants (no end padding). In
  quirks mode the body's `scrollTop` is the viewport's and the root's is 0;
  in standards mode the reverse. `scrollTo(x, y, z)` takes the two-number
  form, `scrollTo({top: 50}, 7)` too (the object is NaN, so 0).
  `scrollIntoView()`: `true` is block start and inline nearest, `false`
  block end, `null` and `undefined` the dictionary's defaults, other values
  booleans; all scroll containers (also `overflow: hidden`) then the
  viewport. `elementFromPoint` rounds the point (799.9 is outside an
  800 px viewport), gives the root element where no box is, skips
  `pointer-events: none` and hidden boxes; `elementsFromPoint` lists the
  hit boxes topmost first and ends with the root. `screen` is the
  viewport (800x600 in the probe, `colorDepth` 24,
  `landscape-primary`), `outerWidth` equals `innerWidth`, `screenX` 0.
  `scroll` events: one per target and frame, in the order the targets
  scrolled (a scroll container before the document if it scrolled first),
  also after a scroll back to the old position; none for a scroll to the
  current position; the document's bubbles to the window. A dispatched
  `MouseEvent` gets `offsetX` relative to the padding edge of the target
  (with the viewport's scroll, while `pageX` adds the scroll only with a
  `view`) and `layerX` relative to the border box of the nearest
  positioned inclusive ancestor (the page origin without one); a document
  or a detached target keeps the page values. `focus()` scrolls at once
  (center if needed: `scrollY` right after it shows the scroll), but not
  when the element has the focus already. swb keeps the request and
  scrolls at the next geometry query or `scrollX`/`scrollY` read of the
  task, or when the task ends, so a loop of 4,000 "append an input,
  `focus()`" on a page of 20,000 elements stays without layouts (hostile
  case `append-focus-loop-js-on`; scrolling at each call would lay out
  4,000 times). Departure (7b1 review): a script can tell the difference
  when it removes or hides the element before that point. Chromium has
  scrolled (`focus(); remove(); scrollY` gives 1223 on the review's
  page), swb has not (0). The focus change restyles, so swb cannot scroll
  at once without a layout.
  `img.width` of a rendered image is its rendered content width (at most
  33554428), otherwise the attribute.
- **Changes outside the bindings.** The engine records each scroll of the
  viewport and of a scroll container (`DocumentState::scrolled`, also the
  scrolls of a task), and the frame fires `scroll` in that order; before,
  it compared positions and fired the document first. `pointer-events:
  none` HTML boxes and text have no hit region (before, only SVG shapes
  used the property); this also changes the user's clicks with JavaScript
  off, as in Chromium. `img` `width` and `height` use `[ReflectSetter]`,
  which now takes `[ReflectRange]`.
- **Departures.** Chromium 148 returns a rejected promise when an argument
  of a scroll method does not convert (`scrollTo(5)`, `behavior: 'fast'`);
  swb throws the `TypeError` of the specification's IDL. `behavior:
  'smooth'` scrolls at once (Chromium animates). An inline box without
  borders, padding and background gets one rectangle per line in swb;
  Chromium gives the rectangles of its children (a culled inline). swb's
  layout does not snap each length to layout units (flex shares, `right:
  10%`: 1/64 px off). Matrices are `f32` (the last digits of skews
  differ). No boxes for SVG `text` and `foreignObject`, nor for an `area`
  without an image. The table's `clientWidth` (Chromium: its wrapper box).
  The quirks-mode root and body are not stretched to the viewport
  (backlog of 7a2). The largest box is 33554432 px (Chromium 33554428).
  `Screen` lacks `onchange` and `isExtended`, `ScreenOrientation` `lock`,
  `unlock` and `onchange`; no `scrollIntoViewIfNeeded`,
  `caretPositionFromPoint`. `screen` is the viewport also in the GUI.
  The `offsetX` of the user's events (6d2) stays relative to the border
  box (backlog).

Feature 7b2 notes (the scroll methods' errors, `innerText` and
`outerText`, `matchMedia`, the rendering update with
`requestAnimationFrame`, the SVG geometry methods of the targets.
Measured with `tools/probes/web-inner-text.json` (12 cases, 136 queries),
`web-match-media.json` (7 cases, 148 queries), `web-render-update.json`
(6 cases, 30 queries), `web-svg-geometry.json` (3 cases, 50 queries) and
the new case `rejections` of `web-scroll.json`, three equal Chromium runs
each; the Rust test is `crates/engine/tests/rendering.rs` with the cases in
`rendering/cases.rs` and the departures in `rendering/departures.rs`):

- **Scroll methods.** CSSOM View now declares `Promise<ScrollResult>`;
  Chromium 148 returns `undefined`, and a rejected promise instead of an
  exception when anything before the scroll fails: an argument that does
  not convert, an options getter that throws, and the brand check (its
  message then names the operation: "Failed to execute 'scrollTo' on
  'Window': Illegal invocation"). The rejection is unhandled, so
  `unhandledrejection` fires. swb's IDL keeps `undefined` and marks the
  methods `[SwbRejectsOnError]`; the generator gives such operations the
  promise path of Web IDL §3.7.6 (`glue::run_promise`). The departures of
  7b1 for these calls are gone.
- **`innerText`** (HTML §3.2.7). The engine computes the rendered text
  (`document/inner_text.rs`, the "rendered text collection steps" with the
  specification's list of strings and required line break counts) from the
  styles: an element is being rendered if its style and its ancestors' are
  not `display: none` and no ancestor hides its children (replaced
  elements and controls, a closed `details`, the non-option children of
  `select`). So a read costs a style flush (a forced style run), not a
  layout; Chromium lays out, which no script can tell apart. White space
  is processed per line as layout does it, `text-transform` with layout's
  function (`swb_layout::transform_text`). Chromium behaviour found: a
  floated, absolutely positioned, flex or grid item is a block (the
  computed `display`); a table cell that is not the last of its row adds a
  tab, a row that is not the last of its table a line feed, and these
  separate the runs of line breaks (`<td><p>p</p></td><td><div>dv</div>`
  gives `p\n\n\t\ndv`); `select` gives its options on lines (not the
  `optgroup` labels); `textarea`, `input`, `img`, `canvas`, `video`,
  `audio`, `iframe`, `progress` and `meter` give nothing, `object` its
  fallback; SVG `text` is a block; `visibility: hidden` text gives nothing
  but a visible descendant does; `display: none` elements and elements
  outside the document give their `textContent`; NBSP stays; soft wraps
  add nothing. The setters make the "rendered text fragment" (one `br` for
  each `\r\n`, `\n` or `\r`); `outerText` merges the text at its edges with
  the text nodes next to it, leaves one empty text node for an empty
  value, and throws `NoModificationAllowedError` ("The element has no
  parent."). Departures: `content-visibility` (swb has none), MathML
  (Chromium makes `math` a block with italic letters). (Feature 9a: the
  setter makes one record, as in Chromium.)
- **`matchMedia`** (CSSOM View §4.2). A `MediaQueryList` is an event target
  whose generic data holds its listeners and its state (the parsed list of
  `swb_css`, the serialization, the matches state); `matches` evaluates at
  each read (Chromium: current inside a `resize` listener, before the
  change events). `onchange` needed a third kind of handler target in the
  generator and in `events/handlers.rs` (an object that is neither a node
  nor the window). Only lists with a listener are evaluated in the
  rendering update, in the order of their creation (measured: the order of
  `matchMedia` calls, not of the listeners), so lists made in a loop keep
  nothing alive. The serialization of `swb_css::MediaQueryList` now keeps
  what Chromium keeps: `only`, unknown media types (`tv`), the
  `min-`/`max-` form (`(min-width: 500px)`), unitless `0`, the written
  resolution unit, ratios as `4 / 3`, nested parentheses and the text of
  `<general-enclosed>` (`(foo: bar)`); invalid queries are `not all`. The
  media environment gets `prefers-color-scheme` from the page
  (`Page::set_color_scheme`, light by default; JavaScript off renders as
  before). Departures: `calc()` in a query is kept as written and cannot
  use `em` (Chromium simplifies and evaluates it); swb knows
  `inverted-colors`, Chromium 148 does not.
- **The rendering update** (section 7; `page/frame.rs`). A frame runs, in
  HTML's order, `resize` at the window (when the viewport size changed),
  `scroll` (7b1), the media query changes and the animation frame
  callbacks; each callback is a task of its own (its deadline, a microtask
  checkpoint after it). A frame is due one frame length after a scroll, a
  resize or a change of the media environment, and for animation frame
  callbacks on a grid of 60 frames per second of page time (the frame time
  is the grid time, so a late pump keeps the grid; after a pause one frame
  runs at once, not one per missed frame). In virtual time the clock jumps
  to the frame like to a task. Style and layout are not part of the frame:
  they stay lazy (painting in the GUI, queries, screenshots), so a page
  with an animation loop does not lay out at each frame unless it reads
  geometry. A page whose only work is a `requestAnimationFrame` loop
  counts as idle (`Page::is_idle`). Chromium behaviour found: handles start
  at 1 and grow by one; all callbacks of a frame get the same time, not
  later than `performance.now()`; callbacks that a callback requests run
  in the next frame, one that it cancels does not run; `this` is
  `undefined` (the window in sloppy functions); headless Chromium runs
  frames 16.7 ms apart on average; `resize` comes before the change events
  of the same frame, which come before the animation frame callbacks.
- **SVG geometry.** `getBBox` (the fill box in the element's user space),
  `getTotalLength` and `createSVGRect` return Chromium's `SVGRect` (SVG
  1.1, `float` attributes; `getBBox() instanceof DOMRect` is false). The
  engine finds the content of the outer `<svg>` in the fragment tree and
  asks it for the boxes and lengths of all its elements at once
  (`SvgContent::script_geometry`), kept until the next layout (review
  fix: a loop of `getBBox` and `getTotalLength` over 4,000 paths walked
  the fragments at each call and took 2.1 s). Of the targets, Leaflet on the
  Craigslist posting tests `'createSVGRect' in svg` (and so uses its SVG
  renderer) and calls `getTotalLength` when it spiderfies markers;
  Wikipedia's startup check tests `'createSVGRect' in svg`. Departures:
  curves are measured with 64 chords each (a circle of radius 10 is
  62.839 long; Chromium's approximation gives 62.427); an element that
  swb does not draw has the length 0 and the box 0, 0, 0, 0, where
  Chromium computes them from the attributes (`display: none`, detached,
  in `defs`, and the elements of a nested `<svg>`, which swb draws as
  part of the outer one); text has no box. The `SVGRect` IDL describes
  Chromium's measured members.

Feature 11c1 notes (CSS from scripts for the Craigslist posting page;
measured with `tools/probes/web-style-inert.json` (6 cases, 48 queries
with about 600 values) and `tools/probes/web-geometry-matrix.json` (14
cases, 53 queries), three equal Chromium runs each; the Rust tests are
`crates/engine/tests/style_inert.rs` and
`crates/web/tests/geometry_matrix.rs`):

- **Inert properties.** Leaflet and the page's slider test and write
  `transition`, `transitionDuration`, `webkitTransition`, `willChange`,
  `touchAction` and `perspective` (`'transition' in el.style`,
  `el.style.transitionDuration = '0ms'`). swb supports these as
  properties that have no effect on layout and paint:
  `transition` with `transition-property`, `-duration`,
  `-timing-function`, `-delay` and `-behavior`, `will-change`,
  `touch-action`, `perspective` and `perspective-origin`, with the
  `-webkit-` aliases that Chromium has (`-webkit-transition` and its
  longhands except `-behavior`, `-webkit-perspective`,
  `-webkit-perspective-origin`; `MozTransition`, `OTransition` and
  `msTransition` do not exist in Chromium either). Each is parsed with
  its grammar, cascaded (the shorthand sets five longhands; `inherit`,
  `initial` and `var()` work as for any property), computed, serialized in
  `element.style` and `getComputedStyle`, and found by `CSS.supports` and
  `@supports`; swb runs no transition and draws no perspective. Code:
  `style/src/properties/inert.rs`. The computed values live in one
  optional shared record, `ComputedStyle::inert` (8 bytes, `None` while
  all are initial), as the text of the computed value; so the style of
  an element that uses none of them costs one pointer, and the cascade,
  `inherit` and `initial` need no code per property. Measured rules:
  times are written in the unit given (`300ms`) and computed in seconds
  (`0.3s`); a timing function is `steps(4)` for `steps(4, end)` and, in
  the computed value, `steps(1, start)` for `step-start`; known property
  names in `transition-property` are lower-cased; `will-change` keeps the
  case of its names; `touch-action` has its keywords in the order
  horizontal pan, vertical pan, `pinch-zoom`; the shorthand leaves out
  what is initial, writes `0.3s` for `all 0.3s`, `opacity 2s` for
  `opacity 0s 2s` (a delay reads as a duration), and `all` for nothing;
  its longhands in a block come in the order behavior, duration, timing
  function, delay, property, and a shorthand with fewer than five longhands
  in the block reads as `""`; lists of unequal length are padded with the
  initial value per layer. The probe page of the tools disables
  transitions with `transition: none !important`, so the computed cases
  write with `!important` in the style attribute, which wins. Departures:
  `calc()` in times, `linear()` as a timing function, and Chromium's
  knowledge of more property names than swb parses (a name in
  `transition-property` that is a property for Chromium and not for swb is
  not lower-cased); `transform-style` and `backface-visibility` are not
  supported (backlog).
- **Geometry interfaces.** `DOMMatrixReadOnly`, `DOMMatrix` (with
  `WebKitCSSMatrix` as its window alias, the same object, as in
  Chromium), `DOMPointReadOnly`, `DOMPoint` with all members of the
  specification: constructors from a string, a sequence of 6 or 16
  numbers or nothing; `fromMatrix`, `fromFloat32Array`,
  `fromFloat64Array`, `fromPoint`; `a` to `f`, `m11` to `m44`, `is2D`,
  `isIdentity`; the transform methods and their `...Self` forms;
  `inverse`, `transformPoint`, `matrixTransform`, `setMatrixValue`,
  `toFloat32Array`, `toFloat64Array`, `toJSON`, the stringifier. Code:
  `web/src/matrix.rs` (the math), `web/src/dom_matrix.rs` (the glue).
  The string form uses `swb_style::parse_transform_list`
  (`style/src/transform_list.rs`: all 2D and 3D functions; as in
  Chromium, the numbers of `matrix()` and `matrix3d()`, the axes and the
  angles in `f64` (the CSS number token's `value_f64`), lengths in px and
  scale factors in `f32`; a relative length or a percentage is a
  `SyntaxError` "Values must be resolvable at parse time"; any other
  failure is "Failed to parse '...'."). Measured: the operations are
  post-multiplications written as Chromium's results show (a translation
  adds to the fourth column, a scale multiplies columns, so `NaN`
  propagates and `scale(-1)` makes `-0`); an angle is reduced to less
  than one turn before its sine and cosine (`rotate(1e17)` is
  `rotate(280)`), and those of the multiples of 90 degrees are exact;
  `perspective(0)` is `perspective(1px)`;
  a 2D matrix transforms a point without mixing in `z` and `w`;
  the stringifier writes numbers as `Number::toString` and throws
  `InvalidStateError` for non-finite values; `m13`, `m14`, `m23`, `m24`,
  `m31`, `m32`, `m34`, `m43` set to a value other than 0 and `m33` and `m44`
  set to a value other than 1 make the matrix 3D for good; `is2D` and
  `isIdentity` are not settable; `new DOMMatrix(object)` converts the object
  to a string. The generator got `[LegacyWindowAlias]` (the alias is
  another property of the global object, right after the interface) and
  `[SwbLateMembers=(a, b)]` (the operations that Chromium installs after
  `constructor`: `setMatrixValue`, the stringifier), and the types
  `Float32Array` and `Float64Array`. Limits: a matrix string over 1 MiB is
  a `SyntaxError` (Chromium has no limit), so the tokens of a hostile string
  cost at most their text; hostile case `dom-matrix-strings-js-on`.
  Departures: an explicit `undefined` in the middle of the arguments of
  `scale()` and `rotate()` is a missing argument (Chromium converts it to
  `NaN`); `calc()` in a matrix string; `DOMQuad`, `SVGMatrix` and
  `SVGPoint` are not in swb (backlog).

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
  (Feature 10b did all three: see the 10b notes below.)
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
  memory, for the life of the process, 5,242,880 code units per origin
  (measured in Chromium, feature 8b); a write past it throws `QuotaExceededError`; the
  `storage` event goes to other documents of the origin (in M8 there
  are none).
- Each document has at most 8 requests from scripts running, at most 6
  per host, on workers of their own; others wait in a queue. The
  requests and the responses not yet in the heap share a byte budget
  (section 16, feature 10a notes).

Feature 10a notes (`fetch`, `Headers`, `Request`, `Response`,
`AbortController`, `AbortSignal`, CORS; measured in
`tools/probes/web-fetch.json` and `web-fetch-cors.json`, three equal
Chromium runs each; swb's differences from Chromium are listed in the Rust
tests that run the cases, `crates/web/tests/fetch.rs`):

- **`net`.** `Method` holds all methods (`parse` normalizes the case of the
  six that Fetch names; `patch` stays `patch`; `CONNECT`, `TRACE` and
  `TRACK` are refused by the script API). A `Request` has a mode
  (`navigate`, `same-origin`, `no-cors`, `cors`), a credentials mode, a
  redirect mode, the response tainting of the current hop and the
  tainted-origin flag. `net::fetch_request` is Fetch's main fetch and
  HTTP fetch over a `Fetcher`: it sets the tainting per hop, sends the
  preflight (`cors.rs`: `needs_preflight`, `preflight_request`,
  `check_preflight`, a bounded `PreflightCache`), runs the CORS check on
  every response of a `cors` request (also on a redirect response), follows
  the redirect modes, and filters the response for a script (basic,
  `cors` with the exposed headers, opaque, opaque-redirect). The loader runs
  it on its workers, keeps one preflight cache for the document and
  returns the response kind with the completion. `Loader::cancel` marks a
  request: one that no worker started completes with `NetError::Aborted`
  without a fetch. Cookies go (and `Set-Cookie` is stored) as the
  credentials mode says: `include` always, `same-origin` while the tainting
  is basic, `omit` never; the jar's `SameSite` rules apply on top (ADR
  0012). `Origin` is sent as Fetch's "append a request `Origin` header"
  says (`Request::origin_header`): always for a `cors` tainting, else for
  methods other than `GET` and `HEAD`; `null` after a redirect from a
  cross-origin URL to a third origin.
- **Replay.** `ReplayFetcher` answers preflights permissively and adds the
  CORS headers described above; it reads `replay_patterns` (the Python
  replay's rule), so the 460 alias entries of the local Craigslist fixture
  are not needed (the cleaned manifest: 22 entries, same scores).
- **Chromium behaviour found.** The CORS check wording of the console (the
  texts are in `cors.rs`, `CorsFailure::reason`; Chromium 148 has no
  "If an opaque response serves your needs..." sentence); the console also
  prints `Failed to load resource: net::ERR_FAILED` after each, which swb
  does not copy. A `Access-Control-Allow-Headers: *` also covers
  `Authorization` (the standard excludes it). A preflight status outside
  200 to 299 and a redirect of the preflight fail it. `fetch()` of
  `blob:`, `file:`, `about:`, `ftp:` and `javascript:` URLs rejects with
  `TypeError: Failed to fetch` (console: `Fetch API cannot load X. URL
  scheme "s" is not supported.`); `data:` works (type `basic`, status 200,
  `content-type` the only header). Mode `no-cors` with a redirect mode other
  than `follow` fails. Mode `same-origin` to another origin fails without a
  request. An abort after the response makes the unread body reject with
  `AbortError` "The user aborted a request." (whatever the reason; the
  body stays unread); swb copies this for the bodies it holds. `Response`
  statuses outside 200 to 599 are a `RangeError`; a `status` of `'a'`
  converts to 0 (no `[EnforceRange]`). `new Headers([['a']])` is
  `Invalid value`. A `Request` has its own `signal` that follows the
  given one.
- **Bodies and streams.** Bodies are bytes in memory: a string,
  `URLSearchParams`, `ArrayBuffer` and views (`body::extract` is where
  `Blob` and `FormData` attach in 10b). `body` is `null` (one warning per
  document), `blob()` and `formData()` reject with `NotSupportedError`
  until 10b. The bytes of a `Request` or `Response` count in the heap (the
  object's `heap_size`). The response is complete when the promise
  resolves.
- **Engine.** The page lends the document state; `fetch()` calls
  `EngineHost::start_api_fetch` (the document adds its origin as the
  initiator, refuses URLs it may not load) and the loader runs the request.
  They run on a pool of 8 workers of their own (`Loader::start_api`), so
  a server that never answers cannot hold the workers of the page's
  images, scripts, style sheets and navigations (review of 10a: six
  `no-cors` calls to such a server took all six workers for up to 60 s).
  At most 8 run at once and at most 6 to one host (Chromium: six
  connections per host); the next ones wait in a FIFO queue of at most
  16,384 requests, and the first 256 of the queue are looked at for a
  request to a host with a free place, so a healthy host passes the queue
  of a hung one. A healthy fetch waits only when 8 requests hang (or 6 hang
  at its own host); it then waits for a place until one of them ends (at
  the latest the 60 s request timeout). Chromium would not wait for hung
  requests of other hosts at all (256 connections in all); the difference
  shows only past 8 hung requests. When a navigation commits, the old
  document's calls are dropped as on a stop, so its hung requests give
  their places to the new document (delta review of 10a: the new
  document's calls waited for them). A call past the queue, or one that does
  not fit in the byte budget, fails at once with `TypeError` and a warning.
  They are not in the document's request table, so they do not delay
  `load`; the waiting loops of the page (`evaluate` of a promise, the
  virtual clock) count them as activity: in virtual time, timers do not
  fire while a `fetch()` is pending (time moves only when nothing is in
  flight). A completion comes back as a task
  (`ScriptRuntime::api_fetch_completed`).
  An abort drops a queued request. A running one cannot be interrupted in
  its blocking read: the loader puts a new thread in the place of the
  blocked worker (`Loader::cancel`; at most 16 such threads, each ends
  with its request), and the call gives up its place for the next request;
  past 16 such threads the call keeps its place until its completion
  arrives. The bytes that the blocked thread still reads count against the
  budget until it lets go. A navigation or `Page::stop` drops the calls:
  their promises stay pending. A pooled connection that the server closed
  is retried only for the idempotent methods (`GET`, `HEAD`, `OPTIONS`,
  `PUT`, `DELETE`; twice); `POST`, `PATCH` and other methods always use a
  new connection and are never sent again, as the server may have read
  them (review of 10a: a `POST` was sent twice; `ureq` does not tell
  whether a connection came from its pool). Form submissions take the same
  path. A `redirect: 'manual'` request whose `Location` has another scheme
  (`ftp:`, `file:`) gives the opaque-redirect response, as Chromium does
  (probe case `fetch-edge`); the scheme check belongs to following.
  A redirect with credentials in its `Location` fails only if the
  request's origin differs from the `Location`'s origin (Fetch), also
  after a chain A -> B -> A. A response that does not fit in the heap
  rejects the promise with `TypeError`.
- **Virtual time while a fetch is pending** (measured in review: one
  `no-cors` `fetch()` to a server that never answers and a
  `setTimeout(..., 1000)`; `--virtual-time --timeout 20 --settle-ms 3000`):
  the run took 25 s and the timer never fired; Chromium fires it. The
  wait is bounded by the caller's real-time timeout and by the 60 s request
  timeout of the network layer, as for any pending load. This stays a
  departure.
- **Signals.** `AbortSignal` is an event target (generic data with a
  `SignalState`); `AbortSignal.any()` flattens the sources, and a source
  keeps its dependents strongly (at most 65,536; the standard keeps them
  weakly); a signal tracks at most 65,536 listeners added with the
  `signal` option; at most 262,144 `AbortSignal.timeout()` signals wait for
  their timer task (the task ends past it). `timeout()` fires from the event
  loop in page time.
- **Departures** (rows in `crates/web/tests/fetch.rs`): `body` is `null`;
  the property order of `Request.prototype` and `Response.prototype` (the
  generator sorts operations, Chromium puts `bytes` last and has
  `targetAddressSpace`); `statusText` is the canonical phrase of the code,
  not the server's; `json()` uses the page's `JSON.parse`; swb sends no
  `Referer` and no `Sec-Fetch-*` headers; methods other than the nine
  standard ones (and `patch` in lowercase) are network errors (the HTTP
  library refuses them); header values are strings, not byte strings;
  `Request.destination` is always `""` and `referrer` is not sent.

Feature 10b notes (`XMLHttpRequest`, the `load` and `error` events of
images and style sheets, `FormData`, `Blob`, `File`, `FileReader`, Blob
URLs, `navigator.sendBeacon`; measured in `tools/probes/web-xhr.json`,
`web-image-events.json` and `web-blob-formdata.json`, three equal runs of
Chromium 148 each; the Rust tests run the same cases):

- **Files.** `web/idl/xhr.idl` (XHR, `FormData`, `ProgressEvent`,
  `FormDataEvent`), `file-api.idl` (`Blob`, `File`, `FileReader`,
  `createObjectURL`), `Navigator.sendBeacon` in `navigator-extensions.idl`,
  `HTMLImageElement.decode()` in `html.idl`; `web/src/xhr.rs`,
  `file_api/{mod,form_data,reader}.rs`, `image_events.rs`,
  `navigator.rs` (`sendBeacon`), `fetch/mod.rs` (`start_request`, the table
  of XHR objects, Blob URL and refused requests as local completions);
  `engine/src/document/element_events.rs` (the watched images and the
  queue of events); `net` got the `use_cors_preflight` flag of a request.
- **XHR path.** `send()` builds a `swb_net::Request` (mode `cors`, credentials
  `same-origin` or `include`) and starts it as `fetch()` does. The result
  comes back in the task of `api_fetch_completed` and the whole sequence of
  events runs in that task, in Chromium's order: `readystatechange` 1 and
  `loadstart` at the request in `send()`; with a body also `loadstart` at
  `upload`; on the response `progress`, `load`, `loadend` at `upload` (not
  for a body of 0 bytes), `readystatechange` 2, 3 and one `progress` (not for
  an empty body), `readystatechange` 4, `load`, `loadend`. A network error
  gives `readystatechange` 4, `error`, `loadend`; `abort()` and the `timeout`
  add `readystatechange` 4 first and the events at `upload` (`error` fires
  at `upload` only with a body). After each event the code checks that the
  request is still in the state it expects (a listener may abort or reopen);
  `load` and `loadend` fire without a check. `abort()` in a `load` listener
  gives `loadend` with `readyState` 0. A timer of 0 ms and the microtasks run
  before the first event of the response. A listener at `upload` and a body
  force a preflight (Chromium sends none for a request without a body). A
  `timeout` is a task at the page time of `send()` plus the timeout.
- **Departures of XHR** (rows in `crates/engine/tests/probe_net.rs`): a
  synchronous `open` throws `InvalidAccessError`; one `progress` event (Chromium
  fires one per chunk and a last one at `readyState` 4); `responseXML` and
  `responseType` `"document"` are `null` for XML (no XML parser) and the
  HTML document has the URL of the page; `statusText` is the standard phrase;
  a `Document` body is sent as its string; the order of the members of the
  prototype (the generator's); an odd trailing byte of UTF-16 text.
- **Images and style sheets.** A document parsed with scripting watches its
  `img` elements (`watch_image`): the journal drain calls it for every `img`
  that was inserted or whose attributes changed, also outside the
  document (`new Image()`); it selects the source (`image_source`), starts
  the load if the URL is new (`start_image`) and queues `(node, ok)` when the
  load ends, or at once when the URL loaded or failed before. A new `src`
  with the same URL fires `load` again (Chromium does). `src=""` fires `error`
  (a task later, `complete` true); removing `src` fires nothing.
  `complete`, `naturalWidth`, `naturalHeight` and `currentSrc` come from the
  same table (`EngineHost::image_info`); `currentSrc` is empty until the
  load ended (Chromium: set a microtask after `src`). A `link` style sheet queues its
  event when the load ends. The page hands the queue to the runtime in the
  pump and before the `load` check of a completion, so the events precede
  window `load`; a load that script created delays window `load` (it is a
  request of the document). `decode()` resolves with the event and rejects
  with `EncodingError` (also `complete` images with no size). Departures
  (rows in `probe_net.rs`): swb watches 65,536 images at most; a style sheet
  that blocked a script fires `load` before the script runs (Chromium after);
  `link.sheet` is `null` (backlog); `rel=preload` fires nothing; an `img` with
  a Blob URL fails (the loader has no `blob:` scheme; backlog); the engine
  parses without yielding to the network between inline scripts, so a fast
  image of a parsed document fires after `DOMContentLoaded` (Chromium before,
  when the parse takes several tasks); a changed viewport that selects another
  source fires no `load`.
- **Blob.** A blob is a list of pieces (slices of shared byte vectors), so a
  slice or a concatenation copies the list, not the bytes; more than 1,024
  pieces are flattened; a blob is at most 64 MiB (`RangeError`). The data of a
  blob object charges the sizes of the distinct vectors that it holds (a
  slice keeps its parent's vector alive, so the logical length would
  undercount); a slice of less than half of the vectors it uses is copied when
  they are longer than 64 KiB, so a small slice never holds a large parent.
  The 256 MiB limit then bounds the memory of the live blobs (blobs that
  share a vector each charge it). The list is flattened past 1,024 pieces plus
  one per KiB of the last flat copy, so many small parts cost a copy of
  about 1 KiB each, not of the whole blob. `type` is
  lower-cased ASCII or empty. `File` adds `name` and `lastModified`.
  `text()`, `arrayBuffer()` and `bytes()` copy into the heap. `Response.blob()`
  has the type of the `Content-Type`; `Response.formData()` parses
  `multipart/form-data` and `application/x-www-form-urlencoded` and rejects
  with `TypeError: Failed to fetch` for other types (Chromium).
- **Blob URLs.** `blob:ORIGIN/UUID` (origin `null` for a `file:` page); the
  store is per document (16,384 URLs, 256 MiB of bytes that the entries keep
  alive, counted as for the heap; a `QuotaExceededError` past them).
  `fetch()` and XHR of a `blob:` URL answer from the store with status 200 and
  `Content-Type`, `Content-Length`; a revoked URL, another method or an
  unknown URL is a network error. The request keeps the blob's handle (a
  cheap clone) until its task runs and copies the bytes then, one request at
  a time; at most 256 such requests wait, the others fail like a refused
  request (warning).
- **Review fixes of 10b.** (1) The bytes outside the script heap that XHR
  and `FileReader` hold (response bytes, the text cache, the request headers,
  a read's result) are charged when they change (`Heap::generic_resized`),
  and a task that ends with a collection due runs a safepoint
  (`ScriptRuntime::collect_between_tasks`): the tasks of a response run no
  script, so nothing else would collect; past the limit the document's
  scripts stop, as for any heap growth. (2) `setRequestHeader` keeps at most
  256 headers and 64 KiB of names and values per request; a header past either
  is dropped with one warning per request (a real page sets a dozen, a few
  hundred bytes; Chromium has no such limit, the server's limits act there).
  (3) An XHR object that left the table of requests in flight stays alive for
  the task that delivers its events (a collection in a listener freed it:
  found by the test of the Blob URL requests). (4) A GET with a body of
  `null` leaves the upload flag unset at the response, so an `abort()` after
  the headers fires `abort` and `loadend` at `upload` (Chromium, measured;
  XHR "send" sets the flag at once for a null body); an `abort()` in the
  `loadstart` listener fires no `loadstart` at `upload`. (5) The timeout task
  leaves the queue when the request ends or aborts. (6) `Page::stop` drops the
  XHR entries of the requests in flight (`xhr::drop_requests`) and their
  timeouts; the objects keep their `readyState` and fire no `abort`, `error`
  or `timeout` (a departure from the standard's "terminate"; not measured).
  (7) A `FileReader` whose result does not fit in the heap ends with
  `readyState` 2, an `error` event (a `NotReadableError`) and `loadend`, and
  can read again. (8) The table of image watches drops the finished watches of
  images outside the document when it is full (a search at most once per 256
  attempts while it finds nothing), so a long-lived page keeps its `load`
  events and `complete`. Delta review: (9) a request of a Blob URL and a
  `FileReader` read keep their snapshot of the blob until their task runs,
  also when the page drops the blob, and nothing counted these bytes (200
  reads or requests of 32 MiB blobs held 3.6 GB). The vectors that they hold
  now count once each against 256 MiB per document (`HeldBlobs`); past it a
  request is a network error and a read ends with `error`. Warnings of
  refused requests and failed reads come once per document. (10) The `on*`
  handlers of event targets that are not nodes (`XMLHttpRequest`,
  `FileReader`, `AbortSignal`, `MediaQueryList`) were in the document's
  handler map, which the collector traced as a root, with the target's data:
  an XHR with an `onload` handler (as jQuery sets one) stayed alive with its
  response until the document ended (8 unreferenced responses of 30 MiB
  stopped the scripts at the heap limit). The handlers of these targets are
  now in the target's own data (`OwnHandlers` in `TargetData`). A
  `MediaQueryList` with a handler stays registered as before. This showed two
  places that took an object out of its table before its last events
  allocate, as the XHR of (3) did: a `FileReader` in its last task and an
  `AbortSignal.timeout()` signal in its timer task (also a 10a signal with
  only `addEventListener` listeners); both now live as long as the task's
  scope. A test in the GC stress mode covers them. The promise and the
  signal of a `fetch()` in its completion task have the same shape (no
  loss seen, also in the stress mode) and are recorded in the scope too.
- **FormData.** The entry list of `new FormData(form, submitter)` and of a
  form submission comes from the engine's submission code
  (`EngineHost::form_entries`), then the trusted `formdata` event fires at the
  form (it bubbles; `FormDataEvent`); a listener may change the list, and a
  submission uses the changed list (`request_form_submission` takes it). A
  `FormData` made inside the event throws `InvalidStateError`. A `Blob` value
  becomes a `File` named `blob`. The multipart body: boundary
  `----WebKitFormBoundary` and 16 letters or digits; `name` and `filename` escape
  `"`, CR, LF as `%22`, `%0D`, `%0A`; text with CR LF line breaks; `Content-Type`
  of a file defaults to `application/octet-stream`; the encoder checks the time
  every 256 entries and refuses a body of more than 64 MiB.
- **FileReader.** A read queues two tasks (Chromium's order): `loadstart`; then
  `progress`, the result, `load`, `loadend`. `abort()` fires `abort` and
  `loadend` with `readyState` 2 and `error` an `AbortError`. A second read
  during a read throws `InvalidStateError`. One `progress` event.
- **sendBeacon.** A `POST` with credentials `include`; mode `no-cors` for a
  CORS-safelisted type or no body, `cors` (with a preflight) for another type;
  `false` if the data passes the quota of 65,536 bytes that wait (the quota
  returns when the request ends: measured, 10 times 10,000 bytes give six
  `true`); `TypeError` for a bad URL or a scheme other than `http:` and
  `https:` (Chromium's texts).

Feature 8b notes (`document.cookie`, `localStorage`, `sessionStorage`;
measured in `tools/probes/web-cookies.json` and `web-storage.json`, three
equal Chromium runs each):

- **`document.cookie`** (`web/src/cookie.rs`). The getter and setter call
  `EngineHost::cookie_string` and `set_cookie`; the engine's document goes
  to the jar of the page's loader (`CookieJar::script_cookie_string`,
  `set_script_cookie`: the non-HTTP API of RFC 6265bis §5.7 and §5.8, ADR
  0012). A document without a browsing context (`createHTMLDocument`,
  `DOMParser`, template contents) and a document whose URL is not `http:`
  or `https:` (`file:`, `about:`) read `""` and ignore writes; a `data:`
  document throws `SecurityError` ("Cookies are disabled inside 'data:'
  URLs."). The `SecurityError` of other opaque origins (sandboxed frames)
  cannot occur: swb has no frames. `navigator.cookieEnabled` is true on
  every page (Chromium: true on `file:` pages too). The replayed fixture
  has an in-memory jar (`ReplayFetcher`) that only scripts fill; it sends no
  `Cookie` header. JavaScript off leaves it empty. A lone surrogate in the
  cookie string becomes U+FFFD (`USVString`); Chromium's jar stops
  answering the page after such a write (measured), which swb does not
  copy. Departures from Chromium: none that the probe shows (123 queries
  equal). The jar rules that Chromium's answers changed (a cookie replaced
  with a new value moves behind the others, with the same value it keeps
  its place; `Partitioned` needs `Secure`; `__Host-` and
  `Domain=<IP host>`, a tab inside a name or value) are in ADR 0012.
- **`localStorage` and `sessionStorage`** (`web/src/storage.rs`). The
  `Storage` interface has the members in Chromium's prototype order; the
  named properties use the legacy platform object support (ADR 0028 feature
  4c1), so `localStorage.x = 1`, `'x' in localStorage`, `delete
  localStorage.x`, `Object.keys` and `JSON.stringify` work. The data is a
  `StorageStore` that the `Page` owns and lends to each document
  (`EngineHost::storage`): both kinds per origin, in memory;
  `localStorage` for the process and `sessionStorage` for the page; both
  survive navigations. The origin is that of an `http:` or `https:` URL;
  all `file:` documents share `file://` (Chromium's `file:` pages have
  storage); `data:` throws `SecurityError` "Storage is disabled inside
  'data:' URLs." and other URLs "Access is denied for this document."
  (the second text is Chromium's default, not measured: the probe tool
  cannot open such a top-level document). `window.localStorage` returns
  the same object each time.
- **Quota** (measured): 5,242,880 UTF-16 code units of all keys and values
  of an area together, in `localStorage` and in `sessionStorage`, each per
  origin (an emoji counts 2, a key counts as much as a value, a replaced
  value counts the difference, an equal value is no write). A write past it
  throws a `DOMException` `QuotaExceededError` (code 22) "Failed to execute
  'setItem' on 'Storage': Setting the value of 'KEY' exceeded the quota.",
  for `localStorage.x = v` "Failed to set a named property 'x' on
  'Storage': ..."; the area stays as it was. swb bounds what Chromium does
  not: 100,000 keys per area and 64 MiB of memory over all areas of both
  kinds and all origins (UTF-8 bytes of keys and values, 64 more per key,
  the origin and 64 more per area); a write past either throws the same
  error. A replaced value gets a new allocation and removals shrink the
  area's tables, so the memory follows the count. See the table in
  section 16.
- **One page per process.** `localStorage` lives for the process because
  the `Page` owns the store and swb has one `Page` per process (GUI,
  headless runner, automation server). With tabs or several pages, the
  `localStorage` areas must move out of `Page` into a store that all
  pages share (`sessionStorage` stays per page).
- **Order of keys** (`key(i)`, `Object.keys`): Chromium's order is that of
  a hash table and cannot be matched; swb uses the order of insertion,
  except that `removeItem` moves the last key to the place of the removed
  one (an O(1) removal). The standard leaves the order open.
- **Departures** (rows in `crates/web/tests/storage.rs`): a name that the
  prototype hides (`getItem`) is a stored key but not listed by
  `Object.keys`; `localStorage.getItem = x` and `localStorage.length = 7`
  go to the named setter (Web IDL; Chromium makes an own property, or
  ignores the write for the accessor `length`) (feature 4c1);
  `QuotaExceededError` is a plain `DOMException` (no subclass, no `quota`
  and `requested`, no `stack`); lone surrogates in keys and values become
  U+FFFD (section 3; Chromium keeps them).
- **`StorageEvent`**: `storageArea` is a `Storage` (a `TypeError` for any
  other object, as in Chromium). No `storage` event fires in M8: it goes
  only to the other documents of the origin, and a page has one document and
  no frames. A second document would get it as a new `Task` variant, queued
  by the writing document for each other document of the origin and run by
  `runtime.rs::run_task`, which dispatches a `StorageEvent` (`key`,
  `oldValue`, `newValue`, `url`, `storageArea`) at the window of the
  receiving document.

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
  only. (Feature 9a: see "Feature 9a notes" below.)
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

Feature 8a1 notes (`URL`, `URLSearchParams`, `location`, `history`,
`popstate`, `hashchange`, `structuredClone`; measured with
`tools/probes/web-url.json` (27 cases, 787 queries), `web-structured-clone.json`
(11 cases, 177 queries) and `web-location-history.json` (37 cases), three Chromium runs each with equal values; the Rust tests are
`crates/web/tests/url.rs`, `structured_clone.rs` and
`crates/engine/tests/script_location.rs`, and three hostile cases):

- **`URL` and `URLSearchParams`** (`web/src/url_api.rs`, `idl/url.idl`) keep a
  `url::Url` and, once read, the `searchParams` object, linked both ways: a
  change of the list sets the query (the "update steps"), a new `search` or
  `href` sets the list. The attribute setters follow the standard's API
  setters on top of the crate's: `host` splits the port, `port` and
  `protocol` read the leading digits or scheme, an empty host is refused
  when the URL has credentials or a port, tab and newline are removed, and
  `search = "?"` removes the query and encodes `'` for every scheme
  (Chromium, measured). `location` uses the same setters on a copy of the
  document's URL. Departures, all in `tests/url.rs` with the reason: a
  space in a host is refused (Chromium percent-encodes it); `file://localhost/a`
  has an empty host (Chromium keeps `localhost`); `protocol` does not
  convert between special and non-special schemes or from an opaque path
  (Chromium does); the empty first segments of a `file:` path go; `chrome:`
  has an opaque origin; no `createObjectURL` and `revokeObjectURL`.
  `URL.origin` of a `file:` URL is `file://`, as Chromium has it.
- **Structured clone** (`web/src/structured.rs`). `Serialized` is a record
  that owns its data: a table of items that refer to one another by index
  (objects, arrays with holes and extra keys, dates, regular expressions,
  maps, sets, buffers, views that share their buffer, errors,
  `DOMException`, wrapper objects) and strings as code units. It outlives the
  heap, so a history entry keeps it, and one record can be read many times.
  Both walks are iterative with an explicit stack, in the order of the
  recursive algorithm, so getters run in Chromium's order. Limits: depth
  4,096 (a `RangeError` with Chromium's words; Chromium's own depends on its
  stack, 4,442 measured), 4,194,304 items, 512 MiB of strings and buffers,
  and 4 MiB for the state of a history entry (a `DataCloneError`; Chromium's
  limit is not measured).
  The objects are read and made with the API of section 6, item 10. Chromium
  behaviour that the probe fixes: the prototype of an object becomes
  `Object.prototype`; only own enumerable string keys, with `Get`
  for accessors; `Date` and `RegExp` lose `lastIndex` and extra properties;
  errors keep `name` (only the standard names, else `Error`), `message`
  (an own data property), `stack` (the text) and `cause` (own data
  property, non-enumerable), in the order `stack`, `message`, `cause`;
  `DataCloneError` words ("`() => 1 could not be cloned.`", "`HTMLBodyElement object
  could not be cloned.`", "`#<Promise> could not be cloned.`",
  "`An ArrayBuffer is detached and could not be cloned.`", "`Value at index 0
  does not have a transferable type.`"); resizable buffers keep their maximum
  and a length-tracking view keeps tracking; `transfer` moves `ArrayBuffer`s
  (the clone has the bytes, the original is detached after a clone that
  succeeded). Departures: platform objects other than `DOMException`
  (`DOMRect`, `Blob`, `ImageData`, ...) throw `DataCloneError` (Chromium
  clones them); an error whose `message` is an accessor makes the text of
  `stack` at the first read of `stack` (swb builds it lazily), so the
  getter runs.
- **`location`** (`web/src/location.rs`) is one object, the value of
  `window.location` and `document.location`, with the members as own
  properties (`[LegacyUnforgeable]`, also `toString`), an immutable
  prototype and `[PutForwards=href]`. A write is a request that the page
  carries out when the task ends (`EngineHost::request_navigation`,
  `request_reload`), except a change of the fragment alone (a URL equal to
  the document's but for the fragment): the document's URL and
  `history.length` change at once, `popstate` fires inside the call and
  `hashchange` as a task (Chromium, measured: `location.hash = 'x'; LOG`
  gives `popstate`, then the rest of the script, microtasks and timers
  that were queued before, then `hashchange`); the page adds or replaces
  the entry, sets `:target`, scrolls and focuses the target when the task
  ends. `location.hash` keeps an empty fragment (`#`), `URL.hash` does not;
  the same hash is no navigation; `replace` adds no entry. An invalid URL
  is a `SyntaxError` ("'x' is not a valid URL."); `protocol` with an
  invalid scheme "is an invalid protocol."; a `javascript:` URL queues its
  script as a task. Departures: the invalid URL is quoted as written
  (Chromium prints its canonical form, `http://[bad/`, `http:`);
  `ancestorOrigins`, the own `valueOf` and the own symbol key of Chromium's
  `Location` do not exist; the text `[object Location]` of messages comes
  from the `toString` rule of section 6, item 10.
- **`history`** (`web/src/location.rs`, `engine/src/document/history_api.rs`,
  `engine/src/page/history_api.rs`). The page keeps the history
  (`history.rs`: up to 50 entries as Chromium, measured through `history.length`;
  each entry has its URL, scroll, state and scroll restoration mode); the
  document keeps a copy for scripts (index, length, state) and a list of
  changes. `pushState` and `replaceState` serialize the state first (a
  `DataCloneError` leaves everything as it was), resolve the URL against the
  base URL (an empty URL is none; Chromium's rule: no credentials, same
  scheme, host and port; a `file:` or opaque document may change only the
  query and fragment; a failure is a `SecurityError` with Chromium's words),
  and change the document's URL, its base URL and `history.state` at once;
  the page applies the changes in order after the task; the entry that a
  push leaves keeps the scroll position of the call. `history.state` is the
  same object until the entry's state changes, and the `state` of
  `popstate` is that object. `back`, `forward` and `go` drop a call whose
  target is out of range at the call and queue the others; the page runs
  each as a task of its own after the tasks that are due (Chromium: the
  timer queued before `back()` runs first, two `back()` calls go two entries
  back, `back(); forward()` does not return), `go(0)` reloads. A traversal
  within the document fires `popstate` and, if the URLs differ only in the
  fragment, `hashchange`; one to another document loads it (the entry's state
  comes with it) and fires no `popstate`. A traversal restores the scroll
  position unless the entry's `scrollRestoration` is `"manual"`; an invalid
  value of `scrollRestoration` is ignored (the generator now does this for
  every enumeration-typed attribute, Web IDL §3.7.6).
- **Which navigations replace, and what they do with the state** (fix round
  after the review of 8a1; `web-location-history.json` has the cases, three
  equal Chromium runs). The history handling of a page's navigation is
  `Push`, `Replace` (a new document takes the entry: `location.replace()`
  to another URL, a navigation before the load ended) or `Reload` (the same
  document again). `Replace` and a push to the URL of the current entry
  clear the entry's state (`History::replace_document`), so a new,
  possibly cross-origin, document never reads the old document's
  `history.state`; a reload and a traversal keep the entry's own state,
  unless the server redirects them to another origin: then the state and
  the scroll restoration mode go too (`History::replace_current`; found
  in the delta review, not measured in Chromium).
  Rules, all measured: (1) a navigation through `location` before the
  document is completely loaded replaces the entry (HTML "Location-object
  navigate"). "Completely loaded" is the end of the `load` event: inline
  scripts, `DOMContentLoaded` and `load` handlers replace, a task after
  `load` pushes (`Scripts::completely_loaded`, `in_load_event`). It holds for
  fragments too (`location.hash = 'x'` in an inline script adds no entry).
  Not measured: transient user activation (a click is not a navigation by
  script here). (2) A navigation to the URL of the document is a
  replacement (HTML "navigate"): with a fragment it is a navigation to a
  fragment that adds no entry, keeps `history.state`, fires `popstate` with
  `state` `null` and no `hashchange`; without a fragment it is a reload
  (`Reload`: the state stays, Chromium measured with `location.href =
  location.href`, `port = port`, `host = host`). Setters that leave the URL
  unchanged follow the same rule (`port = 'abc'` fires `popstate` as
  above); `protocol = 'ftp'` does nothing in Chromium and in swb. (3) `hostname = ''` and `host = ''` on a special
  URL throw a `SyntaxError` in Chromium (`'http://:PORT/path#f' is not a
  valid URL.`); the standard's parser fails and the URL stays. swb throws
  too. Departure not done: Chromium loads a new document for `location.search
  = ''` when the URL has a fragment (it probably writes `?`); swb treats it
  as the same URL.
  The `SecurityError` text of `pushState()` names the origin `null` for a
  `file:` or opaque origin (`location.origin` of a `file:` URL stays
  `file://`).
- **Bound on the traversals** (`MAX_PENDING_TRAVERSALS`, 64). A script
  that calls `back()`, `forward()` and `go()` in a loop made one queue
  entry per call (758 MiB, 32 s for 7 million rounds). Now a task's request
  list takes at most 64 traversals and one `go(0)`, and the page's queue
  holds at most 64; the calls over the bound are dropped. Departure:
  Chromium throttles history calls by rate (not measured further), so a
  loop of 1,000 `back()` calls may go back farther in Chromium than the
  first 64 in swb. The hostile case
  `history-traversal-loop-js-on` has 1,000,000 rounds (1.2 s, 35 MiB).
- **Not done / backlog:** Chromium's `Navigation API`, `history.length` of
  `about:blank` frames, `location.ancestorOrigins`, the cloneable platform
  objects other than `DOMException`, the transfer of other types, and the
  throttling of `pushState` loops (none visible in Chromium: 1,000 calls
  in a loop all took effect).

Feature 8a2 notes (`navigator`, `performance`, `PerformanceObserver`,
`crypto`, `atob`, `btoa`, `TextEncoder`, `TextDecoder`,
`requestIdleCallback`, `document.hidden`; measured with
`tools/probes/web-navigator.json` (35 queries), `web-crypto-encoding.json`
(127), `web-performance.json` (183) and `web-idle.json` (33), three equal
Chromium runs each; the Rust tests are `crates/web/tests/navigator.rs`,
`crypto_encoding.rs`, `performance.rs`, `crates/engine/tests/script_idle.rs`
and unit tests per module, and five hostile cases):

- **Generator and engine additions.** `swb-webidl` generates `static readonly
  attribute` (the getter has no `this`; the accessor is on the interface
  object), the buffer source types (`ArrayBuffer`, `ArrayBufferView`,
  `BufferSource`, `AllowSharedBufferSource`, `Uint8Array`: an `object` with
  a check, `b::convert::buffer`; the message names the type, and the unions
  are `ArrayBuffer` as in Chromium) and `FrozenArray` results as frozen
  arrays (`b::to_js::frozen_sequence`). `js` has three small host calls:
  `Runtime::freeze`, `write_buffer_bytes` and `buffer_range`. The only new
  crate dependency is `getrandom 0.3` (MIT or Apache-2.0, already in the
  tree through other crates; ADR 0003 checked, `cargo deny` passes);
  `encoding_rs` was a workspace dependency.
- **`navigator`** (`navigator.rs`). `userAgent` is the string of swb's
  requests (`EngineHost::user_agent`, ADR 0025) and `appVersion` is that
  string without `Mozilla/`. Values that describe the machine or the browser
  are stable and do not claim to be Chromium: `platform` is the operating
  system and CPU of the build (`Linux x86_64`), `vendor` is empty (Chromium:
  `Google Inc.`), `language` and `languages` are `en-US` and `["en-US"]`
  (`languages` is a new frozen array at each read, as in Chromium),
  `hardwareConcurrency` is 4 (swb has no workers; a constant keeps runs and
  tests equal), `onLine` and `cookieEnabled` are true, `maxTouchPoints` is 0,
  `doNotTrack` null, `pdfViewerEnabled` and `javaEnabled()` false.
  `webdriver` is false: the standard says true when the browser is under
  WebDriver control, and swb's automation API is not WebDriver (Chromium under
  Playwright says true; a page that tests it would treat swb as a robot).
  The constants `appCodeName`, `appName`, `product`, `productSub` and
  `vendorSub` are the standard's. Absent: `userAgentData` (swb does not claim
  to be Chromium), `sendBeacon` (feature 10b), `plugins` and `mimeTypes`
  (backlog: pages read `navigator.plugins.length` for fingerprinting; they
  would throw), `clientInformation`, and the feature-detection members of
  section 13's list.
- **`performance`** (`performance.rs`, `performance_observer.rs`).
  `now()` and `timeOrigin` are the document clock of feature 5b. The buffer
  holds marks and measures in order of making, at most 131,072; the
  navigation entry is the implicit first one; `getEntries*` filter and sort
  by `startTime` (a stable sort; measured: equal times keep the order of
  making, also between marks and measures). A `PerformanceMark` made by the
  constructor is not in the buffer. `detail` is structured-cloned at the
  call and deserialized at the first read; the same object comes back.
  `mark` and `measure` follow User Timing with Chromium's words
  (the `PerformanceTiming` names are not mark names, a negative `startTime`,
  the checks of a non-empty options dictionary, `The mark 'x' does not
  exist.`, a negative time stamp, `InvalidAccessError` for a load step that
  did not happen yet); `measure` takes the names of the `PerformanceTiming`
  attributes as marks. The load milestones (`domLoading`, `domInteractive`,
  `domContentLoadedEventStart/End`, `domComplete`, `loadEventStart/End`) are
  recorded by `document_load.rs` in page time; `fetchStart` and the phases up
  to `responseEnd` are 0 (the loader gives no times for the phases of the
  request), `unloadEvent*`, `redirect*` and `secureConnectionStart` are 0, and
  the steps that did not happen yet are 0 (Chromium: the same). The navigation
  entry's `name` is the document URL when the parse starts: `pushState`,
  `replaceState` and fragment changes do not change it (measured in the
  review; before the fix it was the URL at the first read). It has `type`
  (navigate, reload, back_forward, from the page's handling
  of the navigation: `EngineHost::navigation_info`), `responseStatus` and the
  body size (`transferSize` is the body size: the loader does not keep the
  headers' size); `PerformanceTiming` values are whole milliseconds since the
  epoch.
  `PerformanceObserver` supports `mark`, `measure` and `navigation` (the last
  with `type` and `buffered` only); other types are ignored, as in Chromium
  (which prints a console warning; swb logs at debug level). `entryTypes`
  replaces the types, `type` adds one, the two forms do not mix (an
  `InvalidModificationError`, and `disconnect()` does not reset that: measured),
  `buffered` queues the entries that exist. A new entry is queued at each
  observer that observes its type; the delivery is one task ("queue a
  PerformanceObserver task", `Task::PerformanceObservers`) in the order of
  the first `observe`, with a microtask checkpoint after each callback;
  measured: the callback runs after the microtasks of the calling script and
  before a `setTimeout(f, 0)` that the script set later, with `this` the
  observer and the arguments `(list, observer, { droppedEntriesCount: 0 })`.
  A queue holds 131,072 entries.
- **`crypto`** (`crypto.rs`). `getRandomValues` takes integer typed arrays
  (`DataView` and float arrays: `TypeMismatchError` with the type's name), at
  most 65,536 bytes (`QuotaExceededError` with the length in the message),
  no resizable buffer; a detached buffer gives a view of length 0.
  `randomUUID` makes a version 4 UUID from 16 random bytes. The source is the
  operating system's (`getrandom`). `randomUUID` is not hidden on an insecure
  origin (swb has no secure-context concept; Chromium hides it there). No
  `subtle`.
- **`atob` and `btoa`** (`base64.rs`): the forgiving-base64 decode of the
  Infra Standard and the encode of a binary string; Chromium's words for the
  `InvalidCharacterError`, and a character above U+00FF in the input of `atob`
  has its own message.
- **`TextEncoder` and `TextDecoder`** (`text_codec.rs`). The encoder makes
  UTF-8 (a lone surrogate becomes U+FFFD, as the conversion of `USVString`
  does). The decoder is `encoding_rs`' decoder (the labels of the Encoding
  Standard; `replacement` and its labels, and an unknown one, are a
  `RangeError`; `encoding` is the lower-case name); `stream` carries the state
  to the next call; the BOM is removed once unless `ignoreBOM`; `fatal` throws
  a `TypeError` and the next call starts again. A call takes at most 64 MiB of
  input (a `RangeError`; Chromium decodes 300 MiB, measured): the decoder works
  in chunks of 1 MiB, but the result is one string on the heap.
- **Idle callbacks** (`idle_callbacks.rs`, `page/idle.rs`; section 7, step 4).
  See the architecture document for the engine side. Measured: handles count
  from 1; the callbacks of a period run in the order of their handles after the
  timers and frames that are due; a callback requested inside a period waits
  for the next one; `timeout` is converted as an unsigned long (so `-1` is
  4,294,967,295 ms, `"x"` is 0 and 0 means none) and queues a task
  (`Task::IdleTimeout`) that runs the callback with `didTimeout` true and
  `timeRemaining()` 0; the task is removed when the callback runs or is
  canceled. A loop of `requestIdleCallback` in its callback runs 20 times a
  second. In virtual time `timeRemaining()` charges 0.1 ms of page time at
  each call (never past the deadline), so a loop on it ends and a headless run
  is the same each time. Order against `requestAnimationFrame` in one script:
  Chromium varies between runs (idle first or frame first); swb runs the frame
  first.
- **`document.hidden`** is false and `visibilityState` `"visible"` for the
  window's document; a document without a browsing context (`DOMParser`,
  `createHTMLDocument`) is hidden, as in Chromium (measured). `visibilitychange`
  is never fired by the engine. Backlog: the GUI should tell the page when the
  window is minimized or the tab is hidden (`hidden` true, `visibilitychange`
  at the document, then `pagehide`-like state saving by pages).
- **Departures** (all in the Rust tests with the reason): no `visibility-state`
  entry (Chromium 148 adds one to the timeline) and no `resource` entries;
  `supportedEntryTypes` is `["mark","measure","navigation"]` and a new frozen
  array at each read (Chromium: one array); the navigation entry has 39 keys in
  `toJSON` where Chromium has 47 (no `activationStart`, `notRestoredReasons`,
  `confidence`, `serverTiming` and the four worker router members); no phases of
  the request (0 where Chromium has small positive times); no `unloadEvent*`
  times (Chromium shows the unload of the blank page that precedes a `file:`
  document); no `performance.eventCounts`, `memory`, `interactionCount` and
  `onresourcetimingbufferfull`; `performance.now()` has exact tenths (Chromium's
  values carry floating point noise below 1e-6 ms); `navigator.webdriver`,
  `vendor`, `userAgent`, `hardwareConcurrency` as above; the 64 MiB limit of
  `decode`; no `crypto.subtle`; no `webkitHidden` and `webkitVisibilityState`.
- **Not done / backlog:** `navigator.plugins`, `mimeTypes`, `clientInformation`;
  resource entries and `PerformanceResourceTiming` of subresources; the Event
  Timing, Paint Timing and Long Tasks entry types; `PerformanceObserver` with
  `durationThreshold`; `visibilitychange` from the GUI; `TextDecoderStream` and
  `TextEncoderStream` (no streams); the secure-context rule for `randomUUID`.

Feature 9a notes (`MutationObserver`, `IntersectionObserver`,
`ResizeObserver`; measured with `tools/probes/web-mutation-observer.json`
(145 queries), `web-intersection-observer.json` (121) and
`web-resize-observer.json` (76), three equal Chromium runs each; the Rust
tests are `crates/web/tests/mutation_observer.rs`,
`crates/engine/tests/script_observers.rs` and unit tests in
`mutation_registry.rs` and `intersection_observer.rs`, and six hostile
cases):

- **`MutationObserver`** (`mutation_registry.rs`, `mutation_observer.rs`).
  The registry (registered observers per node, transient registered
  observers, queued records, all Rust data) is the document's mutation sink
  from the first `observe()`; a document whose scripts never observe pays
  nothing (the parse without an observer is within noise of the base). A
  record is made for each interested observer at each primitive change
  (DOM "queue a mutation record"); `MutationRecord` objects and the
  `NodeList`s of `addedNodes` and `removedNodes` are made at delivery or at
  `takeRecords()`. Delivery follows "queue a mutation observer microtask": the
  registry sets a request flag, the glue of every native call and every
  microtask checkpoint check it (`mutation_observer::after_call`) and queue
  one host function as a microtask, so a promise reaction queued before the
  first change runs before the callback and one queued after it runs after
  (measured). The callbacks run in creation order, `this` and the second
  argument the observer, and a record that a callback causes for an observer
  that has not had its turn comes with that observer's records (measured:
  `o2` gets 2 records). An exception is reported and the next observer
  runs. The HTML parser makes a microtask checkpoint before it runs a
  parser-inserted script (HTML, "an end tag whose tag name is script"; the
  step was missing), so an observer sees the nodes that the parser inserted
  since the last script before that script runs (measured). The records of
  the parser equal Chromium's (the concatenated lists of five cases).
  Chromium's order for some DOM operations differs from the standard, and
  `dom` follows Chromium now: a node that is moved inside its parent names
  the sibling that is left after its removal; `normalize` appends the data
  of a text node and removes it, node by node; `splitText` truncates the
  node before it inserts the new one; `textContent` and `innerText` replace
  a single text child with a new node when an observer exists (the fast
  path that keeps the node stays when none does).
  Options and errors are Chromium's (the checks of the old-value and filter
  options come before the "at least one of" check; measured).
  *Lifetime*: the binding state holds an observer strongly from `observe()`
  until `disconnect()`, as the registered observer lists of the (immortal)
  nodes do; an observer that was never observed or was disconnected is
  collected when script drops it. *Bounds*: 131,072 records and 128 MiB of
  record data (old values, names, node lists, 1,024 bytes a record) wait at
  most, and 500,000 transient registered observers exist at most; later
  records and later transient registrations are dropped with one warning
  (section 16 has the measurements). The work of the sink (nodes and
  registrations visited) is counted in the registry, and the glue of the
  native call that caused it checks the time limit when 4,096 units have
  piled up, because native work does not tick the countdown of the VM. The
  transient registered observers of an observer are a set of nodes
  (removing 200,000 children one by one under a subtree observer takes
  0.1 s), and the interested observers of a change are sorted and merged,
  not searched. A callback that changes the tree again is stopped by the
  time limit of the checkpoint (the hostile case ends after two tasks that
  reach the limit, 10 s, not 15 s: measured, the log shows two terminations
  for each such task, so the rule of section 7 stops the scripts at the
  third termination, in the second task). A termination in the middle of "notify mutation
  observers" puts the observers that had no turn back into the notify set,
  so that the next delivery gives them their records. A removed node gets
  one transient registered observer per observer and source, not one per
  removal. *Departures*: `Attr` is not a `Node`, so it cannot be
  observed; the old value of a `style` attribute change is the live
  attribute text (Chromium synchronizes the attribute lazily after CSSOM
  writes and names a stale value); `node.replaceWith(node)` is one record in
  swb and two in Chromium; `style.setProperty('x', '1')` makes a `style`
  record in Chromium, where `x` is the SVG geometry property, and none in
  swb, which has no `x` property (the records of valid properties are
  equal); `document.write` does not exist yet, so no records
  of it; `Range` and `createProcessingInstruction` do not exist.
- **Rendering update** (section 7). Frames are due while observers exist
  and something changed: `Page::observers_due` compares the DOM generation,
  the count of style and layout runs, the scroll offsets of the viewport and
  the containers and the viewport size with what the observers saw at the
  last frame, and an added observation requests a frame. A page that only
  observes and does not change is idle. The frame runs, after the animation
  frame callbacks, `run_resize_observers` and
  `update_intersection_observers`; style and layout run on demand inside them
  (the host calls flush). Measured order: `microtask`, `timeout0`, `raf`,
  `resize observer`, `intersection observer` (the intersection callbacks are
  a task that the update queues, so they run after the frame).
- **`IntersectionObserver`** (`intersection_observer.rs`). For each target
  the update computes the bounding box (`getBoundingClientRect`) and
  intersects it with the padding boxes of the ancestors in the containing
  block chain that clip (`overflow` other than `visible`, also `clip`), then
  with the root; an absolutely positioned target skips ancestors that are
  not positioned or transformed, a fixed one all but transformed ones;
  `html` and `body` do not clip. The implicit root is the viewport (a
  `Document` root too); an element root is its padding box and must be in
  the containing block chain of the target, else all rectangles are 0, also
  `rootBounds` (measured, also for a target without a box). Margins are
  `px` or `%` of the root's size, both cut to whole pixels (a `20%` of a
  114 px root is 22 px; measured); the thresholds are
  sorted, empty means `[0]`. An entry is queued when the threshold index or
  `isIntersecting` changed (the first computation always); a target with
  zero area intersects with ratio 1 if the boxes touch. Entries are
  delivered in the order of creation of the observers (Chromium's order of
  observers is unspecified: it differed between runs, so no probe case
  depends on it). *Departures*: `scrollMargin`, `delay` and
  `trackVisibility` are accepted, validated and reported, but have no effect
  and `isVisible` is false; `clip-path` and masks do not clip; the clips use
  the transformed border box minus the border widths; a target in an SVG
  or a shadow tree is treated like any element.
- **`ResizeObserver`** (`resize_observer.rs`). The loop of the standard:
  "gather active resize observations at depth" and "broadcast" (each
  callback is a task), again below the depth of the shallowest delivered
  target, until none is active; skipped observations fire an `ErrorEvent` at
  the window (message `ResizeObserver loop completed with undelivered
  notifications.`, no error object, the document's URL as `filename`, 0:0;
  measured) and stay active, so the next frame delivers them (a callback that
  resizes its target without end runs once per frame and reports the error
  each time; swb logs it like Chromium). The first gather allows every depth
  (a detached target is delivered at once; measured). A new observation is
  active at once, also for 0x0, `display: none`, detached and inline
  targets (size 0). Observing a target again replaces its observation.
  Sizes are layout units as `getBoundingClientRect` has them (the content
  size, each padding and each border width are units, and the border box is
  their sum: a width of 100.4 px with padding 3.3 px and border 1.7 px
  gives a content box of 100.390625 px and a border box of 108.984375 px);
  the device pixel box is the content size times the
  scale, rounded. *Departures*: no vertical writing modes, so inline size is
  the width; the arrays of an entry are new at each read; `zoom` is not
  undone; a target without a CSS box in SVG reports `getBBox`.
- **Engine additions**: `EngineHost::observed_box` (the layout box of an
  element: border and content sizes, padding origin, border widths, inline,
  `position`, transform, clip), `ScriptRuntime::has_observations`,
  `observations_requested`, `run_resize_observers` and
  `update_intersection_observers`. `js` got `Runtime::new_host_function`
  (a native function that the host queues as a microtask).
  `Exception::member` words the failure of a union member as Chromium does
  (`The provided value is not of type '(Document or Element)'.`).
- **Not done / backlog**: `scrollMargin`, `delay` and `trackVisibility`
  effects; `isVisible`; the IDL `[SameObject]` arrays of `ResizeObserverEntry`;
  `writing-mode`; the observers of elements in frames (M8 has none);
  `contenteditable`, `popover`, `Element.slot` setters (found by the probe
  of attribute records: they write no attribute); `document.write` records;
  `Range`.

Feature 9b notes (forms from scripts; measured with
`tools/probes/web-forms-script.json` (21 cases, 111 queries) and
`web-forms-selection.json` (7 cases, 33 queries), three equal Chromium runs
each; the Rust tests are `crates/automation/tests/forms_script.rs` (both
files, with the input steps, against `tests/data/web-forms-script.json`) and
the unit tests of `crates/engine/src/forms/mod.rs` and `edit.rs`; this is
the first half of the feature, 9b1; the constraint validation API, `labels`
and the `select` and `selectionchange` events are 9b2, with their own notes
below):

- **The dirty flags (ADR 0013, "Scripts").** The gaps that the probe found
  and that are closed: (1) the dirtiness flag of a select was one per
  select; it is now one per option (`OptionState::dirty`; `selectedIndex`,
  `select.value`, `option.selected` and the user's choice set the flag of
  the option they select), so that a `selected` attribute that appears on
  an option nobody touched selects it, and the options keep their
  selectedness over rebuilds (`merge_options`: an option that was in the
  select keeps selectedness and flag, an inserted option takes a seed or
  its attribute, a select that lost `multiple` keeps its first selected
  option, the setting algorithm runs only if the list of options, `multiple`
  or the display size changed, so `selectedIndex = -1` stays over
  unrelated changes); the `clean_applied` marks of 6c are gone. (2) A
  change of `type` lost the state: now the value is sanitized for the new
  type and kept (Chromium: `abc` in a field that became `number` is empty,
  and stays empty as `text`), the dirty checkedness stays for any `input`
  type (`checked` is readable and writable on a text field; seeds hold it
  until the type is a checkbox or radio button again), a script's value in
  a seed is sanitized or dropped (`EngineHost::input_type_changed`), and
  the state of a control of another type than the DOM says is rebuilt on
  the next read or write (`sync_type`: O(1) when the types agree). The
  removal of `type` is a change to `text` and runs the same steps
  (measured: `number` with `abc`, then no `type`, is empty). A `selected`
  attribute that is the only change since the last build updates the state
  of its select in place and keeps the build valid, so a loop over the
  options does not rebuild the forms for each write. (3) A
  radio button that moves to another form owner (or becomes a radio button)
  is the newest of its new group (`Forms::rebuild`, and the owner at the
  time of a seed's write). (4) A reset to the same value keeps the
  selection of a text field; a new default (`value` attribute) keeps the
  previous selection cut to the new text. (5) An indeterminate checkbox is
  not `:checked` (Chromium). (6) `select.selectedIndex` etc. on a select
  outside the document go through seeds with the flag per option. (7)
  `output`: `defaultValue` and `reset()` follow the standard's value mode
  flag (`WebState::output_defaults`).
- **Keyboard on a select.** With scripts on, an arrow, Home or End key that
  changes the selected option of a focused drop-down fires `input` and
  `change` at once (`ScriptRuntime::select_changed`).
- **Selection API.** `selectionStart`, `selectionEnd`, `selectionDirection`,
  `select()`, `setSelectionRange()` and `setRangeText()` (both forms) on
  `input` (types `text`, `search`, `tel`, `url`, `password`) and `textarea`
  (`html/selection.rs`, over `TextEdit`: `select_range_utf16`,
  `selection_utf16`, `replace_range_utf16`, `is_backward`). Measured in
  Chromium: other types give `null` and throw `InvalidStateError` (`The
  input element's type ('email') does not support selection.`) from the
  setters and `setRangeText`, `select()` does not throw; the direction is
  `forward` or `backward` (`none` and invalid values give `forward`, a
  collapsed selection keeps `backward`); `start` above `end` collapses to
  `end`; offsets are clamped; the `selectionStart` setter moves the end up,
  the `selectionEnd` setter moves the start down; `setSelectionRange()`
  needs two arguments; `setRangeText(x)` selects the new text; with four
  arguments `start` above `end` throws `IndexSizeError`, the mode
  `preserve` follows the standard's formula and the direction becomes
  `forward`; `maxlength` does not limit it; a value set when it differs moves
  the caret to the end, the same value keeps the selection; `select()` also
  focuses the element. A control without a state (outside the document, or
  inserted since the last build) keeps its selection in a seed. Departure:
  an offset inside a surrogate pair moves back to its start (the selection
  is at character boundaries). Chromium quirk not copied: `setRangeText`
  with a range on an input that has not been laid out replaces the whole
  value. The calls scan the value, so they call `Runtime::check_time`;
  hostile cases: `selection-huge-value-js-on`,
  `set-range-text-growing-value-js-on`, `set-range-text-over-budget-js-on`.
- **Measured and not done (9b2 and the backlog).** (1) The constraint
  validation API (`willValidate`, `validity`, `validationMessage`,
  `checkValidity()`, `reportValidity()`, `setCustomValidity()`) and
  `labels` (done in 9b2). (2) The events `select` (after `select()`, `setSelectionRange()`
  to a range, the `selectionStart` setter, and the user's selection) and
  `selectionchange` (at the control, bubbling to the document, one task after
  any change of the selection, also when `focus()` moves the selection into
  a control; for a script call `selectionchange` comes first, for the user's
  selection `select` comes first; done in 9b2, with a correction: `select`
  bubbles). (3) `[LegacyOverrideBuiltIns]` on
  `HTMLFormElement`: a control named `submit`, `action`, `reset`, `elements`
  or `length` hides the member in Chromium; swb keeps the members. Measured
  with the attribute on: every property read of the form (also
  `appendChild`) builds the list of its controls, and 20,000 `appendChild`
  calls on a growing form hit the time limit of a task; a form with 5,000
  controls took 1.4 s for 1,000 pairs of DOM change and `form.method` read
  (0.1 s without). Left as a departure (probe `form-methods` 7 and 8).
  (4) The `formdata` event of `submit()`. (5) The up and down arrow keys step
  a `number` input in Chromium. (6) Chromium fires `beforeinput` for a typed
  character in a read-only input (no `input`). (7) `type=color` accepts CSS
  color names and functions (`red` is `#ff0000`). (8) Offsets in surrogate
  pairs (above).

Feature 9b2 notes (constraint validation, `labels`, the events of the
selection; measured with `tools/probes/web-forms-validation.json` (30
cases, 121 queries) and the added cases of `web-forms-selection.json` (22
cases, 51 queries), three equal Chromium runs each; the Rust tests are
`crates/automation/tests/forms_script.rs` (now with the validation file and
the queries that wait) and the unit tests of `forms/validity.rs`,
`forms/values.rs` and `forms/mod.rs`):

- **The computation (`forms/validity.rs`).** One function gives the ten
  states of a control from its state and attributes; style (`:valid`,
  `:invalid`, `:user-valid`, `:user-invalid`), the script API
  (`ValidityState`, `checkValidity()`, `reportValidity()`) and the
  blocking of a submission (with scripts off too) call it. A control
  that is barred from validation (disabled, also through a fieldset;
  read-only, any `input` or `textarea` (measured: also a checkbox);
  `hidden`; `image`; a button that does not submit; in a `datalist`) has
  `willValidate` false, no event and `validationMessage` empty, but its
  states are computed and readable (a read-only checkbox has
  `valueMissing`). `required`: a text field or text area needs a mutable
  control (a read-only or disabled one is not missing), a checkbox and a
  select do not; a radio button without a name is in no group and is
  never missing (measured; the old code said it was); a `select` with
  `multiple` or a display size above 1 has no placeholder label option.
  `minlength`/`maxlength` give `tooShort`/`tooLong` only if the user
  edited the value last (`ControlState::user_edited`; a script's `value`,
  `setRangeText()` and a reset clear it; counted in UTF-16 units).
  `typeMismatch`: `email` (with `multiple` each comma-separated address)
  and `url` (the `url` crate; departure: Chromium accepts a space and a
  `%20` in the host, swb refuses). `pattern`: applies to the types that
  have `maxlength` (not a text area, not `number`); with `multiple` on an
  e-mail field, to each address (measured). `min`/`max`/`step`: `number`
  and the five date and time types, in the unit of `step` (days, months,
  weeks, seconds); the step base is `min`, else the `value` attribute,
  else 0; a whole number of steps is tested with a tolerance (Chromium
  computes in decimals); `range` and `color` are always valid. A `time`
  with `min` above `max` wraps around midnight (the value outside both
  limits has both states). `badInput`: the text of a `number` field that
  is no number (`1e`, `-`), measured: letters cannot be typed, and `1.`
  has the value `1`; the date and time types are text fields in swb, so a
  text that does not parse is bad input (Chromium: an incomplete field;
  departure: a date typed as `12312020` is valid there).
- **The texts.** Chromium's English texts, measured for each state and
  type: the missing-value texts by control type; the e-mail texts
  (`Please include an '@' in the email address. 'abc' is missing an
  '@'.`, the part before and after the `@`, the symbol that is wrong,
  `'.' is used at a wrong position in 'x'`, the non-empty address of
  `multiple`); `Please enter a URL.`; `Please match the requested format.`
  (the `title` is not part of `validationMessage`); the length texts with
  the singular for the current count only (`using 1 character`; the limit
  is always `characters`); `Value must be greater than or equal to
  5.` for numbers and `Value must be 01/01/2021 or later.` for dates, the
  forms for equal limits (`Value must be 5.`) and reversed limits
  (`Minimum value (5) must be less than the maximum value (1).`); the
  step text with the one or two nearest valid values inside the range;
  the dates as `MM/DD/YYYY`, `January 2021`, `Week 05, 2021`, times as
  `9:00 AM` with U+202F before the marker (seconds when they or the step
  need them), the number of an attribute as Chromium's decimal prints it
  (`1e3` is `1e+3`, `1.50` is `1.5`, 15 significant digits for a
  fraction). The order of the states in the message: custom, bad input,
  missing, type, pattern, length, range, step. A custom message wins, also
  over a missing value. One Chromium quirk is kept: a `time` with a
  reversed range whose value is valid still has the message of the limit
  that it is past. `setCustomValidity()` normalizes line breaks to LF
  (measured).
- **`ValidityState`** is a lazy interface (`[SameObject]`, host data the
  node index): the states are read from the engine at each access. The
  constraint members are on `input`, `select`, `textarea`, `button`,
  `fieldset`, `output` and `object` (the last two are never candidates;
  they keep a custom message), `checkValidity()` and `reportValidity()`
  on `form`; `meter` and `progress` have `labels` only. A control that is
  not in the document validates from its attributes and the seeds
  (`forms/mod.rs`, `detached_state`); a detached radio button is its own
  group.
- **The events.** `invalid` is a `Event` at the control that does not
  bubble and can be canceled. `checkValidity()` fires it for a control
  that fails; `form.checkValidity()` for each control that fails when its
  turn comes (a listener that fixes a later control spares it; measured),
  and returns false if any failed at the start; `fieldset.checkValidity()`
  is true (it is barred). `reportValidity()` of a control, `form.
  reportValidity()`, `requestSubmit()`, a click on a submit button and
  the Enter key fire the events at all failing controls first, then focus
  (with focus-visible and a scroll into view) the first control whose event
  nobody canceled; with all canceled nothing gets the focus (the 6d1
  code focused the first control unless the first was canceled).
  `novalidate` and `formnovalidate` do not affect the methods. swb shows
  no bubble with the message (departure: Chromium's native bubble). The
  controls of a form count as interacted with for `:user-valid` and
  `:user-invalid` after a failed submission or `reportValidity()`, and
  a control after the focus left it after a user's edit (measured for
  text fields; not measured: whether a checkbox or select change counts at
  once).
- **Style.** `:valid` and `:invalid` of a `form` and a `fieldset` follow
  the candidate controls inside (a fieldset), or owned by the form; a
  disabled fieldset, an empty form are `:valid`. `div` and `output` have
  neither. `CONTROL_STATES` includes `:user-valid` and `:user-invalid`.
  The pass over the controls is linear: the radio groups are computed
  once, the fieldsets above an invalid control are walked once (each node
  is visited once); `begin_pass` gives the `pattern` matches 2,000,000
  steps.
- **Bounds (section 16).** The `pattern` matches of one pass (a style
  update, a script call) share 2,000,000 steps of `js-regexp`'s `Matcher`
  (each match is charged the steps it used); one match has at most
  200,000 steps and 1 MiB of backtracking memory. A pass compiles at most
  128 KiB of pattern text (64 bytes more per compile); a pattern that the
  pass may not compile any more counts as matching for that pass and is
  not remembered (a compile can take about 3 µs per byte: 300 different
  patterns of 44 KB took 28 s in one style update before this bound;
  delta review of 9b2). A match that runs out counts as matching
  (valid), as an invalid pattern does (Chromium has no limit: `(a+)+$` on
  40 letters gives `patternMismatch` there; departure, not in the probe).
  Results are remembered by pattern and value (4,096 entries, values up
  to 1,024 bytes, 4 MiB of pattern and value text counted per result),
  so a repeated style update costs nothing; a match that ran out of its
  own 200,000 steps is remembered as
  valid. When the pass has no steps left, the later matches of the pass
  count as valid and are not remembered: a later pass (the next script
  call, the next rebuild of the control states) tries them again. Style
  visits the controls in the order of their node ids, so the same
  controls get the steps in every run (on a page with more than ten
  catastrophic patterns, a later control can show `:valid` while its
  `validity` says otherwise, until the states are rebuilt). The compiled
  patterns are kept by text: 256 of them and 8 MiB (text and program),
  then the cache starts again (a script that writes a new 10 KB pattern
  and reads `validity` in a loop grew to 1.6 GB before this bound; review
  of 9b2). The custom messages of a document count against
  8 MiB; past it the task ends (termination, warning). Every script call
  calls `Runtime::check_time`. Hostile cases:
  `set-custom-validity-over-budget-js-on`,
  `pattern-catastrophic-many-inputs`,
  `pattern-catastrophic-check-validity-js-on`,
  `check-validity-loop-js-on`, `validity-rebuild-loop-js-on`,
  `pattern-cache-loop-js-on`, `pattern-compile-many-inputs`.
- **`labels`** is the live `NodeList` (`[SameObject]`) of the `label`
  elements of the tree whose labeled control the element is, in tree
  order (`ElementList::Labels`, built in one pass per DOM generation);
  `null` for `input type=hidden`. A `label` with `for` finds its control by
  `id` in its own tree; a control and a `label` with `for` in a tree
  without a document (a `div` that was never inserted) have none, as in
  Chromium (probe row).
- **`select` and `selectionchange` (`html/selection_events.rs`).** Both
  are plain `Event`s at the control that bubble (measured: `select` bubbles
  too). A script's change of the selection (`select()`,
  `setSelectionRange()`, the setters) queues `selectionchange`, then
  `select`, if the selection (start, end, direction) changed; the same call
  again queues nothing. Two calls in a task give one `selectionchange` and
  two `select`. `setRangeText()` always queues `selectionchange`, and
  `select` if the selection moved (and its `preserve` mode puts the end of
  a selection inside the replaced range at the end of the new text,
  measured; 9b1 had the start); a write of `value` queues
  `selectionchange` if the caret moved, `select` never. The selection
  setters collapse the direction to forward when they collapse the
  selection (measured). A control that is not in the document gets none;
  one that leaves it before the task gets none. A text control that gets the
  focus while another one (or none) owns the document's selection gets a
  `selectionchange`; `blur()` keeps the owner, so a refocus right after
  queues nothing. The user's selection fires `select` at once, if the new
  selection is not empty (Shift and arrow keys, Ctrl+A also when nothing
  changes, a double or triple click, a drag), and queues
  `selectionchange`; typing, deleting and moving the caret queue
  `selectionchange` only. The queue holds 4,096 events. Departures: for
  `select()` on an unfocused control, Chromium's order of the two events
  varied (with another control focused it was `select`, `selectionchange`
  once; and for `focus()` then `setSelectionRange()` in one task, and for a
  control that was set before it was rendered); swb always puts
  `selectionchange` first for a script. Chromium merges the
  `selectionchange` events of user steps that come faster than its task;
  the probe steps wait 100 ms. The `type` step of the probe tools now types
  one character at a time with the due tasks between, as Playwright does.
  A change of `type` to `number` fires `selectionchange` with null
  selection in Chromium; swb fires none.
- **Not done (backlog).** `valueAsNumber`, `valueAsDate`, `stepUp()`,
  `stepDown()`; `:in-range` and `:out-of-range`; the ArrowUp/ArrowDown
  steps of a number field; whether a checkbox or select change counts as an
  interaction for `:user-valid`; the bubble of `reportValidity()`.

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
| Values that scripts write to text fields and text areas (also with `setRangeText`, feature 9b), in the states and in the seeds (feature 6c) | 32 MiB | the task ends (termination, warning) |
| Custom validity messages (`setCustomValidity()`) of a document (feature 9b2) | 8 MiB | the task ends (termination, warning) |
| Steps of the `pattern` matches in one pass (a style update, a script call) and of one match, memory of one match, pattern text compiled in one pass, results remembered, compiled patterns kept (feature 9b2) | 2,000,000 and 200,000 steps; 1 MiB; 128 KiB; 4,096 results and 4 MiB of pattern and value text; 256 patterns and 8 MiB | a match that runs out, or a pattern not compiled in this pass, counts as matching (valid); a full cache starts again |
| `select` and `selectionchange` events that wait for their task (feature 9b2) | 4,096 | further events are dropped |
| Event listeners on the nodes and the window of a document (feature 5a) | 262,144 | the task ends (termination) |
| Active timers and their extra arguments per document (feature 5b) | 1,048,576 | the task ends (termination) |
| String handlers of the active timers of a document (feature 5b) | 32 MiB | the task ends (termination) |
| Long tasks (50 ms or more) of a document, their total real time (feature 5b) | 30 s | the document runs no more scripts |
| Nested event dispatch, nested `document.write`, recursion through DOM callbacks | the shared recursion budget (ADR 0026 section 9), charged per re-entry | `RangeError` |
| Script fetches in flight (feature 5c) | 64 | queued |
| Fetched script text that has not run, per document (feature 5c) | 256 MiB | the script fails as a network error (`error` event, warning) |
| `fetch()` requests that run per document, and per host (feature 10a, review fix) | 8 (their own pool of workers) and 6 | queued; the page's loads do not wait for them |
| `fetch()` requests that wait per document (feature 10a) | 16,384 | the call fails at once (`TypeError: Failed to fetch`, warning) |
| Memory outside the heap of the API path per document, `ByteBudget` (feature 10a, review fix): the requests (URLs, headers, bodies) that wait, run or are in a worker that was cancelled, and the response bodies that are read but not yet in the heap (also while the completion waits in the loader's channel or for the page, and while a coded body is decoded) | 256 MiB (own row from the heap; the worker checks it as the body grows) | a request that does not fit fails at once, a response that grows past it fails while it is read: `TypeError: Failed to fetch` and a warning. The bytes return when the body is in the heap or dropped (abort, navigation, stop); a cancelled request that still runs in a thread holds its bytes until the thread ends. The budget is per document: the thread of an abandoned document counts against the budget of that document, and at most 16 such threads exist (`Loader::cancel`), each at most 64 MiB |
| Response bodies that `Response` objects hold, request bodies, header lists in the heap (feature 10a) | the heap (256 MiB), by `heap_size` of the objects | termination, as for any heap growth. A body is in the heap and counted in the budget at the same time only while `Response` is made from the completion |
| Headers in one `Headers` list (feature 10a) | 65,536 | `TypeError` |
| Preflight cache entries, per loader (feature 10a) | 256; `Access-Control-Max-Age` cut to 7,200 s | the soonest to expire goes |
| Dependent signals of one source (`AbortSignal.any()`) and listener registrations of one signal (`signal` option) (feature 10a) | 65,536 each | further ones are not tracked, with a warning |
| `AbortSignal.timeout()` signals that wait for their timer (feature 10a) | 262,144 | the task ends (termination, warning) |
| Redirects of one request (feature 10a, as for any load) | 20 | network error |
| `XMLHttpRequest` calls (feature 10b) | the places and the queue of `fetch()` (64 run, 16,384 wait) and the budget of 256 MiB for the request and the response; 256 requests of Blob URLs that wait | the call ends with `error` events; a response is charged to the heap when it arrives (`heap_size`, `Heap::generic_resized`) until the object is collected |
| Request headers of one XHR (feature 10b) | 256 headers and 64 KiB of names and values | the header is dropped, one warning per request |
| Length of a blob and of a multipart body (feature 10b) | 64 MiB each | `RangeError` |
| Pieces of a blob before it is flattened (feature 10b) | 1,024 plus one per KiB of the last flat copy | the pieces are copied into one vector (at most 64 MiB) |
| Bytes of the live blobs (feature 10b) | the heap (256 MiB, by `heap_size`: each blob charges the vectors it holds, a vector shared by several blobs counts in each; small slices of large vectors are copies) | termination, as for any heap growth |
| Blob URLs of a document (feature 10b) | 16,384 URLs and 256 MiB of bytes that the entries keep alive (the same count) | `QuotaExceededError`, warning |
| Bytes that waiting requests of Blob URLs and `FileReader` reads keep alive (feature 10b; the page may have dropped the blob) | 256 MiB per document, each vector counted once (`HeldBlobs`) | the request is a network error, the read ends with `error`; one warning per document |
| `FormData` entries (feature 10b) | 1,048,576 per object; the heap counts the strings | `RangeError` |
| Queued beacon bytes (feature 10b) | 65,536 (Chromium's quota) | `sendBeacon` returns `false` |
| Images watched for `load` and `error` per document, and events handed over per delivery (feature 10b) | 65,536 (finished watches of images outside the document make room) and 4,096 | further images get no events, warning; the rest of the queue waits for the next pump |
| `decode()` promises that wait (feature 10b) | 65,536 images, 1,024 per image | `EncodingError` |
| `localStorage` and `sessionStorage`, each per origin (feature 8b): UTF-16 code units of all keys and values | 5,242,880 (Chromium's quota, measured) | `QuotaExceededError`; the area stays as it was |
| Keys in one storage area (feature 8b) | 100,000 | `QuotaExceededError` (Chromium has none: its quota in code units allows millions of one-unit keys, which would cost about 100 bytes of memory each) |
| All storage areas of both kinds and all origins together, in bytes: UTF-8 bytes of keys and values, 64 per key, the origin and 64 per area (feature 8b) | 64 MiB | `QuotaExceededError` (a session that visits many origins holds at most 64 MiB of storage: twelve full areas of ASCII text, four of CJK text) |
| Mutation observer records waiting (feature 9a), over all observers | 131,072 | further records dropped with one warning. The delivery of a full queue makes the JavaScript objects of each record: a callback that reads the type, the target and the added nodes of every record is delivered 150,000 records and reaches the 256 MiB heap limit at 200,000 (about 1.4 KB a record), so the bound leaves room in the heap; Chromium has no bound (it delivers 450,000) |
| Bytes of the waiting mutation records: old values, names, node lists, 1,024 per record (feature 9a) | 128 MiB | further records dropped with one warning (a loop that sets a long attribute with `attributeOldValue` copies each old value); with the fixed cost, the bytes and the count bound are equal for records without strings |
| Transient registered observers of mutation observers that exist at once (feature 9a) | 500,000 | further ones are not made, with one warning (records of removals under them are lost); removing 200,000 children one by one under one subtree observer makes 200,000 and takes 0.1 s |
| Work of the mutation sink: nodes and registrations visited (feature 9a) | 4,096 units, then a check of the time limit | the task ends at its deadline although the work is native (3,000 subtree observers and 3,000 removals took 13.3 s before the bound and the check; now 0.4 s) |
| Targets of all intersection observers, and of all resize observers, of a document (feature 9a) | 1,048,576 each | the task ends (termination, warning) |
| Entries that wait in the queue of one intersection observer (feature 9a) | 1,048,576 | later entries are dropped with a warning |
| Structured clone (feature 8a1): nesting depth, items, bytes of strings and buffers | 4,096, 4,194,304, 512 MiB | `RangeError` (depth) or `RangeError` "The value is too large to clone." |
| State of one history entry (feature 8a1) | 4 MiB (the history keeps 50 entries) | `DataCloneError` |
| Traversals of the session history that wait (`back()`, `forward()`, `go()`; feature 8a1) | 64 per task and 64 in the page's queue; one `go(0)` per task | later calls are dropped |
| Marks and measures in the performance entry buffer (feature 8a2) | 131,072 | later entries are made and returned but not kept; one warning |
| Entries that wait in the queue of one `PerformanceObserver` (feature 8a2) | 131,072 | later entries are not queued for that observer |
| Idle callbacks that wait (feature 8a2) | 1,048,576 | the task ends (termination, warning) |
| Input bytes of one `TextDecoder.decode` call (feature 8a2) | 64 MiB | `RangeError` |

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
