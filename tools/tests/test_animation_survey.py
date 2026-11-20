"""Tests of `swbtools animation-survey` and `animation-cpu` (M13 feature 1)."""

import asyncio

import pytest

from swbtools import browser
from swbtools.animation_survey import SYNTHETIC_PAGES, measure_cpu, summarize, survey_page

PAGE = """<!DOCTYPE html><style>
body{margin:0}
#a{width:20px;height:20px;background:#000;animation:k 1s linear infinite}
#b{width:20px;height:20px;background:#000;animation:k 100ms linear 1}
#c{width:20px;height:20px;background:#000;transition:opacity 5s linear}
#c.on{opacity:.5}
@keyframes k{to{opacity:.5}}</style>
<div id=a></div><div id=b></div><div id=c></div>
<script>document.getElementById('c').getBoundingClientRect();
document.getElementById('c').className = 'on';</script>"""


def test_summarize_counts_the_kinds_and_the_viewport():
    result = {
        "elements": 10,
        "animations": [
            {"type": "CSSAnimation", "iterations": "infinite", "inViewport": True},
            {"type": "CSSAnimation", "iterations": 1, "inViewport": False},
            {"type": "CSSTransition", "iterations": 1, "inViewport": True},
        ],
        "later": 2,
        "declared": {"animation": 2, "transition": 1},
        "events": {"counts": {"animationstart": 2}, "names": {"k": 2}},
    }
    summary = summarize(result)
    assert summary["running"] == 3
    assert summary["types"] == {"CSSAnimation": 2, "CSSTransition": 1}
    assert (summary["in_view"], summary["infinite"], summary["gone_after_2s"]) == (2, 1, 1)
    assert summary["started"] == {"k": 2}


@pytest.mark.usefixtures("chromium")
def test_survey_page_lists_running_animations_and_events(tmp_path):
    path = tmp_path / "page.html"
    path.write_text(PAGE, encoding="utf-8")

    async def run():
        async with browser.session((300, 200), javascript=True) as (_, _, page):
            return await survey_page(page, path.as_uri(), True)

    result = asyncio.run(run())
    kinds = sorted(a["type"] + ":" + a["name"] for a in result["animations"])
    assert kinds == ["CSSAnimation:k", "CSSTransition:opacity"]
    assert result["later"] == 2
    assert result["declared"]["animation"] == 2
    assert result["declared"]["transition"] == 1
    assert result["events"]["counts"]["animationstart"] == 2
    assert result["events"]["counts"]["transitionrun"] == 1
    assert result["events"]["counts"]["animationend"] == 1


@pytest.mark.usefixtures("chromium")
def test_measure_cpu_of_the_synthetic_pages():
    assert set(SYNTHETIC_PAGES) == {"spinner", "marquee", "idle"}
    spinner = asyncio.run(measure_cpu("spinner", False, 0, 1.0, (400, 300)))
    assert spinner["running_animations"] == 8
    idle = asyncio.run(measure_cpu("idle", False, 0, 1.0, (400, 300)))
    assert idle["running_animations"] == 0
    assert idle["cpu_total"] < 50
