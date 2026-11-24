# ADR 0037: Popover, dialog and the top layer

- Status: accepted
- Date: 2026-11-13

## Context

M10 feature 6 (roadmap, M10 step 3): the `popover` attribute and its states,
`showPopover`, `hidePopover`, `togglePopover`, `popovertarget` and
`popovertargetaction` (also with JavaScript off), `beforetoggle` and
`toggle`, light dismiss, `:popover-open`, `<dialog>` (`show`, `showModal`,
`close`, `returnValue`, the `open` attribute, `cancel` and `close`,
`form method=dialog`), modal inertness, `::backdrop`, `:modal`, the paint
order and hit testing of the top layer, and Escape (close requests). JS on
and off.

What exists (read in swb's code at a7198f2):

- **Selectors.** `ElementState` (`crates/css/src/selector/matching.rs`) has
  the bits `OPEN` (25), `MODAL` (26), `FULLSCREEN` (27) and `POPOVER_OPEN`
  (28); bits 29 to 31 are free. `:open`, `:modal`, `:popover-open` and
  `:fullscreen` parse (`STATE_PSEUDO_CLASSES`). `::backdrop` parses
  (`PseudoElement::Backdrop`). `DomElement::attribute_form_state`
  (`crates/style/src/element.rs`) sets `OPEN` for `details` and `dialog`
  with an `open` attribute. Nothing sets `MODAL` or `POPOVER_OPEN`, so
  `:modal` and `:popover-open` never match.
- **Style.** The UA sheet (`crates/style/src/ua.css`) has `dialog` (block,
  `position: absolute`, `inset-inline: 0`, `fit-content`, `margin: auto`,
  `border: solid`, `padding: 1em`), `dialog:not([open]) { display: none }`,
  `[popover]:not(:popover-open):not(dialog[open]) { display: none }` and
  `[popover]` (`position: fixed`, `inset: 0`, `fit-content`, `margin: auto`,
  `border: solid`, `padding: .25em`, `overflow: auto`). There is no
  `::backdrop` and no `dialog:modal` rule. `RuleTarget::of`
  (`crates/style/src/stylist.rs`) keeps rules for `::before`, `::after`,
  `::marker` and `::placeholder` and drops all other pseudo-element rules,
  `::backdrop` included. `PseudoKind` (`style_map.rs`) has no backdrop.
  `getComputedStyle(el, '::backdrop')` gives the element's inherited values
  (`PseudoElement::Unstyled`).
- **`inert`.** The `inert` attribute is only reflected (`html.idl`); no code
  reads it. (`crates/style/src/properties/inert.rs` is about CSS
  properties without effect, not about this.)
- **Layout.** `layout_with` builds the box of the root element, lays it out
  and then places out-of-flow boxes (`positioned.rs`,
  `place_out_of_flow` with a `Scope { absolute, fixed }` that follows
  transforms and filters). `contain: paint` does not make a containing
  block in swb. The box tree has `MAX_BOX_DEPTH` 256.
- **Paint and hit testing.** `build_display_list`
  (`crates/paint/src/display_list/builder.rs`) paints the root stacking
  context; positioned boxes are deferred per context and sorted by
  `(z, tree order)`. `hit_test_with` walks the items in paint order; the last
  region that contains the point wins. `HitRegion` items come from
  `paint_box` for visible boxes without `pointer-events: none`.
- **Input.** `page/input.rs`: `mouse_down` and `mouse_up` dispatch pointer
  and mouse events (JS on) or run the engine's activation
  (`page/forms.rs`, `activate`, JS off). `key_down` dispatches `keydown`
  and then runs `key_default` (Tab, Enter, Space, Ctrl+A, scroll keys).
  Escape has no default action in the page; the frame tree records
  activations (`Frames::note_activation`, sticky activation only) and
  Escape is no activation.
- **Forms.** `forms/submit.rs` logs "dialog forms are not supported" for
  `method=dialog` and does not submit.
- **Events.** No `toggle` event is fired anywhere: `<details>` changes its
  `open` attribute without events (JS on: `crates/web/src/html/activation.rs`,
  JS off: `page/forms.rs`). The handler names `onbeforetoggle`, `ontoggle`,
  `oncancel`, `onclose` and `oncommand` exist.
- **Bindings.** `HTMLDialogElement` has only `open`
  (`crates/web/idl/html.idl`); `HTMLElement` has no popover members;
  `HTMLButtonElement` and `HTMLInputElement` have no `popoverTarget*`;
  `ToggleEvent` does not exist.
- **DOM.** `ElementData` is 64 bytes (pinned by a test). Its
  `shadow_flags: u8` (ADR 0036) uses the bits `HOST` and `SLOT`; bits 2 to
  7 are free. `Document` has per-arena side state behind `Option<Box<_>>`
  (`shadow`), which costs one test on pages that do not use it.
- **Frames.** Each document has its own `DocumentState`
  (`crates/engine/src/document/mod.rs`; ADR 0031), with its own style,
  layout, display list and hit testing.

Sources: the HTML Standard (§6.12 "The popover attribute": the states,
"check popover validity", "show popover", "hide popover algorithm",
"topmost popover ancestor", "nearest inclusive open popover", "hide all
popovers until", "light dismiss open popovers", "topmost clicked popover",
"popover target attributes", "queue a popover toggle event task",
`ToggleEvent`; §4.11.4 "The dialog element": `show`, `showModal`,
`close`, `requestClose`, "close the dialog", "dialog focusing steps",
"previously focused element", `closedby`, `returnValue`; §6.6.1 "inert
subtrees" and "blocked by a modal dialog"; §6.4 "Close requests and close
watchers": the close watcher manager, groups, "request close", "process
close watchers"; §4.10.21.3 form submission with method `dialog`; §15.3
the rendering of `dialog`, `[popover]` and `::backdrop`), CSS Position 4
(§"Top layer": the rendering of top-layer elements, the `::backdrop`
pseudo-element, the `overlay` property), Selectors 4 (`:modal`,
`:popover-open`, `:open`), UI Events (the keys that are not activation),
text of 2026-11-13. Measurements in Chromium 148 (below). The session that
wrote this ADR read no browser or engine source code and no polyfill
internals beyond the calls that the pages make.

### What GitHub needs

Measured in Chromium's replay of `fixtures/js/github-swb` with an init
script that wraps `showPopover`, `hidePopover`, `togglePopover`, the
`popover` accessors, the `HTMLDialogElement` members, `popoverTargetElement`,
`popoverTargetAction`, `command` and `commandForElement` of buttons and
inputs, `setAttribute` of the top-layer attributes, `matches`,
`querySelector*` and `closest` with top-layer selectors, `CSS.supports`,
`addEventListener` for `beforetoggle`, `toggle`, `cancel`, `close` and
`command`, and that logs those events in the capture phase at the window
(`swbtools replay github-swb --init-script FILE --steps FILE --record
EXPR`). Steps: a wait of 2 s; a hover over the latest-commit "..." button;
a click on it; Escape; a click on it again; a click outside; a click on
the Code button; Escape; the branch selector; Escape; the "Platform" menu;
Escape; a hover over the check icon; three Tab presses.

| Item | Chromium replay |
|---|---|
| `popover` elements | 10, all closed after the load: `tool-tip` (`popover=manual`, the only one in the HTML), 7 `span.prc-TooltipV2-Tooltip` (`popover=auto`, set with `setAttribute` by React after `load`), 2 `div.Primer_Brand__Tooltip` (`popover=auto`) |
| `showPopover` | 1 per hover: the hover over the "..." button shows a TooltipV2 span (an auto popover, 125 x 28 px at (166, 358)); the hover over the check icon shows another one (59 x 28 at (235, 358)). The `beforetoggle` and `toggle` events are trusted |
| `hidePopover`, `togglePopover` | 0. The click on the "..." button hides the open tooltip by light dismiss (`beforetoggle` `open>closed` without a call) |
| `popovertarget`, `commandfor`, `<dialog>`, `showModal`, `requestClose` | 0 in the DOM and 0 calls |
| `popoverTargetElement` | 2 reads, by `tool-tip` on its button at its upgrade |
| Selectors | `matches(':popover-open')` on the tooltips (10 per step), `matches(':modal')` once, `querySelectorAll('dialog:modal, [role="dialog"][aria-modal="true"]')` on each Escape |
| `CSS.supports` | `position-anchor --name` (anchor positioning: backlog) |
| The "..." button, Code, branch selector, header menus | not popovers: the "..." button expands the commit message in the page; Code, the branch selector and the menus are React portal `div`s (Primer `AnchoredOverlay`), which ADR 0036 and feature 9 cover |

So GitHub's first view and list items 3 to 7 need: auto popovers shown by
script, `:popover-open`, the top-layer rendering of the tooltip (fixed
position, outside its clipping ancestors), light dismiss on a click, and
`dialog:modal` as a valid selector that matches nothing. The diagnosis's
"the latest-commit '...' button opens a popover (`:popover-open` 1 after
the click)" was the hover tooltip: the hover step came before the click.

Paths that change in swb once it has `popover` (read in the fixture's
files; the targets must keep their scores and integration tests):

- **The popover polyfill** (`8c15e77219dac0be.js`) installs only if
  `!("popover" in HTMLElement.prototype)`. With feature 6 it no longer
  runs: it no longer wraps `attachShadow`, no longer rewrites
  `:popover-open` in `querySelector`, `matches` and `closest` to the class
  `.\:popover-open`, no longer defines `window.ToggleEvent`, and no longer
  adopts its constructed sheet into the document and the 24 shadow roots.
  `crates/automation/tests/github.rs` asserts that sheet (ADR 0036 5c-2:
  `tool-tip:1`, `relative-time:1`, `documentAdopted` 1, `adoptedSheets` 1);
  sub-feature 6a changes those assertions to Chromium's values (no adopted
  sheet anywhere).
- **The `command` polyfill** (`c54db17eb2a4141c.js`) tests
  `"command" in HTMLButtonElement.prototype && "source" in
  CommandEvent.prototype` and another helper tests that
  `document.body.showPopover` is native (`/native code/` in its
  `toString()`; swb's natives print `function showPopover() { [native code]
  }`, as Chromium). It keeps running in swb (no `command`, section 11).
- **TooltipV2 and `tool-tip`** take their native paths: `showPopover` on
  hover and focus, `:popover-open` in their CSS.
- **Other targets** (same tracer, load and the steps of their integration
  tests: `crates/automation/tests/data/bbc-steps.json`,
  `ars-technica-steps.json`): no `popover`, no `<dialog>`, no call. BBC's
  "Save" popover is a React portal `div` (`togglePopover` in BBC's bundle is
  a React prop name); React registers `cancel`, `close` and `toggle`
  listeners on its root (its non-delegated events), which nothing fires.
  Ars Technica has Tailwind's `*,::backdrop,:after,:before{--tw-...}` and
  Alpine's `[x-dialog]`. Wikipedia, Hacker News, Craigslist and senko.net:
  nothing. JS-off fixtures: GitHub's closed `tool-tip` (hidden by the UA
  sheet, as today) and 4 `::backdrop` rules, which apply only to elements
  in the top layer. Expected change for all targets other than GitHub: none.

### Measurements

Two new probe files: `tools/probes/web-top-layer.json` (JavaScript and
events; 61 cases, 237 queries; 43 cases with input `steps`) and
`tools/probes/top-layer.json` (boxes, computed values and pixels; 22 cases,
51 queries; 18 with JavaScript to open popovers and dialogs). The web file
has the logging header of `web-shadow-dom.json` with helpers: `on(target,
types)` logs events as `type:target(old>new)` with `!` for a cancelable
event, `OP()` lists the open popovers and dialogs (`*` for modal), `WR()`
waits 30 ms for the event tasks, `CL()` closes everything and `mk()` makes
a popover. Each query that starts with `CL();` first closes everything and
waits until the `toggle` tasks have fired, so that a query sees only its own
events. Cases with steps read their log after 150 ms. Chromium runs a
dialog's `close` task and its `toggle` task in either order (two task
sources; varies between runs): the dialog cases list those two kinds sorted
after `||` (`NZ()`). The JS-off invoker cases (`noscript-*`) run with a
`Content-Security-Policy: script-src 'none'` meta element: Chromium does not
run the page's scripts, the probe's queries still run (`typeof window.RAN`
is `"undefined"` in each).

**User activation in the probe tool.** The tool evaluates scripts in the
top-level page after `load` and before the steps (`wait_for_fonts`,
`stop_animations`); Playwright's evaluations count as a user gesture. So
every probe page has a sticky, a transient and a history-action activation
before its first step (measured in the four main-frame Escape cases, which
log `navigator.userActivation` from the page's own script: `false/false`
at `load`, `true/true` at the first Escape's `keydown`). Watchers made
before that activation (in `load` handlers) are still in the first group.
The `xo-*` cases measure the state without any activation: their dialogs
and popovers are in a cross-origin iframe (`$OTHER`), which the parent
focuses at `load`; Chromium gives an activation to the frame, its
ancestors and the frames of the same origin, so the cross-origin frame has
none (`false/false` at each Escape) until a step clicks in it. The swb
tests must reproduce this: before the steps of a main-frame case, note one
user activation for the top-level page after `load` (with the frame tree's
existing rule: the frame, its ancestors and the same-origin frames); a
cross-origin frame gets none.

Chromium 148, three runs of each file, equal values.

| Question | Case | Chromium |
|---|---|---|
| Members | `api-surface` | `HTMLDialogElement.prototype`: `close, closedBy, open, requestClose, returnValue, show, showModal`; `HTMLElement`: `popover, showPopover, hidePopover, togglePopover, onbeforetoggle, ontoggle, oncommand, oncancel, onclose, inert`, no `interestForElement`; `HTMLButtonElement`: `popoverTargetElement, popoverTargetAction, command, commandForElement, interestForElement`; `HTMLInputElement`: `popoverTargetElement, popoverTargetAction`; `ToggleEvent`, `CommandEvent`, `CloseWatcher` exist; `ToggleEvent.prototype`: `newState, oldState, source`; lengths: `showPopover`, `togglePopover`, `hidePopover`, `close`, `show`, `requestClose` 0, `ToggleEvent` 1. `CSS.supports`: `:popover-open`, `:modal`, `:open`, `::backdrop`, `:popover-open::backdrop`, `dialog:modal::backdrop`, `:fullscreen` true; `:closed`, `::backdrop:hover` false; `overlay`, `transition-behavior`, `interactivity`, `position-anchor`, `position-try` true. `new ToggleEvent('toggle', {oldState, newState})`: not bubbling, not cancelable, not composed, `source` null; without arguments: `TypeError: Failed to construct 'ToggleEvent': 1 argument required, but only 0 present.`; `{source: 5}`: `... Failed to read the 'source' property from 'ToggleEventInit': Failed to convert value to 'Element'.`. Natives print `function showPopover() { [native code] }` |
| Reflection | `popover-reflection` | `popover`: missing → `null`; `""`, `auto`, `AUTO` → `auto`; `manual`, `Manual` → `manual`; `hint` → `hint`; `bogus`, `" auto"` → `manual`; setting `null` or `undefined` removes the attribute. `popoverTargetAction`: missing, `""`, invalid → `toggle`; ASCII case-insensitive. `popoverTargetElement`: the element with the id in the same tree, also when it is not a popover; setting an element writes `popovertarget=""`; `null` removes the attribute; an element in a shadow tree below the button's tree gives `null`; a button in a shadow tree returns an element of the document (an ancestor tree); a detached button returns `null`; a target that was removed gives `null`. `closedBy`: `any`, `closerequest`, `none` (case-insensitive), invalid and empty → `none`; missing → `closerequest` for a modal dialog, `none` otherwise. `returnValue` is a string (`5` → `"5"`); `open = true` writes `open=""` and does not make the dialog modal |
| Show and hide | `show-hide-events` | `showPopover`: `beforetoggle` (`closed>open`, cancelable) in the call, the element matches `:popover-open` when the call returns, `toggle` in a later task; a second `showPopover` does nothing (no event, no exception); `hidePopover`: `beforetoggle` (`open>closed`, not cancelable), `toggle` later; on a hidden popover nothing. `togglePopover()` returns the new state; `togglePopover(true)` on an open popover returns true without events; `{force: true}` works as `true`; `{}` toggles. Both events: `ToggleEvent`, `bubbles` false, `composed` false, trusted, `source` null when no source was given. `onbeforetoggle` and the `ontoggle` content attribute run |
| `toggle` coalescing | `toggle-coalescing` | one `toggle` task per element: show + hide in one task → one `toggle` `closed>closed`; show, hide, show → `closed>open`; hide, show → `open>open`; the old state is kept and the task moves to the end (`pa` show, `pb` hide, `pa` hide → `pb(open>closed)` then `pa(closed>closed)`); show and a hide in a microtask: one event; show and a hide in a timer: two events |
| Cancel | `beforetoggle-cancel` | `preventDefault()` on the show event: the popover stays closed, no `toggle`, no exception, `togglePopover()` returns false; the hide event cannot be canceled; an invoker's show can be canceled |
| Exceptions | `exceptions` | not a popover: `NotSupportedError: Failed to execute 'showPopover' on 'HTMLElement': Not supported on elements that are not popovers.` (same for `togglePopover`; `hidePopover` adds ` This might have been the result of the "beforetoggle" event handler changing the state of this popover.`); disconnected: `InvalidStateError: ...: Invalid on disconnected popover elements.` for `showPopover` and `togglePopover`, `hidePopover` does nothing; an open non-modal `<dialog popover>`: `showPopover` does nothing; a popover in a document without a browsing context or in template contents: `InvalidStateError: ...: Invalid for popovers within documents that are not fully active.`; `showModal` there: `... Invalid for dialogs within documents that are not fully active.`; `{source: 5}`: `TypeError: ...: Failed to read the 'source' property from 'ShowPopoverOptions': Failed to convert value to 'HTMLElement'.`; `showPopover(5)`: `... The provided value is not of type 'ShowPopoverOptions'.`; `showPopover(null)` shows |
| Changes in `beforetoggle` | `beforetoggle-mutations` | removal in the show event: `InvalidStateError: ...: Invalid on disconnected popover elements. This might have been the result of the "beforetoggle" event handler changing the state of this popover.`; a `popover` change from auto to manual in the event: the popover opens (as manual); `hidePopover()` in the show event: no effect, the popover opens; showing another auto popover in the event: that one opens and is closed again by the first one's show; `showPopover()` of the same element in its own event recurses (each inner call fires a new event and returns) and the element opens once; `hidePopover()` in the `toggle` handler hides it; `showPopover()` in the hide event: the hide continues |
| Removal and attribute changes | `removal-and-attribute-while-open` | removal (also of an ancestor, also a move with `appendChild`, also adoption into another document) hides the popover without `beforetoggle`; its pending `toggle` task still fires; changing `popover` to another state (`manual`, `hint`, `bogus`, removed) while open: `beforetoggle` `open>closed` (not cancelable), then hidden, `toggle` later; the same state in another spelling (`AUTO`, `""`) keeps it open |
| Auto stack, DOM | `auto-stack-dom` | nested popovers stay open together; an unrelated auto popover closes the stack from the top (`p3`, `p2`, `p1` events in that order) before its own `toggle`; `hidePopover` of a middle popover closes the ones above it first; a manual popover stays open; showing a nested popover whose parent popover is closed closes the others; a popover inside an open manual popover is not nested in it |
| Auto stack, invoker and source | `auto-stack-invoker-and-source` | a popover shown by an invoker inside an open popover is nested (stays open); shown by script without a source it is not (the first closes); `showPopover({source})` and `togglePopover({source, force: true})` nest like an invoker; a second click on an invoker hides its popover; `popovertargetaction=show` and `hide` act once; hiding the outer popover through its invoker hides the nested one first; removing the source button after the show breaks the nesting for later shows |
| Hint | `hint-popovers` | a new hint closes an unrelated open hint; an auto popover closes the open hints; a hint inside an open auto popover stays open, also when another hint opens after it; hiding an auto popover closes the open hints |
| Invoker controls | `invokers-types` | with `popovertarget` and a synthetic `click()`: `button` (any type, outside a form), `input` `button`, `submit`, `reset`, `image` toggle the popover; `checkbox`, `text`, `a`, `div`, a disabled button and a button in a disabled `fieldset` do nothing; a target that is not a popover or a missing id: nothing; in a form: a submit button submits and does not toggle, a reset button resets and does not toggle, `type=button` and `input type=button` toggle. A dispatched `MouseEvent` or `PointerEvent` `click` toggles, a plain `Event('click')` does not; a canceled `click` does not; a click on a child of the button does. An invoker inside its own popover: `toggle` on a closed popover opens it, on an open one closes it. A `<dialog popover>` target opens as a popover (`open` stays false) |
| Invokers and shadow trees | `invokers-shadow` | an id resolves in the button's own tree only (a light button does not find a shadow id and the reverse); `popoverTargetElement` to an element in an ancestor tree works; to a descendant tree gives `null` and does nothing. A host can be a popover (`tool-tip`): `:host(:popover-open)` matches, `position` computes to `fixed`. A popover in a shadow tree inside an open popover is nested in it (flat tree) |
| Dialog | `dialog-basics` | `show()`: `beforetoggle` (`closed>open`, cancelable) and `toggle` fire for dialogs too; `open` attribute set; `:open` matches, `:modal` not. `showModal()` on an open non-modal dialog: `InvalidStateError: Failed to execute 'showModal' on 'HTMLDialogElement': The dialog is already open as a non-modal dialog, and therefore cannot be opened as a modal dialog.`; `show()` on a modal one: `... 'show' ...: The dialog is already open as a modal dialog, and therefore cannot be opened as a non-modal dialog.`; `showModal()` on an open modal dialog does nothing; on an open popover dialog: `... The dialog is already open as a Popover, and therefore cannot be opened as a modal dialog.`; on a detached dialog: `... The element is not in a Document.`; `show()` on a detached dialog works. `close(rv)`: `beforetoggle` `open>closed`, `returnValue` set, `close` (task, not bubbling, not cancelable, `Event`); `close()` on a closed dialog: nothing, `returnValue` unchanged. Two modal dialogs: both `:modal`. `requestClose(rv)`: `cancel` (`Event`, cancelable, not bubbling) then the close; a canceled `cancel` keeps it open and keeps the old `returnValue`; `closedby=none`: `requestClose` still fires `cancel` and closes. `removeAttribute('open')` on a modal dialog: `open` false, `:modal` stays true, no events; the dialog stays in the top layer and the document stays blocked (the next queries cannot focus outside it), and `close()` then does nothing. `open = true`/`false` by script: no events |
| `method=dialog` | `dialog-form` | `submit` fires (cancelable); then the dialog closes with the submitter's value (`button value`, `input type=submit value`, `input type=image` with the click coordinates `"0,0"`); a submitter without a value keeps the old `returnValue`; `formmethod=dialog` on a button of a normal form works; a `method=dialog` form outside a dialog: `submit`, no navigation, nothing else; `requestSubmit()` and `submit()` close the dialog without changing `returnValue`; a canceled `submit` keeps it open; a modal dialog closes the same way |
| Dialog focus | `dialog-focus` | `showModal` and `show` focus: the `autofocus` descendant, else the first focusable area in tree order (also `tabindex=-1`, also inside a shadow tree: the host's root `activeElement`), else the dialog itself; an `autofocus` attribute on the dialog is ignored (its first input gets focus). `close()` focuses the element that had focus before the show, also when the focus moved inside; if that element is gone, the focus stays where it is |
| Popover focus | `popover-focus` | showing a popover focuses its `autofocus` descendant, or the popover itself if it has `autofocus`, else the focus does not move; `hidePopover` focuses the element focused before the show if the focus is inside the popover, else nothing; without a previous element (body) or with a removed one the focus stays |
| Top layer and hit tests | `top-layer-hit-order` | the popover shown last is on top (`elementsFromPoint`: `b, a, z, html`) whatever `z-index` they have; a hidden and re-shown popover moves to the top; `position: fixed; z-index: 2147483647` stays below every top-layer element; a nested popover is on top of its parent. With a modal dialog: points outside the dialog give the dialog (its backdrop) and `elementsFromPoint` lists only `dialog, html`; a popover outside the dialog shown after it is drawn but inert (the hit is the dialog); a popover inside the dialog is hit normally; `showModal` closes the open auto popovers; a non-modal dialog changes nothing |
| Inertness under a modal dialog | `inert-click`, `inert-hover`, `inert-tab`, `inert-selection`, `inert-backdrop-click` | clicks on a button, an input, an iframe and text outside the dialog: the events go to the dialog (pointerdown, mousedown, click); the first such click focuses the dialog; the iframe gets no event; the frame's document is not blocked (its own `elementFromPoint` works). `:hover` over an outside button: `html, body, dialog` match, the button not. `interactivity` computes to `auto` and `inert` is false everywhere (Chromium does not express modal inertness in style values). Tab from the dialog's first control: `di`, (focus leaves the document), `db`, `di`, (leaves), and Shift+Tab `di`. A double click on outside text selects the word "in" of the dialog's button; a script range over outside text gives `""` from `getSelection().toString()`. A click on the backdrop does not close a dialog |
| `closedby=any` | `inert-closedby-any` | a click on the backdrop closes the dialog (`returnValue` `""`) |
| Escape | `escape-*`, `xo-*` | Escape on a modal dialog: `cancel` (cancelable) then close; `returnValue` unchanged. A canceled `cancel`: the dialog stays; the next Escape fires a `cancel` that is not cancelable and closes it (main frame: the tool's activation makes the first `cancel` cancelable). Without any activation (`xo-escape-no-activation`) the first `cancel` is not cancelable and the first Escape closes the dialog; after a click in the frame the first `cancel` is cancelable and the second is not (`xo-escape-cancel-after-click`); a second click after a canceled `cancel` does not make the next `cancel` cancelable again (`xo-escape-cancel-click-between`). Two modal dialogs, two auto popovers inside a dialog, or two nested auto popovers, all opened before any activation: one Escape closes all of them, top first (one close watcher group; `xo-groups-no-activation`, `xo-popovers-no-activation`, and the main-frame cases, whose watchers are made in `load` before the tool's activation); a manual popover stays. A dialog opened by a click after them gets a new group: the first Escape closes it, the second closes the earlier ones together (`xo-groups-after-click`, `xo-groups-two-before-click`). Opened by clicks (`noscript-escape`): one Escape closes only the top one. A `keydown` listener that cancels Escape: nothing closes. A non-modal dialog stays open (no close watcher); a non-modal dialog with `closedby=closerequest` closes; a modal dialog with `closedby=none` stays. After Escape the focus returns to the invoker of the popover (`escape-focus-return`) |
| Popover Tab order | `popover-tab-order` | an open popover's content follows its invoker: `inv pb1 pb2 y z`, and backward `y pb2 pb1 inv` |
| Light dismiss | `light-dismiss-*` | a click outside the open auto popovers closes them at the `pointerup` (the `beforetoggle` events come between `pointerdown` and `pointerup` listeners), top first; a click inside a nested popover keeps all; a click inside the parent closes only the child; press outside and release inside (or the reverse): nothing closes; a click on the invoker: the child closes at `pointerup`, then the invoker's click closes the popover; a right click closes; a click on a manual popover closes the auto popovers and keeps the manual one; `preventDefault()` on `pointerdown` and `mousedown` does not stop the dismissal |
| Without script | `noscript-*` | invokers toggle, show and hide; a nested invoker nests; a click outside closes both; Escape closes the top one; Enter and Space on a focused invoker toggle; `form method=dialog` closes `<dialog open>` with `returnValue` `"yes"`; Escape does not close `<dialog open>`; in a form, `type=button` toggles and a submit button submits (`#sent`) without toggling |
| UA styles | `popover-closed`, `popover-open-ua`, `popover-open-inline`, `popover-open-long`, `dialog-nonmodal`, `dialog-modal-ua`, `dialog-modal-big` | closed popover: `display: none`, computed `position: fixed`, `inset: 0`, `margin: auto`, `border: 3px solid`, `padding: 4px` (0.25em), `overflow: auto`, `overlay: none`, no client rects. Open: centered in the viewport (`margin` resolves to 284 px and 348.78 px at 800 x 600), `width` fit-content, `overlay: auto`; an inline `span` becomes `display: block`; long text: the box is the viewport width minus the border (786 px content) and as tall as its text, centered. Non-modal dialog: `position: absolute`, in flow position of its static box (y 50 after a paragraph), centered horizontally, `padding: 16px`, `overlay: none`; it does not take space (the next paragraph is at y 50). Modal: `position: fixed`, centered (272 px, 343 px margins), `max-width` and `max-height` `calc(100% - 38px)` (6px + 2em), `overflow: auto`, `visibility: visible`, `overlay: auto`; big content: 762 x 562 px content in an 800 x 600 box at (0, 0) |
| `::backdrop` | `popover-open-ua`, `dialog-modal-ua`, `backdrop-paint`, `backdrop-author`, `backdrop-inheritance` | `display: block`, `position: fixed`, `inset: 0`, 800 x 600; a dialog's backdrop `rgba(0, 0, 0, 0.1)` with `pointer-events: auto`; a popover's backdrop transparent with `pointer-events: none`; authors can move and size it (`inset: auto; top: 100px; ...`); it paints right below its element and above earlier top-layer elements (a red manual popover under a modal dialog's backdrop gives (229, 0, 0)); it inherits from its element (`color`, `font-size` 30 px, `1em` = 30 px) |
| Escaping ancestors | `escape-ancestors`, `escape-ancestors-modal`, `popover-in-shadow-tree` | an open popover inside an ancestor with `transform`, `overflow: hidden` with `position: relative`, `contain: paint`, `filter` or `opacity: 0.5` has its box and color as without the ancestor (red (255, 0, 0) at full opacity, not clipped); inside a `display: none` ancestor it has no box although it matches `:popover-open`; a modal dialog inside a transformed, clipped ancestor is centered in the viewport; a popover in a shadow tree of a clipped, transformed host is at its own position, `elementFromPoint` gives the host, the root's `elementFromPoint` the popover |
| Containing block and `position` | `dialog-modal-scrolled`, `popover-author-position` | `position: absolute` on a top-layer element: the containing block is the initial containing block (box at (30, 40) of the document also when scrolled by 500 px, in a relative ancestor at (100, 100)); `static` and `relative` compute to `absolute`; `fixed` stays fixed to the viewport |
| `display` | `popover-author-display` | an author `[popover] { display: block }` shows a closed popover as a normal fixed box (not in the top layer: `overlay: none`); an author `display: none` keeps an open popover without a box; the `hidden` attribute does not hide an open popover (it has a box) |
| Paint order | `top-layer-paint-order` | pixels: later top-layer element on top of the earlier one, both above a `z-index: 2147483647` fixed element, their own `z-index` (5, −5) ignored |
| Frames | `dialog-modal-in-iframe` | a modal dialog in an iframe is centered in the frame's viewport and matches `:modal`; the parent document is not blocked (`elementFromPoint` finds the parent's button) |
| `overlay` and states | `overlay-and-states` | `overlay`: `auto` for a modal dialog, `none` for a non-modal dialog, a closed popover and `details`; `:open` matches open dialogs and `details`, `:modal` only the modal dialog |
| JS off | `popover-and-dialog-js-off` | `<dialog open>` renders (absolute, centered); closed popovers and `tool-tip` have no box |

## Decision

### 1. The model in `swb_dom`

The top layer, the popover lists and the dialog state belong to the
document: DOM operations (removal, adoption, attribute changes) change
them, and both the web layer (scripts) and the engine (JS off, input)
use them. A new module `crates/dom/src/top_layer.rs` holds the state and
the algorithms.

- **The flag byte.** `ElementData::shadow_flags` becomes `flags` (one
  mechanical rename in 6a); three new bits: `POPOVER_SHOWING` (the
  popover visibility state is "showing"), `MODAL` (the dialog's "is modal"
  flag) and `TOP_LAYER` (the element is in the top layer). `ElementData`
  stays 64 bytes. Style reads the byte (no lookup) for `:popover-open` and
  `:modal`; the box tree reads `TOP_LAYER`.
- **The side state.** `Document::top_layer: Option<Box<TopLayerState>>`,
  `None` until the first element is shown as a popover or a dialog (a
  parsed `<dialog open>` does not create it):
  - `layer: Vec<NodeId>`: the top layer in order (CSS Position 4); an
    element is at most once; adding moves it to the end.
  - `auto_list`, `hint_list: Vec<NodeId>`: the showing auto and hint
    popover lists (HTML).
  - `popovers: HashMap<NodeId, PopoverData>`: for each showing or
    recently toggled popover: the invoker (`Option<NodeId>`), the
    previously focused element, the "showing or hiding" flag, the pending
    toggle task (the old state; at most one per element, section 2), its
    close watcher.
  - `dialogs: HashMap<NodeId, DialogData>`: the previously focused element,
    the close watcher, the request-close return value and the
    "enable close watcher for request close" state.
  - `pointerdown_target: Option<NodeId>`: light dismiss (section 4).
  - `watchers: CloseWatcherManager` (section 6). The HTML Standard puts the
    manager on the window; swb has one document per window and keeps it
    with the document's state.
  Entries are removed when an element is hidden or closed and its toggle
  task has run; the map sizes are bounded by the showing elements.
- **Blocked by a modal dialog.** `TopLayerState::blocking(&self) ->
  Option<NodeId>`: the last element of `layer` with the `MODAL` bit (HTML
  "blocked by a modal dialog"). One field read on documents without a top
  layer.
- **DOM steps.** The removing steps of an element (`swb_dom`'s removal
  path; the flag byte decides): if it or a shadow-including descendant
  is a showing popover, run "hide popover" without events and without
  focus changes (measured: no `beforetoggle`; the toggle task that is
  already queued still fires); if it is in the top layer, remove it. The
  same at adoption into another document (measured). The attribute change
  steps of `popover`: a change between states while showing hides with
  `beforetoggle` (not cancelable) and a toggle task (measured); a change of
  spelling only (`AUTO`, `""`) does nothing. No attribute change steps for
  `open` (measured: removing `open` from a modal dialog keeps it modal and
  in the top layer; Chromium's behaviour, which the HTML Standard does not
  describe in the same way; section 11).
- **Journal.** Showing and hiding change no attribute of a popover, so
  the journal gets a new entry kind `Change::TopLayer` (the element), which
  the engine's drain turns into a restyle (as every change does today)
  and which mutation observers ignore. `showModal`, `show` and `close`
  change the `open` attribute, which is an ordinary attribute change.

### 2. The algorithms and their events

The HTML algorithms fire synchronous events in their middle
(`beforetoggle`, `cancel`) and re-check the state after each one, because
a listener can change the DOM. swb's web layer cannot hold a borrow of the
document while scripts run. So the algorithms in `top_layer.rs` take a
host:

```rust
pub trait TopLayerHost {
    fn doc(&mut self) -> &mut Document;           // short borrows only
    fn fire_beforetoggle(&mut self, el: NodeId, old: State, new: State, cancelable: bool, source: Option<NodeId>) -> bool; // false: canceled
    fn fire_cancel(&mut self, dialog: NodeId, cancelable: bool) -> bool;
    fn queue_toggle_task(&mut self, el: NodeId);  // the task reads the coalesced states from TopLayerState
    fn queue_close_task(&mut self, dialog: NodeId);
    fn focus(&mut self, el: Option<NodeId>, why: FocusReason);
    fn has_history_action_activation(&mut self) -> bool;
    fn consume_history_action_activation(&mut self);
}
```

Two implementations: the web layer's (events are dispatched through the
bindings; the `toggle` and `close` tasks go to the DOM manipulation task
source) and the engine's for documents without scripts (events are no-ops
that return "not canceled"; tasks do nothing but clear the pending state).
The engine's JS-off paths and the bindings call the same functions:
`show_popover(host, el, throw, source) -> Result<(), PopoverError>`,
`hide_popover(host, el, focus_previous, fire_events, throw, ignore_dom)`,
`toggle_popover`, `hide_all_popovers_until`, `topmost_popover_ancestor`,
`show_dialog`, `show_modal`, `close_dialog`, `request_close_dialog`,
`popover_target_activation(host, invoker)`, `light_dismiss_down`,
`light_dismiss_up`, `process_close_watchers`. Errors are an enum with
Chromium's texts (table above); the bindings throw them, the engine logs
them at `debug!`.

Measured behaviour that the functions follow (HTML's algorithms, with the
measured details):

- **Show.** Check validity (not a popover → `NotSupportedError`; already
  showing → return; not connected → `InvalidStateError`; a document that
  is not fully active, which includes template contents and documents
  without a browsing context → `InvalidStateError`; an open dialog →
  return), set "showing or hiding", fire `beforetoggle` (cancelable), check
  validity again (the measured message with "This might have been the
  result of the beforetoggle event handler ..."), for auto and hint: find
  the topmost popover ancestor and hide all popovers until it (with
  events), check again, then add to the list and the top layer, set the
  flag bit, record the previously focused element, run the popover
  focusing steps (section 5), queue the toggle task. Recursion: a
  `showPopover` from inside its own `beforetoggle` runs a full nested show
  (measured: four events, one open); the "showing or hiding" flag only
  stops the hide paths, as the HTML Standard says.
- **Hide.** As HTML "hide popover algorithm": for auto and hint first hide
  the popovers above it (top first), fire `beforetoggle` (not cancelable),
  re-check, remove from the list and the top layer, focus the previous
  element when asked and the focus is inside, queue the toggle task.
- **Toggle tasks.** HTML "queue a popover toggle event task": if the
  element has a pending task, keep its old state, drop it and queue a new
  one with the new state at the end (measured: `pb` then `pa`). The task
  fires `toggle` (`ToggleEvent`, not cancelable, `source` the source
  element or null). Dialogs use the same mechanism for their toggle
  events (measured `beforetoggle`/`toggle` on `show`, `showModal`,
  `close`).
- **Topmost popover ancestor.** The flat tree ancestors (ADR 0036
  `flat_parent`), the invoker of each showing popover and the source
  element of a show: the highest position in the list wins. Positions
  come from a map built once per call (`HashMap<NodeId, usize>` of the
  list). The walk stops at the first ancestor whose position is the top of
  the list (the common case: a nested popover shown over its parent), so
  a chain of N nested popovers shown in order costs O(N), not O(N²).
- **Hint popovers.** In, as measured: their own list; an auto show hides
  the hints that are not its ancestors; a hint inside an open auto
  popover stays with it.

### 3. Invokers

- **The activation behaviour.** HTML "popover target attribute
  activation behavior" in `top_layer.rs`: the invoker is a `button` or
  an `input` of type `button`, `submit`, `reset` or `image` (measured), not
  disabled (also through a `fieldset`), and not a submit or reset button
  with a form owner (measured: those submit or reset instead); the target
  is `popoverTargetElement` (an element reference or the id in the
  invoker's tree, measured); it must be a popover. Then by
  `popovertargetaction`: `toggle` (hide if showing, else show with the
  invoker as source), `show`, `hide`. A `<dialog popover>` target opens as
  a popover (measured).
- **JS on.** The script path's activation (`crates/web/src/html/
  activation.rs`, `after_dispatch`) gets a role `PopoverInvoker`: a click
  that is a `MouseEvent` (also `PointerEvent`, also synthetic `click()` and
  `dispatchEvent`) and not canceled runs it; a plain `Event('click')` does
  not (measured).
- **JS off.** The engine's `click_role` (`page/forms.rs`) gets the same
  role and `activate` calls the function with the engine's host. Enter and
  Space on a focused invoker already end in `click_element` (measured:
  toggle).
- **`popoverTargetElement`.** An element reflection (HTML
  "attr-associated element"): the setter stores the element in a side
  table of the web layer (a weak reference: the button keeps the target
  alive no longer than the DOM does) and writes `popovertarget=""`; the
  getter returns the stored element if it is a descendant of one of the
  button's shadow-including ancestors (HTML "attr-associated element"),
  else `null`; without a stored element it resolves the id in the
  button's tree (measured: an element of an ancestor tree yes, of a
  descendant tree `null`, a detached button `null`). The engine's JS-off
  path reads only the id.

### 4. Light dismiss

HTML "light dismiss open popovers", hooked into the pointer path for the
primary and the other buttons (measured: a right click dismisses):

- At `pointerdown`: if the auto or hint list is not empty, store the
  "topmost clicked popover" of the hit node (the higher of the nearest
  inclusive open popover and the nearest inclusive target popover of an
  invoker, both through flat tree ancestors) in `pointerdown_target`.
- At `pointerup`, before the `pointerup` listeners run (measured order:
  `pointerdown`, `beforetoggle`..., `pointerup`, `click`): compute the
  topmost clicked popover again; if it is the same as at `pointerdown`,
  hide all popovers until it (or all, if it is none), with events and
  without focus changes. Clear `pointerdown_target`. A canceled
  `pointerdown` or `mousedown` does not stop it (measured).
- JS on: the hooks are in `scripted_down` and `scripted_up`
  (`page/events.rs`) between the hit test and the dispatch; JS off: in
  `press_in_document` and `release_in_document`.
- `closedby=any` dialogs: a click whose `pointerdown` and `pointerup` are
  both on the dialog's backdrop (outside its border box) runs "request
  close" (measured: closes, `returnValue` `""`).
- Fast path: one test of `top_layer` and of the two lists.

### 5. Focus

- **Popover focusing steps.** At show: the popover itself if it has
  `autofocus`, else its first `autofocus` descendant in flat tree order
  that is focusable, else no change (measured). swb does not process
  document `autofocus` (ADR 0036 5d notes); these steps read the attribute
  directly.
- **Dialog focusing steps.** At `show` and `showModal`: the first
  `autofocus` descendant, else the first focusable area in flat tree order
  (also `tabindex=-1`, also through a shadow host's focus delegate), else
  the dialog itself (measured; the dialog's own `autofocus` attribute is
  not used, measured).
- **Previously focused element.** Recorded at show; at hide (popovers:
  when the focus is inside the popover; dialogs: always) it gets the focus
  if it is still connected and focusable; else the focus stays (measured).
- **Tab order.** An open popover with an invoker is placed in the
  sequential order right after its invoker (measured `inv pb1 pb2 y`);
  its content is not visited at its DOM position. `next_in_order`
  (`crates/engine/src/focus.rs`) treats such a popover like a scope owner
  of ADR 0036 section 8, owned by the invoker.
- **Modal dialogs.** The Tab order is the dialog's (inert nodes are not
  focusable); after the last element the focus leaves the document as at
  the end of a document today (measured: one stop without a focused
  element, then the first control again).

### 6. Close requests and Escape

- **Close watchers** (HTML §6.4) as an internal mechanism (no
  `CloseWatcher` interface, section 11): the manager has a list of groups
  and an "allowed number of groups" (starts at 1; each user activation
  that the frame tree records adds one, up to the number of groups + 1).
  A new watcher goes into a new group if the number of groups is below the
  allowed number, else into the last group (measured: popovers and dialogs
  opened without user activation close together with one Escape; opened by
  clicks, one at a time).
- **Who gets a watcher.** Each showing auto and hint popover, each modal
  dialog with `closedby` other than `none`, each non-modal dialog with
  `closedby=closerequest` or `any` (measured: a plain non-modal dialog has
  none).
- **The key path.** Escape's `keydown` is dispatched first; if it is not
  canceled, the default action "process close watchers" runs (measured: a
  canceled `keydown` closes nothing), in `key_default` for JS on and off.
  It takes the last group and requests close for its watchers from the
  last to the first: a popover hides (with events, focus to the previous
  element), a dialog fires `cancel` and closes.
- **Groups** (measured with the `xo-*` cases, without the probe tool's
  activation): watchers made before any activation share the first group;
  a watcher made after an activation starts a new group; Escape closes the
  last group, top first.
- **`cancel` cancelable.** The HTML Standard's rule ("request close":
  `cancel` is cancelable if the number of groups is below the allowed
  number and the window has a history-action activation; a canceled
  `cancel` consumes the history-action activation), which the `xo-*` cases
  confirm: without any activation the first `cancel` is not cancelable and
  the dialog closes; with an activation the first is cancelable and, once
  canceled, the next is not. Measured detail: a further click after a
  canceled `cancel` did not make the next `cancel` cancelable
  (`xo-escape-cancel-click-between`); the implementer of 6d checks this
  against the standard's allowed-groups count before choosing the model
  and records the result. History-action activation: a per-window flag
  that each activation sets and that a canceled `cancel` clears; it does
  not expire (unlike the transient activation of 5 s). The swb tests give
  the probe pages the tool's activation (section "Measurements"). Escape
  is no user activation (already in `key_down`).
- **`requestClose()`** runs the same request-close steps (measured:
  `cancel`, then close; also with `closedby=none`).

### 7. Modal inertness

- **The rule.** While `TopLayerState::blocking()` is a dialog, every node
  of the document that is not that dialog or a flat tree descendant of it
  is inert (HTML "blocked by a modal dialog"). The `inert` attribute is
  not part of feature 6 (open question 5).
- **In style.** `ComputedStyle` gets an internal inherited flag `inert`
  (not a CSS property; Chromium's `interactivity` stays `auto`, measured):
  the style walk sets it at the root element when the document is
  blocked and clears it at the blocking dialog. One branch per element
  when the document is not blocked. A top-layer change restyles (section
  1, journal).
- **Hit testing.** `paint_box` emits no `HitRegion` for an inert box, so
  hits fall through to what is below; under a modal dialog that is the
  dialog's backdrop, whose element is the dialog (measured: clicks and
  `elementFromPoint` outside the dialog give the dialog;
  `elementsFromPoint` lists only non-inert elements and the root). An
  iframe that is inert gets no events (measured).
- **Focus.** `is_focusable` is false for inert elements; a click whose hit
  is the dialog through its backdrop focuses the dialog (measured).
- **`:hover` and `:active`** chains start at the hit node, so inert
  elements do not match (measured: `html, body, dialog`).
- **Selection.** The engine's selection does not start or extend into
  inert text (swb has no `getSelection`; the measured `""` for a script
  range is out of scope). The measured double-click result (Chromium
  selects the word of the dialog's button) is a Chromium detail that swb
  does not copy (section 11).
- **Frames.** A frame's document has its own top layer; a modal dialog in
  a parent makes the iframe element inert, so input does not reach the
  frame; the frame's own document is not blocked (measured).

### 8. Rendering

**Style.**

- `::backdrop` becomes a styled pseudo-element: `PseudoKind::Backdrop`,
  `RuleTarget::Backdrop` (rules with `::backdrop` are no longer dropped),
  styled in `style_pseudo_elements` only for elements with the
  `TOP_LAYER` bit, with the element's style as the parent (measured
  inheritance). `getComputedStyle(el, '::backdrop')` computes it with
  rules (`compute_pseudo_element_style_animated`).
- `:popover-open` and `:modal` from the flag byte in
  `DomElement::state`; `:open` stays as it is (measured: `dialog[open]`
  and `details[open]`).
- Top-layer fix-ups in `apply_fixups`: `position` other than `absolute`
  and `fixed` computes to `absolute` (measured: `static` and `relative`);
  the box is blockified (already for absolute positions; measured `span`
  → `block`); `overlay` computes to `auto` (UA rule).
- The `overlay` property: parsed (`none | auto`), not inherited, settable
  only by the UA sheet in effect (author `overlay` is valid and computes,
  but swb has no transitions, so it never delays a removal).
- UA sheet additions (HTML §15.3 and measured values):

  ```css
  dialog:modal { position: fixed; overflow: auto; inset-block: 0;
    max-width: calc(100% - 6px - 2em); max-height: calc(100% - 6px - 2em);
    visibility: visible; }
  :popover-open, dialog:modal { overlay: auto !important; }
  ::backdrop { display: block; position: fixed; inset: 0; }
  dialog::backdrop { background: rgba(0, 0, 0, 0.1); }
  [popover]::backdrop { pointer-events: none !important; }
  ```

  The existing `[popover]` and `dialog` rules match the measured values.

**Box tree** (`crates/layout/src/box_tree.rs`). When the document has a
top layer (one test otherwise):

- An element with the `TOP_LAYER` bit generates no box at its place in the
  flat tree walk (no placeholder).
- After the root element's box, for each element of `layer` in order:
  if it and its flat ancestors are not `display: none` (measured: no box
  inside a `display: none` ancestor) and it is in the flat tree, its
  `::backdrop` box (unless `display: none`) and then its own box are
  built as children of the initial containing block, after the root box.
- The depth counter starts again at these boxes (they are at depth 1).

**Layout.** Top-layer boxes are out-of-flow boxes whose containing block is
the initial containing block for `absolute` and the viewport for `fixed`
(measured), with no transform, filter or clip of their DOM ancestors:
`place_out_of_flow` places them after the root's out-of-flow boxes with a
fresh `Scope { absolute: icb, fixed: icb }` (measured: `#q`, `static` with
the UA's `inset: 0` and an author `margin: 0`, is at (0, 0) of the ICB,
not in its relative parent at (100, 100)).

**Paint.** After the root stacking context, the builder paints each
top-layer entry in order as its own root-level stacking context: the
backdrop, then the element. `z-index` has no effect on this order
(measured). Clips: only the viewport. Ancestors' `opacity`, `filter`,
`transform`, `clip-path` and `backdrop-filter` groups do not apply
(measured: full red under `opacity: 0.5`). A `backdrop-filter` on a
`::backdrop` works as ADR 0033 describes; its backdrop root is the root
(it reads everything painted before it).

**Hit testing** needs no new code: the items of the top layer come last
in paint order, so they win. `elementFromPoint` retargets as before
(measured for a popover in a shadow tree).

**Frames.** Each `DocumentState` renders its own top layer inside its
viewport (measured).

### 9. Bindings

- `HTMLElement`: `[CEReactions] attribute DOMString? popover` (nullable
  enumerated reflection with the measured keywords: `auto` for `""`,
  `manual` for invalid, `null` for missing), `showPopover(optional
  ShowPopoverOptions options = {})`, `hidePopover()`,
  `togglePopover(optional (TogglePopoverOptions or boolean) options =
  {})` returning `boolean`. Dictionaries `ShowPopoverOptions { HTMLElement
  source; }` and `TogglePopoverOptions : ShowPopoverOptions { boolean
  force; }` with Chromium's conversion messages.
- `HTMLButtonElement` and `HTMLInputElement`: `[CEReactions] attribute
  Element? popoverTargetElement` (section 3), `[CEReactions, Reflect]
  attribute DOMString popoverTargetAction` with `SwbKeywords="toggle show
  hide"`, missing and invalid `toggle`.
- `HTMLDialogElement`: `open`, `[CEReactions] attribute DOMString
  returnValue` (a string in the dialog's side data, not an attribute),
  `[CEReactions, Reflect] closedBy` (keywords `any closerequest none`,
  missing: computed from the state, invalid `none`), `show()`,
  `showModal()`, `close(optional DOMString result)`, `requestClose(optional
  DOMString result)`.
- `ToggleEvent : Event` with `oldState`, `newState`, `source` and the
  constructor `(DOMString type, optional ToggleEventInit eventInitDict)`
  (measured messages; the `source` is retargeted against the listener's
  tree as HTML says).
- The comment in `html.idl` that says the popover members and the dialog
  methods are cut changes.

### 10. JavaScript off

- Invokers: section 3 (engine activation, keyboard).
- Light dismiss: section 4 (engine press and release).
- Escape: section 6 (engine key default).
- `form method=dialog` and `formmethod=dialog`: `submit_form`
  (`forms/submit.rs`) closes the form's nearest `dialog` ancestor with the
  submitter's value instead of logging "dialog forms are not supported"
  (measured with and without scripts; outside a dialog nothing happens).
- `<dialog open>` renders as today (UA sheet; measured).
- Modal dialogs and script-shown popovers do not exist without scripts.

### 11. Scope and departures

In feature 6: the `popover` attribute with `auto`, `manual` and `hint`;
`showPopover`, `hidePopover`, `togglePopover` with `source`; `ToggleEvent`
and the toggle task coalescing; the auto and hint stacks; invokers with
`popovertarget`, `popovertargetaction`, `popoverTargetElement`; light
dismiss; the popover and dialog focusing steps and the popover Tab order;
`HTMLDialogElement` with `show`, `showModal`, `close`, `requestClose`,
`returnValue`, `closedBy`; `cancel` and `close`; `method=dialog`;
internal close watchers and Escape; modal inertness; the top layer in
style, layout, paint and hit testing; `::backdrop`; `:popover-open`,
`:modal`; the `overlay` computed value.

Out (backlog), with reasons:

- `command`, `commandfor`, `CommandEvent` (Chromium 148 has them): no
  element of any target uses them (0 in GitHub's DOM). GitHub's `command`
  polyfill keeps running in swb and keeps wrapping `attachShadow`, as
  ADR 0036 recorded. Its `commandfor` handling calls `showPopover`, which
  then works.
- `interestfor` and `interestForElement` (Chromium 148 has them on
  buttons): no target.
- The `CloseWatcher` interface: no target; the watchers stay internal.
- `@starting-style`, `transition-behavior: allow-discrete` and the
  `overlay` transition: swb has no CSS transitions.
- CSS anchor positioning (`position-anchor`, `anchor()`): already on the
  M10 backlog (owner decision of 2026-11-07); GitHub tests it with
  `CSS.supports` and positions its tooltips with script when it is
  missing.
- The `interactivity` property (Chromium 148 parses it).
- The `inert` attribute: open question 5.
- Fullscreen (`:fullscreen`, `requestFullscreen`).

Departures from the HTML Standard that follow Chromium (measured):

- Removing the `open` attribute of a modal dialog keeps it modal and in
  the top layer (section 1).
- The order of a dialog's `close` and `toggle` tasks: Chromium varies;
  swb queues `toggle` first (one task source for both).

Not copied: the word that Chromium's double click selects in a modal
dialog when the click is outside it (section 7).

### 12. Budgets

Chromium has no limits that the cases found. swb's bounds:

| Resource | Limit | Result |
|---|---|---|
| Top layer, popover lists, side maps | one entry per showing element: bounded by the arena's node limit (1,048,576) | none of their own |
| Toggle tasks | at most one pending per element (coalescing) | |
| Nested `showPopover` from `beforetoggle` | the JavaScript stack limit (each nesting is a script call); the task's time limit | `RangeError`, termination |
| `toggle` handlers that show and hide forever | each round is a new task; the event loop stays responsive (as a `setTimeout(0)` loop) | |
| Topmost popover ancestor | flat ancestors of the element, the invoker and the source: O(depth) per show, O(1) for a chain shown in order (section 2); charged to the task's time | termination |
| Hide all popovers until | one hide per entry above the target; each fires one event | task time |
| Close watcher groups | one per watcher at most; Escape requests close for one group | |
| Top-layer boxes | one box and one backdrop box per entry; the box tree's existing limits | |
| Inertness | one flag per computed style | |

Hostile cases (`tools/swbtools/hostile_cases/top_layer.py`, JavaScript on
unless named):

- `popover-nested-auto-10000`: 10,000 nested auto popovers shown in order
  (budget: the show loop under 2 s, memory under +50 MB; since 6b it
  renders 10,000 boxes and backdrops: about 120 MiB of the 160 MiB limit,
  see "6b fix round"). Since 6c it is also the case of live close
  watchers: a script shows no popover with a user activation, so the 10,000
  watchers are alive in one group; the case counts the open popovers (10,000),
  presses Escape (all close, one group), shows them again and clicks outside
  (light dismiss closes all).
- `popover-manual-10000-open`: 10,000 open manual popovers; style, layout,
  paint, 100 pointer moves, then all hidden.
- `popover-beforetoggle-reentry`: `beforetoggle` handlers that show and
  hide other popovers and their own element; a `toggle` handler that
  toggles again forever (the page stays responsive; the case ends after
  5 s).
- `popover-attribute-flip-100000`: the `popover` attribute changed 100,000
  times on an open popover.
- `popover-remove-while-open-100000`: show, then remove an ancestor,
  100,000 times.
- `dialog-modal-over-100000-nodes`: a modal dialog over 100,000 elements;
  30 Tab presses and 30 clicks outside (the Tab walk skips inert
  subtrees at their root). 100,000 buttons, a dialog with three; the log
  shows the active element (the dialog) and 0 clicks that reached anything
  but the dialog. Since 6e: 16.5 s, 469 MiB, limit 30 s: the time is the
  restyle and layout of the whole document at each focus change (backlog of
  5d), which the base has too ("6e implementation notes").
- `dialog-showmodal-1000-stacked`: 1,000 modal dialogs, then Escape 1,000
  times. A script opens them without a user activation, so their watchers
  share one group: the first Escape closes all of them (each `cancel` cannot
  be canceled, although each has a listener that tries), the others find
  no watcher (6d).
- `top-layer-backdrop-filter-1000`: 1,000 open popovers with
  `::backdrop { backdrop-filter: blur(20px) }` (ADR 0033's budgets apply).
- `close-watchers-100000-without-activation`: 100,000 watchers made and
  destroyed: a script shows 100,000 auto and hint popovers without a user
  activation (each show hides the earlier ones, so one popover is open at
  the end), then one Escape. It does not test a group of 100,000 watchers:
  watchers that stay alive together exist with nested popovers (see
  `popover-nested-auto-10000`) and stacked dialogs (see
  `dialog-showmodal-1000-stacked`). Since 6d the script also opens and
  closes one dialog 100,000 times (`show()` and `showModal()` in turn, a
  watcher and a `close` task each) and leaves it open as a modal dialog,
  which the Escape closes too.
- `invoker-chain-10000-js-off`: 10,000 nested popovers each with the
  invoker of the next; with JavaScript off, 10 clicks on the first 10
  invokers (not on the whole chain), then a click outside.

### 13. Performance

Most pages have no top layer. The costs for them:

- `Document::top_layer` is `None`: the box tree, the paint builder, the
  pointer path (light dismiss), the key path (Escape) and the style walk
  (inertness) each test it once per pass or per input event. `:popover-open`
  and `:modal` read the flag byte that the element's state reads anyway.
- The removal path tests the flag byte of the removed root and walks
  descendants only when the document has showing popovers or a top layer
  (one test otherwise).
- Measure (base: the commit before each sub-feature; release builds):
  `just snapshot` before and after (no change for the targets with
  JavaScript off and on, except GitHub with JavaScript on); `swbtools
  perf` on the JS-off targets within the run-to-run noise; callgrind of
  `swb --headless` on Wikipedia, Ars Technica and BBC with JavaScript off:
  +0.3 % at most; GitHub with JavaScript on: the `load` time (feature 8's
  budget, 1.56 s) and the time of a hover that opens a tooltip.

### 14. Tests

- Probe cases: `tools/probes/web-top-layer.json` runs in Rust against
  Chromium's values (`crates/engine/tests/probe_net.rs`, a generated table
  as for custom elements and shadow DOM, with departures and "later" rows
  per sub-feature); the cases with `steps` run in
  `crates/automation/tests/real_input.rs`'s way. `noscript-*`: swb has no
  Content-Security-Policy, so these run in an engine test with scripts
  off and the same steps, compared with Chromium's values.
  `tools/probes/top-layer.json` with `just probe --with-swb` and as layout
  tests (`crates/engine/tests/top_layer.rs`, boxes within 0.5 px, the
  `pixels` rows within 2 per channel, as `backdrop_filter.rs` does).
- Unit tests: `swb_dom` (`top_layer.rs`: show and hide with a recording
  host, re-entrant hosts that change the DOM in each event, coalescing,
  the stacks with DOM, invoker and source nesting, hint lists, removal and
  adoption steps, attribute changes, close watcher groups, the
  `ElementData` size); `swb_style` (`::backdrop` rule routing and
  inheritance, the top-layer `position` fix-up, `overlay`, the inert
  flag); `swb_layout` (top-layer boxes as children of the ICB, the
  `display: none` ancestor rule); `swb_paint` (order and hit regions of
  top-layer items, no hit regions for inert boxes); `crates/web`
  (`ToggleEvent`, the IDL conversions, `popoverTargetElement`); the engine
  (light dismiss and Escape with JS on and off, the Tab order with a
  popover and with a modal dialog, `method=dialog`).
- `crates/automation/tests/github.rs`: the popover polyfill does not run
  (no adopted sheet; the 5c-2 assertions change to Chromium's values); a
  hover over the "..." button opens the TooltipV2 popover at Chromium's
  box (125 x 28 at (166, 358)) and the click hides it, as in the replay
  records; no new console error.
- Hostile cases of section 12.

### The plan of sub-features

Five sessions, each with a review and a commit. The order lets GitHub's
tooltips work first (6a, 6b, 6c); dialogs follow.

**6a. Popover model, algorithms and bindings** (`implementer-hard`).
Scope: section 1 (the flag byte, `TopLayerState`, removal, adoption and
attribute steps, the journal entry), section 2 (show, hide, toggle tasks,
coalescing, topmost popover ancestor, auto and hint lists, the host
trait with the web layer's host and the engine's no-script host),
section 3's activation behaviour on the script path, section 9's popover
members and `ToggleEvent`, `:popover-open` in style. Until 6b an open
popover renders as a fixed box in the normal stacking order (UA sheet).
Files: new `crates/dom/src/top_layer.rs`; `crates/dom/src/tree.rs`
(`flags`), `mutation.rs`, `journal.rs`; `crates/style/src/element.rs`;
`crates/web/idl/html.idl`, `dom.idl` or `events.idl` (`ToggleEvent`);
new `crates/web/src/html/popover.rs`; `crates/web/src/html/activation.rs`;
`crates/web/src/events/misc.rs`; `crates/engine/src/document/changes.rs`;
`crates/automation/tests/github.rs` (the polyfill assertions).
Tests: probe cases `api-surface` (popover rows), `popover-reflection`
(popover rows), `show-hide-events`, `toggle-coalescing`,
`beforetoggle-cancel`, `exceptions` (popover rows), `beforetoggle-mutations`,
`removal-and-attribute-while-open`, `auto-stack-dom`,
`auto-stack-invoker-and-source`, `hint-popovers`, `invokers-types`,
`invokers-shadow`; unit tests; hostile cases
`popover-beforetoggle-reentry`, `popover-attribute-flip-100000`,
`popover-remove-while-open-100000`, `popover-nested-auto-10000` (the show
part).
Exit: the cases match Chromium; GitHub's polyfill path is gone and
`github.rs` passes with the new values; the other targets unchanged (JS
on and off); `just check`.
Risk: high (re-entrant algorithms across the DOM and the bindings).

**6b. The top layer in rendering** (`implementer-hard`).
Scope: section 8: `::backdrop` styles, the top-layer fix-ups, `overlay`,
the UA sheet, top-layer boxes under the ICB, their containing blocks,
the paint order, the `display: none` ancestor rule; section 13's
measurements.
Files: `crates/style/src/stylist.rs`, `style_map.rs`, `cascade.rs`,
`ua.css`, the `overlay` property; `crates/layout/src/box_tree.rs`,
`lib.rs`, `positioned.rs`; `crates/paint/src/display_list/builder.rs`;
`crates/engine/src/document/pipeline.rs`, `cssom` (`getComputedStyle`
of `::backdrop`).
Tests: `tools/probes/top-layer.json` without the modal cases
(`dialog-modal-*`, `escape-ancestors-modal`, `dialog-modal-in-iframe`,
`backdrop-paint`, `backdrop-author`, `backdrop-inheritance`, the modal
rows of `overlay-and-states`: 6d); `top-layer-hit-order` rows 1 to 4; unit
tests; hostile cases `popover-manual-10000-open`,
`top-layer-backdrop-filter-1000`; `just snapshot` and the performance
rows.
Exit: the boxes and pixels match; GitHub's hover tooltip renders at
Chromium's box; the targets unchanged; `just check`.
Risk: high (the box tree root, out-of-flow placement and the paint
order).

**6c. Popover interaction** (`implementer`).
Scope: section 4 (light dismiss, JS on and off), section 6 for popovers
(close watchers, groups, user activation, Escape in the key path), section
3 with JavaScript off, section 5 for popovers (focusing steps, previous
focus, the Tab order after the invoker).
Files: `crates/dom/src/top_layer.rs` (watchers); `crates/engine/src/page/
input.rs`, `events.rs`, `forms.rs`, `frame_focus.rs`; `crates/engine/src/
focus.rs`; `crates/engine/src/frames/mod.rs` (activation notifications to
the manager).
Tests: `light-dismiss-*`, `escape-popovers`, `xo-popovers-no-activation`,
`escape-focus-return`,
`popover-tab-order`, `popover-focus`, `noscript-*` (the popover cases);
hostile cases `popover-nested-auto-10000` (the dismissal),
`invoker-chain-10000-js-off`, `close-watchers-100000-without-activation`
(popovers).
Exit: the cases match; on GitHub a hover shows the tooltip and the click
on the "..." button hides it as in the replay; `just check`.
Risk: medium (the pointer and key paths are central, the hooks are small).

**6d. Dialogs** (`implementer-hard`).
Scope: section 9's dialog members, `show`, `showModal`, `close`,
`requestClose`, `closedBy`, `returnValue`, dialog toggle events, `cancel`
and `close` tasks, close watchers of dialogs with the measured `cancel`
rule, `closedby=any` light dismiss, `method=dialog` (JS on and off), the
dialog focusing steps and focus restore, modal dialogs in the top layer
with their backdrop.
Files: `crates/dom/src/top_layer.rs`; new `crates/web/src/html/dialog.rs`;
`crates/web/idl/html.idl`; `crates/engine/src/forms/submit.rs`,
`page/forms.rs`, `page/input.rs`.
Tests: `dialog-basics`, `dialog-form`, `dialog-focus`, `escape-modal`,
`escape-cancel-prevented`, `escape-no-activation`, `escape-nested-dialogs`,
`escape-popover-in-dialog`, `escape-keydown-prevented`,
`escape-nonmodal-and-closedby`, `xo-escape-*`, `xo-groups-*`,
`inert-closedby-any` (the closing part),
`noscript-dialog-*`, the dialog rows of `api-surface`, `exceptions` and
`popover-reflection`; layout cases `dialog-modal-*`,
`escape-ancestors-modal`, `dialog-modal-in-iframe`, `backdrop-*`,
`overlay-and-states`; hostile case `dialog-showmodal-1000-stacked`.
Exit: the cases match; the targets unchanged; `just check`.
Risk: medium-high (many small rules; the form path with JS off).

**6e. Modal inertness** (`implementer`).
Scope: section 7: the inert style flag, hit regions, `elementFromPoint`
and `elementsFromPoint`, focusability and the click focus on the backdrop,
the Tab order under a modal dialog, `:hover` chains, the engine's
selection, frames under a dialog.
Files: `crates/style/src/cascade.rs`, `computed.rs`;
`crates/paint/src/display_list/builder.rs`; `crates/engine/src/focus.rs`,
`hit_test.rs`, `document/geometry.rs`, `document/queries.rs`,
`selection.rs`, `page/frame_input.rs`.
Tests: `inert-click`, `inert-hover`, `inert-tab`, `inert-backdrop-click`,
`inert-selection` (the engine-selection part as an engine test),
`top-layer-hit-order` rows 5 to 10; hostile case
`dialog-modal-over-100000-nodes`.
Exit: the cases match; `just check`.
Risk: medium (one flag, many readers).

Feature 6's exit after 6e: GitHub's tooltips open, render and close as in
Chromium's replay; the probe cases match but the listed departures; the
targets keep their scores with JavaScript on and off.

## Open questions

The orchestrator decided them on 2026-11-13: all as recommended.

1. **Five sub-features.** Recommendation: 6a to 6e as above. 6a alone is
   a full session (re-entrant algorithms, bindings, the GitHub test). If
   the orchestrator wants fewer commits, 6d and 6e can merge, at the risk
   of a session that does not finish.
2. **The algorithms in `swb_dom` with a host trait.** Recommendation: yes
   (section 2): one implementation for JS on and off. Alternative: the
   algorithms in `crates/web` and a reduced copy in the engine for JS off
   (the pattern of `<details>` today); simpler borrows, but two copies of
   the stack rules.
3. **`hint` popovers.** Chromium 148 has them (measured); no target uses
   them. Recommendation: in (a second list in the same algorithms;
   without it `popover="hint"` would reflect as `hint` and behave as
   `manual`). Alternative: map `hint` to `manual` and list it as a
   departure.
4. **The `open` removal of a modal dialog.** Recommendation: follow
   Chromium (section 11). Alternative: the HTML Standard's rule; the probe
   row would then be a departure. (The `cancel` rule needs no decision: a
   control without the probe tool's activation showed that Chromium follows
   the standard, section 6; the earlier "cancelable without activation" was
   the tool's activation.)
5. **The `inert` attribute.** Not in the feature's scope and not used by
   the targets' first views (GitHub's CSS has one `[inert]` rule). The
   mechanism of section 7 covers it with one more condition in the style
   walk. Recommendation: backlog, with a note that 6e's flag is the place.
   Alternative: in 6e, with its own probe cases.
6. **The `toggle` event of `<details>`.** swb fires none today. The
   coalesced toggle task of section 2 is the same mechanism (HTML uses it
   for `details` too). Recommendation: backlog (no target listens to it in
   its first view; BBC's React registers a `toggle` listener on its root
   that sees nothing in Chromium's replay either).
7. **Budgets of section 12.** Recommendation: accept.

## Consequences

- `popover`, `showPopover`, `ToggleEvent` and the dialog methods appear
  for every page with JavaScript on; feature tests take their native paths
  (GitHub's popover polyfill no longer runs; its adopted sheets are gone,
  as in Chromium).
- `swb_dom` gets the top-layer state, popover and dialog algorithms with a
  host trait, and removal and attribute steps for popovers; the flag byte
  of `ElementData` gets three bits and a general name.
- Style gets `::backdrop` as a styled pseudo-element, the top-layer
  fix-ups, the `overlay` property and an internal inert flag; the box
  tree builds top-layer boxes as children of the ICB; the display list
  paints them after the root stacking context.
- The pointer path gets light dismiss and the key path close requests,
  both with JavaScript on and off; the Tab order places open popovers
  after their invokers.
- `form method=dialog` works with JavaScript off.
- Backlog: `command`/`commandfor`/`CommandEvent`, `interestfor`, the
  `CloseWatcher` interface, `@starting-style` and `overlay` transitions,
  `interactivity`, the `inert` attribute and the `<details>` `toggle` event
  (open questions 5 and 6), fullscreen.

## 6a implementation notes

Facts of sub-feature 6a (the popover model, the algorithms and the bindings)
that differ from the sections or that later sub-features need. Code:
`crates/dom/src/top_layer.rs` (state, algorithms, `TopLayerHost`),
`tree.rs` (`ElementData::flags`), `mutation.rs` (removal and attribute
hooks), `journal.rs` (`Change::TopLayer`), `transplant.rs`;
`crates/engine/src/document/top_layer.rs` (`NoScriptHost`), `changes.rs`;
`crates/style/src/element.rs`; `crates/web/src/html/popover.rs` (`WebHost`,
the members, invokers, `popoverTargetElement`, the `toggle` task),
`html/activation.rs`, `dom/attrs.rs`, `events/misc.rs` (`ToggleEvent`),
`events/dispatch.rs` (`EventInterface::Toggle`), `event_loop.rs`
(`Task::Toggle`); `crates/web/idl/html.idl`.

**Spec text against Chromium 148.** The HTML Standard of 2026-11-12 differs
from the measured Chromium in three places; swb follows Chromium:

1. *Nested shows.* The standard has a document-wide "showing popover" flag
   and a "hiding popover nesting count" that make a `showPopover()` inside a
   `beforetoggle` listener throw. Chromium runs it (`beforetoggle-mutations`
   #4, #5): a show of the same element inside its own event opens it once
   (the outer shows find it showing and return), a show of another popover
   opens it and the outer show hides it again (with events). swb keeps only
   the per-element "showing or hiding" flag; a nested show of the same
   element hides the others without events.
2. *Hint popovers.* The standard's "hint stack parent" does not explain
   `hint-popovers` #3 (a hint inside an open auto popover stays open when an
   unrelated hint opens) and #4 (hiding an auto popover closes an unrelated
   hint). The model that matches all rows: a hint whose topmost popover
   ancestor is in the auto list joins the auto list above it; any other hint
   goes to the hint list; each auto show and each hide of a popover of the
   auto list first hides the whole hint list. An auto popover whose ancestor
   is in the hint list goes to the hint list (the standard's downgrade; not
   measured). Light dismiss with hints (6c) has no measured case: the
   current `hide_all_popovers_until` with a hint endpoint hides the auto
   list, which closes all hints too; 6c measures it before it relies on it.
3. *Messages.* Every error of a hide has Chromium's sentence "This might have
   been the result of the "beforetoggle" event handler changing the state of
   this popover.", also the first `NotSupportedError` of `hidePopover`;
   errors of a show get it only after the event. For template contents and
   documents without a browsing context "not fully active" comes before
   "disconnected". Not measured, swb's own words: an element that moved to
   another document during the algorithm, an open modal dialog, a type
   change while other popovers were hidden.

**Structures.** `TopLayerState` holds `layer`, the `auto` and `hint` lists,
`popovers` (per element: list and index, the invoker, the busy flag, the
pending toggle) and `dialogs`, `pointerdown_target` and the pending attribute
hides. The index of each popover in its list is kept (a removal from the
middle renumbers the rest), so the topmost popover ancestor and "hide stack
until" need no map per call; the ancestor walk tests the `POPOVER_SHOWING`
bit before it looks up the index, and stops at the top of the lists.
`DialogData` and `pointerdown_target` have accessors only (6c, 6d fill them).
`TopLayerHost` has three methods in 6a: `doc`, `fire_beforetoggle` (returns
`None` when the host stopped: a termination ends the algorithm, which
returns `PopoverErrorKind::Stopped`; the web host returns the stored
`VmError`) and `queue_toggle_task(element, serial)`. The focus, `cancel`,
close task and activation methods of section 2 come with 6c and 6d.

**Toggle tasks.** `queue_toggle` keeps the old state of a pending task and
gives the new one a serial; the web host removes the queued task of the
element from the event loop (its `TaskKey`, `DocState::toggle_tasks`) and
queues the new one at the end, so a script that shows and hides in a loop
keeps one task per element. The task takes its states with
`Document::take_toggle(element, serial)`; the engine's host takes them at
once. A popover that leaves the arena (`take_subtree`) drops its state.

**Attribute steps.** The DOM notes a showing popover whose `popover`
attribute changed state (`take_popover_attribute_hides`); the web layer's
attribute change path (`dom/attrs.rs`) runs the hides at once with events
(the probe sees `beforetoggle` inside the setter), and the engine's drain
runs any that are left with the host without scripts. Because there is a
window between the attribute change and the hide, the hides of "hide stack
until" ignore the attribute (a list member is hidden even if its attribute
is gone), and the walk removes a member that a hide did not remove, so that
it always ends.

**Invokers on the script path.** `Kind::Invoker` (`button type=button`,
`input type=button`) and the submit and reset kinds act as invokers when
they have a popover target and do not submit or reset a form (Chromium: a
reset button with a form owner resets and does not toggle, measured; the
standard toggles after the reset). Their click role is `Interactive` (as
that of a button that does not act; "Review fixes" below).
`popoverTargetElement` stores the element by node id per invoker; a write of
the `popovertarget` content attribute forgets it.

**Answers.** The probe's step cases need the tool's user activation (ADR
"Measurements"): no 6a case has steps, so 6c sets the mechanism up.

**Numbers** (release builds of cab49f6 and 6a on the same machine):
`just snapshot` (JavaScript off) byte-identical; `compare --all` and
`compare --js` of the 8 fixtures unchanged (GitHub with JavaScript on:
geometry 0.9115, pixels 0.9934, as before: the polyfill's sheet set no
value that renders); callgrind of `swb --headless --replay` with
JavaScript off: Wikipedia +0.008 %, Ars Technica +0.002 %, BBC +0.005 %;
binding rows (instructions per iteration): `setAttribute` +0.6 %, `id`
+0.9 %, append and remove +0.8 %, `textContent` -0.9 %, `innerHTML` +0.4 %,
`createElement` +0.2 %, `classList.add` +0.6 %, `dispatchEvent` +1.0 %, and
the empty loop +1.7 % (no code of 6a runs in it: the layout of the binary);
`showPopover` + `hidePopover` of one manual popover: 11,185 instructions.
GitHub with JavaScript on: a headless load to the settled DOM in 0.49 s
(base 0.49 s). Hostile cases: `popover-beforetoggle-reentry` 0.61 s,
103 MiB (10 `RangeError`s at a nesting of about 400 shows);
`popover-attribute-flip-100000` 0.85 s, 119 MiB (the loop 321 ms);
`popover-remove-while-open-100000` 0.80 s, 118 MiB (237 ms);
`popover-nested-auto-10000` 1.06 s, 108 MiB (the show loop 274 ms).

**Review fixes** (the fix round after the review of 6a; probe cases
`popover-validity-6a`, `popover-target-arenas`, `invokers-in-link`,
`invokers-and-labels` and the key cases `invokers-in-link-*`, three equal
Chromium 148 runs each).

1. *`popoverTargetElement` across arenas.* The table is per document and keyed
   by node id, and a move to another arena (`state::move_node_state`) gives
   the nodes new ids. `html::popover::move_targets` now remaps the keys and
   values of the moved nodes; an entry with one end left behind is dropped
   (the other end is in another arena: the getter is `null`, as measured).
   Ids are never reused: an old id reads an empty comment, so no stale entry
   can name another node. Departure (`TOP_LAYER_DEPARTURES`): Chromium keeps
   the association when only the invoker leaves its target's document and
   comes back; swb has lost it. Tests: the iframe row of the probe and
   `popover_target_element_follows_a_move_to_another_arena` (two documents,
   with and without the GC stress mode).
2. *Not fully active.* `check` tests "not fully active" (template contents, a
   document without a browsing context, an element that moved there inside
   `beforetoggle`) before the state test, so `hidePopover()` and
   `togglePopover()` throw `InvalidStateError` there like `showPopover()`
   does; a hide of a hidden popover of the page that is detached still does
   nothing (measured). The message of a hide has the `beforetoggle`
   sentence; that of `togglePopover` does not.
3. *Invoker in a link.* Chromium opens the popover and follows the link
   (`invokers-in-link`: every button and input type, `show` and `toggle`, the
   click on a child, `click()` and Enter and Space on the focused button); a
   canceled click or a disabled button does neither; a submit or reset button
   with a form owner submits or resets and does not follow the link; a plain
   button in a link follows it too (this was already so). The click role of
   an invoker is `Interactive` now (as a button that does not act, so the
   link stays the target) and `find_target` runs the invoker besides the
   target: the nearest invoker below a link, the nearest above a label, or
   the invoker alone if the walk finds no target. The label rows
   (`invokers-and-labels`) kept their values. A fragment navigation of a
   click happens when the task ends: the probe reads `location.hash` in a
   later task. 6c must give the engine's click path (`page/forms.rs`,
   `click_role` and its activation) the same split with JavaScript off, and
   the keys (Enter and Space reach `click_element`).
4. *Elements of another document.* `popoverTargetElement = element` of
   another document, and the `source` option of `showPopover` and
   `togglePopover`, do not throw (Chromium): the setter writes
   `popovertarget=""` and keeps nothing (the getter gives `null`, a click does
   nothing); the option reaches `beforetoggle` as `source` through the
   host (`WebHost::foreign`), not the algorithm. The queued `toggle`
   event of that show has `source` `null` where Chromium keeps the element
   (departure; rare). `new ToggleEvent(type, {source})` already accepted such
   an element (`[SwbAnyDocument]`); the review's report was about the
   options of `showPopover`. The IDL has `[SwbAnyDocument]` on the setter
   and on `ShowPopoverOptions.source`; the generic conversion still throws
   for every other member.
5. *An open dialog and `showPopover()`.* An open non-modal dialog shows as a
   popover in Chromium (`popover-validity-6a` #1: the `open` attribute alone);
   the early return of 6a came from a misread row (`exceptions` #3 only
   showed that no error is thrown). The check refuses only a modal dialog.
6. *Topmost popover ancestor.* `topmost_popover_ancestor` takes the highest
   position over all flat ancestors; HTML's text takes the nearest open
   popover. The two give the same popover in every state that the algorithms
   reach: a farther ancestor that shows after a nearer one hides the nearer
   one (a show hides everything above its own topmost ancestor), so a nearer
   open ancestor never has a lower position than a farther one.
7. *Cost of a removal while a popover shows.* A removal scans the removed
   subtree for popovers and dialogs while any popover or dialog is in the top
   layer (`top_layer_removed`). Measured by the review: 300 removals and
   insertions of a 50,000-node subtree take 116 ms without an open popover
   and 151 ms with one. The cost is linear in the subtree, as the removal
   itself; no quadratic case was found. Accepted as a known cost; a counter
   of showing descendants per subtree would remove it if a page needs that.
8. *Memory budget.* The case `popover-nested-auto-10000` checks the peak
   resident memory (`rss_limit_mib` of the hostile framework, 160 MiB: the
   "+50 MB" of section 12 over a script page's 105 MiB); it peaks at 110 MiB.

Numbers of the fix round (release builds of cab49f6 and the fixed tree):
`just snapshot` byte-identical; `compare --all` and `compare --js` of the 8
fixtures identical; callgrind JavaScript off: Wikipedia +0.03 %, Ars
Technica -0.03 %, BBC +0.01 %; binding rows (instructions per iteration)
at most +0.9 % (`dispatchEvent` 29,297 to 29,558, `id` +0.9 %, `setAttribute`
+0.8 %, the empty loop +0.8 %); `showPopover` + `hidePopover` 11,350.
Hostile cases: reentry 0.61 s, 105 MiB; flip 0.86 s, 126 MiB; remove
0.81 s, 114 MiB; nested 1.06 s, 110 MiB.

## 6b implementation notes

Facts of sub-feature 6b (the top layer in rendering) that differ from
section 8 or that later sub-features need. Code: `crates/style/src/
cascade.rs` (`top_layer_fixup`, the backdrop in `style_pseudo_elements`),
`stylist.rs` (`RuleTarget` 5, `backdrop_rules`), `style_map.rs`
(`PseudoKind::Backdrop`), `ua.css`, the `overlay` keyword and longhand;
`crates/layout/src/box_tree.rs` (`build_boxes`, `build_top_layer`,
`BuildContext::placed_style`), `positioned.rs` (`lay_out_top_layer`),
`fragment.rs` (`FragmentTree::top_layer`, `roots`), `scroll.rs`, `lib.rs`;
`crates/paint/src/display_list/builder.rs` (`top_layer_box`);
`crates/engine/src/document/geometry.rs`, `resolved.rs`, `selection.rs`;
`crates/web/src/cssom.rs`.

**Measured answers** (Chromium 148, three equal runs of four new cases of
`tools/probes/top-layer.json`: `backdrop-popover`,
`popover-static-position`, `popover-scrolled`, `popover-in-iframe`):

1. *The backdrop of an element outside the top layer.* `getComputedStyle`
   of a closed popover's `::backdrop` gives the author's values, `display:
   block`, `position: absolute`, `top: auto`, `pointer-events: auto`: the
   user agent's backdrop rules do not apply there. The UA sheet of section
   8 therefore has `:popover-open::backdrop, dialog:modal::backdrop`
   (display, position, inset), `dialog:modal::backdrop` (the background)
   and `[popover]:popover-open::backdrop` (`pointer-events`), and the
   `position` fix-up applies to every backdrop. A backdrop has a box only
   for an element in the top layer.
2. *Static position.* A top-layer box with `inset: auto` is at the origin
   of the initial containing block, also inside a positioned ancestor.
3. *Scrollable area.* An absolutely positioned top-layer box extends the
   scrollable area of the viewport (`top: 2000px` gives `scrollHeight`
   2020); a fixed one does not.
4. *Inheritance and paint of a popover's backdrop.* It inherits `color`,
   `font-size` and custom properties from its element; it paints above
   the earlier top-layer entries and below its element; its `z-index` has
   no effect; with `pointer-events: none` (UA, `!important`) a hit goes
   through it.
5. *Frames.* A popover in a frame is placed in the frame's viewport,
   outside a transformed, clipped ancestor; the parent's `elementFromPoint`
   gives the `iframe`.

**Structures.** The fragments of the top layer are not children of the
root fragment: `FragmentTree::top_layer` holds them in document
coordinates, and the walkers (`walk`, `walk_painted`, element boxes, the
used boxes of `getComputedStyle`, the geometry index, the scrollable
area, the text extents of the selection) visit `FragmentTree::roots()`,
each with the state of a root (no ancestor transform, clip, scroll
container or sticky box). The box tree and the root share one box
numbering (`BuildState`), so the layout cache and the out-of-flow pass can
key by box number. An element in the top layer is skipped at its place by
`placed_style` (the box tree, the anonymous tables and the flattening
below the depth limit), only when the layer is not empty. Its box starts
at depth 1. A top-layer element with `display: contents` computes to
`display: block` (the fix-up; measured in Chromium 148, case
`top-layer-display-contents`: one client rect, children in it). A top-layer
element gets a box only if the box-tree walk reaches it at its place (its
flat ancestors render): see "6b fix round" below.

**Paint.** `top_layer_box` opens an empty outer context, so that the box
is not the root (the root's background paints the canvas, and a
`backdrop-filter` at outer depth 0 is skipped), then paints the box as a
stacking context; a box fixed to the viewport gets the viewport clip and
the scroll transform as a fixed box of the root does. Backdrop roots and
the tree order of positioned boxes cover the top-layer trees.

**Not done (backlog).** The engine's selection by the mouse
(`selection::position_at`) searches the root's text only; text in a
top-layer box is selected only by select-all. `offsetParent` and
`offsetTop` of a top-layer element follow its DOM ancestors (not
measured).

**`overlay`.** The UA rule `:popover-open, dialog:modal { overlay: auto
!important }` of section 8 is the top-layer fix-up instead (the same
value for every element in the top layer; no author value wins over a UA
`!important`): as a rule, `:popover-open` was tested against every element
(callgrind on Wikipedia +0.21 % with it, +0.05 % without).

**Numbers** (release builds of 936a658 and 6b on the same machine):
`just snapshot` (JavaScript off) byte-identical; `compare --all` and
`compare --js` of the 8 fixtures identical (GitHub with JavaScript on:
geometry 0.9115, pixels 0.9934; its tooltips are closed at load); callgrind
of `swb --headless --replay` with JavaScript off: Wikipedia +0.049 %, Ars
Technica +0.023 %, BBC -0.005 %; `swbtools perf` within 1 % on every
fixture (run-to-run noise). GitHub with JavaScript on: a headless load in
0.465 s (base 0.469 s); the hover over the "..." button that shows the
tooltip adds 0.165 s of wall time to a run (base 0.168 s; the tooltip was
at the same box in 6a, as a fixed box in the normal order, but not on top
of the page). Hostile cases: `popover-manual-10000-open` 0.56 s, 198 MiB;
`top-layer-backdrop-filter-1000` 2.81 s, 100 MiB;
`popover-nested-auto-10000` now renders its 10,000 open popovers: 1.11 s,
149 MiB (6a: 110 MiB; limit 160 MiB).

### 6b fix round

The review of 6b found four things.

**Boxes only where the walk reaches the element.** `build_top_layer`
tested only that the element has a style, but styles exist for nodes that
render nothing: the fallback children of `canvas`, `video` and `iframe`,
and the content of a closed `details`. Chromium gives such a shown popover
no box (measured, cases `top-layer-not-rendered-*`: `getClientRects()`
length 0; in the closed `details` case `elementFromPoint` gives `html`).
`placed_style` now records each top-layer element that a box-tree walk
reaches at its place in `BuildContext::reached`. `build_top_layer` starts
from the nodes that the root's walk reached and keeps a worklist: building
the boxes of an entry walks its subtree and adds the entries reached there
(a popover inside another popover is reached only in the outer one's box).
The built boxes are sorted by layer position at the end, so the order is
that of the layer whichever of two nested popovers was shown first (cases
`top-layer-nested-outer-first` and `-inner-first`). Each entry costs O(1)
besides its own boxes: 10,000 nested popovers shown inner first lay out in
well under a second (unit test). `push_details_content` drops the entries
reached inside a closed `details` (its `::details-content` box is laid out
but not rendered). Departures that stay out of the top layer (backlog):
swb has no `content-visibility` (`top-layer-not-rendered-content-visibility`
is in `LATER`), and it renders the fallback children of an `object` with a
`type` or `data`, which Chromium does not (a plain `div` child has no
client rect there; `top-layer-not-rendered-object` is in `LATER`).

**`display: contents`.** The top-layer fix-up computes `display: contents`
to `block` for the element (Chromium 148: computed `block`, one client
rect, the child at its place in the box, pixels painted; case
`top-layer-display-contents`). A backdrop with `display: contents` is not
measured.

**Memory of `popover-nested-auto-10000`.** 149 to 153 MiB of 160 in 6b
(110 MiB in 6a). Elements in the top layer are not in the style sharing
cache, and in a chain of nested popovers the parent styles differ, so
each of the 10,000 entries held a computed style of its own and one for
its backdrop (about 3 KB of the 4 KB per entry; boxes, fragments and the
display list hold the rest). `Styler::finish_style` and the backdrop loop
now reuse the previous top-layer element style or backdrop style when the
new one is equal in value (`share_last`, one comparison per entry): 149 to
120 MiB (123 MiB in the last run); `popover-manual-10000-open` 199 to 181
MiB. The limit stays 160 MiB with a margin of 37 MiB.

**Numbers** (release builds of 936a658 and the fixed tree): `just snapshot`
(JavaScript off, 370 files) byte-identical; `compare --all` and
`compare --all --js` identical to the base; callgrind with JavaScript
off: Wikipedia +0.120 %, Ars Technica +0.016 %, BBC -0.019 %. Hostile
cases: `popover-nested-auto-10000` 1.11 s, 123 MiB;
`popover-manual-10000-open` 0.55 s, 181 MiB;
`top-layer-backdrop-filter-1000` 2.81 s, 96 MiB.

## 6c implementation notes

Facts of sub-feature 6c (popover interaction) that differ from the sections
or that 6d and 6e need. Code: `crates/dom/src/top_layer/watchers.rs` (close
watchers), `top_layer/dismiss.rs` (light dismiss), `top_layer.rs` (focus
steps, `TopLayerHost::focused_element` and `focus`), `crates/dom/src/
activation.rs` (`with_invoker`, `swallows_invoker_click`);
`crates/engine/src/page/popovers.rs` (the page's hooks), `document/
top_layer.rs` (`NoScriptHost`, user activation), `page/events.rs`,
`input.rs`, `forms.rs` (hooks, invokers without scripts), `focus.rs` (Tab
order), `frames/mod.rs` (`note_activation` returns the frames reached);
`crates/web/src/html/popover.rs`, `runtime.rs`, `navigator.rs`
(`navigator.userActivation`, new IDL `UserActivation`).

**Measured answers** (Chromium 148, three equal runs of 76 new cases of
`web-top-layer.json` (68 old ones, 144 in all); the `escape-*` cases do not log `toggle`, whose task
runs between the tool's steps at times that vary):

1. *Light dismiss with a hint endpoint.* A click in an unrelated hint closes
   nothing; in an auto popover it closes the open hints; outside it closes
   the hints, then the autos. `hide_all_popovers_until` with an endpoint in
   the hint list now hides the hints above it only (6a hid the auto list
   too). The target of an invoker counts as clicked, with any
   `popovertargetaction`, for auto and hint targets (not manual). A popover
   that a `pointerdown` listener opens is closed by that press's release.
   A click in an iframe does not dismiss the parent's popovers. Dismissal
   moves no focus.
2. *A control inside an invoker button* (`invoker-inner-*`). File and color
   fields, checkbox, radio, link, `summary`: the invoker does nothing.
   `select`, `span`, `img`, `range`, date types, submit inputs: it toggles.
   Text-like fields (text, search, url, tel, email, password, number,
   `textarea`): Chromium opens and closes the popover in one click (two
   `beforetoggle` events, no net change). swb does nothing there
   (`swallows_invoker_click`; the two events are a departure in
   `popover_input.rs`). The 6a code, which scanned past `Blocked` controls,
   toggled for file and color fields: fixed (`first_invoker` stops at them).
3. *Invoker in a link without scripts* (`noscript-in-link-*`): as with
   scripts. `swb_dom::with_invoker` is the one rule for both paths (invoker
   below a link first, invoker above a label as `also`, alone otherwise;
   `ClickRole::Invoker`).

**Close watchers.** `CloseWatchers` in the document's `TopLayerState`:
groups, "allowed number of groups" (starts 1), "next user interaction allows
a new group" (starts true), and the history-action flag. An activation with
the flag set adds 1 to the allowed number and clears it; creating a watcher
sets it (HTML "create a close watcher"). This fits every measured sequence
(`escape-popovers-*`, `xo-*`): no activation, one group; one watcher per
click, one group each; several clicks without a watcher in between add one
group; [p1] [p2] [p3, p4] for show, click, show. `WatcherKind::Dialog` is for
6d, which also reads `has_history_action_activation` and
`consume_history_action_activation`. Activations come from the frame tree
(`Frames::note_activation`: the frame, its ancestors and its origin) and go
to each reached document (`DocumentState::note_user_activation`); an
activation before the first popover is kept (`Document::early_activation`).
The probe tool's evaluations count as an activation: `Page::
note_user_activation` (the tests call it before the steps). A cross-origin
frame gets none. Escape is no activation; its default (`key_in_document`)
calls `process_close_watchers`, after `keydown` with scripts.
`navigator.userActivation` (`hasBeenActive`, `isActive`) exists for the
probe cases.

**Focus.** The previously focused element is recorded only for an auto or
hint popover shown while no popover shows (measured, `popover-focus-restore`).
At hide it gets the focus if the focus is inside the popover and it can take
it. The autofocus search runs only in arenas that ever had `autofocus`
(`ever_had_autofocus`, in `handler_attrs.rs`) and visits 100,000 nodes at
most. `Page::focused_frame` no longer resets the focus of a frame that a
script focused with `iframe.focus()` (the `iframe` is the page's focused
element then); Escape did not reach such a frame.

**Tab order.** A showing popover with an invoker (the click's button or the
`source`) is a scope owner after the invoker's subtree, whether or not the
invoker is focusable; manual popovers too; nested ones inside their
popover's scope. Each popover is visited once: a review of the first run
found an endless walk for invokers inside popovers that follow each other
(10^11 bytes of memory); `flattened_order` skips a popover that it reached
and puts unreached ones at the end in layer order; the unit tests cover a
source that is an ancestor, a descendant, a cycle of two and of three, and a
popover that is its own invoker. Departure: a popover whose source is `body`
is entered backward from the page's end at its last element, Chromium at the
body's last element.

**The nested show loop.** Quadratic cost at 10,000, 20,000, 40,000 nested
shows: 272, 1,096, 4,592 ms (manual popovers: 761 ms at 20,000). The cost is
not the algorithms (the ancestor walk stops at the top of the list) but the
O(depth) walks per show: `is_connected` in the validity checks and the
event path of `beforetoggle` (each event walks all ancestors). No small
fix; recorded as backlog.

**Numbers** (release builds of e243724 and 6c): `just snapshot` (370 files)
identical; `compare --all` and `--all --js` the same scores (GitHub JS on:
geometry 0.9115, pixels 0.9934; Ars Technica JS on: pixels 0.9891 to
0.9901, one extra element fewer); callgrind JavaScript off: Wikipedia
+0.02 %, Ars Technica -0.01 %, BBC -0.01 %; GitHub with JavaScript on
loads in 0.517 s (base 0.517 s). The cost of input events on a page without
popovers was not measured (no tool runs a fixed input sequence under
callgrind); each hook is one test of the top layer. Hostile cases:
`popover-nested-auto-10000` (with the click outside and Escape) 0.60 s,
111 MiB; `invoker-chain-10000-js-off` 0.70 s, 101 MiB;
`close-watchers-100000-without-activation` 0.50 s, 284 MiB.

**Review fixes** (6c, one round).

1. *Which inputs give a user activation* (Chromium 148, three equal runs, 24
   new cases `xo-act-*` in a cross-origin frame that has no activation; the
   frame's own scripts log `navigator.userActivation`, because a query that
   the tool evaluates in the frame is a gesture itself). A letter, Space,
   Enter, Tab and an arrow key: `hasBeenActive` and `isActive` are true at
   `keydown`. Shift, Control or Alt alone, and Escape: false. Shift+A and
   Control+A: the A gives it. A press of the left, the middle or the right
   button: true at `pointerdown`. A move and a wheel: false. swb already
   matched; the cases are rows of `popover_input.rs`, also with JavaScript
   off (the history-action activation of the document). swb has no key for a
   modifier alone, so the test leaves out the steps that press one (no event,
   no activation: the same for the page). The state after a click in the
   frame and a new document: the frame keeps it through a navigation to
   the same site, also to another origin of the site (another port, the
   parent's origin), through a reload and `document.open()`; a navigation to
   another site clears it (`xo-act-after-*`). swb cleared it when the origin
   changed and the document of the frame did not read the frame's state
   (`navigator.userActivation` was false after any navigation): the commit
   keeps it for the same site and copies it to the new document (frames and
   the top level).
2. *The Tab press with nested popovers.* `Popovers::of` ran twice per press
   and walked the DOM and the flat tree up from each invoker. It runs once
   per press now and shares the climbs: the nearest popover above each node
   and the connected state are memoized, and the intervals of the tree of
   popovers answer "is the invoker inside its popover". A unit test compares
   it with the plain definition. Per Tab press, release builds, a chain of N
   nested manual popovers: 6b (e243724) 0.08, 0.84, 3.87 s for N = 5,000,
   10,000, 20,000 (with the `source`; 0.08, 0.84, 4.02 s without); 6c before
   the fix 0.17, 1.19, 5.42 s with the `source` (0.08, 0.85, 4.00 s without);
   after the fix 0.08, 0.85, 3.95 s with it and 0.08, 0.85, 3.92 s without.
   The 6c part (+0.09, +0.35, +1.5 s) is gone. The rest is older than 6c: each
   Tab press moves the focus, which restyles and lays out the 10,000 nested
   boxes again, and `in_closed_details_content` walks the ancestors of each
   node (O(depth)): backlog.
3. *Hostile cases.* `close-watchers-100000-without-activation` tests 100,000
   watchers made and destroyed, then one Escape (one popover is open at the
   end); section 12 says so. `popover-nested-auto-10000` now has the live
   watchers: it counts the open popovers (10,000) before its Escape. The case
   `invoker-chain-10000-js-off` clicks the first 10 invokers.
4. *`Frames::note_activation`* collected the frames in a list and searched
   it for each frame of the origin: a set now.
5. *One activation state.* The frame's record is the source: a commit copies
   it to the document (`DocumentState::user_activation`), and `note_activation`
   writes both. The cases `xo-act-after-*` fail if the copy is missing or the
   rule is by origin.
6. *Docs.* The number of new cases is 76 (it said 66).

## 6d implementation notes

Facts of sub-feature 6d (dialogs) that differ from the sections or that 6e
needs. Code: `crates/dom/src/top_layer/dialog.rs` (`show_dialog`,
`show_modal`, `close_dialog`, `request_close_dialog`, `ClosedBy`, the setup
and cleanup steps, the dialog focusing steps, `dialog_form_target`),
`top_layer/watchers.rs` (`request_close`, the dialog watchers),
`top_layer/dismiss.rs` (`light_dismiss_dialogs_down` and `_up`),
`top_layer.rs` (`DialogData`, the open dialogs list, the new host methods),
`tree.rs` (the bit `DIALOG_OPEN`), `mutation.rs` (the removal of `open`);
`crates/web/src/html/dialog.rs` (`HTMLDialogElement`, `Task::DialogToggle`,
`Task::DialogClose`, the form method `dialog`), `html/popover.rs` (the host
methods), `html/focus.rs` (`focus_dialog`), `html/activation.rs`;
`crates/engine/src/page/forms.rs`, `page/popovers.rs`, `forms/submit.rs`,
`document/top_layer.rs`; `crates/web/idl/html.idl`.

**The `cancel` model check** (section 6 asked for it). The HTML Standard of
2026-11-14 has a step that section 6 left out: "process close watchers" ends
with "if the allowed number of groups is greater than 1, decrement it by 1".
With it the standard gives every measured sequence, the three `xo-escape-*`
cases included. `xo-escape-cancel-click-between`: the dialog's watcher is
made at `load` (one group; "next user interaction allows a new group" is
true); click 1: allowed 2, the flag false, a history-action activation;
Escape 1: 1 < 2 and the activation: `cancel` is cancelable, the listener
cancels it, the activation is consumed, and the decrement gives allowed 1;
click 2: a new history-action activation, but the flag is still false (no
watcher was made since click 1), so allowed stays 1; Escape 2: 1 < 1 is
false: not cancelable, the dialog closes (measured). Without the decrement
the standard would make the second `cancel` cancelable. swb has the step
now (also for popovers: the 6c cases keep their values), with one departure
(review fix 1): the standard's step 3 is outside "if the groups are not
empty", so it lowers the number also at an Escape with nothing open;
Chromium does not (`n-noWatchers-first`: key, Escape, `showModal()`, Escape:
the `cancel` is cancelable), and swb follows Chromium. `requestClose()`
and light dismiss request close without the history-action condition
(`cancel` always cancelable; measured: `dialog-basics` #12 after a consumed
activation).

**Who has a watcher.** As the standard (not as section 6 said): every dialog
that `show()` or `showModal()` opened while connected in the document has a
close watcher from the open to the close (HTML "dialog setup steps" and
"cleanup steps"), and its enabled state is read at each request: enabled if
the computed closed-by state is not "none" or a `requestClose()` runs. A plain
non-modal dialog has a disabled watcher: Escape skips it but it takes its
place in the groups. A parsed or script-set `open` attribute does not run the
setup steps (no case; Escape and light dismiss see only dialogs that the
methods opened). The rows that section 6 measured are the same with both
models (`escape-nonmodal-and-closedby`, `noscript-dialog-escape`).

**Measured answers that the design did not have** (all from the existing
cases, no new probe case):

1. `requestSubmit()` and `submit()` of a `method=dialog` form close the
   dialog with `returnValue` `""` (`dialog-form` #7); the standard's result
   is null, which keeps the value. swb gives `""` without a submitter.
2. The dialog takes the focus itself when it has no focusable descendant
   (`dialog-focus` #3), although it is no focusable area: the hosts have
   `focus_dialog`, which skips the focusable check. The click focus on the
   backdrop (`inert-closedby-any`) is 6e's.
3. A `<dialog popover>` has two toggle task trackers (`dialog-basics` #13:
   two `toggle` events); `DialogData` keeps the dialog's.
4. The dialog focusing steps: the first `autofocus` descendant, else the
   first element of the flat tree that the host can focus, else the dialog;
   at most 100,000 nodes are visited. The focus goes back at the close if it
   is inside the dialog or the dialog was modal (the standard's rule; section
   5 said "always").
5. Removing `open` from a modal dialog (section 11) runs the cleanup steps:
   the watcher goes, the dialog stays modal and in the top layer.

**Departures** (the rows that stay): `api-surface` #4 (no `CommandEvent`,
no `CloseWatcher`: backlog); `inert-closedby-any` #1 in `popover_input.rs`
(the backdrop click does not focus the dialog: 6e); `top-layer-hit-order`
rows 5 to 7 stay in `TOP_LAYER_LATER` for 6e (rows 8 to 10 run). The `toggle`
events of the `escape-*` logs are compared merged per element (6c's
`canon`); the order of a dialog's `toggle` and `close` is not checked there,
and `dialog-basics` sorts it (`NZ`): swb queues `toggle` first (section 11).

**Review fixes.** (1) "Process close watchers" lowers the allowed number of
groups only if the manager has a group (departure from the standard, above;
cases `n-noWatchers-first`, `-two`, `n-after-popover`, `n-baseline`).
(2) "Request to close" of a dialog watcher: a request inside the cancel
action of the same watcher returns true at once, with no close (`r-nested-
requestclose`: the nested `requestClose('B')` leaves `open` true; the outer
request closes with `B`); after the cancel action the close action runs only
if the watcher is still the one that the request started with: each watcher
has a serial (`CloseWatchers::serials`), so a listener that removes the
dialog (`r-remove-in-cancel`: `open` stays, no `close` event) or closes and
shows it again (`q-reshow-in-cancel`: `cancel` at each Escape) ends the
request. The cases `q-close-other`, `q-show-in-cancel`, `q-popover-in-cancel`,
`q-removeopen-in-cancel` and `q-remove-reinsert` already matched and stay as
tests. (3) `focus_dialog` focuses only a dialog that is rendered (not in a
closed `details`, no `display: none` on it or an ancestor, not
`visibility: hidden`): Chromium keeps no focus then, and the element that had
it gets `blur` and `focusout` with no related target; `activeElement` is
`body` (`r-hidden-focus`, `show()` and `showModal()`). (4) `returnValue`
goes with an adopted dialog: `take_subtree` returns the values of the
dialogs of the subtree and `put_subtree` sets them on the copies (`r-adopt`;
`cloneNode` and `importNode` do not copy it). `q-close-other` has a 100 ms
wait after each Escape: the `close` task of the other dialog runs before or
after the next evaluation otherwise.

**Numbers** (release builds of 48e3a7a and 6d on the same machine): `just
snapshot` (JavaScript off, 370 files) byte-identical; `compare --all` and
`compare --all --js` identical to the base (GitHub with JavaScript on:
geometry 0.9115, pixels 0.9934); callgrind of `swb --headless --replay` with
JavaScript off: Wikipedia -0.001 %, Ars Technica -0.022 %, BBC -0.017 %;
GitHub with JavaScript on: a headless load in 0.472 s (median of 5; base
0.474 s). Hostile cases: `dialog-showmodal-1000-stacked` 0.20 s, 72 MiB;
`close-watchers-100000-without-activation` 2.56 s, 288 MiB (6c: 0.45 s; the
100,000 dialog opens and closes, each with its events, a `close` task and the
dialog focusing steps, take the difference); the whole set passes (466 ok,
the known failure `nest-div-100000`).

## 6e implementation notes

Facts of sub-feature 6e (modal inertness) that differ from section 7 or
that a later session needs. Code: `crates/style/src/computed.rs`
(`modal_inert`, `receives_pointer_events`, `is_user_selectable`),
`cascade.rs` (`Styler::blocked`, the flag in `style_element`),
`crates/dom/src/top_layer.rs` (`Document::blocking_dialog`,
`is_modal_inert`, `is_inert_under`), `crates/paint/src/display_list/
builder.rs` (hit regions, `Frame.interactive`), `crates/layout/src/svg/
draw.rs` (hit shapes), `crates/layout/src/fragment.rs` and
`crates/engine/src/selection.rs`, `page/events.rs` (selection),
`crates/engine/src/focus.rs` (`is_focusable`, `next_in_order`,
`click_target`), `crates/web/src/html/focus.rs` (`user_focus`).

**What differs from the design.**

1. *Where the flag is set.* In `Styler::style_element`, not in the walk of
   `run`, so that `compute_element_styles_animated` (`getComputedStyle`) agrees. The
   styler reads `Document::blocking_dialog` once (O(1), see the review
   fixes below); an element then costs one `Option` test. The root
   and the dialog are never shared (`shareable`: the root is excluded, a dialog
   is in the top layer), so a shared style never needs a different flag; the
   children of the root and of the dialog have different parent styles, and
   `share_last` compares whole styles, flag included (unit tests in
   `top_layer_tests/modal.rs`). Pseudo-elements and anonymous boxes inherit
   the flag (`copy_inherited_from`); the dialog's `::backdrop` inherits the
   dialog's `false`, which is what puts a hit region with the dialog as its
   element under every inert point. A popover shown after the dialog, outside
   it, and its backdrop are inert (measured row 7); one inside is not.
2. *Selection through the style.* `ComputedStyle::is_user_selectable()` is
   `user-select` not `none` and not inert; the fragment's `is_selectable`, the
   text of the selection and `selectstart` read it, so inert text gets no
   caret position, no word or paragraph, no highlight, no copy and no select
   all, and the engine's code did not change. A double click on outside text
   selects nothing in swb (Chromium selects a word of the dialog's button:
   section 11, not copied). A single press whose nearest selectable text is in
   the dialog (the backdrop of a dialog) can start a collapsed selection there.
3. *Focus through the DOM.* `Document::is_modal_inert(node)` climbs the flat
   ancestors (the flat depth, once per call) and `is_focusable` uses it, so a
   script's `focus()`, `Page::focus` and a click ignore inert elements without
   a style pass (measured: `e-script`, the focus stays in the dialog). The
   Tab walk (`next_in_order`) starts at the dialog: the plain walk iterates the
   dialog's subtree, the scope walk starts its document scope at the dialog,
   so an inert subtree is skipped at its root (the walk is linear in the
   dialog). A start that is not in the dialog counts as no start, as at the
   end of the order (measured: after the last control the focus leaves the
   document; Shift+Tab then enters at the last control, `e-tab-backward`). A
   popover is placed after its invoker only if the popover and the invoker are
   both in the dialog (`Popovers::of` adds the dialog to the nodes that the
   `Climbs` helper knows); one with an inert invoker stays at its place in the
   dialog, one outside is not in the order (`e-tab-popover-in-dialog`).
4. *The click focus is broader than section 7.* Measured with the `e-click-*`
   and `e-nonmodal-*` cases: a click on the text, the padding or the backdrop
   of an open dialog focuses the dialog (focus events `focusout` of the old
   element, `focusin` at the dialog), also when it is not modal and also
   when the focus was on `body`; a control that is nearer wins (its own
   focus); a dialog that has the focus already gets no event; the dialog needs
   no `tabindex`. `focus::click_target` takes an open `dialog` that is rendered
   as a click target, and `user_focus` of the web layer routes it to
   `focus_dialog` (it is no focusable area). This also removes the departure of
   `inert-closedby-any` #1 (6d).
5. *`elementsFromPoint`* needed no code: a point with no hit region under the
   backdrop is covered by the backdrop of the dialog, and the root element is
   added at the end as before (`dialog, html`).
6. *Frames.* The iframe element is inert like any other: it gets no hit
   region, and the `Frame` item is not `interactive`, so the point falls
   through to the dialog. The frame's own document has its own top layer
   (measured: `inert-click` #5).
7. *SVG.* `hit_shape` ignores an inert shape, as `pointer-events: none`.

**Measured answers that the design did not have** (cases added to
`tools/probes/web-top-layer.json`, generator `out/m10-f6e/gen_probes_6e.py`,
three equal runs; the old cases are unchanged byte for byte, the table of
`escape-popovers` flakes as in 6c and the committed run is kept): the
`e-click-*` and `e-nonmodal-*` rows above; `e-tabindex-click-backdrop` (the same
with a `tabindex` on the dialog); `e-empty-click-backdrop` (a dialog that has
the focus already: no event); `e-tab-popover-in-dialog` (`Enter` on the
invoker shows its popover; Tab: `ipb`, `di2`, leave, `di1`; Shift+Tab leaves,
then `di2`); `e-tab-backward` (leave, `di`, `db`, leave); `e-script`
(`ob.focus()`, `body.focus()`: `activeElement` stays `db`; `:focus` matches
nothing outside).

**Departures** (the rows that stay): `inert-click` #3 and #4 read
`getComputedStyle(el).interactivity` (`undefined` in swb: the property is
backlog, section 11); `inert-selection` is not run in `popover_input.rs`
(both queries use `getSelection()`, which swb lacks; the step is the
Chromium detail of section 11); `crates/engine/tests/modal_inert.rs` checks the
engine's selection instead.

**Feature 6 is complete.** The probe rows that stay outside the run, with
the reason: `api-surface` #3 (`command`, `commandForElement`,
`interestForElement`), #4 (`CommandEvent`, `CloseWatcher`) and #5
(`interactivity`, anchor positioning): backlog, no target uses them;
`popover-target-arenas` #2 and #10 (6a fix round); `invoker-inner-text` and
`-textarea`, `popover-tab-order-source-other-tree` (6c); `inert-click` #3 and
#4, `inert-selection` (above); `invokers-in-link-*` (key cases of
`real_input.rs`); two rendering cases in `LATER` of `tests/top_layer.rs`
(`top-layer-not-rendered-object`, `-content-visibility`: not the top layer).

**Review fixes.** (1) `selectstart` fires for a press on the text of an open
dialog and on a modal dialog's backdrop (`Document::press_focuses_element`: a
dialog that only the dialog rule of `click_target` picked is no focus target;
event targets measured: the text node, `dialog#d` for the backdrop; `x-selectstart-*`).
(2) The click focus rule covers a `dialog` that shows as a popover (no `open`
attribute), with scripts and without (`x-popover-dialog`;
`Document::is_shown_dialog`). (3) One `is_open_dialog`, `Document::is_open_dialog`
in `swb_dom`; the engine, the web layer and light dismiss use it. (4) The
focus test of a popover in a dialog with an inert invoker puts the popover
first in the dialog (`x-popover-inert-invoker-f` and `-b`). (5)
`TopLayerState::blocking` is O(1): the field is made again by
`Document::refresh_blocking` after a modal show, a modal close and a removal that
takes an element out of the top layer (the only places that change the `MODAL`
flag or the layer membership of a modal dialog); `invariants` of the dom
tests compares it with the walk `blocking_scan` after every step. The review
added 31 cases `x-*` to `tools/probes/web-top-layer.json` (the cases that match
Chromium; not added: `x-dialog-tabindex` and `x-popover-inert-invoker`, which show
that Chromium keeps the sequential focus starting point at an element after
`blur()` and swb restarts at the edge: older than 6e, backlog).

**Numbers** (release builds of 234c11f and 6e on the same machine): `just
snapshot` (JavaScript off, 370 files) byte-identical; `compare --all` and
`compare --all --js` identical to the base (GitHub with JavaScript on:
geometry 0.9115, pixels 0.9934; `craigslist-search` JavaScript on: geometry 0
in both, older); callgrind of `swb --headless --replay` with JavaScript off:
Wikipedia 650,833,615 to 650,804,582 Ir (-0.004 %), Ars Technica
1,970,622,916 to 1,970,962,699 (+0.017 %), BBC 7,331,292,810 to 7,332,178,582
(+0.012 %); GitHub with JavaScript on: a headless load in 0.464 s (median of
10 runs for each build; base 0.463 s). Hostile case
`dialog-modal-over-100000-nodes`: 16.5 s and 469 MiB (the case's limit is
30 s); the whole hostile set passes (467 ok, the known failure
`nest-div-100000`). The cost of the new case is older than 6e: every focus change restyles and lays out the
whole document (backlog of 5d); on the same page the base build takes 0.66 s
to load, 11.6 s for 30 Tab presses (0.35 s each) and 6.6 s for 30 clicks
outside, 6e takes 0.68 s, 11.2 s and 5.9 s: the Tab walk is linear in the
dialog now, but a walk of 100,000 buttons was not what the time went to.
