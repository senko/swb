"""`swbtools layout-refs`: Chromium box dumps for the layout tests.

For each `tests/layout/<name>.html`, Chromium loads the file and writes
`tests/layout/<name>.boxes.json`. The `url` field is the path relative to the
repository root, so the files do not depend on the checkout location.

Before the dump, every element with a `data-scroll="X Y"` attribute is
scrolled to that offset (`scrollLeft`, `scrollTop`; Chromium clamps it). The
boxes of its content then show the scroll offset, and a large offset tests
the scroll range. The Rust layout test does the same in swb.
"""

import asyncio
import logging
from pathlib import Path

from swbtools import browser, paths
from swbtools.boxes import BoxDump, write_dump

log = logging.getLogger(__name__)

BOXES_SUFFIX = ".boxes.json"

# Scrolls the elements with `data-scroll="X Y"` in tree order.
_APPLY_DATA_SCROLL_JS = """
() => {
  for (const element of document.querySelectorAll('[data-scroll]')) {
    const [x, y] = element.dataset.scroll.trim().split(/\\s+/).map(Number);
    element.scrollLeft = x;
    element.scrollTop = y;
  }
}
"""


def layout_tests(names: list[str]) -> list[Path]:
    """Returns the HTML files of the named layout tests (all if empty)."""
    directory = paths.layout_tests_dir()
    if names:
        return [directory / f"{name.removesuffix('.html')}.html" for name in names]
    return sorted(directory.glob("*.html"))


def boxes_path(file: Path) -> Path:
    """Returns the box dump path for a layout test: `<name>.boxes.json`."""
    return file.with_name(file.name.removesuffix(".html") + BOXES_SUFFIX)


def _dump_url(file: Path) -> str:
    """The `url` of a layout test dump: the path relative to the repository
    root, or the file URL for files outside it."""
    try:
        return file.relative_to(paths.repo_root()).as_posix()
    except ValueError:
        return file.as_uri()


async def dump_layout_tests(files: list[Path], system_fonts: bool) -> None:
    """Writes the box dump of each file next to it."""
    async with browser.session(browser.LAYOUT_VIEWPORT, system_fonts) as (_, _, page):
        for file in (file.resolve() for file in files):
            await page.goto(file.as_uri(), wait_until="load")
            await browser.stop_animations(page)
            await page.evaluate(_APPLY_DATA_SCROLL_JS)
            dump = await browser.collect_boxes(page, _dump_url(file))
            write_dump(boxes_path(file), dump)
            print(f"{_dump_url(boxes_path(file))}: {_count_boxes(dump)} boxes")


def _count_boxes(dump: BoxDump) -> int:
    return sum(element.rect is not None for element in dump.elements)


def layout_refs(names: list[str], system_fonts: bool) -> int:
    """Generates the expected box dumps of the layout tests."""
    files = layout_tests(names)
    missing = [file for file in files if not file.is_file()]
    if missing:
        log.error("no such layout test: %s", ", ".join(str(file) for file in missing))
        return 1
    if not files:
        log.error("no layout tests in %s", paths.layout_tests_dir())
        return 1
    asyncio.run(dump_layout_tests(files, system_fonts))
    return 0
