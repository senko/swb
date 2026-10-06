# Automation protocol

swb can be driven by other programs: tests, the Python tools, and later a
developer inspector. This document is the protocol reference. The design and
its reasons are in [ADR 0008](adr/0008-automation-protocol.md).

## Starting the server

```
swb --headless --remote-port 0 [--replay DIR] [--test-fonts] [--size WxH] [URL]
swb --remote-port 9222 [URL]          # with a window
```

- The server listens on 127.0.0.1 only. Port 0 picks a free port.
- swb prints the address on standard output, as the first line:
  `swb: automation server listening on ws://127.0.0.1:PORT/`.
- In headless mode, swb runs until a client sends `browser.close`. With a
  window, closing the window also ends it.
- The server rejects WebSocket handshakes that have an `Origin` header.
  Browsers always send one, so web pages cannot connect; the swb clients do
  not send one.
- There is no authentication: every local process that can connect to the
  port controls the browser. It can also read all cookies with
  `cookies.get`, including `HttpOnly` session cookies, and the values of
  password fields in plain text with `dom.value`. Do not start the
  server in a session that is logged in to accounts that matter, on a
  machine that other users share.
- Limits: at most 16 connections at the same time; a request message is at
  most 1 MiB (a larger one closes the connection with the close code 1009,
  without a response); the handshake must complete in 10 s.

## Messages

All messages are WebSocket text messages with one JSON object.

Request:

```json
{"id": 1, "method": "page.navigate", "params": {"url": "https://senko.net/"}}
```

- `id`: a number or a string. The response repeats it.
- `method`: `domain.verb`.
- `params`: an object. It can be omitted or null if the method has no
  parameters. Unknown parameters are ignored.

Response, success and error:

```json
{"id": 1, "result": {}}
{"id": 1, "error": {"code": -32602, "message": "missing field `url`"}}
```

Error codes:

| Code   | Meaning |
|--------|---------|
| -32700 | The message is not valid JSON (`id` is null). |
| -32600 | The message is not a valid request (`id` is null). |
| -32601 | The method does not exist. |
| -32602 | The parameters are not valid. |
| -32000 | The request failed: no document is loaded, the node does not exist, the node has no box, the screenshot is too large. |

A connection handles its requests in order: the server reads the next
request after it sent the response to the previous one. Several clients
can connect at the same time; all requests go to the same page, and a
waiting `page.waitForLoad` on one connection does not delay the others.
While a request waits, the server does not answer WebSocket pings on that
connection, so clients must not close the connection for a missing pong.
There are no events yet; use `page.waitForLoad` to wait for a load.

## Conventions

- Coordinates are CSS px. Points for input are in viewport coordinates (0,
  0 is the top left corner of the visible area). Boxes are in document
  coordinates (independent of the scroll position of the page, but with
  the scroll offsets of scroll containers applied, as Chromium's
  `getClientRects` gives them).
- A rectangle is `[x, y, width, height]`.
- A node ID is an integer (the index of the node in swb's DOM arena). It is
  valid until the next navigation.
- Modifiers are a list of names: `Shift`, `Control` (or `Ctrl`), `Alt`,
  `Meta`.
- Keys are DOM `KeyboardEvent.key` values: one character (`"a"`, `" "`), or
  `Tab`, `Enter`, `Escape`, `Backspace`, `Delete`, `ArrowUp`, `ArrowDown`,
  `ArrowLeft`, `ArrowRight`, `PageUp`, `PageDown`, `Home`, `End`. `Space` is
  accepted for `" "`.

## Methods

### page

| Method | Parameters | Result |
|--------|------------|--------|
| `page.navigate` | `url` (absolute URL) | `{}`. The load continues after the response; use `page.waitForLoad`. |
| `page.reload` | — | `{}` |
| `page.back`, `page.forward` | — | `{"navigated": bool}` |
| `page.stop` | — | `{}` |
| `page.waitForLoad` | `timeoutMs` (default 30000, at most 3600000) | `{"loaded": bool}`: false on timeout. Loaded means the document and all its resources (stylesheets, images, background images) are loaded. A failed navigation counts as loaded: then `page.info` has `loadState` `failed`. |
| `page.info` | — | see below |
| `page.setViewport` | `width`, `height` (1 to 16384), `scale` (above 0, at most 8; default 1) | `{}`. For headless mode: with a window, the next resize of the window sets the viewport again. |
| `page.screenshot` | `fullPage` (default false) | `{"width", "height", "png"}`: `png` is base64. The size is in device pixels. A full-page screenshot is limited to 128 Mpx. |
| `page.scrollTo` | `x`, `y` | `{"scroll": {"x", "y"}}` (whole CSS px, clamped to the content, also on an axis with `overflow: hidden`) |
| `page.scrollBy` | `dx`, `dy` | `{"scroll": {"x", "y"}}` |

`page.info` result:

```json
{
  "url": "https://senko.net/",
  "title": "Senko's corner of the Web",
  "loadState": "complete",
  "error": null,
  "scroll": {"x": 0.0, "y": 0.0},
  "viewport": {"width": 1280.0, "height": 800.0, "scale": 1.0},
  "contentSize": {"width": 1280.0, "height": 878.6},
  "canGoBack": false,
  "canGoForward": false,
  "focusedNode": null,
  "hoveredLink": null,
  "cursor": "default"
}
```

- `loadState`: `idle`, `loadingDocument`, `loadingResources`, `complete` or
  `failed` (the page shows an error document; `error` has the reason).
- `url`: while a navigation is pending, its URL.
- `cursor`: the CSS cursor keyword for the last pointer position.

### dom

| Method | Parameters | Result |
|--------|------------|--------|
| `dom.querySelectorAll` | `selector` | `{"nodeIds": [...]}` in tree order (as `querySelectorAll`). Selectors with a pseudo-element match nothing. |
| `dom.querySelector` | `selector` | `{"nodeId": n}` or `{"nodeId": null}` |
| `dom.text` | `nodeId` | `{"text"}`: the rendered text, as `innerText` (collapsed white space, line breaks between blocks) |
| `dom.outerHtml` | `nodeId` | `{"html"}` (HTML serialization) |
| `dom.attributes` | `nodeId` | `{"attributes": {"name": "value", ...}}` |
| `dom.box` | `nodeId` | `{"rect": [x, y, w, h]}` (the union of the element's border boxes) or `{"rect": null}` |
| `dom.value` | `nodeId` | `{"value", "checked"}`: the current value of a form control (as the `value` IDL attribute; for a select, the value of its first selected option), and the checkedness of a checkbox or radio button or the selectedness of an option. `null` where it does not apply. The value of a password field is returned in plain text. |
| `dom.boxes` | — | the box dump of all elements (format: [testing.md](testing.md), "Box dump") |
| `dom.scrollInfo` | `nodeId` | the scroll state of the element (below), or `null` if it has no box |
| `dom.scrollTo` | `nodeId`, `x`, `y` | Scrolls the element to the offset, clamped to its range, as the DOM's `scrollTo` does (also with `overflow: hidden`). The element that scrolls the page (below) scrolls the page. Other elements that are not scroll containers do not scroll. Returns the scroll state; error -32000 if the element has no box. |

`:hover`, `:focus` and the other state pseudo-classes in selectors match
the current state of the page.

Scroll state. For scroll containers and the element that scrolls the
page, these are the values of the DOM properties `scrollLeft`,
`scrollTop`, `scrollWidth`, `scrollHeight`, `clientWidth` and
`clientHeight`, not rounded (scroll offsets are whole CSS px, as in
Chromium; the sizes can have fractions). For other elements they differ
from the DOM (see `scrollable` below):

```json
{
  "scroll": {"x": 0.0, "y": 40.0},
  "scrollWidth": 200.0,
  "scrollHeight": 300.0,
  "clientWidth": 200.0,
  "clientHeight": 100.0,
  "scrollable": true
}
```

- `scrollWidth`, `scrollHeight`: the size of the scrollable overflow
  rectangle, at least the scrollport. `clientWidth`, `clientHeight`: the
  padding box (scrollbars take no space).
- The element that scrolls the page (`document.scrollingElement`): the
  root element; in quirks mode the body, if it is not a scroll container
  (the root element then does not scroll). For it, `scroll` is the page's
  scroll position, `scrollWidth` and `scrollHeight` are the content size,
  and `clientWidth` and `clientHeight` are the viewport size. The body in
  quirks mode also has the viewport size as `clientWidth` and
  `clientHeight` when it is a scroll container (CSSOM View).
- `scrollable`: true for scroll containers (`overflow` `auto`, `scroll`
  or `hidden`) and the element that scrolls the page. For other elements,
  the offset is zero and both sizes are the padding box (the DOM gives
  the overflow extent as `scrollWidth` and `scrollHeight`, and 0 for
  inline elements).
- With `overflow: hidden` on the root (or the body, if the root's is
  `visible`), `dom.scrollTo`, `page.scrollTo` and `page.scrollBy` scroll
  the page as in Chromium, and `contentSize` is the size of the content;
  `input.wheel` and the keys do not scroll the page on that axis.

### input

| Method | Parameters | Result |
|--------|------------|--------|
| `input.mouseMove` | point | `{}` |
| `input.mouseDown` | point, `button`, `clickCount` (default 1), `modifiers` | `{}` |
| `input.mouseUp` | point, `button` | `{}` |
| `input.click` | point, `button`, `clickCount` (1 to 3), `modifiers` | `{}`. For `clickCount` n, presses and releases n times with click counts 1 to n. |
| `input.wheel` | `x`, `y`, `dx`, `dy` | `{"scrolled": bool}`. A mouse wheel event at the point (viewport coordinates): scrolls by (`dx`, `dy`) CSS px the innermost scroll container under the point that can scroll in that direction, else the next one in its scroll chain, else the page (positive values scroll down and right). |
| `input.key` | `key`, `modifiers` | `{"handled": bool}` |
| `input.type` | `text` | `{}`. Types the text into the focused text field or text area, as key presses would (`maxlength` applies). A line break presses Enter (a new line in a text area, implicit submission in a text field); a tab presses Tab (the focus moves to the next element, and typing goes on there). Typing stops when Enter starts a navigation or when the focus leaves editable fields. Error -32000 if no editable field has the focus. |

- A point is `x` and `y`, or `nodeId`: the center of the node's box. If
  that center is outside the viewport or hidden by a scroll container, the
  page first scrolls the node into view (every scroll container that
  contains it, then the page); if the center is still hidden (a box
  larger than its scrollport), the point is the center of the visible
  part of the box.
- `button`: `left` (default), `middle`, `right`, `back`, `forward`. Only
  `left` does something yet.
- `clickCount` 2 selects a word, 3 the text of a block.
- The page handles these keys: Tab and Shift+Tab (focus), Enter (follows
  the focused link), Ctrl+A (select all), the arrows, Page Up, Page Down,
  Space, Shift+Space, Home and End (scrolling). The scrolling keys scroll
  the first scroll container that can scroll in their direction in the
  scroll chain of the focused element (or, without one, of the node of the
  last click), else the page. Browser shortcuts of the window (Ctrl+L,
  Ctrl+C, Backspace for back) are not part of the page.
- A focused form control gets keys first: in a text field or text area,
  characters type, and Backspace, Delete, the arrows (Shift selects,
  Control moves by words), Home, End and Ctrl+A edit; Enter submits the
  form of a text field (implicit submission) and adds a line in a text
  area. Space and Enter click buttons; Space toggles checkboxes and radio
  buttons, the arrows move between the radio buttons of a group; the
  arrows, Home, End and characters select an option of a select. A click
  in a text field places the caret; a click on a label clicks its
  control. The form controls are described in
  [ADR 0013](adr/0013-forms.md).

### selection

| Method | Parameters | Result |
|--------|------------|--------|
| `selection.get` | — | `{"text"}`: the selected text as it would be copied (empty without a selection) |
| `selection.selectAll` | — | `{"changed": bool}` |
| `selection.clear` | — | `{"changed": bool}` |

### cookies

| Method | Parameters | Result |
|--------|------------|--------|
| `cookies.get` | — | `{"cookies": [...]}`: all cookies of the session, oldest first |
| `cookies.clear` | — | `{}`. Removes all cookies. |

A cookie:

```json
{
  "name": "user",
  "value": "abc",
  "domain": "news.ycombinator.com",
  "hostOnly": true,
  "path": "/",
  "secure": true,
  "httpOnly": true,
  "sameSite": "Default",
  "expires": 1767225600
}
```

- `hostOnly`: true if the cookie had no `Domain` attribute; it is then
  sent only to `domain` itself, not to its subdomains.
- `sameSite`: `Strict`, `Lax`, `None`, or `Default` (no attribute or an
  unknown value; treated as `Lax`).
- `expires`: seconds since the Unix epoch, or `null` for a session cookie.
- With `--replay`, there are no cookies: fixtures do not store
  `Set-Cookie` headers. All pages of one swb process share one cookie jar
  ([ADR 0012](adr/0012-cookies.md)).

### browser

| Method | Parameters | Result |
|--------|------------|--------|
| `browser.close` | — | `{}`, then swb exits (also with a window) |

## Clients

- Rust: `swb_automation::Client` (blocking). The integration tests in
  `crates/automation/tests/` use it with an in-process
  `swb_automation::HeadlessBrowser`.
- Python: `swbtools.automation.Browser` in `tools/`. `Browser.start()` runs
  `swb --headless --remote-port 0` and connects:

```python
from swbtools import paths
from swbtools.automation import Browser

with Browser.start(fixture=paths.fixture_dir("senko-net")) as browser:
    browser.navigate("https://senko.net/")
    browser.wait_for_load()
    link = browser.query_selector("main a")
    browser.click_node(link)
    png = browser.screenshot()
```

## Example session

```
→ {"id": 1, "method": "page.navigate", "params": {"url": "https://senko.net/"}}
← {"id": 1, "result": {}}
→ {"id": 2, "method": "page.waitForLoad"}
← {"id": 2, "result": {"loaded": true}}
→ {"id": 3, "method": "input.key", "params": {"key": "Tab"}}
← {"id": 3, "result": {"handled": true}}
→ {"id": 4, "method": "page.info"}
← {"id": 4, "result": {"focusedNode": 42, ...}}
```
