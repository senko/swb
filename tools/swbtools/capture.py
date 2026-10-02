"""`swbtools capture URL NAME`: download a page and its resources into a
fixture directory. `swbtools capture-missing NAME`: add only the resources
that a fixture does not have.

1. Chromium loads the page from the network (JavaScript disabled) and
   scrolls through it. Every response is recorded.
2. If swb is available, `swb --headless --record` loads the page too and
   adds the resources that swb requests (it merges into the manifest).
3. After step 2, Chromium loads the page again from the fixture and fetches
   only what is missing, because swb can replace responses (for example a
   newer version of the HTML).
"""

import asyncio
import logging
import shutil
from datetime import UTC, datetime
from pathlib import Path

from playwright.async_api import async_playwright

from swbtools import browser, paths, swb
from swbtools.manifest import FILES_DIR, MANIFEST_FILE, Fixture
from swbtools.pages import META_FILE, REFERENCE_DIR, FixtureMeta, read_meta, write_meta
from swbtools.routing import Recorder

log = logging.getLogger(__name__)


async def chromium_pass(
    fixture: Fixture, url: str, replay_known: bool, system_fonts: bool
) -> tuple[str, str]:
    """Loads `url` in Chromium and records what it fetches. Returns the
    serialized URL of the first request and the Chromium version."""
    async with async_playwright() as playwright:
        chromium = await browser.launch(playwright, system_fonts)
        context = await browser.new_context(chromium, browser.PAGE_VIEWPORT)
        page = await context.new_page()
        recorder = Recorder(fixture, context, replay_known=replay_known)
        await recorder.attach(context, page)
        response = await page.goto(url, wait_until="load", timeout=120_000)
        await browser.scroll_through(page)
        await browser.wait_for_network_idle(page)
        first_url = url
        if response is not None:
            request = response.request
            while request.redirected_from is not None:
                request = request.redirected_from
            first_url = request.url
        version = chromium.version
        await chromium.close()
    fixture.save()
    log.info("Chromium recorded %d responses", len(recorder.recorded))
    if recorder.failed:
        log.warning("%d requests failed: %s", len(recorder.failed), ", ".join(recorder.failed))
    return first_url, version


def _clear(directory: Path) -> None:
    """Removes the captured data and the reference from a fixture directory."""
    for name in (MANIFEST_FILE, META_FILE):
        (directory / name).unlink(missing_ok=True)
    for name in (FILES_DIR, REFERENCE_DIR):
        if (directory / name).is_dir():
            shutil.rmtree(directory / name)


def directory_size(directory: Path) -> int:
    """Returns the total size of the files under `directory`, in bytes."""
    return sum(path.stat().st_size for path in directory.rglob("*") if path.is_file())


def capture(
    url: str,
    name: str,
    with_swb: bool | None,
    swb_path: Path | None,
    force: bool,
    system_fonts: bool,
) -> int:
    """Runs the capture. `with_swb=None` means: use swb if it exists."""
    directory = paths.fixture_dir(name)
    if (directory / MANIFEST_FILE).exists():
        if not force:
            log.error("%s exists; use --force to capture it again", directory)
            return 1
        _clear(directory)
    directory.mkdir(parents=True, exist_ok=True)

    swb_binary = None
    if with_swb is not False:
        swb_binary = swb.find_swb(swb_path)
        if swb_binary is None and with_swb:
            log.error("swb binary not found; build it or pass --swb PATH")
            return 1

    fixture = Fixture(directory)
    first_url, version = asyncio.run(chromium_pass(fixture, url, False, system_fonts))
    captured = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    write_meta(
        directory,
        FixtureMeta(
            url=first_url, captured=captured, viewport=browser.PAGE_VIEWPORT, chromium=version
        ),
    )

    if swb_binary is not None:
        status = swb.run(swb.record_command(swb_binary, directory, first_url))
        if status != 0:
            log.warning("swb exited with status %d; its recording may be incomplete", status)
        fixture = Fixture(directory)
        asyncio.run(chromium_pass(fixture, first_url, True, system_fonts))
    else:
        log.info("swb not used: only Chromium's requests are recorded")

    for unused in fixture.unused_files():
        log.info("remove unused body file %s", unused.name)
        unused.unlink()

    size = directory_size(directory / FILES_DIR)
    print(f"{name}: {len(fixture.entries)} entries, {size / 1e6:.2f} MB in {directory}")
    return 0


def capture_missing(name: str, swb_path: Path | None, system_fonts: bool) -> int:
    """Adds the responses that swb and Chromium request but the fixture
    does not have. Existing entries do not change, so a dynamic page keeps
    its recorded version. Use it when swb starts to load more resources."""
    directory = paths.fixture_dir(name)
    if not (directory / MANIFEST_FILE).exists():
        log.error("%s has no manifest; capture it first", directory)
        return 1
    swb_binary = swb.find_swb(swb_path)
    if swb_binary is None:
        log.error("swb binary not found; build it or pass --swb PATH")
        return 1
    url = read_meta(directory).url
    before = len(Fixture(directory).entries)
    status = swb.run(swb.record_missing_command(swb_binary, directory, url))
    if status != 0:
        log.warning("swb exited with status %d; its recording may be incomplete", status)
    fixture = Fixture(directory)
    asyncio.run(chromium_pass(fixture, url, True, system_fonts))
    added = len(fixture.entries) - before
    print(f"{name}: {added} entries added, {len(fixture.entries)} in total")
    if added:
        print("New resources can change the rendering: run `just reference` for it.")
    return 0
