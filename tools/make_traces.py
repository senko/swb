"""Makes the input traces of `tools/traces/` (format: `swbtools/traces.py`).

    uv run --project tools python tools/make_traces.py synthetic
    uv run --project tools python tools/make_traces.py wayland LOG NAME \\
        --description TEXT [--until SECONDS]

`synthetic` writes `pointer-sweep`, `wheel-burst` and `idle-10s`.
`wayland` reads the output of a `WAYLAND_DEBUG=1` run of the GUI, one line
per message, each line prefixed with the time of `CLOCK_MONOTONIC` in
seconds (the owner's recordings of M11), and writes the pointer input over
the window (moves, wheel notches, presses and releases of the left button)
with times relative to the first commit of the window's surface; `--until`
drops the events after that time. The repository has no Wayland logs: the
traces `github-owner` and `ars-owner` came from this script (see
docs/testing.md).
"""

import argparse
import sys
from pathlib import Path

from swbtools import traces


def write(
    name: str,
    description: str,
    window: tuple[int, int] | None,
    duration: float,
    events: list[traces.Event],
) -> None:
    """Writes `tools/traces/NAME.json` and checks that it reads back."""
    path = traces.traces_dir() / f"{name}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(traces.trace_json(description, window, duration, events), encoding="utf-8")
    trace = traces.load_trace(str(path))
    print(f"{path}: {len(trace.events)} events, {trace.duration} s")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("synthetic", help="write the synthetic traces")
    p = commands.add_parser("wayland", help="extract a trace from a Wayland log")
    p.add_argument("log", type=Path)
    p.add_argument("name")
    p.add_argument("--description", required=True)
    p.add_argument("--until", type=float, help="drop the events after this time (s)")
    p.add_argument("--window", help="the window size of the recording, WIDTHxHEIGHT")
    args = parser.parse_args()
    if args.command == "synthetic":
        for name, (description, window, duration, events) in traces.synthetic_traces().items():
            write(name, description, window, duration, events)
        return 0
    with args.log.open(encoding="utf-8", errors="replace") as lines:
        events, _ = traces.wayland_events(lines)
    if args.until is not None:
        events = [e for e in events if e.t <= args.until]
    window = tuple(int(v) for v in args.window.split("x")) if args.window else None
    duration = args.until if args.until is not None else (events[-1].t if events else 0.0)
    write(args.name, args.description, window, duration, events)
    return 0


if __name__ == "__main__":
    sys.exit(main())
