"""The line break measurement tool and the table generator (without Chromium)."""

from swbtools.linebreak_tables import read_matrices
from swbtools.linebreaks import (
    ALLOWED,
    FORCED,
    LATIN1,
    NONE,
    NORMAL,
    UNKNOWN,
    Breaks,
    Matrix,
    breaks_from_lines,
    format_case,
    format_matrix,
    marks_for,
)
from swbtools.ucd import has_property, ranges, read_property, value_ranges


def test_breaks_from_lines_skips_code_points_without_rectangles():
    assert breaks_from_lines([0, 0, 1, 1, 2]) == [2, 4]
    assert breaks_from_lines([0, -1, 1]) == [2]
    assert breaks_from_lines([]) == []
    # Lines that go back: the measurement is wrong.
    assert breaks_from_lines([0, 1, 0]) is None


def test_marks():
    widths = [6.0, 6.0, 6.0, 6.0]
    assert marks_for(4, [2], [], widths) == (NONE, ALLOWED, NONE)
    assert marks_for(4, [2], [2], widths) == (NONE, FORCED, NONE)
    assert marks_for(4, None, [], widths) == (UNKNOWN,) * 3
    assert marks_for(1, [], [], [6.0]) == ()


def test_positions_in_text_of_zero_width_are_unknown():
    # "a", U+200B, U+2060, "b": the line breaks only before "b"; the
    # position between U+200B and U+2060 is not observable.
    widths = [6.0, 0.0, 0.0, 6.0]
    assert marks_for(4, [3], [], widths) == (NONE, NONE, ALLOWED)
    # U+200B U+2060 at the start: the content before the break has no
    # width, so the line need not break inside it.
    widths = [0.0, 0.0, 6.0]
    assert marks_for(3, [2], [], widths) == (UNKNOWN, ALLOWED)


def test_format_case():
    assert format_case("a-0", Breaks((NONE, ALLOWED))) == "0061 × 002D ÷ 0030"


def test_matrices_round_trip(tmp_path):
    matrix = Matrix("a", "a", NORMAL, LATIN1, [[ALLOWED] * len(LATIN1) for _ in LATIN1])
    matrix.marks[0][1] = NONE
    matrix.marks[2][3] = UNKNOWN
    path = tmp_path / "latin1.txt"
    path.write_text("# comment\n" + "\n".join(format_matrix(matrix)) + "\n", encoding="utf-8")
    read = read_matrices(path)["word-break=normal hyphens=manual"]
    assert read[0][0] is True
    assert read[0][1] is False
    assert read[2][3] is None


def test_ranges():
    assert ranges([False, True, True, False, True]) == [(1, 2), (4, 4)]
    assert value_ranges(["a", "a", None, "b", "b", "a"]) == [(0, 1, "a"), (3, 4, "b"), (5, 5, "a")]


def test_read_property(tmp_path):
    path = tmp_path / "LineBreak.txt"
    path.write_text(
        "# @missing: 0000..10FFFF; XX\n0041..0043;AL # letters\n0030;NU\n", encoding="utf-8"
    )
    values = read_property(path, "AL")
    assert values[0x41:0x44] == ["AL", "AL", "AL"]
    assert values[0x30] == "NU"
    assert values[0x10FFFF] == "XX"


def test_has_property_reads_one_property_of_several(tmp_path):
    path = tmp_path / "emoji-data.txt"
    path.write_text(
        "0023 ; Emoji # number sign\n"
        "1F000..1F002 ; Extended_Pictographic # tiles\n"
        "1F001 ; Emoji_Presentation\n",
        encoding="utf-8",
    )
    values = has_property(path, "Extended_Pictographic")
    assert values[0x1F000:0x1F004] == [True, True, True, False]
    assert not values[0x23]
