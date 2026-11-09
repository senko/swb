"""Tests of `swbtools module-cases` (the Rust cases of the module script probe file)."""

import json
import re

import pytest

from swbtools import paths
from swbtools.module_cases import (
    CASES_FILE,
    CE_CASES_FILE,
    CE_PROBE_FILE,
    GRAPH_CASES_FILE,
    GRAPH_PROBE_FILE,
    PROBE_FILE,
    SHADOW_CASES_FILE,
    SHADOW_PROBE_FILE,
    raw,
    render,
)

PROBE = {
    "viewport": [400, 200],
    "javascript": True,
    "cases": [
        {
            "name": "one",
            "html": "<script type=module src=a.js></script>",
            "resources": {
                "a.js": {
                    "text": "log('a')",
                    "delay_ms": 5,
                    "headers": {"Access-Control-Allow-Origin": "*"},
                    "content_type": "text/plain",
                    "status": 404,
                }
            },
            "measure": [{"js": "L.join(' ')"}],
        },
        {"name": "two", "html": "<p>", "measure": [{"js": "2"}]},
    ],
}
CHROMIUM = {
    "cases": [
        {"name": "one", "results": [{"value": "a:i"}]},
        {"name": "two", "results": [{"value": [1, "a"]}]},
    ]
}


def test_raw_picks_a_free_fence() -> None:
    assert raw("a") == 'r"a"'
    assert raw('a"b') == 'r#"a"b"#'


def test_render_writes_each_case_with_its_resources_and_values() -> None:
    text = render(PROBE, CHROMIUM)
    assert 'name: r"one",' in text
    assert 'content_type: Some(r"text/plain"),' in text
    assert "status: 404," in text
    assert "delay_ms: 5," in text
    assert 'headers: &[(r"Access-Control-Allow-Origin", r"*")],' in text
    assert '(r"L.join(\' \')", r#""a:i""#),' in text
    assert 'name: r"two",' in text
    assert '(r"2", r#"[1,"a"]"#),' in text


def test_render_fails_for_a_case_that_chromium_did_not_run() -> None:
    with pytest.raises(KeyError):
        render(PROBE, {"cases": CHROMIUM["cases"][:1]})


def test_committed_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / PROBE_FILE).read_text())
    rust = (root / CASES_FILE).read_text()
    written = re.findall(r'^        name: r?"([^"]+)",$', rust, re.MULTILINE)
    assert written == [case["name"] for case in probe["cases"]]


def test_render_skips_the_named_cases() -> None:
    text = render(PROBE, CHROMIUM, GRAPH_PROBE_FILE, lambda name: name == "one")
    assert 'name: r"one",' not in text
    assert 'name: r"two",' in text
    assert "web-module-graph.json" in text
    assert "module-graph-cases" in text


def test_committed_graph_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / GRAPH_PROBE_FILE).read_text())
    rust = (root / GRAPH_CASES_FILE).read_text()
    written = re.findall(r'^        name: r?"([^"]+)",$', rust, re.MULTILINE)
    assert written == [c["name"] for c in probe["cases"]]


def test_render_leaves_out_the_cases_with_javascript_off() -> None:
    probe = {
        **PROBE,
        "cases": [
            *PROBE["cases"],
            {
                "name": "off",
                "javascript": False,
                "html": "",
                "measure": [{"style": "p", "props": ["color"]}],
            },
        ],
    }
    text = render(probe, CHROMIUM, CE_PROBE_FILE)
    assert 'name: r"off",' not in text
    assert "ce-cases" in text
    assert "Custom element tests" in text


def test_committed_ce_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / CE_PROBE_FILE).read_text())
    rust = (root / CE_CASES_FILE).read_text()
    written = re.findall(r'^        name: r?"([^"]+)",$', rust, re.MULTILINE)
    assert written == [c["name"] for c in probe["cases"] if c.get("javascript", True)]


def test_committed_shadow_cases_are_those_of_the_probe_file() -> None:
    root = paths.repo_root()
    probe = json.loads((root / SHADOW_PROBE_FILE).read_text())
    rust = (root / SHADOW_CASES_FILE).read_text()
    written = re.findall(r'^        name: r?"([^"]+)",$', rust, re.MULTILINE)
    assert written == [c["name"] for c in probe["cases"] if c.get("javascript", True)]
    assert "shadow-cases" in rust
