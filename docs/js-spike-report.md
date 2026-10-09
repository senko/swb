# JavaScript spike report (M6 step 3)

The spike tested the architecture of
[ADR 0026](adr/0026-javascript-engine-architecture.md) in six sessions
on the branch `js-spike`, 2026-10-09: a subset of ES2025 from the lexer
to the interpreter, with the heap, the garbage collector, exceptions,
limits, the first built-ins and the test tools.

## Result

The architecture holds. No measurement or review showed a reason to
change the design before M7. The spike made choices where the ADR left
room; they are in ADR 0026 section 14. The branch merges into `main` as
the base of M7.

## Measurements

Release build, Intel Core i5-13500, Node.js 22.11. The "Consequences"
of ADR 0026 list what the spike had to measure.

| Measure | Result |
|---------|--------|
| Lexing, the 60 BBC scripts (3.5 MB) | 121 to 125 MB/s; all 60 lex to the end |
| Parse and scope analysis, 5.2 MB generated program | 72 MB/s; 31 bytes of AST per token |
| Parse and compile, same program | 29 MB/s |
| Instruction size | 12 bytes |
| Code size | 5.5 bytes per source byte (the line table is 6.5 of 28.6 MB); 7.4 with the source text, which the code keeps as UTF-16 |
| GC pause, 1 million objects with 3 properties | 16 ms |
| Allocation of `{a, b, c}` (heap only) | 202 ns |
| Handle access with the generation check | 3.1 ns |
| Recording a handle in a handle scope | 4.2 ns; handle scopes per element in native loops: not measurable |
| Memory per small object | 169 bytes |
| Heap limit 128 MiB, three-phase stress test | 48 MiB growth of resident memory |
| Time-limit countdown | about 5 % on an integer loop; hostile cases end within 3 ms of the deadline |

Interpreter speed against `node --jitless` (V8's interpreter), best of
5 (docs/performance.md, `just jsbench`):

| Program | swb / Node |
|---------|-----------|
| `fib(27)` | 5.2x |
| Sum of 10M integers | 1.07x |
| `o.x = o.x + o.y`, 5M times | 5.1x |
| Closure counter, 1M calls | 3.2x |
| 1M small objects | 11x |
| Sum of 10M integers in `try` / `try`-`finally` | 1.14x / 0.93x |
| `forEach` over 1M elements | 9.8x |
| `for` loop over 1M elements | 7.9x |
| `join` of 1M elements | 2.0x |
| 20k appends of `'ab'` | 83x |

## Tests

- test262 (commit 2e0a567), the subset of 98 groups of `test/language/`:
  3,762 pass, 1,028 fail, 3,476 use syntax outside the subset, 1,917
  are skipped (other harness files, features outside the subset). All
  of `test/language/`: 3,814 of 23,726 pass. No panic; the GC stress
  mode gives the same counts. The reviewer ran each passing test in
  Node 22 too; the 58 differences come from V8.
- About 500 scripts with expected output from Node 22, each also in GC
  stress mode with a check for stale roots; 5,076 number-to-string
  cases. The session 5 reviewer compared 6,000 random programs (nested
  `try`, loops, `switch`, generators with `next`, `throw` and `return`)
  with Node: no difference.
- The generator test: a generator moves its frame and registers into
  the generator object at `yield` and back at resumption. Generators
  with captured variables, closures in loops and `finally` blocks give
  Node's output in GC stress mode, which confirms that frames can move
  because captured bindings are cells (ADR 0026 section 3).

## Hostile cases

| Case | Result |
|------|--------|
| Deep nesting of each kind (500 to 100,000 levels) | `RangeError` or `SyntaxError`, no crash |
| Endless loops, loops over holes, large `Object.keys`, string copies, `console.log` of large structures | Termination within 3 ms of a 100 ms deadline |
| Deep recursion | `RangeError` at 10,000 frames; catchable; the script continues |
| Native re-entry (getters, callbacks, `$262.evalScript`) | `RangeError` from the recursion budget |
| Heap exhaustion (strings, arrays, objects, closures, generators) | Termination; the runtime stays usable |
| 1M throws and catches | Memory stays flat |

## What M7 must add

The spike found these costs and limits. They are in the M7 plan:

- Performance (feature 13): `fib` is slow because each call looks up
  the global `fib` by name; property access and object literals need
  the inline caches of the property sites and cached literal shapes;
  `forEach` re-enters the interpreter for each callback (self-hosted or
  loop-driven natives remove the Rust recursion). The source text kept
  for `Function.prototype.toString` doubles the code memory of ASCII
  scripts; it can be stored with one byte per character.
- Strings (feature 5): appends copy the whole string (83x slower than
  V8 in a loop); ropes are in the plan.
- Syntax (feature 1): member and call chains stop at about 2,700 links
  in the compiler, `else if` chains at about 1,360 and nested function
  expressions at about 370 in the parser. Measure the target scripts;
  walk chains without recursion if they come near.
- Built-ins (feature 2): the main test262 failures are missing `eval`,
  `Boolean`, `isNaN`, `Object.defineProperty`, the `Number` constants,
  `Symbol` and `Math`. 57 of the 60 BBC scripts stop at syntax outside
  the subset (`for`-`in`, destructuring, spread, tagged templates,
  default parameters).

## Reviews

Each session had one review and one fix round. Review findings that
changed the code:

1. Lexer: legacy-octal messages, a hash with unclear provenance, `-->`
   at the start of the input.
2. Parser and scopes: quadratic `var` checks (14 s for a 206 KB hostile
   script), memory amplification by flat-closure captures (523 MB from
   510 KB), mapped `arguments`.
3. Heap: the limit did not bound resident memory (539 MB at a 128 MB
   limit); a heap near the limit collected at every safepoint; no-GC
   regions could switch the limit off.
4. Compiler and interpreter: ties in number-to-string, array `length`
   writes that ended the script with an internal error.
5. Exceptions and built-ins: built-ins counted as one step of the time
   limit whatever their size (101 s past a 100 ms deadline).
6. Tools: an unreadable test counted as a pass, partial runs that
   lowered the test262 scores, a recursive `$262.evalScript` that
   overflowed the Rust stack.
