# Study memo 4: values, strings, numbers, memory

Status: reviewed by the orchestrator on 2026-10-09 (ADR 0025).

Study memo for swb's JavaScript engine (ADR 0025). It answers questions 1
to 4 of group "Memo 4" in `docs/js-study/README.md`. It describes problems,
options and trade-offs. It does not decide the design.

Engines compared: QuickJS, QuickJS-ng, MuJS, Duktape (3.0 development
branch and its design notes), MicroQuickJS. Exact versions and the files
read are in "Sources". Section numbers of ECMA-262 refer to the current
draft at tc39.es/ecma262.

Short summary:

- Values: the engines use three layouts: a tag next to a payload (16
  bytes on 64-bit hosts), NaN-boxing in 8 bytes (only where heap
  references fit in 32 bits), and a tagged machine word with boxed
  doubles. With 32-bit index handles, swb can use any of the three in
  safe Rust, NaN-boxing included.
- Strings: the engines disagree most here. QuickJS stores each string as
  8-bit or 16-bit code units and adds ropes; Duktape 3 and MicroQuickJS
  store WTF-8 and cache index-to-byte-offset positions; MuJS stores
  modified UTF-8 and scans from the start on each index operation. All
  engines intern property names (atoms) and treat array-index keys
  specially.
- Numbers: the QuickJS family uses one exact multi-precision converter
  for all radixes and formats. Duktape uses Dragon4. MuJS uses Grisu2,
  the C library and approximate code, and so deviates from ECMA-262 in
  edge cases. Only the QuickJS family implements BigInt.
- Memory: QuickJS uses reference counting with a trial-deletion cycle
  collector; Duktape reference counting with a mark-and-sweep backup;
  MuJS plain mark-and-sweep; MicroQuickJS mark-compact with explicit
  host roots. How native code keeps values alive and when a collection
  may run differ in each engine; these two choices decide most of the
  work in native (Rust) code.

---

## 4.1 Value representation

### Problem

Every JavaScript value has one of the language types of ECMA-262 §6.1:
Undefined, Null, Boolean, String, Symbol, Number, BigInt, Object. An
engine also needs some values that scripts never see: a marker for an
uninitialized binding (the temporal dead zone), a marker for "an
exception is pending", and stack entries that hold internal data (for
example the target of a `catch`). The value type is copied on every
instruction, so its size and the cost of the type test decide much of
the interpreter speed and of the memory use of arrays, property storage
and the VM stack.

The specification sets two constraints that matter for the layout:

- A Number is an IEEE 754 binary64 value. ECMA-262 §6.1.6.1 says that
  all NaN values are indistinguishable to ECMAScript code. An engine may
  therefore keep only one NaN bit pattern. The bit pattern that a typed
  array stores for a NaN is implementation-chosen (§25.1.3.17,
  NumericToRawBytes), so canonicalization does not break typed arrays.
- `-0` is a distinct Number value. A small-integer representation can
  hold `+0` only; `-0` must stay a double.

### Options in the engines

**Tag next to payload.** The value is a pair: a type tag and a payload
that holds a double, a 32-bit integer, a boolean or a heap reference.
QuickJS and QuickJS-ng use this layout on 64-bit hosts (16 bytes, which
fits in two registers and can be returned from a function in registers).
The QuickJS documentation gives the reason: memory use matters less on
64-bit hosts. Duktape uses the same layout where its compact form is not
possible (12 or 16 bytes). MuJS always uses a 16-byte value and stores
strings of up to 15 bytes inside the value, so short strings need no
heap allocation.

**NaN-boxing.** A double has many NaN bit patterns. The engine keeps one
canonical NaN for real NaN values and uses the other patterns to encode
non-number values: some high bits hold a tag and the rest hold a
pointer, an integer or a boolean. Every value is 8 bytes. QuickJS and
Duktape both use NaN-boxing, but only when pointers are 32 bits wide,
because the payload must fit in the low part of the word. Both engines
canonicalize NaN when they box a computed double, so that a computed NaN
can never be confused with a tagged value.

**Tagged machine word with boxed doubles.** MicroQuickJS stores a value
in one machine word. The low bits select the kind: an integer of one bit
less than the word (31 bits on 32-bit hosts), a pointer to a heap block
(the block header says what the block is), or a special value
(undefined, null, booleans, the internal markers, a single-code-point
string, a compact native-function reference). Doubles are boxed in heap
blocks. On 64-bit hosts it adds an inline form for doubles whose exponent
is in a reduced range, so many doubles need no allocation. This is
the classic low-bit tagging scheme of Lisp and ML systems (see
Gudeman's survey in Sources).

**Internal value kinds.** QuickJS, QuickJS-ng and MicroQuickJS reserve
value kinds for the internal markers listed above. The exception marker
lets every native function return one value and signal failure without a
separate result type.

**Tag choice as a fast test.** QuickJS chooses its tags so that one
comparison tells whether a value holds a counted heap reference, and so
that "both operands are integers" is one bitwise operation on the two
tags.

### Small integers next to doubles

ECMA-262 has one Number type. The engines add an integer representation
for speed:

- QuickJS and QuickJS-ng: a 32-bit signed integer kind. Arithmetic on two
  integers computes the result in a wider type; on overflow it returns a
  double. Multiplication checks the case where the result is zero and one
  operand is negative, because the correct result is `-0`, which only a
  double can hold. The engine has two constructors for numbers: one that
  turns an integral double back into the integer kind (it compares bit
  patterns, so `-0` stays a double), and one that does not. The
  interpreter's double fast paths use the second one; built-in functions
  and the embedding API use the first.
- Duktape: an optional 48-bit signed integer kind, off by default. 48
  bits is what remains of the NaN-boxed word after the tag, and it
  covers both the signed and the unsigned 32-bit results of bitwise
  operators and millisecond `Date` values. The check "can this double
  become an integer" is costly, so Duktape runs it only at selected
  points: compile-time constants, function return values, unary plus.
  Its design note reports much faster integer loops on soft-float
  hardware and up to about 10% slower code in the worst case elsewhere.
- MicroQuickJS: the 31-bit (or 63-bit) word integer; anything else is a
  double.
- MuJS: no integer kind. All numbers are doubles.

Where the engines agree: the integer kind is invisible to scripts; every
operation must give the same result as double arithmetic; `-0`, overflow
and division (which rarely gives an integer) leave the integer kind.
Where they differ: the integer width (31, 32 or 48 bits) and the points
where a double turns back into an integer.

### Arithmetic fast paths

The QuickJS-family interpreters test, in order: both operands are
integers; both are numbers; for `+`, both are strings; otherwise the
general path that calls ToPrimitive and may run user code. The
semantics are in ECMA-262 §6.1.6.1 (Number::add, Number::multiply,
Number::unsignedRightShift and others); the engines differ only in the
order of the type checks. Note that `>>>` produces values above the
signed 32-bit range.

### Trade-offs

| Layout | Size | Cost | Notes |
|---|---|---|---|
| Tag and payload | 16 bytes on 64-bit | type test reads the tag; payload access is direct | simplest; twice the memory of NaN-boxing for arrays and stacks |
| NaN-boxing | 8 bytes | every double read is a bit move; boxing a computed double needs a NaN check | needs references of at most about 48 bits; the engines use it only with 32-bit pointers |
| Tagged word | 4 or 8 bytes | doubles need a heap block (or a reduced-range inline form) | best for memory; double-heavy code allocates |

### For swb

- A heap reference is a 32-bit index (plus a kind), not a pointer, so
  the condition under which QuickJS and Duktape use NaN-boxing (32-bit
  references) holds for swb on every host. NaN-boxing in safe Rust is
  bit manipulation on a 64-bit integer with the standard double/bits
  conversions; no `unsafe` is needed, because no payload becomes a
  pointer.
- A Rust enum with one variant per kind is the simplest option (16
  bytes with a double payload), and the compiler checks every match. A
  design can hide the layout behind a small API and change to NaN-boxing
  later if measurements show a need.
- If the value is NaN-boxed, every path that creates a double from
  outside data (typed-array reads, `Math` results, parsing) must
  canonicalize NaN, as QuickJS and Duktape do.
- Choose the integer width together with the bitwise operators and
  array indices: 32-bit signed is the common choice, but array indices
  and `>>>` results go up to 2^32 − 1.
- Decide where doubles turn back into integers (QuickJS: at the API and
  in built-ins; Duktape: at selected points). Each check costs a few
  instructions on a hot path.

---

## 4.2 Strings

### Problem

ECMA-262 §6.1.4 defines a String as a sequence of 16-bit code units, at
most 2^53 − 1 long. The code units are usually UTF-16, but any sequence
is valid, including unpaired surrogates. `length`, indexing, `charCodeAt`,
`slice` and regular expressions work on code units. Real pages build
large strings in loops, compare property names all the time, and use
strings that look like integers as array keys. The engine must make
indexing, concatenation, comparison and property-key lookup fast, and
must stay correct for unpaired surrogates.

### Storage encoding

**Two widths (QuickJS, QuickJS-ng).** Each string has a flag: either
every code unit fits in 8 bits (Latin-1) and the string uses one byte per
code unit, or it uses two bytes per code unit. Indexing is O(1) in both
cases. A string is created in the narrow form if possible. QuickJS-ng
also narrows a substring of a wide string when all its code units fit in
8 bits. The QuickJS documentation notes that converting a pure-ASCII
string to UTF-8 for the host needs no copy.

**WTF-8 with a position cache (Duktape 3, MicroQuickJS).** The string is
stored as UTF-8, with paired surrogates combined into one four-byte code
point and unpaired surrogates stored in the three-byte form (this is
WTF-8). Scripts must still see 16-bit units: `length` counts a
supplementary code point as two, and an index can point between the
two halves of a pair. Duktape's design note lists the hard places:
concatenation, substring and replace must merge halves that become
adjacent, so that each string has exactly one encoding (interned strings
are compared by identity), and regular expressions must match and
backtrack over each half separately. For fast indexing, both engines keep
a per-string "all ASCII" flag (then an index is a byte offset) and a
small global cache of recent (string, code-unit index, byte offset)
triples: four entries in Duktape, two in MicroQuickJS. A lookup scans
from the nearest cached position or the nearer end. The cache holds its
strings weakly. The benefit: native code sees standard UTF-8 for
well-formed strings.

**Modified UTF-8 with linear scans (MuJS).** MuJS stores UTF-8 with an
overlong two-byte form for U+0000, so strings stay NUL-terminated.
Supplementary code points count as two units. Every index operation
scans from the start of the string, so a loop over `charAt` is quadratic.
MuJS has no position cache.

Where the engines agree: all keep 16-bit-unit semantics for scripts.
Where they differ: QuickJS gives O(1) indexing and pays conversion at the
host boundary; the UTF-8 engines save memory and conversion for
ASCII-heavy text and pay with caches or scans for indexing.

### Concatenation and string building

ECMA-262 does not say how to build strings; `+`, template literals,
`String.prototype.concat` (§22.1.3.5) and `Array.prototype.join`
(§23.1.3.18) only define the result. Loops such as `s += piece` are
common on real pages and are quadratic if each step copies the whole
string.

- **Ropes (QuickJS, QuickJS-ng).** A concatenation can produce a rope
  node: two child strings plus the total length. The engine creates one
  only when it pays: short pieces (up to a few hundred code units) are
  copied, a rope starts when the left part is long (some thousands of
  code units), and a short piece next to a short leaf is merged into the
  leaf. Above a depth of about 60 the rope is rebalanced with the
  algorithm of Boehm, Atkinson and Plass (leaves sorted into buckets
  sized by Fibonacci numbers, then joined). An operation that needs the
  characters flattens the rope and writes the flat result back into the
  node, so other holders of the same rope do not flatten it again. The
  bounded depth also bounds tree walks and recursive release.
- **In-place append (QuickJS).** When the left operand has a reference
  count of one and its allocation has unused capacity, the right operand
  is copied into that capacity. This depends on reference counting: only
  a count of one proves that no other value sees the change.
- **Builders.** All engines use a growable buffer for built-in functions
  that produce strings (join, JSON, replace, templates). The QuickJS
  buffer starts narrow and widens to 16-bit units at the first wide code
  unit. Duktape's design note recommends building strings in one step
  (join) instead of repeated `+`, because every intermediate string is
  interned.
- **No special handling (MuJS).** Every concatenation allocates a new
  string.

### Shared substrings

QuickJS-ng creates a slice (a reference to the parent string plus an
offset) for substrings above about 1 KiB; shorter substrings are copied.
A slice of a slice points to the original parent, so chains never form.
The risk is retention: a small slice keeps a large parent alive. The
other engines copy substrings. MicroQuickJS and MuJS avoid allocation for
the shortest strings instead: MicroQuickJS has an immediate value for a
one-code-point string, and MuJS stores up to 15 bytes inside the value.

### Interned strings (atoms)

Property lookup compares names. If each name is stored once (interned),
comparison is an identity test and the hash is computed once.

- **QuickJS and QuickJS-ng.** An atom is a 32-bit index into a
  runtime-wide atom table; hash chains, linked by table indices, find an
  atom by content. Atoms are reference counted; several hundred
  predefined atoms (built-in names) are constant. Symbols and private
  names are atoms too, without content hashing. Ordinary string values
  are not interned.
- **Duktape.** Every string is interned, so identity comparison works
  for all strings and the hash and flags (ASCII, array index with its
  value) are computed once. The cost is a table lookup for every new
  string, including temporaries; the design note names string-heavy
  loops as the weak case, and notes that resizing the table does not
  help against inputs built to collide.
- **MuJS.** Property names and literals are interned in a balanced
  binary tree (an AA tree) and are never freed while the engine runs.
- **MicroQuickJS.** Interned strings live in a sorted array, searched by
  binary search on content (addresses change when the heap is compacted,
  so the order cannot depend on addresses). Built-in names live in
  read-only tables. The table holds its strings weakly: after marking,
  the collector removes unmarked entries.

Where the engines differ: the lifetime of an interned name (forever in
MuJS, reference counted in QuickJS, weak and collected in MicroQuickJS
and Duktape) and the scope (property names only, or all strings in
Duktape). A hostile page can create unbounded numbers of distinct
property names (for example computed keys in a loop), so an atom table
that never frees entries is a memory risk.

### Property keys that are array indices

ECMA-262 §6.1.7 defines an *array index* as a property key whose
canonical numeric form is an integer from 0 to 2^32 − 2, and an *integer
index* as one from 0 to 2^53 − 1. CanonicalNumericIndexString (§7.1.23)
defines the string-to-number test that typed arrays use. Arrays must
treat the key `"5"` and the number `5` as the same property, and keys
such as `"05"` or `"-0"` as ordinary names.

- QuickJS: half of the atom number range holds integers up to
  2^31 − 1 directly (its documentation says so). When a string becomes
  an atom, the engine checks whether it is a canonical decimal integer
  and then returns the integer form. Integers from 2^31 to 2^32 − 2
  become ordinary string atoms, so the array code must also recognise
  them on its slow path.
- MicroQuickJS: a property key is a value that is either a non-negative
  31-bit integer or an interned string; the interned string carries a
  flag "its text is a number".
- Duktape: the interned string carries the precomputed array index.
- MuJS: parses the key text on each array access.

Length limits: QuickJS allows 2^30 − 1 code units and MuJS 2^28 bytes;
both throw a RangeError above that. A limit below the 2^53 − 1 of the
specification is an implementation limit and a defence against pages
that build huge strings.

### For swb

- Two-width storage maps directly to safe Rust (a byte vector or a
  vector of 16-bit units) and gives O(1) indexing without caches. The
  cost is conversion at the boundary to the rest of swb, which uses UTF-8
  (DOM text, URLs, CSS). WTF-8 reverses the trade. The design should
  weigh boundary crossings in the bindings against string indexing in
  scripts.
- Rope nodes and slices hold handles to other strings, which the
  collector traces. Rewriting a flattened rope in place is an ordinary
  write to one slot.
- The QuickJS in-place append needs a reference count of one, which a
  tracing collector does not have; swb would rely on ropes (or a builder
  for `+=` in loops) for linear-time concatenation.
- Atoms already are index handles in QuickJS. In a tracing heap the atom
  table can hold entries weakly (MicroQuickJS), with built-in names in a
  constant range.
- Hash tables keyed by attacker-chosen strings need a seeded hash or
  another defence against collision floods.
- Keep array-index keys as integers in the key type (QuickJS,
  MicroQuickJS), and handle 2^31 to 2^32 − 2 explicitly.

---

## 4.3 Numbers

### Problem

The engine must convert doubles to strings and back, in several places
and with different rules: `ToString` and `Number.prototype.toString` with
a radix from 2 to 36, `toFixed`, `toExponential`, `toPrecision`,
`parseInt`, `parseFloat`, `Number(string)`, numeric literals and JSON.
The conversions are on hot paths (JSON, string concatenation of numbers)
and must be exact. BigInt adds arbitrary-precision integer arithmetic.

### What ECMA-262 requires

- **Number to string:** Number::toString (§6.1.6.1.20). The digits are
  the *shortest* sequence that converts back to the same double, for any
  radix. When several shortest sequences exist, a non-normative note
  recommends the one closest to the exact value, and on a tie the even
  one, and cites Gay's paper. For radix 10 the section also fixes when
  exponent notation is used. For other radixes the output is always
  positional, without an exponent. Since the radix rule is part of this
  section, the shortest-round-trip requirement also applies to radix 2
  to 36.
- **Fixed formats:** `toFixed` (§21.1.3.3), `toExponential` (§21.1.3.2)
  and `toPrecision` (§21.1.3.5) are defined on the exact mathematical
  value of the double. On a tie they pick the larger integer, which
  means "round half away from zero" on the magnitude. Up to 100
  fraction digits are allowed, so these need exact arithmetic beyond 64
  bits.
- **String to number:** StringToNumber (§7.1.4.1.1) and RoundMVResult
  (§7.1.4.1.3). If the decimal input has more than 20 significant
  digits, the implementation may replace the digits after the 20th with
  zeros, or round up at the 20th digit; otherwise the result must be the
  correctly rounded double. `parseInt` (§19.2.5), `parseFloat`
  (§19.2.4), numeric literals and JSON each have their own lexical rules
  (whitespace, signs, hex prefixes, empty string, `Infinity`).
- **BigInt:** §6.1.6.2 defines the operations; BigInt.prototype.toString
  is §21.2.3.3.

### Published algorithms

Number to string (shortest digits):

- Dragon4 (Steele and White 1990; Burger and Dybvig 1996): exact, with
  multi-precision integers of about 1,100 bits for binary64; any radix;
  slow. Gay (1990) adds practical optimizations (the netlib code that
  ECMA-262 cites).
- Grisu (Loitsch 2010): 64-bit arithmetic and cached powers of ten.
  Grisu2 always round-trips but is not always shortest; Grisu3 detects
  the about 0.5% of inputs it cannot solve, for a fallback.
- Ryū (Adams 2018), and later Schubfach (Giulietti) and Dragonbox
  (Jeon): shortest and correctly rounded for all inputs, with 64/128-bit
  arithmetic and tables; radix 10 only. Champagne Gareau and Lemire
  (2026) compare them.

String to number: Clinger (1990) and Gay (1990) use a fast path when
digits and power of ten are exact in a double, and exact arithmetic
otherwise; Eisel-Lemire (Lemire 2021) decides almost all inputs with a
64-bit or 128-bit product and a table of powers of five, with an exact
fallback.

### Options in the engines

**QuickJS, QuickJS-ng, MicroQuickJS (one shared library).** The three
engines use the same small converter by Bellard (2024). It does all
conversions with exact multi-precision arithmetic on 32-bit limbs, in a
fixed temporary buffer on the caller's stack, without heap allocation.

- Shortest output: start from a digit count that always suffices (17
  for radix 10), compute the correctly rounded digits, then try fewer
  digits and convert each candidate back with correct rounding; stop at
  the last count that round-trips. The same code serves radix 2 to 36.
  Doubles that are exact integers take a fast path.
- Fixed formats scale the exact value by a power of the radix and round
  half away from zero, as required, up to about 100 digits.
- Parsing keeps about 128 bits worth of significant digits (about 38
  decimal digits) and truncates the rest, which RoundMVResult allows;
  power-of-two radixes round exactly. One exact multiplication by the
  radix power and one rounding follow.
- QuickJS-ng parses JSON numbers with the C library's `strtod` instead;
  that result depends on the C library (and its locale handling).

**Duktape.** Dragon4 in the basic free-format form of Burger and Dybvig,
with a fixed-size multi-precision integer in stack buffers (the design
note gives at least 1,050 bits for doubles; the implementation uses more
because parsing needs it). Fixed formats generate one extra digit and
round it, which the note itself calls a shortcut with known incorrect
corner cases. Parsing uses the same machinery. A fast path handles
32-bit integers, which are the most common numbers to print. The note
rejects table-driven algorithms such as Grisu3 because the tables cost
more code size than Duktape's whole regular-expression engine.

**MuJS.** Grisu2 for radix-10 shortest output (so some outputs are
longer than the shortest form); double arithmetic for other radixes (not
exact); the C library's `printf` for `toFixed`, `toExponential` and
`toPrecision`; an old double-arithmetic `strtod` for parsing (not
correctly rounded). From the code, ties in the fixed formats follow the
C library's rounding (round half to even on glibc), while ECMA-262 wants
the larger value, so for example `(2.5).toFixed(0)` should give a
different result than the specification requires; I did not run it.

Where the engines agree: none relies on the C library for the shortest
form. Where they differ: exactness (QuickJS family exact everywhere;
Duktape exact for shortest output; MuJS approximate) and speed (all
three exact implementations use slow multi-precision arithmetic in
general; only MuJS uses a fast published algorithm).

### BigInt

Only QuickJS and QuickJS-ng implement BigInt; MicroQuickJS, Duktape and
MuJS do not. QuickJS:

- Representation: a normalized two's-complement array of 32-bit or
  64-bit limbs; values that fit in one limb are stored inline in the
  value, without allocation.
- Multiplication: schoolbook on unsigned limbs plus the standard
  two's-complement correction for negative operands; nothing faster.
- Division: long division in the style of Knuth's Algorithm D (TAOCP
  vol. 2, §4.3.1).
- To string: repeated division by the largest radix power that fits in a
  limb (quadratic); bit extraction for power-of-two radixes.
- Limit: about one million bits; larger results throw a RangeError,
  which bounds the time of the quadratic algorithms.

The literature for faster methods: Knuth TAOCP vol. 2 §4.3 (classical
algorithms, Karatsuba), and Brent and Zimmermann, *Modern Computer
Arithmetic* (subquadratic multiplication, division and radix
conversion).

### Trade-offs

- An exact multi-precision converter for everything (QuickJS) is small,
  handles every radix and format with one method, and is correct by
  construction, but it is slow for the common radix-10 case.
- A table-driven algorithm (Ryū, Schubfach, Eisel-Lemire) is fast for
  radix 10, but `toString(radix)`, the fixed formats and the slow paths
  of parsing still need exact arithmetic. So an engine needs both.
- Approximate methods (MuJS) are simplest but differ from the
  specification, and from browsers, in edge cases.

### For swb

- ADR 0025 names `ryu` as a candidate library. It covers only one case:
  shortest radix-10 digits. It does not cover radix 2 to 36, the fixed
  formats, or parsing. The rest needs either a small exact
  multi-precision routine (the QuickJS approach, which in Rust is plain
  arithmetic on vectors of limbs and needs no `unsafe`) or more
  libraries.
- Rust's standard library parses decimal floats with correct rounding
  (its implementation follows the Eisel-Lemire approach with a slow
  path). It accepts a different syntax from StringToNumber, so the engine
  must check the ECMAScript syntax itself and pass only valid decimal
  text. Its fixed-precision formatting is exact, but I did not check
  which rule it uses on exact ties; ECMA-262 wants the larger value, so a
  test must cover ties such as `(0.5).toFixed(0)` and `(2.5).toFixed(0)`.
- An exact implementation is easy to test: test262 has many cases, and
  Node.js gives a reference for differential tests.
- Inline small BigInt values (as in QuickJS) keep word-sized BigInt
  arithmetic free of allocation. A size limit bounds the time of
  quadratic algorithms. ADR 0003 decides whether an arbitrary-precision
  crate is acceptable; the QuickJS design shows that a small own
  implementation is feasible.

---

## 4.4 Garbage collection

### Problem

The engine must free objects, strings, closures, environments,
functions, shapes and BigInts that the program can no longer reach. It
must collect cycles: every ordinary function forms one (the function's
`prototype` object refers back to the function through `constructor`,
as Duktape's memory note points out). It must keep alive values that
native code holds, support `WeakMap`, `WeakSet`, `WeakRef` and
`FinalizationRegistry`, and stop pages that allocate without bound.

### What ECMA-262 requires

ECMA-262 does not require garbage collection, but it defines when
weakly held values may disappear (§9.9):

- §9.9.2 (Liveness) defines a set of objects or symbols as live if an
  agent's kept-alive list contains one of them or if some possible future
  execution that ignores WeakRefs could observe the identity of one of
  them. A value stored in a WeakMap key, a WeakSet, a WeakRef target or
  a FinalizationRegistry token does not make it live. A WeakMap key that
  is not live does not keep its value live (this is the ephemeron rule,
  although the specification does not use the word). Liveness is
  undecidable; reachability is the usual approximation.
- §9.9.3 (Execution) says that when a set is not live, the
  implementation *may* atomically empty the WeakRefs to it, empty the
  WeakMap entries and WeakSet elements for it, empty the
  FinalizationRegistry cells for it, and then schedule cleanup. It need
  not do so. If it empties one WeakRef to a set, it must empty all.
- §9.10 (ClearKeptObjects) and §9.11 (AddToKeptObjects): the WeakRef
  constructor and `WeakRef.prototype.deref` (§26.1.3.2) add the target
  to the kept-alive list, so a deref within one synchronous run returns
  the same object. The list is cleared when a synchronous sequence of
  executions completes. The HTML Standard does this in the "perform a
  microtask checkpoint" algorithm (§8.1.7.3).
- §9.9.4.1 (HostEnqueueFinalizationRegistryCleanupJob): the host
  schedules cleanup callbacks. The HTML Standard (§8.1.6.6.2) queues a
  global task on the JavaScript engine task source, so cleanup never
  interrupts synchronous code.
- §9.13 (CanBeHeldWeakly): any object, and any symbol not created by
  `Symbol.for`, can be a weak key or target.

### Options in the engines

**Reference counting with a trial-deletion cycle collector (QuickJS,
QuickJS-ng).** Every heap value has a count. When a count reaches zero
the engine frees the value. To keep native stack depth bounded, freed
objects go to a work list that one loop drains, instead of recursion.
Objects whose class can hold references (objects, function bytecode,
suspended async frames, modules) are also on a list of all collectable
objects. The cycle collector uses the trial-deletion idea published by
Bacon and Rajan (2001, after Lins): if the references that collectable
objects hold to each other are subtracted from the counts, any object
whose count stays above zero must be referenced from outside the graph
(the VM stack, native code, a global structure). Such objects and
everything they reach are live and get their counts back; the rest are
garbage cycles.

Because outside references show up as leftover counts, the collector
needs no root set. This is the main reason QuickJS uses this design: its
documentation says that native code never has to register roots. The
cost: every copy of a value into a new place must increment a count and
every release must decrement it; errors in counting give leaks or
use-after-free; every collection visits every collectable object, not
only the live ones; and every native class that holds JavaScript values
must provide a function that lists them, or the collector misses the
edges. Leaf values (strings, BigInts, symbols) do not take part in cycle
detection. Collection runs when the allocated byte count passes a
threshold, and the new threshold is set to 1.5 times the size that
survives (starting at 256 KiB). In QuickJS the check happens only at
object creation.

**Reference counting plus mark-and-sweep (Duktape).** Reference counts
free most garbage at once and keep memory use flat; a stop-the-world
mark-and-sweep collector handles cycles and also runs in an emergency
when an allocation fails. Points from the design notes: marking recurses
only to a depth limit, and deeper objects are found later by rescanning
the heap; the sweep needs two passes (adjust counts, then free), because
one pass would touch freed memory when garbage forms cycles;
unreachable objects with finalizers survive one more round so that the
finalizer sees intact children; the voluntary trigger is the number of
survivors times a factor, which can be large (about 10) because counting
already frees non-cyclic garbage. The notes also name a problem of every
register-based VM: a temporary register keeps its last value reachable
until it is reused or the function returns.

**Mark-and-sweep (MuJS).** Plain stop-the-world mark-and-sweep. Separate
lists hold objects, functions, environments and strings. Marking uses an
explicit work list for objects, so deep object graphs do not recurse on
the native stack. The mark value alternates between two numbers on each
collection, so no pass is needed to clear marks. The roots are: the
built-in prototypes, the global object, a hidden registry object for
host values, the VM value stack and the environment stack. The trigger:
an allocation counter compared with the number of survivors times 5
(about 80% garbage at each collection). Collection runs only at the top
of the interpreter loop, between instructions.

**Mark-compact (MicroQuickJS).** The engine owns one fixed memory
buffer. The heap grows upward from the bottom with bump allocation; the
VM stack grows downward from the top. When the gap is too small for an
allocation, the engine collects; if the gap is still too small, it throws
an out-of-memory error. Collection has three steps:

1. Mark, with an explicit mark stack placed in the free gap, so marking
   needs no extra memory. On mark-stack overflow, the collector later
   rescans the heap for marked objects (the standard recovery). Large
   arrays are scanned a few elements at a time.
2. Sweep: unmarked blocks become free blocks, host finalizers run, and
   weak tables (interned strings, the position cache) drop unmarked
   entries.
3. Compact with Jonkers' sliding algorithm (1979): references are
   threaded into chains through their targets, then two heap passes
   update every reference and slide live blocks down, in order, without a
   forwarding table. Property hash tables hash on key addresses, so all
   are rehashed afterwards.

The README names the benefits (no fragmentation, smaller headers than
reference counting) and the cost: any allocation may move any object.

**Generational collection.** None of the five engines is generational.
The GC Handbook (Jones, Hosking, Moss) describes the design: most objects
die young, so a young generation is collected often; a write barrier
records old-to-young references. Incremental collection also needs a
barrier. Both make every reference store more expensive.

### How host (native) code keeps values alive

The engines use four models:

1. **Counted references (QuickJS).** Native code owns a count for every
   value it holds; it must increment when it keeps a value and decrement
   when it drops it. Nothing has to be registered. The QuickJS
   documentation sets the convention: arguments are borrowed, results are
   owned.
2. **The VM stack as the only handle (MuJS, Duktape).** Native functions
   never hold values directly; they push values on the VM value stack and
   refer to them by stack position. The stack is a root, so everything on
   it is alive. To keep a value beyond one call, the host stores it in a
   hidden object that is a root (MuJS has a registry object with string
   keys that the API generates; Duktape has hidden objects for the same
   purpose). This is the model of the Lua C API.
3. **Registered root slots (MicroQuickJS).** Native code may hold values
   across an allocating call only in slots registered with the engine:
   short-lived slots in last-in, first-out order (a scope), long-lived
   ones in a second list with explicit removal. The collector updates
   the slots when objects move. A debug mode moves every object at every
   allocation to expose mistakes. This is the split into scoped and
   persistent handles that embedding APIs of large engines also use.
4. **Stable pointers plus reachability (Duktape internals).** Objects
   never move, so internal code holds plain "borrowed" pointers while
   some reachable path keeps the object alive. The side-effects note
   calls such assumptions a frequent source of bugs, because collections
   and finalizers can run at almost any allocation.

### When collection may run

- **At any allocation, with side effects (Duktape).** Collection or a
  count reaching zero may run finalizers, which run arbitrary
  JavaScript. The side-effects note lists the consequences: resizable
  buffers (value stack, property tables) may move, so native code must
  re-read pointers after any call that may allocate; critical sections
  set flags that forbid finalizers or compaction.
- **At any allocation, with moving (MicroQuickJS).** Native code may
  hold values across allocations only in registered slots.
- **At object creation only (QuickJS).** Counts protect outside
  references. Host finalizers run at any release that reaches zero and
  must not run JavaScript.
- **At instruction boundaries only (MuJS).** A native function can
  allocate freely without rooting, unless it calls back into JavaScript;
  then the interpreter loop runs and may collect, so the values it still
  needs must be on the VM stack.

All five engines collect in one thread with the program stopped; none is
incremental or concurrent. Duktape, QuickJS and MicroQuickJS have a
build option that collects (or moves objects) at every allocation,
because rooting errors show up only when a collection happens at the
wrong moment.

### Weak references and ephemerons

Only QuickJS and QuickJS-ng implement `WeakMap`, `WeakSet`, `WeakRef`
and `FinalizationRegistry`. Duktape has its own finalizers and an
internal weak cache; MuJS and MicroQuickJS have no weak collections, but
MicroQuickJS and Duktape hold internal tables weakly.

- **QuickJS.** A weak target has a separate weak count. When its strong
  count reaches zero, its contents are freed but its header stays while
  weak references exist, so holders can see that it is dead. At the
  start of each collection the engine removes WeakMap entries, WeakRef
  targets and registry cells whose target has a strong count of zero,
  and queues registry callbacks as jobs. A WeakMap holds its values as
  children of the map. From the code, a value that refers to its own key
  keeps the key alive as long as the map lives; I did not test this.
- **QuickJS-ng.** Each weak target keeps a list of records that point
  back to the entries, WeakRefs and cells that refer to it. During cycle
  detection, WeakMap values count as children of the *key*, not of the
  map, so a value that refers to its own key forms an ordinary cycle
  with it and is collected. When a key dies, the engine first unlinks and
  then frees its entries, so that freeing does not change the list it
  walks.
- **Kept-alive list.** I found no implementation of the kept-alive list
  of §9.10 and §9.11 in either engine. With reference counting, a WeakRef
  in QuickJS reports its target as gone as soon as the strong count is
  zero, so `new WeakRef({}).deref()` may differ from the specification. I
  did not run test262 to confirm.

The literature: Hayes (1997) defines ephemerons, and the GC Handbook
describes how a tracing collector handles them. Marking does not trace
through weak collections but records the entries it meets. After the
normal marking, the collector repeatedly marks the value of every
recorded entry whose key is marked, until a round marks nothing new.
Then it clears entries with unmarked keys, WeakRef targets and registry
cells that are unmarked, and queues cleanup for the affected registries
(the host hook decides when it runs).

The fixpoint loop can be quadratic in the worst case. The QuickJS-ng idea
of a list on the key gives a linear alternative: when the marker marks a
key, it also pushes the values of all entries for that key.

### Heap limits

QuickJS counts allocated bytes and fails allocations above a
configurable limit with an out-of-memory exception (its newest version
also has its own size-class allocator for blocks up to 512 bytes).
MicroQuickJS is limited by its one buffer. Duktape relies on allocator
failure, runs an emergency collection, then throws. MuJS counts
allocations against a budget without crediting frees, so its limit is an
allocation quota, not a heap size.

### Trade-offs

| Approach | Roots in native code | Pauses | Cycles | Fit for index handles in safe Rust |
|---|---|---|---|---|
| Counting + trial deletion | none, counts carry them | short, but each cycle collection visits all collectable objects | yes | poor: every copy of a handle needs a count update, and dropping a handle needs access to the heap |
| Counting + mark-and-sweep | stack or stable pointers | short, plus full collections | yes | poor, for the same reasons |
| Mark-and-sweep | yes | proportional to the heap | yes | good |
| Mark-compact | yes, and slots must be updated | longer (several heap passes) | yes | possible, but compaction of an index heap is optional |
| Generational | yes, plus write barrier | short for young collections | yes | later option; the barrier is a check on stores |

### For swb

**Collector.** A handle indexes a table (perhaps one per kind: objects,
strings, environments). Slots are fixed size; variable-size data
(property and element vectors, string buffers) is owned by the slot and
freed by Rust when the sweep empties the slot; free slots are reused
from a free list. So fragmentation, the main reason for MicroQuickJS's
compaction, mostly disappears, and mark-and-sweep over the tables is the
direct fit:

- Mark bits in bit vectors separate from the slots: the marker reads
  slots through a shared borrow and writes only the bit vectors.
- An explicit work list of handles instead of recursion (MuJS,
  MicroQuickJS), with resume positions for large arrays.
- A generation counter per slot ("generational index") detects a stale
  handle whose slot was freed and reused. In safe Rust a stale handle is
  not memory-unsafe, but it gives wrong results; detection turns a
  rooting bug into a clear test failure.
- Interned names and caches as weak tables, cleaned after marking.
- Stable handles: if tables were ever renumbered, every hash table keyed
  by handle would need a rebuild, as MicroQuickJS rehashes after
  compaction.

Reference counting is a poor fit: a handle type that updates a count on
copy and drop needs mutable access to the heap wherever a value is
copied, and cycles still need a second collector.

**Roots and host-held values.** The VM's own state gives precise roots:
the value stack (or register file) of every frame, including suspended
generator and async frames, the realms and their intrinsics, the job
queues, the module map and the atom constants. Native (Rust) code is the
hard part. The engine models above give these options:

1. *Stack-only API* (MuJS, Duktape): Rust built-ins use only values on
   the VM stack, addressed by position, so every live value is a root by
   construction. Less direct, but simple and safe.
2. *Scoped root slots* (MicroQuickJS): a built-in registers the values
   it keeps across a possible collection in a scope that ends when it
   returns. In Rust the scope can be a guard value that pops the slots
   when it goes out of scope. The risk is a handle copied out of the
   scope into a plain variable; a generational index catches that in
   stress tests.
3. *Persistent roots* (MicroQuickJS's second list, MuJS's registry): for
   values the host keeps across tasks (listeners, timers, promise
   callbacks). Each needs an owner that removes it, or the host leaks.
4. *Host structures traced by the collector* (QuickJS asks each native
   class to list its references): if DOM nodes that hold JavaScript
   values are persistent roots, DOM–JavaScript cycles are never
   collected; if the collector can ask the DOM for its references, they
   are. Memo 5 covers wrappers; this choice decides whether such cycles
   leak.

**When collection may run.** The models above give these choices:

- *Only at safepoints in the interpreter loop* (MuJS), for example at
  back-edges, calls, or when an allocation counter passes its threshold.
  Rust built-ins that do not call JavaScript need no rooting, because no
  collection can happen during them; the borrow checker can enforce this
  if the built-in holds a mutable borrow of the heap and the collector
  needs one too. Built-ins that call JavaScript (sort comparators,
  getters, `valueOf`, `toJSON`) must root what they keep across the
  call. A built-in that allocates much without calling JavaScript (a
  large `join` or `repeat`) cannot collect, so the heap limit must also
  be checked at allocation, with an error instead of a collection.
- *At any allocation* (Duktape, MicroQuickJS, QuickJS for objects):
  tighter memory, but every Rust built-in must root everything across
  every allocation. With stable index handles this is easier than in
  MicroQuickJS, but a missed root still frees a live object.
- *Only between tasks*, when the JavaScript stack is empty: roots are
  only globals, queues and persistent roots. Not enough alone, because
  one task can allocate without bound, but a cheap extra point.

The safepoint model keeps most Rust built-ins free of rooting work and
matches a VM whose JavaScript-to-JavaScript calls do not recurse on the
Rust stack (ADR 0025). Whatever the choice, swb needs a stress mode that
collects at every safepoint or allocation, as Duktape, QuickJS and
MicroQuickJS have.

**Weak references.** In a tracing heap the ephemeron algorithm above
(fixpoint, or per-key lists) follows §9.9.2. The kept-alive list is a
vector of handles, cleared at the end of each microtask checkpoint
(HTML §8.1.7.3), and it is a root. FinalizationRegistry cleanup goes
through the host hook as a task (HTML §8.1.6.6.2), never during the
collection. Host finalizers (Rust `Drop` of data owned by a slot) run
during the sweep; they must not touch the JavaScript heap, as QuickJS
also requires of its native finalizers.

**Heap limit and triggers.** Count bytes per table and per owned buffer
(as QuickJS counts bytes in its allocator), trigger collection when the
count passes a multiple of the surviving size (QuickJS uses 1.5, MuJS 5
by object count), and throw an out-of-memory error when a collection
cannot get under the limit. Memo 1 covers how the error reaches the
script and the host.

---

## Sources

### Engines

All engines were read from the copies in the session scratchpad. None was
built or run.

**QuickJS** (Fabrice Bellard), commit
535a7c250ff4a577ec36c3e103daab6dadeea650 (2026-09-29), VERSION
2026-06-04. Files read:

- `doc/quickjs.texi`: chapters on the C API and on internals.
- `quickjs.h`: the value representation part (tags, NaN-boxing and
  struct variants, number constructors).
- `quickjs.c`: string and rope types and functions (concatenation, rope
  creation, flattening and rebalancing, in-place append, single-character
  strings, the string buffer); atom functions (integer atoms, the numeric
  string test, atom creation and freeing); the allocator size classes and
  the collection trigger; freeing at count zero, the zero-count work list,
  the cycle collector and the collection entry point; WeakRef, WeakMap
  and FinalizationRegistry liveness and cleanup, Map marking; the BigInt
  section (limits, multiplication, division, conversion to string); the
  interpreter's add and multiply fast paths.
- `dtoa.h`, `dtoa.c`: whole conversion library header, the shortest,
  fixed and fractional output paths, parsing, digit-limit tables.
- `Changelog`: searched for entries on weak references, BigInt and the
  collector.

**QuickJS-ng**, commit c359cac36372e71daade8602908d29bdc259b41e
(2026-10-09). Files read:

- `quickjs.h`: the value representation part.
- `quickjs.c`: string kinds and slices, rope constants, the collection
  trigger, weak-reference records, marking of WeakMap values from the
  key, removal of weak records, BigInt limits, call sites of the number
  conversions, the JSON number parser.
- `dtoa.c`: compared with QuickJS (small differences only).

**MuJS** (Artifex), commit aab59f2c8136b55e8eef7a016d5a15fe8cf69f3f
(2026-10-06), codeberg.org/ccxvii/mujs. Files read:

- `jsi.h`: limits, the value and string types, state fields used by the
  collector.
- `jsgc.c`: whole file.
- `jsintern.c`: whole file.
- `jsrun.c`: allocation and the memory budget, the registry functions,
  the array-index parser, the head of the interpreter loop.
- `jsstring.c`: UTF-16 index helpers and substring.
- `utf.c`: the NUL encoding.
- `jsnumber.c`: Number prototype methods.
- `jsdtoa.c`: the Grisu2 part and the start of the `strtod` part.

**Duktape**, commit 3afa016bbe6eb13418fba6eaca003c660bc4c241
(2026-09-05), master branch (3.0 development). Files read:

- Design notes in `doc/`: `types.rst`, `fastint.rst`,
  `memory-management.rst`, `string-table.rst`,
  `utf8-internal-representation.rst`, `number-conversion.rst`,
  `symbols.rst`, `side-effects.rst` (first part), `low-memory.rst`
  (outline and the pointer-compression section),
  `release-notes-v3-0.rst` (string representation changes).
- `src-input/duk_tval.h` (header comment and the start of the compact
  representation), `src-input/duk_heap_stringcache.c` (first half),
  `src-input/duk_hstring.h` and `src-input/duk_heap.h` (searched for
  flags and cache size), `config/config-options/DUK_USE_PACKED_TVAL.yaml`.

**MicroQuickJS** (Fabrice Bellard, Charlie Gordon), commit
6d4d7eb6b99011d5aab6e88a9b99e8fd4d86c6e9 (2026-09-26). License MIT,
checked by the orchestrator. Files read:

- `README.md`: whole file.
- `mquickjs.h`: the value representation part.
- `mquickjs.c`: memory block and string headers, the allocator and the
  free-memory check, root registration, the interned-string table and its
  lookup, property-table rehashing, the whole collector (mark, sweep,
  weak tables, compaction).
- `dtoa.c`: compared with QuickJS (same library).

### Specifications

- ECMA-262, current draft, https://tc39.es/ecma262/ — §6.1.4 (String
  type), §6.1.6.1 (Number type) and §6.1.6.1.20 (Number::toString),
  §6.1.6.2 (BigInt type), §6.1.7 (array index, integer index), §7.1.4.1.1
  (StringToNumber), §7.1.4.1.3 (RoundMVResult), §7.1.23
  (CanonicalNumericIndexString), §9.9 to §9.13 (WeakRef and
  FinalizationRegistry processing model, ClearKeptObjects,
  AddToKeptObjects, CanBeHeldWeakly), §19.2.4 and §19.2.5 (parseFloat,
  parseInt), §21.1.3.2, §21.1.3.3, §21.1.3.5, §21.1.3.6 (Number
  prototype methods), §21.2.3.3 (BigInt.prototype.toString), §22.1.3.5,
  §23.1.3.18, §24.3, §24.4, §25.1.3.17 (NumericToRawBytes), §26.1, §26.2.
- HTML Standard, https://html.spec.whatwg.org/multipage/webappapis.html —
  §8.1.6.6.2 (HostEnqueueFinalizationRegistryCleanupJob), §8.1.7.3
  (perform a microtask checkpoint, which calls ClearKeptObjects).

### Literature

- R. Jones, A. Hosking, E. Moss: *The Garbage Collection Handbook*, 2nd
  edition, CRC Press, 2023 (mark-sweep, mark-compact, reference counting,
  generational and incremental collection, weak references).
- D. F. Bacon, V. T. Rajan: "Concurrent Cycle Collection in Reference
  Counted Systems", ECOOP 2001, LNCS 2072 (trial deletion).
  https://pages.cs.wisc.edu/~cymen/misc/interests/Bacon01Concurrent.pdf
- B. Hayes: "Ephemerons: A New Finalization Mechanism", OOPSLA 1997,
  pp. 176–183.
  https://static.aminer.org/pdf/PDF/000/522/273/ephemerons_a_new_finalization_mechanism.pdf
- H. B. M. Jonkers: "A fast garbage compaction algorithm", Information
  Processing Letters 9(1), 1979. Preprint: https://ir.cwi.nl/pub/9391/9391D.pdf
- H.-J. Boehm, R. Atkinson, M. Plass: "Ropes: an Alternative to
  Strings", Software: Practice and Experience 25(12), 1995, pp. 1315–1330.
- D. Gudeman: "Representing Type Information in Dynamically Typed
  Languages", University of Arizona, TR 93-27, 1993 (survey of tagging
  schemes). Description: https://compilers.iecc.com/comparch/article/93-04-116
- G. L. Steele Jr., J. L. White: "How to Print Floating-Point Numbers
  Accurately", PLDI 1990.
- R. G. Burger, R. K. Dybvig: "Printing Floating-Point Numbers Quickly
  and Accurately", PLDI 1996.
- D. M. Gay: "Correctly Rounded Binary-Decimal and Decimal-Binary
  Conversions", AT&T Bell Laboratories, 1990; code at
  https://www.netlib.org/fp/
- W. D. Clinger: "How to Read Floating Point Numbers Accurately", PLDI
  1990.
- F. Loitsch: "Printing Floating-Point Numbers Quickly and Accurately
  with Integers", PLDI 2010.
- U. Adams: "Ryū: fast float-to-string conversion", PLDI 2018,
  https://doi.org/10.1145/3192366.3192369
- D. Lemire: "Number Parsing at a Gigabyte per Second", Software:
  Practice and Experience 51(8), 2021, https://arxiv.org/abs/2101.11408
- J. Champagne Gareau, D. Lemire: "Converting Binary Floating-Point
  Numbers to Shortest Decimal Strings: An Experimental Review", 2026,
  https://arxiv.org/abs/2603.06581 (covers Schubfach and Dragonbox).
- D. E. Knuth: *The Art of Computer Programming*, vol. 2, *Seminumerical
  Algorithms*, 3rd edition, §4.3 (multiple-precision arithmetic,
  Algorithm D).
- R. P. Brent, P. Zimmermann: *Modern Computer Arithmetic*, Cambridge
  University Press, 2010 (subquadratic multiplication, division and
  radix conversion).
