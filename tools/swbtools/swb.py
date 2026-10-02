"""Running the swb binary.

All swb command lines are built here, so that a change of swb's flags needs
a change in this file only.
"""

import logging
import os
import subprocess
from pathlib import Path

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


def record_command(swb: Path, fixture: Path, url: str) -> list[str]:
    """Command line that loads `url` from the network and records every
    response into the fixture directory (merging with its manifest)."""
    return [str(swb), "--headless", "--record", str(fixture), url]


def replay_command(
    swb: Path,
    fixture: Path,
    url: str,
    viewport: tuple[int, int],
    boxes: Path,
    screenshot: Path,
) -> list[str]:
    """Command line that loads `url` from the fixture with the test fonts and
    writes a box dump and a first-viewport screenshot."""
    width, height = viewport
    return [
        str(swb),
        "--headless",
        "--replay",
        str(fixture),
        "--test-fonts",
        "--size",
        f"{width}x{height}",
        "--dump-boxes",
        str(boxes),
        "--screenshot",
        str(screenshot),
        url,
    ]


def run(command: list[str], log_file: Path | None = None) -> int:
    """Runs swb and returns its exit status. stderr (swb's log) goes to
    `log_file` if given, otherwise to this process's stderr."""
    log.info("running %s", " ".join(command))
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")
    try:
        if log_file is None:
            result = subprocess.run(command, env=env, timeout=SWB_TIMEOUT_S, check=False)
        else:
            log_file.parent.mkdir(parents=True, exist_ok=True)
            with log_file.open("wb") as output:
                result = subprocess.run(
                    command,
                    env=env,
                    stdout=output,
                    stderr=subprocess.STDOUT,
                    timeout=SWB_TIMEOUT_S,
                    check=False,
                )
    except subprocess.TimeoutExpired:
        log.error("swb did not finish in %d s", SWB_TIMEOUT_S)
        return -1
    return result.returncode
