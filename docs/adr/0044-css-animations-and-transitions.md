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
runs for changes (a targeted restyle of ADR 0041/0042 after a change of
the element states or of the DOM, or a full restyle with a previous map).
Animation updates are not style change events for the animated elements:
a targeted restyle for the marked owners starts no transition for them,
and changes of computed values that come from their own animations start
none. Their children are different: a child whose parent's style changed
in the restyle has a style change event, and its after-change style
inherits from the parent's style without the values of the animations and
transitions of the parent and its ancestors (measured in Chromium 148:
such a child starts its own transition one frame after its parent's,
towards the parent's end value; section 20 has the rules as implemented,
and the departure for lists that cannot name an inherited value).

- The *before-change style* of an element is its style in the map that the
  restyle starts from (the current map for state changes, the stale map of
  ADR 0042 for DOM changes, the previous map of a full restyle), or a newer
  style that a script's read computed for it since (section 20). For a
  property that a running animation or transition drives, the value is
  taken at the current timeline time; a transition that completed since
  counts with its end value.
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
| Registered custom properties | By syntax: `<length>`, `<number>`, `<percentage>`, `<length-percentage>`, `<integer>`, `<angle>`, `<time>`, `<resolution>`, `<color>`, `<transform-list>` interpolate; `+` and `#` lists interpolate item by item when equal in length, else discrete; `*`, idents, strings, URLs, images discrete. The computed tokens are parsed by the syntax, interpolated and written back as tokens; a registered `<integer>` rounds half away from zero, `currentcolor` takes the element's color, a `<transform-list>` keeps its function names (section 14) | `interp-custom-properties`, `registered-*` |
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
every run, and `e1` and `e3` in either order (one of three runs). Settled
in feature 5 (section 20): a cancel by `display: none` (of the element or
an ancestor, or a removed element) has an unresolved scheduled time and
sorts first; other cancels are scheduled at the timeline time of the style
change; at one scheduled time the cancels come before the other events
(the `transitionrun` and `transitionstart` of the reversed `e1`), then
composite order (`e1` before `e3`, the other order that Chromium gives).

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
  `DOMException` and replaces it (a `ready` read after `cancel()` stays
  pending while the animation is idle, as in Chromium; section 16)
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
only if what it can paint intersects the viewport: the painted bounds of
its owner's subtree (with transforms, shadows, outlines and filter
outsets), or of the nearest ancestor that clips it for an animation that
moves or grows the painted area, found after each display list, scroll
and resize; an animation of a layout property counts as visible (the rule
and its implementation: section 16). Elements without a box are not
visible; a `display: contents` owner has the bounds of its children.
Paused and finished animations need nothing; elements under `display:
none` have no animations.

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
crate lists them (the table as implemented, and what the check mode took
out of it, is in section 17): `opacity`; `transform`, `translate`,
`rotate`, `scale`, `transform-origin`; `filter`, `backdrop-filter`; the
color properties (`color`, `background-color`, the border, outline, text
decoration, `fill`, `stroke`, `-webkit-text-stroke-color`,
`-webkit-text-fill-color`); `box-shadow`, `text-shadow`;
`background-position`, `background-size`; `clip-path`, `mask-position`;
the outline and the decoration lines of text. `visibility` is not in it.
A change that moves an element between "has a transform" (`transform`,
`translate`, `rotate` or `scale` is not `none`) and "has none", or
between "has a filter or backdrop filter" and "has none", is not a paint
change: it changes the containing block of fixed and absolute
descendants. Any other change drops the layout as today.

**Paint-only path.** When every change is a paint change, the layout
stays:

1. *Fragment styles.* Fragments hold their own `Arc<ComputedStyle>`, and
   elements with equal values share one object, so a fragment is matched
   to a changed style by its owner element (or pseudo-element) and the
   old object together. One walk of the fragment tree replaces the style
   of each fragment whose owner's style changed. Anonymous boxes and the
   text of controls inherit: when an inherited paint property changed
   (`color`, the text colors and shadows), their styles are derived again
   from the new parent style.
2. *Overflow.* A transform change moves the scrollable overflow (layout
   includes transformed bounds, `scroll.rs`): the stored overflow of the
   scroll containers above the changed box and of the root is computed
   again from the extent that layout kept of each container and its
   descendants; the geometry caches that include transforms
   (`getBoundingClientRect`, client rects, hit testing) are dropped.
3. *Display list.* Built again in full from the fragment tree (0.2 ms on
   the videojs JS-off page, `perf`). Patching items in place is not done:
   items carry no element identity, and groups appear and disappear with
   values (opacity 1 to 0.5 adds a `PushOpacity`).

The check mode (debug assertions, as `restyle_check.rs`) runs a full layout
and display list when the display list of a paint-only frame is built, and
compares the fragment trees box by box and the lists item by item; it is
the oracle for the table above.

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
Headless screenshots keep the full raster. Damage rectangles alone missed
two budgets (section 18); feature 4c (section 19) adds a raster cache whose
entries are exact by construction (the coverage of clips, kept between
frames), composited groups for running animations of `transform` and
`opacity` (their content is drawn once into a layer at a fixed scale and
drawn with the animated transform and opacity in each frame, the one rule
that changes pixels while an animation runs), and a window buffer that gets
only the rows that changed.

**Video (M14).** A video frame changes the content generation of one
`ImageRef::Node`; the list stays, the damage is the item's bounds, and the
region raster draws the frame with its clips.

**As implemented** (feature 4b, section 18): the diff also looks ahead (at
most 64 items per side) for the place where the lists meet again before it
falls back to the common suffix; a rectangle of damage that touches a
`backdrop-filter` box holds all of it; the region raster does not shrink the
target or the layers to the rectangle (tiny-skia's anti-aliasing depends on
the extent of the pixmap) but draws all rectangles in one pass into a
scratch pixmap of the target's size, culling the items and limiting only the
drawing that is the same for each pixel; an incomplete raster (a budget)
makes the next frame full.

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
  `serve_command` (with its `animations` flag; `compare --click` passes
  false) and the tests of M13 keep animations on. `js-survey`
  runs only Chromium.
- Differences to Chromium's references: Chromium injects its sheet after
  the load, so scripts during the load see animations there; none of the
  targets reads them (diagnosis section 3).

### 11. Limits for hostile content

Chromium has no limits for these (`hostile-limits`: 10,000 animations on
one element, `linear()` with 100,001 points, `steps(2147483648)`), so swb
sets only limits that keep it alive:

- **Records**: at most 100,000 live animations and transitions per page,
  all its documents together (`RecordRoom`, shared by the documents of a
  page; fix round of feature 5); beyond, no new record is created (one
  `warn!` per document). About 300 bytes each; a transition keeps the
  computed values of its one property besides.
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
  0); color channels of 0 to 255 by 1, the alpha by 1/255 (feature 6;
  before, 0.5/255); pixels by 2 per channel (the probe tool's rule). Rows that read real time print `~` already.
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
  events; `CSSTransition`. As done (section 20): all but the interpolation
  of `interpolate-size: allow-keywords`, which needs `calc-size()` in
  layout and is a task of its own.

### 14. Done in feature 3a

Feature 3a built the model (sections 1, 4, 5, 11 and 12) without running
an animation: no record, no timeline, no change to the output of the
cascade. Measured in Chromium 148 with the new probe file
`tools/probes/css-animations-3a.json` (18 cases, three equal runs).

**Module.** `crates/style/src/animation/`: `time.rs` (`<time>` and
`<number>`), `easing.rs`, `timing.rs`, `properties.rs` (the longhands and
shorthands; their texts in `properties/text.rs`), `keyframes.rs` and
`interpolate/` (`mod.rs` for lengths, numbers and keywords, `color.rs`,
`transform.rs`, `effects.rs` for the individual transforms, filters and
shadows, `custom.rs` for registered custom properties). The math
functions of times, numbers and the percentages of `linear()` go through
the one parser of `crate::parse::length` in its context-free domain
(`parse_context_free_math`: times as a type, IEEE-754 arithmetic,
`infinity` and `NaN` kept, evaluated in `f64` at parse time). The module
is `pub`, so that its API is not dead code before 3b uses it.

**The animation lists.** One optional shared record per style,
`ComputedStyle::animations: Option<Arc<AnimationStyle>>` (`None` while all
values are initial, as `inert`): `ComputedStyle` grows from 2016 to 2024
bytes. Each longhand is an `Arc<[T]>` of computed items (durations in
seconds, `None` for `auto`; easings in their computed form; iteration
counts with `f64::INFINITY`). The lists keep their lengths, as Chromium's
`getComputedStyle` shows them; `AnimationStyle::entry(i)` repeats them to
the number of names (CSS Animations 1 §4.2). A third group of the
`longhands!` macro (`ids.rs`) holds the twelve longhands, as the inert
group does.

**The API for 3b.**

- `AnimationStyle::of(style)` and `entry(i)`: the name, the `Timing`
  (delay and duration in ms, effect easing `linear`), the keyframe easing
  (`animation-timing-function`) and the play state of animation `i`.
- `Timing::compute(local_time)`: phase, active time, current iteration
  (computed, not counted), directed progress, the before flag and the
  iteration progress (Web Animations 1 §4.6 to §4.10).
- `Stylist::keyframes(name, media, ranks)` (and `keyframes_in(name, env)`):
  the `KeyframeSet` of the rule that wins, built at the first lookup of
  that rule and cached in the stylist (dropped with it). The stylist shares
  the rules with the sheets (`CssRule::Keyframes(Arc<KeyframesRule>)`), so
  a build copies none.
- `KeyframeSet::interval(index, progress, default_easing, before)`: the
  two keyframes around the progress (`None` is an implicit 0% or 100%
  keyframe: the underlying value) and the progress between them after the
  keyframe easing (Web Animations 1 §5.3.4, interval endpoints). The
  keyframes keep their specified declarations, so 3b computes them with
  the element's `ComputeContext` (section 3).
- `interpolate(keyframes, from, to, progress, out)`: the value of the
  property between two computed styles, with the clamps of the property;
  `out.color` must already be the animated `color` (it resolves
  `currentcolor`). It returns false for a custom property: 3b calls
  `interpolate_custom(custom, name, from, to, current, progress)` where the
  custom properties are resolved (`custom` is the element's map, which knows the
  registrations).

**Measured rules that the sections above did not have.**

- Chromium's `animation` shorthand sets eight longhands and resets
  `animation-timeline`, `animation-range-start` and `animation-range-end`
  (eleven in `element.style`); `animation-composition` is not part of it.
  A layer leaves out the duration as `auto` in `element.style`; the
  resolved `animation` writes `0s` only for an explicit duration or before
  a delay, and writes a part that is initial when the name is one of its
  keywords (`ease ease`, `normal normal`; also a string name: `'none'`
  gives `none "none"`, the fill mode first). A timeline or range that is not

  initial empties both serializations.
- `animation-composition` parses and computes (`CSS.supports` is true, as
  in Chromium), with no effect; section 7 said it would be unsupported.
  `animation-timeline` and the ranges also parse with no effect;
  `animation-trigger` (which Chromium also parses) does not.
- Keyframes merge by offset and keyframe easing: two `50%` keyframes with
  different `animation-timing-function` stay apart. `all`, the
  `animation-*` and the `transition-*` properties are dropped from
  keyframes.
- `cssText` is parsed again by swb's cascade (inline styles and rule
  blocks). When the layered lists differ in length, Chromium's `cssText`
  writes an `animation` that does not parse back to the same longhands; swb
  writes the longhands there (`getPropertyValue('animation')` is
  Chromium's).
- `steps()` prints its count with six significant digits in the computed
  value (`steps(1e+06)`) and as an integer in the specified value.
- `@keyframes` rules (case `keyframes-conditions`): a rule inside
  `@container` counts whatever the condition (a name cannot depend on an
  element); inside `@supports`, only when the condition is true.
  `@-webkit-keyframes` is an alias: the same rule type in the CSSOM, its
  `cssText` keeps the prefix (`@-webkit-keyframes wk { ... }`, also for
  `@-WEBKIT-KEYFRAMES`); other prefixes are dropped. An `@keyframes` rule
  of a name beats every `@-webkit-keyframes` rule of it, whatever the
  order and the layers; then the layer rank and the order decide.
  Offsets are `f64` (`33.3%` is 0.33299999999999996, `0.0000001%` is
  9.999999999999999e-10).
- The range of numbers (case `number-ranges`). Chromium keeps the
  values as `f64` but saturates every literal to the `f32` range:
  `1e40s` and `1e999s` are `3.40282e+38s` (also `calc(1e999s)` and the
  arguments of `cubic-bezier()`, `linear()` and the iteration count). A
  math function whose result is infinite or NaN stays valid: its
  specified text is `calc(infinity * 1s)`, `calc(-infinity)`,
  `calc(NaN * 1s)`, and its computed value is ±1.79769e+308 (the largest
  `f64`) or 0 for NaN, then the clamps of the property (`calc(-1s/0)` is
  `0s` as a duration, `-1.79769e+308s` as a delay; `calc(-1/0)` and
  `calc(NaN)` are 0 as an iteration count, `calc(1/0)` is `1.79769e+308`,
  not `infinite`). In `cubic-bezier()` and `linear()` the arguments
  computed so are written as numbers (`cubic-bezier(0, 1.79769e+308, 1,
  1)`, `linear(0 0%, 1 1.79769e+308%)`); an x outside 0 to 1 stays
  invalid. The count of `steps()` keeps its math function in the
  specified text (`steps(calc(2.5))`, `steps(calc(infinity))`) and
  computes to the rounded count clamped to 1 to 2^31 − 1 (NaN computes to
  `steps(0)`); a math count is checked only with `jump-none`
  (`steps(calc(1), jump-none)` is invalid, `steps(calc(-1))` is valid).
  swb's values are finite everywhere: `Easing::evaluate` saturates at the
  largest `f64`, the interpolated lengths, numbers, percentages and
  matrices at the largest `f32` (a percentage at 1e36 as a fraction, so
  that its text is finite), and `AnimationStyle::entry` gives the timing
  model finite times in ms, so it never adds opposite infinities.
  One departure: `element.style.setProperty()` reaches swb's cascade as
  the specified text, so an infinite argument of `cubic-bezier()` or
  `linear()` set that way computes to `3.40282e+38` (the literal
  `1.79769e+308` saturates); from a style sheet it is Chromium's value.

**A matrix pair with percentages.** When the rests of two transform lists
interpolate as matrices and one has a percentage (`translate(-50%)`
against `rotate(45deg)`), the computed value ends with
`TransformFunction::Interpolated(Arc<InterpolatedTransform>)`: both rests
and the progress. Layout (`style_transform`) and `getComputedStyle`
(`function_matrix`) call `InterpolatedTransform::matrix(width, height)`:
both rests as matrices in the reference box, then `interpolate_matrices`
(case `matrix-percent`, four pairs, equal to Chromium's matrices). Such a
value can be the start of a later interpolation (a transition from an
animated value), on either side: `function_matrix` and `list_matrix`
accept it (with the box), and `identity_of` pads it with an identity
matrix. The nesting is bounded: `InterpolatedTransform::nodes` counts the
interpolated values inside, itself included, and a pair that would hold
more than `MAX_NODES` (32) is discrete (it flips at 0.5). Making the
matrix resolves each node once, so a page that chains animations or
interrupts a transition again and again costs at most 32 matrix
interpolations per element and frame. The model has no 3D transforms;
the decomposition stays 2D.

**Registered custom properties** interpolate by their syntax
(`interpolate/custom.rs`, cases `interp-custom-properties` and
`registered-custom`, `registered-tl-1` to `-4`, `registered-currentcolor`
of `css-animations-3a.json`): both computed values are matched against the
terms of the syntax; when both match the same term of an interpolable type
(`<length>`, `<number>`, `<percentage>`, `<length-percentage>`,
`<integer>`, `<angle>`, `<time>`, `<resolution>`, `<color>`,
`<transform-list>`), the tokens are parsed, interpolated with the
functions of `interpolate/` and written back as `registered.rs` computes
them (`50px`, `180deg`, `rgb(191, 0, 64)`); `+` and `#` lists go item by
item when equal in length. Rules measured in Chromium 148:

- A registered `<integer>` rounds half away from zero (0 to -5 at 0.5 is
  `-3`), unlike `order` and `z-index` (half up).
- `currentcolor` interpolates with the element's color (`interpolate_custom`
  takes it as `current`): with black text, `currentcolor` to `red` is
  `rgb(64, 0, 0)` at 0.25. A `color-mix()` is discrete, whatever it holds.
- A `<transform-list>` keeps the function names
  (`interpolate/custom/transform_list.rs`): `none` against a list is
  discrete; the functions pair by position (the shorter list is padded
  with the identity of the function it meets); the same name keeps its
  name (`translateX(2.5px)`), `translate()`, `translateX()` and
  `translateY()` together give `translate(x[, y])` (`y` is left out when it
  is 0), `scale()`, `scaleX()` and `scaleY()` give `scale(x[, y])` (`y` is
  left out when equal to `x`), `skewX()` against `skewY()` or `skew()` does
  not pair; two `matrix()` functions give `matrix(...)`; from the first
  position that does not pair, the rests of both lists interpolate as one
  matrix written `matrix3d(...)` (even for a pair that is 2D), and a rest
  with a percentage is discrete.

Discrete: `*`, keywords, `<custom-ident>`, `<string>`, `<url>`, `<image>`,
`<transform-function>`, two terms, lists of different lengths, and a
`<transform-list>` pair as above. Between two non-legacy colors the text
is `rgb()` (Chromium `oklab()`), as for other properties (section 4).
Departures: a `<transform-list>` with a 3D function is discrete (swb has no
3D model; Chromium writes `translate3d()`, `rotate3d()` and so on, and
swb's registration does not accept `matrix3d()` or `perspective()` at
all); the token text of a tiny number has no exponent (`6.12323e-17` is
written out as a decimal; a trigonometric rest that Chromium prints as `2.22045e-16`
differs in the noise digits).

**Not done in 3a.**

- *The before flag of a keyframe easing*: `KeyframeSet::interval` takes it
  from the caller; whether Chromium passes the effect's before flag to a
  `steps(start)` keyframe easing in the before phase is for 3b to measure.
- *The members of `CSSKeyframesRule`*: `cssRules`, `appendRule()`,
  `deleteRule()`, `findRule()` and `CSSKeyframeRule` (case
  `keyframes-rule-members`; no target page uses them).

### 15. Done in feature 3b

Feature 3b runs the animations: sections 1 (the records), 2, 3, 8 without
the visibility test and 10, and the part of section 7 that seeking needs.
Measured in Chromium 148 with the new probe file
`tools/probes/css-animations-3b.json` (4 cases, three equal runs).

**Where the records live.** `swb_style::animation::DocumentAnimations`
(`records.rs`), owned by `DocumentState` in a `RefCell`: the style passes
update the records while they borrow the stylist and the DOM. A record
holds the name, its position in `animation-name` and its occurrence among
the entries of that name (a duplicate name makes a second record), the
`Timing`, the keyframe easing, the start time, the hold time, a pending
play or pause, the CSS and script play-state flags, the phase, iteration
and current time of the last update (for 3c's events) and the step period
for the scheduling. Keyframe sets are not kept in the record: the cascade
looks them up in the stylist at each sync, so a new stylist takes effect.
The record limit (100,000 per document, one `warn!`) acts at creation.

**Play states.** Web Animations with a playback rate of 1. A new animation
plays with a hold time of 0 and a pending play, resolved at the next
update (start time = the update's timeline time); `animation-play-state:
paused` at creation holds 0 with a pending pause. `pause()` takes the hold
time at once (the timeline does not move within a task), the pending flag
stays until the update; a seek completes a pending pause. `play()` of a
finished or idle animation starts again at 0. Relevance
(`getAnimations()`): the before or active phase, or in effect (measured,
`fill-modes`: a paused animation past its end without a `forwards` fill is
not listed, though the specification's "not finished" would list it).

**The cascade input.** `cascade()` computes the base style from the
winners, then (with an animation target) syncs the owner's records with
the base style's `animation-*` lists and, if an animation has a value now,
runs `compute()` again with the input (`cascade/animated.rs`). A longhand
or custom property with an `!important` winner is not animated; an element
that has animations finds them by running the important phase of its
cascade again alone (`ImportantPhase::collect`), so other elements keep no
record of them. The animated custom properties replace their
winners as declarations of their interpolated values before
`compute_custom_properties`, so `var()` everywhere follows them; their
keyframe values are computed against the base style's map with the
element's font size (`em` of a registered `<length>`), and
`interpolate_custom` gets the base style's `color` as `currentcolor`. The
longhands take their animated values at their place in the order
(`font-family`, `font-size`, `line-height`, then `LonghandId::ALL`): each
keyframe is computed with the element's `ComputeContext` on a copy of the
style so far, an implicit keyframe copies the base style's value, then
`interpolate_longhand`. An element with an animation name is not shared by
the style sharing cache (`animation-*` values without a name, as in a
reduced-motion reset, make no animation and keep the sharing). Pseudo-elements: `::before` and `::after`
with a box, and `::marker`. The style of one element for a query
(`compute_element_styles_animated`) syncs the records of the chain's
elements too, so a script that sets `animation-*` and reads the style at
once sees the new timing; below a `display: none` element it only reads
them, and a pseudo-element without a box only reads them.

**`display: none`.** Measured (`interp-lengths-numbers`, `ds`): an element
whose base style is `display: none` keeps the animations whose keyframes
animate `display` (Chromium shows `block` from `from { display: block }`);
it has no others. Descendants of a `display: none` element have no style
and lose their records (`sweep_animations` after each style pass: an owner
without a style or without an animation name).

**The before flag of a keyframe easing** (section 14, "Not done in 3a"),
measured in `keyframe-easing-before-flag`: Chromium gives a `steps()`
keyframe easing the left limit in the before phase of a forwards iteration
(`steps(2, start)` with a delay and `backwards` is 0 before the delay, not
0.5), and also at the start of the active interval when the effect has a
delay and a backwards fill (0 at `currentTime` = delay, while the same
animation without a fill gives 0.5 there); never in the after phase (a
reversed or alternate iteration that ends at progress 0 with `forwards` is
0.5, where the effect's before flag of Web Animations §4.10 would give 0).
`Timing::keyframe_before_flag` has this rule; the effect's own easing
(linear for CSS animations) keeps the specification's flag.

**Time.** `DocumentAnimations::update(page time)` sets the timeline to the
page time minus the document's time origin (the page time when the
`DocumentState` was made), resolves pending plays and pauses and marks
the owners whose phase, iteration or (while active) current time changed.
`run_frame` runs it first in each rendering update, with or without
scripts. The catch-up (to the last grid time at or before now, when the
last update is older) is requested in `take_document_state` (each task),
`Page::parts` (each engine-side layout or query), `update_layout` and
`render_in_strips`, but runs only when something reads the styles or the
animations (`DocumentAnimations::settle`, from `update_style`,
`query_element_style`, `paint_pending` and the animation queries of
scripts): a task that reads neither costs no pass over the records. Marked owners make `paint_pending` true; the next
style flush (`update_style`, a script's `getComputedStyle`) runs the
targeted restyle with them (`restyle_animated`, counted with the state
restyles).

**Scheduling.** `AnimationRecord::next_time`: the next update for a
pending play or pause; nothing when paused, idle or finished; the end of
the delay; then each iteration's end. With painted frames (the GUI calls
`Page::set_paints_frames(true)`), also the next frame of a continuous
easing, or the next step boundary when every keyframe interval eases with
the same `steps(n)` over keyframes at 0% and 100% only (another shape counts
as continuous), and steps shorter than a frame count as continuous.
`Page::animation_time` puts that time on the grid, at least one frame
after the last update; `frame_time` and the next task time include it (so
the GUI's `WaitUntil` and the jumps of virtual time follow), `frame_work_due`
does not (a page whose only work is animations is idle). The minimum next time over the records is cached
per document (for painted and unpainted frames; `update` computes it in its
loop, a change of a record drops it), so `frame_time` and `next_task_time`
cost O(1) per document while nothing changes. A record whose next event is
not after the timeline (float precision, a script's `currentTime` of 1e19)
asks for the next frame when frames are painted and for nothing otherwise.
A headless `--settle-ms 10000` with an infinite 1 s animation costs about
11 updates.

**API.** `getAnimations()` of documents and elements (one element without
`subtree` reads its own records directly; with `subtree` the owners are
sorted in tree order),
`CSSAnimation` (`animationName`, `play()`, `pause()`, `currentTime`,
`startTime`, `playState`, `pending`, `replaceState`, `id`, `effect`) and
`KeyframeEffect` (`target`, `pseudoElement`, `composite`, `getTiming()`,
`getComputedTiming()`, `getKeyframes()`), in `web-animations.idl`. The
wrapper map of section 1 is `AnimationObjects` in the web crate; the
engine reports released ids, and keeps them only after the bindings
asked once (the first `getAnimations()`), so a page without scripts that
churns animations holds no list. A wrapper whose record went keeps its
`animationName` and `effect.target` (Chromium, probe `remove-element`).
`currentTime` of a held animation (paused or seeked) is returned as stored
(Chromium reads `2500.0004` back); a running animation's is rounded to
1e-9 ms, which removes the float noise of `timeline - start time` but keeps
a time that a script set (Chromium reads `2500.0004` back from a running
animation too; the review's probe `current-time-values`), and `pause()`
rounds the hold time it takes from a running animation to 1e-9 ms for the
same reason. `getComputedTiming()` and `getTiming()` came
along because most seeking rows read them. `getKeyframes()` gives the
specified values in the order of the properties' first declarations,
with the implicit keyframes holding the element's current computed value
(composite `replace`). Departures: Chromium gives computed keyframe values
(`var()` substituted), the underlying value in implicit keyframes, its own
property order and empty keyframes (`keyframes-resolution` waits for 3c).

**The period of `steps()`.** The scheduling uses the period `duration / n`
for every position. The review expected `duration / (n + 1)` for `jump-both`
and `duration / (n - 1)` for `jump-none`; the function of CSS Easing 2
(`floor(input * n)`, plus 1 for the positions that jump at the start, over
n + 1, n or n - 1 levels) jumps at multiples of `1 / n` for all of them, and
a unit test checks every position.

**Fixes of the review.** The matching of the records of an element with its
names builds a map once (50,000 names on one element: 25.7 s before, 0.14 s
after, headless with virtual time); the lazy catch-up and the cached next
time make 20,000 infinite animations with a chain of 20,000
`setTimeout(f, 0)` cost 0.42 s (19.4 s before; 0.10 s with
`--animations off`), 0.34 s paused (6.9 s before); 20,000 `getAnimations()`
calls of single elements cost 0.46 s with the page (2.2 s before).
`swb.py`'s `serve_command` and `Browser.start` take `animations`; `compare
--click` passes false. The cookie jar takes its time from the page clock
(ADR 0012, "The page clock").

**Not in 3b** (feature 3c): events, `ready` and `finished`, `cancel()`,
`finish()`, the rest of the script overrides (`script-overrides`; 3b has
the first: after a script's `play()` or `pause()`, `animation-play-state`
no longer plays or pauses the animation), `document.timeline`,
the visibility test of section 8 and the GUI budgets. `transition-*` stay
inert (feature 5); `getAnimations()` lists no transitions.

### 16. Done in feature 3c

Feature 3c adds sections 6, 7 (without `getKeyframes()` with computed
values, below) and 8, and the hostile cases of section 11 for that code.
Measured in Chromium 148 with the new probe file
`tools/probes/css-animations-3c.json` (6 cases, 20 rows, three equal
runs).

**The event queue** is in `DocumentAnimations` (`events.rs`): a
`PendingAnimationEvent` keeps the type, the id, the owner, the name, the
position in `animation-name`, `elapsedTime` and the scheduled event time,
so a record that went still gets its `animationcancel`. `update` compares
each record's phase and current iteration with the last update (the table
of CSS Animations 2 §5.1; at most one `animationiteration`, its elapsed
time from the iteration boundary). The scheduled event time is the
timeline time of the boundary (the event's local time mapped through the
start time; for a held animation through the timeline time minus the hold
time); a cancel is scheduled at the timeline time of the cancel. A cancel
by a style change (`display: none`, a removed name) or by `cancel()` is
queued at once when the animation was in its before or active phase at
the last update (an animation created and cancelled before its first
update sends nothing, as in Chromium: `ready-promise`). A waiting queue,
or a record that went while the bindings track records, makes
`next_time` ask for the next update. A document without a script runtime
drops its queue in each rendering update: the top-level document with no
runtime, and a frame document without a realm (a sandboxed frame without
`allow-scripts`), whose events no task would take and whose waiting queue
would keep the next update due at once.

**Order.** `send_animation_events` (engine) stable-sorts the queue by
scheduled event time (unresolved first; none is unresolved today), then by
the owner's tree order, the pseudo-element (`::marker`, `::before`,
`::after` after their element) and the position in `animation-name`
(`events-pseudo-order`: `e1`, `e1::before`, `e1::after`, then `e2`; the
cancels of one update before the start of the new animation of the same
element). The scheduled times are placed from the start time as it is
(not from `timeline - current time`, which carries the 1e-9 ms rounding
of the current time) and rounded to 1e-6 ms, so two boundaries that are
one time in exact arithmetic tie, and the stable sort keeps the queue
order: the `animationstart` that a catch-up queued and the
`animationend` after `finish()` (which moves the start time back by the
end) came in either order before (`events-cancel`). A tie between two
animations goes by composite order: in `finish-and-start-time` the start
of `#a` (resolved by the catch-up) and the end of `#b` (finished in the
same task) tie, and swb gives `#a` first; Chromium gives either order
between runs (departure in `SWB_ROWS_3C`). The web side (`update_animations_and_send_events`) runs two
tasks: the promises (the end of the task is the microtask checkpoint, so
reactions run before the events), then the events, with a microtask
checkpoint after each listener. An event type without a listener skips
its path (`listened.rs`).

**A style step in the rendering update.** Chromium's frame computes the
styles; swb's styles are lazy, so a script that sets `animation` and reads
nothing (the event rows) would start nothing. The rendering update now
computes the styles of a document that has `@keyframes` rules and stale
styles (`update_animation_styles`), and a task with DOM changes asks for
an update when that holds. Pages without `@keyframes`, and `--animations
off`, are unchanged. One departure follows: swb starts the animations of
the load before the first frame. Right after the load the animations of
the page's style sheets have already started (`animation-api` query 2:
`pending` false; Chromium's wait for their first frame), and
`animationstart`, `ready` and the first update come earlier than in
Chromium.

**Float noise at the end.** The update at the scheduled end of an
iteration computed `timeline - start` as 199.99999999999997 for 200 and
lost the end until the next update. A running animation's current time is
now rounded to 1e-9 ms in the model (`round_noise`), as the bindings
already did.

**The promises** live in the web crate: a wrapper keeps its current
`ready` and `finished` promises, made at the first read; a pending one puts
the wrapper on a waiting list. The promise step settles them from
`EngineHost::animation_states` (the play state and the pending flag of
each id): `ready` when no task is pending and the animation is not idle,
`finished` when it is finished; a record that went (a cancel by a style
change) rejects both with an `AbortError` ("The user aborted a request.",
as Chromium) marked as handled, and the next read makes new ones. The
commands (`play()`, `pause()`, `finish()`, `currentTime`, `startTime`)
settle the wrapper's promises at once (`finish()` resolves `finished` in
the task, `finish-and-start-time`). `cancel()` rejects a pending `ready` and
the `finished` promise and drops both; a `ready` read afterwards stays
pending while the animation is idle (Chromium; Web Animations would
resolve it). After `cancel()` the record stays, idle, while its name stays
in `animation-name` (`script-overrides` query 2), and `play()` starts it
again. `swb_js` got `Runtime::mark_promise_handled`: it sets
`[[PromiseIsHandled]]` before the rejection, so the rejection tracker never
sees it. Base UI's `Promise.all(el.getAnimations().map((a) => a.finished))`
settles when the animations end, on `display: none` and on a removed name,
with no `unhandledrejection` (`promises-base-ui`). `mark_promise_handled`
is for promises that are still pending: it does not remove a promise that
was already reported, and it fires no `rejectionhandled`.

The waiting list is ordered by animation id, which is the order of
creation, so the promises of one update settle in the same order in
every run (virtual time repeats). Chromium's order is not a rule that
derives from creation: with four animations that start in one update it
settles `ready` as 0 1 3 2 (the same in four runs), `finished` as 0 1 2 3,
and the rejections of a removed element, a removed name, `display: none`
and a replaced animation in the order 2 1 3 0 (`promise-order` of
`css-animations-3c.json`; the rows sort the promise logs and keep the
event logs). The test `promises_settle_in_the_order_of_creation` pins
swb's order. The settle step costs nothing when no animation changed:
`DocumentAnimations::state_epoch` counts the changes of a record's play
state or `pending` flag (by an update, a command or a style change) and
of the number of records; the bindings keep the epoch of their last
settling and the engine runs the promise task only when they differ
(`has_animation_promises_in`). A page whose scripts read the `finished`
of 10,000 infinite animations pays the lookups once per change, not once
per rendering update.

**`document.timeline`**: one `DocumentTimeline` per window (also for the
documents that scripts create). Measured: its `currentTime` equals the
timestamp of the animation frame callbacks of the same update, is not
later than `performance.now()`, does not move within a task, and an
animation's `currentTime` is `timeline.currentTime - startTime`. The value
is a multiple of 0.1 ms in both engines (68.5, 85.1, 101.8 in Chromium);
`Number.isInteger(currentTime * 1000)` is false for about 1 % of such
values in doubles (16.1 gives 16100.000000000002), so the row of the
probe tests the distance to the nearest integer. swb gives
the timeline's time as `performance.now()` gives that page time (coarsened
to 0.1 ms) and shifts the start times by the same coarsening, so the three
relations hold. `startTime` can be set (null holds the current time);
`finish()` of an infinite animation throws `InvalidStateError` with
Chromium's message. `CSSTransition` exists for `instanceof` (no instance
before feature 5); `AnimationEvent` has its constructor
(`document.createEvent('AnimationEvent')` throws, as in Chromium).
Departures: the constructors of `DocumentTimeline`, `Animation` and
`KeyframeEffect` throw `TypeError: Illegal constructor` (Chromium makes
objects); `Element.prototype.animate` does not exist (`animation-api`
query 0).

**The visibility rule** (`visibility.rs`, `swb_paint::animation_bounds`,
`DocumentState::update_animation_visibility`). Each record has a reach,
from the properties of its keyframes:

- `Subtree` (opacity, the colors, `visibility`, the position and size of
  backgrounds): the animation changes only the pixels of its owner's
  subtree. The bounds are the painted rectangles of every fragment of the
  subtree (one walk of the fragment tree, `FragmentTree::walk_painted`:
  scroll offsets, transforms, fixed and sticky offsets), each grown by its
  shadows, its outline and its filter output.
- `Clip` (the transforms, the individual transforms, `transform-origin`,
  the filters, the shadows, `clip-path`, the outline): the animation can
  paint anywhere inside the nearest flat-tree ancestor that clips it in
  both axes; the bounds are that ancestor's painted border box. Without
  such an ancestor, or with an absolutely positioned or fixed box on the
  way (its containing block can be outside the clip), it is visible.
- `Everywhere` (any other property, custom properties included): it can
  move other boxes, so it counts as visible.

An owner that has no fragment of its own and none below it paints through
the box of an ancestor: the children of an `<svg>` (the fragment tree has
one box for the `<svg>` and none for its `<circle>`). Such an owner counts
with the painted bounds of its nearest flat-tree ancestor that has a
fragment, so an SVG spinner has frames while its `<svg>` is in view and
none otherwise (a second walk of the fragment tree, only when such owners
exist; the walk up from the owners visits each ancestor once). An owner
with no box and no ancestor box has no bounds and is not visible. This
over-approximates on purpose: an owner that has a style but never paints
counts as visible while an ancestor box is in view (the content of a
closed `<details>`, a shape inside `<defs>`, a circle outside the
`<svg>` viewport, an empty `display: contents` owner), so it costs
frames that change no pixel, never a missed one. Frame documents have no
visibility test: an animation in a frame below the viewport gets every
frame (backlog).

The rule is safe: it never treats as invisible an animation whose pixels
can enter the viewport, at the cost of counting some as visible (a
transform animation without a clipping ancestor). It runs after each
display list and each scroll or resize (its key: the display list serial,
the viewport scroll offset, the viewport size).
`DocumentAnimations::set_visible` keeps the
visible owners; `next_time` gives an invisible animation its event times
only, also with painted frames. Departure from the plan of section 8: the
bounds come from a walk of the fragment tree at the time of the display
list, not from the builder, which works in local coordinates inside
transform groups and has no element identity on its items.

**Tests.** `css_animations.rs`: every row of `css-animations.json` but
`get-animations-order` (transitions, feature 5), with two departures in
`SWB_ROWS`; every row of `css-animations-3c.json`, with two departures
(`SWB_ROWS_3C`). `animation_frames.rs`: events without a read of the
styles, a paused start and a restart (virtual time); offscreen animations
with painted frames cost only their event times, visible ones each frame,
and scrolling shows them and hides them again. `frame_outdated.rs`: a
visible animation outdates the frames, an offscreen one does not until a
scroll shows it. Unit tests in `events.rs`, `records/tests.rs`,
`visibility.rs` and `animation_bounds.rs`.

**The frame oracle for animations** (`frame_oracle.rs`,
`animation_frames_are_asked_for_when_pixels_change`): the pages run in
virtual time with painted frames, as the GUI does. At each step the clock
advances by 130 ms through the page's event loop (its own frames run), the
oracle records `frame_outdated()`, renders, and compares the pixels with
the last render. A frame that was asked for and drew the same pixels is
allowed and counted (`wasted`); changed pixels without a frame fail. The
earlier reason against an oracle for animations (a render catches the
animations up to now, so a render of a page that asked for no frame can
differ) does not hold when the event loop advances the clock first: a
visible animation then has its frames and the render finds nothing new, and
a missed frame shows. Cases, each with JavaScript off and on: a visible
owner; a transform clipped out of a strip that is in view (13 frames, 12
without a pixel change: the bound of a transform is its clip box) and of a
strip out of view (1 frame); an inner scroll box with an animation in view
and one out of view, swapped by the wheel (39 frames, 2 wasted); an owner in
an `<svg>` in view (13) and out of view (1: the event time); `position:
fixed` after a scroll (14, 1 wasted); `position: sticky` (14); `visibility`
from hidden to visible (13, 12 wasted: the hidden box is in view, so its bounds are); a
`display: contents` owner with an inherited `color` (13); a transform that
moves a box into the viewport (13, 2 wasted). Without the fix for owners
inside an `<svg>` the SVG case fails (a changed pixel without a frame); the
oracle found no other case.

**Hostile cases** (headless, virtual time, release): an `animationend` listener that restarts a 1 ms animation runs 75 rounds in 2.5 s (once per update), 0.05 s; 10,000 elements with a 1 ms infinite animation and an `animationiteration` listener, 1 s of page time: 530,000 events, 1.0 s, 150 MiB; `await` of the `finished` promises of 99,000 animations: 0.45 s, 270 MiB; `cancel()` and `play()` of 10,000 animations 20 times: 0.55 s, 296 MiB; 10,000 animated elements outside the viewport: 0.25 s, 157 MiB.
Added with the review fixes: 10,000 infinite 10 ms animations whose `finished`
and `ready` a script read, 5 s of virtual time (500 updates): 1.25 s before
the state epoch, 0.84 s after, 150 MiB (the `await` of 99,000 animations is
0.45 s in both: it settles once); 10,000 `<svg>` elements with an animated
`<circle>`: 0.30 s, 163 MiB.

**GUI** (`gui-latency`, release, own Xvfb, `idle-10s`; before 3c, then with 3c): the marquee outside the viewport 60 to 0 frames per second (CPU 5.5 % to 0 %); a `display: none` spinner and paused animations 0 frames; the spinner alone 8 frames per second (0.9 % CPU); the marquee in view 60 frames per second (5.5 %); the videojs fixture at scroll 0 29.3 to 3.6 frames per second (CPU 98 % to 14 % during the trace, 2 % after). The remaining frames on videojs came from the catch-up of the offscreen marquee in the page's tasks; `paint_pending` now counts only the marks of visible owners (`has_shown_marks`). After that change and the review fixes: videojs at scroll 0 2.2 frames per second (CPU 9.7 % during the trace, 0.0 % after); an SVG spinner in view 60 frames per second (6.4 %).

**Not done in 3c**: `getKeyframes()` with computed values (the backlog
item of 3b). Chromium's `getKeyframes()` of a CSS animation gives computed
values (`var()` substituted, `currentcolor` as the element's color,
shorthands as longhands with computed values such as `backgroundSize:
"auto"`), the underlying value in implicit keyframes (`left: "auto"`),
keyframes without values (`50%{animation-timing-function:linear}`,
`to{all:initial}`), and no custom properties. Its property order is not a
rule that measurements can give: in `keyframes-resolution` (`sh`) the two
keyframes of one rule list the longhands of `margin` and `background` in
different orders, so the order would be a departure. The values need a
serialization of computed values without layout and the element's style
without its animations; `keyframes-resolution` still waits
(`WAITS_3A` in the test).

### 17. Done in feature 4a

Feature 4a does the first half of section 9: the classification, the
paint-only path for fragments and the check mode. The damage rectangles
and the region raster (feature 4b) are not done; every frame still rasters
the whole viewport.

**The table, as implemented** (`style/src/paint.rs`, `PAINT`, by longhand):
`opacity`; `transform`, `translate`, `rotate`, `scale`, `transform-origin`;
`filter`, `backdrop-filter`; `color`, `background-color`, the four border
colors, `outline-color`, `text-decoration-color`, `fill`, `stroke`,
`-webkit-text-stroke-color`, `-webkit-text-fill-color`; `box-shadow`,
`text-shadow`; `background-position-x` and `-y`, `background-size`;
`clip-path`, `mask-position-x` and `-y`. Two groups are new against the
list of section 9: `outline-style`, `-width` and `-offset`, and
`text-decoration-line` and `-style`. The first hover traces on the page
fixtures showed why: most hover changes that the table of section 9
refused changed the outline (the focus ring) or the decoration line (the
underline of a link), and nothing in layout, scroll or the engine reads
them (no ink overflow in swb; decorations propagate to the text of
descendants in paint, from the fragment styles). With the two groups in
the table, no hover change on the four measured fixtures lays out (table
below). Not in the table: `visibility`
(hit testing and focus), `background-image` and the other layers (image
loads), the border styles and radii (collapsed borders and the native look
of controls read them), the stroke width and the opacities of SVG.

**Classification** (`paint_change` in `style/src/paint.rs`): a change is a
paint change if the old style with the paint properties of the new one
(`copy_paint_properties`) equals the new style, that is, every other field
agrees, also the fields that no table lists; the custom properties are
ignored (no layout code reads them, a changed value that a longhand
resolved shows in that longhand). Two more conditions keep the
containing-block cases out: `has_transform` (any of `transform`,
`translate`, `rotate`, `scale` is not `none`) and `is_filter_containing_block`
(`filter` or `backdrop-filter`, also `will-change`) must be equal in the
old and new style. A change of `transform: none` to a value while
`translate` is set is a paint change, as the box was a containing block
before. `opacity` is a paint change between 1 and a smaller value: layout
never reads the stacking contexts (`forms_stacking_context` is paint and
selection only), a search of the layout and engine crates for the table's
properties found no other reader than the ones listed below. The
comparison is a clone and a compare of the style (about 0.6 us), and runs
only for the styles that changed.

`Restyle::Done` has a new field `paint: Option<PaintChanges>` (`cascade/targeted.rs`,
`finish_targeted`): `Some` if something changed and every change is the
style of an existing element or pseudo-element that passed `paint_change`;
the pairs (`StylePair`: node, pseudo-element, the old and the new style
object, a `PaintDelta` with `geometry`, `inherited`, `colors`) are what
the fragment walk needs. It is `None` when an element appeared or went
(`display: none` both ways, DOM changes), when the counter ordinals or the
`use` instances changed, when a pseudo-element appeared or went, and for
the SVG elements whose styles layout bakes in a way other than a shape or
group below an `<svg>` (`use`, `symbol`, `clipPath`, `defs`; `svg_content_hazard`).

**The fragment walk** (`layout/src/paint_restyle.rs`,
`FragmentTree::apply_paint_changes`): one walk over the root, the top layer
and the hidden contents. Equal styles share one `Arc` (two `details` with
the same values), so matching by the old object alone replaced the style of
an element that had not changed; the first run of the check mode found it.
A fragment is matched by its owner and the object together: a box by
`(node, pseudo)`; a text fragment by its owner, which is the element of its
text node (the flat parent) or, for generated text, the element of the
pseudo-element; the text in an anonymous or generated box by the box's own
(unshared) style object. The root and body fragments can hold a modified
copy of the style (the `overflow` of the viewport propagation): the paint
properties are copied into the copy. Anonymous boxes and the text of
controls derive their style from the parent box (`anonymous_from`): when an
inherited paint property of the parent changed (`PaintDelta::inherited`),
`copy_inherited_paint_properties` copies the inherited paint properties of
the new parent style. The walk cost is one lookup (a binary search over
the pairs sorted by node) per fragment; 10,000 changed styles take 0.9 ms.

What layout bakes from styles, and what the refresh does about it:

| Baked value | Refresh |
|---|---|
| `ControlContent::native` (the look of a control, from the background, borders and radius) | derived again from the new style (`has_native_look`) |
| `CanvasBackground` (which of the root and the body paints the canvas, with the style) | made again (`canvas_background`) |
| Backgrounds of table parts in the cells (`PartBackground`, now with the element) | the style replaced by the part's element; a row that gets or loses a background is added to or taken out of each cell of the row (`toggle_row_background`: the area is the row's box relative to the cell, the clip the cell); a row group or column that gets or loses a background: layout again |
| Collapsed table borders (resolved colors) | layout again if a color of the table or one of its parts changed |
| `<svg>` content (`SvgContent`: styles of groups and shapes) | the styles replaced by the element; layout again if a new style changes what the build derived (an opacity layer, a shape with two paints, a clip path, a rotating transform) |
| The text of a `::placeholder` (derived from the pseudo-element) | layout again |
| An anonymous `inline-table` (derived from an inline box that is not its parent) | layout again |

**Overflow and caches.** A change of a transform property moves the
transformed bounds that scrollable overflow includes (`scroll.rs`). Layout
now keeps, for each scroll container, the extent before the contributions
of the descendants (`BoxFragment::overflow_base`, set by
`set_scrollable_overflow`); `refresh_scrollable_overflow` adds the
descendants again, after the walk has replaced the styles below a container
(bottom-up, only for containers with a changed transform below). The
viewport's scroll size is computed again from the roots. The result equals
layout's up to the last float digit (the check compares within 0.01 px). A
changed transform also bumps `layout_serial` and drops the client rectangles
(`geometry`, `used_boxes`, `svg_geometry`, the observers, lazy images and
frame positions are keyed by the serial) and updates the ranges of the scroll
containers (`scrollers`). Colors and effects keep all caches.
`getBoundingClientRect` after a transform change gives the new value
(`script_geometry_follows_a_transform`).

**Paths.** The state restyle, the animation ticks (marked owners) and the
DOM restyle all go through `run_targeted`, which now returns what stays of
the layout (`LayoutAfter`: keep, drop, or paint with the pairs). After DOM
changes the kept layout is restored first (the changes were layout-free,
section 6 of ADR 0042), then refreshed. Frame documents run their own
pipeline. A full restyle keeps its behavior (the layout goes unless the
styles are equal).

**The check mode** (`engine/src/document/paint_check.rs`, debug builds,
on with `set_restyle_check(true)`): when the display list of a frame whose
fragments went through a refresh is built (the check needs the fonts, which
the restyle does not have), the document is laid out in full from the same
styles and `first_difference` compares the two fragment trees box by box
(owner, style, border and padding boxes, content, baselines, scrollable
overflow, scroll size, canvas background, viewport state) and the display
lists item by item; a difference panics with the path and the first
difference. `swb_engine::paint_checks()` counts the frames compared. The
equality of `SvgPath` ignores its lazy `fill_bounds` (a cache). What the
check found: the shared style objects above, and one false difference (the
cache). No property had to leave the table. Frames compared: 249 to 273
in the hover and press sessions of the page and JS fixtures (8 x 6 grid), 844
with a grid of 20 x 16, 90 and 90 in the videojs animation runs, 33
in the probe pages of `css-animations` at their seek times, and the cases of
`paint_only.rs`.

**Measurements** (release, one run each, `swb --bench 3`, JavaScript off
unless it says on; before is the commit of feature 3c). Hover, 8 x 6 grid, 3
rounds; the steps that changed the state; each cell is before / after:

| Fixture | Steps | Changed | Change median ms | Change p90 ms | Layout runs per change | Layout ms per change | Paint-only changes |
|---|---|---|---|---|---|---|---|
| videojs | 144 | 30 | 30.15 / 30.48 | 32.51 / 31.23 | 0.40 / 0.00 | 0.63 / 0.00 | 0 / 12 |
| hacker-news | 144 | 82 | 0.71 / 0.70 | 2.62 / 0.92 | 0.37 / 0.00 | 0.56 / 0.00 | 0 / 30 |
| wikipedia-web-browser | 144 | 72 | 2.31 / 2.28 | 15.80 / 3.85 | 0.33 / 0.00 | 3.72 / 0.00 | 0 / 24 |
| github-swb | 144 | 72 | 18.05 / 12.75 | 18.87 / 13.42 | 0.58 / 0.00 | 2.89 / 0.00 | 0 / 42 |

On videojs the hover frame stays at 30 ms because the raster of the hero
takes 30 ms (feature 4b); layout was 0.6 ms there. The p90 of Wikipedia
falls from 15.8 to 3.9 ms and of Hacker News from 2.6 to 0.9 ms (the
changes that laid out), GitHub from 18.9 to 13.4. With JavaScript on,
videojs: layout runs per change 1.4 to 0.13, 39 of 45 changing steps
paint-only, median 31.7 to 30.1 ms, p90 51.7 to 50.7 ms. Animation frames
on the videojs JS fixture (real time, animations on, 25 frames): layout
1.39 ms to 0 (24 of 25 frames paint-only; the refresh takes 0.05 ms a frame),
style 0.65 ms, display list 0.33 ms, raster 36 ms, frame median 38.9 to
37.4 ms. With JavaScript off no animation of the page runs in the bench.

GUI (`gui-latency videojs --js --trace videojs-page --window 1280x840`,
release, own Xvfb): pointer move to frame median 50 to 49 ms, p90 70 to
68 ms; frames per second 19.5 to 19.7; CPU during the trace 40.5 % to
40.2 %; layout runs in the trace 430 (650 ms) to 233 (348 ms). The frame
time is the raster, as predicted; the budget of 50 ms at p90 needs
feature 4b.

Hostile cases (release, `just hostile`; time and peak memory, before /
after):

| Case | Time | Memory |
|---|---|---|
| `paint-only-color-animation-10000-elements` (150 frames) | 14.6 s / 11.7 s | 187 / 167 MiB |
| `paint-only-inherited-color-100000-text-fragments` (8 frames) | 3.11 s / 1.61 s | 425 / 418 MiB |
| `paint-only-transform-in-1000-scroll-containers` (150 frames) | 0.10 s / 0.05 s | 79 / 79 MiB |
| `paint-only-opacity-toggle-10000-elements` (100 toggles) | 5.7 s / 5.3 s | 153 / 152 MiB |

A frame of the first case costs 95 ms before (69 ms style, 26 ms layout) and
76 ms after (75 ms style including 6 ms of classification, 0.9 ms
refresh): the cascade of 10,000 animated elements now dominates. The load
stages of `perf videojs` and `perf hacker-news` (5 runs) are unchanged, the
snapshot of the page fixtures and layout tests is byte-equal, and the
scores of `compare --all` and `compare --js --all` are equal (`js-ars-technica`
alternates between two pixel scores with one binary).

**Not done**: the damage rectangles and the region raster (4b); a row
group or column that gets or loses a background, a `::placeholder` change
and the SVG structure changes lay out again; `visibility`,
`background-image` and the border styles are not paint properties here
(backlog: `visibility` needs the hit-test and focus caches, the border
styles the collapsed-border and native-look checks).

### 18. Done in feature 4b

Feature 4b does the second half of section 9: a frame compares its display
list with the one of the last frame, gets damage rectangles, and rasterizes
only those into the kept pixmap of the window. The result is bit-exact with
a full raster; two oracles show it.

**The diff** (`paint/src/damage.rs`, `list_damage`). Both lists are walked
in paint order with one state (the matrix from the current coordinates to
device px, the clip in device px, whether a resampled transform is open):
the walk only goes into groups that are equal on both sides, so both sides
are in the same state. The rules:

- equal items add nothing; `DisplayItem` has an equality
  (`display_list/equality.rs`) that compares shared values (`Arc`) by
  pointer first, then by value. `Image`, `Frame` and `PushMask` items also
  compare their *media keys* (`media_keys`): the id of the decoded image
  (`DecodedImage::id`, unique in the process) and its content generation
  (`ImageSource::generation`), the generation of a frame's documents
  (`FrameContent::generation`), the keys of the images of a mask;
- different drawing items add their device bounds (the item's bounds mapped
  by the state, clipped by the clip, rounded out, plus 1 px);
- two different groups of one kind add the bounds of both whole groups (the
  bounds of `finish_groups`; a filter's grown by `css_output`; content fixed
  to the viewport at the viewport scroll offset, not clipped) and the walk
  goes on after both. One exception is entered instead: transform groups
  with an equal translation (a fixed or sticky box, or a translation
  matrix); their bounds only follow their content and cull it, and their
  pixels are the same for each pixel of equal content. Opacity groups with
  equal opacity and blend mode but other bounds are not entered: the layer
  of an opacity group has the extent of its bounds, and tiny-skia's
  anti-aliasing of equal content changes with that extent (the review found
  it with random list pairs: up to 14 px, 9 levels);
- where the items differ otherwise (a group starts on one side only, a clip
  differs, items were inserted), a lookahead of at most 64 items per side
  looks for the place where the lists meet again: two equal items in a row,
  with only whole groups and clips skipped on each side. The skipped items
  are damage (whole groups for the groups among them). The lookahead has a
  budget of 200,000 tested places per diff; without a meeting place (or
  budget), the longest common suffix of the enclosing group's interior is
  kept, cut to a place at the depth of the difference, and everything between
  is damage. Section 9 named only the prefix and suffix; the first traces
  showed why the lookahead is needed: on videojs.org the marquee changes in
  every frame near the end of the list, so a common suffix ends there, and a
  hover that adds an opacity group damaged everything between (a full raster);
- inside a filter group the content is not clipped by the enclosing clips
  (as the rasterizer draws it), and its damage grows by the filter's reach
  (the `css_output` of an empty rectangle, times the device scale of the
  group) and is clipped when the group ends;
- `HitRegion` and `HitShape` items are skipped.

The walk keeps its own stack, skips whole groups by their ends (`group_ends`,
one pass) and scans the suffix of a group's interior once (the walk then goes
through it without a difference), so it is linear in the lengths of the lists
(the lookahead is bounded per difference and in total). The groups must nest;
a list whose groups and clips do not nest gives a full raster. The builder did
not nest them in one case: a positioned box that repeats a rounded clip (a
group) and a plain clip of its ancestors ended them in the order it started
them (`display_list/builder.rs`, the repeated clips of a deferred box); the
ends are now in reverse order. The rasterizer keeps clips and groups on two
stacks, so the pixels are the same (snapshot equal).

**The merge** (`merge_damage`). The rectangles are rounded out to whole
pixels plus 1 px and clamped to the target; more than 64 are first merged by
the cell of their center on an 8 x 8 grid; then the pair whose bounding box
adds the least area is merged (overlapping pairs first) until none overlap
and at most 8 remain. A `backdrop-filter` reads the whole box (it mirrors at
the box's edges), so each rectangle that touches the box of a
`backdrop-filter` item (`backdrop_boxes`: device boxes of all of them, grown
by the reach of the filters around them and by 4 px inside a resampled
transform; and the rectangle of a frame whose list has a `backdrop-filter`
or a frame) grows to hold all of it, then the merge runs again; after 8
rounds, a full raster. A full raster also runs when the rectangles cover
more than half of the target (section 9).

**The inputs outside the list, and their rules** (`engine/src/document/frame_damage.rs`):

| Input | Rule |
|---|---|
| Scroll offset of the viewport, viewport size, scale | in the raster parameters of the last frame; a change gives a full raster |
| Scroll offsets of scroll containers | in the list (the builder moves the content inside the clip): the diff sees the moved items, clipped to the container |
| Caret, selection highlight, focus ring, scroll indicators of scroll containers | in the list (the builder draws them) |
| Viewport scroll indicators (drawn over the page after the raster) | their items are a second list, compared in viewport coordinates; their damage joins the page's, and they are drawn again in every rectangle of damage (`rasterize_region` with no background) |
| Images: a load, a decode, a new source of the same element | the media key (decoded id): an `Image` item whose key changed is damage, also when the list stays equal |
| Media frames (M14) | the content generation of the image (`Images::bump_generation`, `Page::bump_image_generation`); the paint stamp holds the document's count of bumps, so `frame_outdated()` sees a bump without a new list |
| Frame documents | the media key of the `Frame` item: a hash of the paint stamps of the frame's document and of the documents inside it, of their viewports and canvases (`frames/paint.rs`, `subtree_generation`); a change damages the frame's rectangle |
| Work that a budget skipped or coarsened (`Skipped`, the SVG rendering budget, `reduce.skipped()`) | the rasterizers return a `RasterReport`; an incomplete raster makes the next frame a full raster |
| The pixels of the target | the caller says they hold the last frame of this page (`Page::render_update`); a new document has no last frame; the composer draws in full when its layers are new |

**The region raster** (`paint/src/raster.rs`, `rasterize_region`; section 9
said "the visible area set to the rectangle and a hard device clip"; that is
not exact). What makes it exact:

- The parts are made disjoint (`raster/region.rs`; overlapping parts become
  their bounding box) and drawn in one pass into a scratch pixmap of the
  target's size; each part is filled with the background first (or copied
  from the target, for the viewport indicators), and copied to the target at
  the end. So every pixmap that the rasterizer uses, the target and the
  layers of groups, has the size and place it has in a full raster: tiny-skia
  clips paths at the edges of the pixmap, and a path cut by a smaller pixmap
  gets other coverage up to 10 px inside (measured: up to 11 levels), and a
  draw through a clip mask is 1 level off for opaque colors (measured). The
  layers of opacity, mask, clip path and filter groups keep their full-raster
  extent; transform layers keep their full-raster plan (the pixel grid of
  section 9).
- Only the items are culled to the parts (their bounding box, with 3 device
  px of slack), and the drawing whose pixels do not depend on its extent is
  limited to the parts: rectangles without anti-aliasing, images and
  gradients (fills of whole pixels with a shader that is computed per pixel),
  glyphs (`blit_mask`) and the composite of a layer (cropped). Anti-aliased
  paths, borders, clip paths and masks are drawn as in a full raster.
- A filter draws its content in the parts grown by the filter's backward
  reach (the blur at a pixel reads only that far), inside its layer.
- A transform layer draws its content only where the resampling of the parts
  reads it: the parts' bounding box grown by 2 device px, mapped into the
  group, grown by 2 layer px.
- A part that touches a `backdrop-filter` box holds all of it (the merge
  above; `Page::render_regions`, the oracle's entry point, applies the same
  rule with `hold_backdrop_boxes`).
- The budgets are those of a full raster of the target (the pass draws fewer
  items, so it draws everything a full raster draws). The converse is not
  true: near a budget limit, a region can draw a group that a full raster of
  the same list drops, and `incomplete` stays false. This errs on the side
  of drawing more.
- Equal items in a group that is entered are drawn with the same pixels
  only if the layers around them keep their extent; the diff enters only
  transform groups with an equal translation (above), which make no layer.
- Slack: a border of a box narrower than its border widths snaps its inner
  edge to at least one pixel, which can lie one pixel outside the outer edge
  (a pre-existing imprecision, backlog); the cull has 2 px of slack for it.

The cost follows the damage except for the drawing that keeps its full
extent: the coverage of large clip paths (rounded `overflow` clips of large
boxes), masks, the blur of large filter layers, anti-aliased paths
(rounded rectangles, SVG) and transform layers are paid once per frame in
full. On github-swb (rounded boxes of the file list) a frame of 19 % of the
viewport costs 10.6 ms against 11.5 ms for a full raster (3.7 ms of it are
the clip coverage of 0.93 Mpx of rounded boxes); a first version drew each
part in its own pass and paid these once per part (52 ms).

**The GUI** (`swb/src/gui/compose.rs`). The composer keeps the page pixmap;
a new composer (new window size or scale) renders in full with
`Page::render`, later frames with `Page::render_update`. A navigation gives a
new document, whose first frame is full. The status bubble is drawn again
whenever the page was drawn (it copies the page's pixels under it first), as
before. The window buffer is still written in full (softbuffer's buffer age
can be 0). The frame log has three more fields: `full=F rects=N pixels=N`.
Headless screenshots keep the full raster (`Page::screenshot` does not touch
the last frame).

**`--bench`.** The hover and animation stages render with
`Page::render_update` into the kept pixmap and report `render`: the median
and p90 raster time, the median time of the diff (`StageTimings::damage`),
the frames by kind (full, region, empty) and the median pixels of a region
frame. The scroll stage keeps the full render (each step scrolls the
viewport).

**The oracles.** `engine/tests/region_oracle.rs`: full render into A, copy
to B, random parts of B filled with garbage, `Page::render_regions`, B must
equal A; the nine page fixtures and the JS fixtures (when present), at scales
1 and 1.5, at three scroll offsets, and a page of effects (rounded images,
shadows, tiled gradients and images, blur and drop shadow, backdrop filters,
masks, rotation with scale, skew, inline SVG, a blend mode, a frame, text
strokes and shadows, dashed rounded borders, outlines, clip paths) at scales
1, 1.5 and 2. 200 rounds of up to six parts each passed (about 70,000 parts);
the default is 12 rounds per configuration (40 for the effects page).
`engine/tests/damage_oracle.rs` and the damage check in `frame_oracle.rs`
(`tests/common/damage.rs`): sequences of frames, each rendered with its
damage into the pixmap of the last one and compared with a full render: the
videojs fixture with its animations in virtual time (130 ticks, scrolled to
the marquee and the bars; JavaScript off and on), the hover and press grid on
eight page fixtures and the JS fixtures, scrolls of scroll containers, a frame
whose document animates, hovers and scrolls, an image that arrives after the
first frames, the caret and the selection, a new content generation, and the
sessions and animation cases of `frame_oracle.rs`.
`automation/tests/css_animations.rs`: the probe pages of `css-animations`,
`-3b` and `-3c` at their seek times (134 frames, 64 with damage rectangles).
`paint/src/damage/random_pairs.rs`: random pairs of lists (rectangles,
lines, hit regions, backdrop filters, clips, opacity and blend groups,
transforms, filters; insertions, removals, wraps, unwraps, copies, swaps),
the region raster of the damage over the old raster against a full raster.
What the oracles found: the backdrop rule (a part that cut a
`backdrop-filter` box), the merge of overlapping parts that made a part touch
a box it did not hold, the border that reaches one pixel beyond its outer
edge, and the clip order of the builder. The review's random pairs found
that an opacity group whose bounds change gives other anti-aliasing to its
equal content (the rule above).

**Measurements** (release; before is the commit of feature 4a):

Hover (`swb --bench 3`, 8 x 6 grid, 3 rounds, the steps that changed the
state; before / after; `render` is new):

| Fixture | Change median ms | Change p90 ms | Region frames | Region px (median) | Diff ms |
|---|---|---|---|---|---|
| videojs (JS on) | 29.85 / 0.72 | 51.13 / 20.36 | 12 (21 empty) | 6,624 | 0.05 |
| videojs (JS off) | 30.38 / 0.22 | 31.44 / 13.47 | 12 (18 empty) | 6,660 | 0.04 |
| hacker-news | 0.71 / 0.04 | 0.93 / 0.37 | 30 (52 empty) | 171 | 0.03 |
| wikipedia-web-browser | 2.32 / 0.36 | 3.91 / 2.43 | 18 (54 empty) | 10,001 | 0.28 |
| github-swb | 14.32 / 11.82 | 21.23 / 18.25 | 69 (3 empty) | 194,428 | 0.16 |

"Empty" frames are hover changes whose list changed no pixel (a hit region,
the cursor). On github-swb every hover change also has the page's animation
in its frame, and a region of 19 % of the viewport costs 10.6 ms against 11.5
ms for a full raster: the coverage of the rounded clips of the file list
(0.93 Mpx, 3.7 ms) and the anti-aliased rounded boxes are drawn in full.

Animation frames (`--bench`, real time, animations on): videojs with
JavaScript on: 23 of 25 frames are full rasters (the hero reveal covers more
than half of the viewport), median 37.2 / 37.8 ms (raster 36.0 / 36.6 ms,
diff 0.06 ms); style 0.67 ms, display list 0.31 ms, layout 0. github-swb (JS
off): median 14.6 / 13.7 ms, raster 11.6 / 10.7 ms, every frame a region of
194,428 px. With JavaScript off no animation of videojs runs in the bench.
In virtual time (`damage_oracle.rs`), the videojs frames at the marquee are
regions of 1,232 x 117 px, and those of the hero at the top after the reveal
are empty or a few small rectangles.

GUI (release, own Xvfb, `gui-latency`, before / after):

| Trace | Move to frame median / p90 ms | Frames/s | Render median / p90 / max ms | CPU trace / idle % | Frames full / region / empty |
|---|---|---|---|---|---|
| videojs `--js`, `videojs-page`, 1280 x 840 | 48.5 / 68.6 → 1.8 / 61.0 | 19.9 → 23.4 | 9.2 / 30.3 / 41.0 → 5.1 / 8.6 / 41.8 | 39.9 / 0.7 → 19.6 / 0.0 | – → 61 (28 ms) / 218 (5.2 ms) / 184 |
| videojs `--js`, 12 wheel notches to the marquee and the bars, then 10 s idle | – | 58.6 → 58.8 | 9.0 / 9.3 / 42.7 → 4.2 / 5.3 / 41.1 | 65.5 / 60.7 → 40.0 / 30.0 | – → 26 / 831 (4.1 ms) / 5 |
| spinner page, `idle-10s` | – | 8.0 → 8.0 | 0.5 / 0.6 / 0.8 → 0.5 / 0.6 / 0.7 | 0.9 / 1.0 → 1.0 / 0.7 | – → 3 / 112 / 1 |
| marquee page, `idle-10s` | – | 60 → 60 | 0.2 / 0.4 / 0.6 → 0.3 / 0.4 / 0.8 | 4.4 / 4.3 → 4.5 / 4.7 | – → 3 / 841 / 1 |

Against the budgets of the diagnosis (section 5):

- frame time p90 at most 16 ms with the hero in view: met (render p90 8.6
  ms plus 0.5 ms of composition);
- frame time max at most 33 ms: missed by 9 ms (41.8 ms). The longest frames
  are full rasters (28 ms median, up to 42 ms): the hero reveal right after
  the load (about 28 frames of 40 ms in one second: opacity and transform of
  the hero's content, more than half of the viewport each frame) and every
  wheel step (a scroll of the viewport is a full raster);
- pointer move to frame p90 at most 50 ms: missed by 11 ms (61.0 ms). The
  moves that wait are those during the reveal and behind the full rasters of
  the wheel steps; the median fell from 48.5 to 1.8 ms;
- spinner at most 5 % CPU: met (1.0 %); marquee page at most 15 %: met
  (4.5 %); 60 frames per second while an animation runs: met on the
  marquee page and on videojs at the marquee and the bars (58.8 frames per
  second, the trace's frame pacing);
- videojs at the marquee and the bars, idle: 30 % CPU (60.7 % before). A
  region frame there costs 4.1 ms in release: the parts are small (the
  marquee strip and the bars), but the drawing that keeps its full extent is
  paid once per frame. Measured in the test build on these frames (relative
  shares): the layers of transformed groups (their full-raster layer and
  quad) about 60 %, the coverage of 2.1 Mpx of rounded clip layers about 18
  %, a backdrop filter about 11 %, filter layers about 10 %. A raster cache
  of layers (section 9, the next step) or limiting these operations to the
  parts would meet the budget; neither is done here.

Hostile cases (release, `just hostile`, through `--bench 1`; time and peak
memory, before / after; before has no damage, so its bench renders every
step in full):

| Case | Time | Memory |
|---|---|---|
| `damage-one-change-in-100000-items` | 3.97 s / 4.02 s | 289 / 289 MiB |
| `damage-10000-scattered-color-changes` | 12.25 s / 12.25 s | 169 / 168 MiB |
| `damage-large-blur-over-small-change` | 1.25 s / 1.26 s | 76 / 86 MiB |
| `damage-many-backdrop-filters` | 1.76 s / 1.25 s | 85 / 81 MiB |
| `damage-kind-change-every-item` | 5.32 s / 5.37 s | 112 / 115 MiB |

The cases cost what the load and the stages of the bench cost; the diff and
the merge add no visible time (the unit tests bound 100,000 items, 10,000
nested groups and groups at every item to well under a second). The whole
set: 591 cases pass, one known failure (`nest-div-100000`).

The load stages of `perf videojs` and `perf hacker-news` (5 runs) are
unchanged (videojs total 50.0 / 49.9 ms, Hacker News 4.84 / 4.90 ms); the
perf hover numbers now use the damage (videojs change median 30.1 → 0.15 ms,
p90 31.1 → 12.6 ms). The snapshot of the page fixtures and layout tests is
byte-equal, and the scores of `compare --all` and `compare --js --all` are
equal.

**Departures and what is not done.**

- Section 9 said damage under a filter grows by the filter's reach: it also
  makes the region draw the filter's content where the filter reads it.
- The coverage of clip paths and masks, anti-aliased paths, filter layers and
  transform layers cost their full extent once per frame (above); the
  multiply of a clip path's coverage could be limited to the parts with an
  exact reimplementation of tiny-skia's `apply_mask` (not done).
- The window buffer is converted and presented in full.
- A frame item is damaged as a whole when its documents change (no diff of
  the frame's own list).
- Budgets: a page whose full raster runs out of a budget gets a full raster in
  every frame after it (no frame is asked for by the budget itself, as
  before).

### 19. Done in feature 4c

Feature 4c meets the frame budgets of the diagnosis (section 5) on videojs:
the frame time maximum, the pointer move to frame p90 and the idle CPU with
the marquee in view. A page at rest renders as before (snapshot byte-equal,
scores equal); pixels change only while a `transform` or `opacity` animation
runs, by the rule of composited groups below.

**The profile** (release, a temporary profiler of self time per kind of
work, not in the code; `fixtures/js/videojs` with the real clock, 1280 x 800,
scale 1; "in T" = inside a transform layer):

| Frame | Total | Where the time goes |
|---|---|---|
| First paint (the hero at its 0 % keyframe, opacity 0) | 8 ms | the quad of the hero's transform layer 6.4 ms: the layer is transparent (its opacity group is 0), but the quad is drawn over 1169 x 565 px |
| Reveal, first and later frames (full: the hero is 76 % of the viewport) | 31 ms | in T: images 6.4, coverage and product of 10 rounded clips 5.7, opacity composite 3.1, clip composites 2.9, `backdrop-filter` 2.2, gradients 2.2, filters 2.1; the quad 2.9 to 4.4 |
| Wheel step at the hero, at rest (full) | 23.5 ms | images 6.6, clips 5.6 + 2.8, gradients 2.2, backdrop 2.2, filters 2.1 |
| Wheel step at 1600 (full) | 8 ms | images 3.3, SVG paths 1.5, clips 2.0 |
| Region frame at the marquee and the bars (84,826 px) | 3.7 ms | raster 2.8: three rounded clips 1.4 (coverage and product over the whole layer) and 0.4 (composites), the page's background image under the strip 0.4, scratch pixmaps 0.3, SVG paths 0.2; restyle 0.3, display list 0.3 |

Every pixel of the hero's layer depends on its scale (`s` times the
animated scale) and on the opacity of the group inside it, both new in each
frame, so no cache that is exact can help the reveal: it needs composited
groups (below). The 4b shares of the region frame (transform layers 60 %)
were measured in a debug build; in release no transform layer is in it.

**The raster cache** (`paint/src/raster/cache.rs`, `RasterCache`). The
rasterizer gets it from its `ImageSource` (`raster_cache`); `Images` holds
it, the top-level document one, the page's frame documents one shared
(`DocumentMaker::raster`). An entry is used only when all inputs of its
pixels are equal:

| Entry | Key | Kept when | Invalidation |
|---|---|---|---|
| Coverage of a clip path (`svg_clip.rs`) | the clip path (by value), the matrix to layer px (bits), the layer size | on its second miss (a fingerprint among the last 64 misses): a clip that moves every frame keeps nothing | a new key misses; LRU |
| Content of a composited layer (`composited.rs`) | the content items (by value), their media keys (images, frames, masks), the area and scale of the layer, the viewport scroll and fixed origin if the content has fixed or sticky boxes, the nesting of layers and frames | at once, in place of the entry of its slot (area, scale, nesting): changing content keeps one entry | a new key misses; LRU |

- The budgets: making an entry records what it charged (`Charges`: the
  peak of layer pixels, the mask, filter, transform and frame work, the path
  work charges in order) and is kept only if no budget left anything out
  and no SVG image was rendered or image level reduced in it. The skip
  flags of the strip are reset while an entry is made and merged back
  after, so a skip inside the entry shows even when an earlier item had set
  the same flag (the review found that hole); the SVG and reduction budgets
  count every rendering they drew from another size or left out
  (`FrameBudget::inexact`), including the closest rendering that
  `VectorCache::get` gives when one frame draws an SVG at more than 16
  sizes. A hit charges
  the same amounts, in the same order, and is used only if all of them fit
  into what is left; else the entry is made again. So the later items of a
  raster meet the budgets that they meet without the cache.
- Limits: 48 MiB and 256 entries per cache (the page: the top-level
  document's and the frame documents' together, 96 MiB). An entry counts
  its pixels, the copy of its layer's items in the key (88 bytes per item)
  and its charges; an entry larger than half the limit is not kept; the least recently used entries make
  room. The cache goes with the document, and is cleared on a change of
  the viewport or the scale (`DocumentState::set_view`).
- `RasterCache::set_enabled` (`Page::set_raster_cache`) switches it off for
  the oracle: the damage oracle (`tests/common/damage.rs`, so every session
  of `damage_oracle.rs` and `frame_oracle.rs`) and the probe pages of
  `css_animations.rs` compare each frame with a full render without the
  caches, bit for bit.

**Exact drawing limited to what matters.** Three pointwise operations now
skip the pixels whose result is known, with tiny-skia drawing the rest
gathered into one row (the operations are the same for each pixel):

- the product of a clip layer and its coverage (`raster/coverage.rs`):
  coverage 255 keeps the pixel, 0 clears it (a test checks every
  premultiplied value), other values go through `Pixmap::apply_mask`; in a
  region raster only the part of the layer inside the region's bounds is
  multiplied and composited;
- the composite of a layer with source-over and opacity 1
  (`raster/composite.rs`): opaque pixels are copied, transparent ones skipped
  (as before), the translucent ones are blended in one draw instead of runs
  of at most four per row; a layer with more than half translucent pixels
  is drawn whole, and so is a layer with more than 2^20 translucent pixels
  (the gathered copy stays small); the coverage product gathers at most
  2^20 pixels at a time;
- the scratch pixmap of a region raster is kept for the next one (its
  pixels outside the parts do not matter).

**Composited groups (item 3).** Needed for the reveal (above). The rule:

- *Which groups.* The transform group of an element (or pseudo-element)
  that has an animation of `transform`, `translate`, `rotate`, `scale` or
  `opacity` whose play state is `running` (also in its delay; not paused,
  finished or idle): `DocumentAnimations::composited_owners`. The builder
  sets `PushTransform::composited` on the element's own matrix group
  (`Scrolling::composited`). A page at rest has no such group, so its list
  is unchanged. A transition (feature 5) joins the same set. The display
  list is built again when the set changes, also when no style changes (an
  animation that ends on its held step); `paint_pending` sees the change.
- *Translations* stay on the exact path (a translation draws its items
  moved, without a layer), so the marquee is not composited.
- *The layer.* It covers the whole group (its bounds) at the device scale
  times the group's largest scale factor rounded up to a power of two (1
  for scales up to 1, at most 8), so that neither the animated transform
  nor the scroll offset changes it. A layer over the limits of transform
  layers (4 Mpx, 16,384 px a side) is drawn as a transform layer instead; so
  is a group with content fixed to the viewport or nested deeper than 8.
- *The opacity.* The element's own opacity group (the group's first item,
  ending with its last, without a blend mode or fixed content) is not drawn
  into the layer: its opacity applies when the layer is drawn. At opacity 0
  the layer is made and kept but not drawn: the first visible frame of a
  fade-in after a delay draws only the layer.
- *The draw.* As the layer of a transform group: bilinear filtering,
  anti-aliased edges, the enclosing clips; no snapping of the translation.
- *Misses.* A layer that the cache does not have is drawn whole and kept,
  also in a region raster: the frames after a change of the content (also
  while the group is invisible, before a fade-in) draw only the layer. A
  region raster whose layer missed in the last 3 rasters (content that
  changes in every frame, a spinner in a fading group) draws only the part
  that the parts read, as a transform layer, and keeps nothing.
- *Backdrop filters* in the group read the layer (a transform layer is a
  backdrop root already, ADR 0033; the folded opacity group was the root
  before, with the same content).
- *Start and end.* The first frame with the flag draws the content (for
  the hero: the first paint, while it waits at opacity 0). The frame after
  the end is the static render: the group has no flag then
  (`the_frame_after_a_composited_animation_is_the_static_render`).
- *Pixels against the exact path and Chromium.* The reveal of
  `hero-reveal-pixels-850` while it runs (virtual time, 850 ms;
  `hero_reveal_pixels_while_the_reveal_runs`): the box's inside equals
  Chromium's sample (196, 5, 8) exactly, the title's points are as on the
  static path (within 3); against the static frame of the same time, 400
  pixels differ, all on the box's edges (up to 69 levels: the layer is drawn
  at scale 1 and shrunk by 0.9995, where the static layer is drawn at
  0.9995). Chromium's samples are of paused animations (`seek()`), which are
  not composited in swb either, so `hero_reveal_pixels_at_seek_times` and the
  probe pages of `css-animations`, `-3b`, `-3c` are unchanged.

**The window buffer.** A softbuffer buffer of age 1 (it holds the last
frame: always on X11) gets only the window rows that changed (the page's
damage, the toolbar's rows, the bubble's old and new rows;
`Composer::take_changed_rows`) and is presented with that damage; other
buffers get every pixel, as before. Composition at the marquee: 0.41 ->
0.04 ms per frame.

**The scroll shift (item 4)** is not done: the budgets are met without it
(wheel steps at the hero 23 to 28 ms, the frame maximum).

**Measurements** (release, own Xvfb; before is e17203b):

| Budget | Before | After | Target |
|---|---|---|---|
| Frame time p90 (render + compose), `videojs-page` | 10.1 ms | 8.6 / 8.7 ms | 16 |
| Frame time max after the first paint | 42.5 ms | 27.2 / 27.3 ms | 33 |
| Pointer move to frame p90 | 60 ms | 28 / 12 ms (median 2 / 1) | 50 |
| videojs at the marquee and the bars, 10 s idle (`scroll12-idle10`) | 30.0 % CPU | 12.3 % | 15 |
| Spinner page / marquee page idle | 0.7 % / 4.7 % | 0.0 % / 1.3 % | 5 / 15 |
| Frames per second at the marquee | 58.9 | 61.1 | 60 |

Two runs of `videojs-page` (after): CPU 13.8 / 12.8 % (19.7 % before); full
frames 97 (render median 8.3 ms; 59 of 28 ms before: the reveal frames are
now 8 to 9 ms); the longest frames are the end of the reveal (24 to 27 ms,
the static render of the hero), wheel steps at the hero (23 to 28 ms) and
two region frames after the load (24 ms: the hero's player changes while the
hero waits at opacity 0, and its layer is drawn again). Region frames at the
marquee: 4.0 -> 1.6 ms (render median).

`--bench 3` (hover change median / p90 ms; animation frame median ms):
videojs with JavaScript in real time: hover 0.72 / 20.5 -> 0.75 / 29.1 (the
hovers over the player during the reveal draw its layer again), animation
37.8 -> 8.8 (raster 36.6 -> 7.7); videojs (JS off) hover 0.15 / 14.2 -> 0.15
/ 3.2; github-swb hover 11.6 / 17.6 -> 5.1 / 16.5, animation 13.5 -> 9.4
(raster 10.5 -> 6.5: the rounded boxes' clips are kept); Wikipedia 0.37 /
2.5 -> 0.37 / 2.1; Hacker News 0.04 / 0.35 -> 0.05 / 0.28. Peak cache size:
videojs 5.5 MB without JavaScript (8.2 MB with JavaScript and the
reveal), github-swb 5.3 MB (pixels only; since the review the limit also
counts the keys).

The load stages of `perf videojs` and `perf hacker-news` (5 runs) are the
same except the raster stage of videojs, 30.1 -> 23.3 ms (the bench
rasterizes the same viewport again, which finds the clips' coverage, and the
product of clips skips their inside).

Hostile cases (`raster_cache.py`, through `--bench 1`; time and peak RSS,
before / after; the peak of the cache): 2,000 composited groups (more than
the cache keeps: every lookup misses) 3.52 / 3.42 s, 94 / 96 MiB, 0.4 MB; a
composited group of 20,000 x 20,000 px (drawn as a transform layer) 1.36 /
1.35 s, 84 / 86 MiB; 1,000 rounded clips that move (no coverage kept) 3.16 /
2.41 s, 89 / 87 MiB, 0; a composited layer whose content changes every frame
1.10 / 1.10 s, 74 / 73 MiB, 2.6 MB (one entry).

**Departures and what is not done.**

- Composited groups change pixels while their animations run (above); a
  translation is not composited, and nor is an opacity animation without a
  transform (its layer is exact and drawn each frame).
- The coverage of masks and of clip paths inside a mask layer is not kept;
  mask layers (`raster/masks.rs`) are made in every raster.
- The end of a reveal is a full raster of the hero (24 to 27 ms in the GUI):
  the static render draws its rounded clips and images again.
- A layer that misses in a region raster is drawn whole (up to the cost of
  its content once), until it misses three times in a row.
- The scroll of the viewport is still a full raster (item 4 not needed).

### 20. Done in feature 5

Feature 5 runs CSS transitions: the typed `transition-*` longhands, the
rules of CSS Transitions 1 §3 in every style change event, the events,
`CSSTransition` in the API, and their frames through the paint-only path
and composited groups. Not done: the interpolation of `interpolate-size:
allow-keywords` (below). Measured in Chromium 148 with the probe file
`tools/probes/css-transitions.json` (19 cases: the 18 of feature 1 and
`outline-offset-values`, added for this section; three equal runs).

**The longhands** (`animation/transition_properties.rs`). The five
longhands moved from `inert.rs` to the third group of the `longhands!`
macro with typed specified values: `transition-property` (`all`, `none`,
names: lower case for a property that swb knows, custom properties and
unknown names as written), `transition-duration` and `transition-delay`
(`<time>` of `animation/time.rs`, so `calc()` and the math functions; a
negative math duration computes to 0), `transition-timing-function`
(`parse_easing` of `animation/easing.rs`, so `linear()` and `steps()`),
`transition-behavior`. The computed values are one optional shared record,
`ComputedStyle::transitions: Option<Arc<TransitionStyle>>` (`None` while
all are initial); the lists keep their lengths (`getComputedStyle` shows
`transition-duration: 0.2s, 0.2s, 0.4s` for four properties, as Chromium:
`attribute-popup`), and `TransitionStyle::matching` finds the last item of
`transition-property` that names a longhand (by itself, an alias, a
shorthand that contains it, or `all`) or a custom property, with the other
lists repeated to its length (CSS Transitions 1 §2.1). The CSSOM forms go
through the texts of the animation properties (`Ser::Animation`): the
specified values keep the written times and easings (`300ms`,
`calc(0.4s)`, `steps(4)`), the computed values are in seconds and in the
computed easing forms, the `transition` shorthand is serialized as before.
`Ser::TransitionProperty` and `Ser::Easing` and the text values of
`inert.rs` are gone. `--animations off` resets the longhands as before.
`interpolate-size` (`numeric-only | allow-keywords`, inherited) parses,
computes and serializes; `CSS.supports` is true.

**The records** (`animation/records/transitions.rs`). A transition is an
`AnimationRecord` with a `TransitionState`: the property
(`TransitionTarget`: a longhand or a custom property), the start and end
values and the reversing-adjusted start value (`TransitionValue`: the
computed value of a longhand alone, `ComputedSlot`, or the tokens of a
custom property; fix round below), the reversing shortening
factor and the transition generation. Its effect has the transition's
delay and duration, one iteration, `fill: backwards` and the timing
function as the effect easing (Chromium's `getComputedTiming()`). The
records of an owner's running transitions are a list of their own
(`DocumentAnimations::transitions`), in composite order (generation, then
property name in code point order). The record limit of section 11 counts
transitions (`reserve_id`). A transition that reaches its end at an update
leaves its owner's list, queues `transitionend`, and its record goes at
the next update (`finished`), after the bindings resolved its `finished`
promise. Its end value is kept in the owner's set of completed
transitions until the owner's next style change event (rules 1 and 2:
the before-change style of that event can still hold a value of a tick
before the end; the end value counts instead).

**The rules in each restyle path.** The cascade computes the base style,
then the style with the animated values of the CSS animations (the
after-change style), then, in a style change event, runs
`DocumentAnimations::sync_transitions` for the element or pseudo-element
(`cascade/animated.rs`, `transition_effects`), and computes the style a
third time with the values of the running transitions when there are
any (they win over the animations of their properties):

- Rule 3: a running transition whose property has no matching item is
  cancelled. Rule 4: a running transition whose end value differs from the
  after-change value is cancelled when its current value equals the
  after-change value, the pair cannot transition, or the combined duration
  is 0; it is reversed when the after-change value is its
  reversing-adjusted start value (factor |output × old factor + 1 − old
  factor|, the duration and a negative delay times the factor); else it
  is replaced by a transition from its current value. A transition that a
  script paused at its end (after phase) counts as completed: it goes
  without an event and rule 1 applies.
- Rule 1: for each candidate property (the longhands that the items name,
  all transitionable longhands for `all`, the named custom properties)
  without a running transition, a value that differs between the
  before-change and the after-change style starts a transition when the
  combined duration is above 0 and the pair is transitionable
  (`is_interpolable`, `custom_is_interpolable`; with `allow-discrete` any
  pair). The animation and transition properties, `direction`,
  `unicode-bidi` and `will-change` never transition. A property that a CSS
  animation of the element drives now starts no transition. The after-change
  style goes through the fix-ups of the element first (blockification, the
  border widths of `border-style: none`), as the before-change style did,
  or `transition: all` would see changes that are none.
- An after-change `display: none` cancels every transition of the
  element (unresolved time) unless a `display` transition keeps it
  rendered (`allow-discrete`: `display` is the other value for the whole
  interval, then the transition's end makes it `none`; probe
  `transition-behavior-discrete`).

Where the before-change style comes from (`ChangeSource` in `cascade.rs`,
`TransitionInputs` in the API):

- *The targeted restyle* (state changes, DOM changes; ADR 0041, 0042): the
  element's style in the map it starts from (`visit_targeted` hands the
  old style of the element and of its pseudo-elements to the styler). A
  restyle for the animation ticks alone (no state or DOM change, no
  queried style) is a style change event only for the children of
  elements whose style changed in it (fix round, below).
- *The full restyle with a previous map* (`compute_styles_from`): the
  previous map. The first styles of a document and the check modes have
  none.
- *The paint-only path* (feature 4a) is the targeted restyle; a transition
  of a paint property gives paint-only frames (`paint_only.rs`,
  `transitions_of_colors_stay_paint_only`).
- *The style of one element for a query* (`getComputedStyle` while the
  styles are stale, ADR 0028 section 9): when an element of the chain can
  start a transition or has one, the query runs the style pass of the
  document instead (the read is a style change event, as in Chromium);
  else the chain's styles are kept as the newer before-change styles of
  their elements (`DocumentState::queried_styles`) for the next style
  change event (`inline-style-spring`: `transition: none`, a transform, a
  read, then `transition: transform 600ms` and no transform starts the
  transition from the read's value). `getAnimations()` runs the style pass.
- *The style step of the rendering update* computes the styles when a
  style that can start a transition has been computed in the document
  (`DocumentAnimations::transitions_seen`) and the styles are stale, so a
  class change by a script starts its transitions without a read. The
  transitions of that step start in that frame (their start time is the
  frame's time; `start_pending_transitions`), and their first update
  comes at the next frame. A transition that a script's read started is
  pending until the next update (`pending`, `currentTime` 0, as in
  Chromium).

Style sharing: see the fix round below (an element takes a shared style
when its style change event would do nothing); a shared style with no
transitions cancels the transitions of an element that gets it
(`retain_transitions` in the sweep).

**Discrete cases** (section 4): `display`, `content-visibility` and
`overlay` transition only with `allow-discrete` (a `none` or `hidden` end
keeps the other value for the whole interval); unregistered custom
properties, and registered ones whose syntax does not interpolate the
pair, only with `allow-discrete` (they flip at 0.5); every other discrete
pair of section 4 too. Measured: `outline-offset` computes, also while it
transitions, to a value snapped as a border width with its sign (`1.5px`
is `1px`, `0.75px` is `1px`, `-1.5px` is `-1px`); swb now snaps it in the
computed value and in the interpolation. A `calc()` with a percentage that
is 0 keeps it in the computed value (`background-size: calc(0% + 20px)`,
the start of a transition from a length to a percentage).

**Events and their order** (section 6). `transition_events_of` is the
table of CSS Transitions 2 §5.1: `transitionrun` (scheduled at the start
time, so a delayed transition's `transitionrun` comes with the
`transitionrun` and `transitionstart` of the others: `events-run-start-end`),
`transitionstart` and `transitionend` (scheduled at their interval
boundaries), `transitioncancel` (the active time at the cancel). The queue
is sorted by scheduled time (unresolved first), then the cancels before
the other events of the same time, then transitions before animations,
then the owners in tree order with their pseudo-elements, then (for
transitions) generation and property name, (for animations) the position
in `animation-name` (`events-order-many-properties`: the transitions of
`e1` and `e2`, then the `animationstart` of `e1`). The settled cancel
order of `events-cancel-and-reverse`: the cancel by `display: none` is
unresolved and first; the reversal of `e1` and the `transition: none` of
`e3` share the time of the style change and come in composite order
(`e1`, `e3`; Chromium gives `e3`, `e1` in the recorded run and either in
others; the test accepts both); the new transition of `e1` follows.
`TransitionEvent` (`EventInterface::Transition`, `propertyName`,
`elapsedTime`, `pseudoElement`), `bubbles` and `cancelable` true.

**The API** (section 7). `getAnimations()` of the document and of
elements (`subtree` too) lists the transitions before the animations
(`get-animations-order` of `css-animations.json` passes). A transition's
wrapper is a `CSSTransition` (`transitionProperty`, kept after its record
went); every member of `Animation` works on it (`pause()` and
`currentTime` for the probe's `seek()`, `finish()`, `cancel()` (the
transition leaves its owner), `ready`, `finished`, `effect` with
`getTiming()` and `getComputedTiming()`). Base UI's `Promise.all` of the
`finished` promises resolves when the transitions end and rejects with an
`AbortError` when a reversal or `display: none` cancels them
(`transition-api-finished`). Departure: `getKeyframes()` of a transition
is empty (Chromium lists the start and end values).

**Frames and cost.** Transitions are records, so the frame scheduling of
section 8 (the visibility test by the reach of the property, offscreen and
paused ones ask only for their event times), the paint-only path of
section 17 and the composited groups of section 19 (`composited_owners`
includes running transitions of `transform`, `translate`, `rotate`,
`scale` and `opacity`) apply as for animations. A frame of a running
transition costs what a frame of an animation of the same property costs:
the owner is marked, restyled with the transition input, and its paint
follows the same path.

**`interpolate-size`.** Not done: the property exists, but a length
against `auto`, `min-content`, `max-content` or `fit-content` does not
interpolate (it changes at once, as with `numeric-only`). The design for
the follow-up: the computed value keeps the keyword, the length and the
progress (as `InterpolatedTransform` keeps two transform lists), layout
resolves the keyword's size where it resolves the keyword today
(`resolve_size` in block, flex and grid layout, the intrinsic sizes) and
mixes it linearly; `getComputedStyle` gives the used size (the element has
a box). Rows 1 to 3 of the probe case wait (`WAITS` in
`css_transitions.rs`).

**Measurements** (release; before is the commit of feature 4c;
`docs/performance.md`, "CSS transitions"). The load stages of `perf
videojs` and `perf hacker-news` (5 runs, animations off) and the load style
stage of `--bench 3` with animations on are the same within noise (the
budget: at most 50 ms on a page with transitions, 0 on others). Hover change
median / p90 with animations on: videojs with JavaScript 0.85 / 25.0 ->
1.24 / 26.0 ms (its hovers start transitions, paint-only), the others equal.
GUI, `videojs-page`: pointer move to frame p90 28 -> 20 ms (budget 50);
frame time p90 10.1 -> 10.3 ms (budget 16); CPU idle after the trace 0.7 %
both. The frame maximum of the whole trace (57 ms, both) is a frame of the
load; section 19 measured 27 ms after the first paint. 60 frames per
second while only a transition runs was not measured separately. Hostile
cases (`hostile_cases/transitions.py`, time and peak RSS): the
`transitionend` ping-pong 0.05 s, 117 MiB (bounded by one round per frame);
a class change that starts four transitions on 10,000 elements 0.35 s, 229
MiB; a `transition` list of 10,000 entries 0.05 s; a property toggled
every 16 ms for 1,000 rounds (a reversal each) 0.05 s; an inherited
`color` that starts a transition on 100,000 elements 2.3 s, 1,104 MiB (the
transitions keep the styles of their start and end values). The whole set:
601 cases, 599 ok, the known failure `nest-div-100000`, and
`large-message-to-a-frame-in-a-loop-js-on` inactive once (it passes alone,
with this binary and the base one).
Snapshot byte-equal; `compare --all` and `compare --js --all` equal but
for `js-ars-technica` (its A/B pixels) and `js-craigslist-posting` (0.63 ->
1.00: the first base run ran under load; run again, the base gives 1.00).

**Tests.** `crates/automation/tests/css_transitions.rs`: every row of
`css-transitions.json` but rows 1 to 3 of `interpolate-size`, one pair of
lines accepted in either order (`EITHER`), no departure. The helpers of
the probe values moved to `tests/common/animation_values.rs`.
`cascade/transition_tests.rs`: the start rules, reversal, replacement,
cancels (rule 3, `transition: none`, `display: none` with its unresolved
time), discrete cases, animations that drive a property, composite order,
event times, completed transitions, shorthands. `transition_properties.rs`
tests the grammar and texts. `damage_oracle.rs`
(`transitions_of_hovers_and_class_changes`: hovers and a class toggled
every 170 ms with transitions of colors, `transform`, `opacity`, `scale`
and `height`, at scales 1 and 1.5), `paint_only.rs`
(`transitions_of_colors_stay_paint_only`), `videojs_transitions.rs` (the
hover transitions of the search button and a player control on the JS
fixture: start and end values, events per property).

**Fix round of feature 5** (review findings). Measured in Chromium 148
with `tools/probes/css-transitions.json` (17 cases added; three equal
runs each).

*Children of elements whose inherited values change.* A child whose
parent transitions or animates an inherited property (`color`) starts its
own transition, as in Chromium. The rule, from the probe cases
`inherit-*`:

- The after-change style of an element inherits from its parent's style
  without the values of the CSS animations and transitions of the parent
  and of the parent's ancestors (`DocumentAnimations::transition_parent`:
  kept for an element with element children, or with pseudo-elements that
  can have transitions, while it differs from the element's style in an
  inherited value (in any value once a style of the document that can
  start a transition takes a non-inherited value with `inherit`:
  `DocumentAnimations::reset_inherited_seen`), once a style of the
  document can start a transition of an inherited value; second fix round below; not a field of `ComputedStyle`,
  whose 8 more bytes put each style into the next size class of the
  allocator, +88 MiB on `color-mix-200000-distinct-colors`). So the child's transition runs towards the parent's end value
  (`inherit-parent-child-start`: from `rgb(0, 0, 0)` to `rgb(255, 255,
  255)`), and towards the parent's value without its animation when the
  parent has a CSS animation (`inherit-animated-parent`: `#p` without a
  color, `from rgb(4, 4, 4) to rgb(0, 0, 0)`; with `color: red`, to
  `rgb(255, 0, 0)`). A grandchild through a child without a transition
  does the same (`inherit-grandchild`).
- The restyle of the animation ticks is a style change event for the
  elements whose parent's style changed in it, and for the pseudo-elements
  of an element whose style changed (`TransitionInputs::inherited_only`);
  the animated elements themselves have none. A child is restyled when an
  inherited value of its parent changes, not when only the parent's
  kept style does: so in the parent's style change event the
  parent's value is still its start value, the child sees no change, and
  it starts at the next frame, where the parent's value first changes
  (`inherit-parent-child-start`: one frame later). A child that the event
  restyles for its own reasons starts with the parent
  (`inherit-child-restyled`). A seek of the parent's transition starts
  the child's (pending) at the style change of the seek
  (`inherit-parent-child-seek`). Later frames keep the child's transition
  (its end value is the parent's end value); a reversal of the parent's
  reverses the child's (`inherit-parent-reverse`).
- Frames: in a headless run the frames of an animation come only at its
  event times; a transition or animation of an inherited value now gets
  the frames of a painted one until a tick's restyle changes the owner's
  style (`AnimationRecord::inherited_due`; second fix round below: for CSS
  animations and transitions alike, only for owners whose styles others
  inherit from, and only once a style of the document can start a
  transition of an inherited value), and the style step of the rendering update runs when owners
  that can show wait for a restyle in a document whose styles can start
  transitions (`ticks_can_start_transitions`). The transitions that the
  tick's restyle starts start in that frame (`start_pending_transitions`).
  A painted frame restyled these owners before; it costs nothing more.

*Memory per transition.* `TransitionValue::Longhand` holds the computed
value of the one property (`ComputedSlot`: the field or fields that hold
it, copied out of the style by `LonghandId::value_of` and put back into
scratch styles by `put_value` for the functions on whole styles), not the
before- and after-change styles; the set of completed transitions keeps
such values too. The record limit of section 11 is per page
(`RecordRoom`: the documents of a page share it; a document gives its
records back when it goes). The hostile case of 100,000 inherited `color`
transitions: 2.3 s and 1,100 MiB before, 2.3 s and 574 MiB after (about
5 KB less per transition).

*Cost on pages that declare transitions.* An element without running
transitions whose after-change style (with its fix-ups, built on the
stack) equals its before-change style has nothing to do: no candidate
list, no allocation. `all` alone iterates a static list of the
transitionable longhands and compares the two styles per longhand; other
lists mark longhands in a bit set. The custom properties of a list count
only when the two custom property maps differ (one comparison). Style
sharing stays on: an element takes a shared style when the style can
start no transition, or when the element has no running transition and
its style change event would do nothing (no before-change style, or one
with the values of the shared style for the properties of its
`transition-property`, second fix round); a style with running
transitions is not shared. The computed `transition-duration`, `-delay` and
`-timing-function` lists of the elements of one declaration are one list
(a cache of the last specified list per longhand). Measured (release,
20,005 elements, `--bench 3`, load style / restyle median with unchanged
styles in ms; `--animations off` / on before / on after):
`*{transition:all .2s}` 11.8 / 40.9 / 11.7 and 11.4 / 262.9 / 15.5; four
properties on a class 11.7 / 41.3 / 11.6 and 11.6 / 59.8 / 16.0. A list of
10,000 custom properties on 3,000 elements and one class change on the body
(hostile case `transition-list-10000-entries-3000-elements`): 0.50 s and
2,063 MiB before (over its memory limit), 0.15 s and 78 MiB after; the
class change itself 2,095 ms before, 79 ms after.

*`CSSTransition` after its end.* A held wrapper of a transition whose
record went after its end reads `playState` `finished` and its end time as
`currentTime` (the engine hands the end time with the released id;
`api-promises-after-end`). Departure: `cancel()` releases the record, so
`play()` on the held wrapper does not replay the transition (Chromium
replays it from 0; `api-after-end-and-cancel` rows 9 and 10 in
`SWB_ROWS`; backlog: keep the record of a cancelled transition while a
script holds its wrapper).

*Nits.* `transition: width -1s` has the duration 0s and the delay -1s (a
negative literal cannot be the duration; a math function is checked at
computed-value time). A `background-size` `calc()` whose two parts are 0 with a
percentage computes to `0%` (`calc-zero-percentage`; a `translate` of such a value stays `0px`, `registered-tl-3`). `document.getAnimations()`
lists the animations of the document tree only; `ShadowRoot.getAnimations()`
lists those of its shadow tree (`shadow-tree`).

*`interpolate-size` (design notes for the follow-up).* (a) A reversal
needs the current value of a mix: the computed value must nest (a mix of
a mix), or the transition keeps (from, to, progress) and resolves the
current value at the reversal. (b) The frames of such a transition need
layout, not the paint-only path: its reach for the visibility test and
the frame path must say `Layout`.

*Tests.* `css_transitions.rs` runs the 17 new cases (`SWB_ROWS`:
`getKeyframes()` and the replay after `cancel()`); the hostile case
`transition-list-10000-entries-3000-elements`.

*Measurements of the fix round* (release, the feature's binary before /
after). `perf videojs hacker-news --runs 5` and `--bench 3` on videojs
(with and without JavaScript), github-swb and wikipedia-web-browser: equal
within noise. GUI, `videojs-page` (own Xvfb): frame render p90 8.3 / 8.3
ms, render max after the first paint 26.6 / 27.2 ms (budget 33), pointer
move to frame p90 3.7 / 22.9 ms (budget 50; it varies between runs), 60
frames per second in the runs of continuous frames (hover transitions and
the reveal), CPU idle after the trace 0.2 % both. Snapshot byte-equal;
`compare --all` equal, `compare --js --all` equal but `js-ars-technica`
(its A/B pixels, both binaries).

**Second fix round of feature 5** (delta review findings). Measured in
Chromium 148 with six cases added to `tools/probes/css-transitions.json`
(three equal runs).

*Style sharing and the kept style.* An element that takes a shared style
runs no cascade, so it set no kept style: the children of the second and
later of identical siblings inherited the animated value and restarted
their transitions at every frame (`inherit-shared-siblings`), and a parent
that took a shared style after its animation ended kept a stale style
(`inherit-shared-parent-stale`). The sharing cache now holds the kept
style of each shared style, and an element that takes one sets it (or
drops its old one). The sharing key holds the address of the parent's
kept style (held by the cache entry, so the address is not reused) and,
under a kept style, whether the element keeps one itself: elements with
one key keep one style.

*Which elements keep a style, and for whom.* An element keeps a style
for its children only when it has element children or pseudo-elements
that can have transitions (some rule of the document for a
pseudo-element sets a `transition-*` longhand:
`Stylist::has_pseudo_transitions`; a test per element would need the
pseudo-elements' styles, which come after the element's), and only once
a style of the document can start a transition of an inherited value
(`TransitionStyle::may_transition_inherited`: an item that is `all`, a
custom property or (a shorthand of) an inherited longhand;
`DocumentAnimations::inherited_transitions_seen`). So `::before`,
`::after` and `::marker` of an element whose children are text start with
it, in the same event (`inherit-pseudo-text-only`, `inherit-pseudo-marker`;
they started one frame late). The per-frame headless updates of records of
inherited values (`inherited_due`) need the same flag.

*Lists that cannot name an inherited value.* An element whose
`transition-property` names no inherited value, and that takes no
non-inherited value with `inherit` (`ComputedStyle::inherits_reset`, set
by the cascade; the review of the second fix round found that
`background-color: inherit` with `transition: background-color` under a
transitioning parent had lost its own transition), takes its after-change
style from its parent's style as it is (no second compute from the kept
style), and keeps the style sharing in the tick's restyle: it takes a
shared style when its before-change style has the values of the shared
style for the properties of its list (`same_transitioned_values`), not
only when the two styles are equal. A list that can name an inherited
value does not take a shared style under a parent with a kept style (its
after-change style differs from the shared style).

*Frames until the value changes.* `inherited_due` stayed set only until the
time passed the delay; the first frame of a long animation (`color` over
10 s) does not change the 8-bit value, so the child's transition waited
for the next script read in headless runs
(`inherit-long-animation-parent`, `-no-read`). It is now cleared when a
tick's targeted restyle changes the owner's style
(`DocumentAnimations::note_value_changed`), when a `steps()` animation is
past its first step, or when the record leaves its active phase. It is set
for CSS animations and transitions alike only when the owner's style has
dependents (`AnimationTarget::children`; animations: `note_no_dependents`).
A full restyle during the ticks does not clear it: the frames go on until
the end (or the next targeted tick that changes the style).

*Departures.*
- A child that takes a non-inherited value with `inherit` and transitions
  it starts its transition one frame after the parent in the GUI (the
  parent's computed value does not change in the parent's event, so the
  walk does not restyle the child); Chromium starts both in the same event
  (`r-inherit-kw`, `r-inherit-kw-bw` of the review's probes). In headless
  runs the child starts at the next script read, because a transition of a
  non-inherited value gets no extra frames (`inherited_due`).
- When the first style that can transition an inherited value comes in
  the same event as the parent's change, the child's transition can be
  cancelled and run again once (5 events where Chromium has 2; review
  probe `n-enable-late`), or start one or two frames later
  (`n-enable-same-event`); the values are right.
- A value computed from an inherited value of the parent (`em`, `var()`):
  Chromium restarts the child's transition at every frame while the
  parent's value changes (cancel and run; review probes `d-font-size`,
  `d-custom`). swb restarts it too when the child's list names no
  inherited value (`#c{width:2em;transition:width}`: the after-change style
  takes the animated parent), and runs one transition to the end value when
  it names one (`color: var(--x)` with `transition: color`; a registered
  inherited `--x` transitions on the parent): the kept style applies to the
  inherited value and to what is computed from it. No probe row shows this:
  the number of restarts depends on the frame timing.
- Before the first style of the document whose list can name an inherited
  value, no element keeps a style: if that first style comes in the same
  style change event as the parent's change, the child's after-change
  style takes the animated parent once (the next frame corrects it).

*Tests.* `css_transitions.rs` runs the six new cases (no `SWB_ROWS`), and
`shadow_roots_list_the_animations_of_their_trees` (nested shadow trees).
Unit tests: `cascade/kept_style_tests.rs` (shared siblings, the stale
style, the cleanup when elements go or become `display: none`, the frames
until the value changes, owners without children, sharing with lists of
other properties and of inherited values, a listed value that changed,
the cancel of a running transition whose property leaves the list, the
end times of released transitions) and
`transition_values_round_trip_and_interpolate_as_whole_styles` (every
transitionable longhand: `put_value` of `value_of` gives the value back,
and the interpolation through the scratch styles equals the one of whole
styles; 2 x 3 progress values). `put_value` stops a debug build at a slot
of another type.

*Measurements of the second fix round* (release; before is the first fix
round's binary). 20,000 elements under a body whose `color` animates, a
script reads a style in each `requestAnimationFrame` (frames in 3 s,
headless, 181 is the frame rate): no `transition` 181 / 181;
`.a{transition:opacity .2s}` 34 / 181 (93 ms / 17 ms per frame);
`*{transition:color .2s}` 32 / 35 (98 / 92 ms per frame: every child
computes its after-change style from the kept style). Without the reads:
34 / 181 and 39 / 38. The pages of the first review, `perf videojs
hacker-news --runs 5`, `--bench 3` on the fixtures and the GUI
(`videojs-page`) are equal within noise.

### 21. Done in feature 6

Feature 6 is the exit test of videojs.org
(`crates/automation/tests/videojs.rs`, roadmap M13). It found these
defects of swb; each is fixed with a probe row (Chromium 148, three equal
runs) and a test.

*Vendor-prefixed `hyphens`.* `-webkit-hyphens`, `-moz-hyphens` and
`-ms-hyphens` were aliases of `hyphens`; Chromium 148 has none of them
(probe `vendor-hyphens` of `tools/probes/css-videojs-6.json`). Tailwind's
`@supports (-webkit-hyphens: none) and not (margin-trim: inline)` fallback
for Safari therefore applied in swb, and its `*{--tw-scale-x: 1, ...}` in
`@layer properties` beat the utilities: a popup's open transition started
at scale 1 where Chromium starts at 0.95. The aliases are gone from the
property table and the CSSOM.

*The set of completed transitions* (CSS Transitions 2,
[starting of transitions](https://drafts.csswg.org/css-transitions-2/#starting)).
Three defects of feature 5, found once the test stopped reading the style
after a script's `finish()`:

- A transition that a script finished (or seeked to its end) and that the
  next style change event finds in its after phase now puts its end value
  in the set, as a transition that ended by time does. Before, the
  before-change style still held the old animated value, and the event
  started a transition from it (probe row `finish-then-style-change`:
  Chromium starts none). A property with a completed transition is also a
  candidate where the before-change style holds the after-change value:
  `finish()` and then the old class again starts a transition from the end
  value back (rows `finish-then-restore`, `finish-read-then-restore`,
  `seek-end-then-restore`; found by the second review).
- The set ends at every style change event of the element, also one whose
  after-change style has no transitions (`transition: none`). The page's
  spring sets `transition: none` for the pull and a transition for the
  release; a stale end value of the first release made the second release
  start none (probe row `release-twice`). `may_animate` answers yes for an
  element with completed transitions in a style change event, and
  `transition_effects` forgets them when the event has nothing else to do.
- The cascade of an element with container units runs twice in one event,
  and a style pass can follow a restyle of the animations, with the same
  before-change style. The first run consumes the set and drops the
  finished records, so the second run started a transition from the stale
  value. `DocumentAnimations::repeated` keeps the set that the last event
  of an element used, with its before-change style (`Arc`); a run with the
  same `Arc` uses it again. The `Arc` keeps the style alive, so its address
  cannot come back for another style (a first version that kept only the
  address failed one run in three). An element that leaves the document,
  or whose next event has nothing to do, drops its set.

*The used color scheme.* A change of the used color scheme
([Color Adjust 1 §2](https://www.w3.org/TR/css-color-adjust-1/#color-scheme-prop))
starts a transition of `color` from a value to the equal value on every
element whose `transition-property` names `color` or is `all`, and of no
other property (probe rows `color-scheme-*`: `color-scheme: light only` to
`dark` gives keyframes `rgb(255, 255, 255)` to `rgb(255, 255, 255)`;
`light` to `light dark` changes no used scheme and starts none).
videojs.org's theme change sets `.dark { color-scheme: dark }`, so the
player's buttons start such transitions. `sync_transitions` compares the
used schemes of the before- and after-change styles (with the user's
preference; the page's `color-scheme` meta is not read, backlog) and
treats `color` as changed.

*Focus by script.* A script's `focus()` or `blur()` did not schedule a
rendering update, so the transitions of `:focus-visible` rules started at
the next read of the styles (probe row
`blur-by-script-starts-transition`). The engine now schedules one when a
script request leaves the styles stale.

*Test tolerances* (section 12). `getComputedStyle` of a color in the
middle of an interpolation: Chromium writes `rgba(r, g, b, 1)` where the
alpha rounds to 1 (swb: `rgb(r, g, b)`) and a float alpha where swb keeps
the 8-bit value (alpha 71 is `0.28` in swb, `0.277837` in Chromium).
`tests/common/animation_values.rs` treats the two forms as equal and
compares alphas within one 8-bit step (1/255).

*Wheel handlers* (ADR 0028, "Wheel and scroll"): with only passive
`wheel` listeners on the path, the page scrolls before the event; the
page's spring reads the new `scrollY` in its passive listener and starts
at the same wheel as in Chromium.

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
