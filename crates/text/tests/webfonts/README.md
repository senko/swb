# Web font test files

`swb-variable.ttf` is a variable font made for swb (see below). The
other files are subsets of DejaVu Sans.

A subset of the bundled DejaVu Sans (`fixtures/fonts/DejaVuSans.ttf`,
DejaVu Fonts 2.37) in three formats, for the decoder tests of
`crates/text/src/decode.rs` and the hostile-page set:

| File                  | Format    |
|-----------------------|-----------|
| `dejavu-subset.ttf`   | TrueType  |
| `dejavu-subset.woff`  | WOFF 1.0  |
| `dejavu-subset.woff2` | WOFF 2.0  |

The subset keeps U+0020..U+007E, U+00A0, U+00E9, U+0410..U+044F and
U+2603 with all layout features. It was made with fontTools 4.60
(`fontTools.subset`, `flavor` `None`, `woff` and `woff2`). License: the
Bitstream Vera license (the family name does not contain "Bitstream" or
"Vera", as the license requires for modified fonts); the DejaVu changes
are public domain. See `fixtures/fonts/LICENSE-DejaVu.txt`.

## swb-variable.ttf

A variable TrueType font for the tests of `font-variation-settings`,
`size-adjust` and the metric overrides (`crates/text/tests/web_fonts.rs`,
`tools/probes/web-fonts-2.json`). It has the glyphs `.notdef`, U+0020,
`A`, `B`, `H` and `x`. Each letter is a filled box, so that the advance
width shows the axis values:

| Axis   | Range         | Default | Advance of `A` (units per 1,000)        |
|--------|---------------|---------|------------------------------------------|
| `wght` | 100 to 900    | 400     | 400 at 100, 600 at 400, 1,000 at 900     |
| `wdth` | 75 to 125     | 100     | 400 at 75, 600 at 100, 800 at 125        |
| `SWBX` | 0 to 1,000    | 0       | 600 at 0, 1,100 at 1,000 (0.5 per unit)  |

The axes add up (`wght` 900 and `wdth` 125 give 1,200). Space is 250
units. Ascent 800, descent 200, no line gap, 1,000 units per em, so the
line box of a 100 px font is 100 px high with the baseline at 80 px.

`make-variable-font.py` builds it with fontTools (4.66.1; `fontBuilder` for
six masters, `varLib` for the variations, with `gvar` and `HVAR`). Run it
from the repository root:

    uv run --project tools python crates/text/tests/webfonts/make-variable-font.py

The font and the script were written for swb and are released under CC0
1.0 (no rights reserved).
