"""The test tools with JavaScript on: CORS answers and the replay pattern in
the routing, the fixed start time, `reference --js`, `compare --js`, and the
probe option `"javascript": true` (docs/testing.md, "Compare with JavaScript
on"). No network: hosts are `.test` names that never resolve, so a request
that escapes the interception fails visibly."""

import asyncio
import json
import stat
import threading
from collections.abc import Iterator
from datetime import UTC, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import ClassVar

import pytest
from PIL import Image

from swbtools import browser, cli, paths, swb
from swbtools.boxes import BoxDump, Element, read_dump, write_dump
from swbtools.browser import in_chromium
from swbtools.capture import chromium_pass
from swbtools.compare import Options, compare_fixture
from swbtools.manifest import Entry, Fixture
from swbtools.probe import (
    Case,
    CaseResult,
    ProbeError,
    Query,
    Row,
    _compare_with_swb,
    _probe_all,
    any_outside,
    format_text,
    parse_cases,
    parse_swb_value,
)
from swbtools.reference import render_js_fixture
from swbtools.routing import (
    Replayer,
    cors_headers,
    header_value,
    is_preflight,
    preflight_headers,
    served_headers,
)

HTML = [("content-type", "text/html; charset=utf-8")]
JS = [("content-type", "text/javascript")]
CAPTURED = "2026-03-04T05:06:07Z"

# The page fetches another origin with credentials and a custom header, so a
# preflight is needed. The outcome is the title and the height of #r.
CORS_PAGE = b"""<!DOCTYPE html><body style="margin:0"><div id=r style="height:30px"></div>
<script>
fetch('https://b.test/data', {credentials: 'include', headers: {'X-Custom': '1'}})
  .then((r) => r.text())
  .then((t) => { document.title = 'ok ' + t; r.style.height = '77px'; },
        (e) => { document.title = 'err'; r.style.height = '11px'; });
</script>
"""


def make_fixture(path: Path, entries: dict[str, tuple[list[tuple[str, str]], bytes]]) -> Path:
    """Writes a fixture of GET entries: url to (headers, body)."""
    fixture = Fixture(path)
    for url, (headers, body) in entries.items():
        fixture.record("GET", url, 200, headers, body)
    fixture.save()
    return path


def make_cors_fixture(path: Path) -> Path:
    return make_fixture(
        path,
        {"https://a.test/": (HTML, CORS_PAGE), "https://b.test/data": ([], b"DATA")},
    )


# --- Routing: CORS ------------------------------------------------------------


def entry(headers: tuple[tuple[str, str], ...] = ()) -> Entry:
    return Entry("GET", "https://b.test/x", 200, headers, "files/x.bin")


def test_header_value_ignores_case():
    assert header_value({"Origin": "https://a.test"}, "origin") == "https://a.test"
    assert header_value({"origin": "https://a.test"}, "Origin") == "https://a.test"
    assert header_value({}, "origin") is None


def test_served_headers_without_origin_keep_the_wildcard():
    headers = served_headers(entry((("content-type", "text/css"),)))
    assert headers == [
        {"name": "content-type", "value": "text/css"},
        {"name": "access-control-allow-origin", "value": "*"},
    ]


def test_served_headers_with_origin_reflect_it_with_credentials():
    stored = (("content-type", "text/css"), ("location", "/y"))
    headers = served_headers(entry(stored), "https://a.test")
    pairs = {h["name"]: h["value"] for h in headers}
    assert pairs["access-control-allow-origin"] == "https://a.test"
    assert pairs["access-control-allow-credentials"] == "true"
    assert pairs["vary"] == "Origin"
    assert pairs["access-control-expose-headers"] == "content-type, location"
    assert pairs["content-type"] == "text/css"
    assert cors_headers("null", entry())[0] == ("access-control-allow-origin", "null")


def test_stored_cors_headers_are_replaced_not_duplicated():
    stored = (
        ("content-type", "text/css"),
        ("access-control-allow-origin", "https://a.test"),
        ("Access-Control-Allow-Credentials", "true"),
    )
    for origin in (None, "https://a.test"):
        names = [h["name"].lower() for h in served_headers(entry(stored), origin)]
        assert names.count("access-control-allow-origin") == 1
        assert names.count("access-control-allow-credentials") == names.count("vary")


def test_preflight_detection_and_answer():
    preflight = {
        "method": "OPTIONS",
        "url": "https://b.test/data",
        "headers": {
            "Origin": "https://a.test",
            "Access-Control-Request-Method": "PUT",
            "Access-Control-Request-Headers": "x-custom, content-type",
        },
    }
    assert is_preflight(preflight)
    assert not is_preflight({**preflight, "method": "GET"})
    assert not is_preflight({"method": "OPTIONS", "url": "https://b.test/", "headers": {}})
    answer = {h["name"]: h["value"] for h in preflight_headers(preflight)}
    assert answer == {
        "access-control-allow-origin": "https://a.test",
        "access-control-allow-credentials": "true",
        "access-control-allow-methods": "PUT",
        "access-control-allow-headers": "x-custom, content-type",
        "vary": "Origin",
    }


@pytest.mark.usefixtures("chromium")
def test_replay_answers_a_preflight_and_a_credentialed_request(tmp_path):
    """A cross-origin `fetch` with `credentials: 'include'` and a custom
    header needs a preflight answer and a reflected origin. Neither is in
    the fixture; `b.test` does not resolve, so a preflight that reached the
    network would fail the `fetch`."""
    fixture = make_cors_fixture(tmp_path / "fixture")

    async def run() -> tuple[str, list[str]]:
        replayer = Replayer(Fixture(fixture))
        async with browser.session((400, 300), javascript=True) as (_, context, page):
            await replayer.attach(context, page)
            await page.goto("https://a.test/", wait_until="load")
            await page.wait_for_function("document.title !== ''")
            return await page.title(), replayer.missing

    title, missing = asyncio.run(run())
    assert title == "ok DATA"
    assert missing == []


# --- Routing: capture ------------------------------------------------------------


class _Site(BaseHTTPRequestHandler):
    """Two origins (two ports) share this handler and the `seen` list. The
    page on one fetches the other with a custom header, then reports the
    outcome to its own origin."""

    seen: ClassVar[list[str]] = []
    other_port = 0

    def do_OPTIONS(self) -> None:
        self.seen.append(f"OPTIONS {self.path}")
        self.send_response(500)
        self.end_headers()

    def do_GET(self) -> None:
        self.seen.append(f"GET {self.path}")
        if self.path == "/":
            body = (
                "<!DOCTYPE html><script>"
                f"fetch('http://127.0.0.1:{self.other_port}/data',"
                " {headers: {'X-Custom': '1'}}).then((r) => r.text())"
                ".then((t) => fetch('/done?' + t), () => fetch('/done?failed'));"
                "</script>"
            ).encode()
            kind = "text/html"
        else:
            body = b"DATA"
            kind = "text/plain"
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        pass


@pytest.fixture
def two_sites() -> Iterator[tuple[str, str, list[str]]]:
    seen: list[str] = []
    handler = type("Site", (_Site,), {"seen": seen})
    servers = [ThreadingHTTPServer(("127.0.0.1", 0), handler) for _ in range(2)]
    first, second = (server.server_address[1] for server in servers)
    handler.other_port = second
    for server in servers:
        threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{first}", f"http://127.0.0.1:{second}", seen
    for server in servers:
        server.shutdown()


@pytest.mark.usefixtures("chromium")
def test_capture_answers_a_preflight_locally_and_records_no_options(tmp_path, two_sites):
    first, second, seen = two_sites
    fixture = Fixture(tmp_path)
    asyncio.run(chromium_pass(fixture, f"{first}/", False, False, javascript=True))
    # The preflight never reached a server; the credentialed request passed
    # because the response got the reflected origin.
    assert not [line for line in seen if line.startswith("OPTIONS")]
    assert "GET /done?DATA" in seen
    assert all(method == "GET" for _, method in fixture.entries)
    assert (f"{second}/data", "GET") in fixture.entries


# --- Replay pattern --------------------------------------------------------------


def test_lookup_uses_the_pattern_only_for_missing_urls(tmp_path):
    base = "https://www.example.test/static/d"
    make_fixture(
        tmp_path,
        {
            f"{base}/5/www/manifest.js": (JS, b"five"),
            f"{base}/12/www/manifest.js": (JS, b"twelve"),
            f"{base}/5/www/other.js": (JS, b"other"),
        },
    )
    (tmp_path / "fixture.json").write_text(
        json.dumps({"replay_patterns": [f"{base}/*/www/manifest.js"]})
    )
    fixture = Fixture(tmp_path)
    # A recorded URL wins.
    exact = fixture.lookup(f"{base}/5/www/manifest.js")
    assert exact is not None and fixture.read_body(exact) == b"five"
    # A new version gets the entry with the greatest URL of the pattern.
    replayed = fixture.lookup(f"{base}/99999/www/manifest.js")
    assert replayed is not None and fixture.read_body(replayed) == b"twelve"
    # `*` is one path segment, the rest is literal, the whole URL must match.
    assert fixture.lookup(f"{base}/1/2/www/manifest.js") is None
    assert fixture.lookup(f"{base}//www/manifest.js") is None
    assert fixture.lookup(f"{base}/9/www/manifest.js?v=1") is None
    assert fixture.lookup(f"{base}/9/www/unknown.js") is None
    assert fixture.lookup(f"{base}/9/www/manifest.js", "POST") is None
    # `get` is exact: capture does not use the pattern.
    assert fixture.get(f"{base}/99999/www/manifest.js") is None


def test_lookup_without_patterns_is_exact(tmp_path):
    make_fixture(tmp_path, {"https://a.test/x": (JS, b"x")})
    fixture = Fixture(tmp_path)
    assert fixture.lookup("https://a.test/x") is not None
    assert fixture.lookup("https://a.test/y") is None


def test_bad_replay_patterns_are_rejected(tmp_path):
    make_fixture(tmp_path, {"https://a.test/x": (JS, b"x")})
    (tmp_path / "fixture.json").write_text(json.dumps({"replay_patterns": "https://a.test/*"}))
    with pytest.raises(ValueError, match="replay_patterns"):
        Fixture(tmp_path)


@pytest.mark.usefixtures("chromium")
def test_replay_serves_a_url_by_pattern(tmp_path):
    page = b"<!DOCTYPE html><script src=/static/d/999/app.js></script>"
    make_fixture(
        tmp_path,
        {
            "https://a.test/": (HTML, page),
            "https://a.test/static/d/1/app.js": (JS, b"document.title = 'version 1'"),
        },
    )
    (tmp_path / "fixture.json").write_text(
        json.dumps({"replay_patterns": ["https://a.test/static/d/*/app.js"]})
    )

    async def run() -> tuple[str, list[str]]:
        replayer = Replayer(Fixture(tmp_path))
        async with browser.session((400, 300), javascript=True) as (_, context, page):
            await replayer.attach(context, page)
            await page.goto("https://a.test/", wait_until="load")
            return await page.title(), replayer.missing

    assert asyncio.run(run()) == ("version 1", [])


# --- Fixed start time ------------------------------------------------------------

_DATE_CHECKS = """async () => {
  const before = Date.now();
  await new Promise((resolve) => setTimeout(resolve, 300));
  return {
    start: new Date().toISOString().slice(0, 16),
    flows: Date.now() - before >= 290,
    string: typeof Date(),
    call: Date(1).slice(0, 15) === new Date().toString().slice(0, 15),
    zero: new Date(0).getTime(),
    parts: new Date(2020, 0, 2).getDate(),
    utc: Date.UTC(2000, 0, 1),
    parsed: Date.parse('2000-01-01T00:00:00Z'),
    instance: new Date() instanceof Date,
    constructor: Date.prototype.constructor === Date,
    subclass: new (class extends Date {})() instanceof Date,
    timer: setTimeout.toString().includes('[native code]'),
  };
}"""


@pytest.mark.usefixtures("chromium")
def test_start_time_sets_the_page_clock_and_nothing_else():
    start = datetime(2026, 3, 4, 5, 6, 7, tzinfo=UTC)

    async def run() -> dict[str, object]:
        async with browser.session((400, 300), javascript=True, start_time=start) as (_, _, page):
            await page.goto("data:text/html,<p>x")
            return await page.evaluate(_DATE_CHECKS)

    assert asyncio.run(run()) == {
        "start": "2026-03-04T05:06",
        "flows": True,
        "string": "string",
        "call": True,
        "zero": 0,
        "parts": 2,
        "utc": 946684800000,
        "parsed": 946684800000,
        "instance": True,
        "constructor": True,
        "subclass": True,
        "timer": True,
    }


# --- reference --js and compare --js -----------------------------------------------


@pytest.fixture
def js_repo(tmp_path, monkeypatch) -> Path:
    """A repository root with the JavaScript fixture `tiny`."""
    fonts = paths.fonts_conf().parent
    monkeypatch.setenv("SWB_ROOT", str(tmp_path))
    (tmp_path / "fixtures").mkdir()
    (tmp_path / "fixtures" / "fonts").symlink_to(fonts)
    fixture = make_cors_fixture(tmp_path / "fixtures" / "js" / "tiny")
    meta = {
        "url": "https://a.test/",
        "captured": CAPTURED,
        "viewport": [60, 120],
        "chromium": "",
        "javascript": True,
    }
    (fixture / "fixture.json").write_text(json.dumps(meta))
    return tmp_path


def save_png(path: Path, size: tuple[int, int] = (60, 120)) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.new("RGB", size, (255, 255, 255)).save(path)


@pytest.mark.usefixtures("chromium")
def test_reference_js_runs_the_scripts_of_the_fixture(js_repo, capsys):
    args = cli.build_parser().parse_args(["reference", "--js", "tiny"])
    assert args.func(args) == 0
    assert "0 URLs not in fixture, 0 errors" in capsys.readouterr().out
    output = js_repo / "out" / "js-reference" / "tiny"
    dump = read_dump(output / "boxes.json")
    result = next(e for e in dump.elements if e.tag == "div")
    assert result.rect is not None and result.rect[3] == 77.0  # the fetch worked
    with Image.open(output / "screenshot.png") as shot:
        assert shot.size == (60, 120)
    # JavaScript fixtures are not in the page fixture list.
    assert not (js_repo / "fixtures" / "pages").exists()


@pytest.mark.usefixtures("chromium")
def test_reference_js_reports_misses_and_errors(js_repo):
    fixture = js_repo / "fixtures" / "js" / "tiny"
    make_fixture(
        fixture,
        {
            "https://a.test/": (
                HTML,
                b"<!DOCTYPE html><img src=/missing.png><script>null.x</script>",
            )
        },
    )
    output = js_repo / "out" / "js-reference" / "tiny"
    output.mkdir(parents=True)
    result = asyncio.run(
        render_js_fixture(
            fixture, "https://a.test/", output, (60, 120), datetime(2026, 1, 1, tzinfo=UTC)
        )
    )
    assert result.missing == ["https://a.test/missing.png"]
    assert any("exception" in error and "null" in error for error in result.errors)


def sample_dump() -> BoxDump:
    elements = [
        Element("html", (0.0, 0.0, 60.0, 120.0), None),
        Element("body", (0.0, 0.0, 60.0, 77.0), 0),
    ]
    return BoxDump("https://a.test/", (60.0, 120.0), elements)


def write_js_outputs(root: Path) -> Path:
    """The Chromium reference and swb output of `tiny` in `out/`."""
    reference = root / "out" / "js-reference" / "tiny"
    write_dump(reference / "boxes.json", sample_dump())
    save_png(reference / "screenshot.png")
    output = root / "out" / "compare" / "js-tiny"
    write_dump(output / "boxes.json", sample_dump())
    save_png(output / "screenshot.png")
    return output


def test_compare_js_no_run(js_repo, capsys):
    output = write_js_outputs(js_repo)
    scores = compare_fixture("tiny", None, 2.0, 32, Options(javascript=True))
    assert scores is not None and scores.pixels == 1.0 and scores.geometry == 1.0
    assert capsys.readouterr().out.startswith("js-tiny: geometry 1.0000")
    assert (output / "report.html").is_file() and (output / "diff.png").is_file()
    assert not (js_repo / "out" / "compare" / "tiny").exists()


def test_compare_js_update_scores_writes_only_the_js_scores(js_repo):
    write_js_outputs(js_repo)
    fixtures_scores = js_repo / "fixtures" / "scores.json"
    fixtures_scores.parent.mkdir(exist_ok=True)
    fixtures_scores.write_text("{}\n")
    args = cli.build_parser().parse_args(["compare", "--js", "tiny", "--no-run", "--update-scores"])
    assert args.func(args) == 0
    scores = json.loads((js_repo / "out" / "js-scores.json").read_text())
    assert scores["tiny"]["pixels"] == 1.0
    assert fixtures_scores.read_text() == "{}\n"


def test_compare_js_refuses_states(js_repo):
    for extra in (["--full-page"], ["--click", "a"]):
        args = cli.build_parser().parse_args(["compare", "--js", "tiny", "--no-run", *extra])
        assert args.func(args) == 2


def test_compare_js_without_reference_names_the_command(js_repo, caplog):
    assert compare_fixture("tiny", None, 2.0, 32, Options(javascript=True)) is None
    assert "swbtools reference --js tiny" in caplog.text


# --- A fake swb: flag detection, --js, --eval ----------------------------------------

FAKE_SWB = """#!/usr/bin/env python3
import json, os, shutil, sys

args = sys.argv[1:]
if "--help" in args:
    print("Options:")
    for flag in %r:
        print(f"      {flag}  text")
    sys.exit(0)
with open(os.environ["FAKE_ARGV"], "a") as log:
    log.write(json.dumps(args) + "\\n")
def value(flag):
    return args[args.index(flag) + 1]
if "--dump-boxes" in args:
    shutil.copy(os.environ["FAKE_DUMP"], value("--dump-boxes"))
if "--screenshot" in args:
    shutil.copy(os.environ["FAKE_PNG"], value("--screenshot"))
if any(arg.startswith("--eval=") for arg in args) and "FAKE_ANSWERS" in os.environ:
    sys.stdout.write(open(os.environ["FAKE_ANSWERS"]).read())
"""


@pytest.fixture
def fake(tmp_path, monkeypatch):
    """Returns `make(name, flags)`, which writes a fake swb with the given
    options in its help. The fake appends its arguments to `argv.log`, copies
    a box dump and a screenshot, and prints the lines of `answers.txt` when
    it gets `--eval`."""
    work = tmp_path / "fake"
    work.mkdir()
    write_dump(work / "dump.json", sample_dump())
    save_png(work / "shot.png")
    monkeypatch.setenv("FAKE_ARGV", str(work / "argv.log"))
    monkeypatch.setenv("FAKE_DUMP", str(work / "dump.json"))
    monkeypatch.setenv("FAKE_PNG", str(work / "shot.png"))
    monkeypatch.setenv("FAKE_ANSWERS", str(work / "answers.txt"))

    def make(name: str, flags: list[str]) -> Path:
        path = work / name
        path.write_text(FAKE_SWB % (["--headless", "--dump-boxes", *flags],))
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    make.work = work  # type: ignore[attr-defined]
    return make


def fake_argv(fake) -> list[list[str]]:
    log = fake.work / "argv.log"
    return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []


def test_supported_flags_come_from_the_help(fake):
    binary = fake("swb-a", ["--js", "--eval <EXPR>"])
    assert swb.supported_flags(binary) == {"--headless", "--dump-boxes", "--js", "--eval"}
    assert swb.supported_flags(fake("swb-b", [])) == {"--headless", "--dump-boxes"}
    assert swb.supported_flags(fake.work / "missing") == frozenset()


def test_supported_flags_are_cached_per_binary(fake):
    binary = fake("swb-c", ["--js"])
    assert "--js" in swb.supported_flags(binary)
    binary.unlink()  # a second call must not run the binary again
    binary.write_text("not a program")
    swb._FLAGS_CACHE[(str(binary), binary.stat().st_size, binary.stat().st_mtime_ns)] = frozenset()
    assert swb.supported_flags(binary) == frozenset()


def test_compare_js_runs_swb_without_js_when_it_has_no_option(js_repo, fake, capsys, caplog):
    write_js_outputs(js_repo)
    binary = fake("swb-nojs", [])
    scores = compare_fixture("tiny", binary, 2.0, 32, Options(javascript=True))
    assert scores is not None
    assert "--js" not in fake_argv(fake)[0]
    assert "swb ran without JavaScript" in capsys.readouterr().out
    assert "has no --js option" in caplog.text


def test_compare_js_passes_js_when_swb_has_it(js_repo, fake, capsys):
    write_js_outputs(js_repo)
    binary = fake("swb-js", ["--js"])
    scores = compare_fixture("tiny", binary, 2.0, 32, Options(javascript=True))
    assert scores is not None
    argv = fake_argv(fake)[0]
    assert "--js" in argv and argv[argv.index("--replay") + 1].endswith("fixtures/js/tiny")
    assert "without JavaScript" not in capsys.readouterr().out


def test_plain_compare_never_passes_js(js_repo, fake):
    pages = js_repo / "fixtures" / "pages"
    pages.mkdir()
    (js_repo / "fixtures" / "js" / "tiny").rename(pages / "tiny")
    reference = pages / "tiny" / "reference"
    write_dump(reference / "boxes.json", sample_dump())
    save_png(reference / "screenshot.png")
    binary = fake("swb-js2", ["--js"])
    assert compare_fixture("tiny", binary, 2.0, 32) is not None
    assert "--js" not in fake_argv(fake)[0]


# --- Probe: "javascript": true --------------------------------------------------------


def js_case(queries: list[Query], javascript: bool = True) -> Case:
    return Case("c", "<p>x", (200, 100), tuple(queries), (), None, javascript)


def chromium_result(*rows: Row) -> CaseResult:
    return CaseResult("c", (200, 100), list(rows))


def js_row(expression: str, value: object) -> Row:
    return Row("js", expression, {"value": value})


PAGE = Path("page.html")


def compare_js(fake, binary, case, result, answers: str | None, tmp_path) -> None:
    if answers is not None:
        (fake.work / "answers.txt").write_text(answers)
    _compare_with_swb(result, case, binary, 1.0, tmp_path / PAGE, tmp_path)


def test_probe_option_is_parsed_at_file_and_case_level():
    text = json.dumps(
        {
            "javascript": True,
            "cases": [
                {"name": "a", "html": "x", "measure": [{"js": "1"}]},
                {"name": "b", "html": "x", "measure": [{"js": "1"}], "javascript": False},
            ],
        }
    )
    a, b = parse_cases(text)
    assert a.javascript and not b.javascript
    off = parse_cases(json.dumps({"cases": [{"name": "c", "html": "x", "measure": [{"js": "1"}]}]}))
    assert not off[0].javascript
    bad = json.dumps(
        {"cases": [{"name": "d", "html": "x", "measure": [{"js": "1"}], "javascript": 1}]}
    )
    with pytest.raises(ProbeError, match="javascript must be true or false"):
        parse_cases(bad)


def test_parse_swb_value_prints_like_the_chromium_side():
    assert parse_swb_value('{"value": null}') == "null"
    assert parse_swb_value('{"value": NaN}') == "NaN"
    assert parse_swb_value('{"value": Infinity}') == "Infinity"
    assert parse_swb_value('{"value": -Infinity}') == "-Infinity"
    assert parse_swb_value('{"value": -0.0}') == "-0.0"
    assert parse_swb_value('{"value": 1}') == "1"
    assert parse_swb_value('{"value": 1e+21}') == "1e+21"
    assert parse_swb_value('{"value":[1,null,{"a":"é"}]}') == '[1, null, {"a": "é"}]'
    assert parse_swb_value('{"error": "TypeError: x is not a function"}') == (
        "error: TypeError: x is not a function"
    )
    assert parse_swb_value("garbage").startswith("invalid answer")
    assert parse_swb_value('{"value": 1, "extra": 2}').startswith("invalid answer")


def test_probe_swb_without_js_prints_not_supported(fake, tmp_path):
    binary = fake("swb-0", [])
    case = js_case([Query("boxes", "p"), Query("js", "1")])
    result = chromium_result(
        Row("boxes", "p[1]", {"tag": "p", "index": 2, "rect": (8.0, 8.0, 10.0, 10.0)}),
        js_row("1", 1),
    )
    compare_js(fake, binary, case, result, None, tmp_path)
    text = format_text([result])
    assert "c  swb    p[1]  not supported" in text
    assert "c  swb    1  not supported" in text
    assert not any_outside([result])
    assert fake_argv(fake) == []  # swb did not run


def test_probe_swb_with_js_but_without_eval(fake, tmp_path):
    binary = fake("swb-1", ["--js"])
    case = js_case([Query("boxes", "body"), Query("js", "1")])
    result = chromium_result(
        Row("boxes", "body[1]", {"tag": "body", "index": 1, "rect": (0.0, 0.0, 60.0, 77.0)}),
        js_row("1", 1),
    )
    compare_js(fake, binary, case, result, None, tmp_path)
    argv = fake_argv(fake)[0]
    assert "--js" in argv and not any(arg.startswith("--eval") for arg in argv)
    text = format_text([result])
    assert "c  swb    body[1]  x=0 y=0 w=60 h=77  dx=0 dy=0 dw=0 dh=0" in text
    assert "c  swb    1  not supported" in text
    assert not any_outside([result])


def test_probe_swb_values_are_compared(fake, tmp_path):
    binary = fake("swb-2", ["--js", "--eval"])
    queries = ["a", "b", "c", "d", "e", "f"]
    case = js_case([Query("js", q) for q in queries])
    result = chromium_result(
        js_row("a", 7),
        js_row("b", float("nan")),
        js_row("c", -0.0),
        js_row("d", None),
        js_row("e", [1, "x"]),
        Row("js", "f", {"error": "Page.evaluate: TypeError: boom"}),
    )
    answers = "\n".join(
        [
            '{"value": 8}',
            '{"value": NaN}',
            '{"value": -0.0}',
            '{"value": null}',
            '{"value": [1, "x"]}',
            '{"error": "TypeError: boom"}',
        ]
    )
    compare_js(fake, binary, case, result, answers + "\n", tmp_path)
    argv = fake_argv(fake)[0]
    assert [arg for arg in argv if arg.startswith("--eval")] == [f"--eval={q}" for q in queries]
    assert [row.swb_value.outside for row in result.rows if row.swb_value] == [
        True,
        False,
        False,
        False,
        False,
        False,
    ]
    text = format_text([result])
    assert "c  swb    a  8  !" in text
    assert "c  swb    b  NaN\n" in text
    assert text.endswith("c  swb    f  error: TypeError: boom")
    assert any_outside([result])


def test_probe_swb_answer_count_must_match(fake, tmp_path):
    binary = fake("swb-3", ["--js", "--eval"])
    case = js_case([Query("js", "1"), Query("js", "2")])
    result = chromium_result(js_row("1", 1), js_row("2", 2))
    compare_js(fake, binary, case, result, '{"value": 1}\n', tmp_path)
    assert result.swb_error is not None and "1 answers for 2" in result.swb_error
    assert any_outside([result])


def test_probe_js_off_case_keeps_js_queries_chromium_only(fake, tmp_path):
    binary = fake("swb-4", ["--js", "--eval"])
    case = js_case([Query("js", "1")], javascript=False)
    result = chromium_result(js_row("1", 1))
    compare_js(fake, binary, case, result, None, tmp_path)
    argv = fake_argv(fake)[0]
    assert "--js" not in argv and not any(arg.startswith("--eval") for arg in argv)
    assert result.rows[0].swb_value is None and not result.rows[0].swb_unsupported
    assert "swb" not in format_text([result])


@pytest.mark.usefixtures("chromium")
def test_probe_javascript_case_runs_the_scripts():
    text = json.dumps(
        {
            "cases": [
                {
                    "name": "on",
                    "javascript": True,
                    "html": "<!DOCTYPE html><style>#m { width: 10px } "
                    "@media (scripting) { #m { width: 20px } }</style>"
                    "<p id=a>original</p><div id=m>m</div>"
                    "<script>a.textContent = 'changed'</script>",
                    "measure": [
                        {"js": "document.getElementById('a').textContent"},
                        {"js": "new Promise((r) => setTimeout(() => r('later'), 10))"},
                        {"boxes": "#m"},
                        {"js": "new Date(0)"},
                    ],
                },
                {
                    "name": "off",
                    "html": "<!DOCTYPE html><style>#m { width: 10px } "
                    "@media (scripting) { #m { width: 20px } }</style>"
                    "<p id=a>original</p><div id=m>m</div>"
                    "<script>a.textContent = 'changed'</script>",
                    "measure": [
                        {"js": "document.getElementById('a').textContent"},
                        {"boxes": "#m"},
                    ],
                },
            ]
        }
    )
    cases = parse_cases(text)
    results = asyncio.run(
        in_chromium(lambda page, directory: _probe_all(cases, None, 1.0, page, directory))
    )
    on, off = results
    assert [row.data.get("value") for row in on.rows[:2]] == ["changed", "later"]
    assert on.rows[2].data["rect"][2] == 20.0
    assert "not JSON" in on.rows[3].data["error"]
    assert off.rows[0].data["value"] == "original"
    assert off.rows[1].data["rect"][2] == 10.0
