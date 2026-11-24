"""What the data tools of the exit tests share (`tools/osm_zagreb_records.py`,
`tools/videojs_records.py`): the choice of pixel points from Chromium's
screenshots, the console and missing-URL records, one swb run of step groups,
and the comparison of swb's records with Chromium's.

Nothing here knows a page. The point choice (`stable_flat`, `edge_mask`,
`scan`, `thin`, `spread`) works on the screenshots of the three Chromium runs
alone, never on swb's.
"""

import json
import os
import re
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

import numpy as np

from swbtools.records import ROOT

FLAT_RADIUS = 3
"""A point is chosen only where the (2 radius + 1) square around it is one
color in all runs."""
EDGE_CONTRAST = 60
EDGE_REACH = 10


def slug(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def screenshot_name(number: int, label: str) -> str:
    """The file name that `swbtools replay --screenshots` gives the
    screenshot after the group `label` (record number `number`)."""
    return f"{number:03d}-{slug(label)}.png"


def windowed(pixels: Any, radius: int, reduce: Any) -> Any:
    """`reduce` (`numpy.maximum` or `numpy.minimum`) over the square of
    (2 radius + 1) px around every pixel of the (height, width, channels)
    array; the edge pixels repeat."""
    padded = np.pad(pixels, ((radius, radius), (radius, radius), (0, 0)), mode="edge")
    height, width = pixels.shape[:2]
    rows = padded[0:height]
    for shift in range(1, 2 * radius + 1):
        rows = reduce(rows, padded[shift : shift + height])
    out = rows[:, 0:width]
    for shift in range(1, 2 * radius + 1):
        out = reduce(out, rows[:, shift : shift + width])
    return out


def flat_mask(pixels: Any, radius: int = FLAT_RADIUS) -> Any:
    """True where the square around the pixel has one color."""
    high, low = windowed(pixels, radius, np.maximum), windowed(pixels, radius, np.minimum)
    return (high == low).all(axis=2)


def edge_mask(pixels: Any) -> Any:
    """True where a pixel within `EDGE_REACH` differs from the others of the
    square by more than `EDGE_CONTRAST` in some channel."""
    high, low = windowed(pixels, EDGE_REACH, np.maximum), windowed(pixels, EDGE_REACH, np.minimum)
    return (high.astype("int16") - low.astype("int16") > EDGE_CONTRAST).any(axis=2)


def stable_flat(shots: list[Any]) -> Any:
    """The mask of the pixels that are flat in every shot and have the same
    color in all of them."""
    mask = np.ones(shots[0].shape[:2], dtype=bool)
    for shot in shots:
        mask &= flat_mask(shot)
        mask &= (shot == shots[0]).all(axis=2)
    return mask


def inside(x: int, y: int, boxes: list[tuple[int, int, int, int]]) -> bool:
    return any(a <= x < c and b <= y < d for a, b, c, d in boxes)


def thin(found: list[Any], count: int) -> list[Any]:
    """Up to `count` items of the list, evenly over it."""
    if len(found) <= count:
        return list(found)
    return [found[i * len(found) // count] for i in range(count)]


def spread(found: list[tuple[int, int]], distance: int) -> list[tuple[int, int]]:
    """The points, in the order given, each at least `distance` px (in x or
    y) from those taken before."""
    taken: list[tuple[int, int]] = []
    for x, y in found:
        if all(abs(x - a) >= distance or abs(y - b) >= distance for a, b in taken):
            taken.append((x, y))
    return taken


def scan(mask: Any, area: tuple[int, int, int, int], stride: int) -> list[tuple[int, int]]:
    """The points of `area` (scan order, `stride` apart) where `mask` is
    true."""
    return [
        (x, y)
        for y in range(area[1], area[3], stride)
        for x in range(area[0], area[2], stride)
        if mask[y, x]
    ]


def differences(first: list[Any], second: list[Any]) -> list[str]:
    """The (record, field) pairs in which two runs differ."""
    found = []
    for number, (a, b) in enumerate(zip(first, second, strict=False)):
        found += [f"record {number}: {key}" for key in sorted(a) if a[key] != b.get(key)]
    return found


def mask_missing(rows: list[dict[str, Any]], masked: tuple[str, ...] = ()) -> list[dict[str, Any]]:
    """The rows of `--missing-out` with each URL that starts with one of
    `masked` cut to that prefix (its query has the time and a random number,
    so that it is equal in every run), the URLs of a row sorted, without
    repeats."""

    def mask(url: str) -> str:
        return next((m for m in masked if url.startswith(m)), url)

    return [
        {"label": row["label"], "missing": sorted({mask(u) for u in row["missing"]})}
        for row in rows
    ]


CONSOLE_LINE = re.compile(r"^(?P<type>\S+)  (?P<url>.*?):(?P<line>\d+)  (?P<text>.*)$")


def page_console(text: str, masked: tuple[str, ...] = ()) -> list[dict[str, str]]:
    """The messages of a `--console-out` file at level error and warning and
    the uncaught exceptions, as `{"type", "url", "text"}`, in file order. A
    URL that starts with one of `masked` is cut to that prefix."""
    found = []
    for line in text.splitlines():
        parts = CONSOLE_LINE.match(line)
        if parts is None or parts["type"] not in ("error", "warning", "exception"):
            continue
        url, message = parts["url"], parts["text"]
        url = next((m for m in masked if url.startswith(m)), url)
        found.append({"type": parts["type"], "url": url, "text": message})
    return found


def console_counts(runs: list[list[dict[str, str]]]) -> list[dict[str, Any]]:
    """The console messages of the runs as `{"type", "url", "text", "min",
    "max"}`: how many times a message (type, URL and text) appeared in a run,
    the smallest and the largest number over the runs (0 if a run lacks it).
    Sorted by type, URL and text: the order of an exception and of a network
    log line differs from run to run."""
    keys = sorted({(m["type"], m["url"], m["text"]) for run in runs for m in run})
    found = []
    for key in keys:
        counts = [sum(1 for m in run if (m["type"], m["url"], m["text"]) == key) for run in runs]
        found.append(
            {"type": key[0], "url": key[1], "text": key[2], "min": min(counts), "max": max(counts)}
        )
    return found


# One swb run: the cap of the whole run, and the log levels (the `screenshot`
# step logs at `info` so that the log has a mark per group).
SWB_TIMEOUT_S = 1800
SWB_LOG_FILTER = "warn,swb_automation::steps=info"
SCREENSHOT_LINE = re.compile(r"screenshot \S*?(\d{3})-[^/\s]*\.png")


def parse_answers(lines: list[tuple[float, str]]) -> tuple[list[Any], list[float]]:
    """The records and the arrival times of the answer lines of `record`
    steps (`{"value": JSON TEXT}` or `{"error": MESSAGE}`)."""
    records, times = [], []
    for when, line in lines:
        try:
            answer = json.loads(line)
        except ValueError:
            continue
        if "value" in answer and isinstance(answer["value"], str):
            try:
                records.append(json.loads(answer["value"]))
            except ValueError:
                records.append({"error": "not JSON: " + answer["value"][:200]})
        elif "error" in answer:
            records.append({"error": answer["error"]})
        else:
            continue
        times.append(when)
    return records, times


def split_log(text: str) -> list[list[str]]:
    """The lines of the log, one list per record: the lines before the
    `screenshot` line of record N belong to record N."""
    groups: list[list[str]] = [[]]
    for line in text.splitlines():
        if SCREENSHOT_LINE.search(line):
            groups.append([])
        else:
            groups[-1].append(line)
    return groups


def flat_fields(value: Any, prefix: str = "", depth: int = 0) -> dict[str, Any]:
    """The fields of a record by dotted path, objects opened to two levels."""
    if isinstance(value, dict) and depth < 2:
        found: dict[str, Any] = {}
        for key, item in value.items():
            found.update(flat_fields(item, f"{prefix}{key}.", depth + 1))
        return found
    return {prefix.rstrip("."): value}


def field_differences(expected: Any, actual: Any) -> list[str]:
    """The paths of the fields in which two records differ."""
    a, b = flat_fields(expected), flat_fields(actual)
    return sorted(key for key in a.keys() | b.keys() if a.get(key) != b.get(key))


def run_swb_steps(
    command: list[str], log_path: Path, timeout: float = SWB_TIMEOUT_S
) -> tuple[list[tuple[float, str]], int | None]:
    """Runs swb, writes its stderr to `log_path`, and returns the lines of
    its standard output with their times (seconds from the start) and the
    exit status (None if the run hit the timeout and was stopped)."""
    env = {**os.environ, "RUST_LOG": SWB_LOG_FILTER}
    start = time.monotonic()
    lines: list[tuple[float, str]] = []
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=log, text=True
        )
        killer = threading.Timer(timeout, process.kill)
        killer.start()
        try:
            if process.stdout is not None:
                for line in process.stdout:
                    lines.append((time.monotonic() - start, line.rstrip("\n")))
            status = process.wait()
        finally:
            killer.cancel()
            process.kill()
            process.wait()
    return lines, (None if time.monotonic() - start >= timeout else status)


def labelled(names: list[str], key: str, values: list[Any]) -> list[dict[str, Any]]:
    """`[{"name": N, KEY: V}, ...]` for the pairs of names and values."""
    return [{"name": n, key: v} for n, v in zip(names, values, strict=False)]
