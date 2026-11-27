"""Reads of `/proc` for the tools: the CPU time of a process and of its threads and the numbers
of `/proc/PID/status`."""

import os
import re
from pathlib import Path

CLOCK_TICKS = os.sysconf("SC_CLK_TCK")
"""The unit of the CPU times of `/proc/PID/stat` (ticks per second)."""


def cpu_seconds(stat_text: str) -> float:
    """The CPU time (user and system), in seconds, from the text of a `/proc/PID/stat` or
    `/proc/PID/task/TID/stat`."""
    fields = stat_text[stat_text.rindex(")") + 2 :].split()
    return (int(fields[11]) + int(fields[12])) / CLOCK_TICKS


def process_cpu_s(pid: int) -> float:
    """The CPU time (user and system, all threads) that a process used, in seconds."""
    return cpu_seconds(Path(f"/proc/{pid}/stat").read_text(encoding="utf-8"))


def thread_cpu(pid: int) -> dict[str, float]:
    """The CPU seconds of each thread of a process by thread name (the threads of one name are
    summed)."""
    totals: dict[str, float] = {}
    for stat in Path(f"/proc/{pid}/task").glob("*/stat"):
        try:
            text = stat.read_text(encoding="utf-8")
        except OSError:  # the thread ended
            continue
        name = text[text.index("(") + 1 : text.rindex(")")]
        totals[name] = totals.get(name, 0.0) + cpu_seconds(text)
    return totals


def status_value(text: str, key: str) -> int:
    """The number of the field `key` in the text of `/proc/PID/status` (0 when it has none)."""
    match = re.search(rf"^{re.escape(key)}:\s+(\d+)", text, re.MULTILINE)
    return int(match.group(1)) if match else 0


def status_kib(pid: int, key: str) -> int:
    """The `kB` field `key` of `/proc/PID/status`; 0 if the file is not readable."""
    try:
        return status_value(Path(f"/proc/{pid}/status").read_text(encoding="utf-8"), key)
    except OSError:
        return 0
