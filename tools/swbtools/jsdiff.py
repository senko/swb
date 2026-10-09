"""`swbtools jsdiff`: runs JavaScript files in swb-js and in Node.js and diffs the output.

For each file, the tool compares the standard output and the first line of
the uncaught-error report (`Uncaught Name: message`). Node runs the file
as a classic script in the global scope, with `print` defined as
`ToString` of each argument, joined by spaces (like swb-js). Not part of `just check`.
"""

import difflib
import logging
import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths

log = logging.getLogger(__name__)

TIMEOUT_S = 60

NODE_WRAPPER = """
const fs = require('fs');
const vm = require('vm');
globalThis.print = (...a) => console.log(a.map(String).join(' '));
const file = process.argv[1];
try {
  vm.runInThisContext(fs.readFileSync(file, 'utf8'), { filename: file });
} catch (e) {
  const text = e !== null && typeof e === 'object' && 'message' in e
    ? e.name + ': ' + e.message : String(e);
  console.error('Uncaught ' + text);
  process.exitCode = 1;
}
"""
"""The Node.js side: runs the file, reports an uncaught value like swb-js."""


@dataclass
class Run:
    """The observable result of one run."""

    stdout: str
    error: str
    """The first line of the uncaught-error report, or an empty string."""
    timed_out: bool = False
    """The run did not end within the time limit."""


def error_line(stderr: str) -> str:
    """The first line of an error report, without the position line.

    A compile error is a `SyntaxError` in both engines, so the `Uncaught `
    prefix of `SyntaxError` is removed (swb-js reports a compile error
    without it).
    """
    for line in stderr.splitlines():
        line = line.rstrip()
        if line:
            return re.sub(r"^Uncaught (SyntaxError:)", r"\1", line)
    return ""


def run_process(command: list[str], timeout: float = TIMEOUT_S) -> Run:
    """Runs a command; a run that exceeds `timeout` seconds is a timeout."""
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return Run("", "timeout", timed_out=True)
    return Run(result.stdout, error_line(result.stderr))


def run_swb(binary: Path, file: Path) -> Run:
    """Runs a file in swb-js."""
    return run_process([str(binary), str(file)])


def run_node(node: str, file: Path) -> Run:
    """Runs a file in Node.js."""
    return run_process([node, "-e", NODE_WRAPPER, str(file)])


def diff_runs(name: str, swb_run: Run, node_run: Run) -> str:
    """The unified diff of two runs; an empty string if they agree."""
    parts = []
    if swb_run.stdout != node_run.stdout:
        parts.extend(
            difflib.unified_diff(
                node_run.stdout.splitlines(keepends=True),
                swb_run.stdout.splitlines(keepends=True),
                fromfile=f"node/{name}",
                tofile=f"swb-js/{name}",
            )
        )
        if parts and not parts[-1].endswith("\n"):
            parts.append("\n")
    if swb_run.error != node_run.error:
        parts.extend(
            difflib.unified_diff(
                [node_run.error + "\n"] if node_run.error else [],
                [swb_run.error + "\n"] if swb_run.error else [],
                fromfile=f"node/{name} (error)",
                tofile=f"swb-js/{name} (error)",
            )
        )
    return "".join(parts)


def find_swb_js(explicit: Path | None) -> Path | None:
    """The swb-js binary: `explicit`, `$SWB_JS`, or `target/release/swb-js`."""
    if explicit is not None:
        return explicit if explicit.is_file() else None
    from_env = os.environ.get("SWB_JS")
    candidates = [Path(from_env)] if from_env else []
    candidates.append(paths.repo_root() / "target" / "release" / "swb-js")
    return next((c for c in candidates if c.is_file()), None)


def jsdiff(files: list[Path], swb_js: Path | None) -> int:
    """Compares each file; returns 1 if any differs, 2 on a setup error."""
    binary = find_swb_js(swb_js)
    if binary is None:
        log.error("swb-js not found; build it (`cargo build --release -p swb-js`)")
        return 2
    node = shutil.which("node")
    if node is None:
        log.error("node not found")
        return 2
    differing = 0
    for file in files:
        swb_run, node_run = run_swb(binary, file), run_node(node, file)
        if swb_run.timed_out or node_run.timed_out:
            differing += 1
            engines = [
                name for name, run in (("swb-js", swb_run), ("node", node_run)) if run.timed_out
            ]
            print(f"timeout: {file} ({', '.join(engines)})")
            continue
        diff = diff_runs(str(file), swb_run, node_run)
        if diff:
            differing += 1
            print(diff, end="")
        else:
            print(f"same: {file}")
    return 1 if differing else 0
