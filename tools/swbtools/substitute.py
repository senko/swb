"""`swbtools substitute NAME`: replace copyrighted content in a page fixture.

The repository is public. Photos on news pages and commercial web fonts
must not be published with it. This command keeps the fixture usable for
comparisons and removes that content:

- Every raster image (JPEG, PNG, WebP, GIF, BMP, ICO) becomes a generated
  placeholder of the same pixel size and format: a gradient with a 32 px
  grid, colored by the URL. Layout does not depend on the pixels, and the
  grid shows scaling and position errors in the pixel comparison.
- Every font whose `name` table does not give a free license (SIL Open
  Font License, Apache License, the Bitstream Vera license) becomes the
  bundled DejaVu Sans (Bold for weight 600 or more) in the same format
  (WOFF2, WOFF or plain sfnt). DejaVu Sans, not Liberation: the test
  fonts map the generic families and Arial, Helvetica and Times to
  Liberation, so a page's fallback font is Liberation, and a browser that
  ignored the web font would still match the reference.

Images are found by content type and by their first bytes, so an image
with a wrong content type is replaced too. Animated images become one
frame, and an ICO file one image of its largest size.

After the replacement, `check_fixture` looks at every body again and
reports what is still not allowed: an image that is not a placeholder
(Pillow cannot read it, or it is larger than `MAX_PIXELS`), an image
format that Pillow cannot write (AVIF, TIFF, JPEG XL), a font without a
free license, or a raster `data:` URL in a text body (HTML, CSS, SVG,
JavaScript). The command then fails. The license check trusts the
font's own `name` table.

Run it after `capture` or `capture-missing` and before `reference`. It is
idempotent: placeholders and DejaVu fonts stay as they are. HTML, CSS and
SVG files are not changed.
"""

import hashlib
import io
import logging
import re
from dataclasses import replace
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont
from PIL import Image

from swbtools import paths
from swbtools.manifest import FILES_DIR, Entry, Fixture, body_file_name

log = logging.getLogger(__name__)

IMAGE_FORMATS = {
    "image/jpeg": "JPEG",
    "image/png": "PNG",
    "image/webp": "WEBP",
    "image/gif": "GIF",
    "image/bmp": "BMP",
    "image/x-icon": "ICO",
    "image/vnd.microsoft.icon": "ICO",
}
"""Content type essences of the raster images that get placeholders, and
the Pillow format names."""

FONT_TYPES = ("font/", "application/font", "application/x-font", "application/vnd.ms-fontobject")

FONT_MAGIC = (b"wOF2", b"wOFF", b"\x00\x01\x00\x00", b"OTTO", b"true", b"ttcf")

TEXT_TYPES = ("text/", "image/svg+xml", "application/javascript", "application/json")

RASTER_DATA_URL = re.compile(rb"data:image/(?!svg)[a-z.+-]+", re.IGNORECASE)
"""A `data:` URL of a raster image in a text body."""

FREE_FONT_LICENSES = (
    "scripts.sil.org/ofl",
    "openfontlicense.org",
    "open font license",
    "apache.org/licenses",
    "apache license",
    "bitstream vera",
)
"""Lowercase text in a font's license name records (IDs 13 and 14) that
marks a license that allows redistribution."""

GRID = 32
"""Grid spacing of the placeholders in px."""

BAND = 256
"""Rows of a placeholder computed at a time (bounds the memory)."""

MAX_PIXELS = 100_000_000
"""Larger images are not replaced; `check_fixture` reports them."""


def content_type(entry: Entry) -> str:
    """The essence of the entry's `Content-Type` (lowercase, no parameters)."""
    for name, value in entry.headers:
        if name == "content-type":
            return value.split(";", 1)[0].strip().lower()
    return ""


def sniff_image(body: bytes) -> str | None:
    """The image format from the first bytes: a Pillow format name, or
    `"unsupported"` for AVIF, HEIF, TIFF and JPEG XL; None for others."""
    if body.startswith(b"\xff\xd8\xff"):
        return "JPEG"
    if body.startswith(b"\x89PNG\r\n\x1a\n"):
        return "PNG"
    if body.startswith((b"GIF87a", b"GIF89a")):
        return "GIF"
    if body[:4] == b"RIFF" and body[8:12] == b"WEBP":
        return "WEBP"
    if body.startswith(b"BM") and body[14:18] in (
        bytes([n, 0, 0, 0]) for n in (12, 40, 52, 56, 108, 124)
    ):
        return "BMP"
    if body.startswith(b"\x00\x00\x01\x00"):
        return "ICO"
    if body[4:8] == b"ftyp" and body[8:12] in (b"avif", b"avis", b"heic", b"heix", b"mif1"):
        return "unsupported"
    if body.startswith((b"II*\x00", b"MM\x00*", b"\xff\x0a", b"\x00\x00\x00\x0cJXL ")):
        return "unsupported"
    return None


def image_format(entry: Entry, body: bytes) -> str | None:
    """The format of the placeholder for an entry: the content type's
    format, else the sniffed one. `"unsupported"` or None as in
    `sniff_image`."""
    sniffed = sniff_image(body)
    if sniffed == "unsupported":
        return sniffed
    return IMAGE_FORMATS.get(content_type(entry)) or sniffed


def is_font(entry: Entry, body: bytes) -> bool:
    """True if the entry is a font, by content type or first bytes."""
    return content_type(entry).startswith(FONT_TYPES) or body.startswith(FONT_MAGIC)


def placeholder(width: int, height: int, seed: str, alpha: bool) -> Image.Image:
    """A gradient with a grid, colored by a hash of `seed`. Computed in
    bands of rows."""
    digest = hashlib.sha256(seed.encode("utf-8")).digest()
    base = np.array([digest[0], digest[1], digest[2]], dtype=np.float32) * 0.5 + 64.0
    x = np.linspace(0.0, 1.0, width, dtype=np.float32)[None, :, None]
    ys = np.linspace(0.0, 1.0, height, dtype=np.float32)
    columns = (np.arange(width) % GRID == 0)[None, :]
    pixels = np.empty((height, width, 3), dtype=np.uint8)
    for top in range(0, height, BAND):
        y = ys[top : top + BAND][:, None, None]
        band = base + 48.0 * x - 48.0 * y + np.zeros((len(y), width, 3), dtype=np.float32)
        rows = (np.arange(top, top + len(y)) % GRID == 0)[:, None]
        band[columns | rows] *= 0.6
        pixels[top : top + len(y)] = np.clip(band, 0, 255).astype(np.uint8)
    image = Image.fromarray(pixels, "RGB")
    return image.convert("RGBA") if alpha else image


def substitute_image(body: bytes, pil_format: str, seed: str) -> bytes | None:
    """The placeholder for an image body in `pil_format`, or None if Pillow
    cannot read the image size or the image has more than `MAX_PIXELS`."""
    try:
        with Image.open(io.BytesIO(body)) as original:
            width, height = original.size
            alpha = original.mode in ("RGBA", "LA", "PA") or "transparency" in original.info
    except Exception as error:  # Pillow raises many types for bad data.
        log.warning("cannot read image (%s)", error)
        return None
    if width * height > MAX_PIXELS:
        return None
    image = placeholder(width, height, seed, alpha)
    output = io.BytesIO()
    if pil_format == "JPEG":
        # Low quality keeps the fixture small; the grid stays visible.
        image.convert("RGB").save(output, "JPEG", quality=40)
    elif pil_format == "ICO":
        image.save(output, "ICO", sizes=[(width, height)])
    else:
        image.save(output, pil_format)
    return output.getvalue()


def license_texts(font: TTFont) -> list[str]:
    """The lowercase license description and URL records of a font."""
    return [
        record.toUnicode().lower() for record in font["name"].names if record.nameID in (13, 14)
    ]


def is_free_font(font: TTFont) -> bool:
    """True if the font's license records name a free license."""
    return any(marker in text for text in license_texts(font) for marker in FREE_FONT_LICENSES)


def has_free_license(body: bytes) -> bool:
    """True if fontTools reads the font and it has a free license."""
    try:
        return is_free_font(TTFont(io.BytesIO(body)))
    except Exception:  # fontTools raises many types for bad data.
        return False


def replacement_font(font: TTFont | None) -> Path:
    """DejaVu Sans, or DejaVu Sans Bold if `font` has a weight class of
    600 or more."""
    os2 = None if font is None else font.get("OS/2")
    bold = os2 is not None and os2.usWeightClass >= 600
    return paths.fixtures_dir() / "fonts" / ("DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf")


def substitute_font(body: bytes, url: str) -> bytes | None:
    """The replacement font in the format of `body`, or None if the font
    has a free license. Fonts that fontTools cannot read are replaced in
    sfnt format."""
    try:
        font = TTFont(io.BytesIO(body))
        if is_free_font(font):
            return None
        flavor = font.flavor
        source = replacement_font(font)
    except Exception as error:  # fontTools raises many types for bad data.
        log.warning("cannot read font %s (%s); replaced", url, error)
        flavor = None
        source = replacement_font(None)
    replacement = TTFont(source)
    replacement.flavor = flavor
    output = io.BytesIO()
    replacement.save(output)
    return output.getvalue()


def substitute_fixture(directory: Path) -> tuple[int, int]:
    """Replaces the images and non-free fonts of a fixture. Returns the
    number of replaced images and fonts."""
    fixture = Fixture(directory)
    images = fonts = 0
    for key, entry in sorted(fixture.entries.items()):
        body = fixture.read_body(entry)
        pil_format = image_format(entry, body)
        if pil_format is not None and pil_format != "unsupported":
            new = substitute_image(body, pil_format, entry.url)
        elif pil_format is None and is_font(entry, body):
            new = substitute_font(body, entry.url)
        else:
            continue
        if new is None or new == body:
            continue
        content_types = [value for name, value in entry.headers if name == "content-type"]
        file_name = body_file_name(new, content_types)
        (directory / FILES_DIR / file_name).write_bytes(new)
        fixture.entries[key] = replace(entry, body=f"{FILES_DIR}/{file_name}")
        if pil_format is not None:
            images += 1
        else:
            fonts += 1
    fixture.save()
    for unused in fixture.unused_files():
        unused.unlink()
    return images, fonts


def check_entry(entry: Entry, body: bytes) -> str | None:
    """Why the body of an entry may not be published, or None."""
    pil_format = image_format(entry, body)
    essence = content_type(entry)
    if pil_format == "unsupported":
        return "image format that the tool cannot replace"
    if pil_format is not None:
        if substitute_image(body, pil_format, entry.url) != body:
            return "image that is not a placeholder (unreadable, too large or not replaced)"
        return None
    if is_font(entry, body):
        return None if has_free_license(body) else "font without a free license"
    if essence.startswith("image/") and essence != "image/svg+xml":
        return "image of an unknown format"
    if essence.startswith(TEXT_TYPES) and RASTER_DATA_URL.search(body):
        return "raster image in a data: URL"
    return None


def check_fixture(directory: Path) -> list[str]:
    """The entries of a fixture that may not be published, as
    `URL: reason` lines."""
    fixture = Fixture(directory)
    problems = []
    for entry in sorted(fixture.entries.values(), key=lambda e: e.key):
        reason = check_entry(entry, fixture.read_body(entry))
        if reason is not None:
            problems.append(f"{entry.url}: {reason}")
    return problems


def substitute(names: list[str], check_only: bool = False) -> int:
    """Runs `substitute_fixture` (unless `check_only`) and `check_fixture`
    on the named fixtures. Returns 1 if a fixture still has content that
    may not be published."""
    status = 0
    for name in names:
        directory = paths.fixture_dir(name)
        if not check_only:
            images, fonts = substitute_fixture(directory)
            print(f"{name}: {images} images and {fonts} fonts replaced")
        for problem in check_fixture(directory):
            print(f"{name}: {problem}")
            status = 1
    return status
