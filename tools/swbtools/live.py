"""Live Chromium captures for `compare --full-page` and `compare --click`.

`reference` writes the committed first-viewport reference. These captures
replay the fixture the same way (same viewport, fonts, waits), but write
into `out/compare/NAME/`: the full-page screenshot, and the boxes and
screenshots of a state after clicks.
"""

import logging
from dataclasses import dataclass
from pathlib import Path

from playwright.async_api import Page

from swbtools import browser
from swbtools.boxes import BoxDump, write_dump
from swbtools.manifest import Fixture
from swbtools.reference import load_settled
from swbtools.routing import Replayer

log = logging.getLogger(__name__)

CLICK_TIMEOUT_MS = 5000
SETTLE_MS = 200
"""Time to wait after a click, for layout and style changes to finish."""


@dataclass(frozen=True)
class LiveOutput:
    """The files that `capture_live` writes."""

    boxes: Path
    """The box dump (after the clicks, before the full-page capture)."""
    viewport_png: Path | None
    """The screenshot of the first viewport; None if not wanted."""
    full_page_png: Path | None
    """The full-page screenshot; None if not wanted."""


@dataclass(frozen=True)
class LiveResult:
    """What `capture_live` found."""

    missing: list[str]
    """Requested URLs that the fixture does not have."""
    layout_changed: bool
    """True if the full-page capture changed the box dump (it should not)."""


async def click_and_settle(page: Page, selectors: list[str]) -> None:
    """Clicks the first element of each selector in turn, then scrolls to
    the top and waits for fonts and layout."""
    for selector in selectors:
        await page.locator(selector).first.click(timeout=CLICK_TIMEOUT_MS)
        await page.wait_for_timeout(SETTLE_MS)
    if selectors:
        await page.evaluate("window.scrollTo(0, 0)")
        await browser.wait_for_fonts(page)
        await page.wait_for_timeout(SETTLE_MS)


async def capture_live(
    fixture_path: Path,
    url: str,
    viewport: tuple[int, int],
    clicks: list[str],
    output: LiveOutput,
    system_fonts: bool = False,
) -> LiveResult:
    """Replays the fixture in Chromium, applies the clicks, and writes the
    box dump and the wanted screenshots. The full-page screenshot comes last,
    and the box dump is read again after it: Playwright's full-page capture
    must not change the layout."""
    replayer = Replayer(Fixture(fixture_path))
    async with browser.session(viewport, system_fonts) as (_, context, page):
        await replayer.attach(context, page)
        await load_settled(page, url)
        await click_and_settle(page, clicks)
        dump = await browser.collect_boxes(page, url)
        write_dump(output.boxes, dump)
        if output.viewport_png is not None:
            await page.screenshot(path=output.viewport_png, animations="disabled", caret="hide")
        changed = False
        if output.full_page_png is not None:
            await page.screenshot(
                path=output.full_page_png, full_page=True, animations="disabled", caret="hide"
            )
            changed = _differs(dump, await browser.collect_boxes(page, url))
            if changed:
                log.warning("the full-page capture changed Chromium's layout")
    return LiveResult(sorted(set(replayer.missing)), changed)


def _differs(before: BoxDump, after: BoxDump) -> bool:
    return before.viewport != after.viewport or before.elements != after.elements
