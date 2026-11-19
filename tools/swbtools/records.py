"""Helpers of the record scripts (`tools/github_swb_records.py`,
`tools/osm_zagreb_records.py`): the repository root, JSON files and one run of
`swbtools replay`."""

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]


def load_json(path: Path) -> Any:
    """The JSON value in the file `path`."""
    return json.loads(path.read_text(encoding="utf-8"))


def format_json(value: Any, level: int = 0) -> str:
    """JSON text with one space of indent per level. A list of numbers,
    strings and other scalars that fits on one line of 160 columns takes one
    line (rectangles, points, colors); other lists and objects take a line
    for each item."""
    pad = " " * (level + 1)
    if isinstance(value, dict):
        if not value:
            return "{}"
        items = [
            f"{pad}{json.dumps(key, ensure_ascii=False)}: {format_json(item, level + 1)}"
            for key, item in value.items()
        ]
        return "{\n" + ",\n".join(items) + "\n" + " " * level + "}"
    if isinstance(value, list):
        if not value:
            return "[]"
        flat = json.dumps(value, ensure_ascii=False)
        if not any(isinstance(item, (dict, list)) for item in value) and len(flat) <= 160:
            return flat
        items = [pad + format_json(item, level + 1) for item in value]
        return "[\n" + ",\n".join(items) + "\n" + " " * level + "]"
    return json.dumps(value, ensure_ascii=False)


def write_json(path: Path, value: Any, indent: int | None = None) -> None:
    """Writes `value` to `path`: with `indent` as `json.dumps` formats it,
    else as `format_json` does (short diffs)."""
    text = format_json(value) if indent is None else json.dumps(value, indent=indent)
    path.write_text(text + "\n", encoding="utf-8")


def run_replay(cmd: list[str], log: Path | None = None) -> str:
    """Runs a `swbtools replay` command from the repository root and returns
    its standard error (also written to `log`). Exits with the end of the
    error text if the command fails."""
    result = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=False)
    if log is not None:
        log.write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        where = f" (see {log})" if log is not None else ""
        sys.exit(f"replay failed{where}:\n{result.stderr[-2000:]}")
    return result.stderr
