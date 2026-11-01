# ADR 0031: Frames

- Status: accepted (the owner approved the decisions on 2026-11-01:
  no joint session history, the page-wide limits of section 11, storage
  keyed by top-level site and origin, no top-level navigation from
  cross-origin frames)
- Date: 2026-11-01
- Replaces: ADR 0028 section 14 for frames (media playback stays there),
  and the statement of ADR 0028 section 4 that `window`, `self`,
  `globalThis` and `frames` return the global object itself.

## Context

The owner decided on 2026-11-01 (ground rules, roadmap "M9"): frames are
in M9. `iframe` documents render and run their own scripts, same-origin
and cross-origin, and frames exchange messages with `postMessage`. Media
playback stays out. A second rule of the same day: a feature that works
without JavaScript in other browsers also works in swb with JavaScript
off once swb implements it; JavaScript support itself must not change
JS-off behaviour.

What the targets have (docs/js-survey.md; the diagnosis of M9 step 1
measures the details):

- BBC: five frames that scripts create. The Sourcepoint consent dialog is
  a cross-origin frame (`cdn.privacy-mgmt.com`) that covers the first
  viewport and talks to the page with `postMessage`; the Optimizely
  storage frame (`a4621041136.cdn.optimizely.com`, cross-site) answers
  `postMessage` requests with `e.source.postMessage` and stores data in
  its `localStorage` and in `SameSite=None; Secure` cookies; two
  `about:blank` frames. Sourcepoint's wrapper script contains core-js's
  `NullProtoObjectViaIFrame`: it appends a hidden `iframe`, sets
  `src = "javascript:"`, calls `contentWindow.document.open()`,
  `write('<script>document.F=Object</script>')` and `close()`, and reads
  `document.F` (the frame's `Object`). BBC also uses `MessageChannel`
  (React's scheduler, 52 `MessagePort.postMessage` calls).
- Ars Technica: one frame that a script creates, without `src`. Ad slots
  use frames on the live site (empty in replay).
- Craigslist posting: a hidden same-origin frame
  (`www.craigslist.org/static/www/localStorage-....html`) whose script
  checks `document.referrer`, reads its `localStorage`, posts it to
  `parent` with the referrer's origin as target origin, and applies
  `set`, `remove`, `update` and `clear` messages from the page.

swb today (42a932c): a `Page` is one browsing context with one boxed
`DocumentState` (which owns its DOM arena) and, with JavaScript on, one
`ScriptRuntime` with one realm (ADR 0028 section 2). An `iframe` is an
empty box; `contentWindow` and `contentDocument` are `null`; `frames`,
`parent` and `top` are the window itself; the global object is also the
`this` value and the value of `window`. The `js` crate keeps a vector of
realms and creates realms internally, but its public API uses one realm.

Measurements: `tools/probes/web-frames.json`, 21 cases, Chromium 148,
three equal runs (the cases run from the probe's local servers; a second
port gives a cross-origin, same-site origin, and `localhost` under the
page's `127.0.0.1` gives a cross-site one). `just probe
tools/probes/web-frames.json` prints the values; the numbers below come
from it.

Specifications: HTML "Navigables" and "Document sequences"
(<https://html.spec.whatwg.org/multipage/document-sequences.html>), the
`iframe` element
(<https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>),
"Cross-origin objects"
(<https://html.spec.whatwg.org/multipage/nav-history-apis.html#cross-origin-objects>),
the `WindowProxy` exotic object
(<https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-windowproxy-exotic-object>),
agents and agent clusters
(<https://html.spec.whatwg.org/multipage/webappapis.html#integration-with-the-javascript-agent-cluster-formalism>),
`postMessage` and channel messaging
(<https://html.spec.whatwg.org/multipage/web-messaging.html>), the
rendering of embedded content
(<https://html.spec.whatwg.org/multipage/rendering.html#embedded-content-rendering-rules>),
CSS Color Adjustment 1 §2.3 (the color scheme of embedded documents),
RFC 6265bis (site for cookies), Fetch (destination `iframe`) and the
Referrer Policy specification.

## Decision

### 1. Scope

In scope: `iframe` with `src`, `srcdoc` and without either; nested frames;
same-origin, same-site and cross-site frames; `sandbox` with
`allow-scripts` and `allow-same-origin`; `WindowProxy`, `contentWindow`,
`contentDocument`, `frames`, `length`, `parent`, `top`, `frameElement`,
indexed and named access to child frames; cross-origin `Window` and
`Location` objects; `window.postMessage`, `MessageEvent`, `MessageChannel`
and `MessagePort`; `document.open`, `write` and `close` on a frame
document from its parent; frame rendering, input and focus; frames with
JavaScript off.

Out of scope, with reasons in section 13: `frame` and `frameset`,
documents in `object` and `embed`, `window.open` and auxiliary browsing
contexts, joint session history beyond the top level, `BroadcastChannel`,
workers, CSP, `X-Frame-Options`, the `allow` attribute, lazy loading of
frames, process isolation.

### 2. The frame tree

HTML models a page as a top-level traversable with child navigables:
each `iframe` that is connected in a document with a browsing context has
a content navigable, which has an active document. swb maps this to:

- **`Frames`** (`engine/src/frames/`): an arena of `Frame` records in the
  `Page`, addressed by `FrameId` (a `u32` index with a generation, so a
  stale id is an error, never another frame). `FrameId::TOP` is the
  top-level traversable; its record holds the document that the page
  holds now. A `Frame` has: its parent and its container (the parent's
  `FrameId` and the `NodeId` of the `iframe`), its children in tree order
  (the order of the containers in the parent's document), its name (the
  `name` attribute, then `window.name`), its sandbox flags, its current
  `DocumentState` (boxed, as now), its pending navigation, its viewport
  (the content box of the container), and the key of the script agent of
  its document (section 5).
- **Document state.** Each frame document is a `DocumentState` that owns
  its own DOM arena, as the top document does now. Style, layout, the
  display list, queries, forms, images and scroll offsets stay per
  document and keep `NodeId::DOCUMENT` as the root. Reason: the rendering
  and query code of the engine stays unchanged; the alternative, one arena
  for all same-origin documents (ADR 0028 section 3 allows several
  documents in one arena), would make every `DocumentState` method
  address a document node in a shared arena and route the journal to
  several documents. The cost moves to the bindings (section 5:
  cross-document nodes).
- **The page** keeps what is per tab: the loader, the font context, the
  history, the clock, the storage store, the pointer and the press. The
  methods of `Page` that act on "the document" act on the top frame, or
  on the frame that an argument names (input, automation).

**Life of a frame** (HTML "the `iframe` element", "process the iframe
attributes", "destroy a child navigable"):

1. Insertion of an `iframe` into a document that has a browsing context
   (the post-connection steps) creates the child frame with an initial
   `about:blank` document at once: `contentWindow` is not `null` right
   after `appendChild` (measured). Its URL is `about:blank`, its origin
   is the parent's, `readyState` is `complete`, its markup is
   `<html><head></head><body></body></html>`, quirks mode, UTF-8,
   `text/html` (measured).
2. Then the attributes: `srcdoc` navigates to `about:srcdoc` (a
   no-quirks document parsed from the attribute, origin of the parent);
   else a `src` that parses navigates; else (or `about:blank`) the `load`
   event fires at the `iframe` at once, inside the insertion (measured:
   before `appendChild` returns).
3. A change of `src` or `srcdoc` navigates the frame. A navigation by a
   script of the frame (`location`), by its parent (`w.location.href =`,
   also cross-origin, measured), by a link or form with `target` equal to
   the frame's name, or by the frame's own `meta refresh` is a navigation
   of that frame. The first navigation away from the initial
   `about:blank` replaces it.
4. A navigation commits when its document arrives: the old document
   unloads (`pagehide`, `unload`), the new one replaces it in the
   `Frame`, and when it finishes loading, the `iframe load event steps`
   fire `load` at the container as a task of the parent's document.
5. Removal of the `iframe` (or of an ancestor, or the unload of the
   parent document) destroys the frame and its descendants: their
   requests are cancelled, their documents unload, their realms retire
   (section 5). `contentWindow` and `contentDocument` are `null` at once
   (measured).
6. A `display: none` frame loads and runs as any other (Chromium loads
   the GTM frames of the Ars Technica fixture; the capture recorded
   them); it has no box.

**Recursion.** HTML's shared attribute processing steps return without a
navigation when an inclusive ancestor's document has the same URL
(fragments excluded). Chromium 148 is more lenient (case `self-embed`): a
page that embeds its own URL loads once more, and the second level stays
`about:blank`. swb does what Chromium does: the navigation is refused
when two or more ancestors' documents have the URL.

**`javascript:` URLs** in `src` run as a task in the frame's realm; a
string result does not replace the document (as for links, ADR 0028
section 11). `src = "javascript:"` (empty) does nothing visible
(measured, core-js).

**`document.open`, `write` and `close`** on a frame's document from the
parent (same origin) follow the document write design of ADR 0028 section
8; Chromium (case `about-blank`): `open()` empties the document and sets
its URL to the URL of the calling document, `readyState` is `loading`; an
inline script in the written text runs inside `write()`, in the frame's
realm; `close()` ends the parse and fires the frame's `load` and the
`iframe`'s `load` inside the call. This needs the `document.write` work
of M9 (Ars Technica); the frame part is a feature of its own (section 14).

**Load event and "loaded".** A frame document delays the `load` event of
its parent document until it has loaded (measured order: parent
`DOMContentLoaded`, frame scripts, frame `load`, `iframe` `load`, parent
`load`). With JavaScript off, `Page::is_fully_loaded` and
`page.waitForLoad` wait for the frame documents too. Frames that scripts
create after the `load` event do not count, as for other requests.

**Session history.** Chromium keeps a joint session history: a frame
navigation after the first adds an entry, `location.replace` does not,
and `history.back()` in the parent traverses the frame back (case
`windowproxy`). M9 does not build it: every frame navigation replaces the
frame's current entry, `history.length` counts the entries of the top
level, and `back`, `forward` and `go` traverse the top level only. Reason:
no target navigates a frame after its first document; the joint history
needs a tree of entries per navigable and traversals that move several
navigables at once. It is a backlog item with the measured behaviour.

### 3. Network, loading and the loader

- **Routing of completions.** The page has one `Loader` (its worker
  threads are expensive). Each document gets a *client* number from the
  loader; a request carries it, and a completion goes to the document of
  its client. `Loader::cancel_client` cancels the requests of one
  document (removal or navigation of a frame); a top-level navigation
  still cancels all.
- **The request of a frame document:** destination `iframe` (a new
  `Destination::IFrame`; the comment on `Destination::Document` already
  asks for it), mode `navigate`, credentials `include`, initiator the
  origin of the parent document (or of the document that navigated the
  frame).
- **Cookies** (RFC 6265bis §5.2, "site for cookies"): a request of a frame
  document, and the subresources and `document.cookie` of a frame, are
  same-site only if the frame's origin and all its ancestors' origins are
  same-site with the top-level origin. Measured (case `cookies-frames`):
  in a cross-site frame only `SameSite=None; Secure` cookies are set (from
  the response and from `document.cookie`) and sent; a same-site frame
  sets and sends all. Third-party cookies stay allowed (ADR 0012, as in
  Chromium's test settings). Replay sends no cookies (ADR 0012).
- **`document.referrer`** of a frame (Referrer Policy, default
  `strict-origin-when-cross-origin`; measured, case `referrer`): the
  parent's URL for a same-origin document and for the initial
  `about:blank`, the parent's origin with `/` for a cross-origin one and
  for `about:srcdoc`. swb sends no `Referer` header (unchanged); only the
  document property is set. The Craigslist storage frame needs it.
- **Storage** (case `storage-frames`): a same-origin frame shares
  `localStorage` and `sessionStorage` with its parent; a cross-origin one
  has its own. The `storage` event reaches the other documents with the
  same storage area as a task, with `key`, `oldValue`, `newValue`, `url`
  (the writer's URL) and `storageArea` (measured). The storage key becomes
  (top-level site, origin): Chromium partitions the storage of
  third-party frames by the top-level site (its documented behaviour; the
  probe cannot open a cross-site top level). For first-party documents
  the key is the same as now. The quotas of ADR 0028 section 16 apply per
  key.
- **`sandbox`** (case `sandbox`): without `allow-same-origin` the
  document has an opaque origin (`origin` is `"null"`,
  `contentDocument` is `null`, `localStorage` and `document.cookie` throw
  `SecurityError`, `MessageEvent.origin` is `"null"`); without
  `allow-scripts` its scripts do not run (and `noscript` stays raw text:
  the scripting flag follows JavaScript, sandboxing only stops the
  scripts). The other tokens are parsed (`DOMTokenList.supports` exists)
  and ignored: they gate forms, popups, top-level navigation, modals and
  downloads, and swb either has none of these or blocks them for every
  cross-origin frame (below).
- **Navigation of the top level from a frame**: a cross-origin frame may
  not navigate `top` in M9 (logged). Chromium allows it only after a user
  activation; swb has no activation tracking for it.
- **CORS** of a frame's own requests uses the frame document's origin as
  the request's origin; nothing else changes.

### 4. Rendering

**The box.** An `iframe` becomes a replaced element
(`swb_style::is_replaced_element`). It has no natural size and no
natural aspect ratio; CSS 2.2 §10.3.2 and §10.6.2 give it the default
object size of 300×150 (measured: `iframe` 304×154 with the default
`2px inset` border; `frameborder=0` gives 300×150; `width=120 height=60`
gives 124×64; `width: auto; height: 40px` keeps the width 300; a block
`iframe` with `width: 50%` keeps the height 150). The UA style sheet
already has `border: 2px inset` and `overflow: clip`, the hints have
`frameborder`, `width`, `height` and `align`. Box construction gives it
`BoxContent::Frame(NodeId)`. It is an atomic inline at the baseline, as
an image.

**The frame's viewport** is the content box of the container (measured:
`innerWidth` and `innerHeight` of the frame equal the content box).
After the parent's layout, the engine sets the viewport of each child
frame from the used content box (the geometry index of the parent);
a changed size marks the child's layout dirty and gives its scripts a
`resize` in the next rendering update. A frame with no box keeps its last
viewport (initially 300×150), as a hidden document still lays out for
queries.

**The frame document** lays out as a top-level document does now, in its
own viewport: the root scroller, the overflow propagation, fixed
positioning relative to the frame's viewport, its own scroll offset,
media queries against the frame's viewport (`width`, `height`), and
`prefers-color-scheme` of the page.

**Paint.** The parent's display list gets an item
`DisplayItem::Frame { frame: FrameId, rect }` (the content box) at the
place of the replaced content. The display lists stay separate: the
rasterizer draws the frame's display list when it meets the item, clipped
to the rectangle and translated to its origin minus the frame's scroll
offset, inside the parent's open groups (so the `iframe`'s opacity,
transform, clip and filter apply to the frame). The rasterizer gets the
frame lists through a lookup (`FrameId` to display list and scroll
offset) that the page provides. A change in a frame document rebuilds
only that frame's display list and repaints; the parent's list changes
only when the `iframe` box changes. A full-page screenshot draws frames
at their own scroll offsets.

**Transparency and color scheme** (CSS Color Adjustment 1 §2.3; case
`transparency`): a frame's canvas is transparent, so the parent shows
through where the frame paints nothing, when the used color scheme of the
`iframe` element and that of the frame's root are the same. When they
differ, the frame's canvas is opaque in the frame's canvas color (white,
or rgb(18, 18, 18) for a dark document). Measured: a light frame in a
light parent is transparent; a frame with `<meta name=color-scheme
content=dark>` in a light parent paints rgb(18, 18, 18); a light frame
whose `iframe` has `color-scheme: dark` paints white; dark in dark is
transparent. The `iframe`'s own background paints below the frame.

**`scrolling`**: `scrolling=no` (also `off`, `noscroll`) stops the user's
scrolling of the frame's viewport; the wheel over it scrolls the parent,
and scripts can still scroll it (case `wheel-scroll`). Scrollbars do not
exist in swb's layout (unchanged).

**Hit testing** (`DisplayList::hit_test`): a hit on a `Frame` item
translates the point into the frame (minus the content box origin, plus
the frame's scroll offset) and hit-tests the frame's list; the result is
a (`FrameId`, `NodeId`) pair. Hit testing keeps its work budget per call
across all frames.

**Frames outside the viewport** lay out and paint as others when they are
dirty; no throttling in M9 (Chromium throttles the rendering of hidden
cross-origin frames; a backlog item).

### 5. Scripts: agents, runtimes and realms

**Agents.** Chromium 148 keys its agent clusters by origin: in a
same-site, cross-origin frame `window.originAgentCluster` is `true`, and
`document.domain = "a.localhost"` in `x.a.localhost` and `y.a.localhost`
returns without an error and changes nothing; the frames stay
cross-origin to each other (case `document-domain`). So swb keys its
script runtimes by origin: **one `ScriptRuntime` per origin per page**
(an agent), holding a realm for each document of that origin that has a
`Window`. The top document's runtime is created as now; a frame document
of a new origin gets a new runtime; one of an existing origin gets a realm
in that runtime. An opaque origin (a sandboxed frame, a `data:` URL) is
unique, so it gets a runtime of its own. `about:blank` and `about:srcdoc`
documents take their creator's or parent's origin, so they join its
runtime. `document.domain`'s setter does nothing (as in Chromium with
origin-keyed agent clusters, HTML "document.domain setter" step 9), and
`originAgentCluster` is `true`.

Reasons:

- Same-origin documents must reach each other's objects synchronously
  (`iframe.contentWindow.Array`, `contentDocument.write`, the clean
  built-ins pattern of core-js), so they need one heap (ADR 0026: a
  runtime is one heap; realms of one runtime share it).
- Cross-origin documents never reach each other's objects; they exchange
  serialized messages. Separate runtimes make this a property of the
  types: no object of one heap can point into another.
- Inside a runtime every realm is same-origin with every other, so the
  bindings need no security check between realms. Every cross-origin
  `Window` is in another runtime and is reached only through a remote
  `WindowProxy` (below), whose hooks are the only place of the
  cross-origin rules.
- A runtime per document would break same-origin access; a runtime per
  page would need security checks on every property access between
  realms and would let a cross-origin frame's heap growth stop the top
  document.

**Realms.** A document's realm (its `Window`) is created at its first
need: a script to run, or an access from another realm to its window or
document. An `about:blank` frame that no script touches costs no realm.
The `js` crate makes `create_realm` public, evaluates scripts in a given
realm (`eval_named_at` gets a realm), and the generated
`define_classes`, `create_interfaces` and `init_global` already take a
realm index. Cost: a realm with the `Window` chain is about 0.33 ms and
under 500 KB (the lazy interfaces of M8), charged to the runtime's heap.

**`WindowProxy`.** HTML gives each browsing context one `WindowProxy`
whose target is the `Window` of its current document; it survives
navigations (measured: `contentWindow` before and after a navigation is
the same object, and an expando set on it before is gone after, because
it was on the old `Window`). swb:

- `js` gets a new object kind, `ObjectKind::WindowProxy`, whose data is
  the host's frame id and an optional target (a global object of a realm
  of this runtime). With a target, every internal method forwards to the
  target (HTML §7.2.3.3 for the same-origin case), and the inline caches
  look up the target's shape, so `window.x` stays a cached read (to
  measure with a `vm_bench` row as feature 4a did for the global object;
  budget: at most 5 % on the row "global reads"). Without a target, the
  internal methods call host hooks that implement the cross-origin
  objects (HTML §7.2.3.4: `CrossOriginGetOwnPropertyHelper`,
  `CrossOriginGet`, `CrossOriginSet`, `CrossOriginOwnPropertyKeys`).
- Each runtime has at most one `WindowProxy` object per frame (a table
  `FrameId` to object in the binding state, traced). Identity holds within
  a runtime, which is all that scripts can compare.
- The realm's global `this` value (`[[GlobalThisValue]]`, ECMA-262
  §9.3.1 `SetDefaultGlobalBindings`, HTML "set up a window environment
  settings object") is the `WindowProxy`, not the global object: global
  `this`, `globalThis`, a sloppy function's `this` for `undefined`, and
  `window`, `self`, `frames`, `top`, `parent`, `contentWindow`,
  `document.defaultView`, `MessageEvent.source` and `opener` give the
  proxy (measured: `window === self`, `globalThis === window`,
  `frames === window`, `w.eval('this') === w`,
  `w.Function('return this')() === w`). Global name lookups still go to
  the global object (the `Window`), so they stay cached. Web IDL brand
  checks unwrap a proxy to its target. `js` stores the global this value
  per realm beside the global object.
- When a frame's document changes (a navigation commits), the page
  updates the proxies of that frame in every runtime that has one: the
  target becomes the new `Window` when its realm is in that runtime,
  else the proxy becomes remote.
- After the frame is destroyed the proxy keeps its last target (measured:
  `w.document` is still the old document, `w.Array` still works, `closed`
  is `true`, `parent`, `top` and `frameElement` are `null`, `length` 0).

**Window members.** `frames.length` and `window[i]` give the child frames
in tree order (measured descriptor of `window[0]`: not writable,
enumerable, configurable; a set throws `TypeError: Failed to set an
indexed property [0] on 'Window': Index property setter is not
supported.`; `Object.keys(window)` starts with the indices). Named access
on the window gives a child frame's proxy for its name before elements
(measured `window.n1`). `frameElement` is the container when the
container's document is same-origin with the caller, else `null`
(measured in a grandchild under a cross-origin child). `parent` and `top`
are proxies; on the top level they are the window itself.

**Cross-origin objects** (cases `cross-origin`, `nested-origins`):

- `Window`: own property names `window, self, location, closed, frames,
  length, top, opener, parent, blur, close, focus, postMessage, then`
  and the symbols `@@toStringTag`, `@@hasInstance`,
  `@@isConcatSpreadable` (all reading `undefined`), plus the indices of
  its child frames and the names of child frames whose name is set
  (measured `m.leafname` across a cross-origin child).
- `Location`: `href` (setter only), `replace`, `then`.
- Methods are functions of the caller's realm made per (property, caller
  realm) and cached, so `w.postMessage === w.postMessage`; `location` is
  an accessor pair `get location`/`set location`, `href` a setter `set
  href`.
- Errors (the probe has them verbatim): `SecurityError: Failed to read a
  named property 'document' from 'Window': Blocked a frame with origin
  "O" from accessing a cross-origin frame.`; `Failed to set a named
  property 'foo' on 'Window'`; `Failed to read an indexed property [0]
  from 'Window'`; `Failed to read a named property 'href' from
  'Location'`; `Failed to read a named property from 'Window'` (no name)
  for `String(w)`; and `Blocked a frame with origin "O" from accessing a
  cross-origin frame.` alone for `in`, `delete`, `defineProperty`,
  `setPrototypeOf` and `preventExtensions` ("O" is the caller's origin).
  `Object.getPrototypeOf(w)` is `null`,
  `Object.prototype.toString.call(w)` is `[object Object]`, `typeof w`
  is `object`, `w instanceof Window` is `false`.
- `w.frames[0]` through a cross-origin `w` gives the proxy of a
  grandchild, which is local (with a target) when the grandchild is
  same-origin with the caller (measured: the top reads
  `frames[0].frames[0].document.title` of its own origin through a
  cross-origin child).
- `w.location.href = url` and `w.location.replace(url)` navigate the
  frame from any origin (measured); reads throw.

**Documents and nodes across realms of one runtime.** Each document has
its own arena (section 2), so a node is (document, `NodeId`):

- A wrapper's host data word packs a document key and the `NodeId` (the
  word is 64 bits). The binding state keeps the wrapper vector, the
  `[SameObject]` side table, the listener table and the mutation registry
  per document. `Cx` resolves the document of `this`; the bindings' calls
  of `EngineHost` take the document.
- The runtime lends every document of its agent for a task (a set of
  boxed `EngineHost`s keyed by document), not one. The page takes them
  all back after the task.
- A frame that a task creates (`appendChild` of an `iframe`) needs its
  `about:blank` document inside the task. The page shares its frame
  registry with its runtimes (`Rc<RefCell<...>>`, borrowed only inside a
  native and never across a call into script, `try_borrow` with an
  internal error instead of a panic). The new document is created
  synchronously and added to the lent set; the page files it after the
  task. The same path destroys frames when a task removes an `iframe`.
- Moving a node into a document of another arena (`appendChild` and the
  other insertions, `adoptNode`) transplants it: the subtree is copied
  into the target arena (iterative), removed from the source, and the
  existing wrappers are re-pointed to the new ids, with their listeners
  and `[SameObject]` objects. The wrapper keeps its prototype (measured:
  an element of the parent's realm keeps `HTMLElement.prototype` of the
  parent in the frame's document, listeners and `onclick` stay, and a
  node moved from the frame to the parent keeps the frame's prototype).
  Departures: the form control state and the mutation observer
  registrations of moved nodes are not carried over (rare; logged at
  `debug!`). `importNode` clones into the target arena with the caller's
  prototypes (measured).

**Event handlers, timers and the realm of callbacks**: a timer belongs to
the `Window` whose `setTimeout` was called (measured: `w.setTimeout` from
the parent schedules in the frame), and its callback runs in its own
function's realm.

**Realm lifetime.** Realms are GC roots in `js` today, so a retired realm
(its document unloaded or its frame removed) stays until its runtime is
dropped. A runtime is dropped when its last document goes away (a
cross-origin frame that navigates to another origin drops it at once).
The leak is bounded by a realm budget (section 11). Realm collection
(a retired realm is marked only through its functions and objects, and
swept when unreachable) is a `js` feature after M9's targets work.

### 6. The event loop with several runtimes

HTML gives each agent an event loop. swb keeps one task queue per runtime
(as now) and one thread. `Page::pump` takes, across runtimes, the task
with the earliest due time, then the lowest sequence number; the sequence
counter is per page, so tasks of different runtimes run in the order in
which they were queued. Within one runtime this is the order that
Chromium shows (case `event-loop`: parent and same-origin frame timers of
0 ms run in the order of the calls; case `post-message` and
`message-channel`: a 0 ms timer queued before posted messages runs before
them).

- Microtask checkpoint per runtime after each task (each runtime has its
  own job queue).
- **Rendering update** (HTML §8.1.7.3): one per page frame. For each
  document in tree order, top first (measured order of animation frame
  callbacks: parent, parent, child): `resize`, `scroll`, media queries,
  animation frame callbacks; then style and layout top-down (a child's
  viewport comes from its parent's layout); then resize observers and
  intersection observers per document. One timestamp for all documents of
  the update (Chromium gives the parent's callbacks one timestamp and the
  child's one less than 17 ms earlier; departure).
- **Time budgets**: the 5 s limit per task and the stop after three
  time-outs are per runtime. The 30 s budget of long tasks is per runtime,
  and all runtimes of a page share a cap of 60 s; past it, every runtime
  of the page stops.
- **Load event**: the runtime of a parent document asks the page whether
  its child frames have loaded before it fires `load` (section 2).
- The page clock is shared: one time origin per document, as now.

### 7. Messaging

**`window.postMessage(message, targetOrigin, transfer)`** and the options
overload (HTML §9.4.3; cases `post-message`, `message-channel`):

1. `targetOrigin`: `*`, `/` (the caller's origin) or a URL whose origin
   is used; anything else throws `SyntaxError: Failed to execute
   'postMessage' on 'Window': Invalid target origin 'foo' in a call to
   'postMessage'.` A missing `targetOrigin` means `/` (measured).
2. `StructuredSerializeWithTransfer` in the sender's realm (the
   `Serialized` record of ADR 0028 feature 8a1, which owns its data and
   outlives the heap), with `ArrayBuffer`s (detached in the sender,
   measured) and `MessagePort`s as transferables. Errors with Chromium's
   wording and the prefix `Failed to execute 'postMessage' on 'Window':`
   (measured: `() => 1 could not be cloned.`, `Symbol(s) could not be
   cloned.`, `HTMLBodyElement object could not be cloned.`, `#<Window>
   could not be cloned.`, `ArrayBuffer at index 1 is a duplicate of an
   earlier ArrayBuffer.`; a non-object in the transfer list is a
   `TypeError`).
3. The page queues a task on the posted message source of the target's
   runtime with the record, the source frame and the source origin. A
   message to a document in the same runtime also goes through the
   record (HTML requires a copy).
4. In the task: if `targetOrigin` is not `*` and is not the target
   document's origin at that time, the message is dropped (measured:
   silently for the script; Chromium logs a console error, swb logs the
   same line). Otherwise `message` fires at the target's window: `data`
   deserialized in the target realm, `origin` the source's serialized
   origin (`"null"` for opaque), `source` the source's `WindowProxy` in the
   target runtime (local or remote), `ports` a frozen array, `isTrusted`
   true, not bubbling, not cancelable, `lastEventId` empty (measured).

Messages to one target arrive in the order sent (measured 1, 2, 3 across
origins).

**`MessageChannel` and `MessagePort`** (HTML §9.5):

- A page-level port registry: each port has an id, its entangled port,
  its owner (runtime and realm), its message queue, and whether its
  queue is enabled. A port object is a host object of its realm with the
  port id.
- `postMessage(message, transfer)` and the options form serialize as
  above and append to the entangled port's queue. The queue delivers
  when enabled: by `start()`, or by setting `onmessage` (measured: with
  `addEventListener` alone nothing arrives before `start()`). Each
  message is a task of the owner's runtime, with no timer clamping
  (measured: 1,000 port messages chained in 50 ms against 19 chained 0 ms
  timers; React's scheduler depends on this). `MessageEvent` of a port
  has `origin` `""` and `source` `null` (measured).
- Transfer detaches the port in the sender and makes a new port object
  in the receiving realm with the same id; its queue moves with it.
  `close()` disentangles; Chromium 148 has no `close` event (no
  `onclose`; measured prototype `onmessage, onmessageerror, close,
  postMessage, start`).
- Errors (measured): `Port at index 0 contains the source port.`,
  `Message port at index 1 is a duplicate of an earlier port.`,
  `structuredClone` of a port without transfer: `DataCloneError: Failed to
  execute 'structuredClone' on 'Window': A MessagePort could not be cloned
  because it was not transferred.`; `new MessagePort()` is an illegal
  constructor.
- Within one document (BBC's React scheduler) the same code runs with
  both ports in one realm. This part needs no frame and comes first
  (section 14).

**`BroadcastChannel`** is out (section 13) unless the diagnosis shows
that a target needs it.

### 8. Input

- **Pointer**: the page hit-tests the top display list; a hit in a frame
  continues in it (section 4), and the event goes to the node in the
  frame's document, in the coordinates of the frame's viewport. Hover
  state is per document: the parent's hovered chain ends at the
  `iframe`; the frame's chain starts at its root. Leaving a frame sends
  the frame's documents the `mouseout`/`mouseleave` events of leaving the
  document. The cursor comes from the hit document. A press that starts
  in a frame keeps its events in that frame until the release (the
  press's target document).
- **Wheel** (ADR 0019): the scroll chain starts at the innermost scroller
  in the frame's document, ends at the frame's viewport (unless
  `scrolling=no`), and continues with the parent's chain from the
  `iframe`. Measured: a wheel scrolls the frame; a wheel past the frame's
  end does not move the parent in the same event; the next wheel with the
  frame at its end scrolls the parent. swb picks the scroller per event
  (no latching, ADR 0019), which gives the same result for these steps.
- **Focus** (ADR 0009): each document has a focused area. Clicking into a
  frame focuses the frame's element; the parent's `activeElement` is the
  `iframe`; `document.hasFocus()` is true in both (measured). Events
  (measured order): the parent's element `blur`, the parent window
  `blur`, the frame window `focus`, the element `focus`; leaving: the
  element `blur`, the frame window `blur`, the parent window `focus`, the
  next element `focus`. Sequential navigation treats a frame as its
  content: Tab in the parent enters the frame's first focusable element,
  and Tab after the frame's last one goes to the parent's element after
  the `iframe` (measured). Keys go to the focused document.
- **Selection**: per document; a drag selection stays in the document
  where it started; copy copies the focused document's selection.
- With JavaScript off the same paths run without DOM events (as now for
  the top document).

### 9. JavaScript off

Owner decision of 2026-11-01: frames work with JavaScript off. A frame
document then loads, lays out, paints and takes input, and runs no
scripts; its scripting flag is off, so `noscript` content renders
(measured in Chromium with JavaScript disabled, case
`javascript-off`: the frame's `noscript` block shows, its script does not
run). Nothing in sections 2 to 4 and 8 depends on the setting.

JS-off fixtures with frame elements (`fixtures/pages/*`): only `iframe`
elements, all `display: none`:

- `ars-technica`: `<noscript><iframe src="https://www.googletagmanager.com/ns.html?id=GTM-NLXNPCQ" ... style="display:none;visibility:hidden">`;
  with scripting off the `iframe` is an element. Its document (in the
  fixture) contains another such `iframe` (`GTM-PVBXQMCR`, also in the
  fixture).
- `craigslist-posting`: `iframe#cl-local-storage` with
  `style="display:none;"`, the storage frame (in the fixture).
- `bbc`, `hacker-news`, `senko-net`, `wikipedia-web-browser`: none. No
  fixture has `frame`, `frameset`, `object` or `embed`.

Expected change when frames render: three more requests, all served from
the fixtures (Chromium's capture made them too); no box and no pixel
changes, because the frames have no boxes and the box dump covers the top
document. The feature that turns rendering on reports the scores.

### 10. Automation and tools

- **Frame ids.** `page.frames` returns the frames in tree order:
  `frameId`, `parentId`, `url`, `origin`, `name`, the `nodeId` of the
  container in the parent's document, and the container's content box in
  the parent's coordinates. `page.evaluate`, `page.consoleMessages`
  (filter), `dom.*` and the `input.*` methods that take a node take an
  optional `frameId` (default: the top). Node ids are per document.
- **`--eval`** evaluates in the top document, as now. A new
  `--eval-frame=PATH` sets the frame for the `--eval` arguments after it;
  `PATH` is CSS selectors of `iframe` elements from the top, joined by
  ` >> ` (Playwright's frame selector form).
- **The probe tool**: a `js` query may have `"frame": "SEL >> SEL"`;
  Chromium evaluates it in `frame_locator(...)`'s frame, swb with
  `--eval-frame`. A step may have `"frame"` for its selector. Cases that
  can measure from the top (same-origin access, `postMessage`) do not
  need it.
- **`--dump-boxes`, `dom.boxes`, `compare`, `reference`**: the box dump
  covers the top document, as Chromium's main-frame dump in the tools;
  frame contents count in the pixel score (both screenshots include
  frames). `dom.boxes {frameId}` dumps a frame's document for tests.
- **`swbtools replay`**: steps with `"frame"` as in the probe; clicks by
  coordinates reach frames without it.
- **`page.info`** gets `frames` (the count) and the load state counts the
  frames' loads.
- **Console**: the messages of all documents, each with its `frameId`.

### 11. Budgets and robustness

| Resource | Limit | Result |
|---|---|---|
| Frames per page | 1,000 (Chromium 148, measured: the 1,001st `iframe` has `contentWindow` `null`) | the `iframe` gets no frame (`contentWindow` `null`), a warning |
| Nesting depth of frames | 32 (Chromium has no own limit below its frame count; 300 measured) | the frame is not created, a warning. Bounds the recursion of paint, raster and hit testing |
| A URL that two ancestors' documents have | refused (Chromium, measured) | the frame stays `about:blank` |
| Navigations of one frame | 64 per 10 s of page time (a frame that reloads itself, `meta refresh 0`) | later navigations are dropped with one warning |
| Runtimes (origins with scripts) per page | 32 | a document of a further origin runs without scripts (scripting flag on, no runtime, as after a heap-limit stop), a warning |
| Realms created per runtime over its life | 256 | the document runs without scripts, a warning (until realm collection exists) |
| JavaScript heap | 256 MiB per runtime, 512 MiB for all runtimes of a page | before each task the runtime's limit is the smaller of 256 MiB and what the page has left; past it the task ends (termination), as now |
| Long tasks | 30 s per runtime, 60 s per page | the runtime stops; past the page cap all runtimes stop |
| Messages that wait (window and port), per page | 65,536 messages and 256 MiB of serialized bytes | `postMessage` drops the message with one warning per task (Chromium has no limit) |
| One message | the structured clone limits of ADR 0028 section 16 | `DataCloneError` or `RangeError` as now |
| Message ports per page | 65,536 | `new MessageChannel()` throws `RangeError`, a warning |
| Frame viewport | the viewport limits (`MAX_VIEWPORT_SIDE`) | clamped |
| Hit testing | the 40 ms budget per call covers all frames | as now |

Hostile pages to add (`just hostile`): 1,100 frames created in a loop; a
frame nested 100 deep through `srcdoc`; a page that embeds itself with a
growing query string; a frame that reloads itself with `meta refresh 0`
and with `location.reload()` in `load`; a `postMessage` flood to self and
to a cross-origin frame; a `MessageChannel` ping-pong without end; a
transfer of 10,000 ports; 40 frames of 40 origins; a frame that creates
and removes `about:blank` frames in a loop (realm budget); a cross-origin
frame that allocates to its heap limit (the top document keeps running).
Each case ends within the set's limits and logs the limit that acted.

### 12. Tests

- **Probe**: `tools/probes/web-frames.json` (this ADR's measurements).
  With `--with-swb`, the `js` queries compare swb with Chromium; the Rust
  tests take their expected values from its output, as for the other
  `web-*` files. Cases with `steps` (focus, wheel) run in the automation
  tests (`real_input.rs` pattern); `pixels` cases (transparency, JS off)
  compare swb's screenshot.
- **Engine tests**: the frame tree (insertion, `src` changes, removal,
  navigation, recursion, limits), loading and the load event, rendering
  (box, viewport, nested display list, clip, scroll offset, color
  scheme), hit testing and the wheel chain, focus moves, JS-off frames.
- **`web` and `js` tests**: `WindowProxy` identity and forwarding (also in
  the GC stress mode), the global this value, cross-origin properties and
  errors, `postMessage` and `MessageChannel` order, transfers, adoption
  across arenas.
- **Targets**: Craigslist (the favorites button stores through the
  storage frame), BBC (the consent dialog renders in its frame and the
  accept button inside it closes it), Ars Technica (no frame errors);
  `compare --js` scores of all targets; `just snapshot` for the JS-off
  fixtures, with the frame report of section 9.

### 13. Out of scope, with reasons

- `frame` and `frameset`: no target uses them; they keep the current
  rendering (nothing for `frameset` documents).
- Documents in `object` and `embed`: no target; `object` keeps its
  fallback content.
- `window.open`, `opener` other than `null`, auxiliary browsing
  contexts: swb has one page per process (ADR 0028 section 2).
- Joint session history (section 2): measured, a backlog item.
- `BroadcastChannel`, `SharedWorker`, workers: no target need known.
- CSP (`frame-src`, `frame-ancestors`), `X-Frame-Options`: swb implements
  no CSP, and the fixtures do not store these headers. A security
  backlog item (they protect the framed site from clickjacking).
- The `allow` attribute (permissions policy): it gates features that swb
  does not have (camera, fullscreen, payment).
- `loading="lazy"` on `iframe`: frames load at once (a backlog item if a
  target's frames depend on it).
- `credentialless`, fenced frames, `document.domain` effects (a no-op as
  in Chromium), process isolation (all runtimes run on the page thread),
  rendering throttling of hidden frames.

### 14. Features

Each feature is one commit; swb stays usable after each, and the JS-off
fixtures change only as section 9 says. The roadmap (M9) gives the
scope, exit and agent type of each.

1. **`MessageChannel` and `MessagePort` in one document** (`implementer`):
   the port registry, `start`, transfer within a runtime, the errors.
   BBC's React scheduler. No frames needed.
2. **Frame tree, loading and rendering** (`implementer-hard`): `Frames`,
   loader clients, the `iframe` replaced box, frame documents (`src`,
   `srcdoc`, `about:blank`), viewports, the nested display list, color
   scheme, recursion and limits, the load state; with JavaScript off fully
   working; with JavaScript on, frame documents parse without scripts.
   Reports the JS-off fixtures.
3. **Input in frames** (`implementer`): hit testing into frames, hover,
   the cursor, the wheel chain and `scrolling=no`, focus and Tab, keys,
   selection limits; automation `page.frames` and `frameId`;
   `--eval-frame` and the probe and replay `frame` options.
4. **`WindowProxy` and same-origin frame realms** (`implementer-hard`;
   `js`, `web`, `engine`): the `WindowProxy` object kind and the global
   this value in `js`, the public realm API; per-document binding state
   and lending of several documents; realms of same-origin frames in the
   parent's runtime; `contentWindow`, `contentDocument`, `frames`,
   `length`, `parent`, `top`, `frameElement`, indexed and named access;
   synchronous `about:blank` creation and `load`; adoption across arenas.
5. **Runtimes per origin and cross-origin objects** (`implementer-hard`):
   agents in the page, the event loop across runtimes, remote proxies and
   the cross-origin `Window` and `Location`, `sandbox` origins, budgets
   across runtimes, `document.domain` and `originAgentCluster`.
6. **Messaging and storage across documents** (`implementer`):
   `window.postMessage` with transfer, `MessageEvent.source`, ports
   between runtimes, storage keys and `storage` events across documents,
   site for cookies of frames, `document.referrer`. Exit: the Craigslist
   favorites work; BBC's consent dialog renders and closes.
7. **`document.open`, `write` and `close` on frame documents**
   (`implementer`, after M9's `document.write` feature): core-js's
   pattern in Sourcepoint's wrapper.

Later: realm collection in `js`, joint session history, throttling of
hidden frames.

## Consequences

- A page becomes a tree of documents, each with the state that the top
  document has now; the rendering and query code of a document stays as
  it is, and the page composes the frames in paint, hit testing, input
  and loading.
- Scripts get one runtime per origin: same-origin access is a plain heap
  access, and every cross-origin rule is in one place, the remote
  `WindowProxy`. A hostile cross-origin frame cannot reach the top
  document's heap and has its own limits; a page-wide cap bounds the sum.
- `window` and the global object become two objects. Global name reads
  stay cached; `window.x` reads take one more step in the inline cache.
- The bindings address nodes by (document, `NodeId`). Moving nodes
  between same-origin documents copies them; the rare state that does
  not move is a documented departure.
- Frames work with JavaScript off. The JS-off fixtures load three more
  documents and should not change visibly.
- No joint session history, no `window.open`, no CSP: departures that the
  backlog records with their measurements.
