"""`swbtools hostile`: runs swb on the hostile-page set and checks limits.

Each case (`hostile_cases/`) is a page that goes just past a documented
limit of swb. The runner loads it with the release binary, one case at a
time, and measures the wall time and the peak resident memory. A case
passes when swb finishes within the time and memory limits of the case
without a panic. A watchdog kills swb when it exceeds twice the time limit
or the memory cap, so that a bug cannot take the machine down.

Known failures (open bugs) must fail; a known failure that passes is an
error, so that the list stays current. See docs/testing.md.
"""

import contextlib
import logging
import math
import os
import shutil
import signal
import socketserver
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths, swb
from swbtools.hostile_cases import CASES, Case
from swbtools.procfs import status_kib

log = logging.getLogger(__name__)

VIEWPORT = (1280, 800)
"""The viewport of all cases, in CSS px."""

KILL_FACTOR = 2.0
"""The watchdog kills swb after this many times the time limit."""

MAX_RSS_CAP_MIB = 4096
"""The memory cap is twice the case's limit, but never above this."""

POLL_S = 0.05
"""Interval of the watchdog's memory sampling."""

MIN_SAMPLE_KIB = 16 << 10
"""Samples below this are from the moment of `exec` and are not used."""

LOG_READ_LIMIT = 8 << 20
"""At most this many bytes of swb's log are read back."""

OK, SLOW, MEMORY, PANIC, ERROR, INACTIVE = "ok", "slow", "memory", "panic", "error", "inactive"
KNOWN = "known failure"
UNEXPECTED_PASS = "unexpected pass"
SKIPPED = "skipped"


def has_media(binary: Path) -> bool:
    """Whether `binary` is a build with the Cargo feature `media` (its
    `--version` names FFmpeg)."""
    try:
        result = subprocess.run(
            [str(binary), "--version"], capture_output=True, text=True, timeout=30, check=False
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return "FFmpeg" in result.stdout


@dataclass(frozen=True)
class RunResult:
    """What one run of swb did."""

    returncode: int
    """Exit status; `-N` if signal N ended the process."""
    wall_s: float
    peak_mib: float
    killed: str | None
    """`time` or `memory` if the watchdog killed the process."""
    log: str
    """swb's stdout and stderr."""


def classify(case: Case, run: RunResult) -> str:
    """Returns the outcome of a run, without regard to known failures:
    `ok`, `slow`, `memory`, `panic`, `error` (also: swb logged the warning
    that `forbid_log` names) or `inactive` (swb did not log the warning that
    shows the limit acted)."""
    if "panicked" in run.log or (run.returncode == 101 and run.killed is None):
        return PANIC
    if run.killed == "memory" or run.peak_mib > case.rss_limit_mib:
        return MEMORY
    if run.killed == "time" or run.wall_s > case.time_limit_s:
        return SLOW
    if run.returncode != 0:
        return ERROR
    if case.forbid_log is not None and case.forbid_log in run.log:
        return ERROR
    expected = (case.expect_log,) if isinstance(case.expect_log, str) else case.expect_log
    if expected is not None and any(text not in run.log for text in expected):
        return INACTIVE
    return OK


def verdict(case: Case, outcome: str) -> tuple[str, bool]:
    """Returns the result to print and whether it counts as a pass. A known
    failure passes when it fails, and fails when it passes."""
    if case.known_failure is None:
        return outcome, outcome == OK
    if outcome == OK:
        return UNEXPECTED_PASS, False
    return KNOWN, True


def _kill(pid: int) -> None:
    """Kills the process group of `pid` (started with a new session)."""
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(pid, signal.SIGKILL)


def run_limited(
    command: list[str],
    log_file: Path,
    max_wall_s: float,
    max_rss_mib: float,
    poll_s: float = POLL_S,
) -> RunResult:
    """Runs `command` and returns its exit status, wall time and peak
    memory. Output goes to `log_file`. The watchdog kills the process when
    it runs longer than `max_wall_s` or its resident memory exceeds
    `max_rss_mib`. The peak is the high-water mark of the resident memory.
    `ru_maxrss` of the child includes the memory of this process at the
    time of the `fork` (and so is too high for small runs): it is used only
    when it is above that, and then it is exact. Otherwise the peak is the
    watchdog's last sample of the kernel's high-water mark (for runs of a
    few poll intervals, that can be low); with no useful sample it is
    `ru_maxrss`."""
    log_file.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")
    start = time.monotonic()
    killed = None
    sampled_kib = 0
    with log_file.open("wb") as output:
        process = subprocess.Popen(
            command,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
        pid = process.pid
        parent_kib = status_kib(os.getpid(), "VmHWM")
        try:
            while True:
                done, status, usage = os.wait4(pid, os.WNOHANG)
                if done:
                    break
                sampled_kib = max(sampled_kib, status_kib(pid, "VmHWM"))
                if killed is None:
                    if sampled_kib / 1024 > max_rss_mib:
                        killed = "memory"
                    elif time.monotonic() - start > max_wall_s:
                        killed = "time"
                    if killed is not None:
                        _kill(pid)
                time.sleep(poll_s)
        except BaseException:
            # Ctrl+C: swb is in its own session and does not get the signal.
            _kill(pid)
            with contextlib.suppress(ChildProcessError):
                os.waitpid(pid, 0)
            process.returncode = -signal.SIGKILL
            raise
        wall = time.monotonic() - start
        _kill(pid)  # children that outlive swb
        process.returncode = 0  # reaped above; keeps Popen from waiting again
    code = os.waitstatus_to_exitcode(status)
    if usage.ru_maxrss > parent_kib or sampled_kib < MIN_SAMPLE_KIB:
        peak_kib = usage.ru_maxrss
    else:
        peak_kib = sampled_kib
    text = _read_log(log_file)
    return RunResult(code, wall, peak_kib / 1024, killed, text)


def _read_log(log_file: Path) -> str:
    """The head of the log, at most `LOG_READ_LIMIT` bytes."""
    with log_file.open("rb") as handle:
        return handle.read(LOG_READ_LIMIT).decode("utf-8", errors="replace")


class StallServer:
    """A local HTTP server for the cases that need a peer that does not
    answer (feature 10a, `Case.server`): in mode `never` it reads the
    request and then holds the connection open; in mode `slow` it answers
    `ok` (with `Access-Control-Allow-Origin: *`) after `SLOW_S` seconds. It
    runs on daemon threads until `close`. In mode `big` it answers every
    request with a body of `BIG_MIB` MiB. The page finds its port in the
    text `__PORT__`.

    The media modes (M14 feature 10a) serve the file of the request's path
    from the case's directory: `endless` answers 200 without a length, sends
    the file and holds the connection open; `endless-skip` sends the file and
    then 64 KiB MP4 `free` boxes (which a demuxer skips) for `HOLD_S`;
    `lie-range` and `lie-total` answer `Range` requests with 206, and from the
    second answer on with a `Content-Range` that starts one byte late, or with
    a total that grows by one at each answer; each answer closes after
    `LIE_CHUNK` bytes of its body, so that a client asks again."""

    SLOW_S = 2.0
    BIG_MIB = 60
    HOLD_S = 120.0
    LIE_CHUNK = 64 << 10

    def __init__(self, mode: str, directory: Path | None = None) -> None:
        server = self
        self.answers = 0
        lock = threading.Lock()

        def serve_media(handler: socketserver.StreamRequestHandler, request: list[str]) -> None:
            path = request[0].split(" ")[1].split("?")[0].lstrip("/") if request else ""
            file = (directory or Path()) / path
            data = file.read_bytes() if path and file.is_file() else b""
            if mode in ("endless", "endless-skip"):
                head = "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n"
                handler.wfile.write(head.encode() + data)
                handler.wfile.flush()
                deadline = time.monotonic() + server.HOLD_S
                box = (1 << 16).to_bytes(4, "big") + b"free" + bytes((1 << 16) - 8)
                while time.monotonic() < deadline:
                    if mode == "endless":
                        time.sleep(1.0)
                    else:
                        handler.wfile.write(box)
                return
            with lock:
                earlier = server.answers
                server.answers += 1
            ranges = [line for line in request if line.lower().startswith("range:")]
            first, _, last = (ranges[0].split("=", 1)[1].strip() if ranges else "0-").partition("-")
            start = int(first or 0)
            end = min(int(last), len(data) - 1) if last else len(data) - 1
            shown_start = start + int(mode == "lie-range" and earlier > 0)
            shown_total = len(data) + (earlier if mode == "lie-total" else 0)
            body = data[start : end + 1]
            head = (
                "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\n"
                "Accept-Ranges: bytes\r\n"
                f"Content-Range: bytes {shown_start}-{end}/{shown_total}\r\n"
                f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
            )
            # Each answer closes after `LIE_CHUNK` bytes of its body, so that
            # the client asks again (and meets the lie) even for a small file.
            handler.wfile.write(head.encode() + body[: server.LIE_CHUNK])

        class Handler(socketserver.StreamRequestHandler):
            def handle(self) -> None:
                try:
                    request = []
                    while line := self.rfile.readline().strip():
                        request.append(line.decode("latin-1"))
                    if mode in ("endless", "endless-skip", "lie-range", "lie-total"):
                        serve_media(self, request)
                    elif mode == "big":
                        size = server.BIG_MIB << 20
                        head = (
                            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\n"
                            "Content-Type: application/octet-stream\r\n"
                            f"Content-Length: {size}\r\nConnection: close\r\n\r\n"
                        )
                        self.wfile.write(head.encode())
                        chunk = bytes(1 << 20)
                        for _ in range(server.BIG_MIB):
                            self.wfile.write(chunk)
                    elif mode == "slow":
                        time.sleep(server.SLOW_S)
                        body = b"ok"
                        head = (
                            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\n"
                            f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
                        )
                        self.wfile.write(head.encode() + body)
                    else:
                        time.sleep(3600)
                except OSError:
                    pass

        class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
            daemon_threads = True
            allow_reuse_address = True
            request_queue_size = 128

        self._tcp = Server(("127.0.0.1", 0), Handler)
        threading.Thread(target=self._tcp.serve_forever, daemon=True).start()

    @property
    def port(self) -> int:
        """The port that the server listens on."""
        return int(self._tcp.server_address[1])

    def close(self) -> None:
        self._tcp.shutdown()
        self._tcp.server_close()


def write_case(case: Case, directory: Path, port: int = 0) -> Path:
    """Writes the page of `case` and its extra files into `directory` and
    returns the path of the HTML file. `__PORT__` in the texts is replaced
    by `port` (the port of the case's `StallServer`)."""
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    page = case.build()
    for name, content in page.files.items():
        if isinstance(content, str):
            content = content.replace("__PORT__", str(port))
        data = content.encode() if isinstance(content, str) else content
        (directory / name).parent.mkdir(parents=True, exist_ok=True)
        (directory / name).write_bytes(data)
    index = directory / "index.html"
    index.write_text(page.html.replace("__PORT__", str(port)), encoding="utf-8")
    return index


def run_case(case: Case, binary: Path, root: Path) -> RunResult:
    """Writes and runs one case in `root/NAME/`."""
    stall = StallServer(case.server, root / case.name) if case.server else None
    try:
        return _run_case(case, binary, root, stall.port if stall else 0)
    finally:
        if stall is not None:
            stall.close()


def _run_case(case: Case, binary: Path, root: Path, port: int) -> RunResult:
    directory = root / case.name
    index = write_case(case, directory, port)
    # `{dir}` in an option is the directory of the case (a fixture to
    # replay); a case with a `url` opens that URL, not `index.html`.
    extra = [arg.replace("{dir}", str(directory.resolve())) for arg in case.swb_args]
    command = swb.hostile_command(
        binary,
        case.url or index.resolve().as_uri(),
        VIEWPORT,
        math.ceil(case.time_limit_s),
        directory / "boxes.json",
        directory / "screenshot.png",
        extra,
    )
    cap = min(2 * case.rss_limit_mib, MAX_RSS_CAP_MIB)
    log.info("running %s", " ".join(command))
    return run_limited(command, directory / "swb.log", KILL_FACTOR * case.time_limit_s, cap, POLL_S)


def format_line(case: Case, run: RunResult, result: str) -> str:
    """One output line: name, time, peak memory, result."""
    return f"{case.name:34} {run.wall_s:6.2f} s {run.peak_mib:7.0f} MiB  {result}"


def select(names: list[str], media: bool = False) -> list[Case] | None:
    """Returns the cases for `names` (all if empty; with `media`, all that
    need a media build), or None if a name is unknown."""
    if not names:
        return [case for case in CASES if case.media or not media]
    by_name = {case.name: case for case in CASES}
    unknown = [name for name in names if name not in by_name]
    if unknown:
        log.error("unknown case: %s (see --list)", ", ".join(unknown))
        return None
    return [by_name[name] for name in names]


def list_cases(media: bool = False) -> int:
    """Prints the cases (with `media`, those that need a media build) with
    their limits and descriptions."""
    for case in CASES:
        if media and not case.media:
            continue
        note = f"  [known failure: {case.known_failure}]" if case.known_failure else ""
        note += "  [media build]" if case.media else ""
        print(f"{case.name:34} {case.time_limit_s:4g} s {case.rss_limit_mib:5g} MiB  ", end="")
        print(f"{case.description}{note}")
    return 0


def hostile(
    names: list[str], swb_path: Path | None, list_only: bool, keep: bool, media: bool = False
) -> int:
    """Runs the cases (with `media` and no names: the cases that need a media
    build) and prints one line per case and a summary. Returns 1 if a case
    fails that is not a known failure, or a known failure passes."""
    if list_only:
        return list_cases(media)
    cases = select(names, media)
    if cases is None:
        return 2
    binary = swb.find_swb(swb_path)
    if binary is None:
        log.error("swb binary not found; build it (`just build`) or pass --swb PATH")
        return 1
    if "debug" in binary.parts:
        log.warning("%s is a debug build; times and memory are not representative", binary)
    root = paths.repo_root() / "out" / "hostile"
    failed: list[str] = []
    counts: dict[str, int] = {}
    total = 0.0
    with_media = has_media(binary)
    for case in cases:
        if case.media_only and not with_media:
            print(f"{case.name:34} skipped (needs a media build: just hostile-media)")
            counts[SKIPPED] = counts.get(SKIPPED, 0) + 1
            continue
        run = run_case(case, binary, root)
        result, passed = verdict(case, classify(case, run))
        print(format_line(case, run, result), flush=True)
        counts[result] = counts.get(result, 0) + 1
        total += run.wall_s
        if not passed:
            failed.append(case.name)
        if passed and not keep:
            shutil.rmtree(root / case.name, ignore_errors=True)
    summary = ", ".join(f"{count} {result}" for result, count in counts.items())
    print(f"{len(cases)} cases, {total:.1f} s: {summary}")
    if failed:
        print(f"FAILED: {', '.join(failed)} (logs in {root})")
    return 1 if failed else 0
