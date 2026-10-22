"""Running the swb binary.

All swb command lines are built here, so that a change of swb's flags needs
a change in this file only.
"""

import contextlib
import logging
import os
import re
import subprocess
from collections.abc import Iterator
from pathlib import Path
from typing import IO

from swbtools import paths

log = logging.getLogger(__name__)

SWB_TIMEOUT_S = 300
"""Upper limit for one swb run (the page load timeout is swb's `--timeout`)."""


def find_swb(explicit: Path | None = None) -> Path | None:
    """Returns the swb binary: `explicit`, the `SWB` environment variable,
    `target/release/swb` or `target/debug/swb`, in that order. Returns None
    if none exists."""
    if explicit is not None:
        return explicit if explicit.is_file() else None
    from_env = os.environ.get("SWB")
    if from_env:
        return Path(from_env) if Path(from_env).is_file() else None
    target = paths.repo_root() / "target"
    for candidate in (target / "release" / "swb", target / "debug" / "swb"):
        if candidate.is_file():
            return candidate
    return None


_FLAGS_CACHE: dict[tuple[str, int, int], frozenset[str]] = {}


def supported_flags(swb: Path) -> frozenset[str]:
    """The long options that `swb --help` lists (for example `--js`), so
    that the tools can use options that a swb build does not have yet. The
    answer is cached per binary (path, size and modification time). An
    unusable binary has no flags."""
    try:
        stat = swb.stat()
    except OSError:
        return frozenset()
    key = (str(swb), stat.st_size, stat.st_mtime_ns)
    if key not in _FLAGS_CACHE:
        try:
            result = subprocess.run(
                [str(swb), "--help"], capture_output=True, text=True, timeout=30, check=False
            )
            text = result.stdout + result.stderr
        except (OSError, subprocess.SubprocessError) as error:
            log.warning("%s --help failed: %s", swb, error)
            text = ""
        _FLAGS_CACHE[key] = frozenset(re.findall(r"(?<![\w-])--[a-z][a-z0-9-]*", text))
    return _FLAGS_CACHE[key]


def record_command(swb: Path, fixture: Path, url: str) -> list[str]:
    """Command line that loads `url` from the network and records every
    response into the fixture directory (merging with its manifest)."""
    return [str(swb), "--headless", "--record", str(fixture), url]


def record_missing_command(swb: Path, fixture: Path, url: str) -> list[str]:
    """Command line that loads `url` from the fixture and fetches only the
    responses that the fixture does not have, adding them to it."""
    return [str(swb), "--headless", "--record-missing", str(fixture), url]


def _replay_options(swb: Path, fixture: Path, viewport: tuple[int, int]) -> list[str]:
    """The start of a command line that serves all requests from the fixture
    and uses the test fonts and the viewport size. `compare` and `perf` use
    the same settings."""
    width, height = viewport
    return [
        str(swb),
        "--headless",
        "--replay",
        str(fixture),
        "--test-fonts",
        "--size",
        f"{width}x{height}",
    ]


def replay_command(
    swb: Path,
    fixture: Path,
    url: str,
    viewport: tuple[int, int],
    boxes: Path,
    screenshot: Path,
    extra: list[str] | None = None,
) -> list[str]:
    """Command line that loads `url` from the fixture with the test fonts and
    writes a box dump and a first-viewport screenshot. `extra` are
    additional swb options, for example `--js`."""
    return [
        *_replay_options(swb, fixture, viewport),
        "--dump-boxes",
        str(boxes),
        "--screenshot",
        str(screenshot),
        *(extra or []),
        url,
    ]


def full_page_command(
    swb: Path, fixture: Path, url: str, viewport: tuple[int, int], screenshot: Path
) -> list[str]:
    """Command line that loads `url` from the fixture with the test fonts and
    writes a full-page screenshot."""
    return [
        *_replay_options(swb, fixture, viewport),
        "--full-page",
        "--screenshot",
        str(screenshot),
        url,
    ]


def probe_command(
    swb: Path,
    url: str,
    viewport: tuple[int, int],
    boxes: Path,
    screenshot: Path | None = None,
    extra: list[str] | None = None,
) -> list[str]:
    """Command line that loads `url` (a local file) with the test fonts and
    writes a box dump and, if `screenshot` is given, a full-page
    screenshot. `extra` are additional swb options (`--js`, `--eval EXPR`).
    `probe` uses it."""
    width, height = viewport
    shot = ["--full-page", "--screenshot", str(screenshot)] if screenshot is not None else []
    return [
        str(swb),
        "--headless",
        "--test-fonts",
        "--size",
        f"{width}x{height}",
        "--dump-boxes",
        str(boxes),
        *shot,
        *(extra or []),
        url,
    ]


def hostile_command(
    swb: Path,
    url: str,
    viewport: tuple[int, int],
    timeout_s: int,
    boxes: Path,
    screenshot: Path,
    extra: list[str] | None = None,
) -> list[str]:
    """Command line that loads a hostile page (`url`, normally `file://`)
    with the test fonts, a page load timeout of `timeout_s` and the whole
    pipeline, including the box dump and the screenshot. `extra` are
    additional swb options, for example `--full-page`."""
    width, height = viewport
    return [
        str(swb),
        "--headless",
        "--test-fonts",
        "--size",
        f"{width}x{height}",
        "--timeout",
        str(timeout_s),
        "--dump-boxes",
        str(boxes),
        "--screenshot",
        str(screenshot),
        *(extra or []),
        url,
    ]


def serve_command(
    swb: Path,
    viewport: tuple[int, int],
    fixture: Path | None = None,
    test_fonts: bool = True,
) -> list[str]:
    """Command line that starts a headless swb with the automation server on
    a free port (swb prints the address on its first output line). With
    `fixture`, all requests are served from it."""
    width, height = viewport
    command = [str(swb), "--headless", "--remote-port", "0", "--size", f"{width}x{height}"]
    if fixture is not None:
        command += ["--replay", str(fixture)]
    if test_fonts:
        command.append("--test-fonts")
    return command


def bench_command(
    swb: Path, fixture: Path, url: str, viewport: tuple[int, int], runs: int
) -> list[str]:
    """Command line that loads `url` from the fixture with the test fonts and
    prints the time of each pipeline stage as JSON (`--bench`)."""
    return [*_replay_options(swb, fixture, viewport), "--bench", str(runs), url]


def run(command: list[str], log_file: Path | None = None, stdout_file: Path | None = None) -> int:
    """Runs swb and returns its exit status. stderr (swb's log) goes to
    `log_file` if given, otherwise to this process's stderr. With
    `stdout_file`, stdout goes there and only stderr goes to `log_file`."""
    log.info("running %s", " ".join(command))
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")
    try:
        if log_file is None:
            result = subprocess.run(command, env=env, timeout=SWB_TIMEOUT_S, check=False)
        else:
            log_file.parent.mkdir(parents=True, exist_ok=True)
            with log_file.open("wb") as output, _stdout(stdout_file, output) as out:
                result = subprocess.run(
                    command,
                    env=env,
                    stdout=out,
                    stderr=output,
                    timeout=SWB_TIMEOUT_S,
                    check=False,
                )
    except subprocess.TimeoutExpired:
        log.error("swb did not finish in %d s", SWB_TIMEOUT_S)
        return -1
    return result.returncode


@contextlib.contextmanager
def _stdout(stdout_file: Path | None, shared: IO[bytes]) -> Iterator[IO[bytes]]:
    """The stdout target of `run`: `stdout_file` if given, else `shared` (the
    log file, which also receives stderr)."""
    if stdout_file is None:
        yield shared
        return
    with stdout_file.open("wb") as handle:
        yield handle
