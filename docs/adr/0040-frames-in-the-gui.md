# ADR 0040: Frames in the GUI

- Status: accepted (2026-11-15)
- Date: 2026-11-15

## Context

M11 (roadmap, "M11: Interaction performance"). The diagnosis in
[performance.md](../performance.md) ("Interaction in the GUI (M11
diagnosis)") found three causes of slow input in the GUI shell
(`crates/swb/src/gui/`), apart from the cost of style work:

1. **Every pointer move is handled at once.** `handle_cursor_moved` calls
   `Page::mouse_move` for each `CursorMoved` event. On Wayland, winit
   delivers up to about 100 moves in one batch; each move that changes the
   hovered element costs a full hover change (about 125 ms on GitHub), and
   the frame comes after the whole batch. In the owner's GitHub session the
   time from a move to the next frame was 1.4 s median and 4.7 s at most;
   wheel notches and presses waited behind the moves. The wheel is
   already coalesced: `pending_wheel` collects the deltas of a batch and
   `apply_wheel` scrolls once in `about_to_wait`.
2. **A frame is drawn after every turn of the event loop that did
   something.** `Pump::changed` is true when a network completion or any
   task ran, and the GUI then calls `request_redraw`. Ars Technica's pages
   run a `setTimeout(fn, 10)` loop that changes nothing: the GUI draws up to
   95 frames per second (more than the display's 60 Hz) and the main thread
   stays at 96 to 100 %. The backlog of M8 5b has the same finding for
   `setInterval(f, 0)`.
3. **Every frame is drawn in full and with avoidable copies.**
   `compose_frame` allocates a pixmap of the window and one of the page
   area, rasterizes the page, copies it into the frame with
   `Pixmap::draw_pixmap` (a blending pipeline for an opaque copy),
   rasterizes the toolbar, and converts each pixel into the softbuffer
   buffer: 5.5 ms of overhead at 1280 x 900 and 35.7 ms at 5120 x 1440,
   besides the page raster (7 to 22 ms on GitHub and Ars Technica). The GUI
   also calls `set_title` after every turn of the event loop that changed
   something (`sync_address`; on Wayland the window decorations are then
   drawn again into five new buffers) and `set_cursor` for every move.

What browsers do (behaviour, not code): input is processed per frame
(pointer moves are coalesced to the last position of a frame, the
`mousemove` events of the page follow that rate), a frame is produced only
when something visible changed, and frames follow the display's refresh.

## Decision

### 1. Coalesced pointer moves

The shell keeps the position of the last `CursorMoved` of a batch
(`pending_move`) instead of calling the page at once. It applies the
pending move:

- before any event whose meaning depends on the pointer position or on the
  order of input: a button press or release, a wheel event, a key event, a
  `CursorLeft` (a leave after a move must see the move first);
- before work of the shell that reads the page's pointer state: an
  automation request (its input methods and queries), a resize or a
  scale change, a navigation from the toolbar;
- at the end of the batch, in `about_to_wait`, before `apply_wheel`.

The shell's own record of the pointer (`App::cursor`, which the wheel
and the button handlers read) is updated at once for every move; only the
call into the page waits. The toolbar's drag selection (`selecting`) uses
the same pending move. The cursor shape and the status bubble follow the
applied move.

Consequence for pages: a page gets one `mousemove` (and `pointermove`,
`mouseover` and `mouseout` where the target changed) per applied move,
not per window event; between two applied moves, the targets that the
pointer crossed without stopping get no `mouseover`. Browsers behave in
the same way (they coalesce moves to frames); a page that needs every
position would use `getCoalescedEvents()`, which swb does not have
(backlog if a target needs it).

### 2. A frame only when the page or the toolbar changed

The page answers one question: would a new frame of the page differ from
the last one that it rendered? `Page::frame_outdated()` (name open)
compares the inputs of the last render with the current ones, without
running style or layout:

- for the page's document and every frame document that the last render
  drew: the identity of its display list (a serial that grows each time a
  display list is built; `None` after an invalidation counts as changed),
  its scroll offset, the scroll offsets of its scroll containers (the
  display list is built with them; if a scroll of a container does not
  invalidate the list, the record must hold the offsets), the selection
  that the list shows (the selection generation), and its viewport and
  scale;
- the overlay scroll indicators of the viewport (they follow the scroll
  offset and the content size);
- (the hovered link was an input in feature 2; since feature 3 it is not:
  the page does not draw the status bubble, the shell does, and the shell's
  frame key holds the link.)

`Page::render` records these inputs. Every change that alters pixels must
either invalidate a display list (style, layout, images, fonts, focus
rings, form controls, the caret, frame documents) or change one of the
recorded inputs; the implementation checks each source (section
"Implementation notes") and adds what is missing. This replaces the use of
`Pump::changed` and of the `bool` results of the input methods as the
signal to draw: the shell asks `frame_outdated()` once per batch, after the
pump and the input, and calls `request_redraw` only then. `Pump::changed`
keeps its meaning for the automation server (work happened).

The shell draws a frame for its own reasons too: the toolbar's state
(address text and caret, focus, back and forward, loading, the JavaScript
switch), a resize and a scale change, and the first frame.

A task that changes nothing, such as Ars Technica's timer, then draws
nothing. A `requestAnimationFrame` loop that changes the DOM draws once per
display refresh (section 3).

### 3. Frame pacing

The shell calls `Window::pre_present_notify()` right before
`present()`. On Wayland, winit then delivers the next `RedrawRequested`
after the compositor's frame callback: at most one frame per display
refresh, and none while the window is hidden. On X11 nothing changes. The
event loop keeps running the page's tasks while it waits for the frame
callback (`about_to_wait` still pumps); only the drawing waits.

### 4. Window state only on change

`set_title` only when the title differs from the last one set; `set_cursor`
and `set_cursor_visible` only when the icon or visibility differs.

### 5. Composition (M11 feature 3)

- The frame pixmap stays between frames (it is made again only when the
  window size changes).
- The page is rasterized into a kept pixmap, not copied with
  `draw_pixmap`: the conversion reads the page's rows from it directly (see
  the implementation notes for why not into the frame itself).
- The page area is rasterized only when `frame_outdated()` is true or the
  window changed; the toolbar only when its state changed. A frame for the
  toolbar alone (for example the status bubble of a hovered link, or the
  address caret) keeps the page's pixels.
- The conversion into the softbuffer buffer stays one pass over the frame.

## Consequences

- A missed invalidation now shows as a stale window instead of costing a
  frame. Tests: for each kind of change (DOM change by a script, style
  sheet change, element states, focus, selection, scroll of the page and of
  a scroll container, image and font arrival, form editing, frame
  documents, viewport), `frame_outdated()` becomes true; and an oracle test
  on the fixtures: after scripted input and tasks, whenever
  `frame_outdated()` is false, a render gives the same pixels as the last
  one.
- Pages get fewer `mousemove` events, as in other browsers.
- The headless runner and the automation server do not change: they render
  on request (screenshots), not per frame.

## Alternatives

- A dirty flag set at every change site: more places to keep right, and a
  missed site gives the same stale window; the comparison of recorded
  inputs puts the rule in one place.
- Damage rectangles and partial raster: later, with a raster cache for
  scrolling (backlog). A frame that changes is still drawn in full.
- A thread for rasterization: not needed for the targets once the frame
  count drops; it would also need the page state shared across threads.

## Implementation notes

### Feature 2 (sections 1 to 4)

**Shell** (`crates/swb/src/gui/`). `input_batch.rs` holds `InputBatch`: the
pending move and the pending wheel delta, and the order rule in one
function (`push(Input)` returns the move that must be applied before the
event: a move only replaces the pending one, a press or release, a wheel
event, a key and a leave take it). It has unit tests without a window. The
shell's `App::cursor` follows every move at once. A pending move is also
applied before an automation request (`process_automation`), before a
resize or scale change, at the start of `RedrawRequested` (winit delivers
it after the turn's window events and before `about_to_wait`, so the frame
shows the batch's last move and its wheel instead of a second frame
after them; review of feature 2), and in `about_to_wait` before
`apply_wheel`. (A navigation from the toolbar starts from a press or a
key, which took the move already.) `about_to_wait` does: apply the move,
apply the wheel, pump,
run automation if tasks ran, `sync_address`, then one frame decision
(`frame_wanted`): `Page::frame_outdated()`, or a `FrameKey` that differs
from the one of the last frame (the address field text and selection, its
focus, back, forward, loading, the JavaScript switch, the status link, the
window size and the scale). `request_redraw` is called there, after the
first window creation, and nowhere else; `Pump::changed` and the `bool`
results of the input methods no longer decide a frame. `redraw` stores the
`FrameKey` after the present and calls `pre_present_notify` before it. A
failed frame (`frame_failed`) requests no frame until the window changes
or the user's next input, and a window without pixels none until it has
pixels, so that neither can loop. `set_title` runs when
the page title differs from the one shown, `set_cursor` and
`set_cursor_visible` when the icon or its visibility differs.

**Engine** (`Page::frame_outdated`, `page/pipeline.rs`; `document/
paint_stamp.rs`). `Page::render` records a `FrameRecord`: for the page's
document and for each frame document that has a box, a `PaintStamp` (the
serial of the display list, the scroll offset of the viewport, the scroll
offsets of the scroll containers, the `View`: viewport, scale, indicators,
color scheme), and the frame's viewport (and, until feature 3, the hovered
link). The serial is
taken from one process-wide counter each time `update_display_list` builds a
list (so a frame's new document never reuses the serial of the old one).
`frame_outdated` is true before the first render, when the record differs,
or when `DocumentState::paint_pending` says that a render would run the
pipeline: the DOM has changes that were not drained (generation and
journal), `styles_stale`, no styles while the style sheets are settled, no
layout or display list, or a selection generation that the list does not
show. A document that waits for its style sheets has no styles and nothing
to build: it is not pending. `Page::screenshot` does not record a frame (it
is not what the window shows). While a script runtime holds the document
the answer is true.

Sources of a pixel change, and how each one shows:

| Source | How it shows |
|---|---|
| DOM changes by scripts or the parser, attributes, text, `details` `open`, popovers, dialogs and the top layer | the DOM generation and journal (not drained), then styles dropped, so no list |
| Style sheets (a `style` or `link` element, CSSOM rules, adopted sheets, `media`), arrival of a sheet | stylist and styles dropped; a sheet that arrives makes the styles buildable (`paint_pending`) |
| Element states: hover, active, focus, focus-visible, the focus ring | `restyle` drops the layout when a style differs; a script's focus sets `styles_stale`; a focus that moves between form controls drops the layout (the caret) |
| Selection of the page (user, script) | the selection generation against the one the list shows; the list is dropped when the visible range differs |
| Selection and caret of a form control, values, control scroll | form changes drop the layout (and so the list); a script's value write goes through the DOM or the form state and drops the layout |
| Scroll of the viewport | `PaintStamp::scroll` |
| Scroll of a scroll container | the list is built with the offsets, so a scroll drops it; `PaintStamp::offsets` holds them as well |
| Overlay scroll indicators | they follow the scroll offset (stamp) and the content size (layout, so the list) |
| Images (also lazy ones) and fonts that arrived | an arrival drops the layout or the list |
| Frame documents: their lists, scrolls, viewports, a new or removed frame | one `PaintStamp` per frame with a box, in the record; the frame's viewport is in the record; a change of the parent's layout drops its list |
| Viewport, scale, color scheme, the indicator setting | `View` in the stamp (and the restyle that a change causes) |
| Hovered link | not a page input since feature 3: the shell's `FrameKey` (status bubble) |

The oracle (`crates/engine/tests/frame_oracle.rs`) plays moves, wheel
events, double clicks, drags, Ctrl+A, Tab, arrows, Page Down, Home and End,
a resize and a scale change with pumps in between on hacker-news, the
Wikipedia article and the GitHub page (JavaScript off; JavaScript on when
`fixtures/js/` exists) and compares the pixels of a render against the last
one whenever `frame_outdated()` is false: 72 steps per run, 22 to 49 of them
checked as current. The mutation check (the stamp without the viewport
scroll) fails it at the first wheel event. `crates/engine/tests/
frame_outdated.rs` has one test per source of the table.

Departures: none from the sections. `controls_stale` and
`images.sources_outdated` do not count as pending work: a render does not
consume them (only a style computation does), and they would keep the frame
outdated for ever. Two sources make a frame that the table could avoid: any
DOM change draws one frame, also one that changes no pixel, and a selection
change draws one even if the visible range is the same. Both cost one frame,
not a loop. A page that changes no pixel with a 10 ms timer (Ars Technica)
draws one frame in 10 s.

### Feature 3 (section 5)

**What is kept.** `gui/compose.rs` has `Composer`, one per window size and
scale (`App::composer`; `fits` decides, a new size or scale makes a new one
and so draws everything again). It holds two pixmaps: `page` (the page
area, the target of `Page::render`) and `chrome` (window-sized; only the
toolbar rows and the bubble's rectangle are used). The design in section 5
said "rasterize into the frame, no second pixmap". The page is a kept
pixmap instead, because `Page::render` takes a `&mut Pixmap` and a view
over the lower rows of another pixmap would need a change in the engine's
render path; and a kept page pixmap is what brings the page's pixels back
when the bubble goes away. The page is never copied: the conversion reads
the toolbar rows from `chrome` and the other rows from `page`.

**When each layer is drawn.**

- The page: on the first frame, after a resize or scale change, and when
  `Page::frame_outdated()`. For this, the hovered link is no longer an
  input of `frame_outdated` (the page does not draw it); the shell's
  `FrameKey` has it. A new link alone draws no page.
- The toolbar: when its key changes (the address text, selection and
  focus, back, forward, loading, the JavaScript switch; `ToolbarKey`). The
  rows are cleared first, so the result equals a fresh frame. At a scale
  where `40 * scale` is not whole and rounds down (for example 1.03125) the
  toolbar covers part of the first page row; that row belongs to `chrome`
  (a copy of the page's row, drawn over), so the toolbar is drawn again
  when the page is.
- The status bubble: its list is separate from the toolbar's
  (`Toolbar::status_list`). When the link text changes, or the page or the
  toolbar was drawn, the page's pixels under the bubble's rectangle (with a
  2 px margin for anti-aliased edges) are copied from `page` into `chrome`
  and the bubble is drawn there. The conversion puts that rectangle over the
  page's rows. When the link goes away, the rectangle is not used: the page
  pixels show again at once, with no raster. A bubble over the toolbar (a
  window lower than 62 px) draws over the toolbar's rows; when such a
  bubble goes away, changes or is drawn again over a new page, the toolbar
  is drawn again first, so that neither the old bubble nor a second copy
  of it stays on the toolbar (found in the review).

**Conversion.** One pass over the rows into the softbuffer buffer
(`Composer::write_buffer`). It does not skip rows: softbuffer's buffer may
be a different one each frame (the age can be 0), so old contents cannot be
relied on; a full pass is 0.5 ms at 1280 x 900 and 3.3 ms at 5120 x 1440,
within the budget.

**Headless.** `render_window` (`--with-chrome`) keeps the simple path
(`compose_window`: fresh pixmaps, `draw_pixmap`); the unit tests compare the
`Composer`'s frames with it after each kind of change at scales 1, 33/32
and 2 (also the converted buffer), and count the rasters of each layer.

**Numbers** (release, Xvfb, `gui-latency`, github-swb; the compose time of
the frame log, median in ms): 1280 x 900: 6.8 before, 0.5 after (JS on,
`github-owner`) and 6.8 / 0.5 (JS off, `pointer-sweep`); 5120 x 1400: 42.6
before, 3.4 after (`github-owner`) and 43.2 / 3.2 (`pointer-sweep`). Both
are within the exit limits (1 and 5 ms). The `--with-chrome` screenshots of
senko-net, hacker-news and github-swb at scales 1, 1.25 and 2, and the
JS-off snapshot, are byte-identical to the base.
