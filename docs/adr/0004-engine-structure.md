# ADR 0004: Engine structure and core libraries

- Status: accepted
- Date: 2026-10-02

## Context

We need a structure that supports a long-lived codebase: clear module
boundaries, fast incremental builds, testable parts, and the option to change
one part (for example the rasterizer) without changes to the others.

## Decision

### Workspace

A cargo workspace with one crate per pipeline stage. The crates and their
dependency rules are in [architecture.md](../architecture.md). Shared lint
settings and dependency versions are in the workspace `Cargo.toml`.

### DOM

An arena: `Vec<Node>` addressed by `NodeId(u32)`, with parent, first/last
child and previous/next sibling links. html5ever's `TreeSink` trait is
implemented on our document type, so html5ever builds our tree directly.
Element and attribute names use html5ever's interned atoms (`LocalName`,
`Namespace`), which makes name comparisons cheap.

Reasons: no `Rc<RefCell<…>>` cycles, cheap traversal, stable integer handles
for the automation API and for a future JavaScript binding layer.

### CSS

Our own tokenizer and parser that follow CSS Syntax Level 3, and our own
selector parser and matcher. See [ADR 0003](0003-dependency-policy.md) for
why we do not use Servo's crates.

### Text

- Font discovery, generic family names and fallback on Linux: fontconfig.
  Chromium on Linux also uses fontconfig, so both browsers pick the same
  fonts. This matters for reference comparisons. Tests use a fixed set of
  bundled fonts, so that results do not depend on the fonts installed on the
  machine. Other platforms get their own backend behind the same interface
  later. The exact binding is decided when the `text` crate is written.
- Shaping: `harfrust` (a Rust port of HarfBuzz). Chromium uses HarfBuzz, so
  glyph advances should match.
- Glyph outlines and font metrics: `skrifa` (from the same project as
  harfrust; both use `read-fonts`).
- Break opportunities: `unicode-linebreak` (UAX #14). Line layout itself is
  ours.

### Rasterization

CPU rasterization with `tiny-skia` into an RGBA buffer. The display list is
independent of tiny-skia, so a GPU backend can replace it later if
performance requires it. Glyphs are rasterized once into alpha masks and
cached.

### Windowing

`winit` for the window and input events, `softbuffer` to show the RGBA buffer.
Both support Wayland natively, and also X11, macOS and Windows. The browser UI
(address bar, buttons) is drawn by our own paint code, so no UI toolkit is
needed.

### Networking

`ureq` (blocking HTTP client) with `rustls` for TLS, run on worker threads.
No async runtime. A blocking client on a small thread pool is simpler and is
fast enough for a browser that loads tens of resources per page.

### Errors and logging

- Library crates use typed errors (`thiserror`). The binary uses `anyhow`.
- Logging through the `log` facade, `env_logger` in the binary.
- Content from the network must never cause a panic. Malformed input is
  handled with recovery or with a logged error.

## Consequences

- Many crates means more `Cargo.toml` files, but clear boundaries.
- CPU rasterization may be slow for large windows. We measure before we
  optimize. The display list boundary keeps a GPU backend possible.
- fontconfig is Linux-specific. The `text` crate keeps it behind a trait.
