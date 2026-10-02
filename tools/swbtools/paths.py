"""Locations of repository files that the tools read and write."""

import os
from pathlib import Path


def _is_repo_root(path: Path) -> bool:
    return (path / "Cargo.toml").is_file() and (path / "tools" / "pyproject.toml").is_file()


def repo_root() -> Path:
    """Returns the root of the swb repository.

    The environment variable `SWB_ROOT` overrides the search. Otherwise the
    search starts at this file (editable install), then at the current
    directory, and goes up until it finds `Cargo.toml` and
    `tools/pyproject.toml`.
    """
    override = os.environ.get("SWB_ROOT")
    if override:
        return Path(override).resolve()
    for start in (Path(__file__).resolve(), Path.cwd().resolve()):
        for candidate in (start, *start.parents):
            if _is_repo_root(candidate):
                return candidate
    raise RuntimeError("cannot find the swb repository root; set SWB_ROOT")


def fixtures_dir() -> Path:
    """Returns `fixtures/`."""
    return repo_root() / "fixtures"


def pages_dir() -> Path:
    """Returns `fixtures/pages/`, which holds one directory per page fixture."""
    return fixtures_dir() / "pages"


def fixture_dir(name: str) -> Path:
    """Returns the directory of the page fixture `name`."""
    if not name or "/" in name or name.startswith("."):
        raise ValueError(f"invalid fixture name {name!r}")
    return pages_dir() / name


def fonts_conf() -> Path:
    """Returns the fontconfig file with the bundled test fonts."""
    return fixtures_dir() / "fonts" / "fonts.conf"


def scores_file() -> Path:
    """Returns `fixtures/scores.json`."""
    return fixtures_dir() / "scores.json"


def layout_tests_dir() -> Path:
    """Returns `tests/layout/`."""
    return repo_root() / "tests" / "layout"


def out_dir() -> Path:
    """Returns `out/`, the directory for generated output (not in git)."""
    return repo_root() / "out"
