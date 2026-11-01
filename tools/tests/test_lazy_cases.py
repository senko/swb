"""Tests of `swbtools lazy-cases` (the Rust cases of the lazy loading probe file)."""

import json
import re

import pytest

from swbtools import paths
from swbtools.lazy_cases import CASES_FILE, PROBE_FILE, raw, render

PROBE = {
    "viewport": [800, 600],
    "javascript": True,
    "cases": [
        {
            "name": "one",
            "html": '<img src="a.png">',
            "resources": {"a.png": {"text": "x", "content_type": "image/png", "delay_ms": 5}},
            "measure": [{"js": "1+1"}],
        },
        {"name": "two", "javascript": False, "html": "<p>", "measure": [{"js": "2"}]},
    ],
}
CHROMIUM = {
    "cases": [
        {"name": "one", "results": [{"value": 2}]},
        {"name": "two", "results": [{"value": [1, "a"]}]},
    ]
}


def test_raw_picks_a_free_fence() -> None:
    assert raw("a") == 'r"a"'
    assert raw('a"b') == 'r#"a"b"#'
    assert raw('a"#b') == 'r##"a"#b"##'


def test_render_writes_each_case_with_its_values() -> None:
    text = render(PROBE, CHROMIUM)
    assert 'name: "one",' in text
    assert "viewport: (800.0, 600.0)," in text
    assert 'content_type: Some(r"image/png"),' in text
    assert "delay_ms: 5," in text
    assert 'name: "two",' in text
    assert "javascript: false," in text
    assert '(r"2", r#"[1,"a"]"#),' in text


def test_render_fails_for_a_case_that_chromium_did_not_run() -> None:
    with pytest.raises(KeyError):
        render(PROBE, {"cases": CHROMIUM["cases"][:1]})


def test_committed_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / PROBE_FILE).read_text())
    rust = (root / CASES_FILE).read_text()
    written = re.findall(r'^        name: "([^"]+)",$', rust, re.MULTILINE)
    assert written == [case["name"] for case in probe["cases"]]
