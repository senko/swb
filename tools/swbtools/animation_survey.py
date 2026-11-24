"""`swbtools animation-survey`: how many CSS animations and transitions run in
the first view of a fixture in Chromium (M13 feature 1).

The references of `reference` and `compare` turn animations and transitions
off (`browser.stop_animations`). This command loads the same fixture with the
same steps (replayed requests, the clock of the capture for fixtures with
JavaScript, `load`, network idle, the settle time, fonts) and does not turn
them off. After the settle time it lists `document.getAnimations()` and counts
the elements that declare an animation or a transition, so that a swb that
animates can be given Chromium's reference convention without changing the
scores of the targets (docs/videojs-diagnosis.md, section 4).
"""

import asyncio
import json
import logging
from pathlib import Path
from typing import Any

from playwright.async_api import Page

from swbtools import browser, paths
from swbtools.manifest import Fixture
from swbtools.pages import list_fixtures, list_js_fixtures, read_meta
from swbtools.reference import JS_IDLE_TIMEOUT_MS, JS_SETTLE_MS, parse_captured
from swbtools.routing import Replayer

log = logging.getLogger(__name__)

SURVEY_JS = """
() => {
  const vw = innerWidth, vh = innerHeight;
  const inView = (el) => {
    if (!el || !el.getBoundingClientRect) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 && r.top < vh && r.left < vw;
  };
  const path = (el) => {
    if (!el) return '?';
    let s = el.localName + (el.id ? '#' + el.id : '');
    const c = (el.getAttribute && el.getAttribute('class')) || '';
    if (c) s += '.' + c.trim().split(/\\s+/).slice(0, 2).join('.');
    return s.slice(0, 80);
  };
  const animations = document.getAnimations().map((a) => {
    const e = a.effect;
    const t = e ? e.getComputedTiming() : {};
    const target = e && e.target;
    return {
      type: a.constructor.name,
      name: a.animationName || a.transitionProperty || '',
      target: path(target),
      pseudo: (e && e.pseudoElement) || '',
      playState: a.playState,
      duration: t.duration,
      delay: t.delay,
      iterations: t.iterations === Infinity ? 'infinite' : t.iterations,
      fill: t.fill,
      inViewport: inView(target),
    };
  });
  const declared = {animation: 0, animationInView: 0, transition: 0, transitionInView: 0};
  const properties = {};
  for (const el of document.querySelectorAll('*')) {
    const cs = getComputedStyle(el);
    if (cs.animationName !== 'none') {
      declared.animation++;
      if (inView(el)) declared.animationInView++;
    }
    const ds = cs.transitionDuration.split(',').map((d) => parseFloat(d));
    if (ds.some((d) => d > 0) && cs.transitionProperty !== 'none') {
      declared.transition++;
      if (inView(el)) declared.transitionInView++;
      for (const p of cs.transitionProperty.split(',')) {
        properties[p.trim()] = (properties[p.trim()] || 0) + 1;
      }
    }
  }
  return {animations, declared, properties, elements: document.querySelectorAll('*').length};
}
"""


EVENTS_JS = """
(() => {
  const counts = {}, names = {};
  const types = ['animationstart', 'animationend', 'animationcancel', 'transitionrun',
    'transitionstart', 'transitionend', 'transitioncancel'];
  for (const type of types) {
    addEventListener(type, (e) => {
      counts[type] = (counts[type] || 0) + 1;
      const n = type.startsWith('animation') ? e.animationName : e.propertyName;
      if (type === 'animationstart' || type === 'transitionrun') names[n] = (names[n] || 0) + 1;
    }, true);
  }
  window.__animationEvents = () => ({counts, names});
})();
"""
"""An init script: counts the animation and transition events of the page from
its start (they do not bubble to `window` from a shadow tree, which the
fixtures do not use for animations)."""


async def survey_page(page: Page, url: str, javascript: bool) -> dict[str, Any]:
    """Loads `url` as `reference` does, without turning animations off, and
    returns what `SURVEY_JS` finds after the settle time and again 2 s later
    (the animations that are gone then are finite ones that were still
    running at the first reading)."""
    await page.add_init_script(EVENTS_JS)
    await page.goto(url, wait_until="load", timeout=60_000)
    if javascript:
        await browser.wait_for_network_idle(page, JS_IDLE_TIMEOUT_MS)
        await page.wait_for_timeout(JS_SETTLE_MS)
    await browser.wait_for_fonts(page)
    first = await page.evaluate(SURVEY_JS)
    await page.wait_for_timeout(2000)
    later = await page.evaluate(SURVEY_JS)
    first["later"] = len(later["animations"])
    first["events"] = await page.evaluate("window.__animationEvents()")
    return first


async def survey_fixture(directory: Path, javascript: bool) -> dict[str, Any]:
    """The survey of one fixture."""
    meta = read_meta(directory)
    replayer = Replayer(Fixture(directory))
    start = parse_captured(meta.captured) if javascript else None
    async with browser.session(meta.viewport, False, javascript=javascript, start_time=start) as (
        _,
        context,
        page,
    ):
        await replayer.attach(context, page)
        result = await survey_page(page, meta.url, javascript)
    result["missing"] = len(set(replayer.missing))
    return result


def summarize(result: dict[str, Any]) -> dict[str, Any]:
    """The counts of one survey that the report tables show."""
    animations = result["animations"]
    kinds: dict[str, int] = {}
    for item in animations:
        kinds[item["type"]] = kinds.get(item["type"], 0) + 1
    return {
        "elements": result["elements"],
        "running": len(animations),
        "types": kinds,
        "in_view": sum(1 for a in animations if a["inViewport"]),
        "infinite": sum(1 for a in animations if a["iterations"] == "infinite"),
        "gone_after_2s": max(0, len(animations) - result["later"]),
        "events": result["events"]["counts"],
        "started": result["events"]["names"],
        **result["declared"],
    }


def animation_survey(names: list[str], javascript: bool, out: Path) -> int:
    """Surveys the named fixtures (all if empty) of `fixtures/js` (with
    `javascript`) or `fixtures/pages`, prints a table and writes
    `OUT/NAME.json` for each."""
    names = names or (list_js_fixtures() if javascript else list_fixtures())
    out.mkdir(parents=True, exist_ok=True)
    header = (
        "fixture elements running types in_view infinite gone_after_2s "
        "animation(in_view) transition(in_view) events started"
    )
    print(header)
    for name in names:
        directory = paths.fixture_directory(name, javascript)
        try:
            result = asyncio.run(survey_fixture(directory, javascript))
        except FileNotFoundError:
            log.error("%s: no fixture.json in %s", name, directory)
            return 1
        (out / f"{name}.json").write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8")
        s = summarize(result)
        types = ",".join(f"{k}={v}" for k, v in sorted(s["types"].items())) or "-"
        print(
            f"{name} {s['elements']} {s['running']} {types} {s['in_view']} {s['infinite']} "
            f"{s['gone_after_2s']} {s['animation']}({s['animationInView']}) "
            f"{s['transition']}({s['transitionInView']}) "
            f"events={s['events']} started={s['started']}"
        )
    return 0


# --- CPU use of endless animations ------------------------------------------

SYNTHETIC_PAGES = {
    # The buffering spinner of the Video.js skin: eight segments, one
    # animation each, `steps(8, end)` and a delay from a custom property.
    "spinner": """<!DOCTYPE html><style>
body{margin:0;background:#222}
.seg{animation:var(--media-spinner-animation, media-spinner-fade 1s steps(8, end) infinite);
animation-delay:var(--media-spinner-delay)}
@keyframes media-spinner-fade{0%{opacity:1}to{opacity:0}}</style>
<svg width=48 height=48 viewBox="0 0 18 18" fill=none stroke=#fff stroke-width=1.5
stroke-linecap=round>
<path class=seg opacity=.5 d="M9 1.5v3" style="--media-spinner-delay:0s"/>
<path class=seg opacity=.5 d="m14.5 3.5-2 2" style="--media-spinner-delay:.125s"/>
<path class=seg opacity=.5 d="M16.5 9h-3" style="--media-spinner-delay:.25s"/>
<path class=seg opacity=.5 d="m14.5 14.5-2-2" style="--media-spinner-delay:.375s"/>
<path class=seg opacity=.5 d="M9 16.5v-3" style="--media-spinner-delay:.5s"/>
<path class=seg opacity=.5 d="m3.5 14.5 2-2" style="--media-spinner-delay:.625s"/>
<path class=seg opacity=.5 d="M1.5 9h3" style="--media-spinner-delay:.75s"/>
<path class=seg opacity=.5 d="m3.5 3.5 2 2" style="--media-spinner-delay:.875s"/></svg>""",
    # The marquee of the page: a wide row of text that moves 50 % in 30 s.
    "marquee": """<!DOCTYPE html><style>
body{margin:0;background:#222;color:#f3e7d2;font:bold 15px sans-serif;overflow:hidden}
#m{display:flex;width:2400px;gap:60px;white-space:nowrap;animation:marquee 30s linear infinite}
@keyframes marquee{0%{transform:translate(-50%)}to{transform:translate(0)}}</style>
<div id=m><span>STREAMING BITS ON MILLIONS OF SITES</span>
<span>STREAMING BITS ON MILLIONS OF SITES</span><span>STREAMING BITS ON MILLIONS OF SITES</span>
<span>STREAMING BITS ON MILLIONS OF SITES</span></div>""",
    # A page without animations: the baseline.
    "idle": "<!DOCTYPE html><body style='margin:0;background:#222'><p style='color:#fff'>idle</p>",
}


async def _cpu_seconds(cdp: Any) -> dict[str, float]:
    """The cumulative CPU time of the Chromium processes by type, from CDP's
    `SystemInfo.getProcessInfo`."""
    info = await cdp.send("SystemInfo.getProcessInfo")
    total: dict[str, float] = {}
    for process in info["processInfo"]:
        total[process["type"]] = total.get(process["type"], 0.0) + process["cpuTime"]
    return total


async def measure_cpu(
    target: str, javascript: bool, scroll_y: int, seconds: float, viewport: tuple[int, int]
) -> dict[str, Any]:
    """Loads the target (a name of `SYNTHETIC_PAGES`, or a fixture of
    `fixtures/js`, or of `fixtures/pages` without `javascript`; the page
    runs scripts in all cases), scrolls to `scroll_y`, waits 2 s and measures
    the CPU time of the Chromium processes over `seconds`. Returns the CPU
    use as a share of one core per process type and the number of running
    animations. The page has no `requestAnimationFrame` loop of its own: such
    a loop makes Chromium draw 60 frames per second."""
    async with browser.session(viewport, False, javascript=True) as (chromium, context, page):
        if target in SYNTHETIC_PAGES:
            await page.set_content(SYNTHETIC_PAGES[target])
        else:
            directory = paths.fixture_directory(target, javascript)
            meta = read_meta(directory)
            await Replayer(Fixture(directory)).attach(context, page)
            await page.goto(meta.url, wait_until="load", timeout=60_000)
            await browser.wait_for_network_idle(page, JS_IDLE_TIMEOUT_MS)
            await page.wait_for_timeout(JS_SETTLE_MS)
        await browser.wait_for_fonts(page)
        await page.evaluate(f"window.scrollTo(0, {scroll_y})")
        await page.wait_for_timeout(2000)
        cdp = await chromium.new_browser_cdp_session()
        before = await _cpu_seconds(cdp)
        await page.wait_for_timeout(int(seconds * 1000))
        after = await _cpu_seconds(cdp)
        running = await page.evaluate(
            "document.getAnimations().filter((a) => a.playState === 'running').length"
        )
    cpu = {kind: round(100 * (after[kind] - before.get(kind, 0.0)) / seconds, 1) for kind in after}
    return {
        "target": target,
        "scroll_y": scroll_y,
        "seconds": seconds,
        "cpu_percent_of_one_core": cpu,
        "cpu_total": round(sum(cpu.values()), 1),
        "running_animations": running,
    }


def animation_cpu(
    targets: list[str], javascript: bool, scroll_y: int, seconds: float, out: Path
) -> int:
    """Measures the CPU use of Chromium for each target (see `measure_cpu`),
    prints a table and writes `OUT/cpu.json`."""
    results = []
    print("target scroll_y running cpu_total% by_process")
    for target in targets:
        try:
            result = asyncio.run(measure_cpu(target, javascript, scroll_y, seconds, (1280, 800)))
        except FileNotFoundError:
            log.error("%s: not a synthetic page and no fixture.json of a fixture", target)
            return 1
        results.append(result)
        print(
            f"{target} {scroll_y} {result['running_animations']} "
            f"{result['cpu_total']} {result['cpu_percent_of_one_core']}"
        )
    out.mkdir(parents=True, exist_ok=True)
    (out / "cpu.json").write_text(json.dumps(results, indent=1) + "\n", encoding="utf-8")
    return 0
