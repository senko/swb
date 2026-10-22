"""`swbtools reference NAME`: Chromium output for a page fixture.

Chromium loads the fixture's URL with every request answered from the
fixture, then writes `reference/boxes.json` and `reference/screenshot.png`.

`reference --js NAME` does the same for a JavaScript fixture
(`fixtures/js/NAME`): JavaScript on, the page clock starting at the capture
time, the output in `out/js-reference/NAME/` (docs/testing.md, "Compare with
JavaScript on").
"""

import asyncio
import logging
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path

from playwright.async_api import Page

from swbtools import browser, paths
from swbtools.boxes import write_dump
from swbtools.manifest import Fixture
from swbtools.pages import (
    REFERENCE_BOXES,
    REFERENCE_DIR,
    REFERENCE_SCREENSHOT,
    list_fixtures,
    read_meta,
)
from swbtools.routing import Replayer

log = logging.getLogger(__name__)


async def load_settled(page: Page, url: str) -> None:
    """Loads `url` (served from the fixture) and waits until the page is
    stable: lazy images loaded, web fonts loaded, animations off, scrolled to
    the top. `reference` and the live captures of `compare` share it."""
    await page.goto(url, wait_until="load", timeout=60_000)
    # Load lazy images everywhere on the page, then measure at the top.
    await browser.scroll_through(page, pause_ms=100)
    await browser.wait_for_network_idle(page)
    await page.evaluate("window.scrollTo(0, 0)")
    await browser.wait_for_fonts(page)
    await browser.stop_animations(page)


async def render_fixture(
    fixture_path: Path,
    url: str,
    output: Path,
    viewport: tuple[int, int] = browser.PAGE_VIEWPORT,
    system_fonts: bool = False,
) -> list[str]:
    """Loads `url` from the fixture and writes the box dump and the
    screenshot into `output`. Returns the URLs that were not in the fixture."""
    fixture = Fixture(fixture_path)
    replayer = Replayer(fixture)
    async with browser.session(viewport, system_fonts) as (_, context, page):
        await replayer.attach(context, page)
        await load_settled(page, url)
        dump = await browser.collect_boxes(page, url)
        write_dump(output / REFERENCE_BOXES, dump)
        await browser.screenshot(page, output / REFERENCE_SCREENSHOT)
    return sorted(set(replayer.missing))


JS_IDLE_TIMEOUT_MS = 15_000
"""Cap of the wait for network idle after `load`, with JavaScript on."""

JS_SETTLE_MS = 1_000
"""Wait after the network is idle, for timers and layout that the scripts
start late. The page is not scrolled: scripts react to scrolling."""


async def load_settled_js(page: Page, url: str) -> None:
    """Loads `url` (served from the fixture) with JavaScript on and waits
    until the page is done: `load`, network idle (capped), `JS_SETTLE_MS`,
    web fonts loaded, animations off. No scrolling."""
    await page.goto(url, wait_until="load", timeout=60_000)
    await browser.wait_for_network_idle(page, JS_IDLE_TIMEOUT_MS)
    await page.wait_for_timeout(JS_SETTLE_MS)
    await browser.wait_for_fonts(page)
    await browser.stop_animations(page)
    await page.evaluate("window.scrollTo(0, 0)")


@dataclass
class JsRender:
    """What `render_js_fixture` observed besides the output files."""

    missing: list[str] = field(default_factory=list)
    """Requested URLs that the fixture does not have."""
    errors: list[str] = field(default_factory=list)
    """Console errors and uncaught exceptions of the page."""


async def render_js_fixture(
    fixture_path: Path,
    url: str,
    output: Path,
    viewport: tuple[int, int],
    start_time: datetime,
    system_fonts: bool = False,
) -> JsRender:
    """Loads `url` from the fixture in Chromium with JavaScript on and the
    clock at `start_time`, and writes the box dump and the screenshot into
    `output`."""
    fixture = Fixture(fixture_path)
    replayer = Replayer(fixture)
    result = JsRender()
    async with browser.session(viewport, system_fonts, javascript=True, start_time=start_time) as (
        _,
        context,
        page,
    ):
        await replayer.attach(context, page)
        page.on(
            "console",
            lambda message: (
                result.errors.append(f"console: {message.text}")
                if message.type == "error"
                else None
            ),
        )
        page.on("pageerror", lambda error: result.errors.append(f"exception: {error}"))
        await load_settled_js(page, url)
        write_dump(output / REFERENCE_BOXES, await browser.collect_boxes(page, url))
        await browser.screenshot(page, output / REFERENCE_SCREENSHOT)
    result.missing = sorted(set(replayer.missing))
    return result


def parse_captured(captured: str) -> datetime:
    """Parses the `captured` time of `fixture.json` (UTC, ISO 8601). A time
    without an offset is UTC."""
    time = datetime.fromisoformat(captured)
    return time if time.tzinfo is not None else time.replace(tzinfo=UTC)


def reference_js(names: list[str], system_fonts: bool) -> int:
    """Generates the JavaScript-on references of the named fixtures of
    `fixtures/js/` into `out/js-reference/NAME/`."""
    status = 0
    for name in names:
        directory = paths.js_fixture_dir(name)
        try:
            meta = read_meta(directory)
        except FileNotFoundError:
            log.error("%s: no fixture.json in %s", name, directory)
            status = 1
            continue
        output = paths.js_reference_dir(name)
        output.mkdir(parents=True, exist_ok=True)
        result = asyncio.run(
            render_js_fixture(
                directory,
                meta.url,
                output,
                meta.viewport,
                parse_captured(meta.captured),
                system_fonts,
            )
        )
        for url in result.missing:
            log.warning("%s: not in fixture: %s", name, url)
        for error in result.errors:
            log.warning("%s: %s", name, error)
        print(
            f"{name}: JavaScript reference written to {output} "
            f"({len(result.missing)} URLs not in fixture, {len(result.errors)} errors)"
        )
    return status


def reference(names: list[str], system_fonts: bool) -> int:
    """Generates the references of the named fixtures (all if empty)."""
    status = 0
    for name in names or list_fixtures():
        directory = paths.fixture_dir(name)
        try:
            meta = read_meta(directory)
        except FileNotFoundError:
            log.error("%s: no fixture.json; capture the fixture first", name)
            status = 1
            continue
        output = directory / REFERENCE_DIR
        missing = asyncio.run(
            render_fixture(directory, meta.url, output, meta.viewport, system_fonts)
        )
        for url in missing:
            log.warning("%s: not in fixture: %s", name, url)
        print(f"{name}: reference written to {output} ({len(missing)} URLs not in fixture)")
    return status
