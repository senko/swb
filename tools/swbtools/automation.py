"""Client for swb's automation protocol (docs/automation.md).

`Browser.start()` runs `swb --headless --remote-port 0` and connects to it:

    with Browser.start(fixture=paths.fixture_dir("senko-net")) as browser:
        browser.navigate("https://senko.net/")
        browser.wait_for_load()
        png = browser.screenshot()
"""

import base64
import itertools
import json
import logging
import re
import subprocess
import threading
from pathlib import Path
from types import TracebackType
from typing import Any, Self

from websockets.exceptions import ConnectionClosed
from websockets.sync.client import ClientConnection, connect

from swbtools import swb

log = logging.getLogger(__name__)

START_TIMEOUT_S = 30
"""How long to wait for swb to print the server address."""

_ADDRESS = re.compile(r"listening on (ws://\S+)")


class AutomationError(Exception):
    """The server answered a request with an error."""

    def __init__(self, code: int, message: str) -> None:
        super().__init__(f"{message} (error {code})")
        self.code = code
        self.message = message


class Browser:
    """A connection to an swb automation server, and optionally the swb
    process that serves it."""

    def __init__(self, url: str, process: subprocess.Popen[str] | None = None) -> None:
        # `legacy=True` returns the connection directly; `close()` closes
        # it. The server is local: no proxy, no compression. No keepalive
        # pings: the server answers pings only between requests, and a
        # request can take longer than the ping timeout (`waitForLoad`).
        self._connection: ClientConnection = connect(
            url, max_size=None, proxy=None, compression=None, ping_interval=None, legacy=True
        )
        self._process = process
        self._ids = itertools.count(1)

    @classmethod
    def start(
        cls,
        swb_path: Path | None = None,
        fixture: Path | None = None,
        viewport: tuple[int, int] = (1280, 800),
        test_fonts: bool = True,
    ) -> Self:
        """Starts a headless swb and connects to it. `fixture` serves all
        requests from a fixture directory."""
        binary = swb.find_swb(swb_path)
        if binary is None:
            raise FileNotFoundError("the swb binary does not exist; run `just build`")
        command = swb.serve_command(binary, viewport, fixture, test_fonts)
        log.info("running %s", " ".join(command))
        process = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
        try:
            return cls(_read_address(process), process)
        except BaseException:
            _stop(process)
            raise

    def call(self, method: str, **params: Any) -> Any:
        """Calls a method and returns its result. Raises AutomationError if
        the server answers with an error."""
        request_id = next(self._ids)
        self._connection.send(json.dumps({"id": request_id, "method": method, "params": params}))
        response = json.loads(self._connection.recv())
        if response.get("id") != request_id:
            raise RuntimeError(f"expected the response to request {request_id}: {response}")
        if "error" in response:
            error = response["error"]
            raise AutomationError(error["code"], error["message"])
        return response.get("result")

    def navigate(self, url: str) -> None:
        """Starts loading `url`."""
        self.call("page.navigate", url=url)

    def wait_for_load(self, timeout_ms: int = 30_000) -> bool:
        """Waits until the page is loaded. Returns False on timeout."""
        return bool(self.call("page.waitForLoad", timeoutMs=timeout_ms)["loaded"])

    def info(self) -> dict[str, Any]:
        """The page state: URL, title, load state, scroll position, ..."""
        return self.call("page.info")

    def query_selector_all(self, selector: str) -> list[int]:
        """The node IDs of the elements that match `selector`."""
        return self.call("dom.querySelectorAll", selector=selector)["nodeIds"]

    def query_selector(self, selector: str) -> int | None:
        """The node ID of the first element that matches `selector`."""
        return self.call("dom.querySelector", selector=selector)["nodeId"]

    def box(self, node_id: int) -> list[float] | None:
        """The border box `[x, y, width, height]` of a node in document
        coordinates, or None."""
        return self.call("dom.box", nodeId=node_id)["rect"]

    def text(self, node_id: int) -> str:
        """The rendered text of a node."""
        return self.call("dom.text", nodeId=node_id)["text"]

    def click(self, x: float, y: float, click_count: int = 1) -> None:
        """Clicks at a point in viewport coordinates."""
        self.call("input.click", x=x, y=y, clickCount=click_count)

    def click_node(self, node_id: int) -> None:
        """Clicks the center of a node's box (scrolled into view)."""
        self.call("input.click", nodeId=node_id)

    def mouse_move(self, x: float, y: float) -> None:
        """Moves the mouse pointer to a point in viewport coordinates."""
        self.call("input.mouseMove", x=x, y=y)

    def wheel(self, x: float, y: float, dx: float, dy: float) -> bool:
        """Turns the mouse wheel at a point in viewport coordinates: scrolls
        by (`dx`, `dy`) CSS px the innermost scroll container there that can
        scroll in that direction, else the page. Returns True if something
        scrolled."""
        return bool(self.call("input.wheel", x=x, y=y, dx=dx, dy=dy)["scrolled"])

    def scroll_info(self, node_id: int) -> dict[str, Any] | None:
        """The scroll state of an element: `scroll` (x, y), `scrollWidth`,
        `scrollHeight`, `clientWidth`, `clientHeight` and `scrollable`, or
        None if it has no box."""
        return self.call("dom.scrollInfo", nodeId=node_id)

    def scroll_element_to(self, node_id: int, x: float, y: float) -> dict[str, Any]:
        """Scrolls an element (a scroll container, or the element that
        scrolls the page: the root element, in quirks mode the body if it is
        not a scroll container) to an offset, clamped to its range. Returns
        its scroll state as `scroll_info` does."""
        return self.call("dom.scrollTo", nodeId=node_id, x=x, y=y)

    def key(self, key: str, modifiers: list[str] | None = None) -> bool:
        """Presses a key (a DOM key value such as "Tab" or "a"). Returns
        True if the page handled it."""
        return bool(self.call("input.key", key=key, modifiers=modifiers or [])["handled"])

    def type_text(self, text: str) -> None:
        """Types text into the focused text field or text area, as keyboard
        input would (a line break presses Enter, a tab presses Tab)."""
        self.call("input.type", text=text)

    def value(self, node_id: int) -> str | None:
        """The current value of a form control, or None for other nodes."""
        return self.call("dom.value", nodeId=node_id)["value"]

    def screenshot(self, full_page: bool = False) -> bytes:
        """A PNG screenshot of the viewport, or of the whole page."""
        return base64.b64decode(self.call("page.screenshot", fullPage=full_page)["png"])

    def close(self) -> None:
        """Closes the connection. If this object started swb, asks it to
        exit and waits for it."""
        if self._process is not None:
            try:
                self.call("browser.close")
            except ConnectionClosed:
                # swb exited; the reply can be lost when it exits quickly.
                pass
            finally:
                self._connection.close()
                try:
                    self._process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    _stop(self._process)
            return
        self._connection.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()


def _read_address(process: subprocess.Popen[str]) -> str:
    """Reads the server address from swb's first output lines."""
    found: list[str] = []

    def read() -> None:
        assert process.stdout is not None
        for line in process.stdout:
            match = _ADDRESS.search(line)
            if match:
                found.append(match.group(1))
                return

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    reader.join(START_TIMEOUT_S)
    if not found:
        raise RuntimeError("swb did not start the automation server")
    return found[0]


def _stop(process: subprocess.Popen[str]) -> None:
    """Kills swb and waits for it."""
    process.kill()
    process.wait()
