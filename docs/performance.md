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
example (`crates/js/examples/vm_bench.rs`) and the `lex_files` example,
then prints two Markdown tables.

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
   the numbers below are for the 60 scripts of the session-6 run, so they
   compare only with the same set. Example: `just jsbench out/bbc-js`.

2026-10-09, JS spike session 6 (branch `js-spike`, base b7e9342). Release
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
