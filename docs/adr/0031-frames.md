# ADR 0031: Frames

- Status: accepted (the owner approved the decisions on 2026-11-01:
  no joint session history, the page-wide limits of section 11, storage
  keyed by top-level site and origin, no top-level navigation from
  cross-origin frames without a user's activation)
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

**Done (feature 10a, `engine/src/frames/`, `page/frames.rs`,
`document/frames.rs`).** The arena and the life of frames as above, with
these decisions and departures:

- The top-level frame's record has the tree data only; its document stays
  in `Page::doc`, which script tasks borrow (the lending code is
  unchanged). A new top-level document destroys the child frames but keeps
  the arena, so the ids of old frames stay stale.
- The journal drain reports the `iframe` elements that were inserted,
  removed, and whose `src`, `srcdoc` or `name` was set
  (`FrameChanges`); a walk of the whole document (an incomplete journal)
  makes the page compare its frames with the document's `iframe` elements.
  The page acts after each drain point: the return of a lent document, a
  change through `change_dom`, the commit of a document, the end of a
  parse. With a script runtime the natives make and destroy the frames inside
  the task (feature 12b1, section 5). A move of an `iframe`
  destroys its frame and makes a new one (it is removed and inserted).
- Every set of `src` or `srcdoc` navigates, also to the same value.
  `about:blank` and `about:srcdoc` documents commit in the next turn of the
  event loop (`Page::run_frame_navigations`), not inside the insertion;
  the initial `about:blank` is made at once. With no `src` and no
  `srcdoc`, or `src` `about:blank`, at the insertion the frame keeps its
  initial document and counts as loaded at once.
- The container's `load` event fires as a task of the parent's runtime
  when the frame's document and its own frames have loaded
  (`Page::check_frame_loads`, children first); only containers in the
  top-level document can have listeners in 10a. The order in swb is parent
  `DOMContentLoaded`, `srcdoc` `iframe` `load`, `iframe` `load`, parent
  `load`, as in Chromium's case `load-order` without the entries of the
  frames' scripts (feature 12b1 adds them, and the `load` of the initial
  `about:blank` inside `appendChild`).
- The frame's own declarative refresh (`<meta http-equiv=refresh>`, HTML
  "shared declarative refresh steps") navigates the frame at its time of
  page time; the top-level document still has none (backlog). A frame
  document without a script parses to the end at once (no parser in steps)
  with the page's scripting flag.
- A frame document's `@font-face` rules are not used in 10a: the page's
  font context holds the web fonts of one document (backlog for 10b).
- A `javascript:` URL in `src` makes no navigation; `src = "javascript:"`
  does nothing, as in Chromium (feature 12b1 runs a script in it).
- The fallback base URL of an `about:srcdoc` or `about:blank` frame
  document is its parent's base URL (HTML "fallback base URL"), so a
  relative image or style sheet in a `srcdoc` frame requests the URL that
  Chromium requests (cases `srcdoc-base`, `srcdoc-no-base`). `document.URL`
  stays `about:srcdoc`.
- The sandboxing flags of a frame are the container's `sandbox` attribute
  joined with the parent frame's flags (HTML "determining the creation
  sandboxing flags"); a token allows only if every sandboxed level allows
  it (case `sandbox-nested`: a frame without `sandbox` inside
  `sandbox=allow-scripts` has an opaque origin).
- A frame that is removed no longer delays the parent's `load`: the
  removal re-checks the load state (`Page::change_dom`, the next `pump`
  and `wait_until_loaded`), so `load` fires at once when the removed frame
  was the last one that the page waited for. A `load` that fires for a
  container reaches it only if the container still has the frame that
  loaded (an earlier listener may have removed it and inserted it again).
- The refresh delay and URL may be separated by white space alone
  (`0 s2.html`, `0 url=s2.html`; HTML "shared declarative refresh steps"
  step 11, measured). The `0;"s2.html" x` form differs from Chromium and is
  left.
- Limits (section 11): 1,000 frames, depth 32, 64 navigations per frame
  in 10 s of page time, one warning each (per top-level document, per
  frame); the page-wide budgets for frame documents (source bytes, nodes,
  response bodies, images, style sheets) in the table of section 11.
  Chromium nests 300 deep (case `limits`); swb stops at 32.
- Recursion: as Chromium, a navigation to a URL (without fragment) that
  two or more ancestors' documents have is refused (case `self-embed`).

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
  scripts). Without `allow-forms` its forms do not submit, without
  `allow-popups` it opens no new browsing context, and it navigates only
  itself and its descendants, and the top level with an
  `allow-top-navigation` token (feature 11, section 8). The other tokens are
  parsed (`DOMTokenList.supports` exists) and ignored: they gate modals and
  downloads, which swb lacks in frames.
- **Navigation of the top level from a frame**: a cross-origin frame without
  a sandbox may navigate `top` after a user's activation of that frame: a press
  or a key in it, in a frame inside it, or in a frame of its origin
  (`Frames::note_activation`; "Done (feature 13b)", with the measurements). A
  sandbox's `allow-top-navigation` token allows it.
- **CORS** of a frame's own requests uses the frame document's origin as
  the request's origin; nothing else changes.

**Done (feature 10a).** `swb_net::Loader::new_client`, `start_for`,
`start_api_for` and `cancel_client`; `Completion::client`. The page routes
a completion to the document request of a frame (a table of request ids),
else to the document of its client, else to the top-level document as
before. `cancel_client` cancels the queued requests of a client; requests
that run end and are ignored. The request of a frame document:
`Destination::IFrame` (`Accept` as for documents), mode `navigate`,
credentials `include`, initiator the parent document's origin, and the new
`Request::site_for_cookies`: the top-level origin if the parent and its
ancestors are same-site with it, else an opaque origin. A frame document
gives its subresource requests its own site for cookies (the top-level
origin if it and its ancestors are same-site with it) and its loader client
(`DocumentState::start_subresource`). `SameSiteContext::of` compares the
site for cookies, when there is one, with the request URL, so a cross-site
frame document and its requests set and send only `SameSite=None` cookies
(case `cookies-frames`, the response part; `document.cookie` in frames
comes with scripts in frames). `sandbox` is parsed (`Sandbox`): without
`allow-same-origin` the document's origin is opaque; `allow-scripts` gates
the scripts of the document (feature 12b1). The flags are read at
each navigation. The origin of `about:blank` and `about:srcdoc` documents
is the parent document's; a frame document whose request failed shows an
error document with an opaque origin. `document.referrer` and storage keys
of frames come with feature 14.

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
`DisplayItem::Frame { node, rect }` (the `iframe` element and its content
box; "Done (feature 10b)" below gives the reason) at the place of the
replaced content. The display lists stay separate: the
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

**Done (feature 10a: the box and the viewport; paint is feature 10b).**
`iframe` is a replaced element (`swb_style::is_replaced_element`, so its
children render nothing and it is never a list item); its natural size is
`NaturalSize::default()` (no width, height or ratio), so the default object
size of 300×150 applies; box construction gives `BoxContent::Frame(node)`,
which paints nothing in its content box yet (the `iframe`'s background and
border paint). Case `geometry` matches Chromium within 1 px for the eight
boxes (304×154, `frameborder=0`, `width`/`height`, `width: auto`, a block
with `width: 50%`, `min-width`). After the page's layout
(`Page::update_layout`, and after `Page::render`) the page lays out the
frame documents parents first (`Page::layout_frames`): a frame's viewport
is its container's content box in the parent's latest layout (one walk of
the parent's fragments per parent and layout), clamped to 0 ..
`MAX_VIEWPORT_SIDE`; without a box it keeps the last one (initially
300×150). A changed viewport, scale or color scheme reaches the frame
document through `DocumentState::set_view`, which restyles it. The frame
document lays out as a top-level document (case `geometry`: the frame
viewports and root widths equal Chromium's `innerWidth`,
`clientWidth`; quirks mode for `about:blank`, no-quirks for
`about:srcdoc`). No `resize` events (frame documents run no scripts).

**Done (feature 10b: paint; `paint/raster/frame.rs`, `engine/frames/paint.rs`,
`Page::prepare_frames`).** Measured in Chromium 148 with the cases `paint-*`
and `transparency` of `tools/probes/web-frames.json` (three equal runs; all
points within 2 levels). Decisions and departures:

- *The item.* `DisplayItem::Frame { node, rect }`: the `node` is the `iframe`
  element, not a `FrameId` as above. The lookup finds the frame from the
  document and the element, so the parent's list does not change when a frame
  starts, ends or navigates (a move of an `iframe` changes the list anyway),
  and the paint crate has no ids of the engine.
- *The lookup.* `swb_paint::ImageSource::frame(node)` gives a `FrameContent`:
  the display list, the transform group ends, the scroll offset, the canvas
  and a source for the frame's own images and frames. `ImageSource` is
  already passed through the whole rasterizer. A page without frames passes
  its `Images` as before; a page with frames passes `frames::paint::FrameSource`.
- *The raster* draws the list at its item, in the parent's open groups, with a
  clip to the content box (`Rasterizer::frame`). The origin of the list is the
  corner of the content box snapped to a device pixel, as the clip is (case
  `paint-fractional`: the frame at `left: 10.5px` starts at pixel 11, and the
  content of the frame stays on whole pixels). `overflow: clip` of the `iframe`
  with `border-radius` is a rounded clip around the item (case `paint-effects`).
  Fixed boxes in a frame start at the corner of the content box
  (`Rasterizer::fixed_origin`) and stay inside the frame's clip
  (`Rasterizer::viewport_clip`, also for the layers of groups with fixed
  content); sticky boxes use the frame's scroll offset and their own cache
  (cases `paint-fixed`, `paint-sticky`). Frames that the visible area misses
  are culled before the lookup, as other items.
- *Order.* `Page::render_in_strips` builds the page's display list, then
  `prepare_frames`: the layout of the frames parents first, and for frames whose
  container has a box the display list and the canvas. So the first paint shows
  the frames. A `display: none` frame lays out but has no list. `render` and
  `screenshot` (viewport and full page) take this path, in the GUI too; a
  full-page screenshot draws frames at their own scroll offsets (case
  `paint-full-page`).
- *Invalidation.* A change in a frame document drops that document's display
  list, as in any document, and the next rendering builds it; the parent's list
  stays (test `an_image_that_loads_in_a_frame_repaints_only_the_frame`, with
  the new `FlushCounters::display_list_runs`). The completions of frame
  documents already make `Page::pump` report a change, which is what the GUI's
  frame loop redraws on.
- *Backdrop roots.* A `backdrop-filter` in a frame reads through the frame:
  Chromium's transparent frame shows its parent's red inverted (case
  `paint-backdrop`, 0,255,255). So the frame's root is no backdrop root and the
  frame needs no layer; the backdrop is the surface below, as for a box in the
  parent (`raster/backdrop.rs` is unchanged).
- *Transparency and color scheme.* `color-scheme` is now a property (inherited,
  `swb_style::used_color_scheme`, the meta name in `document/frames.rs`; case
  `paint-color-scheme-syntax`, with `only` last in the serialization and `none`
  as a custom identifier). Measured: `meta` with `light dark` is light on a
  light page; `body { color-scheme: dark }` does not count, only the root's
  value; the root's `color-scheme: light` wins over a `dark` meta element; an
  `iframe` with `normal` takes the meta element of its own document, so a light
  frame in a dark document has a white canvas. The default text color of a
  frame document with a dark root is white (`CanvasText`,
  `Stylist::set_dark_canvas_text`, two style runs when it changes). Departure:
  the other dark defaults (links, form controls) stay light, and the top-level
  document does not use color schemes at all (a backlog item; its pixels must
  not change in this feature).
- *Web fonts.* See ADR 0022, "Frame documents".
- *Budgets.* See section 11. A frame past `MAX_FRAME_ITEMS` paints nothing, also
  no canvas, with one warning per rasterization (case `paint-past-budget`:
  Chromium paints the document of 900,000 boxes blue; swb leaves the frame
  transparent).

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
- Methods are functions of the caller's realm, one per (property, caller
  realm), so `w.postMessage === w.postMessage`, also for two frames
  (`w.focus === g.focus`, measured in feature 13a); `location` is an accessor
  pair `get location`/`set location`, `href` a setter `set href`.
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
  per document. A native runs with the document of its `this` as the
  current document of the binding state, so the bindings' calls of
  `EngineHost` need no document argument (feature 12a, below).
- The runtime can reach every document of its agent in a task: the page
  lends the top-level document, and a native borrows another one from the
  registry at its first switch to it (feature 12b1, below). The page takes
  them all back after the task.
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
  `importNode` clones into the target arena with the prototypes of the
  realm of its `this` (measured; "Done (feature 12b2)").

**Event handlers, timers and the realm of callbacks**: a timer belongs to
the `Window` whose `setTimeout` was called (measured: `w.setTimeout` from
the parent schedules in the frame), and its callback runs in its own
function's realm.

**Realm lifetime.** Realms are GC roots in `js` today, so a retired realm
(its document unloaded or its frame removed) stays until its runtime is
dropped. A runtime is dropped when its last document goes away (a
cross-origin frame that navigates to another origin drops it in the next
turn of the event loop).
The leak is bounded by a realm budget (section 11). Realm collection
(a retired realm is marked only through its functions and objects, and
swept when unreachable) is a `js` feature after M9's targets work.

**Done (feature 12a).** The plumbing of this section, with one document in
each page's runtime; no page changes its behaviour.

- `js`: `create_realm` is public, `eval_named_at` and `eval_module_at` take
  a realm, and each realm has `[[GlobalThisValue]]`
  (`Runtime::set_global_this`, ADR 0026 section 17). `ObjectKind::WindowProxy`
  holds the frame id and an optional target. The generic internal methods
  treat it as a proxy (`vm/proxy.rs` hands it to `vm/window_proxy.rs`), so the
  paths that test `needs_runtime` needed no change. A proxy without a target
  calls `WindowProxyHooks`.
- Inline caches: the entry `SiteCache::WindowProperty` names the target by
  identity, the holder, the slot and the watch epoch. A `Window` has a
  dictionary shape, so the shape proves nothing; the target and its chain are
  watched, as for the global entries. A retarget changes the identity that
  the hit compares, so no old holder is read.
- `web`: the realm's global this value is the frame's proxy (the table
  `WebState::window_proxies`). `this_or_global` gives the proxy for
  `undefined`, so `Platform::object` of a `Window` member is the proxy, and
  `Heap::host_slot` looks through a proxy, which is the unwrapping of every
  brand check. `rt.global()` stays for the realm's own definitions; every
  value that goes to script comes from `state::window_proxy`.
- **The split.** `WebState` keeps what belongs to the agent; `DocState` what
  belongs to a document and its `Window` (ADR 0028 "Feature 12a notes" has the
  list). `state.doc` is a `Box<DocState>`, the current document. The word of a
  wrapper is `(document key << 32) | node index`.
- **How a native finds its document.** The glue's `run` and `run_promise` and
  the hooks of legacy platform objects and of `WindowProperties` start with
  `enter_call` or `enter_object`: if `Runtime::host_flag` says that the runtime
  has several documents, the document of `this` (a word with a key, else the
  realm of the host object, which the slot now holds) or of the function's
  realm becomes current, and `leave` restores what was found. That design was
  chosen over a document argument on every binding (1,600 call sites) and over
  a hook on the realm switch of a call (a native of realm A with a node of
  document B as `this` must also switch: a moved node keeps its prototype).
  The flag is a field of the runtime, so a runtime with one document pays one
  load and one comparison, without the type check of `host_data`.
- A task remembers the document that was current when it was queued
  (`TaskQueue`), and `run_due_tasks` runs it there. Timers are per document;
  the queue is per agent.
- **Node conversions.** A `NodeId` of another document cannot reach an arena:
  an argument conversion (`convert::node`) fails with a `TypeError`, and the
  other readers of a word go through `b::node_here`. `this` needs no check,
  because the entry made its document current. Feature 12b replaces these
  failures with adoption.
- **Measured** (case `windowproxy-top`, three equal runs; every value is
  compared in `crates/engine/tests/frames/window_proxy.rs`): the identities,
  descriptors (`Window`'s accessors and own functions; a `var` is not
  configurable), `in`, `delete`, `Object.preventExtensions` (`TypeError: Cannot
  prevent extensions`, `Cannot freeze`, `Cannot seal`), `Reflect.preventExtensions`
  `false`, `Object.setPrototypeOf` (`Immutable prototype object '#<Window>'
  cannot have their prototype set`), a getter by name and through the proxy
  sees the proxy, `undefined` as `this` of a member works and `{}` is `Illegal
  invocation`, also through `new Proxy(window, {})` and `Object.create(window)`.
  An index on a `WindowProxy` without frames: a sloppy write is ignored, a
  strict one throws `Failed to set an indexed property [6] on 'Window':
  Indexed property setter is not supported.`, `Object.defineProperty` throws
  the same with `Index property setter`, `Reflect.defineProperty` and
  `Reflect.set` answer `true` and define nothing.
- **Departures.** `opener`, `frameElement` and `name` of the window are still
  missing (three rows of the test; feature 12b adds them, because a page that
  reads `name` could change its path). Eval of a function over a
  non-configurable global property gives the `TypeError` of ECMA-262 §19.2.1.3;
  V8 gives a `SyntaxError` (the fourth row). Objects that a native makes with a
  `this` of another document are in that document's realm (`Cx::realm`), the
  function's realm only with one document.
- **A known gap that only several documents reach.** `WebState::blob_urls` is per
  agent but a document's unload does not revoke yet.
- Costs: docs/performance.md, "`WindowProxy` and the split of the binding
  state".

**Done (feature 12b1).** Real frames on the plumbing of 12a: same-origin frame
documents run their scripts in realms of the page's runtime. Measured in
Chromium 148 with the cases `scripted-*` of `tools/probes/web-frames.json` (18
cases, three equal runs); every value is compared in
`crates/engine/tests/frames/scripted.rs`, and a departure has a row there.

- *The registry.* `crates/engine/src/frames/registry.rs`, `swb_web::FrameHost`.
  For the length of a task the page swaps its `Box<Frames>` with an empty one
  in an `Rc<RefCell<FrameRegistry>>` that the runtime holds, so no field of
  `Page` is shared and the page's code keeps `self.frames`. A native borrows the
  registry for one call, never across a call into script; a registry in use is
  an internal error (`VmError::invariant`), not a panic. What needs the page
  waits in `AfterTask`: the attributes of frames that the task made (which start
  navigations) and the input state of those that it destroyed.
- *The lent set is lazy.* `Page::run_task_in` lends the top-level document and
  no other. When a native switches to a document that has no host
  (`state::switch_to`), the document comes from the registry
  (`FrameHost::take_document`) and goes on the list `WebState::lent`; the page
  takes back that list after the task (`take_back_documents`), files each
  document by its `frame_id`, processes its changes and acts on its requests. So
  the cost of a task follows the documents that it touches, not the realms. The
  alternative, lending every document with a realm for each task, costs a loop
  over up to 256 documents per task.
- *The font context.* One cell (`LentFonts`), which all documents of the page
  share while a task runs; a frame document swaps its own web font faces in for
  a query (`with_lent_fonts`).
- *Frame numbers.* `FrameId::number` is `(generation << 10) | index`, below 2^30:
  at most 1,000 live frames, a free slot before a new one, and a slot whose
  generation reaches 2^21 is not used again. So the number needs no table and
  is unique over the life of the arena.
- *First need.* A frame document gets a realm when it commits if its source can
  have a script (`may_have_script`: a `<script`, or an attribute `on...=`; a false
  positive costs a realm, and the parse in steps is the only other change), or
  at the first access from a script (`frames::window_of_frame`), or at commit if
  the runtime has seen the frame's proxy, which a script may hold. A document
  with no script keeps the full parse and the admission of section 11. A
  document with a script parses in steps, and its parser stops at the room that
  the page's node budget has left (`Frames::node_room`; it shows what it parsed,
  a departure from the error document of section 11).
- *Budget of realms.* 256 per runtime, with one warning. Past it the document
  runs without scripts, and its proxy has no target (`SecurityError` from the
  hooks; `contentDocument` is `null`). Consequence: Chromium's `limits` query
  reads 1,100 windows and then nests 300 frames; swb runs out of realms in the
  first part, so the test runs the parts on two pages. A realm stays until the
  runtime ends, so the budget counts every document that ever had a realm, not
  the live ones: a long-lived page that replaces a same-origin frame with scripts
  (an ad slot, a widget that reloads) stops running scripts in new frames after
  255 of them, until the next top-level document. Chromium has no such limit.
  Realm collection in `js` removes the limit (roadmap, backlog from M9).
- *Window members.* `length`, `top`, `parent`, `frameElement`, `opener`, `name`,
  `closed`, `origin`, the index keys of a proxy (`WindowProxyHooks::child_count`,
  `child_frame`: not writable, enumerable, configurable; first in the key
  order) and named access (a child frame's name before elements), as measured.
  `name` is the `name` attribute at the creation of the frame; a later change of
  the attribute does not rename the frame (measured, case
  `scripted-window-members`), so `FrameChanges::names` went. `frameElement` and
  `contentDocument` have the IDL type `object?` and `getComputedStyle` takes
  `any`, because a generated node conversion looks in the current document.
  `getComputedStyle` of an element of another document works (measured).
- *Geometry across documents.* `state::engine_laid_out` is the path of each
  geometry query in `web`: with several documents, `FrameHost::update_viewports`
  lays out the documents above the frame, top first, and gives each frame its
  viewport from its container's box. Measured: a change of the `iframe`'s style
  shows at once in `innerWidth`, `clientWidth`, `getComputedStyle`,
  `getBoundingClientRect` and `matchMedia`. A runtime with one document pays one
  comparison. Departure: a `display: none` frame keeps its last layout
  (Chromium: `clientWidth` 0, `innerWidth` the last).
- *Synchronous `about:blank`.* `scripts::after_insertion` also finds `iframe`
  elements (one walk with the script elements), and `frames::connected` makes the
  frame inside the native; with no document to load, `load` fires there
  (measured, cases `about-blank`, `scripted-load-recursion`). A listener can add
  and remove frames again: the limits of frames and depth end it (hostile cases
  `scripted-frame-append-in-load-js-on`, `scripted-frame-nest-in-load-js-on`).
  `html::focus::before_removal`, which every removal calls, destroys the frames
  in the subtree, so a move makes a new frame inside the call (measured: `load`
  fires again, a new window). The parser's frames come after its step and before
  the next script (`EngineHost::take_inserted_frames`): `load` of a parser's
  `about:blank` frame comes before the next inline script (measured; Chromium
  fires it at the insertion, so a capture listener on the document sees an order
  that swb cannot give). With a runtime the journal's `removed` skips an element
  that is back in the tree. Without a runtime (JavaScript off) nothing changes.
- *Initial `about:blank` reuse* (HTML "initial about:blank"; measured, case
  `scripted-initial-reuse`): a navigation that the insertion started (`src` or
  `srcdoc` at the insertion) to a document of the same origin keeps the `Window`:
  the expando, the listeners, the timers and the interfaces stay, and the window
  gets `load`. A frame without such an attribute at the insertion had its `load`
  already, and a later navigation makes a new `Window` (measured, also for
  `src = 'about:blank'`). `state::reuse_window`: a new record of the same realm
  takes what belongs to the window; the old record keeps the wrappers of the old
  nodes, retires, and forwards queued tasks.
- *Retired documents* (`state::retire_document`, `DocumentState::retire_document`).
  When a frame is destroyed or its document replaced, a document with a realm
  stays in the runtime: its record holds the host for the rest of the runtime's
  life, because script can hold its nodes, and realms are roots. Decided from the
  measurements (case `scripted-retired`): nodes read and write; `closed` true;
  `parent`, `top`, `frameElement` `null`; `length` 0; `name` empty; `defaultView`
  `null`; `readyState` `complete`, `visibilityState` `hidden`; `setTimeout`,
  `setInterval`, `requestAnimationFrame` return a number and never run
  (`initialize` adds nothing, so a loop costs no memory); microtasks run; messages
  are dropped; `fetch` rejects with a `TypeError`; geometry reads give 0 and `""`;
  `location` writes do nothing without an error; no script element runs; no frame
  starts. A queued task of a retired document is dropped and does not count as
  run. What is freed at once: requests, parser, stylist, styles, layout, display
  list, images, timers, callbacks, observers. What stays: the DOM, the wrappers,
  the listeners, and the charge of its nodes in the page's budget until the next
  top-level document (the memory is real). The entry of the frame in
  `window_proxies` goes when the frame is destroyed: nothing can ask for that
  proxy again, and scripts that hold it keep it alive. Realm indexes stay in
  `HostSlot` and `realm_docs`, so a realm is never reused. Departures: the
  interfaces of a retired window stay (`w.Event`, `w.XMLHttpRequest`, `w.Image` are
  constructors; Chromium has no such property once the frame is gone), and
  `document.open` on a retired document does nothing (feature 15).
- *A frame that removes its own `iframe`* (case `scripted-self-remove`). The
  running script goes on; the parse ends there (no later script, no
  `DOMContentLoaded`, no `load`); its timer does not run. Departure: Chromium fires
  the parent's `load` inside `remove()` when that frame was the last that delayed
  it; swb fires it after the task, because the check of the load needs the page.
- *The load path.* A frame document with a script goes through the top-level
  code: `start_document`, `parse_steps`, `script_fetched`, the parser tasks,
  `fire_load_event`, under `Page::run_task_in`; `deliver_scripts_in`,
  `deliver_api_fetches_in` and `deliver_element_events_in` are the page's paths
  with a frame argument. `load` of the window comes first, then the container's
  `load` in the same task, then `pageshow` (measured, case
  `scripted-frame-document`). `Page::check_frame_loads` fires `load` for a document
  that the runtime drives, and the container's for one that it does not.
  `pagehide` and `unload` fire at a commit and at a destruction, children first.
  A limit that ends the task of a frame's `load` leaves the container's `load` to
  come; `check_frame_loads` does it at once, because nothing else may wake the page
  (`finish_load_after_limit`).
- *Navigations by script.* `location` writes check `FrameHost::may_navigate` with
  the caller's frame (`Runtime::caller_realm`) and throw Chromium's `SecurityError`. A
  sandboxed frame needs `allow-top-navigation` (`Sandbox::script_top_navigation_allowed`);
  `allow-top-navigation-by-user-activation` refuses a script without an activation
  (measured; with one: "Done (feature 12b2)"). `form.submit()` in a sandbox needs `allow-forms`. A `javascript:`
  URL in `src` runs as a task in the frame's realm; an empty one does nothing.
  The session history stays at the top level: a frame's `pushState` changes its
  document's URL and no entry.
- *Rendering update.* `Page::run_frame` runs the documents with a realm in tree
  order: `resize` (a viewport that changed after the first layout; measured),
  `scroll`, media queries, animation frame callbacks, then the observers after
  layout. A task that changed the DOM asks for a frame if a frame's window reacts
  to its viewport (`watches_viewport_in`: a `resize` listener or handler, a
  `MediaQueryList` with a listener).
- *Automation.* `page.evaluate` with a `frameId` and `--eval-frame` run in the
  frame; a console message has the number of its frame's document.
- *Sandbox.* Without `allow-scripts` the document parses to its end and gets a
  realm only at an access from a parent of the same origin. Then
  `DocState::scripts_off` stops what the document's own markup can start, as
  measured (case `scripted-sandbox-scripts`, a control frame with `allow-scripts`
  beside it): script elements, also appended ones (`scripts::prepare`); handler
  attributes (`b.onclick` reads `null`; `events::handlers::compile`); `javascript:`
  URLs in `src`, links, forms and `location`; `setTimeout` and `setInterval` of that
  window, also with a function of the parent (a number, and no run). What the parent
  calls through the window still runs in Chromium, and so here: `eval`, `Function`,
  a function of the frame's realm called directly, `onclick = f`, listeners,
  `requestAnimationFrame`, idle callbacks, observers, microtasks and messages. A
  child frame inherits the flags. A navigation has the flags of its start
  (`Frame::pending_sandbox`, HTML "snapshot target snapshot params"), not of its
  commit, in both directions (measured, case `scripted-sandbox-timing`): the page
  acts on a set of `src` or `srcdoc` after the task, so the journal drain gives it
  the flags of `sandbox` at the set (`FrameAttributeSet`, from the old value of a
  later change). A `sandbox` set after an insertion that started a navigation does
  not count for that navigation either. Departure: a frame without `src` or `srcdoc`
  at its insertion that gets `src` and then a new `sandbox` in that same task has
  the flags at the end of the task. Without `allow-same-origin` the origin is
  opaque, and the document has a runtime of its own ("Done (feature 13b)").
- *Cookies and storage of `about:` documents.* An `about:blank` or `about:srcdoc`
  frame document with its creator's origin has its creator's cookies and storage,
  also a nested one; a cookie with the `path` of the creator's URL shows, so the key
  is that URL, not the origin (measured, case `scripted-about-frame-state`;
  `DocumentState::cookie_url`, `EngineHost::storage_url`). A document with an opaque
  origin keeps its own URL, so it has neither.
- *Not in 12b1.* `document.referrer` of a frame document (feature 14: it is empty),
  `document.open` on a frame document (feature 15), cross-origin frames (features 13a and
  13b below).
- Costs: docs/performance.md, "Scripts in frames".

**Done (feature 12b2).** Nodes across documents, the caller's document, the
window of an error, and input with scripts in frames. Measured in Chromium 148 with
the cases `nodes-*`, `errors-*`, `caller-*`, `focus-by-script*`, `activation-*`,
`javascript-link-clicked-in-frame` and `window-name-target` of
`tools/probes/web-frames.json` (three equal runs; `errors-promises` differs between
runs only in the order of the two windows); every value is compared in
`crates/engine/tests/frames/nodes.rs` and `crates/automation/tests/frame_input.rs`,
and a departure has a row there.

- *The transplant* (`swb_dom::Document::take_subtree`, `put_subtree`; `web/dom/adopt.rs`).
  A move into another arena checks the room of the target first, runs the removal
  steps in the source document (frames in the subtree go, a focused element gets
  `blur`; a script can run, and the move goes on from where the node is then), takes
  the subtree (iterative, with template contents), and puts a copy into the target.
  A taken node stays in the old arena as an empty detached comment, because a `NodeId`
  stays valid for the life of an arena: what still holds the old id reads nothing of the
  moved node. So a move adds nodes to the target arena; a subtree that moves back and
  forth in a loop ends at the node limit of the arena (a departure: Chromium has no
  such end; no target does this). Rejected: a free list of ids (every structure that
  holds a `NodeId` would have to know that an id came back into use). The room is
  checked before the removal steps, so a move that cannot fit leaves the node in its
  place, and again right before the take, because a script in those steps can fill
  the arena; from that check to the put no script runs, and the check and the put
  use one formula (`SubtreeSize`, with template contents and inert documents). So a
  subtree is never out of both arenas: a move that does not fit ends the task and
  leaves the subtree whole in its document.
- *The wrappers* (`state::move_node_state`). Each wrapper of a moved node gets its new
  word `(key << 32) | index` (`Runtime::set_host_word`) and stays the same object, with
  expandos, listeners, handlers and `[SameObject]` objects. Tables keyed by node
  (`side`, `listeners`, `handlers`, `attr_objects`, the script state, `decodes`,
  `indeterminate`, `output_defaults`) go only for nodes with a wrapper: without one a
  node has nothing in them, except the script state, which goes for every node.
- *What holds an old id.* `DocState::moved` maps each taken id to its new place.
  `wrap_node` follows it, so a static list and a mutation record read the moved node
  (measured). An object with generic data that names a node (`NodeList`,
  `HTMLCollection`, `DOMTokenList`, `DOMStringMap`, `NamedNodeMap`, `Attr`,
  `CSSStyleDeclaration`) has the document and the node in its data
  (`GenericData::host_word`), and the entry of a native settles them through `moved`
  at its first use (`state::settle_generic`). So no walk of the heap and no registry of
  lists is needed, and a live list of a moved root stays live (measured). A node that
  moved many times has a chain in `moved`; the first read of a stale id walks it and
  points each id on the way at the place now (`state::settled`). The objects
  whose ids mean one arena for good (static lists, `MutationRecord`, sheets, observer
  entries) name the document node of their document. This also closes a gap of 12b1: a
  list of an initial `about:blank` document no longer reads the arena of the document
  that took over its window, which could panic on an index out of range.
- *The realm of what a native makes* (measured, cases `nodes-move-realms`,
  `nodes-adopt-import`). A moved wrapper keeps its prototype, and everything made through
  it is of its realm, not of its document's and not of the called function's:
  `e.firstChild`, `e.childNodes`, `e.cloneNode()`, `w.Element.prototype.querySelectorAll.call(e)`.
  `fd.importNode(x)` gives objects of the realm of `fd`. So `Cx::realm` is the realm of
  `this` (`WebState::receiver`, set at the entry only when it differs from the realm of the
  document). `getComputedStyle` gives a declaration of the window's realm.
- *Mutation observers* (`mutation_registry.rs`). The registry belongs to the agent, as
  HTML's one microtask does; a place is `(document, node)`, each document has a sink
  that names its key, and a record names its document. A move rekeys the registrations
  and the transient ones of the moved nodes, and installs the sink of the target
  (measured: they go on working). `before_switch` went. One departure: a record of an
  observer of the frame's realm that the parent reads has the observer's realm; Chromium
  gives the parent's.
- *Script elements.* `already_started` goes with the element. A script whose fetch runs
  in the old document stays in that document's lists and ends there without running and
  without an event (measured, HTML "execute the script element" step 2). In a sandbox
  `prepare` sets `already_started`, so it does not run there or after a move back
  (measured).
- *Form controls.* The dirty value, the selection, the checkedness, the selectedness of
  an option and the custom validity go as `ControlSnapshot`s through `EngineHost`
  (`take_control_snapshots`, `put_control_snapshot`), and `importNode` copies them.
- *Style sheets.* The old document processes its changes before the nodes go, so a
  `style` element leaves with its sheet: the sheet object stays with `ownerNode` null,
  and the element has a new sheet in the new document (measured).
- *Handlers.* `WebState::handler_marker` is one marker per agent, so that the listener of a
  moved handler can be removed. A handler attribute that was not compiled compiles in
  the new document's realm (measured).

Node arguments. An IDL argument with `[SwbAnyDocument]` arrives as a `NodeRef` (the
wrapper, its document, its node); a plain one fails with `TypeError` for a node of
another document, so a new native cannot read the wrong arena by accident.

| Native | A node of another document |
| --- | --- |
| `appendChild`, `insertBefore`, `replaceChild` (node), `append`, `prepend`, `before`, `after`, `replaceWith`, `replaceChildren`, `insertAdjacentElement`, `adoptNode` | moves, after the checks in both arenas with Chromium's messages; a fragment stays in its document and gives its children (measured); `adoptNode` of a fragment moves it |
| `insertBefore`, `replaceChild`, `removeChild` (child) | `NotFoundError` (measured) |
| `document.body =`, `table.caption`, `tHead`, `tFoot`, `select.add`, `options.add`, `select[i] =`, `options[i] =` | moves (measured); `add` with a `before` of another document: `NotFoundError` |
| `importNode` | copies through `Subtree`, with the cloning steps |
| `contains` | false |
| `compareDocumentPosition` | `DISCONNECTED`, `IMPLEMENTATION_SPECIFIC` and an order by document key |
| `isSameNode` | compares the document and the node |
| `isEqualNode` | reads both arenas (`Document::is_equal_to`) |
| `getComputedStyle` | reads in the element's document |
| `MutationObserver.observe` | registers (agent registry) |
| `FormData(form)` | the entries of the form's document |
| `FormData(form, submitter)`, `requestSubmit(submitter)` | `NotFoundError`: not owned by the form (measured) |
| `SubmitEvent` `submitter`, `MouseEvent` and `FocusEvent` `relatedTarget` | hold the wrapper (measured) |
| `setAttributeNode`, `setAttributeNodeNS`, `setNamedItem`, `setNamedItemNS` | `InUseAttributeError` for an `Attr` with an owner in another document (measured); a detached one moves to the element |
| `removeAttributeNode` | `NotFoundError` (measured) |
| `dispatchEvent`, `addEventListener` and the other members | `this` chooses the document (feature 12a) |
| `IntersectionObserver.observe`, `unobserve`, `root`; `ResizeObserver.observe`, `unobserve` | departure: `TypeError`, and a target that moves away ends its observations (Chromium keeps them). An observer's state is per document, its implicit root would be the top level, and no target page observes across documents |
| `Range`, `Selection`, `NodeIterator`, `TreeWalker`, `adoptedStyleSheets` | do not exist in swb |

- *Departures.* `Attr.ownerDocument` does not exist (an `Attr` is no `Node`, ADR 0028
  section 3); the rows above. The rows of `nodes.rs` have the reasons.

The caller's document:

- *`postMessage`* (`messaging.rs`). `source` is the window of the code that called it, and
  `origin` that document's origin (`EngineHost::origin_text`, so a same-origin `about:`
  frame has its creator's). The caller is HTML's incumbent: measured, `t.send()` with
  `s.postMessage` inside gives `t`; `t.postMessage.call(window)` from the top gives the
  top. A target origin compares with the receiver's origin text.
- *URLs through another window* (measured, case `caller-urls`, with a `base` in the
  frame). `location.href =`, `assign` and `replace` resolve against the base URL of the
  **entry** document (the bottom of the stack: `Runtime::entry_realm`; also inside
  `w.eval`). `w.fetch`, `new w.XMLHttpRequest().open`, `new w.Image().src`,
  `history.pushState` and `replaceState` resolve against the window's own document, which
  the switch by `this` gives. `w.setTimeout('code')` runs in the frame. `window.open` and
  workers do not exist.
- *`javascript:` with a `target`* (`html/activation.rs`, `frames::script_target`, measured,
  case `caller-javascript-links`). A link or a form runs the script in the frame that the
  name means, under the rules of `may_navigate` and the target's scripts: `_parent`,
  `_top`, a sibling's name. A new context (`_blank`, a name without a frame) and a target
  without scripts run nothing. `may_have_script` also looks for `javascript:`, so a frame
  whose only script is a link has a realm. A link whose bytes hide the scheme
  (`java&#115;cript:`, a tab in the word) has no such sign; its click finds the URL and
  runs it in a realm made then (`Page::run_javascript_link`; measured, case
  `javascript-link-hidden-in-frame`). That path has the same rules, because a frame
  of another origin takes it too: the source and the target have one origin (HTML
  "navigate to a javascript: URL"), the source's sandbox allows scripts, and a new
  context or a modified click runs nothing (measured, cases
  `javascript-link-origin-of-source` and `javascript-link-origin-of-child`, with
  same-origin controls; the second is the one where only the rule of the origin
  stops the script, because a frame may navigate its own child). A departure stays:
  the string result does not replace the document (ADR 0028).

The window of an error:

- *Callbacks* (`errors::report_callback`). An exception goes to the window of the function's
  realm (Web IDL "invoke a callback function"; measured, case `errors-callbacks`): timers,
  listeners (for an object, its `handleEvent`), handlers, animation frames, idle callbacks,
  microtasks (`Runtime::take_failed_job_realm`), the three observers. `window.event` is
  also the event of the listener function's window (measured). A window whose frame is
  gone gets no event: its functions report nowhere (measured); the console line stays.
- *Rejections* (`Runtime::take_unhandled_rejections_by_realm`). `unhandledrejection` goes per
  promise. Measured (case `errors-promises`), neither rule of the brief alone gives
  Chromium's values. The realm of the promise gives them except for one path:
  `w.Promise.resolve().then(parentFn)` reports at the parent and
  `Promise.resolve().then(frameFn)` at the frame, while
  `Promise.prototype.then.call(w.Promise.resolve(), parentFn)` reports at the frame. This
  is what the current realm at the rejection gives when the derived promise is rejected
  directly, and a resolving function of `w.Promise` otherwise. swb has the same two
  paths (`Capability::Intrinsic`, `Capability::Record`): a reaction job that rejects an
  intrinsic capability after its handler threw takes the handler's realm; every other
  rejection takes the realm of the promise, which `rejection_realm` finds from its
  prototype chain, only with several realms. So `PromiseState` needs no field and a
  runtime with one realm pays one comparison. A retired window gets no event (measured).
  The events of one task come grouped by window, in the order of the rejections inside
  a window (measured, case `errors-promises-grouped`; the order of the windows varies
  in Chromium). Departure: the prototype chain is not the rule, only a way to find it
  that fits what pages do. After `Object.setPrototypeOf` to the `Promise.prototype` of
  another window, swb reports at that window and Chromium at the promise's own (measured
  in the review of 12b2); the exact rule needs a realm in `PromiseState`, and so a cost
  on each promise. `rejectionhandled` never fires (backlog; also without frames).

Input with scripts in frames (section 8):

- *The exchange stays.* `Page::in_frame` no longer takes the runtime out. A frame with a
  realm takes the paths of a page with scripts (`script_input`); one without takes the
  paths of JavaScript off. A task inside the exchange undoes it for its length
  (`Page::outside_exchange`), because the runtime and the registry see the documents in
  their own places. If a listener destroys the frame, the rest of the input finds no
  document (`doc` is `None`).
- *Pointer.* Moves, presses, releases, clicks and the wheel fire in the innermost
  document; the documents above see the boundary events of the `iframe`, as before.
  A frame that the pointer left gets its leave events before the documents above it
  get theirs (measured, case `input-leave-order`). A press that moves and selects
  nothing keeps its click at the common ancestor (measured, case
  `input-press-in-frame`; the rule holds for all documents). Departures, measured in
  the review of 12b2: `screenX` and `screenY` of an event in a frame are the frame's
  (Chromium: the page's); a second wheel event over a frame that reached its end goes
  to the frame (Chromium: to the parent).
- *Keys.* `key_down_events` and `key_up_here` run in the focused frame.
- *Focus* (`set_focus_frame`, `frames::focus_enters_current`; measured, cases `focus`,
  `input-tab-*`, `focus-by-script*`). The page's focus has one holder. When it moves, the
  old holder's element gets `blur` and its window `blur`, the new holder's window `focus`,
  then the element its `focus`; windows between them get nothing. A script's `focus()`
  does this inside the call, also from the other direction; `window.focus()` moves the
  focus to the window (new member), `window.blur()` does nothing (measured); `f.focus()`
  of an `iframe` moves it into the frame and fires `focus` at the element.
  `document.hasFocus()` of a frame document is true while its frame holds the focus or
  has a frame that does (`Frames::focused`). An `iframe` never matches `:focus` or
  `:focus-visible`; after `f.focus()` it and its ancestors match `:focus-within`, also
  while an element in the frame has the focus, and after a click or Tab into the frame
  they do not (measured, case `focus-iframe-then-inner`). Departures, measured in the
  review of 12b2: after `f.focus()` a click in another frame fires `blur` and `focusout`
  at `f`, and the parent's `activeElement` is the other frame (Chromium: no events, and
  `f`); Chromium ignores a focus change that a `blur` or `focus` listener asks for while
  a change runs, and swb carries it out, so extra events fire, and two windows whose
  `blur` listeners focus each other end at the depth limit after 818 events (Chromium:
  4); a `focus` listener that hides or moves the `iframe` does not stop the element's
  own `focus`.
- *User activation.* `allow-top-navigation-by-user-activation` allows a script's navigation
  for five seconds after a press or a key other than Escape (measured: in a listener, in a
  timer, after a click in a parent of the same origin, and cases `activation-escape-key`,
  `activation-other-key`). The activation is per frame (`Frames::note_activation`, "Done
  (feature 13b)"). Chromium's activation has more rules (consumption); no
  measured case needs them. Without consumption, one press allows any number of top-level
  navigations for five seconds, but the first replaces the page.
- *Fonts.* A face holds its charge by id (`WebFonts::admit_face`), and the charge returns
  when the face goes, so a face that loads again is charged once.
- *Tests.* `frame_input.rs` runs plain scripts; only the selection (no `getSelection`) and
  cross-origin frames (feature 13b) still go through the engine API.
- Costs: docs/performance.md, "Nodes across documents and input in frames".

**Done (feature 13a).** The rules behind the hooks: a frame of another origin has
real cross-origin objects. Measured in
Chromium 148 with the cases `cross-origin`, `nested-origins` and `xo-*` of
`tools/probes/web-frames.json` (three equal runs apart from ports); every value is
compared in `crates/engine/tests/frames/cross_origin.rs`, and a departure has a row
there. Where HTML and the measurements differ, swb follows the measurements; the
list below says where.

- *The hooks* (`crates/web/src/window_proxy.rs`). A proxy without a target comes
  here for each internal method. `[[GetPrototypeOf]]` gives `null`.
  `[[SetPrototypeOf]]` throws for every value, also `null` (HTML:
  `SetImmutablePrototype`). `[[Delete]]`, `[[DefineOwnProperty]]` and
  `[[PreventExtensions]]` throw. `[[OwnPropertyKeys]]` lists the indices of the child
  frames, the 14 names and the three symbols, and no names of frames, as HTML says.
  `[[Get]]` and `[[Set]]` ignore the receiver: `Object.create(w).closed`
  reads the frame's, and `q.location = u` with `w` in the chain of `q` navigates the
  frame. `[[HasProperty]]` is true for an index of a child and for the names and the
  three symbols; it throws for everything else, also for `then` and the name of a
  child, which `[[GetOwnProperty]]` and `[[Get]]` accept. `hasOwnProperty`,
  `Object.hasOwn` and `propertyIsEnumerable` have the answers and errors of
  `[[HasProperty]]`, `__lookupGetter__` and `__lookupSetter__` always throw, and
  `for`-`in` lists nothing (`Object.keys` gives the indices): small branches in the
  cold paths of `js`.
- *A named child* wins over `then` and over a name that is no cross-origin property,
  and loses to the other cross-origin properties: `w.then` is a frame named `then`,
  `w.location` stays the `Location`. An empty name finds nothing. A name that is an
  index is an index. `w.leafname` is the first child of that name; the descriptor of
  a named child is not enumerable, of an index enumerable.
- *Errors.* A `SecurityError` of the realm of the running code (a native runs in its
  function's realm, so `sw.Reflect.get(w, 'x')` gives one of `sw`). The prefixes are
  `Failed to read a named property 'k' from 'Window': `, `Failed to set a named
  property 'k' on 'Window': `, `Failed to read an indexed property [i] from 'Window':
  `, the same for `Location`, and none for an access that names no property. A symbol
  shows as `'[description]'`; the empty string, a symbol without a description and
  `@@toPrimitive` show no name. The text after the prefix has three forms:
  `Blocked a frame with origin "O" from accessing a cross-origin frame.`; when the
  caller's frame is gone, `Blocked a restricted frame with origin "O" from accessing
  another frame.`; and when the frame of the object is gone or the `Location` is stale,
  `An attempt was made to break through the security policy of the user agent.`
- *The functions.* One per (name, realm of the running code), so at most 16 per realm
  (`WebState::cross_origin_functions`, traced) and 4,096 in a runtime. `w.focus ===
  g.focus` for two frames (measured), and each differs from `window.focus`. Each is a
  native of the generated bindings, which the IDL marks: `[SwbCrossOrigin]` on the
  members and `[SwbCrossOriginSetter]` on `location` and `href` (ADR 0028). The native
  checks `this`: a platform object of the interface, or a cross-origin object of the
  interface, which passes with a word of its own in `Platform::data`; the member asks
  `glue::is_cross_origin_this` and acts on the object. So `w.close.call(window)`, `w.focus.call(sw)`,
  `window.focus.call(w)` and `Object.getOwnPropertyDescriptor(window,
  'closed').get.call(w)` behave as measured, and `undefined` means the window of the
  function's realm. A member that a cross-origin object does not have, called on one,
  throws the bare `SecurityError` in `this_platform` and `this_node`, on the path of a
  failed brand check, so that the path with a target pays nothing
  (`Document.prototype.title` as well as `Window`'s `name`: measured). A native that
  returns a promise also throws, on every interface measured (`fetch`, `scroll`,
  `Blob.text`, `Response.json`, `Element.scrollTo`, `decode`, `readText`), while a plain
  object as `this` gets a rejected promise (cases `xo-promise-natives` and
  `xo-promise-natives-others`).
- *The `Location`.* A second use of `ObjectKind::WindowProxy`: a proxy without a target
  whose frame id has bit 30 (`FrameId::number` is now below 2^30). So all the rules stay
  in the hooks, every path of the engine that treats a proxy as exotic works, and
  `ObjectKind` keeps its size. Rejected: a host object with hooks (its hooks do not
  cover the prototype and extensibility methods), and a new object kind. It is one
  object per frame (measured: the same object for callers of two realms, not one per
  caller realm), kept in the proxy (`WindowProxySlot` has room without a target,
  `Runtime::set_window_proxy_companion`): it dies with the proxy and needs no table.
  A new document of the frame drops it, also from one cross-origin document to the next
  (measured: `w.location !== l` after each commit). A kept one is stale: it stays
  cross-origin with its 3 names, its errors have the third text, and it navigates
  nothing, without an error.
- *How a navigation leaves the task.* The frame's document is not lent, so
  `window_proxy::navigate` calls `FrameHost::navigate_frame`, and the page starts the
  navigation in `after_frame_changes` (`AfterTask::navigations`: one per frame, the last
  request; a map by frame, so a loop of a million writes costs no memory). Rules, as
  measured: `href`, `replace`, `w.location =` (through `[PutForwards]`, so its errors
  name `href` of `Location`). `may_navigate` first, so a refusal hides a URL that does
  not parse. Then the URL against the entry document, a `SyntaxError` if it does not
  parse. A `javascript:` URL is the same `SecurityError` as a refusal (HTML "navigate to
  a javascript: URL" needs one origin) and runs nothing. A caller whose frame is gone
  does nothing, without an error. A navigation from another origin is always a full one
  (measured: a change of the fragment only fires `load`). A write to `location`, also
  of a same-origin frame, has the sandboxing flags of the last write, in both directions
  and under a sandboxed parent (`Frame::location_sandbox`; measured, cases
  `xo-navigate-flags-at-the-write` and `xo-navigate-flags-more`), though it starts after
  the task. The flags are parsed once per generation of the parent's DOM, so a loop of
  writes does not pay for a long attribute each time. Departure: a set of `src` or `srcdoc` after the write in the
  same task loses to the write (Chromium: the last one wins; also at the base for a
  same-origin frame; measured in the review of 13a). To follow it, the page would need
  the order of the journal's sets against the requests.
- *Values.* `closed` and `length` come from the frame tree. `window`, `self` and `frames`
  are the proxy, and `null` once the frame is gone (a same-origin window that is gone keeps
  them, measured; so the IDL says `WindowProxy?`). `top` and `parent` are proxies of this
  runtime, local when that frame is of this agent; `null` once the frame is gone. `opener`
  is `null`. `focus()` moves the page's focus to the frame inside the call (measured:
  `blur` at the window that held it, then `activeElement` is the `iframe`; no activation
  needed; also from a caller whose frame is gone). `blur()` does nothing. `close()` does
  nothing (new member, also on a same-origin window, because no script opens a window).
- *`postMessage`.* The generated native converts the arguments, and `post_to_remote`
  serializes and transfers: measured, the transfer list detaches before the check of the
  target origin fails, also for a same-origin target, so `messaging::post` has that order
  too. **The message goes nowhere until feature 14**, and a transferred port closes.
- *Inline caches.* A proxy without a target never hits `SiteCache::WindowProperty`, which
  names its target by identity, and has no entry of its own: each read goes to the hooks
  (case `xo-stale-sites`: one site across same-origin, other origin, same-origin, other
  origin, gone).
- *`frameElement`* is `null` when the container's document has no realm here, also for
  A > B > A' (case `xo-aba`). A' reads its parent B by the rules above, with its own `O`,
  and reads the top level directly.
- *Sandbox.* A frame with an opaque origin has the same object; a top-level navigation of
  it goes through the same path (case `xo-navigate-rules`).
- *The realm past the budget of 256* keeps a proxy without a target although its document
  has this origin, so the rules above apply with `O` the document's own, and `w.document`
  throws `Blocked a frame with origin "O" ...`. A departure that Chromium cannot show,
  because it has no budget (`a_frame_past_the_budget_of_realms_has_the_rules_of_another_origin`).
- *`document.domain` and `originAgentCluster`.* The cases load the document in a frame of
  another origin, which reports through `postMessage`. The test runs the same document as
  the top level at that host and compares what the frame reported in Chromium. The second
  half of `document-domain` (one frame reads the other): "Done (feature 13b)".
- *Departures* (rows of `cross_origin.rs`). A frame that a script renames through a
  same-origin path is found by its new name; Chromium, through a cross-origin parent,
  finds it by no name. `f.focus()` while the frame holds the focus changes nothing;
  Chromium's top window gets `focus` and `blur`. `null[w]` names the type of the key in
  the `TypeError`. `new Event('x', w)`: the message lacks the part about the dictionary.
  A URL with a host that the URL standard refuses (`http://a b/`): swb throws a
  `SyntaxError`; Chromium navigates to an error page (not in the probe, it would navigate
  the probe's top level in the same-origin control).
  Not of this feature: `DOMException` has no `stack`, `console.dir` and `console.table`
  do not exist, `Location` lacks `ancestorOrigins` and `valueOf`, `document.referrer` of
  a frame is empty (feature 14).
- *Memory.* All functions of a realm cost 11.4 KB of script heap (measured, with the atoms
  of their names), so at most 2.9 MiB at the budget of 256 realms, whatever the number of
  frames: 1,000 frames do not multiply them. A `Location` is an object without properties.
- Costs: docs/performance.md, "The cross-origin `Window` and `Location`".

**Done (feature 13b).** One script runtime per origin. Frames of another origin and
with an opaque origin run their scripts. Measured in Chromium 148 with the cases
`agents-*` and `xo-top-*` of `tools/probes/web-frames.json` (17 cases, three equal runs
apart from ports; two ports of one host share cookies, so `document.cookie` carries a
log between origins); every value is compared in `crates/engine/tests/frames/agents.rs`
and `crates/automation/tests/frame_input.rs`, and a departure has a row there.

- *The agents* (`engine/src/page/agents.rs`). `Page::script` stays the runtime of the
  top-level document, and `Agents::others` holds one per other origin of a frame document
  that has a script to run or a first need (an evaluation, a `javascript:` URL). A page
  without a cross-origin frame stays on today's path: `others` is empty, one comparison.
  `ScriptRuntime::for_frame` makes a runtime whose first document has the frame's number,
  so key 0 and realm 0 exist in each runtime; the host flag of "several documents" is on,
  because such a document's viewport and host come from the registry. An opaque origin is
  unique. A child of a sandboxed frame gets a new opaque origin and so a runtime of its
  own, also for `about:blank` and `about:srcdoc` (measured, case `agents-opaque-state`:
  `contentDocument` is `null`); the child without a sandbox, with a clone of its creator's
  origin, joins its runtime.
- *A runtime's life.* It is made at the first need. It goes with the top-level document,
  when it reaches the heap limit, and when no live frame has a document in it
  (`Page::sweep_agents`, at the next turn of the event loop): only tasks run a runtime, and
  the tasks of a retired document do not run. So the budget of 32 counts what can run, and
  a page that replaces a sandboxed frame does not run out of runtimes, each of whose
  documents has a new opaque origin. The nodes of its retired documents leave the page's
  budget with it.
- *The key stays with its runtime.* `Frame::script` is a `FrameScript`: an agent and a key,
  which give the key only for that agent. `Page::frame_runtime` is the way from a frame to
  a runtime and its key. An agent number is not used twice in a page.
- *The registry per task.* The page's frames share one registry, and `enter_registry` tells
  it the agent of the task. `document_key`, `take_document` (which also compares the key),
  `set_document_key`, `document_loading`, `in_this_agent`, `name`, `set_name`, `container`,
  `child_of_container`, `child_containers` and the writes of `set_focused_frame` answer for
  that agent: another agent's frame has no key and no document there, and the answer is
  "no document". The members that read the tree (`is_alive`, `parent`, `child_count`,
  `child_at`, `child_named`, `subtree`, `may_navigate`, `choose_frame`) answer for all
  frames, because the hooks of `window_proxy.rs` need them; the hooks did not change.
- *The top-level document in a task of another runtime* is in its frame record for the
  length of the task, as every other document of another runtime is in its record. So
  layout across documents (`engine_laid_out`, `update_viewports`, `note_location_flags`)
  finds it with no case of its own, and `take_document` does not give it. Rejected: a second
  lent slot in the runtime, which would give the runtime a document with no key.
- *Proxies.* Each runtime has its own `window_proxies`. A commit gives the frame's proxy a
  target in the runtime of the document's origin and none in the others. Destruction
  forgets the proxy in every runtime.
- *A task never runs a script of another runtime inside a native.* What must happen there
  waits in `AfterTask`, and the page runs it after the task: unloads, focus events, the
  container's `load`, navigations.
- *Budgets across runtimes* (section 11). The top level's runtime keeps a whole runtime's
  share of the page's heap and long tasks, and the other runtimes share the rest, so that
  frames cannot end the top level's scripts. The byte budgets that the heap limit does not
  count (Blob URLs, blobs held by waiting operations, waiting messages, waiting mutation
  records) have the same form (`crates/web/src/page_budget.rs`): one count for the top level's
  runtime, and one that the frames' runtimes share. A runtime that goes gives its part back,
  and the waiting mutation records of a runtime that stopped or went go at once, because the
  sinks of its documents outlive it.
- *The event loop* (`Page::run_due_tasks_across_agents`, section 6). A task's key is its due
  time and a sequence number from a counter that the runtimes of a page share
  (`AgentShared`). A runtime runs up to the first task of another one, with a checkpoint
  after each task and the checks of `load` between the slices. `next_task_time`, the jump of
  virtual time, the frames and the idle callbacks count all runtimes. The rendering update
  runs per document in tree order, whatever the runtime (measured, case
  `agents-animation-order`). The console buffer is shared too, so its messages keep their
  order and `frameId`.
- *Load* (measured, cases `agents-load-order`, `agents-load-nested`): frame `load`, the
  container's `load`, frame `pageshow`, as inside one runtime. `frames::container_load`
  finds a parent of another origin, and `pageshow` waits; the page fires the container's
  `load` in the parent's runtime and then `ScriptRuntime::fire_waiting_pageshow`. A script
  can observe the same order as in Chromium. A document that unloaded during its `load` shows
  no page: no `pageshow` after `unload`, also in one runtime. Departure: when a frame goes in
  such a `load`, Chromium fires the `load` of the documents around it and then, after the top
  level's `pageshow`, the `pagehide` and `unload`; swb ends them at the removal, as in one
  runtime (a review's cases; backlog).
- *Removal* (cases `agents-removal*`). The removal holds at once for the remover. A frame of
  another runtime has its `pagehide` and `unload` after the task, in its own runtime, a
  parent before its child (measured), then it retires. Departures: Chromium runs them inside
  `remove()`, so a log across origins differs; the frame is gone for its own unload (`closed`
  true, `parent` and `top` `null`; Chromium: false and the windows). In one runtime swb still
  fires children first, Chromium the parent first (backlog, row of `agents-removal-same`).
- *Navigation of the top level by a frame of another origin* (section 3). `navigate_frame`
  carries `replace`, and the page calls `location_navigate` for the top level (measured,
  cases `xo-top-click-href`, `xo-top-click-replace`: a click adds an entry, `replace` none).
  The activation is per frame (`Frames::note_activation`, HTML "activation notification"): a
  press or a key other than Escape in a document reaches its frame, the ancestors, and all
  frames of its origin (HTML says only descendants; Chromium also a sibling of that origin).
  A cross-origin frame may navigate the top level once it has one. It does not expire, it
  stays through a commit of the same origin, and a commit of another origin clears it (cases
  `activation-xo-*`, three equal runs). So a click in the parent, in blank space or in a
  sibling of another origin gives the frame nothing, and a click on a link with `target=_top`
  in the frame navigates (`input-link-cross-origin-top`). A sandbox's
  `allow-top-navigation-by-user-activation` token needs an activation of that frame within
  five seconds. **Departures**: Chromium 148 also allows the navigation while the top level has
  had no activation at all (cases `xo-top-fresh-early`, `xo-top-fresh-late`, cross-site, with
  a top level that a script navigated to, so the probe gives it none), also from a sandbox with
  the by-user-activation token. This is no effect of the test tools: the headless shell, headed
  Chromium on a virtual display, a run without Playwright and without CDP, and a run with the
  host names of two sites all navigate. swb refuses, by the owner's decision of 2026-11-01,
  because a page cannot defend itself from an ad frame. A click in a frame's initial
  `about:blank` document, then the first commit of another origin: Chromium keeps the
  activation, and swb clears it, on the safe side.
- *Focus* (cases `agents-focus`, `agents-input-click`). Input: `Page::set_focus_frame` runs
  each document's task in its own runtime, and the whole order matches Chromium. A script's
  focus across origins: `set_focused_frame` changes the documents of the agent and records
  the moves (at most 16, so a loop costs no memory); after the task the page replays them for
  the documents of other agents and fires their `blur` and `focus`. Departures: those events
  come after the caller's task, Chromium's inside the call; of 20 moves in one task Chromium
  shows 20 `focus` and `blur` pairs and swb 8, with the same end state.
- *Input.* `Page::in_frame`, `script_input`, `outside_exchange` and `run_task_in` follow the
  frame's runtime. The `c:` queries run, and `link_query` and `target_query` of
  `frame_input.rs` went.
- *`javascript:` URLs.* In `src` they run if the document that asks has the frame's origin,
  in the runtime of that origin.
- *An opaque origin* (measured, cases `agents-opaque-state`, `agents-data-state`,
  `agents-file-state`). What counts is the sandbox and not the origin, because `file:` and
  `data:` origins are opaque too: the commit records on the document that its sandbox has no
  `allow-same-origin`. Such a document throws `SecurityError` for `document.cookie` and
  `localStorage`, `sessionStorage` (`The document is sandboxed and lacks the 'allow-same-origin'
  flag.`, with the prefix of the attribute), also at a `data:` URL; `document.domain` is empty;
  `originAgentCluster` is true. A `data:` document without that sandbox keeps its own texts,
  and `originAgentCluster` is true there too. A frame of a `file:` page reads as its page does.
  Departure: `originAgentCluster` of a sandboxed frame of a `file:` page is false in Chromium.
- *The rows that report through `postMessage`* (`cross-origin` query 2, `xo-focus` query 1,
  `sandbox` query 0, the second half of `document-domain`): the scripts run, and the values
  reach the page only in feature 14. Until then `agents.rs` evaluates the same expressions
  in the frame and compares them with Chromium's values.
- *What feature 14 needs.* `post_to_remote` holds the record and hands it to the page as an
  `AfterTask` entry; `Task::Message` carries the id of its message; `AgentShared` is the place
  for a page-wide message queue, and `Agents` finds the target's runtime. The ports registry
  is per runtime (`WebState::ports`). `document.referrer` and storage keys of frames too.
- Costs: docs/performance.md, "One runtime per origin".

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
  of the page stops. The top level's runtime keeps its 30 s. Before each task
  another runtime's budget is the smaller of 30 s and what the other frames
  leave of the other 30 s (`Page::set_limits_for_task`).
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

**Done before frames (M9 feature 1, `crates/web/src/messaging.rs`;
`document.domain` and `originAgentCluster` in `domain.rs`).**
`window.postMessage` to the own window follows the steps above: the
`targetOrigin` check (`*`, `/`, an absolute URL; `SyntaxError` with
Chromium's message), `StructuredSerializeWithTransfer` (`ArrayBuffer`s
only; any other transferable is a `DataCloneError` until ports exist), a
task `Task::Message` on the runtime's queue, the origin check again in the
task, and a trusted `MessageEvent` at the window with `origin` (`"null"`
for an opaque origin), `source` the window itself, `ports` empty and
`lastEventId` empty. The waiting messages are a per-runtime queue
(`Messages`: at most 65,536 messages and 256 MiB of serialized bytes; a
call past a bound drops its message with one warning until the queue
is empty, and does not detach its transfer list; hostile case
`post-message-flood-to-self-js-on`). `Task::Message` has no payload: the
task takes the oldest message of its runtime's queue. Feature 6 of
section 14 extends these functions: `post` gets a target window (a frame
proxy) and a source `WindowProxy`, the queue moves to the page, and the
ports travel in the transfer list. With a page-wide queue,
`Task::Message` carries the id of its message, so that each runtime
takes its own message and the order does not depend on how the event
loop interleaves the runtimes' tasks. `document.domain` returns the host of
the origin; its setter runs the checks of the standard and returns (the
origin-keyed agent cluster), with Chromium's messages; `originAgentCluster`
is true for a tuple origin. Probe cases: `tools/probes/web-small-gaps.json`
(`document-domain-*`, `post-message-self-*`; the frame cases stay in
`web-frames.json`).

**Done in one document (M9 feature 9, `crates/web/src/message_port.rs`;
probe `tools/probes/web-message-channel.json`).** `MessageChannel` and
`MessagePort` work within the runtime of the document; the registry is
per runtime, which is per page until feature 5 (runtimes per origin)
moves it to the page. Design:

- *Registry.* Each port has an id (a counter that never reuses a value)
  and an entry in the registry: the entangled port's id, the queue of
  waiting messages (`Serialized` records), the enabled flag, the realm,
  the port object (`None` while the port travels), and the number of
  tasks in the event loop. The port object is a host object of its realm
  whose `EventTarget` data holds `PortState` (the id and a share of the
  registry). A closed port has no entry.
- *Transfer.* `structured.rs` takes `MessagePort`s of the transfer list as
  items of the record (`Item::Port`, `Serialized::ports`); the sender's
  object is neutered (`PortState::detached`; every call does nothing) and
  the entry keeps the queue, so messages queued before the transfer move
  with the port; the receiving realm adopts the id with a new object whose
  queue is not enabled, whatever the sender's state was. A record that is
  not delivered (a window message dropped by the origin check, a message to
  a port without partner, a clone that fails) closes its ports.
- *Lifetime.* A port holds its entangled port strongly (the rule of HTML
  "Ports and garbage collection"; `PortState::trace`), so a pair that
  script no longer reaches is collected together (Chromium collects such
  pairs too, also with listeners on both ends; measured by
  finalization). The collector drops the `PortState` of a freed object;
  the drop puts the id on a list and the next operation of the registry
  removes the entry with its queue, disentangles the partner and closes
  the ports that travel in the dropped messages (host-object
  finalization through `Drop` of the generic data, not a weak table; no
  change in `js`). A port stays alive while it has a task in the event
  loop, and while it has a listener and its entangled port travels in a
  message that waits (the entangled port is then not in the heap).
  `close()` removes the entry at once. A port that travels (no object)
  stays only while something can still deliver it: a *sweep* removes the
  travelling entries that no root reaches, with their queues and their
  budget. The roots are the entries with a port object or a queued task,
  and the ports in the records of the window queue; the sweep follows the
  ports in the queued records of the reached entries (one pass, a
  worklist, no recursion). So a port that travels only in the queue of a
  travelling port (`c.port1.postMessage(1, [c.port2])`: the queue of
  `port2` holds `port2`; two channels that send each other a port) is
  freed, as in Chromium. The sweep runs where no record is in flight: at
  `new MessageChannel()` and `close()` when the removals since the last
  sweep are a quarter of the entries (constant cost for each removal), at
  the forced collection below (before and after it), and in the test
  accessor. Root rule of the collector (`Shared::trace`): a port with a
  listener whose partner travels stays alive, because the partner could
  send to it. If the partner sits in an unreachable cycle, the sweep
  removes the partner (the first collection leaves the port, since it is a
  root, but the sweep that follows disentangles it), and the next collection
  frees the port: at most two collections, each followed by a sweep; one
  if a sweep ran before. At the limit of 65,536 live ports (or when 65,535
  messages wait) `new MessageChannel()` forces a collection and a sweep (at
  most once per 1,024 channels), counts again and throws `RangeError: ...
  Too many message ports (65536).` with one warning per episode (until a
  channel fits again); so a page that makes a channel per call (a
  `setImmediate` polyfill) never reaches the limit while the collector
  frees the pairs (hostile case `message-channel-million-unkept-js-on`:
  1,000,000 channels in 1.1 s, 164 MiB), and a page that sends ports
  through their own queues does not either (hostile case
  `message-channel-travelling-cycle-js-on`: 70,000 iterations).
- *Tasks.* Chromium delivers a port's messages one per task, and queues
  the task for the next message after the listener of the previous one has
  returned (measured: a timer or a window message that the handler of
  message 1 queues runs before message 2, also when the handler posted
  message 3 first; port messages are not queued as a block at the time of
  the calls: with the timers 0 ms `t0` and `t0b` around `m1`, `m2`, `m3`
  the order is `t0 m1 win t0b m2 m3`). `Task::PortMessage(id)` follows this
  with one task for each port at most. There is no timer clamping (1,000
  chained port messages in 50 ms against 19 chained timers). The queue
  orders tasks by queuing time, so timers and the frames of the engine's
  loop get their turns in an endless ping-pong (hostile case
  `message-channel-ping-pong-js-on`: 750,000 round trips, 100 ticks of a
  10 ms interval and 58 frames in the 1.5 s of `--settle-ms`).
- *Queue enabled.* The assignment of `onmessage` or `onmessageerror` with
  any value (`null` and `5` too) enables the queue; so does `start()`;
  `addEventListener` does not (all measured). Messages posted to a port
  before it is enabled wait.
- *Closed and neutered ports.* `postMessage()` on a closed or neutered port
  returns before the checks and before the transfer (the buffer stays
  attached: measured); on a port whose partner is closed it serializes
  and transfers, and the message is dropped. A message that was queued
  before `close()` of the sender is delivered; `close()` of the receiver
  drops its queue.
- *Errors.* `Port at index N contains the source port.`, `Message port at
  index N is a duplicate of an earlier port.`, `Port at index N is already
  neutered.`, and `A MessagePort could not be cloned because it was not
  transferred.` with the prefix of the caller (`MessagePort`, `Window`,
  `structuredClone`, `History`). Chromium checks for non-transferable
  values and duplicates first, and for detached buffers and neutered ports
  second (`[b, b]` with a detached `b` is a duplicate at index 1).
- *`MessageEvent`.* `ports` is one frozen array, made at the first read and
  returned at each read (measured; the IDL says `object`); the init
  dictionary and `initMessageEvent` take `sequence<MessagePort>` (a copy:
  later changes of the array do not show); `source` may be a `MessagePort`,
  and an `EventTarget` that is neither a window nor a port is refused
  with Chromium's message; a port message has `origin` `""` and `source`
  `null`.
- *Budget.* The messages of ports and of `window.postMessage()` count
  together: at most 65,536 waiting messages and 256 MiB of serialized bytes
  for the runtime; a message past a bound is dropped with one warning until
  nothing waits (hostile cases `message-port-flood-without-listener-js-on`,
  `post-message-flood-to-self-js-on`).
- *Departures.* The limits above (Chromium has none); no `BroadcastChannel`;
  `messageerror` fires only when deserialization fails (only a heap limit
  does today).

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

**Done (feature 11: `page/frame_input.rs`, `frame_focus.rs`,
`frame_navigate.rs`; `DisplayList::hit_test_with`).** Measured in Chromium
148 with the cases `focus`, `wheel-scroll` and the 77 cases `input-*` of
`tools/probes/web-frames.json` (three equal runs; the values are in
`engine/tests/data/web-frames.json`, the test is
`crates/automation/tests/frame_input.rs`). Frame documents still run no
scripts, so the page's scripts see what they see in Chromium and the frames'
own events wait for feature 12 (the queries that read what a frame's scripts
logged are listed as `c:` entries and skipped by the test). Decisions and
departures:

- *The active document.* The input code of a document (`input.rs`,
  `forms.rs`, `scroll.rs`: hover, press, drag, activation, text editing,
  keys, selection, scrolling) acts on `Page::doc`. Input in a frame runs that
  same code with the frame's document in that place: `Page::in_frame`
  exchanges the document, the pointer and keyboard state of the frame
  (`Page::frame_inputs`), the viewport and the font faces of the frame with
  the page's, runs the code and undoes the exchange. A frame without a realm
  takes the paths of a page without JavaScript. The exchange is not nested: the routing (hit test, focus chain, scroll
  chain) runs in the page's own context and calls it once per document on
  the path. What changes the page (a navigation) is deferred until the
  exchange is undone. Reason: the input code is large and per document;
  the alternative, a `FrameId` argument on every function, touches hundreds
  of calls. Feature 12b2 keeps the runtime in place during the exchange
  ("Done (feature 12b2)").
- *Hit test.* `DisplayItem::Frame` has `interactive` (false for
  `pointer-events: none`). `DisplayList::hit_test_with` reports the topmost of
  the regions and the frames: a `Hit::Frame` has the point and the content
  box in the coordinates of the groups, so the transforms and clips of the
  `iframe` (the rounded one is a `PushSvgClip` around the item) apply. The
  engine maps the point to the frame's viewport (minus the corner of the
  content box snapped to a device pixel, as the raster draws it) and goes
  on in the frame's list (`Page::route_at`; `HitResult::frame`). A point on the
  border hits the `iframe`; `visibility: hidden` has no item; a later region
  above the frame wins; `opacity: 0` does not matter (all measured, cases
  `input-hit-*`). One `HitWork` budget covers the whole test, and the nesting
  limit (32) bounds the depth. A frame's layout is brought up to date for its
  viewport before its list is tested.
- *Pointer.* The parent's document sees the pointer on the `iframe`: it
  fires `pointerout`, `pointerover`, `pointerenter` and the mouse versions on
  entering and leaving it, in the order of other elements, and no
  `pointermove` or `mousemove` for the `iframe`'s content (measured; during a
  press that started in the parent, the move that crosses the edge fires them
  once). Hover is per document: the `iframe` and its ancestors match
  `:hover` and `:active` in the parent, the elements under the pointer in the
  frame; leaving a frame clears its hover (also the pointer leaving the
  page). The cursor and the hovered link come from the innermost document.
  A press of the primary button in a frame keeps the pointer in that frame
  until the release (`FramePress`: the frame sees the point minus the offset
  of the press; outside its viewport the frame's `html` is hovered and a
  selection reaches the nearest text), the parent sees nothing more; a
  press that started in the parent never goes into a frame, and a release
  over a frame after it fires nothing in the parent (measured; Chromium
  gives the frame a `mouseup` there, swb's frame has no listener).
  Other buttons do nothing in frames.
- *Links and forms.* A link or form navigates the browsing context of its
  `target` (HTML "rules for choosing a navigable"): none and `_self` give the
  source's own frame, `_parent` the parent, `_top` the top level, a name the
  first frame in tree order that has it and that the source may navigate. A
  name is compared exactly, the keywords without ASCII case. `_blank` and a
  name that no frame has give the source's own frame (swb has one tab;
  Chromium opens a new tab and leaves the frame as it is), and nothing in a
  sandbox without `allow-popups`. Rules for what a frame may navigate, all
  measured (cases `input-target-*`): itself and its descendants, always; from
  a sandbox (the flags go down to frames without `sandbox`), else only the top
  level with `allow-top-navigation` or
  `allow-top-navigation-by-user-activation`, also from a nested frame and with
  an opaque origin, so `_parent` to a frame that is not the top level is
  refused; without a sandbox, a frame whose document, or the document of one of
  whose ancestors, has the source's origin. So a frame with the top level's
  origin may navigate any frame, and a cross-origin frame may not navigate its
  parent, unless that is the top level just after a user's activation ("Done
  (feature 13b)"). A form in a sandbox without `allow-forms` does not submit.
  What feature 12 must add: the by-user-activation token needs an activation
  when a script asks, and a script's `submit()` needs the forms check. The
  `target` of a link
  or form is read as HTML says (the `formtarget` of the submitter, the
  `target` of the form, the `base` element's) and travels with scripted
  `click()` and `submit()` too (`ScriptNavigation`). The parent's own links
  and forms can name a frame. A fragment link in a frame scrolls the frame
  and gives its document the URL with the fragment; the other navigations
  replace the frame's entry (`POST` bodies included), under the navigation
  limit of 64 per 10 s.
- *Wheel and scroll keys.* The chain is the same function in the frame and
  in the page: the innermost scroller under the point, then the document's
  viewport (not for `scrolling=no`, also `off` and `noscroll`), then the
  parent's chain from the `iframe`. A wheel that reaches the end of the
  frame stops there; the next one scrolls the parent (measured, also for two
  levels). The parent gets no `wheel` event for a wheel over a frame. A
  scrolling key acts on the focused document and chains up the same way (a
  frame at its end, or with `scrolling=no`, hands it to the parent;
  measured). A scroll of a frame builds no new list for the parent. The lazy
  images of frame documents (JavaScript on) load when the frame's viewport
  comes near them: the lazy check of the frame loop runs for the frame
  documents too (it ran for the page's document only; a fix of this
  feature).
- *Focus.* Each document has its focused element. When a frame holds the
  page's focus, `InputState::focus_frame` names the innermost frame, the
  documents above it have no focused element, and `DocumentState::frame_focus`
  is the `iframe` that holds the focus: `document.activeElement` is the
  `iframe`, and `:focus` does not match it (measured). `document.hasFocus()`
  is true in the page and in the frame; it is false after Tab left the page
  and after the focused frame was removed (measured; the engine tells,
  `EngineHost::window_has_focus`). Moving the focus into a frame gives the page's
  focused element a `blur` and the window a `blur`, without a `focus` event
  on the `iframe`; the focus coming back to the page gives the window a
  `focus` and the element a `focus` (measured order, ADR 0009). A document
  loses its selection when the focus enters one of its frames; a frame that
  loses the focus keeps its selection, also to another frame (measured, cases
  `input-select-*`). A frame that is navigated keeps the focus,
  with no focused element in its new document; a frame that is hidden keeps
  it; a removed one gives it to its parent, and the window has none until the
  next focus (measured). The same holds when the page's `blur` listener removes
  the frame that a click or a Tab is moving the focus into (measured). Departure:
  after the removal, Tab starts at the first element, Chromium starts where the
  `iframe` was.
- *Tab.* Sequential navigation treats a frame as its content: the order of
  the parent contains the `iframe` (with its `tabindex`, so a positive one
  comes first and `-1` skips the frame and its content; a hidden frame is
  skipped); a Tab that reaches it enters the frame at its first (last, for
  Shift+Tab) focusable element, recursively; after the frame's last element
  it goes on after the `iframe` in the parent; after the last element of the
  page the focus leaves the page (all measured, nested frames too). A frame
  without a focusable element is a stop of its own: its document is
  focused with no element, the next Tab goes on. The page scrolls to the
  `iframe` of the focused element (departure: the whole `iframe`, not the
  element's rectangle). `rendered` is computed once per step (a page of 1,000
  frames took 55 ms per Tab with a test per element).
- *Keys and selection.* Keys go to the focused document: typing, Enter (the
  default button of a form, a link), Space, the arrows in selects and
  radio groups, Escape, Ctrl+A. The selection is per document. A double and
  a triple click select a word and a paragraph in the frame; a drag stays
  in the document where it started; `Page::selected_text`, `select_all`,
  `cut_selection`, `insert_text` and `has_editable_focus` act on the focused
  document, so copy and paste use it.
- *Departures and what waits.* The transforms of an `iframe` are followed by
  the hit test, but not by the offset that a press keeps or by
  `Page::frame_origin` (automation points); the offset also stays when the
  parent scrolls or changes its layout during the press, so a drag is off by
  that distance. `:focus-within` of the ancestors
  of a frame that holds the focus is not set. The keyboard modality of
  `:focus-visible` is per document. The events of a frame's own scripts, a
  `javascript:` link in a frame, the `focus()` of a frame's element from a
  script and `hasFocus` of a frame document: "Done (feature 12b2)".

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

**Done (feature 11).** `page.frames` lists the frames in tree order (the
top-level frame first, `frameId` 0). `frameId` goes to `dom.*`,
`page.evaluate`, `page.consoleMessages` and the input methods that take a
node or a point (`input.click`, `mouseMove`, `mouseDown`, `mouseUp`): the
call runs in the frame's context (`Page::with_frame`, the same exchange as
the input code of section 8), node IDs, boxes and coordinates are the
frame's, and input is sent to the page at the point in the page's viewport
(`Page::frame_origin`, which does not follow transforms). `dom.boxes` dumps
the frame's document. `page.evaluate` with a `frameId` runs in the frame
(feature 12b1), and every message of `page.consoleMessages` has the `frameId`
of its document. A `frameId` on another method is an invalid
parameter. `swb --eval-frame=PATH` (in order with the `--eval` arguments
after it; an empty PATH goes back to the top; the path is `SEL >> SEL`, the
first match of each selector must be an `iframe` with a frame) gives an
error line `FrameError: ...` for a path that finds no frame and for a frame of
another origin (feature 12b1 evaluates in a frame of the page's origin). The probe tool takes `"frame": "SEL >> SEL"` in a `js`
query (Chromium: the frame of `frame_locator`; swb: `--eval-frame`) and in a
point step with a selector (`click`, `dblclick`, `rightclick`, `middleclick`,
`hover`: the selector is looked up in the frame, the click goes to the
point in the page's viewport); `swbtools replay` shares the step format and
has no further option (clicks by coordinates reach frames without it).
`swb_automation::resolve_frame` is the one implementation of the path.

### 11. Budgets and robustness

| Resource | Limit | Result |
|---|---|---|
| Frames per page | 1,000 (Chromium 148, measured: the 1,001st `iframe` has `contentWindow` `null`) | the `iframe` gets no frame (`contentWindow` `null`), a warning |
| Nesting depth of frames | 32 (Chromium has no own limit below its frame count; 300 measured) | the frame is not created, a warning. Bounds the recursion of paint, raster and hit testing |
| A URL that two ancestors' documents have | refused (Chromium, measured) | the frame stays `about:blank` |
| Navigations of one frame | 64 per 10 s of page time (a frame that reloads itself, `meta refresh 0`) | later navigations are dropped with one warning |
| Runtimes (origins with scripts) per page | 32, counting those with a live document (`MAX_AGENTS`) | a document of a further origin runs without scripts (scripting flag on, no runtime, as after a heap-limit stop), a warning |
| Realms created per runtime over its life | 256 | the document runs without scripts, a warning (until realm collection exists) |
| JavaScript heap | 256 MiB per runtime, 512 MiB for all runtimes of a page | the top level's runtime keeps its 256 MiB, and frames cannot take it. Before each task another runtime's limit is the smaller of 256 MiB and what the other frames leave of the rest. Past it the task ends (termination) and the runtime is dropped alone: its frames go on without scripts, and its origin gets no new runtime until the next top-level document. Consequence: a frame whose limit fell below what it holds, because other frames grew, goes at its next allocation. After the top level's runtime the top-level document stays as it is, with the input of a page without JavaScript, and the other runtimes go on and have its share |
| Long tasks | 30 s per runtime, 60 s per page | the top level's runtime keeps its 30 s, and the other runtimes share the rest. A runtime stops at its limit; past the page cap all runtimes stop |
| Messages that wait (window and port) | 65,536 messages per runtime; 256 MiB of serialized bytes for the top level's runtime, and 256 MiB for the frames' runtimes together | `postMessage` drops the message with one warning per task (Chromium has no limit) |
| Blob URLs | 16,384 per runtime; 256 MiB of bytes for the top level's runtime, and 256 MiB for the frames' runtimes together | `createObjectURL` throws `QuotaExceededError`, a warning |
| Blobs that waiting requests and reads hold | 256 MiB for the top level's runtime, and 256 MiB for the frames' runtimes together | the request or the read fails, a warning |
| Mutation records that wait | 131,072 per runtime; 128 MiB for the top level's runtime, and 128 MiB for the frames' runtimes together | later records are dropped, a warning |
| One message | the structured clone limits of ADR 0028 section 16 | `DataCloneError` or `RangeError` as now |
| Message ports per runtime | 65,536 | `new MessageChannel()` throws `RangeError`, a warning |
| Frame viewport | the viewport limits (`MAX_VIEWPORT_SIDE`) | clamped |
| Source text of frame documents that are alive together, per page | 64 MiB (`MAX_SOURCE_BYTES`): the `srcdoc` text and the bodies of network documents. A document that replaces another pays the difference; a destroyed or navigated frame returns its bytes | checked before the parse; the document becomes an error document (it costs nothing), one warning per top-level document. The frame still exists and loads, so the load state ends |
| DOM nodes of frame documents that are alive together, per page | 1 Mi (`MAX_NODES`, as many as the parser accepts for one document) | the parse of a frame document stops one node past the room that is left; a document that does not fit becomes an error document. After a rejection the parse of the next documents stops at 16 Ki nodes until 64 Ki nodes have gone since the last rejection, so that 1,000 documents that are just too big do not cost 1,000 parses. A document that reaches this limit also shows the error document, never its first nodes; so on a page that has passed the budget, a frame document of more than 16 Ki nodes fails even if it would fit |
| Response bodies of frame documents (documents and their subresources) that the workers have read and the page has not taken, per page | 64 MiB (a `Reservation` of the request in a `ByteBudget`, as for `fetch()`) | the request fails (`OverBudget`): an error document, a failed image or style sheet. Without it the workers outrun the page: 1,000 frames of 10 MiB filled 4.4 GiB in 0.8 s |
| Decoded raster images of frame documents, per page | 512 MiB (`MAX_IMAGE_BYTES`, 4 bytes per pixel, charged from the header before the decode) | the image fails with a warning. One frame with 50 PNGs of 16 Mpx took 3.2 GiB |
| External style sheets of frame documents, per page | 8 MiB of CSS text (`MAX_STYLE_BYTES`) | the sheet fails with a warning. A sheet of 13 MB (1,000,000 rules) took 1.46 GiB parsed |
| SVG images of frame documents, per page | they count against the 512 MiB of `MAX_IMAGE_BYTES` with 8 times their source (`SVG_MEMORY_FACTOR`; an image of 7.4 MB took about 50 MiB) | the image fails with a warning, before the decode if the source alone does not fit |
| Renderings of SVG images of frame documents, per page | one `VectorCache` of 128 MiB for all frame documents; the top-level document keeps its own. The rendering budget of a rasterization (`MAX_FRAME_WORK`) covers all documents | the least recently used renderings go; past the rendering budget an image uses a rendering of another size or is not drawn, with one warning |
| Decoded web fonts of frame documents, per page | 256 MiB (`MAX_FONT_BYTES`): the decoded files of each document, 16 KiB per face that loads, 1 MiB per `local()` face | the font fails with one warning per top-level document, and the text uses the next family |
| Items of the display lists of the frames that a strip draws | 4 Mi (`MAX_FRAME_ITEMS`; frames outside the visible area do not count). 2.5 million items took 0.52 s | the frame that does not fit paints nothing; one warning per rasterization |
| Transform groups nested in frames | 1,024 over a chain of frames (`MAX_FRAME_GROUP_DEPTH`), so that the recursion of the rasterizer stays bounded; 32 frames with 256 nested boxes each would give 8,192 | the deeper group is not drawn, with the warning of the item budget |
| Hit testing | the 40 ms budget per call covers all frames | as now |

Chromium has none of the budgets for frame documents. They are departures
that a large page can see: about 10 photos of 4000×3000 px across the frames
of a page fill the 512 MiB of decoded images, and later images fail. The
frame documents of the Ars Technica and BBC fixtures are about four orders
of magnitude below the budgets (largest document 5,399 bytes, all CSS
148 KB).

**What multiplies by the frame count** (review of feature 10a). Measured
worst cases for one frame, which the 1,000 frames of a page would multiply:
a document of 1 Mi nodes (500,000 `p` elements with text) 1.5 GiB of DOM,
style and layout; 50 PNGs of 4000 x 4000 px 3.2 GiB; a style sheet of
1,000,000 rules (13 MB) 1.46 GiB. The node, image and style sheet budgets
above are page-wide for these. Not multiplied, because frame documents run
no scripts and use no web fonts in feature 10a: the script heap, the
`fetch()` budget of 256 MiB per document, web fonts, the font context.
Feature 10b put the vector cache of SVG images under one page-wide cache and
measured the display lists and rasters of frames with 1,000 visible frames
(hostile cases `frames-*`, table above):

- The display lists follow from the node budget, which is page-wide, so
  they cost what one big top-level document costs. 1,000 frames of 500 boxes
  with text (2.5 million items, 1 Mi nodes): 2.6 GiB of RSS with everything
  else of these documents, 4.8 s to load and paint, 0.52 s for one
  rasterization. They got no memory budget of their own; the budget of items
  bounds the time. The hostile set has a cap of 2,048 MiB per case, so its case
  `frames-1000-big-lists` has half of these boxes; the numbers above were
  measured once with the same case at 500 boxes.
- A frame has no raster of its own. It draws in the target or in the layers of
  the groups around it, so the budgets of a strip (group layers, filters,
  masks, transforms, paths) cover all frames together. 40 nested frames with a
  blur and a rotation each: 0.25 s, 102 MiB, with the warning of the transform
  layers.
- 1,000 small visible frames (a background and a line of text each) take
  12 ms of raster.

The top-level document keeps the per-document limits: it has no page-wide
budget for images and style sheets, so a top-level document with 50 such
PNGs takes the same 3.2 GiB (backlog).

Some byte limits of scripts stay per document and so multiply with the documents:
scripts that wait 256 MiB (`MAX_HELD_SCRIPT_BYTES`), the text of `document.write` that
waits 64 MiB, the string handlers of timers 32 MiB (backlog).

Hostile pages to add (`just hostile`): 1,100 frames created in a loop; a
frame nested 100 deep through `srcdoc`; a page that embeds itself with a
growing query string; a frame that reloads itself with `meta refresh 0`
and with `location.reload()` in `load`; a `postMessage` flood to self and
to a cross-origin frame; a `MessageChannel` ping-pong without end; a
transfer of 10,000 ports; 40 frames of 40 origins; a frame that creates
and removes `about:blank` frames in a loop (realm budget); a cross-origin
frame that allocates to its heap limit (the top document keeps running).
Each case ends within the set's limits and logs the limit that acted.
Feature 10a's review added `frames-1000-big-srcdoc-js-on`,
`frames-1000-dense-srcdoc-js-on`, `frames-1000-big-documents`,
`frames-100-big-images`, `frames-200-big-style-sheets` and
`frames-replaced-in-a-loop-js-on` (the budgets; the last one checks that
replaced and destroyed documents return their bytes).

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
   across runtimes, `document.domain` and `originAgentCluster` (the
   getters and the setter's checks exist since M9 feature 1; this feature
   keeps the cross-origin behaviour of the probe case `document-domain`).
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
