# Study memo 1: execution

Status: reviewed by the orchestrator on 2026-10-09 (ADR 0025).

Study memo for swb's JavaScript engine (ADR 0025), group "Memo 1:
execution", questions 1 to 5. The memo describes problems, options and
trade-offs. It does not decide the design.

Engines compared: QuickJS and QuickJS-ng (one family, with differences
noted), Duktape (source and design notes), MuJS and MicroQuickJS. The
sources and versions are at the end.

## Overview

| Design point | QuickJS / QuickJS-ng | Duktape | MuJS | MicroQuickJS |
|---|---|---|---|---|
| Machine model | stack machine | register machine | stack machine | stack machine |
| Instruction encoding | variable length, 1-byte opcode | fixed 32-bit words | 16-bit words | variable length, 1-byte opcode |
| Script-to-script calls | recurse on the host stack | no host recursion | recurse on the host stack | no host recursion |
| Generators, `async` | yes, frames on the heap | no (has coroutines) | no | no |
| `finally` | subroutine call inside the function | runtime catcher with a stored completion | code copied to every exit | as QuickJS |
| Depth limit | measured host stack space | counters | fixed-size arrays | shared arena, small re-entry limit |
| Time limit | polled counter, uncatchable error | polled counter, error re-thrown at every handler | instruction budget | polled counter, uncatchable error |

Two facts from the specifications frame the whole memo:

- ECMA-262 does not define resource limits. It does not say what
  happens when the call stack, the heap or the time budget runs out. Clause
  17 lists the errors that an implementation must report and does not
  mention resource exhaustion.
- The HTML Standard, section 8.1.4.5 "Killing scripts", lets a user agent
  abort a running script. The abort empties the execution context stack
  and does not run `finally` blocks. The same section lets a user agent
  impose CPU, memory and time limits, and throw a `QuotaExceededError`,
  abort without an exception, ask the user, or throttle.

---

## 1.1 Instruction set

### Problem

The compiler translates each function into instructions for a virtual
machine. The main choice is how instructions name their operands:

- A **stack machine** takes operands from an operand stack and pushes
  results back. Instructions are short because most operands are
  implicit.
- A **register machine** names its operands explicitly as slots in the
  frame ("registers"). One instruction does the work of several stack
  instructions, but each instruction is longer.

The frame size must be known before the function runs: the maximum depth
of the operand stack, or the number of registers. The operand encoding
decides code size and decoding cost. The interpreter loop decides how
fast instructions dispatch.

### What the engines do

**QuickJS and QuickJS-ng** use a stack machine with about 250 instruction
kinds. Arguments, local variables and captured variables are addressed by
index operands; only temporaries go through the operand stack. Each
instruction is one opcode byte followed by zero or more operands of 8,
16 or 32 bits. Constants and nested functions are indices into a
per-function constant table; property names are 32-bit indices into the
engine's table of interned strings (atoms).

Each instruction kind has static metadata (length, values popped, values
pushed, operand format); one table drives the disassembler, the
stack-depth pass and the optimizer. The parser emits an intermediate
stream with pseudo-instructions for scope resolution and symbolic labels.
Later passes resolve variables to slots, resolve labels, run a peephole
optimizer, fold small operands into the opcode (small integer constants,
the first few locals, calls with zero to three arguments) and shrink jumps
to 8-bit or 16-bit offsets when the target is near.

The operand stack depth is computed after code generation by a worklist
walk over the control-flow graph. The walk starts at the entry with depth
zero and applies each instruction's pop and push counts (a call pops a
count read from its operand); each branch queues its target with the
current depth. When a position is reached again, the depth and the
enclosing exception handler must match the earlier visit; otherwise the
compiler reports an internal error. The maximum depth (at most about
65,000) becomes the frame size. The pass also checks that every jump
target lies inside the code, so it is a small verifier for the engine's
own output.

**MicroQuickJS** keeps the QuickJS model with a one-pass compiler and
fewer optimizations. It runs the same depth computation after each
function. To save memory, it stores the depth per code position in one
byte, modulo 255, which still detects nearly all inconsistencies. Property
names in the code go through an indirection table so that bytecode can
live in read-only memory.

**Duktape** uses a register machine. Every instruction is a 32-bit word:
an 8-bit opcode and three 8-bit fields, or one 8-bit field and one 16-bit
field, or one 24-bit field. The first field is usually the destination
register. For source operands, two bits of the opcode say whether each of
the two source fields is a register or a constant index, so one logical
operation uses four opcode values. The design notes say that the 64
most frequent operations sit in the main opcode space and rare ones need
a second dispatch.

Register allocation happens during the single-pass compilation:
variables that the compiler binds to registers take the low registers;
temporaries are allocated above them in strict stack order. After each
statement, the next free temporary resets to its value before the
statement. The function's register count is the highest register used.
Beyond the directly addressable range (about 256 registers or
constants), the code generator moves values through low temporaries with
extra instructions. The design notes report two costs: temporaries are
not cleared after use, so they keep garbage alive in long-running
functions; and ES2015 constructs such as destructuring need an
intermediate representation, which a single pass does not have.

**MuJS** compiles an abstract syntax tree into a stack machine. Code is
an array of 16-bit words. Each instruction starts with a word that holds
the source line, then the opcode word, then operand words; numbers and
string references are stored inline across several words. MuJS does not
compute a maximum depth. All frames share one fixed-size value stack
(4,096 slots by default), and every push checks for overflow.

**Dispatch.** QuickJS uses a `switch` statement, or computed goto when
the C compiler supports it. Duktape uses a `switch`. Duktape's notes
report that keeping the program counter in a local variable of the loop
made dispatch 20 to 25 percent faster than an index read from the call
record on each instruction. The cost: every path that can leave the loop
(a throw, a call, a side effect that inspects the call stack) must first
write the local copy back.

### Literature

- Shi, Gregg, Beatty and Ertl ("Virtual Machine Showdown", VEE 2005;
  extended in ACM TACO 2008) translated Java stack bytecode to register
  code. The register code executed about 46 to 47 percent fewer
  instructions and was about 25 percent larger. With `switch` dispatch,
  run time dropped by about 32 percent on a Pentium 4; with threaded
  dispatch, by about 26 percent.
- "The Implementation of Lua 5.0" (Ierusalimschy, de Figueiredo, Celes)
  describes Lua's move to a register machine with 32-bit instructions
  (6-bit opcode, one 8-bit and two 9-bit fields, or an 18-bit field). Push
  and pop are expensive when each copies a tagged value; register operands
  are cheap to decode. A source operand selects register or constant by a
  value threshold, the same idea as Duktape's flag bits. A compare
  instruction skips the next instruction, a jump with a wide offset, so
  compares need no wide offset.
- Ertl and Gregg (JILP 2003) found threaded code nearly twice as fast as
  `switch` dispatch on processors of that time. Rohou, Swamy and Seznec
  (CGO 2015) found that recent branch predictors (for example Intel
  Haswell) predict a `switch` interpreter almost as well.
- Crafting Interpreters (Nystrom) pairs a single-pass compiler with a
  stack machine, which is the natural match; register code needs register
  planning during parsing, which Lua and Duktape do with restrictions.

### Trade-offs

- **Instruction count versus size.** A register machine executes fewer
  instructions; a stack machine has smaller code and a simpler compiler.
  For 1 to 5 MB bundles code size matters for memory, but most of a
  bundle never runs.
- **Compiler complexity.** A stack machine needs no register allocator.
  Its frame size comes from a separate pass that also catches compiler
  bugs. A register machine gets the register count for free from the
  allocator, but needs rules for running out of registers.
- **Garbage collector roots.** With a stack machine, all slots below the
  operand stack top are live, and the precise depth is known. With a
  register machine, all registers of all frames are roots, including
  stale temporaries (the Duktape notes).
- **Fixed versus variable length.** Fixed-width words decode with a few
  shifts and have no alignment questions. Variable-length bytes are
  compact but need short forms, jump shrinking and more decoding.

### For swb

- Safe Rust has no computed goto. Options: (1) a `match` in a loop,
  which LLVM compiles to a jump table, that is `switch` dispatch, cheaper
  on current processors than older papers suggest (CGO 2015); (2) a table
  of handler functions, with an indirect call per instruction and state
  passed through arguments; (3) tail-call threading, which needs
  guaranteed tail calls, an unstable Rust feature (`become`, RFC 3407);
  (4) pre-decoded instructions, a vector of Rust enum values with typed
  operands: no decoding, but each instruction takes the size of the
  largest variant.
- Every slot access in safe Rust is bounds-checked; the checks are
  cheap, well-predicted branches, and a register machine makes fewer
  accesses. With a tracing GC and index handles, a value copy is a plain
  copy (QuickJS pays a reference-count update on every push and pop). If
  values are 16 bytes, the Lua paper's argument against push and pop
  applies more strongly than for 8-byte NaN-boxed values.
- A depth verifier pass is useful although swb never loads foreign
  bytecode: it turns a compiler bug into a compile error instead of an
  out-of-range index at run time. A failed bounds check in the loop must
  become an internal error, not a panic.
- The Duktape finding has a Rust form: keep the program counter and the
  frame base in locals of the loop, and write them back to the frame
  record before any operation that can call out or trigger GC.

---

## 1.2 Calls

### Problem

A call binds the callee, `this`, `new.target` and the arguments,
creates slots for locals and temporaries, and records where to return.
JavaScript adds `arguments`, rest parameters and default values. A
script-to-script call should not recurse in the host language, so that
the engine bounds the depth and frames can be suspended (question 1.3).
Built-ins call back into script (callbacks, getters, `valueOf`, Proxy
traps), and that nesting needs a limit.

### What the specification says

- ECMA-262 10.2.1 defines [[Call]] for ordinary functions:
  PrepareForOrdinaryCall (10.2.1.1), OrdinaryCallBindThis (10.2.1.2) and
  OrdinaryCallEvaluateBody (10.2.1.4). The execution context stack is
  section 9.4.
- FunctionDeclarationInstantiation (10.2.11) defines when an `arguments`
  object exists (not for arrow functions, not when a parameter is named
  `arguments`, not when a function or lexical declaration named
  `arguments` exists and no parameter has an initializer), whether it is
  mapped (sloppy mode with a simple parameter list) or unmapped, and when
  parameters get their own environment (when any parameter contains an
  expression, such as an initializer). Mapped and unmapped arguments objects are in 10.4.4.6 and
  10.4.4.7. Parameters, including defaults and the rest element, are bound
  by IteratorBindingInitialization (8.6.3).
- Tail calls: 15.10 defines proper tail calls in strict mode. Chromium's
  engine does not implement them, so web content does not depend on them.

### What the engines do

**Frame contents.** All engines keep the same logical content in a
frame: the callee, the `this` value, the arguments, the local variable
slots, the temporaries (operand stack or registers), the position to
resume in the caller, a link to the caller's frame, and the open captured
variables. They differ in where these live:

- **QuickJS** places the frame in the host stack frame of the interpreter
  function: one block for copied arguments (only when needed), locals, the
  operand stack, and a table of references to captured variables that
  still live in the frame. When the caller passes at least as many
  arguments as the callee declares, the callee reads them in place from
  the caller's operand stack; otherwise it copies them and pads with
  `undefined`. `this` and `new.target` are parameters of the host
  function. The current frame links to the previous one so that error
  stack traces and "current function" queries can walk the chain.
- **Duktape** gives each thread one value stack shared by all
  activations, and a separate list of activation records. The caller
  pushes the callee, `this` and the arguments; the callee's register
  window starts at the first argument, so arguments are not copied, and
  the effective `this` sits in the slot just below the window. Call setup
  pads or drops arguments to the declared count (variadic natives keep
  all), then extends the window to the register count with `undefined`.
  The activation record keeps the per-call state that is not a script
  value, such as the program counter and the active catchers. Duktape
  records positions as indices, not pointers, because any side effect can
  reallocate the value stack. Before the call, setup also resolves
  bound functions, `Function.prototype.call` and `apply`, and
  `Reflect.apply` and `Reflect.construct` in place, so these do not add a
  native frame.
- **MicroQuickJS** keeps all frames in one script stack that grows down
  from the top of the engine's single memory arena. Everything in a frame,
  including the link to the caller and the saved program counter, is
  stored as ordinary tagged values; positions are integer offsets, not
  pointers, because the compacting GC can move every heap block,
  including bytecode.
- **MuJS** has two kinds of functions. If a function contains no inner
  function, no `eval` call, no `with`, no `catch` clause and no reference
  to `arguments`, its variables live in value-stack slots. Otherwise, each
  call allocates a heap environment object and stores every variable as a
  property of it. Top-level scripts always use the heap form.

**`arguments`, rest and defaults.** QuickJS creates the
`arguments` object only when the function body refers to it: the compiler
emits a function-entry instruction that builds it from the actual
arguments. The mapped variant aliases the frame's argument slots, so a
write to `arguments[0]` changes the parameter. Rest parameters are built
by a function-entry instruction that copies the arguments beyond the
declared count into a new array. Default values compile to ordinary code
at the function start: test the argument slot for `undefined`, evaluate
the initializer, store. Duktape creates the `arguments` object and the
environment record lazily, only when needed. MuJS builds `arguments`
eagerly at entry when the compiler saw a reference.

**Calls without host recursion.**

- **QuickJS and QuickJS-ng** do not avoid host recursion. Each
  script-to-script call re-enters the interpreter function, and the
  frame lives in the host stack. QuickJS has a tail-call instruction,
  but it still makes a nested host call and only skips the caller's
  cleanup. Generators work anyway because their frames are on the heap
  (question 1.3).
- **Duktape** avoids host recursion for script-to-script calls. The call
  handler sets up the new activation and returns to the interpreter loop,
  which restarts at the callee's first instruction; a return pops the
  activation and restarts the caller. Duktape also implements real tail
  calls: the activation is reused when the compiler proves that no
  `try`, `finally` or `with` catcher is active and the target is a script
  function.
- **MicroQuickJS** avoids host recursion for script-to-script calls in
  the same way: a call pushes a frame and continues the loop.
- **MuJS** recurses on the host stack for every call.

The literature agrees: the Lua 5.0 paper calls this a "stackless"
interpreter; CPython 3.11 adopted it and reports that most
Python-to-Python calls now use no C stack; Crafting Interpreters builds
clox with an array of call frames.

**Built-ins that call back into script.** Host recursion is
unavoidable when a built-in needs a result in the middle of its work
(sort comparators, `replace` callbacks, `ToPrimitive`, getters reached
from native property access, Proxy traps, `toJSON`). The engines bound
this nesting differently:

- QuickJS measures the real host stack (question 1.5), which covers
  every kind of recursion with one check.
- Duktape counts nested native-to-script calls (default limit 1,000)
  separately from the total number of activations (default limit
  10,000).
- MuJS limits the total call depth with fixed-size arrays (1,024).
- MicroQuickJS allows only 8 levels of nested interpreter entry. It
  reduces the need for nesting with a deferred-call protocol: when the
  last action of a native function is a call to a script function (for
  example `call`, `apply`, or a getter found by a property-access
  instruction), the native code leaves the callee and arguments on the
  script stack and returns a special marker. The interpreter loop then
  performs the call as an ordinary script-to-script call. Only built-ins
  that need the result mid-way (array iteration methods, `reduce`, sort,
  `replace`, `ToPrimitive`) use a real nested entry.

Two other options appear in the engines and the literature:

- **Self-hosting.** QuickJS-ng ships a few newer built-ins (for example
  `Array.fromAsync`) as JavaScript compiled to bytecode at build time.
  Callbacks from such built-ins are ordinary script-to-script calls, and
  `await` inside them needs no special support.
- **Resumable native functions.** Lua 5.2 lets a C function that calls
  back into Lua pass a continuation function; if the callee yields, Lua
  later calls the continuation instead of returning into the destroyed C
  frame (Lua 5.2 manual, section 4.7).

### Trade-offs

- A frame in the host stack (QuickJS, MuJS) is simple, but call depth
  depends on host stack size, and suspension needs a second frame kind.
- One contiguous value stack (Duktape, MicroQuickJS) lets the callee use
  the caller's argument slots without copying and shows all frames to the
  GC as one array; positions must be indices, because growth moves it.
- Lazy `arguments` and environments keep the common call cheap; the
  compiler must know whether a function uses `arguments`, direct `eval`
  or closures (memo 2).
- Deferred calls (MicroQuickJS) remove host recursion for common cases,
  but every built-in that uses them must follow the protocol.

### For swb

- ADR 0025 already requires script-to-script calls without Rust
  recursion. Duktape and MicroQuickJS show the structure: one loop, a
  vector of frame records and one value vector shared by all frames. A
  frame record holds a function handle, a program-counter index and a
  base index, never a reference into a vector.
- The borrow checker enforces the discipline that Duktape calls
  error-prone in C: the loop cannot keep a borrowed slice of the value
  vector or the bytecode across an operation that may call out, because
  that operation needs mutable access to the engine. Re-fetch both after
  each such operation (as MicroQuickJS does after every allocation).
- Native-to-script nesting stays as Rust recursion and needs its own
  depth counter (question 1.5). Deferred calls and self-hosted built-ins
  reduce how often it happens.
- GC timing matters for native code that holds handles. MuJS collects
  only between instructions; MicroQuickJS collects at any allocation and
  makes native code register its temporaries as roots. Collecting only at
  safe points lets built-ins keep handles in Rust locals (memo 4).

---

## 1.3 Generators and `async` functions

### Problem

A generator or `async` function must stop in the middle of its body,
keep its locals and temporaries, and continue later, possibly after the
original caller has returned. `await` must resume the function from a
promise job. Async generators combine both and must queue requests.
`yield*` must forward `next`, `throw` and `return` to an inner iterator.

### What the specification says

- Generators are shallow. `yield` is valid only directly in a generator
  body, not in nested functions (the grammar parameter for `yield` and the
  early errors in 15.5.1). Only one frame is ever suspended per
  generator, unlike Lua's stackful coroutines.
- Generator operations are in 27.5.3: GeneratorStart (27.5.3.1),
  GeneratorResume (27.5.3.3), GeneratorResumeAbrupt (27.5.3.4),
  GeneratorYield (27.5.3.6) and Yield (27.5.3.7). A generator has the
  states suspended-start, suspended-yield, executing and completed;
  resuming an executing generator is a `TypeError`.
- `generator.return(v)` resumes the generator with a return completion
  at the `yield`. `finally` blocks run, and a `finally` block may yield
  again.
- Parameter initialization happens before the generator first suspends
  (EvaluateGeneratorBody, 15.5.2, runs FunctionDeclarationInstantiation
  before GeneratorStart), so errors in default values throw at the call.
- `await` is defined in 27.7.5.3: resolve the value to a promise with
  PromiseResolve, attach fulfilment and rejection reactions with
  PerformPromiseThen (27.2.5.4.1), and suspend. The reactions run as
  promise jobs (NewPromiseReactionJob, 27.2.2.1) that the host enqueues
  (HostEnqueuePromiseJob, 9.5.5; in a browser, the HTML microtask queue).
  For a native promise, PromiseResolve returns it unchanged, so one
  `await` costs one job.
- Async functions start with AsyncFunctionStart (27.7.5.1). Async
  generators keep a queue of requests, each with its own promise
  capability (27.6.3). In an async generator, `yield v` awaits `v` first
  (27.5.3.7), and `return v` awaits `v` (14.10.1).
- `yield*` is in 15.5.5 (evaluation of YieldExpression). For a sync
  generator, the inner iterator's result object passes to the outer
  caller unchanged. If the inner iterator has no `throw` method, the outer
  generator closes it and throws a `TypeError`. `for await` over a sync
  iterable uses async-from-sync iterator objects (27.1.6).

### What the engines do

**QuickJS and QuickJS-ng** implement all of this. A call to a generator
or `async` function allocates the frame on the heap, with room for the
arguments, locals, the maximum operand stack and the captured-variable
references. The interpreter function has two entries: a normal call, and
a resume of a heap frame. At a suspension point the interpreter saves the
program counter and the operand stack top into the heap frame and returns
to its host caller with a code that says what happened (yield, delegated
yield, await, or the initial suspension). Because script calls recurse on
the host stack, only the generator's own frame needs saving, which
matches the shallow semantics.

The resume protocol keeps the runtime simple:

- The resumer pushes the sent value and a "resume mode" (next, return
  or throw) onto the saved operand stack.
- The compiler places code after every `yield` that tests the mode. For
  return mode, it jumps into the same compiled return path that a
  `return` statement uses, which already runs `finally` blocks and closes
  iterators (question 1.4). For throw mode, the resume enters the
  exception path directly.
- The compiler inserts an initial suspension point after parameter
  initialization, so default-value errors throw at the call.
- `yield*` is compiled as a bytecode loop: call the inner iterator's
  `next`, `throw` or `return` according to the resume mode, test `done`,
  and suspend with a "delegated" code. The delegated code tells the
  resumer to pass the inner result object through unchanged.

For `await`, the frame state also owns the two resolving functions of the
result promise. At an `await`, the runtime resolves the operand to a
promise and attaches two internal function objects that refer back to the
frame state. When the promise job runs one of them, it stores the value
(or raises the error) in the frame and resumes. QuickJS skips the extra
promise that the specification creates for the result of the reaction,
because user code cannot observe it; QuickJS-ng goes further and lets the
reaction handler be empty for this purpose. Async generators have an
object with a state field (including an awaiting-return state) and a
queue of pending requests; a drain step resumes the frame for the next
request.

A suspended frame is a GC object, reachable from the generator or the
promise reactions. If it becomes unreachable, it is collected and its
`finally` blocks never run; the specification does not require them to.

QuickJS-ng adds a check in the resume path of `async` functions and in
the promise reaction job: an uncatchable error (question 1.5) propagates
out and does not become a promise rejection. In the QuickJS resume path
of `async` functions, every error becomes a rejection.

**Duktape** (3.0 development branch) has no generators or `async`
functions. It has coroutines: each coroutine is a thread object with its
own value stack and call stack. A coroutine may yield only if no native
call or constructor call sits between its entry and the yield; Duktape
keeps a count of such frames. This is the general constraint:
a suspension cannot cross a host frame unless the host code itself can be
resumed (Lua 5.2 continuations, question 1.2).

**MuJS** and **MicroQuickJS** support only ES5 and have no generators.

### Literature

Lua 5.0 coroutines are stackful: the interpreter sets aside the whole
coroutine stack, and resume makes one nested host call into the
interpreter. Another technique compiles a generator into a state machine
(locals in a heap object, a `switch` on a state number at entry), as
regenerator, C# iterators and Rust `async` functions do. A bytecode VM
with resumable frames needs neither.

### Trade-offs

- **Frame always on the heap** (QuickJS): the frame never moves, so
  references to its slots stay valid. Two kinds of frame storage exist in
  the interpreter (host stack and heap).
- **Copy out and copy in**: a VM with one contiguous value stack can copy
  the generator's slots into a heap object at `yield` and back at resume.
  The cost is proportional to the frame size at every suspension; most
  generator frames are small. Captured variables that point into the
  frame by index must survive the move.
- **Resume mode compiled into the code** (QuickJS) reuses the
  `return`-statement path for `generator.return()`. The alternative is a
  runtime unwinder that injects a return completion; it must then
  duplicate the `finally` and iterator-closing logic.

### For swb

- With frames in a Rust vector, suspension is a data move: the frame
  record and its slice of the value vector go into the generator object,
  and resume puts them back on top. Alternatively, generator frames keep
  separate storage for life, at the cost of two addressing modes.
- Captured variables are the hard part: if an open captured variable
  refers to an absolute value-vector slot, copying the frame out
  invalidates it. Options: heap cells from the start for variables that
  generator and async functions capture, or references by (frame
  identity, slot). Memo 2 covers capture analysis.
- `await` uses no Rust stack: every resume starts from a promise job run
  by the host's microtask checkpoint. The tracer must visit frame storage
  held by generator objects and promise reactions.
- An uncatchable termination must not turn into a promise rejection
  (the QuickJS-ng check), or a killed script would continue in its
  `catch` handlers through promise reactions.

---

## 1.4 Exceptions

### Problem

The compiler must translate `try`, `catch` and `finally` so that a
throw finds the right handler, and so that `return`, `break` and
`continue` run every `finally` block they cross. A `finally` block that
ends abruptly replaces the pending completion. Loops over iterators
(`for`-`of`) and array destructuring must close the iterator on an abrupt
exit.

### What the specification says

- Completions are records with a type (normal, break, continue, return,
  throw), a value and a target label (6.2.4). Abrupt completions
  propagate through statement evaluation; UpdateEmpty (6.2.4.4) computes
  the completion value that `eval` and scripts return.
- The `try` statement is 14.15.3; CatchClauseEvaluation is 14.15.2. If
  the `finally` block completes normally, the earlier completion
  continues; if it completes abruptly, its completion wins.
- `for`-`in`, `for`-`of` and `for await`: ForIn/OfBodyEvaluation
  (14.7.5.7). When the body completes abruptly (break to an outer target,
  `return`, `throw`, `continue` to an outer loop), the loop calls
  IteratorClose (7.4.11) or AsyncIteratorClose (7.4.13). It does not close
  the iterator when the iterator's own `next`, `done` or `value` throws.
- IteratorClose: if the completion is a throw, any error from the
  iterator's `return` method is ignored and the original throw continues;
  otherwise an error from `return` propagates, and a non-object result is
  a `TypeError`. AsyncIteratorClose also awaits the result of `return`.
- Destructuring closes the iterator if it is not done:
  BindingInitialization (8.6.2) for binding patterns and
  DestructuringAssignmentEvaluation (13.15.5.2) with
  IteratorDestructuringAssignmentEvaluation (13.15.5.5) for assignment
  patterns. Built-ins use IfAbruptCloseIterator (7.4.12).

### What the engines do

**QuickJS (and MicroQuickJS)** put handler markers on the operand stack.
Entering a `try` pushes a special tagged value that holds the handler's
code position. The stack-depth pass (question 1.1) tracks it, so the
compiler knows the exact stack layout at every point. On a throw, the
interpreter pops operand-stack values until it finds a marker, pushes the
exception and jumps to the handler. If no marker exists in the frame, the
interpreter returns an exception indicator to its caller, which repeats
the search. A `for`-`of` loop pushes a marker with a reserved "no
handler" position next to the iterator: when the unwinder meets it, it
closes the iterator (with a throw completion, so errors from `return`
are ignored) and keeps unwinding. Destructuring uses the same mechanism.
On the non-throwing path, a `try` costs one push and one pop.

`finally` is a subroutine inside the function:

- Every exit from the `try` or `catch` block calls the `finally` code
  with a return address pushed on the operand stack; the `finally` code
  ends with an instruction that jumps to that address. A pending return
  value stays on the operand stack below the address.
- The exception handler of the `try` part calls the same `finally`
  subroutine and then rethrows.
- `break`, `continue` and `return` are resolved at compile time. The
  compiler walks outward over the statements it is leaving and emits, for
  each level, the pops for values that level keeps on the stack, an
  iterator close for a `for`-`of`, and a call to the `finally`
  subroutine. The return path after a `yield` (generator return mode)
  can start in the middle of an expression, where the number of values
  to pop is not known statically, so it uses an instruction that pops to
  the nearest marker.
- A `break` or `return` inside a `finally` block pops the saved address
  and pending value, which discards the pending completion as the
  specification requires.
- For async generators, the `return` path emits the close of the
  iterator as bytecode, including the `await` of the `return` method's
  result.
- The return address is an ordinary integer value on the operand stack.
  QuickJS checks it against the code length before jumping.

**Duktape** keeps runtime catcher records in each activation:

- Label catchers mark the targets of `break` and `continue`.
  Try-catch-finally catchers also implement `with`.
- A `break` or `continue` compiles to a direct jump when it crosses no
  `try` or `with` and targets the innermost label. Otherwise it takes a
  general path that walks the catcher list at run time.
- When control enters a `finally` block, two registers receive the
  pending completion: its value and its type (normal, return, break,
  continue or throw, with the label as value for break and continue). The
  instruction at the end of the `finally` block dispatches on the type:
  continue normally, resume the return, resume the label search, or
  rethrow. This reifies the specification's completion record.
- Throws use `longjmp` to the interpreter's catch point; returns avoid
  `longjmp` because it was slow (very slow under Emscripten).
- A `catch` clause always creates a declarative environment for its
  binding, so the catch variable uses slow-path lookups.

**MuJS** uses `setjmp` at each `try`. A per-try record saves enough
interpreter state to cut back the value stack, the scope chain and the
call depth, and to jump to the handler; the engine allows 64 active
`try` records in total, across all frames. MuJS copies the `finally` block to every exit:
the end of the `try`, the exception path (followed by a rethrow), and
every `break`, `continue` and `return` that crosses it. On those exits
the compiler also pops `for`-`in` iterators and ends `with` scopes. MuJS
has no `for`-`of`.

### Literature

- The Java virtual machine uses a per-method exception table: each entry
  maps a range of code positions to a handler, and the table is searched
  only when an exception occurs (JVMS 4.7.3). `finally` was first
  compiled as an in-method subroutine; Java compilers now copy the
  `finally` code to each exit, and class files of version 51.0 or later
  must not contain the subroutine instructions (JVMS 4.9.1), because they
  complicated bytecode verification.
- CPython 3.11 replaced its runtime block stack with an exception table
  ("zero-cost exceptions"): a `try` costs nothing when no exception
  occurs.
- Lua handles errors with `setjmp` and `longjmp` around protected calls.

### Trade-offs

- **Exception table** (JVM, CPython 3.11): zero cost on the normal path;
  a throw searches by code position and needs the stack depth per handler.
- **Marker on the operand stack** (QuickJS): cheap, verified by the depth
  pass; the unwinder tests every stacked value.
- **Runtime catcher records** (Duktape): flexible; each `try` sets up a
  record.
- **Copied `finally`** (MuJS, Java compilers): no runtime state, but code
  grows with every exit, multiplied by nesting.
- **`finally` as a subroutine** (QuickJS): one copy of the code; the
  return address is a stack value that the GC and verifier must handle.
- **Reified completion** (Duktape): one mechanism for all exit kinds,
  close to the specification; a dispatch at the end of every `finally`.

### For swb

- Safe Rust has no `setjmp`. Built-ins return a result whose error
  variant carries the thrown value; the loop looks for a handler in the
  current frame, and otherwise pops the frame and repeats in the caller.
  A native frame in between receives the error as its call result.
- Handler lookup can use a per-function table (zero cost on the normal
  path, with the stack depth per handler from the depth pass) or markers
  in the value vector.
- If `finally` is a subroutine, the return address should not be an
  ordinary number value that script code could ever see. A dedicated
  internal value kind, or a small per-frame control stack outside the
  value vector, avoids the concern noted in the QuickJS source.
- Sync iterators can be closed by the unwinder. For `for await` and
  async generators, closing awaits, so it must be compiled bytecode.
- An uncatchable termination (question 1.5) is a separate error kind
  that the handler lookup skips and that does not run `finally` blocks,
  as HTML 8.1.4.5 describes.

---

## 1.5 Limits

### Problem

A hostile or buggy page can loop forever, recurse without end, or
allocate without end. The engine must stop each case without crashing
the browser. A Rust stack overflow aborts the process, and a failed Rust
allocation aborts by default, so both must be prevented, not handled.

### What the specifications say

ECMA-262 says nothing about resource limits, and does not specify which
error a stack overflow throws. Engines differ: Duktape throws
`RangeError`; QuickJS-ng throws `RangeError` with the same message text
as V8; QuickJS throws a non-standard internal error type; MuJS throws a
plain string. HTML 8.1.4.5 allows a user agent to abort a script without
running `finally` blocks, or to throw `QuotaExceededError`.

### What the engines do: time

- **QuickJS** decrements a counter (start value 10,000) at every
  function entry, every jump (conditional or not, so every loop
  iteration), in some built-in loops (prototype-chain walks, `for`-`in`
  enumeration) and in the regular expression matcher. At zero it calls a
  host callback. If the host asks to stop, the engine throws an error
  marked uncatchable: the unwinder skips all handlers and `finally`
  blocks. QuickJS keeps the mark in the runtime state; QuickJS-ng marks
  the error object itself and treats all internal errors (out of memory,
  interruption) as uncatchable, and also stops them from becoming promise
  rejections.
- **Duktape** decrements a counter at every instruction dispatch when
  the feature is enabled, and calls the host check every 256K
  instructions by default. A timeout throws `RangeError`. Before any
  `catch` or `finally` block runs, the interpreter forces another check,
  so as long as the host keeps reporting the timeout, every handler
  rethrows at once and the error reaches the host's protected call. The
  design notes list the limitations: no checks inside built-ins (for
  example `JSON.stringify` of a huge sparse array, or many array methods),
  and script finalizers fail during a timeout.
- **MuJS** has an instruction budget: a counter decremented at every
  instruction, with an error when it runs out. When a budget is set, the
  regular expression matcher also runs with a runaway guard. MuJS also
  caps string length (2^28) and array length (2^26 elements).
- **MicroQuickJS** polls a counter at calls and jumps and in the regular
  expression matcher, with an uncatchable error, like QuickJS.

### What the engines do: stack depth

There are two different depths: the number of script frames, and the
depth of host recursion (native-to-script re-entry, recursive built-ins
such as `JSON.stringify` and `Array.prototype.join` on nested arrays, the
parser, the regular expression compiler, and GC marking).

- **QuickJS** reads the real host stack pointer and compares it with a
  limit computed from the stack top at startup and a configured size
  (1 MB by default). It checks at every call, in the parser, in the
  regular expression compiler and matcher, and in other recursive
  internal functions. Because script
  calls recurse, one check covers both depths.
- **Duktape** uses counters: native call nesting (default 1,000), total
  activations (default 10,000), compiler recursion (2,500), and limits
  for JSON, CBOR and regular expressions. Marking in the GC recurses only
  to a depth of 256; deeper objects are marked as temporary roots and
  handled in extra passes, so marking never fails. An optional host hook
  can check the real stack space at the points of unbounded recursion.
  While an error object is being built, the limits allow about ten extra
  levels, so that the stack-overflow error itself can be created.
- **MuJS** uses fixed arrays: 1,024 call levels, 4,096 value slots and
  64 active `try` records; the parser limits expression nesting to 400.
- **MicroQuickJS** keeps the script stack in the same arena as the heap,
  so a deep script recursion is an out-of-memory condition. Host re-entry
  is limited to 8 levels, and the parser does not recurse.

### What the engines do: memory

- **QuickJS** wraps its allocator with an optional limit; an allocation
  beyond the limit fails. A failed allocation throws an out-of-memory
  error; a flag prevents recursion while the error object is built. The
  GC runs when allocated memory passes a threshold, before the limit. In
  QuickJS the error is catchable; in QuickJS-ng it is uncatchable.
- **Duktape** retries a failed allocation after a collection, then after
  an emergency collection that also compacts objects, and only then
  throws. If creating the error object fails, it throws an error object
  that was allocated in advance for this case. The unwinding path is
  designed not to allocate: the reserved value-stack size only grows
  during calls, so unwinding a protected call needs no memory. The notes admit gaps: unwinding can
  allocate scope objects, and script `try`/`catch` is less safe than a
  host protected call under memory pressure.
- **MuJS** has an allocation budget that decreases with every allocation
  and does not increase when memory is freed, so it limits total
  allocation, not live memory. Its resource errors are static strings,
  which need no allocation.
- **MicroQuickJS** allocates from one fixed arena and keeps a minimum
  free gap between heap and stack. While it builds the out-of-memory
  error, it lowers the gap to a smaller critical reserve, so the error
  object fits; if even that fails, it throws `null`.

### Trade-offs

- **Where to poll.** A countdown at calls and jumps costs one decrement
  and a well-predicted branch; a check at every instruction (Duktape,
  MuJS) costs more and catches nothing extra, because every loop has a
  jump.
- **Built-ins** are the gap in every engine: one call can run for
  seconds (sorting a huge array, `join`, `repeat`, regular expression
  backtracking, `JSON.stringify`). Each long built-in loop needs its own
  poll or a size cap.
- **Uncatchable versus re-thrown termination.** An uncatchable error
  (QuickJS, MicroQuickJS) matches HTML 8.1.4.5. Duktape's re-throw lets
  native code clean up, but depends on the host check reporting the
  timeout until the error leaves the engine.
- **Measured stack versus counters.** A measured stack pointer adapts to
  any frame size but needs platform-specific code and a margin. Counters
  are portable and deterministic but must assume worst-case frames.
- **Memory.** A budget checked by the engine fails cleanly; a failure in
  the underlying allocator is hard to recover from. Creating the error
  needs a reserve or a preallocated error.

### For swb

- Time: a countdown at function entry and backward jumps, plus polls in
  long built-in loops and the regular expression matcher; at zero, compare
  the clock with the task deadline. Termination is an error kind that
  handlers and `finally` skip and that promise code never turns into a
  rejection.
- Script call depth: without Rust recursion, the frame vector length is
  the depth. A limit there gives a deterministic `RangeError` (the error
  type that Chromium and QuickJS-ng use; ECMA-262 does not specify one).
- Rust recursion: safe Rust cannot read the stack pointer. Options:
  (a) count every recursion level (native-to-script re-entry, JSON,
  nested `join`, the parser, the regular expression compiler) against
  conservative limits; (b) run the engine on a thread with a known, large
  stack size and size the counters for it; (c) a third-party crate that
  reads the stack pointer (which uses `unsafe` internally and needs the
  ADR 0003 process). GC marking should use an explicit work list, not
  recursion, so that it needs no limit at all.
- Memory: Rust aborts on allocator failure, so the engine must account
  for its heap and refuse an allocation that would exceed the budget
  before asking the allocator. Large allocations that a script requests
  directly (`ArrayBuffer`, `String.prototype.repeat`, array growth) need a
  budget check first; `try_reserve` covers other vectors. Collect before
  reporting out-of-memory, and use an error value that needs no
  allocation (as Duktape does). QuickJS makes out-of-memory catchable,
  QuickJS-ng does not.

---

## Agreement and difference across engines

- Agree: frames that must outlive a host call or survive a moving store
  are addressed by index or offset, not pointer (QuickJS generator frames,
  Duktape, MicroQuickJS). All bound recursion and poll a counter for
  timeouts.
- Differ: the depth mechanism (measured stack, counters, fixed arrays,
  shared arena); whether termination is catchable; the machine model
  (Duktape is the only register machine); and `finally` (subroutine,
  reified completion, copied code).

## Open points for the design session

- Stack or register machine. The literature favours registers for speed;
  the small engines favour a stack machine for compiler simplicity. If
  the parser builds a syntax tree first (memo 2), register allocation is
  easier than in Duktape's single pass.
- Byte-encoded instructions or a vector of Rust enum values.
- Generator frames: separate storage, or copy out and copy in.
- Handler lookup: table or markers; `finally`: subroutine, copy, or
  reified completion.
- Whether out-of-memory is catchable.

I did not check: how QuickJS closes `for await` iterators on a throw
(whether the unwinder awaits the `return` result); how fast any of these
engines are in practice; or whether the QuickJS family polls the
interrupt counter in sort and other array built-ins (I found polls only
in the places listed above).

## Sources

### Engines

- QuickJS (Fabrice Bellard), commit 535a7c2, 2026-09-29 (VERSION
  2026-06-04). Files read: `quickjs-opcode.h` (format declarations and
  the start of the instruction definitions); `quickjs.c`: the
  interpreter function (frame setup, call instructions, function-entry
  instructions for `arguments` and rest, `try` and `finally` and iterator
  instructions, generator and await instructions, the exception unwind
  path), generator and async function state (initialization, resume,
  generator `next`, async function resume and resolve functions, async
  generator state and request queue), the compilation of `try`, `break`,
  `continue`, `return` and `yield`/`yield*`, the stack-size computation,
  the jump-shrinking pass, the stack-overflow check, the interrupt poll,
  the out-of-memory error, the memory limit and GC trigger. Also
  `quickjs.h` (default stack size) and `libregexp.c` (location of the
  stack and timeout checks only).
- QuickJS-ng, commit c359cac, 2026-10-09. Files read: `quickjs.c` (call
  instructions, stack-pointer check, error constructors and the
  uncatchable flag, interrupt poll, out-of-memory and stack-overflow
  errors, async function resume, promise reaction job), `quickjs.h`
  (limit API and default stack size), `builtin-array-fromasync.js` (first
  lines, to confirm self-hosting).
- MuJS, commit aab59f2, 2026-10-06 (codeberg.org/ccxvii/mujs). Files
  read: `jsi.h` (limits, state), `jsrun.c` (limits, allocation budget,
  calls, `try`/throw, the start of the interpreter loop, `try`/`catch`/
  `with`/jump instructions), `jscompile.c` (abrupt-exit code, `try`
  compilation, function body setup), `jsstring.c` and `jsregexp.c`
  (run-limit use only).
- Duktape, commit 3afa016, 2026-09-05 (master, 3.0 development). Design
  notes read: `doc/execution.rst`, `doc/bytecode.rst`,
  `doc/value-stack-resizing.rst`, `doc/sandboxing.rst`,
  `doc/side-effects.rst`, `doc/compiler.rst` (sections on recursion,
  temporary registers, register shuffling, peephole, tail calls, `try`,
  `with`, break/continue, return, throw), `doc/memory-management.rst`
  (overview and emergency collection), `doc/low-memory.rst` and
  `doc/error-objects.rst` (searched only). Configuration descriptions:
  `config/config-options/` entries for the native call limit, call stack
  limit, interrupt counter, execution timeout, compiler recursion limit,
  mark-and-sweep recursion limit and native stack check. Source:
  `src-input/duk_js_bytecode.h` (instruction layout comment),
  `src-input/duk_js_call.c` (limit checks), `src-input/duk_js_executor.c`
  (finally, label, break/continue, interrupt, `try` setup, end of
  `finally`, executor error handling), `src-input/duk_bi_thread.c`
  (coroutine resume and yield checks), `src-input/duk_heap.h` and
  `src-input/duk_hthread.h` (searched for completion types and the
  interrupt default).
- MicroQuickJS, commit 6d4d7eb, 2026-09-26 (MIT). Files read:
  `README.md`; `mquickjs.c`: context layout, free-memory and stack check,
  allocator, frame layout comment, the interpreter entry, the call path
  for native and script functions with the deferred-call protocol, the
  exception unwind path, interrupt polling, the out-of-memory error, the
  getter path of property reads, the stack-size computation, and the
  limits at the top of the file.

### Specifications

- ECMA-262, 16th edition (ECMAScript 2025),
  https://tc39.es/ecma262/2025/ — sections 6.2.4, 7.4.11 to 7.4.13, 8.6.2,
  8.6.3, 9.4, 9.5.5, 10.2.1, 10.2.11, 10.4.4.6, 10.4.4.7, 13.15.5,
  14.7.5.7, 14.10.1, 14.15, 15.5.1, 15.5.2, 15.5.5, 15.10, 17, 27.1.6,
  27.2.2.1, 27.2.5.4.1, 27.5.3, 27.6.3, 27.7.5.
- HTML Standard, 8.1.4.5 Killing scripts,
  https://html.spec.whatwg.org/multipage/webappapis.html#killing-scripts
- The Java Virtual Machine Specification, Java SE 21, chapter 4 (4.7.3
  and 4.9.1), https://docs.oracle.com/javase/specs/jvms/se21/html/jvms-4.html

### Literature

- Y. Shi, D. Gregg, A. Beatty, M. A. Ertl, "Virtual Machine Showdown:
  Stack Versus Registers", VEE 2005,
  https://usenix.org/legacy/events/vee05/full_papers/p153-yunhe.pdf ;
  extended version in ACM TACO 2008 (Shi, Casey, Ertl, Gregg),
  https://mural.maynoothuniversity.ie/10185/1/KC-Virtual-2008.pdf
- R. Ierusalimschy, L. H. de Figueiredo, W. Celes, "The Implementation
  of Lua 5.0", JUCS 2005, https://www.lua.org/doc/jucs05.pdf (sections 5
  to 7).
- Lua 5.2 Reference Manual, section 4.7 "Handling Yields in C",
  https://www.lua.org/manual/5.2/manual.html#4.7
- M. A. Ertl, D. Gregg, "The Structure and Performance of Efficient
  Interpreters", Journal of Instruction-Level Parallelism 5, 2003,
  https://jilp.org/vol5/v5paper12.pdf (cited through the summary in the
  CGO 2015 paper and its discussion; I did not read the full text).
- E. Rohou, B. N. Swamy, A. Seznec, "Branch Prediction and the
  Performance of Interpreters: Don't Trust Folklore", CGO 2015,
  https://hal.inria.fr/hal-01100647 (I read summaries, not the full
  paper; numbers above are from those summaries).
- "What's New In Python 3.11" (inlined Python function calls, zero-cost
  exceptions, lazy frames), https://docs.python.org/3/whatsnew/3.11.html
- R. Nystrom, Crafting Interpreters, chapters "A Virtual Machine" and
  "Calls and Functions", https://craftinginterpreters.com/
- The Rust Unstable Book, "explicit_tail_calls",
  https://doc.rust-lang.org/unstable-book/language-features/explicit-tail-calls.html ;
  RFC 3407, https://github.com/rust-lang/rfcs/pull/3407
