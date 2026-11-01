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
- `pixels`: `{"pixels": "#a", "points": [[0, 0], [5, 3]]}`: the RGB values
  of a screenshot of the page at the points, in px from the top left of the
  border box of each matching element (a point can lie outside the box, for
  a blur). Output `pixels  #a[1]  255,0,0 10,20,30`. With `--with-swb`, swb's
  values follow, with `!` when a channel differs by more than
  `PIXEL_TOLERANCE`. The page background should be set (the screenshot has
  no alpha).
- `files` (case, optional): files to put next to the page, as
  `{"published name": "source path"}`; a relative source path is relative
  to the repository root. Use it for fonts (`@font-face`) and images.
- `resources` (case, optional): text files that a local HTTP server serves
  next to the page, as `{"name": {"text": "...", "delay_ms": 200,
  "content_type": "text/css"}}` (`delay_ms` and `content_type` are
  optional; the type comes from the extension). A case with resources is
  loaded from `http://127.0.0.1:PORT/` in Chromium and in swb, and each
  resource is answered after its delay, so that a case can order the
  arrival of scripts and stylesheets. A resource can also set `status`
  (default 200), `headers` (an object of name to a string or a list of
  strings: CORS headers, `Set-Cookie`, `Location`) and `options`, the answer
  to an `OPTIONS` request: `"none"` (default, 404), `"deny"` (200 without
  CORS headers), `"permissive"` (204 that allows the request's origin,
  credentials, method and headers; `"max_age": N` in the object form adds
  `Access-Control-Max-Age`) or an object `{"status": 204, "headers": {...},
  "max_age": 600}` for an exact answer. Any method is answered with the
  resource. The server listens on two ports, so a case has two origins: in
  the page, in resource texts and in header values `$ORIGIN` is the origin
  of the page and `$OTHER` the other origin (both `http://127.0.0.1:PORT`).
  The server logs the requests of each case: `GET <case folder>/__log`
  (relative URL `__log`) answers a JSON array of `{"method", "path",
  "server": "main"|"other", "origin", "cookie", "headers", "body"}` for the
  requests since the page of the case was loaded (the page itself and
  `__log` are not listed; `headers` has `accept`, `authorization`,
  `content-type`, `range`, `access-control-request-*` and the `x-*`
  headers). A case can then tell whether a preflight ran and whether
  cookies went.
- `time_zone` (file and case, optional): the IANA name of the time zone of
  the page (`"Europe/Zagreb"`), for the results of `Date`. Without it the
  page has the machine's zone. Chromium gets it as the `timezone_id` of a
  browser context; a case with its own zone runs in a context of that zone.

- `javascript` (file and case, optional; default false): true runs the
  scripts of the page. The case runs in a Chromium context with JavaScript
  on (contexts are keyed by time zone and this flag). With `--with-swb`, swb
  runs the case with `--js`, and the `js` queries are also evaluated in swb
  (`--eval`, docs/testing.md "Probe Chromium behaviour").
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
  and calls `element.focus()` in the page, `eval` takes an expression,
  `back` and `forward` (`true`) go in the history of the tab.
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
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PIL import Image
from playwright.async_api import Page

from swbtools import browser, swb
from swbtools.boxes import BoxDump, Rect, read_dump, round2, union
from swbtools.probe_cases import (
    _POINT_STEPS,
    Case,
    ProbeError,
    Query,
    read_cases,
    select_cases,
)
from swbtools.probe_format import (
    CaseResult,
    Row,
    SwbBox,
    SwbInk,
    SwbPixels,
    SwbValue,
    _values,
    format_json,
    format_text,
)
from swbtools.probe_server import CaseServer, _serve_case, _write_case_files

log = logging.getLogger(__name__)

DEFAULT_TOLERANCE = 1.0
"""Default maximum difference in px between swb and Chromium."""
INK_TOLERANCE = 0.05
"""The largest relative difference of swb's ink from Chromium's (at least 2
units), for anti-aliasing differences."""

PIXEL_TOLERANCE = 2
"""The largest difference of one channel of a `pixels` value between swb and
Chromium that still counts as equal (anti-aliasing and rounding)."""


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


def pixels_in(
    screenshot: Image.Image, area: Rect, points: Sequence[Sequence[int]]
) -> list[tuple[int, int, int]]:
    """The RGB values of a screenshot at `points`, in px from the top left
    of `area` (a point outside the image is white)."""
    rgb = screenshot.convert("RGB")
    left, top = round(area[0]), round(area[1])
    values = []
    for x, y in points:
        inside = 0 <= left + x < rgb.width and 0 <= top + y < rgb.height
        values.append(rgb.getpixel((left + x, top + y)) if inside else (255, 255, 255))
    return values  # type: ignore[return-value]


def compare_pixels(
    pixels: Sequence[Sequence[int]], swb_pixels: Sequence[Sequence[int]]
) -> SwbPixels:
    """Compares swb's pixels with Chromium's: a channel that differs by more
    than `PIXEL_TOLERANCE` is outside."""
    outside = len(pixels) != len(swb_pixels) or any(
        abs(a - b) > PIXEL_TOLERANCE
        for chromium, swb_value in zip(pixels, swb_pixels, strict=False)
        for a, b in zip(chromium, swb_value, strict=True)
    )
    return SwbPixels([tuple(p) for p in swb_pixels], outside)  # type: ignore[misc]


def attach_swb(
    result: CaseResult, dump: BoxDump, tolerance: float, screenshot: Image.Image | None = None
) -> None:
    """Sets the swb comparison on each `boxes` row of the result, and on
    each `ink` and `pixels` row if there is a screenshot."""
    for row in result.rows:
        if row.kind == "boxes" and "index" in row.data:
            row.swb = compare_box(
                row.data["tag"], row.data["index"], row.data["rect"], dump, tolerance
            )
        elif row.kind == "ink" and screenshot is not None and row.data.get("area"):
            ink, bbox = ink_in(screenshot, row.data["area"])
            row.swb_ink = compare_ink(row.data["ink"], row.data["bbox"], ink, bbox, tolerance)
        elif row.kind == "pixels" and screenshot is not None and row.data.get("area"):
            swb_pixels = pixels_in(screenshot, row.data["area"], row.data["points"])
            row.swb_pixels = compare_pixels(row.data["pixels"], swb_pixels)


def any_outside(results: Sequence[CaseResult]) -> bool:
    """True if a compared box or ink is outside the tolerance (or swb
    failed)."""
    return any(r.swb_error for r in results) or any(
        (row.swb is not None and row.swb.outside)
        or (row.swb_ink is not None and row.swb_ink.outside)
        or (row.swb_pixels is not None and row.swb_pixels.outside)
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


async def _pixel_rows(page: Page, selector: str, points: Sequence[Sequence[int]]) -> list[Row]:
    """The RGB values at `points` (px from the top left of the element's
    border box) of a screenshot of the whole page, for each match."""
    elements = await page.evaluate(_ELEMENTS_JS, selector)
    if not elements:
        return [Row("pixels", selector, {"matched": False})]
    png = await page.screenshot(full_page=True, animations="disabled", caret="hide")
    rows = []
    with Image.open(io.BytesIO(png)) as image:
        for n, element in enumerate(elements, 1):
            rect = union(element["rects"])
            if rect is None:
                rows.append(Row("pixels", _label(selector, n), {"matched": False}))
                continue
            pixels = pixels_in(image, rect, points)
            data = {"pixels": pixels, "points": [list(p) for p in points], "area": rect}
            rows.append(Row("pixels", _label(selector, n), data))
    return rows


async def _run_query(page: Page, query: Query) -> list[Row]:
    """Runs one query. A failing selector or expression gives an error row."""
    try:
        if query.kind == "pixels":
            return await _pixel_rows(page, query.target, query.points)
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


async def run_steps(page: Page, steps: Sequence[dict[str, Any]], pause_ms: int = 0) -> None:
    """Sends the input of the steps to Chromium. `pause_ms` is a pause after
    each step (page time), for tools that record page state between steps."""
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
        elif kind == "viewport":
            await page.set_viewport_size({"width": argument[0], "height": argument[1]})
        elif kind == "colorScheme":
            await page.emulate_media(color_scheme=argument)
        elif kind == "back":
            await page.go_back()
        elif kind == "forward":
            await page.go_forward()
        for modifier in reversed(modifiers):
            await page.keyboard.up(modifier)
        if pause_ms:
            await page.wait_for_timeout(pause_ms)


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
    runs with `--js` and `--eval` for its `js` queries."""
    extra = [JS_OPTION] if case.javascript else []
    if case.steps:
        extra.append(f"{STEPS_OPTION}={json.dumps(list(case.steps))}")
    expressions = [query.target for query in case.queries if query.kind == "js"]
    if case.javascript:
        extra += [f"{EVAL_OPTION}={expression}" for expression in expressions]
    wants_ink = any(query.kind in ("ink", "pixels") for query in case.queries)
    try:
        run = run_swb(binary, path, case.viewport, directory, wants_ink, extra, url)
        attach_swb(result, run.dump, tolerance, run.image)
        if case.javascript:
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
