"""The step groups and the Chromium data for the exit test of videojs.org
(M13 feature 6; `fixtures/js/videojs`; the test is
`crates/automation/tests/videojs.rs`). Run from the repository root:

    uv run --project tools python tools/videojs_records.py steps [--out DIR]
    uv run --project tools python tools/videojs_records.py records [--sequence S] \\
        [--out DIR] [--work DIR]
    uv run --project tools python tools/videojs_records.py points [--out DIR] [--work DIR]
    uv run --project tools python tools/videojs_records.py summary [--sequence S] [--out DIR]
    uv run --project tools python tools/videojs_records.py swb [--sequence S] [--out DIR] \\
        [--work DIR] [--binary FILE]
    uv run --project tools python tools/videojs_records.py pixel-diff [--out DIR] [--work DIR]

Two sequences of step groups (`--sequence`), each with its own load of the page,
JavaScript on and the page's animations and transitions on (`replay
--animations on`):

- `values`: the first view after the hero reveal, then every animation and
  transition of the page sampled at fixed times. A group runs an action (a
  hover, a Tab, a click, a scroll) with no pause, and the following groups
  pause every running animation and set `currentTime` to its delay plus 0, 0.5
  and 0.95 of its duration (`window.__seekAll`; `window.__seek(name, ms)` for
  the hero reveal and the marquee); the first step of the next action finishes
  them, so that every action starts from the end state in both browsers. The
  record `window.__v()` (`data/videojs.js`) has the state, the animations, the
  computed values of their properties, and for the first view the rectangles
  and texts. Screenshots are taken in the same paused state (`animations:
  "allow"`); `points` chooses the pixel points from the three Chromium runs.
- `events`: the groups of `data/videojs-steps.json` (the capture's steps) with a
  group for the first view and one for the search button before them, run as
  they are with a pause of 300 ms after each step (no seeking), so that the
  events of transitions and animations come in their natural order. The
  record is `window.__v()` again; its `events` field is the log of the events
  since the last record.

`steps` writes `videojs-values-steps.json` and `videojs-events-steps.json`.
`records` runs `swbtools replay videojs --animations on` three times per
sequence and writes `videojs-S-chromium.json` (the records after the first
view and after each group), `videojs-S-orders.json` (the records whose events
differ in order or in number between the runs) and `videojs-S-console.json`;
for `values` also the screenshots in WORK/values/shotsN. It exits with status 1
if the runs differ in a record. The record after the load is not kept: it falls
in the reveal, whose time depends on the run. `points` writes
`videojs-pixel-points.json` and `videojs-pixels.json` from the screenshots, the
records and nothing else. `swb` runs a sequence in swb and writes what differs
from Chromium's records into WORK/S; `pixel-diff` lists the points where swb's
screenshots differ from Chromium's colors.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

import numpy as np

from swbtools import exit_records
from swbtools.exit_records import (
    FLAT_RADIUS,
    console_counts,
    differences,
    edge_mask,
    field_differences,
    labelled,
    parse_answers,
    run_swb_steps,
    scan,
    screenshot_name,
    split_log,
    spread,
    stable_flat,
    thin,
)
from swbtools.records import ROOT, load_json, run_replay, write_json

FIXTURE = "videojs"
RUNS = 3
DATA = ROOT / "crates" / "automation" / "tests" / "data"
WORK = ROOT / "out" / "videojs-exit"
SETUP = DATA / "videojs.js"
RECORD = "window.__v()"
# The pause after each step of the `events` sequence, as the test's
# `PAUSE_MS` (page time).
EVENTS_PAUSE_MS = 300
# The values sequence pauses nothing: a group's steps are the action and the
# seek, and the animations must not run between them.
VALUES_PAUSE_MS = 0
SEQUENCES = ("values", "events")
PAUSE_MS = {"values": VALUES_PAUSE_MS, "events": EVENTS_PAUSE_MS}
# The sampling times as fractions of an animation's duration (after its
# delay): start, middle and near the end.
FRACTIONS = (0, 0.5, 0.95)
# The URL of the page's analytics: the retries differ by their query.
POSTHOG = "https://us.i.posthog.com/"


def data_file(out: Path, sequence: str, name: str) -> Path:
    """The data file `name` of a sequence (`videojs-values-chromium.json`)."""
    return out / f"videojs-{sequence}-{name}.json"


def group(name: str, *steps: dict[str, Any]) -> dict[str, Any]:
    return {"name": name, "steps": list(steps)}


def js(code: str) -> dict[str, Any]:
    return {"eval": code}


def wait(ms: int) -> dict[str, Any]:
    return {"wait": ms}


# Selectors of the elements that the actions of the page's groups use (the
# steps of `videojs-steps.json`).
NAV_BLOG = 'nav a[href="/blog"]:visible >> nth=0'
NAV_DOCS = 'nav a[data-ph-capture-attribute-destination="docs"]:visible >> nth=0'
NAV_GITHUB = 'nav a[href*="github.com"]:visible >> nth=0'
SEARCH = ".DocSearch-Button"
AWAY = [640, 780]
PLAY = ".media-play-button:visible >> nth=-1"
TIME_SLIDER = ".media-time-slider"
SETTINGS = ".media-settings-menu-trigger"
APPEARANCE = 'button[aria-label="Appearance"]'
THEME = 'div[role="group"].grid > button >> nth={}'


# The focus actions after the groups `tab 1` to `tab 4`, which leave the focus
# on the link `Docs`: five presses of Tab reach the skin's container. The
# elements after it are not sampled: the play button and the settings button
# show a tooltip popup on focus, which swb closes differently (a `transform`
# transition where Chromium transitions `opacity`, `scale` and `filter`, and
# the popup stays open: backlog item of this feature); the mute button and the
# volume thumb open the volume popup after a delay that varies between runs;
# the seek thumb and the time toggle are focusable only with a loaded stream
# (M14 and M15); the picture-in-picture and fullscreen buttons do not exist in
# swb.
FOCUS_STEPS: list[tuple[str, list[dict[str, Any]]]] = [
    ("focus container", [{"key": "Tab"}] * 5),
]


def sampled(
    name: str, *action: dict[str, Any], kind: str = "CSSTransition", prefix: str = ""
) -> list[dict[str, Any]]:
    """Four groups for one action: the action that starts transitions, with
    the seek to the start of each running animation right after it (no
    pause), and the seeks to the middle and near the end. The action's first
    step finishes what the one before left paused."""
    first, *rest = (f"__seekFraction('{kind}', '{prefix}', {f})" for f in FRACTIONS)
    return [
        group(f"{name} start", js("__finish()"), *action, js(first)),
        group(f"{name} middle", js(rest[0])),
        group(f"{name} near end", js(rest[1])),
    ]


def seek(name: str, label: str, ms: int) -> dict[str, Any]:
    return group(label, js(f"__seek('{name}', {ms})"))


def values_groups() -> list[dict[str, Any]]:
    """The groups of the `values` sequence."""
    groups = [
        # Item 1: the first view after the hero reveal (it ends 1965 ms after
        # the script in Chromium: the reveal is over after 1000 ms more).
        group(
            "first view",
            js("window.__rects = true; window.__values = true; window.__targets = true"),
            wait(1000),
        ),
        # Item 2: the hero reveal, seeked from its end back to before its
        # delay and forwards (each element has its own delay: the page's
        # script sets it from `performance.now()`).
        seek("hero", "reveal before delay", -100),
        seek("hero", "reveal start", 0),
        seek("hero", "reveal middle", 550),
        seek("hero", "reveal near end", 1050),
        group("reveal end", js("__finishKind('CSSAnimation', 'hero')")),
    ]
    # Item 3: hover and focus transitions.
    hovers = [
        ("hover blog", {"hover": NAV_BLOG}),
        ("hover docs", {"hover": NAV_DOCS}),
        ("hover github", {"hover": NAV_GITHUB}),
        ("hover search", {"hover": SEARCH}),
        ("hover away", {"hover": AWAY}),
    ]
    groups.append(group("rectangles off", js("window.__rects = false")))
    for name, action in hovers:
        groups += sampled(name, action)
    for number in range(1, 5):
        groups += sampled(f"tab {number}", {"key": "Tab"})
    # Keyboard focus on the player: the focus ring of the skin's container
    # (`outline-offset`, `outline-color`, 0.1 s) is a focus transition of the
    # first view that no group sampled (`FOCUS_STEPS`). No link of the page has
    # a transition (the module comment of the test has the evidence), so the
    # groups of the links above only check that none starts.
    for name, action in FOCUS_STEPS:
        groups += sampled(name, *action)
    groups += sampled("focus away", js("document.activeElement.blur()"))
    player = [
        ("hover video", {"hover": [640, 300]}),
        ("hover play", {"hover": PLAY}),
        ("hover time slider", {"hover": TIME_SLIDER}),
        ("hover settings", {"hover": SETTINGS}),
        ("hover player away", {"hover": AWAY}),
    ]
    for name, action in player:
        groups += sampled(name, action)
    # The appearance menu (Base UI's popup: it starts its transitions one
    # frame after the click) and the theme change.
    groups += sampled("appearance open", {"click": APPEARANCE}, wait(60))
    groups += sampled("appearance hover", {"hover": THEME.format(1)})
    groups += sampled("theme dark", {"click": THEME.format(2)}, wait(60))
    groups += sampled("theme light", {"click": THEME.format(0)}, wait(60))
    groups += sampled("appearance close", {"key": "Escape"}, wait(60))
    # The marquee (endless: seeked to times of its 30 s, both sides of the
    # end of an iteration) and the bars that grow when they come into view.
    groups += sampled(
        "bars",
        {"hover": [640, 400]},
        {"wheel": [0, 600]},
        wait(400),
        {"wheel": [0, 400]},
        wait(100),
        kind="CSSAnimation",
        prefix="grow-bar",
    )
    groups += [
        group("bars end", js("__finishKind('CSSAnimation', 'grow-bar')")),
        seek("marquee", "marquee start", 0),
        seek("marquee", "marquee middle", 15000),
        seek("marquee", "marquee near end", 29900),
        seek("marquee", "marquee second iteration", 30100),
    ]
    groups += [
        group(
            "page end",
            {"wheel": [0, 700]},
            wait(500),
            {"wheel": [0, 700]},
            wait(500),
            {"wheel": [0, 700]},
            wait(1000),
        )
    ]
    # The spring at the end of the page: the wheels set an inline transform
    # (no transition), the release 180 ms after the last wheel starts `transform
    # 600ms`.
    groups += sampled(
        "spring",
        {"wheel": [0, 120]},
        {"wheel": [0, 120]},
        {"wheel": [0, 120]},
        wait(250),
    )
    groups.append(group("spring end", js("__finish()"), wait(100)))
    return groups


def events_groups() -> list[dict[str, Any]]:
    """The groups of the `events` sequence: the capture's step groups with
    a group for the first view and one for the search button."""
    captured = json.loads((DATA / "videojs-steps.json").read_text(encoding="utf-8"))
    search = group(
        "hover search button",
        {"hover": SEARCH},
        wait(400),
        {"hover": AWAY},
        wait(400),
    )
    renamed = {
        # No link of the page has a transition: the group checks that none
        # starts.
        "hover nav links": "hover nav links without transitions",
        # Tab: the skip link, the logo, the search button (the only element
        # with a transition), `Docs`.
        "focus links by keyboard": "tab to the search button",
    }
    out = []
    for one in captured:
        one["name"] = renamed.get(one["name"], one["name"])
        if one["name"] == "scroll to the bars":
            # The longer bar runs 3.25 s; the record must see it at its end.
            one["steps"].append(wait(1500))
        out.append(one)
        if one["name"] == "tab to the search button":
            # On to the player: the actions of `FOCUS_STEPS`, then the focus
            # leaves (blur).
            steps = [step for _, action in FOCUS_STEPS for step in action]
            out.append(
                group(
                    "tab to the player controls",
                    *steps,
                    js("document.activeElement.blur()"),
                    wait(400),
                )
            )
    return [group("first view", js("window.__events = true"), wait(1500)), search, *out]


def step_groups(sequence: str) -> list[dict[str, Any]]:
    return values_groups() if sequence == "values" else events_groups()


def write_steps(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    for sequence in SEQUENCES:
        write_json(data_file(out, sequence, "steps"), step_groups(sequence))
    print(f"steps written to {out}")


def replay_once(out: Path, work: Path, sequence: str, run: int, shots: bool) -> list[Any]:
    """One run of `swbtools replay videojs --animations on`; returns its
    records (the one after the load first)."""
    records = work / f"run{run}.json"
    cmd = ["uv", "run", "--project", "tools", "swbtools", "replay", FIXTURE, "--animations", "on"]
    cmd += ["--steps", str(data_file(out, sequence, "steps"))]
    cmd += ["--setup", str(SETUP), "--record", RECORD, "--out", str(records)]
    cmd += ["--pause-ms", str(PAUSE_MS[sequence])]
    cmd += ["--missing-out", str(work / f"run{run}-missing.json")]
    cmd += ["--console-out", str(work / f"run{run}-console.txt")]
    if shots:
        cmd += ["--screenshots", str(work / f"shots{run}")]
    run_replay(cmd, work / f"run{run}.log")
    return load_json(records)


def unify_event_orders(runs: list[list[Any]]) -> tuple[list[int], dict[int, list[Any]]]:
    """For each record in which the runs have the same events in a different
    order, makes the events of the later runs those of the first run and
    lists the record number (the test compares these records as sets).
    Chromium dispatches the events of different elements that are due at the
    same time in an order that varies between runs. For each record in which
    the runs have different events (a transition that starts or not in a
    frame that depends on the timing of the run), lists, per event, the
    smallest and the largest number of times in a run (the test takes the
    range); the later runs take the first run's events."""

    def key(event: Any) -> str:
        return json.dumps(event)

    sets: list[int] = []
    ranges: dict[int, list[Any]] = {}
    for number, first in enumerate(runs[0]):
        logs = [run[number]["events"] for run in runs]
        if all(log == logs[0] for log in logs):
            continue
        counts = [Counter(map(key, log)) for log in logs]
        if all(c == counts[0] for c in counts):
            sets.append(number)
        else:
            events = sorted(set().union(*counts))
            ranges[number] = [
                [json.loads(e), min(c[e] for c in counts), max(c[e] for c in counts)]
                for e in events
            ]
        for run in runs[1:]:
            run[number]["events"] = first["events"]
    return sets, ranges


def write_records(out: Path, work: Path, sequence: str) -> None:
    """Three runs of Chromium: the records, the missing URLs, the console and
    the screenshots. Exits with status 1 if the runs differ in the records."""
    write_steps(out)
    work = work / sequence
    work.mkdir(parents=True, exist_ok=True)
    runs, missing, console = [], [], []
    for run in range(RUNS):
        records = replay_once(out, work, sequence, run, sequence == "values")
        runs.append(records[1:])
        rows = load_json(work / f"run{run}-missing.json")
        missing.append(exit_records.mask_missing(rows[1:], (POSTHOG,)))
        text = (work / f"run{run}-console.txt").read_text(encoding="utf-8")
        console.append(exit_records.page_console(text))
    sets, ranges = unify_event_orders(runs)
    diff = [d for other in runs[1:] for d in differences(runs[0], other)]
    print(f"{len(runs[0])} records; the runs differ in {len(diff)} fields")
    for number in sets:
        print(f"  record {number}: the order of the events differs between the runs (a set)")
    for number in ranges:
        print(f"  record {number}: the events differ between the runs (a range per event)")
    for line in dict.fromkeys(diff):
        print("  " + line)
    # The URLs that the fixture lacks are prefetches of links (`/privacy`
    # when a link passes under the pointer): they differ by run and are no
    # data of the test; the console check covers their messages.
    absent = sorted({u for run in missing for row in run for u in row["missing"]})
    print(f"URLs that the fixture lacks (all runs): {absent}")
    varying = [m for m in console_counts(console) if m["min"] != m["max"]]
    print(f"console: {len(varying)} messages with a count that differs between the runs")
    if diff:
        # Keep a copy of the first run for looking at what differs.
        write_json(work / "differing-run0.json", runs[0])
        write_json(work / "differing-run1.json", runs[1])
        sys.exit(1)
    write_json(data_file(out, sequence, "chromium"), runs[0])
    if sequence == "events":
        write_json(
            data_file(out, sequence, "orders"),
            {"set": sets, "range": {str(n): r for n, r in ranges.items()}},
        )
    write_json(data_file(out, sequence, "console"), console_counts(console))


# The pixel check (values sequence): per group about the number of points
# below, each flat (one color in a 7 x 7 square) and equal in the three
# Chromium runs. Points of the first view and the reveal are more.
GRID_STRIDE = 40
GRID_COUNT = {"first view": 16, "default": 6}
RECT_PER_AREA = 8
TARGET_PER_GROUP = 12
EDGE_COUNT = {"first view": 14, "default": 8}
EDGE_SPREAD = 24
RECT_SPREAD = 8


def rect_candidates(left: float, top: float, width: float, height: float) -> list[tuple[int, int]]:
    """Points inside a rectangle: the center, then the four corners inset by 5
    px (only for a rectangle of 14 px or more)."""
    points = [(int(left + width / 2), int(top + height / 2))]
    if width >= 14 and height >= 14:
        a, b = int(left) + 5, int(top) + 5
        c, d = int(left + width) - 6, int(top + height) - 6
        points += [(a, b), (c, b), (a, d), (c, d)]
    return points


def choose_group(
    label: str, shots: list[Any], record: dict[str, Any]
) -> dict[str, tuple[int, int]]:
    """The points of one group, from its three screenshots (arrays) and its
    record. Every point is a flat color, equal in the three shots. The
    kinds, in order:

    - `rect-AREA-N`: inside the rectangles of the first view (the record has
      them only for the groups that set `window.__rects`);
    - `target-N`: inside the rectangles of the elements whose animations the
      record lists (`targets`);
    - `edge-N`: flat points next to a color edge;
    - `grid-N`: a coarse grid over the viewport.
    """
    flat = stable_flat(shots)
    height, width = flat.shape
    chosen: dict[str, tuple[int, int]] = {}
    taken: list[tuple[int, int]] = []

    def ok(x: int, y: int) -> bool:
        edge = FLAT_RADIUS
        return edge <= x < width - edge and edge <= y < height - edge and bool(flat[y, x])

    def far(x: int, y: int, distance: int) -> bool:
        return all(abs(x - a) >= distance or abs(y - b) >= distance for a, b in taken)

    def take(name: str, at: tuple[int, int]) -> None:
        chosen[name] = at
        taken.append(at)

    for area, entries in record.get("rects", {}).items():
        found = [
            at
            for entry in entries
            for at in [
                next(
                    (p for p in rect_candidates(*entry[2:6]) if ok(*p) and far(*p, 1)),
                    None,
                )
            ]
            if at is not None
        ]
        for number, at in enumerate(thin(spread(found, RECT_SPREAD), RECT_PER_AREA), 1):
            take(f"rect-{area}-{number}", at)
    targets = [
        at
        for entry in record.get("targets", [])
        for at in [next((p for p in rect_candidates(*entry[1:5]) if ok(*p) and far(*p, 1)), None)]
        if at is not None
    ]
    for number, at in enumerate(thin(spread(targets, 2), TARGET_PER_GROUP), 1):
        take(f"target-{number}", at)
    near_edge = flat & edge_mask(shots[0])
    found = [
        p for p in scan(near_edge, (0, 0, width, height), 2) if ok(*p) and far(*p, RECT_SPREAD)
    ]
    count = EDGE_COUNT.get(label, EDGE_COUNT["default"])
    for number, at in enumerate(thin(spread(found, EDGE_SPREAD), count), 1):
        take(f"edge-{number}", at)
    grid_area = (GRID_STRIDE // 2, GRID_STRIDE // 2, width, height)
    grid = [p for p in scan(flat, grid_area, GRID_STRIDE) if ok(*p) and far(*p, RECT_SPREAD)]
    count = GRID_COUNT.get(label, GRID_COUNT["default"])
    for number, at in enumerate(thin(grid, count), 1):
        take(f"grid-{number}", at)
    return chosen


def write_points(out: Path, work: Path) -> None:
    """Writes the pixel points and the colors at them
    (`videojs-pixel-points.json`, `videojs-pixels.json`) from the three runs
    of `records --sequence values`."""
    from PIL import Image

    work = work / "values"
    records = load_json(data_file(out, "values", "chromium"))
    labels = [g["name"] for g in step_groups("values")]
    labels = ["first view", *labels[1:]]
    points: dict[str, list[int]] = {}
    colors: dict[str, dict[str, list[int]]] = {}
    claims: dict[str, list[str]] = {}
    for number, label in enumerate(labels):
        shots = []
        for run in range(RUNS):
            path = work / f"shots{run}" / screenshot_name(number + 1, label)
            with Image.open(path) as image:
                shots.append(np.asarray(image.convert("RGB")))
        chosen = choose_group(label, shots, records[number])
        prefix = f"{number:03d}-"
        claims[label] = [prefix]
        points.update({f"{prefix}{key}": list(at) for key, at in chosen.items()})
        colors[label] = {
            f"{prefix}{key}": [int(c) for c in shots[0][y, x]] for key, (x, y) in chosen.items()
        }
    spec = {"groups": labels, "group_points": claims, "points": points}
    write_json(out / "videojs-pixel-points.json", spec)
    write_json(out / "videojs-pixels.json", colors)
    print(f"{len(points)} points in {len(labels)} groups written to {out}")


def summary(out: Path, sequence: str) -> None:
    records = load_json(data_file(out, sequence, "chromium"))
    names = [g["name"] for g in step_groups(sequence)]
    print("| # | group | animations | values | events |")
    print("|---|---|---|---|---|")
    for number, (name, rec) in enumerate(zip(names, records, strict=False)):
        kinds = sorted({f"{a[0][3:]}:{a[1]}" for a in rec["animations"]})
        events = " ".join(
            f"{e[0].replace('transition', 't').replace('animation', 'a')}" for e in rec["events"]
        )
        animations = f"{len(rec['animations'])} {kinds[:4]}"
        print(f"| {number} | {name} | {animations} | {len(rec['values'])} | {events[:100]} |")


def swb_steps(
    groups: list[dict[str, Any]], shots: Path, pause_ms: int, setup: str
) -> list[dict[str, Any]]:
    """The flat step list of one swb run: the setup script, then a `record`
    step and a `screenshot` step after the first view and after each group;
    a pause after each step (`pause_ms`), as `replay` has."""
    steps: list[dict[str, Any]] = [{"eval": setup}]
    for number, one in enumerate(groups, 1):
        for step in one["steps"]:
            steps.append(step)
            if pause_ms:
                steps.append({"wait": pause_ms})
        steps += [
            {"record": RECORD},
            {"screenshot": str(shots / screenshot_name(number, one["name"]))},
        ]
    return steps


def run_swb_groups(sequence: str, out: Path, work: Path, binary: Path) -> None:
    """The `swb` command: one run of the groups, the comparison, the files."""
    from swbtools import paths
    from swbtools.compare import swb_js_options
    from swbtools.pages import read_meta
    from swbtools.swb import replay_options

    groups = step_groups(sequence)
    target = work / sequence
    shots = target / "swb-shots"
    shots.mkdir(parents=True, exist_ok=True)
    fixture = paths.js_fixture_dir(FIXTURE)
    meta = read_meta(fixture)
    steps = swb_steps(groups, shots, PAUSE_MS[sequence], SETUP.read_text(encoding="utf-8"))
    command = [
        *replay_options(binary, fixture, tuple(meta.viewport), animations=True),
        *swb_js_options(True, meta.captured),
        "--steps",
        json.dumps(steps),
        meta.url,
    ]
    lines, status = run_swb_steps(command, target / "swb-log.txt")
    records, arrival = parse_answers(lines)
    names = [g["name"] for g in groups]
    expected = load_json(data_file(out, sequence, "chromium"))
    diffs = [field_differences(e, a) for e, a in zip(expected, records, strict=False)]
    seconds = [round(b - a, 3) for a, b in zip([0.0, *arrival], arrival, strict=False)]
    log_groups = split_log((target / "swb-log.txt").read_text(encoding="utf-8"))
    write_json(target / "swb-records.json", records)
    write_json(target / "swb-diff.json", labelled(names, "fields", diffs))
    write_json(target / "swb-groups.json", labelled(names, "log", log_groups))
    print(f"swb status {status}; {len(records)} of {len(names)} records")
    for name, fields, took in zip(names, diffs, seconds, strict=False):
        print(f"{took:7.2f} s  {name}: {', '.join(fields) or 'equal'}")


PIXEL_TOLERANCE = 12
"""The largest difference of a color channel that the test accepts at a pixel
point (`PIXEL_TOLERANCE` in the test)."""


def pixel_diff(out: Path, work: Path) -> None:
    """The points where swb's screenshots (`swb --sequence values`) differ
    from Chromium's colors by more than the tolerance."""
    from PIL import Image

    work = work / "values"
    spec = load_json(out / "videojs-pixel-points.json")
    colors = load_json(out / "videojs-pixels.json")
    found: dict[str, list[dict[str, Any]]] = {}
    for number, label in enumerate(spec["groups"]):
        with Image.open(work / "swb-shots" / screenshot_name(number + 1, label)) as image:
            shot = np.asarray(image.convert("RGB"))
        prefix = f"{number:03d}-"
        rows = []
        for name, (x, y) in spec["points"].items():
            if not name.startswith(prefix):
                continue
            got = [int(c) for c in shot[y, x]]
            want = colors[label][name]
            if any(abs(g - w) > PIXEL_TOLERANCE for g, w in zip(got, want, strict=True)):
                rows.append({"name": name, "at": [x, y], "swb": got, "chromium": want})
        found[label] = rows
        if rows:
            print(f"{number:3d} {label}: {len(rows)} of {len(colors[label])} points differ")
    write_json(work / "pixel-diff.json", found)
    print(f"{sum(len(r) for r in found.values())} points differ; see {work / 'pixel-diff.json'}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = ["steps", "records", "points", "summary", "swb", "pixel-diff"]
    parser.add_argument("command", choices=commands)
    parser.add_argument("--sequence", choices=SEQUENCES, default="values")
    parser.add_argument("--out", type=Path, default=DATA, help="the data directory of the test")
    parser.add_argument("--work", type=Path, default=WORK, help="runs, logs, screenshots")
    parser.add_argument("--binary", type=Path, help="swb: the swb binary")
    args = parser.parse_args()
    if args.command == "swb":
        from swbtools import swb as swb_tools

        binary = swb_tools.find_swb(args.binary)
        if binary is None:
            sys.exit("no swb binary (build it, or pass --binary)")
        run_swb_groups(args.sequence, args.out, args.work, binary)
    elif args.command == "records":
        write_records(args.out, args.work, args.sequence)
    elif args.command == "points":
        write_points(args.out, args.work)
    elif args.command == "pixel-diff":
        pixel_diff(args.out, args.work)
    elif args.command == "summary":
        summary(args.out, args.sequence)
    else:
        write_steps(args.out)


if __name__ == "__main__":
    main()
