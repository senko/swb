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
        # 40 s: the budget of a document's long tasks is 30 s (ADR 0028 section 16).
        assert 0 < case.time_limit_s <= 40, case.name
        # A media build may hold up to the runner's cap (the frame budget of
        # the pipelines is 2 GiB, M14 feature 10a).
        most = hostile.MAX_RSS_CAP_MIB if case.media_only else 2048
        assert 0 < case.rss_limit_mib <= most, case.name
        assert case.known_failure is None or case.known_failure.strip(), case.name


def test_every_case_generates_a_bounded_page():
    for case in CASES:
        page = case.build()
        assert page.html.strip(), case.name
        assert page.size() < MAX_PAGE_BYTES, case.name
        for name in page.files:
            # A subdirectory is fine (the body files of a fixture); the
            # path stays inside the case directory.
            assert not name.startswith("/") and ".." not in name, case.name
            assert name != "index.html", case.name


def test_the_known_failures_of_the_backlog_are_in_the_set():
    known = {case.name for case in CASES if case.known_failure}
    assert "nest-div-100000" in known


def test_select_finds_cases_by_name():
    assert hostile.select([]) == CASES
    chosen = hostile.select([CASES[1].name, CASES[0].name]) or []
    assert [c.name for c in chosen] == [CASES[1].name, CASES[0].name]
    assert hostile.select(["no-such-case"]) is None


def test_select_finds_the_media_cases():
    media = hostile.select([], media=True) or []
    assert media and all(case.media for case in media)
    assert {case.name for case in media} == {case.name for case in CASES if case.media}
    assert "media-16-videos-at-the-frame-size-limit" in {case.name for case in media}
    assert all(case.media for case in CASES if case.media_only)


def test_a_binary_without_media_skips_the_media_only_cases(tmp_path, capsys):
    plain = tmp_path / "plain"
    plain.write_text("#!/bin/sh\necho 'swb 0.1.0'\n")
    plain.chmod(0o755)
    with_media = tmp_path / "media"
    with_media.write_text("#!/bin/sh\necho 'swb 0.1.0'\necho 'FFmpeg 9.0.2 (LGPL)'\n")
    with_media.chmod(0o755)
    assert not hostile.has_media(plain)
    assert hostile.has_media(with_media)
    assert not hostile.has_media(tmp_path / "missing")
    only = [case for case in CASES if case.media_only][:1]
    assert hostile.hostile([c.name for c in only], plain, False, False) == 0
    assert "skipped (needs a media build" in capsys.readouterr().out


def test_write_case_writes_the_page_and_its_files(tmp_path):
    case = fake_case(build=lambda: Page("<p>x", {"a.svg": "<svg/>", "b.bin": b"\x00\x01"}))
    index = hostile.write_case(case, tmp_path / "fake")
    assert index.read_text() == "<p>x"
    assert (tmp_path / "fake" / "a.svg").read_text() == "<svg/>"
    assert (tmp_path / "fake" / "b.bin").read_bytes() == b"\x00\x01"


def test_write_case_replaces_the_port_of_the_stall_server(tmp_path):
    files = {"a.js": "x(__PORT__)", "b.bin": b"__PORT__"}
    case = fake_case(build=lambda: Page("<p>__PORT__", files))
    index = hostile.write_case(case, tmp_path / "fake", 4321)
    assert index.read_text() == "<p>4321"
    assert (tmp_path / "fake" / "a.js").read_text() == "x(4321)"
    assert (tmp_path / "fake" / "b.bin").read_bytes() == b"__PORT__"


def test_stall_server_never_answers_or_answers_slowly(monkeypatch):
    import socket

    monkeypatch.setattr(hostile.StallServer, "SLOW_S", 0.2)
    request = b"GET /x HTTP/1.1\r\nHost: x\r\n\r\n"
    never = hostile.StallServer("never")
    slow = hostile.StallServer("slow")
    try:
        with socket.create_connection(("127.0.0.1", never.port)) as connection:
            connection.sendall(request)
            connection.settimeout(0.5)
            with pytest.raises(TimeoutError):
                connection.recv(100)
        with socket.create_connection(("127.0.0.1", slow.port)) as connection:
            connection.sendall(request)
            connection.settimeout(5)
            answer = b""
            while not answer.endswith(b"ok"):
                answer += connection.recv(1000)
            assert answer.startswith(b"HTTP/1.1 200 OK")
            assert b"Access-Control-Allow-Origin: *" in answer
    finally:
        never.close()
        slow.close()


def _media_answer(port: int, range_header: str | None = None, wait_s: float = 5.0) -> bytes:
    """The bytes that the stall server sends for `/a.bin` within `wait_s`."""
    import socket

    request = "GET /a.bin HTTP/1.1\r\nHost: x\r\n"
    if range_header:
        request += f"Range: {range_header}\r\n"
    data = b""
    with socket.create_connection(("127.0.0.1", port)) as connection:
        connection.sendall((request + "\r\n").encode())
        connection.settimeout(wait_s)
        try:
            while chunk := connection.recv(1 << 16):
                data += chunk
        except TimeoutError:
            pass
    return data


def test_stall_server_endless_mode_sends_the_file_and_holds(tmp_path, monkeypatch):
    monkeypatch.setattr(hostile.StallServer, "HOLD_S", 0.5)
    (tmp_path / "a.bin").write_bytes(b"x" * 1000)
    endless = hostile.StallServer("endless", tmp_path)
    skip = hostile.StallServer("endless-skip", tmp_path)
    try:
        head, _, body = _media_answer(endless.port, wait_s=0.3).partition(b"\r\n\r\n")
        assert head.startswith(b"HTTP/1.1 200 OK") and b"Content-Length" not in head
        assert body == b"x" * 1000, "the file, then nothing while the connection is open"
        _, _, body = _media_answer(skip.port, wait_s=2).partition(b"\r\n\r\n")
        assert body.startswith(b"x" * 1000)
        assert body[1000:1008] == (1 << 16).to_bytes(4, "big") + b"free"
        assert len(body) > 1000 + (1 << 16)
    finally:
        endless.close()
        skip.close()


def test_stall_server_lies_from_the_second_range_answer_on(tmp_path):
    (tmp_path / "a.bin").write_bytes(bytes(range(256)) * 512)
    shift = hostile.StallServer("lie-range", tmp_path)
    total = hostile.StallServer("lie-total", tmp_path)
    try:
        first = _media_answer(shift.port, "bytes=0-")
        assert b"Content-Range: bytes 0-131071/131072" in first
        # Each answer closes after 64 KiB of its body.
        assert len(first.partition(b"\r\n\r\n")[2]) == 64 << 10
        assert b"Content-Range: bytes 65537-131071/131072" in _media_answer(
            shift.port, "bytes=65536-"
        )
        assert b"/131072\r\n" in _media_answer(total.port, "bytes=0-")
        assert b"/131073\r\n" in _media_answer(total.port, "bytes=65536-")
        assert b"/131074\r\n" in _media_answer(total.port, "bytes=65536-")
    finally:
        shift.close()
        total.close()


def test_stall_server_big_mode_sends_the_whole_body(monkeypatch):
    import socket

    monkeypatch.setattr(hostile.StallServer, "BIG_MIB", 2)
    big = hostile.StallServer("big")
    try:
        with socket.create_connection(("127.0.0.1", big.port)) as connection:
            connection.sendall(b"GET /x HTTP/1.1\r\nHost: x\r\n\r\n")
            connection.settimeout(5)
            data = b""
            while chunk := connection.recv(1 << 20):
                data += chunk
        head, _, body = data.partition(b"\r\n\r\n")
        assert b"Content-Length: 2097152" in head
        assert len(body) == 2 << 20
    finally:
        big.close()


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


def test_classify_reports_a_forbidden_warning():
    case = fake_case(forbid_log="are watched")
    assert hostile.classify(case, fake_run(log="")) == "ok"
    assert hostile.classify(case, fake_run(log="images are watched")) == "error"


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
