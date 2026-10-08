# Web font test files

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
