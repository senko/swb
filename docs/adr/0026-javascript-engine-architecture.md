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
  specification leaves the digits to the implementation (radixes other
  than 10), swb applies the radix 10 rule to the radix (section 15, M7
  feature 4), not Chromium's output.
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

### 15. Choices made during M7

- Bound functions (feature 2a): an object kind of its own. `bind`
  does not flatten a chain; a call walks the chain in a loop and
  collects the bound arguments, then calls the final target like a
  deferred call, so a deep chain costs no Rust recursion. Natives can
  end in a deferred `[[Construct]]` with an explicit `new.target`
  (`Reflect.construct`, bound constructors), as they can end in a
  deferred call.
- String objects (feature 2a): the heap answers the internal methods
  for the characters of the string without allocation; only a read
  allocates the one-character string.
- Symbols (feature 2b): the well-known symbols and the registry of
  `Symbol.for` belong to the runtime, not to a realm, and are roots.
  The registry maps the atom of the key to the symbol; entries are
  never removed, and each counts against the heap limit.
- Iterator operations for natives (feature 2b, §7.4) are methods of the
  runtime (`vm/iter.rs`). A native keeps the iterator record in its
  handle scope, opens a handle scope per step, and closes the iterator
  on its own errors; it roots a thrown value before it calls `return`
  and lets terminations pass. A native that builds a result from an
  iterator builds it in the heap while it loops, never in a Rust vector
  that a script sizes.
- A thrown value that travels as a Rust `Err` is not traced (feature
  2b, found by the stress mode). Every Rust site that holds such a
  value across a safepoint (IteratorClose now; later `for`-`of`,
  destructuring, the `Map` and `Set` constructors, promise jobs) roots
  it in the open handle scope with one helper (`hold_error`). A single
  traced VM slot for "the current exception" does not work: `return`
  can throw a second value while the first is held.
- `instanceof` (feature 2b) is one loop over InstanceofOperator and
  OrdinaryHasInstance; the default `Function.prototype[@@hasInstance]`
  is recognized by identity and not called.
- `stack` (feature 2c): every error object, made by a constructor or
  raised by the VM or a native, has an own accessor property `stack`
  with the getter and setter of its realm. The error object holds the
  captured frames (function, code, source offset, receiver, construct
  flag; at most `Error.stackTraceLimit` and never more than 200, so
  the cost of an error is bounded) and, after the first read, the
  text. The first line comes from `name` and `message` at the first
  read, as in Chromium. The frames are traced, and the heap is charged
  for them at capture. An error that the VM raises becomes an object
  before any frame is removed. Source positions are Chromium's (the
  name of a property read, the `(` of a call that does not end in an
  identifier, the operator of an assignment); the compiler finds these
  tokens by a scan of the source between two nodes. The names of
  anonymous functions assigned to members (`a.b = function () {}`)
  are inferred for traces only; `name` stays empty. Built-in functions
  are not frames. As in V8, a `new.target` that is a script function
  hides the frames up to and including its innermost call; one that is
  not on the stack leaves no frames.
- `AggregateError` (feature 2c) builds its `errors` array while it
  reads the iterator, so no Rust vector has a size that a script
  chooses; it does not close the iterator on an error (the spec does
  not), so it holds no thrown value.
- `Function.prototype.toString` of a native (feature 2c) shows the
  name that the function got at creation, not its current `name`
  property, as V8 does.
- `for`-`in` (feature 3a) follows V8 where §14.7.5.9 leaves a choice:
  the loop collects the enumerable string keys of the object and its
  prototypes when it starts (a key hides the same key of later
  prototypes, also when it is not enumerable), and each step skips a key
  that `[[HasProperty]]` no longer finds. Keys added during the loop
  are not visited; a key made non-enumerable or a changed prototype
  chain does not change the list (measured in Node.js 22). The list
  lives in an internal iterator object in a register (traced, counted
  in the heap size, reserved before it is built); the set of hidden keys
  exists only when a prototype after the first object has an enumerable
  key. A step walks the chain for its key at a time step per object: a
  key deep in a long chain makes the loop quadratic, as in V8, but the
  deadline holds.
- Template objects (feature 3a, §13.2.8.4) are cached in the code
  object by (site, realm), not in a realm table: they live as long as
  the code that can reach them. The strings of each tagged template are
  one constant of their own (one per site).
- Optional chains (feature 3a) compile to `JumpIfNullish` at each `?.`
  link, all to one exit that writes `undefined`; a call through a
  parenthesized chain (`(a?.b)()`) keeps `this`. Chromium's positions:
  a read inside a chain is at its `.`, a tagged template call at its
  `` ` ``, a getter that object spread calls at the literal (first
  property) or at the spread's operand.
- The `name` of a function with a computed key (feature 3a) is set at
  run time by an instruction that redefines the property (§10.2.9);
  stack traces show it, as in V8 (`at [desc]`, `at get [desc]`).
- Iterator records of compiled code (feature 3b) live in three
  registers: the iterator, its `next` method and a state (not done,
  done, or "a step is running"). A step is two instructions: the call of
  `next` is an ordinary frame for script code (iterating a generator does
  not recurse in Rust), and the second checks the result object (its
  getters run through re-entry). A failure of the step leaves the state
  at "a step is running", which counts as done, so that `IteratorClose`
  skips it. `GetIterator` and the close
  call `@@iterator` and `return` through re-entry.
- `for`-`of` and array patterns close their iterator in an implicit
  `finally` block of the existing machinery (one `IteratorClose`
  instruction and the routes of `break`, `continue`, `return` and the
  generator's return mode); the step of `for`-`of` is outside its handler
  range. A thrown value that the close keeps is in the block's value
  register, so no Rust code holds a thrown value while it closes an
  iterator; `hold_error` stays for the natives.
- The built-in Array Iterator (its `next` is recognized by identity) steps
  without a result object. Spread and array rest copy an array without
  holes in bulk when its iterator is that one and has not started: the
  steps run no script code, so the copy is not observable (measured: the
  spread of a 1M-element array took 103 ms with the steps, 3 ms with the
  copy; `node --jitless` 1.4 ms). A spread call builds its argument list in an
  array of its own, which the call copies onto the value stack (within
  the stack's limit, 4M values by default; below 2^27 values, V8's
  limit, also for a larger configured stack).
- Parameter lists that are not simple (feature 3b): a parameter that is
  a whole element (`a`, `a = 1`, `...a`) and not a cell keeps its
  argument's register, so such a list needs one register per element
  (60,000 parameters with defaults work; V8 allows 65,534). Its TDZ needs
  no marker: every reference with a TDZ check runs before its element,
  and the compiler makes it throw without a test. Elements with patterns
  still take a register for the argument and one per bound name.
- V8's messages for destructuring and iteration depend on the source
  (`Cannot destructure property 'a' of 'x' as it is undefined.`,
  `x is not iterable`, `.for`, `.catch`, `object null` for parameters):
  the compiler renders the source text into a constant, and the
  instruction picks the form; Chromium's positions (the first property's
  target, the initializer, the loop's target) come from the compiler.
- Classes (feature 3c): `ClassDefinitionEvaluation` compiles to one
  instruction per step in the enclosing function (`ClassHeritage`,
  `NewClass`, one definition per element). The fields of a class are
  defined by the two initializer functions that the parser makes
  (instance, and static with the static blocks); computed field keys are
  evaluated at the definition into an internal array that both functions
  read by index. Closures of methods, class constructors and initializers
  carry a box of class slots (home object, `[[Fields]]`, the key array),
  allocated with the closure; other closures have none (a closure record
  is 48 bytes, was 40). A base constructor calls its field initializer at
  its start and a derived one right after `super(...)` returns, both as
  ordinary frames, so a chain of derived constructors costs no Rust
  recursion (10,000 levels end at the frame limit). A default constructor
  has code that the compiler makes; the derived one passes its arguments
  through a rest array and a spread call, which reads the array directly
  (not observable). All parts of a class are strict mode code
  (§11.2.2), also the heritage and computed keys that sloppy code
  evaluates: the code object of sloppy code lists the instruction
  ranges of its class definitions, and the instructions whose result
  depends on strictness (global and property assignment, `delete`,
  `super` assignment) check that list, not only the code's flag.
- A derived constructor starts with `this` = `Empty` in its `this`
  binding (a register or cell, so arrows that call `super(...)` share
  it); loads of it check. Its returns jump to an epilogue after the body,
  which checks the result outside every handler range (the `TypeError` of
  `return 0` is not catchable in the constructor); that `TypeError` is
  raised after the frame is removed, so the stack trace has no line for
  it, as in V8.
- `super` properties keep the super base and `this` in a pair of
  registers; `ToPropertyKey` runs in the access, after `GetSuperBase`
  (the 2025 order). A `super` access charges the time countdown for the
  prototype chain that it walks.
- Private names (feature 3d): a class body with private names has one
  implicit binding (`%private`) that holds its private environment, an
  internal object made when the body scope is entered (§15.7.14 steps 6
  and 7): one new Private Name per name, and for a private method or
  accessor its functions. A private name is an index into it (storage
  `Private(i)` of the scope analysis), so a class takes one register
  and closures capture one cell, whatever the number of its names
  (100,000 work, as in V8; the limit of feature 1b is gone). A private
  member access is one instruction with the environment register and the
  index (`GetPrivate`, `SetPrivate`, `HasPrivate`); the field
  initializers find the environment in their class slots.
- A Private Name is a symbol with a private flag. Private fields are
  ordinary entries of the object's shape and slots under that symbol:
  instances share the shapes, the collector and the heap accounting see
  them as properties, and the accesses work on every kind of object
  (they read the shape directly, not the kind's internal methods), also
  on frozen and non-extensible objects. `[[OwnPropertyKeys]]` leaves the
  private symbols out (the only enumeration of symbol keys); the string
  key enumerations skip all symbols already. No script can hold a
  private symbol.
- Private methods and accessors are not copied into each object: the
  environment holds them, and an instance gets one entry, the brand of
  the class evaluation (a private symbol), before its fields
  (`InitializeInstanceElements`). This is not observable, because a class
  adds all its instance methods at once (§7.3.34); V8 does the same, and
  its messages show it ("Receiver must be an instance of class A",
  "Cannot initialize private methods of class A twice on the same
  object"). Static private methods and accessors belong to the class
  constructor alone, so their check is the constructor's identity.
- V8's messages name the class of an anonymous class `anonymous`, or
  `.` when an expression uses one of its static private methods or
  accessors; the compiler decides this and stores it with the names.
  Chromium's positions of private member errors differ by kind: a field
  at the name for a plain read, at the operator for `=`, at the `.` for
  compound assignments, updates, destructuring and `for`-`of` targets; a
  method or accessor always at the name; a call at the `(`.
- Dynamic environments (feature 3e): a record is an internal object
  (`vm/env.rs`) that links to the next outer one: the record of a `with`
  statement holds its binding object; the record of the vars that sloppy
  direct eval code declares in a function is declarative, its bindings
  are its own configurable properties (made at the entry of the var
  scope, or of the parameter scope for an eval in the parameters). A
  dynamic reference compiles to `FindBinding` over the analysis's count
  of records, then the record's access or the static fallback; code
  outside `with` bodies and eval reach is unchanged. An assignment finds
  its record before it evaluates its right side, as §13.15.2 and test262
  require (V8 looks again after the right side). In the same way, an
  update `f++` in a `with` body whose `@@unscopables` getter deletes
  `o.f` writes `NaN` to `o.f` (the spec); V8 leaves `o.f` undefined.
- Direct eval (feature 3e): a function with a direct `eval` keeps a scope
  description in its code (`EvalInfo`): its scopes with the cells of
  their bindings, and the names of enclosing functions that it captured
  for eval code (one analysis reference per visible name, resolved from
  the scope around the function). At a call, the VM looks up the names
  that the eval code uses (a binary search per scope; all names if the
  eval code calls `eval` itself), and the eval code captures those cells
  of the caller's frame like a closure; functions inside it capture them
  from its frame. Unknown names are global with a dynamic check of the
  caller's records. Eval code runs as a frame of the interpreter loop
  (eval recursion ends at the frame limit) and its code object is
  garbage after the frame. A caller's cell that a function of eval code
  uses is also captured by every function around it, and eval code
  whose functions call `eval` hands every name of the caller on to each
  of them, so the captures can grow with depth x names. The plan of
  these captures stops at the capture limit of a script (2^20 in all,
  65,535 in one function) and the compile ends with its `RangeError`
  (V8 has no such limit); for nested `eval`, the call checks the same
  cost before the compile and charges and reserves it. One compile
  stays below about 0.6 s.
- `EvalDeclarationInstantiation` runs in Rust between the parse and the
  run: the checks read the description (the scopes between the call and
  the variable environment, with Annex B.3.4) and the global object; the
  decisions of Annex B.3.2.3 go into the compile; the var bindings are
  created after the compile, before any script code runs (so the order is
  not observable); the eval code's prologue sets the function values.
  The heap must have room for 8 bytes per code unit before the parse; the
  exact size of the tree is checked after it.
- Completion values (feature 3e): the statements whose value is
  `UpdateEmpty(C, undefined)` set the completion register to `undefined`
  when they start (and a `catch` block at its start); a `finally` block
  saves and restores it.
- Stack traces of eval code follow V8: its script name is its origin
  (`eval at f (file:1:5), <anonymous>`, the outer origin for nested eval
  code; at most 1,024 characters), its top level and its anonymous
  functions are named `eval`.
- The direct eval check is its own instruction (`CallEval`) whose slow
  path is a function outside the interpreter loop: sharing the arm of
  `Call` made call-heavy code 3 to 8 % slower.
- The name table of the parser keeps the well-known names in one table
  per process, and its cache starts at the size that the source needs,
  so a parse of a short eval string costs no table setup (12 µs per
  small eval before, 3 µs after).
- The `Function` constructor (feature 3f, §20.2.1.1.1) builds V8's
  source `(function anonymous(P\n) {\nbody\n})` (measured: positions on
  the first line count the `(`; `toString` shows the text inside the
  parentheses) and parses it once as global code, instead of the three
  parses of the specification. The parse checks with positions what the
  separate parses would find: the parameter list must end at the `)`
  after P ("Arg string terminates parameters early" before it,
  "Unexpected end of arg string" after it), and the script must be the
  parenthesized function alone ("Single function literal required"), so
  its body ends at the final `}`. A token cannot cross between the parts
  without moving one of these ends, because the parts are separated by
  line terminators. The name `anonymous` is not bound. The compile keeps
  the code of the function, not of the script around it (garbage at
  once); the function's code owns the source for the heap limit. The text
  is built under the heap check (two bytes per unit) and parsed like eval
  code. The prototype comes from `new.target` after the parse. V8 names
  these functions `eval` in stack frames (also `new eval`,
  `Object.eval [as m]`, as for all anonymous functions of eval code) and
  `<anonymous>` in the origin of eval code they call; a redefined `name`
  shows. A parameter list stops at 65,534 elements, so a parameter text
  of millions of names fails at once.
- Mapped arguments objects (feature 3f, §10.4.4): the element of a mapped
  index holds the parameter's cell (`Value::Cell`, all parameters of such
  a function are cells since 1c), so `[[ParameterMap]]` costs no memory
  and no allocation. `[[GetOwnProperty]]` takes the cell's value, the
  fast path of `[[Set]]` writes the cell, `[[DefineOwnProperty]]` follows
  §10.4.4.2 and puts the cell back while the index stays mapped, and a
  delete of the element is the end of the mapping. Only the creation of
  the object stores cells, after the parameters became cells; an earlier
  position of a repeated name holds a plain value, so only the last one is
  mapped. A design with a separate map (a boxed list of cells) cost 17 %
  on calls that create an arguments object; this one costs nothing
  measurable (1M calls that read `arguments` 558 ms, before 564 ms;
  `node --jitless` 50 ms: the object's creation is the cost, feature 13).
- Annex B.3.2.2 (feature 3f): the vars of block functions of a script are
  global declarations of their own kind. `DeclareGlobals` creates them
  first (§16.1.7 step 12 as B.3.2.2 replaces it) when no global lexical
  declaration has the name and `CanDeclareGlobalVar` allows it, and never
  throws for them. The evaluation of such a declaration asks again
  (`BlockFunctionHoisted`, then `SetGlobal`) instead of keeping the first
  answer: no script can change it in between, except where swb has no
  `[[VarNames]]` (a later `let` of a name whose property existed before
  the hoisting is allowed). V8 differs from the specification and
  test262: it throws a `SyntaxError` for a block function whose name has
  a global `let` or `const`, and makes the var configurable.
- The interpreter loop is sensitive to its size: a new arm whose slow
  path was inlined (B.3.2.2 with a setter call) made `fib(27)` 6 % and a
  property loop 5 % slower. Rare paths stay in `#[inline(never)]`
  functions, and an instruction that may call a setter reuses an
  existing arm.
- Numbers (feature 4): the conversions are pure functions of `js-text`
  (`number/`); the runtime only adds time charges. Text to number scans
  once and keeps at most 800 significant digits (a "1" after them stands
  for any later digit that is not 0, which cannot change the rounding)
  and a clamped exponent, then calls Rust's float parser; up to 19 digits
  with a power of ten of at most 22 take Clinger's fast path. So
  `Number`, `parseFloat` and `parseInt` of 16 million digits allocate
  under a kilobyte. Each scan charges one step per 6 code units (decimal
  and hexadecimal digits take 1.3 to 1.8 ns per unit, measured), and a
  text that may be decimal 3 steps per 2 of its first 800 units: Rust's
  parser takes up to 13 µs for about 760 digits near a midpoint between
  two doubles close to the subnormals. (The first version charged one
  step per 16 units and nothing for the digits, 3.5 to 16 times less
  than the cost.)
- `toFixed`, `toExponential` and `toPrecision` with a digit count are
  exact (multi-precision integers of 32-bit limbs, one quotient digit per
  step), and charge about 5 time steps per output character (a digit
  costs up to 60 ns).
- `Number.prototype.toString(radix)` in radixes other than 10 prints the
  digit string with the fewest significant digits whose exact value
  rounds (to nearest, ties to even) to the double; of those the closest
  to the double; of two equally close the one whose digits, read as an
  integer in the radix, are even (note 2's "the one that is even"; in an
  odd radix this is not the even last digit: `(1.5).toString(3)` ends in
  …111, not …112). The digits are written out positionally, without
  an exponent ("0." and zeros before small values, zeros after large
  ones). Reason:
  §6.1.6.1.20 leaves these digits to the implementation and says that
  the algorithm should generalize the radix 10 rule (note 2); this is
  that rule with the radix in place of 10. Provenance: the first
  version followed the step structure of V8's code; the owner decided
  on 2026-10-15 to replace it with an algorithm from the literature,
  the free-format digit generation of Burger and Dybvig (PLDI 1996) in
  exact arithmetic (`u128` for the exponents where it fits, else the
  multi-precision naturals). One deviation from the paper: the first
  digit place comes from the value, not from the high end of the
  rounding range, so that the result is also the closest when the range
  holds a power of the radix (subnormals with few bits, such as
  `Number.MIN_VALUE.toString(14)`). Chromium differs often: Node.js 22
  gives other digits for 38,689 of 50,000 random bit patterns and
  17,736 of 50,000 `Math.random()` values (random radixes); in 43,068 of
  these 56,425 cases V8's string does not read back as the double (for
  example `(0.9491242655898582).toString(12)`: V8 "0.b48105a5524447",
  which reads back as 0.9491242655898583; swb "0.b48105a5524446b"), in
  10,501 it has as many digits but is farther from the double, in 2,856
  it is longer. V8 prints `Number.MIN_VALUE.toString(3)` as 3^-677,
  which reads back as 1e-323 (swb: 2 × 3^-678). A test table from a
  `BigInt` reference written from the definition by search (not by the
  algorithm) checks the output.
  Cost: `Math.random().toString(36)` about 240 ns (Node.js `--jitless`
  about 160 ns), `Number.MIN_VALUE.toString(2)` 2.3 µs (7.1 µs),
  `Number.MAX_VALUE.toString(3)` 4.4 µs (2.4 µs); the charge is 12 steps
  per eight output characters, the highest measured rate.
- `parseInt` in the radixes that are not 10 and not powers of two gives
  the exact value of the digit prefix rounded to the nearest double,
  ties to even, as in the radixes 10 and 2^k. §19.2.5 allows an
  approximated integer in these radixes; swb takes the exact one so that
  all radixes follow one rule (owner decision of 2026-10-15, provenance
  as above: written from the definition). The work is linear: leading
  zeros are skipped; more than ⌈1024 / log2(radix)⌉ significant digits
  are at least 2^1024 and give Infinity without arithmetic; fewer
  accumulate exactly in at most about 1,030 bits, several digits per
  limb step, and round once. 2^1024 − 2^970 rounds to Infinity (the tie
  goes to the even 2^1024), one less to `MAX_VALUE`. Node.js 22 differs
  for 21,285 of 100,000 random digit strings of 1 to 800 digits (all of
  them finite; 66,495 strings are Infinity in both) and for 25,697 of
  100,000 strings of 1 to 40 digits: for example
  `parseInt('312040012022424233141333', 5)` is 39177157694427720 in
  swb and 39177157694427710 in Node.js, and the value one below
  2^1024 − 2^970 is Infinity in Node.js. The charge adds one step per
  four code units (up to 700) to the scan.
- `Number.prototype.toLocaleString` has no ECMA-402: it prints what
  Chromium 148 prints for `navigator.language` en-US without arguments
  (the shortest digits rounded half away from zero to three fraction
  digits, groups of three with ",", "∞", "-0"), measured; the arguments
  are ignored. A page that asks for another locale or for options gets
  the en-US default.
- `Math`: the transcendental functions are Rust's `f64` methods (glibc's
  libm on Linux; `asinh`, `acosh` and `atanh` are formulas of Rust's
  standard library). They differ from V8's in the last bit for about 1 to
  15 % of random arguments (`atanh` most); test262 does not see this.
  `hypot` matches V8 (Kahan summation scaled by the largest value).
  `Math.random` is xorshift128+ seeded per runtime from the standard
  library's random hash keys. `Math.sumPrecise` (ECMA-262 2026) and
  `Math.f16round` are in, because Chromium 148 has them; the sum is a
  fixed-point accumulator of 2,176 bits, so every addition is exact and
  costs at most 34 limb operations.
- Ropes (feature 5a, `string/rope.rs`): a concatenation whose result has
  at most 1,024 code units (`FLAT_LIMIT`) is a flat copy; a longer one is
  a node (two children, the length, the width flag, the depth). A short
  flat piece is copied into a fresh leaf with the flat leaf at the edge of
  the other operand, one or two levels down, while the two fit in 1,024
  units, so appends, prepends and both in turn fill leaves instead of
  making a chain of tiny ones. The limit is measured: 128, 256, 512 and
  1,024 give the same speed in the append loops, but larger leaves mean
  fewer slots per append and so less retained slot memory (2M appends of
  `'ab'`: 27.6 MB live after a collection with 256, 7.3 MB with 1,024;
  2,048 is 10 % slower).
- Depth rule: at most 256 levels (`MAX_DEPTH`); a concatenation first
  rebalances an operand at the limit, in place. The rebalance (after
  Boehm, Atkinson and Plass) makes the leaves in order the leaves of a
  balanced tree, keeps leaves of at least half a chunk, and joins runs of
  shorter leaves into flat chunks of at least 1/64 of the length; the
  result has at most about 320 leaves and a depth of about 10. A flatten
  at the limit would copy the whole string each time: 100,000 appends of
  1,000 units would copy O(n²) units; with the rebalance they take 0.2 s
  (Node.js `--jitless`: 0.25 s). No collection runs during a
  concatenation; each new chunk checks the heap limit.
- Flatten: iterative over a fixed stack of `MAX_DEPTH + 1` handles (no
  Rust recursion, no allocation besides the flat text); it checks the
  heap limit before it allocates the text (exact capacity), reports its
  work for the time countdown every 64K units, and stores the flat text
  in the rope's own slot, so other holders see it and the children
  become garbage. The collector marks the children of a rope through its
  work list.
- The access API: `Heap::text(s)` gives the flat view (a rope is an
  internal error); `Runtime::flatten(s)` reserves the bytes first (a
  safepoint: the caller roots its handles) and charges the time;
  `Runtime::flatten_no_gc(s)` and `Heap::flatten(s)` do not collect, for
  paths whose callers do not root the string (`ToNumber`, keys, index
  reads, `ToObject`, `Symbol.for`); `Runtime::flat_text(s)` is flatten
  then view. `Heap::push_string`, `copy_string`, `string_lossy` and
  `string_units` copy or read a rope leaf by leaf without a flatten
  (builders, messages, `console.log`). `length` and the width flag never
  flatten. Keys and atoms flatten before interning, so atoms are flat; a
  String object's string is flattened when the object is made. Equality
  (`Heap::strings_equal`) and order (`Heap::compare_strings`) compare
  the lengths first and then walk the leaves of both strings without a
  flatten and without allocation.
- The length limit stays V8's 2^29 − 24 code units (measured: a string
  of that length concatenates, one unit more is "RangeError: Invalid
  string length"). `s += s` reaches 2^28 units with a rope of a few
  nodes and ends with that `RangeError`, as in Node.js; a read of its
  code units then ends at the heap limit if the flat text does not fit.
- Measured (release, top-level `var` loops, against `node --jitless`):
  200,000 appends of `'ab'` 1.88 s → 0.07 s (Node.js 0.03 s); 2,000,000
  appends 0.57 s (0.12 s; in a function 0.25 s; the empty loop over a
  global variable alone takes 0.33 s); 200,000 prepends 1.74 s → 0.08 s;
  1,000,000 alternating prepends and appends 19.6 s → 0.35 s (0.08 s);
  1,000,000 appends of `'abc'`, then 1,000 reads of `length` 94 s →
  0.29 s (no flatten), then 1,000,000 index reads 92 s → 0.61 s (one
  flatten); 1,000,000 single-character appends fit in a heap limit of
  3 MiB (the flat text is 1 MB).

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
