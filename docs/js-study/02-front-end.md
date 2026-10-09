# Study memo 2: front end and scopes

Status: reviewed by the orchestrator on 2026-10-09 (ADR 0025).

Study memo for the swb JavaScript engine (ADR 0025). It answers the questions
of group "Memo 2: front end and scopes" in `docs/js-study/README.md`. It
describes problems, options and trade-offs. It does not decide the design.

Engines compared: QuickJS and QuickJS-ng (called "the QuickJS family" where
they agree), MicroQuickJS, MuJS, Duktape. The sources section lists versions
and files.

## Scope of the engines

The engines cover different language levels. This limits what each one can
teach for each question.

- QuickJS and QuickJS-ng implement the current language (ES2023 and later):
  `let`/`const`, arrow functions, destructuring, classes, modules,
  generators, `async`.
- MicroQuickJS implements a strict-mode-only subset close to ES5. It has no
  `let`/`const`, no arrow functions, no `with`, no direct `eval`, and no
  sloppy mode.
- MuJS implements ES5 with sloppy mode, `with` and direct `eval`.
- Duktape (master, 3.0 development) implements ES5.1 for the front end. It
  has no arrow functions, no destructuring and no block-scoped `let`; it
  treats `const` as a `var` that must have an initializer. Its compiler notes
  say that ES2015 constructs will need an intermediate representation.

So for cover grammars, temporal dead zone (TDZ) and per-iteration bindings,
only the QuickJS family is a working example. For parser structure, nesting
limits, `with`, `eval` and `arguments`, all five engines give examples.

## Measurement: the BBC fixture scripts

To give the size questions a scale, I downloaded the 60 scripts that the BBC
fixture page (`fixtures/pages/bbc`) loads and counted them with a simple
token scanner of my own. The scanner uses the previous-token rule for
regular expressions, so the counts are approximate.

- Total size: 3.5 MB in 60 files.
- Tokens: about 1.13 million, about 3.1 bytes per token.
- Functions: about 10,700 `function` keywords and 5,300 arrows, so about
  16,000 functions, or about 4,600 per MB.
- Maximum bracket nesting (parentheses, brackets, braces): 36.
- Maximum function nesting: 10.
- Line structure: several files are one line of 190 KB to 485 KB.
- Sum of the source lengths of all function bodies: about 8.0 MB, that is
  about 2.3 times the source size (1.5 to 4 times per file).

The last two numbers matter for source positions and for
`Function.prototype.toString` (section 2.6).

---

## 2.1 Parser structure

### Problem

A front end converts source text to bytecode. It can emit bytecode while it
parses (single pass, no tree), or it can build an abstract syntax tree (AST)
first and generate code from the tree. JavaScript makes a single pass hard:

- Declarations are hoisted. A `var` or function declaration later in the
  function changes how earlier references compile.
- Whether a local variable is captured by a closure is known only after the
  inner functions are parsed.
- Cover grammars (section 2.2) mean that the parser does not know what a
  construct is until it has read past it.

### Options in the engines

**Bytecode during parsing, then passes over the bytecode (QuickJS family).**
The parser emits bytecode directly, but does not resolve variables. It emits
name-based accesses tagged with the current block scope, and records each
declaration with its scope. The function-definition records of the whole
script stay in memory until the parse ends. Then the compiler processes them
with inner functions first: a resolution pass turns name-based accesses into
frame-slot, closure-cell or global accesses and marks outer variables as
captured; a later pass resolves jumps, does peephole optimization and builds
the line table; the stack depth is computed with a worklist. The inner-first
order matters: when an outer function is finalized, its inner functions have
already marked which of its variables are captured, so it emits "close"
operations only for those (section 2.4).

**Single pass with deferred inner functions (MicroQuickJS).** No AST. When the
parser meets an inner function, it skips the body by bracket matching and
records the position and whether the body mentions `arguments` or the
function's own name. After the outer function is complete, the recorded inner
functions are compiled from a worklist, without recursion. Free names
collected during a parse are patched to local accesses if a later (hoisted)
declaration makes them local; the rest are resolved against the complete
variable table of the parent. A function at nesting depth d is skipped d
times before it is compiled.

**Two or three passes per function, no tree (Duktape).** Pass 1 collects
declarations and emits throw-away bytecode with the same code paths; the lexer
rewinds; pass 2 emits the real bytecode; a rare third pass handles register
overflow. Expressions use top-down operator precedence (Pratt parsing) into
small expression fragments one step from final code, so a fragment can become
an assignment target or a value. To avoid 2, 4, 8, ... parses of nested
functions, an inner function is compiled once, in pass 1 of its parent, and
pass 2 jumps over it to its recorded end offset. So an inner function is
compiled before its parent's declarations are known, and it accesses all
outer variables by name at run time (section 2.4).

**AST first, then a compiler (MuJS).** Recursive descent builds a full tree; a
constant-folding pass runs over it; the compiler collects declarations in a
pre-walk per function, then emits code. The tree is freed after the compile.
From the node layout I estimate about 80 bytes per node on a 64-bit host.

### Where the engines agree and differ

- All use hand-written recursive descent for statements. Four of five avoid an
  AST to save memory; only MuJS builds one. Duktape's notes say its design
  will need an intermediate representation for ES2015; QuickJS shows that a
  single pass can work, at the cost of lookahead scans (section 2.2) and
  bytecode patching.
- All tree-less engines still look at each function twice: QuickJS resolves
  on the emitted bytecode, MicroQuickJS patches free names, Duktape re-lexes.

### Trade-offs for a 1 to 5 MB bundle

- **Memory.** An AST exists only while one script (or function) compiles. For
  the BBC set (1.1 million tokens), a rough estimate is 0.5 to 1 million
  nodes: 10 to 30 MB peak at 16 to 32 bytes per node, 40 to 80 MB at 80 bytes.
  I did not measure this. The QuickJS family also holds the records and raw
  bytecode of the whole script at once, so its peak also grows with script
  size, but less.
- **Time.** A single pass reads each token once plus lookahead scans; Duktape
  lexes each function twice; an AST adds one tree walk, which is cheap
  compared to lexing.
- **Analysis.** A tree makes cover grammar refinement, scope analysis before
  code generation, TDZ check elision and constant folding simple. Without a
  tree each needs its own trick (lookahead, bytecode rewriting, re-lexing).

### Literature

- Crafting Interpreters shows both designs: an AST with a separate resolver
  pass ("Resolving and Binding"), and a single-pass Pratt compiler that emits
  bytecode ("Compiling Expressions", "Local Variables"). The Lua 5.0 paper
  (section 2) also uses a one-pass compiler; Lua has no hoisting and no cover
  grammars, so this is easier there.
- The SpiderMonkey documentation describes an AST, then a separate bytecode
  emitter whose output is not in the garbage-collected heap.
- The V8 scanner article interns identifiers and strings at the
  scanner-parser boundary, with a direct table for single-character ASCII
  names, which are common in minified code.

### For swb

- An AST in safe Rust is natural: nodes in a `Vec` (or a few typed `Vec`s)
  addressed by `u32` indices, freed as a whole after each script or function.
  No `unsafe` and no garbage collector involvement.
- Keep all compile-time data outside the GC heap. Create heap objects
  (function templates, constants, strings) only when a function is finished.
  Duktape's notes show the cost of the opposite choice: the compiler must keep
  every intermediate value reachable for the collector, and a collection can
  run a finalizer that starts a nested compile. MicroQuickJS, with a moving
  collector, must register every compile-time object as a root. SpiderMonkey's
  documentation describes the same separation as swb would use.
- Identifiers: intern names into a compile-time table during lexing, and map
  them to runtime property keys when the function template is created.
- Tokens and AST nodes should carry byte offsets into the source, not line
  and column (section 2.6).
- If the design chooses a tree, the tree depth must be bounded for all later
  passes too (section 2.2), or those passes must use explicit stacks.
- An option that no allowed engine uses: lex the whole script into a token
  array first. This makes lookahead and rewinding cheap, but the lexer then
  cannot know whether `/` starts a regular expression (section 2.2). At about
  12 bytes per token, the BBC set would need about 13 MB.

---

## 2.2 Hard parts of the grammar

### Regular expression literal versus division

**Problem.** The lexer cannot tell from the characters whether `/` starts a
regular expression literal or is a division operator. ECMA-262 answers this:
the lexical grammar has several goal symbols, and the syntactic context
selects the goal (introduction of clause 12; regular expression literals in
12.9.5 and 13.2.7). The same mechanism decides how a `}` continues a
template literal (12.9.6, 13.2.8).

**Options.**

- *Parser-driven re-lexing (QuickJS family, main parse).* The lexer always
  produces a division token. When the parser expects a primary expression and
  finds `/` or `/=`, it moves the lexer back to that character and asks it to
  scan a regular expression literal. This follows the specification exactly,
  because only the parser knows the syntactic context.
- *Previous-token rule in the lexer (MuJS, MicroQuickJS; QuickJS family in
  lookahead scans).* The lexer allows a regular expression unless the previous
  token is an identifier, a literal, `this`, or a closing `)` or `]`. This
  fails in known cases: `if (x) /re/.test(s)` (after `)`); `}` that ends a
  block (regular expression may follow) versus `}` that ends an object literal
  (division may follow); `a.return / 2` (keyword as a property name). The
  engines disagree on `}`: MuJS and QuickJS forbid a regular expression after
  it, MicroQuickJS allows it. MicroQuickJS also forbids one after `case` and
  `do`, where it is valid. The QuickJS source notes that its lookahead scans
  are not reliable when regular expressions are present.
- *Previous-token table plus parser hints (Duktape).* A per-token table says
  whether a regular expression may follow. The parser sets a one-shot flag
  after `.` (so `a.return / 2` is a division), and after a function
  declaration it sets a flag to allow a regular expression.

**Trade-off.** Parser-driven lexing is correct but needs a lexer that produces
one token at a time and can restart at a position. Any scan that skips text
without parsing it (lookahead, lazy pre-parse with bracket matching) must
fall back to a heuristic or to a real parse.

### Cover grammars: arrow parameters and destructuring assignment

**Problem.** `(a, b)` can be a parenthesized expression or arrow parameters;
`[a, b]` and `{a, b}` can be literals or assignment patterns. The parser
knows only at `=>` or `=`. ECMA-262 defines this with cover grammars: the
parser accepts a general production and then refines it (the syntactic
grammar, 5.1.4; early errors for the grouping operator, 13.2.9.1; object
initializer with `CoverInitializedName`, 13.2.5.1; assignment and
destructuring assignment, 13.15.1 and 13.15.5.1; arrow functions, 15.3.1;
async arrow functions, 15.9.1). Early errors forbid what is valid only in one
reading, for example `({a = 1})` as an expression.

**Options.**

- *Lookahead scan before parsing (QuickJS family).* At the start of an
  assignment expression, on `(` the parser scans with bracket matching to the
  closing `)`, looks at the next token (`=>` without a preceding line break
  means arrow parameters), rewinds, and parses in the right mode. The same
  scan checks `[` or `{` for a following `=`, and `for` heads for `in`, `of`
  or `;`, and reports whether `...` or `=` occurred inside. Costs: nested
  parentheses cost time quadratic in the depth, and nested patterns are
  scanned once per level. The scan stops at 256 nesting levels and then
  guesses wrong. After a wrong guess, the parser overwrites emitted bytecode
  with no-operation instructions or reports a "too complicated" error.
- *Parse as an expression, then convert the tree (the specification's
  model).* At `=>` or `=` the expression tree becomes a pattern, and the
  pattern's early errors apply. This needs a tree for at least the covered
  part. No allowed engine does it for ES2015. The V8 pre-parsing article
  notes that the variable bookkeeping must wait too: a partial `({ d }` can
  still become an assignment that references `d` or parameters that declare
  it.
- *Speculative parse with rewind.* Parse one reading, rewind on failure.
  Duktape's notes list what a rewind must undo when bytecode is emitted
  directly: bytecode, temporary registers, constants, inner functions,
  labels. Nested arrows can make naive backtracking exponential.

**Trade-off.** Lookahead scans keep a single pass but add rescans and depend on
heuristic lexing inside the scan. Tree conversion is linear and exact but
needs a tree.

### Automatic semicolon insertion (ASI)

ECMA-262 12.10 gives the rules. The engines implement them in three ways:

- *Statement level (Duktape, QuickJS family, MuJS).* After a statement that
  needs a terminator, the parser accepts `;`, or a token preceded by a line
  break, or `}`, or end of input. Each token carries a flag "preceded by a
  line terminator".
- *Restricted productions* (`[no LineTerminator here]`): before postfix
  `++`/`--`, after `return`, `break`, `continue`, `throw`, before `=>`,
  after `async`, and others. The QuickJS family and Duktape check the
  line-break flag at each restricted point in the parser. Duktape handles
  postfix `++` by giving it a very low binding power when a line break
  precedes it. MuJS handles `return`, `break`, `continue` and `throw` in the
  lexer: a line break after these tokens is returned as a `;` token.
- *Special cases.* The rule for `do ... while (x)` followed by another
  statement on the same line is in 12.10.1 (it was an erratum that Duktape's
  notes still describe as a hack). The QuickJS family's handling of `let`
  as an identifier versus a declaration also depends on line breaks: `let`
  followed by a line break and an identifier is a declaration only in some
  contexts.

ASI makes the "longest valid statement" depend on the next token, so the
parser must always read one token past a statement. All engines do this.

### Contextual keywords

`let`, `static`, `yield`, `await`, `async`, `of`, `get`, `set`, `as`,
`from`, `target`, `meta` are keywords only in some contexts. ECMA-262 12.7.2
lists reserved words, and 13.1.1 gives the early errors for `yield`, `await`
and strict-mode names. A keyword written with a Unicode escape cannot act as a
keyword.

- *QuickJS family.* Keywords are interned at startup with small fixed
  numbers, so classification is an integer comparison. The lexer makes an
  identifier a keyword depending on context (strict mode, generator, async
  function, module) and the parser reclassifies the current token when the
  context changes at a function boundary. An escaped identifier never acts as
  a keyword. `let` uses a short lookahead.
- *Duktape.* Each token carries two classifications, as a reserved word and as
  a plain name; the parser picks one (after `.`, any name is allowed).
- *MuJS.* Fixed keyword set; future reserved words are checked in the
  compiler.

### Early errors

ECMA-262 defines early errors per production in "Static Semantics: Early
Errors" subsections, for example 14.2.1 (duplicate lexical names in a block),
14.3.1.1 (`let`/`const`), 14.12.1 (`switch`), 14.13.1 (labels), 15.2.1
(functions), 15.7.1 and 15.7.7 (private names in classes), 16.1.1 (scripts),
16.2.1.1 (modules), and 13.2.7.1 (the pattern of a regular expression literal
must be valid). ParseScript (16.1.5) returns the errors, and the script does
not run at all if one exists.

Engine behaviour:

- *QuickJS family.* Checks during the parse. A lexical declaration is checked
  against the same scope and child scopes; a `var` is checked against the
  lexical declarations of all enclosing blocks of the function, because it
  hoists through them. Annex B exceptions are handled at the same point.
- *Retroactive strictness.* A `"use strict"` directive (11.2.1) makes the
  preceding parameter list strict, and 15.2.1 forbids it with a non-simple
  parameter list. The QuickJS family scans the directive prologue with
  lookahead before it parses the body and checks parameter names after.
  Duktape learns strictness in pass 1 and applies it in pass 2.
- *MuJS* checks some early errors (strict-mode names, duplicate parameters,
  `delete` of a name) in the compiler. This is correct only because the whole
  script compiles before it runs.

### Deep nesting and the host stack

**Problem.** A recursive descent parser uses host stack per nesting level.
Hostile input such as 100,000 open parentheses must not crash the browser.
Later passes that recurse over a tree, and nested function compiles, have the
same problem.

**Options.**

- *Depth counter (Duktape, MuJS).* Duktape counts recursion in the function
  body, statement and expression parsers (default limit 2,500; an example
  configuration for small stacks uses 50) and throws a `RangeError`. MuJS
  limits tree depth to 400 and throws a `SyntaxError`. MuJS also counts
  iterations of left-associative chains (`a+b+c+...`), because they build deep
  trees that the compiler later walks recursively.
- *Measure the host stack (QuickJS family).* Each lexer call compares the
  stack position with a limit set at startup. Every recursion level reads a
  token, so this catches all parser recursion, and it adapts to the real
  stack size.
- *No host recursion (MicroQuickJS).* The expression, statement and block
  parsers are resumable state machines driven by a loop that keeps return
  states, and the locals that must survive a call, on an explicit stack in the
  engine heap. Inner functions come from a worklist (2.1); the regular
  expression compiler works the same way. Nesting is limited only by heap
  size. The cost is a large, hard-to-read transformation of the parser.

**Trade-off.** A counter is simple and portable but its safe value depends on
the stack size and frame sizes, which a debug build changes. Stack measurement
adapts but is platform dependent. An explicit stack removes the problem but
makes the parser much harder to write and review.

### For swb

- Rust has no portable way to read the stack pointer. Taking the address of a
  local variable and comparing integers is safe code, but it is not a
  guaranteed measure. Options: a depth counter with a limit that is tested in
  debug builds; running the compile on a thread with a known large stack; or a
  crate that measures the stack (it would use `unsafe` internally, which needs
  the ADR 0003 process). ADR 0025 already plans a parser nesting limit.
- The limit must cover every recursive pass: parser, any AST walk, scope
  analysis, code generation, and the regular expression parser. A counter in
  the AST builder that also counts long left-associative chains (as MuJS does)
  protects all later tree walks.
- The BBC set nests 36 brackets and 10 functions deep. A limit of a few
  hundred levels leaves a wide margin for real code.
- The hostile-page set should get cases for deep parentheses, deep arrays,
  long `+` chains, deep nested functions and deep templates.
- If swb uses lookahead scans, they need a bound on their total work (the
  quadratic case above).
- Parser-driven lexing for `/` and template `}` requires an on-demand lexer
  that can restart at a byte offset. It conflicts with a pre-built token
  array.

---

## 2.3 Lazy parsing

### Problem

A news page loads megabytes of script, and much of it never runs during a
page load (both the V8 and the SpiderMonkey documentation state this). Full
compilation of every function costs time and memory for bytecode that is
never used. Lazy parsing skips function bodies at first and compiles a
function when it is first called.

### What the allowed engines do

None of the allowed engines compiles lazily. All compile every function before
the script runs. Two engines skip function bodies within one compile, which
shows parts of the technique:

- MicroQuickJS skips each inner function body by bracket matching and compiles
  it later in the same compile. The skip records only whether the body
  mentions `arguments` or the function's name. Bracket matching does not
  check syntax, and it uses the previous-token rule for regular expressions.
  This is acceptable only because the full parse of the body follows in the
  same compile, before the script runs.
- Duktape jumps over inner function bodies in pass 2 using the stored end
  offset from pass 1.

### Options in the literature

The V8 article on pre-parsing and the SpiderMonkey documentation describe the
same design:

- A pre-parser runs over the whole function body without building a tree or
  bytecode. When the function is first called, the engine parses and compiles
  only that function ("delazification" in SpiderMonkey's terms).
- The pre-parser must still check all syntax and all early errors, because
  ECMA-262 requires the script to fail before it runs if any early error
  exists (16.1.5). The V8 article reports that an older pre-parser that did
  not track declarations missed duplicate-declaration errors, and that full
  tracking fixed this without much cost.
- The pre-parser must track declarations and references in the skipped body.
  The enclosing function needs this to decide which of its variables are
  captured (they must live in a heap environment) and which can stay in frame
  slots. Tracking only references overestimates captures, because an inner
  declaration can shadow an outer name.
- Nested functions: in V8 before version 6.3, each inner function was
  pre-parsed again for each enclosing level, so the cost grew with nesting
  depth, which bundlers make common. The fix: for each skipped function with
  inner functions, the pre-parser stores a compact record of its variable
  allocation (flags per variable, in declaration order). The full parse
  reapplies them and skips the inner functions without pre-parsing them
  again, so each function is pre-parsed at most once and fully parsed at most
  once.
- Eager heuristics: V8 compiles at once a function expression that starts
  with `(function` or follows the `!function(){...}()` minifier pattern, and
  warns that too much eager compilation costs memory.

### What the pre-parser must still check or record

From ECMA-262 and the literature:

- All syntax, including regular expression literal patterns (13.2.7.1) and
  template literals, with parser-driven lexing (section 2.2).
- All early errors: duplicate lexical declarations; `var` that conflicts with
  a lexical declaration; labels and `break`/`continue` targets; `yield` and
  `await` placement; `super` and `new.target` placement; private names, which
  must be declared in an enclosing class (15.7.7) and so cross function
  boundaries; strict-mode restrictions, including retroactive strictness from
  a directive prologue (section 2.2).
- For the enclosing function: the free names of the skipped function (after
  removing names it declares), and whether it uses `this`, `arguments`,
  `new.target` or `super` (arrow functions inherit these from the enclosing
  function), and whether it contains a direct `eval` (section 2.5).
- For later: the start and end offsets of the function (for the full parse
  and for `Function.prototype.toString`), the parameter count for `length`,
  and the name.

### Trade-offs

- Gain: less time and memory for code that never runs.
- Cost: each called function is parsed twice. The pre-parser is a second
  parser that must accept exactly the same language; V8 shares one code base
  for both. A lazy function must keep a reference to the source text and to a
  description of its enclosing scopes until it is compiled.
- Errors: a bug in the pre-parser that accepts a program the full parser
  rejects gives a late `SyntaxError` at the first call, which ECMA-262 does
  not allow.

### For swb

- Lazy compilation can come after a correct eager compiler if the front end
  prepares for it: positions as byte offsets, a scope analysis that can run
  over a skipped body, and a storable description of the scope chain.
- In an index-handle heap, a lazy function template can hold a handle to the
  shared source, a byte range, and a handle to a scope description of its
  enclosing functions (names, slot kinds, capture flags), which must stay
  alive while any lazy inner function exists.
- One parser with a mode flag (no tree, no code) avoids two parsers that
  disagree.
- The BBC set has about 16,000 functions. I have no measurement of how many
  run during page load.

---

## 2.4 Scope analysis

### Problem

Each binding needs a storage place: a frame slot (fast, freed at return), a
heap cell or heap environment (survives the frame, needed for closures), or a
property of the global object. ECMA-262 defines scopes as environment records
(9.1; declarative records 9.1.1.1) and the binding of names by walking
outward (GetIdentifierReference 9.1.2.1, ResolveBinding 9.4.2). The
specification creates a new environment for each block and function call; an
engine wants to avoid most of these.

### Options in the engines

**All in frame slots, or all by name in a heap environment (MuJS).** A
function with no inner function, no `with`, no `catch` clause, no direct
`eval` and no use of `arguments` keeps all locals in stack slots. Every other
function creates a heap environment object per call and stores all its
variables there by name; free names are looked up along the environment
chain. No capture analysis is needed, but a function that contains any
closure pays name lookups for all its variables.

**Register frame with an environment that is copied at exit (Duktape).**
Locals live in registers. When needed (closures, `eval`), the function gets an
environment record that refers to the live frame through a name-to-register
map; at return the register values are copied into the record ("closed").
Inner functions access outer variables by name, because they are compiled
before the parent's declarations are known (section 2.1). The compiler omits
the map and the record when it can prove they are not needed. Duktape's notes
list a known problem: closing copies all variables, so a closure keeps every
outer variable alive. Catch variables always get a real environment.

**Open and closed upvalues (QuickJS family, MicroQuickJS).** This is the
design of Lua ("The Implementation of Lua 5.0", section 5) and of Crafting
Interpreters ("Closures"). Locals start in frame slots. A captured variable
gets a cell when the first closure that uses it is created; while the frame
lives, the cell refers to the slot ("open"); when the scope ends or the
function returns, the value moves into the cell ("closed"). Closures always go
through the cell. At most one cell exists per variable, so sibling closures
share it (Lua and clox find it in a list of open cells; the QuickJS family
keeps the cell index per slot). A variable two or more functions up reaches
the closure through each intermediate function ("flat closures" in the Lua
paper). The compiler needs capture information only to emit close operations
at scope ends; MicroQuickJS has no block scopes and closes only at return.

### One binding per loop iteration for `let`

ECMA-262 14.7.4.3 and 14.7.4.4: a `for (let ...; ...; ...)` loop creates a
copy of the loop bindings before the first iteration and before each later
iteration, so each closure created in the body sees the value of its own
iteration. `for-in`/`for-of` with `let` create a new binding per iteration
(14.7.5).

- *QuickJS family.* Each block-scoped binding has its own frame slot. At the
  end of each iteration the captured loop variables are closed: closures of
  this iteration keep the closed cell, the slot keeps the value, so the next
  iteration starts with a copy. The init scope is closed before the first
  iteration, which gives the first copy. Without captures this costs nothing.
  A source comment questions the `continue` path; I did not check it.
- *MuJS, Duktape, MicroQuickJS.* No `let`, so no example.

### Temporal dead zone (TDZ)

ECMA-262 creates `let`, `const` and `class` bindings uninitialized; an access
before initialization throws a `ReferenceError` (9.1.1.1).

- *QuickJS family.* On scope entry each lexical slot gets an internal
  "uninitialized" value, and every access to a lexical variable uses a checked
  instruction. Block-level function declarations are initialized at scope
  entry. The source notes that many checks could be removed; the compiler
  does not remove any. Cells for captured and global lexical bindings carry
  the same state.
- *Other engines.* No TDZ.

When a compiler can omit the check (my analysis of the specification, not
taken from an engine):

- The access is in the same function, in the declaring block or a nested
  block, textually after the end of the declaration, and not in a later
  `switch` case clause than the declaring one (all clauses share one block,
  so a later clause can run without the earlier one). Loops do not break
  this: each block entry re-creates the binding.
- An access from an inner function is safe only if that function is created
  after the initialization, for example a function expression or arrow that
  appears after the declaration. A function declaration is hoisted and can
  run earlier.
- A scope tree with source positions makes this check simple.

### Trade-offs

| Design | Speed of closure variable access | Speed of non-captured locals | Compiler work | Memory kept alive by closures |
|---|---|---|---|---|
| Heap environment by name (MuJS non-simple functions) | slow (name lookup) | slow in such functions | none | all variables |
| Register frame plus copy at exit (Duktape) | slow (name lookup) | fast | small | all variables of the outer function |
| Open/closed cells (QuickJS, MicroQuickJS, Lua, clox) | fast (index plus one indirection) | fast | capture analysis | only captured variables |
| Cells created at declaration for captured variables | fast | fast | capture analysis before code generation | only captured variables |

The last row is an option the engines do not use: if capture is known before
code generation (AST or pre-pass), create the cell at the declaration and keep
its handle in the slot. No open state, no close operation; a per-iteration
copy is "new cell, copy value". The cost is one allocation per captured
variable per scope entry, even when no closure is created.

### For swb

- Open cells refer into a frame. In safe Rust with index handles, an open
  cell can hold (stack identifier, slot index) and a closed cell holds the
  value; access is a match on the two states. There is no aliasing problem.
- Generators and `async` functions move their frame off the stack when they
  suspend (memo 1). Open cells that refer into such a frame must stay valid,
  for example by closing them at suspension or by referring to a frame
  handle instead of a stack position. Cells created at declaration avoid this
  question.
- Cells are small heap objects; with a tracing collector they need no
  reference counting. The number of cells equals the number of captured
  variables, which is the minimum any design needs.
- The TDZ sentinel must be a value variant that user code can never observe;
  every path that copies a slot (closing, `arguments`, debugger output) must
  treat it.
- Capture flags are known only after inner functions are analysed. With a
  tree, run scope analysis over the whole function nest before code
  generation; with a single pass, follow the QuickJS inner-first order.

---

## 2.5 Dynamic scope: direct `eval`, `with`, and sloppy `arguments`

### Problem

Three features make name resolution depend on run time:

- Direct `eval` (detected at run time: a call through the plain name `eval`
  whose value is the original `eval` function, 13.3.6.1) compiles code that
  sees the caller's scopes. In sloppy mode it can add `var` bindings to the
  caller's function scope (PerformEval 19.2.1.1, EvalDeclarationInstantiation
  19.2.1.3). Strict `eval` gets its own variable environment.
- `with` (14.11) puts an object in the scope chain; any name inside may
  resolve to a property of that object, subject to `Symbol.unscopables`
  (object environment records, 9.1.1.2).
- In sloppy functions with simple parameter lists, the `arguments` object is
  mapped: `arguments[i]` and the parameter `i` are the same variable
  (CreateMappedArgumentsObject 10.4.4.7, FunctionDeclarationInstantiation
  10.2.11).

The goal is to confine the cost to the functions that use these features.

### Direct `eval`

All engines detect at compile time a call through the name `eval`, mark the
function, and check at run time whether the callee is the original `eval`.

- *Duktape.* The function is marked as possibly using eval: it keeps its
  name-to-register map and always creates `arguments`. Variables that the
  eval declares go into an environment record as deletable bindings;
  register-bound variables are never deletable.
- *MuJS.* The function keeps all variables in a heap environment by name
  (section 2.4), so eval code sees them.
- *QuickJS family.* The function and all enclosing functions mark all their
  variables as captured. The compiled function keeps its variable names and
  closure-cell names, ordered by scope, and eval code is compiled against this
  description, so its references to caller variables become cell accesses. In
  sloppy mode the function also gets a hidden heap object for variables that
  the eval may add; each name that does not resolve statically to a local
  tests this object at run time before it continues outward. Strict direct
  eval needs no such object (QuickJS's documentation calls it optimized).
  Inner functions without `eval` are still resolved statically.
- *MicroQuickJS.* Only indirect (global) `eval`.

### `with`

- *QuickJS family.* The `with` object sits in a hidden local of the block.
  For every name inside the body, including nested functions, the compiler
  emits a run-time test (does the object have the property?) before the
  static resolution. Code outside the body is not affected.
- *Duktape.* Inside a `with` body (it tracks the nesting depth) all name
  accesses use the slow path by name.
- *MuJS.* The function gets a heap environment, and the object is pushed on
  the environment chain.
- *MicroQuickJS.* No `with`.

### Sloppy-mode `arguments` mapping

- *QuickJS family.* The mapped object holds, per parameter, the same cell that
  a closure would use; creating it captures all parameters. Strict functions
  and non-simple parameter lists get an unmapped object.
- *Duktape.* The mapped object maps indices to parameter names in the
  callee's environment record, so mapping forces an environment record and
  name lookups.
- *MuJS.* The object is a copy of the arguments. I found no mapping, so
  assignments to `arguments[i]` do not change the parameter. This deviates
  from the specification.
- *MicroQuickJS.* Strict only; no mapping.

All engines use the same static test: a function that neither names
`arguments` (unshadowed) nor contains a direct `eval` needs no `arguments`
object. An arrow function that names `arguments` uses the enclosing
function's object, so it counts as a use there. ECMA-262 15.7.9 defines the
related check for class field initializers.

### Trade-offs

- Capturing all variables (QuickJS family) keeps eval code and the rest of
  the program fast, but needs scope descriptions in every function that can
  host a direct eval.
- Name-based environments for whole functions (MuJS, Duktape) conform
  naturally but slow down the whole function.

### For swb

- Store a scope description (names, slot kinds, cell indices, strictness) in
  a function template only when the function contains a direct `eval` or is
  an ancestor of one. This also serves a future debugger and the lazy compile
  of section 2.3.
- The hidden object for eval-added variables can be an ordinary heap object
  addressed by handle. Names declared by sloppy eval are configurable
  bindings (`delete x` can remove them), unlike other function bindings.
- For `with`, the per-name runtime test must include the
  `Symbol.unscopables` check of 9.1.1.2.
- Mapped `arguments` with shared cells fits the handle model: the object
  holds cell handles for the mapped indices and plain values for the rest.
  Deleting or redefining a mapped index must break the mapping (10.4.4).
- Real pages rarely use `with` and sloppy direct `eval`, but old libraries
  and some analytics snippets do. A conforming slow path is enough; it must
  not slow down functions that do not use these features.

---

## 2.6 Top level

### Global object properties versus global lexical declarations

ECMA-262 answers the semantics. The global environment record (9.1.1.4)
combines an object record over the global object (for `var` and function
declarations, and for host-defined properties) and a declarative record (for
top-level `let`, `const` and `class`). All classic scripts of a realm share
both. GlobalDeclarationInstantiation (16.1.7) first checks all top-level
declarations of a script against existing bindings (for example, a `let x`
when another script already declared `let x`, or a `var x` when a lexical `x`
exists), and throws a `SyntaxError` before any part of the script runs. Only
then does it create the bindings. Annex B.3.2 adds sloppy-mode block-level
function declarations to the variable scope.

The browser consequence: each `<script>` element is one Script. A `let` in one
script is visible to later scripts, and a duplicate `let` in a later script
stops that whole script.

**Engine options for access to globals.**

- *Lookup by name at every access (QuickJS-ng, MuJS, Duktape).* QuickJS-ng
  looks up the name first in a hidden object for global lexical declarations,
  then in the global object. MuJS and Duktape walk the environment chain to
  the global object record.
- *Cells bound at closure creation (QuickJS, MicroQuickJS).* In the current
  QuickJS, each global name that a function references is one of its closure
  cells, bound when the closure is created. A global object property created
  by a declaration holds a cell instead of a plain value, so `x = 1` and
  `globalThis.x = 1` change the same cell. Global lexical declarations are
  cells in a separate hidden object. A reference to a global that does not
  exist yet gets a cell in another hidden table, and a later declaration of
  that name takes over the cell. Script instantiation follows the
  specification's two phases: check all declarations, then create cells.
  MicroQuickJS also binds globals to cells, but separate from the global
  object: properties created directly on the global object are not visible as
  variables (its documentation states this restriction).
- I did not check how the QuickJS cell design handles names found on the
  prototype chain of the global object, or host accessor properties.

**For swb.** The global object is `Window`, reached through `WindowProxy`.
The host adds and removes properties on it, and named access to elements by
`id` comes from an object on its prototype chain (HTML specification). Both
QuickJS variants note that they do not support an exotic global object. A cell
design must therefore handle: accessor properties on `Window`; names found
only on the prototype chain; properties deleted by the host or by `delete`;
and a `WindowProxy` that changes its target on navigation. A name-based
lookup with a per-instruction cache (memo 3) is the simpler option that
conforms by construction; cells are faster but need these cases.

### Script versus module top level

ECMA-262 answers this: modules (16.2) are always strict; top-level `this` is
`undefined`; `await` is reserved and top-level `await` is allowed; `import`
and `export` are allowed only there (16.2.2, 16.2.3; early errors 16.2.1.1);
top-level declarations, including `var`, go into a module environment record
(9.1.1.5), not the global object; imports are live, immutable bindings to the
exporting module's variables (InitializeEnvironment, 16.2.1.7.3.1). HTML-like
comments (`<!--`, B.1.1) are allowed in scripts only; old inline scripts on
real pages still use them.

**Engines.** The QuickJS family compiles the module body as a function whose
top-level variables are closure cells, created and initialized with hoisted
functions before evaluation so that cyclic imports work. Imports are a
separate kind of closure entry; I did not read the code that links them to the
exporter's cells. MuJS, Duktape and MicroQuickJS have no modules.

**For swb.** With cells, a module namespace object and import bindings are
handles to the same cells. The TDZ sentinel in a cell gives the required
`ReferenceError` for imports of not-yet-initialized `let` exports.

### Source positions for error messages

**Problem.** Errors and stack traces need a line and column per bytecode
position. Minified bundles are often one long line: in the BBC set several
files are single lines of 190 KB to 485 KB, so a line number alone does not
locate anything.

**Options.**

- *Byte offset per instruction, line and column computed later (QuickJS).*
  The parser records source byte offsets in the instruction stream. When a
  function is finished, the compiler converts them to line and column with a
  cache that moves over the source, and stores a delta-encoded table. The
  source notes that this conversion is slow and that checkpoints would help.
- *Line and column per token (QuickJS-ng).* The lexer tracks both for every
  token; the final table is similar.
- *Exponential-Golomb coded line and column deltas (MicroQuickJS),* with an
  option to drop columns.
- *Line only, delta-coded with an absolute checkpoint every 64 instructions
  (Duktape),* about 10 to 15 percent of the bytecode size by its notes.
- *A line number stored before every instruction (MuJS).* Simple, but doubles
  the code size. No columns.

Columns: QuickJS counts columns in code points (it skips UTF-8 continuation
bytes). I did not check which unit Chromium reports for columns; this is a
black-box question for the differential tool (ADR 0025 lists stack format as
a Chromium/Node measurement).

### Source text for `Function.prototype.toString`

ECMA-262 20.2.3.5 requires that `toString` of a function defined in source
returns the exact source text slice of the definition. Pages use this:
feature tests that look for `[native code]`, and older dependency-injection
libraries that parse parameter names from the source.

- *QuickJS family.* Each function keeps its own copy of its source text
  (an option strips it). Nested functions copy overlapping text. For the BBC
  set, the function bodies sum to about 2.3 times the source size, so this
  design would keep about 8 MB of copies for 3.5 MB of source.
- *Duktape, MuJS, MicroQuickJS.* Return a placeholder string without the
  source, which does not conform. Duktape has a build option that stores the
  source as an internal property; I did not check whether `toString` uses it.
- *Option not used by the allowed engines.* Keep the script source once, as a
  shared immutable string, and store a start and end offset per function.
  The cost is that the whole script text stays alive as long as any function
  from it lives; in a browser this is about the life of the page.

`Function` constructor bodies (CreateDynamicFunction, 20.2.1.1) have a
synthesized source text that the specification defines; they need their own
string.

### For swb

- Use byte offsets into the shared script source everywhere in the front end
  (tokens, tree nodes, scope records, function templates). Convert to line
  and column only when an error or stack trace needs it, with a per-script
  line-start index (one integer per line, built once, binary search). On a
  485 KB line, the column still needs a scan from the line start, or extra
  checkpoints inside long lines if columns are counted in UTF-16 units or code
  points rather than bytes.
- One shared source string per script serves `toString`, lazy compilation
  (section 2.3) and error positions. In the handle model it is a heap string
  or a host-side `Rc<str>`-like buffer that function templates reference.
- Source from the network arrives as Unicode text after the HTML or HTTP
  decoding. Strings passed to `eval` and `Function` are JavaScript strings
  and can contain lone surrogates. The front end needs one input
  representation for both, or a conversion. I did not check how the QuickJS
  family converts a string with lone surrogates before it compiles it.
- The line-number table per function can use delta encoding with periodic
  checkpoints (Duktape's idea, plus columns) to keep it small and fast to
  search.

---

## Summary of agreement and difference

| Question | QuickJS family | MicroQuickJS | Duktape | MuJS |
|---|---|---|---|---|
| Parser output | bytecode, then resolve passes | bytecode in one pass; inner functions deferred | bytecode, 2 to 3 passes per function, re-lexing | AST, then compiler |
| `/` versus regexp | parser-driven (heuristic in lookahead) | previous token | previous token plus parser hints | previous token |
| Cover grammars | lookahead scan, then parse once | not applicable (ES5) | not applicable (ES5.1) | not applicable (ES5) |
| Nesting guard | host stack measurement | no host recursion (explicit stack) | depth counter (2,500) | tree depth limit (400) |
| Lazy parsing | no | skips bodies, compiles in same compile | skips bodies in pass 2 only | no |
| Closure variables | open/closed cells | open/closed cells | name lookup, frame copied at exit | name lookup in heap environment |
| TDZ | checked access everywhere | not applicable | not applicable | not applicable |
| Direct eval | statically resolved cells plus a hidden object for sloppy eval vars | not supported | function falls back to slow path | function uses heap environment |
| `with` | per-name runtime test inside the body | not supported | slow path inside the body | heap environment |
| Mapped `arguments` | shared cells | not applicable | through environment by name | not mapped (copy) |
| Globals | QuickJS: cells; QuickJS-ng: lookup by name | cells, separate from global object | lookup by name | lookup by name |
| Columns | yes | yes (optional) | no | no |
| `toString` source | copy per function | placeholder | placeholder by default | placeholder |

## Open points that I did not check

- How the QuickJS global cells treat names found on the prototype chain of the
  global object and host accessors.
- Whether QuickJS closes per-iteration `let` cells on the `continue` path.
- The unit (code units or code points) of columns in Chromium stack traces.
- How many of the BBC functions run during page load (relevant for lazy
  parsing).
- The real AST memory for the BBC set; the numbers in 2.1 are estimates.

## Sources

### Engines (read only; not built or run)

- **QuickJS** (Fabrice Bellard), commit 535a7c2, 2026-09-29, VERSION
  2026-06-04. Files: `quickjs.c` (token and parse state, lexer entry and error
  positions, directive and name checks, scope push/pop and variable
  definition with redeclaration checks, lookahead scan and regexp decision,
  postfix expression entry, assignment expression and arrow detection, `let`
  detection, contextual keyword classification, `for` statement, function
  parsing and source text capture, closure variable creation, scope variable
  resolution including `with` and eval objects, eval variable capture, hoisted
  definitions, scope entry and exit in the label pass, line table
  computation, function creation order, global variable cells and closure
  instantiation, mapped arguments, variable reference structure, stack
  overflow check, `Function.prototype.toString`); `doc/quickjs.texi`
  (Internals chapter).
- **QuickJS-ng**, commit c359cac, 2026-10-09. Files: `quickjs.c` (token
  structure with line and column, lookahead scan call sites, global variable
  lookup and definition, TDZ checks, stack checks, function source capture,
  line and column table decoding).
- **MicroQuickJS** (Bellard, Gordon), commit 6d4d7eb, 2026-09-26, MIT
  license. Files: `README.md`; `mquickjs.c` (bracket-matching skip and
  regexp decision, recursion-free parser driver, function declaration
  skipping, deferred inner function compilation, free variable patching and
  resolution, stack size computation, closure creation, line table emission,
  `Function.prototype.toString`).
- **MuJS** (Artifex), commit aab59f2, 2026-10-06. Files: `jsparse.c` (whole
  file), `jscompile.c` (local variable handling, eval and call compilation,
  declarations, function body), `jslex.c` (regexp context and restricted
  production handling), `jsi.h` (limits), `jsrun.c` (local access and
  function call setup, arguments object), `jsvalue.c` (arguments object),
  `jsfunction.c` (`toString`).
- **Duktape**, commit 3afa016, 2026-09-05 (master, 3.0 development). Design
  notes: `doc/compiler.rst` (whole), `doc/identifier-handling.rst` (first 640
  lines), `doc/arguments-object.rst` (first 200 lines),
  `doc/function-objects.rst` (property and line table sections). Source:
  `src-input/duk_js_compiler.c` (header comment, recursion limit, multi-pass
  function body parse, inner function skip, function template cleanup,
  statement dispatch for `const`), the compiler recursion limit option in
  `config/config-options/`, `config/examples/shallow_c_stack.yaml`,
  `src-input/duk_lexer.c` (regexp
  comments only), `src-input/duk_bi_function.c` (`toString` placeholder,
  one line).

### Specification

- ECMA-262, current draft, https://tc39.es/ecma262/ . Sections cited: 5.1.4;
  clause 12 introduction; 9.1, 9.1.1.1, 9.1.1.2, 9.1.1.4, 9.1.1.5, 9.1.2.1,
  9.4.2; 10.2.11; 10.4.4, 10.4.4.7; 11.2.1, 11.2.2; 12.7.2; 12.9.5, 12.9.6;
  12.10, 12.10.1; 13.1.1; 13.2.5.1; 13.2.7, 13.2.7.1; 13.2.8; 13.2.9.1;
  13.3.6.1; 13.15.1, 13.15.5.1; 14.2.1; 14.3.1.1; 14.7.4.3, 14.7.4.4;
  14.7.5; 14.11; 14.12.1; 14.13.1; 15.2.1; 15.3.1; 15.7.1; 15.7.7; 15.7.9;
  15.9.1; 16.1.1; 16.1.5; 16.1.7; 16.2; 16.2.1.1; 16.2.1.7.3.1; 16.2.2;
  16.2.3; 19.2.1.1; 19.2.1.3; 20.2.1.1; 20.2.3.5; B.1.1; B.3.2. I took the
  numbers from the table of contents of the draft on 2026-10-09.

### Literature

- R. Ierusalimschy, L. H. de Figueiredo, W. Celes, "The Implementation of Lua
  5.0", Journal of Universal Computer Science, 2005, sections 2 and 5.
  https://www.lua.org/doc/jucs05.pdf
- R. Nystrom, Crafting Interpreters, chapters "Closures", "Resolving and
  Binding", "Compiling Expressions", "Local Variables".
  https://craftinginterpreters.com/closures.html
- V8 team, "Blazingly fast parsing, part 2: lazy parsing".
  https://v8.dev/blog/preparser
- V8 team, "Blazingly fast parsing, part 1: optimizing the scanner".
  https://v8.dev/blog/scanner
- SpiderMonkey documentation, "SpiderMonkey" overview, section on the
  JavaScript parser. https://firefox-source-docs.mozilla.org/js/

### Measurement

- The 60 scripts referenced by `fixtures/pages/bbc/files/*.html` (build
  20261005), downloaded on 2026-10-09 and counted with my own approximate
  token scanner (not part of swb).
