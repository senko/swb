# ADR 0008: Automation protocol

- Status: proposed
- Date: 2026-10-02

## Context

The ground rules require that swb runs headless and can be driven
programmatically, like Playwright drives Chromium. Tests and tools need to
navigate, wait for loads, click, type, scroll, query the DOM, read layout
geometry and take screenshots, in headless mode and in the GUI.

Options:

- **Chrome DevTools Protocol (CDP) or WebDriver BiDi compatibility.** Large
  protocols with many concepts (targets, sessions, frames, runtimes) that
  swb does not have. Compatibility is not required.
- **An in-process Rust API only.** Simple, but Python tools and other
  processes cannot use it.
- **A small JSON-RPC protocol over WebSocket.** The same idea as CDP, with
  only the methods swb needs. WebSocket works from Python, from Rust tests
  and from a browser page (for a future inspector).

## Decision

swb exposes a JSON-RPC 2.0 style protocol over WebSocket:
`swb --remote-port PORT` (headless or GUI) listens on 127.0.0.1 only.

Messages:

- Request: `{"id": 1, "method": "page.navigate", "params": {"url": "..."}}`
- Response: `{"id": 1, "result": {...}}` or
  `{"id": 1, "error": {"code": -32000, "message": "..."}}`
- Event (no id): `{"method": "page.loaded", "params": {...}}`

First method set (names are `domain.verb`):

- `page.navigate {url}`, `page.reload`, `page.back`, `page.forward`,
  `page.stop`
- `page.waitForLoad {timeoutMs}` → `{loaded}`
- `page.info` → url, title, load state, scroll position, viewport size,
  content size
- `page.setViewport {width, height, scale}`
- `page.screenshot {fullPage}` → base64 PNG
- `page.scrollTo {x, y}`, `page.scrollBy {dx, dy}`
- `dom.querySelectorAll {selector}` → node IDs
- `dom.text {nodeId}`, `dom.outerHtml {nodeId}`, `dom.attributes {nodeId}`
- `dom.boxes` → the box dump (docs/testing.md); `dom.box {nodeId}`
- `input.click {x, y}` or `{nodeId}` (center of the element's first box),
  `input.mouseMove {x, y}`, `input.key {key, modifiers}`,
  `input.type {text}`
- Events: `page.navigated`, `page.loaded`.

Node IDs are `swb_dom::NodeId` indices. They are valid until the next
navigation.

Threading: the page lives on one thread (the GUI event loop or the
headless loop). The server thread parses requests and sends them to the
page thread through a channel, with a one-shot reply channel. In the GUI,
a request wakes the event loop like a network completion does.

The `automation` crate contains the message types, the server and a
blocking Rust client. Integration tests use the client against a headless
swb started in-process. A Python client lives in `tools/`.

## Consequences

- Tools and tests can drive swb from outside the process.
- The protocol is small and owned by this project; methods are added when
  tests need them.
- Listening only on 127.0.0.1 limits exposure, but any local process can
  connect. A token in the URL can be added later if needed.
