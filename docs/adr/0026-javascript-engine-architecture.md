# ADR 0026: JavaScript engine: architecture of the language core

- Status: accepted. The spike (roadmap M6, step 3) tests this design.
  If the spike shows a better choice, this ADR gets an update with the
  measurements.
- Date: 2026-10-09

## Context

ADR 0025 decided that swb gets its own JavaScript engine and gave the
starting points for its design: a bytecode VM without a JIT compiler,
script-to-script calls without recursion on the Rust stack, a heap of
index handles with a tracing garbage collector (GC) in safe Rust,
strings with UTF-16 semantics, an own backtracking regular expression
engine, and limits against hostile scripts.

This ADR is the architecture of layer 1, the language core. Its inputs
are ECMA-262, the HTML Standard, the literature, and the five study
memos in [docs/js-study/](../js-study/README.md). The session that wrote
it did not read engine source code. "Memo N.M" below refers to memo N,
section M. Section numbers (§) refer to ECMA-262 2025 unless another
specification is named.

Facts that set the scale (memo 2, the BBC fixture): the page loads 60
scripts with 3.5 MB of source, about 1.1 million tokens and about 16,000
functions. Real code nests at most 36 brackets and 10 functions deep.
Several files are one line of up to 485 KB.

Constraints from the ground rules and CLAUDE.md: no `unsafe`; content
from the network never causes a panic; the code must stay maintainable
by agents; the test suite runs without network and without Node.js.

## Decision

### 1. Language level

- ECMA-262 2025 (16th edition) with Annex B, in sloppy and strict mode.
  Features of later editions come when a target needs them.
- No ECMA-402 (`Intl`) until a target needs it. Until then the locale
  methods (`toLocaleString` and others) give the results that Chromium
  gives for `en-US`, measured.
- Not in scope: `SharedArrayBuffer` (swb runs one thread per page;
  `Atomics` works on ordinary buffers), WebAssembly, the debugger.

### 2. Crates

Four new crates. None depends on an existing swb crate.

| Crate | Responsibility | Depends on |
|-------|----------------|------------|
| `js-text` | Code units of both string widths (section 7): borrowed and owned forms, conversion from and to UTF-8, UTF-16 helpers; the Unicode data that the lexer and the regular expression engine share (identifier characters, case folding, properties). | — |
| `js-regexp` | Regular expression pattern parser with its early errors, compiler, backtracking matcher. | `js-text` |
| `js-syntax` | Lexer, parser, abstract syntax tree (AST), early errors, scope analysis. | `js-text`, `js-regexp` (to check patterns of literals) |
| `js` | Bytecode compiler, interpreter, heap and GC, values, strings, objects, built-in objects, number conversion, `Date`, realms, job queue, the embedding API. The `swb-js` shell binary. | `js-text`, `js-syntax`, `js-regexp` |

`js-text` exists because all three other crates read text of both
widths and none of them can depend on `js`. String values in the AST
(literals, cooked template strings) use its owned UTF-16 form, because
they can contain unpaired surrogates. Identifier characters (`ID_Start`,
`ID_Continue`) come from generated tables or a crate under ADR 0003;
`String.prototype.normalize` uses `unicode-normalization`, which the
workspace already has.

Reasons: each part can be tested alone, builds stay faster than with one
large crate, and CLAUDE.md allows parallel work only in crates that the
plan names as separate. The Web API bindings (layer 2) go into a later
crate with its own ADR, when the owner gives targets.

### 3. Source text and front end

**Source text.** The source is a sequence of code units in one of the
two string widths (section 7): one byte per unit when all units are
below 256, otherwise two. Scripts from the network are converted once;
`eval` and `Function` receive JavaScript strings, which can contain
unpaired surrogates. The lexer is generic over the width. All positions
are code-unit offsets (`u32`). The source of a script is kept once
(shared, reference counted, outside the GC heap) for
`Function.prototype.toString` (§20.2.3.5: a slice by the function's
start and end offsets), error positions and a later lazy compiler
(memo 2.6).

**Lexer.** The parser drives the lexer and asks for one token at a time
with the goal symbol of its context (§12: regular expression or
division, template continuation). There is no token array. This is the
only correct way to separate `/` from a regular expression literal
(memo 2.2).

**Parser.** Recursive descent for statements, precedence climbing for
binary operators. The parser builds an AST in an arena per script:
nodes in vectors, addressed by `u32` indices, freed as a whole after the
compile. Cover grammars follow the specification's model: parse an
expression, and at `=>` or `=` convert it into parameters or an
assignment pattern and apply the early errors of the pattern. This is
linear and needs no lookahead scans (memo 2.2). All early errors are
reported before the script runs (§16.1.5).

**Scope analysis** runs over the whole function nest of a script before
code generation. It assigns each binding one storage kind:

- a register in the function's frame;
- a cell, if an inner function references the binding;
- a global binding, accessed by name;
- dynamic, inside `with` bodies and in functions that contain a direct
  `eval`.

Captured bindings live in heap cells that are created when the scope of
the declaration is entered. There are no "open" upvalues that point into
a frame, so a frame can move (generators, section 5) and nothing refers
to a stack position (memo 2.4, last option). A `let` binding of a `for`
loop gets a new cell per iteration with the value copied (§14.7.4.4).

**Temporal dead zone.** An uninitialized binding holds the internal
value `Empty` (section 7). Loads of lexical bindings check it. The
compiler may leave out the check only where memo 2.4 shows it is safe
(same function, after the declaration in source order, in the declaring
block or a nested one, not in a later `case` clause, not in a function
that can be created before the initialization). The first version leaves
out the check only for a load in the same function that follows the
complete declaration in source order; `switch` must add the `case`
clause rule. Further elision is a performance task.

**Dynamic scope.** Only functions that use the dynamic features pay for
them (memo 2.5):

- A function that contains a direct `eval`, and every function around
  it, keeps a scope description (names, storage kinds, cells). All their
  bindings become cells. `eval` code is compiled against the
  description. In sloppy mode, the variables that `eval` declares go
  into a per-call variable object that dynamic lookups check first.
- `with` puts an object environment on the scope chain. Inside the body,
  each name compiles to a dynamic lookup in the object (with the
  `Symbol.unscopables` check of HasBinding, §9.1.1.2.1) that falls back
  to the static binding.
- Mapped `arguments` objects hold the parameters' cells (§10.4.4.7).

**Globals** are accessed by name through the global environment record
(§9.1.1.4): its declarative part, then the global object. In a browser
the global object is `Window`, reached through `WindowProxy`, with
accessor properties, named properties on its prototype chain and
properties that the host adds and removes. A by-name lookup handles all
of these by construction. A per-instruction cache can come later
(section 6).

**Nesting.** Each recursive parser call and each AST level charges the
shared recursion budget (section 9). Because the AST depth is bounded,
the later passes that recurse over the tree are bounded too. Long chains
of left-associative operators count as depth (memo 2.2).

**Lazy compilation** is not in the first version: every function is
compiled when its script is compiled. The front end keeps what a lazy
compiler needs: positions as offsets, the shared source, and a scope
analysis that can run on one function. Lazy compilation comes only if
measurements on the targets show the need (memo 2.3).

**Line and column numbers** are computed when an error or a stack trace
needs them, from a line-start index per script. Chromium's column unit
is measured first.

Compile-time data never lives in the GC heap. When the compiler finishes
a function, it creates the function's code object and its constants in
the heap (memo 2.1). A collection cannot run during a compile (section
8). The memory of the AST and the compile data counts against the heap
limit while it exists, because `eval` and `Function` let a script
compile large strings.

**Compile limits.** Offsets are `u32`, so a source has fewer than 2^31
code units. A function has at most 65,535 registers (the operand width,
section 4). A source or a function past these limits fails to compile
with a `SyntaxError` (or the error kind that Chromium uses, measured).

### 4. Bytecode and interpreter

**Register machine.** Each function has a frame of registers: the
parameters, the local bindings and the temporaries. The compiler
allocates temporaries in stack order while it walks the AST, so the
register count of each function is known at compile time. Reasons
(memo 1.1): a register machine executes fewer instructions than a stack
machine (Shi et al.; the Lua 5.0 paper); each instruction does fewer
bounds-checked slot accesses in safe Rust; values are 16 bytes, so
pushes and pops cost more than in engines with 8-byte values; and with
an AST, register allocation is simple. Known cost: a temporary keeps its
last value alive until it is reused or the frame returns (memo 1.1,
Duktape's notes). This is accepted; it is measured in the spike.

**Instructions** are values of a Rust enum with typed operands
(registers `u16`, constant and site indices `u32`, jump offsets `i32`).
They need no decoding. A test keeps the size of an instruction at 12
bytes or less, so an instruction has at most two register operands and
one 32-bit operand. The interpreter is a loop around a `match`; the
program counter and the frame base are local variables of the loop,
written back to the frame record before any operation that can call out
(memo 1.1).

**Verifier.** After code generation, a pass checks each function:
register operands below the register count, jump targets inside the
code, a consistent handler table. A failure is an internal error: the
script does not run and swb logs a `warn!`; it is never a panic.
Register access in the loop is still bounds-checked; a failed check is
an internal error too (section 9).

**Code objects** are heap objects. They contain the instructions, the
handler table, the line table and the constant pool. The constants are
values (strings, numbers, nested function code, regular expression
literals), so the GC traces them. Code is shared by all closures of a
function and by all realms of a runtime.

**Property sites.** A property-access instruction with a constant key
names a site: an entry in a table of the code object that holds the key
and, later, the inline cache (section 6). Two registers and the site
index fit the instruction size, and caches can come later without a
change of the instruction format.

### 5. Calls, generators, exceptions

**Calls.** A runtime has one value stack (a vector of values) and one
vector of frame records. A frame record holds handles and indices only:
the function, the code, the program counter, the base of its register
window, `this`, `new.target`. A script-to-script call pushes a frame and
continues the same loop; a return pops it (memo 1.2). A frame starts
with the declared parameters, followed by the locals and temporaries.
The callee's parameter registers start at the caller's argument
registers, so arguments are not copied. Missing arguments become
`undefined`. Arguments beyond the declared count are copied into a list
of the frame record when the function uses `arguments` or a rest
parameter (known at compile time), and dropped otherwise.

Calls that look like native calls but end in a script function also stay
in the loop: `Function.prototype.call` and `apply`, `Reflect.apply`,
bound functions, and getters and setters reached from property
instructions. The native part returns a request "call this function with
these arguments" to the loop instead of calling it (memo 1.2: deferred
calls).

**Native functions that call script code** (callbacks of `forEach`,
`sort` and `replace`, `ToPrimitive`, `toJSON`, Proxy traps, getters
reached from native code) re-enter the interpreter through Rust
recursion. Each re-entry charges the recursion budget (section 9). A
built-in whose callbacks dominate, or that needs `await`, can be written
in JavaScript and compiled with the realm (self-hosted, memo 1.2); this
is decided per built-in.

**`arguments`** is created at function entry only if the function
references it or contains a direct `eval` (§10.2.11). Rest parameters
and default values are ordinary bytecode at the function start.

**Generators and `async` functions.** At a suspension, the frame record
and its register window move into the generator object; a resume moves
them back on top of the stacks. Because captured bindings are cells,
nothing points into the moved window (section 3). The resume delivers a
value and a mode (next, throw, return) in registers; the compiler emits
a dispatch on the mode after each `yield`. The return mode jumps into
the compiled return path, which runs `finally` blocks and closes
iterators (memo 1.3). `yield*` is a bytecode loop. `await` attaches
internal reactions to a promise that resume the frame from a promise
job (§27.7.5.3); awaiting a native promise costs one job. Async
generators keep the request queue of §27.6.3.

**Exceptions.** Each code object has a handler table: ranges of
instruction positions with a handler position and a register for the
caught value. A `try` costs nothing while no exception occurs (memo 1.4,
the JVM and CPython 3.11). A throw searches the table of the current
frame, then pops frames. A native frame on the way receives the error as
the result of its call (a Rust `Result`).

**`finally`** uses a stored completion (memo 1.4, Duktape's design,
which follows §6.2.4). Entering a `finally` block sets two registers to
the pending completion: its kind (normal, throw, return, or "jump to
target k") and its value. `break`, `continue` and `return` that leave a
`try` with a `finally` set the completion and jump to the block. The
instruction at the end of the block continues the completion: fall
through, rethrow, continue the return, or jump to target k (through the
next outer `finally`, if any). Each `finally` block exists once in the
code.

**Iterator closing.** `for`-`of`, `for await` and array destructuring
are compiled with an implicit `finally`-like region that closes the
iterator on an abrupt exit (§7.4.11, §7.4.13): on a throw, errors from
`return` are ignored; in async code, the close awaits.

**Termination.** A time limit, an exhausted heap or a host request ends
the script with an uncatchable error: no handler and no `finally` block
runs, and promise code never turns it into a rejection (memo 1.5; HTML
§8.1.4.5, "killing scripts"). It reaches the host as a distinct result.
The runtime stays usable: the frames of the ended task are removed, and
objects that it left half built are ordinary objects. Suspended
generators and `async` functions stay suspended and can resume in a
later task. The host decides whether the document runs more scripts
(for example, it can stop all scripts of a document after a time-limit
termination).

### 6. Objects

**Representation.** An object is a slot in the object arena: a shape
handle, its kind, the values of its named properties, its elements, and
for host objects a class and host data.

**Shapes** (hidden classes, memo 3.1). A shape describes the ordered
named keys of an object with their attributes, and its prototype (the
prototype is part of the shape, so one shape check also proves the
prototype). Adding a property moves the object along a transition from
(shape, key, attributes) to a child shape. The transition index is weak:
the GC removes entries of dead shapes.

- Root shapes: each prototype (and `null`) has one empty root shape for
  the objects that it is the prototype of. A weak table maps the
  prototype to its root shape. `new`, `Object.create`, classes and
  literals start from the root shape of their prototype.
- Key lists: a shape refers to an ordered key list and the number of
  entries of that list that it uses. A child that extends the end of its
  parent's list shares the list; a second child of the same parent (a
  branch) copies the prefix into a new list. Small shapes are searched
  linearly; larger ones get a hash index, built on first use.
- `Object.setPrototypeOf` and `__proto__`: an object without own named
  properties moves to the root shape of the new prototype; any other
  object goes to dictionary mode.

**Dictionary mode.** An object leaves the shared shapes when a property
is deleted, when an attribute of an existing property changes, when its
prototype changes while it has properties, or when it has more named
properties than a limit (start: 128, then measure). It then has its own
unshared shape that holds its prototype and its own ordered map (entries
in insertion order with tombstones, plus a hash index). It does not
return to shared shapes. So the prototype is always in the shape.

**Key order.** Integer keys come from the elements in ascending order,
then string keys and then symbols, each in insertion order
(§10.1.11.1). The layout gives this order without a sort.

**Elements** (memo 3.2) are separate from named properties:

- A dense vector of values. A hole holds `Empty`. `length` is a `u32`
  field with a writable flag.
- A sparse ordered map from index to property (a B-tree map) when the
  vector would be too sparse (a density rule, measured), or when an
  element gets non-default attributes or an accessor. An array returns
  to the dense form only when its sparse map becomes empty (section 14).
- Memory grows with the number of stored elements, never with `length`
  or an index from the script. Shrinking `length` costs at most the
  number of stored elements.
- Each realm keeps a flag "no prototype of an array has indexed
  properties". While it is set, appends and hole reads need no
  prototype walk.

**Exotic objects** (memo 3.4). The object kind is an enum. The fast
paths handle ordinary objects and arrays; all other kinds go through one
`match` per internal method (§10.1, §10.4, §10.5). Host classes (section
12) provide functions for any of the internal methods. This covers the
WebIDL legacy platform objects, `WindowProxy` and `document.all`
(Annex B.3.6). Proxy implements all internal methods with the invariant
checks of §10.5; trap recursion charges the recursion budget.

**Inline caches** are not in the first version. The interpreter does a
direct lookup in the shape's key list, then along the prototype chain.
After the core passes its test262 targets, monomorphic caches (shape,
slot) come in the property sites of each code object (section 4), with
entries in `Cell`s. Shape handles in caches carry the generation
(section 8), so a reused shape slot cannot match an old entry
(memo 3.3). Objects in dictionary mode never match a cache entry.

**Built-in objects** (memo 3.5) are described by static tables: per
property the name, the kind (native function with its `length`,
accessor pair, constant, nested object), the attributes and the native
function. A realm keeps its intrinsics in an array indexed by an enum
(§6.1.7.4). The first version creates all built-in objects when the
realm is created. The spike measures the cost; lazy creation (memo 3.5)
comes if a target creates many realms (frames).

### 7. Values, strings, numbers

**Values** (memo 4.1) are a Rust enum of 16 bytes: undefined, null,
boolean, 32-bit integer, double, and handles to a string, symbol,
`BigInt` or object. One internal variant, `Empty`, marks uninitialized
bindings and array holes. It never reaches a script: every path out of
the VM and the object model converts or rejects it. The layout stays
behind a small API, so NaN-boxing can come later if measurements show a
need. The integer variant holds the integral results of integer
operations; `-0` and overflow give doubles. Built-in functions and the
API turn integral doubles back into integers; the interpreter's double
paths do not (memo 4.1).

**Strings** (memo 4.2) are immutable and use 16-bit code-unit
semantics:

- A flat string stores one byte per code unit when all units are below
  256 (Latin-1), otherwise two bytes. Indexing is O(1).
- A concatenation of long strings gives a rope (two children and the
  length), with a depth limit and rebalancing. An operation that needs
  the characters flattens the rope and stores the flat result in the
  same string, so other holders do not flatten it again.
- Substrings are copied. Shared slices come only if measurements show
  the need.
- The maximum length is measured in Chromium (about 2^29 code units).
- Property names are interned strings (atoms). The atom table maps
  content to a string handle and holds its entries weakly. A property
  key is an array index (`u32` below 2^32 − 1), an interned string or a
  symbol. Hash tables keyed by script strings use a hash with a random
  seed per runtime, against collision floods.
- At the boundary to the rest of swb (UTF-8): a one-byte string with
  only ASCII converts by copy. The bindings ADR decides the handling of
  unpaired surrogates.

**Numbers** (memo 4.3):

- Number to string in radix 10: the shortest round-trip digits from
  Rust's float formatting (the `{:e}` format gives the shortest digits
  and the exponent), arranged as Number::toString requires
  (§6.1.6.1.20). The `ryu` crate that ADR 0025 named is not needed.
- Radix 2 to 36, `toFixed`, `toExponential` and `toPrecision`: an own
  exact routine on multi-precision integers (vectors of limbs). A tie
  picks the larger value (§21.1.3). Rust's fixed-precision formatting
  rounds ties to even and is therefore not used. Where the
  specification leaves the digits to the implementation (fractions in
  radixes other than 10), swb matches Chromium's output, measured.
- String to number: own code checks the ECMAScript syntax
  (StringToNumber, §7.1.4.1.1; literals; `parseFloat`; JSON) and passes
  valid decimal text to Rust's float parser, which rounds correctly.
  Other radixes: own code.
- `BigInt`: an own implementation (vectors of limbs, schoolbook
  multiplication, long division after Knuth's Algorithm D) with a size
  limit that bounds the time of these quadratic algorithms.

### 8. Heap and garbage collection

**Arenas.** A runtime owns one arena per kind: objects, strings,
symbols, `BigInt`s, cells, shapes, code objects, environments for `eval`
and `with`. A slot holds its data (Rust values that own their vectors)
or is free. A handle is a slot index and a generation (32 bits each).
Freeing a slot increments its generation. Every access compares the
generation: a stale handle is an internal error that ends the script
and is logged; it is never a panic and never reads another object
(memo 4.4).

**Collector** (memo 4.4): stop-the-world mark-and-sweep, not moving, not
generational, not incremental.

- Mark bits are in bit vectors beside the arenas, so the marker reads
  slots and writes only bits.
- Marking uses an explicit work list, never recursion. Large arrays are
  marked in parts.
- The sweep drops the data of unmarked slots (Rust `Drop`), increments
  their generations and puts them on the free list.
- Weak tables (atoms, shape transitions, host wrapper maps, caches) drop
  unmarked entries after marking.
- `WeakMap` and `WeakSet` are ephemerons: a value is marked only when its
  key is marked (§9.9.2). `WeakRef` targets read in a synchronous run
  stay alive through the kept-alive list (§9.10, §9.11), which the host
  clears at each microtask checkpoint. `FinalizationRegistry` cleanup
  runs as a host task (§9.9.4.1), never during a collection.
- No finalizer runs script code. Host data is dropped in the sweep, and
  its `Drop` must not touch the JavaScript heap.

**Roots:** the value stack and the frame records, suspended frames
(through their generator objects), the realms and their intrinsics, the
job queue, the module map, persistent roots, handle scopes, and the
values that the host reports through its trace hook (section 12).

**Native code and handle scopes.** Every heap handle that the native
API returns to a native function (a property value, a call result, a new
object) is recorded in the current handle scope. The scope ends when the
native function returns. A native function that loops over many values
opens an inner scope per iteration and can pass one value out of it.
Values that a native function holds are therefore always roots, also
when it calls script code that collects (memo 4.4, options 2 and 3).
The arguments of a native call are in the value stack.

Memo 4.4 notes that a built-in that never reaches a safepoint needs no
rooting. Almost every built-in can reach one, because `ToPrimitive`,
getters and Proxy traps can run script code. So all native functions
use handle scopes, which makes the rule simple to check. The spike
measures the cost; if it is high, the static table of built-ins can mark
the few functions that never run script code (for example `Math`
functions on numbers) as exempt.

**When a collection runs.** Only at safepoints. An allocation of a
small, fixed size never collects. The safepoints are:

- between instructions in the interpreter, when the bytes allocated
  since the last collection pass the threshold;
- explicit safepoints in long native loops;
- reservations (below).

The threshold is the live size after the last collection times a
factor, with a minimum (start: factor 2, minimum 16 MiB; measure).

**Accounting and the heap limit.** Every slot and every owned buffer
counts its bytes (approximately). An allocation whose size a script
controls (`ArrayBuffer`, `String.prototype.repeat`, `padStart`, array
growth, concatenation, the AST of `eval`) first reserves its size. A
reservation is a safepoint: it collects when the request would pass the
threshold or the limit, then checks the limit again. If the request
still does not fit, or if a safepoint finds the heap over the limit
after a collection, the script ends (termination, section 5). The
reservation happens before the operation creates new handles that are
not yet rooted. Buffers grow with fallible reservation (`try_reserve`).

**Stress mode.** A runtime option collects at every safepoint. A subset
of the tests and test262 runs use it, because rooting errors appear only
when a collection runs at the wrong time.

### 9. Limits

Content from the network must never crash or hang the browser.

| Resource | Mechanism | Result |
|----------|-----------|--------|
| Time per task | A countdown at function entry, backward jumps, regular expression steps and long built-in loops; at zero, the host's time check (deadline per task). | Termination |
| Script call depth | Length of the frame vector; default 10,000, compared with Chromium. | `RangeError` |
| Value stack | Bounded number of slots. | `RangeError` |
| Rust recursion | One shared budget for all Rust recursion in the engine: native re-entry into the interpreter, parser levels and AST depth, JSON, Proxy traps, the regular expression parser. Each kind charges a weight that approximates its stack use. The budget assumes a stack of at least 8 MiB (see below); a test runs the worst cases in a debug build on a thread with that stack size. | `RangeError` (also in the parser, as in Chromium) |
| Scope analysis | At most 2^20 captured-variable entries per script and 65,535 per function (flat closures copy a capture into each function in between); at most 65,535 − 1,024 declared registers per function (1,024 stay for temporaries; the compiler checks the total). | `RangeError` (captures), `SyntaxError` (registers) |
| Heap | Byte accounting, limit set by the host. | Termination |
| String length | About 2^29 code units (measured). | `RangeError` |
| Regular expressions | Steps polled with the time check; backtrack stack counted in the heap; limits for pattern nesting, program size and capture count. | Termination or `SyntaxError` |
| `BigInt` size | A limit in bits. | `RangeError` |

Stack size: every thread that runs JavaScript must have a stack of at
least 8 MiB. Threads that swb creates for scripts (and the threads of
the JavaScript tests, because `cargo test` threads have 2 MiB) set the
size when they are created. The main thread of a process has its size
from the operating system (usually 8 MiB on Linux) and cannot be
checked in safe Rust. Today the GUI runs the engine on the main thread;
the integration ADR (layer 3) decides how scripts get a thread with a
set stack size (for example a page thread).

Engine bugs (a failed verifier check, a stale handle, an index out of
range) are internal errors: they end the script, log a `warn!` and never
panic.

### 10. Regular expressions (`js-regexp`)

Memo 5.1 is the basis:

- The parser builds a small tree for both grammars: the Unicode grammar
  (`u` and `v` flags) and the web grammar of Annex B.1.2. Analyses on
  the tree: whether an atom can match the empty string, which captures
  it contains.
- The compiler emits a backtracking program (Rust enum instructions).
  Counted quantifiers use counter registers, so the program size does
  not depend on the counts.
- The matcher is a loop with an explicit backtrack stack and an undo log
  for captures and counters; it never recurses in Rust. It follows
  §22.2.2.3.1 exactly: captures inside a quantified atom are cleared on
  each iteration, and an iteration that matches the empty string after
  the minimum fails.
- Lookbehind matches backward (§22.2.2.3.4); case-insensitive matching
  canonicalizes classes at compile time (§22.2.2.7.3); the `v` flag
  supports class set operations and strings.
- The matcher works on both string widths. Positions are code-unit
  indices, so a collection during a match changes nothing.
- The Unicode tables (case folding, properties) are generated from the
  Unicode Character Database into `js-text`, as for the line-breaking
  tables in `text`. Their Unicode version is the one that test262
  expects.
- No linear-time fallback and no memoization in the first version. The
  program keeps (instruction, position) pairs meaningful, so they can
  come later.

### 11. `Date`

The calendar arithmetic follows §21.4.1. The local time offset comes from
a time zone library that reads the system's time zone data (TZif;
candidate `jiff`, reviewed under ADR 0003 when the `Date` feature
starts). The time zone is an input of the runtime that the host sets,
so tests can fix it. Local time to UTC follows §21.4.1.26 (the offset
before a transition). `Date.parse` accepts the date time string format
of §21.4.1.32 exactly, then falls back to a token parser whose rules come
from Chromium measurements (memo 5.2).

### 12. Embedding API

- **Runtime:** the heap, the atoms, the shapes, the stacks and the job
  queue. It is not `Send`; one runtime per page thread.
- **Realm:** a global object and its intrinsics. Realms of one runtime
  share the heap (for same-origin frames).
- **Host hooks** (a trait that the host implements): enqueue a job
  (HostEnqueuePromiseJob, §9.5.5, and the other job hooks), track promise
  rejections (HostPromiseRejectionTracker, §27.2.1.9), check the time, load
  modules, schedule finalization cleanup, allow `eval` (§19.2.1.2), the
  current time, the time zone and the console.
- **Host classes:** a class identifier, a prototype per realm, a brand
  check that follows the class inheritance (an `HTMLDivElement` is an
  `Element`), functions for any internal method, host data (for example
  a `NodeId`), and a trace hook that reports the JavaScript values that
  the host data holds, so that cycles between the DOM and scripts can be
  collected (memo 5.3).
- **Persistent roots:** tokens that keep a value alive until the host
  releases them, for values that the host holds outside traced data.
- **Jobs:** the engine enqueues; the host runs the queue at its
  microtask checkpoints (HTML).
- **Modules:** the asynchronous loading of §16.2.1, where the host
  answers HostLoadImportedModule after a fetch. They come when a target
  uses them.
- **Errors:** `Error.prototype.stack` uses Chromium's format, measured.

The design of the bindings (wrappers and their identity, the lifetime of
DOM nodes, the event loop) needs its own ADR with the first target.

### 13. Tests and tools

- Unit tests in each crate; integration tests as JavaScript files with
  expected output in `crates/js/tests/`.
- `swb-js`: a shell binary in the `js` crate. It runs files, prints
  `console.log` output, has the `$262` host functions of test262, and
  options for the limits and the GC stress mode.
- test262: `just test262` fetches test262 at a pinned commit into a
  git-ignored directory and runs it with `swb-js`, with a list of
  features in scope. A scores file in the repository records the pass
  count per directory; it may only go up. Not part of `just check`.
- Differential tool: `just jsdiff FILE` runs a file in `swb-js` and in
  Node.js and compares the output. Not part of `just check`.
- Benchmarks: compile time and memory for the scripts of the target
  fixtures, and a few small programs, compared with `node --jitless`
  (V8's interpreter, as a black box). Results go into
  `docs/performance.md`.
- The hostile-page set gets JavaScript cases: deep nesting, endless
  loops, huge allocations, catastrophic regular expressions, deep
  recursion, Proxy chains, deep JSON.

### 14. Choices made during the spike (M6)

The spike made these choices where the sections above left room or
where the first plan did not work:

- Heap: the internal methods that can grow a buffer whose
  size a script controls (named slots, dense elements, the change to
  sparse elements, dictionary growth) reserve first, so they are
  safepoints. They keep their own arguments alive during the
  reservation; every other handle of the caller must be in its roots or
  a handle scope. All methods that can collect take the VM's root source
  as a parameter, so the signature shows each safepoint.
- Shapes: ordered key lists are things of their own arena (no shared
  mutable Rust references); after marking, each list is cut to the
  longest prefix that a live shape uses. A shape holds its parent
  strongly; the transition index is weak.
- Dictionary mode keeps the property values in the object's slots, as
  in shape mode; the dictionary maps keys to slots.
- Elements: a dense vector of up to 64 elements is always allowed;
  above that, at least one slot in four holds an element. An array
  whose sparse map becomes empty returns to an empty dense vector;
  otherwise it stays sparse.
- The heap limit counts every slot of each arena (free slots included;
  spare vector capacity above the length not, so that a doubling of the
  vector does not count twice) and the capacity of owned buffers, so it
  bounds the memory of the process; a sweep gives back the free slots
  at the end of an arena. If
  the live size is above 90 % of the limit after a collection, the
  script ends (termination), instead of collecting at every safepoint.
- No-GC regions (for example the end of a compile) have a depth that an
  unwinder can restore, as handle scopes can be closed down to a depth;
  a safepoint in a no-GC region still checks the limit.
- Compiler and interpreter: code is shared as an immutable
  `Rc` by the code object (the heap thing that traces the constants),
  the closures and the frames, so the loop needs no heap lookup to fetch
  instructions. Registers hold cells as an internal value variant
  (`Value::Cell`, like `Empty`). Instructions have three register
  operands, or two and a 32-bit operand (12 bytes). The result of a call
  goes into the callee's register; `this` is the register after it.
  Each function compiles separately; the code objects are created
  innermost first in one no-GC region. Left-associative operator chains
  compile without recursion; other compiler recursion charges 1,536
  bytes (expression) or 1,792 bytes (statement) of the budget, a native
  re-entry 10 KiB (measured in a debug build: 992, 1,120 and 6,688 bytes
  per level).
- The speed against `node --jitless`, before inline caches, is in
  [docs/performance.md](../performance.md). Inline caches in the
  property sites, a cache for global access and cached literal shapes
  are the next performance steps (M7 feature 13).
- Exceptions: the handler table is sorted by start (the
  outer range first for equal starts), so the last range that contains
  a position is the innermost. A `finally` block ends with an
  instruction that dispatches on the stored completion through a table
  of routes; each route continues the completion through the next outer
  `finally`. Catch code follows the `try` block, so the normal path
  pays one jump (+14 % on a two-instruction loop). The handler search
  restores the value stack to the catching frame's top, closes handle
  scopes and no-GC regions down to the loop's entry, and creates the
  error object for a `Raise` only there.
- Generators: `yield` uses two registers (the value and the resume
  mode); the `Resume` instruction after it continues on `next`, throws
  on `throw` (inside the handler ranges of the `yield`) and enters an
  inline return path on `return`.
- Time: a countdown of 10,000 steps (backward jumps, function entries,
  generator resumptions), then a check of an atomic termination
  request and of the deadline. Built-in functions and string
  concatenation charge steps in proportion to their work: one per
  element or key, one per 64 code units copied. About 5 % on a loop of
  integer additions; the hostile cases end within 3 ms of a 100 ms
  deadline. One charge per operation was not enough: a loop of
  `Object.keys` calls on a large object ran 101 s past the deadline.
- Handle scopes per element in native loops cost nothing measurable
  (`forEach` over 1M elements).

## Consequences

- The design keeps each memory risk behind a check: generations catch
  stale handles, handle scopes and the stress mode catch rooting errors,
  the verifier catches compiler errors, and the shared recursion budget
  bounds all Rust recursion.
- Costs that the spike must measure: the overhead of handle scopes in
  native loops, the generation check on each access, the instruction
  size, the interpreter speed compared with `node --jitless`, the GC
  pause for a heap of one million objects, and the compile time and peak
  memory for the BBC scripts. The spike measured them
  ([docs/js-spike-report.md](../js-spike-report.md)); none of them
  needs a change of this design. Only 3 of the 60 BBC scripts are inside
  the spike's subset, so the compile time of the real scripts comes
  with M7 feature 1.
- The register machine makes the compiler larger than a stack machine
  compiler; the AST makes this manageable.
- Eager compilation and eager built-ins keep the first version simple.
  Lazy compilation, lazy built-ins and inline caches are later work,
  each after a measurement.
- The regular expression engine is a separate crate, so it can grow in
  parallel with the runtime (CLAUDE.md allows parallel work only in
  separate crates).
