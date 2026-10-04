"""The box dump format: the border box of every element.

Chromium (through `COLLECT_JS`) and swb (`--dump-boxes FILE`) write this
format. The comparison reads both. The specification is in docs/testing.md.

```json
{
  "url": "https://senko.net/",
  "viewport": [1280, 800],
  "elements": [
    {"tag": "html", "rect": [0.0, 0.0, 1280.0, 2100.5], "parent": null},
    {"tag": "head", "rect": null, "parent": 0}
  ]
}
```
"""

import json
import math
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

type Rect = tuple[float, float, float, float]
"""x, y, width, height in CSS px, in document coordinates."""


@dataclass(frozen=True)
class Element:
    """One element of a box dump."""

    tag: str
    """Lowercase local name."""
    rect: Rect | None
    """The union of the element's border boxes, or None if it has no box."""
    parent: int | None = None
    """Index of the parent element, or None for the root or if unknown."""


@dataclass(frozen=True)
class BoxDump:
    """All elements of a document in tree order, with their boxes."""

    url: str
    viewport: tuple[float, float]
    elements: list[Element]

    @property
    def has_parents(self) -> bool:
        """True if the dump has parent indices (Chromium dumps have them)."""
        return any(element.parent is not None for element in self.elements)


def round2(value: float) -> float:
    """Rounds to 2 decimals, halves away from zero (as Rust's `f64::round`)."""
    scaled = abs(value) * 100.0
    whole = math.floor(scaled)
    if scaled - whole >= 0.5:
        whole += 1
    rounded = whole / 100.0
    return math.copysign(rounded, value) if rounded != 0.0 else 0.0


def union(rects: Iterable[Sequence[float]]) -> Rect | None:
    """Returns the bounding box of the rectangles, rounded to 2 decimals, or
    None if there are none. Empty rectangles count: an empty inline element
    still has a position."""
    left = top = math.inf
    right = bottom = -math.inf
    for x, y, width, height in rects:
        left = min(left, x)
        top = min(top, y)
        right = max(right, x + width)
        bottom = max(bottom, y + height)
    if left == math.inf:
        return None
    return (round2(left), round2(top), round2(right - left), round2(bottom - top))


def _element_json(element: Element) -> str:
    rect = None if element.rect is None else [float(value) for value in element.rect]
    data = {"tag": element.tag, "rect": rect, "parent": element.parent}
    return json.dumps(data, ensure_ascii=False)


def format_dump(dump: BoxDump) -> str:
    """Returns the JSON text: one element per line, so that diffs stay small."""
    lines = [
        "{",
        f'  "url": {json.dumps(dump.url, ensure_ascii=False)},',
        f'  "viewport": {json.dumps(list(dump.viewport))},',
    ]
    if dump.elements:
        lines.append('  "elements": [')
        body = [f"    {_element_json(element)}" for element in dump.elements]
        lines.append(",\n".join(body))
        lines.append("  ]")
    else:
        lines.append('  "elements": []')
    lines.append("}")
    return "\n".join(lines) + "\n"


def _parse_rect(value: Any) -> Rect | None:
    if value is None:
        return None
    if not isinstance(value, list) or len(value) != 4:
        raise ValueError(f"invalid rect {value!r}")
    x, y, width, height = (float(v) for v in value)
    return (x, y, width, height)


def parse_dump(text: str) -> BoxDump:
    """Parses a box dump. Lowercases tags again, so that `clipPath` and
    `clippath` are equal also if a writer did not lowercase them. `parent`
    is optional."""
    data = json.loads(text)
    elements = [
        Element(
            tag=str(item["tag"]).lower(),
            rect=_parse_rect(item.get("rect")),
            parent=item.get("parent"),
        )
        for item in data["elements"]
    ]
    width, height = data.get("viewport", [0, 0])
    return BoxDump(url=str(data.get("url", "")), viewport=(width, height), elements=elements)


def read_dump(path: Path) -> BoxDump:
    """Reads a box dump file."""
    return parse_dump(path.read_text(encoding="utf-8"))


def write_dump(path: Path, dump: BoxDump) -> None:
    """Writes a box dump file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(format_dump(dump), encoding="utf-8")


# Runs in the page (Playwright evaluates it even with JavaScript disabled).
# Returns the raw client rects; `from_page_data` computes the union in Python.
COLLECT_JS = """
() => {
  const elements = Array.from(document.querySelectorAll('*'));
  const index = new Map(elements.map((element, i) => [element, i]));
  const sx = window.scrollX;
  const sy = window.scrollY;
  return {
    viewport: [window.innerWidth, window.innerHeight],
    elements: elements.map((element) => ({
      tag: element.localName,
      parent: element.parentElement ? (index.get(element.parentElement) ?? null) : null,
      rects: Array.from(element.getClientRects(), (r) => [r.x + sx, r.y + sy, r.width, r.height]),
    })),
  };
}
"""


def from_page_data(url: str, data: dict[str, Any]) -> BoxDump:
    """Builds a box dump from the result of `COLLECT_JS`."""
    width, height = data["viewport"]
    elements = [
        Element(tag=str(item["tag"]).lower(), rect=union(item["rects"]), parent=item["parent"])
        for item in data["elements"]
    ]
    return BoxDump(url=url, viewport=(width, height), elements=elements)
