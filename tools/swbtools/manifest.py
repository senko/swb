"""Fixture directories: `manifest.json` and `files/`.

The format specification is the module documentation of
`crates/net/src/fixture/mod.rs`. This module writes byte-identical files:
the same entry order, the same JSON formatting and the same body file names.
"""

import hashlib
import json
import os
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from swbtools.mime import extract_essence

MANIFEST_FILE = "manifest.json"
FILES_DIR = "files"
MANIFEST_VERSION = 1
STORED_HEADERS = ("content-type", "location")

# Content type essence to body file extension. Same table as
# `crates/net/src/file_types.rs`. Order matters: for a MIME type with two
# extensions, the first one wins.
_EXTENSIONS: tuple[tuple[str, str], ...] = (
    ("html", "text/html"),
    ("htm", "text/html"),
    ("xhtml", "application/xhtml+xml"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("json", "application/json"),
    ("txt", "text/plain"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("avif", "image/avif"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
)
_ALIASES: dict[str, str] = {
    "application/javascript": "js",
    "application/x-javascript": "js",
    "image/vnd.microsoft.icon": "ico",
    "application/font-woff": "woff",
    "application/x-font-woff": "woff",
    "application/font-woff2": "woff2",
    "application/x-font-ttf": "ttf",
    "application/x-font-otf": "otf",
}


def extension_for_essence(essence: str | None) -> str:
    """Returns the body file extension for a content type essence."""
    if essence is None:
        return "bin"
    for extension, mime in _EXTENSIONS:
        if mime == essence:
            return extension
    return _ALIASES.get(essence, "bin")


@dataclass(frozen=True)
class Entry:
    """One manifest entry. The field order is the order in the file."""

    method: str
    url: str
    status: int
    headers: tuple[tuple[str, str], ...]
    body: str

    @property
    def key(self) -> tuple[str, str]:
        """The sort key and identity of the entry: (url, method)."""
        return (self.url, self.method)

    def to_json(self) -> dict[str, object]:
        """The entry as a JSON object, fields in file order."""
        return {
            "method": self.method,
            "url": self.url,
            "status": self.status,
            "headers": [list(pair) for pair in self.headers],
            "body": self.body,
        }

    @staticmethod
    def from_json(data: dict[str, Any]) -> "Entry":
        """Reads an entry object of the manifest. Ignores unknown fields."""
        headers, status = data["headers"], data["status"]
        if not isinstance(headers, list) or not isinstance(status, int):
            raise ValueError(f"invalid entry {data!r}")
        return Entry(
            method=str(data["method"]),
            url=str(data["url"]),
            status=status,
            headers=tuple((str(name), str(value)) for name, value in headers),
            body=str(data["body"]),
        )


def entry_url(url: str) -> str:
    """Returns the `url` value of an entry: the request URL without the
    fragment. The URL must already be serialized (browsers give serialized
    URLs)."""
    return url.split("#", 1)[0]


def stored_headers(headers: Iterable[tuple[str, str]]) -> tuple[tuple[str, str], ...]:
    """Selects the headers that writers store, in the order of
    `STORED_HEADERS`, with lowercase names."""
    pairs = [(name.lower(), value) for name, value in headers]
    return tuple(
        (stored, value) for stored in STORED_HEADERS for name, value in pairs if name == stored
    )


def body_file_name(body: bytes, content_types: Iterable[str]) -> str:
    """Returns the body file name: 16 hex digits of the SHA-256 hash and an
    extension from the `Content-Type` header values."""
    digest = hashlib.sha256(body).hexdigest()[:16]
    return f"{digest}.{extension_for_essence(extract_essence(content_types))}"


def format_manifest(entries: Iterable[Entry]) -> str:
    """Returns the manifest file content, with entries sorted by (url, method)."""
    ordered = sorted(entries, key=lambda entry: entry.key)
    manifest = {"version": MANIFEST_VERSION, "entries": [entry.to_json() for entry in ordered]}
    return json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"


def parse_manifest(text: str) -> list[Entry]:
    """Parses a manifest file. Rejects unknown versions."""
    data = json.loads(text)
    if data.get("version") != MANIFEST_VERSION:
        raise ValueError(
            f"manifest version {data.get('version')!r} is not supported "
            f"(expected {MANIFEST_VERSION})"
        )
    return [Entry.from_json(entry) for entry in data["entries"]]


def read_manifest(fixture: Path) -> list[Entry]:
    """Reads `manifest.json` in the fixture directory."""
    return parse_manifest((fixture / MANIFEST_FILE).read_text(encoding="utf-8"))


def write_atomic(path: Path, data: bytes) -> None:
    """Writes a temporary file next to `path`, then renames it to `path`."""
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_bytes(data)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def body_path(fixture: Path, body: str) -> Path:
    """Resolves the `body` value of an entry. Rejects absolute paths and `..`."""
    parts = body.split("/")
    if not body or body.startswith("/") or any(part in ("", ".", "..") for part in parts):
        raise ValueError(f"invalid body path {body!r}")
    return fixture.joinpath(*parts)


class Fixture:
    """A fixture directory: the entries in memory, the bodies on disk.

    `record` adds or replaces an entry and writes the body file. `save`
    writes the manifest. The manifest is read on creation if it exists, so
    new entries merge into it, as with swb's `RecordingFetcher`.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        self.entries: dict[tuple[str, str], Entry] = {}
        if (path / MANIFEST_FILE).exists():
            for entry in read_manifest(path):
                self.entries.setdefault(entry.key, entry)

    def get(self, url: str, method: str = "GET") -> Entry | None:
        """Returns the entry for a request URL (the fragment is ignored)."""
        return self.entries.get((entry_url(url), method.upper()))

    def read_body(self, entry: Entry) -> bytes:
        """Returns the body of an entry."""
        return body_path(self.path, entry.body).read_bytes()

    def record(
        self,
        method: str,
        url: str,
        status: int,
        headers: Iterable[tuple[str, str]],
        body: bytes,
    ) -> Entry:
        """Stores a response. Writes the body file if it does not exist."""
        headers = list(headers)
        content_types = [value for name, value in headers if name.lower() == "content-type"]
        file_name = body_file_name(body, content_types)
        files = self.path / FILES_DIR
        files.mkdir(parents=True, exist_ok=True)
        file_path = files / file_name
        if not file_path.exists():
            write_atomic(file_path, body)
        entry = Entry(
            method=method.upper(),
            url=entry_url(url),
            status=status,
            headers=stored_headers(headers),
            body=f"{FILES_DIR}/{file_name}",
        )
        self.entries[entry.key] = entry
        return entry

    def save(self) -> None:
        """Writes `manifest.json` atomically."""
        self.path.mkdir(parents=True, exist_ok=True)
        write_atomic(
            self.path / MANIFEST_FILE, format_manifest(self.entries.values()).encode("utf-8")
        )

    def unused_files(self) -> list[Path]:
        """Returns the files in `files/` that no entry refers to."""
        used = {body_path(self.path, entry.body) for entry in self.entries.values()}
        files = self.path / FILES_DIR
        if not files.is_dir():
            return []
        return sorted(path for path in files.iterdir() if path.is_file() and path not in used)
