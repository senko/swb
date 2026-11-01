"""The local HTTP server for the probe cases with resources."""

import json
import logging
import mimetypes
import shutil
import threading
import time
from collections.abc import Sequence
from dataclasses import dataclass, replace
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from swbtools.probe_cases import Case, ProbeError, Resource

log = logging.getLogger(__name__)


@dataclass
class Route:
    """What the case server answers for one URL path."""

    resource: Resource
    """The resource, with `$ORIGIN` and `$OTHER` replaced."""
    body: bytes
    content_type: str


# The request headers that the log of the case server keeps (the others
# differ between browsers and say nothing about CORS).
_LOGGED_HEADERS = ("accept", "authorization", "content-type", "range")


class CaseServer:
    """A local HTTP server for the cases with resources: it serves a
    directory, answers each resource after its delay with its type, and
    forbids caching. It listens on two ports (two origins, `origin` and
    `other_origin`), answers every method, and logs the requests of each
    case folder. It runs on its own threads until `close`."""

    def __init__(self, directory: Path) -> None:
        self.routes: dict[str, Route] = {}
        """URL path to the answer; paths without a route are files."""
        self.requests: dict[str, list[dict[str, Any]]] = {}
        """Case folder to the log of its requests."""
        self._lock = threading.Lock()
        self._httpds = [self._listen(directory, name) for name in ("main", "other")]
        for httpd in self._httpds:
            httpd.daemon_threads = True
            threading.Thread(target=httpd.serve_forever, daemon=True).start()

    def _listen(self, directory: Path, which: str) -> ThreadingHTTPServer:
        server = self

        class Handler(SimpleHTTPRequestHandler):
            def __init__(self, *args: Any, **kwargs: Any) -> None:
                super().__init__(*args, directory=str(directory), **kwargs)

            def _body(self) -> bytes:
                length = int(self.headers.get("Content-Length") or 0)
                return self.rfile.read(length) if length else b""

            def _send(
                self, status: int, headers: Sequence[tuple[str, str]], body: bytes, head: bool
            ) -> None:
                self.send_response(status)
                for name, value in headers:
                    self.send_header(name, value)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                if not head:
                    self.wfile.write(body)

            def _serve(self) -> None:
                try:
                    self._answer()
                except (BrokenPipeError, ConnectionResetError):
                    # The browser gave up on the request (an abort).
                    log.debug("case server: client closed %s", self.path)

            def _answer(self) -> None:
                method = self.command
                path = self.path.split("?", 1)[0]
                body = self._body()
                folder = path.split("/")[1] if path.count("/") >= 2 else ""
                if path.endswith("/__log"):
                    with server._lock:
                        entries = list(server.requests.get(folder, []))
                    text = json.dumps(entries).encode()
                    self._send(200, [("Content-Type", "application/json")], text, method == "HEAD")
                    return
                if method == "GET" and path.endswith(".html"):
                    with server._lock:
                        server.requests[folder] = []
                elif path != "/favicon.ico":
                    server._log(folder, which, self, path, body)
                route = server.routes.get(path)
                if route is None:
                    if method in ("GET", "HEAD"):
                        super().do_GET() if method == "GET" else super().do_HEAD()
                    else:
                        self._send(404, [], b"", False)
                    return
                if route.resource.delay_ms:
                    time.sleep(route.resource.delay_ms / 1000)
                if method == "OPTIONS":
                    status, headers = server._options_answer(route.resource, self.headers)
                    self._send(status, headers, b"", False)
                    return
                headers = list(route.resource.headers)
                if not any(name.lower() == "content-type" for name, _ in headers):
                    headers.insert(0, ("Content-Type", route.content_type))
                self._send(route.resource.status, headers, route.body, method == "HEAD")

            do_GET = do_HEAD = do_POST = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = _serve

            def guess_type(self, path: Any) -> str:
                return mimetypes.guess_type(str(path))[0] or "application/octet-stream"

            def end_headers(self) -> None:
                self.send_header("Cache-Control", "no-store")
                super().end_headers()

            def log_message(self, format: str, *args: Any) -> None:
                log.debug("case server: " + format, *args)

        return ThreadingHTTPServer(("127.0.0.1", 0), Handler)

    def _log(
        self, folder: str, which: str, handler: SimpleHTTPRequestHandler, path: str, body: bytes
    ) -> None:
        headers = handler.headers
        kept = {
            name.lower(): value
            for name, value in sorted(headers.items(), key=lambda item: item[0].lower())
            if name.lower() in _LOGGED_HEADERS
            or name.lower().startswith(("x-", "access-control-request-"))
        }
        entry = {
            "method": handler.command,
            "path": path.removeprefix(f"/{folder}/"),
            "server": which,
            "origin": headers.get("Origin"),
            "cookie": headers.get("Cookie"),
            "headers": kept,
            "body": body.decode("latin-1")[:200] if body else None,
        }
        with self._lock:
            self.requests.setdefault(folder, []).append(entry)

    @staticmethod
    def _options_answer(resource: Resource, request: Any) -> tuple[int, list[tuple[str, str]]]:
        """The status and headers of the answer to an `OPTIONS` request."""
        options = resource.options
        if options == "none":
            return 404, []
        if options == "deny":
            return 200, []
        if options == "permissive" or "status" not in options:
            options = options if isinstance(options, dict) else {}
            origin = request.get("Origin") or "*"
            answer = [
                ("Access-Control-Allow-Origin", origin),
                ("Access-Control-Allow-Credentials", "true"),
                ("Vary", "Origin"),
            ]
            for asked, header in (
                ("Access-Control-Request-Method", "Access-Control-Allow-Methods"),
                ("Access-Control-Request-Headers", "Access-Control-Allow-Headers"),
            ):
                if request.get(asked):
                    answer.append((header, request.get(asked)))
            if "max_age" in options:
                answer.append(("Access-Control-Max-Age", str(options["max_age"])))
            return options.get("status", 204), answer
        answer = list(options.get("headers", ()))
        if "max_age" in options:
            answer.append(("Access-Control-Max-Age", str(options["max_age"])))
        return options["status"], answer

    @property
    def origin(self) -> str:
        """`http://127.0.0.1:PORT`, the origin of the page."""
        return f"http://127.0.0.1:{self._httpds[0].server_address[1]}"

    @property
    def other_origin(self) -> str:
        """The second origin: `http://127.0.0.1:PORT2`."""
        return f"http://127.0.0.1:{self._httpds[1].server_address[1]}"

    def substitute(self, text: str, folder: str = "") -> str:
        """Replaces `$ORIGIN`, `$OTHER` and `$FOLDER` (the name of the case
        folder, `case-N`) in a text."""
        return (
            text.replace("$ORIGIN", self.origin)
            .replace("$OTHER", self.other_origin)
            .replace("$FOLDER", folder)
        )

    def close(self) -> None:
        for httpd in self._httpds:
            httpd.shutdown()
            httpd.server_close()


_JAVASCRIPT_TYPE = "text/javascript"


def _resource_type(resource: Resource) -> str:
    """The `Content-Type` of a resource: its own, else from the extension."""
    if resource.content_type:
        return resource.content_type
    if resource.name.endswith((".js", ".mjs")):
        return _JAVASCRIPT_TYPE
    return mimetypes.guess_type(resource.name)[0] or "text/plain"


def _write_case_files(case: Case, directory: Path) -> None:
    """Copies the `files` of a case into `directory`."""
    for name, source in case.files:
        try:
            shutil.copyfile(source, directory / name)
        except OSError as error:
            raise ProbeError(f"case {case.name!r}: cannot copy {source}: {error}") from error


def _serve_case(case: Case, number: int, directory: Path, server: CaseServer) -> tuple[Path, str]:
    """Writes the page and the resources of a case into its own directory
    below the server's and returns the page's path and URL."""
    folder = f"case-{number}"
    case_dir = directory / folder
    case_dir.mkdir(exist_ok=True)
    _write_case_files(case, case_dir)
    for resource in case.resources:
        text = server.substitute(resource.text, folder)
        (case_dir / resource.name).write_text(text, encoding="utf-8")
        headers = tuple((n, server.substitute(v, folder)) for n, v in resource.headers)
        options = resource.options
        if isinstance(options, dict) and "headers" in options:
            options = {
                **options,
                "headers": tuple((n, server.substitute(v, folder)) for n, v in options["headers"]),
            }
        replaced = replace(resource, text=text, headers=headers, options=options)
        server.routes[f"/{folder}/{resource.name}"] = Route(
            replaced, text.encode("utf-8"), _resource_type(resource)
        )
    page_path = case_dir / f"case-{number}.html"
    page_path.write_text(server.substitute(case.html, folder), encoding="utf-8")
    return page_path, f"{server.origin}/{folder}/{page_path.name}"
