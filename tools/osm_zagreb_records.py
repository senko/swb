"""The step groups, the record expression and the Chromium records for the
OpenStreetMap target of M12 (`fixtures/js/osm-zagreb`; docs/testing.md,
"Capture with step groups"). Run from the repository root:

    uv run --project tools python tools/osm_zagreb_records.py steps [--out DIR]
    uv run --project tools python tools/osm_zagreb_records.py records [--out DIR]
    uv run --project tools python tools/osm_zagreb_records.py summary [--out DIR]
    uv run --project tools python tools/osm_zagreb_records.py merge-node --from FIXTURE_DIR
    uv run --project tools python tools/osm_zagreb_records.py swb [--fixture NAME] \\
        [--groups N] [--skip N...] [--out DIR]

`steps` writes `osm-zagreb-steps.json` (the step groups) and
`osm-zagreb-record.js` (the setup script that defines `window.__o()`, the
record expression) into DIR (default `out/osm-zagreb`). The live capture of
the fixture, with the groups run against the real site (one run; the tile
usage policy of tile.openstreetmap.org forbids bulk downloads):

    uv run --project tools swbtools capture --js --force --max-responses 1000 \\
        --steps DIR/osm-zagreb-steps.json \\
        'https://www.openstreetmap.org/?mlat=45.813114&mlon=15.976390#map=16/45.81308/15.97721' \\
        osm-zagreb

`merge-node` adds the two responses of one node page from an earlier
recording (the last live run could not open a node, see `merge_node`).
`records` runs `swbtools replay osm-zagreb` twice with those files (no
network), checks that the two runs agree, and writes `osm-zagreb-chromium.json`
(the record after the load and after each group), `osm-zagreb-missing.json`
(the URLs that the fixture lacks, per group), `osm-zagreb-console.txt` and
the screenshots (`shots/`) into DIR. `summary` prints a table of the records.

`swb` runs the step groups in swb on a fixture (default `osm-zagreb`):
one `swb --headless` run with the steps of all groups, a `record` step and a
`screenshot` step after the load and after each group. It compares each record with Chromium's
(`osm-zagreb-chromium.json` in DIR) and writes `swb-records.json` (the
records), `swb-diff.json` (the differing fields per record), `swb-times.json`
(wall time per record), `swb-log.txt` (stderr), `swb-groups.json` (the log
lines of each group) and `swb-shots/` into DIR.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = "osm-zagreb"
URL = "https://www.openstreetmap.org/?mlat=45.813114&mlon=15.976390#map=16/45.81308/15.97721"
RUNS = 2

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


def group(name: str, *steps: dict[str, Any]) -> dict[str, Any]:
    return {"name": name, "steps": list(steps)}


def wait(ms: int) -> dict[str, Any]:
    return {"wait": ms}


def reset_view(item: str, hash_: str = "#map=16/45.81308/15.97721") -> dict[str, Any]:
    """Sets the view by the URL hash, as a user does in the address bar."""
    return group(f"reset view {item}", {"eval": f"location.hash = '{hash_}'"}, wait(2500))


def layer(name: str, code: str) -> dict[str, Any]:
    return group(f"layer {name}", {"click": f"label[for='map-ui-layer-{code}']"}, wait(3500))


def overlay(name: str, label: str, wait_ms: int = 3500) -> dict[str, Any]:
    return group(f"overlay {name}", {"click": f"#label-layers-{label}"}, wait(wait_ms))


def step_groups() -> list[dict[str, Any]]:
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
        # these steps had no such group (see `merge_node`).
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
      pathLength: paths.reduce((n, p) => n + (p.getAttribute('d') || '').length, 0),
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
  });
};
"""


NODE_URLS = (
    "https://www.openstreetmap.org/node/1013329474",
    "https://www.openstreetmap.org/api/0.6/node/1013329474",
)


def merge_node(source: Path) -> None:
    """Copies the two responses of the page of node 1013329474 (the first
    node of the query list) from an earlier recording of the same steps
    (`source`, a fixture directory) into `fixtures/js/osm-zagreb`. The third
    live run, which had no repeated query because the service answers a
    repeat with 429, did not open a node. The responses are `GET`s without a
    token, recorded by the same tool in the first live run."""
    from swbtools import paths
    from swbtools.manifest import Fixture

    earlier = Fixture(source)
    target = Fixture(paths.js_fixture_dir(FIXTURE))
    for url in NODE_URLS:
        entry = earlier.get(url)
        if entry is None:
            sys.exit(f"{url} is not in {source}")
        target.record(entry.method, url, entry.status, entry.headers, earlier.read_body(entry))
    target.save()
    print(f"{len(NODE_URLS)} entries merged from {source}")


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


def write_steps(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    write_json(out / "osm-zagreb-steps.json", step_groups())
    (out / "osm-zagreb-record.js").write_text(RECORD_JS.lstrip(), encoding="utf-8")
    print(f"{len(step_groups())} groups written to {out}")


def replay_once(out: Path, run: int, extra: list[str]) -> list[Any]:
    """One run of `swbtools replay osm-zagreb`; returns its records."""
    records = out / f"run{run}.json"
    cmd = ["uv", "run", "--project", "tools", "swbtools", "replay", FIXTURE]
    cmd += ["--steps", str(out / "osm-zagreb-steps.json")]
    cmd += ["--setup", str(out / "osm-zagreb-record.js"), "--record", "window.__o()"]
    cmd += ["--out", str(records)]
    result = subprocess.run(cmd + extra, cwd=ROOT, capture_output=True, text=True, check=False)
    (out / f"run{run}.log").write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        sys.exit(f"replay failed (see {out / f'run{run}.log'}):\n{result.stderr[-2000:]}")
    return json.loads(records.read_text(encoding="utf-8"))


def differences(first: list[Any], second: list[Any]) -> list[str]:
    """The (record, field) pairs in which two runs differ."""
    found = []
    for number, (a, b) in enumerate(zip(first, second, strict=False)):
        found += [f"record {number}: {key}" for key in sorted(a) if a[key] != b.get(key)]
    return found


def write_records(out: Path) -> None:
    write_steps(out)
    runs = []
    for run in range(RUNS):
        extra = ["--missing-out", str(out / f"run{run}-missing.json")]
        if run == 0:
            extra += ["--screenshots", str(out / "shots")]
            extra += ["--console-out", str(out / "osm-zagreb-console.txt")]
        runs.append(replay_once(out, run, extra))
    diff = differences(runs[0], runs[1])
    write_json(out / "osm-zagreb-chromium.json", runs[0])
    (out / "run0-missing.json").replace(out / "osm-zagreb-missing.json")
    print(f"{len(runs[0])} records; the two runs differ in {len(diff)} fields")
    for line in diff:
        print("  " + line)


def short(value: Any) -> str:
    return "" if value is None else str(value)


def summary(out: Path) -> None:
    records = json.loads((out / "osm-zagreb-chromium.json").read_text(encoding="utf-8"))
    steps = json.loads((out / "osm-zagreb-steps.json").read_text(encoding="utf-8"))
    missing = json.loads((out / "osm-zagreb-missing.json").read_text(encoding="utf-8"))
    names = ["load", *(g["name"] for g in steps)]
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
        absent = [u for u in miss["missing"] if "matomo" not in u]
        print(
            f"| {number} | {name} | {rec['hash'].removeprefix('#map=')} "
            f"| {rec['tiles']['loaded']}/{rec['tiles']['shown']} "
            f"| {rec['base']}+{','.join(rec['overlays'])} | {right} | {left} | {row} "
            f"| {rec['outlines']['paths']} | {len(absent)} |"
        )


# One `swb` run: the pause after each step (the pause of `replay`, in page
# time), the cap of the whole run, and the log levels (the `screenshot` step
# logs at `info` so that the log has a mark per group).
PAUSE_MS = 300
SWB_TIMEOUT_S = 1800
SWB_LOG_FILTER = "warn,swb_automation::steps=info"
SCREENSHOT_LINE = re.compile(r"screenshot \S*?(\d{3})-[^/\s]*\.png")


PICK = "data-swb-pick"
PLAYWRIGHT_SELECTOR = re.compile(r"(?P<css>.*?)(?P<visible>:visible)?(?: >> nth=(?P<nth>\d+))?")
POINTER_STEPS = ("click", "dblclick", "rightclick", "middleclick", "hover", "focus")


def css_steps(step: dict[str, Any]) -> list[dict[str, Any]]:
    """The steps that do what one step does in swb. swb's selectors are CSS;
    the step file uses Playwright's `:visible` and `>> nth=N`. Such a step
    becomes: an `eval` that marks the match with an attribute, the step on
    that attribute (the click still goes through the pointer and the hit
    test), and an `eval` that removes the mark."""
    for kind in POINTER_STEPS:
        selector = step.get(kind)
        if not isinstance(selector, str):
            continue
        parts = PLAYWRIGHT_SELECTOR.fullmatch(selector)
        if parts is None or not (parts["visible"] or parts["nth"]):
            return [step]
        pick = (
            f"(() => {{ let found = [...document.querySelectorAll({json.dumps(parts['css'])})];"
            + (
                " found = found.filter((e) => { const r = e.getBoundingClientRect();"
                " return r.width > 0 && r.height > 0"
                " && getComputedStyle(e).visibility !== 'hidden'; });"
                if parts["visible"]
                else ""
            )
            + f" found[{int(parts['nth'] or 0)}].setAttribute('{PICK}', ''); }})()"
        )
        unpick = f"document.querySelector('[{PICK}]').removeAttribute('{PICK}')"
        return [{"eval": pick}, {kind: f"[{PICK}]"}, {"eval": unpick}]
    return [step]


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
        safe = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
        return {"screenshot": str(shots / f"{number:03d}-{safe}.png")}

    steps: list[dict[str, Any]] = [{"eval": RECORD_JS}, record, shot(0, "load")]
    for number, one in enumerate(groups, 1):
        for step in [] if number in skip else one["steps"]:
            for plain in css_steps(step):
                steps += [plain, {"wait": PAUSE_MS}]
        steps += [record, shot(number, one["name"])]
    return steps


def parse_answers(lines: list[tuple[float, str]]) -> tuple[list[Any], list[float]]:
    """The records and the arrival times of the answer lines of `record`
    steps (`{"value": JSON TEXT}` or `{"error": MESSAGE}`)."""
    records, times = [], []
    for when, line in lines:
        try:
            answer = json.loads(line)
        except ValueError:
            continue
        if "value" in answer and isinstance(answer["value"], str):
            try:
                records.append(json.loads(answer["value"]))
            except ValueError:
                records.append({"error": "not JSON: " + answer["value"][:200]})
        elif "error" in answer:
            records.append({"error": answer["error"]})
        else:
            continue
        times.append(when)
    return records, times


def split_log(text: str) -> list[list[str]]:
    """The lines of the log, one list per record: the lines before the
    `screenshot` line of record N belong to record N."""
    groups: list[list[str]] = [[]]
    for line in text.splitlines():
        if SCREENSHOT_LINE.search(line):
            groups.append([])
        else:
            groups[-1].append(line)
    return groups


def flat_fields(value: Any, prefix: str = "", depth: int = 0) -> dict[str, Any]:
    """The fields of a record by dotted path, objects opened to two levels."""
    if isinstance(value, dict) and depth < 2:
        found: dict[str, Any] = {}
        for key, item in value.items():
            found.update(flat_fields(item, f"{prefix}{key}.", depth + 1))
        return found
    return {prefix.rstrip("."): value}


def field_differences(expected: Any, actual: Any) -> list[str]:
    """The paths of the fields in which two records differ."""
    a, b = flat_fields(expected), flat_fields(actual)
    return sorted(key for key in a.keys() | b.keys() if a.get(key) != b.get(key))


def swb_command(binary: Path, fixture: Path, steps: list[dict[str, Any]]) -> list[str]:
    """The command line of one run (the options of `compare --js`)."""
    from swbtools.compare import swb_js_options
    from swbtools.pages import read_meta

    meta = read_meta(fixture)
    width, height = meta.viewport
    return [
        str(binary),
        "--headless",
        "--replay",
        str(fixture),
        "--test-fonts",
        "--size",
        f"{width}x{height}",
        *swb_js_options(True, meta.captured),
        "--steps",
        json.dumps(steps),
        meta.url,
    ]


def run_swb_steps(
    command: list[str], log_path: Path, timeout: float = SWB_TIMEOUT_S
) -> tuple[list[tuple[float, str]], int | None]:
    """Runs swb, writes its stderr to `log_path`, and returns the lines of
    its standard output with their times (seconds from the start) and the
    exit status (None if the run hit the timeout and was stopped)."""
    env = {**os.environ, "RUST_LOG": SWB_LOG_FILTER}
    start = time.monotonic()
    lines: list[tuple[float, str]] = []
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=log, text=True
        )
        killer = threading.Timer(timeout, process.kill)
        killer.start()
        try:
            if process.stdout is not None:
                for line in process.stdout:
                    lines.append((time.monotonic() - start, line.rstrip("\n")))
            status = process.wait()
        finally:
            killer.cancel()
            process.kill()
            process.wait()
    return lines, (None if time.monotonic() - start >= timeout else status)


def run_swb_groups(
    fixture: str,
    groups_limit: int | None,
    out: Path,
    chromium: Path,
    binary: Path,
    skip: frozenset[int] = frozenset(),
) -> None:
    """The `swb` command: one run of the groups, the comparison, the files."""
    from swbtools import paths

    groups = step_groups()[:groups_limit]
    out.mkdir(parents=True, exist_ok=True)
    shots = out / "swb-shots"
    shots.mkdir(exist_ok=True)
    steps = swb_steps(groups, shots, skip)
    command = swb_command(binary, paths.js_fixture_dir(fixture), steps)
    lines, status = run_swb_steps(command, out / "swb-log.txt")
    records, arrival = parse_answers(lines)
    names = ["load", *(g["name"] for g in groups)]
    expected = json.loads(chromium.read_text(encoding="utf-8"))
    diffs = [field_differences(e, a) for e, a in zip(expected, records, strict=False)]
    seconds = [round(b - a, 3) for a, b in zip([0.0, *arrival], arrival, strict=False)]
    log_groups = split_log((out / "swb-log.txt").read_text(encoding="utf-8"))
    write_json(out / "swb-records.json", records)
    write_json(out / "swb-diff.json", labelled(names, "fields", diffs))
    write_json(out / "swb-times.json", labelled(names, "seconds", seconds))
    write_json(out / "swb-groups.json", labelled(names, "log", log_groups))
    print(f"swb status {status}; {len(records)} of {len(names)} records")
    for name, fields, took in zip(names, diffs, seconds, strict=False):
        print(f"{took:7.2f} s  {name}: {', '.join(fields) or 'equal'}")


def labelled(names: list[str], key: str, values: list[Any]) -> list[dict[str, Any]]:
    """`[{"name": N, KEY: V}, ...]` for the pairs of names and values."""
    return [{"name": n, key: v} for n, v in zip(names, values, strict=False)]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = ["steps", "records", "summary", "merge-node", "swb"]
    parser.add_argument("command", choices=commands)
    parser.add_argument("--out", type=Path, default=ROOT / "out" / "osm-zagreb")
    parser.add_argument("--fixture", default=FIXTURE, help="swb: the fixture to run")
    parser.add_argument("--groups", type=int, help="swb: run only the first N groups")
    parser.add_argument("--skip", type=int, nargs="*", help="swb: group numbers to leave out")
    parser.add_argument("--chromium", type=Path, help="swb: Chromium's records")
    parser.add_argument("--binary", type=Path, help="swb: the swb binary")
    parser.add_argument("--from", dest="source", type=Path, help="merge-node: a fixture directory")
    args = parser.parse_args()
    if args.command == "swb":
        from swbtools import swb as swb_tools

        binary = swb_tools.find_swb(args.binary)
        if binary is None:
            sys.exit("no swb binary (build it, or pass --binary)")
        chromium = args.chromium or args.out / "osm-zagreb-chromium.json"
        skip = frozenset(args.skip or [])
        run_swb_groups(args.fixture, args.groups, args.out, chromium, binary, skip)
        return
    if args.command == "merge-node":
        if args.source is None:
            sys.exit("merge-node needs --from DIR")
        merge_node(args.source)
        return
    {"steps": write_steps, "records": write_records, "summary": summary}[args.command](args.out)


if __name__ == "__main__":
    main()
