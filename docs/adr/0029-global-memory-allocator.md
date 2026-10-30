# ADR 0029: Global memory allocator

- Status: accepted
- Date: 2026-10-30

## Context

M8 feature 11a left the Craigslist posting page (JavaScript on, legacy
path) at 4.05 s, over the budget of 3.65 s (ADR 0028 section 16). The
page makes 16.8 million `malloc`/`free` calls (small element vectors,
short-lived arrays, the sweep of 9.5 million dead objects). glibc's
allocator is a large part of what was left: with jemalloc preloaded
(`LD_PRELOAD`) the task took 3.21 s.

ADR 0003 allows general libraries. An allocator solves a problem that
exists outside browsers. The build already compiles C for `ring`, so a C
allocator adds no new language to the build. `unsafe_code` is forbidden in
swb's own crates (a `#[global_allocator]` static is safe code; the
`unsafe` is inside the allocator crate).

Candidates (maintained, permissive license):

- `tikv-jemallocator` 0.6 (MIT OR Apache-2.0; bundles jemalloc 5.3, BSD-2-Clause).
- `mimalloc` 0.1.52 (MIT; bundles mimalloc 2.3.2 and 3.3.2, selected by the
  `v2` feature; the default is 3.3.2).

Not measured: `snmalloc-rs` (C++, needs a C++ compiler: a new tool),
`rpmalloc` (small ecosystem), pure Rust allocators (none that is mature
for a multi-threaded process of this size).

## Measurements

Release builds of the same tree (9cd2155 plus the allocator), same
machine (Intel Core i5-13500, load average 4 to 8), runs interleaved,
medians. Scripts and logs: the notes file of feature 11a2.

| | system (glibc) | mimalloc 3 | mimalloc 2 | jemalloc |
|---|---|---|---|---|
| Craigslist load, median of 7 (budget 3.65 s) | 4.09 to 4.28 s | 3.37 to 3.46 s | 3.38 s | 3.34 to 3.47 s |
| Craigslist peak RSS | 221 MiB | 346 MiB | 285 MiB | 272 MiB |
| JS-off pipeline total, ars-technica (median of 5) | 42.2 ms | 38.9 | 39.0 | 39.2 |
| same, bbc / wikipedia / hacker-news | 27.5 / 39.0 / 5.7 | 25.0 / 34.4 / 5.1 | 25.7 / 33.9 / 5.1 | 25.0 / 34.5 / 5.1 |
| JS-off peak RSS, ars-technica / bbc / wikipedia / hacker-news / senko-net | 163 / 164 / 51 / 29 / 26 MiB | 246 / 241 / 109 / 65 / 46 | 209 / 208 / 79 / 53 / 50 | 221 / 216 / 87 / 63 / 61 |
| `vm_bench` geomean, 110 rows, new / system (median of 3) | 1 | not measured | 0.923 | 0.937 |
| Hostile set (259 cases) | 258 ok, 1 known failure | not run in full | 256 ok, **2 over the memory limit**, 1 known failure | 258 ok, 1 known failure |
| `swb` / `swb-js` size (with line tables) | 169.5 / 26.0 MB | 171.2 / 27.6 | 170.7 / 27.1 | 175.6 / 32.1 |
| Clean build of the allocator crate (`cargo build -j 8 --release -p ...`) | | 7.9 s (with `cc`) | | 13.9 s wall, 66 s CPU (configure and `make`) |
| Windows (MSVC) | yes | yes | yes | no (the crate does not support it) |
| Linux / macOS | yes | yes | yes | yes |

The two hostile cases that fail with mimalloc, in both versions
(`xhr-retained-responses-js-on`, `xhr-handlers-of-unreferenced-requests-js-on`),
read 60 MiB responses on network threads while the page thread frees
them: peak RSS 2043 and 1270 MiB (mimalloc 2; 1222 and 1122 MiB with
mimalloc 3) against 511 and 559 MiB with glibc and 600 and 707 MiB
with jemalloc, with a limit of 1100 MiB. The cause was not diagnosed.
mimalloc has options that could change this (`MIMALLOC_PURGE_DELAY=0` made
the Craigslist peak 264 MiB, as an environment variable), but no option
was tried on the two hostile cases, and the `mimalloc` crate sets options
only through an `unsafe` call (swb forbids `unsafe`) or the environment
of the process.

Other hostile cases with jemalloc: the peak is 50 to 100 MiB higher for
small pages (the arenas); `huge-glyphs` (limit 128 MiB) 47 to 97 MiB;
`fetch-big-responses-busy-js-on` 287 to 504 MiB; the largest case
(`query-selector-all-every-node-js-on`, limit 1536 MiB) 1157 to 1081 MiB;
times are equal or lower.

Memory after a large page: a script that allocates 5,000 typed arrays of
64 KiB (320 MiB), drops them and collects, then idles: RSS stays at
320 MiB with glibc, 330 MiB with mimalloc (both versions), and falls to
122 MiB with jemalloc (peak 277 MiB). For a heap of one million small
objects the engine keeps its arenas, and the RSS is the same for all
(glibc 393 MiB, mimalloc 424 MiB, jemalloc 354 MiB after the
collection).

## Decision

Use `tikv-jemallocator` as the `#[global_allocator]` of the binaries `swb`
and `swb-js` (and of the `vm_bench` example, so that `just jsbench` measures
the shipped configuration), on every target except `target_env = "msvc"`.
On Windows with MSVC the binaries keep the system allocator. There is no
feature flag. The dependency is declared once in the root
`[workspace.dependencies]`; `swb` and `swb-js` list it under
`[target.'cfg(not(target_env = "msvc"))'.dependencies]`. The build compiles
jemalloc from source, so the build host needs a C compiler, `make` and
`sh` (README, Requirements).

Reasons, in the order of the criteria:

1. Budget: the Craigslist load takes 3.34 to 3.47 s (budget 3.65 s). Both
   candidates meet it.
2. mimalloc exceeds the memory limit of two hostile cases (above); jemalloc
   passes the whole set. This decides between the two, although mimalloc
   supports MSVC.
3. jemalloc returns freed memory to the system in the measured scenario
   (mimalloc and glibc do not). Its peak RSS on the Craigslist page is
   lower than that of mimalloc 3 and a little lower than that of
   mimalloc 2 (272 against 285 MiB).
4. Costs: +23 % peak RSS on the Craigslist page, +32 to +135 % on the
   other targets (a fixed 35 MiB or so), 6 MB more in `swb-js`, a build with
   `make` and a C compiler (the same tools that a Linux or macOS build of
   `ring` needs, plus `make`).

## Alternatives

- Keep glibc: misses the budget by 0.4 s.
- mimalloc 3 or 2: equal speed, fails two hostile memory limits, more RSS
  on the pages (3: +56 %).
- jemalloc on Unix, mimalloc on Windows: two allocators to test; Windows is
  not a target now (ground rule 14 asks only for portable core code).
- Inline storage for small arrays in the object record: fewer
  allocations, but it changes `record_sizes` and gains about 0.2 s at most.
  Still possible later.

## Consequences

- The load time of the Craigslist page and the JS-off pipeline times fall
  (pipeline total by 8 to 12 %); `jsbench` rows with large allocations get
  slower (`typed: toBase64 of 16 MiB` 1.43x, `string: split into 1M
  one-unit pieces` 1.36x), the geomean is 0.937.
- The peak RSS of every run rises. The hostile set has the memory limits;
  `huge-glyphs` is at 76 % of its limit.
- A Windows build needs no allocator crate. If Windows becomes a target,
  measure again; `mimalloc` needs the two hostile cases analysed first.
- The tests of the library crates and the integration tests keep the
  system allocator. The unit tests of the `swb` and `swb-js` binaries
  compile with `main.rs` and so use jemalloc.
- jemalloc's options can be set at build time, without `unsafe`, through
  the `JEMALLOC_SYS_WITH_MALLOC_CONF` environment variable of
  `tikv-jemalloc-sys` (for example in `[env]` of `.cargo/config.toml`).
  swb sets none now. Fewer arenas (`narenas`) may lower the RSS of small
  pages (backlog).
- `swb-js` is also a library. Cargo cannot limit a dependency to the
  binary target, so jemalloc compiles for every crate that uses
  `swb-js`; only the binaries that set the static use it.
- Memory profiles of the page (`heap at load` lines) are unchanged: they
  count the engine's own accounting, not the allocator's.
