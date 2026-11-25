"""Adds a media file to a fixture directory as one whole response.

`swbtools capture` cannot record a media file: the page's requests for a
media file are `Range` requests that get partial responses (docs/media-
diagnosis.md section 5). A fixture instead holds the whole file as one entry
(status 200, the full body); swb's replay answers `Range` requests from it
(206, `Content-Range`, `Accept-Ranges: bytes`, 416 outside the file), and
reads a body file over 8 MiB at an offset, never whole (`swb_net::fixture`).
"""

import logging
from pathlib import Path

from swbtools.manifest import Fixture

log = logging.getLogger("swbtools")


def media_fixture(fixture: Path, url: str, file: Path, content_type: str) -> int:
    """Records `file` as the response to `GET url` in the fixture directory
    `fixture` (created if it does not exist). Returns the exit status."""
    if not file.is_file():
        log.error("%s is not a file", file)
        return 2
    target = Fixture(fixture)
    entry = target.record("GET", url, 200, [("content-type", content_type)], file.read_bytes())
    target.save()
    log.info("%s: %s -> %s (%d bytes)", fixture, entry.url, entry.body, file.stat().st_size)
    return 0
