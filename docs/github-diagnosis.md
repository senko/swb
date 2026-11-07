# M10 step 1: diagnosis of github.com/senko/swb

The report of the diagnosis that planned M10 (docs/roadmap.md, "M10"), kept
as it was written on 2026-11-07. The owner's decisions after it: the list of
section 6 without what replay cannot test, no new capture, CSS anchor
positioning in the backlog. The files named under `m10/diag/` (logs, runs,
screenshots, helper scripts) were working files of the session and are not
kept; the probe files and the fixture are in the repository.

Base 433d211. Chromium 148.0.7778.96 (Playwright 1.60). Capture of 2026-10-22 (`captured` 19:17:27Z; `--now 1792696647000`, computed from `fixture.json`). All timings below were taken while another agent built swb and ran Chromium on the same machine: **mark them as measured under load; repeat the load times, the jitless task time and the `perf` run on an idle machine**. Scores and box dumps do not depend on load.

Working files: `m10/diag/` of the session's scratchpad (not kept).

## Summary

- Scores, JavaScript on (`compare --js github-swb`): geometry 0.1425, size 0.7980, relative 0.7831, pixels 0.9167, missing 148, extra 26 (tag sequences differ). swb runs no page script: all 8 module scripts that have imports are refused (`not supported yet (module syntax)`), the other module has nothing to do. The page is the server-rendered HTML.
- Scores, JavaScript off (new fixture `fixtures/pages/github-swb`, 3.0 MB): geometry 0.1576, size 0.9134, relative 0.8909, pixels 0.9344, missing 0, extra 0. With the `@layer` rules emulated by hand (experiment, not committed): geometry 0.6623, pixels 0.9872.
- JS-off causes: (1) `@layer` is flattened (layer order ignored): wrong link colors, font sizes and spacing; 6,341 of 16,282 style rules are in layers. (2) The header navigation (402 px against 627 px) is not `@layer`, custom properties or `@media (width>=1012px)`: those three work. It is the **min-content size of a non-wrapping `display: flex` container**: swb takes the widest item, Chromium sums the items and their margins (probe cases in `tools/probes/github-nav.json` reproduce it). (3) The Chromium reference was scrolled by 394 px because of `html { scroll-behavior: smooth }`: the tool called `scrollTo(0, 0)`, which animates. Fixed in the tools.
- Load time of swb (release, replay, JS on, under load): 0.159 to 0.173 s to `load` and to a settled page (S = 1500), median 0.170 s. This is not comparable with a working page: nothing runs. Budget (M9 method): 3 x 0.485 s (Chromium jitless task, `js-survey --runs 3`, under load; survey of 2026-10-22: 0.496 s) + 0.103 s (JS-off pipeline, `perf`, under load) = **1.56 s** (1.59 s with the survey's task time).
- Top gaps for JS on: module graph (307 static imports in 20 of 133 files, import map, 111 `modulepreload`), custom elements (75 definitions, 8 tags on the page), shadow DOM (24 open roots: 23 `relative-time`, 1 `tool-tip`), React 19 hydration, `popover` (10 elements, `:popover-open`), `@container` (21 blocks), missing APIs (`Range`, `getSelection`, `indexedDB`, `createTreeWalker`, `ElementInternals`).
- Proposed features (section 6): 1 `@layer`, 2 flex min-content, 3 module graph, 4 custom elements, 5 shadow DOM and flat tree, 6 popover and top layer, 7 small CSS features, 8 bring-up of the first view, 9 interactions. The fixture lacks 44 lazy-loaded URLs that the search dialog, the file finder and the keyboard dialog need; the owner decides about a new capture.

## 1. Baselines

**Chromium (replay, JS on).** `reference --js github-swb`: 1 console error, the 404 of `/_global-navigation/payloads.json` (recorded as 404 in the capture; same in the survey). No page exception, no missing URL. `replay --console-out`: the same single line. The network never goes idle: the 15 s cap of `load_settled_js` is reached on every run (warning `network did not become idle`). The first view: full header with menus, repository header, tabs, branch selector, "Go to file", Code button, latest-commit line ("17646c1 - 4 days ago", the page clock is the capture time), file list with commit messages and relative times, About, Releases (none), Contributors. Screenshot: `out/js-reference/github-swb/screenshot.png` (in the worktree), replay screenshot `s0/000-load.png`.

**swb (JS on).** Screenshot `out/compare/js-github-swb/screenshot.png`: the JS-off look. Console, in order (8 warnings, no error):

```
WARN swb_web::scripts::modules: the module script .../environment-d74cc2a32ff9d79e.js is not run: SyntaxError: not supported yet (module syntax) (swb runs modules that need no other module)
... the same for github-elements-986e98e1e895cc8d.js, element-registry-17737dd1682949de.js, behaviors-cff5b4326dc37549.js, codespaces-31a4b7aa49a008d9.js, repositories-a53d97762821d3f2.js, code-menu-65a0590c2a0eec56.js, code-view-d27b74a189db0f96.js
```

(`high-contrast-cookie-141c17800ca6ba23.js` runs.) What differs from Chromium in the first view: the hydrated parts (file list commit messages and times: skeleton bars stay; the commit line, branch count "1 Branch", "0 Tags", contributors, release text), the header geometry, link colors, spacing, the `react-partial` elements (inline in swb, block in Chromium after the page script adds `loaded`).

**Load time** (release binary of 433d211, `--headless --js --replay fixtures/js/github-swb --test-fonts --size 1280x800 --now 1792696647000 --virtual-time --settle-ms S`, 7 runs, wall time with process start, under load): S = 0: 0.159, 0.162, 0.162, 0.166, 0.170, 0.171, 0.173 s; S = 1500: 0.162 to 0.171 s (median 0.170). Nothing runs, so the time is the JS-off pipeline plus parsing 5 MB of module files that are refused.

**Budget.** `js-survey github-swb --runs 3` (jitless, under load): `TaskDuration` 485 ms, script 241 ms, 9 layouts (28 ms each), 32 style recalculations (93 ms each); JIT: task 475 ms. `perf github-swb --runs 5` (JS off, under load): total 103.3 ms (parse 2.8, stylesheets 61.4, style 23.5, layout 4.9, display list 0.5, raster 10.1). Budget = 3 x 0.485 + 0.103 = 1.558 s. The survey of 2026-10-22 had task 496 ms (budget 1.59 s). The style sheets are the largest JS-off cost: 21 files, 16,282 rules, 22,001 `var()` uses.

## 2. JavaScript off

**The page.** The JS-off HTML is the same server render as the JS-on page, without hydration: skeleton bars in the file list and the sidebar, no commit line, "Branches" and "Tags" without counts, no Releases text. JS-on and JS-off differ in 1.90 % of the first-viewport pixels in Chromium (survey rerun with the scroll fix; it was 16.23 % with the scrolled capture).

**Scroll of 394 px (tool bug, fixed).** GitHub sets `html { scroll-behavior: smooth }` (and `html:focus-within`). `browser.scroll_through` and `load_settled` ended with `window.scrollTo(0, 0)`, which animates; the box dump and screenshot came from the middle of the animation. Fix: `browser.scroll_to_top` and `behavior: 'instant'` in `scroll_through`, `reference.py` (both loaders), `live.py` and `js_survey.py`. Test: `tools/tests/test_js_survey_chromium.py::test_scroll_through_ends_at_the_top_on_a_page_with_smooth_scrolling` (I did not run it against the old code to see it fail). New reference starts at y = 0.

**Header navigation, 402 px against 627 px (diagnosis).** None of the three suspects is wrong. Evidence (swb `--eval` and Chromium `replay --record` of the same expression, `nav-expr*.js`): the button gets `padding: 8px` (the `@media (width>=1012px)` rule applies, `matchMedia` is true), `--base-size-8` resolves, layers do not matter here. The `nav` is a flex item of `.menuWrapper` (flex row, 1168 px) next to a CTA container; the sum of the max-content sizes is larger than 1168 px, so both shrink to their min-content size (`min-width: auto`). Chromium: min-content of a `li` with a `display: flex` button (text 61.5 px, a 16 px SVG with `margin-left: 4px`, padding 8 px) is 97.5 px. swb: 77.5 px, text plus padding; the SVG item and its margin are missing. A non-wrapping row flex container has as min-content size the sum of its items' contributions (spec: CSS Flexbox 9.9.1); swb takes the widest. Reproduction with Chromium numbers: `tools/probes/github-nav.json`, cases `min-content-of-a-row-flex-container` (swb 75.58, Chromium 95.58 for text + 16 px item with 4 px margin + padding 8 px) and `github-header-shrink` (nav 505.06 against 598). The other cases of that file that I ran (no shrink pressure) match; the file has 12 cases, and I ran each of them once. Elements affected in the JS-off dump: 390 boxes (the nav, the dropdown menus that are hidden but have boxes, shifted by 20 px per button). `just probe --with-swb tools/probes/github-nav.json` shows the rows. Size S to M, crate `layout` (flex intrinsic sizes).

**Link colors, font sizes, spacing (`@layer`).** `swb_css` puts the rules of `@layer` blocks in the parent list and ignores layer order (`crates/css/src/parser.rs` "Approximation"). GitHub uses three layers (`primer-css-base`, `primer-react`, `primer-brand`, declared by `@layer` statements): `[data-variant="normal"] .prc-Breadcrumbs-Item-jcraJ { color: ... }` in `primer-react` (specificity 0,2,0) beats the unlayered `.contextCrumb__Q8nET8 { color: var(--fgColor-default) }` (0,1,0) in swb; in Chromium the unlayered rule wins. Computed color of the breadcrumb link: swb `rgb(9, 105, 218)`, Chromium `rgb(31, 35, 40)`. Experiment: a copy of the fixture where every rule inside an `@layer` block got `:where(selector)` selectors (specificity 0; script `m10/diag/variant.py`, 6,341 rules): geometry 0.1576 to 0.6623, pixels 0.9344 to 0.9872. After that, the only differences left are the 390 boxes of the min-content bug. The 12 px shift of the tab row and the 32 x 32 against 24 x 20 box of the `rightWithResponsiveCreateButton` stack is part of the `@layer` effect (an `@container` emulation by `@media` changed no number). Probe cases for layer order, unlayered priority, `!important` reversal, nested and anonymous layers, `revert-layer`: all 6 differ in swb (`tools/probes/github-css.json`).

**Fixture.** `fixtures/pages/github-swb`: made from the survey's capture (`out/candidates/github-swb`, without its scrolled reference); the two raster `data:` URLs were already replaced by SVG by hand there. `substitute github-swb` replaces 0 images and 0 fonts, the check passes (fonts and the avatar are placeholders or free). `reference github-swb`: 1732 elements, starts at y = 0, 0 URLs missing. Added to `fixtures/pages/README.md`. `fixtures/scores.json` is not changed (outside the staged paths; run `just update-scores` after the merge: geometry 0.1576, size 0.9134, relative 0.8909, pixels 0.9344).

**`substitute` and raster `data:` URLs in CSS (tool fix).** `substitute_data_urls` replaces each base64 raster `data:` URL in a text body by an SVG `data:` URL of the same pixel size with one color (hash of the URL); unreadable and percent-encoded payloads stay and the check reports them. Tests in `tools/tests/test_substitute.py`.

**CSS features of GitHub's style sheets** (21 sheets, 16,282 style rules; counts from `m10/diag/cssrules.py` and `cssfeat.py`; probe results from `tools/probes/github-css.json`, swb against Chromium):

| Feature | Use | swb | Changes the first view? |
|---|---|---|---|
| `@layer` (3 layers, 5 blocks, 1 statement) | 6,341 rules (39 %) | order ignored, rules flattened; 6 of 6 probe cases differ | yes, JS off and on (biggest effect) |
| `@container` | 21 blocks, 41 rules; `container-type` 33 | rules dropped; size query, `cqw` and named container cases differ | not in the JS-off first view (emulated: no change); the hydrated page: not measurable yet |
| `:has()` | 104 | works (probe equal) | no gap |
| `:is`, `:where`, `:not`, `@supports` (incl. `selector()`) | 181, 953, 807, 43 | work | no gap |
| CSS nesting | 0 nested style rules | not supported (3 of 4 probe cases differ) | not used |
| `color-mix()` | 35 | `srgb` with percentages works; `currentcolor` with `transparent` wrong (probe: rgb(128,0,128) for color(srgb 0 0 0 / 0.2)); serialization differs | small |
| `light-dark()` | 0 | works | no |
| `@property` | 2 | ignored (probe: width 1264 px, expected 5 px) | no (which elements: not checked) |
| anchor positioning (`anchor()`, `anchor-name`, `position-anchor`, `@position-try` 6) | 30 uses, in `primer-react-css` | not supported (box at wrong place); the page asks `CSS.supports('position-anchor', '--name')` | overlays only (Code, branch, menus) |
| `:popover-open`, `[popover]` | 15, 16 | UA `display: none` works; no `showPopover`, no open state (probe: no box) | the open popovers |
| `::backdrop` | 8 | with `showModal`/`showPopover`: no (probe: no box) | open dialogs |
| `field-sizing` | 3 | ignored (probe 207 px, expected 76 px) | the "Go to file" input? (not checked) |
| `content-visibility` | 14 | box sizes: swb ignores `auto` skipping; the computed value is empty | no (file list) |
| `@media (width>=...)` range syntax | 784 | works | no gap |
| `@media` `prefers-color-scheme`, `hover`, `pointer`, `forced-colors`, `prefers-reduced-motion` | 89, 38, 30, 55, 83 | not checked one by one | no change seen |
| `@starting-style`, `@scope`, `light-dark`, `oklch`, `view-transition`, `@page`, `accent-color` | 0 | n/a | no |
| `interpolate-size`/`calc-size` | 2 | not checked | no |
| `@keyframes` | 72 | exists | no (animations off in the tools) |

## 3. Scripts and the platform

Measured in Chromium with `replay --init-script count-init.js` (wrappers and counters, `m10/diag/count.json`) and by reading the fixture's files (`modules.py`; regex counts, so approximate). swb state: `--eval` in the JS fixture.

**Module graph.** 16 script elements: 9 `type=module` (1 without `defer`: `high-contrast-cookie`; 8 `defer`), 1 `importmap`, 7 JSON data blocks. 133 JavaScript files, 5.37 MB. 111 `modulepreload` links. About 307 static `import` clauses in 20 files (`code-view` 64, `behaviors` 32, `react-dom-client` 3, `react-reconciler` 2); 378 `export` clauses; `import()` seen by regex in 2 files (the survey's 128 compiled scripts and the 9 modules that "load the rest with `import()`" are the lazy chunks that React and the app load; `react-core` has `import.meta`); 4 `import.meta`; 46 lines that start with `await` (candidates for top-level `await`, not verified). Bare specifiers come from the import map. Cycles: not measured. swb: refuses 8 modules, runs the 1 without imports.

**Custom elements.** 75 `customElements.define` calls, all `extends HTMLElement` (3 of the base names minified; no `extends` option, so no customized built-ins), no `formAssociated`, no `attachInternals`, no `whenDefined` call; 8 `upgrade` calls (`customElements.upgrade`). Lifecycle callbacks: `connectedCallback` on 73, `disconnectedCallback` on about 55, `attributeChangedCallback` on about 50 with `observedAttributes` (`relative-time` 21 attributes, `sudo-credential-options` 9). `CustomElementRegistry.get` 26,000 calls and `window.customElements` 26,112 reads (a hot loop in `behaviors`: every mutation checks the tag). Tags in the DOM after load: `react-partial` 4, `relative-time` 23, `clipboard-copy` 2, `cookie-consent-link` 2, `tool-tip` 1, `turbo-frame` 1, `react-app` 1, `ghcc-consent` 1 (8 defined). swb: `customElements`, `CustomElementRegistry`, `ElementInternals` do not exist; `HTMLElement` exists.

**Shadow DOM.** `attachShadow` 26 calls, all `{mode: "open"}`: 23 hosts `relative-time` + 1 `tool-tip` live at load (24 roots). No declarative shadow root (`template[shadowrootmode]` 0 in the HTML), no `delegatesFocus`. `adoptedStyleSheets` is set once on the document; `CSSStyleSheet.replaceSync` once. `:host`, `::slotted`, `::part` appear in no style sheet of the fixture (they are in the components' shadow styles, `relative-time` has its own). `getRootNode` 18, `closest` 155 calls. swb: `ShadowRoot` and `HTMLSlotElement` exist (from M9 feature 1); `Element.attachShadow` and `document.adoptedStyleSheets` do not.

**`<template>`.** 4 in the DOM. `cloneNode` 13 calls, `importNode` 0.

**`popover`.** 10 elements with `popover`, 0 `popovertarget`, 0 `<dialog>`. `showPopover`/`togglePopover` count: not measured by the wrapper (the wrapper missed the prototype); the latest-commit details button opens a popover in Chromium (`:popover-open` 1 after the click). The overlays of the repository header (branch selector, Code) are `role=dialog` `div`s positioned by `anchored-position`/`AnchoredOverlay` (JavaScript or CSS anchor positioning), not `popover` or `<dialog>`. swb: `HTMLDialogElement` exists; `showPopover`, `hidePopover`, `togglePopover`, `ToggleEvent`, `CommandEvent` do not.

**`fetch`.** 8 calls, all `GET`, same origin: `/latest-commit`, `/recently-touched-branches`, `/branch-and-tag-count`, `/tree-commit-info`, `/refs?type=branch`, `/overview-files/main`, `/_sidebar`, `/_global-navigation/payloads.json` (404 in the capture). They return JSON that React renders (files, commit line, tags). `include-fragment` is defined; 0 `include-fragment` elements in the first view. XHR: none. `history.replaceState` 3 calls at load, `pushState` 0 (Turbo/soft navigation is installed: 15 `popstate` and 11 `soft-nav:start` listeners).

**React.** `react-partial` (4) and `react-app` (1) custom elements with embedded JSON data blocks; React 19 (`react-dom-client`, `react-reconciler`, scheduler through `MessageChannel`, which swb has since M9), Primer React components (`prc-*` classes). The header (marketing header) is a `react-partial`; the repository header, the file list (`overview-files`) and the sidebar are React islands.

**Observers and others.** Observers: `MutationObserver` (from the page), `IntersectionObserver`, `ResizeObserver`, `PerformanceObserver` (swb has all four). `matchMedia`: about 150 calls (breakpoints 768, 1012, 1400 px and `prefers-color-scheme`), `CSS.supports` 1 (`position-anchor`). `requestIdleCallback` 10 callbacks (swb has it). `Intl.RelativeTimeFormat.format` 23 calls (for `relative-time`; swb has `Intl`). `navigator.clipboard` object exists in swb; `clipboard-copy` writes through it. Not in swb: `Range`, `Selection`/`getSelection` (3 `selectionchange` listeners), `indexedDB` (IDBRequest handlers in the survey), `document.createTreeWalker`, `BroadcastChannel`, `XMLSerializer`, `Animation`/`element.animate`, `ElementInternals`, `crypto.subtle`, `WebSocket`, `Worker`, `ReadableStream`. Listeners: 168 `click` on 61 targets, `keydown` on `document` 17, `touchstart` 12, `turbo:*` 36, `soft-nav:*` 40, `popstate` 15. Timers: 95 `setTimeout` over 1 s, 1 `setInterval` over 1 s (this keeps the network from going idle; not verified which request).

**Frames.** No iframe in the first view.

## 4. What a user can do in Chromium's replay, and in swb

Steps: `m10/diag/steps-a.json` (25 groups; clicks by coordinates for the header, by id for the branch selector and the Code button), record `m10/diag/rec.js`, results `chromium-a.json`, screenshots `shotsA/`, navigations `chromium-star|issues|crates.json`. swb: `swb --steps` with the same steps; swb has no page script, so the only things that work are native: links, focus order, scroll. Chromium replay needs the fixture; the fixture lacks 44 URLs that the steps requested (lazy chunks `d-89b8dd...js`, `5km-...`, `9r-...`, `chunk-bgp-...`, `ib-...`, `chunk-7h-...`, `9j-...`, `xx-...`, `chunk-xij-...`, `chunk-9id-....js`, `9id....module.css`, `github-copilot-app-light-....svg`, `/senko/swb/tree-list/<sha>?include_directories=true`, `/assets-cdn/worker/chunk-5-....js`, each with `?&r=1..3` retries of the loader).

| Interaction | Chromium replay | swb now |
|---|---|---|
| Header menu Platform (also Solutions, Resources, Open Source, Enterprise) | opens a mega menu (`aria-expanded`), `Escape` closes it; a click outside does not close Solutions in the replay | nothing opens |
| "Pricing", "Sign in", "Sign up", logo | links; Sign in and Sign up go to `/login` and `/signup` (not in the fixture: the request fails) | links work, the navigation fails (not in fixture) |
| Header search button | `aria-expanded=true`, the dialog needs lazy chunks that the fixture lacks: no dialog in the replay | nothing |
| Repository tabs (Code, Issues, ...) | `a` links with a click handler (soft navigation by Turbo/React); Issues: request of `/senko/swb/issues` fails (not in fixture) | plain link, navigation fails |
| Watch, Fork, Star | links to `/login?return_to=...`: the navigation happens, the page is not in the fixture | same: navigates to the missing login page (works as in Chromium) |
| Branch selector | opens an overlay "Select a branch" with a filter input, tabs Branches and Tags; `Escape` closes and returns the focus to the button; the list content needs `/refs?type=branch` (in the fixture: shows `main`) | nothing |
| "Go to file" | focus; typing opens an overlay (empty: needs `tree-list`, not in the fixture); `t` focuses the field (shortcut works) | focus only; no overlay |
| Code button | popover "Clone" with HTTPS and GitHub CLI tabs, the URL, copy button, "Open in GitHub Copilot app" (image missing), "Download ZIP"; `Escape` closes | nothing |
| Copy button of the clone URL | present; the copy works only with clipboard permission (not tested) | n/a |
| Latest commit line: "..." button | opens a popover (`:popover-open` = 1) with the commit message | nothing |
| Status check icon | button; no popup tested | nothing |
| File list links, commit message links | links with a soft-navigation handler; crates folder: requests `/tree/main/crates`, `/latest-commit/main/crates`, `/tree-commit-info/main/crates` (not in the fixture) | links; navigation fails |
| README (headings, anchors, outline menu, code-block copy buttons) | not tested: the README is below the first screen and I did not scroll to it or measure it | not tested |
| About sidebar links (Readme, MIT license, Activity), Releases, Contributors | links; `#readme-ov-file` is an anchor in the page | anchors work if the target exists |
| Relative times | `relative-time` renders "4 days ago", "2 weeks ago" (shadow root text); stable at the page clock | the text is the fallback date text of the HTML (no custom element) |
| Keyboard `?`, `/` | no effect in the replay (the shortcut dialog and the search need the missing chunks) | no effect |
| Keyboard `t` | focuses "Go to file" | no effect |
| Tab through the page | focus order: skip link, header, ..., Code button, "commits by senko", ... ; `:focus-visible` rings | native order (a different DOM, no hydration): after 3 Tab: "Code" tab link |
| Hover states | tested only for Sign in (no visible change checked); hover cards are not in the fixture | CSS `:hover` works |
| Wheel scroll | scrolls 800 px | scrolls |

Not testable in replay: login and signup pages, any soft-navigated page, search results, file finder results, the keyboard-shortcut dialog, hover cards of the avatar and commit authors, clipboard copy (permission), anything that needs `POST`, the "Report repository" page, the README contents below the fold (not looked at: the fixture has `overview-files/main` only).

## 5. Gap list

Sizes: S (one session, a few hundred lines), M (one feature), L (one hard feature with an ADR), XL (several features).

**A. What the first view needs (JS on, like Chromium)**

| # | Gap | Cause and evidence | Size | Crates |
|---|---|---|---|---|
| A1 | `@layer` order | flattened; JS-off geometry 0.16 to 0.66 with layers emulated; 6 of 6 probe cases | M | css, style (cascade) |
| A2 | Min-content of a row flex container | swb takes the widest item; 390 boxes; probes `github-nav.json` | S | layout |
| A3 | Module graph | 8 of 9 module scripts refused; 133 files, 307 static imports, import map, 111 modulepreload, `import()`, `import.meta` | L | js-syntax, js, web, engine, net |
| A4 | Custom elements | no `customElements`; 75 defines; sync construction from the parser, upgrade, reactions, attribute observation | L | dom, web, js (bindings), engine |
| A5 | Shadow DOM | no `attachShadow`; 24 open roots (23 `relative-time`, 1 `tool-tip`); flat tree in style, layout, hit test, focus, events (retargeting, `composedPath`), `:host` | XL | dom, style, layout, paint, web, engine |
| A6 | React 19 hydration of the header, the repository header, the file list and the sidebar | unknown until A3 to A5 exist; needs the missing APIs below; `MessageChannel` exists | L (open-ended) | web, js |
| A7 | Missing APIs found by the page | `getSelection`/`Range` (3 `selectionchange` listeners), `indexedDB`, `createTreeWalker`, `ElementInternals` (not used on this page), `BroadcastChannel`, `Element.animate`, `adoptedStyleSheets`, `XMLSerializer`; the list will change when the scripts run | M | web |
| A8 | `relative-time` | shadow root + `Intl.RelativeTimeFormat` + attribute observation; 23 elements; text must equal Chromium's ("4 days ago", with the page clock) | in A4/A5 | web |
| A9 | `@container` | 21 blocks; not measurable in the first view until the page hydrates; probes differ in 4 of 4 cases | M | css, style, layout |
| A10 | `@property`, `field-sizing`, `color-mix` with `currentcolor`/transparent, `content-visibility` computed value | small differences; none seen in the first view | S each | css, style |
| A11 | CSS anchor positioning (`anchor()`, `position-anchor`, `@position-try`) | 30 uses; the page feature-detects with `CSS.supports('position-anchor', ...)`; if swb answers false, the page falls back (to be verified) | L if needed | style, layout |

**B. What the interactions need**

| # | Gap | Evidence | Size |
|---|---|---|---|
| B1 | `popover` (`showPopover`, `hidePopover`, `togglePopover`, `popovertarget` invoker, `beforetoggle`/`toggle` events, light dismiss, `:popover-open`, top layer, `::backdrop`) | 10 elements; the commit-details popover; 15 `:popover-open` selectors | L (shared with `showModal`) |
| B2 | `<dialog>` `showModal()`, `::backdrop`, top layer | no `<dialog>` in the first view; probe: no box | in B1 |
| B3 | Focus management: `focus()` with `focusVisible`, focus trap, `inert`, `Escape` handling, `delegatesFocus` | branch selector returns focus to the button; `t` shortcut | M |
| B4 | Keyboard shortcuts (`keydown` on `document`, `?`, `/`, `t`) | depend on A3 to A6; the dialogs need the missing chunks | with A6 |
| B5 | Soft navigation (Turbo, `pushState`, `popstate`) | Turbo events 36; pages not in the fixture, so replay cannot test it | M, untestable now |
| B6 | Clipboard (`clipboard-copy`, `navigator.clipboard.writeText`) | exists in swb as an object | S to check |
| B7 | Lazy chunks missing in the fixture | 44 URLs; the owner decides about a new capture | tool |

**C. What no list item needs (found anyway)**

- The Chromium replay never reaches network idle (15 s cap); every `reference --js` and `replay` of this page takes 17 s or more.
- `getComputedStyle` of swb returns an empty `content-visibility`; `font-family` serialization differs (`system-ui, system-ui` for `-apple-system, BlinkMacSystemFont`).
- `swb --eval` and `getComputedStyle` calls work for diagnosis; `swbtools probe` `style` queries print Chromium values only (no swb value): use `boxes` or `js` queries for swb comparisons.
- Page script cost: Chromium's jitless script time is 241 ms for 128 scripts (5.1 MB), 120,000 operations after the load (`Element.matches` 29,000, `customElements.get` 26,000, `querySelector` 26,000); swb's interpreter is about 2 times slower than `node --jitless`, and parsing and compiling 5.4 MB of modules is not in that figure. The budget (1.56 s) may be tight: measure parse and compile of the module files first (`just jsbench fixtures/js/github-swb/files`).

**Module loading in the engine, in concepts (ECMA-262 16.2).** Today: the parser handles module code; `Runtime::eval_module_at` compiles a module that has no `import`, `export`, `import()`, `import.meta` or top-level `await` as a function and runs it (ADR 0028 section 6 item 7, M9 feature 7); `web/src/scripts.rs` has the module map by URL, the lists (`defer` order, `async`), and inline modules. Needed for GitHub: (1) **Module records**: a Source Text Module Record per URL with its requested modules (from `import`/`export ... from` clauses), import and export entries, a module environment, a status (new, unlinked, linking, linked, evaluating, evaluated) and cached errors. (2) **Fetching the graph**: the host loads every requested module (module script fetch: CORS mode `cors`, credentials `same-origin`, the `crossorigin="anonymous"` attribute, MIME check, the import map to resolve bare specifiers and scopes, `modulepreload` as an early fetch into the module map, a failed fetch as an error entry). (3) **Link()**: depth-first, resolves every import binding to an export (indirect `export { x } from`, `export *`, namespace objects, ambiguity and missing exports as `SyntaxError`), creates the environments, instantiates functions; live bindings: an import is an alias to the exporter's cell, not a copy. (4) **Evaluate()**: depth-first post-order through the dependencies, cycles handled by the `[[DFSIndex]]`/`[[DFSAncestorIndex]]` rules (a module in a cycle sees uninitialized `let`/`class` bindings: TDZ errors), each module evaluated once; with top-level `await` the evaluation is asynchronous (`[[HasTLA]]`, `[[PendingAsyncDependencies]]`, `[[AsyncEvaluation]]`, a capability that resolves after the dependencies) and the graph order with promise jobs must match Chromium. (5) **`import()`**: `HostLoadImportedModule`, the host fetches and links the graph from a given referrer (script or module) and returns a promise of the namespace object; used by React's lazy loading and by GitHub's chunk loader. (6) **`import.meta`**: an object per module with `url` (and `resolve`). (7) **Host side**: module scripts evaluate in order after the parse; each module script is a graph; `load`/`error` events; errors of a graph as `error` on the element; the import map must be in the document before the first module fetch; a console message for each failure. The engine pieces (1, 3, 4, 6, and the TLA machinery) are the hard part; the fetch, the map and the events are host work in `web`/`engine`.

## 6. Draft of what "works" (JS on, 1280 x 800, logged out), and the split

**Draft list.** When M10 is done, with JavaScript on:

1. The first view renders like Chromium's replay: header with the five menus and "Pricing", search button, Sign in, Sign up; repository header with breadcrumbs, tabs, Watch, Fork, Star as links; branch selector button, "1 Branch", "0 Tags", "Go to file", Code button; latest-commit line with the author, message, "17646c1 - 4 days ago", the check icon and "86 Commits"; the file list with commit messages and relative times; About, Releases, Contributors. Geometry and pixel scores like the other targets (target values to be set by the owner: JS-off 0.9 geometry, JS-on to be measured after feature 8).
2. The page's scripts run without a console error, except the 404 of `payloads.json` that Chromium logs too.
3. The header menus open (`click`) with the content of Chromium and close with `Escape`.
4. The Code button opens the Clone popover with the HTTPS and GitHub CLI tabs; `Escape` closes it.
5. The branch selector opens its overlay with filter input and the branch list; `Escape` closes it and returns the focus.
6. "Go to file" takes focus; `t` focuses it; typing opens the overlay (with the results only if the fixture gets `tree-list`).
7. The latest-commit "..." button opens its popover.
8. Watch, Fork and Star navigate to the login URL; tabs and file links navigate (with the replay, the next page is not in the fixture).
9. Relative times show the text of Chromium (the page clock).
10. Tab order and `:focus-visible` after the load equal Chromium's.
11. The page's JS-off rendering (feature 1, 2, 7) matches at the scores of the fixture.
12. Time to `load` within the budget (1.56 s on an idle machine; the budget is to be recomputed there).

What replay cannot test: items under "Not testable in replay" in section 4 (login, search results, file finder results, keyboard dialog, soft navigation pages, hover cards, clipboard).

**Proposed features, in order.**

| # | Feature | Scope | Agent | Size | ADR | Changes JS-off? Other targets |
|---|---|---|---|---|---|---|
| 1 | `@layer` | layer order (statements, blocks, nested, anonymous, `@import ... layer`), unlayered priority, `!important` reversal, `revert-layer`, CSSOM `CSSLayerBlockRule`/`CSSLayerStatementRule` (check what exists); probes `github-css.json` | implementer | M | 0034 (small, or a section of the style ADR) | **yes** (JS-off geometry 0.16 to about 0.66). All targets: score check; Ars Technica and BBC have layers? (not measured) |
| 2 | Flex min-content | sum of items and margins for non-wrapping row (and column) flex containers; probes `github-nav.json` (+ layout tests) | implementer | S | none | **yes** (390 boxes). All targets with flex |
| 3 | Module graph | records, fetch, import map, `modulepreload`, link, evaluate, cycles, TLA, `import()`, `import.meta`, events, console; exit test: the 9 modules run in order without exception | implementer-hard (split: 3a engine records, link, evaluate, TLA; 3b host: fetch, map, `import()`, events) | L, 2 sessions | new ADR "ES modules" | no. BBC and Ars Technica module scripts keep working (the existing module map) |
| 4 | Custom elements | registry, `define`, `get`, `whenDefined`, `upgrade`, element constructors and `HTMLElement` as base, synchronous construction by the parser, upgrade of existing elements, reactions queue, `connected`/`disconnected`/`attributeChanged`/`adopted`, `observedAttributes` | implementer-hard | L | new ADR "custom elements" | no (JS off: custom elements are unknown elements). Wikipedia/others: `customElements` appears (feature tests in pages may change paths: check Craigslist, BBC) |
| 5 | Shadow DOM | `attachShadow` (open and closed, `delegatesFocus` later), `shadowRoot`, slots and assigned nodes, flat tree in style/layout/paint/hit testing/focus order, style scoping, `:host`, `::slotted`, `adoptedStyleSheets`, event retargeting and `composedPath`, declarative shadow roots (`<template shadowrootmode>` in the parser; **JS-off rendering**) | implementer-hard, split into 5a tree and flat tree, 5b styles, 5c events, focus, declarative | XL (3 sessions) | new ADR "shadow DOM" (also the flat tree in `layout`) | declarative shadow DOM changes JS-off rendering of pages that use it (none of the current fixtures: grep first). All targets: the flat tree touches `style`, `layout`, hit testing |
| 6 | Popover, dialog and the top layer | `popover` states, `showPopover`/`hidePopover`/`togglePopover`, `popovertarget` and `popovertargetaction` (**works with JS off in Chromium**), `toggle`/`beforetoggle`, light dismiss, `:popover-open`, `::backdrop`, `showModal`/`close`, top layer paint order and hit testing, `Escape` | implementer-hard | L | new ADR "top layer" | **yes**: popover with `popovertarget` and no script; `<dialog>` `open`. BBC (Save popover), Ars Technica (dialogs) may use it: check |
| 7 | Small CSS features | `@container` (size queries, `cq*` units, named containers; M), `@property`, `field-sizing`, `color-mix` with `currentcolor`/alpha, `content-visibility` value, then anchor positioning if the page needs it | implementer (7a `@container` can be implementer-hard) | M + S | section of an ADR | **yes** (computed style and layout) |
| 8 | First-view bring-up | run the page with 3 to 7 in place: fix what breaks (missing APIs A7, React hydration, `relative-time`, `selectionchange`, `indexedDB` stubs as needed), console clean, scores, budget; exit test `crates/automation/tests/github_swb.rs` with the records of Chromium | implementer-hard | L, open-ended | ADR note | no |
| 9 | Interactions | the list items 3 to 10: menus, Code popover, branch selector, go-to-file focus, keyboard shortcuts, Tab order; steps file and records from `chromium-a.json` as a base (`replay` command above) | implementer | M | none | no |

Order: 1 and 2 first (they change JS-off scores and unblock the rest; also fix the existing target scores only if they improve), then 3 (the page does nothing without it), 4, 5 (relative-time and tool-tip need both), 6, 7, 8, 9. Features 4 and 5 can start only after 3a; 6 and 7 are independent of 3 to 5 and could go before 4 if the owner wants JS-off gains first (CSS features and popover without scripts).

Owner decisions needed: (a) whether the 44 missing URLs get a new capture (`capture-missing`-like for `fixtures/js`, which the brief forbade me to run) so that the search dialog, the file finder and the shortcut dialog can be tested; (b) the target for `load` (budget 1.56 s, to be repeated on an idle machine); (c) whether anchor positioning belongs to M10 (the page feature-detects it).

## Proposed text for docs (not edited)

- `docs/roadmap.md`, backlog: remove "CSS (found by the GitHub candidate)", "`compare`: the Chromium reference of the GitHub capture is scrolled" and "`substitute` fails on raster `data:` URLs" (done in M10 step 1). Add: "Flex: the min-content size of a non-wrapping row flex container is the sum of its items (found by GitHub's header; `tools/probes/github-nav.json`)" and "CSS cascade layers are flattened (`crates/css/src/parser.rs`); GitHub is 39 % layered rules".
- `docs/targets.md`: `github-swb` JS-off fixture `fixtures/pages/github-swb`; score line after `just update-scores`.
- `docs/js-survey.md`: the survey's "JS on vs off 16.23 %" for github-swb was a scrolled capture; the value with the fix is 1.90 %.
- `docs/performance.md`: budget of github-swb 1.56 s (to be measured again on an idle machine).

## Tool changes (in the patch)

- `tools/swbtools/browser.py`: `scroll_to_top`, `scroll_through` with `behavior: 'instant'`; `reference.py`, `live.py`, `js_survey.py` use it.
- `tools/swbtools/substitute.py`: raster `data:` URLs in text bodies become SVG `data:` URLs; tests in `tools/tests/test_substitute.py`; Chromium test in `tools/tests/test_js_survey_chromium.py`.
- `tools/probes/github-nav.json` (12 cases; the flex min-content reproduction), `tools/probes/github-css.json` (24 cases: layers, `@container`, `:has`, nesting, `color-mix`, `light-dark`, `@property`, anchor positioning, popover, dialog, `field-sizing`, `@supports`, `content-visibility`).
- `fixtures/pages/github-swb` (3.0 MB) and its row in `fixtures/pages/README.md`; `docs/testing.md` (scrolling, `substitute`).
- `just tools-check` equivalents run: ruff format and check clean; `pytest tools/tests`: 395 passed, 3 skipped. I did not run `just check` (no Rust change).
