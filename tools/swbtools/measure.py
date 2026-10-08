"""`swbtools measure`: black-box measurements of Chromium that swb's data
comes from.

Some tables and rules in swb are measured in Chromium, not taken from its
source code (docs/credits.md). Each measurement loads generated pages in
Chromium with the settings of `browser.py` (bundled fonts, JavaScript
disabled) and prints the result. The swb code next to the data names the
measurement:

- `text-field-families`: the font families for which Chromium makes text
  fields `size` widths of `0` wide instead of using the average character
  width (`ZERO_WIDTH_FAMILIES` in `crates/layout/src/control.rs`).
- `font-size-keywords`: the font sizes of the absolute-size keywords
  (`crates/style/src/values/keywords.rs`).
- `ua-styles`: the computed styles of form controls, other widgets
  (`marquee`, `meter`, `progress`, `output`), and the ruby, frameset and
  map elements (the rules of `crates/style/src/ua.css` that say
  "measured").
- `picture-sources`: which `<source>` elements an `<img>` in a `<picture>`
  skips (`crates/engine/src/image_source.rs`).
- `font-size-sweep`: the font sizes that Chromium shapes text with, for
  every CSS size from 9.000 to 24.000 px in steps of 0.001 px
  (`chromium_sizes` in `crates/text/src/shape.rs`). It takes about 4
  minutes, so it runs only when named, and it writes
  `crates/text/tests/chromium-font-sizes.txt` (see `font_sizes.py`).

The pages are files in a temporary directory, loaded by navigation. A
quirks mode page from `page.set_content` after a standards mode page in
the same tab can keep the standards mode font size table.
"""

import asyncio
import html
import io
import math
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

from PIL import Image
from playwright.async_api import Page

from swbtools import browser
from swbtools.font_sizes import font_size_sweep

# --- Text field families ---------------------------------------------------

TEXT_FIELD_SIZE = 20
"""The `size` of the measured text fields."""

GENERIC_FAMILIES: tuple[str, ...] = (
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "math",
    "emoji",
    "fangsong",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
)
"""Generic family keywords. They are measured unquoted."""


def _names(block: str) -> tuple[str, ...]:
    """The names in `block`, one per line."""
    return tuple(line.strip() for line in block.splitlines() if line.strip())


WINDOWS_FAMILIES = _names(
    """
    Arial
    Arial Black
    Arial Narrow
    Arial Unicode MS
    Bahnschrift
    Batang
    BatangChe
    Book Antiqua
    Bookman Old Style
    Calibri
    Cambria
    Cambria Math
    Candara
    Century
    Century Gothic
    Comic Sans MS
    Consolas
    Constantia
    Corbel
    Courier New
    David
    Dotum
    DotumChe
    Ebrima
    Estrangelo Edessa
    Franklin Gothic Medium
    Gabriola
    Gadugi
    Garamond
    Gautami
    Georgia
    Gulim
    GulimChe
    Gungsuh
    GungsuhChe
    HoloLens MDL2 Assets
    Impact
    Ink Free
    Javanese Text
    Kartika
    Latha
    Leelawadee
    Leelawadee UI
    Lucida Console
    Lucida Sans
    Lucida Sans Unicode
    Malgun Gothic
    Mangal
    Marlett
    Meiryo
    Meiryo UI
    Microsoft Himalaya
    Microsoft JhengHei
    Microsoft New Tai Lue
    Microsoft PhagsPa
    Microsoft Sans Serif
    Microsoft Tai Le
    Microsoft YaHei
    Microsoft Yi Baiti
    MingLiU
    MingLiU-ExtB
    MingLiU_HKSCS
    Miriam
    Mongolian Baiti
    MS Gothic
    MS Mincho
    MS PGothic
    MS PMincho
    MS Reference Sans Serif
    MS Sans Serif
    MS Serif
    MS UI Gothic
    MV Boli
    Myanmar Text
    Nirmala UI
    NSimSun
    Palatino Linotype
    PMingLiU
    Raavi
    Segoe MDL2 Assets
    Segoe Print
    Segoe Script
    Segoe UI
    Segoe UI Emoji
    Segoe UI Historic
    Segoe UI Symbol
    Shruti
    SimHei
    SimSun
    SimSun-ExtB
    Sitka Text
    Sylfaen
    Tahoma
    Times New Roman
    Trebuchet MS
    Tunga
    Verdana
    Vrinda
    Webdings
    Wingdings
    Wingdings 2
    Wingdings 3
    Yu Gothic
    Yu Mincho
    """
)

MACOS_FAMILIES = _names(
    """
    Al Bayan
    Al Nile
    Al Tarikh
    American Typewriter
    Andale Mono
    Apple Braille
    Apple Chancery
    Apple Color Emoji
    Apple LiGothic
    Apple LiSung
    Apple SD Gothic Neo
    Apple Symbols
    AppleGothic
    AppleMyungjo
    AquaKana
    Arial Hebrew
    Arial Hebrew Scholar
    Arial Rounded MT Bold
    Athelas
    Avenir
    Avenir Next
    Avenir Next Condensed
    Ayuthaya
    Baghdad
    Bangla MN
    Bangla Sangam MN
    Baoli SC
    Baskerville
    Beirut
    BiauKai
    Big Caslon
    Bodoni 72
    Bodoni 72 Oldstyle
    Bodoni Ornaments
    Bradley Hand
    Brush Script MT
    Capitals
    Chalkboard
    Chalkboard SE
    Chalkduster
    Charcoal
    Charcoal CY
    Charter
    Chicago
    Cochin
    Copperplate
    Corsiva Hebrew
    Courier
    Damascus
    DecoType Naskh
    Devanagari MT
    Devanagari Sangam MN
    Didot
    DIN Alternate
    DIN Condensed
    Diwan Kufi
    Diwan Thuluth
    Euphemia UCAS
    Farah
    Farisi
    Futura
    Gadget
    GB18030 Bitmap
    Geeza Pro
    Geneva
    Geneva CY
    Gill Sans
    Gujarati MT
    Gujarati Sangam MN
    GungSeo
    Gurmukhi MN
    Gurmukhi MT
    Gurmukhi Sangam MN
    Hannotate SC
    HanziPen SC
    HeadLineA
    Hei
    Heiti SC
    Heiti TC
    Helvetica
    Helvetica CY
    Helvetica Neue
    Herculanum
    Hiragino Kaku Gothic Pro
    Hiragino Kaku Gothic ProN
    Hiragino Kaku Gothic Std
    Hiragino Maru Gothic Pro
    Hiragino Maru Gothic ProN
    Hiragino Mincho Pro
    Hiragino Mincho ProN
    Hiragino Sans
    Hiragino Sans GB
    Hoefler Text
    InaiMathi
    Inai Mathi
    Iowan Old Style
    ITF Devanagari
    Jung Gothic
    Kai
    Kailasa
    Kannada MN
    Kannada Sangam MN
    Kefa
    Khmer MN
    Khmer Sangam MN
    Kohinoor Bangla
    Kohinoor Devanagari
    Kokonor
    Krungthep
    KufiStandardGK
    Lantinghei SC
    Lao MN
    Lao Sangam MN
    LastResort
    LiHei Pro
    LiSong Pro
    Libian SC
    Lucida Grande
    Luminari
    Malayalam MN
    Malayalam Sangam MN
    Marion
    Marker Felt
    Menlo
    Mishafi
    Monaco
    Mshtakan
    Muna
    Myanmar MN
    Myanmar Sangam MN
    Nadeem
    Nanum Gothic
    Nanum Myeongjo
    New Peninim MT
    New York
    Noteworthy
    Optima
    Oriya MN
    Oriya Sangam MN
    Osaka
    Palatino
    Papyrus
    PCMyungjo
    Phosphate
    PilGi
    PingFang HK
    PingFang SC
    PingFang TC
    Plantagenet Cherokee
    PT Mono
    PT Sans
    PT Serif
    Raanana
    Rockwell
    San Francisco
    Sana
    Sand
    Sathu
    Savoye LET
    SF Mono
    SF Pro
    SF Pro Display
    SF Pro Text
    Shree Devanagari 714
    SignPainter
    Silom
    Sinhala MN
    Sinhala Sangam MN
    Skia
    Snell Roundhand
    Songti SC
    Songti TC
    STFangsong
    STHeiti
    STIXGeneral
    STKaiti
    STSong
    STXihei
    Sukhumvit Set
    Symbol
    Tamil MN
    Tamil Sangam MN
    Techno
    Telugu MN
    Telugu Sangam MN
    Textile
    Thonburi
    Times
    Trattatello
    Waseem
    Wawati SC
    Weibei SC
    Xingkai SC
    Yuanti SC
    Yuppy SC
    Zapf Dingbats
    Zapfino
    """
)

LINUX_FAMILIES = _names(
    """
    Abyssinica SIL
    AR PL UMing CN
    Arimo
    Bitstream Charter
    Bitstream Vera Sans
    Bitstream Vera Sans Mono
    Bitstream Vera Serif
    C059
    Caladea
    Cantarell
    Carlito
    Century Schoolbook L
    Comfortaa
    Courier 10 Pitch
    Cousine
    DejaVu Sans
    DejaVu Sans Mono
    DejaVu Serif
    Droid Sans
    Droid Sans Mono
    Droid Serif
    Fira Mono
    Fira Sans
    FreeMono
    FreeSans
    FreeSerif
    Garuda
    Gentium
    Inter
    IPAGothic
    IPAMincho
    Kinnari
    Lato
    Latin Modern Roman
    Liberation Mono
    Liberation Sans
    Liberation Sans Narrow
    Liberation Serif
    Linux Biolinum
    Linux Libertine
    Lohit Devanagari
    Loma
    Mono
    Monospace
    Montserrat
    Nimbus Mono PS
    Nimbus Roman
    Nimbus Sans
    Noto Color Emoji
    Noto Mono
    Noto Sans
    Noto Sans CJK JP
    Noto Sans Mono
    Noto Serif
    Open Sans
    Oxygen
    P052
    Padauk
    Purisa
    Raleway
    Roboto
    Roboto Mono
    Sans
    Sans Serif
    Serif
    Source Code Pro
    Source Han Sans
    Source Sans Pro
    Standard Symbols PS
    TakaoGothic
    TeX Gyre Heros
    Tinos
    Tlwg Typo
    Ubuntu
    Ubuntu Mono
    UnDotum
    URW Bookman
    URW Gothic
    URW Palladio L
    VL Gothic
    WenQuanYi Micro Hei
    Z003
    """
)

OTHER_FAMILIES: tuple[str, ...] = (
    # Generic family names as strings (a quoted name is not a keyword).
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    # Names that start with "#" or "." (some systems hide such fonts).
    "#GungSeo",
    "#HeadLineA",
    "#PCMyungjo",
    "#PilGi",
    "#Seoul",
    "#Gulim",
    "#Dotum",
    "#Batang",
    "#Gungsuh",
    "#AppleGothic",
    "#AppleMyungjo",
    "#Nanum Gothic",
    "#swb-probe",
    ".AppleSystemUIFont",
    ".Helvetica Neue DeskInterface",
    ".LucidaGrandeUI",
    ".SF NS Text",
    ".swb-probe",
    # Other spellings of names above.
    "courier",
    "COURIER",
    "helvetica",
    "HELVETICA",
    "times",
    "lucida grande",
    "LucidaGrande",
    "Times Roman",
    "Times-Roman",
    "Monaco ",
    " Monaco",
    "Osaka-Mono",
    "Osaka−等幅",  # noqa: RUF001 (the font's name has a minus sign, U+2212)
    "Helvetica Bold",
    "Apple Gothic",
    "Apple Myungjo",
    "Apple Garamond",
    "Apple LiGothic Medium",
    "Apple LiSung Light",
    "LiGothicMed",
    "LiSungLight",
    "Seoul",
    "ヒラギノ角ゴ Pro W3",
    "Lucida Sans Typewriter",
    "Lucida Bright",
    "Monotype Corsiva",
    "Gill Sans MT",
    "Euphemia",
)

FAMILY_NAMES: tuple[str, ...] = (
    *WINDOWS_FAMILIES,
    *MACOS_FAMILIES,
    *LINUX_FAMILIES,
    *OTHER_FAMILIES,
)
"""Family names to measure: font families of Windows, macOS (also older
releases) and Linux, common web fonts, generic names as strings, names
that start with `#` or `.`, and other spellings."""

POSITION_PROBES: tuple[str, ...] = (
    '"Times", serif',
    'serif, "Times"',
    '"swb-probe-missing", "Times"',
)
"""`font-family` lists that show which family of the list counts."""

_CONTROL_FAMILY = '"swb-probe-control"'
"""A family name that is not installed and not special."""


def css_string(name: str) -> str:
    """Returns `name` as a CSS string."""
    return '"' + name.replace("\\", "\\\\").replace('"', '\\"') + '"'


@dataclass(frozen=True)
class FieldWidth:
    """The content width of a text field with a `font-family` value, and
    the two widths that Chromium can give it."""

    family: str
    """The `font-family` value."""
    field: float
    """The measured width."""
    average: float
    """The width with the same font when the family is not special: the
    average character width rule."""
    zero: float
    """`size` times the width of `0`, rounded up."""

    @property
    def verdict(self) -> str:
        """`zero`, `average`, or `undetermined` (when the two rules give
        the same width or the field matches neither)."""
        if abs(self.field - self.zero) <= 1 and abs(self.field - self.average) >= 2:
            return "zero"
        if self.field == self.average and abs(self.average - self.zero) >= 2:
            return "average"
        return "undetermined"


_FIELD_JS = """
() => [...document.querySelectorAll('.row')].map(row => ({
  field: parseFloat(getComputedStyle(row.querySelector('.field')).width),
  average: parseFloat(getComputedStyle(row.querySelector('.control')).width),
  zeros: row.querySelector('.zero').getBoundingClientRect().width,
}))
"""


def _field_page(families: Sequence[str]) -> str:
    rows = []
    zeros = "0" * TEXT_FIELD_SIZE
    for family in families:
        value = html.escape(family, quote=True)
        control = html.escape(f"{_CONTROL_FAMILY}, {family}", quote=True)
        rows.append(
            "<div class=row>"
            f'<input class=field size={TEXT_FIELD_SIZE} style="font-family: {value}">'
            f'<input class=control size={TEXT_FIELD_SIZE} style="font-family: {control}">'
            f'<span class=zero style="font-family: {value}">{zeros}</span></div>'
        )
    return "<!DOCTYPE html><style>input, span { font-size: 16px }</style>" + "".join(rows)


async def text_field_widths(
    page: Page, directory: Path, families: Sequence[str]
) -> list[FieldWidth]:
    """Measures a text field for each `font-family` value.

    The control field has an unknown family first and then the measured
    value, so it uses the same font. A family that changes the rule makes
    the field differ from the control field."""
    await browser.load_html(page, directory / "text-field-families.html", _field_page(families))
    rows = await page.evaluate(_FIELD_JS)
    return [
        FieldWidth(family, row["field"], row["average"], math.ceil(row["zeros"] - 0.001))
        for family, row in zip(families, rows, strict=True)
    ]


def family_candidates() -> list[str]:
    """The `font-family` values that `text-field-families` measures."""
    return [*GENERIC_FAMILIES, *(css_string(name) for name in FAMILY_NAMES)]


def zero_width_families(widths: Sequence[FieldWidth]) -> list[str]:
    """The family names (unquoted) whose fields use the width of `0`, in
    byte order."""
    names = dict(zip((css_string(n) for n in FAMILY_NAMES), FAMILY_NAMES, strict=True))
    return sorted(names[w.family] for w in widths if w.verdict == "zero" and w.family in names)


async def _measure_text_field_families(page: Page, directory: Path) -> None:
    widths = await text_field_widths(page, directory, family_candidates())
    probes = await text_field_widths(page, directory, POSITION_PROBES)
    print(
        f"Content width of <input size={TEXT_FIELD_SIZE}> at 16px, by font-family: the field,"
        " the field with an unknown family first (average character width), and size times"
        " the width of '0' (rounded up)."
    )
    print(f"{'font-family':36} {'field':>7} {'average':>8} {'zero':>6}  rule")
    for w in [*widths, *probes]:
        print(f"{w.family:36} {w.field:7g} {w.average:8g} {w.zero:6g}  {w.verdict}")
    undetermined = [w.family for w in widths if w.verdict == "undetermined"]
    print("\nFamilies whose text fields use the width of '0' (byte order):")
    for name in zero_width_families(widths):
        print(f"  {name}")
    if undetermined:
        print("\nUndetermined:", ", ".join(undetermined))


# --- Font size keywords ----------------------------------------------------

FONT_SIZE_KEYWORDS: tuple[str, ...] = (
    "xx-small",
    "x-small",
    "small",
    "medium",
    "large",
    "x-large",
    "xx-large",
    "xxx-large",
)


def _keyword_page(doctype: bool) -> str:
    rows = [
        f'<div style="font-family: {family}; font-size: {keyword}">x</div>'
        for family in ("serif", "monospace")
        for keyword in FONT_SIZE_KEYWORDS
    ]
    return ("<!DOCTYPE html>" if doctype else "") + "<body>" + "".join(rows)


async def font_size_keywords(page: Page, directory: Path) -> dict[str, list[float]]:
    """Measures the computed font size of each absolute-size keyword with
    a proportional family (medium: 16px) and with `monospace` (medium:
    13px), in standards mode and in quirks mode. Returns rows named
    `16px standards`, `16px quirks`, `13px standards`, `13px quirks`."""
    by_mode: dict[str, list[float]] = {}
    for doctype, mode in ((True, "standards"), (False, "quirks")):
        await browser.load_html(page, directory / f"font-size-{mode}.html", _keyword_page(doctype))
        compat = await page.evaluate("document.compatMode")
        expected = "CSS1Compat" if doctype else "BackCompat"
        if compat != expected:
            raise RuntimeError(f"the {mode} page is in {compat}")
        by_mode[mode] = await page.evaluate(
            "[...document.querySelectorAll('div')]"
            ".map(d => parseFloat(getComputedStyle(d).fontSize))"
        )
    count = len(FONT_SIZE_KEYWORDS)
    return {
        "16px standards": by_mode["standards"][:count],
        "16px quirks": by_mode["quirks"][:count],
        "13px standards": by_mode["standards"][count:],
        "13px quirks": by_mode["quirks"][count:],
    }


async def _measure_font_size_keywords(page: Page, directory: Path) -> None:
    table = await font_size_keywords(page, directory)
    print("Computed font-size in px (serif: medium 16px; monospace: medium 13px):")
    print(f"{'':16}" + "".join(f"{k:>10}" for k in FONT_SIZE_KEYWORDS))
    for row, sizes in table.items():
        print(f"{row:16}" + "".join(f"{s:10g}" for s in sizes))


# --- User-agent styles -----------------------------------------------------

_SIDES = ("top", "right", "bottom", "left")
_CORNERS = ("top-left", "top-right", "bottom-right", "bottom-left")

# The inherited properties of `UA_PROPERTIES`. For `::placeholder` only
# these are reported: the others are not inherited from the element, and
# Chromium's placeholder box is internal.
_INHERITED: tuple[str, ...] = (
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "font-stretch",
    "font-variant-caps",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "color",
    "text-align",
    "text-indent",
    "text-transform",
    "white-space",
    "overflow-wrap",
    "word-break",
    "cursor",
)

UA_PROPERTIES: tuple[str, ...] = (
    "display",
    *(f"margin-{side}" for side in _SIDES),
    *(f"padding-{side}" for side in _SIDES),
    *(f"border-{side}-{part}" for part in ("width", "style", "color") for side in _SIDES),
    *(f"border-{corner}-radius" for corner in _CORNERS),
    "box-sizing",
    *_INHERITED,
    "background-color",
    "background-image",
    "overflow-x",
    "overflow-y",
    "user-select",
    "vertical-align",
    "align-content",
    "align-items",
    "min-width",
    "unicode-bidi",
    "text-overflow",
)
"""The properties that `ua-styles` reports: those that swb supports and
that the user-agent stylesheet sets for the measured elements."""

UA_ELEMENTS: tuple[str, ...] = (
    "<input>",
    "<input type=search>",
    "<input type=checkbox>",
    "<input type=radio>",
    "<input type=hidden>",
    "<input type=image>",
    "<input type=file>",
    "<input type=range>",
    "<input type=button>",
    "<input type=submit>",
    "<input type=reset>",
    "<button>x</button>",
    "<select><option>x</select>",
    "<select><option data-measure>x</select>",
    "<textarea></textarea>",
    '<input style="color: red">',
    '<button style="color: red">x</button>',
    "<input disabled>",
    "<input type=checkbox disabled>",
    "<input type=image disabled>",
    "<input type=submit disabled>",
    "<button disabled>x</button>",
    "<textarea disabled></textarea>",
    "<label>x</label>",
    "<fieldset></fieldset>",
    "<marquee>x</marquee>",
    "<meter></meter>",
    "<progress></progress>",
    "<output>x</output>",
    "<ruby>x</ruby>",
    "<ruby>x<rt data-measure>y</rt></ruby>",
    "<rt>y</rt>",
    "<map></map>",
)
"""The measured elements: the element with the `data-measure` attribute,
else the first element."""

UA_PARENT_STYLE = (
    "color: rgb(1, 2, 3); font: italic small-caps 600 condensed 17px/30px 'Liberation Sans';"
    " letter-spacing: 1px; word-spacing: 2px; text-transform: uppercase; text-indent: 5px;"
    " text-align: right; cursor: crosshair; white-space: pre-line; overflow-wrap: anywhere;"
    " word-break: break-all"
)
"""The style of the parent of the measured elements: values that differ
from the defaults, so that the report shows each inherited property that
the user-agent stylesheet sets."""

SYSTEM_COLORS: tuple[str, ...] = (
    "Field",
    "FieldText",
    "ButtonFace",
    "ButtonText",
    "ButtonBorder",
    "ThreeDFace",
    "GrayText",
)
"""System colors that the measured values can come from."""

_UA_JS = """
(props) => {
  const read = (style) => Object.fromEntries(props.map(p => [p, style.getPropertyValue(p)]));
  return [...document.querySelectorAll('.row')].map(row => {
    const el = row.querySelector('[data-measure]') ?? row.firstElementChild;
    const own = read(getComputedStyle(el));
    const ref = read(getComputedStyle(row.querySelector('span.ref')));
    const placeholder = el.hasAttribute('placeholder')
      ? read(getComputedStyle(el, '::placeholder')) : null;
    return {own, ref, placeholder};
  });
}
"""

_SYSTEM_COLORS_JS = """
(colors) => {
  const probe = document.createElement('div');
  document.body.appendChild(probe);
  return Object.fromEntries(colors.map(c => {
    probe.style.color = c;
    return [c, getComputedStyle(probe).color];
  }));
}
"""

_FRAMESET_PAGE = (
    "<html style='border-color: rgb(9, 9, 9)'><frameset cols='50%,50%'><frame><frame></frameset>"
)


def _ua_page(elements: Sequence[str]) -> str:
    rows = "".join(
        f"<div class=row>{_with_placeholder(element)}<span class=ref>r</span></div>"
        for element in elements
    )
    return f'<!DOCTYPE html><body style="{UA_PARENT_STYLE}">{rows}'


def _with_placeholder(element: str) -> str:
    """Adds a placeholder to text fields and text areas, so that their
    `::placeholder` style exists."""
    text_field = element.startswith("<input") and "type=" not in element
    if text_field or element.startswith("<textarea"):
        return element.replace(">", " placeholder=p>", 1)
    return element


@dataclass(frozen=True)
class UaStyle:
    """The user-agent style of one measured element."""

    element: str
    set_values: dict[str, str]
    """The properties whose value differs from a `<span>` next to the
    element (and `display`)."""
    placeholder: dict[str, str] | None
    """`::placeholder` properties that differ from the element's."""


async def ua_styles(page: Page, directory: Path) -> tuple[list[UaStyle], dict[str, str], str]:
    """Measures the computed styles of `UA_ELEMENTS`, the system colors and
    the frameset elements. Returns the styles, the system colors and a
    report line for the frameset document."""
    await browser.load_html(page, directory / "ua-styles.html", _ua_page(UA_ELEMENTS))
    rows = await page.evaluate(_UA_JS, list(UA_PROPERTIES))
    styles = []
    for element, row in zip(UA_ELEMENTS, rows, strict=True):
        own, ref, placeholder = row["own"], row["ref"], row["placeholder"]
        set_values = {p: v for p, v in own.items() if v != ref[p] or p == "display"}
        if placeholder is not None:
            placeholder = {p: v for p, v in placeholder.items() if v != own[p]}
        styles.append(UaStyle(element, set_values, placeholder))
    colors = await page.evaluate(_SYSTEM_COLORS_JS, list(SYSTEM_COLORS))
    await browser.load_html(page, directory / "frameset.html", _FRAMESET_PAGE)
    frameset = await page.evaluate(
        """() => ['frameset', 'frame'].map(tag => {
          const s = getComputedStyle(document.querySelector(tag));
          return `${tag}: display ${s.display}; border-top-color ${s.borderTopColor}`;
        }).join('; ')"""
    )
    return styles, colors, frameset


async def _measure_ua_styles(page: Page, directory: Path) -> None:
    styles, colors, frameset = await ua_styles(page, directory)
    print(f"Parent style: {UA_PARENT_STYLE}")
    print("Values that differ from a <span> next to the element (and display):\n")
    for style in styles:
        print(style.element)
        for prop, value in _collapse(style.set_values).items():
            print(f"  {prop}: {value}")
        if style.placeholder:
            shown = {k: v for k, v in style.placeholder.items() if k in _INHERITED}
            if shown:
                print("  ::placeholder " + "; ".join(f"{k}: {v}" for k, v in shown.items()))
    print("\nFrameset document (<html> border-color rgb(9, 9, 9)):", frameset)
    print("\nSystem colors:", "; ".join(f"{k} {v}" for k, v in colors.items()))


def _side_group(prop: str) -> tuple[str, list[str]] | None:
    """For a property of one side or corner (`margin-top`), the name of the
    group (`margin-*`) and the properties of all four."""
    padded = f"-{prop}-"
    for names in (_CORNERS, _SIDES):
        for side in names:
            token = f"-{side}-"
            if token in padded:
                group = [padded.replace(token, f"-{s}-", 1).strip("-") for s in names]
                return padded.replace(token, "-*-", 1).strip("-"), group
    return None


def _collapse(values: dict[str, str]) -> dict[str, str]:
    """Writes four equal side values as one entry (`margin-*: 3px`)."""
    out: dict[str, str] = {}
    done: set[str] = set()
    for prop, value in values.items():
        if prop in done:
            continue
        group = _side_group(prop)
        if group and all(values.get(p) == value for p in group[1]):
            out[group[0]] = value
            done.update(group[1])
        else:
            out[prop] = value
    return out


# --- Picture sources -------------------------------------------------------

PICTURE_CASES: dict[str, str] = {
    "media does not match": 'media="(max-width: 1px)"',
    "type not supported": 'type="image/x-swb-unsupported"',
    "media does not match, type not supported": (
        'media="(max-width: 1px)" type="image/x-swb-unsupported"'
    ),
    "media matches, type supported": 'media="(min-width: 1px)" type="image/png"',
}
"""Attributes of a `<source srcset=source.png>` before `<img
src=fallback.png>`."""


def _png() -> bytes:
    out = io.BytesIO()
    Image.new("RGB", (1, 1), (0, 128, 0)).save(out, format="PNG")
    return out.getvalue()


async def picture_sources(page: Page, directory: Path) -> dict[str, str]:
    """Returns the file name of the image that each `PICTURE_CASES`
    picture shows (`source.png` or `fallback.png`)."""
    png = _png()
    (directory / "source.png").write_bytes(png)
    (directory / "fallback.png").write_bytes(png)
    pictures = "".join(
        f"<picture><source {attrs} srcset=source.png><img src=fallback.png></picture>"
        for attrs in PICTURE_CASES.values()
    )
    await browser.load_html(page, directory / "picture.html", f"<!DOCTYPE html><body>{pictures}")
    sources = await page.evaluate(
        "[...document.images].map(img => img.currentSrc.split('/').pop())"
    )
    return dict(zip(PICTURE_CASES, sources, strict=True))


async def _measure_picture_sources(page: Page, directory: Path) -> None:
    print("Image that a <picture> shows, by the attributes of its <source>:")
    for case, source in (await picture_sources(page, directory)).items():
        print(f"  {case:44} {source}")


# --- Running ---------------------------------------------------------------

MEASUREMENTS = {
    "text-field-families": _measure_text_field_families,
    "font-size-keywords": _measure_font_size_keywords,
    "ua-styles": _measure_ua_styles,
    "picture-sources": _measure_picture_sources,
    "font-size-sweep": font_size_sweep,
}

SLOW_MEASUREMENTS = ("font-size-sweep",)
"""The measurements that run only when named: they take minutes or write
files."""


def measure(names: list[str]) -> int:
    """Runs the named measurements (all but `SLOW_MEASUREMENTS` if empty) and
    prints the results."""
    for name in names or [n for n in MEASUREMENTS if n not in SLOW_MEASUREMENTS]:
        print(f"== {name}")
        asyncio.run(browser.in_chromium(MEASUREMENTS[name]))
        print()
    return 0
