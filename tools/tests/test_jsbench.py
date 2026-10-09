"""jsbench: the parts that need neither Node.js nor swb-js."""

import sys

import pytest

from swbtools.jsbench import (
    CommandError,
    parse_lexing,
    parse_programs,
    programs_table,
    run,
    scripts_table,
)


def test_parse_programs_ignores_other_lines():
    output = (
        "instruction size 12 bytes\n"
        "fib(27): 59.2 ms (result 196418), GC 0.0 ms in 0 collections (0 %)\n"
        "string: 20k appends of 'ab': 25.1 ms (result 40000), GC 0.1 ms\n"
        "time limit 100 ms: for (;;) {}: Err(x), 0.01 ms after the deadline\n"
    )
    assert parse_programs(output) == {"fib(27)": 59.2, "string: 20k appends of 'ab'": 25.1}


def test_programs_table_has_ratios():
    table = programs_table({"a": 10.0, "b": 5.0}, {"a": 5.0})
    assert "| a | 10.0 | 5.0 | 2.00x |" in table
    assert "| b | 5.0 | | |" in table


def test_scripts_table():
    lexing = parse_lexing(
        "60 files (7 need two-byte units), 59 lexed to the end\n"
        "1132273 tokens, 3536740 bytes\n"
        "best of 5 runs: 28.5 ms, 124 MB/s, 39.7 million tokens/s\n"
    )
    assert lexing["mb_s"] == "124"
    report = {
        "files": 60,
        "source_bytes": 3536740,
        "compiled": 3,
        "compiled_source_bytes": 100000,
        "code_bytes": 700000,
        "compile_seconds": 0.002,
        "attempt_seconds": 0.01,
        "reached_bytes": 500000,
        "peak_rss_kib": 51200,
        "failures": {"not supported yet (class)": 40},
    }
    table = scripts_table(lexing, report)
    assert "| Scripts that compile | 3 of 60 |" in table
    assert "50 MB/s" in table
    assert "500000" not in table
    assert "0.50 MB read (50 MB/s)" in table
    assert "7.0 bytes per source byte" in table
    assert "| Peak memory of the process | 50 MiB |" in table
    assert "not supported yet (class)" in table


def test_a_failed_benchmark_command_reports_its_stderr():
    failing = [sys.executable, "-c", "import sys; print('boom', file=sys.stderr); sys.exit(3)"]
    with pytest.raises(CommandError) as error:
        run(failing)
    assert "exit code 3" in str(error.value)
    assert "boom" in str(error.value)
    with pytest.raises(CommandError, match="did not end"):
        run([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.2)
    with pytest.raises(CommandError, match="cannot run"):
        run(["/nonexistent/command"])


def test_unreadable_files_are_listed_in_the_table():
    report = {
        "files": 1,
        "source_bytes": 10,
        "compiled": 0,
        "compiled_source_bytes": 0,
        "code_bytes": 0,
        "compile_seconds": 0.0,
        "attempt_seconds": 0.0,
        "reached_bytes": 0,
        "peak_rss_kib": 1024,
        "failures": {},
        "unreadable": ["/x/a.js: Permission denied (os error 13)"],
    }
    assert "Unreadable file | /x/a.js: Permission denied" in scripts_table({}, report)
