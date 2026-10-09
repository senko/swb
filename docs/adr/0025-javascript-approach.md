# ADR 0025: JavaScript: approach, sources and the on/off setting

- Status: accepted
- Date: 2026-10-09

## Context

Ground rule 4 does not allow third-party JavaScript engines, and the
ground rules reserved the decision about a third-party JavaScript parser
for the owner. ADR 0021 does not allow agents to read the source code of
browser engines; the M3 line-breaking case showed that an instruction to
read "for ideas only" does not stop an agent from porting code.

JavaScript support has three layers:

1. The language core: parser, bytecode compiler, virtual machine (VM),
   garbage collector (GC), built-in objects, regular expressions.
2. Web API bindings: DOM, events, CSSOM, timers, fetch and XHR,
   observers, storage, history, URL. The target pages set their scope.
3. Engine integration: the event loop, scripts that run while the HTML
   parser runs, incremental style and layout, layout queries from scripts.

Layers 2 and 3 follow the WHATWG and W3C specifications and black-box
measurements in Chromium, as all work since ADR 0021 does. This ADR is
mostly about layer 1.

The ECMAScript specification (ECMA-262) defines almost all behaviour as
numbered algorithm steps, and test262 is its official test suite. The
specification does not say how to build a fast and robust engine:
memory management, object representation, bytecode design, string
representation. Small engines show these choices, and much of the same
material is in published papers, books and engine documentation.

## Decision

The owner decided on 2026-10-09:

### Sources of knowledge

1. Primary: ECMA-262 (and ECMA-402 if a target needs `Intl`), test262,
   and the WHATWG and W3C specifications for the bindings.
2. Literature: books, papers, articles and engine documentation (not
   source code). For example: Crafting Interpreters (Nystrom); "The
   Implementation of Lua 5.0" (Ierusalimschy, de Figueiredo, Celes); the
   Self papers on maps and polymorphic inline caches (Chambers, Ungar,
   Hölzle); The Garbage Collection Handbook (Jones, Hosking, Moss); Russ
   Cox's articles on regular expression matching; "Ryū" (Adams); the
   internals chapter of the QuickJS documentation; Duktape's design
   notes; articles about engine design.
3. Black-box tests: Node.js and Chromium, for behaviour that the
   specification leaves to the implementation (for example the format of
   `Error.prototype.stack`, `Date.parse` of non-ISO strings).
4. Study memos (clean room). A memo answers a group of written questions
   about engine design:
   - The orchestrator writes the questions
     ([docs/js-study/README.md](../js-study/README.md)).
   - An analyst session answers one group in one memo. It compares at
     least two engines from the list below and the literature, and
     describes the options and their trade-offs. The memo contains no
     code, no pseudocode, no identifiers from the source, no opcode or
     instruction lists, no field lists of data structures, and no
     function-by-function outline. It lists the files that the analyst
     read.
   - The orchestrator reviews each memo against these rules before other
     sessions read it.
   - Sessions that design, implement or review the engine read the memos,
     never the engine source. An analyst session does not write swb code.
5. Engines that analysts may read: QuickJS and QuickJS-ng (MIT), MuJS
   (ISC), Duktape (MIT). MicroQuickJS only after its license is checked.
6. Engines that no session reads:
   - Engines written in Rust (Boa, Nova and others). In the same language,
     ideas turn into code too directly. Nova is also MPL-2.0.
   - The engines of browsers (V8, SpiderMonkey, JavaScriptCore) and
     Ladybird (LibJS, LibWeb), as in ADR 0021.

### Other decisions

- Parser: swb has its own JavaScript parser. No third-party parser.
- Libraries: general-purpose libraries at the edges of the engine are
  allowed through the ADR 0003 process. Candidates: `ryu` (shortest
  digits for number-to-string), a time-zone library for `Date` (for
  example `jiff`), and ICU4X for `Intl` if a target needs more than one
  locale.
- Test tools: test262 (BSD-3-Clause) and Node.js. A `just` recipe fetches
  test262 at a pinned commit into a git-ignored directory. Node.js runs
  only in the differential test tool. `cargo build`, `cargo test` and
  `just check` need neither, in the same way as Chromium and `compare`.
- On/off setting: one setting turns JavaScript on or off. Off is the
  default until the owner decides otherwise.
  - Off is exactly the current behaviour: the HTML parser's scripting
    flag is off (`<noscript>` content is parsed as markup and escaped
    when serialized), `@media (scripting)` matches `none`, media
    elements always show controls, and no script runs.
  - On: the scripting flag is on in all these places, and scripts run.
  - The setting is in the command line, the automation API and the
    browser window. A change applies to the next document (a reload), as
    the scripting flag is fixed for a document.
  - The fixtures and references of the existing targets stay with
    JavaScript off.
- Scripts: swb runs all scripts of a page, whatever their origin. A rule
  by origin or site would block the sites' own code: the BBC fixture
  loads 55 of its 60 scripts from `static.files.bbci.co.uk` (a different
  site than `www.bbc.com`), and Ars Technica loads its scripts from
  `cdn.arstechnica.net`. Ad blockers use lists of ad and tracker URLs
  instead. An optional blocker of this kind is a possible later target.
- User-agent string: unchanged.

### Next decision

A design session writes the architecture ADR for the language core after
the study memos exist. Its starting points, which it may change with
reasons:

- A bytecode VM without a JIT compiler. JavaScript-to-JavaScript calls do
  not recurse on the Rust stack, so generators and `async` functions can
  suspend, and deep recursion throws a `RangeError` instead of crashing.
- A heap of objects addressed by index handles with a tracing GC, in safe
  Rust (no `unsafe`, as for the whole workspace).
- Strings with UTF-16 semantics, as ECMA-262 requires.
- A backtracking regular expression engine of our own.
- Limits for time per task, heap size, call depth, parser nesting and
  regular expression steps, so that hostile scripts cannot hang or crash
  the browser.

## Consequences

- The memos are the record of what came from other engines. The engines
  that the analysts read go into docs/credits.md.
- Because the setting is off by default, the existing tests, fixtures
  and scores stay valid while the engine grows.
- test262 and Node.js results are not part of `just check`. A separate
  scores file can ratchet the test262 pass rate.
- The language core will be a large part of swb. A rough estimate is
  60,000 to 100,000 lines of Rust for ES2023 without `Intl` (swb has
  about 122,000 lines now). The bindings add to this, in proportion to
  the APIs that the targets use.

## Updates

- 2026-10-09: the MicroQuickJS license is checked (MIT); the analysts
  read it for the memos (docs/credits.md).
- 2026-10-09: ADR 0026 is the architecture of the language core. It
  sets the language level to ECMA-262 2025 with Annex B, and it does not
  use `ryu`: Rust's float formatting gives the shortest digits.
