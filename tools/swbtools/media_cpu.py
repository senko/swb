"""`swbtools media-cpu`: the CPU time and the memory of Chromium while it plays
a media file (M14 step 1, docs/media-diagnosis.md section 6).

The file is served by the probe tool's case server (`Range` support), as a
`<video>` that fills the window (`object-fit: contain`), muted, played from
the start. After a warm-up the tool measures three phases, each over
`--seconds`: playing, paused (after `pause()`) and ended (after a seek to the
end). For each phase it reports the CPU time of the Chromium processes by type
as a share of one core (CDP `SystemInfo.getProcessInfo`, as `animation-cpu`
does) and the resident memory of all of them (`VmRSS` of `/proc/PID/status`,
when the CDP process ids are operating system ids). For the playing phase it
also reports the media time that passed against the wall time, and the
frames that the player decoded and dropped (`getVideoPlaybackQuality()`).

The browser is the headless shell of Playwright, so the compositor and the
raster run in software and are part of the CPU time (the "gpu" and "renderer"
types).
"""

import asyncio
import json
import logging
import tempfile
from pathlib import Path
from typing import Any

from swbtools import browser
from swbtools.probe_server import CaseServer

log = logging.getLogger(__name__)

# The longest wait for `canplay` and for `ended`, so that a file that does
# not play (or never ends) stops the command instead of hanging it.
_EVENT_TIMEOUT_S = 120

_PAGE = """<!DOCTYPE html><meta charset=utf-8>
<style>html,body{{margin:0;background:#000;overflow:hidden}}
video{{display:block;width:100vw;height:100vh;object-fit:contain}}</style>
<video id=v src="{name}" muted preload=auto{attrs}></video>"""

_QUALITY_JS = """
() => {
  const q = v.getVideoPlaybackQuality();
  return {total: q.totalVideoFrames, dropped: q.droppedVideoFrames, time: v.currentTime,
          wall: performance.now() / 1000, ended: v.ended, paused: v.paused, rs: v.readyState,
          w: v.videoWidth, h: v.videoHeight, dur: v.duration};
}
"""


async def _cpu_seconds(cdp: Any) -> tuple[dict[str, float], dict[str, int]]:
    """The cumulative CPU time and the resident memory (KiB) of the Chromium
    processes by type."""
    info = await cdp.send("SystemInfo.getProcessInfo")
    cpu: dict[str, float] = {}
    rss: dict[str, int] = {}
    for process in info["processInfo"]:
        kind = process["type"]
        cpu[kind] = cpu.get(kind, 0.0) + process["cpuTime"]
        try:
            for line in Path(f"/proc/{process['id']}/status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    rss[kind] = rss.get(kind, 0) + int(line.split()[1])
        except OSError:
            pass
    return cpu, rss


async def _phase(page: Any, cdp: Any, seconds: float) -> dict[str, Any]:
    """CPU use (% of one core by process type), memory and the player's
    counters over `seconds`."""
    cpu0, _ = await _cpu_seconds(cdp)
    q0 = await page.evaluate(_QUALITY_JS)
    await page.wait_for_timeout(int(seconds * 1000))
    cpu1, rss = await _cpu_seconds(cdp)
    q1 = await page.evaluate(_QUALITY_JS)
    percent = {k: round(100 * (cpu1[k] - cpu0.get(k, 0.0)) / seconds, 1) for k in cpu1}
    wall = q1["wall"] - q0["wall"]
    return {
        "cpu_percent_of_one_core": percent,
        "cpu_total": round(sum(percent.values()), 1),
        "rss_mib": {k: round(v / 1024) for k, v in rss.items()},
        "rss_total_mib": round(sum(rss.values()) / 1024),
        "media_seconds": round(q1["time"] - q0["time"], 2),
        "wall_seconds": round(wall, 2),
        "frames": q1["total"] - q0["total"],
        "dropped": q1["dropped"] - q0["dropped"],
        "paused": q1["paused"],
        "ended": q1["ended"],
    }


async def measure(
    file: Path, viewport: tuple[int, int], seconds: float, attrs: str = ""
) -> dict[str, Any]:
    """Plays `file` in a window of `viewport` and measures the three phases."""
    with tempfile.TemporaryDirectory(prefix="swb-media-cpu-") as tmp:
        folder = Path(tmp) / "case-1"
        folder.mkdir()
        name = f"media{file.suffix}"
        (folder / name).symlink_to(file.resolve())
        (folder / "case-1.html").write_text(_PAGE.format(name=name, attrs=attrs), encoding="utf-8")
        server = CaseServer(Path(tmp))
        server.media_folders.add("case-1")
        try:
            async with browser.session(viewport, False, javascript=True) as (chromium, _, page):
                await page.goto(f"{server.origin}/case-1/case-1.html", wait_until="load")
                await asyncio.wait_for(
                    page.evaluate(
                        "new Promise(r => v.readyState >= 3 ? r() "
                        ": v.addEventListener('canplay', r, {once: true}))"
                    ),
                    _EVENT_TIMEOUT_S,
                )
                info = await page.evaluate(_QUALITY_JS)
                await page.evaluate("v.play()")
                await page.wait_for_timeout(3000)  # warm-up
                cdp = await chromium.new_browser_cdp_session()
                playing = await _phase(page, cdp, seconds)
                await page.evaluate("v.pause()")
                await page.wait_for_timeout(1500)
                paused = await _phase(page, cdp, seconds)
                await page.evaluate("(() => { v.currentTime = v.duration - 0.05; v.play(); })()")
                await asyncio.wait_for(
                    page.evaluate(
                        "new Promise(r => v.ended ? r() "
                        ": v.addEventListener('ended', r, {once: true}))"
                    ),
                    _EVENT_TIMEOUT_S,
                )
                await page.wait_for_timeout(1500)
                ended = await _phase(page, cdp, seconds)
        finally:
            server.close()
    return {
        "file": file.name,
        "size_bytes": file.stat().st_size,
        "window": list(viewport),
        "video": f"{info['w']}x{info['h']}",
        "duration": info["dur"],
        "playing": playing,
        "paused": paused,
        "ended": ended,
    }


def media_cpu(files: list[Path], viewport: tuple[int, int], seconds: float, out: Path) -> int:
    """Measures each file (see the module comment), prints one JSON object per
    file and writes the list to `out`."""
    results = []
    for file in files:
        if not file.is_file():
            log.error("%s: not a file", file)
            return 1
        try:
            result = asyncio.run(measure(file, viewport, seconds))
        except TimeoutError:
            log.error(
                "%s: no `canplay` or `ended` within %d s (not playable?)", file, _EVENT_TIMEOUT_S
            )
            return 1
        results.append(result)
        print(json.dumps(result))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, indent=1) + "\n", encoding="utf-8")
    return 0
