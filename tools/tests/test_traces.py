"""The trace format, the files in tools/traces/ and the Wayland extraction."""

import json
from itertools import pairwise

import pytest

from swbtools import traces
from swbtools.traces import Event, TraceError, parse_trace


def valid() -> dict:
    return {
        "description": "x",
        "window": [1280, 900],
        "duration": 2.0,
        "events": [
            {"t": 0.1, "kind": "move", "x": 5, "y": 6},
            {"t": 0.2, "kind": "press", "button": 1},
            {"t": 0.3, "kind": "release"},
            {"t": 1.0, "kind": "wheel", "notches": -1},
        ],
    }


def test_a_valid_trace_parses():
    trace = parse_trace(valid(), "t")
    assert trace.window == (1280, 900)
    assert trace.duration == 2.0
    assert [e.kind for e in trace.events] == ["move", "press", "release", "wheel"]
    assert trace.events[0] == Event(0.1, "move", x=5, y=6)
    assert trace.events[2].button == 1
    assert trace.events[3].notches == -1
    assert trace.count("move") == 1


@pytest.mark.parametrize(
    "change",
    [
        lambda d: d.update(events="no"),
        lambda d: d.update(duration=0.5),
        lambda d: d.update(duration="long"),
        lambda d: d.update(window=[1280]),
        lambda d: d["events"].append({"t": 0.5, "kind": "move", "x": 1, "y": 1}),
        lambda d: d["events"].append({"t": 3.0, "kind": "scroll"}),
        lambda d: d["events"].__setitem__(0, {"t": 0.1, "kind": "move", "x": 1.5, "y": 2}),
        lambda d: d["events"].__setitem__(3, {"t": 1.0, "kind": "wheel", "notches": 2}),
        lambda d: d["events"].__setitem__(1, {"t": 0.2, "kind": "press", "button": 9}),
        lambda d: d["events"].__setitem__(0, {"t": -1, "kind": "move", "x": 1, "y": 1}),
    ],
)
def test_an_invalid_trace_is_rejected(change):
    data = valid()
    change(data)
    with pytest.raises(TraceError):
        parse_trace(data, "t")


def test_the_traces_of_the_repository_are_valid():
    names = traces.list_traces()
    assert {"github-owner", "ars-owner", "idle-10s", "pointer-sweep", "wheel-burst"} <= set(names)
    for name in names:
        trace = traces.load_trace(name)
        assert trace.description
        if name == "idle-10s":
            assert not trace.events
            assert trace.duration == 10.0
    ars = traces.load_trace("ars-owner")
    assert ars.duration == 7.7
    assert all(e.t <= 7.7 for e in ars.events)
    github = traces.load_trace("github-owner")
    assert github.count("move") > 1000
    assert github.count("wheel") == 89
    assert github.count("press") == github.count("release") == 6


def test_the_osm_map_trace_keeps_the_pointer_on_the_map():
    trace = traces.load_trace("osm-map")
    assert trace.window == traces.OSM_MAP_WINDOW
    assert trace.count("press") == trace.count("release") == 5
    assert trace.count("wheel") == 6 + 5
    # Five drags of 200 to 400 px, each with moves at 60 Hz before its release.
    for _, (dx, dy) in traces.OSM_DRAGS:
        assert 200 <= (dx * dx + dy * dy) ** 0.5 <= 400
    left, top, right, bottom = traces.OSM_MAP_AREA
    for event in trace.events:
        if event.kind == "move":
            assert left <= event.x <= right and top <= event.y <= bottom, event
    # The wheel notches: three in, three out (400 ms apart), then a burst of five.
    wheel = [e for e in trace.events if e.kind == "wheel"]
    assert [e.notches for e in wheel[:6]] == [-1, -1, -1, 1, 1, 1]
    assert all(abs((b.t - a.t) - 0.4) < 1e-6 for a, b in pairwise(wheel[:6]))
    assert all(abs((b.t - a.t) - 0.03) < 1e-6 for a, b in pairwise(wheel[6:]))
    first_press = next(e for e in trace.events if e.kind == "press")
    first_release = next(e for e in trace.events if e.kind == "release")
    moves = [e for e in trace.events if e.kind == "move" and first_press.t < e.t < first_release.t]
    assert len(moves) == 36 and moves[1].t - moves[0].t < 0.02
    assert trace.duration > trace.events[-1].t


def test_the_written_text_reads_back():
    original = parse_trace(valid(), "t")
    text = traces.trace_json("x", (1280, 900), 2.0, original.events)
    again = parse_trace(json.loads(text), "t")
    assert again.events == original.events


WAYLAND_LOG = """\
100.0000 [1.000]  -> xdg_wm_base#13.get_xdg_surface(new id xdg_surface#27, wl_surface#26)
100.1000 [1.000]  -> wl_surface#26.commit()
100.2000 [1.000]  -> wl_surface#26.commit()
100.5000 [1.000] wl_pointer#22.enter(1, wl_surface#36, 5.0, 6.0)
100.5100 [1.000] wl_pointer#22.motion(100510, 7.0, 8.0)
100.6000 [1.000] wl_pointer#22.leave(2, wl_surface#36)
100.6000 [1.000] wl_pointer#22.enter(3, wl_surface#26, 640.4, 300.6)
100.6200 [1.000] wl_pointer#22.motion(100620, 641.0, 301.0)
100.7000 [1.000] wl_pointer#22.axis(100700, 0, 10.00000000)
100.7100 [1.000] wl_pointer#22.axis(100710, 0, -20.00000000)
100.7200 [1.000] wl_pointer#22.axis(100720, 1, 10.00000000)
100.8000 [1.000] wl_pointer#22.button(4, 100800, 272, 1)
100.8500 [1.000] wl_pointer#22.button(5, 100850, 272, 0)
100.8600 [1.000] wl_pointer#22.button(6, 100860, 273, 1)
"""


def test_wayland_extraction():
    events, first = traces.wayland_events(WAYLAND_LOG.splitlines())
    assert first == 100.1
    summary = [(e.t, e.kind, e.x, e.y, e.notches) for e in events]
    assert summary == [
        # The decoration surface (36) gives nothing; the main surface's enter is a move.
        (0.5, "move", 640, 301, 0),
        (0.52, "move", 641, 301, 0),
        (0.6, "wheel", 0, 0, 1),
        # A value of -20 is two notches up; the horizontal axis is dropped.
        (0.61, "wheel", 0, 0, -1),
        (0.61, "wheel", 0, 0, -1),
        (0.7, "press", 0, 0, 0),
        (0.75, "release", 0, 0, 0),
    ]


def test_a_recording_without_the_main_surface_is_rejected():
    with pytest.raises(TraceError):
        traces.wayland_events(["1.0 [1.0] wl_pointer#22.motion(1000, 1.0, 2.0)"])
