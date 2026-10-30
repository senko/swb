"""`swbtools js-survey NAME...`: measure what the scripts of a JavaScript
fixture (`fixtures/js/NAME`) do in Chromium.

Per page, four runs, each with every request answered from the fixture
(requests that the fixture lacks fail, and the survey records them):

1. Traced: an init script (js_tracer.js) counts the Web API usage by phase.
   The run also records the scripts that Chromium compiled (CDP
   `Debugger.scriptParsed`), errors and requests.
2. Untraced, with JIT: `Performance.getMetrics` and the first-viewport
   screenshot.
3. Untraced with `--js-flags=--jitless` (a separate browser launch): the
   same metrics. swb's interpreter is about 2 times slower than
   `node --jitless`, so this predicts swb's script time.
4. JavaScript off: element count and screenshot, for the difference.

The output is `out/js-survey/NAME/survey.json`, `js-on.png`, `js-off.png`,
`js-diff.png` and `summary.md`. The fixture stays local: see docs/testing.md.
"""

import asyncio
import json
import logging
import shutil
from collections.abc import Coroutine
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

from playwright.async_api import CDPSession, ConsoleMessage, Page, Request

from swbtools import browser, paths, pixels
from swbtools.js_survey_data import (
    first_line,
    group_scripts,
    interaction_listeners,
    median_metrics,
    metrics_from_cdp,
    page_scripts,
    render_markdown,
    request_summary,
    script_kinds,
    summarize_trace,
)
from swbtools.manifest import MANIFEST_FILE, Fixture
from swbtools.pages import read_meta
from swbtools.routing import Replayer

log = logging.getLogger(__name__)

TRACER_FILE = Path(__file__).with_name("js_tracer.js")
SCROLL_STEPS = 60
IDLE_TIMEOUT_MS = 15_000
SETTLE_MS = 2_000
"""Wait after the network idle wait, for timers that start late."""
MAX_LOGGED = 100
TOOL_SCRIPT_PREFIX = "swb-survey:"
"""The source URL prefix of the tool's own scripts, which the survey ignores."""
JITLESS = ("--js-flags=--jitless",)
RUN_TIMEOUT_S = 300
"""The limit of one run. A page that blocks its main thread (an endless loop
in a timer) would otherwise stop the survey: `Runtime.evaluate` has no
timeout."""


def tracer_source() -> str:
    """The text of the init script."""
    return (
        TRACER_FILE.read_text(encoding="utf-8") + f"\n//# sourceURL={TOOL_SCRIPT_PREFIX}tracer.js\n"
    )


async def harness(cdp: CDPSession, expression: str) -> Any:
    """Evaluates `expression` in the page for the tool itself. The tracer
    is paused meanwhile, so that the tool's own calls (scrolling) are not
    counted as the page's. `Runtime.evaluate` is used because Playwright's
    `page.evaluate` calls `window.eval`, which the tracer would count."""
    wrapped = (
        "(() => { const t = window.__swb_trace__; if (t) t.paused = true;"
        f" try {{ return ({expression}); }} finally {{ if (t) t.paused = false; }} }})()"
        f"\n//# sourceURL={TOOL_SCRIPT_PREFIX}harness.js\n"
    )
    result = await cdp.send(
        "Runtime.evaluate", {"expression": wrapped, "returnByValue": True, "awaitPromise": False}
    )
    if "exceptionDetails" in result:
        raise RuntimeError(f"harness evaluation failed: {result['exceptionDetails']}")
    return result["result"].get("value")


async def scroll_through(page: Page, cdp: CDPSession) -> None:
    """`browser.scroll_through` with `harness`: scrolls down in steps (so
    that lazy content loads) and back to the top."""
    size = page.viewport_size
    step = int(size["height"] * 0.8) if size else 600
    y = 0
    for _ in range(SCROLL_STEPS):
        height = await harness(cdp, "document.documentElement.scrollHeight")
        if y >= height:
            break
        await harness(cdp, f"window.scrollTo(0, {y})")
        await page.wait_for_timeout(250)
        y += step
    await harness(cdp, "window.scrollTo(0, 0)")


async def settle(page: Page, cdp: CDPSession, tracker: browser.PendingRequests) -> list[str]:
    """Scrolls through the page, waits (capped) for the network and for late
    timers, and scrolls back to the top. Returns the URLs that were still
    pending after the capped wait."""
    await scroll_through(page, cdp)
    await browser.wait_for_network_idle(page, IDLE_TIMEOUT_MS)
    pending = tracker.pending()
    if pending:
        log.warning("%d requests still pending: %s", len(pending), ", ".join(pending[:10]))
    await page.wait_for_timeout(SETTLE_MS)
    await harness(cdp, "window.scrollTo(0, 0)")
    return pending


def main_document(fixture: Fixture, url: str) -> str:
    """The text of the page's HTML in the fixture (follows redirects)."""
    for _ in range(6):
        entry = fixture.get(url)
        if entry is None:
            return ""
        location = dict(entry.headers).get("location")
        if entry.status in (301, 302, 303, 307, 308) and location:
            url = urljoin(url, location)
            continue
        return fixture.read_body(entry).decode("utf-8", errors="replace")
    return ""


class Collector:
    """Collects console errors, page errors and requests of one page."""

    def __init__(self, page: Page) -> None:
        self.console: list[dict[str, str]] = []
        self.pageerror: list[str] = []
        self.requests: list[tuple[str, str]] = []
        page.on("console", self._on_console)
        page.on("pageerror", lambda error: self._add(self.pageerror, first_line(str(error))))
        page.on("request", self._on_request)

    def _add(self, target: list[Any], value: Any) -> None:
        if len(target) < MAX_LOGGED:
            target.append(value)

    def _on_console(self, message: ConsoleMessage) -> None:
        if message.type == "error":
            self._add(
                self.console, {"text": first_line(message.text), "url": message.location["url"]}
            )

    def _on_request(self, request: Request) -> None:
        self.requests.append((request.resource_type, request.url))


async def traced_run(
    fixture_path: Path, url: str, viewport: tuple[int, int], system_fonts: bool
) -> dict[str, Any]:
    """Run 1: loads the page with the tracer and returns the raw data."""
    fixture = Fixture(fixture_path)
    replayer = Replayer(fixture)
    async with browser.session(viewport, system_fonts, javascript=True) as (
        chromium,
        context,
        page,
    ):
        await context.add_init_script(tracer_source())
        await replayer.attach(context, page)
        collector = Collector(page)
        tracker = browser.PendingRequests(page)
        scripts: list[dict[str, Any]] = []
        cdp = await context.new_cdp_session(page)
        cdp.on(
            "Debugger.scriptParsed",
            lambda e: scripts.append(
                {
                    "url": e.get("url", ""),
                    "length": e.get("length", 0),
                    "module": bool(e.get("isModule", False)),
                    "main_world": bool(e.get("executionContextAuxData", {}).get("isDefault")),
                    "hash": e.get("hash", ""),
                }
            ),
        )
        await cdp.send("Debugger.enable")
        # Scripts that Playwright injects into every document: a blank page
        # shows them, and the survey ignores scripts with the same hash.
        await page.goto("about:blank")
        baseline = {script["hash"] for script in scripts}
        scripts.clear()
        await page.goto(url, wait_until="load", timeout=120_000)
        pending = await settle(page, cdp, tracker)
        raw = await harness(cdp, "window.__swb_trace__ ? window.__swb_trace__.flush() : null")
        dom = await harness(
            cdp,
            "({elements: document.getElementsByTagName('*').length,"
            " iframes: document.getElementsByTagName('iframe').length,"
            " scripts: document.getElementsByTagName('script').length})",
        )
        return {
            "chromium": chromium.version,
            "raw": raw,
            "scripts": page_scripts(scripts, TOOL_SCRIPT_PREFIX, baseline),
            "console": collector.console,
            "pageerror": collector.pageerror,
            "requests": collector.requests,
            "missing": replayer.missing,
            "pending": pending,
            "dom": dom,
        }


async def timing_run(
    fixture_path: Path,
    url: str,
    viewport: tuple[int, int],
    system_fonts: bool,
    extra_args: tuple[str, ...],
    screenshot: Path | None,
) -> dict[str, Any]:
    """Runs 2 and 3: loads the page without the tracer and reads the
    Performance metrics. With `screenshot`, also writes the first-viewport
    PNG and counts the elements."""
    fixture = Fixture(fixture_path)
    replayer = Replayer(fixture)
    async with browser.session(viewport, system_fonts, javascript=True, extra_args=extra_args) as (
        _,
        context,
        page,
    ):
        await replayer.attach(context, page)
        tracker = browser.PendingRequests(page)
        cdp = await context.new_cdp_session(page)
        await cdp.send("Performance.enable")
        await page.goto(url, wait_until="load", timeout=120_000)
        await settle(page, cdp, tracker)
        metrics = metrics_from_cdp((await cdp.send("Performance.getMetrics"))["metrics"])
        result: dict[str, Any] = {"metrics": metrics}
        if screenshot is not None:
            await browser.wait_for_fonts(page)
            await browser.stop_animations(page)
            result["elements"] = await page.evaluate("document.getElementsByTagName('*').length")
            await browser.screenshot(page, screenshot)
        return result


async def js_off_run(
    fixture_path: Path,
    url: str,
    viewport: tuple[int, int],
    system_fonts: bool,
    screenshot: Path,
) -> dict[str, Any]:
    """Run 4: loads the same fixture with JavaScript off."""
    fixture = Fixture(fixture_path)
    replayer = Replayer(fixture)
    async with browser.session(viewport, system_fonts) as (_, context, page):
        await replayer.attach(context, page)
        await page.goto(url, wait_until="load", timeout=120_000)
        await browser.scroll_through(page, pause_ms=100, max_steps=SCROLL_STEPS)
        await browser.wait_for_network_idle(page, IDLE_TIMEOUT_MS)
        await page.evaluate("window.scrollTo(0, 0)")
        await browser.wait_for_fonts(page)
        await browser.stop_animations(page)
        elements = await page.evaluate("document.getElementsByTagName('*').length")
        await browser.screenshot(page, screenshot)
        return {"elements": elements}


def screenshot_difference(on: Path, off: Path, diff: Path) -> dict[str, Any]:
    """Compares the two screenshots; writes the diff image. Returns the
    fraction of differing pixels and the bounding box of the difference."""
    reference = pixels.load_rgb(on)
    mask = pixels.union_difference(reference, pixels.load_rgb(off))
    pixels.diff_image(reference, mask).save(diff)
    rows = mask.any(axis=1).nonzero()[0]
    cols = mask.any(axis=0).nonzero()[0]
    box = None
    if rows.size:
        box = [int(cols[0]), int(rows[0]), int(cols[-1]) + 1, int(rows[-1]) + 1]
    return {"diff_fraction": round(float(mask.mean()), 6), "diff_box": box}


async def survey_page(name: str, runs: int, system_fonts: bool) -> dict[str, Any]:
    """Runs the four runs for one fixture and returns the survey data."""
    directory = paths.js_fixture_dir(name)
    if not (directory / MANIFEST_FILE).is_file():
        raise FileNotFoundError(f"{directory} has no manifest; capture it with --js first")
    meta = read_meta(directory)
    out = paths.out_dir() / f"js-survey{paths.variant()}" / name
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    viewport, url = meta.viewport, meta.url

    log.info("%s: traced run", name)
    traced = await bounded(traced_run(directory, url, viewport, system_fonts), "traced run")
    log.info("%s: JIT runs", name)
    jit = [
        await bounded(
            timing_run(
                directory, url, viewport, system_fonts, (), out / "js-on.png" if i == 0 else None
            ),
            "JIT run",
        )
        for i in range(runs)
    ]
    log.info("%s: jitless runs", name)
    jitless = [
        await bounded(
            timing_run(directory, url, viewport, system_fonts, JITLESS, None), "jitless run"
        )
        for _ in range(runs)
    ]
    log.info("%s: JavaScript off", name)
    off = await bounded(
        js_off_run(directory, url, viewport, system_fonts, out / "js-off.png"), "JS-off run"
    )
    difference = screenshot_difference(out / "js-on.png", out / "js-off.png", out / "js-diff.png")

    trace = summarize_trace(traced["raw"] or {})
    listeners = interaction_listeners(
        trace["args"].get("addEventListener", []) + trace["args"].get("on-handler", [])
    )
    survey = {
        "name": name,
        "url": url,
        "captured": meta.captured,
        "date": datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "chromium": traced["chromium"],
        "viewport": list(viewport),
        "html_scripts": script_kinds(main_document(Fixture(directory), url)),
        "compiled_scripts": group_scripts(traced["scripts"], url),
        "compiled_script_list": traced["scripts"],
        "trace": trace,
        "interaction_listeners": listeners,
        "errors": {"console": traced["console"], "pageerror": traced["pageerror"]},
        "requests": request_summary(traced["requests"], traced["missing"], url),
        "missing_urls": sorted(set(traced["missing"])),
        "pending_after_wait": traced["pending"],
        "dom": traced["dom"],
        "metrics": {
            "jit": {
                "runs": [r["metrics"] for r in jit],
                "median": median_metrics([r["metrics"] for r in jit]),
            },
            "jitless": {
                "runs": [r["metrics"] for r in jitless],
                "median": median_metrics([r["metrics"] for r in jitless]),
            },
        },
        "js_off": {
            "elements_on": jit[0].get("elements"),
            "elements_off": off["elements"],
            **difference,
        },
    }
    (out / "survey.json").write_text(
        json.dumps(survey, indent=1, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    summary = render_markdown(name, survey)
    (out / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)
    return survey


async def bounded[T](run: Coroutine[Any, Any, T], what: str) -> T:
    """Awaits `run` for at most `RUN_TIMEOUT_S`. On a timeout the browser
    session of the run closes (its context manager runs on cancellation)."""
    try:
        return await asyncio.wait_for(run, RUN_TIMEOUT_S)
    except TimeoutError as error:
        raise TimeoutError(f"the {what} took more than {RUN_TIMEOUT_S} s") from error


def js_survey(names: list[str], runs: int, system_fonts: bool) -> int:
    """Surveys the named JavaScript fixtures. Returns the exit status."""
    status = 0
    for name in names:
        try:
            asyncio.run(survey_page(name, max(1, runs), system_fonts))
        except Exception as error:
            log.error("%s: %s", name, error, exc_info=log.isEnabledFor(logging.DEBUG))
            status = 1
    return status
