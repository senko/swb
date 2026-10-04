# ADR 0013: HTML forms

- Status: accepted
- Date: 2026-10-03

## Context

M2 (Hacker News) needs forms: the search field in the footer
(`<form method="get" action="//hn.algolia.com/">` with one text field and
no button) and the login page (`POST` forms with text, password, hidden
and submit inputs). Forms need state that is not in the DOM (the `value`
and `checked` attributes are only defaults), text editing with a caret and
a selection, rendering that matches Chromium, the activation of buttons,
checkboxes, radio buttons and labels, and submission with `GET` and
`POST`. swb has no JavaScript, so nothing changes the DOM after parsing.

## Decision

### Control state

- The page keeps the state of every form control of the document in
  `swb_engine::forms::Forms`, keyed by `NodeId`: the value (a
  `TextEdit` with the caret and the selection), the checkedness, the
  selectedness of the options of a select, and the scroll offset of the
  text. It is built from the attributes when a document is committed, with
  value sanitization, radio button groups (the last checked button of a
  group stays checked) and the selectedness setting algorithm of selects.
  swb has no dirty flags: without scripts, nothing changes the defaults
  after parsing.
- Facts that depend only on the DOM (the form owner, disabled, barred from
  constraint validation) are computed once per document, so that a
  keystroke or a submission costs linear time also with thousands of
  controls that have a `form` attribute or are in a disabled fieldset.
- `TextEdit` (`crates/engine/src/edit.rs`) is the one editing model of
  the project: page text fields and text areas, and the address bar of the
  GUI. Positions are byte offsets; the caret moves and deletes by
  grapheme clusters; `maxlength` counts UTF-16 code units. Inserted line
  breaks become spaces in a text field (Chromium does this for pasted
  text) and tabs stay. The caret starts at the start of the value; after
  a reset, it is at the end of a text field and at the start of a text
  area. This is where Chromium (measured) puts text that is typed after
  a label focuses the field.
- The form owner follows the HTML algorithm: the `form` attribute, else
  the form that the parser associated with the element (the form element
  pointer, which html5ever reports through `associate_with_form`; it is
  stored in `ElementData::parser_form`), else the nearest `form` ancestor.
  This makes `<table><form><tr><td><input>` work.
- Style gets the states that depend on the control state through
  `ElementStates::controls`: per element, the `swb_style::CONTROL_STATES`
  flags (`:checked`, `:placeholder-shown`, `:valid`, `:invalid`) replace
  the ones from the attributes. A change restyles only if a selector uses
  one of these states (as for hover, ADR 0009). Controls that are barred
  from constraint validation match neither `:valid` nor `:invalid`.
  `:disabled` follows the HTML rules (`swb_style::is_actually_disabled`:
  disabled fieldsets, their first legend, disabled optgroups). Style,
  focus navigation and the control state compute it for all elements in
  one pass (`swb_style::DisabledElements`). `::placeholder` is a
  pseudo-element style (`PseudoKind::Placeholder`, color `#757575` as
  measured in Chromium).
- Value sanitization follows the input type: line breaks are removed from
  text-like values, and number, range, color, date, month, week, time and
  `datetime-local` values that are not valid become empty (or the default
  of a range or color). A valid `datetime-local` value is normalized
  (`2024-01-02 03:04:00` becomes `2024-01-02T03:04`). Deviation: dates
  after 275760-09-13 (the end of the ECMAScript time range) are invalid,
  as in Chromium (measured); the specification has no maximum.

### Rendering

- A control is an atomic box with its own formatting context, like a
  replaced element, also with `display: inline` or `block`. Layout gets
  the controls through `LayoutInput::controls`
  (`swb_layout::FormControls`, implemented by the engine): the kind, the
  text to show (the value, one bullet per character of a password, the
  placeholder, a button label, the selected option), the options of a
  select, the caret offset, the scroll offset and the checkedness. All
  of this is in
  `crates/layout/src/control.rs`; `box_tree.rs`, `block.rs`,
  `intrinsic.rs` and `inline/mod.rs` only call it.
- Box construction gives a control generated content: one inline
  formatting context with the text (text fields, buttons, selects, text
  areas), the DOM content (`<button>`: a block container, or flex items
  if the button has `display: flex` or `inline-flex`), or nothing
  (checkboxes, radio buttons). The text of a text field has caret stops
  that are byte offsets in the shown text; its text fragments belong to
  the control element. The page selection skips the text of controls
  (as Chromium's selection does not enter the shadow tree of an input).
- Sizes follow measurements of Chromium 148 with the test fonts (the
  formulas are in the module documentation of `control.rs`; the layout
  tests `tests/layout/forms-*.html` check them). The width of a text
  field comes from OS/2 `xAvgCharWidth` and the font's bounding box at
  the font size in 26.6 fixed point, rounded as Blink rounds; families
  that Blink does not trust (`Courier`, `Times`, `Helvetica`, ...) use the
  width of `0`. `swb_text::FontMetrics` has the two widths for this.
  Controls keep their intrinsic width when they are block-level.
- Paint draws Chromium's light theme for controls whose border and
  background are the user-agent defaults ("native look"; Chromium uses
  `appearance: auto` unless the author styles the border or the
  background). swb compares computed values, because the cascade does not
  record which origin set a property. Checkboxes and radio buttons always
  get the native look (swb has no `appearance` property). The check mark
  and the select arrow use a new display item, `DisplayItem::Polyline`.
- The caret is computed by layout (`ControlContent::caret`), because its
  position comes from the caret stops of the laid-out text. Layout also
  computes the scroll offset that keeps the moving end of the selection
  visible (`Control::focus`: the caret position, also with a selection or
  in a read-only field), starting from the previous offset; the engine
  stores the used offset after each layout. An edit or a caret move
  therefore needs a new layout of the page (no restyle unless a state
  pseudo-class changed). The caret does
  not blink, so headless output does not depend on time. Read-only
  fields have no caret (as in Chromium). On an empty line of a text area
  (no caret stops), the caret position comes from the line breaks after
  the last caret stop. The selection in the focused control is
  highlighted through `swb_paint::Highlights`.

### Interaction

- `Page::key_down` gives keys to the focused control first: typing,
  Backspace, Delete, the arrows (with Shift to select, with Ctrl by
  words), Home, End, Ctrl+A in text fields; Space and Enter on buttons;
  Space on checkboxes and radio buttons, the arrows between the radio
  buttons of a group; the arrows, Home, End and type-ahead in selects.
  `Page::insert_text` inserts typed or pasted text. Keyboard focus (Tab)
  selects the text of a text field (not of a text area), as in Chromium;
  a click places the caret, a drag selects, a double click selects a word
  (all of a password field: word steps and word deletion in a password
  field go to its ends, so that they do not reveal its words).
- A click runs the activation behavior of the nearest link, button,
  checkbox, radio button or label under the pointer, if the press and the
  release hit the same one. A label focuses its control and clicks it. A
  click on interactive content inside a label (a field, a select) does
  not activate the label. As in Chromium (measured): a button without an
  activation behavior (`type=button`, or a submit button without a form
  owner) and a label without a control let the click go to a link around
  them; a label that clicks a control without an activation behavior (a
  text field, a `type=button` button) focuses it, and the click goes on
  to a link around that control; a disabled control gets no click, so
  nothing around it activates, and a label with a disabled control does
  nothing.
- Enter in a text field does implicit submission: it clicks the form's
  default button, or submits a form without a submit button that has at
  most one text field.

### Submission

- `crates/engine/src/forms/submit.rs` implements the form submission
  algorithm: the entry list (submitter, checkboxes, selects, image button
  coordinates, `_charset_`, `dirname`), the encoding (`accept-charset`,
  else the document encoding, which the page now keeps), the action and
  method with `formaction`, `formmethod`, `formenctype` and
  `formnovalidate` (enumerated values are matched ASCII
  case-insensitively and not trimmed: `method=" post"` is `GET`, as in
  Chromium), and the three encodings (`encode.rs`):
  `application/x-www-form-urlencoded` (unmappable characters become
  numeric character references), `multipart/form-data` (boundary
  `----swbFormBoundary` and 16 random letters and digits) and
  `text/plain`.
- Constraint validation checks only `required`, for the input types that
  the specification lists, text areas and selects (for a select, the
  placeholder label option counts as no value). A radio button group is
  missing a value if one of its buttons is `required` and none is
  checked. `maxlength` applies to text areas and to inputs of type text,
  search, url, tel, email and password. An invalid form is not
  submitted; the first invalid control gets the focus (Chromium also
  shows a message; swb does not).
- Deviations in `accept-charset`, as in Chromium (measured): commas
  separate labels as well as whitespace, and a value that names no known
  encoding gives the document encoding (the specification splits on
  whitespace only and then uses UTF-8).
- `dirname` adds an entry for text areas and for inputs of type text,
  search, tel, url, email, password, hidden, submit, reset and button.
  The direction comes from the nearest valid `dir` attribute (`ltr`,
  `rtl`, `auto`; others are skipped); without one, `ltr`.
  Simplification: `dir=auto` gives `ltr` (it should look at the first
  strong character of the value).
- A web page cannot submit a form to a `file:` URL (as for links).
- Deliberate simplifications: `target` is ignored (the result replaces
  the page), `mailto:` and `javascript:` actions and `dialog` forms do
  nothing.

### Navigation with a request and history

- `Page::navigate_with(Request)` starts a navigation with a `GET` or a
  `POST` request. A form submission builds its request with
  `Request::post` and gives it the origin of the form's document as the
  initiator (ADR 0012): a `POST` sends it as `Origin`, and cookies use it
  for `SameSite`.
- The pending navigation and the history entry keep the `POST` body (with
  its content type) and the initiator; entries for fragments of the same
  document keep them too. A reload and back and forward send the
  initiator again, so a reload after a cross-site link or form gets the
  same cookies as the first request.
- A reload sends the body of a `POST` entry again. Going back or forward
  to such an entry from another document does not send it: the page shows
  "Confirm form resubmission" (a failed load) and a reload sends it, as
  Chromium's `ERR_CACHE_MISS` page does. Deviation: Chromium asks for
  confirmation before it sends the data again on a reload; swb has no
  dialogs, and a reload is an explicit user action.
- A reload while a `POST` is pending drops it and reloads the current
  entry (as Chromium does), so that the reload does not send that form
  again. Limits: if the current entry is itself a `POST` result, the
  reload sends its body again; without a committed entry (a `POST` as
  the first navigation of the page), the reload does nothing and the
  pending `POST` goes on.
- An entry for a fragment of a document keeps the `POST` body and the
  initiator of the document's entry, because a reload of it requests
  that document again.
- Each committed document gets a number, and history entries record it.
  Back and forward only scroll when the entry shows the current document
  (a fragment of it); otherwise they load, also when the URL is the same
  (a form that posts to its own URL, then back to the form).
- `swb_net::Response::redirected` tells that the response came after one
  or more redirects (`fetch_following_redirects` sets it). The entry of a
  `POST` that redirected keeps no body: a `POST` that redirects is
  usually followed by a `GET` (post/redirect/get), and a reload must not
  send the form again, also when the redirect goes to the same URL. Such
  an entry is still a new entry, also for the URL of the current entry.
  Deviation: the HTML standard ("navigate") turns every navigation to
  the URL of the current document into a replacement; Chromium
  (measured) adds an entry for a `POST`, and so does swb. Simplification:
  a 307 or 308 redirect keeps the method and the body, but its entry has
  no body either.

### Automation

- `input.type` types text into the focused text field or text area, as
  key presses would; a line break presses Enter and a tab presses Tab
  (the focus moves, and typing goes on in the next field). It fails if
  nothing editable has the focus. `dom.value` returns the value and the
  checkedness of a control; the value of a password field is returned in
  plain text (the automation server has no authentication, see
  `docs/automation.md`). This replaces the note "`input.type` is not
  implemented" of ADR 0008.

Update (2026-10-04, M2 maintenance review): the state type is `Forms` in
`crates/engine/src/forms/mod.rs`; it is internal to the engine crate
(there is no public `swb_engine::forms` module). Enter on a focused
checkbox or radio button also does implicit submission. `novalidate` and
`formnovalidate` are checked by `Page::submit_form`
(`crates/engine/src/page/forms.rs`) before `submit.rs` builds the
request. The decision did not change.

## Consequences

- Text fields, text areas, buttons, checkboxes, radio buttons and
  drop-down selects render with Chromium's sizes and light theme, can be
  edited and submitted, and work in the GUI and through automation.
- Every keystroke in a field lays out the page again. On large pages this
  costs a full layout per key (8 ms on the Wikipedia fixture); incremental
  layout is in the roadmap.
- The native-look heuristic compares computed values: an author style
  that sets the default values explicitly keeps the native look, while
  Chromium would use the CSS look.
- Not implemented: list boxes (`<select multiple>` or `size` > 1 look
  like drop-downs, and their options have no boxes); the popup list of a
  select (the keyboard changes the selection); date, time, color and range
  inputs (text fields); file inputs (a button without a file chooser;
  submitted as an empty file); image buttons show their `alt` text, not
  the image; a click on an empty line of a text area places the caret on
  the nearest line with text; the legend of a fieldset is laid out
  inside the border instead of on it; `wrap=hard` in text areas;
  validation other than `required`, and its messages; IME composition in
  the GUI; the middle-click paste into a field; scrolling a text area
  with the mouse wheel; vertical caret movement in a text area moves by
  logical lines, not by wrapped lines.
