# ADR 0003: Dependency policy

- Status: accepted
- Date: 2026-10-02
- Updated: 2026-10-03: one exception, the Public Suffix List (MPL-2.0
  data), allowed by the owner in [ADR 0014](0014-public-suffix-list-license-exception.md).
  The policy itself is unchanged; the exception is not a precedent.
- Updated: 2026-10-22: the owner allowed the W3C Software and Document
  License (2023) for WebIDL data files copied from W3C specifications
  ([ADR 0028](0028-javascript-in-the-browser.md) section 5). It is
  permissive: keep the notice and mark changes. It applies to IDL data
  files only, not to code dependencies.
- Updated: 2026-11-24: an exception for builds with the `media`
  feature: FFmpeg (LGPL-2.1-or-later, linked statically) and PipeWire
  (MIT, linked dynamically), allowed by the owner on 2026-11-20. See
  "Update: media builds" below and [ADR 0047](0047-media-playback.md).

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

## Update: media builds (2026-11-24, ADR 0047)

The owner allowed two C libraries for media playback (ground rules,
2026-11-20). They are an exception for builds with the Cargo feature
`media`, which is off by default; like ADR 0014, the exception is not a
precedent for other copyleft code.

- **FFmpeg** (libavformat, libavcodec, libswscale, libswresample,
  libavutil), LGPL-2.1 or later: a pinned source release, built without
  its GPL and non-free parts, with only the demuxers and decoders that
  swb needs, linked statically. FFmpeg is not a crate: `tools/ffmpeg/`
  pins the version and checksum, `just ffmpeg` builds it, and the build
  script of `swb-media-ffi` links it.
- **PipeWire** (libpipewire-0.3), MIT: the system library, linked
  dynamically, for audio output.

What keeps the default build permissive:

- `cargo build` without `media` compiles no C code and links neither
  library; `swb-media-ffi` is then an empty crate.
- `cargo deny` checks Cargo's graph with all features: the media crates
  are swb's own (MIT), and their build dependencies `cc` and
  `pkg-config` are MIT OR Apache-2.0, so the allowed list does not
  change. The C libraries are outside that graph; a test of
  `swb-media-ffi` checks FFmpeg's license strings ("LGPL version 2.1 or
  later") and its configure line (no `--enable-gpl`, no
  `--enable-nonfree`).
- The obligations of a binary distribution of a media build (notices,
  the license text, FFmpeg's source and configure line, swb's source for
  relinking) are in ADR 0047 section 2 and `THIRD_PARTY_NOTICES.md`.

Other copyleft libraries (for example a GPL build of FFmpeg, or x264)
stay excluded.
