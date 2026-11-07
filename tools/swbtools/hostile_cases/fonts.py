"""Web fonts, font settings, metric descriptors and `local()` (ADR 0022)."""

from swbtools.hostile_cases.core import (
    Page,
    case,
    doc,
    web_font,
)

# --- Web fonts (ADR 0022) --------------------------------------------------


@case(
    "100,000 @font-face rules in one family with one code point each, past the limits of"
    " 10,000 rules and 1,000 faces per family (ADR 0022)",
    expect_log="@font-face rules; ignoring the rest",
)
def font_face_many_rules() -> Page:
    rules = "".join(
        f"@font-face{{font-family:F;src:url(f{i}.woff2);unicode-range:U+{i:X}}}"
        for i in range(100_000)
    )
    text = "".join(chr(0x21 + i % 0x5E) for i in range(5_000))
    return Page(doc(f"<p style='font-family:F'>{text}</p>", rules))


@case("an @font-face rule with 300,000 unicode-range entries, used by long text (ADR 0022)")
def font_face_long_unicode_range() -> Page:
    ranges = ",".join(f"U+{i * 3:X}-{i * 3 + 1:X}" for i in range(300_000))
    rules = f"@font-face{{font-family:R;src:url(f.ttf);unicode-range:{ranges}}}"
    text = "".join(chr(0x21 + i % 0x5E) for i in range(20_000))
    return Page(doc(f"<p style='font-family:R'>{text}</p>", rules), {"f.ttf": web_font()})


@case(
    "5,000 web font families in use, each with its own font URL, past the limit of 1,000"
    " faces loaded (ADR 0022)",
    time_limit_s=10,
    expect_log="web font faces; not loading more",
)
def font_families_many() -> Page:
    rules = "".join(f"@font-face{{font-family:F{i};src:url(f.ttf?{i})}}" for i in range(5_000))
    spans = "".join(f"<span style='font-family:F{i}'>x{i} </span>" for i in range(5_000))
    return Page(doc(spans, rules), {"f.ttf": web_font()})


@case(
    "1,000 loaded faces of one family with the same descriptors and 300,000 characters that"
    " no face has (ADR 0022)",
)
def font_composite_many_faces() -> Page:
    rules = "".join(f"@font-face{{font-family:C;src:url(f.ttf?{i})}}" for i in range(1_000))
    text = "".join(chr(0x400 + i % 0x100) for i in range(300_000))
    return Page(doc(f"<p style='font-family:C'>{text}</p>", rules), {"f.ttf": web_font()})


@case(
    "1,000 loaded faces of one family with the same descriptors and the CJK block (20,992"
    " distinct characters) that no face has; at most 256 faces are checked (ADR 0022)",
)
def font_composite_many_chars() -> Page:
    rules = "".join(f"@font-face{{font-family:C;src:url(f.ttf?{i})}}" for i in range(1_000))
    text = "".join(chr(0x4E00 + i) for i in range(0x5200))
    return Page(doc(f"<p style='font-family:C'>{text}</p>", rules), {"f.ttf": web_font()})


def _woff2_bomb() -> bytes:
    """A WOFF2 header and one table that declares a decoded size of 2^31
    bytes, with 16 bytes of data."""
    import struct

    directory = bytes([0]) + bytes([0x88, 0x80, 0x80, 0x80, 0x00])
    data = bytes(16)
    length = 48 + len(directory) + len(data)
    header = b"wOF2" + struct.pack(
        ">IIHHIIHHIIIII", 0x00010000, length, 1, 0, 0xFFFFFFFF, 16, 0, 0, 0, 0, 0, 0, 0
    )
    return header + directory + data


def _woff1_bomb() -> bytes:
    """A WOFF 1.0 file with 1,000 compressed tables that all point at the
    same small zlib stream, which expands to 40,000 bytes: the sum of
    `origLength` is 40 MB, above the limit of 32 MiB."""
    import struct
    import zlib

    tables = 1000
    data_at = 44 + 20 * tables
    original = 40_000
    blob = zlib.compress(bytes(original))
    length = data_at + len(blob)
    header = b"wOFF" + struct.pack(
        ">IIHHIHHIIIII", 0x00010000, length, tables, 0, 0xFFFFFFFF, 1, 0, 0, 0, 0, 0, 0
    )
    entries = b"".join(
        struct.pack(">4sIIII", f"t{i:03d}".encode(), data_at, len(blob), original, 0)
        for i in range(tables)
    )
    return header + entries + blob


@case("web fonts that fail to decode: garbage, truncated, decompression bombs (ADR 0022)")
def font_decode_failures() -> Page:
    good = web_font()
    files: dict[str, str | bytes] = {
        "garbage.woff2": bytes(range(256)) * 64,
        "empty.ttf": b"",
        "truncated.ttf": good[: len(good) // 3],
        "bomb.woff2": _woff2_bomb(),
        "bomb.woff": _woff1_bomb(),
        "html.ttf": "<!doctype html><p>not a font",
    }
    rules = "".join(
        f"@font-face{{font-family:D{i};src:url({name}),url(missing-{i}.ttf)}}"
        for i, name in enumerate(files)
    )
    paragraphs = "".join(f"<p style='font-family:D{i}'>text {i}</p>" for i in range(len(files)))
    return Page(doc(paragraphs, rules), files)


def _composite_bomb_font(depth: int = 14, points: int = 3000) -> bytes:
    """A TrueType font whose glyph for `A` nests composites `depth` deep,
    each with two copies of the level below, over a contour of `points`
    points (2^depth copies when expanded); `maxp` claims the largest
    values."""
    import io

    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent

    names = [".notdef", "base"] + [f"g{i}" for i in range(1, depth + 1)]
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({0x41: f"g{depth}", 0x42: "base"})
    pen = TTGlyphPen(None)
    pen.moveTo((0, 0))
    for i in range(points):
        pen.lineTo((i % 500, (i * 7) % 700))
    pen.closePath()
    glyphs = {".notdef": TTGlyphPen(None).glyph(), "base": pen.glyph()}
    for i in range(1, depth + 1):
        glyph = Glyph()
        glyph.numberOfContours = -1
        glyph.components = []
        for dx in (0, 1):
            component = GlyphComponent()
            component.glyphName = "base"
            component.x, component.y = dx, 0
            component.flags = 0x4
            glyph.components.append(component)
        glyphs[f"g{i}"] = glyph
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({n: (600, 0) for n in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({"familyName": "Bomb", "styleName": "Regular"})
    builder.setupOS2()
    builder.setupPost()
    first = io.BytesIO()
    builder.save(first)
    # Point each level at the level below without recomputing bounds:
    # fontTools itself would expand the composites.
    font = TTFont(io.BytesIO(first.getvalue()), recalcBBoxes=False)
    for i in range(2, depth + 1):
        for component in font["glyf"][f"g{i}"].components:
            component.glyphName = f"g{i - 1}"
    maxp = font["maxp"]
    maxp.maxPoints = maxp.maxCompositePoints = 0xFFFF
    maxp.maxContours = maxp.maxCompositeContours = 0xFFFF
    maxp.maxComponentElements = maxp.maxComponentDepth = 0xFFFF
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


@case("a web font with composite glyphs nested 14 deep (16,384 copies of 3,000 points) (ADR 0022)")
def font_composite_glyphs() -> Page:
    text = "AAAA BBBB " * 200
    rules = "@font-face{font-family:B;src:url(b.ttf)}"
    return Page(doc(f"<p style='font:60px B'>{text}</p>", rules), {"b.ttf": _composite_bomb_font()})


# --- Font settings, metric descriptors, local() (ADR 0022, part 2) ----------

VARIABLE_FONT_FILE = "crates/text/tests/webfonts/swb-variable.ttf"
"""The variable test font of the web font tests (axes `wght`, `wdth`, `SWBX`)."""


def _variable_font() -> bytes:
    from swbtools import paths

    return (paths.repo_root() / VARIABLE_FONT_FILE).read_bytes()


@case(
    "font-variation-settings and font-feature-settings with 200,000 distinct tags each, in"
    " properties and @font-face descriptors, past the limit of 64 tags per declaration"
    " (ADR 0022)",
    expect_log="font-variation-settings: more than 64 tags",
)
def font_settings_long_lists() -> Page:
    def tags(n: int) -> list[str]:
        letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        return [
            letters[i // 676 % 26] + letters[i // 26 % 26] + letters[i % 26] + "x" for i in range(n)
        ]

    variations = ",".join(f'"{tag}" {i}' for i, tag in enumerate(tags(200_000)))
    features = ",".join(f'"{tag}" {i}' for i, tag in enumerate(tags(200_000)))
    rules = (
        "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900;"
        f"font-variation-settings:{variations};font-feature-settings:{features}}}"
    )
    style = f"font-family:V;font-variation-settings:{variations};font-feature-settings:{features}"
    body = f"<p style='{style}'>AAAA BBBB " + "AB " * 2000 + "</p>"
    return Page(doc(body, rules), {"v.ttf": _variable_font()})


@case(
    "extreme numbers in font-variation-settings, font-feature-settings, size-adjust and the"
    " metric overrides: 1e308, 1e400, -1e999, 2^32, a million percent (ADR 0022)",
)
def font_settings_extreme_values() -> Page:
    values = ["1e308", "1e400", "-1e999", "4294967295", "99999999999", "-99999999999", "0.0000001"]
    rules = ""
    body = ""
    for i, value in enumerate(values):
        rules += (
            f"@font-face{{font-family:E{i};src:url(v.ttf);font-weight:100 900;"
            f"size-adjust:{value}%;ascent-override:{value}%;descent-override:{value}%;"
            f"line-gap-override:{value}%;font-variation-settings:'wght' {value},'SWBX' {value};"
            f"font-feature-settings:'liga' {value}}}"
        )
        style = (
            f"font:24px E{i};font-variation-settings:'wdth' {value},'wght' {value};"
            f"font-feature-settings:'kern' {value},'liga' {value}"
        )
        body += f'<p style="{style}">AAAA BBBB fi fl ffi</p>'
    return Page(doc(body, rules), {"v.ttf": _variable_font()})


@case(
    "20,000 elements with distinct font-variation-settings on a variable font, past the limit of"
    " 4,096 remembered lists (ADR 0022)",
    expect_log="font-variation-settings lists",
)
def font_variation_many_lists() -> Page:
    rules = "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900}"
    spans = "".join(
        '<span style="font-family:V;font-variation-settings:'
        f"'wght' {100 + i % 800}.{i // 800:02d}\">A{i} </span>"
        for i in range(20_000)
    )
    return Page(doc(spans, rules), {"v.ttf": _variable_font()})


@case(
    "60,000 elements with distinct font-feature-settings lists on one font: each list needs its"
    " own shape plan, at most 32 are kept per font (ADR 0022)"
)
def font_feature_many_lists() -> Page:
    spans = "".join(
        f"<span style=\"font-feature-settings:'liga' {i}\">fi{i} </span>" for i in range(60_000)
    )
    return Page(doc(spans))


@case(
    "1,000 @font-face rules with local() names of 10 KB, and 5,000 local() faces in use, past"
    " the limit of 1,000 face loads (ADR 0022)",
    expect_log="web font faces; not loading more",
)
def font_local_many_and_long() -> Page:
    long_rules = "".join(
        f"@font-face{{font-family:L{i};src:local('{'x' * 10_000}{i}')}}" for i in range(1_000)
    )
    many_rules = "".join(
        f"@font-face{{font-family:M{i};src:local('Nope {i}'),local('Liberation Sans')}}"
        for i in range(5_000)
    )
    spans = "".join(f"<span style='font-family:M{i}'>x{i} </span>" for i in range(5_000))
    spans += "".join(f"<span style='font-family:L{i}'>y{i} </span>" for i in range(1_000))
    return Page(doc(spans, long_rules + many_rules))
