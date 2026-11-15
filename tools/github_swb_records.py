"""Makes the Chromium data of `crates/automation/tests/github_swb.rs`
(M10 feature 9c): the steps files, and the records of Chromium's replay of
the fixture `github-swb` after those steps. Run from the repository root:

    uv run --project tools python tools/github_swb_records.py steps
    uv run --project tools python tools/github_swb_records.py interactions
    uv run --project tools python tools/github_swb_records.py tab
    uv run --project tools python tools/github_swb_records.py nav [CASE...]

`steps` writes the steps files. The other commands run `swbtools replay`
three times and write the data file only if the three runs are equal in every
field that the test compares. They need Chromium (docs/testing.md).

- `interactions`: the header menus, the Code popover, the branch selector,
  "Go to file" and the "..." button of the commit line, one load and 26
  groups: `data/github-swb-interactions-chromium.json` (the record
  `window.__i()` after the load and each group), the colors at the pixel
  points (`data/github-swb-interactions-pixels.json`), the URLs that the
  fixture lacks (`data/github-swb-interactions-missing.json`) and the errors
  of the page (`data/github-swb-interactions-errors.json`).
- `tab`: 70 Tab and 5 Shift+Tab presses, `data/github-swb-tab-chromium.json`
  (`window.__t()` after the load and each press).
- `nav`: 13 loads, each with one click: `data/github-swb-nav-chromium.json`,
  a map of the case name to the records `{"url": ...}` after the load and
  after the click (`chrome-error://chromewebdata/` when the click leaves the
  page for a URL that the fixture lacks) and the sorted URLs that the
  fixture lacks. With case names it records only those cases.
"""

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "crates/automation/tests/data"
RUNS = 3

# The groups of the interactions: the header menus (five open, one switch,
# Escape), the Code popover (open, GitHub CLI tab, HTTPS tab, Escape), the
# branch selector (open, type, clear, Tags tab, Escape), "Go to file" (click,
# type, Escape, a click on a blank area and the key `t`) and the "..."
# button of the commit line.
INTERACTIONS = [
    ("platform open", [{"click": [129, 36]}, {"wait": 600}]),
    ("platform escape", [{"key": "Escape"}, {"wait": 400}]),
    ("solutions open", [{"click": [230, 36]}, {"wait": 600}]),
    ("resources switch", [{"click": [338, 36]}, {"wait": 600}]),
    ("resources escape", [{"key": "Escape"}, {"wait": 400}]),
    ("open source open", [{"click": [462, 36]}, {"wait": 600}]),
    ("open source escape", [{"key": "Escape"}, {"wait": 400}]),
    ("enterprise open", [{"click": [584, 36]}, {"wait": 600}]),
    ("enterprise escape", [{"key": "Escape"}, {"wait": 400}]),
    ("code open", [{"click": [882, 281]}, {"wait": 1000}]),
    ("code cli tab", [{"click": [674, 366]}, {"wait": 500}]),
    ("code https tab", [{"click": [588, 366]}, {"wait": 500}]),
    ("code escape", [{"key": "Escape"}, {"wait": 500}]),
    ("branch open", [{"click": "#ref-picker-repos-header-ref-selector"}, {"wait": 1200}]),
    ("branch type", [{"type": "xyz"}, {"wait": 600}]),
    (
        "branch clear",
        [{"key": "Backspace"}, {"key": "Backspace"}, {"key": "Backspace"}, {"wait": 600}],
    ),
    ("branch tags tab", [{"click": [150, 414]}, {"wait": 800}]),
    ("branch escape", [{"key": "Escape"}, {"wait": 500}]),
    ("go to file click", [{"click": "input[aria-label='Go to file']"}, {"wait": 800}]),
    ("go to file type", [{"type": "cr"}, {"wait": 1500}]),
    ("go to file escape", [{"key": "Escape"}, {"wait": 500}]),
    ("blank click", [{"click": [700, 215]}, {"wait": 300}]),
    ("key t", [{"key": "t"}, {"wait": 800}]),
    ("t escape", [{"key": "Escape"}, {"wait": 500}]),
    ("commit dots", [{"click": "[data-testid=latest-commit-details-toggle]"}, {"wait": 800}]),
    ("commit dots escape", [{"key": "Escape"}, {"wait": 500}]),
]

# The Tab order: the search button (stop 9) starts lazy chunks that the
# fixture lacks; the loader retries them and the header then shows an error
# heading (+14 px), at a stop that differs between runs. The wait lets the
# retries run before the next stop. `github-swb-tab.js` reports rectangles
# without that heading.
TAB_STOPS = 70
SHIFT_TAB_STOPS = 5

# The 13 loads with one click each: the click target and the wait after it.
NAVIGATION = {
    "notifications": "[data-testid=notifications-sign-in-button]",
    "fork": "[data-testid=fork-button]",
    "star": "[data-testid=star-button]",
    "tab-issues": "nav[aria-label=Repository] a[href='/senko/swb/issues']",
    "tab-pulls": "nav[aria-label=Repository] a[href='/senko/swb/pulls']",
    "file-dir": [94, 469],
    "file-file": [115, 748],
    "commit-msg": "a[href^='/senko/swb/commit/cdbcd8']",
    "sign-in": [1090, 36],
    "sign-up": "a[href^='/signup']",
    "branches-link": "a[href='/senko/swb/branches']",
    "commits-link": "a[href='/senko/swb/commits/main/']",
    "readme-anchor": [1026, 395],
}


def groups(items: list[tuple[str, list]]) -> list[dict]:
    """The steps file format: a list of named groups."""
    return [{"name": name, "steps": steps} for name, steps in items]


def tab_groups() -> list[dict]:
    items = []
    for i in range(1, TAB_STOPS + 1):
        wait = 14000 if i == 9 else 300
        items.append((f"tab {i:02d}", [{"key": "Tab"}, {"wait": wait}]))
    for i in range(1, SHIFT_TAB_STOPS + 1):
        items.append((f"shift-tab {i:02d}", [{"key": "Shift+Tab"}, {"wait": 300}]))
    return groups(items)


def nav_groups() -> list[dict]:
    return groups([(n, [{"click": c}, {"wait": 1500}]) for n, c in NAVIGATION.items()])


def write(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=1) + "\n", encoding="utf-8")


def write_steps() -> None:
    DATA.mkdir(parents=True, exist_ok=True)
    write(DATA / "github-swb-interactions-steps.json", groups(INTERACTIONS))
    write(DATA / "github-swb-tab-steps.json", tab_groups())
    write(DATA / "github-swb-nav-steps.json", nav_groups())


def replay(
    steps: Path, setup: Path | None, record: str, out: Path, extra: list[str]
) -> tuple[list[str], list[str]]:
    """One run of `swbtools replay github-swb`; returns the URLs that the
    fixture lacks (sorted) and the errors of the page (the exceptions and the
    `console.error` calls, in order)."""
    cmd = ["uv", "run", "--project", "tools", "swbtools", "replay", "github-swb"]
    cmd += ["--steps", str(steps), "--record", record, "--out", str(out)]
    if setup is not None:
        cmd += ["--setup", str(setup)]
    result = subprocess.run(cmd + extra, cwd=ROOT, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        sys.exit(f"replay failed:\n{result.stderr[-2000:]}")
    missing = sorted(set(re.findall(r"github-swb: not in fixture: (\S+)", result.stderr)))
    errors = re.findall(r"github-swb: ((?:exception|console\.error): .*)", result.stderr)
    return missing, errors


def equal_runs(name: str, runs: list) -> object:
    for i, run in enumerate(runs[1:], 2):
        if run != runs[0]:
            for k, (a, b) in enumerate(zip(runs[0], run, strict=False)):
                if a != b:
                    sys.exit(f"{name}: run 1 and run {i} differ at record {k}:\n{a}\n{b}")
            sys.exit(f"{name}: run 1 and run {i} differ")
    return runs[0]


def load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def record_interactions(tmp: Path) -> None:
    steps = DATA / "github-swb-interactions-steps.json"
    setup = DATA / "github-swb-interactions.js"
    points = DATA / "github-swb-interactions-pixel-points.json"
    runs, missing, errors, colors = [], [], [], []
    for run in range(RUNS):
        out, pix = tmp / f"run{run}.json", tmp / f"pixels{run}.json"
        extra = ["--pixels", str(points), "--pixels-out", str(pix)]
        urls, problems = replay(steps, setup, "window.__i()", out, extra)
        runs.append(load(out))
        colors.append(load(pix))
        missing.append(urls)
        errors.append(problems)
    write(DATA / "github-swb-interactions-chromium.json", equal_runs("records", runs))
    write(DATA / "github-swb-interactions-pixels.json", equal_runs("pixels", colors))
    write(DATA / "github-swb-interactions-missing.json", equal_runs("missing URLs", missing))
    write(DATA / "github-swb-interactions-errors.json", equal_runs("errors", errors))


def record_tab(tmp: Path) -> None:
    steps = DATA / "github-swb-tab-steps.json"
    setup = DATA / "github-swb-tab.js"
    runs = []
    for run in range(RUNS):
        out = tmp / f"run{run}.json"
        replay(steps, setup, "window.__t()", out, [])
        runs.append(load(out))
    write(DATA / "github-swb-tab-chromium.json", equal_runs("records", runs))


def record_nav(tmp: Path, only: list[str]) -> None:
    """The cases in `only` (all without names); the others stay in the file."""
    path = DATA / "github-swb-nav-chromium.json"
    result = load(path) if only and path.is_file() else {}
    expr = "JSON.stringify({url: location.href})"
    for case in nav_groups():
        if only and case["name"] not in only:
            continue
        print(case["name"], flush=True)
        steps = tmp / "case.json"
        steps.write_text(json.dumps([case]), encoding="utf-8")
        runs, missing = [], []
        for run in range(RUNS):
            out = tmp / f"run{run}.json"
            missing.append(replay(steps, None, expr, out, [])[0])
            runs.append(load(out))
        result[case["name"]] = {
            "records": equal_runs(case["name"], runs),
            "missing": equal_runs(case["name"], missing),
        }
        write(path, result)


def main() -> None:
    commands = {
        "steps": write_steps,
        "interactions": record_interactions,
        "tab": record_tab,
    }
    if len(sys.argv) < 2 or sys.argv[1] not in [*commands, "nav"]:
        sys.exit(__doc__)
    if sys.argv[1] == "steps":
        write_steps()
        return
    with tempfile.TemporaryDirectory(dir=ROOT / "tmp") as tmp:
        if sys.argv[1] == "nav":
            record_nav(Path(tmp), sys.argv[2:])
        else:
            commands[sys.argv[1]](Path(tmp))


if __name__ == "__main__":
    main()
