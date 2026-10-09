# ADR 0021: Code derived from Chromium, Skia and Servo

- Status: accepted
- Date: 2026-10-07

## Context

Ground rule 6 allows reading other projects' code for ideas, but not
copying it, and asks to respect its license. ADR 0003 does not allow
copyleft licenses (LGPL, GPL, MPL-2.0; ADR 0014 is a one-off exception).

From M0 on, agents read Chromium source code to match Chromium's
results: font selection (M0), table layout and form controls (M2), grid,
floats, positioning and line breaking (M3). docs/credits.md recorded this
as "read for ideas, no code copied".

In M3 an integration review found that a new line-breaking module was a
port of Blink's `text_break_iterator.cc` (LGPL-2+) and restated a data
table from Chromium's `character_property_data_generator.cc` (BSD-3).
That module was not committed. A provenance audit of the committed code
(HEAD b5ae8da) then compared swb with the sources that credits.md names
and found:

- Close ports of Chromium code (BSD-3-Clause): table layout column
  constraints, width distribution, row heights and cell alignment
  (`layout/src/table/columns.rs`, `distribute.rs`, `rows.rs`, `cells.rs`,
  parts of `layout.rs`; about 1,300 lines, M2), grid track sizing
  (`layout/src/grid/sizing.rs` and the automatic repetitions in
  `grid/mod.rs`; about 600 lines, M3), and three small functions (sticky
  offsets in `positioned.rs`, the placement of formatting context roots
  next to floats in `block.rs`, intrinsic sizes with floats in
  `intrinsic.rs`), and the check mark path of native checkboxes.
- Data restated from Skia (BSD-3-Clause): the font equivalence classes
  in `text/src/source/fontconfig.rs` and the fake-bold constants in
  `text/src/raster.rs`.
- A small match with Servo (MPL-2.0): the margin-collapsing type in
  `layout/src/block.rs` (about 25 lines).
- Data derived from LGPL Blink files: the font families for which form
  controls use the width of `0` (`layout/src/control.rs`), three rows of
  the font-size keyword tables (`style/src/values/keywords.rs`), and the
  order and values of some form-control and ruby rules in
  `style/src/ua.css`.

Everything else in the audit was independent or "inspired" (the same
rules, in swb's own structure). The CSS counters work (M3) was audited
before its commit: independent.

## Decision

The owner decided:

- Keep the ported BSD-3 and MPL-2.0 code for now, with correct
  attribution: a comment at each derived item and a section per source in
  `THIRD_PARTY_NOTICES.md` with its license text; corrected statements in
  docs/credits.md, ADR 0010 and ADR 0017. The project has no capacity for
  clean-room rewrites of table layout and grid track sizing now. The
  Servo-derived code is in its own file with the MPL-2.0 header.
- Replace the LGPL-derived data with values derived from black-box
  measurements of Chromium (Playwright scripts in `tools/`), as ADR 0003
  requires.
- Rewrite the new line-breaking module in a clean room: an agent that
  had not read Chromium's or the previous version's code wrote it from
  UAX #14, CSS Text 3 and measurements.
- From now on, agents do not read Chromium, Blink, WebKit, Skia or Servo
  source code when they implement a feature, unless the owner allows it
  for a specific case. Behaviour that the specifications leave open is
  measured in Chromium. Integration reviews check provenance.
- Update (2026-10-08): the rule also covers ports of these projects,
  such as tiny-skia (a Rust port of Skia, a dependency of swb). The
  behaviour and cost of such a dependency are measured through its
  public API. For M4 item 7 (the raster cost model, ADR 0023 part 3) an
  agent was allowed to read tiny-skia's source before this update. No
  code was copied, and the model's constants come from measurements;
  the owner decided to keep the model.
- The rule names engines and their ports. `wuff` (ADR 0022), a Rust port
  of Google's woff2 decoder, is neither: swb uses it as a library, and
  its source was read only to audit its robustness before it was
  adopted. No code was copied (docs/credits.md).
- Update (2026-10-09): the rule also covers Ladybird (LibWeb, LibJS) and
  the JavaScript engines of browsers (V8, SpiderMonkey,
  JavaScriptCore). For the JavaScript engine, ADR 0025 allows analyst
  sessions to read a few small engines with permissive licenses and to
  write study memos without code; all other sessions read only the
  memos.

## Consequences

- swb's source is MIT-licensed except for the attributed parts listed
  in `THIRD_PARTY_NOTICES.md`: BSD-3-Clause (Chromium, Skia; compatible
  with MIT, notices required) and one small file under the MPL-2.0
  (Servo, `layout/src/collapsed_margin.rs`; the MPL-2.0 applies per file,
  so the derived code was moved into its own file).
- Table layout and grid track sizing are candidates for a clean-room
  rewrite (roadmap). Their layout tests (measured in Chromium) would
  serve as the specification of the behaviour.
- Measurement scripts become part of the tools, so that data such as the
  keyword font sizes can be checked again when Chromium changes.

## Follow-up

The owner asked why Chromium code was read at all ("that was the
original error"). The answer given on 2026-10-07: ground rule 6 allows
reading for ideas; from M0 on the practice became studying Chromium's
implementation to get identical results; in M3 the prompts set exact
parity with Chromium as the goal and pointed agents at Blink internals,
with no boundary between reading and writing, and reviews did not check
provenance.

On 2026-10-08 the owner closed the question: no further discussion is
needed. The rule in "Decision" stays. The list of derived code in
`THIRD_PARTY_NOTICES.md` and in this ADR is the record for a later
clean-room rewrite. The largest items are table layout (about 1,300
lines) and grid track sizing (about 600 lines).
