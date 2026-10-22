"""Tests of the pure functions of js-survey (no browser)."""

import pytest

from swbtools import paths
from swbtools.js_survey_data import (
    area_of,
    area_table,
    event_types,
    group_scripts,
    interaction_listeners,
    is_interaction_event,
    median_metrics,
    merge_hists,
    merge_members,
    merge_sums,
    metrics_from_cdp,
    render_markdown,
    request_summary,
    script_kinds,
    site_of,
    summarize_trace,
    top_members,
)
from swbtools.pages import FixtureMeta, read_meta, write_meta


def test_area_of():
    assert area_of("Node.appendChild") == "dom-core"
    assert area_of("Element.getBoundingClientRect") == "cssom-layout"
    assert area_of("HTMLElement.offsetWidth") == "cssom-layout"
    assert area_of("CSSStyleDeclaration.display") == "cssom-layout"
    assert area_of("window.setTimeout") == "timers"
    assert area_of("window.fetch") == "network"
    assert area_of("XMLHttpRequest.open") == "network"
    assert area_of("Document.cookie") == "storage"
    assert area_of("Storage.getItem") == "storage"
    assert area_of("MutationObserver.observe") == "observers"
    assert area_of("History.pushState") == "history-location"
    assert area_of("HTMLCanvasElement.getContext") == "canvas"
    assert area_of("HTMLDivElement.align") == "html-elements"
    assert area_of("EventTarget.addEventListener") == "events"
    assert area_of("MouseEvent.clientX") == "events"
    assert area_of("Worker.postMessage") == "workers-other"
    assert area_of("console.log") == "browser-misc"
    assert area_of("window.name") == "window-misc"
    assert area_of("Unknown.thing") == "other"


COUNTS = {
    "parse": {"Node.appendChild|call": 3, "Element.id|get": 2, "Element.id|set": 1},
    "load": {"Node.appendChild|call": 1},
    "after": {"window.setTimeout|call": 5},
}


def test_merge_members_and_tables():
    members = merge_members(COUNTS)
    assert members["Node.appendChild"] == {"call": {"parse": 3, "load": 1}}
    assert members["Element.id"] == {"get": {"parse": 2}, "set": {"parse": 1}}
    table = area_table(members)
    assert table["dom-core"] == {"members": 2, "ops": 7}
    assert table["timers"] == {"members": 1, "ops": 5}
    assert list(table) == ["dom-core", "timers"]
    top = top_members(members, 2)
    assert top[0] == ("window.setTimeout", 5, {"parse": 0, "load": 0, "after": 5})
    assert top[1][0] == "Node.appendChild"
    assert top_members(members, 5, area="timers")[0][0] == "window.setTimeout"


def test_merge_hists_orders_and_cuts():
    merged = merge_hists(
        {
            "parse": {"h": {"a": 1, "b": 5}},
            "after": {"h": {"a": 2, "c": 1}},
        },
        limit=2,
    )
    assert merged["h"] == [
        {"value": "b", "total": 5, "parse": 5, "load": 0, "after": 0},
        {"value": "a", "total": 3, "parse": 1, "load": 0, "after": 2},
    ]


def test_event_types():
    args = {
        "parse": {
            "addEventListener": {"click @ a": 2, "click @ b": 1, "load @ window": 1},
            "on-handler": {"click @ a": 1},
        },
        "after": {"addEventListener": {"load @ window": 3}},
    }
    assert event_types(args) == [("click", 4, 2), ("load", 4, 1)]
    assert event_types(args, limit=1)[0][1] == 4


def test_merge_sums():
    merged = merge_sums(
        {"parse": {"w": {"count": 2, "total": 30}}, "after": {"w": {"count": 1, "total": 5}}}
    )
    assert merged["w"]["count"] == 3
    assert merged["w"]["total"] == 35
    assert merged["w"]["by_phase"]["after"] == {"count": 1, "total": 5}


def test_interaction_listeners():
    assert is_interaction_event("click")
    assert is_interaction_event("keydown")
    assert is_interaction_event("pointerenter")
    assert not is_interaction_event("DOMContentLoaded")
    assert not is_interaction_event("load")
    rows = [
        {"value": "click @ button#menu", "total": 2},
        {"value": "load @ window", "total": 1},
        {"value": "keydown @ document", "total": 1},
    ]
    assert [r["value"] for r in interaction_listeners(rows)] == [
        "click @ button#menu",
        "keydown @ document",
    ]


def test_site_of():
    assert site_of("www.bbc.com") == "bbc.com"
    assert site_of("static.files.bbci.co.uk") == "bbci.co.uk"
    assert site_of("news.ycombinator.com:443") == "ycombinator.com"
    assert site_of("localhost") == "localhost"
    assert site_of("127.0.0.1") == "127.0.0.1"


def test_group_scripts():
    page = "https://www.example.com/"
    groups = group_scripts(
        [
            {"url": "https://www.example.com/", "length": 100},
            {"url": "https://static.example.com/a.js", "length": 2000},
            {"url": "https://ads.other.net/x.js", "length": 5000, "module": True},
            {"url": "https://ads.other.net/y.js", "length": 10},
            {"url": "", "length": 7},
        ],
        page,
    )
    assert next(iter(groups)) == "ads.other.net"
    assert groups["ads.other.net"] == {"count": 2, "bytes": 5010, "modules": 1, "party": "third"}
    assert groups["static.example.com"]["party"] == "first"
    assert groups["(inline in the page)"]["count"] == 1
    assert groups["(eval or Function)"]["bytes"] == 7


def test_script_kinds():
    html = """
    <script>var a;</script>
    <script src="a.js"></script>
    <script src="b.js" async></script>
    <script src="c.js" defer></script>
    <script src="d.mjs" type="module"></script>
    <script src="e.js" nomodule></script>
    <script type="application/ld+json">{}</script>
    <script type="text/javascript" src="f.js"></script>
    """
    assert script_kinds(html) == {
        "total": 8,
        "external": 6,
        "inline": 1,
        "classic_blocking": 3,
        "async": 1,
        "defer": 1,
        "module": 1,
        "nomodule": 1,
        "data_or_other_type": 1,
    }


def test_request_summary():
    summary = request_summary(
        [
            ("document", "https://a.com/"),
            ("script", "https://a.com/x.js"),
            ("script", "https://t.net/y.js"),
            ("xhr", "https://t.net/z"),
            ("image", "data:image/png;base64,AA"),
        ],
        ["https://t.net/z#frag"],
        "https://a.com/",
    )
    assert summary["total"] == 4
    assert summary["third_party"] == 2
    assert summary["by_type"]["xhr"] == {"served": 0, "missing": 1}
    assert summary["by_type"]["script"] == {"served": 2, "missing": 0}
    assert summary["missing_by_host"] == [("t.net", 1)]


def test_metrics():
    cdp = [{"name": "ScriptDuration", "value": 0.5}, {"name": "Other", "value": 1}]
    assert metrics_from_cdp(cdp) == {"ScriptDuration": 0.5}
    runs = [{"ScriptDuration": 1.0}, {"ScriptDuration": 3.0}, {"ScriptDuration": 2.0}]
    assert median_metrics(runs) == {"ScriptDuration": 2.0}


def _survey() -> dict:
    raw = {
        "counts": COUNTS,
        "args": {"parse": {"addEventListener": {"click @ a": 2}}},
        "sums": {},
        "timers_run": {"after": {"timeout": 2}},
        "mutations": {
            "parse": {"records": 1, "added_by_tag": {"div": 1}, "attributes_by_name": {}},
            "later": {"records": 0},
        },
    }
    trace = summarize_trace(raw)
    metrics = {"ScriptDuration": 0.01}
    return {
        "url": "https://a.com/",
        "chromium": "1",
        "date": "d",
        "html_scripts": script_kinds("<script>x</script>"),
        "compiled_scripts": group_scripts([{"url": "", "length": 2048}], "https://a.com/"),
        "trace": trace,
        "interaction_listeners": trace["args"]["addEventListener"],
        "errors": {"console": [], "pageerror": []},
        "requests": request_summary([], [], "https://a.com/"),
        "metrics": {"jit": {"median": metrics}, "jitless": {"median": {}}},
        "js_off": {"elements_on": 10, "elements_off": 9, "diff_fraction": 0.015},
    }


def test_summarize_trace_and_markdown():
    survey = _survey()
    assert survey["trace"]["phase_ops"] == {"parse": 6, "load": 1, "after": 5}
    text = render_markdown("page", survey)
    assert text.startswith("## page")
    assert "`Node.appendChild`" in text
    assert "1.50%" in text
    assert "Compiled scripts: 1, 2 KB" in text


def test_js_fixture_paths(tmp_path, monkeypatch):
    monkeypatch.setenv("SWB_ROOT", str(tmp_path))
    assert paths.js_fixtures_dir() == tmp_path / "fixtures" / "js"
    assert paths.js_fixture_dir("x") == tmp_path / "fixtures" / "js" / "x"
    with pytest.raises(ValueError):
        paths.js_fixture_dir("../x")


def test_meta_javascript_field(tmp_path):
    plain = FixtureMeta("https://a/", "t", (1, 2), "c")
    write_meta(tmp_path, plain)
    assert '"javascript"' not in (tmp_path / "fixture.json").read_text()
    assert read_meta(tmp_path).javascript is False
    write_meta(tmp_path, FixtureMeta("https://a/", "t", (1, 2), "c", javascript=True))
    assert read_meta(tmp_path).javascript is True
