"""Fixture manifest writing: must match crates/net/src/fixture byte for byte."""

import pytest

from swbtools.manifest import (
    MANIFEST_FILE,
    Fixture,
    body_file_name,
    body_path,
    entry_url,
    format_manifest,
    parse_manifest,
    read_manifest,
    stored_headers,
)

# The expected file of the Rust test `manifest_format_is_exact`
# (crates/net/src/fixture/tests.rs), with the body names filled in.
RUST_SAMPLE = """\
{
  "version": 1,
  "entries": [
    {
      "method": "GET",
      "url": "https://a.test/old",
      "status": 301,
      "headers": [
        [
          "location",
          "/"
        ]
      ],
      "body": "files/e3b0c44298fc1c14.bin"
    },
    {
      "method": "GET",
      "url": "https://a.test/style.css",
      "status": 200,
      "headers": [
        [
          "content-type",
          "text/css"
        ]
      ],
      "body": "files/9e07b08613fbdd60.css"
    }
  ]
}
"""


def test_format_matches_rust_sample(tmp_path):
    fixture = Fixture(tmp_path)
    # Recorded in the opposite order of the file order.
    fixture.record(
        "GET", "https://a.test/style.css", 200, [("Content-Type", "text/css")], b"p { color: red }"
    )
    fixture.record("GET", "https://a.test/old", 301, [("Location", "/")], b"")
    fixture.save()
    assert (tmp_path / MANIFEST_FILE).read_bytes() == RUST_SAMPLE.encode()
    assert sorted(path.name for path in (tmp_path / "files").iterdir()) == [
        "9e07b08613fbdd60.css",
        "e3b0c44298fc1c14.bin",
    ]


def test_round_trip_is_stable(tmp_path):
    (tmp_path / MANIFEST_FILE).write_text(RUST_SAMPLE)
    entries = read_manifest(tmp_path)
    assert [entry.url for entry in entries] == ["https://a.test/old", "https://a.test/style.css"]
    assert entries[0].headers == (("location", "/"),)
    assert format_manifest(entries) == RUST_SAMPLE
    fixture = Fixture(tmp_path)
    fixture.save()
    assert (tmp_path / MANIFEST_FILE).read_text() == RUST_SAMPLE


def test_entries_sorted_by_url_then_method(tmp_path):
    fixture = Fixture(tmp_path)
    fixture.record("POST", "https://a.test/form", 200, [], b"thanks")
    fixture.record("GET", "https://a.test/form", 200, [], b"form")
    fixture.record("GET", "https://a.test/", 200, [], b"page")
    fixture.record("GET", "https://a.test/Z", 200, [], b"upper")
    fixture.save()
    keys = [entry.key for entry in read_manifest(tmp_path)]
    assert keys == [
        ("https://a.test/", "GET"),
        ("https://a.test/Z", "GET"),
        ("https://a.test/form", "GET"),
        ("https://a.test/form", "POST"),
    ]


def test_empty_manifest_and_empty_headers(tmp_path):
    fixture = Fixture(tmp_path)
    fixture.save()
    assert (tmp_path / MANIFEST_FILE).read_text() == '{\n  "version": 1,\n  "entries": []\n}\n'
    fixture.record("GET", "https://a.test/x", 200, [], b"")
    assert '"headers": [],' in format_manifest(fixture.entries.values())


def test_non_ascii_is_written_as_utf8(tmp_path):
    fixture = Fixture(tmp_path)
    fixture.record(
        "GET", "https://a.test/a", 200, [("content-type", 'text/html; title="Rašić"')], b""
    )
    fixture.save()
    text = (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    assert 'title=\\"Rašić\\"' in text


def test_record_replaces_entry_and_merges_existing(tmp_path):
    first = Fixture(tmp_path)
    first.record("GET", "https://a.test/", 200, [("content-type", "text/html")], b"one")
    first.record("GET", "https://a.test/style.css", 200, [("content-type", "text/css")], b"a")
    first.save()
    second = Fixture(tmp_path)
    assert len(second.entries) == 2
    second.record("GET", "https://a.test/style.css", 200, [("content-type", "text/css")], b"b")
    second.save()
    entries = {entry.url: entry for entry in read_manifest(tmp_path)}
    assert len(entries) == 2
    assert second.read_body(entries["https://a.test/style.css"]) == b"b"
    assert second.read_body(entries["https://a.test/"]) == b"one"
    assert [path.name for path in second.unused_files()] == [body_file_name(b"a", ["text/css"])]


def test_identical_bodies_share_one_file(tmp_path):
    fixture = Fixture(tmp_path)
    html = [("content-type", "text/html; charset=utf-8")]
    a = fixture.record("GET", "https://a.test/", 200, html, b"<p>page")
    b = fixture.record("GET", "https://a.test/copy", 200, html, b"<p>page")
    assert a.body == b.body
    assert len(list((tmp_path / "files").iterdir())) == 1


def test_fragment_is_not_part_of_the_entry_url(tmp_path):
    assert entry_url("https://a.test/#top") == "https://a.test/"
    fixture = Fixture(tmp_path)
    fixture.record("GET", "https://a.test/#top", 200, [], b"")
    assert fixture.get("https://a.test/#elsewhere") is not None
    assert fixture.get("https://a.test/", "post") is None


def test_stored_headers_order_and_case():
    headers = [
        ("Location", "/next"),
        ("Cache-Control", "no-cache"),
        ("Content-Type", "text/html"),
        ("content-type", "text/plain"),
    ]
    assert stored_headers(headers) == (
        ("content-type", "text/html"),
        ("content-type", "text/plain"),
        ("location", "/next"),
    )


@pytest.mark.parametrize(
    ("content_types", "name"),
    [
        ([], "e3b0c44298fc1c14.bin"),
        (["text/css; charset=utf-8"], "e3b0c44298fc1c14.css"),
        (["application/x-unknown"], "e3b0c44298fc1c14.bin"),
        (["TEXT/HTML"], "e3b0c44298fc1c14.html"),
        (["application/javascript"], "e3b0c44298fc1c14.js"),
        (["image/jpeg"], "e3b0c44298fc1c14.jpg"),
        (["image/vnd.microsoft.icon"], "e3b0c44298fc1c14.ico"),
        (["font/woff2"], "e3b0c44298fc1c14.woff2"),
        (["text/plain", "text/html"], "e3b0c44298fc1c14.html"),
        (["text/html", "*/*"], "e3b0c44298fc1c14.html"),
        (["nonsense"], "e3b0c44298fc1c14.bin"),
    ],
)
def test_body_file_names(content_types, name):
    assert body_file_name(b"", content_types) == name


def test_body_path_rejects_unsafe_paths(tmp_path):
    assert body_path(tmp_path, "files/a.html") == tmp_path / "files" / "a.html"
    for bad in ["", "/etc/passwd", "files/../x", "../x", "files//x"]:
        with pytest.raises(ValueError):
            body_path(tmp_path, bad)


def test_unknown_version_is_rejected():
    with pytest.raises(ValueError):
        parse_manifest('{"version": 2, "entries": []}')
