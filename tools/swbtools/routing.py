"""Request interception: serve responses from a fixture, or fetch them from
the network and record them.

This uses the Chrome DevTools Protocol `Fetch` domain directly, not
Playwright's `page.route`. Playwright calls a route handler only for the
first URL of a redirect chain; the browser then fetches the redirect target
from the network without interception. With `Fetch`, every hop of a chain
pauses, so each hop is recorded and replayed like any other response.
<https://chromedevtools.github.io/devtools-protocol/tot/Fetch/>
"""

import asyncio
import base64
import logging
from typing import Any

from playwright.async_api import BrowserContext, CDPSession, Error, Page

from swbtools.manifest import Entry, Fixture

log = logging.getLogger(__name__)

# Added to every served response. The fixture does not store CORS headers,
# so without this Chromium would block cross-origin fonts and other CORS
# requests in replay. swb does not check CORS.
EXTRA_HEADERS = (("access-control-allow-origin", "*"),)

# Request headers that are not forwarded when Chromium's requests are
# fetched from the network. The fetcher sets its own encoding (and decodes
# the body) and length.
_DROPPED_REQUEST_HEADERS = {"accept-encoding", "content-length", "host"}


def served_headers(entry: Entry) -> list[dict[str, str]]:
    """The headers that Chromium gets for a fixture entry: the stored headers
    and `EXTRA_HEADERS`, in CDP form."""
    return [{"name": name, "value": value} for name, value in (*entry.headers, *EXTRA_HEADERS)]


def _is_http(url: str) -> bool:
    return url.startswith(("http://", "https://"))


class Interceptor:
    """Pauses every request of a page and hands it to `handle`."""

    def __init__(self, fixture: Fixture) -> None:
        self.fixture = fixture
        self._tasks: set[asyncio.Task[None]] = set()
        self._session: CDPSession | None = None

    async def attach(self, context: BrowserContext, page: Page) -> None:
        """Starts interception. Call it before the first navigation."""
        session = await context.new_cdp_session(page)
        self._session = session
        session.on("Fetch.requestPaused", self._on_paused)
        await session.send("Fetch.enable", {"patterns": [{"urlPattern": "*"}]})

    def _on_paused(self, event: dict[str, Any]) -> None:
        task = asyncio.create_task(self._run(event))
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)

    async def _run(self, event: dict[str, Any]) -> None:
        try:
            await self.handle(event)
        except Error as error:
            # The page or browser closed while the request was pending.
            log.debug("request %s: %s", event["request"]["url"], error)

    async def handle(self, event: dict[str, Any]) -> None:
        raise NotImplementedError

    async def send(self, method: str, params: dict[str, Any]) -> None:
        assert self._session is not None, "attach() must run before requests pause"
        await self._session.send(method, params)

    async def fulfill(self, event: dict[str, Any], entry: Entry, body: bytes) -> None:
        await self.send(
            "Fetch.fulfillRequest",
            {
                "requestId": event["requestId"],
                "responseCode": entry.status,
                "responseHeaders": served_headers(entry),
                "body": base64.b64encode(body).decode("ascii"),
            },
        )

    async def fail(self, event: dict[str, Any]) -> None:
        params = {"requestId": event["requestId"], "errorReason": "Failed"}
        await self.send("Fetch.failRequest", params)

    async def pass_through(self, event: dict[str, Any]) -> None:
        await self.send("Fetch.continueRequest", {"requestId": event["requestId"]})


class Replayer(Interceptor):
    """Serves every request from the fixture. Fails requests for URLs that
    are not in it and remembers them in `missing`."""

    def __init__(self, fixture: Fixture) -> None:
        super().__init__(fixture)
        self.missing: list[str] = []

    async def handle(self, event: dict[str, Any]) -> None:
        request = event["request"]
        url, method = request["url"], request["method"]
        if not _is_http(url):
            await self.pass_through(event)
            return
        entry = self.fixture.get(url, method)
        if entry is None:
            log.warning("not in fixture: %s %s", method, url)
            self.missing.append(url)
            await self.fail(event)
            return
        await self.fulfill(event, entry, self.fixture.read_body(entry))


class Recorder(Interceptor):
    """Fetches every request from the network, records the response in the
    fixture and passes it to the page.

    Redirects are not followed by the fetch: each hop is recorded and given
    to the page, which then requests the next URL. With `replay_known`,
    requests whose URL is already in the fixture are served from it.

    Chromium gets the same headers as in replay (`served_headers`), so the
    capture and the replay see the same responses.
    """

    def __init__(self, fixture: Fixture, context: BrowserContext, replay_known: bool) -> None:
        super().__init__(fixture)
        self.context = context
        self.replay_known = replay_known
        self.recorded: list[str] = []
        self.failed: list[str] = []

    async def handle(self, event: dict[str, Any]) -> None:
        request = event["request"]
        url, method = request["url"], request["method"]
        if not _is_http(url):
            await self.pass_through(event)
            return
        if self.replay_known:
            entry = self.fixture.get(url, method)
            if entry is not None:
                await self.fulfill(event, entry, self.fixture.read_body(entry))
                return
        headers = {
            name: value
            for name, value in request.get("headers", {}).items()
            if name.lower() not in _DROPPED_REQUEST_HEADERS
        }
        try:
            response = await self.context.request.fetch(
                url,
                method=method,
                headers=headers,
                data=request.get("postData"),
                max_redirects=0,
                fail_on_status_code=False,
                timeout=60_000,
            )
            body = await response.body()
        except Error as error:
            log.warning("fetch failed: %s %s: %s", method, url, error)
            self.failed.append(url)
            await self.fail(event)
            return
        pairs = [(header["name"], header["value"]) for header in response.headers_array]
        entry = self.fixture.record(method, url, response.status, pairs, body)
        self.recorded.append(entry.url)
        log.info("%d %s %s (%d bytes)", response.status, method, url, len(body))
        await self.fulfill(event, entry, body)
