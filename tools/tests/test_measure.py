"""Tests of `swbtools measure`.

The Chromium tests compare the measurements with the data in swb's source
code, so they fail when swb's data no longer matches Chromium. They are
skipped when Chromium cannot start.
"""

import asyncio
import re

import pytest

from swbtools import paths
from swbtools.measure import (
    FAMILY_NAMES,
    GENERIC_FAMILIES,
    POSITION_PROBES,
    FieldWidth,
    _collapse,
    family_candidates,
    font_size_keywords,
    in_chromium,
    picture_sources,
    text_field_widths,
    ua_styles,
    zero_width_families,
)


def rust_const(path: str, name: str) -> str:
    """The text between the brackets of `const NAME: [...] = [...];`."""
    source = (paths.repo_root() / path).read_text(encoding="utf-8")
    match = re.search(rf"const {name}: \[[^\]]*\] = \[(.*?)\];", source, re.S)
    assert match, f"{name} not found in {path}"
    return match.group(1)


def test_field_width_verdicts():
    assert FieldWidth("x", 160, 200, 160).verdict == "zero"
    assert FieldWidth("x", 200, 200, 160).verdict == "average"
    # Both rules give the same width: the family cannot be classified.
    assert FieldWidth("x", 200, 200, 200).verdict == "undetermined"
    assert FieldWidth("x", 180, 200, 160).verdict == "undetermined"


def test_family_names_are_unique():
    assert len(set(FAMILY_NAMES)) == len(FAMILY_NAMES)


def test_collapse_joins_equal_sides():
    values = {
        "margin-top": "3px",
        "margin-right": "3px",
        "margin-bottom": "3px",
        "margin-left": "4px",
        "border-top-left-radius": "2px",
        "border-top-right-radius": "2px",
        "border-bottom-right-radius": "2px",
        "border-bottom-left-radius": "2px",
        "color": "red",
    }
    assert _collapse(values) == {
        "margin-top": "3px",
        "margin-right": "3px",
        "margin-bottom": "3px",
        "margin-left": "4px",
        "border-*-radius": "2px",
        "color": "red",
    }


@pytest.mark.usefixtures("chromium")
def test_zero_width_families_match_swb():
    widths = asyncio.run(in_chromium(lambda p, d: text_field_widths(p, d, family_candidates())))
    assert [w.family for w in widths if w.verdict == "undetermined"] == []
    # swb treats generic families as never special.
    generic = [w for w in widths if w.family in GENERIC_FAMILIES]
    assert len(generic) == len(GENERIC_FAMILIES)
    assert {w.verdict for w in generic} == {"average"}
    body = rust_const("crates/layout/src/control.rs", "ZERO_WIDTH_FAMILIES")
    assert zero_width_families(widths) == re.findall(r'"([^"]*)"', body)


@pytest.mark.usefixtures("chromium")
def test_only_the_first_family_counts():
    widths = asyncio.run(in_chromium(lambda p, d: text_field_widths(p, d, POSITION_PROBES)))
    assert {w.family: w.verdict for w in widths} == {
        '"Times", serif': "zero",
        'serif, "Times"': "average",
        '"swb-probe-missing", "Times"': "average",
    }


@pytest.mark.usefixtures("chromium")
def test_font_size_keywords_match_swb():
    table = asyncio.run(in_chromium(font_size_keywords))

    def sizes(name: str) -> list[float]:
        body = rust_const("crates/style/src/values/keywords.rs", name)
        return [float(v) for v in body.split(",") if v.strip()]

    assert table["16px standards"] == sizes("KEYWORD_SIZES_16")
    assert table["16px quirks"] == sizes("KEYWORD_SIZES_16")
    assert table["13px standards"] == sizes("KEYWORD_SIZES_13_STRICT")
    assert table["13px quirks"] == sizes("KEYWORD_SIZES_13_QUIRKS")


@pytest.mark.usefixtures("chromium")
def test_picture_sources():
    assert asyncio.run(in_chromium(picture_sources)) == {
        "media does not match": "fallback.png",
        "type not supported": "fallback.png",
        "media does not match, type not supported": "fallback.png",
        "media matches, type supported": "source.png",
    }


@pytest.mark.usefixtures("chromium")
def test_ua_styles():
    styles, colors, frameset = asyncio.run(in_chromium(ua_styles))
    by_element = {style.element: style for style in styles}
    text_field = by_element["<input>"]
    assert text_field.set_values["padding-top"] == "1px"
    assert text_field.set_values["border-top-style"] == "inset"
    assert text_field.set_values["font-family"] == "Arial"
    assert text_field.placeholder is not None
    assert text_field.placeholder["color"] == "rgb(117, 117, 117)"
    assert by_element["<ruby>x<rt data-measure>y</rt></ruby>"].set_values["font-size"] == "8.5px"
    assert by_element["<select><option data-measure>x</select>"].set_values["white-space"] == (
        "nowrap"
    )
    assert "text-indent" not in by_element["<map></map>"].set_values
    assert colors["ButtonFace"] == "rgb(239, 239, 239)"
    assert "frameset: display block; border-top-color rgb(9, 9, 9)" in frameset
