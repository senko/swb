"""Page fixtures: metadata (`fixture.json`) and the list of fixtures."""

import json
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths
from swbtools.manifest import MANIFEST_FILE

META_FILE = "fixture.json"
REFERENCE_DIR = "reference"
REFERENCE_BOXES = "boxes.json"
REFERENCE_SCREENSHOT = "screenshot.png"


@dataclass(frozen=True)
class FixtureMeta:
    """The content of `fixture.json`."""

    url: str
    """The URL that was captured, as the browser serialized it."""
    captured: str
    """Capture time, UTC, ISO 8601."""
    viewport: tuple[int, int]
    chromium: str
    """Chromium version used for the capture."""


def read_meta(fixture: Path) -> FixtureMeta:
    """Reads `fixture.json`."""
    data = json.loads((fixture / META_FILE).read_text(encoding="utf-8"))
    width, height = data["viewport"]
    return FixtureMeta(
        url=data["url"],
        captured=data["captured"],
        viewport=(int(width), int(height)),
        chromium=data.get("chromium", ""),
    )


def write_meta(fixture: Path, meta: FixtureMeta) -> None:
    """Writes `fixture.json`."""
    data = {
        "url": meta.url,
        "captured": meta.captured,
        "viewport": list(meta.viewport),
        "chromium": meta.chromium,
    }
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    (fixture / META_FILE).write_text(text, encoding="utf-8")


def list_fixtures() -> list[str]:
    """Returns the names of all page fixtures (directories with a manifest)."""
    pages = paths.pages_dir()
    if not pages.is_dir():
        return []
    return sorted(child.name for child in pages.iterdir() if (child / MANIFEST_FILE).is_file())
