# ADR 0009: Interaction: element states, focus and text selection

- Status: accepted
- Date: 2026-10-02

## Context

M1 adds the interactions that a reader of a page uses: links that change on
hover, the mouse cursor, keyboard focus with Tab, and selecting and copying
text. The engine has no incremental style or layout (each change recomputes
everything), and the layout is rebuilt on every change. Real pages must stay
responsive: on the Wikipedia fixture, style takes about 8 ms and layout
about 8 ms (see [performance.md](../performance.md)).

## Decision

### Element states and restyling

- The page keeps the element states (`swb_style::ElementStates`): hovered,
  active, focused (with a `focus_visible` flag) and target elements.
  `:hover` and `:active` match the element and its ancestors.
- When a state changes, the page restyles only if a selector in the
  stylesheets depends on that state (`Stylist::state_dependencies`, the
  union over all selectors). The restyle computes all styles again. If no
  style changed (`StyleMap::same_styles`), the layout stays.
- Scrolling updates the hovered link and the cursor, but `:hover` waits
  for the next mouse movement, as in Chromium; otherwise each scroll step
  on a page with `:hover` rules would restyle the page (24 ms per step on
  Wikipedia).
- `:focus-visible` matches after keyboard focus (Tab), not after a click,
  as in Chromium. `:target` follows the URL fragment.

This is not incremental restyling, but it costs nothing when a page has no
rules for a state, and it avoids a layout when the rules do not match the
elements involved. Incremental style and layout remain in the roadmap.

### Pointer, clicks and keys

- The engine gets mouse down, move and up events and key presses in its own
  types (`swb_engine::{MouseButton, Modifiers, Key}`), so the GUI and the
  automation server drive it the same way. Key names are the DOM
  `KeyboardEvent.key` values.
- A link is followed on the release, if the press and the release were on
  the same link element and the pointer did not move more than 4 px
  (Chromium's drag threshold) in between. A larger movement selects text.
- The cursor comes from the computed `cursor` property; `auto` is the text
  cursor over selectable text, otherwise the default cursor.
- The browser window keeps its own shortcuts (Ctrl+L, Ctrl+C, Backspace
  for back, Escape for stop). The page handles Tab, Enter, Ctrl+A and the
  scrolling keys.

Update (2026-10-04): since [ADR 0013](0013-forms.md), a focused form
control gets the keys first (`Page::key_down`). While an editable text
field or text area has the focus, Backspace, Ctrl+X and Ctrl+V go to it;
otherwise Backspace goes back. A focused text control has its own
selection, which Ctrl+C copies instead of the page selection.

Update (2026-10-04): since [ADR 0016](0016-positioning-and-transforms.md),
hit testing (in the display list) uses the scroll offset and transforms:
a fixed box is hit where it is on the screen, a sticky box at its stuck
position, a transformed box where it is painted (a transform that cannot
be inverted is never hit), and an absolutely positioned box is clipped
only by the boxes in its containing block chain. Text positions for
selection (`selection::position_at`) still use the layout geometry, so in
transformed boxes, in sticky boxes that are stuck, and in fixed boxes of
a scrolled page, a click selects at the layout position of the text.

### Focus

- Focusable elements and the sequential focus order follow the HTML
  specification: links with `href`, form controls that are not disabled
  (also through a disabled `fieldset`), `summary`, elements with
  `tabindex`; positive `tabindex` values first, then tree order. Without a
  focused element, Tab starts after the last clicked node (the sequential
  focus navigation starting point).
- Elements that are not rendered (no box, or not visible) cannot get the
  focus, and Enter does not follow a focused link that stopped being
  rendered (for example in a menu that opens on hover).
- A navigation to a fragment of the same document focuses the target if
  it is focusable; otherwise it removes the focus and the next Tab starts
  at the target ("skip to content" links).
- After the last element, the focus leaves the page (as Chromium moves it
  to the browser window), and the next Tab starts at the first element.
- Keyboard focus scrolls the element into view as Chromium does ("center
  if needed"): not at all if it is visible, to the nearest edge if it is
  partly visible, centered if it is not visible.
- `outline-style: auto` is drawn as Chromium draws it (measured with
  Chromium 148): a 2 px ring in the outline color centered on
  `outline-offset` (1 px further in when the box has a border on every
  side), a 1 px white ring around it, rounded corners. The ring encloses
  the descendants too (an image in a link); an empty box gets no ring.
  The UA stylesheet has Chromium's
  `a:any-link:focus-visible { outline-offset: 1px }`. Deliberate
  simplification: each line fragment of a wrapped link gets its own ring;
  Chromium merges them into one outline.

Update (2026-10-04): since [ADR 0019](0019-scroll-containers.md), focus,
fragment navigation and automation clicks scroll every scroll container
that contains the element, then the viewport. The scrolling keys and the
mouse wheel scroll the first scroll container in the scroll chain (of the
focused element or the last clicked node; of the node under the pointer)
that can scroll in their direction, else the viewport. Scrolling an
element updates the hovered link and the cursor, but not `:hover`, as for
the viewport. Text selection and hit testing use the scrolled positions.

### Text selection

- A selection is a range between two **positions in text nodes** (node and
  byte offset), not between positions in the layout. The layout is rebuilt
  on every change (a hover restyle, a resize); DOM positions stay valid.
- White-space processing and `text-transform` change the text, so the box
  tree keeps a **source map** for each inline text item: the offset in the
  text node of each character of the processed text
  (`layout/src/source_map.rs`). Characters whose length changes
  (`ß` → `SS`) map as a unit, so mapped offsets are always character
  boundaries in the node's data.
- Each text fragment stores **caret stops**: the node offset and the x
  position of each glyph cluster boundary. Hit testing a point gives a
  position (nearest caret stop); painting a selection gives the x range of
  the selected offsets. Generated content (`::before`, `::after`, list
  markers) has no caret stops and cannot be selected, as in Chromium.
- The position at a point is searched in the innermost block that contains
  the point and has text: the nearest line, then the nearest fragment.
- A double click selects a word (UAX #29 word boundaries, within one text
  node), a triple click the paragraph (the text of the enclosing block
  between nested blocks and `<br>`), Shift+click extends the selection.
- The highlight is part of the display list: a rectangle behind the
  selected glyphs, then the glyphs and decorations again in white, clipped
  to the rectangle. The colors are Chromium's defaults (background
  `rgb(51, 103, 209)`, text white). The rectangle fills the line box, so
  selected lines have no gaps. Text with `user-select: none` is not
  selected, highlighted or copied. A selection change rebuilds only the
  display list.
- The copied text follows the HTML `innerText` algorithm for the selected
  range: collapsed white space, one line break between blocks, two around
  paragraphs, tabs between table cells, and only rendered text.
  `text-transform` is not applied (a deliberate simplification).

### Clipboard

- The GUI uses `arboard` (MIT/Apache-2.0) with the Wayland data-control
  feature. GNOME (Mutter) does not offer the data-control protocol, so
  arboard falls back to X11 through Xwayland. Owning the Wayland clipboard
  directly needs the raw `wl_display` of the window, which needs `unsafe`
  code (forbidden by the workspace lints).
- On Linux, a selection also sets the primary selection (middle-click
  paste).

## Consequences

- Hover on pages with `:hover` rules costs a full restyle and often a full
  layout. On the Wikipedia fixture that is about 16 ms; acceptable until
  incremental style and layout exist.
- Text fragments carry caret stops (8 bytes per cluster) and their line
  box.
- Copy and paste through Xwayland depends on Mutter's clipboard bridge
  between X11 and Wayland clients. It cannot be tested automatically; the
  owner checked it on GNOME 48 (copy and middle-click paste into Wayland
  applications work).
- Selection behaviors that are not implemented yet: the highlight of line
  ends inside the selection (Chromium paints a space-wide box), words that
  cross element boundaries (`un<b>believ</b>able` selects only `believ`),
  extending by words after a double click, auto-scroll while dragging
  beyond the viewport, `::selection` styles, the tint of selected images,
  and dragging links or selected text.
- After a navigation, the hover state of the new document waits for the
  next mouse movement.
