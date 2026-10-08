# swb

An experimental web browser, written from scratch in Rust.

This is an experiment: an AI agent (Claude) designs, writes and maintains all
of the code. The project owner sets the goals and checks the results. See
[docs/ground-rules.md](docs/ground-rules.md).

swb uses general-purpose libraries (HTTP, TLS, HTML tokenizing, image
decoding, WOFF and WOFF2 decoding, font shaping, 2D rasterization,
windowing). The browser parts
(CSS, style, layout, painting, page loading, user interface) are written in
this repository. There is no JavaScript support yet.

## Status

Early development. Static pages render, with tables, floats, flexbox,
grid, absolute, fixed and sticky positioning, 2D transforms, masks,
scroll containers, `aspect-ratio`, web fonts (WOFF2, variable fonts),
inline SVG and SVG images, responsive images, video posters, CSS
counters and quirks mode; links, scrolling, back/forward, the address
bar, hover effects, keyboard focus (Tab), text selection with copy, forms
(GET and POST) and cookies work. No JavaScript yet. Four target pages
(senko.net, Hacker News, a Wikipedia article, the Ars Technica front
page) match Chromium's layout; the BBC front page is next.
See [docs/roadmap.md](docs/roadmap.md) and [docs/targets.md](docs/targets.md).

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

Keys: Ctrl+L focuses the address bar, Alt+Left/Right go back/forward
(Backspace goes back when no editable text field has the focus), F5 or Ctrl+R
reloads, Escape stops loading, Ctrl+Q or Ctrl+W quits. Tab and Shift+Tab
move the focus between links and form controls, Enter follows the focused
link, Ctrl+A selects all text and Ctrl+C copies the selection. In text
fields and the address bar, Ctrl+X cuts and Ctrl+V pastes. Mouse
back/forward buttons work.

Headless mode (for tests and scripts):

```sh
swb --headless --screenshot page.png https://senko.net/
swb --headless --dump-layout https://senko.net/
swb --headless --with-chrome --screenshot window.png https://senko.net/
swb --headless --bench 20 https://senko.net/   # time each pipeline stage
```

Remote control: `swb --headless --remote-port 0` starts a WebSocket server
on 127.0.0.1 that tests and tools use to drive the browser (also with a
window: `swb --remote-port 9222`). See [docs/automation.md](docs/automation.md).

See `swb --help` for all options, and [docs/testing.md](docs/testing.md)
for the comparison tooling.

## Documentation

- [Architecture](docs/architecture.md)
- [Automation protocol](docs/automation.md), [performance](docs/performance.md)
- [Decision records](docs/adr/)
- [Roadmap](docs/roadmap.md), [development log](docs/devlog.md)
- [Credits and sources](docs/credits.md)

## License

swb's own source code is MIT-licensed. See [LICENSE](LICENSE).

swb binaries also contain third-party material that is not under swb's
MIT License, and the MIT License does not apply to it: the Public Suffix
List under the Mozilla Public License 2.0 (MPL-2.0, a one-off exception,
[ADR 0014](docs/adr/0014-public-suffix-list-license-exception.md)), and
stylesheets based on the WHATWG HTML Standard under CC BY 4.0. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The test data in `fixtures/` (snapshots of web pages, fonts) is
third-party material under its own licenses; see
[fixtures/pages/README.md](fixtures/pages/README.md) and
[fixtures/fonts/README.md](fixtures/fonts/README.md). It is not part of
swb binaries.
