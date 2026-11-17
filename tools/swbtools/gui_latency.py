"""`swbtools gui-latency`: the latency of input in swb's GUI, on a private X
server.

The tool starts its own `Xvfb` (a free display number; it never uses the
display of the desktop), runs the GUI binary there (the Wayland variables
removed) with a fixture or a live URL, sets the window size, waits for the
load and plays an input trace (`swbtools/traces.py`) with XTEST. The GUI
prints its frame log (`swbtools/frame_log.py`); the tool maps each injected
event to the batch of window events that handled it, and reports, per kind
of input, the time from the input to the end of that batch and to the
first frame after it, with the frames per second, the CPU of the process
during the trace and when idle, and the style and layout runs.

Every process that the tool starts is stopped by its PID, also after an
error.
"""

import contextlib
import json
import logging
import os
import re
import select
import signal
import subprocess
import threading
import time
from collections.abc import Iterator
from dataclasses import dataclass, field
from pathlib import Path

from swbtools import automation, frame_log, paths, swb
from swbtools.compare import parse_captured
from swbtools.pages import read_meta
from swbtools.traces import KINDS, Trace
from swbtools.xinput import XDisplay, XError

log = logging.getLogger(__name__)

SCREEN = "5120x1440x24"
"""The screen of the private X server: large enough for the biggest window
that the diagnosis measured (5120 x 1440)."""

RUST_LOG = "warn,swb::gui=debug,swb_engine::document::pipeline=debug"
"""The log filter of the GUI process: the frame log and the pipeline log."""

EVENTS_PER_INPUT = {"wheel": 2}
"""The window events that one injected event makes in the GUI: on X11 a
wheel notch is the press and the release of button 4 or 5, and the GUI
counts both as wheel events (so a notch scrolls twice on X11)."""

CLOCK_TICKS = os.sysconf("SC_CLK_TCK")


def monotonic() -> float:
    """The time of `CLOCK_MONOTONIC` in seconds: the clock of the frame log."""
    return time.clock_gettime(time.CLOCK_MONOTONIC)


@dataclass
class Options:
    """What to run."""

    trace: Trace
    fixture: Path | None = None
    url: str | None = None
    javascript: bool = False
    window: tuple[int, int] = (1280, 900)
    settle_s: float = 1.0
    idle_s: float = 3.0
    load_timeout_s: float = 60.0
    drain_timeout_s: float = 60.0
    swb_path: Path | None = None
    label: str = ""
    log_file: Path | None = None


# ----- Processes -----


def start_xvfb() -> tuple[subprocess.Popen[bytes], str]:
    """Starts an Xvfb on a free display number. Returns the process and the
    display name (`:N`). Raises XError."""
    read_end, write_end = os.pipe()
    try:
        process = subprocess.Popen(
            ["Xvfb", "-displayfd", str(write_end), "-screen", "0", SCREEN, "-nolisten", "tcp"],
            pass_fds=[write_end],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError as error:
        os.close(read_end)
        raise XError("Xvfb is not installed") from error
    finally:
        os.close(write_end)
    try:
        number = _read_line(read_end, 20.0)
    except BaseException:
        # Ctrl-C or SIGTERM while Xvfb starts: it must not stay behind.
        stop(process)
        raise
    finally:
        os.close(read_end)
    if not number.strip().isdigit():
        stop(process)
        raise XError("Xvfb did not report a display number")
    display = f":{number.strip()}"
    if display == os.environ.get("DISPLAY"):
        stop(process)
        raise XError(f"refusing to use the desktop display {display}")
    return process, display


def _read_line(descriptor: int, timeout_s: float) -> str:
    """Reads one line from a pipe, waiting at most `timeout_s`."""
    deadline = time.monotonic() + timeout_s
    data = b""
    while b"\n" not in data:
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([descriptor], [], [], left)[0]:
            break
        chunk = os.read(descriptor, 64)
        if not chunk:
            break
        data += chunk
    return data.decode(errors="replace")


def stop(process: subprocess.Popen) -> None:
    """Stops a process that this tool started (by its handle): terminate,
    then kill."""
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def gui_command(binary: Path, options: Options) -> list[str]:
    """The command line of the GUI process (automation server on a free port
    for the load check)."""
    command = [str(binary), "--remote-port", "0"]
    if options.fixture is not None:
        meta = read_meta(options.fixture)
        command += ["--replay", str(options.fixture), "--test-fonts"]
        url = meta.url
        if options.javascript:
            command += [
                "--js",
                "--now",
                str(round(parse_captured(meta.captured).timestamp() * 1000)),
            ]
    else:
        assert options.url is not None
        url = options.url
        if options.javascript:
            command.append("--js")
    return [*command, url]


def gui_environment(display: str) -> dict[str, str]:
    """The environment of the GUI process: the private X display, no Wayland
    (winit would prefer it), the log filter."""
    env = {k: v for k, v in os.environ.items() if not k.startswith("WAYLAND_")}
    env["DISPLAY"] = display
    env["RUST_LOG"] = RUST_LOG
    return env


class LogCollector(threading.Thread):
    """Reads the stderr of the GUI: the frame log and the style and layout
    runs of the pipeline log, each with the time of its arrival."""

    def __init__(self, stream: object, log_file: Path | None = None) -> None:
        super().__init__(daemon=True)
        self.stream = stream
        self.log_file = log_file
        self.batches: list[frame_log.Batch] = []
        self.frames: list[frame_log.Frame] = []
        self.stages: list[tuple[float, str, float]] = []
        self.restyles: list[tuple[float, str, int]] = []
        self.logged = dict.fromkeys(frame_log.KIND_FIELDS, 0)

    def run(self) -> None:
        copy = self.log_file.open("w", encoding="utf-8") if self.log_file else None
        try:
            self.read(copy)
        finally:
            if copy is not None:
                copy.close()

    def read(self, copy: object) -> None:
        for line in self.stream:  # type: ignore[attr-defined]
            arrived = monotonic()
            if copy is not None:
                copy.write(line)  # type: ignore[attr-defined]
            item = frame_log.parse_line(line)
            if isinstance(item, frame_log.Batch):
                self.batches.append(item)
                for kind, field_name in frame_log.KIND_FIELDS.items():
                    self.logged[kind] += getattr(item, field_name)
            elif isinstance(item, frame_log.Frame):
                self.frames.append(item)
            else:
                stage = parse_stage(line)
                if stage is not None:
                    self.stages.append((arrived, *stage))
                restyle = parse_restyle(line)
                if restyle is not None:
                    self.restyles.append((arrived, *restyle))


_STAGE = re.compile(r"pipeline\]\s+(style|layout): ([0-9.]+)(ns|µs|ms|s)\s*$")


def parse_stage(line: str) -> tuple[str, float] | None:
    """The stage name (`style`, `layout`) and time in ms of a pipeline log
    line (`style: 22.5ms`), or None. The time is a Rust `Duration` in debug
    form."""
    match = _STAGE.search(line)
    if match is None:
        return None
    scale = {"ns": 1e-6, "µs": 1e-3, "ms": 1.0, "s": 1000.0}[match.group(3)]
    return match.group(1), float(match.group(2)) * scale


_TARGETED = re.compile(
    r"pipeline\]\s+targeted restyle after (a state change|DOM changes): (\d+) elements"
)
_FULL = re.compile(r"pipeline\]\s+style: full restyle after (a state change|DOM changes) \((\w+)\)")


def parse_restyle(line: str) -> tuple[str, int] | None:
    """The path of a restyle after a state change (ADR 0041) or after DOM
    changes (ADR 0042) from a pipeline log line: (`targeted`, restyled
    elements) or (`full: REASON`, 0), with `DOM ` before the path of a
    restyle after DOM changes; None for other lines."""
    match = _TARGETED.search(line)
    if match is not None:
        prefix = "DOM " if match.group(1) == "DOM changes" else ""
        return f"{prefix}targeted", int(match.group(2))
    match = _FULL.search(line)
    if match is not None:
        prefix = "DOM " if match.group(1) == "DOM changes" else ""
        return f"{prefix}full: {match.group(2)}", 0
    return None


def restyle_paths(restyles: list[tuple[float, str, int]], start: float, end: float) -> dict:
    """The restyles after state changes logged from `start` up to `end`, by
    path, and the elements that the targeted ones restyled."""
    paths: dict[str, int] = {}
    elements = 0
    for arrived, path, count in restyles:
        if start <= arrived < end:
            paths[path] = paths.get(path, 0) + 1
            elements += count
    return {"paths": paths, "restyled_elements": elements}


def process_cpu_s(pid: int) -> float:
    """The CPU time (user and system, all threads) that a process used, in
    seconds, from `/proc/PID/stat`."""
    text = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    fields = text[text.rindex(")") + 2 :].split()
    return (int(fields[11]) + int(fields[12])) / CLOCK_TICKS


# ----- Playing a trace -----


@dataclass
class Injected:
    """What `play` sent."""

    times: dict[str, list[float]] = field(default_factory=lambda: {k: [] for k in KINDS})
    skipped_moves: int = 0
    clamped_moves: int = 0
    start: float = 0.0
    end: float = 0.0


def play(display: XDisplay, trace: Trace, window: tuple[int, int]) -> Injected:
    """Sends the events of a trace at their times. Moves are clamped into the
    window; a move to the place where the pointer is gives no X event and
    is skipped (counted). The time of each event is taken just before it
    is sent."""
    injected = Injected()
    width, height = window
    pointer = (0, 0)
    injected.start = monotonic()
    for event in trace.events:
        wait = injected.start + event.t - monotonic()
        if wait > 0:
            time.sleep(wait)
        if event.kind == "move":
            target = (min(max(event.x, 0), width - 1), min(max(event.y, 0), height - 1))
            if target != (event.x, event.y):
                injected.clamped_moves += 1
            if target == pointer:
                injected.skipped_moves += 1
                continue
            pointer = target
            injected.times["move"].append(monotonic())
            display.motion(*target)
        elif event.kind == "wheel":
            injected.times["wheel"].append(monotonic())
            button = 5 if event.notches > 0 else 4
            display.button(button, True)
            display.button(button, False)
        else:
            injected.times[event.kind].append(monotonic())
            display.button(event.button, event.kind == "press")
    wait = injected.start + trace.duration - monotonic()
    if wait > 0:
        time.sleep(wait)
    injected.end = monotonic()
    return injected


# ----- The run -----


@contextlib.contextmanager
def private_session(
    binary: Path, options: Options
) -> Iterator[tuple[subprocess.Popen[str], LogCollector, XDisplay, int, str]]:
    """Xvfb and the GUI, both stopped at the end. Yields the GUI process,
    its log collector, the display, the browser window and the URL of the
    automation server."""
    xvfb, name = start_xvfb()
    gui: subprocess.Popen[str] | None = None
    display: XDisplay | None = None
    try:
        display = XDisplay(name)
        command = gui_command(binary, options)
        log.info("running %s on %s", " ".join(command), name)
        gui = subprocess.Popen(
            command,
            env=gui_environment(name),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        collector = LogCollector(gui.stderr, options.log_file)
        collector.start()
        address = automation.read_address(gui)
        window = display.wait_for_window(30.0)
        yield gui, collector, display, window, address
    finally:
        if display is not None:
            display.close()
        if gui is not None:
            stop(gui)
        stop(xvfb)


STALL_S = 10.0
"""`drain` gives up when the GUI counted no new event for this long."""


def drain(
    collector: LogCollector, injected: Injected, baseline: dict[str, int], timeout_s: float
) -> float:
    """Waits until the GUI's batches have counted every injected event since
    `baseline` (the counts at the start), or the timeout, or no progress for
    `STALL_S`. Returns the seconds waited."""
    started = time.monotonic()
    wanted = {
        kind: len(times) * EVENTS_PER_INPUT.get(kind, 1) for kind, times in injected.times.items()
    }
    last_total, last_change = -1, started
    while time.monotonic() - started < timeout_s:
        counted = {kind: collector.logged[kind] - baseline[kind] for kind in wanted}
        if all(counted[kind] >= count for kind, count in wanted.items()):
            break
        total = sum(counted.values())
        if total != last_total:
            last_total, last_change = total, time.monotonic()
        elif time.monotonic() - last_change > STALL_S:
            log.warning("the GUI counted no input for %.0f s; giving up", STALL_S)
            break
        time.sleep(0.05)
    return time.monotonic() - started


def run(options: Options) -> dict:
    """Runs the measurement and returns the report (a JSON-ready dict).
    Raises XError, RuntimeError or FileNotFoundError."""
    binary = swb.find_swb(options.swb_path)
    if binary is None:
        raise FileNotFoundError("the swb binary does not exist; run `just build`")
    if "debug" in binary.parts:
        log.warning("%s is a debug build; times are not representative", binary)
    with private_session(binary, options) as (gui, collector, display, window, address):
        width, height = options.window
        display.move_resize(window, 0, 0, width, height)
        with automation.Browser(address) as browser:
            if not browser.wait_for_load(int(options.load_timeout_s * 1000)):
                raise RuntimeError("the page did not load in time")
        # The pointer goes to the corner of the window (its events are not
        # part of the trace), then the page settles.
        display.motion(0, 0)
        time.sleep(options.settle_s)
        if not collector.frames:
            log.warning("the GUI drew no frame before the trace starts")
        batches_before = len(collector.batches)
        baseline = dict(collector.logged)
        cpu_start = process_cpu_s(gui.pid)
        injected = play(display, options.trace, (width, height))
        cpu_end = process_cpu_s(gui.pid)
        waited = drain(collector, injected, baseline, options.drain_timeout_s)
        idle_start = monotonic()
        cpu_idle_start = process_cpu_s(gui.pid)
        time.sleep(options.idle_s)
        idle_end = monotonic()
        cpu_idle_end = process_cpu_s(gui.pid)
        # The batches of the trace: not those before it (the preamble).
        batches = [b for b in collector.batches[batches_before:] if b.start >= injected.start]
        frames = list(collector.frames)
        stages = list(collector.stages)
        restyles = list(collector.restyles)
    report = build_report(
        options,
        injected,
        batches,
        frames,
        stages,
        {
            "trace_cpu_s": cpu_end - cpu_start,
            "idle_cpu_s": cpu_idle_end - cpu_idle_start,
            "idle_s": idle_end - idle_start,
            "drain_s": waited,
        },
    )
    report["restyles"] = restyle_paths(restyles, injected.start, injected.end)
    return report


def stage_totals(
    stages: list[tuple[float, str, float]], start: float, end: float
) -> dict[str, float | int]:
    """The runs and the total time in ms of the style and layout stages that
    were logged from `start` up to `end`."""
    totals: dict[str, float | int] = {
        "style_runs": 0,
        "style_ms": 0.0,
        "layout_runs": 0,
        "layout_ms": 0.0,
    }
    for arrived, stage, ms in stages:
        if start <= arrived < end:
            totals[f"{stage}_runs"] += 1
            totals[f"{stage}_ms"] += ms
    return totals


def build_report(
    options: Options,
    injected: Injected,
    batches: list[frame_log.Batch],
    frames: list[frame_log.Frame],
    stages: list[tuple[float, str, float]],
    cpu: dict[str, float],
) -> dict:
    """The report of a run from its parts (see the module docstring)."""
    in_trace = [f for f in frames if injected.start <= f.at <= injected.end]
    seconds = max(injected.end - injected.start, 1e-9)
    events = {
        kind: frame_log.event_latencies(
            injected.times[kind], batches, frames, kind, EVENTS_PER_INPUT.get(kind, 1)
        )
        for kind in KINDS
        if injected.times[kind]
    }
    mismatched = {
        kind: (e["injected"], e["logged"])
        for kind, e in events.items()
        if e["injected"] * EVENTS_PER_INPUT.get(kind, 1) != e["logged"]
    }
    after_end = injected.end + cpu["drain_s"] + cpu["idle_s"]
    return {
        "label": options.label,
        "trace": options.trace.name,
        "javascript": options.javascript,
        "window": list(options.window),
        "duration_s": round(seconds, 3),
        "events": events,
        "count_mismatch": {
            k: {"injected": a, "logged": b, "expected": a * EVENTS_PER_INPUT.get(k, 1)}
            for k, (a, b) in mismatched.items()
        },
        "skipped_moves": injected.skipped_moves,
        "clamped_moves": injected.clamped_moves,
        "frames": {
            "in_trace": len(in_trace),
            "per_second": round(len(in_trace) / seconds, 2),
            "render_ms": _ms(frame_log.summary([f.render * 1000 for f in in_trace])),
            "compose_ms": _ms(frame_log.summary([f.compose * 1000 for f in in_trace])),
            "present_ms": _ms(frame_log.summary([f.present * 1000 for f in in_trace])),
        },
        "batches": {
            "n": len(batches),
            "duration_ms": _ms(frame_log.summary([(b.end - b.start) * 1000 for b in batches])),
        },
        "cpu": {
            "trace_percent": round(100 * cpu["trace_cpu_s"] / seconds, 1),
            "idle_percent": round(100 * cpu["idle_cpu_s"] / max(cpu["idle_s"], 1e-9), 1),
            "idle_s": round(cpu["idle_s"], 2),
            "drain_s": round(cpu["drain_s"], 2),
        },
        "pipeline": {
            "trace": _round(stage_totals(stages, injected.start, injected.end)),
            "after_trace": _round(stage_totals(stages, injected.end, after_end)),
        },
    }


def _ms(values: dict) -> dict:
    return {k: (round(v, 1) if isinstance(v, float) else v) for k, v in values.items()}


def _round(values: dict) -> dict:
    return {k: (round(v, 1) if isinstance(v, float) else v) for k, v in values.items()}


# ----- Output -----


def _cell(value: float | None) -> str:
    return "-" if value is None else f"{value:.0f}"


def markdown(report: dict) -> str:
    """The report as Markdown: a table of the input latencies, one of the
    other numbers, and a line for each count that does not match."""
    title = report["label"] or report["trace"]
    head = (
        f"{title}: trace `{report['trace']}`, {report['duration_s']} s, window "
        f"{report['window'][0]} x {report['window'][1]}, JavaScript "
        f"{'on' if report['javascript'] else 'off'}"
    )
    lines = [head, ""]
    if report["events"]:
        lines += [
            "| Input | Events | to batch end median / p90 / max (ms) | "
            "to frame median / p90 / max (ms) | no frame |",
            "|---|---|---|---|---|",
        ]
    for kind, e in report["events"].items():
        batch, frame = e["to_batch_end_ms"], e["to_frame_ms"]
        lines.append(
            f"| {kind} | {e['injected']} | "
            f"{_cell(batch['median'])} / {_cell(batch['p90'])} / {_cell(batch['max'])} | "
            f"{_cell(frame['median'])} / {_cell(frame['p90'])} / {_cell(frame['max'])} | "
            f"{e['no_frame']} |"
        )
    cpu, frames, pipeline = report["cpu"], report["frames"], report["pipeline"]
    during, after = pipeline["trace"], pipeline["after_trace"]
    lines += [
        *([""] if report["events"] else []),
        "| Frames/s | CPU trace | CPU idle | Style runs (ms), trace | Layout runs (ms), trace "
        "| Style runs (ms), after | Layout runs (ms), after | Drain (s) |",
        "|---|---|---|---|---|---|---|---|",
        f"| {frames['per_second']} | {cpu['trace_percent']} % | {cpu['idle_percent']} % | "
        f"{during['style_runs']} ({during['style_ms']:.0f}) | "
        f"{during['layout_runs']} ({during['layout_ms']:.0f}) | "
        f"{after['style_runs']} ({after['style_ms']:.0f}) | "
        f"{after['layout_runs']} ({after['layout_ms']:.0f}) | {cpu['drain_s']} |",
    ]
    restyles = report.get("restyles", {})
    if restyles.get("paths"):
        paths = ", ".join(f"{path} {n}" for path, n in sorted(restyles["paths"].items()))
        lines.append(
            f"\nTargeted and full restyles in the trace (DOM: after DOM changes): {paths} "
            f"({restyles['restyled_elements']} elements restyled)."
        )
    for kind, counts in report["count_mismatch"].items():
        lines.append(
            f"\nCount mismatch ({kind}): the tool sent {counts['injected']} events, the frame "
            f"log has {counts['logged']} (expected {counts['expected']})."
        )
    return "\n".join(lines)


def _exit_on_sigterm(signum: int, _frame: object) -> None:
    """SIGTERM ends the tool like Ctrl-C: the `finally` blocks stop Xvfb
    and the GUI (the default action would leave them running)."""
    raise SystemExit(128 + signum)


def gui_latency(options: Options, as_json: bool) -> int:
    """Runs a measurement and prints the report."""
    previous = signal.signal(signal.SIGTERM, _exit_on_sigterm)
    try:
        report = run(options)
    except (XError, RuntimeError, FileNotFoundError) as error:
        log.error("%s", error)
        return 1
    finally:
        signal.signal(signal.SIGTERM, previous)
    print(json.dumps(report, indent=2) if as_json else markdown(report))
    return 0


def fixture_directory(name: str, javascript: bool, fixtures: str | None) -> Path:
    """The directory of a fixture: `fixtures/pages/NAME`, or `fixtures/js/NAME`
    when `fixtures` is `js` (the default with JavaScript on). Raises
    ValueError if there is none."""
    which = fixtures or ("js" if javascript else "pages")
    directory = paths.js_fixture_dir(name) if which == "js" else paths.fixture_dir(name)
    if not directory.is_dir():
        raise ValueError(f"there is no fixture {directory}")
    return directory
