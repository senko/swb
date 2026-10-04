# ADR 0008: Automation protocol

- Status: accepted
- Date: 2026-10-02 (proposed), 2026-10-02 (accepted with the changes in
  "Implementation")

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

The `jsonrpc: "2.0"` member is not used. Method names are `domain.verb`.
The method list is in [automation.md](../automation.md). Methods are added
when tests need them.

Node IDs are `swb_dom::NodeId` indices. They are valid until the next
navigation.

Threading: the page lives on one thread (the GUI event loop or the
headless loop). Server threads parse requests and send them to the page
thread through a channel, with a reply channel. A request wakes the page
thread like a network completion does.

The `automation` crate contains the message types, the server, the method
implementations, a headless runner and a blocking Rust client. Integration
tests use the client against a headless browser in the same process. A
Python client lives in `tools/`.

## Implementation

Decisions made during implementation (M1):

- **WebSocket library: `tungstenite`** (MIT/Apache-2.0), without TLS. It is
  synchronous, so it fits the design without an async runtime (ADR 0004).
- **One blocking thread per connection.** The thread reads a request,
  passes it to the page thread, waits for the response and writes it.
  tungstenite's `WebSocket` cannot be read and written from two threads, so
  a connection cannot send messages while it waits for a request.
  Therefore:
- **No events yet.** The proposal had `page.navigated` and `page.loaded`
  events. Tests wait with `page.waitForLoad` instead, which the page thread
  answers when the page is loaded or the timeout expires (the request stays
  pending without blocking the page thread). Events need a writer that is
  independent of the reader; add them when a client needs them.
- **Origin check.** Any web page open in a browser on the same machine can
  open a WebSocket to 127.0.0.1. Through swb it could then read local files
  (`file:` URLs and `dom.outerHtml`). Browsers always send an `Origin`
  header in the handshake, so the server rejects handshakes with one. The
  swb clients send none.
- **`input.type` is not implemented.** swb has no text inputs yet (M2).
- **`browser.close`** ends swb (headless or with a window). Tools start
  swb as a process and need a way to end it. swb waits until the reply is
  written before it exits.
- **Limits**: 16 connections, 1 MiB per request message and frame, 10 s
  for the handshake. A `page.waitForLoad` whose client disconnected stays
  pending until its deadline.
- **Keepalive**: a connection thread cannot answer pings while it waits for
  the page thread, so the clients do not send pings.
- **Screenshots** are rendered by the page (`Page::screenshot`), not by the
  window, so they look the same in headless mode and in the GUI and do not
  include the toolbar.

Update (2026-10-04): M2 added `input.type` and `dom.value` (form controls,
ADR 0013) and `cookies.get` and `cookies.clear` (ADR 0012). With them, a
local client can read `HttpOnly` cookies and the values of password
fields; [automation.md](../automation.md) states this risk.

## Consequences

- Tools and tests can drive swb from outside the process.
- The protocol is small and owned by this project.
- Listening only on 127.0.0.1 and rejecting browser origins limits
  exposure, but any local process can connect. A token in the URL can be
  added later if needed.
- A client that pipelines requests gets the responses in order, but a slow
  request (`page.waitForLoad`) delays the following requests of the same
  connection.
