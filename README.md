# swb

An experimental web browser, written from scratch in Rust.

This is an experiment: an AI agent (Claude) designs, writes and maintains all
of the code. The project owner sets the goals and checks the results. See
[docs/ground-rules.md](docs/ground-rules.md).

swb uses general-purpose libraries (HTTP, TLS, HTML tokenizing, image
decoding, WOFF and WOFF2 decoding, font shaping, 2D rasterization,
windowing). The browser parts (CSS, style, layout, painting, page loading,
the JavaScript engine, the DOM and the other web APIs, user interface) are
written in this repository.

## Status

Early development. JavaScript is off by default.

- Rendering: tables, floats, flexbox, grid, absolute, fixed and sticky
  positioning, 2D transforms, masks, filters and `backdrop-filter`, scroll
  containers, `aspect-ratio`, web fonts (WOFF2, variable fonts), inline SVG
  and SVG images, responsive and lazy images, video posters, CSS counters,
  quirks mode, and `iframe` documents (also with JavaScript off).
- Interaction: links, scrolling, back/forward, the address bar, hover
  effects, keyboard focus (Tab), text selection with copy, forms (GET and
  POST), cookies, and `details` menus.
- JavaScript (turn it on with the toolbar's JS button, Ctrl+Shift+J or
  `--js`): a JavaScript engine of its own (`crates/js*`; 40,574 tests of
  its test262 subset pass), the DOM and many web APIs (events, scripts in
  the parser and `document.write`, the CSS object model, `fetch` and
  `XMLHttpRequest`, storage, observers, `Intl` for English), module
  scripts that import nothing, and frames, with one script runtime per
  origin and `postMessage` between them. Not done: modules that import
  other modules, workers, `window.open` and tabs, media playback.
- Six target pages match Chromium: senko.net, Hacker News, a Wikipedia
  article, a Craigslist posting, and the Ars Technica and BBC front
  pages. Hacker News, Wikipedia, Craigslist, Ars Technica and BBC also
  work with JavaScript on, with the interactions that
  [docs/targets.md](docs/targets.md) lists (menus, dialogs, consent
  banners, carousels, the map of the Craigslist posting).

See [docs/roadmap.md](docs/roadmap.md) and [docs/targets.md](docs/targets.md).

## Build and run

Requirements (Debian/Ubuntu package names):

- Rust toolchain via [rustup](https://rustup.rs/). The version is pinned in
  `rust-toolchain.toml` and installed automatically.
- `libfontconfig-dev`, `libxkbcommon-dev`, `libwayland-dev`, `pkg-config`.
- A C compiler, `make` and `sh` (`build-essential`): the build compiles
  jemalloc, the memory allocator of the binaries, from source (ADR 0029).
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
back/forward buttons work. Ctrl+Shift+J or the toolbar's JS button turns
JavaScript on or off for the next page that loads (reload to apply it to
the current one); `just run --js URL` starts with it on. JavaScript needs a
stack limit of at least 8 MiB (`ulimit -s`); below it, the button is gray
and JavaScript stays off.

Headless mode (for tests and scripts):

```sh
swb --headless --screenshot page.png https://senko.net/
swb --headless --dump-layout https://senko.net/
swb --headless --with-chrome --screenshot window.png https://senko.net/
swb --headless --bench 20 https://senko.net/   # time each pipeline stage
swb --headless --js --eval 'document.title' https://senko.net/
```

Remote control: `swb --headless --remote-port 0` starts a WebSocket server
on 127.0.0.1 that tests and tools use to drive the browser (also with a
window: `swb --remote-port 9222`). See [docs/automation.md](docs/automation.md).

See `swb --help` for all options, and [docs/testing.md](docs/testing.md)
for the comparison tooling.

## Documentation

- [Ground rules](docs/ground-rules.md), [target pages](docs/targets.md)
- [Architecture](docs/architecture.md)
- [Testing and tools](docs/testing.md), [performance](docs/performance.md)
- [Automation protocol](docs/automation.md)
- [Decision records](docs/adr/)
- [Roadmap](docs/roadmap.md), [development log](docs/devlog.md)
- [Credits and sources](docs/credits.md)

## License

swb's own source code is MIT-licensed. See [LICENSE](LICENSE).

Some of swb's source code and some of the material in swb binaries are
third-party material that keeps its own license; the MIT License does
not apply to it:

- the Public Suffix List under the Mozilla Public License 2.0 (MPL-2.0, a
  one-off exception,
  [ADR 0014](docs/adr/0014-public-suffix-list-license-exception.md));
- stylesheets based on the WHATWG HTML Standard under CC BY 4.0;
- code derived from Chromium and Skia under the BSD-3-Clause license
  (copies of swb's source code and binaries must include their license
  texts), and one file derived from Servo under the MPL-2.0;
- the WebIDL files of `crates/web/idl/`, copied from WHATWG standards
  (BSD-3-Clause) and W3C specifications (W3C Software and Document
  License);
- tables generated from the Unicode Character Database and Unicode CLDR
  (Unicode License v3).

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the details and
license texts.

The test data in `fixtures/` (snapshots of web pages, fonts) is
third-party material under its own licenses; see
[fixtures/pages/README.md](fixtures/pages/README.md) and
[fixtures/fonts/README.md](fixtures/fonts/README.md). It is not part of
swb binaries. The fixtures for JavaScript (`fixtures/js/`) are local
only and not in the repository.
