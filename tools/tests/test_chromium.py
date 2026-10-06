"""Tests that run Chromium. Skipped when Playwright's Chromium is missing.

They do not use the network: pages come from files, from a fixture through
request interception, or from an HTTP server on 127.0.0.1.
"""

import asyncio
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from PIL import Image

from swbtools.boxes import read_dump
from swbtools.capture import chromium_pass
from swbtools.layout_refs import boxes_path, dump_layout_tests
from swbtools.manifest import Fixture, read_manifest
from swbtools.reference import render_fixture

pytestmark = pytest.mark.usefixtures("chromium")

LAYOUT_HTML = """<!DOCTYPE html>
<html><head><style>
  body { margin: 8px; }
  .box { width: 100px; height: 50px; margin: 10px; }
  span { font: 16px Arial; }
  .serif { font: 13px serif; }
  .missing { font: 15px "No Such Family"; }
</style></head>
<body><div class="box"></div><span>Hello world, this is a test of text width</span><br>
<span class="serif">Hello world, this is a test of text width</span><br>
<span class="missing">Hello world iiiii</span></body></html>
"""


def test_layout_dump_uses_the_bundled_fonts(tmp_path):
    html = tmp_path / "test.html"
    html.write_text(LAYOUT_HTML)
    asyncio.run(dump_layout_tests([html], system_fonts=False))
    dump = read_dump(boxes_path(html))
    assert dump.viewport == (800, 600)
    assert dump.url == html.resolve().as_uri()
    tags = [element.tag for element in dump.elements]
    assert tags == ["html", "head", "style", "body", "div", "span", "br", "span", "br", "span"]
    div = dump.elements[4]
    # The body's top margin collapses with the div's.
    assert div.rect == (18.0, 10.0, 100.0, 50.0)
    assert div.parent == 3
    # Unhinted advances: Arial maps to Liberation Sans (metric-compatible).
    assert dump.elements[5].rect[2] == 262.33
    assert dump.elements[7].rect[2] == 199.67
    # A family that does not exist falls back to Chromium's standard font,
    # which is serif (Times New Roman, here Liberation Serif).
    assert dump.elements[9].rect[2] == 96.67


def write_fixture(tmp_path):
    fixture = Fixture(tmp_path / "fixture")
    html = "text/html; charset=utf-8"
    fixture.record("GET", "https://fixture.test/old", 301, [("location", "/")], b"")
    fixture.record(
        "GET",
        "https://fixture.test/",
        200,
        [("content-type", html)],
        b"<!DOCTYPE html><link rel=stylesheet href=style.css>"
        b"<div id=a>styled</div><img src=missing.png>",
    )
    fixture.record(
        "GET",
        "https://fixture.test/style.css",
        200,
        [("content-type", "text/css")],
        b"body { margin: 0 } #a { width: 123px; height: 45px; background: rgb(0, 128, 0) }",
    )
    fixture.save()
    return fixture.path


def test_reference_replays_a_fixture_with_redirects(tmp_path):
    fixture = write_fixture(tmp_path)
    output = tmp_path / "reference"
    missing = asyncio.run(render_fixture(fixture, "https://fixture.test/old", output, (640, 480)))
    assert missing == ["https://fixture.test/missing.png"]
    dump = read_dump(output / "boxes.json")
    assert dump.url == "https://fixture.test/old"
    div = next(element for element in dump.elements if element.tag == "div")
    assert div.rect == (0.0, 0.0, 123.0, 45.0)
    with Image.open(output / "screenshot.png") as screenshot:
        assert screenshot.size == (640, 480)
        assert screenshot.convert("RGB").getpixel((10, 40)) == (0, 128, 0)


class _Site(BaseHTTPRequestHandler):
    """/ redirects to /final, which uses /old.css, which redirects to /new.css."""

    def do_GET(self) -> None:
        port = self.server.server_address[1]
        routes = {
            "/": (301, [("Location", "/final")], b""),
            "/final": (
                200,
                [("Content-Type", "text/html")],
                b"<!DOCTYPE html><link rel=stylesheet href=/old.css><p>hi",
            ),
            "/old.css": (302, [("Location", f"http://127.0.0.1:{port}/new.css")], b""),
            "/new.css": (200, [("Content-Type", "text/css"), ("X-Other", "1")], b"p { margin: 0 }"),
        }
        status, headers, body = routes.get(self.path, (404, [], b""))
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        pass


@pytest.fixture
def site() -> Iterator[str]:
    server = ThreadingHTTPServer(("127.0.0.1", 0), _Site)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_address[1]}"
    server.shutdown()


def test_capture_records_every_redirect_hop(tmp_path, site):
    fixture = Fixture(tmp_path)
    first_url, version = asyncio.run(chromium_pass(fixture, f"{site}/", False, False))
    assert first_url == f"{site}/"
    assert version
    entries = {entry.url: entry for entry in read_manifest(tmp_path)}
    assert sorted(entries) == [f"{site}/", f"{site}/final", f"{site}/new.css", f"{site}/old.css"]
    assert entries[f"{site}/"].status == 301
    assert entries[f"{site}/"].headers == (("location", "/final"),)
    assert entries[f"{site}/old.css"].headers == (("location", f"{site}/new.css"),)
    assert entries[f"{site}/new.css"].headers == (("content-type", "text/css"),)
    assert fixture.read_body(entries[f"{site}/new.css"]) == b"p { margin: 0 }"


def test_layout_dump_ignores_css_animations(tmp_path):
    html = tmp_path / "animated.html"
    html.write_text(
        "<!DOCTYPE html><style>@keyframes grow { from { width: 10px } to { width: 500px } }"
        "#a { width: 100px; height: 10px; animation: grow 10s linear infinite }</style>"
        "<div id=a></div>"
    )
    asyncio.run(dump_layout_tests([html], system_fonts=False))
    dump = read_dump(boxes_path(html))
    assert [element.tag for element in dump.elements] == ["html", "head", "style", "body", "div"]
    assert dump.elements[4].rect == (8.0, 8.0, 100.0, 10.0)


def test_layout_dump_scrolls_elements_with_data_scroll(tmp_path):
    html = tmp_path / "scrolled.html"
    html.write_text(
        "<!DOCTYPE html><style>body { margin: 0 }</style>"
        "<div data-scroll='5 9999' style='overflow: auto; width: 100px; height: 100px'>"
        "<div style='width: 120px; height: 300px'></div></div>"
    )
    asyncio.run(dump_layout_tests([html], system_fonts=False))
    dump = read_dump(boxes_path(html))
    # The inner box moves by the offset, clamped to the range (20, 200).
    assert dump.elements[5].rect == (-5.0, -200.0, 120.0, 300.0)
