"""Command line: `uv run swbtools <command>`. See docs/testing.md."""

import argparse
import logging
import sys
from pathlib import Path

from swbtools import paths, swb
from swbtools.capture import capture, capture_missing, directory_size
from swbtools.compare import compare_fixture, update_scores
from swbtools.layout_refs import layout_refs
from swbtools.manifest import FILES_DIR, read_manifest
from swbtools.pages import list_fixtures, read_meta
from swbtools.perf import perf
from swbtools.pixels import DEFAULT_THRESHOLD
from swbtools.reference import reference
from swbtools.scoring import DEFAULT_TOLERANCE, Scores

log = logging.getLogger("swbtools")


def _names(args: argparse.Namespace) -> list[str] | None:
    """The fixture names from `NAME...` and `--all`; None if neither is given."""
    if args.all:
        return list_fixtures()
    return args.names or None


def cmd_capture(args: argparse.Namespace) -> int:
    return capture(args.url, args.name, args.with_swb, args.swb, args.force, args.system_fonts)


def cmd_capture_missing(args: argparse.Namespace) -> int:
    return capture_missing(args.name, args.swb, args.system_fonts)


def cmd_reference(args: argparse.Namespace) -> int:
    names = _names(args)
    if names is None:
        log.error("give fixture names or --all")
        return 2
    return reference(names, args.system_fonts)


def cmd_compare(args: argparse.Namespace) -> int:
    names = _names(args)
    if names is None:
        log.error("give fixture names or --all")
        return 2
    binary = None
    if not args.no_run:
        binary = swb.find_swb(args.swb)
        if binary is None:
            log.error("swb binary not found; build it (`just build`) or pass --swb PATH")
            return 1
    results: dict[str, Scores] = {}
    failed = False
    for name in names:
        scores = compare_fixture(name, binary, args.tolerance, args.threshold)
        if scores is None:
            failed = True
        else:
            results[name] = scores
    if args.update_scores and results:
        update_scores(paths.scores_file(), results)
    return 1 if failed else 0


def cmd_perf(args: argparse.Namespace) -> int:
    names = _names(args) or list_fixtures()
    return perf(names, args.swb, args.runs)


def cmd_layout_refs(args: argparse.Namespace) -> int:
    return layout_refs(args.names, args.system_fonts)


def cmd_list(args: argparse.Namespace) -> int:
    for name in list_fixtures():
        directory = paths.fixture_dir(name)
        entries = len(read_manifest(directory))
        size = directory_size(directory / FILES_DIR) / 1e6
        try:
            url = read_meta(directory).url
        except FileNotFoundError:
            url = "(no fixture.json)"
        print(f"{name:24} {entries:5} entries {size:8.2f} MB  {url}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="swbtools", description=__doc__)
    parser.add_argument(
        "-v", "--verbose", action="count", default=0, help="more log output (-vv for debug)"
    )
    commands = parser.add_subparsers(dest="command", required=True)

    fonts_help = "use the system fonts instead of fixtures/fonts"

    p = commands.add_parser("capture", help="download a page into fixtures/pages/NAME")
    p.add_argument("url")
    p.add_argument("name")
    group = p.add_mutually_exclusive_group()
    group.add_argument(
        "--with-swb",
        dest="with_swb",
        action="store_true",
        default=None,
        help="also record swb's requests (default: if the swb binary exists)",
    )
    group.add_argument("--no-swb", dest="with_swb", action="store_false")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.add_argument("--force", action="store_true", help="replace an existing fixture")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_capture)

    p = commands.add_parser(
        "capture-missing", help="add the responses that fixtures/pages/NAME does not have"
    )
    p.add_argument("name")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_capture_missing)

    p = commands.add_parser("reference", help="write Chromium's boxes and screenshot")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--all", action="store_true", help="all fixtures in fixtures/pages")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_reference)

    p = commands.add_parser("compare", help="run swb on fixtures and compare with Chromium")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--all", action="store_true", help="all fixtures in fixtures/pages")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.add_argument(
        "--no-run", action="store_true", help="do not run swb; use the output in out/compare/NAME"
    )
    p.add_argument(
        "--tolerance",
        type=float,
        default=DEFAULT_TOLERANCE,
        help=f"maximum difference in px (default {DEFAULT_TOLERANCE:g})",
    )
    p.add_argument(
        "--threshold",
        type=int,
        default=DEFAULT_THRESHOLD,
        help=f"maximum channel difference for equal pixels (default {DEFAULT_THRESHOLD})",
    )
    p.add_argument(
        "--update-scores", action="store_true", help="write the scores to fixtures/scores.json"
    )
    p.set_defaults(func=cmd_compare)

    p = commands.add_parser("layout-refs", help="write tests/layout/*.boxes.json with Chromium")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_layout_refs)

    p = commands.add_parser("perf", help="time swb's pipeline stages for fixtures")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--all", action="store_true", help="all fixtures (the default)")
    p.add_argument("--runs", type=int, default=20, help="runs per stage (default: 20)")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.set_defaults(func=cmd_perf)

    p = commands.add_parser("list", help="list the page fixtures and their sizes")
    p.set_defaults(func=cmd_list)
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    level = [logging.WARNING, logging.INFO, logging.DEBUG][min(args.verbose, 2)]
    logging.basicConfig(level=level, format="%(levelname)s %(name)s: %(message)s")
    sys.exit(args.func(args))
