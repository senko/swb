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
