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
      {"js": "document.scrollingElement.scrollHeight"}
    ]}]}
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

Chromium has the settings of the other tools (`browser.py`: bundled fonts,
JavaScript disabled, scale 1). The page is a file in a temporary directory,
loaded by navigation (see `measure.py`).

Output: one line per value, `CASE  KIND  LABEL  VALUES`. A label is the
selector and the 1-based index among its matches (`#a > p[2]`; a selector
list goes in parentheses: `(#a, #b)[2]`); `rects` adds `#N`, the
fragment. With `--with-swb`, swb renders the same page; each `boxes` element is
looked up by its index in `document.querySelectorAll('*')`, and a line
`CASE  swb  LABEL  VALUES  dx=.. dy=.. dw=.. dh=..` follows the Chromium
line, with `!` when a delta is above the tolerance. `rects`, `style` and
`js` are Chromium only.
"""

import asyncio
import json
import logging
from collections.abc import Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from playwright.async_api import Page

from swbtools import browser, swb
from swbtools.boxes import BoxDump, Rect, read_dump, round2, union
from swbtools.measure import in_chromium

log = logging.getLogger(__name__)

DEFAULT_VIEWPORT = browser.LAYOUT_VIEWPORT
DEFAULT_TOLERANCE = 1.0
"""Default maximum difference in px between swb and Chromium."""

QUERY_KINDS = ("boxes", "rects", "style", "js")
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
    """The CSS selector (`boxes`, `rects`, `style`) or the expression (`js`)."""
    props: tuple[str, ...] = ()
    """The properties of a `style` query."""


@dataclass(frozen=True)
class Case:
    """One HTML document and what to measure in it."""

    name: str
    html: str
    viewport: tuple[int, int]
    queries: tuple[Query, ...]


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
    return Case(name, html, viewport, queries)


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
class Row:
    """One value of a case."""

    kind: str
    label: str
    """Selector with index, or the expression."""
    data: dict[str, Any] = field(default_factory=dict)
    swb: SwbBox | None = None


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
    return json.dumps(data["value"], ensure_ascii=False)


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


def attach_swb(result: CaseResult, dump: BoxDump, tolerance: float) -> None:
    """Sets the swb comparison on each `boxes` row of the result."""
    for row in result.rows:
        if row.kind == "boxes" and "index" in row.data:
            row.swb = compare_box(
                row.data["tag"], row.data["index"], row.data["rect"], dump, tolerance
            )


def any_outside(results: Sequence[CaseResult]) -> bool:
    """True if a compared box is outside the tolerance (or swb failed)."""
    return any(r.swb_error for r in results) or any(
        row.swb is not None and row.swb.outside for r in results for row in r.rows
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


async def _run_query(page: Page, query: Query) -> list[Row]:
    """Runs one query. A failing selector or expression gives an error row."""
    try:
        if query.kind == "js":
            return [Row("js", query.target, {"value": await page.evaluate(query.target)})]
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
    path = directory / f"case-{number}.html"
    path.write_text(case.html, encoding="utf-8")
    await page.set_viewport_size({"width": case.viewport[0], "height": case.viewport[1]})
    await page.goto(path.as_uri(), wait_until="load")
    await browser.stop_animations(page)
    result = CaseResult(case.name, case.viewport)
    for query in case.queries:
        result.rows.extend(await _run_query(page, query))
    return result, path


# --- swb -------------------------------------------------------------------


def run_swb(binary: Path, page: Path, viewport: tuple[int, int], directory: Path) -> BoxDump:
    """Renders the page in swb and returns its box dump. Raises `ProbeError`
    if swb fails."""
    dump_path = directory / f"{page.stem}.swb-boxes.json"
    log_path = directory / f"{page.stem}.swb.log"
    status = swb.run(swb.probe_command(binary, page.as_uri(), viewport, dump_path), log_path)
    if status != 0 or not dump_path.is_file():
        tail = log_path.read_text(encoding="utf-8", errors="replace").strip().splitlines()[-3:]
        raise ProbeError(f"swb failed (exit {status}): {' | '.join(tail)}")
    return read_dump(dump_path)


# --- Running ---------------------------------------------------------------


async def _probe_all(
    cases: Sequence[Case], binary: Path | None, tolerance: float, page: Page, directory: Path
) -> list[CaseResult]:
    results = []
    for number, case in enumerate(cases, 1):
        result, path = await probe_case(page, directory, number, case)
        if binary is not None:
            try:
                attach_swb(result, run_swb(binary, path, case.viewport, directory), tolerance)
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
    results = asyncio.run(
        in_chromium(
            lambda page, directory: _probe_all(cases, swb_binary, tolerance, page, directory)
        )
    )
    print(format_json(results) if as_json else format_text(results))
    if any_error(results):
        return 1
    return 1 if swb_binary is not None and any_outside(results) else 0
