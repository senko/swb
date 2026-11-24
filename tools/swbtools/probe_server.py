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
        self.media_folders: set[str] = set()
        """The folders of the cases with `http: true`: their files are served
        with `Range` support and the log has `status` and `sent`."""
        self.ranges: dict[str, bool] = {}
        """Case folder to whether the server honors `Range` there."""
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

            def _send_file(self, folder: str, entry: dict[str, Any] | None, head: bool) -> None:
                """Answers a file of a media case (`http: true`): `Range`
                requests get 206 (unless the case has `ranges: false`), the
                log entry gets the `status`, the `sent` body bytes and the
                `content_range`."""
                file = Path(self.translate_path(self.path))
                if not file.is_file():
                    self._send(404, [], b"", head)
                    if entry is not None:
                        with server._lock:
                            entry.update(status=404, sent=0)
                    return
                size = file.stat().st_size
                ranged = server.ranges.get(folder, True)
                spec = _range_of(self.headers.get("Range"), size) if ranged else "full"
                headers = [("Content-Type", self.guess_type(str(file)))]
                if ranged:
                    headers.append(("Accept-Ranges", "bytes"))
                if spec is None:
                    status, start, end = 416, 0, -1
                    headers.append(("Content-Range", f"bytes */{size}"))
                elif spec == "full":
                    status, start, end = 200, 0, size - 1
                else:
                    status, (start, end) = 206, spec
                    headers.append(("Content-Range", f"bytes {start}-{end}/{size}"))
                length = max(0, end - start + 1)
                self.send_response(status)
                for name, value in headers:
                    self.send_header(name, value)
                self.send_header("Content-Length", str(length))
                self.end_headers()
                sent = 0
                try:
                    if not head and length:
                        with file.open("rb") as stream:
                            stream.seek(start)
                            while sent < length:
                                chunk = stream.read(min(1 << 20, length - sent))
                                if not chunk:
                                    break
                                self.wfile.write(chunk)
                                sent += len(chunk)
                finally:
                    if entry is not None:
                        # `__log` serializes the entries under the lock.
                        with server._lock:
                            entry.update(
                                status=status,
                                sent=sent,
                                content_range=dict(headers).get("Content-Range"),
                            )

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
                        text = json.dumps(server.requests.get(folder, [])).encode()
                    self._send(200, [("Content-Type", "application/json")], text, method == "HEAD")
                    return
                entry = None
                if method == "GET" and path.endswith(".html"):
                    with server._lock:
                        server.requests[folder] = []
                elif path != "/favicon.ico":
                    entry = server._log(folder, which, self, path, body)
                route = server.routes.get(path)
                if route is None:
                    if method in ("GET", "HEAD") and folder in server.media_folders:
                        self._send_file(folder, entry, method == "HEAD")
                    elif method in ("GET", "HEAD"):
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
    ) -> dict[str, Any]:
        """Logs a request and returns its entry (the server may add fields
        to it after the answer)."""
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
        return entry

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
_LINK_ABOVE = 8 << 20
"""Case files above this size (bytes) are linked into the case folder, not
copied."""


def _range_of(header: str | None, size: int) -> tuple[int, int] | str | None:
    """The byte range of a `Range` header for a file of `size` bytes
    (RFC 9110 section 14): `(first, last)`; `None` if it cannot be satisfied;
    `"full"` if there is no header or it is not a single `bytes` range (a
    server ignores such a header)."""
    if not header or not header.startswith("bytes=") or "," in header:
        return "full"
    first, _, last = header.removeprefix("bytes=").strip().partition("-")
    try:
        if first == "":
            count = int(last)
            if count <= 0 or size == 0:
                return None
            return max(0, size - count), size - 1
        start = int(first)
        end = int(last) if last else size - 1
    except ValueError:
        return "full"
    if last and end < start:
        # An invalid range-spec (section 14.1.1): the header is ignored.
        return "full"
    if start >= size:
        return None
    return start, min(end, size - 1)


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
        target = directory / name
        try:
            if source.stat().st_size > _LINK_ABOVE:
                # A real media file can be 160 MB: link it, do not copy it.
                target.unlink(missing_ok=True)
                target.symlink_to(source.resolve())
            else:
                shutil.copyfile(source, target)
        except OSError as error:
            raise ProbeError(f"case {case.name!r}: cannot copy {source}: {error}") from error


def _serve_case(case: Case, number: int, directory: Path, server: CaseServer) -> tuple[Path, str]:
    """Writes the page and the resources of a case into its own directory
    below the server's and returns the page's path and URL."""
    folder = f"case-{number}"
    case_dir = directory / folder
    case_dir.mkdir(exist_ok=True)
    _write_case_files(case, case_dir)
    if case.http:
        server.media_folders.add(folder)
    server.ranges[folder] = case.ranges
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
