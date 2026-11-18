"""Input traces for `swbtools gui-latency`: the format, the reader, and the
tools that make the traces in `tools/traces/`.

A trace is a JSON file:

    {
      "description": "text",
      "window": [1280, 935],
      "duration": 12.5,
      "events": [
        {"t": 0.016, "kind": "move", "x": 640, "y": 300},
        {"t": 0.5, "kind": "press", "button": 1},
        {"t": 0.58, "kind": "release", "button": 1},
        {"t": 1.0, "kind": "wheel", "notches": 1}
      ]
    }

- `t` is in seconds from the start of the trace, in ascending order. The
  tool starts a trace after the page loaded and has had its first frame.
- `kind` is `move` (the pointer goes to `x`, `y`), `press` or `release` (a
  mouse button, `button` 1 to 3, default 1; at the pointer) or `wheel` (one
  notch at the pointer: `notches` is 1 for down and -1 for up). Each event
  is one X event, so that the tool can count them in the frame log.
- `x`, `y` are window pixels: (0, 0) is the top left corner of the window,
  the toolbar included. The tool clamps them into the window it uses.
- `duration` is the length of the trace in seconds, at least the time of
  the last event (an idle trace has no events). `window` is the window
  size of the recording, or null for a synthetic trace; `description` says
  where the trace comes from.

`python tools/make_traces.py` makes the traces of the repository: the
pointer input of a Wayland recording (`WAYLAND_DEBUG=1`) and the synthetic
ones (`synthetic_traces`).
"""

import itertools
import json
import re
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from swbtools import paths

KINDS = ("move", "press", "release", "wheel")
"""The kinds of events."""


class TraceError(ValueError):
    """A trace file is not valid."""


@dataclass(frozen=True)
class Event:
    """One input event of a trace."""

    t: float
    kind: str
    x: int = 0
    y: int = 0
    button: int = 1
    notches: int = 0


@dataclass(frozen=True)
class Trace:
    """A trace: events in time order."""

    name: str
    description: str
    window: tuple[int, int] | None
    duration: float
    events: tuple[Event, ...]

    def count(self, kind: str) -> int:
        """The number of events of a kind."""
        return sum(1 for event in self.events if event.kind == kind)


def traces_dir() -> Path:
    """Returns `tools/traces/`."""
    return paths.repo_root() / "tools" / "traces"


def parse_trace(data: object, name: str) -> Trace:
    """Checks the JSON value of a trace file and returns the trace. Raises
    TraceError."""
    if not isinstance(data, dict):
        raise TraceError(f"{name}: not a JSON object")
    raw_events = data.get("events")
    if not isinstance(raw_events, list):
        raise TraceError(f"{name}: `events` is not a list")
    events = tuple(_parse_event(raw, name, index) for index, raw in enumerate(raw_events))
    for before, after in itertools.pairwise(events):
        if after.t < before.t:
            raise TraceError(f"{name}: the events are not in time order at t={after.t}")
    duration = data.get("duration")
    if not isinstance(duration, int | float) or isinstance(duration, bool) or duration < 0:
        raise TraceError(f"{name}: `duration` is not a number of seconds")
    if events and duration < events[-1].t:
        raise TraceError(f"{name}: `duration` is before the last event")
    window = data.get("window")
    if window is not None:
        if not (isinstance(window, list) and len(window) == 2 and all(_is_int(v) for v in window)):
            raise TraceError(f"{name}: `window` is not [width, height]")
        window = (window[0], window[1])
    return Trace(name, str(data.get("description", "")), window, float(duration), events)


def _is_int(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _parse_event(raw: object, name: str, index: int) -> Event:
    where = f"{name}: event {index}"
    if not isinstance(raw, dict):
        raise TraceError(f"{where}: not an object")
    t, kind = raw.get("t"), raw.get("kind")
    if not isinstance(t, int | float) or isinstance(t, bool) or t < 0:
        raise TraceError(f"{where}: `t` is not a time in seconds")
    if kind not in KINDS:
        raise TraceError(f"{where}: `kind` is not one of {', '.join(KINDS)}")
    if kind == "move":
        x, y = raw.get("x"), raw.get("y")
        if not (_is_int(x) and _is_int(y)):
            raise TraceError(f"{where}: a move needs integer `x` and `y`")
        return Event(float(t), kind, x=x, y=y)
    if kind == "wheel":
        if raw.get("notches") not in (1, -1):
            raise TraceError(f"{where}: `notches` of a wheel event is 1 or -1")
        return Event(float(t), kind, notches=raw["notches"])
    button = raw.get("button", 1)
    if button not in (1, 2, 3):
        raise TraceError(f"{where}: `button` is 1, 2 or 3")
    return Event(float(t), kind, button=button)


def load_trace(spec: str) -> Trace:
    """Loads a trace by file path, or by name from `tools/traces/` (`NAME` or
    `NAME.json`). Raises TraceError (also for a missing file)."""
    path = Path(spec)
    if not path.is_file():
        path = traces_dir() / (spec if spec.endswith(".json") else f"{spec}.json")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise TraceError(f"{spec}: {error}") from error
    return parse_trace(data, path.stem)


def list_traces() -> list[str]:
    """The names of the traces in `tools/traces/`."""
    return sorted(p.stem for p in traces_dir().glob("*.json"))


# ----- Making traces -----


def trace_json(
    description: str, window: tuple[int, int] | None, duration: float, events: Iterable[Event]
) -> str:
    """The text of a trace file: one event per line."""
    lines = []
    for event in events:
        fields = {"t": round(event.t, 3), "kind": event.kind}
        if event.kind == "move":
            fields.update(x=event.x, y=event.y)
        elif event.kind == "wheel":
            fields["notches"] = event.notches
        else:
            fields["button"] = event.button
        lines.append("    " + json.dumps(fields))
    size = json.dumps(list(window)) if window else "null"
    body = ",\n".join(lines)
    return (
        f'{{\n  "description": {json.dumps(description)},\n  "window": {size},\n'
        f'  "duration": {round(duration, 3)},\n  "events": [\n{body}\n  ]\n}}\n'
    )


_EVENT = re.compile(
    r"^([0-9.]+) \[[0-9.]+\] (?:\{[^}]*\} )?(?: -> )?([a-z_0-9]+)#(\d+)\.([a-z_0-9]+)\((.*)\)$"
)
_XDG_SURFACE = re.compile(r"get_xdg_surface\(new id xdg_surface#\d+, wl_surface#(\d+)\)")

LEFT_BUTTON = 272
"""The Linux code of the left mouse button."""

NOTCH = 10.0
"""The value of `wl_pointer.axis` for one wheel notch."""


def wayland_events(lines: Iterable[str]) -> tuple[list[Event], float]:
    """The pointer input of a `WAYLAND_DEBUG=1` recording over the window's
    main surface, with times in seconds from the first commit of that
    surface; and the time of that commit on the clock of the log.

    Each line starts with the time of `CLOCK_MONOTONIC` in seconds; the
    events of `wl_pointer` carry the compositor's time stamp in milliseconds
    on the same clock, which is used for `motion`, `axis` and `button`.
    `enter` has no time stamp: the time of its line is used. The main
    surface is the one that `get_xdg_surface` names; the pointer over
    another surface (the window decorations) gives no event. A wheel event
    of value 10 is one notch down (a multiple of it, that many notches);
    only the left button is kept.
    """
    main: str | None = None
    current: str | None = None
    first_commit: float | None = None
    raw: list[tuple[float, Event]] = []
    for line in lines:
        text = line.rstrip("\n")
        found = _XDG_SURFACE.search(text)
        if found:
            main = found.group(1)
            continue
        match = _EVENT.match(text)
        if not match:
            continue
        stamp, interface, object_id, method, argument = match.groups()
        if interface == "wl_surface" and method == "commit":
            if object_id == main and first_commit is None:
                first_commit = float(stamp)
            continue
        if interface != "wl_pointer":
            continue
        parts = [part.strip() for part in argument.split(",")]
        if method == "enter":
            current = parts[1].removeprefix("wl_surface#")
            if current == main:
                raw.append((float(stamp), _move(parts[2], parts[3])))
        elif method == "leave":
            current = None
        elif current == main and method == "motion":
            raw.append((int(parts[0]) / 1000, _move(parts[1], parts[2])))
        elif current == main and method == "axis" and parts[1] == "0" and float(parts[2]):
            value = float(parts[2])
            count = max(1, round(abs(value) / NOTCH))
            notches = 1 if value > 0 else -1
            raw.extend(
                (int(parts[0]) / 1000, Event(0, "wheel", notches=notches)) for _ in range(count)
            )
        elif current == main and method == "button" and int(parts[2]) == LEFT_BUTTON:
            kind = "press" if parts[3] == "1" else "release"
            raw.append((int(parts[1]) / 1000, Event(0, kind)))
    if first_commit is None:
        raise TraceError("the recording has no commit of the main surface")
    raw.sort(key=lambda item: item[0])
    events = [
        Event(round(t - first_commit, 3), e.kind, e.x, e.y, e.button, e.notches)
        for t, e in raw
        if t >= first_commit
    ]
    return events, first_commit


def _move(x: str, y: str) -> Event:
    return Event(0, "move", x=round(float(x)), y=round(float(y)))


OSM_MAP_WINDOW = (1280, 900)
"""The window of the `osm-map` trace. The window's y includes swb's toolbar
(40 px: `TOOLBAR_HEIGHT` in `crates/swb/src/gui/toolbar.rs`)."""

OSM_MAP_AREA = (360, 150, 1200, 840)
"""The part of the window (left, top, right, bottom) that the `osm-map` pointer
stays in: the map of openstreetmap.org, not the sidebar (350 px with the
welcome box), the header (55 px under the toolbar) or the controls (right
edge)."""

OSM_DRAGS = (
    ((900, 450), (-300, 0)),
    ((600, 450), (250, 200)),
    ((850, 650), (0, -300)),
    ((850, 350), (-200, 250)),
    ((650, 600), (350, -100)),
)
"""The five drags of `osm-map`: the start point and the vector, 200 to 400 px
in different directions."""


def osm_map_trace() -> tuple[list[Event], float]:
    """The events and the duration of the `osm-map` trace: five drags of the
    map (press, moves at 60 Hz over 0.6 s, a rest of 0.1 s, release; 0.8 s
    between drags), wheel notches in and out one at a time 400 ms apart, and
    one burst of five notches 30 ms apart, all with the pointer on the map."""
    events: list[Event] = []
    t = 0.5
    steps = 36
    for (x0, y0), (dx, dy) in OSM_DRAGS:
        events.append(Event(round(t, 3), "move", x=x0, y=y0))
        events.append(Event(round(t + 0.05, 3), "press", button=1))
        t += 0.05
        for i in range(1, steps + 1):
            at = round(t + i / 60, 3)
            events.append(
                Event(at, "move", x=x0 + round(dx * i / steps), y=y0 + round(dy * i / steps))
            )
        t += steps / 60 + 0.1
        events.append(Event(round(t, 3), "release", button=1))
        t += 0.8
    events.append(Event(round(t, 3), "move", x=800, y=450))
    t += 0.4
    for notches in (-1, -1, -1, 1, 1, 1):
        events.append(Event(round(t, 3), "wheel", notches=notches))
        t += 0.4
    t += 0.4
    for i in range(5):
        events.append(Event(round(t + i * 0.03, 3), "wheel", notches=-1))
    t += 0.12 + 1.5
    return events, round(t, 3)


def synthetic_traces() -> dict[str, tuple[str, tuple[int, int] | None, float, list[Event]]]:
    """The synthetic traces by name: (description, window, duration,
    events). All are for a window of at least 1280 x 900 pixels."""
    # A diagonal sweep over the page: 125 moves per second for 3 s, 4 px
    # across and 2 px down per move, wrapping inside the area x 50 to 1230,
    # y 100 to 700 (the pointer rate of a mouse).
    sweep = [
        Event(round(i / 125, 3), "move", x=50 + (i * 4) % 1180, y=100 + (i * 2) % 600)
        for i in range(375)
    ]
    # A wheel burst: the pointer to the middle of the page, 10 notches down
    # and 10 up, 30 ms apart (a fast wheel), a pause between.
    burst = [Event(0.0, "move", x=640, y=500)]
    burst += [Event(round(0.5 + i * 0.03, 3), "wheel", notches=1) for i in range(10)]
    burst += [Event(round(1.5 + i * 0.03, 3), "wheel", notches=-1) for i in range(10)]
    osm_events, osm_duration = osm_map_trace()
    return {
        "osm-map": (
            "synthetic: drags of the map of openstreetmap.org (five drags of 200 to 400 px, "
            "60 Hz moves, a rest before the release), then wheel zooms (three in, three out, "
            "400 ms apart, and a burst of five)",
            OSM_MAP_WINDOW,
            osm_duration,
            osm_events,
        ),
        "pointer-sweep": (
            "synthetic: a diagonal pointer sweep, 125 moves per second for 3 s",
            None,
            3.0,
            sweep,
        ),
        "wheel-burst": (
            "synthetic: the pointer to the page's middle, then 10 wheel notches down and "
            "10 up, 30 ms apart",
            None,
            2.5,
            burst,
        ),
        "idle-10s": ("synthetic: no input for 10 s", None, 10.0, []),
    }
