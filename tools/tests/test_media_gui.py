"""Tests of `swbtools media-gui`: the parsing of the frame log, the numbers
from it and from the page's marks, the command line and the budgets. No GUI
runs here."""
# ruff: noqa: E501  (the sample log lines)

from pathlib import Path

import pytest

from swbtools import media_gui as mg
from swbtools.media_gui import (
    Budget,
    Case,
    FrameLine,
    MediaFile,
    MediaLine,
    SyncLine,
)

FILES = {
    "idle": MediaFile(None, None, "none"),
    "1080p": MediaFile(Path("/x/a.mp4"), 30.0, "video"),
    "m4a": MediaFile(Path("/x/a.m4a"), None, "audio"),
}

LOG = """\
[2026-11-27T10:26:30Z DEBUG swb::gui] batch start=1.0 end=1.1 moves=2 wheels=0 presses=0 releases=0 keys=0 frame=1
[2026-11-27T10:26:30Z DEBUG swb::gui] frame at=100.250000 render=0.010000 compose=0.003000 present=0.001500 full=0 rects=2 pixels=31500
[2026-11-27T10:26:30Z DEBUG swb::gui] frame at=100.300000 render=0.000096 compose=0.000003 present=0.000001
[2026-11-27T10:26:30Z DEBUG swb::gui] media at=100.500000 wakeups=640 shown=300 dropped=0 decoded=306 converted=300
[2026-11-27T10:26:30Z DEBUG swb::gui] sync at=100.600000 video=1.040000 media=1.052345 audio=1
[2026-11-27T10:26:30Z WARN  console] page.html: Uncaught TypeError: x
[2026-11-27T10:26:30Z DEBUG swb_media::output] streaming 120 ms after the connect
"""


def test_the_log_is_parsed() -> None:
    log = mg.parse_log(LOG.splitlines(keepends=True))
    assert [f.at for f in log.frames] == [100.25, 100.3]
    assert [f.pixels for f in log.frames] == [31500, 0]
    assert log.media == [MediaLine(100.5, 640, 300, 0, 306, 300)]
    assert len(log.sync) == 1
    assert log.sync[0].audio
    assert log.sync[0].lag_ms == pytest.approx(12.345)
    assert log.streaming == 1
    assert len(log.other) == 1
    assert "Uncaught" in log.other[0]


def test_counters_take_the_last_line_up_to_each_end() -> None:
    media = [
        MediaLine(1.0, 10, 5, 0, 6, 5),
        MediaLine(2.0, 74, 35, 1, 37, 35),
        MediaLine(3.0, 138, 65, 1, 67, 65),
        MediaLine(4.0, 202, 95, 2, 97, 95),
    ]
    got = mg.counters(media, 2.5, 4.2)
    assert (got["shown"], got["dropped"], got["wakeups"]) == (60, 1, 128)
    assert got["span"] == pytest.approx(2.0)


def test_counters_do_not_use_the_flush_after_the_window() -> None:
    media = [
        MediaLine(1.0, 10, 5, 0, 6, 5),
        MediaLine(2.0, 74, 35, 0, 37, 35),
        MediaLine(4.4, 202, 95, 2, 97, 95),  # the flush at the close, 0.4 s after the window
    ]
    got = mg.counters(media, 2.5, 4.0)
    assert (got["shown"], got["dropped"], got["wakeups"], got["span"]) == (0, 0, 0, 1.5)
    assert mg.counters(media, 2.5, 4.5)["dropped"] == 2


def test_counters_without_a_line_in_the_window_show_no_change() -> None:
    media = [MediaLine(1.0, 10, 5, 0, 6, 5)]
    got = mg.counters(media, 2.0, 5.0)
    assert (got["shown"], got["wakeups"], got["span"]) == (0, 0, 3.0)


def test_counters_without_a_line_before_the_window_have_no_wake_ups() -> None:
    got = mg.counters([MediaLine(3.0, 10, 5, 0, 6, 5)], 2.0, 5.0)
    assert got["shown"] == 5
    assert got["wakeups"] is None
    assert mg.counters([], 2.0, 5.0)["shown"] == 0


def test_sync_stats_count_the_jumps_of_seeks() -> None:
    lines = [
        SyncLine(1.0, 1.0, 1.01, True),
        SyncLine(1.033, 1.033, 1.04, True),
        SyncLine(1.066, 12.0, 12.01, True),  # a seek
        SyncLine(1.1, 12.033, 12.04, True),
        SyncLine(1.133, 0.0, 0.01, True),  # a loop
    ]
    stats = mg.sync_stats(lines, 0.0, 2.0, 33.3)
    assert stats is not None
    assert stats["jumps"] == 2


def test_sync_stats_count_the_lines_over_the_period() -> None:
    lines = [
        SyncLine(1.0, 1.000, 1.010, True),
        SyncLine(2.0, 2.000, 2.040, True),
        SyncLine(3.0, 3.000, 3.020, False),
        SyncLine(9.0, 9.000, 9.500, True),
    ]
    stats = mg.sync_stats(lines, 0.0, 5.0, 33.3)
    # The frame times 1.0, 2.0, 3.0 follow the wall time: no jump.
    assert stats == {
        "jumps": 0,
        "n": 3,
        "min_ms": 10.0,
        "mean_ms": 23.33,
        "max_ms": 40.0,
        "max_at_s": 2.0,
        "over_period": 1,
        "audio_share": 0.667,
    }
    assert mg.sync_stats(lines, 20.0, 30.0, 33.3) is None


FRAMES = [
    FrameLine(10.0, 0, 0, 0, 5000),
    FrameLine(10.2, 0, 0, 0, 0),
    FrameLine(10.5, 0, 0, 0, 5000),
    FrameLine(11.0, 0, 0, 0, 5000),
]


def test_the_first_frame_needs_pixels() -> None:
    assert mg.first_frame_after(FRAMES, 10.1) == FRAMES[2]
    assert mg.first_frame_after(FRAMES, 12.0) is None


def test_startup_times_are_from_the_src_assignment() -> None:
    # offset 0: the page's clock is the log's clock.
    marks = {"t0": 10_000, "ev": {"loadedmetadata": [10_020], "loadeddata": [10_300]}}
    got = mg.startup_metrics(marks, FRAMES, 0.0)
    assert got == {"loadedmetadata_ms": 20, "loadeddata_ms": 300, "first_frame_ms": 500.0}
    assert mg.startup_metrics({}, FRAMES, 0.0) == {}


def test_a_paused_seek_ends_at_the_next_frame_after_the_assignment() -> None:
    marks = {
        "seeks": [
            {"start": 10_100, "seeked": 10_130, "target": 3},
            {"start": 10_900, "seeked": 10_980, "target": 5},
            {"start": 12_000, "target": 7},
        ]
    }
    got = mg.seek_metrics(marks, FRAMES, 0.0, paused=True)
    assert got["seeks"] == 3
    assert got["seeked"] == 2
    assert got["seek_to_seeked_ms"] == 80
    assert got["seek_to_frame_ms"] == 400.0
    playing = mg.seek_metrics(marks, FRAMES, 0.0, paused=False)
    assert playing["seek_to_frame_ms"] == 400.0  # 10.5 after seeked 10.13


def test_the_state_of_a_case_is_verified() -> None:
    paused = Case("1080p", "paused", "full")
    good = {"v": {"paused": True, "ended": False, "t": 0.6, "rs": 4, "err": None}}
    assert mg.verify_state(paused, good) == []
    bad = {"v": {"paused": False, "t": 2.0, "rs": 4, "err": None}}
    assert mg.verify_state(paused, bad) == ["not paused"]
    broken = {"v": {"paused": True, "t": 0, "rs": 0, "err": 4}}
    assert "media error 4" in mg.verify_state(paused, broken)
    assert mg.verify_state(Case("1080p", "ended", "full"), good) == ["not ended"]
    seek = Case("1080p", "seek", "full")
    assert mg.verify_state(seek, {"v": {"rs": 4}, "m": {"seekDone": True}}) == []
    assert mg.verify_state(seek, {"v": {"rs": 4}, "m": {}}) == ["the seek series did not finish"]
    assert mg.verify_state(paused, None) == []
    assert mg.verify_state(Case("idle", "idle", "full"), {}) == []


def test_thread_shares_keep_the_busy_names_largest_first() -> None:
    a = {"swb": 1.0, "video": 2.0, "idle": 0.5}
    b = {"swb": 1.5, "video": 3.0, "idle": 0.5, "new": 0.2}
    got = mg.thread_shares(a, b, 10.0)
    assert got == {"video": 10.0, "swb": 5.0, "new": 2.0}


def test_the_cpu_of_the_threads_of_this_process_is_read() -> None:
    import os

    assert sum(mg.thread_cpu(os.getpid()).values()) >= 0.0


def test_cases_are_parsed() -> None:
    case = mg.parse_case("1080p:controls-still:box:12", FILES)
    assert case == Case("1080p", "controls-still", "box", 12.0)
    assert case.controls
    assert case.base_state == "still"
    assert case.name == "1080p:controls-still:box"
    assert case.timing() == (6.0, 12.0)
    assert mg.parse_case("1080p:controls-paused:box", FILES).base_state == "paused"
    for bad in ("1080p:playing", "nope:playing:full", "1080p:nope:full", "1080p:playing:wide"):
        with pytest.raises(ValueError):
            mg.parse_case(bad, FILES)
    with pytest.raises(ValueError):
        mg.parse_case("1080p:idle:full", FILES)


def test_every_suite_case_parses_and_every_budget_names_a_state() -> None:
    files = mg.media_files()
    for cases in mg.SUITES.values():
        for text in cases:
            mg.parse_case(text, files)
    for budget in mg.BUDGETS:
        _file, state, _layout = budget.case.split(":")
        assert state == "*" or state in mg.STATES or state.startswith("*")


def test_the_page_of_a_silent_case_autoplays_muted() -> None:
    html = mg.page_html(Case("1080p", "playing", "full"), FILES, sound=False)
    assert "v.muted=true" in html
    assert "v.autoplay=true" in html
    assert "file:///x/a.mp4" in html
    assert "<body><script>" in html


def test_the_page_of_a_sound_case_starts_on_a_click() -> None:
    html = mg.page_html(Case("m4a", "playing", "full"), FILES, sound=True)
    assert "v.muted=false" in html
    assert "addEventListener('click'" in html
    assert "createElement('audio')" in html
    assert "v.autoplay" not in html


def test_the_page_of_controls_stays_muted_and_has_controls() -> None:
    html = mg.page_html(Case("1080p", "controls-still", "box"), FILES, sound=True)
    assert "v.controls=true" in html
    assert "v.muted=true" in html


def test_the_sync_page_seeks_after_the_long_run() -> None:
    html = mg.page_html(Case("1080p", "sync", "full"), FILES, sound=True)
    assert "180000" in html
    assert "setInterval" in html


def test_the_idle_page_has_no_media() -> None:
    assert "video" not in mg.page_html(Case("idle", "idle", "full"), FILES, sound=False)


def test_the_command_runs_swb_in_a_capped_scope_without_wayland() -> None:
    command = mg.build_command(
        Path("/x/swb"), ["--js", "about:blank"], ["WAYLAND_DISPLAY"], {"DISPLAY": ":5"}, True
    )
    assert command[:4] == ["systemd-run", "--user", "--scope", "--quiet"]
    assert "MemoryMax=4G" in command
    tail = command[command.index("--") + 1 :]
    assert tail == ["env", "-u", "WAYLAND_DISPLAY", "DISPLAY=:5", "/x/swb", "--js", "about:blank"]
    plain = mg.build_command(Path("/x/swb"), [], [], {}, False)
    assert plain == ["env", "/x/swb"]


def result(case: str, **values: float | dict) -> dict:
    return {"case": case, "audio": "none", **values}


def test_the_idle_baseline_is_subtracted() -> None:
    results = [
        result("idle:idle:full", rss_settled_mib=90.0),
        result("idle:idle:full", rss_settled_mib=100.0),
        result("1080p:playing:full", rss_settled_mib=300.0),
    ]
    mg.add_idle_baseline(results)
    assert results[2]["rss_above_idle_mib"] == 205.0


def test_the_worst_of_the_runs_is_checked_against_the_limit() -> None:
    budgets = [
        Budget("1080p:playing:full", "cpu_pct", "max", 50.0, "cpu"),
        Budget("1080p:playing:full", "fps_ratio", "min", 0.98, "frames"),
        Budget("*:paused:full", "cpu_pct", "max", 0.2, "idle"),
        Budget("m4a:playing:full", "cpu_pct", "max", 3.0, "not run"),
    ]
    results = [
        result("1080p:playing:full", cpu_pct=40.0, fps_ratio=1.0),
        result("1080p:playing:full", cpu_pct=51.0, fps_ratio=0.99),
        result("1080p:paused:full", cpu_pct=0.0),
        result("wiki:paused:full", cpu_pct=0.3),
    ]
    checks = mg.check_budgets(results, budgets)
    got = {(c.case, c.budget.metric): (c.value, c.ok) for c in checks}
    assert got == {
        ("1080p:playing:full", "cpu_pct"): (51.0, False),
        ("1080p:playing:full", "fps_ratio"): (0.99, True),
        ("1080p:paused:full", "cpu_pct"): (0.0, True),
        ("wiki:paused:full", "cpu_pct"): (0.3, False),
    }


def test_a_budget_of_one_audio_mode_skips_the_other_runs() -> None:
    budget = Budget("a:playing:full", "loadedmetadata_ms", "max", 40, "start", None, "none")
    sound = {"case": "a:playing:full", "audio": "pw-private", "cpu_pct": 1.0}
    assert mg.check_budgets([sound], [budget]) == []
    silent = result("a:playing:full", loadedmetadata_ms=60.0)
    assert [c.ok for c in mg.check_budgets([sound, silent], [budget])] == [False]


def test_a_case_without_the_metric_fails_its_budget() -> None:
    budgets = [Budget("4k:playing:full", "alive", "min", 1, "no crash")]
    checks = mg.check_budgets([result("4k:playing:full", cpu_pct=1.0)], budgets)
    assert [c.ok for c in checks] == [False]


def test_a_ratio_budget_divides_by_the_other_case() -> None:
    budget = Budget("a:controls-still:box", "cpu_pct", "max", 1.1, "ratio", "a:still:box")
    ok = [result("a:controls-still:box", cpu_pct=36.0), result("a:still:box", cpu_pct=34.0)]
    assert [c.ok for c in mg.check_budgets(ok, [budget])] == [True]
    bad = [result("a:controls-still:box", cpu_pct=40.0), result("a:still:box", cpu_pct=34.0)]
    assert [c.ok for c in mg.check_budgets(bad, [budget])] == [False]
    assert mg.check_budgets([result("a:controls-still:box", cpu_pct=1.0)], [budget]) == []


def test_a_ratio_budget_uses_the_median_of_the_runs() -> None:
    budget = Budget("a:controls-still:box", "cpu_pct", "max", 1.1, "ratio", "a:still:box")
    runs = [result("a:controls-still:box", cpu_pct=cpu) for cpu in (36.0, 36.5, 45.0)] + [
        result("a:still:box", cpu_pct=cpu) for cpu in (34.0, 35.0, 33.0)
    ]
    checks = mg.check_budgets(runs, [budget])
    assert [c.ok for c in checks] == [True]
    assert checks[0].value == pytest.approx(36.5 / 34.0)


def test_dotted_metrics_read_nested_numbers() -> None:
    assert mg.dotted({"a": {"b": 2}}, "a.b") == 2.0
    assert mg.dotted({"a": None}, "a.b") is None
    assert mg.dotted({"a": True}, "a") is None


def test_the_tables_have_a_row_per_run() -> None:
    runs = [result("1080p:playing:full", cpu_pct=40.0), result("1080p:playing:full", cpu_pct=41.0)]
    assert len(mg.table(runs).splitlines()) == 4
    checks = mg.check_budgets(runs, [Budget("1080p:playing:full", "cpu_pct", "max", 50.0, "x")])
    assert "ok" in mg.check_table(checks)


def test_failed_runs_are_listed() -> None:
    runs = [result("a:b:c", problems=[]), result("d:e:f", problems=["not paused"])]
    assert mg.results_failed(runs) == ["d:e:f: not paused"]


# A fake swb that prints its arguments and its environment: the tests run the
# real command of `_start_swb` (`env -u ... NAME=VALUE binary args`) on it, so
# they see what swb would get. No systemd scope, no GUI.
FAKE_SWB = '#!/bin/sh\nfor a in "$@"; do echo "ARG:$a"; done\nenv\n'

OWNER_ENV = {
    "PIPEWIRE_REMOTE": "/run/user/1000/pipewire-0",
    "PIPEWIRE_CORE": "owner-core",
    "PIPEWIRE_CONFIG_DIR": "/home/owner/pw",
    "PIPEWIRE_CONFIG_NAME": "owner.conf",
    "PULSE_SERVER": "unix:/run/user/1000/pulse/native",
    "WAYLAND_DISPLAY": "wayland-0",
    "WAYLAND_SOCKET": "3",
}


def run_fake_swb(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, audio: str
) -> tuple[list[str], dict[str, str]]:
    """Runs `_start_swb` on the fake swb. Returns its arguments and its
    environment."""
    binary = tmp_path / "swb"
    binary.write_text(FAKE_SWB, encoding="utf-8")
    binary.chmod(0o755)
    for name, value in OWNER_ENV.items():
        monkeypatch.setenv(name, value)
    monkeypatch.setattr(mg, "have_systemd_scope", lambda: False)
    work = tmp_path / "work"
    work.mkdir()
    options = mg.Options(binary=binary, audio=audio, daemon_dir=tmp_path / "pw", tmp=tmp_path)
    process = mg._start_swb(options, ":99", work, "file:///x/page.html")
    out, _ = process.communicate(timeout=30)
    lines = out.splitlines()
    args = [line[4:] for line in lines if line.startswith("ARG:")]
    env = dict(line.split("=", 1) for line in lines if "=" in line and not line.startswith("ARG:"))
    return args, env


def test_a_silent_run_has_no_sound_output_and_an_empty_runtime_dir(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    args, env = run_fake_swb(tmp_path, monkeypatch, "none")
    assert "--no-audio-output" in args
    assert args[-1] == "about:blank"
    empty = str(tmp_path / "work" / "empty-runtime")
    assert env["PIPEWIRE_RUNTIME_DIR"] == empty
    assert env["XDG_RUNTIME_DIR"] == empty
    assert (tmp_path / "work" / "empty-runtime").is_dir()
    assert env["DISPLAY"] == ":99"
    for name in OWNER_ENV:
        assert name not in env, name


def test_a_private_daemon_run_has_the_three_client_variables_only(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    args, env = run_fake_swb(tmp_path, monkeypatch, "pw-private")
    assert "--no-audio-output" not in args
    assert args == ["--js", "file:///x/page.html"]
    daemon = tmp_path / "pw"
    assert env["PIPEWIRE_RUNTIME_DIR"] == str(daemon)
    assert env["XDG_RUNTIME_DIR"] == str(daemon)
    assert env["PIPEWIRE_CONFIG_NAME"] == str(daemon / "swb-private-client.conf")
    for name in OWNER_ENV:
        if name != "PIPEWIRE_CONFIG_NAME":
            assert name not in env, name


def test_the_names_that_reach_another_server_are_one_list() -> None:
    from swbtools import pw_private

    assert set(OWNER_ENV) >= set(pw_private.OTHER_SERVER_NAMES)
    assert "PULSE_SERVER" in pw_private.OTHER_SERVER_NAMES


def test_the_hidden_paused_state_must_be_paused() -> None:
    case = Case("1080p", "hidden-paused", "full")
    playing = {"v": {"paused": False, "t": 2.0, "rs": 4, "err": None}}
    assert mg.verify_state(case, playing) == ["not paused"]
    at_zero = {"v": {"paused": True, "t": 0, "rs": 4, "err": None}}
    assert mg.verify_state(case, at_zero) == ["paused at 0"]
    assert mg.verify_state(case, {"v": {"paused": True, "t": 0.6, "rs": 4, "err": None}}) == []


def test_check_counts_the_budgets_checked_and_skipped() -> None:
    budgets = [
        Budget("a:playing:full", "cpu_pct", "max", 50.0, "cpu"),
        Budget("b:playing:full", "cpu_pct", "max", 50.0, "not run"),
        Budget("*:paused:full", "cpu_pct", "max", 0.2, "idle"),
    ]
    runs = [result("a:playing:full", cpu_pct=1.0), result("a:paused:full", cpu_pct=0.0)]
    checks = mg.check_budgets(runs, budgets)
    assert len(checks) == 2
    assert mg.skipped_budgets(checks, budgets) == 1


def test_a_missing_idle_baseline_says_so() -> None:
    budget = Budget("1080p:playing:full", "rss_above_idle_mib", "max", 220.0, "rss")
    checks = mg.check_budgets([result("1080p:playing:full", rss_settled_mib=300.0)], [budget])
    assert "needs an idle run" in mg.check_table(checks)
    other = Budget("1080p:playing:full", "fps_ratio", "min", 0.98, "frames")
    plain = mg.check_budgets([result("1080p:playing:full", cpu_pct=1.0)], [other])
    assert "needs an idle run" not in mg.check_table(plain)


def test_seconds_is_one_source_for_the_page_and_the_window() -> None:
    sync = Case("1080p", "sync", "full")
    assert mg.with_seconds(sync, None) is sync
    longer = mg.with_seconds(sync, 100.0)
    assert longer.timing()[1] == 100.0
    assert "65000" in mg.page_html(longer, FILES, sound=True)
    own = Case("1080p", "playing", "full", 7.0)
    assert mg.with_seconds(own, 100.0) is own


def test_the_sync_column_shows_only_runs_with_sound() -> None:
    silent = result("a:playing:full", sync_all={"max_ms": 12.0, "audio_share": 0.0})
    sound = result("a:playing:full", sync_all={"max_ms": 12.0, "audio_share": 0.99})
    assert mg.dotted(silent, "sync_all.max_ms") == 12.0  # kept for the JSON
    assert "12" not in mg.table([silent]).splitlines()[2]
    assert "12" in mg.table([sound]).splitlines()[2]
