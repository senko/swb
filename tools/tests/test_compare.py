"""Scores file, pixel comparison and report output."""

import io
import json

import numpy as np
from PIL import Image

from swbtools.boxes import BoxDump, Element
from swbtools.compare import floor4, format_scores, scores_entry, summary_line, update_scores
from swbtools.pixels import (
    compare_screenshots,
    difference_mask,
    pixel_score,
    write_png_unless_close,
)
from swbtools.report import write_report
from swbtools.scoring import Scores, compare


def test_floor4_never_rounds_up():
    assert floor4(0.45126) == 0.4512
    assert floor4(1.0) == 1.0
    assert floor4(0.0) == 0.0
    assert floor4(2 / 3) == 0.6666
    for k in range(1, 1000):
        value = k / 997
        assert floor4(value) <= value


def test_scores_file_format(tmp_path):
    path = tmp_path / "scores.json"
    path.write_text(
        json.dumps({"zeta": {"geometry": 0.5, "size": 0.5, "relative": 0.5, "pixels": 0.5}})
    )
    update_scores(path, {"alpha": Scores(0.123456, 0.9, 0.95, 0, 0, 0.87654)})
    text = path.read_text()
    assert text == format_scores(json.loads(text))
    assert text.endswith("}\n")
    data = json.loads(text)
    assert list(data) == ["alpha", "zeta"]
    assert list(data["alpha"]) == ["geometry", "pixels", "relative", "size"]
    assert data["alpha"] == {"geometry": 0.1234, "pixels": 0.8765, "relative": 0.95, "size": 0.9}
    assert data["zeta"]["geometry"] == 0.5


def test_scores_entry_without_pixels():
    assert scores_entry(Scores(1.0, 1.0, 1.0, 0, 0, None))["pixels"] == 0.0


def test_summary_line(tmp_path):
    line = summary_line("page", Scores(0.5, 0.75, 1.0, 3, 4, None), False, tmp_path / "r.html")
    assert line.startswith("page: geometry 0.5000 size 0.7500 relative 1.0000 pixels n/a")
    assert "missing 3 extra 4 (tag sequences differ)" in line


def test_pixel_score_with_threshold():
    reference = np.zeros((10, 10, 3), dtype=np.int16)
    other = reference.copy()
    other[0, :5] = (32, 0, 0)  # within the threshold
    other[1, :5] = (0, 33, 0)  # beyond it
    mask = difference_mask(reference, other)
    assert mask.sum() == 5
    assert pixel_score(mask) == 0.95


def test_pixel_score_with_different_sizes():
    reference = np.zeros((10, 10, 3), dtype=np.int16)
    smaller = np.zeros((5, 10, 3), dtype=np.int16)
    assert pixel_score(difference_mask(reference, smaller)) == 0.5
    larger = np.zeros((20, 20, 3), dtype=np.int16)
    assert pixel_score(difference_mask(reference, larger)) == 1.0


def test_compare_screenshots_writes_diff(tmp_path):
    Image.new("RGB", (4, 2), (255, 255, 255)).save(tmp_path / "a.png")
    other = Image.new("RGBA", (4, 2), (255, 255, 255, 255))
    other.putpixel((0, 0), (0, 0, 0, 255))
    other.save(tmp_path / "b.png")
    score = compare_screenshots(tmp_path / "a.png", tmp_path / "b.png", tmp_path / "d.png")
    assert score == 7 / 8
    with Image.open(tmp_path / "d.png") as diff:
        assert diff.getpixel((0, 0)) == (255, 0, 0)
        assert diff.getpixel((1, 0)) != (255, 0, 0)


def test_report_is_written(tmp_path):
    reference = BoxDump(
        "https://a.test/",
        (800, 600),
        [
            Element("html", (0, 0, 800, 100), None),
            Element("body", (8, 8, 784, 84), 0),
            Element("p", (8, 8, 100, 20), 1),
            Element("div", (8, 30, 100, 20), 1),
        ],
    )
    swb = BoxDump(
        "https://a.test/",
        (800, 600),
        [
            Element("html", (0, 0, 800, 100)),
            Element("body", (8, 8, 784, 84)),
            Element("p", (8, 18, 100, 20)),
            Element("span", (8, 30, 100, 20)),
        ],
    )
    comparison = compare(reference, swb)
    path = tmp_path / "report.html"
    write_report(path, "a<b", comparison, comparison.scores(), {"Chromium": "x.png"}, "swb.log")
    html = path.read_text()
    assert "a&lt;b" in html
    assert "html &gt; body &gt; p:nth-child(1)" in html
    assert "Missing in swb (1)" in html
    assert "Extra in swb (1)" in html
    assert "+10.00" in html


def png_bytes(color):
    buffer = io.BytesIO()
    Image.new("RGB", (3, 2), color).save(buffer, format="PNG")
    return buffer.getvalue()


def test_screenshot_noise_does_not_rewrite_the_file(tmp_path):
    path = tmp_path / "shot.png"
    assert write_png_unless_close(path, png_bytes((100, 100, 100)))
    original = path.read_bytes()
    assert not write_png_unless_close(path, png_bytes((102, 99, 100)))
    assert path.read_bytes() == original
    assert write_png_unless_close(path, png_bytes((103, 100, 100)))
    assert path.read_bytes() != original
