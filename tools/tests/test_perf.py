"""The perf table."""

from swbtools.perf import STAGES, table


def test_table_has_one_row_per_fixture():
    stages = {stage: {"first_ms": 2.0, "median_ms": 1.0} for stage in STAGES}
    text = table({"a": {"elements": 5, "stages": stages}})
    lines = text.splitlines()
    assert lines[0].startswith("| Fixture | Elements | parse |")
    assert lines[2] == "| a | 5 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 6.00 |"
