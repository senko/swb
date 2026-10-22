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


def test_generate_property_names(tmp_path):
    from swbtools.js_property_names import BINARY, SPECIAL_BINARY, generate_property_names

    aliases = tmp_path / "PropertyAliases.txt"
    lines = ["gc ; General_Category", "sc ; Script", "scx ; Script_Extensions"]
    lines += [f"{name} ; {name}" for name in BINARY if name not in SPECIAL_BINARY]
    lines = [line for line in lines if line != "White_Space ; White_Space"]
    lines += ["WSpace ; White_Space ; space", "CE ; Composition_Exclusion"]
    aliases.write_text("# comment\n" + "\n".join(lines) + "\n", encoding="utf-8")
    values = tmp_path / "PropertyValueAliases.txt"
    values.write_text(
        "gc ; Lu ; Uppercase_Letter\n"
        "gc ; P ; Punctuation ; punct # Pc | Pd\n"
        "sc ; Latn ; Latin\n"
        "sc ; Zinh ; Inherited ; Qaai\n"
        "lb ; AL ; Alphabetic\n",
        encoding="utf-8",
    )
    source = generate_property_names(aliases, values, "17.0.0")
    assert "Do not edit." in source
    assert 'pub(super) static GENERAL_CATEGORIES: [&str; 2] = [\n    "Lu",\n    "P",\n];' in source
    assert '    ("punct", 1),\n' in source
    assert '    ("Qaai", 1),\n' in source
    assert '    ("Zinh", 1),\n' in source
    assert '("WSpace", ' in source
    assert '("space", ' in source
    assert "Composition_Exclusion" not in source
    assert '"AL"' not in source
    assert '("ASCII", 0),' in source


def test_the_committed_property_names_name_the_generator_version():
    header = (tables_path().parent / "property_names.rs").read_text(encoding="utf-8")
    assert f"Character Database {VERSION}" in header.split("\n\n", 1)[0]


def test_range_and_sequence_encoding_round_trips():
    from swbtools.js_property_sets import (
        decode_ranges,
        decode_sequences,
        encode_ranges,
        encode_sequences,
        leb128,
        read_leb128,
    )
    from swbtools.ucd import merge_ranges

    for number in (0, 1, 127, 128, 300, 0x10FFFF):
        assert read_leb128(leb128(number), 0) == (number, len(leb128(number)))
    assert leb128(300) == bytes([0xAC, 0x02])
    ranges = [(0, 0), (5, 9), (0x80, 0x3FFF), (0x10FFFF, 0x10FFFF)]
    # gap 0, length 0; gap 4, length 4; gap 0x76, length 0x3F7F; ...
    assert encode_ranges(ranges[:2]) == bytes([0, 0, 4, 4])
    assert decode_ranges(encode_ranges(ranges)) == ranges
    assert decode_ranges(b"") == []
    sequences = [(0x1F1ED, 0x1F1F7), (0x23, 0xFE0F, 0x20E3)]
    assert decode_sequences(encode_sequences(sequences)) == sorted(sequences)
    assert merge_ranges([(5, 6), (1, 2), (3, 4), (10, 10)]) == [(1, 6), (10, 10)]
    try:
        encode_ranges([(5, 6), (6, 7)])
    except ValueError:
        pass
    else:
        raise AssertionError("overlapping ranges must be rejected")


def test_group_members():
    from swbtools.js_property_sets import group_members

    names = ["C", "Cc", "Cn", "L", "LC", "Ll", "Lu", "Lt", "Zs"]
    members = group_members(names)
    assert members[names.index("C")] == [names.index("Cc"), names.index("Cn")]
    assert members[names.index("L")] == [names.index(n) for n in ("Ll", "Lu", "Lt")]
    assert members[names.index("LC")] == [names.index(n) for n in ("Lu", "Ll", "Lt")]
    assert members[names.index("Lu")] == [names.index("Lu")]


def test_generate_property_data(tmp_path):
    from swbtools.js_property_names import BINARY, SPECIAL_BINARY, STRING_PROPERTIES
    from swbtools.js_property_sets import (
        FILES,
        decode_ranges,
        decode_sequences,
        generate_property_data,
    )

    files = {key: tmp_path / key for key in FILES}
    files["DerivedGeneralCategory"].write_text(
        "# @missing: 0000..10FFFF; Cn\n0041..005A ; Lu\n0061..007A ; Ll\n0030 ; Nd\n"
    )
    files["Scripts"].write_text(
        "# @missing: 0000..10FFFF; Unknown\n0041..005A ; Latin\n0061..007A ; Latin\n"
        "0391..03A1 ; Greek\n0300 ; Inherited\n"
    )
    files["ScriptExtensions"].write_text("0300 ; Grek Latn\n0041 ; Grek\n")
    names = [name for name in BINARY if name not in SPECIAL_BINARY]
    files["PropList"].write_text("".join(f"0041..0042 ; {name}\n" for name in names))
    for key in ("DerivedCoreProperties", "DerivedNormalizationProps"):
        files[key].write_text("")
    files["DerivedBinaryProperties"].write_text("")
    files["emoji-data"].write_text("")
    files["emoji-sequences"].write_text(
        "231A..231B ; Basic_Emoji ; watch\n"
        "1F5D3 FE0F ; Basic_Emoji ; calendar\n"
        "0023 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap\n"
        "1F44D 1F3FD ; RGI_Emoji_Modifier_Sequence ; thumbs\n"
        "1F1ED 1F1F7 ; RGI_Emoji_Flag_Sequence ; flag\n"
        "1F3F4 E0067 E007F ; RGI_Emoji_Tag_Sequence ; tag\n"
    )
    files["emoji-zwj-sequences"].write_text("1F468 200D 1F469 ; RGI_Emoji_ZWJ_Sequence ; couple\n")
    categories = ["Ll", "Lu", "L", "Nd", "Cn"]
    scripts = [
        ["Latn", "Latin"],
        ["Grek", "Greek"],
        ["Zinh", "Inherited", "Qaai"],
        ["Zzzz", "Unknown"],
    ]
    source = generate_property_data(files, categories, scripts, "17.0.0")
    assert "Do not edit." in source
    assert "pub(super) static GENERAL_CATEGORY_MEMBERS: [&[u8]; 5]" in source
    assert "    &[1, 0],\n" in source or "    &[0, 1],\n" in source
    assert f"pub(super) static BINARY_OFFSETS: [u32; {len(BINARY) + 1}]" in source
    assert f"STRING_SEQUENCES_OFFSETS: [u32; {len(STRING_PROPERTIES) + 1}]" in source
    # Decode the data back from the source.
    import re

    body = source.split('pub(super) static DATA: &[u8] = b"\\\n', 1)[1].split('";', 1)[0]
    data = bytes(int(h, 16) for h in re.findall(r"\\x([0-9A-F]{2})", body))

    def offsets(name):
        text = source.split(f"static {name}: [u32;", 1)[1].split("];", 1)[0]
        return [int(n) for n in re.findall(r"\d+", text.split("= [", 1)[1])]

    def item(table, index):
        o = offsets(table)
        return data[o[index] : o[index + 1]]

    assert decode_ranges(item("GENERAL_CATEGORY_OFFSETS", 1)) == [(0x41, 0x5A)]
    assert decode_ranges(item("GENERAL_CATEGORY_OFFSETS", 2)) == []  # a group has no data
    assert decode_ranges(item("SCRIPT_OFFSETS", 0)) == [(0x41, 0x5A), (0x61, 0x7A)]
    # Script_Extensions: U+0300 is extended to Greek and Latin, U+0041 is Greek only.
    assert decode_ranges(item("SCRIPT_EXTENSIONS_OFFSETS", 1)) == [
        (0x41, 0x41),
        (0x300, 0x300),
        (0x391, 0x3A1),
    ]
    assert decode_ranges(item("SCRIPT_EXTENSIONS_OFFSETS", 0)) == [
        (0x42, 0x5A),
        (0x61, 0x7A),
        (0x300, 0x300),
    ]
    assert decode_ranges(item("SCRIPT_EXTENSIONS_OFFSETS", 2)) == []
    assert decode_ranges(item("BINARY_OFFSETS", BINARY.index("Dash"))) == [(0x41, 0x42)]
    assert decode_ranges(item("BINARY_OFFSETS", BINARY.index("Any"))) == []
    assert decode_ranges(item("STRING_CODE_POINTS_OFFSETS", 0)) == [(0x231A, 0x231B)]
    assert decode_sequences(item("STRING_SEQUENCES_OFFSETS", 0)) == [(0x1F5D3, 0xFE0F)]
    assert decode_sequences(item("STRING_SEQUENCES_OFFSETS", 5)) == [(0x1F468, 0x200D, 0x1F469)]
    assert item("STRING_SEQUENCES_OFFSETS", 6) == b""  # RGI_Emoji: the code computes it


def test_property_data_needs_every_binary_property(tmp_path):
    import pytest

    from swbtools.js_property_sets import FILES, generate_property_data

    files = {key: tmp_path / key for key in FILES}
    for path in files.values():
        path.write_text("")
    files["DerivedGeneralCategory"].write_text("# @missing: 0000..10FFFF; Cn\n")
    files["Scripts"].write_text("# @missing: 0000..10FFFF; Unknown\n")
    with pytest.raises(ValueError, match="no data for the binary property"):
        generate_property_data(files, ["Cn"], [["Zzzz", "Unknown"]], "17.0.0")


def test_the_committed_property_data_names_the_generator_version():
    header = (tables_path().parent / "property_data.rs").read_text(encoding="utf-8")
    assert f"Character Database\n//! {VERSION} (" in header.split("\n\n", 1)[0]
