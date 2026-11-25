"""Tests of `swbtools media-fixture`."""

import json
from pathlib import Path

from swbtools.manifest import Fixture, read_manifest
from swbtools.media_fixture import media_fixture


def test_records_the_whole_file_as_one_entry(tmp_path: Path) -> None:
    media = tmp_path / "clip.mp4"
    media.write_bytes(bytes(range(256)) * 40)
    fixture = tmp_path / "fixture"
    assert media_fixture(fixture, "https://media.test/clip.mp4#t=1", media, "video/mp4") == 0
    entries = read_manifest(fixture)
    assert len(entries) == 1
    entry = entries[0]
    assert (entry.method, entry.url, entry.status) == ("GET", "https://media.test/clip.mp4", 200)
    assert dict(entry.headers) == {"content-type": "video/mp4"}
    assert Fixture(fixture).read_body(entry) == media.read_bytes()
    manifest = json.loads((fixture / "manifest.json").read_text())
    assert manifest["version"] == 1


def test_adds_to_an_existing_fixture_and_rejects_a_missing_file(tmp_path: Path) -> None:
    fixture = tmp_path / "fixture"
    first = tmp_path / "a.mp4"
    first.write_bytes(b"a" * 100)
    second = tmp_path / "b.webm"
    second.write_bytes(b"b" * 100)
    assert media_fixture(fixture, "https://media.test/a.mp4", first, "video/mp4") == 0
    assert media_fixture(fixture, "https://media.test/b.webm", second, "video/webm") == 0
    assert len(read_manifest(fixture)) == 2
    assert media_fixture(fixture, "https://media.test/c.mp4", tmp_path / "none", "video/mp4") == 2
