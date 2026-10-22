"""Page fixtures: metadata (`fixture.json`) and the list of fixtures."""

import json
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths
from swbtools.manifest import MANIFEST_FILE, META_FILE, REPLAY_PATTERNS_KEY, replay_patterns_of

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
    javascript: bool = False
    """True for a fixture in `fixtures/js/`: captured with JavaScript on."""
    replay_patterns: tuple[str, ...] = ()
    """URL patterns for requests that the fixture lacks (`Fixture.lookup`)."""


def read_meta(fixture: Path) -> FixtureMeta:
    """Reads `fixture.json`."""
    path = fixture / META_FILE
    data = json.loads(path.read_text(encoding="utf-8"))
    width, height = data["viewport"]
    return FixtureMeta(
        url=data["url"],
        captured=data["captured"],
        viewport=(int(width), int(height)),
        chromium=data.get("chromium", ""),
        javascript=bool(data.get("javascript", False)),
        replay_patterns=replay_patterns_of(data, path),
    )


def write_meta(fixture: Path, meta: FixtureMeta) -> None:
    """Writes `fixture.json`."""
    data = {
        "url": meta.url,
        "captured": meta.captured,
        "viewport": list(meta.viewport),
        "chromium": meta.chromium,
    }
    if meta.javascript:
        data["javascript"] = True
    if meta.replay_patterns:
        data[REPLAY_PATTERNS_KEY] = list(meta.replay_patterns)
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    (fixture / META_FILE).write_text(text, encoding="utf-8")


def _list_in(directory: Path) -> list[str]:
    if not directory.is_dir():
        return []
    return sorted(child.name for child in directory.iterdir() if (child / MANIFEST_FILE).is_file())


def list_fixtures() -> list[str]:
    """Returns the names of all page fixtures (directories with a manifest)."""
    return _list_in(paths.pages_dir())


def list_js_fixtures() -> list[str]:
    """Returns the names of all JavaScript fixtures in `fixtures/js/`."""
    return _list_in(paths.js_fixtures_dir())
