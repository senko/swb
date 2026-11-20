# M13 step 1: diagnosis of videojs.org (front page)

The report of the diagnosis that plans the features of M13 (docs/roadmap.md,
"M13"), written on 2026-11-20. Base f6125e0 plus the tools of this step.
Chromium 148.0.7778.96 (Playwright 1.60). Capture of 2026-11-20 (`captured`
19:52Z for the first JS-on capture; the final one with the step groups
followed); `--now` is computed from `fixture.json`. The page is an Astro site
with React islands (Base UI) and the Video.js 10 player; this report is about
the page without playback.

Measured on a machine with a load of 0.8 to 1.3 (idle), release build of the
base commit.

## Summary

- **Fixtures.** `fixtures/pages/videojs` (JS off, committed: 14 entries,
  1.0 MB; the thumbnail is a placeholder, the two Mux display fonts are DejaVu
  Sans) and `fixtures/js/videojs` (JS on, local only: 275 entries, 5.4 MB,
  with the HLS manifests and the first segments that Chromium fetches). Seven
  step groups (`crates/automation/tests/data/videojs-steps.json`) are in the
  JS fixture. The JS-on reference is byte-identical in two runs.
- **Scores.** JS off: geometry 0.9096, size 0.9277, relative 0.9126, pixels
  0.7216, no missing or extra box (`fixtures/scores.json`). JS on: geometry
  0.8347, size 0.8627, relative 0.8459, pixels 0.9139, 15 boxes missing
  (`out/js-scores.json`; JS on shows the poster of the video and the controls,
  so the pixel score is higher).
- **The page runs in swb today.** With JS on, swb runs the page and all seven
  step groups without a console error, and the state records after the load and
  each group (focus, scroll, theme class, `aria-expanded`, the page transform,
  the bar widths, `data-animate`, `data-hero-ready`) are equal to Chromium's in
  every field except `--hero-delay` (900 ms in swb: virtual time starts the
  script at `performance.now() = 0`; Chromium's value is 859.5 ms). Screenshots
  of the groups differ from Chromium's in 5.2 % of the pixels in the first
  view (the hero title and the logo, below) and in 0.1 % after the scroll
  groups. Works item 4 (console) and the state side of item 3 hold already;
  the missing pieces are static CSS (items 1 and 2: the first view), the
  animations and the cost of a frame (items 2, 3 and 5).
- **Gaps, static CSS** (feature 2): `CSS.supports` in swb rejects 55 of the 689
  declarations (29 of the 180 properties) that the page's style sheets make and
  Chromium accepts. The owner's list had five properties; the full list is in
  section 4. `text-wrap: balance` (the hero title) changes the layout of the
  first view, `grid-template-rows: subgrid` (the sponsors) the layout further
  down, and the inline SVG logo has width 0 (a bug, not a missing property).
- **Gaps, APIs** (features 3 and 4): swb has none of `document.getAnimations`,
  `Element.getAnimations`, `Element.animate`, `Animation`, `CSSAnimation`,
  `CSSTransition`, `KeyframeEffect`, `AnimationEvent`, `document.timeline` and
  `DocumentTimeline`. The page's scripts do not call `element.animate()`; its
  libraries (Base UI) call `getAnimations()` and wait for `finished` before
  they close a popup, and only if the function exists.
- **Sampling method** (section 2): `js` rows that pause every animation and
  set `currentTime`, with a new probe flag `"animations": true`. 40 cases in
  three probe files (`css-animations`, `css-transitions`,
  `css-videojs-static`) and one in `css-videojs-support`; three Chromium runs
  of each file are equal. Under `--with-swb` 61 of 61 rows of the animation
  file and 69 of 72 rows of the transition file differ today.
- **First-view convention** (section 3): the references do not use
  Playwright's `animations: "disabled"`; they inject `animation: none
  !important; transition: none !important` after the load. Proposal: a swb
  option with that meaning for `compare`, `replay` and the tests of the other
  targets. Only github-swb (JS off: 50 running animations, 27 in view),
  ars-technica, osm-zagreb and videojs have animations or transitions in their
  first view.
- **Budgets** (section 5): `load` 1.34 s (3 x 0.430 s Chromium jitless task +
  0.051 s pipeline); swb loads the JS-on fixture in 0.32 s (0.55 s with the
  1.5 s settle). The GUI trace `videojs-page` shows what an animation would
  cost: a full repaint of the hero is 30 ms (31 ms of raster in the JS-off
  pipeline), pointer move to frame median 52 ms (p90 94 ms) for the moves that
  need a frame. Chromium needs 0.9 % of a core for the spinner, 2.1 % for the
  marquee. Proposed budgets: frame p90 16 ms with the hero in view, move to
  frame p90 50 ms, 0 % idle CPU when no animation is visible, at most 5 % with
  the spinner and 15 % with the marquee.
- **Plan changes** (section 7): feature 2 grows (about 30 properties and
  values, not 5) and gets a layout part; feature 5 (the cost of a frame)
  moves before feature 4, because every hover transition would cost 30 ms
  frames on this page; the exit test (6) keeps its items.

## 1. Baselines

**Chromium, JS off** (`fixtures/pages/videojs`): the page shows the poster
(placeholder) in the hero, the controls without a time (`0:00` twice), no
console message. 515 elements. `@media (scripting: none)` sets `animation:
none` on the three hero reveal elements, so the title is at its end state.

**Chromium, JS on** (`reference --js`): 542 elements. The first view has the
video frame (hls.js has loaded the stream's metadata: the time shows
`-0:35` and the timeline has a buffered bar), and the controls mute, settings,
picture-in-picture and fullscreen. Console: one PostHog warning ("Events
are being dropped because the user agent looks like a bot") and four
`ERR_FAILED` for URLs that the page prefetches on hover and focus and that a
first capture without steps lacks (`/blog`, `/docs/guides/installation/react`,
`DocSearchModal.*.js`, `built-bg-horizontal-line-black.svg`); the capture with
the step groups has them. 271 responses at the load (the HLS manifests and the
first segments of two renditions: 4.4 MB), 275 with the groups.

**swb, JS off** (`compare videojs`): geometry 0.9096, size 0.9277, relative
0.9126, pixels 0.7216; 515 of 515 elements, same tag sequence. The two
visible causes (`out/compare/videojs/report.html`): the inline SVG logo (a
link with `height: 100%` in a flex row) has width 0 and is not drawn (225 x 40
in Chromium), and the title is one left-aligned line instead of two centered
lines (`text-wrap: balance`, section 4), with the outlined words invisible
(`color: transparent` and `-webkit-text-stroke`).

**swb, JS on** (`compare --js videojs`): geometry 0.8347, size 0.8627,
relative 0.8459, pixels 0.9139, 15 boxes missing, 542 / 527 elements, tag
sequences differ in two blocks. Causes of the worst boxes: the logo (as
above), the `video` element (1180 x 150 px, the default size, against
1180 x 663.75 in Chromium, which knows the video's size from the stream), the
storyboard `img` of the timeline preview (0 x 0), the time `-0:35` and its
slider (the metadata of the stream), and the picture-in-picture and
fullscreen buttons with their icons (14 of the 15 missing boxes: swb has
neither feature; Video.js hides the buttons when the feature is missing). All but the logo belong to M14 and M15.
Console: nothing. The page asks for `POST /ph/i/v0/e/` (PostHog events, with
retries) at the load and in every group, which the fixture lacks; Chromium
sends none (PostHog recognizes headless Chromium as a bot and drops the events);
the fixture's replay answers with an error and PostHog retries (the retry URLs
differ). A replay rule that answers `/ph/i/v0/e/` with 204 would remove the
noise (it is the same kind of backlog item as the Matomo requests of OSM).

**Hero in swb without MediaSource.** `typeof MediaSource` is `undefined`, so
hls.js is not supported and no stream is loaded; `HTMLMediaElement.prototype`
has no `canPlayType` (an M14 item). The poster is drawn (the same picture as
Chromium's first frame, 0.17 % of the pixels differ with JS on against off in
Chromium), the title and the controls are drawn, there is no log line and no
exception. The first-view difference of 5 % is the title, the logo, the
outlined words and the missing buttons.

**Step groups** (`swb --steps`, one run, virtual time, 300 ms after each step;
records from `record.js` in the notes file; Chromium through `replay`):
the records are equal as said in the summary. The hover over a link or
control changes the style to the end state at once (no transitions), so the
group screenshots of Chromium (animations off) and swb are equal in the
groups with hovers. No group fails.

## 2. How a test samples an animation at a fixed time

Decision: **`js` rows that pause and seek** (docs/testing.md, "Probe
animations and transitions"):

```js
function seek(t) { for (const a of document.getAnimations()) { a.pause(); a.currentTime = t; } }
```

then `getComputedStyle()` reads, and `pixels` or `ink` rows of the same case
see the paused state. The probe tool got a case and file flag `"animations":
true`, which leaves the page's animations on (the tool otherwise injects the
kill sheet of section 3 and uses Playwright's `animations: "disabled"` for
screenshots, so no case could have seen an animation) and takes screenshots
with `animations: "allow"`.

Why not CDP's `Animation` domain or Chromium's virtual time:

- The expressions run unchanged in swb (`--eval`); they are the contract of
  `getAnimations()`, `pause()`, `currentTime`, `effect.getComputedTiming()`,
  `finished`, which features 3 and 4 must implement anyway and which the
  page's libraries call. CDP would sample something swb cannot be asked.
- No dependence on real time: three Chromium runs of every file give equal
  output (the cases that read real time print `~`).
- The rows are text values that the Rust tests of the features can compare
  with a tolerance for numbers.

Findings of the probes that the features must follow (all measured in
Chromium 148; the case in parentheses):

- Seeking: `currentTime` includes the delay; a paused animation with `both`
  fill shows its first keyframe before the delay (`hero-reveal`); an animation
  that is finished and has no fill is not in `getAnimations()`
  (`animation-api`, `transition-api-finished`).
- `CSSAnimation` has `animationName`, no `transitionProperty`; `CSSTransition`
  the reverse; `effect.target`, `effect.pseudoElement` (`""`), `getKeyframes()`
  with `computedOffset`, `getComputedTiming()` has no `phase` (`animation-api`).
- `cancel()` rejects `finished` with `AbortError`; a cancelled CSS animation
  is not restarted when `animation` is set again to the same name; `Promise.all
  (el.getAnimations().map((a) => a.finished))` (Base UI) resolves after the
  last transition ends, and rejects when an element gets `display: none`
  (`animation-api`, `transition-api-finished`).
- A custom property that is not registered does not transition and flips at
  once; `@property --x { syntax: "<percentage>" }` interpolates (`custom-
  properties`: the player's `--media-slider-fill`). The Tailwind
  `--tw-gradient-*` properties are unregistered.
- `transition-property` lists repeat or truncate to the longest list; the
  start events of two properties come in the order of the property list of the
  style, `left` before `opacity` here (`events-run-start-end`; the order
  `left`, `opacity` is not the order of the declaration).
- A transition needs a before-change style: an element inserted and changed in
  one task gets none, and an element that goes from `display: none` has none
  (`no-transition-on-insert`). `display` with `allow-discrete` keeps the old
  value until the end of the transition, `content-visibility` flips at the end,
  `transition-behavior: normal` ignores them (`transition-behavior-discrete`).
- Reversal: a transition at 40 % of 1000 ms that is reversed gets a duration
  of 400 ms and starts at 0.4; a reversal during the delay cancels (the value
  equals the end value); a change to a third value starts from the current
  value with the full duration (`reverse-running-transition`).
- `interpolate-size: allow-keywords` is inherited and lets `height: 0` to
  `auto` and `width: 40px` to `max-content` transition (25 px at half time
  for a 50 px box); without it nothing starts (`interpolate-size`).
- `linear()` serializes as `linear(0 0%, -24 20%, ...)` with explicit
  percentages; `steps(8, end)` computes to `steps(8)`; the `animation`
  shorthand serializes in the canonical order (`animation-shorthand`).

## 3. Animations in the first view, and the reference convention

**Mechanism** (docs/testing.md, "Animations in the references"):
`browser.stop_animations` adds a constructed style sheet with `*, ::before,
::after { animation: none !important; transition: none !important }` after the
load, the network idle, the 1 s settle and the fonts. Playwright's
`animations: "disabled"` does nothing then. So Chromium's reference is the
base style at the end of the load; `replay` and `compare --click` run their
steps with animations off too.

**Measured** (`swbtools animation-survey`, Chromium, same load and settle,
animations on; "running" = `document.getAnimations()` after the settle;
"declared" = elements whose computed style has an animation or a transition
of non-zero duration, in the first viewport in parentheses):

| Fixture (JS on) | elements | running | infinite | animation declared | transition declared | events since the start |
|---|---|---|---|---|---|---|
| ars-technica | 2709 | 1 (spin, in view) | 1 | 68 (1) | 3 (1) | 1 animationstart |
| bbc | 2754 | 0 | 0 | 0 | 15 (4) | none |
| craigslist-posting | 283 | 0 | 0 | 1 (0) | 5 (5) | 1 transition (background-color) |
| craigslist-search | 6006 | 0 | 0 | 128 (0) | 0 | none |
| github-swb | 1873 | 2 (1 in view) | 0 | 2 (1) | 47 (22) | 53 animationstart (skeleton loaders), all done |
| hacker-news | 821 | 0 | 0 | 0 | 0 | none |
| osm-zagreb | 522 | 0 | 0 | 2 (0) | 54 (20) | 5 transitions (box-shadow, border colors) |
| senko-net | 65 | 0 | 0 | 0 | 0 | none |
| videojs | 542 | 6 (3 in view) | 1 (marquee, below) | 6 (3) | 52 (31) | 6 animationstart, 3 animationend, 3 transitions |
| wikipedia-web-browser | 4169 | 0 | 0 | 0 | 50 (21) | none |

| Fixture (JS off) | elements | running | infinite | animation declared | transition declared |
|---|---|---|---|---|---|
| github-swb | 1732 | 50 (27 in view) | 48 | 7 (4) | 42 (20) |
| osm-zagreb | 379 | 5 transitions (all gone after 2 s) | 0 | 2 (0) | 46 (18) |
| videojs | 515 | 3 (0 in view) | 1 | 3 (0) | 40 (15) |
| the other six | | 0 | 0 | 0 | 0 to 39 |

(The JS-off GitHub page shows 48 infinite skeleton animations that scripts
remove with JavaScript on.) videojs with JS on: the three hero animations are
still in their delay phase at the settle time (`--hero-delay` is 865 ms after
the script, so the end comes at about 2 s); the two bar animations are
paused (they wait for `[data-animate]`); the marquee is infinite and below the
first view. Most targets declare transitions on links and buttons that never
start in the first view.

**Proposal** (for feature 3; nothing is implemented): a swb option `--animations
off` (automation setting `Browser.setAnimations`, GUI always on) with the
meaning of the kill sheet: for every element `animation-name` computes to
`none` and the transition duration to `0s` for the purpose of starting
animations, `getAnimations()` returns `[]`, no animation or transition event
fires, scripts that wait for `finished` see no animation (Base UI then closes
at once, as in the Chromium replay). It acts from the start of the document,
not at the end of the load. Reason: the point of the option is that the other
targets do not depend on the animation code; an option that started
animations and switched them off at the settle time would run that code on
every target and could change their scores. The difference to Chromium
(its animations run until the sheet is injected) shows only in a script that
reads `animationend` or `getAnimations()` during the load; none of the
targets does in the first view (the survey above; github's skeleton
animations end before the settle time).

`compare`, `compare --js`, `replay`-based tests, `swbtools perf` and
`js-survey` pass the option (`swb.py` builds all command lines). The tests of
feature 6 and the probe rows of animations run without it. The default of the
`swb` binary is "on" once feature 3 exists; until then the flag is not
needed.

## 4. Gaps

### Console, JS on

swb: no error and no warning in the load and in the seven step groups (only
the missing `POST /ph/i/v0/e/` of the replay). Chromium: the PostHog warning
and, from a fixture without the steps, the four failed prefetches. So item 4 of
"what works means" holds already for this page; it needs a test that keeps it
true after the features (the console of `--steps`).

### CSS that swb ignores (JS off and on)

From `css-videojs-support.json`: 689 declarations (180 properties) of the two
style sheets and the inline style, without `var()` values; swb's `CSS.supports`
rejects 55 declarations of 29 properties. Not accepted at all (also not as
inert properties: `CSS.supports('box-shadow', 'none')` is false, and
`getComputedStyle` has no value):

| Group | Properties | Effect on the page |
|---|---|---|
| Owner's list | `translate`, `rotate`, `scale` (17 declarations, `-90deg` to `scale: -1 1`), `box-shadow` (3), `text-shadow`, `-webkit-text-stroke` and `-webkit-text-stroke-color` (the values with `clamp()` and `var()` are not in the 689), `corner-shape: squircle` (the `corner-squircle` class of every button, link and card) | transforms of the controls and popups, shadows of cards and inputs, the outlined title and headings, the squircle corners |
| Layout | `text-wrap` (`balance`, `pretty`, `wrap`; the hero title is `text-balance`), `grid-template-rows: subgrid` (the sponsors section), `min-height: 1lh`, `-webkit-line-clamp` with `-webkit-box-orient`, `line-break: loose` | the title wraps in two lines of equal length in Chromium; the sponsors grid; clamped text |
| Paint | `isolation: isolate` (stacking), `-webkit-text-fill-color: transparent`, `-webkit-background-clip: text` (the shimmer of the search dialog), `all: unset` | stacking contexts, gradient text, the reset of buttons |
| Text | `font-variant-ligatures`, `font-variant-numeric: tabular-nums` (the time display) | digit widths |
| Scroll | `overscroll-behavior`, `overflow-anchor`, `-webkit-tap-highlight-color`, `-webkit-text-size-adjust`, `-webkit-font-smoothing` | none visible |
| Anchor positioning | `anchor-name`, `anchor-scope`, `position-anchor`, `inset: anchor(inside)` (the highlight of the menu items of the player) | only inside the settings menu |
| Animations | `animation` (6 declarations), `animation-play-state`, `interpolate-size` | features 3 and 4 |
| Transitions | `transition` with `linear()` (the inert grammar rejects it; times with `calc()` are rejected too, see `inert.rs`) | feature 4 |

Rules that swb drops with an invalid selector (debug log): only vendor
pseudo-elements (`::-webkit-scrollbar*`, `::-webkit-datetime-edit*`,
`::-webkit-search-*`, `::-webkit-media-text-track-container`, `:-moz-*`);
none matters for the first view. The `@keyframes` rules (9) and the `@property`
rules are parsed (CSSOM), `@starting-style` does not occur.

### APIs that scripts need

`Element.animate()`: not used by the page; React's view-transition code calls
it only inside `document.startViewTransition()`, which the page does not
call. `getAnimations()` (also `{subtree: true}`), `Animation.finished`,
`.cancel()`, `CSSTransition.transitionProperty` and `instanceof`/`in` checks
(`'transitionProperty' in t`): used by Base UI (popovers, menus, the slider).
Event types `animationstart`, `animationiteration`, `animationend`,
`transitionrun`, `transitionstart`, `transitionend`, `transitioncancel` (React
lists them; `AnimationEvent` is missing in swb, `TransitionEvent` exists).
`IntersectionObserver` drives the bars (exists). `matchMedia`, `requestIdle
Callback`, `ResizeObserver` exist. Missing for M14 and M15: `MediaSource`,
`HTMLMediaElement.canPlayType`, the Fullscreen and Picture-in-Picture
APIs. `document.fonts` (`FontFaceSet`) is also undefined (the page's own
scripts do not need it; the reference tools use it). `CSS.registerProperty` is not defined (the page
uses `@property` rules only).

### Scripts that start transitions

The two inline scripts of the page work in swb as written: the hero reveal
sets `--hero-delay` and `data-hero-ready` (swb: `hero: true`, delay 900 ms,
Chromium 859.5 ms), the spring sets `transition: none` and an inline
`transform` while the wheel acts at the end of the page, and `transition:
transform 600ms var(--ease-soft)` with an empty `transform` at the release. The
records show `pageTransform: none` at the end in both (the spring is finished
after the 1 s wait). Feature 4 needs the case of `inline-style-spring` of the
probe (a style flush between the two writes is what makes the transition
start; without a flush there is none).

## 5. Budgets, load time and GUI

**Load budget** (M9 method, idle machine, load 1.3): `js-survey videojs
--runs 3`: jitless `TaskDuration` median 0.430 s (runs 0.416, 0.430, 0.451),
script 0.182 s, 203 layouts, 268 style recalculations, 973 nodes; with JIT
0.427 s. `perf videojs` (JS off): 50.6 ms (parse 0.8, stylesheets 5.4, style
11.6, layout 1.5, display list 0.2, raster 31.0). Budget for the `load` event
= 3 x 0.430 + 0.051 = **1.34 s**. The Chromium task includes hls.js and the
video's start-up, which swb will not run until M15, so the budget is generous
until then. swb today (release, replay, JS on, `--virtual-time`, 7 runs, wall
time with process start and a screenshot): `--settle-ms 0` 0.316 to 0.330 s
(median 0.319); `--settle-ms 1500` 0.542 to 0.561 s (median 0.548); JS off
0.195 to 0.203 s.

**GUI trace `videojs-page`** (new, 19.24 s, window 1280 x 840; `gui-latency
videojs --js --trace videojs-page --window 1280x840`, own Xvfb, release
binary, idle machine, two runs with the same numbers):

| Input | Events | to batch end median / p90 / max (ms) | to frame median / p90 / max (ms) | no frame | no frame needed |
|---|---|---|---|---|---|
| move | 195 | 0 / 21 / 161 | 52 / 94 / 110 | 0 | 172 |
| press | 2 | 21 / 21 / 21 | 52 / 52 / 52 | 0 | 0 |
| release | 2 | 53 / 53 / 53 | 84 / 84 / 84 | 0 | 1 |
| wheel | 40 | 1 / 1 / 22 | 10 / 26 / 43 | 0 | 10 |

2.9 frames per second; CPU 8.4 % during the trace, 0.0 % in the 3 s after
(`CPU idle`); style runs 99 (131 ms), layout runs 54 (81 ms); 58 targeted
restyles (390 elements), 33 targeted after DOM changes, 7 full after DOM
changes (containers). Of 63 frames, render time: 28 frames of 30 to 33 ms (the
frames of the first view with the hero), 27 of 4.6 to 11 ms (scroll and small
changes), 5 of 14 to 28 ms, 2 of under 2 ms and one of 55 ms; compose 0.4 ms
median. So a frame that repaints the
hero costs 30 ms (the JS-off pipeline: raster 31 ms; a rounded 1180 x 664 image
alone costs 12 ms, a `backdrop-filter` bar over it 2 ms more, measured with
`--bench` on a page of that shape). The 52 ms median of the moves is one
restyle, one layout and one such frame. `idle-10s` on the fixture: 0.1
frames/s and 0.0 % CPU. Without animations the page costs nothing at rest;
with them every frame in the first view would cost 30 ms, which is 33 frames
per second at best and 100 % of a core.

**Chromium, CPU use** (`swbtools animation-cpu`, 8 s after a 2 s wait, share of
one core, all processes; the page has no `requestAnimationFrame` loop of its
own, which would force 60 frames per second):

| Target | running animations | CPU total | renderer / GPU |
|---|---|---|---|
| idle page | 0 | 0.2 % | 0.0 / 0.2 |
| player spinner (8 paths, `steps(8)`, 48 x 48 px) | 8 | 0.9 % | 0.5 / 0.4 |
| marquee (1158 x 15 px strip, 30 s) | 1 | 2.1 % | 1.0 / 1.1 |
| videojs fixture, scroll 0 (hero done, marquee below) | 1 | 1.8 % | 1.0 / 0.4 |
| videojs fixture, scroll 1500 (marquee and bars in view) | 1 | 10.7 % | 3.1 / 7.1 |

**Proposed budgets** (to be checked by feature 5 and the exit test; GUI, release
build, Xvfb, idle machine):

- Frame time (render + compose, `frame` lines of the frame log) during the
  animations of the page (the hero reveal, the hover transitions, the spring,
  the marquee): p90 at most 16 ms, maximum 33 ms, with the hero in view. Today
  30 ms. This needs a damage rectangle per frame: an animation of `transform`,
  `opacity` or `filter` repaints only the union of the old and the new
  rectangle of the element (feature 5; also needed for video in M14).
- Pointer move to frame (moves that need a frame): p90 at most 50 ms (the M12
  goal; today 94 ms), also with a hover transition running.
- Idle CPU (the 3 s after the trace, all threads): 0 % with no visible
  animation, also with the marquee outside the viewport, a `display: none`
  spinner and paused animations; at most 5 % of a core with only the spinner
  and at most 15 % with only the marquee in view (Chromium: 0.9 % and 2.1 %,
  with the GPU).
- Frames per second during a running animation: 60 (the frame loop asks for
  the next frame while an animation runs and for none after).
- `load`: at most 1.34 s (above); the animation code must not add more than
  50 ms to the load of a page without animations (the other targets: 0).
- Headless and tests: animations advance with virtual time; a test that
  seeks does not wait.

## 6. The probe files in swb (baseline of 2026-11-20)

`just probe --with-swb --swb target/release/swb FILE`: `css-animations.json`
61 of 61 rows differ (`document.getAnimations is not a function` in the 59
`js` rows, the pixels of `hero-reveal` and `shimmer`); `css-transitions.json`
69 of 72 differ (3 equal rows: values that do not depend on a transition);
`css-videojs-static.json`: 35 of 52 rows differ (17 equal: the boxes of the
shadow and stroke cases, which do not depend on the new properties, and the
white pixels of `#s-none` and `text-shadow #b`); `css-videojs-support.json`
the 55 declarations of section 4. The static file shows the visible size of
feature 2: `text-stroke` ink 0 for the outlined title, `box-shadow` and
`text-shadow` pixels white, `corner-shape` pixels square, the boxes of
`translate`, `rotate` and `scale` unmoved (`dx` of 10 to 20 px). The Rust tests of
the features will run the `js` rows against Chromium's values (as
`crates/automation/tests/osm_gaps.rs` does); the data files are made from
`swbtools probe --json`.

## 7. Proposed changes to the plan

1. **Feature 2 (static CSS) is larger than listed.** Beyond `translate`,
   `rotate`, `scale`, `box-shadow`, `text-shadow`, `-webkit-text-stroke` and
   `corner-shape`: `text-wrap` (`balance` and `pretty` change the title and
   many paragraphs), `isolation`, `-webkit-text-fill-color`,
   `-webkit-background-clip: text`, `all`, `grid-template-rows: subgrid`,
   `min-height: 1lh`, `font-variant-ligatures` and `-numeric`, `line-break`,
   `-webkit-line-clamp`, and the properties that parse and do nothing
   (`overscroll-behavior`, `overflow-anchor`, `-webkit-tap-highlight-color`,
   `-webkit-text-size-adjust`, `-webkit-font-smoothing`), plus the logo bug
   (an inline SVG with `height: 100%` in a flex item has width 0). Anchor
   positioning (`anchor-name`, `position-anchor`, `inset: anchor()`) only
   affects the settings menu after a click: out of M13 (backlog; the menu
   works without the highlight). Split it in two commits if one session is too
   little: 2a the properties that change the first view (transforms, shadows,
   stroke, corner-shape, `text-wrap`, `isolation`, `-webkit-text-fill-color`,
   the logo), 2b the rest.
2. **The scope of the "works" list is smaller in two places.** Item 4 (no
   console error) holds today; feature 6 only has to keep it. Item 1 for JS
   off needs feature 2 only (no animation is involved: the reveal has
   `animation: none` when scripting is off).
3. **Move feature 5 (the cost of a frame) before feature 4**, or at least its
   damage rectangles: every hover on this page repaints the hero for 30 ms, so
   a hover transition of 200 ms is a run of 6 such frames and the GUI budget
   cannot be met by feature 4 alone. Feature 3 (animations) has the same
   problem for the hero reveal; its acceptance should not include the frame
   budget, only the correctness of values and the frame loop.
4. **Feature 3 includes the option of section 3** (`--animations off`) and the
   change of `swb.py`, so that the scores of the other targets stay stable.
5. **`getAnimations()` and `finished` are needed for correctness, not only
   for the probes**: Base UI waits on them to close popups. The first
   implementation of `finished` must settle (resolve at the end, reject with
   `AbortError` on cancel), or the appearance menu never closes after
   animations exist.
6. **`inert.rs` departures to remove in feature 4:** times with `calc()`
   (the skin writes `calc(var(--media-duration-controls) / 2)`) and `linear()`
   as a timing function (the slider thumb).
7. **A replay rule** for `POST /ph/i/v0/e/` (204) would silence the retries of
   the analytics; backlog for the tools.
8. **Hostile cases for features 3 and 4** (hostile-page set, to add with the
   features): 10,000 animated elements, 100,000 `@keyframes` names, an
   `animation` list of 10,000 entries, `steps(1000000)`, `linear()` with
   100,000 points, an `animationend` listener that sets `animation` again
   (loop), a `transitionend` listener that changes the property back (ping-pong),
   `getAnimations()` on a document with 100,000 animations, an infinite
   animation of a 30,000 px element in a full-page screenshot, and custom
   property animations that cascade 1,000 levels deep.

## 8. What was done and what was not

Done: fixtures (JS off committed, JS on local), their references and scores,
the step file and its capture with steps, `probe` flag `animations` (with
tests), four probe files (three equal Chromium runs each, `--with-swb` once),
`animation-survey` and `animation-cpu` (with tests), the trace `videojs-page`,
the survey of the 19 fixtures, the swb run of the step groups, the CSS
support list, the GUI traces and the budgets, the notes and the proposals.
THIRD_PARTY_NOTICES.md and `fixtures/pages/README.md` list the new fixture.

Not done: no swb code was changed; the exit test and the data files for the
Rust tests (the features make them); the hostile cases (section 7, item 8);
the swb option of section 3. The files that were working material (the logs
and screenshots under `out/v1/`, the generators of the probe files) are in the
session's scratchpad and not kept; the generators are described in the notes
of the session, the probe files are the product.
