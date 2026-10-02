"""`swbtools compare NAME`: run swb on a fixture and compare with Chromium.

Output goes to `out/compare/<name>/`: swb's box dump and screenshot, the
diff image, swb's log and `report.html`.
"""

import json
import logging
import math
import shutil
from pathlib import Path

from swbtools import paths, swb
from swbtools.boxes import read_dump
from swbtools.pages import (
    META_FILE,
    REFERENCE_BOXES,
    REFERENCE_DIR,
    REFERENCE_SCREENSHOT,
    read_meta,
)
from swbtools.pixels import compare_screenshots
from swbtools.report import write_report
from swbtools.scoring import Scores, compare

log = logging.getLogger(__name__)

SWB_BOXES = "boxes.json"
SWB_SCREENSHOT = "screenshot.png"
SWB_LOG = "swb.log"
REFERENCE_COPY = "reference.png"
DIFF_IMAGE = "diff.png"
REPORT = "report.html"


def output_dir(name: str) -> Path:
    """Returns `out/compare/<name>/`."""
    return paths.out_dir() / "compare" / name


def run_swb(name: str, swb_binary: Path, output: Path) -> bool:
    """Runs swb on the fixture. Returns True if it wrote a box dump."""
    fixture = paths.fixture_dir(name)
    meta = read_meta(fixture)
    for stale in (SWB_BOXES, SWB_SCREENSHOT):
        (output / stale).unlink(missing_ok=True)
    command = swb.replay_command(
        swb_binary,
        fixture,
        meta.url,
        meta.viewport,
        output / SWB_BOXES,
        output / SWB_SCREENSHOT,
    )
    status = swb.run(command, log_file=output / SWB_LOG)
    if status != 0:
        log.error("%s: swb exited with status %d; see %s", name, status, output / SWB_LOG)
    if not (output / SWB_BOXES).is_file():
        log.error("%s: swb wrote no box dump", name)
        return False
    return True


def compare_fixture(
    name: str, swb_binary: Path | None, tolerance: float, threshold: int
) -> Scores | None:
    """Compares one fixture. With `swb_binary=None`, uses the swb output that
    is already in the output directory. Returns None on failure."""
    fixture = paths.fixture_dir(name)
    reference_dir = fixture / REFERENCE_DIR
    if not (fixture / META_FILE).is_file():
        log.error("%s: %s does not exist; capture the fixture first", name, fixture / META_FILE)
        return None
    if not (reference_dir / REFERENCE_BOXES).is_file():
        log.error("%s: no reference; run `swbtools reference %s`", name, name)
        return None
    output = output_dir(name)
    output.mkdir(parents=True, exist_ok=True)
    if swb_binary is not None and not run_swb(name, swb_binary, output):
        return None
    if not (output / SWB_BOXES).is_file():
        log.error("%s: %s does not exist", name, output / SWB_BOXES)
        return None

    comparison = compare(
        read_dump(reference_dir / REFERENCE_BOXES), read_dump(output / SWB_BOXES), tolerance
    )
    images: dict[str, str] = {}
    pixels = None
    reference_png = reference_dir / REFERENCE_SCREENSHOT
    if reference_png.is_file():
        shutil.copyfile(reference_png, output / REFERENCE_COPY)
        images["Chromium"] = REFERENCE_COPY
    if (output / SWB_SCREENSHOT).is_file():
        images["swb"] = SWB_SCREENSHOT
        if reference_png.is_file():
            pixels = compare_screenshots(
                reference_png, output / SWB_SCREENSHOT, output / DIFF_IMAGE, threshold
            )
            images[f"Difference (channel delta > {threshold})"] = DIFF_IMAGE
    scores = comparison.scores(pixels)
    log_file = SWB_LOG if (output / SWB_LOG).is_file() else None
    write_report(output / REPORT, name, comparison, scores, images, log_file)
    print(summary_line(name, scores, comparison.alignment.identical, output / REPORT))
    return scores


def summary_line(name: str, scores: Scores, identical: bool, report: Path) -> str:
    """One line with the scores of a fixture."""
    pixels = "n/a" if scores.pixels is None else f"{scores.pixels:.4f}"
    tags = "" if identical else " (tag sequences differ)"
    return (
        f"{name}: geometry {scores.geometry:.4f} size {scores.size:.4f} "
        f"relative {scores.relative:.4f} pixels {pixels} "
        f"missing {scores.missing} extra {scores.extra}{tags}  {report}"
    )


def floor4(value: float) -> float:
    """Rounds down to 4 decimals. The stored score must never be higher than
    the exact value, or the Rust ratchet test would fail on the same build."""
    return round(math.floor(value * 10_000) / 10_000, 4)


def scores_entry(scores: Scores) -> dict[str, float]:
    """The `fixtures/scores.json` entry for one fixture."""
    return {
        "geometry": floor4(scores.geometry),
        "size": floor4(scores.size),
        "relative": floor4(scores.relative),
        "pixels": floor4(scores.pixels or 0.0),
    }


def format_scores(scores: dict[str, dict[str, float]]) -> str:
    """The text of `fixtures/scores.json`: sorted keys, 2-space indent."""
    return json.dumps(scores, indent=2, sort_keys=True) + "\n"


def update_scores(path: Path, results: dict[str, Scores]) -> None:
    """Writes the scores of the compared fixtures into the scores file. Other
    fixtures keep their entries. Logs every score that goes down."""
    stored: dict[str, dict[str, float]] = {}
    if path.is_file():
        stored = json.loads(path.read_text(encoding="utf-8"))
    for name, scores in results.items():
        if scores.pixels is None:
            log.warning("%s: no screenshots to compare; pixels score stored as 0", name)
        entry = scores_entry(scores)
        old = stored.get(name, {})
        for key, value in entry.items():
            if key in old and value < old[key]:
                log.warning("%s: %s goes down from %.4f to %.4f", name, key, old[key], value)
        stored[name] = entry
    path.write_text(format_scores(stored), encoding="utf-8")
    print(f"scores written to {path}")
