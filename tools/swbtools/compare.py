"""`swbtools compare NAME`: run swb on a fixture and compare with Chromium.

Output goes to `out/compare/<name>/`: swb's box dump and screenshot, the
diff image, swb's log and `report.html`.

Two options extend the comparison (docs/testing.md, "Compare swb with
Chromium"):

- `--full-page` adds Chromium's and swb's full-page screenshots, a
  full-page pixel score and the list of differing regions.
- `--click SELECTOR` compares the page after clicks. Chromium's boxes and
  screenshots then come from a live capture, not from the committed
  reference. All file names of such a run end with the state name, so that
  the plain run's files stay.
"""

import asyncio
import hashlib
import json
import logging
import math
import re
import shutil
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths, swb
from swbtools.automation import AutomationError, Browser
from swbtools.boxes import BoxDump, read_dump
from swbtools.fullpage import FullPage, compare_full_page
from swbtools.live import LiveOutput, capture_live
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
from swbtools.swb_state import StateError, capture_state

log = logging.getLogger(__name__)

SWB_BOXES = "boxes.json"
SWB_SCREENSHOT = "screenshot.png"
SWB_FULL_PAGE = "fullpage.png"
SWB_LOG = "swb.log"
SWB_FULL_PAGE_LOG = "swb-fullpage.log"
REFERENCE_COPY = "reference.png"
REFERENCE_LIVE_BOXES = "reference-boxes.json"
REFERENCE_FULL_PAGE = "reference-fullpage.png"
DIFF_IMAGE = "diff.png"
DIFF_FULL_PAGE = "diff-fullpage.png"
REPORT = "report.html"


@dataclass(frozen=True)
class Options:
    """What `compare` does beyond the plain first-viewport comparison."""

    full_page: bool = False
    clicks: tuple[str, ...] = ()
    """Selectors to click, in order, in both browsers."""


def state_suffix(clicks: tuple[str, ...]) -> str:
    """The suffix of the output files of a state: empty for the plain run,
    else `-click-<slug>-<hash>` (the hash tells apart different selector
    lists with the same slug)."""
    if not clicks:
        return ""
    slug = re.sub(r"[^a-z0-9]+", "-", " ".join(clicks).lower()).strip("-")[:32].strip("-")
    digest = hashlib.sha256("\0".join(clicks).encode()).hexdigest()[:6]
    return f"-click-{slug}-{digest}" if slug else f"-click-{digest}"


def named(file: str, suffix: str) -> str:
    """Inserts the state suffix before the extension: `boxes.json` and
    `-click-a-1` give `boxes-click-a-1.json`."""
    stem, dot, extension = file.rpartition(".")
    return f"{stem}{suffix}{dot}{extension}"


def output_dir(name: str) -> Path:
    """Returns `out/compare/<name>/`."""
    return paths.out_dir() / "compare" / name


def run_swb(name: str, swb_binary: Path, output: Path, full_page: bool = False) -> bool:
    """Runs swb on the fixture. Returns True if it wrote a box dump. With
    `full_page`, a second run writes the full-page screenshot."""
    fixture = paths.fixture_dir(name)
    meta = read_meta(fixture)
    for stale in (SWB_BOXES, SWB_SCREENSHOT, SWB_FULL_PAGE):
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
    if full_page:
        command = swb.full_page_command(
            swb_binary, fixture, meta.url, meta.viewport, output / SWB_FULL_PAGE
        )
        if swb.run(command, log_file=output / SWB_FULL_PAGE_LOG) != 0:
            log.error("%s: swb failed to take the full-page screenshot", name)
            return False
    return True


def run_swb_state(name: str, swb_binary: Path, output: Path, options: Options, suffix: str) -> bool:
    """Runs swb through the automation API: load, click, then write the box
    dump and the screenshots of the state. Returns True on success."""
    fixture = paths.fixture_dir(name)
    meta = read_meta(fixture)
    files = [named(f, suffix) for f in (SWB_BOXES, SWB_SCREENSHOT, SWB_FULL_PAGE)]
    for stale in files:
        (output / stale).unlink(missing_ok=True)
    try:
        log_file = output / named(SWB_LOG, suffix)
        with Browser.start(swb_binary, fixture, meta.viewport, log_file=log_file) as browser:
            capture_state(
                browser,
                meta.url,
                list(options.clicks),
                output / files[0],
                output / files[1],
                output / files[2] if options.full_page else None,
            )
    except (StateError, AutomationError, OSError, RuntimeError) as error:
        log.error(
            "%s: swb could not reach the state: %s; see %s",
            name,
            error,
            output / named(SWB_LOG, suffix),
        )
        return False
    return True


def run_chromium_state(name: str, output: Path, options: Options, suffix: str) -> bool:
    """Captures the live Chromium output of a run: the boxes, the viewport
    screenshot of a state and the full-page screenshot. Returns True on
    success."""
    fixture = paths.fixture_dir(name)
    meta = read_meta(fixture)
    live = LiveOutput(
        boxes=output / named(REFERENCE_LIVE_BOXES, suffix),
        viewport_png=output / named(REFERENCE_COPY, suffix) if options.clicks else None,
        full_page_png=output / named(REFERENCE_FULL_PAGE, suffix) if options.full_page else None,
    )
    try:
        result = asyncio.run(
            capture_live(fixture, meta.url, meta.viewport, list(options.clicks), live)
        )
    except Exception as error:
        log.error("%s: Chromium could not reach the state: %s", name, error)
        return False
    for url in result.missing:
        log.warning("%s: not in fixture: %s", name, url)
    return True


def compare_fixture(
    name: str,
    swb_binary: Path | None,
    tolerance: float,
    threshold: int,
    options: Options | None = None,
) -> Scores | None:
    """Compares one fixture. With `swb_binary=None`, uses the output that is
    already in the output directory (swb's and, for `--full-page` and
    `--click`, Chromium's). Returns None on failure."""
    fixture = paths.fixture_dir(name)
    reference_dir = fixture / REFERENCE_DIR
    options = options or Options()
    suffix = state_suffix(options.clicks)
    if not (fixture / META_FILE).is_file():
        log.error("%s: %s does not exist; capture the fixture first", name, fixture / META_FILE)
        return None
    # A state is compared with a live capture; only the plain run needs the
    # committed reference.
    if not options.clicks and not (reference_dir / REFERENCE_BOXES).is_file():
        log.error("%s: no reference; run `swbtools reference %s`", name, name)
        return None
    output = output_dir(name)
    output.mkdir(parents=True, exist_ok=True)
    if swb_binary is not None and not run_outputs(name, swb_binary, output, options, suffix):
        return None
    swb_boxes = output / named(SWB_BOXES, suffix)
    if not swb_boxes.is_file():
        log.error("%s: %s does not exist", name, swb_boxes)
        return None

    if options.clicks:
        reference_boxes = output / named(REFERENCE_LIVE_BOXES, suffix)
        reference_png = output / named(REFERENCE_COPY, suffix)
    else:
        reference_boxes = reference_dir / REFERENCE_BOXES
        reference_png = reference_dir / REFERENCE_SCREENSHOT
    if not reference_boxes.is_file():
        log.error("%s: %s does not exist", name, reference_boxes)
        return None
    reference_dump = read_dump(reference_boxes)
    comparison = compare(reference_dump, read_dump(swb_boxes), tolerance)

    images: dict[str, str] = {}
    pixels = None
    swb_png = output / named(SWB_SCREENSHOT, suffix)
    diff_png = named(DIFF_IMAGE, suffix)
    if reference_png.is_file():
        if not options.clicks:
            shutil.copyfile(reference_png, output / REFERENCE_COPY)
        images["Chromium"] = reference_png.name if options.clicks else REFERENCE_COPY
    if swb_png.is_file():
        images["swb"] = swb_png.name
        if reference_png.is_file():
            pixels = compare_screenshots(reference_png, swb_png, output / diff_png, threshold)
            images[f"Difference (channel delta > {threshold})"] = diff_png
    full_page = None
    full_images: dict[str, str] = {}
    if options.full_page:
        full_page = full_page_result(name, output, suffix, threshold, reference_dump)
        if full_page is None:
            return None
        full_images = {
            "Chromium": named(REFERENCE_FULL_PAGE, suffix),
            "swb": named(SWB_FULL_PAGE, suffix),
            f"Difference (channel delta > {threshold})": named(DIFF_FULL_PAGE, suffix),
        }
    scores = comparison.scores(pixels)
    log_name = named(SWB_LOG, suffix)
    report = output / named(REPORT, suffix)
    write_report(
        report,
        name,
        comparison,
        scores,
        images,
        log_name if (output / log_name).is_file() else None,
        full_page=full_page,
        full_page_images=full_images,
        clicks=options.clicks,
    )
    print(summary_line(name, scores, comparison.alignment.identical, report, full_page))
    return scores


def run_outputs(name: str, swb_binary: Path, output: Path, options: Options, suffix: str) -> bool:
    """Runs swb and, if needed, Chromium. Returns True on success."""
    if options.clicks:
        return run_swb_state(name, swb_binary, output, options, suffix) and run_chromium_state(
            name, output, options, suffix
        )
    if not run_swb(name, swb_binary, output, options.full_page):
        return False
    return not options.full_page or run_chromium_state(name, output, options, suffix)


def full_page_result(
    name: str, output: Path, suffix: str, threshold: int, reference_dump: BoxDump
) -> FullPage | None:
    """Compares the full-page screenshots in `output`. Returns None if one
    is missing."""
    reference_png = output / named(REFERENCE_FULL_PAGE, suffix)
    swb_png = output / named(SWB_FULL_PAGE, suffix)
    for path in (reference_png, swb_png):
        if not path.is_file():
            log.error("%s: %s does not exist", name, path)
            return None
    return compare_full_page(
        reference_png, swb_png, output / named(DIFF_FULL_PAGE, suffix), threshold, reference_dump
    )


def summary_line(
    name: str,
    scores: Scores,
    identical: bool,
    report: Path,
    full_page: FullPage | None = None,
) -> str:
    """One line with the scores of a fixture. With `full_page`, it adds the
    full-page score, the number of differing regions and, if the sizes
    differ, both sizes."""
    pixels = "n/a" if scores.pixels is None else f"{scores.pixels:.4f}"
    tags = "" if identical else " (tag sequences differ)"
    full = ""
    if full_page is not None:
        full = f" fullpage {full_page.score:.4f} regions {len(full_page.regions)}"
        if full_page.sizes_differ:
            ref, other = full_page.reference_size, full_page.swb_size
            full += f" (size Chromium {ref[0]}x{ref[1]} swb {other[0]}x{other[1]})"
    return (
        f"{name}: geometry {scores.geometry:.4f} size {scores.size:.4f} "
        f"relative {scores.relative:.4f} pixels {pixels} "
        f"missing {scores.missing} extra {scores.extra}{full}{tags}  {report}"
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
