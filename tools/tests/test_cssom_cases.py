"""Tests of `swbtools cssom-cases` (the Rust cases of the CSSOM sheets probe file)."""

import json
import re

import pytest

from swbtools import paths
from swbtools.cssom_cases import CASES_FILE, PROBE_FILE, raw, render

PROBE = {
    "cases": [
        {
            "name": "one",
            "html": "<p>",
            "resources": {"a.css": {"text": "p{}", "content_type": "text/css"}},
            "measure": [{"js": "1+1"}],
        },
        {"name": "two", "html": "<p>", "measure": [{"js": "2"}, {"js": "'é'"}]},
        {"name": "three", "html": "<b>", "measure": [{"js": "3"}]},
    ],
}
CHROMIUM = {
    "cases": [
        {"name": "one", "results": [{"value": 2}]},
        {"name": "two", "results": [{"value": [1, "a"]}, {"value": "é"}]},
        {"name": "three", "results": [{"value": None}]},
    ]
}


def test_raw_picks_a_free_fence() -> None:
    assert raw("a") == 'r"a"'
    assert raw('a"b') == 'r#"a"b"#'
    assert raw('a"#b') == 'r##"a"#b"##'


def test_render_writes_each_case_with_its_values() -> None:
    text = render(PROBE, CHROMIUM)
    assert 'name: r"one",' in text
    assert '(r"a.css", r"text/css", r"p{}"),' in text
    assert '(r"2", r#"[1, "a"]"#),' in text
    assert '(r"3", r"null"),' in text


def test_cases_with_one_page_share_a_constant() -> None:
    text = render(PROBE, CHROMIUM)
    assert text.count('pub(crate) const PAGE_1: &str = r"<p>";') == 1
    assert 'pub(crate) const PAGE_2: &str = r"<b>";' in text
    assert text.count("html: PAGE_1,") == 2


def test_render_fails_for_a_case_that_chromium_did_not_run() -> None:
    with pytest.raises(KeyError):
        render(PROBE, {"cases": CHROMIUM["cases"][:1]})


def test_committed_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / PROBE_FILE).read_text(encoding="utf-8"))
    rust = (root / CASES_FILE).read_text(encoding="utf-8")
    written = re.findall(r'^        name: r"([^"]+)",$', rust, re.MULTILINE)
    assert written == [case["name"] for case in probe["cases"]]
