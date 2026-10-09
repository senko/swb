"""jsdiff: the parts that need neither Node.js nor swb-js."""

import sys

from swbtools.jsdiff import NODE_WRAPPER, Run, diff_runs, error_line, run_process


def test_error_line_takes_the_first_line_and_unifies_syntax_errors():
    assert error_line("") == ""
    assert error_line("Uncaught TypeError: x is not a function\n    at a.js:1:1\n") == (
        "Uncaught TypeError: x is not a function"
    )
    assert error_line("SyntaxError: Unexpected number\n    at a.js:1:5") == (
        "SyntaxError: Unexpected number"
    )
    assert error_line("Uncaught SyntaxError: Unexpected number") == "SyntaxError: Unexpected number"


def test_diff_runs_agrees_and_differs():
    same = Run("1\n2\n", "")
    assert diff_runs("a.js", same, Run("1\n2\n", "")) == ""
    diff = diff_runs("a.js", Run("1\n3\n", ""), Run("1\n2\n", ""))
    assert "--- node/a.js" in diff
    assert "+++ swb-js/a.js" in diff
    assert "-2\n" in diff
    assert "+3\n" in diff


def test_diff_runs_reports_the_error_line():
    diff = diff_runs("a.js", Run("", "Uncaught TypeError: a"), Run("", "Uncaught RangeError: b"))
    assert "-Uncaught RangeError: b" in diff
    assert "+Uncaught TypeError: a" in diff
    assert diff_runs("a.js", Run("x", ""), Run("x", "")) == ""


def test_a_run_that_exceeds_the_limit_is_a_timeout():
    sleeper = [sys.executable, "-c", "import time; time.sleep(30)"]
    result = run_process(sleeper, timeout=0.2)
    assert result.timed_out
    assert result.error == "timeout"
    quick = run_process([sys.executable, "-c", "print('x')"], timeout=30)
    assert not quick.timed_out
    assert quick.stdout == "x\n"


def test_node_print_matches_to_string_join():
    assert "a.map(String).join(' ')" in NODE_WRAPPER
