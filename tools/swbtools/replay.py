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
"""

import asyncio
import json
import logging
from pathlib import Path
from typing import Any

from playwright.async_api import Page

from swbtools import browser, paths
from swbtools.manifest import Fixture
from swbtools.pages import read_meta
from swbtools.probe import ProbeError, parse_steps, run_steps
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
) -> tuple[list[Any], list[str], list[str]]:
    """Replays the fixture `name` and returns the records, the requested
    URLs that the fixture does not have, and the console errors and
    uncaught exceptions of the page."""
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
        for steps in groups:
            await run_steps(page, steps, pause_ms)
            records.append(await evaluate_record(page, record))
    return records, sorted(set(replayer.missing)), errors


def replay_command(
    name: str,
    setup: Path | None,
    steps: Path,
    record: str,
    out: Path | None,
    pause_ms: int,
    system_fonts: bool,
) -> int:
    """Runs `replay` and writes the records (JSON, indent 1) to `out` or
    stdout. Logs the missing URLs and the page's errors."""
    groups = read_groups(steps)
    setup_text = setup.read_text(encoding="utf-8") if setup else ""
    try:
        records, missing, errors = asyncio.run(
            asyncio.wait_for(
                replay(name, setup_text, groups, record, pause_ms, system_fonts),
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
