"""The perf table."""

from swbtools.perf import RESTYLE_PATHS, STAGES, restyle_table, table


def test_table_has_one_row_per_fixture():
    stages = {stage: {"first_ms": 2.0, "median_ms": 1.0} for stage in STAGES}
    text = table({"a": {"elements": 5, "stages": stages}})
    lines = text.splitlines()
    assert lines[0].startswith("| Fixture | Elements | parse |")
    assert lines[2] == "| a | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 6.00 |"


def test_restyle_table_only_with_the_paths():
    assert restyle_table({"a": {"interaction": {"hover": {}}}}) is None
    paths = dict.fromkeys(RESTYLE_PATHS, 0) | {"targeted": 7, "restyled_elements": 30}
    text = restyle_table({"a": {"interaction": {"hover": {"state_restyles": paths}}}})
    assert text is not None
    lines = text.splitlines()
    assert lines[0].startswith("| Fixture | targeted | nothing |")
    assert lines[2] == "| a | 7 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |"
