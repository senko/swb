"""`swbtools linebreaks`: measures Chromium's line break opportunities.

swb's line breaking (`crates/text/src/linebreak.rs`) is written from UAX #14
and from black-box measurements of Chromium. This module makes the
measurements and writes them to `crates/text/tests/linebreak/`, where the
Rust test `crates/text/tests/linebreak.rs` compares them with swb, and
`swbtools linebreak-tables` makes swb's Latin-1 pair tables from them.

Method: each string is the text of its own `<div>` with `width: 0`, so that
the line breaks at every opportunity (and only there: `overflow-wrap` is
`normal`). The line height is 100 px and the font size 10 px, so the
vertical center of a character's rectangle (`Range.getClientRects()`) gives
its line, whatever the font of the glyph. The line can break before a code
point that is on a later line than the code point before it. The same
strings with `width: 100000px` give the forced breaks and the width of each
code point.

Details:

- The tool takes the last rectangle of each code point. After a break at
  U+00AD SOFT HYPHEN, the rectangles of the characters around the break
  also contain the generated hyphen, which is on the line before.
- Code points without a rectangle are skipped. The break is then reported
  before the next code point that has one.
- The number of lines of the `<div>` (its height / 100 px) must be the
  number of breaks plus one. A string where this is not so is inconsistent:
  all its positions are reported as unknown.
- Content of zero width fits into the zero-width box: the line does not
  break between zero-width characters (U+200B, U+2060, marks) at the start
  of a line. So the positions between two breaks (or the ends of the text)
  are unknown if all code points there have zero width.
- The style is `white-space: pre-wrap`: swb gets its text after white-space
  collapsing, and with `pre-wrap` Chromium also breaks the text as it is,
  with every space and tab. No `lang` attribute: the locale is Chromium's
  default (en-US).

The data format is described in `crates/text/tests/linebreak/README.md`.
"""

import asyncio
import logging
from collections.abc import Iterator, Sequence
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path

from playwright.async_api import Page, async_playwright

from swbtools import browser, paths

log = logging.getLogger(__name__)

LINE_HEIGHT = 100
"""Line height of the measured text, in CSS px."""

BASE_STYLE = f"font: 10px/{LINE_HEIGHT}px monospace; white-space: pre-wrap"
"""Style of each measured `<div>`; the mode adds its declarations."""

WIDE = 100000
"""Width in CSS px that no measured string reaches: breaks there are forced."""

BATCH = 4000
"""Strings per `page.evaluate` call."""

# For each string: [line, width] of each code point (line -1: no
# rectangle) and the number of lines of the box.
_MEASURE_JS = """
([texts, style, width, lineHeight]) => {
  const root = document.createElement('div');
  document.body.appendChild(root);
  const boxes = texts.map((text) => {
    const box = document.createElement('div');
    box.style.cssText = style;
    box.style.width = width + 'px';
    box.textContent = text;
    root.appendChild(box);
    return box;
  });
  const range = document.createRange();
  const out = boxes.map((box) => {
    const outer = box.getBoundingClientRect();
    const points = [];
    const node = box.firstChild;
    if (node) {
      const data = node.data;
      for (let i = 0; i < data.length; ) {
        const n = data.codePointAt(i) > 0xffff ? 2 : 1;
        range.setStart(node, i);
        range.setEnd(node, i + n);
        const rects = range.getClientRects();
        let line = -1;
        let w = 0;
        if (rects.length > 0) {
          const r = rects[rects.length - 1];
          line = Math.floor((r.top + r.height / 2 - outer.top) / lineHeight);
          for (const each of rects) w += each.width;
        }
        points.push([line, w]);
        i += n;
      }
    }
    return [points, Math.round(outer.height / lineHeight)];
  });
  root.remove();
  return out;
}
"""


@dataclass(frozen=True)
class Mode:
    """The CSS properties of a measurement."""

    word_break: str = "normal"
    """`word-break`: normal, break-all or keep-all."""
    hyphens: str = "manual"
    """`hyphens`: manual or none."""

    def css(self) -> str:
        """The declarations for the measured `<div>`."""
        return f"word-break: {self.word_break}; hyphens: {self.hyphens}"

    def directive(self) -> str:
        """The mode in the data files: `word-break=... hyphens=...`."""
        return f"word-break={self.word_break} hyphens={self.hyphens}"


NORMAL = Mode()
BREAK_ALL = Mode("break-all")
KEEP_ALL = Mode("keep-all")
NO_HYPHENS = Mode(hyphens="none")
BREAK_ALL_NO_HYPHENS = Mode("break-all", "none")
WORD_BREAK_MODES = (NORMAL, BREAK_ALL, KEEP_ALL)

ALLOWED = "÷"
FORCED = "!"
NONE = "×"
UNKNOWN = "?"


@dataclass(frozen=True)
class Breaks:
    """The measured opportunity before each code point of a text:
    `marks[i]` is the mark for the position before `text[i + 1]` (counted in
    code points): ALLOWED, FORCED, NONE or UNKNOWN."""

    marks: tuple[str, ...]

    def allowed(self) -> list[int]:
        """The code point indices before which the line can break."""
        return [i + 1 for i, m in enumerate(self.marks) if m in (ALLOWED, FORCED)]


def breaks_from_lines(lines: Sequence[int]) -> list[int] | None:
    """The code point indices before which a new line starts. Returns None if
    the lines do not increase in text order."""
    out: list[int] = []
    last: int | None = None
    for i, line in enumerate(lines):
        if line < 0:
            continue
        if last is not None:
            if line < last:
                return None
            if line > last:
                out.append(i)
        last = line
    return out


def marks_for(
    count: int, narrow: list[int] | None, forced: list[int] | None, widths: Sequence[float]
) -> tuple[str, ...]:
    """The marks of a text of `count` code points from the breaks at width 0
    (`narrow`), the forced breaks and the width of each code point."""
    if count < 2:
        return ()
    if narrow is None or forced is None:
        return (UNKNOWN,) * (count - 1)
    marks = [NONE] * (count - 1)
    for i in narrow:
        marks[i - 1] = FORCED if i in forced else ALLOWED
    # Positions inside a region of zero width are unknown.
    edges = [0, *narrow, count]
    for start, end in pairwise(edges):
        if all(w <= 0.01 for w in widths[start:end]):
            for i in range(start + 1, end):
                marks[i - 1] = UNKNOWN
    return tuple(marks)


async def _measure_width(
    page: Page, texts: Sequence[str], style: str, width: int
) -> list[tuple[list[int] | None, list[float]]]:
    """For each text: the breaks (None if inconsistent) and the widths."""
    results: list[tuple[list[int] | None, list[float]]] = []
    for start in range(0, len(texts), BATCH):
        batch = list(texts[start : start + BATCH])
        data = await page.evaluate(_MEASURE_JS, [batch, style, width, LINE_HEIGHT])
        for text, (points, count) in zip(batch, data, strict=True):
            lines = [line for line, _ in points]
            found = breaks_from_lines(lines)
            if found is None or len(found) + 1 != max(count, 1):
                log.warning("inconsistent lines for %s: %s, %s lines", show(text), lines, count)
                found = None
            results.append((found, [w for _, w in points]))
    return results


async def measure(page: Page, texts: Sequence[str], mode: Mode = NORMAL) -> list[Breaks]:
    """Measures the break opportunities of each text in `mode`."""
    style = f"{BASE_STYLE}; {mode.css()}"
    narrow = await _measure_width(page, texts, style, 0)
    wide = await _measure_width(page, texts, style, WIDE)
    out: list[Breaks] = []
    for (allowed, _), (forced, widths) in zip(narrow, wide, strict=True):
        out.append(Breaks(marks_for(len(widths), allowed, forced, widths)))
    return out


def show(text: str) -> str:
    """The code points of `text` in hex."""
    return " ".join(f"{ord(c):04X}" for c in text)


class Session:
    """A Chromium page for measurements."""

    def __init__(self, system_fonts: bool = False) -> None:
        self._system_fonts = system_fonts
        self._playwright = None
        self._browser = None
        self.page: Page | None = None

    async def __aenter__(self) -> "Session":
        self._playwright = await async_playwright().start()
        self._browser = await browser.launch(self._playwright, self._system_fonts)
        context = await browser.new_context(self._browser, browser.LAYOUT_VIEWPORT)
        self.page = await context.new_page()
        await self.page.set_content("<!DOCTYPE html><html><body></body></html>")
        return self

    async def __aexit__(self, *_: object) -> None:
        if self._browser is not None:
            await self._browser.close()
        if self._playwright is not None:
            await self._playwright.stop()

    @property
    def version(self) -> str:
        """The Chromium version."""
        assert self._browser is not None, "the session is open"
        return self._browser.version

    async def measure(self, texts: Sequence[str], mode: Mode = NORMAL) -> list[Breaks]:
        """See `measure`."""
        assert self.page is not None, "the session is open"
        return await measure(self.page, texts, mode)


def format_case(text: str, breaks: Breaks) -> str:
    """One line of a case file: the code points in hex, separated by the
    marks."""
    parts = [f"{ord(text[0]):04X}"] if text else []
    for c, mark in zip(text[1:], breaks.marks, strict=True):
        parts += [mark, f"{ord(c):04X}"]
    return " ".join(parts)


# --- Matrices ---------------------------------------------------------------

LATIN1 = "".join(chr(c) for c in range(0x20, 0x100))
"""U+0020..U+00FF."""

# Representative characters of each UAX #14 line break class (Unicode 17.0,
# LineBreak.txt), mostly outside Latin-1.
CLASS_SAMPLES: dict[str, str] = {
    "AI": "\u2460\u2200\u2190",  # circled digit one, for all, leftwards arrow
    "AK": "\u1b05\ua984",  # Balinese A, Javanese A
    "AL": "\u0430\u00c0\u25cc\u0915",  # Cyrillic a, A grave, dotted circle, Devanagari ka
    "AP": "\U00011003",  # Brahmi sign jihvamuliya
    "AS": "\u1b50\u1bc0",  # Balinese digit zero, Batak A
    "B2": "\u2014\u2e3a",  # em dash, two-em dash
    "BA": "\u2027\u0964\u2002\u1680\u3000",  # hyphenation point, danda, spaces
    "BB": "\u02c8\u00b4",  # modifier vertical line, acute accent
    "BK": "\u2028",  # line separator
    "CB": "\ufffc",  # object replacement character
    "CJ": "\u3041\u30a1",  # small hiragana a, small katakana a
    "CL": "\u3001\u300d\uff09",  # ideographic comma, corner bracket, fullwidth )
    "CM": "\u0301\u093f\u20dd",  # combining acute, Devanagari sign i, enclosing circle
    "CP": "\u2e56",  # right square bracket with stroke
    "EB": "\u261d\U0001f466",  # index pointing up, boy
    "EM": "\U0001f3fb",  # skin tone modifier
    "EX": "\u05c6\uff01",  # Hebrew nun hafukha, fullwidth !
    "GL": "\u202f\u2007\u0f0c",  # narrow nbsp, figure space, Tibetan tsheg
    "H2": "\uac00",
    "H3": "\uac01",
    "HH": "\u2010\u2013\u058a",  # hyphen, en dash, Armenian hyphen
    "HL": "\u05d0",  # alef
    "HY": "-",
    "ID": "\u4e00\u3042\U0001f600\u2615",  # ideograph, hiragana, emoji
    "IN": "\u2026",  # ellipsis
    "IS": "\u037e\u060c",  # Greek question mark, Arabic comma
    "JL": "\u1100",
    "JT": "\u11a8",
    "JV": "\u1161",
    "NL": "\u0085",
    "NS": "\u3005\u30fb\u203c\u17d6",
    "NU": "\u0660\u0966",  # Arabic-Indic and Devanagari zero
    "OP": "\u3008\uff08\u201a\u2e18",  # angle bracket, fullwidth (, low-9 quote
    "PO": "\u2030\u2103",  # per mille, degree Celsius
    "PR": "\u20ac\u2116",  # euro, numero
    "QU": "\u2018\u2019\u201c\u201d\u2039\u203a\u2e00",
    "RI": "\U0001f1e6",
    "SA": "\u0e01\u0e31\u1000\u1780",  # Thai ko kai, mai han-akat (Mn), Myanmar, Khmer
    "SP": " ",
    "SY": "/",
    "VF": "\u1bf2",  # Batak pangolat
    "VI": "\u1b44\ua9c0",  # Balinese adeg adeg, Javanese pangkon
    "WJ": "\u2060\ufeff",
    "XX": "\ue000\u0378",  # private use, unassigned
    "ZW": "\u200b",
    "ZWJ": "\u200d",
}
"""Representative characters of each line break class."""

SAMPLES = (
    "".join(CLASS_SAMPLES.values())
    + "a0!?\"'(),.-/%$+#;:]}|\\<"
    + "\u00e9\u00ab\u00bb\u00a1\u00a0\u00ad"
)
"""The characters of the class matrices: the class samples, ASCII and
Latin-1 characters (Chromium treats Latin-1 pairs differently from other
characters of the same class)."""


@dataclass(frozen=True)
class Matrix:
    """Break opportunities of `left + x + y + right` for every pair (x, y)
    of `chars`: `marks[i][j]` is the mark between `chars[i]` and
    `chars[j]`."""

    left: str
    right: str
    mode: Mode
    chars: str
    marks: list[list[str]]


async def measure_matrix(
    session: Session, chars: str, left: str, right: str, mode: Mode = NORMAL
) -> Matrix:
    """Measures the pairs of `chars` between `left` and `right`."""
    texts = [left + x + y + right for x in chars for y in chars]
    results = await session.measure(texts, mode)
    n = len(chars)
    at = len(left)  # the mark index of the x|y position
    marks = [[UNKNOWN] * n for _ in range(n)]
    for k, found in enumerate(results):
        mark = found.marks[at]
        marks[k // n][k % n] = UNKNOWN if mark == FORCED else mark
    return Matrix(left, right, mode, chars, marks)


MATRIX_SYMBOLS = {ALLOWED: "/", NONE: ".", UNKNOWN: "?"}
"""The cells of a matrix in the data files."""


def format_matrix(matrix: Matrix) -> list[str]:
    """The lines of a matrix in a data file."""
    lines = [
        f"@matrix left={ord(matrix.left):04X} right={ord(matrix.right):04X} "
        f"{matrix.mode.directive()}",
        "@chars " + show(matrix.chars),
    ]
    lines += ["".join(MATRIX_SYMBOLS[m] for m in row) for row in matrix.marks]
    return lines


# --- Cases ------------------------------------------------------------------


@dataclass(frozen=True)
class CaseGroup:
    """Strings measured in some modes."""

    title: str
    texts: list[str]
    modes: tuple[Mode, ...] = WORD_BREAK_MODES


def _hyphen_contexts() -> list[str]:
    """`x L - y` for many characters L: the break after the hyphen depends
    on L before a digit and before a letter outside ASCII (LB20a, LB21a)."""
    lefts = [chr(c) for c in range(0x21, 0x100) if not 0x7F <= c <= 0x9F]
    lefts += [c for c in SAMPLES if ord(c) > 0xFF and c not in "\u200b\u2060\ufeff\u200d"]
    out = []
    for left in lefts:
        for tail in ("-0", "-\u00e9", "-\u00a9", "-\u00d7", "-\u0430", "\u2010a", "-\u0660"):
            out.append("x" + left + tail)
    return out


SPACES_AND_CONTROLS = [
    "a b",
    "a  b",
    "a\tb",
    "a \tb",
    "a\t b",
    "a\t\tb",
    "a  )",
    "a\t)",
    "( a",
    "(\ta",
    "a ;b",
    "a !b",
    "a\u00a0 b",
    "a \u00a0b",
    "a ,5",
    "subtract .5",
    "a \u2060b",
    "a \u200bb",
    "\u2014 \u2014",
    "\uff09 \u3005",
    "\u3008 a",
    "\u201c a",
    "\u00ab a",
    "a \u00bb",
    "a\u3000b",
    "a \u3000b",
    "a\u3000 b",
    "a\u3000\u3000b",
    # After spaces and U+3000, the line can break before anything.
    "a \u3000\u200bb",
    "a\u3000\u200bb",
    "a  \u3000\u200bb",
    "a\t\u3000\u200bb",
    "a \u3000\u3000\u200bb",
    "a \u3000\u2060b",
    "a \u3000\ufeffb",
    "a \u3000\u200db",
    "a \u3000\u0301b",
    "a\u3000)",
    "a\u3000\u3001",
    "\u4e00\u3000\u4e00",
    *[f"a {s}b" for s in "\u1680\u2000\u2002\u2006\u2008\u200a\u205f\u180e\u202f"],
    *[f"a{s})" for s in "\u1680\u2000\u2002\u2006\u2008\u200a\u205f"],
    "a\u0085b",
    "a \u0085b",
    "a\u0085 b",
    "a\u2028b",
    "a \u2028b",
    "a\u2028 b",
    "a\u2029b",
    "a\u2028\u2028b",
    "a\u000bb",
    "a \u000bb",
    "a\u000b b",
    "a\u000cb",
    "x\u000c-a",
    "x-\u000ca",
    "a\u0001b",
    "a\u001fb",
    "a\u0001)",
    "(\u0001a",
    "x-\u0001a",
    "x?\u0001a",
    "x-\u007fa",
    "x?\u007fa",
    "x-\u0080a",
    "\u4e00\u0001(",
    "\u4e00\u0001a",
    "\u0430\u0001(",
    "a-\u3000)",
    "a.\u3000)",
    " \u0001\u0002\u00b4",
    " \u0001\u0001\u00b4",
    "a\u200bb",
    "a\u200b b",
    "a\u200b\u2060b",
    "a\u2060b",
    "a\ufeffb",
    "a\u200db",
    "a\u200b\u0301b",
    "a \u0301b",
    "a\u0301\u0301b",
    "a\ufffcb",
    "a \ufffcb",
    "a\ufffc)",
    "(\ufffca",
    "a\ufffc\ufffcb",
    "\u4e00\ufffc\u3002",
]

TEXT = [
    "Hello big world",
    "well-known",
    "-5",
    "a -5",
    "978-1-4503-6438-6",
    "ABCD-1234",
    "(-1)",
    "a--b",
    "a-$",
    "x-\u00e9",
    "-\u00e9",
    "a \u2010b",
    "a\u2010b",
    "a\uff08b",
    "64\u201371",
    "a\u2014b",
    "\u201ca\u201d.",
    "https://en.wikipedia.org/wiki/Web_browser",
    "example.com/a-b?q=1",
    "10.1145/3240431.3240443",
    "http://a.b/c?d=e&f=g#h",
    "user@example.com",
    "a.(b",
    '"(b',
    "a(b",
    "a]b}c!d|e",
    '?"x',
    "a?b",
    "foo_bar(baz)",
    "C++ and C#",
    "$12.50, 12.50$, (12)\u00a2",
    "\u20b91,00,000.00",
    "-1/12 and 1-2",
    "50% and 100 %",
    "1e-5 or 10^-3",
    "2024-10-07 10:30",
    "\u20ac(12) \u20ac\u00a10 $\u00a10 0$\u00a10",
    "x\u20ac\u3008\u0660 x\u2030(\u0660 x\u0660\u2030",
    "[1] (a) {x} <y>",
    "\u2014a\u2014 1\u20132 a\u2013b",
    "\u00ab Bonjour \u00bb, dit-il. \u00bbJa\u00ab, sagte er.",
    "\u201eDeutsch\u201c und \u201aeinfach\u2018",
    "\u2018single\u2019 \u201cdouble\u201d \u2039guillemet\u203a",
    "a\u00adb aaaa\u00adbbbb \u3042\u00adb",
    "a\u00a0b a\u202fb a\u2007b",
    "e\u0301x \u0430\u0301\u0430",
]

SCRIPTS = [
    "\u0e01\u0e32\u0e23\u0e1a\u0e49\u0e32\u0e19 \u0e20\u0e32\u0e29\u0e32\u0e44\u0e17\u0e22",
    "\u0e9e\u0eb2\u0eaa\u0eb2\u0ea5\u0eb2\u0ea7",
    "\u1797\u17b6\u179f\u17b6\u1781\u17d2\u1798\u17c2\u179a\u17d4",
    "\u1019\u103c\u1014\u103a\u1019\u102c",
    "\u0645\u0631\u062d\u0628\u0627 "
    "\u0628\u0627\u0644\u0639\u0627\u0644\u0645\u060c \u0661\u0662\u0663.",
    "\u05e9\u05dc\u05d5\u05dd, \u05e2\u05d5\u05dc\u05dd! \u05d0-\u05d1 \u05d0-a \u05d0\u2010a",
    "\u05d0-\u0430 \u05d0-0 \u05d0-\u00e9 a\u05d0-a",
    "\u0915\u094d\u0937\u093f\u0924\u093f \u0928\u092e\u0938\u094d\u0924\u0947\u0964",
    "\uc548\ub155\ud558\uc138\uc694, \uc138\uacc4! \u1100\u1161\u11a8\u1100\u1161",
    "\u3053\u308c\u306f\u65e5\u672c\u8a9e\u306e\u6587\u7ae0\u3067\u3059\u3002",
    "\u300c\u5f15\u7528\u300d\uff08\u62ec\u5f27\uff09\u3001\u3041\u3043\u30a1\u3005\u30fc",
    "\u4e2d\u6587\uff0c\u6807\u70b9\u3002\u201c\u5f15\u53f7\u201d\u4e2d",
    "\u4e00\u201c\u4e00\u201d\u4e00 a\u201c\u4e00 \u4e00\u201da",
    "\u6f22\u5b57 \u3067\u3059 \u5b57\u3001\u5b57 \u6587\u3002",
    "\U0001f468\u200d\U0001f469\u200d\U0001f467 \U0001f44d\U0001f3fd \u261d\U0001f3fb",
    "\U0001f1ed\U0001f1f7\U0001f1eb\U0001f1f7\U0001f1e6 1\ufe0f\u20e3 \u2615\ufe0f",
    "\u1b05\u1b44\u1b13\u1b05 \ua984\ua9c0\ua9a4 \U00011003\U00011013\U00011046\U00011013",
    "\u1bc0\u1bf2\u1bc0 \u25cc\u1b44\u1b05",
    "\u0391\u03b8\u03ae\u03bd\u03b1; \u0410\u0431\u0432\u0433\u0434\u0435.",
]


def case_groups() -> list[CaseGroup]:
    """All case groups."""
    return [
        CaseGroup("spaces and control characters", SPACES_AND_CONTROLS),
        CaseGroup("text", TEXT),
        CaseGroup("scripts", SCRIPTS),
        CaseGroup("hyphen after various characters", _hyphen_contexts()),
        CaseGroup(
            "soft hyphens with hyphens: none",
            [
                "a\u00adb",
                "aaaa\u00adbbbb",
                "\u3042\u00adb",
                "x\u00ad%",
                "x\u00ad\u00a0x",
                "x\u00ad\u00b4",
                "x\u00ad\u0430",
                "x\u00ad\u4e00",
                "x\u00ad(",
                "x \u00adb",
            ],
            (NO_HYPHENS, BREAK_ALL_NO_HYPHENS),
        ),
    ]


def matrix_specs() -> Iterator[tuple[str, str, str, str, Mode]]:
    """(file, chars, left, right, mode) of every matrix."""
    for mode in WORD_BREAK_MODES:
        yield ("latin1.txt", LATIN1, "a", "a", mode)
    yield ("latin1.txt", LATIN1, "a", "a", BREAK_ALL_NO_HYPHENS)
    for mode in WORD_BREAK_MODES:
        for context in ("a", "\u4e00", "0"):
            yield ("classes.txt", SAMPLES, context, context, mode)
    yield ("classes.txt", SAMPLES, "a", "a", NO_HYPHENS)


HEADER = (
    "# Chromium {version} line break opportunities, measured by `swbtools linebreaks`\n"
    "# (tools/swbtools/linebreaks.py). Format: README.md. Do not edit."
)


async def measure_all(out: Path) -> None:
    """Measures everything and writes the data files to `out`."""
    out.mkdir(parents=True, exist_ok=True)
    async with Session() as session:
        header = HEADER.format(version=session.version)
        files: dict[str, list[str]] = {}
        for name, chars, left, right, mode in matrix_specs():
            matrix = await measure_matrix(session, chars, left, right, mode)
            files.setdefault(name, [header]).extend(format_matrix(matrix))
            log.info("%s: matrix %r %r %s", name, left, right, mode.directive())
        lines = [header]
        for group in case_groups():
            lines.append(f"# {group.title}")
            for mode in group.modes:
                lines.append(f"@mode {mode.directive()}")
                results = await session.measure(group.texts, mode)
                lines += [format_case(t, b) for t, b in zip(group.texts, results, strict=True)]
        files["cases.txt"] = lines
    for name, content in files.items():
        (out / name).write_text("\n".join(content) + "\n", encoding="utf-8")
        print(f"wrote {out / name}")


def data_dir() -> Path:
    """Returns `crates/text/tests/linebreak/`."""
    return paths.repo_root() / "crates" / "text" / "tests" / "linebreak"


def linebreaks(out: Path | None = None) -> int:
    """`swbtools linebreaks`: writes the data files. Returns the exit code."""
    asyncio.run(measure_all(out or data_dir()))
    return 0
