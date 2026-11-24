"""`swbtools capture URL NAME`: download a page and its resources into a
fixture directory (with `--js`: JavaScript on, into `fixtures/js/NAME`).
`swbtools capture-missing NAME`: add only the resources that a fixture does
not have.

1. Chromium loads the page from the network (JavaScript disabled) and
   scrolls through it. Every response is recorded.
2. If swb is available, `swb --headless --record` loads the page too and
   adds the resources that swb requests (it merges into the manifest).
3. After step 2, Chromium loads the page again from the fixture and fetches
   only what is missing, because swb can replace responses (for example a
   newer version of the HTML).

With `--js --steps FILE`, step 1 also runs the step groups of FILE in the
live page, one after the other, and records the responses of the steps too
(docs/testing.md, "Capture with step groups"). `--max-responses` caps the
fetches from the network.

`fixture.json` has the URL of the first request with the fragment of the URL
that the user gave: Playwright's request URLs have no fragment.
"""

import asyncio
import logging
import shutil
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from playwright.async_api import Error, Page

from swbtools import browser, paths, swb
from swbtools.manifest import FILES_DIR, MANIFEST_FILE, Fixture
from swbtools.pages import META_FILE, REFERENCE_DIR, FixtureMeta, read_meta, write_meta
from swbtools.probe import run_steps
from swbtools.probe_cases import ProbeError
from swbtools.replay import DEFAULT_PAUSE_MS
from swbtools.replay import read_step_groups as read_named_groups
from swbtools.routing import Recorder

log = logging.getLogger(__name__)


@dataclass(frozen=True)
class StepGroup:
    """One group of a steps file: a name for the log and the steps."""

    name: str
    steps: tuple[dict[str, Any], ...]


def read_step_groups(path: Path) -> list[StepGroup]:
    """Reads a steps file (the format of `replay --steps`)."""
    return [StepGroup(name, steps) for name, steps in read_named_groups(path)]


JS_SCROLL_STEPS = 60
"""Scroll steps of a capture with JavaScript on (a page can grow for ever)."""

JS_IDLE_TIMEOUT_MS = 15_000
"""Cap of the wait for network idle after the scroll, with JavaScript on."""


STEP_SETTLE_MS = 500
"""Pause after each step group of a capture with steps, before the wait for
network idle: a page's own timers (a debounce, an animation) start their
requests in it."""

STEP_IDLE_TIMEOUT_MS = 10_000
"""Cap of the wait for network idle after each step group."""

DEFAULT_MAX_RESPONSES = 1000
"""Cap of the responses that a capture with steps fetches from the network
(`--max-responses`): the tile servers' usage policy forbids bulk downloads."""


def with_fragment(first_url: str, given_url: str) -> str:
    """Returns `first_url` with the fragment of `given_url`. Playwright's
    request URLs have no fragment, and a browser keeps the fragment of the
    URL that the user typed (also over a redirect whose `Location` has none),
    so a fixture needs it for the page's scripts (a map's `#map=...`)."""
    fragment = given_url.partition("#")[2]
    base = first_url.partition("#")[0]
    return f"{base}#{fragment}" if fragment else base


def count_hosts(urls: list[str]) -> str:
    """`host=count` for the recorded URLs, most first, for the log."""
    counts: dict[str, int] = {}
    for url in urls:
        host = urlsplit(url).netloc
        counts[host] = counts.get(host, 0) + 1
    return ", ".join(
        f"{host}={n}" for host, n in sorted(counts.items(), key=lambda i: (-i[1], i[0]))
    )


async def run_groups(
    page: Page, recorder: Recorder, groups: Sequence[StepGroup], pause_ms: int
) -> list[str]:
    """Runs the step groups in the live page, one after the other. After
    each group: a pause and a capped wait for network idle, so that the
    responses of the group are recorded before the next one starts. Logs
    the number of recorded responses per group. A step that fails (a
    selector that matches nothing) ends its group and is logged; the next
    group runs: a live run is rare and must not be lost to one selector
    (the replay of the steps then fails at the same step). Returns the
    names of the groups that failed."""
    failed: list[str] = []
    for number, group in enumerate(groups, 1):
        before = len(recorder.recorded)
        try:
            await run_steps(page, group.steps, pause_ms)
        except (Error, ProbeError) as error:
            first_line = (str(error).strip().splitlines() or [type(error).__name__])[0]
            log.warning("group %d (%s) failed: %s", number, group.name, first_line)
            failed.append(group.name)
        await page.wait_for_timeout(STEP_SETTLE_MS)
        await browser.wait_for_network_idle(page, STEP_IDLE_TIMEOUT_MS)
        # A checkpoint: a crash later loses at most the next group.
        recorder.fixture.save()
        log.info(
            "group %d (%s): %d responses",
            number,
            group.name,
            len(recorder.recorded) - before,
        )
    return failed


async def chromium_pass(
    fixture: Fixture,
    url: str,
    replay_known: bool,
    system_fonts: bool,
    javascript: bool = False,
    groups: Sequence[StepGroup] = (),
    pause_ms: int = DEFAULT_PAUSE_MS,
    max_responses: int | None = None,
) -> tuple[str, str]:
    """Loads `url` in Chromium and records what it fetches. Returns the
    URL of the first request (with the fragment of `url`) and the Chromium
    version. With `javascript`, scripts run, the scroll has a step limit,
    and the log names the requests that were still pending after the capped
    wait. With `groups` (step groups, JavaScript only), the page runs them
    after the load and records the responses of the steps too.
    `max_responses` caps the fetches from the network."""
    async with browser.session(browser.PAGE_VIEWPORT, system_fonts, javascript) as (
        chromium,
        context,
        page,
    ):
        recorder = Recorder(
            fixture,
            context,
            replay_known=replay_known,
            max_responses=max_responses,
            reuse_get=bool(groups),
        )
        await recorder.attach(context, page)
        tracker = browser.PendingRequests(page)
        response = await page.goto(url, wait_until="load", timeout=120_000)
        await browser.scroll_through(page, max_steps=JS_SCROLL_STEPS if javascript else None)
        await browser.wait_for_network_idle(page, JS_IDLE_TIMEOUT_MS if javascript else 15_000)
        if javascript and tracker.pending():
            log.warning(
                "%d requests still pending: %s",
                len(tracker.pending()),
                ", ".join(tracker.pending()),
            )
        log.info("load: %d responses", len(recorder.recorded))
        fixture.save()
        first_url = url
        if response is not None:
            request = response.request
            while request.redirected_from is not None:
                request = request.redirected_from
            first_url = request.url
        first_url = with_fragment(first_url, url)
        failed_groups = await run_groups(page, recorder, groups, pause_ms)
        version = chromium.version
    fixture.save()
    log.info("Chromium recorded %d responses", len(recorder.recorded))
    if groups:
        log.warning("recorded per host: %s", count_hosts(recorder.recorded))
        log.warning("%d of %d groups failed: %s", len(failed_groups), len(groups), failed_groups)
    level = logging.WARNING if groups else logging.INFO
    for status, method, recorded_url in recorder.statuses:
        if not 200 <= status < 400:
            log.log(level, "status %d: %s %s", status, method, recorded_url)
    if recorder.failed:
        log.warning("%d requests failed: %s", len(recorder.failed), ", ".join(recorder.failed))
    if recorder.refused:
        log.warning("%d requests refused (limit): %s", len(recorder.refused), recorder.refused[:5])
    if recorder.reused:
        log.info("%d repeated GET requests served from the first response", recorder.reused)
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
    javascript: bool = False,
    steps: Path | None = None,
    max_responses: int | None = None,
    pause_ms: int = DEFAULT_PAUSE_MS,
) -> int:
    """Runs the capture. `with_swb=None` means: use swb if it exists, but not
    for a JavaScript capture (swb runs no scripts yet). `steps` (a steps
    file, JavaScript captures only) makes the page run the step groups after
    the load, and the fixture gets their responses too; then
    `max_responses` defaults to `DEFAULT_MAX_RESPONSES`."""
    if steps is not None and not javascript:
        log.error("--steps needs --js")
        return 2
    if steps is not None and with_swb:
        log.error("--steps and --with-swb do not go together")
        return 2
    groups = read_step_groups(steps) if steps is not None else []
    if steps is not None and max_responses is None:
        max_responses = DEFAULT_MAX_RESPONSES
    if javascript and with_swb is None:
        with_swb = False
    directory = paths.fixture_directory(name, javascript)
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
    first_url, version = asyncio.run(
        chromium_pass(
            fixture, url, False, system_fonts, javascript, groups, pause_ms, max_responses
        )
    )
    captured = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    write_meta(
        directory,
        FixtureMeta(
            url=first_url,
            captured=captured,
            viewport=browser.PAGE_VIEWPORT,
            chromium=version,
            javascript=javascript,
        ),
    )

    if swb_binary is not None:
        status = swb.run(swb.record_command(swb_binary, directory, first_url))
        if status != 0:
            log.warning("swb exited with status %d; its recording may be incomplete", status)
        fixture = Fixture(directory)
        asyncio.run(chromium_pass(fixture, first_url, True, system_fonts, javascript))
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
