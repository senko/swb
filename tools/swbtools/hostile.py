"""`swbtools hostile`: runs swb on the hostile-page set and checks limits.

Each case (`hostile_cases.py`) is a page that goes just past a documented
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
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths, swb
from swbtools.hostile_cases import CASES, Case

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
    `ok`, `slow`, `memory`, `panic`, `error` or `inactive` (swb did not log
    the warning that shows the limit acted)."""
    if "panicked" in run.log or (run.returncode == 101 and run.killed is None):
        return PANIC
    if run.killed == "memory" or run.peak_mib > case.rss_limit_mib:
        return MEMORY
    if run.killed == "time" or run.wall_s > case.time_limit_s:
        return SLOW
    if run.returncode != 0:
        return ERROR
    if case.expect_log is not None and case.expect_log not in run.log:
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


def _status_kib(pid: int, key: str) -> int:
    """Reads a `kB` field of /proc/PID/status; 0 if it is not readable."""
    try:
        with Path(f"/proc/{pid}/status").open() as status:
            for line in status:
                if line.startswith(key + ":"):
                    return int(line.split()[1])
    except (OSError, ValueError, IndexError):
        pass
    return 0


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
        parent_kib = _status_kib(os.getpid(), "VmHWM")
        try:
            while True:
                done, status, usage = os.wait4(pid, os.WNOHANG)
                if done:
                    break
                sampled_kib = max(sampled_kib, _status_kib(pid, "VmHWM"))
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


def write_case(case: Case, directory: Path) -> Path:
    """Writes the page of `case` and its extra files into `directory` and
    returns the path of the HTML file."""
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    page = case.build()
    for name, content in page.files.items():
        data = content.encode() if isinstance(content, str) else content
        (directory / name).write_bytes(data)
    index = directory / "index.html"
    index.write_text(page.html, encoding="utf-8")
    return index


def run_case(case: Case, binary: Path, root: Path) -> RunResult:
    """Writes and runs one case in `root/NAME/`."""
    directory = root / case.name
    index = write_case(case, directory)
    command = swb.hostile_command(
        binary,
        index.resolve().as_uri(),
        VIEWPORT,
        math.ceil(case.time_limit_s),
        directory / "boxes.json",
        directory / "screenshot.png",
        list(case.swb_args),
    )
    cap = min(2 * case.rss_limit_mib, MAX_RSS_CAP_MIB)
    log.info("running %s", " ".join(command))
    return run_limited(command, directory / "swb.log", KILL_FACTOR * case.time_limit_s, cap, POLL_S)


def format_line(case: Case, run: RunResult, result: str) -> str:
    """One output line: name, time, peak memory, result."""
    return f"{case.name:34} {run.wall_s:6.2f} s {run.peak_mib:7.0f} MiB  {result}"


def select(names: list[str]) -> list[Case] | None:
    """Returns the cases for `names` (all if empty), or None if a name is
    unknown."""
    if not names:
        return list(CASES)
    by_name = {case.name: case for case in CASES}
    unknown = [name for name in names if name not in by_name]
    if unknown:
        log.error("unknown case: %s (see --list)", ", ".join(unknown))
        return None
    return [by_name[name] for name in names]


def list_cases() -> int:
    """Prints the cases with their limits and descriptions."""
    for case in CASES:
        note = f"  [known failure: {case.known_failure}]" if case.known_failure else ""
        print(f"{case.name:34} {case.time_limit_s:4g} s {case.rss_limit_mib:5g} MiB  ", end="")
        print(f"{case.description}{note}")
    return 0


def hostile(names: list[str], swb_path: Path | None, list_only: bool, keep: bool) -> int:
    """Runs the cases and prints one line per case and a summary. Returns 1
    if a case fails that is not a known failure, or a known failure passes."""
    if list_only:
        return list_cases()
    cases = select(names)
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
    for case in cases:
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
