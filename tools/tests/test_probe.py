"""Tests of `swbtools probe`: the case file format, the output, the
comparison with swb (synthetic dumps), and the example file in Chromium (and
swb, if its release binary exists)."""

import asyncio
import io
import json
from pathlib import Path

import pytest
from PIL import Image

from swbtools import paths, swb
from swbtools.boxes import BoxDump, Element
from swbtools.browser import in_chromium
from swbtools.cli import build_parser
from swbtools.probe import (
    Case,
    CaseResult,
    ProbeError,
    Query,
    Row,
    SwbBox,
    _probe_all,
    any_error,
    any_outside,
    attach_swb,
    compare_box,
    compare_ink,
    format_json,
    format_row,
    format_text,
    ink_of,
    parse_cases,
    probe,
    read_cases,
    select_cases,
)

EXAMPLE = paths.repo_root() / "tools" / "probes" / "example.json"


def case_json(**overrides):
    case = {"name": "a", "html": "<p>x", "measure": [{"boxes": "p"}]}
    case.update(overrides)
    return json.dumps({"cases": [case]})


# --- Parsing ---------------------------------------------------------------


def test_parse_all_query_kinds_and_viewports():
    text = json.dumps(
        {
            "viewport": [500, 400],
            "cases": [
                {
                    "name": "one",
                    "html": "<p>x",
                    "measure": [
                        {"boxes": "p"},
                        {"rects": "p"},
                        {"style": "p", "props": ["width"]},
                        {"js": "1 + 1"},
                    ],
                },
                {"name": "two", "html": "", "viewport": [100, 200], "measure": [{"js": "2"}]},
            ],
        }
    )
    one, two = parse_cases(text)
    assert one.viewport == (500, 400)
    assert two.viewport == (100, 200)
    assert one.queries == (
        Query("boxes", "p"),
        Query("rects", "p"),
        Query("style", "p", ("width",)),
        Query("js", "1 + 1"),
    )


def test_default_viewport():
    assert parse_cases(case_json())[0].viewport == (800, 600)


@pytest.mark.parametrize(
    ("text", "message"),
    [
        ("{", "not valid JSON"),
        ("[]", '"cases" list'),
        (case_json(name=""), "missing name"),
        (json.dumps({"cases": [{"html": "", "measure": [{"js": "1"}]}]}), "case 1: missing name"),
        (case_json(html=3), "html must be a string"),
        (case_json(measure=[]), "measure must be a non-empty list"),
        (case_json(measure=[{"colors": "p"}]), "exactly one query kind"),
        (case_json(measure=[{"boxes": "p", "js": "1"}]), "exactly one query kind"),
        (case_json(measure=[{"boxes": "p", "props": ["x"]}]), "props belongs to style"),
        (case_json(measure=[{"boxes": "p", "extra": 1}]), "unknown key 'extra'"),
        (case_json(measure=[{"style": "p"}]), "style needs props"),
        (case_json(measure=[{"boxes": ""}]), "non-empty string"),
        (case_json(viewport=[0, 5]), "viewport must be"),
        (case_json(viewport="800x600"), "viewport must be"),
        (case_json(files=["a.ttf"]), "files must be an object"),
        (case_json(files={"sub/a.ttf": "x"}), "plain file name"),
        (case_json(files={"a.ttf": 3}), "must be a path"),
    ],
)
def test_bad_case_files(text, message):
    with pytest.raises(ProbeError, match=message):
        parse_cases(text, "f.json")


def test_files_resolve_against_the_repository():
    (case,) = parse_cases(case_json(files={"a.ttf": "fixtures/fonts/DejaVuSans.ttf", "b": "/x/b"}))
    assert case.files == (
        ("a.ttf", paths.repo_root() / "fixtures/fonts/DejaVuSans.ttf"),
        ("b", Path("/x/b")),
    )
    assert parse_cases(case_json())[0].files == ()


def test_ink_of_a_screenshot():
    image = Image.new("RGB", (6, 4), "white")
    image.putpixel((1, 2), (0, 0, 0))
    image.putpixel((4, 1), (255, 255, 255))
    image.putpixel((3, 3), (128, 128, 128))
    buffer = io.BytesIO()
    image.save(buffer, "PNG")
    assert ink_of(buffer.getvalue()) == (1.5, (1, 2, 3, 2))
    blank = io.BytesIO()
    Image.new("RGB", (2, 2), "white").save(blank, "PNG")
    assert ink_of(blank.getvalue()) == (0.0, None)


def test_error_names_the_file_and_the_case():
    with pytest.raises(ProbeError, match=r"f\.json: case 'a', measure 1: .*exactly one"):
        parse_cases(case_json(measure=[{"nope": 1}]), "f.json")


def test_duplicate_case_names():
    case = {"name": "a", "html": "", "measure": [{"js": "1"}]}
    with pytest.raises(ProbeError, match="duplicate case name 'a'"):
        parse_cases(json.dumps({"cases": [case, case]}))


def test_read_missing_file(tmp_path):
    with pytest.raises(ProbeError, match="cannot read"):
        read_cases(tmp_path / "missing.json")


def test_example_file_is_valid():
    names = [case.name for case in read_cases(EXAMPLE)]
    assert names == ["bfc-next-to-float", "inline-split-over-two-lines", "computed-line-height"]


def test_select_cases():
    cases = parse_cases(
        json.dumps(
            {"cases": [{"name": n, "html": "", "measure": [{"js": "1"}]} for n in ("a", "b", "c")]}
        )
    )
    assert [c.name for c in select_cases(cases, [])] == ["a", "b", "c"]
    assert [c.name for c in select_cases(cases, ["c", "a"])] == ["a", "c"]
    with pytest.raises(ProbeError, match="unknown case 'z'"):
        select_cases(cases, ["z"])


def test_cli_parses_swb_forms():
    parser = build_parser()
    assert not parser.parse_args(["probe", "f.json"]).with_swb
    args = parser.parse_args(["probe", "--with-swb", "f.json", "--swb", "bin"])
    assert args.with_swb
    assert args.swb == Path("bin")
    assert args.files == [Path("f.json")]
    args = parser.parse_args(["probe", "f.json", "--case", "a", "--case", "b", "--tolerance", "2"])
    assert args.case == ["a", "b"]
    assert args.tolerance == 2.0


# --- Output ----------------------------------------------------------------


def box_row(label="#a > p[1]", rect=(8.0, 8.0, 300.0, 18.0), tag="p", index=5):
    return Row("boxes", label, {"tag": tag, "index": index, "rect": rect})


def test_format_chromium_rows():
    assert format_row("c", box_row()) == ["c  boxes  #a > p[1]  x=8 y=8 w=300 h=18"]
    rects = Row("rects", "span.x[1]#2", {"rect": (0.0, 21.5, 144.95, 17.0)})
    assert format_row("c", rects) == ["c  rects  span.x[1]#2  x=0 y=21.5 w=144.95 h=17"]
    assert format_row("c", Row("boxes", "p[1]", {"tag": "p", "index": 1, "rect": None})) == [
        "c  boxes  p[1]  no box"
    ]
    style = Row("style", "#a[1]", {"style": {"width": "10px", "line-height": "normal"}})
    assert format_row("c", style) == ["c  style  #a[1]  width=10px line-height=normal"]
    js = Row("js", "document.title", {"value": {"a": [1, "é"]}})
    assert format_row("c", js) == ['c  js     document.title  {"a": [1, "é"]}']
    nothing = Row("boxes", "p", {"matched": False, "rect": None})
    assert format_row("c", nothing) == ["c  boxes  p  no match"]
    error = Row("js", "(", {"error": "SyntaxError"})
    assert format_row("c", error) == ["c  js     (  error: SyntaxError"]


def test_format_swb_rows():
    row = box_row()
    row.swb = SwbBox("ok", rect=(8.0, 8.0, 300.5, 18.0), delta=(0.0, 0.0, 0.5, 0.0))
    assert format_row("c", row)[1] == (
        "c  swb    #a > p[1]  x=8 y=8 w=300.5 h=18  dx=0 dy=0 dw=0.5 dh=0"
    )
    row.swb = SwbBox("ok", rect=(8.0, 8.0, 310.0, 18.0), delta=(0.0, 0.0, 10.0, 0.0), outside=True)
    assert format_row("c", row)[1].endswith("dw=10 dh=0  !")
    row.swb = SwbBox("tag-mismatch", tag="div", outside=True)
    assert format_row("c", row)[1] == "c  swb    #a > p[1]  tag differs at index 5: swb has <div>"
    row.swb = SwbBox("missing", outside=True)
    assert "no element at index 5" in format_row("c", row)[1]


def test_format_json_has_the_same_data():
    row = box_row()
    row.swb = SwbBox("ok", rect=(8.0, 8.0, 301.0, 18.0), delta=(0.0, 0.0, 1.0, 0.0))
    result = CaseResult("c", (800, 600), [row, Row("js", "1", {"value": 1})])
    data = json.loads(format_json([result]))
    first, second = data["cases"][0]["results"]
    assert data["cases"][0]["viewport"] == [800, 600]
    assert first["label"] == "#a > p[1]"
    assert first["rect"] == [8.0, 8.0, 300.0, 18.0]
    assert first["swb"]["delta"] == [0.0, 0.0, 1.0, 0.0]
    assert second == {"kind": "js", "label": "1", "value": 1}
    assert format_text([result]).count("\n") == 2


# --- Comparison with swb ---------------------------------------------------


def dump(*elements):
    return BoxDump("u", (800, 600), [Element(tag, rect) for tag, rect in elements])


def test_compare_box_within_and_outside_tolerance():
    swb_dump = dump(("html", None), ("p", (8.0, 8.0, 300.0, 19.5)))
    inside = compare_box("p", 1, (8.0, 8.0, 300.0, 18.5), swb_dump, 1.0)
    assert (inside.status, inside.outside, inside.delta) == ("ok", False, (0.0, 0.0, 0.0, 1.0))
    outside = compare_box("p", 1, (8.0, 8.0, 300.0, 18.0), swb_dump, 1.0)
    assert outside.outside
    assert outside.delta == (0.0, 0.0, 0.0, 1.5)
    assert not compare_box("p", 1, (8.0, 8.0, 300.0, 18.0), swb_dump, 2.0).outside


def test_compare_box_tag_missing_and_box_presence():
    swb_dump = dump(("html", None), ("div", (0.0, 0.0, 1.0, 1.0)))
    mismatch = compare_box("p", 1, (0.0, 0.0, 1.0, 1.0), swb_dump, 1.0)
    assert (mismatch.status, mismatch.tag, mismatch.outside) == ("tag-mismatch", "div", True)
    assert compare_box("p", 7, None, swb_dump, 1.0).status == "missing"
    assert compare_box("html", 0, None, swb_dump, 1.0).outside is False
    assert compare_box("html", 0, (0.0, 0.0, 1.0, 1.0), swb_dump, 1.0).status == "box-mismatch"
    assert compare_box("div", 1, None, swb_dump, 1.0).status == "box-mismatch"


def test_attach_swb_uses_the_document_index_and_skips_other_rows():
    swb_dump = dump(("html", None), ("p", (0.0, 0.0, 10.0, 10.0)), ("span", (0.0, 0.0, 5.0, 5.0)))
    result = CaseResult(
        "c",
        (800, 600),
        [
            box_row(tag="span", index=2, rect=(0.0, 0.0, 5.0, 5.0)),
            Row("js", "1", {"value": 1}),
            Row("boxes", "x", {"matched": False, "rect": None}),
        ],
    )
    attach_swb(result, swb_dump, 1.0)
    assert result.rows[0].swb.status == "ok"
    assert result.rows[1].swb is None
    assert result.rows[2].swb is None
    assert not any_outside([result])
    result.rows[0].swb = SwbBox("tag-mismatch", outside=True)
    assert any_outside([result])
    assert any_outside([CaseResult("c", (1, 1), swb_error="swb failed")])


def test_compare_ink_and_attach_it_from_a_screenshot():
    assert not compare_ink(100.0, (0, 0, 10, 10), 104.0, (0, 0, 10, 10), 1.0).outside
    assert compare_ink(100.0, (0, 0, 10, 10), 106.0, (0, 0, 10, 10), 1.0).outside
    assert not compare_ink(10.0, (0, 0, 2, 2), 12.0, (1, 0, 2, 2), 1.0).outside
    assert compare_ink(10.0, (0, 0, 2, 2), 10.0, (2, 0, 2, 2), 1.0).outside
    assert compare_ink(0.0, None, 1.0, (0, 0, 1, 1), 1.0).outside
    # A black 2x2 square at (11, 12) of a white screenshot; the row's area
    # is (10, 10, 5, 5).
    shot = Image.new("RGB", (20, 20), "white")
    for x in (11, 12):
        for y in (12, 13):
            shot.putpixel((x, y), (0, 0, 0))
    row = Row("ink", "#a[1]", {"ink": 4.0, "bbox": (1, 2, 2, 2), "area": (10.0, 10.0, 5.0, 5.0)})
    result = CaseResult("c", (20, 20), [row])
    attach_swb(result, dump(("html", None)), 1.0, shot)
    assert row.swb_ink is not None
    assert (row.swb_ink.ink, row.swb_ink.bbox, row.swb_ink.outside) == (4.0, (1, 2, 2, 2), False)
    assert format_row("c", row)[1] == "c  swb    #a[1]  ink=4 bbox=1,2,2,2  dink=0"
    assert not any_outside([result])


def test_probe_command_with_a_screenshot():
    command = swb.probe_command(Path("swb"), "file:///p.html", (800, 600), Path("b.json"))
    assert "--screenshot" not in command
    command = swb.probe_command(
        Path("swb"), "file:///p.html", (800, 600), Path("b.json"), Path("s.png")
    )
    assert command[-4:] == ["--full-page", "--screenshot", "s.png", "file:///p.html"]


def test_any_error_finds_failed_queries():
    ok = CaseResult("c", (1, 1), [Row("js", "1", {"value": 1})])
    failed = CaseResult("d", (1, 1), [Row("boxes", "p[", {"error": "not a valid selector"})])
    assert not any_error([ok])
    assert any_error([ok, failed])


# --- Chromium and swb ------------------------------------------------------


@pytest.mark.usefixtures("chromium")
def test_example_in_chromium():
    cases = read_cases(EXAMPLE)
    results = asyncio.run(
        in_chromium(lambda page, directory: _probe_all(cases, None, 1.0, page, directory))
    )
    by_name = {result.name: result for result in results}
    floats = by_name["bfc-next-to-float"].rows
    assert [row.data["tag"] for row in floats] == ["div", "div", "p"]
    assert floats[0].data["rect"] == (8.0, 8.0, 100.0, 50.0)
    # The BFC root is narrowed by the float; its box is next to it.
    assert floats[1].data["rect"][0] == 108.0
    split = by_name["inline-split-over-two-lines"].rows
    assert [row.kind for row in split] == ["boxes", "rects", "rects"]
    assert split[1].data["rect"][1] < split[2].data["rect"][1]
    style = by_name["computed-line-height"].rows
    assert [row.data["style"]["line-height"] for row in style[:3]] == ["normal", "30px", "30px"]
    assert style[3].data == {"value": 600}


@pytest.mark.usefixtures("chromium")
def test_probe_waits_for_web_fonts():
    # The font is used by a paragraph only, so layout starts its load; the
    # probe must wait for it before it runs the queries.
    html = (
        "<style>@font-face{font-family:W;src:url(x.ttf)}p{font-family:W}</style><body><p>text</p>"
    )
    font = paths.repo_root() / "fixtures/fonts/DejaVuSans.ttf"
    queries = (
        Query("js", "document.fonts.status"),
        Query("js", "[...document.fonts].every(f => f.status === 'loaded')"),
    )
    case = Case("w", html, (400, 300), queries, (("x.ttf", font),))
    (result,) = asyncio.run(
        in_chromium(lambda page, directory: _probe_all([case], None, 1.0, page, directory))
    )
    assert [row.data["value"] for row in result.rows] == ["loaded", True]


@pytest.mark.usefixtures("chromium")
def test_errors_do_not_stop_the_run():
    case = Case("e", "<p>x", (400, 300), (Query("boxes", "p["), Query("js", "("), Query("js", "1")))
    (result,) = asyncio.run(
        in_chromium(lambda page, directory: _probe_all([case], None, 1.0, page, directory))
    )
    assert [("error" in row.data) for row in result.rows] == [True, True, False]


@pytest.mark.usefixtures("chromium")
def test_probe_with_swb_on_the_example(capsys):
    binary = swb.find_swb()
    if binary is None:
        pytest.skip("the swb binary does not exist")
    status = probe([EXAMPLE], ["bfc-next-to-float"], binary, 1.0, False)
    out = capsys.readouterr().out
    assert status == 0, out
    assert out.count("  swb    (#f, #b, #b > p)[") == 3
    assert "!" not in out


@pytest.mark.usefixtures("chromium")
def test_probe_json_output(capsys):
    assert probe([EXAMPLE], ["computed-line-height"], None, 1.0, True) == 0
    data = json.loads(capsys.readouterr().out)
    assert data["cases"][0]["name"] == "computed-line-height"
