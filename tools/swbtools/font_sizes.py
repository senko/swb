"""`swbtools measure font-size-sweep`: the font sizes that Chromium shapes
text with.

Chromium does not shape text at the CSS font size. swb's
`chromium_sizes` (`crates/text/src/shape.rs`) models two conversions, and
this measurement checks them for every size from 9.000 to 24.000 px in
steps of 0.001 px:

- *kerning size*: the CSS size truncated to 1/100 px, in `f32` arithmetic
  (16.21 px becomes 16.2 px, because `16.21f32 * 100` is just below 1621).
  The shaper scales kerning with this size in 16.16 fixed point, truncated
  (`trunc(size * 65536)`, the `HarfBuzz` font scale);
- *advance size*: the kerning size truncated to 1/64 px. The nominal
  advances of the glyphs are those of this size.

Method. Each size is measured alone, on a new page in a new browser
context: Chromium keeps fonts in a cache that all pages of a renderer
process share (see "Shared fonts"), and each context has its own process.
The page has four `<span>` elements, each with its own family: 20 times
`0` in three families, and 100 times `AV` in Liberation Sans. The width of
a span is the sum of the glyph advances, each in 16.16 fixed point, rounded
up to 1/64 px (the width of a text item in layout). The tool reads the
units that the widths are made of from the same fonts at 2048 px, where
one pixel is one font unit. Then:

- the advance size is the one size in 1/64 px with which the digits in all
  three families have the measured widths;
- the kerning size is the one size in 1/100 px with which the kerning of
  `AV` and `VA` (added to the advance of the first glyph) gives the
  measured width of the pairs, with that advance size.

The results are two files in `crates/text/tests/`:

- `chromium-font-sizes.txt`: the two sizes of every CSS size, as ranges.
  The Rust test `font_size_rule_matches_chromium` in `shape.rs` checks
  `chromium_sizes` against it.
- `chromium-font-widths.txt`: the four widths of every 13th size.
  `tests/font_widths.rs` shapes the same text with swb.

Shared fonts. A page with several sizes can show other sizes than the
ones above. Chromium gets fonts from a cache whose key is the size
truncated to 1/100 px *twice* (`key(s) = trunc(100 * (trunc(100 * s) /
100))`, in `f32` arithmetic). The first size that asks for a key creates
the font, and every later size with the same key gets that font. The two
truncations give another key than one truncation only for the hundredths
`c / 100` whose `f32` value is so far below `c / 100` that `f32(c / 100)
* 100` rounds below `c`: sizes from `c / 100` to `c / 100 + 0.009` get the
font of the sizes one hundredth below (for example, 9.111 to 9.119 px get
the font of 9.110 px, and 16.211 to 16.220 px that of 16.2 px). A scan of
all sizes in one page (ascending, as the `font-size` of 15,001 `<div>`
elements) shows this: 684 sizes get the sizes of another size, and the
model predicts all 15,001 results. swb does not model this: it shapes
each size alone. In a real page, two sizes of one family and style that
differ by less than 0.01 px are rare. The measurement runs the page scan
and prints how many sizes differ from the isolated ones and from the
model.
"""

import asyncio
import functools
import math
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from playwright.async_api import Browser, Page

from swbtools import browser, paths

FIRST = 9000
"""The first CSS font size of the sweep, in thousandths of a pixel."""

LAST = 24000
"""The last CSS font size of the sweep, in thousandths of a pixel."""

WIDTH_STEP = 13
"""Every `WIDTH_STEP`th size of the sweep gets its widths in the data."""

CONCURRENCY = 12
"""Browser contexts that measure at the same time."""

SIZES_FILE = "crates/text/tests/chromium-font-sizes.txt"
"""The sizes, relative to the repository root."""

WIDTHS_FILE = "crates/text/tests/chromium-font-widths.txt"
"""The sampled widths, relative to the repository root."""

ZERO_FAMILIES: tuple[str, ...] = ("Liberation Mono", "Liberation Serif", "DejaVu Sans")
"""The families in which the width of `0` gives the advance size."""

PAIR_FAMILY = "Liberation Sans"
"""The family in which the width of `AV` pairs gives the kerning size."""

ZEROS = 20
"""The number of `0` in each span."""

PAIRS = 100
"""The number of `AV` in the span for the kerning size."""

CALIBRATION_SIZE = 2048
"""The font size in px at which one px is one font unit (`unitsPerEm`)."""

UNITS_PER_EM = 2048
"""`unitsPerEm` of all measured fonts."""

_WIDTHS_JS = (
    "() => [...document.querySelectorAll('span')].map(s => s.getBoundingClientRect().width)"
)

_KERNING_SEARCH = range(500, 3001)
"""The kerning sizes in 1/100 px that the tool tries."""

_ADVANCE_SEARCH = range(8 * 64, 26 * 64)
"""The advance sizes in 1/64 px that the tool tries."""


@dataclass(frozen=True)
class Units:
    """Advances and kerning in font units, read at `CALIBRATION_SIZE`."""

    zero: tuple[int, ...]
    """The advance of `0` in each of `ZERO_FAMILIES`."""
    a: int
    v: int
    """The advances of `A` and `V` in `PAIR_FAMILY`."""
    kern_av: int
    kern_va: int
    """The kerning between `A` and `V`, and `V` and `A`."""


@dataclass(frozen=True)
class Sizes:
    """The sizes that Chromium used for one CSS size."""

    kerning: int
    """The kerning size in 1/100 px."""
    advance: int
    """The advance size in 1/64 px."""


def _span(family: str, text: str) -> str:
    return f"<span style=\"font-family: '{family}'\">{text}</span>"


def _css_size(milli: int) -> str:
    return f"{milli // 1000}.{milli % 1000:03d}px"


def _spans() -> str:
    """The spans that one size measures."""
    spans = [_span(family, "0" * ZEROS) for family in ZERO_FAMILIES]
    spans.append(_span(PAIR_FAMILY, "AV" * PAIRS))
    return "".join(spans)


def sweep_page(milli: int) -> str:
    """The page that measures the CSS size `milli` (thousandths of a px)."""
    return (
        '<!DOCTYPE html><div style="white-space: pre; '
        f'font-size: {_css_size(milli)}">{_spans()}</div>'
    )


def scan_page(sizes: Sequence[int]) -> str:
    """One page with a `<div>` for each CSS size."""
    rows = [f'<div style="font-size: {_css_size(m)}">{_spans()}</div>' for m in sizes]
    return "<!DOCTYPE html><style>div { white-space: pre }</style>" + "".join(rows)


# --- Widths ----------------------------------------------------------------
#
# Lengths in "16.16" are integers in 1/65536 px.


def _round_half_away(numerator: np.ndarray | int, denominator: int) -> np.ndarray:
    """`numerator / denominator` rounded to the nearest integer, halves away
    from zero. `denominator` is positive."""
    n = np.asarray(numerator, dtype=np.int64)
    magnitude = (2 * np.abs(n) + denominator) // (2 * denominator)
    return np.where(n >= 0, magnitude, -magnitude)


def advance16(units: int, advance_64th: int) -> int:
    """The advance of a glyph of `units` font units at the advance size
    `advance_64th` (1/64 px), in 16.16."""
    return int(_round_half_away(units * advance_64th * 65536, 64 * UNITS_PER_EM))


def scale16(kerning_100th: np.ndarray | int) -> np.ndarray:
    """The shaper's font scale for the kerning sizes `kerning_100th`: the
    size as `f32`, times 65536, truncated."""
    size = np.asarray(kerning_100th, dtype=np.float32) / np.float32(100)
    return np.trunc(size * np.float32(65536)).astype(np.int64)


def kerning16(units: int, kerning_100th: np.ndarray | int) -> np.ndarray:
    """The kerning of `units` font units at the kerning sizes
    `kerning_100th` (1/100 px), in 16.16."""
    return _round_half_away(units * scale16(kerning_100th), UNITS_PER_EM)


def sum_width(advances16: Sequence[np.ndarray | int]) -> np.ndarray:
    """The width of glyphs with the advances `advances16` (16.16, integers
    or arrays), in 1/64 px: the exact sum as `f32` (the width of a text
    item in Chromium is a `float`), rounded up to 1/64 px. (Adding the
    advances in `f32`, glyph after glyph, fits the measured widths
    worse.)"""
    total = sum(np.asarray(advance, dtype=np.int64) for advance in advances16)
    width = (np.asarray(total, dtype=np.float64) / 65536).astype(np.float32)
    return np.ceil(width.astype(np.float64) * 64).astype(np.int64)


def zeros_width(units: int, advance_64th: int) -> int:
    """The width of `ZEROS` glyphs of `units` font units, in 1/64 px."""
    return int(sum_width([advance16(units, advance_64th)] * ZEROS))


def pairs_widths(units: Units, advance_64th: int, kerning_100th: np.ndarray | int) -> np.ndarray:
    """The width of `PAIRS` times `AV` for each kerning size, in 1/64 px.
    The kerning is added to the advance of the glyph before it."""
    a, v = advance16(units.a, advance_64th), advance16(units.v, advance_64th)
    after_a = a + kerning16(units.kern_av, kerning_100th)
    after_v = v + kerning16(units.kern_va, kerning_100th)
    advances = [after_a, after_v] * (PAIRS - 1) + [after_a, v]
    return sum_width(advances)


def _to_64th(width: float) -> int:
    """A measured width in 1/64 px. Chromium's widths are multiples of it."""
    scaled = width * 64
    if abs(scaled - round(scaled)) > 1e-6:
        raise ValueError(f"width {width} is not a multiple of 1/64 px")
    return round(scaled)


@functools.cache
def _advance_table(units: Units) -> dict[tuple[int, ...], list[int]]:
    """For the digit widths (1/64 px, one for each of `ZERO_FAMILIES`), the
    advance sizes (1/64 px) that give them."""
    table: dict[tuple[int, ...], list[int]] = {}
    for size in _ADVANCE_SEARCH:
        widths = tuple(zeros_width(u, size) for u in units.zero)
        table.setdefault(widths, []).append(size)
    return table


@functools.cache
def _pair_table(units: Units, advance_64th: int) -> dict[int, list[int]]:
    """For a pair width (1/64 px), the kerning sizes (1/100 px) that give it
    with the advance size `advance_64th`."""
    sizes = np.array(_KERNING_SEARCH)
    table: dict[int, list[int]] = {}
    widths = pairs_widths(units, advance_64th, sizes).tolist()
    for size, width in zip(sizes.tolist(), widths, strict=True):
        table.setdefault(width, []).append(size)
    return table


def solve_advance(widths: Sequence[float], units: Units) -> int | None:
    """The advance size in 1/64 px with which the digit widths `widths` (one
    for each of `ZERO_FAMILIES`) were made, or None if there is none or
    more than one."""
    found = _advance_table(units).get(tuple(_to_64th(w) for w in widths), [])
    return found[0] if len(found) == 1 else None


def solve_kerning(width: float, units: Units, advance_64th: int) -> int | None:
    """The kerning size in 1/100 px with which the pair width `width` was
    made, given the advance size, or None if there is none or more than
    one."""
    found = _pair_table(units, advance_64th).get(_to_64th(width), [])
    return found[0] if len(found) == 1 else None


def solve(widths: Sequence[float], units: Units) -> Sizes | None:
    """The sizes behind the four widths of one page (the digits in
    `ZERO_FAMILIES`, then the pairs), or None if they are not unique."""
    advance = solve_advance(widths[: len(ZERO_FAMILIES)], units)
    if advance is None:
        return None
    kerning = solve_kerning(widths[len(ZERO_FAMILIES)], units, advance)
    return None if kerning is None else Sizes(kerning, advance)


# --- The model of swb and of the shared font cache -------------------------


def truncate_centi(size: float) -> int:
    """`size` in px (as `f32`) truncated to 1/100 px, in 1/100 px, in `f32`
    arithmetic (`(size * 100).trunc()`)."""
    return int(np.trunc(np.float32(size) * np.float32(100)))


def css_to_f32(milli: int) -> float:
    """The CSS size `milli` (thousandths of a px) as `f32`, in `float`."""
    return float(np.float32(milli / 1000))


def cache_key(size: float) -> int:
    """The key of Chromium's font cache for the `f32` size `size`, in 1/100
    px: the size truncated to 1/100 px twice (see the module
    documentation)."""
    once = np.float32(truncate_centi(size)) / np.float32(100)
    return truncate_centi(float(once))


def model_sizes(milli: int) -> Sizes:
    """The sizes of swb's rule for the CSS size `milli`: the kerning size,
    and the advance size from it."""
    centi = truncate_centi(css_to_f32(milli))
    kerning = np.float32(centi) / np.float32(100)
    return Sizes(centi, math.trunc(float(kerning * np.float32(64))))


def shared_sizes(milli_sizes: Sequence[int], isolated: dict[int, Sizes]) -> dict[int, Sizes]:
    """The sizes that a page with the CSS sizes `milli_sizes` (in this
    order) gets in the model of the shared font cache: the first size of
    each key decides."""
    first: dict[int, int] = {}
    out: dict[int, Sizes] = {}
    for milli in milli_sizes:
        out[milli] = isolated[first.setdefault(cache_key(css_to_f32(milli)), milli)]
    return out


# --- Data files ------------------------------------------------------------


@dataclass(frozen=True)
class SizeRange:
    """A range of CSS sizes (thousandths of a px, both ends included) with
    the same sizes."""

    first: int
    last: int
    sizes: Sizes


def size_ranges(by_size: dict[int, Sizes]) -> list[SizeRange]:
    """Joins consecutive CSS sizes with equal sizes into ranges."""
    ranges: list[SizeRange] = []
    for milli in sorted(by_size):
        sizes = by_size[milli]
        last = ranges[-1] if ranges else None
        if last and last.last + 1 == milli and last.sizes == sizes:
            ranges[-1] = SizeRange(last.first, milli, sizes)
        else:
            ranges.append(SizeRange(milli, milli, sizes))
    return ranges


def format_sizes(ranges: Sequence[SizeRange], version: str) -> str:
    """The text of the sizes file."""
    lines = [
        f"# Font sizes of Chromium {version}, measured by `swbtools measure font-size-sweep`.",
        "# Do not edit. The Rust test `font_size_rule_matches_chromium` in",
        "# `crates/text/src/shape.rs` reads this file.",
        "#",
        f"# Each CSS font size from {FIRST / 1000:.3f} to {LAST / 1000:.3f} px in steps of",
        "# 0.001 px was measured alone on a new page. A line has four numbers:",
        "#",
        "#   FIRST LAST KERNING ADVANCE",
        "#",
        "# FIRST and LAST: the first and the last CSS size of the range, in",
        "# thousandths of a px. KERNING: the size that kerning used, in",
        "# hundredths of a px. ADVANCE: the size that the nominal advances",
        "# used, in 1/64 px.",
    ]
    lines += [f"{r.first} {r.last} {r.sizes.kerning} {r.sizes.advance}" for r in ranges]
    return "\n".join(lines) + "\n"


def format_widths(widths: dict[int, list[float]], version: str) -> str:
    """The text of the widths file: the widths of every `WIDTH_STEP`th size."""
    lines = [
        f"# Text widths in Chromium {version}, measured by `swbtools measure font-size-sweep`.",
        "# Do not edit. `crates/text/tests/font_widths.rs` reads this file.",
        "#",
        f"# Every {WIDTH_STEP}th CSS font size from {FIRST / 1000:.3f} px, in steps of 0.001 px,",
        "# alone on a new page, `white-space: pre`. A line has five numbers:",
        "#",
        "#   SIZE MONO SERIF DEJAVU PAIRS",
        "#",
        "# SIZE: the CSS size in thousandths of a px. The other numbers are the",
        f"# widths of a span in 1/64 px: {ZEROS} times `0` in {', '.join(ZERO_FAMILIES)},",
        f"# and {PAIRS} times `AV` in {PAIR_FAMILY}.",
    ]
    for milli in sorted(widths):
        if (milli - FIRST) % WIDTH_STEP == 0:
            lines.append(f"{milli} " + " ".join(str(_to_64th(w)) for w in widths[milli]))
    return "\n".join(lines) + "\n"


def parse_sizes(text: str) -> dict[int, Sizes]:
    """The sizes in the sizes file, by CSS size."""
    by_size: dict[int, Sizes] = {}
    for line in text.splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        first, last, kerning, advance = (int(field) for field in line.split())
        for milli in range(first, last + 1):
            by_size[milli] = Sizes(kerning, advance)
    return by_size


# --- Chromium --------------------------------------------------------------


async def _fresh_widths(chromium: Browser, directory: Path, name: str, html: str) -> list[float]:
    """The widths of the `<span>` elements of `html` on a new page in a new
    browser context (its own renderer process, so its own font cache)."""
    context = await browser.new_context(chromium, browser.LAYOUT_VIEWPORT)
    try:
        page = await context.new_page()
        await browser.load_html(page, directory / f"{name}.html", html)
        return await page.evaluate(_WIDTHS_JS)
    finally:
        await context.close()


async def read_units(chromium: Browser, directory: Path) -> Units:
    """Reads the advances and the kerning in font units at 2048 px."""
    texts = ["0", "0", "0", "A", "V", "AV", "VA"]
    families = [*ZERO_FAMILIES, *([PAIR_FAMILY] * 4)]
    # Spaces between the spans: glyphs of one font kern across the boundary
    # of two elements.
    spans = " ".join(_span(f, t) for f, t in zip(families, texts, strict=True))
    html = (
        f'<!DOCTYPE html><div style="white-space: pre; font-size: {CALIBRATION_SIZE}px">'
        f"{spans}</div>"
    )
    widths = await _fresh_widths(chromium, directory, "units", html)
    if any(w != round(w) for w in widths):
        raise ValueError(f"the widths at {CALIBRATION_SIZE} px are not whole: {widths}")
    zero, (a, v, av, va) = tuple(round(w) for w in widths[:3]), [round(w) for w in widths[3:]]
    return Units(zero=zero, a=a, v=v, kern_av=av - a - v, kern_va=va - a - v)


def sweep_sizes() -> list[int]:
    """All CSS sizes of the sweep, in thousandths of a px."""
    return list(range(FIRST, LAST + 1))


async def measure_isolated(
    chromium: Browser, directory: Path, milli_sizes: Sequence[int]
) -> dict[int, list[float]]:
    """The widths of the CSS sizes `milli_sizes`, each measured alone."""
    semaphore = asyncio.Semaphore(CONCURRENCY)

    async def one(milli: int) -> list[float]:
        async with semaphore:
            return await _fresh_widths(chromium, directory, f"s{milli}", sweep_page(milli))

    widths = await asyncio.gather(*(one(m) for m in milli_sizes))
    return dict(zip(milli_sizes, widths, strict=True))


async def measure_scan(
    page: Page, directory: Path, milli_sizes: Sequence[int]
) -> dict[int, list[float]]:
    """The widths of the CSS sizes `milli_sizes` in one page, in this
    order."""
    await browser.load_html(page, directory / "scan.html", scan_page(milli_sizes))
    flat = await page.evaluate(_WIDTHS_JS)
    per_row = len(ZERO_FAMILIES) + 1
    return {m: flat[i * per_row : (i + 1) * per_row] for i, m in enumerate(milli_sizes)}


def solve_all(widths: dict[int, list[float]], units: Units) -> tuple[dict[int, Sizes], list[int]]:
    """The sizes behind the widths of each CSS size, and the CSS sizes whose
    widths no sizes explain."""
    by_size: dict[int, Sizes] = {}
    unexplained: list[int] = []
    for milli, row in widths.items():
        sizes = solve(row, units)
        if sizes is None:
            unexplained.append(milli)
        else:
            by_size[milli] = sizes
    return by_size, unexplained


def compare_with_model(isolated: dict[int, Sizes], scan: dict[int, Sizes]) -> tuple[int, int, int]:
    """Compares the sizes of the page scan with the sizes of the same CSS
    sizes alone. Returns the number of CSS sizes in the scan, how many of
    them have other sizes, and how many differ from the model of the shared
    font cache."""
    common = sorted(set(isolated) & set(scan))
    predicted = shared_sizes(common, isolated)
    differ = sum(scan[m] != isolated[m] for m in common)
    mismatch = sum(scan[m] != predicted[m] for m in common)
    return len(common), differ, mismatch


def swb_differences(by_size: dict[int, Sizes]) -> list[int]:
    """The CSS sizes where `model_sizes` (swb's rule) differs from the
    measurement."""
    return [m for m, sizes in sorted(by_size.items()) if model_sizes(m) != sizes]


async def font_size_sweep(page: Page, directory: Path) -> None:
    """Measures the font sizes, writes the data files and prints a summary."""
    chromium = page.context.browser
    if chromium is None:
        raise RuntimeError("the page has no browser")
    units = await read_units(chromium, directory)
    print(f"Chromium {chromium.version}; font units: {units}")
    widths = await measure_isolated(chromium, directory, sweep_sizes())
    isolated, unexplained = solve_all(widths, units)
    print(f"{len(isolated)} sizes measured alone; {len(unexplained)} not explained")
    if unexplained:
        print("  first:", unexplained[:10])
    ranges = size_ranges(isolated)
    root = paths.repo_root()
    (root / SIZES_FILE).write_text(format_sizes(ranges, chromium.version), encoding="utf-8")
    (root / WIDTHS_FILE).write_text(format_widths(widths, chromium.version), encoding="utf-8")
    print(f"wrote {len(ranges)} ranges to {SIZES_FILE} and the widths to {WIDTHS_FILE}")
    differences = swb_differences(isolated)
    print(f"sizes where swb's rule differs from Chromium: {len(differences)}")
    if differences:
        print("  first:", differences[:10])
    scan_widths = await measure_scan(page, directory, sweep_sizes())
    scan, scan_unexplained = solve_all(scan_widths, units)
    count, differ, mismatch = compare_with_model(isolated, scan)
    print(
        f"one page with {count} sizes: {differ} sizes get the sizes of another size; "
        f"{mismatch} differ from the model of the shared font cache "
        f"({len(scan_unexplained)} not explained)"
    )
