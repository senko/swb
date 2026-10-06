"""The Python client of the automation protocol, against a headless swb that
serves the senko.net fixture (or a `data:` page). Skipped if the swb binary
does not exist."""

import io

import pytest
from PIL import Image

from swbtools import paths, swb
from swbtools.automation import AutomationError, Browser


@pytest.fixture(autouse=True)
def _require_swb():
    if swb.find_swb() is None:
        pytest.skip("the swb binary does not exist")


@pytest.fixture
def browser():
    with Browser.start(fixture=paths.fixture_dir("senko-net")) as b:
        b.navigate("https://senko.net/")
        assert b.wait_for_load()
        yield b


def test_inspect_and_screenshot(browser):
    assert browser.info()["title"] == "Senko's corner of the Web"
    heading = browser.query_selector("h2")
    assert browser.text(heading) == ":: Work"
    assert browser.box(heading)[2] > 0
    image = Image.open(io.BytesIO(browser.screenshot()))
    assert image.size == (1280, 800)


def test_keyboard_and_errors(browser):
    assert browser.key("Tab")
    assert browser.info()["focusedNode"] == browser.query_selector("main a")
    with pytest.raises(AutomationError) as error:
        browser.call("no.such.method")
    assert error.value.code == -32601


def test_scrolling_an_element():
    with Browser.start() as b:
        b.navigate(
            "data:text/html,<body style='margin:0'><div id=s style='overflow:auto;"
            "height:100px;width:200px'><div style='height:300px'></div></div>"
        )
        assert b.wait_for_load()
        s = b.query_selector("#s")
        info = b.scroll_info(s)
        assert info["scrollable"]
        assert info["clientHeight"] == 100.0
        assert info["scrollHeight"] == 300.0
        assert b.wheel(50, 50, 0, 40)
        assert b.scroll_info(s)["scroll"] == {"x": 0.0, "y": 40.0}
        assert b.scroll_element_to(s, 0, 1000)["scroll"]["y"] == 200.0
        assert b.info()["scroll"]["y"] == 0.0


def test_typing_into_a_text_field():
    with Browser.start() as b:
        b.navigate("data:text/html,<input id=q value=a><textarea id=t></textarea>")
        assert b.wait_for_load()
        field = b.query_selector("#q")
        b.click_node(field)
        b.type_text("bc")
        assert b.value(field) == "abc"
        assert b.value(b.query_selector("body")) is None
        b.click_node(b.query_selector("#t"))
        b.type_text("one\ntwo")
        assert b.value(b.query_selector("#t")) == "one\ntwo"
