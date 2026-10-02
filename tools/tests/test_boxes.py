"""The box dump format: rect union, rounding, reading and writing."""

import pytest

from swbtools.boxes import BoxDump, Element, format_dump, from_page_data, parse_dump, round2, union


def test_union_of_line_boxes():
    # An inline element split over two lines.
    rects = [[100.0, 10.0, 200.0, 17.0], [8.0, 30.0, 50.5, 17.0]]
    assert union(rects) == (8.0, 10.0, 292.0, 37.0)


def test_union_of_one_rect_and_of_none():
    assert union([[1.0, 2.0, 3.0, 4.0]]) == (1.0, 2.0, 3.0, 4.0)
    assert union([]) is None


def test_union_counts_empty_rects():
    assert union([[10.0, 10.0, 0.0, 0.0], [20.0, 30.0, 5.0, 5.0]]) == (10.0, 10.0, 15.0, 25.0)


def test_union_rounds_to_two_decimals():
    assert union([[0.333333, 1.0 / 3.0, 10.004999, 2.0]]) == (0.33, 0.33, 10.0, 2.0)
    assert union([[0.125, 0.0, 1.0, 1.0]]) == (0.13, 0.0, 1.0, 1.0)


@pytest.mark.parametrize(
    ("value", "rounded"),
    [
        (0.125, 0.13),
        (-0.125, -0.13),
        (1.005, 1.0),
        (2.5, 2.5),
        (-0.001, 0.0),
        (1280.0, 1280.0),
    ],
)
def test_round2_rounds_halves_away_from_zero(value, rounded):
    # 1.005 is 1.00499999... in binary, so it rounds down, as in Rust.
    result = round2(value)
    assert result == rounded
    assert str(result) != "-0.0"


SAMPLE = """\
{
  "url": "https://a.test/",
  "viewport": [1280, 800],
  "elements": [
    {"tag": "html", "rect": [0.0, 0.0, 1280.0, 2100.5], "parent": null},
    {"tag": "head", "rect": null, "parent": 0},
    {"tag": "p", "rect": [8.0, 8.0, 100.25, 18.0], "parent": 0}
  ]
}
"""


def sample_dump():
    return BoxDump(
        url="https://a.test/",
        viewport=(1280, 800),
        elements=[
            Element("html", (0.0, 0.0, 1280.0, 2100.5), None),
            Element("head", None, 0),
            Element("p", (8, 8, 100.25, 18), 0),
        ],
    )


def test_format_has_one_element_per_line():
    assert format_dump(sample_dump()) == SAMPLE


def test_format_without_elements():
    dump = BoxDump("https://a.test/", (800, 600), [])
    assert format_dump(dump).endswith('"elements": []\n}\n')
    assert parse_dump(format_dump(dump)) == dump


def test_parse_round_trip():
    dump = parse_dump(SAMPLE)
    assert dump.elements == sample_dump().elements
    assert dump.has_parents
    assert format_dump(dump) == SAMPLE


def test_parse_swb_dump_without_parents():
    text = """{
      "url": "https://a.test/",
      "viewport": [1280.0, 800.0],
      "elements": [
        {"tag": "html", "rect": [0.0, 0.0, 1280.0, 800.0]},
        {"tag": "clipPath", "rect": null}
      ]
    }"""
    dump = parse_dump(text)
    assert dump.viewport == (1280.0, 800.0)
    assert [element.tag for element in dump.elements] == ["html", "clippath"]
    assert not dump.has_parents


def test_from_page_data():
    data = {
        "viewport": [800, 600],
        "elements": [
            {"tag": "html", "parent": None, "rects": [[0, 0, 800, 100]]},
            {"tag": "linearGradient", "parent": 0, "rects": []},
            {"tag": "span", "parent": 0, "rects": [[10, 5, 20.004, 10], [0, 20, 5, 10]]},
        ],
    }
    dump = from_page_data("https://a.test/", data)
    assert dump.viewport == (800, 600)
    assert dump.elements == [
        Element("html", (0.0, 0.0, 800.0, 100.0), None),
        Element("lineargradient", None, 0),
        Element("span", (0.0, 5.0, 30.0, 25.0), 0),
    ]
