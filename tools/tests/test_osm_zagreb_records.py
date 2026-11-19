"""`tools/osm_zagreb_records.py` (M12): the step list of the swb runner and the
comparison of swb's records with Chromium's. No Chromium and no swb binary:
the steps and the log are data."""

import importlib.util
import json
import sys
from pathlib import Path

from swbtools.records import format_json

SCRIPT = Path(__file__).resolve().parents[1] / "osm_zagreb_records.py"
spec = importlib.util.spec_from_file_location("osm_zagreb_records", SCRIPT)
assert spec is not None and spec.loader is not None
records = importlib.util.module_from_spec(spec)
sys.modules["osm_zagreb_records"] = records
spec.loader.exec_module(records)


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
        {"wait": records.DEFAULT_PAUSE_MS},
        {"wait": 10},
        {"wait": records.DEFAULT_PAUSE_MS},
        {"record": "window.__o()"},
        steps[8],
    ]


def test_a_skipped_group_runs_no_steps_but_is_recorded(tmp_path):
    groups = [{"name": "one", "steps": [{"click": ".a"}]}, {"name": "two", "steps": [{"key": "x"}]}]
    steps = records.swb_steps(groups, tmp_path, frozenset({1}))
    assert {"click": ".a"} not in steps and {"key": "x"} in steps
    assert [next(iter(s)) for s in steps].count("record") == 3


def test_the_steps_of_the_file_run_as_they_are(tmp_path):
    """swb's step runner takes Playwright's `:visible` and `>> nth=N`: the
    runner translates nothing."""
    groups = records.step_groups()
    steps = records.swb_steps(groups, tmp_path)
    assert any(":visible" in str(s) for s in steps) and any(">> nth=" in str(s) for s in steps)
    for number, one in enumerate(groups):
        assert all(step in steps for step in one["steps"]), number


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


def test_matomo_urls_are_cut_to_the_path_and_the_missing_urls_sorted():
    matomo = records.MATOMO
    rows = [
        {
            "label": "load",
            "missing": [f"{matomo}?r=1&s=2", "https://b/", f"{matomo}?r=3", "https://a/"],
        },
        {"label": "none", "missing": []},
    ]
    assert records.mask_missing(rows) == [
        {"label": "load", "missing": ["https://a/", "https://b/", matomo]},
        {"label": "none", "missing": []},
    ]


def test_the_console_keeps_errors_and_warnings_of_the_page():
    text = "\n".join(
        [
            "log  https://x/a.js:1  hello",
            "error  https://x/a.js:2  boom: a:b",
            f"warning  {records.MATOMO}?r=5:0  careful",
            "exception  :0  Failed to fetch",
            "error  https://x/b:0  Failed to load resource",
            "not a message",
        ]
    )
    assert records.page_console(text) == [
        {"type": "error", "url": "https://x/a.js", "text": "boom: a:b"},
        {"type": "warning", "url": records.MATOMO, "text": "careful"},
        {"type": "exception", "url": "", "text": "Failed to fetch"},
        {"type": "error", "url": "https://x/b", "text": "Failed to load resource"},
    ]


def test_the_console_counts_are_a_range_over_the_runs():
    a = {"type": "error", "url": "u", "text": "a"}
    b = {"type": "exception", "url": "", "text": "b"}
    runs = [[a, b, a], [b, a, a], [a, a, a, a]]
    assert records.console_counts(runs) == [
        {**a, "min": 2, "max": 4},
        {**b, "min": 0, "max": 1},
    ]
    assert records.console_counts([[a], [a]]) == [{**a, "min": 1, "max": 1}]


def test_the_sequences_differ_in_one_group():
    full = [g["name"] for g in records.step_groups("full")]
    without = [g["name"] for g in records.step_groups("no-transport")]
    assert len(full) == 60 and len(without) == 59
    assert [n for n in full if n not in without] == ["layer transport map"]
    assert records.data_file(Path("d"), "no-transport", "pixels") == Path(
        "d/osm-zagreb-nt-pixels.json"
    )


def test_json_puts_a_list_of_scalars_on_one_line():
    value = {"rects": [["a", 0, 1.5, 2, 3, 4], ["b", 1, 0, 0, 5, 6]], "color": [1, 2, 3], "e": []}
    text = format_json(value)
    assert json.loads(text) == value
    assert '["a", 0, 1.5, 2, 3, 4],' in text and '"color": [1, 2, 3],' in text


def _panel_record(thumbnail):
    return {"rects": {"panel": [[records.THUMBNAILS, 0, *thumbnail]], "map": []}}


def test_points_are_flat_in_all_runs_and_never_in_a_thumbnail():
    """The points come from Chromium's screenshots alone: a color that
    differs in one run, a pixel next to a color edge or a thumbnail rules a
    point out."""
    import numpy as np

    white = np.full((records.SIZE[1], records.SIZE[0], 3), 255, dtype=np.uint8)
    runs = [white.copy() for _ in range(3)]
    for shot in runs:
        shot[300:304, 700:760] = (255, 0, 0)
    runs[1][500, 600] = (0, 0, 0)
    record = _panel_record((1000, 200, 200, 100))
    chosen = records.choose_group("load", runs, record)
    assert chosen["header-1"] == records.FIXED_POINTS["header"]
    assert not any(abs(x - 600) <= 3 and abs(y - 500) <= 3 for x, y in chosen.values())
    assert not any(997 <= x <= 1203 and 197 <= y <= 303 for x, y in chosen.values())
    edge = [at for name, at in chosen.items() if name.startswith("edge-map")]
    assert edge and all(690 <= x <= 770 and 290 <= y <= 314 for x, y in edge)
    assert len([n for n in chosen if n.startswith("grid-")]) == records.GRID_COUNT


def test_rectangles_of_the_record_give_points_inside_them():
    import numpy as np

    white = np.full((records.SIZE[1], records.SIZE[0], 3), 255, dtype=np.uint8)
    record = {"rects": {"panel": [["x", 0, 900, 400, 60, 40]], "map": [["y", 0, 400, 500, 8, 8]]}}
    chosen = records.choose_group("load", [white, white, white], record)
    assert chosen["rect-panel-1"] == (930, 420)
    assert chosen["rect-map-1"] == (404, 504)


def test_thin_and_spread():
    assert records.thin(list(range(10)), 5) == [0, 2, 4, 6, 8]
    assert records.thin([1, 2], 5) == [1, 2]
    assert records.spread([(0, 0), (10, 10), (100, 0), (100, 95)], 90) == [
        (0, 0),
        (100, 0),
        (100, 95),
    ]


def _fake_replay(records_by_run):
    """A `replay_once` that returns `records_by_run[run]` and writes the files
    that `swbtools replay` writes for `--missing-out` and `--console-out`."""

    def replay_once(out, work, sequence, run, extra):
        missing = Path(extra[extra.index("--missing-out") + 1])
        missing.write_text(json.dumps([{"label": "load", "missing": [records.MATOMO + "?x"]}]))
        console = Path(extra[extra.index("--console-out") + 1])
        console.write_text("error  https://x/a.js:1  boom\n" * (1 + run))
        return records_by_run[run]

    return replay_once


def test_records_exit_with_status_1_when_the_runs_differ(tmp_path, monkeypatch):
    import pytest

    runs = [[{"hash": "#a"}], [{"hash": "#a"}], [{"hash": "#b"}]]
    monkeypatch.setattr(records, "replay_once", _fake_replay(runs))
    with pytest.raises(SystemExit) as stopped:
        records.write_records(tmp_path, tmp_path / "work", "full")
    assert stopped.value.code == 1
    # The data are written all the same, for a look at the difference.
    assert (tmp_path / "osm-zagreb-chromium.json").is_file()


def test_records_of_equal_runs_write_the_data_and_a_console_range(tmp_path, monkeypatch):
    runs = [[{"hash": "#a"}]] * 3
    monkeypatch.setattr(records, "replay_once", _fake_replay(runs))
    records.write_records(tmp_path, tmp_path / "work", "no-transport")
    console = json.loads((tmp_path / "osm-zagreb-nt-console.json").read_text())
    assert console == [
        {"type": "error", "url": "https://x/a.js", "text": "boom", "min": 1, "max": 3}
    ]
    missing = json.loads((tmp_path / "osm-zagreb-nt-missing.json").read_text())
    assert missing == [{"label": "load", "missing": [records.MATOMO]}]
    assert (tmp_path / "osm-zagreb-nt-steps.json").is_file()


def test_the_ink_point_is_the_core_of_a_stroke_and_follows_the_runs():
    import numpy as np

    shot = np.full((100, 100, 3), 235, dtype=np.uint8)
    shot[40:50, 60:62] = (20, 20, 20)  # a vertical stroke of 2 px
    shot[70, 70] = (0, 0, 0)  # a single dark pixel: not a stroke
    entry = ["x", 0, 50, 30, 30, 50]
    assert records.ink_point([shot], entry, "all") == (60, 40)
    other = shot.copy()
    other[40, 60] = (200, 200, 200)  # differs in a second run
    assert records.ink_point([shot, other], entry, "all") == (61, 40)
    assert records.ink_point([np.full((100, 100, 3), 9, dtype=np.uint8)], entry, "all") is None
    # `corner`: only the 14 x 14 px at the bottom right of the rectangle.
    assert records.ink_point([shot], entry, "corner") is None


def test_exact_points_need_the_same_color_in_all_runs_but_no_flat_square():
    """The resize handle of the share panel has no square of one color: its
    points are taken when all runs agree."""
    import numpy as np

    white = np.full((records.SIZE[1], records.SIZE[0], 3), 255, dtype=np.uint8)
    runs = [white.copy() for _ in range(3)]
    for shot in runs:
        for x, y in records.EXACT_POINTS["share html"].values():
            shot[y, x] = (102, 102, 102)
    record = {"rects": {"panel": [], "map": []}}
    chosen = records.choose_group("share html", runs, record)
    for name, at in records.EXACT_POINTS["share html"].items():
        assert chosen[f"{name}-1"] == at
    # A run that differs at one point drops that point only.
    first = next(iter(records.EXACT_POINTS["share marker"].values()))
    runs[2][first[1], first[0]] = (255, 255, 255)
    chosen = records.choose_group("share marker", runs, record)
    assert "resizer-top-1" not in chosen and "resizer-middle-1" in chosen
    load = records.choose_group("load", runs, record)
    assert not any(name.startswith("resizer") for name in load)
