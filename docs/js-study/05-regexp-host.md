# Study memo 5: regular expressions, dates, embedding

Status: reviewed by the orchestrator on 2026-10-09 (ADR 0025).

Study memo for swb's JavaScript engine (ADR 0025), group 5, questions 1
to 3. It compares QuickJS, QuickJS-ng, MicroQuickJS, MuJS and Duktape,
and the literature. It describes options and trade-offs. It does not
decide swb's design.

Terms used in this memo:

- "Choice point": the state (program position, input position) that a
  backtracking matcher saves and returns to after a failure.
- "Undo log" (in logic programming, a "trail"): the old values of
  capture slots and counters, written before each change and restored in
  reverse order on backtracking.
- "Pike VM" (Thompson NFA simulation): a matcher that advances all
  alternatives in lock step over the input (Cox, articles 1 and 2).
- "Wrapper": the JavaScript object that represents a host object (for
  example a DOM node).

Engine names: "QuickJS" means both QuickJS and QuickJS-ng where they
agree. The memo names QuickJS-ng or MicroQuickJS separately where they
differ.

---

## 5.1 Regular expressions

### The problem

A JavaScript regular expression engine must:

1. Parse the pattern with the grammar of ECMA-262 §22.2.1, including the
   web-compatibility grammar of Annex B.1.2 when neither the `u` nor the
   `v` flag is set, and report early errors (§22.2.1.1; for literals in
   source text, §13.2.7.2).
2. Produce exactly the match that the specification defines. §22.2.2
   defines the semantics as a backtracking search written in
   continuation-passing style: alternatives are tried left to right,
   greedy quantifiers try "one more" before "stop", lazy quantifiers the
   reverse. The first successful path wins. Any engine design must give
   the same result as this search, including the values of all captures.
3. Work on strings of UTF-16 code units, and in `u` and `v` modes on code
   points (surrogate pairs decoded on the fly).
4. Support lookahead, lookbehind, backreferences, named groups, case
   folding, Unicode properties and (in `v` mode) set operations and
   strings in classes.
5. Not hang or crash the host on hostile patterns or inputs.

The specification answers all questions of observable behaviour. The
engine questions are: what form the compiled pattern takes, how the
matcher keeps its choice points and captures, how it handles quantifiers
with counts, and how it bounds time and memory.

### What the specification fixes (sections)

- Pattern semantics, matchers and continuations: §22.2.2.1 to §22.2.2.3.
- Quantifiers: RepeatMatcher, §22.2.2.3.1. Two details matter for
  engines. Each iteration clears the captures inside the quantified atom
  before it runs the atom again. After the minimum count is reached, an
  iteration that matches the empty string fails (the "empty check").
- Sequences and direction: MatchSequence, §22.2.2.3.4. In a lookbehind
  the direction is backward, and the terms of a sequence are matched
  right to left.
- Assertions, including lookahead and lookbehind: §22.2.2.4. Lookarounds
  are atomic: after a lookaround succeeds, the matcher does not backtrack
  into it. Captures from a successful positive lookaround stay set. A
  negative lookaround never sets captures.
- Backreferences: BackreferenceMatcher, §22.2.2.7.2. A reference to a
  group that did not participate matches the empty string. In backward
  direction the comparison runs backward.
- Case-insensitive matching: Canonicalize, §22.2.2.7.3. Without `u` or
  `v`: map each code unit with the Unicode toUppercase operation, but
  keep the character if the result is more than one character, or if
  the mapping goes from non-ASCII to ASCII. With `u` or `v`: simple or
  common case folding from CaseFolding.txt.
- Character classes and the `v` mode: CompileCharacterClass and
  CompileToCharSet, §22.2.2.8 and §22.2.2.9, with MaybeSimpleCaseFolding
  (§22.2.2.9.5) and CompileClassSetString (§22.2.2.10). Classes with
  strings match the longest string first.
- Unicode properties: UnicodeMatchProperty and UnicodeMatchPropertyValue,
  §22.2.2.9.7 and §22.2.2.9.8 (the tables in the specification list the
  property names and aliases that must be accepted).
- Inline modifiers such as `(?i:...)`: UpdateModifiers, §22.2.2.7.4.
- The search loop over start positions, `lastIndex`, sticky and global
  flags: RegExpBuiltinExec, §22.2.7.2.
- Web compatibility syntax (braces as literals, legacy octal escapes,
  quantified lookahead, `\k` without named groups): Annex B.1.2.

The legacy static properties (`RegExp.$1`, `lastMatch` and others) are
not in ECMA-262. They are a TC39 proposal. None of the studied engines
implements them. Browsers implement them; swb must measure Chromium if a
target page uses them.

### Options

#### Compiled form

All five engines use a backtracking matcher. None of them uses a Thompson
NFA simulation or a DFA. The Duktape design notes give the reason that
the literature also gives (Cox, article 1 and 3): counted repetition,
assertions, captures and above all backreferences make an automaton
design hard or impossible. Backreferences make matching NP-complete in
general, so no automaton handles them in linear time.

The engines differ in how they get from the pattern text to the program:

- **No tree; emit and patch (QuickJS, MicroQuickJS, Duktape).** The
  parser emits bytecode while it parses. When it later learns that the
  code just emitted must be preceded by something (an alternation, a
  quantifier), it inserts bytes before the emitted code and moves the
  rest. Jumps are relative to the current position, so moved code stays
  valid. The QuickJS documentation states that it has no parse tree for
  patterns. The cost is quadratic copying in pathological patterns;
  real patterns are short. Duktape notes one more cost: with
  variable-length jump encodings, a backward jump's own length changes
  its offset, so the compiler must compute the offset in two steps.
  QuickJS avoids this with fixed-width jump offsets.
- **Tree first (MuJS).** MuJS parses into a small tree of nodes, counts
  the program size from the tree, rejects the pattern if the program
  would be too large, and then emits an array of instructions. Each
  instruction refers to its successors directly. The tree makes size
  checks and analyses (for example "can this atom match the empty
  string?") simple.

Both forms work in safe Rust. A tree costs one allocation per node, but
patterns are small. A tree also makes analyses easier, which matters for
the empty check and for limits on program size (see below).

Besides the instructions, a compiled pattern carries the metadata that
the exec builtin needs, for example the group names. Duktape stores the
compiled program as an internal string in a hidden property of the
RegExp object. QuickJS and Duktape compile a regular expression literal
when the script is compiled, so syntax errors are early errors.

#### Start-position loop

The specification tries each start position in turn (§22.2.7.2). QuickJS
and MuJS instead put a lazy "skip any character" loop in front of a
non-sticky program, so one run of the matcher covers all start positions
(this form also suits a later lock-step mode). Duktape loops over start
positions outside the matcher. The results are the same.

#### Choice points and recursion

- **Host recursion per choice point (MuJS, Duktape).** MuJS calls its
  matcher recursively for every alternative; the depth limit is 4,096.
  Duktape also recurses, for alternatives, for capture saves and for
  quantifier iterations, with a configurable depth limit (default
  10,000). Duktape reduces recursion for one common case: a quantifier
  whose atom has no alternatives, no captures, no lookaheads and a fixed
  length in characters (for example `.*` or `[a-f]+`). For such an
  atom, a greedy loop can match as many as possible and then give back
  one atom length at a time, without any saved state.
- **Explicit stack (QuickJS, MicroQuickJS).** The current QuickJS matcher
  is a loop with a growable array as its stack. A choice point pushes the
  program position, the input position and a link to the previous
  choice point. Every change of a capture slot or a counter pushes an
  undo entry. On failure the matcher pops undo entries back to the last
  choice point, restores them, and resumes at the saved position. The
  QuickJS documentation states the purpose: no recursion on the system
  stack. A small fixed buffer inside the matcher context avoids an
  allocation for most patterns. MicroQuickJS does the same on the
  engine's own value stack.

Capture restore has three forms in the engines:

- Undo log (QuickJS, MicroQuickJS): cost proportional to the number of
  changes. QuickJS skips a second undo entry for a counter that was
  already saved since the last choice point.
- Save in the recursion frame (Duktape): each capture save recurses once
  so that the old value can be restored on return.
- Copy the whole capture array at each choice point (MuJS): simple, and
  cheap only because MuJS limits the array to 16 slots.

#### Counted quantifiers

`a{n,m}` with large counts is the main source of program size.

- **Expansion (Duktape, MuJS).** The compiler copies the atom's code
  `n` times, then adds `m - n` optional copies. Both engines put a cap on
  it: Duktape limits the number of copies (default 1,000); MuJS limits
  counts to below 255 and the program to about 32,000 instructions.
  Expansion is easy to get right: copied code reuses the same capture
  slots, so later iterations overwrite earlier captures, as the
  specification requires.
- **Counter registers (QuickJS, MicroQuickJS).** The compiler emits a
  loop with a counter. The counter lives in a slot next to the capture
  slots, so the undo log restores it on backtracking. Because quantifiers
  nest properly, the compiler assigns counters like a stack, by nesting
  depth; QuickJS allows at most 255 nested counted loops and 255
  capture groups. Program size stays proportional to the pattern size,
  independent of the counts. The cost is a counter write per iteration.

#### Empty check and capture reset (RepeatMatcher)

The specification requires both on every iteration (§22.2.2.3.1). The
engines differ:

- QuickJS analyses the atom when it compiles the quantifier. If every
  path through the atom consumes at least one character, it omits the
  empty check. Otherwise it records the input position at the start of
  the iteration and fails the iteration if the position did not change.
  It inserts a "clear captures in this range" step at the start of each
  iteration when the atom contains captures (and once before the loop
  when the minimum is 0, for the case that the atom never runs).
- Duktape clears the capture slots of the atom on each iteration, with
  restore on backtrack. It has no empty check for general quantifiers;
  its notes say that an atom that can match empty under `*` loops until
  the step limit stops it. For fixed-length atoms of length 0 it caps the
  count at 1.
- MuJS rejects, at compile time, an unbounded quantifier over an atom
  that can match the empty string. This rejects
  valid patterns such as `(a*)*b`. I did not find a capture reset per
  iteration in MuJS.

The QuickJS approach is the only one of the three that follows the
specification without exceptions. It needs a small static analysis of
the atom (can it match empty; does it initialise all its captures).

#### Lookahead and lookbehind

- Lookahead in a backtracking matcher: run the sub-pattern as a nested
  match at the current position, then discard the choice points inside
  it (atomicity). QuickJS marks the choice point at the start of the
  lookahead. When the lookahead body reaches its end, the matcher drops
  all choice points down to that mark but keeps their undo entries, so
  that captures set inside a positive lookahead stay set but are still
  undone if an outer alternative fails. A negative lookahead that reaches
  its end restores everything to the mark and then fails. MuJS and
  Duktape do the same with a recursive call.
- Lookbehind exists only in QuickJS (Duktape and MuJS follow the ES5
  grammar). QuickJS follows the specification's direction model: inside
  a lookbehind it compiles the terms of each sequence in reverse order,
  and each character test steps one character back before it tests. A
  capture in backward direction records its end before its start.
  Backreferences in backward direction compare backward. Because the
  sub-pattern can contain arbitrary quantifiers, this gives the exact
  semantics of §22.2.2.3.4 and §22.2.2.4, including greedy behaviour
  from the right, without any limit on the lookbehind length. The
  TC39 lookbehind proposal describes the same model.

#### Backreferences

All engines compare the captured text with the input at the current
position, character by character, with case folding when `i` is set.
QuickJS handles duplicate named groups (allowed in different
alternatives since ES2025) by emitting one backreference test over the
list of all groups with that name; the first group that participated is
used. MuJS rejects references to groups that are not yet closed at the
point of the reference; the specification allows forward references and
references from inside the group (they match empty). Duktape and QuickJS
allow them.

#### Case-insensitive matching

Two strategies, both found in the engines:

- **Canonicalize the class at compile time, the input at run time
  (QuickJS, Duktape).** A naive approach, canonicalizing only the end
  points of a range, is wrong: one input range can map to several output
  ranges (Duktape's notes show this with a range that crosses the end of
  the lowercase letters). Duktape walks every code point of every range
  in the class at compile time and builds the canonical ranges; this is
  slow to compile for large classes but needs no extra tables. QuickJS
  splits the class into the part that case mapping changes and the part
  it does not change (with set operations against a precomputed set of
  "case-sensitive" characters), maps only the first part, and joins the
  results. At run time both engines canonicalize each input character
  once and test it against the canonical class.
- **Canonicalize at match time only (MuJS).** For each input character,
  MuJS walks every code point of every range in the class and compares
  canonical forms. The cost per character is proportional to the size of
  the ranges. This is acceptable only for small classes.

Built-in classes such as `\w`, `\s` and `.` need no case processing when
their ranges are already closed under Canonicalize. Duktape's notes
point out that this holds for `\w` only because Canonicalize refuses to
map non-ASCII to ASCII. With `u` and `i`, it no longer holds: `\w` and
`\b` must also accept the two non-ASCII characters whose case folding is
an ASCII letter (§22.2.2.9.3, WordCharacters). QuickJS special-cases
them.

Case tables: QuickJS has its own compressed Unicode tables, generated
from the Unicode Character Database by a build-time tool, with fast paths
for ASCII. Duktape packs case rules into a bit stream (ranges with a
fixed step, single mappings, multi-character mappings) and offers an
optional 128 KB lookup table for speed. MicroQuickJS folds only ASCII
(documented deviation).

#### The `u` and `v` modes

- Duktape and MuJS support only the ES5 flags (`g`, `i`, `m`).
  MicroQuickJS accepts `u`, `s` and `y` but not Unicode properties, and
  always matches by code point.
- QuickJS supports `u`, `v`, `d`, `s`, `y` and inline modifiers.
  - Matching: in `u` and `v` mode the matcher decodes surrogate pairs when
    it reads a character, and steps over a whole pair when it moves back.
    It runs on both string widths (8-bit and 16-bit) with the same
    program.
  - Properties: compressed tables for general categories, scripts,
    script extensions and binary properties, from the UCD.
  - `v` mode: a class is a pair of a code point set and a set of
    strings. Union, intersection and difference are computed at compile
    time on both parts. A class with strings compiles to an ordered
    alternation: strings from longest to shortest, then the single
    characters (as §22.2.2.10 and the class semantics require). With `i`,
    the strings and the code point set are case-folded first
    (§22.2.2.9.5).

#### Bounding backtracking

A backtracking matcher has exponential worst cases ("catastrophic
backtracking", ReDoS): for example nested quantifiers such as `(a*)*b`
on a long run of `a` without `b`. The engines and the literature use
these mechanisms:

| Mechanism | Engines | Effect |
|---|---|---|
| Host recursion depth limit | MuJS (4,096), Duktape (10,000, compile and match) | Stops stack overflow; deep but valid patterns fail. |
| No host recursion (explicit stack) | QuickJS, MicroQuickJS | No stack overflow; stack memory counts against the heap limit. |
| Count of executed steps, fixed cap | Duktape (1e9 per match call, across all start positions), MuJS (1,048,576, only when the host set a run limit) | Hard cap; error or failure when reached. |
| Periodic call of the host's interrupt check | QuickJS, MicroQuickJS (every 10,000 backtracks or backward jumps) | Uses the same time budget as the rest of the script; the host decides. |
| Program size cap | MuJS, Duktape (copy limit) | Bounds compile time and memory. |
| Compile-time rejection of risky forms | MuJS (unbounded loops over atoms that can match empty) | Deviates from the specification. |

None of the engines uses an automaton fallback or memoization. The
literature has two more options:

- **Linear-time engine for a subset, with fallback.** A Thompson NFA or
  Pike VM matches in time linear in the input for patterns without
  backreferences and lookaround (Cox, articles 1 and 2). A browser engine
  team describes an experimental second engine of this kind that takes
  over when the backtracking engine exceeds a backtrack count (default
  50,000), but only for patterns without backreferences, lookaround,
  large counted repetition and without the `i` and `u` flags (V8 blog,
  2021). The Pike VM keeps the leftmost-first priority of JavaScript
  semantics by ordering its threads (Cox, article 2).
- **Memoization.** If the matcher records which pairs (program position,
  input position) already failed, it never explores a pair twice, and
  the time becomes proportional to program size times input length. Cox
  describes this as a "bit-state" backtracker for small inputs
  (article 3). Davis, Servant and Lee (2021) make it selective (only for
  loop entries and join points) to reduce memory. The pair is a correct
  key only when the rest of the match does not depend on other state:
  backreferences (the captures differ) and counter registers (the count
  differs) break this assumption, so these patterns need the full state
  in the key or no memoization.

For a browser, the time budget matters more than the exact cap: a page
that runs a slow regular expression should hit the same per-task time
limit as a slow loop, and the user can stop it. A step counter that
calls the host's time check is the common mechanism.

#### Capture representation

QuickJS and MicroQuickJS store captures as positions in the input.
QuickJS uses raw addresses; MicroQuickJS uses integer offsets, because
its garbage collector moves objects. When MicroQuickJS calls the
interrupt check (which can run the collector), it saves its positions as
offsets, re-reads the addresses of the program, the input and the
capture array afterwards, and continues. Duktape and MuJS work on UTF-8
input and must convert byte positions to UTF-16 indices for the result.

### Trade-offs

- Explicit stack versus recursion: the explicit stack needs no depth
  limit; it costs a manual undo log. Recursion is shorter but its depth
  limit rejects valid inputs (long alternations, many captures).
- Counters versus expansion: counters keep the program small and remove
  limits on counts; expansion keeps the matcher simpler.
- Compile-time class canonicalization moves the cost to the compiler;
  match-time canonicalization is simple but slow for large classes.
- A fixed step cap makes the error depend on the cap; an interrupt check
  makes it depend on machine speed.
- A linear-time fallback engine protects many common patterns against
  ReDoS, but it is a second matcher and cannot handle backreferences or
  lookaround.

### For swb

- The specification defines the semantics as a backtracking search, so a
  backtracking matcher (the ADR 0025 starting point) maps directly to
  the text. Every engine studied made this choice.
- Without host recursion, the matcher is a loop over a `Vec` of choice
  points and undo entries, all plain integers. The pattern parser still
  recurses on groups unless it has an explicit stack (MicroQuickJS); it
  needs a nesting limit either way.
- Positions as code-unit indices match the specification's index
  semantics and stay valid if the collector runs during the match
  (MicroQuickJS shows why this matters). The input string must stay
  rooted during the match. With two string widths (memo 4), the matcher
  runs on both (QuickJS) or reads through one accessor.
- Of the studied designs, only the QuickJS combination (undo log,
  counters, empty check, capture reset per iteration) meets the
  specification without exceptions. The atom analysis it needs is
  easier on a tree.
- Limits that fit the ADR 0025 starting points: a step counter that
  calls the same per-task time check as bytecode execution; the
  choice-point stack counted against the heap limit; caps on pattern
  nesting, program size and capture count; limit errors that behave like
  the other limit errors (memo 1).
- Unicode data (case folding, properties, properties of strings) must
  match the Unicode version that test262 expects: generated tables in
  the repository, or a data crate through the ADR 0003 process.
- Memoization or a linear-time fallback can come later if the program
  format keeps a (program position, input position) pair meaningful:
  fixed-width jumps and a separate counter area help.

---

## 5.2 `Date`

### The problem

A `Date` holds one number: milliseconds since 1970-01-01 UTC, without
leap seconds, in the range ±8.64e15, or NaN (§21.4.1.1). All calendar
arithmetic in UTC is pure arithmetic, defined in §21.4.1.3 to §21.4.1.16
and §21.4.1.27 to §21.4.1.31. Two parts depend on the host:

1. The offset of local time from UTC at a given instant, which includes
   daylight saving time (DST) and historical rule changes.
2. `Date.parse` of strings that are not in the date time string format.

### What the specification fixes

- LocalTime (§21.4.1.25): UTC to local time with the offset of the
  system time zone at that instant. Implementations that know time zones
  must use the IANA Time Zone Database (Note 2). An implementation
  without time zone data uses UTC.
- UTC (§21.4.1.26): local time to UTC. A local time that occurs twice
  (when DST ends) and a local time that does not exist (when DST starts)
  are both interpreted with the offset before the transition. The note
  gives an example: 1:30 on the day New York leaves DST is read as UTC−4.
  The round trip of LocalTime and UTC is not the identity.
- The system time zone: SystemTimeZoneIdentifier (§21.4.1.24), with the
  identifier rules of §21.4.1.19 and the named-zone operations of
  §21.4.1.20 and §21.4.1.21. ECMA-402 adds more if `Intl` is present.
- Date time string format (§21.4.1.32): `YYYY-MM-DDTHH:mm:ss.sssZ` and
  its date-only and shorter forms, with expanded six-digit years
  (§21.4.1.32.1), and 24:00 as the end of a day.
- Date.parse (§21.4.3.2): first try the date time string format. Without
  an offset, date-only forms are UTC and date-time forms are local time.
  If the string does not conform, the implementation "may fall back to
  any implementation-specific heuristics or implementation-specific date
  formats". It must parse what its own `toString` (§21.4.4.41) and
  `toUTCString` (§21.4.4.43) produce, back to the same value when the
  milliseconds are zero.

### Options: the local time offset

| Approach | Engine | Properties |
|---|---|---|
| One fixed offset, computed once at startup; DST adjustment always zero | MuJS | Wrong by the DST amount for about half of the year in DST zones. |
| Ask the C library for each instant | QuickJS | Correct for the range that the C library covers; on 32-bit platforms QuickJS clamps the instant to the 32-bit `time_t` range. |
| Ask the C library, but for the "current bias" only | QuickJS-ng on Windows | Same offset for all dates; wrong for dates in the other DST season. |
| Map the year to an "equivalent year" (same leap-year status, same weekday of 1 January) inside 1971 to 2037, then ask the C library | Duktape | ES5 allowed this mapping (Duktape cites ES5 §15.9.1.8). Historical rule changes are lost outside the mapped range. |
| Pluggable provider | Duktape (configuration hooks for "now", local offset, parse and format) | The host chooses; a provider that always returns zero gives UTC. |

The current specification text asks for the political rules in effect at
the instant and recommends the IANA database. The equivalent-year mapping
is an ES5-era technique that the specification no longer describes.

The IANA database and its binary form (TZif, RFC 9636) give, for each
zone, a list of transition instants with the offset after each, and a
rule string (POSIX TZ syntax) for instants after the last listed
transition. Looking up an offset is a binary search over the transitions,
or an evaluation of the rule for future dates. The IANA "theory"
document states that zones are defined by agreement of clocks after
1970, and that much of the data before 1970 (and for the future) is not
reliable.

### Options: local time to UTC

The inverse needs the offset at an instant that is not yet known.

- **One step (QuickJS, MuJS).** Treat the local value as if it were UTC,
  get the offset at that instant, and subtract it. The result is wrong in
  a window of the size of the DST change around each transition, because
  the offset was looked up at the wrong instant. I did not test the exact
  results; the risk follows from the method.
- **Fixed-point iteration (Duktape).** Repeat: offset at (local − last
  offset). Stop when the offset repeats, at most four times. A two-value
  cycle (a skipped local time) takes the larger offset, which in the
  usual DST case is the offset after the transition; the specification
  asks for the offset before it.
- **Specification method (§21.4.1.26).** Find all instants whose local
  time equals the input. If there are two, take the earlier; if there is
  none, use the offset in effect just before the transition. With a list
  of transitions this is direct: look up the offsets just before and
  just after the transition nearest to the input and test both.

### Options: Date.parse beyond the standard format

The specification leaves the non-standard formats to the implementation.
The engines show three policies:

- **Standard format only (MuJS).** Anything else gives NaN. MuJS also
  applies its fixed offset to date-time strings without an offset.
- **Relaxed standard format, then the platform (Duktape).** Duktape's
  parser accepts the standard format with relaxations: any number of
  digits per field, a space instead of `T`, an offset without a colon,
  out-of-range components that it then normalises. It treats a missing
  offset as UTC for date-time forms too (the ES5 rule; ES2016 and later
  say local time). If this parser fails, an optional platform provider
  runs; the Unix provider uses the C library with the locale's default
  date format. Results then depend on the platform and locale.
- **Strict standard format, then a tolerant token parser (QuickJS).**
  The first parser accepts only the standard format. The second parser
  reads the string as a sequence of tokens and gives each token a role
  by its form, not by its position. The classes of input it accepts are:
  the engine's own `toString` and `toUTCString` output; English month
  names in any case and position; numeric dates with various separators,
  where heuristics decide which number is the year and otherwise assume
  the US month-day order; two-digit years mapped to a century by a
  window; 12-hour and 24-hour times; numeric offsets and a small set of
  English zone abbreviations; parenthesised comments, which it skips.
  Missing fields get defaults, including a fixed year when only month
  and day are present.

Web pages use these classes in practice: the HTTP date and RFC 2822 form
(`Tue, 15 Nov 1994 08:12:31 GMT`), the `toString` form, US numeric dates
(`11/15/1994`), "month day, year" (`Nov 15, 1994`), and ISO-like strings
with a space or with slashes. Some details differ between browsers and
are only observable by testing: for example, Duktape's notes record that
one browser engine reads `2012-01-01` as UTC (as the specification
requires) but `2012/01/01` as local time, and is lenient with a space
separator but strict with `T`. swb will measure Chromium for the details:
which classes it accepts, the field order rules, the two-digit year
window, the default year, the accepted zone abbreviations, and how it
handles trailing text.

### Trade-offs

- The C library gives correct historical offsets for free on Unix but
  depends on the platform, its `time_t` range and the process time zone
  setting. It cannot give per-document time zones and makes tests depend
  on the machine.
- A time zone database inside the engine (or a library that reads the
  system's TZif files) gives the same results on all platforms and
  allows a fixed time zone for tests. It costs data size and an update
  policy. I did not measure the size.
- A tolerant parser accepts what pages write, but its rules are
  heuristics; only black-box measurement of Chromium tells which ones
  pages depend on.

### For swb

- Calendar arithmetic follows the specification sections above. The
  order of floating-point operations in MakeTime and MakeDate is
  observable (QuickJS had to prevent fused multiply-add); Rust does not
  fuse unless asked to, but the order must follow the specification.
- The local offset is one function "offset at instant" plus the inverse
  rule of §21.4.1.26. ADR 0025 names a time zone library (for example
  `jiff`) as a candidate through the ADR 0003 process. It must read the
  IANA data (system TZif files or bundled data) and handle the rule
  string for future dates.
- The system time zone should be an input of the engine (set by the
  browser), not read inside the engine. Then tests can fix it (UTC or a
  zone with DST), the automation API can set it, and the differential
  test tool can run Node.js with the same zone.
- Offsets change rarely; caching the current transition interval avoids
  repeated searches. I did not see a cache in the studied engines.
- `Date.parse`: the strict path follows §21.4.1.32 exactly; the fallback
  path is a token parser with rules taken from Chromium measurements,
  with a corpus of strings in the probe tool.

---

## 5.3 Embedding

### The problem

The host must:

1. Define native functions and classes, with prototypes per realm, and
   check that `this` and the arguments have the right class (WebIDL
   requires a TypeError otherwise).
2. Give some host objects non-ordinary property behaviour (WebIDL
   indexed and named properties, for example on collections; the
   `document.all` object with [[IsHTMLDDA]], Annex B.3.6).
3. Connect a host object (for swb: a DOM node in the `dom` crate) to its
   JavaScript wrapper, keep the right things alive, and free the rest.
4. Run promise jobs (microtasks) and connect them to the host's event
   loop; track unhandled rejections.
5. Load modules, including dynamic `import()` and `import.meta`, with
   asynchronous fetching.

ECMA-262 defines the interface between engine and host as host hooks.
The engine calls them; the host implements them. The WHATWG HTML
standard defines the browser's implementation of each hook. WebIDL
defines how DOM interfaces appear as JavaScript objects.

### Native functions and classes

- **QuickJS.** A native function receives the `this` value and an array
  of arguments and returns a value or an exception marker (the exception
  itself is stored in the context). Methods and accessors are usually
  installed from static tables. A host class has a numeric tag. When the
  host registers the class, it can supply cleanup on collection, edge
  reporting for the collector (see below), call behaviour, and
  replacements for internal methods. Objects of the class carry one slot
  for host data; reading the slot checks the tag, which is a cheap brand
  check. Each context (realm) has its own prototype per class.
  QuickJS-ng allocates tags per runtime instead of globally and lets the
  host give an object the [[IsHTMLDDA]] behaviour.
- **MicroQuickJS.** User classes are declared at build time; a build tool
  generates read-only tables for the whole standard library and the host
  classes. Finalizers are a table indexed by class. Native functions
  receive a pointer to their `this` value and arguments, because values
  can move.
- **Duktape.** All exchange goes through a value stack: the host pushes
  values, calls operations that refer to stack indices, and reads
  results from the stack. There is no class concept. The host builds an
  object, puts native functions on it, and stores host data in a
  property whose key is a "hidden" symbol that script code cannot reach.
  A finalizer is attached per object.
- **MuJS.** Also a value stack. A "host-data object" carries a pointer
  and a type tag string; reading the pointer checks the tag. Optional
  hooks intercept has, put and delete for property names. They cover
  lookup and assignment of named and indexed properties, but not their
  enumeration.

Where the engines agree: a host object is an ordinary engine object plus
(a) one slot for host data, (b) a type tag for brand checks, and
(c) optional overrides of internal methods. Where they differ: how
arguments are passed (array versus value stack), how errors propagate
(return marker versus long jump), and whether classes are a registered
concept (QuickJS, MicroQuickJS) or a convention (Duktape, MuJS).

The ECMA-262 hooks for exotic behaviour are the internal methods of
§10.1 (ordinary) and §10.4 and §10.5 (exotic and proxy objects). WebIDL
"legacy platform objects" define their internal methods in terms of
these.

### Lifetime: three kinds of edges

A host object and its wrapper create three kinds of references:

1. Wrapper to host object (the host data slot).
2. Host to JavaScript values: event listeners, callbacks of timers and
   observers, the wrapper itself (so that the host can return the same
   wrapper the next time), values stored by the host.
3. Host object to host object (the DOM tree: parent, children, document).

#### Edge 1 and finalizers

All engines call a host finalizer when the wrapper is collected, so that
the host can release the host object.

- QuickJS and MicroQuickJS: the finalizer receives the runtime and the
  host data; it must not run JavaScript.
- MuJS: the finalizer receives only the host pointer.
- Duktape: the finalizer is a function (native or JavaScript) stored on
  the object. It can run JavaScript and can "resurrect" the object by
  storing a reference to it; the engine may then call it again later.
  When the heap is destroyed, Duktape runs the finalizers of all objects,
  reachable or not. Its design notes advise to make finalizers
  re-entrant and to touch only the object itself.

Finalizers that run script complicate the collector (resurrection,
re-entry, ordering) and make collection timing observable. ECMA-262
exposes finalization to scripts only through FinalizationRegistry
(§26.2), whose callbacks run later as a job scheduled by the host hook
HostEnqueueFinalizationRegistryCleanupJob (§9.9.4.1), never inside the
collector.

#### Edge 2: rooting values that the host holds

The engines use these forms:

- **Reference counts (QuickJS).** The host takes a counted reference to
  any value it keeps (for example a timer callback in a host-side list)
  and releases it later. No explicit root registration is needed for
  correctness, but a host reference that is part of a cycle keeps the
  cycle alive unless the cycle collector can see it (see edge 3).
- **Scoped root stack (MicroQuickJS; Duktape and MuJS through their value
  stacks).** Values that native code holds for a short time live in
  slots that the collector scans. In MicroQuickJS the host pushes a root
  slot, keeps only the slot's address, and pops it in reverse order; the
  collector updates the slot when the object moves. A debug mode that
  moves every object on every allocation finds missing roots.
- **Persistent root table.** MicroQuickJS has a second, unordered list of
  roots for long-lived references. Duktape offers hidden objects that
  are reachable from the heap root (per heap and per global
  environment); its event loop example stores timer callbacks there,
  keyed by timer number, and deletes them when the timer is cleared.
  MuJS offers a hidden table with string keys and a call that stores a
  value under a fresh key and returns the key.
- **Borrowed raw references (Duktape).** The host can get the address of
  a heap object and push it back later. This is valid only while the
  object stays reachable through other paths. Pushing such a reference
  for an object that waits for finalization cancels the finalization.

#### Edge 3: cycles through the host

The typical leak: a DOM node holds an event listener (edge 2); the
listener's closure holds the node's wrapper; the wrapper holds the node
(edge 1). If edge 2 is a root, the cycle never dies.

- QuickJS lets each host class provide a tracing callback that reports
  the JavaScript values held in its host data. The cycle collector then
  sees edges through host objects and collects such cycles. The
  QuickJS-libc worker class uses it for its message handler. Values held
  in host-side lists that are not reachable from an object (timer
  callbacks) are plain counted references and therefore roots.
- Duktape and MuJS have no such callback. Host code that wants
  collectable edges stores the JavaScript values as (hidden) properties
  of the wrapper itself, so the collector sees them as ordinary edges.
- The browser literature describes the same problem at full scale. A
  browser engine team describes how it first kept DOM wrappers alive by
  "object grouping" (a group per document, alive if any member is
  reachable), which hid real retaining paths and sometimes collected
  wrappers too early, replacing them with empty wrappers that had lost
  their properties. It replaced grouping with tracing across the
  boundary: the collector traces from JavaScript objects into the DOM
  and from DOM objects back into JavaScript (V8 blog, "Tracing from JS
  to the DOM and back again", 2018).

#### Wrapper identity

WebIDL makes a DOM object and its JavaScript object the same "platform
object". Script can observe identity in three ways: `===`, properties
added to the wrapper ("expandos"), and use of the wrapper as a `WeakMap`
key or `WeakRef` target. So once a wrapper has been exposed, the host
must return the same wrapper for as long as the node can be reached
again from script, for example through `document.querySelector`. The
options:

- **Strong both ways, collected by one tracer.** The wrapper holds the
  node; the node holds its wrapper; the collector traces through host
  objects (QuickJS-style tracing callback, or the cross-boundary tracing
  of the browser literature). Liveness is exact. The host must report
  its edges correctly, and for an incremental collector it needs write
  barriers on host-side edges.
- **Node holds the wrapper weakly; the document keeps wrappers of
  attached nodes alive.** A wrapper of a node in a live document is a
  root (or is kept alive while the document is alive); a wrapper of a
  detached node lives only while script references it. This is the
  "grouping" idea; it is simpler, but a detached subtree whose wrapper
  dies must not lose its identity if a script can still reach another
  node of the subtree.
- **Recreate wrappers on demand when they are unobservable.** A wrapper
  that never got expandos, never became a weak key and was never
  compared may be dropped and recreated. Deciding "unobservable" is
  error-prone; the browser report above shows what happens when it is
  wrong.

None of the small engines solves this problem; they only provide the
mechanisms (host data slot, finalizer, tracing callback, roots).

### Job queue and rejection tracking

ECMA-262 defines jobs and the hooks that schedule them (§9.5):
HostEnqueuePromiseJob (§9.5.5), HostEnqueueGenericJob (§9.5.4),
HostEnqueueTimeoutJob (§9.5.6), HostMakeJobCallback and
HostCallJobCallback (§9.5.2, §9.5.3; HTML uses them to carry the
incumbent settings object). Promise jobs are created in §27.5.2. A job
runs only when the execution context stack is empty, and runs to
completion. HostPromiseRejectionTracker (§27.5.1.9) reports a rejection
without a handler and a handler added later. WeakRef targets that script
has read stay alive until the end of the job: AddToKeptObjects and
ClearKeptObjects (§9.10, §9.11).

- **QuickJS.** One first-in-first-out list of jobs per runtime. A job is
  a native function with its arguments and its context (realm). Promise
  reactions and the start of a dynamic import enqueue jobs. The host
  drains the list by calling "run one pending job" until none is left;
  the engine never drains it by itself. A host callback receives
  rejection events with a "handled" flag. QuickJS-ng adds hooks on
  promise lifecycle events for debugging tools.
- **Duktape.** The Promise built-in is disabled by default; its
  configuration notes say that it does not work yet. There is no job
  queue. **MuJS** implements ES5 only: no promises, no jobs. I found no
  promises or modules in **MicroQuickJS** either.

The HTML standard defines the browser side: a microtask queue per event
loop; HostEnqueuePromiseJob queues a microtask; a "microtask checkpoint"
runs after each task and also when the JavaScript stack becomes empty
after a callback ("clean up after running script"), so microtasks run
between two event listeners of the same event dispatch. The checkpoint
also reports unhandled rejections (the `unhandledrejection` event) and
calls ClearKeptObjects. The QuickJS split (the engine queues, the host
decides when to drain) fits this model; the host only needs a way to
drain at the points HTML defines.

### Modules

ECMA-262 defines module loading as an asynchronous process driven by the
host (§16.2.1):

- LoadRequestedModules (§16.2.1.6.1.1) walks the import graph and calls
  HostLoadImportedModule (§16.2.1.10) for each request. The host may
  answer later, by calling FinishLoadingImportedModule (§16.2.1.11).
  ContinueModuleLoading (§16.2.1.6.1.1.2) continues the walk.
- Linking and evaluation of cyclic module records, including top-level
  `await`: §16.2.1.6.
- Dynamic import: EvaluateImportCall and ContinueDynamicImport
  (§13.3.10.2, §13.3.10.3).
- `import.meta`: HostGetImportMetaProperties and HostFinalizeImportMeta
  (§13.3.12.1.1, §13.3.12.1.2).
- Import attributes: HostGetSupportedImportAttributes (§16.2.1.12.1).

The engines:

- **QuickJS.** The host gives two callbacks: one turns a specifier and
  the name of the importing module into a module name; the other loads
  the named module and returns it compiled. Loading is synchronous: the
  whole graph is loaded and compiled before linking. Dynamic import runs
  as a job, but the loading inside it is still synchronous. Variants of
  the callbacks receive import attributes. The host can create native
  modules by declaring export names before linking and setting their
  values after. The host can get and fill the `import.meta` object.
- **Duktape.** No ECMAScript modules. An optional extra provides CommonJS
  `require` with a host "search" callback that returns source text,
  synchronously. Its design note explains that the module record is
  registered before the search runs, so that cycles work.
- **MuJS and MicroQuickJS.** No modules.

A browser fetches modules over the network, so the synchronous design of
QuickJS does not fit. The specification's design does: the engine keeps
the state of a graph load, and the host resumes it when each fetch ends.
The HTML standard defines the host side: a module map per document keyed
by URL and module type, specifier resolution with import maps, fetching,
and `import.meta.url` and `import.meta.resolve`.

### Other host hooks

A browser host also implements HostEnsureCanCompileStrings (§19.2.1.2,
Content Security Policy for `eval`), HostHasSourceTextAvailable
(§20.2.5) and InitializeHostDefinedRealm (§9.3.1, one realm per window
or iframe). QuickJS models realms as contexts that share one heap, which
matches same-origin frames that share objects.

### Trade-offs

- A value stack API (Duktape, MuJS) roots everything on the stack but
  makes host code verbose; direct values (QuickJS) are easy to use but
  need reference counts or explicit roots.
- A tracing callback per host class makes cycles through host objects
  collectable, but a missed edge is a premature free and an extra edge
  is a leak.
- Script-running finalizers (Duktape) make collection observable and
  need resurrection handling; native-only finalizers are simpler.
- A synchronous module loader fits command-line hosts, not a browser.

### For swb

- In safe Rust with index handles, a handle that native code holds in a
  local variable is invisible to the collector. If a collection runs
  while such a handle is live, its slot can be freed and reused, and the
  handle then refers to another object (not a memory safety violation,
  but a logic error that is hard to find). The options from the engines:
  - collect only at points where native code holds no unrooted handles
    (for example only between bytecode instructions, or only on explicit
    allocation calls that take a root set);
  - scoped root slots (MicroQuickJS, Duktape, MuJS), in Rust a root
    scope object that registers handles and releases them in reverse
    order;
  - a persistent root table with explicit release for long-lived host
    references (timer callbacks, listener lists if they are kept
    outside the heap);
  - generation numbers in handles, so that a stale handle is detected
    instead of silently aliasing another object;
  - a stress mode that collects at every allocation (the MicroQuickJS
    debug mode is the model) to find missing roots in tests.
- The `dom` crate addresses nodes by `NodeId` in a `Vec`. A wrapper can
  hold the `NodeId` (edge 1 is then an integer, not a pointer). Today
  nodes are never freed during a document's life; with scripts, detached
  subtrees created by `createElement` or `removeChild` need a lifetime
  rule. The options above apply: trace from the engine heap into the DOM
  arena and back (one collector decides for both), or keep nodes alive
  while the document lives and accept the memory cost for detached nodes
  until the document unloads. The second is simpler; the first is exact.
- Edge 2 values that belong to a node (event listeners, the wrapper)
  should live where the collector sees them: either in the engine heap
  (for example in a hidden per-node object), or in the DOM with a
  tracing callback that the collector calls. A plain root per listener
  leaks the listener cycle described above.
- Wrapper identity: a map from `NodeId` to wrapper handle, held by the
  engine or the bindings. Whether this map is strong, weak with document
  roots, or traced decides the leak and identity behaviour; this is the
  main design decision for bindings.
- Host classes: a numeric class tag per WebIDL interface, an inheritance
  check for brand tests (an `HTMLDivElement` is an `Element`), one
  prototype per interface per realm, and internal-method overrides for
  legacy platform objects. Finalizers that cannot run script avoid
  resurrection; if the DOM arena is traced, DOM nodes may need no
  finalizer at all.
- Job queue: the engine provides "enqueue job" and "run jobs until the
  queue is empty", and calls a host rejection callback; the browser
  calls the drain at the HTML microtask checkpoints. Jobs must carry
  their realm.
- Modules: implement the specification's asynchronous loading state, so
  that the browser can answer HostLoadImportedModule after a network
  fetch completes.

---

## Open questions

1. Regular expressions: does swb need the legacy `RegExp.$1` static
   properties for its targets? This is a measurement on the target pages.
2. Regular expressions: is a step-count limit enough, or do the targets
   contain patterns for which a memoizing or linear-time mode matters?
   I did not find data on this.
3. Dates: the size of bundled time zone data versus reading system TZif
   files, and the update policy. I did not measure sizes.
4. Dates: the exact Chromium rules for non-standard `Date.parse` input
   (to be measured with the probe tool).
5. Embedding: which DOM lifetime model (traced DOM arena, or nodes alive
   until document unload) fits the target pages' memory use. This needs
   a measurement of how many nodes the targets detach.
6. I did not check how the engines implement fast paths for
   `RegExp.prototype` methods when the regular expression object is
   unmodified (the generic protocol of §22.2.6 calls user-visible `exec`
   and reads `lastIndex` and `flags`).

---

## Sources

### Engines

- **QuickJS** (Fabrice Bellard), commit 535a7c2, 2026-09-29, VERSION
  2026-06-04. Files read: `libregexp.c` (parser of terms, alternatives
  and disjunctions; quantifier compilation; counter allocation; search
  prefix; matcher loop with its stack, undo log, lookahead handling and
  interrupt polling), `libunicode.c` (case conversion and regular
  expression canonicalization, class canonicalization), `libunicode.h`,
  `quickjs.c` (Date: local offset, field conversion, the two date
  parsers, `Date.parse`; the regular expression exec entry; the interrupt
  and stack check glue; dynamic import job), `quickjs.h` (class, job,
  module loader, rejection tracker, interrupt declarations),
  `quickjs-libc.c` (file and worker classes, timers),
  `doc/quickjs.texi` (C API and Internals chapters).
- **QuickJS-ng**, commit c359cac, 2026-10-09. Files read: `libregexp.c`
  (compared with QuickJS: same instruction set, same limits),
  `quickjs.c` (local offset, Date parse entry), `quickjs.h` (class tags,
  tracing and liveness calls, promise hooks, rejection tracker, module
  loader variants, job calls).
- **MicroQuickJS**, commit 6d4d7eb, 2026-09-26 (MIT). Files read:
  `README.md` (whole), `mquickjs.h` (root stack and list, class and
  finalizer declarations), `mquickjs.c` (regular expression compiler
  outline and matcher entry with its handling of moving objects).
- **MuJS** (Artifex), commit aab59f2, 2026-10-06. Files read: `regexp.c`
  (whole: lexer, tree parser, size count, compiler, recursive matcher),
  `regexp.h`, `jsregexp.c` and `jsstring.c` (exec glue, run limit),
  `jsdate.c` (local offset, DST, ISO parser), `mujs.h`,
  `docs/reference.html` (host data objects, hidden table, garbage
  collection).
- **Duktape**, commit 3afa016, 2026-09-05 (master, 3.0 development).
  Design notes read: `doc/regexp.rst` (whole), `doc/datetime.rst`
  (whole), `doc/unicode-support.rst` (overview and case conversion
  parts), `doc/api-design-guidelines.rst` (whole),
  `doc/memory-management.rst` (finalizer behaviour section),
  `doc/modules.rst` (introduction), `doc/symbols.rst` (hidden symbols,
  first part). Source read: `src-input/duk_bi_date.c` (parse entry,
  local-to-UTC iteration), `src-input/duk_bi_date_unix.c` (whole),
  `src-input/duk_regexp.h`, parts of `src-input/duk_regexp_executor.c`
  and `src-input/duk_regexp_compiler.c` (flags, canonicalization calls),
  `src-input/duk_bi_promise.c` (size only), `src-input/duktape.h.in`
  (declarations of heap pointers, hidden objects, finalizers),
  `config/config-options/` entries for regular expression limits,
  canonicalization tables, execution timeout and the Promise built-in,
  `examples/eventloop/c_eventloop.c` (comments on timer callback
  storage).

### Specifications

- ECMA-262, current draft, https://tc39.es/ecma262/ (multipage edition,
  fetched 2026-10-09). Sections cited above: §9.3.1, §9.5, §9.9.4.1,
  §9.10, §9.11, §10.1, §10.4, §10.5, §13.2.7.2, §13.3.10, §13.3.12,
  §16.2.1, §19.2.1.2, §20.2.5, §21.4.1, §21.4.3.2, §21.4.4.41,
  §21.4.4.43, §22.2.1, §22.2.2, §22.2.7.2, §26.2, §27.5.1.9,
  §27.5.2, Annex B.1.2, Annex B.3.6.
- WHATWG HTML, "Integration with the JavaScript job queue", "Perform a
  microtask checkpoint", HostLoadImportedModule, HostEnqueuePromiseJob,
  unhandled promise rejections:
  https://html.spec.whatwg.org/multipage/webappapis.html
- WebIDL, platform objects and legacy platform objects:
  https://webidl.spec.whatwg.org/#es-platform-objects
- TC39 proposal, RegExp lookbehind assertions:
  https://github.com/tc39/proposal-regexp-lookbehind
- TC39 proposal, RegExp `v` flag:
  https://github.com/tc39/proposal-regexp-v-flag
- TC39 proposal, RegExp legacy features:
  https://github.com/tc39/proposal-regexp-legacy-features
- Unicode Technical Standard #18, Unicode Regular Expressions:
  https://www.unicode.org/reports/tr18/
- Unicode Character Database, CaseFolding.txt:
  https://www.unicode.org/Public/UCD/latest/ucd/CaseFolding.txt
- IANA Time Zone Database: https://www.iana.org/time-zones ; theory and
  pragmatics: https://data.iana.org/time-zones/theory.html
- RFC 9636, The Time Zone Information Format (TZif), October 2024
  (obsoletes RFC 8536): https://www.rfc-editor.org/rfc/rfc9636

### Literature

- Russ Cox, "Regular Expression Matching Can Be Simple And Fast":
  https://swtch.com/~rsc/regexp/regexp1.html
- Russ Cox, "Regular Expression Matching: the Virtual Machine Approach":
  https://swtch.com/~rsc/regexp/regexp2.html
- Russ Cox, "Regular Expression Matching in the Wild":
  https://swtch.com/~rsc/regexp/regexp3.html
- J. C. Davis, F. Servant, D. Lee, "Using Selective Memoization to Defeat
  Regular Expression Denial of Service (ReDoS)", IEEE Symposium on
  Security and Privacy 2021, doi 10.1109/SP40001.2021.00032:
  https://www3.cs.stonybrook.edu/~dongyoon/papers/SP-21-Memoization.pdf
- V8 blog, "An additional non-backtracking RegExp engine" (2021), article
  only: https://v8.dev/blog/non-backtracking-regexp
- V8 blog, "Tracing from JS to the DOM and back again" (2018), article
  only: https://v8.dev/blog/tracing-js-dom
