# M12 step 1: diagnosis of openstreetmap.org (Zagreb)

The report of the diagnosis that plans the features of M12 (docs/roadmap.md,
"M12"), written on 2026-11-18 (part 1a: tools and recordings, part 1b: this
report). Base 8e93e61 plus part 1a. Chromium 148.0.7778.96 (Playwright 1.60).
Capture of 2026-11-17 (`captured` 21:36:25Z; `--now 1795037785000`, computed
from `fixture.json`). The files named under `1b/` (logs, runs, screenshots,
helper scripts) were working files of the session and are not kept; the tools,
the probe files and the traces are in the repository, the fixtures are local.

Part 1b ran while another program loaded the machine (1-minute load 15 to
18). The budget, the load time of swb and the GUI trace were measured again
on the idle machine (load 1.5 to 3.6) and section 5 gives both sets; the
summary gives the idle numbers. The slow-group times of section 2 are from
the loaded machine. Scores, records and counts do not depend on load.

## Summary

- The first view needs one thing: a `Worker`. With JavaScript on, swb stops at `new Worker` in the main script (MapLibre makes its worker pool while the script runs) and shows no map: geometry 0.3973, pixels 0.4632, 87 boxes missing. With a `Worker` stub in a copy of the fixture the first view equals Chromium's (geometry 1.0000, pixels 0.9977), and **all 60 step groups run in swb in 5 to 10 s**, with records equal to Chromium's in every field except two classes of differences (below).
- With the stub, and leaving out the Transport Map group (below), swb's records equal Chromium's in every field of all 60 groups except `elements` and `outlines`. `elements`: 3 elements of a Bootstrap tooltip that swb never shows (18 groups), and 53 to 56 elements of the layer thumbnails that Chromium removes when the panel closes and that stay without WebGL. `outlines` (from the object pages on): one hover highlight is missing. The causes are small: `Attr` is not a `Node` in swb (a script error in a timer), and swb sends no mouse events when the content under a stationary pointer changes.
- **The Transport Map is a vector map** (MapLibre, WebGL; its style loads with the site's `apikey`, a second request for it with `key=undefined` gets 401, and Chromium's replay shows no Transport tiles either). The plan's list of "six raster maps" is wrong for this OSM version. The first click on it throws in swb, and the layer panel then breaks. It breaks the same way in Chromium without WebGL (`--disable-3d-apis`): with the Transport Map group in the steps, **swb's records equal those of Chromium without WebGL in every field except `elements` and `outlines`**. The owner decides about item 4 (section 6).
- The layer panel draws its eight thumbnails with MapLibre, also for the raster maps: with WebGL they show the map, without WebGL (Chromium and swb) they are empty grey boxes. The panel still opens and the raster maps switch.
- **XML documents are not needed.** This OSM version asks its API for JSON (`map.json`, `notes.json`, `relation/ID/full`); the only `DOMParser` call of the page is `text/html`; `responseXML`, `parseXML` and `XMLSerializer` are not called. ADR 0044 and its feature can leave M12.
- **Dedicated workers are not needed for items 1 to 12 either.** The page makes one blob worker at load and posts one message; nothing in the raster-only flows waits for its answer (the MapLibre worker serves vector sources). A worker that accepts messages and runs nothing is enough; the full feature (a planned ADR) serves only the vector maps (not in M12).
- Gaps found for items 1 to 12 (section 4), all S: `ImageData`, `Attr` as a node, the GIF without a color table, `HTMLCanvasElement.getContext` (null for WebGL), a minimal `Worker`, mouse events after a layout change under the pointer, SVG `<use>` of a `<symbol>` (the direction icons), the scroll of an input after a script sets its value.
- Budget (M9 method, idle machine): 3 x 0.142 s (Chromium jitless task, `js-survey --runs 3`) + 0.012 s (JS-off pipeline, `perf`) = **0.44 s**, as the plan estimated (0.43 s). swb needs 0.21 s to `load` with the stub (7 runs, 0.209 to 0.222 s), so the budget is met with a margin of 2 times.
- GUI trace `osm-map` (synthetic, 13 s, idle machine): pointer move to frame median 12 ms, p90 13 ms, max 18 ms during the drags (the M12 goal is 50 ms); 27.4 frames/s and 31.9 % CPU during the trace; idle: 0 frames/s, 0 % CPU.
- Proposed features (section 7): the plan's six features become four, and no ADR: 1 small gaps (with the `Worker` stand-in), 2 pointer events after layout changes, 3 the exit test `osm_zagreb.rs` with Chromium's records and the 60 groups (the plan's "map" and "sidebar" features: nothing left to build, only to test and to fix what the test finds), 4 the exit measurement. JS off: nothing to do (geometry 1.0000, pixels 0.9992).

## 1. Baselines

**Chromium (replay, JS on).** `reference --js osm-zagreb` and the stub: 2 URLs not in the fixture (the two `matomo.php` requests, timestamped), 4 console errors (the Matomo cookie `_pk_ses` error twice, two failed Matomo requests). Chromium 148 has WebGL here (`webgl=1` in the Matomo URL). The page has 522 elements.

**swb, JS on, original fixture** (`compare --js osm-zagreb`): geometry 0.3973, size 0.3973, relative 0.3973, pixels 0.4632, missing 87, extra 1. The page shows the header and the two empty forms on a grey area (screenshot `1b/compare-orig.png`). Console, in order:

```
console: application-....js: Uncaught ReferenceError: Worker is not defined
console: i18n/en-....js: Uncaught TypeError: Cannot read properties of undefined (reading 'store')
console: jQuery.Deferred exception: L.OSM.Map is not a constructor TypeError ...
console: a timer: Uncaught TypeError: L.OSM.Map is not a constructor
```

The first line is the cause of the others: the script stops at the worker pool, `OSM.i18n` and `L.OSM.Map` are not defined. The load takes 0.24 s.

**swb, JS on, stub fixture** (`compare --js osm-zagreb-stub`): geometry 1.0000, size 1.0000, relative 1.0000, pixels 0.9977, missing 0, extra 0. One console line that Chromium does not have (plus the Matomo noise, which Chromium has too): `a promise: Uncaught (in promise) ReferenceError: ImageData is not defined`. In the whole step run swb adds two lines to the console: the `toLowerCase` `TypeError` in a timer, after the groups that show a tooltip (section 2), and one warning, `cannot decode image: Format error decoding Gif: no color table available for current frame`.

**JS off** (`fixtures/pages/osm-zagreb`, part 1a): geometry 1.0000, pixels 0.9992; nothing to do.

**The stub.** `tools/osm_zagreb_records.py stub` makes `fixtures/js/osm-zagreb-stub`: a copy of the fixture whose `application-*.js` starts with a class `Worker` that does nothing (`postMessage`, `terminate`, `addEventListener`, `removeEventListener`, `dispatchEvent`, `onmessage`/`onerror`/`onmessageerror` as null), defined only if `window.Worker` is missing. The stub is only for swb (Chromium has workers); the Chromium records are those of the original fixture.

## 2. The step groups in swb

**Method.** `osm_zagreb_records.py swb`: one `swb --headless --js --replay` run (options of `compare --js`) with the steps of the 60 groups, a pause of 300 ms after each step (as `replay`), and after the load and each group a `record` step (`window.__o()`) and a `screenshot` step (new step kind, see docs/testing.md). The log of swb is split at the screenshot lines, so each group has its own warnings. No new mode of `swb --headless` was needed. The runner translates Playwright's `:visible` and `>> nth=N` (the step file uses both) into a mark attribute, so the clicks still go through swb's pointer and hit test. The result: `swb-records.json` and `swb-diff.json` (the fields that differ), per group.

**Run with all 60 groups, stub fixture.** 61 of 61 records (the run did not stop at a step; `swb --steps` supports all kinds of steps of the file). Groups 0 to 14 differ from Chromium only in `elements`. From group 15 ("layer transport map") on, the records differ in the hash, the attribution, the tiles and the elements: the Transport Map click throws `TypeError: this._canvas.getContext is not a function` (MapLibre makes a WebGL canvas), the layer switch is half done, and the next five layer switches fail (`Cannot read properties of undefined (reading 'remove')`, then `NotFoundError ... removeChild`), then every animation frame fails (`Cannot read properties of undefined (reading '_actualCanvas')`).

**Chromium without WebGL** (`--disable-3d-apis`, through a one-off driver that adds the flag to `browser.launch`; `replay` has no option for flags): the groups 0 to 14 are identical to the run with WebGL; the Transport Map group fails in the same way (the hash stays `layers=C`, the attribution is empty) and so do the later layer groups. `OSM.MapLibre.Map` catches `webglcontextcreationerror` and fetches `/panes/webgl_error` for the thumbnails (not in the fixture: `ERR_FAILED`, eight times). **Compared with this run, swb's records are equal in all 61 records except the field `elements` (3 fewer in groups 6 to 12, 23, 24, 27 to 32, 36, 37 and 46: the tooltip; 1 fewer in groups 48 to 60: the hover circle) and `outlines` (groups 48 to 60).** That is the measure of what swb lacks without WebGL.

**Run with group 15 left out** (`--skip 15`, to see past the Transport Map): equal to Chromium (with WebGL) in all fields, in all groups, except `elements`, `outlines` (48 to 60) and group 15 itself. `elements` is 3 fewer for the tooltip (groups 6 to 12, 23, 24, 27, 28) and 53 to 56 more from group 29 ("layers close") on: the eight thumbnail maps keep their DOM (`div.maplibregl-map`, canvas container, four control corners) after the panel closes. Chromium without WebGL keeps them too (the records above are equal), Chromium with WebGL removes them. The five other raster maps, the three overlays (Map Notes; Map Data: "Load Data" at zoom 16 with 15410 features and 1190 paths at zoom 18; GPS traces), the zooms, the panes, the search, the query, the directions, History and Export all give Chromium's hash, tiles (the same 20 to 24 tile URLs), base layer, overlays, panes and texts, counts, markers and scale. The pixels of the screenshots agree to 99.3 to 99.7 % in the groups without the layer panel and 98 to 99 % in the sidebar groups (differences of anti-aliasing in the map tiles); 91 % with the layer panel open (the empty thumbnails) and 95 % after "directions go" (icons, below).

**Where each difference comes from (first cause):**

| What | Evidence | Cause |
|---|---|---|
| `elements`, 3 fewer in swb | DOM diff after group 6: `div.tooltip.bs-tooltip-auto.fade.show`, `div.tooltip-arrow`, `div.tooltip-inner` only in Chromium | Bootstrap's tooltip on the control buttons (hover and focus after the click). Its `sanitizeHtml` loops over `[].concat(...element.attributes)` and reads `attribute.nodeName.toLowerCase()`: in swb `Attr.prototype.nodeName` is undefined (`Attr` is an `EventTarget` with `name`, `value`, `localName`, `namespaceURI`, `ownerElement`, `prefix`, `specified`; no `Node` members), so `show()` throws in a timer: `TypeError: Cannot read properties of undefined (reading 'toLowerCase')`, at `application.js` `sanitizeHtml` from `Tooltip._createTipElement`. Probe `web-osm-gaps.json`, cases `attr-is-a-node` and `bootstrap-sanitize-html`. |
| `outlines` +1 path (group 48 on; length 238 against 193) | the circle `M751,399a10,10 ...` of a hover highlight is in Chromium only; the relation's own geometry (two polylines) and the query marker are equal | In the "Query features" list the `mouseover` on a link adds the feature's geometry (`$(this).data("geometry")`). After the click on the relation link Turbo replaces the sidebar and Chromium sends `pointerover`, `mouseover` and `mouseenter` to the element that is now under the stationary pointer (a node link, whose geometry is a `circleMarker`); the highlight stays after the page change (a bug of the site). swb sends no mouse events after the content changes under the pointer (event log after the click, with 3 s of event loop: none). Probe `web-osm-gaps.json`, case `events-after-the-content-under-the-pointer-is-replaced`. |
| Empty thumbnails in the layer panel | screenshot 012 | MapLibre needs WebGL (above); same as Chromium without WebGL. The failed maps also keep their DOM after the panel closes (53 to 56 more elements than Chromium with WebGL). |
| Direction icons, input scroll (group 55) | screenshot 055: no start/destination icons in the steps and the pins in the two inputs; the second input shows the end of its text (`l, Zagreb, ...`) | The icons are inline `<svg><use href="#routing-sprite-start">` and `<use href="#pin-start">` of `<symbol>`s in the same document; swb draws nothing for them. The input: Chromium scrolls a text field back to the start when a script sets its value; swb keeps the end (the caret). Not examined further (not in the records: the fields of `window.__o()` do not read them). |
| `ImageData is not defined` (load) | console, group 0 | MapLibre's message serializer (`instanceof ImageData`) runs when the page posts the setup message to the worker; in the stub fixture it is the only script error at load. A rejected promise: nothing else fails. |
| GIF warning (group 3 on) | `data:image/gif;base64,R0lGODlhAQABAAD/ACwAAAAAAQABAAACADs=` is Leaflet's `emptyImageUrl` | **Confirmed**: swb cannot decode a GIF whose frame has no color table (here the image is only a 1 x 1 placeholder: the header has no global table, the frame has no local table); Chromium loads it as 1 x 1 (`load`, `naturalWidth` 1), swb fires `error` (probe `gif-without-color-table`). The tile counts of the records are equal, so Leaflet's use does no harm; the `error` event on a tile that is being reset may log a `tileerror`. |
| `TypeError` after the zoom click | see the tooltip row | **Confirmed**, and explained: it is the tooltip, not the zoom. |

**Items 1 to 12** (stub fixture; "records" are the fields of `window.__o()`; the groups of item 4 without group 15):

| Item | Chromium's replay | swb with the stub | Cause of the gap | Gap |
|---|---|---|---|---|
| 1 first view | 24 of 24 tiles, marker, welcome, controls | equal (geometry 1.0000, pixels 0.9977) | none | none |
| 2 console | only the Transport Map style 401 and Matomo | extra: `ImageData` at load; the tooltip `TypeError` in timers; the GIF warning | `ImageData`; `Attr`; GIF | S, S, S |
| 3 map | drag, wheel, double click, buttons, keys; the hash follows; 24 tiles (20 after a pan) | records equal | none (the tooltip adds `elements`) | none |
| 4 layers | panel opens; five raster maps switch; Transport Map: no tiles (401); three overlays | panel opens, five raster maps switch (equal hash, tiles, base), overlays equal; thumbnails empty; **the Transport Map click breaks the panel** | WebGL (vector style); `getContext` missing | decision (section 6) |
| 5 key, share | legend; link, short link, HTML, marker | equal | none | none |
| 6 welcome | closes | equal | none | none |
| 7 search | 2 results; the first opens way 105489682, outlined, zoom 18 | records equal | none | none |
| 8 where is this | internal and Nominatim results; relation 1493356 opens | equal | none | none |
| 9 directions | OSRM: 10 steps, 2.7 km, route drawn, map fitted | records equal; the icons of the steps and the pins in the inputs are missing, the second input is scrolled | SVG `<use>` of a `<symbol>`; input scroll after a script sets the value | S, S |
| 10 query features | 18 nearby, 7 enclosing, click point outlined | equal | none | none |
| 11 object pages | relation, way, node: tags, outlines | equal except `outlines` (the hover circle) | mouse events after a layout change | S to M |
| 12 history, export | changesets (38 outlines), export form; back and forward | equal | none | none |

**Without WebGL, what happens in the layer panel.** In swb: `getContext` is not a function on a canvas, MapLibre's constructor throws a `TypeError`, OSM's `catch` calls `JSON.parse(error.message)` (a `SyntaxError` for this message) and rethrows; the thumbnails stay empty and the panel works. In Chromium without WebGL: `getContext('webgl')` is null, MapLibre throws the JSON message, OSM fetches `/panes/webgl_error` (not in the fixture) and puts it in the thumbnail; the panel works and the Transport Map fails as in swb. To get the same behavior swb needs `getContext` returning null for the WebGL context types (S); `/panes/webgl_error` can be added to the fixture (`capture-missing`).

**Wall time of the slow groups in swb** (release, virtual time, under load; the whole 60-group run 5.4 s with the Transport Map failing, 9.8 s without it; wait steps cost nothing in virtual time): Map Data at zoom 16, group 20: 0.51 and 0.57 s (the request of the 11 MB `map.json` and its parse; 15410 features); Map Data at zoom 18, group 25: 0.45 and 0.23 s (1190 paths); layer switches 0.03 to 0.3 s; search 0.05 to 0.19 s; "where is this" result 0.17 to 0.22 s; object pages 0.05 to 0.16 s; directions 0.05 to 0.2 s; History 0.05 to 0.2 s; the load 0.30 s. Nothing is slow.

## 3. The platform

`js-survey osm-zagreb --runs 3` (Chromium, with WebGL): 4 script elements (1 module: `turbo`), 7 compiled scripts, 1768 KB (the 1.5 MB `application.js` has jQuery, Bootstrap, Leaflet, MapLibre GL with its worker source, and the site's code); 180 distinct members; API operations: parse 464, load 4089, after 1098; element counts 522 with JS and 379 without; 41 requests; first viewport differs in 57.73 % of the pixels JS on against JS off. Listeners: 55 `click`, 31 `load`, 25 `error` and `mousemove` (24 map tiles), 24 `selectstart`, 13 `dblclick`, `mouseover`, `mouseout`. Mutations: 913 records at parse, 510 after.

| Web API | Where the page uses it | Items | swb | Size |
|---|---|---|---|---|
| `Worker` (blob URL, `postMessage`) | `OSM.MapLibre` worker pool at load (1 `new Worker`, 1 `postMessage`); the worker code has `importScripts` (2), `fetch`, `OffscreenCanvas` (6), `createImageBitmap` (4), `ImageData` (18), `structuredClone`-style serialization with a transfer list, `getContext` (11) | 1 (must not throw); no answer is awaited in items 1 to 12 | `Worker` undefined: the script stops | stand-in that accepts messages: S; the full feature (script runs in a worker global scope, structured clone and transfer, `importScripts`, `fetch`, timers, `terminate`, errors; the probe `web-workers.json` has 16 cases): L, only for the vector maps |
| `ImageData` | MapLibre's serializer (`instanceof ImageData`) at load; the worker code; `new ImageData(1,1)` in `createImageBitmap` fallbacks | 2 | undefined: a rejected promise at load | S (constructor, `width`, `height`, `data`, `colorSpace`, `structuredClone`) |
| `Attr` as `Node` | Bootstrap's tooltip (`sanitizeHtml`: `nodeName`, `nodeValue`); any `[].concat(...el.attributes)` | 2, 3 (every control button) | `EventTarget` with 8 properties; `cloneNode`, `isEqualNode`, `nodeType`, `textContent`, `ownerDocument` missing; `getNamedItem().nodeName` null | S |
| GIF without a color table | Leaflet `emptyImageUrl` | 2, 3 | warning and `error` event | S (the `gif` decoder) |
| `HTMLCanvasElement.getContext` | MapLibre (WebGL; `webgl2` then `webgl`), Matomo (WebGL detection, `webgl=1`) | 4 | a canvas without `getContext` | null for `webgl`, `webgl2`, `experimental-webgl`: S; Canvas 2D (fills, `getImageData`, `toDataURL`): L, not needed by items 1 to 12 |
| WebGL | thumbnails of the layer panel (8 MapLibre maps), the vector layers (Transport Map, Shortbread, MapTiler OMT); `OSM.MapLibre.showWebGLError` fetches `/panes/webgl_error` when the context cannot be created | 4 (the Transport Map group) | none | XL, not in M12 |
| `OffscreenCanvas`, `createImageBitmap`, `ImageBitmap` | the worker code only (vector tiles, sprites, glyphs) | none | undefined | M each, backlog |
| XML documents: `DOMParser` with `text/xml`, `responseXML`, `XMLSerializer` | jQuery's `parseXML` and `responseXML` converters (not called: the API answers JSON); `L.OSM.XMLParser` is used only when the data is a `Document`; `DOMParser` `text/html` in Turbo's frames and the export error | none | `parseFromString` throws `NotSupportedError` for XML types; `XMLSerializer` undefined | not needed; if done later: L (probe `web-xml.json`, 10 cases) |
| Mouse events after layout change | "Query features" list hover highlight | 11 (outlines) | none | S to M (a timer after a layout change that finds the element under the pointer and sends the boundary events and the `:hover` update) |
| SVG `<use href="#id">` of a `<symbol>` | direction icons, route marker pins | 9 | not drawn | S to M (the SVG crate) |
| `Intl`, `MutationObserver`, `IntersectionObserver`, `ResizeObserver`, `matchMedia`, `history`, `fetch`, `URLSearchParams`, `requestAnimationFrame` | the rest of the page | all | work (no error) | none |

The probe files for the rows above: `tools/probes/web-workers.json`, `web-xml.json`, `web-osm-gaps.json` (docs/testing.md). Each ran three times in Chromium with equal output; under `--with-swb` every case differs, except `structured-clone-function`, because the APIs do not exist yet.

## 4. Gap list

Sizes as in the M10 report: S (one session, a few hundred lines), M (one feature), L (one hard feature with an ADR), XL (several features).

| # | Gap | Cause and evidence | Items | Size | Crates |
|---|---|---|---|---|---|
| 1 | `Attr` as a node | `nodeName` etc.; tooltip script error and 3 missing elements in 18 groups | 2, 3 | S | dom, web (bindings) |
| 2 | `ImageData` | rejected promise at load | 2 | S | web |
| 3 | `Worker` stand-in | the page stops at `new Worker` | 1 to 12 | S | web |
| 4 | `getContext` returns null for WebGL | thumbnails and vector layers as in Chromium without WebGL | 4 | S | web |
| 5 | GIF without a color table | warning, `error` event | 2 | S | image decoding |
| 6 | Mouse events after a layout change | hover highlight of the query list | 11 | S to M | engine (input), web |
| 7 | SVG `<use>` of a `<symbol>` | direction icons | 9 | S to M | svg, paint |
| 8 | Input scroll after `value` is set by a script | second direction input | 9 | S | engine (forms) |
| 9 | `/panes/webgl_error` in the fixture | Chromium without WebGL fetches it | 4 | tool (`capture-missing`) | none |
| 10 | Transport Map (vector, WebGL) | item 4 as written cannot hold | 4 | decision | none |
| 11 | Dedicated workers (full) | vector maps only | none | L | engine, web, js |
| 12 | XML documents | not used | none | L | dom, web |
| 13 | Canvas 2D, `OffscreenCanvas`, `createImageBitmap`, WebGL | vector maps, thumbnails | none | L to XL | web, paint |

Not found: layout or paint differences in the first view and in the panes (geometry 1.0000), CSS gaps, scripts that need modules, custom elements or shadow DOM (the page has none), `fetch` or XHR failures (every request of the groups is in the fixture; the two Overpass `POST`s, the geocoder `POST`s and the cookies work; Matomo is the only noise).

## 5. Budget, load time and GUI

**Budget** (M9 method, idle machine, load 1.5): `js-survey osm-zagreb --runs 3`: jitless `TaskDuration` 0.142 s, script 0.090 s, 13 layouts, 37 style recalculations; JIT: task 0.143 s, script 0.092 s. `perf osm-zagreb` (JS off, the new page fixture, 379 elements): total 11.6 ms (parse 0.5, stylesheets 9.1, style 1.2, layout 0.3, display list 0.04, raster 0.5). Budget for the `load` event = 3 x 0.142 + 0.012 = **0.44 s**, as the plan's estimate (0.43 s, from a task time of 0.138 s). Under load 16 (part 1b) the same runs gave a task of 0.238 s, a pipeline of 17.8 ms and a budget of 0.73 s: the task time depends on the load of the machine, so the budget is the idle one.

**Load time of swb** (release, replay, JS on, `--virtual-time`, stub fixture, 7 runs, wall time with process start, idle machine): `--settle-ms 0`: 0.209 to 0.222 s (median 0.210); `--settle-ms 1500`: 0.204 to 0.214 s (median 0.209). Both are below half of the budget. Under load 16 the medians were 0.338 and 0.329 s. The original fixture stops early (0.24 s under load).

**GUI** (`gui-latency osm-zagreb-stub --js --trace osm-map --window 1280x900`, own Xvfb, release binary). Idle machine (load 2.7):

| Input | Events | to batch end median / p90 / max (ms) | to frame median / p90 / max (ms) | no frame | no frame needed |
|---|---|---|---|---|---|
| move | 182 | 2 / 2 / 22 | 12 / 13 / 18 | 0 | 5 |
| press | 5 | 1 / 2 / 2 | 13 / 13 / 13 | 0 | 4 |
| release | 5 | 6 / 9 / 9 | 17 / 20 / 20 | 0 | 0 |
| wheel | 11 | 0 / 24 / 49 | 49 / 49 / 49 | 0 | 10 |

Frames per second 27.39; CPU 31.9 % during the trace; style runs 391 (168 ms) and layout runs 375 (219 ms) in the trace. Under load 16 (part 1b):

| Input | Events | to batch end median / p90 / max (ms) | to frame median / p90 / max (ms) | no frame | no frame needed |
|---|---|---|---|---|---|
| move | 182 | 11 / 21 / 29 | 25 / 37 / 46 | 0 | 19 |
| press | 5 | 2 / 3 / 3 | 16 / 16 / 16 | 0 | 4 |
| release | 5 | 32 / 37 / 37 | 30 / 35 / 35 | 0 | 0 |
| wheel | 11 | 1 / 33 / 61 | 32 / 60 / 60 | 0 | 8 |

Frames per second 24.64; CPU 41.5 % during the trace; style runs 366 (261 ms) and layout runs 354 (336 ms) in the trace (358 targeted restyles after DOM changes, 8 after state changes). `idle-10s`: 0 frames/s, 0 % CPU, no runs. The goal of feature 3 of the plan (p90 of the pointer move to its frame at most 50 ms during a drag) is met (idle: 13 ms, max 18 ms; under load: 37 ms, max 46 ms; the 182 counted moves are the moves of the drags and the moves that place the pointer). Nobody looked at the screenshots of the GUI during the trace, so it is not verified that each drag moves the map in the window, and 8 (under load) and 10 (idle) of the 11 wheel notches request no frame at once (Leaflet zooms from a timer that its own `wheelDebounceTime` starts). The new trace is `tools/traces/osm-map.json` (`tools/make_traces.py synthetic`).

## 6. Decisions for the owner

The owner took the three recommendations on 2026-11-18: item 4 has five raster maps and swb gets `getContext` returning null for WebGL, so that the Transport Map group fails as in Chromium without WebGL (1, (a) with (b)); dedicated workers and XML documents leave M12 (2); the exit test compares records in every group and pixels only with the layer panel closed (3).

1. **Transport Map and item 4.** In this OSM version the Transport Map is a MapLibre vector style; one request for it (`style.json?key=undefined`) gets 401. Chromium with WebGL shows no tiles for it; without WebGL (and in swb) its click throws and the layer panel stops switching. Options: (a) item 4 says "five raster maps" and the Transport Map is out like the two vector maps (the group stays in the steps as "breaks like Chromium without WebGL"); (b) swb gets `getContext` returning null for WebGL and the Transport Map fails like Chromium without WebGL (the records are then equal; this is gap 4); (c) WebGL (XL, not in M12). Recommendation: (a) with (b).
2. **Workers and XML.** The diagnosis finds no need for the planned ADRs on full workers and XML documents in M12 (planned as 0043 and 0044; 0043 went to blend modes in feature 4). Recommendation: drop both from M12; the `Worker` stand-in goes into feature 1. If the owner wants workers for the platform, the workers ADR stays as planned and the probe file `web-workers.json` has the cases. Note the cost: the MapLibre worker source (inside `application.js`) would run at load in every map that makes a worker, for no result in M12.
3. **Chromium without WebGL as the reference for the layer panel.** The thumbnails differ from Chromium with WebGL in about 9 % of the pixels while the panel is open. Recommendation: the test compares records, and pixels only with the panel closed.

## 7. Proposed features of M12 step 3

The plan had: dedicated workers, small gaps, the map, XML documents, the sidebar, JS off. After the diagnosis:

| # | Feature | Scope | Agent | Size | ADR |
|---|---|---|---|---|---|
| 1 | Small gaps and the `Worker` stand-in | gaps 1 to 5 and 8: `Attr` as a node, `ImageData`, `Worker` (a constructor that accepts `postMessage` and runs nothing; says so in a `warn!`), `getContext` null for WebGL, the GIF decoder, the scroll of a text field after a script sets `value`; probe cases from `web-osm-gaps.json` (Chromium's values in the Rust tests as for `small_gaps.rs`); the scores of the other targets stay | implementer | S to M (one session) | none |
| 2 | Pointer events after layout and SVG `<use>` | gap 6 (a mouse boundary update after a layout or DOM change under a stationary pointer: `pointerover`/`mouseover`/`mouseenter`/`mouseout`/`mouseleave`, the `:hover` update; the probe cases `events-after-*`; check the cost on the other targets: it is a timer and a hit test), gap 7 (`<use href="#id">` of a `<symbol>`, with `currentColor`/custom properties) | implementer | M | none |
| 3 | The exit test | `crates/automation/tests/osm_zagreb.rs`: the 60 groups with the records of Chromium (`data/osm-zagreb-chromium.json` from `osm_zagreb_records.py records`, three equal runs); from group 15 on, the records of Chromium without WebGL (owner's decision 1; a `replay` option for Chromium flags makes them reproducible, which the one-off driver of part 1b is not); pixels of a few points with the layer panel closed (decision 3), the URLs that the fixture lacks (`/panes/webgl_error` fails in both browsers the same way and need not be added), the console; fix what the test finds in the map, the layer panel and the sidebar (the plan's features 3 and 5) | implementer | M (open-ended) | none |
| 4 | Exit measurement and maintenance | the budget and the load time on an idle machine; `gui-latency osm-map` with a check of the screenshots (each drag moves the map; a Playwright-free check: the hash after the trace); hostile cases only if feature 1 adds a loop (none expected); end-of-milestone maintenance | orchestrator | S | none |

Not in M12 (backlog): dedicated workers (L), XML documents (L), Canvas 2D (L), `OffscreenCanvas` and `createImageBitmap` (M each), WebGL (XL) and with it the layer thumbnails, the Transport Map and the two vector maps.

## 8. What was done and what was not

Done: baselines (original and stub fixture, `reference --js` and `compare --js`), the 61 records in swb (with and without group 15, and the first 6 groups on the original fixture, which stops at the welcome box: `.welcome .btn-close` has no box), Chromium without WebGL for all 60 groups, the DOM diff after group 6 and group 48, event logs of the click on the relation link in both browsers, the survey, `perf`, the load times, the GUI trace and `idle-10s`, the three probe files (three equal Chromium runs each; `--with-swb` once).

Not done: the pixel comparison of the screenshots with a score (only counts of differing pixels per group); the GUI screenshots during the trace; the CSS of the direction inputs (padding for the pin: `ps-4` only); a probe case that reproduces the icons and the input scroll; the slow-group times of section 2 on an idle machine.
