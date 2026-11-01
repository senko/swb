"""The case files of `swbtools probe`: the case, query and resource records and
the parser of the JSON format (see `probe.py` for the format)."""

import json
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from swbtools import browser, paths

DEFAULT_VIEWPORT = browser.LAYOUT_VIEWPORT
QUERY_KINDS = ("boxes", "rects", "style", "js", "ink", "pixels")


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
    points: tuple[tuple[int, int], ...] = ()
    """The points of a `pixels` query: (x, y) in px from the top left of the
    element's border box (also outside the box)."""


@dataclass(frozen=True)
class Resource:
    """A text file that the local server serves next to the page of a case."""

    name: str
    text: str
    delay_ms: int = 0
    """How long the server waits before it answers."""
    content_type: str | None = None
    """The `Content-Type`; `None` takes it from the extension."""
    status: int = 200
    """The status of the answer to every method but `OPTIONS`."""
    headers: tuple[tuple[str, str], ...] = ()
    """Extra response headers, in order (`$ORIGIN` and `$OTHER` are
    replaced)."""
    options: Any = "none"
    """The answer to `OPTIONS`: `"none"`, `"deny"`, `"permissive"` or an
    object with `status`, `headers` and `max_age`."""


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
    extra = set(item) - {kind, "props", "points"}
    if extra:
        raise ProbeError(f"{where}: unknown key {sorted(extra)[0]!r} in a {kind} entry")
    target = item[kind]
    if not isinstance(target, str) or not target.strip():
        raise ProbeError(f"{where}: {kind} must be a non-empty string")
    if kind == "pixels":
        return _parse_pixels(item, target, where)
    if "points" in item:
        raise ProbeError(f"{where}: points belongs to pixels, not {kind}")
    if kind != "style":
        if "props" in item:
            raise ProbeError(f"{where}: props belongs to style, not {kind}")
        return Query(kind, target)
    props = item.get("props")
    if not isinstance(props, list) or not props or not all(isinstance(p, str) for p in props):
        raise ProbeError(f"{where}: style needs props, a non-empty list of property names")
    return Query(kind, target, tuple(props))


def _parse_pixels(item: dict[str, Any], target: str, where: str) -> Query:
    if "props" in item:
        raise ProbeError(f"{where}: props belongs to style, not pixels")
    points = item.get("points")
    ok = (
        isinstance(points, list)
        and bool(points)
        and all(
            isinstance(p, list)
            and len(p) == 2
            and all(isinstance(v, int) and not isinstance(v, bool) for v in p)
            for p in points
        )
    )
    if not ok:
        raise ProbeError(f"{where}: pixels needs points, a non-empty list of [x, y] integers")
    return Query("pixels", target, points=tuple((p[0], p[1]) for p in points))


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
    steps = parse_steps(item.get("steps", []), where)
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
    "viewport",
    "colorScheme",
    "back",
    "forward",
)
_POINT_STEPS = ("click", "dblclick", "rightclick", "middleclick", "hover")


def _is_point(argument: Any) -> bool:
    return (
        isinstance(argument, list)
        and len(argument) == 2
        and all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in argument)
    )


def parse_steps(value: Any, where: str) -> tuple[dict[str, Any], ...]:
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
        elif kind == "viewport":
            ok = _is_point(argument) and all(1 <= v <= 16384 for v in argument)
        elif kind == "colorScheme":
            ok = argument in ("light", "dark")
        elif kind in ("back", "forward"):
            ok = argument is True
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
        extra = set(item) - {"text", "delay_ms", "content_type", "status", "headers", "options"}
        if extra:
            raise ProbeError(f"{where}: unknown key {sorted(extra)[0]!r} in resource {name!r}")
        delay = item.get("delay_ms", 0)
        if not isinstance(delay, int) or isinstance(delay, bool) or not 0 <= delay <= 10_000:
            raise ProbeError(f"{where}: delay_ms of {name!r} must be 0 to 10000")
        kind = item.get("content_type")
        if kind is not None and (not isinstance(kind, str) or not kind):
            raise ProbeError(f"{where}: content_type of {name!r} must be a non-empty string")
        status = _parse_status(item.get("status", 200), f"{where}: status of {name!r}")
        headers = _parse_headers(item.get("headers", {}), f"{where}: headers of {name!r}")
        options = _parse_options(item.get("options", "none"), f"{where}: options of {name!r}")
        resources.append(Resource(name, item["text"], delay, kind, status, headers, options))
    return tuple(resources)


def _parse_status(value: Any, where: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or not 100 <= value <= 599:
        raise ProbeError(f"{where} must be 100 to 599")
    return value


def _parse_headers(value: Any, where: str) -> tuple[tuple[str, str], ...]:
    """Response headers: an object of name to a string or a list of strings."""
    if not isinstance(value, dict):
        raise ProbeError(f"{where} must be an object")
    pairs = []
    for name, item in value.items():
        values = [item] if isinstance(item, str) else item
        if not name or not isinstance(values, list) or not all(isinstance(v, str) for v in values):
            raise ProbeError(f"{where}: {name!r} must be a string or a list of strings")
        pairs.extend((name, v) for v in values)
    return tuple(pairs)


def _parse_options(value: Any, where: str) -> Any:
    """The answer to `OPTIONS`: a keyword or `{status, headers, max_age}`."""
    if value in ("none", "deny", "permissive"):
        return value
    if not isinstance(value, dict) or set(value) - {"status", "headers", "max_age"}:
        raise ProbeError(f"{where} must be none, deny, permissive or an object")
    result: dict[str, Any] = {}
    if "status" in value:
        result["status"] = _parse_status(value["status"], f"{where}: status")
    if "headers" in value:
        result["headers"] = _parse_headers(value["headers"], f"{where}: headers")
    if "max_age" in value:
        max_age = value["max_age"]
        if not isinstance(max_age, int) or isinstance(max_age, bool) or max_age < 0:
            raise ProbeError(f"{where}: max_age must be a non-negative integer")
        result["max_age"] = max_age
    return result


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
