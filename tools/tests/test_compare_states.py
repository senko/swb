"""`compare --full-page` and `compare --click`: regions, full-page score,
state file names, report and the state capture. No network."""

import asyncio
import json

import numpy as np
import pytest
from PIL import Image

from swbtools import browser as chromium_browser
from swbtools import cli, swb
from swbtools.automation import Browser
from swbtools.boxes import BoxDump, Element, write_dump
from swbtools.compare import Options, compare_fixture, named, state_suffix, summary_line
from swbtools.fullpage import compare_full_page
from swbtools.live import click_and_settle
from swbtools.pixels import BAND, union_diff_image, union_difference
from swbtools.regions import Region, covering_boxes, find_regions
from swbtools.report import write_report
from swbtools.scoring import Scores, compare
from swbtools.swb_state import StateError, capture_state, click_selectors

WHITE = (255, 255, 255)


def test_state_suffix_and_names():
    assert state_suffix(()) == ""
    assert named("boxes.json", "") == "boxes.json"
    one = state_suffix(("details > summary",))
    assert one.startswith("-click-details-summary-")
    assert state_suffix(("details > summary",)) == one
    assert state_suffix(("details > summary", "a")) != one
    # Selectors with the same slug do not collide.
    assert state_suffix(("a b",)) != state_suffix(("a-b",))
    assert state_suffix(("!!!",)).startswith("-click-")
    assert named("boxes.json", one) == f"boxes{one}.json"
    assert "/" not in one


def test_union_difference_counts_the_extra_area():
    reference = np.zeros((10, 10, 3), dtype=np.uint8)
    taller = np.zeros((20, 10, 3), dtype=np.uint8)
    mask = union_difference(reference, taller)
    assert mask.shape == (20, 10)
    assert mask.sum() == 100  # the 10 rows that only `taller` has
    image = np.asarray(union_diff_image(reference, mask))
    assert image.shape == (20, 10, 3)
    assert (image[10:] == (255, 0, 0)).all()
    assert (image[:10] == 178).all()  # pale gray of black
    shorter = np.zeros((5, 10, 3), dtype=np.uint8)
    mask = union_difference(reference, shorter)
    assert mask.shape == (10, 10) and mask.sum() == 50


def test_union_difference_works_across_bands():
    rows = BAND * 2 + 7
    reference = np.zeros((rows, 4, 3), dtype=np.uint8)
    other = reference.copy()
    other[BAND - 1 : BAND + 1, 1] = 200  # across the first band boundary
    other[rows - 1, 3] = 10  # within the threshold
    mask = union_difference(reference, other)
    assert np.argwhere(mask).tolist() == [[BAND - 1, 1], [BAND, 1]]


def test_find_regions_groups_nearby_pixels():
    mask = np.zeros((100, 200), dtype=bool)
    mask[10:12, 10:50] = True  # 80 px
    mask[14, 52] = True  # within one block of the first: same region
    mask[60:90, 150:152] = True  # 60 px, a separate region
    mask[0, 199] = True  # a single pixel at the corner
    regions = find_regions(mask)
    assert regions == [
        Region((10, 10, 43, 5), 81),  # the most differing pixels first
        Region((150, 60, 2, 30), 60),
        Region((199, 0, 1, 1), 1),
    ]


def test_find_regions_empty_and_full():
    assert find_regions(np.zeros((5, 5), dtype=bool)) == []
    assert find_regions(np.ones((5, 7), dtype=bool)) == [Region((0, 0, 7, 5), 35)]


def test_covering_boxes_pick_the_smallest_containing_box():
    dump = BoxDump(
        "u",
        (100, 100),
        [
            Element("html", (0, 0, 100, 100), None),
            Element("body", (0, 0, 100, 100), 0),
            Element("p", (10, 10, 50, 20), 1),
            Element("span", (12, 12, 20, 10), 2),
            Element("head", None, 0),
        ],
    )
    regions = [Region((13, 13, 5, 5), 3), Region((40, 12, 5, 5), 3), Region((90, 90, 5, 5), 3)]
    covers = covering_boxes(regions, dump)
    assert covers == [
        "html > body:nth-child(1) > p > span",
        "html > body:nth-child(1) > p",
        "html > body:nth-child(1)",
    ]
    assert covering_boxes([Region((0, 0, 500, 500), 1)], dump) == [""]


def save(path, size, boxes=()):
    image = Image.new("RGB", size, WHITE)
    for x, y, w, h in boxes:
        image.paste((0, 0, 0), (x, y, x + w, y + h))
    image.save(path)


def test_compare_full_page_with_different_heights(tmp_path):
    save(tmp_path / "a.png", (40, 20), [(5, 5, 10, 4)])
    save(tmp_path / "b.png", (40, 30), [(5, 5, 10, 4)])
    dump = BoxDump("u", (40, 20), [Element("html", (0, 0, 40, 20), None)])
    result = compare_full_page(tmp_path / "a.png", tmp_path / "b.png", tmp_path / "d.png", 32, dump)
    assert result.sizes_differ
    assert result.reference_size == (40, 20)
    assert result.swb_size == (40, 30)
    assert result.score == pytest.approx(1 - 400 / 1200)
    assert result.regions == [Region((0, 20, 40, 10), 400)]
    assert result.covers == [""]
    with Image.open(tmp_path / "d.png") as diff:
        assert diff.size == (40, 30)
        assert diff.getpixel((0, 25)) == (255, 0, 0)


def test_compare_full_page_with_equal_sizes(tmp_path):
    save(tmp_path / "a.png", (40, 20), [(5, 5, 10, 4)])
    save(tmp_path / "b.png", (40, 20), [(5, 5, 10, 8)])
    dump = BoxDump("u", (40, 20), [Element("html", (0, 0, 40, 20), None)])
    result = compare_full_page(tmp_path / "a.png", tmp_path / "b.png", tmp_path / "d.png", 32, dump)
    assert not result.sizes_differ
    assert result.score == pytest.approx(1 - 40 / 800)
    assert result.regions == [Region((5, 9, 10, 4), 40)]
    assert result.covers == ["html #0"]


def sample_comparison():
    dump = BoxDump(
        "https://a.test/",
        (40, 20),
        [Element("html", (0, 0, 40, 20), None), Element("body", (0, 0, 40, 20), 0)],
    )
    return compare(dump, dump)


def make_full_page(tmp_path):
    save(tmp_path / "a.png", (40, 20))
    save(tmp_path / "b.png", (40, 24), [(0, 0, 4, 4)])
    return compare_full_page(
        tmp_path / "a.png",
        tmp_path / "b.png",
        tmp_path / "d.png",
        32,
        sample_comparison().reference,
    )


def test_summary_line_with_full_page(tmp_path):
    full_page = make_full_page(tmp_path)
    scores = Scores(1.0, 1.0, 1.0, 0, 0, 1.0)
    line = summary_line("page", scores, True, tmp_path / "r.html", full_page)
    assert " fullpage 0.8" in line
    assert "regions 2" in line
    assert "(size Chromium 40x20 swb 40x24)" in line
    assert "fullpage" not in summary_line("page", scores, True, tmp_path / "r.html")


def test_report_with_full_page_and_state(tmp_path):
    full_page = make_full_page(tmp_path)
    comparison = sample_comparison()
    path = tmp_path / "report.html"
    write_report(
        path,
        "page",
        comparison,
        comparison.scores(),
        {"Chromium": "x.png"},
        None,
        full_page=full_page,
        full_page_images={"swb": "fullpage.png"},
        clicks=("details > summary", "a<b"),
    )
    html = path.read_text()
    assert "<h2>Full page</h2>" in html
    assert "The sizes differ" in html
    assert "0, 20, 40&times;4" in html
    assert 'src="fullpage.png"' in html
    assert "<code>details &gt; summary</code>, then <code>a&lt;b</code>" in html
    plain = tmp_path / "plain.html"
    write_report(plain, "page", comparison, comparison.scores(), {}, None)
    assert "Full page" not in plain.read_text()
    assert "State after clicking" not in plain.read_text()


@pytest.fixture
def repo(tmp_path, monkeypatch):
    """A repository root with one fixture and its swb and Chromium output
    already in `out/compare/page/`, as `--no-run` expects."""
    monkeypatch.setenv("SWB_ROOT", str(tmp_path))
    fixture = tmp_path / "fixtures" / "pages" / "page"
    (fixture / "reference").mkdir(parents=True)
    meta = {"url": "https://a.test/", "captured": "x", "viewport": [40, 20], "chromium": ""}
    (fixture / "fixture.json").write_text(json.dumps(meta))
    dump = sample_comparison().reference
    write_dump(fixture / "reference" / "boxes.json", dump)
    save(fixture / "reference" / "screenshot.png", (40, 20))
    out = tmp_path / "out" / "compare" / "page"
    out.mkdir(parents=True)
    write_dump(out / "boxes.json", dump)
    save(out / "screenshot.png", (40, 20))
    return out


def test_compare_fixture_plain_run_is_unchanged(repo, capsys):
    scores = compare_fixture("page", None, 2.0, 32)
    assert scores is not None and scores.pixels == 1.0
    assert "fullpage" not in capsys.readouterr().out
    assert sorted(path.name for path in repo.iterdir()) == [
        "boxes.json",
        "diff.png",
        "reference.png",
        "report.html",
        "screenshot.png",
    ]


def test_compare_fixture_full_page(repo, capsys):
    save(repo / "reference-fullpage.png", (40, 60))
    save(repo / "fullpage.png", (40, 60), [(0, 50, 8, 8)])
    write_dump(repo / "reference-boxes.json", sample_comparison().reference)
    scores = compare_fixture("page", None, 2.0, 32, Options(full_page=True))
    assert scores is not None
    out = capsys.readouterr().out
    assert "fullpage 0.9733 regions 1" in out
    assert "<h2>Full page</h2>" in (repo / "report.html").read_text()
    assert (repo / "diff-fullpage.png").is_file()


def test_compare_fixture_click_state_keeps_the_plain_files(repo, capsys):
    clicks = ("details > summary",)
    suffix = state_suffix(clicks)
    plain_report = "plain report"
    (repo / "report.html").write_text(plain_report)
    # In a state, the Chromium output is the live capture, not the reference.
    write_dump(repo / f"reference-boxes{suffix}.json", sample_comparison().reference)
    save(repo / f"reference{suffix}.png", (40, 20), [(0, 0, 4, 4)])
    write_dump(repo / f"boxes{suffix}.json", sample_comparison().reference)
    save(repo / f"screenshot{suffix}.png", (40, 20))
    scores = compare_fixture("page", None, 2.0, 32, Options(clicks=clicks))
    assert scores is not None
    assert scores.pixels == pytest.approx(1 - 16 / 800)
    assert (repo / "report.html").read_text() == plain_report
    assert (repo / f"report{suffix}.html").is_file()
    assert (repo / f"diff{suffix}.png").is_file()
    assert "pixels 0.9800" in capsys.readouterr().out


def test_compare_fixture_without_full_page_files_fails(repo):
    assert compare_fixture("page", None, 2.0, 32, Options(full_page=True)) is None


def test_update_scores_is_refused_with_options(repo):
    for extra in (["--full-page"], ["--click", "a"]):
        args = cli.build_parser().parse_args(["compare", "page", "--update-scores", *extra])
        assert args.func(args) == 2


def test_click_option_is_repeatable():
    args = cli.build_parser().parse_args(["compare", "page", "--click", "a", "--click", "b > c"])
    assert args.click == ["a", "b > c"]
    assert cli.build_parser().parse_args(["compare", "page"]).click == []


class FakeBrowser:
    """Records the calls of `swb_state`."""

    def __init__(self, nodes):
        self.nodes = nodes
        self.calls = []

    def query_selector(self, selector):
        return self.nodes.get(selector)

    def click_node(self, node):
        self.calls.append(("click", node))

    def wait_for_load(self, timeout_ms=0):
        return True

    def info(self):
        return {"loadState": "complete"}

    def call(self, method, **params):
        self.calls.append((method, params))


def test_clicks_apply_in_order_then_scroll_to_the_top():
    browser = FakeBrowser({"a": 1, "b": 2})
    click_selectors(browser, ["b", "a"])
    assert browser.calls == [("click", 2), ("click", 1), ("page.scrollTo", {"x": 0, "y": 0})]
    browser = FakeBrowser({})
    click_selectors(browser, [])
    assert browser.calls == []


def test_missing_selector_is_an_error():
    with pytest.raises(StateError, match="no element matches 'x'"):
        click_selectors(FakeBrowser({}), ["x"])


PAGE = (
    "data:text/html,<body style='margin:0'><input id=c type=checkbox>"
    "<style>div{height:20px}%23c:checked ~ %23t{height:100px}</style><div id=t></div>"
    "<div style='height:2000px'></div>"
)


def test_capture_state_with_swb(tmp_path):
    if swb.find_swb() is None:
        pytest.skip("the swb binary does not exist")
    with Browser.start(viewport=(400, 300)) as browser:
        capture_state(
            browser,
            PAGE,
            ["#c"],
            tmp_path / "boxes.json",
            tmp_path / "shot.png",
            tmp_path / "full.png",
        )
    dump = json.loads((tmp_path / "boxes.json").read_text())
    heights = [e["rect"][3] for e in dump["elements"] if e["rect"] and e["tag"] == "div"]
    assert heights[0] == 100.0  # the checkbox was clicked
    with Image.open(tmp_path / "shot.png") as shot:
        assert shot.size == (400, 300)
    with Image.open(tmp_path / "full.png") as full:
        assert full.size[1] > 2000


def test_chromium_click_and_full_page_capture(chromium, tmp_path):
    """Chromium toggles `details` on a click, and the full-page capture
    leaves the layout (`vh`, fixed boxes) as it was."""
    html = (
        "<!doctype html><body style='margin:0'><details><summary>menu</summary>"
        "<div style='height:50px'>x</div></details><div style='height:100vh'></div>"
        "<div style='position:fixed;bottom:0;height:10px;width:10px'></div>"
    )

    async def run():
        async with chromium_browser.session((400, 300)) as (_, _, page):
            await chromium_browser.load_html(page, tmp_path / "t.html", html)
            before = await chromium_browser.collect_boxes(page, "u")
            await click_and_settle(page, ["details > summary"])
            opened = await chromium_browser.collect_boxes(page, "u")
            await page.screenshot(path=tmp_path / "full.png", full_page=True)
            after = await chromium_browser.collect_boxes(page, "u")
            return before, opened, after

    before, opened, after = asyncio.run(run())
    assert before != opened
    assert opened == after
    with Image.open(tmp_path / "full.png") as full:
        assert full.size[1] > 300
