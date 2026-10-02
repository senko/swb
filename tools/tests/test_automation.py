"""The Python client of the automation protocol, against a headless swb that
serves the senko.net fixture. Skipped if the swb binary does not exist."""

import io

import pytest
from PIL import Image

from swbtools import paths, swb
from swbtools.automation import AutomationError, Browser


@pytest.fixture
def browser():
    if swb.find_swb() is None:
        pytest.skip("the swb binary does not exist")
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
