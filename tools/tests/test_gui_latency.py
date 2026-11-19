"""`swbtools gui-latency` on senko-net: a real GUI on a private Xvfb."""

import shutil

import pytest

from swbtools import gui_latency, paths
from swbtools.traces import Event, Trace

RELEASE = paths.repo_root() / "target" / "release" / "swb"

pytestmark = [
    pytest.mark.skipif(shutil.which("Xvfb") is None, reason="Xvfb is not installed"),
    pytest.mark.skipif(not RELEASE.is_file(), reason="the release binary is not built"),
]


def short_trace() -> Trace:
    events = [Event(0.0, "move", x=300, y=200)]
    events += [Event(0.05 * i, "move", x=300 + 10 * i, y=200 + 5 * i) for i in range(1, 11)]
    events += [
        Event(0.6, "press"),
        Event(0.65, "release"),
        Event(0.8, "wheel", notches=1),
        Event(0.9, "wheel", notches=1),
        Event(1.0, "wheel", notches=-1),
    ]
    return Trace("short", "a short trace for the test", None, 1.2, tuple(events))


def test_a_short_trace_on_senko_net():
    options = gui_latency.Options(
        trace=short_trace(),
        fixture=paths.fixture_dir("senko-net"),
        swb_path=RELEASE,
        settle_s=0.5,
        idle_s=0.5,
        load_timeout_s=30.0,
        drain_timeout_s=15.0,
        label="pages/senko-net",
    )
    report = gui_latency.run(options)
    events = report["events"]
    assert events["move"]["injected"] == 11
    assert events["press"]["injected"] == 1
    assert events["release"]["injected"] == 1
    assert events["wheel"]["injected"] == 3
    # The GUI counted every event that the tool sent: the mapping by order
    # within the kind is valid.
    assert report["count_mismatch"] == {}
    for kind in ("move", "press", "release", "wheel"):
        assert events[kind]["unmapped"] == 0
        assert events[kind]["to_batch_end_ms"]["n"] == events[kind]["injected"]
    # The press changes the page (focus, selection) and a frame follows.
    assert events["press"]["no_frame"] == 0
    assert report["frames"]["in_trace"] >= 1
    assert "| move | 11 |" in gui_latency.markdown(report)


def test_the_urls_are_read_after_each_release():
    options = gui_latency.Options(
        trace=short_trace(),
        fixture=paths.fixture_dir("senko-net"),
        swb_path=RELEASE,
        settle_s=0.5,
        idle_s=0.5,
        load_timeout_s=30.0,
        drain_timeout_s=15.0,
        label="pages/senko-net",
        urls=True,
    )
    report = gui_latency.run(options)
    assert [u["after"] for u in report["urls"]] == ["start", "release 1", "end"]
    assert all(u["url"] == "https://senko.net/" for u in report["urls"]), report["urls"]
    # The read waits URL_DELAY_S after the release at 0.65 s.
    assert report["urls"][1]["at_s"] >= 0.65 + gui_latency.URL_DELAY_S
    assert "| release 1 |" in gui_latency.markdown(report)
