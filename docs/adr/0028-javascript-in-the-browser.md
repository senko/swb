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
  safe Rust.
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
  box costs a pointer copy.
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
  lazily when the generation differs.
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

### 6. Changes to `js`

The embedding API grows by what the bindings need (ADR 0026 section 12
lists the design; these are the parts to build):

1. **Host classes:** a registry per runtime (number, parent, name),
   prototypes and interface objects per realm, the brand check of
   section 4, and objects created with a class and host data.
2. **Host hooks for internal methods:** optional functions per class
   for `[[GetOwnProperty]]`, `[[DefineOwnProperty]]`, `[[Get]]`,
   `[[Set]]`, `[[Delete]]` and `[[OwnPropertyKeys]]`. A hook may run
   script (a named setter converts its value with `ToString`); such a
   re-entry charges the recursion budget as other native re-entries do.
   Inline caches already treat host objects as uncacheable.
3. **A host-defined global object** (InitializeHostDefinedRealm with a
   global object and `this` value from the host). Host objects are not
   cacheable today (`object/cache.rs`), and every global read
   (`document`, `Math`) goes through the global object, so the global
   must stay an ordinary, cacheable object that carries the `Window`
   class and host data; only `WindowProperties` (named access to
   elements by id) in its prototype chain is exotic. Feature 4a
   measures global reads before and after.
4. **Host data that the collector traces:** the runtime's host data
   gets a trace method, and the collector treats it as a root; the
   binding state (wrapper vector, listener table, timers) is traced
   through it, without one persistent root per value.
5. **Termination from a native function:** a native can end the running
   task with a termination reason (the DOM limits of section 16).
6. **Event handler functions:** compile a function from source text and
   parameter names with a scope chain of object environments (HTML
   "getting the current value of the event handler": the document, the
   form owner, the element), with the early errors of a function body.
7. **Module scripts without imports:** a Module goal script with no
   `import`, `export`, `import()` or `import.meta` runs as strict code
   with `this` undefined and its top-level declarations local to the
   script (M7 feature 14 note). A module with imports is reported as
   unsupported and does not run. This item comes with the first target
   that has such a script; until then module scripts do not run.
8. **The heap accounting of source text** charges its real size
   (`SourceText::byte_size`), together with the per-document heap limit
   (backlog item of M7 13e).

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

### 9. Style and layout after changes; layout queries

**Invalidation.** The engine drains the journal after each task and
parser step and before each flush. A change of the tree, an attribute
or a text marks the style as dirty; a change of a `style` or `link`
element, an arriving stylesheet or a CSSOM rule change marks the
stylist as dirty. The stylist keeps each stylesheet parsed, so a
rebuild makes only the rule index again (today `invalidate_style`
parses all sheets again). New elements start their loads at insertion.

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
   layout runs per load, and the parsed-stylesheet cache.
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
  automation API.
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
| DOM bytes: text, names and attributes | 256 MiB | the task ends |
| Nested event dispatch, nested `document.write`, recursion through DOM callbacks | the shared recursion budget (ADR 0026 section 9), charged per re-entry | `RangeError` |
| Script fetches in flight | 64 | queued |
| `localStorage` per origin | 5 MiB | `QuotaExceededError` |
| Mutation observer records waiting | 1,000,000 | further records dropped with a warning |

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
