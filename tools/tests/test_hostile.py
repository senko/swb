"""The hostile-page set: registry, classification and the watchdog."""

import os
import re
import sys
import time

import pytest

from swbtools import hostile
from swbtools.hostile_cases import CASES, MAX_PAGE_BYTES, Case, Page


def fake_case(**changes: object) -> Case:
    fields = {
        "name": "fake",
        "description": "fake",
        "build": lambda: Page("<p>x"),
        "time_limit_s": 5.0,
        "rss_limit_mib": 1024,
    }
    return Case(**{**fields, **changes})


def fake_run(**changes: object) -> hostile.RunResult:
    fields = {"returncode": 0, "wall_s": 1.0, "peak_mib": 100.0, "killed": None, "log": ""}
    return hostile.RunResult(**{**fields, **changes})


# --- The registry ----------------------------------------------------------


def test_names_are_unique_and_safe_as_directory_names():
    names = [case.name for case in CASES]
    assert len(names) == len(set(names))
    assert all(re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*", name) for name in names)


def test_cases_have_descriptions_and_sane_limits():
    for case in CASES:
        assert case.description.strip(), case.name
        assert 0 < case.time_limit_s <= 30, case.name
        assert 0 < case.rss_limit_mib <= 2048, case.name
        assert case.known_failure is None or case.known_failure.strip(), case.name


def test_every_case_generates_a_bounded_page():
    for case in CASES:
        page = case.build()
        assert page.html.strip(), case.name
        assert page.size() < MAX_PAGE_BYTES, case.name
        for name in page.files:
            assert "/" not in name and name != "index.html", case.name


def test_the_known_failures_of_the_backlog_are_in_the_set():
    known = {case.name for case in CASES if case.known_failure}
    assert "nest-div-100000" in known


def test_select_finds_cases_by_name():
    assert hostile.select([]) == CASES
    chosen = hostile.select([CASES[1].name, CASES[0].name]) or []
    assert [c.name for c in chosen] == [CASES[1].name, CASES[0].name]
    assert hostile.select(["no-such-case"]) is None


def test_write_case_writes_the_page_and_its_files(tmp_path):
    case = fake_case(build=lambda: Page("<p>x", {"a.svg": "<svg/>", "b.bin": b"\x00\x01"}))
    index = hostile.write_case(case, tmp_path / "fake")
    assert index.read_text() == "<p>x"
    assert (tmp_path / "fake" / "a.svg").read_text() == "<svg/>"
    assert (tmp_path / "fake" / "b.bin").read_bytes() == b"\x00\x01"


# --- Classification --------------------------------------------------------


@pytest.mark.parametrize(
    ("run", "expected"),
    [
        (fake_run(), "ok"),
        (fake_run(wall_s=5.5), "slow"),
        (fake_run(killed="time", returncode=-9, wall_s=10.0), "slow"),
        (fake_run(peak_mib=1500), "memory"),
        (fake_run(killed="memory", returncode=-9, peak_mib=2100), "memory"),
        (fake_run(returncode=101), "panic"),
        (fake_run(log="thread 'main' panicked at src/x.rs:1"), "panic"),
        (fake_run(returncode=-11), "error"),
        (fake_run(returncode=1), "error"),
        (fake_run(returncode=-9, killed="memory", peak_mib=2100, log="panicked"), "panic"),
    ],
)
def test_classify(run, expected):
    assert hostile.classify(fake_case(), run) == expected


def test_classify_uses_the_limits_of_the_case():
    case = fake_case(time_limit_s=20, rss_limit_mib=2048)
    assert hostile.classify(case, fake_run(wall_s=15, peak_mib=1500)) == "ok"


def test_classify_reports_a_limit_that_did_not_act():
    case = fake_case(expect_log="too deep")
    assert hostile.classify(case, fake_run(log="boxes too deep")) == "ok"
    assert hostile.classify(case, fake_run(log="")) == "inactive"


def test_known_failures_must_fail():
    plain = fake_case()
    known = fake_case(known_failure="bug")
    assert hostile.verdict(plain, "ok") == ("ok", True)
    assert hostile.verdict(plain, "slow") == ("slow", False)
    assert hostile.verdict(known, "memory") == ("known failure", True)
    assert hostile.verdict(known, "ok") == ("unexpected pass", False)


# --- The watchdog ----------------------------------------------------------


def python(code: str) -> list[str]:
    return [sys.executable, "-c", code]


SLEEP_FOREVER = "import time; time.sleep(60)"


def test_run_limited_reports_exit_status_output_and_time(tmp_path):
    run = hostile.run_limited(
        python("import sys; print('hello'); sys.exit(3)"), tmp_path / "log", 10, 4096
    )
    assert run.returncode == 3
    assert run.killed is None
    assert "hello" in run.log
    assert 0 < run.wall_s < 5
    assert run.peak_mib > 0


def test_run_limited_kills_a_process_over_the_memory_cap(tmp_path):
    code = (
        "import time\n"
        "blocks = []\n"
        "while True:\n"
        "    blocks.append(bytearray(b'x' * (16 << 20)))\n"
        "    time.sleep(0.01)\n"
    )
    run = hostile.run_limited(python(code), tmp_path / "log", 30, 300, poll_s=0.02)
    assert run.killed == "memory"
    assert run.returncode == -9
    assert 300 < run.peak_mib < 1500
    assert run.wall_s < 20


def test_run_limited_kills_a_process_over_the_time_limit(tmp_path):
    run = hostile.run_limited(python(SLEEP_FOREVER), tmp_path / "log", 0.3, 4096)
    assert run.killed == "time"
    assert run.wall_s < 10
    assert hostile.classify(fake_case(), run) == "slow"


def test_run_limited_kills_the_children_of_the_process(tmp_path):
    code = f"import subprocess, sys; subprocess.run([sys.executable, '-c', {SLEEP_FOREVER!r}])"
    run = hostile.run_limited(python(code), tmp_path / "log", 0.3, 4096)
    assert run.killed == "time"
    assert run.wall_s < 10


def test_run_limited_kills_the_process_on_ctrl_c(tmp_path, monkeypatch):
    pid_file = tmp_path / "pid"
    code = f"import os, time; open({str(pid_file)!r}, 'w').write(str(os.getpid())); time.sleep(60)"
    sleep = time.sleep

    def interrupt(seconds: float) -> None:
        sleep(seconds)
        if pid_file.exists() and pid_file.read_text():
            raise KeyboardInterrupt

    monkeypatch.setattr(hostile.time, "sleep", interrupt)
    with pytest.raises(KeyboardInterrupt):
        hostile.run_limited(python(code), tmp_path / "log", 30, 4096, poll_s=0.02)
    with pytest.raises(ProcessLookupError):
        os.kill(int(pid_file.read_text()), 0)
