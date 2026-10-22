# JavaScript survey of the target pages

What the scripts of the five target pages do, measured in Chromium with
JavaScript on. The M8 design (Web API bindings, event loop, scripts during
parsing) uses it to order the work. The document gives facts and counts; the
full data (every member, every histogram) is in `out/js-survey/NAME/survey.json`
(local, git-ignored, with the fixtures).

## How the data was made

- Date: 2026-10-22. Chromium 148.0.7778.96 (Playwright 1.60.0), the test
  settings of [testing.md](testing.md) ("Chromium settings"), viewport
  1280x800, JavaScript on, service workers blocked.
- Capture: `swbtools capture --js URL NAME` from the live sites. Fixtures in
  `fixtures/js/NAME/`: local only, never committed (they hold the sites'
  scripts). No site blocked headless Chromium or showed a challenge page.
- Survey: `swbtools js-survey --all` (`tools/swbtools/js_survey.py`,
  `js_tracer.js`; method in testing.md, "Survey the scripts of a page"). Every
  request is answered from the fixture. Per page: one traced run, 3 runs with
  JIT, 3 runs with `--js-flags=--jitless` (median), one run with JavaScript
  off.
- Phases: `parse` is until `DOMContentLoaded`, `load` until the `load` event,
  `after` the rest (scroll through the page, wait for network idle up to 15 s,
  wait 2 s, scroll back). A call that a `DOMContentLoaded` handler makes
  counts as `load`.
- Operations (`ops`) are calls, property reads (get), property writes (set)
  and `new X()` of Web API members, counted at the member where the
  interface defines it (`Node.parentNode`, `HTMLElement.offsetWidth`).

Limits of the method:

- A wrapper counts the calls of page scripts only. Calls that the tool makes
  for itself are paused or excluded; the tracer's own bookkeeping does not
  count.
- Not countable: `window.location`, `window.document`, `window.top`,
  `window.window` (non-configurable; listed as unwrapped), named-property
  access (`localStorage.x = 1`), and anything inside cross-origin iframes (only
  the top frame is traced). Location use is therefore missing from the counts;
  `window.history` and `History.*` are counted.
- The proxies make scripts slower; the timing runs are untraced.
- Replay misses requests with new URLs (analytics beacons with ids and
  timestamps, ad calls). Replay also cannot satisfy some CORS requests (see
  Ars Technica). The pages therefore run a little differently from the live
  site: ad slots stay empty, and the consent dialogs cannot always be
  accepted.
- `ScriptDuration` and `TaskDuration` (Chrome DevTools Protocol
  `Performance.getMetrics`) are totals since page start, including the scroll
  and the wait. They include the time that Chromium needs to compile scripts.
  JIT versus jitless differs little for the small pages, so the numbers mostly
  measure compile, parse and DOM work, not hot JavaScript loops.
- "First party" in the script tables is a rough match of the registrable
  domain. Hosts that the sites own under another domain (`bbci.co.uk`,
  `arstechnica.net`, `conde.digital`) are marked in the text.
- One run per page, one day. Counts of ad and timer activity vary between
  runs.
- The mutation phase is the phase at the time the observer delivers a record;
  parser inserts and script inserts are not separated.
- For the argument histograms the tracer converts arguments with `String()`.
  For an object argument this calls the page's own `toString` a second time.
- When the tracer's own bookkeeping fails, it catches the error and loses
  that one count (`tracer_errors` in `survey.json`).
- The run with JavaScript off scrolls with 100 ms pauses, the traced run with
  250 ms. The difference can change only content that loads while the page
  scrolls.

## Summary

| Target | Scripts compiled (KB) | `ScriptDuration` JIT / jitless | JS on vs off, first viewport | JS features a user sees |
|---|---|---|---|---|
| senko-net | 0 | 0 | 0.00 % | none |
| hacker-news | 1 (5 KB) | 0.6 / 0.4 ms | 0.00 % | none on the logged-out front page |
| wikipedia-web-browser | 14 (1,137 KB) | 74 / 75 ms | 2.42 % | many: menus, TOC, appearance, collapsibles, previews |
| ars-technica | 214 (5,219 KB) | 318 / 331 ms | 63.1 % | consent dialog, menus, theme, view settings, ads |
| bbc | 93 (5,126 KB) | 490 / 955 ms | 75.6 % | consent dialog, menu with search, carousels, save popover |
| craigslist-posting | 9 (1,234 KB) | 111 / 111 ms | 31.0 % | photos and thumbnails, map, toolbar, reply (the page is empty without JavaScript) |

Read across the targets:

- Two pages (senko-net, hacker-news) need no JavaScript to look and work
  right. Hacker News has a 5 KB script whose handler matches nothing on the
  logged-out front page.
- Wikipedia is first-party only and has no errors in replay. Its page looks
  the same with JavaScript on, except for the pinned sidebars and the video.
- Ars Technica and BBC are mostly third-party and framework code. A consent
  dialog covers the first viewport on both (the main cause of the pixel
  difference). Their real script work is DOM construction, feature detection,
  analytics and ads.
- The members that the survey sees most are DOM reads (`Node.nodeType`,
  `parentNode`, `nextSibling`, `NamedNodeMap.length`), `getComputedStyle` (Ars
  Technica), `setTimeout`, observers (`IntersectionObserver`, `MutationObserver`,
  `ResizeObserver`), `Document.cookie` and `localStorage`, `URL` and
  `URLSearchParams`.
- Chromium compiles 5 MB of script for each of the two large pages, about
  80 % of it third-party. The jitless script time of BBC is about 1 s.

## senko-net

- Scripts: none (no `<script>`; Chromium compiles nothing from the page).
- API use: none. The MutationObserver of the tracer saw only the parser: 186
  records, 332 elements added.
- Requests: 3 (document, 2 images), all served. No errors.
- Chromium: `ScriptDuration` 0; `TaskDuration` 9 ms; layout 1 (3 ms), style
  recalculation 1; 195 nodes.
- JS on and off give the same 65 elements and identical pixels.
- JavaScript features: none.

## hacker-news

- Scripts: 1 (`hn.js`, 5 KB, first-party, classic blocking at the end of the
  body). No script inserts another.
- API use (11 members, 29 ops): `Document.getElementsByClassName`,
  `Element.innerHTML`, `EventTarget.addEventListener` (one `click` listener
  on `document`), `parseFloat`, `isNaN`. All in the parse phase. Nothing runs
  after load: no timers, no requests from scripts.
- `document.write`: none. Iframes: none. Errors: none.
- DOM mutations: only the parser (1,310 records, 9,827 elements).
- Chromium: `ScriptDuration` 0.6 ms (JIT) and 0.4 ms (jitless); `TaskDuration`
  22 ms; layout 1 (4 ms); style recalculation 1 (2 ms); 1,336 nodes.
- JS on vs off: 821 elements both, 0.00 % of the pixels differ.
- JavaScript features, from the script: the `click` handler on `document`
  acts on links with class `clicky`: vote arrows (XHR `GET vote?...`), hide
  (`fetch` and `insertAdjacentHTML`), comment collapse (`[–]`). The
  logged-out front page has no `clicky` links, so the handler does nothing.
  Clicking a vote arrow navigates to the vote URL (no JavaScript effect).
  The search field is a plain form.

## wikipedia-web-browser

Scripts (14 compiled, 1,137 KB):

| Origin | Scripts | KB |
|---|---|---|
| en.wikipedia.org (`load.php` ResourceLoader bundles; first-party) | 7 | 1,128 |
| inline in the page | 3 | 7 |
| `eval` or `new Function` | 4 | 2 |

HTML: 5 `<script>` elements (1 external async, 3 inline, 1 data block). Seven
more are inserted by scripts, all async: 5 `load.php` bundles (ResourceLoader
loads modules this way), one inline, and `Special:CentralAutoLogin` (a
cross-site login probe). None is a module.

APIs by area (176 members, 12,341 ops; parse 29, load 9,754, after 2,558):

| Area | Members | Ops | Top members (ops) |
|---|---|---|---|
| dom-core | 70 | 10,253 | `Node.nodeType` 2,611, `ownerDocument` 1,365, `parentNode` 836, `nodeName` 579 |
| events | 9 | 446 | `addEventListener` 430 (402 in load), `removeEventListener` 3 |
| html-elements | 36 | 618 | `HTMLAnchorElement.type` 89, `URL.href` 89, `HTMLElement.dataset` 46 |
| cssom-layout | 11 | 42 | `clientHeight`, `scrollHeight`, `offsetParent` 8 each, `matchMedia` 5 |
| timers | 4 | 97 | `setTimeout` 71, `requestIdleCallback` 20, `requestAnimationFrame` 4 |
| network | 1 | 1 | `Navigator.sendBeacon` 1 |
| storage | 8 | 104 | `Storage.getItem` 37, `localStorage` 17, `Document.cookie` 12 |
| observers | 4 | 159 | `IntersectionObserver.observe` 144 (132 after load), `disconnect` 13 |
| history-location | 1 | 2 | `window.history` 2 |
| canvas | 0 | 0 | |
| workers-other | 0 | 0 | |
| browser-misc | 17 | 41 | `Navigator.userAgent` 8, `Crypto.getRandomValues` 5 |

Details:

- Listeners (149 `click` on 111 targets, 86 `mouseover` and 86 `mouseout`
  pairs, 22 `keydown`, 13 `beforematch`, 11 `change`, 10 `input`, 10 `keyup`,
  5 `hashchange`, 5 `keypress`, `focusin`, `scroll`, `copy`, `visibilitychange`).
  Targets are collapsible-table buttons, dropdown content, pin/unpin buttons,
  appearance radios (`change`), the search box, reference links
  (`mouseover`).
- Elements created: `span` 56, `div` 41, `input` 13, `label` 11, `a` 10,
  `fieldset` 7, `script` 7, `li` 6, `style` 4, `form` 3.
- Selectors: `querySelector` (`.vector-toc-toggle`, `a`,
  `.vector-dropdown-*`), `querySelectorAll` (`:scope > caption`,
  `:scope th,:scope td`, `[data-mw-tmh]`), `matches` (`.reference`,
  `a[href^="#CITEREF"]`, `tr`, `table`), `closest` (`.vector-toc-level-1`,
  `.vector-menu`). The selectors use `:scope`, attribute selectors and
  selector lists.
- `setAttribute`: `class` 70, `title` 46, `aria-label` 42, `hidden` 41,
  `aria-expanded` 29. `Element.innerHTML` set 20 times (2,160 characters).
  `matchMedia`: `(max-width: 1119px)`, `(prefers-color-scheme: dark)`,
  `(max-width: 767px)`. `setTimeout` delays: 0 (51), 101-1000 (12), >1000 (8).
  Callbacks that ran (load + after): timeouts 34 + 32, idle callbacks 4 + 16,
  animation frames 2 + 2.
- `document.write`: none. Iframes: none.
- Mutations: parse 7,659 records (55,016 elements added, parser). Later: 381
  records, 251 elements added (buttons 13, div 9, span 8, li 6, script 4,
  style 4, fieldset 3), 38 removed; 290 attribute changes (`class` 81, `title`
  64, `aria-label` 42, `hidden` 41, `aria-expanded` 29).
- Errors in replay: none (no console errors, no uncaught exceptions). Requests:
  44 (32 images, 8 scripts, 2 stylesheets, 1 ping), none missing. The tracer
  caught one `Illegal invocation` in its own bookkeeping (one count lost; no
  effect on the page).
- Chromium: `ScriptDuration` 74 ms (JIT), 75 ms (jitless); `TaskDuration` 211 /
  217 ms; layout 14-15 (18-19 ms); style recalculation 18-19 (25-29 ms); 9,657
  nodes; JS heap 9 MB.
- JS on vs off: 4,169 against 4,051 elements; 2.42 % of the pixels differ in
  the box x 44-1179, y 143-766: with JS the TOC and the right-hand
  "Appearance" panel are pinned with "hide" buttons, TOC sections have
  collapse chevrons, and the video shows a poster with a play button and
  "0:24" instead of the native `<video controls>`.

JavaScript features found (replayed fixture, Chromium):

| Feature | Evidence |
|---|---|
| Appearance: color theme (light, dark, automatic) | Click on the Dark radio changes 95 % of the pixels; `change` listeners on the radios; `matchMedia(prefers-color-scheme: dark)` |
| Appearance: text size and page width | Radio `change`; font size "Large" changes 13.6 % of the pixels; width "Wide" changes layout |
| Pin and unpin the TOC and the Tools/Appearance menus (move to sidebar, hide) | `click` on `vector-pinnable-header-*` buttons; layout change 12 % |
| TOC section collapse chevrons | `aria-expanded` buttons (`Toggle ... subsection`) |
| Collapsible tables and navboxes (`show`/`hide`) | 13 `click` + `keydown` listeners on `.mw-collapsible-toggle`; click changes the page by 21 % |
| Sticky header with search and tools after scrolling down | Scroll to 1,500-2,500 px changes 16 % of the pixels; `IntersectionObserver` (144 observes) |
| Search suggestions (typeahead) | Focus on the search box loads the typeahead modules (`load.php`, `cirrus-config-dump`); the suggestions come from the live API (not replayable) |
| Reference preview on hover | `mouseover` on reference links adds 15 elements |
| Video player (poster, play button) | Click on `a.mw-tmh-play` requests the `.webm` files |
| Lazy images, language dropdown, main menu | `load.php` requests after interaction; dropdowns also work without JavaScript (checkbox hack) |
| Back/forward and anchors | `hashchange` listener (5) |

## ars-technica

Scripts (214 compiled, 5,219 KB). The HTML has 28 `<script>` elements (9
external: 6 classic blocking, 2 async, 1 defer; 17 inline; 2 data blocks); 46
exist at the end:

| Origin | Scripts | KB |
|---|---|---|
| `privacy.condenastdigital.com` (Fides consent) | 1 | 1,978 |
| `www.googletagmanager.com` | 2 | 971 |
| `securepubads.g.doubleclick.net` (Google Publisher Tag) | 1 | 671 |
| `*.awswaf.com` (AWS WAF challenge SDK) | 1 | 670 |
| `cdn.arstechnica.net` (first-party CDN) | 7 | 402 |
| `planebasin.com` | 1 | 134 |
| `www.googletagservices.com` | 1 | 131 |
| `ads-static.conde.digital` (publisher's ad code) | 1 | 117 |
| `globalservices.conde.digital` | 2 | 86 |
| inline in the page | 16 | 22 |
| `eval` or `new Function` | 180 | 33 |

Most of the code is third-party (consent, tags, ads, WAF). 22 scripts are
inserted by scripts (GTM, GPT, ad partners, `planebasin`, rum.js), most
async, one classic non-async (`fides.js`), two through `document.write`
(`ars-ajax-handler.php`). The site's own code (Alpine.js attributes `x-data`
and `x-init`: 572 and 537 `matches` calls) is in `cdn.arstechnica.net`.

APIs by area (364 members, 41,427 ops; parse 31,394, load 7,005, after 3,028):

| Area | Members | Ops | Top members (ops) |
|---|---|---|---|
| dom-core | 81 | 32,808 | `Node.parentNode` 7,141, `NamedNodeMap.length` 3,449, `NodeList.length` 2,658, `parentElement` 2,459 |
| events | 14 | 268 | `addEventListener` 204 (87 in parse), `dispatchEvent` 10, `CustomEvent.detail` 9 |
| html-elements | 62 | 1,064 | `HTMLScriptElement.src` 180, `HTMLElement.style` 159, `dataset` 141 |
| cssom-layout | 30 | 3,894 | `getComputedStyle` 3,141 (3,135 in parse), `offsetHeight` 142, `Document.elementFromPoint` 116, `offsetWidth` 97, `CSSStyleDeclaration.setProperty` 44 |
| timers | 8 | 827 | `setTimeout` 711, `queueMicrotask` 70, `clearTimeout` 19, `requestAnimationFrame` 9, `setInterval` 6 |
| network | 10 | 50 | `fetch` 19, `Response.json` 9, `Headers.append` 6, `FormData.append` 2 |
| storage | 13 | 473 | `Document.cookie` 204, `localStorage` 106, `Storage.getItem` 66, `setItem` 53 |
| observers | 7 | 226 | `MutationObserver.observe` 66 (parse), `ResizeObserver.observe` 10, `PerformanceObserver.observe` 9 |
| history-location | 4 | 6 | `window.history`, `window.navigation` |
| canvas | 26 | 85 | 2D context (`fillStyle`, `arc`, `beginPath`, `fill`): a fingerprint or feature probe |
| browser-misc | 64 | 667 | `Navigator.userAgent` 149, `Performance.now` 86, `Crypto.getRandomValues` 50 |

Details:

- Listeners: `click` 77 on 28 targets, `load` 17, `keydown` 14,
  `visibilitychange` 10, `pageshow` 9, Fides custom events (`FidesUIShown`,
  `FidesUpdated`, `FidesInitialized`, `FidesConsentLoaded`), `message` 5,
  `DOMContentLoaded` 5, `mousedown` 4, `focusin` 4, `scroll` 3, `navigatesuccess`.
  Targets: theme items, menu buttons, consent buttons, `document`, `window`.
- Elements created: `div` 21, `script` 17, `fieldset` 7, `a` 5, `input` 5,
  `meta` 5, `style` 4, `canvas` 2, `audio` 1. `getContext`: `2d` 2,
  `experimental-webgl` 1.
- `fetch`: AWS WAF `POST` 8, Fides `GET` 3 and `PATCH` 1, `POST c.arstechnica.com`
  3, ad hosts (amazon-adsystem, doubleclick, googlesyndication) 3,
  `planebasin.com` 1. No XHR from the page; `sendBeacon`: none.
- `getComputedStyle`: 3,141 calls, almost all during parse, from a script
  that walks the DOM. `matchMedia`: `(max-width: 768px)`,
  `(prefers-color-scheme: dark/light)`. `setTimeout` delays are mostly 1-16 ms
  (569), `>1000` 18. Callbacks that ran: timeouts 1 + 66 + 621, animation
  frames 3 + 2 + 4, idle 3, intervals 2.
- `document.write`: 2 calls, 330 characters (script tags for
  `ars-ajax-handler.php`). `Element.innerHTML` set 13 times, 347,582
  characters (343,802 of them in parse: a script puts the page's HTML
  templates in place, probably Alpine/Fides).
- Iframes created by scripts: 1 (no `src`, connected). The page has 1 iframe
  at the end.
- Mutations: parse 4,076 records, 16,540 elements added (many SVG `path`,
  `clippath`, `g`: inline SVG icons added by scripts), 13 removed. Later: 40
  records, 250 elements added.
- Errors in replay: 25 console errors, 2 uncaught `Event` objects (script
  load failures). 17 are `ERR_FAILED` for the 15 requests the fixture lacks
  (ad scripts and `ars-ajax-handler.php`, 7 distinct hosts) and CORS
  failures of `fides.condenastdigital.com` and `c.arstechnica.com`
  (the replay's extra header does not satisfy the preflight). As a result
  the consent button stays disabled ("I ACCEPT" with a spinner) and the
  dialog cannot be closed in replay.
- Chromium: `ScriptDuration` 318 ms (JIT), 331 ms (jitless); `TaskDuration`
  558 / 566 ms; layout 77-79 (about 20 ms); style recalculation 130 (28-33
  ms); 8,269 nodes; JS heap 20 MB.
- JS on vs off: 2,708 against 1,947 elements; 63.1 % of the first viewport
  differs: with JS a dark overlay and the "We Care About Your Privacy" dialog
  cover the page, the header shows only the logo band (tools and sections
  hidden), and the page background becomes dark.

JavaScript features found:

| Feature | Evidence |
|---|---|
| Consent dialog (Fides): covers the page, "I accept", "Your privacy choices", vendor list | Dialog in the screenshot; `Fides*` events; `fides.condenastdigital.com` fetches; `PATCH notices-served` after a click |
| Sections dropdown | Click on `SECTIONS` adds 90 characters of text (menu opens) |
| Theme switch (light, dark, auto) | Click on `THEME` then an item changes 27 % of the pixels; `click @ li.theme-selection-item` |
| Grid settings panel | Click on `GRID SETTINGS` adds 635 characters of text, fetches consent data |
| Sign in | Click on `SIGN IN` requests `/civis/login`, opens a dialog (`a11y-dialog`) |
| View switcher (classic, grid, list, neutron star) | `<form>` submit buttons: works without JavaScript (not a JS feature) |
| Ads, tag manager, WAF telemetry (`awswaf.com/telemetry` POST), analytics (`c.arstechnica.com`) | Requests; no visible effect |
| Scroll effects | 3 `scroll` listeners; not tested beyond the first screens |

## bbc

Scripts (93 compiled, 5,126 KB; HTML: 68 `<script>`: 60 external defer, 5
async, 6 inline, 1 module, 1 nomodule, 2 data blocks):

| Origin | Scripts | KB |
|---|---|---|
| `static.files.bbci.co.uk` (BBC's own, Next.js bundles) | 67 | 2,437 |
| `cdn.privacy-mgmt.com` (Sourcepoint consent) | 6 | 1,089 |
| `cdn.tinypass.com` (Piano) | 1 | 484 |
| `cdn.optimizely.com` | 2 | 415 |
| `mybbc-analytics.files.bbci.co.uk` (BBC) | 1 | 397 |
| `emp.bbci.co.uk` (BBC player) | 1 | 133 |
| `gn-web-assets.api.bbc.com` (BBC) | 1 | 74 |
| `static.chartbeat.com`, `static.bbci.co.uk`, `uk-script.dotmetrics.net` | 4 | 91 |
| inline in the page, `eval` or `new Function` | 10 | 6 |

About 3.1 MB (60 %) is BBC's own code. 22 scripts are inserted by scripts
(Next.js chunks, Sourcepoint, Optimizely, Piano), all async. 5 iframes are
created by scripts (consent message, Optimizely storage, 2 `about:blank`).

APIs by area (272 members, 41,321 ops; parse 1,206, load 29,012, after 11,103):

| Area | Members | Ops | Top members (ops) |
|---|---|---|---|
| dom-core | 62 | 19,892 | `Node.nodeType` 6,312, `nodeName` 2,618, `nextSibling` 2,587, `firstChild` 2,528, `DOMTokenList.length` 957 |
| events | 13 | 2,057 | `addEventListener` 1,079 (1,048 in load), `Event.target` 467, `defaultPrevented` 153 |
| html-elements | 61 | 6,268 | `URLSearchParams.toString` 797, `URL.host` 461, `HTMLElement.onclick` (setter) 460, `URLSearchParams.append` 444 |
| cssom-layout | 21 | 1,834 | `CSSStyleSheet.insertRule` 813, `pageYOffset` 137, `innerWidth` 130, `innerHeight` 126, `Screen.height` 108 |
| timers | 6 | 1,040 | `setTimeout` 764, `clearTimeout` 159, `requestAnimationFrame` 93 |
| network | 21 | 369 | `XMLHttpRequest.open` and `send` 110, `setRequestHeader` 104 |
| storage | 8 | 1,171 | `Document.cookie` 982, `Storage.getItem` 61, `setItem` 28 |
| observers | 7 | 1,223 | `IntersectionObserver.observe` 565, `unobserve` 452, `ResizeObserver.observe` 109 |
| history-location | 2 | 2 | `History.replaceState` 1 |
| workers-other | 8 | 226 | `MessageEvent.data` 107, `MessagePort.postMessage` 52, `MessageChannel` |
| browser-misc | 27 | 2,403 | `Performance.now` 1,123, `PerformanceEntry.name` 865, `Navigator.userAgentData` 105 |

Details:

- Listeners: `click` 503 on 32 targets (React-style delegation plus 460 `onclick`
  property sets on anchors, cards and buttons), `error` 185 and `load` 164
  (images), `keydown` 38, `scroll` 23, `message` 11, and one root element that
  listens to 20 other event types (animation, composition, drag, copy, cut:
  the framework's event system).
- Elements created: `div` 88, `span` 50, `button` 33, `p` 25, `meta` 24,
  `script` 23, `link` 20, `a` 9, `iframe` 4, `style` 4.
- Requests from scripts: `XMLHttpRequest` 110 (103 `POST a1.api.bbc.com`
  analytics events, `GET cdn.privacy-mgmt.com` 3, optimizely 2), `fetch` 1
  (`GET www.bbc.co.uk`). `sendBeacon`: none.
- `matchMedia`: `(max-width: 1024px)`, `(pointer: coarse)`. `setTimeout`
  delays: 101-1000 (563), 17-100 (150), >1000 (38). Callbacks that ran:
  timeouts 20 + 676, animation frames 92 + 1, intervals 2 + 39.
- `setAttribute`: `class` 208, `data-testid` 183, `id` 53, `aria-hidden` 50,
  `data-next-head` 35. `closest('[data-analytics-group=true]')` 97 times.
  `Element.innerHTML` set 6 times (10,513 characters).
  `CSSStyleSheet.insertRule` 813 times (styled-components, in the parse phase:
  732).
- `document.write`: none.
- Mutations: parse 3,000 records, 32,868 elements added (the server HTML is
  built into the DOM in a few batches; this is the parser). Later: 205
  records, 305 added (`div` 57, `script` 12, `link` 9, `iframe` 3), 193
  removed (`img` 71, `button` 24: the carousel and the lazy images).
- Errors in replay: 100 console errors (capped; all `ERR_FAILED` for the 103
  `POST a1.api.bbc.com` analytics requests that the fixture lacks), no
  uncaught exceptions, 3 images and 3 other requests missing. The accept
  click on the consent dialog works (it posts to `cdn.privacy-mgmt.com`,
  which the fixture lacks; the dialog still closes, with one `Failed to fetch`
  rejection).
- Chromium: `ScriptDuration` 490 ms (JIT), 955 ms (jitless); `TaskDuration`
  1,056 / 1,523 ms; layout 100-103 (about 47 ms); style recalculation
  102-104 (40-45 ms); 3,364 nodes; JS heap 23 MB.
- JS on vs off: 2,703 against 2,814 elements; 75.6 % of the first viewport
  differs: with JS a consent modal (Sourcepoint iframe) and a cookie banner
  cover the page, and the header is a client-rendered bar (menu icon with a
  search icon, Register and Sign In buttons) instead of the no-JS
  `details` menu.

JavaScript features found:

| Feature | Evidence |
|---|---|
| Consent modal (Sourcepoint iframe) and the cookie banner ("Yes, I agree") | Screenshot; accepting removes both (75 % of pixels change); iframe inserted by scripts |
| Main menu panel with search field and expandable sections (News, Sport, ...) | Click on "Open menu" opens a left panel (69 % pixel change, new images fetched); `Escape` closes it; `click @ button.NavigationPanel...` |
| Search field in the menu | Input `Search news, topics and more` and a Search button, only enabled with JavaScript |
| Carousels (horizontal card rows with Previous/Next page chevrons) | `click @ SectionTitleChevrons` buttons; `scroll @ GridContainer` listeners (8); images swap (71 `img` removed) |
| "Save" with a sign-in popover | Click shows "Sign in to save for later" with Sign In and Register buttons (`click @ div#popover-wrapper`) |
| Register and Sign In buttons | Click navigates to `session.bbc.com` (script-driven navigation) |
| Language dropdown and footer social buttons | `click @ LanguageDropdown`, `SocialFollowUs` buttons |
| Lazy images with fallbacks (`error`, `load` on 140 images) | 142 `img` added; `IntersectionObserver` (565 observes) |
| Analytics, A/B tests (Optimizely), paywall (Piano), player loader | Requests; no visible effect on the first viewport |

## craigslist-posting

Added on 2026-10-22 as an M8 target, after the candidate survey below. The
posting is a for-sale item with four photos and a map, picked from the San
Francisco results.

- Scripts (9 compiled, 1,234 KB, all first-party): a loader inserts a
  `manifest.js` (its URL has a version that counts up with time) and the
  page bundle (1.2 MB: jQuery 3 with jQuery Migrate, Leaflet, the site's
  code); 3 inline scripts (2 KB). No modules.
- API use (148 members, 1,915 ops; parse 18, load 1,738, after 159):
  `Node.nodeType` 242, `HTMLElement.style` 209, `addEventListener` 130,
  `createElement` 110 (`div` 53, `span` 15, `fieldset` 7, `button` 6,
  `input` 6, `a` 5, `img` 4, `canvas` 2 for feature tests, `script` 2),
  `appendChild` 96, `classList` 57, `innerHTML` set 15 times, about 40
  `querySelector`/`querySelectorAll`, `getComputedStyle` 8, `matchMedia`
  5, `localStorage`, `URL`, one `IntersectionObserver`, one
  `XMLHttpRequest` (the reply flow), `postMessage` to the hidden frame.
  Timers: `setTimeout` 9, `setInterval` 4.
- Listeners: Leaflet's mouse, touch, `dblclick`, `contextmenu` and key
  listeners on the map, the zoom and fullscreen buttons, the toolbar
  buttons, the thumbnails, `load` and `error` on the tile images.
- One hidden same-origin frame (`display: none`): a helper page that keeps
  `localStorage` in step across Craigslist's subdomains through
  `postMessage`. The page's call is inside `try`; with the frame's URL
  removed from the fixture, Chromium renders the same 283 elements with no
  errors (0.17 % of the pixels differ: the relative date and the map).
- No custom elements, shadow DOM, video, audio, canvas drawing, `dialog`,
  `popover`, `template`, workers or `document.write`.
- Errors in replay: none. Requests: 19, one missing.
- Chromium: `ScriptDuration` 111 ms (JIT and jitless); `TaskDuration` 146 /
  149 ms; 11 layouts, 26 style recalculations; 283 elements.
- JS on vs off: 283 against 208 elements; 31.0 % of the first viewport
  differs. With JavaScript off the page shows only the title bar, one
  sentence and the notice "We've detected that JavaScript is not enabled".
  With JavaScript on: the navigation bar, the toolbar (reply, favorite,
  hide, flag, share, the posting age, print), the heading with the price,
  the main photo, four thumbnails, the Leaflet map with a radius circle
  (SVG) and zoom and fullscreen buttons, the attributes, the text, the post
  id row.
- Tried in Chromium on the replay: a thumbnail changes the main photo (the
  large photos of thumbnails 2 to 4 were added to the local fixture
  afterwards), zoom in works, reply shows a spinner (its request is live
  only).

## Candidates that are not M8 targets

On 2026-10-22 the owner asked for pages whose scripts need no HTML
features that swb lacks (no frames, no media, preferably no custom
elements). The same tools surveyed four candidates; the owner chose the
Craigslist posting (above).

| | github-swb | craigslist-search | stackoverflow-question |
|---|---|---|---|
| URL | github.com/senko/swb | sfbay.craigslist.org/search/sss | stackoverflow.com/questions/11227809 |
| Capture | worked | worked; replay lacks the result data and photos (see below) | blocked: Cloudflare challenge page (HTTP 403) for headless Chromium and for swb |
| JS off in swb | pixels 0.851; the geometry score is not valid (the Chromium reference is scrolled by 394 px); the header menu buttons lose their side padding (backlog) | 1.000 / 0.993, but the page without JavaScript is a different, unstyled list | none |
| Scripts | 128 (5,103 KB), 9 module scripts that load the rest with `import()` | 9 (904 KB), first-party | none |
| Script time JIT / jitless | 227 / 237 ms | 338 / 424 ms (task 1,458 / 1,578 ms) | none |
| Members / ops | 317 / 147,000 | 151 / 205,000 | none |
| Special features | 75 `customElements.define`, 24 shadow roots, 4 `<template>`, 10 `popover` elements, 8 `fetch` calls that the first view needs, five observer kinds | no custom elements or shadow DOM; about 140,000 elements created and removed, 1,119 layouts, 5,700 `getBoundingClientRect` calls, an `IntersectionObserver` that drives the grid, credentialed XHR | none |

- GitHub has every feature that the owner wanted to avoid: custom
  elements, shadow DOM, `popover` and module scripts with imports. A
  later milestone for web components.
- The Craigslist search page needs no special features, but its result
  grid needs incremental style and layout. Its replay also needs two tool
  changes: credentialed CORS (replay adds `Access-Control-Allow-Origin: *`,
  which a credentialed request rejects) and a rule for the time-based
  `manifest.js` URL. Both are in M8 feature 1.
- Stack Overflow: the live page also challenges swb, and passing the
  challenge needs its scripts and frames. Dropped.

The JS-off captures of github-swb and craigslist-search are in the
git-ignored `out/candidates/`; their JS fixtures are in `fixtures/js/`.

## Cross-target table

Distinct members used and operations (calls, reads, writes, `new`), from
`survey.json`. Members count the keys `Interface.member` at the defining
interface.

| Area | senko-net | hacker-news | wikipedia | ars-technica | bbc |
|---|---|---|---|---|---|
| dom-core | 0 / 0 | 2 / 2 | 70 / 10,253 | 81 / 32,808 | 62 / 19,892 |
| events | 0 / 0 | 1 / 1 | 9 / 446 | 14 / 268 | 13 / 2,057 |
| html-elements | 0 / 0 | 0 / 0 | 36 / 618 | 62 / 1,064 | 61 / 6,268 |
| cssom-layout | 0 / 0 | 0 / 0 | 11 / 42 | 30 / 3,894 | 21 / 1,834 |
| timers | 0 / 0 | 0 / 0 | 4 / 97 | 8 / 827 | 6 / 1,040 |
| network (fetch, XHR, beacons) | 0 / 0 | 0 / 0 | 1 / 1 | 10 / 50 | 21 / 369 |
| storage and cookies | 0 / 0 | 0 / 0 | 8 / 104 | 13 / 473 | 8 / 1,171 |
| observers | 0 / 0 | 0 / 0 | 4 / 159 | 7 / 226 | 7 / 1,223 |
| history-location | 0 / 0 | 0 / 0 | 1 / 2 | 4 / 6 | 2 / 2 |
| canvas | 0 / 0 | 0 / 0 | 0 / 0 | 26 / 85 | 0 / 0 |
| workers, WebSocket, messaging | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 8 / 226 |
| browser-misc (navigator, performance, crypto, console) | 0 / 0 | 0 / 0 | 17 / 41 | 64 / 667 | 27 / 2,403 |
| window-misc | 0 / 0 | 0 / 0 | 5 / 39 | 12 / 618 | 11 / 698 |
| other (observer entries, `URL`, records) | 0 / 0 | 0 / 0 | 10 / 539 | 33 / 441 | 25 / 4,138 |
| Total | 0 / 0 | 3 / 3 | 176 / 12,341 | 364 / 41,427 | 272 / 41,321 |

The union of the members across targets is larger than any single page. For
the order of the bindings: the DOM core (nodes, elements, attributes,
`classList`, `dataset`, selectors with `:scope` and attribute selectors,
`innerHTML`), events (listeners, `dispatchEvent`, `CustomEvent`, the `on*`
setters), timers and microtasks, `URL` and `URLSearchParams`,
`localStorage` and `document.cookie`, `matchMedia`, and layout reads
(`getBoundingClientRect`, `offsetWidth`, `getComputedStyle`) are used by all
three script-heavy pages. Next come `fetch` and `XMLHttpRequest`, the
observers (`IntersectionObserver`, `MutationObserver`, `ResizeObserver`),
`document.write` (Ars Technica), dynamically inserted scripts (all three),
iframes created by scripts (Ars Technica, BBC), `MessageChannel` and
`postMessage` (BBC), canvas 2D (a probe on Ars Technica).

## Open questions

- The survey cannot see `window.location`, `document` and `top` (the
  non-configurable members), so `location.href`, `location.search` and
  similar reads are missing from the counts. A CDP-based measurement or a
  source check of the target scripts would settle it.
- Cross-origin iframes (consent dialogs, ads, Optimizely storage) run their
  own scripts in other frames. The survey does not trace them. Decided:
  M8 renders no frames and runs no scripts in them (ADR 0028 section 14);
  BBC, whose consent dialog is such a frame, is a later target.
- The Ars Technica consent dialog cannot be accepted in replay because the
  CORS preflights of `fides.condenastdigital.com` fail. Whether it can be
  accepted on the live site in Chromium was not tried (the survey runs from
  the fixture). The dialog covers the first viewport on both Ars Technica and
  BBC, so how swb treats these dialogs decides how the pages look.
- Replay misses requests with new URLs (BBC analytics `POST`s, ad calls), so
  the ad slots stay empty. The live pages run more third-party code than
  these numbers show.
- JIT and jitless `ScriptDuration` are close for three pages; only BBC shows a
  2x difference. Whether the script time of swb's interpreter follows the
  jitless number needs a run of the target scripts in `swb-js`.
- The listener tables of React and Next.js pages (BBC) show one element that
  listens to every event type; the number of real controls is smaller than the
  listener count. The listed features come from trying the controls in
  Chromium, not from the counts alone.
- The first viewport of Hacker News with a logged-in user (votes, hide,
  comment collapse) was not measured; it needs a session.
