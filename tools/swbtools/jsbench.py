"""`swbtools jsbench`: benchmarks of the JavaScript engine (docs/performance.md).

Two parts, printed as Markdown:

1. The small programs of `crates/js/examples/vm_bench.rs`, best of 5, in swb's
   engine and in `node --jitless` (V8's interpreter, as a black box).
2. For a directory of scripts: the lexing speed of all scripts, and the parse
   and compile time, the code size and the peak memory of the scripts that
   compile (`swb-js bench-compile`).
"""

import json
import logging
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from swbtools import paths

log = logging.getLogger(__name__)

RESULT_LINE = re.compile(r"^(?P<name>.+?): (?P<ms>[0-9.]+) ms \(result ")
"""A program line of vm_bench (and of its Node.js version)."""


def parse_programs(output: str) -> dict[str, float]:
    """The best time in ms of each program from the output of vm_bench."""
    times = {}
    for line in output.splitlines():
        match = RESULT_LINE.match(line)
        if match:
            times[match["name"]] = float(match["ms"])
    return times


def programs_table(swb: dict[str, float], node: dict[str, float]) -> str:
    """A Markdown table of the program times and the ratio swb / node."""
    lines = ["| Program | swb-js (ms) | node --jitless (ms) | ratio |", "|---|---|---|---|"]
    for name, swb_ms in swb.items():
        node_ms = node.get(name)
        if node_ms:
            lines.append(f"| {name} | {swb_ms:.1f} | {node_ms:.1f} | {swb_ms / node_ms:.2f}x |")
        else:
            lines.append(f"| {name} | {swb_ms:.1f} | | |")
    return "\n".join(lines)


def parse_lexing(output: str) -> dict[str, str]:
    """The numbers in the output of the `lex_files` example."""
    result = {}
    files = re.search(r"(\d+) files .*?, (\d+) lexed to the end", output)
    if files:
        result["files"], result["lexed"] = files.group(1), files.group(2)
    tokens = re.search(r"(\d+) tokens, (\d+) bytes", output)
    if tokens:
        result["tokens"], result["bytes"] = tokens.group(1), tokens.group(2)
    speed = re.search(r"best of \d+ runs: ([0-9.]+) ms, (\d+) MB/s, ([0-9.]+) million", output)
    if speed:
        result["ms"], result["mb_s"], result["mtokens_s"] = speed.groups()
    return result


def scripts_table(lexing: dict[str, str], compile_report: dict) -> str:
    """The Markdown lines for a directory of scripts."""
    compiled = compile_report["compiled"]
    seconds = compile_report["compile_seconds"]
    source = compile_report["compiled_source_bytes"]
    lines = [
        "| Measure | Value |",
        "|---|---|",
        f"| Scripts | {compile_report['files']} ({compile_report['source_bytes'] / 1e6:.1f} MB) |",
    ]
    if lexing:
        lines.append(
            f"| Lexing, all scripts | {lexing['ms']} ms, {lexing['mb_s']} MB/s, "
            f"{lexing['mtokens_s']} million tokens/s ({lexing['lexed']} of "
            f"{lexing['files']} lex to the end) |"
        )
    lines.append(f"| Scripts that compile | {compiled} of {compile_report['files']} |")
    if compiled and seconds > 0:
        lines.append(
            f"| Parse and compile, scripts that compile | {seconds * 1000:.2f} ms for "
            f"{source / 1e3:.0f} KB ({source / seconds / 1e6:.0f} MB/s) |"
        )
        lines.append(
            f"| Code objects, scripts that compile | {compile_report['code_bytes'] / 1e3:.0f} KB "
            f"({compile_report['code_bytes'] / max(source, 1):.1f} bytes per source byte) |"
        )
    attempt = compile_report["attempt_seconds"]
    reached = compile_report["reached_bytes"]
    if attempt > 0:
        lines.append(
            f"| Parse until the first unsupported construct, all scripts | "
            f"{attempt * 1000:.1f} ms for {reached / 1e6:.2f} MB read "
            f"({reached / attempt / 1e6:.0f} MB/s) |"
        )
    lines.append(
        f"| Peak memory of the process | {compile_report['peak_rss_kib'] / 1024:.0f} MiB |"
    )
    for entry in compile_report.get("unreadable", []):
        lines.append(f"| Unreadable file | {entry} |")
    failures = sorted(compile_report["failures"].items(), key=lambda kv: -kv[1])
    for message, count in failures[:5]:
        lines.append(f"| Does not compile: {message} | {count} |")
    return "\n".join(lines)


class CommandError(Exception):
    """A command of the benchmark failed."""


def run(command: list[str], cwd: Path | None = None, timeout: float = 900) -> str:
    """Runs a command and returns its standard output.

    Raises `CommandError` with the command and its standard error if the
    command fails, cannot start, or exceeds `timeout` seconds.
    """
    log.info("running %s", " ".join(command))
    shown = " ".join(command)
    try:
        result = subprocess.run(
            command, capture_output=True, text=True, cwd=cwd, check=False, timeout=timeout
        )
    except OSError as error:
        raise CommandError(f"cannot run `{shown}`: {error}") from error
    except subprocess.TimeoutExpired as error:
        raise CommandError(f"`{shown}` did not end within {timeout:.0f} s") from error
    if result.returncode != 0:
        stderr = result.stderr.strip() or "(no standard error)"
        raise CommandError(f"`{shown}` failed with exit code {result.returncode}:\n{stderr}")
    return result.stdout


def jsbench(directory: Path | None, runs: int) -> int:
    """Runs the benchmarks and prints the tables; 1 if a command fails."""
    try:
        return _jsbench(directory, runs)
    except CommandError as error:
        log.error("%s", error)
        return 1


def _jsbench(directory: Path | None, runs: int) -> int:
    """The benchmarks of `jsbench`."""
    root = paths.repo_root()
    node = shutil.which("node")
    cargo = ["cargo", "build", "--release", "-q", "-p", "swb-js"]
    run([*cargo, "--example", "vm_bench", "--bin", "swb-js"], cwd=root)
    vm_bench = root / "target" / "release" / "examples" / "vm_bench"
    swb_programs = parse_programs(run([str(vm_bench)]))
    node_programs: dict[str, float] = {}
    if node is None:
        log.warning("node not found; the node column stays empty")
    else:
        script = run([str(vm_bench), "--print-js"])
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "bench.js"
            file.write_text(script)
            node_programs = parse_programs(run([node, "--jitless", str(file)]))
    print(programs_table(swb_programs, node_programs))
    if directory is not None:
        run(["cargo", "build", "--release", "-q", "-p", "swb-js-syntax", "--examples"], cwd=root)
        lexing_out = run(
            [
                str(root / "target" / "release" / "examples" / "lex_files"),
                "--runs",
                str(runs),
                str(directory),
            ]
        )
        report = json.loads(
            run([str(root / "target" / "release" / "swb-js"), "bench-compile", str(directory)])
        )
        print()
        print(scripts_table(parse_lexing(lexing_out), report))
    return 0
