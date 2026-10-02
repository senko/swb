# Ground rules

These are the project owner's requirements. They apply to all work in this
repository. Read them before you start work.

## Roles

- The owner does not read or write code. Claude (the AI agent) writes all code,
  designs the architecture, and maintains the project over time.
- The owner cares about the end result: a usable web browser.
- The owner verifies results from time to time, usually when a target page is
  reported as done. Do not depend on manual verification otherwise.

## Requirements

1. **Technology.** Any language, framework or library is acceptable.
2. **Code quality.** The code must stay maintainable. Refactor when necessary.
   Use tests, linters and other automated quality tools.
3. **Goal.** Real websites must be usable. Official conformance suites (WPT,
   CSS tests) are optional tools, not goals.
4. **Libraries.** General-purpose libraries are allowed: URL parsing, image
   decoding, HTTP, HTML and CSS *syntax* parsing into an in-memory
   representation, and similar. Browser engines (Gecko, WebKit, Blink, Servo)
   and JavaScript engines (V8, SpiderMonkey, QuickJS, Boa, etc.) are not
   allowed. The browser itself must be written in this repository.
5. **License.** The project is MIT-licensed. All dependencies must be
   compatible. See [ADR 0003](adr/0003-dependency-policy.md).
6. **Inspiration.** It is permitted to read other code for ideas. Do not copy
   it. Respect its license. Record every source of inspiration in
   [credits.md](credits.md).
7. **Documentation.** Keep a detailed record in commit messages and in the
   repository: ADRs, plans, analysis, documentation, development log.
8. **Maintenance.** Periodically review the codebase. Remove cruft and simplify.
9. **Testing.** A reliable automated test suite is mandatory. It must run
   locally without third-party services.
10. **Fixtures.** Download target pages and their resources once and keep local
    copies. Iterate and test against the copies, not the live sites.
11. **Targets.** Keep a list of target URLs in [targets.md](targets.md). The
    owner adds targets over time. Only the given page must work, not the whole
    site.
12. **Reference comparison.** Use Playwright (Chromium) to check layout and
    behaviour automatically.
13. **Automation.** The browser must run headless and expose a remote-control
    API, so that tests can drive it (similar in idea to Playwright, not
    wire-compatible).
14. **Platform.** The primary platform is Linux, GNOME, Wayland. Keep the core
    portable so that other platforms are possible later. Platform toolkits are
    acceptable if the dependencies are easy to install.
15. **Review.** Before each commit, a sub-agent with a clean context reviews the
    changes.

## Decisions from the initial Q&A (2026-10-02)

- Initial targets: `https://senko.net/`,
  `https://en.wikipedia.org/wiki/Web_browser`, `https://news.ycombinator.com/`.
- Rust toolchain: installed with rustup. The version is pinned in
  `rust-toolchain.toml`.
- Hosting: public GitHub repository.
- Commits: the owner approves each commit manually. Make coarse commits: one
  commit for a substantial body of work (what would otherwise be one branch).
  At each commit, all tests and lints pass, and the binary is usable for a demo
  or test. Use separate branches for spikes that can fail.
- JavaScript: not in scope until a target needs it. Before adding JS, ask the
  owner whether a third-party JS *parser* (source to AST only) is acceptable.
