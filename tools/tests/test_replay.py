"""Tests of `swbtools replay`: the steps file, the command line and a replay
of a small fixture in Chromium (skipped when Chromium is missing)."""

import asyncio
import io
import json
from pathlib import Path

import pytest
from PIL import Image

from swbtools.cli import build_parser
from swbtools.manifest import Fixture
from swbtools.pages import FixtureMeta, write_meta
from swbtools.probe_cases import ProbeError
from swbtools.replay import (
    DEFAULT_PAUSE_MS,
    read_group_names,
    read_groups,
    read_pixel_points,
    replay,
    replay_command,
    sample_pixels,
)


def write(tmp_path, data):
    path = tmp_path / "steps.json"
    path.write_text(json.dumps(data))
    return path


def test_groups_are_lists_or_named_objects(tmp_path):
    path = write(
        tmp_path,
        [[{"click": "#a"}, {"wait": 10}], {"name": "back", "steps": [{"back": True}]}],
    )
    assert read_groups(path) == [
        ({"click": "#a"}, {"wait": 10}),
        ({"back": True},),
    ]


@pytest.mark.parametrize(
    "data, message",
    [
        ({"steps": []}, "list of groups"),
        ([{"name": "x"}], "group 1: steps must be a list"),
        ([[{"click": "#a"}], [{"jump": 1}]], "group 2, step 1: unknown step"),
    ],
)
def test_bad_steps_files(tmp_path, data, message):
    with pytest.raises(ProbeError, match=message):
        read_groups(write(tmp_path, data))


def test_replay_command_line():
    args = build_parser().parse_args(
        ["--no-intl", "replay", "wikipedia-web-browser", "--steps", "s.json", "--record", "1"]
    )
    assert args.name == "wikipedia-web-browser"
    assert args.pause_ms == DEFAULT_PAUSE_MS
    assert args.setup is None
    assert args.out is None


PAGE = (
    b"<!DOCTYPE html><style>body { margin: 0 }</style>"
    b"<a id=l href='#t' style='position: fixed; left: 0; top: 0'>go</a>"
    b"<div style='height: 2000px'></div><div id=t>t</div>"
    b"<script>window.__s = () => JSON.stringify({y: Math.round(scrollY), hash: location.hash,"
    b" year: new Date().getFullYear(), setup: window.__setup})</script>"
)


@pytest.mark.usefixtures("chromium")
def test_replay_runs_the_groups_and_records_after_each(tmp_path, monkeypatch):
    """The page clock starts at `captured`, the setup runs after the load,
    and `back` and `forward` go through the history of the tab."""
    root = tmp_path / "root"
    fixture = root / "fixtures" / "js" / "page"
    fixture.mkdir(parents=True)
    recorded = Fixture(fixture)
    recorded.record("GET", "https://fixture.test/", 200, [("content-type", "text/html")], PAGE)
    recorded.save()
    write_meta(
        fixture,
        FixtureMeta("https://fixture.test/", "2031-05-06T07:08:09Z", (800, 600), "test", True),
    )
    monkeypatch.setenv("SWB_ROOT", str(root))
    groups = [
        ({"click": "#l"}, {"wait": 50}),
        ({"back": True}, {"wait": 50}),
        ({"forward": True}, {"wait": 50}),
    ]
    records, missing, errors = asyncio.run(
        replay("page", "window.__setup = 7", groups, "window.__s()", 50, system_fonts=True)
    )
    assert [(r["y"] > 1000, r["hash"]) for r in records] == [
        (False, ""),
        (True, "#t"),
        (False, ""),
        (True, "#t"),
    ]
    assert {r["year"] for r in records} == {2031}
    assert {r["setup"] for r in records} == {7}
    assert (missing, errors) == ([], [])


def test_group_names_and_pixel_points(tmp_path):
    steps = write(
        tmp_path, [[{"wait": 1}], {"name": "night", "steps": [{"wait": 1}]}, {"steps": []}]
    )
    assert read_group_names(steps) == ["group 1", "night", "group 3"]
    points = tmp_path / "points.json"
    points.write_text(json.dumps({"groups": ["load", "night"], "points": {"a": [1, 2]}}))
    assert read_pixel_points(points) == (["load", "night"], {"a": (1, 2)})
    for data, message in [
        ({"groups": "night", "points": {}}, "groups must be a list"),
        ({"groups": [], "points": {"a": [1]}}, "points must map names"),
        ({"groups": [], "points": {"a": [1.5, 2]}}, "points must map names"),
    ]:
        points.write_text(json.dumps(data))
        with pytest.raises(ProbeError, match=message):
            read_pixel_points(points)


def test_sample_pixels_of_a_screenshot():
    image = Image.new("RGB", (4, 3), (255, 255, 255))
    image.putpixel((2, 1), (1, 2, 3))
    buffer = io.BytesIO()
    image.save(buffer, "PNG")
    png = buffer.getvalue()
    assert sample_pixels(png, {"p": (2, 1), "q": (0, 0)}) == {"p": [1, 2, 3], "q": [255] * 3}
    with pytest.raises(ProbeError, match="outside"):
        sample_pixels(png, {"far": (4, 0)})


def test_replay_pixel_options_on_the_command_line(tmp_path):
    args = build_parser().parse_args(
        [
            "replay",
            "w",
            "--steps",
            "s.json",
            "--record",
            "1",
            "--screenshots",
            "shots",
            "--pixels",
            "p.json",
            "--pixels-out",
            "o.json",
        ]
    )
    assert (args.screenshots, args.pixels, args.pixels_out) == (
        Path("shots"),
        Path("p.json"),
        Path("o.json"),
    )
    # The options go together.
    steps = write(tmp_path, [[{"wait": 1}]])
    assert replay_command("w", None, steps, "1", None, 1, True, None, Path("p"), None) == 2


PIXEL_PAGE = (
    b"<!DOCTYPE html><style>body { margin: 0 } #a { width: 20px; height: 20px;"
    b" background: rgb(10, 20, 30) } body.on #a { background: rgb(200, 100, 50) }</style>"
    b"<div id=a></div><button id=b onclick=\"document.body.className = 'on'\">go</button>"
)


@pytest.mark.usefixtures("chromium")
def test_replay_saves_screenshots_and_reads_pixels(tmp_path, monkeypatch):
    """`--pixels` gives the colors after the load and after the groups that
    it names; `--screenshots` saves one image per record."""
    root = tmp_path / "root"
    fixture = root / "fixtures" / "js" / "page"
    fixture.mkdir(parents=True)
    recorded = Fixture(fixture)
    recorded.record(
        "GET", "https://fixture.test/", 200, [("content-type", "text/html")], PIXEL_PAGE
    )
    recorded.save()
    write_meta(
        fixture,
        FixtureMeta("https://fixture.test/", "2031-05-06T07:08:09Z", (200, 100), "test", True),
    )
    monkeypatch.setenv("SWB_ROOT", str(root))
    steps = write(tmp_path, [{"name": "go", "steps": [{"click": "#b"}, {"wait": 50}]}])
    points = tmp_path / "points.json"
    spec = {"groups": ["load", "go"], "points": {"a": [5, 5], "w": [150, 50]}}
    points.write_text(json.dumps(spec))
    out = tmp_path / "out" / "pixels.json"
    status = replay_command(
        "page",
        None,
        steps,
        "1",
        tmp_path / "records.json",
        50,
        True,
        tmp_path / "shots",
        points,
        out,
    )
    assert status == 0
    assert json.loads(out.read_text()) == {
        "load": {"a": [10, 20, 30], "w": [255, 255, 255]},
        "go": {"a": [200, 100, 50], "w": [255, 255, 255]},
    }
    assert sorted(p.name for p in (tmp_path / "shots").iterdir()) == ["000-load.png", "001-go.png"]
    unknown = tmp_path / "unknown.json"
    unknown.write_text(json.dumps({"groups": ["nope"], "points": {}}))
    assert replay_command("page", None, steps, "1", None, 50, True, None, unknown, out) == 2


@pytest.mark.parametrize("wanted", ["go", "load"])
def test_pixels_reject_a_name_used_twice(tmp_path, wanted):
    """The samples are keyed by the group name: a name that two groups
    have (or a group named `load`) is an error before the replay starts."""
    steps = write(
        tmp_path,
        [
            {"name": "go", "steps": [{"wait": 1}]},
            {"name": wanted, "steps": [{"wait": 1}]},
        ],
    )
    points = tmp_path / "points.json"
    points.write_text(json.dumps({"groups": [wanted], "points": {}}))
    out = tmp_path / "pixels.json"
    assert replay_command("page", None, steps, "1", None, 50, True, None, points, out) == 2
    assert not out.exists()


def test_steps_files_take_frame_steps(tmp_path):
    path = write(tmp_path, [[{"click": "#b", "frame": "#f >> #g"}, {"hover": [1, 2]}]])
    assert read_groups(path) == [({"click": "#b", "frame": "#f >> #g"}, {"hover": [1, 2]})]
    bad = write(tmp_path, [[{"type": "x", "frame": "#f"}]])
    with pytest.raises(ProbeError, match="group 1, step 1: frame"):
        read_groups(bad)
