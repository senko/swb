"""`tools/osm_zagreb_records.py` (M12): the step list of the swb runner and the
comparison of swb's records with Chromium's. No Chromium and no swb binary:
the steps and the log are data."""

import importlib.util
import json
import sys
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "osm_zagreb_records.py"
spec = importlib.util.spec_from_file_location("osm_zagreb_records", SCRIPT)
assert spec is not None and spec.loader is not None
records = importlib.util.module_from_spec(spec)
sys.modules["osm_zagreb_records"] = records
spec.loader.exec_module(records)


def test_a_playwright_selector_becomes_a_marked_click():
    steps = records.css_steps({"click": "input#query:visible"})
    assert [next(iter(s)) for s in steps] == ["eval", "click", "eval"]
    assert steps[1] == {"click": "[data-swb-pick]"}
    assert "getBoundingClientRect" in steps[0]["eval"]
    assert '"input#query"' in steps[0]["eval"]
    nth = records.css_steps({"click": ".control-button >> nth=3"})
    assert "found[3]" in nth[0]["eval"] and "getBoundingClientRect" not in nth[0]["eval"]
    both = records.css_steps({"hover": "li a >> nth=0"})
    assert both[1] == {"hover": "[data-swb-pick]"}


def test_a_plain_step_stays():
    for step in ({"click": ".btn-close"}, {"click": [1, 2]}, {"key": "Enter"}, {"wait": 5}):
        assert records.css_steps(step) == [step]


def test_the_step_list_has_a_record_and_a_screenshot_after_every_group(tmp_path):
    groups = [
        {"name": "one", "steps": [{"click": ".a"}, {"wait": 10}]},
        {"name": "two words", "steps": [{"key": "x"}]},
    ]
    steps = records.swb_steps(groups, tmp_path)
    kinds = [next(iter(s)) for s in steps]
    assert kinds[:3] == ["eval", "record", "screenshot"]
    assert kinds.count("record") == 3 and kinds.count("screenshot") == 3
    assert steps[2]["screenshot"] == str(tmp_path / "000-load.png")
    assert steps[-1]["screenshot"] == str(tmp_path / "002-two-words.png")
    # A pause follows each step of the groups.
    assert steps[3:9] == [
        {"click": ".a"},
        {"wait": records.PAUSE_MS},
        {"wait": 10},
        {"wait": records.PAUSE_MS},
        {"record": "window.__o()"},
        steps[8],
    ]


def test_a_skipped_group_runs_no_steps_but_is_recorded(tmp_path):
    groups = [{"name": "one", "steps": [{"click": ".a"}]}, {"name": "two", "steps": [{"key": "x"}]}]
    steps = records.swb_steps(groups, tmp_path, frozenset({1}))
    assert {"click": ".a"} not in steps and {"key": "x"} in steps
    assert [next(iter(s)) for s in steps].count("record") == 3


def test_the_real_groups_translate_without_a_playwright_selector(tmp_path):
    steps = records.swb_steps(records.step_groups(), tmp_path)
    for step in steps:
        for kind in records.POINTER_STEPS:
            selector = step.get(kind)
            if isinstance(selector, str):
                assert ":visible" not in selector and ">> nth" not in selector


def test_answers_become_records_with_their_times():
    lines = [
        (0.5, json.dumps({"value": json.dumps({"hash": "#a"})})),
        (0.7, "not json"),
        (0.9, json.dumps({"error": "TypeError: x"})),
        (1.0, json.dumps({"value": "{broken"})),
        (1.2, json.dumps({"value": 5})),
    ]
    found, times = records.parse_answers(lines)
    assert found[0] == {"hash": "#a"}
    assert found[1] == {"error": "TypeError: x"}
    assert found[2]["error"].startswith("not JSON")
    assert len(found) == 3 and times == [0.5, 0.9, 1.0]


def test_the_log_is_split_at_the_screenshot_lines():
    text = "\n".join(
        [
            "WARN console load error",
            "INFO swb_automation::steps] screenshot /x/swb-shots/000-load.png",
            "WARN console group one",
            "WARN console group one again",
            "INFO swb_automation::steps] screenshot /x/swb-shots/001-one.png",
            "WARN console group two",
        ]
    )
    parts = records.split_log(text)
    assert parts == [
        ["WARN console load error"],
        ["WARN console group one", "WARN console group one again"],
        ["WARN console group two"],
    ]


def test_the_differences_name_the_fields_to_two_levels():
    a = {"hash": "#1", "tiles": {"loaded": 24, "srcs": ["a"]}, "counts": {"li": 3}, "same": [1]}
    b = {"hash": "#2", "tiles": {"loaded": 24, "srcs": ["b"]}, "counts": {"li": 3}, "same": [1]}
    assert records.field_differences(a, a) == []
    assert records.field_differences(a, b) == ["hash", "tiles.srcs"]
    assert records.field_differences(a, {"hash": "#1"}) == [
        "counts.li",
        "same",
        "tiles.loaded",
        "tiles.srcs",
    ]
    assert records.field_differences({"x": 1}, {"error": "boom"}) == ["error", "x"]
