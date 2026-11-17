"""The frame log parser, the mapping of events to batches and the report."""

import pytest

from swbtools import frame_log, gui_latency
from swbtools.frame_log import Batch, Frame
from swbtools.traces import Trace

BATCH_LINE = (
    "[2026-11-15T10:00:00Z DEBUG swb::gui::framelog] batch start=12.500000 end=12.562500 "
    "moves=3 wheels=1 presses=0 releases=2 keys=4"
)
FRAME_LINE = (
    "[2026-11-15T10:00:00Z DEBUG swb::gui::framelog] frame at=100.250000 render=0.010000 "
    "compose=0.003000 present=0.001500"
)


def test_the_lines_of_the_gui_parse():
    assert frame_log.parse_line(BATCH_LINE) == Batch(12.5, 12.5625, 3, 1, 0, 2, 4)
    assert frame_log.parse_line(FRAME_LINE) == Frame(100.25, 0.01, 0.003, 0.0015)
    assert frame_log.parse_line("[x DEBUG swb_engine::document::pipeline] style: 5ms") is None
    assert frame_log.parse_line("batch start=1 end=2") is None
    assert frame_log.parse_line("[x DEBUG swb::gui] batch start=1 end=2 moves=x") is None


def test_the_pipeline_lines_parse():
    line = "[2026-11-15T10:00:00Z DEBUG swb_engine::document::pipeline] style: 22.5ms"
    assert gui_latency.parse_stage(line) == ("style", 22.5)
    assert gui_latency.parse_stage(line.replace("22.5ms", "1.5s")) == ("style", 1500.0)
    assert gui_latency.parse_stage(line.replace("style: 22.5ms", "layout: 500µs")) == (
        "layout",
        0.5,
    )
    assert gui_latency.parse_stage(line.replace("style", "display list")) is None


def test_the_restyle_lines_parse():
    prefix = "[2026-11-15T10:00:00Z DEBUG swb_engine::document::pipeline] "
    targeted = prefix + "targeted restyle after a state change: 12 elements restyled"
    full = prefix + "style: full restyle after a state change (Containers)"
    assert gui_latency.parse_restyle(targeted) == ("targeted", 12)
    assert gui_latency.parse_restyle(full) == ("full: Containers", 0)
    assert gui_latency.parse_restyle(prefix + "style: 22.5ms") is None
    assert gui_latency.parse_stage(full) is None
    dom = prefix + "targeted restyle after DOM changes: 40 elements restyled"
    assert gui_latency.parse_restyle(dom) == ("DOM targeted", 40)
    dom_full = prefix + "style: full restyle after DOM changes (TooLarge)"
    assert gui_latency.parse_restyle(dom_full) == ("DOM full: TooLarge", 0)
    paths = gui_latency.restyle_paths(
        [(1.0, "targeted", 12), (2.0, "full: Containers", 0), (9.0, "targeted", 5)], 0.0, 5.0
    )
    assert paths == {"paths": {"targeted": 1, "full: Containers": 1}, "restyled_elements": 12}


def test_the_log_is_split_into_batches_and_frames():
    batches, frames = frame_log.parse_log([BATCH_LINE, "noise", FRAME_LINE, BATCH_LINE])
    assert len(batches) == 2
    assert len(frames) == 1


def batches_of(*counts: int) -> list[Batch]:
    """Batches that each count some moves, one second apart."""
    return [Batch(float(i), i + 0.5, moves=n) for i, n in enumerate(counts)]


def test_events_map_to_batches_by_order_within_the_kind():
    batches = batches_of(2, 0, 3, 1)
    mapped, logged = frame_log.map_events(6, batches, "move")
    assert logged == 6
    # Events 1 and 2 are in batch 0, 3 to 5 in batch 2, 6 in batch 3.
    assert [m.start for m in mapped if m] == [0.0, 0.0, 2.0, 2.0, 2.0, 3.0]


def test_a_kind_is_counted_by_its_own_field():
    batches = [Batch(0.0, 1.0, wheels=2), Batch(2.0, 3.0, moves=1)]
    mapped, logged = frame_log.map_events(1, batches, "move")
    assert logged == 1
    assert mapped[0] is batches[1]


def test_missing_and_extra_events_are_reported():
    mapped, logged = frame_log.map_events(4, batches_of(2), "move")
    assert logged == 2
    assert mapped[2:] == [None, None]
    mapped, logged = frame_log.map_events(1, batches_of(2, 2), "move")
    assert logged == 4
    assert len(mapped) == 1


def test_an_event_with_several_window_events_is_handled_by_the_last():
    # Two injected wheel notches, each two window events: the first notch's
    # pair is split over two batches.
    batches = [Batch(0.0, 1.0, wheels=1), Batch(2.0, 3.0, wheels=3)]
    mapped, logged = frame_log.map_events(2, batches, "wheel", per_event=2)
    assert logged == 4
    assert [m.end for m in mapped if m] == [3.0, 3.0]
    mapped, _ = frame_log.map_events(3, batches, "wheel", per_event=2)
    assert mapped[2] is None


def test_summary_uses_the_nearest_rank():
    assert frame_log.summary([]) == {"n": 0, "median": None, "p90": None, "max": None}
    values = [float(v) for v in range(1, 11)]
    assert frame_log.summary(values) == {"n": 10, "median": 6.0, "p90": 9.0, "max": 10.0}


def test_latencies_to_the_batch_end_and_the_next_frame():
    batches = batches_of(1, 1, 1)  # batches at [0, 0.5], [1, 1.5], [2, 2.5]
    frames = [Frame(0.7, 0, 0, 0), Frame(1.6, 0, 0, 0)]  # nothing follows the third batch
    injected = [-0.1, 0.9, 1.9]
    result = frame_log.event_latencies(injected, batches, frames, "move")
    assert result["logged"] == 3
    assert result["unmapped"] == 0
    assert result["to_batch_end_ms"]["median"] == pytest.approx(600.0)
    # The first two have frames (0.8 s and 0.7 s after the input); the third has none.
    assert result["no_frame"] == 1
    to_frame = result["to_frame_ms"]
    assert to_frame["n"] == 2
    assert to_frame["max"] == pytest.approx(800.0)


def test_the_frame_inside_the_batch_shows_its_input():
    # winit draws a turn's frame after its input events, before
    # `about_to_wait` ends the batch: that frame shows the batch's input.
    batches = batches_of(1, 1)  # [0, 0.5], [1, 1.5]
    frames = [Frame(0.45, 0, 0, 0), Frame(1.2, 0, 0, 0)]
    result = frame_log.event_latencies([-0.05, 0.9], batches, frames, "move")
    assert result["no_frame"] == 0
    assert result["to_frame_ms"]["n"] == 2
    assert result["to_frame_ms"]["max"] == pytest.approx(500.0)
    # A frame of an earlier turn does not count for a later batch.
    result = frame_log.event_latencies([0.9], batches_of(0, 1), [Frame(0.45, 0, 0, 0)], "move")
    assert result["no_frame"] == 1


def test_a_frame_more_than_two_seconds_later_is_no_frame():
    frames = [Frame(0.5 + 2.5, 0, 0, 0)]
    result = frame_log.event_latencies([0.0], batches_of(1), frames, "move")
    assert result["no_frame"] == 1
    assert result["to_frame_ms"]["n"] == 0


def test_the_report_and_its_tables():
    options = gui_latency.Options(
        trace=Trace("t", "", None, 2.0, ()), javascript=True, label="js/x"
    )
    injected = gui_latency.Injected()
    injected.times["move"] = [-0.1, 0.9]
    injected.times["wheel"] = [1.0]
    injected.start, injected.end = -1.0, 3.0
    # One window event of the wheel notch is missing (two are expected).
    batches = [Batch(0.0, 0.5, moves=1), Batch(1.0, 1.5, moves=1, wheels=1)]
    frames = [Frame(0.7, 0.01, 0.002, 0.001), Frame(1.6, 0.02, 0.002, 0.001)]
    stages = [(0.2, "style", 10.0), (0.4, "layout", 2.0), (3.5, "style", 5.0)]
    cpu = {"trace_cpu_s": 2.0, "idle_cpu_s": 0.3, "idle_s": 3.0, "drain_s": 0.5}
    report = gui_latency.build_report(options, injected, batches, frames, stages, cpu)
    assert report["frames"]["per_second"] == 0.5
    assert report["cpu"] == {
        "trace_percent": 50.0,
        "idle_percent": 10.0,
        "idle_s": 3.0,
        "drain_s": 0.5,
    }
    assert report["pipeline"]["trace"]["style_runs"] == 1
    assert report["pipeline"]["trace"]["layout_ms"] == 2.0
    assert report["pipeline"]["after_trace"]["style_runs"] == 1
    assert report["count_mismatch"] == {"wheel": {"injected": 1, "logged": 1, "expected": 2}}
    text = gui_latency.markdown(report)
    assert text.startswith("js/x: trace `t`")
    assert "| move | 2 |" in text
    assert "Count mismatch (wheel)" in text
