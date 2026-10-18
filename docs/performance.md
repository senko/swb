# Performance

How to measure swb's speed, and the current numbers. Measure before you
optimize, and compare with the baseline below after changes to style,
layout or paint.

## Measuring

`just perf` builds the release binary and runs `swbtools perf` (directly:
`uv run --project tools swbtools perf [NAME...] [--runs N]`, which uses the
existing binary). For each page fixture, it runs:

```
swb --headless --replay fixtures/pages/NAME --test-fonts --size 1280x800 --bench 20 URL
```

`--bench N` loads the page, renders it once, and then runs each stage N
times:

| Stage | What it measures |
|-------|------------------|
| `parse` | HTML parsing of the document (the bytes are fetched again from the fixture and parsed N times) |
| `stylesheets` | Parsing all stylesheets and building the rule index (`Stylist`) |
| `style` | The cascade: computed styles of all elements |
| `layout` | Box tree construction and layout (text shaping included) |
| `display_list` | Building the display list |
| `raster` | Rasterizing the first viewport (1280×800 device pixels) |

Before each run, all results are discarded (`Page::restart_pipeline`), so
every stage runs again. Fonts, shape plans and glyph masks stay cached
between runs, as they do in the browser (shaping results are cached only
within one layout pass). `--bench` prints JSON:

```json
{
  "url": "https://senko.net/",
  "runs": 20,
  "elements": 65,
  "viewport": [1280.0, 800.0],
  "stages": {
    "layout": {"first_ms": 0.43, "median_ms": 0.305},
    ...
  }
}
```

`first_ms` is the time at the end of loading: the last time each stage ran
while the page loaded (stages run again when stylesheets and images
arrive, so this is not always the first, cold run), and the first raster.
`median_ms` is the median of the N runs. `swbtools perf` prints the medians as a Markdown
table.

With `RUST_LOG=swb_engine=debug`, swb also logs the duration of each stage
whenever it runs (style, layout, display list).

## Baseline

2026-10-09, M5 (commit 1cbcadb). Release build, Linux, Intel Core i5-13500,
20 runs, median times in ms.

| Fixture | Elements | parse | stylesheets | style | layout | display list | raster | Total |
|---|---|---|---|---|---|---|---|---|
| ars-technica | 1943 | 3.15 | 4.13 | 20.70 | 6.81 | 0.47 | 5.06 | 40.33 |
| bbc | 2745 | 4.05 | 2.30 | 4.65 | 10.39 | 0.70 | 3.35 | 25.45 |
| hacker-news | 818 | 0.65 | 0.46 | 1.46 | 1.82 | 0.14 | 0.67 | 5.20 |
| senko-net | 65 | 0.06 | 0.24 | 0.08 | 0.38 | 0.03 | 0.56 | 1.34 |
| wikipedia-web-browser | 4052 | 5.26 | 5.54 | 10.00 | 12.99 | 1.17 | 2.23 | 37.19 |

Observations:

- The whole pipeline, HTML parsing included, takes at most 30 ms
  (Wikipedia), without any incremental work.
- On Wikipedia, stylesheet parsing (6.8 ms) runs only when the stylesheets
  or the media query results change, but a resize currently recomputes it
  (backlog: keep parsed stylesheets across resizes).
- A hover change on a page with `:hover` rules costs style plus layout
  (about 16 ms on Wikipedia), see ADR 0009. Scrolling does not restyle for
  `:hover`; the next mouse movement does.
- Raster time depends on the window size and on how much of the page has
  text; glyph masks are cached.

## JavaScript engine

`just jsbench [DIR]` (`swbtools jsbench`) builds `swb-js`, the `vm_bench`
example (`crates/js/examples/vm_bench.rs`) and the `lex_files` example
(`crates/js-syntax/examples/lex_files.rs`), then prints two Markdown tables.

1. The small programs of `vm_bench`. Each program runs five times in the
   engine and the best time counts. The same programs (`vm_bench
   --print-js`) run in `node --jitless` (V8's interpreter, used as a black
   box) with the same best-of-5 timing. The ratio is swb / Node. The
   programs run through the embedding API, not through the `swb-js`
   shell, because the shell has no clock.
2. With a directory DIR: `lex_files` lexes all `*.js` files (best of
   `--runs`, default 5); `swb-js bench-compile DIR` parses and compiles each
   file (best of 3 passes) and reports the count of scripts that compile,
   the time and code size of those, the time that the front end needs until
   the first unsupported construct for all scripts, and the peak resident
   memory of the process. The BBC scripts (60 files, 3.5 MB) are not in the
   repository. The fixture `fixtures/pages/bbc` has no scripts (the
   browser runs without JavaScript). To get the set, open https://www.bbc.com/
   in a browser, save the page's scripts (the `<script src>` files and the
   inline scripts) with the developer tools or with `curl` into one
   directory, one `.js` file per script. Any directory of `.js` files works;
   the numbers below are for the 60 scripts of the M6 spike run, so they
   compare only with the same set. Example: `just jsbench out/bbc-js`.

Two examples are not part of `jsbench`:

- `cargo run --release -p swb-js --example heap_bench` measures the heap
  (the collection pause for one million small objects, allocation speed,
  the cost of a handle access and of recording a handle, memory per
  small object).
- `cargo run --release -p swb-js-syntax --example parse_files -- FILE_OR_DIRECTORY...`
  parses files with the scope analysis; with `--generate MB` it parses a
  generated program and reports the time, the memory of the tree and the
  bytes per token.

2026-10-09, end of the M6 spike (commit f3dd6c3). Release
build, Linux, Intel Core i5-13500, Node.js 22.11.

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| fib(27) | 55.4 | 10.7 | 5.18x |
| sum 10M integers | 121.5 | 113.7 | 1.07x |
| o.x = o.x + o.y, 5M times | 397.3 | 77.5 | 5.13x |
| closure counter 1M calls | 64.0 | 20.1 | 3.18x |
| 1M small objects | 251.8 | 22.8 | 11.04x |
| string: 20k appends of 'ab' | 25.0 | 0.3 | 83.33x |
| sum 10M integers in a try | 140.9 | 123.8 | 1.14x |
| sum 10M integers in a try-finally | 204.4 | 219.3 | 0.93x |
| throw and catch 1M times | 24.4 | 308.2 | 0.08x |
| throw TypeError 1M times | 248.5 | 3997.0 | 0.06x |
| forEach over 1M elements | 107.2 | 11.0 | 9.75x |
| for loop over 1M elements | 129.9 | 16.4 | 7.92x |
| join 1M elements | 133.4 | 66.1 | 2.02x |

Programs added in M7 3f (2026-10-15, same machine):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| 1M calls that read arguments | 558 | 50 | 11.2x |
| 1M calls that write mapped arguments | 510 | 65 | 7.8x |
| 1M calls of new Function code | 87 | 17.5 | 5.0x |

The cost of the first two is the creation of the arguments object
(about 0.5 µs; a cached shape and a bulk element store are feature 13).

Programs added in M7 6a (2026-10-16, same machine; the arrays are made
outside the timed function):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| string: split 100k times into 8 pieces | 20.2 (was 98.8) | 3.8 | 5.3x |
| array: sort 100k integers (default order) | 15.5 | 19.0 | 0.82x |
| array: sort 100k integers with (a, b) => a - b | 113 | 22.6 | 5.0x |
| array: 1M push, then 1M pop | 198 | 38.6 | 5.1x |
| array: indexOf on 100k elements, 200 times | 10.8 | 13.4 | 0.81x |
| array: filter of 100k elements, 10 times | 103 | 11.2 | 9.2x |
| array: reduce of 100k elements, 10 times | 96 | 8.2 | 11.7x |

The split row repeats one constant string, which both engines cache
(with 100 different strings: swb 86 ms, Node.js 19 ms). The callback rows
(`filter`, `reduce`, the comparator `sort`, and `forEach`, 93 ms, was
107) cost the re-entry of each call, about 65 ns (feature 13); `push`
and `pop` the interpreter's method calls. `join 1M elements` went from
131 to 106 ms.

`shift` moves the whole dense vector: `while (h.length) h.shift()` on
100,000 elements takes 4.1 s (`node --jitless` 0.59 s), on 200,000 about
19 s (found in the 6a review; backlog).

Programs added in M7 6b (2026-10-16, same machine; the keys are made
outside the timed function):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| map: set and get of 100k integer keys | 21.4 | 8.8 | 2.4x |
| map: set and get of 100k string keys | 36.2 | 12.2 | 3.0x |
| set: add and has of 100k integers | 19.6 | 8.0 | 2.5x |
| map: for-of over 100k entries, 5 times | 224 | 42.1 | 5.3x |
| set: for-of over 100k values, 5 times | 19.9 | 9.7 | 2.1x |
| map: 1M alternating inserts and deletes | 171 | 56.5 | 3.0x |

About 75 ns of each call is the native call of the interpreter (a call of
`Math.abs` costs the same, feature 13); the table itself takes about 30 ns
for an integer key and about 100 ns for a string key (the keyed `SipHash`
of the content and a comparison through two handles; a cached hash in the
string is a backlog item). The iterator steps need no result object when
`next` is the built-in one (`Runtime::builtin_step`), so `for (const v of
set)` costs 40 ns per element. `for (const [k, v] of map)` costs 450 ns
per entry: reading `e[0]` of the pair instead of destructuring it takes
140 ns, of which 100 ns make the pair array; so the destructuring of the pair costs about 300 ns, as it
does for any two-element array (it creates an Array Iterator). A shortcut
for a dense array with the unmodified iterator is a backlog item for
feature 13. The other rows are within noise of the 6a numbers.

Programs added in M7 6c (2026-10-16, same machine; the arrays are made
outside the timed function):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| iterator: values().map().filter().toArray() of 100k, 5 times | 116 | 19.7 | 5.9x |
| iterator: Iterator.from(user iterator).map().toArray() of 100k, 5 times | 241 | 26.0 | 9.3x |
| iterator: take, drop, flatMap and reduce over 100k, 5 times | 352 | 51.7 | 6.8x |
| iterator: for-of over map().filter() of 100k, 5 times | 117 | 20.9 | 5.6x |

A step of a helper over a built-in iterator or another helper takes no
result object, so the cost is the callbacks (about 65 ns each for the
re-entry, as for `Array.prototype.map`; 230 ns per element with two
callbacks). A user iterator costs a call of `next`, a result object and
two property reads per element (480 ns). The other rows are within noise
of the 6b numbers.

Programs added in M7 7a (2026-10-17, same machine):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| generator: for-of over 1M yielded values | 340 | 45.7 | 7.4x |
| generator: yield* chain of depth 10, 100k values | 177 | 53.6 | 3.3x |

A step of `for`-`of` over a generator costs about 340 ns: the native call
of `next`, the move of the frame's registers in and out, the result
object and two property reads. A level of `yield*` adds about 150 ns per
value (the resumption of the level, the call of the delegate's `next`
as a frame and the read of `done`; the delegate's result object passes
through without a copy). A step without the result object when `for`-`of`
calls the built-in `next` of a generator is a backlog item for feature 13.
The other rows are within noise of a build of the previous commit, measured
alternately (three runs each, best of each), except `string: toLowerCase,
toUpperCase 4M calls`, which takes 545 to 600 ms in both builds: its
collection threshold depends on whether a live string happens to occupy a
high slot of the string arena after a burst of garbage (the slot vector
counts as live with its whole capacity), so the count of collections in the
best run is 1 or 7.

Programs added in M7 7b (2026-10-17, same machine). A promise program
returns a promise; its time includes the jobs (`Runtime::run_jobs`; in
Node.js, `await run()`):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| promise: chain of 100k then | 29.3 | 4.0 | 7.3x |
| promise: Promise.all of 100k values | 52.9 | 7.8 | 6.8x |
| promise: 100k Promise.resolve(i).then | 80.6 | 9.2 | 8.8x |

Measured parts, per call: `Promise.resolve(i)` 160 ns, `then` on a
pending promise 195 ns, a job that calls a function 90 ns more than one
with an empty handler (195 ns for `then` and its job), a closure with a
`prototype` (the handler of the last row) 390 ns, `new Promise(executor)`
with a new closure 1.2 µs (1.4 µs before `length` and `name` of the
resolving functions skipped the checks of `[[DefineOwnProperty]]`). Most of
it is the native call from script code (about 100 ns, as for
`Object.keys`), the allocations of the promise and its state, and the
closures; feature 13 works on these. Memory: `Promise.all` over a million
values needs a heap limit between 384 and 512 MiB (about 450 bytes per
element at the peak: the element function, the result promise of `then`,
the job and the lists), Node.js 22 an old space between 128 and 192 MiB.
The other rows are within noise of a build of the previous commit, measured
alternately (four runs each, the median of each; mean change +0.03 %); the
rows beyond 3 % are the `toLowerCase` row (-7 %, the effect above), two
JSON rows (-4 % and -5 %, 0.3 ms), `localeCompare` (-3 %) and `throw new
Error 10 frames deep` (+3.1 %).

Programs added in M7 7c (2026-10-17, same machine); the time includes the
jobs:

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| async: 100k await in a loop | 17.7 | 4.9 | 3.6x |
| async: 100k calls of an async function | 23.6 | 3.5 | 6.7x |
| async: recursion of depth 1000, 100 times | 41.1 | 8.6 | 4.8x |

An `await` of a number in a loop takes 177 ns (the new promise of
`PromiseResolve`, the reaction, the job, the resumption of the frame); a
call of an async function that returns at once 236 ns, about as much as a
call of a function that returns `Promise.resolve(x)`: the result promise
and the internal generator object (176 bytes of state) are most of it.
The other rows against a build of the previous commit, measured
alternately: the mean change is +0.2 % (two runs each). Four variants of
the interpreter arms and of the return path were measured; each moved a
different handful of rows by 3 to 6 % in both directions (layout
effects; the variant with separate inline arms made `fib(27)` 4 % and the
closure row 4 % slower). The one kept (one out-of-line arm for the three
async instructions, the generator part of `return_from_frame` out of
line): `string: indexOf` +6 %, the `sort` with a comparator +3 %, `new
Function` code +4 %, `toLowerCase` +4 % (the effect above), `Promise.all`
-4 %, `sum 10M integers in a try` -3 %, `throw and catch` -3 %.

Programs added in M7 7d (2026-10-17, same machine); the time includes the
jobs:

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| async: for await over an async generator of 100k values | 81.0 | 14.7 | 5.5x |
| async: for await over an array of 100k values | 73.8 | 10.7 | 6.9x |
| async: Array.fromAsync of 100k values | 67.2 | 7.2 | 9.3x |

A step of `for await` over an async generator takes 810 ns: the call of
`next` (the request's promise, the queue, the resumption of the
generator's frame), the iterator result, the resolution of the request,
the `await` of the request's promise and two jobs. Over an array, the
Async-from-Sync Iterator steps the built-in Array Iterator without its
result object (as `for`-`of` does; 112 ms before that shortcut), and the
rest is the wrapper promise of the value, the continuation that builds
the result and two jobs. `Array.fromAsync` has the same costs without the
frame. Feature 13 works on the promise and job costs.
The other rows against a build of the previous commit, measured
alternately (four runs each, the median of each): the mean change is
+0.3 %. Rows beyond 3 %: `fib(27)` +8 %, `sum 10M integers in a try` +7 %,
`throw TypeError 1M times` +5 %, `localeCompare` +5 %, `string: indexOf`
-9 %. None of them runs a new instruction; `fib(22)` runs 3 % fewer
machine instructions than at the previous commit (cachegrind, startup
included), so these are layout effects of the interpreter loop. Three
variants (the new instructions at the end of `Insn`, the arm at the end
of the loop's `match`, `CodeKind::AsyncGenerator` at the end of its
enum) moved `fib(27)` between +7 % and +13 %; the first is kept, the
other two are not.

### Regular expressions

`cargo run --release -p swb-js-regexp --example bench`
(`crates/js-regexp/examples/bench.rs`) times the matcher alone: typical
patterns of the target scripts and patterns that backtrack much, each
finding all matches over its input like a global `exec` loop (a new
`Matcher` per match, as `exec` makes one), averaged over the runs; it
also prints the steps of the matcher.
`node --jitless crates/js-regexp/examples/bench.js` runs the same cases
in V8's regular expression interpreter (no native code), which is the
fair comparison for an interpreter; without `--jitless` V8 compiles hot
patterns to machine code. 2026-10-17, M7 8a, same machine, Node.js 22.11:

| Case | Input | swb (ms) | node --jitless (ms) | ratio |
|---|---|---|---|---|
| trim `^\s+\|\s+$` /g | 102 KB, 2 matches | 0.064 | 0.061 | 1.0x |
| query `([^&=]+)=([^&]*)` /g | 56 KB, 8,000 | 0.899 | 0.848 | 1.1x |
| word `\b(?:fox\|dog)\b` /gi | 126 KB, 4,000 | 0.322 | 0.462 | 0.7x |
| class `[A-Z]` /g | 126 KB, 2,000 | 0.151 | 0.436 | 0.3x |
| tag `<a href="([^"]*)">(.*?)</a>` /g | 118 KB, 2,000 | 0.620 | 0.386 | 1.6x |
| literal `Headline` /g | 118 KB, 2,000 | 0.145 | 0.124 | 1.2x |
| alternation `(?:quick\|lazy\|again)\s(\w+)` /g | 126 KB, 6,000 | 0.700 | 0.927 | 0.8x |
| dot-star `o.*z` /g | 126 KB, 1 | 0.131 | 0.040 | 3.3x |
| nested `(a+)+b` | 22 `a`, no match | 291.8 | 152.7 | 1.9x |
| redos `^(\w+\s?)*$` | 21 units, no match | 0.462 | 0.347 | 1.3x |
| backref `(\w+)\s\1` /g | 120 KB, 10,000 | 0.804 | 1.567 | 0.5x |
| lookbehind `(?<=\$)\d+` /g | 145 KB, 10,000 | 0.774 | 2.230 | 0.3x |

The search skips start positions whose character cannot start a match
(the set of first characters, after zero-width terms); without that,
`trim` took 1.1 ms and `lookbehind` 1.7 ms. A loop over one character
test without captures (`\w+`, `.*`, `[^&]*`) is one instruction that gives
back one character per backtrack. `nested` is exponential in both engines
(88 million steps in swb, 3.3 ns per step); the time check of the
caller stops such a match (ADR 0026 section 9).

Pattern compile cost (release, one `Regex::new`): a class with `v` of
100,000 distinct characters 6.7 ms (35 s before the fix of the review
round: each element copied the set), `\q{...}` with 100,000 alternatives
6.7 ms, 50,000 nested `[x]` with `iv` 5.5 ms, 20,000 duplicate group names
with 20,000 `\k<a>` 6.3 ms and 3 MB (3.1 GB), `\W` 50,000 times with `iu`
9 ms (5.4 s), 1,000,000 times 0.19 s and 100 MB peak (107 s, 10 GB), and
100,000 identical classes `[Ā-ကa-z]` with `i` 9.8 ms (5.6 s). Different
classes still pay for their case closure: 20,000 distinct classes of the
last form take 1.35 s (67 µs each; the closure of a range of 3,800
characters walks about 300 changed characters).

Property escapes (M7 8c, release, wall time of `swb-js -e` minus 2 ms of
start-up): the data of all sets is 76 KB of delta-encoded ranges plus
2 KB of offsets, about 82 KB with the decoder in `swb-js`. A set is
decoded on use (`\p{L}` unites five values). The parser keeps the set and, outside a class,
the node of each property by property, `\P` and `i`: 10,000 times `\p{L}`,
`\P{L}` or `\p{Any}` with `iu` or `iv` compile in 1 to 2 ms and share one
set; 10,000 literals `/\p{L}/u` evaluated in a loop 5 ms. A class pays
for the union and the hash of its set: 10,000 times `[\p{L}--\p{Lu}]`
with `v` or `iv` 80 to 100 ms and 10,000 times `[\p{L}\d]` with `iu`
96 ms (about 10 µs each); 20 different properties 50 times each in
classes with `iv` 11 ms. One `\p{RGI_Emoji}` (about 3,800 strings)
takes 1 to 2 ms with `v` and 2 ms with `iv`; 20 of them 14 ms before and
11 to 12 ms after the cache of the decoded and folded strings of a
property (about 0.3 ms for each further occurrence); 200 of them with
`iv` 86 ms. Matching 1.1 MB of text with `/\p{L}+/gu` takes 24 ms.

Match time of `\p{RGI_Emoji}` (M7 8c fix round, release): the strings were
an alternation that tried all 3,800 strings at every position, about
43 µs per position. They are now a trie (one `Switch` per node, a few
steps per position) and the prefix scan skips positions that cannot
start a string.

| Case | Before | After |
|---|---|---|
| `/\p{RGI_Emoji}/v.test("a".repeat(1e6))` | 43 s | 5 ms (the prefix scan) |
| `/\p{RGI_Emoji}*b/v.test("a".repeat(1e6))` (no prefix scan) | 43 s | 33 ms |
| `/\p{RGI_Emoji}+/iv.test("\u{1F600}".repeat(2e5))` | not measured | 18 ms |
| `bench` example, `\p{RGI_Emoji}` with `gv` over 110k units, 10,000 matches | 1,154 ms, 501M steps | 0.85 ms, 128k steps |
| `bench`, same with `giv` | 1,160 ms | 0.88 ms |
| `/\p{RGI_Emoji}+x/v.test("\u{1F600}".repeat(2e5))` | runs until the limit | runs until the limit (quadratic) |

The other `bench` rows are within noise of the numbers before.

### The `RegExp` built-in

The `regexp:` rows of `vm_bench` (best of 5, release, against `node
--jitless`, 2026-10-17, M7 8b):

| Program | swb (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| `test` 300k times | 51.5 | 17.7 | 2.9x |
| `exec` with groups 300k times | 201.8 | 107.7 | 1.9x |
| 300k evaluations of a literal | 31.1 | 4.0 | 7.8x |
| 300k `replace` of a short string | 172.0 | 28.4 | 6.1x |
| global `replace`, 1M matches in 3M units | 53.1 | 98.2 | 0.5x |
| global `replace` with `$1`, 1M matches | 217.0 | 77.1 | 2.8x |
| global `replace` with a function, 1M matches | 225.9 | 48.2 | 4.7x |
| `split` of 3M units into 1M pieces | 118.7 | 39.0 | 3.0x |
| `match` with `g`, 1M matches | 114.0 | 16.0 | 7.1x |
| `matchAll`, 300k matches | 164.3 | 10.3 | 16x |

All rows are linear in the input. The global loops of `match` and
`replace` and the `split` of an unchanged `RegExp` run the matcher in a
native loop (`builtins/regexp_fast.rs`); the other paths take the steps
of the specification, with the captures instead of result arrays when
`exec` is the built-in one. Before the shapes of result arrays and `RegExp`
objects were kept in the realm, the literal row took 59 ms and `exec`
with groups 304 ms (the property definitions); before the shortcut of the
`flags` getter, a short `replace` took 233 ms. The remaining cost is
general: each match allocates its strings (V8 keeps one-character strings
in a cache, which the `match` row profits from), and each step of
`matchAll` makes a result array and an iterator result object through the
general property definition. Catastrophic backtracking
(`/(a*)*b/.test('a'.repeat(40))`) ends 0.05 ms after a deadline of 100 ms
(the "time limit" lines of `vm_bench`).

### Proxy

The `proxy:` rows of `vm_bench` (best of 5, release, against `node
--jitless`, 2026-10-17, M7 9a):

| Program | swb (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| 1M reads without a trap | 89.0 | 16.5 | 5.4x |
| 1M reads with a `get` trap | 201.1 | 27.5 | 7.3x |
| 1M writes with a `set` trap | 182.1 | 25.9 | 7.0x |
| 100k `Object.keys` with an `ownKeys` trap | 125.5 | 79.8 | 1.6x |
| 1M calls with an `apply` trap | 220.9 | 26.6 | 8.3x |

A read through a proxy leaves the heap's walk, opens a handle scope,
records the target, the handler and the key, reads the trap from the
handler (`[[Get]]` on the handler) and, with a trap, calls it through
re-entry (about 65 ns); V8 has native built-ins for the internal methods
of proxies. The trap names are atoms of the VM (reading them by name took
128 ns per read without a trap). A call through an `apply` trap makes the
argument array and is a tail call of the trap.

The other rows, before (ad32583) and after, each the best of two runs of
`vm_bench` alternating between the two builds: these moved by more than
3 %.

| Program | before (ms) | after (ms) | change |
|---|---|---|---|
| fib(27) | 62.1 | 66.9 | +7.7 % |
| sum 10M integers in a try | 135.9 | 142.0 | +4.5 % |
| new Error and read stack, 100k times | 323.9 | 335.6 | +3.6 % |
| 1M calls that read arguments | 560.0 | 578.3 | +3.3 % |
| 1M calls of new Function code | 89.7 | 98.1 | +9.4 % |
| regexp: test 300k times | 51.1 | 54.9 | +7.4 % |
| regexp: exec with groups 300k times | 194.8 | 223.6 | +14.8 % |
| throw TypeError 1M times | 439.4 | 420.4 | −4.3 % |
| iterator: take, drop, flatMap and reduce | 365.7 | 342.8 | −6.3 % |
| string: toLowerCase, toUpperCase 4M calls | 557.4 | 506.0 | −9.2 % |
| async: for await over an array of 100k values | 75.2 | 69.5 | −7.6 % |
| regexp: global replace, 1M matches in 3M units | 59.2 | 55.0 | −7.1 % |

Nine more rows were 3 to 6 % faster. The rows that move change from
build to build: in the builds of this work, the regular expression
rows `split` and `match` with `g` were 15 % slower in some and 5 %
faster in others, and `fib(27)` between 0 and +8 %. The same programs as
scripts of `swb-js` show no difference for `exec`, `split` and `match`,
and callgrind counts the same instructions within 1.2 % for `fib(22)`
and the `new Function` loop (+0.3 % for a mix of the `regexp`, property
and call programs), so the moves are effects of code placement, not of
work. The `exec` row also collects once more after the change (24 ms):
the realm has a few more objects, so the collections fall elsewhere.
The ordinary paths do one compare more per object of a prototype walk
(the kind of the record that the walk loads anyway) and one more lookup
of the object in a few runtime internal methods (`own_property`,
`define_own_property`).

### Weak collections

The `weakmap:` row of `vm_bench` (best of 5, release, 2026-10-18, M7 9b):
`set` and `get` of 100,000 object keys take 32.4 ms (`node --jitless`
10.3 ms, 3.1x; the `Map` row with integer keys: 21.2 ms and 8.8 ms). The
key is the handle of the object, hashed with the keyed hasher, in the
table of `Map`.

The collector without weak collections: the first marking phase has no
ephemeron check (the second phase is a separate instance of the drain),
and the drain no longer dispatches on the kind of an ordinary object or
an array (they have no payload to trace; with the four new kinds, the
match on the kind had added about 5 instructions per marked object).
Callgrind counts, before (3a0f0ec) and after: `Heap::collect` in a script that collects 2.25 million objects five times
2,600 → 2,284 million instructions (−12 %); in `heap_bench` 3,154 → 2,918
million (−7.5 %). The pause of `heap_bench` for one million reachable
objects is 16.7 to 17.1 ms before and 17.7 to 18.1 ms after (three runs
each); the half-reachable case is the same (13.1 to 13.2 ms).

The rows of `vm_bench` that moved by more than 3 % (best of two runs
alternating between the two builds) all run the same number of
instructions within 0.5 % (callgrind, the programs as `swb-js` scripts):
`set: for-of` +12 % (+0.01 % instructions), `join 1M elements` +8 %
(−0.5 %), `toLowerCase, toUpperCase` +8 % (−0.5 %), `regexp: test` +10 %
(0.0 %), `forEach over 1M elements` +6 % (+0.1 %), `closure counter`
+4 % (0.0 %); others moved by 3 to 6 % in both directions. These are
effects of code placement.

### BigInt

`cargo run --release -p swb-js-text --example bigint_bench`
(`crates/js-text/examples/bigint_bench.rs`) times the arithmetic of
`js-text` alone (M7 10a; no script can make a `BigInt` before 10b), best
of 5 runs, with the work units of `bigint::cost` per nanosecond: the cost
model is calibrated so that a unit takes 0.3 to 1.3 ns (about 0.7 ns for
the quadratic loops; one unit is one step of the schoolbook
multiplication; the review of 10a measured each operation at one or two
limbs and at the limit). `node --jitless crates/js-text/examples/bigint_bench.js`
runs the same values in Node.js 22.11; its times include the
interpreter's dispatch of one operation (about 25 ns). 2026-10-18, same
machine:

| Case | swb | node --jitless |
|---|---|---|
| add, 1 limb | 14.9 ns | 39.8 ns |
| mul, 1 × 1 limb | 14.5 ns | 39.1 ns |
| div, 2 by 1 limb | 30.0 ns | 51.4 ns |
| toString(10), 1 limb | 44.9 ns | 53.2 ns |
| add, 10 limbs | 23.0 ns | 44.4 ns |
| mul, 10 × 10 limbs | 72.8 ns | 171.6 ns |
| div, 20 by 10 limbs | 224 ns | 310 ns |
| toString(10), 10 limbs | 556 ns | 723 ns |
| add, 1,000 limbs | 0.72 µs | 0.66 µs |
| mul, 1,000 × 1,000 limbs | 709 µs | 282 µs |
| div, 2,000 by 1,000 limbs | 728 µs | 655 µs |
| toString(10), 1,000 limbs | 1.9 ms | 1.0 ms |

A result of one or two limbs is computed in 128-bit arithmetic. Most
paths give it a vector of exactly its limbs; two reserve one limb more:
`from_f64` of a double from 2^53 to 2^64, and a left shift (`x << 32n`
of a 1-limb value holds 16 bytes). The heap charges the reserved bytes.
The division of two limbs by one (all single-limb divisions,
`toString`, the quotient estimate of Algorithm D)
multiplies by a precomputed reciprocal (Möller and Granlund 2011); the
reciprocals of the radix powers of `toString` are constants, and a
1-limb value prints its digits directly (before both, `toString(10)` of a
1-limb value took 88 ns). V8's multiplication is faster from about 100
limbs on (Karatsuba and beyond).

The size limit (ADR 0026 sections 7 and 15): the slowest operations on
values of 2^18 bits (4,096 limbs):

| Case | swb | node (with JIT) |
|---|---|---|
| `*`, two values of 2^17 bits | 3.0 ms | 0.56 ms |
| `/`, 2^18 bits by 2^17 bits | 3.1 ms | 1.9 ms |
| `toString()` (radix 10; radixes 3 and 36 the same) | 32.2 ms | 8.5 ms |
| `toString(2)` | 0.33 ms | |
| `BigInt` of the decimal text (78,914 digits) | 3.0 ms | |
| `3n ** 165394n` (2^18 bits) | 4.0 ms | |
| `x ** 7n`, x of 2^18 / 7 bits | 4.4 ms | |
| `^` of two values at the limit | 7.8 µs | |
| `BigInt.asUintN(2 ** 18, -x)` | 2.4 µs | |
| `x << (1n << 40n)`, `3n ** (2n ** 64n - 1n)` (errors at once) | 40 to 50 ns | |

With 2^19 bits `toString()` took 126 ms, more than the 100 ms that one
operation may take; so the limit is 2^18. Chromium 148 prints a value of
a million bits in 36 ms (`tools/probes/js-bigint.json`).

`BigInt` in the language (M7 10b). The `bigint:` rows of `vm_bench`
(best of 5, release, against `node --jitless`, 2026-10-18):

| Program | swb | node --jitless |
|---|---|---|
| 1M steps `s = (s * c + i) % m` on 64-bit values | 280 ms | 80 ms |
| 100k int64 joins and splits (`<<`, `\|`, `asIntN`, `&`, `>>`, `Number`) | 80 ms | 11 ms |

An operation on one-limb values costs about 70 ns in a loop: the library
call (15 to 30 ns), `ToNumeric`, the time charge, the handle scope of the
general path and the allocation of the result (the library grows the
result vector, `RawVec::finish_grow`, and `malloc`/`free` are about a
third of the instructions). The loops of the time limit: endless loops
of the slowest operations on values at the limit (`*`, `/`, `%`,
`toString` in radixes 10 and 36, powers, `BigInt` of a 65,536-digit hex
text, shifts, bitwise operators, `asUintN`, `==` with a long text,
`toLocaleString`, `console.log`) end 2 to 10 ms after `swb-js
--time-limit 200` (process start included); `BigInt` of a text of 50
million digits and `<` with it, 39 ms after. The 165 error paths that
the review of 10b tried with a value at the limit in the message (`big
in big`, `Object.create(big)`, `[1, 2].sort(big)` and others) end 4 to
37 ms after `--time-limit 300`, since the message text is charged
(`Promise.all(big)` 130 ms after: collector pauses for the rejected
promises that the loop keeps, also with a Number). The countdown became
a `Cell` for that, with no cost (callgrind: a global `sum` loop −0.8 %,
`fib(20)` −0.3 % instructions).

The rows of `vm_bench` before (7ee436a) and after, best of two runs
alternating between the two builds: the number rows did not get slower
(`sum 10M integers` 118.6 → 107.3 ms, `in a try` 136.3 → 126.2,
`try-finally` 196.3 → 184.5, `fib(27)` 65.7 → 63.2, `o.x = o.x + o.y`
364.8 → 365.7). Callgrind (the programs as `swb-js` scripts): `sum 10M
integers` −5.2 % instructions, `fib(27)` +0.3 %, `array: filter`
+0.7 % (`Elements::next_index`, which 10b did not change: placement of
inlined code after the new object kind). A first version had the integer
`++` store its result after the match with the general path, three
instructions more per `i++` (+1.0 % on `sum 10M integers`); the integer
arms of `Inc` and `Dec` now store directly. Rows that moved by more than
3 % without a number path: the async rows +4 to +7 % (callgrind of
`for await over an array`: +1.0 % instructions, all in `Shape::find` of
the global object, which has one property more, `BigInt`), `regexp: test`
−15 %, `set: for-of` −11 %, `toLowerCase, toUpperCase` +4 %, `JSON.parse`
+5 %: code placement.

### ArrayBuffer and DataView

The `dataview:` and `arraybuffer:` rows of `vm_bench` (best of 5,
release, 2026-10-18, M7 11a; the interpreter is unchanged, so the other
rows are not measured again):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| dataview: 1M getInt32 reads | 121.1 | 23.2 | 5.2x |
| dataview: 1M setUint32 writes | 96.5 | 24.1 | 4.0x |
| arraybuffer: 100k new ArrayBuffer(256) and slice | 59.6 | 68.0 | 0.9x |

A read or write allocates nothing (a 32-bit integer is a `Value::Int`);
most of the time is the call of a native method, which costs about as
much as `Math.abs` (a `getInt32` call takes about 24 ns more than a
`Math.abs` call in the same loop; V8's `DataView` methods are
compiled builtins). Large buffers: 40 allocations of 64 MiB take 271 ms
(`node --jitless` 21 ms: V8 maps zeroed pages lazily, swb zero-fills
fallibly and touches every page), 40 `slice`s of 64 MiB 355 ms (Node.js
307 ms). Endless loops of 64 MiB and 256 MiB allocations, slices,
transfers and resizes end within 21 ms of a 200 ms time limit (process
start included).

### Typed arrays

The `typed:` rows of `vm_bench` and the `array:` rows that do the same
with an `Array` (best of 5, release, 2026-10-18, M7 11b; the arrays are
globals, as in the `for loop over 1M elements` row):

| Program | swb-js (ms) | node --jitless (ms) | ratio |
|---|---|---|---|
| typed: sum a Float64Array of 1M elements | 177.6 | 21.0 | 8.5x |
| for loop over 1M elements (an `Array`) | 127.2 | 16.5 | 7.7x |
| typed: fill a Uint8Array of 1M elements | 63.2 | 13.0 | 4.9x |
| array: fill an array of 1M elements | 65.0 | 14.5 | 4.5x |
| typed: Int32Array read-modify-write, 1M times | 137.0 | 22.7 | 6.0x |
| array: read-modify-write, 1M times | 137.1 | 23.4 | 5.9x |
| typed: 100k new Uint8Array(64) and subarray | 50.9 | 24.9 | 2.0x |

A read `ta[i]` takes the element in `get_with_receiver` (out of line,
with the slots of the array passed along), a write of a `Value::Int` to
an integer type stores its low bits without a conversion of a double,
and the element bytes are read and written with fixed-size loads and
stores (a first version with `copy_from_slice` and the conversions of
`DataView` took 1.50 G instructions for the read-modify-write program
against 1.24 G for the `Array`; now 1.35 G). `ta.length` reads the
length without the call when the getter is the built-in one; it still
walks two prototypes to find the accessor (`Float64Array.prototype`,
`%TypedArray%.prototype`), so the sum loop with `i < f.length` is slower
than the `Array` loop, whose `length` is an own property (inline caches,
M7 feature 13).

The other paths did not get slower: callgrind (instructions, the
programs as `swb-js` scripts with the loops in a function) against
cc701dd: an `Array` element read loop (1M appends, then 3M reads with
`arr.length`) +0.7 %, an element write loop +0.7 %, the `Array`
read-modify-write loop +0.7 %, `o.x = o.x + o.y` 5M times +0.3 %. The
walks of `[[Get]]`, `[[Set]]` and `[[HasProperty]]` test proxies and typed
arrays with one comparison (adjacent variants of `ObjectKind`), handle
them out of line, and `[[Set]]` keeps the fast path of an own property in
a short function; a first version that tested the kind in
`own_property_of` and re-wrapped its result cost +7.7 % on the `o.x`
row.

### BBC scripts (M6 spike)

The BBC scripts (the 60 scripts of the target page):

| Measure | Value |
|---|---|
| Scripts | 60 (3.5 MB) |
| Lexing, all scripts | 29.1 ms, 121 MB/s, 38.9 million tokens/s (60 of 60 lex to the end) |
| Scripts that compile | 3 of 60 |
| Parse and compile, scripts that compile | 0.09 ms for 2 KB (21 MB/s) |
| Code objects, scripts that compile | 8 KB (4.2 bytes per source byte) |
| Parse until the first unsupported construct, all scripts | 4.6 ms for 0.14 MB read (30 MB/s) |
| Peak memory of the process | 8 MiB |
| Does not compile: for-in | 15 |
| Does not compile: destructuring | 13 |
| Does not compile: spread property | 11 |
| Does not compile: tagged template | 7 |
| Does not compile: default parameter | 6 |

Observations:

- Only 3 of the 60 scripts are inside the spike subset, so the compile
  numbers for real scripts come from tiny inputs. The speed of the front
  end on a large input is better shown by the generated 5.2 MB program of
  `vm_bench`: parse and compile in 182 ms (29 MB/s), code objects 38.7 MB
  (7.4 bytes per source byte), 1.1 million instructions. The measure "until
  the first unsupported construct" includes the per-file set-up and the
  error path, so its 30 MB/s is a lower bound.
- The lexer runs at 121 MB/s over the BBC scripts (7 of them need two-byte
  strings), and the parser and compiler at about a quarter of that speed.
  A page like the BBC scripts (3.5 MB) would need about 120 ms to compile
  at 29 MB/s if all of it were supported.
- The throw rows compare an engine without `stack` with V8, which builds
  stack traces; they are not comparable until M7 adds `stack`. The string
  row is slow because swb has no ropes (ADR 0026: flat strings): each
  append copies.
- The slowest rows (small objects 11x, `forEach` 9.8x, `for` loop 7.9x,
  property update 5.1x) point at M7 work: inline caches for property access,
  the global cache, and callbacks that do not re-enter the interpreter.
