"""swb's boxes and screenshots of a page state, through the automation API.

`compare --click` uses it: load the fixture, click the first element of each
selector, wait until the page settles, then read the boxes and take the
screenshots.
"""

import json
import logging
import time
from pathlib import Path

from swbtools.automation import Browser
from swbtools.boxes import parse_dump, write_dump

log = logging.getLogger(__name__)

SETTLE_TIMEOUT_S = 30
POLL_S = 0.05


class StateError(Exception):
    """A selector matched nothing, or the page did not settle."""


def wait_until_settled(browser: Browser, timeout_s: float = SETTLE_TIMEOUT_S) -> None:
    """Waits until the page has loaded (a click can start a load, for
    example of images that appear)."""
    deadline = time.monotonic() + timeout_s
    while True:
        if not browser.wait_for_load(int(timeout_s * 1000)):
            raise StateError("the page did not finish loading")
        if browser.info()["loadState"] in ("complete", "failed"):
            return
        if time.monotonic() > deadline:
            raise StateError("the page did not settle")
        time.sleep(POLL_S)


def click_selectors(browser: Browser, selectors: list[str]) -> None:
    """Clicks the first element of each selector in turn, then scrolls to
    the top (as `live.click_and_settle` does)."""
    for selector in selectors:
        node = browser.query_selector(selector)
        if node is None:
            raise StateError(f"no element matches {selector!r}")
        browser.click_node(node)
        wait_until_settled(browser)
    if selectors:
        browser.call("page.scrollTo", x=0, y=0)


def capture_state(
    browser: Browser,
    url: str,
    clicks: list[str],
    boxes: Path,
    viewport_png: Path | None,
    full_page_png: Path | None,
) -> None:
    """Loads `url`, applies the clicks and writes the box dump and the wanted
    screenshots."""
    browser.navigate(url)
    wait_until_settled(browser)
    click_selectors(browser, clicks)
    write_dump(boxes, parse_dump(json.dumps(browser.call("dom.boxes"))))
    if viewport_png is not None:
        viewport_png.write_bytes(browser.screenshot())
    if full_page_png is not None:
        full_page_png.write_bytes(browser.screenshot(full_page=True))
