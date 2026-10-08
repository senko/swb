"""Builds swb-variable.ttf, the variable test font of the web font tests.

Run from the repository root:

    uv run --project tools python crates/text/tests/webfonts/make-variable-font.py

The font is made for swb (no third-party outlines), released under CC0.
Glyphs: .notdef, space, A, B, H, x. Each letter is a filled box whose
width depends on the axes, so that tests can see the axis values in the
advance widths:

- `wght` 100..900, default 400: advance of A is 400 at 100, 600 at 400
  and 1000 at 900 (linear between the masters).
- `wdth` 75..125, default 100: advance of A = 600 at 100, 400 at 75, 800
  at 125.
- `SWBX` (custom axis) 0..1000, default 0: 0.5 units per unit more.

Space is always 250 units. Ascent 800, descent 200, units per em 1000,
no line gap. The box of a letter has the height of its cap (700 units for
A, B and H, 500 for x).
"""

import sys
from pathlib import Path

from fontTools.designspaceLib import (
    AxisDescriptor,
    DesignSpaceDocument,
    SourceDescriptor,
)
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.varLib import build as build_variable

OUT = Path(__file__).with_name("swb-variable.ttf")
UPM = 1000


def advance(wght: float, wdth: float, swbx: float) -> int:
    """The advance width of the letters at the master location."""
    width = 600 + (wght - 400) * (400 / 500 if wght > 400 else 200 / 300)
    width += (wdth - 100) * 8
    width += swbx * 0.5
    return round(width)


def build_master(path: Path, wght: float, wdth: float, swbx: float) -> None:
    fb = FontBuilder(UPM, isTTF=True)
    glyph_order = [".notdef", "space", "A", "B", "H", "x"]
    fb.setupGlyphOrder(glyph_order)
    fb.setupCharacterMap({0x20: "space", 0x41: "A", 0x42: "B", 0x48: "H", 0x78: "x"})
    glyphs = {}
    metrics = {}
    for name in glyph_order:
        pen = TTGlyphPen(None)
        if name == "space":
            metrics[name] = (250, 0)
        else:
            width = advance(wght, wdth, swbx)
            height = 500 if name == "x" else 700
            pen.moveTo((50, 0))
            pen.lineTo((50, height))
            pen.lineTo((width - 50, height))
            pen.lineTo((width - 50, 0))
            pen.closePath()
            metrics[name] = (width, 50)
        glyphs[name] = pen.glyph()
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
    fb.setupNameTable(
        {
            "familyName": "SWB Variable",
            "styleName": "Regular",
            "uniqueFontIdentifier": "SWB Variable Regular",
            "fullName": "SWB Variable",
            "psName": "SWBVariable",
            "licenseDescription": "CC0 1.0 Universal. Made for swb.",
        }
    )
    fb.setupOS2(
        sTypoAscender=800,
        sTypoDescender=-200,
        sTypoLineGap=0,
        usWinAscent=800,
        usWinDescent=200,
        usWeightClass=400,
        sxHeight=500,
        sCapHeight=700,
    )
    fb.setupPost()
    fb.save(path)


def main() -> None:
    build_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT.parent / "build"
    build_dir.mkdir(exist_ok=True)
    doc = DesignSpaceDocument()
    for tag, name, lo, default, hi in [
        ("wght", "Weight", 100, 400, 900),
        ("wdth", "Width", 75, 100, 125),
        ("SWBX", "Extra", 0, 0, 1000),
    ]:
        axis = AxisDescriptor()
        axis.tag, axis.name = tag, name
        axis.minimum, axis.default, axis.maximum = lo, default, hi
        doc.addAxis(axis)
    locations = [
        (400, 100, 0),
        (100, 100, 0),
        (900, 100, 0),
        (400, 75, 0),
        (400, 125, 0),
        (400, 100, 1000),
    ]
    for i, (wght, wdth, swbx) in enumerate(locations):
        path = build_dir / f"master{i}.ttf"
        build_master(path, wght, wdth, swbx)
        source = SourceDescriptor()
        source.path = str(path)
        source.location = {"Weight": wght, "Width": wdth, "Extra": swbx}
        if i == 0:
            source.copyInfo = True
        doc.addSource(source)
    font, _, _ = build_variable(doc)
    font.save(OUT)
    for path in build_dir.glob("master*.ttf"):
        path.unlink()
    build_dir.rmdir()


if __name__ == "__main__":
    main()
