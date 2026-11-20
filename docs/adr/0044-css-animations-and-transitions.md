# ADR 0044: CSS animations and transitions

- Status: accepted
- Date: 2026-11-20

## Context

M13 (roadmap, "M13: CSS transitions and animations (videojs.org)") adds CSS
animations (feature 3), a cheaper frame (feature 4) and CSS transitions
(feature 5). The diagnosis ([videojs-diagnosis.md](../videojs-diagnosis.md))
lists what the page needs: 9 `@keyframes`, about 170 transition
declarations, `getAnimations()` and `finished` for Base UI, the frame and
idle CPU budgets, and the option that turns animations off for the
comparisons of the other targets.

Today swb runs no animation. `@keyframes` is kept for the CSSOM only
(`swb_css::KeyframesRule`), the `animation-*` properties do not exist, and
`transition-*` are inert text values (`properties/inert.rs`). Style and
layout run on demand; a rendering update (`page/rendering_update.rs`) runs
only for scripts, and only with JavaScript on. Every style change drops the
layout (ADR 0041, 0042), every frame rebuilds the display list and
rasterizes the whole page (ADR 0040, "Alternatives"): a repaint of the hero
of videojs.org costs 30 ms.

This ADR decides the design of features 3 to 5. The behaviour comes from
the specifications below and from black-box measurements in Chromium 148:
the probe files `tools/probes/css-animations.json` and
`tools/probes/css-transitions.json` (the cases of feature 1, and the cases
that this ADR adds: `interp-*`, `easing-precision-duration`,
`easing-progress-precision`, `script-overrides`, `get-animations-order`,
`hostile-limits`, `events-order-many-properties`; three equal Chromium
runs).

Specifications: [Web Animations 1](https://drafts.csswg.org/web-animations-1/),
[CSS Animations 1](https://www.w3.org/TR/css-animations-1/) and
[2](https://drafts.csswg.org/css-animations-2/),
[CSS Transitions 1](https://drafts.csswg.org/css-transitions-1/) and
[2](https://drafts.csswg.org/css-transitions-2/),
[CSS Easing 2](https://drafts.csswg.org/css-easing-2/),
[CSS Values 4](https://www.w3.org/TR/css-values-4/#combining-values),
[CSS Color 4](https://www.w3.org/TR/css-color-4/#interpolation),
[CSS Transforms 1](https://www.w3.org/TR/css-transforms-1/#interpolation-of-transforms)
and [2](https://drafts.csswg.org/css-transforms-2/#matrix-interpolation),
[Filter Effects 1](https://www.w3.org/TR/filter-effects-1/#interpolation-of-filters),
[CSS Backgrounds 3](https://www.w3.org/TR/css-backgrounds-3/#box-shadow),
[CSS Properties and Values API 1](https://www.w3.org/TR/css-properties-values-api-1/#animation-behavior-of-custom-properties),
[CSS Cascade 5](https://drafts.csswg.org/css-cascade-5/#cascade-sort),
[HTML "update the rendering"](https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering),
[CSS Values 5 `interpolate-size`](https://drafts.csswg.org/css-values-5/#interpolate-size).

## Decision

### 1. Where the model lives

**The style crate holds the model** (`crates/style/src/animation/`): the
timing model of Web Animations (timeline, animation, keyframe effect), the
easing functions, the interpolation of computed values, the resolution of
`@keyframes`, and the CSS rules that create, update and cancel CSS
animations and transitions. Reasons: interpolation works on
`ComputedStyle` values and needs the compute step of the cascade (section
3); the timing model is small, plain code that the cascade calls. The
engine only drives it (time, frames, restyles, paint); the web crate only
exposes it.

**One `DocumentAnimations` per document**, owned by `DocumentState` (frame
documents have their own). It holds:

- the document timeline: its current time in ms from the document's time
  origin (`DocumentTimeline`,
  [Web Animations §4.3.1](https://drafts.csswg.org/web-animations-1/#document-timelines));
- the animation records in a table by `AnimationId` (a `u64` that grows per
  document and is never reused), and an index from the owning element
  (node and pseudo-element) to its records in composite order;
- the pending animation event queue (section 6);
- the keyframe sets: one resolved, sorted set per `@keyframes` rule (per
  property: offsets, specified declarations, keyframe easing), built at the
  first use and dropped with the stylist.

A record holds: the kind (CSS animation with its name and its index in
`animation-name`; CSS transition with its property, start and end values,
reversing-adjusted start value, reversing shortening factor and transition
generation), the timing (delay, duration, iteration count, direction,
fill), the play state (start time, hold time, pending play or pause, the
CSS Animations 2 override flags of section 7), the phase and iteration of
the last update (for events), and the state of the `ready` and `finished`
promises (pending, resolved, rejected, with a generation that grows when a
promise is replaced).

**Lifetime.** A CSS animation record lives while its name is in the owning
element's computed `animation-name` list (also when finished, so that
`play()` can restart it and its identity stays). A CSS transition record
lives until it finishes or is cancelled and the next rendering update has
dispatched its last event. Then the record is *released*: the engine
removes it and lists its id for the bindings.

**JavaScript objects.** The web crate keeps one wrapper per live record:
`DocState::animations: HashMap<AnimationId, Gc<Object>>`, traced strongly,
like the sheet and rule wrappers of `cssom_sheets.rs`. `getAnimations()`
looks up the id and makes the wrapper only when it is missing, so the same
record gives the same object on every call (`animation-api`). The entry
goes when the engine reports the id as released; the wrapper then lives on
only if a script holds it. Its host data holds the document key, the id
and a snapshot of the last state (play state, current time, timing,
target), so that the members of a released animation answer from the
snapshot. `effect` is a `KeyframeEffect` wrapper per animation (a
`[SameObject]` slot in the wrapper's data); `document.timeline` is one
`DocumentTimeline` object per window. The bindings reach the model through
new `EngineHost` methods with no-op defaults (the animations of an element
or document, the state of one id, the commands of section 7).

Alternatives: a new crate (it would need the private parts of
`ComputedStyle` and the cascade); the engine (it would put style code
outside the style crate); wrappers without a map (a new object per call,
which breaks `a === el.getAnimations()[0]`).

### 2. Time

**The document timeline advances at the rendering update.** The new step
"update animations and send events" of `Page::run_frame` (section 6) sets
the timeline's current time to the frame time minus the time origin. Style
computations between two updates use that time, so all reads in one task
agree.

**A rendering update for animations does not need scripts.** Today
`runs_rendering_updates()` is false without a script runtime. Animations
add a source of rendering updates that does not depend on JavaScript:
`Page::frame_time()` and `frame_work_due()` include the next time that
the animations need (section 8). With JavaScript off the update runs the
animation step and dispatches no events.

**Catch-up.** When the page runs no rendering update for longer than a
frame (nothing visible to animate, section 8), a style computation for a
paint, a screenshot or a script query first advances the timeline to the
last grid time at or before now (the 60 Hz grid of ADR 0028, "The rendering
update"). Events found then are queued and dispatched at the next update.

**Clocks.** The timeline reads the page clock (`PageClock`, ADR 0028
"Clocks"):

- GUI: real time.
- Headless with `--virtual-time`: virtual time. The blocking loops
  (`--settle-ms`, `page.waitForIdle`, the `wait` step of `--steps`) jump
  the clock to the next due time, which includes the animation updates of
  section 8. In headless mode nothing paints between steps, so an
  animation needs updates only at its event times (start, iteration, end,
  the end of a delay): a 30 s infinite marquee costs one update per
  iteration, not 60 per second. `--now` fixes the wall time only; page time
  starts at 0. The steps of `--steps` other than `wait` run the loop for
  1 µs and do not move virtual time.
- A page whose only work is running animations counts as idle
  (`Page::is_idle`), as a `requestAnimationFrame` loop does.

**Determinism.** A screenshot is taken at the page time of the clock, and
the timeline is at the grid time at or before it. In virtual time both are
the same in every run, so screenshots and `compare` are deterministic. The
probe rows and the Rust tests seek (`pause()`, then `currentTime`): a
paused animation's output does not depend on the clock. The references of
the other targets run with `--animations off` (section 10).

### 3. The cascade

**Origins** ([Cascade 5 §6.1](https://drafts.csswg.org/css-cascade-5/#cascade-sort)):
transition declarations win over everything; animation declarations sit
between important author declarations and normal author declarations. So a
property with an `!important` author or user-agent winner is not animated;
a transition wins over `!important`.

**Animated values enter the compute step as computed values.** The
cascade of an element (`cascade()` and `compute()` in `cascade.rs`) gets an
optional animation input: for each animated property, the two keyframes
around the current progress (their specified declarations from the
keyframe set) and the progress, and for each transitioning property the
transition's start and end computed values and its progress. After the
winners are picked, a property with an animation input and no important
winner, or with a transition input, takes its value from the input instead
of its winner, at the same place in the order of `compute()`:

- custom properties first: animated custom properties enter the custom
  property map before `var()` is substituted in other declarations, so a
  `width: var(--len)` follows an animated `--len` (`interp-custom-properties`:
  `len`, `kvu`);
- `font-size` and `font-family` in their step, so that `em` and the
  lengths of keyframes use the animated font size (`interp-lengths-numbers`:
  `fs`);
- the other properties in the order of `LonghandId::ALL`.

The keyframe declarations are computed with the element's
`ComputeContext` (the same `apply_declaration` path), so `var()`, `em`,
`currentcolor` and percentages in keyframes resolve as for the element
(`hero-reveal`; `interp-custom-properties`: `kv` follows a change of
`--w1`). A missing `0%` or `100%` keyframe uses the element's value
without the animation (the underlying value; `spin-implicit-from`). The
two endpoint values are then interpolated (section 4). The result goes
through the computed-value rules of its property: clamps (`opacity` in
0 to 1, non-negative lengths: `interp-lengths-numbers`: `op`, `wn`), the
rounding of integers, the snapping of border widths (`bw`: 0.3 px computes
to 1 px).

**Inheritance.** The animated value is the element's computed value, so
children inherit it like any other (`interp-lengths-numbers`: `cl`, `fs`;
`interp-custom-properties`: the child of `len`).

**Effect stack.** All animations of one element apply in composite order
([Web Animations §5.4.2](https://drafts.csswg.org/web-animations-1/#the-effect-stack));
with the composite mode `replace` only the last effect that has a value
for a property counts. Transitions come before animations in composite
order, but sit in a higher origin, so a transition of a property wins over
an animation of it.

**Style change events, before- and after-change styles**
([Transitions 1 §3](https://drafts.csswg.org/css-transitions-1/#starting)).
In swb a style change event is each style computation that the engine
runs for changes (a targeted restyle of ADR 0041/0042, or a full restyle
with a previous map). Animation updates are not style change events:
changes of computed values that come from animations start no transition.

- The *before-change style* of an element is its style in the map that the
  restyle starts from (the current map for state changes, the stale map of
  ADR 0042 for DOM changes, the previous map of a full restyle). For a
  property that a running animation or transition drives, the value is
  taken at the current timeline time.
- The *after-change style* is the new cascade with the animation input
  and without the transition input.
- Only restyled elements can start, update or cancel transitions: an
  element that the pruned walk does not restyle has no new style. Children
  of a changed element are restyled (ADR 0041), so an inherited change
  reaches the transitions of children.
- An element without a before-change style (inserted in the same task,
  coming out of `display: none`, the first styles of a document) starts no
  transition (`no-transition-on-insert`).
- An element whose new style has transitions (non-zero combined duration
  for a property that changed) gets a second compute with the transition
  input; one compute more per element with an actual transition.

**What each frame costs in style.** An animation update marks the owning
elements of the animations whose output can have changed (active, not
paused, or a seek by script). The next style flush runs ADR 0041's pruned
walk with them as the marked elements, with the animation input, and
compares the new styles with the old ones. Selector matching runs again for
each marked element (about 19 µs per element on GitHub, ADR 0041); a cache
of the matched rules of animated elements is an option for feature 4 if
its measurements show matching. The walk restyles one level of children
of a changed element as ADR 0041 requires; a transform or opacity change
stops there.

**`display: none`.** An element without a computed style (itself or an
ancestor `display: none`) has no CSS animations: its records are cancelled
(`animationcancel`, `transitioncancel`), as in `events-cancel`.

### 4. Interpolation

Interpolation works on computed values
([Values 4 §3](https://www.w3.org/TR/css-values-4/#combining-values)). A
pair of values that cannot be combined is *discrete*: in an animation it
flips at progress 0.5 (`interp-custom-properties`: `un` at 499 and 500 ms;
`interp-filters`: `f-mismatch`); a transition of such a pair starts only
with `transition-behavior: allow-discrete`, and then also flips at 0.5,
except for the special cases below. Progress outside 0 to 1 (from
`cubic-bezier()` or `linear()` overshoot) extrapolates, then the
property's clamps apply.

| Type | Rule | Probe |
|---|---|---|
| Lengths, percentages, `calc()` | Linear per part; a length and a percentage combine into `calc(a% + bpx)` | `interp-lengths-numbers` (`ti`), `interp-background-*` |
| Numbers | Linear; integers round half toward +infinity (5.5 gives 6) | `fw`, `zi` |
| Keywords against values (`auto`, `normal`, `line-height` number against length) | Discrete | `wa`, `lh`, `lhm` |
| Colors | See below | `interp-colors` |
| Transform lists | See below | `interp-transforms`, `interp-transform-decomposition` |
| `translate`, `rotate`, `scale` | Per component on their computed values (Transforms 2 §5, [individual transforms](https://drafts.csswg.org/css-transforms-2/#individual-transforms)); `none` is 0, 0 deg and 1; percentages stay (`translate: 5px 25%`) | `interp-individual-transforms` |
| `filter`, `backdrop-filter` | Function by function when one list is a prefix of the other in kind; the shorter list is padded with each function's initial value (`drop-shadow` with a transparent shadow); `none` is the other list with initial values; other pairs are discrete | `interp-filters` |
| `box-shadow`, `text-shadow` | Shadow by shadow: color, offsets, blur, spread; the shorter list is padded with `transparent 0 0 0 0` of the other's `inset`; a pair that differs in `inset` makes the whole list discrete | `interp-box-shadows`, `interp-text-shadows` |
| `background-position`, `background-size` | Per layer and component as lengths; a pair with `auto`, `cover` or `contain` on one side only is discrete; lists of different length repeat to their least common multiple (Values 4) | `interp-background-position`, `interp-background-size` |
| Registered custom properties | By syntax: `<length>`, `<number>`, `<percentage>`, `<length-percentage>`, `<integer>`, `<angle>`, `<time>`, `<resolution>`, `<color>`, `<transform-list>` interpolate; `+` and `#` lists interpolate item by item when equal in length, else discrete; `*`, idents, strings, URLs, images discrete. The computed tokens are parsed by the syntax, interpolated and written back as tokens | `interp-custom-properties` |
| Unregistered custom properties | Discrete; no transition without `allow-discrete` | `un`, `custom-properties` |
| `visibility` | `visible` for the whole interval when one end is `visible` | `vi`, `vi2` |
| `display`, `content-visibility`, `overlay` | With `none` (`hidden`, `none`) on one end, the other value for the whole interval; in transitions only with `allow-discrete` ([Display 4](https://drafts.csswg.org/css-display-4/#display-animation), [Contain 2](https://drafts.csswg.org/css-contain-2/#content-visibility-animation)) | `ds`, `cv`, `transition-behavior-discrete` |
| Sizes with `interpolate-size: allow-keywords` | A length against `auto`, `min-content`, `max-content` or `fit-content` interpolates: the computed value keeps the keyword, the length and the progress, and layout resolves it (the keyword's size in px, then linear) | `interpolate-size` |
| Other properties of feature 2a (`corner-shape`, `text-wrap`, `isolation`); everything not listed | Discrete | |

**Colors** ([Color 4 §12](https://www.w3.org/TR/css-color-4/#interpolation)),
measured:

- Both ends legacy (`rgb()`, `hsl()`, `hwb()`, named, hex, `transparent`):
  sRGB with premultiplied alpha, channels rounded to 8 bits (`red` to
  `blue` at 0.25 is `rgb(191, 0, 64)`; `rgba(255,0,0,1)` to
  `rgba(0,0,255,0)` at 0.5 is `rgba(255, 0, 0, 0.5)`; `hsl()` mixes in
  sRGB, not HSL).
- One end not legacy (`oklch()`, `lab()`, `color()`): Oklab with
  premultiplied alpha, also for two `oklch()` values (Chromium serializes
  `oklab(...)`).
- `currentcolor` resolves to the element's (animated) `color` before the
  mix (`c-current`, `interp-color-current`, `sh-current`).
- Decision for swb: the space follows Color 4 as above. For that, a
  computed `oklch()`, `oklab()`, `lab()`, `lch()` or `color()` value keeps
  its space and its float coordinates (a `Wide` color of `color_mix.rs`),
  as Chromium's `getComputedStyle` shows it. Today swb computes these to
  8-bit sRGB; feature 2b changes that (Tailwind 4 writes every color of
  the page as `oklch()`, so without it the hover transitions would mix in
  sRGB). The table holds one entry per distinct declared color, and a full
  table falls back to 8-bit sRGB as it does for `color-mix()`.
- The result of an interpolation is stored as an 8-bit `Rgba`, never in
  the process-wide table (2^20 entries that never shrink; one entry per
  frame would fill it). So paint matches Chromium, and only the text of
  `getComputedStyle` during an animation between non-legacy colors
  differs (`rgb(...)` against Chromium's `oklab(...)`). Tests compare such
  values as 8-bit sRGB with a tolerance of 1 per channel. A `color-mix()`
  endpoint (a `Wide` color) is not legacy and mixes in Oklab.

**Transform lists**
([Transforms 1 §10](https://www.w3.org/TR/css-transforms-1/#interpolation-of-transforms),
[Transforms 2 §13, §14](https://drafts.csswg.org/css-transforms-2/#matrix-interpolation)),
measured:

- `none` is a list of identity functions of the other list.
- Lists whose functions match pair by pair (same function, or two
  functions with a common primitive such as `translateX` and `translate`)
  interpolate per function; the shorter list is padded with identity
  functions when it is a prefix (`t-prefix`). Angles interpolate as
  numbers (`rotate(0)` to `rotate(720deg)` at 0.25 is 180 deg; `scale(1)`
  to `scale(-1, 1)` at 0.5 is `scale(0, 1)`).
- Other pairs interpolate as matrices. swb's transforms are 2D; the 2D
  decomposition that matches all seven `interp-transform-decomposition`
  cases: translation; x scale = length of the first row; skew = dot
  product of the rows, the second row made orthogonal and its length the y
  scale, skew divided by it; if the determinant is negative, negate the x
  scale (with its row and the skew) when m11 < m22, else the y scale;
  angle = atan2 of the first row. Interpolate each part linearly, the
  angle the short way when the two differ by more than 180 deg (no
  replacement of 0 by 360: identity to `matrix(-1,0,0,-1,0,0)` turns
  +90 deg at 0.5). Recompose as translate, rotate, skew, scale. This is
  the 3D decomposition of Transforms 2 reduced to 2D, with the flip rule
  of the 2D algorithm of Transforms 1.
- A matrix pair with percentages (for example `translate(-50%)`
  against `rotate()`) cannot be resolved at computed-value time: the
  computed value keeps both lists and the progress, and the matrix is made
  when the reference box is known. `getComputedStyle` resolves it with the
  layout box (it shows a `matrix()`, `t-percent`).

How feature 2a represents `translate`, `rotate`, `scale` and the shadows
is its own choice; this section fixes only the results on their computed
values.

### 5. Easing

[CSS Easing 2](https://drafts.csswg.org/css-easing-2/): the keywords are
their `cubic-bezier()` forms; `animation-timing-function` is the keyframe
easing of each interval (also per keyframe, `easing-in-keyframes`), the
effect easing of a CSS animation is `linear` (so `getComputedTiming()
.progress` is linear: `easing-progress-precision`); the
`transition-timing-function` is the easing of the transition's effect.

- **`cubic-bezier()`**
  ([§3](https://drafts.csswg.org/css-easing-2/#cubic-bezier-easing-functions)):
  solve x(t) = p with Newton's method from t = p, falling back to
  bisection on [0, 1], until |x(t) − p| < 1e-9, then return y(t); for p
  outside [0, 1] extend with the tangent at the end point. Chromium's
  values agree with an exact solution to 6 significant digits (the digits
  it prints) and do not depend on the duration (100 ms to 100 s,
  `easing-precision-duration`), so a fixed precision is right.
- **`steps(n, position)`**
  ([§4](https://drafts.csswg.org/css-easing-2/#step-easing-algo)) with the
  jump terms and the *before flag*; `n` is clamped to 2^31 − 1 (Chromium
  accepts `steps(2147483648)`). Evaluation is O(1).
- **`linear()`**
  ([§2](https://drafts.csswg.org/css-easing-2/#linear-easing-function-output)):
  at parse time the missing input positions are filled (first 0, last 1,
  each at least the largest before it, runs spread evenly); evaluation
  finds the segment by binary search and extrapolates beyond the first and
  last points. The computed value serializes with explicit percentages
  (`linear(0 0%, -24 20%, ...)`). No limit on the number of points
  (Chromium accepts 100,001: `hostile-limits`); the memory counts as style
  data.
- All timing math is `f64`. Tests compare numbers with a relative
  tolerance of 1e-5 and an absolute one of 1e-4 (section 12).

`transition-*` leave `inert.rs` in feature 5 and get typed computed
values, with `calc()` in times and `linear()`, which the inert grammar
rejects today.

### 6. Events

**When.** A new step of the rendering update, after the media queries and
before the animation frame callbacks (HTML's order; `run_frame` and
`frame_rendering_steps`), runs
[update animations and send events](https://drafts.csswg.org/web-animations-1/#animation-frame-loop):

1. Set the timeline's time; update each record; compare its phase and
   iteration with the last update and queue the events of CSS Animations 2
   and CSS Transitions 2 §5.1 (`event-dispatch`) with their scheduled
   event times. Finished animations resolve their `finished` promise.
2. A microtask checkpoint (promise reactions run before the events).
3. Take the queue, stable-sort it by scheduled event time (unresolved
   first), then by composite order, and dispatch each event at its target.

Changes in a style change event (a cancel by `display: none` or by a
removed name, a reversed transition) queue their events at once; they are
dispatched at the next update (`events-cancel`). A frame update never
dispatches more than one `animationiteration` per animation, also when it
skips several iterations.

**Order** (composite order,
[Animations 2 §2.2](https://drafts.csswg.org/css-animations-2/#animation-composite-order),
[Transitions 2 §4.1](https://drafts.csswg.org/css-transitions-2/#animation-composite-order)):
all transitions before all animations; within each, owning elements in
tree order (an element's pseudo-elements after it: `::marker`, `::before`,
`::after`); transitions of one element by transition generation, then by
property name in code point order; animations by their position in
`animation-name`. One animation's events stay together (`transitionrun`
then `transitionstart` of `background-color`, then those of `color`, ...).
Measured in `events-order-many-properties`: the order does not depend on
the order of the script's changes or of the declarations (the diagnosis's
"`left` before `opacity`" is code point order), and `getAnimations()`
returns the same order (`get-animations-order`). Three cancels from one
change (`events-cancel-and-reverse`: a reversal on `e1`, `display: none`
on `e2`, `transition: none` on `e3`): Chromium dispatches `e2` first in
every run, and `e1` and `e3` in either order (one of three runs). swb uses
the rule above; the scheduled time of a cancel by `display: none`
(possibly unresolved, which sorts first) is for feature 5 to settle with
that row.

**Members.** `AnimationEvent` is new (IDL, `Other::Animation`,
`EventInterface::Animation` for trusted dispatch); `TransitionEvent` gets
`EventInterface::Transition`. `bubbles` and `cancelable` are true (as
measured), `pseudoElement` is `""` or `"::before"` etc. with the
originating element as the target. `elapsedTime` in seconds:
`animationstart` and `transitionrun`/`transitionstart` the interval start,
max(min(−delay, active duration), 0); `animationend` and `transitionend` the
interval end; `animationiteration` (current iteration) × duration;
`*cancel` the active time at the cancel
([interval start](https://drafts.csswg.org/css-animations-2/#interval-start)).
Without a script runtime nothing is dispatched; an event type without a
listener on its path skips the path (`events/listened.rs`).

### 7. The API

In scope (Web Animations §4.5 and §6, CSS Animations 2 §6.1, CSS
Transitions 2 §6.1):

- `document.getAnimations()`, `Element.getAnimations({subtree})` (the
  subtree includes pseudo-elements), in composite order; only *relevant*
  animations (not finished without fill, not idle).
- `Animation` (an `EventTarget`): `play()`, `pause()`, `cancel()`,
  `finish()`, `currentTime` and `startTime` (get and set), `playState`,
  `pending`, `ready`, `finished`, `effect`, `timeline`, `id`, `replaceState`
  (`"active"`), `playbackRate` (read only, 1).
- `KeyframeEffect`: `target`, `pseudoElement`, `getKeyframes()` (with
  `computedOffset` and the keyframe easing), `getTiming()`,
  `getComputedTiming()` (no `phase`), `composite` (`"replace"`).
- `CSSAnimation.animationName`, `CSSTransition.transitionProperty`;
  `DocumentTimeline.currentTime`.
- Promises: `ready` resolves when a pending play or pause completes, at the
  next update; `finished` resolves when the animation finishes (at once for
  `finish()`), and `cancel()` rejects it with an `AbortError`
  `DOMException` and replaces it
  ([§4.5.11](https://drafts.csswg.org/web-animations-1/#the-current-finished-promise)).
  A rejected `finished` is marked as handled, so it reports no
  `unhandledrejection`; `swb_js` gets an operation for that. Base UI's
  `Promise.all(el.getAnimations().map((a) => a.finished))` must settle
  (`transition-api-finished`).

**Script overrides** ([Animations 2 §2](https://drafts.csswg.org/css-animations-2/#animations)),
measured in `script-overrides`:

- After `pause()` or `play()` by script, `animation-play-state` no longer
  plays or pauses the animation (both directions).
- After `cancel()`, the animation stays cancelled while its name stays in
  `animation-name`, also when other `animation-*` values change; a new name
  creates a new animation, and the old name again a new object.
- Changes of `animation-duration`, `-delay` and the other longhands update
  the timing of the existing object; a current time that a script set
  stays (`currentTime` 3000 after a duration change).

**Out of scope, and how it fails:**

- `Element.prototype.animate` does not exist: `'animate' in el` is false
  and a call throws `TypeError: el.animate is not a function`, so feature
  tests take their fallback (the page never calls it).
- The constructors of `Animation`, `KeyframeEffect` and `DocumentTimeline`
  throw `TypeError: Illegal constructor` (the interfaces exist for
  `instanceof`).
- `updateTiming()`, `setKeyframes()`, `reverse()`, `updatePlaybackRate()`,
  `persist()`, `commitStyles()` do not exist. `composite`,
  `iterationComposite` are read-only `"replace"`; the property
  `animation-composition` is not supported (`CSS.supports` false).
- The playback events `finish`, `cancel` and `remove` of `Animation` are
  not fired (backlog); `onfinish` and `oncancel` do not exist.

### 8. The frame loop

**Which animation needs time.** Each record answers "the next time my
output or my events can change" from its timing:

- none when idle, paused, finished, or in the before phase without
  `backwards` fill and with nothing to show until the delay ends (then the
  end of the delay);
- the end of the delay, of each iteration and of the active phase (event
  times);
- while active: the next grid frame for a continuous easing; for `steps()`
  the next step boundary in time (the spinner: 8 updates per second); an
  interval whose steps are shorter than a frame counts as continuous.

**Which animation can change pixels.** An animation needs paint frames
only if its owning element has a box that intersects the viewport: its
visual bounds from the last display list (the builder records the bounds
of the boxes of animated elements, with transforms and filter outsets)
intersected with the viewport. Elements without a box: not visible, except
`display: contents` with an animated inherited property (treated as
visible). Paused and finished animations need nothing; elements under
`display: none` have no animations.

**Scheduling.** `Page::frame_time()` gets the earliest of: the next event
time of any animation, and, when an animation is visible and continuous,
the next grid frame (or the next step boundary). In virtual time the clock
jumps there; in the GUI `next_task_instant()` gives the
`ControlFlow::WaitUntil` deadline, as for timers. At the update, records
whose output changed mark their elements (section 3), and
`DocumentState::paint_pending` becomes true when a visible one did, so
`Page::frame_outdated()` asks the shell for a frame; the frame pacing of
ADR 0040 applies. A frame that is late runs once at the current grid time
(frames are never queued).

**Budgets** (diagnosis section 5): 0 % idle CPU with no visible animation
(no wake-ups but event times); spinner at most 5 %: 8 updates and 8 small
frames per second; marquee at most 15 %: 60 frames per second of one
restyled element, a display list and a damage raster of a 1158 x 15 strip
(feature 4).

### 9. The cost of a frame (feature 4)

**Classification.** The restyle (tick or change) reports, per changed
element, whether only *paint properties* changed. A table in the style
crate lists them: `opacity`; `transform`, `translate`, `rotate`, `scale`,
`transform-origin`; `filter`, `backdrop-filter`; the color properties
(`color`, `background-color`, the border, outline, text decoration,
`fill`, `stroke`, `-webkit-text-stroke-color`, `-webkit-text-fill-color`);
`box-shadow`, `text-shadow`; `background-position`, `background-size`;
`clip-path`, `mask-position`; `visibility` is not in it. A change between
`none` and a value of `transform` (and the individual transforms),
`filter` or `backdrop-filter` is not a paint change: it changes the
containing block of fixed and absolute descendants. Any other change drops
the layout as today.

**Paint-only path.** When every change is a paint change, the layout
stays:

1. *Fragment styles.* Fragments hold their own `Arc<ComputedStyle>`. One
   walk of the fragment tree replaces the style of each fragment whose
   element's style changed, copying only the changed paths
   (`Arc::make_mut`). Anonymous boxes and text fragments inherit: when an
   inherited paint property changed (`color`, the text colors and shadows),
   their styles are derived again from the new parent style.
2. *Overflow.* A transform change moves the scrollable overflow (layout
   includes transformed bounds, `scroll.rs`): the stored overflow of the
   scroll containers above the changed box and of the root is computed
   again from the fragments; the geometry caches that include transforms
   (`getBoundingClientRect`, client rects, hit testing) are dropped.
3. *Display list.* Built again in full from the fragment tree (0.2 ms on
   the videojs JS-off page, `perf`). Patching items in place is not done:
   items carry no element identity, and groups appear and disappear with
   values (opacity 1 to 0.5 adds a `PushOpacity`).

The check mode (debug assertions, as `restyle_check.rs`) runs a full layout
and display list after each paint-only frame and compares the lists item
by item; it is the oracle for the table above.

**Damage rectangles.** The new display list is compared with the one of
the last rendered frame, in paint order, with the transform and clip
stacks of both sides:

- equal items add nothing; `Text` and `Image` compare by pointer first,
  `Image` also by a content generation that the `ImageSource` gives (video
  frames of M14 bump it without a new list; the `PaintStamp` gets the
  generations of the drawn media, so that `frame_outdated()` sees them);
- a differing drawing item adds its old and new device bounds;
- a differing group (`PushTransform`, `PushOpacity`, `PushFilter`,
  `PushMask`, `PushSvgClip`) adds the old and new bounds of the whole
  group (the bounds of `finish_groups`, mapped to device pixels) and skips
  to the matching pops;
- where the sequences stop matching in kind, the longest common prefix and
  suffix are kept and everything between is damaged (old and new);
- damage under a filter group grows by the filter's forward reach
  (`css_output`); damage that overlaps a `BackdropFilter` item painted
  later adds that item's whole rectangle.

The result is a list of device rectangles, rounded out to whole pixels
plus 1 px for anti-aliasing, merged to at most 8 (else their bounding box).
A full raster runs instead when the damage covers more than half of the
viewport, after a scroll of the viewport (its offset moves every pixel), a
viewport or scale change, or the first frame.

**Region raster.** `rasterize_region(list, pixmap, params, rects)` in the
paint crate: for each rectangle, fill it with the page background and run
the rasterizer with its visible area set to the rectangle and a hard
device clip, so that nothing outside changes. The rasterizer already
culls items by `visible` (`raster.rs`, the strips) and sizes its layers to
the visible part, so the cost follows the damage: the hero's rounded
1180 x 664 image (12 ms in full) is drawn only where the damage crosses
it. Two rules keep the result equal to a full raster: filter layers read
their backward reach around the visible area (as now), and a transform
layer keeps the pixel grid of a full raster (its origin does not move with
the visible area), because its resampling depends on it. A test compares
region raster with full raster bit for bit on the page fixtures, with
random damage rectangles and with the frames of the probe animations.

**Reuse.** The GUI keeps the page pixmap (`Composer`, ADR 0040); a frame
rasterizes only the damage into it, so unchanged regions keep their pixels.
Headless screenshots keep the full raster. A raster cache of layers
(drawing a transformed or translucent group once and compositing it per
frame) is not part of M13: damage rectangles meet the budgets on the
traces of the diagnosis by estimate; the cache is the next step if a
measurement of feature 4 misses them.

**Video (M14).** A video frame changes the content generation of one
`ImageRef::Node`; the list stays, the damage is the item's bounds, and the
region raster draws the frame with its clips.

### 10. `--animations off`

Meaning: from the start of each document, as if a style sheet
`*, ::before, ::after { animation: none !important; transition: none
!important }` of the highest author precedence applied to every element
and pseudo-element. So the `animation-*` and `transition-*` longhands
compute to their reset values (`getComputedStyle` shows `none`, `0s`), no
CSS animation or transition is created, `getAnimations()` returns `[]`, no
animation or transition event fires, and scripts that wait for `finished`
find nothing to wait for (Base UI closes at once, as in Chromium's replay).
The cascade applies it as a fix-up after the winners (a flag of the
document, set from the page when the document is created, like the
JavaScript flag).

- Command line `--animations on|off` for every mode; automation method
  `page.setAnimations {enabled}` (named like `page.setJavaScript`, not the
  diagnosis's `Browser.setAnimations`); both apply to the next document.
  The default is on, also in the GUI.
- `swb.py`: `replay_options` (so `compare`, `compare --js`, `perf`, the
  snapshot) and `probe_command` for cases without `"animations": true`
  (Chromium injects the same sheet there) pass `--animations off`. The
  Rust tests that compare fixtures with Chromium's references (replay and
  step tests) set it through the page configuration. `hostile_command`,
  `serve_command` and the tests of M13 keep animations on. `js-survey`
  runs only Chromium.
- Differences to Chromium's references: Chromium injects its sheet after
  the load, so scripts during the load see animations there; none of the
  targets reads them (diagnosis section 3).

### 11. Limits for hostile content

Chromium has no limits for these (`hostile-limits`: 10,000 animations on
one element, `linear()` with 100,001 points, `steps(2147483648)`), so swb
sets only limits that keep it alive:

- **Records**: at most 100,000 live animations and transitions per
  document; beyond, no new record is created (one `warn!`). About 300 bytes
  each.
- **`steps()`**: `n` clamped to 2^31 − 1; scheduling treats steps shorter
  than a frame as continuous.
- **`linear()`**: no point limit; binary search; style memory.
- **Iterations**: the current iteration is computed, not counted (an
  infinite 1 ms animation seeked to 1e9 ms is iteration 1e9 at once); one
  `animationiteration` per update.
- **Per frame**: an update is O(live records); a restyle is O(marked
  elements and their changed children). A slow frame lowers the frame rate;
  input and tasks run between frames.
- **Listener loops**: events are dispatched only in rendering updates, so
  an `animationend` listener that restarts its animation, or a
  `transitionend` ping-pong, runs at most once per frame (60 per second in
  real time; one frame of virtual time per round in headless mode, where
  the settle time bounds it). A zero-duration transition does not start.
- **Memory**: keyframe sets once per rule; transitions keep two computed
  values; wrappers only for animations that scripts asked for.

The diagnosis's hostile cases (section 7, item 8) go into the hostile-page
set with features 3 and 5: 10,000 animated elements; 100,000 `@keyframes`
names; an `animation` list of 10,000 entries; `steps(1000000)`;
`linear()` with 100,000 points; the `animationend` restart loop; the
`transitionend` ping-pong; `getAnimations()` with 100,000 animations; an
infinite animation of a 30,000 px element in a full-page screenshot;
animated custom properties through 1,000 levels of inheritance.

### 12. Tests

- **Probe rows as Rust test data**: `swbtools probe --json` of the two
  probe files gives `crates/automation/tests/data/css-animations.json` and
  `css-transitions.json`; tests in the style of `osm_gaps.rs` run each case
  with `run_probe_file` and compare the `js` values, with a list of
  deliberate departures (case, query, swb's value, reason).
- **Tolerances**: the text without numbers must be equal; numbers compare
  with a relative tolerance of 1e-5 or an absolute one of 1e-4 (Chromium
  prints 6 significant digits; `2.22045e-16` and `6.12323e-17` are both
  0); color channels of 0 to 255 by 1; pixels by 2 per channel (the probe
  tool's rule). Rows that read real time print `~` already.
- **Pixels at seek times**: the `pixels` rows of `hero-reveal`, `shimmer`
  and the transition cases run with `probe --with-swb`, and the exit test
  (feature 6) samples the page's animations at fixed seek times.
- **Unit tests** in the style crate: the timing model (phases, fill,
  direction, iterations, the before flag), each easing against exact
  values, each row of the table of section 4, the 2D decomposition against
  the seven decomposition cases.
- **Frame loop**: the next-time function per kind of record (spinner: 8
  per second; offscreen and paused: none); a headless test that `--settle-ms`
  with only an offscreen infinite animation costs its iteration events
  only; the GUI budgets with `gui-latency` (`videojs-page`, `idle-10s` on
  the fixture and on spinner and marquee pages).
- **Paint**: the paint-only oracle and the region raster oracle of section
  9 on the page fixtures; `frame_oracle.rs` and `frame_outdated.rs` get
  animation sources.
- **Hostile cases** of section 11, with the tool's time bounds.

### 13. Order of the work

Each step ends with its tests; the roadmap decides how the steps form
commits.

- Feature 3: the `animation-*` longhands, the shorthand and their CSSOM
  forms; the timing model and the easing functions (unit tests); the
  interpolation of section 4 (unit tests from the probe values); the
  keyframe sets and the animation input of the cascade (section 3); the
  timeline, the animation step of the rendering update without scripts,
  virtual time and `--animations off` with `swb.py`; the API, the
  promises and the events; the frame scheduling of section 8 in headless
  mode and in the GUI.
- Feature 4: the classification and the paint-only path with its check
  mode; then the display list diff, the region raster with its oracle, and
  the GUI path; then the budgets.
- Feature 5: the typed `transition-*` longhands (out of `inert.rs`); the
  before- and after-change styles in the targeted and full restyles; start,
  cancel and reversal; `allow-discrete` and `interpolate-size`; the
  events; `CSSTransition`.

## Consequences

- Every style computation can now depend on time; the timeline time is an
  input of style and enters `paint_pending` and the frame scheduling.
- The cascade and `compute()` get an animation input, the restyle a new
  source of marked elements, and the engine a paint-only path and a region
  raster. Missed invalidations show as stale pixels; the check modes of
  sections 3 and 9 and the oracles guard them.
- The rendering update runs without scripts when animations need it.
- Departures: during an animation between non-legacy colors,
  `getComputedStyle` shows `rgb()` (Chromium: `oklab()`; the pixels are
  equal); transforms are 2D; no `element.animate()`, no playback events, no
  composite modes.
- `--animations off` keeps the scores of the other targets independent of
  this code; the scores of videojs compare with the same convention as
  Chromium's references.

## Alternatives

- **A separate animation layer after the cascade** (compute the style,
  then overwrite the animated fields): cheaper to build, but `em`,
  `currentcolor`, `var()` of animated custom properties and the inheritance
  inside the element would see the unanimated values (the `fs` and `kvu`
  cases fail).
- **Keyframe values computed once and cached per element**: wrong when the
  element's context changes (`kv` after a change of `--w1`), and the cache
  needs its own invalidation; computing two declarations per animated
  property per frame is cheap.
- **Timeline in real time at every query** (no grid): reads in one task
  would disagree, and virtual time would not be the same across runs.
- **60 Hz rendering updates for every running animation**: simple, but
  breaks the idle budget with offscreen or paused animations, and spends
  60 updates per second on a stepped spinner.
- **Patching the display list** instead of building it again: needs
  element identity on items and group bookkeeping; the build is 0.2 ms.
- **A layer cache (compositor)**: the largest saving for a moving layer,
  but a second paint path; deferred until measurements need it.
