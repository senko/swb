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
(memo 1.1). All instructions stay in the one `match`: in M7 13b a loop
with only about 40 frequent arms (a third of the machine code) ran 3 to
7 % slower and 3.5 % more instructions, and five alignments of
functions, loops and jump targets changed the times by at most 2 %
(section 15).

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
and the inline cache (section 6). Two registers and the site index fit
the instruction size. Since M7 13a every access has a site of its own
(the keys are shared), and so do the global accesses and the object
literals that are built in their final shape.

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

The path of a script-to-script call (M7 13b, section 15): one lookup of
the callee object gives the code, the `this` mode and the realm; the
countdown step, the frame limit and the value-stack limit are checked
together; the `this` of a sloppy function is bound in place (an object
stays, `undefined` and `null` give the realm's global object; only a
primitive takes the general `ToObject`); the stack is cut to the passed
parameters and grown with `undefined` to the register count; the record
is pushed. Constructor calls and class constructors without `new` take a
separate path. A return drops the record in place (no move of the
128-byte record) and restores the caller's stack length.

Calls that look like native calls but end in a script function also stay
in the loop: `Function.prototype.call` and `apply`, `Reflect.apply`,
bound functions, and getters and setters reached from property
instructions. The native part returns a request "call this function with
these arguments" to the loop instead of calling it (memo 1.2: deferred
calls).

**Native functions that call script code** in a loop (the callbacks of
the array and typed-array methods, `sort` with a comparator, `replace`
and `replaceAll` with a function) are native loops since M7 13c (`vm/native_loop.rs`, section 15; memo 1.2,
"resumable native functions"): the native returns its state, and the
interpreter runs the state's steps; a step's call of a script function
is a frame of the running interpreter loop whose return
(`ReturnTo::Native`) runs the next step. The state keeps its heap values
in registers, a window of the value stack. The other natives that need a
result of script code in the middle of their work (`ToPrimitive`,
`toJSON`, Proxy traps, getters reached from native code, the steps of
iterator helpers, promise jobs) re-enter the interpreter through Rust
recursion. Each
re-entry charges the recursion budget (section 9), and so does the start
of a native callee from a loop step. A built-in whose callbacks dominate,
or that needs `await`, can be written in JavaScript and compiled with the
realm (self-hosted, memo 1.2); this is decided per built-in.

**`arguments`** is created at function entry only if the function
references it or contains a direct `eval` (§10.2.11). Rest parameters
and default values are ordinary bytecode at the function start. Since
M7 13b the first arguments object of a realm (one for sloppy, one for
strict functions) is made with the general definitions and its shape and
named slots become a template of the realm; later objects are made in
that shape at once, with the elements (values or the cells of mapped
parameters) in one dense vector.

**Generators and `async` functions.** At a suspension, the frame record
and its register window move into the generator object; a resume moves
them back on top of the stacks. Because captured bindings are cells,
nothing points into the moved window (section 3). The resume delivers a
value and a mode (next, throw, return) in registers; the compiler emits
a dispatch on the mode after each `yield`. The return mode jumps into
the compiled return path, which runs `finally` blocks and closes
iterators (memo 1.3). `yield*` is a bytecode loop. A step of `for`-`of` over a generator
whose `next` is the built-in one resumes the frame directly, and its
`yield` puts the value into the loop's register without the result
object (M7 13c, section 15). `await` attaches
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
  field with a writable flag. Since M7 13d the vector has a start offset
  (`DenseVec`, `object/dense.rs`): `shift` advances it, so a queue
  drains in O(1) per element; the offset and the count of non-holes fit
  in the 8 bytes after the buffer, so an object keeps its size.
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
`match` per internal method (§10.1, §10.4, §10.5); the walks of
`[[Get]]`, `[[Set]]` and `[[HasProperty]]` test one predicate for the
kinds that need their own step (proxies and typed arrays, section 15,
feature 11b). Host classes (section
12) provide functions for any of the internal methods. This covers the
WebIDL legacy platform objects, `WindowProxy` and `document.all`
(Annex B.3.6). Proxy implements all internal methods with the invariant
checks of §10.5; trap recursion charges the recursion budget.

**Inline caches** (M7 feature 13a; details in section 15). Each property
site of a code object (section 4) has one monomorphic entry, a small
`Copy` value in a `Cell` of a side table. An entry checks the receiver's
shape handle; shape handles carry the generation (section 8), so a
reused shape slot cannot match an old entry (memo 3.3). An entry that
depends on the prototype chain also checks the heap's watch epoch,
which every change of the keys, attributes or prototype of a prototype
or a global object increments. Dictionary shapes are never stored in an
entry; proxies and host objects never match. A miss runs the direct
lookup in the shape's key list and along the prototype chain, which
gives the result and the new entry in one walk.

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
`BigInt` or object. Since M7 13b the enum has an 8-byte tag
(`#[repr(u64)]`), so every payload starts at byte 8 and a copy is two
8-byte moves; with the default layout the compiler copied a value in
five pieces (tag, a byte, two bytes, four bytes, eight bytes) in the
interpreter loop (section 15). One internal variant, `Empty`, marks uninitialized
bindings and array holes. It never reaches a script: every path out of
the VM and the object model converts or rejects it. The layout stays
behind a small API, so NaN-boxing can come later if measurements show a
need. The integer variant holds the integral results of integer
operations; `-0` and overflow give doubles. Built-in functions and the
API turn integral doubles back into integers; the interpreter's double
paths do not (memo 4.1; M7 13d measured the integer form for the
bitwise operators and rejected it, section 15).

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
- Since M7 13d the strings of one code unit below 256 are atoms made at
  their first use and kept alive by the heap (`Heap::unit_string`):
  `s[i]`, `charAt`, `at`, `split`, iteration, `fromCharCode` and every
  substring of one unit allocate nothing. A string also keeps the 32-bit
  keyed hash that `Map` and `Set` use, in the padding of its record (40
  bytes with or without it).
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
- `BigInt` (M7 10a, `js-text/src/bigint/`, no heap and no engine
  types): sign and magnitude, the magnitude in 64-bit limbs, least
  significant first, without zero limbs at the top, zero never negative,
  so the derived equality is the mathematical one. Schoolbook
  multiplication; division after Knuth's Algorithm D (TAOCP vol. 2,
  §4.3.1) with a short path for a one-limb divisor; both divide 128 by
  64 bits with a precomputed reciprocal (Möller and Granlund, 2011)
  instead of the hardware division. Exponentiation by squaring; shifts
  and the bitwise operators on the two's complement form read only the
  limbs that the result can depend on. `toString` divides repeatedly by
  the largest power of the radix that fits in a limb (radixes that are
  powers of two: bit groups); `StringToBigInt` and the literals
  accumulate as many digits per step as fit in a limb. A limit of 2^18
  bits (262,144; Chromium 2^30 − 64) bounds the time of these quadratic
  algorithms: at the limit the slowest operation, `toString` in a radix
  that is not a power of two, takes 32 ms in a release build, a product
  or a quotient 3 ms (2^19 bits would give 126 ms). A result above the
  limit is a `RangeError` before any work in proportion to its size (the
  size is computed or bounded first; a product, a power or a decimal text
  whose bound cannot decide computes at most a few bits beyond the limit
  and then checks). Every allocation is fallible (`try_reserve`). Each
  operation has a cost in work units (about 0.7 ns each, measured), which
  the engine charges to the time countdown (`bigint::cost`).

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
  runs as a host task (§9.9.4.1), never during a collection. The
  algorithm, the sweep order and the host interface are in section 15
  (feature 9b).
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
after a collection, the script ends (termination, section 5); only the
bytes of an `ArrayBuffer` (also the buffer that a typed array makes, and
the memory that a typed array method reserves outside the heap) are
a `RangeError` instead, as `CreateByteDataBlock` says, already above 90 %
of the limit (section 15, features 11a to 11c). The
reservation happens before the operation creates new handles that are
not yet rooted. Buffers grow with fallible reservation (`try_reserve`).

**Stress mode.** A runtime option collects at every safepoint. A subset
of the tests and test262 runs use it, because rooting errors appear only
when a collection runs at the wrong time. After the first 65,536
safepoints of a run, it collects at 16,384 safepoints in each doubling
of the count (M7 8c: the generated property escape tests of test262 loop
a million times, and a collection at each safepoint made the stress run
take 75 minutes; it now takes about 10). Reservations still collect each
time.

### 9. Limits

Content from the network must never crash or hang the browser.

| Resource | Mechanism | Result |
|----------|-----------|--------|
| Time per task | A countdown at function entry, backward jumps, regular expression steps and long built-in loops; at zero, the host's time check (deadline per task). | Termination |
| Script call depth | Length of the frame vector; default 10,000, compared with Chromium. | `RangeError` |
| Value stack | Bounded number of slots. | `RangeError` |
| Rust recursion | One shared budget for all Rust recursion in the engine: native re-entry into the interpreter, the start of a native callee from a native loop's step (section 15, 13c), parser levels and AST depth, JSON, Proxy traps, the regular expression parser. Each kind charges a weight that approximates its stack use. The budget assumes a stack of at least 8 MiB (see below); a test runs the worst cases in a debug build on a thread with that stack size. | `RangeError` (also in the parser, as in Chromium) |
| Scope analysis | At most 2^20 captured-variable entries per script and 65,535 per function (flat closures copy a capture into each function in between); at most 65,535 − 1,024 declared registers per function (1,024 stay for temporaries; the compiler checks the total). | `RangeError` (captures), `SyntaxError` (registers) |
| Heap | Byte accounting, limit set by the host. | Termination |
| String length | About 2^29 code units (measured). | `RangeError` |
| Regular expressions | Steps polled with the time check; backtrack stack counted in the heap; limits for pattern nesting, program size and capture count. | Termination or `SyntaxError` |
| `BigInt` size | A limit of 2^18 bits, checked before the work (section 7). | `RangeError` |
| `ArrayBuffer` and typed array length | V8's limit (2^35 − 1 bytes; `maxByteLength` 2^35 − 4096) and the heap limit, checked before the allocation; zero-fills and copies charged to the time countdown, loops of the typed array natives with a callback or a script source one step per element, loops without one in chunks of 16,384 elements (section 15). The memory outside the heap that a method reserves first (the sort, the list of `filter`, the output of the base64 methods) is a `RangeError` at the heap limit; the text of `join` and `toLocaleString` ends the script at the heap limit, as `Array.prototype.join` does. | `RangeError` |

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
`jiff`, ADR 0027). The time zone and the clock are inputs of the runtime
that the host sets, so tests can fix them. Local time to UTC follows
§21.4.1.26 (the offset before a transition). `Date.parse` accepts the
date time string format of §21.4.1.32 exactly and the output of
`toString` and `toUTCString`, then falls back to a token parser whose
rules come from Chromium measurements (memo 5.2; feature 12b).

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
  deferred call, so a deep chain of bound functions costs no Rust
  recursion (a chain that alternates bound functions and natives did
  until the tail calls of the robustness fix after feature 6). Natives can
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
  costs up to 60 ns). `toFixed` with at most 20 digits and a value below
  1e21 computes the integer of step 8 in 128-bit arithmetic since M7 13d
  (see section 15).
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
- Strings (feature 5b, `builtins/string*.rs`, `uri.rs`): the methods
  follow the specification's order of conversions and V8's messages
  (measured with `tools/probes/js-strings.json`). `repeat`, `padStart`
  and `padEnd` build ropes by doubling with `concat` (`'ab'.repeat(2 **
  28 - 12)` is a few nodes; V8 does the same), so only reading the code
  units costs memory; the length is checked before anything is built.
  `concat` is a chain of `concat`s. Substrings are flat copies, except the
  whole string and the empty string. `replace`, `replaceAll` and `split`
  dispatch only for an object argument (`GetMethod` of `@@replace`,
  `@@split`; the primitive is not looked up: test262 and Chromium 148 agree),
  as a deferred call. `replaceAll` and `split` find their matches one at
  a time, without a list of positions. `GetSubstitution` takes captures
  and named captures, for `RegExp` (feature 8b). `Builder` (`string.rs`)
  is the copying builder: it checks the maximum length before a copy, the
  heap limit at each doubling, and charges the copy. The heap check is
  for the doubled text in bytes of its width (again when the text
  becomes wide) and stops at the maximum length, so that a result too
  long for a string is V8's `RangeError` and not the heap limit
  (since 5d).
- Search: a needle of fewer than 16 units uses a scan for the first unit
  and a comparison of the rest; a longer one the prefix-function search of
  Knuth, Morris and Pratt (linear; `lastIndexOf` over the mirrored
  strings). Work is charged in points (a scan step 1, a comparison 4; 64
  points make a time step), also inside one call, so a hostile search
  ends at the deadline (`while (true) h.indexOf(n)` with `'a' * 8M` and a
  needle of 14 `a` and a `b`: 0.1 s limit, ends within 0.5 s). The
  table of the prefix function checks the heap limit first.
- The URI functions and `escape`/`unescape` work in chunks of 4,096 input
  units; before a chunk the builder checks the largest output of the
  chunk. A bad escape, a lone surrogate or invalid UTF-8 is `URIError:
  URI malformed`.
- `%StringIteratorPrototype%` is a new intrinsic and `ObjectKind::
  StringIterator` (flat string, next index); `for`-`of` calls its `next`
  through the generic path (no compiled fast path yet).
- JSON (feature 5c, `builtins/json*.rs`): no JSON operation recurses on
  the Rust stack; each runs a loop over a vector of open levels, and the
  code that it calls (`toJSON`, replacers, revivers, getters, `valueOf`)
  runs through re-entry under the recursion budget. Depth limits, from
  Chromium 148 (`tools/probes/js-json.json`, 824 cases, 806 of them a
  test): V8's
  `JSON.parse` has none (10 million nested arrays parse; so swb's parse
  has none, and the heap and time limits end an absurd text); V8's
  `JSON.stringify` has none for plain data (a million levels) but
  recurses on its native stack with a gap, a replacer, a property list or
  getters and ends at 3,256 to 5,157 levels with `RangeError: Maximum
  call stack size exceeded`; the reviver walk ends at about 3,000 levels.
  swb ends a stringify at 100,000 levels, with a gap or a replacer at
  4,000, and a reviver walk at 4,000 (the same `RangeError`), so a
  `toJSON` or replacer that makes a new object at each level ends in a
  `RangeError` that script can catch, not in the heap limit, and a
  pretty-printed structure cannot grow quadratically. The cycle check is
  the vector of open objects, plus a hash set from depth 16, so it is
  linear; the message of V8 for a cycle (`--> starting at object with
  constructor 'A'`, the path with at most three lines in the middle
  (the first two, `...`, the last), `--- property 'k' closes the circle`)
  is made only when the cycle is found. The constructor name is the name
  of the first `constructor` data property on the prototype chain above
  the object (the own property is ignored), else a string `@@toStringTag`
  if that name is `Object`; V8 uses the function that made the object
  (two measured cases differ: `Foo.prototype.constructor = null` and an
  inherited prototype that was not reset). The parser builds objects with
  `CreateDataProperty` semantics through `Heap::create_fresh_data_property`
  and `push_fresh_element` (no descriptor; a key that repeats keeps its
  first position and takes the last value), reserves the buffers from the
  size of the last object or array at the same depth (`Heap::presize`),
  and keeps a cache of the 256 last member names of a parse (the names
  are recorded in the handle scope, so the cache holds live atoms).
  Numbers of at most 19 digits with a mantissa below 2^53 and a decimal
  exponent of at most 22 are one multiplication or division (Clinger);
  the others use the exact `StringToNumber`. With a reviver the parser
  also makes the parse records (value, source range, members) that the
  `context.source` of the source text access needs; they are outside the
  heap, and the parse checks their size against the heap limit every
  1,024 records. The serializer writes into one flat buffer that checks
  V8's length limit and, at each doubling, the heap limit for the
  doubled buffer in bytes of its width (one byte per unit until a unit
  above 255 comes), at most for V8's maximum length, so that an output
  too long for a string is V8's `RangeError` and not the heap limit
  (with a heap limit below the size of the output, the heap limit ends
  it first), reads a primitive in an own data
  property of an ordinary object or array without a handle scope, and
  caches the quoted text of the last 64 keys. A `space` between 0 and 1
  gives line breaks without indentation, as in Chromium. A message built
  by the parser holds Rust text, so an unpaired surrogate in the shown
  source is U+FFFD (V8 keeps it); so is the token of a character outside
  the BMP (`JSON.parse('😀')`: V8 shows its first surrogate). A number that is not an integer
  and has at most 15 digits (a price, a coordinate) is written by a
  division check (`short_decimal`: the smallest `k` with
  `round(x * 10^k) / 10^k == x`), which gives the digits of
  `Number::toString` (60 million random numbers compared) without the
  three formatting passes of the general path; `Number.prototype.toString`
  itself still takes them (backlog). Speed against `node --jitless` (a
  1.4 MB text of 5,000 records): `JSON.parse` 5.8 ms (3.1, 1.9 times),
  `JSON.stringify` with an indent of 1 5.2 ms (3.2, 1.6 times), compact
  5.0 ms (1.9, 2.6 times). Differential checks against Chromium 148, not
  kept in the repository (a generator of mutated texts, random
  `stringify` calls and random revivers; `tools/probes/js-json.json`
  has the fixed cases): 12,000 mutated texts, 16,000 `stringify` calls
  and 12,000 `parse` calls with revivers gave no difference.
- The test262 runner multiplies its time limit by 30 in `--stress` mode:
  `decodeURI/S15.1.3.1_A2.5_T1` makes a million calls and takes 3 s
  normally, 185 s with a collection at every safepoint.
- Unicode strings (feature 5d: `js-text/src/unicode/case.rs`,
  `normalize.rs`, `collate.rs`, `js/src/builtins/string_unicode.rs`).
  The tables come from the UCD 17.0.0 (`swbtools js-unicode-tables`;
  `case_tables.rs` 37 KB, `norm_tables.rs` 258 KB of source: sorted
  ranges, runs `(first, last, step, delta)` for the case mappings and
  one pool of fully expanded decompositions, so nothing recurses).
  Chromium 148 uses Unicode 17.0: the 56 case pairs and the
  compatibility mapping of U+A7F1 that 17.0 added are in its results.
  `tools/probes/js-unicode.json` maps all 1,114,112 code points with
  `toLowerCase` and `toUpperCase` (3,037 change) and with the four forms
  of `normalize` (17,086 change): swb and Chromium have no difference,
  and `NormalizationTest.txt` of 17.0.0 (20,034 lines) has no failure.
  `crates/js/tests/builtins_unicode.rs` keeps the case results and the
  digest of the normalization results, `crates/js-text/tests/
  normalization.rs` runs a sample of the file (the whole file with
  `--ignored`).
- Case mapping is the Default Case Conversion: the unconditional
  mappings of `SpecialCasing.txt` and `Final_Sigma`; no language-
  sensitive mapping, so the `toLocale` forms ignore their argument.
  Difference from Chromium: `'I'.toLocaleLowerCase('tr')` is `'ı'` there
  and a malformed tag is a `RangeError`; swb has no `Intl`. A Latin-1
  string is converted in one pass over the bytes without a table, and the
  receiver itself comes back when nothing changes (also for `normalize`
  of a string that is in the form: the quick check of UAX #15 §9.1 with
  the No tables of the four forms and `NFC_QC=Maybe`; Latin-1 text in NFC
  is always normalized). Sixteen thousand units at a time are converted;
  between two slices the code checks V8's maximum length, reserves heap
  memory by doubling and charges the time.
- `normalize` sorts each run of non-starters by class with a stable sort
  (`O(n log n)`; a million marks take 0.07 s) and composes in one pass
  with one binary search per character; a text above V8's maximum length
  is `RangeError: Invalid string length` (checked after each slice, so
  the buffers never exceed the limit by more than one slice). Differences
  from V8 that follow from the heap limit: a result of more than the heap
  limit ends the script with the heap-limit termination before the
  length limit applies.
- The order of `localeCompare` (no `Intl`, no DUCET): the strings are
  compared in NFD in four levels, each decided only if the earlier ones
  are equal: (1) the base characters (marks left out) by simple case
  folding, white space before punctuation and symbols before digits
  before letters, the ASCII punctuation in Chromium's order, control
  characters ignored, and `æ œ ß ø đ ł ħ ı` as their base letters;
  (2) the marks at their positions (U+0300 to U+036F in Chromium's
  order); (3) lower case before upper case; (4) the code points. The
  specification's requirements hold: it is a total order on strings, and
  canonically equivalent strings compare as 0 (levels 1 to 3 and the
  NFD text of level 4 are equal), where Chromium also gives 0 for
  strings that differ in ignorable characters. Agreement with Chromium
  148 on 225 words in Latin, Greek and Cyrillic with accents, case and
  punctuation: 600 random pairs 100 %; all 25,200 pairs in Chromium's
  order 99.94 % (16 pairs, all with U+0402, which ICU sorts after
  U+0434; swb sorts by code point after folding). Not done: other
  scripts' order (by code point), digits of other scripts as numbers,
  contractions, `locales` and `options` (`Intl.Collator`). The compare
  reports its work to the time check every 32,768 code points.
  Speed against `node --jitless` (calls of 10-unit strings): `toLowerCase`
  and `toUpperCase`, half of them changing the text, 124 ns (24 ns, 5.2
  times); `normalize('NFC')` of normalized text 142 ns (43 ns, 3.3 times);
  `localeCompare` 159 ns (40 ns, 4 times).
- Element access for natives (feature 6a, `object/element_access.rs`,
  `builtins/array_access.rs`). An object has ordinary elements when its
  index properties are exactly its stored elements and the internal
  methods for index keys are the ordinary ones: every kind except String
  objects and host objects (later also proxies and typed arrays); mapped
  arguments objects qualify, because `[[GetOwnProperty]]` reads the cell.
  Natives use three levels: (1) a dense array (`DenseArray`: an `Array`
  with dense elements whose prototypes have ordinary elements and store
  none, so a hole reads `undefined` and has no setter on its chain) may be
  read and changed in its vector, where the steps cannot fail or run script
  code (the method checks extensibility and a writable `length` as its
  steps need them, and that `length` is still the value it read before the
  conversions of its arguments); (2) a chain with ordinary elements takes
  the specification's steps only at the indices where they can matter;
  (3) any other chain takes the steps for every index, one time step each.
  The check runs again after every step that can run script code
  (callbacks, getters, `ToString`), so a callback that adds elements,
  shrinks the array or puts a String object on the chain is seen at the
  next index.
- The rule for sparse loops: while the chain has ordinary elements,
  `HasProperty(O, k)` has no effect and is true exactly where an object of
  the chain stores `k`, and `DeletePropertyOrThrow(O, k)` has no effect
  where O does not own `k`. So the loops that skip absent indices
  (`forEach`, `map`, `filter`, `every`, `some`, `reduce`, `indexOf`,
  `lastIndexOf`, `slice`, `concat`, `flat`, the collection of `sort`) go
  from one stored index of the chain to the next (`Heap::search_chain`:
  the nearest index of each object of the chain, so the cost is the number
  of stored elements times the length of the chain); the loops that read
  every index with `Get` (`includes`, `join`, `toReversed`, `toSorted`,
  `toSpliced`, `with`, `Array.from` of an array-like) get a run of indices
  without an element as one gap that reads `undefined` (`includes` matches
  a gap only when it searches `undefined`; `join` adds its separators at
  once; the copies append `undefined` in chunks, so their cost and memory
  stay the length); the loops that delete visit the own indices; the moves
  (`shift`, `unshift`, `splice`, `copyWithin`) visit the `i` where the
  source has the property or the receiver owns the target; `reverse`
  visits the pairs with an element. The `find` family calls its predicate
  for every index (holes included), so it stays in proportion to the
  length. Indices from 2^32 − 1 to 2^53 − 2 are named keys; the search
  parses the named keys of an object only when the range reaches them.
  Measured in Chromium 148: V8 loops over the length in these methods
  (`a = []; a.length = 2 ** 32 - 1; a.reverse()` takes 37.5 s, `shift`,
  `unshift(1)`, `splice`, `copyWithin` and `sort` more than 60 s,
  `Array(2 ** 32 - 1).slice(0)` 40.6 s); swb takes under a millisecond.
- The dense builder: `Heap::new_array_with_capacity` (one reservation),
  `new_array_from` (`CreateArrayFromList` from a slice, one step),
  `push_element` and `extend_elements` (append to an extensible array
  without holes and with a writable `length`: no descriptor, no
  validation; a full vector doubles after a check of the heap limit, so
  appends never collect), and a fast path at the start of
  `CreateDataProperty` for an index of an ordinary object or array with
  dense elements that needs no growth. `split`, `Object.keys`, `values`,
  `entries`, `getOwnPropertyNames`, the `Array` constructor, `of`, `from`,
  `filter`, `concat` (`copy_dense` copies the vector of a dense source),
  `slice`, `splice` and the copying methods use it. `split` also keeps its
  last result (subject and separator strings by handle, at most 64
  pieces, traced by the VM) and copies it for a repeated split of the same
  strings: 100,000 splits of a constant string into 8 pieces take 20 ms
  (were 99 ms; `node --jitless` 3.8 ms, which caches too; 100 different
  strings: swb 86 ms, Node.js 19 ms).
- `sort` and `toSorted` (`array_sort.rs`): the values go into an internal
  array (no prototype, never reachable from script code), the sort orders
  a permutation of `u32` indices, and only then are the values written
  back with `Set` (or into the vector of a dense array) and the holes
  deleted. The sort is binary insertion of runs of 16 (Knuth, TAOCP vol. 3,
  §5.2.1) and a bottom-up merge of the runs that skips two runs whose
  boundary is in order (Sedgewick and Wayne, "Algorithms", 4th ed., §2.2),
  written from the textbook descriptions (not TimSort): stable, at most
  about n⌈log2 n⌉ comparisons, every index checked, a permutation whatever
  the comparator answers. Each insertion compares with the last element
  of the sorted part first, so input that is in order costs n − 1
  comparisons (found in the review: before, 8 sorted elements cost 13
  comparator calls, V8 7). The comparator is called as `cmp(later,
  earlier)`, and a negative result moves the later value first; this gives
  Chromium's results for `() => -1` (reverses) and `() => 1` (keeps the
  order) on the measured lengths. `undefined` values are counted and
  appended (no comparison sees them). The default order compares integer
  values (also integral doubles) by their decimal digits without strings,
  other primitives through their strings made once (unobservable), and
  objects with `ToString` at each comparison, as the specification does.
  Speed against `node --jitless` (100,000 integers): default order 15 ms
  (19 ms), with `(a, b) => a - b` 113 ms (23 ms; the cost is the re-entry of
  each comparator call, about 65 ns, feature 13).
- Limits of the array methods: `flat` keeps its open levels in a vector
  and ends at 9,000 levels with V8's `RangeError` (V8: 8,000 levels work,
  10,000 fail); a length above 2^32 − 1 for `ArrayCreate` is "Invalid
  array length"; `unshift`, `splice`, `toSpliced` and `concat` throw
  V8's `TypeError` "Invalid array length" above 2^53 − 1 (for `concat` V8
  does not throw for a spreadable array-like of length 2^53 − 1 followed by
  another item; swb follows the specification and test262). Also by the
  specification: `[1].concat(a)` with `a.length = 2 ** 32 - 1` throws the
  `RangeError` of the final `length` write (V8 returns an array of length
  4294967295), and `Object.freeze([1]).sort()` throws the `TypeError` of
  its write back (V8 returns the array; with two elements both throw). A
  write of an
  invalid `length` through `[[Set]]` (`a.length = -1`, `push`,
  `Reflect.set`) has Chromium 148's message "Failed to set the 'length'
  property on 'Array': Invalid array length" (Node.js 22: "Invalid array
  length"); `Object.defineProperty` keeps the short one. `toLocaleString`
  calls the elements' `toLocaleString` without arguments, as ECMA-262
  says without ECMA-402 (Chromium passes `locales` and `options`).
- Collections near the heap limit (found in 6a): a safepoint also
  collects when the heap is within a fifth of the limit and a tenth of the
  limit was allocated since the last collection. Paths that allocate
  without a safepoint (a concatenation that rebalances a rope checks the
  limit but cannot collect) ended a rope loop with 3 MB of live data at a
  limit of 13 MiB, because with a limit below the minimum threshold
  nothing collected before the check failed.
- `Map` and `Set` (feature 6b, `object/table.rs`, `builtins/map.rs`,
  `set.rs`, `set_methods.rs`, `collection.rs`): one table type for both
  (`ObjectKind::Map` and `Set` hold a `Box<Table>`; a `Set` stores its
  elements as keys and `undefined` as values). The table follows the
  specification's list: `entries` is a vector in insertion order, a delete
  sets the key to `Empty` and leaves the entry, `clear` empties the table.
  `index` is open addressing with linear probing over entry positions
  (`u32`; its length is a power of two and at least twice the number of
  entries, deleted ones included; a free slot is `u32::MAX`). An entry
  stores its 64-bit hash, which is compared before the key. Lookup,
  insert and delete are O(1) on average; the index and the vector double
  together, with a check of the heap limit before and a charge after (a
  table never collects while it grows, like the dense builder). A delete
  takes the entry out of the index with the deletion algorithm for linear
  probing (Knuth, TAOCP vol. 3, §6.4, Algorithm R: later entries of the
  run move back into the free slot), so the index holds only live
  entries. Found in the review: when deleted entries stayed in the index,
  a key deleted and added again while unfinished iterators blocked the
  compaction made its probe longer at each step (`it = m.keys();
  m.delete(i % 10); m.set(i % 10, 1)`: 3 million steps took more than 2
  minutes, now 2.1 s; `node --jitless` 0.23 s).
- The key and its hash. A caller prepares the key
  (`Runtime::collection_key`): `-0` becomes `+0`, a double with an integer
  value takes the integer form, a rope is flattened (a safepoint; the key
  is rooted by the arguments or the handle scope, and the time is charged
  per 64 units). `SameValueZero` then compares numbers by value (all NaNs
  are equal), strings by content (both flat), symbols, objects and
  `BigInt`s by handle (objects do not move) or limbs. The hash of a
  number is the keyed hash of the bits of its `f64` value, of a symbol or
  object the keyed hash of its handle (`swb_js_text::hash`, a key per
  process), of a string the keyed `SipHash` of its code units
  (`AtomTable::hash`, a key per heap; both widths agree), so content from
  the network cannot choose collisions. `Heap::key_hash` is the function
  that `WeakMap` and `WeakSet` (feature 9) reuse. Measured, a lookup costs
  about 30 ns for an integer key and about 100 ns for a string key. Since
  M7 13d a string key keeps a 32-bit part of its hash (`JsString::
  key_hash`, in padding), so a key used again is not hashed again (the
  per-process key stays).
- Compaction. When at least half of the entries are deleted, and at least
  as many as the iterators and native loops that hold positions, the
  vector is rebuilt in place without them and the index for the live
  entries (a delete does it when the table has 32 entries or more; an
  insert does it instead of growing when the index is full). So the
  memory is in proportion to the live entries and the iterators (200,000
  inserts and deletes in a table with 10 live entries use under 8 KiB),
  and the work of a compaction (entries plus positions) is paid by the
  deletes that caused it. A table of 100,000 entries with 99,990 deleted
  gives its memory back (a test checks a factor of 20).
- Iteration. A position is an index into `entries`, kept in the table:
  an unfinished Map or Set Iterator has a *cursor* in the table's list
  (`ObjectKind::MapIterator` holds the table, the slot of its cursor and
  the kind), and a native loop (`forEach`, the loops of the set methods)
  pushes a position on the table's stack of loops. A compaction maps each
  position to the number of live entries before it (a transient vector of
  `u32` when there are positions). `clear` frees the vectors, bumps the
  table's epoch and sets the loop positions to 0; a cursor from an older
  epoch is at position 0 when it is next used, so `clear` costs nothing
  per iterator. An iterator sees the entries added after it started and
  skips the deleted ones; once it has returned `done` it stays done, and
  its cursor is gone (the last cursor takes the slot, and its iterator's
  slot is updated). Each entry looked at is a step of the time countdown.
- The cursor list holds the handles of the iterators without tracing
  them: the iterator points to its table, not the other way round, so an
  abandoned iterator is garbage. A handle whose slot the collector freed
  is stale (generation) and the list drops it at the next prune. The list
  is pruned when an iterator is created and the list has doubled since
  the last prune or a collection has run since; the prune keeps the live
  cursors in order and gives their iterators the new slots. A delete or
  an insert that finds its compaction blocked by the length of the list
  prunes it too (at most once per collection). So the list
  has at most the live iterators plus those made since the last
  collection. A million iterators made and dropped in a loop run under a
  heap limit of 24 MiB; the unit test counts the cursors after 200,000
  dropped iterators. Until that collection the compaction waits, so the
  deleted entries pile up; `head` counts the deleted entries at the start
  of the vector, and every scan starts there. Found in the review: without
  it, the LRU idiom (`m.set(i, i); if (m.size > 10)
  m.delete(m.keys().next().value)`) scanned the whole deleted prefix with
  each new iterator (400,000 steps: 13.7 s; now the three forms of the
  idiom, 400,000 steps each, take 0.9 s together, `node --jitless` 0.15
  s). Deleted entries further on hold a skip: the entry's hash field (no
  longer needed once the entry is out of the index) holds a later
  position up to which all entries are deleted, and each scan shortens
  the path it walks (path splitting, as in union-find; Tarjan and van
  Leeuwen, 1984). Entries do not become live again before a compaction, so a skip
  stays valid while the vector grows. Found in the review of the fix: a
  full iteration per step over a table whose deleted entries wait in the
  middle walked all of them (`m.keys(); m.set(i, 1); m.delete(i); for
  (const k of m) {}`: 400,000 steps 14 s, now 0.43 s; `node --jitless`
  0.21 s for 1.6 million).
- The set methods (`set_methods.rs`) follow §24.2.4 branch by branch:
  `GetSetRecord` reads `size`, `has` and `keys` in that order with V8's
  messages (`The .size property is NaN`, `'-1' is an invalid size`,
  `string "has" is not a function`); a receiver with at most `size`
  elements calls `has` for its own elements (`intersection` iterates the
  smaller), otherwise they iterate `keys()`. `GetIteratorFromMethod`
  checks at once that `next` is callable (V8 does; the specification's
  first step would throw the same `TypeError`). `union` and
  `symmetricDifference` call `keys()` first and copy the receiver after
  it, `difference` copies first, as the steps say (a `keys()` that
  changes the receiver is seen by the copy); a loop over the receiver
  sees the elements that `has` adds and skips those it deletes, also
  across a compaction. The result is a plain `Set` with
  `%Set.prototype%`. V8 words a `return` that gives a non-object "return
  called on non-object"; swb copies it for these methods only.
- Limits. A table has at most 2^24 live entries; adding one more is
  `RangeError: Map maximum size exceeded` (`Set maximum size exceeded`),
  as in Chromium 148 (a `has` that adds an element at each call ends
  there). The heap limit is checked before every growth. A `Map` or `Set`
  constructor with the built-in adder changes the table directly (not
  observable); the compiled `for`-`of` and the natives that consume an
  iterable step a built-in Array, Map or Set Iterator without a result
  object when its `next` is the built-in one (`Runtime::builtin_step`).
- V8's messages for receivers print the constructor name found on the
  prototype chain (`#<Map>` for `Map.prototype`, `#<Object>` for a Map
  whose prototype is `Object.prototype`) and `[object Map Iterator]` for
  iterators. `Map.prototype.getOrInsertComputed` words a callback that is
  not a function "Map.prototype.getOrInsertComputed is not a function".
  `Map.groupBy` shares `group_items` with `Object.groupBy`.
- Iterator helpers (feature 6c, `builtins/iterator*.rs`,
  `object/iterator_helper.rs`). The specification writes each helper as an
  abstract closure with `Yield`; swb stores the variables of the closure
  in the object (`ObjectKind::IteratorHelper`, traced by the collector:
  the underlying iterator and its `next`, the callback, the counter or
  `remaining`, the inner iterator of `flatMap` and `concat`, the argument
  pairs of `concat`) and runs one pass of the closure per `next`: a native
  state machine with the states of a generator (suspended-start,
  suspended-yield, executing, completed). A `next` or `return` while the
  state is executing throws V8's `TypeError: Generator is already running`;
  any step that returns or throws completes the helper. `return` before the
  first `next` completes the helper and closes the underlying iterator
  (`concat` has none open yet); at a yield it runs the `IfAbruptCloseIterator`
  that follows the `Yield` (the inner iterator of `flatMap` first, and when
  that fails the outer one with the failure). A callback that throws
  closes the underlying iterator with its error; a failing `next` of the
  underlying iterator does not. Argument checks close the iterator, as the
  current specification says (a callback that is not callable, `ToNumber`
  of the limit, NaN, negative integers and finite limits above 2^53 - 1;
  V8 in Chromium 148 accepts the last). `drop` skips with `IteratorStep`
  (it does not read `value`). V8's words are used for the messages
  ("... called on non-object", "number 1 is not a function", "NaN must be
  positive", "Reduce of a done iterator with no initial value",
  "return called on non-object"). Two more differences from Chromium 148,
  both where swb follows the specification and test262 does not decide
  (found in the review): when the `next` of `Iterator.concat`'s current
  iterator throws or returns a non-object, V8 calls that iterator's
  `return` and swb does not (the proposal's `IteratorStepValue` does not
  close); a `next` called from inside the underlying `return` of a
  `take(0)` throws "Generator is already running" (the helper is
  executing), where V8 gives no error.
- No Rust recursion that grows with the data. A loop that skips values
  (`filter`, `drop`, `flatMap` with empty inner iterators, `concat` with
  empty iterables) runs in one native step with a handle scope per pass,
  and ticks the time countdown for each step. Nesting is the one place
  where a helper calls a helper: when the underlying iterator is a
  built-in iterator or a helper with the built-in `next`
  (`Runtime::builtin_step`), the step is a direct Rust call without a
  result object, charged to the recursion budget (2 KiB per level;
  measured 0.7 to 1.2 KiB of stack per level in a debug build); a chain of
  more than about 2,000 helpers ends with
  `RangeError: Maximum call stack size exceeded` (Chromium 148 handles
  5,000 and fails at 10,000). A user-defined `next` goes through
  `Runtime::call`, which charges 10 KiB, and so does `return` through a
  chain: a `return` (or the close of an exhausted `take`) through about
  400 levels ends with the same `RangeError`, and the helpers below that
  level stay open (measured in the review: `next` through 2,038 `map`s,
  `return` through about 400). `Iterator.from` returns an
  object that inherits from `Iterator.prototype` unchanged, so a chain of
  `from` calls does not nest; the wrapper's `next` and `return` are
  deferred calls. `Iterator.concat` stores its arguments in the helper
  while it reads them (no scope that grows with the count; the heap limit
  is checked first). `toArray` uses the dense builder and copies the rest
  of a built-in Array Iterator in one step.
- Messages for receivers: `Method Iterator Helper.prototype.next called on
  incompatible receiver [object Iterator Helper]`; the text of an object
  whose `constructor` on the chain is an accessor
  (`Iterator.prototype` and its heirs) is `[object TAG]` (V8 does not run
  getters in messages).
- Tail calls (a robustness fix after feature 6, `vm/call.rs`). The
  deferred call of a native (`NativeReturn::Call` and `Construct`) and
  the call of a bound function's target are tail calls:
  `invoke_tail_calls` runs them in a loop. The first pushes a window at
  the top of the value stack; each later one truncates the stack to
  below the previous window and pushes its own there, because the
  values of the finished call are in Rust locals and nothing can collect
  before they are in the new window. A frame that a tail call pushes
  restores the stack to the original call's `restore_top`; a native
  result drops the window. Each level is a step of the countdown and
  charges the copy of its arguments: `call` passes on all but one of its
  arguments, so `c.apply(c, args)` over a million copies of `c` would be
  quadratic work that the countdown did not see. Before, each level recursed in Rust without a
  frame and without a charge, so `f = Function.prototype.call.bind(f)`
  10,000 times overflowed the stack (found in the 6c review); now
  200,000 levels take 0.2 s (Chromium 148 returns the result for 100,000
  levels of `call`, `apply`, `Reflect.apply` and `bind`). `invoke` keeps
  its shape for ordinary functions and natives, and the loop, marked
  cold, has its own dispatch: one dispatch for both paths made
  call-heavy rows of `vm_bench` 5 to 20 % slower.
- `yield*` (feature 7a, `compiler/iterate.rs`, `vm/generator.rs`,
  §15.5.5) is a loop of instructions in the generator's code over a
  record of four registers: the iterator, its `next`, the received value
  and the resume mode. `DelegateCall` takes the method by the mode (`next`
  from the record; `throw` and `return` with `GetMethod` at each step, as
  the specification says) and calls it as an ordinary frame with the
  received value as its one argument (also the first `undefined`;
  measured in Chromium 148). `DelegateResult` checks the result object and
  reads `done`, and `value` only when it is done. `YieldDelegate` is
  `GeneratorYield` of the result object itself: the caller of `next` gets
  the delegate's object (Chromium: the same object, only `done` read). A
  resumption writes the next value and mode into the record, so the loop
  does not use the dispatch after a plain `yield`; when the delegate is
  done in return mode, `Resume` takes the compiled return path (the
  `finally` blocks run). Throw mode without a `throw` method closes the
  delegate through re-entry (`IteratorClose` with a normal completion) and
  throws V8's "The iterator does not provide a 'throw' method."; return
  mode without a `return` method returns the received value. Async
  generators (feature 7d) can reuse the loop: an `Await` after
  `DelegateCall` and before the return path, and their own yield of the
  result's `value` in place of `YieldDelegate`.
- The depth rule of `yield*`: a delegate generator's `next`, `throw` and
  `return` are the natives that return a resume request, so the delegate
  resumes as a frame of the same interpreter loop. A chain of n
  generators that delegate to each other costs n frames and no Rust stack;
  the frame limit (10,000) ends a longer chain with a catchable
  `RangeError: Maximum call stack size exceeded`, and the generators on
  the way complete (Chromium 148: a recursion of 1,500 generators works,
  one of 5,000 throws the error; a chain of 100,000 throws it in both, and
  a `return` afterwards gives `{ value, done: true }` in both). A step does
  not skip the result object, because the delegate's object is the result
  of the outer `next`: it is made once, where the innermost generator
  yields, and passes through every level. Getters of `throw`, `return`,
  `done` and `value`, and the close of a delegate without `throw`, run
  through re-entry and charge the recursion budget. The loop's backward
  jump is a step of the time countdown and a safepoint; everything held
  across a resume (the delegate, its `next`, the received value) is in the
  record, which the suspended generator's saved window traces.
- V8's messages for `yield*` depend on the form of the operand (measured
  in Chromium 148): for an identifier, `this`, a literal, a function or
  class expression, a conditional or logical expression, an optional
  chain or `void`, the type of the value (`number 5 is not iterable
  (cannot read property Symbol(Symbol.iterator))`, and for a `next`,
  `throw` or `return` that is not callable `number 5 is not a function`);
  for every other form `yield* (intermediate value) is not iterable` for
  both. V8 has a third form for some call operands, "yield* (intermediate
  value)(intermediate value) is not iterable"; swb gives the second form
  there. The errors are at the `yield` keyword.
- `GeneratorFunction` (feature 7a, `builtins/generator.rs`) is the
  `CreateDynamicFunction` of 3f with the source `(function*
  anonymous(P\n) {\nbody\n})`; its fallback prototype is
  `%GeneratorFunction.prototype%` of the constructor's realm, and the new
  function's own `prototype` inherits from `%GeneratorPrototype%`.
  `GeneratorFunction` is not a global; its `[[Prototype]]` is `Function`.
  A generator object's prototype is read from the function's `prototype`
  after the parameters are bound (§15.5.2), with `%GeneratorPrototype%`
  of the function's realm for a value that is not an object.
- Promises (feature 7b, `vm/promise.rs`, `builtins/promise.rs`,
  `builtins/promise_combinators.rs`, §27.2) are an object kind with the
  state, the result, `[[PromiseIsHandled]]` and the reactions.
  `PerformPromiseThen` adds the fulfill and the reject reaction together,
  so a promise keeps one list of reaction pairs that share the capability;
  a single pair (the usual case) needs no vector. A handler is `empty`, a
  function, or an internal continuation: a Rust function and a value,
  which the job calls with the kind of the settlement and the value. `await`
  (7c) and async generators (7d) attach continuations, so they need no
  function objects and no change of the promise code; the host's "react
  to a promise" of WebIDL is the same path (`Runtime::react_to_promise`).
- `NewPromiseCapability(%Promise%)` whose resolving functions no script
  can reach (`then`, `Promise.resolve`, `reject`, `try`, the
  `PromiseResolve` of `finally`) is the promise alone: the engine resolves
  it without the executor and the two functions. This is not observable:
  `Construct(%Promise%, executor)` reads only the `prototype` of `%Promise%`,
  which is not writable and not configurable, and the engine resolves such a
  promise once (one of the two reactions of one `then` runs once).
  Subclasses, `withResolvers` and the combinators (whose resolving
  functions `then` receives) get the functions. A promise species that is
  `%Promise%` with its built-in `@@species` getter is recognized without
  calling the getter.
- Built-in functions with internal slots (`CreateBuiltinFunction` with
  `additionalInternalSlotsList`): a native function has a list of slots
  (empty for the others), traced by the collector. A record that two
  functions share (`[[AlreadyResolved]]` of the resolving functions, the
  `alreadyCalled` of `allSettled`) is a cell in a slot. A combinator keeps
  the records that its element functions share (the list of values or
  errors, the capability, `remainingElementsCount`) in one internal array
  per call, so an element function has two slots (the state and its
  index, which becomes `undefined` when it was called) and no Rust vector
  has a size that a script chooses. These functions get `length` and
  `name` without the checks of `[[DefineOwnProperty]]` (they are new):
  `new Promise` took 1.4 µs with the checks, 1.2 µs without.
- The job queue (`vm/jobs.rs`, §9.5): one FIFO queue per runtime of
  `NewPromiseReactionJob` and `NewPromiseResolveThenableJob` with their
  realm (`GetFunctionRealm` of the handler or of `then`; `None` for an
  empty handler). The queue is a root; its bytes count as live heap memory
  (`RootSource::extra_bytes`) and each job is charged when it is enqueued,
  so a queue that grows without end ends at the heap limit. A queue
  that becomes empty with room for more than 1,024 jobs frees its
  memory, and each take of the rejection list makes a new list, so a
  burst does not stay counted for the life of the runtime (7b review).
  `Runtime::run_jobs` runs the jobs until the queue is empty, only when no
  frame is on the stack (a native that calls it does nothing). Each job is
  a step of the countdown, runs in a handle scope of its own (the job's
  values are recorded there when it leaves the queue), and a safepoint
  follows it, so a loop of jobs that run no script code still collects.
  The handlers run through re-entry from the host level, so a chain of
  jobs costs no Rust stack. A thrown value that no capability takes (a
  capability function of a subclass that throws) ends the drain with
  `ScriptError::Uncaught`; the other jobs stay in the queue for the next
  call. A termination inside a job ends the drain as a termination, never
  as a rejection (section 5): the job that ran is gone, the others stay in
  the queue until the host runs them or drops them (`clear_jobs`).
- The rejection tracker (§27.2.1.9): "reject" appends the promise to a
  list (HTML's "about-to-be-notified rejected promises list", which also
  holds its promises strongly); "handle" is the `[[PromiseIsHandled]]`
  flag that `PerformPromiseThen` sets. The list drops the handled promises
  when it has doubled since the last prune and when the host takes it
  (`take_unhandled_rejections`), so both operations cost O(1) amortized.
  The promises that the host took stay alive until it takes the next list.
- `swb-js` runs the jobs after each script under the script's deadline and
  reports each unhandled rejection on stderr as the console of Chromium
  shows it: `Uncaught (in promise) TypeError: message` and the lines of the
  error's `stack` (its text once a script read it), or the value for
  other reasons (`Uncaught (in promise) 42`). The form comes from the
  DevTools console of Chromium; the probe tool reads no console, so it is
  not measured. An unhandled rejection ends the run with exit code 1 after
  all of them are reported, like an uncaught error (Node.js 22 exits with
  1 too). The test262 runner ignores unhandled rejections.
- V8's messages and the job order are measured in Chromium 148
  (`tools/probes/js-promises.json`, 257 snippets, one left out of the
  tests): for example `Promise resolver #<Object> is not a function`, `Chaining cycle
  detected for promise #<Promise>`, `Promise executor has already been
  invoked with non-undefined arguments`, `Promise resolve or reject
  function is not callable`, `The .constructor property is not an
  object`, `PromiseResolve called on non-object`, `resolve is not a
  function` (GetPromiseResolve), and `AggregateError: All promises were
  rejected` with own `stack`, `message` and `errors`. Chromium 148 has the
  `Promise.try` of ES2025 (the capability first, a promise result is
  wrapped); the two newer test262 tests `try/avoids-wrap*.js` expect the
  2026 change and fail.
- Limits: everything after a settlement runs as jobs (chains of `then`,
  resolution with promises and thenables, `finally`, the combinators), so
  a million-step chain costs no Rust recursion. An executor runs through
  re-entry: `new Promise` nested in executors ends at about 400 levels
  with the `RangeError` that rejects the innermost promise (Chromium 148:
  3,000 levels work, 10,000 fail).
- Async functions (feature 7c, `vm/async_function.rs`,
  `builtins/async_function.rs`, §15.8, §27.7) run on the generator
  machinery: one code kind (`CodeKind::Async`) for functions, arrows and
  methods (`this`, `arguments`, `super` and `new.target` follow the form,
  as for the other kinds), not constructors, without `prototype`. The
  first instruction, `AsyncStart`, makes the result promise (the intrinsic
  capability of 7b: no script sees its functions) and an internal
  generator object without a prototype that holds it; the frame roots
  that object while it runs. The parameters and the body run at once
  until the first `await`.
- `await` is an `Await` instruction followed by the `Resume` dispatch of
  `yield`: `PromiseResolve(%Promise%, v)` of the function's realm, then
  `then_internal` with a continuation whose data is the generator
  object, then the frame moves into the object and the caller gets the
  result promise (at the first `await`; later ones return to the job).
  The job of the reaction resumes the frame at the host level, so
  a chain of async functions that await each other costs no Rust stack.
  A native promise with the default `constructor` costs one job, a
  non-promise one, a thenable two (the thenable job, then the reaction),
  a promise of a subclass three (measured in Chromium 148, the same in
  swb). A `return` of a native promise settles the result one job later
  than `return await` of it, and two jobs later than a `return` of a
  value. Nothing in `vm/promise.rs` changed. The interpreter has one arm
  for `AsyncStart`, `Await` and `AsyncReject` that calls one function
  out of line; separate arms made unrelated call rows of `vm_bench` up to
  5 % slower (layout effects, measured in four variants).
- The end of the body: a return resolves the result promise
  (`return_from_frame`, whose generator part is now out of line), with
  the frame still on the stack (the specification removes the frame
  first; Chromium 148 shows the function in the stack of an error that
  a `then` getter of the returned value makes, measured). A throw that
  nothing catches reaches one handler that the compiler puts around all
  the code after `AsyncStart` (`AsyncReject`, at the end of the code),
  which rejects the promise; so does a throw in the parameters. The
  handler table does the work, so a `try` around the body costs nothing.
  A termination skips the handler (section 5): a running function's frame
  goes and its object completes, the promise stays pending; a suspended
  one stays suspended and resumes when its promise settles in a later
  task. A function that waits for a promise that no one can settle is
  garbage once that promise is: only the reaction refers to its object.
- Async stack traces (Chromium's `at async f (file:line:column)`,
  measured in Chromium 148): when the outermost frame of the stack is an
  async function, the capture follows its result promise to the only
  reaction, and when that is the `await` of another async function, adds
  that function at the position of its `await`, then the one that awaits
  it, within `Error.stackTraceLimit`. Two reactions, a `then` or a
  resolution with the promise end the list, as in Chromium. V8 also
  follows `Promise.all` (`at async Promise.all (index 0)` and the
  function that awaits it); swb does not (no lines for built-ins).
- `AsyncFunction` is the `CreateDynamicFunction` of 3f with the source
  `(async function anonymous(P\n) {\nbody\n})`; its fallback prototype is
  `%AsyncFunction.prototype%` of the constructor's realm. Async
  generators, `for await` and `yield*` in them stay "not supported yet
  (async generator)" and "(for await)" until 7d.
- Async generators (feature 7d, `vm/async_generator.rs`, §27.6) are
  generator objects: the state's kind says what runs in the object (a
  generator, an async function with its result promise, or an async
  generator with a boxed record of `[[AsyncGeneratorState]]`, the queue of
  requests and the promise for the caller), so the state stays at 176
  bytes. A call runs the prologue and stops at `InitialYield`, as for a
  generator, so an error in the parameters throws to the caller; one
  handler over the body after `InitialYield` (`AsyncReject`) completes the
  generator with the throw. `yield v` is `Await` of `v` and `AsyncYield`
  (`AsyncGeneratorYield`): the head request is resolved with `{ value,
  done: false }`, then the frame continues with the next request or
  suspends. `return v` awaits `v` (§14.10.1).
- `next`, `return` and `throw` make the request's promise (the intrinsic
  capability of 7b), append the request and, when the generator is
  suspended, return a resume request, so the frame runs in the caller's
  interpreter loop (no Rust recursion); the frame delivers the request's
  promise to the caller when it next suspends or completes. The value of
  a return request at a `yield` is awaited before the frame resumes
  (`AsyncGeneratorUnwrapYieldResumption`): a continuation resumes it in
  return mode, or in throw mode when the promise rejects, and the frame
  takes the compiled return path as after a generator's `yield`. This is
  not observable: the generator is "executing" during that await in both
  forms, and a `PromiseResolve` that throws resumes in throw mode at the
  `yield`. The end of the body completes the head request and drains the
  queue while the frame is still on the stack (as for async functions);
  `AsyncGeneratorAwaitReturn` is a continuation that goes on with the
  drain. The queue is part of the object's heap size, so requests that
  pile up end at the heap limit. A termination removes the frame and
  completes the generator, also when it comes during a drain that a job
  started (7d review); the waiting requests are dropped and their
  promises stay pending.
- `for await` and `yield*` in async generators are the loops of `for`-`of`
  and `yield*` with awaits (`compiler/iterate.rs`): `GetAsyncIterator`
  (`GetIterator` with kind async, wrapping a sync iterator),
  `AsyncIteratorCall`, `Await` and `IteratorValue` per step, and a close
  in the implicit `finally` block that awaits the result of `return`
  (`AsyncClose`, `Await`, `AsyncCloseEnd`; after a throw completion the
  errors of `return` are dropped, `AsyncIteratorClose` §7.4.13). An async
  `yield*` keeps a record of six registers (the four of 7a and an
  `Await` pair): `AsyncDelegateCall` calls `next`, `throw` or `return`
  (without `throw`: `return`, for the close before the protocol error;
  without `return`: the received value is awaited), `AsyncDelegateResult`
  checks the awaited result, `AsyncYieldDelegate` yields its `value`. The
  eight new instructions share the one out-of-line arm of the async
  instructions; the calls of `next` and of the delegate's methods push
  frames from there (the arm reloads the top frame).
- The Async-from-Sync Iterator (§27.1.6) is an internal object kind with
  the sync record; its `next`, `return` and `throw` are natives that call
  the sync method and resolve their promise through `PerformPromiseThen`
  with internal continuations (`unwrap`, and the close of the sync
  iterator when a value rejects, the 2023 change, which Chromium 148 has
  and Node.js 22 does not). `vm/promise.rs` did not change.
- `Array.fromAsync` (`builtins/array_from_async.rs`, §23.1.2.2 of the
  2026 draft) is a state machine in an internal object kind: each `Await`
  is `PromiseResolve` and an internal reaction whose job runs the next
  step, so a million elements cost no Rust stack. Chromium 148 differs in
  its error paths (it closes the iterator also when `next` fails, closes
  an Async-from-Sync Iterator twice, and calls `return` again in every
  job when the close fails, never settling the promise). It also
  fulfills when the `length` of the result object cannot be set (a
  constructor that returns a frozen object), where `Set(A, "length",
  len, true)` throws. swb follows the specification and V8's messages.
- V8's messages and the job order are measured in Chromium 148
  (`tools/probes/js-async-iteration.json`, 341 snippets, 329 kept as
  tests): for example `5 is not async iterable` (V8 renders any operand;
  swb renders identifiers, literals, member chains and calls), `f(...) is
  not a function or its return value is not async iterable`, `yield*
  (intermediate value) is not async iterable`, `Result of the
  Symbol.asyncIterator method is not an object`, `Method
  [AsyncGenerator].prototype.next called on incompatible receiver X` (a
  rejection), `The iterator does not provide a 'throw' method.`. When the
  value of a return request at a `yield*` rejects, V8 resumes the delegate
  with `next(reason)`; the specification gives it a throw completion
  (swb calls `throw`).
- Async stack traces follow async generators too: when the outermost
  frame is an async generator, the function that awaits the promise of
  its head request follows as `at async f`, as in Chromium. V8 also shows
  the frame of the built-in `next` (`at g.next`); swb shows no lines for
  built-in functions.
- Regular expressions (feature 8a, `js-regexp`): the parser builds the
  tree bottom-up with an explicit stack of open groups and classes, so
  no part of the engine recurses with the pattern; the tree is a vector
  in which children precede their parents, and the analyses (shortest
  length, zero width) are computed as nodes are added. The modifiers
  `(?ims-ims:...)` and the case closure of `i` are resolved into the
  leaves: a class becomes the set of input characters it matches (for
  each character whose canonical form is the canonical form of a member,
  computed from the list of characters that the mapping changes), so the
  matcher compares no case at run time except in backreferences. Without
  `u` and `v`, Canonicalize is the toUppercase rule (`js-text`); with
  them it is simple case folding, which `js-text` already had (5d), so
  `u` and `v` with `i` work for all characters now (with
  MaybeSimpleCaseFolding and the two extra word characters); 8c only adds
  the property sets. Annex B.1.2.9 parses twice when a pattern has a
  group name; a scan before the parse finds group names and counts the
  capture groups (which a decimal escape of the web grammar needs), so
  one parse with the right parameters is enough. Duplicate group names:
  each group is checked against the previous group of its name only (the
  first and last of three groups can only both participate if a
  neighbouring pair can), in O(log depth) from the positions of the open
  groups and their current alternatives; the error comes when the
  group closes (V8 reports an unterminated group first). All `\k<name>`
  references to one name share one list of groups, so a pattern with N
  groups of a name and N references costs O(N) memory. Classes and
  escapes are built once: a set of up to 16 ranges maps to one tree node
  per (set, invert, `i`), so `\W` repeated a million times with `iu` runs
  its case closure once; the sets in the tree have no spare capacity. A
  class with `v` collects the ranges of its union operands and merges
  them once when it closes (merging after each operand was quadratic).
- The program: Rust enum instructions with `u32` jump targets (at most
  24 bytes each), the sets with a bitmap of the first 256 characters, and
  the slots: two per capture, one start position per open group, and
  the registers of loops (counter and iteration start) and lookarounds.
  A quantifier over one character test without captures is one `Repeat`
  instruction (it gives back one character per backtrack, or takes one
  more if lazy); `*` and `?` over other atoms need no counter; counted
  loops have `LoopInit`/`LoopHead`/`LoopTail` with the empty check only
  when the atom can match empty and the capture reset only when it has
  groups (§22.2.2.3.1). An atom that only matches the empty string runs
  at most once (`(?:){4294967296}` is instant; V8 is too). Counts above
  2^32 - 2 count as unbounded. A lookbehind compiles its body backward;
  a capture closes from its recorded start to the current position in
  either direction. The search skips start positions whose character is
  not in the set of first characters, when every match starts with one.
- The matcher API is resumable: `Matcher::new(regex, start)` and
  `Matcher::run(regex, input, steps, memory_limit)`, which returns
  `Match`, `NoMatch` or `Pending` after at most `steps` steps
  (instructions, characters of a `Repeat` loop or a backreference
  compare, restored choice points); an instruction that runs out of
  steps keeps its progress in the state and goes on in the next call, so
  each call does bounded work (popping a choice point is a step too: a
  call that runs out of steps while it pops continues in the next call).
  The caller passes the input again on each
  call (positions are code-unit indices; the matcher checks that the
  length did not change), so 8b can run the time check and the heap
  check between calls without a borrow of the input. A start inside a
  surrogate pair with `u` or `v` moves to the pair (§22.2.7.2; V8 does
  the same). Choice points have a generation number; a slot is logged in
  the undo log only once per choice point, so loops without choice
  points do not grow the log. `heap_bytes` of the program and of the
  matcher state are for the heap accounting of 8b.
- Limits: at most 32,767 capture groups ("Too many captures", as V8),
  nesting of groups and classes at most 32,768 deep, at most 2^22 tree
  nodes and 2^22 instructions ("Regular expression too large"; V8 accepts
  1,000,000 nested `(?:` but not 100,000 nested lookaheads). The
  backtrack stack and the undo log stop at the caller's memory limit
  (`Error::MemoryLimit`): the check runs before a vector doubles, with
  the size after the growth, and the limit also covers the capture slots
  and their undo markers (12 bytes per slot). `check_literal` runs the parser without
  building the tree or any set, so a 10 MB literal costs no memory beyond
  the parser's stacks.
- Errors: `Error::Syntax(SyntaxKind)` with V8's reason
  (`Error::message` writes `Invalid regular expression: /P/F: REASON`
  with the pattern unescaped, as V8 does for both literals and the
  constructor), `InvalidFlags` (the constructor's message, "Invalid flags
  supplied to RegExp constructor 'F'", is 8b's),
  `MemoryLimit`, `StepLimit` (only `Regex::exec`, the helper for tests
  and tools) and `InputChanged`. The order of errors follows V8 where
  measured (the first error from the left; unknown `\k` names at the
  end): `^{`, `${` and `\B{` with `u` or `v` are "Lone quantifier
  brackets" (a `{` after an assertion is not its quantifier), a class
  with `v` that ends after `-` is "Unterminated character class", and a
  duplicate group name is reported when the group closes. Two differences
  from Chromium 148 are deliberate, because swb follows the
  specification (`KNOWN` in `crates/js-regexp/tests/cases.rs`): the
  single-character alternatives of `\q{...}` fold with `iv`
  (§22.2.2.9 applies MaybeSimpleCaseFolding to the whole
  ClassStringDisjunction; Chromium folds only the strings of other
  lengths, so `[\q{a|bc}]` does not match "A" there), and `\p{sc=Hrkt}`,
  `\p{scx=Hrkt}` and the long name `Katakana_Or_Hiragana` are accepted
  (§22.2.2.9.8 takes every value of `PropertyValueAliases.txt`; Chromium
  says "Invalid property name"). The delta review measured a third
  difference: in Chromium the `i` of a modifier group also applies to
  some atoms after the group (`(?i:x)\w` with `u` matches "xſ",
  `(?i:[a])[a]` with `v` matches "aA"); swb applies it only inside the
  group, as §22.2.2 says.
- `\p{...}` and `\P{...}` (feature 8c). The names and values are checked
  as in 8a (ECMA-262 tables 65 to 67 and `PropertyValueAliases.txt`; also
  `WSpace`, which `PropertyAliases.txt` lists and V8 accepts, though
  table 66 leaves it out). The sets come from the Unicode Character
  Database 17.0.0, the version of the names, of test262's generated
  tests and of the other tables; `swbtools js-unicode-tables` writes them
  to `js-text/src/unicode/property_data.rs` (generator
  `tools/swbtools/js_property_sets.py`, test
  `tools/tests/test_js_unicode_tables.py`). Files: `DerivedGeneralCategory.txt`,
  `Scripts.txt`, `ScriptExtensions.txt`, `PropList.txt`,
  `DerivedCoreProperties.txt`, `DerivedNormalizationProps.txt`
  (`Changes_When_NFKC_Casefolded`), `DerivedBinaryProperties.txt`
  (`Bidi_Mirrored`), `ucd/emoji/emoji-data.txt`, and
  `emoji/emoji-sequences.txt` and `emoji/emoji-zwj-sequences.txt` of the
  17.0.0 emoji data (UTS #51). Data: a set is a list of ranges, each as
  two LEB128 numbers (the gap from the previous range and the length
  minus one); all sets are one byte string of 76,118 bytes plus offset
  tables (about 2 KB), which `js-text/src/unicode/property_sets.rs`
  decodes on use. `General_Category` has one set for each of its 30
  values that is not a group; `L`, `LC`, `M`, `N`, `P`, `S`, `Z` and `C`
  unite those at decode time. `Script_Extensions` has a set per script
  (the explicit entries replace the `Script` value of their code points).
  `ASCII`, `Any` and `Assigned` (the complement of `Cn`) have no data.
  A property of strings has its single code points as a set and its
  sequences as length-prefixed lists of code points; `RGI_Emoji` is the
  union of the other six (`emoji-sequences.txt` says so) and has no data.
- The parser builds a property set once for each property, `\P` and `i`
  and keeps it (`property_sets`), and keeps the node of `\p{...}` outside
  a class by the same key (`property_nodes`): 10,000 `\p{L}` with `iu`
  compile in 1 ms and share one set. The cache of set nodes (8a: sets of
  at most 16 ranges) now has no limit per set; it holds at most 2^18
  ranges in the keys. A tree may have at most 2^22 ranges in all its
  sets (8 bytes each); beyond that the pattern is "too large" (a class
  `[\p{L}x]` with a thousand different `x` has a thousand different
  sets). `check_literal` builds no set.
- Case-insensitive property sets: with `u` the set goes through the case
  closure like any class. With `v` and `i` the set is first folded
  (`MaybeSimpleCaseFolding`) and `\P{...}` is the complement among the
  fixed points of the folding (`CharacterComplement`), and the strings of
  a property of strings are folded code point by code point. A deliberate
  difference from §22.2.2.9, which leaves a lone `General_Category` value
  (`\p{Lu}`, not `\p{gc=Lu}`) unfolded: Chromium 148 folds it like every
  other property (`/\P{Lu}/iv` does not match "A", `/[\p{Lu}&&\p{Ll}]/iv`
  matches "a"; the cases are in `tools/probes/js-regexp.json`), and no
  test262 test tells the two apart. The draft of 2026 still has the text.
  swb follows Chromium here, unlike the other differences in this
  section, because the text gives `\p{Lu}` and `\p{gc=Lu}` different
  sets with `iv` although they name the same property value, and the
  folding of `v` exists to make complements and set operations
  consistent with `i`. This looks like an omission in the text.
- `\p{ASCII}` with `iv` matches U+017F and U+212A, because their folds
  `s` and `k` are in the folded set, and `\P{ASCII}`, `[^\p{ASCII}]`,
  `[\p{Lu}--\p{ASCII}]` and `[\p{L}&&\p{ASCII}]` follow from that
  (MaybeSimpleCaseFolding and `AllCharacters`, §22.2.2.9). swb follows
  the spec here. Chromium 148 gives the opposite result on
  `/\p{ASCII}/iv.test("\u017f")` and on five more cases (`KNOWN` in
  `cases.rs`; the cases are in the probe file). The review of 8c found
  this difference.
- `\p{sc=Hrkt}` and `\p{scx=Hrkt}` (and `Katakana_Or_Hiragana`) are
  accepted and match nothing: no code point has this value in the
  UCD 17.0.0.
- Strings of a class with `v` (`\q{...}`, a property of strings, and the
  results of `&&`, `--` and union) with more than 8 strings compile to a
  trie, `Node::Strings` in the tree and one `Inst::Switch` per trie node
  in the program (a sorted table of (code point, target) found by binary
  search, in `Program::tables`). A short list keeps the alternation of
  sequences. The alternation tried every string at every position:
  `/\p{RGI_Emoji}/v.test("a".repeat(1e6))` took 43 s (3,800 strings).
  Order: the spec tries the strings from the longest to the shortest.
  All strings that match at one position are prefixes of each other, so
  they lie on one path of the trie; the trie goes down as far as the
  input allows, and a node where a string ends has a `Fork` before its
  `Switch`, a choice point that is taken only after everything deeper
  failed. That is the order of the alternation, also when a later part of
  the pattern fails and the matcher backtracks into the strings. A step
  is one `Switch` or one `Fork`; the choice points are those forks, and
  the matcher state is unchanged (the resumable matcher and the undo log
  need nothing new). With `i` the strings are folded when the trie is
  built and the `Switch` folds the input character (`Canonicalize`, the
  simple case folding, as the sets do). A lookbehind builds the trie from
  the reversed strings and reads the input backward (the strings that end
  at a position are suffixes of each other, the same argument). The
  prefix scan takes the first characters of the strings, so a position
  that cannot start a match is skipped. A tree may have at most 2^23
  characters in all its string lists. The strings of a property are
  decoded, folded and sorted once for each property and `i` and shared
  (`Arc`) by its occurrences; one occurrence compiles in about 2 ms and
  20 of them in 11 ms; a pattern with about 750 of them is "too large".
  A pattern like `\p{RGI_Emoji}+x` over 200,000 emoji is still quadratic
  (each start position scans to the end), as in any backtracking matcher;
  the caller's step limit and time check stop it.
  `Error::Unsupported` is gone.
- Chromium 148 agrees with Unicode 17.0.0 on all 264 probe cases with
  `\p{...}` (151 matches, 110 patterns, 3 literals), so there is no
  `KNOWN` entry for the Unicode version. test262's 613 tests of
  `built-ins/RegExp/property-escapes` and its tests of
  `unicodeSets/generated` pass; the largest take 0.5 s.
- The `RegExp` built-in (feature 8b, `builtins/regexp*.rs`,
  `builtins/string_match.rs`, `object/regexp.rs`): a `RegExp` object is
  `ObjectKind::RegExp` with `[[OriginalSource]]` and the compiled program
  in an `Rc`; `[[OriginalFlags]]` are the program's flags, because they
  are only read through their letters. Each holder of a program counts
  its share in the heap (bytes divided by the holders), so the sum is the
  program's size. A regular expression literal is an instruction
  `NewRegExp` (it shares the out-of-line arm of `GetTemplate`) with a
  constant that keeps the pattern atom and, after the first evaluation,
  the program: each evaluation makes a new object, the compile happens
  once. The errors of `Regex::new` become V8's `SyntaxError`s ("Invalid
  flags supplied to RegExp constructor 'F'", "Invalid regular expression:
  /P/F: reason"). Messages that quote a pattern with lone surrogates show
  U+FFFD for them (the messages are Rust strings).
- The match (`regexp_exec.rs`): `RegExpBuiltinExec` reads and converts
  `lastIndex` before it reads the program and the flags, so a `valueOf`
  that calls `compile` changes both for the running match (Chromium 148
  agrees). The matcher runs in slices of 32,768 steps; between two slices
  the driver charges the countdown (four matcher steps per step), runs a
  safepoint (a collection changes nothing: the input is rooted, positions
  are indices, the program is an `Rc`) and gives the matcher the bytes
  that the heap limit still allows, at most 256 MiB. Beyond that the
  match fails with V8's `RangeError` "Maximum call stack size exceeded".
  The bound is lower than Chromium's: with `^(?:a|b)*c` on
  `'ab'.repeat(n) + '!'`, Chromium 148 matches up to n = 4,000,000 and
  fails at 5,000,000, and swb fails from n = 2,000,000 (1,000,000 is
  fine), because each loop iteration takes two choice points of 32
  bytes (review; backlog). When the heap limit was the
  smaller bound, the driver collects once and starts the match again
  before it fails, so garbage does not cause the error. The VM keeps the
  matcher's vectors for the next match (`Matcher::reset`) while they are
  small. A match start inside a surrogate pair with `u` or `v` reports
  the pair's start as `index` (the matcher of 8a; V8 does the same).
- `RegExpExec` with the built-in `exec` runs no call and returns the
  captures (`Exec::Match`); the symbol methods read `0`, `length`,
  `index`, the captures and `groups` of such a result from the captures,
  because a result array that no script has seen cannot observe the
  reads. Result arrays and new `RegExp` objects of `%RegExp%` are made
  with the shapes that the realm recorded at the first one (no property
  definitions). The `groups` object of duplicate names has one property
  per name in the order of first appearance with the value of the group
  that matched (§22.2.7.2 step 33).
- Fast paths (`regexp_fast.rs`): the realm records the shape of a new
  `RegExp` object, the shapes of `%RegExp.prototype%` and `%RegExp%`, and
  the slots of `exec`, `flags`, the flag getters, `constructor`, the
  symbol methods and `@@species`. When the object and both of these are
  as recorded, `Get(R, "flags")` is the program's flags without the eight
  getter calls, `Get(R, "exec")` is the built-in `exec`, `lastIndex` is
  slot 0, and `SpeciesConstructor` is `%RegExp%`. `@@split` of such an
  object without `y` then searches each next match from the current
  position with the object's own program instead of making the sticky
  splitter and trying every position (the same matches: the attempt at a
  position is the same; matches at or after the end are ignored, as the
  loop of the specification never tries them; the legacy statics get the
  last match the splitter would have found). `@@replace` with a template
  appends each replacement as its match is found while all results are
  built-in matches (the reads and `GetSubstitution` run no script code);
  with a replacer function, or once a script's `exec` gave a result, the
  results are kept until the loop ends (built-in matches as spans in a
  vector charged to the heap, other results in an internal array), as
  the specification orders the calls.
- The legacy static properties follow V8 (measured), not the legacy
  features proposal: accessors with a getter and a setter each
  (non-enumerable, configurable), getters for any receiver, every
  successful match of any `RegExp` object (subclasses included) updates
  them, only `input` and `$_` can be set, and `input` is the subject of
  the last match until a script sets it; `compile` works on objects of
  subclasses too. The test262 feature `legacy-regexp` stays out of the
  subset (18 of its tests check the proposal's TypeErrors). The getter
  `flags` reads `hasIndices`, `global`, `ignoreCase`, `multiline`,
  `dotAll`, `unicode`, `unicodeSets`, `sticky` as §22.2.6.4 says (V8
  reads `sticky` before `unicodeSets`). V8 rejects a literal pattern of
  100,000 characters as too large; swb compiles it (the limits of 8a).
- `%RegExpStringIteratorPrototype%.next` follows the generator of
  `CreateRegExpStringIterator`: a throw from `exec` (or from the reads
  after it) completes the iterator, so the next call returns `done`. V8
  (Node.js 22, measured) keeps the iterator going.
- Speed (vm_bench, against `node --jitless`): a global `replace` with a
  template takes 0.5 times V8's interpreter, with captures or a function
  3 to 5 times, `split` 3 times, `match` with `g` 7 times (V8 caches
  one-character strings), the loops of `exec` and `test` 2 to 3 times,
  `matchAll` 16 times (a result array and an iterator result per step);
  all are linear in the input (docs/performance.md).
- Proxy (feature 9a, `object/proxy.rs`, `vm/proxy.rs`, `builtins/proxy.rs`,
  §10.5, §28.2): `ObjectKind::Proxy` holds `[[ProxyTarget]]` and
  `[[ProxyHandler]]` (both `None` after revocation) and whether the proxy
  has `[[Call]]` and `[[Construct]]`, fixed by ProxyCreate. A proxy stores
  no properties; its shape is the root shape of `null`. Only the entries of
  private names go into it, because private fields work on proxies without
  traps (`PrivateFieldAdd`). `Proxy` is a constructor without `prototype`;
  the revoke function keeps the proxy in a native slot.
- Dispatch: the heap's internal methods stay the ordinary ones and run no
  script code. Its three walks along a prototype chain (`[[Get]]`,
  `[[Set]]`, `[[HasProperty]]`) look at the kind of each object they visit
  (a compare on the record that the walk loads anyway) and return
  `GetResult::Generic`, `SetResult::Generic` or `None` when they meet a
  proxy, or when the receiver of a `[[Set]]` is one, before anything has
  happened. The caller then runs the generic path of `vm/proxy.rs` from the
  start: the same walk over the runtime's internal methods, and the
  proxy's methods at a proxy. The runtime's internal methods
  (`own_property`, `define_own_property`, `define_or_throw`, `own_keys`,
  `has_property`, `get_with_receiver`, `set_with_receiver`,
  `delete_generic`, `get_prototype_of`, `set_prototype_of`,
  `is_extensible`, `prevent_extensions`) dispatch a proxy at their start,
  so the natives and the interpreter reach the traps through them. The
  heap's methods that change an object refuse a proxy with an internal
  error, so a call site that the review missed ends the script instead of
  writing into the proxy. Proxies do not have ordinary elements
  (`element_access.rs`): a chain with a proxy makes the natives take the
  specification's steps, and the dense shortcut checks every prototype, so
  a proxy on the chain of an array or of `Array.prototype` stops it (the
  realm flag "no prototype of an array has indexed properties" of section
  6 was never built; the chain walk does its work). The pristine-shape
  checks of the `RegExp` fast paths cannot meet a proxy: a proxy is not a
  `RegExp` object, and a proxy as the prototype of `RegExp.prototype`
  changes the recorded shape.
- `[[Call]]` and `[[Construct]]` of a proxy without a trap are tail calls of
  the target in the loop of `invoke_tail_calls`, and a trap is a tail call
  of the trap with `(target, this, argArray)`, so a chain of a million
  callable proxies costs no Rust stack (Chromium 148 calls through 100,000).
  Only the result check of a `construct` trap needs re-entry. `IsArray` and
  `GetFunctionRealm` (a revoked proxy is V8's TypeError "Cannot perform
  'undefined' on a proxy that has been revoked") are loops.
- Recursion: every other internal method of a proxy is one level of Rust
  recursion when it forwards to a target, and charges `PROXY_WEIGHT`
  (2 KiB; measured per level at most 960 bytes in a debug build and 752 in
  a release build) and a time step; a trap call charges the weight of a
  re-entry. A chain of a million proxies ends with V8's `RangeError`
  "Maximum call stack size exceeded" after about 2,000 levels, also in the
  debug build on a thread with 8 MiB (`proxy_hostile.rs`).
- Errors where the `false` comes from: the generic `[[Set]]`,
  `[[DefineOwnProperty]]`, `[[Delete]]` and `[[PreventExtensions]]` take a
  `throw` flag (strict assignment, `DefinePropertyOrThrow`, strict
  `delete`, `Object.freeze`) and raise the `TypeError` at the point that
  returns `false`, as V8 does: a falsish trap names the trap ("'set' on
  proxy: trap returned falsish for property 'x'"), a target that refuses
  gives its own message ("Cannot assign to read only property 'x' of object
  '#<Object>'", "Cannot define property x, object is not extensible").
  Nothing observable happens between that point and the throw of the
  specification. The texts of all invariant errors are V8's, measured in
  Chromium 148 (`tools/probes/js-proxy.json`); V8 shows a proxy in messages
  as its target (`#<Map>`, `[object RegExp]`).
- `[[OwnPropertyKeys]]`: CreateListFromArrayLike grows the list with a heap
  check at each doubling (a `length` of 2^32 − 1 ends at the time or heap
  limit), and the duplicate check and the checks against the target's keys
  use one hash set with the keyed hasher, so a million keys against a
  non-extensible target with a million properties are linear.
- `for`-`in` over a chain with a proxy follows V8 (measured): the walk calls
  `ownKeys` and then `getPrototypeOf` of each proxy and keeps all its string
  keys; each step asks the proxy's `getOwnPropertyDescriptor` and visits the
  key if it is enumerable, or walks on with `getPrototypeOf` when the proxy
  does not have it. swb has no cache of `for`-`in` keys: each loop collects
  its own.
- Rooting: a proxy method records the target, the handler, the key, the
  receiver, the values of descriptors and its results in the caller's
  handle scope; the interpreter's paths into the generic methods are
  out-of-line functions that open a scope (the hot arms gained one call).
  The trap names are atoms of the VM. A read of a global name whose
  global object has a proxy on its prototype chain runs the traps without
  the frame's pc saved, so an error inside such a trap has the position of
  the last saved pc in its stack trace.
- `console.log` of the shell shows the innermost target of a proxy without
  running a trap, and a revoked proxy as `<Revoked Proxy>`; Node.js 22
  unwraps one level and runs the traps of an inner proxy.
- Two differences from Chromium 148, where swb follows the specification
  (`KNOWN` in `gen-proxy-cases.js`): in a `with` body V8 omits the second
  `HasProperty` of GetBindingValue and SetMutableBinding (§9.1.1.2.5,
  §9.1.1.2.6), and V8 resolves the target of an assignment after its right
  side (the case of feature 3e, here visible through a `has` trap).
- Speed: `vm_bench` has five proxy rows; a read without a trap takes about
  90 ns, with a `get` trap about 200 ns (`node --jitless`: 16 and 28 ns,
  V8's built-ins for proxies are native code). On the ordinary paths a
  heap walk compares the object kind of each record that it already
  loads. Callgrind counts 0.3 % more instructions for the mixed programs
  and 1.2 % more for `fib` and the `new Function` loop; the rows of
  `vm_bench` that moved by more than 3 % move between builds (code
  layout, docs/performance.md).
- Weak collections (feature 9b, `object/weak.rs`, `heap/gc.rs`,
  `heap/weak_roots.rs`, `builtins/weak.rs`, `weak_ref.rs`, §24.3, §24.4,
  §26.1, §26.2, §9.9 to §9.13): `ObjectKind::WeakMap` and `WeakSet` hold
  the hash table of `Map` and `Set` (`Table`, keyed with
  `Heap::key_hash` by the handle of an object or of a symbol that
  `Symbol.for` did not register; well-known symbols are allowed, as the
  specification says and Chromium 148 does). The tables have no cursors
  and no `size`, and no Rust code iterates them except the collector.
  `WeakRef` holds its target, `FinalizationRegistry` its callback and its
  cells (target and unregister token weak, held value strong, an index
  from token to cells so that `unregister` costs the cells of its token).
- Ephemerons (Hayes 1997): the marker does not trace a weak table. The
  first phase marks from the roots and records the live `WeakMap`s,
  `WeakSet`s, `WeakRef`s and registries by slot. If a `WeakMap` is live,
  a second phase visits each entry of each recorded table once, in parts
  of 1,024 entries like a large array: an entry with a marked key marks
  its value, any other value waits in a list under its key (a map from
  key to list head, with a bit per object and symbol slot so that most
  marked objects need no lookup). In the second phase, a `WeakMap`
  marked by the drain is visited when it is marked, an object that the
  drain takes from the work list pushes its waiting list (parts of 1,024
  values), and a marked symbol does the same. Each entry is visited and
  waits at most once and each waiting value is marked at most once, so
  the work is linear in entries plus marked things for chains in any
  order, in one table or across many (a test: 100,000 links across 1,000
  tables in both orders, and a million links in reverse order, on an
  8 MiB stack). The first phase has no check: the drain of the
  second phase is a separate instance of the generic drain.
- Sweep order: after the marking and before anything is freed, each live
  weak table removes the entries whose keys are not marked and compacts
  (the memory returns in this collection), a live `WeakRef` forgets a
  dead target, a live registry moves the cells with dead targets to its
  pending list, drops dead tokens and enters the cleanup queue; then the
  other weak tables (atoms, transitions) and the arenas are swept. No
  entry, target or token outlives the slot it names; even if one did, a
  handle carries its slot's generation, which equality and the keyed
  hash include, so a later object in the slot never matches it.
- `AddToKeptObjects` (new `WeakRef`, `deref`) adds the target to a set in
  the heap, a root; `ClearKeptObjects` empties it at the end of
  `Runtime::run_jobs` (the end of the synchronous run of a script and its
  promise jobs, where HTML performs it at the end of a microtask
  checkpoint), and `Runtime::clear_kept_objects` lets a host do it at
  other ends of script runs. `run_jobs` clears only when it empties the
  queue, so after a script or a job that threw, the host calls `run_jobs`
  again or `clear_kept_objects` (HTML performs a microtask checkpoint
  after every script, also one that threw; the shell exits). A set
  instead of the specification's list: a loop of `deref` does not grow
  it.
- `HostEnqueueFinalizationRegistryCleanupJob`: a collection puts a
  registry whose cells became pending into the heap's cleanup queue
  (once). There is no hook trait yet (section 12): the host asks
  `Runtime::pending_finalization_cleanups` after its tasks and runs one
  job per registry with `Runtime::run_finalization_cleanup` as a task of
  its own (the shell
  after the promise jobs, each followed by the promise jobs it enqueued;
  HTML queues a global task); the job calls the callback with the held
  value of each pending cell in the order the collector found them, so no
  script code runs during a collection. An exception ends the job, goes
  to the host as `ScriptError::Uncaught` like an error of a promise job,
  and the remaining cells get a new job. The queue holds its registries
  weakly: a registry that script cannot reach calls no callback, also
  when its job was queued before it died (cleanup is optional, §9.13).
  `cleanupSome` is not defined (Chromium 148 has no such method).
- No difference from Chromium 148 was found in 318 measured snippets
  (`tools/probes/js-weak.json`); the messages are V8's
  (`Invalid value used as weak map key`, `WeakRef: invalid target`,
  `FinalizationRegistry.prototype.register: target and holdings must not
  be same`, `Invalid unregisterToken ('1')` and the others), and
  `getOrInsertComputed` checks the callback before the key, as the
  proposal and test262 do.

- `BigInt` arithmetic (feature 10a, `js-text/src/bigint/`): the
  library type `swb_js_text::bigint::BigInt` is the engine's `BigInt`
  (`value.rs` re-exports it); the arena, equality and the keyed hash of
  `Map` keys use its normalized limbs. The operations are free functions
  that return a new value or a `BigIntError` (size limit, division by
  zero, negative exponent, not an integral Number, out of memory, the
  syntax error of StringToBigInt); `bigint::cost` gives the work of each
  for its operands, so the engine can charge it before or after the call.
  No script can make a `BigInt` before feature 10b. `Natural`
  (`number/natural.rs`, 32-bit limbs, infallible, sizes of at most a few
  hundred limbs in the number conversions) stays separate: sharing the
  code would put fallible allocation and 64-bit limbs into the hot paths
  of `toFixed` and `parseInt` for no gain.
- The size limit is lower than Chromium's (KNOWN): 2^18 bits against
  2^30 − 64 (1,073,741,760, measured in Chromium 148 with
  `1n << (k − 1n)` in `tools/probes/js-bigint.json`; the message is
  `Maximum BigInt size exceeded`). Chromium's subquadratic algorithms
  print a value of 2^18 bits in radix 10 in 8.5 ms (`node`), swb's
  quadratic ones in 32 ms; at Chromium's limit a quadratic `toString`
  would take about six days. swb's error is exact: a result of 2^18 bits
  is allowed and one of 2^18 + 1 bits is not, also for products and
  powers (Chromium rejects `(1n << (k − 2n)) * 2n`, whose result has
  the k bits of its limit).

- `BigInt` in the language (feature 10b; `vm/bigint.rs`,
  `builtins/bigint.rs`, `compiler/bigint.rs`). The value representation
  is unchanged (section 7). A literal is a constant of its code object
  (`ConstSpec::BigInt`; `-1n` is one negative constant), which the code
  object traces; a property key `1n` is the decimal text of the value,
  computed at compile time. A literal above the size limit is a
  `SyntaxError` "Maximum BigInt size exceeded" before the compile, as in
  V8 (measured in Chromium 148 with a literal of 2^30 + 64 bits: also in
  a function that never runs and as a key), at swb's lower limit; only
  literals of at least 65,539 characters are parsed for the check. A
  `BigInt` object is `ObjectKind::BigIntWrapper`; `%BigInt.prototype%` is
  an ordinary object (§21.2.3), the property reads and writes of a
  `BigInt` primitive start there.
- The interpreter's integer and double fast paths are unchanged: every
  other operand already took the general paths (`arith_slow`,
  `add_slow`, the relational and equality operators), which now apply
  `ToNumeric` and dispatch to the `BigInt` operations; the unary
  operators and `++`/`--` got one general path (`unary_slow`), and the
  `ToNumeric` instruction (the old value of a postfix `++`) no longer
  shares the arm of `ToNumber` (unary `+`, which throws for a `BigInt`).
  Every operation charges `bigint::cost / 16` steps before the work (a
  step is about 12 ns, a unit about 0.7 ns; loops of the slowest
  operations on values at the limit end within 40 ms of a 200 ms
  `--time-limit` in a release build, process start included), and every
  result goes through `Heap::alloc_bigint`. `StringToBigInt` and the
  comparisons with a string charge `cost::parse` of the text before the
  scan. `===`, `SameValue`, `SameValueZero` and the keyed hash compare
  the limbs without a charge (one pass, at most 4,096 limbs). The
  decimal text of a value in a message or in `console.log` (35 ms at the
  limit) is charged too: the message is built behind a shared borrow, so
  the charge lowers the countdown without a check (`Vm::charge_deferred`,
  at most to 1; the countdown is a `Cell`), and the next step runs the
  check. Before the review found it, an endless loop of `big in big`
  with a value at the limit never reached a time check.
- Comparisons by value: all language comparisons go through
  `Heap::equal` (`===`, `switch`, `Object.is`, the `SameValue` of
  property definitions and of frozen objects, the proxy invariants,
  `includes`, `indexOf` and `lastIndexOf` whose number fast path does not
  take a `BigInt`) or the table's `same_value_zero` (`Map`, `Set`); no
  code compares two `BigInt` handles alone.
- Messages are V8's (`Cannot mix BigInt and other types, use explicit
  conversions`, `Cannot convert a BigInt value to a number`, `BigInts have
  no unsigned right shift, use >> instead`, `Cannot convert X to a
  BigInt`, `The number X cannot be converted to a BigInt because it is
  not an integer`, `Invalid value: not (convertible to) a safe integer`
  for the bit count of `asIntN`, the `requires that 'this' be a BigInt`
  of the receivers). The text of a failed `StringToBigInt` is quoted up
  to 1,000 code units and then `…`, as V8 does; a lone surrogate in it
  becomes U+FFFD (KNOWN: swb's messages are Rust strings). The `[[Set]]`
  of an array `length` with a `BigInt` or a symbol gets Chromium's
  prefix `Failed to set the 'length' property on 'Array': `.
- `toLocaleString` has no ECMA-402 (KNOWN, as for Numbers): it ignores
  its arguments and groups the digits in threes with commas, Chromium's
  output with its default locale (en-US, measured).
- `ToBigInt64` and `ToBigUint64` exist for the 64-bit typed arrays of
  feature 11 (unit tests only so far).
- No other difference from Chromium 148 was found in 850 measured
  snippets (`tools/probes/js-bigint.json`); the snippets of values above
  swb's limit and the callee text of `o[Symbol.iterator]()` (swb shows
  `(intermediate value)` for every receiver, also for Numbers; not
  specific to `BigInt`) are left out of the test.

- `ArrayBuffer` and `DataView` (feature 11a; `object/buffer.rs`,
  `builtins/array_buffer.rs`, `builtins/data_view.rs`). Two object kinds:
  `ObjectKind::ArrayBuffer` holds `[[ArrayBufferData]]` as a Rust vector
  (`None` when detached; its length is `[[ArrayBufferByteLength]]`) and
  `[[ArrayBufferMaxByteLength]]` of a resizable buffer;
  `ObjectKind::DataView` holds a `BufferView` (the buffer, the byte
  offset, a fixed length or `None` for a view that tracks a resizable
  buffer), which the typed arrays of 11b reuse. The value representation
  is unchanged (section 7).
- Memory: the bytes live outside the arenas, in the vector that the
  buffer object owns; the sweep drops it with the object. The heap counts
  the capacity of the vector like the other buffers of live things
  (`ObjectKind::heap_size`), so it counts in the limit and in the
  collection threshold. An allocation reserves first (`Heap::reserve_buffer`,
  a safepoint that collects as `reserve` does) and then allocates
  fallibly (`try_reserve_exact`, then a zero-fill; no `vec![0; n]`).
  A length that does not fit is a `RangeError` with V8's message ("Array
  buffer allocation failed"), not a termination: `CreateByteDataBlock`
  throws (§6.2.9.1), Chromium does the same, and the script can go on.
  The room for buffers ends at 90 % of the heap limit, where a
  collection ends the script (section 14): with the full limit, a script
  that keeps buffers and drops them after the `RangeError` would end at
  the next collection instead (found in the review). A
  length above V8's limit (2^35 − 1 bytes, measured; `maxByteLength` up
  to 2^35 − 4096) is "Invalid array buffer length" ("max length"). A
  resizable buffer allocates only its current length, not its maximum
  (V8 reserves the maximum as virtual memory); `resize` grows the vector
  exactly and gives back the capacity when it shrinks below half of it.
  Detaching (and `transfer`) takes the bytes out of the heap estimate at
  once (`Heap::release_bytes`), so a loop that allocates and detaches
  needs no collection. `transfer` moves the vector into the new buffer
  (the note of §25.1.3.3 allows it): a transfer to the same or a smaller
  length copies nothing.
- Time: zero-fills and copies (the constructor, `slice`, `resize`,
  `transfer` to another length) charge one step per 128 bytes after the
  work (`Runtime::charge_bytes`; a first write of new memory runs at about
  10 GB/s, a step is about 12 ns): endless loops of 64 MiB or 256 MiB
  allocations, slices, transfers and resizes end within 21 ms of a 200 ms
  limit in a release build. The zero-fill touches every page, so a
  buffer that a script never writes costs what a written one costs
  (V8's allocation is lazy: 40 buffers of 64 MiB take 21 ms in
  `node --jitless`, 271 ms in swb). `slice` with the default
  constructor copies into a new vector instead of zero-filling and then
  copying.
- `DataView` methods: one native per method over `GetViewValue` and
  `SetViewValue` with the element type as a parameter; the view record
  is read once (its fields never change), the conversions run, then
  `Heap::view_range` checks the buffer (detached, out of bounds) and the
  element is read or written in place. A read of an 8-, 16- or 32-bit
  integer type gives `Value::Int` without an allocation, `Uint32` and
  the floats a Number, the 64-bit types a new `BigInt` (`ToBigInt64`
  and `ToBigUint64` of 10b on the way in). binary16 uses `f16_round` of
  `js-text` with the bits of `number/binary16.rs`. NaN bits: binary32
  and binary64 keep the bits of the double (the hardware conversion), a
  NaN becomes the quiet binary16 NaN with its sign, and a binary16 NaN
  reads as the quiet double NaN with its payload, all as Chromium 148
  does (measured). The element conversions (`encode`, `decode`,
  `Runtime::to_element`, `Runtime::element_value`, with
  `Uint8Clamped` for 11b) are shared with the typed arrays.
- `SpeciesConstructor` has a generic form for natives
  (`Runtime::species_constructor`); `slice` with the default
  constructor (and its built-in `@@species` getter) creates the buffer
  without a call. `ArrayBuffer.isView` is true for a `DataView` (11b
  adds the typed arrays).
- `DetachArrayBuffer` is `Runtime::detach_array_buffer` for the host;
  the shell and the test262 runner have `$262.detachArrayBuffer`.
- Not defined, as in Chromium 148 (measured in the probe page):
  `SharedArrayBuffer` (Chromium has it only in cross-origin isolated
  pages; `crossOriginIsolated` is false there) and the immutable buffers
  (`transferToImmutable`, `sliceToImmutable`, `immutable`).
- Differences from Chromium 148 (KNOWN, 724 measured snippets in
  `tools/probes/js-binary.json` agree otherwise, also V8's messages,
  among them the odd "Invalid DataView length undefined" of the check
  after `OrdinaryCreateFromConstructor`): `ToIndex` of the length of
  `new ArrayBuffer` rejects 2^53 and Infinity at once, as §25.1.4.1
  says (V8 reads `maxByteLength` and the prototype first and then
  rejects the length); `resize` converts the length before it checks for
  a detached buffer (§25.1.6.6; V8 checks the buffer first); the NaN that
  an operator makes can have other bits (`0 / 0` is 0xFFF8... from the
  hardware's division, V8 folds it to 0x7FF8...; V8 has its own NaN for
  `undefined`); the specification leaves NaN bits to the
  implementation. `console.log` of a `DataView` whose buffer is
  detached or that is out of bounds prints `(detached)` or
  `(out of bounds)` (Node.js 22 fails with an internal assertion).

- Typed arrays (feature 11b; `object/typed_array.rs`,
  `vm/typed_array.rs`, `builtins/typed_array.rs`,
  `typed_array_create.rs`, `typed_array_proto.rs`). One object kind,
  `ObjectKind::TypedArray`, holds the slots of §23.2.7: the `BufferView`
  of 11a (buffer, byte offset, a fixed byte length or `None` when
  `[[ByteLength]]` and `[[ArrayLength]]` are `auto`) and the element type,
  which gives `[[TypedArrayName]]` and `[[ContentType]]`. The slots never
  change, so a copy of them stays right while script code runs; the
  object traces its buffer. The elements are little endian. The value
  representation is unchanged (section 7).
- The exotic internal methods (§10.4.5): a canonical numeric key (an
  array index, or a string for which `ToString(ToNumber(s))` is `s`, or
  "-0") is an element or no property and never a stored one, so the
  object's own element storage stays empty and `"-0"`, `"1.5"`, `"NaN"`
  never walk the prototype chain. `CanonicalNumericIndexString` parses
  only strings that start like a Number's text. The heap answers all
  that runs no script code (`[[GetOwnProperty]]` and `[[Get]]` of the
  Number types, `[[HasProperty]]`, `[[Delete]]`, `[[OwnPropertyKeys]]`,
  `[[PreventExtensions]]`, which refuses for an array on a resizable
  buffer, the checks of `[[DefineOwnProperty]]`); the conversions of
  `TypedArraySetElement` (`ToNumber`, `ToBigInt`) and the elements of the
  `BigInt` arrays (new `BigInt`s) go to the runtime through the
  `Generic` results of the walks, as for proxies. The value is converted
  first and written only if the index is still valid then.
- Speed: the walks of `[[Get]]`, `[[Set]]` and `[[HasProperty]]` test one
  predicate (`ObjectKind::walks_exotic`, `sets_exotic`: the variants
  `StringWrapper`, `Proxy` and `TypedArray` are adjacent, one comparison)
  and handle these kinds out of line; `[[Set]]` keeps its fast path of an
  own writable property in a short function and walks out of line. A
  read `ta[i]` with an integer index takes the element in
  `get_with_receiver`, a write of a Number (or a `BigInt` to a `BigInt`
  array) in the fast path of `[[Set]]` after the dense write fails. A
  read of `ta.length` whose getter is the built-in one gives the length
  without the call. Callgrind against cc701dd: an `Array` element read
  loop +0.7 % instructions, an element write loop +0.7 %, a
  read-modify-write loop +0.7 %, `o.x = o.x + o.y` +0.3 % (first
  versions were up to +7.7 %: the check of the kind before the tail call
  of `named_property` must not re-wrap the result, and the typed paths
  must stay out of line).
- Creation (§23.2.5): the steps run in the specification's order (the
  `prototype` of `new.target` before the source is read); the new array
  gets its buffer when it is made and its elements are converted one by
  one into it, which no script code can reach before the constructor
  returns. An iterable source goes through `IteratorToList` into an
  internal array (no prototype) first, as the specification says; an
  Array Iterator with the built-in `next` over an array without holes is
  copied at once. `TypedArraySpeciesCreate` with the default
  constructor (the `constructor` and the inherited `@@species` getter of
  `%TypedArray%` are the built-in ones) makes the array without the call;
  `Runtime::species_constructor` now recognizes the built-in getter
  anywhere on the constructor's chain (also for subclasses of
  `ArrayBuffer`).
- Copies: `set` of the same element type copies the bytes (`memmove`
  within one buffer), of another type converts element by element (from
  a copy of the source bytes when both views share a buffer:
  `CloneArrayBuffer`); `slice` copies forward byte by byte within one
  buffer, as the specification says (a species view of the same buffer
  sees the bytes it wrote; measured in Chromium). Bulk copies and
  conversions charge one step per 128 bytes; loops over array-like and
  iterable sources, `from` with a mapping function and `set` from an
  array-like take one step per element.
- Limits: a byte length above V8's 2^35 − 1 is "Invalid typed array
  length: N" (measured: `new Float64Array(2 ** 32)` fails,
  `new Uint8Array(2 ** 35 - 1)` is "Array buffer allocation failed");
  offsets and lengths up to 2^53 − 1 are checked with checked arithmetic.
  `Object.keys` and the other key lists of a large array reserve their
  size first and end at the heap limit.
- Messages are V8's (measured in Chromium 148, `tools/probes/js-typed.json`),
  which differ between methods for the same check (`set` and the
  iterator methods "this is not a typed array.", `slice` and `subarray`
  "Method … called on incompatible receiver"; `keys` "on a detached
  ArrayBuffer", `values` "on a detached or out-of-bounds ArrayBuffer").
  A typed array's text in messages is `[object Uint8Array]` (an own
  `@@toStringTag` data string in place of the name, `#<Constructor>` when
  the chain does not reach a `%TypedArray.prototype%`); objects that
  inherit from `%TypedArray.prototype%` are `[object Object]`, as for
  `RegExp.prototype`. "Cannot set property x of Y which has only a getter"
  names the object that holds the accessor (`#<A>` for an instance of a
  subclass of `A`; measured), not the receiver. `Object.freeze` and
  `Object.seal` of an array with elements: "Cannot freeze array buffer
  views with elements"; of an array that tracks a resizable buffer:
  "Cannot freeze".
- Differences from Chromium 148 (KNOWN; 897 of the 901 measured snippets
  are a test, 4 are these): `Object.isSealed`
  of a non-extensible typed array with elements is false (the elements
  are configurable, §10.4.5; V8 answers true); an Array Iterator whose
  step threw stays finished (GeneratorStart; V8 lets the iterator of a
  typed array go on after its `TypeError`); `subarray` of a
  length-tracking array with `end` undefined passes two arguments to the
  species constructor (§23.2.3.30; V8 passes a third, `undefined`);
  `new T(typedArray)` reads the source's length after the `prototype`
  getter of `new.target` (§23.2.5.1 step 6.b; V8 reads it before, so a
  getter that shrinks the source makes V8 copy elements past its end; 8
  lines of the callback cases of `typed_hostile.rs`). Found in the
  review, not in the snippets: a replaced `%TypedArray%[@@species]`
  getter runs (`SpeciesConstructor`, §7.3.22); V8 does not call it.
- Copies of 0 bytes check no range: a conversion in `slice` can shrink
  a length-tracking array to no element after the new array exists, and
  the copy then has nothing to do (the review found an internal error
  there; `copy_between_buffers` returns at once for 0 bytes).
- `console.log` prints a typed array as Node.js 22 does
  (`Uint8Array(3) [ 1, 2, 3 ]`, `X(2) [Uint8Array] [ 0, 0 ]` for a
  subclass, `[Uint8Array(2): null prototype]`, `5n`, `Uint8Array(0) []`
  when detached or out of bounds, the other enumerable properties after
  the elements, at most 100 elements), without the columns of arrays of
  more than six elements (as for arrays).
- The methods of `%TypedArray%.prototype` (feature 11c; the callback
  methods are `builtins/typed_array_callback.rs`, the readers
  `typed_array_search.rs`, the writers `typed_array_change.rs`, the sorts
  `typed_array_sort.rs`; `typed_array_method.rs` has what they share).
  Each method follows its algorithm in §23.2.3: `ValidateTypedArray`
  first, then the length is read once, and the loops go up to that
  length. A callback or a conversion can detach, shrink or grow the
  buffer; the element of a step is `Get(O, k)`, which is `undefined` for
  an index that is no longer valid (the loop goes on), `indexOf` and
  `lastIndexOf` skip such an index (`HasProperty`), `includes` reads it as
  `undefined`, `fill` and `copyWithin` read the length again after their
  conversions (and throw when the array is out of bounds), `with` fills
  the elements that are gone with `Set(A, k, undefined)` (NaN or 0, a
  `TypeError` for a `BigInt` array). `map`, `filter` use
  `TypedArraySpeciesCreate`; `filter` keeps the selected elements as
  element bytes in a vector (as many bytes as the receiver has at most;
  runs of `undefined` entries are kept as ranges) until the species
  constructor ran. `toReversed`, `toSorted`, `with` use
  `TypedArrayCreateSameType` (the intrinsic prototype of the current
  realm, no species lookup). Every callback step runs in a handle scope
  of its own (a `BigInt` element is a new heap value) and takes one step
  of the time countdown; `reduce` keeps the accumulator in a cell.
- Native loops without callbacks work on the bytes of the buffer
  (`object/typed_window.rs`: the window of the elements for the length
  now, `typed_fill`, `typed_reverse_pairs`, `typed_write_bits`,
  `typed_put_bytes`) in chunks of 16,384 elements with a charge of the
  time countdown after each chunk (one step per 128 bytes), so a loop
  over 64 MiB ends near the deadline; a single `memmove` (`copyWithin`,
  the copy of `toReversed`, `with`) is charged before it. The searches
  turn the search value into a needle once (the bits of an integer or
  `BigInt` element that it equals, a Number for the float types, or none:
  a Number that no element of the type can hold ends the search at once)
  and compare bits; `Float16Array` and `Float32Array` compare the decoded
  value. `join` formats the elements in blocks of 4,096 into a `Builder`
  (the length limit before the heap limit, the lower bound of the final
  size reserved first), without a string per element; `toLocaleString`
  calls the `toLocaleString` of each element with no arguments (no
  ECMA-402, as `Array.prototype.toLocaleString`). `toString` is the
  function object of `Array.prototype`.
- `sort` and `toSorted`: the default order is numeric (−0 before +0, NaN
  last). One-byte types, and two-byte types of 262,144 elements or more,
  are counted into a table (256 or 65,536 counters of 64 bits, no memory
  per element; a float type counts its NaNs in a first pass and keeps
  those bits only, 2 bytes each). The other types turn the elements into
  keys of 32 or 64 bits (a two-byte type uses 32) whose unsigned order is
  the numeric order (an integer biased by its sign bit, a float with its
  sign handled), sort runs of 16,384 keys with `sort_unstable` and merge
  them bottom-up with a charge of the time countdown in each chunk. The
  keys have one slot per element: the keys of the numbers fill the vector
  from the front, and the bit patterns of the NaNs from the back (a NaN
  fits a slot), so the NaNs cost no memory of their own; only the front
  is sorted, and the NaNs are written after the numbers as they were (V8
  keeps their bit patterns; measured). The threshold is a measurement:
  clearing and scanning 65,536 counters costs about 80 to 130 µs per
  sort, and the key path is faster up to about 260,000 random elements
  (8 elements, 20,000 sorts: 1.65 s with the table, 13 ms with keys). The
  keys and the merge buffer are reserved first
  (a `RangeError` at the heap limit). With a comparator, the values are
  read first into an internal array (rooted by the scope), and a
  top-down merge sort of a permutation (split in the middle, the left
  half the shorter, the left element wins unless
  `comparator(left, right)` is above zero; NaN is +0) calls the
  comparator in the order of Chromium 148 (435 measured call sequences
  for sizes 2 to 100 and eight comparators, among them `() => 1`, which
  reverses, and `() => -1`, which keeps the order). `Array.prototype.sort`
  calls `comparator(later, earlier)` (module `array_sort.rs`); the two
  orders differ and each is V8's for its method. The sorted values are
  written back with `Set`, which skips an index that the comparator made
  invalid.
- Receiver and callback checks use V8's messages, which differ between
  the methods (measured): `at`, `join`, `sort`, `toReversed`,
  `toSorted`, `toLocaleString` say "Method %TypedArray%.prototype.NAME
  called on incompatible receiver X", the others "this is not a typed
  array."; a detached or out-of-bounds array is "Cannot perform
  %TypedArray%.prototype.NAME on a detached or out-of-bounds ArrayBuffer"
  with `findIndexLast` for `findLastIndex`, no "or out-of-bounds" for
  `reduce`, and no "or out-of-bounds" for `fill` and `copyWithin` after
  their conversions; a callback that is not callable is "number 5 is not
  a function" except for `map` ("5 is not a function"); `sort` and
  `toSorted` check the comparator before the receiver. `copyWithin` cuts
  the count to what is left of the source and the target after the
  conversions (V8 and test262's `coerced-target-start-end-shrink.js`; the
  byte loop of the specification text copies nothing for an overlapping
  copy that has to start past the limit).
- `Atomics` (§25.4; `builtins/atomics.rs`) on integer typed arrays over
  `ArrayBuffer`s: every operation is a read, a modification and a write
  of the element (no other agent sees the buffer). The steps are those of
  `AtomicReadModifyWrite`: `ValidateIntegerTypedArray` (for `wait`,
  `waitAsync` and `notify` an `Int32Array` or a `BigInt64Array`),
  `ValidateAtomicAccess` (`ToIndex`, "Invalid atomic access index"), the
  operand (`ToIntegerOrInfinity` and the modular conversion, or `ToBigInt`),
  `RevalidateAtomicAccess` and the operation; `store` returns the
  converted operand (not wrapped), `compareExchange` compares the element
  bits. Chromium 148 has no `SharedArrayBuffer` on a normal page, so
  `wait` and `waitAsync` always end in "X is not a shared typed array."
  (after the checks of the array; a detached array says "Cannot perform
  Atomics.wait", also for `waitAsync`), `notify` returns 0, and `pause`
  (Chromium 148 has it) checks that its argument is undefined or an
  integral Number. V8 words the errors of a detached array in
  `compareExchange` with `Atomics.store`; swb does the same. The key
  order of `Atomics` is Chromium 148's.
- The base64 and hex methods of `Uint8Array` (`builtins/typed_array_base64.rs`;
  the text of the proposal <https://tc39.es/proposal-arraybuffer-base64/>,
  no library read): a decoder that follows `FromBase64` step by step as a
  state machine (so it runs in blocks of 16,384 steps with a charge of the
  time countdown), `FromHex`, and encoders that write Latin-1 text into a
  vector in blocks of 12 KiB. `setFromBase64` and `setFromHex` write the
  bytes that were decoded before a failure, then throw. The output
  decoders grow their output in blocks (at most 48 KiB per step of the
  loop, each growth reserved first), so the memory follows the decoded
  bytes and not the length of the string; the encoders reserve their
  exact output first (a `RangeError` at the heap limit; "Invalid string
  length" for an encoded text above the string limit). The
  messages are V8's: the three `SyntaxError`s of base64 (a bad character,
  a single character left, non-zero padding bits; V8 words a partial
  chunk in strict mode like a single character, and `A=` like a single
  character but `A===` like a bad character), one for hex (odd length and
  a bad pair), "input argument must be a string", "invalid option X" and
  `invalid_argument` for options that are not an object. A receiver that
  is another typed array is shown as `undefined` in "called on
  incompatible receiver".
- Differences from Chromium 148 (KNOWN; 8,263 snippets of
  `typed-method-snippets.txt` are a test, 81 snippets and 8 lines of the
  callback cases are these; the groups `m-known-*` hold the snippets): `toLocaleString` of an element
  gets no arguments (V8 passes `locales` and `options`, ECMA-402);
  `Atomics.isLockFree(1.5)` is true (`ToIntegerOrInfinity`; V8 false);
  `setFromBase64` into a target that is too small stops when the output
  is full (the proposal text and the specification of hex; V8 goes on
  reading and throws a `SyntaxError` for an error after that point, for
  base64 only); `map` and `filter` check the content type of the
  species result (§23.2.4.1; V8 throws only when a conversion fails, so
  `map(() => false)` into a `BigInt64Array` works there); a `map` whose
  species constructor returns the receiver, which a callback then
  detaches or shrinks, ignores the writes to the invalid indices
  (TypedArraySetElement; V8 throws a `TypeError`; 8 lines of the callback
  cases); the text of an object in a message is `#<Object>` where V8
  shows `[object Object]` (an object with its own `toString`, the instance
  of a function without a name) and `[object Int32Array]` where V8 shows
  `[object Object]` (a length-tracking array in the message of
  `Atomics.wait`).
- Tests: `tools/probes/js-typed.json` has the 11b cases and, for 11c,
  `crates/js/tests/data/typed-method-snippets.txt` (the methods with
  every element type, receivers, callbacks, species, arguments that
  change the buffer, the calls of the comparator, `Atomics`, base64 and
  hex with options and malformed input) and `typed_methods_detach.js`
  (callbacks that detach, shrink, grow or collect, 648 lines); the
  generator `gen-typed-cases.js` writes `typed_methods_cases.*` and
  `typed_methods_detach.expected` from the probe output. test262 adds
  `built-ins/TypedArray/prototype/{at … with}`, `toString`, `Atomics` and
  `Uint8Array` (the runner skips the tests that make a
  `SharedArrayBuffer`, and loads `testAtomics.js`).
- `Date` (feature 12a; ADR 0027; `date/`, `builtins/date*.rs`): the
  object kind `ObjectKind::Date(f64)` holds the time value (no handles to
  trace); `Date.prototype` is an ordinary object, with Chromium 148's key
  order and `toGMTString` the same function as `toUTCString`. The calendar
  is integer arithmetic over the whole range; `MakeDay` computes years up
  to 10^12 away exactly. The setters read the time value first, convert the
  arguments in order, and only then test for NaN (ES2025). The text of the
  parse is read in place (`Str16`), without a copy. The clock and the zone
  are fields of the `Vm` (`TimeHost`), set by `Runtime::set_clock` and
  `set_time_zone`; the system zone is found at the first use.
  `OrdinaryToPrimitive` is a function of its own (`@@toPrimitive` of `Date`
  calls it). `console.log` of a Date is the ISO string as in Node.js 22.
- 12a tests: 11,670 Chromium 148 results in seven zones
  (`builtins_date.rs`; equal in normal and stress mode, 336 KNOWN and 1,386
  `!later` snippets are probed, not run), `date_hostile.rs`, the unit tests
  of `date/`, and test262: `built-ins/Date` 583 of 583 that run (11 need
  `Temporal` or `cross-realm`), `annexB/built-ins/Date` 24 of 24, and the
  155 tests of other directories that failed with "Date is not defined"
  (runtime 39,515 → 40,286 pass, 208 → 44 fail; parse-only 42,240 as
  before; the stress run has the same counts). KNOWN differences from
  Chromium 148 (ADR 0027 section 8): fractional `getTimezoneOffset`, the
  setters' time value read before the conversions (`setFullYear`,
  `setYear`), `Date.parse` rejects days beyond the month and reads back
  `toString` for the years before 100, `MakeDay` beyond 10^6 years, no
  `toTemporalInstant`, the locale arguments of `toLocale*String`. LATER
  (12b): Chromium's other date forms and the long zone names.
- Inline caches (feature 13a; `object/cache.rs`, `vm/inline_cache.rs`,
  the global entries in `vm/global.rs`; memo 3.3). Every named access,
  global access and object literal has a site of its own (the compiler
  shares the key texts, not the sites; the write of a compound assignment
  gets a second site). A code object keeps one entry per site, a
  `SiteCache` (28 bytes, `Copy`) in a `Cell`, in a table that the first
  fill allocates and charges to the heap (most functions of a page never
  run). No `unsafe`, no `RefCell` on the hot path. The entries are
  monomorphic: a miss overwrites the entry; a miss that gives no entry
  (a proxy on the chain, a dictionary receiver) keeps the old one.
  - Entry kinds: an own data property or accessor of receivers with a
    shape (shape, slot); a data property or accessor on the prototype
    chain, or no property at all (shape, holder, slot, epoch); a write
    that adds a property (old shape, new shape, epoch); `length` of an
    array (the kind alone); `length` of a typed array through its
    accessor (shape, holder, slot, epoch: the interpreter computes the
    length when the getter is the built-in one, without the walk and the
    call); a proxy receiver (straight to the generic path, without the
    walk of a miss); a global lexical binding (realm, position); a
    property of the global object or of its chain (realm, number of
    lexical bindings, holder, slot, epoch); the final shape of an object
    literal (prototype, shape).
  - Receivers: an entry stores only shared shapes. A shape handle carries
    the generation of its slot, so a shape in a reused slot never matches;
    the new shape of an add entry and the shape of a literal are checked
    alive before use. Values are read from the slots at each hit, so a
    value written in place (also a getter replaced with the same
    attributes) needs no invalidation.
  - Validation of prototype hits: a watch epoch (a `u32` counter of the
    heap) instead of the shape of each object on the path. Every object
    that is or was the prototype of a shape is *watched*: the flag is set
    when its root shape is made and when `[[SetPrototypeOf]]` puts it into
    a dictionary shape; each realm's global object is watched from its
    creation. Adding, removing or redefining (other attributes) a property
    of a watched object, a change of its prototype, and an add through a
    cached transition increment the epoch; entries of the chain store the
    epoch of their walk. While the epoch is unchanged and the receiver has
    the cached shape (which fixes its prototype), every object on the path
    has the keys, attributes and prototype of the walk, so the holder and
    the slot are the same; dictionary prototypes are covered as well. The
    cost: a change of any prototype or global object invalidates all chain
    entries (a refill each); scripts change prototypes at start-up, not in
    loops. Reasons against the per-object shapes: an entry of unbounded
    size, and dictionary prototypes (shapes changed in place) would never
    hit. At `u32::MAX` the epoch stays and no chain entry is made any more.
    A fill checks the flag of every prototype on the path as a guard.
  - Kinds: the shape does not fix the kind (an array and
    `Object.create(Array.prototype)` share the root shape). Proxies and
    host objects never match; a typed array matches a chain entry only
    when the walk met a typed array with the key (so the key is not a
    canonical numeric string); `length` is cached through the chain only
    for typed-array receivers (an array's `length` is not in its shape).
    String objects, arguments objects and arrays are ordinary for the
    non-index keys of named sites; index keys (`o[0]` with a constant key)
    are never cached.
  - Writes: an own writable data property, an own or inherited setter,
    and adds. An add entry needs an extensible receiver at the hit (the
    flag is not in the shape), no setter and no read-only property with
    the key on the chain (the epoch), and never `length`; it pushes the
    value and moves the object to the cached shape (a growth of the slots
    can collect; the new shape is checked alive afterwards, else the
    general path). Everything else (`freeze`, a read-only own property,
    array `length`, a proxy target) takes the general `PutValue`.
  - Misses walk once: the walk that gives the result also gives the entry,
    so a polymorphic site costs about what a site without a cache did.
  - Global cache: `GetGlobal` and `SetGlobal` name a site. A lexical
    binding keeps its position for the life of the realm (bindings are
    never removed; the record is a vector with a name index), so its entry
    needs no invalidation; a binding in its dead zone and a constant are
    misses (the general path throws). A property entry is valid while the
    realm has the same number of lexical bindings (a later script's
    `let` can shadow the global property) and the epoch is unchanged
    (`delete`, a sloppy assignment that creates a global, a `var` of eval
    code, a redefinition, the dictionary mode of the global object). Code
    in the reach of `with` and of eval vars checks those records first
    and reaches the global site only after them.
  - Object literals: a literal whose properties are all data properties
    with constant keys that are not array indices (`a: v`, shorthands,
    methods; repeated keys keep the first position) and at most 128 keys
    compiles to `NewLiteral` (the object in its final shape, cached per
    site and realm by its prototype, with `undefined` values) and one
    `InitSlot` per property. Nothing can observe the object before its
    last value is stored (a method's home object cannot run before the
    literal ends). Literals with `__proto__: v`, computed keys, spread or
    accessors keep `NewObject` and the definitions.
  - Measured (vm_bench, best of 5, release, same session, base e9badd9
    against 13a; the table is in `docs/performance.md`): `fib(27)` 63 →
    42 ms, `o.x = o.x + o.y` 5M times 367 → 135 ms, 1M small objects
    239 → 76 ms, the `for` loop over 1M elements 124 → 65 ms, the sum of a
    `Float64Array` 182 → 88 ms, the array read-modify-write 133 → 73 ms;
    the geometric mean over all 95 programs 0.86. Code objects grow by the
    sites of their own (one key per access, 8 bytes) and, once a function
    runs, by its cache table.
  - Tests: `tests/inline_caches.rs` (the invalidation cases, normal and in
    the GC stress mode, expected outputs from Node.js 22), the unit tests
    of `object/cache.rs`; test262 unchanged (40,286 pass, 44 fail, normal
    and stress; parse-only 42,240).
- Call paths (feature 13b; `vm/call.rs`, `object/arguments.rs`,
  `heap/roots.rs`, `value.rs`). Measured first with callgrind on
  `fib(27)` (1,089 instructions per call, of them about 450 in the call
  and the return) and the closure counter (1,149 per iteration). The
  largest costs and what replaced them:
  - Copies of values: the default layout of the `Value` enum put the
    payloads at bytes 1, 4 and 8, and the interpreter copied a value in
    five pieces at each register read and write. An 8-byte tag
    (`#[repr(u64)]`) makes it two moves: `fib` 8 % fewer instructions,
    `Date.now()` 12 %. `#[repr(C, u8)]` (payload at byte 8, 1-byte tag)
    gained nothing. The 8-byte tag leaves no room beside the niche for
    the 12-byte key variant of `Root` (24 instead of 16 bytes, which made
    a test with a million handle-scope values reach the heap limit), so
    a property key is now recorded as its string or symbol value; a test
    keeps `Root` at 16 bytes.
  - Frame setup: `invoke` reads the code, the `this` mode and the realm
    in one lookup of the callee (was two); the `this` of a sloppy
    function is bound in place (was a call); the frame limit and the
    stack limit are one check; constructor frames have their own
    function. A return drops the frame record in place instead of
    moving it out of the vector through an `Option` and a `Result` (about
    60 instructions of copies per return).
  - Checks: `gc_due` at every call and backward jump compared the
    allocation with three rules (threshold, limit, near the limit) in
    about 25 instructions; the rules are now folded into one bound that
    is recomputed when the live size changes (after a collection,
    `release_bytes`), and the check is one comparison and a flag. A unit
    test compares the bound with the rules at their edges. The countdown
    step at function entry and the frame limit are unchanged (section 9).
  - Native calls: a handle scope was two vector pushes (start, id) and
    two pops; it is one vector of (start, id) pairs. The deferred-call
    and resume results of natives go to an out-of-line function, so the
    common `Value` result returns directly. The native function signature
    is unchanged. `Date.now` converts the clock without the 128-bit
    milliseconds of `Duration::as_millis` (a library call).
  - Arguments objects: the first object of a realm (sloppy: `callee` a
    data property; strict: the `%ThrowTypeError%` accessor) is made with
    the general definitions; its shape and named slots (`length`,
    `@@iterator`, `callee`) become a template of the realm (a root of the
    realm). Later objects are made in the template's shape with the
    elements in one dense vector (the cells of mapped parameters, the
    values otherwise): 7,414 → 2,940 instructions per call of `f(a, b)`
    that reads `arguments`. The rest is the allocation of the object, its
    two vectors and the two parameter cells, the collection of them, and
    the generic `[[Get]]` of `arguments[0]`.
  - Kept: the register clearing (`resize` to the register count, about
    16 instructions for 6 registers), the `Rc` count of the code in the
    frame and the loop (4 updates per call), the 128-byte frame record.
  - Layout of the loop (the 7d note: `fib` 8 % slower at 3 % fewer
    instructions): five builds with different alignments of functions,
    loops and jump targets (`-align-all-functions`, `-align-loops`,
    `-align-all-nofallthru-blocks` of 16 and 32 bytes) gave `fib` 29.5 to
    30.4 ms and six other programs within 2 %. A build whose `match`
    kept only about 40 frequent instructions (the others an error; 35 KB of
    machine code instead of 109 KB) ran 3 to 7 % slower with 3.5 % more
    instructions. So the size of the `match` is not the cause, and no
    layout attribute is needed; the loop stays as it is. The 7d effect
    did not reproduce on this build.
  - Measured (vm_bench, best of 5, two alternating runs, base bfe3560;
    `docs/performance.md`): `fib(27)` 42.6 → 29.8 ms, closure counter
    64.2 → 47.4, calls that read `arguments` 513 → 214, that write mapped
    arguments 507 → 212, `new Function` code 63.7 → 45.1, `push`/`pop`
    136 → 107, `slice`/`charCodeAt` 267 → 232, `Date.now` 83.2 → 80.2,
    `getInt32` 87.4 → 84.6; geometric mean over 95 programs 0.85. The
    native rows gain little: the call costs about 100 instructions now,
    the bodies of `getInt32` (about 300) and `Date.now` (about 200, most
    of it the system clock) dominate.
  - Tests: `tests/calls.rs` (arguments objects after the template,
    mapped and unmapped aliasing, `this` binding, call errors, deep
    recursion, generators and async functions that use `arguments`;
    normal and stress; expected outputs from Node.js 22); test262
    unchanged (40,286 pass, 44 fail, normal and stress; parse-only
    42,240).
- Callbacks from natives (feature 13c; `vm/native_loop.rs`; memo 1.2,
  "resumable native functions"). Measured first with callgrind on
  `forEach` over 200,000 elements with an empty callback: 1,397
  instructions per element, of them 686 in `Runtime::call` (the re-entry:
  `push_window` 107, `invoke` and the frame 217, `run_frames` and the
  entry and exit of a new `execute` 283 for a body of two instructions)
  and 710 in the element loop (460 of them the search of the prototype
  chain for the next stored index, which a dense array without holes
  does not need).
  - Alternatives. (1) A cheaper re-entry: the entry of a new `execute`
    and `run_frames` is the part that only a nested loop has; even with
    `push_window` and the bookkeeping of `call` cut, a re-entry stays
    above 500 instructions per call, and it keeps the recursion limit of
    about 400 levels of the budget. (2) Self-hosted built-ins (memo 1.2):
    the callbacks would be script calls, but every built-in would need a
    JavaScript version with the exact order of observable steps and V8's
    messages, and the bodies would run slower in the interpreter than in
    Rust. (3) Native loops (chosen): the native returns its state, a
    step returns "call this" or "done", and the call is a frame of the
    running interpreter loop. Measured with the first version: 1,129
    instructions per element (the state was rooted by recording its
    handles in each step's handle scope, and a step returned its call as
    a 100-byte value); with registers on the value stack and the window
    pushed by the step, 893.
  - The mechanism: `NativeReturn::Loop` carries the state (a boxed
    `NativeLoop`) and the index of its registers. `Runtime::start_loop`
    pushes a `LoopRecord` (the state, the registers, where the result
    goes, the stack length to restore, the number of frames below it)
    and runs steps until one calls a script function. A step gets the
    last call's result (recorded in the step's handle scope), pushes the
    next call's window (`push_call`) and returns `LoopStep::Call`, or
    returns `Done(value)`. The call runs with `ReturnTo::Native`: its frame's return delivers to the
    top record, whose next step runs (`resume_loop`). A native callee
    that returns at once continues the loop without a frame. A loop
    whose result goes to the loop below it continues that one in the
    same Rust loop, not by recursion.
  - GC: the state keeps its heap values in registers, a window of the
    value stack above the native's arguments (the copies in Rust fields
    are only valid because the registers hold them); the state itself
    holds no other handles, so a step needs no rooting of its state and
    the records need no tracing.
  - Exceptions: `run_frames` gives an exception that leaves the frames
    above the top record to the record's `on_error` before it searches
    the handlers below (the default passes the exception on). Records
    belong to the run of
    the interpreter whose frames they wait for: a record whose step runs,
    or whose call of a native callee runs in Rust, is below any run that
    the step or the callee starts (re-entry), so `catching_loop` and
    `drop_loops` skip it. Found by `Promise.prototype.finally` in a job
    loop of a first version (its closure calls the callback through
    re-entry, and the exception reached the loop of the outer run).
  - Limits: a step ticks the countdown; the frame of a script callee
    checks the frame limit, so a recursion through callbacks ends at the
    frame limit (two frames per level: 4,999 levels of
    `f → forEach → callback → f`; before, 408 levels of the recursion
    budget; Node.js 22 2,789 for `forEach`, 1,743 for `sort`, 705 for
    `replace`). The start of a native callee from a step charges the
    recursion budget (a native callee can start a loop of its own in
    Rust).
  - Converted: `Array.prototype` `forEach`, `map`, `filter`, `every`,
    `some`, `reduce`, `reduceRight` and the `find` family (one state
    machine; an element in the receiver's dense vector is the next one
    without the chain search, which also speeds up the loops that still
    call `for_present`); the typed-array equivalents; `sort` and
    `toSorted` with a comparator (the merge sort as a state machine,
    `SortMachine`, which makes the comparisons of `merge_sort` in the same
    order, checked by a unit test with random and inconsistent answers);
    `RegExp.prototype[@@replace]` with a function (after the collection
    of the matches) and `String.prototype.replace` and `replaceAll` with a
    function.
  - Measured and not converted: a proxy `get` trap as a native loop (two
    steps, a boxed state per access) was 36 % slower than the re-entry
    (133 → 181 ms for 1M reads): the fixed costs of a loop with one call
    exceed a re-entry. The consumers of iterator helpers (`reduce`,
    `forEach`, `some`, `every`, `find`) as loops whose steps pull the
    next value through the helpers (which still re-enter for the
    mapper and for a script's `next`) were 10 % slower in wall time at
    1 % more instructions: cachegrind showed 70 times more instruction
    cache misses, from the re-entries inside a step of the outer loop.
    The promise job queue as one loop (the handlers of reaction jobs as
    frames, the resumption of an `await` as a frame through a step that
    resumes a generator) gave no gain on `then` (64 ms both) and made
    100k `await`s 10 % slower (+5 % instructions): one job is one call,
    and a step costs about what a re-entry costs. These stay on the
    re-entry path; iterator helpers need their own resumable steps (a
    later feature).
  - Iterator results: `CreateIterResultObject` makes the first result of
    a realm with the general definitions and keeps its shape (`value`,
    `done` on `%Object.prototype%`); later results are made in that shape
    with both slots at once. The results of `RegExp` exec already had
    fixed shapes (8b).
  - `for`-`of` fast paths (`IteratorCall`): the record's `next` is read
    once by `GetIterator`, as before; when it is the built-in `next` of
    the iterator's kind (compared with the intrinsic of the function's
    realm), the step needs no result object. Since 6b this held for
    Array, Map and Set Iterators and iterator helpers; 13c adds String and
    `RegExp` String Iterators (a step without the result object, as
    `next` does it) and generators: the step resumes the generator's
    frame directly with `ReturnTo::IterValue`, and its `yield` puts the
    value into the loop's register and marks the step done or not in the
    record's state register, without the result object; a return marks
    it done; a `yield*` that passes a result object of its delegate gives
    that object to the general check (its getters run). A patched
    `%GeneratorPrototype%.next`, an own `next` of the generator, or an
    async generator take the general path.
  - Interpreter: the arms that can remove the top frame (`Return`, the
    yields, the async instructions) no longer write the pc back after an
    error, because a native loop that resumed there can fail after the
    frame went (the pc went into the frame below before). `deliver` is
    fallible and stays inlined for registers; the native loop and the
    `for`-`of` step are out of line.
  - Measured (vm_bench, best of 5, two alternating runs, base 43e5f3e;
    `docs/performance.md`): `forEach` 66 → 51 ms, `reduce` 71 → 49,
    `filter` 77 → 57, typed `map` 82 → 70, `sort` with a comparator 89 →
    80, `matchAll` 150 → 88, `for`-`of` over a generator 266 → 69, `for
    await` over an async generator 66 → 58 (iterator results); `replace`
    with a function, the iterator helper and promise rows unchanged; the
    rows of helpers that call script functions 2 to 6 % slower (1 % more
    instructions on the re-entry path). Geometric mean over 95 programs
    0.97.
  - Tests: `tests/callbacks.rs` (exceptions in callback loops with
    their stack text, callbacks that change the array, re-entrant calls of
    the same native, natives as callbacks that re-enter and throw, deep
    recursion through callbacks, a time limit inside callback loops,
    generators and async functions as callbacks, patched `next` methods
    and `return` on early exits of `for`-`of`; normal and stress;
    expected outputs from Node.js 22), the unit test of `SortMachine`;
    test262 unchanged (40,286 pass, 44 fail, normal and stress;
    parse-only 42,240).
- Strings, collections, arrays and numbers (feature 13d). Measured with
  callgrind on the string rows first (instructions per iteration, base
  d36be4f): `slice` and `charCodeAt` 3,620; `replace`/`trim`/`padStart`
  9,147; case conversion of 4 strings 7,621; `split` 3,057; two
  `indexOf` 3,645. The costs were: the read of a method on a primitive
  string (about 290 instructions for each of 3 reads: the general
  `[[Get]]` through the String prototype, a shape search), the copy of
  a short text (a vector grown from empty, 276 instructions for 5
  units), `toLowerCase` pushing unit by unit, a `padStart` built from a
  rope of the repeated filler (6 allocations), `replace` growing its
  builder 3 times, the scan of `indexOf` through closures with a bounds
  check per unit, and the allocation of the result (a vector per string,
  about 220 instructions with a cold `malloc`, which stays).
  - One-unit strings: `Heap::unit_string` (a table of 256 atoms in
    `Names`, made at first use, traced by the collector). Used by `s[i]`,
    `charAt`, `at`, `fromCharCode`, `Runtime::substring` (so also the
    parts of `split`, `match`, `matchAll`, `replace`, `trim`), `split('')`,
    the String Iterator and the `[[GetOwnProperty]]` of String objects.
    As atoms they are also property keys without a table lookup.
  - Reads on primitives: a new site entry `PrimitiveOwn { shape, holder,
    slot }` for a data property of the prototype object of a string,
    number or boolean (`"abc".slice`, `n.toFixed`): valid while the
    holder is the running realm's prototype and still has the cached
    shape (`Heap::cached_primitive_get`); the value is read from the slot
    at each hit. The holder is the first object of the walk, so no
    watch epoch is needed. A property further down the chain, an
    accessor, a dictionary-mode holder and `length` and indices of
    strings give no entry; `StringLength` reads the length of a string
    value. The usual design of monomorphic caches (Deutsch and
    Schiffman); no code from other engines.
  - Smaller local fixes: `to_number` of an integer or double returns at
    once; `Str16::to_string16` allocates the exact size; Latin-1 case
    conversion maps in one pass over iterators; `padStart`/`padEnd` of up
    to 256 units build the result in one buffer; `replace` reserves its
    result; the forward search for a needle of fewer than 16 units scans
    slices; the `split` cache copies its pieces without a temporary
    vector (`Heap::new_array_copy`).
  - Not changed: the 20k appends of `'ab'` (1.7 ms): each append copies
    the right leaf (up to 1,024 units) into a new leaf, because leaves
    are immutable; the cost is the allocation. Small strings inline in
    the slot would change the representation (backlog).
  - `Map` and `Set` string keys: `JsString::key_hash`, a `Cell<u32>` in
    the padding of the record (`size_of::<JsString>()` stays 40, the slot
    48; a test checks it), set at the first `Heap::key_hash` of the
    string. It is a 32-bit part of the keyed hash of `AtomTable::hash`
    (the per-process key and the SipHash stay, so a page cannot choose
    collisions); the table needs only the low bits and compares the key
    after the hash. A flatten keeps the content, so the hash stays valid.
    Alternative measured on paper and not built: a side table keyed by
    handle (a lookup as costly as the hash for short keys).
  - `toFixed`: for at most 20 digits, `x = m * 2^e` with a 53-bit `m` and
    `n = floor((m * 10^f + 2^(-e-1)) / 2^(-e))` for `e < 0` (a tie goes
    up, as step 8 asks), `m * 10^f * 2^e` for `e >= 0`, in `u128`
    (`m * 10^20 < 2^120`); a shift of 128 or more gives 0. An overflow
    of the integer case falls back to the multi-precision path, which
    stays as `to_fixed_exact` and is the reference of the tests: a
    differential test over 300,000 random doubles of five families (any
    exponent, exponents near the rounding position, decimals with a 5
    after the last digit and their neighbours, exact binary ties) with
    0 to 100 digits is part of the test run, and an ignored test ran 30
    million (16 s in release) with no difference; fixed cases include
    halves, 1e21 and its neighbours, `-0`, subnormals and `MAX_VALUE`.
    The digits are written from `u128` in chunks of 19 digits.
  - `shift`: `DenseVec` (a `Vec<Value>` and a start `u32`) replaces the
    vector in `Elements::Dense`; it reads as the slice after the offset
    (`Deref`), so all readers and writers (the trace of the collector,
    `unshift`, `splice`, `reverse`, `sort`, length changes, holes,
    `next_own_index`, the inline caches for `length` and elements) see
    the elements only. `pop_front` advances the offset; the elements move
    down when the offset reaches the count of elements (and 16), so the
    moves of a series of shifts are at most the number of shifts; an empty
    vector resets the offset; `splice` (used by `unshift`) first moves the
    elements to the start. The offset and the count of non-holes share
    the 8 bytes after the buffer (`record_sizes` checks 88 bytes for an
    object). `Heap::shift_dense` is O(1) (the old `change_dense` also
    counted the holes of the whole vector on each call). The time charge
    of a dense `shift` is 1. The dead slots before the offset stay in
    the heap's count of allocated bytes until the elements move down, so
    a queue (`push` and `shift`) can hold about twice its live size: a
    loop of 3M `push` and 1.5M `shift` after 2M garbage objects ends at a
    heap limit of 256 MiB and passes at 300 MiB (13d review).
  - Rejected after the review: the integer form on the double paths of
    `|`, `&`, `^`, `<<`, `>>` and `>>>`. With `Value::Int` there, the
    integer paths of these operators became 25 % slower (`s = i | 0`
    over 8M iterations 110 → 138 ms) through the code of the
    interpreter loop, also with the double path in a function out of
    line; with `Value::number` the integer paths kept their speed, but
    a loop of bitwise operators on doubles was 8 % slower. The gain was
    1 % on the row that stores `x | 0` in an array and 3 % on the
    default `sort` of such values. The double paths give
    `Value::Double` as before.
  - Rejected: (1) the dense vector as a `VecDeque` (a slice view needs
    `make_contiguous`, which moves everything on each call); (2) the
    offset as a separate field of `Elements::Dense` (the record grew by
    8 bytes, `record_sizes` failed: 1M small objects would cost 8 MB
    more); (3) a cache entry for any property of the primitive's chain
    (it needs the watch epoch and a watched String prototype, which no
    String object may have made yet).
  - Tests: `tests/small_items.rs` (Node.js 22 outputs, normal and
    stress): one-unit strings, caches on primitives (changes of the
    prototype objects after the cache filled), string keys in `Map` and
    `Set`, `toFixed`, `shift` (holes, prototype elements, frozen and
    sealed arrays, getters, queues of 100,000, collections), the values
    of the bitwise operators on doubles; unit tests of `DenseVec` and
    `toFixed`; the size test of `JsString`.

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
  Lazy compilation and lazy built-ins are later work, each after a
  measurement; the inline caches came in M7 13a (section 15).
- The regular expression engine is a separate crate, so it can grow in
  parallel with the runtime (CLAUDE.md allows parallel work only in
  separate crates).
