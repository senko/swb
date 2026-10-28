# Development log

Newest entries first. One entry per working session or milestone. Record what
was done, what was learned, and what is next. Keep entries short; details go
in commit messages, ADRs and other docs.

## 2026-10-28: M8 feature 7a2, `getComputedStyle` and layout from scripts

- A script task can lay out: the page lends its font context with the
  document state for each task. 7b's geometry uses the same path.
  Forced runs show in `flushes`.
- `getComputedStyle(element, pseudoElt)` returns a new, live, read-only
  `CSSStyleDeclaration` at each call, as Chromium does. It gives the
  values of every property that `style` computes, in Chromium's form:
  colors with `currentcolor` resolved, lengths in px, sizes, margins,
  paddings and insets from layout in 1/64 px units, transforms as
  `matrix()`, shorthands. `::before`, `::after`, `::marker` and
  `::placeholder` get their own cascade, also without a box.
- A style value needs only the style of the element (the element style
  on demand of 7a1); a layout value needs a layout and one walk of the
  fragment tree per layout.
- Learned: a style or layout flush inside one native call is invisible
  to the interpreter's countdown. A loop of style writes and layout
  reads on 50,000 elements ran past 30 s; the binding now checks the
  task deadline after each read, and the loop stops at the 5 s limit.
- Probe `web-computed-style.json`: 23 cases, 316 queries (with the 843
  values of 7a1 read through `getComputedStyle`); 46 departures with
  their reasons. Three layout differences that now show through
  `getComputedStyle` are in the backlog.
- Review fixes: a margin, padding or inset of a length without a
  percentage is the computed value, not snapped to layout units, as in
  Chromium (`padding: 0.4em` at 16px gave `6.39062px`; Chromium gives
  `6.4px`). If the runtime ever loses the document state, the page now
  takes the system fonts instead of keeping the empty font context.
- JS off: snapshot identical, perf within noise. Hostile set 196 cases,
  195 ok + the known failure (3 new). Runtime +288 bytes.

Next: feature 7b, geometry, scrolling from scripts, `matchMedia` and
`requestAnimationFrame`.

## 2026-10-28: M8 feature 7a1, style from scripts: invalidation, counters, `element.style`

- Feature 7a is split: 7a1 is everything except `getComputedStyle`,
  which is 7a2.
- The journal drain marks the styles dirty only for changes in the
  document. Changes in detached subtrees and in template contents keep
  the styles.
- `focus()` after a DOM change computes the style of the element and
  its ancestors only (`compute_element_styles`, "the style of one
  element on demand" of ADR 0028 section 9). A unit test checks that
  the result equals the full restyle. 2,000 times "append an input,
  `focus()`" on a page of 20,000 elements reached the 5 s task limit;
  now they take 0.21 s.
- Style and layout runs are counted per document, with the runs that
  script queries force apart (`flushes` in `--bench` and `page.info`).
  The loads of the three JS targets force none.
- `element.style`: a `CSSStyleDeclaration` over the `style` attribute
  with Chromium's serialization of specified values (843 measured
  values) and of shorthands. The property names are named properties
  of the object, as in Chromium, so feature tests see what swb parses.
  `CSS.supports` and `CSS.escape`.
- Decisions (ADR 0028 "Feature 7a notes"): the declaration block keeps
  Chromium's text per longhand, validated by swb's parsers; the new
  extended attribute `[SwbPropertyNames]`; `grid`, `mask` and
  multi-layer backgrounds are kept as written.
- Learned from the probes: the property names are own, enumerable data
  properties of each declaration; `delete style.color` is true and
  changes nothing; important declarations move to the end of
  `cssText`; numbers have 6 significant digits; `#hex` and `hsl()`
  become `rgb()`, but color names stay names.
- Review fixes: `outline: auto` sets `outline-style`, as in Chromium
  (it set the color); `CSS.supports` takes `!important` in a
  declaration; the declaration blocks of elements that are not
  connected go when 4,096 blocks exist, since nodes live as long as
  their document and the blocks of thrown-away elements stayed too.
- JS off: snapshot identical, perf within noise. Hostile set 193 cases,
  192 ok + the known failure (5 new).

Next: feature 7a2, `getComputedStyle`.

## 2026-10-25: M8 feature 6d2, the user's input as DOM events

- With JavaScript on, the engine's input is DOM events: pointer and
  mouse events with the hover chains, `click`, `auxclick`, `dblclick`
  and `contextmenu`; `wheel` and `scroll`; `keydown`, `keypress` and
  `keyup`; `beforeinput`, `input` and `change` for edits; focus events
  for every focus change by the user. The default actions run only if
  no listener canceled the event.
- Decision (ADR 0028 "Feature 6d2 notes"): the user's input is the task
  source "user interaction" without a queue of its own. The embedder's
  call is the task and runs at once; the other sources run only in
  `Page::pump`. Each event is one dispatch task, so a listener that
  removes the target changes what follows.
- The trusted `click` runs the activation of 6d1, and the engine skips
  its own action. `summary` and `javascript:` URLs (links and form
  actions) are new; a string result does not replace the document.
- Learned from the probes: a canceled `pointerdown` also suppresses
  `mousedown`, the focus change and the selection; `contextmenu` fires
  on the press on Linux; Enter on a link clicks after `keydown` and
  without `keypress`, Space clicks on `keyup`; `change` fires before
  `blur` and compares with the value before the first edit; the window
  gets `blur` when Tab leaves the page; `wheel` is cancelable only with
  a non-passive listener.
- The probe tool has `steps` (click, type, key, wheel), and swb has
  `--headless --steps=JSON`, so real input runs the same way in both.
  New probe file `web-input-real.json` (45 cases), no difference in
  swb.
- Feature 6 is complete. The Hacker News test passes: `hn.js` runs, its
  `click` listener runs once per click, and the links and the search
  form work as with JavaScript off. `compare --js hacker-news` equals
  the JS-off scores; load time 20 ms with JavaScript on, 19 ms off.
- Review fixes: after the document's scripts stopped at the time
  limit, every click did nothing; input now works as with JavaScript
  off. An image button sends the click point. Pointer moves collected
  the boxes of the whole page for `offsetX` at each move (100,000
  elements, 200 moves: 5.1 s against 1.0 s with JS off); the boxes are
  now cached per layout and scroll position (1.6 s against 1.4 s). A
  `javascript:` URL runs in a queued task, and not for another target
  or a Ctrl, Shift or Alt click, as in Chromium.
- Hostile set 188 cases, 187 ok + the known failure (six new); JS-off
  snapshot identical, perf within noise.

Next: feature 7a, style and layout invalidation from the journal.

## 2026-10-25: M8 feature 6d1, input events from scripts

- Feature 6d is split: 6d1 is the script side, 6d2 the engine's own
  input as DOM events.
- 18 event interfaces from UI Events, Pointer Events and HTML, with
  constructors, init dictionaries and Chromium's conversions, and the
  `createEvent` names that Chromium accepts.
- All `on*` handlers on `HTMLElement`, `SVGElement`, `Document` and
  `Window`. Content attributes compile at their first use with the
  scope chain of document, form owner and element; `return false`
  cancels; `window.onerror` gets five arguments.
- `focus()`, `blur()`, `activeElement` and `hasFocus()` fire the focus
  events in Chromium's order. `click()` and a dispatched `click`
  `MouseEvent` run the activation behaviour: checkboxes and radio
  buttons, links, submit and reset buttons, labels. `submit()`,
  `requestSubmit()` and `reset()` fire their events. The page carries
  out the navigations and scrolls that a script asks for after the
  task.
- Learned from the probes: `click()` fires a `PointerEvent` with
  `pointerId` -1; a nested `focus()` inside a `blur` handler stops the
  first focus change (no `focusin`); removing, disabling or hiding
  (`hidden`) the focused element blurs it, `display: none` does not.
- Two new probe files (149 and 199 queries). A runtime costs 368 KB
  and about 0.9 ms more, for about 480 handler accessors.
- With JavaScript on, the Craigslist posting now differs from JS off in
  17,803 pixels (94,935 before).
- Review fixes: `click()` on a `label` that contains its `input`
  overflowed the stack, because the click at the control bubbled back
  to the label. The label now ignores a click that comes from its
  control, and a flag per label stops other loops (new hostile case).
  Handlers that `body` forwards to the window have no document scope;
  moving the focused element blurs it; `hidden` on an ancestor does not.
- A runtime now costs about 1.13 MB and 3.1 ms at creation, past the
  limit in ADR 0028; lazy creation of the interface objects comes
  before 8a.

Next: feature 6d2, the engine's input as DOM events.

## 2026-10-25: M8 feature 6c2, `Image`, `Option` and the SVG interfaces

- `new Image(width, height)` and `new Option(text, value,
  defaultSelected, selected)` work as in Chromium: `name`, `length`,
  the interface prototype, the error for a call without `new`,
  subclassing with `class extends` and `Reflect.construct`. The
  generator writes `[LegacyFactoryFunction]`; `js` has one new API,
  `Runtime::define_constructor`. test262 and jsbench are unchanged.
- 38 SVG element interfaces and `SVGAnimatedString` come from the SVG 2
  IDL, cut to the members that need no layout. The prototype chains and
  constructor names of the SVG elements match Chromium, and unknown SVG
  names give `SVGElement`. `className` is a live `SVGAnimatedString`;
  `ownerSVGElement`, `viewportElement`, `dataset`, `tabIndex` and
  `classList` work on SVG elements.
- Learned from the probes: `viewportElement` is the nearest `svg`,
  `symbol` or `image` ancestor, and the SVG `tabIndex` is -1 (0 for
  `a`).
- Two new probe files (151 and 102 queries). The SVG departures of the
  older tests are gone, and the older probes lost 14 differences. A
  runtime costs 36 KB more.
- Not done: the SVG geometry members (`getBBox`, `createSVGRect`;
  feature 7b). Target scripts test for them and take their non-SVG
  path. A detached `img` from `new Image()` loads only when it is
  inserted.
- Review fixes: `ownerSVGElement` under `foreignObject`; `new
  Option()` selectedness without the dirty mark (a new engine path
  from the attribute change steps); `SVGStyleElement.disabled`; the
  factory functions read `new.target.prototype` first. A non-object
  `prototype` of `new.target` stays a documented departure (Web IDL
  against Chromium's `Object.prototype`).
- Performance: every attribute write dropped the cache of
  `children`, so a `for...of` loop that writes to the children was
  quadratic (100,000 children: 5.4 s). The child lists now cache
  against `Document::structure_generation`, which only child-list
  changes bump (0.42 s). JS-off load time is 1 % higher on the large
  fixtures; part of it is code placement.

Next: feature 6d, input as DOM events.

## 2026-10-24: M8 feature 6c1, HTML element interfaces and form controls

- Every HTML element has its interface: prototype chains, constructor
  names, `instanceof` and `toString` match Chromium. Unknown names give
  `HTMLUnknownElement`, valid custom element names `HTMLElement`, and
  the document is an `HTMLDocument`. The constructors throw `Illegal
  constructor`.
- The reflected attributes come from the HTML standard's IDL: strings,
  URLs, enumerated attributes with Chromium's missing and invalid value
  defaults, booleans, numbers with their limits, `relList` with
  `supports`, `htmlFor`. `a.href` and the other URL members of `a` and
  `area` use the `url` crate.
- Form controls keep their state in the engine. Scripts read and write
  `value`, `checked`, `selectedIndex` and `selected` through
  `EngineHost`; the writes reach the rendering and the `:checked`,
  `:valid` and `:placeholder-shown` queries. A write to a control that
  has no state yet (a new `input`) waits as a seed for the next rebuild.
  A radio button that a script checks now wins over its group, as HTML
  says.
- Collections: `table.rows`, `tBodies`, `tr.cells` and the insert and
  delete methods, `select.options`, `selectedOptions`, `form.elements`
  with `RadioNodeList`, `fieldset.elements`.
- Learned: with `[LegacyOverrideBuiltIns]` on `form`, every property
  read of a form (`f.appendChild`) rebuilt the control list after each
  change; a loop of 8,000 appends took seconds. `form` has no override
  now (18 ms), so a control named `submit` does not hide `form.submit`.
- The probe `web-html-elements.json` has 62 cases (671 values). The
  older probes lost 13 differences. A runtime with all the interfaces
  costs 725 KB of heap (464 KB before) and the same 1.6 ms to create.
- `new Image()`, `new Option()` and the SVG interfaces move to 6c2:
  `[LegacyFactoryFunction]` needs a `js` API for a native constructor
  with a given prototype.
- Review fixes:
  - Values that scripts write to text controls bypassed the memory
    limits: 400 writes of one 5 MB string reached 6.4 GB and an
    allocation abort. A budget of 32 MiB per document now ends such a
    script; it is low because a visible control costs 70 to 180 bytes
    per character in layout.
  - Fixed against Chromium: a removed select keeps its selectedness;
    `cloneNode` copies value and checkedness; `option.text` skips
    scripts; an inserted checked radio button wins; range clamping,
    `:indeterminate`, the value at a `type` change, the `host` setters.
  - A loop of `checked = true` on new radio buttons rebuilt the form
    state per write (6,000 radios: 4.5 s); now 0.04 s. 30,000
    `option.selected = true` writes went from the time limit to 0.2 s.
- With JavaScript on, the Craigslist posting has no script error but
  differs at the top of the page; Wikipedia stops at a `TypeError`
  (`reading 'match'`). Features 11 and 12 take them up.

Next: feature 6c2, `Image`, `Option` and the SVG interfaces.

## 2026-10-24: M8 feature 6b, DOM queries and markup

- Selectors from scripts: `querySelector`, `querySelectorAll` (a static
  list), `matches`, `closest` and `webkitMatchesSelector` use the
  selector parser of `css` and the matcher of `style`. The new
  `style::Matcher` makes no pass over the document. `:scope` is the
  start element, the dynamic pseudo-classes follow the engine's states,
  and an invalid selector throws Chromium's `SyntaxError`.
- Live lists: `getElementsByTagName`, `...NS`, `getElementsByClassName`,
  `getElementsByName` and the document collections (`forms`, `images`,
  `links`, `scripts`, `anchors`, `embeds`, `plugins`, `applets`). They
  cache their nodes per DOM generation. The lists that scripts make are
  not in the registry, so the collector can free them. `window[name]`
  with several elements gives a live `HTMLCollection`.
- Markup: `innerHTML`, `outerHTML` and `insertAdjacent{HTML,Element,
  Text}` serialize and parse with the HTML algorithms, with Chromium's
  exceptions. The serializer is iterative: a read of a tree 100,000
  levels deep takes 5 ms. Scripts in markup do not run; style sheets and
  images in it apply and load.
- More documents: `DOMParser` (`text/html`), `document.implementation`
  (`createHTMLDocument`, `createDocumentType`, `hasFeature`) and
  `template.content` make documents in the same arena. Nobody renders
  them, their scripts do not run, and each has its own quirks mode and
  scripting flag, so `noscript` in them is markup.
- `hn.js` now runs to the end. The missing `getElementsByClassName` was
  its last error; with JavaScript on, Hacker News still matches its
  JS-off boxes and pixels (821 of 821, 0 pixels).
- The probe `web-dom-query.json` has 78 cases (406 values, three equal
  Chromium runs). swb matches except two departures: a new list object
  per `getElementsBy*` call, and U+FFFD for a lone surrogate in markup.
- Learned: a descendant selector that fails high in a chain of 100,000
  levels walks all ancestors of each element, which is quadratic in the
  depth; without a bound one query ran for over 60 s. One query now
  stops after 100,000,000 steps with a warning (0.7 s).
- Eleven new hostile cases: 166 of 167 pass, plus the known failure
  `nest-div-100000`. JS-off snapshot identical, JS-off page loads within
  1 %.
- Review fixes:
  - `closest` had no step bound (91 s on 100,000 levels); it now stops
    at the query bound (1.2 s). The bound also counts each selector of
    a list, so a list of 200,000 selectors takes 1.0 s, not 9.2 s.
  - Live lists rebuilt in full after each change, so a drain loop
    `while (r.children.length) r.removeChild(r.children[0])` was
    quadratic (50,000 children: 3.06 s). Lists now walk only as far as
    the item asked for, and each node keeps its child count and
    element-child count, so `length` of `children` and `childNodes` is
    O(1). The same loop takes 0.07 s; memory grows by 8 bytes per node
    (0.5 % on a page of 900,000 nodes).
  - An `innerHTML` write of `<p>x` makes 2 nodes instead of 3: the
    parse reuses an empty fragment, and `insertAdjacentHTML` keeps one
    stand-in `body` per document.
  - Found, not fixed: in the `table` fragment case, html5ever ignores
    `tbody` and `tfoot` after `thead` (backlog).

Next: feature 6c, the HTML element interfaces.

## 2026-10-24: M8 feature 6a, DOM tree changes and attributes

- Scripts can now change the DOM: `insertBefore`, `appendChild`,
  `replaceChild`, `removeChild`, `append`, `prepend`,
  `replaceChildren`, `before`, `after`, `replaceWith`, `remove`,
  `cloneNode`, `importNode`, `adoptNode`, `normalize`, `textContent`,
  `nodeValue`, the `CharacterData` methods, `splitText`,
  `compareDocumentPosition`, `isEqualNode` and the namespace lookups.
  The exceptions come in Chromium's order and with its messages.
- Attributes: `getAttribute`, `setAttribute`, `removeAttribute`,
  `toggleAttribute`, the NS variants, `attributes` (`NamedNodeMap`),
  `Attr`, `id`, `className`, `classList` (`DOMTokenList`) and `dataset`
  (`DOMStringMap`). All attribute writes go through one function, so the
  hooks see each of them.
- Nodes come from `createElement` (with options), `createElementNS`,
  `createTextNode`, `createComment`, `createDocumentFragment`, and the
  constructors of `Text`, `Comment` and `DocumentFragment`. `children`
  is a live `HTMLCollection`; `getElementById`, `referrer` and
  `defaultView` complete the `Document` accessors.
- Each insertion runs the steps of script elements: inline scripts run
  inside the call, and a script that an earlier one removed does not
  run. `src` and `async` added by a script, text changes in a script and
  `cloneNode` of a started script follow HTML.
- The change journal of the DOM already reached the engine. New engine
  tests check that an inserted, moved or removed element changes the
  boxes, that `class`, `id`, `style` and text changes restyle and
  relayout, and that an inserted `style`, `link` and `img` apply or
  load. A moved form control gets its form owner from its new
  ancestors.
- Lists cache their nodes per DOM generation and drop stale caches.
  `getElementById` among thousands of equal ids walks the tree once per
  generation. A tree of 100,000 nested divs built by `appendChild`
  renders in 0.5 s (the node walks are iterative).
- The probe `web-dom-mutation.json` has 476 cases, equal in three
  Chromium runs. swb matches all but two: `Attr` is not a `Node` in swb,
  and `setAttribute('Q:C')` next to a namespaced `q:c` replaces it (the
  standard; Chromium adds a second attribute). Where Chromium departs
  from the standard elsewhere, swb follows Chromium: looser attribute
  names, `classList.add()` without tokens creates `class`, `namedItem`
  prefers ids, and `textContent` keeps a single Text node that already
  has the new data.
- JS-off snapshot identical, JS-off page loads within 3 %, hostile set
  155 ok + 1 known failure with nine new cases.
- Review fixes:
  - `createElement(name, {is})` set an `is` attribute. The implementer
    had not measured it; Chromium sets none.
  - Token list reads parsed the whole attribute at each call. Now the
    parsed tokens stay for the DOM generation: an indexed loop over
    5,000 tokens went from 4.8 s to 2 ms.
  - A chain built bottom-up (wrap the tree in a new parent, repeat) was
    quadratic in three places: the search for inserted scripts, the
    form-owner reset and the engine's walk of the change journal. Now
    the script search and the connection test advance in turns, the
    reset walks only subtrees moved from a parent (removals clear the
    parser's form pointers, so a detached subtree holds none), and the
    journal walk skips subtrees it visited. 100,000 levels: from over
    the 5 s limit to 0.3 s.

Next: feature 6b, queries and markup.

## 2026-10-24: M8 feature 5c, scripts in the HTML parser

- Scripts now run where the parser finds them. The runtime drives the
  parse through `EngineHost::parse_step`. Inline scripts run at their
  end tag, each as its own task. Parser-blocking scripts and the style
  sheets that block scripts make the parser wait, and parser tasks in
  the event loop continue it.
- "The end" follows HTML: readiness `interactive`, deferred scripts in
  order, `DOMContentLoaded`, the scripts that run as soon as possible,
  then `load` and `pageshow` when nothing delays the load event. The
  engine decides that from the requests after a layout. Navigation fires
  `pagehide` and `unload`. `page.waitForLoad` now waits for the `load`
  event; requests that scripts start later do not count.
- `document.readyState`, `document.currentScript`, `HTMLScriptElement`,
  and `load` and `error` at script elements. `createElement` and
  `appendChild` came forward from 6a: inserted external scripts are
  async or in order, inserted inline scripts run inside the insertion.
- The engine fetches the scripts: at most 64 at once, the others wait in
  a queue. It checks the status, the MIME type and nosniff, and decodes
  the text.
- Error positions of inline scripts count from the document. The parser
  finds the start of each script's text, and `js` has a new
  `Runtime::eval_named_at`. Functions that an inline script defines and
  a timer calls later also report document positions, as in Chromium.
- Chromium was measured with a new probe option: `resources` serves
  files with delays from a local HTTP server. Two Chromium orders differ
  from HTML, and swb follows them: `DOMContentLoaded` and `load` fire at
  once, not in queued tasks, but the end of the parse waits for a task
  for a document from HTTP(S) and after any wait of the parser. Only a
  file document that never waited ends at once, so a 0 ms timer of an
  inline script runs after `load`. The probe `web-scripts.json` (38
  cases) matches Chromium except module scripts, which swb does not run.
- Departures: module scripts, import maps and SVG scripts do not run; no
  preload scanner; `stop` fires no readiness events; errors of
  cross-origin scripts are not muted.
- JS-off snapshot identical, JS-off page loads within noise (−1.6 % to
  +0.5 %), hostile 146 ok + 1 known failure with four new cases (script
  chains, held script text), test262 unchanged.
- Review fixes:
  - When a document's scripts stopped (time limits, the long-task
    budget) while the parser waited for a script, the parse never went
    on and `load` never came. Now the page drops the script fetches and
    the parser runs to the end without scripts.
  - "The end" waited for a task only while subresources loaded. The
    first probe cases came from `file:` URLs, which hid the HTTP rule;
    16 new cases measure it (above). So for the common page shape, an
    external script and then an inline script that sets a 0 ms timer,
    the timer now runs before `DOMContentLoaded`, as in Chromium.
  - A lone CR ended a line in error positions; Chromium counts only LF
    and CRLF.
  - Two quadratic costs: the column of each script start was counted
    from the line start (80,000 scripts on one line: 12.7 s, now
    0.4 s), and each finished async script was removed from a list
    (200,000 inserted scripts: 9.3 s, now 1.8 s).
  - Fetched script text that has not run had no limit outside the JS
    heap. Now at most 256 MiB per document; later scripts fail with an
    `error` event. The loader itself still queues whole bodies without
    a bound (backlog).
- Exit of feature 5: senko.net and Hacker News render the same with
  JavaScript on and off (all boxes, 0 pixels differ). `hn.js` runs at
  the end of the body and stops at the missing `getElementsByClassName`
  (6b).

Next: feature 6a, tree changes and attributes.

## 2026-10-24: M8 feature 5b, the event loop

- Documents with JavaScript on have an event loop. The binding state
  keeps one task queue, ordered by due time in page time and then by the
  order of queuing. Timers are due when they expire; other tasks (the
  notification of rejected promises, finalization cleanups) are due when
  they are queued. This gives the order that Chromium shows.
  `Page::pump` runs due tasks for at most 16 ms and returns the time of
  the next one. The GUI calls it after each batch of window events and
  sleeps until then; the headless server calls it in its loop; the load
  waits run due tasks while the page loads.
- Every task runs under the 5 s deadline and ends with a microtask
  checkpoint. The checkpoint also runs after each listener of an event
  that the engine fires and after a timer callback.
- `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval` (string
  handlers, extra arguments, ids shared by both kinds),
  `queueMicrotask` and `reportError`. A new probe,
  `tools/probes/web-event-loop.json`, has 48 Chromium 148 values: the
  order of tasks, microtasks and timers, the nesting clamp (Chromium
  uses HTML's threshold, so the seventh nested timer waits 4 ms, and
  keeps the level in microtasks), timer arguments and ids, error events
  and the clock. swb matches 46; the two others are error positions.
- Uncaught exceptions of scripts, timers, listeners and microtasks fire
  a cancelable `ErrorEvent` at the window. Only an event that no
  listener canceled gives the console line. Unhandled rejections fire
  `unhandledrejection`.
- The page clock runs in real time or in virtual time
  (`--virtual-time`), from the system time or from a fixed start
  (`--now MS`). `Date.now()`, `performance.now()`,
  `performance.timeOrigin` and `Event.timeStamp` use it. `compare --js`
  starts swb's clock at the fixture's capture time, as Chromium's, and
  runs 1.5 s of virtual time after the load. The headless runner has
  `--settle-ms`, the automation API has `page.waitForIdle`, and
  `page.evaluate` and `--eval` wait for promises that timers settle.
- Limits (ADR 0028 section 16): a runtime that reaches the heap limit is
  dropped, and the page stays. The long tasks of a document (50 ms or
  more) may run for 30 s in total, so a page of 1,000 scripts of 4.9 s
  stops after 30 s instead of 80 minutes. At most 1,048,576 timers and
  their arguments per document. After 1,000 console lines, a document's
  lines go to the debug log.
- `js`: `queue_microtask`, `promise_is_handled` and the locations of
  errors and throws. A first version grew the `Vm` struct by one field
  and made the "1M small objects" benchmark 45 % slower; the throw site
  now shares an existing field. jsbench geomean −0.7 %, test262
  unchanged.
- Nine new hostile cases pass (143 cases: 142 ok, 1 known failure).
  JS-off snapshot unchanged; JS-off page loads within ±1.5 %.
- Review fixes:
  - An interval scheduled its next run from the end of the callback, as
    HTML says. Chromium keeps a grid of beats from the first due time: a
    20 ms interval whose first callback runs 30 ms fires at 20, 50, 60,
    80. swb now does the same and matches five measured cases.
  - Each string handler kept its own copy of the code outside the JS
    heap, so `for (;;) setTimeout(s, 1e9)` with a 20 MB string used all
    memory. Now at most 32 MiB of handler code per document.
  - A chain of tasks that are due at once (an `unhandledrejection`
    listener that rejects again) kept virtual time still, so
    `--settle-ms` ran until the real time-out. Each task now moves
    virtual time by 0.01 ms.
  - `page.waitForIdle` and `page.evaluate` in virtual time block the
    server for at most 30 s.
  - In real time, `Date.now()` is the system time, so it stays correct
    after the machine was suspended.
- Next: feature 5c, script processing.

## 2026-10-24: M8 feature 5a, the DOM event core

- `EventTarget` keeps listeners and dispatches events. The listeners of
  nodes and of the window live in the binding state (a node with
  listeners has a wrapper, so both live as long as the document); a
  target made by `new EventTarget()` keeps them in its own data. Each
  list is per event type with an index by callback and capture: adding,
  the duplicate check and removal take constant time, and a dispatch
  copies only the listeners of its type. At most 262,144 listeners per
  document (ADR 0028 section 16), because these lists are outside the JS
  heap.
- `addEventListener` takes `capture`, `once` and `passive` in both
  forms, with the default passive value of touch and wheel events at the
  window, the document and its body; `signal` waits for `AbortSignal`.
- The dispatch follows DOM §2.9: the path up to the document and then
  the window (not for `load`), the capture and bubble passes,
  `stopPropagation`, `stopImmediatePropagation`, `cancelBubble`,
  `preventDefault` with `cancelable` and passive listeners,
  `returnValue`, `composedPath`, `window.event`, and the dispatch flag
  with Chromium's `InvalidStateError` messages. An exception in a
  listener goes to the console in the words of Chromium's `error` event,
  and the dispatch goes on; a time-limit termination ends the task.
- `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`,
  `PageTransitionEvent` and `document.createEvent` (for these
  interfaces). The engine fires a trusted event at a node or the window
  as a task (`Page::dispatch_event`) for 5c and 6d. The generator now
  handles unions with `undefined`.
- A new probe, `tools/probes/web-events.json`, has 65 Chromium values;
  swb matches all but four expected ones (no `signal`, no `error` events
  until 5b, `PromiseRejectionEvent.promise` as an `object`). Four new
  hostile cases (nested dispatch, an endless listener, many listeners on
  one target, the listener limit). JS-off snapshot unchanged.
- Review fixes: a dispatch found each struct's adjusted target by a scan
  back to the start of the path (O(depth²)); it now finds them once. A
  type's list builds its index only from 8 listeners, and the listener
  limit is 262,144 per document (was 1,048,576): the hostile case for
  the limit used 590 MiB, now 141 MiB.
- Next: feature 5b, the event loop and timers.

## 2026-10-23: M8 feature 4c2, the runtime of a document and the first DOM bindings

- Each document with JavaScript on gets a script runtime
  (`swb_web::ScriptRuntime`), created at the commit and dropped with the
  document. The engine lends its `DocumentState` for each task through
  the trait `EngineHost` and takes it back by downcast; that drains the
  task's journal.
- The global object is the `Window` (generated `init_global`, immutable
  prototype) with `WindowProperties` in its chain; `window[id]` uses the
  DOM's id index and `window[name]` a cache keyed by the DOM generation.
  Wrappers live in a vector indexed by `NodeId`, the `[SameObject]`
  objects in a side table; both are traced, so identity, expandos and
  `WeakMap` keys hold for the document's life.
- Bindings: the read-only tree accessors of `Node`, `Element`,
  `CharacterData`, `Document` (with HTML's `title`, `body` and `head`
  and their setters), `DocumentType`, the `ParentNode` and
  `NonDocumentTypeChildNode` mixins, and the live `childNodes`, whose
  cache by DOM generation makes a loop over it walk the children once.
  `Node`'s tree-changing members left the IDL until 6a. `Event` and
  `EventTarget` are stand-ins until 5a.
- `console` has `log`, `info`, `warn`, `error` and `debug` in Chromium's
  shape; lines go to the log and to the buffer of
  `page.consoleMessages`. `page.evaluate` and `swb --headless --js
  --eval EXPR` return the value as JSON (`--eval` in the probe's
  contract, so `just probe --with-swb` compares `js` rows).
- Inline classic scripts run in tree order after the parse (5c moves
  them into the parse). After three tasks that reach the 5 s limit, a
  document runs no more scripts. The refusal of JavaScript on a small
  stack moved from the GUI into the engine
  (`PageConfig::scripts_allowed`).
- A new probe, `tools/probes/web-dom-tree.json`, has 74 Chromium
  values; the tests use them, with five departures (no element
  interfaces beyond `HTMLElement` and `HTMLHeadElement`, no
  `HTMLDocument`) and one for `window[name]` of several elements.
- Feature 4 exit: `document.title` gives Chromium's value on all page
  fixtures; RSS returns after a navigation; a runtime takes 1.25 ms to
  create. JS-off snapshot unchanged, perf within 2 %, test262
  unchanged, the hostile set as before with three new cases.
- Review fixes: errors that throw a `DOMException` read as `Name:
  message` in `--eval`, `page.evaluate` and the console (the engine's
  description does not call the accessors; the serializer does);
  `page.evaluate` replaces lone surrogates, which strict JSON parsers
  refuse; `--eval` exits with an error when the navigation fails and
  requires `--headless`.
- Next: feature 5a, the event core.

## 2026-10-23: M8 feature 4c1, legacy platform objects and iterables

- Feature 4c was split into 4c1 (the generator) and 4c2 (the runtime per
  document) after 4b.
- The generator makes the indexed and named properties of Web IDL §3.9.
  The interface that declares a getter, setter or deleter gets trait
  functions for the supported indices and names and the special
  operations. Every class whose instances have them (also by
  inheritance) gets hooks for [[GetOwnProperty]], [[Set]],
  [[DefineOwnProperty]], [[Delete]], [[PreventExtensions]] and
  [[OwnPropertyKeys]], with `[LegacyOverrideBuiltIns]` and
  `[LegacyUnenumerableNamedProperties]`. Refused writes in strict code,
  definitions and deletions throw Chromium's messages ("Failed to set an
  indexed property [0] on 'NodeList': Indexed property setter is not
  supported."); `Object.freeze(list)` throws "Cannot freeze".
- `iterable<V>` uses the `Array.prototype` functions; `iterable<K, V>`
  gets default iterator objects with an "X Iterator" prototype, `next`
  and `forEach`.
- `js`: hooks for [[PreventExtensions]] and for the messages of
  refusals (asked only when the engine throws, so sloppy writes stay
  silent); a `set` hook can hand the property to the ordinary steps;
  hook keys keep their order. [[SetPrototypeOf]] is not a hook: any
  object can carry an immutable-prototype mark, so the global object,
  `Window.prototype` and its ancestors stay cacheable. test262 unchanged
  in both modes; jsbench geomean −0.6 %.
- `NodeList` is in `dom.idl`. Tests: interfaces shaped like `NodeList`,
  `HTMLCollection`, `HTMLOptionsCollection`, `DOMStringMap`, `Storage`,
  `HTMLFormElement` and `Headers`, in both GC modes, against a new probe
  (`tools/probes/web-legacy.json`). Where Chromium departs from Web IDL
  (`Reflect.defineProperty` answers, key order, writes through another
  receiver), swb follows Web IDL.
- Review fixes: the exception prefixes of the special operations are
  made only for an exception (each `list[i]` read allocated two strings);
  the key list of `[[OwnPropertyKeys]]` is reserved with a failure
  check; a test ends a run at its deadline inside a named setter's
  conversion.
- Next: feature 4c2, the runtime per document.

## 2026-10-23: M8 feature 4b, the WebIDL generator and the binding glue

- New crate `swb-webidl`: a parser for the WebIDL subset of ADR 0028
  section 5 (errors with file and line), a checked model (partials,
  mixins, typedefs, extended attributes) and the generator of the glue.
  Per interface, it writes a trait of hand-written functions and the
  native functions that check `this`, convert the arguments (Web IDL
  §3.2), resolve overloads, call the trait and convert the result;
  `types.rs` has the dictionaries, enumerations, unions and callbacks.
  `just bindings` writes the checked-in output; a test fails when it is
  stale.
- New crate `swb-web` with the support code: conversions in both
  directions, exceptions with Chromium's messages and member prefixes,
  `DOMException` (it inherits from `Error.prototype`, as in Chromium),
  overload selection, `[Reflect]`, `[LegacyUnforgeable]`,
  `[Replaceable]`, `[PutForwards]`, `[Global]`, callbacks. The glue of
  `EventTarget`, `Event`, `Node` and `DOMException` is generated from
  the DOM, Web IDL and HR-Time IDL (license headers, notices). The
  behaviour of `Node` and `EventTarget` comes in 4c and 5a; the glue is
  generic over the host type, so it compiles and registers without
  them.
- `js`: `ClassId::from_index`, `begin_interface`/`end_interface`
  (`constructor` last, as Chromium lists it), public entries to the
  abstract operations the conversions need. jsbench within noise.
- Tests: a test IDL with a member for each type and extended attribute,
  run through the runtime with a fake DOM; the browser's glue with a
  stand-in host (names, descriptors, lengths, constants, messages); all
  in both GC modes; expected values from a new probe
  (`tools/probes/web-bindings.json`).
- Review fixes: a dictionary that inherits from itself made the
  generator allocate without end, and a typedef that refers to itself
  overflowed its stack; both are now errors. A record whose keys convert
  to the same string (lone surrogates) keeps one entry with the last
  value, as Web IDL says.
- The first review run ended when the machine ran short of memory:
  `/tmp` is a RAM disk, and agents had left copies of the tree and
  build directories there. Briefs now keep builds out of the scratchpad.
- Next: feature 4c, the runtime per document.

## 2026-10-23: M8 feature 4a, the embedding hooks in `js`

- Host classes (ADR 0028 section 6): a registry per runtime with
  parents, numbered in the order of registration, so generated glue can
  use constants. Per realm, each class gets an interface object and a
  prototype linked to the parent's, with methods, accessors and
  constants. A host object has a class and host data: a word (a
  `NodeId`) or generic data that the collector traces, in 12 bytes, so
  an object stays 88 bytes. A wrong `this` gives Chromium's
  `TypeError: Illegal invocation`; interface objects throw Chromium's
  `Illegal constructor` messages (probe
  `tools/probes/web-embedding.json`).
- Hooks for six internal methods. A class with hooks makes exotic
  objects: the heap's walks stop at them as at a proxy, the runtime's
  generic methods call the hooks, and the inline caches never store
  them. Host objects of classes without hooks (wrappers, the global
  object) stay ordinary and cacheable. ADR 0028 said that all host
  objects are uncacheable; that would leave every wrapper access
  uncached.
- `init_global` makes the realm's global object a `Window` host object
  with its prototype chain (and a named properties object on it). The
  global becomes a dictionary; global reads stay cached, +3 % on a
  micro row, `fib(27)` unchanged.
- The host state has a trace method and is a root. A native ends the
  task with `terminate(reason)`: uncatchable, and repeated at each time
  check until the outermost run returns. Event handlers compile as
  direct eval code with a captured chain of object environments
  (element, form, document; `@@unscopables` honoured); `toString`,
  `name`, the lookups and the early errors match Chromium. A script
  charges the real size of its source text; `set_heap_limit`.
- Tests: a test host (a class tree, a `Window` global with a named
  properties object, a legacy indexed object, a named-property object)
  and tests for termination, handlers and host data, all in both GC
  modes. test262 unchanged in both modes. jsbench geomean −0.4 %; two
  rows lose 15 to 20 % to a moved collection (the real source size, as
  M7 13e predicted).
- Review fixes: direct `eval` in a handler sees the scope objects; the
  `onerror` parameters are separated by ", " as in Chromium; a native
  that drops the termination error no longer lets the task end as a
  success; `Reflect.ownKeys` on a host object with hooks lists indices
  first; error messages name host objects by their constructor
  (`#<NodeList>`, `#<Window>`).
- Next: feature 4b, the WebIDL generator.

## 2026-10-23: M8 feature 3b, the engine for scripts

- `Page` is split into the page (history, pending navigation, loader,
  font context, viewport, pointer state) and a boxed `DocumentState`
  (the DOM and its parser, sheets, stylist, styles, layout, images, web
  fonts, requests, forms, scroll offsets, selection, element states). The
  box is `'static`, so feature 4c can lend it to a script runtime for a
  task (ADR 0028 section 2). Style, layout and the queries are its
  methods; the loader is shared through an `Rc`, the font context is an
  argument.
- With JavaScript on, the page parses with `HtmlParser` in steps (no
  script runs yet) and drains the change journal after each step:
  sheets, images and posters start to load as their elements are
  inserted. `Page::change_dom`, and a lent state that comes back, drain
  the same way. The drain reconciles the touched elements in tree order
  (a moved link keeps its sheet, a removed one loses it, a new `src`
  loads) and marks styles or the stylist dirty. Style and layout run on
  the next frame or query; a flush after no change does nothing.
- The stylist keeps each sheet parsed, so a rebuild makes only the rule
  index again; the `stylesheets` stage is 10 % faster on the large
  pages. Title, base URL, tree order and form controls (with dirty
  flags) are derived data keyed by the DOM generation.
- With JavaScript off, the parse and the one walk are as before. The
  snapshot does not change, also with `--js`. A test checks that the
  journal and the walk find the same loads for every fixture and layout
  test.
- Review fixes: with distinct image URLs, each parser step scanned all
  images (20,000 images with a script after each took 40 s; now
  0.25 s). A network completion that arrived while the state was lent
  was lost and the page never finished loading; completions now stay
  queued until the state returns. A removal scans the sheets only when
  the removed subtree held a `style` or `link`.
- Next: feature 4a, host classes and the embedding hooks in `js`.

## 2026-10-23: M8 feature 3a, the DOM for scripts

- The `dom` crate gets what scripts need (ADR 0028 section 3). A
  resumable parser (`HtmlParser`) drives html5ever's tokenizer and
  stops at each HTML `script` end tag; the caller keeps the document
  and lends it to each step. A test pauses at every script end tag of
  all fixtures and layout tests and gets the same tree as one parse.
- All changes go through change functions that follow the DOM
  algorithms (pre-insert, replace, replace all, remove, adopt,
  attributes, character data) and return `DomError`. Each change goes
  to a journal (drained by the engine), to an installed mutation sink
  (for mutation observers) and to a generation counter. An id index
  serves `getElementById`; `parse_fragment` parses with a context
  element; an arena holds more documents, and each node knows its node
  document.
- Limits per arena: 1,048,576 nodes and 256 MiB of text, names and
  attributes. The change functions return an error; the parser stops
  with a warning. Two new hostile cases reach them.
- Design changes, recorded in ADR 0028: the parser borrows the
  document per step; the sink lives in the document (html5ever's sink
  cannot hold a borrowed one without `unsafe`); the parse without
  scripts records no journal (recording costs 2 to 3 % of the parse
  time); the id index starts at the first lookup.
- `NodeId` stores index + 1 in a `NonZeroU32`, so links take 4 bytes;
  this pays for the new bookkeeping. Parse time +1 to +4 % against M7;
  the JS-off snapshot does not change.
- Review fixes: a lookup among many elements of one id walked siblings
  for each pair (1 s for 30,000 duplicates); it now walks the tree once.
  The journal overflows past 16 MiB of old attribute values. The byte
  limit counts names and a record size per attribute.
  `replaceChildren` on a document checks against the current children,
  as the DOM standard says.
- Next: feature 3b, the engine's document state and the parse in steps.

## 2026-10-22: M8 feature 2, the JavaScript setting

- The setting: `--js` (`--javascript on|off`), the session option
  `javascript`, `page.setJavaScript`, and `page.info` (`javascript`
  for the setting, `documentJavascript` for the shown document). In
  the GUI, a "JS" toolbar button and Ctrl+Shift+J. Off by default. A
  change applies to the next document; a reload applies it.
- Each document has a scripting flag (`Document::scripting`), so two
  pages in one process can differ. With it on: the parser reads
  `<noscript>` as raw text (its `<link>` and `<img>` do not load), the
  serializer and the DOM dump follow; `@media (scripting)` matches;
  `noscript` gets no box; a video shows controls only with `controls`;
  `<canvas>` is a replaced element (300×150 by default) that paints
  only its background and border. No script runs yet.
- Measured in Chromium (26 probe cases, all boxes equal): a `noscript`
  with an author `display: block` still gets no box; a video without
  `controls` paints only its poster (the dark default fill belongs to
  the controls); huge canvas sizes clamp.
- Stack (ADR 0028 section 2): the headless runner and server run the
  page on a thread with a 16 MiB stack. The GUI reads the process stack
  limit with `rustix` (now a direct dependency) and keeps JavaScript
  off below 8 MiB.
- The JS-off snapshot does not change. `compare --js` now runs swb with
  `--js`: senko-net and Hacker News keep geometry 1.0000; the
  Craigslist posting's pixel score goes from 0.6897 to 0.7747 (its
  `noscript` notice is gone).
- Next: feature 3a, the resumable parser and the change journal.

## 2026-10-22: M8 feature 1, test tools with JavaScript on

- `reference --js NAME` and `compare --js NAME` render the local
  fixtures with JavaScript on (`fixtures/js/`). References go to
  `out/js-reference/`, reports to `out/compare/js-NAME/`, scores to
  `out/js-scores.json`; nothing of it is committed. The reference waits
  for `load`, network idle (at most 15 s) and 1 s more, and does not
  scroll. Three runs give the same boxes on the four M8 fixtures.
- CORS in replay: the CDP `Fetch` domain pauses preflight requests, so
  the routing answers them itself (204, permissive) in capture and
  replay, and never sends them to the network. A response to a request
  with an `Origin` header gets that origin and
  `Access-Control-Allow-Credentials: true` instead of `*`, which
  credentialed requests reject. The new survey of Ars Technica and both
  Craigslist pages shows no CORS errors. The JS-off references do not
  change (byte for byte).
- Craigslist requests `manifest.js` with a version of
  `floor(Date.now() / 300000)` in the path. Two fixes: `replay_patterns`
  in `fixture.json` (one wildcard path segment; Python replay only for
  now), and a fixed start time: the page's `Date` starts at the
  fixture's capture time. Playwright's clock replaces the timers and
  `requestAnimationFrame` with its own code, so an init script shifts
  `Date` by a constant offset instead.
- Probe option `"javascript": true`. With `--with-swb`, the `js` queries
  of such cases also run in swb with `--eval EXPR`; docs/testing.md
  defines the output (one JSON line per expression), which feature 4c
  implements. Until swb has `--js` and `--eval`, the probe prints `not
  supported` and does not fail. `compare --js` reads swb's flags from
  `swb --help` and runs swb without JavaScript (with a warning) until
  feature 2 adds `--js`.
- Baseline of swb without JavaScript against Chromium with JavaScript
  (geometry / pixels): senko-net 1.0000 / 0.9990, hacker-news 1.0000 /
  0.9986, wikipedia-web-browser 0.1810 / 0.9735, craigslist-posting
  0.0000 / 0.6897.
- Next: feature 2, the JavaScript setting and the scripting flag.

## 2026-10-22: M8 step 2, ADR 0028 and the feature plan; new targets

- The owner changed the targets: Ars Technica and BBC use JavaScript
  mostly for ads, consent dialogs in frames and video, so they come
  later. Wikipedia stays without its video player (media playback is a
  milestone of its own). A survey of candidates with the step 1 tools:
  - github.com/senko/swb: 75 custom element definitions, 24 shadow roots,
    `popover`, module scripts that load about 130 files with `import()`:
    a later milestone for web components. It also found an swb CSS bug
    (the header menu buttons lose their padding; backlog).
  - A Craigslist search page: no special features, but about 140,000
    elements created and removed and 1,119 layouts in Chromium: it needs
    incremental layout. Later.
  - A Craigslist posting: 111 ms of script time, jQuery and Leaflet,
    nothing visible without JavaScript. Chosen as the third M8 target
    (target 6, fixture `craigslist-posting`, JS-off scores 1.000 and
    0.9994). Its hidden storage frame is not needed: the page renders
    the same in Chromium when the frame does not load.
  - Stack Overflow: a Cloudflare challenge page for headless Chromium and
    for swb. Dropped.
- ADR 0028 (written by a design session from the survey, the
  specifications and memo 5.3): the crates `web` and `webidl` (binding
  glue generated from WebIDL, checked in), one runtime per document, the
  document state lent to the runtime for each task, a resumable parser
  and a change journal, nodes that live as long as their document, one
  wrapper per node, the HTML event loop with 5 s per task, style and
  layout on demand, the CORS check, no frames and no media in M8.
- Owner decisions: the W3C Software and Document License for IDL data
  files (ADR 0003 update); the defaults of ADR 0028.
- The roadmap has the 12 features of M8: test tools, the setting and the
  scripting flag, the DOM and engine for scripts, the embedding, events,
  the event loop and the parser, the DOM core and input events (Hacker
  News), style and layout from scripts, window and document APIs,
  observers and forms, network, the Craigslist posting, Wikipedia.
- Review: no blocking error in the ADR. Fixed: the event core moved
  before the event loop (5a), the DOM core (6a to 6d) and network (10a,
  10b) split into sessions; `readyState` follows "the end" of the
  parser; mutation observers get their records at each primitive change
  (a sink of the `dom` change functions), not from the journal; the
  global object stays cacheable for the inline caches; the stack-limit
  check needs `rustix` as a direct dependency; `document.write`, the
  CSSOM sheet API and the canvas context come with later targets; the
  deviations (synchronous XHR, `write` after the parse) are stated; the
  timer clamp threshold is measured in Chromium first.

## 2026-10-22: M8 step 1, the script survey

- Owner decisions: the M8 targets are the five target pages with
  JavaScript on (they render like Chromium with JavaScript on, no script
  error breaks them, their JavaScript features work); fixtures with
  JavaScript on stay local (`fixtures/js/`, git-ignored), because they
  contain the sites' scripts. Recorded in the ground rules and the new
  M8 section of the roadmap.
- Tools: `capture --js` records a page with JavaScript on into
  `fixtures/js/NAME/`. `js-survey` replays it in Chromium four times:
  with a tracer (an init script that wraps the members of the Web API
  interfaces and counts calls, reads, writes and `new` per phase), with
  JIT and with `--js-flags=--jitless` for the DevTools performance
  metrics, and with JavaScript off for the screenshot difference. Each
  run has a 300 s limit.
- Report: `docs/js-survey.md`. Senko.net and Hacker News need no
  JavaScript. Wikipedia: 14 scripts (1.1 MB, all first-party), 176
  members, no errors in replay; features: appearance (dark mode, text
  size, width), pinned menus, collapsibles, a sticky header, reference
  previews, search suggestions, the video player. Ars Technica: 214
  scripts (5.2 MB, mostly third-party), 364 members, 3,141
  `getComputedStyle` calls during parse, `document.write`; a consent
  dialog covers the page. BBC: 93 scripts (5.1 MB, 60 % its own), 272
  members, 955 ms of script time in Chromium without JIT; a consent
  dialog in a cross-origin iframe covers the page.
- Review: no blocking findings. Fixed: the per-run time limit (a page
  that blocks its main thread stopped the survey), a test that the
  tool's own calls are not counted, three limits of the method in the
  report.

## 2026-10-22: M7 maintenance; end of M7

- Feature 14 (modules) is postponed: of the script tags of the five
  target pages, only one BBC script has `type="module"`, and it has no
  `import` or `export`. The roadmap says how the engine integration
  runs it.
- M7 exit: test262 subset 40,286 pass, 43 fail (stress the same),
  parse-only 42,240. The roadmap lists each failure with its reason and
  the skipped tests in the scope of ES2025 (174 `cross-realm`, 68 that
  need `nativeFunctionMatcher.js`, the module tests).
- Two reviewers read the JavaScript crates, the Python generators and
  the docs; two implementer sessions applied the lists.
  - Duplicates removed: the test helpers of the hostile tests (one
    `tests/common` module), the `@@species` getter (6 copies), the
    incompatible-receiver and not-a-function messages, the
    relative-index clamp, `syntax_error`, the "Invalid regular
    expression" message, the compile error constructors, the parse
    error kind mapping, the line-terminator sets of the compiler, the
    shared parts of the Unicode table generators (the tables are
    byte-identical).
  - Dead code removed: `ParseError::unsupported` (always empty), two
    intrinsics that nothing read, test-only helpers. The `js` crate
    re-exports only what an embedder names.
  - Splits along existing seams: `bytecode/` (verifier, eval
    metadata), the compiler's assignments, calls and statements, the
    Map and Set operations (`object/collection.rs`), `@@replace`
    (`regexp_replace.rs`).
  - Docs: ADR 0026 no longer describes the spike as future work, says
    how built-ins are defined, and marks section 12 as the design of
    the engine integration with what exists now; `docs/testing.md` has
    all shell options; stale comments that named features as future
    work are gone.
- Two small fixes: the test262 runner reads the front matter of files
  with CR line ends (one test that needs `nativeFunctionMatcher.js` is
  skipped now instead of failing); the compiler's position scan skips
  every white space character between tokens, as the lexer does.
- Left: the regexp matcher's line terminator test, the digit value
  helpers of the lexer and the `await` path (hot paths), and the split
  of ADR 0026 (its sections are referenced by number).
- `vm_bench`, geometric mean per run: session 1 0.996 and 0.999 of 13e,
  session 2 0.998 to 1.004 of session 1. No row was slower by more than
  5 % in both runs except `date: new Date(y, m, d)` (6 to 8 %), which
  runs no changed code and has the same instruction count in
  callgrind.

## 2026-10-22: M7 feature 13e, compiler and memory; end of feature 13

- Dead-zone checks: the scope analysis drops the check for a reference
  in a closure that is created after the declarator (not a function
  declaration, which is created at scope entry; not in a `switch`
  scope). ADR 0026 section 4 has the rule and its reasons. Five
  expectations of the scope tests lost a check; three captured
  constants read in a closure loop take 8 % less time.
- Source text: scripts with only Latin-1 units already kept one byte per
  unit. Seven of the 60 BBC scripts have 1 to 130 wider units (quotes,
  the euro sign) and kept two bytes for every unit. `SourceText` keeps
  one byte per unit and a sorted list of the wide units: 4.71 → 3.54 MB.
- Array patterns on a plain array use the array and an index in
  registers in place of an Array Iterator. The shortcut is valid at the
  watch epoch of 13a while the slots of `@@iterator` and `next` hold the
  built-in functions and no object of the chain has `return`. If a
  `return` appears while the pattern runs, the close makes the Array
  Iterator at the stop index. `const [a, b] = pair` 288 → 52 ms
  (Node.js 76).
- Measured and dropped: charging the real size of the source text in
  the heap accounting moved a collection and made the `toLowerCase` and
  `regexp replace` rows 13 and 25 % slower with equal instruction
  counts.
- `out/notes/stress-9b-r1.js` runs in 0.2 s with `--stress`; the 13.5
  hours do not reproduce. Stress mode collects at each allocation, so
  its time grows with the live heap; the weak structures add nothing.
- 13f (lazy compilation) is not started: the BBC scripts compile in
  176 ms, and the parse with the scope analysis (75 ms) would stay.
- Geometric mean 0.957 of 13d over 108 programs; 2.05 times
  `node --jitless` (3.4 times at the start of feature 13). test262
  unchanged.
- Review: no correctness defect. About 200 dead-zone cases and 190 array
  pattern cases (prototype changes before and during the pattern,
  `return` added from defaults and getters, `yield` in a default with
  `return()` and `throw()`) gave the Node.js output, also in stress
  mode. No row moved by more than 5 % in both A/B runs except
  `regexp: test` (+3 to 5.5 %, a row that runs none of the changed
  code). Fixed: the line index of a Latin-1 script copied the whole
  text; the close of a pattern record without its array now raises the
  same internal error as the step. Backlog: a second shape in the
  pattern cache; the catch parameter pattern has no dead zone (also at
  13d).

## 2026-10-22: M7 feature 13d, strings and collections

- The rest of feature 13 is split into 13d (strings and collections),
  13e (compiler and memory) and 13f (lazy compilation, only if needed).
- Callgrind on the string rows: a method read on a string cost about
  290 instructions (the general `[[Get]]`), a short copy 276, and
  `padStart` made six strings. Method reads on strings, numbers and
  booleans and the `length` of strings now have a site cache entry.
  Strings of one unit below 256 come from a table of atoms. Local fixes:
  exact-size copies, one-pass case conversion, `padStart` in one
  buffer, a slice scan in `indexOf`.
- `Map` and `Set` string keys keep 32 bits of their keyed hash in the
  padding of the string. `toFixed` with up to 20 digits computes in
  128-bit integers (290 → 121 ns) and agrees with the exact path on 30
  million random doubles.
- Dense elements have a start offset, so `shift` takes constant time
  (100,000 shifts 3,500 → 11 ms). The first version put the offset next
  to the element count and made every object 8 bytes larger; the
  `record_sizes` test caught it, and the count moved into the new
  vector type.
- `indexOf` 186 → 111 ms, case conversion 478 → 279, `for`-`of` over a
  string 69 → 34; geometric mean 0.86 of 13c, 2.21 times
  `node --jitless`. test262 unchanged; new tests in
  `tests/small_items.rs`.
- Review: no correctness defect (the start offset in every element path
  and in the collector, shifted objects collected, caches on primitives
  after prototype changes, all bitwise results, Latin-1 case mapping,
  `toFixed` against the exact path). It traced the 10 % loss of
  `array: read-modify-write` to the integer results of `|` on doubles:
  with `Value::Int` on the double path, the integer paths of `|`, `&`,
  `^` and the shifts were 25 % slower (also with the double path out of
  line); with `Value::number`, a loop of bitwise operators on doubles was
  8 % slower. The item gained 1 to 3 %, so 13d drops it. Also fixed: the
  `split` pieces of two-byte units took a handle each.

## 2026-10-22: M7 feature 13c, callbacks from natives

- Callgrind on `forEach` showed 1,397 instructions per element: 686 in
  the re-entry of the interpreter (`Runtime::call`) and 460 in the
  search of the prototype chain for the next index. Natives that call
  script functions in a loop are now native loops: the native returns
  its state, the interpreter runs its steps, and each call is a frame of
  the same interpreter loop (908 instructions per element). The state's
  heap values live in registers on the value stack, so a step needs no
  rooting. An element in the receiver's dense vector skips the chain
  search.
- Converted: the array and typed-array callback methods, `sort` with a
  comparator, `replace` with a function. Proxy traps, promise jobs and
  the consumers of iterator helpers were tried as loops and were not
  faster (a loop with one call costs as much as a re-entry; the helper
  consumers lost to instruction cache misses), so they keep the
  re-entry.
- Iterator results have one shape per realm; `for`-`of` over a generator
  resumes the frame and takes the yielded value without the result
  object (266 → 69 ms for 1M values); String and `RegExp` String
  Iterators step without result objects (`matchAll` 150 → 88 ms).
- `forEach` 66 → 51 ms, `reduce` 71 → 49, `filter` 77 → 57; geometric
  mean 0.97 of 13b. Recursion through callbacks now ends at the frame
  limit (4,999 levels instead of 408). test262 unchanged; new tests in
  `tests/callbacks.rs`.
- Review: about 430 probe cases (step order on proxies, arrays and typed
  arrays changed by the callback, sort comparators, `replace` callbacks,
  exception routing through nested loops and re-entries, the `for`-`of`
  fast paths with patched `next`) give the same output as 13b in normal
  and stress mode; deep recursion through callbacks ends with a
  `RangeError` (peak RSS at 4,999 levels 11.6 MB). One defect fixed: an
  uncaught error from a callback reported the offset of the loop's call
  instead of the throw (only the `swb-js` shell shows it). One nit
  fixed: the array and typed-array loops share `Found` and `Method`.

## 2026-10-22: M7 feature 13b, call paths

- Callgrind on `fib(27)` showed 1,089 instructions per call, about 450
  of them in the call and the return, and a `Value` copied in five
  pieces at every register access. The call now reads the callee in one
  lookup, binds a sloppy `this` in place, checks the limits together and
  drops the frame record in place; the check for a due collection is
  one comparison; a native call pushes one handle scope; `Value` has an
  8-byte tag (two moves per copy). `Root` stays at 16 bytes: it records
  property keys as their string or symbol values.
- Arguments objects after the first of a realm are made from a template
  in one step (7,414 → 2,940 instructions per call).
- fib 42.6 → 29.8 ms, closure counter 64 → 47 ms, calls with
  `arguments` 513 → 214 ms; geometric mean 0.85 of 13a. The interpreter
  loop is not sensitive to code alignment, and a smaller `match` was
  slower, so the layout stays; the 8 % fib loss of 7d did not
  reproduce. test262 unchanged; new tests in `tests/calls.rs`.
- Review: no defect (GC rooting of keys without `Root::Key`, every path
  that changes the live size, call semantics, arguments templates after
  changes to the first object; peak memory within 2 MB of 13a under a
  64 MiB limit). Two nits fixed: a comment on an unreachable arm, one
  local for the template condition.

## 2026-10-22: M7 feature 13a, property access caches

- Feature 13 (performance) starts with a baseline: 95 `jsbench`
  programs at a geometric mean of 3.4 times `node --jitless`. The slow
  rows come from property access, object creation, calls, calls of
  natives and callbacks from natives; the roadmap splits the feature in
  sessions 13a to 13e by these causes. Startup is 2 ms, so lazy
  built-ins are not needed.
- Every named access, global access and object literal has a site of
  its own now, with one cache entry. An entry checks the receiver's
  shape handle (with its generation, so a reused shape slot never
  matches); entries that depend on the prototype chain also check a
  watch epoch that each change of a prototype's or the global object's
  keys, attributes or prototype increments. A miss walks the chain once
  and fills the entry. Global lexical bindings keep their position;
  global-object properties depend on the epoch and the number of
  lexical bindings (a later `let` can shadow them). Literals with plain
  data properties are made in their final shape.
- `o.x` loop 367 → 135 ms, 1M small objects 239 → 76 ms, fib 63 → 42
  ms; geometric mean 0.86 of base. test262 and the target scripts are
  unchanged.
- The faster literals exposed an arena effect in a hostile test: objects
  allocated at the end of a long slot vector keep its capacity counted
  as live (backlog).
- Review: no correctness defect (the reviewer compared hundreds of
  programs on prototype, attribute, global and literal changes with the
  base build, Node and stress mode). A megamorphic site is faster than
  before; a loop that changes a prototype in each iteration is 7 to 15 %
  slower (each change increments the epoch). Fixed: an add to a watched
  object no longer fills an entry that can never hit; a comment on why
  entries filled before a setter runs cannot go stale; the credits.

## 2026-10-21: M7 feature 12b, Date.parse forms and zone names; end of feature 12

- The first session's reader of `Date.parse` matched Chromium on
  130,000 texts, but its design (an ISO phase, a token scanner with a
  keyword table, a digit limit, objects that compose the day, the clock
  and the zone) and the equivalent-year formula came from the author's
  memory of V8's source. Recalled engine code counts as read code (ADR
  0025), so the owner chose a rewrite, as in feature 4. The CLDR zone
  names of that session stayed; the reader was removed before the
  second session started, and its scratch work was not given to it.
- The second reader is a grammar of named forms, each with a source:
  ISO 8601 and RFC 3339 for the ISO variants, RFC 5322 (with the
  two-digit year rule of section 4.3), RFC 850, `asctime`, and probes
  in Chromium 148 for the written English and numeric forms. Texts
  outside the forms are NaN. On about 600,000 compared texts swb never
  gives a value where Chromium gives NaN or another value; the cost of
  the rewrite is NaN for Chromium's quirks (a number alone, minute 60,
  stray tokens): 501 snippets KNOWN in six classes.
- Zone names: Chromium takes the metazone of the present, not of the
  printed date, and decides the saving flag outside 1970 to 2038 at an
  equivalent year. The years were measured with seven zones whose rules
  change from year to year: one year fits each of the 14 kinds of year.
- test262 runtime unchanged (40,286); the targets behave as before.
- Review: one defect, fixed: a non-ASCII unit glued to the date in the
  free text gave a value where Chromium gives NaN (`ĀJul 1 2024`). Probes
  showed that Chromium counts non-ASCII units as word characters, skips
  every word but a month before the first number (also `GMT`, `PM`,
  `T`), and reads a signed six-digit year only at the very start of the
  text. swb now does the same; a leftover keyword `T` and a number cap
  that looked like the withdrawn design are gone. About 260,000 texts
  compared again: no value where Chromium has NaN, no different value.

## 2026-10-18: M7 feature 12a, Date

- Feature 12 (`Date`) is split into two sessions: 12a the time values
  and the built-in, 12b Chromium's other `Date.parse` forms and the long
  zone names of `toString`. There is no `Intl` in M7, so
  `toLocaleDateString("en-GB", {...})` in the target scripts goes to
  the backlog; without arguments the `toLocale*String` methods give
  Chromium's en-US text.
- The calendar is exact in integers over the whole range of time
  values; `MakeDay` and `MakeTime` work in doubles as the spec says.
- Time zones come from `jiff` (ADR 0027), which was already in the lock
  file through `env_logger`; `tz-rs` is smaller but has no API for
  skipped and repeated local times. Outside `jiff`'s range of years
  the zone code shifts by whole 400-year cycles. The host sets the
  clock and the zone; the test262 runner uses a POSIX rule with
  daylight saving time, so the scores do not depend on the machine.
- The probe tool has a `time_zone` setting. 11,670 Chromium results in
  seven zones (UTC, Berlin, New York, Kolkata, Adelaide, Lord Howe,
  Apia) are a test, also in stress mode. Chromium differs from the
  spec in a few places (KNOWN): `setFullYear` reads the time value
  after the conversions, `getTimezoneOffset` drops the seconds of old
  local mean times, `Date.parse` rolls "2000-02-30" into March.
- test262 runtime 39,515 → 40,286; `built-ins/Date` and
  `annexB/built-ins/Date` pass all tests that run. bbc-js/s5.js now
  runs to its end.
- The review found no defect. Nits fixed: a shared table of weekday
  names, a section number, imports, an `expect` for the fixed test
  zone. To the backlog: the snippets with instants after 2026 depend
  on the future rules in the system tzdata, the size of the test data,
  and the KNOWN items that pages see as V8 does them.

## 2026-10-18: M7 feature 11c, typed-array methods, Atomics, base64; end of feature 11

- The 25 remaining methods of `%TypedArray%.prototype`, `Atomics` on
  `ArrayBuffer`s and the base64 and hex methods of `Uint8Array`. This
  ends feature 11. test262 runtime 38,347 → 39,515, and 36,369 → 39,515
  over the three sessions.
- Loops without a callback run on the bytes of the buffer in chunks of
  16,384 elements and charge the time countdown after each chunk, so a
  `for (;;) big.sort()` over 64 MiB ends at the deadline. A first
  `indexOf` read one value per element and took 75 ms for 20 searches
  over 1M elements; the chunk loops take 4.4 ms, as `node --jitless`.
- The default sort counts the one-byte types and the two-byte types of
  262,144 elements or more, and sorts keys of the others (below that
  size the keys are faster than a table of 65,536 counts); NaNs go
  after the numbers with their bit patterns. The sort
  with a comparator is a top-down merge sort whose comparator calls
  equal Chromium's (435 measured sequences; `Array.prototype.sort` calls
  the comparator in another order).
- 8,263 snippets measured in Chromium 148 are a test, also in stress
  mode. The differences that stay: the `toLocaleString` arguments
  (ECMA-402), a content-type check that V8 does not make, `setFromBase64`
  into a full target (V8 reads on; the proposal and test262 stop), and
  the text of a few objects in messages.
- `Atomics` and `Uint8Array` are in the test262 subset. The runner skips
  tests that make a `SharedArrayBuffer`, which Chromium 148 does not
  have on a normal page.
- The review found no defect in memory safety or writes after user code.
  Fixed: the default sort of floats reserved 8 bytes per element for
  the NaN list, so `new Float64Array(2**25).sort()` was a RangeError at
  the default heap limit; small arrays of two-byte types cleared 65,536
  counters per sort (20,000 sorts of 8 elements took 1.7 s);
  `fromBase64` reserved 3/4 of the string length before it read
  anything. A delta review checked the fixes.
- A timing test in `language_hostile.rs` (a `parseInt` loop against a
  deadline) fails about 1 run in 6 when its file runs in parallel, also
  at 788905a. It is in the backlog.

## 2026-10-18: M7 feature 11b, typed arrays

- The 12 typed arrays and `%TypedArray%`, with every argument form of
  the constructors, length tracking over resizable buffers and V8's
  messages. A typed array is one more object kind over the `BufferView`
  of 11a. A canonical numeric key is an element or no property, and
  never walks the prototype chain.
- The heap answers what runs no script code. Conversions and `BigInt`
  elements go to the runtime through the same `Generic` results as
  proxies.
- Performance: keeping the other paths fast took three rounds with
  callgrind. The kind tests are now one comparison of adjacent enum
  variants, and the typed paths are out of line. `Array` loops and `o.x`
  changed by at most +0.7 % instructions (a first version cost +7.7 % on
  `o.x`). `ta[i]` loops run as fast as `Array` loops.
- Spec over V8 in four places (KNOWN), among them the order in which
  `new T(typedArray)` reads its source. The hostile callback cases use
  Chromium's output: Node.js 22 (V8 12) differs from Chromium 148 there.
- The test262 runner accepts `dir` lines below a group, so the method
  tests of 11c stay out until 11c. test262 runtime 37,011 → 38,347; the
  25 failures of 11a pass.
- The review found one defect: a `start` conversion in `slice` that
  shrinks a length-tracking array to no element made the empty copy an
  internal error (Chromium returns the new array). A copy of 0 bytes now
  checks no range; a test covers it. The byte-by-byte copy of an
  overlapping `slice` is now charged before the work.

## 2026-10-18: M7 feature 11a, ArrayBuffer and DataView

- Feature 11 (binary data) is split into three sessions: 11a buffers,
  11b typed arrays, 11c the other typed-array methods, `Atomics` and
  base64/hex.
- An `ArrayBuffer` owns its bytes as a Rust vector outside the arenas
  (`object/buffer.rs`). The heap counts the vector's capacity, so
  buffers count in the heap limit and the collection threshold; a
  detached buffer gives its bytes back at once, `transfer` moves the
  vector. Allocation reserves first and then allocates fallibly. A
  buffer that does not fit is a `RangeError` ("Array buffer allocation
  failed"), as in V8, not a termination.
- Zero-fills and copies charge one step per 128 bytes. Endless loops of
  64 MiB and 256 MiB operations end within 21 ms of a 200 ms limit.
- `DataView` converts the arguments first and then reads or writes in
  place, so a `valueOf` that detaches or shrinks the buffer is seen.
  Reads of Number types allocate nothing. The element types and their
  byte encoding (with binary16 in `js-text`) are ready for the typed
  arrays of 11b.
- Probes: Chromium 148 has no `SharedArrayBuffer` in a normal page
  (`crossOriginIsolated` is false) and no immutable buffers, so swb has
  neither. V8's limits (2^35 − 1 bytes, `maxByteLength` 2^35 − 4096) and
  all messages come from 733 probe snippets; 724 are equal, 9 KNOWN.
- The review found no defect. One nit: a buffer could take the heap to
  the full limit, but a collection ends the script above 90 % of it, so
  a script that kept buffers and dropped them after the `RangeError`
  ended with a termination. The room for buffers now ends at the same
  90 % line.
- test262 runtime 36,369 → 37,011. The 25 new failures call typed array
  constructors without the feature tag; they need 11b.
- `getInt32` loops are 5 times slower than `node --jitless`, almost all
  of it the cost of a native call (feature 13).

## 2026-10-18: rustls 0.23.46

- `just audit` reported three advisories for rustls 0.23.45
  (RUSTSEC-2026-0346, 0347, 0348). None affects swb: it has no custom
  session store, no ECH and not the `brotli` feature. 0.23.46 fixes all
  three; only `Cargo.lock` changes.
- One informational advisory remains: `ttf-parser`, through winit, is
  unmaintained (backlog).

## 2026-10-18: M7 feature 10b, BigInt in the language; end of feature 10

- Scripts can use `BigInt`. Literals are constants of the code object
  (`-1n` is one constant) and give their decimal text as property keys.
  A literal above swb's size limit is a `SyntaxError` before the
  compile, as V8 gives one above its own limit (measured, also in
  functions that never run).
- The interpreter's integer and double fast paths stay as they were:
  all other operands already took the general paths, which now apply
  `ToNumeric` and call the arithmetic of `js-text`. Every operation is
  charged to the time countdown (16 work units per step) before the
  work, and every result to the heap. A first version made `i++` three
  instructions slower through a shared store; the integer arms store
  directly now (`sum 10M integers` −5 % instructions).
- `BigInt` with `asIntN` and `asUintN`, the prototype methods, wrapper
  objects, `ToBigInt`, `ToBigInt64` and `ToBigUint64` (for the typed
  arrays of feature 11), the comparisons with Numbers and strings (exact,
  both string widths), V8's messages, `console.log` as Node.
- Comparisons by value: an audit found no comparison of two values by
  handle; all go through `Heap::equal` or the table's SameValueZero.
- The review found one defect: the decimal text of a `BigInt` in an
  error message was not charged. An endless loop of `big in big` with a
  value at the limit (35 ms per text) never reached a time check. The
  text is built behind a shared borrow, so the countdown is now a `Cell`
  and the text lowers it without a check; the next step runs the check.
  All 165 error paths of the review end within 40 ms of the deadline
  (one with a collector pause, 130 ms). Also from the review: the
  operators charge before the work (they charged after), and one
  message constant is shared instead of copied.
- Tests: 850 snippets equal to Chromium 148 in normal and stress mode,
  Node's `console.log` forms, hostile cases. test262 runtime 36,000 →
  36,369; ars-js/s02.js now takes its BigInt branch.
- Next: feature 11, binary data.

## 2026-10-18: M7 feature 10a, BigInt arithmetic

- Feature 10 is split into 10a (the arithmetic) and 10b (the language).
  One target script needs BigInts: protobuf's 64-bit integers on Ars.
- The arithmetic is a library module of `js-text` (`bigint/`), without
  heap or engine types; the engine's placeholder type is now the
  library's type. Sign and magnitude in 64-bit limbs, normalized.
  Schoolbook multiplication; Knuth's Algorithm D for long division and
  a short path for one-limb divisors, both with Möller and Granlund's
  reciprocal division (the hardware 128-by-64 division is a slow library
  call). Shifts and bitwise operators work on the two's complement form
  and read only the limbs that the result depends on.
- Size limit: 2^18 bits. At the limit the slowest operation, `toString`
  in radix 10 (quadratic), takes 32 ms in a release build; 2^19 bits
  would give 126 ms. Chromium allows 2^30 − 64 bits (KNOWN). Every
  result above the limit fails before work in proportion to its size,
  all allocation is fallible, and each operation has a cost in work
  units for the time countdown.
- `Natural` of the number conversions stays separate: sharing would put
  fallible allocation and 64-bit limbs into the hot paths of `toFixed`
  and `parseInt`.
- Tests: 6,636 reference cases from Python's `int`, 40 of them with
  Algorithm D's add-back step (found by a simulation of the quotient
  estimate in the generator).
- The review found no defect. Its own check of 40,352 cases against
  Python, in a debug build with overflow checks, gave no difference. Its
  nits: a right shift costs more per limb than the cost model charged
  (now 3 units per limb), an overflow guard in the parse cost, and a
  sentence in performance.md about the limbs that a result reserves.
- Next: 10b, BigInt in the language.

## 2026-10-18: M7 feature 9b, weak collections; end of feature 9

- `WeakMap`, `WeakSet`, `WeakRef` and `FinalizationRegistry`. The weak
  tables reuse the hash table of `Map` with handle keys; symbols that
  are not registered are keys too.
- The collector marks `WeakMap` entries as ephemerons in a second
  phase, only when a `WeakMap` is live. Each entry is visited once, and
  a value whose key is not marked yet waits in a list under the key
  until the key is marked, so long chains cost linear time in any
  order. The sweep removes dead entries, clears dead `WeakRef` targets
  and moves the cells of dead targets to the registry's pending list
  before any slot is freed.
- A first version checked for waiting values at every mark and made
  collections 10 % more expensive for heaps without `WeakMap`s. The
  check now exists only in the second phase, and the drain skips the
  kind dispatch for plain objects: collections execute 7 to 12 % fewer
  instructions than before.
- `ClearKeptObjects` runs at the end of `run_jobs`, once per microtask
  checkpoint as HTML does. Cleanup callbacks run in a host task
  (`run_finalization_cleanup`), never during a collection, and a
  registry that script dropped calls nothing.
- test262 runtime: 35,666 → 36,000, the same in stress mode; no failure
  in the four groups. 318 snippets give Chromium 148's results, with no
  difference from the spec.
- The review found no defect. It checked that every mark of a key in
  the second phase wakes its waiting values, the sweep order, rooting
  in callbacks that collect, and 47 more snippets in Chromium. Its nits:
  `register` now reserves its memory before it stores the cell, and the
  docs say that the host clears the kept set also after a script that
  threw. The unbudgeted side memory of the marking went to the backlog.
- Feature 9 is done: test262 runtime 35,233 → 36,000.
- Next: feature 10, `BigInt`.

## 2026-10-17: M7 feature 9a, Proxy

- Feature 9 is split into 9a (`Proxy`) and 9b (the weak collections).
  Four target scripts use `new Proxy`: a reactive store on Ars, a lazy
  object that forwards all 13 traps and a module namespace on BBC.
- Proxy objects have the 13 internal methods of §10.5 with every
  invariant check and V8's messages, and `Proxy.revocable`. The heap
  stays free of script code: its prototype walks stop at a proxy before
  any effect, and the runtime takes the generic path (`vm/proxy.rs`).
  Heap methods that change objects refuse a proxy, so a missed call
  site gives an internal error, not a wrong write.
- Calls through a proxy without a trap are tail calls. The other proxy
  levels charge the shared recursion budget (2 KiB per level, measured
  at most 960 B), so a chain of a million proxies gives a `RangeError`,
  not a stack overflow.
- swb follows V8 where the spec leaves the order to the implementation
  (`for-in` over proxies) and where V8 throws (the error names the trap
  or the target that returned `false`). It follows the spec in two
  orders of `with` that V8 does not.
- test262 runtime: 35,233 → 35,666, the same in stress mode;
  `built-ins/Proxy` 273 pass, 0 fail. 672 snippets give Chromium 148's
  output except five (KNOWN). Hostile tests: chains, traps that call
  themselves, endless prototype chains through traps, a million keys.
- The review found no defect. It checked about 130 paths that read
  objects and called every built-in function with five kinds of proxy
  (5,940 calls, also in stress mode): no internal error. Its nits: a
  conversion that read an unexpected proxy as an empty object is now an
  internal error, and one record lookup less per new property. Two
  spec details of globals and the chain limit went to the backlog.
- Next: 9b, the weak collections.

## 2026-10-17: M7 feature 8c, Unicode property sets; end of feature 8

- `\p{...}` and `\P{...}` have their code point sets. A generator in
  `tools/swbtools` writes the sets of General_Category (with the
  groups), Script, Script_Extensions, the binary properties of ECMA-262
  table 66 and the properties of strings of table 67 from the UCD and
  emoji files 17.0.0. They are delta-encoded ranges in `js-text`, about
  82 KB of the binary, decoded when a pattern uses them.
- The parser builds each set once per property, `\P` and `i`, and keeps
  one node for repeated escapes, so 10,000 `\p{L}` with `iu` compile in
  1 to 2 ms. The set-node cache of 8a has no per-set limit now; a tree
  holds at most 2^22 set ranges.
- With `v` and `i` a lone General_Category value such as `\p{Lu}` is
  case folded as in Chromium. The spec text leaves it unfolded, but it
  folds `\p{gc=Lu}`, the same value; this looks like an omission in the
  text.
- The review found that `\p{RGI_Emoji}` (about 3,800 strings) was an
  alternation tried at every position: a million characters took 43 s.
  Classes with more than 8 strings and the properties of strings now
  compile to a trie with a sorted table per node, and the prefix scan
  skips positions that cannot start a string: 5 ms. A node where a
  string ends tries the longer strings first, so the order of the spec
  stays. Lookbehind uses a trie of the reversed strings. A delta review
  compared the trie with the alternation in the spec order in 36,960
  matches and found no difference.
- 299 probe cases with `\p` give Chromium 148's results, except six
  cases of `\p{ASCII}` with `iv` and U+017F (ſ) or U+212A (Kelvin
  sign): the spec folds them into the set, Chromium does not. Chromium
  agrees with Unicode 17.0.0 on all property sets of the cases.
  test262 runtime: 35,233 (was 34,707), the same in stress mode;
  `built-ins/RegExp` has 1,854 passes and 1 failure (needs `Date`).
- The stress mode now collects at every safepoint only for the first
  65,536 of a run, then less often. The generated property tests loop a
  million times, and the stress run of test262 took 75 minutes; now it
  takes about 10.
- Feature 8 summary: test262 runtime 32,775 → 35,233; target scripts
  that compile 46 → 84.
- Next: feature 9, Proxy and the weak collections.

## 2026-10-17: M7 feature 8b, the `RegExp` built-in

- Regular expression literals compile. A literal is compiled at its
  first evaluation and the program stays in the literal's constant;
  each evaluation makes a new object. All 84 target scripts compile now
  (were 46). Without a DOM, 56 of them run to the end; the others stop
  at host objects (`window`, `document`), typed arrays or `Date`.
- `RegExp` has the constructor, `exec`, `test`, `toString`, `source`,
  the flags, `compile`, `RegExp.escape`, the five symbol methods with
  the RegExp String Iterator, and the legacy static properties
  (`RegExp.$1` and the others) as V8 has them. `String.prototype` has
  `match`, `matchAll` and `search`, in V8's key order.
- The matcher runs in slices of 32,768 steps. Between two slices the
  time check and a safepoint run, so catastrophic backtracking stops at
  the deadline, and a collection during a match changes nothing. A
  backtrack stack beyond 256 MiB, or beyond the room under the heap
  limit, is V8's `RangeError` "Maximum call stack size exceeded".
  Chromium throws it at about ten million loop iterations, swb at about
  four million, because a choice point is larger (backlog).
- The global loops of `match`, `replace` and `split` run natively when
  the `RegExp` object, `RegExp.prototype` and `RegExp` keep their
  recorded shapes. Then no skipped step can be observed.
- 453 snippets give Chromium 148's output, also in stress mode. swb
  follows the specification, not V8, in the read order of `flags` and
  for a RegExp String Iterator after its `exec` throws.
- test262 runtime: 34,707 (was 32,805), the same in stress mode;
  `built-ins/RegExp` 1,333 pass, 1 fail (needs `Date`), 531 unsupported
  (`\p{...}`, 8c). Parse-only unchanged (42,240).
- Speed against `node --jitless`: a global `replace` 0.5 times, `exec`
  loops 2 times, `split` 3 times, `match` 7 times, `matchAll` 16 times;
  all linear.
- Review: about 200 more snippets measured in Chromium, all the same
  apart from the documented differences. Fixed: `@@split` took its fast
  path before it converted `limit`, so a `valueOf` of the limit that
  replaced `exec` was not seen; an object limit now takes the spec
  path. The heap accounting of shared programs, the time limit in all
  symbol methods and 10M-unit inputs had no findings.
- Next: 8c, the code point sets of `\p{...}`.

## 2026-10-17: M7 feature 8a, the regular expression engine

- Feature 8 is split into three sessions: 8a the engine, 8b the
  `RegExp` built-in and the string methods, 8c the Unicode property
  sets.
- `js-regexp` parses patterns in the grammar of `u` and `v` and in the
  web grammar of Annex B.1.2, with all early errors and V8's messages.
  The parser keeps open groups and nested classes on an explicit stack;
  the check of a literal builds no tree.
- The compiler emits a backtracking program: enum instructions, counter
  registers for counted quantifiers, one-instruction loops for atoms of
  one character, backward code for lookbehinds. The matcher is a loop
  with a backtrack stack and an undo log.
- The matcher stops after a given number of steps and continues on the
  next call with the same state. 8b runs the time check and the heap
  check between two calls, without a borrow of the input string. The
  backtrack memory stops at a limit that the caller gives.
- Case-insensitive matching canonicalizes classes at compile time: the
  toUppercase rule without `u`, simple case folding with `u` and `v`.
  `\p{...}` names are checked from tables generated from UCD 17.0.0;
  their code point sets come in 8c.
- 475 matches, 491 patterns and 46 literals give Chromium 148's
  results, also in one-step slices, apart from seven cases where swb
  follows the specification and Chromium does not (`\q{...}` with `iv`,
  the script value `Hrkt`). test262 parse-only: 42,240 (was 41,882);
  runtime: 32,805 (was 32,775), the same in stress mode. All 84 target
  scripts parse, and their 1,142 literals compile.
- Review: the matcher gave Chromium's results on about 690,000 cases.
  Three kinds of pattern made one compile take seconds to minutes and
  up to 10 GB: a `v` class of 100,000 characters (35 s), 20,000
  `\k<name>` of a duplicate name (3.1 GB), a million `\W` with `iu`
  (107 s). A class now merges its ranges once, the references of one
  name share one group list, and the parser keeps one node for each
  small set. All three now take milliseconds. Also fixed: backtracking
  counts each popped frame as a step, and the undo log checks the memory
  limit before it grows. The delta review found no bug; one message
  difference and a modifier difference in Chromium went to the backlog
  and the ADR.
- Speed: most benchmark patterns take 0.3 to 1.5 times the time of
  `node --jitless`.
- Next: 8b, the `RegExp` built-in.

## 2026-10-17: M7 feature 7d, async iteration; end of feature 7

- Async generators are generator objects with the queue of requests of
  §27.6. `next`, `return` and `throw` make the request's promise, append
  the request and resume a suspended generator in the caller's
  interpreter loop; the frame gives the promise to the caller when it
  next suspends. A chain of 100,000 delegating generators ends with a
  `RangeError` at the frame limit, without Rust recursion.
- `yield` awaits its operand and resolves the head request; more waiting
  requests continue the frame at once. The value of a return request is
  awaited before the frame resumes, so the `yield` code is that of a
  sync generator. The end of the body completes the request and drains
  the queue.
- `for await` and `yield*` in async generators are the loops of 7a with
  awaits and an awaited close. Sync iterables go through the
  Async-from-Sync Iterator, which closes the sync iterator when a value
  rejects (the 2023 change; Chromium 148 has it, Node.js 22 does not).
  `Array.fromAsync` is a state machine of internal reactions.
- 329 Chromium 148 cases give the same output, also in stress mode. swb
  follows the specification where V8 differs: a rejected return value at
  `yield*` (V8 calls the delegate's `next`) and four error paths of
  `Array.fromAsync` (V8 closes the iterator more often, calls `return`
  in every job without end when the close fails, and ignores a `length`
  that it cannot set).
- Eight new instructions share the out-of-line arm of 7c. Unrelated
  `vm_bench` rows moved by up to 9 % in both directions (mean +0.3 %);
  `fib(27)` is 8 % slower although it runs 3 % fewer machine
  instructions. Four layouts gave the same effect (backlog, feature 13).
- Review: no blocking finding; about 230 more cases measured in
  Chromium. Fixed: a termination during a drain of the queue that a job
  started left the generator draining, so later requests stayed pending;
  now it completes the generator, as when a termination removes a frame.
- Runtime test262: 32,775 (was 27,701), the same in stress mode;
  parse-only unchanged. 4,835 async-flag tests pass (were 886).
- Target scripts: no script stops at an async generator or `for await`;
  46 of 84 compile, the other 38 stop at regular expression literals.
- Feature 7 is complete: runtime test262 went from 26,091 to 32,775
  passes.
- Next: feature 8, regular expressions.

## 2026-10-17: M7 feature 7c, `async` functions

- Async functions, arrows and methods run on the generator machinery.
  The first instruction makes the result promise and an internal
  generator object that no script sees; the body runs at once until the
  first `await`.
- `await` is `PromiseResolve(%Promise%)` and an internal reaction on the
  promise. The frame moves into the generator object, and the caller
  gets the result promise. The reaction's job resumes the frame at the
  host level, so a chain of awaiting functions costs no Rust stack. The
  job order is that of Chromium 148: one job for a native promise or a
  value, two for a thenable, three for a promise of a subclass.
  `vm/promise.rs` did not change.
- One handler over the whole code of the function rejects the promise
  on a throw, also in the parameters. A termination never becomes a
  rejection; a suspended function stays suspended and can resume in a
  later task. A function that waits for a promise that no one can
  settle is garbage.
- `return` resolves the promise while the frame is still on the stack,
  because Chromium 148 shows the function in the stack of an error that
  a `then` getter makes. Stack traces show the awaiting functions
  (`at async f`) at their `await`, as in Chromium; V8's lines for
  `Promise.all` are missing.
- 275 Chromium 148 cases give the same output, also in stress mode.
  Runtime test262: 27,701 (was 26,919), the same in stress mode;
  parse-only unchanged. 886 async-flag tests pass.
- Speed: four layouts of the new interpreter arms moved unrelated
  `vm_bench` rows by 3 to 6 % in both directions. One out-of-line arm
  for the three instructions keeps the mean change at +0.2 %.
- Target scripts: 46 of 84 compile (were 39); 37 stop at regular
  expression literals and one at an async generator.
- Next: 7d, async iteration.

## 2026-10-17: M7 feature 7b, `Promise` and the job queue

- A promise is an object kind: state, result, `[[PromiseIsHandled]]`
  and reaction pairs. The fulfill and reject reactions of one `then`
  share their capability, and a single pair needs no vector. A handler
  is empty, a function or an internal continuation, so `await` (7c)
  attaches to a promise without function objects.
- `NewPromiseCapability(%Promise%)` whose resolving functions no script
  sees (`then`, `Promise.resolve`) makes only the promise. Subclasses,
  `withResolvers` and the combinators get real resolving functions.
  Built-in functions have internal slots now; each combinator call keeps
  its shared state in one internal array.
- The job queue is a FIFO of the runtime. It is a GC root and counts as
  live heap, so a queue that grows without end stops at the heap limit.
  `run_jobs` drains it with a tick and a safepoint per job. A
  termination ends the drain as a termination and leaves the other jobs
  in the queue.
- The probe pages cannot run timers, so the runner of the probe file
  waits with a chain of `then` jobs; the same runner runs in swb-js.
  256 Chromium 148 cases (shape, messages, job order) give the same
  output, also in stress mode. Chromium 148 has the ES2025
  `Promise.try`, so two newer test262 tests fail.
- The shell runs the jobs after each script. It reports an unhandled
  rejection as `Uncaught (in promise) Error: message` with the stack and
  exits with 1. The test262 runner runs the async tests (`$DONE`).
- Runtime test262: 26,919 (was 26,277), the same in stress mode;
  `built-ins/Promise` 635 pass; parse-only unchanged.
- Speed: 100k `then` steps take 29 ms (`node --jitless` 4 ms). Most of
  the cost is the native call and the closures (feature 13). Nested
  executors run through re-entry and stop at about 400 levels (Chromium
  3,000; backlog).
- Next: 7c, `async` functions.

## 2026-10-17: M7 feature 7a, generators complete

- `yield*` compiles to a loop in the generator's code. `DelegateCall`
  calls the delegate's `next`, `throw` or `return`, by the resume mode,
  as an ordinary frame. `DelegateResult` reads `done`, and `value` only
  when done. `YieldDelegate` gives the delegate's result object to the
  caller unchanged.
- The Chromium 148 probes fixed the details: `next` always gets one
  argument, only `done` is read while the delegate runs, and the error
  messages depend on the syntactic form of the operand.
- A delegate generator resumes as a frame of the same interpreter loop,
  so a chain of 100,000 delegating generators ends at the frame limit
  with a catchable `RangeError`, as in Chromium.
- `GeneratorFunction` uses the source building and parse of the
  `Function` constructor with a `function*` prefix.
  `%GeneratorPrototype%` has `constructor` now.
- Speed: in an A/B run against the previous commit, an out-of-line
  `DelegateCall` arm made an unrelated integer loop 25 % slower, so the
  arm stays inline. The run also found an older heap accounting effect
  (slot vectors counted with their whole capacity) that makes one string
  row vary by 8 % (backlog).
- Runtime test262: 26,277 (was 26,091), the same in stress mode;
  parse-only unchanged.
- Next: 7b, `Promise` and the job queue.

## 2026-10-16: tail calls for deferred and bound calls

- The crash from the 6c review: `f = Function.prototype.call.bind(f)`
  10,000 times, then `f()`, aborted the process with a Rust stack
  overflow. Each level was a recursion `invoke` → `call_native` →
  `invoke_deferred` → `invoke` → `invoke_bound` → `invoke_deferred`,
  which pushed no frame and charged no recursion budget. Chromium 148
  returns the result for 100,000 levels of `call`, `apply`,
  `Reflect.apply` and `bind`.
- Both recursive calls are tail calls, so `invoke_tail_calls` runs them
  in a loop, in one window at the top of the value stack that each next
  call replaces: no Rust stack and no value stack in proportion to the
  chain. 200,000 levels take 0.2 s. `invoke` keeps its old shape for
  ordinary functions and natives; the loop has its own dispatch. Each
  level ticks and charges its argument copy: without that, `call` over a
  million copies of `call` was quadratic work that the time limit did
  not stop (found while testing the fix).
- Speed: a first version, in which every call went through the loop's
  dispatch, made call-heavy `vm_bench` rows 5 to 20 % slower. With the
  separate cold loop, the mean change over all rows is +0.3 %.
- The 32,768-unit limit of bound names stays: without it, 100,000 binds
  need 128 to 256 MiB, because rope rebalancing copies (backlog).
- Runtime test262 unchanged: 26,091, the same in stress mode.
- Next: feature 7 (generators, promises, `async`).

## 2026-10-16: M7 feature 6c, iterator helpers; end of feature 6

- The abstract `Iterator` constructor, `Iterator.from`,
  `Iterator.concat`, and the 11 methods of `Iterator.prototype`. Chromium
  148 does not have `zip`, `chunks`, `windows`, `join` or `includes`, so
  they are not in scope.
- A helper is a native state machine with the generator states. The
  closure variables of the specification are fields of the helper
  object; `next` and `return` run one pass of the closure. A re-entrant
  call throws "Generator is already running".
- A step over a built-in iterator or another helper takes no result
  object (6b's `builtin_step`). Each level of a helper chain charges 2
  KiB of the recursion budget, so chains deeper than about 2,000 end
  with a catchable `RangeError` (Chromium: about 5,000).
- Measured in Chromium 148: 499 cases, all equal in swb. Found on the
  way: V8 words a `return` that is not callable differently in helpers
  and in `Iterator.from` wrappers; a `take` limit above 2^53 − 1 is a
  `RangeError` in test262 but not in Chromium.
- The implementer stopped a hung probe browser with `pkill -f chrom`,
  which ended every Chrome process on the machine. CLAUDE.md now says:
  stop only the processes you started, by PID.
- Review: no blocking bug. Recorded: `return` through a helper chain
  ends at about 400 levels (`next` at about 2,000), and two closing
  details where swb follows the specification and Chromium does not.
  Found, older than 6c: about 10,000 nested bound functions overflow
  the Rust stack and abort the process (backlog, first item to fix).
- Runtime test262: 26,091 pass (was 25,672), the same in stress mode;
  `built-ins/Iterator` 415 pass, no failure. Feature 6 took test262
  from 23,222 to 26,091.
- Next: the bound-function crash, then feature 7 (generators,
  promises, `async`).

## 2026-10-16: M7 feature 6b, `Map` and `Set`

- One table type (`object/table.rs`): a vector of entries in insertion
  order, where a delete leaves an empty entry, and an open-addressing
  index of positions with the hash stored per entry. Compaction runs at
  half deleted entries. The positions of the iterators live in the table
  and compaction corrects them; `clear` uses an epoch, so it costs O(1)
  per iterator. The list of unfinished iterators is weak: the collector
  does not trace it, and the table drops the dead handles.
- Hash: the keyed hasher of `js-text` for numbers and handles, the atom
  table's SipHash for strings, so a page cannot choose collisions.
  Feature 9 can reuse `Heap::key_hash` for `WeakMap`.
- `Map`, `Set`, `Map.groupBy`, the seven ES2025 set methods,
  `getOrInsert` and `getOrInsertComputed`, with V8's messages (1,490 of
  1,499 measured cases are a test). Chromium has a limit of 2^24
  entries; swb has the same limit and message.
- `for`-`of` and the natives that consume iterables step the built-in
  Array, Map and Set iterators without a result object.
- Speed: `set` and `get` are 2 to 3 times slower than `node --jitless`,
  most of it the native call (about 75 ns); `for (const [k, v] of map)`
  5 times, because of the destructuring (feature 13).
- Review: two quadratic cases while unfinished iterators block the
  compaction. The LRU idiom (`m.delete(m.keys().next().value)` after
  each `set`) scanned the deleted entries at the start with each new
  iterator: 400,000 steps took 13.7 s. A key deleted and added again
  lengthened its probe at each step, because deleted entries stayed in
  the index: 3 million steps took more than 2 minutes. Fixed: a `head`
  count of the deleted entries at the start, where every scan begins,
  and deletion from the index with Knuth's Algorithm R (backward shift
  for linear probing). Now 0.9 s and 2.1 s. The review of that fix
  found a third case: a full iteration per step walked the deleted
  entries in the middle (400,000 steps: 14 s). Fixed: a deleted entry
  keeps, in its unused hash field, a skip to a later position, and each
  scan shortens the path (path splitting, from union-find); now 0.43 s.
  Both fixes had their own review.
- Runtime test262: 25,672 pass (was 25,091), the same in stress mode;
  the four new groups have no failures.
- Next: 6c iterator helpers.

## 2026-10-16: M7 feature 6a, `Array`

- Feature 6 is split into 6a `Array`, 6b `Map` and `Set`, 6c iterator
  helpers (roadmap). Chromium 148 has `Map.prototype.getOrInsert` and
  `Iterator.concat`, so they are in scope; it does not have
  `Iterator.zip`, `chunks`, `windows`, `join` or `includes`.
- `Array` is complete except `fromAsync` (feature 7): `from`, `of` and
  every method of `Array.prototype`, in Chromium 148's key order and
  with V8's messages (608 of 615 measured cases are a test).
- The natives have an element access layer (`object/element_access.rs`,
  `builtins/array_access.rs`): a dense array is read and changed in its
  vector where no script can see the difference; a prototype chain of
  ordinary objects lets the loops go from one stored index to the next;
  anything else takes the specification's steps at every index. A
  sparse array of length 2^32 − 1 now costs its stored elements in
  `reverse`, `shift`, `splice`, `sort` and the searches; V8 loops for
  37 s to minutes.
- New arrays from natives use a dense builder. With it and a cache of
  the last result, 100,000 splits into 8 pieces take 20 ms (were 99 ms).
- `sort` is a stable merge sort of an index permutation, written from
  Knuth and from Sedgewick and Wayne. 100,000 integers take 15 ms
  (`node --jitless` 19 ms); with a comparator 113 ms (23 ms), because
  each call from a native re-enters the interpreter (feature 13).
- Found on the way: a rope loop could end at the heap limit with little
  live data, because the rebalance cannot collect. A safepoint now also
  collects near the limit; the cause is in the backlog.
- Review: no correctness bug in about 1,200 cases of callbacks that
  change the array and of receiver states, compared with Node.js 22.
  Fixed: input in order cost more comparator calls than in V8 (now
  n − 1). Recorded: two spec-conforming differences from V8 (`concat`
  above 2^32 − 1, `sort` of a frozen one-element array); `shift` on a
  large dense array moves the whole vector, 7 times slower than
  `node --jitless` for a queue of 100,000 elements (backlog).
- Runtime test262: 25,091 pass (was 23,222), the same in stress mode;
  `built-ins/Array` 2,804 of 3,083 (was 966); parse-only unchanged.
- Next: 6b `Map` and `Set`.

## 2026-10-16: M7 feature 5d, Unicode string functions; end of feature 5

- `js-text`: `unicode/case.rs` (the Default Case Conversion with the
  full mappings and `Final_Sigma`, simple case folding), `normalize.rs`
  (UAX #15: fully expanded decomposition tables, so nothing recurses; a
  stable sort of each run of marks; one-pass composition; a quick
  check), `collate.rs` (the order of `localeCompare`). The tables come
  from UCD 17.0.0 through `swbtools js-unicode-tables`
  (`js_case_tables.py`, `js_norm_tables.py`).
- `js`: `builtins/string_unicode.rs`. The common case is short Latin-1
  text: one pass without tables, and the same string back when nothing
  changes. Long text converts in slices of 16,384 units, which check
  the length limit, reserve heap and charge time.
- `localeCompare`: the specification's requirements, and within them
  an order close to Chromium's for Latin, Greek and Cyrillic: base
  letters on the canonical decomposition by simple case folding, then
  marks, then case with lowercase first, then code points. It agrees
  with Chromium on 600 random word pairs, and on 25,184 of the 25,200
  pairs of Chromium's sorted list (the others have Ђ). ICU's collation
  is not in scope.
- Measured: all 1,114,112 code points give Chromium 148's results for
  the case functions and the four forms of `normalize` (Chromium 148
  has Unicode 17.0); `NormalizationTest.txt` passes in full; a sample
  of it is a test (Unicode License v3, text in
  `THIRD_PARTY_NOTICES.md`).
- Speed against `node --jitless`, 10-unit strings: `toLowerCase` 124 ns
  (24 ns), `normalize('NFC')` 142 ns (43 ns), `localeCompare` 159 ns
  (40 ns).
- Review: one bug. The one-pass Latin-1 path of the case functions had
  no length check and no heap reservation: `'ß'.repeat(3e8)
  .toUpperCase()` made a string longer than V8's limit, and with a
  300 MiB heap limit the process reached 719 MB. Fixed: that path is
  only for strings of at most one slice. Also fixed: the string builder
  of 5b reserved 4 bytes per unit without a cap, so a result too long
  for a string ended at the heap limit where V8 throws `RangeError`
  (now it reserves by the text's width, at most for the maximum
  length, as `JSON.stringify` does since 5c); the generator downloads
  `NormalizationTest.txt` for the full test, as the docs say; the
  license text.
- Runtime test262: 23,222 pass (was 23,081), the same in stress mode;
  parse-only unchanged. Feature 5 took test262 from 21,621 to 23,222.
- Next: feature 6 (arrays and collections).

## 2026-10-15: M7 feature 5c, `JSON`

- `js`: `builtins/json.rs` (`parse`, `rawJSON`, `isRawJSON`,
  `Symbol.toStringTag`), `json_parse.rs` (the grammar of ECMA-404 and
  V8's messages), `json_revive.rs` (`InternalizeJSONProperty` with
  `context.source`), `json_stringify.rs`; `ObjectKind::RawJson`; heap
  helpers for fresh objects (`create_fresh_data_property`,
  `push_fresh_element`, `presize`).
- No JSON walk recurses on the Rust stack: each is a loop over a vector
  of open levels. Depth limits (ADR 0026 section 15): V8's `parse` has
  none, so swb's has none (the heap and time limits end an absurd
  text); `stringify` ends at 100,000 levels, 4,000 with a gap or a
  replacer, the reviver walk at 4,000, with `RangeError: Maximum call
  stack size exceeded`, so that a `toJSON` that makes a new object at
  each level ends in an error that script can catch.
- Measured in Chromium 148: 824 cases (`tools/probes/js-json.json`),
  806 of them a test. They give the 17 families of `SyntaxError`
  messages and when each is used, the context window (a source of at
  most 20 units is shown whole, else 10 units on each side with
  `...`), the cycle message, and the quirk that a `space` between 0 and
  1 gives line breaks without indentation.
- Speed: `parse` of a 1.4 MB text 5.8 ms (`node --jitless` 3.1 ms; was
  9.4 ms before a cache of member names, size hints from the last
  container at the same depth and fresh-property stores); `stringify`
  5.0 ms (1.9 ms).
- Review: no blocking bug. Fixed: the parse records of the reviver were
  outside the heap accounting (20 million elements reached 1.8 GB RSS
  at a 1 GiB limit; now the parse checks them every 1,024 records);
  `stringify` reserved 4 bytes per output unit, so an output too long
  for a string ended the script at the heap limit where V8 throws a
  `RangeError` (now it reserves by the buffer's width, at most for the
  maximum length); an error message copied the whole source (now only
  a source of at most 20 units). A hostile test covers the records.
- Runtime test262: 23,081 pass (was 22,888), the same in stress mode;
  parse-only unchanged.

## 2026-10-15: M7 feature 5b, `String`

- `js`: `builtins/string.rs` (the statics, character access, `concat`,
  `repeat`, `padStart`, `padEnd`, `slice`, `substring`, the trims,
  `isWellFormed`, `toWellFormed`), `string_search.rs`,
  `string_replace.rs` (`replace`, `replaceAll`, `split`,
  `GetSubstitution`), `string_iter.rs`, `string_annexb.rs`, `uri.rs`.
- Decisions:
  - `replace`, `replaceAll` and `split` look up `Symbol.replace` and
    `Symbol.split` only on objects, as test262 and Chromium 148 do
    (Node.js 22 also looks them up on primitives).
  - `GetSubstitution` is complete with captures and named captures, so
    that 8b only adds the `RegExp` side.
  - `repeat`, `padStart` and `padEnd` make ropes by doubling:
    `'ab'.repeat(2 ** 28 - 12)` is a few nodes until its code units are
    read.
  - Searches: a needle of fewer than 16 units by a scan for its first
    unit, a longer one by Knuth-Morris-Pratt; `lastIndexOf` runs the
    same search on the mirrored strings. All search, copy and URI work
    is charged to the time countdown.
  - V8's messages and its quirks (the receiver message of `trimStart`
    names `trimLeft`), measured in Chromium 148: 318 cases in
    `tools/probes/js-strings.json`, which are also a test.
- The test262 runner's default time limit in stress mode is now 30
  times longer: `decodeURI/S15.1.3.1_A2.5_T1` takes 185 s with a
  collection at every safepoint.
- Review: one bug. `$<` in a replacement template scanned for `>` before
  it checked for named captures, without a time charge, so a template of
  many `$<` was quadratic (a 4-million-unit template ran 8 s past a
  0.3 s deadline). Fixed: the spec's order, and one charged scan per
  template that remembers the next `>`. Nits fixed: a time charge in
  chunks for the quote scan of the HTML methods, docs.
- Runtime test262: 22,888 pass (was 21,621), the same in stress mode;
  parse-only unchanged. The remaining failures of the new groups wait
  for `RegExp` (8b), case mapping and `normalize` (5d), `BigInt` (10)
  and `Array.prototype.lastIndexOf` (6).

## 2026-10-15: M7 feature 5a, ropes

- `js`: a string is flat text or a rope node (`string/rope.rs`: two
  children, the length, the width and the depth). Decisions (ADR 0026
  section 15):
  - A result of at most 1,024 code units is a flat copy. The limit
    comes from memory, not speed: 128 to 1,024 run at the same speed,
    but after 2 million appends a collection keeps 27.6 MB with 256
    and 7.3 MB with 1,024 (fewer string slots per append); 2,048 is
    10 % slower.
  - A short piece merges into the edge leaf of the other operand, one
    or two levels down, so that appends, prepends and both in turn fill
    leaves and do not make chains.
  - At depth 256 the concatenation first rebalances the operand in
    place into a balanced tree of its leaves. A flatten at the limit
    would be quadratic for pieces too long to merge: 100,000 appends
    of 1,000 units take 0.2 s.
  - One access API: `Heap::text` after `Runtime::flatten` (a safepoint)
    or `Runtime::flatten_no_gc` (for paths whose callers do not root
    the string). The flatten is iterative, checks the heap limit before
    it allocates, charges time while it copies and keeps the result in
    the same string. Builders and messages copy or read a rope leaf by
    leaf. Equality and order walk the leaves of both strings without a
    flatten. Keys, atoms and the strings of `String` objects are flat.
  - The collector marks the children of a rope through its work list.
- Speed: 200,000 appends of `'ab'` 0.07 s (was 1.9 s; `node --jitless`
  0.03 s), 2 million 0.57 s (was over two minutes), 1 million
  alternating appends and prepends 0.35 s (was 19.6 s). An empty loop
  over a global variable takes 0.33 s for 2 million iterations, most of
  the rest of the gap (feature 13). `vm_bench`: the append case 23.8 →
  2.1 ms; "new Error and read stack" 5 % slower from code layout.
- Tests: 17 unit tests of the rope code, `tests/strings.rs` (24 cases
  checked against Node.js 22, each also in stress mode; `length` does
  not flatten, an index read flattens once), 5 hostile tests.
- Review: no bugs; the GC safety of every flatten call checked, also
  in stress mode with a small heap limit. Two items went to the
  backlog: the comparisons of long strings charge no time, and an index
  read of a rope cannot collect before its flatten, so it ends at a
  heap limit where a collection would make room.
- Runtime test262 unchanged at 21,621 (stress mode the same),
  parse-only 41,882.

## 2026-10-15: M7 feature 4, numbers

- `js-text`: `number/` with the parsers (StringToNumber, `parseFloat`,
  `parseInt` in one pass: at most 800 kept significant digits and a
  sticky digit, Clinger's fast path, then Rust's float parser), the
  formats (`toFixed`, `toExponential`, `toPrecision` exact on
  natural numbers of 32-bit limbs, a tie picks the larger value;
  `toString(radix)` with the shortest digits that read back, the
  closest of them; the en-US `toLocaleString`), the
  exact sum of `Math.sumPrecise` (a 2,176-bit fixed-point accumulator)
  and `Math.f16round`.
- `js`: `builtins/number.rs`, `builtins/math.rs`, `parseInt` and
  `parseFloat` in `global.rs`, `vm/random.rs` (xorshift128+). The time
  charges grow with the digits made or scanned.
- Tests: a table of 4,724 format cases (`js-text/tests/data/`, with
  the scripts that write it: Node.js 22 for `toFixed`, `toExponential`
  and `toPrecision`, Chromium 148 for `toLocaleString`, an own `BigInt`
  reference for `toString(radix)`), a table of 1,209 `parseInt` cases,
  property tests of the radix digits, case tables in
  `tests/builtins_number.rs`, hostile tests of the conversions under
  the time and heap limits.
- Provenance: the review found that the first version of
  `toString(radix)` and of `parseInt` in radixes that are not powers of
  two followed the step structure of V8's code. The implementer's notes
  said it was derived from black-box runs, but its first model in double
  arithmetic already had V8's small details; most likely it came from
  the model's memory of V8. The owner decided to rewrite both from the
  literature. A new session that did not see the old code wrote them:
  `toString(radix)` after Burger and Dybvig's free-format algorithm in
  exact arithmetic (with one change: the first digit place comes from
  the value, so that the result is the closest also for subnormals),
  `parseInt` exact in all radixes. A `BigInt` reference written from the
  definition by search gives the test expectations. Chromium differs
  often, and V8's digits often do not read back as the number.
- Reviews: the first review found no bugs; its nits were a time charge
  for number scans about 4 times too low (now one step per 6 code
  units, plus a charge for the parse of up to 800 decimal digits) and
  the message of `Math.sumPrecise` for strings (until feature 5). The
  review of the rewrite checked it against an own oracle in Python
  (12,770 `toString` and 6,580 `parseInt` cases, all equal) and found
  only nits in comments.
- Learned: Chromium's `toLocaleString` rounds the shortest decimal
  digits, not the exact value (`(1.0005).toLocaleString()` is "1.001").
  `Math.hypot` with scaling by the largest value and Kahan's summation
  gives V8's results.
- Runtime test262: 21,621 pass (was 20,639), the same in stress mode;
  parse-only unchanged (41,882).

## 2026-10-15: M7 feature 3f, `new Function`, Annex B.3.2.2 and mapped `arguments`; end of feature 3

- `js-syntax`: `parse_dynamic_function` parses the source of a
  `Function` call in V8's form `(function anonymous(P\n) {\nbody\n})`
  once and checks with positions what the specification's separate
  parses check, with V8's messages; `anonymous` is not bound. A
  parameter list stops at the limit of 65,534.
- `js`: the `Function` constructor (`vm/create_function.rs`): ToString
  of the arguments, the text built under the heap check, parse and
  compile charged like eval code, the function's code kept and the
  script's dropped, the prototype from `new.target`. V8's stack frames.
- Mapped arguments (`object/arguments.rs`): a mapped element holds its
  parameter's cell; the internal methods follow §10.4.4. A first design
  with a separate boxed map cost 17 % on calls that create the object;
  this one costs nothing measurable.
- Annex B.3.2.2: global block function vars are created only if no
  lexical declaration has the name and `CanDeclareGlobalVar` allows it;
  never an error. V8 throws a `SyntaxError` there; swb follows the spec
  and test262.
- Learned: the interpreter loop is sensitive to code size; an arm with
  an inlined slow path made fib 6 % slower. Rare paths stay out of line.
- Review: no findings except nits; about 70 parameter and body pairs of
  `Function` (comment, string, template and line-terminator tricks)
  accepted or rejected as in Node, about 90 mapped-arguments cases
  equal to Node, also in GC stress mode.
- Runtime test262: 20,639 pass (was 20,296), the same in stress mode;
  parse-only unchanged (41,882). 39 of 84 target scripts compile; the
  7 that use `Function` stop at regular expressions or `async`.
- Feature 3 is complete (six sessions, 3a to 3f, 2026-10-14 and 15):
  runtime test262 went from 8,429 to 20,639 passes; the compiler
  rejects only `yield*`, `async`, `for await`, modules, `import()`,
  `import.meta`, regular expression literals and `BigInt`. Next:
  feature 4 (numbers); feature 8a (regular expressions) can run in
  parallel, and with feature 7 (`async`) it unblocks the 45 target
  scripts that do not compile.

## 2026-10-15: M7 feature 3e, `eval` and `with`

- Plan: the last session of feature 3 is split in two; `new Function`,
  Annex B.3.2.2 and mapped `arguments` move to 3f.
- `js`: the global `eval` and direct `eval` calls (`vm/eval.rs`):
  PerformEval with the early errors of the call site,
  EvalDeclarationInstantiation (conflict checks, Annex B.3.2.3 and
  B.3.4) in Rust before the eval code runs, the vars of sloppy eval
  code in the caller's variable environment, eval code as a frame of
  the interpreter loop. A function with a direct `eval` keeps a scope
  description (`compiler/eval.rs`); its eval code captures the cells of
  its caller like a closure; `this`, `new.target`, `super`,
  `super(...)`, `arguments` and private names come from the caller.
- `with` (`vm/env.rs`, `compiler/dynamic.rs`): object environment
  records with `@@unscopables`, dynamic references with the analysis's
  counts, the `with` object as `this` of calls; code outside their
  reach is unchanged.
- Exact completion values (`UpdateEmpty`); V8's stack frames of eval
  code. `js-syntax`: functions with a direct `eval` capture every name
  that eval code can reach; the well-known names live in one table per
  process (a small eval took 12 µs, now 3 µs).
- Review: no correctness bugs; about 200 probe cases equal to Node 22,
  also in GC stress mode. Fixed: eval code with many functions that
  call `eval`, inside a function with many names, cost functions x
  names in one compile without a time or memory bound (500 functions
  and 20,000 names ran 5 s past a 2 s deadline). The call now checks
  the cost before the compile and throws the `RangeError` of the
  capture limit past 2^20 captures; below it, a compile takes at most
  0.6 s. A delta review found the same growth without a nested `eval`
  (a cell that a function uses is captured by every function around
  it: 250 levels and 60,000 names took 3.5 s); the capture plan itself
  now stops at the limit. Documented: two deviations from V8 that
  follow the spec. Backlog: closures charge no time per capture.
- Runtime test262: 20,296 pass (was 18,550), the same in stress mode;
  parse-only unchanged (41,882). 39 of 84 target scripts compile.
  Next: 3f, `new Function`, Annex B.3.2.2 and mapped `arguments`.

## 2026-10-15: M7 feature 3d, private names

- `js-syntax`: a class body with private names has one implicit
  binding (`%private`) for its private environment; private names are
  indices into it and take no register (the limit of about 64,500
  private names per class from 1b is gone; 100,000 work, as in V8).
- `js`: private fields, methods, getters and setters, instance and
  static (`vm/private.rs`, `object/private.rs`): new Private Names for
  each class evaluation, private fields as shape entries under private
  symbols (not enumerated, not affected by freezing), one brand per
  instance for the private methods (as in V8), the static ones checked
  by the constructor's identity; `#x in o`, optional chains, calls
  with `this`, updates, compound and logical assignment, destructuring
  and loop targets, nested classes. V8's messages, stack frames
  (`#m`, `get #g`) and positions.
- Review: no findings; about 70 probe cases equal to Node 22, also in
  GC stress mode, among them every key enumeration path (no private
  symbol leaks), private fields on arrays, functions, bound functions,
  String and error objects, and double stamping through a constructor
  that returns another object.
- Runtime test262: 18,550 pass (was 15,881), the same in stress mode;
  parse-only unchanged (41,882). 39 of 84 target scripts compile; none
  uses private names. Next: 3e, `eval`, `with` and `new Function`.

## 2026-10-15: M7 feature 3c, classes without private names

- `js`: class declarations and expressions (`compiler/class.rs`,
  `vm/class.rs`): the heritage with V8's `TypeError`s, the inner and
  outer bindings with their dead zones, methods, accessors, static
  members, generator methods, computed keys (once, in order), public
  fields and static blocks through the parser's two initializer
  functions, default constructors made by the compiler, `super(...)`
  with the `this` binding and its checks, `super` properties in every
  assignment form and in object literal methods (home objects in class
  slots of the closure).
- Derived constructors: `this` starts uninitialized; the result check
  runs in an epilogue outside the handlers. Field initializers and
  `super(...)` are frames, so long class chains need no Rust
  recursion.
- Stack traces and `Function.prototype.toString` follow V8 for
  classes; `console.log` prints `[class A extends B]`.
- Review: no bugs in the class machinery; about 280 probe cases equal
  to Node 22, also in GC stress mode. Fixed: the heritage and computed
  keys of a class in sloppy code ran as sloppy code (§11.2.2 says all
  parts of a class are strict; `class C extends (x = Object) {}`
  created a global). The code object now lists the instruction ranges
  of class definitions in sloppy code, and the instructions whose
  result depends on strictness check them. Backlog: V8 names instances
  of classes `#<K>` in messages, swb `#<Object>`.
- Runtime test262: 15,881 pass (was 12,958), the same in stress mode;
  parse-only unchanged (41,882). 39 of 84 target scripts compile; none
  stops at a class now. Next: 3d, private names.

## 2026-10-14: M7 feature 3b, iteration and destructuring

- `js`: `for`-`of` with iterator closing (§7.4.11) on `break`,
  `continue` and `break` to outer labels, `return`, throws and the
  return of a suspended generator, through an implicit `finally` block
  of the existing completion machinery (`compiler/iterate.rs`). A
  failure of `next` does not close the iterator. Iterator records live
  in three registers; `next` of script code runs as a frame. Spread in
  array literals, calls, `new`, optional and member calls.
- Destructuring in declarations, parameters, `catch`, loop heads and
  assignments with the order of §13.15.5 (`compiler/pattern.rs`),
  defaults with NamedEvaluation, array and object rest; parameters with
  the TDZ of later parameters, rest, unmapped `arguments`, `length`
  (`compiler/params.rs`). `js-syntax`: a parameter that is a whole
  element keeps its argument's register (60,000 parameters with
  defaults work).
- Fast paths that are not observable: the built-in Array Iterator
  steps without result objects; spread and array rest copy arrays
  without holes in bulk (`[...a]` of 1M elements: 103 ms before, 3 ms;
  `node --jitless` 1.4 ms).
- Review: no bugs; about 300 probe cases equal to Node 22, also in GC
  stress mode (iterator closing on every abrupt path, patched
  iterators during a loop, generators suspended in loops and
  defaults). Fixed: docs of the argument list limit and of the
  iterator state after a failed step; a spread now releases its
  iterator after the loop. Mapped `arguments` aliasing, which the
  split had wrongly listed as done, moves to 3e; the shared scope of a
  `catch` parameter and its block goes to the backlog.
- Runtime test262: 12,958 pass (was 10,065), the same in stress mode;
  parse-only unchanged (41,882). 38 of 84 target scripts compile (were
  16). Next: 3c, classes.

## 2026-10-14: M7 feature 3a, `for`-`in`, object literals, optional chains, tagged templates

- Plan: feature 3 (language semantics) is split into five sessions
  (3a to 3e in the roadmap), ordered by the test262 tests that each
  construct blocks: before the split, the compiler rejected about
  1,920 `test/language/` tests at `class`, 1,710 at destructuring,
  560 at `for`-`of` and 375 at `for`-`in`, and 1,420 tests of the
  built-ins groups stopped at `for`-`in` (`propertyHelper.js`).
- `js`: `for`-`in` (`vm/for_in.rs`, `ForInStart`/`ForInNext`) with
  V8's choices where §14.7.5.9 leaves them: the keys of the object and
  its prototypes are collected at the start, a key that
  `[[HasProperty]]` no longer finds is skipped, added keys are not
  visited (50 mutation and order cases equal to Node 22). The key list
  is an internal heap object, reserved first and counted in the heap;
  each key and each prototype walked costs a time step.
- Getters, setters and spread in object literals, the run-time names
  of methods and accessors with computed keys (`vm/literal.rs`),
  optional chains, tagged templates (one frozen template object per
  site and realm, cached in the code object) and `new.target`.
- Review: no bugs; 1,500 probe cases equal to Node, also in GC stress
  mode; 11 hostile scripts end by the time or heap limit. Fixed: V8's
  messages for an assignment to a call in an update or a `for` head,
  the callee text of a tagged template, and the key list of a
  `for`-`in` left in a register after `break`. Backlog: reads, `in`
  and `[[Set]]` along a long prototype chain are not charged per
  prototype (a loop of failed reads on a chain of 100,000 objects ran
  7 s past a 100 ms limit).
- Runtime test262: 10,065 pass (was 8,429); 1,289 of the 1,428
  built-ins tests that stopped at `for`-`in` pass. Parse-only
  unchanged (41,882). Next: 3b, iteration and destructuring.

## 2026-10-10: M7 feature 2c, errors, `stack` and the rest of `Function`; end of feature 2

- `js`: `AggregateError`, `cause`, `Error.prototype.toString`, and
  the `stack` property in Chromium's format, measured in Node 22
  (frame lines, `new`, methods, the 10-frame default), with V8's
  `Error.captureStackTrace` and `Error.stackTraceLimit`, which two BBC
  scripts feature-test. Every error object has an own `stack`
  accessor; the frames are captured at creation (at most 200) and the
  text is built at the first read. Source positions and the names of
  anonymous functions in traces follow V8 (`compiler/positions.rs`,
  `compiler/debug_names.rs`).
- `Function.prototype.toString` for all function kinds,
  `Function.prototype.caller` and `arguments`; `Array[@@species]`,
  `Array.prototype[@@unscopables]`, ArraySpeciesCreate, `indexOf`,
  `slice`. The test262 runner accepts `nativeErrors.js`; `jsdiff`
  removes Node's own frames, so printed stacks compare.
- Cost: 1M `throw new Error` take about 440 ms (was 245 ms;
  `node --jitless` 1,254 ms); other benchmarks unchanged.
- Review: about 40 stack traces and 30 function texts equal to Node,
  also in GC stress mode; retained errors count against the heap
  limit. Fixed: `new.target` now hides the frames up to its own call,
  as in V8 (without it, every subclass of `Error` would show an extra
  `at new E` line); the internal name of a constructor in a frame;
  the message of a refused `map` result; `jsdiff` kept no user frames
  whose path contains `node:`. Backlog: the legacy `caller` of sloppy
  functions, `Error.prepareStackTrace`.
- Runtime test262: 8,429 pass (was 7,215); new groups `Error`,
  `NativeErrors`, `AggregateError`, `Function`, `Array`. Parse-only
  unchanged (41,882).
- Feature 2 is complete: runtime test262 went from 4,235 to 8,429
  passes. About 1,150 tests of the new groups wait for `for`-`in`
  (feature 3), because `propertyHelper.js` uses it. Next: M7
  feature 3, language semantics, after a pause that the owner asked
  for.

## 2026-10-10: M7 feature 2b, symbols, `Boolean` and iteration

- `js`: the 13 well-known symbols and the `Symbol.for` registry (keyed
  by atom) as roots of the runtime (`vm/symbols.rs`); `Symbol` and
  `Symbol.prototype`; `Boolean`; `@@toPrimitive` in ToPrimitive;
  `instanceof` with `@@hasInstance` (one loop, also over bound
  chains); `@@toStringTag` in `Object.prototype.toString` and on the
  built-in objects. `%IteratorPrototype%` (`@@iterator` only; the
  iterator helpers are feature 6), the array iterator,
  `Array.prototype.keys`, `values`, `entries`, `@@iterator`, and
  `@@iterator` on arguments objects. The iterator operations of §7.4
  for natives (`vm/iter.rs`), used by `Object.fromEntries` and
  `Object.groupBy`.
- GC stress mode found that a thrown value is not rooted while it
  propagates as a Rust error: a native that calls `return` on an
  iterator after an error must root the value first. The rule and its
  helper (`hold_error`) are in ADR 0026 section 15.
- Review: about 400 cases equal to Node, also in GC stress mode; no
  other site holds a thrown value across a safepoint. Fixed: messages
  quoted whole script strings (134 MB for a 128M-unit string; now cut
  as V8 does, at 100 units plus `<...>`, and other quotes at 1,024
  units, where V8 does not cut); the message of a write to a symbol
  primitive; intrinsics that are set late are `None` until set, not
  the global object; three `console.log` formats.
- Runtime test262: 7,215 pass (was 6,679); the new groups `Symbol`,
  `Boolean` and `ArrayIteratorPrototype` pass 89, and about 40
  `test/language/` groups gained passes. Parse-only unchanged.

## 2026-10-10: M7 feature 2a, `Object`, `Reflect` and `bind`

- Feature 2 is split into three sessions (roadmap): 2a `Object`,
  `Reflect` and the property paths; 2b symbols, `Boolean` and
  iteration; 2c errors and the rest of `Function`.
- `js`: `Object` (all static functions except `fromEntries` and
  `groupBy`), `Object.prototype` with the Annex B accessor methods and
  `__proto__`, all of `Reflect`, `Function.prototype.bind`, `isNaN`,
  `isFinite`, `Math.pow`. New modules `builtins/descriptor.rs`,
  `integrity.rs`, `object_proto.rs`, `reflect.rs`, `global.rs`, and
  `vm/internal.rs` (the internal methods for natives).
- Bound functions: a call walks the chain in a loop (100,000 levels
  work). Natives can end in a deferred `[[Construct]]` with
  `new.target`. The name of a bound function is limited to 32,768
  code units until strings have ropes (feature 5): each bound function
  stores a flat copy, so a loop of `f = f.bind()` stored O(n²) units
  and reached the heap limit at 20,000 levels.
- Property paths: String objects are exotic (the heap answers for
  their characters without allocation); `ArraySetLength` converts
  string and object values; the cycle check of `[[SetPrototypeOf]]`
  charges the time countdown; `%Object.prototype%` is immutable. A
  native that lists the keys of an object charges the countdown and
  checks the heap limit before it allocates the list (a String object
  lists one key per character).
- Review: about 190 cases equal to Node, also in GC stress mode, and
  no rooting error. Fixed: bound calls did not charge the copy of
  their bound arguments (142 s past a 1 s limit); key lists of large
  String objects were allocated outside the heap limit (an abort at
  2^28 characters); an own `undefined` index property was hidden by a
  String object on the prototype chain; two messages. Removed
  `Runtime::define_accessor` (backlog M6); the tests define accessors
  in script.
- test262 runner: loads `propertyHelper.js`, `compareArray.js`,
  `isConstructor.js`, `fnGlobalObject.js`, `nans.js` and
  `decimalToHexString.js` after `sta.js` and `assert.js`. Runtime
  test262: 6,679 pass (was 4,235); the new groups `Object`, `Reflect`,
  `isNaN`, `isFinite`, `global`, `ThrowTypeError` pass 1,836, the
  `test/language/` groups 608 more. 1,153 tests of the new groups wait
  for `for`-`in` in the compiler (feature 3), because
  `propertyHelper.js` uses it.

## 2026-10-10: M7 feature 1c, modules, eval and Annex B; end of feature 1

- `js-syntax`: `parse_module` (imports and exports with import
  attributes and string names, all `export default` forms,
  `import.meta`, top-level `await`, no HTML-like comments, the module
  early errors, the module records of ParseModule); `import()` with
  options, also in scripts; `parse_eval` with the early errors of the
  call site (`new.target`, `super`, `super()`, `arguments` in field
  initializers, the caller's private names). Scope analysis: `with`
  bodies and the var scopes of sloppy functions with a direct `eval`
  (also an `eval` in the parameters, §10.2.11) are dynamic
  environments; each reference in their reach records how many of them
  to check before its static binding, so the data stays linear.
  Functions with an `eval` get every implicit binding that eval code
  may use. Annex B.3.2/B.3.3 block functions get their var binding,
  decided after the parse. Module scope: top-level bindings belong to
  the module function, exported ones are cells, imports are captures.
- `js`: the compiler rejects module code, `import()` and `import.meta`,
  compiles dynamic references with their static fallback (no `eval`
  and no `with` at run time yet), and copies hoisted block functions
  into their var bindings in function code. The parse-only test262
  mode parses module tests with the Module goal.
- Parse-only test262: 41,882 pass (was 40,604), none unsupported; the
  358 failures are regular expression syntax (feature 8a). Runtime
  test262: 4,235 pass (was 4,231). All BBC and Ars scripts parse at
  49 MB/s. Their deepest member chain is 16 links, `else if` chain 5,
  nested function expressions 10: far below the compiler limits that
  the spike found (about 2,700, 1,360, 370), so no change is needed.
- Review: no wrong verdict in about 350 module probes, 140 eval
  contexts, 120 Annex B programs run in swb-js and Node, and 4,000
  prefixes. Fixed: a 1a message for `({...a = 1} = x)` that the patch
  had changed, the position of `export async`, the namespace re-export
  in the module contract. Deviations from V8 that follow the spec or
  test262: a block function named `arguments`, nested block functions
  of one name.
- Feature 1 (the full ES2025 syntax) is complete. Next: M7 feature 2,
  fundamental objects.

## 2026-10-10: M7 feature 1b, classes and async

- `js-syntax`: classes (declarations and expressions, `extends`,
  constructors, methods, accessors, generator and async methods,
  static members, fields, private names, `#x in o`, static blocks)
  and the `async` forms (functions, arrows through the `async(...)`
  cover, methods, async generators, `await`, `for await`), with their
  early errors and V8's messages and positions, measured in Node 22.
  Private names are bindings of a class body scope; the parser checks
  them when the outermost class ends. The field initializers of a
  class belong to one synthetic function for instance fields and one
  for static fields and static blocks. `super` resolves to implicit
  home object and active function bindings, so arrows capture it like
  `this`. Annex B.3.2.4 duplicates of generators and async functions
  are errors now (planned for 1c).
- `js`: the compiler rejects the new forms with "not supported yet".
- All 60 BBC and 24 Ars scripts parse (were 34 and 19), at 48 MB/s.
  Parse-only test262: 40,604 pass (was 28,203); the 358 failures are
  regular expression syntax (feature 8a), the 591 unsupported tests
  use modules (1c). Runtime test262: 4,231 pass (was 4,152).
- Review: about 1,000 targeted sources and 1,700 prefixes compared
  with Node, no verdict difference. Fixed: a class named `await` in
  an async arrow head was accepted; unresolved private names cost
  names times nesting depth (now merged smaller into larger: 0.62 s to
  0.07 s at 300 levels and 80k names); the dead `async` reference of
  an async arrow head forced a capture; five messages and positions.
- Open: `await` in field initializers follows V8, not the spec
  (test262 has no test); a class with more than about 64,500 private
  names does not fit the registers (feature 3).
- Next: M7 feature 1c, modules and the rest of the scope analysis.

## 2026-10-09: M7 feature 1a, patterns and the remaining expressions

- `js-syntax`: destructuring (declarations, parameters, `catch`, `for`
  heads, assignment through the cover grammar), spread and rest,
  default parameters, `for`-`in` and `for`-`of`, optional chaining,
  tagged templates, `new.target`, `BigInt` literals, `yield*`,
  accessors and computed keys in object literals, `with`; their early
  errors with V8's messages and positions, measured in Node 22. The
  cover conversion stays linear: the names, scopes and functions of a
  cover expression are logged per open scope, and an arrow moves only
  its own part. Functions with parameter expressions get a separate
  body scope (§10.2.11); `let` and `const` in `for`-`in`/`of` heads get
  per-iteration cells and a TDZ for the right side.
- `js`: the compiler rejects the new forms with "not supported yet"
  before it compiles anything (`compiler/support.rs`).
- test262 has a parse-only mode with its own scores: 28,203 pass, 364
  fail (358 regular expression syntax). The runtime scores rose from
  3,762 to 4,152 (negative syntax tests). 34 of 60 BBC and 19 of 24 Ars
  scripts parse; the rest stop at `class` or `async`.
- Review: no wrong verdict in about 1,300 sources and 36,000
  truncations compared with Node; all 53,655 passing parse verdicts
  were also checked in Node. Fixed: error positions (the tests now
  check them) and some messages.
- Next: M7 feature 1b, classes and `async`.

## 2026-10-09: M6 maintenance of the JavaScript crates

- Two reviewers read `js-text`, `js-regexp`, `js-syntax` and the Python
  tools, and the crate `js` with the docs; one implementer applied the
  lists. Duplicates removed: the radix parser (the exact one of the
  lexer, now in `js-text`, also serves StringToNumber, so `"0x..."`
  strings above 2^53 now convert exactly, as in V8), the two
  multiplicative hashers (one keyed hasher in `js-text`), messages
  defined in both crates, array-length checks in four places, value
  descriptions for messages and `console.log`. Dead code removed (about
  ten unused API items, an unused instruction operand, the unused
  `js-regexp` dependency of `js`). `test262.rs` and `vm/property.rs`
  are split into modules; the modules of `js` are private; comments no
  longer refer to spike sessions. `swb-js --dump-ast` prints the AST.
- Behaviour: the tests, the test262 totals (3,762 pass) and the
  benchmark rows are unchanged within noise. No browser crate depends
  on the JavaScript crates, so `just snapshot` was not needed.
- M7 feature 1 is split into three sessions (roadmap). The Ars
  Technica scripts (24, external and inline) are in `out/ars-js/` and
  the BBC scripts in `out/bbc-js/`, git-ignored.
- Next: M7 feature 1a, patterns and the remaining expressions.

## 2026-10-09: M6 spike session 6, tools; end of the spike

- `swb-js`: a shell with `print`, `console.log`, `$262`, the limits,
  the GC stress mode and exit codes per result. `just test262` runs the
  subset of test262 (98 groups of `test/language/`, 10,183 tests) in
  0.3 s: 3,762 pass, 1,028 fail, 3,476 use syntax outside the subset;
  no panics, the same counts in stress mode. A scores file per group
  may only go up. `just jsdiff` compares output with Node.js 22;
  `just jsbench` measures against `node --jitless`
  (docs/performance.md).
- Review: no wrong verdict of the runner (each pass was checked in
  Node). Fixed: an unreadable test file counted as a pass, runs on part
  of a group lowered or broke the scores, a recursive
  `$262.evalScript` overflowed the Rust stack (re-entrant `eval` now
  charges the recursion budget), the shell did not stop on a closed
  output pipe, `jsdiff` stopped at a timeout.
- Spike report: docs/js-spike-report.md. The architecture of ADR 0026
  holds; the spike's choices are in section 14. The costs for M7: calls
  5x, property access 5x, object literals 11x slower than V8's
  interpreter (global cache, inline caches, literal shapes in feature
  13); string appends 83x (ropes in feature 5).
- `js-spike` merges into `main`. Next: M7 feature 1 (full syntax).
  M6 step 4 (the on/off setting) comes with the first engine
  integration.

## 2026-10-09: M6 spike session 5, exceptions, limits and built-ins (`js-spike`)

- `js`: a handler table per code object; a throw searches it from the
  top frame down and restores the value stack, handle scopes and no-GC
  regions at the handler. `finally` keeps the pending completion in two
  registers; each block exists once. Generators resume with a mode
  (`next`, `throw`, `return`). Termination (time limit, heap limit,
  host request) skips every handler; the runtime stays usable after it.
  A time countdown with a deadline and a termination handle for other
  threads. The first built-ins in `src/builtins/`: `console.log` in
  Node's format, the error constructors, `Object.keys`, `Array` with
  `push`, `join`, `forEach` and `map`, `Function.prototype.call` and
  `apply` as deferred calls, `String()` and `Number()`. `js-syntax`:
  `switch`.
- Measurements against `node --jitless`: a loop inside a `try` 1.15x,
  `forEach` over 1M elements 9.7x. The countdown costs about 5 % on an
  integer loop; handle scopes per element in native loops cost nothing
  measurable.
- Review: 6,000 random programs with nested `try`, loops, `switch` and
  generators gave the same output as Node, also in GC stress mode. The
  time limit had a gap: built-ins and string concatenation counted as
  one step whatever their size, so a loop of `Object.keys` calls on a
  large object ran 101 s past a 100 ms deadline. Now they charge steps
  in proportion to their work, and the hostile cases end within 3 ms
  of the deadline. Also fixed: three messages, a cap on key length and
  total output in `console.log`.
- test262 (commit 2e0a567, 2026-10-08) is downloaded for session 6.
- Next: spike session 6, the `swb-js` shell, `just test262`,
  `just jsdiff` and the benchmarks.

## 2026-10-09: M6 spike session 4, compiler and interpreter (`js-spike`)

- `js`: a compiler from the AST and the scope analysis to a register
  machine (12-byte instructions, constant pool, property sites, line
  table, verifier; each function separately, code objects created in
  one no-GC region); an interpreter with one value stack and one frame
  vector, without Rust recursion for script calls, accessor calls and
  generator resumptions; native functions with a handle scope and
  deferred calls; closures with cells; generators that move their frame
  into the generator object; the realm with the global object and the
  global lexical record; conversions and operators; the embedding API
  (`Runtime`, `eval`, native functions, errors). Error messages follow
  V8, measured in Node 22.
- Measurements (release) against `node --jitless`: `fib(27)` 4.7x, a
  10M-step integer loop 1.0x, `o.x = o.x + o.y` 4.7x, a closure counter
  3.1x, 1M object literals 10.6x slower. The time goes to global lookup
  by name and the generic `[[Get]]`, `[[Set]]` and define paths; inline
  caches and literal shapes come in M7 feature 13. Code: 5.5 bytes per
  source byte; parse and compile at 29 MB/s.
- Review: no rooting error and no panic in about 500 scripts in normal
  and GC stress mode. Fixed: Number-to-string broke exact ties upward
  (810 of 5,076 test doubles differed from Node), a string or object
  written to an array's `length` ended the script with an internal
  error, a wrapper for `this` stayed in the native's handle scope, the
  global declaration checks ran after earlier bindings were created,
  host calls could pass internal values to script, and five messages.
- Next: spike session 5, exceptions, limits and the first built-ins.

## 2026-10-09: M6 spike session 3, heap, objects and GC (`js-spike`)

- `js`: an arena per kind with (index, generation) handles; a
  stop-the-world mark-and-sweep collector with mark bits beside the
  arenas, a work list and large vectors marked in parts; weak tables
  for atoms, shape transitions and root shapes; roots from the VM,
  persistent roots, handle scopes; safepoints, reservations, the heap
  limit and a stress mode. A 16-byte `Value` with the three equality
  algorithms; flat strings of both widths and atoms; shapes with root
  shapes per prototype, shared key lists and dictionary mode; dense and
  sparse elements; the ordinary internal methods and the array exotic
  object, with accessors returned to the caller as call requests.
- Measurements (release): GC pause 16 ms for 1 million objects with 3
  properties each; 202 ns to allocate such an object; 3.1 ns per handle
  access; 169 bytes per object.
- Review: no path freed a live object, but the limit did not bound
  memory (539 MB resident at a 128 MB limit, because arenas kept their
  peak size), a heap near the limit collected at every safepoint, and a
  no-GC region could switch the limit off. Fixed: the limit counts the
  slots of every arena, a sweep gives back the free tail with a
  generation floor for reused indices, a live size above 90 % of the
  limit after a collection ends the script, safepoints in no-GC regions
  check the limit. A second review of the fixes found that counting the
  doubled vector capacity could end a script at 75 % live data; the
  limit now counts slots up to the length (the resident pages), and
  handle-scope tokens carry an id. Resident growth in the three-phase
  test: 48 MiB at a 128 MiB limit.
- ADR 0026 got section 14 (changes from the spike).
- Next: spike session 4, the compiler and the interpreter core.

## 2026-10-09: M6 spike session 2, parser and scope analysis (`js-spike`)

- `js-syntax`: an AST arena (`Expr` at most 24 bytes, `Stmt` 32; node
  types for the whole grammar), a recursive-descent parser for the
  subset of roadmap step 3.2 with re-scanning for `/` and template `}`,
  arrow parameters converted from the parenthesized expression, early
  errors with V8's messages (about 400 cases measured in Node), and
  "not supported yet" outside the subset. The scope analysis runs over
  tables after the parse, without recursion: registers, cells, globals
  by name, captures through each function in between (flat closures),
  temporal-dead-zone flags, per-iteration `let` cells, `arguments` and
  arrow `this`. `js-text` got the shared recursion budget (approximate
  stack bytes, 4 MiB by default, weights measured in a debug build).
- 72 MB/s for parse and analysis of a 5 MB generated program, about 31
  bytes per token. 3 of 60 BBC scripts are inside the subset; the rest
  stop at `for`-`in`, destructuring, spread and other M7 constructs.
- Review: mapped `arguments` did not make parameters cells; `var`
  declarations were quadratic in the block depth (14 s for a 206 KB
  hostile script); flat-closure captures could amplify memory (523 MB
  from 510 KB); resolution was O(depth) per reference. Fixed: cells for
  mapped parameters, constant-time `var` checks, limits on captures, a
  resolution cache, an explicit `needs_tdz` flag, a register reserve for
  temporaries, three messages. Annex B.3.3 (functions declared in blocks
  in sloppy mode) is missing; it is in M7 feature 1. Right-nested `?:`
  chains stop at about 1,360 terms (backlog in the roadmap).
- Next: spike session 3, the heap, values, strings, objects and GC.

## 2026-10-09: M6 spike session 1, crates and lexer (branch `js-spike`)

- The four crates of ADR 0026. `js-text`: code units of both widths
  (`Str16`, `String16`), code points, UTF-8 conversion, and the
  identifier and white-space classes from UCD 17.0 (new `swbtools
  js-unicode-tables`; the UCD helpers moved to `tools/swbtools/ucd.py`,
  the line-break tables regenerate unchanged). `js-regexp`: a flag check
  only. `js-syntax`: the complete lexer of clause 12 and Annex B.1, on
  demand by goal symbol, with checkpoints for re-scanning and a name
  interner. `js`: a skeleton.
- The BBC scripts (3.5 MB) lex in about 28 ms (125 MB/s); interning is a
  quarter of that.
- Review: no lexer bug. Fixed: one strict-mode flag became an enum of
  the four legacy forms (Chromium has a message for each); the name
  cache used the Fx hash constant, now Fibonacci hashing (Knuth, in
  credits); `-->` at the input start no longer depends on the goal
  symbol; the regular expression error type is non-exhaustive.
- Next: spike session 2, the subset parser and the scope analysis.

## 2026-10-09: M6 step 2, architecture of the language core

- ADR 0026, written from the memos, the specifications and the
  literature only. Main choices and why:
  - Four crates (`js-text`, `js-regexp`, `js-syntax`, `js`), so that
    each can be tested alone and the regular expression engine can grow
    in parallel; `js-text` holds the code units and Unicode data that
    the other three share.
  - An AST first: cover grammars by conversion (linear), scope analysis
    over the whole function nest, captured variables in cells created
    at the declaration, so that generator frames can move.
  - A register machine with Rust enum instructions, a verifier pass and
    a handler table; `finally` with a stored completion.
  - Arenas with (index, generation) handles, stop-the-world
    mark-and-sweep, handle scopes for native code, collections only at
    safepoints, a stress mode.
  - Two string widths with ropes; shapes with transitions and the
    prototype in the shape; dense and sparse elements; no inline caches
    at first.
  - Numbers: Rust's shortest formatting and correctly rounded parsing,
    plus an own exact routine for radixes and fixed formats (no `ryu`).
  - One shared budget for all Rust recursion.
- A `reviewer` session checked ADR 0026, the roadmap and the step 1
  docs (the step 1 commit had no reviewer session; ground rule 15). It
  found an instruction-size conflict with cache operands (now property
  sites), an undefined order of collection and heap-limit checks (now
  reservations as safepoints), gaps (root shapes per prototype, the
  state after termination, a shared text crate, the stack size, extra
  arguments) and dependency errors in the M7 order (iterators before
  the language semantics, `BigInt` before typed arrays, weak
  collections with ephemerons, the regular expression feature split so
  that only its crate part runs in parallel). All fixed. No section
  number was wrong.
- Roadmap: the spike (M6 step 3) is six sessions on the branch
  `js-spike`; M7 is the ordered feature plan of the language core (14
  features).
- Next: spike session 1 (crates, `js-text`, the lexer).

## 2026-10-09: M6 step 1, JavaScript study memos

- Five `analyst` sessions (in parallel, read-only) wrote the study memos
  in `docs/js-study/` from QuickJS, QuickJS-ng, MicroQuickJS (MIT,
  license checked), MuJS (now on Codeberg) and Duktape with its design
  notes, plus the literature. Each memo is about 7,000 words. The
  orchestrator reviewed each against the rules of ADR 0025 (no code,
  pseudocode, source identifiers, opcode or field lists); all passed
  without changes except a status line. The uncertain ECMA-262 section
  numbers were checked against the draft.
- Main findings for the design: none of the five engines has inline
  caches, lazy parsing or a linear-time regular expression fallback;
  only the QuickJS family has generators, `async`, `BigInt`, weak
  references, modules and spec-exact `RepeatMatcher`. MicroQuickJS is
  closest to an index-handle heap (moving collector, registered roots,
  offsets instead of pointers). Script-to-script calls without host
  recursion: Duktape and MicroQuickJS. The BBC scripts: 3.5 MB, about
  1.1 million tokens, 16,000 functions, nesting at most 36 brackets and
  10 functions deep; several files are one line of up to 485 KB.
- Memo 1 cites ES2025 section numbers, memos 2 to 5 the current draft;
  clause 27 differs by three (README notes this).
- Next: step 2, the architecture ADR of the language core.

## 2026-10-09: JavaScript decisions

- Discussed with the owner how to add JavaScript without a third-party
  engine. Decisions are in the ground rules and ADR 0025: the
  specification, test262, literature and black-box tests are the main
  sources; `analyst` sessions read a few small engines with permissive
  licenses (QuickJS, MuJS, Duktape) and answer written questions in
  memos without code; all other sessions read only the memos. Our own
  parser. Edge libraries (number formatting, time zones, ICU4X) are
  allowed. Node.js and test262 are test tools only. An on/off setting,
  off by default. All scripts run, whatever their origin. The user-agent
  string stays.
- A rule by origin would block the sites' own code: BBC loads 55 of its
  60 scripts from `static.files.bbci.co.uk`, Ars Technica from
  `cdn.arstechnica.net`. A later ad and tracker blocker would use URL
  lists.
- ADR 0021 now also names Ladybird and the browsers' JavaScript engines.
  New agent type `analyst`; the other agent types do not read JavaScript
  engine source.
- Next: M6 (roadmap): the study memos, then the architecture ADR, then a
  spike on a branch.

## 2026-10-09: M5 maintenance

- Two read-only reviewers (layout, paint and text; the other crates,
  tools and docs) listed duplication, names and outdated docs; one
  implementer applied the selected items. `just snapshot` is
  byte-identical before and after (346 files); the compare scores do
  not change.
- Code: `grid::layout` merged with `layout_grid`, duplicate arms in the
  button layout merged, `build_flex_items` renamed `build_item_boxes`
  (it builds grid items too), `MAX_CACHED_MASK_BYTES` renamed
  `MAX_CACHED_MASK_PIXELS`, one `is_rendered` test for focus. Engine test
  helpers (`TestSite`, `open_url`, `center`, `click`, `key`) moved to
  `tests/common`. Tools: small duplicates in `pixels.py`, a dead field,
  constants for literals.
- Docs: ADRs 0006, 0013, 0017 and 0019 match M5; docs/performance.md has
  a new baseline for all five fixtures (BBC 25 ms, Ars 40 ms, Wikipedia
  37 ms).
- Skipped, with reasons in the roadmap backlog: the two sources for
  hidden details content (style and attribute), the doubled marker style
  of a `details` with `display: list-item`.
- M5 is complete. The owner accepted target 5.

## 2026-10-09: final pass on BBC (M5 item 4)

- The text pixel differences on all pages were glyph edge coverage, not
  positions: swb and Chromium use the same advances and quarter-pixel
  offsets, but tiny-skia resolves edges to 1/4 px. Glyph masks that the
  cache keeps are now filled at 4 times the size and averaged down (ADR
  0006 update). Review: large glyphs paid 16 times the cost on every
  use; they are now filled directly, with the hostile case
  `huge-glyphs`.
- Single images are scaled into their pixel-snapped rectangle (the 1–2 px
  lines at the edges of BBC images).
- Engine tests for the BBC links and `:hover` rules; no bug found.
- Scores: BBC pixels 0.9987 (full page 0.9987), Hacker News 0.9988,
  senko.net 0.9989. 148 snapshot images change.
- Target 5 is done. Next: the M5 maintenance.

## 2026-10-09: grid buttons, `scrollbar-width` (M5 item 3)

- `<button>` with `display: grid` or `inline-grid` lays out its content
  as a grid. Measured: Chromium centers only flow content of a button
  vertically; flex and grid content is laid out as in a plain container
  (stretch, `align-content`). swb centered flex content before; fixed.
- `scrollbar-width: auto | thin | none`; `none` hides the GUI scroll
  indicator (the BBC carousels). The viewport follows the root element.
- BBC geometry 0.9660 → 1.0000. Other fixtures unchanged.
- Review: no bugs. A release-only stack overflow in the `deep_nesting`
  test exists on main too (backlog).
- Next: M5 item 4, the final pass.

## 2026-10-09: `details` and `summary` (M5 item 2)

- The content of a closed `details` is laid out in a `::details-content`
  box with size, layout and paint containment, as Chromium's
  `content-visibility: hidden`. After layout the boxes move to
  `BoxFragment::hidden`: the box dump reports them, paint, hit testing,
  selection, Tab and scrolling do not see them.
- A click, Enter or Space on the first `summary` toggles `open` (not a
  click on a link or field in it). A `details` without `summary` gets the
  default summary ("Details").
- Measured: inside disclosure markers are 1.0592 em wide and keep the
  line height.
- Review: no blocking bugs; closing a `details` now also removes a
  selection in its content. The other findings went to the backlog.
- BBC geometry 0.8786 → 0.9660, no missing boxes; the open menu matches
  Chromium. Other fixtures unchanged.
- Next: M5 item 3, grid buttons and `scrollbar-width`.

## 2026-10-09: M5 plan, full-page and state comparison (M5 item 1)

- The owner accepted target 4 (Ars Technica).
- M5 plan (BBC): after M4 the page has geometry 0.8786 and pixels 0.9985.
  What remains: the content of the closed `details` menu (229 missing
  boxes) and its toggle, `display: grid` on `<button>` in the off-screen
  drawer, `scrollbar-width`, and text pixels. `quotes`, complex `:not()`
  and `::-webkit-scrollbar` from the old list are not needed.
- `compare --full-page` compares full-page screenshots (Chromium live,
  swb `--full-page`) with a score over the union of both areas and a
  list of differing regions; `compare --click SELECTOR` compares the
  state after clicks in both browsers (Playwright; swb's automation
  API). The full-page arrays are uint8 and processed in bands of 512
  rows (review: the first version used about 700 MB for BBC).
- Playwright's full-page capture does not change Chromium's layout.
- Full page: BBC 0.9981, Ars 0.9966. No layout difference and no missing
  paint feature below the first viewport. The differences are sub-pixel
  glyph positions inside words (item 4) and 1–2 px lines at the edges
  of scaled images.
- Next: M5 item 2, `details` and `summary`.

## 2026-10-09: M4 maintenance

- Two read-only reviewers (paint and layout; the other crates, tools and
  docs) listed duplication, dead code, names and outdated docs; one
  implementer applied the selected items. `just snapshot` is
  byte-identical before and after (338 files).
- Paint: the path cost model moved from `svg/` to `paint/src/path_cost/`
  (it serves inline SVG and SVG images). One helper makes the dash
  outline for both. Layer pushes take named structs (`NewLayer`,
  `LayerKind`, `SvgClipLayer`). The inline SVG raster tests moved out
  of `raster.rs` (4,257 → 3,662 lines). Module docs list path items,
  SVG clip groups, rounded clips and image reduction.
- Layout: Bezier evaluation is in `layout/src/bezier.rs` (six copies;
  the derivative-root solvers stay apart, because their thresholds
  differ and the cost numbers would change). One circle constant
  (`KAPPA`), one SVG length table, one warn-once counter for the SVG
  limits.
- Other crates: the engine's copy of `element_ids` and a second opacity
  parser are gone; constants that two crates share say so.
- Tools: `reference` and `layout-refs` wait for web fonts like `probe`;
  `substitute` uses the shared MIME parser.
- Docs: README status, roadmap (M4 done), testing.md (`substitute`, font
  waits), ADR 0021 (`wuff` is a library, not an engine port),
  architecture.md (`CountBudget`, module list).
- Backlog: the `font-variation-settings` table is not cleared per
  document; typed web font sources; the performance baseline.

## 2026-10-08: M4 item 9, final pass on Ars Technica

- Four causes behind the last 4,964 differing pixels:
  - The underline bars of the view selector were 14 px too far right.
    The box tree put an absolutely positioned child that follows text
    into the anonymous flex item around that text, so its static
    position came after the icon and the gap. In a flex or grid
    container the child now ends the anonymous item and is a child of
    the container, as in Chromium (layout test `abspos-flex-text`).
  - The round thumbnails use `overflow: hidden` with
    `border-radius: 50%`. Overflow clips now follow the rounded padding
    edge (radii reduced by the border widths), also for deferred
    positioned descendants.
  - The thumbnail grid looked coarser in swb because bilinear sampling
    of a 5× reduction aliases. Raster images drawn at less than half
    their size are now averaged 2×2 until the scale is at least 0.5,
    then sampled bilinearly.
  - Chromium paints inline SVG from the pixel-snapped origin of its
    content box; swb now does too (scale-aware). The logo differs in
    51 px instead of 820.
- The `:hover` rule forms of the page's Tailwind sheet work without a
  change (new engine test). The probe tool cannot hover, so this is not
  compared with Chromium.
- Review fixes (ADR 0024): the averaging first ran at every raster;
  200 large images drawn at 50×50 cost 4.1 s per frame. The reduced
  levels are now kept with the decoded image (at most its own size)
  and made lazily, and one frame averages at most 64 Mpx; 2,000 small
  draws of a 4000×3000 PNG take 0.10 s. A rounded clip is a layer
  group; when the layer budget ran out, 150 nested rounded boxes drew
  nothing. They now fall back to a rectangle clip. Four hostile-page
  cases.
- Ars geometry 0.9963 → 1.0000, pixels 0.9952 → 0.9992. Target 4 is
  done. BBC images below the first viewport are smoother; its scores do
  not change.
- Backlog: the word "LIST" is 0.36 px narrower than in Chromium, which
  shifts the next buttons by a fraction of a pixel.

## 2026-10-08: M4 item 8, raster cost of fills with thin separate spans

- The review of item 7 found that an anti-aliased fill of separate thin
  shapes cost far more than the model charged: a comb of 2,000–4,000
  teeth 0.25 px wide on 1,200 × 780 px took 5–6 s per frame and was
  drawn; the area under a noisy chart of 40,000 points took 1.6 s.
- Measured (tiny-skia public API, 183 new timings): coverage steps in
  quarter pixels in x and y. The extra time comes from spans that lie
  inside one pixel and cover part of it; a span that crosses a pixel
  boundary ends a group. A row costs the inside spans of each group
  times the pixels they touch (2–5.3 ns per pair, at most about 7.7 ms
  per row at 1,200 px). The time grows linearly with the teeth, not with
  their square. Fills without anti-aliasing and strokes are cheap.
- New `paint/src/svg/spans.rs` finds the spans at sample rows (crossings
  in x order, winding under the fill rule), with margins for rounding
  and slope, and charges 6 ns per pair. An O(n) bound from the sweep
  runs first; the count runs only when the bound is large and the count
  fits into the rest of the budget, and its own work is charged. Inline
  fills, clip coverage and SVG images use it (images: counted at 17
  scales; the size search now goes down from the requested size).
- The combs, stacked combs and the area are rejected in 0.05 s. The
  paths of item 7 still draw. Over all timings the real time is at most
  0.88 of the charged time. Five hostile-page cases. Snapshots unchanged.
- Review fixes. Dashes are separate shapes: a dashed stroke wider than
  a pixel is now charged from its outline (tiny-skia's public
  `Path::dash` and `Path::stroke`), counted like a fill. A line 780 px
  wide with 0.2 px dashes took 6 s; it and 15 such lines are rejected in
  0.05–0.15 s, inline and in SVG images. Strokes without dashes need no
  span charge (measured: 800 parallel lines with 0.15 px gaps take
  45 ms). Counting the spans of SVG images has a budget per document
  (2 billion units, 0.6 s): 30 images of a 250,000-segment walk decode
  in 2.2 s instead of 7.8 s. Four more hostile-page cases.
- The item 7 bench read `/proc/thread-self/schedstat`, which on this
  kernel advances in 4 ms steps; the new bench uses
  `CLOCK_THREAD_CPUTIME_ID`.
- Backlog: some fast fills are charged as slow because the model cannot
  see rounding and sub-pixel position (diagonal hatching of 1,000 thin
  filled lines: 106 ms real, rejected; the area of 20,000 points: 0.6 s
  real, rejected). Slanted edges whose spans change between sample rows
  are estimated from the samples, not bounded. Decoding SVG images of
  very many segments costs about 40 ms per 250,000 segments outside the
  counting budget, with no limit per document.

## 2026-10-08: M4 item 7, raster cost of dense paths

- The review of item 6 found pages that take minutes to draw: the path
  cost estimate charged a segment about 9 ns, but one path of 40,000
  curves across 100 rows takes 2.6 s in tiny-skia (the old estimate:
  1 ms).
- Measured (tiny-skia 0.12.0, release build, CPU time of the thread,
  315 timings; tables in ADR 0023 part 3): the fill time fits the edge
  rows (rows that each edge crosses) and the pairs of edges that cross.
  Pairs that overlap in y but do not cross cost almost nothing. Two
  edges can cross only if their bounding boxes overlap, so the model
  counts those pairs.
- New `paint/src/svg/edges.rs` counts rows and box pairs in O(n log n)
  (Fenwick trees over x). A stroke of at most 1 device pixel is a
  hairline: its time follows its length (18–60 ns per pixel), with no
  pair term. Inline fills and strokes, clip coverage and the SVG image
  estimate (ADR 0011; an exact term at the rendered size) use it. Over
  the measurements the real time is at most 1.02 of the charged time,
  except for the known gap below. Hit tests have a work budget (about
  40 ms). `SvgPath::fill_bounds` is cached: each clip reference walked
  all segments.
- A first version counted all pairs that overlap in y. Its review found
  that it rejected realistic paths: noisy line charts of 10,000–40,000
  points, a histogram of 5,000 bars, a random walk of 60,000 segments,
  SVG images of 100 paths of 400 curves. These now draw in 0.06–0.38 s
  (images at 1,000 px at a lower resolution).
- The repro pages take 0.1–0.6 s (before: 22 s to more than 120 s).
  Five hostile-page cases. The target pages do not change; their paths
  use at most 0.1 % of the budget.
- Open: an anti-aliased fill whose edges do not merge into long spans
  is charged 5 to 15 times too little. A comb of 2,000–4,000 separate
  teeth takes 5–6 s per frame and is drawn; the area under a noisy
  chart takes 3.3 s. This is item 8, next. The review of the fix also
  found that one edge far outside the window merged the x columns of
  the pair count, so a chart with a long baseline was rejected; the x
  columns are now cut to the window.
- Backlog: the weights follow the slowest case of each kind, so some
  paths that draw in time are rejected.
- ADR 0021 now also covers ports of the engines it names, such as
  tiny-skia. An agent read tiny-skia's source for the first version of
  this model; no code was copied, and the owner kept the model.

## 2026-10-08: M4 feature 6, inline SVG part 2

- `clip-path: url(#id)` on `g`, `a`, `use` and shapes, as a property or
  a presentation attribute. `clipPath` supports `clipPathUnits`, its own
  `transform`, `clip-rule`, shapes and `use` children, and `clip-path`
  on the clip path (intersection); cycles are cut. As in Chromium, the
  id is looked up in the whole document, and an invalid reference leaves
  the element unclipped. A clip that is one axis-aligned rectangle
  becomes a clip rectangle; it is dropped when it covers the content box
  (all Ars icons). Other clips are a layer multiplied by the coverage of
  the clip shapes (new display item `PushSvgClip`).
- `use` draws a copy of the referenced element whose styles inherit from
  the `use`. The style crate computes these instance trees
  (`style/src/cascade/use_instances.rs`): at most 20,000 elements and
  16 levels per document, no copy for a cycle.
- Box dump: `g`, `a`, `use` and the shapes get their fill bounding boxes
  in page coordinates, as Chromium reports them; `defs`, `clipPath`,
  `title` and elements with `display: none` get none. New layout test
  `tests/layout/inline-svg-boxes.html`.
- Hit testing: a shape is hit by its geometry (`pointer-events`). An
  SVG `a` with `href` is a link with the pointer cursor.
- `shape-rendering: crispEdges` and `optimizeSpeed` draw without
  anti-aliasing (BBC icons).
- Six hostile-page cases (`use` expansion and cycles, clip chains,
  layers and shapes).
- Scores: Ars geometry 0.7978 → 0.9963, no missing boxes; BBC 0.8053 →
  0.8786; pixels unchanged. The 229 boxes still missing on BBC are the
  content of closed `details` (M5).
- Review fixes: the `use` expansion walked `display: none` subtrees
  without counting them (10.4 s → 0.15 s on a test page); clip shapes
  with their own `clip-path` ran full-layer passes that the work budget
  did not count (19.9 s → 0.58 s). A `use` inside a `clipPath` now
  keeps the `clip-path` of its target, and the clipped-out part of a
  shape no longer takes clicks. Two new hostile-page cases.

## 2026-10-08: M4 feature 5, inline SVG part 1

- swb draws inline `<svg>` with its own code (ADR 0023). resvg stays
  for SVG images only. The outer `<svg>` is a replaced box. Its
  `width`/`height` attributes follow Chromium 148 (measured): they act
  as presentation attributes and as the natural size, an invalid value
  is `100%`, and the ratio of the attributes wins over the `viewBox`.
  SVG descendants get no CSS boxes and no text runs.
- Style: the fill and stroke properties, presentation attributes as
  author-level hints with specificity 0 (the `transform` attribute
  through `svgtypes`), two SVG 2 user-agent rules.
- Layout (`layout/src/svg/`): `g`, `a`, `path`, `rect`, `circle`,
  `ellipse`, `line`, `polyline` and `polygon`; path data with swb's own
  arc conversion (SVG 2 implementation notes); `viewBox` and
  `preserveAspectRatio`. Paint: new `FillPath` and `StrokePath` items,
  rasterized with tiny-skia; group opacity as layers; a clip at the
  content box.
- Limits: 50,000 shapes and 1,000,000 path segments per document,
  groups 64 deep, dash arrays of 256 entries, strokes with more than
  100,000 dashes drawn solid, a work budget for paths per strip. Seven
  hostile-page cases.
- The probe tool compares `ink` with swb (`--with-swb`).
  `tools/probes/inline-svg.json` has 16 cases; all sizes match.
- Scores: Ars geometry 0.7740 → 0.7978, pixels 0.9898 → 0.9952; BBC
  geometry 0.7775 → 0.8053, pixels 0.9942 → 0.9985. The count of
  missing boxes went up (Ars 217, BBC 202): Chromium reports boxes for
  `g` and the shapes, which part 2 adds.
- Review fixes: each `<g>` with `opacity` below 1 took a layer, and
  20,000 such groups took 80 s. Now at most 256 layers per document;
  past the limit the group opacity multiplies the alpha of each child
  paint, and the same page takes 1.9 s. A group with one shape and one
  paint needs no layer. Path fills cost about 2.6 ns per pixel, opaque or not,
  so the work budget now counts every pixel as blended. `<style>` in
  SVG is now a document style sheet; as in Chromium, a `type` other
  than empty or `text/css` turns off a `<style>` element, also in HTML.

## 2026-10-08: M4 feature 4, web fonts part 2

- New properties `font-variation-settings` and `font-feature-settings`
  (`style/src/font_settings.rs`), and the same two descriptors in
  `@font-face`. As in Chromium, the computed value is sorted by tag and
  keeps the last value of a repeated tag, and the `font` shorthand
  resets both. At most 64 tags per declaration.
- Axis values come in this order: font matching, the `@font-face`
  descriptor, the property. This also applies to variable system fonts.
  Synthetic bold ignores the property (measured in Chromium 148).
- The metric descriptors `size-adjust`, `ascent-override`,
  `descent-override` and `line-gap-override` apply in the text crate
  (shaping, metrics, glyph masks), so layout code did not change. The
  overrides are ratios of the size after `size-adjust` (measured).
- `src: local()` matches the full name or the PostScript name of a
  system font, ignoring ASCII case and spaces, and loads without a
  request. `local("Arial")` does not match in Chromium either, so the
  fallback faces of both target pages never load.
- Ars geometry 0.6908 → 0.7740, pixels 0.9544 → 0.9898: the headings
  now use `wght` 660. BBC is unchanged. New: probe file
  `tools/probes/web-fonts-2.json` (16 cases), a variable test font made
  for swb (`crates/text/tests/webfonts/swb-variable.ttf`, CC0, with its
  build script), five hostile-page cases.
- Review fix: each distinct `font-feature-settings` list needs its own
  shape plan, and the plan lookup is linear, so 60,000 distinct lists
  took 11.1 s. A font instance now keeps at most 32 plans (0.81 s).
- Tools: `just review-tree reset` failed when the review worktree held
  a patch that touched files changed by the new commit; it now checks
  out with `--force`.
- Backlog: in Chromium, `letter-spacing` other than 0 turns off optional
  ligatures; `ex` uses 0.5em in swb; automatic `opsz`; named instances
  of variable fonts in `local()`.

## 2026-10-08: M4 feature 3, web fonts part 1

- `@font-face` works (ADR 0022). Style parses the descriptors
  (`style/src/font_face.rs`) and keeps the rules with their `@media` and
  `@supports` conditions. The text crate owns the face set
  (`text/src/web.rs`): web families shadow system families of the same
  name, CSS Fonts 4 §5 matching selects a composite font, and its faces
  are checked per character, the last rule first.
- A face loads only when text needs it, and for a line box only the face
  that covers U+0020 (measured in Chromium). The engine loads the faces
  after layout (each URL once, `src` entries in order) and lays out again
  when a font arrives. Font loads count as pending requests, so headless
  rendering waits for them. `local()` fails until part 2. All
  `font-display` values act as `swap` with an infinite swap period.
- WOFF and WOFF2 decoding: the `wuff` crate (MIT) with swb's own Brotli
  and zlib decompressors. A read of its source for robustness found two
  gaps, which swb closes: WOFF 1.0 tables that overlap or repeat, and
  allocation of the declared size before decompression. Limits: 32 MiB
  file, 32 MiB decompressed, 64 MiB decoded.
- Variable fonts get `wght`, `wdth`, `slnt` and `ital` from the font
  properties, clamped to the descriptors (moved from part 2: both Ars
  families are variable). Synthetic bold and oblique follow rules
  measured in Chromium 148 with the probe tool's new `ink` query. The
  probe tool also got `files` (fonts next to the page); 32 cases in
  `tools/probes/web-fonts.json`. Seven new hostile-page cases.
- Review fixes: itemizing text with many loaded faces in one composite
  font took time proportional to characters × faces (49 s for 1,000
  faces and 300,000 characters that no face has). Now itemization keeps
  the result per distinct cluster and checks at most the last 256 faces
  of a composite (0.2 s). The probe tool now waits for
  `document.fonts.ready`, because one Chromium measurement differed
  between runs.
- Scores: Ars geometry 0.1830 → 0.6908, BBC 0.0240 → 0.7775 (pixels
  0.9587 → 0.9942). Ars pixels went down, 0.9595 → 0.9544: the
  headings use `font-variation-settings: "wght" 660` (part 2), so swb
  draws them at `wght` 700, wider, and the cards below move.

## 2026-10-08: M4 feature 2, `:host` and `sizes="auto"`

- `:host`, `:host()` and `:host-context()` parse and match nothing (no
  shadow trees), so Tailwind's base rule `:host,html{...}` now applies
  to html. Chromium rejects `:host()` with a complex selector or a list.
- `sizes="auto"` on lazy images: the candidate is selected after layout
  from the content-box width (`engine/src/page/auto_sizes.rs`) and again
  when the width, viewport or scale changes (at most 8 times per image);
  the old image stays until the new one loads. New in style: `contain`
  and `contain-intrinsic-size` (full syntax); the user-agent rule for
  such images; size containment for replaced elements; the
  `aspect-ratio: auto w/h` hint of `img` attributes (before: `video`
  only).
- Chromium 148, measured: for eager images `auto` is `100vw` and ends
  the list (the spec skips it); swb does the same. With JavaScript off,
  `loading=lazy` defers nothing. For lazy images on a fast local file,
  the chosen source depends on timing (first request against layout);
  the probe file keeps only the cases that do not.
- Correction of the previous entry: the new Ars capture has the same
  103 resources as the first one. Chromium loads the `100vw` candidates
  too, for the hidden (`display: none`) copies of the card images. The
  fixture is in this commit, substituted (86 images; its three font
  families are under the OFL and stay): 4.17 MB instead of 9.25 MB. Ars
  geometry 0.1491 → 0.1876 on the first capture; on the new, substituted
  capture geometry 0.1830, pixels 0.9595.

## 2026-10-08: M4 Ars Technica, `aspect-ratio`

- The owner added two targets, in this order: Ars Technica (target 4,
  M4) and BBC (target 5, M5), both with JavaScript off. Both fixtures
  were captured. Baselines: Ars geometry 0.0531, pixels 0.7841; BBC
  geometry 0.0290, pixels 0.9583. Both pages are close in structure;
  most differences come from web fonts, inline SVG and, on Ars, boxes
  with `aspect-ratio`. The M4 plan is in the roadmap (seven features).
  The BBC fixture is in this commit; the Ars fixture follows after
  `sizes="auto"` (feature 2), because swb now loads 1536w image
  candidates where Chromium loads 384w ones.
- Owner decision: the repository is public, so fixtures must not publish
  photos or commercial fonts. New `swbtools substitute`
  (`just substitute`, docs/testing.md): every raster image becomes a
  generated placeholder of the same size and format; every font without
  a free license becomes DejaVu Sans in the same format (not Liberation,
  the fallback of the test fonts, so the fixture still shows whether a
  browser uses the web font). The BBC fixture is substituted (67
  images, 6 BBC Reith fonts); new baseline geometry 0.0240, pixels
  0.9586.
- Decision: swb draws inline SVG with its own code. ADR 0003 allows
  resvg only for SVG as an image format (`<img>`, CSS images).
- Feature 1, `aspect-ratio` for non-replaced boxes
  (`layout/src/aspect.rs`): blocks, floats, inline-blocks, flex items,
  grid items and absolutely positioned boxes take an `auto` axis from
  the other axis; min and max sizes transfer as for images; a height
  from the ratio is definite for percentage children and grows to the
  content (`min-height: auto`) unless the box is a scroll container.
  Rules measured in Chromium (`tools/probes/aspect-ratio.json`, 22
  cases). On Ars, all 61 `aspect-video` and `aspect-square` boxes now
  match Chromium: geometry 0.0531 → 0.1491, pixels 0.7841 → 0.9592,
  page height 7,420 → 9,505 px (Chromium 9,626).

## 2026-10-08: M3 maintenance

- Two read-only reviewers (layout; the other crates, tools and docs)
  listed duplication, dead code, names and outdated docs; two
  implementers applied the selected items in separate crates. `just
  snapshot` is byte-identical before and after.
- Layout: shared helpers for width/height limits, `auto` self-alignment
  and the alignment edge (new `align.rs`, one `Edge` for grid and
  positioned layout), box-sizing conversion, margins and point/rect
  arithmetic; module docs and a module map in `lib.rs`.
- Other crates: the HTML integer and float microsyntaxes are in
  `dom/src/microsyntax.rs` (five copies removed). As the spec says, `-0`
  now parses as 0 (`maxlength`, `rowspan`, `colspan`, `span`); this is
  the only behaviour change. `escape_html` is in `net`; `mul_255` in
  paint; the shorthand parser is split by property group.
- Tools: `browser.session` and `browser.load_html` replace five copies of
  the Chromium start-up and page loading; Chromium now closes also when
  a page load fails.
- Docs: README status, architecture (flex, replaced and intrinsic
  sizing, the two `calc()` implementations, microsyntaxes), roadmap
  backlog (parsed but unused properties, missing media query units).

## 2026-10-08: Shared tools for the agent workflow

- The owner accepted target 3 (Wikipedia) and closed the question of why
  Chromium source was read (ADR 0021, "Follow-up").
- New tools that the serial agent workflow (CLAUDE.md) relies on, all in
  docs/testing.md:
  - `swbtools probe` (`just probe`): JSON case files with HTML documents
    and queries (boxes, client rects, computed values, script values),
    run in Chromium with the test settings and, with `--with-swb`, in
    swb, compared box by box. It replaces the one-off Playwright scripts
    of M2 and M3. First use: swb and Chromium both give no width to
    columns that only spanning cells cover (a row of `colspan=1000`
    cells is a few px wide in both).
  - `swbtools hostile` (`just hostile`): 64 generated hostile pages, one
    or more for each limit in ADRs 0007, 0010, 0011 and 0015–0019, plus
    generic ones, each run with a time and memory limit and a watchdog.
    Known failures: generated `content` text without a size limit (the
    high-priority bug, next task) and html5ever's quadratic deep nesting
    (100,000 nested `div`s: 13 s).
  - `tools/review-tree.sh` (`just review-tree`): the persistent review
    worktree `../swb-review` with its own target directory.
- CI: `cargo deny --offline` failed on GitHub because the build downloads
  only the crates of the host platform; the workflow now runs
  `cargo fetch --locked` first.
- Generated content limit (the high-priority robustness bug of M3): box
  construction keeps at most 1 MiB of generated text per box tree
  (`MAX_GENERATED_TEXT`; `::before`, `::after`, `::marker` and marker
  numbers) and drops the rest with one warning. 100 KB of `content` on
  3,000 elements took 10 s and 13 GB; now 0.16 s and 84 MiB. At 4 MiB the
  block variant still took 940 MB (inline layout of short words is
  memory-heavy; backlog), so the limit is 1 MiB.

## 2026-10-07: Provenance and attribution

- A provenance audit compared swb with the Chromium, Skia and Servo
  sources that `credits.md` lists. The owner decided to keep the code
  ported from BSD-3 and MPL-2.0 sources with attribution (a clean-room
  rewrite may follow) and to replace data derived from LGPL files with
  measurements.
- New: `swbtools measure` (`tools/swbtools/measure.py`, docs/testing.md)
  measures Chromium 148 black-box. `tools/tests/test_measure.py` compares
  the results with swb's data.
- Text field families (`layout/src/control.rs`): of about 450 candidate
  families, 34 make Chromium size text fields with the width of `0`: the
  31 of the old list and `Kai`, `Lucida Grande` and `#PilGi`. The match is
  on the first family of `font-family` and case-sensitive (`helvetica`
  uses the average width); names that start with `.` are not special.
  swb matched case-insensitively and treated `.` names as special; it
  now does what Chromium does (new layout test
  `forms-zero-width-families`).
- Font size keywords: the measurement confirms the old tables (the 16px
  row is the same in quirks mode). A quirks mode page from
  `page.set_content` after a standards mode page in the same tab kept the
  standards table; the tool loads files instead.
- `ua.css`: the form control rules are rewritten from the HTML Standard
  (spec order, unsupported properties left out) and from measured
  computed values; ruby, frameset, `meter` and `progress` rules cite the
  spec text or the measurement; the `map` and `output` rules (the initial
  value) are gone. Changes where the old rules differed from Chromium:
  disabled checkboxes, radio buttons and image and file inputs keep a
  transparent background; color inputs are `border-box`; range inputs
  have `color: #9d968e`; a text area's placeholder inherits `white-space`
  (it wraps); `marquee` has `text-align: start` (the spec's `initial`)
  and `white-space: nowrap`. `just snapshot` is unchanged. Kept on
  purpose, with comments: the select background (`Field`; Chromium
  computes rgb(239, 239, 239) but draws white) and
  `ButtonBorder` (#767676 in swb, black in Chromium, whose native look
  draws gray).
- Attribution: comments at the derived Chromium code (table layout, grid
  track sizing, sticky offsets, `try_opportunity`, `blocks_content_sizes`,
  the check mark), the Skia data (metric-compatible classes, fake bold
  widths); the Servo-derived `CollapsedMargin` and `BlockMargins` moved
  into their own file with the MPL-2.0 header
  (`layout/src/collapsed_margin.rs`, because the MPL-2.0 applies per
  file); new sections in `THIRD_PARTY_NOTICES.md` with the license
  texts. ADR 0021 records the findings and the decision. Corrected `credits.md`, ADRs 0006, 0010, 0015 and 0017,
  and comments in `floats.rs`, `image_source.rs` (spec order of the
  `<source>` checks, same result as measured), `submit.rs` and
  `raster.rs`.
- Workflow change (owner decision): the M3 workflow (five parallel
  workstreams, stacked reviews, many rebases, everything on the largest
  model) cost far more than the code needed. New rules in CLAUDE.md
  ("Workflow", "Agents"): one feature at a time on `main`, one review per
  feature, typed sub-agents with an explicit model and reasoning effort
  (`.claude/agents/`: `implementer-hard`, `implementer`, `reviewer`,
  `measurer`), short reports, shared measurement tools, and no reading of
  other engines' source code (ADR 0021).

## 2026-10-04: M3 Wikipedia

- M3 runs as five parallel workstreams (floats, positioning, grid, masks,
  scrolling), each in its own worktree. The integrator merges them one
  after another, with a clean-context integration review and a commit
  for each.
- Masks (ADR 0018): `mask-image`, `mask-mode`, `mask-repeat`,
  `mask-position`, `mask-size`, `mask-origin`, `mask-clip`,
  `mask-composite` and `mask`, with Chromium's property model: pure
  aliases (`-webkit-mask-image`, `-size`, `-repeat`), legacy syntaxes
  (`-webkit-mask`, `-webkit-mask-clip`, `-origin`, `-composite`,
  `-position`), and `mask-position` as a shorthand of
  `-webkit-mask-position-x/-y`. `@supports` answers as in Chromium, so
  Wikipedia takes its mask branch. A masked box paints into a layer that
  is multiplied by the mask (alpha or luminance; layers composited with
  `mask-composite`, the bottom layer's operator ignored). Mask images
  load like background images. Measured in Chromium: a failed image
  hides the box, masks do not change hit testing, and a masked box is a
  stacking context but not a containing block. The root element's mask
  and opacity also apply to the canvas background, as in Chromium.
  Limits: 32 layers per box; one memory budget for the layers of all
  open opacity and mask groups (beyond it, opacity groups draw without
  opacity and mask groups draw nothing); a mask work budget (a mask group
  starts only if its work fits). Wikipedia's menu, language, chevron and
  ellipsis icons match the reference (pixels 0.8848 → 0.8859). The
  author's two review rounds found a hang (tiled gradient masks), a
  regression (large opacity groups in full-page screenshots) and smaller
  bugs. The integration review found that full-page screenshots of long
  pages went blank when page-high groups exceeded the budgets: full-page
  screenshots are now rasterized in strips of the viewport height (at
  least 16 Mpx, `swb_paint::rasterize_in_strips`), and the budgets apply
  per strip. It also found that `mask-clip: no-clip` must cover the
  descendants that have their own layer (positioned, opacity, mask). A
  review of those fixes found unbounded gradient-tile work in nested
  masks, a seam at the last strip (it had other budgets), and inexact
  strip offsets at fractional scales: mask groups are now admitted with
  all their work (layer pixels, a cost per row, gradient tiles), the
  strips of a screenshot have equal heights and budgets, and strips are
  drawn in place with the device coordinates of one pass. All fixed with
  tests.
- Scroll containers (ADR 0019): `overflow: auto | scroll | hidden`
  boxes scroll (`hidden` only for scripts and scroll into view). Layout
  computes the scrollable overflow rectangle of each scroll container as
  Chromium does (about 70 cases measured): in-flow margin boxes, line
  boxes and the content height with the end padding, plus the border
  boxes of descendants; hanging `pre-wrap` spaces do not count. The
  engine keeps one offset per element, in whole CSS px and clamped after
  each layout; a scroll rebuilds only the display list. Absolutely
  positioned boxes whose containing block is outside a scroller, and
  fixed boxes, do not move with it (also through block-in-inline
  structures). The wheel scrolls the innermost scrollable container and
  chains outwards; keys use the scroll chain of the focused or last
  clicked node; focus, fragments and automation clicks scroll every
  container into view; selection and hit testing work in scrolled
  content. A root with `overflow: hidden` scrolls for scripts but not
  for the user. The GUI batches wheel events and shows overlay scroll
  indicators. Automation: `dom.scrollTo`, `dom.scrollInfo`,
  `input.wheel`; layout tests can scroll elements first
  (`data-scroll`). Fixture scores are unchanged (nothing on them
  scrolls at 1280×800). Three integration reviews found, among others,
  abspos boxes in positioned inline boxes that did not scroll, a root
  with `overflow: hidden` that scripts could not scroll, one display
  list rebuild per wheel event, and full-page screenshots that reset
  element offsets; all fixed with tests.
- Grid (ADR 0017): CSS Grid Layout 2 without subgrid and masonry.
  Style: the grid longhands and shorthands, `justify-items` and
  `justify-self` (`place-*` set them now), `@supports`. Layout:
  placement and auto-placement (sparse, dense, both flows, named lines
  and areas), Chromium's track collection of ranges and sets, the track
  sizing algorithm as in Chromium, the grid sizing algorithm with
  Chromium's extra row pass, item alignment, intrinsic sizes and
  baselines. Limits: lines clamped to ±10,000, a span budget per layout
  pass, work budgets for placement and named lines; because tracks are
  sets, `repeat(100000, ...)` costs as much as one track. The final
  layout of an item reuses its measuring layout when the result cannot
  differ (without this, Wikipedia's nested grids made layout 25% slower;
  now +5%). Grid scroll containers: as in Chromium, their in-flow
  content is the area of their tracks after content alignment, and items
  count only with their border boxes. Wikipedia: the table of contents
  sits beside the article; size 0.6175 → 0.8100, relative 0.4587 →
  0.7728, geometry 0.0099 → 0.0345; pixels 0.8858 → 0.8722 until floats
  exist (the text is now in its column, where Chromium wraps it around
  floated images). 13 new layout tests match Chromium. The author's
  review found 4 bugs (parser prefixes, image widths with percentage
  heights, measuring-layout reuse with a percentage `flex-basis`,
  stretched size keywords). The integration review found more: the
  measuring layout was reused where a definite height changes the
  result (`min-height`, `max-height`, auto-repeat rows); named areas and
  line names were rebuilt for every layout (20,000 areas shared by 3,000
  grids took 90 s; now they are indexed once per declaration, 0.1 s);
  the grid baseline ignored baseline-aligned items (§11.6); explicit
  `stretch` on images; border-box `min-width`/`max-width` counted the
  padding twice in intrinsic sizes (also outside grid); inline flex and
  grid scroll containers use their first baseline. All fixed with tests.
- Positioning (ADR 0016): absolute, fixed and sticky positioning, 2D
  transforms and `clip: rect()`. Layout puts a placeholder fragment at
  the static position of each absolutely positioned box (block, inline,
  flex and grid layout); after layout, one walk lays out each box in its
  containing block and replaces the placeholder, so each box is laid out
  once, also inside cached flex items and cells. Static positions, the
  CSS 2.2 §10.3.7/§10.6.4 rules, inline containing blocks, flex static
  positions, sticky offsets (Chromium's algorithm, nested sticky boxes,
  sticky boxes in scroll containers) and painted element boxes match
  Chromium in 18 new layout tests, among them reductions of Wikipedia's
  title bar, dropdowns, search field, thumbnails and sticky table of
  contents. Paint: transform groups (matrix, fixed, sticky) in the
  display list, resolved with the scroll offset when rasterizing and
  hit testing; translations are exact, other transforms draw into a
  bounded, anti-aliased layer that shares the per-strip budgets with
  opacity and mask layers. Positioned boxes with `z-index: auto` no
  longer form stacking contexts; positioned boxes keep only the clips of
  their containing block chain; fixed boxes escape all ancestor clips
  except `clip`. Scroll containers: transformed boxes are containing
  blocks for scrolling too, and out-of-flow boxes count in scroll
  ranges. Full-page screenshots keep the viewport layout and draw at
  scroll offset 0, as Chromium does. Wikipedia: geometry 0.0345 →
  0.1577, size 0.8100 → 0.9592, relative 0.7728 → 0.9217, pixels 0.8722
  → 0.8730, no missing element boxes (was 542). Hacker News at 700×900
  now scales its vote arrows (`transform: scale(1.3)` in its narrow
  layout). The integration review found the `inset: 0; margin: auto;
  height: fit-content` centering idiom broken (intrinsic height keywords
  stretched), self-alignment of abspos boxes ignored (now CSS Position 3
  §4.4, measured in Chromium), fragment navigation to a fixed element
  that scrolled the viewport, full-page screenshots of a scrolled page
  that differed from Chromium, and hostile pages that took 1–39 s (an
  opacity group around a tiny fixed box became a full-surface layer;
  sticky offsets were recomputed per group; the display-list build was
  O(items × depth)). Fixed: the display list collects items in chunks
  and computes group bounds in one pass, sticky offsets are cached per
  frame, fixed content has its own bounds; those pages take 2–31 ms.
  Typed URLs that differ only in the fragment scroll without a reload;
  selection honours `clip: rect()`; `html { position: absolute }`
  works.
- Floats (ADR 0015): an exclusion space per block formatting context
  (`layout/src/floats.rs`), the float placement rules of CSS 2.2
  §9.5.1, lines shortened by floats that move down when they do not fit,
  floats in inline content on the line if they fit, clearance with
  Chromium's margin rules (a block that clears floats waiting for its
  container's position goes exactly to the clearance offset), boxes that
  establish a BFC beside floats, auto heights that include floats, and
  intrinsic sizes with floats. Block layout knows positions in the BFC:
  containers whose top margin can still collapse wait in a chain, and
  their floats are placed when the chain resolves, without a second
  layout. Paint phases (CSS 2.2 Appendix E): block backgrounds, floats,
  inline content; positioned boxes keep tree order. Floats count in the
  scrollable overflow of scroll containers, and abspos boxes next to
  floats get Chromium's static positions. Limits: 10,000 floats per
  BFC, a work budget per layout pass, at most 64 attempts per line.
  Wikipedia: size 0.9592 → 0.9631, relative 0.9217 → 0.9377, pixels
  0.8730 → 0.8903. The author's two reviews found 2 resource risks and
  11 layout differences; the integration review found opacity and masks
  on inline boxes ignored, positioned boxes out of tree order, the
  forced clearance offset, lines in `nowrap` containers, and hostile
  pages that took up to 130 s (now under 1 s); all fixed with tests.
- Flexbox: the flex container's own `min-height` and `max-height` take
  part in the flex algorithm (single-line cross size, column main size
  and line breaking, multi-line cross size); `align-content: stretch`
  grows lines before items stretch; the container's baseline comes from
  the visually first item of the first line, synthesized from its
  border box (CSS Flexbox 1 §8.5, measured in Chromium). Wikipedia's
  icon buttons were 36 px lines instead of 32 and the page toolbar 36 px
  instead of 33; the header and toolbar now match. Known failures
  `flex-container-min-size`, `inline-flex-baseline` and
  `flex-align-content-stretch` pass.
- Media elements (ADR 0020): `<video>` and `<audio>` are replaced
  elements. A video's natural size is its poster's; without a loaded
  poster it has none (300×150 without a ratio, as in Chromium). `width`
  and `height` on `<video>` map to `aspect-ratio: auto w/h`, and the
  computed `aspect-ratio` keeps `auto` (fixes an M2 backlog item). New
  `object-position`; `<img>` and posters use `object-fit` and
  `object-position`. Posters load like `<img>` sources; media resources
  are never fetched. The controls approximate Chromium's with scripting
  disabled (always shown); their parts drop out at sizes measured in
  Chromium. Children (`<source>`, `<track>`, fallback content) get no
  boxes. Wikipedia: the video thumbnail now has its size, so the page
  below it is no longer 80 px too high; with floats and the flex fixes,
  geometry 0.2073 → 0.5136, pixels 0.9378 → 0.9766, no extra boxes.
- Responsive images: `<img srcset>` with `x` and `w` descriptors,
  `sizes`, and `<picture>`/`<source>` (`media`, `type`, `sizes`,
  `width`/`height`) (`engine/src/image_source.rs`, `css/src/sizes.rs`;
  ADR 0011 and 0007 updates). Measured in Chromium 148: candidates are
  sorted by density, and the first with density ≥ the device pixel
  ratio wins, else the densest. Natural sizes are divided by the density
  (also for SVG and `object-fit`). A selected `<source>` with `width` or
  `height` replaces both dimension attributes of the image. Sources are
  selected again after viewport or scale changes; an image keeps its
  source until the new one has loaded. Wikipedia's footer icons now have
  Chromium's sizes (size 0.9669 → 0.9694, relative 0.9476 → 0.9501).
  Two reviews found a negative-zero density bug, cancelled loads that
  never retried, and unbounded waiting lists; all fixed. The Wikipedia
  fixture gained the six 2x thumbnails that swb now requests at scale 2,
  and one icon of the 700 px layout (recorded with `--record-missing`).
- CSS counters: `counter-reset`, `counter-increment`, `counter-set`,
  `counter()` and `counters()` in `::before`, `::after` and `::marker`,
  and the `list-item` counter. A pass at the end of style computation
  (`style/src/counters.rs`) resolves counters to text and computes the
  ordinal values of list items; layout no longer numbers list items
  (ADR 0007 update). Where Chromium 148 differs from CSS Lists 3, swb
  follows Chromium (measured with DOM snapshots): the scope of a nested
  reset, `display: contents`, markers that use HTML ordinal values,
  `list-item` without presentational hints. The unit tests are generated
  from Chromium output; a review fuzzed about 13,000 values against
  Chromium. Wikipedia: the backlink letters of the references (geometry
  0.5136 → 0.5382, size 0.9694 → 0.9862). A provenance check against
  the Blink files that were read found no copied code.
- Line breaking and text measurement: break opportunities follow
  Chromium (URLs break only after `-` and `?`, a break after every run of
  spaces, UAX #14 for Unicode 17 with measured tailorings), with
  `word-break: break-all | keep-all`, `hyphens: none`, `overflow-wrap:
  break-word | anywhere` and `word-break: break-word` (in layout and
  min-content); rules at element boundaries (isolating boxes, `nowrap`
  box ends, atomic inlines in nowrap text); text widths as Chromium
  computes them (the font size truncated before shaping, item widths
  rounded up to 1/64 px, a 1/64 px fit tolerance); synthesized small
  capitals; right-to-left scripts shaped right to left with context;
  multicol containers are block formatting contexts. The first line
  breaker was a port of Blink's LGPL `text_break_iterator.cc`; a review
  found it before the commit, and a clean-room agent rewrote
  `text/src/linebreak.rs` from UAX #14, the Unicode data and about
  360,000 measured strings (`swbtools linebreaks`; tables generated from
  the measurements; 27 known differences listed). Wikipedia geometry
  0.5382 → 0.9733, pixels 0.9766 → 0.9936; Hacker News pixels 0.9909 →
  0.9981. The known failure `nowrap-atomic-inlines` passes. A review of
  the combined change found the kerning scale (now the 16.16 scale of the
  size), small-caps line metrics in fallback fonts, `bidi-override`
  shaping and smaller items; all fixed. With the counters, Wikipedia
  geometry is 0.9978 (pixels 0.9936): target 3 is done.
- Text: a variable font with a malformed `wght` axis (min > max) no
  longer panics; the axis range includes the default value, as in
  HarfBuzz.
- `just snapshot DIR` (`tools/snapshot.sh`) writes swb's rendering of
  all fixtures and layout tests, to check that a refactoring changes
  nothing (testing.md). The M2 maintenance pass used it first.

## 2026-10-04: M2 maintenance

- End-of-milestone review of the whole codebase. Five clean-context
  agents each reviewed a group of crates (css and style; layout and text;
  dom and paint; engine; net, automation, swb and the Python tools) in
  their own git worktree; the integrator merged the patches and fixed the
  shared docs.
- Behaviour is unchanged. A snapshot script wrote the layout dump, the
  DOM dump and full-page screenshots (1280×800, 700×900, and 1280×800 at
  scale 2) of every fixture, and the layout dump and a screenshot of
  every layout test. The snapshots before and after the changes are
  byte-identical, for each agent's patch and for the merged result.
- Dead code removed: unused helpers in css (`Declaration::parser`,
  `MatchingContext::clear_caches`, `Stylesheet::parse`), layout
  (`Rect::size`, `Rect::outset`, unused table builder methods and
  parameters), net (`impl Extend for Headers`) and engine
  (`TextEdit::select`).
- Duplication removed: style's Bloom filter keys come from the parsed
  selector (`Selector::ancestor_keys`) instead of serializing and
  re-tokenizing each selector; one shrink-to-fit width; shared table
  helpers; one word lookup for double clicks and text fields; one
  "nearest link" search; one editability rule for form controls; the
  history entry holds its `Commit`; shared test helpers in every crate.
- Narrowed many `pub` and `pub(crate)` items; corrected wrong spec
  section numbers and anchors (tables, form encodings), outdated module
  docs and comments; dated update notes in ADRs 0004 to 0010 and 0013.
- The reviewers found 10 bugs (one panic on a malformed system font).
  They are listed in the roadmap ("Backlog from the M2 maintenance
  review").
- M3 plan: five workstreams (floats, positioning, grid, masks,
  scrolling). `@font-face` and `@import` moved to "Later": the Wikipedia
  page uses neither.

## 2026-10-03: M2 Hacker News

- The owner accepted Hacker News (target 2).
- The owner allowed the Public Suffix List (MPL-2.0) as a one-off license
  exception (ADR 0014). `deny.toml` now declares the real license of the
  `psl` crate (the crate metadata says only MIT/Apache-2.0, and its copy
  of the list has the license header removed) and allows MPL-2.0 for that
  crate only, and only `swb-net` may depend on it. `THIRD_PARTY_NOTICES.md`
  states the license for binaries; it also names the WHATWG-derived
  stylesheets (CC BY 4.0), which binaries contain too. `fixtures/pages`
  has a note that the page snapshots are third-party material.

- Cookies: an RFC 6265bis jar in the `net` HTTP client with Chromium's
  defaults (Lax by default, schemeful sites, a 400-day cap, 180 cookies
  per domain and 3300 in total), the Public Suffix List through `psl`,
  `Request::initiator` for `SameSite`, `Request::post` and the `Origin`
  header, automation `cookies.get` and `cookies.clear` (ADR 0012). The
  list's data is MPL-2.0 (decided later the same day: ADR 0014).
- Tables (ADR 0010): Chromium's LayoutNG algorithm for column widths,
  row heights and cell alignment, automatic and fixed layout, separated
  and collapsing borders, captions, column and row backgrounds, the
  anonymous table fixup; `-webkit-center` for blocks; the quirks-mode line
  height and table cell rules; boxes for `<br>` and for inline elements
  around blocks. Hacker News geometry went from 0 to 0.9876 (the rest is
  the search field, which needs forms); Wikipedia size 0.58 → 0.62. 20
  new layout tests against Chromium. The author's review found three bugs
  (memory of block-in-inline boxes, a `display: none` sibling that split
  anonymous tables, column backgrounds painted per spanned column); the
  integration review found more: trimmed trailing spaces added a strut in
  quirks mode, table `min-height` was ignored, `<img>` with a table
  display lost its image, row gradients restarted in every cell, floats
  did not make a cell non-empty, and hostile tables could cost quadratic
  time (grid placement) or much memory (collapsed borders). All fixed
  with tests; grid placement now uses a segment tree, and collapsed
  borders have a budget per layout pass. Two findings (struts in list
  items, row backgrounds under hidden empty cells) were checked against
  Chromium and are not bugs.
- SVG images (ADR 0011): resvg 0.48 (no `text` feature), detection by
  MIME type as in Chromium, natural sizes and the CSS default sizing
  measured against Chromium (24 `<img>` cases), rendering at device
  resolution with a size cache, only `data:` URLs inside SVG images.
  Three adversarial review rounds with about 75 attack files found
  crashes (reference cycles and chains that overflow the stack in usvg)
  and unbounded costs (`<use>`, markers, arcs, entities, CSS, masks,
  filters); each is bounded before conversion or rendering. Each frame
  has an SVG rendering budget; when it runs out, images use a cached
  rendering of another size or stay blank until a repaint. The Hacker
  News logo and vote arrows now match Chromium (pixels 0.9901 →
  0.9909).
- Two integration reviews of the SVG work found: a cost estimate that
  missed canvas area outside the image box (a crafted file could render
  for tens of seconds), infinite sizes from extreme aspect ratios, NaN in
  the cost bound, a redraw loop in the first per-frame budget design
  (replaced by the simpler budget above), nondeterministic choice of a
  cached rendering, and replaced-element sizing bugs that also affected
  raster images: `max-width` and `max-height` together (now the full CSS
  2.2 §10.4 table), definite heights of images in row and column flex
  containers (the known failure `flex-replaced-item` now passes). All
  fixed with tests; 3 new layout tests against Chromium. Wikipedia pixels went down 0.8854 → 0.8850: its SVG images now
  render, at positions that are wrong until floats and grid exist (M3).
- Forms (ADR 0013): control state outside the DOM (`Forms`, keyed by
  node), one editing model (`TextEdit`) for page fields and the address
  bar, controls as atomic boxes with Chromium's sizes and light theme,
  activation of buttons, checkboxes, radio buttons, labels and links
  around controls, the HTML form submission algorithm with the
  urlencoded, multipart and text/plain encodings, `POST` navigations
  that keep their body and initiator in history ("Confirm form
  resubmission" on back/forward), automation `input.type` and
  `dom.value`. The integration review found a CSRF-relevant bug (a
  reloaded `POST` lost its initiator, so it was sent without `Origin` and
  with same-site cookies), a duplicate `POST` on reload during a pending
  submission, quadratic time in disabled fieldsets, and activation and
  spec details; all fixed with tests. `swb_net::Response::redirected`
  lets a post/redirect/get to the same URL drop the body.
- Flex containers: max-content widths now include `column-gap`
  (Wikipedia geometry 0.0072 → 0.0099; pixels 0.8850 → 0.8848).
- Hacker News is done: geometry 1.0 (818 elements), pixels 0.9909; the
  search form submits to hn.algolia.com, links and history work.
- M2 ran as four parallel workstreams (cookies, tables, SVG images,
  forms), each in its own worktree and with its own commit. Each
  workstream had one or two integration reviews after the merge; they
  found real bugs in every workstream.
- Next: end-of-milestone maintenance for M2, then M3 (Wikipedia).

## 2026-10-02: M1 maintenance

- End-of-milestone review of the whole codebase. Five clean-context
  agents each reviewed a group of crates (css; style; layout and text;
  dom, paint and engine; net, automation, swb and the Python tools) in
  their own git worktree. The integrator merged the patches, made the
  changes that cross crates, and fixed the docs.
- Behaviour is unchanged. The layout agent compared full-precision
  fragment-tree dumps of all layout tests and fixtures (at several
  viewport sizes) before and after its changes: byte-identical. Scores
  and layout tests are unchanged.
- Removed dead code: unused CSS parser entry points and helpers, unused
  style value types (`BackgroundLayer`, `Sides`, `Corners`,
  `ContentItem::Attr`, `GridAutoFlowDirection`), `dump_subtree`, the
  `markup5ever` dependency, an unused manifest helper in the tools.
- Removed duplication: one CSS-wide keyword table; one 1-to-4 value
  expansion for box shorthands; one `@supports` condition-or-declaration
  fallback; `Fragment::move_by`; one `percent_decode` (exported by
  `net`); the viewport limits and the network thread count now live in
  the engine (`check_viewport_size`, `check_scale`,
  `PageConfig::DEFAULT_NETWORK_THREADS`); `Page::is_fully_loaded`
  replaces a copy in the automation crate; shared engine test helpers.
- Split `engine/src/page/mod.rs` (1,220 lines) into `mod.rs`,
  `loading.rs`, `pipeline.rs` and `scroll.rs`. The moved code is
  unchanged apart from visibility (`pub(super)`).
- Narrowed `pub` items, fixed wrong spec section numbers, added spec
  URLs, corrected module docs, ADR 0006 and 0007 details,
  `architecture.md`, `testing.md`, `performance.md` and `credits.md`.
- Fixed a flaky test: `connection_refused_is_an_error` connected to a
  port that a concurrently running test server could take; it now uses
  port 0.
- The reviewers found 12 bugs and 3 undocumented paint simplifications.
  They are listed in the roadmap ("Backlog from the M1 maintenance
  review").

## 2026-10-02: M1 senko.net, automation API

- The owner checked senko.net (hover, Tab, Enter, selection, copy) and
  copy and paste into Wayland applications, including middle-click paste:
  target 1 is accepted.

- Automation: the `automation` crate (tungstenite server, one thread per
  connection, methods executed on the page thread, `HeadlessBrowser`,
  blocking `Client`), `swb --remote-port PORT` in headless and GUI mode, a
  Python client (`swbtools.automation`). ADR 0008 accepted with changes: no
  events yet (a tungstenite connection cannot write while it waits for a
  read), an `Origin` check so web pages cannot drive swb, `browser.close`.
- Interaction (ADR 0009): mouse down/move/up and key events in the
  engine; hover, active, focus, focus-visible and target states with a
  restyle only when a selector depends on the state, and no relayout when
  no style changed; cursor shapes; Tab order per the HTML spec; links follow
  on release, not on press.
- Text selection: the box tree records a source map from processed text to
  DOM offsets, text fragments carry caret stops, and a selection is a pair
  of DOM text positions. Copy uses the `innerText` rules. The GUI copies
  with Ctrl+C (arboard; X11 fallback on GNOME) and sets the primary
  selection.
- Measured in Chromium 148 and matched: selection colors, the focus ring of
  `outline-style: auto` (and the UA rule that links get `outline-offset:
  1px`), the 4 px drag threshold.
- `swb --record-missing DIR` and `just capture-missing NAME`: add only the
  missing responses to a fixture. Added the two footer SVGs that swb
  requests to the Wikipedia fixture (scores unchanged).
- `swb --bench N` and `just perf`: per-stage timings; the baseline is in
  [performance.md](performance.md). Wikipedia: 30 ms for the whole pipeline.
- `dom.outerHtml` needed an HTML serializer (`swb_dom::outer_html`);
  `dom.querySelectorAll` uses `swb_style::query_selector_all`.
- senko.net is done: geometry 1.0, and hover, focus, selection and the
  narrow layout look the same as in Chromium side by side.
- Review before the commit: three clean-context reviewers (layout, paint,
  style and dom; engine interaction; automation, binary, tools and docs)
  reported about 40 findings, no panics. Fixed with tests: a full restyle
  on every scroll step on pages with `:hover` rules (now `:hover` waits for
  the next mouse movement, as in Chromium); the old page's stylesheet
  applied to the new document for `:target`; `user-select: none` text
  selected and copied; tabs between table cells; the cursor one movement
  behind `:hover` rules; focus on hidden elements; skip links; triple
  click selected the whole block instead of the paragraph; the selection
  highlight now fills the line box; focus rings enclose images in links;
  disabled controls in disabled fieldsets; the `browser.close` reply lost
  when swb exited quickly; keepalive pings that broke long requests in the
  Python client; server limits (request size with close code 1009, 16
  connections, a 10 s deadline for the handshake; all tested except the
  deadline).
- A second review of the fixes found 9 more issues, fixed with tests: the
  line box of text in relatively positioned inline boxes, a trailing tab
  in copied text, focus rings around clipped overflow, Chromium's exact
  "center if needed" rules for large elements, `tabindex` with leading
  zeros, and the server tests above.
- Next: M2 (Hacker News): tables, SVG images, forms, cookies.

## 2026-10-02: M0 foundation

- Built all crates of the pipeline. Five crates were written by
  sub-agents in parallel from written specifications (`net`, `css`, `text`,
  `style`, and the Python tools); the integrator wrote `dom`, `layout`,
  `paint`, `engine` and `swb`, and integrated everything.
- Captured fixtures for the three targets and their Chromium references.
- First comparison results (test fonts, 1280×800):
  - senko.net: all 65 element boxes match Chromium within 0.1 px.
  - Hacker News: readable; geometry 0 because tables are laid out as
    blocks (M2).
  - Wikipedia: readable; geometry 0.007 because of missing grid, floats
    and SVG (M3).
- Bugs found by comparing with Chromium and fixed:
  - Anonymous boxes inherited the initial border width (3 px) without the
    "style none means width 0" rule.
  - Blink floors the ascent half of the leading.
  - `overflow: clip` (the UA style of `img`) does not remove the flex
    automatic minimum size; only scroll containers do.
  - Piece grouping dropped the start of an inline box at a line start.
  - Chromium shapes across inline element boundaries (kerning between a
    space and a link's first letter) and removes trailing spaces at line
    ends, so split inline boxes end before them.
  - Nested positioned boxes were painted before their positioned
    ancestor; paint order now uses stacking contexts.
  - With system fonts, Chromium maps `serif` to Times New Roman,
    `sans-serif` to Arial, `cursive` to Comic Sans MS and `fantasy` to
    Impact (Chrome's default font settings), not to fontconfig's choice.
- Added `--with-chrome` to render the whole window headless, so the user
  interface can be checked without a display.
- Review before the first commit: three clean-context reviewers (layout;
  dom/paint/engine/swb; net/text) reported about 70 findings, most verified
  against Chromium. The same agents then fixed the agreed subset with
  regression tests: stack overflows on deep documents, exponential flex
  layout, quadratic text processing, infinite geometry, unbounded memory
  (glyph cache, image decoding, `file:` devices, content-coding chains),
  paint order and clipping of positioned boxes, hit testing in paint order,
  border colors with radii, seams at fractional scales, fragment
  navigation and history, cancelled requests on navigation, and many
  layout details (markers, margins, baselines, `vertical-align`,
  `text-transform`). The rest is in the roadmap backlog; layout items have
  a failing layout test each (`tests/layout/known-failures.txt`).
- A final review of the fixes found a few more problems, fixed with tests:
  the first version of the flex cache stored a deep copy per nesting level
  (memory proportional to depth × page size); fragment children are now
  shared. Also quadratic paths in itemization, box construction and the
  toolbar; gaps in rounded borders; fragment scrolling before images
  load; `file:` loads from web pages are now blocked as in Chromium.
- Next: M1 (automation API, hover, text selection), then tables (M2).

## 2026-10-02: Project start

- Initial Q&A with the owner. Requirements and answers recorded in
  [ground-rules.md](ground-rules.md).
- Installed the Rust toolchain with rustup (Debian's rustc 1.85 is too old
  for current winit and image crates).
- Checked the three initial targets:
  - senko.net: 5.6 KB, inline `<style>`, flexbox, media queries, no scripts.
  - Hacker News: 34 KB, table layout, one stylesheet, SVG logo, one script
    (not needed for reading).
  - Wikipedia "Web browser": 415 KB, two `load.php` stylesheets, Vector 2022
    skin, JavaScript not needed for reading.
- Wrote ADRs 0001–0005: ADR process, Rust, dependency policy, engine
  structure, testing strategy.
- Next: M0 vertical slice (see [roadmap.md](roadmap.md)).
