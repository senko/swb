# ADR 0032: Lazy loading of images

- Status: accepted
- Date: 2026-11-01

## Context

`loading="lazy"` tells the browser to load an image only when it is near
the viewport. swb loaded every lazy image at once. On the Ars Technica
fixture (62 `img`, 56 of them lazy) swb requested 72 images where Chromium
requested 20, and 25 of the extra requests were sources that the page never
asks for in Chromium: the `sizes="auto"` sources of images at
`display: none` and far below the viewport, selected with the width `100vw`.
The BBC fixture showed the same (swb 72 images, Chromium 21). A page that
replaces its own placeholders when a lazy image loads (BBC) also needs the
timing.

The specification
([lazy loading attributes](https://html.spec.whatwg.org/multipage/urls-and-fetching.html#lazy-loading-attributes),
"will lazy load element steps", "lazy load resumption steps", and the
"update the image data" algorithm) leaves the distance and the details to
the user agent. This ADR records what Chromium 148 does (black-box
measurements, `tools/probes/web-lazy-loading.json`, three equal runs; no
Chromium source was read, ADR 0021) and what swb does.

## Decision

### Scope

`img` elements (also in `picture`) of a document with scripting. Frames
(`iframe loading=lazy`) come with the frames of M9 feature 10 and are in
the backlog.

### Scripting off: nothing is lazy

The specification says that an image is not lazy when scripting is
disabled (an anti-tracking rule). Chromium 148 follows it: with JavaScript
off it requests every image of the page at once, also the lazy ones at
`display: none`, far below, in a closed `details`, and in `<picture>`
(`js-off-static`, `kinds-js-off`). swb does the same, so the output with
JavaScript off does not change (`just snapshot` of all fixtures is
identical before and after).

### The distances (measured)

A lazy image starts to load when its box is within **3000 CSS px above or
below** the viewport, and **1500 CSS px left or right** of it. The distance
is from the edge of the viewport to the nearest edge of the border box.
Images 2990 px below load; images at 3000 px do not (a box that only
touches the edge is outside). The distances do not depend on the viewport
size (800×300, 800×600, 1600×1000, 1280×800 tried) or on the scroll
position, and they are the same after scrolling by script, wheel and
keyboard (`dist-*`, `scroll-*`). A change of the viewport size checks
again (`dist-resize`).

Which box counts (`kinds-load`, `containers-*`):

- The painted box: the offsets of scroll containers, transforms, `fixed`
  and `sticky` positions apply. An image in a scroller loads when the
  scroller's offset puts it within the distance (`containers-scrolled`,
  `scroller-wheel`).
- Clipping does not matter. An image inside `overflow: hidden`, `auto` or
  `clip` that the clip hides completely loads when its unclipped box is
  within the distance.
- An element without a box never loads: `display: none` on the image or an
  ancestor, `hidden`, `<template>` contents, the content of a closed
  `details`, and `content-visibility: hidden`.
- Elements with a box load even if they are invisible or empty:
  `visibility: hidden`, `opacity: 0`, `width: 0; height: 0`,
  `display: contents` parents.
- `loading` is compared ASCII case-insensitively with `lazy`
  (`LAZY` is lazy; `" lazy"`, `bogus`, `eager` and a missing attribute
  are eager). `img.loading` reflects `auto` for a missing or invalid value
  (already so in swb).
- A `<picture>` image defers as a whole: the `<source>` is selected when
  the load starts.

### The state of an element (measured, `lazy-flag*`, `loading-toggle`)

- A lazy image in a document with scripting defers when its source is
  selected: on insertion into the document and on each change of `src`,
  `srcset` or `sizes`, as long as it is deferred. Only the last selection
  loads. A change of the source of a deferred image does not start a load.
- An image that is not in the document (`new Image()`) never loads while
  it is lazy; it defers again as soon as it is inserted.
- When the deferred load starts, the element is **released**: from then on
  it loads at once, also when it is far from the viewport, whatever its
  `loading` attribute says later. (Measured: a lazy image that loaded, then
  moved to 8000 px and got a new `src`: the new image loads at once. An
  eager image that loaded, then got `loading="lazy"` and a new `src`: the
  load defers.)
- A change of `loading` from `lazy` to anything else starts the deferred
  load at once ("lazy load resumption steps") and releases the element.
  `loading` from eager to lazy changes nothing until the next source
  selection.
- A failed load (404) counts as started: the element is released.
- `img.complete` is false, `naturalWidth` is 0 and `currentSrc` is empty
  while the load waits (`unstarted-state*`).

### The window `load` event

Images that wait for the viewport do not delay the window `load` event or
`document.readyState`. A lazy image that is within the distance at the load
also does not delay it in Chromium (`window-load`: a 500 ms image started
after the `load` event); in swb the check runs in the first frame, after
the `load` event, so it is the same. An `IntersectionObserver` callback for
the image comes before the `load` event of the image (`io-order*`).

### `sizes="auto"`

The source of a lazy image with `sizes="auto"` and `w` descriptors is
chosen after layout with the width of its box (`sizes-auto`: widths 100,
300, 700 and an attribute `width=500` give the 200w, 400w, 800w and 800w
candidates). A deferred image keeps the latest choice and loads that URL
when it is released, so an image at `display: none` or far below makes no
request (the 25 requests of Ars Technica). The page selects such images
again after each layout, also when a script changes the width
(`sizes-auto-resize`). The frame check does not lay out for these images
(see "The check" and the departures): the selection follows the next
layout, which the GUI makes for every frame with a DOM change.

### A known URL (measured, `same-url`, `same-url-404`)

A lazy image whose URL is loaded already (another image used it) does not
start either: `complete` is false until its box is near the viewport, then
it is complete at once, without a new request. Before that, `currentSrc` is
the URL and `naturalWidth` is the size of the loaded image (Chromium:
`[true, false, "same.svg", 30]` for the second image).

### The check

The document keeps the deferred images (element to URL) and the released
elements (`document/lazy.rs`). A check runs in the frame of the page
(`page/rendering_update.rs`, after the animation frame callbacks and before the
observers, HTML "update the rendering"):

1. It does nothing while no image is deferred. Images with `sizes="auto"`
   that are not deferred do not count: they are selected again after a
   layout that happens for another reason.
2. It is due, and then lays out (`update_layout`), when an element was
   deferred since the last check (its box is unknown), or the scroll
   offsets of the viewport or of a scroll container, the viewport size or
   the state of the style sheets changed. A change of an attribute of the
   image element itself selects its source again, which defers it again
   (`hide-show`: `display: none` to `block` on the image loads it in the
   next frame). Nothing else can bring a box near the viewport without a
   layout.
3. It is due, without a layout of its own, when a layout happened since the
   last check for another reason: paint (the GUI lays out for each frame
   with a DOM change), a layout query of a script, an observer. A DOM
   change alone does not make it due. A page that changes the DOM in every
   frame, with a deferred `display: none` image (common), so pays nothing
   in headless mode and one walk per frame in the GUI, where the layout
   exists anyway.
4. A due check walks the painted fragments once and starts the load of each
   deferred element with a box in range, in node order (8 ms for 100,000
   boxes). The state it compares (layout serial, scroll offsets, viewport,
   sheets) is compared in place, without a copy, for each question
   "is a frame due?".
5. A document whose frames cannot run (no script runtime, or the scripts
   stopped at the time limits) starts all deferred loads once.

Alternatives weighed. (a) Check after every DOM change, with a forced
layout (the first version): a page with 50,000 divs, one deferred
`display: none` image and a `requestAnimationFrame` loop that changes one
text node 300 times took 30.7 s, against 1.1 s without the image (a
layout and a walk per frame, new work in headless mode, which does not
paint). (b) Visit only the deferred images through the node-to-box lookup
of `getBoundingClientRect`: the painted position depends on the scroll
offsets and transforms of the ancestors, so the lookup would need the
same ancestor state as the walk; the walk costs 8 ms for 100,000 boxes
and runs only for a scroll or a layout, so it stays. (c) Track which
deferred elements had no box and check them after a style change: the
engine has no cheap signal for "style changed" (every DOM change marks the
styles dirty), so this equals (a). The chosen rule follows the policy of
`IntersectionObserver` (ADR 0028): observations come with a layout that
happens anyway, and a scroll or a viewport change makes one.

The check runs in the GUI (the pump of the page), the headless runner and
the automation server alike, since all call `Page::pump`.

## Departures from Chromium

- Chromium decides lazy or eager in a microtask after `src` is set. swb
  decides when it processes the DOM changes, after the microtasks of the
  task. `img.src = x; await null; img.loading = 'lazy'` loads in Chromium
  and defers in swb (`detached-attr-order`).
- Chromium's keyboard scroll (`End`, `PageDown`) is animated, and the
  check at each frame of the animation loads the images that the viewport
  passes. swb jumps to the end, so images more than 3000 px above the new
  viewport are skipped (`scroll-key-end`). Scrolling by wheel steps and by
  script steps with a frame between them gives the same loads.
- swb has no `content-visibility`: the content of `content-visibility:
  hidden` has boxes, so its lazy images load (`kinds-load`; backlog).
- After a `sizes="auto"` source failed and the image has no width, Chromium
  selects the `100vw` source again; swb keeps its first selection
  (`sizes-auto`).
- With scripting off, Chromium sometimes requests the `100vw` candidate of
  a `sizes="auto"` image as well as the candidate for the laid-out width
  (two requests, in some of the runs); swb requests the second only. This
  is not lazy loading and does not change the output.
- A mutation of an ancestor (a class that shows a hidden parent) or a
  script that moves the image away from the viewport or into it by a
  change elsewhere in the page loads the image at the next layout, not in
  the next frame. In the GUI this is the same frame (paint lays out). In
  the headless runner, which paints nothing between the steps, it is the
  next layout query, scroll, viewport change or the final output. The
  probe case `sizes-auto-resize` shows it: the page changes the width of a
  loaded `sizes="auto"` image, and the new source is requested at the next
  layout (`DEPARTURES` in `lazy_loading.rs`). The cheaper rule is chosen
  on purpose (finding of the review of feature 4: 30.7 s against 1.1 s).
- A lazy image of a loaded URL (`same-url`) is complete only when released
  as in Chromium, but its loaded image already shows and sizes the box
  (the engine selects the image of the element before the lazy decision);
  Chromium shows nothing until the release.
- The distances are fixed. Chromium takes them from the network
  information (measured here without it: 3000 and 1500). swb has no
  network information.

## Consequences

- Ars Technica at the load: the 20 photos that Chromium requests; after
  the wheel to the end and back: the same 37 (`crates/automation/tests/
  ars_technica.rs`). BBC: the same 21 images as Chromium (was 72). The
  placeholder timeline of BBC needs hydration (feature 6) and is checked
  in feature 8.
- The scores of the six JavaScript fixtures do not change.
- A page that reads `img.complete` or `naturalWidth` of a lazy image that
  has not started sees Chromium's values.
- Frames lay out for documents with deferred images only for a new
  deferred image, a scroll, a viewport change or a change of the style
  sheets. The page of 50,000 divs with a deferred `display: none` image
  and 300 text changes takes 1.5 s (1.1 s without the image; the first
  version took 30.7 s). A hostile page with 100,000 lazy images and 100
  frames of scrolling (17.4 s) pays 8 ms per frame for the walk (0.8 s).
  The page costs 12.1 s with JavaScript on and no scrolling script, and
  11.8 s with eager images (measured), so most of the time is not the
  check; the check had the same cost per frame before the fix, because
  the layout of that page does not change between frames.
