# ADR 0003: Dependency policy

- Status: accepted
- Date: 2026-10-02
- Updated: 2026-10-03: one exception, the Public Suffix List (MPL-2.0
  data), allowed by the owner in [ADR 0014](0014-public-suffix-list-license-exception.md).
  The policy itself is unchanged; the exception is not a precedent.

## Context

The project is MIT-licensed. The owner allows general-purpose libraries but
not browser engines or JavaScript engines (see
[ground rules](../ground-rules.md), item 4). The boundary must be clear so
that each new dependency is easy to decide.

## Decision

### Licenses

Allowed licenses (enforced by `cargo deny check licenses`, see `deny.toml`):
MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause,
ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, CC0-1.0, Unlicense, BSL-1.0.

Copyleft licenses are not allowed, including weak copyleft (MPL-2.0, LGPL).
MPL-2.0 is legally usable from MIT code, but we keep the dependency tree
fully permissive so that the license situation stays simple. This excludes,
for example, Servo's `cssparser` and `selectors` crates.

Font files are data, not code. If we ever bundle fonts (for tests), they must
use the SIL Open Font License or a permissive license, and they must be listed
in [credits.md](../credits.md).

### Scope

Allowed: libraries that solve a general problem that also exists outside
browsers, or that only convert a format into an in-memory representation.

- URL parsing, HTTP, TLS, compression, character encodings.
- HTML *parsing* (tokenizer and tree builder) into our own DOM.
- Image decoding (PNG, JPEG, GIF, WebP). SVG *as an image format* (an `<img>`
  or CSS background that points to an SVG file).
- Font file parsing, text shaping, glyph outlines, Unicode algorithms
  (line breaking, bidi, segmentation).
- 2D rasterization (paths, fills, anti-aliasing).
- Windowing, input events, clipboard.
- Serialization, logging, CLI parsing, testing utilities.

Not allowed: anything that does the browser's own job.

- Browser engines and their layout or style systems (Servo `layout`,
  `style`, Blink, WebKit, Gecko, Ladybird LibWeb).
- JavaScript engines or interpreters.
- Text *layout* libraries that do CSS-style inline layout for us (for example
  `parley` or `cosmic-text` as a layout engine). Line breaking into CSS line
  boxes is the browser's job. Libraries that only give break opportunities
  (UAX #14) are allowed.
- CSS cascade, selector matching or layout libraries (`taffy`, `stretch`,
  Servo `selectors`).

### Process

For each new dependency:

1. Check the license and the scope rules above.
2. Prefer crates that are maintained, widely used and have few dependencies.
3. Add it to the workspace `[workspace.dependencies]` table, not to a single
   crate, so versions stay consistent.
4. Run `just check`. It runs `cargo deny`.
5. If the choice is not obvious, write an ADR.

## Consequences

- We write our own CSS parser and selector engine. This is a moderate amount
  of work and gives us full control.
- Some useful crates are excluded. If a case appears where excluding a crate
  costs a lot, write an ADR and ask the owner.
