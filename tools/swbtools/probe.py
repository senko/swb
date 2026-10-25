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
    "files": {"x.ttf": "fixtures/fonts/DejaVuSans.ttf"},
    "resources": {"a.js": {"text": "log('a')", "delay_ms": 200}}}]}
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
- `resources` (case, optional): text files that a local HTTP server serves
  next to the page, as `{"name": {"text": "...", "delay_ms": 200,
  "content_type": "text/css"}}` (`delay_ms` and `content_type` are
  optional; the type comes from the extension). A case with resources is
  loaded from `http://127.0.0.1:PORT/` in Chromium and in swb, and each
  resource is answered after its delay, so that a case can order the
  arrival of scripts and stylesheets.
- `time_zone` (file and case, optional): the IANA name of the time zone of
  the page (`"Europe/Zagreb"`), for the results of `Date`. Without it the
  page has the machine's zone. Chromium gets it as the `timezone_id` of a
  browser context; a case with its own zone runs in a context of that zone.

- `javascript` (file and case, optional; default false): true runs the
  scripts of the page. The case runs in a Chromium context with JavaScript
  on (contexts are keyed by time zone and this flag). With `--with-swb`, swb
  runs the case with `--js`, and the `js` queries are also evaluated in swb
  (`--eval`, docs/testing.md "Probe Chromium behaviour"). A swb without
  these options prints `not supported` for its lines.
- `steps` (case, optional; needs `javascript`): input to send before the
  queries, in order. Chromium gets it through Playwright's mouse and
  keyboard; swb through the `input.*` methods of the automation API
  (`--steps`). Each step is an object with one key: `click`, `dblclick`,
  `rightclick`, `middleclick` and `hover` take a selector (the center of the
  element) or `[x, y]` in viewport coordinates; `mousedown` and `mouseup`
  take a button name (`left`, `middle`, `right`) at the current position;
  `type` takes text, `key` a key name (`Enter`, `Tab`, `a`, `Shift+Tab`),
  `wheel` takes `[dx, dy]` at the current position, `wait` takes
  milliseconds (the event loop runs meanwhile), `focus` takes a selector
  and calls `element.focus()` in the page, `eval` takes an expression.
  The point steps take `"modifiers": ["Shift", "Control", "Alt", "Meta"]`,
  held during the step.

Chromium has the settings of the other tools (`browser.py`: bundled fonts,
JavaScript disabled unless the case sets `javascript`, scale 1). The page is a
file in a temporary directory, loaded by navigation (see `measure.py`).

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
bounding box by more than the tolerance. `rects` and `style` are Chromium
only, and so is `js` in a case without `javascript`; in a JavaScript case a
line `CASE  swb    EXPR  VALUE` follows the `js` line, with `!` when the
value differs.
"""

import asyncio
import io
import json
import logging
import mimetypes
import shutil
import threading
import time
from collections.abc import Sequence
from dataclasses import dataclass, field
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
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
class Resource:
    """A text file that the local server serves next to the page of a case."""

    name: str
    text: str
    delay_ms: int = 0
    """How long the server waits before it answers."""
    content_type: str | None = None
    """The `Content-Type`; `None` takes it from the extension."""


@dataclass(frozen=True)
class Case:
    """One HTML document and what to measure in it."""

    name: str
    html: str
    viewport: tuple[int, int]
    queries: tuple[Query, ...]
    files: tuple[tuple[str, Path], ...] = ()
    """Files to copy next to the page: (published name, source path)."""
    time_zone: str | None = None
    """The IANA name of the time zone of the page, or `None` for the
    machine's zone."""
    javascript: bool = False
    """True if the scripts of the page run."""
    resources: tuple[Resource, ...] = ()
    """Files that the local HTTP server serves; with them the page loads
    over HTTP."""
    steps: tuple[dict[str, Any], ...] = ()
    """The input to send before the queries (see the module comment)."""


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


def _parse_time_zone(value: Any, where: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not value.strip():
        raise ProbeError(f"{where}: time_zone must be a non-empty string (an IANA name)")
    return value


def _parse_javascript(value: Any, where: str) -> bool:
    if not isinstance(value, bool):
        raise ProbeError(f"{where}: javascript must be true or false")
    return value


def _parse_case(
    item: Any,
    number: int,
    default_viewport: tuple[int, int],
    default_zone: str | None = None,
    default_javascript: bool = False,
) -> Case:
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
    zone = _parse_time_zone(item["time_zone"], where) if "time_zone" in item else default_zone
    javascript = (
        _parse_javascript(item["javascript"], where) if "javascript" in item else default_javascript
    )
    files = _parse_files(item.get("files", {}), where)
    resources = _parse_resources(item.get("resources", {}), where)
    steps = _parse_steps(item.get("steps", []), where)
    if steps and not javascript:
        raise ProbeError(f"{where}: steps need javascript")
    return Case(name, html, viewport, queries, files, zone, javascript, resources, steps)


STEP_KINDS = (
    "click",
    "dblclick",
    "rightclick",
    "middleclick",
    "hover",
    "mousedown",
    "mouseup",
    "type",
    "key",
    "wheel",
    "wait",
    "focus",
    "eval",
)
_POINT_STEPS = ("click", "dblclick", "rightclick", "middleclick", "hover")


def _is_point(argument: Any) -> bool:
    return (
        isinstance(argument, list)
        and len(argument) == 2
        and all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in argument)
    )


def _parse_steps(value: Any, where: str) -> tuple[dict[str, Any], ...]:
    """The `steps` of a case: objects with one key of `STEP_KINDS`."""
    if not isinstance(value, list):
        raise ProbeError(f"{where}: steps must be a list")
    steps = []
    for number, item in enumerate(value, 1):
        if not isinstance(item, dict) or not item:
            raise ProbeError(f"{where}, step {number}: a step is an object with one key")
        modifiers = item.get("modifiers")
        rest = {k: v for k, v in item.items() if k != "modifiers"}
        if len(rest) != 1:
            raise ProbeError(f"{where}, step {number}: a step is an object with one key")
        ((kind, argument),) = rest.items()
        if modifiers is not None and not (
            kind in _POINT_STEPS
            and isinstance(modifiers, list)
            and all(m in ("Shift", "Control", "Alt", "Meta") for m in modifiers)
        ):
            raise ProbeError(f"{where}, step {number}: bad modifiers")
        if kind not in STEP_KINDS:
            raise ProbeError(f"{where}, step {number}: unknown step {kind!r}")
        if kind in _POINT_STEPS:
            ok = _is_point(argument) or (isinstance(argument, str) and bool(argument))
        elif kind == "wheel":
            ok = _is_point(argument)
        elif kind == "wait":
            ok = isinstance(argument, int) and not isinstance(argument, bool) and argument >= 0
        elif kind in ("mousedown", "mouseup"):
            ok = argument in ("left", "middle", "right")
        else:
            ok = isinstance(argument, str) and bool(argument)
        if not ok:
            raise ProbeError(f"{where}, step {number}: bad argument for {kind}")
        steps.append(item)
    return tuple(steps)


def _parse_resources(value: Any, where: str) -> tuple[Resource, ...]:
    """The `resources` of a case: plain file names to text, delay and type."""
    if not isinstance(value, dict):
        raise ProbeError(f"{where}: resources must be an object of name to resource")
    resources = []
    for name, item in value.items():
        if not name or "/" in name or "\\" in name or name in (".", ".."):
            raise ProbeError(f"{where}: resource name {name!r} must be a plain file name")
        if not isinstance(item, dict) or not isinstance(item.get("text"), str):
            raise ProbeError(f"{where}: resource {name!r} needs a text string")
        extra = set(item) - {"text", "delay_ms", "content_type"}
        if extra:
            raise ProbeError(f"{where}: unknown key {sorted(extra)[0]!r} in resource {name!r}")
        delay = item.get("delay_ms", 0)
        if not isinstance(delay, int) or isinstance(delay, bool) or not 0 <= delay <= 10_000:
            raise ProbeError(f"{where}: delay_ms of {name!r} must be 0 to 10000")
        kind = item.get("content_type")
        if kind is not None and (not isinstance(kind, str) or not kind):
            raise ProbeError(f"{where}: content_type of {name!r} must be a non-empty string")
        resources.append(Resource(name, item["text"], delay, kind))
    return tuple(resources)


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
    zone = _parse_time_zone(data.get("time_zone"), "file")
    javascript = _parse_javascript(data.get("javascript", False), "file")
    cases = [
        _parse_case(item, i, viewport, zone, javascript) for i, item in enumerate(data["cases"], 1)
    ]
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
class SwbValue:
    """swb's answer to a `js` query (`--eval`) and whether it differs."""

    text: str
    """The printed value, or `error: MESSAGE`."""
    outside: bool = False


@dataclass
class Row:
    """One value of a case."""

    kind: str
    label: str
    """Selector with index, or the expression."""
    data: dict[str, Any] = field(default_factory=dict)
    swb: SwbBox | None = None
    swb_ink: SwbInk | None = None
    swb_value: SwbValue | None = None
    """swb's value of a `js` row in a JavaScript case."""
    swb_unsupported: bool = False
    """True if swb lacks an option that the row needs (`--js`)."""


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
    if row.swb_unsupported:
        lines.append(f"{case}  {_SWB_LABEL:<5}  {row.label}  not supported")
    swb_value = row.swb_value
    if swb_value is not None:
        text = swb_value.text + ("  !" if swb_value.outside else "")
        lines.append(f"{case}  {_SWB_LABEL:<5}  {row.label}  {text}")
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
            if row.swb_value is not None:
                item["swb_value"] = {
                    "text": row.swb_value.text,
                    "outside": row.swb_value.outside,
                }
            if row.swb_unsupported:
                item["swb_unsupported"] = True
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
        or (row.swb_value is not None and row.swb_value.outside)
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
            value = await page.evaluate(query.target)
            try:
                json.dumps(value)
            except TypeError as error:
                raise ProbeError(f"the value is not JSON: {error}") from error
            return [Row("js", query.target, {"value": value})]
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


class CaseServer:
    """A local HTTP server for the cases with resources: it serves a
    directory, answers each resource after its delay with its type, and
    forbids caching. It runs on its own threads until `close`."""

    def __init__(self, directory: Path) -> None:
        self.delays: dict[str, float] = {}
        """URL path to the delay in seconds."""
        self.types: dict[str, str] = {}
        """URL path to the `Content-Type`."""
        server = self

        class Handler(SimpleHTTPRequestHandler):
            def __init__(self, *args: Any, **kwargs: Any) -> None:
                super().__init__(*args, directory=str(directory), **kwargs)

            def do_GET(self) -> None:
                path = self.path.split("?", 1)[0]
                delay = server.delays.get(path, 0.0)
                if delay:
                    time.sleep(delay)
                self._swb_type = server.types.get(path)
                super().do_GET()

            def guess_type(self, path: Any) -> str:
                kind = getattr(self, "_swb_type", None)
                return kind or mimetypes.guess_type(str(path))[0] or "application/octet-stream"

            def end_headers(self) -> None:
                self.send_header("Cache-Control", "no-store")
                super().end_headers()

            def log_message(self, format: str, *args: Any) -> None:
                log.debug("case server: " + format, *args)

        self._httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._httpd.daemon_threads = True
        self._thread = threading.Thread(target=self._httpd.serve_forever, daemon=True)
        self._thread.start()

    @property
    def origin(self) -> str:
        """`http://127.0.0.1:PORT`."""
        return f"http://127.0.0.1:{self._httpd.server_address[1]}"

    def close(self) -> None:
        self._httpd.shutdown()
        self._httpd.server_close()


_JAVASCRIPT_TYPE = "text/javascript"


def _resource_type(resource: Resource) -> str:
    """The `Content-Type` of a resource: its own, else from the extension."""
    if resource.content_type:
        return resource.content_type
    if resource.name.endswith((".js", ".mjs")):
        return _JAVASCRIPT_TYPE
    return mimetypes.guess_type(resource.name)[0] or "text/plain"


def _write_case_files(case: Case, directory: Path) -> None:
    """Copies the `files` of a case into `directory`."""
    for name, source in case.files:
        try:
            shutil.copyfile(source, directory / name)
        except OSError as error:
            raise ProbeError(f"case {case.name!r}: cannot copy {source}: {error}") from error


def _serve_case(case: Case, number: int, directory: Path, server: CaseServer) -> tuple[Path, str]:
    """Writes the page and the resources of a case into its own directory
    below the server's and returns the page's path and URL."""
    folder = f"case-{number}"
    case_dir = directory / folder
    case_dir.mkdir(exist_ok=True)
    _write_case_files(case, case_dir)
    for resource in case.resources:
        (case_dir / resource.name).write_text(resource.text, encoding="utf-8")
        path = f"/{folder}/{resource.name}"
        server.delays[path] = resource.delay_ms / 1000
        server.types[path] = _resource_type(resource)
    page_path = case_dir / f"case-{number}.html"
    page_path.write_text(case.html, encoding="utf-8")
    return page_path, f"{server.origin}/{folder}/{page_path.name}"


_BUTTONS = {"click": "left", "dblclick": "left", "rightclick": "right", "middleclick": "middle"}


async def _point_of(page: Page, argument: Any) -> tuple[float, float]:
    """The viewport point of a step: `[x, y]`, or the center of the first
    element that the selector matches (scrolled into view)."""
    if isinstance(argument, list):
        return float(argument[0]), float(argument[1])
    locator = page.locator(argument).first
    await locator.scroll_into_view_if_needed(timeout=5000)
    box = await locator.bounding_box(timeout=5000)
    if box is None:
        raise ProbeError(f"step: {argument!r} has no box")
    return box["x"] + box["width"] / 2, box["y"] + box["height"] / 2


async def run_steps(page: Page, steps: Sequence[dict[str, Any]]) -> None:
    """Sends the input of the steps to Chromium."""
    for step in steps:
        modifiers = step.get("modifiers", [])
        ((kind, argument),) = [(k, v) for k, v in step.items() if k != "modifiers"]
        for modifier in modifiers:
            await page.keyboard.down(modifier)
        if kind in _POINT_STEPS:
            x, y = await _point_of(page, argument)
            if kind == "hover":
                await page.mouse.move(x, y)
            else:
                clicks = 2 if kind == "dblclick" else 1
                await page.mouse.click(x, y, button=_BUTTONS[kind], click_count=clicks)
        elif kind == "mousedown":
            await page.mouse.down(button=argument)
        elif kind == "mouseup":
            await page.mouse.up(button=argument)
        elif kind == "type":
            await page.keyboard.type(argument)
        elif kind == "key":
            await page.keyboard.press(argument)
        elif kind == "wheel":
            await page.mouse.wheel(argument[0], argument[1])
        elif kind == "wait":
            await page.wait_for_timeout(argument)
        elif kind == "focus":
            await page.evaluate("(s) => document.querySelector(s).focus()", argument)
        elif kind == "eval":
            await page.evaluate(argument)
        for modifier in reversed(modifiers):
            await page.keyboard.up(modifier)


async def probe_case(
    page: Page, directory: Path, number: int, case: Case, server: CaseServer | None = None
) -> tuple[CaseResult, Path, str]:
    """Loads the case in Chromium and runs its queries. Returns the result,
    the path of the page file and its URL (`http:` from `server` for a case
    with resources, else `file:`)."""
    await page.set_viewport_size({"width": case.viewport[0], "height": case.viewport[1]})
    if case.resources:
        if server is None:
            raise ProbeError(f"case {case.name!r}: resources need the case server")
        path, url = _serve_case(case, number, directory, server)
        await page.goto(url, wait_until="load")
    else:
        path = directory / f"case-{number}.html"
        _write_case_files(case, directory)
        await browser.load_html(page, path, case.html)
        url = path.as_uri()
    await browser.wait_for_fonts(page)
    await browser.stop_animations(page)
    if case.steps:
        try:
            await run_steps(page, case.steps)
        except ProbeError:
            raise
        except Exception as error:
            message = str(error).strip().splitlines()[0] if str(error).strip() else "failed"
            raise ProbeError(f"case {case.name!r}: step failed: {message}") from error
    result = CaseResult(case.name, case.viewport)
    for query in case.queries:
        result.rows.extend(await _run_query(page, query))
    return result, path, url


# --- swb -------------------------------------------------------------------


@dataclass
class SwbRun:
    """What one swb run of a case produced."""

    dump: BoxDump
    image: Image.Image | None
    """The full-page screenshot, if it was requested."""
    stdout: list[str]
    """The lines swb printed to stdout (the `--eval` answers)."""


def run_swb(
    binary: Path,
    page: Path,
    viewport: tuple[int, int],
    directory: Path,
    screenshot: bool = False,
    extra: list[str] | None = None,
    url: str | None = None,
) -> SwbRun:
    """Renders the page in swb and returns its box dump, with `screenshot`
    its full-page screenshot, and its stdout. `extra` are additional swb
    options; `url` is the page's URL if it is not the file's (a case served
    over HTTP). Raises `ProbeError` if swb fails."""
    dump_path = directory / f"{page.stem}.swb-boxes.json"
    log_path = directory / f"{page.stem}.swb.log"
    out_path = directory / f"{page.stem}.swb.out"
    shot_path = directory / f"{page.stem}.swb.png" if screenshot else None
    command = swb.probe_command(binary, url or page.as_uri(), viewport, dump_path, shot_path, extra)
    status = swb.run(command, log_path, out_path)
    if status != 0 or not dump_path.is_file():
        tail = log_path.read_text(encoding="utf-8", errors="replace").strip().splitlines()[-3:]
        raise ProbeError(f"swb failed (exit {status}): {' | '.join(tail)}")
    image = None
    if shot_path is not None:
        with Image.open(shot_path) as shot:
            image = shot.convert("RGB")
    text = out_path.read_text(encoding="utf-8", errors="replace")
    stdout = [line for line in text.splitlines() if line.strip()]
    return SwbRun(read_dump(dump_path), image, stdout)


# --- swb values of `js` queries ----------------------------------------------

JS_OPTION = "--js"
EVAL_OPTION = "--eval"
STEPS_OPTION = "--steps"
_PLAYWRIGHT_PREFIX = "Page.evaluate: "


def printed_error(message: str) -> str:
    """The first line of Chromium's error message as the probe prints it,
    without Playwright's `Page.evaluate: ` prefix."""
    return message.removeprefix(_PLAYWRIGHT_PREFIX)


def parse_swb_value(line: str) -> str:
    """The printed form of one `--eval` answer: the line is `{"value": V}`
    or `{"error": "MESSAGE"}` (docs/testing.md, "Probe Chromium behaviour").
    A value is parsed (Python's JSON reader accepts `NaN`, `Infinity` and
    `-Infinity`) and printed as the probe prints Chromium's, so that the
    two compare as text. A line that does not follow the contract gives
    `invalid answer: LINE`."""
    try:
        answer = json.loads(line)
    except json.JSONDecodeError:
        return f"invalid answer: {line}"
    if isinstance(answer, dict) and set(answer) == {"value"}:
        return json.dumps(answer["value"], ensure_ascii=False)
    if isinstance(answer, dict) and set(answer) == {"error"} and isinstance(answer["error"], str):
        return f"error: {printed_error(answer['error'])}"
    return f"invalid answer: {line}"


def attach_swb_values(result: CaseResult, answers: list[str]) -> None:
    """Sets swb's value on each `js` row of the result, from the `--eval`
    answer lines in order. Raises `ProbeError` if the number of answers is
    wrong."""
    rows = [row for row in result.rows if row.kind == "js"]
    if len(answers) != len(rows):
        raise ProbeError(f"swb printed {len(answers)} answers for {len(rows)} --eval expressions")
    for row, answer in zip(rows, answers, strict=True):
        text = parse_swb_value(answer)
        if "error" in row.data:
            chromium = f"error: {printed_error(row.data['error'])}"
        else:
            chromium = _values(row)
        row.swb_value = SwbValue(text, outside=text != chromium)


def mark_unsupported(result: CaseResult, kinds: Sequence[str]) -> None:
    """Marks the rows of the given kinds: swb lacks an option they need."""
    for row in result.rows:
        if row.kind in kinds:
            row.swb_unsupported = True


# --- Running ---------------------------------------------------------------


async def _new_context_page(page: Page, case: Case) -> Page:
    """A page in a new browser context with the time zone and the
    JavaScript setting of the case (the context is closed with the
    browser)."""
    chromium = page.context.browser
    if chromium is None:
        raise ProbeError("the page has no browser")
    try:
        context = await browser.new_context(
            chromium, case.viewport, case.time_zone, case.javascript
        )
    except Exception as error:
        message = str(error).strip().splitlines()[0] if str(error).strip() else "failed"
        raise ProbeError(f"case {case.name!r}: time zone {case.time_zone!r}: {message}") from error
    return await context.new_page()


def _compare_with_swb(
    result: CaseResult,
    case: Case,
    binary: Path,
    tolerance: float,
    path: Path,
    directory: Path,
    url: str | None = None,
) -> None:
    """Runs swb on the case (the page at `url`, else the file `path`) and
    attaches its boxes, ink and values to the result. A JavaScript case
    needs `--js` (and `--eval` for its `js` queries); a swb without them has
    `not supported` lines instead."""
    flags = swb.supported_flags(binary)
    if case.javascript and JS_OPTION not in flags:
        mark_unsupported(result, ("boxes", "js"))
        return
    if case.steps and STEPS_OPTION not in flags:
        mark_unsupported(result, ("boxes", "js"))
        return
    evaluates = case.javascript and EVAL_OPTION in flags
    extra = [JS_OPTION] if case.javascript else []
    if case.steps:
        extra.append(f"{STEPS_OPTION}={json.dumps(list(case.steps))}")
    expressions = [query.target for query in case.queries if query.kind == "js"]
    if evaluates:
        extra += [f"{EVAL_OPTION}={expression}" for expression in expressions]
    elif case.javascript:
        mark_unsupported(result, ("js",))
    wants_ink = any(query.kind == "ink" for query in case.queries)
    try:
        run = run_swb(binary, path, case.viewport, directory, wants_ink, extra, url)
        attach_swb(result, run.dump, tolerance, run.image)
        if evaluates:
            attach_swb_values(result, run.stdout)
    except ProbeError as error:
        result.swb_error = str(error)


async def _probe_all(
    cases: Sequence[Case], binary: Path | None, tolerance: float, page: Page, directory: Path
) -> list[CaseResult]:
    results = []
    pages = {(None, False): page}
    server = CaseServer(directory) if any(case.resources for case in cases) else None
    try:
        for number, case in enumerate(cases, 1):
            key = (case.time_zone, case.javascript)
            if key not in pages:
                pages[key] = await _new_context_page(page, case)
            # Input steps get a page of their own: the mouse position of a
            # page carries over to its next document.
            case_page = await pages[key].context.new_page() if case.steps else pages[key]
            try:
                result, path, url = await probe_case(case_page, directory, number, case, server)
            finally:
                if case.steps:
                    await case_page.close()
            if binary is not None:
                _compare_with_swb(result, case, binary, tolerance, path, directory, url)
            results.append(result)
    finally:
        if server is not None:
            server.close()
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
