"""`swbtools capture`: the URL fragment, and step groups that run during a
JavaScript capture (docs/testing.md, "Capture with step groups"). No network:
a local server plays the site (a session cookie, a token that the server
checks, a button that sends a `POST` after a click)."""

import asyncio
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import ClassVar

import pytest

from swbtools import browser, cli
from swbtools.capture import (
    StepGroup,
    chromium_pass,
    count_hosts,
    read_step_groups,
    with_fragment,
)
from swbtools.manifest import Fixture
from swbtools.probe import run_steps
from swbtools.routing import Replayer

PAGE = b"""<!DOCTYPE html><meta name=csrf content=tok1>
<button id=b1>one</button><button id=b2>two</button><button id=b3>static</button>
<p id=out>none</p><p id=hash></p>
<script>
document.getElementById('hash').textContent = location.hash;
document.getElementById('b3').onclick = () =>
  fetch('/static').then((r) => r.text()).then((t) => {
    document.getElementById('out').textContent = t;
  });
for (const [id, body] of [['b1', 'q=1'], ['b2', 'q=2']]) {
  document.getElementById(id).onclick = () =>
    fetch('/api', {method: 'POST', body, headers: {'X-Token': 'tok1'}})
      .then((r) => r.text().then((t) => r.status + ' ' + t), () => 'failed')
      .then((t) => { document.getElementById('out').textContent = t; });
}
</script>
"""


class _Site(BaseHTTPRequestHandler):
    """`/` sets a session cookie; `/api` answers a `POST` only with that
    cookie and the page's token, and echoes the body."""

    seen: ClassVar[list[tuple[str, str, int]]] = []

    def _send(self, status: int, body: bytes, extra: dict[str, str] | None = None) -> None:
        self.send_response(status)
        self.send_header("Content-Type", "text/html" if body.startswith(b"<!") else "text/plain")
        self.send_header("Content-Length", str(len(body)))
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path == "/static":
            self.seen.append(("GET", self.path, 200))
            self._send(200, b"static")
        elif self.path == "/start":
            self.seen.append(("GET", self.path, 302))
            self._send(302, b"", {"Location": "/"})
        else:
            self.seen.append(("GET", self.path, 200))
            self._send(200, PAGE, {"Set-Cookie": "sid=abc; Path=/"})

    def do_POST(self) -> None:
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        cookie_ok = "sid=abc" in (self.headers.get("Cookie") or "")
        token_ok = self.headers.get("X-Token") == "tok1"
        status = 200 if cookie_ok and token_ok else 403
        self.seen.append(("POST", self.path, status))
        self._send(status, b"answer:" + body if status == 200 else b"rejected")

    def log_message(self, format: str, *args: object) -> None:
        pass


@pytest.fixture
def site() -> Iterator[tuple[str, list[tuple[str, str, int]]]]:
    seen: list[tuple[str, str, int]] = []
    handler = type("Site", (_Site,), {"seen": seen})
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{server.server_address[1]}", seen
    server.shutdown()


GROUPS = [
    StepGroup("first", ({"click": "#b1"}, {"wait": 100})),
    StepGroup("second", ({"click": "#b2"}, {"wait": 100})),
]


@pytest.mark.parametrize(
    "first, given, expected",
    [
        ("https://a.test/?x=1", "https://a.test/?x=1#map=16/1/2", "https://a.test/?x=1#map=16/1/2"),
        ("https://a.test/", "https://a.test/", "https://a.test/"),
        ("https://a.test/", "https://a.test/#", "https://a.test/"),
        ("https://a.test/b", "https://a.test/a#x#y", "https://a.test/b#x#y"),
    ],
)
def test_with_fragment(first, given, expected):
    assert with_fragment(first, given) == expected


def test_count_hosts_is_sorted_by_count():
    urls = ["https://b.test/1", "https://a.test/1", "https://b.test/2", "http://c.test:8/x"]
    assert count_hosts(urls) == "b.test=2, a.test=1, c.test:8=1"


def test_read_step_groups_gives_names(tmp_path):
    path = tmp_path / "steps.json"
    path.write_text('[{"name": "open", "steps": [{"wait": 1}]}, [{"key": "a"}]]')
    assert read_step_groups(path) == [
        StepGroup("open", ({"wait": 1},)),
        StepGroup("group 2", ({"key": "a"},)),
    ]


def test_steps_need_javascript(tmp_path):
    steps = tmp_path / "steps.json"
    steps.write_text("[]")
    argv = ["capture", "https://a.test/", "a", "--steps", str(steps)]
    args = cli.build_parser().parse_args(argv)
    assert args.steps == steps and args.max_responses is None
    assert cli.cmd_capture(args) == 2


@pytest.mark.usefixtures("chromium")
def test_the_first_url_keeps_the_fragment_over_a_redirect(tmp_path, site):
    base, _ = site
    fixture = Fixture(tmp_path)
    first_url, _ = asyncio.run(
        chromium_pass(fixture, f"{base}/start#map=16/45.8/15.9", False, True, javascript=True)
    )
    assert first_url == f"{base}/start#map=16/45.8/15.9"
    # The manifest has no fragments.
    assert all("#" not in url for url, _, _ in fixture.entries)


@pytest.mark.usefixtures("chromium")
def test_steps_are_recorded_with_the_session_and_replayed(tmp_path, site):
    base, seen = site
    fixture = Fixture(tmp_path)
    asyncio.run(
        chromium_pass(
            fixture, f"{base}/#h", False, True, javascript=True, groups=GROUPS, pause_ms=50
        )
    )
    # The cookie of the page's response reached the `POST`s of the same
    # capture, and the page's own token did too: both were accepted.
    assert [(m, s) for m, _, s in seen] == [("GET", 200), ("POST", 200), ("POST", 200)]
    posts = [entry for key, entry in fixture.entries.items() if key[1] == "POST"]
    assert len(posts) == 2 and all(entry.request_body for entry in posts)
    assert {entry.status for entry in posts} == {200}

    async def replay() -> list[str]:
        replayer = Replayer(fixture)
        texts = []
        async with browser.session((400, 300), javascript=True) as (_, context, page):
            await replayer.attach(context, page)
            await page.goto(f"{base}/#h", wait_until="load")
            for group in GROUPS:
                await run_steps(page, group.steps, 50)
                texts.append(await page.inner_text("#out"))
            assert await page.inner_text("#hash") == "#h"
        assert replayer.missing == []
        return texts

    count = len(seen)
    # Each click gets the answer of its own body, and the server is not used.
    assert asyncio.run(replay()) == ["200 answer:q=1", "200 answer:q=2"]
    assert len(seen) == count


@pytest.mark.usefixtures("chromium")
def test_the_response_limit_fails_new_requests(tmp_path, site):
    base, seen = site
    fixture = Fixture(tmp_path)
    asyncio.run(
        chromium_pass(
            fixture,
            f"{base}/",
            False,
            True,
            javascript=True,
            groups=GROUPS,
            pause_ms=50,
            max_responses=2,
        )
    )
    # The page and the first `POST` were fetched; the second `POST` was not.
    assert [(m, s) for m, _, s in seen] == [("GET", 200), ("POST", 200)]
    assert len(fixture.entries) == 2


@pytest.mark.usefixtures("chromium")
def test_a_failing_group_does_not_stop_the_capture(tmp_path, site, caplog):
    base, seen = site
    groups = [StepGroup("broken", ({"eval": "throw new Error('no such control')"},)), GROUPS[0]]
    asyncio.run(
        chromium_pass(
            Fixture(tmp_path), f"{base}/", False, True, javascript=True, groups=groups, pause_ms=50
        )
    )
    assert [(m, s) for m, _, s in seen] == [("GET", 200), ("POST", 200)]
    assert "group 1 (broken) failed" in caplog.text
    assert "1 of 2 groups failed" in caplog.text


@pytest.mark.usefixtures("chromium")
def test_a_repeated_get_is_served_from_the_first_response(tmp_path, site):
    base, seen = site
    click = StepGroup("static", ({"click": "#b3"}, {"wait": 100}))
    asyncio.run(
        chromium_pass(
            Fixture(tmp_path),
            f"{base}/",
            False,
            True,
            javascript=True,
            groups=[click, click, click],
            pause_ms=50,
        )
    )
    assert [path for _, path, _ in seen if path == "/static"] == ["/static"]
