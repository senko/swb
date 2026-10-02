# ADR 0002: Use Rust

- Status: accepted
- Date: 2026-10-02

## Context

A browser parses untrusted input from the network, does a lot of CPU work
(style, layout, rasterization) and must stay maintainable for a long time.
The owner does not care which language is used. The primary platform is
Linux/Wayland, but the core should be portable.

Options considered:

- **C++**: fast and the language of all major engines, but memory safety
  depends on discipline. Build and dependency management are harder.
- **Rust**: memory-safe without a garbage collector, fast, good tooling
  (cargo, clippy, rustfmt), a strong ecosystem of permissively licensed
  general-purpose libraries (HTTP, TLS, URL, HTML parsing, fonts, 2D raster,
  windowing).
- **Go, Java, C#, Python**: a garbage collector and a runtime. Python is too
  slow for layout and rasterization. The library ecosystems for fonts and
  windowing are weaker.
- **Zig**: small ecosystem and the language is not yet stable.

## Decision

Write the browser in Rust, edition 2024. Pin the toolchain version in
`rust-toolchain.toml` and update it on purpose.

Tools that run outside the browser (fixture capture, Chromium reference
comparisons) are written in Python, managed with `uv`, because Playwright has
a good Python API.

## Consequences

- Compile times are longer than in most languages. A cargo workspace with
  several crates keeps incremental builds fast.
- Tree structures with parent links are awkward with Rust ownership. We use
  arenas with integer IDs (see [ADR 0004](0004-engine-structure.md)).
- Two languages in the repository. Python is limited to `tools/` and is not
  part of the browser.
