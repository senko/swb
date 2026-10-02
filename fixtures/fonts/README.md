# Bundled test fonts

A small, fixed set of fonts for deterministic tests. `FontContext::for_tests()`
in the `text` crate uses only these files, with `GenericFamilyMap::bundled()`.
`fonts.conf` is a fontconfig configuration with the same fonts and the same
family rules, for Chromium (set `FONTCONFIG_FILE` to its absolute path).
fontconfig writes its cache for this directory to `~/.cache/fontconfig`.

The files are unmodified copies from Debian 13 packages.

| File                            | Family           | Source package (version)       | License                           |
|---------------------------------|------------------|--------------------------------|-----------------------------------|
| `LiberationSans-Regular.ttf`    | Liberation Sans  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSans-Bold.ttf`       | Liberation Sans  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSans-Italic.ttf`     | Liberation Sans  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSans-BoldItalic.ttf` | Liberation Sans  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSerif-Regular.ttf`   | Liberation Serif | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSerif-Bold.ttf`      | Liberation Serif | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSerif-Italic.ttf`    | Liberation Serif | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationSerif-BoldItalic.ttf`| Liberation Serif | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationMono-Regular.ttf`    | Liberation Mono  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationMono-Bold.ttf`       | Liberation Mono  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationMono-Italic.ttf`     | Liberation Mono  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `LiberationMono-BoldItalic.ttf` | Liberation Mono  | fonts-liberation (1:2.1.5-3)   | SIL OFL 1.1                       |
| `DejaVuSans.ttf`                | DejaVu Sans      | fonts-dejavu-core (2.37-8)     | Bitstream Vera license; DejaVu changes are public domain |
| `DejaVuSans-Bold.ttf`           | DejaVu Sans      | fonts-dejavu-core (2.37-8)     | Bitstream Vera license; DejaVu changes are public domain |

Upstream versions: Liberation Fonts 2.1.5
(<https://github.com/liberationfonts>), DejaVu Fonts 2.37
(<https://dejavu-fonts.github.io/>).

License texts:

- `LICENSE-Liberation.txt`: copyright notice and the SIL Open Font License
  1.1, from `/usr/share/doc/fonts-liberation/copyright`.
- `LICENSE-DejaVu.txt`: copyright notice and the Bitstream Vera license,
  from `/usr/share/doc/fonts-dejavu-core/copyright`.

The Debian packaging sections of those files (about the `debian/` directory,
GPL-2+) are left out, because no packaging files are included here.

## Family rules

Both `GenericFamilyMap::bundled()` and `fonts.conf` use these rules:

- Generic families: `serif` is Liberation Serif, `sans-serif` is Liberation
  Sans, `monospace` is Liberation Mono. All other generic families use
  Liberation Sans. Queries where no family exists use Liberation Serif
  (Blink's standard font is serif).
- Aliases for families that are not bundled: Arial, Arimo and Helvetica use
  Liberation Sans; Times New Roman, Tinos and Times use Liberation Serif;
  Courier New, Cousine and Courier use Liberation Mono.
- Fallback for characters that the requested fonts do not have: Liberation
  Sans, Liberation Serif, Liberation Mono, DejaVu Sans (regular faces first).
  DejaVu Sans covers many symbols and scripts that Liberation does not.

The test `crates/text/tests/fontconfig_bundled.rs` checks that fontconfig
with `fonts.conf` selects the same fonts as the bundled map.
