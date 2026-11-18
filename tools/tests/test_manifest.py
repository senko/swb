"""Fixture manifest writing: must match crates/net/src/fixture byte for byte."""

import re

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
    request_body_hash,
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
        ("https://a.test/", "GET", ""),
        ("https://a.test/Z", "GET", ""),
        ("https://a.test/form", "GET", ""),
        ("https://a.test/form", "POST", ""),
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


# --- The request body hash (the Rust tests of `posts_with_different_bodies...`) -------

QUERY = "https://a.test/query"
TEXT = [("content-type", "text/plain")]


def record_posts(fixture, *bodies):
    for body in bodies:
        fixture.record("POST", QUERY, 200, TEXT, b"answer:" + body, body)


def test_request_body_hash_is_16_hex_digits_of_sha256():
    assert request_body_hash(b"x=1") == "1f206b11c23e28cc"
    assert request_body_hash(b"") == "e3b0c44298fc1c14"


def test_posts_with_different_bodies_get_their_own_entries(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1", b"b=2")
    fixture.save()
    text = (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    assert text.count('"request_body"') == 2
    # The field comes after `body`, as in the Rust format.
    assert re.search(r'"body": "files/\w+\.txt",\n      "request_body": "c22fea5d7428e5cf"', text)
    again = Fixture(tmp_path)
    assert len(again.entries) == 2
    for body in (b"a=1", b"b=2"):
        entry = again.get(QUERY, "POST", body)
        assert entry is not None and again.read_body(entry) == b"answer:" + body
    # No entry without the field: another body, or none, has no answer.
    assert again.get(QUERY, "POST", b"c=3") is None
    assert again.get(QUERY, "POST") is None
    assert again.lookup(QUERY, "POST", b"c=3") is None


def test_a_single_body_is_not_written_and_matches_any_body(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1")
    fixture.save()
    assert "request_body" not in (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    again = Fixture(tmp_path)
    assert again.get(QUERY, "POST", b"other") is not None
    assert again.get(QUERY, "POST") is not None


def test_entries_without_the_field_answer_when_no_hash_matches(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1", b"b=2")
    # An entry without the field, as an older fixture has.
    wildcard = fixture.record("POST", QUERY, 202, TEXT, b"wild")
    assert wildcard.request_body == ""
    assert len(fixture.entries) == 3
    assert fixture.get(QUERY, "POST", b"a=1").status == 200
    assert fixture.get(QUERY, "POST", b"c=3").status == 202
    assert fixture.get(QUERY, "POST").status == 202


def test_a_new_post_keeps_the_entry_without_the_field(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1")
    fixture.save()
    again = Fixture(tmp_path)
    assert [key[2] for key in again.entries] == [""]
    # A later session records another body: the old entry stays and answers
    # the other bodies (as the Rust test `a_new_post_keeps_...`).
    record_posts(again, b"b=2")
    assert sorted(key[2] for key in again.entries) == ["", request_body_hash(b"b=2")]
    again.save()
    text = (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    assert text.count('"request_body"') == 1
    last = Fixture(tmp_path)
    assert last.read_body(last.get(QUERY, "POST", b"a=1")) == b"answer:a=1"
    assert last.read_body(last.get(QUERY, "POST", b"b=2")) == b"answer:b=2"
    assert last.read_body(last.get(QUERY, "POST", b"c=3")) == b"answer:a=1"


@pytest.mark.parametrize(
    "value",
    [
        "5",
        '""',
        '"C22FEA5D7428E5CF"',
        '"c22fea5d7428e5c"',
        '"c22fea5d7428e5cfa"',
        '"c22fea5d7428e5cg"',
    ],
)
def test_a_malformed_request_body_is_an_error(value):
    text = (
        '{"version": 1, "entries": [{"method": "POST", "url": "https://a.test/q", '
        f'"status": 200, "headers": [], "body": "files/x.txt", "request_body": {value}}}]}}'
    )
    with pytest.raises(ValueError):
        parse_manifest(text)


def test_a_null_request_body_is_no_field():
    text = (
        '{"version": 1, "entries": [{"method": "POST", "url": "https://a.test/q", '
        '"status": 200, "headers": [], "body": "files/x.txt", "request_body": null}]}'
    )
    assert parse_manifest(text)[0].request_body == ""


def test_an_empty_request_body_is_no_body(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"", b"a=1")
    fixture.save()
    text = (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    assert text.count('"request_body"') == 1
    again = Fixture(tmp_path)
    assert again.read_body(again.get(QUERY, "POST", b"")) == b"answer:"
    assert again.read_body(again.get(QUERY, "POST")) == b"answer:"
    assert again.read_body(again.get(QUERY, "POST", b"a=1")) == b"answer:a=1"


def test_a_pattern_prefers_the_hashed_entry_of_the_same_url(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1", b"b=2")
    fixture.record("POST", QUERY, 202, TEXT, b"wild")
    fixture.save()
    (tmp_path / "fixture.json").write_text('{"replay_patterns": ["https://a.test/*"]}')
    again = Fixture(tmp_path)
    missing = "https://a.test/zzz"
    assert again.lookup(missing, "POST", b"a=1").status == 200
    assert again.lookup(missing, "POST", b"b=2").status == 200
    assert again.lookup(missing, "POST", b"c=3").status == 202


def test_the_field_survives_a_rewrite(tmp_path):
    fixture = Fixture(tmp_path)
    record_posts(fixture, b"a=1", b"b=2")
    fixture.save()
    again = Fixture(tmp_path)
    again.record("GET", "https://a.test/other", 200, TEXT, b"x")
    again.save()
    after = (tmp_path / MANIFEST_FILE).read_text(encoding="utf-8")
    assert after.count('"request_body"') == 2
    assert after.count('"method"') == 3
    assert Fixture(tmp_path).get(QUERY, "POST", b"b=2") is not None


def test_replay_patterns_respect_the_request_body(tmp_path):
    fixture = Fixture(tmp_path)
    for version, body in (("1", b"a=1"), ("2", b"b=2")):
        fixture.record(
            "POST", f"https://a.test/v{version}/q", 200, TEXT, b"v" + version.encode(), body
        )
    fixture.save()
    (tmp_path / "fixture.json").write_text('{"replay_patterns": ["https://a.test/*/q"]}')
    again = Fixture(tmp_path)
    # Each URL has one entry (no field written), so both match any body.
    entry = again.lookup("https://a.test/v9/q", "POST", b"zzz")
    assert entry is not None and again.read_body(entry) == b"v2"
