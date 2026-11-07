"""Tests of `swbtools web-cases` (the Rust cases of a probe file for crates/web/tests)."""

import json
import re
from pathlib import Path

import pytest

from swbtools import paths
from swbtools.web_cases import raw, render, write_web_cases

PROBE = {
    "cases": [
        {"name": "one", "html": "<p>x</p>", "measure": [{"js": "1+1"}, {"js": "'a'"}]},
        {"name": "two", "html": '<p id="a">', "measure": [{"js": "[1]"}]},
    ]
}
CHROMIUM = {
    "cases": [
        {"name": "one", "results": [{"value": 2}, {"value": "a"}]},
        {"name": "two", "results": [{"value": [1, "b"]}]},
    ]
}


def test_raw_picks_a_free_fence() -> None:
    assert raw("a") == 'r"a"'
    assert raw('a"b') == 'r#"a"b"#'


def test_render_writes_each_case_with_its_values() -> None:
    text = render(PROBE, CHROMIUM, "tools/probes/x.json")
    assert text.startswith("// The cases of tools/probes/x.json")
    assert 'name: r"one",' in text
    assert '(r"1+1", r"2"),' in text
    assert """(r"'a'", r#""a""#),""" in text
    assert 'html: r#"<p id="a">"#,' in text
    assert """(r"[1]", r#"[1, "b"]"#),""" in text


def test_render_fails_for_a_case_that_chromium_did_not_run() -> None:
    with pytest.raises(KeyError):
        render(PROBE, {"cases": CHROMIUM["cases"][:1]}, "x")


def test_render_fails_for_a_query_without_a_value() -> None:
    bad = {
        "cases": [
            {"name": "one", "results": [{"error": "boom"}, {"value": 1}]},
            CHROMIUM["cases"][1],
        ]
    }
    with pytest.raises(ValueError):
        render(PROBE, bad, "x")


def test_committed_node_iterator_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / "tools/probes/web-node-iterator.json").read_text())
    rust = (root / "crates/web/tests/node_iterator/cases.rs").read_text()
    written = re.findall(r'^        name: r"([^"]+)",$', rust, re.MULTILINE)
    assert written == [case["name"] for case in probe["cases"]]


def test_a_missing_input_is_an_error_message(tmp_path, caplog) -> None:
    out = tmp_path / "cases.rs"
    status = write_web_cases(
        Path("tools/probes/web-node-iterator.json"), tmp_path / "missing.json", out
    )
    assert status == 2
    assert "no such Chromium output" in caplog.text
    status = write_web_cases(Path("tools/probes/missing.json"), tmp_path / "missing.json", out)
    assert status == 2
    assert "no such probe file" in caplog.text
    assert not out.exists()
