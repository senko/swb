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
