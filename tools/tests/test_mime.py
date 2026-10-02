"""Content type essence extraction (port of crates/net/src/mime.rs)."""

import pytest

from swbtools.mime import extract_essence, parse_essence, split_header_value


@pytest.mark.parametrize(
    ("value", "essence"),
    [
        ("text/html", "text/html"),
        (" Text/HTML ; charset=utf-8", "text/html"),
        ("image/svg+xml", "image/svg+xml"),
        ("text/html;", "text/html"),
        ("text/ html", None),
        ("text", None),
        ("/html", None),
        ("text/", None),
        ("te xt/html", None),
        ("", None),
    ],
)
def test_parse_essence(value, essence):
    assert parse_essence(value) == essence


def test_split_keeps_quoted_commas():
    assert split_header_value('text/html;a="b,c", text/css') == ['text/html;a="b,c"', "text/css"]
    assert split_header_value('a="x\\"y,z"') == ['a="x\\"y,z"']
    assert split_header_value("a,,b") == ["a", "", "b"]


@pytest.mark.parametrize(
    ("values", "essence"),
    [
        # Examples from https://fetch.spec.whatwg.org/#example-extract-a-mime-type
        (["text/plain;charset=gbk, text/html"], "text/html"),
        (["text/html;charset=gbk;a=b, text/html;x=y"], "text/html"),
        (["text/html;charset=gbk;a=b", "text/html;x=y"], "text/html"),
        (["text/html;charset=gbk", "x/x", "text/html;x=y"], "text/html"),
        (["text/html", "cannot-parse"], "text/html"),
        (["text/html", "*/*"], "text/html"),
        (["text/html", ""], "text/html"),
        ([], None),
        (["bogus"], None),
    ],
)
def test_extract_essence(values, essence):
    assert extract_essence(values) == essence
