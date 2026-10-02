"""`swbtools perf`: times each pipeline stage of swb for page fixtures.

For each fixture, `swb --bench N` loads the page from the fixture with the
test fonts and runs every stage N times (docs/performance.md). This prints
the median times as a Markdown table.
"""

import json
import logging
import subprocess
from pathlib import Path

from swbtools import paths, swb
from swbtools.pages import read_meta

log = logging.getLogger(__name__)

STAGES = ["parse", "stylesheets", "style", "layout", "display_list", "raster"]
"""The stages in pipeline order (the keys of swb's output)."""


def bench_fixture(binary: Path, name: str, runs: int) -> dict | None:
    """Runs `swb --bench` for a fixture and returns its JSON output, or None
    if swb failed."""
    directory = paths.fixture_dir(name)
    meta = read_meta(directory)
    command = swb.bench_command(binary, directory, meta.url, meta.viewport, runs)
    log.info("running %s", " ".join(command))
    result = subprocess.run(
        command, capture_output=True, text=True, timeout=swb.SWB_TIMEOUT_S, check=False
    )
    if result.returncode != 0:
        log.error("%s: swb exited with status %d: %s", name, result.returncode, result.stderr)
        return None
    return json.loads(result.stdout)


def table(results: dict[str, dict]) -> str:
    """The median times in ms as a Markdown table, one row per fixture."""
    header = ["Fixture", "Elements", *(s.replace("_", " ") for s in STAGES), "Total"]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for name, result in results.items():
        medians = [result["stages"][stage]["median_ms"] for stage in STAGES]
        cells = [name, str(result["elements"]), *(f"{m:.2f}" for m in medians)]
        cells.append(f"{sum(medians):.2f}")
        lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines)


def perf(names: list[str], swb_path: Path | None, runs: int) -> int:
    """Benchmarks the fixtures and prints the table."""
    binary = swb.find_swb(swb_path)
    if binary is None:
        log.error("swb binary not found; build it (`just build`) or pass --swb PATH")
        return 1
    if "debug" in binary.parts:
        log.warning("%s is a debug build; times are not representative", binary)
    results = {}
    for name in names:
        result = bench_fixture(binary, name, runs)
        if result is None:
            return 1
        results[name] = result
    print(table(results))
    return 0
