"""Tests of `swbtools replay`: the steps file, the command line and a replay
of a small fixture in Chromium (skipped when Chromium is missing)."""

import asyncio
import json

import pytest

from swbtools.cli import build_parser
from swbtools.manifest import Fixture
from swbtools.pages import FixtureMeta, write_meta
from swbtools.probe import ProbeError
from swbtools.replay import DEFAULT_PAUSE_MS, read_groups, replay


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
