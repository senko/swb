"""`swbtools substitute`: placeholders for images, free fonts for others."""

import io
from pathlib import Path

import pytest
from fontTools.ttLib import TTFont
from PIL import Image

from swbtools import paths, substitute
from swbtools.manifest import Fixture
from swbtools.substitute import (
    check_fixture,
    is_free_font,
    replacement_font,
    substitute_fixture,
)

FONTS = paths.fixtures_dir() / "fonts"


def image_bytes(fmt: str, size: tuple[int, int], mode: str = "RGB") -> bytes:
    output = io.BytesIO()
    Image.new(mode, size, (200, 10, 10, 128)[: len(mode)]).save(output, fmt)
    return output.getvalue()


def font_bytes(source: Path, flavor: str | None, license_text: str | None) -> bytes:
    font = TTFont(source)
    if license_text is not None:
        names = font["name"]
        for record in list(names.names):
            if record.nameID in (13, 14):
                names.removeNames(nameID=record.nameID)
        names.setName(license_text, 13, 3, 1, 0x409)
    font.flavor = flavor
    output = io.BytesIO()
    font.save(output)
    return output.getvalue()


def make_fixture(directory: Path, responses: dict[str, tuple[str, bytes]]) -> Fixture:
    fixture = Fixture(directory)
    for url, (content_type, body) in responses.items():
        fixture.record("GET", url, 200, [("Content-Type", content_type)], body)
    fixture.save()
    return fixture


def bodies(directory: Path) -> dict[str, bytes]:
    fixture = Fixture(directory)
    return {url: fixture.read_body(entry) for (url, _), entry in fixture.entries.items()}


def test_images_keep_size_and_format_and_lose_their_pixels(tmp_path: Path) -> None:
    jpeg = image_bytes("JPEG", (120, 80))
    png = image_bytes("PNG", (33, 7), "RGBA")
    webp = image_bytes("WEBP", (64, 64))
    make_fixture(
        tmp_path,
        {
            "https://a.test/photo.jpg": ("image/jpeg", jpeg),
            "https://a.test/logo.png": ("image/png", png),
            "https://a.test/pic.webp": ("image/webp; q=1", webp),
            "https://a.test/": ("text/html", b"<img src=photo.jpg>"),
        },
    )
    assert substitute_fixture(tmp_path) == (3, 0)
    after = bodies(tmp_path)
    assert after["https://a.test/"] == b"<img src=photo.jpg>"
    for url, original, fmt, size, mode in [
        ("https://a.test/photo.jpg", jpeg, "JPEG", (120, 80), "RGB"),
        ("https://a.test/logo.png", png, "PNG", (33, 7), "RGBA"),
        ("https://a.test/pic.webp", webp, "WEBP", (64, 64), "RGB"),
    ]:
        assert after[url] != original
        with Image.open(io.BytesIO(after[url])) as image:
            assert (image.format, image.size, image.mode) == (fmt, size, mode)
    # Only the files that entries use remain.
    assert len(list((tmp_path / "files").iterdir())) == 4


def test_non_free_fonts_become_dejavu_in_the_same_format(tmp_path: Path) -> None:
    proprietary = font_bytes(FONTS / "LiberationSerif-Bold.ttf", "woff2", "Commercial EULA")
    free = font_bytes(FONTS / "LiberationSans-Regular.ttf", "woff2", None)
    make_fixture(
        tmp_path,
        {
            "https://a.test/Brand-Serif-Bd.woff2": ("font/woff2", proprietary),
            "https://a.test/free.woff2": ("font/woff2", free),
            "https://a.test/broken.ttf": ("font/ttf", b"not a font"),
        },
    )
    assert substitute_fixture(tmp_path) == (0, 2)
    after = bodies(tmp_path)
    assert after["https://a.test/free.woff2"] == free
    replaced = TTFont(io.BytesIO(after["https://a.test/Brand-Serif-Bd.woff2"]))
    assert replaced.flavor == "woff2"
    assert replaced["name"].getDebugName(4) == "DejaVu Sans Bold"
    assert is_free_font(replaced)
    broken = TTFont(io.BytesIO(after["https://a.test/broken.ttf"]))
    assert broken.flavor is None
    assert broken["name"].getDebugName(4) == "DejaVu Sans"


def test_a_second_run_changes_nothing(tmp_path: Path) -> None:
    make_fixture(
        tmp_path,
        {
            "https://a.test/a.jpg": ("image/jpeg", image_bytes("JPEG", (40, 30))),
            "https://a.test/b.gif": ("image/gif", image_bytes("GIF", (10, 10))),
            "https://a.test/c.png": ("image/png", image_bytes("PNG", (9, 9), "RGBA")),
            "https://a.test/f.woff2": (
                "font/woff2",
                font_bytes(FONTS / "LiberationMono-Regular.ttf", "woff2", "Proprietary"),
            ),
        },
    )
    substitute_fixture(tmp_path)
    first = bodies(tmp_path)
    assert substitute_fixture(tmp_path) == (0, 0)
    assert bodies(tmp_path) == first


def test_replacement_font_follows_the_weight() -> None:
    assert replacement_font(TTFont(FONTS / "LiberationSerif-Bold.ttf")).name == (
        "DejaVuSans-Bold.ttf"
    )
    assert replacement_font(TTFont(FONTS / "LiberationSans-Italic.ttf")).name == "DejaVuSans.ttf"
    assert replacement_font(None).name == "DejaVuSans.ttf"


def test_images_are_found_by_their_first_bytes(tmp_path: Path) -> None:
    jpeg = image_bytes("JPEG", (20, 10))
    make_fixture(
        tmp_path,
        {
            "https://a.test/no-type": ("application/octet-stream", jpeg),
            "https://a.test/a.gif": ("image/gif", image_bytes("GIF", (5, 6))),
            "https://a.test/a.bmp": ("image/bmp", image_bytes("BMP", (7, 8))),
            "https://a.test/a.ico": ("image/x-icon", image_bytes("ICO", (16, 16), "RGBA")),
        },
    )
    assert substitute_fixture(tmp_path) == (4, 0)
    after = bodies(tmp_path)
    assert after["https://a.test/no-type"] != jpeg
    for url, fmt, size in [
        ("https://a.test/no-type", "JPEG", (20, 10)),
        ("https://a.test/a.gif", "GIF", (5, 6)),
        ("https://a.test/a.bmp", "BMP", (7, 8)),
        ("https://a.test/a.ico", "ICO", (16, 16)),
    ]:
        with Image.open(io.BytesIO(after[url])) as image:
            assert (image.format, image.size) == (fmt, size)
    assert check_fixture(tmp_path) == []


def test_the_check_reports_what_cannot_be_replaced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(substitute, "MAX_PIXELS", 100)
    make_fixture(
        tmp_path,
        {
            "https://a.test/broken.png": ("image/png", b"\x89PNG\r\n\x1a\nbroken"),
            "https://a.test/big.png": ("image/png", image_bytes("PNG", (20, 20))),
            "https://a.test/a.avif": ("image/avif", b"\x00\x00\x00\x1cftypavif" + bytes(20)),
            "https://a.test/odd": ("image/x-odd", b"????"),
            "https://a.test/s.css": ("text/css", b"a{background:url(data:image/png;base64,AA)}"),
            "https://a.test/i.svg": ("image/svg+xml", b"<svg><image href='data:image/svg+xml,x'/>"),
            "https://a.test/ok.css": ("text/css", b"a{color:red}"),
        },
    )
    assert substitute_fixture(tmp_path) == (0, 0)
    assert check_fixture(tmp_path) == [
        "https://a.test/a.avif: image format that the tool cannot replace",
        "https://a.test/big.png: image that is not a placeholder"
        " (unreadable, too large or not replaced)",
        "https://a.test/broken.png: image that is not a placeholder"
        " (unreadable, too large or not replaced)",
        "https://a.test/odd: image of an unknown format",
        "https://a.test/s.css: raster image in a data: URL",
    ]


def test_placeholders_do_not_depend_on_the_band_size(monkeypatch: pytest.MonkeyPatch) -> None:
    whole = substitute.placeholder(50, 70, "x", False).tobytes()
    monkeypatch.setattr(substitute, "BAND", 3)
    assert substitute.placeholder(50, 70, "x", False).tobytes() == whole
