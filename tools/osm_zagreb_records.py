"""The step groups, the record expression and the Chromium data for the
OpenStreetMap target of M12 (`fixtures/js/osm-zagreb`; docs/testing.md,
"Capture with step groups"; the test is
`crates/automation/tests/osm_zagreb.rs`). Run from the repository root:

    uv run --project tools python tools/osm_zagreb_records.py steps [--out DIR]
    uv run --project tools python tools/osm_zagreb_records.py records [--sequence S] \\
        [--out DIR] [--work DIR]
    uv run --project tools python tools/osm_zagreb_records.py points [--sequence S] \\
        [--out DIR] [--work DIR]
    uv run --project tools python tools/osm_zagreb_records.py summary [--sequence S] [--out DIR]
    uv run --project tools python tools/osm_zagreb_records.py swb [--sequence S] [--fixture NAME] \\
        [--groups N] [--skip N...] [--out DIR] [--chromium FILE] [--binary FILE]
    uv run --project tools python tools/osm_zagreb_records.py pixel-diff [--sequence S] \\
        [--out DIR] [--work DIR]

There are two sequences of step groups (`--sequence`): `full` (the files
`osm-zagreb-*`: the 60 groups) and `no-transport` (the files `osm-zagreb-nt-*`:
the same without the group `layer transport map`). The Transport Map needs
WebGL: in Chromium without WebGL its click breaks the layer, so that the map
has no tiles from that group on. The second sequence keeps the tiles for the
groups after it. Each sequence has its own files and its own work directory
(`WORK/full`, `WORK/no-transport`).

`steps` writes the data of the test into DIR (default
`crates/automation/tests/data`): `osm-zagreb-steps.json` and
`osm-zagreb-nt-steps.json` (the step groups) and `osm-zagreb.js` (the setup
script that defines `window.__o()`, the record expression). The live capture
of the fixture, with the groups run against the real site (one run; the tile
usage policy of tile.openstreetmap.org forbids bulk downloads):

    uv run --project tools swbtools capture --js --force --max-responses 1000 \\
        --steps crates/automation/tests/data/osm-zagreb-steps.json \\
        'https://www.openstreetmap.org/?mlat=45.813114&mlon=15.976390#map=16/45.81308/15.97721' \\
        osm-zagreb

`records` runs `swbtools replay osm-zagreb` three times with those files (no
network) in Chromium without WebGL (`--chromium-arg=--disable-3d-apis`, the
owner decision of 2026-11-18: the Transport Map and the thumbnails of the
layer panel stay empty), saves the screenshots of each run (`WORK/S/shots0`,
`shots1`, `shots2`), and writes into DIR `osm-zagreb-chromium.json` (the
record after the load and after each group, with the rectangles of item 1),
`osm-zagreb-missing.json` (the URLs that the fixture lacks, per group, with
the Matomo URLs masked) and `osm-zagreb-console.json` (the console messages of
the page at level error and warning and the uncaught exceptions, with the
number of times that each appeared in a run: a range, if the runs differ). It
exits with status 1 if the three runs differ in a record or in the missing
URLs; the console counts may differ (they become a range).
`points` chooses the pixel points from the screenshots of the three
Chromium runs and the records (never from swb): see `choose_points`. It
writes `osm-zagreb-pixel-points.json` and the colors at the points,
`osm-zagreb-pixels.json`. Order of the commands: `steps`, `records`, `points`
(a change of the steps or of the record expression needs all three again).
`summary` prints a table of the records.

`swb` runs the step groups in swb on a fixture (default `osm-zagreb`):
one `swb --headless` run with the steps of all groups, a `record` step and a
`screenshot` step after the load and after each group. It compares each record with Chromium's
(`osm-zagreb-chromium.json` in the data directory, or `--chromium FILE`) and
writes `swb-records.json` (the records), `swb-diff.json` (the differing
fields per record), `swb-times.json` (wall time per record), `swb-log.txt`
(stderr), `swb-groups.json` (the log lines of each group) and `swb-shots/`
into `--out` (default `out/osm-zagreb/S`). `pixel-diff` then lists the pixel
points where swb's screenshots differ from Chromium's colors (by more than
the tolerance of the test) and writes `pixel-diff.json` next to them: the
input for the departures file of the test.
"""

import argparse
import sys
from pathlib import Path
from typing import Any

import numpy as np

from swbtools import exit_records
from swbtools.exit_records import (
    FLAT_RADIUS,
    compare_swb_run,
    console_counts,
    differences,
    edge_mask,
    group,
    inside,
    scan,
    screenshot_name,
    spread,
    stable_flat,
    swb_replay_command,
    thin,
    wait,
)
from swbtools.records import ROOT, load_json, run_replay, write_json
from swbtools.replay import DEFAULT_PAUSE_MS

FIXTURE = "osm-zagreb"
URL = "https://www.openstreetmap.org/?mlat=45.813114&mlon=15.976390#map=16/45.81308/15.97721"
RUNS = 3
DATA = ROOT / "crates" / "automation" / "tests" / "data"
WORK = ROOT / "out" / "osm-zagreb"
# Chromium without WebGL (owner decision of 2026-11-18).
CHROMIUM_ARGS = ["--chromium-arg=--disable-3d-apis"]
# The URLs of the page's analytics: the query has the time and a random
# number, so the URLs differ in every run. The data keep the path.
MATOMO = "https://matomo.openstreetmap.org/matomo.php"

# The sequences: name, the prefix of the data files, the groups left out.
SEQUENCES = {
    "full": ("osm-zagreb", ()),
    "no-transport": ("osm-zagreb-nt", ("layer transport map",)),
}


def data_file(out: Path, sequence: str, name: str) -> Path:
    """The data file `name` ("steps", "chromium", ...) of a sequence."""
    return out / f"{SEQUENCES[sequence][0]}-{name}.json"


def work_dir(work: Path, sequence: str) -> Path:
    return work / sequence


# The controls of the map (`.control-button`, in this order).
ZOOM_IN = ".control-button.plus-lg"
ZOOM_OUT = ".control-button.dash-lg"
LAYERS = ".control-button >> nth=3"
LEGEND = ".control-button >> nth=4"
SHARE = ".control-button >> nth=5"
QUERY = ".control-button >> nth=7"

# A point of the map that no control or sidebar covers (1280 x 800).
MAP = [800, 450]

# Closes the pane of the right sidebar (layers, key, share): the visible one
# of the two close buttons of `#map-ui`.
CLOSE_RIGHT = {"click": "#map-ui .btn-close.rounded-5"}
# Closes the page of the left sidebar (results, object, directions, query,
# history, export) with its first visible close button; nothing happens if no
# page is open. A `click` step cannot say "the visible one" for a button that
# the page has twice.
CLOSE_LEFT = {
    "eval": "([...document.querySelectorAll('#sidebar .btn-close')].find((b) =>"
    " b.getBoundingClientRect().width > 0 && !b.classList.contains('invisible'))"
    " || {click() {}}).click()"
}


def reset_view(item: str, hash_: str = "#map=16/45.81308/15.97721") -> dict[str, Any]:
    """Sets the view by the URL hash, as a user does in the address bar."""
    return group(f"reset view {item}", {"eval": f"location.hash = '{hash_}'"}, wait(2500))


def layer(name: str, code: str) -> dict[str, Any]:
    return group(f"layer {name}", {"click": f"label[for='map-ui-layer-{code}']"}, wait(3500))


def overlay(name: str, label: str, wait_ms: int = 3500) -> dict[str, Any]:
    return group(f"overlay {name}", {"click": f"#label-layers-{label}"}, wait(wait_ms))


def all_step_groups() -> list[dict[str, Any]]:
    """One group per user action; the order matters: each group starts from
    the state that the one before leaves (the map at zoom 16, the sidebars
    closed)."""
    return [
        # Item 6: the welcome box.
        group("welcome close", {"click": ".welcome .btn-close"}, wait(500)),
        # Item 3: the map. Zoom 16 to 17 and back, never below 13 or above 18.
        group(
            "drag",
            {"hover": MAP},
            {"mousedown": "left"},
            {"hover": [780, 445]},
            {"hover": [750, 440]},
            {"hover": [720, 435]},
            wait(300),
            {"mouseup": "left"},
            wait(1500),
        ),
        group("wheel in", {"hover": MAP}, {"wheel": [0, -60]}, wait(2000)),
        group("wheel out", {"wheel": [0, 60]}, wait(2000)),
        group("double click", {"dblclick": MAP}, wait(2000)),
        group("zoom out button", {"click": ZOOM_OUT}, wait(2000)),
        group("zoom in button", {"click": ZOOM_IN}, wait(2000)),
        group("key minus", {"focus": "#map"}, {"key": "-"}, wait(2000)),
        group("key plus", {"key": "+"}, wait(2000)),
        group("key minus again", {"key": "-"}, wait(2000)),
        group("key arrow right", {"key": "ArrowRight"}, wait(1500)),
        # Item 4: the layer panel, at zoom 16.
        group("layers open", {"click": LAYERS}, wait(1500)),
        layer("cyclosm", "cyclosm"),
        layer("cycle map", "cyclemap"),
        layer("transport map", "transportmap"),
        layer("tracestrack topo", "tracestracktopo"),
        layer("humanitarian", "hot"),
        layer("standard", "mapnik"),
        overlay("map notes", "notes"),
        overlay("map data", "data", 4000),
        overlay("public gps traces", "gps"),
        group(
            "overlays off",
            {"click": "#label-layers-notes"},
            {"click": "#label-layers-data"},
            {"click": "#label-layers-gps"},
            wait(1500),
        ),
        # "Map Data" at zoom 16 asks "Load Data" (15410 features); at zoom 18
        # it loads the data of the view at once.
        group("zoom in to 17", {"click": ZOOM_IN}, wait(2500)),
        group("zoom in to 18", {"click": ZOOM_IN}, wait(2500)),
        overlay("map data at 18", "data", 5000),
        overlay("map data off", "data", 1500),
        group("zoom out to 17", {"click": ZOOM_OUT}, wait(2000)),
        group("zoom out to 16", {"click": ZOOM_OUT}, wait(2000)),
        group("layers close", CLOSE_RIGHT, wait(800)),
        # Item 5: the map key and the share panel.
        group("legend open", {"click": LEGEND}, wait(2500)),
        group("legend close", CLOSE_RIGHT, wait(800)),
        group("share open", {"click": SHARE}, wait(2500)),
        group("share short link", {"click": "#short_link"}, wait(500)),
        group("share html", {"click": "#embed_link"}, wait(500)),
        group("share marker", {"click": "#link_marker"}, wait(500)),
        group("share close", CLOSE_RIGHT, wait(800)),
        # Item 7: search. Each of the items 7 to 10 starts at a known view
        # (what a user gets by typing the hash into the address bar).
        reset_view("search"),
        group(
            "search",
            {"click": "input#query:visible"},
            {"type": "Trg bana Jelačića"},
            {"key": "Enter"},
            wait(4000),
        ),
        group(
            "search result",
            {"click": "li.search_results_entry a.set_position >> nth=0"},
            wait(4000),
        ),
        group("search close", CLOSE_LEFT, wait(1500)),
        # Item 8: "Where is this?" and the object page of its Nominatim result
        # (the internal result has no `data-id`).
        reset_view("where is this"),
        group("where is this", {"click": "button.describe_location:visible"}, wait(4000)),
        group(
            "where is this result",
            {"click": "li.search_results_entry a[data-id] >> nth=0"},
            wait(5000),
        ),
        group("where is this close", CLOSE_LEFT, wait(1500)),
        # Items 10 and 11: "Query features" (two Overpass requests; the view
        # is the one of the first recording, which had nearby and enclosing
        # features), then a relation, one of its member ways and a node of
        # that way, each by a click on a link of the page before. No `back`
        # in this part: it would repeat the two requests, and the service
        # answers a repeat within a minute with 429.
        reset_view("query", "#map=16/45.81269/15.97861"),
        group("query open", {"click": QUERY}, wait(2000)),
        group("query click", {"click": [760, 430]}, wait(10000)),
        group(
            "object relation",
            {"click": "#sidebar_content a[href^='/relation/'] >> nth=0"},
            wait(5000),
        ),
        group(
            "object way",
            {"click": "#sidebar_content details a[href^='/way/'] >> nth=0"},
            wait(5000),
        ),
        # The node: back to the list of the query (two `back`; the two
        # requests of the query are in the fixture, and the replay answers
        # them), then the first node of "Nearby features". The live run of
        # these steps had no such group; the fixture got the node's responses
        # from an earlier recording.
        group(
            "object node",
            {"back": True},
            wait(3000),
            {"back": True},
            wait(5000),
            {"click": "#sidebar_content a[href^='/node/'] >> nth=0"},
            wait(5000),
        ),
        group("object close", CLOSE_LEFT, wait(1500)),
        # Item 9: directions with the default engine (the route moves the map,
        # so this comes after the items that need a known view).
        reset_view("directions"),
        group("directions open", {"click": ".switch_link:visible"}, wait(2000)),
        group(
            "directions fill",
            {"click": "#route_from:visible"},
            {"type": "Trg bana Jelačića"},
            {"click": "#route_to:visible"},
            {"type": "Glavni kolodvor"},
            wait(1000),
        ),
        group("directions go", {"key": "Enter"}, wait(7000)),
        group("directions close", CLOSE_LEFT, wait(1500)),
        # Item 12: History and Export, then back and forward.
        group("history", {"click": "header a[href*='/history']:visible"}, wait(5000)),
        group("export", {"click": "header a[href*='/export']:visible"}, wait(3000)),
        group("back to history", {"back": True}, wait(4000)),
        group("forward to export", {"forward": True}, wait(3000)),
    ]


# The pixel check. After the load and after every group the test reads the
# colors at points that `points` chose from the screenshots of the three
# Chromium runs and the records alone (see `choose_points`).
SIZE = (1280, 800)
MAP_AREA = (362, 112, 1226, 744)
SIDEBAR_AREA = (4, 104, 346, 776)
# Points that exist in every group (fixed parts of the page): name and x, y.
FIXED_POINTS = {
    "header": (700, 8),
    "header-gap": (600, 44),
    "search-box": (140, 90),
    "search-button": (268, 70),
    "directions-button": (312, 70),
    "zoom-out": (1244, 112),
    "locate": (1244, 150),
    "layers": (1244, 200),
    "share": (1244, 300),
    "notes": (1244, 330),
    "help": (1244, 380),
    "scale": (88, 787),
}
# Points of one group that the scan does not find: the white dot of the
# marker, the icons of the directions steps, the stroke of the route (read
# off Chromium's screenshots).
EXTRA_POINTS = {
    "load": {"marker-dot": (603, 402)},
    "directions go": {
        "step-icon-1": (25, 294),
        "step-icon-2": (24, 324),
        "step-icon-3": (25, 354),
        "step-icon-4": (26, 384),
        "step-icon-8": (24, 504),
        "step-icon-10": (25, 606),
        "route-1": (704, 372),
        "route-2": (846, 466),
    },
    # The spinner of the History pane ("Changesets": a ring of 32 px at
    # (159, 167) with a transparent right border): the white gap at its right.
    "history": {"spinner-gap": (188, 183)},
    "back to history": {"spinner-gap": (188, 183)},
}
# Points of a drawing that has no square of one color (the resize handle of
# the share panel's text area: two strokes of 1 px at its bottom right, each
# pixel black with alpha 0.6 over white = 102): the color must be the same in
# all runs, flat or not (read off Chromium's screenshots, item D of part 4c).
EXACT_POINTS = {
    group: {
        "resizer-top": (1261, 292),
        "resizer-middle": (1258, 295),
        "resizer-short-top": (1261, 296),
        "resizer-short-bottom": (1260, 297),
    }
    for group in ("share html", "share marker")
}
# The thumbnails of the layer list need WebGL: no point inside one of them
# (the `.base-layers > div` rectangles of the record), with a margin in px.
THUMBNAILS = ".base-layers > div"
THUMBNAIL_MARGIN = 3
# Points per group and kind: the rectangles of each area of the record (the
# first of the candidates of the rectangles, evenly over the list), the map
# and the sidebar near a color edge (a flat point within 10 px of a color that
# differs by more than 60), and a coarse grid.
RECT_PER_AREA = 3
EDGE_PER_AREA = 5
INK_COUNT = 8
GRID_STRIDE = 40
GRID_COUNT = 8
EDGE_SPREAD = 24
RECT_SPREAD = 8
# The rectangles whose glyph or drawing gets an "ink" point: the pixel of the
# largest contrast to the rectangle's main color, with a neighbor of the same
# color (a stroke of 2 px or more: its core has the glyph's own color on any
# background), equal in all runs. The value is the search region: `all` the
# rectangle, or its `corner` of 14 x 14 px (the resize grabber of a textarea).
# A glyph has no 7 x 7 square of one color, so a flat point cannot see it. An
# ink point may lie in a thumbnail of the layer list (the "(i)" button is
# drawn over it; the core pixel of the glyph does not depend on what is
# behind it).
INK_RECTS = {
    ".layer-info-toggle i": "all",
    ".control-button": "all",
    "#sidebar .btn-close": "all",
    "#map-ui .btn-close": "all",
    "#nav-tabContent textarea": "corner",
}
INK_CONTRAST = 100
CORNER = 14


def thumbnail_boxes(record: dict[str, Any]) -> list[tuple[int, int, int, int]]:
    """The thumbnails of the layer list in a record, as (left, top, right,
    bottom) with the margin."""
    boxes = []
    for selector, _, x, y, w, h in record["rects"]["panel"]:
        if selector == THUMBNAILS:
            m = THUMBNAIL_MARGIN
            boxes.append((int(x) - m, int(y) - m, int(x + w) + m + 1, int(y + h) + m + 1))
    return boxes


def rect_candidates(entry: list[Any]) -> list[tuple[int, int]]:
    """Points inside a rectangle of the record: the center, then the four
    corners inset by 5 px (only for a rectangle of 14 px or more)."""
    _, _, x, y, w, h = entry
    points = [(int(x + w / 2), int(y + h / 2))]
    if w >= 14 and h >= 14:
        left, top, right, bottom = int(x) + 5, int(y) + 5, int(x + w) - 6, int(y + h) - 6
        points += [(left, top), (right, top), (left, bottom), (right, bottom)]
    return points


def ink_point(shots: list[Any], entry: list[Any], region: str) -> tuple[int, int] | None:
    """The "ink" point of a rectangle of the record (see `INK_RECTS`), or
    None if no pixel has a contrast of `INK_CONTRAST` or more to the main
    color of the region, a neighbor of the same color and the same color in
    all shots."""
    _, _, x, y, w, h = entry
    height, width = shots[0].shape[:2]
    left, top = max(int(x), 1), max(int(y), 1)
    right, bottom = min(int(x + w), width - 1), min(int(y + h), height - 1)
    if region == "corner":
        left, top = max(left, right - CORNER), max(top, bottom - CORNER)
    if right - left < 3 or bottom - top < 3:
        return None
    area = shots[0][top:bottom, left:right].astype("int16")
    same = np.ones(area.shape[:2], dtype=bool)
    for shot in shots[1:]:
        same &= (shot[top:bottom, left:right] == shots[0][top:bottom, left:right]).all(axis=2)
    colors, counts = np.unique(area.reshape(-1, 3), axis=0, return_counts=True)
    contrast = np.abs(area - colors[counts.argmax()]).max(axis=2)
    stroke = np.zeros(area.shape[:2], dtype=bool)
    across = (area[:, :-1] == area[:, 1:]).all(axis=2)
    stroke[:, :-1] |= across
    stroke[:, 1:] |= across
    down = (area[:-1] == area[1:]).all(axis=2)
    stroke[:-1] |= down
    stroke[1:] |= down
    score = np.where(same & stroke, contrast, 0)
    row, column = np.unravel_index(score.argmax(), score.shape)
    if score[row, column] < INK_CONTRAST:
        return None
    return left + int(column), top + int(row)


def choose_group(
    label: str, shots: list[Any], record: dict[str, Any]
) -> dict[str, tuple[int, int]]:
    """The points of one group, from its three screenshots (arrays) and its
    record. Every point is a flat color, equal in the three shots, and outside
    the thumbnails. The kinds, in order:

    - `fixed`: the fixed parts of the page, and the extras of the group;
    - `rect-AREA-N`: points inside the rectangles of the record (for the
      layer panel, the share panel, the sidebar pages, the map controls);
    - `edge-AREA-N`: flat points near a color edge in the map and the sidebar;
    - `grid-N`: a coarse grid over the whole viewport.
    """
    flat = stable_flat(shots)
    height, width = flat.shape
    avoid = thumbnail_boxes(record)

    def ok(x: int, y: int) -> bool:
        edge = FLAT_RADIUS
        inner = edge <= x < width - edge and edge <= y < height - edge
        return inner and bool(flat[y, x]) and not inside(x, y, avoid)

    chosen: dict[str, tuple[int, int]] = {}
    taken: list[tuple[int, int]] = []

    def far(x: int, y: int, distance: int) -> bool:
        return all(abs(x - a) >= distance or abs(y - b) >= distance for a, b in taken)

    def take(name: str, at: tuple[int, int]) -> None:
        chosen[name] = at
        taken.append(at)

    for key, at in {**FIXED_POINTS, **EXTRA_POINTS.get(label, {})}.items():
        if ok(*at):
            take(f"{key}-1", at)
    for area, entries in record["rects"].items():
        found = [
            at
            for entry in entries
            for at in [next((p for p in rect_candidates(entry) if ok(*p) and far(*p, 1)), None)]
            if at is not None
        ]
        for number, at in enumerate(thin(spread(found, RECT_SPREAD), RECT_PER_AREA), 1):
            take(f"rect-{area}-{number}", at)
    inks = [
        at
        for entries in record["rects"].values()
        for entry in entries
        if entry[0] in INK_RECTS
        for at in [ink_point(shots, entry, INK_RECTS[entry[0]])]
        if at is not None and far(*at, 1)
    ]
    for number, at in enumerate(thin(spread(inks, 1), INK_COUNT), 1):
        take(f"ink-{number}", at)
    near_edge = flat & edge_mask(shots[0])
    for key, area in (("map", MAP_AREA), ("side", SIDEBAR_AREA)):
        found = [p for p in scan(near_edge, area, 2) if ok(*p) and far(*p, RECT_SPREAD)]
        for number, at in enumerate(thin(spread(found, EDGE_SPREAD), EDGE_PER_AREA), 1):
            take(f"edge-{key}-{number}", at)
    grid_area = (GRID_STRIDE // 2, GRID_STRIDE // 2, width, height)
    grid = [p for p in scan(flat, grid_area, GRID_STRIDE) if ok(*p) and far(*p, RECT_SPREAD)]
    for number, at in enumerate(thin(grid, GRID_COUNT), 1):
        take(f"grid-{number}", at)
    for key, (x, y) in EXACT_POINTS.get(label, {}).items():
        if all(np.array_equal(shots[0][y, x], shot[y, x]) for shot in shots):
            take(f"{key}-1", (x, y))
    return chosen


def choose_points(
    shot_dirs: list[Path], records: list[dict[str, Any]], labels: list[str]
) -> tuple[dict[str, Any], dict[str, Any]]:
    """The `--pixels`-style spec and the colors at its points, from the
    screenshots of the Chromium runs (`shot_dirs`, one per run) and the
    records. The spec is `{"groups", "group_points", "points"}` (a group
    claims the points whose names start with `NNN-`, its number), the colors
    `{label: {name: [r, g, b]}}`."""
    from PIL import Image

    points: dict[str, list[int]] = {}
    colors: dict[str, dict[str, list[int]]] = {}
    claims: dict[str, list[str]] = {}
    for number, label in enumerate(labels):
        shots = []
        for directory in shot_dirs:
            with Image.open(directory / screenshot_name(number, label)) as image:
                shots.append(np.asarray(image.convert("RGB")))
        chosen = choose_group(label, shots, records[number])
        prefix = f"{number:03d}-"
        claims[label] = [prefix]
        points.update({f"{prefix}{key}": list(at) for key, at in chosen.items()})
        colors[label] = {
            f"{prefix}{key}": [int(c) for c in shots[0][y, x]] for key, (x, y) in chosen.items()
        }
    return {"groups": labels, "group_points": claims, "points": points}, colors


# The record expression. `window.__o()` returns the JSON text of what each
# item needs: the URL and hash, the view from the hash, the loaded tile
# images in the viewport, the base layer and the overlays that are checked,
# the open panes and their titles and texts, the counts of rows, the
# outlined objects (Leaflet paths of the overlay pane) and the welcome box.
RECORD_JS = r"""
window.__o = () => {
  const visible = (e) => {
    const r = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const text = (e, max = 500) => (e ? e.innerText.replace(/\s+/g, ' ').trim().slice(0, max) : null);
  const count = (root, selector) => (root ? root.querySelectorAll(selector).length : 0);
  const titles = (root) => (root
    ? [...root.querySelectorAll('h1, h2, h3')].filter(visible).map((h) => text(h, 80))
    : []);
  // The rectangles of the visible elements that the items of M12 depend on:
  // per area a list of [selector, index among the visible matches, left, top,
  // width, height] in CSS px, rounded to 0.01 (at most 40 matches a selector).
  const areas = {
    header: ['header', 'header a', 'header .btn', 'header img'],
    map: [
      '#map', '.control-button', '.leaflet-control-scale', '.leaflet-control-scale > div',
      '.leaflet-control-attribution', '.leaflet-marker-pane > *', '.leaflet-popup',
    ],
    sidebar: [
      '#sidebar', '#sidebar form', '#sidebar input', '#sidebar select', '#sidebar label',
      '#sidebar button, #sidebar .btn', '#sidebar .btn-close', '#sidebar_content',
      '#sidebar_content h2, #sidebar_content h3, #sidebar_content h4, #sidebar_content h5',
      '#sidebar_content p', '#sidebar_content details', '#sidebar_content summary',
      '#sidebar_content table', '#sidebar_content tr', '#sidebar_content td',
      '#sidebar_content ul', '#sidebar_content li', '#sidebar_content nav',
      '#sidebar_content .pagination > *', '#sidebar_content img', '#sidebar_content dt',
      '#sidebar_content dd',
    ],
    panel: [
      '#map-ui', '#map-ui h2', '#map-ui h4', '#map-ui .btn-close', '.base-layers > div',
      '.base-layers label', '.base-layers .badge', '.layer-info-toggle', '.layer-info-toggle i',
      '.overlay-layers p', '.overlay-layers .form-check', '.overlay-layers label',
      '.overlay-layers input', '.legend-ui table', '.legend-ui tr', '.legend-ui img',
      '.share-link form', '.share-link .form-check', '.share-link label', '.share-link input',
      '#nav-tab', '#nav-tab a', '#nav-tab > span', '#nav-tabContent textarea',
      '#nav-tabContent p', '.share-geo-uri a', '#export-image .row', '#export-image label',
      '#export-image select', '#export-image .col-auto', '#export-image .form-check',
      '#export-image input', '#export-image p',
    ],
  };
  const round = (v) => Math.round(v * 100) / 100;
  const rects = Object.fromEntries(Object.entries(areas).map(([area, selectors]) => [
    area,
    selectors.flatMap((selector) => [...document.querySelectorAll(selector)]
      .filter(visible).slice(0, 40).map((e, i) => {
        const r = e.getBoundingClientRect();
        return [selector, i, round(r.left), round(r.top), round(r.width), round(r.height)];
      })),
  ]));
  const view = location.hash.match(/^#map=(\d+)\/(-?[\d.]+)\/(-?[\d.]+)/);
  const tiles = [...document.querySelectorAll('#map .leaflet-tile-pane img.leaflet-tile')];
  const inViewport = (e) => {
    const r = e.getBoundingClientRect();
    return r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight;
  };
  const shown = tiles.filter(inViewport);
  const loaded = shown.filter((i) => i.complete && i.naturalWidth > 0);
  const base = document.querySelector('.layers-ui input[name=layer]:checked');
  const overlays = ['notes', 'data', 'gps'].filter((n) => {
    const box = document.querySelector('#label-layers-' + n + ' input');
    return box && box.checked;
  });
  const mapUi = document.querySelector('#map-ui');
  const sidebar = document.querySelector('#sidebar_content');
  const paths = [...document.querySelectorAll('#map .leaflet-overlay-pane path')];
  const markers = [...document.querySelectorAll('#map .leaflet-marker-pane > *')];
  const welcome = document.querySelector('.welcome');
  return JSON.stringify({
    url: location.pathname + location.search,
    hash: location.hash,
    title: document.title,
    view: view ? {zoom: +view[1], lat: +view[2], lon: +view[3]} : null,
    tiles: {
      shown: shown.length,
      loaded: loaded.length,
      srcs: [...new Set(loaded.map((i) => i.src))].sort(),
    },
    base: base ? base.id.replace('map-ui-layer-', '') : null,
    overlays,
    mapUi: {open: !!mapUi && visible(mapUi), titles: titles(mapUi), text: text(mapUi)},
    sidebar: {titles: titles(sidebar), text: text(sidebar, 800)},
    counts: {
      li: count(sidebar, 'li'),
      tr: count(sidebar, 'tr'),
      listGroupItem: count(sidebar, '.list-group-item'),
      links: count(sidebar, 'a'),
      searchResults: count(sidebar, 'li.search_results_entry'),
      routeSteps: count(sidebar, 'tr.turn'),
      queryNearby: count(sidebar, '#query-nearby .query-results-list > li'),
      queryEnclosing: count(sidebar, '#query-isin .query-results-list > li'),
    },
    outlines: {
      paths: paths.length,
      d: paths.map((p) => p.getAttribute('d') || ''),
      strokes: [...new Set(paths.map((p) => p.getAttribute('stroke')))].sort(),
    },
    markers: markers.length,
    share: {
      tab: text(document.querySelector('#nav-tab .active'), 40),
      longInput: (document.querySelector('#long_input') || {}).value || null,
      shortInput: (document.querySelector('#short_input') || {}).value || null,
      embedHtml: (document.querySelector('#embed_html') || {}).value || null,
      marker: !!(document.querySelector('#link_marker') || {}).checked,
    },
    welcome: !!welcome && visible(welcome),
    scale: text(document.querySelector('.leaflet-control-scale')),
    attribution: text(document.querySelector('.leaflet-control-attribution'), 200),
    elements: document.getElementsByTagName('*').length,
    rects,
  });
};
"""


def step_groups(sequence: str = "full") -> list[dict[str, Any]]:
    """The groups of a sequence (`all_step_groups` without the groups that the
    sequence leaves out)."""
    skipped = SEQUENCES[sequence][1]
    return [g for g in all_step_groups() if g["name"] not in skipped]


def write_steps(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    for sequence in SEQUENCES:
        write_json(data_file(out, sequence, "steps"), step_groups(sequence))
    (out / "osm-zagreb.js").write_text(RECORD_JS.lstrip(), encoding="utf-8")
    print(f"{len(all_step_groups())} groups written to {out} ({len(SEQUENCES)} sequences)")


def replay_once(out: Path, work: Path, sequence: str, run: int, extra: list[str]) -> list[Any]:
    """One run of `swbtools replay osm-zagreb`; returns its records."""
    records = work / f"run{run}.json"
    cmd = ["uv", "run", "--project", "tools", "swbtools", "replay", FIXTURE, *CHROMIUM_ARGS]
    cmd += ["--steps", str(data_file(out, sequence, "steps"))]
    cmd += ["--setup", str(out / "osm-zagreb.js"), "--record", "window.__o()"]
    cmd += ["--out", str(records)]
    run_replay(cmd + extra, work / f"run{run}.log")
    return load_json(records)


def write_records(out: Path, work: Path, sequence: str) -> None:
    """Three runs of Chromium: the records, the missing URLs, the console and
    the screenshots. Exits with status 1 if the runs differ in the records or
    the missing URLs."""
    write_steps(out)
    work = work_dir(work, sequence)
    work.mkdir(parents=True, exist_ok=True)
    runs, missing, console = [], [], []
    for run in range(RUNS):
        extra = ["--missing-out", str(work / f"run{run}-missing.json")]
        extra += ["--console-out", str(work / f"run{run}-console.txt")]
        extra += ["--screenshots", str(work / f"shots{run}")]
        runs.append(replay_once(out, work, sequence, run, extra))
        found = load_json(work / f"run{run}-missing.json")
        missing.append(mask_missing(found))
        console.append(page_console((work / f"run{run}-console.txt").read_text(encoding="utf-8")))
    diff = [d for other in runs[1:] for d in differences(runs[0], other)]
    write_json(data_file(out, sequence, "chromium"), runs[0])
    write_json(data_file(out, sequence, "missing"), missing[0])
    write_json(data_file(out, sequence, "console"), console_counts(console))
    print(f"{len(runs[0])} records; the runs differ in {len(diff)} fields")
    for line in dict.fromkeys(diff):
        print("  " + line)
    same_missing = all(v == missing[0] for v in missing)
    print("missing URLs: " + ("equal in all runs" if same_missing else "DIFFER between runs"))
    ranges = [m for m in console_counts(console) if m["min"] != m["max"]]
    print(f"console: {len(ranges)} messages with a count that differs between the runs")
    if diff or not same_missing:
        sys.exit(1)


def write_points(out: Path, work: Path, sequence: str) -> None:
    """Writes the pixel points and the colors at them (`osm-zagreb-pixel-points.json`
    and `osm-zagreb-pixels.json`) from the three runs of `records`."""
    work = work_dir(work, sequence)
    records = load_json(data_file(out, sequence, "chromium"))
    labels = ["load", *(g["name"] for g in step_groups(sequence))]
    shot_dirs = [work / f"shots{run}" for run in range(RUNS)]
    spec, colors = choose_points(shot_dirs, records, labels)
    write_json(data_file(out, sequence, "pixel-points"), spec)
    write_json(data_file(out, sequence, "pixels"), colors)
    print(f"{len(spec['points'])} points in {len(labels)} groups written to {out}")


def short(value: Any) -> str:
    return "" if value is None else str(value)


def summary(out: Path, sequence: str) -> None:
    records = load_json(data_file(out, sequence, "chromium"))
    missing = load_json(data_file(out, sequence, "missing"))
    names = ["load", *(g["name"] for g in step_groups(sequence))]
    print(
        "| # | group | hash | tiles | base+overlays | right pane | left sidebar | rows | paths"
        " | missing |"
    )
    print("|---|---|---|---|---|---|---|---|---|---|")
    for number, (name, rec, miss) in enumerate(zip(names, records, missing, strict=False)):
        left = " / ".join(rec["sidebar"]["titles"]) or short(rec["sidebar"]["text"])[:40]
        right = " / ".join(rec["mapUi"]["titles"]) if rec["mapUi"]["open"] else ""
        rows = rec["counts"]
        row = f"{rows['searchResults']}s {rows['routeSteps']}r {rows['li']}li"
        absent = [u for u in miss["missing"] if u != MATOMO]
        print(
            f"| {number} | {name} | {rec['hash'].removeprefix('#map=')} "
            f"| {rec['tiles']['loaded']}/{rec['tiles']['shown']} "
            f"| {rec['base']}+{','.join(rec['overlays'])} | {right} | {left} | {row} "
            f"| {rec['outlines']['paths']} | {len(absent)} |"
        )


def mask_missing(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The rows of `--missing-out` with each Matomo URL cut to its path."""
    return exit_records.mask_missing(rows, (MATOMO,))


def page_console(text: str) -> list[dict[str, str]]:
    """The page messages of a `--console-out` file; a Matomo URL is cut to
    its path."""
    return exit_records.page_console(text, (MATOMO,))


# One `swb` run: the pause after each step is the pause of `replay`, in page
# time.


def swb_steps(
    groups: list[dict[str, Any]], shots: Path, skip: frozenset[int] = frozenset()
) -> list[dict[str, Any]]:
    """The flat step list of one swb run: the record function, then a
    `record` step and a `screenshot` step after the load and after each
    group; a pause after each step, as `replay` has. The groups of `skip`
    (numbers from 1) get their records but run no steps: a way to look past
    a group that breaks the page."""
    record = {"record": "window.__o()"}

    def shot(number: int, name: str) -> dict[str, Any]:
        return {"screenshot": str(shots / screenshot_name(number, name))}

    steps: list[dict[str, Any]] = [{"eval": RECORD_JS}, record, shot(0, "load")]
    for number, one in enumerate(groups, 1):
        for step in [] if number in skip else one["steps"]:
            steps += [step, {"wait": DEFAULT_PAUSE_MS}]
        steps += [record, shot(number, one["name"])]
    return steps


def run_swb_groups(
    sequence: str,
    fixture: str,
    groups_limit: int | None,
    out: Path,
    chromium: Path,
    binary: Path,
    skip: frozenset[int] = frozenset(),
) -> None:
    """The `swb` command: one run of the groups, the comparison, the files."""
    from swbtools import paths

    groups = step_groups(sequence)[:groups_limit]
    out.mkdir(parents=True, exist_ok=True)
    shots = out / "swb-shots"
    shots.mkdir(exist_ok=True)
    steps = swb_steps(groups, shots, skip)
    command = swb_replay_command(binary, paths.js_fixture_dir(fixture), steps, animations=False)
    names = ["load", *(g["name"] for g in groups)]
    compare_swb_run(command, out, load_json(chromium), names, with_times=True)


def pixel_diff(out: Path, work: Path, sequence: str) -> None:
    """The `pixel-diff` command: the points where swb's screenshots (`swb`
    command) differ from Chromium's colors by more than the tolerance.
    Writes `pixel-diff.json` into the work directory of the sequence."""
    work = work_dir(work, sequence)
    exit_records.pixel_diff(
        data_file(out, sequence, "pixel-points"),
        data_file(out, sequence, "pixels"),
        work / "swb-shots",
        work / "pixel-diff.json",
        number_offset=0,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = ["steps", "records", "points", "summary", "swb", "pixel-diff"]
    parser.add_argument("command", choices=commands)
    parser.add_argument(
        "--sequence",
        choices=list(SEQUENCES),
        default="full",
        help="the sequence of step groups (default full)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=DATA,
        help="the data directory of the test (default crates/automation/tests/data)",
    )
    parser.add_argument(
        "--work",
        type=Path,
        default=WORK,
        help="records, points, swb, pixel-diff: runs, logs, screenshots (default out/osm-zagreb)",
    )
    parser.add_argument("--fixture", default=FIXTURE, help="swb: the fixture to run")
    parser.add_argument("--groups", type=int, help="swb: run only the first N groups")
    parser.add_argument("--skip", type=int, nargs="*", help="swb: group numbers to leave out")
    parser.add_argument("--chromium", type=Path, help="swb: Chromium's records")
    parser.add_argument("--binary", type=Path, help="swb: the swb binary")
    args = parser.parse_args()
    sequence, out = args.sequence, args.out
    if args.command == "swb":
        from swbtools import swb as swb_tools

        binary = swb_tools.find_swb(args.binary)
        if binary is None:
            sys.exit("no swb binary (build it, or pass --binary)")
        chromium = args.chromium or data_file(out, sequence, "chromium")
        skip = frozenset(args.skip or [])
        target = work_dir(args.work, sequence)
        run_swb_groups(sequence, args.fixture, args.groups, target, chromium, binary, skip)
    elif args.command == "records":
        write_records(out, args.work, sequence)
    elif args.command == "points":
        write_points(out, args.work, sequence)
    elif args.command == "pixel-diff":
        pixel_diff(out, args.work, sequence)
    elif args.command == "summary":
        summary(out, sequence)
    else:
        write_steps(out)


if __name__ == "__main__":
    main()
