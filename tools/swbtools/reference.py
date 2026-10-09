"""`swbtools reference NAME`: Chromium output for a page fixture.

Chromium loads the fixture's URL with every request answered from the
fixture, then writes `reference/boxes.json` and `reference/screenshot.png`.
"""

import asyncio
import logging
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
