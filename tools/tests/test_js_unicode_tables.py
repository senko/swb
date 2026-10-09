"""The generator of the JavaScript lexer's Unicode tables (without network)."""

from swbtools.js_unicode_tables import generate_tables


def test_generate_tables(tmp_path):
    core = tmp_path / "DerivedCoreProperties.txt"
    core.write_text(
        "# comment\n"
        "0041..005A    ; ID_Start # L& [26] LATIN CAPITAL LETTER A..Z\n"
        "0061..007A    ; ID_Start\n"
        "0030..0039    ; ID_Continue\n"
        "0041..005A    ; ID_Continue\n"
        "0061..007A    ; ID_Continue\n"
        "0041          ; Alphabetic\n",
        encoding="utf-8",
    )
    category = tmp_path / "DerivedGeneralCategory.txt"
    category.write_text(
        "# @missing: 0000..10FFFF; Cn\n0020 ; Zs # SPACE\n00A0 ; Zs\n3000 ; Zs\n0041 ; Lu\n",
        encoding="utf-8",
    )
    source = generate_tables(core, category)
    assert "Do not edit." in source
    assert "pub(super) static ID_START: [(u32, u32); 2] = [\n    (0x0041, 0x005A),\n" in source
    assert "    (0x0061, 0x007A),\n];" in source
    assert "pub(super) static ID_CONTINUE: [(u32, u32); 3] = [\n    (0x0030, 0x0039),\n" in source
    assert "    (0x0041, 0x005A),\n    (0x0061, 0x007A),\n];" in source
    spaces = source.split("SPACE_SEPARATORS")[1]
    assert "(0x0020, 0x0020),\n    (0x00A0, 0x00A0),\n    (0x3000, 0x3000),\n];" in spaces
    assert source.endswith("];\n")
