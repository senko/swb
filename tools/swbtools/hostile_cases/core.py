"""The types, the registry and the helpers that the cases share."""

import json
from collections.abc import Callable
from dataclasses import dataclass, field

MAX_PAGE_BYTES = 20_000_000
"""The most that a case may generate (the HTML and its extra files)."""

MIB = 1 << 20

JS = ("--js",)
"""The swb options of a page with scripts."""

REPLAY = ("--js", "--replay", "{dir}")
"""Scripts, and the files of the case as a replayed site."""


@dataclass(frozen=True)
class Page:
    """The generated page: `index.html` and files written next to it."""

    html: str
    files: dict[str, str | bytes] = field(default_factory=dict)

    def size(self) -> int:
        """The size of all files in bytes."""
        extra = sum(len(c.encode() if isinstance(c, str) else c) for c in self.files.values())
        return len(self.html.encode()) + extra


@dataclass(frozen=True)
class Case:
    """One hostile page with the limits that swb must keep on it."""

    name: str
    description: str
    build: Callable[[], Page]
    time_limit_s: float = 5.0
    rss_limit_mib: float = 1024
    known_failure: str | None = None
    """The reason, if swb has a known bug that this case shows."""
    expect_log: str | tuple[str, ...] | None = None
    swb_args: tuple[str, ...] = ()
    """Options for swb; `{dir}` stands for the directory of the case."""
    url: str | None = None
    """The URL to open instead of `index.html` (with `--replay {dir}`: a page
    of an `http:` origin, which cookies need)."""
    forbid_log: str | None = None
    """A part of a warning that swb must not log: the case shows that a limit
    does not act too early (`error` if the log has it)."""
    server: str | None = None
    """`never` or `slow`: the runner starts a local server that holds each
    request without an answer, or answers it after 2 s; the page reaches it
    at `http://127.0.0.1:__PORT__/` (feature 10a)."""


CASES: list[Case] = []
"""All cases, in the order in which they run."""


def case(
    description: str,
    time_limit_s: float = 5.0,
    rss_limit_mib: float = 1024,
    known_failure: str | None = None,
    expect_log: str | tuple[str, ...] | None = None,
    swb_args: tuple[str, ...] = (),
    url: str | None = None,
    server: str | None = None,
    forbid_log: str | None = None,
) -> Callable[[Callable[[], Page]], Callable[[], Page]]:
    """Registers the decorated generator as a case."""

    def register(build: Callable[[], Page]) -> Callable[[], Page]:
        name = build.__name__.removeprefix("_").replace("_", "-")
        CASES.append(
            Case(
                name,
                description,
                build,
                time_limit_s,
                rss_limit_mib,
                known_failure,
                expect_log,
                swb_args,
                url,
                forbid_log,
                server,
            )
        )
        return build

    return register


def doc(body: str, css: str = "") -> str:
    """A standards-mode document with a style sheet."""
    return f"<!doctype html><meta charset=utf-8><style>{css}</style><body style='margin:0'>{body}"


def nest(open_tag: str, close_tag: str, depth: int, inner: str = "") -> str:
    """`inner` inside `depth` levels of `open_tag` ... `close_tag`."""
    return open_tag * depth + inner + close_tag * depth


def img_page(svg: str, size: str = "width=200 height=200") -> Page:
    """A page with one SVG image from the file `img.svg`."""
    return Page(doc(f"<img src=img.svg {size}>"), {"img.svg": svg})


SVG_NS = "xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink'"


REPLAY_URL = "http://hostile.test/index.html"


WEB_FONT_FILE = "crates/text/tests/webfonts/dejavu-subset.ttf"
"""A small valid TrueType font (a DejaVu Sans subset) in the repository."""


def web_font() -> bytes:
    from swbtools import paths

    return (paths.repo_root() / WEB_FONT_FILE).read_bytes()


def stripes_png(width: int, height: int) -> bytes:
    """An RGB PNG of `width` x `height` with 8-pixel stripes (small as a file, `width * height`
    pixels once decoded)."""
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    stripe = [b"\xff\x00\x00" if (x // 8) % 2 else b"\x00\x00\xff" for x in range(width)]
    row = b"\x00" + b"".join(stripe)
    packer = zlib.compressobj(6)
    packed = b"".join(packer.compress(row) for _ in range(height)) + packer.flush()
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    ihdr, idat, iend = chunk(b"IHDR", header), chunk(b"IDAT", packed), chunk(b"IEND", b"")
    return b"\x89PNG\r\n\x1a\n" + ihdr + idat + iend


def steps_args(steps: list[dict[str, object]]) -> tuple[str, ...]:
    """The swb arguments of a case that sends input after the load."""
    return ("--js", f"--steps={json.dumps(steps)}")


def origins_page(html: str, documents: dict[str, str]) -> Page:
    """A replayed page at `http://hostile.test/index.html` with documents at other URLs."""
    entries = [
        {
            "method": "GET",
            "url": REPLAY_URL,
            "status": 200,
            "headers": [["content-type", "text/html; charset=utf-8"]],
            "body": "files/index.html",
        }
    ]
    files: dict[str, str | bytes] = {"files/index.html": html}
    for index, (url, text) in enumerate(documents.items()):
        entries.append(
            {
                "method": "GET",
                "url": url,
                "status": 200,
                "headers": [["content-type", "text/html; charset=utf-8"]],
                "body": f"files/d{index}.html",
            }
        )
        files[f"files/d{index}.html"] = text
    files["manifest.json"] = json.dumps({"version": 1, "entries": entries})
    return Page(html, files)
