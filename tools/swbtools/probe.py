"""`swbtools probe`: ask Chromium (and optionally swb) about small HTML cases.

Use it during development instead of one-off scripts. `measure` regenerates
checked data; `probe` answers a question about one layout.

A case file is JSON (committed ones are in `tools/probes/`):

```json
{"viewport": [800, 600],
 "cases": [
   {"name": "bfc-next-to-float",
    "html": "<!DOCTYPE html><style>...</style><div id=a>...</div>",
    "viewport": [800, 600],
    "measure": [
      {"boxes": "#a, #a > p"},
      {"rects": "span.x"},
      {"style": "#a", "props": ["width", "line-height"]},
      {"js": "document.scrollingElement.scrollHeight"},
      {"ink": "#a"}
    ],
    "files": {"x.ttf": "fixtures/fonts/DejaVuSans.ttf"}}]}
```

- `viewport` (file and case, optional; default 800x600): the layout viewport.
- `html`: the whole document, used verbatim. Without a doctype the page is in
  quirks mode, on purpose.
- `boxes`: for each element that matches the CSS selector, the border box
  in document coordinates, as the box dump computes it (union of the
  `getClientRects()`, rounded to 2 decimals).
- `rects`: each rectangle of `getClientRects()` of each matching element
  (the line fragments of an inline element), document coordinates.
- `style`: `getComputedStyle` values of the properties in `props`.
- `js`: the JSON value of an expression.
- `ink`: for each matching element, what Chromium paints in its border box
  (a screenshot of the area): `ink`, the sum of the darkness of all
  pixels (0 for white, 1 for black, by luminance), and `bbox`, the
  bounding box of the pixels that are not white, relative to the box. It
  shows synthetic bold (more ink) and slanted glyphs (a wider box) where
  the geometry does not change.
- `files` (case, optional): files to put next to the page, as
  `{"published name": "source path"}`; a relative source path is relative
  to the repository root. Use it for fonts (`@font-face`) and images.

Chromium has the settings of the other tools (`browser.py`: bundled fonts,
JavaScript disabled, scale 1). The page is a file in a temporary directory,
loaded by navigation (see `measure.py`).

Output: one line per value, `CASE  KIND  LABEL  VALUES`. A label is the
selector and the 1-based index among its matches (`#a > p[2]`; a selector
list goes in parentheses: `(#a, #b)[2]`); `rects` adds `#N`, the
fragment. With `--with-swb`, swb renders the same page; each `boxes` element is
looked up by its index in `document.querySelectorAll('*')`, and a line
`CASE  swb  LABEL  VALUES  dx=.. dy=.. dw=.. dh=..` follows the Chromium
line, with `!` when a delta is above the tolerance. For `ink`, swb also
writes a full-page screenshot, and the same area of it (Chromium's border
box of the element) gives swb's ink and bounding box: a line
`CASE  swb  LABEL  ink=.. bbox=..  dink=..` with `!` when the ink differs
by more than `INK_TOLERANCE` (relative, at least 2) or an edge of the
bounding box by more than the tolerance. `rects`, `style` and `js` are
Chromium only.
"""

import asyncio
import io
import json
import logging
import shutil
from collections.abc import Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from PIL import Image
from playwright.async_api import Page

from swbtools import browser, paths, swb
from swbtools.boxes import BoxDump, Rect, read_dump, round2, union

log = logging.getLogger(__name__)

DEFAULT_VIEWPORT = browser.LAYOUT_VIEWPORT
DEFAULT_TOLERANCE = 1.0
"""Default maximum difference in px between swb and Chromium."""
INK_TOLERANCE = 0.05
"""The largest relative difference of swb's ink from Chromium's (at least 2
units), for anti-aliasing differences."""

QUERY_KINDS = ("boxes", "rects", "style", "js", "ink")
_SWB_LABEL = "swb"


class ProbeError(Exception):
    """A bad case file or an unusable request. The message says what to fix."""


# --- Case files ------------------------------------------------------------


@dataclass(frozen=True)
class Query:
    """One measurement of a case."""

    kind: str
    """One of `QUERY_KINDS`."""
    target: str
    """The CSS selector (`boxes`, `rects`, `style`, `ink`) or the expression
    (`js`)."""
    props: tuple[str, ...] = ()
    """The properties of a `style` query."""


@dataclass(frozen=True)
class Case:
    """One HTML document and what to measure in it."""

    name: str
    html: str
    viewport: tuple[int, int]
    queries: tuple[Query, ...]
    files: tuple[tuple[str, Path], ...] = ()
    """Files to copy next to the page: (published name, source path)."""


def _parse_viewport(value: Any, where: str) -> tuple[int, int]:
    ok = (
        isinstance(value, list)
        and len(value) == 2
        and all(isinstance(v, int) and not isinstance(v, bool) and v > 0 for v in value)
    )
    if not ok:
        raise ProbeError(f"{where}: viewport must be [width, height] (positive integers)")
    return (value[0], value[1])


def _parse_query(item: Any, where: str) -> Query:
    if not isinstance(item, dict):
        raise ProbeError(f"{where}: a measure entry must be an object")
    kinds = [key for key in item if key in QUERY_KINDS]
    if len(kinds) != 1:
        found = ", ".join(item) or "none"
        raise ProbeError(
            f"{where}: a measure entry needs exactly one query kind"
            f" ({', '.join(QUERY_KINDS)}); its keys: {found}"
        )
    kind = kinds[0]
    extra = set(item) - {kind, "props"}
    if extra:
        raise ProbeError(f"{where}: unknown key {sorted(extra)[0]!r} in a {kind} entry")
    target = item[kind]
    if not isinstance(target, str) or not target.strip():
        raise ProbeError(f"{where}: {kind} must be a non-empty string")
    if kind != "style":
        if "props" in item:
            raise ProbeError(f"{where}: props belongs to style, not {kind}")
        return Query(kind, target)
    props = item.get("props")
    if not isinstance(props, list) or not props or not all(isinstance(p, str) for p in props):
        raise ProbeError(f"{where}: style needs props, a non-empty list of property names")
    return Query(kind, target, tuple(props))


def _parse_case(item: Any, number: int, default_viewport: tuple[int, int]) -> Case:
    if not isinstance(item, dict):
        raise ProbeError(f"case {number}: must be an object")
    name = item.get("name")
    if not isinstance(name, str) or not name.strip():
        raise ProbeError(f"case {number}: missing name")
    where = f"case {name!r}"
    html = item.get("html")
    if not isinstance(html, str):
        raise ProbeError(f"{where}: html must be a string")
    viewport = _parse_viewport(item["viewport"], where) if "viewport" in item else default_viewport
    measure = item.get("measure")
    if not isinstance(measure, list) or not measure:
        raise ProbeError(f"{where}: measure must be a non-empty list")
    queries = tuple(_parse_query(q, f"{where}, measure {i}") for i, q in enumerate(measure, 1))
    return Case(name, html, viewport, queries, _parse_files(item.get("files", {}), where))


def _parse_files(value: Any, where: str) -> tuple[tuple[str, Path], ...]:
    """The `files` of a case. Published names are plain file names."""
    if not isinstance(value, dict):
        raise ProbeError(f"{where}: files must be an object of name to source path")
    files = []
    for name, source in value.items():
        if not name or "/" in name or "\\" in name or name in (".", ".."):
            raise ProbeError(f"{where}: file name {name!r} must be a plain file name")
        if not isinstance(source, str) or not source:
            raise ProbeError(f"{where}: the source of {name!r} must be a path")
        path = Path(source)
        files.append((name, path if path.is_absolute() else paths.repo_root() / path))
    return tuple(files)


def parse_cases(text: str, origin: str = "case file") -> list[Case]:
    """Parses and validates the text of a case file."""
    try:
        data = json.loads(text)
    except json.JSONDecodeError as error:
        raise ProbeError(f"{origin}: not valid JSON: {error}") from error
    try:
        return _parse_data(data)
    except ProbeError as error:
        raise ProbeError(f"{origin}: {error}") from error


def _parse_data(data: Any) -> list[Case]:
    if not isinstance(data, dict) or not isinstance(data.get("cases"), list):
        raise ProbeError('the top level must be an object with a "cases" list')
    viewport = _parse_viewport(data["viewport"], "file") if "viewport" in data else DEFAULT_VIEWPORT
    cases = [_parse_case(item, i, viewport) for i, item in enumerate(data["cases"], 1)]
    seen: set[str] = set()
    for case in cases:
        if case.name in seen:
            raise ProbeError(f"duplicate case name {case.name!r}")
        seen.add(case.name)
    return cases


def read_cases(path: Path) -> list[Case]:
    """Reads a case file."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise ProbeError(f"{path}: cannot read: {error.strerror or error}") from error
    return parse_cases(text, str(path))


def select_cases(cases: Sequence[Case], names: Sequence[str]) -> list[Case]:
    """Returns the cases with the given names (all if `names` is empty), in
    file order. Raises `ProbeError` for a name that no case has."""
    known = {case.name for case in cases}
    unknown = [name for name in names if name not in known]
    if unknown:
        raise ProbeError(f"unknown case {unknown[0]!r} (known: {', '.join(sorted(known))})")
    return [case for case in cases if not names or case.name in names]


# --- Results ---------------------------------------------------------------


@dataclass
class SwbBox:
    """swb's box for an element that Chromium reports, and the difference."""

    status: str
    """`ok`, `tag-mismatch` (the tag at that index differs), `missing` (no
    such index) or `box-mismatch` (only one of the two has a box)."""
    tag: str | None = None
    """swb's tag at the index, for `tag-mismatch`."""
    rect: Rect | None = None
    delta: Rect | None = None
    """swb minus Chromium."""
    outside: bool = False
    """True if the status is not `ok` or a delta is above the tolerance."""


@dataclass
class SwbInk:
    """swb's ink in the area of an `ink` row, and whether it differs."""

    ink: float
    bbox: tuple[int, int, int, int] | None
    outside: bool


@dataclass
class Row:
    """One value of a case."""

    kind: str
    label: str
    """Selector with index, or the expression."""
    data: dict[str, Any] = field(default_factory=dict)
    swb: SwbBox | None = None
    swb_ink: SwbInk | None = None


@dataclass
class CaseResult:
    """All rows of a case."""

    name: str
    viewport: tuple[int, int]
    rows: list[Row] = field(default_factory=list)
    swb_error: str | None = None
    """Why swb has no dump for this case, if `--with-swb` was given."""


def _num(value: float) -> str:
    return f"{round2(value):g}"


def format_rect(rect: Rect | None) -> str:
    """`x=8 y=8 w=300 h=18`, or `no box`."""
    if rect is None:
        return "no box"
    x, y, w, h = rect
    return f"x={_num(x)} y={_num(y)} w={_num(w)} h={_num(h)}"


def format_delta(delta: Rect) -> str:
    """`dx=0 dy=0 dw=0 dh=0`."""
    return " ".join(f"d{n}={_num(v)}" for n, v in zip("xywh", delta, strict=True))


def _values(row: Row) -> str:
    data = row.data
    if "error" in data:
        return f"error: {data['error']}"
    if row.kind in ("boxes", "rects"):
        return format_rect(data["rect"]) if data.get("matched", True) else "no match"
    if row.kind == "style":
        return " ".join(f"{prop}={value}" for prop, value in data["style"].items())
    if row.kind == "ink":
        if not data.get("matched", True):
            return "no match"
        return format_ink(data["ink"], data["bbox"])
    return json.dumps(data["value"], ensure_ascii=False)


def format_ink(ink: float, bbox: tuple[int, int, int, int] | None) -> str:
    """`ink=12.5 bbox=0,0,10,10`."""
    shown = "none" if bbox is None else ",".join(str(v) for v in bbox)
    return f"ink={ink:g} bbox={shown}"


def format_row(case: str, row: Row) -> list[str]:
    """The output lines of a row: the Chromium line, and the swb line for a
    compared box."""
    lines = [f"{case}  {row.kind:<5}  {row.label}  {_values(row)}"]
    swb_box = row.swb
    if swb_box is not None:
        prefix = f"{case}  {_SWB_LABEL:<5}  {row.label}  "
        if swb_box.status == "tag-mismatch":
            lines.append(
                f"{prefix}tag differs at index {row.data['index']}: swb has <{swb_box.tag}>"
            )
        elif swb_box.status == "missing":
            lines.append(f"{prefix}swb has no element at index {row.data['index']}")
        else:
            text = format_rect(swb_box.rect)
            if swb_box.delta is not None:
                text += "  " + format_delta(swb_box.delta)
            lines.append(prefix + text + ("  !" if swb_box.outside else ""))
    swb_ink = row.swb_ink
    if swb_ink is not None:
        delta = round2(swb_ink.ink - row.data["ink"])
        text = f"{format_ink(swb_ink.ink, swb_ink.bbox)}  dink={delta:g}"
        lines.append(
            f"{case}  {_SWB_LABEL:<5}  {row.label}  {text}" + ("  !" if swb_ink.outside else "")
        )
    return lines


def format_text(results: Sequence[CaseResult]) -> str:
    """The text output."""
    lines: list[str] = []
    for result in results:
        for row in result.rows:
            lines.extend(format_row(result.name, row))
        if result.swb_error:
            lines.append(f"{result.name}  swb    {result.swb_error}")
    return "\n".join(lines)


def format_json(results: Sequence[CaseResult]) -> str:
    """The same data as JSON."""
    cases = []
    for result in results:
        rows = []
        for row in result.rows:
            item: dict[str, Any] = {"kind": row.kind, "label": row.label, **row.data}
            if row.swb is not None:
                s = row.swb
                item["swb"] = {
                    "status": s.status,
                    "tag": s.tag,
                    "rect": s.rect,
                    "delta": s.delta,
                    "outside": s.outside,
                }
            if row.swb_ink is not None:
                item["swb_ink"] = {
                    "ink": row.swb_ink.ink,
                    "bbox": row.swb_ink.bbox,
                    "outside": row.swb_ink.outside,
                }
            rows.append(item)
        entry: dict[str, Any] = {
            "name": result.name,
            "viewport": list(result.viewport),
            "results": rows,
        }
        if result.swb_error:
            entry["swb_error"] = result.swb_error
        cases.append(entry)
    return json.dumps({"cases": cases}, indent=2, ensure_ascii=False)


# --- Comparing with swb ----------------------------------------------------


def compare_box(tag: str, index: int, rect: Rect | None, dump: BoxDump, tolerance: float) -> SwbBox:
    """Compares Chromium's box of the element at `index` (with `tag`) with
    the same index in swb's dump."""
    if index >= len(dump.elements):
        return SwbBox("missing", outside=True)
    element = dump.elements[index]
    if element.tag != tag:
        return SwbBox("tag-mismatch", tag=element.tag, outside=True)
    if rect is None and element.rect is None:
        return SwbBox("ok")
    if rect is None or element.rect is None:
        return SwbBox("box-mismatch", rect=element.rect, outside=True)
    delta = tuple(round2(s - c) for s, c in zip(element.rect, rect, strict=True))
    outside = any(abs(d) > tolerance for d in delta)
    return SwbBox("ok", rect=element.rect, delta=delta, outside=outside)  # type: ignore[arg-type]


def compare_ink(
    ink: float,
    bbox: tuple[int, int, int, int] | None,
    swb_ink: float,
    swb_bbox: tuple[int, int, int, int] | None,
    tolerance: float,
) -> SwbInk:
    """Compares swb's ink in an area with Chromium's (see the module
    documentation)."""
    outside = abs(swb_ink - ink) > max(2.0, INK_TOLERANCE * ink)
    if (bbox is None) != (swb_bbox is None):
        outside = True
    elif bbox is not None and swb_bbox is not None:
        edges = (bbox[0], bbox[1], bbox[0] + bbox[2], bbox[1] + bbox[3])
        swb_edges = (
            swb_bbox[0],
            swb_bbox[1],
            swb_bbox[0] + swb_bbox[2],
            swb_bbox[1] + swb_bbox[3],
        )
        outside |= any(abs(a - b) > tolerance for a, b in zip(edges, swb_edges, strict=True))
    return SwbInk(swb_ink, swb_bbox, outside)


def ink_in(screenshot: Image.Image, rect: Rect) -> tuple[float, tuple[int, int, int, int] | None]:
    """The ink of the area `rect` (CSS px at scale 1) of a screenshot."""
    x, y, w, h = rect
    box = (round(x), round(y), round(x + w), round(y + h))
    return ink_of_image(screenshot.crop(box))


def attach_swb(
    result: CaseResult, dump: BoxDump, tolerance: float, screenshot: Image.Image | None = None
) -> None:
    """Sets the swb comparison on each `boxes` row of the result, and on
    each `ink` row if there is a screenshot."""
    for row in result.rows:
        if row.kind == "boxes" and "index" in row.data:
            row.swb = compare_box(
                row.data["tag"], row.data["index"], row.data["rect"], dump, tolerance
            )
        elif row.kind == "ink" and screenshot is not None and row.data.get("area"):
            ink, bbox = ink_in(screenshot, row.data["area"])
            row.swb_ink = compare_ink(row.data["ink"], row.data["bbox"], ink, bbox, tolerance)


def any_outside(results: Sequence[CaseResult]) -> bool:
    """True if a compared box or ink is outside the tolerance (or swb
    failed)."""
    return any(r.swb_error for r in results) or any(
        (row.swb is not None and row.swb.outside)
        or (row.swb_ink is not None and row.swb_ink.outside)
        for r in results
        for row in r.rows
    )


def any_error(results: Sequence[CaseResult]) -> bool:
    """True if a query failed in Chromium (a bad selector or expression)."""
    return any("error" in row.data for r in results for row in r.rows)


# --- Chromium --------------------------------------------------------------

_ELEMENTS_JS = """
(selector) => {
  const all = Array.from(document.querySelectorAll('*'));
  const index = new Map(all.map((element, i) => [element, i]));
  const sx = window.scrollX;
  const sy = window.scrollY;
  return Array.from(document.querySelectorAll(selector), (element) => ({
    index: index.get(element),
    tag: element.localName,
    rects: Array.from(element.getClientRects(), (r) => [r.x + sx, r.y + sy, r.width, r.height]),
  }));
}
"""

_STYLE_JS = """
([selector, props]) => Array.from(document.querySelectorAll(selector), (element) => {
  const style = getComputedStyle(element);
  return Object.fromEntries(props.map((p) => [p, style.getPropertyValue(p)]));
})
"""


def _label(selector: str, number: int) -> str:
    """`selector[number]`; a selector list goes in parentheses."""
    shown = f"({selector})" if "," in selector else selector
    return f"{shown}[{number}]"


def _no_match(kind: str, selector: str) -> Row:
    return Row(kind, selector, {"matched": False, "rect": None})


def _box_rows(selector: str, elements: list[dict[str, Any]]) -> list[Row]:
    if not elements:
        return [_no_match("boxes", selector)]
    return [
        Row(
            "boxes",
            _label(selector, n),
            {"tag": e["tag"].lower(), "index": e["index"], "rect": union(e["rects"])},
        )
        for n, e in enumerate(elements, 1)
    ]


def _rect_rows(selector: str, elements: list[dict[str, Any]]) -> list[Row]:
    if not elements:
        return [_no_match("rects", selector)]
    rows = []
    for n, element in enumerate(elements, 1):
        rects = element["rects"] or [None]
        for f, rect in enumerate(rects, 1):
            rounded = None if rect is None else tuple(round2(v) for v in rect)
            rows.append(Row("rects", f"{_label(selector, n)}#{f}", {"rect": rounded}))
    return rows


def ink_of(png: bytes) -> tuple[float, tuple[int, int, int, int] | None]:
    """The ink of a screenshot: the sum of the darkness of its pixels (by
    luminance, 1 for black), rounded to 2 decimals, and the bounding box
    (x, y, width, height) of the pixels that are not white."""
    with Image.open(io.BytesIO(png)) as image:
        return ink_of_image(image)


def ink_of_image(image: Image.Image) -> tuple[float, tuple[int, int, int, int] | None]:
    """`ink_of` for a decoded image."""
    gray = image.convert("L")
    darkness = [255 - v for v in gray.tobytes()]
    ink = round(sum(darkness) / 255, 2)
    width = gray.width
    inked = [i for i, d in enumerate(darkness) if d > 0]
    if not inked:
        return ink, None
    xs = [i % width for i in inked]
    ys = [i // width for i in inked]
    return ink, (min(xs), min(ys), max(xs) - min(xs) + 1, max(ys) - min(ys) + 1)


async def _screenshot(page: Page, clip: dict[str, float]) -> bytes:
    """A screenshot of `clip` (document coordinates). The first screenshot
    of a session can fail before the page has painted once; it is tried
    again after a short wait."""
    for attempt in range(2):
        try:
            return await page.screenshot(
                clip=clip, full_page=True, animations="disabled", caret="hide"
            )
        except Exception:
            if attempt == 1:
                raise
            await page.wait_for_timeout(100)
    raise AssertionError("unreachable")


async def _ink_rows(page: Page, selector: str) -> list[Row]:
    elements = await page.evaluate(_ELEMENTS_JS, selector)
    if not elements:
        return [Row("ink", selector, {"matched": False})]
    rows = []
    for n, element in enumerate(elements, 1):
        rect = union(element["rects"])
        if rect is None or rect[2] <= 0 or rect[3] <= 0:
            rows.append(Row("ink", _label(selector, n), {"ink": 0.0, "bbox": None}))
            continue
        x, y, w, h = rect
        clip = {"x": x, "y": y, "width": w, "height": h}
        png = await _screenshot(page, clip)
        ink, bbox = ink_of(png)
        rows.append(Row("ink", _label(selector, n), {"ink": ink, "bbox": bbox, "area": rect}))
    return rows


async def _run_query(page: Page, query: Query) -> list[Row]:
    """Runs one query. A failing selector or expression gives an error row."""
    try:
        if query.kind == "js":
            return [Row("js", query.target, {"value": await page.evaluate(query.target)})]
        if query.kind == "ink":
            return await _ink_rows(page, query.target)
        if query.kind == "style":
            styles = await page.evaluate(_STYLE_JS, [query.target, list(query.props)])
            if not styles:
                return [_no_match("style", query.target)]
            return [
                Row("style", _label(query.target, n), {"style": style})
                for n, style in enumerate(styles, 1)
            ]
        elements = await page.evaluate(_ELEMENTS_JS, query.target)
        build = _box_rows if query.kind == "boxes" else _rect_rows
        return build(query.target, elements)
    except Exception as error:
        message = str(error).strip().splitlines()[0] if str(error).strip() else type(error).__name__
        return [Row(query.kind, query.target, {"error": message})]


async def probe_case(
    page: Page, directory: Path, number: int, case: Case
) -> tuple[CaseResult, Path]:
    """Loads the case in Chromium and runs its queries. Returns the result
    and the path of the page file."""
    await page.set_viewport_size({"width": case.viewport[0], "height": case.viewport[1]})
    path = directory / f"case-{number}.html"
    for name, source in case.files:
        try:
            shutil.copyfile(source, directory / name)
        except OSError as error:
            raise ProbeError(f"case {case.name!r}: cannot copy {source}: {error}") from error
    await browser.load_html(page, path, case.html)
    await browser.wait_for_fonts(page)
    await browser.stop_animations(page)
    result = CaseResult(case.name, case.viewport)
    for query in case.queries:
        result.rows.extend(await _run_query(page, query))
    return result, path


# --- swb -------------------------------------------------------------------


def run_swb(
    binary: Path, page: Path, viewport: tuple[int, int], directory: Path, screenshot: bool = False
) -> tuple[BoxDump, Image.Image | None]:
    """Renders the page in swb and returns its box dump and, with
    `screenshot`, its full-page screenshot. Raises `ProbeError` if swb
    fails."""
    dump_path = directory / f"{page.stem}.swb-boxes.json"
    log_path = directory / f"{page.stem}.swb.log"
    shot_path = directory / f"{page.stem}.swb.png" if screenshot else None
    command = swb.probe_command(binary, page.as_uri(), viewport, dump_path, shot_path)
    status = swb.run(command, log_path)
    if status != 0 or not dump_path.is_file():
        tail = log_path.read_text(encoding="utf-8", errors="replace").strip().splitlines()[-3:]
        raise ProbeError(f"swb failed (exit {status}): {' | '.join(tail)}")
    image = None
    if shot_path is not None:
        with Image.open(shot_path) as shot:
            image = shot.convert("RGB")
    return read_dump(dump_path), image


# --- Running ---------------------------------------------------------------


async def _probe_all(
    cases: Sequence[Case], binary: Path | None, tolerance: float, page: Page, directory: Path
) -> list[CaseResult]:
    results = []
    for number, case in enumerate(cases, 1):
        result, path = await probe_case(page, directory, number, case)
        if binary is not None:
            wants_ink = any(query.kind == "ink" for query in case.queries)
            try:
                dump, shot = run_swb(binary, path, case.viewport, directory, wants_ink)
                attach_swb(result, dump, tolerance, shot)
            except ProbeError as error:
                result.swb_error = str(error)
        results.append(result)
    return results


def probe(
    files: Sequence[Path],
    case_names: Sequence[str],
    swb_binary: Path | None,
    tolerance: float,
    as_json: bool,
) -> int:
    """Runs the cases of the files and prints the results. Returns 1 if a
    query failed (a bad selector or expression) or if swb was compared and
    a box is outside the tolerance, 2 for a bad request."""
    try:
        cases = select_cases([case for path in files for case in read_cases(path)], case_names)
    except ProbeError as error:
        log.error("%s", error)
        return 2
    if not cases:
        log.error("no cases to run")
        return 2
    try:
        results = asyncio.run(
            browser.in_chromium(
                lambda page, directory: _probe_all(cases, swb_binary, tolerance, page, directory)
            )
        )
    except ProbeError as error:
        log.error("%s", error)
        return 2
    print(format_json(results) if as_json else format_text(results))
    if any_error(results):
        return 1
    return 1 if swb_binary is not None and any_outside(results) else 0
