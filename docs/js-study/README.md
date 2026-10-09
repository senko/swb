# JavaScript engine study

This directory holds the study memos for swb's JavaScript engine
([ADR 0025](../adr/0025-javascript-approach.md)). An `analyst` session
answers one group of questions below in one memo. The memos describe
problems, options and trade-offs in words. They contain no code and no
pseudocode. Sessions that design, implement or review the engine read
these memos and never the source code of other engines.

## Memos

| Memo | Subject |
|------|---------|
| [01-execution.md](01-execution.md) | Instruction set, calls, generators and `async`, exceptions, limits |
| [02-front-end.md](02-front-end.md) | Parser structure, hard parts of the grammar, lazy parsing, scopes, `eval` and `with`, top level; measurements of the BBC scripts |
| [03-objects.md](03-objects.md) | Property storage and shapes, arrays, property caches, exotic objects, built-in objects and realms |
| [04-values-memory.md](04-values-memory.md) | Value layout, strings, number conversion, `BigInt`, garbage collection, roots, weak references |
| [05-regexp-host.md](05-regexp-host.md) | Regular expressions, `Date`, embedding: host objects, wrappers, job queue, modules |

The five memos were written on 2026-10-09. They compare QuickJS
535a7c2, QuickJS-ng c359cac, MicroQuickJS 6d4d7eb, MuJS aab59f2 and
Duktape 3afa016 (the 3.0 development branch and its design notes).
Literature includes engine documentation and articles (the V8 blog, the
SpiderMonkey documentation, the WebKit blog), never engine source.

ECMA-262 section numbers: memo 1 cites the 2025 edition (16th edition);
memos 2 to 5 cite the draft of 2026-10-09. In clause 27 the numbers
differ: the draft inserts the resource management sections (27.2 to
27.4), so the sections from Promise objects onward are three higher in
the draft (Promise objects are §27.2 in ES2025 and §27.5 in the draft).

## Rules for memos

- Compare at least two of the allowed engines: QuickJS or QuickJS-ng
  (MIT), MuJS (ISC), Duktape (MIT); MicroQuickJS only after its license
  is checked. Use the literature listed in ADR 0025 as well.
- No code, no pseudocode, no identifiers from the source, no opcode or
  instruction lists, no field lists of data structures, no
  function-by-function outline.
- If ECMA-262 answers a question, give the section instead of describing
  an engine.
- For each question, say what matters for an engine in safe Rust (no
  `unsafe`, objects addressed by index handles, a tracing garbage
  collector).
- End with the sources: engine versions, files read, literature.
- The orchestrator reviews each memo against these rules before other
  sessions read it.

## Questions

### Memo 1: execution (`01-execution.md`)

1. Instruction set: stack machine or register machine. How a compiler
   computes the stack depth or the register count of a function. How
   operands are encoded. What changes for an interpreter loop in safe
   Rust, which has no computed goto.
2. Calls: what a call frame holds; `arguments`, rest parameters, default
   parameter values; how JavaScript-to-JavaScript calls avoid recursion
   in the host language; how built-in functions call back into
   JavaScript (callbacks, getters, `valueOf`) and how deep that nesting
   can go.
3. Generators and `async` functions: how a suspended frame is stored and
   resumed; how `await` uses the promise job queue; async generators;
   `yield*` delegation.
4. Exceptions: how `try`, `catch` and `finally` are compiled; `return`,
   `break` and `continue` through `finally`; closing iterators on an
   abrupt exit from `for`-`of` and destructuring.
5. Limits: how engines stop a script that runs too long, detect stack
   overflow, and handle out-of-memory without crashing the host.

### Memo 2: front end and scopes (`02-front-end.md`)

1. Parser structure: a single pass from tokens to bytecode, or an
   abstract syntax tree first. Cost and benefits of each for a large
   script bundle (1 to 5 MB).
2. Hard parts of the grammar: cover grammars (arrow function parameters,
   destructuring assignment), regular expression literal versus
   division, automatic semicolon insertion, contextual keywords, early
   errors. How engines keep the parser from overflowing the host stack
   on deeply nested input.
3. Lazy parsing: skipping function bodies until the first call, and what
   must still be checked in the skipped text.
4. Scope analysis: which variables live in frame slots and which in heap
   environments; captured variables (closures); one binding per loop
   iteration for `let`; temporal dead zone checks and when the compiler
   can omit them.
5. Dynamic scope: direct `eval`, `with`, and the link between `arguments`
   and parameters in sloppy mode. How engines keep their cost inside the
   functions that use them.
6. Top level: global object properties versus global lexical
   declarations; script versus module top level; source positions for
   error messages and for `Function.prototype.toString`.

### Memo 3: objects (`03-objects.md`)

1. Property storage: shapes (hidden classes) versus a dictionary per
   object; shape transitions; when an object changes to dictionary mode;
   deletion; the property order that the specification requires.
2. Arrays: dense and sparse storage, holes, the `length` property,
   changes between storage kinds; typed arrays and `ArrayBuffer`.
3. Caches for property access on instructions, and their invalidation;
   lookups along the prototype chain.
4. Exotic objects (`Proxy`, `arguments`, `String` objects, bound
   functions, module namespaces) and how engines keep them off the fast
   path for ordinary objects.
5. Built-in objects: how built-in functions and their properties are
   defined; the setup of a realm and its intrinsics; lazy creation; the
   cost at startup.

### Memo 4: values, strings, numbers, memory (`04-values-memory.md`)

1. Value representation: tagged unions, NaN-boxing, small integers next
   to doubles; fast paths for arithmetic.
2. Strings: 8-bit and 16-bit storage, ropes, interned strings for
   property names (atoms), shared substrings, string building in loops;
   property keys that are array indices.
3. Numbers: which published algorithms engines use for number-to-string
   and string-to-number, other radixes, exact `toFixed` and
   `toPrecision`; `BigInt` arithmetic.
4. Garbage collection: reference counting with cycle collection versus
   tracing (mark-sweep, mark-compact, generations); how values that the
   host (native code) holds are kept alive; when collection may run;
   weak references (`WeakMap`, `WeakRef`, `FinalizationRegistry`) and
   ephemeron marking; heap limits. Which of these fit a heap of index
   handles in safe Rust.

### Memo 5: regular expressions, dates, embedding (`05-regexp-host.md`)

1. Regular expressions: compilation to a backtracking program versus
   other designs; lookbehind; backreferences; case-insensitive matching
   with Unicode; the `u` and `v` modes; how engines bound backtracking
   (if they do) and keep the capture semantics of the specification.
2. `Date`: the local time zone and daylight saving time; the formats
   that `Date.parse` accepts beyond ISO 8601.
3. Embedding: how a host defines native classes and wraps host objects
   (for example DOM nodes), keeps them alive, and connects the job queue
   and module loading (the host hooks of ECMA-262).
