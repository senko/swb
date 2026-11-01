"""The results of `swbtools probe` and their text and JSON output."""

import json
from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Any

from swbtools.boxes import Rect, round2

_SWB_LABEL = "swb"


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
class SwbPixels:
    """swb's pixels at the points of a `pixels` row, and whether they differ."""

    pixels: list[tuple[int, int, int]]
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
    swb_pixels: SwbPixels | None = None
    swb_value: SwbValue | None = None
    """swb's value of a `js` row in a JavaScript case."""


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
    if row.kind == "pixels":
        if not data.get("matched", True):
            return "no match"
        return format_pixels(data["pixels"])
    return json.dumps(data["value"], ensure_ascii=False)


def format_pixels(pixels: Sequence[Sequence[int]]) -> str:
    """`10,20,30 255,255,255`: the RGB values of the points."""
    return " ".join(",".join(str(c) for c in pixel) for pixel in pixels)


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
    swb_value = row.swb_value
    if swb_value is not None:
        text = swb_value.text + ("  !" if swb_value.outside else "")
        lines.append(f"{case}  {_SWB_LABEL:<5}  {row.label}  {text}")
    swb_pixels = row.swb_pixels
    if swb_pixels is not None:
        lines.append(
            f"{case}  {_SWB_LABEL:<5}  {row.label}  {format_pixels(swb_pixels.pixels)}"
            + ("  !" if swb_pixels.outside else "")
        )
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
            if row.swb_pixels is not None:
                item["swb_pixels"] = {
                    "pixels": row.swb_pixels.pixels,
                    "outside": row.swb_pixels.outside,
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
