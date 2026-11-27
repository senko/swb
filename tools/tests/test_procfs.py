"""The reads of `/proc` in `swbtools.procfs`."""

import os

from swbtools import procfs


def test_cpu_seconds_adds_user_and_system_ticks_after_the_name() -> None:
    # Fields after the name (and a name with a space and a parenthesis): the 12th and 13th
    # are the user and system ticks.
    after = ["S"] + ["0"] * 10 + ["30", "20"] + ["0"] * 5
    text = "123 (a b) c) " + " ".join(after)
    assert procfs.cpu_seconds(text) == 50 / procfs.CLOCK_TICKS


def test_status_value_reads_a_field_or_gives_zero() -> None:
    text = "Name:\tswb\nVmRSS:\t   2048 kB\nThreads:\t7\n"
    assert procfs.status_value(text, "VmRSS") == 2048
    assert procfs.status_value(text, "Threads") == 7
    assert procfs.status_value(text, "VmHWM") == 0


def test_the_reads_of_this_process_work_and_a_missing_process_gives_zero() -> None:
    assert procfs.status_kib(os.getpid(), "VmRSS") > 0
    assert procfs.process_cpu_s(os.getpid()) >= 0.0
    assert sum(procfs.thread_cpu(os.getpid()).values()) >= 0.0
    assert procfs.status_kib(2**30, "VmRSS") == 0
