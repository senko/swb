# Line break measurements of Chromium

`swbtools linebreaks` (`tools/swbtools/linebreaks.py`) writes these files
from measurements of Chromium; do not edit them. The method is described in
the module documentation of the tool and in `docs/testing.md`. The test
`crates/text/tests/linebreak.rs` compares `swb_text::line_breaks` with every
measured position. `swbtools linebreak-tables` makes the Latin-1 pair tables
in `crates/text/src/linebreak/tables.rs` from `latin1.txt`.

| File                     | Content |
|--------------------------|---------|
| `latin1.txt`             | Every pair of U+0020..U+00FF between two `a`, for `word-break: normal`, `break-all`, `keep-all`, and `break-all` with `hyphens: none`. |
| `classes.txt`            | Every pair of about 120 characters (samples of each UAX #14 class, ASCII and Latin-1 characters) between `a`, `一` and `0`, for the three `word-break` values, and between `a` with `hyphens: none`. |
| `cases.txt`              | Strings: spaces and control characters, text with punctuation and numbers, scripts and emoji, a hyphen after many characters, soft hyphens. |
| `known-differences.txt`  | The measured positions where swb differs, with the reason. The test fails if a difference is not listed or a listed one is gone. |

## Format

Lines that start with `#` are comments. Code points are written in hex.

A *mode* has the form `word-break=normal hyphens=manual` (the values of the
CSS properties). All measurements use `white-space: pre-wrap` and no `lang`.

### Cases

`@mode MODE` sets the mode of the cases that follow. A case is the code
points of a string, separated by marks for the position between them:

- `÷`: the line can break here.
- `!`: the line breaks here even when everything fits (a forced break).
- `×`: the line cannot break here.
- `?`: unknown: the measurement was not consistent, or the text around the
  position has no width (zero-width content fits into the box of width 0,
  so Chromium does not need to break there).

Example: `0061 × 002D ÷ 0030` is `a-0`, with a break after the hyphen.

### Matrices

```
@matrix left=0061 right=0061 MODE
@chars 0020 0021 ...
./..?/...
...
```

The rows and columns follow `@chars`. The cell in row x and column y is the
position between x and y in the string `left x y right`: `/` (break), `.`
(no break) or `?` (unknown, or a forced break).
