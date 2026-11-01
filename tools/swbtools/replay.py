"""`swbtools replay NAME`: Chromium records of a JavaScript fixture after steps.

Replays `fixtures/js/NAME` in Chromium with the setup of `reference --js`
(every request answered from the fixture, JavaScript on, the page clock
starting at the `captured` time of the fixture, the settle of
`load_settled_js`; the global options `--no-shared-worker` and `--no-intl`
apply). Then it runs a setup script, evaluates the record expression, and
for each step group runs the steps and evaluates the expression again. The
records are the values of the expression, in a JSON list: one after the load
and one after each group. The integration tests of the M8 targets compare
swb's records of the same steps with this file
(`crates/automation/tests/data/*-chromium.json`, docs/testing.md,
"Automation tests").

The steps are those of `swb --steps` and of the probe tool's `steps` key
(`probe.run_steps` sends them to Chromium; there is one implementation of
the step format in Python).

Pixels (M8 feature 12c): `--screenshots DIR` saves a screenshot of the
viewport after the load and after each group (`NNN-NAME.png`, for looking at
the page). `--pixels FILE` names points and the groups after which to read
them, `{"groups": ["theme night"], "points": {"wordmark": [x, y]}}` (`"load"`
is the state after the load); `--pixels-out FILE` then gets
`{"theme night": {"wordmark": [r, g, b]}}`, the colors of the viewport
screenshot at the points. The numbers go into the tests of the targets, so
that no image is committed.
"""

import asyncio
import io
import json
import logging
import re
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

from PIL import Image
from playwright.async_api import Page

from swbtools import browser, paths
from swbtools.manifest import Fixture
from swbtools.pages import read_meta
from swbtools.probe import run_steps
from swbtools.probe_cases import ProbeError, parse_steps
from swbtools.reference import load_settled_js, parse_captured
from swbtools.routing import Replayer

log = logging.getLogger(__name__)

DEFAULT_PAUSE_MS = 300
"""Pause after each step. The records of the Craigslist test were made with
it (docs/testing.md): the page's own handlers and timers run before the
next input, as they do while swb's `--steps` runs the due tasks."""

REPLAY_TIMEOUT_S = 600
"""The limit of one replay. Without it a step that waits on a page that never
answers would block the command (`page.evaluate` has no timeout)."""


def read_groups(path: Path) -> list[tuple[dict[str, Any], ...]]:
    """Reads a steps file: a JSON list of step groups. A group is a list of
    steps, or an object `{"name": ..., "steps": [...]}` (the name is for
    people; a record is written for each group)."""
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, list):
        raise ProbeError(f"{path}: the steps file is a list of groups")
    groups = []
    for number, item in enumerate(data, 1):
        steps = item.get("steps") if isinstance(item, dict) else item
        groups.append(parse_steps(steps, f"{path}, group {number}"))
    return groups


def read_group_names(path: Path) -> list[str]:
    """The names of the groups of a steps file (`group N` for a group
    without a name), in order."""
    data = json.loads(path.read_text(encoding="utf-8"))
    names = []
    for number, item in enumerate(data, 1):
        name = item.get("name") if isinstance(item, dict) else None
        names.append(name if isinstance(name, str) and name else f"group {number}")
    return names


Observer = Callable[[str, Page], Awaitable[None]]
"""Called with the label (`load` or the name of a group) and the page after
each record."""


def read_pixel_points(path: Path) -> tuple[list[str], dict[str, tuple[int, int]]]:
    """Reads a `--pixels` file: the labels to sample after and the named
    points (x, y in viewport px)."""
    data = json.loads(path.read_text(encoding="utf-8"))
    groups = data.get("groups")
    points = data.get("points")
    if not (isinstance(groups, list) and all(isinstance(g, str) for g in groups)):
        raise ProbeError(f"{path}: groups must be a list of group names")
    ok = isinstance(points, dict) and all(
        isinstance(p, list) and len(p) == 2 and all(isinstance(v, int) for v in p)
        for p in points.values()
    )
    if not ok:
        raise ProbeError(f"{path}: points must map names to [x, y] integers")
    return groups, {name: (p[0], p[1]) for name, p in points.items()}


def sample_pixels(png: bytes, points: dict[str, tuple[int, int]]) -> dict[str, list[int]]:
    """The color of the screenshot `png` at each named point; a point
    outside the image is an error."""
    with Image.open(io.BytesIO(png)) as image:
        rgb = image.convert("RGB")
        out = {}
        for name, (x, y) in points.items():
            if not (0 <= x < rgb.width and 0 <= y < rgb.height):
                raise ProbeError(f"point {name!r} ({x}, {y}) is outside the {rgb.size} screenshot")
            out[name] = list(rgb.getpixel((x, y)))  # type: ignore[arg-type]
        return out


def pixel_observer(
    names: list[str],
    screenshots: Path | None,
    wanted: list[str],
    points: dict[str, tuple[int, int]],
    samples: dict[str, dict[str, list[int]]],
) -> Observer:
    """An observer that saves screenshots into `screenshots` and reads the
    `points` after the groups in `wanted` into `samples`. `names` are the
    group names in order."""
    labels = ["load", *names]
    count = 0

    async def observe(label: str, page: Page) -> None:
        nonlocal count
        index = count
        count += 1
        if screenshots is None and label not in wanted:
            return
        png = await page.screenshot(animations="disabled", caret="hide")
        if screenshots is not None:
            screenshots.mkdir(parents=True, exist_ok=True)
            slug = re.sub(r"[^a-z0-9]+", "-", labels[index].lower()).strip("-")
            (screenshots / f"{index:03d}-{slug}.png").write_bytes(png)
        if label in wanted:
            samples[label] = sample_pixels(png, points)

    return observe


async def evaluate_record(page: Page, expression: str) -> Any:
    """Evaluates the record expression. A string value is the JSON text of
    the record (as `window.__s()` of the tests returns it) and is parsed."""
    value = await page.evaluate(expression)
    return json.loads(value) if isinstance(value, str) else value


async def replay(
    name: str,
    setup: str,
    groups: list[tuple[dict[str, Any], ...]],
    record: str,
    pause_ms: int = DEFAULT_PAUSE_MS,
    system_fonts: bool = False,
    observer: Observer | None = None,
    names: list[str] | None = None,
) -> tuple[list[Any], list[str], list[str]]:
    """Replays the fixture `name` and returns the records, the requested
    URLs that the fixture does not have, and the console errors and
    uncaught exceptions of the page. `observer` is called after each
    record with the label: `load`, or the name of the group from `names`
    (`group N` without names)."""
    directory = paths.js_fixture_dir(name)
    meta = read_meta(directory)
    replayer = Replayer(Fixture(directory))
    errors: list[str] = []
    records: list[Any] = []
    async with browser.session(
        meta.viewport, system_fonts, javascript=True, start_time=parse_captured(meta.captured)
    ) as (_, context, page):
        await replayer.attach(context, page)
        page.on(
            "console",
            lambda message: (
                errors.append(f"console.{message.type}: {message.text}")
                if message.type == "error"
                and not message.text.startswith("Failed to load resource")
                else None
            ),
        )
        page.on("pageerror", lambda error: errors.append(f"exception: {error}"))
        await load_settled_js(page, meta.url)
        if setup:
            await page.evaluate(setup)
        records.append(await evaluate_record(page, record))
        if observer is not None:
            await observer("load", page)
        for number, steps in enumerate(groups, 1):
            await run_steps(page, steps, pause_ms)
            records.append(await evaluate_record(page, record))
            if observer is not None:
                await observer(names[number - 1] if names else f"group {number}", page)
    return records, sorted(set(replayer.missing)), errors


def replay_command(
    name: str,
    setup: Path | None,
    steps: Path,
    record: str,
    out: Path | None,
    pause_ms: int,
    system_fonts: bool,
    screenshots: Path | None = None,
    pixels: Path | None = None,
    pixels_out: Path | None = None,
) -> int:
    """Runs `replay` and writes the records (JSON, indent 1) to `out` or
    stdout. Logs the missing URLs and the page's errors. With `pixels` and
    `pixels_out` it also writes the colors at the points; `screenshots`
    saves the screenshots (see the module comment)."""
    groups = read_groups(steps)
    names = read_group_names(steps)
    setup_text = setup.read_text(encoding="utf-8") if setup else ""
    if (pixels is None) != (pixels_out is None):
        log.error("--pixels and --pixels-out go together")
        return 2
    samples: dict[str, dict[str, list[int]]] = {}
    observer = None
    if pixels is not None or screenshots is not None:
        wanted, points = read_pixel_points(pixels) if pixels is not None else ([], {})
        unknown = [w for w in wanted if w != "load" and w not in names]
        if unknown:
            log.error("--pixels: no group named %r in %s", unknown[0], steps)
            return 2
        # The samples are keyed by the name: a repeated name (or a group
        # named "load") would overwrite another sample.
        repeated = [w for w in dict.fromkeys(wanted) if ["load", *names].count(w) > 1]
        if repeated:
            log.error("--pixels: %r names more than one group in %s", repeated[0], steps)
            return 2
        observer = pixel_observer(names, screenshots, wanted, points, samples)
    try:
        records, missing, errors = asyncio.run(
            asyncio.wait_for(
                replay(name, setup_text, groups, record, pause_ms, system_fonts, observer, names),
                REPLAY_TIMEOUT_S,
            )
        )
    except TimeoutError:
        log.error("%s: the replay took more than %d s", name, REPLAY_TIMEOUT_S)
        return 1
    for url in missing:
        log.warning("%s: not in fixture: %s", name, url)
    for error in errors:
        log.warning("%s: %s", name, error)
    if pixels_out is not None:
        pixels_out.parent.mkdir(parents=True, exist_ok=True)
        pixels_out.write_text(json.dumps(samples, indent=1) + "\n", encoding="utf-8")
    text = json.dumps(records, indent=1)
    if out is None:
        print(text)
    else:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text, encoding="utf-8")
        print(
            f"{name}: {len(records)} records written to {out} "
            f"({len(missing)} URLs not in fixture, {len(errors)} errors)"
        )
    return 0
