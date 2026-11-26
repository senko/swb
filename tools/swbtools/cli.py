"""Command line: `uv run swbtools <command>`. See docs/testing.md."""

import argparse
import json
import logging
import os
import sys
from pathlib import Path

from swbtools import paths, pw_private, swb
from swbtools.animation_survey import animation_cpu, animation_survey
from swbtools.capture import DEFAULT_MAX_RESPONSES, capture, capture_missing, directory_size
from swbtools.compare import Options, compare_fixture, update_scores
from swbtools.cssom_cases import write_cssom_cases
from swbtools.gui_latency import Options as GuiOptions
from swbtools.gui_latency import fixture_directory, gui_latency
from swbtools.hostile import hostile
from swbtools.js_intl_data import write_data as write_js_intl_data
from swbtools.js_survey import js_survey
from swbtools.js_time_zone_names import write_data as write_js_time_zone_names
from swbtools.js_unicode_tables import write_tables as write_js_unicode_tables
from swbtools.jsbench import jsbench
from swbtools.jsdiff import jsdiff
from swbtools.layout_refs import layout_refs
from swbtools.lazy_cases import write_lazy_cases
from swbtools.linebreak_tables import write_tables
from swbtools.linebreaks import linebreaks
from swbtools.manifest import FILES_DIR, read_manifest
from swbtools.measure import MEASUREMENTS, SLOW_MEASUREMENTS, measure
from swbtools.media_cpu import media_cpu
from swbtools.media_fixture import media_fixture
from swbtools.module_cases import SUBCOMMANDS as PROBE_NET_CASES
from swbtools.module_cases import write_cases
from swbtools.pages import list_fixtures, list_js_fixtures, read_meta
from swbtools.perf import perf
from swbtools.pixels import DEFAULT_THRESHOLD
from swbtools.probe import DEFAULT_TOLERANCE as PROBE_TOLERANCE
from swbtools.probe import probe
from swbtools.reference import reference, reference_js
from swbtools.replay import DEFAULT_PAUSE_MS, replay_command
from swbtools.scoring import DEFAULT_TOLERANCE, Scores
from swbtools.substitute import substitute
from swbtools.traces import TraceError, load_trace
from swbtools.web_cases import write_web_cases

log = logging.getLogger("swbtools")


def _names(args: argparse.Namespace) -> list[str] | None:
    """The fixture names from `NAME...` and `--all`; None if neither is given.
    With `--js`, `--all` means the fixtures of `fixtures/js/`."""
    if args.all:
        return list_js_fixtures() if getattr(args, "js", False) else list_fixtures()
    return args.names or None


def cmd_capture(args: argparse.Namespace) -> int:
    return capture(
        args.url,
        args.name,
        args.with_swb,
        args.swb,
        args.force,
        args.system_fonts,
        args.js,
        args.steps,
        args.max_responses,
        args.pause_ms,
    )


def cmd_capture_missing(args: argparse.Namespace) -> int:
    return capture_missing(args.name, args.swb, args.system_fonts)


def cmd_reference(args: argparse.Namespace) -> int:
    names = _names(args)
    if names is None:
        log.error("give fixture names or --all")
        return 2
    if args.js:
        return reference_js(names, args.system_fonts)
    return reference(names, args.system_fonts)


def cmd_compare(args: argparse.Namespace) -> int:
    names = _names(args)
    if names is None:
        log.error("give fixture names or --all")
        return 2
    options = Options(full_page=args.full_page, clicks=tuple(args.click), javascript=args.js)
    if args.js and (options.full_page or options.clicks):
        log.error("--js does not work with --full-page or --click")
        return 2
    if args.update_scores and (options.full_page or options.clicks):
        log.error("--update-scores works only with the plain comparison")
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
        scores = compare_fixture(name, binary, args.tolerance, args.threshold, options)
        if scores is None:
            failed = True
        else:
            results[name] = scores
    if args.update_scores and results:
        update_scores(paths.js_scores_file() if args.js else paths.scores_file(), results)
    return 1 if failed else 0


def cmd_perf(args: argparse.Namespace) -> int:
    names = _names(args) or (list_js_fixtures() if args.js else list_fixtures())
    return perf(names, args.swb, args.runs, args.js)


def cmd_gui_latency(args: argparse.Namespace) -> int:
    try:
        trace = load_trace(args.trace)
        directory = None
        if args.url is None:
            if args.name is None:
                raise ValueError("give a fixture name or --url")
            directory = fixture_directory(args.name, args.js, args.fixtures)
        elif args.name is not None:
            raise ValueError("give a fixture name or --url, not both")
        width, height = (int(v) for v in args.window.split("x"))
    except (TraceError, ValueError) as error:
        log.error("%s", error)
        return 2
    options = GuiOptions(
        trace=trace,
        fixture=directory,
        url=args.url,
        javascript=args.js,
        window=(width, height),
        settle_s=args.settle,
        idle_s=args.idle,
        swb_path=args.swb,
        label=args.url or f"{directory.parent.name}/{args.name}",  # type: ignore[union-attr]
        log_file=args.log,
        urls=args.urls,
    )
    return gui_latency(options, args.json)


def cmd_jsdiff(args: argparse.Namespace) -> int:
    return jsdiff(args.files, args.swb_js)


def cmd_jsbench(args: argparse.Namespace) -> int:
    return jsbench(args.directory, args.runs)


def cmd_hostile(args: argparse.Namespace) -> int:
    return hostile(args.names, args.swb, args.list, args.keep)


def cmd_layout_refs(args: argparse.Namespace) -> int:
    return layout_refs(args.names, args.system_fonts)


def cmd_measure(args: argparse.Namespace) -> int:
    unknown = [name for name in args.names if name not in MEASUREMENTS]
    if unknown:
        log.error(
            "unknown measurement: %s (known: %s)", ", ".join(unknown), ", ".join(MEASUREMENTS)
        )
        return 2
    return measure(args.names)


def cmd_probe(args: argparse.Namespace) -> int:
    binary = None
    if args.with_swb:
        binary = swb.find_swb(args.swb)
        if binary is None:
            log.error("swb binary not found; build it (`just build`) or pass --swb PATH")
            return 1
    return probe(
        args.files,
        args.case,
        binary,
        args.tolerance,
        args.json,
        args.chromium_arg,
        args.shots_dir,
    )


def cmd_linebreaks(args: argparse.Namespace) -> int:
    return linebreaks(args.out)


def cmd_linebreak_tables(args: argparse.Namespace) -> int:
    return write_tables()


def cmd_lazy_cases(args: argparse.Namespace) -> int:
    return write_lazy_cases(args.chromium)


def cmd_web_cases(args: argparse.Namespace) -> int:
    return write_web_cases(args.probe, args.chromium, args.rust)


def cmd_cssom_cases(args: argparse.Namespace) -> int:
    return write_cssom_cases(args.chromium)


def cmd_probe_net_cases(args: argparse.Namespace) -> int:
    return write_cases(args.probe_file, args.cases_file, args.chromium)


def cmd_js_unicode_tables(args: argparse.Namespace) -> int:
    return write_js_unicode_tables()


def cmd_js_time_zone_names(args: argparse.Namespace) -> int:
    return write_js_time_zone_names()


def cmd_js_intl_data(args: argparse.Namespace) -> int:
    return write_js_intl_data()


def cmd_substitute(args: argparse.Namespace) -> int:
    return substitute(args.names, args.check)


def cmd_js_survey(args: argparse.Namespace) -> int:
    names = list_js_fixtures() if args.all else args.names
    if not names:
        log.error("give fixture names or --all")
        return 2
    return js_survey(names, args.runs, args.system_fonts)


def cmd_animation_survey(args: argparse.Namespace) -> int:
    return animation_survey(args.names, args.js, Path(args.out))


def cmd_animation_cpu(args: argparse.Namespace) -> int:
    return animation_cpu(args.targets, args.js, args.scroll, args.seconds, Path(args.out))


def cmd_media_cpu(args: argparse.Namespace) -> int:
    width, _, height = args.window.partition("x")
    if not (width.isdigit() and height.isdigit()):
        log.error("--window: expected WIDTHxHEIGHT, got %r", args.window)
        return 2
    return media_cpu(args.files, (int(width), int(height)), args.seconds, Path(args.out))


def cmd_pw_private(args: argparse.Namespace) -> int:
    directory = Path(args.dir) if args.dir else pw_private.default_dir()
    try:
        match args.action:
            case "start":
                print(json.dumps(pw_private.start(directory), indent=2))
            case "stop":
                stopped = pw_private.stop(directory)
                print("stopped" if stopped else "no private daemon runs")
            case "status":
                print(json.dumps(pw_private.status(directory), indent=2))
            case "env":
                print("\n".join(pw_private.env_lines(pw_private.check_dir(directory))))
            case "watch":
                made = pw_private.watch(directory, args.seconds)
                print(f"{made} links made")
            case "record":
                if not args.out:
                    log.error("record needs --out FILE")
                    return 2
                return pw_private.record(directory, Path(args.out), args.seconds)
    except pw_private.IsolationError as e:
        log.error("isolation: %s", e)
        return 3
    return 0


def cmd_media_fixture(args: argparse.Namespace) -> int:
    return media_fixture(Path(args.fixture), args.url, Path(args.file), args.content_type)


def cmd_replay(args: argparse.Namespace) -> int:
    return replay_command(
        args.name,
        args.setup,
        args.steps,
        args.record,
        args.out,
        args.pause_ms,
        args.system_fonts,
        args.screenshots,
        args.pixels,
        args.pixels_out,
        args.init_script,
        args.console_out,
        args.missing_out,
        args.chromium_arg,
        args.animations == "on",
    )


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
    parser.add_argument(
        "--no-shared-worker",
        action="store_true",
        help="Chromium runs without window.SharedWorker, as swb does (js-survey, reference --js, "
        "compare --js); the references go to out/js-reference-legacy",
    )
    parser.add_argument(
        "--no-intl",
        action="store_true",
        help="Chromium runs without window.Intl, as swb does (js-survey, reference --js, "
        "compare --js); together with --no-shared-worker the page loads its polyfills",
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
    p.add_argument(
        "--js",
        action="store_true",
        help="run the page's scripts and write fixtures/js/NAME (local only, not committed); "
        "swb recording is off unless --with-swb",
    )
    p.add_argument(
        "--steps",
        type=Path,
        metavar="FILE",
        help="with --js: after the load, run the step groups of FILE (the format of "
        "`replay --steps`) in the live page and record every response of the steps too "
        "(a pause and a capped network-idle wait after each group)",
    )
    p.add_argument(
        "--max-responses",
        type=int,
        metavar="N",
        help="stop fetching after N responses from the network: new requests fail and a "
        f"warning is logged (default with --steps: {DEFAULT_MAX_RESPONSES}; none without)",
    )
    p.add_argument(
        "--pause-ms",
        type=int,
        default=DEFAULT_PAUSE_MS,
        help=f"with --steps: pause after each step (default {DEFAULT_PAUSE_MS})",
    )
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
    p.add_argument(
        "--js",
        action="store_true",
        help="JavaScript on: the fixtures of fixtures/js (local only); the output goes to "
        "out/js-reference/NAME/ and --all means all of them",
    )
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
        "--update-scores",
        action="store_true",
        help="write the scores to fixtures/scores.json (with --js: out/js-scores.json)",
    )
    p.add_argument(
        "--js",
        action="store_true",
        help="compare the fixtures of fixtures/js with Chromium's JavaScript-on output in "
        "out/js-reference (`reference --js`); swb gets --js if it has the option; the output "
        "is in out/compare/js-NAME",
    )
    p.add_argument(
        "--full-page",
        action="store_true",
        help="also compare the full-page screenshots (live Chromium capture) and list the "
        "differing regions",
    )
    p.add_argument(
        "--click",
        action="append",
        default=[],
        metavar="SELECTOR",
        help="click the first element that matches SELECTOR in both browsers before the "
        "comparison (repeatable; applied in order). Chromium's boxes and screenshots are then a "
        "live capture, and the output files get a state suffix",
    )
    p.set_defaults(func=cmd_compare)

    p = commands.add_parser(
        "hostile", help="run swb on the hostile-page set and check time and memory limits"
    )
    p.add_argument("names", nargs="*", metavar="NAME", help="cases to run (default: all)")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.add_argument("--list", action="store_true", help="list the cases and their limits")
    p.add_argument("--keep", action="store_true", help="keep out/hostile/NAME of passing cases")
    p.set_defaults(func=cmd_hostile)

    p = commands.add_parser(
        "jsdiff",
        help="run JavaScript files in swb-js and Node.js and diff stdout and the uncaught error",
    )
    p.add_argument("files", nargs="+", type=Path, metavar="FILE")
    p.add_argument("--swb-js", type=Path, help="path of swb-js (default: $SWB_JS, target/release)")
    p.set_defaults(func=cmd_jsdiff)

    p = commands.add_parser(
        "jsbench",
        help="benchmark the JavaScript engine against node --jitless (docs/performance.md)",
    )
    p.add_argument(
        "directory", nargs="?", type=Path, help="a directory of scripts to lex, parse and compile"
    )
    p.add_argument("--runs", type=int, default=5, help="runs of the lexer measurement (default 5)")
    p.set_defaults(func=cmd_jsbench)

    p = commands.add_parser("layout-refs", help="write tests/layout/*.boxes.json with Chromium")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_layout_refs)

    p = commands.add_parser("perf", help="time swb's pipeline stages for fixtures")
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--all", action="store_true", help="all fixtures (the default)")
    p.add_argument("--runs", type=int, default=20, help="runs per stage (default: 20)")
    p.add_argument(
        "--js",
        action="store_true",
        help="the fixtures of fixtures/js/, with JavaScript on and the fixture's clock",
    )
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.set_defaults(func=cmd_perf)

    p = commands.add_parser(
        "gui-latency",
        help="play an input trace into the GUI on a private Xvfb and report the input latency",
    )
    p.add_argument("name", nargs="?", metavar="NAME", help="a fixture (see --fixtures)")
    p.add_argument("--url", help="a live URL instead of a fixture")
    p.add_argument(
        "--fixtures",
        choices=["pages", "js"],
        help="where NAME is (default: fixtures/js with --js, else fixtures/pages)",
    )
    p.add_argument("--js", action="store_true", help="JavaScript on (with the fixture's clock)")
    p.add_argument(
        "--trace",
        default="pointer-sweep",
        help="a trace file or the name of one in tools/traces/ (default: pointer-sweep)",
    )
    p.add_argument(
        "--window", default="1280x900", help="window size, WIDTHxHEIGHT (default 1280x900)"
    )
    p.add_argument("--settle", type=float, default=1.0, help="seconds after the load (default 1)")
    p.add_argument("--idle", type=float, default=3.0, help="idle seconds measured after the trace")
    p.add_argument("--swb", type=Path, help="path of the swb binary")
    p.add_argument("--json", action="store_true", help="print the report as JSON")
    p.add_argument(
        "--log",
        type=Path,
        metavar="FILE",
        help="write the GUI's stderr (the frame log and the pipeline log) to FILE",
    )
    p.add_argument(
        "--urls",
        action="store_true",
        help="also read the page URL before the trace, 0.4 s after each release and after "
        "the trace (the reads can change the timing: use a separate run)",
    )
    p.set_defaults(func=cmd_gui_latency)

    p = commands.add_parser(
        "measure", help="measure the Chromium behaviour that swb's data comes from"
    )
    p.add_argument(
        "names",
        nargs="*",
        metavar="NAME",
        help=(
            f"{', '.join(MEASUREMENTS)} "
            f"(default: all but {', '.join(SLOW_MEASUREMENTS)}, which takes minutes)"
        ),
    )
    p.set_defaults(func=cmd_measure)

    p = commands.add_parser(
        "probe",
        help="ask Chromium (and swb) about the cases of JSON files (see docs/testing.md)",
        description=(
            "Renders each case in Chromium and prints boxes, line fragments (rects), "
            "computed styles or JS values. With --with-swb, boxes are also compared with swb "
            "(rects, style and js are Chromium only); exit status 1 if a box differs by "
            "more than the tolerance."
        ),
    )
    p.add_argument("files", nargs="+", type=Path, metavar="FILE", help="case files (JSON)")
    p.add_argument(
        "--case", action="append", default=[], metavar="NAME", help="run only this case (repeat)"
    )
    p.add_argument(
        "--with-swb", action="store_true", help="also render in swb and compare the boxes"
    )
    p.add_argument("--swb", type=Path, help="path of the swb binary (default: $SWB, target/)")
    p.add_argument(
        "--tolerance",
        type=float,
        default=PROBE_TOLERANCE,
        help=f"maximum box difference in px (default {PROBE_TOLERANCE:g})",
    )
    p.add_argument("--json", action="store_true", help="print JSON instead of text")
    p.add_argument(
        "--chromium-arg",
        action="append",
        default=[],
        metavar="ARG",
        help="add a flag to Chromium's launch (repeatable; write --chromium-arg=--flag), "
        "for example --chromium-arg=--autoplay-policy=document-user-activation-required",
    )
    p.add_argument(
        "--shots-dir",
        type=Path,
        metavar="DIR",
        help='where the {"shot": NAME} steps write CASE-NAME.png (default out/probe-shots/)',
    )
    p.set_defaults(func=cmd_probe)

    p = commands.add_parser(
        "lazy-cases",
        help="write crates/automation/tests/lazy_loading/cases.rs from web-lazy-loading.json",
    )
    p.add_argument(
        "chromium",
        type=Path,
        help="the output of `probe --json tools/probes/web-lazy-loading.json`",
    )
    p.set_defaults(func=cmd_lazy_cases)

    p = commands.add_parser(
        "web-cases",
        help="write the Rust cases of a probe file for crates/web/tests",
    )
    p.add_argument("probe", type=Path, help="the probe file, relative to the repository root")
    p.add_argument("chromium", type=Path, help="the output of `probe --json PROBE`")
    p.add_argument(
        "rust", type=Path, help="the Rust file to write, relative to the repository root"
    )
    p.set_defaults(func=cmd_web_cases)

    p = commands.add_parser(
        "cssom-cases",
        help="write crates/engine/tests/cssom_sheets/cases.rs from web-cssom-sheets.json",
    )
    p.add_argument(
        "chromium",
        type=Path,
        help="the output of `probe --json tools/probes/web-cssom-sheets.json`",
    )
    p.set_defaults(func=cmd_cssom_cases)

    for name, (probe_file, cases_file) in PROBE_NET_CASES.items():
        p = commands.add_parser(
            name,
            help=f"write {cases_file.as_posix()} from {probe_file.name}",
        )
        p.add_argument(
            "chromium",
            type=Path,
            help=f"the output of `probe --json {probe_file.as_posix()}`",
        )
        p.set_defaults(func=cmd_probe_net_cases, probe_file=probe_file, cases_file=cases_file)

    p = commands.add_parser(
        "linebreaks", help="measure Chromium's line break opportunities (text crate test data)"
    )
    p.add_argument(
        "--out", type=Path, help="output directory (default: crates/text/tests/linebreak)"
    )
    p.set_defaults(func=cmd_linebreaks)

    p = commands.add_parser(
        "linebreak-tables", help="write crates/text/src/linebreak/tables.rs (UCD, measurements)"
    )
    p.set_defaults(func=cmd_linebreak_tables)

    p = commands.add_parser(
        "js-unicode-tables", help="write crates/js-text/src/unicode/tables.rs (UCD)"
    )
    p.set_defaults(func=cmd_js_unicode_tables)

    p = commands.add_parser(
        "js-time-zone-names",
        help="write crates/js/src/date/zone_names_data.rs (CLDR time zone names)",
    )
    p.set_defaults(func=cmd_js_time_zone_names)

    p = commands.add_parser(
        "js-intl-data",
        help="write crates/js/src/intl/data.rs (CLDR locale data of Intl: en-US, en-GB)",
    )
    p.set_defaults(func=cmd_js_intl_data)

    p = commands.add_parser(
        "substitute",
        help="replace images and non-free fonts of fixtures/pages/NAME (public repository)",
    )
    p.add_argument("names", nargs="+", metavar="NAME")
    p.add_argument(
        "--check", action="store_true", help="only report content that may not be published"
    )
    p.set_defaults(func=cmd_substitute)

    p = commands.add_parser(
        "js-survey",
        help="measure what the scripts of fixtures/js/NAME do in Chromium (docs/js-survey.md)",
    )
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--all", action="store_true", help="all fixtures in fixtures/js")
    p.add_argument("--runs", type=int, default=3, help="runs of the timing measurement (default 3)")
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_js_survey)

    p = commands.add_parser(
        "animation-survey",
        help="count the CSS animations and transitions that run in the first view in Chromium",
        description=(
            "Loads fixtures (fixtures/pages, or fixtures/js with --js) as `reference` does but "
            "without turning animations off, lists document.getAnimations() after the settle "
            "time and counts the elements that declare animations and transitions. All fixtures "
            "if no NAME. Writes out/animations/NAME.json."
        ),
    )
    p.add_argument("names", nargs="*", metavar="NAME")
    p.add_argument("--js", action="store_true", help="JavaScript on: the fixtures of fixtures/js")
    p.add_argument(
        "--out", default="out/animations", help="output directory (default out/animations)"
    )
    p.set_defaults(func=cmd_animation_survey)

    p = commands.add_parser(
        "animation-cpu",
        help="measure the CPU use of Chromium while only endless animations run",
        description=(
            "Loads each target (the synthetic pages spinner, marquee and idle, or a fixture "
            "of fixtures/js with --js, else of fixtures/pages; scripts run in all cases), "
            "scrolls to --scroll, waits 2 s "
            "and measures the CPU time of the Chromium processes over --seconds (CDP "
            "SystemInfo.getProcessInfo), as a share of one core. Writes out/animations/cpu.json."
        ),
    )
    p.add_argument("targets", nargs="+", metavar="TARGET")
    p.add_argument("--js", action="store_true", help="fixtures of fixtures/js")
    p.add_argument("--scroll", type=int, default=0, help="scroll position in px (default 0)")
    p.add_argument("--seconds", type=float, default=10.0, help="measured time (default 10)")
    p.add_argument("--out", default="out/animations", help="output directory")
    p.set_defaults(func=cmd_animation_cpu)

    p = commands.add_parser(
        "media-cpu",
        help="measure the CPU use and the memory of Chromium while it plays a media file",
        description=(
            "Plays each file muted in a <video> that fills the window and measures, over "
            "--seconds each, the playing, paused and ended phases: CPU time of the Chromium "
            "processes by type (share of one core), resident memory, media time against wall "
            "time and the frames decoded and dropped (docs/media-diagnosis.md section 6)."
        ),
    )
    p.add_argument("files", nargs="+", type=Path, metavar="FILE")
    p.add_argument("--window", default="1280x720", help="window size (default 1280x720)")
    p.add_argument("--seconds", type=float, default=10.0, help="time per phase (default 10)")
    p.add_argument("--out", default="out/media/cpu.json", help="output file")
    p.set_defaults(func=cmd_media_cpu)

    p = commands.add_parser(
        "pw-private",
        help="start and stop a private PipeWire daemon (a null sink, no device) for audio runs",
        description=(
            "A PipeWire daemon of its own for swb's audio measurements (docs/testing.md, "
            "'Audio output'): its own runtime directory (tmp/pw-private/), a configuration file "
            "of its own, no session manager and no device, one null sink. `start` proves the "
            "isolation first (pw-cli info 0 fails without the daemon; pw-dump shows no "
            "alsa/bluez5/v4l2 property and one Audio/Sink) and prints the proof. `env` prints "
            "the two variables for swb. `watch` links swb's stream to the sink (and the monitor "
            "to the recorder); `record` writes the monitor to a WAV file. Never use the owner's "
            "daemon."
        ),
    )
    p.add_argument("action", choices=["start", "stop", "status", "env", "watch", "record"])
    p.add_argument("--dir", help="the private runtime directory (default tmp/pw-private)")
    p.add_argument(
        "--seconds",
        type=float,
        default=0.0,
        help="watch: how long (0: until SIGTERM); record: how long",
    )
    p.add_argument("--out", help="record: the WAV file (32-bit float, stereo, 48 kHz)")
    p.set_defaults(func=cmd_pw_private)

    p = commands.add_parser(
        "media-fixture",
        help="record a media file as one whole response in a fixture directory",
        description=(
            "Adds FILE as the response (status 200, the whole body) to GET URL in the fixture "
            "directory FIXTURE, creating it if needed. `capture` cannot record media: the "
            "page's Range requests get partial responses. swb's replay answers Range requests "
            "from the whole body (206, 416) and reads a body file over 8 MiB at an offset "
            "(docs/media-diagnosis.md section 5)."
        ),
    )
    p.add_argument("fixture", metavar="FIXTURE")
    p.add_argument("url", metavar="URL")
    p.add_argument("file", metavar="FILE")
    p.add_argument(
        "--content-type", default="video/mp4", help="the stored content type (default video/mp4)"
    )
    p.set_defaults(func=cmd_media_fixture)

    p = commands.add_parser(
        "replay",
        help="replay fixtures/js/NAME in Chromium, run step groups, record the page after each",
        description=(
            "Loads the fixture as `reference --js` does, evaluates the setup script, then "
            "evaluates the record expression after the load and after each step group, and "
            "writes the records as a JSON list (docs/testing.md, 'Automation tests'). The "
            "global options --no-shared-worker and --no-intl apply."
        ),
    )
    p.add_argument("name", metavar="NAME", help="a fixture of fixtures/js")
    p.add_argument(
        "--steps",
        type=Path,
        required=True,
        help="JSON file: a list of step groups (a list of steps, or {name, steps}); the steps "
        "are those of `swb --steps` and of the probe tool",
    )
    p.add_argument(
        "--record",
        required=True,
        metavar="EXPR",
        help="expression that returns the record (a string is parsed as JSON)",
    )
    p.add_argument(
        "--setup", type=Path, help="script to evaluate after the load, before the first record"
    )
    p.add_argument("--out", type=Path, help="write the records here (default: stdout)")
    p.add_argument(
        "--pause-ms",
        type=int,
        default=DEFAULT_PAUSE_MS,
        help=f"pause after each step (default {DEFAULT_PAUSE_MS})",
    )
    p.add_argument(
        "--screenshots",
        type=Path,
        metavar="DIR",
        help="save a screenshot of the viewport after the load and after each group here",
    )
    p.add_argument(
        "--pixels",
        type=Path,
        metavar="FILE",
        help='JSON {"groups": [names or "load"], "points": {name: [x, y]}}: read the colors at '
        "the points after these groups (needs --pixels-out)",
    )
    p.add_argument(
        "--pixels-out",
        type=Path,
        metavar="FILE",
        help="write {group: {point: [r, g, b]}} here (needs --pixels)",
    )
    p.add_argument(
        "--init-script",
        type=Path,
        metavar="FILE",
        help="script that runs in every frame before the page's scripts (add_init_script)",
    )
    p.add_argument(
        "--console-out",
        type=Path,
        metavar="FILE",
        help="write the console of all frames here, one line per message: type, url:line, text",
    )
    p.add_argument(
        "--missing-out",
        type=Path,
        metavar="FILE",
        help="write the URLs that the fixture lacks per record here: "
        '[{"label": "load" or the group name, "missing": [url, ...]}, ...]',
    )
    p.add_argument(
        "--chromium-arg",
        action="append",
        default=[],
        metavar="ARG",
        help="add a flag to Chromium's launch (repeatable; write --chromium-arg=--flag for a "
        "flag that starts with --)",
    )
    p.add_argument(
        "--animations",
        choices=["on", "off"],
        default="off",
        help="off (default): inject the kill sheet (animation: none; transition: none) after the "
        "load and take screenshots with animations disabled; on: leave the page's animations and "
        "transitions running and take screenshots with animations allowed",
    )
    p.add_argument("--system-fonts", action="store_true", help=fonts_help)
    p.set_defaults(func=cmd_replay)

    p = commands.add_parser("list", help="list the page fixtures and their sizes")
    p.set_defaults(func=cmd_list)
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    if args.no_shared_worker:
        os.environ[paths.NO_SHARED_WORKER_ENV] = "1"
    if args.no_intl:
        os.environ[paths.NO_INTL_ENV] = "1"
    level = [logging.WARNING, logging.INFO, logging.DEBUG][min(args.verbose, 2)]
    logging.basicConfig(level=level, format="%(levelname)s %(name)s: %(message)s")
    sys.exit(args.func(args))
