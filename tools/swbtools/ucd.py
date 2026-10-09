"""Reading the Unicode Character Database (UCD) for the table generators.

The generators (`linebreak_tables.py`, `js_unicode_tables.py`) download
the UCD files that they need from unicode.org into `out/ucd/VERSION/`,
unless they are there already, and turn properties into code point
ranges for generated Rust tables.
"""

import logging
import re
import urllib.request
from collections.abc import Iterator
from pathlib import Path

from swbtools import paths

log = logging.getLogger(__name__)

MAX_CODE_POINT = 0x10FFFF


def ucd_file(relative: str, version: str) -> Path:
    """Returns the local copy of a UCD file; downloads it if missing.

    `relative` is the path below `https://www.unicode.org/Public/VERSION/`,
    for example `ucd/DerivedCoreProperties.txt`.
    """
    path = paths.out_dir() / "ucd" / version / Path(relative).name
    if not path.is_file():
        url = f"https://www.unicode.org/Public/{version}/{relative}"
        log.info("downloading %s", url)
        path.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(url, timeout=60) as response:
            path.write_bytes(response.read())
    return path


def _data_lines(path: Path) -> Iterator[tuple[int, int, list[str]]]:
    """The data lines of a UCD file: first and last code point, and the
    fields after the code points. Comments and empty lines are skipped."""
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [field.strip() for field in line.split(";")]
        first, last = _code_points(fields[0])
        yield first, last, fields[1:]


def _code_points(field: str) -> tuple[int, int]:
    """The first and last code point of a field such as `0041..005A` or `0030`."""
    first_text, _, last_text = field.partition("..")
    first = int(first_text, 16)
    return first, int(last_text, 16) if last_text else first


def read_property(path: Path, default: str) -> list[str]:
    """The value of a property for every code point: the `@missing` lines
    first, then the data lines."""
    values = [default] * (MAX_CODE_POINT + 1)
    for raw in path.read_text(encoding="utf-8").splitlines():
        missing = re.match(r"#\s*@missing:\s*([0-9A-F]+)\.\.([0-9A-F]+);\s*(\w+)", raw)
        if missing:
            first, last = int(missing.group(1), 16), int(missing.group(2), 16)
            values[first : last + 1] = [missing.group(3)] * (last - first + 1)
    for first, last, fields in _data_lines(path):
        values[first : last + 1] = [fields[0]] * (last - first + 1)
    return values


def has_property(path: Path, name: str) -> list[bool]:
    """For every code point, whether a binary property file (such as
    emoji-data.txt, which lists several properties) gives it `name`."""
    values = [False] * (MAX_CODE_POINT + 1)
    for first, last, fields in _data_lines(path):
        if fields[0] == name:
            values[first : last + 1] = [True] * (last - first + 1)
    return values


def ranges(selected: list[bool]) -> list[tuple[int, int]]:
    """The ranges of code points where `selected` is true."""
    out: list[tuple[int, int]] = []
    for cp, on in enumerate(selected):
        if not on:
            continue
        if out and out[-1][1] == cp - 1:
            out[-1] = (out[-1][0], cp)
        else:
            out.append((cp, cp))
    return out


def value_ranges(values: list[str | None]) -> list[tuple[int, int, str]]:
    """The ranges of equal values, without the code points whose value is None."""
    out: list[tuple[int, int, str]] = []
    for cp, value in enumerate(values):
        if value is None:
            continue
        if out and out[-1][1] == cp - 1 and out[-1][2] == value:
            out[-1] = (out[-1][0], cp, value)
        else:
            out.append((cp, cp, value))
    return out


def range_table(name: str, doc: str, items: list[tuple[int, int]]) -> list[str]:
    """The Rust source of one table of code point ranges."""
    lines = [f"/// {doc}", f"pub(super) static {name}: [(u32, u32); {len(items)}] = ["]
    lines += [f"    (0x{a:04X}, 0x{b:04X})," for a, b in items]
    lines += ["];", ""]
    return lines
