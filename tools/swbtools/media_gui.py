"""`swbtools media-gui`: measure media playback in swb's GUI on a private X
server (M14 feature 10b, docs/testing.md "Media in the GUI").

The tool starts its own `Xvfb` (a free display number, never the desktop's),
runs the release media binary there with the frame log, one case at a time:
a file (1080p, the Wikimedia file, 4K, the M4A, a small file), a state
(playing, paused after playing, ended, scrolled out, `display: none` paused,
a hidden window, a seek series, the default controls with the pointer moving
or still) and a layout (the whole window, or a 640 x 360 box). Per case it
reports the CPU of the process (all threads, from `/proc`), the peak and
settled resident memory, the frames shown and dropped, the wake-ups of the
event loop per second, the time to `loadedmetadata` and to the first frame,
the time from a seek to its frame, and (with `--audio pw-private`) the largest
`media - video` of the frame log's `sync` lines.

Without `--audio` the run is silent: `--no-audio-output`, and `PIPEWIRE_RUNTIME_DIR`
and `XDG_RUNTIME_DIR` on an empty directory, so that no process of the tool can
reach the owner's PipeWire daemon. With `--audio pw-private` the tool starts
the private daemon of `swbtools pw-private` (its isolation proof first; when
the proof fails nothing runs), runs swb with the client environment of that
daemon and links swb's stream to the null sink as `pw-private watch` does.
Every swb run is in a memory-capped systemd scope. `--check` compares the
worst of the runs with `BUDGETS` (the one place that holds them) and exits 1
on a failure.

The page of a case is a script that makes the element, records the times of
its events with `Date.now()` and, in a silent run, hands them to the tool
through the automation server (`page.evaluate`, after the measured window);
a run with sound has no automation server (it keeps swb silent), so it has
no marks and measures the frame log only.
"""

import contextlib
import dataclasses
import fnmatch
import functools
import itertools
import json
import logging
import os
import re
import shutil
import signal
import statistics
import subprocess
import tempfile
import threading
import time
from collections.abc import Iterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from swbtools import automation, paths, pw_private, swb
from swbtools.gui_latency import exit_on_sigterm, monotonic, start_xvfb, stop
from swbtools.procfs import process_cpu_s, status_value, thread_cpu
from swbtools.xinput import XDisplay, XError

log = logging.getLogger(__name__)

WINDOW = (1280, 720)
"""The window size of every run (the toolbar included)."""

MEMORY_CAP = "4G"
"""The memory cap of every swb run: the 4K file needs well under 2 GiB."""

RUST_LOG = "warn,swb::gui=debug"
"""The log filter of a run; a run with sound adds the output's log."""

AUDIO_RUST_LOG = RUST_LOG + ",swb_media::output=debug"

SEEK_TARGETS = [10, 2.5, 17, 6, 13, 0.5, 19, 8, 4, 15]
"""The seek times (seconds) of a seek series: ten seeks, far and near, the
last four across a loop wrap of the 20 s files."""

SYNC_SEEK_GAP_S = 3
"""The seconds between the seeks that end a `sync` run."""

CLICK = (640, 400)
"""Where the tool clicks to give the page its user activation."""

POINTER_STILL = (300, 250)
POINTER_BOX = (200, 500, 220, 370)
"""The pointer of the controls cases: still at `POINTER_STILL`, or moving
every 0.3 s inside this rectangle (x0, x1, y0, y1), which lies in the box."""


@dataclass(frozen=True)
class MediaFile:
    """A media file of the cases."""

    path: Path | None
    fps: float | None
    kind: str


def media_files() -> dict[str, MediaFile]:
    """The files by the short names of the cases. `idle` is a page without
    media (the baseline of the memory budgets)."""
    real = paths.js_fixtures_dir() / "media"
    return {
        "idle": MediaFile(None, None, "none"),
        "1080p": MediaFile(real / "mux-1080p30-20s.mp4", 30.0, "video"),
        "wiki": MediaFile(real / "wikimedia-web-browser-demo.webm", 25.0, "video"),
        # avg_frame_rate 77490000/3225877; 47.95 is the time base.
        "4k": MediaFile(real / "mux-highest.mp4", 77490000 / 3225877, "video"),
        "m4a": MediaFile(real / "mux-audio.m4a", None, "audio"),
        "small": MediaFile(paths.fixtures_dir() / "media" / "mp4-h264-aac-long.mp4", 25.0, "video"),
    }


STATES = (
    "idle",  # no media
    "playing",
    "paused",  # playing, then paused after 0.5 s
    "ended",  # seeks to 0.5 s before the end, plays to it
    "out",  # playing, scrolled out of view
    "display-none",  # playing, then paused, `display: none`
    "hidden",  # playing, then the window is covered
    "hidden-paused",  # paused, then the window is covered
    "unmapped",  # playing, then the window is unmapped
    "seek",  # paused, ten seeks
    "seek-playing",  # playing, ten seeks
    "moving",  # playing, the pointer moves in the box, no controls
    "still",  # playing, the pointer moves in once, no controls
    "controls-moving",  # the same with `controls`
    "controls-still",
    "controls-paused",  # paused with `controls`, the pointer in the box
    "sync",  # playing with sound, ten seeks at the end (--audio pw-private)
)
LAYOUTS = ("full", "box")


@dataclass(frozen=True)
class Case:
    """One case: the page and how long to measure."""

    file: str
    state: str
    layout: str
    seconds: float | None = None

    @property
    def name(self) -> str:
        """`FILE:STATE:LAYOUT`, the key of the budgets."""
        return f"{self.file}:{self.state}:{self.layout}"

    @property
    def controls(self) -> bool:
        """Whether the element has the default controls."""
        return self.state.startswith("controls-")

    @property
    def base_state(self) -> str:
        """The state without the controls."""
        return "paused" if self.state == "controls-paused" else self.state.removeprefix("controls-")

    def timing(self) -> tuple[float, float]:
        """The settle time (the page's start, the state reached) and the
        measured seconds."""
        match self.state:
            case "idle":
                settle, seconds = 2.0, 5.0
            case (
                "paused"
                | "ended"
                | "display-none"
                | "hidden"
                | "hidden-paused"
                | "unmapped"
                | "controls-paused"
            ):
                settle, seconds = 4.0, 5.0
            case "controls-still" | "still":
                settle, seconds = 6.0, 10.0
            case "seek" | "seek-playing":
                settle, seconds = 3.0, 25.0  # the limit: the series ends the run
            case "sync":
                settle, seconds = 3.0, 215.0
            case _:
                settle, seconds = 4.0, 10.0
        return settle, self.seconds if self.seconds is not None else seconds


def parse_case(text: str, files: dict[str, MediaFile] | None = None) -> Case:
    """Parses `FILE:STATE:LAYOUT[:SECONDS]`. Raises ValueError."""
    files = files if files is not None else media_files()
    parts = text.split(":")
    if len(parts) not in (3, 4):
        raise ValueError(f"{text!r}: expected FILE:STATE:LAYOUT[:SECONDS]")
    file, state, layout = parts[:3]
    if file not in files:
        raise ValueError(f"{text!r}: unknown file {file!r} (one of {', '.join(files)})")
    if state not in STATES:
        raise ValueError(f"{text!r}: unknown state {state!r} (one of {', '.join(STATES)})")
    if layout not in LAYOUTS:
        raise ValueError(f"{text!r}: unknown layout {layout!r} (one of {', '.join(LAYOUTS)})")
    if (file == "idle") != (state == "idle"):
        raise ValueError(f"{text!r}: the file and the state `idle` go together")
    seconds = float(parts[3]) if len(parts) == 4 else None
    return Case(file, state, layout, seconds)


BUDGET_CASES = [
    "idle:idle:full:5",
    "1080p:playing:full:20",
    "wiki:playing:full:20",
    "4k:playing:full:10",
    "1080p:paused:full:5",
    "1080p:ended:full:5",
    "1080p:hidden:full:5",
    "1080p:hidden-paused:full:5",
    "1080p:display-none:full:5",
    "wiki:paused:full:5",
    "wiki:ended:full:5",
    "4k:paused:full:5",
    "4k:ended:full:5",
    "m4a:playing:full:10",
    "1080p:seek:full",
    "1080p:seek-playing:full",
    "1080p:moving:box:10",
    "1080p:controls-moving:box:10",
    "1080p:still:box:10",
    "1080p:controls-still:box:10",
    "1080p:controls-paused:box:5",
]
SUITES = {
    "budgets": BUDGET_CASES,
    "audio": ["idle:idle:full:5", "m4a:playing:full:10", "1080p:playing:full:20"],
    "sync": ["1080p:sync:full"],
}


# ----- Pages -----


def page_html(case: Case, files: dict[str, MediaFile], sound: bool) -> str:
    """The page of a case. `sound`: the element is not muted and starts on a
    click (the user activation); else it is muted and `autoplay`."""
    media = files[case.file]
    if media.path is None:
        return (
            "<!doctype html><meta charset=utf-8>"
            "<style>html,body{margin:0;background:#888}</style><p>idle</p>"
        )
    state = case.base_state
    if case.layout == "full":
        style = (
            "html,body{margin:0;background:#222;overflow:hidden}"
            "video{display:block;width:100vw;height:100vh;object-fit:contain}"
        )
    else:
        style = (
            "html,body{margin:0;background:#888}"
            "video{display:block;margin:100px 0 0 100px;width:640px;height:360px}"
        )
    if state == "out":
        style += "body{height:6000px}video{margin-top:3000px}"
    if state == "display-none":
        style += "video{display:none}"
    script_state = {
        "paused": "once('playing',()=>setTimeout(()=>v.pause(),500));",
        "hidden-paused": "once('playing',()=>setTimeout(()=>v.pause(),500));",
        "display-none": "once('playing',()=>setTimeout(()=>v.pause(),500));",
        "ended": (
            "v.loop=false;once('loadedmetadata',()=>{v.currentTime=Math.max(0,v.duration-0.5)});"
        ),
        "seek": SEEK_JS.replace("AUTO", "false").replace("DELAY", "1000"),
        "seek-playing": SEEK_JS.replace("AUTO", "true").replace("DELAY", "1000"),
        "sync": SYNC_JS.replace("DELAY", str(int(case.timing()[1] - 35) * 1000)),
    }.get(state, "")
    autoplay = state not in ("seek", "sync")
    if sound and not case.controls:
        start = "document.addEventListener('click',()=>{mark('click');v.play()});"
    elif autoplay:
        start = "v.autoplay=true;"
    else:
        start = ""
    loop = "false" if state == "ended" else "true"
    muted = "false" if sound and not case.controls else "true"
    element = "audio" if media.kind == "audio" else "video"
    controls = "v.controls=true;" if case.controls else ""
    return (
        f"<!doctype html><meta charset=utf-8><style>{style}</style>"
        "<body><script>"
        "const m=window.__m={ev:{},seeks:[]};"
        "const mark=n=>(m.ev[n]=m.ev[n]||[]).push(Date.now());"
        f"const v=window.__v=document.createElement('{element}');"
        "for(const e of ['loadstart','loadedmetadata','loadeddata','canplay','playing',"
        "'pause','ended','seeked','error','waiting'])v.addEventListener(e,()=>mark(e));"
        "const once=(e,f)=>v.addEventListener(e,f,{once:true});"
        f"v.muted={muted};v.loop={loop};v.preload='auto';{controls}{script_state}{start}"
        "m.t0=Date.now();"
        f"v.src={json.dumps('file://' + str(media.path))};"
        "document.body.appendChild(v);"
        "</script>"
    )


SEEK_JS = (
    f"const T={SEEK_TARGETS};let k=0;"
    "function next(){if(k>=T.length){m.seekDone=true;return}"
    "const s={start:Date.now(),target:T[k++]};m.seeks.push(s);"
    "once('seeked',()=>{s.seeked=Date.now();setTimeout(next,DELAY)});v.currentTime=s.target}"
    "v.autoplay=AUTO;once('loadeddata',()=>setTimeout(next,DELAY));"
)
"""The seek series: each seek 1 s after the last one's `seeked`."""

SYNC_JS = (
    f"const T={SEEK_TARGETS};"
    "once('playing',()=>setTimeout(()=>{let k=0;const id=setInterval(()=>{"
    "if(k>=T.length){clearInterval(id);m.seekDone=true;return}v.currentTime=T[k++]},"
    f"{SYNC_SEEK_GAP_S * 1000})}},DELAY));"
)
"""The `sync` page: after DELAY ms of playing, ten seeks 3 s apart."""

STATE_JS = (
    "({m:window.__m,v:window.__v&&{paused:__v.paused,ended:__v.ended,t:__v.currentTime,"
    "rs:__v.readyState,ns:__v.networkState,err:__v.error?__v.error.code:null,"
    "w:__v.videoWidth,h:__v.videoHeight,d:__v.duration}})"
)


# ----- The frame log -----

_FRAME = re.compile(
    r"\] frame at=([0-9.]+) render=([0-9.]+) compose=([0-9.]+) present=([0-9.]+)"
    r"(?: full=(\d+) rects=(\d+) pixels=(\d+))?"
)
_MEDIA = re.compile(
    r"\] media at=([0-9.]+) wakeups=(\d+) shown=(\d+) dropped=(\d+) decoded=(\d+) converted=(\d+)"
)
_SYNC = re.compile(r"\] sync at=([0-9.]+) video=([0-9.]+) media=([0-9.]+) audio=(\d)")
_STREAMING = re.compile(r"streaming \d+ ms after the connect")


@dataclass(frozen=True)
class FrameLine:
    """A presented frame (`frame`); `pixels` are the pixels rasterized."""

    at: float
    render: float
    compose: float
    present: float
    pixels: int


@dataclass(frozen=True)
class MediaLine:
    """The counters of the video pipelines (`media`)."""

    at: float
    wakeups: int
    shown: int
    dropped: int
    decoded: int
    converted: int


@dataclass(frozen=True)
class SyncLine:
    """The frame drawn and the element's time (`sync`)."""

    at: float
    video: float
    media: float
    audio: bool

    @property
    def lag_ms(self) -> float:
        """`media - video` in milliseconds."""
        return (self.media - self.video) * 1000


@dataclass
class Log:
    """What the frame log of a run holds."""

    frames: list[FrameLine] = field(default_factory=list)
    media: list[MediaLine] = field(default_factory=list)
    sync: list[SyncLine] = field(default_factory=list)
    streaming: int = 0
    other: list[str] = field(default_factory=list)


def parse_log(lines: list[str]) -> Log:
    """Parses the stderr of a run. Lines of other kinds (a warning, the
    output's log) are kept in `other`, except the `streaming` lines, which
    are counted."""
    result = Log()
    for line in lines:
        if match := _FRAME.search(line):
            at, render, compose, present, _full, _rects, pixels = match.groups()
            result.frames.append(
                FrameLine(
                    float(at), float(render), float(compose), float(present), int(pixels or 0)
                )
            )
        elif match := _MEDIA.search(line):
            result.media.append(MediaLine(float(match[1]), *(int(match[i]) for i in range(2, 7))))
        elif match := _SYNC.search(line):
            result.sync.append(
                SyncLine(float(match[1]), float(match[2]), float(match[3]), match[4] == "1")
            )
        elif _STREAMING.search(line):
            result.streaming += 1
        elif " batch start=" not in line:
            result.other.append(line.rstrip())
    return result


# ----- Numbers from the log and from the page -----


MEDIA_LINE_STEP_S = 0.5
"""A `media` line is printed when a number changed, at most twice a second."""


def counters(media: list[MediaLine], t0: float, t1: float) -> dict[str, Any]:
    """The counters of the pipelines over the window [t0, t1] from the
    `media` lines: the last line at or before each end. The resolution is
    `MEDIA_LINE_STEP_S`: a change in the last 0.5 s of the window is missed
    (the first line after the end is not used: at the end of a run the
    pipeline's last line is its flush at the close, which is not the window's;
    a paused run showed one). No line in the window means no change."""
    before = next((m for m in reversed(media) if m.at <= t0), None)
    after = next((m for m in reversed(media) if m.at <= t1), None)
    if after is None:
        return {
            "shown": 0,
            "dropped": 0,
            "decoded": 0,
            "converted": 0,
            "wakeups": None,
            "span": t1 - t0,
        }
    base = before or MediaLine(t0, 0, 0, 0, 0, 0)
    changed = before is not after
    span = after.at - base.at if changed and before is not None else t1 - t0
    return {
        "shown": after.shown - base.shown,
        "dropped": after.dropped - base.dropped,
        "decoded": after.decoded - base.decoded,
        "converted": after.converted - base.converted,
        "wakeups": (after.wakeups - base.wakeups) if before is not None else None,
        "span": span,
    }


def sync_stats(
    lines: list[SyncLine], t0: float, t1: float, period_ms: float
) -> dict[str, Any] | None:
    """`media - video` of the `sync` lines in [t0, t1]: the count, the
    minimum, the mean and the maximum (ms), the lines over `period_ms`, and
    the share of lines whose clock was the audio clock, and the `jumps` (the
    times where the frame's time moved by more than half a second from what
    the wall time explains: a seek or a loop). None without lines."""
    window = [s for s in lines if t0 <= s.at <= t1]
    if not window:
        return None
    lags = [s.lag_ms for s in window]
    jumps = sum(
        1 for a, b in itertools.pairwise(window) if abs((b.video - a.video) - (b.at - a.at)) > 0.5
    )
    return {
        "jumps": jumps,
        "n": len(window),
        "min_ms": round(min(lags), 2),
        "mean_ms": round(statistics.fmean(lags), 2),
        "max_ms": round(max(lags), 2),
        "max_at_s": round(window[lags.index(max(lags))].at - t0, 1),
        "over_period": sum(1 for lag in lags if lag > period_ms),
        "audio_share": round(sum(1 for s in window if s.audio) / len(window), 3),
    }


def first_frame_after(frames: list[FrameLine], at: float) -> FrameLine | None:
    """The first presented frame that rasterized pixels at or after `at`."""
    return next((f for f in frames if f.at >= at and f.pixels > 0), None)


def page_time(epoch_ms: float, offset: float) -> float:
    """A `Date.now()` value on the clock of the frame log (`offset` is the
    `CLOCK_MONOTONIC` time minus the epoch time, in seconds)."""
    return epoch_ms / 1000 + offset


def startup_metrics(
    marks: dict[str, Any], frames: list[FrameLine], offset: float
) -> dict[str, Any]:
    """The start of the element: milliseconds from the script's `src`
    assignment (a local file: the bytes are there) to `loadedmetadata`, to
    `loadeddata`, and to the first frame presented after `loadeddata`."""
    events, t0 = marks.get("ev", {}), marks.get("t0")
    result: dict[str, Any] = {}
    if t0 is None:
        return result
    for name, key in (("loadedmetadata", "loadedmetadata_ms"), ("loadeddata", "loadeddata_ms")):
        if events.get(name):
            result[key] = events[name][0] - t0
    if events.get("loadeddata"):
        frame = first_frame_after(frames, page_time(events["loadeddata"][0], offset))
        if frame is not None:
            result["first_frame_ms"] = round((frame.at - page_time(t0, offset)) * 1000, 1)
    return result


def seek_metrics(
    marks: dict[str, Any], frames: list[FrameLine], offset: float, paused: bool
) -> dict[str, Any]:
    """The time from each seek to `seeked` and to its frame. For a paused
    element the frame is the first one presented after the assignment (no
    other frame is drawn); for a playing one the first presented after
    `seeked`, since frames come all the time."""
    to_seeked: list[float] = []
    to_frame: list[float] = []
    for seek in marks.get("seeks", []):
        if "seeked" not in seek:
            continue
        to_seeked.append(seek["seeked"] - seek["start"])
        start = page_time(seek["start" if paused else "seeked"], offset)
        frame = first_frame_after(frames, start)
        if frame is not None:
            to_frame.append((frame.at - page_time(seek["start"], offset)) * 1000)
    result: dict[str, Any] = {"seeks": len(marks.get("seeks", [])), "seeked": len(to_seeked)}
    if to_seeked:
        result["seek_to_seeked_ms"] = round(max(to_seeked), 1)
    if to_frame:
        result["seek_to_frame_ms"] = round(max(to_frame), 1)
        result["seek_to_frame_median_ms"] = round(statistics.median(to_frame), 1)
    return result


def verify_state(case: Case, page: dict[str, Any] | None) -> list[str]:
    """The reasons why the page did not reach the state of the case (empty
    when it did). `page` is `STATE_JS`' value; None (a run with sound has no
    automation server) is not checked."""
    if page is None or case.file == "idle":
        return []
    v = page.get("v") or {}
    problems: list[str] = []
    if v.get("err") is not None:
        problems.append(f"media error {v['err']}")
    if not v.get("rs"):
        problems.append("no metadata")
    state = case.base_state
    time_now = v.get("t") or 0
    if state in ("paused", "display-none", "hidden-paused") and not v.get("paused"):
        problems.append("not paused")
    if state in ("paused", "display-none", "hidden-paused") and time_now <= 0:
        problems.append("paused at 0")
    if state == "ended" and not v.get("ended"):
        problems.append("not ended")
    if state in ("playing", "out", "hidden", "unmapped", "moving", "still", "seek-playing"):
        if v.get("paused") and state not in ("hidden", "unmapped", "out"):
            problems.append("paused")
        if time_now <= 0:
            problems.append("time 0")
    if state in ("seek", "seek-playing", "sync") and not (page.get("m") or {}).get("seekDone"):
        problems.append("the seek series did not finish")
    return problems


# ----- The process -----


def build_command(
    binary: Path,
    args: list[str],
    unset: list[str],
    set_env: dict[str, str],
    systemd: bool,
    cap: str = MEMORY_CAP,
) -> list[str]:
    """The command line of a swb run: `env` (the variables of the run) under
    `systemd-run --scope` with a memory cap. `systemd-run --scope` and `env`
    exec, so the PID of the process is the PID of swb."""
    command = ["env"]
    for name in unset:
        command += ["-u", name]
    command += [f"{k}={v}" for k, v in set_env.items()]
    command += [str(binary), *args]
    if systemd:
        command = [
            "systemd-run", "--user", "--scope", "--quiet", "--collect",
            "-p", f"MemoryMax={cap}", "-p", "MemorySwapMax=0", "--", *command,
        ]  # fmt: skip
    return command


def have_systemd_scope() -> bool:
    """Whether a systemd user manager can run a scope."""
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    return bool(
        runtime and Path(runtime, "systemd", "private").is_socket() and shutil.which("systemd-run")
    )


@functools.cache
def warn_uncapped() -> None:
    """Warns once per process that swb runs without the memory cap."""
    log.warning("no working systemd user scope: swb runs without the memory cap (%s)", MEMORY_CAP)


def drain(stream: Any) -> None:
    """Reads and drops what is left of a pipe, so that a process that prints
    much cannot block on it."""
    for _ in stream:
        pass


class LogReader(threading.Thread):
    """Reads the stderr of swb into a list."""

    def __init__(self, stream: Any) -> None:
        super().__init__(daemon=True)
        self.stream = stream
        self.lines: list[str] = []

    def run(self) -> None:
        for line in self.stream:
            self.lines.append(line)


class LinkWatcher(threading.Thread):
    """Makes the links of `pw-private watch` (swb's stream to the null sink)
    until stopped: often for `fast_s` seconds after `kick`, then once a
    second."""

    def __init__(self, directory: Path) -> None:
        super().__init__(daemon=True)
        self.directory = directory
        self.stopped = threading.Event()
        self.fast_until = 0.0

    def kick(self, fast_s: float = 10.0) -> None:
        """Link at a short interval for the next `fast_s` seconds."""
        self.fast_until = time.monotonic() + fast_s

    def run(self) -> None:
        while not self.stopped.is_set():
            with contextlib.suppress(Exception):
                pw_private.link_once(self.directory)
            self.stopped.wait(0.05 if time.monotonic() < self.fast_until else 1.0)


@contextlib.contextmanager
def private_daemon(directory: Path) -> Iterator[tuple[dict[str, Any], LinkWatcher]]:
    """The private PipeWire daemon with its isolation proof, and the links;
    stopped by its PID at the end. Raises `pw_private.IsolationError` when
    the proof fails (nothing then runs)."""
    report = pw_private.start(directory)
    watcher = LinkWatcher(directory)
    watcher.start()
    try:
        yield report, watcher
    finally:
        watcher.stopped.set()
        watcher.join(5)
        pw_private.stop(directory)


@dataclass
class Options:
    """What to run."""

    binary: Path
    runs: int = 1
    audio: str = "none"
    seconds: float | None = None
    tmp: Path = field(default_factory=lambda: paths.repo_root() / "tmp")
    daemon_dir: Path | None = None
    watcher: LinkWatcher | None = None
    log_dir: Path | None = None
    """Where to write the stderr of each run (the frame log)."""


def snapshot(pid: int) -> dict[str, Any]:
    """CPU seconds, resident memory (KiB) and the voluntary context switches
    of the main thread (a wake-up of the event loop each) of a process."""
    status = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    task = Path(f"/proc/{pid}/task/{pid}/status").read_text(encoding="utf-8")

    return {
        "t": monotonic(),
        "cpu": process_cpu_s(pid),
        "rss_kib": status_value(status, "VmRSS"),
        "peak_kib": status_value(status, "VmHWM"),
        "switches": status_value(task, "voluntary_ctxt_switches"),
        "threads": status_value(status, "Threads"),
        "by_thread": thread_cpu(pid),
    }


def with_seconds(case: Case, seconds: float | None) -> Case:
    """The case with `--seconds` as its measured seconds, unless it has its
    own. The page of a `sync` case schedules its seeks from the same number."""
    if seconds is None or case.seconds is not None:
        return case
    return dataclasses.replace(case, seconds=seconds)


def run_case(case: Case, options: Options, files: dict[str, MediaFile]) -> dict[str, Any]:
    """Runs one case once on a fresh Xvfb and a fresh swb. Returns its
    result (see `docs/testing.md`)."""
    sound = options.audio == "pw-private" or case.file == "m4a" or case.state == "sync"
    sound = sound and not case.controls and case.file != "idle"
    case = with_seconds(case, options.seconds)  # one source for the page and the window
    settle, seconds = case.timing()
    work = Path(tempfile.mkdtemp(prefix="media-gui-", dir=options.tmp))
    work.chmod(0o700)
    page = work / "page.html"
    page.write_text(page_html(case, files, sound), encoding="utf-8")
    result: dict[str, Any] = {"case": case.name, "audio": options.audio, "seconds": seconds}
    xvfb, display_name = start_xvfb()
    gui: subprocess.Popen[str] | None = None
    display: XDisplay | None = None
    try:
        display = XDisplay(display_name)
        gui = _start_swb(options, display_name, work, f"file://{page}")
        reader = LogReader(gui.stderr)
        reader.start()
        browser = None
        if options.audio == "none":
            browser = automation.Browser(automation.read_address(gui), gui)
        # Nobody reads swb's stdout from here on: drain it, so that a page
        # that prints much cannot block swb.
        threading.Thread(target=drain, args=(gui.stdout,), daemon=True).start()
        window = display.wait_for_window(30.0)
        display.move_resize(window, 0, 0, *WINDOW)
        display.motion(0, 0)
        if browser is not None:
            browser.navigate(f"file://{page}")
            browser.wait_for_load(60_000)
        marks = _play(case, options, display, browser, gui.pid, window, sound, settle, seconds)
        result.update(marks["result"])
        times = marks["times"]
        page_state = _page_state(browser)
        gui_status = gui.poll()
        result["alive"] = int(gui_status is None)
    finally:
        if display is not None:
            display.close()
        if gui is not None:
            stop(gui)
        stop(xvfb)
        shutil.rmtree(work, ignore_errors=True)
    if options.log_dir is not None:
        options.log_dir.mkdir(parents=True, exist_ok=True)
        name = f"{case.name.replace(':', '-')}-{options.audio}-{time.strftime('%H%M%S')}.log"
        (options.log_dir / name).write_text("".join(reader.lines), encoding="utf-8")
    log_data = parse_log(reader.lines)
    result.update(_log_metrics(case, files, log_data, times, page_state, options))
    problems = verify_state(case, page_state)
    if gui_status is not None:
        problems.append(f"swb exited with {gui_status}")
    problems += [line[-120:] for line in log_data.other if "Uncaught" in line]
    plays_sound = sound and options.audio == "pw-private"
    if plays_sound and case.base_state in ("playing", "sync") and not log_data.streaming:
        problems.append("the audio output did not stream")
    result["problems"] = problems
    result["stderr_other"] = log_data.other[:5]
    return result


def _start_swb(options: Options, display: str, work: Path, url: str) -> subprocess.Popen[str]:
    """Starts swb (release media binary) on the private display."""
    set_env = {"DISPLAY": display, "XDG_SESSION_TYPE": "x11"}
    if options.audio == "pw-private":
        assert options.daemon_dir is not None
        set_env["RUST_LOG"] = AUDIO_RUST_LOG
        for line in pw_private.env_lines(options.daemon_dir):
            name, _, value = line.partition("=")
            set_env[name] = value
        args = ["--js", url]
        if options.watcher is not None:
            options.watcher.kick()
    else:
        runtime = work / "empty-runtime"
        runtime.mkdir(mode=0o700)
        set_env.update(
            RUST_LOG=RUST_LOG, PIPEWIRE_RUNTIME_DIR=str(runtime), XDG_RUNTIME_DIR=str(runtime)
        )
        args = ["--no-audio-output", "--js", "--remote-port", "0", "about:blank"]
    # No variable of the owner that names another PipeWire or Pulse server may
    # reach swb, whichever the audio mode (the set variables come after the
    # unset ones in `env`).
    unset = [name for name in os.environ if name.startswith("WAYLAND_")]
    unset += pw_private.OTHER_SERVER_NAMES
    systemd = have_systemd_scope()
    if not systemd:
        warn_uncapped()
    command = build_command(options.binary, args, unset, set_env, systemd)
    log.info("running %s", " ".join(command))
    return subprocess.Popen(
        command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=os.environ
    )


def _page_state(browser: automation.Browser | None) -> dict[str, Any] | None:
    """The page's marks and the element's state (silent runs only)."""
    if browser is None:
        return None
    try:
        answer = browser.call("page.evaluate", expression=STATE_JS, timeoutMs=10_000)
    except Exception as error:  # the process may be gone
        log.warning("cannot read the page's state: %s", error)
        return None
    value = answer.get("value")
    return value if isinstance(value, dict) else None


def _play(
    case: Case,
    options: Options,
    display: XDisplay,
    browser: automation.Browser | None,
    pid: int,
    window: int,
    sound: bool,
    settle: float,
    seconds: float,
) -> dict[str, Any]:
    """The timeline of a case after the load: the actions, the settle time,
    the measured window. Returns the numbers of the window and its times."""
    offset = monotonic() - time.time()
    t_click: float | None = None
    if sound:
        time.sleep(2.0)
        t_click = monotonic()
        display.motion(*CLICK)
        display.button(1, True)
        display.button(1, False)
        display.motion(0, 0)
    state = case.base_state
    if case.state in ("still", "controls-still"):
        time.sleep(1.0)
        display.motion(*POINTER_STILL)
        time.sleep(max(settle - 1.0, 0.0))
    elif state in ("hidden", "hidden-paused", "unmapped"):
        time.sleep(max(settle - 1.5, 0.0))
        if state != "unmapped":
            display.cover(3000, 1500)
        else:
            display.unmap(window)
        time.sleep(1.5)
    elif case.state == "controls-paused":
        time.sleep(1.0)
        display.motion(*POINTER_STILL)
        time.sleep(max(settle - 1.0, 0.0))
    else:
        time.sleep(settle)
    a = snapshot(pid)
    if case.state in ("moving", "controls-moving"):
        _jiggle(display, seconds)
    elif state in ("seek", "seek-playing"):
        _wait_for_seeks(browser, seconds)
    else:
        time.sleep(seconds)
    b = snapshot(pid)
    dt = b["t"] - a["t"]
    return {
        "times": {"a": a["t"], "b": b["t"], "offset": offset, "click": t_click},
        "result": {
            "window_s": round(dt, 2),
            "cpu_pct": round(100 * (b["cpu"] - a["cpu"]) / dt, 2),
            "rss_settled_mib": round(b["rss_kib"] / 1024, 1),
            "rss_peak_mib": round(b["peak_kib"] / 1024, 1),
            "switches_per_s": round((b["switches"] - a["switches"]) / dt, 1),
            "threads": b["threads"],
            "thread_cpu_pct": thread_shares(a["by_thread"], b["by_thread"], dt),
        },
    }


def thread_shares(a: dict[str, float], b: dict[str, float], dt: float) -> dict[str, float]:
    """The CPU use of each thread name between two `thread_cpu` reads, in
    percent of one core, for the names above 0.5 %, largest first. (A thread
    that ended in the window is missing from `b`, so its CPU is not counted.)"""
    shares = {name: 100 * (b[name] - a.get(name, 0.0)) / dt for name in b}
    kept = {name: round(share, 1) for name, share in shares.items() if share >= 0.5}
    return dict(sorted(kept.items(), key=lambda item: -item[1]))


def _jiggle(display: XDisplay, seconds: float) -> None:
    """Moves the pointer inside the box every 0.3 s for `seconds`."""
    x0, x1, y0, y1 = POINTER_BOX
    end = time.monotonic() + seconds
    i = 0
    while time.monotonic() < end:
        display.motion(x0 + (i * 37) % (x1 - x0), y0 + (i * 53) % (y1 - y0))
        i += 1
        time.sleep(0.3)


def _wait_for_seeks(browser: automation.Browser | None, limit: float) -> None:
    """Waits until the page's seek series is done (at most `limit` s)."""
    end = time.monotonic() + limit
    while time.monotonic() < end and browser is not None:
        time.sleep(0.5)
        state = _page_state(browser) or {}
        if (state.get("m") or {}).get("seekDone"):
            return


def _log_metrics(
    case: Case,
    files: dict[str, MediaFile],
    log_data: Log,
    times: dict[str, Any],
    page_state: dict[str, Any] | None,
    options: Options,
) -> dict[str, Any]:
    """The numbers that come from the frame log and the page's marks."""
    a, b, offset = times["a"], times["b"], times["offset"]
    fps = files[case.file].fps
    result: dict[str, Any] = {}
    window_frames = [f for f in log_data.frames if a <= f.at <= b]
    result["presented_per_s"] = round(len(window_frames) / (b - a), 1)
    counts = counters(log_data.media, a, b)
    span = counts["span"] or (b - a)
    result["counters_step_s"] = MEDIA_LINE_STEP_S  # the resolution of shown, dropped, wake-ups
    result["shown"] = counts["shown"]
    result["dropped"] = counts["dropped"]
    result["shown_per_s"] = round(counts["shown"] / span, 2)
    result["wakeups_per_s"] = (
        round(counts["wakeups"] / span, 1) if counts["wakeups"] is not None else None
    )
    if fps and case.state in (
        "playing",
        "sync",
        "moving",
        "still",
        "controls-moving",
        "controls-still",
    ):
        result["fps_ratio"] = round(result["shown_per_s"] / fps, 3)
    if page_state is not None:
        marks = page_state.get("m") or {}
        result.update(startup_metrics(marks, log_data.frames, offset))
        if case.base_state in ("seek", "seek-playing"):
            result.update(seek_metrics(marks, log_data.frames, offset, case.base_state == "seek"))
    if log_data.streaming:
        result["audio_streaming"] = log_data.streaming
    if log_data.sync:
        period = 1000 / fps if fps else 1000 / 30
        result["sync_period_ms"] = round(period, 2)
        t_click = times.get("click")
        if case.state == "sync" and t_click is not None:
            seeks_at = t_click + case.timing()[1] - 35 + 0.2
            result["sync_long"] = sync_stats(log_data.sync, t_click + 1, seeks_at - 1, period)
            result["sync_seeks"] = sync_stats(log_data.sync, seeks_at, b, period)
        else:
            result["sync_all"] = sync_stats(log_data.sync, a, b, period)
    return result


# ----- Budgets -----


@dataclass(frozen=True)
class Budget:
    """A limit on a metric of a case, in the worst of the runs. `kind` `max`:
    the worst is the highest; `min`: the lowest. With `against`, the limit is
    a ratio of the medians of the runs of the two cases."""

    case: str
    metric: str
    kind: str
    limit: float
    note: str
    against: str | None = None
    audio: str | None = None
    """`none` or `pw-private`: the budget applies to runs of that audio mode
    only (a run with sound has no page marks); None: to all."""


# The limits are what swb reaches (the worst of three runs, release build,
# idle machine; docs/performance.md "M14 exit: media budgets") plus a margin:
# 25 % rounded up to 5 (CPU %, ms) or 10 (MiB); a floor of 1 % of a core for
# the CPU of a file that plays; the start times (a few ms to 50 ms) twice the
# worst. An idle state has 0.2 % (two ticks of /proc in the 5 s window).
# They replace the ceilings of docs/media-diagnosis.md section 6 (150 % CPU,
# 600 MiB, 3 %, 300 ms, 500 ms, 250 ms), except `4k` memory, whose works limit
# (1.5 GiB) stays in the note.
BUDGETS: list[Budget] = [
    Budget("1080p:playing:full", "cpu_pct", "max", 55.0, "CPU % of one core"),
    Budget("1080p:playing:full", "rss_above_idle_mib", "max", 220.0, "RSS above the idle page"),
    Budget("1080p:playing:full", "dropped", "max", 0, "frames dropped"),
    Budget("1080p:playing:full", "fps_ratio", "min", 0.98, "frames shown / the file's rate"),
    Budget("1080p:playing:full", "loadedmetadata_ms", "max", 40, "to loadedmetadata", None, "none"),
    Budget("1080p:playing:full", "first_frame_ms", "max", 100, "to the first frame", None, "none"),
    Budget("wiki:playing:full", "cpu_pct", "max", 65.0, "CPU % of one core"),
    Budget("wiki:playing:full", "rss_above_idle_mib", "max", 310.0, "RSS above the idle page"),
    Budget("wiki:playing:full", "dropped", "max", 0, "frames dropped"),
    Budget("wiki:playing:full", "fps_ratio", "min", 0.98, "frames shown / the file's rate"),
    Budget("4k:playing:full", "rss_peak_mib", "max", 650.0, "peak RSS (the works limit: 1536)"),
    Budget("4k:playing:full", "alive", "min", 1, "no crash"),
    Budget("*:paused:full", "cpu_pct", "max", 0.2, "CPU % after pause"),
    Budget("*:ended:full", "cpu_pct", "max", 0.2, "CPU % after the end"),
    Budget("*:hidden:full", "cpu_pct", "max", 15.0, "CPU % of a playing hidden window"),
    Budget("*:hidden-paused:full", "cpu_pct", "max", 0.2, "CPU % of a paused hidden window"),
    Budget("*:display-none:full", "cpu_pct", "max", 0.2, "CPU % of a paused display: none"),
    Budget("m4a:playing:full", "cpu_pct", "max", 1.0, "CPU % of an audio file"),
    Budget(
        "1080p:seek:full", "seek_to_frame_ms", "max", 85, "from a seek to its frame", None, "none"
    ),
    Budget(
        "1080p:controls-moving:box",
        "cpu_pct",
        "max",
        1.10,
        "controls / no controls",
        "1080p:moving:box",
    ),
    Budget(
        "1080p:controls-still:box",
        "cpu_pct",
        "max",
        1.10,
        "controls / no controls",
        "1080p:still:box",
    ),
    Budget("1080p:controls-paused:box", "cpu_pct", "max", 0.2, "CPU % of paused controls"),
    Budget("1080p:sync:full", "sync_long.max_ms", "max", 33.3, "media - video, 180 s"),
    Budget("1080p:sync:full", "sync_seeks.max_ms", "max", 33.3, "media - video, after seeks"),
]


def dotted(result: dict[str, Any], metric: str) -> float | None:
    """A metric of a result; `a.b` reads the key `b` of the dictionary `a`."""
    value: Any = result
    for part in metric.split("."):
        if not isinstance(value, dict) or part not in value:
            return None
        value = value[part]
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def add_idle_baseline(results: list[dict[str, Any]]) -> None:
    """Adds `rss_above_idle_mib` to each result: its settled RSS minus the
    median settled RSS of the `idle` runs of the same invocation and audio
    mode (nothing is added without an idle run)."""
    for audio in {r["audio"] for r in results}:
        idle = [
            r["rss_settled_mib"]
            for r in results
            if r["audio"] == audio and r["case"].startswith("idle:")
        ]
        if not idle:
            continue
        base = statistics.median(idle)
        for r in results:
            if r["audio"] == audio and "rss_settled_mib" in r:
                r["rss_above_idle_mib"] = round(r["rss_settled_mib"] - base, 1)


def worst(results: list[dict[str, Any]], case: str, metric: str, kind: str) -> float | None:
    """The worst value of a metric over the runs of a case (None: no run
    has it)."""
    values = [v for r in results if r["case"] == case if (v := dotted(r, metric)) is not None]
    if not values:
        return None
    return max(values) if kind == "max" else min(values)


def median_of(results: list[dict[str, Any]], case: str, metric: str) -> float | None:
    """The median of a metric over the runs of a case (None: no run has it)."""
    values = [v for r in results if r["case"] == case if (v := dotted(r, metric)) is not None]
    return statistics.median(values) if values else None


@dataclass(frozen=True)
class Check:
    """The outcome of one budget."""

    budget: Budget
    case: str
    value: float | None
    limit: float
    ok: bool


def check_budgets(
    results: list[dict[str, Any]], budgets: list[Budget] | None = None
) -> list[Check]:
    """Compares the worst of the runs with each budget. A budget whose case
    was not run is skipped; a case that ran without the metric fails."""
    budgets = BUDGETS if budgets is None else budgets
    checks: list[Check] = []
    for budget in budgets:
        rows = [r for r in results if budget.audio in (None, r["audio"])]
        ran = {r["case"] for r in rows}
        for case in sorted(c for c in ran if fnmatch.fnmatchcase(c, budget.case)):
            value = worst(rows, case, budget.metric, budget.kind)
            limit = budget.limit
            if budget.against is not None:
                # A ratio of two noisy numbers: the medians of the runs.
                value = median_of(rows, case, budget.metric)
                base = median_of(rows, budget.against, budget.metric)
                if base is None or value is None:
                    if budget.against in ran:
                        checks.append(Check(budget, case, value, limit, False))
                    continue
                value, limit = value / base if base else float("inf"), budget.limit
            ok = value is not None and (value <= limit if budget.kind == "max" else value >= limit)
            checks.append(Check(budget, case, value, limit, ok))
    return checks


def skipped_budgets(checks: list[Check], budgets: list[Budget] | None = None) -> int:
    """The number of budgets without a check: their case did not run."""
    budgets = BUDGETS if budgets is None else budgets
    done = {id(c.budget) for c in checks}
    return sum(1 for b in budgets if id(b) not in done)


def results_failed(results: list[dict[str, Any]]) -> list[str]:
    """The runs whose page did not reach its state or whose swb died."""
    return [f"{r['case']}: {', '.join(r['problems'])}" for r in results if r.get("problems")]


# ----- Output -----

COLUMNS = [
    ("case", "case"),
    ("cpu_pct", "CPU %"),
    ("rss_peak_mib", "RSS peak"),
    ("rss_settled_mib", "RSS end"),
    ("rss_above_idle_mib", "above idle"),
    ("shown_per_s", "shown/s"),
    ("dropped", "dropped"),
    ("wakeups_per_s", "wake/s"),
    ("loadedmetadata_ms", "meta ms"),
    ("first_frame_ms", "frame ms"),
    ("seek_to_frame_ms", "seek ms"),
    ("sync_long.max_ms|sync_all.max_ms", "sync max"),
    ("sync_seeks.max_ms", "seek sync"),
]


def _shown(result: dict[str, Any], key: str) -> float | None:
    """A column value. The `sync` numbers show only for a run with sound
    (the audio clock for more than half of the lines); they stay in the JSON."""
    if key.startswith("sync_") and (dotted(result, key.split(".")[0] + ".audio_share") or 0) <= 0.5:
        return None
    return dotted(result, key)


def table(results: list[dict[str, Any]]) -> str:
    """A Markdown table with one row per run."""
    head = [title for _, title in COLUMNS]
    rows = ["| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    for r in results:
        cells = []
        for key, _ in COLUMNS:
            if key == "case":
                cells.append(r["case"] + (" (sound)" if r["audio"] != "none" else ""))
                continue
            value = next((v for k in key.split("|") if (v := _shown(r, k)) is not None), None)
            cells.append("" if value is None else f"{value:g}")
        rows.append("| " + " | ".join(cells) + " |")
    return "\n".join(rows)


def check_table(checks: list[Check]) -> str:
    """A Markdown table of the budgets and their outcome."""
    rows = ["| case | metric | worst | limit | |", "|---|---|---|---|---|"]
    for c in checks:
        kind = "<=" if c.budget.kind == "max" else ">="
        against = f" x {c.budget.against}" if c.budget.against else ""
        if c.value is not None:
            value = f"{c.value:g}"
        elif c.budget.metric == "rss_above_idle_mib":
            value = "missing (needs an idle run)"
        else:
            value = "missing"
        rows.append(
            f"| {c.case} | {c.budget.metric} | {value} | {kind} {c.limit:g}{against} |"
            f" {'ok' if c.ok else 'FAIL'} |"
        )
    return "\n".join(rows)


# ----- Entry point -----


def media_gui(
    case_names: list[str],
    options: Options,
    check: bool,
    json_out: Path | None,
) -> int:
    """Runs the cases `options.runs` times each (the runs of a case one after
    the other), prints the table, writes the JSON, and with `check`
    compares with the budgets. Returns the exit code: 1 for a failed budget
    or a run that did not reach its state, 2 for a bad argument or a run
    that could not start, 3 when the isolation proof of the private daemon
    failed."""
    files = media_files()
    try:
        cases = [parse_case(name, files) for name in case_names]
    except ValueError as error:
        log.error("%s", error)
        return 2
    used = {c.file for c in cases}
    missing = [str(files[u].path) for u in used if files[u].path and not files[u].path.is_file()]
    if missing:
        log.error("missing media files: %s", ", ".join(missing))
        return 2
    options.tmp.mkdir(exist_ok=True)
    # SIGTERM ends the tool like Ctrl+C: the `finally` blocks stop Xvfb, swb,
    # the link watcher and the private daemon by their PIDs.
    previous = signal.signal(signal.SIGTERM, exit_on_sigterm)
    try:
        if options.audio == "pw-private":
            options.daemon_dir = options.daemon_dir or pw_private.default_dir()
            with private_daemon(options.daemon_dir) as (report, watcher):
                log.info("private daemon: %s", json.dumps(report))
                options.watcher = watcher
                results = _run_all(cases, options, files)
        elif options.audio == "none":
            results = _run_all(cases, options, files)
        else:
            log.error("--audio takes none or pw-private")
            return 2
    except pw_private.IsolationError as error:
        log.error("isolation: %s; nothing was run", error)
        return 3
    except (XError, RuntimeError, OSError) as error:
        log.error("%s", error)
        return 2
    finally:
        signal.signal(signal.SIGTERM, previous)
    add_idle_baseline(results)
    print(table(results))
    failures = results_failed(results)
    for line in failures:
        print(f"state: {line}")
    checks = check_budgets(results) if check else []
    if check:
        print()
        print(check_table(checks))
        skipped = skipped_budgets(checks)
        print(f"\n{len(checks)} budgets checked, {skipped} skipped (their case did not run).")
    if json_out is not None:
        json_out.parent.mkdir(parents=True, exist_ok=True)
        payload = {"results": results, "checks": [_check_json(c) for c in checks]}
        json_out.write_text(json.dumps(payload, indent=1), encoding="utf-8")
    return 1 if failures or any(not c.ok for c in checks) else 0


def _check_json(check: Check) -> dict[str, Any]:
    return {
        "case": check.case,
        "metric": check.budget.metric,
        "value": check.value,
        "limit": check.limit,
        "kind": check.budget.kind,
        "against": check.budget.against,
        "ok": check.ok,
    }


def _run_all(
    cases: list[Case], options: Options, files: dict[str, MediaFile]
) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for case in cases:
        for run in range(options.runs):
            log.info("%s, run %d of %d", case.name, run + 1, options.runs)
            result = run_case(case, options, files)
            result["run"] = run + 1
            results.append(result)
            print(json.dumps({k: v for k, v in result.items() if k != "stderr_other"}), flush=True)
    return results


def find_binary(explicit: Path | None) -> Path | None:
    """The binary that `swb.find_swb` picks (release, else debug), or `explicit`."""
    if explicit is not None:
        return explicit if explicit.is_file() else None
    return swb.find_swb(None)
