# swb

An experimental web browser, written from scratch in Rust.

This is an experiment: an AI agent (Claude) designs, writes and maintains all
of the code. The project owner sets the goals and checks the results. See
[docs/ground-rules.md](docs/ground-rules.md).

swb uses general-purpose libraries (HTTP, TLS, HTML tokenizing, image
decoding, font shaping, 2D rasterization, windowing). The browser parts
(CSS, style, layout, painting, page loading, user interface) are written in
this repository. There is no JavaScript support yet.

## Status

Early development. Static pages render; links, scrolling, back/forward
and the address bar work. No JavaScript, no forms, no tables yet. See
[docs/roadmap.md](docs/roadmap.md) and [docs/targets.md](docs/targets.md).

## Build and run

Requirements (Debian/Ubuntu package names):

- Rust toolchain via [rustup](https://rustup.rs/). The version is pinned in
  `rust-toolchain.toml` and installed automatically.
- `libfontconfig-dev`, `libxkbcommon-dev`, `libwayland-dev`, `pkg-config`.
- [`just`](https://github.com/casey/just) (optional, for the commands below).

```sh
just run https://senko.net/        # open a window
just check                         # format, lint, test, license check
```

Keys: Ctrl+L focuses the address bar, Alt+Left/Right go back/forward,
F5 or Ctrl+R reloads, Ctrl+Q quits. Mouse back/forward buttons work.

Headless mode (for tests and scripts):

```sh
swb --headless --screenshot page.png https://senko.net/
swb --headless --dump-layout https://senko.net/
swb --headless --with-chrome --screenshot window.png https://senko.net/
```

See `swb --help` for all options, and [docs/testing.md](docs/testing.md)
for the comparison tooling.

## Documentation

- [Architecture](docs/architecture.md)
- [Decision records](docs/adr/)
- [Roadmap](docs/roadmap.md), [development log](docs/devlog.md)
- [Credits and sources](docs/credits.md)

## License

MIT. See [LICENSE](LICENSE).
