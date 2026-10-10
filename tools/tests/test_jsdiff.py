"""jsdiff: the parts that need neither Node.js nor swb-js."""

import sys

from swbtools.jsdiff import (
    NODE_WRAPPER,
    Run,
    diff_runs,
    error_line,
    run_process,
    strip_node_frames,
)


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


def test_node_frames_are_removed_from_printed_stacks():
    text = (
        "Error: x\n"
        "    at f (a.js:1:1)\n"
        "    at Script.runInThisContext (node:vm:137:12)\n"
        "    at [eval]:7:6\n"
        "    at node:internal/main/eval_string:55:3\n"
        "after\n"
    )
    assert strip_node_frames(text) == "Error: x\n    at f (a.js:1:1)\nafter\n"


def test_node_frame_filter_keeps_user_frames_with_similar_paths():
    text = (
        "Error: x\n"
        "    at f (/work/node:thing/a.js:1:1)\n"
        "    at /work/[eval]/b.js:2:2\n"
        "    at g (/work/x[eval]y.js:3:3)\n"
        "    at Module._compile (node:internal/modules/cjs/loader:1546:14)\n"
        "    at Object.runInThisContext (node:vm:317:38)\n"
        "    at [eval]:7:6\n"
        "    at [eval]-wrapper:6:24\n"
        "    at evalScript (node:internal/process/execution:136:3)\n"
    )
    assert strip_node_frames(text) == (
        "Error: x\n"
        "    at f (/work/node:thing/a.js:1:1)\n"
        "    at /work/[eval]/b.js:2:2\n"
        "    at g (/work/x[eval]y.js:3:3)\n"
    )
