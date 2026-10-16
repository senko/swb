"""The generator of the JavaScript lexer's Unicode tables (without network)."""

from swbtools.js_unicode_tables import VERSION, generate_tables, tables_path


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


def test_the_committed_tables_name_the_generator_version():
    header = tables_path().read_text(encoding="utf-8").split("\n\n", 1)[0]
    assert f"Character Database {VERSION}" in header


def test_make_runs_groups_steps_and_deltas():
    from swbtools.js_case_tables import expand_runs, make_runs

    mapping = {cp: cp + 32 for cp in range(0x41, 0x5B)}
    mapping.update({cp: cp + 1 for cp in range(0x100, 0x110, 2)})
    mapping[0x130] = 0x69
    runs = make_runs(mapping)
    assert runs == [(0x41, 0x5A, 1, 32), (0x100, 0x10E, 2, 1), (0x130, 0x130, 1, 0x69 - 0x130)]
    assert expand_runs(runs) == mapping


def test_generate_case_tables(tmp_path):
    from swbtools.js_case_tables import generate_case_tables

    data = tmp_path / "UnicodeData.txt"
    data.write_text(
        "0041;LATIN CAPITAL LETTER A;Lu;0;L;;;;;N;;;;0061;\n"
        "0061;LATIN SMALL LETTER A;Ll;0;L;;;;;N;;;0041;;0041\n"
        "00DF;LATIN SMALL LETTER SHARP S;Ll;0;L;;;;;N;;;;;\n"
        "0130;LATIN CAPITAL LETTER I WITH DOT ABOVE;Lu;0;L;0049 0307;;;;N;;;;0069;\n",
        encoding="utf-8",
    )
    special = tmp_path / "SpecialCasing.txt"
    special.write_text(
        "00DF; 00DF; 0053 0073; 0053 0053; # LATIN SMALL LETTER SHARP S\n"
        "0130; 0069 0307; 0130; 0130; # LATIN CAPITAL LETTER I WITH DOT ABOVE\n"
        "03A3; 03C2; 03A3; 03A3; Final_Sigma; # GREEK CAPITAL LETTER SIGMA\n",
        encoding="utf-8",
    )
    folding = tmp_path / "CaseFolding.txt"
    folding.write_text(
        "0041; C; 0061; # LATIN CAPITAL LETTER A\n"
        "00DF; F; 0073 0073; # LATIN SMALL LETTER SHARP S\n"
        "1E9E; S; 00DF; # LATIN CAPITAL LETTER SHARP S\n",
        encoding="utf-8",
    )
    core = tmp_path / "DerivedCoreProperties.txt"
    core.write_text("0041..005A ; Cased\n0027 ; Case_Ignorable\n", encoding="utf-8")
    source = generate_case_tables(data, special, folding, core, "17.0.0")
    assert "Do not edit." in source
    assert "LOWER_RUNS: [(u32, u32, u8, i32); 1] = [\n    (0x0041, 0x0041, 1, 32),\n];" in source
    assert "UPPER_RUNS: [(u32, u32, u8, i32); 1] = [\n    (0x0061, 0x0061, 1, -32),\n];" in source
    # Special mappings take the place of the simple ones.
    lower_special = (
        "LOWER_SPECIAL: [(u32, [u32; 3]); 1] = [\n    (0x0130, [0x0069, 0x0307, 0x0000]),\n];"
    )
    assert lower_special in source
    assert "(0x00DF, [0x0053, 0x0053, 0x0000])," in source
    assert "FOLD_RUNS: [(u32, u32, u8, i32); 2]" in source
    assert "CASED: [(u32, u32); 1] = [\n    (0x0041, 0x005A),\n];" in source
    assert "CASE_IGNORABLE: [(u32, u32); 1] = [\n    (0x0027, 0x0027),\n];" in source


def test_full_decompositions_are_recursive():
    from swbtools.js_norm_tables import full_decompositions

    canonical = {
        0x1E69: [0x1E63, 0x307],
        0x1E63: [0x73, 0x323],
        0x212B: [0xC5],
        0xC5: [0x41, 0x30A],
        0x1E9B: [0x17F, 0x307],
    }
    compat = {0x17F: [0x73], 0x2160: [0x49]}
    full_canon, full_compat = full_decompositions(canonical, compat)
    assert full_canon[0x1E69] == [0x73, 0x323, 0x307]
    assert full_canon[0x212B] == [0x41, 0x30A]
    # The compatibility decomposition applies to the parts of a canonical one.
    assert full_compat[0x1E9B] == [0x73, 0x307]
    assert full_compat[0x17F] == [0x73]
    assert full_compat[0x2160] == [0x49]
    assert 0x1E69 not in full_compat


def test_the_committed_case_and_normalization_tables_name_the_generator_version():
    directory = tables_path().parent
    for name in ("case_tables.rs", "norm_tables.rs"):
        header = (directory / name).read_text(encoding="utf-8").split("\n\n", 1)[0]
        assert f"//! {VERSION} (" in header
