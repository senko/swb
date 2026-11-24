"""`swbtools perf`: times each pipeline stage and the interaction of swb for
page fixtures.

For each fixture, `swb --bench N` loads the page from the fixture with the
test fonts, runs every stage N times, then moves the pointer over a grid
(hover) and turns the wheel (scroll) (docs/performance.md). This prints the
median times of the stages as a Markdown table, and the interaction numbers
as a second table. With `--js`, the fixtures are those of `fixtures/js/`,
loaded with JavaScript on and the fixture's clock (as `compare --js`).
"""

import json
import logging
import subprocess
from pathlib import Path

from swbtools import paths, swb
from swbtools.compare import swb_js_options
from swbtools.pages import read_meta

log = logging.getLogger(__name__)

STAGES = ["parse", "stylesheets", "style", "layout", "display_list", "raster"]
"""The stages in pipeline order (the keys of swb's output)."""


def bench_fixture(binary: Path, name: str, runs: int, javascript: bool = False) -> dict | None:
    """Runs `swb --bench` for a fixture (of `fixtures/js/` with `javascript`)
    and returns its JSON output, or None if swb failed."""
    directory = paths.fixture_directory(name, javascript)
    meta = read_meta(directory)
    extra = swb_js_options(javascript, meta.captured)
    command = swb.bench_command(binary, directory, meta.url, meta.viewport, runs, extra)
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


INTERACTION_HEADER = [
    "Fixture",
    "Hover steps",
    "changed",
    "change median ms",
    "change p90 ms",
    "all median ms",
    "all p90 ms",
    "style runs/change",
    "layout runs/change",
    "paint-only changes",
    "Scroll steps",
    "median ms",
    "p90 ms",
    "style runs",
    "layout runs",
]


def _number(value: float | None, digits: int = 2) -> str:
    """A number for a table cell; `-` for none (no hover change)."""
    return "-" if value is None else f"{value:.{digits}f}"


def interaction_table(results: dict[str, dict]) -> str:
    """The interaction numbers (hover and scroll) as a Markdown table, one
    row per fixture. Results without the `interaction` key are skipped."""
    header = INTERACTION_HEADER
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for name, result in results.items():
        interaction = result.get("interaction")
        if interaction is None:
            continue
        hover, scroll = interaction["hover"], interaction["scroll"]
        cells = [
            name,
            str(hover["steps"]),
            str(hover["changed"]),
            _number(hover["changed_median_ms"]),
            _number(hover["changed_p90_ms"]),
            _number(hover["median_ms"]),
            _number(hover["p90_ms"]),
            _number(hover["style_runs_per_change"], 1),
            _number(hover["layout_runs_per_change"], 1),
            str(hover.get("paint_only_changes", "-")),
            str(scroll["steps"]),
            _number(scroll["median_ms"]),
            _number(scroll["p90_ms"]),
            str(scroll["style_runs"]),
            str(scroll["layout_runs"]),
        ]
        lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines)


RESTYLE_PATHS = [
    "targeted",
    "nothing",
    "restyled_elements",
    "full_other_states",
    "full_everything",
    "full_root_font_size",
    "full_containers",
    "full_counters",
    "full_too_large",
    "full_structure",
]
"""The keys of `interaction.hover.state_restyles` (ADR 0041 and 0042)."""


def restyle_table(results: dict[str, dict]) -> str | None:
    """The paths of the restyles of the hover steps (targeted, nothing to
    restyle, the restyled elements, the full restyles by reason) as a
    Markdown table; None if no result has them (an older swb)."""
    rows = {
        name: result["interaction"]["hover"]["state_restyles"]
        for name, result in results.items()
        if "state_restyles" in result.get("interaction", {}).get("hover", {})
    }
    if not rows:
        return None
    header = ["Fixture", *(k.replace("_", " ") for k in RESTYLE_PATHS)]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for name, paths_ in rows.items():
        counts = (str(paths_.get(k, 0)) for k in RESTYLE_PATHS)
        lines.append("| " + " | ".join([name, *counts]) + " |")
    return "\n".join(lines)


def perf(names: list[str], swb_path: Path | None, runs: int, javascript: bool = False) -> int:
    """Benchmarks the fixtures and prints the tables."""
    binary = swb.find_swb(swb_path)
    if binary is None:
        log.error("swb binary not found; build it (`just build`) or pass --swb PATH")
        return 1
    if "debug" in binary.parts:
        log.warning("%s is a debug build; times are not representative", binary)
    results = {}
    for name in names:
        result = bench_fixture(binary, name, runs, javascript)
        if result is None:
            return 1
        results[name] = result
    print(table(results))
    print()
    print(interaction_table(results))
    restyles = restyle_table(results)
    if restyles is not None:
        print()
        print(restyles)
    return 0
