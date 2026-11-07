"""Chromium test of the js-survey tracer. Skipped without Chromium. No network:
the page is a local file."""

import asyncio

import pytest

from swbtools import browser
from swbtools.js_survey import harness, tracer_source
from swbtools.js_survey_data import summarize_trace

pytestmark = pytest.mark.usefixtures("chromium")

PAGE = """<!DOCTYPE html>
<html><head><title>t</title>
<script>
  document.write('<p id="written">hello</p>');
  var parsed = document.createElement('div');
  parsed.setAttribute('class', 'a');
  document.addEventListener('DOMContentLoaded', function () {
    document.querySelector('#written');
    setTimeout(function () { window.ran = (window.ran || 0) + 1; }, 0);
  });
</script></head>
<body>
<script>
  var el = document.createElement('span');
  el.id = 'x';
  document.body.appendChild(el);
  var inserted = document.createElement('script');
  inserted.src = 'https://cdn.example/a.js';
  inserted.async = true;
  document.body.appendChild(inserted);
  var ev = new Event('ping');
  window.addEventListener('ping', function () {});
  window.addEventListener('load', function () {
    window.rect = el.getBoundingClientRect();
    window.offset = el.offsetWidth;
    window.scrollTo(0, 0);
    el.innerHTML = '<b>bold</b>';
    el.onclick = function () {};
    setTimeout(function () { window.late = 1; }, 50);
  });
  window.probe = {
    isEvent: ev instanceof Event,
    ctor: ev.constructor === Event,
    text: Function.prototype.toString.call(Node.prototype.appendChild),
    wrapper: Function.prototype.toString.call(Function.prototype.toString),
    name: Node.prototype.appendChild.name,
    length: Document.prototype.createElement.length,
    same: el.appendChild === Node.prototype.appendChild,
  };
</script></body></html>
"""


async def run_page(tmp_path):
    path = tmp_path / "page.html"
    path.write_text(PAGE, encoding="utf-8")
    async with browser.session(browser.PAGE_VIEWPORT, javascript=True) as (_, context, page):
        await context.add_init_script(tracer_source())
        cdp = await context.new_cdp_session(page)
        await page.goto(path.as_uri(), wait_until="load")
        await page.wait_for_timeout(300)
        # The tool's own calls do not count.
        await harness(cdp, "window.scrollTo(0, 0)")
        probe = await harness(cdp, "window.probe")
        ran = await harness(cdp, "[window.ran, window.late]")
        raw = await harness(cdp, "window.__swb_trace__ ? window.__swb_trace__.flush() : null")
    return raw, probe, ran


def total(members, key, kind, phase):
    return members.get(key, {}).get(kind, {}).get(phase, 0)


def test_tracer_counts_phases_and_keeps_the_page_working(tmp_path):
    raw, probe, ran = asyncio.run(run_page(tmp_path))
    assert raw is not None
    trace = summarize_trace(raw)
    members = trace["members"]

    # The page works as without the tracer.
    assert probe["isEvent"] is True
    assert probe["ctor"] is True
    assert probe["same"] is True
    assert probe["name"] == "appendChild"
    assert probe["length"] == 1
    assert "[native code]" in probe["text"]
    assert "appendChild" in probe["text"]
    assert "[native code]" in probe["wrapper"]
    assert ran == [1, 1]

    # Calls by phase.
    assert total(members, "Document.createElement", "call", "parse") == 3
    assert total(members, "Document.write", "call", "parse") == 1
    assert total(members, "Document.querySelector", "call", "load") == 1
    assert total(members, "Node.appendChild", "call", "parse") == 2
    assert total(members, "Event", "construct", "parse") == 1
    # Getters and setters are counted separately.
    assert total(members, "Element.id", "set", "parse") == 1
    assert total(members, "Element.innerHTML", "set", "after") == 1
    assert total(members, "Element.getBoundingClientRect", "call", "after") == 1
    assert total(members, "HTMLElement.offsetWidth", "get", "after") == 1
    # The page's own scrollTo counts; the harness call does not.
    assert sum(members.get("window.scrollTo", {}).get("call", {}).values()) == 1

    # Arguments, sums, timers.
    args = trace["args"]
    assert {r["value"] for r in args["createElement"]} == {"span", "script", "div"}
    listeners = {r["value"] for r in args["addEventListener"]}
    assert {"ping @ window", "DOMContentLoaded @ document"} <= listeners
    assert "click @ span#x" in {r["value"] for r in args["on-handler"]}
    assert trace["sums"]["Document.write"]["total"] == len('<p id="written">hello</p>')
    assert trace["sums"]["Element.innerHTML"]["count"] == 1
    assert sum(t.get("timeout", 0) for t in trace["timers_run"].values()) == 2

    # Scripts inserted by scripts; mutations.
    assert trace["inserted_scripts"][0]["src"] == "https://cdn.example/a.js"
    assert trace["inserted_scripts"][0]["async"] is True
    assert trace["mutations"]["later"]["added_elements_total"] >= 1
    assert trace["tracer_errors"] == []


SMOOTH_PAGE = (
    "<!DOCTYPE html><style>html{scroll-behavior:smooth} body{margin:0;height:5000px}</style>"
    "<body>x</body>"
)


def test_scroll_through_ends_at_the_top_on_a_page_with_smooth_scrolling(tmp_path):
    """`html { scroll-behavior: smooth }` (GitHub) animates a plain
    `scrollTo(0, 0)`; the reference of github-swb was scrolled by 394 px."""

    async def run():
        path = tmp_path / "smooth.html"
        path.write_text(SMOOTH_PAGE, encoding="utf-8")
        async with browser.session((400, 300)) as (_, _, page):
            await page.goto(path.as_uri(), wait_until="load")
            await browser.scroll_through(page, pause_ms=20, max_steps=4)
            return await page.evaluate("window.scrollY")

    assert asyncio.run(run()) == 0
