"""`tools/videojs_records.py` (M13 feature 6): the step groups of the two
sequences, the comparison of the events of Chromium's runs and the choice of
pixel points. No Chromium and no swb binary: the groups and the screenshots are
data."""

import importlib.util
import sys
from pathlib import Path

import numpy as np

SCRIPT = Path(__file__).resolve().parents[1] / "videojs_records.py"
spec = importlib.util.spec_from_file_location("videojs_records", SCRIPT)
assert spec is not None and spec.loader is not None
records = importlib.util.module_from_spec(spec)
sys.modules["videojs_records"] = records
spec.loader.exec_module(records)


def test_every_sampled_action_has_three_groups_and_starts_by_finishing_the_last():
    groups = records.values_groups()
    names = [g["name"] for g in groups]
    assert len(names) == len(set(names)), "group names are the keys of the pixel data"
    start = next(g for g in groups if g["name"] == "hover search start")
    assert start["steps"][0] == {"eval": "__finish()"}
    assert start["steps"][1] == {"hover": ".DocSearch-Button"}
    assert start["steps"][-1] == {"eval": "__seekFraction('CSSTransition', '', 0)"}
    for kind in ("middle", "near end"):
        assert f"hover search {kind}" in names
    middle = next(g for g in groups if g["name"] == "hover search middle")
    assert middle["steps"] == [{"eval": "__seekFraction('CSSTransition', '', 0.5)"}]
    # No group pauses between its steps: the animations must not run between
    # the action and the seek (the `wait`s are explicit).
    assert records.PAUSE_MS["values"] == 0


def test_the_values_sequence_samples_the_focus_of_the_player_and_its_blur():
    groups = {g["name"]: g for g in records.values_groups()}
    start = groups["focus container start"]
    assert start["steps"][:2] == [{"eval": "__finish()"}, {"key": "Tab"}]
    assert start["steps"].count({"key": "Tab"}) == 5
    assert groups["focus away start"]["steps"][1] == {"eval": "document.activeElement.blur()"}


def test_the_bars_are_seeked_as_animations_and_the_marquee_by_time():
    groups = {g["name"]: g for g in records.values_groups()}
    assert groups["bars start"]["steps"][-1] == {
        "eval": "__seekFraction('CSSAnimation', 'grow-bar', 0)"
    }
    assert groups["marquee second iteration"]["steps"] == [{"eval": "__seek('marquee', 30100)"}]
    assert groups["reveal before delay"]["steps"] == [{"eval": "__seek('hero', -100)"}]
    assert groups["first view"]["steps"][0]["eval"].startswith("window.__rects = true")


def test_the_events_sequence_is_the_capture_with_a_first_view_and_a_search_hover():
    groups = records.events_groups()
    names = [g["name"] for g in groups]
    assert names[:3] == [
        "first view",
        "hover search button",
        "hover nav links without transitions",
    ]
    assert names[-1] == "overscroll spring"
    # The focus on the player follows the Tab presses at the links; the
    # presses are those of the values sequence's focus groups.
    at = names.index("tab to the search button")
    assert names[at + 1] == "tab to the player controls"
    focus = groups[at + 1]["steps"]
    assert focus[: len([s for _, a in records.FOCUS_STEPS for s in a])] == [
        step for _, action in records.FOCUS_STEPS for step in action
    ]
    assert focus[-2:] == [{"eval": "document.activeElement.blur()"}, {"wait": 400}]
    bars = next(g for g in groups if g["name"] == "scroll to the bars")
    assert bars["steps"][-1] == {"wait": 1500}, "the longer bar ends before the record"
    assert records.PAUSE_MS["events"] == records.EVENTS_PAUSE_MS


def test_events_of_runs_that_differ_in_order_or_in_number_are_a_set_or_a_range():
    def event(kind, target):
        return [kind, "color", target, "", 0]

    a, b, c = event("transitionrun", "a"), event("transitionrun", "b"), event("transitionend", "b")

    def run(*logs):
        return [{"events": log} for log in logs]

    same = [run([a], [a, b], [a, b, c]), run([a], [b, a], [a, b, c]), run([a], [a, b], [a, c, b])]
    sets, ranges = records.unify_event_orders(same)
    assert (sets, ranges) == ([1, 2], {})
    assert all(r[1]["events"] == same[0][1]["events"] for r in same)
    differ = [run([a, b]), run([a]), run([a, b, b])]
    sets, ranges = records.unify_event_orders(differ)
    assert sets == [] and list(ranges) == [0]
    assert ranges[0] == [[a, 1, 1], [b, 0, 2]]
    assert differ[1][0]["events"] == [a, b]


def test_points_are_flat_in_all_runs_and_inside_the_rectangles_of_the_targets():
    white = np.full((100, 100, 3), 255, dtype=np.uint8)
    other = white.copy()
    other[40:60, 40:60] = (200, 0, 0)  # a square that only the second run has
    record = {
        "targets": [["button[x]", 10, 10, 30, 20]],
        "rects": {"hero": [["h1", 0, 60, 60, 30, 30]]},
    }
    chosen = records.choose_group("tab 1 start", [white, other, white], record)
    assert chosen, "some points are chosen"
    for x, y in chosen.values():
        assert not (37 <= x < 63 and 37 <= y < 63), "no point in or next to the unstable square"
    assert "target-1" in chosen
    x, y = chosen["target-1"]
    assert 10 <= x < 40 and 10 <= y < 30
    assert any(name.startswith("rect-hero") for name in chosen)


def test_the_swb_steps_have_a_record_and_a_screenshot_after_every_group(tmp_path):
    groups = [
        {"name": "one", "steps": [{"hover": "#a"}]},
        {"name": "two words", "steps": [{"wait": 5}]},
    ]
    steps = records.swb_steps(groups, tmp_path, 300, "setup()")
    assert steps[0] == {"eval": "setup()"}
    kinds = [next(iter(s)) for s in steps]
    assert kinds == [
        "eval",
        "hover",
        "wait",
        "record",
        "screenshot",
        "wait",
        "wait",
        "record",
        "screenshot",
    ]
    assert steps[4]["screenshot"] == str(tmp_path / "001-one.png")
    assert steps[-1]["screenshot"] == str(tmp_path / "002-two-words.png")
    none = records.swb_steps(groups, tmp_path, 0, "setup()")
    assert [next(iter(s)) for s in none] == [
        "eval", "hover", "record", "screenshot", "wait", "record", "screenshot",
    ]  # fmt: skip
