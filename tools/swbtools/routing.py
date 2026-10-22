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

# Added to every served response whose request has no `Origin` header. The
# fixture does not store CORS headers, so without this Chromium would block
# cross-origin fonts and other CORS requests in replay. A request with an
# `Origin` header (a CORS-mode request) gets `cors_headers` instead.
EXTRA_HEADERS = (("access-control-allow-origin", "*"),)

# Request headers that are not forwarded when Chromium's requests are
# fetched from the network. The fetcher sets its own encoding (and decodes
# the body) and length.
_DROPPED_REQUEST_HEADERS = {"accept-encoding", "content-length", "host"}


def header_value(headers: dict[str, str], name: str) -> str | None:
    """Returns the value of a request header (CDP gives a dict with the
    case of the sender), or None."""
    lowered = name.lower()
    for key, value in headers.items():
        if key.lower() == lowered:
            return value
    return None


def cors_headers(origin: str | None, entry: Entry) -> tuple[tuple[str, str], ...]:
    """The CORS headers for a served response (ADR 0028 section 12). With
    the request's `Origin`: that origin, credentials allowed, `Vary: Origin`
    and the names of the stored headers as exposed headers. Without it: `*`."""
    if origin is None:
        return EXTRA_HEADERS
    headers = [
        ("access-control-allow-origin", origin),
        ("access-control-allow-credentials", "true"),
        ("vary", "Origin"),
    ]
    if entry.headers:
        names = ", ".join(name for name, _ in entry.headers)
        headers.append(("access-control-expose-headers", names))
    return tuple(headers)


def served_headers(entry: Entry, origin: str | None = None) -> list[dict[str, str]]:
    """The headers that Chromium gets for a fixture entry: the stored headers
    and the CORS headers for a request with the given `Origin`, in CDP
    form."""
    extra = cors_headers(origin, entry)
    # The tool owns the CORS headers. A fixture that stores some (the local
    # craigslist-search fixture has two, added by hand) would otherwise give
    # `Access-Control-Allow-Origin` twice, which Chromium rejects.
    stored = [(n, v) for n, v in entry.headers if not n.lower().startswith("access-control-")]
    return [{"name": name, "value": value} for name, value in (*stored, *extra)]


def is_preflight(request: dict[str, Any]) -> bool:
    """True for a CORS preflight request: `OPTIONS` with
    `Access-Control-Request-Method`."""
    headers = request.get("headers", {})
    return (
        request["method"] == "OPTIONS"
        and header_value(headers, "access-control-request-method") is not None
    )


def preflight_headers(request: dict[str, Any]) -> list[dict[str, str]]:
    """The headers of the permissive answer to a preflight: the requested
    origin, method and headers are allowed, with credentials."""
    headers = request.get("headers", {})
    origin = header_value(headers, "origin") or "*"
    answer = [
        ("access-control-allow-origin", origin),
        ("access-control-allow-credentials", "true"),
        ("access-control-allow-methods", header_value(headers, "access-control-request-method")),
        ("access-control-allow-headers", header_value(headers, "access-control-request-headers")),
        ("vary", "Origin"),
    ]
    return [{"name": name, "value": value} for name, value in answer if value]


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
            request = event["request"]
            if _is_http(request["url"]) and is_preflight(request):
                # The fixtures store no preflights, and a preflight never
                # goes to the network or into a recording.
                await self.answer_preflight(event)
            else:
                await self.handle(event)
        except Error as error:
            # The page or browser closed while the request was pending.
            log.debug("request %s: %s", event["request"]["url"], error)

    async def handle(self, event: dict[str, Any]) -> None:
        raise NotImplementedError

    async def send(self, method: str, params: dict[str, Any]) -> None:
        assert self._session is not None, "attach() must run before requests pause"
        await self._session.send(method, params)

    async def answer_preflight(self, event: dict[str, Any]) -> None:
        """Answers a CORS preflight locally with 204 and a permissive
        response."""
        log.debug("preflight %s", event["request"]["url"])
        await self.send(
            "Fetch.fulfillRequest",
            {
                "requestId": event["requestId"],
                "responseCode": 204,
                "responseHeaders": preflight_headers(event["request"]),
            },
        )

    async def fulfill(self, event: dict[str, Any], entry: Entry, body: bytes) -> None:
        origin = header_value(event["request"].get("headers", {}), "origin")
        await self.send(
            "Fetch.fulfillRequest",
            {
                "requestId": event["requestId"],
                "responseCode": entry.status,
                "responseHeaders": served_headers(entry, origin),
                "body": base64.b64encode(body).decode("ascii"),
            },
        )

    async def fail(self, event: dict[str, Any]) -> None:
        params = {"requestId": event["requestId"], "errorReason": "Failed"}
        await self.send("Fetch.failRequest", params)

    async def pass_through(self, event: dict[str, Any]) -> None:
        await self.send("Fetch.continueRequest", {"requestId": event["requestId"]})


class Replayer(Interceptor):
    """Serves every request from the fixture. A URL that is not in it is
    served by a replay pattern of the fixture (`Fixture.lookup`) or fails; the
    failed URLs are in `missing`."""

    def __init__(self, fixture: Fixture) -> None:
        super().__init__(fixture)
        self.missing: list[str] = []

    async def handle(self, event: dict[str, Any]) -> None:
        request = event["request"]
        url, method = request["url"], request["method"]
        if not _is_http(url):
            await self.pass_through(event)
            return
        entry = self.fixture.lookup(url, method)
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
