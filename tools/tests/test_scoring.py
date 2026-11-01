"""Score computation and tag sequence alignment."""

import itertools
import random

import pytest

from swbtools.boxes import BoxDump, Element
from swbtools.scoring import align, compare, element_paths, myers_matches


def dump(*elements):
    return BoxDump("https://a.test/", (1280, 800), list(elements))


def page(shift=0.0):
    """html > body > (div > p, p, div > span); everything after the first div
    moves down by `shift`."""
    return dump(
        Element("html", (0, 0, 1280, 500 + shift), None),  # 0
        Element("head", None, 0),  # 1
        Element("body", (8, 8, 1264, 484 + shift), 0),  # 2
        Element("div", (8, 8, 1264, 100 + shift), 2),  # 3
        Element("p", (8, 8, 1264, 20), 3),  # 4
        Element("p", (8, 120 + shift, 1264, 20), 2),  # 5
        Element("div", (8, 150 + shift, 1264, 40), 2),  # 6
        Element("span", (20, 160 + shift, 50, 17), 6),  # 7
    )


def test_identical_dumps_score_one():
    result = compare(page(), page())
    scores = result.scores()
    assert result.alignment.identical
    assert (scores.geometry, scores.size, scores.relative) == (1.0, 1.0, 1.0)
    assert (scores.missing, scores.extra) == (0, 0)
    assert len(result.results) == 7  # head has no box


def test_relative_score_ignores_an_earlier_vertical_shift():
    scores = compare(page(), page(shift=10)).scores()
    # html, body and the first div change height. p (5), div (6) and span (7)
    # move down. Only p (4) keeps its absolute geometry.
    assert scores.geometry == pytest.approx(1 / 7)
    # Sizes: html, body, first div differ.
    assert scores.size == pytest.approx(4 / 7)
    # Relative to body, p (5) and div (6) move; span is fine relative to div.
    # html, body and the first div fail on size.
    assert scores.relative == pytest.approx(2 / 7)


def test_tolerance_is_inclusive():
    reference = dump(Element("html", (0, 0, 100, 100), None))
    exact = compare(reference, dump(Element("html", (2, -2, 102, 98), None)))
    assert exact.scores().geometry == 1.0
    beyond = compare(reference, dump(Element("html", (2.01, 0, 100, 100), None)))
    assert beyond.scores().geometry == 0.0
    assert (
        compare(reference, dump(Element("html", (2.01, 0, 100, 100), None)), 3).scores().geometry
        == 1.0
    )


def test_missing_and_extra_boxes():
    reference = dump(
        Element("html", (0, 0, 100, 100), None),
        Element("div", (0, 0, 10, 10), 0),
        Element("span", None, 0),
    )
    swb = dump(
        Element("html", (0, 0, 100, 100), None),
        Element("div", None, 0),
        Element("span", (0, 0, 5, 5), 0),
    )
    result = compare(reference, swb)
    scores = result.scores()
    assert (scores.missing, scores.extra) == (1, 1)
    assert result.missing == [1]
    assert result.extra == [2]
    assert scores.geometry == 0.5


def test_inserted_element_is_aligned_away():
    reference = page()
    elements = list(reference.elements)
    # swb has an extra <b> after the first <p>. Its parent index is not used.
    elements.insert(5, Element("b", (8, 30, 10, 10), 3))
    swb = dump(*elements)
    result = compare(reference, swb)
    assert not result.alignment.identical
    assert result.alignment.pairs[4] == 4
    assert result.alignment.pairs[5] == 6
    assert result.alignment.pairs[7] == 8
    assert [(d.kind, d.reference, d.swb) for d in result.alignment.differences] == [
        ("insert", (5, 5), (5, 6))
    ]
    scores = result.scores()
    assert scores.geometry == 1.0
    assert (scores.missing, scores.extra) == (0, 1)


def test_deleted_element_counts_as_missing():
    reference = page()
    # swb lacks the first div (its children stay).
    swb = dump(*[element for index, element in enumerate(reference.elements) if index != 3])
    result = compare(reference, swb)
    scores = result.scores()
    assert result.missing == [3]
    assert scores.geometry == pytest.approx(6 / 7)
    assert scores.extra == 0


def test_align_handles_replaced_runs_and_common_ends():
    reference = ["html", "head", "body", "div", "p", "p", "span", "footer"]
    swb = ["html", "head", "body", "section", "p", "span", "footer"]
    alignment = align(reference, swb)
    assert alignment.pairs[0] == 0
    assert alignment.pairs[2] == 2
    assert 3 not in alignment.pairs
    assert alignment.pairs[6] == 5
    assert alignment.pairs[7] == 6
    paired = sorted(alignment.pairs.items())
    # The pairing keeps document order.
    assert [j for _, j in paired] == sorted(j for _, j in paired)
    for i, j in paired:
        assert reference[i] == swb[j]


def test_align_with_one_side_empty():
    assert align([], []).identical
    alignment = align(["html", "body"], [])
    assert alignment.pairs == {}
    assert [(d.kind, d.reference, d.swb) for d in alignment.differences] == [
        ("delete", (0, 2), (0, 0))
    ]


def test_no_boxes_scores_one():
    scores = compare(dump(Element("head", None, None)), dump(Element("head", None, None))).scores()
    assert scores.geometry == 1.0


def test_element_paths():
    paths = element_paths(page())
    assert paths[0] == "html"
    assert paths[2] == "html > body:nth-child(2)"
    assert paths[4] == "html > body:nth-child(2) > div:nth-child(1) > p"
    assert paths[7] == "html > body:nth-child(2) > div:nth-child(3) > span"


def test_element_paths_without_parents():
    paths = element_paths(dump(Element("html", None), Element("body", None)))
    assert paths == ["html #0", "body #1"]


def lcs_length(a, b):
    table = [[0] * (len(b) + 1) for _ in range(len(a) + 1)]
    for i, x in enumerate(a):
        for j, y in enumerate(b):
            table[i + 1][j + 1] = (
                table[i][j] + 1 if x == y else max(table[i][j + 1], table[i + 1][j])
            )
    return table[-1][-1]


def test_myers_finds_a_longest_common_subsequence():
    rng = random.Random(5)
    for _ in range(300):
        a = [rng.choice("abc") for _ in range(rng.randrange(0, 15))]
        b = [rng.choice("abc") for _ in range(rng.randrange(0, 15))]
        matches = myers_matches(a, b)
        assert matches is not None
        assert len(matches) == lcs_length(a, b)
        for (i1, j1), (i2, j2) in itertools.pairwise(matches):
            assert i1 < i2 and j1 < j2
        assert all(a[i] == b[j] for i, j in matches)


def test_myers_gives_up_above_the_edit_limit():
    assert myers_matches(list("abcdef"), list("uvwxyz"), max_edits=4) is None
    assert myers_matches(list("abcdef"), list("abxdef"), max_edits=2) == [
        (0, 0),
        (1, 1),
        (3, 3),
        (4, 4),
        (5, 5),
    ]


def test_repeated_rows_align_with_the_fewest_edits():
    row = ["tr", "td", "span", "td", "a", "span", "a"]
    reference = ["html", "body", "table", *(row * 30)]
    swb = list(reference)
    del swb[40]
    swb.insert(100, "b")
    alignment = align(reference, swb)
    assert len(alignment.pairs) == len(reference) - 1
    assert [d.kind for d in alignment.differences] == ["delete", "insert"]


def js_page(extra_script=False, extra_section=False, hidden_iframe=False, iframe_rect=None):
    """A page as scripts change it: html > head (meta, script) and body
    (div, p, div > span), where a run can add a script in the head, an
    iframe (hidden, or with `iframe_rect`) and a section with a box before
    the first div."""
    elements = [
        Element("html", (0, 0, 1280, 500), None),  # 0
        Element("head", None, 0),
        Element("meta", None, 1),
        Element("script", None, 1),
    ]
    if extra_script:
        elements.append(Element("script", None, 1))
    elements.append(Element("body", (8, 8, 1264, 484), 0))
    body = len(elements) - 1
    if hidden_iframe:
        elements.append(Element("iframe", iframe_rect, body))
    if extra_section:
        elements.append(Element("section", (8, 8, 1264, 1), body))
    elements.append(Element("div", (8, 9, 1264, 100), body))
    div = len(elements) - 1
    elements.append(Element("p", (8, 109, 1264, 20), body))
    elements.append(Element("div", (8, 130, 1264, 40), body))
    elements.append(Element("span", (20, 140, 50, 17), len(elements) - 1))
    elements.append(Element("script", None, div))
    return dump(*elements)


def test_scripts_and_frames_without_a_box_do_not_count_with_javascript():
    reference = js_page(extra_script=True, hidden_iframe=True)
    swb = js_page()
    stock = compare(reference, swb)
    assert not stock.alignment.identical
    assert len(stock.alignment.differences) == 2
    skipped = compare(reference, swb, skip_ignored=True)
    scores = skipped.scores()
    assert (scores.geometry, scores.size, scores.relative) == (1.0, 1.0, 1.0)
    assert (scores.missing, scores.extra) == (0, 0)
    assert skipped.alignment.identical
    assert skipped.alignment.differences == []
    # The other elements keep their partners.
    assert skipped.alignment.pairs[len(reference.elements) - 2] == len(swb.elements) - 2


def test_ignored_elements_with_a_box_still_count():
    reference = js_page(hidden_iframe=True, iframe_rect=(0, 0, 10, 10))
    comparison = compare(reference, js_page(), skip_ignored=True)
    assert not comparison.alignment.identical
    assert comparison.scores().missing == 1


def test_differences_keep_the_indices_of_the_dumps():
    # A section with a box in Chromium only, after an extra script.
    reference = js_page(extra_script=True, extra_section=True)
    comparison = compare(reference, js_page(), skip_ignored=True)
    assert not comparison.alignment.identical
    [difference] = comparison.alignment.differences
    assert difference.kind == "delete"
    section = next(i for i, e in enumerate(reference.elements) if e.tag == "section")
    assert difference.reference == (section, section + 1)
    assert comparison.scores().missing == 1
