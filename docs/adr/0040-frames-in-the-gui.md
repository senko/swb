# ADR 0040: Frames in the GUI

- Status: proposed
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
- the hovered link (the status bubble).

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
- The page is rasterized straight into the frame: the rows below the
  toolbar are one contiguous part of the frame's pixel data, so a
  `PixmapMut` over that part is the page's target. No second pixmap, no
  `draw_pixmap`.
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

(Added by M11 features 2 and 3.)
